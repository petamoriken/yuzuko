//! WebCodecs classes on `globalThis`.

use std::cell::{Cell, RefCell};
use std::panic::{AssertUnwindSafe, catch_unwind};

use yuzuko_core::op::{
  Js, New, OpError, host_ref, new_dom_exception, traced, traced_local,
  wrap_instance,
};
use yuzuko_core::rust_string;
use yuzuko_image::{FramePixels, Raster};
use yuzuko_macros::op;
use yuzuko_winter::{array_buffer, buffer_source};

use crate::audio::{AudioBackend, AudioCodec};
use crate::backend::{Codec, VideoBackend, VideoConfig};
use crate::error::DecodeError;
use crate::layout::{FrameColor, FrameData, frame_from_raster};

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
  duration: Option<f64>,
  data: Vec<u8>,
}

pub struct VideoFrame {
  body: RefCell<Option<FrameData>>,
  timestamp: Cell<f64>,
  duration: Cell<Option<f64>>,
}

unsafe impl v8::cppgc::GarbageCollected for VideoFrame {
  fn trace(&self, _visitor: &mut v8::cppgc::Visitor) {}

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"VideoFrame"
  }
}

impl VideoFrame {
  pub(crate) fn from_raster(raster: Raster) -> Self {
    Self::from_data(frame_from_raster(&raster), 0.0, None)
  }

  pub(crate) fn from_data(
    data: FrameData,
    timestamp: f64,
    duration: Option<f64>,
  ) -> Self {
    Self {
      body: RefCell::new(Some(data)),
      timestamp: Cell::new(timestamp),
      duration: Cell::new(duration),
    }
  }

  pub(crate) fn snapshot(&self) -> Option<FrameData> {
    self.body.borrow().clone()
  }

  pub(crate) fn timestamp_us(&self) -> f64 {
    self.timestamp.get()
  }

  pub(crate) fn duration_us(&self) -> Option<f64> {
    self.duration.get()
  }

  fn pixels(&self) -> Option<Raster> {
    self.body.borrow().as_ref().map(|data| data.raster.clone())
  }

  fn metric(&self, read: impl FnOnce(&FrameData) -> u32) -> u32 {
    self.body.borrow().as_ref().map(read).unwrap_or(0)
  }

  pub(crate) fn is_closed(&self) -> bool {
    self.body.borrow().is_none()
  }

  pub(crate) fn neuter(&self) {
    *self.body.borrow_mut() = None;
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
  #[constructor]
  #[length(1)]
  fn new(
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<Self, OpError> {
    crate::frame::construct(scope, args)
  }

  #[getter]
  fn format(&self) -> Option<String> {
    self
      .body
      .borrow()
      .as_ref()
      .map(|data| data.format.name().to_string())
  }

  #[getter]
  fn color_space(&self, _scope: &mut v8::PinScope) -> New<VideoColorSpace> {
    let color = self.body.borrow().as_ref().map(|data| data.color.clone());
    let space = match color {
      Some(color) => VideoColorSpace::from_frame(&color),
      None => VideoColorSpace::empty(),
    };
    New(space)
  }

  #[getter]
  fn coded_width(&self) -> u32 {
    self.metric(|data| data.coded_width)
  }

  #[getter]
  fn coded_height(&self) -> u32 {
    self.metric(|data| data.coded_height)
  }

  #[getter]
  fn display_width(&self) -> u32 {
    self.metric(|data| data.display_width)
  }

  #[getter]
  fn display_height(&self) -> u32 {
    self.metric(|data| data.display_height)
  }

  #[getter]
  fn timestamp(&self) -> f64 {
    self.timestamp_us()
  }

  #[getter]
  fn duration(&self) -> Option<f64> {
    self.duration_us()
  }

  #[getter]
  fn visible_rect(&self, scope: &mut v8::PinScope) -> Js {
    crate::frame::visible_rect(scope, self)
  }

  #[getter]
  fn coded_rect(&self, scope: &mut v8::PinScope) -> Js {
    crate::frame::coded_rect(scope, self)
  }

  fn close(&self) {
    self.neuter();
  }

  #[rename("clone")]
  fn clone_frame(&self) -> Result<New<Self>, OpError> {
    let data = self.snapshot().ok_or_else(closed_frame)?;
    Ok(New(Self::from_data(
      data,
      self.timestamp_us(),
      self.duration_us(),
    )))
  }

  fn allocation_size(
    &self,
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<f64, OpError> {
    crate::frame::allocation_size(scope, self, args)
  }

  fn copy_to(
    &self,
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<Js, OpError> {
    crate::frame::copy_to(scope, self, args)
  }
}

pub(crate) const COLOR_PRIMARIES: &[&str] =
  &["bt709", "bt470bg", "smpte170m", "bt2020", "smpte432"];
pub(crate) const COLOR_TRANSFER: &[&str] =
  &["bt709", "smpte170m", "iec61966-2-1", "linear", "pq", "hlg"];
pub(crate) const COLOR_MATRIX: &[&str] =
  &["rgb", "bt709", "bt470bg", "smpte170m", "bt2020-ncl"];

pub(crate) struct VideoColorSpace {
  primaries: Option<String>,
  transfer: Option<String>,
  matrix: Option<String>,
  full_range: Option<bool>,
}

unsafe impl v8::cppgc::GarbageCollected for VideoColorSpace {
  fn trace(&self, _visitor: &mut v8::cppgc::Visitor) {}

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"VideoColorSpace"
  }
}

impl VideoColorSpace {
  fn empty() -> Self {
    Self {
      primaries: None,
      transfer: None,
      matrix: None,
      full_range: None,
    }
  }

  fn from_frame(color: &FrameColor) -> Self {
    Self {
      primaries: color.primaries.clone(),
      transfer: color.transfer.clone(),
      matrix: color.matrix.clone(),
      full_range: color.full_range,
    }
  }
}

#[op]
impl VideoColorSpace {
  #[constructor]
  fn new(
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<Self, OpError> {
    if args.length() == 0 || args.get(0).is_null_or_undefined() {
      return Ok(Self::empty());
    }
    let object = v8::Local::<v8::Object>::try_from(args.get(0))
      .map_err(|_| OpError::type_("VideoColorSpace init must be an object"))?;
    Ok(Self {
      primaries: color_enum(scope, object, "primaries", COLOR_PRIMARIES)?,
      transfer: color_enum(scope, object, "transfer", COLOR_TRANSFER)?,
      matrix: color_enum(scope, object, "matrix", COLOR_MATRIX)?,
      full_range: color_bool(scope, object, "fullRange")?,
    })
  }

  #[getter]
  fn primaries(&self) -> Option<String> {
    self.primaries.clone()
  }

  #[getter]
  fn transfer(&self) -> Option<String> {
    self.transfer.clone()
  }

  #[getter]
  fn matrix(&self) -> Option<String> {
    self.matrix.clone()
  }

  #[getter]
  fn full_range(&self) -> Option<bool> {
    self.full_range
  }

  #[rename("toJSON")]
  fn to_json(&self, scope: &mut v8::PinScope) -> Js {
    let object = v8::Object::new(scope);
    put_color_string(scope, object, "primaries", &self.primaries);
    put_color_string(scope, object, "transfer", &self.transfer);
    put_color_string(scope, object, "matrix", &self.matrix);
    let full_range = match self.full_range {
      Some(value) => v8::Boolean::new(scope, value).into(),
      None => v8::null(scope).into(),
    };
    object.set(
      scope,
      v8::String::new(scope, "fullRange").unwrap().into(),
      full_range,
    );
    let value: v8::Local<v8::Value> = object.into();
    Js(v8::Global::new(scope, value))
  }
}

pub(crate) fn color_enum(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
  allowed: &[&str],
) -> Result<Option<String>, OpError> {
  let Some(value) = color_member(scope, object, name)? else {
    return Ok(None);
  };
  let captured = {
    v8::tc_scope!(let tc, scope);
    match value.to_string(tc) {
      Some(text) => Ok(text.to_rust_string_lossy(tc)),
      None => {
        let exception = tc
          .exception()
          .unwrap_or_else(|| v8::undefined(tc).into());
        Err(v8::Global::new(tc, exception))
      }
    }
  };
  let text = match captured {
    Ok(text) => text,
    Err(exception) => {
      let exception = v8::Local::new(scope, exception);
      return Err(OpError::thrown(scope, exception));
    }
  };
  if allowed.iter().any(|item| *item == text) {
    Ok(Some(text))
  } else {
    Err(OpError::type_(format!("The {name} is invalid")))
  }
}

pub(crate) fn color_bool(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
) -> Result<Option<bool>, OpError> {
  let Some(value) = color_member(scope, object, name)? else {
    return Ok(None);
  };
  Ok(Some(value.boolean_value(scope)))
}

fn color_member<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  object: v8::Local<'_, v8::Object>,
  name: &str,
) -> Result<Option<v8::Local<'s, v8::Value>>, OpError> {
  let captured = {
    v8::tc_scope!(let tc, scope);
    match object.get(tc, v8::String::new(tc, name).unwrap().into()) {
      Some(value) if value.is_null() || value.is_undefined() => Ok(None),
      Some(value) => Ok(Some(v8::Global::new(tc, value))),
      None => {
        let exception = tc
          .exception()
          .unwrap_or_else(|| v8::undefined(tc).into());
        Err(v8::Global::new(tc, exception))
      }
    }
  };
  match captured {
    Ok(Some(value)) => Ok(Some(v8::Local::new(scope, value))),
    Ok(None) => Ok(None),
    Err(exception) => {
      let exception = v8::Local::new(scope, exception);
      Err(OpError::thrown(scope, exception))
    }
  }
}

fn put_color_string(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
  value: &Option<String>,
) {
  let js = match value {
    Some(text) => v8::String::new(scope, text)
      .unwrap_or_else(|| v8::String::empty(scope))
      .into(),
    None => v8::null(scope).into(),
  };
  object.set(scope, v8::String::new(scope, name).unwrap().into(), js);
}

pub struct EncodedVideoChunk {
  key: bool,
  timestamp: i64,
  duration: Option<f64>,
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
      duration: parts.duration,
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
  fn duration(&self) -> Option<f64> {
    self.duration
  }

  #[getter]
  fn byte_length(&self) -> u32 {
    u32::try_from(self.data.len()).unwrap_or(u32::MAX)
  }

  #[length(1)]
  fn copy_to(
    &self,
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<(), OpError> {
    crate::frame::write_bytes(scope, args.get(0), &self.data)
  }
}

impl EncodedVideoChunk {
  pub(crate) fn copy_parts(&self) -> (bool, i64, Option<f64>, Vec<u8>) {
    (self.key, self.timestamp, self.duration, self.data.clone())
  }

  pub(crate) fn from_parts(
    key: bool,
    timestamp: i64,
    duration: Option<f64>,
    data: Vec<u8>,
  ) -> Self {
    Self {
      key,
      timestamp,
      duration,
      data,
    }
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

  #[static_method]
  #[promise]
  #[length(1)]
  fn is_config_supported(
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Js {
    match check_video(scope, args.get(0)) {
      Ok(checked) => {
        let config = video_support_object(scope, &checked);
        resolve_support(scope, checked.supported, config)
      }
      Err(error) => reject_support(scope, error),
    }
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
    #[this] this: v8::Local<v8::Object>,
    chunk: v8::Local<v8::Value>,
  ) -> Result<(), OpError> {
    self.ensure_configured()?;
    let (bytes, timestamp, duration) = video_chunk(scope, chunk)?;
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
          self.emit_frame(scope, this, raster, timestamp, duration)?;
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

  fn flush(
    &self,
    scope: &mut v8::PinScope,
    #[this] this: v8::Local<v8::Object>,
  ) -> Result<Js, OpError> {
    self.ensure_configured()?;
    let frames = {
      let mut guard = self.backend.borrow_mut();
      let Some(backend) = guard.as_mut() else {
        return Err(not_configured());
      };
      backend.flush().map_err(config_error)?
    };
    for raster in frames {
      self.emit_frame(scope, this, raster, 0.0, None)?;
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
    this: v8::Local<v8::Object>,
    raster: Raster,
    timestamp: f64,
    duration: Option<f64>,
  ) -> Result<(), OpError> {
    let Some(function) = callback(scope, &self.output) else {
      return Ok(());
    };
    let _ = VideoFrame::template(scope);
    let frame = unsafe {
      wrap_instance(
        scope,
        VideoFrame::from_data(
          frame_from_raster(&raster),
          timestamp,
          duration,
        ),
      )
    };
    let frame: v8::Local<v8::Value> = frame.into();
    enqueue_delivery(
      scope,
      Delivery::Output {
        kind: DecoderKind::Video,
        decoder: v8::Global::new(scope, this),
        callback: v8::Global::new(scope, function),
        argument: v8::Global::new(scope, frame),
      },
    );
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
    let callback =
      callback(scope, &self.error).map(|function| v8::Global::new(scope, function));
    enqueue_delivery(scope, Delivery::Error { callback, error });
  }
}

pub struct AudioData {
  body: RefCell<Option<crate::samples::AudioSamples>>,
  timestamp: Cell<f64>,
}

unsafe impl v8::cppgc::GarbageCollected for AudioData {
  fn trace(&self, _visitor: &mut v8::cppgc::Visitor) {}

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"AudioData"
  }
}

impl AudioData {
  pub(crate) fn from_stored(
    channels: Vec<Vec<f32>>,
    rate: f64,
    timestamp: f64,
  ) -> Self {
    Self::from_samples(crate::samples::AudioSamples::from_planar_f32(channels, rate), timestamp)
  }

  pub(crate) fn from_samples(
    samples: crate::samples::AudioSamples,
    timestamp: f64,
  ) -> Self {
    Self {
      body: RefCell::new(Some(samples)),
      timestamp: Cell::new(timestamp),
    }
  }

  pub(crate) fn samples(&self) -> Option<crate::samples::AudioSamples> {
    self.body.borrow().clone()
  }

  pub(crate) fn timestamp_us(&self) -> f64 {
    self.timestamp.get()
  }

  pub(crate) fn is_closed(&self) -> bool {
    self.body.borrow().is_none()
  }

  pub(crate) fn neuter(&self) {
    *self.body.borrow_mut() = None;
  }

  fn metric(&self, read: impl FnOnce(&crate::samples::AudioSamples) -> u32) -> u32 {
    self.body.borrow().as_ref().map(read).unwrap_or(0)
  }
}

#[op]
impl AudioData {
  #[constructor]
  #[length(1)]
  fn new(
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<Self, OpError> {
    let built = crate::pcm::construct(scope, args)?;
    Ok(Self::from_samples(built.samples, built.timestamp))
  }

  #[getter]
  fn format(&self) -> Option<String> {
    self.body.borrow().as_ref().map(|samples| samples.format_name())
  }

  #[getter]
  fn sample_rate(&self) -> f64 {
    self
      .body
      .borrow()
      .as_ref()
      .map(|samples| samples.rate)
      .unwrap_or(0.0)
  }

  #[getter]
  fn number_of_frames(&self) -> u32 {
    self.metric(|samples| samples.frames)
  }

  #[getter]
  fn number_of_channels(&self) -> u32 {
    self.metric(|samples| samples.channels)
  }

  #[getter]
  fn timestamp(&self) -> f64 {
    self.timestamp_us()
  }

  #[getter]
  fn duration(&self) -> f64 {
    self
      .body
      .borrow()
      .as_ref()
      .map(|samples| samples.duration_us())
      .unwrap_or(0.0)
  }

  fn close(&self) {
    self.neuter();
  }

  #[rename("clone")]
  fn clone_data(&self) -> Result<New<Self>, OpError> {
    let samples = self.samples().ok_or_else(closed_data)?;
    Ok(New(Self::from_samples(samples, self.timestamp_us())))
  }

  #[length(1)]
  fn allocation_size(
    &self,
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<u32, OpError> {
    if self.is_closed() {
      return Err(closed_data());
    }
    let request = crate::pcm::copy_request(scope, args, 0)?;
    let body = self.body.borrow();
    let samples = body.as_ref().ok_or_else(closed_data)?;
    samples
      .allocation_size(&request)
      .map_err(crate::pcm::copy_error)
  }

  #[length(2)]
  fn copy_to(
    &self,
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<(), OpError> {
    if self.is_closed() {
      return Err(closed_data());
    }
    let request = crate::pcm::copy_request(scope, args, 1)?;
    let bytes = {
      let body = self.body.borrow();
      let samples = body.as_ref().ok_or_else(closed_data)?;
      samples.copy_to(&request).map_err(crate::pcm::copy_error)?
    };
    crate::pcm::write_destination(scope, args.get(0), &bytes)
  }
}

pub struct EncodedAudioChunk {
  key: bool,
  timestamp: i64,
  duration: Option<f64>,
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
      duration: parts.duration,
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
  fn duration(&self) -> Option<f64> {
    self.duration
  }

  #[getter]
  fn byte_length(&self) -> u32 {
    u32::try_from(self.data.len()).unwrap_or(u32::MAX)
  }

  #[length(1)]
  fn copy_to(
    &self,
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<(), OpError> {
    crate::frame::write_bytes(scope, args.get(0), &self.data)
  }
}

impl EncodedAudioChunk {
  pub(crate) fn copy_parts(&self) -> (bool, i64, Option<f64>, Vec<u8>) {
    (
      self.key,
      self.timestamp,
      self.duration,
      self.data.clone(),
    )
  }

  pub(crate) fn from_parts(
    key: bool,
    timestamp: i64,
    duration: Option<f64>,
    data: Vec<u8>,
  ) -> Self {
    Self {
      key,
      timestamp,
      duration,
      data,
    }
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

  #[static_method]
  #[promise]
  #[length(1)]
  fn is_config_supported(
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Js {
    match check_audio(scope, args.get(0)) {
      Ok(checked) => {
        let config = audio_support_object(scope, &checked);
        resolve_support(scope, checked.supported, config)
      }
      Err(error) => reject_support(scope, error),
    }
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
    #[this] this: v8::Local<v8::Object>,
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
        self.emit_audio(scope, this, pcm.channels, pcm.rate, timestamp)
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
    this: v8::Local<v8::Object>,
    channels: Vec<Vec<f32>>,
    rate: f64,
    timestamp: f64,
  ) -> Result<(), OpError> {
    let Some(function) = callback(scope, &self.output) else {
      return Ok(());
    };
    let _ = AudioData::template(scope);
    let data = unsafe {
      wrap_instance(scope, AudioData::from_stored(channels, rate, timestamp))
    };
    let data: v8::Local<v8::Value> = data.into();
    enqueue_delivery(
      scope,
      Delivery::Output {
        kind: DecoderKind::Audio,
        decoder: v8::Global::new(scope, this),
        callback: v8::Global::new(scope, function),
        argument: v8::Global::new(scope, data),
      },
    );
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
    let callback =
      callback(scope, &self.error).map(|function| v8::Global::new(scope, function));
    enqueue_delivery(scope, Delivery::Error { callback, error });
  }
}

struct VideoChecked {
  supported: bool,
  codec: String,
  description: Option<Vec<u8>>,
  width: Option<u32>,
  height: Option<u32>,
  display_width: Option<u32>,
  display_height: Option<u32>,
  acceleration: Option<String>,
}

struct AudioChecked {
  supported: bool,
  codec: String,
  description: Option<Vec<u8>>,
  rate: u32,
  channels: u32,
}

fn video_config(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<VideoConfig, OpError> {
  let checked = check_video(scope, value)?;
  if !checked.supported {
    return Err(unsupported_codec());
  }
  Ok(VideoConfig {
    codec: Codec::Avc,
    description: checked.description.unwrap_or_default(),
    width: checked.width.unwrap_or(0),
    height: checked.height.unwrap_or(0),
  })
}

fn audio_config(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<(AudioCodec, Vec<u8>, u32, u32), OpError> {
  let checked = check_audio(scope, value)?;
  if !checked.supported {
    return Err(unsupported_codec());
  }
  Ok((
    AudioCodec::Aac,
    checked.description.unwrap_or_default(),
    checked.rate,
    checked.channels,
  ))
}

fn check_video(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<VideoChecked, OpError> {
  let object = dictionary(scope, value, "VideoDecoder configuration")?;
  let codec = required_codec(scope, object)?;
  let description = optional_buffer(scope, object, "description")?;
  let width = optional_positive(scope, object, "codedWidth")?;
  let height = optional_positive(scope, object, "codedHeight")?;
  let display_width = optional_positive(scope, object, "displayAspectWidth")?;
  let display_height = optional_positive(scope, object, "displayAspectHeight")?;
  let acceleration = match member(scope, object, "hardwareAcceleration") {
    Some(hint) => Some(acceleration_hint(scope, hint)?),
    None => None,
  };
  Ok(VideoChecked {
    supported: video_codec(&codec).is_some(),
    codec,
    description,
    width,
    height,
    display_width,
    display_height,
    acceleration,
  })
}

fn check_audio(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<AudioChecked, OpError> {
  let object = dictionary(scope, value, "AudioDecoder configuration")?;
  let codec = required_codec(scope, object)?;
  let rate = required_positive(scope, object, "sampleRate")?;
  let channels = required_positive(scope, object, "numberOfChannels")?;
  let description = optional_buffer(scope, object, "description")?;
  Ok(AudioChecked {
    supported: audio_codec(&codec).is_some(),
    codec,
    description,
    rate,
    channels,
  })
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
) -> Result<(Vec<u8>, f64, Option<f64>), OpError> {
  let object = v8::Local::<v8::Object>::try_from(value)
    .map_err(|_| OpError::type_("The chunk is not an EncodedVideoChunk"))?;
  let Some(host) = (unsafe { host_ref::<EncodedVideoChunk>(scope, object) })
  else {
    return Err(OpError::type_("The chunk is not an EncodedVideoChunk"));
  };
  let chunk = host.get();
  Ok((
    chunk.data.clone(),
    chunk.timestamp as f64,
    chunk.duration,
  ))
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
    duration: chunk_duration(scope, object)?,
    data,
  })
}

fn chunk_duration(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
) -> Result<Option<f64>, OpError> {
  let Some(value) = member(scope, object, "duration") else {
    return Ok(None);
  };
  if value.is_null() {
    return Ok(None);
  }
  let number = value
    .number_value(scope)
    .ok_or_else(|| OpError::type_("duration is not a number"))?;
  if !number.is_finite() || number < 0.0 || number > u64::MAX as f64 {
    return Err(OpError::type_("duration is out of range"));
  }
  Ok(Some(number.trunc()))
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

pub(crate) fn dictionary<'s>(
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

pub(crate) fn member<'s>(
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

fn optional_buffer(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
) -> Result<Option<Vec<u8>>, OpError> {
  let Some(value) = member(scope, object, name) else {
    return Ok(None);
  };
  if detached_buffer(scope, value) {
    return Err(OpError::type_(format!("{name} is detached")));
  }
  buffer_source(scope, value)
    .map(Some)
    .ok_or_else(|| OpError::type_(format!("{name} is not a BufferSource")))
}

fn detached_buffer(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> bool {
  if v8::Local::<v8::SharedArrayBuffer>::try_from(value).is_ok() {
    return false;
  }
  if let Ok(view) = v8::Local::<v8::ArrayBufferView>::try_from(value) {
    return view
      .buffer(scope)
      .is_some_and(|buffer| buffer.was_detached());
  }
  if let Ok(buffer) = v8::Local::<v8::ArrayBuffer>::try_from(value) {
    return buffer.was_detached();
  }
  false
}

fn required_codec(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
) -> Result<String, OpError> {
  let codec = required_string(scope, object, "codec")?;
  if codec.is_empty() {
    return Err(OpError::type_("codec is empty"));
  }
  Ok(codec)
}

fn optional_positive(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
) -> Result<Option<u32>, OpError> {
  let Some(value) = member(scope, object, name) else {
    return Ok(None);
  };
  let number = unsigned(scope, value, name)?;
  if number == 0 {
    return Err(OpError::type_(format!("{name} must be greater than 0")));
  }
  Ok(Some(number))
}

fn required_positive(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
) -> Result<u32, OpError> {
  let number = required_u32(scope, object, name)?;
  if number == 0 {
    return Err(OpError::type_(format!("{name} must be greater than 0")));
  }
  Ok(number)
}

pub(crate) fn unsigned(
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

pub(crate) fn long_long(
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

fn acceleration_hint(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<String, OpError> {
  let text = rust_string(scope, value);
  match text.as_str() {
    "no-preference" | "prefer-hardware" | "prefer-software" => Ok(text),
    _ => Err(OpError::type_("hardwareAcceleration is not supported")),
  }
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

fn video_support_object<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  checked: &VideoChecked,
) -> v8::Local<'s, v8::Object> {
  let object = v8::Object::new(scope);
  set_string(scope, object, "codec", &checked.codec);
  if let Some(bytes) = &checked.description {
    set_bytes(scope, object, "description", bytes);
  }
  if let Some(width) = checked.width {
    set_number(scope, object, "codedWidth", f64::from(width));
  }
  if let Some(height) = checked.height {
    set_number(scope, object, "codedHeight", f64::from(height));
  }
  if let Some(width) = checked.display_width {
    set_number(scope, object, "displayAspectWidth", f64::from(width));
  }
  if let Some(height) = checked.display_height {
    set_number(scope, object, "displayAspectHeight", f64::from(height));
  }
  if let Some(hint) = &checked.acceleration {
    set_string(scope, object, "hardwareAcceleration", hint);
  }
  object
}

fn audio_support_object<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  checked: &AudioChecked,
) -> v8::Local<'s, v8::Object> {
  let object = v8::Object::new(scope);
  set_string(scope, object, "codec", &checked.codec);
  set_number(scope, object, "sampleRate", f64::from(checked.rate));
  set_number(
    scope,
    object,
    "numberOfChannels",
    f64::from(checked.channels),
  );
  if let Some(bytes) = &checked.description {
    set_bytes(scope, object, "description", bytes);
  }
  object
}

fn resolve_support(
  scope: &mut v8::PinScope,
  supported: bool,
  config: v8::Local<v8::Object>,
) -> Js {
  let Some(resolver) = v8::PromiseResolver::new(scope) else {
    return Js::null(scope);
  };
  let object = v8::Object::new(scope);
  let flag: v8::Local<v8::Value> = v8::Boolean::new(scope, supported).into();
  set_value(scope, object, "supported", flag);
  set_value(scope, object, "config", config.into());
  resolver.resolve(scope, object.into());
  let promise: v8::Local<v8::Value> = resolver.get_promise(scope).into();
  Js(v8::Global::new(scope, promise))
}

fn reject_support(scope: &mut v8::PinScope, error: OpError) -> Js {
  let Some(resolver) = v8::PromiseResolver::new(scope) else {
    return Js::null(scope);
  };
  let rejected = support_error(scope, error);
  resolver.reject(scope, rejected);
  let promise: v8::Local<v8::Value> = resolver.get_promise(scope).into();
  Js(v8::Global::new(scope, promise))
}

fn support_error<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  error: OpError,
) -> v8::Local<'s, v8::Value> {
  match error {
    OpError::Type(message) => builtin_exception(scope, &message, false),
    OpError::Range(message) => builtin_exception(scope, &message, true),
    OpError::Dom { name, message } => new_dom_exception(scope, &name, &message)
      .map(v8::Local::<v8::Value>::from)
      .unwrap_or_else(|| v8::undefined(scope).into()),
    OpError::Thrown(value) => v8::Local::new(scope, *value),
  }
}

fn builtin_exception<'s>(
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

fn set_string(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
  value: &str,
) {
  let Some(text) = v8::String::new(scope, value) else {
    return;
  };
  set_value(scope, object, name, text.into());
}

fn set_number(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
  value: f64,
) {
  let number: v8::Local<v8::Value> = v8::Number::new(scope, value).into();
  set_value(scope, object, name, number);
}

fn set_bytes(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
  bytes: &[u8],
) {
  let buffer = array_buffer(scope, bytes.to_vec());
  set_value(scope, object, name, buffer.into());
}

fn set_value(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
  value: v8::Local<v8::Value>,
) {
  let Some(key) = v8::String::new(scope, name) else {
    return;
  };
  object.set(scope, key.into(), value);
}

enum DecoderKind {
  Video,
  Audio,
}

enum Delivery {
  Output {
    kind: DecoderKind,
    decoder: v8::Global<v8::Object>,
    callback: v8::Global<v8::Function>,
    argument: v8::Global<v8::Value>,
  },
  Error {
    callback: Option<v8::Global<v8::Function>>,
    error: v8::Global<v8::Value>,
  },
}

thread_local! {
  static DELIVERIES: RefCell<Vec<Delivery>> = const { RefCell::new(Vec::new()) };
}

/// `output` and `error` run after `decode` returns, in the order they were
/// queued. A `flush` reaction queued after that call therefore sees them.
fn enqueue_delivery(scope: &mut v8::PinScope, job: Delivery) {
  let Some(function) = v8::Function::new(scope, deliver_decoder) else {
    return;
  };
  DELIVERIES.with(|jobs| jobs.borrow_mut().push(job));
  scope.enqueue_microtask(function);
}

fn deliver_decoder(
  scope: &mut v8::PinScope,
  _args: v8::FunctionCallbackArguments,
  mut _ret: v8::ReturnValue,
) {
  let Some(job) = DELIVERIES.with(|jobs| {
    let mut jobs = jobs.borrow_mut();
    if jobs.is_empty() {
      None
    } else {
      Some(jobs.remove(0))
    }
  }) else {
    return;
  };
  match job {
    Delivery::Output {
      kind,
      decoder,
      callback,
      argument,
    } => {
      if let Some(thrown) = call_decoder(scope, callback, argument) {
        fail_thrown(scope, kind, &decoder, thrown);
      }
    }
    Delivery::Error { callback, error } => match callback {
      Some(callback) => {
        if let Some(thrown) = call_decoder(scope, callback, error) {
          let thrown = v8::Local::new(scope, &thrown);
          yuzuko_dom::report_error(scope, thrown);
        }
      }
      None => {
        let error = v8::Local::new(scope, &error);
        yuzuko_dom::report_error(scope, error);
      }
    },
  }
}

fn call_decoder(
  scope: &mut v8::PinScope,
  function: v8::Global<v8::Function>,
  argument: v8::Global<v8::Value>,
) -> Option<v8::Global<v8::Value>> {
  let receiver = yuzuko_core::global(scope);
  v8::tc_scope!(let scope, scope);
  let function = v8::Local::new(scope, &function);
  let argument = v8::Local::new(scope, &argument);
  let receiver = v8::Local::new(scope, receiver);
  if function
    .call(scope, receiver.into(), &[argument.into()])
    .is_some()
  {
    None
  } else {
    scope.exception().map(|value| v8::Global::new(scope, value))
  }
}

fn fail_thrown(
  scope: &mut v8::PinScope,
  kind: DecoderKind,
  decoder: &v8::Global<v8::Object>,
  thrown: v8::Global<v8::Value>,
) {
  let object = v8::Local::new(scope, decoder);
  match kind {
    DecoderKind::Video => {
      let Some(host) = (unsafe { host_ref::<VideoDecoder>(scope, object) })
      else {
        return;
      };
      host.get().fail_value(scope, thrown);
    }
    DecoderKind::Audio => {
      let Some(host) = (unsafe { host_ref::<AudioDecoder>(scope, object) })
      else {
        return;
      };
      host.get().fail_value(scope, thrown);
    }
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

pub(crate) fn closed_frame() -> OpError {
  OpError::dom("InvalidStateError", "The frame is closed")
}

fn closed_data() -> OpError {
  OpError::dom("InvalidStateError", "The audio data is closed")
}
