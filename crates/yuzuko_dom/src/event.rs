//! `Event` and its subclasses. State lives on the cppgc object.

use std::cell::{Cell, RefCell};

use yuzuko_core::op::{
  Js, OpError, OpException, host_ref, traced, traced_local,
};
use yuzuko_macros::op;

use crate::util::{self, key};

pub struct Event {
  type_name: RefCell<String>,
  bubbles: Cell<bool>,
  cancelable: Cell<bool>,
  composed: Cell<bool>,
  time_stamp: f64,
  default_prevented: Cell<bool>,
  phase: Cell<i32>,
  target: RefCell<Option<v8::TracedReference<v8::Object>>>,
  current: RefCell<Option<v8::TracedReference<v8::Object>>>,
  /// Target first, then ancestors. Used by `composedPath()` while dispatching.
  path: RefCell<Vec<v8::TracedReference<v8::Object>>>,
  trusted: Cell<bool>,
  stopped: Cell<bool>,
  immediate: Cell<bool>,
  dispatching: Cell<bool>,
  passive: Cell<bool>,
}

unsafe impl v8::cppgc::GarbageCollected for Event {
  fn trace(&self, visitor: &mut v8::cppgc::Visitor) {
    if let Ok(target) = self.target.try_borrow() {
      visitor.trace(&*target);
    }
    if let Ok(current) = self.current.try_borrow() {
      visitor.trace(&*current);
    }
    if let Ok(path) = self.path.try_borrow() {
      for object in path.iter() {
        visitor.trace(object);
      }
    }
  }

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"Event"
  }
}

impl Event {
  pub fn new(
    type_name: impl Into<String>,
    bubbles: bool,
    cancelable: bool,
    composed: bool,
    trusted: bool,
  ) -> Self {
    Self {
      type_name: RefCell::new(type_name.into()),
      bubbles: Cell::new(bubbles),
      cancelable: Cell::new(cancelable),
      composed: Cell::new(composed),
      time_stamp: yuzuko_core::performance_now_ms(),
      default_prevented: Cell::new(false),
      phase: Cell::new(0),
      target: RefCell::new(None),
      current: RefCell::new(None),
      path: RefCell::new(Vec::new()),
      trusted: Cell::new(trusted),
      stopped: Cell::new(false),
      immediate: Cell::new(false),
      dispatching: Cell::new(false),
      passive: Cell::new(false),
    }
  }

  pub(crate) fn event_name(&self) -> String {
    self.type_name.borrow().clone()
  }

  pub(crate) fn prevent(&self) {
    if self.cancelable.get() && !self.passive.get() {
      self.default_prevented.set(true);
    }
  }

  pub fn from_args(
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
    trusted: bool,
  ) -> Result<Self, OpError> {
    if args.length() < 1 {
      return Err(OpError::type_("Event requires a type"));
    }
    let init = util::init_arg(args);
    Ok(Self::new(
      util::try_string(scope, args.get(0))?,
      util::dict_bool(scope, init, "bubbles"),
      util::dict_bool(scope, init, "cancelable"),
      util::dict_bool(scope, init, "composed"),
      trusted,
    ))
  }
}

#[op]
impl Event {
  #[constructor]
  fn new(
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<Event, OpError> {
    Event::from_args(scope, args, false)
  }

  #[getter]
  #[rename("type")]
  fn type_(&self) -> String {
    self.type_name.borrow().clone()
  }

  #[getter]
  fn bubbles(&self) -> bool {
    self.bubbles.get()
  }

  #[getter]
  fn cancelable(&self) -> bool {
    self.cancelable.get()
  }

  #[getter]
  fn composed(&self) -> bool {
    self.composed.get()
  }

  #[getter]
  fn time_stamp(&self) -> f64 {
    self.time_stamp
  }

  #[getter]
  fn default_prevented(&self) -> bool {
    self.default_prevented.get()
  }

  #[getter]
  fn event_phase(&self) -> i32 {
    self.phase.get()
  }

  #[getter]
  fn target(&self, scope: &mut v8::PinScope) -> Js {
    traced_object(scope, &self.target)
  }

  #[getter]
  fn current_target(&self, scope: &mut v8::PinScope) -> Js {
    traced_object(scope, &self.current)
  }

  #[getter]
  #[rename("srcElement")]
  fn src_element(&self, scope: &mut v8::PinScope) -> Js {
    traced_object(scope, &self.target)
  }

  #[getter]
  #[rename("returnValue")]
  fn return_value(&self) -> bool {
    !self.default_prevented.get()
  }

  #[setter]
  #[rename("returnValue")]
  fn return_value(&self, value: bool) {
    if !value {
      self.prevent();
    }
  }

  #[construct_hook]
  fn finish(&self, scope: &mut v8::PinScope, object: v8::Local<v8::Object>) {
    let _ = self;
    define_is_trusted(scope, object);
  }

  fn prevent_default(&self) {
    self.prevent();
  }

  fn stop_propagation(&self) {
    self.stopped.set(true);
  }

  fn stop_immediate_propagation(&self) {
    self.stopped.set(true);
    self.immediate.set(true);
  }

  fn composed_path(&self, scope: &mut v8::PinScope) -> Js {
    if !self.dispatching.get() {
      let array = v8::Array::new(scope, 0);
      let value: v8::Local<v8::Value> = array.into();
      return Js(v8::Global::new(scope, value));
    }
    let locals: Vec<v8::Local<v8::Object>> = self
      .path
      .borrow()
      .iter()
      .filter_map(|reference| traced_local(scope, reference))
      .collect();
    let array = v8::Array::new(scope, locals.len() as i32);
    for (index, object) in locals.iter().enumerate() {
      array.set_index(scope, index as u32, (*object).into());
    }
    let value: v8::Local<v8::Value> = array.into();
    Js(v8::Global::new(scope, value))
  }

  fn init_event(
    &self,
    type_name: String,
    bubbles: Option<bool>,
    cancelable: Option<bool>,
  ) {
    if self.dispatching.get() {
      return;
    }
    *self.type_name.borrow_mut() = type_name;
    self.bubbles.set(bubbles.unwrap_or(false));
    self.cancelable.set(cancelable.unwrap_or(false));
  }
}

pub(crate) fn install(scope: &mut v8::PinScope) {
  Event::install(scope);
  let getter = v8::Function::new(scope, is_trusted_getter).unwrap();
  IS_TRUSTED.with(|slot| {
    *slot.borrow_mut() = Some(v8::Global::new(scope, getter));
  });
  define_constants(scope);
  CustomEvent::install(scope);
  ErrorEvent::install(scope);
  PromiseRejectionEvent::install(scope);
}

thread_local! {
  static IS_TRUSTED: RefCell<Option<v8::Global<v8::Function>>> =
    const { RefCell::new(None) };
}

pub(crate) fn clear_is_trusted() {
  IS_TRUSTED.with(|slot| *slot.borrow_mut() = None);
}

pub fn define_is_trusted(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
) {
  let getter = IS_TRUSTED.with(|slot| slot.borrow().clone());
  let Some(getter) = getter else {
    return;
  };
  let getter = v8::Local::new(scope, getter);
  let global = scope.get_current_context().global(scope);
  let Some(object_ctor) = global
    .get(scope, key(scope, "Object").into())
    .and_then(|value| v8::Local::<v8::Object>::try_from(value).ok())
  else {
    return;
  };
  let Some(define) = object_ctor
    .get(scope, key(scope, "defineProperty").into())
    .and_then(|value| v8::Local::<v8::Function>::try_from(value).ok())
  else {
    return;
  };
  let descriptor = v8::Object::new(scope);
  descriptor.set(scope, key(scope, "get").into(), getter.into());
  descriptor.set(
    scope,
    key(scope, "enumerable").into(),
    v8::Boolean::new(scope, true).into(),
  );
  descriptor.set(
    scope,
    key(scope, "configurable").into(),
    v8::Boolean::new(scope, false).into(),
  );
  let name = key(scope, "isTrusted");
  let _ = define.call(
    scope,
    object_ctor.into(),
    &[object.into(), name.into(), descriptor.into()],
  );
}

fn is_trusted_getter(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut ret: v8::ReturnValue,
) {
  let object = args.this();
  let Some(host) = (unsafe { host_ref::<Event>(scope, object) }) else {
    OpException::throw(OpError::type_("Illegal invocation"), scope);
    return;
  };
  ret.set(v8::Boolean::new(scope, host.get().trusted.get()).into());
}

fn define_constants(scope: &mut v8::PinScope) {
  let constructor = Event::template(scope).get_function(scope).unwrap();
  let Some(prototype) = constructor
    .get(scope, key(scope, "prototype").into())
    .and_then(|value| v8::Local::<v8::Object>::try_from(value).ok())
  else {
    return;
  };
  for (name, value) in [
    ("NONE", 0),
    ("CAPTURING_PHASE", 1),
    ("AT_TARGET", 2),
    ("BUBBLING_PHASE", 3),
  ] {
    let number = v8::Integer::new(scope, value);
    let name = key(scope, name);
    let _ = constructor.define_own_property(
      scope,
      name.into(),
      number.into(),
      v8::PropertyAttribute::READ_ONLY,
    );
    let _ = prototype.define_own_property(
      scope,
      name.into(),
      number.into(),
      v8::PropertyAttribute::READ_ONLY,
    );
  }
}

fn traced_object(
  scope: &mut v8::PinScope,
  slot: &RefCell<Option<v8::TracedReference<v8::Object>>>,
) -> Js {
  let local = slot
    .borrow()
    .as_ref()
    .and_then(|reference| traced_local(scope, reference));
  match local {
    Some(object) => {
      let value: v8::Local<v8::Value> = object.into();
      Js(v8::Global::new(scope, value))
    }
    None => Js::null(scope),
  }
}

pub(crate) fn with_event<T>(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  body: impl FnOnce(&Event) -> T,
) -> Option<T> {
  let host = unsafe { host_ref::<Event>(scope, object) }?;
  Some(body(host.get()))
}

pub(crate) fn event_type(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
) -> Option<String> {
  with_event(scope, object, |event| event.type_name.borrow().clone())
}

pub(crate) fn is_immediate(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
) -> bool {
  with_event(scope, object, |event| event.immediate.get()).unwrap_or(false)
}

pub(crate) fn is_dispatching(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
) -> bool {
  with_event(scope, object, |event| event.dispatching.get()).unwrap_or(false)
}

pub(crate) fn begin_dispatch(
  scope: &mut v8::PinScope,
  event: v8::Local<v8::Object>,
  target: v8::Local<v8::Object>,
) {
  let target_ref = traced(scope, target);
  with_event(scope, event, |event| {
    event.dispatching.set(true);
    event.phase.set(2);
    *event.target.borrow_mut() = Some(target_ref);
    *event.current.borrow_mut() = None;
  });
}

pub(crate) fn set_path(
  scope: &mut v8::PinScope,
  event: v8::Local<v8::Object>,
  path: &[v8::Local<v8::Object>],
) {
  let path: Vec<_> = path
    .iter()
    .copied()
    .map(|object| traced(scope, object))
    .collect();
  with_event(scope, event, |event| {
    *event.path.borrow_mut() = path;
  });
}

pub(crate) fn set_current(
  scope: &mut v8::PinScope,
  event: v8::Local<v8::Object>,
  current: v8::Local<v8::Object>,
  phase: i32,
) {
  let current_ref = traced(scope, current);
  with_event(scope, event, |event| {
    event.phase.set(phase);
    *event.current.borrow_mut() = Some(current_ref);
  });
}

pub(crate) fn bubbles(
  scope: &mut v8::PinScope,
  event: v8::Local<v8::Object>,
) -> bool {
  with_event(scope, event, |event| event.bubbles.get()).unwrap_or(false)
}

pub(crate) fn is_stopped(
  scope: &mut v8::PinScope,
  event: v8::Local<v8::Object>,
) -> bool {
  with_event(scope, event, |event| event.stopped.get()).unwrap_or(false)
}

pub(crate) fn finish_dispatch(
  scope: &mut v8::PinScope,
  event: v8::Local<v8::Object>,
) {
  with_event(scope, event, |event| {
    event.phase.set(0);
    *event.current.borrow_mut() = None;
    event.path.borrow_mut().clear();
    event.dispatching.set(false);
    event.passive.set(false);
  });
}

pub(crate) fn set_passive(
  scope: &mut v8::PinScope,
  event: v8::Local<v8::Object>,
  passive: bool,
) {
  with_event(scope, event, |event| event.passive.set(passive));
}

pub(crate) fn was_prevented(
  scope: &mut v8::PinScope,
  event: v8::Local<v8::Object>,
) -> bool {
  with_event(scope, event, |event| event.default_prevented.get())
    .unwrap_or(false)
}

#[repr(C)]
pub struct CustomEvent {
  base: Event,
  detail: RefCell<Option<v8::TracedReference<v8::Value>>>,
}

unsafe impl v8::cppgc::GarbageCollected for CustomEvent {
  fn trace(&self, visitor: &mut v8::cppgc::Visitor) {
    v8::cppgc::GarbageCollected::trace(&self.base, visitor);
    if let Ok(detail) = self.detail.try_borrow() {
      visitor.trace(&*detail);
    }
  }

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"CustomEvent"
  }
}

#[op(inherit = Event)]
impl CustomEvent {
  #[constructor]
  fn new(
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<CustomEvent, OpError> {
    let base = Event::from_args(scope, args, false)?;
    let detail = custom_detail(scope, util::init_arg(args));
    Ok(CustomEvent {
      base,
      detail: RefCell::new(detail),
    })
  }

  #[construct_hook]
  fn finish(&self, scope: &mut v8::PinScope, object: v8::Local<v8::Object>) {
    let _ = self;
    define_is_trusted(scope, object);
  }

  #[getter]
  fn detail(&self, scope: &mut v8::PinScope) -> Js {
    let local = self
      .detail
      .borrow()
      .as_ref()
      .and_then(|reference| traced_local(scope, reference));
    match local {
      Some(value) => Js(v8::Global::new(scope, value)),
      None => Js::null(scope),
    }
  }
}

fn custom_detail(
  scope: &mut v8::PinScope,
  init: Option<v8::Local<v8::Value>>,
) -> Option<v8::TracedReference<v8::Value>> {
  if !util::has_own(scope, init, "detail") {
    return None;
  }
  let value = util::dict_get(scope, init, "detail")
    .unwrap_or_else(|| v8::undefined(scope).into());
  Some(traced(scope, value))
}

#[repr(C)]
pub struct ErrorEvent {
  base: Event,
  message: String,
  filename: String,
  lineno: f64,
  colno: f64,
  error: RefCell<Option<v8::TracedReference<v8::Value>>>,
}

unsafe impl v8::cppgc::GarbageCollected for ErrorEvent {
  fn trace(&self, visitor: &mut v8::cppgc::Visitor) {
    v8::cppgc::GarbageCollected::trace(&self.base, visitor);
    if let Ok(error) = self.error.try_borrow() {
      visitor.trace(&*error);
    }
  }

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"ErrorEvent"
  }
}

impl ErrorEvent {
  pub(crate) fn worker(
    scope: &mut v8::PinScope,
    message: String,
    filename: String,
    line: f64,
    column: f64,
  ) -> Self {
    Self {
      base: Event::new("error", false, true, false, true),
      message,
      filename,
      lineno: line,
      colno: column,
      error: RefCell::new(Some(traced(scope, v8::undefined(scope).into()))),
    }
  }

  pub(crate) fn trusted(
    scope: &mut v8::PinScope,
    message: String,
    error: v8::Local<v8::Value>,
  ) -> Self {
    Self {
      base: Event::new("error", false, true, false, true),
      message,
      filename: String::new(),
      lineno: 0.0,
      colno: 0.0,
      error: RefCell::new(Some(traced(scope, error))),
    }
  }
}

#[op(inherit = Event)]
impl ErrorEvent {
  #[constructor]
  fn new(
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<ErrorEvent, OpError> {
    let base = Event::from_args(scope, args, false)?;
    let init = util::init_arg(args);
    let error = init.map(|init| {
      let value = util::dict_get(scope, Some(init), "error")
        .unwrap_or_else(|| v8::undefined(scope).into());
      traced(scope, value)
    });
    Ok(ErrorEvent {
      base,
      message: util::dict_string(scope, init, "message"),
      filename: util::dict_string(scope, init, "filename"),
      lineno: util::dict_number(scope, init, "lineno"),
      colno: util::dict_number(scope, init, "colno"),
      error: RefCell::new(error),
    })
  }

  #[construct_hook]
  fn finish(&self, scope: &mut v8::PinScope, object: v8::Local<v8::Object>) {
    let _ = self;
    define_is_trusted(scope, object);
  }

  #[getter]
  fn message(&self) -> String {
    self.message.clone()
  }

  #[getter]
  fn filename(&self) -> String {
    self.filename.clone()
  }

  #[getter]
  fn lineno(&self) -> f64 {
    self.lineno
  }

  #[getter]
  fn colno(&self) -> f64 {
    self.colno
  }

  #[getter]
  fn error(&self, scope: &mut v8::PinScope) -> Js {
    let local = self
      .error
      .borrow()
      .as_ref()
      .and_then(|reference| traced_local(scope, reference));
    match local {
      Some(value) => Js(v8::Global::new(scope, value)),
      None => {
        let undefined: v8::Local<v8::Value> = v8::undefined(scope).into();
        Js(v8::Global::new(scope, undefined))
      }
    }
  }
}

pub(crate) fn error_fields(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
) -> Option<(String, String, f64, f64, v8::Global<v8::Value>)> {
  let host = unsafe { host_ref::<ErrorEvent>(scope, object) }?;
  let event = host.get();
  let error = event
    .error
    .borrow()
    .as_ref()
    .and_then(|reference| traced_local(scope, reference))
    .map(|value| v8::Global::new(scope, value))
    .unwrap_or_else(|| {
      let undefined: v8::Local<v8::Value> = v8::undefined(scope).into();
      v8::Global::new(scope, undefined)
    });
  Some((
    event.message.clone(),
    event.filename.clone(),
    event.lineno,
    event.colno,
    error,
  ))
}

#[repr(C)]
pub struct PromiseRejectionEvent {
  base: Event,
  promise: RefCell<Option<v8::TracedReference<v8::Value>>>,
  reason: RefCell<Option<v8::TracedReference<v8::Value>>>,
}

unsafe impl v8::cppgc::GarbageCollected for PromiseRejectionEvent {
  fn trace(&self, visitor: &mut v8::cppgc::Visitor) {
    v8::cppgc::GarbageCollected::trace(&self.base, visitor);
    if let Ok(promise) = self.promise.try_borrow() {
      visitor.trace(&*promise);
    }
    if let Ok(reason) = self.reason.try_borrow() {
      visitor.trace(&*reason);
    }
  }

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"PromiseRejectionEvent"
  }
}

impl PromiseRejectionEvent {
  pub(crate) fn rejection(
    scope: &mut v8::PinScope,
    type_name: &str,
    promise: v8::Local<v8::Value>,
    reason: v8::Local<v8::Value>,
  ) -> Self {
    Self {
      base: Event::new(type_name, false, true, false, false),
      promise: RefCell::new(Some(traced(scope, promise))),
      reason: RefCell::new(Some(traced(scope, reason))),
    }
  }
}

#[op(inherit = Event)]
impl PromiseRejectionEvent {
  #[constructor]
  fn new(
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<PromiseRejectionEvent, OpError> {
    let base = Event::from_args(scope, args, false)?;
    let init = util::init_arg(args);
    if !util::has_own(scope, init, "promise") {
      return Err(OpError::type_("PromiseRejectionEvent requires a promise"));
    }
    let promise = util::dict_get(scope, init, "promise")
      .unwrap_or_else(|| v8::undefined(scope).into());
    let reason = util::dict_get(scope, init, "reason")
      .unwrap_or_else(|| v8::undefined(scope).into());
    Ok(PromiseRejectionEvent {
      base,
      promise: RefCell::new(Some(traced(scope, promise))),
      reason: RefCell::new(Some(traced(scope, reason))),
    })
  }

  #[construct_hook]
  fn finish(&self, scope: &mut v8::PinScope, object: v8::Local<v8::Object>) {
    let _ = self;
    define_is_trusted(scope, object);
  }

  #[getter]
  fn promise(&self, scope: &mut v8::PinScope) -> Js {
    traced_value(scope, &self.promise)
  }

  #[getter]
  fn reason(&self, scope: &mut v8::PinScope) -> Js {
    traced_value(scope, &self.reason)
  }
}

fn traced_value(
  scope: &mut v8::PinScope,
  slot: &RefCell<Option<v8::TracedReference<v8::Value>>>,
) -> Js {
  let local = slot
    .borrow()
    .as_ref()
    .and_then(|reference| traced_local(scope, reference));
  match local {
    Some(value) => Js(v8::Global::new(scope, value)),
    None => {
      let undefined: v8::Local<v8::Value> = v8::undefined(scope).into();
      Js(v8::Global::new(scope, undefined))
    }
  }
}
