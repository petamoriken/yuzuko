//! Replay one command list onto vello's CPU or GPU renderer.

use std::cell::RefCell;
use std::sync::{Arc, OnceLock};

use kurbo::Affine;
use kurbo::Rect;
use kurbo::Shape;
use vello::peniko::BlendMode;
use vello::peniko::Brush;
use vello::peniko::Compose;
use vello::peniko::Extend;
use vello::peniko::Fill;
use vello::peniko::Gradient;
use vello::peniko::ImageAlphaType;
use vello::peniko::ImageBrush;
use vello::peniko::ImageData;
use vello::peniko::ImageFormat;
use vello::peniko::ImageQuality;
use vello::peniko::ImageSampler;
use vello::peniko::InterpolationAlphaSpace;
use vello::peniko::Mix;
use yuzuko_image::ColorSpace;
use yuzuko_image::Raster;
use yuzuko_image::SampleFormat;
use yuzuko_image::convert;
use yuzuko_image::premultiply;
use yuzuko_image::unpremultiply;

use crate::paint::color::CssColor;
use crate::paint::record::Brush as Paint;
use crate::paint::record::Command;
use crate::paint::record::Drawing;
use crate::paint::record::ImageBuf;

static BACKEND: OnceLock<Backend> = OnceLock::new();

enum Backend {
  Cpu,
  Gpu,
}

pub fn renderer_name() -> &'static str {
  match backend() {
    Backend::Cpu => "cpu",
    Backend::Gpu => "gpu",
  }
}

pub fn select_renderer() {
  let _ = backend();
}

fn backend() -> &'static Backend {
  BACKEND.get_or_init(|| {
    let requested = std::env::var("YUZUKO_CANVAS").ok();
    let skip = std::env::var("YUZUKO_SKIP_GPU").ok().as_deref() == Some("1");
    if requested.as_deref() == Some("cpu") || skip {
      return Backend::Cpu;
    }
    if yuzuko_window::ensure_gpu() && gpu_renderer_works() {
      Backend::Gpu
    } else {
      Backend::Cpu
    }
  })
}

fn gpu_renderer_works() -> bool {
  yuzuko_window::with_device(|device, _queue| {
    // vello's compute pipelines need wgpu's default limits. On a downlevel
    // device, such as a GL driver, building them fails as an uncaptured
    // validation error that panics instead of returning `Err`.
    if !wgpu::Limits::default().check_limits(&device.limits()) {
      return false;
    }
    vello::Renderer::new(device, vello::RendererOptions::default()).is_ok()
  })
  .unwrap_or(false)
}

struct GpuRenderer {
  renderer: std::mem::ManuallyDrop<vello::Renderer>,
}

std::thread_local! {
  static GPU: RefCell<Option<GpuRenderer>> = const { RefCell::new(None) };
}

/// Rasterize a layer's commands to a premultiplied sRGB bitmap.
///
/// The filter string is not applied. vello has no CSS filter.
pub fn layer_image(
  commands: &[Command],
  width: u32,
  height: u32,
) -> Option<ImageBuf> {
  if width == 0 || height == 0 || commands.is_empty() {
    return None;
  }
  let bytes = render_premul(commands, width, height)?;
  Some(ImageBuf {
    width,
    height,
    display_p3: false,
    float16: false,
    premultiplied: true,
    bytes,
  })
}

pub fn rasterize(drawing: &Drawing) -> Option<Raster> {
  if drawing.width == 0 || drawing.height == 0 {
    return None;
  }
  if drawing.attrs.vello_target() {
    let premul =
      render_premul(&drawing.commands, drawing.width, drawing.height)?;
    let mut raster = Raster::from_rgba8(
      drawing.width,
      drawing.height,
      ColorSpace::Srgb,
      &premul,
    )
    .ok()?;
    raster.premultiplied = true;
    if !drawing.attrs.alpha {
      force_opaque(&mut raster);
    }
    unpremultiply(&mut raster);
    return Some(raster);
  }
  Some(rasterize_backing(drawing))
}

fn force_opaque(raster: &mut Raster) {
  let pixels = raster.width as usize * raster.height as usize;
  for pixel in 0..pixels {
    let index = pixel * 4 + 3;
    if index < raster.bytes.len() {
      raster.bytes[index] = 255;
    }
  }
}

fn rasterize_backing(drawing: &Drawing) -> Raster {
  let mut image = Raster::new(
    drawing.width,
    drawing.height,
    drawing.attrs.color_space,
    drawing.attrs.format,
  )
  .expect("canvas size was checked");
  let mut batch = Vec::new();
  for command in &drawing.commands {
    match command {
      Command::Put {
        image: source,
        dx,
        dy,
      } => {
        paint_batch(&mut image, &batch, drawing);
        batch.clear();
        put_direct(&mut image, source, *dx, *dy);
      }
      Command::Clear {
        rect,
        transform,
        clips,
      } => {
        paint_batch(&mut image, &batch, drawing);
        batch.clear();
        clear_direct(&mut image, *rect, *transform, clips);
      }
      other => batch.push(other.clone()),
    }
  }
  paint_batch(&mut image, &batch, drawing);
  if !drawing.attrs.alpha {
    force_opaque(&mut image);
  }
  image
}

fn paint_batch(image: &mut Raster, batch: &[Command], drawing: &Drawing) {
  if batch.is_empty() {
    return;
  }
  let Some(premul) = render_premul(batch, drawing.width, drawing.height) else {
    return;
  };
  let mut layer = Raster::from_rgba8(
    drawing.width,
    drawing.height,
    ColorSpace::Srgb,
    &premul,
  )
  .expect("layer");
  layer.premultiplied = true;
  unpremultiply(&mut layer);
  convert(&mut layer, image.color_space);
  if image.format == SampleFormat::Float16 {
    composite_straight(image, &layer);
  } else {
    premultiply(&mut layer);
    let mut dest_premul = image.clone();
    premultiply(&mut dest_premul);
    composite_premul(&mut dest_premul, &layer);
    *image = dest_premul;
    unpremultiply(image);
  }
}

fn composite_straight(dest: &mut Raster, src: &Raster) {
  let pixels = dest.width as usize * dest.height as usize;
  for pixel in 0..pixels {
    let x = (pixel % dest.width as usize) as i64;
    let y = (pixel / dest.width as usize) as i64;
    let src_px = src.sample(x, y);
    let dst_px = dest.sample(x, y);
    let src_a = src_px[3];
    let dst_a = dst_px[3];
    let out_a = src_a + dst_a * (1.0 - src_a);
    let out = if out_a == 0.0 {
      [0.0, 0.0, 0.0, 0.0]
    } else {
      let channel = |index: usize| {
        (src_px[index] * src_a + dst_px[index] * dst_a * (1.0 - src_a)) / out_a
      };
      [channel(0), channel(1), channel(2), out_a]
    };
    dest.write_sample(x, y, out);
  }
  let _ = pixels;
}

fn composite_premul(dest: &mut Raster, src: &Raster) {
  let width = dest.width as i64;
  for y in 0..dest.height as i64 {
    for x in 0..width {
      let src_px = src.sample(x, y);
      let dst_px = dest.sample(x, y);
      let inv = 1.0 - src_px[3];
      dest.write_sample(
        x,
        y,
        [
          src_px[0] + dst_px[0] * inv,
          src_px[1] + dst_px[1] * inv,
          src_px[2] + dst_px[2] * inv,
          src_px[3] + dst_px[3] * inv,
        ],
      );
    }
  }
}

fn put_direct(dest: &mut Raster, source: &ImageBuf, dx: i32, dy: i32) {
  let mut raster = image_to_raster(source);
  convert(&mut raster, dest.color_space);
  if raster.format != dest.format {
    raster = convert_format(raster, dest.format);
  }
  for y in 0..raster.height as i32 {
    for x in 0..raster.width as i32 {
      let sample = raster.sample(i64::from(x), i64::from(y));
      dest.write_sample(i64::from(dx + x), i64::from(dy + y), sample);
    }
  }
}

fn clear_direct(
  dest: &mut Raster,
  rect: Rect,
  transform: Affine,
  clips: &[crate::paint::record::Clip],
) {
  if transform.as_coeffs()[1] == 0.0
    && transform.as_coeffs()[2] == 0.0
    && clips.is_empty()
  {
    let mapped = transform.transform_rect_bbox(rect);
    let x0 = mapped.x0.floor() as i32;
    let y0 = mapped.y0.floor() as i32;
    let x1 = mapped.x1.ceil() as i32;
    let y1 = mapped.y1.ceil() as i32;
    for y in y0..y1 {
      for x in x0..x1 {
        dest.write_sample(i64::from(x), i64::from(y), [0.0, 0.0, 0.0, 0.0]);
      }
    }
    return;
  }
  let mask = render_premul(
    &[Command::Fill {
      path: rect.to_path(0.1),
      fill: Fill::NonZero,
      brush: Paint::Solid(CssColor::BLACK),
      transform,
      brush_transform: Affine::IDENTITY,
      alpha: 1.0,
      clips: Arc::from(clips),
    }],
    dest.width,
    dest.height,
  );
  let Some(mask) = mask else {
    return;
  };
  for y in 0..dest.height {
    for x in 0..dest.width {
      let alpha = mask[((y * dest.width + x) * 4 + 3) as usize];
      if alpha > 0 {
        dest.write_sample(i64::from(x), i64::from(y), [0.0, 0.0, 0.0, 0.0]);
      }
    }
  }
}

fn image_to_raster(image: &ImageBuf) -> Raster {
  let space = if image.display_p3 {
    ColorSpace::DisplayP3
  } else {
    ColorSpace::Srgb
  };
  let format = if image.float16 {
    SampleFormat::Float16
  } else {
    SampleFormat::Unorm8
  };
  let mut raster =
    Raster::new(image.width, image.height, space, format).expect("image");
  raster.bytes.clone_from(&image.bytes);
  raster.premultiplied = image.premultiplied;
  if raster.premultiplied {
    unpremultiply(&mut raster);
  }
  raster
}

fn convert_format(mut raster: Raster, format: SampleFormat) -> Raster {
  if raster.format == format {
    return raster;
  }
  let mut output =
    Raster::new(raster.width, raster.height, raster.color_space, format)
      .expect("format");
  for y in 0..raster.height {
    for x in 0..raster.width {
      output.write_sample(
        i64::from(x),
        i64::from(y),
        raster.sample(i64::from(x), i64::from(y)),
      );
    }
  }
  let _ = &mut raster;
  output
}

pub fn render_premul(
  commands: &[Command],
  width: u32,
  height: u32,
) -> Option<Vec<u8>> {
  if width == 0 || height == 0 {
    return None;
  }
  match backend() {
    Backend::Gpu => render_gpu(commands, width, height)
      .or_else(|| render_cpu(commands, width, height)),
    Backend::Cpu => render_cpu(commands, width, height),
  }
}

fn render_cpu(
  commands: &[Command],
  width: u32,
  height: u32,
) -> Option<Vec<u8>> {
  let width = u16::try_from(width).ok()?;
  let height = u16::try_from(height).ok()?;
  let mut context = vello_cpu::RenderContext::new(width, height);
  let mut resources = vello_cpu::Resources::new();
  for command in commands {
    replay_cpu(&mut context, &mut resources, command, width, height);
  }
  context.flush();
  let mut buffer = vec![0; usize::from(width) * usize::from(height) * 4];
  context.render_to_buffer(
    &mut resources,
    &mut buffer,
    width,
    height,
    vello_cpu::RenderMode::OptimizeQuality,
  );
  Some(buffer)
}

fn replay_cpu(
  context: &mut vello_cpu::RenderContext,
  resources: &mut vello_cpu::Resources,
  command: &Command,
  width: u16,
  height: u16,
) {
  let _ = (width, height);
  match command {
    Command::Fill {
      path,
      fill,
      brush,
      transform,
      brush_transform,
      alpha,
      clips,
    } => {
      if !starts_with_move(path) {
        return;
      }
      push_cpu_clips(context, clips);
      let pattern = pattern_clip(brush, *transform, *brush_transform);
      if let Some(clip) = pattern.as_ref() {
        push_cpu_clips(context, std::slice::from_ref(clip));
      }
      if *alpha < 1.0 {
        context.push_opacity_layer(*alpha);
      }
      paint_cpu(context, brush, *brush_transform);
      context.set_fill_rule(*fill);
      context.set_transform(*transform);
      context.set_blend_mode(source_over());
      context.fill_path(path);
      if *alpha < 1.0 {
        context.pop_layer();
      }
      if pattern.is_some() {
        pop_cpu_clips(context, 1);
      }
      pop_cpu_clips(context, clips.len());
    }
    Command::Stroke {
      path,
      stroke,
      brush,
      transform,
      brush_transform,
      alpha,
      clips,
    } => {
      if !starts_with_move(path) {
        return;
      }
      push_cpu_clips(context, clips);
      let pattern = pattern_clip(brush, *transform, *brush_transform);
      if let Some(clip) = pattern.as_ref() {
        push_cpu_clips(context, std::slice::from_ref(clip));
      }
      if *alpha < 1.0 {
        context.push_opacity_layer(*alpha);
      }
      paint_cpu(context, brush, *brush_transform);
      context.set_stroke(stroke.clone());
      context.set_transform(*transform);
      context.set_blend_mode(source_over());
      context.stroke_path(path);
      if *alpha < 1.0 {
        context.pop_layer();
      }
      if pattern.is_some() {
        pop_cpu_clips(context, 1);
      }
      pop_cpu_clips(context, clips.len());
    }
    Command::Glyphs {
      font,
      size,
      glyphs,
      brush,
      transform,
      alpha,
      stroke,
      clips,
    } => {
      push_cpu_clips(context, clips);
      if *alpha < 1.0 {
        context.push_opacity_layer(*alpha);
      }
      paint_cpu(context, brush, Affine::IDENTITY);
      context.set_transform(*transform);
      context.set_blend_mode(source_over());
      if let Some(stroke) = stroke {
        context.set_stroke(stroke.clone());
      }
      let run = context.glyph_run(resources, font).font_size(*size);
      let glyphs =
        glyphs
          .iter()
          .copied()
          .map(|(id, x, y)| vello_cpu::Glyph { id, x, y });
      if stroke.is_some() {
        run.stroke_glyphs(glyphs);
      } else {
        run.fill_glyphs(glyphs);
      }
      if *alpha < 1.0 {
        context.pop_layer();
      }
      pop_cpu_clips(context, clips.len());
    }
    Command::Clear {
      rect,
      transform,
      clips,
    } => {
      push_cpu_clips(context, clips);
      context.set_blend_mode(BlendMode::new(Mix::Normal, Compose::Clear));
      context.set_transform(*transform);
      context.set_paint(vello::peniko::Color::new([0.0, 0.0, 0.0, 1.0]));
      context.fill_rect(rect);
      context.set_blend_mode(source_over());
      pop_cpu_clips(context, clips.len());
    }
    Command::Put { image, dx, dy } => {
      let premul = premul_rgba8(image);
      context.set_blend_mode(BlendMode::new(Mix::Normal, Compose::Copy));
      context.set_transform(Affine::translate((*dx as f64, *dy as f64)));
      paint_image(context, &premul, image.width, image.height, true);
      context.fill_rect(&Rect::new(
        0.0,
        0.0,
        image.width as f64,
        image.height as f64,
      ));
      context.set_blend_mode(source_over());
    }
    Command::Image {
      image,
      transform,
      alpha,
      nearest,
      clips,
    } => {
      push_cpu_clips(context, clips);
      if *alpha < 1.0 {
        context.push_opacity_layer(*alpha);
      }
      context.set_transform(*transform);
      context.set_blend_mode(source_over());
      paint_image(context, &image.bytes, image.width, image.height, *nearest);
      context.fill_rect(&Rect::new(
        0.0,
        0.0,
        image.width as f64,
        image.height as f64,
      ));
      if *alpha < 1.0 {
        context.pop_layer();
      }
      pop_cpu_clips(context, clips.len());
    }
  }
}

fn push_cpu_clips(
  context: &mut vello_cpu::RenderContext,
  clips: &[crate::paint::record::Clip],
) {
  for clip in clips {
    context.set_fill_rule(clip.fill);
    context.set_transform(clip.transform);
    context.push_clip_path(&clip.path);
  }
}

fn pop_cpu_clips(context: &mut vello_cpu::RenderContext, count: usize) {
  for _ in 0..count {
    context.pop_clip_path();
  }
}

fn paint_cpu(
  context: &mut vello_cpu::RenderContext,
  brush: &Paint,
  transform: Affine,
) {
  context.reset_paint_transform();
  if transform != Affine::IDENTITY {
    context.set_paint_transform(transform);
  }
  match brush_of(brush, true) {
    Some(Brush::Solid(color)) => context.set_paint(color),
    Some(Brush::Gradient(gradient)) => context.set_paint(gradient),
    Some(Brush::Image(image)) => {
      let source = vello_cpu::ImageSource::from_peniko_image_data(&image.image);
      context.set_paint(Brush::Image(ImageBrush {
        image: source,
        sampler: image.sampler,
      }));
    }
    None => context.set_paint(vello::peniko::Color::new([0.0, 0.0, 0.0, 0.0])),
  }
}

fn paint_image(
  context: &mut vello_cpu::RenderContext,
  bytes: &[u8],
  width: u32,
  height: u32,
  nearest: bool,
) {
  let data = image_data(bytes, width, height);
  let source = vello_cpu::ImageSource::from_peniko_image_data(&data);
  context.reset_paint_transform();
  context.set_paint(Brush::Image(ImageBrush {
    image: source,
    sampler: ImageSampler::default().with_quality(if nearest {
      ImageQuality::Low
    } else {
      ImageQuality::Medium
    }),
  }));
}

fn starts_with_move(path: &kurbo::BezPath) -> bool {
  matches!(path.elements().first(), Some(kurbo::PathEl::MoveTo(_)))
}

fn brush_affine(transform: Affine) -> Option<Affine> {
  if transform == Affine::IDENTITY {
    None
  } else {
    Some(transform)
  }
}

/// `no-repeat` is transparent outside the image. Peniko only pads or repeats,
/// so the uncovered axis is clipped to the image.
fn pattern_clip(
  brush: &Paint,
  transform: Affine,
  brush_transform: Affine,
) -> Option<crate::paint::record::Clip> {
  let Paint::Pattern {
    image,
    repeat_x,
    repeat_y,
    ..
  } = brush
  else {
    return None;
  };
  if *repeat_x && *repeat_y {
    return None;
  }
  let (x, w) = if *repeat_x {
    (-1.0e6, 2.0e6)
  } else {
    (0.0, f64::from(image.width))
  };
  let (y, h) = if *repeat_y {
    (-1.0e6, 2.0e6)
  } else {
    (0.0, f64::from(image.height))
  };
  let mut path = kurbo::BezPath::new();
  path.move_to((x, y));
  path.line_to((x + w, y));
  path.line_to((x + w, y + h));
  path.line_to((x, y + h));
  path.close_path();
  Some(crate::paint::record::Clip {
    path,
    fill: Fill::NonZero,
    transform: transform * brush_transform,
  })
}

fn source_over() -> BlendMode {
  BlendMode::new(Mix::Normal, Compose::SrcOver)
}

fn brush_of(brush: &Paint, _cpu: bool) -> Option<Brush> {
  match brush {
    Paint::Solid(color) => Some(Brush::Solid(color.to_srgb_peniko())),
    Paint::Linear { start, end, stops } => {
      if stops.is_empty() {
        return None;
      }
      let stops = peniko_stops(stops);
      let mut gradient =
        Gradient::new_linear(*start, *end).with_stops(&stops[..]);
      gradient.interpolation_alpha_space =
        InterpolationAlphaSpace::Unpremultiplied;
      Some(Brush::Gradient(gradient))
    }
    Paint::Radial { start, end, stops } => {
      if stops.is_empty() {
        return None;
      }
      let stops = peniko_stops(stops);
      let mut gradient = Gradient::new_two_point_radial(
        (start.0, start.1),
        start.2,
        (end.0, end.1),
        end.2,
      )
      .with_stops(&stops[..]);
      gradient.interpolation_alpha_space =
        InterpolationAlphaSpace::Unpremultiplied;
      Some(Brush::Gradient(gradient))
    }
    Paint::Pattern {
      image,
      repeat_x,
      repeat_y,
      nearest,
    } => {
      let premul = premul_rgba8(image);
      let data = image_data(&premul, image.width, image.height);
      let extend_x = if *repeat_x {
        Extend::Repeat
      } else {
        Extend::Pad
      };
      let extend_y = if *repeat_y {
        Extend::Repeat
      } else {
        Extend::Pad
      };
      Some(Brush::Image(ImageBrush {
        image: data,
        sampler: ImageSampler::default()
          .with_x_extend(extend_x)
          .with_y_extend(extend_y)
          .with_quality(if *nearest {
            ImageQuality::Low
          } else {
            ImageQuality::Medium
          }),
      }))
    }
  }
}

fn peniko_stops(stops: &[(f32, CssColor)]) -> Vec<(f32, vello::peniko::Color)> {
  let mut stops: Vec<(f32, vello::peniko::Color)> = stops
    .iter()
    .map(|(offset, color)| (*offset, color.to_srgb_peniko()))
    .collect();
  if stops.len() == 1 {
    let color = stops[0].1;
    stops = vec![(0.0, color), (1.0, color)];
  }
  stops
}

fn premul_rgba8(image: &ImageBuf) -> Vec<u8> {
  let mut raster = image_to_raster(image);
  convert(&mut raster, ColorSpace::Srgb);
  if raster.format != SampleFormat::Unorm8 {
    raster = convert_format(raster, SampleFormat::Unorm8);
  }
  premultiply(&mut raster);
  raster.bytes
}

fn image_data(bytes: &[u8], width: u32, height: u32) -> ImageData {
  ImageData {
    data: vello::peniko::Blob::from(bytes.to_vec()),
    format: ImageFormat::Rgba8,
    alpha_type: ImageAlphaType::AlphaPremultiplied,
    width,
    height,
  }
}

fn render_gpu(
  commands: &[Command],
  width: u32,
  height: u32,
) -> Option<Vec<u8>> {
  yuzuko_window::with_device(|device, queue| {
    let scene = build_scene(commands, width, height);
    let texture = device.create_texture(&wgpu::TextureDescriptor {
      label: Some("yuzuko-canvas"),
      size: wgpu::Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
      },
      mip_level_count: 1,
      sample_count: 1,
      dimension: wgpu::TextureDimension::D2,
      format: wgpu::TextureFormat::Rgba8Unorm,
      usage: wgpu::TextureUsages::STORAGE_BINDING
        | wgpu::TextureUsages::COPY_SRC
        | wgpu::TextureUsages::TEXTURE_BINDING,
      view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    GPU.with(|slot| {
      let mut slot = slot.borrow_mut();
      if slot.is_none() {
        let renderer =
          vello::Renderer::new(device, vello::RendererOptions::default())
            .ok()?;
        *slot = Some(GpuRenderer {
          renderer: std::mem::ManuallyDrop::new(renderer),
        });
      }
      let gpu = slot.as_mut()?;
      gpu
        .renderer
        .render_to_texture(
          device,
          queue,
          &scene,
          &view,
          &vello::RenderParams {
            base_color: vello::peniko::Color::new([0.0, 0.0, 0.0, 0.0]),
            width,
            height,
            antialiasing_method: vello::AaConfig::Area,
          },
        )
        .ok()?;
      Some(())
    })?;
    readback(device, queue, &texture, width, height)
  })?
}

fn build_scene(commands: &[Command], width: u32, height: u32) -> vello::Scene {
  let mut scene = vello::Scene::new();
  for command in commands {
    replay_gpu(&mut scene, command, width, height);
  }
  scene
}

fn replay_gpu(
  scene: &mut vello::Scene,
  command: &Command,
  width: u32,
  height: u32,
) {
  let full = Rect::new(0.0, 0.0, f64::from(width), f64::from(height));
  match command {
    Command::Fill {
      path,
      fill,
      brush,
      transform,
      brush_transform,
      alpha,
      clips,
    } => {
      if !starts_with_move(path) {
        return;
      }
      push_gpu_clips(scene, clips);
      let pattern = pattern_clip(brush, *transform, *brush_transform);
      if let Some(clip) = pattern.as_ref() {
        push_gpu_clips(scene, std::slice::from_ref(clip));
      }
      push_gpu_alpha(scene, *alpha, full);
      if let Some(paint) = brush_of(brush, false) {
        scene.fill(
          *fill,
          *transform,
          &paint,
          brush_affine(*brush_transform),
          path,
        );
      }
      pop_gpu_alpha(scene, *alpha);
      if pattern.is_some() {
        pop_gpu_clips(scene, 1);
      }
      pop_gpu_clips(scene, clips.len());
    }
    Command::Stroke {
      path,
      stroke,
      brush,
      transform,
      brush_transform,
      alpha,
      clips,
    } => {
      if !starts_with_move(path) {
        return;
      }
      push_gpu_clips(scene, clips);
      let pattern = pattern_clip(brush, *transform, *brush_transform);
      if let Some(clip) = pattern.as_ref() {
        push_gpu_clips(scene, std::slice::from_ref(clip));
      }
      push_gpu_alpha(scene, *alpha, full);
      if let Some(paint) = brush_of(brush, false) {
        scene.stroke(
          stroke,
          *transform,
          &paint,
          brush_affine(*brush_transform),
          path,
        );
      }
      pop_gpu_alpha(scene, *alpha);
      if pattern.is_some() {
        pop_gpu_clips(scene, 1);
      }
      pop_gpu_clips(scene, clips.len());
    }
    Command::Glyphs {
      font,
      size,
      glyphs,
      brush,
      transform,
      alpha,
      stroke,
      clips,
    } => {
      push_gpu_clips(scene, clips);
      push_gpu_alpha(scene, *alpha, full);
      if let Some(brush) = brush_of(brush, false) {
        let draw = scene
          .draw_glyphs(font)
          .font_size(*size)
          .transform(*transform)
          .brush(&brush);
        let glyphs =
          glyphs
            .iter()
            .copied()
            .map(|(id, x, y)| vello::Glyph { id, x, y });
        if let Some(stroke) = stroke {
          draw.draw(stroke, glyphs);
        } else {
          draw.draw(Fill::NonZero, glyphs);
        }
      }
      pop_gpu_alpha(scene, *alpha);
      pop_gpu_clips(scene, clips.len());
    }
    Command::Clear {
      rect,
      transform,
      clips,
    } => {
      push_gpu_clips(scene, clips);
      scene.push_layer(
        Fill::NonZero,
        BlendMode::new(Mix::Normal, Compose::Clear),
        1.0,
        *transform,
        rect,
      );
      scene.fill(
        Fill::NonZero,
        *transform,
        vello::peniko::Color::new([0.0, 0.0, 0.0, 1.0]),
        None,
        rect,
      );
      scene.pop_layer();
      pop_gpu_clips(scene, clips.len());
    }
    Command::Put { image, dx, dy } => {
      let premul = premul_rgba8(image);
      let data = image_data(&premul, image.width, image.height);
      let brush = ImageBrush {
        sampler: ImageSampler::default().with_quality(ImageQuality::Low),
        image: data,
      };
      scene.push_layer(
        Fill::NonZero,
        BlendMode::new(Mix::Normal, Compose::Copy),
        1.0,
        Affine::IDENTITY,
        &Rect::new(
          f64::from(*dx),
          f64::from(*dy),
          f64::from(*dx) + image.width as f64,
          f64::from(*dy) + image.height as f64,
        ),
      );
      scene.draw_image(
        &brush,
        Affine::translate((f64::from(*dx), f64::from(*dy))),
      );
      scene.pop_layer();
    }
    Command::Image {
      image,
      transform,
      alpha,
      nearest,
      clips,
    } => {
      push_gpu_clips(scene, clips);
      push_gpu_alpha(scene, *alpha, full);
      let data = image_data(&image.bytes, image.width, image.height);
      let brush = ImageBrush {
        image: data,
        sampler: ImageSampler::default().with_quality(if *nearest {
          ImageQuality::Low
        } else {
          ImageQuality::Medium
        }),
      };
      scene.draw_image(&brush, *transform);
      pop_gpu_alpha(scene, *alpha);
      pop_gpu_clips(scene, clips.len());
    }
  }
}

fn push_gpu_clips(
  scene: &mut vello::Scene,
  clips: &[crate::paint::record::Clip],
) {
  for clip in clips {
    scene.push_clip_layer(clip.fill, clip.transform, &clip.path);
  }
}

fn pop_gpu_clips(scene: &mut vello::Scene, count: usize) {
  for _ in 0..count {
    scene.pop_layer();
  }
}

fn push_gpu_alpha(scene: &mut vello::Scene, alpha: f32, full: Rect) {
  if alpha < 1.0 {
    scene.push_layer(
      Fill::NonZero,
      source_over(),
      alpha,
      Affine::IDENTITY,
      &full,
    );
  }
}

fn pop_gpu_alpha(scene: &mut vello::Scene, alpha: f32) {
  if alpha < 1.0 {
    scene.pop_layer();
  }
}

fn readback(
  device: &wgpu::Device,
  queue: &wgpu::Queue,
  texture: &wgpu::Texture,
  width: u32,
  height: u32,
) -> Option<Vec<u8>> {
  let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
  let bytes_per_row = (width * 4).div_ceil(align) * align;
  let buffer = device.create_buffer(&wgpu::BufferDescriptor {
    label: Some("yuzuko-canvas-readback"),
    size: u64::from(bytes_per_row) * u64::from(height),
    usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
    mapped_at_creation: false,
  });
  let mut encoder =
    device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
      label: Some("yuzuko-canvas-readback"),
    });
  encoder.copy_texture_to_buffer(
    texture.as_image_copy(),
    wgpu::TexelCopyBufferInfo {
      buffer: &buffer,
      layout: wgpu::TexelCopyBufferLayout {
        offset: 0,
        bytes_per_row: Some(bytes_per_row),
        rows_per_image: Some(height),
      },
    },
    texture.size(),
  );
  queue.submit([encoder.finish()]);
  let slice = buffer.slice(..);
  let (sender, receiver) = std::sync::mpsc::channel();
  slice.map_async(wgpu::MapMode::Read, move |result| {
    let _ = sender.send(result);
  });
  let _ = device.poll(wgpu::PollType::Wait {
    submission_index: None,
    timeout: None,
  });
  receiver.recv().ok()?.ok()?;
  let view = slice.get_mapped_range();
  let mut pixels = vec![0; width as usize * height as usize * 4];
  for row in 0..height as usize {
    let src = row * bytes_per_row as usize;
    let dst = row * width as usize * 4;
    pixels[dst..dst + width as usize * 4]
      .copy_from_slice(&view[src..src + width as usize * 4]);
  }
  drop(view);
  buffer.unmap();
  Some(pixels)
}

pub fn present_texture(
  device: &wgpu::Device,
  queue: &wgpu::Queue,
  target: &wgpu::TextureView,
  format: wgpu::TextureFormat,
  pixels: &[u8],
  width: u32,
  height: u32,
) {
  if width == 0
    || height == 0
    || pixels.len() < width as usize * height as usize * 4
  {
    return;
  }
  let texture = device.create_texture(&wgpu::TextureDescriptor {
    label: Some("yuzuko-canvas-present"),
    size: wgpu::Extent3d {
      width,
      height,
      depth_or_array_layers: 1,
    },
    mip_level_count: 1,
    sample_count: 1,
    dimension: wgpu::TextureDimension::D2,
    format: wgpu::TextureFormat::Rgba8Unorm,
    usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
    view_formats: &[],
  });
  queue.write_texture(
    texture.as_image_copy(),
    pixels,
    wgpu::TexelCopyBufferLayout {
      offset: 0,
      bytes_per_row: Some(width * 4),
      rows_per_image: Some(height),
    },
    texture.size(),
  );
  let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
  let blitter = wgpu::util::TextureBlitter::new(device, format);
  let mut encoder =
    device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
      label: Some("yuzuko-canvas-present"),
    });
  blitter.copy(device, &mut encoder, &view, target);
  queue.submit([encoder.finish()]);
}
