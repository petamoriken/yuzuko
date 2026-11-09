//! Cross-realm `ReadableStream`, `WritableStream`, and `TransformStream`.
//!
//! The helper is compiled once for the realm that installs streams. A call
//! from another context on the same thread, such as an `AudioWorklet`, fails
//! instead of recompiling. Export runs JavaScript while serializing. Import
//! only builds a shell: `ReadHostObject` cannot call JavaScript. The
//! outermost clone finisher then binds the port and arms `start`. `started`
//! stays false until that promise reaction, and the isolate drains
//! microtasks only at an explicit checkpoint. A user source that is already
//! started can pull during `pipeTo` before export returns. That pull does
//! not post.

use std::cell::RefCell;

use yuzuko_core::op::host_ref;

use super::{
  ReadableStream, TransformStream, WritableStream, is_locked, writable_locked,
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum StreamKind {
  Readable,
  Writable,
  Transform,
}

/// Whether `object` can move, before any transfer list check.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum StreamStatus {
  /// `object` is not a `ReadableStream`, `WritableStream`, or `TransformStream`.
  Skip,
  /// The stream is unlocked. `Ready` still requires the transfer list.
  Ready(StreamKind),
  /// A readable or writable side is locked, or a side is missing.
  Locked,
}

pub enum TransferError {
  /// The helper threw. The caller rethrows this and does not create another
  /// `DataCloneError`.
  Thrown(v8::Global<v8::Value>),
  /// The helper was missing, the realm did not match, or the value was not a
  /// usable stream. The caller rejects the clone.
  Failed,
}

struct Helper {
  realm: v8::Global<v8::Object>,
  api: v8::Global<v8::Object>,
  unit_size: v8::Global<v8::Function>,
}

enum Pending {
  Readable {
    id: u64,
    port: v8::Global<v8::Object>,
  },
  Writable {
    id: u64,
    port: v8::Global<v8::Object>,
  },
}

thread_local! {
  static HELPER: RefCell<Option<Helper>> = const { RefCell::new(None) };
  static FRAMES: RefCell<Vec<Vec<Pending>>> = const { RefCell::new(Vec::new()) };
}

enum Call {
  Value(v8::Global<v8::Object>),
  Thrown(v8::Global<v8::Value>),
  Failed,
}

pub(super) fn install(scope: &mut v8::PinScope) {
  let compiled = {
    v8::tc_scope!(let tc, scope);
    let source = v8::String::new(tc, HELPER_SOURCE);
    let missing_source = source.is_none();
    let value = source.and_then(|source| {
      v8::Script::compile(tc, source, None).and_then(|script| script.run(tc))
    });
    match value {
      Some(value) => match v8::Local::<v8::Object>::try_from(value) {
        Ok(object) => Ok(v8::Global::new(tc, object)),
        Err(_) => {
          Err("the transfer helper did not return an object".to_string())
        }
      },
      None => {
        let text = tc.exception().map(|value| value.to_rust_string_lossy(tc));
        let fallback = if missing_source {
          "the transfer helper source is not a string"
        } else {
          "the transfer helper failed to compile"
        };
        Err(text.unwrap_or_else(|| fallback.to_string()))
      }
    }
  };
  let api = match compiled {
    Ok(api) => api,
    Err(text) => panic!("stream transfer helper: {text}"),
  };
  let Some(unit_size) = v8::Function::new(scope, unit_size_callback) else {
    panic!("stream transfer helper: the unit size function is missing");
  };
  let unit_size = v8::Global::new(scope, unit_size);
  let realm = yuzuko_core::global(scope);
  let realm = v8::Global::new(scope, realm);
  HELPER.with(|slot| {
    *slot.borrow_mut() = Some(Helper {
      realm,
      api,
      unit_size,
    });
  });
}

pub(super) fn register_hooks() {
  yuzuko_core::add_clone_enter(push_frame);
  yuzuko_core::add_clone_discard(pop_frame);
  yuzuko_core::add_clone_finisher(finish_pending);
}

pub(super) fn clear() {
  HELPER.with(|slot| *slot.borrow_mut() = None);
  FRAMES.with(|slot| slot.borrow_mut().clear());
}

fn unit_size_callback(
  _scope: &mut v8::PinScope,
  _args: v8::FunctionCallbackArguments,
  mut ret: v8::ReturnValue,
) {
  ret.set_int32(1);
}

fn push_frame() {
  FRAMES.with(|slot| slot.borrow_mut().push(Vec::new()));
}

fn pop_frame() {
  FRAMES.with(|slot| {
    slot.borrow_mut().pop();
  });
}

fn finish_pending(
  scope: &mut v8::PinScope,
  success: bool,
  outermost: bool,
) -> Result<(), Option<v8::Global<v8::Value>>> {
  let mine = FRAMES.with(|slot| slot.borrow_mut().pop().unwrap_or_default());
  if !success {
    return Ok(());
  }
  if !outermost {
    FRAMES.with(|slot| {
      if let Some(parent) = slot.borrow_mut().last_mut() {
        parent.extend(mine);
      }
    });
    return Ok(());
  }
  for item in mine {
    if let Err(error) = wire(scope, item) {
      return Err(clone_failure(scope, error));
    }
  }
  Ok(())
}

fn clone_failure(
  scope: &mut v8::PinScope,
  error: TransferError,
) -> Option<v8::Global<v8::Value>> {
  match error {
    TransferError::Thrown(thrown) => Some(thrown),
    TransferError::Failed => {
      v8::tc_scope!(let tc, scope);
      yuzuko_core::reject_clone(tc);
      tc.exception().map(|thrown| v8::Global::new(tc, thrown))
    }
  }
}

/// Classifies `object` without running JavaScript.
pub fn transfer_status(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
) -> StreamStatus {
  if let Some(id) = readable_id(scope, object) {
    return if is_locked(id) {
      StreamStatus::Locked
    } else {
      StreamStatus::Ready(StreamKind::Readable)
    };
  }
  if let Some(id) = writable_stream_id(scope, object) {
    return if writable_locked(id) {
      StreamStatus::Locked
    } else {
      StreamStatus::Ready(StreamKind::Writable)
    };
  }
  let Some(host) = (unsafe { host_ref::<TransformStream>(scope, object) })
  else {
    return StreamStatus::Skip;
  };
  let readable = yuzuko_core::op::traced_local(scope, &host.get().readable);
  let writable = yuzuko_core::op::traced_local(scope, &host.get().writable);
  let sides = match (readable, writable) {
    (Some(readable), Some(writable)) => Some((
      v8::Global::new(scope, readable),
      v8::Global::new(scope, writable),
    )),
    _ => None,
  };
  let Some((readable, writable)) = sides else {
    return StreamStatus::Locked;
  };
  if side_locked(scope, &readable, true) || side_locked(scope, &writable, false)
  {
    StreamStatus::Locked
  } else {
    StreamStatus::Ready(StreamKind::Transform)
  }
}

/// Pipes `stream` into a cross-realm writable and returns the port the
/// destination reads.
pub fn export_readable(
  scope: &mut v8::PinScope,
  stream: v8::Local<v8::Object>,
) -> Result<v8::Global<v8::Object>, TransferError> {
  let Some(id) = readable_id(scope, stream) else {
    return Err(TransferError::Failed);
  };
  if is_locked(id) {
    return Err(TransferError::Failed);
  }
  call_helper(scope, "exportReadable", &[stream.into()])
}

/// Pipes a cross-realm readable into `stream` and returns the port the
/// destination writes.
pub fn export_writable(
  scope: &mut v8::PinScope,
  stream: v8::Local<v8::Object>,
) -> Result<v8::Global<v8::Object>, TransferError> {
  let Some(id) = writable_stream_id(scope, stream) else {
    return Err(TransferError::Failed);
  };
  if writable_locked(id) {
    return Err(TransferError::Failed);
  }
  call_helper(scope, "exportWritable", &[stream.into()])
}

/// Transfers both sides. Both locks are checked before either side is piped.
pub fn export_transform(
  scope: &mut v8::PinScope,
  stream: v8::Local<v8::Object>,
) -> Result<(v8::Global<v8::Object>, v8::Global<v8::Object>), TransferError> {
  let Some((readable, writable)) = transform_sides(scope, stream) else {
    return Err(TransferError::Failed);
  };
  if side_locked(scope, &readable, true) || side_locked(scope, &writable, false)
  {
    return Err(TransferError::Failed);
  }
  let readable = v8::Local::new(scope, &readable);
  let readable_port = export_readable(scope, readable)?;
  let writable = v8::Local::new(scope, &writable);
  let writable_port = export_writable(scope, writable)?;
  Ok((readable_port, writable_port))
}

/// Readable shell for `port`. The port is bound after the clone leaves V8.
///
/// Creating the shell does not run JavaScript. `started` stays false.
pub fn prepare_cross_readable(
  scope: &mut v8::PinScope,
  port: v8::Global<v8::Object>,
) -> Result<v8::Global<v8::Object>, TransferError> {
  let size_fn = unit_size(scope)?;
  if !frame_open() {
    return Err(TransferError::Failed);
  }
  let id = super::alloc();
  super::READS.with(|slot| {
    slot.borrow_mut().insert(
      id,
      super::ReadState {
        queue: std::collections::VecDeque::new(),
        queued: 0.0,
        high_water: 0.0,
        phase: super::Phase::Readable,
        error: None,
        reader: None,
        close_requested: false,
        disturbed: false,
        pulling: false,
        pull_again: false,
        started: false,
        pull_fn: None,
        cancel_fn: None,
        size_fn: Some(size_fn),
        native: None,
        paired: None,
      },
    );
  });
  let _ = ReadableStream::template(scope);
  let object =
    unsafe { yuzuko_core::op::wrap_instance(scope, ReadableStream { id }) };
  if !enqueue_pending(Pending::Readable { id, port }) {
    return Err(TransferError::Failed);
  }
  Ok(v8::Global::new(scope, object))
}

/// Writable shell for `port`. One chunk of queue space, not started.
pub fn prepare_cross_writable(
  scope: &mut v8::PinScope,
  port: v8::Global<v8::Object>,
) -> Result<v8::Global<v8::Object>, TransferError> {
  let size_fn = unit_size(scope)?;
  if !frame_open() {
    return Err(TransferError::Failed);
  }
  let id = super::alloc();
  super::WRITES.with(|slot| {
    slot.borrow_mut().insert(
      id,
      super::WriteState {
        high_water: 1.0,
        queued: 0.0,
        phase: super::Phase::Readable,
        error: None,
        writer: None,
        started: false,
        closing: false,
        close_started: false,
        sink: super::Sink::Js {
          write: None,
          close: None,
          abort: None,
        },
        in_flight: 0,
        size_fn: Some(size_fn),
        queue: std::collections::VecDeque::new(),
        close_resolver: None,
        abort_wait: None,
        abort_promise: None,
        abort_reason: None,
        abort_controller: None,
      },
    );
  });
  let _ = WritableStream::template(scope);
  let object =
    unsafe { yuzuko_core::op::wrap_instance(scope, WritableStream { id }) };
  if !enqueue_pending(Pending::Writable { id, port }) {
    return Err(TransferError::Failed);
  }
  Ok(v8::Global::new(scope, object))
}

/// A `TransformStream` shell. `readable` and `writable` are already the
/// transferred sides, so this does not run the transform constructor.
pub fn shell_transform(
  scope: &mut v8::PinScope,
  readable: v8::Local<v8::Object>,
  writable: v8::Local<v8::Object>,
) -> v8::Global<v8::Object> {
  let _ = TransformStream::template(scope);
  let readable = yuzuko_core::op::traced(scope, readable);
  let writable = yuzuko_core::op::traced(scope, writable);
  let object = unsafe {
    yuzuko_core::op::wrap_instance(
      scope,
      TransformStream { readable, writable },
    )
  };
  v8::Global::new(scope, object)
}

fn readable_id(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
) -> Option<u64> {
  let host = unsafe { host_ref::<ReadableStream>(scope, object) }?;
  Some(host.get().id)
}

fn writable_stream_id(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
) -> Option<u64> {
  let host = unsafe { host_ref::<WritableStream>(scope, object) }?;
  Some(host.get().id)
}

fn transform_sides(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
) -> Option<(v8::Global<v8::Object>, v8::Global<v8::Object>)> {
  let host = unsafe { host_ref::<TransformStream>(scope, object) }?;
  let readable = yuzuko_core::op::traced_local(scope, &host.get().readable)?;
  let writable = yuzuko_core::op::traced_local(scope, &host.get().writable)?;
  Some((
    v8::Global::new(scope, readable),
    v8::Global::new(scope, writable),
  ))
}

fn side_locked(
  scope: &mut v8::PinScope,
  object: &v8::Global<v8::Object>,
  readable: bool,
) -> bool {
  let object = v8::Local::new(scope, object);
  if readable {
    match readable_id(scope, object) {
      Some(id) => is_locked(id),
      None => true,
    }
  } else {
    match writable_stream_id(scope, object) {
      Some(id) => writable_locked(id),
      None => true,
    }
  }
}

fn frame_open() -> bool {
  FRAMES.with(|slot| !slot.borrow().is_empty())
}

fn enqueue_pending(item: Pending) -> bool {
  FRAMES.with(|slot| {
    let mut frames = slot.borrow_mut();
    let Some(frame) = frames.last_mut() else {
      return false;
    };
    frame.push(item);
    true
  })
}

fn unit_size(
  scope: &mut v8::PinScope,
) -> Result<v8::Global<v8::Function>, TransferError> {
  let current = yuzuko_core::global(scope);
  let current = v8::Global::new(scope, current);
  HELPER.with(|slot| {
    let helper = slot.borrow();
    let Some(helper) = helper.as_ref() else {
      return Err(TransferError::Failed);
    };
    let stored = v8::Local::new(scope, &helper.realm);
    let current = v8::Local::new(scope, &current);
    if !stored.strict_equals(current.into()) {
      return Err(TransferError::Failed);
    }
    Ok(v8::Global::clone(&helper.unit_size))
  })
}

fn helper_api(
  scope: &mut v8::PinScope,
) -> Result<v8::Global<v8::Object>, TransferError> {
  let current = yuzuko_core::global(scope);
  let current = v8::Global::new(scope, current);
  HELPER.with(|slot| {
    let helper = slot.borrow();
    let Some(helper) = helper.as_ref() else {
      return Err(TransferError::Failed);
    };
    let stored = v8::Local::new(scope, &helper.realm);
    let current = v8::Local::new(scope, &current);
    if !stored.strict_equals(current.into()) {
      return Err(TransferError::Failed);
    }
    Ok(v8::Global::clone(&helper.api))
  })
}

fn call_helper(
  scope: &mut v8::PinScope,
  name: &str,
  args: &[v8::Local<v8::Value>],
) -> Result<v8::Global<v8::Object>, TransferError> {
  let api = helper_api(scope)?;
  let args: Vec<v8::Global<v8::Value>> = args
    .iter()
    .map(|arg| v8::Global::new(scope, *arg))
    .collect();
  let result = {
    v8::tc_scope!(let tc, scope);
    let api = v8::Local::new(tc, &api);
    let key = v8::String::new(tc, name);
    let function = key
      .and_then(|key| api.get(tc, key.into()))
      .and_then(|value| v8::Local::<v8::Function>::try_from(value).ok());
    match function {
      None => Call::Failed,
      Some(function) => {
        let args: Vec<v8::Local<v8::Value>> =
          args.iter().map(|arg| v8::Local::new(tc, arg)).collect();
        let this = v8::undefined(tc).into();
        match function.call(tc, this, &args) {
          Some(value) => match v8::Local::<v8::Object>::try_from(value) {
            Ok(object) => Call::Value(v8::Global::new(tc, object)),
            Err(_) => Call::Failed,
          },
          None => match tc.exception() {
            Some(thrown) => Call::Thrown(v8::Global::new(tc, thrown)),
            None => Call::Failed,
          },
        }
      }
    }
  };
  match result {
    Call::Value(object) => Ok(object),
    Call::Thrown(thrown) => Err(TransferError::Thrown(thrown)),
    Call::Failed => Err(TransferError::Failed),
  }
}

fn wire(scope: &mut v8::PinScope, item: Pending) -> Result<(), TransferError> {
  match item {
    Pending::Readable { id, port } => {
      let port = v8::Local::new(scope, &port);
      wire_readable(scope, id, port)
    }
    Pending::Writable { id, port } => {
      let port = v8::Local::new(scope, &port);
      wire_writable(scope, id, port)
    }
  }
}

fn wire_readable(
  scope: &mut v8::PinScope,
  id: u64,
  port: v8::Local<v8::Object>,
) -> Result<(), TransferError> {
  let present = super::READS.with(|slot| slot.borrow().contains_key(&id));
  if !present {
    return Err(TransferError::Failed);
  }
  let _ = super::ReadableStreamDefaultController::template(scope);
  let controller = unsafe {
    yuzuko_core::op::wrap_instance(
      scope,
      super::ReadableStreamDefaultController { stream: id },
    )
  };
  let controller: v8::Local<v8::Value> = controller.into();
  let bound = call_helper(scope, "bindReadable", &[port.into(), controller])?;
  let pull = function_field(scope, &bound, "pull")?;
  let cancel = function_field(scope, &bound, "cancel")?;
  let stored = super::READS.with(|slot| {
    let mut states = slot.borrow_mut();
    let Some(state) = states.get_mut(&id) else {
      return false;
    };
    state.pull_fn = Some(pull);
    state.cancel_fn = Some(cancel);
    true
  });
  if !stored {
    return Err(TransferError::Failed);
  }
  let value = v8::undefined(scope).into();
  let _ = super::watch_promise(scope, value, super::Token::StartRead(id));
  Ok(())
}

fn wire_writable(
  scope: &mut v8::PinScope,
  id: u64,
  port: v8::Local<v8::Object>,
) -> Result<(), TransferError> {
  let present = super::WRITES.with(|slot| slot.borrow().contains_key(&id));
  if !present {
    return Err(TransferError::Failed);
  }
  let abort_controller = super::fresh_abort_controller(scope);
  super::WRITES.with(|slot| {
    if let Some(state) = slot.borrow_mut().get_mut(&id) {
      state.abort_controller = abort_controller;
    }
  });
  let _ = super::WritableStreamDefaultController::template(scope);
  let controller = unsafe {
    yuzuko_core::op::wrap_instance(
      scope,
      super::WritableStreamDefaultController { stream: id },
    )
  };
  let controller: v8::Local<v8::Value> = controller.into();
  let bound = call_helper(scope, "bindWritable", &[port.into(), controller])?;
  let write = function_field(scope, &bound, "write")?;
  let close = function_field(scope, &bound, "close")?;
  let abort = function_field(scope, &bound, "abort")?;
  let stored = super::WRITES.with(|slot| {
    let mut states = slot.borrow_mut();
    let Some(state) = states.get_mut(&id) else {
      return false;
    };
    state.sink = super::Sink::Js {
      write: Some(write),
      close: Some(close),
      abort: Some(abort),
    };
    true
  });
  if !stored {
    return Err(TransferError::Failed);
  }
  let value = v8::undefined(scope).into();
  let _ = super::watch_promise(scope, value, super::Token::StartWrite(id));
  Ok(())
}

fn function_field(
  scope: &mut v8::PinScope,
  object: &v8::Global<v8::Object>,
  name: &str,
) -> Result<v8::Global<v8::Function>, TransferError> {
  let result = {
    v8::tc_scope!(let tc, scope);
    let object = v8::Local::new(tc, object);
    let key = v8::String::new(tc, name);
    let function = key
      .and_then(|key| object.get(tc, key.into()))
      .and_then(|value| v8::Local::<v8::Function>::try_from(value).ok());
    match function {
      Some(function) => {
        let function: v8::Local<v8::Object> = function.into();
        Call::Value(v8::Global::new(tc, function))
      }
      None => match tc.exception() {
        Some(thrown) => Call::Thrown(v8::Global::new(tc, thrown)),
        None => Call::Failed,
      },
    }
  };
  match result {
    Call::Value(function) => {
      let function = v8::Local::new(scope, &function);
      match v8::Local::<v8::Function>::try_from(function) {
        Ok(function) => Ok(v8::Global::new(scope, function)),
        Err(_) => Err(TransferError::Failed),
      }
    }
    Call::Thrown(thrown) => Err(TransferError::Thrown(thrown)),
    Call::Failed => Err(TransferError::Failed),
  }
}

const HELPER_SOURCE: &str = r#"(function () {
  "use strict";
  const reflectApply = Reflect.apply;
  const objectCreate = Object.create;
  const promiseThen = Promise.prototype.then;
  const promiseResolve = Promise.resolve;
  const promiseReject = Promise.reject;
  const PromiseCtor = Promise;
  const DOMExceptionCtor = DOMException;
  const MessageChannelCtor = MessageChannel;
  const ReadableStreamCtor = ReadableStream;
  const WritableStreamCtor = WritableStream;
  const addEventListener = EventTarget.prototype.addEventListener;
  const portStart = MessagePort.prototype.start;
  const portClose = MessagePort.prototype.close;
  const portPostMessage = MessagePort.prototype.postMessage;
  const pipeTo = ReadableStream.prototype.pipeTo;
  const readableEnqueue = ReadableStreamDefaultController.prototype.enqueue;
  const readableClose = ReadableStreamDefaultController.prototype.close;
  const readableError = ReadableStreamDefaultController.prototype.error;
  const writableError = WritableStreamDefaultController.prototype.error;

  if (typeof MessageChannelCtor !== "function"
      || typeof ReadableStreamCtor !== "function"
      || typeof WritableStreamCtor !== "function"
      || typeof DOMExceptionCtor !== "function"
      || typeof portPostMessage !== "function"
      || typeof pipeTo !== "function") {
    throw new TypeError("The stream transfer helper is missing its constructors");
  }

  function noop() {}

  function dataCloneError() {
    return new DOMExceptionCtor("The object could not be cloned.", "DataCloneError");
  }

  function pack(port, type, value) {
    const message = objectCreate(null);
    message.type = type;
    message.value = value;
    reflectApply(portPostMessage, port, [message]);
  }

  function packError(port, reason) {
    try {
      pack(port, "error", reason);
    } catch (error) {
      void error;
    }
  }

  function packHandling(port, type, value) {
    try {
      pack(port, type, value);
      return null;
    } catch (error) {
      packError(port, error);
      return error;
    }
  }

  function pending() {
    let resolvePromise;
    let rejectPromise;
    const promise = new PromiseCtor((resolve, reject) => {
      resolvePromise = resolve;
      rejectPromise = reject;
    });
    return { promise, resolve: resolvePromise, reject: rejectPromise };
  }

  function resolvedBackpressure() {
    return {
      promise: reflectApply(promiseResolve, PromiseCtor, [undefined]),
      resolve() {},
      reject() {},
    };
  }

  function closePort(port) {
    try {
      reflectApply(portClose, port, []);
    } catch (error) {
      void error;
    }
  }

  function listen(port, onMessage, onError) {
    reflectApply(addEventListener, port, ["message", onMessage]);
    reflectApply(addEventListener, port, ["messageerror", onError]);
    reflectApply(portStart, port, []);
  }

  function readMessage(event) {
    let data;
    try {
      data = event.data;
    } catch (error) {
      return { failed: error };
    }
    if (data === null || (typeof data !== "object" && typeof data !== "function")) {
      return { ignore: true };
    }
    let type;
    let value;
    try {
      type = data.type;
      value = data.value;
    } catch (error) {
      return { failed: error };
    }
    if (typeof type !== "string") {
      return { ignore: true };
    }
    return { type, value };
  }

  // The cross stream is not started while export runs, so its pull, close,
  // and abort posts wait for a later microtask. The port has moved by then.
  // bind* receives a controller that already exists. ReadHostObject cannot
  // construct the stream, so the shell is armed after the clone leaves V8.
  function writableAlgorithms(port, given) {
    let backpressure = pending();
    let controller = given;
    const early = [];
    let queuedError = null;

    function releaseBackpressure() {
      if (backpressure === null) {
        return;
      }
      const resolve = backpressure.resolve;
      backpressure = null;
      resolve(undefined);
    }

    function rejectBackpressure(error) {
      if (backpressure === null) {
        return;
      }
      const reject = backpressure.reject;
      backpressure = null;
      reject(error);
    }

    function errorController(reason) {
      if (controller === null) {
        queuedError = reason;
        return;
      }
      try {
        reflectApply(writableError, controller, [reason]);
      } catch (error) {
        void error;
      }
    }

    function apply(message) {
      if (message.type === "pull") {
        releaseBackpressure();
        return;
      }
      if (message.type === "error") {
        errorController(message.value);
        releaseBackpressure();
      }
    }

    function flush() {
      const queued = early.splice(0, early.length);
      for (let index = 0; index < queued.length; index += 1) {
        apply(queued[index]);
      }
      if (queuedError !== null) {
        const reason = queuedError;
        queuedError = null;
        errorController(reason);
      }
    }

    function fail(error) {
      errorController(error);
      rejectBackpressure(error);
      closePort(port);
    }

    function onMessage(event) {
      let message;
      try {
        message = readMessage(event);
      } catch (error) {
        fail(error);
        return;
      }
      if (message.failed) {
        fail(message.failed);
        return;
      }
      if (message.ignore) {
        return;
      }
      if (controller === null) {
        early.push(message);
        return;
      }
      try {
        apply(message);
      } catch (error) {
        fail(error);
      }
    }

    function onError() {
      const error = dataCloneError();
      packError(port, error);
      errorController(error);
      closePort(port);
    }

    if (controller !== null) {
      flush();
    }
    listen(port, onMessage, onError);
    return {
      start(controllerArg) {
        controller = controllerArg;
        flush();
      },
      write(chunk) {
        if (backpressure === null) {
          backpressure = resolvedBackpressure();
        }
        const gate = backpressure.promise;
        return reflectApply(promiseThen, gate, [function () {
          backpressure = pending();
          try {
            pack(port, "chunk", chunk);
          } catch (error) {
            packError(port, error);
            closePort(port);
            return reflectApply(promiseReject, PromiseCtor, [error]);
          }
        }]);
      },
      close() {
        try {
          pack(port, "close", undefined);
        } catch (error) {
          void error;
        }
        closePort(port);
      },
      abort(reason) {
        const failed = packHandling(port, "error", reason);
        closePort(port);
        if (failed !== null) {
          return reflectApply(promiseReject, PromiseCtor, [failed]);
        }
      }
    };
  }

  function crossWritable(port) {
    const side = writableAlgorithms(port, null);
    return new WritableStreamCtor({
      start(controllerArg) {
        side.start(controllerArg);
      },
      write(chunk) {
        return side.write(chunk);
      },
      close() {
        return side.close();
      },
      abort(reason) {
        return side.abort(reason);
      }
    }, {
      highWaterMark: 1,
      size() {
        return 1;
      }
    });
  }

  function bindWritable(port, controller) {
    const side = writableAlgorithms(port, controller);
    return {
      write: side.write,
      close: side.close,
      abort: side.abort
    };
  }

  function readableAlgorithms(port, given) {
    let controller = given;
    const early = [];

    function errorController(reason) {
      if (controller === null) {
        early.push({ type: "error", value: reason });
        return;
      }
      try {
        reflectApply(readableError, controller, [reason]);
      } catch (error) {
        void error;
      }
    }

    function apply(message) {
      if (message.type === "chunk") {
        try {
          reflectApply(readableEnqueue, controller, [message.value]);
        } catch (error) {
          void error;
        }
        return;
      }
      if (message.type === "close") {
        try {
          reflectApply(readableClose, controller, []);
        } catch (error) {
          void error;
        }
        closePort(port);
        return;
      }
      if (message.type === "error") {
        errorController(message.value);
        closePort(port);
      }
    }

    function flush() {
      const queued = early.splice(0, early.length);
      for (let index = 0; index < queued.length; index += 1) {
        apply(queued[index]);
      }
    }

    function fail(error) {
      errorController(error);
      closePort(port);
    }

    function onMessage(event) {
      let message;
      try {
        message = readMessage(event);
      } catch (error) {
        fail(error);
        return;
      }
      if (message.failed) {
        fail(message.failed);
        return;
      }
      if (message.ignore) {
        return;
      }
      if (controller === null) {
        early.push(message);
        return;
      }
      try {
        apply(message);
      } catch (error) {
        fail(error);
      }
    }

    function onError() {
      const error = dataCloneError();
      packError(port, error);
      errorController(error);
      closePort(port);
    }

    if (controller !== null) {
      flush();
    }
    listen(port, onMessage, onError);
    return {
      start(controllerArg) {
        controller = controllerArg;
        flush();
      },
      pull() {
        pack(port, "pull", undefined);
      },
      cancel(reason) {
        const failed = packHandling(port, "error", reason);
        closePort(port);
        if (failed !== null) {
          return reflectApply(promiseReject, PromiseCtor, [failed]);
        }
      }
    };
  }

  function crossReadable(port) {
    const side = readableAlgorithms(port, null);
    return new ReadableStreamCtor({
      start(controllerArg) {
        side.start(controllerArg);
      },
      pull() {
        side.pull();
      },
      cancel(reason) {
        return side.cancel(reason);
      }
    }, {
      highWaterMark: 0,
      size() {
        return 1;
      }
    });
  }

  function bindReadable(port, controller) {
    const side = readableAlgorithms(port, controller);
    return {
      pull: side.pull,
      cancel: side.cancel
    };
  }

  function handled(promise) {
    reflectApply(promiseThen, promise, [noop, noop]);
  }

  function exportReadable(stream) {
    const channel = new MessageChannelCtor();
    const writable = crossWritable(channel.port1);
    handled(reflectApply(pipeTo, stream, [writable]));
    return channel.port2;
  }

  function exportWritable(stream) {
    const channel = new MessageChannelCtor();
    const readable = crossReadable(channel.port1);
    handled(reflectApply(pipeTo, readable, [stream]));
    return channel.port2;
  }

  return {
    exportReadable: exportReadable,
    exportWritable: exportWritable,
    bindReadable: bindReadable,
    bindWritable: bindWritable
  };
})()"#;
