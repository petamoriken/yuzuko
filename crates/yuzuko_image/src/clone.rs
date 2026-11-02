//! Structured clone for `ImageBitmap` and `ImageData`.

use yuzuko_core::op::host_ref;
use yuzuko_core::op::wrap_instance;

use crate::bitmap::ImageBitmap;
use crate::data::ImageData;
use crate::raster::ColorSpace;
use crate::raster::Raster;
use crate::raster::SampleFormat;
use crate::raster::within_limits;

const TAG_BITMAP: u32 = 14;
const TAG_IMAGE_DATA: u32 = 15;

pub(crate) fn register() {
  yuzuko_core::add_host_clone(write_bitmap, read_bitmap);
  yuzuko_core::add_host_clone(write_image_data, read_image_data);
  yuzuko_core::add_host_transfer(probe_bitmap);
  yuzuko_core::add_host_detach(detach_bitmap);
}

pub fn write_raster(helper: &dyn v8::ValueSerializerHelper, raster: &Raster) {
  helper.write_uint32(raster.width);
  helper.write_uint32(raster.height);
  helper.write_uint32(color_code(raster.color_space));
  helper.write_uint32(format_code(raster.format));
  helper.write_uint32(u32::from(raster.premultiplied));
  yuzuko_core::write_bytes(helper, &raster.bytes);
}

pub fn read_raster(helper: &dyn v8::ValueDeserializerHelper) -> Option<Raster> {
  let mut width = 0u32;
  let mut height = 0u32;
  let mut color = 0u32;
  let mut format = 0u32;
  let mut premultiplied = 0u32;
  if !helper.read_uint32(&mut width)
    || !helper.read_uint32(&mut height)
    || !helper.read_uint32(&mut color)
    || !helper.read_uint32(&mut format)
    || !helper.read_uint32(&mut premultiplied)
  {
    return None;
  }
  let color_space = color_space(color)?;
  let sample = sample_format(format)?;
  if premultiplied > 1 || !within_limits(width, height) {
    return None;
  }
  let expected = byte_length(width, height, sample)?;
  let mut len = 0u64;
  if !helper.read_uint64(&mut len) || len != expected {
    return None;
  }
  let bytes = if expected == 0 {
    Vec::new()
  } else {
    helper.read_raw_bytes(expected as usize)?.to_vec()
  };
  Some(Raster {
    width,
    height,
    color_space,
    format: sample,
    premultiplied: premultiplied == 1,
    bytes,
  })
}

fn write_bitmap(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  helper: &dyn v8::ValueSerializerHelper,
) -> yuzuko_core::WriteResult {
  let Some(bitmap) = (unsafe { host_ref::<ImageBitmap>(scope, object) }) else {
    return yuzuko_core::WriteResult::Skip;
  };
  let bitmap = bitmap.get();
  if bitmap.is_detached() {
    yuzuko_core::reject_clone(scope);
    return yuzuko_core::WriteResult::Failed;
  }
  let Some(raster) = bitmap.copy_raster() else {
    yuzuko_core::reject_clone(scope);
    return yuzuko_core::WriteResult::Failed;
  };
  helper.write_uint32(TAG_BITMAP);
  write_raster(helper, &raster);
  yuzuko_core::WriteResult::Wrote
}

fn read_bitmap(
  scope: &mut v8::PinScope,
  tag: u32,
  helper: &dyn v8::ValueDeserializerHelper,
) -> yuzuko_core::ReadResult {
  if tag != TAG_BITMAP {
    return yuzuko_core::ReadResult::Skip;
  }
  let Some(raster) = read_raster(helper) else {
    yuzuko_core::reject_clone(scope);
    return yuzuko_core::ReadResult::Failed;
  };
  let _ = ImageBitmap::template(scope);
  let wrapper =
    unsafe { wrap_instance(scope, ImageBitmap::from_raster(raster)) };
  yuzuko_core::ReadResult::Done(v8::Global::new(scope, wrapper))
}

fn probe_bitmap(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
) -> yuzuko_core::TransferStatus {
  let Some(bitmap) = (unsafe { host_ref::<ImageBitmap>(scope, object) }) else {
    return yuzuko_core::TransferStatus::Skip;
  };
  if bitmap.get().is_detached() {
    yuzuko_core::TransferStatus::Detached
  } else {
    yuzuko_core::TransferStatus::Ready
  }
}

fn detach_bitmap(scope: &mut v8::PinScope, object: v8::Local<v8::Object>) {
  let Some(bitmap) = (unsafe { host_ref::<ImageBitmap>(scope, object) }) else {
    return;
  };
  bitmap.get().neuter();
}

fn write_image_data(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  helper: &dyn v8::ValueSerializerHelper,
) -> yuzuko_core::WriteResult {
  let Some(image) = (unsafe { host_ref::<ImageData>(scope, object) }) else {
    return yuzuko_core::WriteResult::Skip;
  };
  let image = image.get();
  if yuzuko_core::is_structured_transfer(scope, object)
    || image.buffer_is_detached(scope)
  {
    yuzuko_core::reject_clone(scope);
    return yuzuko_core::WriteResult::Failed;
  }
  let (width, height, color, format) = image.parts();
  let Some(data) = image.data_local(scope) else {
    yuzuko_core::reject_clone(scope);
    return yuzuko_core::WriteResult::Failed;
  };
  helper.write_uint32(TAG_IMAGE_DATA);
  if write_nested(scope, helper, data.into()).is_err() {
    return yuzuko_core::WriteResult::Failed;
  }
  helper.write_uint32(width);
  helper.write_uint32(height);
  helper.write_uint32(color_code(color));
  helper.write_uint32(format_code(format));
  yuzuko_core::WriteResult::Wrote
}

fn read_image_data(
  scope: &mut v8::PinScope,
  tag: u32,
  helper: &dyn v8::ValueDeserializerHelper,
) -> yuzuko_core::ReadResult {
  if tag != TAG_IMAGE_DATA {
    return yuzuko_core::ReadResult::Skip;
  }
  let Ok(value) = read_nested(scope, helper) else {
    return yuzuko_core::ReadResult::Failed;
  };
  let Ok(data) = v8::Local::<v8::Object>::try_from(value) else {
    yuzuko_core::reject_clone(scope);
    return yuzuko_core::ReadResult::Failed;
  };
  let mut width = 0u32;
  let mut height = 0u32;
  let mut color = 0u32;
  let mut format = 0u32;
  if !helper.read_uint32(&mut width)
    || !helper.read_uint32(&mut height)
    || !helper.read_uint32(&mut color)
    || !helper.read_uint32(&mut format)
  {
    yuzuko_core::reject_clone(scope);
    return yuzuko_core::ReadResult::Failed;
  }
  let Some(color_space) = color_space(color) else {
    yuzuko_core::reject_clone(scope);
    return yuzuko_core::ReadResult::Failed;
  };
  let Some(sample) = sample_format(format) else {
    yuzuko_core::reject_clone(scope);
    return yuzuko_core::ReadResult::Failed;
  };
  let Ok(image) =
    ImageData::holding(scope, width, height, color_space, sample, data)
  else {
    yuzuko_core::reject_clone(scope);
    return yuzuko_core::ReadResult::Failed;
  };
  let _ = ImageData::template(scope);
  let wrapper = unsafe { wrap_instance(scope, image) };
  yuzuko_core::ReadResult::Done(v8::Global::new(scope, wrapper))
}

fn write_nested(
  scope: &mut v8::PinScope,
  helper: &dyn v8::ValueSerializerHelper,
  value: v8::Local<v8::Value>,
) -> Result<(), ()> {
  let value = v8::Global::new(scope, value);
  let thrown = {
    v8::tc_scope!(let tc, scope);
    let value = v8::Local::new(tc, value);
    let context = tc.get_current_context();
    if helper.write_value(context, value) == Some(true) {
      None
    } else {
      Some(tc.exception().map(|error| v8::Global::new(tc, error)))
    }
  };
  match thrown {
    None => Ok(()),
    Some(error) => {
      rethrow_or_reject(scope, error);
      Err(())
    }
  }
}

fn read_nested<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  helper: &dyn v8::ValueDeserializerHelper,
) -> Result<v8::Local<'s, v8::Value>, ()> {
  let read = {
    v8::tc_scope!(let tc, scope);
    let context = tc.get_current_context();
    if let Some(value) = helper.read_value(context) {
      Ok(v8::Global::new(tc, value))
    } else {
      Err(tc.exception().map(|error| v8::Global::new(tc, error)))
    }
  };
  match read {
    Ok(value) => Ok(v8::Local::new(scope, value)),
    Err(error) => {
      rethrow_or_reject(scope, error);
      Err(())
    }
  }
}

fn rethrow_or_reject(
  scope: &mut v8::PinScope,
  error: Option<v8::Global<v8::Value>>,
) {
  match error {
    Some(error) => {
      scope.throw_exception(v8::Local::new(scope, error));
    }
    None => yuzuko_core::reject_clone(scope),
  }
}

fn color_code(color: ColorSpace) -> u32 {
  match color {
    ColorSpace::Srgb => 0,
    ColorSpace::DisplayP3 => 1,
  }
}

fn format_code(format: SampleFormat) -> u32 {
  match format {
    SampleFormat::Unorm8 => 0,
    SampleFormat::Float16 => 1,
  }
}

fn color_space(code: u32) -> Option<ColorSpace> {
  match code {
    0 => Some(ColorSpace::Srgb),
    1 => Some(ColorSpace::DisplayP3),
    _ => None,
  }
}

fn sample_format(code: u32) -> Option<SampleFormat> {
  match code {
    0 => Some(SampleFormat::Unorm8),
    1 => Some(SampleFormat::Float16),
    _ => None,
  }
}

fn byte_length(width: u32, height: u32, format: SampleFormat) -> Option<u64> {
  let pixels = u64::from(width) * u64::from(height);
  let bytes = pixels
    * match format {
      SampleFormat::Unorm8 => 4,
      SampleFormat::Float16 => 8,
    };
  Some(bytes)
}
