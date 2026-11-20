//! Structured clone for `ImageBitmap` and `ImageData`.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::LazyLock;
use std::sync::Mutex;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;

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
  yuzuko_core::add_resource_frame_hooks(begin_seen, end_seen);
  yuzuko_core::add_host_clone(write_bitmap, read_bitmap);
  yuzuko_core::add_host_clone(write_image_data, read_image_data);
  yuzuko_core::add_host_transfer(probe_bitmap);
  yuzuko_core::add_host_detach(detach_bitmap);
}

/// Pixel allocations parked for a transfer.
///
/// The source keeps its `Arc` until detach, so a failed clone still shows
/// the bitmap. The reader clones that `Arc`. The id stays until the clone
/// frame or the cross-agent parcel drops it.
static BITMAP_MOVES: LazyLock<Mutex<HashMap<u64, Arc<Raster>>>> =
  LazyLock::new(|| Mutex::new(HashMap::new()));
static NEXT_BITMAP: AtomicU64 = AtomicU64::new(1);

thread_local! {
  static SEEN: RefCell<Vec<HashMap<usize, u64>>> = const { RefCell::new(Vec::new()) };
}

fn begin_seen() {
  SEEN.with(|slot| slot.borrow_mut().push(HashMap::new()));
}

fn end_seen() {
  SEEN.with(|slot| {
    slot.borrow_mut().pop();
  });
}

fn park_bitmap(key: usize, raster: Arc<Raster>) -> u64 {
  if let Some(id) = seen_id(key) {
    return id;
  }
  let id = NEXT_BITMAP.fetch_add(1, Ordering::Relaxed);
  BITMAP_MOVES
    .lock()
    .expect("bitmap move")
    .insert(id, Arc::clone(&raster));
  let release_id = id;
  yuzuko_core::note_resource_release(Box::new(move || {
    BITMAP_MOVES
      .lock()
      .expect("bitmap move")
      .remove(&release_id);
  }));
  remember(key, id);
  id
}

fn recall_bitmap(id: u64) -> Option<Arc<Raster>> {
  BITMAP_MOVES.lock().expect("bitmap move").get(&id).cloned()
}

fn seen_id(key: usize) -> Option<u64> {
  SEEN.with(|slot| {
    slot
      .borrow()
      .last()
      .and_then(|seen| seen.get(&key).copied())
  })
}

fn remember(key: usize, id: u64) {
  SEEN.with(|slot| {
    if let Some(seen) = slot.borrow_mut().last_mut() {
      seen.insert(key, id);
    }
  });
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
  helper.write_uint32(TAG_BITMAP);
  if yuzuko_core::is_structured_transfer(scope, object) {
    let Some(raster) = bitmap.share_raster() else {
      yuzuko_core::reject_clone(scope);
      return yuzuko_core::WriteResult::Failed;
    };
    let id = park_bitmap(std::ptr::from_ref(bitmap) as usize, raster);
    helper.write_uint32(1);
    helper.write_uint64(id);
    return yuzuko_core::WriteResult::Wrote;
  }
  let Some(raster) = bitmap.copy_raster() else {
    yuzuko_core::reject_clone(scope);
    return yuzuko_core::WriteResult::Failed;
  };
  helper.write_uint32(0);
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
  let mut mode = 0u32;
  if !helper.read_uint32(&mut mode) {
    yuzuko_core::reject_clone(scope);
    return yuzuko_core::ReadResult::Failed;
  }
  let bitmap = if mode == 1 {
    let mut id = 0u64;
    if !helper.read_uint64(&mut id) {
      yuzuko_core::reject_clone(scope);
      return yuzuko_core::ReadResult::Failed;
    }
    let Some(raster) = recall_bitmap(id) else {
      yuzuko_core::reject_clone(scope);
      return yuzuko_core::ReadResult::Failed;
    };
    ImageBitmap::from_shared(raster)
  } else if mode == 0 {
    let Some(raster) = read_raster(helper) else {
      yuzuko_core::reject_clone(scope);
      return yuzuko_core::ReadResult::Failed;
    };
    ImageBitmap::from_raster(raster)
  } else {
    yuzuko_core::reject_clone(scope);
    return yuzuko_core::ReadResult::Failed;
  };
  let _ = ImageBitmap::template(scope);
  let wrapper = unsafe { wrap_instance(scope, bitmap) };
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

#[cfg(test)]
mod tests {
  use super::begin_seen;
  use super::end_seen;
  use super::park_bitmap;
  use super::recall_bitmap;
  use crate::bitmap::ImageBitmap;
  use crate::raster::ColorSpace;
  use crate::raster::Raster;
  use crate::raster::SampleFormat;
  use std::sync::Arc;

  fn red_bitmap() -> (ImageBitmap, *const u8) {
    let mut raster =
      Raster::new(1, 1, ColorSpace::Srgb, SampleFormat::Unorm8).unwrap();
    raster.bytes[0] = 9;
    let ptr = raster.bytes.as_ptr();
    (ImageBitmap::from_raster(raster), ptr)
  }

  #[test]
  fn transferred_bitmap_shares_pixels() {
    let (bitmap, ptr) = red_bitmap();
    let shared = bitmap.share_raster().unwrap();
    assert_eq!(shared.bytes.as_ptr(), ptr);
    let copied = bitmap.copy_raster().unwrap();
    assert_ne!(copied.bytes.as_ptr(), ptr);
    assert_eq!(copied.bytes[0], 9);

    begin_seen();
    yuzuko_core::begin_resource_frame();
    let key = std::ptr::from_ref(&bitmap) as usize;
    let id = park_bitmap(key, Arc::clone(&shared));
    assert_eq!(park_bitmap(key, Arc::clone(&shared)), id);
    let taken = recall_bitmap(id).unwrap();
    assert!(Arc::ptr_eq(&shared, &taken));
    assert_eq!(taken.bytes.as_ptr(), ptr);
    assert_eq!(Arc::strong_count(&taken), 4);
    drop(shared);
    bitmap.neuter();
    assert!(bitmap.share_raster().is_none());
    yuzuko_core::end_resource_frame(true);
    end_seen();
    assert_eq!(Arc::strong_count(&taken), 1);
    assert_eq!(taken.bytes.as_ptr(), ptr);
    assert_eq!(taken.bytes[0], 9);
  }

  #[test]
  fn abandoned_bitmap_transfer_keeps_pixels() {
    let (bitmap, ptr) = red_bitmap();
    let shared = bitmap.share_raster().unwrap();
    yuzuko_core::begin_resource_frame();
    let id =
      park_bitmap(std::ptr::from_ref(&bitmap) as usize, Arc::clone(&shared));
    yuzuko_core::end_resource_frame(true);
    assert!(recall_bitmap(id).is_none());
    assert!(!bitmap.is_detached());
    assert_eq!(Arc::strong_count(&shared), 2);
    let still = bitmap.share_raster().unwrap();
    assert!(Arc::ptr_eq(&shared, &still));
    assert_eq!(still.bytes.as_ptr(), ptr);
    assert_eq!(still.bytes[0], 9);
  }
}
