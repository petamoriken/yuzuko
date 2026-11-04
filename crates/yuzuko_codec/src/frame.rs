//! `VideoFrame` constructors, `allocationSize`, and `copyTo`.

use yuzuko_core::op::{Js, OpError, host_ref, new_dom_exception};
use yuzuko_core::rust_string;
use yuzuko_image::{ColorSpace, ImageBitmap, Raster, SampleFormat};
use yuzuko_window::{CanvasSnapshot, OffscreenCanvas, snapshot_canvas};
use yuzuko_winter::buffer_source;

use crate::classes::{
  COLOR_MATRIX, COLOR_PRIMARIES, COLOR_TRANSFER, VideoFrame, closed_frame,
  color_bool, color_enum, dictionary, long_long, member,
};
use crate::layout::{
  FormatClass, FrameColor, FrameData, LayoutError, PixelFormat, PlaneLayoutIn,
  PlaneLayoutOut, RectD, allocation, classify, discard_alpha, display_size,
  full_rect, limited_range, parse_visible, raster_color, read_buffer,
  rgba_plane, visible_raster, write_copy,
};

const MAX_EDGE: u32 = 16_384;
const MAX_PIXELS: u64 = 100_000_000;

struct ImageInit {
  timestamp: Option<f64>,
  duration: Option<f64>,
  /// `duration` was present in `init`, including an explicit null.
  duration_set: bool,
  discard: bool,
  visible: Option<RectD>,
  display: Option<(u32, u32)>,
}

struct CopyOptions {
  rect: Option<RectD>,
  layout: Option<Vec<PlaneLayoutIn>>,
  format: Option<PixelFormat>,
  /// Present only when `options.colorSpace` was set.
  color_space: Option<ColorSpace>,
}

pub(crate) fn construct(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
) -> Result<VideoFrame, OpError> {
  if args.length() < 1 || args.get(0).is_null_or_undefined() {
    return Err(OpError::type_(
      "VideoFrame constructor requires an image or a buffer",
    ));
  }
  let source = args.get(0);
  if is_buffer_source(source) {
    if args.length() < 2 || args.get(1).is_null_or_undefined() {
      return Err(OpError::type_("VideoFrameBufferInit is required"));
    }
    return construct_buffer(scope, source, args.get(1));
  }
  let object = v8::Local::<v8::Object>::try_from(source).map_err(|_| {
    OpError::type_("The image is not a supported VideoFrame source")
  })?;
  if let Some(frame) = unsafe { host_ref::<VideoFrame>(scope, object) } {
    return construct_from_frame(scope, frame.get(), args);
  }
  if let Some(bitmap) = unsafe { host_ref::<ImageBitmap>(scope, object) } {
    return construct_from_bitmap(scope, bitmap.get(), args);
  }
  if let Some(canvas) = unsafe { host_ref::<OffscreenCanvas>(scope, object) } {
    return construct_from_canvas(scope, canvas.get(), args);
  }
  Err(OpError::type_(
    "The image is not a supported VideoFrame source",
  ))
}

pub(crate) fn allocation_size(
  scope: &mut v8::PinScope,
  frame: &VideoFrame,
  args: &v8::FunctionCallbackArguments,
) -> Result<f64, OpError> {
  // `allocationSize(options)` has no destination, so the dictionary is argument 0.
  let options = copy_options(scope, args, 0)?;
  let data = frame.snapshot().ok_or_else(closed_frame)?;
  let (rect, format, layout) = copy_target(&data, &options)?;
  let (size, _) = allocation(format, rect, layout)
    .map_err(|error| layout_error(error, false))?;
  Ok(size as f64)
}

pub(crate) fn copy_to(
  scope: &mut v8::PinScope,
  frame: &VideoFrame,
  args: &v8::FunctionCallbackArguments,
) -> Result<Js, OpError> {
  let outcome = (|| {
    // `copyTo(destination, options)` keeps the dictionary in argument 1.
    let options = copy_options(scope, args, 1)?;
    let data = frame.snapshot().ok_or_else(closed_frame)?;
    let (rect, format, layout) = copy_target(&data, &options)?;
    let (size, layouts) = allocation(format, rect, layout)
      .map_err(|error| layout_error(error, false))?;
    let limited = limited_range(data.format, &data.color);
    let dest_color = rgb_destination_color(options.format, options.color_space);
    // Write sample bytes only, so stride padding already in the destination
    // stays as the caller left it.
    with_destination(scope, args.get(0), size, |dest| {
      write_copy(&data, rect, format, layout, limited, dest_color, dest)
        .map(|_| ())
        .map_err(|error| layout_error(error, false))
    })?;
    Ok(layouts)
  })();
  settle(scope, outcome)
}

pub(crate) fn visible_rect(scope: &mut v8::PinScope, frame: &VideoFrame) -> Js {
  match frame.snapshot() {
    Some(data) => rect_value(
      scope,
      data.visible_x,
      data.visible_y,
      data.visible_width,
      data.visible_height,
    ),
    None => null_value(scope),
  }
}

pub(crate) fn coded_rect(scope: &mut v8::PinScope, frame: &VideoFrame) -> Js {
  match frame.snapshot() {
    Some(data) => rect_value(
      scope,
      0.0,
      0.0,
      f64::from(data.coded_width),
      f64::from(data.coded_height),
    ),
    None => null_value(scope),
  }
}

fn construct_buffer(
  scope: &mut v8::PinScope,
  source: v8::Local<v8::Value>,
  init: v8::Local<v8::Value>,
) -> Result<VideoFrame, OpError> {
  let object = dictionary(scope, init, "VideoFrameBufferInit")?;
  reject_orientation(scope, object)?;
  let format = pixel_format(scope, object)?;
  let coded_width = required_size(scope, object, "codedWidth")?;
  let coded_height = required_size(scope, object, "codedHeight")?;
  ensure_size(coded_width, coded_height)?;
  let timestamp = required_timestamp(scope, object, None)?;
  let duration = optional_duration(scope, object)?;
  let color = color_of(scope, object, format)?;
  let visible = optional_rect(scope, object, "visibleRect")?;
  let visible = parse_visible(
    full_rect(coded_width, coded_height),
    visible,
    coded_width,
    coded_height,
    format,
  )
  .map_err(|error| layout_error(error, true))?;
  let layout = optional_layout(scope, object)?;
  let bytes = buffer_source(scope, source)
    .ok_or_else(|| OpError::type_("data is not a BufferSource"))?;
  let pixels = read_buffer(
    &bytes,
    format,
    coded_width,
    coded_height,
    visible,
    layout.as_deref(),
  )
  .map_err(|error| layout_error(error, true))?;
  let visible = full_rect(pixels.coded_width, pixels.coded_height);
  let display = match display_pair(scope, object)? {
    Some(pair) => pair,
    None => (pixels.coded_width, pixels.coded_height),
  };
  assemble(
    format,
    pixels.coded_width,
    pixels.coded_height,
    visible,
    display,
    color,
    pixels.planes,
    timestamp,
    duration,
  )
}

fn construct_from_bitmap(
  scope: &mut v8::PinScope,
  bitmap: &ImageBitmap,
  args: &v8::FunctionCallbackArguments,
) -> Result<VideoFrame, OpError> {
  if bitmap.is_detached() {
    return Err(OpError::dom("InvalidStateError", "The image is closed"));
  }
  let Some(raster) = bitmap.copy_raster() else {
    return Err(OpError::dom("InvalidStateError", "The image is closed"));
  };
  image_frame(scope, args, &raster)
}

fn construct_from_canvas(
  scope: &mut v8::PinScope,
  canvas: &OffscreenCanvas,
  args: &v8::FunctionCallbackArguments,
) -> Result<VideoFrame, OpError> {
  if canvas.is_detached() {
    return Err(OpError::dom(
      "InvalidStateError",
      "The canvas has been transferred",
    ));
  }
  let (width, height) = canvas.bitmap();
  if width == 0.0 || height == 0.0 || !width.is_finite() || !height.is_finite()
  {
    return Err(OpError::dom(
      "InvalidStateError",
      "The canvas has no pixels",
    ));
  }
  let raster = if let Some(snapshot) = snapshot_canvas(canvas.id()) {
    raster_from_snapshot(snapshot)?
  } else {
    let width = width as u32;
    let height = height as u32;
    ensure_size(width, height)?;
    Raster::new(width, height, ColorSpace::Srgb, SampleFormat::Unorm8)
      .map_err(|_| OpError::type_("The frame is too large"))?
  };
  image_frame(scope, args, &raster)
}

fn construct_from_frame(
  scope: &mut v8::PinScope,
  source: &VideoFrame,
  args: &v8::FunctionCallbackArguments,
) -> Result<VideoFrame, OpError> {
  let mut data = source.snapshot().ok_or_else(closed_frame)?;
  let init = image_init(scope, args, Some(source.timestamp_us()))?;
  if init.discard {
    data.format = discard_alpha(data.format, &mut data.planes);
  }
  let visible = if let Some(visible) = init.visible {
    parse_visible(
      full_rect(data.coded_width, data.coded_height),
      Some(visible),
      data.coded_width,
      data.coded_height,
      data.format,
    )
    .map_err(|error| layout_error(error, true))?
  } else {
    RectD {
      x: data.visible_x,
      y: data.visible_y,
      width: data.visible_width,
      height: data.visible_height,
    }
  };
  let display = if let Some(display) = init.display {
    display
  } else {
    display_size(
      visible.width,
      visible.height,
      data.visible_width,
      data.visible_height,
      f64::from(data.display_width),
      f64::from(data.display_height),
    )
    .map_err(|error| layout_error(error, true))?
  };
  let timestamp = init.timestamp.unwrap_or_else(|| source.timestamp_us());
  let duration = if init.duration_set {
    init.duration
  } else {
    source.duration_us()
  };
  assemble(
    data.format,
    data.coded_width,
    data.coded_height,
    visible,
    display,
    data.color,
    data.planes,
    timestamp,
    duration,
  )
}

fn image_frame(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
  raster: &Raster,
) -> Result<VideoFrame, OpError> {
  ensure_size(raster.width, raster.height)?;
  let init = image_init(scope, args, None)?;
  let Some(timestamp) = init.timestamp else {
    return Err(OpError::type_("timestamp is required"));
  };
  let mut format = PixelFormat::Rgba;
  let mut planes = vec![rgba_plane(raster)];
  if init.discard {
    format = discard_alpha(format, &mut planes);
  }
  let visible = parse_visible(
    full_rect(raster.width, raster.height),
    init.visible,
    raster.width,
    raster.height,
    format,
  )
  .map_err(|error| layout_error(error, true))?;
  let display = if let Some(display) = init.display {
    display
  } else {
    display_size(
      visible.width,
      visible.height,
      f64::from(raster.width),
      f64::from(raster.height),
      f64::from(raster.width),
      f64::from(raster.height),
    )
    .map_err(|error| layout_error(error, true))?
  };
  let color = FrameColor::from_color_space(raster.color_space);
  assemble(
    format,
    raster.width,
    raster.height,
    visible,
    display,
    color,
    planes,
    timestamp,
    init.duration,
  )
}

fn assemble(
  format: PixelFormat,
  coded_width: u32,
  coded_height: u32,
  visible: RectD,
  display: (u32, u32),
  color: FrameColor,
  planes: Vec<Vec<u8>>,
  timestamp: f64,
  duration: Option<f64>,
) -> Result<VideoFrame, OpError> {
  let raster = visible_raster(
    format,
    &planes,
    coded_width,
    coded_height,
    visible,
    raster_color(&color),
    limited_range(format, &color),
  )
  .map_err(|error| layout_error(error, true))?;
  Ok(VideoFrame::from_data(
    FrameData {
      format,
      coded_width,
      coded_height,
      visible_x: visible.x,
      visible_y: visible.y,
      visible_width: visible.width,
      visible_height: visible.height,
      display_width: display.0,
      display_height: display.1,
      color,
      planes,
      raster,
    },
    timestamp,
    duration,
  ))
}

fn copy_target<'a>(
  data: &FrameData,
  options: &'a CopyOptions,
) -> Result<(RectD, PixelFormat, Option<&'a [PlaneLayoutIn]>), OpError> {
  let rect = parse_visible(
    RectD {
      x: data.visible_x,
      y: data.visible_y,
      width: data.visible_width,
      height: data.visible_height,
    },
    options.rect,
    data.coded_width,
    data.coded_height,
    data.format,
  )
  .map_err(|error| layout_error(error, false))?;
  let format = match options.format {
    None => data.format,
    Some(format) if format == data.format || format.is_rgb() => format,
    Some(_) => {
      return Err(OpError::dom(
        "NotSupportedError",
        "The pixel format is not supported",
      ));
    }
  };
  Ok((rect, format, options.layout.as_deref()))
}

fn image_init(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
  timestamp_fallback: Option<f64>,
) -> Result<ImageInit, OpError> {
  let Some(object) = init_object(scope, args)? else {
    return Ok(ImageInit {
      timestamp: timestamp_fallback,
      duration: None,
      duration_set: false,
      discard: false,
      visible: None,
      display: None,
    });
  };
  reject_orientation(scope, object)?;
  let (duration_set, duration) = duration_member(scope, object)?;
  Ok(ImageInit {
    timestamp: optional_timestamp(scope, object, timestamp_fallback)?,
    duration,
    duration_set,
    discard: alpha_discard(scope, object)?,
    visible: optional_rect(scope, object, "visibleRect")?,
    display: display_pair(scope, object)?,
  })
}

fn init_object<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  args: &v8::FunctionCallbackArguments,
) -> Result<Option<v8::Local<'s, v8::Object>>, OpError> {
  if args.length() < 2 || args.get(1).is_null_or_undefined() {
    return Ok(None);
  }
  Ok(Some(dictionary(scope, args.get(1), "VideoFrameInit")?))
}

fn copy_options(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
  index: i32,
) -> Result<CopyOptions, OpError> {
  if args.length() <= index || args.get(index).is_null_or_undefined() {
    return Ok(CopyOptions {
      rect: None,
      layout: None,
      format: None,
      color_space: None,
    });
  }
  let object = dictionary(scope, args.get(index), "VideoFrameCopyToOptions")?;
  let color_space = match member(scope, object, "colorSpace") {
    None => None,
    Some(value) => match rust_string(scope, value).as_str() {
      "srgb" => Some(ColorSpace::Srgb),
      "display-p3" => Some(ColorSpace::DisplayP3),
      _ => return Err(OpError::type_("colorSpace is invalid")),
    },
  };
  let format = match member(scope, object, "format") {
    None => None,
    Some(value) => Some(require_format(&rust_string(scope, value))?),
  };
  Ok(CopyOptions {
    rect: optional_rect(scope, object, "rect")?,
    layout: optional_layout(scope, object)?,
    format,
    color_space,
  })
}

/// An explicit RGB `format` converts into `colorSpace`, which defaults to sRGB.
/// A same-format copy ignores `colorSpace`.
fn rgb_destination_color(
  format: Option<PixelFormat>,
  color: Option<ColorSpace>,
) -> Option<ColorSpace> {
  match format {
    Some(format) if format.is_rgb() => Some(color.unwrap_or(ColorSpace::Srgb)),
    _ => None,
  }
}

fn pixel_format(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
) -> Result<PixelFormat, OpError> {
  let Some(value) = member(scope, object, "format") else {
    return Err(OpError::type_("format is required"));
  };
  require_format(&rust_string(scope, value))
}

fn require_format(name: &str) -> Result<PixelFormat, OpError> {
  match classify(name) {
    FormatClass::Stored(format) => Ok(format),
    FormatClass::Unsupported => Err(OpError::dom(
      "NotSupportedError",
      "The pixel format is not supported",
    )),
    FormatClass::Unknown => Err(OpError::type_("The format is invalid")),
  }
}

fn optional_rect(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
) -> Result<Option<RectD>, OpError> {
  let Some(value) = member(scope, object, name) else {
    return Ok(None);
  };
  if value.is_null() {
    return Ok(None);
  }
  let rect = dictionary(scope, value, "visibleRect")?;
  Ok(Some(RectD {
    x: finite_member(scope, rect, "x")?,
    y: finite_member(scope, rect, "y")?,
    width: finite_member(scope, rect, "width")?,
    height: finite_member(scope, rect, "height")?,
  }))
}

fn optional_layout(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
) -> Result<Option<Vec<PlaneLayoutIn>>, OpError> {
  let Some(value) = member(scope, object, "layout") else {
    return Ok(None);
  };
  if value.is_null() {
    return Ok(None);
  }
  let array = v8::Local::<v8::Array>::try_from(value)
    .map_err(|_| OpError::type_("layout is not a sequence"))?;
  let mut layouts = Vec::with_capacity(array.length() as usize);
  for index in 0..array.length() {
    let item = array
      .get_index(scope, index)
      .ok_or_else(|| OpError::type_("layout is not a sequence"))?;
    let plane = dictionary(scope, item, "PlaneLayout")?;
    let offset = required_enforce(scope, plane, "offset")?;
    let stride = required_enforce(scope, plane, "stride")?;
    layouts.push(PlaneLayoutIn {
      offset: u64::from(offset),
      stride: u64::from(stride),
    });
  }
  Ok(Some(layouts))
}

fn display_pair(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
) -> Result<Option<(u32, u32)>, OpError> {
  let width = member(scope, object, "displayWidth");
  let height = member(scope, object, "displayHeight");
  match (width, height) {
    (None, None) => Ok(None),
    (Some(width), Some(height)) => {
      let width = enforce_u32(scope, width, "displayWidth")?;
      let height = enforce_u32(scope, height, "displayHeight")?;
      if width == 0 || height == 0 {
        return Err(OpError::type_("displayWidth is out of range"));
      }
      Ok(Some((width, height)))
    }
    _ => Err(OpError::type_(
      "displayWidth and displayHeight are required together",
    )),
  }
}

fn required_timestamp(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  fallback: Option<f64>,
) -> Result<f64, OpError> {
  match optional_timestamp(scope, object, fallback)? {
    Some(timestamp) => Ok(timestamp),
    None => Err(OpError::type_("timestamp is required")),
  }
}

fn optional_timestamp(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  fallback: Option<f64>,
) -> Result<Option<f64>, OpError> {
  let Some(value) = member(scope, object, "timestamp") else {
    return Ok(fallback);
  };
  if value.is_null() {
    return Err(OpError::type_("timestamp is required"));
  }
  Ok(Some(long_long(scope, value)? as f64))
}

fn duration_member(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
) -> Result<(bool, Option<f64>), OpError> {
  let Some(value) = member(scope, object, "duration") else {
    return Ok((false, None));
  };
  if value.is_null() {
    return Ok((true, None));
  }
  let number = value
    .number_value(scope)
    .ok_or_else(|| OpError::type_("duration is not a number"))?;
  if !number.is_finite() || number < 0.0 || number > u64::MAX as f64 {
    return Err(OpError::type_("duration is out of range"));
  }
  Ok((true, Some(number.trunc())))
}

fn optional_duration(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
) -> Result<Option<f64>, OpError> {
  Ok(duration_member(scope, object)?.1)
}

fn alpha_discard(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
) -> Result<bool, OpError> {
  let Some(value) = member(scope, object, "alpha") else {
    return Ok(false);
  };
  match rust_string(scope, value).as_str() {
    "keep" => Ok(false),
    "discard" => Ok(true),
    _ => Err(OpError::type_("alpha is invalid")),
  }
}

fn color_of(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  format: PixelFormat,
) -> Result<FrameColor, OpError> {
  let Some(value) = member(scope, object, "colorSpace") else {
    return Ok(if format.is_rgb() {
      FrameColor::srgb()
    } else {
      FrameColor::rec709()
    });
  };
  if value.is_null() {
    return Ok(FrameColor::empty());
  }
  let dict = dictionary(scope, value, "colorSpace")?;
  Ok(FrameColor {
    primaries: color_enum(scope, dict, "primaries", COLOR_PRIMARIES)?,
    transfer: color_enum(scope, dict, "transfer", COLOR_TRANSFER)?,
    matrix: color_enum(scope, dict, "matrix", COLOR_MATRIX)?,
    full_range: color_bool(scope, dict, "fullRange")?,
  })
}

fn reject_orientation(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
) -> Result<(), OpError> {
  if let Some(value) = member(scope, object, "rotation")
    && !value.is_null()
  {
    let number = value
      .number_value(scope)
      .ok_or_else(|| OpError::type_("rotation is invalid"))?;
    if !number.is_finite() {
      return Err(OpError::type_("rotation is invalid"));
    }
    let aligned = (number / 90.0).round() * 90.0;
    let mut reduced = aligned % 360.0;
    if reduced < 0.0 {
      reduced += 360.0;
    }
    if reduced != 0.0 {
      return Err(OpError::dom(
        "NotSupportedError",
        "VideoFrame rotation is not supported",
      ));
    }
  }
  if let Some(value) = member(scope, object, "flip")
    && value.boolean_value(scope)
  {
    return Err(OpError::dom(
      "NotSupportedError",
      "VideoFrame flip is not supported",
    ));
  }
  Ok(())
}

fn required_size(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
) -> Result<u32, OpError> {
  let Some(value) = member(scope, object, name) else {
    return Err(OpError::type_(format!("{name} is required")));
  };
  let size = enforce_u32(scope, value, name)?;
  if size == 0 {
    return Err(OpError::type_(format!("{name} is out of range")));
  }
  Ok(size)
}

fn required_enforce(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
) -> Result<u32, OpError> {
  let Some(value) = member(scope, object, name) else {
    return Err(OpError::type_(format!("{name} is required")));
  };
  enforce_u32(scope, value, name)
}

fn enforce_u32(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
  name: &str,
) -> Result<u32, OpError> {
  if value.is_null() {
    return Err(OpError::type_(format!("{name} is out of range")));
  }
  let number = value
    .number_value(scope)
    .ok_or_else(|| OpError::type_(format!("{name} is not a number")))?;
  if !number.is_finite()
    || number < 0.0
    || number > f64::from(u32::MAX)
    || number.fract() != 0.0
  {
    return Err(OpError::type_(format!("{name} is out of range")));
  }
  Ok(number as u32)
}

fn finite_member(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
) -> Result<f64, OpError> {
  let Some(value) = member(scope, object, name) else {
    return Ok(0.0);
  };
  if value.is_null() {
    return Ok(0.0);
  }
  value
    .number_value(scope)
    .ok_or_else(|| OpError::type_(format!("{name} is not a number")))
}

fn ensure_size(width: u32, height: u32) -> Result<(), OpError> {
  if width == 0
    || height == 0
    || width > MAX_EDGE
    || height > MAX_EDGE
    || u64::from(width) * u64::from(height) > MAX_PIXELS
  {
    return Err(OpError::type_("The frame is too large"));
  }
  Ok(())
}

fn raster_from_snapshot(snapshot: CanvasSnapshot) -> Result<Raster, OpError> {
  if snapshot.width == 0 || snapshot.height == 0 {
    return Err(OpError::dom(
      "InvalidStateError",
      "The canvas has no pixels",
    ));
  }
  ensure_size(snapshot.width, snapshot.height)?;
  let color = if snapshot.display_p3 {
    ColorSpace::DisplayP3
  } else {
    ColorSpace::Srgb
  };
  let format = if snapshot.float16 {
    SampleFormat::Float16
  } else {
    SampleFormat::Unorm8
  };
  let mut raster = Raster::new(snapshot.width, snapshot.height, color, format)
    .map_err(|_| OpError::type_("The frame is too large"))?;
  if raster.bytes.len() != snapshot.bytes.len() {
    return Err(OpError::dom(
      "InvalidStateError",
      "The canvas has no pixels",
    ));
  }
  raster.bytes = snapshot.bytes;
  Ok(raster)
}

fn is_buffer_source(value: v8::Local<v8::Value>) -> bool {
  v8::Local::<v8::ArrayBuffer>::try_from(value).is_ok()
    || v8::Local::<v8::SharedArrayBuffer>::try_from(value).is_ok()
    || v8::Local::<v8::ArrayBufferView>::try_from(value).is_ok()
}

fn layout_error(error: LayoutError, constructing: bool) -> OpError {
  let message = match error {
    LayoutError::PlaneCount => "The layout plane count is invalid",
    LayoutError::Stride => "The stride is too small",
    LayoutError::Overflow => "The layout exceeds the address space",
    LayoutError::Overlap => "The plane layout overlaps",
    LayoutError::Rect => "The visible rectangle is invalid",
    LayoutError::Alignment => "The visible rectangle is not sample aligned",
    LayoutError::TooSmall => {
      if constructing {
        "data is too small"
      } else {
        "The destination is too small"
      }
    }
    LayoutError::Format => "The pixel format is not supported",
  };
  if error == LayoutError::Format {
    OpError::dom("NotSupportedError", message)
  } else {
    OpError::type_(message)
  }
}

pub(crate) fn write_bytes(
  scope: &mut v8::PinScope,
  dest: v8::Local<v8::Value>,
  bytes: &[u8],
) -> Result<(), OpError> {
  if bytes.is_empty() {
    return require_buffer(scope, dest);
  }
  let size = u64::try_from(bytes.len())
    .map_err(|_| OpError::type_("The destination is too small"))?;
  with_destination(scope, dest, size, |slice| {
    let into = slice
      .get_mut(..bytes.len())
      .ok_or_else(|| OpError::type_("The destination is too small"))?;
    into.copy_from_slice(bytes);
    Ok(())
  })
}

fn require_buffer(
  scope: &mut v8::PinScope,
  dest: v8::Local<v8::Value>,
) -> Result<(), OpError> {
  if v8::Local::<v8::SharedArrayBuffer>::try_from(dest).is_ok() {
    return Ok(());
  }
  if let Ok(view) = v8::Local::<v8::ArrayBufferView>::try_from(dest) {
    if view
      .buffer(scope)
      .is_some_and(|buffer| buffer.was_detached())
    {
      return Err(OpError::type_("The destination is detached"));
    }
    return Ok(());
  }
  if let Ok(buffer) = v8::Local::<v8::ArrayBuffer>::try_from(dest) {
    if buffer.was_detached() {
      return Err(OpError::type_("The destination is detached"));
    }
    return Ok(());
  }
  Err(OpError::type_("The destination is not a BufferSource"))
}

fn with_destination(
  scope: &mut v8::PinScope,
  dest: v8::Local<v8::Value>,
  size: u64,
  write: impl FnOnce(&mut [u8]) -> Result<(), OpError>,
) -> Result<(), OpError> {
  let size = usize::try_from(size)
    .map_err(|_| OpError::type_("The layout exceeds the address space"))?;
  if let Ok(view) = v8::Local::<v8::ArrayBufferView>::try_from(dest) {
    if view.byte_length() < size {
      return Err(OpError::type_("The destination is too small"));
    }
    if let Some(buffer) = view.buffer(scope) {
      if buffer.was_detached() {
        return Err(OpError::type_("The destination is too small"));
      }
      return with_buffer(
        &buffer,
        view.byte_offset(),
        view.byte_length(),
        write,
      );
    }
    // A SharedArrayBuffer view has no ArrayBuffer. `data` includes the offset.
    let data = view.data();
    if data.is_null() {
      return Err(OpError::type_("The destination is detached"));
    }
    let slice = unsafe {
      std::slice::from_raw_parts_mut(data.cast::<u8>(), view.byte_length())
    };
    return write(slice);
  }
  if let Ok(buffer) = v8::Local::<v8::ArrayBuffer>::try_from(dest) {
    if buffer.was_detached() || buffer.byte_length() < size {
      return Err(OpError::type_("The destination is too small"));
    }
    return with_buffer(&buffer, 0, buffer.byte_length(), write);
  }
  if let Ok(buffer) = v8::Local::<v8::SharedArrayBuffer>::try_from(dest) {
    if buffer.byte_length() < size {
      return Err(OpError::type_("The destination is too small"));
    }
    let store = buffer.get_backing_store();
    let Some(data) = store.data() else {
      return Err(OpError::type_("The destination is detached"));
    };
    let slice = unsafe {
      std::slice::from_raw_parts_mut(
        data.as_ptr().cast::<u8>(),
        buffer.byte_length(),
      )
    };
    return write(slice);
  }
  Err(OpError::type_("The destination is not a BufferSource"))
}

fn with_buffer(
  buffer: &v8::ArrayBuffer,
  offset: usize,
  length: usize,
  write: impl FnOnce(&mut [u8]) -> Result<(), OpError>,
) -> Result<(), OpError> {
  let store = buffer.get_backing_store();
  let Some(data) = store.data() else {
    return Err(OpError::type_("The destination is detached"));
  };
  let slice = unsafe {
    std::slice::from_raw_parts_mut(
      data.as_ptr().cast::<u8>().add(offset),
      length,
    )
  };
  write(slice)
}

fn settle(
  scope: &mut v8::PinScope,
  result: Result<Vec<PlaneLayoutOut>, OpError>,
) -> Result<Js, OpError> {
  let Some(resolver) = v8::PromiseResolver::new(scope) else {
    return Err(OpError::type_("Promise is unavailable"));
  };
  let promise = resolver.get_promise(scope);
  match result {
    Ok(layouts) => {
      let value = layout_array(scope, &layouts);
      resolver.resolve(scope, value);
    }
    Err(error) => {
      let rejected = exception_value(scope, error);
      resolver.reject(scope, rejected);
    }
  }
  let promise: v8::Local<v8::Value> = promise.into();
  Ok(Js(v8::Global::new(scope, promise)))
}

fn layout_array<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  layouts: &[PlaneLayoutOut],
) -> v8::Local<'s, v8::Value> {
  let array = v8::Array::new(scope, layouts.len() as i32);
  for (index, layout) in layouts.iter().enumerate() {
    let object = v8::Object::new(scope);
    let offset = v8::Number::new(scope, layout.offset as f64);
    let stride = v8::Number::new(scope, layout.stride as f64);
    let offset_key = v8::String::new(scope, "offset").unwrap();
    let stride_key = v8::String::new(scope, "stride").unwrap();
    object.set(scope, offset_key.into(), offset.into());
    object.set(scope, stride_key.into(), stride.into());
    array.set_index(scope, index as u32, object.into());
  }
  array.into()
}

fn rect_value(
  scope: &mut v8::PinScope,
  x: f64,
  y: f64,
  width: f64,
  height: f64,
) -> Js {
  let value = dom_rect(scope, x, y, width, height);
  Js(v8::Global::new(scope, value))
}

fn null_value(scope: &mut v8::PinScope) -> Js {
  let value: v8::Local<v8::Value> = v8::null(scope).into();
  Js(v8::Global::new(scope, value))
}

fn dom_rect<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  x: f64,
  y: f64,
  width: f64,
  height: f64,
) -> v8::Local<'s, v8::Value> {
  let global = yuzuko_core::global(scope);
  if let Some(key) = v8::String::new(scope, "DOMRectReadOnly")
    && let Some(ctor) = global.get(scope, key.into())
    && let Ok(ctor) = v8::Local::<v8::Function>::try_from(ctor)
  {
    let args = [
      v8::Number::new(scope, x).into(),
      v8::Number::new(scope, y).into(),
      v8::Number::new(scope, width).into(),
      v8::Number::new(scope, height).into(),
    ];
    if let Some(object) = ctor.new_instance(scope, &args) {
      return object.into();
    }
  }
  let object = v8::Object::new(scope);
  for (name, value) in [
    ("x", x),
    ("y", y),
    ("width", width),
    ("height", height),
    ("top", y),
    ("left", x),
    ("right", x + width),
    ("bottom", y + height),
  ] {
    let key = v8::String::new(scope, name).unwrap();
    object.set(scope, key.into(), v8::Number::new(scope, value).into());
  }
  object.into()
}

fn exception_value<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  error: OpError,
) -> v8::Local<'s, v8::Value> {
  match error {
    OpError::Type(message) => builtin_error(scope, &message, false),
    OpError::Range(message) => builtin_error(scope, &message, true),
    OpError::Dom { name, message } => new_dom_exception(scope, &name, &message)
      .map(v8::Local::<v8::Value>::from)
      .unwrap_or_else(|| v8::undefined(scope).into()),
    OpError::Thrown(value) => v8::Local::new(scope, *value),
  }
}

fn builtin_error<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  message: &str,
  range: bool,
) -> v8::Local<'s, v8::Value> {
  let Some(text) = v8::String::new(scope, message) else {
    return v8::undefined(scope).into();
  };
  if range {
    v8::Exception::range_error(scope, text).into()
  } else {
    v8::Exception::type_error(scope, text).into()
  }
}
