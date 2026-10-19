//! `MessageChannel`, `MessagePort`, and `MessageEvent`.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{LazyLock, Mutex};

use yuzuko_core::op::Js;
use yuzuko_core::op::OpError;
use yuzuko_core::op::traced;
use yuzuko_core::op::traced_local;
use yuzuko_macros::op;

use crate::event::Event;
use crate::event::define_is_trusted;
use crate::target::{self, EventTarget};

struct PortState {
  peer: Option<u64>,
  started: bool,
  closed: bool,
  wrapper: Option<v8::Global<v8::Object>>,
  onmessage: Option<v8::Global<v8::Value>>,
  queue: VecDeque<v8::Global<v8::Value>>,
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
  queue: Vec<yuzuko_core::Parcel>,
}

static PORT_MOVES: LazyLock<Mutex<HashMap<u64, PortMove>>> =
  LazyLock::new(|| Mutex::new(HashMap::new()));

thread_local! {
  static CAPTURE_PORTS: Cell<bool> = const { Cell::new(false) };
  static READ_PORTS: RefCell<Vec<v8::Global<v8::Object>>> =
    const { RefCell::new(Vec::new()) };
  static EARLY_PORTS: RefCell<HashMap<u64, Vec<v8::Global<v8::Value>>>> =
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
  READ_PORTS.with(|slot| slot.borrow_mut().clear());
  CAPTURE_PORTS.with(|cell| cell.set(false));
}

pub(crate) fn message_event_for<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  data: v8::Local<v8::Value>,
) -> v8::Local<'s, v8::Object> {
  let data = v8::Global::new(scope, data);
  message_event(scope, &data)
}

const TAG_PORT: u32 = 12;

thread_local! {
  static MOVED_PORTS: RefCell<HashMap<u64, u64>> = RefCell::new(HashMap::new());
}

pub(crate) fn register_clone() {
  yuzuko_core::add_host_clone(write_port_clone, read_port_clone);
}

fn write_port_clone(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  helper: &dyn v8::ValueSerializerHelper,
) -> yuzuko_core::WriteResult {
  let Some(port) =
    (unsafe { yuzuko_core::op::host_ref::<MessagePort>(scope, object) })
  else {
    return yuzuko_core::WriteResult::Skip;
  };
  let old_id = port.get().id;
  if !yuzuko_core::is_structured_transfer(scope, object) {
    yuzuko_core::reject_clone(scope);
    return yuzuko_core::WriteResult::Failed;
  }
  let new_id = if yuzuko_core::cross_agent() {
    match move_port_cross(scope, old_id) {
      Ok(id) => id,
      Err(()) => {
        yuzuko_core::reject_clone(scope);
        return yuzuko_core::WriteResult::Failed;
      }
    }
  } else {
    match moved_port(scope, old_id) {
      Ok(id) => id,
      Err(()) => {
        yuzuko_core::reject_clone(scope);
        return yuzuko_core::WriteResult::Failed;
      }
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
      .map(|state| state.queue.clone())
  });
  let Some(queued) = queued else {
    return Err(());
  };
  let target = yuzuko_core::cross_target();
  let mut parcels = Vec::new();
  for value in &queued {
    let local = v8::Local::new(scope, value);
    match yuzuko_core::serialize_parcel(scope, local, &[], &[], target) {
      Ok(parcel) => parcels.push(parcel),
      Err(_) => return Err(()),
    }
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
  if let Some(ticket) = PORT_MOVES.lock().expect("port moves").remove(&id) {
    let wrapper = install_moved_port(scope, ticket);
    note_captured(scope, v8::Local::new(scope, &wrapper));
    return yuzuko_core::ReadResult::Done(wrapper);
  }
  let wrapper = PORTS.with(|slot| {
    slot
      .borrow()
      .get(&id)
      .and_then(|state| state.wrapper.clone())
  });
  let Some(wrapper) = wrapper else {
    yuzuko_core::reject_clone(scope);
    return yuzuko_core::ReadResult::Failed;
  };
  note_captured(scope, v8::Local::new(scope, &wrapper));
  yuzuko_core::ReadResult::Done(wrapper)
}

fn install_moved_port(
  scope: &mut v8::PinScope,
  ticket: PortMove,
) -> v8::Global<v8::Object> {
  let mut queue = VecDeque::new();
  for parcel in ticket.queue {
    if let Ok(value) = yuzuko_core::deserialize_parcel(scope, parcel) {
      queue.push_back(v8::Global::new(scope, value));
    }
  }
  let early = EARLY_PORTS
    .with(|slot| slot.borrow_mut().remove(&ticket.id).unwrap_or_default());
  queue.extend(early);
  PORTS.with(|slot| {
    slot.borrow_mut().insert(
      ticket.id,
      PortState {
        peer: ticket.peer,
        started: false,
        closed: false,
        wrapper: None,
        onmessage: None,
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

pub(crate) fn receive_port_message(
  scope: &mut v8::PinScope,
  id: u64,
  value: v8::Local<v8::Value>,
) {
  let started = PORTS.with(|slot| {
    let mut ports = slot.borrow_mut();
    let state = ports.get_mut(&id)?;
    if state.closed {
      return Some(false);
    }
    state.queue.push_back(v8::Global::new(scope, value));
    Some(state.started)
  });
  match started {
    None => {
      EARLY_PORTS.with(|slot| {
        slot
          .borrow_mut()
          .entry(id)
          .or_default()
          .push(v8::Global::new(scope, value));
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
  let (peer, queue) = PORTS.with(|slot| {
    let mut ports = slot.borrow_mut();
    let state = ports.get_mut(&old_id).ok_or(())?;
    if state.closed {
      return Err(());
    }
    let peer = state.peer.take().ok_or(())?;
    state.closed = true;
    let queue = std::mem::take(&mut state.queue);
    Ok((peer, queue))
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
        onmessage: None,
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

#[op(inherit = EventTarget)]
impl MessagePort {
  #[constructor]
  fn new() -> Result<Self, OpError> {
    Err(OpError::type_("Illegal constructor"))
  }

  fn post_message(
    &self,
    scope: &mut v8::PinScope,
    message: Option<v8::Local<v8::Value>>,
  ) -> Result<(), OpError> {
    let message = message.unwrap_or_else(|| v8::undefined(scope).into());
    let peer = PORTS.with(|slot| {
      let ports = slot.borrow();
      let state = ports.get(&self.id);
      match state {
        Some(state) if state.closed => {
          Err(OpError::dom("InvalidStateError", "The port is closed"))
        }
        Some(state) => Ok(state.peer),
        None => Err(OpError::dom("InvalidStateError", "The port is closed")),
      }
    })?;
    let Some(peer) = peer else {
      return Ok(());
    };
    if port_agent(peer) != yuzuko_core::agent_id() {
      return post_port_remote(scope, peer, message);
    }
    let cloned = match yuzuko_core::clone_value(scope, message) {
      Ok(cloned) => cloned,
      Err(Some(thrown)) => return Err(OpError::Thrown(Box::new(thrown))),
      Err(None) => {
        return Err(OpError::dom(
          "DataCloneError",
          "The object could not be cloned.",
        ));
      }
    };
    let started = PORTS.with(|slot| {
      let mut ports = slot.borrow_mut();
      let Some(state) = ports.get_mut(&peer) else {
        return false;
      };
      state.queue.push_back(v8::Global::new(scope, cloned));
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

  #[getter]
  fn onmessage(&self, scope: &mut v8::PinScope) -> Js {
    let value = PORTS.with(|slot| {
      slot
        .borrow()
        .get(&self.id)
        .and_then(|state| state.onmessage.clone())
    });
    match value {
      Some(value) => {
        let local = v8::Local::new(scope, &value);
        Js(v8::Global::new(scope, local))
      }
      None => {
        let value: v8::Local<v8::Value> = v8::null(scope).into();
        Js(v8::Global::new(scope, value))
      }
    }
  }

  #[setter]
  fn onmessage(&self, scope: &mut v8::PinScope, value: v8::Local<v8::Value>) {
    let previous = PORTS.with(|slot| {
      slot
        .borrow()
        .get(&self.id)
        .and_then(|state| state.onmessage.clone())
    });
    if let Some(previous) = previous {
      let previous = v8::Local::new(scope, &previous);
      self.base.remove_callback(scope, "message", previous, false);
    }
    if value.is_function() {
      self
        .base
        .add_callback(scope, "message", value, false, false, false);
      PORTS.with(|slot| {
        if let Some(state) = slot.borrow_mut().get_mut(&self.id) {
          state.onmessage = Some(v8::Global::new(scope, value));
        }
      });
    } else {
      PORTS.with(|slot| {
        if let Some(state) = slot.borrow_mut().get_mut(&self.id) {
          state.onmessage = None;
        }
      });
    }
    start_port(self.id);
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

#[op]
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
  origin: String,
  last_event_id: String,
}

unsafe impl v8::cppgc::GarbageCollected for MessageEvent {
  fn trace(&self, visitor: &mut v8::cppgc::Visitor) {
    v8::cppgc::GarbageCollected::trace(&self.base, visitor);
    if let Ok(data) = self.data.try_borrow() {
      visitor.trace(&*data);
    }
  }

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"MessageEvent"
  }
}

#[op(inherit = Event)]
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
    Ok(Self {
      base,
      data: RefCell::new(Some(traced(scope, data))),
      origin,
      last_event_id,
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
    self.origin.clone()
  }

  #[getter]
  fn last_event_id(&self) -> String {
    self.last_event_id.clone()
  }

  #[getter]
  fn source(&self, scope: &mut v8::PinScope) -> Js {
    let value: v8::Local<v8::Value> = v8::null(scope).into();
    Js(v8::Global::new(scope, value))
  }

  #[getter]
  fn ports(&self, scope: &mut v8::PinScope) -> Js {
    let array = v8::Array::new(scope, 0);
    let value: v8::Local<v8::Value> = array.into();
    Js(v8::Global::new(scope, value))
  }
}

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
    origin,
    last_event_id,
  };
  let object = unsafe { yuzuko_core::op::wrap_instance(scope, event) };
  define_is_trusted(scope, object);
  object
}

fn post_port_remote(
  scope: &mut v8::PinScope,
  peer: u64,
  message: v8::Local<v8::Value>,
) -> Result<(), OpError> {
  let agent = port_agent(peer);
  let parcel =
    match yuzuko_core::serialize_parcel(scope, message, &[], &[], agent) {
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
  yuzuko_core::post_inbound(
    agent,
    yuzuko_core::Inbound::Port { port: peer, parcel },
  );
  Ok(())
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
    !state.queue.is_empty()
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
        onmessage: None,
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
        onmessage: None,
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
  let pending = PORTS.with(|slot| {
    let mut ports = slot.borrow_mut();
    let Some(state) = ports.get_mut(&id) else {
      return Vec::new();
    };
    if !state.started || state.closed {
      return Vec::new();
    }
    state.queue.drain(..).collect()
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
  for data in pending {
    let target = v8::Local::new(scope, &wrapper);
    let event = message_event(scope, &data);
    let _ = target::dispatch(scope, target, event);
  }
}

fn message_event<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  data: &v8::Global<v8::Value>,
) -> v8::Local<'s, v8::Object> {
  let _ = MessageEvent::template(scope);
  let data = v8::Local::new(scope, data);
  let data = yuzuko_core::op::root(scope, data);
  let data = traced(scope, data);
  let event = unsafe {
    yuzuko_core::op::wrap_instance(
      scope,
      MessageEvent {
        base: Event::new("message", false, false, false, true),
        data: RefCell::new(Some(data)),
        origin: String::new(),
        last_event_id: String::new(),
      },
    )
  };
  define_is_trusted(scope, event);
  event
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
  MessageEvent::install(scope);
  MessageChannel::install(scope);
}
