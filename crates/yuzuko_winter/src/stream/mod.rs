//! WHATWG streams used by fetch bodies, blobs, text, and compression.
//!
//! Byte streams do not implement BYOB readers yet.

#![allow(clippy::too_many_arguments)]

use std::cell::RefCell;
use std::collections::HashMap;
use std::collections::HashSet;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};

use flate2::Compression;
use flate2::write::DeflateDecoder;
use flate2::write::DeflateEncoder;
use flate2::write::GzDecoder;
use flate2::write::GzEncoder;
use flate2::write::ZlibDecoder;
use flate2::write::ZlibEncoder;
use yuzuko_core::op::{Js, OpError};
use yuzuko_macros::op;

use crate::bytes;

mod byte_stream;

const DEFAULT_HWM: f64 = 1.0;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
  Readable,
  Closed,
  Errored,
  /// Writable only. `start` has not settled, so `closed` stays pending.
  Erroring,
}

struct Chunk {
  value: v8::Global<v8::Value>,
  size: f64,
}

struct ReadState {
  queue: VecDeque<Chunk>,
  queued: f64,
  high_water: f64,
  phase: Phase,
  error: Option<v8::Global<v8::Value>>,
  reader: Option<u64>,
  close_requested: bool,
  disturbed: bool,
  pulling: bool,
  pull_again: bool,
  started: bool,
  pull_fn: Option<v8::Global<v8::Function>>,
  cancel_fn: Option<v8::Global<v8::Function>>,
  size_fn: Option<v8::Global<v8::Function>>,
  /// Bytes still on disk or in shared chunks. Pull reads the next piece.
  native: Option<NativeBody>,
  /// Writable side when this readable is the readable side of a transform.
  paired: Option<u64>,
}

struct NativeBody {
  source: crate::blob::BlobSource,
  cursor: u64,
}

struct ReaderState {
  stream: u64,
  released: bool,
  byob: bool,
  requests: VecDeque<v8::Global<v8::PromiseResolver>>,
  into_requests: VecDeque<v8::Global<v8::PromiseResolver>>,
  closed: v8::Global<v8::Promise>,
  closed_resolver: v8::Global<v8::PromiseResolver>,
  settled: bool,
}

enum Sink {
  Js {
    write: Option<v8::Global<v8::Function>>,
    close: Option<v8::Global<v8::Function>>,
    abort: Option<v8::Global<v8::Function>>,
  },
  Transform {
    readable: u64,
  },
  Utf8Encode {
    pending: Option<u16>,
    readable: u64,
  },
  Utf8Decode {
    decoder: encoding_rs::Decoder,
    fatal: bool,
    readable: u64,
  },
  Codec {
    // zlib-rs decoders are much larger than the other sink states.
    codec: Box<Codec>,
    emitted: usize,
    readable: u64,
  },
}

enum Codec {
  GzipEnc(GzEncoder<Vec<u8>>),
  ZlibEnc(ZlibEncoder<Vec<u8>>),
  RawEnc(DeflateEncoder<Vec<u8>>),
  GzipDec(GzDecoder<Vec<u8>>),
  ZlibDec(ZlibDecoder<Vec<u8>>),
  RawDec(DeflateDecoder<Vec<u8>>),
  BrotliEnc(Box<brotli::CompressorWriter<Vec<u8>>>),
  BrotliDec(Box<brotli::DecompressorWriter<Vec<u8>>>),
  BrotliFinished(Vec<u8>),
}

struct WriteState {
  high_water: f64,
  queued: f64,
  phase: Phase,
  error: Option<v8::Global<v8::Value>>,
  writer: Option<u64>,
  started: bool,
  closing: bool,
  close_started: bool,
  sink: Sink,
  in_flight: u32,
  size_fn: Option<v8::Global<v8::Function>>,
  queue: VecDeque<QueuedWrite>,
  close_resolver: Option<v8::Global<v8::PromiseResolver>>,
  /// Rejected with the stored error once an `erroring` stream finishes.
  abort_wait: Option<v8::Global<v8::PromiseResolver>>,
  /// The promise returned by `abort`, shared by repeat calls.
  abort_promise: Option<v8::Global<v8::Promise>>,
  /// Present when sink `abort` must run after in-flight writes finish.
  abort_reason: Option<v8::Global<v8::Value>>,
  /// Internal `AbortController` for `WritableStreamDefaultController.signal`.
  abort_controller: Option<v8::Global<v8::Object>>,
}

struct WriterState {
  stream: u64,
  closed: v8::Global<v8::Promise>,
  closed_resolver: v8::Global<v8::PromiseResolver>,
  ready: v8::Global<v8::Promise>,
  ready_resolver: Option<v8::Global<v8::PromiseResolver>>,
  settled: bool,
  released: bool,
}

struct Inflight {
  stream: u64,
  resolver: v8::Global<v8::PromiseResolver>,
  size: f64,
}

struct AbortFlight {
  stream: u64,
  resolver: v8::Global<v8::PromiseResolver>,
}

struct QueuedWrite {
  chunk: v8::Global<v8::Value>,
  resolver: v8::Global<v8::PromiseResolver>,
  size: f64,
}

/// Backpressure shared by the two sides of one transform stream.
/// Keyed by the writable id.
struct TransformLink {
  readable: u64,
  backpressure: Option<bool>,
  change_resolver: Option<v8::Global<v8::PromiseResolver>>,
  change_promise: Option<v8::Global<v8::Promise>>,
  transform: Option<v8::Global<v8::Function>>,
  flush: Option<v8::Global<v8::Function>>,
  cancel: Option<v8::Global<v8::Function>>,
  cleared: bool,
  finish_resolver: Option<v8::Global<v8::PromiseResolver>>,
  finish_promise: Option<v8::Global<v8::Promise>>,
  /// Reason passed to a cancel or abort that owns `finish_promise`.
  finish_reason: Option<v8::Global<v8::Value>>,
  finish_kind: TransformFinish,
  /// The cancel algorithm errored the writable before it returned.
  cancel_errored: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TransformFinish {
  None,
  Flush,
  SourceCancel,
  SinkAbort,
}

struct TransformWait {
  writable: u64,
  chunk: v8::Global<v8::Value>,
  resolver: v8::Global<v8::PromiseResolver>,
}

struct TransformForward {
  writable: u64,
  resolver: v8::Global<v8::PromiseResolver>,
  /// When set, a rejection errors both sides before the guard rejects.
  guard: bool,
}

struct Tee {
  reader: u64,
  branches: [u64; 2],
  canceled: [bool; 2],
  reading: bool,
  /// Cancel reasons in branch order. The source is canceled once with both.
  reasons: [Option<v8::Global<v8::Value>>; 2],
  cancel_resolver: v8::Global<v8::PromiseResolver>,
  cancel_promise: v8::Global<v8::Promise>,
}

struct Pipe {
  readable: u64,
  writable: u64,
  reader: u64,
  writer: u64,
  prevent_close: bool,
  prevent_abort: bool,
  prevent_cancel: bool,
  resolver: v8::Global<v8::PromiseResolver>,
  settled: bool,
  shutting_down: bool,
  plan_started: bool,
  plan: PipePlan,
  latest_write: Option<v8::Global<v8::Promise>>,
  /// Real `AbortSignal`. Absent when `pipeTo` was called without one.
  signal: Option<v8::Global<v8::Object>>,
  abort_listener: Option<v8::Global<v8::Function>>,
  /// Outstanding abort and cancel actions for `PipePlan::Signal`.
  signal_left: u8,
  signal_error: Option<v8::Global<v8::Value>>,
  /// `PipePlan::Signal` already waited one microtask for an idle writable.
  signal_drained: bool,
}

enum PipePlan {
  Idle,
  Close,
  Succeed,
  Abort(v8::Global<v8::Value>),
  Cancel(v8::Global<v8::Value>),
  Fail(v8::Global<v8::Value>),
  /// Abort and/or cancel with `signal.reason`, then reject with that reason
  /// unless one of those actions rejects.
  Signal(v8::Global<v8::Value>),
}

struct BodyRead {
  reader: u64,
  output: Vec<u8>,
  resolver: v8::Global<v8::PromiseResolver>,
}

type BodyContinue =
  Box<dyn FnOnce(&mut v8::PinScope, Result<Vec<u8>, v8::Global<v8::Value>>)>;

enum Token {
  StartRead(u64),
  Pull(u64),

  StartWrite(u64),
  Write(u64),
  CloseWrite(u64),
  AbortWrite(u64),
  AbortForget(u64),
  TeeRead(u64),
  TeeClosed(u64),
  TeeKick(u64),
  TransformStart(u64),
  /// A writable write is waiting for transform backpressure to clear.
  TransformRelease(u64),
  /// `[[started]]` flips one turn after the transformer start promise.
  TransformReady(u64),
  /// Settles a promise that guards transform/flush/cancel completion.
  TransformForward(u64),
  /// Flush, source cancel, or sink abort finished.
  TransformFinish(u64),
  PipeRead(u64),
  PipeReady(u64),
  PipeWriteDone(u64),
  PipeSourceClosed(u64),
  PipeDestClosed(u64),
  PipeDeferredClose(u64),
  PipeDrain(u64),
  PipeAction(u64),
  PipeSignal(u64),
  BodyRead(u64),
  BodyWait(u64),
  CodecError(u64),
}

thread_local! {
  static READS: RefCell<HashMap<u64, ReadState>> =
    RefCell::new(HashMap::new());
  static READERS: RefCell<HashMap<u64, ReaderState>> =
    RefCell::new(HashMap::new());
  static WRITES: RefCell<HashMap<u64, WriteState>> =
    RefCell::new(HashMap::new());
  static WRITERS: RefCell<HashMap<u64, WriterState>> =
    RefCell::new(HashMap::new());
  static REACTIONS: RefCell<HashMap<u64, Token>> =
    RefCell::new(HashMap::new());
  static INFLIGHT: RefCell<HashMap<u64, Inflight>> =
    RefCell::new(HashMap::new());
  static TEES: RefCell<HashMap<u64, Tee>> = RefCell::new(HashMap::new());
  static TRANSFORMS: RefCell<HashMap<u64, TransformLink>> =
    RefCell::new(HashMap::new());
  static TRANSFORM_WAITS: RefCell<HashMap<u64, TransformWait>> =
    RefCell::new(HashMap::new());
  static TRANSFORM_FORWARDS: RefCell<HashMap<u64, TransformForward>> =
    RefCell::new(HashMap::new());
  /// `None` means the transformer start promise fulfilled.
  static TRANSFORM_STARTS: RefCell<HashMap<u64, Option<v8::Global<v8::Value>>>> =
    RefCell::new(HashMap::new());
  static PIPES: RefCell<HashMap<u64, Pipe>> = RefCell::new(HashMap::new());
  static ABORTS: RefCell<HashMap<u64, AbortFlight>> =
    RefCell::new(HashMap::new());
  static BODY_READS: RefCell<HashMap<u64, BodyRead>> =
    RefCell::new(HashMap::new());
  static BODY_WAITS: RefCell<HashMap<u64, BodyContinue>> =
    RefCell::new(HashMap::new());
  static ITERS: RefCell<HashMap<u64, IterGate>> = RefCell::new(HashMap::new());
  static OWNING: RefCell<HashSet<u64>> = RefCell::new(HashSet::new());
  static FUNCTION_BIND: RefCell<Option<v8::Global<v8::Function>>> =
    const { RefCell::new(None) };
}

static NEXT: AtomicU64 = AtomicU64::new(1);

fn alloc() -> u64 {
  NEXT.fetch_add(1, Ordering::Relaxed)
}

pub fn clear() {
  FUNCTION_BIND.with(|slot| *slot.borrow_mut() = None);
  READS.with(|slot| slot.borrow_mut().clear());
  READERS.with(|slot| slot.borrow_mut().clear());
  WRITES.with(|slot| slot.borrow_mut().clear());
  WRITERS.with(|slot| slot.borrow_mut().clear());
  REACTIONS.with(|slot| slot.borrow_mut().clear());
  INFLIGHT.with(|slot| slot.borrow_mut().clear());
  TEES.with(|slot| slot.borrow_mut().clear());
  PIPES.with(|slot| slot.borrow_mut().clear());
  ABORTS.with(|slot| slot.borrow_mut().clear());
  BODY_READS.with(|slot| slot.borrow_mut().clear());
  BODY_WAITS.with(|slot| slot.borrow_mut().clear());
  ITERS.with(|slot| slot.borrow_mut().clear());
  OWNING.with(|slot| slot.borrow_mut().clear());
  byte_stream::clear();
}

pub struct ReadableStream {
  id: u64,
}

unsafe impl v8::cppgc::GarbageCollected for ReadableStream {
  fn trace(&self, _visitor: &mut v8::cppgc::Visitor) {}

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"ReadableStream"
  }
}

pub struct ReadableStreamDefaultReader {
  id: u64,
}

unsafe impl v8::cppgc::GarbageCollected for ReadableStreamDefaultReader {
  fn trace(&self, _visitor: &mut v8::cppgc::Visitor) {}

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"ReadableStreamDefaultReader"
  }
}

pub struct ReadableStreamDefaultController {
  stream: u64,
}

unsafe impl v8::cppgc::GarbageCollected for ReadableStreamDefaultController {
  fn trace(&self, _visitor: &mut v8::cppgc::Visitor) {}

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"ReadableStreamDefaultController"
  }
}

pub struct WritableStream {
  id: u64,
}

unsafe impl v8::cppgc::GarbageCollected for WritableStream {
  fn trace(&self, _visitor: &mut v8::cppgc::Visitor) {}

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"WritableStream"
  }
}

pub struct WritableStreamDefaultWriter {
  id: u64,
}

unsafe impl v8::cppgc::GarbageCollected for WritableStreamDefaultWriter {
  fn trace(&self, _visitor: &mut v8::cppgc::Visitor) {}

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"WritableStreamDefaultWriter"
  }
}

pub struct WritableStreamDefaultController {
  stream: u64,
}

unsafe impl v8::cppgc::GarbageCollected for WritableStreamDefaultController {
  fn trace(&self, _visitor: &mut v8::cppgc::Visitor) {}

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"WritableStreamDefaultController"
  }
}

pub struct TransformStreamDefaultController {
  readable: u64,
  writable: u64,
}

unsafe impl v8::cppgc::GarbageCollected for TransformStreamDefaultController {
  fn trace(&self, _visitor: &mut v8::cppgc::Visitor) {}

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"TransformStreamDefaultController"
  }
}

pub struct TransformStream {
  readable: v8::TracedReference<v8::Object>,
  writable: v8::TracedReference<v8::Object>,
}

unsafe impl v8::cppgc::GarbageCollected for TransformStream {
  fn trace(&self, visitor: &mut v8::cppgc::Visitor) {
    visitor.trace(&self.readable);
    visitor.trace(&self.writable);
  }

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"TransformStream"
  }
}

pub struct CountQueuingStrategy {
  high_water: f64,
}

unsafe impl v8::cppgc::GarbageCollected for CountQueuingStrategy {
  fn trace(&self, _visitor: &mut v8::cppgc::Visitor) {}

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"CountQueuingStrategy"
  }
}

pub struct ByteLengthQueuingStrategy {
  high_water: f64,
}

unsafe impl v8::cppgc::GarbageCollected for ByteLengthQueuingStrategy {
  fn trace(&self, _visitor: &mut v8::cppgc::Visitor) {}

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"ByteLengthQueuingStrategy"
  }
}

pub struct TextEncoderStream {
  readable: v8::TracedReference<v8::Object>,
  writable: v8::TracedReference<v8::Object>,
}

unsafe impl v8::cppgc::GarbageCollected for TextEncoderStream {
  fn trace(&self, visitor: &mut v8::cppgc::Visitor) {
    visitor.trace(&self.readable);
    visitor.trace(&self.writable);
  }

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"TextEncoderStream"
  }
}

pub struct TextDecoderStream {
  readable: v8::TracedReference<v8::Object>,
  writable: v8::TracedReference<v8::Object>,
  encoding: String,
  fatal: bool,
  ignore_bom: bool,
}

unsafe impl v8::cppgc::GarbageCollected for TextDecoderStream {
  fn trace(&self, visitor: &mut v8::cppgc::Visitor) {
    visitor.trace(&self.readable);
    visitor.trace(&self.writable);
  }

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"TextDecoderStream"
  }
}

pub struct CompressionStream {
  readable: v8::TracedReference<v8::Object>,
  writable: v8::TracedReference<v8::Object>,
}

unsafe impl v8::cppgc::GarbageCollected for CompressionStream {
  fn trace(&self, visitor: &mut v8::cppgc::Visitor) {
    visitor.trace(&self.readable);
    visitor.trace(&self.writable);
  }

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"CompressionStream"
  }
}

pub struct DecompressionStream {
  readable: v8::TracedReference<v8::Object>,
  writable: v8::TracedReference<v8::Object>,
}

unsafe impl v8::cppgc::GarbageCollected for DecompressionStream {
  fn trace(&self, visitor: &mut v8::cppgc::Visitor) {
    visitor.trace(&self.readable);
    visitor.trace(&self.writable);
  }

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"DecompressionStream"
  }
}

#[op]
impl CountQueuingStrategy {
  #[constructor]
  fn new(
    scope: &mut v8::PinScope,
    init: v8::Local<v8::Value>,
  ) -> Result<Self, OpError> {
    let high_water = strategy_high_water(scope, init, true)?;
    Ok(Self { high_water })
  }

  #[getter]
  fn high_water_mark(&self) -> f64 {
    self.high_water
  }
}

#[op]
impl ByteLengthQueuingStrategy {
  #[constructor]
  fn new(
    scope: &mut v8::PinScope,
    init: v8::Local<v8::Value>,
  ) -> Result<Self, OpError> {
    let high_water = strategy_high_water(scope, init, true)?;
    Ok(Self { high_water })
  }

  #[getter]
  fn high_water_mark(&self) -> f64 {
    self.high_water
  }
}

#[op]
impl ReadableStream {
  #[constructor]
  fn new(
    scope: &mut v8::PinScope,
    source: Option<v8::Local<v8::Value>>,
    strategy: Option<v8::Local<v8::Value>>,
  ) -> Result<Self, OpError> {
    let (high_water, size_fn) = read_strategy(scope, strategy, DEFAULT_HWM)?;
    if source.as_ref().is_some_and(|value| value.is_null()) {
      return Err(OpError::type_("The underlying source is null"));
    }
    let source = source.filter(|value| !value.is_null_or_undefined());
    let kind = source_kind(scope, source)?;
    if kind == SourceKind::Bytes {
      return byte_stream::construct(scope, source, strategy);
    }
    let (start, pull, cancel) = source_methods(scope, source)?;
    let id = alloc();
    if kind == SourceKind::Owning {
      OWNING.with(|slot| slot.borrow_mut().insert(id));
    }
    READS.with(|slot| {
      slot.borrow_mut().insert(
        id,
        ReadState {
          queue: VecDeque::new(),
          queued: 0.0,
          high_water,
          phase: Phase::Readable,
          error: None,
          reader: None,
          close_requested: false,
          disturbed: false,
          pulling: false,
          pull_again: false,
          started: false,
          pull_fn: pull,
          cancel_fn: cancel,
          size_fn,
          native: None,
          paired: None,
        },
      );
    });
    let controller = unsafe {
      yuzuko_core::op::wrap_instance(
        scope,
        ReadableStreamDefaultController { stream: id },
      )
    };
    if let Some(start) = start {
      let controller: v8::Local<v8::Value> = controller.into();
      let controller = yuzuko_core::op::root(scope, controller);
      match call_method(scope, &start, &[controller]) {
        Ok(value) => {
          let _ = watch_promise(scope, value, Token::StartRead(id));
        }
        Err(error) => {
          let thrown = error_value(scope, &error);
          let _ = error_stream(scope, id, thrown);
          return Err(error);
        }
      }
    } else {
      let value = v8::undefined(scope).into();
      let _ = watch_promise(scope, value, Token::StartRead(id));
    }
    Ok(Self { id })
  }

  #[getter]
  fn locked(&self) -> bool {
    READS.with(|slot| {
      slot
        .borrow()
        .get(&self.id)
        .is_some_and(|state| state.reader.is_some())
    })
  }

  fn get_reader(
    &self,
    scope: &mut v8::PinScope,
    options: Option<v8::Local<v8::Value>>,
  ) -> Result<Js, OpError> {
    let mode = if let Some(options) = options {
      reader_mode(scope, options)?
    } else {
      ReaderMode::Default
    };
    if mode == ReaderMode::Byob {
      let reader = byte_stream::acquire_byob(scope, self.id)?;
      let object = unsafe { yuzuko_core::op::wrap_instance(scope, reader) };
      let value: v8::Local<v8::Value> = object.into();
      return Ok(Js(v8::Global::new(scope, value)));
    }
    let reader = acquire_reader(scope, self.id)?;
    let object = unsafe { yuzuko_core::op::wrap_instance(scope, reader) };
    let value: v8::Local<v8::Value> = object.into();
    Ok(Js(v8::Global::new(scope, value)))
  }

  #[promise]
  fn cancel(
    &self,
    scope: &mut v8::PinScope,
    reason: Option<v8::Local<v8::Value>>,
  ) -> Js {
    let locked = READS.with(|slot| {
      slot
        .borrow()
        .get(&self.id)
        .is_some_and(|state| state.reader.is_some())
    });
    if locked {
      let error = type_error(scope, "The stream is locked");
      return rejected(scope, error);
    }
    cancel_stream(scope, self.id, reason)
  }

  fn tee(&self, scope: &mut v8::PinScope) -> Result<Js, OpError> {
    let (branch_a, branch_b) = tee_pair(scope, self.id)?;
    let array = v8::Array::new(scope, 2);
    let left = v8::Local::new(scope, &branch_a);
    let right = v8::Local::new(scope, &branch_b);
    let left: v8::Local<v8::Value> = left.into();
    let right: v8::Local<v8::Value> = right.into();
    array.set_index(scope, 0, left);
    array.set_index(scope, 1, right);
    let value: v8::Local<v8::Value> = array.into();
    Ok(Js(v8::Global::new(scope, value)))
  }
}

fn empty_readable(
  scope: &mut v8::PinScope,
  cancel: Option<v8::Local<v8::Function>>,
) -> Result<v8::Global<v8::Object>, OpError> {
  let id = alloc();
  let cancel_fn = cancel.map(|function| v8::Global::new(scope, function));
  READS.with(|slot| {
    slot.borrow_mut().insert(
      id,
      ReadState {
        queue: VecDeque::new(),
        queued: 0.0,
        high_water: DEFAULT_HWM,
        phase: Phase::Readable,
        error: None,
        reader: None,
        close_requested: false,
        disturbed: false,
        pulling: false,
        pull_again: false,
        started: true,
        pull_fn: None,
        cancel_fn,
        size_fn: None,
        native: None,
        paired: None,
      },
    );
  });
  let object =
    unsafe { yuzuko_core::op::wrap_instance(scope, ReadableStream { id }) };
  Ok(v8::Global::new(scope, object))
}

pub fn stream_from_source(
  scope: &mut v8::PinScope,
  source: crate::blob::BlobSource,
) -> v8::Global<v8::Object> {
  let mut cursor = 0u64;
  let mut bytes = Vec::new();
  loop {
    match source.read_next(cursor, 64 * 1024) {
      Ok(Some(chunk)) => {
        cursor += chunk.len() as u64;
        bytes.extend_from_slice(&chunk);
        if cursor >= source.size() {
          break;
        }
      }
      Ok(None) => break,
      Err(_) => {
        let id = byte_stream::create_push();
        let error = type_error(scope, "The blob could not be read");
        byte_stream::fail(scope, id, error);
        return wrap_stream(scope, id);
      }
    }
  }
  stream_from_bytes(scope, bytes)
}

pub(crate) fn open_byte_stream(
  scope: &mut v8::PinScope,
) -> v8::Global<v8::Object> {
  wrap_stream(scope, byte_stream::create_push())
}

pub(crate) fn byte_stream_id(
  scope: &mut v8::PinScope,
  object: &v8::Global<v8::Object>,
) -> u64 {
  stream_id_of(scope, object).unwrap_or(0)
}

pub(crate) fn byte_stream_open(id: u64) -> bool {
  READS.with(|slot| {
    slot.borrow().get(&id).is_some_and(|state| {
      state.phase == Phase::Readable && !state.close_requested
    })
  })
}

pub(crate) fn push_byte_chunk(
  scope: &mut v8::PinScope,
  id: u64,
  bytes: Vec<u8>,
) -> bool {
  if !byte_stream_open(id) || bytes.is_empty() {
    return byte_stream_open(id);
  }
  if byte_stream::is_byte(id) {
    return byte_stream::push_bytes(scope, id, bytes);
  }
  let view = bytes::uint8_array(scope, bytes);
  enqueue_chunk(scope, id, view.into(), Some(1.0)).is_ok()
}

pub(crate) fn finish_byte_stream(scope: &mut v8::PinScope, id: u64) {
  if byte_stream::is_byte(id) {
    byte_stream::finish(scope, id);
    return;
  }
  let _ = close_stream(scope, id);
}

pub(crate) fn error_byte_stream(
  scope: &mut v8::PinScope,
  id: u64,
  error: v8::Local<v8::Value>,
) {
  if byte_stream::is_byte(id) {
    byte_stream::fail(scope, id, error);
    return;
  }
  let _ = error_stream(scope, id, error);
}

pub(crate) fn stream_error(id: u64) -> Option<v8::Global<v8::Value>> {
  READS.with(|slot| {
    slot
      .borrow()
      .get(&id)
      .filter(|state| state.phase == Phase::Errored)
      .and_then(|state| state.error.clone())
  })
}

pub(crate) fn cancel_stream_object(
  scope: &mut v8::PinScope,
  object: &v8::Global<v8::Object>,
  reason: v8::Local<v8::Value>,
) {
  let Some(id) = stream_id_of(scope, object) else {
    return;
  };
  let _ = cancel_stream(scope, id, Some(reason));
}

pub(crate) fn tee_pair(
  scope: &mut v8::PinScope,
  id: u64,
) -> Result<(v8::Global<v8::Object>, v8::Global<v8::Object>), OpError> {
  let locked = READS.with(|slot| {
    slot
      .borrow()
      .get(&id)
      .is_some_and(|state| state.reader.is_some())
  });
  if locked {
    return Err(OpError::type_("The stream is locked"));
  }
  if byte_stream::is_byte(id) {
    return byte_stream::tee(scope, id);
  }
  let reader = acquire_reader(scope, id)?;
  let tee = alloc();
  let cancel_resolver = v8::PromiseResolver::new(scope).unwrap();
  let cancel_promise = cancel_resolver.get_promise(scope);
  let cancel_a = tee_cancel_function(scope, tee, 0);
  let branch_a = empty_readable(scope, Some(cancel_a))?;
  let cancel_b = tee_cancel_function(scope, tee, 1);
  let branch_b = empty_readable(scope, Some(cancel_b))?;
  let id_a = readable_object_id(scope, &branch_a)?;
  let id_b = readable_object_id(scope, &branch_b)?;
  TEES.with(|slot| {
    slot.borrow_mut().insert(
      tee,
      Tee {
        reader: reader.id,
        branches: [id_a, id_b],
        canceled: [false, false],
        reading: false,
        reasons: [None, None],
        cancel_resolver: v8::Global::new(scope, cancel_resolver),
        cancel_promise: v8::Global::new(scope, cancel_promise),
      },
    );
  });
  // Pull on a later turn so `tee()` inside `size()` does not consume the
  // chunk this enqueue is about to queue.
  let kick = resolved(scope, v8::undefined(scope).into());
  let kick = js_value(scope, kick);
  let _ = watch_promise(scope, kick, Token::TeeKick(id_a));
  if let Some(closed) = READERS.with(|slot| {
    slot
      .borrow()
      .get(&reader.id)
      .map(|state| state.closed.clone())
  }) {
    let closed = v8::Local::new(scope, &closed);
    let closed: v8::Local<v8::Value> = closed.into();
    let closed = yuzuko_core::op::root(scope, closed);
    let _ = watch_promise(scope, closed, Token::TeeClosed(tee));
  }
  Ok((branch_a, branch_b))
}

/// UTF-8 text stream for `textStream()`. Buffered bodies become one string
/// chunk. An open body is decoded when its bytes arrive.
pub fn text_stream_from_body(
  scope: &mut v8::PinScope,
  body: v8::Local<v8::Object>,
) -> Result<v8::Global<v8::Object>, OpError> {
  let value: v8::Local<v8::Value> = body.into();
  let id = stream_id(scope, value)?;
  match read_stream_bytes(scope, id)? {
    StreamBody::Ready(bytes) => {
      Ok(stream_from_text(scope, &String::from_utf8_lossy(&bytes)))
    }
    StreamBody::Pending(promise) => {
      let (output, object) = open_value_stream(scope);
      when_stream_bytes(scope, promise, move |scope, result| match result {
        Ok(bytes) => {
          let text = String::from_utf8_lossy(&bytes);
          if !text.is_empty()
            && let Some(value) = v8::String::new(scope, &text)
          {
            let _ = enqueue_chunk(scope, output, value.into(), None);
          }
          let _ = close_stream(scope, output);
        }
        Err(error) => {
          let error = v8::Local::new(scope, &error);
          let _ = error_stream(scope, output, error);
        }
      });
      Ok(object)
    }
  }
}

fn stream_from_text(
  scope: &mut v8::PinScope,
  text: &str,
) -> v8::Global<v8::Object> {
  let (id, object) = open_value_stream(scope);
  if !text.is_empty()
    && let Some(value) = v8::String::new(scope, text)
  {
    let _ = enqueue_chunk(scope, id, value.into(), None);
  }
  let _ = close_stream(scope, id);
  object
}

fn open_value_stream(
  scope: &mut v8::PinScope,
) -> (u64, v8::Global<v8::Object>) {
  let id = alloc();
  READS.with(|slot| {
    slot.borrow_mut().insert(
      id,
      ReadState {
        queue: VecDeque::new(),
        queued: 0.0,
        high_water: DEFAULT_HWM,
        phase: Phase::Readable,
        error: None,
        reader: None,
        close_requested: false,
        disturbed: false,
        pulling: false,
        pull_again: false,
        started: true,
        pull_fn: None,
        cancel_fn: None,
        size_fn: None,
        native: None,
        paired: None,
      },
    );
  });
  let object =
    unsafe { yuzuko_core::op::wrap_instance(scope, ReadableStream { id }) };
  (id, v8::Global::new(scope, object))
}

pub fn stream_from_bytes(
  scope: &mut v8::PinScope,
  bytes: Vec<u8>,
) -> v8::Global<v8::Object> {
  let id = byte_stream::create_push();
  if !bytes.is_empty() {
    let _ = byte_stream::push_bytes(scope, id, bytes);
  }
  byte_stream::finish(scope, id);
  wrap_stream(scope, id)
}

fn wrap_stream(scope: &mut v8::PinScope, id: u64) -> v8::Global<v8::Object> {
  let object =
    unsafe { yuzuko_core::op::wrap_instance(scope, ReadableStream { id }) };
  v8::Global::new(scope, object)
}

pub fn stream_id_of(
  scope: &mut v8::PinScope,
  object: &v8::Global<v8::Object>,
) -> Option<u64> {
  readable_object_id(scope, object).ok()
}

pub fn is_disturbed(id: u64) -> bool {
  if id == 0 {
    return false;
  }
  READS.with(|slot| slot.borrow().get(&id).is_some_and(|state| state.disturbed))
}

pub fn is_locked(id: u64) -> bool {
  if id == 0 {
    return false;
  }
  READS.with(|slot| {
    slot
      .borrow()
      .get(&id)
      .is_some_and(|state| state.reader.is_some())
  })
}

pub enum StreamBody {
  Ready(Vec<u8>),
  Pending(v8::Global<v8::Promise>),
}

/// Copies a body that is already complete. An open stream is read until it
/// closes so later chunks are not discarded.
pub fn read_stream_bytes(
  scope: &mut v8::PinScope,
  id: u64,
) -> Result<StreamBody, OpError> {
  if let Some(error) = stream_error(id) {
    return Err(OpError::Thrown(Box::new(error)));
  }
  let ready = READS.with(|slot| {
    slot.borrow().get(&id).map(|state| {
      (
        state.reader.is_some(),
        state.pull_fn.is_none()
          && state.phase != Phase::Errored
          && (state.phase == Phase::Closed
            || state.close_requested
            || state.native.is_some()),
      )
    })
  });
  let Some((locked, buffered)) = ready else {
    return Err(OpError::type_("Body has already been used"));
  };
  if locked {
    return Err(OpError::type_("Body has already been used"));
  }
  if buffered {
    // Copy without a read promise. A poisoned `Object.prototype.then` must
    // not replace a chunk that is already queued. The reader stays locked.
    let mut bytes = byte_stream::take_queued_bytes(scope, id)?;
    bytes.extend(consume_buffered(scope, id)?);
    let _reader = acquire_reader(scope, id)?;
    return Ok(StreamBody::Ready(bytes));
  }
  Ok(StreamBody::Pending(start_body_read(scope, id)?))
}

fn start_body_read(
  scope: &mut v8::PinScope,
  id: u64,
) -> Result<v8::Global<v8::Promise>, OpError> {
  let reader = acquire_reader(scope, id)?;
  let resolver = v8::PromiseResolver::new(scope).unwrap();
  let promise = resolver.get_promise(scope);
  let job = alloc();
  BODY_READS.with(|slot| {
    slot.borrow_mut().insert(
      job,
      BodyRead {
        reader: reader.id,
        output: Vec::new(),
        resolver: v8::Global::new(scope, resolver),
      },
    );
  });
  pull_body(scope, job);
  Ok(v8::Global::new(scope, promise))
}

fn pull_body(scope: &mut v8::PinScope, job: u64) {
  let reader =
    BODY_READS.with(|slot| slot.borrow().get(&job).map(|state| state.reader));
  let Some(reader) = reader else {
    return;
  };
  let read = reader_read(scope, reader);
  let value = js_value(scope, read);
  let _ = watch_promise(scope, value, Token::BodyRead(job));
}

fn body_on_read(
  scope: &mut v8::PinScope,
  job: u64,
  result: Result<v8::Local<v8::Value>, v8::Local<v8::Value>>,
) {
  let value = match result {
    Ok(value) => value,
    Err(error) => {
      finish_body(scope, job, Err(error));
      return;
    }
  };
  let Some((done, chunk)) = read_record(scope, value) else {
    let error = type_error(scope, "The read result is not an object");
    finish_body(scope, job, Err(error));
    return;
  };
  if done {
    finish_body(scope, job, Ok(()));
    return;
  }
  // Fetch reads a default stream as a sequence of `Uint8Array` chunks.
  if v8::Local::<v8::Uint8Array>::try_from(chunk).is_err() {
    let error = type_error(scope, "The chunk is not a Uint8Array");
    finish_body(scope, job, Err(error));
    return;
  }
  if let Some(bytes) = bytes::buffer_source(scope, chunk) {
    BODY_READS.with(|slot| {
      if let Some(state) = slot.borrow_mut().get_mut(&job) {
        state.output.extend(bytes);
      }
    });
  }
  pull_body(scope, job);
}

fn finish_body(
  scope: &mut v8::PinScope,
  job: u64,
  result: Result<(), v8::Local<v8::Value>>,
) {
  let Some(state) = BODY_READS.with(|slot| slot.borrow_mut().remove(&job))
  else {
    return;
  };
  let BodyRead {
    reader,
    output,
    resolver,
  } = state;
  let resolver = v8::Local::new(scope, &resolver);
  let resolver = yuzuko_core::op::root(scope, resolver);
  match result {
    Ok(()) => {
      let view = bytes::uint8_array(scope, output);
      let value: v8::Local<v8::Value> = view.into();
      resolver.resolve(scope, value);
    }
    Err(error) => {
      resolver.reject(scope, error);
    }
  }
  let _ = release_reader(scope, reader);
}

pub fn when_stream_bytes(
  scope: &mut v8::PinScope,
  promise: v8::Global<v8::Promise>,
  finish: impl FnOnce(&mut v8::PinScope, Result<Vec<u8>, v8::Global<v8::Value>>)
  + 'static,
) {
  let id = alloc();
  BODY_WAITS.with(|slot| slot.borrow_mut().insert(id, Box::new(finish)));
  let local = v8::Local::new(scope, &promise);
  let _ = watch_promise(scope, local.into(), Token::BodyWait(id));
}

fn body_on_wait(
  scope: &mut v8::PinScope,
  id: u64,
  result: Result<v8::Local<v8::Value>, v8::Local<v8::Value>>,
) {
  let finish = BODY_WAITS.with(|slot| slot.borrow_mut().remove(&id));
  let Some(finish) = finish else {
    return;
  };
  match result {
    Ok(value) => {
      let bytes = bytes::buffer_source(scope, value).unwrap_or_default();
      finish(scope, Ok(bytes));
    }
    Err(error) => {
      let error = v8::Global::new(scope, error);
      finish(scope, Err(error));
    }
  }
}

pub fn consume_buffered(
  scope: &mut v8::PinScope,
  id: u64,
) -> Result<Vec<u8>, OpError> {
  let usable = READS.with(|slot| {
    slot.borrow().get(&id).map(|state| {
      !state.disturbed
        && state.reader.is_none()
        && state.phase != Phase::Errored
    })
  });
  if usable != Some(true) {
    return Err(OpError::type_("Body has already been used"));
  }
  let (chunks, native) = READS.with(|slot| {
    let mut states = slot.borrow_mut();
    let Some(state) = states.get_mut(&id) else {
      return (Vec::new(), None);
    };
    state.disturbed = true;
    state.phase = Phase::Closed;
    state.close_requested = true;
    (
      state.queue.drain(..).collect::<Vec<_>>(),
      state.native.take(),
    )
  });
  let mut output = Vec::new();
  for chunk in chunks {
    let value = v8::Local::new(scope, &chunk.value);
    let value = yuzuko_core::op::root(scope, value);
    // A closed body is copied without a read loop, so the Uint8Array
    // check has to happen here as well as in `body_on_read`.
    if v8::Local::<v8::Uint8Array>::try_from(value).is_err() {
      return Err(OpError::type_("The chunk is not a Uint8Array"));
    }
    if let Some(bytes) = bytes::buffer_source(scope, value) {
      output.extend(bytes);
    }
  }
  if let Some(native) = native {
    let mut cursor = native.cursor;
    loop {
      let next = native
        .source
        .read_next(cursor, 64 * 1024)
        .map_err(|_| OpError::type_("The body could not be read"))?;
      let Some(chunk) = next else {
        break;
      };
      cursor += chunk.len() as u64;
      output.extend_from_slice(&chunk);
    }
  }
  Ok(output)
}

#[op]
impl ReadableStreamDefaultReader {
  #[constructor]
  fn new(
    scope: &mut v8::PinScope,
    stream: v8::Local<v8::Value>,
  ) -> Result<Self, OpError> {
    let id = stream_id(scope, stream)?;
    acquire_reader(scope, id)
  }

  #[promise]
  fn read(&self, scope: &mut v8::PinScope) -> Js {
    reader_read(scope, self.id)
  }

  #[promise]
  fn cancel(
    &self,
    scope: &mut v8::PinScope,
    reason: Option<v8::Local<v8::Value>>,
  ) -> Js {
    let state = READERS.with(|slot| {
      slot
        .borrow()
        .get(&self.id)
        .map(|reader| (reader.released, reader.stream))
    });
    match state {
      Some((false, stream)) => cancel_stream(scope, stream, reason),
      _ => {
        let error = type_error(scope, "The reader is released");
        rejected(scope, error)
      }
    }
  }

  fn release_lock(&self, scope: &mut v8::PinScope) -> Result<(), OpError> {
    release_reader(scope, self.id)
  }

  #[promise]
  #[getter]
  fn closed(&self, scope: &mut v8::PinScope) -> Js {
    let promise = READERS.with(|slot| {
      slot
        .borrow()
        .get(&self.id)
        .map(|reader| reader.closed.clone())
    });
    match promise {
      Some(promise) => {
        let local = v8::Local::new(scope, &promise);
        let value: v8::Local<v8::Value> = local.into();
        Js(v8::Global::new(scope, value))
      }
      None => resolved(scope, v8::undefined(scope).into()),
    }
  }
}

#[op]
impl ReadableStreamDefaultController {
  fn enqueue(
    &self,
    scope: &mut v8::PinScope,
    chunk: Option<v8::Local<v8::Value>>,
    options: Option<v8::Local<v8::Value>>,
  ) -> Result<(), OpError> {
    let chunk = chunk.unwrap_or_else(|| v8::undefined(scope).into());
    let transfer = enqueue_transfers(scope, options)?;
    let owning = OWNING.with(|slot| slot.borrow().contains(&self.stream));
    if !transfer.is_empty() && !owning {
      return Err(OpError::type_("The transfer list is not empty"));
    }
    let chunk = if owning {
      clone_for_owning(scope, chunk, transfer)?
    } else {
      chunk
    };
    enqueue_chunk(scope, self.stream, chunk, None)
  }

  fn close(&self, scope: &mut v8::PinScope) -> Result<(), OpError> {
    close_stream(scope, self.stream)
  }

  fn error(
    &self,
    scope: &mut v8::PinScope,
    error: Option<v8::Local<v8::Value>>,
  ) -> Result<(), OpError> {
    let error = error.unwrap_or_else(|| v8::undefined(scope).into());
    error_stream(scope, self.stream, error)
  }

  #[getter]
  fn desired_size(&self) -> Option<f64> {
    desired_size(self.stream)
  }
}

fn desired_size(id: u64) -> Option<f64> {
  READS.with(|slot| {
    let state = slot.borrow();
    let state = state.get(&id)?;
    match state.phase {
      Phase::Errored => None,
      Phase::Erroring => None,
      Phase::Closed => Some(0.0),
      Phase::Readable => Some(state.high_water - state.queued),
    }
  })
}

#[op]
impl WritableStream {
  #[constructor]
  fn new(
    scope: &mut v8::PinScope,
    sink: Option<v8::Local<v8::Value>>,
    strategy: Option<v8::Local<v8::Value>>,
  ) -> Result<Self, OpError> {
    let (high_water, size_fn) = read_strategy(scope, strategy, DEFAULT_HWM)?;
    if sink_type_present(scope, sink)? {
      return Err(OpError::range("The stream type is invalid"));
    }
    let (start, write, close, abort) = sink_methods(scope, sink)?;
    let id = alloc();
    WRITES.with(|slot| {
      slot.borrow_mut().insert(
        id,
        WriteState {
          high_water,
          queued: 0.0,
          phase: Phase::Readable,
          error: None,
          writer: None,
          started: false,
          closing: false,
          close_started: false,
          sink: Sink::Js {
            write,
            close,
            abort,
          },
          in_flight: 0,
          size_fn,
          queue: VecDeque::new(),
          close_resolver: None,
          abort_wait: None,
          abort_promise: None,
          abort_reason: None,
          abort_controller: fresh_abort_controller(scope),
        },
      );
    });
    let controller = unsafe {
      yuzuko_core::op::wrap_instance(
        scope,
        WritableStreamDefaultController { stream: id },
      )
    };
    if let Some(start) = start {
      let controller: v8::Local<v8::Value> = controller.into();
      let controller = yuzuko_core::op::root(scope, controller);
      match call_method(scope, &start, &[controller]) {
        Ok(value) => {
          let _ = watch_promise(scope, value, Token::StartWrite(id));
        }
        Err(error) => {
          let thrown = error_value(scope, &error);
          let _ = error_write(scope, id, thrown);
          return Err(error);
        }
      }
    } else {
      let value = v8::undefined(scope).into();
      let _ = watch_promise(scope, value, Token::StartWrite(id));
    }
    Ok(Self { id })
  }

  #[getter]
  fn locked(&self) -> bool {
    WRITES.with(|slot| {
      slot
        .borrow()
        .get(&self.id)
        .is_some_and(|state| state.writer.is_some())
    })
  }

  fn get_writer(&self, scope: &mut v8::PinScope) -> Result<Js, OpError> {
    let writer = acquire_writer(scope, self.id)?;
    let object = unsafe { yuzuko_core::op::wrap_instance(scope, writer) };
    let value: v8::Local<v8::Value> = object.into();
    Ok(Js(v8::Global::new(scope, value)))
  }

  #[promise]
  fn abort(
    &self,
    scope: &mut v8::PinScope,
    reason: Option<v8::Local<v8::Value>>,
  ) -> Js {
    if writable_locked(self.id) {
      let error = type_error(scope, "The stream is locked");
      return rejected(scope, error);
    }
    let reason = reason.unwrap_or_else(|| v8::undefined(scope).into());
    abort_write(scope, self.id, reason)
  }

  #[promise]
  fn close(&self, scope: &mut v8::PinScope) -> Js {
    if writable_locked(self.id) {
      let error = type_error(scope, "The stream is locked");
      return rejected(scope, error);
    }
    close_write(scope, self.id)
  }
}

fn writable_locked(stream: u64) -> bool {
  WRITES.with(|slot| {
    slot
      .borrow()
      .get(&stream)
      .is_some_and(|state| state.writer.is_some())
  })
}

#[op]
impl WritableStreamDefaultWriter {
  #[constructor]
  fn new(
    scope: &mut v8::PinScope,
    stream: v8::Local<v8::Value>,
  ) -> Result<Self, OpError> {
    let id = writable_id(scope, stream)?;
    acquire_writer(scope, id)
  }

  #[promise]
  fn write(
    &self,
    scope: &mut v8::PinScope,
    chunk: Option<v8::Local<v8::Value>>,
  ) -> Js {
    let chunk = chunk.unwrap_or_else(|| v8::undefined(scope).into());
    match require_writer(self.id) {
      Ok(stream) => write_chunk(scope, stream, chunk, Some(self.id)),
      Err(error) => {
        let thrown = error_value(scope, &error);
        rejected(scope, thrown)
      }
    }
  }

  #[promise]
  fn close(&self, scope: &mut v8::PinScope) -> Js {
    match require_writer(self.id) {
      Ok(stream) => close_write(scope, stream),
      Err(error) => {
        let thrown = error_value(scope, &error);
        rejected(scope, thrown)
      }
    }
  }

  #[promise]
  fn abort(
    &self,
    scope: &mut v8::PinScope,
    reason: Option<v8::Local<v8::Value>>,
  ) -> Js {
    let reason = reason.unwrap_or_else(|| v8::undefined(scope).into());
    match require_writer(self.id) {
      Ok(stream) => abort_write(scope, stream, reason),
      Err(error) => {
        let thrown = error_value(scope, &error);
        rejected(scope, thrown)
      }
    }
  }

  fn release_lock(&self, scope: &mut v8::PinScope) -> Result<(), OpError> {
    release_writer(scope, self.id)
  }

  #[promise]
  #[getter]
  fn closed(&self, scope: &mut v8::PinScope) -> Js {
    promise_field(scope, self.id, true)
  }

  #[promise]
  #[getter]
  fn ready(&self, scope: &mut v8::PinScope) -> Js {
    promise_field(scope, self.id, false)
  }

  #[getter]
  fn desired_size(&self) -> Result<Option<f64>, OpError> {
    let stream = WRITERS.with(|slot| {
      slot
        .borrow()
        .get(&self.id)
        .filter(|writer| !writer.released)
        .map(|writer| writer.stream)
    });
    let Some(stream) = stream else {
      return Err(OpError::type_("The writer is released"));
    };
    Ok(write_desired(stream))
  }
}

fn write_desired(id: u64) -> Option<f64> {
  WRITES.with(|slot| {
    let state = slot.borrow();
    let state = state.get(&id)?;
    match state.phase {
      Phase::Errored | Phase::Erroring => None,
      Phase::Closed => Some(0.0),
      Phase::Readable => Some(state.high_water - state.queued),
    }
  })
}

#[op]
impl WritableStreamDefaultController {
  #[getter]
  fn signal(&self, scope: &mut v8::PinScope) -> Js {
    let controller = WRITES.with(|slot| {
      slot
        .borrow()
        .get(&self.stream)
        .and_then(|state| state.abort_controller.clone())
    });
    let Some(controller) = controller else {
      let value: v8::Local<v8::Value> = v8::undefined(scope).into();
      return Js(v8::Global::new(scope, value));
    };
    let controller = v8::Local::new(scope, &controller);
    let key = v8::String::new(scope, "signal").unwrap();
    let Some(value) = controller.get(scope, key.into()) else {
      let value: v8::Local<v8::Value> = v8::undefined(scope).into();
      return Js(v8::Global::new(scope, value));
    };
    Js(v8::Global::new(scope, value))
  }

  fn error(
    &self,
    scope: &mut v8::PinScope,
    error: Option<v8::Local<v8::Value>>,
  ) -> Result<(), OpError> {
    let error = error.unwrap_or_else(|| v8::undefined(scope).into());
    error_write(scope, self.stream, error)
  }
}

#[op]
impl TransformStreamDefaultController {
  #[getter]
  fn desired_size(&self) -> Option<f64> {
    desired_size(self.readable)
  }

  fn enqueue(
    &self,
    scope: &mut v8::PinScope,
    chunk: Option<v8::Local<v8::Value>>,
  ) -> Result<(), OpError> {
    let chunk = chunk.unwrap_or_else(|| v8::undefined(scope).into());
    transform_enqueue(scope, self.writable, chunk)
  }

  fn error(
    &self,
    scope: &mut v8::PinScope,
    reason: Option<v8::Local<v8::Value>>,
  ) -> Result<(), OpError> {
    let reason = reason.unwrap_or_else(|| v8::undefined(scope).into());
    transform_error(scope, self.writable, reason);
    Ok(())
  }

  fn terminate(&self, scope: &mut v8::PinScope) -> Result<(), OpError> {
    transform_terminate(scope, self.writable);
    Ok(())
  }
}

#[op]
impl TransformStream {
  #[constructor]
  fn new(
    scope: &mut v8::PinScope,
    transformer: Option<v8::Local<v8::Value>>,
    writable_strategy: Option<v8::Local<v8::Value>>,
    readable_strategy: Option<v8::Local<v8::Value>>,
  ) -> Result<Self, OpError> {
    reject_transform_type(scope, transformer, "readableType")?;
    reject_transform_type(scope, transformer, "writableType")?;
    let (writable_hwm, writable_size) =
      read_strategy(scope, writable_strategy, 1.0)?;
    let (readable_hwm, readable_size) =
      read_strategy(scope, readable_strategy, 0.0)?;
    let (start, transform, flush, cancel) =
      transform_methods(scope, transformer)?;
    let (readable, writable) =
      linked_pair(scope, Sink::Transform { readable: 0 })?;
    let Some(readable_object) = yuzuko_core::op::traced_local(scope, &readable)
    else {
      return Err(OpError::type_("The stream is closed"));
    };
    let readable_object = yuzuko_core::op::root(scope, readable_object);
    let readable_id = stream_id(scope, readable_object.into())?;
    let Some(writable_object) = yuzuko_core::op::traced_local(scope, &writable)
    else {
      return Err(OpError::type_("The stream is closed"));
    };
    let writable_object = yuzuko_core::op::root(scope, writable_object);
    let writable_id = writable_id(scope, writable_object.into())?;
    arm_transform(
      scope,
      readable_id,
      writable_id,
      readable_hwm,
      readable_size,
      writable_hwm,
      writable_size,
      transform,
      flush,
      cancel,
    );
    let start_value = if let Some(start) = start {
      let controller = transform_controller(scope, readable_id, writable_id);
      call_method(scope, &start, &[controller])?
    } else {
      v8::undefined(scope).into()
    };
    let _ =
      watch_promise(scope, start_value, Token::TransformStart(writable_id));
    Ok(Self { readable, writable })
  }

  #[getter]
  fn readable(&self, scope: &mut v8::PinScope) -> Js {
    traced_js(scope, &self.readable)
  }

  #[getter]
  fn writable(&self, scope: &mut v8::PinScope) -> Js {
    traced_js(scope, &self.writable)
  }
}

#[op]
impl TextEncoderStream {
  #[constructor]
  fn new(scope: &mut v8::PinScope) -> Result<Self, OpError> {
    let (readable, writable) = linked_pair(
      scope,
      Sink::Utf8Encode {
        pending: None,
        readable: 0,
      },
    )?;
    Ok(Self { readable, writable })
  }

  #[getter]
  fn encoding(&self) -> String {
    "utf-8".to_string()
  }

  #[getter]
  fn readable(&self, scope: &mut v8::PinScope) -> Js {
    traced_js(scope, &self.readable)
  }

  #[getter]
  fn writable(&self, scope: &mut v8::PinScope) -> Js {
    traced_js(scope, &self.writable)
  }
}

#[op]
impl TextDecoderStream {
  #[constructor]
  fn new(
    scope: &mut v8::PinScope,
    label: Option<String>,
    options: Option<v8::Local<v8::Value>>,
  ) -> Result<Self, OpError> {
    let label = label.unwrap_or_else(|| "utf-8".to_string());
    let encoding = encoding_rs::Encoding::for_label(label.as_bytes())
      .ok_or_else(|| OpError::range("The encoding label is unknown"))?;
    let fatal = option_bool(scope, options, "fatal");
    let ignore_bom = option_bool(scope, options, "ignoreBOM");
    let decoder = if ignore_bom {
      encoding.new_decoder_without_bom_handling()
    } else {
      encoding.new_decoder()
    };
    let (readable, writable) = linked_pair(
      scope,
      Sink::Utf8Decode {
        decoder,
        fatal,
        readable: 0,
      },
    )?;
    Ok(Self {
      readable,
      writable,
      encoding: encoding.name().to_ascii_lowercase(),
      fatal,
      ignore_bom,
    })
  }

  #[getter]
  fn encoding(&self) -> String {
    self.encoding.clone()
  }

  #[getter]
  fn fatal(&self) -> bool {
    self.fatal
  }

  #[getter]
  #[rename("ignoreBOM")]
  fn ignore_bom(&self) -> bool {
    self.ignore_bom
  }

  #[getter]
  fn readable(&self, scope: &mut v8::PinScope) -> Js {
    traced_js(scope, &self.readable)
  }

  #[getter]
  fn writable(&self, scope: &mut v8::PinScope) -> Js {
    traced_js(scope, &self.writable)
  }
}

#[op]
impl CompressionStream {
  #[constructor]
  fn new(scope: &mut v8::PinScope, format: String) -> Result<Self, OpError> {
    let sink = compression_sink(&format, true)?;
    let (readable, writable) = linked_pair(scope, sink)?;
    Ok(Self { readable, writable })
  }

  #[getter]
  fn readable(&self, scope: &mut v8::PinScope) -> Js {
    traced_js(scope, &self.readable)
  }

  #[getter]
  fn writable(&self, scope: &mut v8::PinScope) -> Js {
    traced_js(scope, &self.writable)
  }
}

#[op]
impl DecompressionStream {
  #[constructor]
  fn new(scope: &mut v8::PinScope, format: String) -> Result<Self, OpError> {
    let sink = compression_sink(&format, false)?;
    let (readable, writable) = linked_pair(scope, sink)?;
    Ok(Self { readable, writable })
  }

  #[getter]
  fn readable(&self, scope: &mut v8::PinScope) -> Js {
    traced_js(scope, &self.readable)
  }

  #[getter]
  fn writable(&self, scope: &mut v8::PinScope) -> Js {
    traced_js(scope, &self.writable)
  }
}

fn compression_sink(format: &str, compress: bool) -> Result<Sink, OpError> {
  let format = format.to_ascii_lowercase();
  let level = Compression::default();
  let codec = match (format.as_str(), compress) {
    ("gzip", true) => Codec::GzipEnc(GzEncoder::new(Vec::new(), level)),
    ("deflate", true) => Codec::ZlibEnc(ZlibEncoder::new(Vec::new(), level)),
    ("deflate-raw", true) => {
      Codec::RawEnc(DeflateEncoder::new(Vec::new(), level))
    }
    ("gzip", false) => Codec::GzipDec(GzDecoder::new(Vec::new())),
    ("deflate", false) => Codec::ZlibDec(ZlibDecoder::new(Vec::new())),
    ("deflate-raw", false) => Codec::RawDec(DeflateDecoder::new(Vec::new())),
    ("brotli", true) => Codec::BrotliEnc(Box::new(
      brotli::CompressorWriter::new(Vec::new(), 4096, 5, 22),
    )),
    ("brotli", false) => Codec::BrotliDec(Box::new(
      brotli::DecompressorWriter::new(Vec::new(), 4096),
    )),
    _ => {
      return Err(OpError::type_("The compression format is not supported"));
    }
  };
  Ok(Sink::Codec {
    codec: Box::new(codec),
    emitted: 0,
    readable: 0,
  })
}

fn linked_pair(
  scope: &mut v8::PinScope,
  mut sink: Sink,
) -> Result<
  (
    v8::TracedReference<v8::Object>,
    v8::TracedReference<v8::Object>,
  ),
  OpError,
> {
  let readable_id = alloc();
  READS.with(|slot| {
    slot.borrow_mut().insert(
      readable_id,
      ReadState {
        queue: VecDeque::new(),
        queued: 0.0,
        high_water: DEFAULT_HWM,
        phase: Phase::Readable,
        error: None,
        reader: None,
        close_requested: false,
        disturbed: false,
        pulling: false,
        pull_again: false,
        started: true,
        pull_fn: None,
        cancel_fn: None,
        size_fn: None,
        native: None,
        paired: None,
      },
    );
  });
  set_sink_readable(&mut sink, readable_id);
  let writable_id = alloc();
  WRITES.with(|slot| {
    slot.borrow_mut().insert(
      writable_id,
      WriteState {
        high_water: DEFAULT_HWM,
        queued: 0.0,
        phase: Phase::Readable,
        error: None,
        writer: None,
        started: true,
        closing: false,
        close_started: false,
        sink,
        in_flight: 0,
        size_fn: None,
        queue: VecDeque::new(),
        close_resolver: None,
        abort_wait: None,
        abort_promise: None,
        abort_reason: None,
        abort_controller: fresh_abort_controller(scope),
      },
    );
  });
  let readable = unsafe {
    yuzuko_core::op::wrap_instance(scope, ReadableStream { id: readable_id })
  };
  let writable = unsafe {
    yuzuko_core::op::wrap_instance(scope, WritableStream { id: writable_id })
  };
  Ok((
    yuzuko_core::op::traced(scope, readable),
    yuzuko_core::op::traced(scope, writable),
  ))
}

fn set_sink_readable(sink: &mut Sink, readable: u64) {
  match sink {
    Sink::Transform { readable: slot, .. }
    | Sink::Utf8Encode { readable: slot, .. }
    | Sink::Utf8Decode { readable: slot, .. }
    | Sink::Codec { readable: slot, .. } => *slot = readable,
    Sink::Js { .. } => {}
  }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ReaderMode {
  Default,
  Byob,
}

fn reader_mode(
  scope: &mut v8::PinScope,
  options: v8::Local<v8::Value>,
) -> Result<ReaderMode, OpError> {
  // Web IDL treats null the same as a missing dictionary here.
  if options.is_null() {
    return Ok(ReaderMode::Default);
  }
  if !options.is_object() {
    return Err(OpError::type_("The reader options are not an object"));
  }
  let object = object_value(scope, options)?;
  let Some(mode) = property(scope, object, "mode")? else {
    return Ok(ReaderMode::Default);
  };
  if mode.is_undefined() {
    return Ok(ReaderMode::Default);
  }
  let text = js_string(scope, mode)?;
  if text == "byob" {
    return Ok(ReaderMode::Byob);
  }
  Err(OpError::type_("The reader mode is invalid"))
}

fn acquire_reader(
  scope: &mut v8::PinScope,
  stream: u64,
) -> Result<ReadableStreamDefaultReader, OpError> {
  let locked = READS.with(|slot| {
    slot
      .borrow()
      .get(&stream)
      .is_some_and(|state| state.reader.is_some())
  });
  if locked {
    return Err(OpError::type_("The stream is locked"));
  }
  let resolver = v8::PromiseResolver::new(scope).unwrap();
  let promise = resolver.get_promise(scope);
  let id = alloc();
  let closed_state =
    READS.with(|slot| slot.borrow().get(&stream).map(|state| state.phase));
  READERS.with(|slot| {
    slot.borrow_mut().insert(
      id,
      ReaderState {
        stream,
        released: false,
        byob: false,
        requests: VecDeque::new(),
        into_requests: VecDeque::new(),
        closed: v8::Global::new(scope, promise),
        closed_resolver: v8::Global::new(scope, resolver),
        settled: false,
      },
    );
  });
  READS.with(|slot| {
    if let Some(state) = slot.borrow_mut().get_mut(&stream) {
      state.reader = Some(id);
    }
  });
  if closed_state == Some(Phase::Closed) {
    settle_reader_closed(scope, id, true, None);
  } else if closed_state == Some(Phase::Errored) {
    let error = READS.with(|slot| {
      slot
        .borrow()
        .get(&stream)
        .and_then(|state| state.error.clone())
    });
    settle_reader_closed(scope, id, false, error);
  }
  Ok(ReadableStreamDefaultReader { id })
}

fn reader_read(scope: &mut v8::PinScope, reader: u64) -> Js {
  reader_read_at(scope, reader, true)
}

fn reader_read_at(scope: &mut v8::PinScope, reader: u64, pull: bool) -> Js {
  let state = READERS.with(|slot| {
    slot
      .borrow()
      .get(&reader)
      .map(|state| (state.released, state.stream))
  });
  let Some((false, stream)) = state else {
    let error = type_error(scope, "The reader is released");
    return rejected(scope, error);
  };
  if byte_stream::is_byte(stream) {
    return byte_stream::default_read(scope, reader, stream);
  }
  READS.with(|slot| {
    if let Some(state) = slot.borrow_mut().get_mut(&stream) {
      state.disturbed = true;
    }
  });
  // State wins over anything left in the queue. A chunk enqueued after
  // `close()` or `error()` inside `size()` is not readable.
  let phase =
    READS.with(|slot| slot.borrow().get(&stream).map(|state| state.phase));
  match phase {
    Some(Phase::Closed) => {
      let result = read_result(scope, None, true);
      return resolved(scope, result);
    }
    Some(Phase::Errored) => {
      let error = READS.with(|slot| {
        slot
          .borrow()
          .get(&stream)
          .and_then(|state| state.error.clone())
      });
      let error = error
        .map(|error| v8::Local::new(scope, &error))
        .unwrap_or_else(|| type_error(scope, "The stream is errored"));
      return rejected(scope, error);
    }
    _ => {}
  }
  if let Some(chunk) = dequeue(stream) {
    let value = v8::Local::new(scope, &chunk.value);
    let value = yuzuko_core::op::root(scope, value);
    finish_close_if_drained(scope, stream);
    if pull {
      pull_if_needed(scope, stream);
    }
    let result = read_result(scope, Some(value), false);
    return resolved(scope, result);
  }
  let resolver = v8::PromiseResolver::new(scope).unwrap();
  let promise = resolver.get_promise(scope);
  READERS.with(|slot| {
    if let Some(state) = slot.borrow_mut().get_mut(&reader) {
      state.requests.push_back(v8::Global::new(scope, resolver));
    }
  });
  if pull {
    pull_if_needed(scope, stream);
  }
  let value: v8::Local<v8::Value> = promise.into();
  Js(v8::Global::new(scope, value))
}

fn dequeue(stream: u64) -> Option<Chunk> {
  READS.with(|slot| {
    let mut states = slot.borrow_mut();
    let state = states.get_mut(&stream)?;
    let chunk = state.queue.pop_front()?;
    state.queued = (state.queued - chunk.size).max(0.0);
    Some(chunk)
  })
}

fn enqueue_chunk(
  scope: &mut v8::PinScope,
  stream: u64,
  chunk: v8::Local<v8::Value>,
  size: Option<f64>,
) -> Result<(), OpError> {
  let phase =
    READS.with(|slot| slot.borrow().get(&stream).map(|state| state.phase));
  match phase {
    Some(Phase::Closed) => {
      return Err(OpError::type_("The stream is closed"));
    }
    Some(Phase::Errored | Phase::Erroring) => {
      return Err(OpError::type_("The stream is errored"));
    }
    None => return Err(OpError::type_("The stream is closed")),
    Some(Phase::Readable) => {}
  }
  let close_requested = READS.with(|slot| {
    slot
      .borrow()
      .get(&stream)
      .is_some_and(|state| state.close_requested)
  });
  if close_requested {
    return Err(OpError::type_("The stream is closed"));
  }
  // Pending reads are observed before `size()`. A read started inside
  // `size()` does not receive this chunk.
  if read_pending(stream)
    && let Some(request) = pop_request(stream)
  {
    let resolver = v8::Local::new(scope, &request);
    let resolver = yuzuko_core::op::root(scope, resolver);
    let result = read_result(scope, Some(chunk), false);
    fulfill_resolver(scope, resolver, result);
    pull_if_needed(scope, stream);
    return Ok(());
  }
  let size = match size {
    Some(size) => size,
    None => match chunk_size(scope, stream, chunk) {
      Ok(size) => size,
      Err(error) => {
        // A throwing or invalid size algorithm errors the stream, then throws.
        let thrown = error_value(scope, &error);
        let _ = error_stream(scope, stream, thrown);
        return Err(error);
      }
    },
  };
  READS.with(|slot| {
    if let Some(state) = slot.borrow_mut().get_mut(&stream) {
      state.queued += size;
      state.queue.push_back(Chunk {
        value: v8::Global::new(scope, chunk),
        size,
      });
    }
  });
  // A queued chunk can leave room under the high water mark. Pull again
  // after this pull settles, even when nobody is reading.
  pull_if_needed(scope, stream);
  Ok(())
}

fn chunk_size(
  scope: &mut v8::PinScope,
  stream: u64,
  chunk: v8::Local<v8::Value>,
) -> Result<f64, OpError> {
  let size_fn = READS.with(|slot| {
    slot
      .borrow()
      .get(&stream)
      .and_then(|state| state.size_fn.clone())
  });
  let Some(size_fn) = size_fn else {
    return Ok(1.0);
  };
  let value = call(scope, &size_fn, v8::undefined(scope).into(), &[chunk])?;
  let number = value
    .number_value(scope)
    .ok_or_else(|| OpError::type_("The chunk size is not a number"))?;
  if !number.is_finite() || number < 0.0 {
    let message = v8::String::new(scope, "The chunk size is invalid").unwrap();
    let error = v8::Exception::range_error(scope, message);
    return Err(OpError::thrown(scope, error));
  }
  Ok(number)
}

fn pop_request(stream: u64) -> Option<v8::Global<v8::PromiseResolver>> {
  let reader = READS
    .with(|slot| slot.borrow().get(&stream).and_then(|state| state.reader))?;
  READERS.with(|slot| {
    slot
      .borrow_mut()
      .get_mut(&reader)
      .and_then(|state| state.requests.pop_front())
  })
}

fn close_stream(scope: &mut v8::PinScope, stream: u64) -> Result<(), OpError> {
  let phase = READS.with(|slot| {
    slot
      .borrow()
      .get(&stream)
      .map(|state| (state.phase, state.close_requested))
  });
  match phase {
    Some((Phase::Readable, false)) => {}
    _ => return Err(OpError::type_("The stream is closed")),
  }
  let empty = READS.with(|slot| {
    let mut states = slot.borrow_mut();
    let Some(state) = states.get_mut(&stream) else {
      return true;
    };
    state.close_requested = true;
    state.queue.is_empty()
  });
  if empty {
    finish_close(scope, stream);
  }
  Ok(())
}

fn finish_close_if_drained(scope: &mut v8::PinScope, stream: u64) {
  let done = READS.with(|slot| {
    slot.borrow().get(&stream).is_some_and(|state| {
      state.close_requested
        && state.queue.is_empty()
        && state.phase == Phase::Readable
    })
  });
  if done {
    finish_close(scope, stream);
  }
}

fn finish_close(scope: &mut v8::PinScope, stream: u64) {
  let reader = READS.with(|slot| {
    if let Some(state) = slot.borrow_mut().get_mut(&stream) {
      state.phase = Phase::Closed;
      state.reader
    } else {
      None
    }
  });
  if let Some(reader) = reader {
    // `closed` settles before queued reads, so its reactions run first.
    settle_reader_closed(scope, reader, true, None);
    while let Some(request) = pop_request(stream) {
      let resolver = v8::Local::new(scope, &request);
      let resolver = yuzuko_core::op::root(scope, resolver);
      let result = read_result(scope, None, true);
      fulfill_resolver(scope, resolver, result);
    }
  }
}

fn error_stream(
  scope: &mut v8::PinScope,
  stream: u64,
  error: v8::Local<v8::Value>,
) -> Result<(), OpError> {
  // `error()` after close, or a second `error()`, does not throw.
  let readable = READS.with(|slot| {
    slot
      .borrow()
      .get(&stream)
      .is_some_and(|state| state.phase == Phase::Readable)
  });
  if !readable {
    return Ok(());
  }
  let stored = v8::Global::new(scope, error);
  let reader = READS.with(|slot| {
    if let Some(state) = slot.borrow_mut().get_mut(&stream) {
      if state.phase != Phase::Readable {
        return None;
      }
      state.phase = Phase::Errored;
      state.error = Some(stored.clone());
      state.queue.clear();
      state.queued = 0.0;
      state.reader
    } else {
      None
    }
  });
  if let Some(reader) = reader {
    // Reject `closed` before read requests. Reactions then run in that order.
    settle_reader_closed(scope, reader, false, Some(stored.clone()));
    while let Some(request) = pop_request(stream) {
      let resolver = v8::Local::new(scope, &request);
      let resolver = yuzuko_core::op::root(scope, resolver);
      let error = v8::Local::new(scope, &stored);
      let error = yuzuko_core::op::root(scope, error);
      resolver.reject(scope, error);
    }
    byte_stream::reject_into(scope, reader, v8::Local::new(scope, &stored));
  }
  Ok(())
}

fn settle_reader_closed(
  scope: &mut v8::PinScope,
  reader: u64,
  ok: bool,
  error: Option<v8::Global<v8::Value>>,
) {
  let taken = READERS.with(|slot| {
    let mut readers = slot.borrow_mut();
    let state = readers.get_mut(&reader)?;
    if state.settled {
      return None;
    }
    state.settled = true;
    Some(state.closed_resolver.clone())
  });
  let Some(resolver) = taken else {
    return;
  };
  let resolver = v8::Local::new(scope, &resolver);
  let resolver = yuzuko_core::op::root(scope, resolver);
  if ok {
    let value = v8::undefined(scope).into();
    resolver.resolve(scope, value);
  } else {
    let error = match error {
      Some(error) => {
        let error = v8::Local::new(scope, &error);
        yuzuko_core::op::root(scope, error)
      }
      None => type_error(scope, "The stream is errored"),
    };
    resolver.reject(scope, error);
    mark_reader_closed(scope, reader);
  }
}

fn mark_reader_closed(scope: &mut v8::PinScope, reader: u64) {
  let promise = READERS
    .with(|slot| slot.borrow().get(&reader).map(|state| state.closed.clone()));
  if let Some(promise) = promise {
    let promise = v8::Local::new(scope, &promise);
    promise.mark_as_handled();
  }
}

fn cancel_stream(
  scope: &mut v8::PinScope,
  stream: u64,
  reason: Option<v8::Local<v8::Value>>,
) -> Js {
  let reason = reason.unwrap_or_else(|| v8::undefined(scope).into());
  let snapshot = READS.with(|slot| {
    let mut states = slot.borrow_mut();
    let state = states.get_mut(&stream)?;
    state.disturbed = true;
    Some((state.phase, state.error.clone(), state.cancel_fn.clone()))
  });
  let Some((phase, error, cancel)) = snapshot else {
    let thrown = type_error(scope, "The stream is closed");
    return rejected(scope, thrown);
  };
  match phase {
    Phase::Closed => return resolved(scope, v8::undefined(scope).into()),
    Phase::Errored => {
      let thrown = error
        .map(|error| v8::Local::new(scope, &error))
        .unwrap_or_else(|| type_error(scope, "The stream is errored"));
      return rejected(scope, thrown);
    }
    Phase::Readable | Phase::Erroring => {}
  }
  // Cancel discards queued chunks and closes. The cancel algorithm runs after.
  READS.with(|slot| {
    if let Some(state) = slot.borrow_mut().get_mut(&stream) {
      state.queue.clear();
      state.queued = 0.0;
    }
  });
  byte_stream::prepare_cancel(stream);
  finish_close(scope, stream);
  byte_stream::finish_cancel_reads(scope, stream);
  if let Some(writable) = transform_writable(stream) {
    return transform_source_cancel(scope, writable, reason);
  }
  let Some(cancel) = cancel else {
    return resolved(scope, v8::undefined(scope).into());
  };
  match call(scope, &cancel, v8::undefined(scope).into(), &[reason]) {
    Ok(value) => promise_undefined(scope, value),
    Err(error) => {
      let thrown = error_value(scope, &error);
      rejected(scope, thrown)
    }
  }
}

fn release_reader(
  scope: &mut v8::PinScope,
  reader: u64,
) -> Result<(), OpError> {
  let snapshot = READERS.with(|slot| {
    slot
      .borrow()
      .get(&reader)
      .map(|state| (state.stream, state.released))
  });
  let Some((stream, false)) = snapshot else {
    return Ok(());
  };
  let phase =
    READS.with(|slot| slot.borrow().get(&stream).map(|state| state.phase));
  byte_stream::release_steps(scope, stream);
  READS.with(|slot| {
    if let Some(state) = slot.borrow_mut().get_mut(&stream)
      && state.reader == Some(reader)
    {
      state.reader = None;
    }
  });
  READERS.with(|slot| {
    if let Some(state) = slot.borrow_mut().get_mut(&reader) {
      state.released = true;
    }
  });
  let message = "Reader was released and can no longer be used to monitor the stream's closedness";
  if phase == Some(Phase::Readable) {
    let error = type_error(scope, message);
    let stored = v8::Global::new(scope, error);
    settle_reader_closed(scope, reader, false, Some(stored));
  } else {
    let error = type_error(scope, message);
    replace_reader_closed(scope, reader, error);
  }
  let requests = READERS.with(|slot| {
    slot
      .borrow_mut()
      .get_mut(&reader)
      .map(|state| std::mem::take(&mut state.requests))
      .unwrap_or_default()
  });
  if !requests.is_empty() {
    let error = type_error(scope, "Reader was released");
    let error = yuzuko_core::op::root(scope, error);
    for request in requests {
      let resolver = v8::Local::new(scope, &request);
      let resolver = yuzuko_core::op::root(scope, resolver);
      resolver.reject(scope, error);
    }
  }
  let into_requests = READERS.with(|slot| {
    slot
      .borrow_mut()
      .get_mut(&reader)
      .map(|state| std::mem::take(&mut state.into_requests))
      .unwrap_or_default()
  });
  if !into_requests.is_empty() {
    let error = type_error(scope, "Reader was released");
    let error = yuzuko_core::op::root(scope, error);
    for request in into_requests {
      let resolver = v8::Local::new(scope, &request);
      let resolver = yuzuko_core::op::root(scope, resolver);
      resolver.reject(scope, error);
    }
  }
  Ok(())
}

fn replace_reader_closed(
  scope: &mut v8::PinScope,
  reader: u64,
  error: v8::Local<v8::Value>,
) {
  let resolver = v8::PromiseResolver::new(scope).unwrap();
  let promise = resolver.get_promise(scope);
  let error = yuzuko_core::op::root(scope, error);
  resolver.reject(scope, error);
  promise.mark_as_handled();
  READERS.with(|slot| {
    if let Some(state) = slot.borrow_mut().get_mut(&reader) {
      state.closed = v8::Global::new(scope, promise);
      state.closed_resolver = v8::Global::new(scope, resolver);
      state.settled = true;
    }
  });
}

fn mark_started(scope: &mut v8::PinScope, id: u64) {
  READS.with(|slot| {
    if let Some(state) = slot.borrow_mut().get_mut(&id) {
      state.started = true;
    }
  });
  if byte_stream::is_byte(id) {
    byte_stream::on_started(scope, id);
  } else {
    pull_if_needed(scope, id);
  }
}

fn pull_if_needed(scope: &mut v8::PinScope, id: u64) {
  if native_ready(id) {
    pull_native(scope, id);
    return;
  }
  if transform_writable(id).is_some() {
    transform_pull(scope, id);
    return;
  }
  let pending = read_pending(id);
  if let Some((tee, branch)) = tee_branch(id) {
    let should = READS.with(|slot| {
      slot.borrow().get(&id).is_some_and(|state| {
        state.started
          && state.phase == Phase::Readable
          && !state.close_requested
          && (state.high_water - state.queued > 0.0 || pending)
      })
    });
    if should {
      tee_pull(scope, tee, branch);
    }
    return;
  }
  let pull = READS.with(|slot| {
    let mut states = slot.borrow_mut();
    let state = states.get_mut(&id)?;
    if !state.started
      || state.phase != Phase::Readable
      || state.close_requested
      || state.pull_fn.is_none()
    {
      return None;
    }
    if state.high_water - state.queued <= 0.0 && !pending {
      return None;
    }
    if state.pulling {
      state.pull_again = true;
      return None;
    }
    state.pulling = true;
    state.pull_fn.clone()
  });
  let Some(pull) = pull else {
    return;
  };
  let controller = unsafe {
    yuzuko_core::op::wrap_instance(
      scope,
      ReadableStreamDefaultController { stream: id },
    )
  };
  let controller: v8::Local<v8::Value> = controller.into();
  let controller = yuzuko_core::op::root(scope, controller);
  match call_method(scope, &pull, &[controller]) {
    Ok(value) => {
      let _ = watch_promise(scope, value, Token::Pull(id));
    }
    Err(error) => {
      let thrown = error_value(scope, &error);
      let _ = error_stream(scope, id, thrown);
    }
  }
}

fn native_ready(id: u64) -> bool {
  READS.with(|slot| {
    slot.borrow().get(&id).is_some_and(|state| {
      state.native.is_some()
        && state.started
        && state.phase == Phase::Readable
        && !state.close_requested
        && state.high_water - state.queued > 0.0
    })
  })
}

fn pull_native(scope: &mut v8::PinScope, id: u64) {
  let step = READS.with(|slot| {
    let mut states = slot.borrow_mut();
    let Some(state) = states.get_mut(&id) else {
      return NativeStep::Idle;
    };
    if !state.started
      || state.phase != Phase::Readable
      || state.close_requested
      || state.high_water - state.queued <= 0.0
    {
      return NativeStep::Idle;
    }
    let Some(native) = state.native.as_mut() else {
      return NativeStep::Idle;
    };
    match native.source.read_next(native.cursor, 64 * 1024) {
      Ok(Some(chunk)) => {
        native.cursor += chunk.len() as u64;
        let done = native.cursor >= native.source.size();
        NativeStep::Chunk(chunk, done)
      }
      Ok(None) => NativeStep::End,
      Err(_) => NativeStep::Error,
    }
  });
  match step {
    NativeStep::Idle => {}
    NativeStep::End => {
      let _ = close_stream(scope, id);
    }
    NativeStep::Error => {
      let error = type_error(scope, "The blob could not be read");
      let _ = error_stream(scope, id, error);
    }
    NativeStep::Chunk(chunk, done) => {
      let view = bytes::uint8_array(scope, chunk.to_vec());
      if enqueue_chunk(scope, id, view.into(), None).is_err() {
        return;
      }
      if done {
        let _ = close_stream(scope, id);
      }
    }
  }
}

enum NativeStep {
  Idle,
  End,
  Error,
  Chunk(::bytes::Bytes, bool),
}

fn on_pull_settled(
  scope: &mut v8::PinScope,
  id: u64,
  failed: Option<v8::Local<v8::Value>>,
) {
  if let Some(error) = failed {
    let _ = error_stream(scope, id, error);
    return;
  }
  let again = READS.with(|slot| {
    if let Some(state) = slot.borrow_mut().get_mut(&id) {
      state.pulling = false;
      let again = state.pull_again;
      state.pull_again = false;
      again
    } else {
      false
    }
  });
  if again {
    pull_if_needed(scope, id);
  }
}

fn acquire_writer(
  scope: &mut v8::PinScope,
  stream: u64,
) -> Result<WritableStreamDefaultWriter, OpError> {
  let locked = WRITES.with(|slot| {
    slot
      .borrow()
      .get(&stream)
      .is_some_and(|state| state.writer.is_some())
  });
  if locked {
    return Err(OpError::type_("The stream is locked"));
  }
  let snapshot = WRITES.with(|slot| {
    slot.borrow().get(&stream).map(|state| {
      (
        state.phase,
        state.queued >= state.high_water,
        state.error.clone(),
      )
    })
  });
  let Some((phase, backpressure, error)) = snapshot else {
    return Err(OpError::type_("The stream is closed"));
  };
  let closed_resolver = v8::PromiseResolver::new(scope).unwrap();
  let closed = closed_resolver.get_promise(scope);
  let ready_resolver = v8::PromiseResolver::new(scope).unwrap();
  let ready = ready_resolver.get_promise(scope);
  let mut settled = false;
  let mut ready_pending = true;
  if phase == Phase::Closed {
    let value = v8::undefined(scope).into();
    closed_resolver.resolve(scope, value);
    ready_resolver.resolve(scope, value);
    settled = true;
    ready_pending = false;
  } else if phase == Phase::Erroring {
    let thrown = error
      .as_ref()
      .map(|error| v8::Local::new(scope, error))
      .unwrap_or_else(|| type_error(scope, "The stream is errored"));
    let thrown = yuzuko_core::op::root(scope, thrown);
    ready_resolver.reject(scope, thrown);
    ready.mark_as_handled();
    ready_pending = false;
  } else if phase == Phase::Errored {
    let thrown = error
      .as_ref()
      .map(|error| v8::Local::new(scope, error))
      .unwrap_or_else(|| type_error(scope, "The stream is errored"));
    let thrown = yuzuko_core::op::root(scope, thrown);
    closed_resolver.reject(scope, thrown);
    ready_resolver.reject(scope, thrown);
    closed.mark_as_handled();
    ready.mark_as_handled();
    settled = true;
    ready_pending = false;
  } else if !backpressure {
    let value = v8::undefined(scope).into();
    ready_resolver.resolve(scope, value);
    ready_pending = false;
  }
  let id = alloc();
  WRITERS.with(|slot| {
    slot.borrow_mut().insert(
      id,
      WriterState {
        stream,
        closed: v8::Global::new(scope, closed),
        closed_resolver: v8::Global::new(scope, closed_resolver),
        ready: v8::Global::new(scope, ready),
        ready_resolver: ready_pending
          .then(|| v8::Global::new(scope, ready_resolver)),
        settled,
        released: false,
      },
    );
  });
  WRITES.with(|slot| {
    if let Some(state) = slot.borrow_mut().get_mut(&stream) {
      state.writer = Some(id);
    }
  });
  Ok(WritableStreamDefaultWriter { id })
}

fn require_writer(writer: u64) -> Result<u64, OpError> {
  WRITERS.with(|slot| match slot.borrow().get(&writer) {
    Some(state) if !state.released => Ok(state.stream),
    _ => Err(OpError::type_("The writer is released")),
  })
}

fn release_writer(
  scope: &mut v8::PinScope,
  writer: u64,
) -> Result<(), OpError> {
  let stream = WRITERS.with(|slot| {
    slot
      .borrow()
      .get(&writer)
      .and_then(|state| (!state.released).then_some(state.stream))
  });
  let Some(stream) = stream else {
    return Ok(());
  };
  WRITES.with(|slot| {
    if let Some(state) = slot.borrow_mut().get_mut(&stream)
      && state.writer == Some(writer)
    {
      state.writer = None;
    }
  });
  let error = type_error(
    scope,
    "Writer was released and can no longer be used to monitor the stream's closedness",
  );
  let stored = v8::Global::new(scope, error);
  let thrown = v8::Local::new(scope, &stored);
  let thrown = yuzuko_core::op::root(scope, thrown);
  reject_ready(scope, writer, thrown);
  let thrown = v8::Local::new(scope, &stored);
  let thrown = yuzuko_core::op::root(scope, thrown);
  reject_released_closed(scope, writer, thrown);
  WRITERS.with(|slot| {
    if let Some(state) = slot.borrow_mut().get_mut(&writer) {
      state.released = true;
    }
  });
  Ok(())
}

fn reject_released_closed(
  scope: &mut v8::PinScope,
  writer: u64,
  error: v8::Local<v8::Value>,
) {
  let settled = WRITERS.with(|slot| {
    slot
      .borrow()
      .get(&writer)
      .is_some_and(|state| state.settled)
  });
  if !settled {
    settle_writer(scope, writer, false, Some(v8::Global::new(scope, error)));
    return;
  }
  let resolver = v8::PromiseResolver::new(scope).unwrap();
  let promise = resolver.get_promise(scope);
  resolver.reject(scope, error);
  promise.mark_as_handled();
  WRITERS.with(|slot| {
    if let Some(state) = slot.borrow_mut().get_mut(&writer) {
      state.closed = v8::Global::new(scope, promise);
      state.closed_resolver = v8::Global::new(scope, resolver);
    }
  });
}

fn promise_field(scope: &mut v8::PinScope, writer: u64, closed: bool) -> Js {
  let promise = WRITERS.with(|slot| {
    slot.borrow().get(&writer).map(|state| {
      if closed {
        state.closed.clone()
      } else {
        state.ready.clone()
      }
    })
  });
  match promise {
    Some(promise) => {
      let local = v8::Local::new(scope, &promise);
      let value: v8::Local<v8::Value> = local.into();
      Js(v8::Global::new(scope, value))
    }
    None => resolved(scope, v8::undefined(scope).into()),
  }
}

fn write_chunk(
  scope: &mut v8::PinScope,
  stream: u64,
  chunk: v8::Local<v8::Value>,
  writer: Option<u64>,
) -> Js {
  let blocked = WRITES.with(|slot| {
    slot
      .borrow()
      .get(&stream)
      .map(|state| (state.phase, state.closing, state.error.clone()))
  });
  match blocked {
    Some((Phase::Readable, false, _)) => {}
    Some((Phase::Errored | Phase::Erroring, _, error)) => {
      let error = error
        .map(|error| v8::Local::new(scope, &error))
        .unwrap_or_else(|| type_error(scope, "The stream is errored"));
      return rejected(scope, error);
    }
    _ => {
      let error = type_error(scope, "The stream is closed");
      return rejected(scope, error);
    }
  }
  let size = match writable_size(scope, stream, chunk) {
    Ok(size) => size,
    Err(error) => {
      let thrown = error_value(scope, &error);
      let _ = error_write(scope, stream, thrown);
      let thrown = error_value(scope, &error);
      return rejected(scope, thrown);
    }
  };
  // `size()` may close, error, abort, or release the writer.
  if writer.is_some_and(writer_is_released) {
    let error =
      type_error(scope, "Cannot write to a stream using a released writer");
    return rejected(scope, error);
  }
  let after = WRITES.with(|slot| {
    slot
      .borrow()
      .get(&stream)
      .map(|state| (state.phase, state.closing, state.error.clone()))
  });
  match after {
    Some((Phase::Readable, false, _)) => {}
    Some((Phase::Errored | Phase::Erroring, _, error)) => {
      let error = error
        .map(|error| v8::Local::new(scope, &error))
        .unwrap_or_else(|| type_error(scope, "The stream is errored"));
      return rejected(scope, error);
    }
    _ => {
      let error = type_error(
        scope,
        "The stream is closing or closed and cannot be written to",
      );
      return rejected(scope, error);
    }
  }
  let resolver = v8::PromiseResolver::new(scope).unwrap();
  let promise = resolver.get_promise(scope);
  let defer = WRITES.with(|slot| {
    slot
      .borrow()
      .get(&stream)
      .is_some_and(|state| !state.started || state.in_flight > 0)
  });
  if defer {
    WRITES.with(|slot| {
      if let Some(state) = slot.borrow_mut().get_mut(&stream) {
        state.queued += size;
        state.queue.push_back(QueuedWrite {
          chunk: v8::Global::new(scope, chunk),
          resolver: v8::Global::new(scope, resolver),
          size,
        });
      }
    });
    update_ready(scope, stream);
  } else {
    start_inflight(scope, stream, chunk, resolver, size, true);
  }
  let value: v8::Local<v8::Value> = promise.into();
  Js(v8::Global::new(scope, value))
}

fn writer_is_released(writer: u64) -> bool {
  WRITERS.with(|slot| {
    slot
      .borrow()
      .get(&writer)
      .is_none_or(|state| state.released)
  })
}

fn writable_size(
  scope: &mut v8::PinScope,
  stream: u64,
  chunk: v8::Local<v8::Value>,
) -> Result<f64, OpError> {
  let size_fn = WRITES.with(|slot| {
    slot
      .borrow()
      .get(&stream)
      .and_then(|state| state.size_fn.clone())
  });
  let Some(size_fn) = size_fn else {
    return Ok(1.0);
  };
  let value = call(scope, &size_fn, v8::undefined(scope).into(), &[chunk])?;
  let number = value
    .number_value(scope)
    .ok_or_else(|| OpError::type_("The chunk size is not a number"))?;
  if !number.is_finite() || number < 0.0 {
    let message = v8::String::new(scope, "The chunk size is invalid").unwrap();
    let error = v8::Exception::range_error(scope, message);
    return Err(OpError::thrown(scope, error));
  }
  Ok(number)
}

fn start_inflight(
  scope: &mut v8::PinScope,
  stream: u64,
  chunk: v8::Local<v8::Value>,
  resolver: v8::Local<v8::PromiseResolver>,
  size: f64,
  count_size: bool,
) {
  let write_id = alloc();
  INFLIGHT.with(|slot| {
    slot.borrow_mut().insert(
      write_id,
      Inflight {
        stream,
        resolver: v8::Global::new(scope, resolver),
        size,
      },
    );
  });
  WRITES.with(|slot| {
    if let Some(state) = slot.borrow_mut().get_mut(&stream) {
      if count_size {
        state.queued += size;
      }
      state.in_flight += 1;
    }
  });
  match apply_sink(scope, stream, chunk, false) {
    Ok(value) => {
      let _ = watch_promise(scope, value, Token::Write(write_id));
    }
    Err(error) => {
      let thrown = error_value(scope, &error);
      let thrown = yuzuko_core::op::root(scope, thrown);
      if let Some(inflight) =
        INFLIGHT.with(|slot| slot.borrow_mut().remove(&write_id))
      {
        reject_resolver(scope, inflight.resolver, thrown);
        note_write_finished(inflight.stream, inflight.size);
      }
      let phase =
        WRITES.with(|slot| slot.borrow().get(&stream).map(|state| state.phase));
      if phase == Some(Phase::Erroring) {
        maybe_finish_erroring(scope, stream);
      } else {
        let _ = error_write(scope, stream, thrown);
      }
    }
  }
  update_ready(scope, stream);
}

fn close_write(scope: &mut v8::PinScope, stream: u64) -> Js {
  let blocked = WRITES.with(|slot| {
    slot
      .borrow()
      .get(&stream)
      .map(|state| (state.phase, state.closing, state.error.clone()))
  });
  match blocked {
    Some((Phase::Readable | Phase::Erroring, false, _)) => {}
    _ => {
      let error = type_error(scope, "The stream is closed");
      return rejected(scope, error);
    }
  }
  let resolver = v8::PromiseResolver::new(scope).unwrap();
  let promise = resolver.get_promise(scope);
  let writer = WRITES.with(|slot| {
    let mut states = slot.borrow_mut();
    let state = states.get_mut(&stream)?;
    let writable = state.phase == Phase::Readable;
    state.closing = true;
    state.close_resolver = Some(v8::Global::new(scope, resolver));
    writable.then_some(state.writer).flatten()
  });
  if let Some(writer) = writer {
    resolve_ready(scope, writer);
  }
  maybe_finish_close(scope, stream);
  let value: v8::Local<v8::Value> = promise.into();
  Js(v8::Global::new(scope, value))
}

fn maybe_finish_close(scope: &mut v8::PinScope, stream: u64) {
  let should = WRITES.with(|slot| {
    let mut states = slot.borrow_mut();
    let Some(state) = states.get_mut(&stream) else {
      return false;
    };
    if !state.started
      || !state.closing
      || state.close_started
      || state.in_flight != 0
      || !state.queue.is_empty()
      || state.phase != Phase::Readable
    {
      return false;
    }
    state.close_started = true;
    true
  });
  if !should {
    return;
  }
  let chunk = v8::undefined(scope).into();
  match apply_sink(scope, stream, chunk, true) {
    Ok(value) => {
      let _ = watch_promise(scope, value, Token::CloseWrite(stream));
    }
    Err(error) => {
      let thrown = error_value(scope, &error);
      fail_close_write(scope, stream, thrown);
    }
  }
}

fn finish_close_write(scope: &mut v8::PinScope, stream: u64) {
  let taken = WRITES.with(|slot| {
    let mut states = slot.borrow_mut();
    let state = states.get_mut(&stream)?;
    // An error raised while `close` is in flight is too late to stick.
    state.phase = Phase::Closed;
    state.error = None;
    state.close_started = false;
    Some((
      state.writer,
      state.close_resolver.take(),
      state.abort_wait.take(),
    ))
  });
  let Some((writer, close_resolver, abort_wait)) = taken else {
    return;
  };
  let value = v8::undefined(scope).into();
  if let Some(resolver) = close_resolver {
    resolve_resolver(scope, resolver, value);
  }
  if let Some(resolver) = abort_wait {
    resolve_resolver(scope, resolver, value);
  }
  if let Some(writer) = writer {
    resolve_ready(scope, writer);
    settle_writer(scope, writer, true, None);
  }
  close_transform_readable(scope, stream);
}

fn close_transform_readable(scope: &mut v8::PinScope, writable: u64) {
  let readable = WRITES.with(|slot| {
    slot
      .borrow()
      .get(&writable)
      .and_then(|state| match &state.sink {
        Sink::Transform { readable, .. } => Some(*readable),
        _ => None,
      })
  });
  if let Some(readable) = readable {
    let _ = close_stream(scope, readable);
  }
}

fn fail_close_write(
  scope: &mut v8::PinScope,
  stream: u64,
  error: v8::Local<v8::Value>,
) {
  let taken = WRITES.with(|slot| {
    let mut states = slot.borrow_mut();
    let state = states.get_mut(&stream)?;
    state.close_started = false;
    Some((
      state.close_resolver.take(),
      state.abort_wait.take(),
      state.phase,
      state.in_flight,
    ))
  });
  let Some((close_resolver, abort_wait, phase, in_flight)) = taken else {
    return;
  };
  let error = yuzuko_core::op::root(scope, error);
  if let Some(resolver) = close_resolver {
    reject_resolver(scope, resolver, error);
  }
  if let Some(resolver) = abort_wait {
    reject_resolver(scope, resolver, error);
  }
  if phase == Phase::Erroring && in_flight == 0 {
    finish_erroring(scope, stream);
  } else if phase != Phase::Erroring {
    let _ = error_write(scope, stream, error);
  }
}

fn fresh_abort_controller(
  scope: &mut v8::PinScope,
) -> Option<v8::Global<v8::Object>> {
  let global = scope.get_current_context().global(scope);
  let key = v8::String::new(scope, "AbortController")?;
  let ctor = global
    .get(scope, key.into())
    .and_then(|value| v8::Local::<v8::Function>::try_from(value).ok())?;
  let controller = ctor.new_instance(scope, &[])?;
  Some(v8::Global::new(scope, controller))
}

fn abort_controller_signal(
  scope: &mut v8::PinScope,
  stream: u64,
  reason: v8::Local<v8::Value>,
) {
  let controller = WRITES.with(|slot| {
    slot
      .borrow()
      .get(&stream)
      .and_then(|state| state.abort_controller.clone())
  });
  let Some(controller) = controller else {
    return;
  };
  let controller = v8::Local::new(scope, &controller);
  let controller = yuzuko_core::op::root(scope, controller);
  let key = v8::String::new(scope, "abort").unwrap();
  let Some(function) = controller
    .get(scope, key.into())
    .and_then(|value| v8::Local::<v8::Function>::try_from(value).ok())
  else {
    return;
  };
  let function = yuzuko_core::op::root(scope, function);
  let function = v8::Global::new(scope, function);
  let _ = call(scope, &function, controller.into(), &[reason]);
}

fn stored_abort_promise(scope: &mut v8::PinScope, stream: u64) -> Option<Js> {
  let promise = WRITES.with(|slot| {
    slot
      .borrow()
      .get(&stream)
      .and_then(|state| state.abort_promise.clone())
  })?;
  let promise = v8::Local::new(scope, &promise);
  let value: v8::Local<v8::Value> = promise.into();
  Some(Js(v8::Global::new(scope, value)))
}

fn abort_write(
  scope: &mut v8::PinScope,
  stream: u64,
  reason: v8::Local<v8::Value>,
) -> Js {
  let finished = WRITES.with(|slot| {
    slot.borrow().get(&stream).is_some_and(|state| {
      state.phase == Phase::Errored || state.phase == Phase::Closed
    })
  });
  if finished {
    let value = v8::undefined(scope).into();
    return resolved(scope, value);
  }
  if let Some(promise) = stored_abort_promise(scope, stream) {
    return promise;
  }
  let reason = yuzuko_core::op::root(scope, reason);
  // Listeners run now and may abort or close again before this call
  // stores its own promise.
  abort_controller_signal(scope, stream, reason);
  let reason = yuzuko_core::op::root(scope, reason);
  if let Some(promise) = stored_abort_promise(scope, stream) {
    return promise;
  }
  let finished = WRITES.with(|slot| {
    slot.borrow().get(&stream).is_some_and(|state| {
      state.phase == Phase::Errored || state.phase == Phase::Closed
    })
  });
  if finished {
    let value = v8::undefined(scope).into();
    return resolved(scope, value);
  }
  let erroring = WRITES.with(|slot| {
    slot
      .borrow()
      .get(&stream)
      .is_some_and(|state| state.phase == Phase::Erroring)
  });
  if erroring {
    return abort_erroring(scope, stream);
  }
  let close_started = WRITES.with(|slot| {
    slot
      .borrow()
      .get(&stream)
      .is_some_and(|state| state.close_started)
  });
  if close_started {
    let _ = error_write(scope, stream, reason);
    return abort_erroring(scope, stream);
  }
  let defer = WRITES.with(|slot| {
    slot
      .borrow()
      .get(&stream)
      .is_some_and(|state| state.in_flight > 0 || !state.started)
  });
  if defer {
    let _ = error_write(scope, stream, reason);
    return defer_sink_abort(scope, stream, reason);
  }
  let is_transform = WRITES.with(|slot| {
    slot
      .borrow()
      .get(&stream)
      .is_some_and(|state| matches!(state.sink, Sink::Transform { .. }))
  });
  if is_transform {
    let _ = error_write(scope, stream, reason);
    let promise = transform_sink_abort(scope, stream, reason);
    WRITES.with(|slot| {
      if let Some(state) = slot.borrow_mut().get_mut(&stream) {
        state.abort_promise = Some(promise.clone());
      }
    });
    let promise = v8::Local::new(scope, &promise);
    let value: v8::Local<v8::Value> = promise.into();
    return Js(v8::Global::new(scope, value));
  }
  let abort_fn = WRITES.with(|slot| {
    slot
      .borrow()
      .get(&stream)
      .and_then(|state| match &state.sink {
        Sink::Js { abort, .. } => abort.clone(),
        _ => None,
      })
  });
  let _ = error_write(scope, stream, reason);
  let resolver = v8::PromiseResolver::new(scope).unwrap();
  let promise = resolver.get_promise(scope);
  WRITES.with(|slot| {
    if let Some(state) = slot.borrow_mut().get_mut(&stream) {
      state.abort_promise = Some(v8::Global::new(scope, promise));
    }
  });
  let Some(function) = abort_fn else {
    let value = v8::undefined(scope).into();
    forget_abort_promise(scope, stream);
    resolver.resolve(scope, value);
    let value: v8::Local<v8::Value> = promise.into();
    return Js(v8::Global::new(scope, value));
  };
  match call_method(scope, &function, &[reason]) {
    Ok(value) => {
      let _pending = settle_abort_now(scope, stream, resolver, value);
      let value: v8::Local<v8::Value> = promise.into();
      Js(v8::Global::new(scope, value))
    }
    Err(error) => {
      let thrown = error_value(scope, &error);
      forget_abort_promise(scope, stream);
      reject_resolver(scope, v8::Global::new(scope, resolver), thrown);
      let value: v8::Local<v8::Value> = promise.into();
      Js(v8::Global::new(scope, value))
    }
  }
}

/// Settles the abort promise now when `value` is an already settled
/// promise. A pending promise, thenable, or plain value is watched.
/// Returns true when the abort promise is still pending.
fn settle_abort_now(
  scope: &mut v8::PinScope,
  stream: u64,
  resolver: v8::Local<v8::PromiseResolver>,
  value: v8::Local<v8::Value>,
) -> bool {
  if let Ok(promise) = v8::Local::<v8::Promise>::try_from(value) {
    match promise.state() {
      v8::PromiseState::Fulfilled => {
        promise.mark_as_handled();
        forget_abort_promise(scope, stream);
        let undefined = v8::undefined(scope).into();
        resolver.resolve(scope, undefined);
        return false;
      }
      v8::PromiseState::Rejected => {
        let error = v8::Global::new(scope, promise.result(scope));
        promise.mark_as_handled();
        forget_abort_promise(scope, stream);
        let error = v8::Local::new(scope, &error);
        reject_resolver(scope, v8::Global::new(scope, resolver), error);
        return false;
      }
      v8::PromiseState::Pending => {}
    }
  }
  let id = alloc();
  ABORTS.with(|slot| {
    slot.borrow_mut().insert(
      id,
      AbortFlight {
        stream,
        resolver: v8::Global::new(scope, resolver),
      },
    );
  });
  let _ = watch_promise(scope, value, Token::AbortWrite(id));
  true
}

fn forget_abort_promise(scope: &mut v8::PinScope, stream: u64) {
  let value = v8::undefined(scope).into();
  let _ = watch_promise(scope, value, Token::AbortForget(stream));
}

fn settle_abort(
  scope: &mut v8::PinScope,
  id: u64,
  result: Result<v8::Local<v8::Value>, v8::Local<v8::Value>>,
) {
  let flight = ABORTS.with(|slot| slot.borrow_mut().remove(&id));
  let Some(flight) = flight else {
    return;
  };
  forget_abort_promise(scope, flight.stream);
  match result {
    Ok(_) => {
      let value = v8::undefined(scope).into();
      resolve_resolver(scope, flight.resolver, value);
    }
    Err(error) => {
      reject_resolver(scope, flight.resolver, error);
    }
  }
  let stored = WRITES.with(|slot| {
    slot.borrow().get(&flight.stream).and_then(|state| {
      (state.phase == Phase::Erroring)
        .then(|| state.error.clone())
        .flatten()
    })
  });
  if let Some(stored) = stored {
    become_errored(scope, flight.stream, stored);
  }
}

fn error_write(
  scope: &mut v8::PinScope,
  stream: u64,
  error: v8::Local<v8::Value>,
) -> Result<(), OpError> {
  let snapshot = WRITES.with(|slot| {
    slot.borrow().get(&stream).map(|state| {
      (
        state.phase,
        state.started,
        state.in_flight,
        state.close_started,
      )
    })
  });
  let Some((phase, started, in_flight, close_started)) = snapshot else {
    return Ok(());
  };
  if phase != Phase::Readable {
    return Ok(());
  }
  let stored = v8::Global::new(scope, error);
  if !started || in_flight > 0 || close_started {
    let writer = WRITES.with(|slot| {
      let mut states = slot.borrow_mut();
      let state = states.get_mut(&stream)?;
      state.phase = Phase::Erroring;
      state.error = Some(stored.clone());
      state.writer
    });
    if let Some(writer) = writer {
      let thrown = v8::Local::new(scope, &stored);
      let thrown = yuzuko_core::op::root(scope, thrown);
      reject_ready(scope, writer, thrown);
    }
    return Ok(());
  }
  become_errored(scope, stream, stored);
  Ok(())
}

fn defer_sink_abort(
  scope: &mut v8::PinScope,
  stream: u64,
  reason: v8::Local<v8::Value>,
) -> Js {
  let resolver = v8::PromiseResolver::new(scope).unwrap();
  let promise = resolver.get_promise(scope);
  WRITES.with(|slot| {
    if let Some(state) = slot.borrow_mut().get_mut(&stream) {
      state.abort_wait = Some(v8::Global::new(scope, resolver));
      state.abort_reason = Some(v8::Global::new(scope, reason));
      state.abort_promise = Some(v8::Global::new(scope, promise));
    }
  });
  let value: v8::Local<v8::Value> = promise.into();
  Js(v8::Global::new(scope, value))
}

fn abort_erroring(scope: &mut v8::PinScope, stream: u64) -> Js {
  let existing = WRITES.with(|slot| {
    slot
      .borrow()
      .get(&stream)
      .and_then(|state| state.abort_wait.clone())
  });
  if let Some(resolver) = existing {
    let resolver = v8::Local::new(scope, &resolver);
    let promise = resolver.get_promise(scope);
    let value: v8::Local<v8::Value> = promise.into();
    return Js(v8::Global::new(scope, value));
  }
  let resolver = v8::PromiseResolver::new(scope).unwrap();
  let promise = resolver.get_promise(scope);
  WRITES.with(|slot| {
    if let Some(state) = slot.borrow_mut().get_mut(&stream) {
      state.abort_wait = Some(v8::Global::new(scope, resolver));
    }
  });
  let value: v8::Local<v8::Value> = promise.into();
  Js(v8::Global::new(scope, value))
}

fn reject_queued_writes(
  scope: &mut v8::PinScope,
  stream: u64,
  stored: &v8::Global<v8::Value>,
) {
  let resolvers = WRITES.with(|slot| {
    let mut states = slot.borrow_mut();
    let state = states.get_mut(&stream)?;
    state.queued = 0.0;
    Some(
      state
        .queue
        .drain(..)
        .map(|item| item.resolver)
        .collect::<Vec<_>>(),
    )
  });
  let Some(resolvers) = resolvers else {
    return;
  };
  let thrown = v8::Local::new(scope, stored);
  let thrown = yuzuko_core::op::root(scope, thrown);
  for resolver in resolvers {
    reject_resolver(scope, resolver, thrown);
  }
}

fn finish_erroring(scope: &mut v8::PinScope, stream: u64) {
  let taken = WRITES.with(|slot| {
    let mut states = slot.borrow_mut();
    let state = states.get_mut(&stream)?;
    if state.phase != Phase::Erroring {
      return None;
    }
    let error = state.error.clone()?;
    let wait = state.abort_wait.take();
    let reason = state.abort_reason.take();
    Some((error, wait, reason))
  });
  let Some((stored, wait, reason)) = taken else {
    return;
  };
  // Queued writes reject before the abort promise. `writer.closed` waits
  // until sink abort settles.
  reject_queued_writes(scope, stream, &stored);
  if let Some(reason) = reason {
    let reason = v8::Local::new(scope, &reason);
    let reason = yuzuko_core::op::root(scope, reason);
    let pending = call_deferred_abort(scope, stream, reason, wait);
    if !pending {
      become_errored(scope, stream, stored);
    }
    return;
  }
  if let Some(resolver) = wait {
    let error = v8::Local::new(scope, &stored);
    let error = yuzuko_core::op::root(scope, error);
    reject_resolver(scope, resolver, error);
  }
  become_errored(scope, stream, stored);
}

fn call_deferred_abort(
  scope: &mut v8::PinScope,
  stream: u64,
  reason: v8::Local<v8::Value>,
  wait: Option<v8::Global<v8::PromiseResolver>>,
) -> bool {
  let is_transform = WRITES.with(|slot| {
    slot
      .borrow()
      .get(&stream)
      .is_some_and(|state| matches!(state.sink, Sink::Transform { .. }))
  });
  if is_transform {
    let promise = transform_sink_abort(scope, stream, reason);
    let promise = v8::Local::new(scope, &promise);
    let value: v8::Local<v8::Value> = promise.into();
    let Some(wait) = wait else {
      return false;
    };
    let resolver = v8::Local::new(scope, &wait);
    return settle_abort_now(scope, stream, resolver, value);
  }
  let abort_fn = WRITES.with(|slot| {
    slot
      .borrow()
      .get(&stream)
      .and_then(|state| match &state.sink {
        Sink::Js { abort, .. } => abort.clone(),
        _ => None,
      })
  });
  let Some(wait) = wait else {
    if let Some(function) = abort_fn {
      let _ = call_method(scope, &function, &[reason]);
    }
    return false;
  };
  let Some(function) = abort_fn else {
    let value = v8::undefined(scope).into();
    let id = alloc();
    ABORTS.with(|slot| {
      slot.borrow_mut().insert(
        id,
        AbortFlight {
          stream,
          resolver: wait,
        },
      );
    });
    let _ = watch_promise(scope, value, Token::AbortWrite(id));
    return true;
  };
  match call_method(scope, &function, &[reason]) {
    Ok(value) => {
      let resolver = v8::Local::new(scope, &wait);
      settle_abort_now(scope, stream, resolver, value)
    }
    Err(error) => {
      let thrown = error_value(scope, &error);
      reject_resolver(scope, wait, thrown);
      false
    }
  }
}

fn become_errored(
  scope: &mut v8::PinScope,
  stream: u64,
  stored: v8::Global<v8::Value>,
) {
  let (writer, mut resolvers) = WRITES.with(|slot| {
    let mut states = slot.borrow_mut();
    let Some(state) = states.get_mut(&stream) else {
      return (None, Vec::new());
    };
    state.phase = Phase::Errored;
    state.error = Some(stored.clone());
    let mut resolvers = state
      .queue
      .drain(..)
      .map(|item| item.resolver)
      .collect::<Vec<_>>();
    if let Some(resolver) = state.close_resolver.take() {
      resolvers.push(resolver);
    }
    (state.writer, resolvers)
  });
  let inflight = INFLIGHT.with(|slot| {
    let mut flights = slot.borrow_mut();
    let ids = flights
      .iter()
      .filter(|(_, item)| item.stream == stream)
      .map(|(id, _)| *id)
      .collect::<Vec<_>>();
    ids
      .into_iter()
      .filter_map(|id| flights.remove(&id).map(|item| item.resolver))
      .collect::<Vec<_>>()
  });
  resolvers.extend(inflight);
  let thrown = v8::Local::new(scope, &stored);
  let thrown = yuzuko_core::op::root(scope, thrown);
  // ready, then queued writes, then closed.
  if let Some(writer) = writer {
    reject_ready(scope, writer, thrown);
  }
  for resolver in resolvers {
    reject_resolver(scope, resolver, thrown);
  }
  if let Some(writer) = writer {
    settle_writer(scope, writer, false, Some(stored));
  }
}

fn settle_writer(
  scope: &mut v8::PinScope,
  writer: u64,
  ok: bool,
  error: Option<v8::Global<v8::Value>>,
) {
  let resolver = WRITERS.with(|slot| {
    let mut writers = slot.borrow_mut();
    let state = writers.get_mut(&writer)?;
    if state.settled {
      return None;
    }
    state.settled = true;
    Some(state.closed_resolver.clone())
  });
  let Some(resolver) = resolver else {
    return;
  };
  let resolver = v8::Local::new(scope, &resolver);
  let resolver = yuzuko_core::op::root(scope, resolver);
  if ok {
    let value = v8::undefined(scope).into();
    resolver.resolve(scope, value);
  } else {
    let error = match error {
      Some(error) => {
        let error = v8::Local::new(scope, &error);
        yuzuko_core::op::root(scope, error)
      }
      None => type_error(scope, "The stream is errored"),
    };
    resolver.reject(scope, error);
    mark_writer_promise(scope, writer, true);
  }
}

fn mark_writer_promise(scope: &mut v8::PinScope, writer: u64, closed: bool) {
  let promise = WRITERS.with(|slot| {
    slot.borrow().get(&writer).map(|state| {
      if closed {
        state.closed.clone()
      } else {
        state.ready.clone()
      }
    })
  });
  if let Some(promise) = promise {
    let promise = v8::Local::new(scope, &promise);
    promise.mark_as_handled();
  }
}

fn mark_write_started(scope: &mut v8::PinScope, id: u64) {
  let erroring = WRITES.with(|slot| {
    let mut states = slot.borrow_mut();
    let state = states.get_mut(&id)?;
    state.started = true;
    Some(state.phase == Phase::Erroring)
  });
  if erroring == Some(true) {
    finish_erroring(scope, id);
    return;
  }
  pump_writes(scope, id);
  update_ready(scope, id);
}

fn pump_writes(scope: &mut v8::PinScope, id: u64) {
  let next = WRITES.with(|slot| {
    let mut states = slot.borrow_mut();
    let state = states.get_mut(&id)?;
    if !state.started || state.in_flight > 0 || state.phase != Phase::Readable {
      return None;
    }
    state.queue.pop_front()
  });
  let Some(item) = next else {
    maybe_finish_close(scope, id);
    return;
  };
  let chunk = v8::Local::new(scope, &item.chunk);
  let chunk = yuzuko_core::op::root(scope, chunk);
  let resolver = v8::Local::new(scope, &item.resolver);
  let resolver = yuzuko_core::op::root(scope, resolver);
  start_inflight(scope, id, chunk, resolver, item.size, false);
}

fn finish_write(
  scope: &mut v8::PinScope,
  write_id: u64,
  result: Result<v8::Local<v8::Value>, v8::Local<v8::Value>>,
) {
  let Some(inflight) =
    INFLIGHT.with(|slot| slot.borrow_mut().remove(&write_id))
  else {
    return;
  };
  let phase = WRITES
    .with(|slot| slot.borrow().get(&inflight.stream).map(|state| state.phase));
  if phase == Some(Phase::Errored) {
    return;
  }
  match result {
    Err(error) => {
      reject_resolver(scope, inflight.resolver, error);
      note_write_finished(inflight.stream, inflight.size);
      if phase == Some(Phase::Erroring) {
        maybe_finish_erroring(scope, inflight.stream);
      } else {
        let _ = error_write(scope, inflight.stream, error);
      }
    }
    Ok(_) => {
      let value = v8::undefined(scope).into();
      resolve_resolver(scope, inflight.resolver, value);
      note_write_finished(inflight.stream, inflight.size);
      if phase == Some(Phase::Erroring) {
        maybe_finish_erroring(scope, inflight.stream);
        return;
      }
      update_ready(scope, inflight.stream);
      pump_writes(scope, inflight.stream);
    }
  }
}

fn note_write_finished(stream: u64, size: f64) {
  WRITES.with(|slot| {
    if let Some(state) = slot.borrow_mut().get_mut(&stream) {
      state.in_flight = state.in_flight.saturating_sub(1);
      state.queued = (state.queued - size).max(0.0);
    }
  });
}

fn maybe_finish_erroring(scope: &mut v8::PinScope, stream: u64) {
  let ready = WRITES.with(|slot| {
    slot.borrow().get(&stream).is_some_and(|state| {
      state.phase == Phase::Erroring
        && state.in_flight == 0
        && !state.close_started
    })
  });
  if ready {
    finish_erroring(scope, stream);
  }
}

fn update_ready(scope: &mut v8::PinScope, stream: u64) {
  let snapshot = WRITES.with(|slot| {
    slot.borrow().get(&stream).map(|state| {
      (
        state.writer,
        state.phase,
        state.queued >= state.high_water,
        state.closing || state.close_started,
      )
    })
  });
  let Some((Some(writer), Phase::Readable, backpressure, closing)) = snapshot
  else {
    return;
  };
  if closing {
    return;
  }
  if backpressure {
    ensure_pending_ready(scope, writer);
  } else {
    resolve_ready(scope, writer);
  }
}

fn resolve_ready(scope: &mut v8::PinScope, writer: u64) {
  let resolver = WRITERS.with(|slot| {
    slot
      .borrow_mut()
      .get_mut(&writer)
      .and_then(|state| state.ready_resolver.take())
  });
  let Some(resolver) = resolver else {
    return;
  };
  let value = v8::undefined(scope).into();
  resolve_resolver(scope, resolver, value);
}

fn ensure_pending_ready(scope: &mut v8::PinScope, writer: u64) {
  let pending = WRITERS.with(|slot| {
    slot
      .borrow()
      .get(&writer)
      .is_some_and(|state| state.ready_resolver.is_some())
  });
  if pending {
    return;
  }
  let resolver = v8::PromiseResolver::new(scope).unwrap();
  let promise = resolver.get_promise(scope);
  WRITERS.with(|slot| {
    if let Some(state) = slot.borrow_mut().get_mut(&writer) {
      state.ready = v8::Global::new(scope, promise);
      state.ready_resolver = Some(v8::Global::new(scope, resolver));
    }
  });
}

fn reject_ready(
  scope: &mut v8::PinScope,
  writer: u64,
  error: v8::Local<v8::Value>,
) {
  let pending = WRITERS.with(|slot| {
    slot
      .borrow_mut()
      .get_mut(&writer)
      .and_then(|state| state.ready_resolver.take())
  });
  if let Some(resolver) = pending {
    reject_resolver(scope, resolver, error);
    mark_writer_promise(scope, writer, false);
    return;
  }
  let resolver = v8::PromiseResolver::new(scope).unwrap();
  let promise = resolver.get_promise(scope);
  resolver.reject(scope, error);
  promise.mark_as_handled();
  WRITERS.with(|slot| {
    if let Some(state) = slot.borrow_mut().get_mut(&writer) {
      state.ready = v8::Global::new(scope, promise);
    }
  });
}

fn resolve_resolver(
  scope: &mut v8::PinScope,
  resolver: v8::Global<v8::PromiseResolver>,
  value: v8::Local<v8::Value>,
) {
  let resolver = v8::Local::new(scope, &resolver);
  let resolver = yuzuko_core::op::root(scope, resolver);
  resolver.resolve(scope, value);
}

fn reject_resolver(
  scope: &mut v8::PinScope,
  resolver: v8::Global<v8::PromiseResolver>,
  error: v8::Local<v8::Value>,
) {
  let resolver = v8::Local::new(scope, &resolver);
  let resolver = yuzuko_core::op::root(scope, resolver);
  resolver.reject(scope, error);
}

fn apply_sink<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  stream: u64,
  chunk: v8::Local<v8::Value>,
  finish: bool,
) -> Result<v8::Local<'s, v8::Value>, OpError> {
  let kind = WRITES.with(|slot| {
    slot.borrow().get(&stream).map(|state| match &state.sink {
      Sink::Js { .. } => 0,
      Sink::Transform { .. } => 1,
      Sink::Utf8Encode { .. } => 2,
      Sink::Utf8Decode { .. } => 3,
      Sink::Codec { .. } => 4,
    })
  });
  match kind {
    Some(0) => js_sink(scope, stream, chunk, finish),
    Some(1) => transform_sink(scope, stream, chunk, finish),
    Some(2) => utf8_encode_sink(scope, stream, chunk, finish),
    Some(3) => utf8_decode_sink(scope, stream, chunk, finish),
    Some(4) => codec_sink(scope, stream, chunk, finish),
    _ => Ok(v8::undefined(scope).into()),
  }
}

fn js_sink<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  stream: u64,
  chunk: v8::Local<v8::Value>,
  finish: bool,
) -> Result<v8::Local<'s, v8::Value>, OpError> {
  let function = WRITES.with(|slot| {
    slot
      .borrow()
      .get(&stream)
      .and_then(|state| match &state.sink {
        Sink::Js { write, close, .. } => {
          if finish {
            close.clone()
          } else {
            write.clone()
          }
        }
        _ => None,
      })
  });
  let Some(function) = function else {
    return Ok(v8::undefined(scope).into());
  };
  if finish {
    call_method(scope, &function, &[])
  } else {
    let controller = writable_controller(scope, stream);
    call_method(scope, &function, &[chunk, controller])
  }
}

fn writable_controller<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  stream: u64,
) -> v8::Local<'s, v8::Value> {
  let controller = unsafe {
    yuzuko_core::op::wrap_instance(
      scope,
      WritableStreamDefaultController { stream },
    )
  };
  let controller: v8::Local<v8::Value> = controller.into();
  yuzuko_core::op::root(scope, controller)
}

fn transform_sink<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  stream: u64,
  chunk: v8::Local<v8::Value>,
  finish: bool,
) -> Result<v8::Local<'s, v8::Value>, OpError> {
  if finish {
    transform_close(scope, stream)
  } else {
    transform_write(scope, stream, chunk)
  }
}

fn transform_controller<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  readable: u64,
  writable: u64,
) -> v8::Local<'s, v8::Value> {
  let controller = unsafe {
    yuzuko_core::op::wrap_instance(
      scope,
      TransformStreamDefaultController { readable, writable },
    )
  };
  let controller: v8::Local<v8::Value> = controller.into();
  yuzuko_core::op::root(scope, controller)
}

fn arm_transform(
  scope: &mut v8::PinScope,
  readable: u64,
  writable: u64,
  readable_hwm: f64,
  readable_size: Option<v8::Global<v8::Function>>,
  writable_hwm: f64,
  writable_size: Option<v8::Global<v8::Function>>,
  transform: Option<v8::Global<v8::Function>>,
  flush: Option<v8::Global<v8::Function>>,
  cancel: Option<v8::Global<v8::Function>>,
) {
  READS.with(|slot| {
    if let Some(state) = slot.borrow_mut().get_mut(&readable) {
      state.high_water = readable_hwm;
      state.size_fn = readable_size;
      state.started = false;
      state.paired = Some(writable);
    }
  });
  WRITES.with(|slot| {
    if let Some(state) = slot.borrow_mut().get_mut(&writable) {
      state.high_water = writable_hwm;
      state.size_fn = writable_size;
      state.started = false;
    }
  });
  TRANSFORMS.with(|slot| {
    slot.borrow_mut().insert(
      writable,
      TransformLink {
        readable,
        backpressure: None,
        change_resolver: None,
        change_promise: None,
        transform,
        flush,
        cancel,
        cleared: false,
        finish_resolver: None,
        finish_promise: None,
        finish_reason: None,
        finish_kind: TransformFinish::None,
        cancel_errored: false,
      },
    );
  });
  // Writes block until `pull` or an error clears the initial backpressure.
  set_transform_backpressure(scope, writable, true);
}

fn reject_transform_type(
  scope: &mut v8::PinScope,
  transformer: Option<v8::Local<v8::Value>>,
  name: &str,
) -> Result<(), OpError> {
  let Some(transformer) =
    transformer.filter(|value| !value.is_null_or_undefined())
  else {
    return Ok(());
  };
  let object = object_value(scope, transformer)?;
  if property(scope, object, name)?.is_some() {
    return Err(OpError::range(format!("Invalid {name} specified")));
  }
  Ok(())
}

fn transform_readable(writable: u64) -> Option<u64> {
  TRANSFORMS.with(|slot| slot.borrow().get(&writable).map(|link| link.readable))
}

fn transform_writable(readable: u64) -> Option<u64> {
  READS.with(|slot| slot.borrow().get(&readable).and_then(|state| state.paired))
}

fn set_transform_backpressure(
  scope: &mut v8::PinScope,
  writable: u64,
  backpressure: bool,
) {
  let resolver = v8::PromiseResolver::new(scope).unwrap();
  let promise = resolver.get_promise(scope);
  let previous = TRANSFORMS.with(|slot| {
    let mut links = slot.borrow_mut();
    let link = links.get_mut(&writable)?;
    let previous = link.change_resolver.take();
    link.change_resolver = Some(v8::Global::new(scope, resolver));
    link.change_promise = Some(v8::Global::new(scope, promise));
    link.backpressure = Some(backpressure);
    previous
  });
  if let Some(previous) = previous {
    let value = v8::undefined(scope).into();
    resolve_resolver(scope, previous, value);
  }
}

fn transform_backpressure(writable: u64) -> bool {
  TRANSFORMS.with(|slot| {
    slot
      .borrow()
      .get(&writable)
      .and_then(|link| link.backpressure)
      .unwrap_or(true)
  })
}

fn transform_change_promise<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  writable: u64,
) -> v8::Local<'s, v8::Value> {
  let promise = TRANSFORMS.with(|slot| {
    slot
      .borrow()
      .get(&writable)
      .and_then(|link| link.change_promise.clone())
  });
  match promise {
    Some(promise) => {
      let promise = v8::Local::new(scope, &promise);
      promise.into()
    }
    None => v8::undefined(scope).into(),
  }
}

fn unblock_transform_write(scope: &mut v8::PinScope, writable: u64) {
  if transform_backpressure(writable) {
    set_transform_backpressure(scope, writable, false);
  }
}

fn clear_transform_algorithms(writable: u64) {
  TRANSFORMS.with(|slot| {
    if let Some(link) = slot.borrow_mut().get_mut(&writable) {
      link.transform = None;
      link.flush = None;
      link.cancel = None;
      link.cleared = true;
    }
  });
}

fn readable_stored_error<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  readable: u64,
) -> Option<v8::Local<'s, v8::Value>> {
  let error = READS.with(|slot| {
    slot
      .borrow()
      .get(&readable)
      .and_then(|state| state.error.clone())
  })?;
  let error = v8::Local::new(scope, &error);
  Some(yuzuko_core::op::root(scope, error))
}

fn writable_stored_error<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  writable: u64,
) -> Option<v8::Local<'s, v8::Value>> {
  let error = WRITES.with(|slot| {
    slot
      .borrow()
      .get(&writable)
      .and_then(|state| state.error.clone())
  })?;
  let error = v8::Local::new(scope, &error);
  Some(yuzuko_core::op::root(scope, error))
}

fn transform_error(
  scope: &mut v8::PinScope,
  writable: u64,
  reason: v8::Local<v8::Value>,
) {
  let reason = yuzuko_core::op::root(scope, reason);
  if let Some(readable) = transform_readable(writable) {
    let _ = error_stream(scope, readable, reason);
  }
  clear_transform_algorithms(writable);
  let _ = error_write(scope, writable, reason);
  unblock_transform_write(scope, writable);
}

fn transform_terminate(scope: &mut v8::PinScope, writable: u64) {
  if let Some(readable) = transform_readable(writable) {
    let _ = close_stream(scope, readable);
  }
  let error = type_error(scope, "The transform stream has terminated");
  let error = yuzuko_core::op::root(scope, error);
  clear_transform_algorithms(writable);
  let _ = error_write(scope, writable, error);
  unblock_transform_write(scope, writable);
}

fn can_close_or_enqueue(readable: u64) -> bool {
  READS.with(|slot| {
    slot.borrow().get(&readable).is_some_and(|state| {
      state.phase == Phase::Readable && !state.close_requested
    })
  })
}

fn transform_has_backpressure(readable: u64) -> bool {
  !should_pull_transform(readable)
}

fn should_pull_transform(readable: u64) -> bool {
  if !can_close_or_enqueue(readable) {
    return false;
  }
  let started = READS.with(|slot| {
    slot
      .borrow()
      .get(&readable)
      .is_some_and(|state| state.started)
  });
  if !started {
    return false;
  }
  if read_pending(readable) {
    return true;
  }
  desired_size(readable).is_some_and(|size| size > 0.0)
}

fn sync_transform_backpressure(scope: &mut v8::PinScope, writable: u64) {
  let Some(readable) = transform_readable(writable) else {
    return;
  };
  let has = transform_has_backpressure(readable);
  let current = TRANSFORMS.with(|slot| {
    slot
      .borrow()
      .get(&writable)
      .and_then(|link| link.backpressure)
  });
  if current != Some(has) && has {
    set_transform_backpressure(scope, writable, true);
  }
}

fn transform_enqueue(
  scope: &mut v8::PinScope,
  writable: u64,
  chunk: v8::Local<v8::Value>,
) -> Result<(), OpError> {
  let readable = transform_readable(writable)
    .ok_or_else(|| OpError::type_("The stream is closed"))?;
  if !can_close_or_enqueue(readable) {
    return Err(OpError::type_("The stream is closed"));
  }
  if let Err(error) = enqueue_chunk(scope, readable, chunk, None) {
    let thrown = error_value(scope, &error);
    let thrown = yuzuko_core::op::root(scope, thrown);
    clear_transform_algorithms(writable);
    let _ = error_write(scope, writable, thrown);
    unblock_transform_write(scope, writable);
    let stored = readable_stored_error(scope, readable).unwrap_or(thrown);
    return Err(OpError::thrown(scope, stored));
  }
  sync_transform_backpressure(scope, writable);
  Ok(())
}

fn transform_write<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  writable: u64,
  chunk: v8::Local<v8::Value>,
) -> Result<v8::Local<'s, v8::Value>, OpError> {
  if transform_backpressure(writable) {
    let waiting = transform_change_promise(scope, writable);
    let resolver = v8::PromiseResolver::new(scope).unwrap();
    let promise = resolver.get_promise(scope);
    let id = alloc();
    TRANSFORM_WAITS.with(|slot| {
      slot.borrow_mut().insert(
        id,
        TransformWait {
          writable,
          chunk: v8::Global::new(scope, chunk),
          resolver: v8::Global::new(scope, resolver),
        },
      );
    });
    let _ = watch_promise(scope, waiting, Token::TransformRelease(id));
    return Ok(promise.into());
  }
  perform_transform(scope, writable, chunk)
}

fn perform_transform<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  writable: u64,
  chunk: v8::Local<v8::Value>,
) -> Result<v8::Local<'s, v8::Value>, OpError> {
  let (function, cleared, readable) = TRANSFORMS
    .with(|slot| {
      slot
        .borrow()
        .get(&writable)
        .map(|link| (link.transform.clone(), link.cleared, link.readable))
    })
    .ok_or_else(|| OpError::type_("The stream is closed"))?;
  if cleared {
    return Ok(v8::undefined(scope).into());
  }
  let Some(function) = function else {
    transform_enqueue(scope, writable, chunk)?;
    return Ok(v8::undefined(scope).into());
  };
  let controller = transform_controller(scope, readable, writable);
  match call_method(scope, &function, &[chunk, controller]) {
    Ok(value) => Ok(guard_transform(scope, writable, value)),
    Err(error) => {
      let thrown = error_value(scope, &error);
      let thrown = yuzuko_core::op::root(scope, thrown);
      transform_error(scope, writable, thrown);
      Err(OpError::thrown(scope, thrown))
    }
  }
}

fn guard_transform<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  writable: u64,
  value: v8::Local<v8::Value>,
) -> v8::Local<'s, v8::Value> {
  let resolver = v8::PromiseResolver::new(scope).unwrap();
  let promise = resolver.get_promise(scope);
  let id = alloc();
  TRANSFORM_FORWARDS.with(|slot| {
    slot.borrow_mut().insert(
      id,
      TransformForward {
        writable,
        resolver: v8::Global::new(scope, resolver),
        guard: true,
      },
    );
  });
  let _ = watch_promise(scope, value, Token::TransformForward(id));
  promise.into()
}

fn forward_to_resolver(
  scope: &mut v8::PinScope,
  resolver: v8::Global<v8::PromiseResolver>,
  value: v8::Local<v8::Value>,
) {
  let id = alloc();
  TRANSFORM_FORWARDS.with(|slot| {
    slot.borrow_mut().insert(
      id,
      TransformForward {
        writable: 0,
        resolver,
        guard: false,
      },
    );
  });
  let _ = watch_promise(scope, value, Token::TransformForward(id));
}

fn transform_close<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  writable: u64,
) -> Result<v8::Local<'s, v8::Value>, OpError> {
  if let Some(promise) = take_finish_promise(scope, writable) {
    return Ok(promise);
  }
  begin_transform_finish(scope, writable, TransformFinish::Flush, None);
  let (flush, readable) = TRANSFORMS
    .with(|slot| {
      slot
        .borrow()
        .get(&writable)
        .map(|link| (link.flush.clone(), link.readable))
    })
    .ok_or_else(|| OpError::type_("The stream is closed"))?;
  let result = if let Some(flush) = flush {
    let controller = transform_controller(scope, readable, writable);
    call_method(scope, &flush, &[controller])
  } else {
    Ok(v8::undefined(scope).into())
  };
  clear_transform_algorithms(writable);
  let promise = finish_promise_value(scope, writable)
    .ok_or_else(|| OpError::type_("The stream is closed"))?;
  match result {
    Ok(value) => {
      let _ = watch_promise(scope, value, Token::TransformFinish(writable));
    }
    Err(error) => {
      let thrown = error_value(scope, &error);
      let thrown = yuzuko_core::op::root(scope, thrown);
      if let Some(readable) = transform_readable(writable) {
        let _ = error_stream(scope, readable, thrown);
      }
      reject_finish(scope, writable, thrown);
    }
  }
  Ok(promise)
}

fn transform_source_cancel(
  scope: &mut v8::PinScope,
  writable: u64,
  reason: v8::Local<v8::Value>,
) -> Js {
  if let Some(promise) = take_finish_promise(scope, writable) {
    return Js(v8::Global::new(scope, promise));
  }
  let reason = yuzuko_core::op::root(scope, reason);
  begin_transform_finish(
    scope,
    writable,
    TransformFinish::SourceCancel,
    Some(v8::Global::new(scope, reason)),
  );
  let cancel = TRANSFORMS.with(|slot| {
    slot
      .borrow()
      .get(&writable)
      .and_then(|link| link.cancel.clone())
  });
  let before =
    WRITES.with(|slot| slot.borrow().get(&writable).map(|state| state.phase));
  let result = if let Some(cancel) = cancel {
    call_method(scope, &cancel, &[reason])
  } else {
    Ok(v8::undefined(scope).into())
  };
  let after =
    WRITES.with(|slot| slot.borrow().get(&writable).map(|state| state.phase));
  // Only an error raised by the algorithm itself rejects `cancel()`.
  // `abort()` or `terminate()` beforehand leaves the writable erroring
  // without changing that promise.
  let cancel_errored = result.is_ok()
    && before == Some(Phase::Readable)
    && matches!(after, Some(Phase::Errored | Phase::Erroring));
  if cancel_errored {
    TRANSFORMS.with(|slot| {
      if let Some(link) = slot.borrow_mut().get_mut(&writable) {
        link.cancel_errored = true;
      }
    });
  }
  clear_transform_algorithms(writable);
  let promise = finish_promise_value(scope, writable)
    .unwrap_or_else(|| v8::undefined(scope).into());
  match result {
    Ok(value) => {
      let _ = watch_promise(scope, value, Token::TransformFinish(writable));
    }
    Err(error) => {
      let thrown = error_value(scope, &error);
      let thrown = yuzuko_core::op::root(scope, thrown);
      let _ = error_write(scope, writable, thrown);
      unblock_transform_write(scope, writable);
      reject_finish(scope, writable, thrown);
    }
  }
  Js(v8::Global::new(scope, promise))
}

fn transform_sink_abort(
  scope: &mut v8::PinScope,
  writable: u64,
  reason: v8::Local<v8::Value>,
) -> v8::Global<v8::Promise> {
  if let Some(existing) = TRANSFORMS.with(|slot| {
    slot
      .borrow()
      .get(&writable)
      .and_then(|link| link.finish_promise.clone())
  }) {
    return existing;
  }
  let reason = yuzuko_core::op::root(scope, reason);
  begin_transform_finish(
    scope,
    writable,
    TransformFinish::SinkAbort,
    Some(v8::Global::new(scope, reason)),
  );
  let cancel = TRANSFORMS.with(|slot| {
    slot
      .borrow()
      .get(&writable)
      .and_then(|link| link.cancel.clone())
  });
  let result = if let Some(cancel) = cancel {
    call_method(scope, &cancel, &[reason])
  } else {
    Ok(v8::undefined(scope).into())
  };
  clear_transform_algorithms(writable);
  match result {
    Ok(value) => {
      let _ = watch_promise(scope, value, Token::TransformFinish(writable));
    }
    Err(error) => {
      let thrown = error_value(scope, &error);
      let thrown = yuzuko_core::op::root(scope, thrown);
      if let Some(readable) = transform_readable(writable) {
        let _ = error_stream(scope, readable, thrown);
      }
      reject_finish(scope, writable, thrown);
    }
  }
  TRANSFORMS
    .with(|slot| {
      slot
        .borrow()
        .get(&writable)
        .and_then(|link| link.finish_promise.clone())
    })
    .unwrap_or_else(|| {
      let resolver = v8::PromiseResolver::new(scope).unwrap();
      let promise = resolver.get_promise(scope);
      let value = v8::undefined(scope).into();
      resolver.resolve(scope, value);
      v8::Global::new(scope, promise)
    })
}

fn begin_transform_finish(
  scope: &mut v8::PinScope,
  writable: u64,
  kind: TransformFinish,
  reason: Option<v8::Global<v8::Value>>,
) {
  let resolver = v8::PromiseResolver::new(scope).unwrap();
  let promise = resolver.get_promise(scope);
  TRANSFORMS.with(|slot| {
    if let Some(link) = slot.borrow_mut().get_mut(&writable) {
      link.finish_resolver = Some(v8::Global::new(scope, resolver));
      link.finish_promise = Some(v8::Global::new(scope, promise));
      link.finish_reason = reason;
      link.finish_kind = kind;
    }
  });
}

fn take_finish_promise<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  writable: u64,
) -> Option<v8::Local<'s, v8::Value>> {
  let promise = TRANSFORMS.with(|slot| {
    slot
      .borrow()
      .get(&writable)
      .and_then(|link| link.finish_promise.clone())
  })?;
  let promise = v8::Local::new(scope, &promise);
  Some(promise.into())
}

fn finish_promise_value<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  writable: u64,
) -> Option<v8::Local<'s, v8::Value>> {
  take_finish_promise(scope, writable)
}

fn reject_finish(
  scope: &mut v8::PinScope,
  writable: u64,
  error: v8::Local<v8::Value>,
) {
  let resolver = TRANSFORMS.with(|slot| {
    slot
      .borrow_mut()
      .get_mut(&writable)
      .and_then(|link| link.finish_resolver.take())
  });
  if let Some(resolver) = resolver {
    reject_resolver(scope, resolver, error);
  }
}

fn resolve_finish(scope: &mut v8::PinScope, writable: u64) {
  let resolver = TRANSFORMS.with(|slot| {
    slot
      .borrow_mut()
      .get_mut(&writable)
      .and_then(|link| link.finish_resolver.take())
  });
  if let Some(resolver) = resolver {
    let value = v8::undefined(scope).into();
    resolve_resolver(scope, resolver, value);
  }
}

fn on_transform_release(scope: &mut v8::PinScope, id: u64) {
  let Some(wait) = TRANSFORM_WAITS.with(|slot| slot.borrow_mut().remove(&id))
  else {
    return;
  };
  let phase = WRITES
    .with(|slot| slot.borrow().get(&wait.writable).map(|state| state.phase));
  if phase != Some(Phase::Readable) {
    let error = writable_stored_error(scope, wait.writable)
      .unwrap_or_else(|| type_error(scope, "The stream is errored"));
    reject_resolver(scope, wait.resolver, error);
    return;
  }
  let chunk = v8::Local::new(scope, &wait.chunk);
  let chunk = yuzuko_core::op::root(scope, chunk);
  match perform_transform(scope, wait.writable, chunk) {
    Ok(value) => forward_to_resolver(scope, wait.resolver, value),
    Err(error) => {
      let thrown = error_value(scope, &error);
      reject_resolver(scope, wait.resolver, thrown);
    }
  }
}

fn on_transform_forward(
  scope: &mut v8::PinScope,
  id: u64,
  result: Result<v8::Local<v8::Value>, v8::Local<v8::Value>>,
) {
  let Some(forward) =
    TRANSFORM_FORWARDS.with(|slot| slot.borrow_mut().remove(&id))
  else {
    return;
  };
  match result {
    Ok(_) => {
      let value = v8::undefined(scope).into();
      resolve_resolver(scope, forward.resolver, value);
    }
    Err(error) => {
      let error = yuzuko_core::op::root(scope, error);
      if forward.guard {
        transform_error(scope, forward.writable, error);
      }
      reject_resolver(scope, forward.resolver, error);
    }
  }
}

fn on_transform_finish(
  scope: &mut v8::PinScope,
  writable: u64,
  result: Result<v8::Local<v8::Value>, v8::Local<v8::Value>>,
) {
  let kind = TRANSFORMS.with(|slot| {
    slot
      .borrow()
      .get(&writable)
      .map(|link| link.finish_kind)
      .unwrap_or(TransformFinish::None)
  });
  match kind {
    TransformFinish::Flush => on_transform_flush(scope, writable, result),
    TransformFinish::SourceCancel => {
      on_transform_source_cancel(scope, writable, result)
    }
    TransformFinish::SinkAbort => {
      on_transform_sink_abort(scope, writable, result)
    }
    TransformFinish::None => {}
  }
}

fn on_transform_flush(
  scope: &mut v8::PinScope,
  writable: u64,
  result: Result<v8::Local<v8::Value>, v8::Local<v8::Value>>,
) {
  let readable = transform_readable(writable);
  match result {
    Ok(_) => {
      let errored = readable.is_some_and(|readable| {
        READS.with(|slot| {
          slot
            .borrow()
            .get(&readable)
            .is_some_and(|state| state.phase == Phase::Errored)
        })
      });
      if errored {
        let error = readable
          .and_then(|readable| readable_stored_error(scope, readable))
          .unwrap_or_else(|| type_error(scope, "The stream is errored"));
        reject_finish(scope, writable, error);
      } else {
        if let Some(readable) = readable {
          let _ = close_stream(scope, readable);
        }
        resolve_finish(scope, writable);
      }
    }
    Err(error) => {
      let error = yuzuko_core::op::root(scope, error);
      if let Some(readable) = readable {
        let _ = error_stream(scope, readable, error);
      }
      reject_finish(scope, writable, error);
    }
  }
}

fn on_transform_source_cancel(
  scope: &mut v8::PinScope,
  writable: u64,
  result: Result<v8::Local<v8::Value>, v8::Local<v8::Value>>,
) {
  let (phase, cancel_errored) = {
    let phase =
      WRITES.with(|slot| slot.borrow().get(&writable).map(|state| state.phase));
    let cancel_errored = TRANSFORMS.with(|slot| {
      slot
        .borrow()
        .get(&writable)
        .is_some_and(|link| link.cancel_errored)
    });
    (phase, cancel_errored)
  };
  match result {
    Ok(_) => {
      // `controller.error()` inside `cancel()` stores the reason before
      // the algorithm's promise settles. A later `terminate()` only
      // erroring the writable must not reject `cancel()`.
      let errored = phase == Some(Phase::Errored)
        || (phase == Some(Phase::Erroring) && cancel_errored);
      if errored {
        let error = writable_stored_error(scope, writable)
          .unwrap_or_else(|| type_error(scope, "The stream is errored"));
        reject_finish(scope, writable, error);
      } else {
        let reason = TRANSFORMS.with(|slot| {
          slot
            .borrow()
            .get(&writable)
            .and_then(|link| link.finish_reason.clone())
        });
        let reason = reason
          .map(|reason| {
            let reason = v8::Local::new(scope, &reason);
            yuzuko_core::op::root(scope, reason)
          })
          .unwrap_or_else(|| v8::undefined(scope).into());
        let _ = error_write(scope, writable, reason);
        unblock_transform_write(scope, writable);
        resolve_finish(scope, writable);
      }
    }
    Err(error) => {
      let error = yuzuko_core::op::root(scope, error);
      let _ = error_write(scope, writable, error);
      unblock_transform_write(scope, writable);
      reject_finish(scope, writable, error);
    }
  }
}

fn on_transform_sink_abort(
  scope: &mut v8::PinScope,
  writable: u64,
  result: Result<v8::Local<v8::Value>, v8::Local<v8::Value>>,
) {
  let readable = transform_readable(writable);
  match result {
    Ok(_) => {
      let errored = readable.is_some_and(|readable| {
        READS.with(|slot| {
          slot
            .borrow()
            .get(&readable)
            .is_some_and(|state| state.phase == Phase::Errored)
        })
      });
      if errored {
        let error = readable
          .and_then(|readable| readable_stored_error(scope, readable))
          .unwrap_or_else(|| type_error(scope, "The stream is errored"));
        reject_finish(scope, writable, error);
      } else {
        let reason = TRANSFORMS.with(|slot| {
          slot
            .borrow()
            .get(&writable)
            .and_then(|link| link.finish_reason.clone())
        });
        let reason = reason
          .map(|reason| {
            let reason = v8::Local::new(scope, &reason);
            yuzuko_core::op::root(scope, reason)
          })
          .unwrap_or_else(|| v8::undefined(scope).into());
        if let Some(readable) = readable {
          let _ = error_stream(scope, readable, reason);
        }
        resolve_finish(scope, writable);
      }
    }
    Err(error) => {
      let error = yuzuko_core::op::root(scope, error);
      if let Some(readable) = readable {
        let _ = error_stream(scope, readable, error);
      }
      reject_finish(scope, writable, error);
    }
  }
}

fn transform_pull(scope: &mut v8::PinScope, readable: u64) {
  let Some(writable) = transform_writable(readable) else {
    return;
  };
  if !should_pull_transform(readable) {
    return;
  }
  let busy = READS.with(|slot| {
    let mut states = slot.borrow_mut();
    let Some(state) = states.get_mut(&readable) else {
      return true;
    };
    if state.pulling {
      state.pull_again = true;
      return true;
    }
    state.pulling = true;
    false
  });
  if busy {
    return;
  }
  set_transform_backpressure(scope, writable, false);
  let promise = transform_change_promise(scope, writable);
  let _ = watch_promise(scope, promise, Token::Pull(readable));
}

fn utf8_encode_sink<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  stream: u64,
  chunk: v8::Local<v8::Value>,
  finish: bool,
) -> Result<v8::Local<'s, v8::Value>, OpError> {
  let readable = sink_readable(stream)?;
  if !finish {
    let text = chunk
      .to_string(scope)
      .map(|text| text.to_rust_string_lossy(scope))
      .unwrap_or_default();
    let (output, pending) = encode_utf8(&text);
    WRITES.with(|slot| {
      if let Some(state) = slot.borrow_mut().get_mut(&stream)
        && let Sink::Utf8Encode { pending: slot, .. } = &mut state.sink
      {
        *slot = pending;
      }
    });
    if !output.is_empty() {
      let view = bytes::uint8_array(scope, output);
      enqueue_chunk(scope, readable, view.into(), None)?;
    }
  } else {
    let pending = WRITES.with(|slot| {
      slot
        .borrow()
        .get(&stream)
        .and_then(|state| match &state.sink {
          Sink::Utf8Encode { pending, .. } => *pending,
          _ => None,
        })
    });
    if pending.is_some() {
      let view = bytes::uint8_array(scope, vec![0xef, 0xbf, 0xbd]);
      enqueue_chunk(scope, readable, view.into(), None)?;
    }
    let _ = close_stream(scope, readable);
  }
  Ok(v8::undefined(scope).into())
}

fn encode_utf8(text: &str) -> (Vec<u8>, Option<u16>) {
  let mut pending = None;
  let mut units = Vec::new();
  for unit in text.encode_utf16() {
    if let Some(high) = pending.take() {
      if (0xDC00..=0xDFFF).contains(&unit) {
        let point =
          0x10000 + (((high as u32 - 0xD800) << 10) | (unit as u32 - 0xDC00));
        units.extend(
          char::from_u32(point)
            .unwrap_or('\u{FFFD}')
            .encode_utf8(&mut [0; 4])
            .as_bytes(),
        );
      } else {
        units.extend_from_slice("\u{FFFD}".as_bytes());
        if (0xD800..=0xDBFF).contains(&unit) {
          pending = Some(unit);
        } else {
          units.extend(
            char::from_u32(unit as u32)
              .unwrap_or('\u{FFFD}')
              .encode_utf8(&mut [0; 4])
              .as_bytes(),
          );
        }
      }
    } else if (0xD800..=0xDBFF).contains(&unit) {
      pending = Some(unit);
    } else if (0xDC00..=0xDFFF).contains(&unit) {
      units.extend_from_slice("\u{FFFD}".as_bytes());
    } else {
      units.extend(
        char::from_u32(unit as u32)
          .unwrap_or('\u{FFFD}')
          .encode_utf8(&mut [0; 4])
          .as_bytes(),
      );
    }
  }
  (units, pending)
}

fn utf8_decode_sink<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  stream: u64,
  chunk: v8::Local<v8::Value>,
  finish: bool,
) -> Result<v8::Local<'s, v8::Value>, OpError> {
  let readable = sink_readable(stream)?;
  let bytes = if finish {
    Vec::new()
  } else {
    buffer_bytes(scope, chunk)?
  };
  let decoded = WRITES.with(|slot| {
    let mut states = slot.borrow_mut();
    let state = states.get_mut(&stream)?;
    let Sink::Utf8Decode { decoder, fatal, .. } = &mut state.sink else {
      return None;
    };
    let mut output = String::new();
    let (_result, _read, had_errors) =
      decoder.decode_to_string(&bytes, &mut output, finish);
    Some((output, *fatal && had_errors))
  });
  let Some((text, failed)) = decoded else {
    return Err(OpError::type_("The stream is closed"));
  };
  if failed {
    let error = type_error(scope, "The encoded data was not valid");
    let _ = error_stream(scope, readable, error);
    return Err(OpError::type_("The encoded data was not valid"));
  }
  if !text.is_empty() {
    let text = v8::String::new(scope, &text).unwrap();
    enqueue_chunk(scope, readable, text.into(), None)?;
  }
  if finish {
    let _ = close_stream(scope, readable);
  }
  Ok(v8::undefined(scope).into())
}

fn codec_sink<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  stream: u64,
  chunk: v8::Local<v8::Value>,
  finish: bool,
) -> Result<v8::Local<'s, v8::Value>, OpError> {
  let readable = sink_readable(stream)?;
  let input = if finish {
    Vec::new()
  } else {
    match buffer_bytes(scope, chunk) {
      Ok(bytes) => bytes,
      Err(error) => {
        let thrown = error_value(scope, &error);
        let _ = error_stream(scope, readable, thrown);
        return Err(error);
      }
    }
  };
  let produced = WRITES.with(|slot| {
    let mut states = slot.borrow_mut();
    let state = states.get_mut(&stream)?;
    let Sink::Codec { codec, emitted, .. } = &mut state.sink else {
      return None;
    };
    Some(codec_write(codec, emitted, &input, finish))
  });
  let Some(output) = produced else {
    return Err(OpError::type_("The stream is closed"));
  };
  if !output.bytes.is_empty() {
    let view = bytes::uint8_array(scope, output.bytes);
    enqueue_chunk(scope, readable, view.into(), None)?;
  }
  if output.failed {
    let pending = v8::undefined(scope).into();
    let _ = watch_promise(scope, pending, Token::CodecError(readable));
    return Err(OpError::type_("The compressed data is not valid"));
  }
  if finish {
    let _ = close_stream(scope, readable);
  }
  Ok(v8::undefined(scope).into())
}

struct Produced {
  bytes: Vec<u8>,
  failed: bool,
}

fn codec_write(
  codec: &mut Codec,
  emitted: &mut usize,
  input: &[u8],
  finish: bool,
) -> Produced {
  if matches!(
    codec,
    Codec::BrotliEnc(_) | Codec::BrotliDec(_) | Codec::BrotliFinished(_)
  ) {
    return brotli_write(codec, emitted, input, finish);
  }
  use std::io::Write;
  let fed = match codec {
    Codec::GzipEnc(encoder) => {
      encoder.write_all(input).map(|()| false).map_err(|_| ())
    }
    Codec::ZlibEnc(encoder) => {
      encoder.write_all(input).map(|()| false).map_err(|_| ())
    }
    Codec::RawEnc(encoder) => {
      encoder.write_all(input).map(|()| false).map_err(|_| ())
    }
    Codec::GzipDec(decoder) => write_decoder(decoder, input),
    Codec::ZlibDec(decoder) => write_decoder(decoder, input),
    Codec::RawDec(decoder) => write_decoder(decoder, input),
    _ => Err(()),
  };
  let mut failed = fed.unwrap_or(true);
  if finish {
    let finished = match codec {
      Codec::GzipEnc(encoder) => encoder.try_finish(),
      Codec::ZlibEnc(encoder) => encoder.try_finish(),
      Codec::RawEnc(encoder) => encoder.try_finish(),
      Codec::GzipDec(decoder) => decoder.try_finish(),
      Codec::ZlibDec(decoder) => finish_zlib_decoder(decoder, &mut failed),
      Codec::RawDec(decoder) => decoder.try_finish(),
      _ => Err(std::io::Error::other("unused")),
    };
    if finished.is_err() {
      failed = true;
    }
  }
  let Some(buf) = (match codec {
    Codec::GzipEnc(encoder) => Some(encoder.get_mut().as_slice()),
    Codec::ZlibEnc(encoder) => Some(encoder.get_mut().as_slice()),
    Codec::RawEnc(encoder) => Some(encoder.get_mut().as_slice()),
    Codec::GzipDec(decoder) => Some(decoder.get_mut().as_slice()),
    Codec::ZlibDec(decoder) => Some(decoder.get_mut().as_slice()),
    Codec::RawDec(decoder) => Some(decoder.get_mut().as_slice()),
    _ => None,
  }) else {
    return Produced {
      bytes: Vec::new(),
      failed: true,
    };
  };
  Produced {
    bytes: take_emitted(buf, emitted),
    failed,
  }
}

/// zlib's `try_finish` accepts a stream that stopped before the Adler
/// trailer. A finished stream refuses another byte. A truncated stream
/// either consumes it or reports a corrupt deflate stream.
fn finish_zlib_decoder(
  decoder: &mut flate2::write::ZlibDecoder<Vec<u8>>,
  failed: &mut bool,
) -> std::io::Result<()> {
  use std::io::Write;
  if !*failed {
    match decoder.write(&[0]) {
      Ok(0) => {}
      Ok(_) | Err(_) => *failed = true,
    }
  }
  decoder.try_finish()
}

/// `Ok(true)` means the compressed stream ended before `input` was consumed.
fn write_decoder<W: std::io::Write>(
  writer: &mut W,
  input: &[u8],
) -> Result<bool, ()> {
  if input.is_empty() {
    return Ok(false);
  }
  let mut offset = 0;
  while offset < input.len() {
    match writer.write(&input[offset..]) {
      Ok(0) => return Ok(true),
      Ok(n) => offset += n,
      Err(_) => return Err(()),
    }
  }
  // A finished chunk can sit inside the decoder until it is flushed.
  // Callers read that output before `close`.
  writer.flush().map_err(|_| ())?;
  Ok(false)
}

fn take_emitted(buf: &[u8], emitted: &mut usize) -> Vec<u8> {
  let start = (*emitted).min(buf.len());
  let output = buf[start..].to_vec();
  *emitted = buf.len();
  output
}

fn brotli_write(
  codec: &mut Codec,
  emitted: &mut usize,
  input: &[u8],
  finish: bool,
) -> Produced {
  use std::io::Write;
  let mut failed = false;
  match codec {
    Codec::BrotliEnc(encoder) => {
      if encoder.write_all(input).is_err() {
        failed = true;
      }
    }
    Codec::BrotliDec(decoder) => match write_decoder(decoder, input) {
      Ok(trailing) => failed = trailing,
      Err(()) => failed = true,
    },
    Codec::BrotliFinished(_) if input.is_empty() => {}
    _ => {
      return Produced {
        bytes: Vec::new(),
        failed: true,
      };
    }
  }
  if finish {
    let taken = std::mem::replace(codec, Codec::BrotliFinished(Vec::new()));
    let finished = match taken {
      Codec::BrotliEnc(encoder) => encoder.into_inner(),
      Codec::BrotliDec(decoder) => match decoder.into_inner() {
        Ok(buf) => buf,
        Err(buf) => {
          failed = true;
          buf
        }
      },
      Codec::BrotliFinished(bytes) => bytes,
      other => {
        *codec = other;
        return Produced {
          bytes: Vec::new(),
          failed: true,
        };
      }
    };
    *codec = Codec::BrotliFinished(finished);
  }
  let Some(buf) = (match codec {
    Codec::BrotliEnc(encoder) => Some(encoder.get_mut().as_slice()),
    Codec::BrotliDec(decoder) => Some(decoder.get_mut().as_slice()),
    Codec::BrotliFinished(bytes) => Some(bytes.as_slice()),
    _ => None,
  }) else {
    return Produced {
      bytes: Vec::new(),
      failed: true,
    };
  };
  Produced {
    bytes: take_emitted(buf, emitted),
    failed,
  }
}

#[cfg(test)]
mod brotli_tests {
  use super::Codec;
  use super::brotli_write;

  #[test]
  fn large_window_hello_world() {
    let chunk = [
      17, 25, 20, 0, 2, 72, 101, 108, 108, 111, 32, 87, 111, 114, 108, 100, 3,
    ];
    let mut codec = Codec::BrotliDec(Box::new(
      brotli::DecompressorWriter::new(Vec::new(), 4096),
    ));
    let mut emitted = 0usize;
    let first = brotli_write(&mut codec, &mut emitted, &chunk, false);
    let second = brotli_write(&mut codec, &mut emitted, &[], true);
    assert!(!first.failed);
    assert!(!second.failed);
    let mut output = first.bytes;
    output.extend(second.bytes);
    assert_eq!(output, b"Hello World");
  }

  #[test]
  fn complete_chunk_emits_before_close() {
    use super::codec_write;
    let expected = b"expected output";
    let cases: &[(&str, &[u8])] = &[
      (
        "deflate",
        &[
          120, 156, 75, 173, 40, 72, 77, 46, 73, 77, 81, 200, 47, 45, 41, 40,
          45, 1, 0, 48, 173, 6, 36,
        ],
      ),
      (
        "gzip",
        &[
          31, 139, 8, 0, 0, 0, 0, 0, 0, 3, 75, 173, 40, 72, 77, 46, 73, 77, 81,
          200, 47, 45, 41, 40, 45, 1, 0, 176, 1, 57, 179, 15, 0, 0, 0,
        ],
      ),
      (
        "raw",
        &[
          0x4b, 0xad, 0x28, 0x48, 0x4d, 0x2e, 0x49, 0x4d, 0x51, 0xc8, 0x2f,
          0x2d, 0x29, 0x28, 0x2d, 0x01, 0x00,
        ],
      ),
      (
        "brotli",
        &[
          0x21, 0x38, 0x00, 0x04, 0x65, 0x78, 0x70, 0x65, 0x63, 0x74, 0x65,
          0x64, 0x20, 0x6F, 0x75, 0x74, 0x70, 0x75, 0x74, 0x03,
        ],
      ),
    ];
    for (name, chunk) in cases {
      let mut codec = match *name {
        "deflate" => {
          Codec::ZlibDec(flate2::write::ZlibDecoder::new(Vec::new()))
        }
        "gzip" => Codec::GzipDec(flate2::write::GzDecoder::new(Vec::new())),
        "raw" => Codec::RawDec(flate2::write::DeflateDecoder::new(Vec::new())),
        "brotli" => Codec::BrotliDec(Box::new(
          brotli::DecompressorWriter::new(Vec::new(), 4096),
        )),
        _ => unreachable!(),
      };
      let mut emitted = 0usize;
      let produced = codec_write(&mut codec, &mut emitted, chunk, false);
      assert!(!produced.failed, "{name}");
      assert_eq!(produced.bytes, expected, "{name}");
    }
  }

  #[test]
  fn trailing_byte_outputs_then_fails() {
    use super::codec_write;
    let expected = b"expected output";
    let cases: &[(&str, &[u8])] = &[
      (
        "deflate",
        &[
          120, 156, 75, 173, 40, 72, 77, 46, 73, 77, 81, 200, 47, 45, 41, 40,
          45, 1, 0, 48, 173, 6, 36,
        ],
      ),
      (
        "gzip",
        &[
          31, 139, 8, 0, 0, 0, 0, 0, 0, 3, 75, 173, 40, 72, 77, 46, 73, 77, 81,
          200, 47, 45, 41, 40, 45, 1, 0, 176, 1, 57, 179, 15, 0, 0, 0,
        ],
      ),
      (
        "raw",
        &[
          0x4b, 0xad, 0x28, 0x48, 0x4d, 0x2e, 0x49, 0x4d, 0x51, 0xc8, 0x2f,
          0x2d, 0x29, 0x28, 0x2d, 0x01, 0x00,
        ],
      ),
      (
        "brotli",
        &[
          0x21, 0x38, 0x00, 0x04, 0x65, 0x78, 0x70, 0x65, 0x63, 0x74, 0x65,
          0x64, 0x20, 0x6F, 0x75, 0x74, 0x70, 0x75, 0x74, 0x03,
        ],
      ),
    ];
    for (name, chunk) in cases {
      let mut input = chunk.to_vec();
      input.push(0);
      let mut codec = match *name {
        "deflate" => {
          Codec::ZlibDec(flate2::write::ZlibDecoder::new(Vec::new()))
        }
        "gzip" => Codec::GzipDec(flate2::write::GzDecoder::new(Vec::new())),
        "raw" => Codec::RawDec(flate2::write::DeflateDecoder::new(Vec::new())),
        "brotli" => Codec::BrotliDec(Box::new(
          brotli::DecompressorWriter::new(Vec::new(), 4096),
        )),
        _ => unreachable!(),
      };
      let mut emitted = 0usize;
      let produced = codec_write(&mut codec, &mut emitted, &input, false);
      assert!(produced.failed, "{name}");
      assert_eq!(produced.bytes, expected, "{name}");
    }
  }

  #[test]
  fn deflate_truncated_and_data_byte_five_fail() {
    use super::codec_write;
    let chunk = [
      120, 156, 75, 173, 40, 72, 77, 46, 73, 77, 81, 200, 47, 45, 41, 40, 45,
      1, 0, 48, 173, 6, 36,
    ];
    let mut truncated = chunk.to_vec();
    truncated.pop();
    let mut codec = Codec::ZlibDec(flate2::write::ZlibDecoder::new(Vec::new()));
    let mut emitted = 0usize;
    let _ = codec_write(&mut codec, &mut emitted, &truncated, false);
    let closed = codec_write(&mut codec, &mut emitted, &[], true);
    assert!(closed.failed, "truncated deflate");

    let mut corrupt = chunk;
    corrupt[18] = 5;
    let mut codec = Codec::ZlibDec(flate2::write::ZlibDecoder::new(Vec::new()));
    let mut emitted = 0usize;
    let wrote = codec_write(&mut codec, &mut emitted, &corrupt, false);
    assert!(wrote.failed, "deflate data byte 5");

    let mut ok = chunk;
    ok[18] = 4;
    let mut codec = Codec::ZlibDec(flate2::write::ZlibDecoder::new(Vec::new()));
    let mut emitted = 0usize;
    let wrote = codec_write(&mut codec, &mut emitted, &ok, false);
    let closed = codec_write(&mut codec, &mut emitted, &[], true);
    assert!(!wrote.failed && !closed.failed, "deflate data byte 4");
    assert_eq!(wrote.bytes, b"expected output");
  }
}

fn sink_readable(stream: u64) -> Result<u64, OpError> {
  WRITES
    .with(|slot| {
      slot
        .borrow()
        .get(&stream)
        .and_then(|state| match &state.sink {
          Sink::Transform { readable, .. }
          | Sink::Utf8Encode { readable, .. }
          | Sink::Utf8Decode { readable, .. }
          | Sink::Codec { readable, .. } => Some(*readable),
          Sink::Js { .. } => None,
        })
    })
    .ok_or_else(|| OpError::type_("The stream is closed"))
}

fn buffer_bytes(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<Vec<u8>, OpError> {
  // Compression chunks are BufferSource, which is not [AllowShared].
  if value.is_shared_array_buffer()
    || v8::Local::<v8::ArrayBufferView>::try_from(value)
      .ok()
      .and_then(|view| view.get_backing_store())
      .is_some_and(|store| store.is_shared())
  {
    return Err(OpError::type_("The buffer is shared"));
  }
  bytes::buffer_source(scope, value)
    .ok_or_else(|| OpError::type_("A compression chunk must be a BufferSource"))
}

fn read_strategy(
  scope: &mut v8::PinScope,
  strategy: Option<v8::Local<v8::Value>>,
  default_hwm: f64,
) -> Result<(f64, Option<v8::Global<v8::Function>>), OpError> {
  let Some(strategy) = strategy.filter(|value| !value.is_null_or_undefined())
  else {
    return Ok((default_hwm, None));
  };
  let object = object_value(scope, strategy)?;
  // `unrestricted double` accepts NaN. A size that is not a function throws
  // TypeError while the dictionary is converted, before the NaN check.
  let high_water = match property(scope, object, "highWaterMark")? {
    Some(value) => value
      .number_value(scope)
      .ok_or_else(|| OpError::type_("highWaterMark is not a number"))?,
    None => default_hwm,
  };
  let size = match property(scope, object, "size")? {
    Some(value) => {
      let function = v8::Local::<v8::Function>::try_from(value)
        .map_err(|_| OpError::type_("The strategy size is not a function"))?;
      Some(v8::Global::new(scope, function))
    }
    None => None,
  };
  if high_water.is_nan() || high_water < 0.0 {
    return Err(OpError::range("highWaterMark is invalid"));
  }
  Ok((high_water, size))
}

fn sink_type_present(
  scope: &mut v8::PinScope,
  sink: Option<v8::Local<v8::Value>>,
) -> Result<bool, OpError> {
  let Some(sink) = sink.filter(|value| !value.is_null_or_undefined()) else {
    return Ok(false);
  };
  let object = object_value(scope, sink)?;
  Ok(property(scope, object, "type")?.is_some())
}

fn strategy_high_water(
  scope: &mut v8::PinScope,
  init: v8::Local<v8::Value>,
  required: bool,
) -> Result<f64, OpError> {
  let object = object_value(scope, init)?;
  match property(scope, object, "highWaterMark")? {
    Some(value) => unrestricted_double(scope, value),
    None if required => Err(OpError::type_("highWaterMark is required")),
    None => Ok(DEFAULT_HWM),
  }
}

type Callback = Option<v8::Global<v8::Function>>;

fn source_methods(
  scope: &mut v8::PinScope,
  source: Option<v8::Local<v8::Value>>,
) -> Result<(Callback, Callback, Callback), OpError> {
  let Some(source) = source else {
    return Ok((None, None, None));
  };
  let object = object_value(scope, source)?;
  Ok((
    method(scope, object, "start")?,
    method(scope, object, "pull")?,
    method(scope, object, "cancel")?,
  ))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SourceKind {
  Default,
  Bytes,
  Owning,
}

fn source_kind(
  scope: &mut v8::PinScope,
  source: Option<v8::Local<v8::Value>>,
) -> Result<SourceKind, OpError> {
  let Some(source) = source else {
    return Ok(SourceKind::Default);
  };
  let object = object_value(scope, source)?;
  let Some(value) = property(scope, object, "type")? else {
    return Ok(SourceKind::Default);
  };
  let text = js_string(scope, value)?;
  match text.as_str() {
    "bytes" => Ok(SourceKind::Bytes),
    "owning" => Ok(SourceKind::Owning),
    _ => Err(OpError::type_("The stream type is invalid")),
  }
}

struct EnqueueTransfer {
  buffers: Vec<v8::Global<v8::ArrayBuffer>>,
  hosts: Vec<v8::Global<v8::Object>>,
}

impl EnqueueTransfer {
  fn is_empty(&self) -> bool {
    self.buffers.is_empty() && self.hosts.is_empty()
  }
}

/// `ReadableStreamDefaultController.enqueue`'s transfer list. A non-empty
/// list is rejected unless the stream's type is `"owning"`.
fn enqueue_transfers(
  scope: &mut v8::PinScope,
  options: Option<v8::Local<v8::Value>>,
) -> Result<EnqueueTransfer, OpError> {
  let Some(options) = options else {
    return Ok(EnqueueTransfer {
      buffers: Vec::new(),
      hosts: Vec::new(),
    });
  };
  let object = object_value(scope, options)?;
  let Some(transfer) = property(scope, object, "transfer")? else {
    return Ok(EnqueueTransfer {
      buffers: Vec::new(),
      hosts: Vec::new(),
    });
  };
  let list = v8::Local::<v8::Array>::try_from(transfer)
    .map_err(|_| OpError::type_("The transfer list is not an array"))?;
  let mut buffers = Vec::new();
  let mut hosts = Vec::new();
  for index in 0..list.length() {
    let Some(item) = list.get_index(scope, index) else {
      return Err(OpError::type_("The transfer list is invalid"));
    };
    if let Ok(buffer) = v8::Local::<v8::ArrayBuffer>::try_from(item) {
      if buffer.was_detached() || !buffer.is_detachable() {
        return Err(OpError::dom(
          "DataCloneError",
          "The ArrayBuffer cannot be transferred",
        ));
      }
      let duplicate = buffers.iter().any(|existing| {
        let existing: v8::Local<v8::ArrayBuffer> =
          v8::Local::new(scope, existing);
        existing.strict_equals(buffer.into())
      });
      if duplicate {
        return Err(OpError::dom(
          "DataCloneError",
          "The value is already in the transfer list",
        ));
      }
      buffers.push(v8::Global::new(scope, buffer));
      continue;
    }
    let object = v8::Local::<v8::Object>::try_from(item).map_err(|_| {
      OpError::dom("DataCloneError", "The value cannot be transferred")
    })?;
    let duplicate = hosts.iter().any(|existing| {
      let existing: v8::Local<v8::Object> = v8::Local::new(scope, existing);
      existing.strict_equals(object.into())
    });
    if duplicate {
      return Err(OpError::dom(
        "DataCloneError",
        "The value is already in the transfer list",
      ));
    }
    hosts.push(v8::Global::new(scope, object));
  }
  Ok(EnqueueTransfer { buffers, hosts })
}

fn clone_for_owning<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  chunk: v8::Local<v8::Value>,
  transfer: EnqueueTransfer,
) -> Result<v8::Local<'s, v8::Value>, OpError> {
  yuzuko_core::set_structured_transfer(transfer.hosts);
  let cloned = match yuzuko_core::clone_value(scope, chunk) {
    Ok(cloned) => cloned,
    Err(Some(thrown)) => {
      yuzuko_core::clear_structured_transfer();
      return Err(OpError::Thrown(Box::new(thrown)));
    }
    Err(None) => {
      yuzuko_core::clear_structured_transfer();
      return Err(OpError::dom(
        "DataCloneError",
        "The chunk could not be cloned",
      ));
    }
  };
  yuzuko_core::clear_structured_transfer();
  for buffer in &transfer.buffers {
    let buffer = v8::Local::new(scope, buffer);
    if buffer.was_detached() || !buffer.is_detachable() {
      return Err(OpError::dom(
        "DataCloneError",
        "The ArrayBuffer cannot be transferred",
      ));
    }
    let _ = buffer.detach(None);
  }
  Ok(cloned)
}

fn js_string(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<String, OpError> {
  let captured = {
    v8::tc_scope!(let tc, scope);
    match value.to_string(tc) {
      Some(text) => Ok(text.to_rust_string_lossy(tc)),
      None => {
        let thrown = tc.exception().map(|thrown| v8::Global::new(tc, thrown));
        Err(thrown)
      }
    }
  };
  match captured {
    Ok(text) => Ok(text),
    Err(Some(thrown)) => Err(OpError::Thrown(Box::new(thrown))),
    Err(None) => Err(OpError::type_("ToString failed")),
  }
}

fn sink_methods(
  scope: &mut v8::PinScope,
  sink: Option<v8::Local<v8::Value>>,
) -> Result<(Callback, Callback, Callback, Callback), OpError> {
  let Some(sink) = sink.filter(|value| !value.is_null_or_undefined()) else {
    return Ok((None, None, None, None));
  };
  let object = object_value(scope, sink)?;
  Ok((
    method(scope, object, "start")?,
    method(scope, object, "write")?,
    method(scope, object, "close")?,
    method(scope, object, "abort")?,
  ))
}

fn transform_methods(
  scope: &mut v8::PinScope,
  transformer: Option<v8::Local<v8::Value>>,
) -> Result<(Callback, Callback, Callback, Callback), OpError> {
  let Some(transformer) =
    transformer.filter(|value| !value.is_null_or_undefined())
  else {
    return Ok((None, None, None, None));
  };
  let object = object_value(scope, transformer)?;
  Ok((
    method(scope, object, "start")?,
    method(scope, object, "transform")?,
    method(scope, object, "flush")?,
    method(scope, object, "cancel")?,
  ))
}

fn method(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
) -> Result<Option<v8::Global<v8::Function>>, OpError> {
  let Some(value) = property(scope, object, name)? else {
    return Ok(None);
  };
  if value.is_undefined() {
    return Ok(None);
  }
  let function = v8::Local::<v8::Function>::try_from(value)
    .map_err(|_| OpError::type_(format!("{name} is not a function")))?;
  let function = bind_receiver(scope, function, object)?;
  Ok(Some(v8::Global::new(scope, function)))
}

fn capture_function_bind(scope: &mut v8::PinScope) {
  let Some(function) = v8::Function::new(scope, unused_function) else {
    return;
  };
  let Some(proto) = function.get_prototype(scope) else {
    return;
  };
  let Ok(proto) = v8::Local::<v8::Object>::try_from(proto) else {
    return;
  };
  let Some(key) = v8::String::new(scope, "bind") else {
    return;
  };
  let Some(bind) = proto.get(scope, key.into()) else {
    return;
  };
  let Ok(bind) = v8::Local::<v8::Function>::try_from(bind) else {
    return;
  };
  let bind = v8::Global::new(scope, bind);
  FUNCTION_BIND.with(|slot| *slot.borrow_mut() = Some(bind));
}

fn unused_function(
  _scope: &mut v8::PinScope,
  _args: v8::FunctionCallbackArguments,
  _ret: v8::ReturnValue,
) {
}

fn bind_receiver<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  function: v8::Local<'_, v8::Function>,
  receiver: v8::Local<'_, v8::Object>,
) -> Result<v8::Local<'s, v8::Function>, OpError> {
  let function = yuzuko_core::op::root(scope, function);
  let bind = FUNCTION_BIND.with(|slot| {
    slot
      .borrow()
      .as_ref()
      .map(|bind| v8::Local::new(scope, bind))
  });
  let Some(bind) = bind else {
    return Err(OpError::type_("Function.prototype.bind is missing"));
  };
  let receiver: v8::Local<v8::Value> = receiver.into();
  let bound = bind
    .call(scope, function.into(), &[receiver])
    .ok_or_else(|| OpError::type_("Function.prototype.bind failed"))?;
  let bound = v8::Local::<v8::Function>::try_from(bound)
    .map_err(|_| OpError::type_("Function.prototype.bind failed"))?;
  Ok(yuzuko_core::op::root(scope, bound))
}

fn object_value<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  value: v8::Local<v8::Value>,
) -> Result<v8::Local<'s, v8::Object>, OpError> {
  let object = v8::Local::<v8::Object>::try_from(value)
    .map_err(|_| OpError::type_("The provided value is not an object"))?;
  Ok(yuzuko_core::op::root(scope, object))
}

fn property<'a>(
  scope: &mut v8::PinScope<'a, '_>,
  object: v8::Local<v8::Object>,
  name: &str,
) -> Result<Option<v8::Local<'a, v8::Value>>, OpError> {
  let found = {
    v8::tc_scope!(let tc, scope);
    let key = v8::String::new(tc, name).unwrap();
    match object.get(tc, key.into()) {
      Some(value) => Ok(v8::Global::new(tc, value)),
      None => {
        let thrown = tc.exception().map(|value| v8::Global::new(tc, value));
        Err(thrown)
      }
    }
  };
  match found {
    Ok(value) => {
      let value = v8::Local::new(scope, value);
      if value.is_undefined() {
        Ok(None)
      } else {
        Ok(Some(value))
      }
    }
    Err(Some(thrown)) => Err(OpError::Thrown(Box::new(thrown))),
    Err(None) => Err(OpError::type_("The property access failed")),
  }
}

fn call_method<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  function: &v8::Global<v8::Function>,
  args: &[v8::Local<v8::Value>],
) -> Result<v8::Local<'s, v8::Value>, OpError> {
  let this = v8::undefined(scope).into();
  call(scope, function, this, args)
}

fn call<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  function: &v8::Global<v8::Function>,
  this: v8::Local<v8::Value>,
  args: &[v8::Local<v8::Value>],
) -> Result<v8::Local<'s, v8::Value>, OpError> {
  let result = {
    v8::tc_scope!(let tc, scope);
    let function = v8::Local::new(tc, function);
    match function.call(tc, this, args) {
      Some(value) => Ok(v8::Global::new(tc, value)),
      None => {
        let thrown = tc.exception().map(|value| v8::Global::new(tc, value));
        Err(thrown)
      }
    }
  };
  match result {
    Ok(value) => Ok(v8::Local::new(scope, value)),
    Err(Some(thrown)) => Err(OpError::Thrown(Box::new(thrown))),
    Err(None) => Err(OpError::type_("The callback failed")),
  }
}

fn watch_promise(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
  token: Token,
) -> Js {
  let promise = promise_from(scope, value);
  let id = alloc();
  REACTIONS.with(|slot| {
    slot.borrow_mut().insert(id, token);
  });
  let Some(fulfill) = bind_settle(scope, on_fulfill, id) else {
    let value: v8::Local<v8::Value> = promise.into();
    return Js(v8::Global::new(scope, value));
  };
  let fulfill = yuzuko_core::op::root(scope, fulfill);
  let Some(reject) = bind_settle(scope, on_reject, id) else {
    let value: v8::Local<v8::Value> = promise.into();
    return Js(v8::Global::new(scope, value));
  };
  let reject = yuzuko_core::op::root(scope, reject);
  promise.then2(scope, fulfill, reject);
  let value: v8::Local<v8::Value> = promise.into();
  Js(v8::Global::new(scope, value))
}

fn promise_from<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  value: v8::Local<v8::Value>,
) -> v8::Local<'s, v8::Promise> {
  if let Ok(promise) = v8::Local::<v8::Promise>::try_from(value) {
    return yuzuko_core::op::root(scope, promise);
  }
  let resolver = v8::PromiseResolver::new(scope).unwrap();
  resolver.resolve(scope, value);
  resolver.get_promise(scope)
}

fn bind_settle<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  callback: impl v8::MapFnTo<v8::FunctionCallback>,
  id: u64,
) -> Option<v8::Local<'s, v8::Function>> {
  // `terminate()` can make the next V8 call throw. Shutdown must not abort.
  let function = v8::Function::new(scope, callback)?;
  let function = yuzuko_core::op::root(scope, function);
  let bind_key = v8::String::new(scope, "bind")?;
  let bind = function.get(scope, bind_key.into())?;
  let bind = v8::Local::<v8::Function>::try_from(bind).ok()?;
  let bind = yuzuko_core::op::root(scope, bind);
  let this_arg = v8::undefined(scope).into();
  let id_value = v8::Number::new(scope, id as f64);
  let id_value: v8::Local<v8::Value> = id_value.into();
  let bound = {
    v8::tc_scope!(let tc, scope);
    let bound = bind.call(tc, function.into(), &[this_arg, id_value])?;
    let bound = v8::Local::<v8::Function>::try_from(bound).ok()?;
    v8::Global::new(tc, bound)
  };
  Some(v8::Local::new(scope, bound))
}

fn on_fulfill(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut _ret: v8::ReturnValue,
) {
  settle_reaction(scope, args.get(0), args.get(1), true);
}

fn on_reject(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut _ret: v8::ReturnValue,
) {
  settle_reaction(scope, args.get(0), args.get(1), false);
}

fn settle_reaction(
  scope: &mut v8::PinScope,
  id_value: v8::Local<v8::Value>,
  value: v8::Local<v8::Value>,
  ok: bool,
) {
  let Some(id) = id_value.number_value(scope) else {
    return;
  };
  let token = REACTIONS.with(|slot| slot.borrow_mut().remove(&(id as u64)));
  let Some(token) = token else {
    return;
  };
  if ok {
    resume(scope, token, Ok(value));
  } else {
    resume(scope, token, Err(value));
  }
}

fn resume(
  scope: &mut v8::PinScope,
  token: Token,
  result: Result<v8::Local<v8::Value>, v8::Local<v8::Value>>,
) {
  match token {
    Token::StartRead(id) => match result {
      Ok(_) => mark_started(scope, id),
      Err(error) => {
        let _ = error_stream(scope, id, error);
      }
    },
    Token::Pull(id) => {
      if byte_stream::is_byte(id) {
        byte_stream::on_pull_settled(scope, id, result.err());
      } else {
        on_pull_settled(scope, id, result.err());
      }
    }
    Token::StartWrite(id) => match result {
      Ok(_) => mark_write_started(scope, id),
      Err(error) => {
        // `started` is set before the rejection is applied, so a stream that
        // is already erroring finishes and a writable stream errors now.
        let erroring = WRITES.with(|slot| {
          let mut states = slot.borrow_mut();
          let state = states.get_mut(&id)?;
          state.started = true;
          Some(state.phase == Phase::Erroring)
        });
        if erroring == Some(true) {
          finish_erroring(scope, id);
        } else {
          let _ = error_write(scope, id, error);
        }
      }
    },
    Token::Write(id) => finish_write(scope, id, result),
    Token::CloseWrite(id) => match result {
      Ok(_) => finish_close_write(scope, id),
      Err(error) => fail_close_write(scope, id, error),
    },
    Token::AbortWrite(id) => settle_abort(scope, id, result),
    Token::AbortForget(stream) => {
      WRITES.with(|slot| {
        if let Some(state) = slot.borrow_mut().get_mut(&stream) {
          state.abort_promise = None;
        }
      });
    }
    Token::TeeRead(id) => {
      if byte_stream::owns_tee(id) {
        byte_stream::tee_settled(scope, id, result);
      } else {
        match result {
          Ok(value) => tee_chunk(scope, id, value),
          Err(error) => tee_fail(scope, id, error),
        }
      }
    }
    Token::TeeKick(id) => pull_if_needed(scope, id),
    Token::TeeClosed(id) => {
      if byte_stream::owns_tee(id) {
        if let Err(error) = result {
          byte_stream::tee_reader_rejected(scope, id, error);
        }
      } else if let Err(error) = result {
        tee_fail(scope, id, error);
        // `cancel()` fulfills when the source errors and a branch is still open.
        tee_resolve_cancel(scope, id);
      }
    }
    Token::TransformStart(id) => {
      // `promiseResolvedWith` adopts the start promise, so `[[started]]`
      // flips one turn after that promise settles. `cancel()` in the
      // constructor turn must still see a stream that has not started.
      let error = result.err().map(|error| v8::Global::new(scope, error));
      TRANSFORM_STARTS.with(|slot| {
        slot.borrow_mut().insert(id, error);
      });
      let value = v8::undefined(scope).into();
      let _ = watch_promise(scope, value, Token::TransformReady(id));
    }
    Token::TransformReady(id) => {
      let stored = TRANSFORM_STARTS.with(|slot| slot.borrow_mut().remove(&id));
      match stored {
        Some(None) => {
          mark_write_started(scope, id);
          if let Some(readable) = transform_readable(id) {
            mark_started(scope, readable);
          }
        }
        Some(Some(error)) => {
          let error = v8::Local::new(scope, &error);
          let error = yuzuko_core::op::root(scope, error);
          let erroring = WRITES.with(|slot| {
            let mut states = slot.borrow_mut();
            let state = states.get_mut(&id)?;
            state.started = true;
            Some(state.phase == Phase::Erroring)
          });
          if erroring == Some(true) {
            finish_erroring(scope, id);
          } else {
            let _ = error_write(scope, id, error);
          }
          if let Some(readable) = transform_readable(id) {
            let _ = error_stream(scope, readable, error);
          }
        }
        None => {}
      }
    }
    Token::TransformRelease(id) => on_transform_release(scope, id),
    Token::TransformForward(id) => on_transform_forward(scope, id, result),
    Token::TransformFinish(id) => on_transform_finish(scope, id, result),
    Token::PipeRead(id) => pipe_on_read(scope, id, result),
    Token::PipeReady(id) => {
      if result.is_ok() {
        pipe_step(scope, id);
      }
    }
    Token::PipeWriteDone(id) => continue_plan(scope, id),
    Token::PipeSourceClosed(id) => pipe_on_source_closed(scope, id, result),
    Token::PipeDestClosed(id) => {
      if let Err(error) = result {
        pipe_on_dest_closed(scope, id, error);
      }
    }
    Token::PipeDeferredClose(id) => pipe_on_deferred_close(scope, id),
    Token::PipeDrain(id) => continue_plan(scope, id),
    Token::PipeAction(id) => pipe_on_action(scope, id, result),
    Token::PipeSignal(id) => pipe_on_signal(scope, id, result),
    Token::BodyRead(id) => body_on_read(scope, id, result),
    Token::BodyWait(id) => body_on_wait(scope, id, result),
    Token::CodecError(id) => {
      let error = type_error(scope, "The compressed data is not valid");
      let _ = error_stream(scope, id, error);
    }
  }
}

fn stream_id(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<u64, OpError> {
  let object = v8::Local::<v8::Object>::try_from(value).map_err(|_| {
    OpError::type_("The provided value is not a ReadableStream")
  })?;
  let host =
    unsafe { yuzuko_core::op::host_ref::<ReadableStream>(scope, object) }
      .ok_or_else(|| {
        OpError::type_("The provided value is not a ReadableStream")
      })?;
  Ok(host.get().id)
}

fn writable_id(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<u64, OpError> {
  let object = v8::Local::<v8::Object>::try_from(value).map_err(|_| {
    OpError::type_("The provided value is not a WritableStream")
  })?;
  let host =
    unsafe { yuzuko_core::op::host_ref::<WritableStream>(scope, object) }
      .ok_or_else(|| {
        OpError::type_("The provided value is not a WritableStream")
      })?;
  Ok(host.get().id)
}

fn read_result<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  value: Option<v8::Local<v8::Value>>,
  done: bool,
) -> v8::Local<'s, v8::Value> {
  let object = v8::Object::new(scope);
  let value_key = v8::String::new(scope, "value").unwrap();
  let done_key = v8::String::new(scope, "done").unwrap();
  let value = value.unwrap_or_else(|| v8::undefined(scope).into());
  object.set(scope, value_key.into(), value);
  object.set(scope, done_key.into(), v8::Boolean::new(scope, done).into());
  object.into()
}

/// React to `value` so the returned promise fulfills with `undefined`.
fn promise_undefined(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Js {
  let promise = promise_from(scope, value);
  let fulfill = v8::Function::new(scope, cancel_fulfill).unwrap();
  let fulfill = yuzuko_core::op::root(scope, fulfill);
  let reject = v8::Function::new(scope, cancel_reject).unwrap();
  let reject = yuzuko_core::op::root(scope, reject);
  let next = promise.then2(scope, fulfill, reject).unwrap();
  let value: v8::Local<v8::Value> = next.into();
  Js(v8::Global::new(scope, value))
}

fn cancel_fulfill(
  scope: &mut v8::PinScope,
  _args: v8::FunctionCallbackArguments,
  mut ret: v8::ReturnValue,
) {
  ret.set(v8::undefined(scope).into());
}

fn cancel_reject(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut _ret: v8::ReturnValue,
) {
  scope.throw_exception(args.get(0));
}

fn resolved(scope: &mut v8::PinScope, value: v8::Local<v8::Value>) -> Js {
  let resolver = v8::PromiseResolver::new(scope).unwrap();
  fulfill_resolver(scope, resolver, value);
  let promise = resolver.get_promise(scope);
  let value: v8::Local<v8::Value> = promise.into();
  Js(v8::Global::new(scope, value))
}

/// Fulfills `resolver` without running `Object.prototype.then`.
///
/// V8's `Resolver::resolve` uses the Promise Resolve functions, which
/// assimilate thenables. Stream read records are ordinary objects, and a
/// test that puts `then` on `Object.prototype` must not observe them.
pub(super) fn fulfill_resolver(
  scope: &mut v8::PinScope,
  resolver: v8::Local<v8::PromiseResolver>,
  value: v8::Local<v8::Value>,
) {
  let key = v8::String::new(scope, "then").unwrap();
  let armed = arm_then_shadow(scope, value, key);
  resolver.resolve(scope, value);
  if armed && let Ok(object) = v8::Local::<v8::Object>::try_from(value) {
    let _ = object.delete(scope, key.into());
  }
}

fn arm_then_shadow(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
  key: v8::Local<v8::String>,
) -> bool {
  if value.is_null_or_undefined() || value.is_promise() {
    return false;
  }
  let Ok(object) = v8::Local::<v8::Object>::try_from(value) else {
    return false;
  };
  if object.has_own_property(scope, key.into()).unwrap_or(true) {
    return false;
  }
  object
    .set(scope, key.into(), v8::undefined(scope).into())
    .unwrap_or(false)
}

fn rejected(scope: &mut v8::PinScope, error: v8::Local<v8::Value>) -> Js {
  let resolver = v8::PromiseResolver::new(scope).unwrap();
  resolver.reject(scope, error);
  let promise = resolver.get_promise(scope);
  let value: v8::Local<v8::Value> = promise.into();
  Js(v8::Global::new(scope, value))
}

fn type_error<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  message: &str,
) -> v8::Local<'s, v8::Value> {
  let message = v8::String::new(scope, message).unwrap();
  v8::Exception::type_error(scope, message)
}

fn error_value<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  error: &OpError,
) -> v8::Local<'s, v8::Value> {
  match error {
    OpError::Thrown(value) => v8::Local::new(scope, value.as_ref()),
    OpError::Type(message) | OpError::Range(message) => {
      type_error(scope, message)
    }
    OpError::Dom { name, message } => {
      yuzuko_core::op::new_dom_exception(scope, name, message)
        .map(v8::Local::<v8::Value>::from)
        .unwrap_or_else(|| type_error(scope, message))
    }
  }
}

fn traced_js(
  scope: &mut v8::PinScope,
  reference: &v8::TracedReference<v8::Object>,
) -> Js {
  let value = yuzuko_core::op::traced_local(scope, reference)
    .map(v8::Local::<v8::Value>::from)
    .unwrap_or_else(|| v8::undefined(scope).into());
  Js(v8::Global::new(scope, value))
}

fn unrestricted_double(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<f64, OpError> {
  let converted = {
    v8::tc_scope!(let tc, scope);
    match value.number_value(tc) {
      Some(number) => Ok(number),
      None => Err(tc.exception().map(|thrown| v8::Global::new(tc, thrown))),
    }
  };
  match converted {
    Ok(number) => Ok(number),
    Err(Some(thrown)) => Err(OpError::Thrown(Box::new(thrown))),
    Err(None) => Err(OpError::type_("highWaterMark is not a number")),
  }
}

fn define_strategy_size(
  scope: &mut v8::PinScope,
  class_name: &str,
  callback: impl v8::MapFnTo<v8::FunctionCallback>,
  length: i32,
) {
  let global = scope.get_current_context().global(scope);
  let class_key = v8::String::new(scope, class_name).unwrap();
  let Some(ctor) = global.get(scope, class_key.into()) else {
    return;
  };
  let Ok(ctor) = v8::Local::<v8::Object>::try_from(ctor) else {
    return;
  };
  let proto_key = v8::String::new(scope, "prototype").unwrap();
  let Some(proto) = ctor.get(scope, proto_key.into()) else {
    return;
  };
  let Ok(proto) = v8::Local::<v8::Object>::try_from(proto) else {
    return;
  };
  let name = v8::String::new(scope, "size").unwrap();
  let Some(function) = v8::Function::builder(callback)
    .length(length)
    .constructor_behavior(v8::ConstructorBehavior::Throw)
    .build(scope)
  else {
    return;
  };
  function.set_name(name);
  let getter_callback: v8::FunctionCallback =
    if class_name == "CountQueuingStrategy" {
      v8::MapFnTo::map_fn_to(count_size_getter)
    } else {
      v8::MapFnTo::map_fn_to(byte_size_getter)
    };
  let Some(getter) = v8::Function::builder_raw(getter_callback)
    .length(0)
    .data(function.into())
    .constructor_behavior(v8::ConstructorBehavior::Throw)
    .build(scope)
  else {
    return;
  };
  let getter_name = v8::String::new(scope, "get size").unwrap();
  getter.set_name(getter_name);
  let get: v8::Local<v8::Value> = getter.into();
  let set: v8::Local<v8::Value> = v8::undefined(scope).into();
  let mut descriptor = v8::PropertyDescriptor::new_from_get_set(get, set);
  descriptor.set_enumerable(true);
  descriptor.set_configurable(true);
  let _ = proto.define_property(scope, name.into(), &descriptor);
}

fn count_size_getter(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut ret: v8::ReturnValue,
) {
  let object = args.this();
  if unsafe { yuzuko_core::op::host_ref::<CountQueuingStrategy>(scope, object) }
    .is_none()
  {
    yuzuko_core::op::OpException::throw(
      OpError::type_("illegal invocation"),
      scope,
    );
    return;
  }
  ret.set(args.data());
}

fn byte_size_getter(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut ret: v8::ReturnValue,
) {
  let object = args.this();
  if unsafe {
    yuzuko_core::op::host_ref::<ByteLengthQueuingStrategy>(scope, object)
  }
  .is_none()
  {
    yuzuko_core::op::OpException::throw(
      OpError::type_("illegal invocation"),
      scope,
    );
    return;
  }
  ret.set(args.data());
}

fn count_size_callback(
  _scope: &mut v8::PinScope,
  _args: v8::FunctionCallbackArguments,
  mut ret: v8::ReturnValue,
) {
  ret.set_double(1.0);
}

fn byte_size_callback(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut ret: v8::ReturnValue,
) {
  let chunk = args.get(0);
  if chunk.is_null_or_undefined() {
    let message =
      v8::String::new(scope, "The chunk has no byte length").unwrap();
    let error = v8::Exception::type_error(scope, message);
    scope.throw_exception(error);
    return;
  }
  let Some(object) = chunk.to_object(scope) else {
    return;
  };
  let key = v8::String::new(scope, "byteLength").unwrap();
  let Some(value) = object.get(scope, key.into()) else {
    return;
  };
  ret.set(value);
}

fn option_bool(
  scope: &mut v8::PinScope,
  options: Option<v8::Local<v8::Value>>,
  name: &str,
) -> bool {
  let Some(options) = options else {
    return false;
  };
  let Ok(object) = v8::Local::<v8::Object>::try_from(options) else {
    return false;
  };
  property(scope, object, name)
    .ok()
    .flatten()
    .is_some_and(|value| value.boolean_value(scope))
}

fn readable_object_id(
  scope: &mut v8::PinScope,
  object: &v8::Global<v8::Object>,
) -> Result<u64, OpError> {
  let local = v8::Local::new(scope, object);
  let local = yuzuko_core::op::root(scope, local);
  let value: v8::Local<v8::Value> = local.into();
  stream_id(scope, value)
}

fn tee_cancel_function<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  tee: u64,
  branch: u8,
) -> v8::Local<'s, v8::Function> {
  let function = v8::Function::new(scope, tee_cancel_callback).unwrap();
  let function = yuzuko_core::op::root(scope, function);
  let bind_key = v8::String::new(scope, "bind").unwrap();
  let bind = function.get(scope, bind_key.into()).unwrap();
  let bind = v8::Local::<v8::Function>::try_from(bind).unwrap();
  let bind = yuzuko_core::op::root(scope, bind);
  let this_arg = v8::undefined(scope).into();
  let tee_value: v8::Local<v8::Value> =
    v8::Number::new(scope, tee as f64).into();
  let branch_value: v8::Local<v8::Value> =
    v8::Number::new(scope, branch as f64).into();
  let bound = bind
    .call(scope, function.into(), &[this_arg, tee_value, branch_value])
    .unwrap();
  let bound = v8::Local::<v8::Function>::try_from(bound).unwrap();
  yuzuko_core::op::root(scope, bound)
}

fn tee_cancel_callback(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut ret: v8::ReturnValue,
) {
  let Some(tee) = args.get(0).number_value(scope) else {
    return;
  };
  let Some(branch) = args.get(1).number_value(scope) else {
    return;
  };
  let tee = tee as u64;
  let branch = branch as usize;
  if branch > 1 {
    return;
  }
  let reason = args.get(2);
  let promise = TEES.with(|slot| {
    slot
      .borrow()
      .get(&tee)
      .map(|state| state.cancel_promise.clone())
  });
  let Some(promise) = promise else {
    return;
  };
  let both = TEES.with(|slot| {
    let mut tees = slot.borrow_mut();
    let Some(state) = tees.get_mut(&tee) else {
      return false;
    };
    state.canceled[branch] = true;
    state.reasons[branch] = Some(v8::Global::new(scope, reason));
    state.canceled[0] && state.canceled[1]
  });
  if both {
    tee_cancel_source(scope, tee);
  }
  let promise = v8::Local::new(scope, &promise);
  ret.set(promise.into());
}

fn tee_cancel_source(scope: &mut v8::PinScope, tee: u64) {
  let snapshot = TEES.with(|slot| {
    slot.borrow().get(&tee).map(|state| {
      (
        state.reader,
        state.reasons[0].clone(),
        state.reasons[1].clone(),
        state.cancel_resolver.clone(),
      )
    })
  });
  let Some((reader, reason1, reason2, resolver)) = snapshot else {
    return;
  };
  let source =
    READERS.with(|slot| slot.borrow().get(&reader).map(|state| state.stream));
  let Some(source) = source else {
    return;
  };
  let array = v8::Array::new(scope, 2);
  let first = reason1
    .as_ref()
    .map(|value| v8::Local::new(scope, value))
    .unwrap_or_else(|| v8::undefined(scope).into());
  let second = reason2
    .as_ref()
    .map(|value| v8::Local::new(scope, value))
    .unwrap_or_else(|| v8::undefined(scope).into());
  array.set_index(scope, 0, first);
  array.set_index(scope, 1, second);
  let result = cancel_stream(scope, source, Some(array.into()));
  let resolver = v8::Local::new(scope, &resolver);
  let resolver = yuzuko_core::op::root(scope, resolver);
  let value = js_value(scope, result);
  resolver.resolve(scope, value);
}

fn tee_branch(stream: u64) -> Option<(u64, u8)> {
  TEES.with(|slot| {
    for (id, tee) in slot.borrow().iter() {
      if tee.branches[0] == stream {
        return Some((*id, 0));
      }
      if tee.branches[1] == stream {
        return Some((*id, 1));
      }
    }
    None
  })
}

fn tee_pull(scope: &mut v8::PinScope, tee: u64, branch: u8) {
  let reader = TEES.with(|slot| {
    let mut tees = slot.borrow_mut();
    let state = tees.get_mut(&tee)?;
    if state.canceled[branch as usize] || state.reading {
      return None;
    }
    state.reading = true;
    Some(state.reader)
  });
  let Some(reader) = reader else {
    return;
  };
  // Queue the chunk reaction before `pull`, so a pull that errors the source
  // cannot overtake a chunk this read already dequeued.
  let read = reader_read_at(scope, reader, false);
  let value = js_value(scope, read);
  let _ = watch_promise(scope, value, Token::TeeRead(tee));
  let source = READERS.with(|slot| {
    slot.borrow().get(&reader).and_then(|state| {
      if state.released {
        None
      } else {
        Some(state.stream)
      }
    })
  });
  if let Some(source) = source {
    pull_if_needed(scope, source);
  }
}

fn tee_chunk(scope: &mut v8::PinScope, tee: u64, value: v8::Local<v8::Value>) {
  TEES.with(|slot| {
    if let Some(state) = slot.borrow_mut().get_mut(&tee) {
      state.reading = false;
    }
  });
  let Some((done, chunk)) = read_record(scope, value) else {
    let error = type_error(scope, "The read result is not an object");
    tee_fail(scope, tee, error);
    return;
  };
  let (branches, canceled) = TEES
    .with(|slot| {
      slot
        .borrow()
        .get(&tee)
        .map(|state| (state.branches, state.canceled))
    })
    .unwrap_or(([0, 0], [true, true]));
  if done {
    for (index, branch) in branches.into_iter().enumerate() {
      if !canceled[index] {
        let _ = close_stream(scope, branch);
      }
    }
    tee_resolve_cancel(scope, tee);
    return;
  }
  let cloned = match yuzuko_core::clone_value(scope, chunk) {
    Ok(cloned) => cloned,
    Err(Some(thrown)) => {
      let error = v8::Local::new(scope, &thrown);
      tee_fail(scope, tee, error);
      return;
    }
    Err(None) => {
      let error = type_error(scope, "The chunk could not be cloned");
      tee_fail(scope, tee, error);
      return;
    }
  };
  if !canceled[0] {
    let _ = enqueue_chunk(scope, branches[0], chunk, Some(1.0));
  }
  if !canceled[1] {
    let _ = enqueue_chunk(scope, branches[1], cloned, Some(1.0));
  }
  pull_if_needed(scope, branches[0]);
  pull_if_needed(scope, branches[1]);
}

fn tee_resolve_cancel(scope: &mut v8::PinScope, tee: u64) {
  let resolver = TEES.with(|slot| {
    slot
      .borrow()
      .get(&tee)
      .map(|state| state.cancel_resolver.clone())
  });
  let Some(resolver) = resolver else {
    return;
  };
  let resolver = v8::Local::new(scope, &resolver);
  let resolver = yuzuko_core::op::root(scope, resolver);
  resolver.resolve(scope, v8::undefined(scope).into());
}

fn tee_fail(scope: &mut v8::PinScope, tee: u64, error: v8::Local<v8::Value>) {
  let branches = TEES.with(|slot| {
    let mut tees = slot.borrow_mut();
    let state = tees.get_mut(&tee)?;
    state.reading = false;
    Some(state.branches)
  });
  let Some(branches) = branches else {
    return;
  };
  let error = yuzuko_core::op::root(scope, error);
  for branch in branches {
    let _ = error_stream(scope, branch, error);
  }
}

fn read_record<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  value: v8::Local<v8::Value>,
) -> Option<(bool, v8::Local<'s, v8::Value>)> {
  let object = v8::Local::<v8::Object>::try_from(value).ok()?;
  let object = yuzuko_core::op::root(scope, object);
  let done_key = v8::String::new(scope, "done").unwrap();
  let value_key = v8::String::new(scope, "value").unwrap();
  let done = object.get(scope, done_key.into())?;
  let done = done.boolean_value(scope);
  let chunk = object
    .get(scope, value_key.into())
    .unwrap_or_else(|| v8::undefined(scope).into());
  Some((done, yuzuko_core::op::root(scope, chunk)))
}

fn js_value<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  value: Js,
) -> v8::Local<'s, v8::Value> {
  let Js(global) = value;
  let local = v8::Local::new(scope, &global);
  yuzuko_core::op::root(scope, local)
}

fn read_pending(id: u64) -> bool {
  let reader =
    READS.with(|slot| slot.borrow().get(&id).and_then(|state| state.reader));
  reader.is_some_and(|reader| {
    READERS.with(|slot| {
      slot
        .borrow()
        .get(&reader)
        .is_some_and(|state| !state.requests.is_empty())
    })
  })
}

fn pipe_to<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  readable: u64,
  dest: v8::Local<'s, v8::Value>,
  options: Option<v8::Local<'s, v8::Value>>,
) -> Js {
  let flags = match pipe_flags(scope, options) {
    Ok(flags) => flags,
    Err(error) => {
      let thrown = error_value(scope, &error);
      return rejected(scope, thrown);
    }
  };
  pipe_with_flags(scope, readable, dest, flags)
}

fn pipe_with_flags<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  readable: u64,
  dest: v8::Local<'s, v8::Value>,
  flags: PipeFlags<'s>,
) -> Js {
  let writable = match writable_id(scope, dest) {
    Ok(id) => id,
    Err(error) => {
      let thrown = error_value(scope, &error);
      return rejected(scope, thrown);
    }
  };
  let readable_locked = READS.with(|slot| {
    slot
      .borrow()
      .get(&readable)
      .is_some_and(|state| state.reader.is_some())
  });
  let writable_locked = WRITES.with(|slot| {
    slot
      .borrow()
      .get(&writable)
      .is_some_and(|state| state.writer.is_some())
  });
  if readable_locked || writable_locked {
    let error = type_error(scope, "The stream is locked");
    return rejected(scope, error);
  }
  // `pipeTo` disturbs the source before any chunk is read.
  READS.with(|slot| {
    if let Some(state) = slot.borrow_mut().get_mut(&readable) {
      state.disturbed = true;
    }
  });
  let reader = match acquire_reader(scope, readable) {
    Ok(reader) => reader,
    Err(error) => {
      let thrown = error_value(scope, &error);
      return rejected(scope, thrown);
    }
  };
  let writer = match acquire_writer(scope, writable) {
    Ok(writer) => writer,
    Err(error) => {
      let _ = release_reader(scope, reader.id);
      let thrown = error_value(scope, &error);
      return rejected(scope, thrown);
    }
  };
  let resolver = v8::PromiseResolver::new(scope).unwrap();
  let promise = resolver.get_promise(scope);
  let id = alloc();
  PIPES.with(|slot| {
    slot.borrow_mut().insert(
      id,
      Pipe {
        readable,
        writable,
        reader: reader.id,
        writer: writer.id,
        prevent_close: flags.0,
        prevent_abort: flags.1,
        prevent_cancel: flags.2,
        resolver: v8::Global::new(scope, resolver),
        settled: false,
        shutting_down: false,
        plan_started: false,
        plan: PipePlan::Idle,
        latest_write: None,
        signal: None,
        abort_listener: None,
        signal_left: 0,
        signal_error: None,
        signal_drained: false,
      },
    );
  });
  if let Some(signal) = flags.3 {
    match aborted_reason(scope, signal) {
      Ok(Some(reason)) => {
        watch_pipe_closed(scope, id);
        begin_shutdown(scope, id, PipePlan::Signal(reason));
      }
      Ok(None) => match add_abort_listener(scope, signal, id) {
        Ok(listener) => {
          PIPES.with(|slot| {
            if let Some(pipe) = slot.borrow_mut().get_mut(&id) {
              pipe.signal = Some(v8::Global::new(scope, signal));
              pipe.abort_listener = Some(listener);
            }
          });
          watch_pipe_closed(scope, id);
          if !shutdown_ready_source(scope, id) {
            pipe_initial(scope, id);
          }
        }
        Err(error) => {
          let thrown = error_value(scope, &error);
          return abandon_pipe(scope, id, thrown);
        }
      },
      Err(error) => {
        let thrown = error_value(scope, &error);
        return abandon_pipe(scope, id, thrown);
      }
    }
  } else {
    watch_pipe_closed(scope, id);
    if !shutdown_ready_source(scope, id) {
      pipe_initial(scope, id);
    }
  }
  let value: v8::Local<v8::Value> = promise.into();
  Js(v8::Global::new(scope, value))
}

fn shutdown_ready_source(scope: &mut v8::PinScope, id: u64) -> bool {
  let state = PIPES.with(|slot| {
    slot
      .borrow()
      .get(&id)
      .map(|pipe| (pipe.readable, pipe.prevent_abort, pipe.prevent_close))
  });
  let Some((readable, prevent_abort, prevent_close)) = state else {
    return false;
  };
  let source = READS.with(|slot| {
    slot
      .borrow()
      .get(&readable)
      .map(|state| (state.phase, state.error.clone()))
  });
  match source {
    Some((Phase::Errored, error)) => {
      let error = error
        .map(|error| v8::Local::new(scope, &error))
        .unwrap_or_else(|| type_error(scope, "The stream is errored"));
      let error = yuzuko_core::op::root(scope, error);
      let stored = v8::Global::new(scope, error);
      if prevent_abort {
        begin_shutdown(scope, id, PipePlan::Fail(stored));
      } else {
        begin_shutdown(scope, id, PipePlan::Abort(stored));
      }
      true
    }
    Some((Phase::Closed, _)) => {
      if prevent_close {
        begin_shutdown(scope, id, PipePlan::Succeed);
      } else {
        begin_shutdown(scope, id, PipePlan::Close);
      }
      true
    }
    _ => false,
  }
}

fn pipe_initial(scope: &mut v8::PinScope, id: u64) {
  let state = PIPES.with(|slot| {
    slot
      .borrow()
      .get(&id)
      .map(|pipe| (pipe.writable, pipe.prevent_cancel))
  });
  let Some((writable, prevent_cancel)) = state else {
    return;
  };
  if writable_close_queued(writable) {
    let error = type_error(
      scope,
      "The destination writable stream closed before all data could be piped to it",
    );
    let stored = v8::Global::new(scope, error);
    if prevent_cancel {
      begin_shutdown(scope, id, PipePlan::Fail(stored));
    } else {
      begin_shutdown(scope, id, PipePlan::Cancel(stored));
    }
  } else {
    pipe_step(scope, id);
  }
}

fn abandon_pipe(
  scope: &mut v8::PinScope,
  id: u64,
  error: v8::Local<v8::Value>,
) -> Js {
  let pipe = PIPES.with(|slot| slot.borrow_mut().remove(&id));
  let Some(pipe) = pipe else {
    return rejected(scope, error);
  };
  let _ = release_reader(scope, pipe.reader);
  let _ = release_writer(scope, pipe.writer);
  reject_resolver(scope, pipe.resolver.clone(), error);
  let resolver = v8::Local::new(scope, &pipe.resolver);
  let promise = resolver.get_promise(scope);
  let value: v8::Local<v8::Value> = promise.into();
  Js(v8::Global::new(scope, value))
}

type PipeFlags<'s> = (bool, bool, bool, Option<v8::Local<'s, v8::Object>>);

fn pipe_flags<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  options: Option<v8::Local<v8::Value>>,
) -> Result<PipeFlags<'s>, OpError> {
  let Some(options) = options.filter(|value| !value.is_null_or_undefined())
  else {
    return Ok((false, false, false, None));
  };
  let object = object_value(scope, options)?;
  let prevent_abort = pipe_flag(scope, object, "preventAbort")?;
  let prevent_cancel = pipe_flag(scope, object, "preventCancel")?;
  let prevent_close = pipe_flag(scope, object, "preventClose")?;
  let signal = match property(scope, object, "signal")? {
    Some(signal) if !signal.is_undefined() => {
      Some(abort_signal(scope, signal)?)
    }
    _ => None,
  };
  Ok((prevent_close, prevent_abort, prevent_cancel, signal))
}

fn abort_signal<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  value: v8::Local<v8::Value>,
) -> Result<v8::Local<'s, v8::Object>, OpError> {
  let Ok(object) = v8::Local::<v8::Object>::try_from(value) else {
    return Err(OpError::type_("The signal is not an AbortSignal"));
  };
  // A prototype impersonator is `instanceof AbortSignal` and has no host brand.
  if !object.is_api_wrapper() {
    return Err(OpError::type_("The signal is not an AbortSignal"));
  }
  let global = scope.get_current_context().global(scope);
  let key = v8::String::new(scope, "AbortSignal").unwrap();
  let Some(ctor) = global.get(scope, key.into()) else {
    return Err(OpError::type_("The signal is not an AbortSignal"));
  };
  let Ok(ctor) = v8::Local::<v8::Object>::try_from(ctor) else {
    return Err(OpError::type_("The signal is not an AbortSignal"));
  };
  let object = yuzuko_core::op::root(scope, object);
  let ctor = yuzuko_core::op::root(scope, ctor);
  let probe: v8::Local<v8::Value> = object.into();
  if probe.instance_of(scope, ctor) != Some(true) {
    return Err(OpError::type_("The signal is not an AbortSignal"));
  }
  v8::Local::<v8::Object>::try_from(probe)
    .map_err(|_| OpError::type_("The signal is not an AbortSignal"))
}

fn aborted_reason(
  scope: &mut v8::PinScope,
  signal: v8::Local<v8::Object>,
) -> Result<Option<v8::Global<v8::Value>>, OpError> {
  let aborted = property(scope, signal, "aborted")?
    .is_some_and(|value| value.boolean_value(scope));
  if !aborted {
    return Ok(None);
  }
  let reason = property(scope, signal, "reason")?
    .unwrap_or_else(|| v8::undefined(scope).into());
  Ok(Some(v8::Global::new(scope, reason)))
}

fn pipe_flag(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
) -> Result<bool, OpError> {
  Ok(
    property(scope, object, name)?
      .is_some_and(|value| value.boolean_value(scope)),
  )
}

fn writable_close_queued(stream: u64) -> bool {
  WRITES.with(|slot| {
    slot.borrow().get(&stream).is_some_and(|state| {
      state.phase == Phase::Closed || state.closing || state.close_started
    })
  })
}

fn pipe_step(scope: &mut v8::PinScope, id: u64) {
  let active = PIPES.with(|slot| {
    slot
      .borrow()
      .get(&id)
      .is_some_and(|pipe| !pipe.settled && !pipe.shutting_down)
  });
  if !active {
    return;
  }
  let (writable, writer) = PIPES
    .with(|slot| {
      slot
        .borrow()
        .get(&id)
        .map(|pipe| (pipe.writable, pipe.writer))
    })
    .unwrap_or((0, 0));
  let desired = write_desired(writable).unwrap_or(0.0);
  if desired <= 0.0 {
    ensure_pending_ready(scope, writer);
    if let Some(ready) = writer_promise(scope, writer, false) {
      let _ = watch_promise(scope, ready, Token::PipeReady(id));
    }
    return;
  }
  pipe_read(scope, id);
}

fn watch_pipe_closed(scope: &mut v8::PinScope, id: u64) {
  let (reader, writer) = PIPES
    .with(|slot| {
      slot
        .borrow()
        .get(&id)
        .map(|pipe| (pipe.reader, pipe.writer))
    })
    .unwrap_or((0, 0));
  if let Some(closed) = reader_closed_promise(scope, reader) {
    let _ = watch_promise(scope, closed, Token::PipeSourceClosed(id));
  }
  if let Some(closed) = writer_promise(scope, writer, true) {
    let _ = watch_promise(scope, closed, Token::PipeDestClosed(id));
  }
}

fn reader_closed_promise<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  reader: u64,
) -> Option<v8::Local<'s, v8::Value>> {
  let promise = READERS
    .with(|slot| slot.borrow().get(&reader).map(|state| state.closed.clone()));
  promise.map(|promise| {
    let local = v8::Local::new(scope, &promise);
    let value: v8::Local<v8::Value> = local.into();
    yuzuko_core::op::root(scope, value)
  })
}

fn writer_promise<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  writer: u64,
  closed: bool,
) -> Option<v8::Local<'s, v8::Value>> {
  let promise = WRITERS.with(|slot| {
    slot.borrow().get(&writer).map(|state| {
      if closed {
        state.closed.clone()
      } else {
        state.ready.clone()
      }
    })
  });
  promise.map(|promise| {
    let local = v8::Local::new(scope, &promise);
    let value: v8::Local<v8::Value> = local.into();
    yuzuko_core::op::root(scope, value)
  })
}

fn pipe_read(scope: &mut v8::PinScope, id: u64) {
  let reader = PIPES.with(|slot| {
    slot
      .borrow()
      .get(&id)
      .filter(|pipe| !pipe.settled)
      .map(|pipe| pipe.reader)
  });
  let Some(reader) = reader else {
    return;
  };
  let read = reader_read(scope, reader);
  let value = js_value(scope, read);
  let _ = watch_promise(scope, value, Token::PipeRead(id));
}

fn pipe_on_read(
  scope: &mut v8::PinScope,
  id: u64,
  result: Result<v8::Local<v8::Value>, v8::Local<v8::Value>>,
) {
  let Ok(value) = result else {
    return;
  };
  let Some((done, chunk)) = read_record(scope, value) else {
    let error = type_error(scope, "The read result is not an object");
    begin_shutdown(scope, id, PipePlan::Fail(v8::Global::new(scope, error)));
    return;
  };
  if done {
    return;
  }
  let writable =
    PIPES.with(|slot| slot.borrow().get(&id).map(|pipe| pipe.writable));
  let Some(writable) = writable else {
    return;
  };
  let write = write_chunk(scope, writable, chunk, None);
  let promise = js_value(scope, write);
  let promise = promise_from(scope, promise);
  PIPES.with(|slot| {
    if let Some(pipe) = slot.borrow_mut().get_mut(&id) {
      pipe.latest_write = Some(v8::Global::new(scope, promise));
    }
  });
  let value: v8::Local<v8::Value> = promise.into();
  let _ = watch_promise(scope, value, Token::PipeWriteDone(id));
  let shutting = PIPES.with(|slot| {
    slot
      .borrow()
      .get(&id)
      .is_some_and(|pipe| pipe.shutting_down || pipe.settled)
  });
  if !shutting {
    pipe_step(scope, id);
  }
}

fn pipe_on_source_closed(
  scope: &mut v8::PinScope,
  id: u64,
  result: Result<v8::Local<v8::Value>, v8::Local<v8::Value>>,
) {
  let busy = PIPES.with(|slot| {
    slot
      .borrow()
      .get(&id)
      .is_some_and(|pipe| pipe.shutting_down || pipe.settled)
  });
  if busy {
    return;
  }
  match result {
    Ok(_) => {
      let prevent_close = PIPES.with(|slot| {
        slot
          .borrow()
          .get(&id)
          .is_some_and(|pipe| pipe.prevent_close)
      });
      let plan = if prevent_close {
        PipePlan::Succeed
      } else {
        PipePlan::Close
      };
      // Arm now so a later abort does not replace the close. Perform the
      // close on a microtask so a read that fulfilled `closed` in this turn
      // can still write its chunk.
      if arm_shutdown(id, plan) {
        let pending = v8::undefined(scope).into();
        let _ = watch_promise(scope, pending, Token::PipeDeferredClose(id));
      }
    }
    Err(error) => {
      let prevent_abort = PIPES.with(|slot| {
        slot
          .borrow()
          .get(&id)
          .is_some_and(|pipe| pipe.prevent_abort)
      });
      let stored = v8::Global::new(scope, error);
      if prevent_abort {
        begin_shutdown(scope, id, PipePlan::Fail(stored));
      } else {
        begin_shutdown(scope, id, PipePlan::Abort(stored));
      }
    }
  }
}

fn pipe_on_deferred_close(scope: &mut v8::PinScope, id: u64) {
  try_run_plan(scope, id);
}

fn pipe_on_dest_closed(
  scope: &mut v8::PinScope,
  id: u64,
  error: v8::Local<v8::Value>,
) {
  let busy = PIPES.with(|slot| {
    slot
      .borrow()
      .get(&id)
      .is_some_and(|pipe| pipe.shutting_down || pipe.settled)
  });
  if busy {
    return;
  }
  let prevent_cancel = PIPES.with(|slot| {
    slot
      .borrow()
      .get(&id)
      .is_some_and(|pipe| pipe.prevent_cancel)
  });
  let stored = v8::Global::new(scope, error);
  if prevent_cancel {
    begin_shutdown(scope, id, PipePlan::Fail(stored));
  } else {
    begin_shutdown(scope, id, PipePlan::Cancel(stored));
  }
}

fn arm_shutdown(id: u64, plan: PipePlan) -> bool {
  PIPES.with(|slot| {
    let mut pipes = slot.borrow_mut();
    let Some(pipe) = pipes.get_mut(&id) else {
      return false;
    };
    if pipe.shutting_down || pipe.settled {
      return false;
    }
    pipe.shutting_down = true;
    pipe.plan = plan;
    true
  })
}

fn begin_shutdown(scope: &mut v8::PinScope, id: u64, plan: PipePlan) {
  if arm_shutdown(id, plan) {
    try_run_plan(scope, id);
  }
}

fn try_run_plan(scope: &mut v8::PinScope, id: u64) {
  consider_plan(scope, id, true);
}

fn continue_plan(scope: &mut v8::PinScope, id: u64) {
  consider_plan(scope, id, false);
}

/// Runs a shutdown plan once the destination has no queued or in-flight
/// writes. An already-idle abort signal waits one extra microtask so
/// `start` can settle and sink `abort` runs before source `cancel`.
fn consider_plan(scope: &mut v8::PinScope, id: u64, defer_idle_signal: bool) {
  let snapshot = PIPES.with(|slot| {
    let pipes = slot.borrow();
    let pipe = pipes.get(&id)?;
    if !pipe.shutting_down || pipe.plan_started || pipe.settled {
      return None;
    }
    Some((
      pipe.writable,
      matches!(pipe.plan, PipePlan::Signal(_)),
      pipe.signal_drained,
    ))
  });
  let Some((writable, signal, drained)) = snapshot else {
    return;
  };
  let gate = WRITES.with(|slot| {
    slot.borrow().get(&writable).map(|state| {
      (
        state.phase == Phase::Readable,
        state.closing || state.close_started,
        state.in_flight > 0 || !state.queue.is_empty(),
      )
    })
  });
  let Some((open, closing, busy)) = gate else {
    run_plan(scope, id);
    return;
  };
  if open && !closing && busy {
    return;
  }
  if defer_idle_signal && signal && !drained && open && !closing {
    PIPES.with(|slot| {
      if let Some(pipe) = slot.borrow_mut().get_mut(&id) {
        pipe.signal_drained = true;
      }
    });
    let pending = v8::undefined(scope).into();
    let _ = watch_promise(scope, pending, Token::PipeDrain(id));
    return;
  }
  run_plan(scope, id);
}

fn run_plan(scope: &mut v8::PinScope, id: u64) {
  let plan = PIPES.with(|slot| {
    let mut pipes = slot.borrow_mut();
    let pipe = pipes.get_mut(&id)?;
    if pipe.plan_started || pipe.settled {
      return None;
    }
    pipe.plan_started = true;
    Some(std::mem::replace(&mut pipe.plan, PipePlan::Idle))
  });
  let Some(plan) = plan else {
    return;
  };
  let (readable, writable) = PIPES
    .with(|slot| {
      slot
        .borrow()
        .get(&id)
        .map(|pipe| (pipe.readable, pipe.writable))
    })
    .unwrap_or((0, 0));
  let action = match plan {
    PipePlan::Idle => return,
    PipePlan::Succeed => {
      pipe_settle(scope, id, Ok(()));
      return;
    }
    PipePlan::Fail(error) => {
      let error = v8::Local::new(scope, &error);
      pipe_settle(scope, id, Err(error));
      return;
    }
    PipePlan::Close => close_for_pipe(scope, writable),
    PipePlan::Abort(error) => {
      let error = v8::Local::new(scope, &error);
      let error = yuzuko_core::op::root(scope, error);
      PIPES.with(|slot| {
        if let Some(pipe) = slot.borrow_mut().get_mut(&id) {
          pipe.plan = PipePlan::Fail(v8::Global::new(scope, error));
        }
      });
      abort_for_pipe(scope, writable, error)
    }
    PipePlan::Cancel(error) => {
      let error = v8::Local::new(scope, &error);
      let error = yuzuko_core::op::root(scope, error);
      PIPES.with(|slot| {
        if let Some(pipe) = slot.borrow_mut().get_mut(&id) {
          pipe.plan = PipePlan::Fail(v8::Global::new(scope, error));
        }
      });
      cancel_for_pipe(scope, readable, error)
    }
    PipePlan::Signal(reason) => {
      let reason = v8::Local::new(scope, &reason);
      let reason = yuzuko_core::op::root(scope, reason);
      run_signal(scope, id, reason);
      return;
    }
  };
  let promise = js_value(scope, action);
  let _ = watch_promise(scope, promise, Token::PipeAction(id));
}

fn run_signal(scope: &mut v8::PinScope, id: u64, reason: v8::Local<v8::Value>) {
  let state = PIPES.with(|slot| {
    slot.borrow().get(&id).map(|pipe| {
      (
        pipe.prevent_abort,
        pipe.prevent_cancel,
        pipe.readable,
        pipe.writable,
      )
    })
  });
  let Some((prevent_abort, prevent_cancel, readable, writable)) = state else {
    return;
  };
  let dest_open = WRITES.with(|slot| {
    slot
      .borrow()
      .get(&writable)
      .is_some_and(|state| state.phase == Phase::Readable)
  });
  let source_open = READS.with(|slot| {
    slot
      .borrow()
      .get(&readable)
      .is_some_and(|state| state.phase == Phase::Readable)
  });
  let mut actions = Vec::new();
  if !prevent_abort && dest_open {
    actions.push(abort_for_pipe(scope, writable, reason));
  }
  if !prevent_cancel && source_open {
    actions.push(cancel_for_pipe(scope, readable, reason));
  }
  if actions.is_empty() {
    pipe_settle(scope, id, Err(reason));
    return;
  }
  PIPES.with(|slot| {
    if let Some(pipe) = slot.borrow_mut().get_mut(&id) {
      pipe.plan = PipePlan::Fail(v8::Global::new(scope, reason));
      pipe.signal_error = None;
      pipe.signal_left = u8::try_from(actions.len()).unwrap_or(u8::MAX);
    }
  });
  for action in actions {
    let promise = js_value(scope, action);
    let _ = watch_promise(scope, promise, Token::PipeSignal(id));
  }
}

fn pipe_on_signal(
  scope: &mut v8::PinScope,
  id: u64,
  result: Result<v8::Local<v8::Value>, v8::Local<v8::Value>>,
) {
  let rejection = result.err().map(|error| v8::Global::new(scope, error));
  let finished = PIPES.with(|slot| {
    let mut pipes = slot.borrow_mut();
    let pipe = pipes.get_mut(&id)?;
    if pipe.settled {
      return None;
    }
    if pipe.signal_error.is_none() {
      pipe.signal_error = rejection;
    }
    pipe.signal_left = pipe.signal_left.saturating_sub(1);
    if pipe.signal_left > 0 {
      return None;
    }
    Some(pipe.signal_error.take().or_else(|| {
      match std::mem::replace(&mut pipe.plan, PipePlan::Idle) {
        PipePlan::Fail(error) => Some(error),
        other => {
          pipe.plan = other;
          None
        }
      }
    }))
  });
  let Some(error) = finished else {
    return;
  };
  let Some(error) = error else {
    pipe_settle(scope, id, Ok(()));
    return;
  };
  let error = v8::Local::new(scope, &error);
  pipe_settle(scope, id, Err(error));
}

fn pipe_on_action(
  scope: &mut v8::PinScope,
  id: u64,
  result: Result<v8::Local<v8::Value>, v8::Local<v8::Value>>,
) {
  match result {
    Ok(_) => {
      let fallback =
        PIPES.with(|slot| {
          slot.borrow_mut().get_mut(&id).and_then(
            |pipe| match std::mem::replace(&mut pipe.plan, PipePlan::Idle) {
              PipePlan::Fail(error) => Some(error),
              _ => None,
            },
          )
        });
      if let Some(error) = fallback {
        let error = v8::Local::new(scope, &error);
        pipe_settle(scope, id, Err(error));
      } else {
        pipe_settle(scope, id, Ok(()));
      }
    }
    Err(error) => pipe_settle(scope, id, Err(error)),
  }
}

fn abort_for_pipe(
  scope: &mut v8::PinScope,
  stream: u64,
  reason: v8::Local<v8::Value>,
) -> Js {
  let phase =
    WRITES.with(|slot| slot.borrow().get(&stream).map(|state| state.phase));
  match phase {
    Some(Phase::Readable) => abort_write(scope, stream, reason),
    Some(Phase::Erroring) => abort_erroring(scope, stream),
    _ => resolved(scope, v8::undefined(scope).into()),
  }
}

fn close_for_pipe(scope: &mut v8::PinScope, stream: u64) -> Js {
  let state = WRITES.with(|slot| {
    slot.borrow().get(&stream).map(|state| {
      (
        state.phase,
        state.closing || state.close_started,
        state.error.clone(),
      )
    })
  });
  match state {
    Some((Phase::Closed, _, _)) | Some((_, true, _)) => {
      resolved(scope, v8::undefined(scope).into())
    }
    Some((Phase::Errored | Phase::Erroring, _, error)) => {
      let error = error
        .map(|error| v8::Local::new(scope, &error))
        .unwrap_or_else(|| type_error(scope, "The stream is errored"));
      rejected(scope, error)
    }
    _ => close_write(scope, stream),
  }
}

fn cancel_for_pipe(
  scope: &mut v8::PinScope,
  stream: u64,
  reason: v8::Local<v8::Value>,
) -> Js {
  let phase =
    READS.with(|slot| slot.borrow().get(&stream).map(|state| state.phase));
  match phase {
    Some(Phase::Closed) | None => resolved(scope, v8::undefined(scope).into()),
    Some(Phase::Errored | Phase::Erroring) => {
      let error = READS.with(|slot| {
        slot
          .borrow()
          .get(&stream)
          .and_then(|state| state.error.clone())
      });
      let error = error
        .map(|error| v8::Local::new(scope, &error))
        .unwrap_or_else(|| type_error(scope, "The stream is errored"));
      rejected(scope, error)
    }
    Some(Phase::Readable) => {
      let cancel = READS.with(|slot| {
        let mut states = slot.borrow_mut();
        let state = states.get_mut(&stream)?;
        state.disturbed = true;
        state.queue.clear();
        state.queued = 0.0;
        state.close_requested = true;
        state.cancel_fn.clone()
      });
      finish_close(scope, stream);
      let Some(cancel) = cancel else {
        return resolved(scope, v8::undefined(scope).into());
      };
      match call(scope, &cancel, v8::undefined(scope).into(), &[reason]) {
        Ok(value) => {
          let promise = promise_from(scope, value);
          let value: v8::Local<v8::Value> = promise.into();
          Js(v8::Global::new(scope, value))
        }
        Err(error) => {
          let thrown = error_value(scope, &error);
          rejected(scope, thrown)
        }
      }
    }
  }
}

fn pipe_settle(
  scope: &mut v8::PinScope,
  id: u64,
  result: Result<(), v8::Local<v8::Value>>,
) {
  let pipe = PIPES.with(|slot| {
    let mut pipes = slot.borrow_mut();
    let pipe = pipes.get_mut(&id)?;
    if pipe.settled {
      return None;
    }
    pipe.settled = true;
    Some((
      pipe.resolver.clone(),
      pipe.reader,
      pipe.writer,
      pipe.signal.clone(),
      pipe.abort_listener.clone(),
    ))
  });
  let Some((resolver, reader, writer, signal, listener)) = pipe else {
    return;
  };
  if let (Some(signal), Some(listener)) = (signal, listener) {
    remove_abort_listener(scope, signal, listener);
  }
  match result {
    Ok(()) => {
      let value = v8::undefined(scope).into();
      resolve_resolver(scope, resolver, value);
    }
    Err(error) => reject_resolver(scope, resolver, error),
  }
  let _ = release_reader(scope, reader);
  let _ = release_writer(scope, writer);
}

fn add_abort_listener(
  scope: &mut v8::PinScope,
  signal: v8::Local<v8::Object>,
  id: u64,
) -> Result<v8::Global<v8::Function>, OpError> {
  let key = v8::String::new(scope, "addEventListener").unwrap();
  let function = signal
    .get(scope, key.into())
    .and_then(|value| v8::Local::<v8::Function>::try_from(value).ok())
    .ok_or_else(|| OpError::type_("addEventListener is not a function"))?;
  let function = yuzuko_core::op::root(scope, function);
  let function = v8::Global::new(scope, function);
  let listener = v8::Function::builder(pipe_abort_listener)
    .data(v8::Number::new(scope, id as f64).into())
    .build(scope)
    .ok_or_else(|| OpError::type_("The abort listener could not be created"))?;
  let listener = yuzuko_core::op::root(scope, listener);
  let name = v8::String::new(scope, "abort").unwrap();
  let this: v8::Local<v8::Value> = signal.into();
  let listener_value: v8::Local<v8::Value> = listener.into();
  call(scope, &function, this, &[name.into(), listener_value])?;
  let listener = v8::Local::<v8::Function>::try_from(listener_value)
    .map_err(|_| OpError::type_("The abort listener could not be created"))?;
  Ok(v8::Global::new(scope, listener))
}

fn remove_abort_listener(
  scope: &mut v8::PinScope,
  signal: v8::Global<v8::Object>,
  listener: v8::Global<v8::Function>,
) {
  let signal = v8::Local::new(scope, &signal);
  let listener = v8::Local::new(scope, &listener);
  let key = v8::String::new(scope, "removeEventListener").unwrap();
  let Some(function) = signal
    .get(scope, key.into())
    .and_then(|value| v8::Local::<v8::Function>::try_from(value).ok())
  else {
    return;
  };
  let function = yuzuko_core::op::root(scope, function);
  let function = v8::Global::new(scope, function);
  let name = v8::String::new(scope, "abort").unwrap();
  let this: v8::Local<v8::Value> = signal.into();
  let listener: v8::Local<v8::Value> = listener.into();
  let _ = call(scope, &function, this, &[name.into(), listener]);
}

fn pipe_abort_listener(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut _ret: v8::ReturnValue,
) {
  let Some(id) = args.data().number_value(scope) else {
    return;
  };
  let id = id as u64;
  let signal = PIPES
    .with(|slot| slot.borrow().get(&id).and_then(|pipe| pipe.signal.clone()));
  let Some(signal) = signal else {
    return;
  };
  let signal = v8::Local::new(scope, &signal);
  let reason = match property(scope, signal, "reason") {
    Ok(Some(reason)) => reason,
    Ok(None) => v8::undefined(scope).into(),
    Err(error) => error_value(scope, &error),
  };
  begin_shutdown(scope, id, PipePlan::Signal(v8::Global::new(scope, reason)));
}

fn pipe_to_callback(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut ret: v8::ReturnValue,
) {
  let this = args.this();
  let id = unsafe { yuzuko_core::op::host_ref::<ReadableStream>(scope, this) }
    .map(|host| host.get().id);
  let Some(id) = id else {
    let error = type_error(scope, "pipeTo called on a non-ReadableStream");
    let promise = rejected(scope, error);
    yuzuko_core::op::ToV8::to_v8(promise, scope, &mut ret);
    return;
  };
  let dest = yuzuko_core::op::root(scope, args.get(0));
  let options = if args.length() > 1 {
    let value = args.get(1);
    if value.is_null_or_undefined() {
      None
    } else {
      Some(yuzuko_core::op::root(scope, value))
    }
  } else {
    None
  };
  let promise = pipe_to(scope, id, dest, options);
  yuzuko_core::op::ToV8::to_v8(promise, scope, &mut ret);
}

fn pipe_through_callback(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut ret: v8::ReturnValue,
) {
  let this = args.this();
  let id = unsafe { yuzuko_core::op::host_ref::<ReadableStream>(scope, this) }
    .map(|host| host.get().id);
  let Some(id) = id else {
    yuzuko_core::op::OpException::throw(
      OpError::type_("pipeThrough called on a non-ReadableStream"),
      scope,
    );
    return;
  };
  let transform = yuzuko_core::op::root(scope, args.get(0));
  let options = if args.length() > 1 {
    let value = args.get(1);
    if value.is_null_or_undefined() {
      None
    } else {
      Some(yuzuko_core::op::root(scope, value))
    }
  } else {
    None
  };
  let readable = match pipe_transform(scope, transform) {
    Ok(readable) => readable,
    Err(error) => {
      yuzuko_core::op::OpException::throw(error, scope);
      return;
    }
  };
  let flags = match pipe_flags(scope, options) {
    Ok(flags) => flags,
    Err(error) => {
      yuzuko_core::op::OpException::throw(error, scope);
      return;
    }
  };
  let locked = READS.with(|slot| {
    slot
      .borrow()
      .get(&id)
      .is_some_and(|state| state.reader.is_some())
  });
  if locked {
    yuzuko_core::op::OpException::throw(
      OpError::type_("The stream is locked"),
      scope,
    );
    return;
  }
  if let Ok(writable) = writable_id(scope, readable.1) {
    let locked = WRITES.with(|slot| {
      slot
        .borrow()
        .get(&writable)
        .is_some_and(|state| state.writer.is_some())
    });
    if locked {
      yuzuko_core::op::OpException::throw(
        OpError::type_("The stream is locked"),
        scope,
      );
      return;
    }
  }
  let piped = pipe_with_flags(scope, id, readable.1, flags);
  let local = js_value(scope, piped);
  if let Ok(promise) = v8::Local::<v8::Promise>::try_from(local) {
    promise.mark_as_handled();
  }
  yuzuko_core::op::ToV8::to_v8(readable.0, scope, &mut ret);
}

fn pipe_transform<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  transform: v8::Local<v8::Value>,
) -> Result<(Js, v8::Local<'s, v8::Value>), OpError> {
  let object = object_value(scope, transform)?;
  let readable = property(scope, object, "readable")?
    .ok_or_else(|| OpError::type_("pipeThrough requires a readable"))?;
  stream_id(scope, readable)?;
  let writable = property(scope, object, "writable")?
    .ok_or_else(|| OpError::type_("pipeThrough requires a writable"))?;
  writable_id(scope, writable)?;
  Ok((Js(v8::Global::new(scope, readable)), writable))
}

struct IterGate {
  ongoing: v8::Global<v8::Promise>,
  finished: bool,
  /// `next()` has been called and its read has not settled.
  reading: bool,
  prevent_cancel: bool,
}

struct ReadableStreamAsyncIterator {
  reader: u64,
  prevent_cancel: bool,
}

unsafe impl v8::cppgc::GarbageCollected for ReadableStreamAsyncIterator {
  fn trace(&self, _visitor: &mut v8::cppgc::Visitor) {}

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"ReadableStreamAsyncIterator"
  }
}

#[op]
impl ReadableStreamAsyncIterator {
  fn next(&self, scope: &mut v8::PinScope) -> Js {
    ITERS.with(|slot| {
      if let Some(gate) = slot.borrow_mut().get_mut(&self.reader) {
        gate.reading = true;
      }
    });
    let worker = iter_bind_reader(scope, iter_next_worker, self.reader);
    iter_chain(scope, self.reader, worker)
  }

  #[rename("return")]
  fn return_(
    &self,
    scope: &mut v8::PinScope,
    reason: Option<v8::Local<v8::Value>>,
  ) -> Js {
    let reason = reason.unwrap_or_else(|| v8::undefined(scope).into());
    let reason = yuzuko_core::op::root(scope, reason);
    let reading = ITERS.with(|slot| {
      slot
        .borrow()
        .get(&self.reader)
        .is_some_and(|gate| gate.reading)
    });
    if reading || iter_finished(self.reader) {
      let worker =
        iter_bind_reason(scope, iter_return_worker, self.reader, reason);
      return iter_chain(scope, self.reader, worker);
    }
    iter_return_now(scope, self.reader, self.prevent_cancel, reason)
  }
}

fn iterator_released(reader: u64) -> bool {
  READERS.with(|slot| {
    slot
      .borrow()
      .get(&reader)
      .is_none_or(|state| state.released)
  })
}

fn iter_finished(reader: u64) -> bool {
  ITERS
    .with(|slot| slot.borrow().get(&reader).is_some_and(|gate| gate.finished))
}

fn resolved_promise(scope: &mut v8::PinScope) -> v8::Global<v8::Promise> {
  let promise = resolved(scope, v8::undefined(scope).into());
  let value = js_value(scope, promise);
  let promise = v8::Local::<v8::Promise>::try_from(value).unwrap();
  v8::Global::new(scope, promise)
}

fn iter_chain(
  scope: &mut v8::PinScope,
  reader: u64,
  worker: v8::Local<v8::Function>,
) -> Js {
  let prev = ITERS
    .with(|slot| slot.borrow().get(&reader).map(|gate| gate.ongoing.clone()));
  let Some(prev) = prev else {
    let result = read_result(scope, None, true);
    return resolved(scope, result);
  };
  let prev = v8::Local::new(scope, &prev);
  let prev = yuzuko_core::op::root(scope, prev);
  let worker = yuzuko_core::op::root(scope, worker);
  let step = prev.then2(scope, worker, worker).unwrap();
  let swallow = v8::Function::new(scope, iter_swallow).unwrap();
  let swallow = yuzuko_core::op::root(scope, swallow);
  let cont = step.then2(scope, swallow, swallow).unwrap();
  ITERS.with(|slot| {
    if let Some(gate) = slot.borrow_mut().get_mut(&reader) {
      gate.ongoing = v8::Global::new(scope, cont);
    }
  });
  let value: v8::Local<v8::Value> = step.into();
  Js(v8::Global::new(scope, value))
}

fn iter_swallow(
  _scope: &mut v8::PinScope,
  _args: v8::FunctionCallbackArguments,
  mut _ret: v8::ReturnValue,
) {
}

fn iter_bind_reader<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  callback: impl v8::MapFnTo<v8::FunctionCallback>,
  reader: u64,
) -> v8::Local<'s, v8::Function> {
  let function = v8::Function::new(scope, callback).unwrap();
  let function = yuzuko_core::op::root(scope, function);
  let bind_key = v8::String::new(scope, "bind").unwrap();
  let bind = function.get(scope, bind_key.into()).unwrap();
  let bind = v8::Local::<v8::Function>::try_from(bind).unwrap();
  let bind = yuzuko_core::op::root(scope, bind);
  let this_arg = v8::undefined(scope).into();
  let id: v8::Local<v8::Value> = v8::Number::new(scope, reader as f64).into();
  let bound = bind.call(scope, function.into(), &[this_arg, id]).unwrap();
  let bound = v8::Local::<v8::Function>::try_from(bound).unwrap();
  yuzuko_core::op::root(scope, bound)
}

fn iter_bind_reason<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  callback: impl v8::MapFnTo<v8::FunctionCallback>,
  reader: u64,
  reason: v8::Local<v8::Value>,
) -> v8::Local<'s, v8::Function> {
  let function = v8::Function::new(scope, callback).unwrap();
  let function = yuzuko_core::op::root(scope, function);
  let bind_key = v8::String::new(scope, "bind").unwrap();
  let bind = function.get(scope, bind_key.into()).unwrap();
  let bind = v8::Local::<v8::Function>::try_from(bind).unwrap();
  let bind = yuzuko_core::op::root(scope, bind);
  let this_arg = v8::undefined(scope).into();
  let id: v8::Local<v8::Value> = v8::Number::new(scope, reader as f64).into();
  let reason = yuzuko_core::op::root(scope, reason);
  let bound = bind
    .call(scope, function.into(), &[this_arg, id, reason])
    .unwrap();
  let bound = v8::Local::<v8::Function>::try_from(bound).unwrap();
  yuzuko_core::op::root(scope, bound)
}

fn iter_next_worker(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut ret: v8::ReturnValue,
) {
  let Some(reader) = args.get(0).number_value(scope) else {
    return;
  };
  let reader = reader as u64;
  ITERS.with(|slot| {
    if let Some(gate) = slot.borrow_mut().get_mut(&reader) {
      gate.reading = false;
    }
  });
  if iter_finished(reader) || iterator_released(reader) {
    ret.set(read_result(scope, None, true));
    return;
  }
  let read = reader_read(scope, reader);
  let followed = iter_follow_read(scope, reader, read);
  ret.set(js_value(scope, followed));
}

fn iter_follow_read(scope: &mut v8::PinScope, reader: u64, read: Js) -> Js {
  let value = js_value(scope, read);
  let promise = promise_from(scope, value);
  let fulfill = iter_bind_reader(scope, iter_read_ok, reader);
  let reject = iter_bind_reader(scope, iter_read_err, reader);
  let next = promise.then2(scope, fulfill, reject).unwrap();
  let value: v8::Local<v8::Value> = next.into();
  Js(v8::Global::new(scope, value))
}

fn iter_read_ok(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut ret: v8::ReturnValue,
) {
  let Some(reader) = args.get(0).number_value(scope) else {
    return;
  };
  let result = args.get(1);
  if iter_result_done(scope, result) {
    iter_mark_finished(scope, reader as u64);
  }
  ret.set(result);
}

fn iter_read_err(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut _ret: v8::ReturnValue,
) {
  let Some(reader) = args.get(0).number_value(scope) else {
    return;
  };
  iter_mark_finished(scope, reader as u64);
  scope.throw_exception(args.get(1));
}

fn iter_result_done(
  scope: &mut v8::PinScope,
  result: v8::Local<v8::Value>,
) -> bool {
  let Ok(object) = v8::Local::<v8::Object>::try_from(result) else {
    return false;
  };
  let key = v8::String::new(scope, "done").unwrap();
  object
    .get(scope, key.into())
    .is_some_and(|value| value.boolean_value(scope))
}

fn iter_mark_finished(scope: &mut v8::PinScope, reader: u64) {
  ITERS.with(|slot| {
    if let Some(gate) = slot.borrow_mut().get_mut(&reader) {
      gate.finished = true;
    }
  });
  if !iterator_released(reader) {
    let _ = release_reader(scope, reader);
  }
}

fn iter_return_worker(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut ret: v8::ReturnValue,
) {
  let Some(reader_num) = args.get(0).number_value(scope) else {
    return;
  };
  let reader = reader_num as u64;
  let reason = args.get(1);
  let reason = yuzuko_core::op::root(scope, reason);
  if iter_finished(reader) || iterator_released(reader) {
    ret.set(read_result(scope, Some(reason), true));
    return;
  }
  let value = iter_return_now(scope, reader, iter_prevent(reader), reason);
  ret.set(js_value(scope, value));
}

fn iter_prevent(reader: u64) -> bool {
  ITERS.with(|slot| {
    slot
      .borrow()
      .get(&reader)
      .is_some_and(|gate| gate.prevent_cancel)
  })
}

fn iter_return_now(
  scope: &mut v8::PinScope,
  reader: u64,
  prevent_cancel: bool,
  reason: v8::Local<v8::Value>,
) -> Js {
  let reason = yuzuko_core::op::root(scope, reason);
  let stream =
    READERS.with(|slot| slot.borrow().get(&reader).map(|state| state.stream));
  let phase = stream.and_then(|stream| {
    READS.with(|slot| slot.borrow().get(&stream).map(|state| state.phase))
  });
  if phase == Some(Phase::Errored) {
    let error = stream.and_then(|stream| {
      READS.with(|slot| {
        slot
          .borrow()
          .get(&stream)
          .and_then(|state| state.error.clone())
      })
    });
    iter_mark_finished(scope, reader);
    let thrown = error
      .map(|error| v8::Local::new(scope, &error))
      .unwrap_or_else(|| type_error(scope, "The stream is errored"));
    return rejected(scope, thrown);
  }
  let result = read_result(scope, Some(reason), true);
  let cancel = if !prevent_cancel && phase == Some(Phase::Readable) {
    stream.map(|stream| cancel_stream(scope, stream, Some(reason)))
  } else {
    None
  };
  iter_mark_finished(scope, reader);
  let Some(cancel) = cancel else {
    let worker = iter_bind_reason(scope, iter_done_worker, reader, reason);
    return iter_chain(scope, reader, worker);
  };
  let cancel = js_value(scope, cancel);
  let user = promise_map(scope, cancel, result);
  let value = {
    let Js(global) = &user;
    v8::Local::new(scope, global)
  };
  if let Ok(promise) = v8::Local::<v8::Promise>::try_from(value) {
    let swallow = v8::Function::new(scope, iter_swallow).unwrap();
    let swallow = yuzuko_core::op::root(scope, swallow);
    if let Some(cont) = promise.then2(scope, swallow, swallow) {
      ITERS.with(|slot| {
        if let Some(gate) = slot.borrow_mut().get_mut(&reader) {
          gate.ongoing = v8::Global::new(scope, cont);
        }
      });
    }
  }
  user
}

fn iter_done_worker(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut ret: v8::ReturnValue,
) {
  let reason = args.get(1);
  ret.set(read_result(scope, Some(reason), true));
}

fn stream_async_iterator(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut ret: v8::ReturnValue,
) {
  let this = args.this();
  let id = unsafe { yuzuko_core::op::host_ref::<ReadableStream>(scope, this) }
    .map(|host| host.get().id);
  let Some(id) = id else {
    yuzuko_core::op::OpException::throw(
      OpError::type_("The provided value is not a ReadableStream"),
      scope,
    );
    return;
  };
  let prevent_cancel = match iter_prevent_cancel(scope, args.get(0)) {
    Ok(value) => value,
    Err(error) => {
      yuzuko_core::op::OpException::throw(error, scope);
      return;
    }
  };
  let reader = match acquire_reader(scope, id) {
    Ok(reader) => reader,
    Err(error) => {
      yuzuko_core::op::OpException::throw(error, scope);
      return;
    }
  };
  let _ = ReadableStreamAsyncIterator::template(scope);
  let ongoing = resolved_promise(scope);
  ITERS.with(|slot| {
    slot.borrow_mut().insert(
      reader.id,
      IterGate {
        ongoing,
        finished: false,
        reading: false,
        prevent_cancel,
      },
    );
  });
  let object = unsafe {
    yuzuko_core::op::wrap_instance(
      scope,
      ReadableStreamAsyncIterator {
        reader: reader.id,
        prevent_cancel,
      },
    )
  };
  ret.set(object.into());
}

fn iter_prevent_cancel(
  scope: &mut v8::PinScope,
  options: v8::Local<v8::Value>,
) -> Result<bool, OpError> {
  if options.is_undefined() || options.is_null() {
    return Ok(false);
  }
  if !options.is_object() {
    return Err(OpError::type_("The iterator options are not an object"));
  }
  let object = object_value(scope, options)?;
  let Some(value) = property(scope, object, "preventCancel")? else {
    return Ok(false);
  };
  Ok(value.boolean_value(scope))
}

fn promise_map(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
  mapped: v8::Local<v8::Value>,
) -> Js {
  let promise = promise_from(scope, value);
  let fulfill = v8::Function::new(scope, iter_map_fulfill).unwrap();
  let fulfill = yuzuko_core::op::root(scope, fulfill);
  let bind_key = v8::String::new(scope, "bind").unwrap();
  let bind = fulfill.get(scope, bind_key.into()).unwrap();
  let bind = v8::Local::<v8::Function>::try_from(bind).unwrap();
  let bind = yuzuko_core::op::root(scope, bind);
  let this_arg = v8::undefined(scope).into();
  let mapped = yuzuko_core::op::root(scope, mapped);
  let bound = bind
    .call(scope, fulfill.into(), &[this_arg, mapped])
    .unwrap();
  let fulfill = v8::Local::<v8::Function>::try_from(bound).unwrap();
  let fulfill = yuzuko_core::op::root(scope, fulfill);
  let reject = v8::Function::new(scope, cancel_reject).unwrap();
  let reject = yuzuko_core::op::root(scope, reject);
  let next = promise.then2(scope, fulfill, reject).unwrap();
  let value: v8::Local<v8::Value> = next.into();
  Js(v8::Global::new(scope, value))
}

fn iter_map_fulfill(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut ret: v8::ReturnValue,
) {
  let _ = scope;
  ret.set(args.get(0));
}

fn install_stream_extras(scope: &mut v8::PinScope) {
  capture_function_bind(scope);
  let _ = ReadableStreamAsyncIterator::template(scope);
  let templ = ReadableStream::template(scope);
  let ctor = templ.get_function(scope).unwrap();
  let key = v8::String::new(scope, "prototype").unwrap();
  let prototype = ctor.get(scope, key.into()).unwrap();
  let prototype = v8::Local::<v8::Object>::try_from(prototype).unwrap();
  let prototype = yuzuko_core::op::root(scope, prototype);
  let iterator = v8::Function::builder(stream_async_iterator)
    .length(0)
    .constructor_behavior(v8::ConstructorBehavior::Throw)
    .build(scope)
    .unwrap();
  let values_name = v8::String::new(scope, "values").unwrap();
  iterator.set_name(values_name);
  prototype.set(scope, values_name.into(), iterator.into());
  let symbol = v8::Symbol::get_async_iterator(scope);
  prototype.set(scope, symbol.into(), iterator.into());
  let pipe_to = v8::Function::builder(pipe_to_callback)
    .length(1)
    .constructor_behavior(v8::ConstructorBehavior::Throw)
    .build(scope)
    .unwrap();
  let name = v8::String::new(scope, "pipeTo").unwrap();
  pipe_to.set_name(name);
  prototype.set(scope, name.into(), pipe_to.into());
  let pipe_through = v8::Function::builder(pipe_through_callback)
    .length(1)
    .constructor_behavior(v8::ConstructorBehavior::Throw)
    .build(scope)
    .unwrap();
  let name = v8::String::new(scope, "pipeThrough").unwrap();
  pipe_through.set_name(name);
  prototype.set(scope, name.into(), pipe_through.into());
  install_readable_stream_from(scope, ctor);
  fix_async_iterator_prototype(scope);
}

const READABLE_STREAM_FROM: &str = r#"(function () {
  const reflectApply = Reflect.apply;
  const promiseThen = Promise.prototype.then;
  const promiseResolve = Promise.resolve;
  const promiseReject = Promise.reject;
  const PromiseCtor = Promise;
  const TypeErrorCtor = TypeError;
  const asyncIter = Symbol.asyncIterator;
  const syncIter = Symbol.iterator;

  function resolve(value) {
    return reflectApply(promiseResolve, PromiseCtor, [value]);
  }
  function reject(error) {
    return reflectApply(promiseReject, PromiseCtor, [error]);
  }
  function then(promise, onFulfilled) {
    return reflectApply(promiseThen, promise, [onFulfilled]);
  }

  function from(asyncIterable) {
    if (asyncIterable === null || (typeof asyncIterable !== "object" && typeof asyncIterable !== "function")) {
      throw new TypeErrorCtor("The object is not async iterable");
    }
    let method = asyncIterable[asyncIter];
    let iterator;
    if (method != null) {
      if (typeof method !== "function") {
        throw new TypeErrorCtor("The object is not async iterable");
      }
      iterator = reflectApply(method, asyncIterable, []);
      if (iterator === null || (typeof iterator !== "object" && typeof iterator !== "function")) {
        throw new TypeErrorCtor("The iterator is not an object");
      }
    } else {
      method = asyncIterable[syncIter];
      if (method == null || typeof method !== "function") {
        throw new TypeErrorCtor("The object is not iterable");
      }
      const sync = reflectApply(method, asyncIterable, []);
      if (sync === null || (typeof sync !== "object" && typeof sync !== "function")) {
        throw new TypeErrorCtor("The iterator is not an object");
      }
      iterator = {
        next() {
          let result;
          try {
            result = sync.next();
          } catch (error) {
            return reject(error);
          }
          if (result === null || (typeof result !== "object" && typeof result !== "function")) {
            return reject(new TypeErrorCtor("The iterator result is not an object"));
          }
          const done = !!result.done;
          return then(resolve(result.value), (value) => ({ value, done }));
        },
        return(reason) {
          const ret = sync.return;
          if (ret == null) {
            return resolve({ value: undefined, done: true });
          }
          if (typeof ret !== "function") {
            return reject(new TypeErrorCtor("The iterator return method is invalid"));
          }
          let result;
          try {
            result = reflectApply(ret, sync, [reason]);
          } catch (error) {
            return reject(error);
          }
          return then(resolve(result), (fulfilled) => {
            if (fulfilled === null || (typeof fulfilled !== "object" && typeof fulfilled !== "function")) {
              throw new TypeErrorCtor("The iterator result is not an object");
            }
            const done = !!fulfilled.done;
            return then(resolve(fulfilled.value), (value) => ({ value, done }));
          });
        }
      };
    }
    const ctor = this;
    return new ctor({
      async pull(controller) {
        const result = await iterator.next();
        if (result === null || (typeof result !== "object" && typeof result !== "function")) {
          throw new TypeErrorCtor("The iterator result is not an object");
        }
        if (result.done) {
          controller.close();
          return;
        }
        controller.enqueue(result.value);
      },
      cancel(reason) {
        const ret = iterator.return;
        if (ret == null) {
          return undefined;
        }
        if (typeof ret !== "function") {
          throw new TypeErrorCtor("The iterator return method is invalid");
        }
        return then(resolve(reflectApply(ret, iterator, [reason])), (fulfilled) => {
          if (fulfilled === null || (typeof fulfilled !== "object" && typeof fulfilled !== "function")) {
            throw new TypeErrorCtor("The iterator result is not an object");
          }
        });
      }
    }, { highWaterMark: 0 });
  }
  return from;
})()"#;

fn install_readable_stream_from(
  scope: &mut v8::PinScope,
  ctor: v8::Local<v8::Function>,
) {
  let source = v8::String::new(scope, READABLE_STREAM_FROM).unwrap();
  let function = v8::Script::compile(scope, source, None)
    .and_then(|script| script.run(scope))
    .and_then(|value| v8::Local::<v8::Function>::try_from(value).ok());
  let Some(function) = function else {
    return;
  };
  let key = v8::String::new(scope, "from").unwrap();
  ctor.set(scope, key.into(), function.into());
}

fn fix_async_iterator_prototype(scope: &mut v8::PinScope) {
  let templ = ReadableStreamAsyncIterator::template(scope);
  let ctor = templ.get_function(scope).unwrap();
  let proto_key = v8::String::new(scope, "prototype").unwrap();
  let Some(proto) = ctor.get(scope, proto_key.into()) else {
    return;
  };
  let Ok(proto) = v8::Local::<v8::Object>::try_from(proto) else {
    return;
  };
  if let Some(async_proto) = async_iterator_prototype(scope) {
    let _ = proto.set_prototype(scope, async_proto);
  }
  let constructor = v8::String::new(scope, "constructor").unwrap();
  let _ = proto.delete(scope, constructor.into());
  reconfigure_iterator_method(scope, proto, "next", 0);
  reconfigure_iterator_method(scope, proto, "return", 1);
}

fn async_iterator_prototype<'s>(
  scope: &mut v8::PinScope<'s, '_>,
) -> Option<v8::Local<'s, v8::Value>> {
  let source =
    v8::String::new(scope, "(async function* () {}).prototype").unwrap();
  let script = v8::Script::compile(scope, source, None)?;
  let generator = script.run(scope)?;
  let generator = v8::Local::<v8::Object>::try_from(generator).ok()?;
  let parent = generator.get_prototype(scope)?;
  let parent = v8::Local::<v8::Object>::try_from(parent).ok()?;
  parent.get_prototype(scope)
}

fn reconfigure_iterator_method(
  scope: &mut v8::PinScope,
  proto: v8::Local<v8::Object>,
  name: &str,
  length: i32,
) {
  let key = v8::String::new(scope, name).unwrap();
  let Some(value) = proto.get(scope, key.into()) else {
    return;
  };
  let Ok(function) = v8::Local::<v8::Function>::try_from(value) else {
    return;
  };
  function.set_name(key);
  let length_key = v8::String::new(scope, "length").unwrap();
  let length_value: v8::Local<v8::Value> =
    v8::Integer::new(scope, length).into();
  let mut length_desc =
    v8::PropertyDescriptor::new_from_value_writable(length_value, false);
  length_desc.set_enumerable(false);
  length_desc.set_configurable(true);
  let _ = function.define_property(scope, length_key.into(), &length_desc);
  let _ = proto.delete(scope, key.into());
  let mut descriptor =
    v8::PropertyDescriptor::new_from_value_writable(function.into(), true);
  descriptor.set_enumerable(true);
  descriptor.set_configurable(true);
  let _ = proto.define_property(scope, key.into(), &descriptor);
}

pub fn install(scope: &mut v8::PinScope) {
  ReadableStream::install(scope);
  install_stream_extras(scope);
  ReadableStreamDefaultReader::install(scope);
  ReadableStreamDefaultController::install(scope);
  byte_stream::install(scope);
  WritableStream::install(scope);
  WritableStreamDefaultWriter::install(scope);
  WritableStreamDefaultController::install(scope);
  TransformStreamDefaultController::install(scope);
  TransformStream::install(scope);
  CountQueuingStrategy::install(scope);
  ByteLengthQueuingStrategy::install(scope);
  define_strategy_size(scope, "CountQueuingStrategy", count_size_callback, 0);
  define_strategy_size(
    scope,
    "ByteLengthQueuingStrategy",
    byte_size_callback,
    1,
  );
  TextEncoderStream::install(scope);
  TextDecoderStream::install(scope);
  CompressionStream::install(scope);
  DecompressionStream::install(scope);
}
