//! WebCodecs classes on `globalThis`.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::panic::{AssertUnwindSafe, catch_unwind};

use yuzuko_core::op::{
  Js, New, OpError, host_ref, new_dom_exception, traced, traced_local,
  wrap_instance,
};
use yuzuko_core::rust_string;
use yuzuko_dom::EventTarget;
use yuzuko_image::{FramePixels, Raster};
use yuzuko_macros::op;
use yuzuko_winter::{array_buffer, buffer_source};

use crate::audio::{AudioBackend, AudioCodec};
use crate::backend::{Acceleration, Codec, Picture, VideoBackend, VideoConfig};
use crate::error::DecodeError;
use crate::layout::{FrameColor, FrameData, frame_from_av1, frame_from_raster};

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
  pub(crate) fn from_timed(
    raster: Raster,
    timestamp: f64,
    duration: Option<f64>,
  ) -> Self {
    Self::from_data(frame_from_raster(&raster), timestamp, duration)
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
    let borrowed = self.body.borrow();
    let data = borrowed.as_ref()?;
    if data.rotation == 0 && !data.flip {
      return Some(data.raster.clone());
    }
    Some(
      yuzuko_image::orient_display(&data.raster, data.rotation, data.flip)
        .unwrap_or_else(|_| data.raster.clone()),
    )
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
  fn rotation(&self) -> u32 {
    self.metric(|data| u32::from(data.rotation))
  }

  #[getter]
  fn flip(&self) -> bool {
    self.body.borrow().as_ref().is_some_and(|data| data.flip)
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

  /// The VideoFrame Metadata Registry has no members this runtime records.
  fn metadata(&self, scope: &mut v8::PinScope) -> Js {
    let object: v8::Local<v8::Value> = v8::Object::new(scope).into();
    Js(v8::Global::new(scope, object))
  }

  #[promise]
  #[length(1)]
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
        let exception =
          tc.exception().unwrap_or_else(|| v8::undefined(tc).into());
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
        let exception =
          tc.exception().unwrap_or_else(|| v8::undefined(tc).into());
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

struct QueuedChunk {
  bytes: Vec<u8>,
  timestamp: f64,
  duration: Option<f64>,
}

enum Control {
  Decode(QueuedChunk),
  Flush(u64),
}

struct FlushWait {
  id: u64,
  resolver: v8::TracedReference<v8::PromiseResolver>,
}

/// Kept when an unsupported `configure` closes the decoder. A later `flush`
/// rejects with this error. `close` and `reset` take it. A decode error does
/// not store one, so a flush after that close rejects `InvalidStateError`.
#[derive(Clone)]
struct StoredError {
  name: &'static str,
  message: String,
}

#[repr(C)]
pub struct VideoDecoder {
  base: EventTarget,
  ondequeue: RefCell<Option<v8::TracedReference<v8::Value>>>,
  state: Cell<State>,
  output: RefCell<Option<v8::TracedReference<v8::Function>>>,
  error: RefCell<Option<v8::TracedReference<v8::Function>>>,
  backend: RefCell<Option<VideoBackend>>,
  /// Timestamp and duration of each decoded chunk whose frame has not come
  /// out yet. A hardware decoder may hold a frame until `flush`, and it
  /// outputs frames in presentation order, so each frame takes the earliest.
  pending: RefCell<Vec<(f64, Option<f64>)>>,
  queue: RefCell<VecDeque<Control>>,
  queue_size: Cell<u32>,
  /// True until a key chunk has been queued for this configuration.
  key_required: Cell<bool>,
  flushes: RefCell<VecDeque<FlushWait>>,
  flush_ids: Cell<u64>,
  /// Bumps when `reset`, `close`, `configure`, or a decode error drops work
  /// that a running pump must not finish.
  generation: Cell<u64>,
  scheduled: Cell<bool>,
  close_error: RefCell<Option<StoredError>>,
  /// `VideoDecoderConfig.rotation`, applied to each output frame.
  rotation: Cell<u16>,
  /// `VideoDecoderConfig.flip`, applied after `rotation`.
  flip: Cell<bool>,
}

unsafe impl v8::cppgc::GarbageCollected for VideoDecoder {
  fn trace(&self, visitor: &mut v8::cppgc::Visitor) {
    v8::cppgc::GarbageCollected::trace(&self.base, visitor);
    if let Ok(slot) = self.ondequeue.try_borrow() {
      visitor.trace(&*slot);
    }
    trace_fn(&self.output, visitor);
    trace_fn(&self.error, visitor);
    if let Ok(flushes) = self.flushes.try_borrow() {
      for flush in flushes.iter() {
        visitor.trace(&flush.resolver);
      }
    }
  }

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"VideoDecoder"
  }
}

thread_local! {
  static VIDEO_PUMPS: RefCell<VecDeque<v8::Global<v8::Object>>> =
    const { RefCell::new(VecDeque::new()) };
}

#[op(inherit = EventTarget)]
impl VideoDecoder {
  #[constructor]
  #[length(1)]
  fn new(
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<Self, OpError> {
    let (output, error) = decoder_init(scope, args.get(0))?;
    Ok(Self {
      base: EventTarget::new(),
      ondequeue: RefCell::new(None),
      state: Cell::new(State::Unconfigured),
      output: RefCell::new(Some(traced(scope, output))),
      error: RefCell::new(Some(traced(scope, error))),
      backend: RefCell::new(None),
      pending: RefCell::new(Vec::new()),
      queue: RefCell::new(VecDeque::new()),
      queue_size: Cell::new(0),
      key_required: Cell::new(true),
      flushes: RefCell::new(VecDeque::new()),
      flush_ids: Cell::new(0),
      generation: Cell::new(0),
      scheduled: Cell::new(false),
      close_error: RefCell::new(None),
      rotation: Cell::new(0),
      flip: Cell::new(false),
    })
  }

  #[getter]
  fn state(&self) -> String {
    self.state.get().name().to_string()
  }

  #[getter]
  fn decode_queue_size(&self) -> u32 {
    self.queue_size.get()
  }

  #[getter]
  fn ondequeue(&self, scope: &mut v8::PinScope) -> Js {
    event_handler_value(scope, &self.ondequeue)
  }

  #[setter]
  fn ondequeue(&self, scope: &mut v8::PinScope, value: v8::Local<v8::Value>) {
    set_event_handler(scope, &self.base, &self.ondequeue, "dequeue", value);
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
    #[this] this: v8::Local<v8::Object>,
    config: v8::Local<v8::Value>,
  ) -> Result<(), OpError> {
    self.ensure_open()?;
    let checked = check_video(scope, config)?;
    self.generation.set(self.generation.get().wrapping_add(1));
    self.abort_work(scope, this);
    if self.state.get() == State::Closed {
      return Err(decoder_closed());
    }
    self.close_error.borrow_mut().take();
    self.key_required.set(true);
    *self.backend.borrow_mut() = None;
    let Some(config) = video_open_config(&checked) else {
      self.reject_config(
        scope,
        "NotSupportedError",
        "The codec configuration is not supported".to_string(),
      );
      return Ok(());
    };
    let opened = catch_unwind(AssertUnwindSafe(|| VideoBackend::open(config)));
    match opened {
      Ok(Ok(backend)) => {
        *self.backend.borrow_mut() = Some(backend);
        self.rotation.set(checked.rotation);
        self.flip.set(checked.flip);
        self.state.set(State::Configured);
        Ok(())
      }
      Ok(Err(DecodeError::Panic)) | Err(_) => {
        self.reset_backend();
        Err(panicked())
      }
      Ok(Err(error)) => {
        let (name, message) = config_exception(error);
        self.reject_config(scope, name, message);
        Ok(())
      }
    }
  }

  fn decode(
    &self,
    scope: &mut v8::PinScope,
    #[this] this: v8::Local<v8::Object>,
    chunk: v8::Local<v8::Value>,
  ) -> Result<(), OpError> {
    self.ensure_configured()?;
    let (key, bytes, timestamp, duration) = video_chunk(scope, chunk)?;
    if self.key_required.get() && !key {
      return Err(OpError::dom("DataError", "A key frame is required"));
    }
    if key {
      self.key_required.set(false);
    }
    self
      .queue
      .borrow_mut()
      .push_back(Control::Decode(QueuedChunk {
        bytes,
        timestamp,
        duration,
      }));
    self.queue_size.set(self.queue_size.get().saturating_add(1));
    self.schedule(scope, this);
    Ok(())
  }

  #[promise]
  fn flush(
    &self,
    scope: &mut v8::PinScope,
    #[this] this: v8::Local<v8::Object>,
  ) -> Result<Js, OpError> {
    if self.state.get() != State::Configured {
      return rejected_state(scope, self.state.get(), &self.close_error);
    }
    // WebCodecs sets [[key chunk required]] before the promise is returned.
    // Chunks already queued were accepted and stay queued.
    self.key_required.set(true);
    let Some(resolver) = v8::PromiseResolver::new(scope) else {
      return Err(OpError::type_("Promise is unavailable"));
    };
    let promise = resolver.get_promise(scope);
    let id = self.flush_ids.get().wrapping_add(1);
    self.flush_ids.set(id);
    self.flushes.borrow_mut().push_back(FlushWait {
      id,
      resolver: traced(scope, resolver),
    });
    self.queue.borrow_mut().push_back(Control::Flush(id));
    self.schedule(scope, this);
    let promise: v8::Local<v8::Value> = promise.into();
    Ok(Js(v8::Global::new(scope, promise)))
  }

  fn reset(
    &self,
    scope: &mut v8::PinScope,
    #[this] this: v8::Local<v8::Object>,
  ) -> Result<(), OpError> {
    self.ensure_open()?;
    self.close_error.borrow_mut().take();
    self.generation.set(self.generation.get().wrapping_add(1));
    self.abort_work(scope, this);
    if self.state.get() == State::Closed {
      return Err(decoder_closed());
    }
    self.reset_backend();
    self.key_required.set(true);
    Ok(())
  }

  fn close(
    &self,
    scope: &mut v8::PinScope,
    #[this] this: v8::Local<v8::Object>,
  ) -> Result<(), OpError> {
    self.ensure_open()?;
    self.close_error.borrow_mut().take();
    self.generation.set(self.generation.get().wrapping_add(1));
    self.abort_work(scope, this);
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
    match self.state.get() {
      State::Configured => Ok(()),
      State::Closed => Err(decoder_closed()),
      State::Unconfigured => Err(not_configured()),
    }
  }

  /// The earliest pending chunk timing, for the next frame out.
  fn next_timing(&self) -> (f64, Option<f64>) {
    let mut pending = self.pending.borrow_mut();
    let earliest = pending
      .iter()
      .enumerate()
      .min_by(|(_, left), (_, right)| left.0.total_cmp(&right.0))
      .map(|(index, _)| index);
    earliest.map_or((0.0, None), |index| pending.remove(index))
  }

  fn reset_backend(&self) {
    self.pending.borrow_mut().clear();
    *self.backend.borrow_mut() = None;
    if self.state.get() != State::Closed {
      self.state.set(State::Unconfigured);
    }
  }

  fn shutdown(&self) {
    self.pending.borrow_mut().clear();
    *self.backend.borrow_mut() = None;
    self.state.set(State::Closed);
  }

  /// Close now and report `error` on a microtask. `flush` in this turn
  /// rejects with the same error.
  fn reject_config(
    &self,
    scope: &mut v8::PinScope,
    name: &'static str,
    message: String,
  ) {
    *self.close_error.borrow_mut() = Some(StoredError {
      name,
      message: message.clone(),
    });
    self.shutdown();
    queue_config_error(scope, &self.error, name, &message);
  }

  fn schedule(&self, scope: &mut v8::PinScope, this: v8::Local<v8::Object>) {
    if self.scheduled.get() {
      return;
    }
    let Some(function) = v8::Function::new(scope, pump_video) else {
      return;
    };
    self.scheduled.set(true);
    VIDEO_PUMPS.with(|pumps| {
      pumps.borrow_mut().push_back(v8::Global::new(scope, this));
    });
    scope.enqueue_microtask(function);
  }

  /// Drop queued chunks and reject every in-flight `flush` with `AbortError`.
  fn abort_work(&self, scope: &mut v8::PinScope, this: v8::Local<v8::Object>) {
    let waits = std::mem::take(&mut *self.flushes.borrow_mut());
    self.queue.borrow_mut().clear();
    let queued = self.queue_size.get();
    self.queue_size.set(0);
    self.pending.borrow_mut().clear();
    if let Ok(error) = dom_value(scope, "AbortError", "The flush was aborted") {
      for wait in waits {
        reject_resolver(scope, wait.resolver, error);
      }
    }
    if queued > 0 {
      self.fire_ondequeue(scope, this);
    }
  }

  fn shrink_queue(
    &self,
    scope: &mut v8::PinScope,
    this: v8::Local<v8::Object>,
  ) {
    let queued = self.queue_size.get();
    if queued == 0 {
      return;
    }
    self.queue_size.set(queued - 1);
    self.fire_ondequeue(scope, this);
  }

  fn fire_ondequeue(
    &self,
    scope: &mut v8::PinScope,
    this: v8::Local<v8::Object>,
  ) {
    yuzuko_dom::dispatch_trusted_event(scope, this, "dequeue");
  }

  fn process(&self, scope: &mut v8::PinScope, this: v8::Local<v8::Object>) {
    loop {
      if self.state.get() == State::Closed {
        return;
      }
      let message = self.queue.borrow_mut().pop_front();
      let Some(message) = message else {
        return;
      };
      let generation = self.generation.get();
      match message {
        Control::Decode(chunk) => {
          self.shrink_queue(scope, this);
          if self.generation.get() != generation
            || self.state.get() != State::Configured
          {
            return;
          }
          if !self.decode_chunk(scope, this, chunk) {
            return;
          }
        }
        Control::Flush(id) => {
          if !self.finish_flush(scope, this, id) {
            return;
          }
        }
      }
    }
  }

  /// `false` stops the pump. A decode error has already closed the decoder.
  fn decode_chunk(
    &self,
    scope: &mut v8::PinScope,
    this: v8::Local<v8::Object>,
    chunk: QueuedChunk,
  ) -> bool {
    match self.pull_decode(&chunk.bytes) {
      Ok(frames) => {
        self
          .pending
          .borrow_mut()
          .push((chunk.timestamp, chunk.duration));
        for picture in frames {
          let (timestamp, duration) = self.next_timing();
          if !self.emit_now(scope, this, picture, timestamp, duration) {
            return false;
          }
        }
        true
      }
      Err(error) => {
        self.fail_decode(scope, this, &error);
        false
      }
    }
  }

  /// `false` stops the pump, including when `reset` ran inside `output`.
  fn finish_flush(
    &self,
    scope: &mut v8::PinScope,
    this: v8::Local<v8::Object>,
    id: u64,
  ) -> bool {
    if !self.flush_live(id) {
      return true;
    }
    let generation = self.generation.get();
    let frames = match self.pull_flush() {
      Ok(frames) => frames,
      Err(error) => {
        self.fail_decode(scope, this, &error);
        return false;
      }
    };
    for picture in frames {
      let (timestamp, duration) = self.next_timing();
      if !self.emit_now(scope, this, picture, timestamp, duration) {
        return false;
      }
    }
    if self.generation.get() != generation || !self.flush_live(id) {
      return false;
    }
    self.pending.borrow_mut().clear();
    self.resolve_flush(scope, id);
    true
  }

  fn flush_live(&self, id: u64) -> bool {
    self.flushes.borrow().iter().any(|wait| wait.id == id)
  }

  fn resolve_flush(&self, scope: &mut v8::PinScope, id: u64) {
    let wait = {
      let mut flushes = self.flushes.borrow_mut();
      let Some(position) = flushes.iter().position(|wait| wait.id == id) else {
        return;
      };
      let Some(wait) = flushes.remove(position) else {
        return;
      };
      wait
    };
    let Some(resolver) = traced_local(scope, &wait.resolver) else {
      return;
    };
    let value: v8::Local<v8::Value> = v8::undefined(scope).into();
    resolver.resolve(scope, value);
  }

  fn pull_decode(&self, bytes: &[u8]) -> Result<Vec<Picture>, DecodeError> {
    let mut guard = self.backend.borrow_mut();
    let Some(backend) = guard.as_mut() else {
      return Err(DecodeError::Rejected);
    };
    match catch_unwind(AssertUnwindSafe(|| backend.decode(bytes))) {
      Ok(result) => result,
      Err(_) => Err(DecodeError::Panic),
    }
  }

  fn pull_flush(&self) -> Result<Vec<Picture>, DecodeError> {
    let mut guard = self.backend.borrow_mut();
    let Some(backend) = guard.as_mut() else {
      return Err(DecodeError::Rejected);
    };
    match catch_unwind(AssertUnwindSafe(|| backend.flush())) {
      Ok(result) => result,
      Err(_) => Err(DecodeError::Panic),
    }
  }

  /// Calls `output` now, so `reset` inside it can drop the rest of the queue.
  /// `false` means the decoder was reset, closed, or the callback threw.
  fn emit_now(
    &self,
    scope: &mut v8::PinScope,
    this: v8::Local<v8::Object>,
    picture: Picture,
    timestamp: f64,
    duration: Option<f64>,
  ) -> bool {
    let generation = self.generation.get();
    let Some(function) = callback(scope, &self.output) else {
      return self.still_configured(generation);
    };
    let _ = VideoFrame::template(scope);
    let mut data = match picture {
      Picture::Raster(raster) => frame_from_raster(&raster),
      Picture::Av1(picture) => frame_from_av1(picture),
    };
    apply_orientation(&mut data, self.rotation.get(), self.flip.get());
    let frame = unsafe {
      wrap_instance(scope, VideoFrame::from_data(data, timestamp, duration))
    };
    let frame: v8::Local<v8::Value> = frame.into();
    let thrown = call_decoder(
      scope,
      v8::Global::new(scope, function),
      v8::Global::new(scope, frame),
    );
    if let Some(thrown) = thrown {
      self.fail_closed(scope, thrown, Some(this));
      return false;
    }
    self.still_configured(generation)
  }

  fn still_configured(&self, generation: u64) -> bool {
    self.generation.get() == generation && self.state.get() == State::Configured
  }

  fn fail_decode(
    &self,
    scope: &mut v8::PinScope,
    this: v8::Local<v8::Object>,
    error: &DecodeError,
  ) {
    let (name, message) = match error {
      DecodeError::Panic => {
        ("InvalidStateError", "The decoder panicked".to_string())
      }
      DecodeError::Unsupported | DecodeError::Rejected => (
        "EncodingError",
        "The frame could not be decoded".to_string(),
      ),
      #[cfg(target_os = "macos")]
      DecodeError::Hardware(status) => (
        "EncodingError",
        format!(
          "The frame could not be decoded (VideoToolbox status {status})"
        ),
      ),
    };
    let Ok(value) = dom_value(scope, name, &message) else {
      self.shutdown();
      return;
    };
    self.fail_closed(scope, v8::Global::new(scope, value), Some(this));
  }

  fn fail_closed(
    &self,
    scope: &mut v8::PinScope,
    error: v8::Global<v8::Value>,
    this: Option<v8::Local<v8::Object>>,
  ) {
    self.generation.set(self.generation.get().wrapping_add(1));
    let waits = std::mem::take(&mut *self.flushes.borrow_mut());
    self.queue.borrow_mut().clear();
    let queued = self.queue_size.get();
    self.queue_size.set(0);
    self.close_error.borrow_mut().take();
    self.shutdown();
    let error_local = v8::Local::new(scope, &error);
    if let Some(function) = callback(scope, &self.error) {
      if let Some(thrown) = call_decoder(
        scope,
        v8::Global::new(scope, function),
        v8::Global::new(scope, error_local),
      ) {
        let thrown = v8::Local::new(scope, thrown);
        yuzuko_dom::report_error(scope, thrown);
      }
    } else {
      yuzuko_dom::report_error(scope, error_local);
    }
    let error_local = v8::Local::new(scope, &error);
    for wait in waits {
      reject_resolver(scope, wait.resolver, error_local);
    }
    if queued > 0
      && let Some(this) = this
    {
      self.fire_ondequeue(scope, this);
    }
  }
}

fn event_handler_value(
  scope: &mut v8::PinScope,
  slot: &RefCell<Option<v8::TracedReference<v8::Value>>>,
) -> Js {
  let value = slot
    .borrow()
    .as_ref()
    .and_then(|reference| traced_local(scope, reference))
    .unwrap_or_else(|| v8::null(scope).into());
  Js(v8::Global::new(scope, value))
}

fn set_event_handler(
  scope: &mut v8::PinScope,
  target: &EventTarget,
  slot: &RefCell<Option<v8::TracedReference<v8::Value>>>,
  type_name: &str,
  value: v8::Local<v8::Value>,
) {
  if let Some(previous) = slot.borrow().as_ref()
    && let Some(previous) = traced_local(scope, previous)
  {
    target.remove_callback(scope, type_name, previous, false);
  }
  if value.is_function() {
    target.add_callback(scope, type_name, value, false, false, false);
    *slot.borrow_mut() = Some(traced(scope, value));
  } else {
    *slot.borrow_mut() = None;
  }
}

fn pump_video(
  scope: &mut v8::PinScope,
  _args: v8::FunctionCallbackArguments,
  mut _return: v8::ReturnValue,
) {
  let Some(global) = VIDEO_PUMPS.with(|pumps| {
    let mut pumps = pumps.borrow_mut();
    if pumps.is_empty() {
      None
    } else {
      Some(pumps.pop_front().expect("pump"))
    }
  }) else {
    return;
  };
  let object = v8::Local::new(scope, &global);
  let Some(host) = (unsafe { host_ref::<VideoDecoder>(scope, object) }) else {
    return;
  };
  let decoder = host.get();
  decoder.scheduled.set(false);
  decoder.process(scope, object);
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
    Self::from_samples(
      crate::samples::AudioSamples::from_planar_f32(channels, rate),
      timestamp,
    )
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

  fn metric(
    &self,
    read: impl FnOnce(&crate::samples::AudioSamples) -> u32,
  ) -> u32 {
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
    self
      .body
      .borrow()
      .as_ref()
      .map(|samples| samples.format_name())
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

struct AudioQueued {
  bytes: Vec<u8>,
  timestamp: f64,
}

enum AudioControl {
  Decode(AudioQueued),
  Flush(u64),
}

#[repr(C)]
pub struct AudioDecoder {
  base: EventTarget,
  ondequeue: RefCell<Option<v8::TracedReference<v8::Value>>>,
  state: Cell<State>,
  output: RefCell<Option<v8::TracedReference<v8::Function>>>,
  error: RefCell<Option<v8::TracedReference<v8::Function>>>,
  backend: RefCell<Option<AudioBackend>>,
  queue: RefCell<VecDeque<AudioControl>>,
  queue_size: Cell<u32>,
  flushes: RefCell<VecDeque<FlushWait>>,
  flush_ids: Cell<u64>,
  generation: Cell<u64>,
  scheduled: Cell<bool>,
  close_error: RefCell<Option<StoredError>>,
}

unsafe impl v8::cppgc::GarbageCollected for AudioDecoder {
  fn trace(&self, visitor: &mut v8::cppgc::Visitor) {
    v8::cppgc::GarbageCollected::trace(&self.base, visitor);
    if let Ok(slot) = self.ondequeue.try_borrow() {
      visitor.trace(&*slot);
    }
    trace_fn(&self.output, visitor);
    trace_fn(&self.error, visitor);
    if let Ok(flushes) = self.flushes.try_borrow() {
      for flush in flushes.iter() {
        visitor.trace(&flush.resolver);
      }
    }
  }

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"AudioDecoder"
  }
}

thread_local! {
  static AUDIO_PUMPS: RefCell<VecDeque<v8::Global<v8::Object>>> =
    const { RefCell::new(VecDeque::new()) };
}

#[op(inherit = EventTarget)]
impl AudioDecoder {
  #[constructor]
  #[length(1)]
  fn new(
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<Self, OpError> {
    let (output, error) = decoder_init(scope, args.get(0))?;
    Ok(Self {
      base: EventTarget::new(),
      ondequeue: RefCell::new(None),
      state: Cell::new(State::Unconfigured),
      output: RefCell::new(Some(traced(scope, output))),
      error: RefCell::new(Some(traced(scope, error))),
      backend: RefCell::new(None),
      queue: RefCell::new(VecDeque::new()),
      queue_size: Cell::new(0),
      flushes: RefCell::new(VecDeque::new()),
      flush_ids: Cell::new(0),
      generation: Cell::new(0),
      scheduled: Cell::new(false),
      close_error: RefCell::new(None),
    })
  }

  #[getter]
  fn state(&self) -> String {
    self.state.get().name().to_string()
  }

  #[getter]
  fn decode_queue_size(&self) -> u32 {
    self.queue_size.get()
  }

  #[getter]
  fn ondequeue(&self, scope: &mut v8::PinScope) -> Js {
    event_handler_value(scope, &self.ondequeue)
  }

  #[setter]
  fn ondequeue(&self, scope: &mut v8::PinScope, value: v8::Local<v8::Value>) {
    set_event_handler(scope, &self.base, &self.ondequeue, "dequeue", value);
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
    #[this] this: v8::Local<v8::Object>,
    config: v8::Local<v8::Value>,
  ) -> Result<(), OpError> {
    self.ensure_open()?;
    let checked = check_audio(scope, config)?;
    self.generation.set(self.generation.get().wrapping_add(1));
    self.abort_work(scope, this);
    if self.state.get() == State::Closed {
      return Err(decoder_closed());
    }
    self.close_error.borrow_mut().take();
    *self.backend.borrow_mut() = None;
    let Some(codec) = audio_codec(&checked.codec) else {
      self.reject_config(
        scope,
        "NotSupportedError",
        "The codec configuration is not supported".to_string(),
      );
      return Ok(());
    };
    let description = checked.description.clone().unwrap_or_default();
    let opened = catch_unwind(AssertUnwindSafe(|| {
      AudioBackend::open(codec, &description, checked.rate, checked.channels)
    }));
    match opened {
      Ok(Ok(backend)) => {
        *self.backend.borrow_mut() = Some(backend);
        self.state.set(State::Configured);
        Ok(())
      }
      Ok(Err(DecodeError::Panic)) | Err(_) => {
        self.reset_backend();
        Err(panicked())
      }
      Ok(Err(error)) => {
        let (name, message) = config_exception(error);
        self.reject_config(scope, name, message);
        Ok(())
      }
    }
  }

  fn decode(
    &self,
    scope: &mut v8::PinScope,
    #[this] this: v8::Local<v8::Object>,
    chunk: v8::Local<v8::Value>,
  ) -> Result<(), OpError> {
    self.ensure_configured()?;
    let (bytes, timestamp) = audio_chunk(scope, chunk)?;
    self
      .queue
      .borrow_mut()
      .push_back(AudioControl::Decode(AudioQueued { bytes, timestamp }));
    self.queue_size.set(self.queue_size.get().saturating_add(1));
    self.schedule(scope, this);
    Ok(())
  }

  #[promise]
  fn flush(
    &self,
    scope: &mut v8::PinScope,
    #[this] this: v8::Local<v8::Object>,
  ) -> Result<Js, OpError> {
    if self.state.get() != State::Configured {
      return rejected_state(scope, self.state.get(), &self.close_error);
    }
    let Some(resolver) = v8::PromiseResolver::new(scope) else {
      return Err(OpError::type_("Promise is unavailable"));
    };
    let promise = resolver.get_promise(scope);
    let id = self.flush_ids.get().wrapping_add(1);
    self.flush_ids.set(id);
    self.flushes.borrow_mut().push_back(FlushWait {
      id,
      resolver: traced(scope, resolver),
    });
    self.queue.borrow_mut().push_back(AudioControl::Flush(id));
    self.schedule(scope, this);
    let promise: v8::Local<v8::Value> = promise.into();
    Ok(Js(v8::Global::new(scope, promise)))
  }

  fn reset(
    &self,
    scope: &mut v8::PinScope,
    #[this] this: v8::Local<v8::Object>,
  ) -> Result<(), OpError> {
    self.ensure_open()?;
    self.close_error.borrow_mut().take();
    self.generation.set(self.generation.get().wrapping_add(1));
    self.abort_work(scope, this);
    if self.state.get() == State::Closed {
      return Err(decoder_closed());
    }
    self.reset_backend();
    Ok(())
  }

  fn close(
    &self,
    scope: &mut v8::PinScope,
    #[this] this: v8::Local<v8::Object>,
  ) -> Result<(), OpError> {
    self.ensure_open()?;
    self.close_error.borrow_mut().take();
    self.generation.set(self.generation.get().wrapping_add(1));
    self.abort_work(scope, this);
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
    match self.state.get() {
      State::Configured => Ok(()),
      State::Closed => Err(decoder_closed()),
      State::Unconfigured => Err(not_configured()),
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

  fn reject_config(
    &self,
    scope: &mut v8::PinScope,
    name: &'static str,
    message: String,
  ) {
    *self.close_error.borrow_mut() = Some(StoredError {
      name,
      message: message.clone(),
    });
    self.shutdown();
    queue_config_error(scope, &self.error, name, &message);
  }

  fn schedule(&self, scope: &mut v8::PinScope, this: v8::Local<v8::Object>) {
    if self.scheduled.get() {
      return;
    }
    let Some(function) = v8::Function::new(scope, pump_audio) else {
      return;
    };
    self.scheduled.set(true);
    AUDIO_PUMPS.with(|pumps| {
      pumps.borrow_mut().push_back(v8::Global::new(scope, this));
    });
    scope.enqueue_microtask(function);
  }

  fn abort_work(&self, scope: &mut v8::PinScope, this: v8::Local<v8::Object>) {
    let waits = std::mem::take(&mut *self.flushes.borrow_mut());
    self.queue.borrow_mut().clear();
    let queued = self.queue_size.get();
    self.queue_size.set(0);
    if let Ok(error) = dom_value(scope, "AbortError", "The flush was aborted") {
      for wait in waits {
        reject_resolver(scope, wait.resolver, error);
      }
    }
    if queued > 0 {
      self.fire_ondequeue(scope, this);
    }
  }

  fn shrink_queue(
    &self,
    scope: &mut v8::PinScope,
    this: v8::Local<v8::Object>,
  ) {
    let queued = self.queue_size.get();
    if queued == 0 {
      return;
    }
    self.queue_size.set(queued - 1);
    self.fire_ondequeue(scope, this);
  }

  fn fire_ondequeue(
    &self,
    scope: &mut v8::PinScope,
    this: v8::Local<v8::Object>,
  ) {
    yuzuko_dom::dispatch_trusted_event(scope, this, "dequeue");
  }

  fn process(&self, scope: &mut v8::PinScope, this: v8::Local<v8::Object>) {
    loop {
      if self.state.get() == State::Closed {
        return;
      }
      let message = self.queue.borrow_mut().pop_front();
      let Some(message) = message else {
        return;
      };
      let generation = self.generation.get();
      match message {
        AudioControl::Decode(chunk) => {
          self.shrink_queue(scope, this);
          if self.generation.get() != generation
            || self.state.get() != State::Configured
          {
            return;
          }
          if !self.decode_chunk(scope, this, chunk) {
            return;
          }
        }
        AudioControl::Flush(id) => {
          if self.flush_live(id)
            && let Some(backend) = self.backend.borrow_mut().as_mut()
          {
            backend.flush();
          }
          if !self.finish_flush(scope, id) {
            return;
          }
        }
      }
    }
  }

  /// `false` stops the pump. A decode error has already closed the decoder.
  fn decode_chunk(
    &self,
    scope: &mut v8::PinScope,
    this: v8::Local<v8::Object>,
    chunk: AudioQueued,
  ) -> bool {
    match self.pull_decode(&chunk.bytes, chunk.timestamp) {
      Ok(Some(pcm)) => self.emit_now(scope, this, pcm),
      Ok(None) => true,
      Err(error) => {
        self.fail_decode(scope, this, &error);
        false
      }
    }
  }

  /// `false` stops the pump, including when `reset` ran inside `output`.
  fn finish_flush(&self, scope: &mut v8::PinScope, id: u64) -> bool {
    if self.flush_live(id) {
      self.resolve_flush(scope, id);
    }
    true
  }

  fn flush_live(&self, id: u64) -> bool {
    self.flushes.borrow().iter().any(|wait| wait.id == id)
  }

  fn resolve_flush(&self, scope: &mut v8::PinScope, id: u64) {
    let wait = {
      let mut flushes = self.flushes.borrow_mut();
      let Some(position) = flushes.iter().position(|wait| wait.id == id) else {
        return;
      };
      let Some(wait) = flushes.remove(position) else {
        return;
      };
      wait
    };
    let Some(resolver) = traced_local(scope, &wait.resolver) else {
      return;
    };
    let value: v8::Local<v8::Value> = v8::undefined(scope).into();
    resolver.resolve(scope, value);
  }

  fn pull_decode(
    &self,
    bytes: &[u8],
    timestamp: f64,
  ) -> Result<Option<crate::audio::Pcm>, DecodeError> {
    let mut guard = self.backend.borrow_mut();
    let Some(backend) = guard.as_mut() else {
      return Err(DecodeError::Rejected);
    };
    match catch_unwind(AssertUnwindSafe(|| backend.decode(bytes, timestamp))) {
      Ok(result) => result,
      Err(_) => Err(DecodeError::Panic),
    }
  }

  /// Calls `output` now, so `reset` inside it can reject an in-flight flush.
  /// `false` means the decoder was reset, closed, or the callback threw.
  fn emit_now(
    &self,
    scope: &mut v8::PinScope,
    this: v8::Local<v8::Object>,
    pcm: crate::audio::Pcm,
  ) -> bool {
    let generation = self.generation.get();
    let Some(function) = callback(scope, &self.output) else {
      return self.still_configured(generation);
    };
    let _ = AudioData::template(scope);
    let data = unsafe {
      wrap_instance(
        scope,
        AudioData::from_stored(pcm.channels, pcm.rate, pcm.timestamp),
      )
    };
    let data: v8::Local<v8::Value> = data.into();
    let thrown = call_decoder(
      scope,
      v8::Global::new(scope, function),
      v8::Global::new(scope, data),
    );
    if let Some(thrown) = thrown {
      self.fail_closed(scope, thrown, Some(this));
      return false;
    }
    self.still_configured(generation)
  }

  fn still_configured(&self, generation: u64) -> bool {
    self.generation.get() == generation && self.state.get() == State::Configured
  }

  fn fail_decode(
    &self,
    scope: &mut v8::PinScope,
    this: v8::Local<v8::Object>,
    error: &DecodeError,
  ) {
    let (name, message) = match error {
      DecodeError::Panic => {
        ("InvalidStateError", "The decoder panicked".to_string())
      }
      _ => (
        "EncodingError",
        "The audio could not be decoded".to_string(),
      ),
    };
    let Ok(value) = dom_value(scope, name, &message) else {
      self.shutdown();
      return;
    };
    self.fail_closed(scope, v8::Global::new(scope, value), Some(this));
  }

  fn fail_closed(
    &self,
    scope: &mut v8::PinScope,
    error: v8::Global<v8::Value>,
    this: Option<v8::Local<v8::Object>>,
  ) {
    self.generation.set(self.generation.get().wrapping_add(1));
    let waits = std::mem::take(&mut *self.flushes.borrow_mut());
    self.queue.borrow_mut().clear();
    let queued = self.queue_size.get();
    self.queue_size.set(0);
    self.close_error.borrow_mut().take();
    self.shutdown();
    let error_local = v8::Local::new(scope, &error);
    if let Some(function) = callback(scope, &self.error) {
      if let Some(thrown) = call_decoder(
        scope,
        v8::Global::new(scope, function),
        v8::Global::new(scope, error_local),
      ) {
        let thrown = v8::Local::new(scope, thrown);
        yuzuko_dom::report_error(scope, thrown);
      }
    } else {
      yuzuko_dom::report_error(scope, error_local);
    }
    let error_local = v8::Local::new(scope, &error);
    for wait in waits {
      reject_resolver(scope, wait.resolver, error_local);
    }
    if queued > 0
      && let Some(this) = this
    {
      self.fire_ondequeue(scope, this);
    }
  }
}

fn pump_audio(
  scope: &mut v8::PinScope,
  _args: v8::FunctionCallbackArguments,
  mut _return: v8::ReturnValue,
) {
  let Some(global) = AUDIO_PUMPS.with(|pumps| {
    let mut pumps = pumps.borrow_mut();
    if pumps.is_empty() {
      None
    } else {
      Some(pumps.pop_front().expect("pump"))
    }
  }) else {
    return;
  };
  let object = v8::Local::new(scope, &global);
  let Some(host) = (unsafe { host_ref::<AudioDecoder>(scope, object) }) else {
    return;
  };
  let decoder = host.get();
  decoder.scheduled.set(false);
  decoder.process(scope, object);
}

struct VideoColorInit {
  primaries: Option<String>,
  transfer: Option<String>,
  matrix: Option<String>,
  full_range: Option<bool>,
}

struct VideoChecked {
  supported: bool,
  kind: Option<Codec>,
  codec: String,
  description: Option<Vec<u8>>,
  width: Option<u32>,
  height: Option<u32>,
  display_width: Option<u32>,
  display_height: Option<u32>,
  acceleration: Option<String>,
  color_space: Option<VideoColorInit>,
  rotation: u16,
  flip: bool,
}

struct AudioChecked {
  supported: bool,
  codec: String,
  description: Option<Vec<u8>>,
  rate: u32,
  channels: u32,
}

fn video_open_config(checked: &VideoChecked) -> Option<VideoConfig> {
  let codec = checked.kind.filter(|_| checked.supported)?;
  Some(VideoConfig {
    codec,
    description: checked.description.clone().unwrap_or_default(),
    width: checked.width.unwrap_or(0),
    height: checked.height.unwrap_or(0),
    acceleration: match checked.acceleration.as_deref() {
      Some("prefer-hardware") => Acceleration::PreferHardware,
      Some("prefer-software") => Acceleration::PreferSoftware,
      _ => Acceleration::NoPreference,
    },
  })
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
  let color_space = optional_color_space(scope, object)?;
  let rotation = config_rotation(scope, object)?;
  let flip = member(scope, object, "flip")
    .is_some_and(|value| value.boolean_value(scope));
  let kind = video_codec(&codec);
  let supported = match kind {
    Some(Codec::Avc) => crate::h264::codec_string_supported(&codec),
    Some(Codec::Av1) => {
      crate::av1::description_supported(description.as_deref())
    }
    None => false,
  };
  Ok(VideoChecked {
    supported,
    kind,
    codec,
    description,
    width,
    height,
    display_width,
    display_height,
    acceleration,
    color_space,
    rotation,
    flip,
  })
}

/// `[EnforceRange] unsigned short`, then a quarter turn.
fn config_rotation(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
) -> Result<u16, OpError> {
  let Some(value) = member(scope, object, "rotation") else {
    return Ok(0);
  };
  let number = value
    .number_value(scope)
    .ok_or_else(|| OpError::type_("rotation is out of range"))?;
  if !number.is_finite() {
    return Err(OpError::type_("rotation is out of range"));
  }
  let truncated = number.trunc();
  if !(0.0..=65535.0).contains(&truncated) {
    return Err(OpError::type_("rotation is out of range"));
  }
  match truncated as u16 {
    quarter @ (0 | 90 | 180 | 270) => Ok(quarter),
    _ => Err(OpError::type_("rotation must be 0, 90, 180, or 270")),
  }
}

fn apply_orientation(data: &mut FrameData, rotation: u16, flip: bool) {
  if rotation == 90 || rotation == 270 {
    std::mem::swap(&mut data.display_width, &mut data.display_height);
  }
  data.rotation = rotation;
  data.flip = flip;
}

/// `colorSpace` on a `VideoDecoderConfig`. Omitted members stay null.
fn optional_color_space(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
) -> Result<Option<VideoColorInit>, OpError> {
  let Some(value) = member(scope, object, "colorSpace") else {
    return Ok(None);
  };
  if value.is_null() {
    return Ok(None);
  }
  let color = dictionary(scope, value, "colorSpace")?;
  Ok(Some(VideoColorInit {
    primaries: color_enum(scope, color, "primaries", COLOR_PRIMARIES)?,
    transfer: color_enum(scope, color, "transfer", COLOR_TRANSFER)?,
    matrix: color_enum(scope, color, "matrix", COLOR_MATRIX)?,
    full_range: color_bool(scope, color, "fullRange")?,
  }))
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
  } else if crate::av1::string_supported(codec) {
    Some(Codec::Av1)
  } else {
    None
  }
}

fn audio_codec(codec: &str) -> Option<AudioCodec> {
  if crate::audio::aac_supported(codec) {
    Some(AudioCodec::Aac)
  } else if codec == "opus" {
    Some(AudioCodec::Opus)
  } else if codec == "vorbis" {
    Some(AudioCodec::Vorbis)
  } else {
    None
  }
}

fn video_chunk(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<(bool, Vec<u8>, f64, Option<f64>), OpError> {
  let object = v8::Local::<v8::Object>::try_from(value)
    .map_err(|_| OpError::type_("The chunk is not an EncodedVideoChunk"))?;
  let Some(host) = (unsafe { host_ref::<EncodedVideoChunk>(scope, object) })
  else {
    return Err(OpError::type_("The chunk is not an EncodedVideoChunk"));
  };
  let chunk = host.get();
  Ok((
    chunk.key,
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
  set_number(scope, object, "rotation", f64::from(checked.rotation));
  set_value(
    scope,
    object,
    "flip",
    v8::Boolean::new(scope, checked.flip).into(),
  );
  if let Some(color) = &checked.color_space {
    let space = v8::Object::new(scope);
    put_color_string(scope, space, "primaries", &color.primaries);
    put_color_string(scope, space, "transfer", &color.transfer);
    put_color_string(scope, space, "matrix", &color.matrix);
    put_color_bool(scope, space, "fullRange", color.full_range);
    set_value(scope, object, "colorSpace", space.into());
  }
  object
}

fn put_color_bool(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
  value: Option<bool>,
) {
  let js = match value {
    Some(flag) => v8::Boolean::new(scope, flag).into(),
    None => v8::null(scope).into(),
  };
  let Some(key) = v8::String::new(scope, name) else {
    return;
  };
  object.set(scope, key.into(), js);
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
    v8::Exception::range_error(scope, text)
  } else {
    v8::Exception::type_error(scope, text)
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

struct Delivery {
  callback: Option<v8::Global<v8::Function>>,
  error: v8::Global<v8::Value>,
}

thread_local! {
  static DELIVERIES: RefCell<Vec<Delivery>> = const { RefCell::new(Vec::new()) };
}

/// Queues `error` for the next microtask, ahead of a `flush` reaction that
/// the caller attaches after `configure` returns.
fn enqueue_delivery(scope: &mut v8::PinScope, job: Delivery) {
  let Some(function) = v8::Function::new(scope, deliver_decoder) else {
    return;
  };
  DELIVERIES.with(|jobs| jobs.borrow_mut().push(job));
  scope.enqueue_microtask(function);
}

fn queue_config_error(
  scope: &mut v8::PinScope,
  slot: &RefCell<Option<v8::TracedReference<v8::Function>>>,
  name: &str,
  message: &str,
) {
  let Ok(error) = dom_value(scope, name, message) else {
    return;
  };
  let callback =
    callback(scope, slot).map(|function| v8::Global::new(scope, function));
  enqueue_delivery(
    scope,
    Delivery {
      callback,
      error: v8::Global::new(scope, error),
    },
  );
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
  match job.callback {
    Some(callback) => {
      if let Some(thrown) = call_decoder(scope, callback, job.error) {
        let thrown = v8::Local::new(scope, &thrown);
        yuzuko_dom::report_error(scope, thrown);
      }
    }
    None => {
      let error = v8::Local::new(scope, &job.error);
      yuzuko_dom::report_error(scope, error);
    }
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
  if function.call(scope, receiver.into(), &[argument]).is_some() {
    None
  } else {
    scope.exception().map(|value| v8::Global::new(scope, value))
  }
}

fn dom_value<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  name: &str,
  message: &str,
) -> Result<v8::Local<'s, v8::Value>, OpError> {
  new_dom_exception(scope, name, message)
    .map(v8::Local::<v8::Value>::from)
    .ok_or_else(|| OpError::type_("DOMException is unavailable"))
}

fn rejected_promise(
  scope: &mut v8::PinScope,
  error: v8::Local<v8::Value>,
) -> Result<Js, OpError> {
  let Some(resolver) = v8::PromiseResolver::new(scope) else {
    return Err(OpError::type_("Promise is unavailable"));
  };
  let promise = resolver.get_promise(scope);
  resolver.reject(scope, error);
  let promise: v8::Local<v8::Value> = promise.into();
  Ok(Js(v8::Global::new(scope, promise)))
}

fn reject_resolver(
  scope: &mut v8::PinScope,
  reference: v8::TracedReference<v8::PromiseResolver>,
  value: v8::Local<v8::Value>,
) {
  let Some(resolver) = traced_local(scope, &reference) else {
    return;
  };
  let resolver = yuzuko_core::op::root(scope, resolver);
  let value = yuzuko_core::op::root(scope, value);
  v8::tc_scope!(let scope, scope);
  resolver.reject(scope, value);
}

fn rejected_state(
  scope: &mut v8::PinScope,
  state: State,
  close_error: &RefCell<Option<StoredError>>,
) -> Result<Js, OpError> {
  if let Some(stored) = close_error.borrow().clone() {
    let error = dom_value(scope, stored.name, &stored.message)?;
    return rejected_promise(scope, error);
  }
  let message = if state == State::Closed {
    "The decoder is closed"
  } else {
    "The decoder is not configured"
  };
  let error = dom_value(scope, "InvalidStateError", message)?;
  rejected_promise(scope, error)
}

fn config_exception(error: DecodeError) -> (&'static str, String) {
  match error {
    DecodeError::Panic => {
      ("InvalidStateError", "The decoder panicked".to_string())
    }
    #[cfg(target_os = "macos")]
    DecodeError::Hardware(status) => (
      "NotSupportedError",
      format!(
        "The codec configuration is not supported (VideoToolbox status {status})"
      ),
    ),
    DecodeError::Unsupported | DecodeError::Rejected => (
      "NotSupportedError",
      "The codec configuration is not supported".to_string(),
    ),
  }
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
