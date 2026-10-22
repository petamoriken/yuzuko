//! Dedicated `Worker`, `WorkerGlobalScope`, and `DedicatedWorkerGlobalScope`.

use std::cell::RefCell;
use std::collections::HashMap;

use yuzuko_core::op::{Js, OpError, OpException};
use yuzuko_core::{
  Inbound, ScriptKind, WorkerScript, agent_alive, agent_id,
  eval_classic_in_worker, is_dedicated_worker, load_worker_source,
  parent_agent, post_inbound, resolve_specifier, rust_string, spawn_worker,
  take_inbox, terminate_worker, worker_is_module, worker_location, worker_name,
};
use yuzuko_macros::op;

use crate::animation;
use crate::event::{ErrorEvent, define_is_trusted};
use crate::message::{
  capture_ports, message_event_for, receive_port_message, take_captured_ports,
};
use crate::report;
use crate::target::{self, EventTarget};

thread_local! {
  static OWNED: RefCell<HashMap<u64, v8::Global<v8::Object>>> =
    RefCell::new(HashMap::new());
}

pub(crate) fn install(scope: &mut v8::PinScope) {
  WorkerGlobalScope::install(scope);
  DedicatedWorkerGlobalScope::install(scope);
  WorkerLocation::install(scope);
  WorkerNavigator::install(scope);
  Worker::install(scope);
  if !is_dedicated_worker() {
    return;
  }
  let global = yuzuko_core::global(scope);
  if let Some(proto) = prototype(scope, "DedicatedWorkerGlobalScope") {
    let _ = global.set_prototype(scope, proto.into());
  }
  if let Some(proto) = prototype(scope, "WorkerGlobalScope") {
    yuzuko_core::set_function(scope, proto, "importScripts", import_scripts);
  }
  if let Some(proto) = prototype(scope, "DedicatedWorkerGlobalScope") {
    yuzuko_core::set_function(scope, proto, "postMessage", global_post_message);
    yuzuko_core::set_function(scope, proto, "close", close_worker);
  }
  install_location(scope, global);
  animation::install(scope);
  if let Some(proto) = prototype(scope, "WorkerNavigator")
    && let Some(navigator) =
      global.get(scope, v8::String::new(scope, "navigator").unwrap().into())
    && let Ok(navigator) = v8::Local::<v8::Object>::try_from(navigator)
  {
    let _ = navigator.set_prototype(scope, proto.into());
  }
  let name = v8::String::new(scope, &worker_name()).unwrap();
  yuzuko_core::set_value(scope, global, "name", name.into());
  for property in [
    "onmessage",
    "onmessageerror",
    "onerror",
    "onoffline",
    "ononline",
  ] {
    yuzuko_core::set_value(scope, global, property, v8::null(scope).into());
  }
}

pub(crate) fn clear() {
  OWNED.with(|slot| slot.borrow_mut().clear());
}

pub(crate) fn drain(scope: &mut v8::PinScope) {
  // Snapshot the inbox. A worker that posts again while these run must not
  // keep this call from returning, or `setTimeout` and the test deadline
  // never get a turn (`terminate.html` flooded the heap that way).
  // Microtasks still run between items so a promise reaction can replace
  // `onmessage` before the next event already in this batch.
  let items = take_inbox(agent_id());
  for item in items {
    match item {
      Inbound::Script { worker, parcel } => {
        deliver_script(scope, worker, parcel);
      }
      Inbound::Port { port, parcel } => deliver_port(scope, port, parcel),
      Inbound::Failed {
        worker,
        message,
        filename,
        line,
        column,
      } => deliver_failure(scope, worker, message, filename, line, column),
    }
    scope.perform_microtask_checkpoint();
  }
}

fn deliver_script(
  scope: &mut v8::PinScope,
  worker: u64,
  parcel: yuzuko_core::Parcel,
) {
  capture_ports(true);
  let value = match yuzuko_core::deserialize_parcel(scope, parcel) {
    Ok(value) => value,
    Err(error) => {
      let _ = take_captured_ports();
      if let Some(error) = error {
        let error = v8::Local::new(scope, error);
        report::report_error(scope, error);
      }
      return;
    }
  };
  let _ports = take_captured_ports();
  let event = message_event_for(scope, value);
  if worker == agent_id() {
    let global = yuzuko_core::global(scope);
    let _ = target::dispatch(scope, global, event);
    call_property(scope, global, "onmessage", event);
    return;
  }
  let wrapper = OWNED.with(|slot| slot.borrow().get(&worker).cloned());
  let Some(wrapper) = wrapper else {
    return;
  };
  let target = v8::Local::new(scope, wrapper);
  let _ = target::dispatch(scope, target, event);
}

fn deliver_port(
  scope: &mut v8::PinScope,
  port: u64,
  parcel: yuzuko_core::Parcel,
) {
  capture_ports(true);
  let value = match yuzuko_core::deserialize_parcel(scope, parcel) {
    Ok(value) => value,
    Err(_) => {
      let _ = take_captured_ports();
      return;
    }
  };
  let _ = take_captured_ports();
  receive_port_message(scope, port, value);
}

fn deliver_failure(
  scope: &mut v8::PinScope,
  worker: u64,
  message: String,
  filename: String,
  line: u32,
  column: u32,
) {
  let event = error_event(scope, message, filename, line, column);
  if worker == agent_id() {
    let global = yuzuko_core::global(scope);
    let _ = target::dispatch(scope, global, event);
    call_property(scope, global, "onerror", event);
    return;
  }
  let wrapper = OWNED.with(|slot| slot.borrow().get(&worker).cloned());
  let Some(wrapper) = wrapper else {
    return;
  };
  let target = v8::Local::new(scope, wrapper);
  let _ = target::dispatch(scope, target, event);
}

fn error_event<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  message: String,
  filename: String,
  line: u32,
  column: u32,
) -> v8::Local<'s, v8::Object> {
  let _ = ErrorEvent::template(scope);
  let built = ErrorEvent::worker(
    scope,
    message,
    filename,
    f64::from(line),
    f64::from(column),
  );
  let event = unsafe { yuzuko_core::op::wrap_instance(scope, built) };
  define_is_trusted(scope, event);
  event
}

fn call_property(
  scope: &mut v8::PinScope,
  target: v8::Local<v8::Object>,
  name: &str,
  event: v8::Local<v8::Object>,
) {
  let Some(key) = v8::String::new(scope, name) else {
    return;
  };
  let Some(value) = target.get(scope, key.into()) else {
    return;
  };
  let Ok(function) = v8::Local::<v8::Function>::try_from(value) else {
    return;
  };
  let thrown = {
    v8::tc_scope!(let tc, scope);
    let called = function.call(tc, target.into(), &[event.into()]).is_some();
    if called && !tc.has_caught() {
      None
    } else {
      let error = tc.exception().map(|value| v8::Global::new(tc, value));
      tc.reset();
      error
    }
  };
  if let Some(error) = thrown {
    let error = v8::Local::new(scope, error);
    report::report_error(scope, error);
  }
}

fn prototype<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  name: &str,
) -> Option<v8::Local<'s, v8::Object>> {
  let global = yuzuko_core::global(scope);
  let key = v8::String::new(scope, name)?;
  let ctor = global.get(scope, key.into())?;
  let ctor = v8::Local::<v8::Object>::try_from(ctor).ok()?;
  let proto_key = v8::String::new(scope, "prototype")?;
  let proto = ctor.get(scope, proto_key.into())?;
  v8::Local::<v8::Object>::try_from(proto).ok()
}

fn install_location(scope: &mut v8::PinScope, global: v8::Local<v8::Object>) {
  let parts = worker_location();
  let location = v8::Object::new(scope);
  set_string(scope, location, "href", &parts.href);
  set_string(scope, location, "origin", &parts.origin);
  set_string(scope, location, "protocol", &parts.protocol);
  set_string(scope, location, "host", &parts.host);
  set_string(scope, location, "hostname", &parts.hostname);
  set_string(scope, location, "port", &parts.port);
  set_string(scope, location, "pathname", &parts.pathname);
  set_string(scope, location, "search", &parts.search);
  set_string(scope, location, "hash", &parts.hash);
  if let Some(proto) = prototype(scope, "WorkerLocation") {
    let _ = location.set_prototype(scope, proto.into());
  }
  yuzuko_core::set_value(scope, global, "location", location.into());
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
  yuzuko_core::set_value(scope, object, name, text.into());
}

fn import_scripts(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut _ret: v8::ReturnValue,
) {
  if worker_is_module() {
    yuzuko_core::throw_type(
      scope,
      "importScripts is not available in a module worker",
    );
    return;
  }
  if args.length() == 0 {
    return;
  }
  for index in 0..args.length() {
    let spec = rust_string(scope, args.get(index));
    let url = match resolve_specifier(&spec) {
      Ok(url) => url,
      Err(message) => {
        yuzuko_core::throw_type(scope, &message);
        return;
      }
    };
    let sandbox = yuzuko_core::entry_dir();
    let loaded = match load_worker_source(&url, sandbox.as_deref()) {
      Ok(loaded) => loaded,
      Err(message) => {
        OpError::dom("NetworkError", message).throw(scope);
        return;
      }
    };
    if let Err(message) = eval_classic_in_worker(scope, &url, &loaded.text) {
      yuzuko_core::throw_type(scope, &message);
      return;
    }
  }
}

fn global_post_message(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut _ret: v8::ReturnValue,
) {
  let message = if args.length() == 0 {
    v8::undefined(scope).into()
  } else {
    args.get(0)
  };
  let transfer = if args.length() > 1 {
    args.get(1)
  } else {
    v8::undefined(scope).into()
  };
  let id = agent_id();
  let Some(parent) = parent_agent(id) else {
    return;
  };
  let message = rooted(scope, message);
  let transfer = rooted(scope, transfer);
  let parcel = match prepare_message(scope, message, transfer, parent) {
    Ok(parcel) => parcel,
    Err(error) => {
      error.throw(scope);
      return;
    }
  };
  post_inbound(parent, Inbound::Script { worker: id, parcel });
}

fn close_worker(
  _scope: &mut v8::PinScope,
  _args: v8::FunctionCallbackArguments,
  mut _ret: v8::ReturnValue,
) {
  terminate_worker(agent_id());
}

fn rooted<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  value: v8::Local<v8::Value>,
) -> v8::Local<'s, v8::Value> {
  let global = v8::Global::new(scope, value);
  v8::Local::new(scope, global)
}

fn prepare_message<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  message: v8::Local<'s, v8::Value>,
  transfer: v8::Local<'s, v8::Value>,
  target: u64,
) -> Result<yuzuko_core::Parcel, OpError> {
  let (objects, buffers) = crate::message::transfer_parts(scope, transfer)?;
  match yuzuko_core::serialize_parcel(
    scope, message, &objects, &buffers, target,
  ) {
    Ok(parcel) => Ok(parcel),
    Err(Some(thrown)) => Err(OpError::Thrown(Box::new(thrown))),
    Err(None) => Err(OpError::dom(
      "DataCloneError",
      "The object could not be cloned.",
    )),
  }
}

#[repr(C)]
struct WorkerGlobalScope {
  base: EventTarget,
}

unsafe impl v8::cppgc::GarbageCollected for WorkerGlobalScope {
  fn trace(&self, visitor: &mut v8::cppgc::Visitor) {
    v8::cppgc::GarbageCollected::trace(&self.base, visitor);
  }

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"WorkerGlobalScope"
  }
}

#[op(name = "WorkerGlobalScope", inherit = EventTarget)]
impl WorkerGlobalScope {
  #[constructor]
  fn new() -> Result<Self, OpError> {
    Err(OpError::type_("Illegal constructor"))
  }
}

#[repr(C)]
struct DedicatedWorkerGlobalScope {
  base: WorkerGlobalScope,
}

unsafe impl v8::cppgc::GarbageCollected for DedicatedWorkerGlobalScope {
  fn trace(&self, visitor: &mut v8::cppgc::Visitor) {
    v8::cppgc::GarbageCollected::trace(&self.base, visitor);
  }

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"DedicatedWorkerGlobalScope"
  }
}

#[op(name = "DedicatedWorkerGlobalScope", inherit = WorkerGlobalScope)]
impl DedicatedWorkerGlobalScope {
  #[constructor]
  fn new() -> Result<Self, OpError> {
    Err(OpError::type_("Illegal constructor"))
  }
}

struct WorkerLocation;

unsafe impl v8::cppgc::GarbageCollected for WorkerLocation {
  fn trace(&self, _visitor: &mut v8::cppgc::Visitor) {}

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"WorkerLocation"
  }
}

#[op(name = "WorkerLocation")]
impl WorkerLocation {
  #[constructor]
  fn new() -> Result<Self, OpError> {
    Err(OpError::type_("Illegal constructor"))
  }
}

struct WorkerNavigator;

unsafe impl v8::cppgc::GarbageCollected for WorkerNavigator {
  fn trace(&self, _visitor: &mut v8::cppgc::Visitor) {}

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"WorkerNavigator"
  }
}

#[op(name = "WorkerNavigator")]
impl WorkerNavigator {
  #[constructor]
  fn new() -> Result<Self, OpError> {
    Err(OpError::type_("Illegal constructor"))
  }
}

#[repr(C)]
struct Worker {
  base: EventTarget,
  id: u64,
  onmessage: RefCell<Option<v8::TracedReference<v8::Value>>>,
  onerror: RefCell<Option<v8::TracedReference<v8::Value>>>,
}

unsafe impl v8::cppgc::GarbageCollected for Worker {
  fn trace(&self, visitor: &mut v8::cppgc::Visitor) {
    v8::cppgc::GarbageCollected::trace(&self.base, visitor);
    if let Ok(handler) = self.onmessage.try_borrow()
      && let Some(handler) = handler.as_ref()
    {
      visitor.trace(handler);
    }
    if let Ok(handler) = self.onerror.try_borrow()
      && let Some(handler) = handler.as_ref()
    {
      visitor.trace(handler);
    }
  }

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"Worker"
  }
}

#[op(inherit = EventTarget)]
impl Worker {
  #[constructor]
  fn new(
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<Worker, OpError> {
    if args.length() < 1 || args.get(0).is_undefined() {
      return Err(OpError::type_("Worker requires a URL"));
    }
    let spec = rust_string(scope, args.get(0));
    let options = if args.length() > 1 {
      args.get(1)
    } else {
      v8::undefined(scope).into()
    };
    let (module, name) = read_options(scope, options)?;
    let url = resolve_specifier(&spec).map_err(OpError::type_)?;
    let id = spawn_worker(
      agent_id(),
      WorkerScript {
        url,
        kind: if module {
          ScriptKind::Module
        } else {
          ScriptKind::Classic
        },
        name,
        sandbox: yuzuko_core::entry_dir(),
      },
    )
    .map_err(OpError::type_)?;
    Ok(Worker {
      base: EventTarget::new(),
      id,
      onmessage: RefCell::new(None),
      onerror: RefCell::new(None),
    })
  }

  #[construct_hook]
  fn finish(&self, scope: &mut v8::PinScope, object: v8::Local<v8::Object>) {
    OWNED.with(|slot| {
      slot
        .borrow_mut()
        .insert(self.id, v8::Global::new(scope, object));
    });
  }

  fn post_message(
    &self,
    scope: &mut v8::PinScope,
    message: Option<v8::Local<v8::Value>>,
    transfer: Option<v8::Local<v8::Value>>,
  ) -> Result<(), OpError> {
    if !agent_alive(self.id) {
      return Ok(());
    }
    let message = message.unwrap_or_else(|| v8::undefined(scope).into());
    let transfer = transfer.unwrap_or_else(|| v8::undefined(scope).into());
    let message = rooted(scope, message);
    let transfer = rooted(scope, transfer);
    let parcel = prepare_message(scope, message, transfer, self.id)?;
    post_inbound(
      self.id,
      Inbound::Script {
        worker: self.id,
        parcel,
      },
    );
    Ok(())
  }

  fn terminate(&self) {
    terminate_worker(self.id);
  }

  #[getter]
  fn onmessage(&self, scope: &mut v8::PinScope) -> Js {
    handler_value(scope, &self.onmessage)
  }

  #[setter]
  fn onmessage(&self, scope: &mut v8::PinScope, value: v8::Local<v8::Value>) {
    set_handler(scope, &self.base, &self.onmessage, "message", value);
  }

  #[getter]
  fn onerror(&self, scope: &mut v8::PinScope) -> Js {
    handler_value(scope, &self.onerror)
  }

  #[setter]
  fn onerror(&self, scope: &mut v8::PinScope, value: v8::Local<v8::Value>) {
    set_handler(scope, &self.base, &self.onerror, "error", value);
  }
}

fn read_options(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<(bool, String), OpError> {
  if value.is_null_or_undefined() {
    return Ok((false, String::new()));
  }
  let object = v8::Local::<v8::Object>::try_from(value)
    .map_err(|_| OpError::type_("Worker options are not an object"))?;
  let mut module = false;
  if let Some(kind) = object_get(scope, object, "type")
    && !kind.is_undefined()
  {
    match rust_string(scope, kind).as_str() {
      "classic" => module = false,
      "module" => module = true,
      _ => {
        return Err(OpError::type_("Worker type must be classic or module"));
      }
    }
  }
  let mut name = String::new();
  if let Some(value) = object_get(scope, object, "name")
    && !value.is_undefined()
  {
    name = rust_string(scope, value);
  }
  Ok((module, name))
}

fn object_get<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  object: v8::Local<v8::Object>,
  name: &str,
) -> Option<v8::Local<'s, v8::Value>> {
  let key = v8::String::new(scope, name)?;
  object.get(scope, key.into())
}

fn handler_value(
  scope: &mut v8::PinScope,
  slot: &RefCell<Option<v8::TracedReference<v8::Value>>>,
) -> Js {
  let value = slot
    .borrow()
    .as_ref()
    .and_then(|reference| yuzuko_core::op::traced_local(scope, reference))
    .unwrap_or_else(|| v8::null(scope).into());
  Js(v8::Global::new(scope, value))
}

fn set_handler(
  scope: &mut v8::PinScope,
  target: &EventTarget,
  slot: &RefCell<Option<v8::TracedReference<v8::Value>>>,
  type_name: &str,
  value: v8::Local<v8::Value>,
) {
  if let Some(previous) = slot.borrow().as_ref()
    && let Some(previous) = yuzuko_core::op::traced_local(scope, previous)
  {
    target.remove_callback(scope, type_name, previous, false);
  }
  if value.is_function() {
    target.add_callback(scope, type_name, value, false, false, false);
    *slot.borrow_mut() = Some(yuzuko_core::op::traced(scope, value));
  } else {
    *slot.borrow_mut() = None;
  }
}
