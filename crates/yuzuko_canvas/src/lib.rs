//! Canvas 2D and `bitmaprenderer`.

mod geom;
mod paint;
mod text;

use std::sync::Once;

use yuzuko_core::op::{Js, OpError};
use yuzuko_image::ColorSpace;
use yuzuko_image::SampleFormat;
use yuzuko_window::CanvasSnapshot;
use yuzuko_window::ContextKind;
use yuzuko_window::OffscreenCanvas;

use crate::paint::bitmap::BitmapContext;
use crate::paint::context::Context2D;
use crate::paint::encode;
use crate::paint::objects;
use crate::paint::record::{self, Attrs};
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
      present,
      dispose: record::dispose,
      take_slot: record::take_slot,
      put_slot: record::put_slot,
      install_presented,
    });
    yuzuko_core::add_extension(&install);
    yuzuko_core::on_realm_drop(fonts::clear);
    geom::geometry::register_clone();
    geom::matrix::register_clone();
  });
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
  objects::Path2D::install(scope);
  objects::CanvasGradient::install(scope);
  objects::CanvasPattern::install(scope);
  objects::TextCluster::install(scope);
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
  if let Some(alpha) = dictionary(scope, object, "alpha") {
    attrs.alpha = alpha.boolean_value(scope);
  }
  if let Some(color_space) = dictionary(scope, object, "colorSpace") {
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
  if let Some(desynchronized) = dictionary(scope, object, "desynchronized") {
    attrs.desynchronized = desynchronized.boolean_value(scope);
  }
  if let Some(will_read) = dictionary(scope, object, "willReadFrequently") {
    attrs.will_read_frequently = will_read.boolean_value(scope);
  }
  if let Some(color_type) = dictionary(scope, object, "colorType") {
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
  Ok(attrs)
}

fn dictionary<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  object: v8::Local<v8::Object>,
  name: &str,
) -> Option<v8::Local<'s, v8::Value>> {
  let key = v8::String::new(scope, name)?;
  let value = object.get(scope, key.into())?;
  if value.is_undefined() {
    None
  } else {
    Some(value)
  }
}

fn convert_to_blob(
  scope: &mut v8::PinScope,
  id: u64,
  options: Option<v8::Global<v8::Value>>,
) -> Result<Js, OpError> {
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
  if let Some(kind) = dictionary(scope, object, "type") {
    let text = kind.to_rust_string_lossy(scope).to_ascii_lowercase();
    if text == "image/jpeg" || text == "image/webp" || text == "image/png" {
      mime = text;
    }
  }
  if let Some(value) = dictionary(scope, object, "quality")
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

fn present(
  device: &wgpu::Device,
  queue: &wgpu::Queue,
  view: &wgpu::TextureView,
  id: u64,
  target_width: u32,
  target_height: u32,
  format: wgpu::TextureFormat,
) -> bool {
  let Some(mut raster) = snapshot_raster(id) else {
    return false;
  };
  if raster.width == 0 || raster.height == 0 {
    return true;
  }
  yuzuko_image::convert(&mut raster, ColorSpace::Srgb);
  if raster.format != SampleFormat::Unorm8 {
    let mut bytes =
      Vec::with_capacity(raster.width as usize * raster.height as usize * 4);
    for y in 0..raster.height {
      for x in 0..raster.width {
        let sample = raster.sample(i64::from(x), i64::from(y));
        for channel in sample {
          bytes.push((channel.clamp(0.0, 1.0) * 255.0).round() as u8);
        }
      }
    }
    raster = yuzuko_image::Raster::from_rgba8(
      raster.width,
      raster.height,
      ColorSpace::Srgb,
      &bytes,
    )
    .expect("present");
  }
  yuzuko_image::premultiply(&mut raster);
  let style = yuzuko_window::canvas_style(id);
  if target_width == 0
    || target_height == 0
    || (style.fit == yuzuko_window::ObjectFit::Fill
      && yuzuko_window::rendering_is_smooth(style.rendering))
  {
    replay::present_texture(
      device,
      queue,
      view,
      format,
      &raster.bytes,
      raster.width,
      raster.height,
    );
    return true;
  }
  let pad = if yuzuko_window::canvas_is_transparent(id) {
    0
  } else {
    255
  };
  let fitted = yuzuko_window::composite_pad(
    &raster.bytes,
    raster.width,
    raster.height,
    target_width,
    target_height,
    style.fit,
    style.rendering,
    pad,
  );
  replay::present_texture(
    device,
    queue,
    view,
    format,
    &fitted,
    target_width,
    target_height,
  );
  true
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
