//! `GPUCanvasContext` for `OffscreenCanvas` and `Window`.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use yuzuko_core::op::{Js, OpError};
use yuzuko_image::ColorSpace;
use yuzuko_image::Raster;
use yuzuko_image::SampleFormat;
use yuzuko_image::convert;
use yuzuko_image::{FramePixels, frame_pixels};
use yuzuko_macros::op;
use yuzuko_window::ContextKind;
use yuzuko_window::OffscreenCanvas;

use crate::js;
use crate::resource::{self, GpuTexture};
use crate::state::{self, Handle};

struct Config {
  device_id: u64,
  format: wgpu::TextureFormat,
  usage: u32,
  alpha_opaque: bool,
  color_space: ColorSpace,
  view_formats: Vec<wgpu::TextureFormat>,
}

struct Current {
  texture: Rc<RefCell<Option<wgpu::Texture>>>,
  destroyed: Arc<AtomicBool>,
  object: v8::Global<v8::Object>,
  width: u32,
  height: u32,
}

struct Slot {
  config: Option<Config>,
  current: Option<Current>,
}

std::thread_local! {
  static CANVASES: RefCell<HashMap<u64, Slot>> = RefCell::new(HashMap::new());
}

pub struct GpuCanvasContext {
  id: u64,
  canvas: RefCell<v8::TracedReference<v8::Object>>,
}

unsafe impl v8::cppgc::GarbageCollected for GpuCanvasContext {
  fn trace(&self, visitor: &mut v8::cppgc::Visitor) {
    if let Ok(canvas) = self.canvas.try_borrow() {
      visitor.trace(&*canvas);
    }
  }

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"GPUCanvasContext"
  }
}

impl GpuCanvasContext {
  pub fn create(
    scope: &mut v8::PinScope,
    id: u64,
    canvas: v8::Local<v8::Object>,
  ) -> Self {
    CANVASES.with(|canvases| {
      canvases.borrow_mut().entry(id).or_insert_with(|| Slot {
        config: None,
        current: None,
      });
    });
    Self {
      id,
      canvas: RefCell::new(yuzuko_core::op::traced(scope, canvas)),
    }
  }
}

#[op(name = "GPUCanvasContext")]
impl GpuCanvasContext {
  #[getter]
  fn label(&self) -> String {
    String::new()
  }

  #[getter]
  fn canvas(&self, scope: &mut v8::PinScope) -> Js {
    let value = yuzuko_core::op::traced_local(scope, &self.canvas.borrow())
      .map(v8::Local::<v8::Value>::from)
      .unwrap_or_else(|| v8::null(scope).into());
    Js(v8::Global::new(scope, value))
  }

  fn configure(
    &self,
    scope: &mut v8::PinScope,
    descriptor: v8::Local<v8::Value>,
  ) -> Result<(), OpError> {
    let object = js::object(scope, descriptor)
      .ok_or_else(|| OpError::type_("configure requires a descriptor"))?;
    let device = js::get(scope, object, "device").ok_or_else(|| {
      OpError::type_("GPUCanvasConfiguration.device is required")
    })?;
    let device_id = crate::device_id(scope, device).ok_or_else(|| {
      OpError::type_("GPUCanvasConfiguration.device is not a GPUDevice")
    })?;
    let _ = state::handle(device_id)?;
    let format_name = js::get(scope, object, "format")
      .and_then(|value| js::string(scope, value))
      .ok_or_else(|| {
        OpError::type_("GPUCanvasConfiguration.format is required")
      })?;
    let format = resource::texture_format(&format_name)?;
    let usage = js::field_u32(scope, object, "usage")?
      .unwrap_or(wgpu::TextureUsages::RENDER_ATTACHMENT.bits());
    let alpha = js::get(scope, object, "alphaMode")
      .and_then(|value| js::string(scope, value))
      .unwrap_or_else(|| "opaque".to_string());
    let alpha_opaque = match alpha.as_str() {
      "opaque" => true,
      "premultiplied" => false,
      _ => {
        return Err(OpError::type_("The canvas alpha mode is invalid"));
      }
    };
    let color_space = js::get(scope, object, "colorSpace")
      .and_then(|value| js::string(scope, value))
      .unwrap_or_else(|| "srgb".to_string());
    let color_space = match color_space.as_str() {
      "srgb" => ColorSpace::Srgb,
      "display-p3" => ColorSpace::DisplayP3,
      _ => {
        return Err(OpError::type_("The canvas color space is invalid"));
      }
    };
    let mut view_formats = Vec::new();
    if let Some(list) = js::get(scope, object, "viewFormats") {
      let array = v8::Local::<v8::Array>::try_from(list)
        .map_err(|_| OpError::type_("viewFormats must be an array"))?;
      for index in 0..array.length() {
        let name = js::string(scope, array.get_index(scope, index).unwrap())
          .unwrap_or_default();
        view_formats.push(resource::texture_format(&name)?);
      }
    }
    expire_current(self.id);
    CANVASES.with(|canvases| {
      let mut canvases = canvases.borrow_mut();
      let slot = canvases.entry(self.id).or_insert_with(|| Slot {
        config: None,
        current: None,
      });
      slot.config = Some(Config {
        device_id,
        format,
        usage,
        alpha_opaque,
        color_space,
        view_formats,
      });
    });
    Ok(())
  }

  fn unconfigure(&self) {
    expire_current(self.id);
    CANVASES.with(|canvases| {
      if let Some(slot) = canvases.borrow_mut().get_mut(&self.id) {
        slot.config = None;
      }
    });
  }

  fn get_current_texture(
    &self,
    scope: &mut v8::PinScope,
  ) -> Result<Js, OpError> {
    let (device_id, format, usage, view_formats) =
      CANVASES.with(|canvases| {
        let canvases = canvases.borrow();
        let slot = canvases.get(&self.id);
        let config = slot.and_then(|slot| slot.config.as_ref());
        let Some(config) = config else {
          return Err(OpError::dom(
            "InvalidStateError",
            "The canvas context is not configured",
          ));
        };
        Ok((
          config.device_id,
          config.format,
          config.usage,
          config.view_formats.clone(),
        ))
      })?;
    let handle = state::handle(device_id)?;
    let (width, height) = self.bitmap_size(scope)?;
    let cached = CANVASES.with(|canvases| {
      let canvases = canvases.borrow();
      let current = canvases.get(&self.id)?.current.as_ref()?;
      if current.destroyed.load(Ordering::SeqCst)
        || current.width != width
        || current.height != height
      {
        return None;
      }
      Some(current.object.clone())
    });
    if let Some(object) = cached {
      let local = v8::Local::new(scope, &object);
      let value: v8::Local<v8::Value> = local.into();
      return Ok(Js(v8::Global::new(scope, value)));
    }
    expire_current(self.id);
    let usage = wgpu::TextureUsages::from_bits_truncate(usage)
      | wgpu::TextureUsages::TEXTURE_BINDING
      | wgpu::TextureUsages::COPY_SRC
      | wgpu::TextureUsages::RENDER_ATTACHMENT;
    let texture = handle.device.create_texture(&wgpu::TextureDescriptor {
      label: Some("yuzuko-webgpu-canvas"),
      size: wgpu::Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
      },
      mip_level_count: 1,
      sample_count: 1,
      dimension: wgpu::TextureDimension::D2,
      format,
      usage,
      view_formats: &view_formats,
    });
    let gpu_texture = GpuTexture::from_parts(
      texture,
      Arc::new(AtomicBool::new(false)),
      Arc::clone(&handle.lost),
      width,
      height,
      format,
    );
    let texture_slot = gpu_texture.texture_slot();
    let destroyed = gpu_texture.destroyed_flag();
    let object = unsafe { yuzuko_core::op::wrap_instance(scope, gpu_texture) };
    CANVASES.with(|canvases| {
      if let Some(slot) = canvases.borrow_mut().get_mut(&self.id) {
        slot.current = Some(Current {
          texture: texture_slot,
          destroyed,
          object: v8::Global::new(scope, object),
          width,
          height,
        });
      }
    });
    let value: v8::Local<v8::Value> = object.into();
    Ok(Js(v8::Global::new(scope, value)))
  }
}

impl GpuCanvasContext {
  fn bitmap_size(
    &self,
    scope: &mut v8::PinScope,
  ) -> Result<(u32, u32), OpError> {
    let object = {
      let canvas = self.canvas.borrow();
      yuzuko_core::op::traced_local(scope, &canvas)
    };
    let object = object.ok_or_else(|| {
      OpError::dom("InvalidStateError", "The canvas has no pixels")
    })?;
    let value: v8::Local<v8::Value> = object.into();
    let (_id, width, height) = yuzuko_window::offscreen_canvas(scope, value)
      .ok_or_else(|| {
        OpError::dom("InvalidStateError", "The canvas has no pixels")
      })?;
    let width = bitmap_dimension(width)?;
    let height = bitmap_dimension(height)?;
    if width == 0 || height == 0 {
      return Err(OpError::dom(
        "InvalidStateError",
        "The canvas has no pixels",
      ));
    }
    Ok((width, height))
  }
}

pub fn get_context(
  scope: &mut v8::PinScope,
  canvas: &OffscreenCanvas,
  this: v8::Global<v8::Object>,
  type_name: String,
  _options: Option<v8::Global<v8::Value>>,
) -> Result<Js, OpError> {
  if type_name != "webgpu" {
    return Ok(Js::null(scope));
  }
  if canvas.context_kind() == ContextKind::WebGpu
    && let Some(object) = canvas.context_object(scope)
  {
    let value: v8::Local<v8::Value> = object.into();
    return Ok(Js(v8::Global::new(scope, value)));
  }
  if canvas.context_kind() != ContextKind::None {
    return Ok(Js::null(scope));
  }
  let this = v8::Local::new(scope, &this);
  let context = GpuCanvasContext::create(scope, canvas.id(), this);
  let object = unsafe { yuzuko_core::op::wrap_instance(scope, context) };
  canvas.store_context(scope, ContextKind::WebGpu, object);
  let value: v8::Local<v8::Value> = object.into();
  Ok(Js(v8::Global::new(scope, value)))
}

enum StagedGpu {
  Blit {
    texture: wgpu::Texture,
    alpha_opaque: bool,
    color_space: ColorSpace,
  },
  Fitted {
    pixels: Vec<u8>,
    width: u32,
    height: u32,
  },
}

thread_local! {
  static STAGED: RefCell<Option<(u64, StagedGpu)>> =
    const { RefCell::new(None) };
}

pub fn stage(
  device: &wgpu::Device,
  queue: &wgpu::Queue,
  id: u64,
  target_width: u32,
  target_height: u32,
) -> bool {
  let taken = CANVASES.with(|canvases| {
    let mut canvases = canvases.borrow_mut();
    let slot = canvases.get_mut(&id)?;
    let config = slot.config.as_ref()?;
    let alpha_opaque = config.alpha_opaque;
    let color_space = config.color_space;
    let current = slot.current.take()?;
    Some((current, alpha_opaque, color_space))
  });
  let Some((current, alpha_opaque, color_space)) = taken else {
    return false;
  };
  if current.destroyed.load(Ordering::SeqCst) {
    return false;
  }
  let texture = current.texture.borrow_mut().take();
  current.destroyed.store(true, Ordering::SeqCst);
  let Some(texture) = texture else {
    return false;
  };
  let style = yuzuko_window::canvas_style(id);
  let source_width = texture.size().width;
  let source_height = texture.size().height;
  let fitted = target_width != 0
    && target_height != 0
    && (source_width != target_width || source_height != target_height)
    && !(style.fit == yuzuko_window::ObjectFit::Fill
      && yuzuko_window::rendering_is_smooth(style.rendering));
  let staged = if fitted {
    read_rgba(device, queue, &texture).map(|pixels| {
      let pad = if yuzuko_window::canvas_is_transparent(id) {
        0
      } else {
        255
      };
      let pixels = yuzuko_window::composite_pad(
        &pixels,
        source_width,
        source_height,
        target_width,
        target_height,
        style.fit,
        style.rendering,
        pad,
      );
      StagedGpu::Fitted {
        pixels,
        width: target_width,
        height: target_height,
      }
    })
  } else {
    None
  };
  let staged = staged.unwrap_or(StagedGpu::Blit {
    texture,
    alpha_opaque,
    color_space,
  });
  STAGED.with(|slot| *slot.borrow_mut() = Some((id, staged)));
  true
}

pub fn discard_staged() {
  STAGED.with(|slot| *slot.borrow_mut() = None);
}

pub fn present(
  device: &wgpu::Device,
  queue: &wgpu::Queue,
  target: &wgpu::TextureView,
  id: u64,
  _width: u32,
  _height: u32,
  format: wgpu::TextureFormat,
) -> bool {
  let Some((staged_id, staged)) = STAGED.with(|slot| slot.borrow_mut().take())
  else {
    return false;
  };
  if staged_id != id {
    return false;
  }
  match staged {
    StagedGpu::Blit {
      texture,
      alpha_opaque,
      color_space,
    } => {
      let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
      blit_canvas(
        device,
        queue,
        &view,
        target,
        format,
        alpha_opaque,
        color_space,
      );
    }
    StagedGpu::Fitted {
      pixels,
      width,
      height,
    } => {
      blit_rgba(device, queue, target, format, &pixels, width, height);
    }
  }
  true
}

fn read_rgba(
  device: &wgpu::Device,
  queue: &wgpu::Queue,
  texture: &wgpu::Texture,
) -> Option<Vec<u8>> {
  let width = texture.size().width;
  let height = texture.size().height;
  if width == 0 || height == 0 {
    return None;
  }
  let swap = match texture.format() {
    wgpu::TextureFormat::Rgba8Unorm | wgpu::TextureFormat::Rgba8UnormSrgb => {
      false
    }
    wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb => {
      true
    }
    _ => return None,
  };
  let tight = width as usize * 4;
  let stride = tight.div_ceil(256) * 256;
  let buffer = device.create_buffer(&wgpu::BufferDescriptor {
    label: Some("yuzuko-surface-read"),
    size: (stride * height as usize) as u64,
    usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
    mapped_at_creation: false,
  });
  let mut encoder =
    device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
      label: Some("yuzuko-surface-read"),
    });
  encoder.copy_texture_to_buffer(
    texture.as_image_copy(),
    wgpu::TexelCopyBufferInfo {
      buffer: &buffer,
      layout: wgpu::TexelCopyBufferLayout {
        offset: 0,
        bytes_per_row: Some(stride as u32),
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
  let mut pixels = vec![0u8; tight * height as usize];
  for row in 0..height as usize {
    let from = row * stride;
    let to = row * tight;
    pixels[to..to + tight].copy_from_slice(&view[from..from + tight]);
  }
  drop(view);
  buffer.unmap();
  if swap {
    for pixel in pixels.as_chunks_mut::<4>().0 {
      pixel.swap(0, 2);
    }
  }
  Some(pixels)
}

fn blit_rgba(
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
    label: Some("yuzuko-surface-fit"),
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
      label: Some("yuzuko-surface-fit"),
    });
  blitter.copy(device, &mut encoder, &view, target);
  queue.submit([encoder.finish()]);
}

fn blit_canvas(
  device: &wgpu::Device,
  queue: &wgpu::Queue,
  source: &wgpu::TextureView,
  target: &wgpu::TextureView,
  format: wgpu::TextureFormat,
  alpha_opaque: bool,
  color_space: ColorSpace,
) {
  // There is no page composite. Both alpha modes blit stored texels.
  // display-p3 is presented as the unorm8 swapchain image.
  match (alpha_opaque, color_space) {
    (true | false, ColorSpace::Srgb | ColorSpace::DisplayP3) => {
      let blitter = wgpu::util::TextureBlitter::new(device, format);
      let mut encoder =
        device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
          label: Some("yuzuko-webgpu-present"),
        });
      blitter.copy(device, &mut encoder, source, target);
      queue.submit([encoder.finish()]);
    }
  }
}

pub fn expire(id: u64) {
  expire_current(id);
}

pub fn resized(id: u64) {
  expire_current(id);
}

pub fn dispose(id: u64) {
  CANVASES.with(|canvases| {
    canvases.borrow_mut().remove(&id);
  });
}

pub fn clear() {
  CANVASES.with(|canvases| canvases.borrow_mut().clear());
}

fn expire_current(id: u64) {
  CANVASES.with(|canvases| {
    if let Some(slot) = canvases.borrow_mut().get_mut(&id)
      && let Some(current) = slot.current.take()
    {
      current.destroyed.store(true, Ordering::SeqCst);
      *current.texture.borrow_mut() = None;
    }
  });
}

fn bitmap_dimension(value: f64) -> Result<u32, OpError> {
  if !value.is_finite() || value < 0.0 || value > f64::from(u32::MAX) {
    return Err(OpError::dom(
      "InvalidStateError",
      "The canvas has no pixels",
    ));
  }
  Ok(value.round() as u32)
}

pub fn copy_external_image(
  scope: &mut v8::PinScope,
  handle: &Handle,
  source: v8::Local<v8::Value>,
  destination: v8::Local<v8::Value>,
  size: v8::Local<v8::Value>,
) -> Result<(), OpError> {
  let source = js::object(scope, source).ok_or_else(|| {
    OpError::type_("copyExternalImageToTexture source must be an object")
  })?;
  let image = js::get(scope, source, "source")
    .ok_or_else(|| OpError::type_("The external image source is required"))?;
  let mut raster = raster_of(scope, image)?;
  let destination = js::object(scope, destination).ok_or_else(|| {
    OpError::type_("copyExternalImageToTexture destination must be an object")
  })?;
  let texture = js::get(scope, destination, "texture")
    .and_then(|value| resource::host_texture(scope, value))
    .ok_or_else(|| OpError::type_("The destination is not a GPUTexture"))?;
  let texture = texture.texture()?;
  let color_space = js::get(scope, destination, "colorSpace")
    .and_then(|value| js::string(scope, value))
    .unwrap_or_else(|| "srgb".to_string());
  let color_space = match color_space.as_str() {
    "srgb" => ColorSpace::Srgb,
    "display-p3" => ColorSpace::DisplayP3,
    _ => return Err(OpError::type_("The destination color space is invalid")),
  };
  convert(&mut raster, color_space);
  let premultiply = js::get(scope, destination, "premultipliedAlpha")
    .is_some_and(|value| value.boolean_value(scope));
  if premultiply {
    yuzuko_image::premultiply(&mut raster);
  }
  let (width, height) = image_size(scope, size, raster.width, raster.height)?;
  let source_origin = origin_of(scope, source)?;
  let format = texture.format();
  let tight = texel_bytes(
    &raster,
    format,
    source_origin.0,
    source_origin.1,
    width,
    height,
  )?;
  let (bytes, stride) = pad_rows(&tight, row_bytes(format, width)?, height);
  let origin = origin_of(scope, destination)?;
  handle.queue.write_texture(
    wgpu::TexelCopyTextureInfo {
      texture: &texture,
      mip_level: 0,
      origin: wgpu::Origin3d {
        x: origin.0,
        y: origin.1,
        z: 0,
      },
      aspect: wgpu::TextureAspect::All,
    },
    &bytes,
    wgpu::TexelCopyBufferLayout {
      offset: 0,
      bytes_per_row: Some(stride),
      rows_per_image: Some(height),
    },
    wgpu::Extent3d {
      width,
      height,
      depth_or_array_layers: 1,
    },
  );
  Ok(())
}

fn raster_of(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<Raster, OpError> {
  if let Ok(object) = v8::Local::<v8::Object>::try_from(value)
    && let Some(host) = unsafe {
      yuzuko_core::op::host_ref::<yuzuko_image::ImageBitmap>(scope, object)
    }
  {
    if host.get().is_detached() {
      return Err(OpError::dom("InvalidStateError", "The image is detached"));
    }
    return host.get().copy_raster().ok_or_else(|| {
      OpError::dom("InvalidStateError", "The image is detached")
    });
  }
  if let Some((id, width, height)) =
    yuzuko_window::offscreen_canvas(scope, value)
  {
    if let Some(snapshot) = yuzuko_window::snapshot_canvas(id) {
      let space = if snapshot.display_p3 {
        ColorSpace::DisplayP3
      } else {
        ColorSpace::Srgb
      };
      let format = if snapshot.float16 {
        SampleFormat::Float16
      } else {
        SampleFormat::Unorm8
      };
      let mut raster =
        Raster::new(snapshot.width, snapshot.height, space, format).map_err(
          |_| OpError::dom("InvalidStateError", "The canvas has no pixels"),
        )?;
      raster.bytes = snapshot.bytes;
      return Ok(raster);
    }
    return Raster::new(
      width as u32,
      height as u32,
      ColorSpace::Srgb,
      SampleFormat::Unorm8,
    )
    .map_err(|_| {
      OpError::dom("InvalidStateError", "The canvas has no pixels")
    });
  }
  match frame_pixels(scope, value) {
    FramePixels::Ready(raster) => return Ok(raster),
    FramePixels::Closed => {
      return Err(OpError::dom("InvalidStateError", "The frame is closed"));
    }
    FramePixels::NotFrame => {}
  }
  Err(OpError::type_(
    "The external image is not an ImageBitmap, a VideoFrame, or an OffscreenCanvas",
  ))
}

fn texel_bytes(
  raster: &Raster,
  format: wgpu::TextureFormat,
  origin_x: u32,
  origin_y: u32,
  width: u32,
  height: u32,
) -> Result<Vec<u8>, OpError> {
  let full = origin_x == 0
    && origin_y == 0
    && raster.width == width
    && raster.height == height;
  match format {
    wgpu::TextureFormat::Rgba8Unorm | wgpu::TextureFormat::Rgba8UnormSrgb => {
      if full && raster.format == SampleFormat::Unorm8 {
        Ok(raster.bytes.clone())
      } else {
        Ok(quantize(raster, origin_x, origin_y, width, height))
      }
    }
    wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb => {
      let mut bytes = if full && raster.format == SampleFormat::Unorm8 {
        raster.bytes.clone()
      } else {
        quantize(raster, origin_x, origin_y, width, height)
      };
      swap_red_blue(&mut bytes);
      Ok(bytes)
    }
    wgpu::TextureFormat::Rgba16Float
      if full && raster.format == SampleFormat::Float16 =>
    {
      Ok(raster.bytes.clone())
    }
    _ => Err(OpError::type_(
      "The destination format is not supported for an external image copy",
    )),
  }
}

fn row_bytes(
  format: wgpu::TextureFormat,
  width: u32,
) -> Result<usize, OpError> {
  let pixel = match format {
    wgpu::TextureFormat::Rgba16Float => 8usize,
    wgpu::TextureFormat::Rgba8Unorm
    | wgpu::TextureFormat::Rgba8UnormSrgb
    | wgpu::TextureFormat::Bgra8Unorm
    | wgpu::TextureFormat::Bgra8UnormSrgb => 4usize,
    _ => {
      return Err(OpError::type_(
        "The destination format is not supported for an external image copy",
      ));
    }
  };
  Ok(pixel * width as usize)
}

fn pad_rows(tight: &[u8], row_bytes: usize, height: u32) -> (Vec<u8>, u32) {
  let stride = row_bytes.div_ceil(256) * 256;
  if stride == row_bytes {
    return (tight.to_vec(), stride as u32);
  }
  let mut padded = vec![0u8; stride * height as usize];
  for y in 0..height as usize {
    let src = y * row_bytes;
    let dst = y * stride;
    let end = src + row_bytes;
    if end > tight.len() {
      break;
    }
    padded[dst..dst + row_bytes].copy_from_slice(&tight[src..end]);
  }
  (padded, stride as u32)
}

fn quantize(
  raster: &Raster,
  origin_x: u32,
  origin_y: u32,
  width: u32,
  height: u32,
) -> Vec<u8> {
  let mut bytes = Vec::with_capacity(width as usize * height as usize * 4);
  for y in 0..height {
    for x in 0..width {
      let sample =
        raster.sample(i64::from(origin_x + x), i64::from(origin_y + y));
      for channel in sample {
        let channel = channel.clamp(0.0, 1.0);
        bytes.push((channel * 255.0).round() as u8);
      }
    }
  }
  bytes
}

fn swap_red_blue(bytes: &mut [u8]) {
  let (pixels, _) = bytes.as_chunks_mut::<4>();
  for pixel in pixels {
    pixel.swap(0, 2);
  }
}

fn image_size(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
  fallback_width: u32,
  fallback_height: u32,
) -> Result<(u32, u32), OpError> {
  if let Ok(array) = v8::Local::<v8::Array>::try_from(value) {
    let width = js::index_u32(scope, array, 0)?;
    let height = if array.length() > 1 {
      js::index_u32(scope, array, 1)?
    } else {
      fallback_height
    };
    return Ok((width, height));
  }
  Ok((fallback_width, fallback_height))
}

fn origin_of(
  scope: &mut v8::PinScope,
  destination: v8::Local<v8::Object>,
) -> Result<(u32, u32), OpError> {
  let Some(origin) = js::get(scope, destination, "origin") else {
    return Ok((0, 0));
  };
  if let Ok(array) = v8::Local::<v8::Array>::try_from(origin) {
    let x = js::index_u32(scope, array, 0)?;
    let y = if array.length() > 1 {
      js::index_u32(scope, array, 1)?
    } else {
      0
    };
    return Ok((x, y));
  }
  Ok((0, 0))
}
