//! Structured clone for WebCodecs frames, samples, and encoded chunks.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::LazyLock;
use std::sync::Mutex;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;

use yuzuko_core::ReadResult;
use yuzuko_core::TransferStatus;
use yuzuko_core::WriteResult;
use yuzuko_core::is_structured_transfer;
use yuzuko_core::op::host_ref;
use yuzuko_core::op::wrap_instance;
use yuzuko_core::reject_clone;
use yuzuko_core::write_bytes;

use crate::classes::AudioData;
use crate::classes::EncodedAudioChunk;
use crate::classes::EncodedVideoChunk;
use crate::classes::VideoFrame;
use crate::layout::FrameColor;
use crate::layout::FrameData;
use crate::layout::PixelFormat;
use crate::samples::AudioSamples;
use crate::samples::MAX_BYTES;
use crate::samples::SharedAudio;
use crate::samples::kind_from_code;
use crate::store::ByteStore;

const TAG_AUDIO: u32 = 16;
const TAG_FRAME: u32 = 17;
const TAG_AUDIO_CHUNK: u32 = 18;
const TAG_VIDEO_CHUNK: u32 = 19;

/// An encoded chunk stays within 64 MiB.
const MAX_CHUNK_BYTES: usize = 64 * 1024 * 1024;
/// One plane stays within the 8-bit raster limit (100 million pixels, 4 bytes).
const MAX_PLANE_BYTES: usize = 400_000_000;

pub(crate) fn register() {
  yuzuko_core::add_resource_frame_hooks(begin_seen, end_seen);
  yuzuko_core::add_host_clone(write_audio, read_audio);
  yuzuko_core::add_host_clone(write_frame, read_frame);
  yuzuko_core::add_host_clone(write_audio_chunk, read_audio_chunk);
  yuzuko_core::add_host_clone(write_video_chunk, read_video_chunk);
  yuzuko_core::add_host_transfer(probe_audio);
  yuzuko_core::add_host_transfer(probe_frame);
  yuzuko_core::add_host_detach(detach_audio);
  yuzuko_core::add_host_detach(detach_frame);
}

/// Sample allocations parked for a transfer. See the bitmap map.
static AUDIO_MOVES: LazyLock<Mutex<HashMap<u64, SharedAudio>>> =
  LazyLock::new(|| Mutex::new(HashMap::new()));
static NEXT_AUDIO: AtomicU64 = AtomicU64::new(1);
/// Frame allocations parked for a transfer. The `Arc` is the planes and the
/// display raster. The source drops its handle only after the clone succeeds.
static FRAME_MOVES: LazyLock<Mutex<HashMap<u64, Arc<FrameData>>>> =
  LazyLock::new(|| Mutex::new(HashMap::new()));
static NEXT_FRAME: AtomicU64 = AtomicU64::new(1);

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

fn park_audio(key: usize, audio: SharedAudio) -> u64 {
  if let Some(id) = seen_id(key) {
    return id;
  }
  let id = NEXT_AUDIO.fetch_add(1, Ordering::Relaxed);
  AUDIO_MOVES.lock().expect("audio move").insert(id, audio);
  let release_id = id;
  yuzuko_core::note_resource_release(Box::new(move || {
    AUDIO_MOVES.lock().expect("audio move").remove(&release_id);
  }));
  remember(key, id);
  id
}

fn recall_audio(id: u64) -> Option<SharedAudio> {
  let map = AUDIO_MOVES.lock().expect("audio move");
  let audio = map.get(&id)?;
  Some(SharedAudio {
    kind: audio.kind,
    planar: audio.planar,
    channels: audio.channels,
    frames: audio.frames,
    rate: audio.rate,
    timestamp: audio.timestamp,
    bytes: audio.bytes.share_again(),
  })
}

fn park_frame(key: usize, data: Arc<FrameData>) -> u64 {
  if let Some(id) = seen_id(key) {
    return id;
  }
  let id = NEXT_FRAME.fetch_add(1, Ordering::Relaxed);
  FRAME_MOVES
    .lock()
    .expect("frame move")
    .insert(id, Arc::clone(&data));
  let release_id = id;
  yuzuko_core::note_resource_release(Box::new(move || {
    FRAME_MOVES.lock().expect("frame move").remove(&release_id);
  }));
  remember(key, id);
  id
}

fn recall_frame(id: u64) -> Option<Arc<FrameData>> {
  FRAME_MOVES.lock().expect("frame move").get(&id).cloned()
}

fn write_audio(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  helper: &dyn v8::ValueSerializerHelper,
) -> WriteResult {
  let Some(data) = (unsafe { host_ref::<AudioData>(scope, object) }) else {
    return WriteResult::Skip;
  };
  let audio = data.get();
  if is_structured_transfer(scope, object) {
    let Some(payload) = audio.share_payload() else {
      reject_clone(scope);
      return WriteResult::Failed;
    };
    if payload.channels > crate::samples::MAX_CHANNELS
      || payload.frames > crate::samples::MAX_FRAMES
    {
      reject_clone(scope);
      return WriteResult::Failed;
    }
    let id = park_audio(std::ptr::from_ref(audio) as usize, payload);
    helper.write_uint32(TAG_AUDIO);
    helper.write_uint32(1);
    helper.write_uint64(id);
    return WriteResult::Wrote;
  }
  let Some(samples) = audio.samples() else {
    reject_clone(scope);
    return WriteResult::Failed;
  };
  if samples.channels > crate::samples::MAX_CHANNELS
    || samples.frames > crate::samples::MAX_FRAMES
  {
    reject_clone(scope);
    return WriteResult::Failed;
  }
  helper.write_uint32(TAG_AUDIO);
  helper.write_uint32(0);
  helper.write_double(samples.rate);
  helper.write_double(audio.timestamp_us());
  helper.write_uint32(samples.channels);
  helper.write_uint32(samples.frames);
  helper.write_uint32(samples.code());
  write_bytes(helper, &samples.bytes);
  WriteResult::Wrote
}

fn read_audio(
  scope: &mut v8::PinScope,
  tag: u32,
  helper: &dyn v8::ValueDeserializerHelper,
) -> ReadResult {
  if tag != TAG_AUDIO {
    return ReadResult::Skip;
  }
  let mut mode = 0u32;
  if !helper.read_uint32(&mut mode) {
    reject_clone(scope);
    return ReadResult::Failed;
  }
  let built = if mode == 1 {
    let mut id = 0u64;
    if !helper.read_uint64(&mut id) {
      reject_clone(scope);
      return ReadResult::Failed;
    }
    let Some(payload) = recall_audio(id) else {
      reject_clone(scope);
      return ReadResult::Failed;
    };
    let Some(bytes) = ByteStore::from_share(payload.bytes) else {
      reject_clone(scope);
      return ReadResult::Failed;
    };
    let Ok(samples) = AudioSamples::from_store(
      payload.kind,
      payload.planar,
      payload.channels,
      payload.frames,
      payload.rate,
      bytes,
    ) else {
      reject_clone(scope);
      return ReadResult::Failed;
    };
    (samples, payload.timestamp)
  } else if mode == 0 {
    let Some(parts) = read_audio_parts(helper) else {
      reject_clone(scope);
      return ReadResult::Failed;
    };
    parts
  } else {
    reject_clone(scope);
    return ReadResult::Failed;
  };
  let _ = AudioData::template(scope);
  let wrapper =
    unsafe { wrap_instance(scope, AudioData::from_samples(built.0, built.1)) };
  ReadResult::Done(v8::Global::new(scope, wrapper))
}

fn probe_audio(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
) -> TransferStatus {
  let Some(data) = (unsafe { host_ref::<AudioData>(scope, object) }) else {
    return TransferStatus::Skip;
  };
  if data.get().is_closed() {
    TransferStatus::Detached
  } else {
    TransferStatus::Ready
  }
}

fn detach_audio(scope: &mut v8::PinScope, object: v8::Local<v8::Object>) {
  let Some(data) = (unsafe { host_ref::<AudioData>(scope, object) }) else {
    return;
  };
  data.get().neuter();
}

fn write_frame(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  helper: &dyn v8::ValueSerializerHelper,
) -> WriteResult {
  let Some(frame) = (unsafe { host_ref::<VideoFrame>(scope, object) }) else {
    return WriteResult::Skip;
  };
  let frame = frame.get();
  if is_structured_transfer(scope, object) {
    let Some(data) = frame.share() else {
      reject_clone(scope);
      return WriteResult::Failed;
    };
    if !planes_fit(&data) {
      reject_clone(scope);
      return WriteResult::Failed;
    }
    let id = park_frame(std::ptr::from_ref(frame) as usize, data);
    helper.write_uint32(TAG_FRAME);
    helper.write_uint32(1);
    write_timing(helper, frame.timestamp_us(), frame.duration_us());
    helper.write_uint64(id);
    return WriteResult::Wrote;
  }
  let Some(data) = frame.snapshot() else {
    reject_clone(scope);
    return WriteResult::Failed;
  };
  if !planes_fit(&data) {
    reject_clone(scope);
    return WriteResult::Failed;
  }
  helper.write_uint32(TAG_FRAME);
  helper.write_uint32(0);
  write_timing(helper, frame.timestamp_us(), frame.duration_us());
  if !write_frame_body(helper, &data) {
    reject_clone(scope);
    return WriteResult::Failed;
  }
  WriteResult::Wrote
}

fn planes_fit(data: &FrameData) -> bool {
  data.planes.len() == data.format.plane_count()
    && data
      .planes
      .iter()
      .all(|plane| plane.len() <= MAX_PLANE_BYTES)
}

fn write_timing(
  helper: &dyn v8::ValueSerializerHelper,
  timestamp: f64,
  duration: Option<f64>,
) {
  helper.write_double(timestamp);
  match duration {
    Some(duration) => {
      helper.write_uint32(1);
      helper.write_double(duration);
    }
    None => helper.write_uint32(0),
  }
}

fn read_timing(
  helper: &dyn v8::ValueDeserializerHelper,
) -> Option<(f64, Option<f64>)> {
  let mut timestamp = 0.0;
  let mut duration_flag = 0u32;
  if !helper.read_double(&mut timestamp)
    || !helper.read_uint32(&mut duration_flag)
  {
    return None;
  }
  let duration = match duration_flag {
    0 => None,
    1 => {
      let mut duration = 0.0;
      if !helper.read_double(&mut duration) {
        return None;
      }
      Some(duration)
    }
    _ => return None,
  };
  Some((timestamp, duration))
}

fn read_frame(
  scope: &mut v8::PinScope,
  tag: u32,
  helper: &dyn v8::ValueDeserializerHelper,
) -> ReadResult {
  if tag != TAG_FRAME {
    return ReadResult::Skip;
  }
  let mut mode = 0u32;
  if !helper.read_uint32(&mut mode) {
    reject_clone(scope);
    return ReadResult::Failed;
  }
  let frame = if mode == 1 {
    let Some((timestamp, duration)) = read_timing(helper) else {
      reject_clone(scope);
      return ReadResult::Failed;
    };
    let mut id = 0u64;
    if !helper.read_uint64(&mut id) {
      reject_clone(scope);
      return ReadResult::Failed;
    }
    let Some(data) = recall_frame(id) else {
      reject_clone(scope);
      return ReadResult::Failed;
    };
    VideoFrame::from_shared(data, timestamp, duration)
  } else if mode == 0 {
    let Some((data, timestamp, duration)) = read_frame_body(helper) else {
      reject_clone(scope);
      return ReadResult::Failed;
    };
    VideoFrame::from_data(data, timestamp, duration)
  } else {
    reject_clone(scope);
    return ReadResult::Failed;
  };
  let _ = VideoFrame::template(scope);
  let wrapper = unsafe { wrap_instance(scope, frame) };
  ReadResult::Done(v8::Global::new(scope, wrapper))
}

fn write_frame_body(
  helper: &dyn v8::ValueSerializerHelper,
  data: &FrameData,
) -> bool {
  helper.write_uint32(data.format.code());
  helper.write_uint32(data.coded_width);
  helper.write_uint32(data.coded_height);
  helper.write_double(data.visible_x);
  helper.write_double(data.visible_y);
  helper.write_double(data.visible_width);
  helper.write_double(data.visible_height);
  helper.write_uint32(data.display_width);
  helper.write_uint32(data.display_height);
  helper.write_uint32(u32::from(data.rotation));
  helper.write_uint32(u32::from(data.flip));
  if !write_optional_text(helper, &data.color.primaries)
    || !write_optional_text(helper, &data.color.transfer)
    || !write_optional_text(helper, &data.color.matrix)
  {
    return false;
  }
  helper.write_uint32(range_code(data.color.full_range));
  helper.write_uint32(data.planes.len() as u32);
  for plane in &data.planes {
    write_bytes(helper, plane);
  }
  yuzuko_image::write_raster(helper, &data.raster);
  true
}

fn read_frame_body(
  helper: &dyn v8::ValueDeserializerHelper,
) -> Option<(FrameData, f64, Option<f64>)> {
  let (timestamp, duration) = read_timing(helper)?;
  let mut format_code = 0u32;
  let mut coded_width = 0u32;
  let mut coded_height = 0u32;
  let mut visible_x = 0.0;
  let mut visible_y = 0.0;
  let mut visible_width = 0.0;
  let mut visible_height = 0.0;
  let mut display_width = 0u32;
  let mut display_height = 0u32;
  let mut rotation = 0u32;
  let mut flip = 0u32;
  if !helper.read_uint32(&mut format_code)
    || !helper.read_uint32(&mut coded_width)
    || !helper.read_uint32(&mut coded_height)
    || !helper.read_double(&mut visible_x)
    || !helper.read_double(&mut visible_y)
    || !helper.read_double(&mut visible_width)
    || !helper.read_double(&mut visible_height)
    || !helper.read_uint32(&mut display_width)
    || !helper.read_uint32(&mut display_height)
    || !helper.read_uint32(&mut rotation)
    || !helper.read_uint32(&mut flip)
  {
    return None;
  }
  if !matches!(rotation, 0 | 90 | 180 | 270) || flip > 1 {
    return None;
  }
  let format = PixelFormat::from_code(format_code)?;
  let primaries = read_optional_text(helper)?;
  let transfer = read_optional_text(helper)?;
  let matrix = read_optional_text(helper)?;
  let mut range = 0u32;
  let mut plane_count = 0u32;
  if !helper.read_uint32(&mut range) || !helper.read_uint32(&mut plane_count) {
    return None;
  }
  if plane_count as usize != format.plane_count() || plane_count > 4 {
    return None;
  }
  let mut planes = Vec::with_capacity(plane_count as usize);
  for _ in 0..plane_count {
    planes.push(read_capped(helper, MAX_PLANE_BYTES)?);
  }
  let raster = yuzuko_image::read_raster(helper)?;
  Some((
    FrameData {
      format,
      coded_width,
      coded_height,
      visible_x,
      visible_y,
      visible_width,
      visible_height,
      display_width,
      display_height,
      rotation: rotation as u16,
      flip: flip == 1,
      color: FrameColor {
        primaries,
        transfer,
        matrix,
        full_range: range_value(range)?,
      },
      planes,
      raster,
    },
    timestamp,
    duration,
  ))
}

fn write_optional_text(
  helper: &dyn v8::ValueSerializerHelper,
  text: &Option<String>,
) -> bool {
  match text {
    None => {
      helper.write_uint32(0);
      true
    }
    Some(text) => {
      helper.write_uint32(1);
      yuzuko_core::write_text(helper, text)
    }
  }
}

fn read_optional_text(
  helper: &dyn v8::ValueDeserializerHelper,
) -> Option<Option<String>> {
  let mut flag = 0u32;
  if !helper.read_uint32(&mut flag) {
    return None;
  }
  match flag {
    0 => Some(None),
    1 => Some(Some(yuzuko_core::read_text(helper)?)),
    _ => None,
  }
}

fn range_code(value: Option<bool>) -> u32 {
  match value {
    None => 0,
    Some(false) => 1,
    Some(true) => 2,
  }
}

fn range_value(code: u32) -> Option<Option<bool>> {
  match code {
    0 => Some(None),
    1 => Some(Some(false)),
    2 => Some(Some(true)),
    _ => None,
  }
}

fn read_capped(
  helper: &dyn v8::ValueDeserializerHelper,
  max: usize,
) -> Option<Vec<u8>> {
  let mut len = 0u64;
  if !helper.read_uint64(&mut len) {
    return None;
  }
  let len = usize::try_from(len).ok()?;
  if len > max {
    return None;
  }
  if len == 0 {
    return Some(Vec::new());
  }
  Some(helper.read_raw_bytes(len)?.to_vec())
}

fn probe_frame(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
) -> TransferStatus {
  let Some(frame) = (unsafe { host_ref::<VideoFrame>(scope, object) }) else {
    return TransferStatus::Skip;
  };
  if frame.get().is_closed() {
    TransferStatus::Detached
  } else {
    TransferStatus::Ready
  }
}

fn detach_frame(scope: &mut v8::PinScope, object: v8::Local<v8::Object>) {
  let Some(frame) = (unsafe { host_ref::<VideoFrame>(scope, object) }) else {
    return;
  };
  frame.get().neuter();
}

fn write_audio_chunk(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  helper: &dyn v8::ValueSerializerHelper,
) -> WriteResult {
  let Some(chunk) = (unsafe { host_ref::<EncodedAudioChunk>(scope, object) })
  else {
    return WriteResult::Skip;
  };
  write_chunk(
    scope,
    object,
    helper,
    TAG_AUDIO_CHUNK,
    chunk.get().copy_parts(),
  )
}

fn read_audio_chunk(
  scope: &mut v8::PinScope,
  tag: u32,
  helper: &dyn v8::ValueDeserializerHelper,
) -> ReadResult {
  if tag != TAG_AUDIO_CHUNK {
    return ReadResult::Skip;
  }
  let Some((key, timestamp, duration, data)) = read_chunk(helper) else {
    reject_clone(scope);
    return ReadResult::Failed;
  };
  let _ = EncodedAudioChunk::template(scope);
  let wrapper = unsafe {
    wrap_instance(
      scope,
      EncodedAudioChunk::from_parts(key, timestamp, duration, data),
    )
  };
  ReadResult::Done(v8::Global::new(scope, wrapper))
}

fn write_video_chunk(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  helper: &dyn v8::ValueSerializerHelper,
) -> WriteResult {
  let Some(chunk) = (unsafe { host_ref::<EncodedVideoChunk>(scope, object) })
  else {
    return WriteResult::Skip;
  };
  write_chunk(
    scope,
    object,
    helper,
    TAG_VIDEO_CHUNK,
    chunk.get().copy_parts(),
  )
}

fn read_video_chunk(
  scope: &mut v8::PinScope,
  tag: u32,
  helper: &dyn v8::ValueDeserializerHelper,
) -> ReadResult {
  if tag != TAG_VIDEO_CHUNK {
    return ReadResult::Skip;
  }
  let Some((key, timestamp, duration, data)) = read_chunk(helper) else {
    reject_clone(scope);
    return ReadResult::Failed;
  };
  let _ = EncodedVideoChunk::template(scope);
  let wrapper = unsafe {
    wrap_instance(
      scope,
      EncodedVideoChunk::from_parts(key, timestamp, duration, data),
    )
  };
  ReadResult::Done(v8::Global::new(scope, wrapper))
}

fn write_chunk(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  helper: &dyn v8::ValueSerializerHelper,
  tag: u32,
  parts: (bool, i64, Option<f64>, Vec<u8>),
) -> WriteResult {
  if is_structured_transfer(scope, object) || parts.3.len() > MAX_CHUNK_BYTES {
    reject_clone(scope);
    return WriteResult::Failed;
  }
  helper.write_uint32(tag);
  helper.write_uint32(u32::from(parts.0));
  helper.write_uint64(parts.1 as u64);
  match parts.2 {
    Some(duration) => {
      helper.write_uint32(1);
      helper.write_double(duration);
    }
    None => helper.write_uint32(0),
  }
  write_bytes(helper, &parts.3);
  WriteResult::Wrote
}

fn read_chunk(
  helper: &dyn v8::ValueDeserializerHelper,
) -> Option<(bool, i64, Option<f64>, Vec<u8>)> {
  let mut key = 0u32;
  let mut timestamp = 0u64;
  let mut present = 0u32;
  if !helper.read_uint32(&mut key)
    || key > 1
    || !helper.read_uint64(&mut timestamp)
    || !helper.read_uint32(&mut present)
    || present > 1
  {
    return None;
  }
  let duration = if present == 1 {
    let mut duration = 0.0;
    if !helper.read_double(&mut duration) {
      return None;
    }
    Some(duration)
  } else {
    None
  };
  let data = read_limited(helper, MAX_CHUNK_BYTES)?;
  Some((key == 1, timestamp as i64, duration, data))
}

fn read_audio_parts(
  helper: &dyn v8::ValueDeserializerHelper,
) -> Option<(crate::samples::AudioSamples, f64)> {
  let mut rate = 0.0;
  let mut timestamp = 0.0;
  let mut channels = 0u32;
  let mut frames = 0u32;
  let mut code = 0u32;
  if !helper.read_double(&mut rate)
    || !helper.read_double(&mut timestamp)
    || !helper.read_uint32(&mut channels)
    || !helper.read_uint32(&mut frames)
    || !helper.read_uint32(&mut code)
  {
    return None;
  }
  let (kind, planar) = kind_from_code(code)?;
  let bytes = read_limited(helper, MAX_BYTES)?;
  let samples = crate::samples::AudioSamples::from_parts(
    kind, planar, channels, frames, rate, bytes,
  )?;
  Some((samples, timestamp))
}

fn read_limited(
  helper: &dyn v8::ValueDeserializerHelper,
  max: usize,
) -> Option<Vec<u8>> {
  let mut len = 0u64;
  if !helper.read_uint64(&mut len) {
    return None;
  }
  let len = usize::try_from(len).ok()?;
  if len > max {
    return None;
  }
  if len == 0 {
    return Some(Vec::new());
  }
  Some(helper.read_raw_bytes(len)?.to_vec())
}

#[cfg(test)]
mod tests {
  use super::begin_seen;
  use super::end_seen;
  use super::park_audio;
  use super::park_frame;
  use super::recall_audio;
  use super::recall_frame;
  use crate::classes::AudioData;
  use crate::classes::VideoFrame;
  use crate::layout::frame_from_raster;
  use crate::samples::AudioSamples;
  use crate::samples::SampleKind;
  use crate::store::ByteShare;
  use std::sync::Arc;
  use yuzuko_image::ColorSpace;
  use yuzuko_image::Raster;
  use yuzuko_image::SampleFormat;

  fn owned(bytes: &ByteShare) -> &Arc<Vec<u8>> {
    match bytes {
      ByteShare::Owned(bytes) => bytes,
      ByteShare::Shared { .. } => panic!("expected owned samples"),
    }
  }

  fn red_frame() -> (VideoFrame, *const u8, *const u8) {
    let mut raster =
      Raster::new(1, 1, ColorSpace::Srgb, SampleFormat::Unorm8).unwrap();
    raster.bytes[0] = 9;
    let data = frame_from_raster(&raster);
    let plane = data.planes[0].as_ptr();
    let pixels = data.raster.bytes.as_ptr();
    (VideoFrame::from_data(data, 3.0, None), plane, pixels)
  }

  #[test]
  fn transferred_frame_shares_planes() {
    let (frame, plane, pixels) = red_frame();
    let shared = frame.share().unwrap();
    assert_eq!(shared.planes[0].as_ptr(), plane);
    assert_eq!(shared.raster.bytes.as_ptr(), pixels);
    let copied = frame.snapshot().unwrap();
    assert_ne!(copied.planes[0].as_ptr(), plane);
    assert_ne!(copied.raster.bytes.as_ptr(), pixels);
    assert_eq!(copied.planes[0][0], 9);
    assert_eq!(copied.raster.bytes[0], 9);

    begin_seen();
    yuzuko_core::begin_resource_frame();
    let key = std::ptr::from_ref(&frame) as usize;
    let id = park_frame(key, Arc::clone(&shared));
    assert_eq!(park_frame(key, Arc::clone(&shared)), id);
    let taken = recall_frame(id).unwrap();
    assert!(Arc::ptr_eq(&shared, &taken));
    assert_eq!(Arc::strong_count(&taken), 4);
    drop(shared);
    frame.neuter();
    assert!(frame.share().is_none());
    yuzuko_core::end_resource_frame(true);
    end_seen();
    assert_eq!(Arc::strong_count(&taken), 1);
    assert_eq!(taken.planes[0].as_ptr(), plane);
    assert_eq!(taken.raster.bytes.as_ptr(), pixels);
    assert_eq!(taken.planes[0][0], 9);
    assert_eq!(taken.raster.bytes[0], 9);
  }

  #[test]
  fn abandoned_frame_transfer_keeps_planes() {
    let (frame, plane, pixels) = red_frame();
    let shared = frame.share().unwrap();
    yuzuko_core::begin_resource_frame();
    let id =
      park_frame(std::ptr::from_ref(&frame) as usize, Arc::clone(&shared));
    yuzuko_core::end_resource_frame(true);
    assert!(recall_frame(id).is_none());
    assert!(!frame.is_closed());
    assert_eq!(Arc::strong_count(&shared), 2);
    let still = frame.share().unwrap();
    assert!(Arc::ptr_eq(&shared, &still));
    assert_eq!(still.planes[0].as_ptr(), plane);
    assert_eq!(still.raster.bytes.as_ptr(), pixels);
    assert_eq!(still.planes[0][0], 9);
    assert_eq!(still.raster.bytes[0], 9);
  }

  fn pcm(bytes: &[u8], timestamp: f64) -> (AudioData, *const u8) {
    let samples = AudioSamples::from_pcm(
      SampleKind::U8,
      false,
      1,
      u32::try_from(bytes.len()).unwrap(),
      48_000.0,
      bytes,
    )
    .unwrap();
    let ptr = samples.bytes.as_slice().as_ptr();
    (AudioData::from_samples(samples, timestamp), ptr)
  }

  #[test]
  fn transferred_audio_shares_samples() {
    let (data, ptr) = pcm(&[1, 2, 3, 4], 5.0);
    let copied = data.samples().unwrap();
    assert_ne!(copied.bytes.as_slice().as_ptr(), ptr);
    assert_eq!(&copied.bytes[..], &[1, 2, 3, 4]);

    begin_seen();
    yuzuko_core::begin_resource_frame();
    let key = std::ptr::from_ref(&data) as usize;
    let id = park_audio(key, data.share_payload().unwrap());
    assert_eq!(park_audio(key, data.share_payload().unwrap()), id);
    let taken = recall_audio(id).unwrap();
    assert_eq!(taken.bytes.as_ptr(), ptr);
    assert_eq!(taken.timestamp, 5.0);
    assert_eq!(Arc::strong_count(owned(&taken.bytes)), 3);
    data.neuter();
    assert!(data.share_payload().is_none());
    yuzuko_core::end_resource_frame(true);
    end_seen();
    assert_eq!(Arc::strong_count(owned(&taken.bytes)), 1);
    assert_eq!(owned(&taken.bytes).as_slice(), &[1, 2, 3, 4]);
  }

  #[test]
  fn abandoned_audio_transfer_keeps_samples() {
    let (data, ptr) = pcm(&[7, 8], 9.0);
    let kept = data.share_payload().unwrap();
    yuzuko_core::begin_resource_frame();
    let id = park_audio(
      std::ptr::from_ref(&data) as usize,
      data.share_payload().unwrap(),
    );
    yuzuko_core::end_resource_frame(true);
    assert!(recall_audio(id).is_none());
    assert!(!data.is_closed());
    assert_eq!(kept.bytes.as_ptr(), ptr);
    assert_eq!(kept.timestamp, 9.0);
    assert_eq!(owned(&kept.bytes).as_slice(), &[7, 8]);
    assert_eq!(Arc::strong_count(owned(&kept.bytes)), 2);
  }
}
