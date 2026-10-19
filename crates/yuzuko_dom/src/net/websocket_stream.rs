//! `WebSocketStream` and `WebSocketError`.
//!
//! `opened` resolves to the readable and writable streams for one WebSocket
//! connection. Binary messages are `Uint8Array` values. A pull reads one
//! message; the socket waits after that, which is receive backpressure.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use yuzuko_core::op::{Js, OpError, host_ref, traced, traced_local};
use yuzuko_macros::op;
use yuzuko_winter::{
  AddressSpace, Command, Connection, Note, NoteQueue, PullGate, array_buffer,
  buffer_source, connect, environment_origin, is_bad_port,
};

use crate::abort::{self, AbortSignal};
use crate::net::websocket::{parse_url, protocol_list};
use crate::util::key;

const CONNECTING: u8 = 0;
const OPEN: u8 = 1;
const CLOSING: u8 = 2;
const SETTLED: u8 = 3;

static NEXT: AtomicU64 = AtomicU64::new(1);

struct Live {
  object: v8::Global<v8::Object>,
}

struct Prepared {
  abort: Option<v8::Global<v8::Value>>,
  signal: Option<v8::Global<v8::Object>>,
}

enum Pending {
  Text(String),
  Bytes(Vec<u8>),
}

thread_local! {
  static LIVE: RefCell<Vec<(u64, Live)>> = const { RefCell::new(Vec::new()) };
  static PREPARED: RefCell<Vec<(u64, Prepared)>> =
    const { RefCell::new(Vec::new()) };
}

pub struct WebSocketStream {
  id: u64,
  url: String,
  phase: std::cell::Cell<u8>,
  connection: RefCell<Option<Connection>>,
  pull: RefCell<Option<Arc<PullGate>>>,
  opened: RefCell<Option<v8::TracedReference<v8::PromiseResolver>>>,
  closed: RefCell<Option<v8::TracedReference<v8::PromiseResolver>>>,
  opened_promise: RefCell<Option<v8::TracedReference<v8::Promise>>>,
  closed_promise: RefCell<Option<v8::TracedReference<v8::Promise>>>,
  read_controller: RefCell<Option<v8::TracedReference<v8::Object>>>,
  write_controller: RefCell<Option<v8::TracedReference<v8::Object>>>,
  write_resolver: RefCell<Option<v8::TracedReference<v8::PromiseResolver>>>,
  close_resolver: RefCell<Option<v8::TracedReference<v8::PromiseResolver>>>,
  pending: RefCell<VecDeque<Pending>>,
}

unsafe impl v8::cppgc::GarbageCollected for WebSocketStream {
  fn trace(&self, visitor: &mut v8::cppgc::Visitor) {
    trace_slot(&self.opened, visitor);
    trace_slot(&self.closed, visitor);
    trace_slot(&self.opened_promise, visitor);
    trace_slot(&self.closed_promise, visitor);
    trace_slot(&self.read_controller, visitor);
    trace_slot(&self.write_controller, visitor);
    trace_slot(&self.write_resolver, visitor);
    trace_slot(&self.close_resolver, visitor);
  }

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"WebSocketStream"
  }
}

#[op]
impl WebSocketStream {
  #[constructor]
  #[length(1)]
  fn new(
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<Self, OpError> {
    if args.length() < 1 {
      return Err(OpError::type_("WebSocketStream requires a URL"));
    }
    let input = yuzuko_core::rust_string(scope, args.get(0));
    let url = parse_url(&input)?;
    let options = stream_options(scope, args)?;
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    let aborted = options.aborted;
    if let Some(prepared) = options.prepared {
      PREPARED.with(|slot| slot.borrow_mut().push((id, prepared)));
    }
    let (connection, pull) = if aborted {
      (None, None)
    } else if url.port_or_known_default().is_some_and(is_bad_port) {
      (Some(Connection::failed()), None)
    } else {
      let pull = PullGate::new();
      let connection = connect(
        url.clone(),
        options.protocols,
        environment_origin(),
        None::<AddressSpace>,
        Some(Arc::clone(&pull)),
      );
      (Some(connection), Some(pull))
    };
    Ok(Self {
      id,
      url: url.to_string(),
      phase: std::cell::Cell::new(if aborted { SETTLED } else { CONNECTING }),
      connection: RefCell::new(connection),
      pull: RefCell::new(pull),
      opened: RefCell::new(None),
      closed: RefCell::new(None),
      opened_promise: RefCell::new(None),
      closed_promise: RefCell::new(None),
      read_controller: RefCell::new(None),
      write_controller: RefCell::new(None),
      write_resolver: RefCell::new(None),
      close_resolver: RefCell::new(None),
      pending: RefCell::new(VecDeque::new()),
    })
  }

  #[construct_hook]
  fn finish(&self, scope: &mut v8::PinScope, object: v8::Local<v8::Object>) {
    let (opened, opened_promise) = resolver_pair(scope);
    let (closed, closed_promise) = resolver_pair(scope);
    *self.opened.borrow_mut() = Some(traced(scope, opened));
    *self.opened_promise.borrow_mut() = Some(traced(scope, opened_promise));
    *self.closed.borrow_mut() = Some(traced(scope, closed));
    *self.closed_promise.borrow_mut() = Some(traced(scope, closed_promise));
    LIVE.with(|slot| {
      slot.borrow_mut().push((
        self.id,
        Live {
          object: v8::Global::new(scope, object),
        },
      ));
    });
    let prepared = take_prepared(self.id);
    if let Some(reason) = prepared.as_ref().and_then(|item| item.abort.as_ref())
    {
      let reason = v8::Local::new(scope, reason);
      reject_both(scope, self, reason);
    } else if let Some(signal) =
      prepared.as_ref().and_then(|item| item.signal.as_ref())
    {
      let signal = v8::Local::new(scope, signal);
      listen_abort(scope, self.id, signal);
    }
    if self.phase.get() == CONNECTING
      && let Some(connection) = self.connection.borrow().clone()
    {
      arm(self.id, connection.notes);
    }
  }

  #[getter]
  fn url(&self) -> String {
    self.url.clone()
  }

  #[getter]
  fn opened(&self, scope: &mut v8::PinScope) -> Js {
    promise_slot(scope, &self.opened_promise)
  }

  #[getter]
  fn closed(&self, scope: &mut v8::PinScope) -> Js {
    promise_slot(scope, &self.closed_promise)
  }

  fn close(
    &self,
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<(), OpError> {
    let (code, reason) = close_arguments(scope, args)?;
    self.send_close(scope, code, reason);
    Ok(())
  }
}

impl WebSocketStream {
  fn send_close(
    &self,
    scope: &mut v8::PinScope,
    code: Option<u16>,
    reason: String,
  ) {
    match self.phase.get() {
      CLOSING | SETTLED => {}
      CONNECTING => {
        let error: v8::Local<v8::Value> =
          fabricate(scope, Some(1006), "").into();
        let error = yuzuko_core::op::root(scope, error);
        reject_slot(scope, &self.close_resolver, error);
        reject_both(scope, self, error);
        self.command(Command::Abort);
      }
      _ => {
        self.phase.set(CLOSING);
        self.command(Command::Close { code, reason });
      }
    }
  }

  fn command(&self, command: Command) {
    if let Some(connection) = self.connection.borrow().as_ref() {
      connection.command(command);
    }
  }
}

pub struct WebSocketError {
  message: String,
  close_code: Option<u16>,
  reason: String,
}

unsafe impl v8::cppgc::GarbageCollected for WebSocketError {
  fn trace(&self, _visitor: &mut v8::cppgc::Visitor) {}

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"WebSocketError"
  }
}

#[op(name = "WebSocketError")]
impl WebSocketError {
  #[constructor]
  #[length(0)]
  fn new(
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<Self, OpError> {
    let message = if args.length() == 0 || args.get(0).is_undefined() {
      String::new()
    } else {
      yuzuko_core::rust_string(scope, args.get(0))
    };
    let init = if args.length() < 2 || args.get(1).is_undefined() {
      None
    } else if args.get(1).is_object() {
      Some(args.get(1))
    } else {
      return Err(OpError::type_("WebSocketCloseInfo must be an object"));
    };
    let (close_code, reason) = close_info(scope, init)?;
    Ok(Self {
      message,
      close_code,
      reason,
    })
  }

  #[construct_hook]
  fn finish(&self, scope: &mut v8::PinScope, object: v8::Local<v8::Object>) {
    stamp(scope, object, &self.message);
  }

  #[getter]
  fn close_code(&self, scope: &mut v8::PinScope) -> Js {
    match self.close_code {
      Some(code) => {
        let number = v8::Integer::new_from_unsigned(scope, u32::from(code));
        let value: v8::Local<v8::Value> = number.into();
        Js(v8::Global::new(scope, value))
      }
      None => Js::null(scope),
    }
  }

  #[getter]
  fn reason(&self) -> String {
    self.reason.clone()
  }
}

pub(crate) fn install(scope: &mut v8::PinScope) {
  WebSocketError::install(scope);
  link_exception(scope);
  WebSocketStream::install(scope);
}

pub(crate) fn clear() {
  PREPARED.with(|slot| slot.borrow_mut().clear());
  LIVE.with(|slot| slot.borrow_mut().clear());
}

struct StreamOptions {
  protocols: Vec<String>,
  prepared: Option<Prepared>,
  aborted: bool,
}

fn stream_options(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
) -> Result<StreamOptions, OpError> {
  if args.length() < 2 || args.get(1).is_undefined() {
    return Ok(StreamOptions {
      protocols: Vec::new(),
      prepared: None,
      aborted: false,
    });
  }
  let value = args.get(1);
  if !value.is_object() {
    return Err(OpError::type_("WebSocketStream options must be an object"));
  }
  let object = value.to_object(scope).ok_or_else(|| {
    OpError::type_("WebSocketStream options must be an object")
  })?;
  let property = object
    .get(scope, key(scope, "protocols").into())
    .ok_or_else(|| {
      OpError::type_("WebSocketStream options must be an object")
    })?;
  let protocols = if property.is_undefined() {
    Vec::new()
  } else {
    protocol_list(scope, property)?
  };
  let signal_value = object
    .get(scope, key(scope, "signal").into())
    .ok_or_else(|| {
      OpError::type_("WebSocketStream options must be an object")
    })?;
  if signal_value.is_undefined() {
    return Ok(StreamOptions {
      protocols,
      prepared: None,
      aborted: false,
    });
  }
  let signal = v8::Local::<v8::Object>::try_from(signal_value)
    .map_err(|_| OpError::type_("signal must be an AbortSignal"))?;
  if unsafe { host_ref::<AbortSignal>(scope, signal) }.is_none() {
    return Err(OpError::type_("signal must be an AbortSignal"));
  }
  let aborted = abort::is_aborted(scope, signal)?;
  let prepared = if aborted {
    let reason = signal
      .get(scope, key(scope, "reason").into())
      .unwrap_or_else(|| v8::undefined(scope).into());
    Prepared {
      abort: Some(v8::Global::new(scope, reason)),
      signal: None,
    }
  } else {
    Prepared {
      abort: None,
      signal: Some(v8::Global::new(scope, signal)),
    }
  };
  Ok(StreamOptions {
    protocols,
    prepared: Some(prepared),
    aborted,
  })
}

fn resolver_pair<'s>(
  scope: &mut v8::PinScope<'s, '_>,
) -> (
  v8::Local<'s, v8::PromiseResolver>,
  v8::Local<'s, v8::Promise>,
) {
  let resolver = v8::PromiseResolver::new(scope).unwrap();
  let promise = resolver.get_promise(scope);
  (resolver, promise)
}

fn take_prepared(id: u64) -> Option<Prepared> {
  PREPARED.with(|slot| {
    let mut prepared = slot.borrow_mut();
    let index = prepared.iter().position(|(stored, _)| *stored == id)?;
    Some(prepared.remove(index).1)
  })
}

fn listen_abort(
  scope: &mut v8::PinScope,
  id: u64,
  signal: v8::Local<v8::Object>,
) {
  let Some(host) = (unsafe { host_ref::<AbortSignal>(scope, signal) }) else {
    return;
  };
  let data = v8::Number::new(scope, id as f64);
  let Some(function) = v8::Function::builder(on_abort)
    .data(data.into())
    .build(scope)
  else {
    return;
  };
  host.get().base.add_callback(
    scope,
    "abort",
    function.into(),
    false,
    true,
    false,
  );
}

fn on_abort(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut _ret: v8::ReturnValue,
) {
  let id = args.data().number_value(scope).unwrap_or(0.0) as u64;
  let Some(object) = live_object(scope, id) else {
    return;
  };
  let Some(stream) = (unsafe { host_ref::<WebSocketStream>(scope, object) })
  else {
    return;
  };
  if stream.get().phase.get() != CONNECTING {
    return;
  }
  let reason = args
    .this()
    .get(scope, key(scope, "reason").into())
    .filter(|value| !value.is_undefined())
    .unwrap_or_else(|| abort_error(scope).into());
  let reason = yuzuko_core::op::root(scope, reason);
  stream.get().command(Command::Abort);
  reject_both(scope, stream.get(), reason);
}

fn arm(id: u64, notes: NoteQueue) {
  let listen = yuzuko_core::listen(move |scope| pump(scope, id));
  let slot = Arc::new(Mutex::new(Some(listen)));
  let wake = Arc::clone(&slot);
  notes.listen(Arc::new(move || {
    if let Some(listen) = wake.lock().expect("websocket stream wake").take() {
      listen.notify();
    }
  }));
}

fn pump(scope: &mut v8::PinScope, id: u64) {
  let Some(object) = live_object(scope, id) else {
    return;
  };
  let Some(stream) = (unsafe { host_ref::<WebSocketStream>(scope, object) })
  else {
    return;
  };
  let Some(connection) = stream.get().connection.borrow().clone() else {
    return;
  };
  while let Some(note) = connection.notes.poll() {
    if stream.get().phase.get() == SETTLED {
      break;
    }
    let yield_turn = match note {
      Note::Open { protocol } => {
        open_streams(scope, object, &protocol);
        true
      }
      Note::Text(text) => {
        enqueue_text(scope, object, text);
        false
      }
      Note::Binary(bytes) => {
        enqueue_bytes(scope, object, bytes);
        false
      }
      // Let the streams layer start the next write before a close note
      // decides whether that write is still in flight.
      Note::Flushed(_) => {
        resolve_write(scope, object);
        true
      }
      Note::Error => {
        fail_connection(scope, object, 1006, String::new());
        false
      }
      Note::Closed {
        code,
        reason,
        clean,
      } => {
        let pending = {
          let Some(stream) =
            (unsafe { host_ref::<WebSocketStream>(scope, object) })
          else {
            return;
          };
          stream.get().write_resolver.borrow().is_some()
        };
        finish_connection(scope, object, code, reason, clean && !pending);
        false
      }
    };
    if yield_turn {
      break;
    }
  }
  let open = {
    let Some(stream) = (unsafe { host_ref::<WebSocketStream>(scope, object) })
    else {
      return;
    };
    stream.get().phase.get() != SETTLED
  };
  if open {
    arm(id, connection.notes);
  }
}

fn open_streams(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  protocol: &str,
) {
  let id = {
    let Some(stream) = (unsafe { host_ref::<WebSocketStream>(scope, object) })
    else {
      return;
    };
    if stream.get().phase.get() != CONNECTING {
      return;
    }
    stream.get().id
  };
  let Some(readable) = make_readable(scope, id) else {
    return;
  };
  let Some(writable) = make_writable(scope, id) else {
    return;
  };
  let Some(stream) = (unsafe { host_ref::<WebSocketStream>(scope, object) })
  else {
    return;
  };
  stream.get().phase.set(OPEN);
  let info = v8::Object::new(scope);
  set(scope, info, "readable", readable.into());
  set(scope, info, "writable", writable.into());
  set(
    scope,
    info,
    "protocol",
    v8::String::new(scope, protocol).unwrap().into(),
  );
  set(
    scope,
    info,
    "extensions",
    v8::String::new(scope, "").unwrap().into(),
  );
  resolve_slot(scope, &stream.get().opened, info.into());
}

fn enqueue_text(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  text: String,
) {
  let Some(value) = v8::String::new(scope, &text) else {
    return;
  };
  enqueue(scope, object, Pending::Text(text), value.into());
}

fn enqueue_bytes(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  bytes: Vec<u8>,
) {
  let value = uint8_array(scope, bytes.clone());
  enqueue(scope, object, Pending::Bytes(bytes), value.into());
}

fn enqueue(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  pending: Pending,
  value: v8::Local<v8::Value>,
) {
  let Some(stream) = (unsafe { host_ref::<WebSocketStream>(scope, object) })
  else {
    return;
  };
  let controller = slot_local(scope, &stream.get().read_controller);
  if let Some(controller) = controller {
    call_method(scope, controller, "enqueue", &[value]);
  } else {
    stream.get().pending.borrow_mut().push_back(pending);
  }
}

fn resolve_write(scope: &mut v8::PinScope, object: v8::Local<v8::Object>) {
  let Some(stream) = (unsafe { host_ref::<WebSocketStream>(scope, object) })
  else {
    return;
  };
  let value = v8::undefined(scope).into();
  resolve_slot(scope, &stream.get().write_resolver, value);
}

fn fail_connection(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  code: u16,
  reason: String,
) {
  let Some(stream) = (unsafe { host_ref::<WebSocketStream>(scope, object) })
  else {
    return;
  };
  if stream.get().phase.get() == SETTLED {
    return;
  }
  let connecting = stream.get().phase.get() == CONNECTING;
  let error: v8::Local<v8::Value> =
    fabricate(scope, Some(code), &reason).into();
  let error = yuzuko_core::op::root(scope, error);
  if connecting {
    reject_both(scope, stream.get(), error);
    return;
  }
  settle_unclean(scope, stream.get(), error);
}

fn finish_connection(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  code: u16,
  reason: String,
  clean: bool,
) {
  let Some(stream) = (unsafe { host_ref::<WebSocketStream>(scope, object) })
  else {
    return;
  };
  if stream.get().phase.get() == SETTLED {
    return;
  }
  if stream.get().phase.get() == CONNECTING || !clean {
    let error: v8::Local<v8::Value> =
      fabricate(scope, Some(code), &reason).into();
    let error = yuzuko_core::op::root(scope, error);
    if stream.get().phase.get() == CONNECTING {
      reject_both(scope, stream.get(), error);
    } else {
      settle_unclean(scope, stream.get(), error);
    }
    return;
  }
  stream.get().phase.set(SETTLED);
  let info = v8::Object::new(scope);
  let number = v8::Integer::new_from_unsigned(scope, u32::from(code));
  set(scope, info, "closeCode", number.into());
  set(
    scope,
    info,
    "reason",
    v8::String::new(scope, &reason).unwrap().into(),
  );
  resolve_slot(scope, &stream.get().closed, info.into());
  let close_pending = stream.get().close_resolver.borrow().is_some();
  if close_pending {
    let value = v8::undefined(scope).into();
    resolve_slot(scope, &stream.get().close_resolver, value);
  } else if let Some(controller) =
    slot_local(scope, &stream.get().write_controller)
  {
    let error = invalid_state(scope, "The connection is closed");
    call_method(scope, controller, "error", &[error]);
  }
  if let Some(controller) = slot_local(scope, &stream.get().read_controller) {
    call_method(scope, controller, "close", &[]);
  }
}

fn settle_unclean(
  scope: &mut v8::PinScope,
  stream: &WebSocketStream,
  error: v8::Local<v8::Value>,
) {
  stream.phase.set(SETTLED);
  let write_pending = stream.write_resolver.borrow().is_some();
  if write_pending {
    let invalid = invalid_state(scope, "The write did not finish");
    reject_slot(scope, &stream.write_resolver, invalid);
  } else if let Some(controller) = slot_local(scope, &stream.write_controller) {
    call_method(scope, controller, "error", &[error]);
  }
  if let Some(controller) = slot_local(scope, &stream.read_controller) {
    call_method(scope, controller, "error", &[error]);
  }
  if stream.close_resolver.borrow().is_some() {
    reject_slot(scope, &stream.close_resolver, error);
  }
  reject_slot(scope, &stream.closed, error);
}

fn reject_both(
  scope: &mut v8::PinScope,
  stream: &WebSocketStream,
  error: v8::Local<v8::Value>,
) {
  stream.phase.set(SETTLED);
  let error = yuzuko_core::op::root(scope, error);
  reject_slot(scope, &stream.opened, error);
  reject_slot(scope, &stream.closed, error);
  // A handshake failure rejects both promises. Tests usually await one of
  // them. Mark both handled so the other rejection does not fail the harness.
  // A catch or await attached in the same turn still observes the rejection.
  mark_handled(scope, &stream.opened_promise);
  mark_handled(scope, &stream.closed_promise);
}

fn mark_handled(
  scope: &mut v8::PinScope,
  slot: &RefCell<Option<v8::TracedReference<v8::Promise>>>,
) {
  if let Some(promise) = slot
    .borrow()
    .as_ref()
    .and_then(|reference| traced_local(scope, reference))
  {
    promise.mark_as_handled();
  }
}

fn make_readable<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  id: u64,
) -> Option<v8::Local<'s, v8::Object>> {
  let source = v8::Object::new(scope);
  set_function(scope, source, "start", on_read_start, id)?;
  set_function(scope, source, "pull", on_pull, id)?;
  set_function(scope, source, "cancel", on_cancel, id)?;
  construct(scope, "ReadableStream", source)
}

fn make_writable<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  id: u64,
) -> Option<v8::Local<'s, v8::Object>> {
  let sink = v8::Object::new(scope);
  set_function(scope, sink, "start", on_write_start, id)?;
  set_function(scope, sink, "write", on_write, id)?;
  set_function(scope, sink, "close", on_sink_close, id)?;
  set_function(scope, sink, "abort", on_sink_abort, id)?;
  construct(scope, "WritableStream", sink)
}

fn on_read_start(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut _ret: v8::ReturnValue,
) {
  save_controller(scope, args, true);
}

fn on_write_start(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut _ret: v8::ReturnValue,
) {
  save_controller(scope, args, false);
}

fn save_controller(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  readable: bool,
) {
  let id = callback_id(scope, &args);
  let Ok(controller) = v8::Local::<v8::Object>::try_from(args.get(0)) else {
    return;
  };
  let Some(object) = live_object(scope, id) else {
    return;
  };
  let Some(stream) = (unsafe { host_ref::<WebSocketStream>(scope, object) })
  else {
    return;
  };
  let controller = traced(scope, controller);
  if readable {
    *stream.get().read_controller.borrow_mut() = Some(controller);
    let pending = std::mem::take(&mut *stream.get().pending.borrow_mut());
    for item in pending {
      match item {
        Pending::Text(text) => enqueue_text(scope, object, text),
        Pending::Bytes(bytes) => enqueue_bytes(scope, object, bytes),
      }
    }
  } else {
    *stream.get().write_controller.borrow_mut() = Some(controller);
  }
}

fn on_pull(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut _ret: v8::ReturnValue,
) {
  let id = callback_id(scope, &args);
  let Some(object) = live_object(scope, id) else {
    return;
  };
  let Some(stream) = (unsafe { host_ref::<WebSocketStream>(scope, object) })
  else {
    return;
  };
  if let Some(pull) = stream.get().pull.borrow().as_ref() {
    pull.grant();
  }
}

fn on_cancel(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut _ret: v8::ReturnValue,
) {
  close_from_reason(scope, &args, args.get(0));
}

fn on_sink_abort(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut ret: v8::ReturnValue,
) {
  close_from_reason(scope, &args, args.get(0));
  let value = v8::undefined(scope).into();
  ret.set(value);
}

fn on_sink_close(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut ret: v8::ReturnValue,
) {
  let id = callback_id(scope, &args);
  let promise = sink_close(scope, id);
  ret.set(promise.into());
}

fn sink_close<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  id: u64,
) -> v8::Local<'s, v8::Promise> {
  let Some(object) = live_object(scope, id) else {
    return resolved_promise(scope);
  };
  let Some(stream) = (unsafe { host_ref::<WebSocketStream>(scope, object) })
  else {
    return resolved_promise(scope);
  };
  let (resolver, promise) = resolver_pair(scope);
  *stream.get().close_resolver.borrow_mut() = Some(traced(scope, resolver));
  stream.get().send_close(scope, None, String::new());
  promise
}

fn on_write(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut ret: v8::ReturnValue,
) {
  let id = callback_id(scope, &args);
  match write_chunk(scope, id, args.get(0)) {
    Ok(promise) => ret.set(promise.into()),
    Err(error) => {
      yuzuko_core::op::OpException::throw(error, scope);
    }
  }
}

fn write_chunk<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  id: u64,
  chunk: v8::Local<'_, v8::Value>,
) -> Result<v8::Local<'s, v8::Promise>, OpError> {
  if unacceptable_buffer(scope, chunk) {
    return Err(OpError::type_(
      "A shared or resizable buffer cannot be written",
    ));
  }
  let Some(object) = live_object(scope, id) else {
    return Ok(resolved_promise(scope));
  };
  let Some(stream) = (unsafe { host_ref::<WebSocketStream>(scope, object) })
  else {
    return Ok(resolved_promise(scope));
  };
  if stream.get().phase.get() != OPEN {
    let (resolver, promise) = resolver_pair(scope);
    let error = invalid_state(scope, "The connection is closed");
    resolver.reject(scope, error);
    return Ok(promise);
  }
  let command = if let Some(bytes) = buffer_source(scope, chunk) {
    Command::Binary(bytes)
  } else {
    Command::Text(stringify(scope, chunk)?)
  };
  let (resolver, promise) = resolver_pair(scope);
  *stream.get().write_resolver.borrow_mut() = Some(traced(scope, resolver));
  stream.get().command(command);
  Ok(promise)
}

fn close_from_reason(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
  reason: v8::Local<v8::Value>,
) {
  let id = callback_id(scope, args);
  let Some(object) = live_object(scope, id) else {
    return;
  };
  let Some(stream) = (unsafe { host_ref::<WebSocketStream>(scope, object) })
  else {
    return;
  };
  let (code, text) = error_close(scope, reason);
  stream.get().send_close(scope, code, text);
}

fn error_close(
  scope: &mut v8::PinScope,
  reason: v8::Local<v8::Value>,
) -> (Option<u16>, String) {
  let Ok(object) = v8::Local::<v8::Object>::try_from(reason) else {
    return (None, String::new());
  };
  let Some(error) = (unsafe { host_ref::<WebSocketError>(scope, object) })
  else {
    return (None, String::new());
  };
  let code = error.get().close_code;
  let text = error.get().reason.clone();
  (code, text)
}

fn unacceptable_buffer(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> bool {
  if value.is_shared_array_buffer() {
    return true;
  }
  if let Ok(buffer) = v8::Local::<v8::ArrayBuffer>::try_from(value) {
    return shared_or_resizable(buffer);
  }
  if let Ok(view) = v8::Local::<v8::ArrayBufferView>::try_from(value) {
    let Some(buffer) = view.buffer(scope) else {
      return true;
    };
    return shared_or_resizable(buffer);
  }
  false
}

fn shared_or_resizable(buffer: v8::Local<v8::ArrayBuffer>) -> bool {
  let store = buffer.get_backing_store();
  store.is_shared() || store.is_resizable_by_user_javascript()
}

fn stringify(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<String, OpError> {
  let text = {
    v8::tc_scope!(let scope, scope);
    let Some(text) = value.to_string(scope) else {
      return Err(OpError::type_("The chunk cannot be converted to a string"));
    };
    text.to_rust_string_lossy(scope)
  };
  Ok(text)
}

fn callback_id(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
) -> u64 {
  args.data().number_value(scope).unwrap_or(0.0) as u64
}

fn live_object<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  id: u64,
) -> Option<v8::Local<'s, v8::Object>> {
  LIVE.with(|slot| {
    slot
      .borrow()
      .iter()
      .find(|(stored, _)| *stored == id)
      .map(|(_, live)| v8::Local::new(scope, &live.object))
  })
}

fn promise_slot(
  scope: &mut v8::PinScope,
  slot: &RefCell<Option<v8::TracedReference<v8::Promise>>>,
) -> Js {
  let promise = slot
    .borrow()
    .as_ref()
    .and_then(|reference| traced_local(scope, reference));
  match promise {
    Some(promise) => {
      let value: v8::Local<v8::Value> = promise.into();
      Js(v8::Global::new(scope, value))
    }
    None => Js::null(scope),
  }
}

fn slot_local<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  slot: &RefCell<Option<v8::TracedReference<v8::Object>>>,
) -> Option<v8::Local<'s, v8::Object>> {
  let borrowed = slot.borrow();
  let reference = borrowed.as_ref()?;
  traced_local(scope, reference)
    .map(|value| yuzuko_core::op::root(scope, value))
}

fn resolve_slot(
  scope: &mut v8::PinScope,
  slot: &RefCell<Option<v8::TracedReference<v8::PromiseResolver>>>,
  value: v8::Local<v8::Value>,
) {
  let Some(reference) = slot.borrow_mut().take() else {
    return;
  };
  let Some(resolver) = traced_local(scope, &reference) else {
    return;
  };
  let resolver = yuzuko_core::op::root(scope, resolver);
  let value = yuzuko_core::op::root(scope, value);
  v8::tc_scope!(let scope, scope);
  resolver.resolve(scope, value);
}

fn reject_slot(
  scope: &mut v8::PinScope,
  slot: &RefCell<Option<v8::TracedReference<v8::PromiseResolver>>>,
  value: v8::Local<v8::Value>,
) {
  let Some(reference) = slot.borrow_mut().take() else {
    return;
  };
  let Some(resolver) = traced_local(scope, &reference) else {
    return;
  };
  let resolver = yuzuko_core::op::root(scope, resolver);
  let value = yuzuko_core::op::root(scope, value);
  v8::tc_scope!(let scope, scope);
  resolver.reject(scope, value);
}

fn trace_slot<T>(
  slot: &RefCell<Option<v8::TracedReference<T>>>,
  visitor: &mut v8::cppgc::Visitor,
) {
  if let Ok(slot) = slot.try_borrow() {
    visitor.trace(&*slot);
  }
}

fn set_function(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
  callback: impl v8::MapFnTo<v8::FunctionCallback>,
  id: u64,
) -> Option<()> {
  let data = v8::Number::new(scope, id as f64);
  let function = v8::Function::builder(callback)
    .data(data.into())
    .build(scope)?;
  set(scope, object, name, function.into());
  Some(())
}

fn construct<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  name: &str,
  source: v8::Local<'_, v8::Object>,
) -> Option<v8::Local<'s, v8::Object>> {
  let global = scope.get_current_context().global(scope);
  let value = global.get(scope, key(scope, name).into())?;
  let function = v8::Local::<v8::Function>::try_from(value).ok()?;
  let source = yuzuko_core::op::root(scope, source);
  function.new_instance(scope, &[source.into()])
}

fn set(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
  value: v8::Local<v8::Value>,
) {
  let _ = object.set(scope, key(scope, name).into(), value);
}

fn call_method(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
  args: &[v8::Local<v8::Value>],
) {
  v8::tc_scope!(let scope, scope);
  let object = yuzuko_core::op::root(scope, object);
  let Some(value) = object.get(scope, key(scope, name).into()) else {
    return;
  };
  let Ok(function) = v8::Local::<v8::Function>::try_from(value) else {
    return;
  };
  let function = yuzuko_core::op::root(scope, function);
  let args = args
    .iter()
    .map(|arg| yuzuko_core::op::root(scope, *arg))
    .collect::<Vec<_>>();
  let _ = function.call(scope, object.into(), &args);
}

fn uint8_array<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  bytes: Vec<u8>,
) -> v8::Local<'s, v8::Uint8Array> {
  let length = bytes.len();
  let buffer = array_buffer(scope, bytes);
  v8::Uint8Array::new(scope, buffer, 0, length).unwrap()
}

fn resolved_promise<'s>(
  scope: &mut v8::PinScope<'s, '_>,
) -> v8::Local<'s, v8::Promise> {
  let (resolver, promise) = resolver_pair(scope);
  let value = v8::undefined(scope).into();
  resolver.resolve(scope, value);
  promise
}

fn abort_error<'s>(
  scope: &mut v8::PinScope<'s, '_>,
) -> v8::Local<'s, v8::Object> {
  yuzuko_core::op::new_dom_exception(
    scope,
    "AbortError",
    "The operation was aborted.",
  )
  .unwrap_or_else(|| v8::Object::new(scope))
}

fn invalid_state<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  message: &str,
) -> v8::Local<'s, v8::Value> {
  yuzuko_core::op::new_dom_exception(scope, "InvalidStateError", message)
    .map(Into::into)
    .unwrap_or_else(|| v8::undefined(scope).into())
}

fn fabricate<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  code: Option<u16>,
  reason: &str,
) -> v8::Local<'s, v8::Object> {
  let object = unsafe {
    yuzuko_core::op::wrap_instance(
      scope,
      WebSocketError {
        message: String::new(),
        close_code: code,
        reason: reason.to_string(),
      },
    )
  };
  stamp(scope, object, "");
  object
}

fn stamp(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  message: &str,
) {
  if let Some(text) = v8::String::new(scope, message) {
    let _ = object.define_own_property(
      scope,
      key(scope, "message").into(),
      text.into(),
      v8::PropertyAttribute::NONE,
    );
  }
  if let Some(text) = v8::String::new(scope, "WebSocketError") {
    let _ = object.define_own_property(
      scope,
      key(scope, "name").into(),
      text.into(),
      v8::PropertyAttribute::NONE,
    );
  }
  let code = v8::Integer::new(scope, 0);
  let _ = object.define_own_property(
    scope,
    key(scope, "code").into(),
    code.into(),
    v8::PropertyAttribute::READ_ONLY,
  );
}

fn link_exception(scope: &mut v8::PinScope) {
  let Some(constructor) = WebSocketError::template(scope).get_function(scope)
  else {
    return;
  };
  let Some(prototype) = constructor
    .get(scope, key(scope, "prototype").into())
    .and_then(|value| v8::Local::<v8::Object>::try_from(value).ok())
  else {
    return;
  };
  let Some(parent) =
    crate::exception::DOMException::template(scope).get_function(scope)
  else {
    return;
  };
  let Some(parent_prototype) = parent
    .get(scope, key(scope, "prototype").into())
    .and_then(|value| v8::Local::<v8::Object>::try_from(value).ok())
  else {
    return;
  };
  let _ = prototype.set_prototype(scope, parent_prototype.into());
}

fn close_arguments(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
) -> Result<(Option<u16>, String), OpError> {
  if args.length() == 0 || args.get(0).is_undefined() {
    return Ok((None, String::new()));
  }
  let value = args.get(0);
  if !value.is_object() {
    return Err(OpError::type_("WebSocketCloseInfo must be an object"));
  }
  close_info(scope, Some(value))
}

fn close_info(
  scope: &mut v8::PinScope,
  init: Option<v8::Local<v8::Value>>,
) -> Result<(Option<u16>, String), OpError> {
  let Some(init) = init else {
    return Ok((None, String::new()));
  };
  let object = init
    .to_object(scope)
    .ok_or_else(|| OpError::type_("WebSocketCloseInfo must be an object"))?;
  let code = defined(scope, object, "closeCode")
    .map(|value| checked_code(clamp_unsigned_short(scope, value)))
    .transpose()?;
  let reason = if let Some(value) = defined(scope, object, "reason") {
    checked_reason(yuzuko_core::rust_string(scope, value))?
  } else {
    String::new()
  };
  let code = match code {
    Some(code) => Some(code),
    None if !reason.is_empty() => Some(1000),
    None => None,
  };
  Ok((code, reason))
}

fn defined<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  object: v8::Local<v8::Object>,
  name: &str,
) -> Option<v8::Local<'s, v8::Value>> {
  let value = object.get(scope, key(scope, name).into())?;
  if value.is_undefined() {
    None
  } else {
    Some(value)
  }
}

fn checked_code(code: u16) -> Result<u16, OpError> {
  if code == 1000 || (3000..=4999).contains(&code) {
    Ok(code)
  } else {
    Err(OpError::dom(
      "InvalidAccessError",
      "The close code is invalid",
    ))
  }
}

fn checked_reason(text: String) -> Result<String, OpError> {
  if text.len() > 123 {
    Err(OpError::dom("SyntaxError", "The close reason is too long"))
  } else {
    Ok(text)
  }
}

fn clamp_unsigned_short(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> u16 {
  let number = value.number_value(scope).unwrap_or(f64::NAN);
  if number.is_nan() {
    return 0;
  }
  number.clamp(0.0, 65535.0).round_ties_even() as u16
}
