//! Structured clone for WebCodecs frames, samples, and encoded chunks.

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
use crate::samples::MAX_BYTES;
use crate::samples::kind_from_code;

const TAG_AUDIO: u32 = 16;
const TAG_FRAME: u32 = 17;
const TAG_AUDIO_CHUNK: u32 = 18;
const TAG_VIDEO_CHUNK: u32 = 19;

/// An encoded chunk stays within 64 MiB.
const MAX_CHUNK_BYTES: usize = 64 * 1024 * 1024;
/// One plane stays within the 8-bit raster limit (100 million pixels, 4 bytes).
const MAX_PLANE_BYTES: usize = 400_000_000;

pub(crate) fn register() {
  yuzuko_core::add_host_clone(write_audio, read_audio);
  yuzuko_core::add_host_clone(write_frame, read_frame);
  yuzuko_core::add_host_clone(write_audio_chunk, read_audio_chunk);
  yuzuko_core::add_host_clone(write_video_chunk, read_video_chunk);
  yuzuko_core::add_host_transfer(probe_audio);
  yuzuko_core::add_host_transfer(probe_frame);
  yuzuko_core::add_host_detach(detach_audio);
  yuzuko_core::add_host_detach(detach_frame);
}

fn write_audio(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  helper: &dyn v8::ValueSerializerHelper,
) -> WriteResult {
  let Some(data) = (unsafe { host_ref::<AudioData>(scope, object) }) else {
    return WriteResult::Skip;
  };
  let Some(samples) = data.get().samples() else {
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
  helper.write_double(samples.rate);
  helper.write_double(data.get().timestamp_us());
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
  let Some((samples, timestamp)) = read_audio_parts(helper) else {
    reject_clone(scope);
    return ReadResult::Failed;
  };
  let _ = AudioData::template(scope);
  let wrapper = unsafe {
    wrap_instance(scope, AudioData::from_samples(samples, timestamp))
  };
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
  let Some(data) = frame.get().snapshot() else {
    reject_clone(scope);
    return WriteResult::Failed;
  };
  if data.planes.len() != data.format.plane_count()
    || data
      .planes
      .iter()
      .any(|plane| plane.len() > MAX_PLANE_BYTES)
  {
    reject_clone(scope);
    return WriteResult::Failed;
  }
  helper.write_uint32(TAG_FRAME);
  helper.write_double(frame.get().timestamp_us());
  match frame.get().duration_us() {
    Some(duration) => {
      helper.write_uint32(1);
      helper.write_double(duration);
    }
    None => helper.write_uint32(0),
  }
  if !write_frame_body(helper, &data) {
    reject_clone(scope);
    return WriteResult::Failed;
  }
  WriteResult::Wrote
}

fn read_frame(
  scope: &mut v8::PinScope,
  tag: u32,
  helper: &dyn v8::ValueDeserializerHelper,
) -> ReadResult {
  if tag != TAG_FRAME {
    return ReadResult::Skip;
  }
  let Some((data, timestamp, duration)) = read_frame_body(helper) else {
    reject_clone(scope);
    return ReadResult::Failed;
  };
  let _ = VideoFrame::template(scope);
  let wrapper = unsafe {
    wrap_instance(scope, VideoFrame::from_data(data, timestamp, duration))
  };
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
  let mut format_code = 0u32;
  let mut coded_width = 0u32;
  let mut coded_height = 0u32;
  let mut visible_x = 0.0;
  let mut visible_y = 0.0;
  let mut visible_width = 0.0;
  let mut visible_height = 0.0;
  let mut display_width = 0u32;
  let mut display_height = 0u32;
  if !helper.read_uint32(&mut format_code)
    || !helper.read_uint32(&mut coded_width)
    || !helper.read_uint32(&mut coded_height)
    || !helper.read_double(&mut visible_x)
    || !helper.read_double(&mut visible_y)
    || !helper.read_double(&mut visible_width)
    || !helper.read_double(&mut visible_height)
    || !helper.read_uint32(&mut display_width)
    || !helper.read_uint32(&mut display_height)
  {
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
