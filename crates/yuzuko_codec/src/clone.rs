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

const TAG_AUDIO: u32 = 16;
const TAG_FRAME: u32 = 17;
const TAG_AUDIO_CHUNK: u32 = 18;
const TAG_VIDEO_CHUNK: u32 = 19;

/// WebCodecs allows at most 32 channels.
const MAX_CHANNELS: usize = 32;
/// One plane stays within 16,777,216 frames.
const MAX_FRAMES: usize = 16_777_216;
/// An encoded chunk stays within 64 MiB.
const MAX_CHUNK_BYTES: usize = 64 * 1024 * 1024;

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
  let Some((channels, rate, timestamp)) = data.get().stored() else {
    reject_clone(scope);
    return WriteResult::Failed;
  };
  if !audio_fits(&channels) {
    reject_clone(scope);
    return WriteResult::Failed;
  }
  helper.write_uint32(TAG_AUDIO);
  helper.write_double(rate);
  helper.write_double(timestamp);
  helper.write_uint32(channels.len() as u32);
  for channel in &channels {
    write_bytes(helper, &f32_bytes(channel));
  }
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
  let Some((channels, rate, timestamp)) = read_audio_parts(helper) else {
    reject_clone(scope);
    return ReadResult::Failed;
  };
  let _ = AudioData::template(scope);
  let wrapper = unsafe {
    wrap_instance(scope, AudioData::from_stored(channels, rate, timestamp))
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
  let Some((raster, timestamp)) = frame.get().stored() else {
    reject_clone(scope);
    return WriteResult::Failed;
  };
  helper.write_uint32(TAG_FRAME);
  helper.write_double(timestamp);
  yuzuko_image::write_raster(helper, &raster);
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
  let mut timestamp = 0.0;
  if !helper.read_double(&mut timestamp) {
    reject_clone(scope);
    return ReadResult::Failed;
  }
  let Some(raster) = yuzuko_image::read_raster(helper) else {
    reject_clone(scope);
    return ReadResult::Failed;
  };
  let _ = VideoFrame::template(scope);
  let wrapper =
    unsafe { wrap_instance(scope, VideoFrame::from_stored(raster, timestamp)) };
  ReadResult::Done(v8::Global::new(scope, wrapper))
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
  let Some((key, timestamp, data)) = read_chunk(helper) else {
    reject_clone(scope);
    return ReadResult::Failed;
  };
  let _ = EncodedAudioChunk::template(scope);
  let wrapper = unsafe {
    wrap_instance(scope, EncodedAudioChunk::from_parts(key, timestamp, data))
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
  let Some((key, timestamp, data)) = read_chunk(helper) else {
    reject_clone(scope);
    return ReadResult::Failed;
  };
  let _ = EncodedVideoChunk::template(scope);
  let wrapper = unsafe {
    wrap_instance(scope, EncodedVideoChunk::from_parts(key, timestamp, data))
  };
  ReadResult::Done(v8::Global::new(scope, wrapper))
}

fn write_chunk(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  helper: &dyn v8::ValueSerializerHelper,
  tag: u32,
  parts: (bool, i64, Vec<u8>),
) -> WriteResult {
  if is_structured_transfer(scope, object) || parts.2.len() > MAX_CHUNK_BYTES {
    reject_clone(scope);
    return WriteResult::Failed;
  }
  helper.write_uint32(tag);
  helper.write_uint32(u32::from(parts.0));
  helper.write_uint64(parts.1 as u64);
  write_bytes(helper, &parts.2);
  WriteResult::Wrote
}

fn read_chunk(
  helper: &dyn v8::ValueDeserializerHelper,
) -> Option<(bool, i64, Vec<u8>)> {
  let mut key = 0u32;
  let mut timestamp = 0u64;
  if !helper.read_uint32(&mut key)
    || key > 1
    || !helper.read_uint64(&mut timestamp)
  {
    return None;
  }
  let data = read_limited(helper, MAX_CHUNK_BYTES)?;
  Some((key == 1, timestamp as i64, data))
}

fn audio_fits(channels: &[Vec<f32>]) -> bool {
  if channels.len() > MAX_CHANNELS {
    return false;
  }
  let frames = channels.first().map(Vec::len).unwrap_or(0);
  frames <= MAX_FRAMES && channels.iter().all(|channel| channel.len() == frames)
}

fn read_audio_parts(
  helper: &dyn v8::ValueDeserializerHelper,
) -> Option<(Vec<Vec<f32>>, f64, f64)> {
  let mut rate = 0.0;
  let mut timestamp = 0.0;
  let mut count = 0u32;
  if !helper.read_double(&mut rate)
    || !helper.read_double(&mut timestamp)
    || !helper.read_uint32(&mut count)
    || count as usize > MAX_CHANNELS
  {
    return None;
  }
  let mut channels = Vec::with_capacity(count as usize);
  let mut frames = None;
  for _ in 0..count {
    let plane = read_plane(helper)?;
    if let Some(expected) = frames {
      if plane.len() != expected {
        return None;
      }
    } else {
      frames = Some(plane.len());
    }
    channels.push(plane);
  }
  Some((channels, rate, timestamp))
}

fn read_plane(helper: &dyn v8::ValueDeserializerHelper) -> Option<Vec<f32>> {
  let bytes = read_limited(helper, MAX_FRAMES * 4)?;
  if !bytes.len().is_multiple_of(4) {
    return None;
  }
  Some(
    bytes
      .chunks_exact(4)
      .map(|chunk| {
        let mut array = [0u8; 4];
        array.copy_from_slice(chunk);
        f32::from_le_bytes(array)
      })
      .collect(),
  )
}

fn f32_bytes(samples: &[f32]) -> Vec<u8> {
  let mut bytes = Vec::with_capacity(samples.len() * 4);
  for sample in samples {
    bytes.extend_from_slice(&sample.to_le_bytes());
  }
  bytes
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
