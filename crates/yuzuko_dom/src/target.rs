//! `EventTarget`. `globalThis` is not a cppgc wrapper, so its listeners live
//! beside the realm and are dropped while the isolate is still alive.

use std::cell::{Cell, RefCell};

use yuzuko_core::op::{OpError, host_ref, traced};
use yuzuko_macros::op;

use crate::abort;
use crate::event::{self, Event};
use crate::report;
use crate::util::{self, key};

struct Listener {
  id: u64,
  type_name: String,
  callback: v8::TracedReference<v8::Value>,
  capture: bool,
  once: bool,
  passive: bool,
}

pub struct EventTarget {
  listeners: RefCell<Vec<Listener>>,
  next_id: Cell<u64>,
}

unsafe impl v8::cppgc::GarbageCollected for EventTarget {
  fn trace(&self, visitor: &mut v8::cppgc::Visitor) {
    if let Ok(listeners) = self.listeners.try_borrow() {
      for listener in listeners.iter() {
        visitor.trace(&listener.callback);
      }
    }
  }

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"EventTarget"
  }
}

impl Default for EventTarget {
  fn default() -> Self {
    Self::new()
  }
}

impl EventTarget {
  pub fn new() -> Self {
    Self {
      listeners: RefCell::new(Vec::new()),
      next_id: Cell::new(1),
    }
  }

  pub fn remove_callback(
    &self,
    scope: &mut v8::PinScope,
    type_name: &str,
    callback: v8::Local<v8::Value>,
    capture: bool,
  ) {
    let index = self.listener_index(scope, type_name, callback, capture);
    if let Some(index) = index {
      self.listeners.borrow_mut().remove(index);
    }
  }

  pub fn add_callback(
    &self,
    scope: &mut v8::PinScope,
    type_name: &str,
    callback: v8::Local<v8::Value>,
    capture: bool,
    once: bool,
    passive: bool,
  ) {
    if self
      .listener_index(scope, type_name, callback, capture)
      .is_some()
    {
      return;
    }
    let callback = traced(scope, callback);
    let id = self.next_id.get();
    self.next_id.set(id + 1);
    self.listeners.borrow_mut().push(Listener {
      id,
      type_name: type_name.to_string(),
      callback,
      capture,
      once,
      passive,
    });
  }

  fn listener_index(
    &self,
    scope: &mut v8::PinScope,
    type_name: &str,
    callback: v8::Local<v8::Value>,
    capture: bool,
  ) -> Option<usize> {
    let len = self.listeners.borrow().len();
    for index in 0..len {
      let matched = {
        let listeners = self.listeners.borrow();
        let listener = &listeners[index];
        listener.type_name == type_name
          && listener.capture == capture
          && yuzuko_core::op::traced_local(scope, &listener.callback)
            .is_some_and(|existing| existing.strict_equals(callback))
      };
      if matched {
        return Some(index);
      }
    }
    None
  }

  fn contains(&self, id: u64) -> bool {
    self
      .listeners
      .borrow()
      .iter()
      .any(|listener| listener.id == id)
  }

  pub(crate) fn listens_for(&self, type_name: &str) -> bool {
    self
      .listeners
      .borrow()
      .iter()
      .any(|listener| listener.type_name == type_name)
  }
}

#[op]
impl EventTarget {
  #[constructor]
  fn new() -> EventTarget {
    EventTarget::new()
  }

  fn add_event_listener(
    #[this] target: v8::Local<v8::Object>,
    scope: &mut v8::PinScope,
    type_name: String,
    callback: v8::Local<v8::Value>,
    options: Option<v8::Local<v8::Value>>,
  ) -> Result<(), OpError> {
    add_listener(scope, target, &type_name, callback, options)
  }

  fn remove_event_listener(
    #[this] target: v8::Local<v8::Object>,
    scope: &mut v8::PinScope,
    type_name: String,
    callback: v8::Local<v8::Value>,
    options: Option<v8::Local<v8::Value>>,
  ) -> Result<(), OpError> {
    let capture = capture_flag(scope, options);
    remove_listener(scope, target, &type_name, callback, capture)
  }

  fn dispatch_event(
    #[this] target: v8::Local<v8::Object>,
    scope: &mut v8::PinScope,
    event: v8::Local<v8::Value>,
  ) -> Result<bool, OpError> {
    let Ok(event) = v8::Local::<v8::Object>::try_from(event) else {
      return Err(OpError::type_("dispatchEvent requires an Event"));
    };
    if unsafe { host_ref::<Event>(scope, event) }.is_none() {
      return Err(OpError::type_("dispatchEvent requires an Event"));
    }
    dispatch(scope, target, event)
  }
}

pub(crate) fn install(scope: &mut v8::PinScope) {
  EventTarget::install(scope);
  let constructor = EventTarget::template(scope)
    .get_function(scope)
    .expect("EventTarget");
  let Some(prototype) = constructor
    .get(scope, key(scope, "prototype").into())
    .filter(|value| !value.is_null_or_undefined())
  else {
    return;
  };
  let global = scope.get_current_context().global(scope);
  let _ = global.set_prototype(scope, prototype);
  if !yuzuko_core::is_dedicated_worker() {
    install_window_event(scope, global);
  }
}

struct Snap {
  id: u64,
  type_name: String,
  callback: v8::Global<v8::Value>,
  once: bool,
  passive: bool,
  capture: bool,
}

struct Options {
  capture: bool,
  once: bool,
  passive: bool,
  signal: Option<v8::Global<v8::Object>>,
}

pub(crate) fn add_listener(
  scope: &mut v8::PinScope,
  target: v8::Local<v8::Object>,
  type_name: &str,
  callback: v8::Local<v8::Value>,
  options: Option<v8::Local<v8::Value>>,
) -> Result<(), OpError> {
  // Dictionary conversion runs even when the listener is null, so a
  // `passive` getter is observed and a null `signal` throws.
  let options = listener_options(scope, options)?;
  if callback.is_null() || callback.is_undefined() {
    return Ok(());
  }
  if !callback.is_function() && !callback.is_object() {
    return Ok(());
  }
  if let Some(signal) = options.signal.as_ref() {
    let signal = v8::Local::new(scope, signal);
    if abort::is_aborted(scope, signal)? {
      return Ok(());
    }
  }
  attach(
    scope,
    target,
    type_name,
    callback,
    options.capture,
    options.once,
    options.passive,
  )?;
  if let Some(signal) = options.signal {
    let signal = v8::Local::new(scope, signal);
    let remover = remover(scope, target, type_name, callback, options.capture);
    if let Some(remover) = remover {
      attach(scope, signal, "abort", remover.into(), false, true, false)?;
    }
  }
  Ok(())
}

pub(crate) fn remove_listener(
  scope: &mut v8::PinScope,
  target: v8::Local<v8::Object>,
  type_name: &str,
  callback: v8::Local<v8::Value>,
  capture: bool,
) -> Result<(), OpError> {
  if let Some(host) = unsafe { host_ref::<EventTarget>(scope, target) } {
    host
      .get()
      .remove_callback(scope, type_name, callback, capture);
    if type_name == "abort" {
      abort::sync_observers(scope, target);
    }
    return Ok(());
  }
  if util::is_global(scope, target) {
    realm_remove(scope, type_name, callback, capture);
    return Ok(());
  }
  Err(OpError::type_("illegal invocation"))
}

pub fn dispatch(
  scope: &mut v8::PinScope,
  target: v8::Local<v8::Object>,
  event: v8::Local<v8::Object>,
) -> Result<bool, OpError> {
  dispatch_path(scope, &[target], event)
}

/// `path` starts at the event target and continues through ancestors.
///
/// Capture runs from the root down to the target. The target then runs every
/// listener. Bubble runs back out when the event bubbles. `stopPropagation`
/// skips later objects. `stopImmediatePropagation` also skips the remaining
/// listeners on the current object.
pub fn dispatch_path(
  scope: &mut v8::PinScope,
  path: &[v8::Local<v8::Object>],
  event: v8::Local<v8::Object>,
) -> Result<bool, OpError> {
  let Some(target) = path.first().copied() else {
    return Ok(true);
  };
  if event::is_dispatching(scope, event) {
    return Err(OpError::dom(
      "InvalidStateError",
      "The event is already being dispatched.",
    ));
  }
  event::begin_dispatch(scope, event, target);
  event::set_path(scope, event, path);
  for ancestor in path.iter().skip(1).rev() {
    if event::is_stopped(scope, event) {
      break;
    }
    invoke(scope, *ancestor, event, Phase::Capture)?;
  }
  if !event::is_stopped(scope, event) {
    invoke(scope, target, event, Phase::Target)?;
  }
  if event::bubbles(scope, event) && !event::is_stopped(scope, event) {
    for ancestor in path.iter().skip(1) {
      if event::is_stopped(scope, event) {
        break;
      }
      invoke(scope, *ancestor, event, Phase::Bubble)?;
    }
  }
  event::finish_dispatch(scope, event);
  Ok(!event::was_prevented(scope, event))
}

enum Phase {
  Capture,
  Target,
  Bubble,
}

fn invoke(
  scope: &mut v8::PinScope,
  current: v8::Local<v8::Object>,
  event: v8::Local<v8::Object>,
  phase: Phase,
) -> Result<(), OpError> {
  let phase_number = match phase {
    Phase::Capture => 1,
    Phase::Target => 2,
    Phase::Bubble => 3,
  };
  event::set_current(scope, event, current, phase_number);
  let snapshot = snapshot(scope, current)?;
  let event_type = event::event_type(scope, event).unwrap_or_default();
  for item in snapshot {
    if event::is_immediate(scope, event) {
      break;
    }
    if item.type_name != event_type {
      continue;
    }
    if matches!(phase, Phase::Capture) && !item.capture {
      continue;
    }
    if matches!(phase, Phase::Bubble) && item.capture {
      continue;
    }
    if !still_present(scope, current, item.id) {
      continue;
    }
    if item.once {
      let callback = v8::Local::new(scope, &item.callback);
      let _ = remove_listener(
        scope,
        current,
        &item.type_name,
        callback,
        item.capture,
      );
    }
    event::set_passive(scope, event, item.passive);
    // `window.event` is set before `handleEvent` is looked up, and restored
    // after a thrown listener is reported.
    let restore_event = push_current_event(scope, event);
    call_listener(scope, current, event, &item.callback);
    event::set_passive(scope, event, false);
    drop(restore_event);
  }
  Ok(())
}

fn attach(
  scope: &mut v8::PinScope,
  target: v8::Local<v8::Object>,
  type_name: &str,
  callback: v8::Local<v8::Value>,
  capture: bool,
  once: bool,
  passive: bool,
) -> Result<(), OpError> {
  if let Some(host) = unsafe { host_ref::<EventTarget>(scope, target) } {
    let duplicate = host
      .get()
      .listener_index(scope, type_name, callback, capture)
      .is_some();
    if !duplicate {
      let callback = traced(scope, callback);
      let id = host.get().next_id.get();
      host.get().next_id.set(id + 1);
      host.get().listeners.borrow_mut().push(Listener {
        id,
        type_name: type_name.to_string(),
        callback,
        capture,
        once,
        passive,
      });
    }
    if type_name == "abort" {
      abort::sync_observers(scope, target);
    }
    return Ok(());
  }
  if util::is_global(scope, target) {
    if !realm_contains(scope, type_name, callback, capture) {
      realm_push(scope, type_name, callback, capture, once, passive);
    }
    return Ok(());
  }
  Err(OpError::type_("illegal invocation"))
}

fn snapshot(
  scope: &mut v8::PinScope,
  target: v8::Local<v8::Object>,
) -> Result<Vec<Snap>, OpError> {
  if let Some(host) = unsafe { host_ref::<EventTarget>(scope, target) } {
    let len = host.get().listeners.borrow().len();
    let mut snaps = Vec::with_capacity(len);
    for index in 0..len {
      let snap = {
        let listeners = host.get().listeners.borrow();
        let listener = &listeners[index];
        let Some(callback) =
          yuzuko_core::op::traced_local(scope, &listener.callback)
        else {
          continue;
        };
        Snap {
          id: listener.id,
          type_name: listener.type_name.clone(),
          callback: v8::Global::new(scope, callback),
          once: listener.once,
          passive: listener.passive,
          capture: listener.capture,
        }
      };
      snaps.push(snap);
    }
    return Ok(snaps);
  }
  if util::is_global(scope, target) {
    return Ok(realm_snapshot(scope));
  }
  Err(OpError::type_("illegal invocation"))
}

fn still_present(
  scope: &mut v8::PinScope,
  target: v8::Local<v8::Object>,
  id: u64,
) -> bool {
  if let Some(host) = unsafe { host_ref::<EventTarget>(scope, target) } {
    return host.get().contains(id);
  }
  realm_contains_id(id)
}

fn call_listener(
  scope: &mut v8::PinScope,
  target: v8::Local<v8::Object>,
  event: v8::Local<v8::Object>,
  callback: &v8::Global<v8::Value>,
) {
  let thrown = {
    v8::tc_scope!(let tc, scope);
    let callback = v8::Local::new(tc, callback);
    let called =
      if let Ok(function) = v8::Local::<v8::Function>::try_from(callback) {
        function.call(tc, target.into(), &[event.into()]).is_some()
      } else if let Ok(object) = v8::Local::<v8::Object>::try_from(callback) {
        object
          .get(tc, key(tc, "handleEvent").into())
          .and_then(|method| v8::Local::<v8::Function>::try_from(method).ok())
          .and_then(|method| method.call(tc, object.into(), &[event.into()]))
          .is_some()
      } else {
        true
      };
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

fn listener_options(
  scope: &mut v8::PinScope,
  options: Option<v8::Local<v8::Value>>,
) -> Result<Options, OpError> {
  let empty = Options {
    capture: false,
    once: false,
    passive: false,
    signal: None,
  };
  let Some(options) = options else {
    return Ok(empty);
  };
  if options.is_boolean() {
    return Ok(Options {
      capture: options.is_true(),
      ..empty
    });
  }
  if !options.is_object() {
    return Ok(empty);
  }
  let Ok(object) = v8::Local::<v8::Object>::try_from(options) else {
    return Ok(empty);
  };
  let capture = dict_flag(scope, object, "capture")?;
  let once = dict_flag(scope, object, "once")?;
  let passive = dict_flag(scope, object, "passive")?;
  let signal = dict_signal(scope, object)?;
  Ok(Options {
    capture,
    once,
    passive,
    signal,
  })
}

fn dict_flag(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
) -> Result<bool, OpError> {
  let captured = {
    v8::tc_scope!(let tc, scope);
    match object.get(tc, key(tc, name).into()) {
      Some(value) => Ok(value.boolean_value(tc)),
      None => Err(caught(tc)),
    }
  };
  match captured {
    Ok(flag) => Ok(flag),
    Err(exception) => Err(rethrow(scope, exception)),
  }
}

fn dict_signal(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
) -> Result<Option<v8::Global<v8::Object>>, OpError> {
  let captured = {
    v8::tc_scope!(let tc, scope);
    match object.get(tc, key(tc, "signal").into()) {
      Some(value) => Ok(v8::Global::new(tc, value)),
      None => Err(caught(tc)),
    }
  };
  let value = match captured {
    Ok(value) => v8::Local::new(scope, value),
    Err(exception) => return Err(rethrow(scope, exception)),
  };
  if value.is_undefined() {
    return Ok(None);
  }
  let Ok(signal) = v8::Local::<v8::Object>::try_from(value) else {
    return Err(OpError::type_(
      "addEventListener signal is not an AbortSignal",
    ));
  };
  if unsafe { host_ref::<crate::abort::AbortSignal>(scope, signal) }.is_none() {
    return Err(OpError::type_(
      "addEventListener signal is not an AbortSignal",
    ));
  }
  Ok(Some(v8::Global::new(scope, signal)))
}

fn caught(
  scope: &mut v8::PinnedRef<v8::TryCatch<v8::HandleScope>>,
) -> v8::Global<v8::Value> {
  let exception = scope
    .exception()
    .unwrap_or_else(|| v8::undefined(scope).into());
  v8::Global::new(scope, exception)
}

fn rethrow(
  scope: &mut v8::PinScope,
  exception: v8::Global<v8::Value>,
) -> OpError {
  let exception = v8::Local::new(scope, exception);
  OpError::thrown(scope, exception)
}

fn capture_flag(
  scope: &mut v8::PinScope,
  options: Option<v8::Local<v8::Value>>,
) -> bool {
  let Some(options) = options else {
    return false;
  };
  if options.is_boolean() {
    return options.is_true();
  }
  if let Ok(object) = v8::Local::<v8::Object>::try_from(options) {
    return object
      .get(scope, key(scope, "capture").into())
      .is_some_and(|value| value.boolean_value(scope));
  }
  false
}

fn remover<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  target: v8::Local<'_, v8::Object>,
  type_name: &str,
  callback: v8::Local<'_, v8::Value>,
  capture: bool,
) -> Option<v8::Local<'s, v8::Function>> {
  let data = v8::Object::new(scope);
  data.set(scope, key(scope, "target").into(), target.into());
  data.set(
    scope,
    key(scope, "type").into(),
    key(scope, type_name).into(),
  );
  data.set(scope, key(scope, "callback").into(), callback);
  data.set(
    scope,
    key(scope, "capture").into(),
    v8::Boolean::new(scope, capture).into(),
  );
  v8::Function::builder(signal_abort_remove)
    .data(data.into())
    .build(scope)
}

fn signal_abort_remove(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut _ret: v8::ReturnValue,
) {
  let Ok(data) = v8::Local::<v8::Object>::try_from(args.data()) else {
    return;
  };
  let Some(target) = data
    .get(scope, key(scope, "target").into())
    .and_then(|value| v8::Local::<v8::Object>::try_from(value).ok())
  else {
    return;
  };
  let type_name = data
    .get(scope, key(scope, "type").into())
    .map(|value| util::string_value(scope, value))
    .unwrap_or_default();
  let Some(callback) = data.get(scope, key(scope, "callback").into()) else {
    return;
  };
  let capture = data
    .get(scope, key(scope, "capture").into())
    .is_some_and(|value| value.is_true());
  let _ = remove_listener(scope, target, &type_name, callback, capture);
}

struct RealmListener {
  id: u64,
  type_name: String,
  callback: v8::Global<v8::Value>,
  capture: bool,
  once: bool,
  passive: bool,
}

thread_local! {
  static REALM: RefCell<Vec<RealmListener>> = const { RefCell::new(Vec::new()) };
  static NEXT_REALM_ID: Cell<u64> = const { Cell::new(1) };
  static CURRENT_EVENT: RefCell<Option<v8::Global<v8::Object>>> =
    const { RefCell::new(None) };
}

struct RestoreEvent {
  previous: Option<v8::Global<v8::Object>>,
}

impl Drop for RestoreEvent {
  fn drop(&mut self) {
    let previous = self.previous.take();
    CURRENT_EVENT.with(|slot| *slot.borrow_mut() = previous);
  }
}

fn push_current_event(
  scope: &mut v8::PinScope,
  event: v8::Local<v8::Object>,
) -> RestoreEvent {
  let previous = CURRENT_EVENT
    .with(|slot| slot.replace(Some(v8::Global::new(scope, event))));
  RestoreEvent { previous }
}

fn install_window_event(
  scope: &mut v8::PinScope,
  global: v8::Local<v8::Object>,
) {
  // A JavaScript accessor, not a V8 native one: assigning to a native
  // accessor reenters its setter. `[Replaceable]` turns that assignment
  // into an own data property.
  let getter = v8::Function::new(scope, read_window_event).expect("event");
  let source = v8::String::new(
    scope,
    r#"(getter) => {
      const reflectApply = Reflect.apply;
      const defineProperty = Object.defineProperty;
      const ObjectCtor = Object;
      function define(target, key, descriptor) {
        return reflectApply(defineProperty, ObjectCtor, [target, key, descriptor]);
      }
      define(globalThis, "event", {
        configurable: true,
        enumerable: true,
        get() {
          return getter();
        },
        set(value) {
          define(this, "event", {
            value,
            writable: true,
            enumerable: true,
            configurable: true
          });
        }
      });
    }"#,
  )
  .expect("event");
  let Some(script) = v8::Script::compile(scope, source, None) else {
    return;
  };
  let Some(installer) = script.run(scope) else {
    return;
  };
  let Ok(installer) = v8::Local::<v8::Function>::try_from(installer) else {
    return;
  };
  let _ = installer.call(scope, global.into(), &[getter.into()]);
}

fn read_window_event(
  scope: &mut v8::PinScope,
  _args: v8::FunctionCallbackArguments,
  mut ret: v8::ReturnValue,
) {
  let event = CURRENT_EVENT.with(|slot| {
    slot
      .borrow()
      .as_ref()
      .map(|event| v8::Local::new(scope, event))
  });
  if let Some(event) = event {
    ret.set(event.into());
  } else {
    ret.set(v8::undefined(scope).into());
  }
}

pub(crate) fn clear() {
  REALM.with(|listeners| listeners.borrow_mut().clear());
  NEXT_REALM_ID.with(|id| id.set(1));
  CURRENT_EVENT.with(|slot| *slot.borrow_mut() = None);
}

fn realm_push(
  scope: &mut v8::PinScope,
  type_name: &str,
  callback: v8::Local<v8::Value>,
  capture: bool,
  once: bool,
  passive: bool,
) {
  let id = NEXT_REALM_ID.with(|next| {
    let id = next.get();
    next.set(id + 1);
    id
  });
  REALM.with(|listeners| {
    listeners.borrow_mut().push(RealmListener {
      id,
      type_name: type_name.to_string(),
      callback: v8::Global::new(scope, callback),
      capture,
      once,
      passive,
    });
  });
}

fn realm_contains(
  scope: &mut v8::PinScope,
  type_name: &str,
  callback: v8::Local<v8::Value>,
  capture: bool,
) -> bool {
  REALM.with(|listeners| {
    listeners.borrow().iter().any(|listener| {
      listener.type_name == type_name
        && listener.capture == capture
        && v8::Local::new(scope, &listener.callback).strict_equals(callback)
    })
  })
}

fn realm_contains_id(id: u64) -> bool {
  REALM.with(|listeners| {
    listeners.borrow().iter().any(|listener| listener.id == id)
  })
}

fn realm_remove(
  scope: &mut v8::PinScope,
  type_name: &str,
  callback: v8::Local<v8::Value>,
  capture: bool,
) {
  REALM.with(|listeners| {
    let mut listeners = listeners.borrow_mut();
    if let Some(index) = listeners.iter().position(|listener| {
      listener.type_name == type_name
        && listener.capture == capture
        && v8::Local::new(scope, &listener.callback).strict_equals(callback)
    }) {
      listeners.remove(index);
    }
  });
}

fn realm_snapshot(scope: &mut v8::PinScope) -> Vec<Snap> {
  REALM.with(|listeners| {
    listeners
      .borrow()
      .iter()
      .map(|listener| Snap {
        id: listener.id,
        type_name: listener.type_name.clone(),
        callback: v8::Global::new(
          scope,
          v8::Local::new(scope, &listener.callback),
        ),
        once: listener.once,
        passive: listener.passive,
        capture: listener.capture,
      })
      .collect()
  })
}
