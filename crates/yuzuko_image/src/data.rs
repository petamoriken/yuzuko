//! `ImageData`. Alpha stays unpremultiplied.

use std::cell::RefCell;

use yuzuko_core::op::Js;
use yuzuko_core::op::OpError;
use yuzuko_core::op::traced;
use yuzuko_core::op::traced_local;
use yuzuko_macros::op;

use crate::raster::ColorSpace;
use crate::raster::Raster;
use crate::raster::SampleFormat;
use crate::raster::within_limits;

const MAX_BYTES: u64 = 800_000_000;

pub struct ImageData {
  width: u32,
  height: u32,
  color_space: ColorSpace,
  format: SampleFormat,
  data: RefCell<v8::TracedReference<v8::Object>>,
}

unsafe impl v8::cppgc::GarbageCollected for ImageData {
  fn trace(&self, visitor: &mut v8::cppgc::Visitor) {
    if let Ok(data) = self.data.try_borrow() {
      visitor.trace(&*data);
    }
  }

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"ImageData"
  }
}

impl ImageData {
  pub fn buffer_is_detached(&self, scope: &mut v8::PinScope) -> bool {
    let Ok(data) = self.data.try_borrow() else {
      return true;
    };
    let Some(object) = traced_local(scope, &data) else {
      return true;
    };
    let Ok(view) = v8::Local::<v8::ArrayBufferView>::try_from(object) else {
      return true;
    };
    view
      .buffer(scope)
      .is_none_or(|buffer| buffer.was_detached())
  }

  pub fn from_raster(
    scope: &mut v8::PinScope,
    raster: Raster,
  ) -> Result<ImageData, OpError> {
    let array = allocate_with(scope, &raster)?;
    Ok(host(
      scope,
      raster.width,
      raster.height,
      raster.color_space,
      raster.format,
      array,
    ))
  }

  pub fn snapshot(&self, scope: &mut v8::PinScope) -> Result<Raster, OpError> {
    let object = traced_local(scope, &self.data.borrow())
      .ok_or_else(|| invalid("ImageData data is detached"))?;
    let view = v8::Local::<v8::ArrayBufferView>::try_from(object)
      .map_err(|_| invalid("ImageData data is detached"))?;
    if view
      .buffer(scope)
      .is_some_and(|buffer| buffer.was_detached())
    {
      return Err(invalid("ImageData data is detached"));
    }
    let mut bytes = vec![0; view.byte_length()];
    view.copy_contents(&mut bytes);
    raster_from_bytes(
      self.width,
      self.height,
      self.color_space,
      self.format,
      &bytes,
    )
  }
}

#[op]
impl ImageData {
  #[constructor]
  fn new(
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<ImageData, OpError> {
    if args.length() < 1 {
      return Err(OpError::type_("ImageData requires a width"));
    }
    if is_clamped(scope, args.get(0)) || is_float16(scope, args.get(0)) {
      from_array(scope, args)
    } else if args.get(0).is_object() {
      Err(OpError::type_(
        "ImageData data must be a Uint8ClampedArray or a Float16Array",
      ))
    } else {
      from_size(scope, args)
    }
  }

  #[getter]
  fn width(&self) -> f64 {
    f64::from(self.width)
  }

  #[getter]
  fn height(&self) -> f64 {
    f64::from(self.height)
  }

  #[getter]
  fn color_space(&self) -> String {
    match self.color_space {
      ColorSpace::Srgb => "srgb".to_string(),
      ColorSpace::DisplayP3 => "display-p3".to_string(),
    }
  }

  #[getter]
  #[rename("pixelFormat")]
  fn pixel_format(&self) -> String {
    match self.format {
      SampleFormat::Unorm8 => "rgba-unorm8".to_string(),
      SampleFormat::Float16 => "rgba-float16".to_string(),
    }
  }

  #[getter]
  fn data(&self, scope: &mut v8::PinScope) -> Js {
    let value = traced_local(scope, &self.data.borrow())
      .map(v8::Local::<v8::Value>::from)
      .unwrap_or_else(|| v8::undefined(scope).into());
    Js(v8::Global::new(scope, value))
  }
}

fn from_size(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
) -> Result<ImageData, OpError> {
  if args.length() < 2 {
    return Err(OpError::type_("ImageData requires a height"));
  }
  let width = to_uint32(scope, args.get(0))?;
  let height = to_uint32(scope, args.get(1))?;
  if width == 0 || height == 0 {
    return Err(OpError::dom(
      "IndexSizeError",
      "ImageData width and height must be non-zero",
    ));
  }
  let settings = if args.length() >= 3 {
    Some(args.get(2))
  } else {
    None
  };
  let (color_space, format) =
    settings_of(scope, settings, SampleFormat::Unorm8)?;
  let array = allocate(scope, width, height, format)?;
  Ok(host(scope, width, height, color_space, format, array))
}

fn from_array(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
) -> Result<ImageData, OpError> {
  if args.length() < 2 {
    return Err(OpError::type_("ImageData requires a width"));
  }
  let source = args.get(0);
  let clamped = is_clamped(scope, source);
  let width = to_uint32(scope, args.get(1))?;
  let third = if args.length() >= 3 {
    Some(args.get(2))
  } else {
    None
  };
  let (height_arg, settings) = match third {
    Some(value) if value.is_null() || value.is_undefined() => (None, None),
    Some(value) if value.is_object() && !value.is_array() => {
      (None, Some(value))
    }
    Some(value) => {
      let fourth = if args.length() >= 4 {
        Some(args.get(3))
      } else {
        None
      };
      (Some(to_uint32(scope, value)?), fourth)
    }
    None => (None, None),
  };
  let (color_space, format) =
    settings_of(scope, settings, SampleFormat::Unorm8)?;
  if format == SampleFormat::Unorm8 && !clamped {
    return Err(invalid(
      "rgba-unorm8 ImageData requires a Uint8ClampedArray",
    ));
  }
  if format == SampleFormat::Float16 && clamped {
    return Err(invalid("rgba-float16 ImageData requires a Float16Array"));
  }
  let view = v8::Local::<v8::ArrayBufferView>::try_from(source)
    .map_err(|_| OpError::type_("ImageData data is not a typed array"))?;
  if view
    .buffer(scope)
    .is_some_and(|buffer| buffer.was_detached())
  {
    return Err(invalid("ImageData data is detached"));
  }
  let bytes_per_pixel: u64 = if format == SampleFormat::Unorm8 { 4 } else { 8 };
  let length = view.byte_length() as u64;
  if length == 0 || !length.is_multiple_of(bytes_per_pixel) {
    return Err(invalid(
      "ImageData data length is not a multiple of a pixel",
    ));
  }
  let pixels = length / bytes_per_pixel;
  if width == 0 || !pixels.is_multiple_of(u64::from(width)) {
    return Err(OpError::dom(
      "IndexSizeError",
      "ImageData width does not divide the pixel count",
    ));
  }
  let height = u32::try_from(pixels / u64::from(width)).map_err(|_| {
    OpError::dom("IndexSizeError", "ImageData height is too large")
  })?;
  if let Some(given) = height_arg
    && given != height
  {
    return Err(OpError::dom(
      "IndexSizeError",
      "ImageData height does not match the pixel count",
    ));
  }
  if !within_limits(width, height) {
    return Err(OpError::range("ImageData is too large"));
  }
  let kept = v8::Local::<v8::Object>::try_from(source)
    .map_err(|_| OpError::type_("ImageData data is not an object"))?;
  Ok(host(scope, width, height, color_space, format, kept))
}

fn host(
  scope: &mut v8::PinScope,
  width: u32,
  height: u32,
  color_space: ColorSpace,
  format: SampleFormat,
  data: v8::Local<v8::Object>,
) -> ImageData {
  ImageData {
    width,
    height,
    color_space,
    format,
    data: RefCell::new(traced(scope, data)),
  }
}

fn allocate_with<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  raster: &Raster,
) -> Result<v8::Local<'s, v8::Object>, OpError> {
  match raster.format {
    SampleFormat::Unorm8 => {
      let elements = raster.bytes.len();
      let store =
        v8::ArrayBuffer::new_backing_store_from_vec(raster.bytes.clone())
          .make_shared();
      let buffer = v8::ArrayBuffer::with_backing_store(scope, &store);
      let array = v8::Uint8ClampedArray::new(scope, buffer, 0, elements)
        .ok_or_else(|| OpError::range("ImageData allocation failed"))?;
      let value: v8::Local<v8::Value> = array.into();
      v8::Local::<v8::Object>::try_from(value)
        .map_err(|_| OpError::range("ImageData allocation failed"))
    }
    SampleFormat::Float16 => {
      let store =
        v8::ArrayBuffer::new_backing_store_from_vec(raster.bytes.clone())
          .make_shared();
      let buffer = v8::ArrayBuffer::with_backing_store(scope, &store);
      let global = scope.get_current_context().global(scope);
      let key = v8::String::new(scope, "Float16Array").unwrap();
      let constructor = global
        .get(scope, key.into())
        .and_then(|value| v8::Local::<v8::Function>::try_from(value).ok())
        .ok_or_else(|| OpError::type_("Float16Array is not available"))?;
      let buffer: v8::Local<v8::Value> = buffer.into();
      constructor
        .new_instance(scope, &[buffer])
        .ok_or_else(|| OpError::range("ImageData allocation failed"))
    }
  }
}

fn allocate<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  width: u32,
  height: u32,
  format: SampleFormat,
) -> Result<v8::Local<'s, v8::Object>, OpError> {
  if !within_limits(width, height) {
    return Err(OpError::range("ImageData is too large"));
  }
  let pixels = u64::from(width) * u64::from(height);
  let bytes = pixels * if format == SampleFormat::Unorm8 { 4 } else { 8 };
  if bytes > MAX_BYTES {
    return Err(OpError::range("ImageData is too large"));
  }
  let len = usize::try_from(bytes)
    .map_err(|_| OpError::range("ImageData is too large"))?;
  let elements = (pixels * 4) as usize;
  match format {
    SampleFormat::Unorm8 => {
      let store =
        v8::ArrayBuffer::new_backing_store_from_vec(vec![0; len]).make_shared();
      let buffer = v8::ArrayBuffer::with_backing_store(scope, &store);
      let array = v8::Uint8ClampedArray::new(scope, buffer, 0, elements)
        .ok_or_else(|| OpError::range("ImageData allocation failed"))?;
      let value: v8::Local<v8::Value> = array.into();
      v8::Local::<v8::Object>::try_from(value)
        .map_err(|_| OpError::range("ImageData allocation failed"))
    }
    SampleFormat::Float16 => {
      // `Local<Float16Array>` does not convert to `Local<Value>` in this
      // rusty_v8, so allocate through the JavaScript constructor.
      let global = scope.get_current_context().global(scope);
      let key = v8::String::new(scope, "Float16Array").unwrap();
      let constructor = global
        .get(scope, key.into())
        .and_then(|value| v8::Local::<v8::Function>::try_from(value).ok())
        .ok_or_else(|| OpError::type_("Float16Array is not available"))?;
      let length = v8::Number::new(scope, elements as f64);
      constructor
        .new_instance(scope, &[length.into()])
        .ok_or_else(|| OpError::range("ImageData allocation failed"))
    }
  }
}

fn settings_of(
  scope: &mut v8::PinScope,
  settings: Option<v8::Local<v8::Value>>,
  default_format: SampleFormat,
) -> Result<(ColorSpace, SampleFormat), OpError> {
  let Some(settings) = settings.filter(|value| !value.is_null_or_undefined())
  else {
    return Ok((ColorSpace::Srgb, default_format));
  };
  let object = v8::Local::<v8::Object>::try_from(settings)
    .map_err(|_| OpError::type_("ImageData settings must be an object"))?;
  let color_space = match optional_string(scope, object, "colorSpace")? {
    None => ColorSpace::Srgb,
    Some(value) if value == "srgb" => ColorSpace::Srgb,
    Some(value) if value == "display-p3" => ColorSpace::DisplayP3,
    Some(_) => {
      return Err(OpError::type_(
        "The provided value is not a valid enum value of type PredefinedColorSpace",
      ));
    }
  };
  let format = match optional_string(scope, object, "pixelFormat")? {
    None => default_format,
    Some(value) if value == "rgba-unorm8" => SampleFormat::Unorm8,
    Some(value) if value == "rgba-float16" => SampleFormat::Float16,
    Some(_) => {
      return Err(OpError::type_(
        "The provided value is not a valid enum value of type ImageDataPixelFormat",
      ));
    }
  };
  Ok((color_space, format))
}

fn raster_from_bytes(
  width: u32,
  height: u32,
  color_space: ColorSpace,
  format: SampleFormat,
  bytes: &[u8],
) -> Result<Raster, OpError> {
  if !within_limits(width, height) {
    return Err(invalid("The image is too large"));
  }
  match format {
    SampleFormat::Unorm8 => {
      Raster::from_rgba8(width, height, color_space, bytes)
        .map_err(|_| invalid("ImageData data has the wrong length"))
    }
    SampleFormat::Float16 => {
      if bytes.len() != width as usize * height as usize * 8 {
        return Err(invalid("ImageData data has the wrong length"));
      }
      let mut samples =
        Vec::with_capacity(width as usize * height as usize * 4);
      for chunk in bytes.as_chunks::<2>().0 {
        let bits = u16::from_le_bytes(*chunk);
        samples.push(f64::from(half::f16::from_bits(bits)));
      }
      Raster::from_float16(width, height, color_space, &samples)
        .map_err(|_| invalid("ImageData data has the wrong length"))
    }
  }
}

fn optional_string(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
) -> Result<Option<String>, OpError> {
  let key = v8::String::new(scope, name).unwrap();
  let value = object
    .get(scope, key.into())
    .ok_or_else(|| OpError::type_("failed to read a dictionary member"))?;
  if value.is_undefined() || value.is_null() {
    return Ok(None);
  }
  let text = to_string(scope, value)?;
  Ok(Some(text))
}

fn to_string(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<String, OpError> {
  let captured = {
    v8::tc_scope!(let tc, scope);
    match value.to_string(tc) {
      Some(text) => Ok(text.to_rust_string_lossy(tc)),
      None => Err(caught(tc)),
    }
  };
  match captured {
    Ok(text) => Ok(text),
    Err(exception) => Err(rethrow(scope, exception)),
  }
}

fn to_uint32(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<u32, OpError> {
  let captured = {
    v8::tc_scope!(let tc, scope);
    match value.uint32_value(tc) {
      Some(number) => Ok(number),
      None => Err(caught(tc)),
    }
  };
  match captured {
    Ok(number) => Ok(number),
    Err(exception) => Err(rethrow(scope, exception)),
  }
}

fn is_clamped(scope: &mut v8::PinScope, value: v8::Local<v8::Value>) -> bool {
  v8::Local::<v8::Uint8ClampedArray>::try_from(value)
    .is_ok_and(|array| array.buffer(scope).is_some() || true)
}

fn is_float16(scope: &mut v8::PinScope, value: v8::Local<v8::Value>) -> bool {
  let _ = scope;
  v8::Local::<v8::Float16Array>::try_from(value).is_ok()
}

fn invalid(message: &str) -> OpError {
  OpError::dom("InvalidStateError", message)
}

fn caught(
  scope: &mut v8::PinnedRef<v8::TryCatch<v8::HandleScope>>,
) -> v8::Global<v8::Value> {
  let exception = scope
    .exception()
    .unwrap_or_else(|| v8::undefined(scope).into());
  v8::Global::new(scope, exception)
}

fn rethrow(
  scope: &mut v8::PinScope,
  exception: v8::Global<v8::Value>,
) -> OpError {
  let exception = v8::Local::new(scope, exception);
  OpError::thrown(scope, exception)
}
