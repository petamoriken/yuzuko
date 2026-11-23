//! Canvas 2D and `bitmaprenderer`.

mod geom;
mod paint;
mod text;

use std::cell::RefCell;
use std::sync::Once;

use yuzuko_core::op::{Js, OpError};
use yuzuko_image::ColorSpace;
use yuzuko_image::SampleFormat;
use yuzuko_window::CanvasSnapshot;
use yuzuko_window::ContextKind;
use yuzuko_window::OffscreenCanvas;
use yuzuko_window::PresentRequest;
use yuzuko_window::PresentTarget;

use crate::paint::bitmap::BitmapContext;
use crate::paint::context::Context2D;
use crate::paint::encode;
use crate::paint::objects;
use crate::paint::record::{self, Attrs, ToneMapping};
use crate::paint::replay;
use crate::text::fonts;

pub fn register() {
  static ONCE: Once = Once::new();
  ONCE.call_once(|| {
    replay::select_renderer();
    yuzuko_window::install_hooks(yuzuko_window::CanvasHooks {
      get_context,
      convert_to_blob,
      transfer_to_image_bitmap,
      resized: record::resized,
      snapshot,
      layers_open: record::layers_open,
      present_request,
      stage,
      present,
      discard_present: discard_staged,
      dispose: record::dispose,
      take_slot: record::take_slot,
      put_slot: record::put_slot,
      install_presented,
    });
    yuzuko_core::add_extension(&install);
    yuzuko_core::on_realm_drop(fonts::clear);
    yuzuko_core::on_reload(reload_fonts);
    geom::geometry::register_clone();
    geom::matrix::register_clone();
  });
}

fn reload_fonts() {
  if yuzuko_core::reload_targets_modules() && yuzuko_core::reload_keeps_module()
  {
    return;
  }
  fonts::clear();
}

fn install(scope: &mut v8::PinScope) {
  geom::matrix::DomMatrixReadOnly::install(scope);
  geom::matrix::DomMatrix::install(scope);
  geom::geometry::DomPointReadOnly::install(scope);
  geom::geometry::DomPoint::install(scope);
  geom::geometry::DomRectReadOnly::install(scope);
  geom::geometry::DomRect::install(scope);
  geom::geometry::DomQuad::install(scope);
  geom::geometry::DomRectList::install(scope);
  geom::geometry::install_dom_rect_list_iterator(scope);
  objects::Path2D::install(scope);
  objects::CanvasGradient::install(scope);
  objects::CanvasPattern::install(scope);
  objects::TextMetrics::install(scope);
  Context2D::install(scope);
  BitmapContext::install(scope);
  let yuzuko = yuzuko_core::namespace(scope, "Yuzuko");
  let key = v8::String::new(scope, "canvasRenderer").unwrap();
  let value = v8::String::new(scope, replay::renderer_name()).unwrap();
  yuzuko.set(scope, key.into(), value.into());
  fonts::install(scope);
  if yuzuko_core::is_dedicated_worker() {
    hide_worker_matrix(scope);
  }
}

fn hide_worker_matrix(scope: &mut v8::PinScope) {
  delete_method(scope, "DOMMatrixReadOnly", "toString");
  delete_method(scope, "DOMMatrix", "setMatrixValue");
}

fn delete_method(scope: &mut v8::PinScope, class_name: &str, method: &str) {
  let global = scope.get_current_context().global(scope);
  let Some(class_key) = v8::String::new(scope, class_name) else {
    return;
  };
  let Some(ctor) = global.get(scope, class_key.into()) else {
    return;
  };
  let Ok(ctor) = v8::Local::<v8::Object>::try_from(ctor) else {
    return;
  };
  let Some(proto_key) = v8::String::new(scope, "prototype") else {
    return;
  };
  let Some(proto) = ctor.get(scope, proto_key.into()) else {
    return;
  };
  let Ok(proto) = v8::Local::<v8::Object>::try_from(proto) else {
    return;
  };
  let Some(key) = v8::String::new(scope, method) else {
    return;
  };
  let _ = proto.delete(scope, key.into());
}

fn get_context(
  scope: &mut v8::PinScope,
  canvas: &OffscreenCanvas,
  this: v8::Global<v8::Object>,
  type_name: String,
  options: Option<v8::Global<v8::Value>>,
) -> Result<Js, OpError> {
  let requested = match type_name.as_str() {
    "2d" => ContextKind::TwoD,
    "bitmaprenderer" => ContextKind::Bitmap,
    "webgl" | "webgl2" => return Ok(Js::null(scope)),
    _ => {
      return Err(OpError::type_("The context type is not supported"));
    }
  };
  if canvas.context_kind() == requested
    && let Some(object) = canvas.context_object(scope)
  {
    let value: v8::Local<v8::Value> = object.into();
    return Ok(Js(v8::Global::new(scope, value)));
  }
  if canvas.context_kind() != ContextKind::None {
    return Ok(Js::null(scope));
  }
  let this = v8::Local::new(scope, &this);
  let (width, height) = canvas.bitmap();
  let object = match requested {
    ContextKind::TwoD => {
      let attrs = read_attributes(scope, options)?;
      record::open_2d(canvas.id(), width, height, attrs);
      let context = Context2D::create(scope, canvas.id(), this);
      unsafe { yuzuko_core::op::wrap_instance(scope, context) }
    }
    ContextKind::Bitmap => {
      record::open_bitmap(canvas.id(), width, height);
      let context = BitmapContext::create(scope, canvas.id(), this);
      unsafe { yuzuko_core::op::wrap_instance(scope, context) }
    }
    ContextKind::None | ContextKind::WebGpu => {
      return Ok(Js::null(scope));
    }
  };
  canvas.store_context(scope, requested, object);
  let value: v8::Local<v8::Value> = object.into();
  Ok(Js(v8::Global::new(scope, value)))
}

fn read_attributes(
  scope: &mut v8::PinScope,
  options: Option<v8::Global<v8::Value>>,
) -> Result<Attrs, OpError> {
  let mut attrs = Attrs::default();
  let Some(options) = options else {
    return Ok(attrs);
  };
  let value = v8::Local::new(scope, &options);
  if value.is_null_or_undefined() {
    return Ok(attrs);
  }
  let Ok(object) = v8::Local::<v8::Object>::try_from(value) else {
    return Ok(attrs);
  };
  if let Some(alpha) = dictionary(scope, object, "alpha")? {
    attrs.alpha = alpha.boolean_value(scope);
  }
  if let Some(color_space) = dictionary(scope, object, "colorSpace")? {
    let text = color_space.to_rust_string_lossy(scope);
    attrs.color_space = match text.as_str() {
      "srgb" => ColorSpace::Srgb,
      "display-p3" => ColorSpace::DisplayP3,
      _ => {
        return Err(OpError::type_(
          "The provided value is not a valid enum value of type PredefinedColorSpace",
        ));
      }
    };
  }
  if let Some(desynchronized) = dictionary(scope, object, "desynchronized")? {
    attrs.desynchronized = desynchronized.boolean_value(scope);
  }
  if let Some(will_read) = dictionary(scope, object, "willReadFrequently")? {
    attrs.will_read_frequently = will_read.boolean_value(scope);
  }
  if let Some(color_type) = dictionary(scope, object, "colorType")? {
    let text = color_type.to_rust_string_lossy(scope);
    attrs.format = match text.as_str() {
      "unorm8" => SampleFormat::Unorm8,
      "float16" => SampleFormat::Float16,
      _ => {
        return Err(OpError::type_(
          "The provided value is not a valid enum value of type CanvasColorType",
        ));
      }
    };
  }
  if let Some(tone) = dictionary(scope, object, "toneMapping")? {
    attrs.tone_mapping = tone_mapping(scope, tone)?;
  }
  Ok(attrs)
}

fn tone_mapping(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<ToneMapping, OpError> {
  let Ok(object) = v8::Local::<v8::Object>::try_from(value) else {
    return Err(OpError::type_(
      "The provided value is not of type 'CanvasToneMapping'",
    ));
  };
  let Some(mode) = dictionary(scope, object, "mode")? else {
    return Ok(ToneMapping::Standard);
  };
  match mode.to_rust_string_lossy(scope).as_str() {
    "standard" => Ok(ToneMapping::Standard),
    "extended" => Ok(ToneMapping::Extended),
    _ => Err(OpError::type_(
      "The provided value is not a valid enum value of type CanvasToneMappingMode",
    )),
  }
}

fn dictionary<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  object: v8::Local<v8::Object>,
  name: &str,
) -> Result<Option<v8::Local<'s, v8::Value>>, OpError> {
  yuzuko_core::dictionary_member(scope, object, name)
}

fn convert_to_blob(
  scope: &mut v8::PinScope,
  id: u64,
  options: Option<v8::Global<v8::Value>>,
) -> Result<Js, OpError> {
  if record::layers_open(id) {
    return Ok(reject(
      scope,
      "InvalidStateError",
      "The canvas has an open layer",
    ));
  }
  let (mime, quality) = blob_options(scope, options)?;
  let Some(raster) = snapshot_raster(id) else {
    return Ok(reject(
      scope,
      "InvalidStateError",
      "The canvas has no pixels",
    ));
  };
  if raster.width == 0 || raster.height == 0 {
    return Ok(reject(
      scope,
      "InvalidStateError",
      "The canvas has no pixels",
    ));
  }
  match encode::encode(&raster, &mime, quality) {
    Ok(bytes) => Ok(resolve_blob(scope, bytes, &mime)),
    Err(message) => Ok(reject(scope, "EncodingError", &message)),
  }
}

fn blob_options(
  scope: &mut v8::PinScope,
  options: Option<v8::Global<v8::Value>>,
) -> Result<(String, f32), OpError> {
  let mut mime = "image/png".to_string();
  let mut quality = 0.92;
  let Some(options) = options else {
    return Ok((mime, quality));
  };
  let value = v8::Local::new(scope, &options);
  let Ok(object) = v8::Local::<v8::Object>::try_from(value) else {
    return Ok((mime, quality));
  };
  if let Some(kind) = dictionary(scope, object, "type")? {
    let text = kind.to_rust_string_lossy(scope).to_ascii_lowercase();
    if text == "image/jpeg" || text == "image/webp" || text == "image/png" {
      mime = text;
    }
  }
  if let Some(value) = dictionary(scope, object, "quality")?
    && let Some(number) = value.number_value(scope)
    && number.is_finite()
  {
    quality = number as f32;
  }
  Ok((mime, quality))
}

fn transfer_to_image_bitmap(
  scope: &mut v8::PinScope,
  id: u64,
) -> Result<Js, OpError> {
  if record::layers_open(id) {
    return Err(OpError::dom(
      "InvalidStateError",
      "The canvas has an open layer",
    ));
  }
  let Some(raster) = snapshot_raster(id) else {
    return Err(OpError::dom(
      "InvalidStateError",
      "The canvas has no bitmap",
    ));
  };
  record::resized(id, f64::from(raster.width), f64::from(raster.height));
  let bitmap = yuzuko_image::ImageBitmap::from_raster(raster);
  let object = unsafe { yuzuko_core::op::wrap_instance(scope, bitmap) };
  let value: v8::Local<v8::Value> = object.into();
  Ok(Js(v8::Global::new(scope, value)))
}

fn install_presented(id: u64, snapshot: CanvasSnapshot) {
  record::present_bitmap(id, snapshot.width, snapshot.height, snapshot.bytes);
}

fn snapshot(id: u64) -> Option<CanvasSnapshot> {
  snapshot_raster(id).map(|raster| CanvasSnapshot {
    width: raster.width,
    height: raster.height,
    display_p3: raster.color_space == ColorSpace::DisplayP3,
    float16: raster.format == SampleFormat::Float16,
    bytes: raster.bytes,
  })
}

fn snapshot_raster(id: u64) -> Option<yuzuko_image::Raster> {
  if let Some(raster) =
    record::with_drawing(id, |drawing| replay::rasterize(drawing))
  {
    return raster;
  }
  record::with_bitmap(id, |image| {
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
      yuzuko_image::Raster::new(image.width, image.height, space, format)
        .ok()?;
    if raster.bytes.len() != image.bytes.len() {
      return None;
    }
    raster.bytes.clone_from(&image.bytes);
    raster.premultiplied = image.premultiplied;
    if raster.premultiplied {
      yuzuko_image::unpremultiply(&mut raster);
    }
    Some(raster)
  })?
}

enum Staged {
  /// Nothing to upload. The caller still presents the current drawable.
  Skip,
  Texture(wgpu::Texture),
  Pixels {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
    /// Encoded `Rgba16Float` samples. Otherwise tightly packed unorm8.
    float16: bool,
  },
}

enum Prepare {
  Ready(Staged),
  Failed,
  Fallback,
}

thread_local! {
  static STAGED: RefCell<Option<(u64, Staged)>> = const { RefCell::new(None) };
}

fn present_request(id: u64) -> Option<PresentRequest> {
  if let Some(request) = record::with_drawing(id, |drawing| {
    request_of(drawing.attrs.color_space, drawing.attrs.format)
  }) {
    return Some(request);
  }
  record::with_bitmap(id, |image| {
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
    request_of(space, format)
  })
}

fn request_of(space: ColorSpace, format: SampleFormat) -> PresentRequest {
  match (space, format) {
    (_, SampleFormat::Float16) => PresentRequest::Float16 {
      display_p3: space == ColorSpace::DisplayP3,
    },
    (ColorSpace::DisplayP3, SampleFormat::Unorm8) => {
      PresentRequest::DisplayP3Unorm8
    }
    (ColorSpace::Srgb, SampleFormat::Unorm8) => PresentRequest::SrgbUnorm8,
  }
}

fn stage(
  _device: &wgpu::Device,
  _queue: &wgpu::Queue,
  id: u64,
  target_width: u32,
  target_height: u32,
  target: PresentTarget,
) -> bool {
  match prepare(id, target_width, target_height, target) {
    Some(staged) => {
      STAGED.with(|slot| *slot.borrow_mut() = Some((id, staged)));
      true
    }
    None => {
      discard_staged();
      false
    }
  }
}

fn discard_staged() {
  STAGED.with(|slot| *slot.borrow_mut() = None);
}

fn present(
  device: &wgpu::Device,
  queue: &wgpu::Queue,
  view: &wgpu::TextureView,
  id: u64,
  _target_width: u32,
  _target_height: u32,
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
    Staged::Skip => true,
    Staged::Texture(texture) => {
      let source = texture.create_view(&wgpu::TextureViewDescriptor::default());
      replay::blit_texture(device, queue, &source, view, format);
      true
    }
    Staged::Pixels {
      width,
      height,
      pixels,
      float16,
    } => {
      replay::present_texture(
        device,
        queue,
        view,
        format,
        replay::PresentSamples {
          pixels: &pixels,
          width,
          height,
          float16,
        },
      );
      true
    }
  }
}

fn prepare(
  id: u64,
  target_width: u32,
  target_height: u32,
  target: PresentTarget,
) -> Option<Staged> {
  if target == PresentTarget::SrgbUnorm8 {
    match record::with_drawing(id, |drawing| {
      prepare_drawing(drawing, id, target_width, target_height)
    }) {
      Some(Prepare::Ready(staged)) => return Some(staged),
      Some(Prepare::Failed) => return None,
      Some(Prepare::Fallback) | None => {}
    }
  }
  prepare_snapshot(id, target_width, target_height, target)
}

fn prepare_drawing(
  drawing: &record::Drawing,
  id: u64,
  target_width: u32,
  target_height: u32,
) -> Prepare {
  if !drawing.attrs.vello_target() {
    return Prepare::Fallback;
  }
  if drawing.width == 0 || drawing.height == 0 {
    return Prepare::Ready(Staged::Skip);
  }
  let style = yuzuko_window::canvas_style(id);
  let direct = drawing.attrs.alpha
    && (target_width == 0
      || target_height == 0
      || (drawing.width == target_width && drawing.height == target_height)
      || (style.fit == yuzuko_window::ObjectFit::Fill
        && yuzuko_window::rendering_is_smooth(style.rendering)));
  if direct
    && let Some(texture) =
      replay::render_texture(&drawing.commands, drawing.width, drawing.height)
  {
    return Prepare::Ready(Staged::Texture(texture));
  }
  let Some(mut pixels) =
    replay::render_premul(&drawing.commands, drawing.width, drawing.height)
  else {
    return Prepare::Failed;
  };
  if !drawing.attrs.alpha {
    for pixel in pixels.as_chunks_mut::<4>().0 {
      pixel[3] = 255;
    }
  }
  Prepare::Ready(fit_pixels(
    pixels,
    drawing.width,
    drawing.height,
    target_width,
    target_height,
    id,
    false,
  ))
}

/// Premultiplied bytes for `target`. Unorm8 targets quantize float16.
/// Extended targets keep encoded float16, including components outside 0..1.
fn encode_present(
  mut raster: yuzuko_image::Raster,
  target: PresentTarget,
) -> Option<(Vec<u8>, bool)> {
  let (space, float16) = match target {
    PresentTarget::SrgbUnorm8 => (ColorSpace::Srgb, false),
    PresentTarget::DisplayP3Unorm8 => (ColorSpace::DisplayP3, false),
    PresentTarget::ExtendedSrgb => (ColorSpace::Srgb, true),
    PresentTarget::ExtendedDisplayP3 => (ColorSpace::DisplayP3, true),
  };
  yuzuko_image::convert(&mut raster, space);
  let format = if float16 {
    SampleFormat::Float16
  } else {
    SampleFormat::Unorm8
  };
  raster = yuzuko_image::convert_format(raster, format).ok()?;
  yuzuko_image::premultiply(&mut raster);
  Some((raster.bytes, float16))
}

fn prepare_snapshot(
  id: u64,
  target_width: u32,
  target_height: u32,
  target: PresentTarget,
) -> Option<Staged> {
  let raster = snapshot_raster(id)?;
  if raster.width == 0 || raster.height == 0 {
    return Some(Staged::Skip);
  }
  let width = raster.width;
  let height = raster.height;
  let (pixels, float16) = encode_present(raster, target)?;
  Some(fit_pixels(
    pixels,
    width,
    height,
    target_width,
    target_height,
    id,
    float16,
  ))
}

fn fit_pixels(
  pixels: Vec<u8>,
  source_width: u32,
  source_height: u32,
  target_width: u32,
  target_height: u32,
  id: u64,
  float16: bool,
) -> Staged {
  if source_width == 0 || source_height == 0 {
    return Staged::Skip;
  }
  let style = yuzuko_window::canvas_style(id);
  let direct = target_width == 0
    || target_height == 0
    || (source_width == target_width && source_height == target_height)
    || (style.fit == yuzuko_window::ObjectFit::Fill
      && yuzuko_window::rendering_is_smooth(style.rendering));
  if direct {
    return Staged::Pixels {
      width: source_width,
      height: source_height,
      pixels,
      float16,
    };
  }
  let pad = if yuzuko_window::canvas_is_transparent(id) {
    0
  } else {
    255
  };
  // `composite_pad` is unorm8. A float16 object-fit pad quantizes, pads, and
  // expands. That path is lossy and is not the equal-size present.
  let unorm = if float16 {
    let Some(bytes) = quantize_encoded(&pixels, source_width, source_height)
    else {
      return Staged::Skip;
    };
    bytes
  } else {
    pixels
  };
  let fitted = yuzuko_window::composite_pad(
    &unorm,
    source_width,
    source_height,
    target_width,
    target_height,
    style.fit,
    style.rendering,
    pad,
  );
  if !float16 {
    return Staged::Pixels {
      width: target_width,
      height: target_height,
      pixels: fitted,
      float16: false,
    };
  }
  let Some(pixels) = expand_encoded(&fitted, target_width, target_height)
  else {
    return Staged::Skip;
  };
  Staged::Pixels {
    width: target_width,
    height: target_height,
    pixels,
    float16: true,
  }
}

fn quantize_encoded(bytes: &[u8], width: u32, height: u32) -> Option<Vec<u8>> {
  let mut raster = yuzuko_image::Raster::new(
    width,
    height,
    ColorSpace::Srgb,
    SampleFormat::Float16,
  )
  .ok()?;
  if raster.bytes.len() != bytes.len() {
    return None;
  }
  raster.bytes.copy_from_slice(bytes);
  raster.premultiplied = true;
  let raster =
    yuzuko_image::convert_format(raster, SampleFormat::Unorm8).ok()?;
  Some(raster.bytes)
}

fn expand_encoded(bytes: &[u8], width: u32, height: u32) -> Option<Vec<u8>> {
  let raster =
    yuzuko_image::Raster::from_rgba8(width, height, ColorSpace::Srgb, bytes)
      .ok()?;
  let mut raster =
    yuzuko_image::convert_format(raster, SampleFormat::Float16).ok()?;
  raster.premultiplied = true;
  Some(raster.bytes)
}

fn resolve_blob(scope: &mut v8::PinScope, bytes: Vec<u8>, mime: &str) -> Js {
  let Some(resolver) = v8::PromiseResolver::new(scope) else {
    return Js::null(scope);
  };
  let promise = resolver.get_promise(scope);
  let blob = yuzuko_winter::new_blob(scope, bytes, mime);
  let blob_value: v8::Local<v8::Value> = blob.into();
  let _ = resolver.resolve(scope, blob_value);
  let promise_value: v8::Local<v8::Value> = promise.into();
  Js(v8::Global::new(scope, promise_value))
}

fn reject(scope: &mut v8::PinScope, name: &str, message: &str) -> Js {
  let Some(resolver) = v8::PromiseResolver::new(scope) else {
    return Js::null(scope);
  };
  let promise = resolver.get_promise(scope);
  if let Some(error) = yuzuko_core::op::new_dom_exception(scope, name, message)
  {
    let error_value: v8::Local<v8::Value> = error.into();
    let _ = resolver.reject(scope, error_value);
  }
  let promise_value: v8::Local<v8::Value> = promise.into();
  Js(v8::Global::new(scope, promise_value))
}

#[cfg(test)]
mod present_tests {
  use super::encode_present;
  use yuzuko_image::ColorSpace;
  use yuzuko_image::Raster;
  use yuzuko_window::PresentTarget;

  #[test]
  fn present_keeps_display_p3_and_quantizes_when_asked() {
    let wide =
      Raster::from_rgba8(1, 1, ColorSpace::DisplayP3, &[10, 20, 30, 255])
        .unwrap();
    let (kept, float16) =
      encode_present(wide, PresentTarget::DisplayP3Unorm8).unwrap();
    assert!(!float16);
    assert_eq!(kept, vec![10, 20, 30, 255]);

    let wide =
      Raster::from_rgba8(1, 1, ColorSpace::DisplayP3, &[10, 20, 30, 255])
        .unwrap();
    let (srgb, _) = encode_present(wide, PresentTarget::SrgbUnorm8).unwrap();
    assert_ne!(srgb, vec![10, 20, 30, 255]);
  }

  #[test]
  fn extended_present_keeps_a_component_above_one() {
    let raster =
      Raster::from_float16(1, 1, ColorSpace::Srgb, &[1.5, 0.0, 0.0, 1.0])
        .unwrap();
    let (bytes, float16) =
      encode_present(raster, PresentTarget::ExtendedSrgb).unwrap();
    assert!(float16);
    assert_eq!(bytes.len(), 8);
    assert_eq!(&bytes[0..2], &0x3E00u16.to_le_bytes());

    let raster =
      Raster::from_float16(1, 1, ColorSpace::Srgb, &[1.5, 0.0, 0.0, 1.0])
        .unwrap();
    let (quantized, float16) =
      encode_present(raster, PresentTarget::SrgbUnorm8).unwrap();
    assert!(!float16);
    assert_eq!(quantized, vec![255, 0, 0, 255]);
  }
}
