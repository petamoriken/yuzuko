//! `WebSocket` and `CloseEvent`.
//!
//! The second constructor argument is a protocol string, a sequence of
//! protocols, or a `WebSocketInit` dictionary whose `protocols` member is
//! that sequence. Further arguments are ignored.

use std::cell::{Cell, RefCell};
use std::sync::atomic::{AtomicU64, Ordering};

use yuzuko_core::op::{OpError, host_ref};
use yuzuko_macros::op;
use yuzuko_winter::{
  AddressSpace, Blob, Command, Connection, Note, NoteQueue, abort_all,
  array_buffer, buffer_source, connect, environment_origin, is_bad_port,
  new_blob, snapshot_blob, valid_protocol,
};

use super::common::{
  self, clamp_unsigned_short, define_constant, dispatch_simple,
};
use crate::event::{Event, define_is_trusted};
use crate::message::trusted_message;
use crate::target::{self, EventTarget};
use crate::util::{self, key};

const CONNECTING: i32 = 0;
const OPEN: i32 = 1;
const CLOSING: i32 = 2;
const CLOSED: i32 = 3;

static NEXT: AtomicU64 = AtomicU64::new(1);

struct Live {
  object: v8::Global<v8::Object>,
}

thread_local! {
  static LIVE: RefCell<Vec<(u64, Live)>> = const { RefCell::new(Vec::new()) };
}

#[repr(C)]
pub struct WebSocket {
  base: EventTarget,
  id: u64,
  url: String,
  ready: Cell<i32>,
  protocol: RefCell<String>,
  buffered: Cell<u64>,
  binary_blob: Cell<bool>,
  connection: RefCell<Option<Connection>>,
}

unsafe impl v8::cppgc::GarbageCollected for WebSocket {
  fn trace(&self, visitor: &mut v8::cppgc::Visitor) {
    v8::cppgc::GarbageCollected::trace(&self.base, visitor);
  }

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"WebSocket"
  }
}

#[op(
  inherit = EventTarget,
  inspect = [
    "url",
    "readyState",
    "extensions",
    "protocol",
    "binaryType",
    "bufferedAmount",
    "onmessage",
    "onerror",
    "onclose",
    "onopen",
  ]
)]
impl WebSocket {
  #[constructor]
  #[length(1)]
  fn new(
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<Self, OpError> {
    if args.length() < 1 {
      return Err(OpError::type_("WebSocket requires a URL"));
    }
    let input = yuzuko_core::rust_string(scope, args.get(0));
    let url = parse_url(&input)?;
    yuzuko_core::authorize_url(&url)?;
    let init = if args.length() < 2 {
      SocketInit::default()
    } else {
      socket_init(scope, args.get(1))?
    };
    let bad_port = url.port_or_known_default().is_some_and(is_bad_port);
    let origin = environment_origin();
    let connection = if bad_port {
      Connection::failed()
    } else {
      connect(url.clone(), init.protocols, origin, init.target, None)
    };
    Ok(Self {
      base: EventTarget::new(),
      id: NEXT.fetch_add(1, Ordering::Relaxed),
      url: url.to_string(),
      ready: Cell::new(CONNECTING),
      protocol: RefCell::new(String::new()),
      buffered: Cell::new(0),
      binary_blob: Cell::new(true),
      connection: RefCell::new(Some(connection)),
    })
  }

  #[construct_hook]
  fn finish(&self, scope: &mut v8::PinScope, object: v8::Local<v8::Object>) {
    LIVE.with(|slot| {
      slot.borrow_mut().push((
        self.id,
        Live {
          object: v8::Global::new(scope, object),
        },
      ));
    });
    if let Some(connection) = self.connection.borrow().clone() {
      arm(self.id, connection.notes);
    }
  }

  #[getter]
  fn url(&self) -> String {
    self.url.clone()
  }

  #[getter]
  fn ready_state(&self) -> i32 {
    self.ready.get()
  }

  #[getter]
  fn buffered_amount(&self) -> f64 {
    self.buffered.get() as f64
  }

  #[getter]
  fn extensions(&self) -> String {
    String::new()
  }

  #[getter]
  fn protocol(&self) -> String {
    self.protocol.borrow().clone()
  }

  #[getter]
  fn binary_type(&self) -> String {
    if self.binary_blob.get() {
      "blob".to_string()
    } else {
      "arraybuffer".to_string()
    }
  }

  #[setter]
  fn binary_type(&self, value: String) {
    if value == "blob" {
      self.binary_blob.set(true);
    } else if value == "arraybuffer" {
      self.binary_blob.set(false);
    }
  }

  fn close(
    &self,
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<(), OpError> {
    let code = if args.length() == 0 || args.get(0).is_undefined() {
      None
    } else {
      let value = clamp_unsigned_short(scope, args.get(0));
      if value != 1000 && !(3000..=4999).contains(&value) {
        return Err(OpError::dom(
          "InvalidAccessError",
          "The close code is invalid",
        ));
      }
      Some(value)
    };
    let reason = if args.length() < 2 || args.get(1).is_undefined() {
      String::new()
    } else {
      let text = yuzuko_core::rust_string(scope, args.get(1));
      if text.len() > 123 {
        return Err(OpError::dom(
          "SyntaxError",
          "The close reason is too long",
        ));
      }
      text
    };
    match self.ready.get() {
      CLOSING | CLOSED => return Ok(()),
      CONNECTING => {
        self.ready.set(CLOSING);
        self.command(Command::Abort);
      }
      _ => {
        self.ready.set(CLOSING);
        self.command(Command::Close { code, reason });
      }
    }
    Ok(())
  }

  fn send(
    &self,
    scope: &mut v8::PinScope,
    value: v8::Local<v8::Value>,
  ) -> Result<(), OpError> {
    if self.ready.get() != OPEN {
      return Err(OpError::dom("InvalidStateError", "WebSocket is not open"));
    }
    if let Some(bytes) = buffer_source(scope, value)? {
      self.queue_binary(bytes);
      return Ok(());
    }
    if let Ok(object) = v8::Local::<v8::Object>::try_from(value)
      && unsafe { host_ref::<Blob>(scope, object) }.is_some()
    {
      let (bytes, _) = snapshot_blob(scope, value)?;
      self.queue_binary(bytes);
      return Ok(());
    }
    let text = yuzuko_core::rust_string(scope, value);
    let len = text.len() as u64;
    self.buffered.set(self.buffered.get().saturating_add(len));
    self.command(Command::Text(text));
    Ok(())
  }
}

impl WebSocket {
  fn command(&self, command: Command) {
    if let Some(connection) = self.connection.borrow().as_ref() {
      connection.command(command);
    }
  }

  fn queue_binary(&self, bytes: Vec<u8>) {
    let len = bytes.len() as u64;
    self.buffered.set(self.buffered.get().saturating_add(len));
    self.command(Command::Binary(bytes));
  }
}

#[repr(C)]
pub struct CloseEvent {
  base: Event,
  clean: bool,
  code: u16,
  reason: String,
}

unsafe impl v8::cppgc::GarbageCollected for CloseEvent {
  fn trace(&self, visitor: &mut v8::cppgc::Visitor) {
    v8::cppgc::GarbageCollected::trace(&self.base, visitor);
  }

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"CloseEvent"
  }
}

#[op(inherit = Event, inspect_after = ["wasClean", "code", "reason"])]
impl CloseEvent {
  #[constructor]
  #[length(1)]
  fn new(
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<Self, OpError> {
    let base = Event::from_args(scope, args, false)?;
    let init = util::init_arg(args);
    let clean = util::dict_bool(scope, init, "wasClean");
    let code_value = util::dict_get(scope, init, "code")
      .unwrap_or_else(|| v8::undefined(scope).into());
    let code = clamp_unsigned_short(scope, code_value);
    let reason = util::dict_string(scope, init, "reason");
    Ok(Self {
      base,
      clean,
      code,
      reason,
    })
  }

  #[construct_hook]
  fn finish(&self, scope: &mut v8::PinScope, object: v8::Local<v8::Object>) {
    let _ = self;
    define_is_trusted(scope, object);
  }

  #[getter]
  fn was_clean(&self) -> bool {
    self.clean
  }

  #[getter]
  fn code(&self) -> u32 {
    u32::from(self.code)
  }

  #[getter]
  fn reason(&self) -> String {
    self.reason.clone()
  }
}

pub(crate) fn install(scope: &mut v8::PinScope) {
  CloseEvent::install(scope);
  WebSocket::install(scope);
  let constructor = WebSocket::template(scope)
    .get_function(scope)
    .expect("WebSocket");
  crate::target::define_prototype_event_handlers(
    scope,
    constructor,
    &["open", "message", "error", "close"],
  );
  let constructor = v8::Local::<v8::Object>::try_from(
    v8::Local::<v8::Value>::from(constructor),
  )
  .expect("WebSocket constructor");
  let prototype = constructor
    .get(scope, key(scope, "prototype").into())
    .and_then(|value| v8::Local::<v8::Object>::try_from(value).ok())
    .expect("WebSocket prototype");
  for (name, value) in [
    ("CONNECTING", CONNECTING),
    ("OPEN", OPEN),
    ("CLOSING", CLOSING),
    ("CLOSED", CLOSED),
  ] {
    define_constant(scope, constructor, name, value);
    define_constant(scope, prototype, name, value);
  }
}

pub(crate) fn clear() {
  abort_all();
  LIVE.with(|slot| slot.borrow_mut().clear());
}

fn arm(id: u64, notes: NoteQueue) {
  notes.listen(common::one_shot_wake("websocket wake", move |scope| {
    pump(scope, id)
  }));
}

fn pump(scope: &mut v8::PinScope, id: u64) {
  let Some(object) = live_object(scope, id) else {
    return;
  };
  let Some(socket) = (unsafe { host_ref::<WebSocket>(scope, object) }) else {
    return;
  };
  let socket = socket.get();
  let Some(connection) = socket.connection.borrow().clone() else {
    return;
  };
  let mut open = socket.ready.get() != CLOSED;
  while let Some(note) = connection.notes.poll() {
    if socket.ready.get() == CLOSED {
      open = false;
      break;
    }
    match note {
      Note::Open { protocol } => {
        if socket.ready.get() == CLOSING {
          socket.command(Command::Abort);
          continue;
        }
        *socket.protocol.borrow_mut() = protocol;
        socket.ready.set(OPEN);
        dispatch_simple(scope, object, "open");
      }
      Note::Text(text) => {
        let value = v8::String::new(scope, &text).unwrap();
        dispatch_message(scope, object, &socket.url, value.into());
      }
      Note::Binary(bytes) => {
        let value: v8::Local<v8::Value> = if socket.binary_blob.get() {
          new_blob(scope, bytes, "").into()
        } else {
          array_buffer(scope, bytes).into()
        };
        dispatch_message(scope, object, &socket.url, value);
      }
      Note::Flushed(bytes) => {
        socket
          .buffered
          .set(socket.buffered.get().saturating_sub(bytes));
      }
      Note::Error => {
        if socket.ready.get() != CLOSED {
          dispatch_simple(scope, object, "error");
        }
      }
      Note::Closed {
        code,
        reason,
        clean,
      } => {
        socket.ready.set(CLOSED);
        dispatch_close(scope, object, clean, code, reason);
        open = false;
      }
    }
  }
  if open && socket.ready.get() != CLOSED {
    arm(id, connection.notes);
  }
}

fn dispatch_message(
  scope: &mut v8::PinScope,
  target: v8::Local<v8::Object>,
  url: &str,
  data: v8::Local<v8::Value>,
) {
  let origin = url::Url::parse(url)
    .map(|url| url.origin().ascii_serialization())
    .unwrap_or_else(|_| "null".to_string());
  let event = trusted_message(scope, "message", data, origin, String::new());
  let _ = target::dispatch(scope, target, event);
}

fn dispatch_close(
  scope: &mut v8::PinScope,
  target: v8::Local<v8::Object>,
  clean: bool,
  code: u16,
  reason: String,
) {
  let event = CloseEvent {
    base: Event::new("close", false, false, false, true),
    clean,
    code,
    reason,
  };
  let object = unsafe { yuzuko_core::op::wrap_instance(scope, event) };
  define_is_trusted(scope, object);
  let _ = target::dispatch(scope, target, object);
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

pub(crate) fn parse_url(input: &str) -> Result<url::Url, OpError> {
  common::parse_url(input, &["ws", "wss"], "The URL scheme is not ws or wss")
}

#[derive(Default)]
struct SocketInit {
  protocols: Vec<String>,
  target: Option<AddressSpace>,
}

fn socket_init(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<SocketInit, OpError> {
  if value.is_undefined() || value.is_null() {
    return Ok(SocketInit::default());
  }
  if value.is_array() {
    return Ok(SocketInit {
      protocols: protocol_list(scope, value)?,
      target: None,
    });
  }
  if value.is_object() {
    let object = value
      .to_object(scope)
      .ok_or_else(|| OpError::type_("WebSocket options must be an object"))?;
    let property = object
      .get(scope, key(scope, "protocols").into())
      .ok_or_else(|| OpError::type_("WebSocket options must be an object"))?;
    let protocols = if property.is_undefined() {
      Vec::new()
    } else {
      protocol_list(scope, property)?
    };
    let target = target_space(scope, &object)?;
    return Ok(SocketInit { protocols, target });
  }
  Ok(SocketInit {
    protocols: one_protocol(scope, value)?,
    target: None,
  })
}

fn target_space(
  scope: &mut v8::PinScope,
  object: &v8::Local<v8::Object>,
) -> Result<Option<AddressSpace>, OpError> {
  let value = object
    .get(scope, key(scope, "targetAddressSpace").into())
    .ok_or_else(|| OpError::type_("WebSocket options must be an object"))?;
  if value.is_undefined() {
    return Ok(None);
  }
  let text = yuzuko_core::rust_string(scope, value);
  match text.as_str() {
    "loopback" => Ok(Some(AddressSpace::Loopback)),
    "local" => Ok(Some(AddressSpace::Local)),
    "public" => Ok(Some(AddressSpace::Public)),
    _ => Err(OpError::type_(
      "The targetAddressSpace is not loopback, local, or public",
    )),
  }
}

pub(crate) fn protocol_list(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<Vec<String>, OpError> {
  let array = v8::Local::<v8::Array>::try_from(value)
    .map_err(|_| OpError::type_("protocols must be a sequence"))?;
  let mut protocols = Vec::new();
  for index in 0..array.length() {
    let item = array
      .get_index(scope, index)
      .ok_or_else(|| OpError::type_("protocols must be a sequence"))?;
    protocols.push(one_protocol_text(scope, item)?);
  }
  reject_duplicates(&protocols)?;
  Ok(protocols)
}

fn reject_duplicates(protocols: &[String]) -> Result<(), OpError> {
  for (index, protocol) in protocols.iter().enumerate() {
    if protocols[..index]
      .iter()
      .any(|earlier| earlier.eq_ignore_ascii_case(protocol))
    {
      return Err(OpError::dom("SyntaxError", "The protocol is duplicated"));
    }
  }
  Ok(())
}

fn one_protocol(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<Vec<String>, OpError> {
  Ok(vec![one_protocol_text(scope, value)?])
}

fn one_protocol_text(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<String, OpError> {
  let text = yuzuko_core::rust_string(scope, value);
  if !valid_protocol(&text) {
    return Err(OpError::dom("SyntaxError", "The protocol is invalid"));
  }
  Ok(text)
}
