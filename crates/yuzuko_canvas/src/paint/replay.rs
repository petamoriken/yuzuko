//! Replay one command list onto the CPU renderer or the GPU renderer.

use std::cell::Cell;
use std::cell::RefCell;
use std::sync::{Arc, OnceLock};

use kurbo::Affine;
use kurbo::PathEl;
use kurbo::Rect;
use kurbo::Shape;
use vello_cpu::peniko::BlendMode;
use vello_cpu::peniko::Brush;
use vello_cpu::peniko::Compose;
use vello_cpu::peniko::Extend;
use vello_cpu::peniko::Fill;
use vello_cpu::peniko::Gradient;
use vello_cpu::peniko::ImageAlphaType;
use vello_cpu::peniko::ImageBrush;
use vello_cpu::peniko::ImageData;
use vello_cpu::peniko::ImageFormat;
use vello_cpu::peniko::ImageQuality;
use vello_cpu::peniko::ImageSampler;
use vello_cpu::peniko::InterpolationAlphaSpace;
use vello_cpu::peniko::Mix;
use yuzuko_image::ColorSpace;
use yuzuko_image::Raster;
use yuzuko_image::SampleFormat;
use yuzuko_image::convert;
use yuzuko_image::convert_encoded;
use yuzuko_image::premultiply;
use yuzuko_image::unpremultiply;

use crate::paint::color::ColorSyntax;
use crate::paint::color::CssColor;
use crate::paint::composite;
use crate::paint::record::Brush as Paint;
use crate::paint::record::Command;
use crate::paint::record::Drawing;
use crate::paint::record::ImageBuf;
use crate::paint::shade::Shade;

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
    // `Renderer::new` builds pipelines and does not return `Result`. A
    // panic means this device cannot run the fragment renderer.
    let created =
      std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        vello_gpu::Renderer::new(
          device,
          &vello_gpu::RenderTargetConfig {
            format: wgpu::TextureFormat::Rgba8Unorm,
            width: 1,
            height: 1,
          },
        )
      }));
    let Ok((renderer, resources)) = created else {
      return false;
    };
    GPU.with(|slot| {
      *slot.borrow_mut() = Some(GpuState {
        renderer,
        resources,
        depth: None,
      });
    });
    true
  })
  .unwrap_or(false)
}

struct GpuState {
  renderer: vello_gpu::Renderer,
  resources: vello_gpu::Resources,
  depth: Option<DepthCache>,
}

enum DepthCache {
  View(u16, u16, wgpu::TextureView),
  Unavailable,
}

std::thread_local! {
  static GPU: RefCell<Option<GpuState>> = const { RefCell::new(None) };
  static GPU_BROKEN: Cell<bool> = const { Cell::new(false) };
  static BLITTER: RefCell<
    Option<(wgpu::TextureFormat, wgpu::util::TextureBlitter)>,
  > = const { RefCell::new(None) };
}

/// Rasterize a layer's commands to a premultiplied sRGB bitmap.
///
/// A per-command filter is applied while the commands replay. The
/// `beginLayer` filter is applied by the caller to this bitmap.
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
      other
        if image.format == SampleFormat::Float16
          && is_opaque_solid_rect(other) =>
      {
        paint_batch(&mut image, &batch, drawing);
        batch.clear();
        paint_opaque_solid_rect(&mut image, other);
      }
      other if composite::needs_isolated(other) => {
        paint_batch(&mut image, &batch, drawing);
        batch.clear();
        paint_isolated(&mut image, other);
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
  if image.format == SampleFormat::Float16 {
    // Scale 0..255 into encoded 0..1 before the blend. Color conversion then
    // runs in float16 so an in-gamut result is not quantized twice.
    let mut layer = convert_format(layer, SampleFormat::Float16);
    convert(&mut layer, image.color_space);
    composite_straight(image, &layer);
  } else {
    convert(&mut layer, image.color_space);
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
  // A negative channel, such as Display P3 red stored in sRGB, survives only
  // when the color conversion runs after the samples are float16.
  if raster.format != dest.format && dest.format == SampleFormat::Float16 {
    raster = convert_format(raster, SampleFormat::Float16);
  }
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
      shade: Shade::source_over(),
      filter: crate::paint::record::empty_filter(),
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

fn convert_format(raster: Raster, format: SampleFormat) -> Raster {
  yuzuko_image::convert_format(raster, format).expect("canvas size was checked")
}

/// An opaque axis-aligned solid `fillRect` can be stored without vello's
/// sRGB unorm8 clip. Other fills stay on the vello path.
fn is_opaque_solid_rect(command: &Command) -> bool {
  solid_rect(command).is_some()
}

fn paint_opaque_solid_rect(dest: &mut Raster, command: &Command) {
  let Some((rect, color)) = solid_rect(command) else {
    return;
  };
  let encoded = solid_encoded(color, dest.color_space);
  let pixel = match dest.format {
    SampleFormat::Float16 => encoded,
    SampleFormat::Unorm8 => encoded.map(|channel| channel * 255.0),
  };
  for y in 0..dest.height as i32 {
    for x in 0..dest.width as i32 {
      let center_x = f64::from(x) + 0.5;
      let center_y = f64::from(y) + 0.5;
      if center_x >= rect.x0
        && center_x < rect.x1
        && center_y >= rect.y0
        && center_y < rect.y1
      {
        dest.write_sample(i64::from(x), i64::from(y), pixel);
      }
    }
  }
}

fn solid_rect(command: &Command) -> Option<(Rect, CssColor)> {
  let Command::Fill {
    path,
    fill,
    brush,
    transform,
    alpha,
    shade,
    filter,
    clips,
    ..
  } = command
  else {
    return None;
  };
  if *fill != Fill::NonZero
    || !shade.is_plain()
    || !filter.is_empty()
    || !clips.is_empty()
    || (*alpha - 1.0).abs() > 1.0e-4
  {
    return None;
  }
  let Paint::Solid(color) = brush else {
    return None;
  };
  if (color.a - 1.0).abs() > 1.0e-4 {
    return None;
  }
  let rect = axis_aligned_rect(path, *transform)?;
  Some((rect, *color))
}

fn solid_encoded(color: CssColor, space: ColorSpace) -> [f64; 4] {
  let source = if color.syntax == ColorSyntax::DisplayP3 {
    ColorSpace::DisplayP3
  } else {
    ColorSpace::Srgb
  };
  let rgb = convert_encoded(
    source,
    space,
    [f64::from(color.r), f64::from(color.g), f64::from(color.b)],
  );
  [rgb[0], rgb[1], rgb[2], f64::from(color.a)]
}

fn axis_aligned_rect(path: &kurbo::BezPath, transform: Affine) -> Option<Rect> {
  let elements: Vec<PathEl> = path.elements().to_vec();
  let points = match elements.as_slice() {
    [
      PathEl::MoveTo(p0),
      PathEl::LineTo(p1),
      PathEl::LineTo(p2),
      PathEl::LineTo(p3),
      PathEl::ClosePath,
    ] => [*p0, *p1, *p2, *p3],
    _ => return None,
  };
  let points = points.map(|point| transform * point);
  let min_x = points
    .iter()
    .map(|point| point.x)
    .fold(f64::INFINITY, f64::min);
  let max_x = points
    .iter()
    .map(|point| point.x)
    .fold(f64::NEG_INFINITY, f64::max);
  let min_y = points
    .iter()
    .map(|point| point.y)
    .fold(f64::INFINITY, f64::min);
  let max_y = points
    .iter()
    .map(|point| point.y)
    .fold(f64::NEG_INFINITY, f64::max);
  for point in points {
    let on_x =
      (point.x - min_x).abs() <= 1.0e-6 || (point.x - max_x).abs() <= 1.0e-6;
    let on_y =
      (point.y - min_y).abs() <= 1.0e-6 || (point.y - max_y).abs() <= 1.0e-6;
    if !on_x || !on_y {
      return None;
    }
  }
  Some(Rect::new(min_x, min_y, max_x, max_y))
}

pub fn render_premul(
  commands: &[Command],
  width: u32,
  height: u32,
) -> Option<Vec<u8>> {
  if width == 0 || height == 0 {
    return None;
  }
  // source-in, copy, clear, and shadows touch pixels the shape misses.
  // Coverage-only blending does not, so those lists run on the CPU.
  if commands.iter().any(composite::needs_isolated) {
    return render_isolated(commands, width, height);
  }
  match backend() {
    Backend::Gpu => render_gpu(commands, width, height)
      .or_else(|| render_plain(commands, width, height)),
    Backend::Cpu => render_plain(commands, width, height),
  }
}

fn render_isolated(
  commands: &[Command],
  width: u32,
  height: u32,
) -> Option<Vec<u8>> {
  let mut pixels = vec![0u8; width as usize * height as usize * 4];
  let mut batch = Vec::new();
  for command in commands {
    match command {
      Command::Put { image, dx, dy } => {
        flush_plain(&mut pixels, &mut batch, width, height);
        put_premul(&mut pixels, image, *dx, *dy, width, height);
      }
      Command::Clear {
        rect,
        transform,
        clips,
      } => {
        flush_plain(&mut pixels, &mut batch, width, height);
        clear_premul(&mut pixels, *rect, *transform, clips, width, height);
      }
      other if composite::needs_isolated(other) => {
        flush_plain(&mut pixels, &mut batch, width, height);
        composite::apply_one(&mut pixels, other, width, height, &|list| {
          render_plain(list, width, height)
        });
      }
      other => batch.push(other.clone()),
    }
  }
  flush_plain(&mut pixels, &mut batch, width, height);
  Some(pixels)
}

fn flush_plain(
  pixels: &mut [u8],
  batch: &mut Vec<Command>,
  width: u32,
  height: u32,
) {
  if batch.is_empty() {
    return;
  }
  let rendered = render_plain(batch, width, height);
  batch.clear();
  if let Some(rendered) = rendered {
    composite::source_over(pixels, &rendered);
  }
}

fn put_premul(
  dest: &mut [u8],
  image: &ImageBuf,
  dx: i32,
  dy: i32,
  width: u32,
  height: u32,
) {
  let premul = premul_rgba8(image);
  for y in 0..image.height as i32 {
    let dest_y = dy + y;
    if dest_y < 0 || dest_y >= height as i32 {
      continue;
    }
    for x in 0..image.width as i32 {
      let dest_x = dx + x;
      if dest_x < 0 || dest_x >= width as i32 {
        continue;
      }
      let src_i = (y as u32 * image.width + x as u32) as usize * 4;
      let dest_i = (dest_y as u32 * width + dest_x as u32) as usize * 4;
      dest[dest_i..dest_i + 4].copy_from_slice(&premul[src_i..src_i + 4]);
    }
  }
}

fn clear_premul(
  dest: &mut [u8],
  rect: Rect,
  transform: Affine,
  clips: &[crate::paint::record::Clip],
  width: u32,
  height: u32,
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
      if y < 0 || y >= height as i32 {
        continue;
      }
      for x in x0..x1 {
        if x < 0 || x >= width as i32 {
          continue;
        }
        let index = (y as u32 * width + x as u32) as usize * 4;
        dest[index..index + 4].fill(0);
      }
    }
    return;
  }
  let mask = render_plain(
    &[Command::Fill {
      path: rect.to_path(0.1),
      fill: Fill::NonZero,
      brush: Paint::Solid(CssColor::BLACK),
      transform,
      brush_transform: Affine::IDENTITY,
      alpha: 1.0,
      shade: Shade::source_over(),
      filter: crate::paint::record::empty_filter(),
      clips: Arc::from(clips),
    }],
    width,
    height,
  );
  let Some(mask) = mask else {
    return;
  };
  let pixels = width as usize * height as usize;
  for pixel in 0..pixels {
    if mask[pixel * 4 + 3] > 0 {
      dest[pixel * 4..pixel * 4 + 4].fill(0);
    }
  }
}

fn paint_isolated(image: &mut Raster, command: &Command) {
  let width = image.width;
  let height = image.height;
  let original = image.clone();
  let mut premul = raster_to_premul_srgb8(image);
  let before = premul.clone();
  composite::apply_one(&mut premul, command, width, height, &|list| {
    render_plain(list, width, height)
  });
  let mut updated = premul_srgb8_to_raster(
    &premul,
    width,
    height,
    original.color_space,
    original.format,
  );
  restore_unchanged(&mut updated, &original, &premul, &before);
  *image = updated;
}

fn raster_to_premul_srgb8(image: &Raster) -> Vec<u8> {
  let mut raster = image.clone();
  if raster.premultiplied {
    unpremultiply(&mut raster);
  }
  convert(&mut raster, ColorSpace::Srgb);
  if raster.format != SampleFormat::Unorm8 {
    raster = convert_format(raster, SampleFormat::Unorm8);
  }
  premultiply(&mut raster);
  raster.bytes
}

fn premul_srgb8_to_raster(
  bytes: &[u8],
  width: u32,
  height: u32,
  space: ColorSpace,
  format: SampleFormat,
) -> Raster {
  let mut raster = Raster::from_rgba8(width, height, ColorSpace::Srgb, bytes)
    .expect("canvas size was checked");
  raster.premultiplied = true;
  unpremultiply(&mut raster);
  convert(&mut raster, space);
  if raster.format != format {
    raster = convert_format(raster, format);
  }
  raster.premultiplied = false;
  raster
}

fn restore_unchanged(
  dest: &mut Raster,
  original: &Raster,
  after: &[u8],
  before: &[u8],
) {
  let pixels = dest.width as usize * dest.height as usize;
  for pixel in 0..pixels {
    let start = pixel * 4;
    if after[start..start + 4] == before[start..start + 4] {
      let x = (pixel % dest.width as usize) as i64;
      let y = (pixel / dest.width as usize) as i64;
      dest.write_sample(x, y, original.sample(x, y));
    }
  }
}

fn render_plain(
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
  let pixmap = vello_cpu::PixmapMut::new(width, height, &mut buffer)?;
  context.render_with(
    pixmap,
    &mut resources,
    vello_cpu::RasterizerSettings {
      render_mode: vello_cpu::RenderMode::OptimizeQuality,
      ..Default::default()
    },
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
      shade: _,
      filter: _,
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
      shade: _,
      filter: _,
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
      shade: _,
      filter: _,
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
        let _ = run.stroke_glyphs(glyphs);
      } else {
        let _ = run.fill_glyphs(glyphs);
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
      context.set_paint(vello_cpu::peniko::Color::new([0.0, 0.0, 0.0, 1.0]));
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
      shade: _,
      filter: _,
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
    context.pop_clip();
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
    None => {
      context.set_paint(vello_cpu::peniko::Color::new([0.0, 0.0, 0.0, 0.0]))
    }
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
      // The same center and radius have no circles to interpolate.
      // HTML paints nothing in that case. vello_cpu otherwise covers
      // the shape with a color stop.
      if start.0 == end.0 && start.1 == end.1 && start.2 == end.2 {
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
    Paint::Conic {
      start,
      center,
      stops,
    } => {
      if stops.is_empty() {
        return None;
      }
      let stops = peniko_stops(stops);
      // Offset 0 sits at `start` and offset 1 is the same ray one turn later.
      // Repeat maps the arc before the start angle onto the end of that ramp.
      // Pad would clamp it to the first stop.
      let mut gradient =
        Gradient::new_sweep(*center, *start, *start + std::f32::consts::TAU)
          .with_stops(&stops[..])
          .with_extend(Extend::Repeat);
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

fn peniko_stops(
  stops: &[(f32, CssColor)],
) -> Vec<(f32, vello_cpu::peniko::Color)> {
  let mut stops: Vec<(f32, vello_cpu::peniko::Color)> = stops
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
    data: vello_cpu::peniko::Blob::from(bytes.to_vec()),
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
  let texture = render_gpu_texture(commands, width, height)?;
  yuzuko_window::with_device(|device, queue| {
    readback(device, queue, &texture, width, height)
  })
  .flatten()
}

/// GPU render of `commands` with no readback. `None` on the CPU backend.
pub fn render_texture(
  commands: &[Command],
  width: u32,
  height: u32,
) -> Option<wgpu::Texture> {
  if width == 0 || height == 0 || commands.iter().any(composite::needs_isolated)
  {
    return None;
  }
  match backend() {
    Backend::Gpu => render_gpu_texture(commands, width, height),
    Backend::Cpu => None,
  }
}

fn render_gpu_texture(
  commands: &[Command],
  width: u32,
  height: u32,
) -> Option<wgpu::Texture> {
  if GPU_BROKEN.with(Cell::get) {
    return None;
  }
  let width = u16::try_from(width).ok()?;
  let height = u16::try_from(height).ok()?;
  yuzuko_window::with_device(|device, queue| {
    let rendered =
      std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        draw_gpu_texture(device, queue, commands, width, height)
      }));
    match rendered {
      Ok(texture) => texture,
      Err(_) => {
        GPU_BROKEN.with(|flag| flag.set(true));
        GPU.with(|slot| *slot.borrow_mut() = None);
        None
      }
    }
  })
  .flatten()
}

fn draw_gpu_texture(
  device: &wgpu::Device,
  queue: &wgpu::Queue,
  commands: &[Command],
  width: u16,
  height: u16,
) -> Option<wgpu::Texture> {
  let mut scene = vello_gpu::Scene::new(width, height);
  let mut bindings = vello_gpu::TextureBindings::new();
  let mut textures = Vec::new();
  let mut next_id = 1u64;
  let recorded = GPU.with(|slot| {
    let mut slot = slot.borrow_mut();
    let state = slot.as_mut()?;
    let mut draw = GpuDraw {
      scene: &mut scene,
      resources: &mut state.resources,
      device,
      queue,
      bindings: &mut bindings,
      textures: &mut textures,
      next_id: &mut next_id,
    };
    Some(
      commands
        .iter()
        .all(|command| replay_gpu(&mut draw, command)),
    )
  })?;
  if !recorded {
    // Drop pending image uploads before a later submit observes them.
    queue.submit(std::iter::empty::<wgpu::CommandBuffer>());
    return None;
  }
  let texture = device.create_texture(&wgpu::TextureDescriptor {
    label: Some("yuzuko-canvas"),
    size: wgpu::Extent3d {
      width: u32::from(width),
      height: u32::from(height),
      depth_or_array_layers: 1,
    },
    mip_level_count: 1,
    sample_count: 1,
    dimension: wgpu::TextureDimension::D2,
    format: wgpu::TextureFormat::Rgba8Unorm,
    usage: wgpu::TextureUsages::RENDER_ATTACHMENT
      | wgpu::TextureUsages::COPY_SRC
      | wgpu::TextureUsages::TEXTURE_BINDING,
    view_formats: &[],
  });
  let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
  GPU.with(|slot| {
    let mut slot = slot.borrow_mut();
    let state = slot.as_mut()?;
    let depth = depth_view(&mut state.depth, device, width, height);
    let mut encoder =
      device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("yuzuko-canvas"),
      });
    match state.renderer.render(
      &scene,
      &mut state.resources,
      device,
      queue,
      &mut encoder,
      &vello_gpu::RenderSize { width, height },
      &view,
      depth.as_ref(),
      &bindings,
      vello_gpu::TargetInit::Clear(vello_gpu::ClearSettings::default()),
    ) {
      Ok(()) => {
        queue.submit([encoder.finish()]);
        Some(())
      }
      Err(_) => {
        drop(encoder);
        queue.submit(std::iter::empty::<wgpu::CommandBuffer>());
        None
      }
    }
  })?;
  Some(texture)
}

fn depth_view(
  depth: &mut Option<DepthCache>,
  device: &wgpu::Device,
  width: u16,
  height: u16,
) -> Option<wgpu::TextureView> {
  if let Some(DepthCache::Unavailable) = depth {
    return None;
  }
  if let Some(DepthCache::View(cached_width, cached_height, view)) = depth
    && *cached_width == width
    && *cached_height == height
  {
    return Some(view.clone());
  }
  let created = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
    vello_gpu::Renderer::create_depth_texture_view(
      device,
      &vello_gpu::RenderSize { width, height },
    )
  }));
  match created {
    Ok(view) => {
      let cloned = view.clone();
      *depth = Some(DepthCache::View(width, height, view));
      Some(cloned)
    }
    Err(_) => {
      *depth = Some(DepthCache::Unavailable);
      None
    }
  }
}

struct GpuDraw<'a> {
  scene: &'a mut vello_gpu::Scene,
  resources: &'a mut vello_gpu::Resources,
  device: &'a wgpu::Device,
  queue: &'a wgpu::Queue,
  bindings: &'a mut vello_gpu::TextureBindings,
  textures: &'a mut Vec<wgpu::Texture>,
  next_id: &'a mut u64,
}

fn replay_gpu(draw: &mut GpuDraw<'_>, command: &Command) -> bool {
  match command {
    Command::Fill {
      path,
      fill,
      brush,
      transform,
      brush_transform,
      alpha,
      shade: _,
      filter: _,
      clips,
    } => {
      if !starts_with_move(path) {
        return true;
      }
      push_gpu_clips(draw.scene, clips);
      let pattern = pattern_clip(brush, *transform, *brush_transform);
      if let Some(clip) = pattern.as_ref() {
        push_gpu_clips(draw.scene, std::slice::from_ref(clip));
      }
      push_gpu_opacity(draw.scene, *alpha);
      if !paint_gpu(draw, brush, *brush_transform) {
        return false;
      }
      draw.scene.set_fill_rule(*fill);
      draw.scene.set_transform(*transform);
      draw.scene.set_blend_mode(source_over());
      draw.scene.fill_path(path);
      pop_gpu_opacity(draw.scene, *alpha);
      if pattern.is_some() {
        pop_gpu_clips(draw.scene, 1);
      }
      pop_gpu_clips(draw.scene, clips.len());
      true
    }
    Command::Stroke {
      path,
      stroke,
      brush,
      transform,
      brush_transform,
      alpha,
      shade: _,
      filter: _,
      clips,
    } => {
      if !starts_with_move(path) {
        return true;
      }
      push_gpu_clips(draw.scene, clips);
      let pattern = pattern_clip(brush, *transform, *brush_transform);
      if let Some(clip) = pattern.as_ref() {
        push_gpu_clips(draw.scene, std::slice::from_ref(clip));
      }
      push_gpu_opacity(draw.scene, *alpha);
      if !paint_gpu(draw, brush, *brush_transform) {
        return false;
      }
      draw.scene.set_stroke(stroke.clone());
      draw.scene.set_transform(*transform);
      draw.scene.set_blend_mode(source_over());
      draw.scene.stroke_path(path);
      pop_gpu_opacity(draw.scene, *alpha);
      if pattern.is_some() {
        pop_gpu_clips(draw.scene, 1);
      }
      pop_gpu_clips(draw.scene, clips.len());
      true
    }
    Command::Glyphs {
      font,
      size,
      glyphs,
      brush,
      transform,
      alpha,
      stroke,
      shade: _,
      filter: _,
      clips,
    } => {
      push_gpu_clips(draw.scene, clips);
      push_gpu_opacity(draw.scene, *alpha);
      if !paint_gpu(draw, brush, Affine::IDENTITY) {
        return false;
      }
      draw.scene.set_transform(*transform);
      draw.scene.set_blend_mode(source_over());
      if let Some(stroke) = stroke {
        draw.scene.set_stroke(stroke.clone());
      }
      let glyphs =
        glyphs
          .iter()
          .copied()
          .map(|(id, x, y)| vello_cpu::Glyph { id, x, y });
      let run = draw.scene.glyph_run(draw.resources, font).font_size(*size);
      if stroke.is_some() {
        let _ = run.stroke_glyphs(glyphs);
      } else {
        let _ = run.fill_glyphs(glyphs);
      }
      pop_gpu_opacity(draw.scene, *alpha);
      pop_gpu_clips(draw.scene, clips.len());
      true
    }
    Command::Clear {
      rect,
      transform,
      clips,
    } => {
      push_gpu_clips(draw.scene, clips);
      // `set_blend_mode` panics on Clear. A destructive blend covers the
      // parent layer, so the blend layer itself is clipped to this rectangle.
      push_gpu_destructive(draw.scene, rect, *transform, Compose::Clear);
      draw
        .scene
        .set_paint(vello_cpu::peniko::Color::new([0.0, 0.0, 0.0, 1.0]));
      draw.scene.fill_rect(rect);
      draw.scene.pop_layer();
      pop_gpu_clips(draw.scene, clips.len());
      true
    }
    Command::Put { image, dx, dy } => {
      let premul = premul_rgba8(image);
      let rect =
        Rect::new(0.0, 0.0, f64::from(image.width), f64::from(image.height));
      // Copy has the same parent-sized blend as Clear. Clip it to the image.
      push_gpu_destructive(
        draw.scene,
        &rect,
        Affine::translate((f64::from(*dx), f64::from(*dy))),
        Compose::Copy,
      );
      if !paint_rgba8(draw, &premul, image.width, image.height, true) {
        draw.scene.pop_layer();
        return false;
      }
      draw.scene.fill_rect(&rect);
      draw.scene.pop_layer();
      true
    }
    Command::Image {
      image,
      transform,
      alpha,
      nearest,
      shade: _,
      filter: _,
      clips,
    } => {
      push_gpu_clips(draw.scene, clips);
      push_gpu_opacity(draw.scene, *alpha);
      draw.scene.set_transform(*transform);
      draw.scene.set_blend_mode(source_over());
      if !paint_rgba8(draw, &image.bytes, image.width, image.height, *nearest) {
        return false;
      }
      draw.scene.fill_rect(&Rect::new(
        0.0,
        0.0,
        f64::from(image.width),
        f64::from(image.height),
      ));
      pop_gpu_opacity(draw.scene, *alpha);
      pop_gpu_clips(draw.scene, clips.len());
      true
    }
  }
}

fn push_gpu_clips(
  scene: &mut vello_gpu::Scene,
  clips: &[crate::paint::record::Clip],
) {
  for clip in clips {
    scene.set_fill_rule(clip.fill);
    scene.set_transform(clip.transform);
    scene.push_clip_path(&clip.path);
  }
}

fn pop_gpu_clips(scene: &mut vello_gpu::Scene, count: usize) {
  for _ in 0..count {
    scene.pop_clip();
  }
}

/// Push an isolated destructive blend limited to `rect`.
///
/// vello_gpu composites Clear and Copy across the parent layer. The clip
/// stored on the blend layer is what reduces that region to the rectangle.
fn push_gpu_destructive(
  scene: &mut vello_gpu::Scene,
  rect: &Rect,
  transform: Affine,
  compose: Compose,
) {
  scene.set_transform(transform);
  scene.set_fill_rule(Fill::NonZero);
  let path = rect.to_path(0.1);
  scene.push_layer(
    Some(&path),
    Some(BlendMode::new(Mix::Normal, compose)),
    None,
    None,
    None,
  );
  scene.set_blend_mode(source_over());
}

fn push_gpu_opacity(scene: &mut vello_gpu::Scene, alpha: f32) {
  if alpha < 1.0 {
    scene.push_opacity_layer(alpha);
  }
}

fn pop_gpu_opacity(scene: &mut vello_gpu::Scene, alpha: f32) {
  if alpha < 1.0 {
    scene.pop_layer();
  }
}

fn paint_gpu(draw: &mut GpuDraw<'_>, brush: &Paint, transform: Affine) -> bool {
  draw.scene.reset_paint_transform();
  if transform != Affine::IDENTITY {
    draw.scene.set_paint_transform(transform);
  }
  match brush_of(brush, true) {
    Some(Brush::Solid(color)) => {
      draw.scene.set_paint(color);
      true
    }
    Some(Brush::Gradient(gradient)) => {
      draw.scene.set_paint(gradient);
      true
    }
    Some(Brush::Image(image)) => {
      match upload_rgba8(
        draw,
        image.image.data.data(),
        image.image.width,
        image.image.height,
      ) {
        ImageUpload::Texture(source) => {
          draw.scene.set_paint(vello_gpu::Image {
            image: source,
            sampler: image.sampler,
          });
          true
        }
        ImageUpload::Empty => {
          draw.scene.set_paint(transparent_color());
          true
        }
        ImageUpload::Unsupported => false,
      }
    }
    None => {
      draw.scene.set_paint(transparent_color());
      true
    }
  }
}

fn paint_rgba8(
  draw: &mut GpuDraw<'_>,
  bytes: &[u8],
  width: u32,
  height: u32,
  nearest: bool,
) -> bool {
  draw.scene.reset_paint_transform();
  match upload_rgba8(draw, bytes, width, height) {
    ImageUpload::Texture(source) => {
      draw.scene.set_paint(vello_gpu::Image {
        image: source,
        sampler: ImageSampler::default().with_quality(if nearest {
          ImageQuality::Low
        } else {
          ImageQuality::Medium
        }),
      });
      true
    }
    ImageUpload::Empty => {
      draw.scene.set_paint(transparent_color());
      true
    }
    ImageUpload::Unsupported => false,
  }
}

fn transparent_color() -> vello_cpu::peniko::Color {
  vello_cpu::peniko::Color::new([0.0, 0.0, 0.0, 0.0])
}

enum ImageUpload {
  Texture(vello_gpu::ImageSource),
  Empty,
  Unsupported,
}

/// `ImageSource::Pixmap` panics in this renderer, and its image atlas cannot
/// be reset from outside the crate. Each image is a texture sampled for this
/// frame. The texture stays alive until the frame is submitted.
fn upload_rgba8(
  draw: &mut GpuDraw<'_>,
  bytes: &[u8],
  width: u32,
  height: u32,
) -> ImageUpload {
  let Ok(width) = u16::try_from(width) else {
    return ImageUpload::Unsupported;
  };
  let Ok(height) = u16::try_from(height) else {
    return ImageUpload::Unsupported;
  };
  if width == 0 || height == 0 {
    return ImageUpload::Empty;
  }
  let row = usize::from(width) * 4;
  let Some(needed) = row.checked_mul(usize::from(height)) else {
    return ImageUpload::Unsupported;
  };
  if bytes.len() < needed {
    return ImageUpload::Unsupported;
  }
  let bytes = &bytes[..needed];
  let id = vello_gpu::TextureId(*draw.next_id);
  *draw.next_id += 1;
  let texture = draw.device.create_texture(&wgpu::TextureDescriptor {
    label: Some("yuzuko-canvas-image"),
    size: wgpu::Extent3d {
      width: u32::from(width),
      height: u32::from(height),
      depth_or_array_layers: 1,
    },
    mip_level_count: 1,
    sample_count: 1,
    dimension: wgpu::TextureDimension::D2,
    format: wgpu::TextureFormat::Rgba8Unorm,
    usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
    view_formats: &[],
  });
  draw.queue.write_texture(
    texture.as_image_copy(),
    bytes,
    wgpu::TexelCopyBufferLayout {
      offset: 0,
      bytes_per_row: Some(u32::from(width) * 4),
      rows_per_image: Some(u32::from(height)),
    },
    texture.size(),
  );
  let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
  draw.bindings.insert(id, view);
  draw.textures.push(texture);
  let transparent =
    bytes.as_chunks::<4>().0.iter().any(|pixel| pixel[3] != 255);
  ImageUpload::Texture(vello_gpu::ImageSource::external_texture(
    id,
    vello_gpu::RectU16::new(0, 0, width, height),
    transparent,
  ))
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
  let view = slice.get_mapped_range().ok()?;
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
  blit_texture(device, queue, &view, target, format);
}

pub fn blit_texture(
  device: &wgpu::Device,
  queue: &wgpu::Queue,
  source: &wgpu::TextureView,
  target: &wgpu::TextureView,
  format: wgpu::TextureFormat,
) {
  let mut encoder =
    device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
      label: Some("yuzuko-canvas-present"),
    });
  BLITTER.with(|slot| {
    let mut slot = slot.borrow_mut();
    if slot.as_ref().is_none_or(|(cached, _)| *cached != format) {
      *slot = Some((format, wgpu::util::TextureBlitter::new(device, format)));
    }
    let Some((_, blitter)) = slot.as_ref() else {
      return;
    };
    blitter.copy(device, &mut encoder, source, target);
  });
  queue.submit([encoder.finish()]);
}

#[cfg(test)]
mod tests {
  use std::sync::Arc;

  use kurbo::Affine;
  use kurbo::Rect;
  use kurbo::Shape;
  use vello_cpu::peniko::Fill;

  use super::render_plain;
  use super::render_premul;
  use crate::paint::color::ColorSyntax;
  use crate::paint::color::CssColor;
  use crate::paint::record::Brush;
  use crate::paint::record::Clip;
  use crate::paint::record::Command;
  use crate::paint::shade::Shade;

  fn color(r: f32, g: f32, b: f32) -> CssColor {
    CssColor {
      r,
      g,
      b,
      a: 1.0,
      syntax: ColorSyntax::Legacy,
    }
  }

  fn rect_path(x: f64, y: f64, w: f64, h: f64) -> kurbo::BezPath {
    Rect::new(x, y, x + w, y + h).to_path(0.1)
  }

  fn fill(
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    paint: CssColor,
    shade: Shade,
  ) -> Command {
    Command::Fill {
      path: rect_path(x, y, w, h),
      fill: Fill::NonZero,
      brush: Brush::Solid(paint),
      transform: Affine::IDENTITY,
      brush_transform: Affine::IDENTITY,
      alpha: 1.0,
      shade,
      filter: crate::paint::record::empty_filter(),
      clips: Arc::from([]),
    }
  }

  fn op(name: &str) -> Shade {
    Shade::from_canvas(name, 0.0, 0.0, 0.0, CssColor::TRANSPARENT)
  }

  fn pixel(bytes: &[u8], width: u32, x: u32, y: u32) -> [u8; 4] {
    let index = ((y * width + x) * 4) as usize;
    [
      bytes[index],
      bytes[index + 1],
      bytes[index + 2],
      bytes[index + 3],
    ]
  }

  #[test]
  fn plain_source_over_replaces_an_opaque_rect() {
    let commands = vec![
      fill(
        0.0,
        0.0,
        4.0,
        4.0,
        color(1.0, 0.0, 0.0),
        Shade::source_over(),
      ),
      fill(
        0.0,
        0.0,
        4.0,
        4.0,
        color(0.0, 1.0, 0.0),
        Shade::source_over(),
      ),
    ];
    let bytes = render_plain(&commands, 4, 4).unwrap();
    assert_eq!(pixel(&bytes, 4, 1, 1), [0, 255, 0, 255]);
  }

  fn gpu_on_this_thread() -> bool {
    super::renderer_name() == "gpu"
      && yuzuko_window::with_device(|_device, _queue| ()).is_some()
  }

  #[test]
  fn gpu_plain_source_over_clear_and_image() {
    if !gpu_on_this_thread() {
      return;
    }
    let commands = vec![
      fill(
        0.0,
        0.0,
        4.0,
        4.0,
        color(1.0, 0.0, 0.0),
        Shade::source_over(),
      ),
      fill(
        0.0,
        0.0,
        4.0,
        4.0,
        color(0.0, 1.0, 0.0),
        Shade::source_over(),
      ),
    ];
    assert!(super::render_texture(&commands, 4, 4).is_some());
    let bytes = render_premul(&commands, 4, 4).unwrap();
    assert_eq!(pixel(&bytes, 4, 1, 1), [0, 255, 0, 255]);

    let cleared = vec![
      fill(
        0.0,
        0.0,
        4.0,
        4.0,
        color(1.0, 0.0, 0.0),
        Shade::source_over(),
      ),
      Command::Clear {
        rect: Rect::new(0.0, 0.0, 2.0, 4.0),
        transform: Affine::IDENTITY,
        clips: Arc::from([]),
      },
    ];
    let bytes = render_premul(&cleared, 4, 4).unwrap();
    assert_eq!(pixel(&bytes, 4, 0, 1), [0, 0, 0, 0]);
    assert_eq!(pixel(&bytes, 4, 3, 1), [255, 0, 0, 255]);

    let copied = vec![
      fill(
        0.0,
        0.0,
        4.0,
        4.0,
        color(1.0, 0.0, 0.0),
        Shade::source_over(),
      ),
      Command::Put {
        image: Arc::new(crate::paint::record::ImageBuf {
          width: 2,
          height: 4,
          display_p3: false,
          float16: false,
          premultiplied: true,
          bytes: [0, 255, 0, 255].repeat(8),
        }),
        dx: 0,
        dy: 0,
      },
    ];
    let bytes = render_premul(&copied, 4, 4).unwrap();
    assert_eq!(pixel(&bytes, 4, 0, 1), [0, 255, 0, 255]);
    assert_eq!(pixel(&bytes, 4, 3, 1), [255, 0, 0, 255]);

    let image = Arc::new(crate::paint::record::ImageBuf {
      width: 4,
      height: 4,
      display_p3: false,
      float16: false,
      premultiplied: true,
      bytes: [128, 0, 0, 128].repeat(16),
    });
    let drawn = Command::Image {
      image,
      transform: Affine::IDENTITY,
      alpha: 1.0,
      nearest: true,
      shade: Shade::source_over(),
      filter: crate::paint::record::empty_filter(),
      clips: Arc::from([]),
    };
    assert!(
      super::render_texture(std::slice::from_ref(&drawn), 4, 4).is_some()
    );
    let bytes = render_premul(std::slice::from_ref(&drawn), 4, 4).unwrap();
    assert_eq!(pixel(&bytes, 4, 1, 1), [128, 0, 0, 128]);
  }

  #[test]
  fn source_in_and_copy_clear_uncovered_pixels() {
    let source_in = vec![
      fill(
        0.0,
        0.0,
        8.0,
        4.0,
        color(1.0, 0.0, 0.0),
        Shade::source_over(),
      ),
      fill(0.0, 0.0, 4.0, 4.0, color(0.0, 0.0, 1.0), op("source-in")),
    ];
    let bytes = render_premul(&source_in, 8, 4).unwrap();
    assert_eq!(pixel(&bytes, 8, 1, 1), [0, 0, 255, 255]);
    assert_eq!(pixel(&bytes, 8, 6, 1), [0, 0, 0, 0]);

    let copy = vec![
      fill(
        0.0,
        0.0,
        8.0,
        4.0,
        color(1.0, 0.0, 0.0),
        Shade::source_over(),
      ),
      fill(0.0, 0.0, 2.0, 4.0, color(0.0, 0.0, 1.0), op("copy")),
    ];
    let bytes = render_premul(&copy, 8, 4).unwrap();
    assert_eq!(pixel(&bytes, 8, 0, 1), [0, 0, 255, 255]);
    assert_eq!(pixel(&bytes, 8, 4, 1), [0, 0, 0, 0]);
  }

  #[test]
  fn destination_over_lighter_and_multiply() {
    let dest_over = vec![
      fill(
        2.0,
        0.0,
        2.0,
        4.0,
        color(1.0, 0.0, 0.0),
        Shade::source_over(),
      ),
      fill(
        0.0,
        0.0,
        8.0,
        4.0,
        color(0.0, 0.0, 1.0),
        op("destination-over"),
      ),
    ];
    let bytes = render_premul(&dest_over, 8, 4).unwrap();
    assert_eq!(pixel(&bytes, 8, 0, 1), [0, 0, 255, 255]);
    assert_eq!(pixel(&bytes, 8, 3, 1), [255, 0, 0, 255]);

    let lighter = vec![
      fill(
        0.0,
        0.0,
        4.0,
        4.0,
        color(0.0, 1.0, 1.0),
        Shade::source_over(),
      ),
      fill(0.0, 0.0, 4.0, 4.0, color(1.0, 1.0, 0.0), op("lighter")),
    ];
    let bytes = render_premul(&lighter, 4, 4).unwrap();
    assert_eq!(pixel(&bytes, 4, 1, 1), [255, 255, 255, 255]);

    let multiply = vec![
      fill(
        0.0,
        0.0,
        4.0,
        4.0,
        color(1.0, 1.0, 0.0),
        Shade::source_over(),
      ),
      fill(0.0, 0.0, 4.0, 4.0, color(0.0, 0.0, 1.0), op("multiply")),
    ];
    let bytes = render_premul(&multiply, 4, 4).unwrap();
    assert_eq!(pixel(&bytes, 4, 1, 1), [0, 0, 0, 255]);
  }

  #[test]
  fn shadow_offset_ignores_transform_and_respects_clip() {
    let mut shadowed = fill(
      0.0,
      0.0,
      2.0,
      2.0,
      color(1.0, 0.0, 0.0),
      Shade::from_canvas("source-over", 0.0, 2.0, 0.0, CssColor::BLACK),
    );
    if let Command::Fill { transform, .. } = &mut shadowed {
      *transform = Affine::scale_non_uniform(2.0, 2.0);
    }
    let bytes = render_premul(std::slice::from_ref(&shadowed), 10, 6).unwrap();
    assert_eq!(pixel(&bytes, 10, 1, 2), [255, 0, 0, 255]);
    assert_eq!(pixel(&bytes, 10, 5, 2), [0, 0, 0, 255]);
    assert_eq!(pixel(&bytes, 10, 7, 2), [0, 0, 0, 0]);

    let mut clipped = fill(
      0.0,
      0.0,
      4.0,
      4.0,
      color(1.0, 0.0, 0.0),
      Shade::from_canvas("source-over", 0.0, 4.0, 0.0, color(0.0, 0.0, 1.0)),
    );
    if let Command::Fill { clips, .. } = &mut clipped {
      *clips = Arc::from([Clip {
        path: rect_path(4.0, 0.0, 4.0, 4.0),
        fill: Fill::NonZero,
        transform: Affine::IDENTITY,
      }]);
    }
    let commands = vec![
      fill(
        0.0,
        0.0,
        8.0,
        4.0,
        color(0.0, 1.0, 0.0),
        Shade::source_over(),
      ),
      clipped,
    ];
    let bytes = render_premul(&commands, 8, 4).unwrap();
    assert_eq!(pixel(&bytes, 8, 1, 2), [0, 255, 0, 255]);
    assert_eq!(pixel(&bytes, 8, 6, 2), [0, 0, 255, 255]);

    let outside = fill(
      -4.0,
      0.0,
      4.0,
      4.0,
      color(1.0, 0.0, 0.0),
      Shade::from_canvas("source-over", 0.0, 4.0, 0.0, color(0.0, 0.0, 1.0)),
    );
    let commands = vec![
      fill(
        0.0,
        0.0,
        8.0,
        4.0,
        color(1.0, 0.0, 0.0),
        Shade::source_over(),
      ),
      outside,
    ];
    let bytes = render_premul(&commands, 8, 4).unwrap();
    assert_eq!(pixel(&bytes, 8, 1, 1), [0, 0, 255, 255]);
    assert_eq!(pixel(&bytes, 8, 6, 1), [255, 0, 0, 255]);
  }

  fn float16_canvas(
    space: yuzuko_image::ColorSpace,
    commands: Vec<Command>,
  ) -> yuzuko_image::Raster {
    use crate::paint::record::Attrs;
    use crate::paint::record::Drawing;
    use yuzuko_image::SampleFormat;
    let drawing = Drawing::from_commands(
      1,
      1,
      Attrs {
        color_space: space,
        format: SampleFormat::Float16,
        ..Attrs::default()
      },
      commands,
    );
    super::rasterize(&drawing).unwrap()
  }

  #[test]
  fn float16_put_keeps_wide_gamut_unorm8_within_two_codes() {
    use yuzuko_image::ColorSpace;
    let p3_on_srgb = float16_canvas(
      ColorSpace::Srgb,
      vec![Command::Put {
        image: Arc::new(crate::paint::record::ImageBuf {
          width: 1,
          height: 1,
          display_p3: true,
          float16: false,
          premultiplied: false,
          bytes: vec![5, 250, 128, 255],
        }),
        dx: 0,
        dy: 0,
      }],
    );
    let stored = p3_on_srgb.sample(0, 0);
    assert!(stored[0] < 0.0, "{stored:?}");
    let back = yuzuko_image::convert_encoded(
      ColorSpace::Srgb,
      ColorSpace::DisplayP3,
      [stored[0], stored[1], stored[2]],
    );
    let unorm = back.map(|channel| (channel * 255.0).round());
    assert!((unorm[0] - 5.0).abs() <= 2.0, "{unorm:?} from {stored:?}");
    assert!((unorm[1] - 250.0).abs() <= 2.0, "{unorm:?}");
    assert!((unorm[2] - 128.0).abs() <= 2.0, "{unorm:?}");

    let srgb_on_p3 = float16_canvas(
      ColorSpace::DisplayP3,
      vec![Command::Put {
        image: Arc::new(crate::paint::record::ImageBuf {
          width: 1,
          height: 1,
          display_p3: false,
          float16: false,
          premultiplied: false,
          bytes: vec![5, 250, 128, 255],
        }),
        dx: 0,
        dy: 0,
      }],
    );
    let stored = srgb_on_p3.sample(0, 0);
    let back = yuzuko_image::convert_encoded(
      ColorSpace::DisplayP3,
      ColorSpace::Srgb,
      [stored[0], stored[1], stored[2]],
    );
    let unorm = back.map(|channel| (channel * 255.0).round());
    assert!((unorm[0] - 5.0).abs() <= 2.0, "{unorm:?} from {stored:?}");
    assert!((unorm[1] - 250.0).abs() <= 2.0, "{unorm:?}");
    assert!((unorm[2] - 128.0).abs() <= 2.0, "{unorm:?}");
  }

  #[test]
  fn float16_solid_fill_round_trips_display_p3_red() {
    use yuzuko_image::ColorSpace;
    let filled = float16_canvas(
      ColorSpace::Srgb,
      vec![Command::Fill {
        path: crate::geom::path::rect_path(0.0, 0.0, 1.0, 1.0),
        fill: Fill::NonZero,
        brush: Brush::Solid(CssColor {
          r: 1.0,
          g: 0.0,
          b: 0.0,
          a: 1.0,
          syntax: ColorSyntax::DisplayP3,
        }),
        transform: Affine::IDENTITY,
        brush_transform: Affine::IDENTITY,
        alpha: 1.0,
        shade: Shade::source_over(),
        filter: crate::paint::record::empty_filter(),
        clips: Arc::from([]),
      }],
    );
    let stored = filled.sample(0, 0);
    let back = yuzuko_image::convert_encoded(
      ColorSpace::Srgb,
      ColorSpace::DisplayP3,
      [stored[0], stored[1], stored[2]],
    );
    assert!((back[0] - 1.0).abs() < 0.01, "{back:?} from {stored:?}");
    assert!(back[1].abs() < 0.01, "{back:?}");
    assert!(back[2].abs() < 0.01, "{back:?}");
    assert!((stored[3] - 1.0).abs() < 1.0e-3);
  }
}
