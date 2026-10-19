//! `createImageBitmap`.

use yuzuko_core::op::host_ref;
use yuzuko_core::op::new_dom_exception;
use yuzuko_core::op::wrap_instance;
use yuzuko_winter::blob_parts;

use crate::bitmap::ImageBitmap;
use crate::data::ImageData;
use crate::frame::{FramePixels, frame_pixels};
use crate::raster::ColorSpace;
use crate::raster::Crop;
use crate::raster::PixelError;
use crate::raster::Raster;
use crate::raster::ResizeQuality;
use crate::raster::SampleFormat;
use crate::raster::crop_resize;
use crate::raster::decode;
use crate::raster::decode::DecodeError;
use crate::raster::flip_y;
use crate::raster::orient;
use crate::raster::premultiply;
use crate::raster::unpremultiply;
use crate::raster::within_limits;

#[derive(Clone)]
struct Options {
  from_image: bool,
  alpha: Alpha,
  to_srgb: bool,
  resize_width: Option<u32>,
  resize_height: Option<u32>,
  quality: ResizeQuality,
}

#[derive(Clone, Copy)]
enum Alpha {
  Default,
  Premultiply,
  Straight,
}

pub fn install(scope: &mut v8::PinScope) {
  let global = scope.get_current_context().global(scope);
  let function = v8::Function::new(scope, create_image_bitmap).unwrap();
  let name = v8::String::new(scope, "createImageBitmap").unwrap();
  global.set(scope, name.into(), function.into());
}

fn create_image_bitmap(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut ret: v8::ReturnValue,
) {
  let promise = start(scope, &args);
  ret.set(promise);
}

fn start<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  args: &v8::FunctionCallbackArguments,
) -> v8::Local<'s, v8::Value> {
  if args.length() < 1 {
    return reject_type(scope, "createImageBitmap requires an image");
  }
  let parsed = match parse_arguments(scope, args) {
    Ok(parsed) => parsed,
    Err(error) => return reject_error(scope, error),
  };
  if let Some(crop) = parsed.crop
    && (crop.sw == 0 || crop.sh == 0)
  {
    return reject_range(
      scope,
      "The crop rectangle has a zero width or height",
    );
  }
  if parsed.options.resize_width == Some(0)
    || parsed.options.resize_height == Some(0)
  {
    return reject_dom(
      scope,
      "InvalidStateError",
      "The resize width and height must be non-zero",
    );
  }
  let source = args.get(0);
  match blob_parts(scope, source) {
    Ok(Some((bytes, _))) => return spawn_blob(scope, bytes, parsed),
    Ok(None) => {}
    Err(_) => {
      return reject_dom(
        scope,
        "NotReadableError",
        "The blob could not be read",
      );
    }
  }
  if let Some(raster) = bitmap_source(scope, source) {
    return match raster {
      Ok(raster) => enqueue_raster(scope, raster, parsed, false),
      Err(message) => reject_dom(scope, "InvalidStateError", &message),
    };
  }
  if let Some(detached) = image_data_detached(scope, source) {
    if detached {
      return reject_dom(
        scope,
        "InvalidStateError",
        "ImageData data is detached",
      );
    }
    let object = v8::Local::<v8::Object>::try_from(source).unwrap();
    let object = v8::Global::new(scope, object);
    return enqueue_data(scope, object, parsed);
  }
  if let Some((id, width, height)) =
    yuzuko_window::offscreen_canvas(scope, source)
  {
    return match canvas_raster(id, width, height) {
      Ok(raster) => enqueue_raster(scope, raster, parsed, false),
      Err(message) => reject_dom(scope, "InvalidStateError", &message),
    };
  }
  match frame_pixels(scope, source) {
    FramePixels::Ready(raster) => {
      return enqueue_raster(scope, raster, parsed, false);
    }
    FramePixels::Closed => {
      return reject_dom(scope, "InvalidStateError", "The frame is closed");
    }
    FramePixels::NotFrame => {}
  }
  reject_type(
    scope,
    "The provided value is not of type 'ImageBitmapSource'",
  )
}

struct Parsed {
  crop: Option<Crop>,
  options: Options,
}

fn parse_arguments(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
) -> Result<Parsed, Reject> {
  let length = args.length();
  if length == 1 {
    return Ok(Parsed {
      crop: None,
      options: Options::default(),
    });
  }
  if length == 2 {
    let second = args.get(1);
    if second.is_null_or_undefined() {
      return Ok(Parsed {
        crop: None,
        options: Options::default(),
      });
    }
    if second.is_object() {
      return Ok(Parsed {
        crop: None,
        options: read_options(scope, second)?,
      });
    }
    return Err(Reject::Type(
      "createImageBitmap requires four crop arguments".to_string(),
    ));
  }
  if length < 5 {
    return Err(Reject::Type(
      "createImageBitmap requires four crop arguments".to_string(),
    ));
  }
  let crop = Crop {
    sx: to_int32(scope, args.get(1))?,
    sy: to_int32(scope, args.get(2))?,
    sw: to_int32(scope, args.get(3))?,
    sh: to_int32(scope, args.get(4))?,
  };
  let options = if length >= 6 && !args.get(5).is_null_or_undefined() {
    read_options(scope, args.get(5))?
  } else {
    Options::default()
  };
  Ok(Parsed {
    crop: Some(crop),
    options,
  })
}

impl Default for Options {
  fn default() -> Self {
    Self {
      from_image: true,
      alpha: Alpha::Default,
      to_srgb: true,
      resize_width: None,
      resize_height: None,
      quality: ResizeQuality::Nearest,
    }
  }
}

fn read_options(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<Options, Reject> {
  let object = v8::Local::<v8::Object>::try_from(value).map_err(|_| {
    Reject::Type("ImageBitmapOptions must be an object".to_string())
  })?;
  let mut options = Options::default();
  if let Some(text) = member_string(scope, object, "imageOrientation")? {
    options.from_image = match text.as_str() {
      "from-image" => true,
      "flipY" => false,
      _ => {
        return Err(Reject::Type(
          "The provided value is not a valid enum value of type ImageOrientation"
            .to_string(),
        ));
      }
    };
  }
  if let Some(text) = member_string(scope, object, "premultiplyAlpha")? {
    options.alpha = match text.as_str() {
      "default" => Alpha::Default,
      "premultiply" => Alpha::Premultiply,
      "none" => Alpha::Straight,
      _ => {
        return Err(Reject::Type(
          "The provided value is not a valid enum value of type PremultiplyAlpha"
            .to_string(),
        ));
      }
    };
  }
  if let Some(text) = member_string(scope, object, "colorSpaceConversion")? {
    options.to_srgb = match text.as_str() {
      "default" => true,
      "none" => false,
      _ => {
        return Err(Reject::Type(
          "The provided value is not a valid enum value of type ColorSpaceConversion"
            .to_string(),
        ));
      }
    };
  }
  if let Some(text) = member_string(scope, object, "resizeQuality")? {
    options.quality = match text.as_str() {
      "pixelated" | "low" => ResizeQuality::Nearest,
      "medium" | "high" => ResizeQuality::Triangle,
      _ => {
        return Err(Reject::Type(
          "The provided value is not a valid enum value of type ResizeQuality"
            .to_string(),
        ));
      }
    };
  }
  options.resize_width = member_ulong(scope, object, "resizeWidth")?;
  options.resize_height = member_ulong(scope, object, "resizeHeight")?;
  Ok(options)
}

fn spawn_blob<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  bytes: Vec<u8>,
  parsed: Parsed,
) -> v8::Local<'s, v8::Value> {
  let resolver = v8::PromiseResolver::new(scope).unwrap();
  let promise = resolver.get_promise(scope);
  let resolver = v8::Global::new(scope, resolver);
  let options = parsed.options;
  let crop = parsed.crop;
  let to_srgb = options.to_srgb;
  yuzuko_core::spawn(
    move || decode_blob(bytes, crop, options, to_srgb),
    move |scope, result| {
      let resolver = v8::Local::new(scope, resolver);
      match result {
        Ok(Ok(raster)) => {
          let bitmap =
            unsafe { wrap_instance(scope, ImageBitmap::from_raster(raster)) };
          resolver.resolve(scope, bitmap.into());
        }
        Ok(Err(message)) => reject_local(
          scope,
          resolver,
          Reject::Dom("InvalidStateError".to_string(), message),
        ),
        Err(()) => reject_local(
          scope,
          resolver,
          Reject::Dom(
            "InvalidStateError".to_string(),
            "The source image could not be decoded".to_string(),
          ),
        ),
      }
    },
  );
  promise.into()
}

fn decode_blob(
  bytes: Vec<u8>,
  crop: Option<Crop>,
  options: Options,
  to_srgb: bool,
) -> Result<Raster, String> {
  let decoded = decode::decode(&bytes, to_srgb).map_err(|error| {
    match error {
      DecodeError::TooLarge => "The image is too large",
      DecodeError::Unsupported | DecodeError::Corrupt => {
        "The source image could not be decoded"
      }
    }
    .to_string()
  })?;
  compose(decoded.raster, decoded.orientation, true, crop, &options).map_err(
    |error| {
      match error {
        PixelError::TooLarge => "The image is too large",
        PixelError::Empty => "The source image could not be decoded",
      }
      .to_string()
    },
  )
}

fn enqueue_raster<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  raster: Raster,
  parsed: Parsed,
  from_blob: bool,
) -> v8::Local<'s, v8::Value> {
  let resolver = v8::PromiseResolver::new(scope).unwrap();
  let promise = resolver.get_promise(scope);
  let resolver = v8::Global::new(scope, resolver);
  yuzuko_core::enqueue(move |scope| {
    let resolver = v8::Local::new(scope, resolver);
    match compose(raster, 1, from_blob, parsed.crop, &parsed.options) {
      Ok(raster) => {
        let bitmap =
          unsafe { wrap_instance(scope, ImageBitmap::from_raster(raster)) };
        resolver.resolve(scope, bitmap.into());
      }
      Err(error) => reject_local(scope, resolver, pixel_reject(error)),
    }
  });
  promise.into()
}

fn enqueue_data<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  object: v8::Global<v8::Object>,
  parsed: Parsed,
) -> v8::Local<'s, v8::Value> {
  let resolver = v8::PromiseResolver::new(scope).unwrap();
  let promise = resolver.get_promise(scope);
  let resolver = v8::Global::new(scope, resolver);
  yuzuko_core::enqueue(move |scope| {
    let resolver = v8::Local::new(scope, resolver);
    let object = v8::Local::new(scope, object);
    let raster = match snapshot_data(scope, object) {
      Ok(raster) => raster,
      Err(error) => {
        reject_local(scope, resolver, error);
        return;
      }
    };
    match compose(raster, 1, false, parsed.crop, &parsed.options) {
      Ok(raster) => {
        let bitmap =
          unsafe { wrap_instance(scope, ImageBitmap::from_raster(raster)) };
        resolver.resolve(scope, bitmap.into());
      }
      Err(error) => reject_local(scope, resolver, pixel_reject(error)),
    }
  });
  promise.into()
}

fn compose(
  mut raster: Raster,
  orientation: u8,
  from_blob: bool,
  crop: Option<Crop>,
  options: &Options,
) -> Result<Raster, PixelError> {
  if from_blob && options.from_image {
    raster = orient(raster, orientation)?;
  }
  raster = crop_resize(
    &raster,
    crop,
    options.resize_width,
    options.resize_height,
    options.quality,
  )?;
  if !options.from_image {
    raster = flip_y(raster)?;
  }
  match options.alpha {
    Alpha::Premultiply => premultiply(&mut raster),
    Alpha::Straight => unpremultiply(&mut raster),
    Alpha::Default => {}
  }
  Ok(raster)
}

fn canvas_raster(id: u64, width: f64, height: f64) -> Result<Raster, String> {
  if width == 0.0 || height == 0.0 {
    return Err("The canvas has no pixels".to_string());
  }
  if !width.is_finite() || !height.is_finite() || width < 0.0 || height < 0.0 {
    return Err("The canvas has no pixels".to_string());
  }
  if let Some(snapshot) = yuzuko_window::snapshot_canvas(id) {
    return raster_from_snapshot(snapshot);
  }
  let width = width as u32;
  let height = height as u32;
  if !within_limits(width, height) {
    return Err("The image is too large".to_string());
  }
  Raster::new(width, height, ColorSpace::Srgb, SampleFormat::Unorm8)
    .map_err(|_| "The image is too large".to_string())
}

fn raster_from_snapshot(
  snapshot: yuzuko_window::CanvasSnapshot,
) -> Result<Raster, String> {
  if snapshot.width == 0 || snapshot.height == 0 {
    return Err("The canvas has no pixels".to_string());
  }
  let color_space = if snapshot.display_p3 {
    ColorSpace::DisplayP3
  } else {
    ColorSpace::Srgb
  };
  let format = if snapshot.float16 {
    SampleFormat::Float16
  } else {
    SampleFormat::Unorm8
  };
  if !within_limits(snapshot.width, snapshot.height) {
    return Err("The image is too large".to_string());
  }
  let mut raster =
    Raster::new(snapshot.width, snapshot.height, color_space, format)
      .map_err(|_| "The image is too large".to_string())?;
  if raster.bytes.len() != snapshot.bytes.len() {
    return Err("The canvas has no pixels".to_string());
  }
  raster.bytes = snapshot.bytes;
  Ok(raster)
}

fn bitmap_source(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Option<Result<Raster, String>> {
  let object = v8::Local::<v8::Object>::try_from(value).ok()?;
  let host = unsafe { host_ref::<ImageBitmap>(scope, object) }?;
  if host.get().is_detached() {
    return Some(Err("The image is closed".to_string()));
  }
  let Some(raster) = host.get().copy_raster() else {
    return Some(Err("The image is closed".to_string()));
  };
  if !within_limits(raster.width, raster.height) {
    return Some(Err("The image is too large".to_string()));
  }
  Some(Ok(raster))
}

fn image_data_detached(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Option<bool> {
  let object = v8::Local::<v8::Object>::try_from(value).ok()?;
  let host = unsafe { host_ref::<ImageData>(scope, object) }?;
  Some(host.get().buffer_is_detached(scope))
}

fn snapshot_data(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
) -> Result<Raster, Reject> {
  let host =
    unsafe { host_ref::<ImageData>(scope, object) }.ok_or_else(|| {
      Reject::Dom(
        "InvalidStateError".to_string(),
        "ImageData data is detached".to_string(),
      )
    })?;
  host.get().snapshot(scope).map_err(Reject::Thrown)
}

enum Reject {
  Type(String),
  Range(String),
  Dom(String, String),
  Thrown(yuzuko_core::op::OpError),
}

fn pixel_reject(error: PixelError) -> Reject {
  let message = match error {
    PixelError::TooLarge => "The image is too large",
    PixelError::Empty => "The source image could not be decoded",
  };
  Reject::Dom("InvalidStateError".to_string(), message.to_string())
}

fn member_string(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
) -> Result<Option<String>, Reject> {
  let key = v8::String::new(scope, name).unwrap();
  let value = object.get(scope, key.into()).ok_or_else(|| {
    Reject::Type("failed to read a dictionary member".to_string())
  })?;
  if value.is_undefined() || value.is_null() {
    return Ok(None);
  }
  to_rust_string(scope, value).map(Some)
}

fn member_ulong(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
) -> Result<Option<u32>, Reject> {
  let key = v8::String::new(scope, name).unwrap();
  let value = object.get(scope, key.into()).ok_or_else(|| {
    Reject::Type("failed to read a dictionary member".to_string())
  })?;
  if value.is_undefined() {
    return Ok(None);
  }
  let number = to_number(scope, value)?;
  enforce_unsigned_long(number)
}

fn enforce_unsigned_long(number: f64) -> Result<Option<u32>, Reject> {
  if !number.is_finite() {
    return Err(Reject::Type(
      "The provided value cannot be converted to 'unsigned long'".to_string(),
    ));
  }
  let rounded = if number == 0.0 {
    0.0
  } else {
    number.round_ties_even()
  };
  if rounded < 0.0 || rounded > 4_294_967_295.0 {
    return Err(Reject::Type(
      "The provided value is outside the 'unsigned long' range".to_string(),
    ));
  }
  Ok(Some(rounded as u32))
}

fn to_int32(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<i32, Reject> {
  let captured = {
    v8::tc_scope!(let tc, scope);
    match value.int32_value(tc) {
      Some(number) => Ok(number),
      None => Err(caught(tc)),
    }
  };
  match captured {
    Ok(number) => Ok(number),
    Err(exception) => Err(Reject::Thrown(rethrow(scope, exception))),
  }
}

fn to_number(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<f64, Reject> {
  let captured = {
    v8::tc_scope!(let tc, scope);
    match value.number_value(tc) {
      Some(number) => Ok(number),
      None => Err(caught(tc)),
    }
  };
  match captured {
    Ok(number) => Ok(number),
    Err(exception) => Err(Reject::Thrown(rethrow(scope, exception))),
  }
}

fn to_rust_string(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<String, Reject> {
  let captured = {
    v8::tc_scope!(let tc, scope);
    match value.to_string(tc) {
      Some(text) => Ok(text.to_rust_string_lossy(tc)),
      None => Err(caught(tc)),
    }
  };
  match captured {
    Ok(text) => Ok(text),
    Err(exception) => Err(Reject::Thrown(rethrow(scope, exception))),
  }
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
) -> yuzuko_core::op::OpError {
  let exception = v8::Local::new(scope, exception);
  yuzuko_core::op::OpError::thrown(scope, exception)
}

fn reject_type<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  message: &str,
) -> v8::Local<'s, v8::Value> {
  reject_now(scope, Reject::Type(message.to_string()))
}

fn reject_range<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  message: &str,
) -> v8::Local<'s, v8::Value> {
  reject_now(scope, Reject::Range(message.to_string()))
}

fn reject_dom<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  name: &str,
  message: &str,
) -> v8::Local<'s, v8::Value> {
  reject_now(scope, Reject::Dom(name.to_string(), message.to_string()))
}

fn reject_error<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  error: Reject,
) -> v8::Local<'s, v8::Value> {
  reject_now(scope, error)
}

fn reject_now<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  error: Reject,
) -> v8::Local<'s, v8::Value> {
  let resolver = v8::PromiseResolver::new(scope).unwrap();
  let promise = resolver.get_promise(scope);
  reject_local(scope, resolver, error);
  promise.into()
}

fn reject_local(
  scope: &mut v8::PinScope,
  resolver: v8::Local<v8::PromiseResolver>,
  error: Reject,
) {
  let exception = exception_value(scope, error);
  resolver.reject(scope, exception);
}

fn exception_value<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  error: Reject,
) -> v8::Local<'s, v8::Value> {
  match error {
    Reject::Type(message) => {
      let message = v8::String::new(scope, &message).unwrap();
      v8::Exception::type_error(scope, message)
    }
    Reject::Range(message) => {
      let message = v8::String::new(scope, &message).unwrap();
      v8::Exception::range_error(scope, message)
    }
    Reject::Dom(name, message) => new_dom_exception(scope, &name, &message)
      .map(v8::Local::<v8::Value>::from)
      .unwrap_or_else(|| {
        let message = v8::String::new(scope, &message).unwrap();
        v8::Exception::error(scope, message)
      }),
    Reject::Thrown(error) => {
      // Build the same exception `#[op]` would throw, then reject with it.
      let message = match error {
        yuzuko_core::op::OpError::Range(message) => {
          let message = v8::String::new(scope, &message).unwrap();
          return v8::Exception::range_error(scope, message);
        }
        yuzuko_core::op::OpError::Type(message) => message,
        yuzuko_core::op::OpError::Dom { name, message } => {
          return new_dom_exception(scope, &name, &message)
            .map(v8::Local::<v8::Value>::from)
            .unwrap_or_else(|| {
              let message = v8::String::new(scope, &message).unwrap();
              v8::Exception::error(scope, message)
            });
        }
        yuzuko_core::op::OpError::Thrown(value) => {
          return v8::Local::new(scope, *value);
        }
      };
      let message = v8::String::new(scope, &message).unwrap();
      v8::Exception::type_error(scope, message)
    }
  }
}
