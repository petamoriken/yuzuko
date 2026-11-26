//! `MessageChannel`, `MessagePort`, and `MessageEvent`.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::collections::HashSet;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{LazyLock, Mutex};

use yuzuko_core::op::Js;
use yuzuko_core::op::OpError;
use yuzuko_core::op::OpException;
use yuzuko_core::op::traced;
use yuzuko_core::op::traced_local;
use yuzuko_macros::op;

use crate::event::Event;
use crate::event::define_is_trusted;
use crate::target::{self, EventTarget};

/// A message waiting on a port, with the `MessagePort`s its sender listed
/// in the transfer list. They become `MessageEvent.ports`.
#[derive(Clone)]
pub(crate) struct Queued {
  data: v8::Global<v8::Value>,
  ports: Vec<v8::Global<v8::Object>>,
}

struct PortState {
  peer: Option<u64>,
  started: bool,
  closed: bool,
  wrapper: Option<v8::Global<v8::Object>>,
  /// Deserialization failures waiting for `deliver`.
  failures: u32,
  queue: VecDeque<Queued>,
}

thread_local! {
  static PORTS: RefCell<HashMap<u64, PortState>> =
    RefCell::new(HashMap::new());
}

static NEXT: AtomicU64 = AtomicU64::new(1);

static PORT_AGENT: LazyLock<Mutex<HashMap<u64, u64>>> =
  LazyLock::new(|| Mutex::new(HashMap::new()));

struct PortMove {
  id: u64,
  peer: Option<u64>,
  started: bool,
  failures: u32,
  /// Each queued message and the ids of the ports it transferred.
  queue: Vec<(yuzuko_core::Parcel, Vec<u64>)>,
}

static PORT_MOVES: LazyLock<Mutex<HashMap<u64, PortMove>>> =
  LazyLock::new(|| Mutex::new(HashMap::new()));

thread_local! {
  static CAPTURE_PORTS: Cell<bool> = const { Cell::new(false) };
  static READ_PORTS: RefCell<Vec<v8::Global<v8::Object>>> =
    const { RefCell::new(Vec::new()) };
  static EARLY_PORTS: RefCell<HashMap<u64, Vec<Queued>>> =
    RefCell::new(HashMap::new());
  static EARLY_FAILURES: RefCell<HashMap<u64, u32>> =
    RefCell::new(HashMap::new());
}

fn bind_port(id: u64) {
  PORT_AGENT
    .lock()
    .expect("port agents")
    .insert(id, yuzuko_core::agent_id());
}

fn port_agent(id: u64) -> u64 {
  PORT_AGENT
    .lock()
    .expect("port agents")
    .get(&id)
    .copied()
    .unwrap_or_else(yuzuko_core::agent_id)
}

pub(crate) fn capture_ports(enabled: bool) {
  CAPTURE_PORTS.with(|cell| cell.set(enabled));
  if enabled {
    READ_PORTS.with(|slot| slot.borrow_mut().clear());
  }
}

pub(crate) fn take_captured_ports() -> Vec<v8::Global<v8::Object>> {
  CAPTURE_PORTS.with(|cell| cell.set(false));
  READ_PORTS.with(|slot| std::mem::take(&mut *slot.borrow_mut()))
}

fn note_captured(scope: &mut v8::PinScope, wrapper: v8::Local<v8::Object>) {
  if !CAPTURE_PORTS.with(Cell::get) {
    return;
  }
  READ_PORTS
    .with(|slot| slot.borrow_mut().push(v8::Global::new(scope, wrapper)));
}

fn alloc() -> u64 {
  NEXT.fetch_add(1, Ordering::Relaxed)
}

pub(crate) fn clear() {
  PORTS.with(|slot| slot.borrow_mut().clear());
  MOVED_PORTS.with(|slot| slot.borrow_mut().clear());
  EARLY_PORTS.with(|slot| slot.borrow_mut().clear());
  EARLY_FAILURES.with(|slot| slot.borrow_mut().clear());
  READ_PORTS.with(|slot| slot.borrow_mut().clear());
  CAPTURE_PORTS.with(|cell| cell.set(false));
}

pub(crate) fn message_event_for<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  data: v8::Local<v8::Value>,
  ports: &[v8::Global<v8::Object>],
) -> v8::Local<'s, v8::Object> {
  let data = v8::Global::new(scope, data);
  message_event(scope, "message", &data, ports, "")
}

const TAG_PORT: u32 = 12;

thread_local! {
  static MOVED_PORTS: RefCell<HashMap<u64, u64>> = RefCell::new(HashMap::new());
}

pub(crate) fn register_clone() {
  yuzuko_core::add_host_clone(write_port_clone, read_port_clone);
  yuzuko_core::add_host_transfer(probe_port);
}

fn probe_port(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
) -> yuzuko_core::TransferStatus {
  let Some(port) =
    (unsafe { yuzuko_core::op::host_ref::<MessagePort>(scope, object) })
  else {
    return yuzuko_core::TransferStatus::Skip;
  };
  let closed = PORTS.with(|slot| {
    slot
      .borrow()
      .get(&port.get().id)
      .is_none_or(|state| state.closed)
  });
  if closed {
    yuzuko_core::TransferStatus::Detached
  } else {
    yuzuko_core::TransferStatus::Ready
  }
}

/// Moves `object` the way a transfer-list entry moves, without consulting
/// that list.
pub(crate) fn transfer_port(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
) -> Result<u64, ()> {
  let id = {
    let Some(port) =
      (unsafe { yuzuko_core::op::host_ref::<MessagePort>(scope, object) })
    else {
      return Err(());
    };
    port.get().id
  };
  if yuzuko_core::cross_agent() {
    move_port_cross(scope, id)
  } else {
    moved_port(scope, id)
  }
}

/// Installs a port moved by [`transfer_port`].
///
/// A second call returns the wrapper already installed for this id. The
/// caller records that wrapper on `MessageEvent.ports` when the id came
/// from the caller's transfer list.
pub(crate) fn receive_transferred_port(
  scope: &mut v8::PinScope,
  id: u64,
) -> Result<v8::Global<v8::Object>, ()> {
  if let Some(ticket) = PORT_MOVES.lock().expect("port moves").remove(&id) {
    return Ok(install_moved_port(scope, ticket));
  }
  PORTS
    .with(|slot| {
      slot
        .borrow()
        .get(&id)
        .and_then(|state| state.wrapper.clone())
    })
    .ok_or(())
}

fn write_port_clone(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  helper: &dyn v8::ValueSerializerHelper,
) -> yuzuko_core::WriteResult {
  let is_port = {
    unsafe { yuzuko_core::op::host_ref::<MessagePort>(scope, object) }.is_some()
  };
  if !is_port {
    return yuzuko_core::WriteResult::Skip;
  }
  if !yuzuko_core::is_structured_transfer(scope, object) {
    yuzuko_core::reject_clone(scope);
    return yuzuko_core::WriteResult::Failed;
  }
  let new_id = match transfer_port(scope, object) {
    Ok(id) => id,
    Err(()) => {
      yuzuko_core::reject_clone(scope);
      return yuzuko_core::WriteResult::Failed;
    }
  };
  helper.write_uint32(TAG_PORT);
  helper.write_uint64(new_id);
  yuzuko_core::WriteResult::Wrote
}

fn move_port_cross(scope: &mut v8::PinScope, old_id: u64) -> Result<u64, ()> {
  let queued = PORTS.with(|slot| {
    slot
      .borrow()
      .get(&old_id)
      .filter(|state| !state.closed)
      .map(|state| (state.queue.clone(), state.failures))
  });
  let Some((queued, failures)) = queued else {
    return Err(());
  };
  let target = yuzuko_core::cross_target();
  let mut parcels = Vec::new();
  for queued in &queued {
    let local = v8::Local::new(scope, &queued.data);
    let parcel =
      yuzuko_core::serialize_parcel(scope, local, &queued.ports, &[], target)
        .map_err(|_| ())?;
    let ports = transfer_listed_ports(scope, &queued.ports).map_err(|_| ())?;
    parcels.push((parcel, ports));
  }
  let (peer, started) = PORTS.with(|slot| {
    let mut ports = slot.borrow_mut();
    let state = ports.get_mut(&old_id).ok_or(())?;
    if state.closed {
      return Err(());
    }
    let peer = state.peer.take().ok_or(())?;
    state.closed = true;
    state.queue.clear();
    Ok((Some(peer), state.started))
  })?;
  PORT_MOVES.lock().expect("port moves").insert(
    old_id,
    PortMove {
      id: old_id,
      peer,
      started,
      failures,
      queue: parcels,
    },
  );
  PORT_AGENT
    .lock()
    .expect("port agents")
    .insert(old_id, target);
  Ok(old_id)
}

fn read_port_clone(
  scope: &mut v8::PinScope,
  tag: u32,
  helper: &dyn v8::ValueDeserializerHelper,
) -> yuzuko_core::ReadResult {
  if tag != TAG_PORT {
    return yuzuko_core::ReadResult::Skip;
  }
  let mut id = 0u64;
  if !helper.read_uint64(&mut id) {
    yuzuko_core::reject_clone(scope);
    return yuzuko_core::ReadResult::Failed;
  }
  match receive_transferred_port(scope, id) {
    Ok(wrapper) => {
      note_captured(scope, v8::Local::new(scope, &wrapper));
      yuzuko_core::ReadResult::Done(wrapper)
    }
    Err(()) => {
      yuzuko_core::reject_clone(scope);
      yuzuko_core::ReadResult::Failed
    }
  }
}

fn install_moved_port(
  scope: &mut v8::PinScope,
  ticket: PortMove,
) -> v8::Global<v8::Object> {
  let mut queue = VecDeque::new();
  let mut failures = ticket.failures;
  for (parcel, ids) in ticket.queue {
    match received_message(scope, parcel, &ids) {
      Some(queued) => queue.push_back(queued),
      None => failures = failures.saturating_add(1),
    }
  }
  let early = EARLY_PORTS
    .with(|slot| slot.borrow_mut().remove(&ticket.id).unwrap_or_default());
  queue.extend(early);
  let early_failures = EARLY_FAILURES
    .with(|slot| slot.borrow_mut().remove(&ticket.id).unwrap_or(0));
  failures = failures.saturating_add(early_failures);
  PORTS.with(|slot| {
    slot.borrow_mut().insert(
      ticket.id,
      PortState {
        peer: ticket.peer,
        started: false,
        closed: false,
        wrapper: None,
        failures,
        queue,
      },
    );
  });
  bind_port(ticket.id);
  let _ = MessagePort::template(scope);
  let wrapper = unsafe {
    yuzuko_core::op::wrap_instance(
      scope,
      MessagePort {
        base: EventTarget::new(),
        id: ticket.id,
      },
    )
  };
  let stored = v8::Global::new(scope, wrapper);
  PORTS.with(|slot| {
    if let Some(state) = slot.borrow_mut().get_mut(&ticket.id) {
      state.wrapper = Some(v8::Global::clone(&stored));
    }
  });
  if ticket.started {
    start_port(ticket.id);
  }
  stored
}

/// Deserializes a parcel posted to a port and installs the ports its
/// sender transferred, in transfer-list order. `None` is a message error.
pub(crate) fn received_message(
  scope: &mut v8::PinScope,
  parcel: yuzuko_core::Parcel,
  ids: &[u64],
) -> Option<Queued> {
  capture_ports(true);
  let value = yuzuko_core::deserialize_parcel(scope, parcel);
  // A port inside the value is installed during deserialization, and the
  // lookup below returns that wrapper.
  let _installed = take_captured_ports();
  let value = value.ok()?;
  let mut ports = Vec::with_capacity(ids.len());
  for id in ids {
    ports.push(receive_transferred_port(scope, *id).ok()?);
  }
  Some(Queued {
    data: v8::Global::new(scope, value),
    ports,
  })
}

pub(crate) fn receive_port_message(id: u64, queued: Queued) {
  let started = PORTS.with(|slot| {
    let mut ports = slot.borrow_mut();
    let state = ports.get_mut(&id)?;
    if state.closed {
      return Some(false);
    }
    state.queue.push_back(queued.clone());
    Some(state.started)
  });
  match started {
    None => {
      EARLY_PORTS.with(|slot| {
        slot.borrow_mut().entry(id).or_default().push(queued);
      });
    }
    Some(true) => schedule(id),
    Some(false) => {}
  }
}

pub(crate) fn fail_port_message(id: u64) {
  let started = PORTS.with(|slot| {
    let mut ports = slot.borrow_mut();
    let state = ports.get_mut(&id)?;
    if state.closed {
      return Some(false);
    }
    state.failures = state.failures.saturating_add(1);
    Some(state.started)
  });
  match started {
    None => {
      EARLY_FAILURES.with(|slot| {
        let mut failures = slot.borrow_mut();
        let count = failures.entry(id).or_default();
        let next = count.saturating_add(1);
        *count = next;
      });
    }
    Some(true) => schedule(id),
    Some(false) => {}
  }
}

fn moved_port(scope: &mut v8::PinScope, old_id: u64) -> Result<u64, ()> {
  if let Some(new_id) =
    MOVED_PORTS.with(|slot| slot.borrow().get(&old_id).copied())
  {
    return Ok(new_id);
  }
  let (peer, queue, failures) = PORTS.with(|slot| {
    let mut ports = slot.borrow_mut();
    let state = ports.get_mut(&old_id).ok_or(())?;
    if state.closed {
      return Err(());
    }
    let peer = state.peer.take().ok_or(())?;
    state.closed = true;
    let queue = std::mem::take(&mut state.queue);
    let failures = state.failures;
    state.failures = 0;
    Ok((peer, queue, failures))
  })?;
  let new_id = alloc();
  PORTS.with(|slot| {
    let mut ports = slot.borrow_mut();
    if let Some(peer_state) = ports.get_mut(&peer)
      && peer_state.peer == Some(old_id)
    {
      peer_state.peer = Some(new_id);
    }
    ports.insert(
      new_id,
      PortState {
        peer: Some(peer),
        started: false,
        closed: false,
        wrapper: None,
        failures,
        queue,
      },
    );
  });
  bind_port(new_id);
  let _ = MessagePort::template(scope);
  let wrapper = unsafe {
    yuzuko_core::op::wrap_instance(
      scope,
      MessagePort {
        base: EventTarget::new(),
        id: new_id,
      },
    )
  };
  PORTS.with(|slot| {
    if let Some(state) = slot.borrow_mut().get_mut(&new_id) {
      state.wrapper = Some(v8::Global::new(scope, wrapper));
    }
  });
  MOVED_PORTS.with(|slot| slot.borrow_mut().insert(old_id, new_id));
  Ok(new_id)
}

#[repr(C)]
pub struct MessagePort {
  base: EventTarget,
  id: u64,
}

unsafe impl v8::cppgc::GarbageCollected for MessagePort {
  fn trace(&self, visitor: &mut v8::cppgc::Visitor) {
    v8::cppgc::GarbageCollected::trace(&self.base, visitor);
  }

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"MessagePort"
  }
}

#[op(
  inherit = EventTarget,
  inspect = ["active", "refed", "onmessage", "onmessageerror"]
)]
impl MessagePort {
  #[constructor]
  fn new() -> Result<Self, OpError> {
    Err(OpError::type_("Illegal constructor"))
  }

  fn post_message(
    &self,
    scope: &mut v8::PinScope,
    message: v8::Local<v8::Value>,
    transfer: Option<v8::Local<v8::Value>>,
  ) -> Result<(), OpError> {
    let transfer = transfer.unwrap_or_else(|| v8::undefined(scope).into());
    let message = v8::Local::new(scope, v8::Global::new(scope, message));
    let transfer = v8::Local::new(scope, v8::Global::new(scope, transfer));
    // HTML message port post message steps: a closed or unentangled port
    // has no target. The message is still serialized with its transfer
    // list, so a transferred buffer detaches and a bad value throws, and
    // then nothing is queued.
    let peer = PORTS.with(|slot| {
      let ports = slot.borrow();
      ports
        .get(&self.id)
        .filter(|state| !state.closed)
        .and_then(|state| state.peer)
    });
    let (objects, buffers) = transfer_parts(scope, transfer)?;
    if let Some(peer) = peer
      && port_agent(peer) != yuzuko_core::agent_id()
    {
      return post_port_remote(scope, peer, message, &objects, &buffers);
    }
    // A transferred ArrayBuffer moves its backing store. Detach afterward
    // so the sender's buffer is empty and the peer keeps that allocation.
    // `MessagePort` and `OffscreenCanvas` move during that clone.
    // `ImageBitmap`, `AudioData`, and `VideoFrame` move their pixels or
    // samples and close after the clone succeeds.
    let listed = objects.iter().map(v8::Global::clone).collect::<Vec<_>>();
    capture_ports(true);
    let cloned = {
      let _guard = yuzuko_core::StructuredTransferGuard::install(objects);
      // Same-agent peers share a SharedArrayBuffer. `structuredClone` does not.
      let _share = yuzuko_core::AllowSharedArrayBufferClone::enter();
      match yuzuko_core::clone_value_with_buffers(scope, message, &buffers) {
        Ok(cloned) => {
          yuzuko_core::detach_structured_transfers(scope);
          Ok(cloned)
        }
        Err(error) => Err(error),
      }
    };
    let _reached = take_captured_ports();
    let cloned = match cloned {
      Ok(cloned) => cloned,
      Err(Some(thrown)) => return Err(OpError::Thrown(Box::new(thrown))),
      Err(None) => {
        return Err(OpError::dom(
          "DataCloneError",
          "The object could not be cloned.",
        ));
      }
    };
    // The event lists the transfer list's ports as the receiver sees them.
    let ports = port_wrappers_for_transfer(scope, &listed)?;
    for buffer in &buffers {
      let buffer = v8::Local::new(scope, buffer);
      let _ = buffer.detach(None);
    }
    let Some(peer) = peer else {
      return Ok(());
    };
    let queued = Queued {
      data: v8::Global::new(scope, cloned),
      ports,
    };
    let started = PORTS.with(|slot| {
      let mut ports = slot.borrow_mut();
      let Some(state) = ports.get_mut(&peer) else {
        return false;
      };
      state.queue.push_back(queued);
      state.started
    });
    if started {
      schedule(peer);
    }
    Ok(())
  }

  fn start(&self, _scope: &mut v8::PinScope) {
    start_port(self.id);
  }

  fn close(&self) {
    let peer = PORTS.with(|slot| {
      let mut ports = slot.borrow_mut();
      let state = ports.get_mut(&self.id)?;
      state.closed = true;
      state.peer.take()
    });
    if let Some(peer) = peer {
      PORTS.with(|slot| {
        if let Some(state) = slot.borrow_mut().get_mut(&peer) {
          state.peer = None;
        }
      });
    }
  }
}

pub struct MessageChannel {
  port1: v8::TracedReference<v8::Object>,
  port2: v8::TracedReference<v8::Object>,
}

unsafe impl v8::cppgc::GarbageCollected for MessageChannel {
  fn trace(&self, visitor: &mut v8::cppgc::Visitor) {
    visitor.trace(&self.port1);
    visitor.trace(&self.port2);
  }

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"MessageChannel"
  }
}

#[op(inspect = ["port1", "port2"])]
impl MessageChannel {
  #[constructor]
  fn new(scope: &mut v8::PinScope) -> Self {
    let (first, second) = entangled(scope);
    Self {
      port1: traced(scope, first),
      port2: traced(scope, second),
    }
  }

  #[getter]
  fn port1(&self, scope: &mut v8::PinScope) -> Js {
    traced_js(scope, &self.port1)
  }

  #[getter]
  fn port2(&self, scope: &mut v8::PinScope) -> Js {
    traced_js(scope, &self.port2)
  }
}

#[repr(C)]
pub struct MessageEvent {
  base: Event,
  data: RefCell<Option<v8::TracedReference<v8::Value>>>,
  /// Frozen array of the transfer-list `MessagePort`s, in list order.
  /// On the main global, a port the value did not reach is that same object.
  ports: RefCell<v8::TracedReference<v8::Object>>,
  origin: RefCell<String>,
  last_event_id: RefCell<String>,
  /// `MessagePort` or the main realm from `initMessageEvent`.
  /// Constructed and posted events stay null.
  source: RefCell<Option<v8::TracedReference<v8::Object>>>,
}

unsafe impl v8::cppgc::GarbageCollected for MessageEvent {
  fn trace(&self, visitor: &mut v8::cppgc::Visitor) {
    v8::cppgc::GarbageCollected::trace(&self.base, visitor);
    if let Ok(ports) = self.ports.try_borrow() {
      visitor.trace(&*ports);
    }
    if let Ok(data) = self.data.try_borrow() {
      visitor.trace(&*data);
    }
    if let Ok(source) = self.source.try_borrow() {
      visitor.trace(&*source);
    }
  }

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"MessageEvent"
  }
}

#[op(
  inherit = Event,
  inspect_after = [
    "data",
    "origin",
    "lastEventId",
    "source",
    "ports",
  ]
)]
impl MessageEvent {
  #[constructor]
  fn new(
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<Self, OpError> {
    let base = Event::from_args(scope, args, false)?;
    let init = crate::util::init_arg(args);
    let data = crate::util::dict_get(scope, init, "data")
      .unwrap_or_else(|| v8::undefined(scope).into());
    let origin = crate::util::dict_string(scope, init, "origin");
    let last_event_id = crate::util::dict_string(scope, init, "lastEventId");
    let ports = port_array_from_init(scope, init)?;
    Ok(Self {
      base,
      data: RefCell::new(Some(traced(scope, data))),
      ports: RefCell::new(ports),
      origin: RefCell::new(origin),
      last_event_id: RefCell::new(last_event_id),
      source: RefCell::new(None),
    })
  }

  #[construct_hook]
  fn finish(&self, scope: &mut v8::PinScope, object: v8::Local<v8::Object>) {
    let _ = self;
    define_is_trusted(scope, object);
  }

  #[getter]
  fn data(&self, scope: &mut v8::PinScope) -> Js {
    let value = self
      .data
      .borrow()
      .as_ref()
      .and_then(|reference| traced_local(scope, reference))
      .unwrap_or_else(|| v8::undefined(scope).into());
    Js(v8::Global::new(scope, value))
  }

  #[getter]
  fn origin(&self) -> String {
    self.origin.borrow().clone()
  }

  #[getter]
  fn last_event_id(&self) -> String {
    self.last_event_id.borrow().clone()
  }

  #[getter]
  fn source(&self, scope: &mut v8::PinScope) -> Js {
    let object = self
      .source
      .borrow()
      .as_ref()
      .and_then(|reference| traced_local(scope, reference));
    match object {
      Some(object) => {
        let value: v8::Local<v8::Value> = object.into();
        Js(v8::Global::new(scope, value))
      }
      None => Js::null(scope),
    }
  }

  #[getter]
  fn ports(&self, scope: &mut v8::PinScope) -> Js {
    let ports = self.ports.borrow();
    let value = traced_local(scope, &ports)
      .map(v8::Local::<v8::Value>::from)
      .unwrap_or_else(|| v8::Array::new(scope, 0).into());
    Js(v8::Global::new(scope, value))
  }

  /// `source` is a `MessagePort`, the main realm, or null.
  /// `ports` replaces the frozen array.
  #[length(1)]
  fn init_message_event(
    &self,
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<(), OpError> {
    let type_name = crate::util::try_string(scope, args.get(0))?;
    let bubbles = message_flag(scope, args, 1);
    let cancelable = message_flag(scope, args, 2);
    let data = message_data(scope, args, 3)?;
    let origin = message_text(scope, args, 4)?;
    let last_event_id = message_text(scope, args, 5)?;
    let source = message_source(scope, args, 6)?;
    let ports = port_sequence(scope, args, 7)?;
    if !self.base.init_type(type_name, bubbles, cancelable) {
      return Ok(());
    }
    *self.data.borrow_mut() = Some(traced(scope, data));
    *self.origin.borrow_mut() = origin;
    *self.last_event_id.borrow_mut() = last_event_id;
    *self.source.borrow_mut() = source.map(|object| traced(scope, object));
    *self.ports.borrow_mut() = port_array(scope, &ports);
    Ok(())
  }
}

#[cfg(any(feature = "fetch", feature = "socket"))]
pub(crate) fn trusted_message<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  type_name: &str,
  data: v8::Local<'_, v8::Value>,
  origin: String,
  last_event_id: String,
) -> v8::Local<'s, v8::Object> {
  let event = MessageEvent {
    base: Event::new(type_name, false, false, false, true),
    data: RefCell::new(Some(traced(scope, data))),
    ports: RefCell::new(empty_port_array(scope)),
    origin: RefCell::new(origin),
    last_event_id: RefCell::new(last_event_id),
    source: RefCell::new(None),
  };
  let object = unsafe { yuzuko_core::op::wrap_instance(scope, event) };
  define_is_trusted(scope, object);
  object
}

type MovedParts = (
  Vec<v8::Global<v8::Object>>,
  Vec<v8::Global<v8::ArrayBuffer>>,
);

pub(crate) fn transfer_parts<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  value: v8::Local<'s, v8::Value>,
) -> Result<MovedParts, OpError> {
  if value.is_null_or_undefined() {
    return Ok((Vec::new(), Vec::new()));
  }
  let list = transfer_array(scope, value)?;
  match yuzuko_core::collect_transfers(
    scope,
    list,
    yuzuko_core::TransferKind::Message,
  ) {
    Ok(transfers) => Ok((transfers.objects, transfers.buffers)),
    Err(yuzuko_core::TransferReject::Pending) => {
      Err(OpError::type_("The transfer list is not readable"))
    }
    Err(yuzuko_core::TransferReject::DataClone(message)) => {
      Err(OpError::dom("DataCloneError", message))
    }
    Err(yuzuko_core::TransferReject::Type(message)) => {
      Err(OpError::type_(message))
    }
  }
}

/// Moves `MessagePort` entries the value serializer left open.
///
/// A port the value reached is already closed and keeps its id. The returned
/// ids follow the transfer list. Other host objects are left to their own
/// writers.
pub(crate) fn transfer_listed_ports(
  scope: &mut v8::PinScope,
  objects: &[v8::Global<v8::Object>],
) -> Result<Vec<u64>, OpError> {
  let mut ids = Vec::new();
  for object in objects {
    let object = v8::Local::new(scope, object);
    let id = {
      let Some(port) =
        (unsafe { yuzuko_core::op::host_ref::<MessagePort>(scope, object) })
      else {
        continue;
      };
      port.get().id
    };
    let open =
      PORTS.with(|slot| slot.borrow().get(&id).map(|state| !state.closed));
    match open {
      Some(true) => {
        transfer_port(scope, object).map_err(|()| {
          OpError::dom("DataCloneError", "The object could not be cloned.")
        })?;
      }
      Some(false) => {}
      None => {
        return Err(OpError::dom(
          "DataCloneError",
          "The object could not be cloned.",
        ));
      }
    }
    ids.push(id);
  }
  Ok(ids)
}

/// `MessagePort`s from the caller's transfer list, in that list's order.
///
/// A port the value reached was moved to a new id, and the event holds that
/// wrapper. A port the value did not reach stays the original object and
/// stays open. A stream's own channel port is not in `objects`.
fn port_wrappers_for_transfer(
  scope: &mut v8::PinScope,
  objects: &[v8::Global<v8::Object>],
) -> Result<Vec<v8::Global<v8::Object>>, OpError> {
  let mut ports = Vec::new();
  for object in objects {
    let object = v8::Local::new(scope, object);
    let old_id = {
      let Some(port) =
        (unsafe { yuzuko_core::op::host_ref::<MessagePort>(scope, object) })
      else {
        continue;
      };
      port.get().id
    };
    let closed =
      PORTS.with(|slot| slot.borrow().get(&old_id).map(|state| state.closed));
    match closed {
      Some(true) => {
        let Some(new_id) =
          MOVED_PORTS.with(|slot| slot.borrow().get(&old_id).copied())
        else {
          return Err(OpError::dom(
            "DataCloneError",
            "The object could not be cloned.",
          ));
        };
        let wrapper =
          receive_transferred_port(scope, new_id).map_err(|()| {
            OpError::dom("DataCloneError", "The object could not be cloned.")
          })?;
        ports.push(wrapper);
      }
      Some(false) => ports.push(v8::Global::new(scope, object)),
      None => {
        return Err(OpError::dom(
          "DataCloneError",
          "The object could not be cloned.",
        ));
      }
    }
  }
  Ok(ports)
}

fn transfer_array<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  value: v8::Local<'s, v8::Value>,
) -> Result<v8::Local<'s, v8::Array>, OpError> {
  if let Ok(list) = v8::Local::<v8::Array>::try_from(value) {
    return Ok(list);
  }
  let Ok(object) = v8::Local::<v8::Object>::try_from(value) else {
    return Err(OpError::type_("The transfer list is not an array"));
  };
  let key = v8::String::new(scope, "transfer").unwrap();
  let Some(transfer) = object.get(scope, key.into()) else {
    return Err(OpError::type_("The transfer list is not an array"));
  };
  if transfer.is_undefined() {
    return Ok(v8::Array::new(scope, 0));
  }
  v8::Local::<v8::Array>::try_from(transfer)
    .map_err(|_| OpError::type_("The transfer list is not an array"))
}

fn post_port_remote(
  scope: &mut v8::PinScope,
  peer: u64,
  message: v8::Local<v8::Value>,
  objects: &[v8::Global<v8::Object>],
  buffers: &[v8::Global<v8::ArrayBuffer>],
) -> Result<(), OpError> {
  let agent = port_agent(peer);
  let parcel = match yuzuko_core::serialize_parcel(
    scope, message, objects, buffers, agent,
  ) {
    Ok(parcel) => parcel,
    Err(Some(thrown)) => {
      return Err(OpError::Thrown(Box::new(thrown)));
    }
    Err(None) => {
      return Err(OpError::dom(
        "DataCloneError",
        "The object could not be cloned.",
      ));
    }
  };
  // A port the value did not reach is still open, and moving it has to
  // target the receiving agent.
  let ports = {
    let _restore = crate::worker::CrossAgentRestore::enter(true, agent);
    transfer_listed_ports(scope, objects)?
  };
  yuzuko_core::post_inbound(
    agent,
    yuzuko_core::Inbound::Port {
      port: peer,
      parcel,
      ports,
    },
  );
  Ok(())
}

/// Setting `onmessage` starts the port, including an assignment of `null`.
pub(crate) fn note_message_handler(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  type_name: &str,
) {
  if type_name != "message" {
    return;
  }
  let Some(host) =
    (unsafe { yuzuko_core::op::host_ref::<MessagePort>(scope, object) })
  else {
    return;
  };
  start_port(host.get().id);
}

fn start_port(id: u64) {
  let started = PORTS.with(|slot| {
    let mut ports = slot.borrow_mut();
    let Some(state) = ports.get_mut(&id) else {
      return false;
    };
    if state.closed || state.started {
      return false;
    }
    state.started = true;
    !state.queue.is_empty() || state.failures > 0
  });
  if started {
    schedule(id);
  }
}

pub fn entangled_ports<'s>(
  scope: &mut v8::PinScope<'s, '_>,
) -> (v8::Local<'s, v8::Object>, v8::Local<'s, v8::Object>) {
  entangled(scope)
}

fn entangled<'s>(
  scope: &mut v8::PinScope<'s, '_>,
) -> (v8::Local<'s, v8::Object>, v8::Local<'s, v8::Object>) {
  let _ = MessagePort::template(scope);
  let first_id = alloc();
  let second_id = alloc();
  PORTS.with(|slot| {
    let mut ports = slot.borrow_mut();
    ports.insert(
      first_id,
      PortState {
        peer: Some(second_id),
        started: false,
        closed: false,
        wrapper: None,
        failures: 0,
        queue: VecDeque::new(),
      },
    );
    ports.insert(
      second_id,
      PortState {
        peer: Some(first_id),
        started: false,
        closed: false,
        wrapper: None,
        failures: 0,
        queue: VecDeque::new(),
      },
    );
  });
  bind_port(first_id);
  bind_port(second_id);
  let first = unsafe {
    yuzuko_core::op::wrap_instance(
      scope,
      MessagePort {
        base: EventTarget::new(),
        id: first_id,
      },
    )
  };
  let second = unsafe {
    yuzuko_core::op::wrap_instance(
      scope,
      MessagePort {
        base: EventTarget::new(),
        id: second_id,
      },
    )
  };
  PORTS.with(|slot| {
    let mut ports = slot.borrow_mut();
    if let Some(state) = ports.get_mut(&first_id) {
      state.wrapper = Some(v8::Global::new(scope, first));
    }
    if let Some(state) = ports.get_mut(&second_id) {
      state.wrapper = Some(v8::Global::new(scope, second));
    }
  });
  (first, second)
}

fn schedule(id: u64) {
  yuzuko_core::enqueue(move |scope| deliver(scope, id));
}

fn deliver(scope: &mut v8::PinScope, id: u64) {
  let (pending, failures) = PORTS.with(|slot| {
    let mut ports = slot.borrow_mut();
    let Some(state) = ports.get_mut(&id) else {
      return (Vec::new(), 0);
    };
    if !state.started || state.closed {
      return (Vec::new(), 0);
    }
    let pending = state.queue.drain(..).collect::<Vec<_>>();
    let failures = state.failures;
    state.failures = 0;
    (pending, failures)
  });
  let wrapper = PORTS.with(|slot| {
    slot
      .borrow()
      .get(&id)
      .and_then(|state| state.wrapper.clone())
  });
  let Some(wrapper) = wrapper else {
    return;
  };
  for queued in pending {
    let target = v8::Local::new(scope, &wrapper);
    adopt_shared_buffers(scope, id, &queued.data);
    let event =
      message_event(scope, "message", &queued.data, &queued.ports, "");
    let _ = target::dispatch(scope, target, event);
  }
  for _ in 0..failures {
    let target = v8::Local::new(scope, &wrapper);
    let event = message_error_event(scope);
    let _ = target::dispatch(scope, target, event);
  }
}

/// `instanceof SharedArrayBuffer` walks the receiver realm's prototype.
///
/// The clone is built in the sender realm. Point each shared buffer at the
/// `onmessage` handler's `SharedArrayBuffer.prototype` before delivery.
fn adopt_shared_buffers(
  scope: &mut v8::PinScope,
  id: u64,
  data: &v8::Global<v8::Value>,
) {
  let wrapper = PORTS.with(|slot| {
    slot
      .borrow()
      .get(&id)
      .and_then(|state| state.wrapper.clone())
  });
  let Some(wrapper) = wrapper else {
    return;
  };
  let wrapper = v8::Local::new(scope, wrapper);
  let Some(host) =
    (unsafe { yuzuko_core::op::host_ref::<MessagePort>(scope, wrapper) })
  else {
    return;
  };
  let handler = host.get().base.event_handler(scope, "message");
  if handler.is_null() {
    return;
  }
  let Ok(handler) = v8::Local::<v8::Object>::try_from(handler) else {
    return;
  };
  let Some(context) = handler.get_creation_context(scope) else {
    return;
  };
  let value = v8::Local::new(scope, data);
  let scope = &mut v8::ContextScope::new(scope, context);
  let mut seen = HashSet::new();
  retarget_shared(scope, value, &mut seen, 0);
}

fn retarget_shared(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
  seen: &mut HashSet<i32>,
  depth: u32,
) {
  if depth > 8 {
    return;
  }
  if let Ok(buffer) = v8::Local::<v8::SharedArrayBuffer>::try_from(value) {
    set_shared_prototype(scope, buffer);
    return;
  }
  let Ok(object) = v8::Local::<v8::Object>::try_from(value) else {
    return;
  };
  if object.is_api_wrapper() || value.is_function() {
    return;
  }
  let hash = object.get_identity_hash().get();
  if !seen.insert(hash) {
    return;
  }
  if let Ok(array) = v8::Local::<v8::Array>::try_from(object) {
    for index in 0..array.length() {
      let Some(item) = array.get_index(scope, index) else {
        continue;
      };
      retarget_shared(scope, item, seen, depth + 1);
    }
    return;
  }
  let Some(names) =
    object.get_own_property_names(scope, v8::GetPropertyNamesArgs::default())
  else {
    return;
  };
  for index in 0..names.length() {
    v8::tc_scope!(let tc, scope);
    let Some(key) = names.get_index(tc, index) else {
      continue;
    };
    let Some(item) = object.get(tc, key) else {
      if tc.has_caught() {
        tc.reset();
      }
      continue;
    };
    retarget_shared(tc, item, seen, depth + 1);
    if tc.has_caught() {
      tc.reset();
    }
  }
}

fn set_shared_prototype(
  scope: &mut v8::PinScope,
  buffer: v8::Local<v8::SharedArrayBuffer>,
) {
  let global = scope.get_current_context().global(scope);
  let Some(name) = v8::String::new(scope, "SharedArrayBuffer") else {
    return;
  };
  let Some(ctor) = global.get(scope, name.into()) else {
    return;
  };
  let Ok(ctor) = v8::Local::<v8::Object>::try_from(ctor) else {
    return;
  };
  let Some(key) = v8::String::new(scope, "prototype") else {
    return;
  };
  let Some(prototype) = ctor.get(scope, key.into()) else {
    return;
  };
  let _ = buffer.set_prototype(scope, prototype);
}

pub(crate) fn message_error_event<'s>(
  scope: &mut v8::PinScope<'s, '_>,
) -> v8::Local<'s, v8::Object> {
  let data: v8::Local<v8::Value> = v8::null(scope).into();
  let data = v8::Global::new(scope, data);
  message_event(scope, "messageerror", &data, &[], "")
}

fn message_event<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  type_name: &str,
  data: &v8::Global<v8::Value>,
  ports: &[v8::Global<v8::Object>],
  origin: &str,
) -> v8::Local<'s, v8::Object> {
  let _ = MessageEvent::template(scope);
  let data = v8::Local::new(scope, data);
  let data = yuzuko_core::op::root(scope, data);
  let data = traced(scope, data);
  let ports = port_array(scope, ports);
  let origin = origin.to_string();
  let event = unsafe {
    yuzuko_core::op::wrap_instance(
      scope,
      MessageEvent {
        base: Event::new(type_name, false, false, false, true),
        data: RefCell::new(Some(data)),
        ports: RefCell::new(ports),
        origin: RefCell::new(origin),
        last_event_id: RefCell::new(String::new()),
        source: RefCell::new(None),
      },
    )
  };
  define_is_trusted(scope, event);
  event
}

fn empty_port_array(
  scope: &mut v8::PinScope,
) -> v8::TracedReference<v8::Object> {
  port_array(scope, &[])
}

fn port_array(
  scope: &mut v8::PinScope,
  ports: &[v8::Global<v8::Object>],
) -> v8::TracedReference<v8::Object> {
  let length = i32::try_from(ports.len()).unwrap_or(i32::MAX);
  let array = v8::Array::new(scope, length);
  for (index, port) in ports.iter().enumerate() {
    let Ok(index) = u32::try_from(index) else {
      break;
    };
    let port = v8::Local::new(scope, port);
    let _ =
      yuzuko_core::define_element(scope, array.into(), index, port.into());
  }
  let object: v8::Local<v8::Object> = array.into();
  let _ = object.set_integrity_level(scope, v8::IntegrityLevel::Frozen);
  traced(scope, object)
}

fn message_present<'s>(
  args: &v8::FunctionCallbackArguments<'s>,
  index: i32,
) -> Option<v8::Local<'s, v8::Value>> {
  if index < 0 || index >= args.length() {
    return None;
  }
  let value = args.get(index);
  if value.is_undefined() {
    None
  } else {
    Some(value)
  }
}

fn message_flag(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
  index: i32,
) -> bool {
  message_present(args, index).is_some_and(|value| value.boolean_value(scope))
}

fn message_data<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  args: &v8::FunctionCallbackArguments<'_>,
  index: i32,
) -> Result<v8::Local<'s, v8::Value>, OpError> {
  match message_present(args, index) {
    None => Ok(v8::null(scope).into()),
    Some(value) => Ok(yuzuko_core::op::root(scope, value)),
  }
}

fn message_text(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
  index: i32,
) -> Result<String, OpError> {
  let Some(value) = message_present(args, index) else {
    return Ok(String::new());
  };
  usv_string(scope, value)
}

fn message_source<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  args: &v8::FunctionCallbackArguments<'_>,
  index: i32,
) -> Result<Option<v8::Local<'s, v8::Object>>, OpError> {
  let Some(value) = message_present(args, index) else {
    return Ok(None);
  };
  if value.is_null() {
    return Ok(None);
  }
  let Ok(object) = v8::Local::<v8::Object>::try_from(value) else {
    return Err(source_type());
  };
  let is_port =
    unsafe { yuzuko_core::op::host_ref::<MessagePort>(scope, object) }
      .is_some();
  // The main realm is the WindowProxy. A dedicated worker global is not.
  if is_port || realm_window(scope, object) {
    return Ok(Some(yuzuko_core::op::root(scope, object)));
  }
  Err(source_type())
}

fn realm_window(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
) -> bool {
  !yuzuko_core::is_dedicated_worker() && crate::util::is_global(scope, object)
}

fn source_type() -> OpError {
  OpError::type_("The provided value is not of type 'MessagePort or Window'.")
}

fn port_sequence(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
  index: i32,
) -> Result<Vec<v8::Global<v8::Object>>, OpError> {
  let Some(value) = message_present(args, index) else {
    return Ok(Vec::new());
  };
  let object = port_to_object(scope, value)?;
  let symbol = v8::Symbol::get_iterator(scope);
  let method = port_get(scope, object, symbol.into())?;
  if method.is_null_or_undefined() {
    return Err(OpError::type_("The ports list is not iterable"));
  }
  let Ok(method) = v8::Local::<v8::Function>::try_from(method) else {
    return Err(OpError::type_("@@iterator is not callable"));
  };
  let iterator = port_call(scope, method, object.into(), &[])?;
  let Ok(iterator) = v8::Local::<v8::Object>::try_from(iterator) else {
    return Err(OpError::type_("The ports list is not iterable"));
  };
  let mut ports = Vec::new();
  loop {
    let next = port_get_name(scope, iterator, "next")?;
    let Ok(next) = v8::Local::<v8::Function>::try_from(next) else {
      return Err(OpError::type_("The ports list is not iterable"));
    };
    let step = port_call(scope, next, iterator.into(), &[])?;
    let Ok(step) = v8::Local::<v8::Object>::try_from(step) else {
      return Err(OpError::type_("The ports list is not iterable"));
    };
    let done = port_get_name(scope, step, "done")?;
    if done.boolean_value(scope) {
      return Ok(ports);
    }
    let item = port_get_name(scope, step, "value")?;
    let Ok(item) = v8::Local::<v8::Object>::try_from(item) else {
      return Err(OpError::type_("A message port is required"));
    };
    let is_port =
      unsafe { yuzuko_core::op::host_ref::<MessagePort>(scope, item) }
        .is_some();
    if !is_port {
      return Err(OpError::type_("A message port is required"));
    }
    ports.push(v8::Global::new(scope, item));
  }
}

enum PortGrab<T> {
  Value(T),
  Thrown(v8::Global<v8::Value>),
  Missing,
}

fn port_to_object<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  value: v8::Local<v8::Value>,
) -> Result<v8::Local<'s, v8::Object>, OpError> {
  let captured = {
    v8::tc_scope!(let tc, scope);
    match value.to_object(tc) {
      Some(object) => PortGrab::Value(v8::Global::new(tc, object)),
      None => match tc.exception() {
        Some(exception) => PortGrab::Thrown(v8::Global::new(tc, exception)),
        None => PortGrab::Missing,
      },
    }
  };
  match captured {
    PortGrab::Value(object) => Ok(v8::Local::new(scope, object)),
    PortGrab::Thrown(exception) => {
      Err(OpError::thrown(scope, v8::Local::new(scope, exception)))
    }
    PortGrab::Missing => Err(OpError::type_("The ports list is not iterable")),
  }
}

fn port_get_name<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  object: v8::Local<v8::Object>,
  name: &str,
) -> Result<v8::Local<'s, v8::Value>, OpError> {
  let Some(key) = v8::String::new(scope, name) else {
    return Err(OpError::type_("The ports list is not iterable"));
  };
  port_get(scope, object, key.into())
}

fn port_get<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  object: v8::Local<v8::Object>,
  key: v8::Local<v8::Value>,
) -> Result<v8::Local<'s, v8::Value>, OpError> {
  let captured = {
    v8::tc_scope!(let tc, scope);
    match object.get(tc, key) {
      Some(value) => PortGrab::Value(v8::Global::new(tc, value)),
      None => match tc.exception() {
        Some(exception) => PortGrab::Thrown(v8::Global::new(tc, exception)),
        None => PortGrab::Missing,
      },
    }
  };
  match captured {
    PortGrab::Value(value) => Ok(v8::Local::new(scope, value)),
    PortGrab::Thrown(exception) => {
      Err(OpError::thrown(scope, v8::Local::new(scope, exception)))
    }
    PortGrab::Missing => Err(OpError::type_("The ports list is not iterable")),
  }
}

fn port_call<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  function: v8::Local<v8::Function>,
  receiver: v8::Local<v8::Value>,
  call_args: &[v8::Local<v8::Value>],
) -> Result<v8::Local<'s, v8::Value>, OpError> {
  let captured = {
    v8::tc_scope!(let tc, scope);
    match function.call(tc, receiver, call_args) {
      Some(value) => PortGrab::Value(v8::Global::new(tc, value)),
      None => match tc.exception() {
        Some(exception) => PortGrab::Thrown(v8::Global::new(tc, exception)),
        None => PortGrab::Missing,
      },
    }
  };
  match captured {
    PortGrab::Value(value) => Ok(v8::Local::new(scope, value)),
    PortGrab::Thrown(exception) => {
      Err(OpError::thrown(scope, v8::Local::new(scope, exception)))
    }
    PortGrab::Missing => Err(OpError::type_("The ports list is not iterable")),
  }
}

fn port_array_from_init(
  scope: &mut v8::PinScope,
  init: Option<v8::Local<v8::Value>>,
) -> Result<v8::TracedReference<v8::Object>, OpError> {
  let Some(value) = crate::util::dict_get(scope, init, "ports") else {
    return Ok(empty_port_array(scope));
  };
  if value.is_undefined() {
    return Ok(empty_port_array(scope));
  }
  let Ok(list) = v8::Local::<v8::Array>::try_from(value) else {
    return Err(OpError::type_("The ports list is not an array"));
  };
  let length = list.length();
  let mut ports = Vec::with_capacity(usize::try_from(length).unwrap_or(0));
  for index in 0..length {
    let Some(item) = list.get_index(scope, index) else {
      return Err(OpError::type_("The ports list is not readable"));
    };
    let Ok(object) = v8::Local::<v8::Object>::try_from(item) else {
      return Err(OpError::type_("A message port is required"));
    };
    let is_port =
      unsafe { yuzuko_core::op::host_ref::<MessagePort>(scope, object) }
        .is_some();
    if !is_port {
      return Err(OpError::type_("A message port is required"));
    }
    ports.push(v8::Global::new(scope, object));
  }
  Ok(port_array(scope, &ports))
}

fn traced_js(
  scope: &mut v8::PinScope,
  reference: &v8::TracedReference<v8::Object>,
) -> Js {
  let value = traced_local(scope, reference)
    .map(v8::Local::<v8::Value>::from)
    .unwrap_or_else(|| v8::undefined(scope).into());
  Js(v8::Global::new(scope, value))
}

pub(crate) fn install(scope: &mut v8::PinScope) {
  MessagePort::install(scope);
  let constructor = MessagePort::template(scope)
    .get_function(scope)
    .expect("MessagePort");
  crate::target::define_prototype_event_handlers(
    scope,
    constructor,
    &["message", "messageerror"],
  );
  MessageEvent::install(scope);
  MessageChannel::install(scope);
  if yuzuko_core::is_dedicated_worker() {
    return;
  }
  let global = yuzuko_core::global(scope);
  yuzuko_core::set_function(scope, global, "postMessage", post_to_self);
}

fn post_to_self(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut _ret: v8::ReturnValue,
) {
  if let Err(error) = queue_self_message(scope, &args) {
    error.throw(scope);
  }
}

fn queue_self_message(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
) -> Result<(), OpError> {
  if args.length() < 1 {
    return Err(OpError::type_("postMessage requires a message"));
  }
  let (origin, transfer) = message_target(scope, args)?;
  // HTML serializes before it discards a parsed origin that will not match.
  // `"/"` and `"*"` match. An unparsable string throws before the clone.
  let deliver = target_origin_matches(&origin)?;
  let message = args.get(0);
  let message = v8::Local::new(scope, v8::Global::new(scope, message));
  let transfer = match transfer {
    Some(transfer) => v8::Local::new(scope, transfer),
    None => v8::undefined(scope).into(),
  };
  let (objects, buffers) = transfer_parts(scope, transfer)?;
  // `read_port_clone` records a `MessagePort` reached from the value, and
  // a stream's own channel port. The event lists the transfer list instead.
  let listed = objects.iter().map(v8::Global::clone).collect::<Vec<_>>();
  capture_ports(true);
  let cloned = {
    let _guard = yuzuko_core::StructuredTransferGuard::install(objects);
    match yuzuko_core::clone_value_with_buffers(scope, message, &buffers) {
      Ok(cloned) => {
        yuzuko_core::detach_structured_transfers(scope);
        Ok(cloned)
      }
      Err(error) => Err(error),
    }
  };
  let _reached = take_captured_ports();
  let cloned = match cloned {
    Ok(cloned) => cloned,
    Err(Some(thrown)) => return Err(OpError::Thrown(Box::new(thrown))),
    Err(None) => {
      return Err(OpError::dom(
        "DataCloneError",
        "The object could not be cloned.",
      ));
    }
  };
  let ports = port_wrappers_for_transfer(scope, &listed)?;
  for buffer in &buffers {
    let buffer = v8::Local::new(scope, buffer);
    let _ = buffer.detach(None);
  }
  let data = v8::Global::new(scope, cloned);
  let origin = realm_origin();
  if !deliver {
    return Ok(());
  }
  // A zero-delay timer runs before a `setTimeout` scheduled after this
  // call returns. A macrotask queued behind timers would leave the message
  // for the next test's listener.
  yuzuko_core::schedule_host(std::time::Duration::ZERO, move |scope| {
    let event = message_event(scope, "message", &data, &ports, &origin);
    let global = yuzuko_core::global(scope);
    let _ = target::dispatch(scope, global, event);
  });
  Ok(())
}

fn message_target(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
) -> Result<(String, Option<v8::Global<v8::Value>>), OpError> {
  if args.length() < 2 {
    return Ok(("/".to_string(), None));
  }
  let second = args.get(1);
  if second.is_null() || !second.is_object() {
    let origin = usv_string(scope, second)?;
    let transfer =
      (args.length() > 2).then(|| v8::Global::new(scope, args.get(2)));
    return Ok((origin, transfer));
  }
  let Ok(object) = v8::Local::<v8::Object>::try_from(second) else {
    return Err(OpError::type_("The message options are not an object"));
  };
  let origin = option_origin(scope, object)?;
  let transfer = option_transfer(scope, object)?;
  Ok((origin, transfer))
}

fn option_origin(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
) -> Result<String, OpError> {
  let value = read_option(scope, object, "targetOrigin")?;
  let value = v8::Local::new(scope, value);
  if value.is_undefined() {
    return Ok("/".to_string());
  }
  usv_string(scope, value)
}

fn option_transfer(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
) -> Result<Option<v8::Global<v8::Value>>, OpError> {
  let value = read_option(scope, object, "transfer")?;
  let local = v8::Local::new(scope, &value);
  if local.is_undefined() {
    Ok(None)
  } else {
    Ok(Some(value))
  }
}

fn read_option(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
) -> Result<v8::Global<v8::Value>, OpError> {
  v8::tc_scope!(let tc, scope);
  let Some(key) = v8::String::new(tc, name) else {
    return Err(OpError::type_("The message options are not readable"));
  };
  match object.get(tc, key.into()) {
    Some(value) => Ok(v8::Global::new(tc, value)),
    None => Err(match tc.exception() {
      Some(thrown) => OpError::thrown(tc, thrown),
      None => OpError::type_("The message options are not readable"),
    }),
  }
}

fn usv_string(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<String, OpError> {
  v8::tc_scope!(let tc, scope);
  let Some(text) = value.to_string(tc) else {
    return Err(match tc.exception() {
      Some(thrown) => OpError::thrown(tc, thrown),
      None => OpError::type_("The target origin is not a string"),
    });
  };
  Ok(text.to_rust_string_lossy(tc))
}

/// `Ok(true)` queues the message. `Ok(false)` has already been serialized.
///
/// A single `"/"` is the incumbent origin. `"*"` matches every origin.
/// Any other string is a URL, and a parse failure is `SyntaxError`.
fn target_origin_matches(target: &str) -> Result<bool, OpError> {
  if target == "/" || target == "*" {
    return Ok(true);
  }
  let Ok(parsed) = url::Url::parse(target) else {
    return Err(OpError::dom(
      "SyntaxError",
      "The target origin is not a URL.",
    ));
  };
  let Some(entry) = yuzuko_core::entry_url() else {
    return Ok(false);
  };
  let Ok(current) = url::Url::parse(&entry) else {
    return Ok(false);
  };
  Ok(parsed.origin() == current.origin())
}

fn realm_origin() -> String {
  let Some(entry) = yuzuko_core::entry_url() else {
    return "null".to_string();
  };
  let Ok(url) = url::Url::parse(&entry) else {
    return "null".to_string();
  };
  url.origin().ascii_serialization()
}
