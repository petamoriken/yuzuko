//! `AudioData` construction and `copyTo`.

use yuzuko_core::op::OpError;
use yuzuko_core::rust_string;
use yuzuko_winter::buffer_source;

use crate::classes::{dictionary, long_long, member, unsigned};
use crate::samples::{AudioSamples, BuildError, CopyError, CopyRequest, parse_format};

pub(crate) struct BuiltAudio {
  pub samples: AudioSamples,
  pub timestamp: f64,
}

pub(crate) fn construct(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
) -> Result<BuiltAudio, OpError> {
  let object = dictionary(scope, args.get(0), "AudioDataInit")?;
  let format = required_string(scope, object, "format")?;
  let Some((kind, planar)) = parse_format(&format) else {
    return Err(OpError::type_("The format is invalid"));
  };
  let rate = sample_rate(scope, object)?;
  let frames = unsigned_member(scope, object, "numberOfFrames")?;
  let channels = unsigned_member(scope, object, "numberOfChannels")?;
  let timestamp = timestamp(scope, object)?;
  let Some(data_value) = member(scope, object, "data") else {
    return Err(OpError::type_("data is required"));
  };
  let Some(data) = buffer_source(scope, data_value) else {
    return Err(OpError::type_("data is not a BufferSource"));
  };
  let samples = AudioSamples::from_pcm(kind, planar, channels, frames, rate, &data)
    .map_err(build_error)?;
  Ok(BuiltAudio {
    samples,
    timestamp: timestamp as f64,
  })
}

pub(crate) fn copy_request(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
  index: i32,
) -> Result<CopyRequest, OpError> {
  let object = dictionary(scope, args.get(index), "AudioDataCopyToOptions")?;
  let Some(plane) = member(scope, object, "planeIndex") else {
    return Err(OpError::type_("planeIndex is required"));
  };
  let plane = unsigned(scope, plane, "planeIndex")?;
  let frame_offset = match member(scope, object, "frameOffset") {
    Some(value) => unsigned(scope, value, "frameOffset")?,
    None => 0,
  };
  let frame_count = match member(scope, object, "frameCount") {
    Some(value) => Some(unsigned(scope, value, "frameCount")?),
    None => None,
  };
  let format = match member(scope, object, "format") {
    Some(value) => {
      let name = rust_string(scope, value);
      let Some(parsed) = parse_format(&name) else {
        return Err(OpError::type_("The format is invalid"));
      };
      Some(parsed)
    }
    None => None,
  };
  Ok(CopyRequest {
    plane,
    frame_offset,
    frame_count,
    format,
  })
}

pub(crate) fn copy_error(error: CopyError) -> OpError {
  let message = match error {
    CopyError::Plane => "planeIndex is out of range",
    CopyError::Offset => "frameOffset is out of range",
    CopyError::Count => "frameCount is out of range",
  };
  OpError::range(message)
}

pub(crate) fn write_destination(
  _scope: &mut v8::PinScope,
  dest: v8::Local<v8::Value>,
  bytes: &[u8],
) -> Result<(), OpError> {
  if let Ok(view) = v8::Local::<v8::ArrayBufferView>::try_from(dest) {
    return write_at(view.data().cast(), view.byte_length(), bytes);
  }
  if let Ok(buffer) = v8::Local::<v8::SharedArrayBuffer>::try_from(dest) {
    let store = buffer.get_backing_store();
    let Some(data) = store.data() else {
      return Err(OpError::type_("The destination is detached"));
    };
    return write_at(data.as_ptr().cast(), buffer.byte_length(), bytes);
  }
  if let Ok(buffer) = v8::Local::<v8::ArrayBuffer>::try_from(dest) {
    if buffer.was_detached() {
      return Err(OpError::range("The destination is too small"));
    }
    let Some(data) = buffer.data() else {
      return Err(OpError::type_("The destination is detached"));
    };
    return write_at(data.as_ptr().cast(), buffer.byte_length(), bytes);
  }
  Err(OpError::type_("The destination is not a BufferSource"))
}

fn write_at(ptr: *mut u8, len: usize, bytes: &[u8]) -> Result<(), OpError> {
  if len < bytes.len() {
    return Err(OpError::range("The destination is too small"));
  }
  if bytes.is_empty() {
    return Ok(());
  }
  if ptr.is_null() {
    return Err(OpError::type_("The destination is detached"));
  }
  // `ptr` addresses `len` initialized bytes of a buffer that stays alive for
  // this copy. The source is a separate allocation.
  unsafe {
    std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr, bytes.len());
  }
  Ok(())
}

fn sample_rate(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
) -> Result<f64, OpError> {
  let Some(value) = member(scope, object, "sampleRate") else {
    return Err(OpError::type_("sampleRate is required"));
  };
  let number = value
    .number_value(scope)
    .ok_or_else(|| OpError::type_("sampleRate is not a number"))?;
  if !number.is_finite() {
    return Err(OpError::type_("sampleRate is out of range"));
  }
  let rate = number as f32;
  if !rate.is_finite() || rate <= 0.0 {
    return Err(OpError::type_("sampleRate must be greater than 0"));
  }
  Ok(f64::from(rate))
}

fn timestamp(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
) -> Result<i64, OpError> {
  let Some(value) = member(scope, object, "timestamp") else {
    return Err(OpError::type_("timestamp is required"));
  };
  long_long(scope, value)
}

fn unsigned_member(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
) -> Result<u32, OpError> {
  let Some(value) = member(scope, object, name) else {
    return Err(OpError::type_(format!("{name} is required")));
  };
  unsigned(scope, value, name)
}

fn required_string(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
) -> Result<String, OpError> {
  let Some(value) = member(scope, object, name) else {
    return Err(OpError::type_(format!("{name} is required")));
  };
  Ok(rust_string(scope, value))
}

fn build_error(error: BuildError) -> OpError {
  let message = match error {
    BuildError::Rate => "sampleRate must be greater than 0",
    BuildError::Frames => "numberOfFrames must be greater than 0",
    BuildError::Channels => "numberOfChannels must be greater than 0",
    BuildError::TooSmall => "data is too small",
    BuildError::Limits => "The audio data is too large",
  };
  OpError::type_(message)
}
