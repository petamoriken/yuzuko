//! `WebTransport` and `WebTransportError`.
//!
//! The session uses its own QUIC connection. `datagrams` is a plain
//! `{ readable, writable }`. Datagrams and stream chunks are `Uint8Array`
//! values. A pull grants one read. `reliability` stays `"pending"` until the
//! handshake note is pumped, which is after the constructor returns.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use yuzuko_core::op::{HostRef, Js, OpError, host_ref, traced, traced_local};
use yuzuko_macros::op;
use yuzuko_winter::{
  WebTransportCommand as Command, WebTransportCongestion as Congestion,
  WebTransportConnection as Connection, WebTransportNote as Note,
  WebTransportNotes as NoteQueue, WebTransportOptions as Options,
  abort_web_transports, array_buffer, buffer_source, connect_web_transport,
  next_web_transport_stream_id,
};

use crate::util::key;

const CONNECTING: u8 = 0;
const CONNECTED: u8 = 1;
const CLOSING: u8 = 2;
const SETTLED: u8 = 3;

static NEXT: AtomicU64 = AtomicU64::new(1);

struct Live {
  object: v8::Global<v8::Object>,
}

#[derive(Default)]
struct StreamSlot {
  read: Option<v8::TracedReference<v8::Object>>,
  write: Option<v8::TracedReference<v8::Object>>,
  write_resolver: Option<v8::TracedReference<v8::PromiseResolver>>,
  close_resolver: Option<v8::TracedReference<v8::PromiseResolver>>,
  pending: VecDeque<Vec<u8>>,
  read_ended: bool,
  read_error: Option<(Option<u32>, String)>,
}

struct PendingOpen {
  resolver: v8::TracedReference<v8::PromiseResolver>,
  bidirectional: bool,
}

thread_local! {
  static LIVE: RefCell<Vec<(u64, Live)>> = const { RefCell::new(Vec::new()) };
}

pub struct WebTransport {
  id: u64,
  url: String,
  phase: Cell<u8>,
  reliability: RefCell<String>,
  congestion: String,
  connection: RefCell<Option<Connection>>,
  ready: RefCell<Option<v8::TracedReference<v8::PromiseResolver>>>,
  closed: RefCell<Option<v8::TracedReference<v8::PromiseResolver>>>,
  ready_promise: RefCell<Option<v8::TracedReference<v8::Promise>>>,
  closed_promise: RefCell<Option<v8::TracedReference<v8::Promise>>>,
  datagrams: RefCell<Option<v8::TracedReference<v8::Object>>>,
  datagram_read: RefCell<Option<v8::TracedReference<v8::Object>>>,
  datagram_write: RefCell<Option<v8::TracedReference<v8::Object>>>,
  datagram_pending: RefCell<VecDeque<Vec<u8>>>,
  datagram_writes: RefCell<VecDeque<v8::TracedReference<v8::PromiseResolver>>>,
  incoming_bi: RefCell<Option<v8::TracedReference<v8::Object>>>,
  incoming_uni: RefCell<Option<v8::TracedReference<v8::Object>>>,
  incoming_bi_controller: RefCell<Option<v8::TracedReference<v8::Object>>>,
  incoming_uni_controller: RefCell<Option<v8::TracedReference<v8::Object>>>,
  streams: RefCell<HashMap<u64, StreamSlot>>,
  opens: RefCell<HashMap<u64, PendingOpen>>,
}

unsafe impl v8::cppgc::GarbageCollected for WebTransport {
  fn trace(&self, visitor: &mut v8::cppgc::Visitor) {
    trace_slot(&self.ready, visitor);
    trace_slot(&self.closed, visitor);
    trace_slot(&self.ready_promise, visitor);
    trace_slot(&self.closed_promise, visitor);
    trace_slot(&self.datagrams, visitor);
    trace_slot(&self.datagram_read, visitor);
    trace_slot(&self.datagram_write, visitor);
    trace_slot(&self.incoming_bi, visitor);
    trace_slot(&self.incoming_uni, visitor);
    trace_slot(&self.incoming_bi_controller, visitor);
    trace_slot(&self.incoming_uni_controller, visitor);
    trace_writes(&self.datagram_writes, visitor);
    trace_streams(&self.streams, visitor);
    trace_opens(&self.opens, visitor);
  }

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"WebTransport"
  }
}

#[op]
impl WebTransport {
  #[constructor]
  #[length(1)]
  fn new(
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<Self, OpError> {
    if args.length() < 1 {
      return Err(OpError::type_("WebTransport requires a URL"));
    }
    let input = yuzuko_core::rust_string(scope, args.get(0));
    let url = parse_url(&input)?;
    let parsed = parsed_options(scope, args)?;
    let connection = connect_web_transport(url.clone(), parsed.options);
    Ok(Self {
      id: NEXT.fetch_add(1, Ordering::Relaxed),
      url: url.to_string(),
      phase: Cell::new(CONNECTING),
      reliability: RefCell::new("pending".to_string()),
      congestion: parsed.congestion,
      connection: RefCell::new(Some(connection)),
      ready: RefCell::new(None),
      closed: RefCell::new(None),
      ready_promise: RefCell::new(None),
      closed_promise: RefCell::new(None),
      datagrams: RefCell::new(None),
      datagram_read: RefCell::new(None),
      datagram_write: RefCell::new(None),
      datagram_pending: RefCell::new(VecDeque::new()),
      datagram_writes: RefCell::new(VecDeque::new()),
      incoming_bi: RefCell::new(None),
      incoming_uni: RefCell::new(None),
      incoming_bi_controller: RefCell::new(None),
      incoming_uni_controller: RefCell::new(None),
      streams: RefCell::new(HashMap::new()),
      opens: RefCell::new(HashMap::new()),
    })
  }

  #[construct_hook]
  fn finish(&self, scope: &mut v8::PinScope, object: v8::Local<v8::Object>) {
    let (ready, ready_promise) = resolver_pair(scope);
    let (closed, closed_promise) = resolver_pair(scope);
    *self.ready.borrow_mut() = Some(traced(scope, ready));
    *self.ready_promise.borrow_mut() = Some(traced(scope, ready_promise));
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
    if let Some(datagrams) = make_datagrams(scope, self.id) {
      *self.datagrams.borrow_mut() = Some(traced(scope, datagrams));
    }
    if let Some(incoming) = make_incoming(scope, self.id, true) {
      *self.incoming_bi.borrow_mut() = Some(traced(scope, incoming));
    }
    if let Some(incoming) = make_incoming(scope, self.id, false) {
      *self.incoming_uni.borrow_mut() = Some(traced(scope, incoming));
    }
    if let Some(connection) = self.connection.borrow().clone() {
      arm(self.id, connection.notes);
    }
  }

  #[getter]
  fn url(&self) -> String {
    self.url.clone()
  }

  #[getter]
  fn ready(&self, scope: &mut v8::PinScope) -> Js {
    promise_slot(scope, &self.ready_promise)
  }

  #[getter]
  fn closed(&self, scope: &mut v8::PinScope) -> Js {
    promise_slot(scope, &self.closed_promise)
  }

  #[getter]
  fn datagrams(&self, scope: &mut v8::PinScope) -> Js {
    object_slot(scope, &self.datagrams)
  }

  #[getter]
  fn incoming_bidirectional_streams(&self, scope: &mut v8::PinScope) -> Js {
    object_slot(scope, &self.incoming_bi)
  }

  #[getter]
  fn incoming_unidirectional_streams(&self, scope: &mut v8::PinScope) -> Js {
    object_slot(scope, &self.incoming_uni)
  }

  #[getter]
  fn congestion_control(&self) -> String {
    self.congestion.clone()
  }

  #[getter]
  fn reliability(&self) -> String {
    self.reliability.borrow().clone()
  }

  fn close(
    &self,
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<(), OpError> {
    let (code, reason) = close_arguments(scope, args)?;
    match self.phase.get() {
      CLOSING | SETTLED => Ok(()),
      CONNECTING => {
        let error = session_error(scope, "The connection was closed");
        let error = yuzuko_core::op::root(scope, error);
        self.phase.set(SETTLED);
        reject_slot(scope, &self.ready, error);
        reject_slot(scope, &self.closed, error);
        mark_handled(scope, &self.ready_promise);
        mark_handled(scope, &self.closed_promise);
        cleanup(scope, self, error);
        self.command(Command::Abort);
        Ok(())
      }
      _ => {
        self.phase.set(CLOSING);
        self.command(Command::Close { code, reason });
        Ok(())
      }
    }
  }

  fn create_bidirectional_stream(
    &self,
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<Js, OpError> {
    reject_send_group(scope, args)?;
    self.open_stream(scope, true)
  }

  fn create_unidirectional_stream(
    &self,
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<Js, OpError> {
    reject_send_group(scope, args)?;
    self.open_stream(scope, false)
  }
}

impl WebTransport {
  fn command(&self, command: Command) {
    if let Some(connection) = self.connection.borrow().as_ref() {
      connection.command(command);
    }
  }

  fn open_stream(
    &self,
    scope: &mut v8::PinScope,
    bidirectional: bool,
  ) -> Result<Js, OpError> {
    if self.phase.get() != CONNECTING && self.phase.get() != CONNECTED {
      let error = invalid_state(scope, "The session is closed");
      return Ok(rejected(scope, error));
    }
    let id = next_web_transport_stream_id();
    let (resolver, promise) = resolver_pair(scope);
    self.opens.borrow_mut().insert(
      id,
      PendingOpen {
        resolver: traced(scope, resolver),
        bidirectional,
      },
    );
    if bidirectional {
      self.command(Command::OpenBi { id });
    } else {
      self.command(Command::OpenUni { id });
    }
    Ok(promise_js(scope, promise))
  }
}

pub struct WebTransportError {
  message: String,
  source: String,
  stream_error_code: Option<u32>,
}

unsafe impl v8::cppgc::GarbageCollected for WebTransportError {
  fn trace(&self, _visitor: &mut v8::cppgc::Visitor) {}

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"WebTransportError"
  }
}

#[op]
impl WebTransportError {
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
    let (source, stream_error_code) =
      if args.length() < 2 || args.get(1).is_undefined() {
        ("stream".to_string(), None)
      } else {
        error_options(scope, args.get(1))?
      };
    Ok(Self {
      message,
      source,
      stream_error_code,
    })
  }

  #[construct_hook]
  fn finish(&self, scope: &mut v8::PinScope, object: v8::Local<v8::Object>) {
    stamp(scope, object, &self.message);
  }

  #[getter]
  fn source(&self) -> String {
    self.source.clone()
  }

  #[getter]
  fn stream_error_code(&self, scope: &mut v8::PinScope) -> Js {
    match self.stream_error_code {
      Some(code) => {
        let number = v8::Integer::new_from_unsigned(scope, code);
        let value: v8::Local<v8::Value> = number.into();
        Js(v8::Global::new(scope, value))
      }
      None => Js::null(scope),
    }
  }
}

pub(crate) fn install(scope: &mut v8::PinScope) {
  WebTransportError::install(scope);
  link_exception(scope);
  WebTransport::install(scope);
}

pub(crate) fn clear() {
  abort_web_transports();
  LIVE.with(|slot| slot.borrow_mut().clear());
}

fn arm(id: u64, notes: NoteQueue) {
  let listen = yuzuko_core::listen(move |scope| pump(scope, id));
  let slot = Arc::new(Mutex::new(Some(listen)));
  let wake = Arc::clone(&slot);
  notes.listen(Arc::new(move || {
    if let Some(listen) = wake.lock().expect("webtransport wake").take() {
      listen.notify();
    }
  }));
}

fn pump(scope: &mut v8::PinScope, session: u64) {
  let notes = {
    let Some(transport) = live_host(scope, session) else {
      return;
    };
    let Some(connection) = transport.get().connection.borrow().clone() else {
      return;
    };
    connection.notes
  };
  while let Some(note) = notes.poll() {
    {
      let Some(transport) = live_host(scope, session) else {
        return;
      };
      if transport.get().phase.get() == SETTLED {
        break;
      }
    }
    match note {
      Note::Ready { max_datagram } => on_ready(scope, session, max_datagram),
      Note::Closed {
        code,
        reason,
        clean,
        message,
      } => {
        on_closed(scope, session, code, reason, clean, message);
        break;
      }
      Note::Datagram(bytes) => on_datagram(scope, session, bytes),
      Note::DatagramSent => on_datagram_sent(scope, session, None),
      Note::DatagramFailed(message) => {
        on_datagram_sent(scope, session, Some(message));
      }
      Note::BiOpened { id } => on_outgoing(scope, session, id, true),
      Note::UniOpened { id } => on_outgoing(scope, session, id, false),
      Note::IncomingBi { id } => on_incoming(scope, session, id, true),
      Note::IncomingUni { id } => on_incoming(scope, session, id, false),
      Note::Data { id, bytes } => on_data(scope, session, id, bytes),
      Note::End { id } => on_end(scope, session, id),
      Note::StreamError { id, code, message } => {
        on_stream_error(scope, session, id, code, message);
      }
      Note::Wrote { id } => on_wrote(scope, session, id, None),
      Note::WriteFailed { id, code, message } => {
        on_wrote(scope, session, id, Some((code, message)));
      }
      Note::Finished { id } => on_finished(scope, session, id),
    }
  }
  let open = {
    let Some(transport) = live_host(scope, session) else {
      return;
    };
    transport.get().phase.get() != SETTLED
  };
  if open {
    arm(session, notes);
  }
}

fn on_ready(scope: &mut v8::PinScope, session: u64, max_datagram: usize) {
  let Some(transport) = live_host(scope, session) else {
    return;
  };
  if transport.get().phase.get() == SETTLED {
    return;
  }
  let mode = if max_datagram == 0 {
    "reliable-only"
  } else {
    "supports-unreliable"
  };
  *transport.get().reliability.borrow_mut() = mode.to_string();
  if transport.get().phase.get() == CONNECTING {
    transport.get().phase.set(CONNECTED);
  }
  let value = v8::undefined(scope).into();
  resolve_slot(scope, &transport.get().ready, value);
}

fn on_closed(
  scope: &mut v8::PinScope,
  session: u64,
  code: u32,
  reason: String,
  clean: bool,
  message: String,
) {
  let Some(transport) = live_host(scope, session) else {
    return;
  };
  let phase = transport.get().phase.get();
  if phase == SETTLED {
    return;
  }
  transport.get().phase.set(SETTLED);
  if phase == CONNECTING {
    let text = if message.is_empty() {
      "The connection failed".to_string()
    } else {
      message
    };
    let error = session_error(scope, &text);
    let error = yuzuko_core::op::root(scope, error);
    reject_slot(scope, &transport.get().ready, error);
    reject_slot(scope, &transport.get().closed, error);
    mark_handled(scope, &transport.get().ready_promise);
    mark_handled(scope, &transport.get().closed_promise);
    cleanup(scope, transport.get(), error);
    return;
  }
  if !clean {
    let text = if message.is_empty() {
      "The connection failed".to_string()
    } else {
      message
    };
    let error = session_error(scope, &text);
    let error = yuzuko_core::op::root(scope, error);
    reject_slot(scope, &transport.get().closed, error);
    cleanup(scope, transport.get(), error);
    return;
  }
  let info = v8::Object::new(scope);
  let number = v8::Integer::new_from_unsigned(scope, code);
  set(scope, info, "closeCode", number.into());
  set(
    scope,
    info,
    "reason",
    v8::String::new(scope, &reason).unwrap().into(),
  );
  resolve_slot(scope, &transport.get().closed, info.into());
  let error = session_error(scope, "The session is closed");
  let error = yuzuko_core::op::root(scope, error);
  cleanup(scope, transport.get(), error);
}

fn on_datagram(scope: &mut v8::PinScope, session: u64, bytes: Vec<u8>) {
  let controller = {
    let Some(transport) = live_host(scope, session) else {
      return;
    };
    let controller = transport
      .get()
      .datagram_read
      .borrow()
      .as_ref()
      .and_then(|reference| traced_object(scope, reference));
    if controller.is_none() {
      transport
        .get()
        .datagram_pending
        .borrow_mut()
        .push_back(bytes);
      return;
    }
    controller
  };
  let Some(controller) = controller else {
    return;
  };
  let value = uint8_array(scope, bytes);
  call_method(scope, controller, "enqueue", &[value.into()]);
}

fn on_datagram_sent(
  scope: &mut v8::PinScope,
  session: u64,
  failed: Option<String>,
) {
  let resolver = {
    let Some(transport) = live_host(scope, session) else {
      return;
    };
    transport.get().datagram_writes.borrow_mut().pop_front()
  };
  let Some(resolver) = resolver else {
    return;
  };
  if let Some(message) = failed {
    let error = session_error(scope, &message);
    reject_moved(scope, resolver, error);
  } else {
    let value = v8::undefined(scope).into();
    resolve_moved(scope, resolver, value);
  }
}

fn on_outgoing(
  scope: &mut v8::PinScope,
  session: u64,
  id: u64,
  bidirectional: bool,
) {
  let pending = {
    let Some(transport) = live_host(scope, session) else {
      return;
    };
    transport.get().opens.borrow_mut().remove(&id)
  };
  let Some(pending) = pending else {
    return;
  };
  if pending.bidirectional != bidirectional {
    let error = session_error(scope, "The stream failed");
    reject_moved(scope, pending.resolver, error);
    return;
  }
  {
    let Some(transport) = live_host(scope, session) else {
      return;
    };
    transport
      .get()
      .streams
      .borrow_mut()
      .insert(id, StreamSlot::default());
  }
  let value = if bidirectional {
    let Some(readable) = make_stream_readable(scope, session, id) else {
      let error = session_error(scope, "The stream failed");
      reject_moved(scope, pending.resolver, error);
      return;
    };
    let Some(writable) = make_stream_writable(scope, session, id) else {
      let error = session_error(scope, "The stream failed");
      reject_moved(scope, pending.resolver, error);
      return;
    };
    let info = v8::Object::new(scope);
    set(scope, info, "readable", readable.into());
    set(scope, info, "writable", writable.into());
    info.into()
  } else {
    let Some(writable) = make_stream_writable(scope, session, id) else {
      let error = session_error(scope, "The stream failed");
      reject_moved(scope, pending.resolver, error);
      return;
    };
    writable.into()
  };
  resolve_moved(scope, pending.resolver, value);
}

fn on_incoming(
  scope: &mut v8::PinScope,
  session: u64,
  id: u64,
  bidirectional: bool,
) {
  {
    let Some(transport) = live_host(scope, session) else {
      return;
    };
    transport
      .get()
      .streams
      .borrow_mut()
      .insert(id, StreamSlot::default());
  }
  let (controller, value) = if bidirectional {
    let Some(readable) = make_stream_readable(scope, session, id) else {
      return;
    };
    let Some(writable) = make_stream_writable(scope, session, id) else {
      return;
    };
    let info = v8::Object::new(scope);
    set(scope, info, "readable", readable.into());
    set(scope, info, "writable", writable.into());
    let controller = {
      let Some(transport) = live_host(scope, session) else {
        return;
      };
      transport
        .get()
        .incoming_bi_controller
        .borrow()
        .as_ref()
        .and_then(|reference| traced_object(scope, reference))
    };
    (controller, info.into())
  } else {
    let Some(readable) = make_stream_readable(scope, session, id) else {
      return;
    };
    let controller = {
      let Some(transport) = live_host(scope, session) else {
        return;
      };
      transport
        .get()
        .incoming_uni_controller
        .borrow()
        .as_ref()
        .and_then(|reference| traced_object(scope, reference))
    };
    (controller, readable.into())
  };
  if let Some(controller) = controller {
    call_method(scope, controller, "enqueue", &[value]);
  }
}

fn on_data(scope: &mut v8::PinScope, session: u64, id: u64, bytes: Vec<u8>) {
  let controller = {
    let Some(transport) = live_host(scope, session) else {
      return;
    };
    let mut streams = transport.get().streams.borrow_mut();
    let Some(slot) = streams.get_mut(&id) else {
      return;
    };
    let controller = slot
      .read
      .as_ref()
      .and_then(|reference| traced_object(scope, reference));
    if controller.is_none() {
      slot.pending.push_back(bytes);
      return;
    }
    controller
  };
  let Some(controller) = controller else {
    return;
  };
  let value = uint8_array(scope, bytes);
  call_method(scope, controller, "enqueue", &[value.into()]);
}

fn on_end(scope: &mut v8::PinScope, session: u64, id: u64) {
  let controller = {
    let Some(transport) = live_host(scope, session) else {
      return;
    };
    let mut streams = transport.get().streams.borrow_mut();
    let Some(slot) = streams.get_mut(&id) else {
      return;
    };
    slot.read_ended = true;
    slot
      .read
      .as_ref()
      .and_then(|reference| traced_object(scope, reference))
  };
  if let Some(controller) = controller {
    call_method(scope, controller, "close", &[]);
  }
}

fn on_stream_error(
  scope: &mut v8::PinScope,
  session: u64,
  id: u64,
  code: Option<u32>,
  message: String,
) {
  let pending = {
    let Some(transport) = live_host(scope, session) else {
      return;
    };
    transport.get().opens.borrow_mut().remove(&id)
  };
  let error = stream_error(scope, &message, code);
  let error = yuzuko_core::op::root(scope, error);
  if let Some(pending) = pending {
    reject_moved(scope, pending.resolver, error);
    return;
  }
  let taken = {
    let Some(transport) = live_host(scope, session) else {
      return;
    };
    let mut streams = transport.get().streams.borrow_mut();
    let Some(slot) = streams.get_mut(&id) else {
      return;
    };
    slot.read_error = Some((code, message));
    TakenStream {
      read: slot.read.take(),
      write: slot.write.take(),
      write_resolver: slot.write_resolver.take(),
      close_resolver: slot.close_resolver.take(),
    }
  };
  error_side(scope, taken.read.as_ref(), error);
  error_side(scope, taken.write.as_ref(), error);
  if let Some(resolver) = taken.write_resolver {
    reject_moved(scope, resolver, error);
  }
  if let Some(resolver) = taken.close_resolver {
    reject_moved(scope, resolver, error);
  }
}

struct TakenStream {
  read: Option<v8::TracedReference<v8::Object>>,
  write: Option<v8::TracedReference<v8::Object>>,
  write_resolver: Option<v8::TracedReference<v8::PromiseResolver>>,
  close_resolver: Option<v8::TracedReference<v8::PromiseResolver>>,
}

fn on_wrote(
  scope: &mut v8::PinScope,
  session: u64,
  id: u64,
  failed: Option<(Option<u32>, String)>,
) {
  let resolver = {
    let Some(transport) = live_host(scope, session) else {
      return;
    };
    let mut streams = transport.get().streams.borrow_mut();
    let Some(slot) = streams.get_mut(&id) else {
      return;
    };
    slot.write_resolver.take()
  };
  let Some(resolver) = resolver else {
    if let Some((code, message)) = failed {
      let error = stream_error(scope, &message, code);
      let Some(transport) = live_host(scope, session) else {
        return;
      };
      let controller = {
        let streams = transport.get().streams.borrow();
        streams.get(&id).and_then(|slot| {
          slot
            .write
            .as_ref()
            .and_then(|reference| traced_object(scope, reference))
        })
      };
      if let Some(controller) = controller {
        call_method(scope, controller, "error", &[error]);
      }
    }
    return;
  };
  if let Some((code, message)) = failed {
    let error = stream_error(scope, &message, code);
    reject_moved(scope, resolver, error);
  } else {
    let value = v8::undefined(scope).into();
    resolve_moved(scope, resolver, value);
  }
}

fn on_finished(scope: &mut v8::PinScope, session: u64, id: u64) {
  let resolver = {
    let Some(transport) = live_host(scope, session) else {
      return;
    };
    let mut streams = transport.get().streams.borrow_mut();
    let Some(slot) = streams.get_mut(&id) else {
      return;
    };
    slot.close_resolver.take()
  };
  let Some(resolver) = resolver else {
    return;
  };
  let value = v8::undefined(scope).into();
  resolve_moved(scope, resolver, value);
}

fn cleanup(
  scope: &mut v8::PinScope,
  transport: &WebTransport,
  error: v8::Local<v8::Value>,
) {
  let streams = std::mem::take(&mut *transport.streams.borrow_mut());
  let opens = std::mem::take(&mut *transport.opens.borrow_mut());
  let writes = std::mem::take(&mut *transport.datagram_writes.borrow_mut());
  let datagram_read = transport.datagram_read.borrow_mut().take();
  let datagram_write = transport.datagram_write.borrow_mut().take();
  let incoming_bi = transport.incoming_bi_controller.borrow_mut().take();
  let incoming_uni = transport.incoming_uni_controller.borrow_mut().take();
  let error = yuzuko_core::op::root(scope, error);
  for open in opens.into_values() {
    reject_moved(scope, open.resolver, error);
  }
  for resolver in writes {
    reject_moved(scope, resolver, error);
  }
  for (_, slot) in streams {
    error_side(scope, slot.read.as_ref(), error);
    error_side(scope, slot.write.as_ref(), error);
    if let Some(resolver) = slot.write_resolver {
      reject_moved(scope, resolver, error);
    }
    if let Some(resolver) = slot.close_resolver {
      reject_moved(scope, resolver, error);
    }
  }
  error_side(scope, datagram_read.as_ref(), error);
  error_side(scope, datagram_write.as_ref(), error);
  error_side(scope, incoming_bi.as_ref(), error);
  error_side(scope, incoming_uni.as_ref(), error);
}

fn error_side(
  scope: &mut v8::PinScope,
  reference: Option<&v8::TracedReference<v8::Object>>,
  error: v8::Local<v8::Value>,
) {
  let Some(reference) = reference else {
    return;
  };
  let Some(controller) = traced_object(scope, reference) else {
    return;
  };
  call_method(scope, controller, "error", &[error]);
}

fn make_datagrams<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  session: u64,
) -> Option<v8::Local<'s, v8::Object>> {
  let readable = make_source(
    scope,
    session,
    0,
    on_datagram_read_start,
    on_datagram_pull,
    on_ignore,
  )?;
  let writable = make_sink(
    scope,
    session,
    0,
    on_datagram_write_start,
    on_datagram_write,
    on_datagram_close,
    on_datagram_abort,
  )?;
  let object = v8::Object::new(scope);
  set(scope, object, "readable", readable.into());
  set(scope, object, "writable", writable.into());
  Some(object)
}

fn make_incoming<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  session: u64,
  bidirectional: bool,
) -> Option<v8::Local<'s, v8::Object>> {
  if bidirectional {
    make_source(
      scope,
      session,
      0,
      on_incoming_bi_start,
      on_ignore,
      on_ignore,
    )
  } else {
    make_source(
      scope,
      session,
      0,
      on_incoming_uni_start,
      on_ignore,
      on_ignore,
    )
  }
}

fn make_stream_readable<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  session: u64,
  stream: u64,
) -> Option<v8::Local<'s, v8::Object>> {
  make_source(
    scope,
    session,
    stream,
    on_stream_read_start,
    on_stream_pull,
    on_stream_cancel,
  )
}

fn make_stream_writable<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  session: u64,
  stream: u64,
) -> Option<v8::Local<'s, v8::Object>> {
  make_sink(
    scope,
    session,
    stream,
    on_stream_write_start,
    on_stream_write,
    on_stream_close,
    on_stream_abort,
  )
}

fn make_source<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  session: u64,
  stream: u64,
  start: impl v8::MapFnTo<v8::FunctionCallback>,
  pull: impl v8::MapFnTo<v8::FunctionCallback>,
  cancel: impl v8::MapFnTo<v8::FunctionCallback>,
) -> Option<v8::Local<'s, v8::Object>> {
  let source = v8::Object::new(scope);
  set_callback(scope, source, "start", start, session, stream)?;
  set_callback(scope, source, "pull", pull, session, stream)?;
  set_callback(scope, source, "cancel", cancel, session, stream)?;
  construct(scope, "ReadableStream", source)
}

fn make_sink<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  session: u64,
  stream: u64,
  start: impl v8::MapFnTo<v8::FunctionCallback>,
  write: impl v8::MapFnTo<v8::FunctionCallback>,
  close: impl v8::MapFnTo<v8::FunctionCallback>,
  abort: impl v8::MapFnTo<v8::FunctionCallback>,
) -> Option<v8::Local<'s, v8::Object>> {
  let sink = v8::Object::new(scope);
  set_callback(scope, sink, "start", start, session, stream)?;
  set_callback(scope, sink, "write", write, session, stream)?;
  set_callback(scope, sink, "close", close, session, stream)?;
  set_callback(scope, sink, "abort", abort, session, stream)?;
  construct(scope, "WritableStream", sink)
}

fn on_datagram_read_start(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut _ret: v8::ReturnValue,
) {
  save_simple_controller(scope, &args, true);
}

fn on_datagram_pull(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut _ret: v8::ReturnValue,
) {
  let (session, _) = callback_ids(scope, &args);
  let Some(transport) = live_host(scope, session) else {
    return;
  };
  transport.get().command(Command::PullDatagram);
}

fn on_datagram_write_start(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut _ret: v8::ReturnValue,
) {
  save_simple_controller(scope, &args, false);
}

fn on_datagram_write(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut ret: v8::ReturnValue,
) {
  let (session, _) = callback_ids(scope, &args);
  match datagram_write(scope, session, args.get(0)) {
    Ok(promise) => ret.set(promise.into()),
    Err(error) => yuzuko_core::op::OpException::throw(error, scope),
  }
}

fn on_datagram_close(
  scope: &mut v8::PinScope,
  _args: v8::FunctionCallbackArguments,
  mut ret: v8::ReturnValue,
) {
  ret.set(v8::undefined(scope).into());
}

fn on_datagram_abort(
  scope: &mut v8::PinScope,
  _args: v8::FunctionCallbackArguments,
  mut ret: v8::ReturnValue,
) {
  ret.set(v8::undefined(scope).into());
}

fn on_incoming_bi_start(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut _ret: v8::ReturnValue,
) {
  save_incoming_controller(scope, &args, true);
}

fn on_incoming_uni_start(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut _ret: v8::ReturnValue,
) {
  save_incoming_controller(scope, &args, false);
}

fn on_ignore(
  _scope: &mut v8::PinScope,
  _args: v8::FunctionCallbackArguments,
  mut _ret: v8::ReturnValue,
) {
}

fn on_stream_read_start(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut _ret: v8::ReturnValue,
) {
  let (session, stream) = callback_ids(scope, &args);
  let Ok(controller) = v8::Local::<v8::Object>::try_from(args.get(0)) else {
    return;
  };
  let Some(transport) = live_host(scope, session) else {
    return;
  };
  let flush = {
    let mut streams = transport.get().streams.borrow_mut();
    let Some(slot) = streams.get_mut(&stream) else {
      return;
    };
    slot.read = Some(traced(scope, controller));
    let pending = std::mem::take(&mut slot.pending);
    let ended = slot.read_ended;
    let error = slot.read_error.clone();
    (pending, ended, error)
  };
  let controller = yuzuko_core::op::root(scope, controller);
  let (pending, ended, error) = flush;
  for bytes in pending {
    let value = uint8_array(scope, bytes);
    call_method(scope, controller, "enqueue", &[value.into()]);
  }
  if let Some((code, message)) = error {
    let error = stream_error(scope, &message, code);
    call_method(scope, controller, "error", &[error]);
  } else if ended {
    call_method(scope, controller, "close", &[]);
  }
}

fn on_stream_pull(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut _ret: v8::ReturnValue,
) {
  let (session, stream) = callback_ids(scope, &args);
  let Some(transport) = live_host(scope, session) else {
    return;
  };
  transport.get().command(Command::Pull { id: stream });
}

fn on_stream_cancel(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut _ret: v8::ReturnValue,
) {
  let (session, stream) = callback_ids(scope, &args);
  let code = reason_code(scope, args.get(0));
  let Some(transport) = live_host(scope, session) else {
    return;
  };
  transport.get().command(Command::Stop { id: stream, code });
}

fn on_stream_write_start(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut _ret: v8::ReturnValue,
) {
  let (session, stream) = callback_ids(scope, &args);
  let Ok(controller) = v8::Local::<v8::Object>::try_from(args.get(0)) else {
    return;
  };
  let Some(transport) = live_host(scope, session) else {
    return;
  };
  let mut streams = transport.get().streams.borrow_mut();
  let Some(slot) = streams.get_mut(&stream) else {
    return;
  };
  slot.write = Some(traced(scope, controller));
}

fn on_stream_write(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut ret: v8::ReturnValue,
) {
  let (session, stream) = callback_ids(scope, &args);
  match stream_write(scope, session, stream, args.get(0)) {
    Ok(promise) => ret.set(promise.into()),
    Err(error) => yuzuko_core::op::OpException::throw(error, scope),
  }
}

fn on_stream_close(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut ret: v8::ReturnValue,
) {
  let (session, stream) = callback_ids(scope, &args);
  let promise = stream_finish(scope, session, stream);
  ret.set(promise.into());
}

fn on_stream_abort(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut ret: v8::ReturnValue,
) {
  let (session, stream) = callback_ids(scope, &args);
  let code = reason_code(scope, args.get(0));
  let resolver = {
    let Some(transport) = live_host(scope, session) else {
      ret.set(v8::undefined(scope).into());
      return;
    };
    let mut streams = transport.get().streams.borrow_mut();
    streams
      .get_mut(&stream)
      .and_then(|slot| slot.write_resolver.take())
  };
  if let Some(resolver) = resolver {
    let error = stream_error(scope, "The stream was reset", Some(code));
    reject_moved(scope, resolver, error);
  }
  if let Some(transport) = live_host(scope, session) {
    transport.get().command(Command::Reset { id: stream, code });
  }
  ret.set(v8::undefined(scope).into());
}

fn save_simple_controller(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
  readable: bool,
) {
  let (session, _) = callback_ids(scope, args);
  let Ok(controller) = v8::Local::<v8::Object>::try_from(args.get(0)) else {
    return;
  };
  let Some(transport) = live_host(scope, session) else {
    return;
  };
  let transport = transport.get();
  if readable {
    *transport.datagram_read.borrow_mut() = Some(traced(scope, controller));
    let pending = std::mem::take(&mut *transport.datagram_pending.borrow_mut());
    let controller = yuzuko_core::op::root(scope, controller);
    for bytes in pending {
      let value = uint8_array(scope, bytes);
      call_method(scope, controller, "enqueue", &[value.into()]);
    }
  } else {
    *transport.datagram_write.borrow_mut() = Some(traced(scope, controller));
  }
}

fn save_incoming_controller(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
  bidirectional: bool,
) {
  let (session, _) = callback_ids(scope, args);
  let Ok(controller) = v8::Local::<v8::Object>::try_from(args.get(0)) else {
    return;
  };
  let Some(transport) = live_host(scope, session) else {
    return;
  };
  let slot = if bidirectional {
    &transport.get().incoming_bi_controller
  } else {
    &transport.get().incoming_uni_controller
  };
  *slot.borrow_mut() = Some(traced(scope, controller));
}

fn datagram_write<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  session: u64,
  chunk: v8::Local<'_, v8::Value>,
) -> Result<v8::Local<'s, v8::Promise>, OpError> {
  let bytes = chunk_bytes(scope, chunk)?;
  let Some(transport) = live_host(scope, session) else {
    return Ok(resolved_promise(scope));
  };
  if transport.get().phase.get() == SETTLED {
    let (resolver, promise) = resolver_pair(scope);
    let error = invalid_state(scope, "The session is closed");
    resolver.reject(scope, error);
    return Ok(promise);
  }
  let (resolver, promise) = resolver_pair(scope);
  transport
    .get()
    .datagram_writes
    .borrow_mut()
    .push_back(traced(scope, resolver));
  transport.get().command(Command::SendDatagram(bytes));
  Ok(promise)
}

fn stream_write<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  session: u64,
  stream: u64,
  chunk: v8::Local<'_, v8::Value>,
) -> Result<v8::Local<'s, v8::Promise>, OpError> {
  let bytes = chunk_bytes(scope, chunk)?;
  let Some(transport) = live_host(scope, session) else {
    return Ok(resolved_promise(scope));
  };
  if transport.get().phase.get() == SETTLED {
    let (resolver, promise) = resolver_pair(scope);
    let error = invalid_state(scope, "The session is closed");
    resolver.reject(scope, error);
    return Ok(promise);
  }
  let (resolver, promise) = resolver_pair(scope);
  {
    let mut streams = transport.get().streams.borrow_mut();
    let Some(slot) = streams.get_mut(&stream) else {
      let error = invalid_state(scope, "The stream is closed");
      resolver.reject(scope, error);
      return Ok(promise);
    };
    slot.write_resolver = Some(traced(scope, resolver));
  }
  transport
    .get()
    .command(Command::Write { id: stream, bytes });
  Ok(promise)
}

fn stream_finish<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  session: u64,
  stream: u64,
) -> v8::Local<'s, v8::Promise> {
  let Some(transport) = live_host(scope, session) else {
    return resolved_promise(scope);
  };
  let (resolver, promise) = resolver_pair(scope);
  {
    let mut streams = transport.get().streams.borrow_mut();
    if let Some(slot) = streams.get_mut(&stream) {
      slot.close_resolver = Some(traced(scope, resolver));
    }
  }
  transport.get().command(Command::Finish { id: stream });
  promise
}

fn chunk_bytes(
  scope: &mut v8::PinScope,
  chunk: v8::Local<v8::Value>,
) -> Result<Vec<u8>, OpError> {
  if unacceptable_buffer(scope, chunk) {
    return Err(OpError::type_(
      "A shared or resizable buffer cannot be written",
    ));
  }
  buffer_source(scope, chunk)
    .ok_or_else(|| OpError::type_("The chunk must be a BufferSource"))
}

struct ParsedOptions {
  options: Options,
  congestion: String,
}

fn parsed_options(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
) -> Result<ParsedOptions, OpError> {
  if args.length() < 2 || args.get(1).is_undefined() {
    return Ok(ParsedOptions {
      options: Options {
        congestion: Congestion::Default,
        hashes: Vec::new(),
        hashes_only: false,
        require_unreliable: false,
      },
      congestion: "default".to_string(),
    });
  }
  let value = args.get(1);
  if value.is_null() || !value.is_object() {
    return Err(OpError::type_("WebTransport options must be an object"));
  }
  let object = value
    .to_object(scope)
    .ok_or_else(|| OpError::type_("WebTransport options must be an object"))?;
  let hashes_value = property(scope, object, "serverCertificateHashes")?;
  let (hashes, hashes_only) = certificate_hashes(scope, hashes_value)?;
  let pooling = property(scope, object, "allowPooling")?;
  if pooling.boolean_value(scope) && hashes_only {
    return Err(OpError::dom(
      "NotSupportedError",
      "allowPooling cannot be used with serverCertificateHashes",
    ));
  }
  let mode = property(scope, object, "datagramsReadableType")?;
  if !mode.is_undefined() {
    let text = yuzuko_core::rust_string(scope, mode);
    if text == "bytes" {
      return Err(OpError::dom(
        "NotSupportedError",
        "datagramsReadableType bytes is not supported",
      ));
    }
    return Err(OpError::type_("The datagram readable type is invalid"));
  }
  let protocols = property(scope, object, "protocols")?;
  if !protocols.is_undefined() {
    if !protocols.is_array() {
      return Err(OpError::type_("protocols must be an array"));
    }
    let array = v8::Local::<v8::Array>::try_from(protocols)
      .map_err(|_| OpError::type_("protocols must be an array"))?;
    if array.length() > 0 {
      return Err(OpError::dom(
        "NotSupportedError",
        "WebTransport protocols are not supported",
      ));
    }
  }
  let headers = property(scope, object, "headers")?;
  reject_headers(scope, headers)?;
  let anticipated_uni = property(
    scope,
    object,
    "anticipatedConcurrentIncomingUnidirectionalStreams",
  )?;
  ignore_unsigned_short(scope, anticipated_uni)?;
  let anticipated_bi = property(
    scope,
    object,
    "anticipatedConcurrentIncomingBidirectionalStreams",
  )?;
  ignore_unsigned_short(scope, anticipated_bi)?;
  let congestion_value = property(scope, object, "congestionControl")?;
  let (congestion, label) = congestion_control(scope, congestion_value)?;
  let require_unreliable =
    property(scope, object, "requireUnreliable")?.boolean_value(scope);
  Ok(ParsedOptions {
    options: Options {
      congestion,
      hashes,
      hashes_only,
      require_unreliable,
    },
    congestion: label,
  })
}

fn certificate_hashes(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<(Vec<[u8; 32]>, bool), OpError> {
  if value.is_undefined() {
    return Ok((Vec::new(), false));
  }
  let array = v8::Local::<v8::Array>::try_from(value)
    .map_err(|_| OpError::type_("serverCertificateHashes must be an array"))?;
  let mut hashes = Vec::new();
  for index in 0..array.length() {
    let element = array
      .get_index(scope, index)
      .ok_or_else(|| OpError::type_("A certificate hash must be an object"))?;
    let object = v8::Local::<v8::Object>::try_from(element)
      .map_err(|_| OpError::type_("A certificate hash must be an object"))?;
    let algorithm = defined(scope, object, "algorithm")
      .ok_or_else(|| OpError::type_("A certificate hash needs an algorithm"))?;
    let hash_value = defined(scope, object, "value")
      .ok_or_else(|| OpError::type_("A certificate hash needs a value"))?;
    let algorithm = yuzuko_core::rust_string(scope, algorithm);
    if algorithm != "sha-256" {
      continue;
    }
    // WebIDL `BufferSource` here is not `[AllowShared]`.
    if unacceptable_buffer(scope, hash_value) {
      return Err(OpError::type_(
        "A certificate hash value must be a BufferSource",
      ));
    }
    let bytes = buffer_source(scope, hash_value).ok_or_else(|| {
      OpError::type_("A certificate hash value must be a BufferSource")
    })?;
    if bytes.len() != 32 {
      return Err(OpError::type_(
        "A sha-256 certificate hash must be 32 bytes",
      ));
    }
    let mut hash = [0u8; 32];
    hash.copy_from_slice(&bytes);
    hashes.push(hash);
  }
  let hashes_only = array.length() > 0;
  Ok((hashes, hashes_only))
}

fn reject_headers(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<(), OpError> {
  if value.is_undefined() {
    return Ok(());
  }
  if !value.is_object() {
    return Err(OpError::type_("headers must be a HeadersInit"));
  }
  let object = v8::Local::<v8::Object>::try_from(value)
    .map_err(|_| OpError::type_("headers must be a HeadersInit"))?;
  if constructor_name(scope, object) == "Headers" {
    return Err(OpError::dom(
      "NotSupportedError",
      "WebTransport headers are not supported",
    ));
  }
  if value.is_array() {
    let array = v8::Local::<v8::Array>::try_from(value)
      .map_err(|_| OpError::type_("headers must be a HeadersInit"))?;
    if array.length() == 0 {
      return Ok(());
    }
    return Err(OpError::dom(
      "NotSupportedError",
      "WebTransport headers are not supported",
    ));
  }
  let names = object
    .get_own_property_names(scope, v8::GetPropertyNamesArgs::default())
    .ok_or_else(|| OpError::type_("headers must be a HeadersInit"))?;
  if names.length() == 0 {
    Ok(())
  } else {
    Err(OpError::dom(
      "NotSupportedError",
      "WebTransport headers are not supported",
    ))
  }
}

fn congestion_control(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<(Congestion, String), OpError> {
  if value.is_undefined() {
    return Ok((Congestion::Default, "default".to_string()));
  }
  let text = yuzuko_core::rust_string(scope, value);
  match text.as_str() {
    "default" => Ok((Congestion::Default, text)),
    "throughput" => Ok((Congestion::Throughput, text)),
    "low-latency" => Ok((Congestion::LowLatency, text)),
    _ => Err(OpError::type_("The congestion control is invalid")),
  }
}

fn ignore_unsigned_short(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<(), OpError> {
  if value.is_undefined() || value.is_null() {
    return Ok(());
  }
  let number = value.number_value(scope).unwrap_or(f64::NAN);
  if !number.is_finite()
    || number.fract() != 0.0
    || !(0.0..=65535.0).contains(&number)
  {
    return Err(OpError::type_("Expected an unsigned short"));
  }
  Ok(())
}

fn reject_send_group(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
) -> Result<(), OpError> {
  if args.length() == 0 || args.get(0).is_undefined() {
    return Ok(());
  }
  let value = args.get(0);
  if !value.is_object() {
    return Err(OpError::type_(
      "WebTransport stream options must be an object",
    ));
  }
  let object = value.to_object(scope).ok_or_else(|| {
    OpError::type_("WebTransport stream options must be an object")
  })?;
  if let Some(group) = defined(scope, object, "sendGroup")
    && !group.is_null()
  {
    return Err(OpError::dom(
      "NotSupportedError",
      "WebTransportSendGroup is not supported",
    ));
  }
  Ok(())
}

fn error_options(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<(String, Option<u32>), OpError> {
  if !value.is_object() {
    return Err(OpError::type_("WebTransportErrorOptions must be an object"));
  }
  let object = value.to_object(scope).ok_or_else(|| {
    OpError::type_("WebTransportErrorOptions must be an object")
  })?;
  let source = if let Some(value) = defined(scope, object, "source") {
    let text = yuzuko_core::rust_string(scope, value);
    if text != "stream" && text != "session" {
      return Err(OpError::type_("The error source is invalid"));
    }
    text
  } else {
    "stream".to_string()
  };
  let code = match defined(scope, object, "streamErrorCode") {
    None => None,
    Some(value) if value.is_null() => None,
    Some(value) => Some(clamp_unsigned_long(scope, value)),
  };
  Ok((source, code))
}

fn close_arguments(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
) -> Result<(u32, String), OpError> {
  if args.length() == 0 || args.get(0).is_undefined() {
    return Ok((0, String::new()));
  }
  let value = args.get(0);
  if !value.is_object() {
    return Err(OpError::type_("WebTransportCloseInfo must be an object"));
  }
  let object = value
    .to_object(scope)
    .ok_or_else(|| OpError::type_("WebTransportCloseInfo must be an object"))?;
  let code = if let Some(value) = defined(scope, object, "closeCode") {
    value.uint32_value(scope).unwrap_or(0)
  } else {
    0
  };
  let reason = if let Some(value) = defined(scope, object, "reason") {
    yuzuko_core::rust_string(scope, value)
  } else {
    String::new()
  };
  if reason.len() > 1024 {
    return Err(OpError::dom("SyntaxError", "The close reason is too long"));
  }
  Ok((code, reason))
}

fn parse_url(input: &str) -> Result<url::Url, OpError> {
  let url = if let Ok(url) = url::Url::parse(input) {
    url
  } else if let Some(base) = yuzuko_core::entry_url() {
    let base = url::Url::parse(&base)
      .map_err(|_| OpError::dom("SyntaxError", "The URL is invalid"))?;
    base
      .join(input)
      .map_err(|_| OpError::dom("SyntaxError", "The URL is invalid"))?
  } else {
    return Err(OpError::dom("SyntaxError", "The URL is invalid"));
  };
  if url.scheme() != "https" {
    return Err(OpError::dom("SyntaxError", "The URL scheme is not https"));
  }
  if url.fragment().is_some() {
    return Err(OpError::dom("SyntaxError", "The URL has a fragment"));
  }
  Ok(url)
}

fn live_host(
  scope: &mut v8::PinScope,
  id: u64,
) -> Option<HostRef<WebTransport>> {
  let object = live_object(scope, id)?;
  unsafe { host_ref::<WebTransport>(scope, object) }
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

fn callback_ids(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
) -> (u64, u64) {
  let Ok(array) = v8::Local::<v8::Array>::try_from(args.data()) else {
    return (0, 0);
  };
  let session = array
    .get_index(scope, 0)
    .and_then(|value| v8::Local::<v8::BigInt>::try_from(value).ok())
    .map(|value| value.u64_value().0)
    .unwrap_or(0);
  let stream = array
    .get_index(scope, 1)
    .and_then(|value| v8::Local::<v8::BigInt>::try_from(value).ok())
    .map(|value| value.u64_value().0)
    .unwrap_or(0);
  (session, stream)
}

fn set_callback(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
  callback: impl v8::MapFnTo<v8::FunctionCallback>,
  session: u64,
  stream: u64,
) -> Option<()> {
  let data = v8::Array::new(scope, 2);
  let session = v8::BigInt::new_from_u64(scope, session);
  let stream = v8::BigInt::new_from_u64(scope, stream);
  let _ = data.set_index(scope, 0, session.into());
  let _ = data.set_index(scope, 1, stream.into());
  let function = v8::Function::builder(callback)
    .data(data.into())
    .build(scope)?;
  set(scope, object, name, function.into());
  Some(())
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

fn promise_js(scope: &mut v8::PinScope, promise: v8::Local<v8::Promise>) -> Js {
  let value: v8::Local<v8::Value> = promise.into();
  Js(v8::Global::new(scope, value))
}

fn rejected(scope: &mut v8::PinScope, error: v8::Local<v8::Value>) -> Js {
  let (resolver, promise) = resolver_pair(scope);
  let error = yuzuko_core::op::root(scope, error);
  resolver.reject(scope, error);
  promise_js(scope, promise)
}

fn resolved_promise<'s>(
  scope: &mut v8::PinScope<'s, '_>,
) -> v8::Local<'s, v8::Promise> {
  let (resolver, promise) = resolver_pair(scope);
  let value = v8::undefined(scope).into();
  resolver.resolve(scope, value);
  promise
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

fn object_slot(
  scope: &mut v8::PinScope,
  slot: &RefCell<Option<v8::TracedReference<v8::Object>>>,
) -> Js {
  match slot
    .borrow()
    .as_ref()
    .and_then(|reference| traced_object(scope, reference))
  {
    Some(object) => {
      let value: v8::Local<v8::Value> = object.into();
      Js(v8::Global::new(scope, value))
    }
    None => Js::null(scope),
  }
}

fn traced_object<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  reference: &v8::TracedReference<v8::Object>,
) -> Option<v8::Local<'s, v8::Object>> {
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
  resolve_moved(scope, reference, value);
}

fn reject_slot(
  scope: &mut v8::PinScope,
  slot: &RefCell<Option<v8::TracedReference<v8::PromiseResolver>>>,
  value: v8::Local<v8::Value>,
) {
  let Some(reference) = slot.borrow_mut().take() else {
    return;
  };
  reject_moved(scope, reference, value);
}

fn resolve_moved(
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
  resolver.resolve(scope, value);
}

fn reject_moved(
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

fn trace_slot<T>(
  slot: &RefCell<Option<v8::TracedReference<T>>>,
  visitor: &mut v8::cppgc::Visitor,
) {
  if let Ok(slot) = slot.try_borrow() {
    visitor.trace(&*slot);
  }
}

fn trace_writes(
  slot: &RefCell<VecDeque<v8::TracedReference<v8::PromiseResolver>>>,
  visitor: &mut v8::cppgc::Visitor,
) {
  if let Ok(queue) = slot.try_borrow() {
    for resolver in queue.iter() {
      visitor.trace(resolver);
    }
  }
}

fn trace_streams(
  slot: &RefCell<HashMap<u64, StreamSlot>>,
  visitor: &mut v8::cppgc::Visitor,
) {
  if let Ok(streams) = slot.try_borrow() {
    for stream in streams.values() {
      visitor.trace(&stream.read);
      visitor.trace(&stream.write);
      visitor.trace(&stream.write_resolver);
      visitor.trace(&stream.close_resolver);
    }
  }
}

fn trace_opens(
  slot: &RefCell<HashMap<u64, PendingOpen>>,
  visitor: &mut v8::cppgc::Visitor,
) {
  if let Ok(opens) = slot.try_borrow() {
    for open in opens.values() {
      visitor.trace(&open.resolver);
    }
  }
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

fn property<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  object: v8::Local<v8::Object>,
  name: &str,
) -> Result<v8::Local<'s, v8::Value>, OpError> {
  object
    .get(scope, key(scope, name).into())
    .ok_or_else(|| OpError::type_("WebTransport options must be an object"))
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

fn constructor_name(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
) -> String {
  let Some(ctor) = object.get(scope, key(scope, "constructor").into()) else {
    return String::new();
  };
  let Ok(ctor) = v8::Local::<v8::Object>::try_from(ctor) else {
    return String::new();
  };
  let Some(name) = ctor.get(scope, key(scope, "name").into()) else {
    return String::new();
  };
  yuzuko_core::rust_string(scope, name)
}

fn reason_code(scope: &mut v8::PinScope, reason: v8::Local<v8::Value>) -> u32 {
  let Ok(object) = v8::Local::<v8::Object>::try_from(reason) else {
    return 0;
  };
  let Some(error) = (unsafe { host_ref::<WebTransportError>(scope, object) })
  else {
    return 0;
  };
  error.get().stream_error_code.unwrap_or(0)
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

fn clamp_unsigned_long(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> u32 {
  let number = value.number_value(scope).unwrap_or(f64::NAN);
  if number.is_nan() || number == f64::NEG_INFINITY {
    return 0;
  }
  if number == f64::INFINITY {
    return u32::MAX;
  }
  let rounded = number.clamp(0.0, u32::MAX as f64).round_ties_even();
  if rounded >= u32::MAX as f64 {
    u32::MAX
  } else {
    rounded as u32
  }
}

fn session_error<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  message: &str,
) -> v8::Local<'s, v8::Value> {
  fabricate(scope, message, "session", None).into()
}

fn stream_error<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  message: &str,
  code: Option<u32>,
) -> v8::Local<'s, v8::Value> {
  fabricate(scope, message, "stream", code).into()
}

fn fabricate<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  message: &str,
  source: &str,
  stream_error_code: Option<u32>,
) -> v8::Local<'s, v8::Object> {
  let object = unsafe {
    yuzuko_core::op::wrap_instance(
      scope,
      WebTransportError {
        message: message.to_string(),
        source: source.to_string(),
        stream_error_code,
      },
    )
  };
  stamp(scope, object, message);
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
  if let Some(text) = v8::String::new(scope, "WebTransportError") {
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
  let Some(constructor) =
    WebTransportError::template(scope).get_function(scope)
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

fn invalid_state<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  message: &str,
) -> v8::Local<'s, v8::Value> {
  yuzuko_core::op::new_dom_exception(scope, "InvalidStateError", message)
    .map(Into::into)
    .unwrap_or_else(|| v8::undefined(scope).into())
}
