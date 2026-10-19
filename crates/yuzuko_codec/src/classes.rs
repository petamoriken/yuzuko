//! WebCodecs classes on `globalThis`.

use std::cell::{Cell, RefCell};
use std::panic::{AssertUnwindSafe, catch_unwind};

use yuzuko_core::op::{
  Js, OpError, host_ref, new_dom_exception, traced, traced_local, wrap_instance,
};
use yuzuko_core::rust_string;
use yuzuko_image::{FramePixels, Raster};
use yuzuko_macros::op;
use yuzuko_winter::buffer_source;

use crate::audio::{AudioBackend, AudioCodec};
use crate::backend::{Codec, VideoBackend, VideoConfig};
use crate::error::DecodeError;

#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
  Unconfigured,
  Configured,
  Closed,
}

impl State {
  fn name(self) -> &'static str {
    match self {
      Self::Unconfigured => "unconfigured",
      Self::Configured => "configured",
      Self::Closed => "closed",
    }
  }
}

struct ChunkParts {
  key: bool,
  timestamp: i64,
  data: Vec<u8>,
}

pub struct VideoFrame {
  raster: RefCell<Option<Raster>>,
  timestamp: f64,
}

unsafe impl v8::cppgc::GarbageCollected for VideoFrame {
  fn trace(&self, _visitor: &mut v8::cppgc::Visitor) {}

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"VideoFrame"
  }
}

impl VideoFrame {
  pub(crate) fn from_raster(raster: Raster) -> Self {
    Self::new(raster, 0.0)
  }

  fn new(raster: Raster, timestamp: f64) -> Self {
    Self {
      raster: RefCell::new(Some(raster)),
      timestamp,
    }
  }

  fn pixels(&self) -> Option<Raster> {
    self.raster.borrow().clone()
  }
}

pub fn lookup_frame(
  scope: &mut v8::PinScope,
  value: v8::Global<v8::Value>,
) -> FramePixels {
  let value = v8::Local::new(scope, value);
  let Ok(object) = v8::Local::<v8::Object>::try_from(value) else {
    return FramePixels::NotFrame;
  };
  let Some(host) = (unsafe { host_ref::<VideoFrame>(scope, object) }) else {
    return FramePixels::NotFrame;
  };
  match host.get().pixels() {
    Some(raster) => FramePixels::Ready(raster),
    None => FramePixels::Closed,
  }
}

#[op]
impl VideoFrame {
  #[getter]
  fn format(&self) -> Result<String, OpError> {
    self.live()?;
    Ok("RGBA".to_string())
  }

  #[getter]
  fn color_space(&self) -> Result<String, OpError> {
    self.live()?;
    Ok("srgb".to_string())
  }

  #[getter]
  fn coded_width(&self) -> Result<u32, OpError> {
    Ok(self.live()?.width)
  }

  #[getter]
  fn coded_height(&self) -> Result<u32, OpError> {
    Ok(self.live()?.height)
  }

  #[getter]
  fn display_width(&self) -> Result<u32, OpError> {
    Ok(self.live()?.width)
  }

  #[getter]
  fn display_height(&self) -> Result<u32, OpError> {
    Ok(self.live()?.height)
  }

  #[getter]
  fn timestamp(&self) -> Result<f64, OpError> {
    self.live()?;
    Ok(self.timestamp)
  }

  #[getter]
  fn duration(&self) -> Result<Option<f64>, OpError> {
    self.live()?;
    Ok(None)
  }

  fn close(&self) -> Result<(), OpError> {
    let mut raster = self.raster.borrow_mut();
    if raster.is_none() {
      return Err(closed_frame());
    }
    *raster = None;
    Ok(())
  }
}

impl VideoFrame {
  fn live(&self) -> Result<Raster, OpError> {
    self.pixels().ok_or_else(closed_frame)
  }
}

pub struct EncodedVideoChunk {
  key: bool,
  timestamp: i64,
  data: Vec<u8>,
}

unsafe impl v8::cppgc::GarbageCollected for EncodedVideoChunk {
  fn trace(&self, _visitor: &mut v8::cppgc::Visitor) {}

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"EncodedVideoChunk"
  }
}

#[op]
impl EncodedVideoChunk {
  #[constructor]
  #[length(1)]
  fn new(
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<Self, OpError> {
    let parts = chunk_parts(scope, args.get(0))?;
    Ok(Self {
      key: parts.key,
      timestamp: parts.timestamp,
      data: parts.data,
    })
  }

  #[getter]
  #[rename("type")]
  fn chunk_type(&self) -> String {
    if self.key {
      "key".to_string()
    } else {
      "delta".to_string()
    }
  }

  #[getter]
  fn timestamp(&self) -> f64 {
    self.timestamp as f64
  }

  #[getter]
  fn byte_length(&self) -> u32 {
    u32::try_from(self.data.len()).unwrap_or(u32::MAX)
  }
}

pub struct VideoDecoder {
  state: Cell<State>,
  output: RefCell<Option<v8::TracedReference<v8::Function>>>,
  error: RefCell<Option<v8::TracedReference<v8::Function>>>,
  backend: RefCell<Option<VideoBackend>>,
}

unsafe impl v8::cppgc::GarbageCollected for VideoDecoder {
  fn trace(&self, visitor: &mut v8::cppgc::Visitor) {
    trace_fn(&self.output, visitor);
    trace_fn(&self.error, visitor);
  }

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"VideoDecoder"
  }
}

#[op]
impl VideoDecoder {
  #[constructor]
  #[length(1)]
  fn new(
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<Self, OpError> {
    let (output, error) = decoder_init(scope, args.get(0))?;
    Ok(Self {
      state: Cell::new(State::Unconfigured),
      output: RefCell::new(Some(traced(scope, output))),
      error: RefCell::new(Some(traced(scope, error))),
      backend: RefCell::new(None),
    })
  }

  #[getter]
  fn state(&self) -> String {
    self.state.get().name().to_string()
  }

  #[getter]
  fn decode_queue_size(&self) -> u32 {
    0
  }

  fn configure(
    &self,
    scope: &mut v8::PinScope,
    config: v8::Local<v8::Value>,
  ) -> Result<(), OpError> {
    self.ensure_open()?;
    let config = video_config(scope, config)?;
    let opened = catch_unwind(AssertUnwindSafe(|| VideoBackend::open(config)));
    let backend = match opened {
      Ok(Ok(backend)) => backend,
      Ok(Err(error)) => {
        self.reset_backend();
        return Err(config_error(error));
      }
      Err(_) => {
        self.reset_backend();
        return Err(panicked());
      }
    };
    *self.backend.borrow_mut() = Some(backend);
    self.state.set(State::Configured);
    Ok(())
  }

  fn decode(
    &self,
    scope: &mut v8::PinScope,
    chunk: v8::Local<v8::Value>,
  ) -> Result<(), OpError> {
    self.ensure_configured()?;
    let (bytes, timestamp) = video_chunk(scope, chunk)?;
    let decoded = {
      let mut guard = self.backend.borrow_mut();
      let Some(backend) = guard.as_mut() else {
        return Err(not_configured());
      };
      catch_unwind(AssertUnwindSafe(|| backend.decode(&bytes)))
    };
    match decoded {
      Ok(Ok(frames)) => {
        for raster in frames {
          self.emit_frame(scope, raster, timestamp)?;
        }
        Ok(())
      }
      Ok(Err(DecodeError::Panic)) | Err(_) => {
        self.shutdown();
        Err(panicked())
      }
      Ok(Err(_)) => {
        self.fail(scope, "The frame could not be decoded");
        Ok(())
      }
    }
  }

  fn flush(&self, scope: &mut v8::PinScope) -> Result<Js, OpError> {
    self.ensure_configured()?;
    let frames = {
      let mut guard = self.backend.borrow_mut();
      let Some(backend) = guard.as_mut() else {
        return Err(not_configured());
      };
      backend.flush().map_err(config_error)?
    };
    for raster in frames {
      self.emit_frame(scope, raster, 0.0)?;
    }
    resolved(scope)
  }

  fn reset(&self) -> Result<(), OpError> {
    self.ensure_open()?;
    self.reset_backend();
    Ok(())
  }

  fn close(&self) -> Result<(), OpError> {
    self.ensure_open()?;
    self.shutdown();
    Ok(())
  }
}

impl VideoDecoder {
  fn ensure_open(&self) -> Result<(), OpError> {
    if self.state.get() == State::Closed {
      Err(decoder_closed())
    } else {
      Ok(())
    }
  }

  fn ensure_configured(&self) -> Result<(), OpError> {
    if self.state.get() != State::Configured {
      Err(not_configured())
    } else {
      Ok(())
    }
  }

  fn reset_backend(&self) {
    *self.backend.borrow_mut() = None;
    if self.state.get() != State::Closed {
      self.state.set(State::Unconfigured);
    }
  }

  fn shutdown(&self) {
    *self.backend.borrow_mut() = None;
    self.state.set(State::Closed);
  }

  fn emit_frame(
    &self,
    scope: &mut v8::PinScope,
    raster: Raster,
    timestamp: f64,
  ) -> Result<(), OpError> {
    let Some(function) = callback(scope, &self.output) else {
      return Ok(());
    };
    let _ = VideoFrame::template(scope);
    let frame =
      unsafe { wrap_instance(scope, VideoFrame::new(raster, timestamp)) };
    let function = v8::Global::new(scope, function);
    let frame = v8::Global::new(scope, frame);
    let global = yuzuko_core::global(scope);
    let global = v8::Global::new(scope, global);
    let thrown = {
      v8::tc_scope!(let scope, scope);
      let function = v8::Local::new(scope, function);
      let frame = v8::Local::new(scope, frame);
      let global = v8::Local::new(scope, global);
      if function
        .call(scope, global.into(), &[frame.into()])
        .is_some()
      {
        None
      } else {
        scope.exception().map(|value| v8::Global::new(scope, value))
      }
    };
    if let Some(thrown) = thrown {
      self.fail_value(scope, thrown);
    }
    Ok(())
  }

  fn fail(&self, scope: &mut v8::PinScope, message: &str) {
    let Some(error) = new_dom_exception(scope, "EncodingError", message) else {
      self.shutdown();
      return;
    };
    let error: v8::Local<v8::Value> = error.into();
    self.fail_value(scope, v8::Global::new(scope, error));
  }

  fn fail_value(&self, scope: &mut v8::PinScope, error: v8::Global<v8::Value>) {
    self.shutdown();
    let error_local = v8::Local::new(scope, &error);
    if let Some(function) = callback(scope, &self.error) {
      let global = yuzuko_core::global(scope);
      v8::tc_scope!(let scope, scope);
      let function = v8::Local::new(scope, function);
      let error_local = v8::Local::new(scope, error_local);
      let global = v8::Local::new(scope, global);
      let _ = function.call(scope, global.into(), &[error_local]);
    }
    let error_local = v8::Local::new(scope, error);
    yuzuko_dom::report_error(scope, error_local);
  }
}

pub struct AudioData {
  channels: RefCell<Option<Vec<Vec<f32>>>>,
  rate: f64,
  timestamp: f64,
}

unsafe impl v8::cppgc::GarbageCollected for AudioData {
  fn trace(&self, _visitor: &mut v8::cppgc::Visitor) {}

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"AudioData"
  }
}

impl AudioData {
  fn new(channels: Vec<Vec<f32>>, rate: f64, timestamp: f64) -> Self {
    Self {
      channels: RefCell::new(Some(channels)),
      rate,
      timestamp,
    }
  }
}

#[op]
impl AudioData {
  #[getter]
  fn format(&self) -> Result<String, OpError> {
    self.live()?;
    Ok("f32-planar".to_string())
  }

  #[getter]
  fn sample_rate(&self) -> Result<f64, OpError> {
    self.live()?;
    Ok(self.rate)
  }

  #[getter]
  fn number_of_frames(&self) -> Result<u32, OpError> {
    let channels = self.live()?;
    let frames = channels.first().map(Vec::len).unwrap_or(0);
    u32::try_from(frames).map_err(|_| closed_data())
  }

  #[getter]
  fn number_of_channels(&self) -> Result<u32, OpError> {
    let channels = self.live()?;
    u32::try_from(channels.len()).map_err(|_| closed_data())
  }

  #[getter]
  fn timestamp(&self) -> Result<f64, OpError> {
    self.live()?;
    Ok(self.timestamp)
  }

  #[getter]
  fn duration(&self) -> Result<Option<f64>, OpError> {
    let channels = self.live()?;
    let frames = channels.first().map(Vec::len).unwrap_or(0);
    if self.rate <= 0.0 {
      return Ok(None);
    }
    Ok(Some(frames as f64 / self.rate * 1_000_000.0))
  }

  fn close(&self) -> Result<(), OpError> {
    let mut channels = self.channels.borrow_mut();
    if channels.is_none() {
      return Err(closed_data());
    }
    *channels = None;
    Ok(())
  }

  fn copy_to(
    &self,
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<(), OpError> {
    let channels = self.live()?;
    let plane = plane_index(scope, args)?;
    let samples = channels
      .get(plane)
      .ok_or_else(|| OpError::range("planeIndex is out of range"))?;
    write_f32(scope, args.get(0), samples)
  }
}

impl AudioData {
  fn live(&self) -> Result<Vec<Vec<f32>>, OpError> {
    self.channels.borrow().clone().ok_or_else(closed_data)
  }
}

pub struct EncodedAudioChunk {
  key: bool,
  timestamp: i64,
  data: Vec<u8>,
}

unsafe impl v8::cppgc::GarbageCollected for EncodedAudioChunk {
  fn trace(&self, _visitor: &mut v8::cppgc::Visitor) {}

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"EncodedAudioChunk"
  }
}

#[op]
impl EncodedAudioChunk {
  #[constructor]
  #[length(1)]
  fn new(
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<Self, OpError> {
    let parts = chunk_parts(scope, args.get(0))?;
    Ok(Self {
      key: parts.key,
      timestamp: parts.timestamp,
      data: parts.data,
    })
  }

  #[getter]
  #[rename("type")]
  fn chunk_type(&self) -> String {
    if self.key {
      "key".to_string()
    } else {
      "delta".to_string()
    }
  }

  #[getter]
  fn timestamp(&self) -> f64 {
    self.timestamp as f64
  }

  #[getter]
  fn byte_length(&self) -> u32 {
    u32::try_from(self.data.len()).unwrap_or(u32::MAX)
  }
}

pub struct AudioDecoder {
  state: Cell<State>,
  output: RefCell<Option<v8::TracedReference<v8::Function>>>,
  error: RefCell<Option<v8::TracedReference<v8::Function>>>,
  backend: RefCell<Option<AudioBackend>>,
}

unsafe impl v8::cppgc::GarbageCollected for AudioDecoder {
  fn trace(&self, visitor: &mut v8::cppgc::Visitor) {
    trace_fn(&self.output, visitor);
    trace_fn(&self.error, visitor);
  }

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"AudioDecoder"
  }
}

#[op]
impl AudioDecoder {
  #[constructor]
  #[length(1)]
  fn new(
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<Self, OpError> {
    let (output, error) = decoder_init(scope, args.get(0))?;
    Ok(Self {
      state: Cell::new(State::Unconfigured),
      output: RefCell::new(Some(traced(scope, output))),
      error: RefCell::new(Some(traced(scope, error))),
      backend: RefCell::new(None),
    })
  }

  #[getter]
  fn state(&self) -> String {
    self.state.get().name().to_string()
  }

  #[getter]
  fn decode_queue_size(&self) -> u32 {
    0
  }

  fn configure(
    &self,
    scope: &mut v8::PinScope,
    config: v8::Local<v8::Value>,
  ) -> Result<(), OpError> {
    self.ensure_open()?;
    let (codec, description, rate, channels) = audio_config(scope, config)?;
    let opened = catch_unwind(AssertUnwindSafe(|| {
      AudioBackend::open(codec, &description, rate, channels)
    }));
    let backend = match opened {
      Ok(Ok(backend)) => backend,
      Ok(Err(error)) => {
        self.reset_backend();
        return Err(config_error(error));
      }
      Err(_) => {
        self.reset_backend();
        return Err(panicked());
      }
    };
    *self.backend.borrow_mut() = Some(backend);
    self.state.set(State::Configured);
    Ok(())
  }

  fn decode(
    &self,
    scope: &mut v8::PinScope,
    chunk: v8::Local<v8::Value>,
  ) -> Result<(), OpError> {
    self.ensure_configured()?;
    let (bytes, timestamp) = audio_chunk(scope, chunk)?;
    let decoded = {
      let mut guard = self.backend.borrow_mut();
      let Some(backend) = guard.as_mut() else {
        return Err(not_configured());
      };
      catch_unwind(AssertUnwindSafe(|| backend.decode(&bytes)))
    };
    match decoded {
      Ok(Ok(Some(pcm))) => {
        self.emit_audio(scope, pcm.channels, pcm.rate, timestamp)
      }
      Ok(Ok(None)) => Ok(()),
      Ok(Err(DecodeError::Panic)) | Err(_) => {
        self.shutdown();
        Err(panicked())
      }
      Ok(Err(_)) => {
        self.fail(scope, "The audio could not be decoded");
        Ok(())
      }
    }
  }

  fn flush(&self, scope: &mut v8::PinScope) -> Result<Js, OpError> {
    self.ensure_configured()?;
    resolved(scope)
  }

  fn reset(&self) -> Result<(), OpError> {
    self.ensure_open()?;
    self.reset_backend();
    Ok(())
  }

  fn close(&self) -> Result<(), OpError> {
    self.ensure_open()?;
    self.shutdown();
    Ok(())
  }
}

impl AudioDecoder {
  fn ensure_open(&self) -> Result<(), OpError> {
    if self.state.get() == State::Closed {
      Err(decoder_closed())
    } else {
      Ok(())
    }
  }

  fn ensure_configured(&self) -> Result<(), OpError> {
    if self.state.get() != State::Configured {
      Err(not_configured())
    } else {
      Ok(())
    }
  }

  fn reset_backend(&self) {
    *self.backend.borrow_mut() = None;
    if self.state.get() != State::Closed {
      self.state.set(State::Unconfigured);
    }
  }

  fn shutdown(&self) {
    *self.backend.borrow_mut() = None;
    self.state.set(State::Closed);
  }

  fn emit_audio(
    &self,
    scope: &mut v8::PinScope,
    channels: Vec<Vec<f32>>,
    rate: f64,
    timestamp: f64,
  ) -> Result<(), OpError> {
    let Some(function) = callback(scope, &self.output) else {
      return Ok(());
    };
    let _ = AudioData::template(scope);
    let data = unsafe {
      wrap_instance(scope, AudioData::new(channels, rate, timestamp))
    };
    let function = v8::Global::new(scope, function);
    let data = v8::Global::new(scope, data);
    let global = yuzuko_core::global(scope);
    let global = v8::Global::new(scope, global);
    let thrown = {
      v8::tc_scope!(let scope, scope);
      let function = v8::Local::new(scope, function);
      let data = v8::Local::new(scope, data);
      let global = v8::Local::new(scope, global);
      if function
        .call(scope, global.into(), &[data.into()])
        .is_some()
      {
        None
      } else {
        scope.exception().map(|value| v8::Global::new(scope, value))
      }
    };
    if let Some(thrown) = thrown {
      self.fail_value(scope, thrown);
    }
    Ok(())
  }

  fn fail(&self, scope: &mut v8::PinScope, message: &str) {
    let Some(error) = new_dom_exception(scope, "EncodingError", message) else {
      self.shutdown();
      return;
    };
    let error: v8::Local<v8::Value> = error.into();
    self.fail_value(scope, v8::Global::new(scope, error));
  }

  fn fail_value(&self, scope: &mut v8::PinScope, error: v8::Global<v8::Value>) {
    self.shutdown();
    let error_local = v8::Local::new(scope, &error);
    if let Some(function) = callback(scope, &self.error) {
      let global = yuzuko_core::global(scope);
      v8::tc_scope!(let scope, scope);
      let function = v8::Local::new(scope, function);
      let error_local = v8::Local::new(scope, error_local);
      let global = v8::Local::new(scope, global);
      let _ = function.call(scope, global.into(), &[error_local]);
    }
    let error_local = v8::Local::new(scope, error);
    yuzuko_dom::report_error(scope, error_local);
  }
}

fn video_config(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<VideoConfig, OpError> {
  let object = dictionary(scope, value, "VideoDecoder configuration")?;
  let codec = required_string(scope, object, "codec")?;
  let Some(codec) = video_codec(&codec) else {
    return Err(unsupported_codec());
  };
  let description = optional_bytes(scope, object, "description")?;
  let width = optional_u32(scope, object, "codedWidth")?;
  let height = optional_u32(scope, object, "codedHeight")?;
  if let Some(hint) = member(scope, object, "hardwareAcceleration") {
    let _ignored = software_hint(scope, hint)?;
  }
  Ok(VideoConfig {
    codec,
    description,
    width,
    height,
  })
}

fn audio_config(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<(AudioCodec, Vec<u8>, u32, u32), OpError> {
  let object = dictionary(scope, value, "AudioDecoder configuration")?;
  let codec = required_string(scope, object, "codec")?;
  let Some(codec) = audio_codec(&codec) else {
    return Err(unsupported_codec());
  };
  let rate = required_u32(scope, object, "sampleRate")?;
  let channels = required_u32(scope, object, "numberOfChannels")?;
  let description = optional_bytes(scope, object, "description")?;
  Ok((codec, description, rate, channels))
}

fn video_codec(codec: &str) -> Option<Codec> {
  if codec.starts_with("avc1") || codec.starts_with("avc3") {
    Some(Codec::Avc)
  } else {
    None
  }
}

fn audio_codec(codec: &str) -> Option<AudioCodec> {
  if codec == "mp4a" || codec.starts_with("mp4a.") {
    Some(AudioCodec::Aac)
  } else {
    None
  }
}

fn video_chunk(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<(Vec<u8>, f64), OpError> {
  let object = v8::Local::<v8::Object>::try_from(value)
    .map_err(|_| OpError::type_("The chunk is not an EncodedVideoChunk"))?;
  let Some(host) = (unsafe { host_ref::<EncodedVideoChunk>(scope, object) })
  else {
    return Err(OpError::type_("The chunk is not an EncodedVideoChunk"));
  };
  let chunk = host.get();
  Ok((chunk.data.clone(), chunk.timestamp as f64))
}

fn audio_chunk(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<(Vec<u8>, f64), OpError> {
  let object = v8::Local::<v8::Object>::try_from(value)
    .map_err(|_| OpError::type_("The chunk is not an EncodedAudioChunk"))?;
  let Some(host) = (unsafe { host_ref::<EncodedAudioChunk>(scope, object) })
  else {
    return Err(OpError::type_("The chunk is not an EncodedAudioChunk"));
  };
  let chunk = host.get();
  Ok((chunk.data.clone(), chunk.timestamp as f64))
}

fn chunk_parts(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<ChunkParts, OpError> {
  let object = dictionary(scope, value, "Encoded chunk")?;
  let kind = required_string(scope, object, "type")?;
  let key = match kind.as_str() {
    "key" => true,
    "delta" => false,
    _ => return Err(OpError::type_("The chunk type is invalid")),
  };
  let Some(timestamp) = member(scope, object, "timestamp") else {
    return Err(OpError::type_("timestamp is required"));
  };
  let Some(data) = member(scope, object, "data") else {
    return Err(OpError::type_("data is required"));
  };
  let Some(data) = buffer_source(scope, data) else {
    return Err(OpError::type_("data is not a BufferSource"));
  };
  Ok(ChunkParts {
    key,
    timestamp: long_long(scope, timestamp)?,
    data,
  })
}

fn decoder_init<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  value: v8::Local<'_, v8::Value>,
) -> Result<(v8::Local<'s, v8::Function>, v8::Local<'s, v8::Function>), OpError>
{
  let object = dictionary(scope, value, "Decoder initialization")?;
  let output = required_function(scope, object, "output")?;
  let error = required_function(scope, object, "error")?;
  Ok((output, error))
}

fn dictionary<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  value: v8::Local<'_, v8::Value>,
  name: &str,
) -> Result<v8::Local<'s, v8::Object>, OpError> {
  let value = yuzuko_core::op::root(scope, value);
  if value.is_null() || !value.is_object() {
    return Err(OpError::type_(format!("{name} is not an object")));
  }
  v8::Local::<v8::Object>::try_from(value)
    .map_err(|_| OpError::type_(format!("{name} is not an object")))
}

fn member<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  object: v8::Local<'_, v8::Object>,
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

fn required_function<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  object: v8::Local<v8::Object>,
  name: &str,
) -> Result<v8::Local<'s, v8::Function>, OpError> {
  let Some(value) = member(scope, object, name) else {
    return Err(OpError::type_(format!("{name} is required")));
  };
  v8::Local::<v8::Function>::try_from(value)
    .map_err(|_| OpError::type_(format!("{name} is not a function")))
}

fn required_u32(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
) -> Result<u32, OpError> {
  let Some(value) = member(scope, object, name) else {
    return Err(OpError::type_(format!("{name} is required")));
  };
  unsigned(scope, value, name)
}

fn optional_u32(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
) -> Result<u32, OpError> {
  let Some(value) = member(scope, object, name) else {
    return Ok(0);
  };
  unsigned(scope, value, name)
}

fn optional_bytes(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
) -> Result<Vec<u8>, OpError> {
  let Some(value) = member(scope, object, name) else {
    return Ok(Vec::new());
  };
  buffer_source(scope, value)
    .ok_or_else(|| OpError::type_(format!("{name} is not a BufferSource")))
}

fn unsigned(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
  name: &str,
) -> Result<u32, OpError> {
  let number = value
    .number_value(scope)
    .ok_or_else(|| OpError::type_(format!("{name} is not a number")))?;
  if !number.is_finite() || number < 0.0 || number > f64::from(u32::MAX) {
    return Err(OpError::type_(format!("{name} is out of range")));
  }
  Ok(number as u32)
}

fn long_long(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<i64, OpError> {
  let number = value
    .number_value(scope)
    .ok_or_else(|| OpError::type_("The timestamp is not a number"))?;
  if !number.is_finite() || number < i64::MIN as f64 || number > i64::MAX as f64
  {
    return Err(OpError::type_("The timestamp is out of range"));
  }
  Ok(number as i64)
}

fn software_hint(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<bool, OpError> {
  match rust_string(scope, value).as_str() {
    "no-preference" | "prefer-hardware" => Ok(false),
    "prefer-software" => Ok(true),
    _ => Err(OpError::type_("hardwareAcceleration is not supported")),
  }
}

fn plane_index(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
) -> Result<usize, OpError> {
  if args.length() < 2 || args.get(1).is_undefined() {
    return Ok(0);
  }
  let object = dictionary(scope, args.get(1), "AudioDataCopyToOptions")?;
  let Some(value) = member(scope, object, "planeIndex") else {
    return Ok(0);
  };
  Ok(unsigned(scope, value, "planeIndex")? as usize)
}

fn write_f32(
  scope: &mut v8::PinScope,
  dest: v8::Local<v8::Value>,
  samples: &[f32],
) -> Result<(), OpError> {
  let bytes = samples.len() * 4;
  if let Ok(view) = v8::Local::<v8::ArrayBufferView>::try_from(dest) {
    let buffer = view
      .buffer(scope)
      .ok_or_else(|| OpError::type_("The destination is detached"))?;
    if buffer.was_detached() || view.byte_length() < bytes {
      return Err(OpError::type_("The destination is too small"));
    }
    copy_f32(samples, &buffer, view.byte_offset())?;
    return Ok(());
  }
  if let Ok(buffer) = v8::Local::<v8::ArrayBuffer>::try_from(dest) {
    if buffer.was_detached() || buffer.byte_length() < bytes {
      return Err(OpError::type_("The destination is too small"));
    }
    copy_f32(samples, &buffer, 0)?;
    return Ok(());
  }
  Err(OpError::type_("The destination is not a BufferSource"))
}

fn copy_f32(
  samples: &[f32],
  buffer: &v8::ArrayBuffer,
  offset: usize,
) -> Result<(), OpError> {
  let store = buffer.get_backing_store();
  let Some(data) = store.data() else {
    return Err(OpError::type_("The destination is detached"));
  };
  if samples.is_empty() {
    return Ok(());
  }
  unsafe {
    std::ptr::copy_nonoverlapping(
      samples.as_ptr().cast::<u8>(),
      data.as_ptr().cast::<u8>().add(offset),
      samples.len() * 4,
    );
  }
  Ok(())
}

fn callback<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  slot: &RefCell<Option<v8::TracedReference<v8::Function>>>,
) -> Option<v8::Local<'s, v8::Function>> {
  let slot = slot.borrow();
  let reference = slot.as_ref()?;
  traced_local(scope, reference)
}

fn trace_fn(
  slot: &RefCell<Option<v8::TracedReference<v8::Function>>>,
  visitor: &mut v8::cppgc::Visitor,
) {
  if let Ok(slot) = slot.try_borrow() {
    visitor.trace(&*slot);
  }
}

fn resolved(scope: &mut v8::PinScope) -> Result<Js, OpError> {
  let Some(resolver) = v8::PromiseResolver::new(scope) else {
    return Err(OpError::type_("Promise is unavailable"));
  };
  let promise = resolver.get_promise(scope);
  let value: v8::Local<v8::Value> = v8::undefined(scope).into();
  resolver.resolve(scope, value);
  let promise: v8::Local<v8::Value> = promise.into();
  Ok(Js(v8::Global::new(scope, promise)))
}

fn config_error(error: DecodeError) -> OpError {
  match error {
    DecodeError::Panic => panicked(),
    DecodeError::Unsupported | DecodeError::Rejected => unsupported_codec(),
  }
}

fn unsupported_codec() -> OpError {
  OpError::dom(
    "NotSupportedError",
    "The codec configuration is not supported",
  )
}

fn panicked() -> OpError {
  OpError::dom("InvalidStateError", "The decoder panicked")
}

fn decoder_closed() -> OpError {
  OpError::dom("InvalidStateError", "The decoder is closed")
}

fn not_configured() -> OpError {
  OpError::dom("InvalidStateError", "The decoder is not configured")
}

fn closed_frame() -> OpError {
  OpError::dom("InvalidStateError", "The frame is closed")
}

fn closed_data() -> OpError {
  OpError::dom("InvalidStateError", "The audio data is closed")
}
