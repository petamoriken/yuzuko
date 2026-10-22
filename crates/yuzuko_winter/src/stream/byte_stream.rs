//! Readable byte streams: `ReadableByteStreamController`, BYOB readers, and
//! byte-stream tee. Default streams stay in the parent module.

use std::cell::RefCell;
use std::collections::HashMap;
use std::collections::VecDeque;

use yuzuko_core::op::Js;
use yuzuko_core::op::OpError;
use yuzuko_macros::op;

use super::Phase;
use super::READERS;
use super::READS;
use super::Token;
use super::call_method;
use super::error_stream;
use super::error_value;
use super::finish_close;
use super::object_value;
use super::property;
use super::read_result;
use super::read_strategy;
use super::rejected;
use super::resolved;
use super::settle_reader_closed;
use super::source_methods;
use super::type_error;
use super::watch_promise;

thread_local! {
  static BYTES: RefCell<HashMap<u64, ByteCtrl>> = RefCell::new(HashMap::new());
  static TEES: RefCell<HashMap<u64, ByteTee>> = RefCell::new(HashMap::new());
}

struct QueueChunk {
  buffer: v8::Global<v8::ArrayBuffer>,
  offset: usize,
  length: usize,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ViewKind {
  Uint8,
  Uint8Clamped,
  Int8,
  Uint16,
  Int16,
  Uint32,
  Int32,
  Float16,
  Float32,
  Float64,
  BigUint64,
  BigInt64,
  DataView,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PullReader {
  None,
  Default,
  Byob,
}

struct PullInto {
  buffer: v8::Global<v8::ArrayBuffer>,
  buffer_byte_length: usize,
  byte_offset: usize,
  byte_length: usize,
  bytes_filled: usize,
  minimum_fill: usize,
  element_size: usize,
  view: ViewKind,
  reader: PullReader,
}

struct ByteCtrl {
  queue: VecDeque<QueueChunk>,
  queue_total: usize,
  pending: VecDeque<PullInto>,
  auto_allocate: Option<usize>,
  byob_generation: u64,
  byob_view: Option<v8::Global<v8::Value>>,
  byob_request: Option<v8::Global<v8::Object>>,
  tee: Option<(u64, u8)>,
}

struct ByteTee {
  reader: u64,
  branches: [u64; 2],
  again: [bool; 2],
  reading: bool,
  /// `Some(branch)` when the in-flight read uses that branch's BYOB view.
  read_branch: Option<u8>,
  canceled: [bool; 2],
  reasons: [Option<v8::Global<v8::Value>>; 2],
  cancel_resolver: v8::Global<v8::PromiseResolver>,
  cancel_promise: v8::Global<v8::Promise>,
  resolved: bool,
}

pub(super) fn clear() {
  BYTES.with(|slot| slot.borrow_mut().clear());
  TEES.with(|slot| slot.borrow_mut().clear());
}

pub(super) fn is_byte(id: u64) -> bool {
  BYTES.with(|slot| slot.borrow().contains_key(&id))
}

pub(super) fn create_push() -> u64 {
  let id = super::alloc();
  READS.with(|slot| {
    slot.borrow_mut().insert(
      id,
      super::ReadState {
        queue: VecDeque::new(),
        queued: 0.0,
        high_water: 0.0,
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
  BYTES.with(|slot| {
    slot.borrow_mut().insert(
      id,
      ByteCtrl {
        queue: VecDeque::new(),
        queue_total: 0,
        pending: VecDeque::new(),
        auto_allocate: None,
        byob_generation: 0,
        byob_view: None,
        byob_request: None,
        tee: None,
      },
    );
  });
  id
}

pub(super) fn push_bytes(
  scope: &mut v8::PinScope,
  id: u64,
  bytes: Vec<u8>,
) -> bool {
  if bytes.is_empty() {
    return phase_of(id) == Some(Phase::Readable);
  }
  let view = crate::bytes::uint8_array(scope, bytes);
  enqueue_view(scope, id, view.into(), false).is_ok()
}

pub(super) fn finish(scope: &mut v8::PinScope, id: u64) {
  let _ = controller_close(scope, id);
}

pub(super) fn fail(
  scope: &mut v8::PinScope,
  id: u64,
  error: v8::Local<v8::Value>,
) {
  let _ = error_controller(scope, id, error);
}

pub(super) fn take_queued_bytes(
  scope: &mut v8::PinScope,
  id: u64,
) -> Result<Vec<u8>, OpError> {
  let queued = BYTES.with(|slot| {
    let mut bytes = slot.borrow_mut();
    let Some(byte) = bytes.get_mut(&id) else {
      return Vec::new();
    };
    byte.queue_total = 0;
    byte.queue.drain(..).collect::<Vec<_>>()
  });
  let mut output = Vec::new();
  for chunk in queued {
    let buffer = v8::Local::new(scope, &chunk.buffer);
    output.extend(copy_out(buffer, chunk.offset, chunk.length)?);
  }
  Ok(output)
}

pub(super) fn construct(
  scope: &mut v8::PinScope,
  source: Option<v8::Local<v8::Value>>,
  strategy: Option<v8::Local<v8::Value>>,
) -> Result<super::ReadableStream, OpError> {
  if strategy_defines_size(scope, strategy)? {
    return Err(OpError::range(
      "A byte stream cannot use a strategy size function",
    ));
  }
  let (high_water, _) = read_strategy(scope, strategy, 0.0)?;
  let auto = auto_allocate(scope, source)?;
  let (start, pull, cancel) = source_methods(scope, source)?;
  let id = super::alloc();
  READS.with(|slot| {
    slot.borrow_mut().insert(
      id,
      super::ReadState {
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
        size_fn: None,
        native: None,
        paired: None,
      },
    );
  });
  BYTES.with(|slot| {
    slot.borrow_mut().insert(
      id,
      ByteCtrl {
        queue: VecDeque::new(),
        queue_total: 0,
        pending: VecDeque::new(),
        auto_allocate: auto,
        byob_generation: 0,
        byob_view: None,
        byob_request: None,
        tee: None,
      },
    );
  });
  let controller = unsafe {
    yuzuko_core::op::wrap_instance(
      scope,
      ReadableByteStreamController { stream: id },
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
  Ok(super::ReadableStream { id })
}

pub(super) fn on_started(scope: &mut v8::PinScope, id: u64) {
  call_pull_if_needed(scope, id);
}

pub(super) fn on_pull_settled(
  scope: &mut v8::PinScope,
  id: u64,
  failed: Option<v8::Local<v8::Value>>,
) {
  if let Some(error) = failed {
    let _ = error_controller(scope, id, error);
    return;
  }
  let again = READS.with(|slot| {
    let mut states = slot.borrow_mut();
    let state = states.get_mut(&id)?;
    state.pulling = false;
    let again = state.pull_again;
    state.pull_again = false;
    Some(again)
  });
  if again == Some(true) {
    call_pull_if_needed(scope, id);
  }
}

pub(super) fn desired_size(id: u64) -> Option<f64> {
  let phase =
    READS.with(|slot| slot.borrow().get(&id).map(|state| state.phase));
  match phase {
    Some(Phase::Errored) | None => None,
    Some(Phase::Closed) => Some(0.0),
    Some(Phase::Readable | Phase::Erroring) => {
      let queued = BYTES.with(|slot| {
        slot
          .borrow()
          .get(&id)
          .map(|byte| byte.queue_total)
          .unwrap_or(0)
      });
      let high = READS.with(|slot| {
        slot
          .borrow()
          .get(&id)
          .map(|state| state.high_water)
          .unwrap_or(0.0)
      });
      Some(high - queued as f64)
    }
  }
}

pub(super) fn default_read(
  scope: &mut v8::PinScope,
  reader: u64,
  stream: u64,
) -> Js {
  let released = READERS.with(|slot| {
    slot
      .borrow()
      .get(&reader)
      .is_some_and(|state| state.released)
  });
  if released {
    let error = type_error(scope, "The reader is released");
    let error = yuzuko_core::op::root(scope, error);
    return rejected(scope, error);
  }
  disturb(stream);
  let phase = phase_of(stream);
  match phase {
    Some(Phase::Closed) => {
      let result = read_result(scope, None, true);
      let result = yuzuko_core::op::root(scope, result);
      return resolved(scope, result);
    }
    Some(Phase::Errored) => {
      let error = stored_error(scope, stream);
      let error = yuzuko_core::op::root(scope, error);
      return rejected(scope, error);
    }
    _ => {}
  }
  if queue_total(stream) > 0 {
    return match fill_read_from_queue(scope, stream) {
      Ok(view) => {
        let view = yuzuko_core::op::root(scope, view);
        let result = read_result(scope, Some(view), false);
        let result = yuzuko_core::op::root(scope, result);
        resolved(scope, result)
      }
      Err(error) => {
        let thrown = error_value(scope, &error);
        let thrown = yuzuko_core::op::root(scope, thrown);
        rejected(scope, thrown)
      }
    };
  }
  if let Some(size) = auto_allocate_of(stream) {
    match alloc_buffer(scope, size) {
      Ok(buffer) => {
        let buffer = yuzuko_core::op::root(scope, buffer);
        push_pull(
          scope,
          stream,
          PullInto {
            buffer: v8::Global::new(scope, buffer),
            buffer_byte_length: size,
            byte_offset: 0,
            byte_length: size,
            bytes_filled: 0,
            minimum_fill: 1,
            element_size: 1,
            view: ViewKind::Uint8,
            reader: PullReader::Default,
          },
        );
      }
      Err(error) => {
        let thrown = error_value(scope, &error);
        let thrown = yuzuko_core::op::root(scope, thrown);
        return rejected(scope, thrown);
      }
    }
  }
  let resolver = v8::PromiseResolver::new(scope).unwrap();
  let promise = resolver.get_promise(scope);
  READERS.with(|slot| {
    if let Some(state) = slot.borrow_mut().get_mut(&reader) {
      state.requests.push_back(v8::Global::new(scope, resolver));
    }
  });
  call_pull_if_needed(scope, stream);
  let value: v8::Local<v8::Value> = promise.into();
  Js(v8::Global::new(scope, value))
}

pub(super) fn acquire_byob(
  scope: &mut v8::PinScope,
  stream: u64,
) -> Result<ReadableStreamBYOBReader, OpError> {
  if !is_byte(stream) {
    return Err(OpError::type_(
      "Cannot get a BYOB reader from a non-byte stream",
    ));
  }
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
  let id = super::alloc();
  let phase = phase_of(stream);
  READERS.with(|slot| {
    slot.borrow_mut().insert(
      id,
      super::ReaderState {
        stream,
        released: false,
        byob: true,
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
  if phase == Some(Phase::Closed) {
    settle_reader_closed(scope, id, true, None);
  } else if phase == Some(Phase::Errored) {
    let error = READS.with(|slot| {
      slot
        .borrow()
        .get(&stream)
        .and_then(|state| state.error.clone())
    });
    settle_reader_closed(scope, id, false, error);
  }
  Ok(ReadableStreamBYOBReader { id })
}

pub(super) fn release_steps(scope: &mut v8::PinScope, stream: u64) {
  if !is_byte(stream) {
    return;
  }
  BYTES.with(|slot| {
    let mut bytes = slot.borrow_mut();
    let Some(byte) = bytes.get_mut(&stream) else {
      return;
    };
    if byte.pending.is_empty() {
      return;
    }
    let mut first = byte.pending.pop_front().unwrap();
    first.reader = PullReader::None;
    byte.pending.clear();
    byte.pending.push_back(first);
    let _ = scope;
  });
}

pub(super) fn prepare_cancel(stream: u64) {
  if !is_byte(stream) {
    return;
  }
  BYTES.with(|slot| {
    let mut bytes = slot.borrow_mut();
    let Some(byte) = bytes.get_mut(&stream) else {
      return;
    };
    byte.byob_generation = byte.byob_generation.wrapping_add(1);
    byte.byob_view = None;
    byte.byob_request = None;
    byte.pending.clear();
    byte.queue.clear();
    byte.queue_total = 0;
  });
}

pub(super) fn finish_cancel_reads(scope: &mut v8::PinScope, stream: u64) {
  let reader = READS
    .with(|slot| slot.borrow().get(&stream).and_then(|state| state.reader));
  let Some(reader) = reader else {
    return;
  };
  let byob = READERS
    .with(|slot| slot.borrow().get(&reader).is_some_and(|state| state.byob));
  if !byob {
    return;
  }
  let requests = READERS.with(|slot| {
    slot
      .borrow_mut()
      .get_mut(&reader)
      .map(|state| std::mem::take(&mut state.into_requests))
  });
  let Some(requests) = requests else {
    return;
  };
  for request in requests {
    let resolver = v8::Local::new(scope, &request);
    let resolver = yuzuko_core::op::root(scope, resolver);
    let result = read_result(scope, None, true);
    super::fulfill_resolver(scope, resolver, result);
  }
}

pub(super) fn reject_into(
  scope: &mut v8::PinScope,
  reader: u64,
  error: v8::Local<v8::Value>,
) {
  let requests = READERS.with(|slot| {
    slot
      .borrow_mut()
      .get_mut(&reader)
      .map(|state| std::mem::take(&mut state.into_requests))
  });
  let Some(requests) = requests else {
    return;
  };
  for request in requests {
    let resolver = v8::Local::new(scope, &request);
    let error = yuzuko_core::op::root(scope, error);
    resolver.reject(scope, error);
  }
}

pub(super) fn tee(
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
  let reader = super::acquire_reader(scope, id)?;
  let tee = super::alloc();
  let resolver = v8::PromiseResolver::new(scope).unwrap();
  let promise = resolver.get_promise(scope);
  let cancel_a = tee_cancel_function(scope, tee, 0);
  let branch_a = empty_branch(scope, tee, 0, cancel_a)?;
  let cancel_b = tee_cancel_function(scope, tee, 1);
  let branch_b = empty_branch(scope, tee, 1, cancel_b)?;
  let id_a = super::readable_object_id(scope, &branch_a)?;
  let id_b = super::readable_object_id(scope, &branch_b)?;
  TEES.with(|slot| {
    slot.borrow_mut().insert(
      tee,
      ByteTee {
        reader: reader.id,
        branches: [id_a, id_b],
        again: [false, false],
        reading: false,
        read_branch: None,
        canceled: [false, false],
        reasons: [None, None],
        cancel_resolver: v8::Global::new(scope, resolver),
        cancel_promise: v8::Global::new(scope, promise),
        resolved: false,
      },
    );
  });
  watch_reader_closed(scope, tee, reader.id);
  Ok((branch_a, branch_b))
}

fn empty_branch(
  scope: &mut v8::PinScope,
  tee: u64,
  branch: u8,
  cancel: v8::Local<v8::Function>,
) -> Result<v8::Global<v8::Object>, OpError> {
  let id = super::alloc();
  READS.with(|slot| {
    slot.borrow_mut().insert(
      id,
      super::ReadState {
        queue: VecDeque::new(),
        queued: 0.0,
        high_water: 0.0,
        phase: Phase::Readable,
        error: None,
        reader: None,
        close_requested: false,
        disturbed: false,
        pulling: false,
        pull_again: false,
        started: false,
        pull_fn: None,
        cancel_fn: Some(v8::Global::new(scope, cancel)),
        size_fn: None,
        native: None,
        paired: None,
      },
    );
  });
  BYTES.with(|slot| {
    slot.borrow_mut().insert(
      id,
      ByteCtrl {
        queue: VecDeque::new(),
        queue_total: 0,
        pending: VecDeque::new(),
        auto_allocate: None,
        byob_generation: 0,
        byob_view: None,
        byob_request: None,
        tee: Some((tee, branch)),
      },
    );
  });
  let value = v8::undefined(scope).into();
  let _ = watch_promise(scope, value, Token::StartRead(id));
  let object = unsafe {
    yuzuko_core::op::wrap_instance(scope, super::ReadableStream { id })
  };
  Ok(v8::Global::new(scope, object))
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

fn strategy_defines_size(
  scope: &mut v8::PinScope,
  strategy: Option<v8::Local<v8::Value>>,
) -> Result<bool, OpError> {
  let Some(strategy) = strategy.filter(|value| !value.is_null_or_undefined())
  else {
    return Ok(false);
  };
  let object = object_value(scope, strategy)?;
  match property(scope, object, "size")? {
    Some(value) => Ok(!value.is_undefined()),
    None => Ok(false),
  }
}

fn auto_allocate(
  scope: &mut v8::PinScope,
  source: Option<v8::Local<v8::Value>>,
) -> Result<Option<usize>, OpError> {
  let Some(source) = source else {
    return Ok(None);
  };
  let object = object_value(scope, source)?;
  let Some(value) = property(scope, object, "autoAllocateChunkSize")? else {
    return Ok(None);
  };
  if value.is_undefined() {
    return Ok(None);
  }
  let number = value
    .number_value(scope)
    .ok_or_else(|| OpError::type_("autoAllocateChunkSize is not a number"))?;
  if !number.is_finite() || number < 0.0 || number.fract() != 0.0 {
    return Err(OpError::type_("autoAllocateChunkSize is invalid"));
  }
  if number == 0.0 {
    return Err(OpError::type_(
      "autoAllocateChunkSize must be greater than 0",
    ));
  }
  Ok(Some(number as usize))
}

fn auto_allocate_of(stream: u64) -> Option<usize> {
  BYTES.with(|slot| {
    slot
      .borrow()
      .get(&stream)
      .and_then(|byte| byte.auto_allocate)
  })
}

fn phase_of(stream: u64) -> Option<Phase> {
  READS.with(|slot| slot.borrow().get(&stream).map(|state| state.phase))
}

fn disturb(stream: u64) {
  READS.with(|slot| {
    if let Some(state) = slot.borrow_mut().get_mut(&stream) {
      state.disturbed = true;
    }
  });
}

fn stored_error<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  stream: u64,
) -> v8::Local<'s, v8::Value> {
  let error = READS.with(|slot| {
    slot
      .borrow()
      .get(&stream)
      .and_then(|state| state.error.clone())
  });
  error
    .map(|error| v8::Local::new(scope, &error))
    .unwrap_or_else(|| type_error(scope, "The stream is errored"))
}

fn queue_total(stream: u64) -> usize {
  BYTES.with(|slot| {
    slot
      .borrow()
      .get(&stream)
      .map(|byte| byte.queue_total)
      .unwrap_or(0)
  })
}

fn close_requested(stream: u64) -> bool {
  READS.with(|slot| {
    slot
      .borrow()
      .get(&stream)
      .is_some_and(|state| state.close_requested)
  })
}

fn alloc_buffer<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  length: usize,
) -> Result<v8::Local<'s, v8::ArrayBuffer>, OpError> {
  Ok(crate::bytes::array_buffer(scope, vec![0u8; length]))
}

fn push_pull(scope: &mut v8::PinScope, stream: u64, pull: PullInto) {
  let _ = scope;
  BYTES.with(|slot| {
    if let Some(byte) = slot.borrow_mut().get_mut(&stream) {
      byte.pending.push_back(pull);
    }
  });
}

fn call_pull_if_needed(scope: &mut v8::PinScope, id: u64) {
  if !should_pull(id) {
    return;
  }
  let pulling = READS.with(|slot| {
    let mut states = slot.borrow_mut();
    let state = states.get_mut(&id)?;
    if state.pulling {
      state.pull_again = true;
      return Some(true);
    }
    state.pulling = true;
    Some(false)
  });
  if pulling != Some(false) {
    return;
  }
  let tee = BYTES.with(|slot| slot.borrow().get(&id).and_then(|byte| byte.tee));
  if let Some((tee, branch)) = tee {
    tee_pull(scope, tee, branch);
    let value = v8::undefined(scope).into();
    let _ = watch_promise(scope, value, Token::Pull(id));
    return;
  }
  let pull = READS.with(|slot| {
    slot
      .borrow()
      .get(&id)
      .and_then(|state| state.pull_fn.clone())
  });
  let Some(pull) = pull else {
    // A push byte stream has no underlying pull. Leaving `pulling` set
    // would schedule another pull on every microtask while a read waits.
    READS.with(|slot| {
      if let Some(state) = slot.borrow_mut().get_mut(&id) {
        state.pulling = false;
        state.pull_again = false;
      }
    });
    return;
  };
  let controller = unsafe {
    yuzuko_core::op::wrap_instance(
      scope,
      ReadableByteStreamController { stream: id },
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
      let _ = error_controller(scope, id, thrown);
    }
  }
}

fn should_pull(id: u64) -> bool {
  let readable = READS.with(|slot| {
    slot.borrow().get(&id).is_some_and(|state| {
      state.phase == Phase::Readable && state.started && !state.close_requested
    })
  });
  if !readable {
    return false;
  }
  let (reader, requests, into_requests) = reader_counts(id);
  if reader && requests > 0 {
    return true;
  }
  if into_requests > 0 {
    return true;
  }
  desired_size(id).is_some_and(|size| size > 0.0)
}

fn reader_counts(id: u64) -> (bool, usize, usize) {
  let reader =
    READS.with(|slot| slot.borrow().get(&id).and_then(|state| state.reader));
  let Some(reader) = reader else {
    return (false, 0, 0);
  };
  READERS
    .with(|slot| {
      slot.borrow().get(&reader).map(|state| {
        (!state.byob, state.requests.len(), state.into_requests.len())
      })
    })
    .unwrap_or((false, 0, 0))
}

fn error_controller(
  scope: &mut v8::PinScope,
  stream: u64,
  error: v8::Local<v8::Value>,
) -> Result<(), OpError> {
  if phase_of(stream) != Some(Phase::Readable) {
    return Ok(());
  }
  BYTES.with(|slot| {
    if let Some(byte) = slot.borrow_mut().get_mut(&stream) {
      byte.byob_generation = byte.byob_generation.wrapping_add(1);
      byte.byob_view = None;
      byte.byob_request = None;
      byte.pending.clear();
      byte.queue.clear();
      byte.queue_total = 0;
    }
  });
  READS.with(|slot| {
    if let Some(state) = slot.borrow_mut().get_mut(&stream) {
      state.pull_fn = None;
      state.cancel_fn = None;
    }
  });
  error_stream(scope, stream, error)
}

fn controller_close(
  scope: &mut v8::PinScope,
  stream: u64,
) -> Result<(), OpError> {
  if close_requested(stream) {
    return Err(OpError::type_("The stream is already closed"));
  }
  if phase_of(stream) != Some(Phase::Readable) {
    return Err(OpError::type_("The stream is not readable"));
  }
  close_readable(scope, stream)
}

fn close_readable(
  scope: &mut v8::PinScope,
  stream: u64,
) -> Result<(), OpError> {
  if close_requested(stream) || phase_of(stream) != Some(Phase::Readable) {
    return Ok(());
  }
  if queue_total(stream) > 0 {
    READS.with(|slot| {
      if let Some(state) = slot.borrow_mut().get_mut(&stream) {
        state.close_requested = true;
      }
    });
    return Ok(());
  }
  let misaligned = BYTES.with(|slot| {
    slot.borrow().get(&stream).is_some_and(|byte| {
      byte
        .pending
        .front()
        .is_some_and(|pull| pull.bytes_filled % pull.element_size != 0)
    })
  });
  if misaligned {
    let error = type_error(
      scope,
      "Insufficient bytes to fill elements in the given buffer",
    );
    let error = yuzuko_core::op::root(scope, error);
    let _ = error_controller(scope, stream, error);
    return Err(OpError::thrown(scope, error));
  }
  READS.with(|slot| {
    if let Some(state) = slot.borrow_mut().get_mut(&stream) {
      state.pull_fn = None;
      state.cancel_fn = None;
    }
  });
  finish_close(scope, stream);
  Ok(())
}

fn enqueue_view(
  scope: &mut v8::PinScope,
  stream: u64,
  chunk: v8::Local<v8::Value>,
  strict: bool,
) -> Result<(), OpError> {
  if close_requested(stream) || phase_of(stream) != Some(Phase::Readable) {
    if strict {
      return Err(OpError::type_("The stream is not readable"));
    }
    return Ok(());
  }
  let view = v8::Local::<v8::ArrayBufferView>::try_from(chunk)
    .map_err(|_| OpError::type_("The chunk is not an ArrayBuffer view"))?;
  if view.byte_length() == 0 {
    return Err(OpError::type_("chunk must have non-zero byteLength"));
  }
  let Some(buffer) = view.buffer(scope) else {
    return Err(OpError::type_("The chunk buffer is missing"));
  };
  if buffer.byte_length() == 0 || buffer.was_detached() {
    return Err(OpError::type_(
      "chunk's buffer is detached and so cannot be enqueued",
    ));
  }
  let offset = view.byte_offset();
  let length = view.byte_length();
  let transferred = transfer_buffer(scope, buffer)?;
  let transferred = yuzuko_core::op::root(scope, transferred);
  transfer_pending_head(scope, stream)?;
  let (default_reader, _, _) = reader_counts(stream);
  if default_reader {
    while queue_total(stream) > 0 && reader_counts(stream).1 > 0 {
      let filled = fill_read_from_queue(scope, stream)?;
      fulfill_default(scope, stream, filled, false);
    }
    if reader_counts(stream).1 == 0 {
      enqueue_chunk(scope, stream, transferred, offset, length);
    } else {
      shift_pending_if_default(stream);
      let view = uint8_view(scope, transferred, offset, length)?;
      fulfill_default(scope, stream, view, false);
    }
  } else if has_byob_reader(stream) {
    enqueue_chunk(scope, stream, transferred, offset, length);
    let filled = process_pulls(scope, stream)?;
    for pull in filled {
      commit_pull(scope, stream, pull)?;
    }
  } else {
    enqueue_chunk(scope, stream, transferred, offset, length);
  }
  call_pull_if_needed(scope, stream);
  Ok(())
}

fn transfer_pending_head(
  scope: &mut v8::PinScope,
  stream: u64,
) -> Result<(), OpError> {
  let buffer = BYTES.with(|slot| {
    slot
      .borrow()
      .get(&stream)
      .and_then(|byte| byte.pending.front().map(|pull| pull.buffer.clone()))
  });
  let Some(buffer) = buffer else {
    return Ok(());
  };
  let buffer = v8::Local::new(scope, &buffer);
  if buffer.was_detached() || buffer.byte_length() == 0 {
    return Err(OpError::type_(
      "The BYOB request's buffer has been detached and so cannot be filled with an enqueued chunk",
    ));
  }
  invalidate_byob(stream);
  let transferred = transfer_buffer(scope, buffer)?;
  let transferred = yuzuko_core::op::root(scope, transferred);
  let reader = BYTES.with(|slot| {
    let mut bytes = slot.borrow_mut();
    let byte = bytes.get_mut(&stream)?;
    let pull = byte.pending.front_mut()?;
    pull.buffer_byte_length = transferred.byte_length();
    pull.buffer = v8::Global::new(scope, transferred);
    Some(pull.reader)
  });
  if reader == Some(PullReader::None) {
    enqueue_detached_head(scope, stream)?;
  }
  Ok(())
}

fn has_byob_reader(stream: u64) -> bool {
  let reader = READS
    .with(|slot| slot.borrow().get(&stream).and_then(|state| state.reader));
  let Some(reader) = reader else {
    return false;
  };
  READERS
    .with(|slot| slot.borrow().get(&reader).is_some_and(|state| state.byob))
}

fn shift_pending_if_default(stream: u64) {
  BYTES.with(|slot| {
    if let Some(byte) = slot.borrow_mut().get_mut(&stream)
      && byte
        .pending
        .front()
        .is_some_and(|pull| pull.reader == PullReader::Default)
    {
      byte.pending.pop_front();
    }
  });
}

fn enqueue_detached_head(
  scope: &mut v8::PinScope,
  stream: u64,
) -> Result<(), OpError> {
  let head = BYTES.with(|slot| {
    slot
      .borrow_mut()
      .get_mut(&stream)
      .and_then(|byte| byte.pending.pop_front())
  });
  let Some(head) = head else {
    return Ok(());
  };
  if head.bytes_filled > 0 {
    enqueue_clone(
      scope,
      stream,
      &head.buffer,
      head.byte_offset,
      head.bytes_filled,
    )?;
  }
  Ok(())
}

fn enqueue_chunk(
  scope: &mut v8::PinScope,
  stream: u64,
  buffer: v8::Local<v8::ArrayBuffer>,
  offset: usize,
  length: usize,
) {
  BYTES.with(|slot| {
    if let Some(byte) = slot.borrow_mut().get_mut(&stream) {
      byte.queue_total += length;
      byte.queue.push_back(QueueChunk {
        buffer: v8::Global::new(scope, buffer),
        offset,
        length,
      });
    }
  });
}

fn enqueue_clone(
  scope: &mut v8::PinScope,
  stream: u64,
  buffer: &v8::Global<v8::ArrayBuffer>,
  offset: usize,
  length: usize,
) -> Result<(), OpError> {
  let buffer = v8::Local::new(scope, buffer);
  let bytes = copy_out(buffer, offset, length)?;
  let cloned = crate::bytes::array_buffer(scope, bytes);
  enqueue_chunk(scope, stream, cloned, 0, length);
  Ok(())
}

fn fill_read_from_queue<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  stream: u64,
) -> Result<v8::Local<'s, v8::Value>, OpError> {
  let entry = BYTES.with(|slot| {
    let mut bytes = slot.borrow_mut();
    let byte = bytes.get_mut(&stream)?;
    let entry = byte.queue.pop_front()?;
    byte.queue_total = byte.queue_total.saturating_sub(entry.length);
    Some(entry)
  });
  let Some(entry) = entry else {
    return Err(OpError::type_("The byte queue is empty"));
  };
  handle_drain(scope, stream);
  let buffer = v8::Local::new(scope, &entry.buffer);
  let view = uint8_view(scope, buffer, entry.offset, entry.length)?;
  Ok(view)
}

fn handle_drain(scope: &mut v8::PinScope, stream: u64) {
  if phase_of(stream) != Some(Phase::Readable) {
    return;
  }
  if queue_total(stream) == 0 && close_requested(stream) {
    READS.with(|slot| {
      if let Some(state) = slot.borrow_mut().get_mut(&stream) {
        state.pull_fn = None;
        state.cancel_fn = None;
      }
    });
    finish_close(scope, stream);
  } else {
    call_pull_if_needed(scope, stream);
  }
}

fn process_pulls(
  scope: &mut v8::PinScope,
  stream: u64,
) -> Result<Vec<PullInto>, OpError> {
  let mut filled = Vec::new();
  loop {
    if queue_total(stream) == 0 {
      break;
    }
    let Some(mut pull) = head_pull(stream) else {
      break;
    };
    if pull.reader == PullReader::None {
      break;
    }
    if fill_from_queue(scope, stream, &mut pull)? {
      shift_head(stream);
      filled.push(pull);
    } else {
      store_head(stream, pull);
      break;
    }
  }
  Ok(filled)
}

fn head_pull(stream: u64) -> Option<PullInto> {
  BYTES.with(|slot| {
    slot
      .borrow()
      .get(&stream)
      .and_then(|byte| byte.pending.front().cloned())
  })
}

fn store_head(stream: u64, pull: PullInto) {
  BYTES.with(|slot| {
    if let Some(byte) = slot.borrow_mut().get_mut(&stream)
      && let Some(head) = byte.pending.front_mut()
    {
      *head = pull;
    }
  });
}

fn shift_head(stream: u64) {
  BYTES.with(|slot| {
    if let Some(byte) = slot.borrow_mut().get_mut(&stream) {
      byte.pending.pop_front();
    }
  });
}

fn fill_from_queue(
  scope: &mut v8::PinScope,
  stream: u64,
  pull: &mut PullInto,
) -> Result<bool, OpError> {
  let max_copy = queue_total(stream).min(pull.byte_length - pull.bytes_filled);
  let max_filled = pull.bytes_filled + max_copy;
  let remainder = max_filled % pull.element_size;
  let aligned = max_filled - remainder;
  let mut ready = false;
  let mut remaining = max_copy;
  if aligned >= pull.minimum_fill {
    remaining = aligned - pull.bytes_filled;
    ready = true;
  }
  while remaining > 0 {
    let copied = copy_queue_head(scope, stream, pull, remaining)?;
    pull.bytes_filled += copied;
    remaining -= copied;
  }
  Ok(ready)
}

fn copy_queue_head(
  scope: &mut v8::PinScope,
  stream: u64,
  pull: &PullInto,
  remaining: usize,
) -> Result<usize, OpError> {
  let head = BYTES.with(|slot| {
    slot.borrow().get(&stream).and_then(|byte| {
      byte
        .queue
        .front()
        .map(|chunk| (chunk.buffer.clone(), chunk.offset, chunk.length))
    })
  });
  let Some((buffer, offset, length)) = head else {
    return Ok(0);
  };
  let bytes = remaining.min(length);
  let dest = v8::Local::new(scope, &pull.buffer);
  let src = v8::Local::new(scope, &buffer);
  copy_block(
    dest,
    pull.byte_offset + pull.bytes_filled,
    src,
    offset,
    bytes,
  )?;
  BYTES.with(|slot| {
    if let Some(byte) = slot.borrow_mut().get_mut(&stream) {
      if let Some(head) = byte.queue.front_mut() {
        if head.length == bytes {
          byte.queue.pop_front();
        } else {
          head.offset += bytes;
          head.length -= bytes;
        }
      }
      byte.queue_total = byte.queue_total.saturating_sub(bytes);
    }
  });
  Ok(bytes)
}

fn commit_pull(
  scope: &mut v8::PinScope,
  stream: u64,
  pull: PullInto,
) -> Result<(), OpError> {
  let done = phase_of(stream) == Some(Phase::Closed);
  let view = convert_pull(scope, &pull)?;
  match pull.reader {
    PullReader::Default => fulfill_default(scope, stream, view, done),
    PullReader::Byob => fulfill_into(scope, stream, view, done),
    PullReader::None => {}
  }
  Ok(())
}

fn convert_pull<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  pull: &PullInto,
) -> Result<v8::Local<'s, v8::Value>, OpError> {
  let buffer = v8::Local::new(scope, &pull.buffer);
  let transferred = transfer_buffer(scope, buffer)?;
  let length = pull.bytes_filled / pull.element_size;
  make_view(scope, pull.view, transferred, pull.byte_offset, length)
}

fn fulfill_default(
  scope: &mut v8::PinScope,
  stream: u64,
  view: v8::Local<v8::Value>,
  done: bool,
) {
  let request = pop_request(stream);
  let Some(request) = request else {
    return;
  };
  let resolver = v8::Local::new(scope, &request);
  let resolver = yuzuko_core::op::root(scope, resolver);
  let view = yuzuko_core::op::root(scope, view);
  let value = if done { None } else { Some(view) };
  let result = read_result(scope, value, done);
  super::fulfill_resolver(scope, resolver, result);
}

fn fulfill_into(
  scope: &mut v8::PinScope,
  stream: u64,
  view: v8::Local<v8::Value>,
  done: bool,
) {
  let request = pop_into(stream);
  let Some(request) = request else {
    return;
  };
  let resolver = v8::Local::new(scope, &request);
  let resolver = yuzuko_core::op::root(scope, resolver);
  let view = yuzuko_core::op::root(scope, view);
  let result = read_result(scope, Some(view), done);
  super::fulfill_resolver(scope, resolver, result);
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

fn pop_into(stream: u64) -> Option<v8::Global<v8::PromiseResolver>> {
  let reader = READS
    .with(|slot| slot.borrow().get(&stream).and_then(|state| state.reader))?;
  READERS.with(|slot| {
    slot
      .borrow_mut()
      .get_mut(&reader)
      .and_then(|state| state.into_requests.pop_front())
  })
}

fn invalidate_byob(stream: u64) {
  BYTES.with(|slot| {
    if let Some(byte) = slot.borrow_mut().get_mut(&stream) {
      byte.byob_generation = byte.byob_generation.wrapping_add(1);
      byte.byob_view = None;
      byte.byob_request = None;
    }
  });
}

fn byob_view_object(
  scope: &mut v8::PinScope,
  stream: u64,
) -> Option<v8::Global<v8::Object>> {
  let cached = BYTES.with(|slot| {
    slot
      .borrow()
      .get(&stream)
      .and_then(|byte| byte.byob_request.clone())
  });
  if cached.is_some() {
    return cached;
  }
  let info = BYTES.with(|slot| {
    slot.borrow().get(&stream).and_then(|byte| {
      let pull = byte.pending.front()?;
      Some((
        pull.buffer.clone(),
        pull.byte_offset + pull.bytes_filled,
        pull.byte_length - pull.bytes_filled,
        byte.byob_generation,
      ))
    })
  });
  let (buffer, offset, length, generation) = info?;
  let buffer = v8::Local::new(scope, &buffer);
  let view = uint8_view(scope, buffer, offset, length).ok()?;
  let object = unsafe {
    yuzuko_core::op::wrap_instance(
      scope,
      ReadableStreamBYOBRequest { stream, generation },
    )
  };
  let request = v8::Global::new(scope, object);
  BYTES.with(|slot| {
    if let Some(byte) = slot.borrow_mut().get_mut(&stream)
      && byte.byob_generation == generation
      && byte.byob_request.is_none()
    {
      byte.byob_view = Some(v8::Global::new(scope, view));
      byte.byob_request = Some(request.clone());
    }
  });
  Some(request)
}

fn respond(
  scope: &mut v8::PinScope,
  stream: u64,
  bytes_written: usize,
) -> Result<(), OpError> {
  let state = phase_of(stream);
  let descriptor = BYTES.with(|slot| {
    slot
      .borrow()
      .get(&stream)
      .and_then(|byte| byte.pending.front().cloned())
  });
  let Some(mut first) = descriptor else {
    return Err(OpError::type_("There is no pending BYOB request"));
  };
  if state == Some(Phase::Closed) {
    if bytes_written != 0 {
      return Err(OpError::type_(
        "bytesWritten must be 0 when calling respond() on a closed stream",
      ));
    }
  } else if state == Some(Phase::Readable) {
    if bytes_written == 0 {
      return Err(OpError::type_(
        "bytesWritten must be greater than 0 when calling respond() on a readable stream",
      ));
    }
    if first.bytes_filled + bytes_written > first.byte_length {
      return Err(OpError::range("bytesWritten out of range"));
    }
  } else {
    return Err(OpError::type_("The stream is not readable"));
  }
  let buffer = v8::Local::new(scope, &first.buffer);
  let transferred = transfer_buffer(scope, buffer)?;
  let transferred = yuzuko_core::op::root(scope, transferred);
  first.buffer_byte_length = transferred.byte_length();
  first.buffer = v8::Global::new(scope, transferred);
  store_head(stream, first.clone());
  invalidate_byob(stream);
  respond_internal(scope, stream, bytes_written)?;
  call_pull_if_needed(scope, stream);
  Ok(())
}

fn respond_internal(
  scope: &mut v8::PinScope,
  stream: u64,
  bytes_written: usize,
) -> Result<(), OpError> {
  let mut first = BYTES
    .with(|slot| {
      slot
        .borrow()
        .get(&stream)
        .and_then(|byte| byte.pending.front().cloned())
    })
    .ok_or_else(|| OpError::type_("There is no pending BYOB request"))?;
  if phase_of(stream) == Some(Phase::Closed) {
    respond_closed(scope, stream, &first)?;
  } else {
    respond_readable(scope, stream, bytes_written, &mut first)?;
  }
  Ok(())
}

fn respond_closed(
  scope: &mut v8::PinScope,
  stream: u64,
  first: &PullInto,
) -> Result<(), OpError> {
  if first.reader == PullReader::None {
    shift_head(stream);
  }
  let count = reader_counts(stream).2;
  let mut filled = Vec::new();
  for _ in 0..count {
    if let Some(pull) = BYTES.with(|slot| {
      slot
        .borrow_mut()
        .get_mut(&stream)
        .and_then(|byte| byte.pending.pop_front())
    }) {
      filled.push(pull);
    }
  }
  for pull in filled {
    commit_pull(scope, stream, pull)?;
  }
  Ok(())
}

fn respond_readable(
  scope: &mut v8::PinScope,
  stream: u64,
  bytes_written: usize,
  pull: &mut PullInto,
) -> Result<(), OpError> {
  pull.bytes_filled += bytes_written;
  store_head(stream, pull.clone());
  if pull.reader == PullReader::None {
    enqueue_detached_head(scope, stream)?;
    let filled = process_pulls(scope, stream)?;
    for item in filled {
      commit_pull(scope, stream, item)?;
    }
    return Ok(());
  }
  if pull.bytes_filled < pull.minimum_fill {
    return Ok(());
  }
  shift_head(stream);
  let remainder = pull.bytes_filled % pull.element_size;
  if remainder > 0 {
    let end = pull.byte_offset + pull.bytes_filled;
    enqueue_clone(scope, stream, &pull.buffer, end - remainder, remainder)?;
    pull.bytes_filled -= remainder;
  }
  let filled = process_pulls(scope, stream)?;
  commit_pull(scope, stream, pull.clone())?;
  for item in filled {
    commit_pull(scope, stream, item)?;
  }
  Ok(())
}

fn respond_with_view(
  scope: &mut v8::PinScope,
  stream: u64,
  view: v8::Local<v8::Value>,
) -> Result<(), OpError> {
  let view = v8::Local::<v8::ArrayBufferView>::try_from(view)
    .map_err(|_| OpError::type_("The view is not an ArrayBuffer view"))?;
  let Some(buffer) = view.buffer(scope) else {
    return Err(OpError::type_("The view buffer is missing"));
  };
  if buffer.was_detached() {
    return Err(OpError::type_("The view's buffer has been detached"));
  }
  let first = BYTES
    .with(|slot| {
      slot
        .borrow()
        .get(&stream)
        .and_then(|byte| byte.pending.front().cloned())
    })
    .ok_or_else(|| OpError::type_("There is no pending BYOB request"))?;
  let state = phase_of(stream);
  if state == Some(Phase::Closed) {
    if view.byte_length() != 0 {
      return Err(OpError::type_(
        "The view's length must be 0 when calling respondWithNewView() on a closed stream",
      ));
    }
  } else if view.byte_length() == 0 {
    return Err(OpError::type_(
      "The view's length must be greater than 0 when calling respondWithNewView() on a readable stream",
    ));
  }
  if first.byte_offset + first.bytes_filled != view.byte_offset() {
    return Err(OpError::range(
      "The region specified by view does not match byobRequest",
    ));
  }
  if first.buffer_byte_length != buffer.byte_length() {
    return Err(OpError::range(
      "The buffer of view has different capacity than byobRequest",
    ));
  }
  if first.bytes_filled + view.byte_length() > first.byte_length {
    return Err(OpError::range(
      "The region specified by view is larger than byobRequest",
    ));
  }
  let written = view.byte_length();
  let transferred = transfer_buffer(scope, buffer)?;
  BYTES.with(|slot| {
    if let Some(byte) = slot.borrow_mut().get_mut(&stream)
      && let Some(head) = byte.pending.front_mut()
    {
      head.buffer_byte_length = transferred.byte_length();
      head.buffer = v8::Global::new(scope, transferred);
    }
  });
  invalidate_byob(stream);
  respond_internal(scope, stream, written)?;
  call_pull_if_needed(scope, stream);
  Ok(())
}

fn pull_into<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  stream: u64,
  view: v8::Local<'s, v8::Value>,
  min: usize,
) -> Js {
  let view = yuzuko_core::op::root(scope, view);
  let view_info = match describe_view(scope, view) {
    Ok(info) => info,
    Err(error) => {
      let thrown = error_value(scope, &error);
      let thrown = yuzuko_core::op::root(scope, thrown);
      return rejected(scope, thrown);
    }
  };
  let byte_length = view_info.byte_length;
  let buffer_length = view_info.buffer_length;
  let detached = view_info.detached;
  let element_size = view_info.element_size;
  let byte_offset = view_info.byte_offset;
  let kind = view_info.kind;
  let buffer = yuzuko_core::op::root(scope, view_info.buffer);
  if byte_length == 0 || buffer_length == 0 {
    let error = type_error(scope, "view must have non-zero byteLength");
    let error = yuzuko_core::op::root(scope, error);
    return rejected(scope, error);
  }
  if detached {
    let error = type_error(scope, "view's buffer has been detached");
    let error = yuzuko_core::op::root(scope, error);
    return rejected(scope, error);
  }
  let minimum_fill = min.saturating_mul(element_size);
  if minimum_fill < element_size
    || minimum_fill > byte_length
    || minimum_fill % element_size != 0
  {
    let error = type_error(scope, "The minimum fill is outside the view");
    let error = yuzuko_core::op::root(scope, error);
    return rejected(scope, error);
  }
  let transferred = match transfer_buffer(scope, buffer) {
    Ok(buffer) => yuzuko_core::op::root(scope, buffer),
    Err(error) => {
      let thrown = error_value(scope, &error);
      let thrown = yuzuko_core::op::root(scope, thrown);
      return rejected(scope, thrown);
    }
  };
  let pull = PullInto {
    buffer_byte_length: transferred.byte_length(),
    buffer: v8::Global::new(scope, transferred),
    byte_offset,
    byte_length,
    bytes_filled: 0,
    minimum_fill,
    element_size,
    view: kind,
    reader: PullReader::Byob,
  };
  if pending_len(stream) > 0 {
    push_pull(scope, stream, pull);
    return queue_into(scope, stream);
  }
  if phase_of(stream) == Some(Phase::Closed) {
    let buffer = v8::Local::new(scope, &pull.buffer);
    let buffer = yuzuko_core::op::root(scope, buffer);
    let empty = match make_view(scope, pull.view, buffer, pull.byte_offset, 0) {
      Ok(view) => yuzuko_core::op::root(scope, view),
      Err(error) => {
        let thrown = error_value(scope, &error);
        let thrown = yuzuko_core::op::root(scope, thrown);
        return rejected(scope, thrown);
      }
    };
    let result = read_result(scope, Some(empty), true);
    let result = yuzuko_core::op::root(scope, result);
    return resolved(scope, result);
  }
  if queue_total(stream) > 0 {
    let mut pull = pull;
    match fill_from_queue(scope, stream, &mut pull) {
      Ok(true) => {
        return match convert_pull(scope, &pull) {
          Ok(view) => {
            let view = yuzuko_core::op::root(scope, view);
            handle_drain(scope, stream);
            let result = read_result(scope, Some(view), false);
            let result = yuzuko_core::op::root(scope, result);
            resolved(scope, result)
          }
          Err(error) => {
            let thrown = error_value(scope, &error);
            let thrown = yuzuko_core::op::root(scope, thrown);
            rejected(scope, thrown)
          }
        };
      }
      Ok(false) if close_requested(stream) => {
        let error = type_error(
          scope,
          "Insufficient bytes to fill elements in the given buffer",
        );
        let error = yuzuko_core::op::root(scope, error);
        let _ = error_controller(scope, stream, error);
        return rejected(scope, error);
      }
      Ok(false) => {
        push_pull(scope, stream, pull);
        let promise = queue_into(scope, stream);
        call_pull_if_needed(scope, stream);
        return promise;
      }
      Err(error) => {
        let thrown = error_value(scope, &error);
        let thrown = yuzuko_core::op::root(scope, thrown);
        return rejected(scope, thrown);
      }
    }
  }
  push_pull(scope, stream, pull);
  let promise = queue_into(scope, stream);
  call_pull_if_needed(scope, stream);
  promise
}

fn pending_len(stream: u64) -> usize {
  BYTES.with(|slot| {
    slot
      .borrow()
      .get(&stream)
      .map(|byte| byte.pending.len())
      .unwrap_or(0)
  })
}

fn queue_into(scope: &mut v8::PinScope, stream: u64) -> Js {
  let reader = READS
    .with(|slot| slot.borrow().get(&stream).and_then(|state| state.reader));
  let Some(reader) = reader else {
    let error = type_error(scope, "The reader is released");
    let error = yuzuko_core::op::root(scope, error);
    return rejected(scope, error);
  };
  let resolver = v8::PromiseResolver::new(scope).unwrap();
  let promise = resolver.get_promise(scope);
  let promise = yuzuko_core::op::root(scope, promise);
  let resolver = yuzuko_core::op::root(scope, resolver);
  READERS.with(|slot| {
    if let Some(state) = slot.borrow_mut().get_mut(&reader) {
      state
        .into_requests
        .push_back(v8::Global::new(scope, resolver));
    }
  });
  let value: v8::Local<v8::Value> = promise.into();
  Js(v8::Global::new(scope, value))
}

struct ViewInfo<'s> {
  buffer: v8::Local<'s, v8::ArrayBuffer>,
  buffer_length: usize,
  byte_offset: usize,
  byte_length: usize,
  element_size: usize,
  kind: ViewKind,
  detached: bool,
}

fn describe_view<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  value: v8::Local<'s, v8::Value>,
) -> Result<ViewInfo<'s>, OpError> {
  let view = v8::Local::<v8::ArrayBufferView>::try_from(value)
    .map_err(|_| OpError::type_("The view is not an ArrayBuffer view"))?;
  let Some(buffer) = view.buffer(scope) else {
    return Err(OpError::type_("The view buffer is missing"));
  };
  let (kind, element_size) = view_kind(scope, value)?;
  Ok(ViewInfo {
    buffer_length: buffer.byte_length(),
    detached: buffer.was_detached(),
    byte_offset: view.byte_offset(),
    byte_length: view.byte_length(),
    element_size,
    kind,
    buffer,
  })
}

fn view_kind(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<(ViewKind, usize), OpError> {
  let _ = scope;
  if v8::Local::<v8::Uint8Array>::try_from(value).is_ok() {
    return Ok((ViewKind::Uint8, 1));
  }
  if v8::Local::<v8::Uint8ClampedArray>::try_from(value).is_ok() {
    return Ok((ViewKind::Uint8Clamped, 1));
  }
  if v8::Local::<v8::Int8Array>::try_from(value).is_ok() {
    return Ok((ViewKind::Int8, 1));
  }
  if v8::Local::<v8::Uint16Array>::try_from(value).is_ok() {
    return Ok((ViewKind::Uint16, 2));
  }
  if v8::Local::<v8::Int16Array>::try_from(value).is_ok() {
    return Ok((ViewKind::Int16, 2));
  }
  if v8::Local::<v8::Uint32Array>::try_from(value).is_ok() {
    return Ok((ViewKind::Uint32, 4));
  }
  if v8::Local::<v8::Int32Array>::try_from(value).is_ok() {
    return Ok((ViewKind::Int32, 4));
  }
  if v8::Local::<v8::Float32Array>::try_from(value).is_ok() {
    return Ok((ViewKind::Float32, 4));
  }
  if v8::Local::<v8::Float64Array>::try_from(value).is_ok() {
    return Ok((ViewKind::Float64, 8));
  }
  if v8::Local::<v8::BigUint64Array>::try_from(value).is_ok() {
    return Ok((ViewKind::BigUint64, 8));
  }
  if v8::Local::<v8::BigInt64Array>::try_from(value).is_ok() {
    return Ok((ViewKind::BigInt64, 8));
  }
  if v8::Local::<v8::Float16Array>::try_from(value).is_ok() {
    return Ok((ViewKind::Float16, 2));
  }
  if v8::Local::<v8::DataView>::try_from(value).is_ok() {
    return Ok((ViewKind::DataView, 1));
  }
  Err(OpError::type_("The view type is not supported"))
}

fn make_view<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  kind: ViewKind,
  buffer: v8::Local<'s, v8::ArrayBuffer>,
  offset: usize,
  length: usize,
) -> Result<v8::Local<'s, v8::Value>, OpError> {
  let value: Option<v8::Local<v8::Value>> =
    match kind {
      ViewKind::Uint8 => v8::Uint8Array::new(scope, buffer, offset, length)
        .map(|view| view.into()),
      ViewKind::Uint8Clamped => {
        v8::Uint8ClampedArray::new(scope, buffer, offset, length)
          .map(|view| view.into())
      }
      ViewKind::Int8 => v8::Int8Array::new(scope, buffer, offset, length)
        .map(|view| view.into()),
      ViewKind::Uint16 => v8::Uint16Array::new(scope, buffer, offset, length)
        .map(|view| view.into()),
      ViewKind::Int16 => v8::Int16Array::new(scope, buffer, offset, length)
        .map(|view| view.into()),
      ViewKind::Uint32 => v8::Uint32Array::new(scope, buffer, offset, length)
        .map(|view| view.into()),
      ViewKind::Int32 => v8::Int32Array::new(scope, buffer, offset, length)
        .map(|view| view.into()),
      ViewKind::Float16 => v8::Float16Array::new(scope, buffer, offset, length)
        .map(|view| unsafe { v8::Local::<v8::Value>::cast_unchecked(view) }),
      ViewKind::Float32 => v8::Float32Array::new(scope, buffer, offset, length)
        .map(|view| view.into()),
      ViewKind::Float64 => v8::Float64Array::new(scope, buffer, offset, length)
        .map(|view| view.into()),
      ViewKind::BigUint64 => {
        v8::BigUint64Array::new(scope, buffer, offset, length)
          .map(|view| view.into())
      }
      ViewKind::BigInt64 => {
        v8::BigInt64Array::new(scope, buffer, offset, length)
          .map(|view| view.into())
      }
      ViewKind::DataView => {
        Some(v8::DataView::new(scope, buffer, offset, length).into())
      }
    };
  value.ok_or_else(|| OpError::type_("The view could not be created"))
}

fn uint8_view<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  buffer: v8::Local<'s, v8::ArrayBuffer>,
  offset: usize,
  length: usize,
) -> Result<v8::Local<'s, v8::Value>, OpError> {
  make_view(scope, ViewKind::Uint8, buffer, offset, length)
}

fn transfer_buffer<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  buffer: v8::Local<'s, v8::ArrayBuffer>,
) -> Result<v8::Local<'s, v8::ArrayBuffer>, OpError> {
  if buffer.was_detached() || !buffer.is_detachable() {
    return Err(OpError::type_("The buffer cannot be transferred"));
  }
  let length = buffer.byte_length();
  let bytes = copy_out(buffer, 0, length)?;
  let next = crate::bytes::array_buffer(scope, bytes);
  let _ = buffer.detach(None);
  Ok(next)
}

fn copy_out(
  buffer: v8::Local<v8::ArrayBuffer>,
  offset: usize,
  length: usize,
) -> Result<Vec<u8>, OpError> {
  if buffer.was_detached() {
    return Err(OpError::type_("The buffer has been detached"));
  }
  let mut bytes = vec![0u8; length];
  if length == 0 {
    return Ok(bytes);
  }
  let Some(data) = buffer.data() else {
    return Err(OpError::type_("The buffer has no data"));
  };
  unsafe {
    std::ptr::copy_nonoverlapping(
      data.as_ptr().cast::<u8>().add(offset),
      bytes.as_mut_ptr(),
      length,
    );
  }
  Ok(bytes)
}

fn copy_block(
  dest: v8::Local<v8::ArrayBuffer>,
  dest_offset: usize,
  src: v8::Local<v8::ArrayBuffer>,
  src_offset: usize,
  length: usize,
) -> Result<(), OpError> {
  if length == 0 {
    return Ok(());
  }
  let Some(dest_data) = dest.data() else {
    return Err(OpError::type_("The buffer has no data"));
  };
  let Some(src_data) = src.data() else {
    return Err(OpError::type_("The buffer has no data"));
  };
  unsafe {
    std::ptr::copy_nonoverlapping(
      src_data.as_ptr().cast::<u8>().add(src_offset),
      dest_data.as_ptr().cast::<u8>().add(dest_offset),
      length,
    );
  }
  Ok(())
}

fn tee_pull(scope: &mut v8::PinScope, tee: u64, branch: u8) {
  let reading = TEES.with(|slot| {
    let mut tees = slot.borrow_mut();
    let Some(state) = tees.get_mut(&tee) else {
      return true;
    };
    if state.reading {
      state.again[branch as usize] = true;
      return true;
    }
    state.reading = true;
    false
  });
  if reading {
    return;
  }
  let branch_id = TEES.with(|slot| {
    slot
      .borrow()
      .get(&tee)
      .map(|state| state.branches[branch as usize])
  });
  if branch_id.is_some_and(pending_len_positive) {
    tee_pull_byob(scope, tee, branch);
  } else {
    tee_pull_default(scope, tee);
  }
}

fn pending_len_positive(stream: u64) -> bool {
  pending_len(stream) > 0
}

fn tee_pull_default(scope: &mut v8::PinScope, tee: u64) {
  TEES.with(|slot| {
    if let Some(state) = slot.borrow_mut().get_mut(&tee) {
      state.read_branch = None;
    }
  });
  let Some(reader) = ensure_tee_reader(scope, tee, false) else {
    return;
  };
  let read = super::reader_read(scope, reader);
  let value = super::js_value(scope, read);
  let _ = watch_promise(scope, value, Token::TeeRead(tee));
}

fn tee_pull_byob(scope: &mut v8::PinScope, tee: u64, branch: u8) {
  let branch_id = TEES.with(|slot| {
    slot
      .borrow()
      .get(&tee)
      .map(|state| state.branches[branch as usize])
  });
  let Some(branch_id) = branch_id else {
    tee_pull_default(scope, tee);
    return;
  };
  let _ = byob_view_object(scope, branch_id);
  let Some(view) = current_byob_view(scope, branch_id) else {
    tee_pull_default(scope, tee);
    return;
  };
  let view = yuzuko_core::op::root(scope, view);
  TEES.with(|slot| {
    if let Some(state) = slot.borrow_mut().get_mut(&tee) {
      state.read_branch = Some(branch);
    }
  });
  let Some(reader) = ensure_tee_reader(scope, tee, true) else {
    return;
  };
  let stream =
    READERS.with(|slot| slot.borrow().get(&reader).map(|state| state.stream));
  let Some(stream) = stream else {
    return;
  };
  let read = pull_into(scope, stream, view, 1);
  let value = super::js_value(scope, read);
  let _ = watch_promise(scope, value, Token::TeeRead(tee));
}

fn ensure_tee_reader(
  scope: &mut v8::PinScope,
  tee: u64,
  byob: bool,
) -> Option<u64> {
  let (reader, stream) = TEES.with(|slot| {
    let reader = slot.borrow().get(&tee).map(|state| state.reader)?;
    let stream = READERS.with(|readers| {
      readers.borrow().get(&reader).map(|state| state.stream)
    })?;
    Some((reader, stream))
  })?;
  let is_byob = READERS
    .with(|slot| slot.borrow().get(&reader).is_some_and(|state| state.byob));
  if is_byob == byob {
    return Some(reader);
  }
  super::release_reader(scope, reader).ok()?;
  let new = if byob {
    acquire_byob(scope, stream).ok()?.id
  } else {
    super::acquire_reader(scope, stream).ok()?.id
  };
  TEES.with(|slot| {
    if let Some(state) = slot.borrow_mut().get_mut(&tee) {
      state.reader = new;
    }
  });
  watch_reader_closed(scope, tee, new);
  Some(new)
}

fn watch_reader_closed(scope: &mut v8::PinScope, tee: u64, reader: u64) {
  let closed = READERS
    .with(|slot| slot.borrow().get(&reader).map(|state| state.closed.clone()));
  let Some(closed) = closed else {
    return;
  };
  let closed = v8::Local::new(scope, &closed);
  let closed: v8::Local<v8::Value> = closed.into();
  let _ = watch_promise(scope, closed, Token::TeeClosed(tee));
}

fn tee_source(tee: u64) -> Option<u64> {
  let reader =
    TEES.with(|slot| slot.borrow().get(&tee).map(|state| state.reader))?;
  READERS.with(|slot| slot.borrow().get(&reader).map(|state| state.stream))
}

fn tee_branches(tee: u64) -> Option<([u64; 2], [bool; 2])> {
  TEES.with(|slot| {
    slot
      .borrow()
      .get(&tee)
      .map(|state| (state.branches, state.canceled))
  })
}

fn tee_cancel_source(scope: &mut v8::PinScope, tee: u64) {
  let snapshot = TEES.with(|slot| {
    slot.borrow().get(&tee).map(|state| {
      (
        state.reader,
        state.reasons[0].clone(),
        state.reasons[1].clone(),
      )
    })
  });
  let Some((reader, reason1, reason2)) = snapshot else {
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
  let result = super::cancel_stream(scope, source, Some(array.into()));
  let value = super::js_value(scope, result);
  tee_resolve_with(scope, tee, value);
}

fn tee_resolve_cancel(scope: &mut v8::PinScope, tee: u64) {
  let value = v8::undefined(scope).into();
  tee_resolve_with(scope, tee, value);
}

fn tee_resolve_with(
  scope: &mut v8::PinScope,
  tee: u64,
  value: v8::Local<v8::Value>,
) {
  let resolver = TEES.with(|slot| {
    let mut tees = slot.borrow_mut();
    let state = tees.get_mut(&tee)?;
    if state.resolved {
      return None;
    }
    state.resolved = true;
    Some(state.cancel_resolver.clone())
  });
  let Some(resolver) = resolver else {
    return;
  };
  let resolver = v8::Local::new(scope, &resolver);
  let resolver = yuzuko_core::op::root(scope, resolver);
  resolver.resolve(scope, value);
}

fn tee_error_branches(
  scope: &mut v8::PinScope,
  tee: u64,
  error: v8::Local<v8::Value>,
) {
  let Some((branches, _)) = tee_branches(tee) else {
    return;
  };
  let error = yuzuko_core::op::root(scope, error);
  for branch in branches {
    let _ = error_controller(scope, branch, error);
  }
}

fn tee_fail_clone(
  scope: &mut v8::PinScope,
  tee: u64,
  error: v8::Local<v8::Value>,
) {
  let error = yuzuko_core::op::root(scope, error);
  tee_error_branches(scope, tee, error);
  if let Some(source) = tee_source(tee) {
    let result = super::cancel_stream(scope, source, Some(error));
    let value = super::js_value(scope, result);
    tee_resolve_with(scope, tee, value);
  }
  TEES.with(|slot| {
    if let Some(state) = slot.borrow_mut().get_mut(&tee) {
      state.reading = false;
    }
  });
}

fn finish_tee_pull(scope: &mut v8::PinScope, tee: u64) {
  let again = TEES.with(|slot| {
    let mut tees = slot.borrow_mut();
    let state = tees.get_mut(&tee)?;
    state.reading = false;
    Some(state.again)
  });
  let Some(again) = again else {
    return;
  };
  if again[0] {
    tee_pull(scope, tee, 0);
  } else if again[1] {
    tee_pull(scope, tee, 1);
  }
}

fn clone_uint8<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  value: v8::Local<v8::Value>,
) -> Result<v8::Local<'s, v8::Value>, OpError> {
  let view = v8::Local::<v8::ArrayBufferView>::try_from(value)
    .map_err(|_| OpError::type_("The chunk is not an ArrayBuffer view"))?;
  let buffer = view
    .buffer(scope)
    .ok_or_else(|| OpError::type_("The chunk buffer is missing"))?;
  let length = view.byte_length();
  let bytes = copy_out(buffer, view.byte_offset(), length)?;
  let cloned = crate::bytes::array_buffer(scope, bytes);
  uint8_view(scope, cloned, 0, length)
}

fn tee_default_chunk(
  scope: &mut v8::PinScope,
  tee: u64,
  chunk: v8::Local<v8::Value>,
) {
  let Some((branches, canceled)) = tee_branches(tee) else {
    return;
  };
  let chunk = yuzuko_core::op::root(scope, chunk);
  let second = if !canceled[0] && !canceled[1] {
    match clone_uint8(scope, chunk) {
      Ok(cloned) => yuzuko_core::op::root(scope, cloned),
      Err(error) => {
        let thrown = error_value(scope, &error);
        tee_fail_clone(scope, tee, thrown);
        return;
      }
    }
  } else {
    chunk
  };
  if !canceled[0]
    && let Err(error) = enqueue_view(scope, branches[0], chunk, false)
  {
    let thrown = error_value(scope, &error);
    tee_fail_clone(scope, tee, thrown);
    return;
  }
  if !canceled[1]
    && let Err(error) = enqueue_view(scope, branches[1], second, false)
  {
    let thrown = error_value(scope, &error);
    tee_fail_clone(scope, tee, thrown);
    return;
  }
  finish_tee_pull(scope, tee);
}

fn tee_byob_chunk(
  scope: &mut v8::PinScope,
  tee: u64,
  branch: u8,
  chunk: v8::Local<v8::Value>,
) {
  let Some((branches, canceled)) = tee_branches(tee) else {
    return;
  };
  let branch = branch as usize;
  let other = 1 - branch;
  let chunk = yuzuko_core::op::root(scope, chunk);
  if !canceled[other] {
    let cloned = match clone_uint8(scope, chunk) {
      Ok(cloned) => yuzuko_core::op::root(scope, cloned),
      Err(error) => {
        let thrown = error_value(scope, &error);
        tee_fail_clone(scope, tee, thrown);
        return;
      }
    };
    if !canceled[branch] {
      let _ = respond_with_view(scope, branches[branch], chunk);
    }
    if let Err(error) = enqueue_view(scope, branches[other], cloned, false) {
      let thrown = error_value(scope, &error);
      tee_fail_clone(scope, tee, thrown);
      return;
    }
  } else if !canceled[branch] {
    let _ = respond_with_view(scope, branches[branch], chunk);
  }
  finish_tee_pull(scope, tee);
}

fn tee_close(
  scope: &mut v8::PinScope,
  tee: u64,
  read_branch: Option<u8>,
  chunk: v8::Local<v8::Value>,
) {
  TEES.with(|slot| {
    if let Some(state) = slot.borrow_mut().get_mut(&tee) {
      state.reading = false;
    }
  });
  let Some((branches, canceled)) = tee_branches(tee) else {
    return;
  };
  if let Some(branch) = read_branch {
    let branch = branch as usize;
    let other = 1 - branch;
    if !canceled[branch] {
      let _ = close_readable(scope, branches[branch]);
    }
    if !canceled[other] {
      let _ = close_readable(scope, branches[other]);
    }
    if !chunk.is_undefined() {
      if !canceled[branch] {
        let _ = respond_with_view(scope, branches[branch], chunk);
      }
      if !canceled[other] && pending_len(branches[other]) > 0 {
        let _ = respond(scope, branches[other], 0);
      }
    }
  } else {
    if !canceled[0] {
      let _ = close_readable(scope, branches[0]);
    }
    if !canceled[1] {
      let _ = close_readable(scope, branches[1]);
    }
    if pending_len(branches[0]) > 0 {
      let _ = respond(scope, branches[0], 0);
    }
    if pending_len(branches[1]) > 0 {
      let _ = respond(scope, branches[1], 0);
    }
  }
  if !canceled[0] || !canceled[1] {
    tee_resolve_cancel(scope, tee);
  }
}

fn current_byob_view<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  stream: u64,
) -> Option<v8::Local<'s, v8::Value>> {
  let generation = BYTES
    .with(|slot| slot.borrow().get(&stream).map(|byte| byte.byob_generation));
  let _ = generation;
  BYTES.with(|slot| {
    slot.borrow().get(&stream).and_then(|byte| {
      byte
        .byob_view
        .as_ref()
        .map(|view| v8::Local::new(scope, view))
    })
  })
}

pub(super) struct ReadableStreamBYOBReader {
  id: u64,
}

unsafe impl v8::cppgc::GarbageCollected for ReadableStreamBYOBReader {
  fn trace(&self, _visitor: &mut v8::cppgc::Visitor) {}

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"ReadableStreamBYOBReader"
  }
}

#[op]
impl ReadableStreamBYOBReader {
  #[constructor]
  fn new(
    scope: &mut v8::PinScope,
    stream: v8::Local<v8::Value>,
  ) -> Result<Self, OpError> {
    let id = super::stream_id(scope, stream)?;
    acquire_byob(scope, id)
  }

  #[promise]
  fn read(
    &self,
    scope: &mut v8::PinScope,
    view: v8::Local<v8::Value>,
    options: Option<v8::Local<v8::Value>>,
  ) -> Js {
    let view = yuzuko_core::op::root(scope, view);
    let options = options.map(|value| yuzuko_core::op::root(scope, value));
    let min = match read_min(scope, view, options) {
      Ok(min) => min,
      Err(error) => {
        let thrown = rejected_error(scope, &error);
        let thrown = yuzuko_core::op::root(scope, thrown);
        return rejected(scope, thrown);
      }
    };
    let state = READERS.with(|slot| {
      slot
        .borrow()
        .get(&self.id)
        .map(|reader| (reader.released, reader.stream))
    });
    let Some((false, stream)) = state else {
      let error = type_error(scope, "The reader is released");
      let error = yuzuko_core::op::root(scope, error);
      return rejected(scope, error);
    };
    if phase_of(stream) == Some(Phase::Errored) {
      let error = stored_error(scope, stream);
      let error = yuzuko_core::op::root(scope, error);
      return rejected(scope, error);
    }
    disturb(stream);
    pull_into(scope, stream, view, min)
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
      Some((false, stream)) => super::cancel_stream(scope, stream, reason),
      _ => {
        let error = type_error(scope, "The reader is released");
        let error = yuzuko_core::op::root(scope, error);
        rejected(scope, error)
      }
    }
  }

  fn release_lock(&self, scope: &mut v8::PinScope) -> Result<(), OpError> {
    super::release_reader(scope, self.id)
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

fn rejected_error<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  error: &OpError,
) -> v8::Local<'s, v8::Value> {
  match error {
    OpError::Range(message) => {
      let message = v8::String::new(scope, message).unwrap();
      v8::Exception::range_error(scope, message)
    }
    _ => error_value(scope, error),
  }
}

fn read_min<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  view: v8::Local<'s, v8::Value>,
  options: Option<v8::Local<'s, v8::Value>>,
) -> Result<usize, OpError> {
  let Some(options) = options.filter(|value| !value.is_undefined()) else {
    return Ok(1);
  };
  if options.is_null() || !options.is_object() {
    return Err(OpError::type_("The read options are not an object"));
  }
  let object = object_value(scope, options)?;
  let Some(min) = property(scope, object, "min")? else {
    return Ok(1);
  };
  if min.is_undefined() {
    return Ok(1);
  }
  let number = min
    .number_value(scope)
    .ok_or_else(|| OpError::type_("options.min is not a number"))?;
  if !number.is_finite() || number < 0.0 || number.fract() != 0.0 {
    return Err(OpError::type_("options.min is invalid"));
  }
  if number == 0.0 {
    return Err(OpError::type_("options.min must be greater than 0"));
  }
  let min = number as usize;
  let info = describe_view(scope, view)?;
  let limit = if info.kind == ViewKind::DataView {
    info.byte_length
  } else {
    info.byte_length / info.element_size
  };
  if min > limit {
    return Err(OpError::range(
      "options.min must be less than or equal to the view length",
    ));
  }
  Ok(min)
}

pub(super) struct ReadableByteStreamController {
  stream: u64,
}

unsafe impl v8::cppgc::GarbageCollected for ReadableByteStreamController {
  fn trace(&self, _visitor: &mut v8::cppgc::Visitor) {}

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"ReadableByteStreamController"
  }
}

#[op]
impl ReadableByteStreamController {
  fn enqueue(
    &self,
    scope: &mut v8::PinScope,
    chunk: v8::Local<v8::Value>,
  ) -> Result<(), OpError> {
    enqueue_view(scope, self.stream, chunk, true)
  }

  fn close(&self, scope: &mut v8::PinScope) -> Result<(), OpError> {
    controller_close(scope, self.stream)
  }

  fn error(
    &self,
    scope: &mut v8::PinScope,
    error: Option<v8::Local<v8::Value>>,
  ) -> Result<(), OpError> {
    let error = error.unwrap_or_else(|| v8::undefined(scope).into());
    error_controller(scope, self.stream, error)
  }

  #[getter]
  fn desired_size(&self) -> Option<f64> {
    desired_size(self.stream)
  }

  #[getter]
  fn byob_request(&self, scope: &mut v8::PinScope) -> Js {
    if let Some(object) = ensure_byob_request(scope, self.stream) {
      let local = v8::Local::new(scope, &object);
      let value: v8::Local<v8::Value> = local.into();
      return Js(v8::Global::new(scope, value));
    }
    let value: v8::Local<v8::Value> = v8::null(scope).into();
    Js(v8::Global::new(scope, value))
  }
}

fn ensure_byob_request(
  scope: &mut v8::PinScope,
  stream: u64,
) -> Option<v8::Global<v8::Object>> {
  byob_view_object(scope, stream)
}

struct ReadableStreamBYOBRequest {
  stream: u64,
  generation: u64,
}

unsafe impl v8::cppgc::GarbageCollected for ReadableStreamBYOBRequest {
  fn trace(&self, _visitor: &mut v8::cppgc::Visitor) {}

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"ReadableStreamBYOBRequest"
  }
}

#[op]
impl ReadableStreamBYOBRequest {
  #[getter]
  fn view(&self, scope: &mut v8::PinScope) -> Js {
    let current = BYTES.with(|slot| {
      slot
        .borrow()
        .get(&self.stream)
        .map(|byte| byte.byob_generation)
    });
    if current != Some(self.generation) {
      let value: v8::Local<v8::Value> = v8::null(scope).into();
      return Js(v8::Global::new(scope, value));
    }
    match current_byob_view(scope, self.stream) {
      Some(view) => Js(v8::Global::new(scope, view)),
      None => {
        let value: v8::Local<v8::Value> = v8::null(scope).into();
        Js(v8::Global::new(scope, value))
      }
    }
  }

  fn respond(
    &self,
    scope: &mut v8::PinScope,
    bytes_written: f64,
  ) -> Result<(), OpError> {
    self.require_current()?;
    if !bytes_written.is_finite()
      || bytes_written < 0.0
      || bytes_written.fract() != 0.0
    {
      return Err(OpError::type_("bytesWritten is invalid"));
    }
    respond(scope, self.stream, bytes_written as usize)
  }

  fn respond_with_new_view(
    &self,
    scope: &mut v8::PinScope,
    view: v8::Local<v8::Value>,
  ) -> Result<(), OpError> {
    self.require_current()?;
    respond_with_view(scope, self.stream, view)
  }
}

impl ReadableStreamBYOBRequest {
  fn require_current(&self) -> Result<(), OpError> {
    let current = BYTES.with(|slot| {
      slot
        .borrow()
        .get(&self.stream)
        .map(|byte| byte.byob_generation)
    });
    if current == Some(self.generation) {
      Ok(())
    } else {
      Err(OpError::type_("The BYOB request is no longer valid"))
    }
  }
}

pub(super) fn install(scope: &mut v8::PinScope) {
  ReadableByteStreamController::install(scope);
  ReadableStreamBYOBReader::install(scope);
  ReadableStreamBYOBRequest::install(scope);
}

/// What a readable byte stream's controller will accept right now.
///
/// `Byob.element_size` is 0 for a `DataView`. The byte-stream descriptor
/// stores 1 for that view so later fills do not divide by zero. Datagram
/// delivery must use this reported size: a `DataView` is not an element
/// size of 1.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ByteWait {
  /// The controller is not a readable byte stream.
  Closed,
  /// Readable, and no default or BYOB read is waiting.
  Idle,
  /// A default reader has a pending read.
  Default,
  /// A BYOB read is waiting. `byte_length` is the unfilled view length.
  Byob {
    byte_length: usize,
    element_size: usize,
  },
}

pub fn byte_controller_wait(
  scope: &mut v8::PinScope,
  controller: v8::Local<v8::Object>,
) -> ByteWait {
  let Some(id) = controller_stream(scope, controller) else {
    return ByteWait::Closed;
  };
  if phase_of(id) != Some(Phase::Readable) {
    return ByteWait::Closed;
  }
  let byob = BYTES.with(|slot| {
    slot.borrow().get(&id).and_then(|byte| {
      let pull = byte.pending.front()?;
      if pull.reader != PullReader::Byob {
        return None;
      }
      let element_size = if pull.view == ViewKind::DataView {
        0
      } else {
        pull.element_size
      };
      Some(ByteWait::Byob {
        byte_length: pull.byte_length.saturating_sub(pull.bytes_filled),
        element_size,
      })
    })
  });
  if let Some(wait) = byob {
    return wait;
  }
  let (default_reader, requests, _) = reader_counts(id);
  if default_reader && requests > 0 {
    ByteWait::Default
  } else {
    ByteWait::Idle
  }
}

/// Enqueue one `Uint8Array` chunk. A waiting default reader receives the
/// whole view. A waiting BYOB reader is filled by the byte queue, which
/// may split the chunk.
pub fn byte_controller_enqueue(
  scope: &mut v8::PinScope,
  controller: v8::Local<v8::Object>,
  bytes: &[u8],
) -> Result<(), OpError> {
  let id = controller_stream(scope, controller)
    .ok_or_else(|| OpError::type_("The controller is not a byte stream"))?;
  let length = bytes.len();
  let buffer = crate::bytes::array_buffer(scope, bytes.to_vec());
  let view = v8::Uint8Array::new(scope, buffer, 0, length)
    .ok_or_else(|| OpError::type_("The view could not be created"))?;
  let view = yuzuko_core::op::root(scope, view);
  enqueue_view(scope, id, view.into(), true)
}

/// Copy `bytes` into the pending BYOB view and `respond` with that length.
pub fn byte_controller_respond(
  scope: &mut v8::PinScope,
  controller: v8::Local<v8::Object>,
  bytes: &[u8],
) -> Result<(), OpError> {
  let id = controller_stream(scope, controller)
    .ok_or_else(|| OpError::type_("The controller is not a byte stream"))?;
  let placed = BYTES.with(|slot| {
    let states = slot.borrow();
    let byte = states.get(&id)?;
    let pull = byte.pending.front()?;
    if pull.reader != PullReader::Byob {
      return None;
    }
    Some((pull.buffer.clone(), pull.byte_offset + pull.bytes_filled))
  });
  let Some((buffer, offset)) = placed else {
    return Err(OpError::type_("There is no pending BYOB request"));
  };
  let buffer = v8::Local::new(scope, &buffer);
  write_bytes(buffer, offset, bytes)?;
  respond(scope, id, bytes.len())
}

fn controller_stream(
  scope: &mut v8::PinScope,
  controller: v8::Local<v8::Object>,
) -> Option<u64> {
  let host = unsafe {
    yuzuko_core::op::host_ref::<ReadableByteStreamController>(scope, controller)
  }?;
  Some(host.get().stream)
}

fn write_bytes(
  buffer: v8::Local<v8::ArrayBuffer>,
  offset: usize,
  bytes: &[u8],
) -> Result<(), OpError> {
  if bytes.is_empty() {
    return Ok(());
  }
  if buffer.was_detached() {
    return Err(OpError::type_("The buffer has been detached"));
  }
  let end = offset
    .checked_add(bytes.len())
    .ok_or_else(|| OpError::range("bytesWritten out of range"))?;
  if end > buffer.byte_length() {
    return Err(OpError::range("bytesWritten out of range"));
  }
  let Some(data) = buffer.data() else {
    return Err(OpError::type_("The buffer has no data"));
  };
  unsafe {
    std::ptr::copy_nonoverlapping(
      bytes.as_ptr(),
      data.as_ptr().cast::<u8>().add(offset),
      bytes.len(),
    );
  }
  Ok(())
}

impl Clone for PullInto {
  fn clone(&self) -> Self {
    Self {
      buffer: self.buffer.clone(),
      buffer_byte_length: self.buffer_byte_length,
      byte_offset: self.byte_offset,
      byte_length: self.byte_length,
      bytes_filled: self.bytes_filled,
      minimum_fill: self.minimum_fill,
      element_size: self.element_size,
      view: self.view,
      reader: self.reader,
    }
  }
}

pub(super) fn owns_tee(id: u64) -> bool {
  TEES.with(|slot| slot.borrow().contains_key(&id))
}

pub(super) fn tee_reader_rejected(
  scope: &mut v8::PinScope,
  tee: u64,
  error: v8::Local<v8::Value>,
) {
  let current =
    TEES.with(|slot| slot.borrow().get(&tee).map(|state| state.reader));
  let Some(reader) = current else {
    return;
  };
  let (released, stream) = READERS
    .with(|slot| {
      slot
        .borrow()
        .get(&reader)
        .map(|state| (state.released, state.stream))
    })
    .unwrap_or((true, 0));
  // Releasing a reader rejects its closed promise. Ignore that when the
  // replacement reader is already current and the source is still readable.
  if !released && phase_of(stream) == Some(Phase::Readable) {
    return;
  }
  let error = yuzuko_core::op::root(scope, error);
  tee_error_branches(scope, tee, error);
  TEES.with(|slot| {
    if let Some(state) = slot.borrow_mut().get_mut(&tee) {
      state.reading = false;
    }
  });
  let open = TEES.with(|slot| {
    slot
      .borrow()
      .get(&tee)
      .is_some_and(|state| !state.canceled[0] || !state.canceled[1])
  });
  if open {
    tee_resolve_cancel(scope, tee);
  }
}

pub(super) fn tee_settled(
  scope: &mut v8::PinScope,
  tee: u64,
  result: Result<v8::Local<v8::Value>, v8::Local<v8::Value>>,
) {
  let read_branch = TEES.with(|slot| {
    let mut tees = slot.borrow_mut();
    let state = tees.get_mut(&tee)?;
    state.again = [false, false];
    Some(state.read_branch)
  });
  let Some(read_branch) = read_branch else {
    return;
  };
  match result {
    Err(_) => {
      TEES.with(|slot| {
        if let Some(state) = slot.borrow_mut().get_mut(&tee) {
          state.reading = false;
        }
      });
    }
    Ok(value) => {
      let Some((done, chunk)) = super::read_record(scope, value) else {
        TEES.with(|slot| {
          if let Some(state) = slot.borrow_mut().get_mut(&tee) {
            state.reading = false;
          }
        });
        return;
      };
      let chunk = yuzuko_core::op::root(scope, chunk);
      if done {
        tee_close(scope, tee, read_branch, chunk);
      } else if let Some(branch) = read_branch {
        tee_byob_chunk(scope, tee, branch, chunk);
      } else {
        tee_default_chunk(scope, tee, chunk);
      }
    }
  }
}
