//! `AbortSignal` and `AbortController`. The public constructor is illegal;
//! host code wraps instances directly.

use std::cell::{Cell, RefCell};

use v8::cppgc::GetRustObj;
use yuzuko_core::op::{
  HostObject, Js, OpError, host_gc, host_ref, traced, traced_local,
};
use yuzuko_macros::op;

use crate::event::Event;
use crate::target::{self, EventTarget};
use crate::util::key;

pub struct AbortSignal {
  pub(crate) base: EventTarget,
  aborted: Cell<bool>,
  /// Created by `AbortSignal.any`. Only non-dependent signals abort it.
  dependent: Cell<bool>,
  reason: RefCell<Option<v8::TracedReference<v8::Value>>>,
  onabort: RefCell<Option<v8::TracedReference<v8::Value>>>,
  /// Wrapper for this signal. The wrapper also points back here, and that
  /// cycle is traced, so it does not root the signal by itself.
  wrapper: RefCell<Option<v8::TracedReference<v8::Object>>>,
  /// Signals this one follows. Weak, so a composite signal does not keep
  /// its sources alive.
  source_signals: RefCell<Vec<v8::cppgc::WeakMember<HostObject<AbortSignal>>>>,
  /// Signals that follow this one. Weak, so a source does not keep an
  /// unused composite signal alive.
  dependent_signals:
    RefCell<Vec<v8::cppgc::WeakMember<HostObject<AbortSignal>>>>,
  /// Composite signals that still have abort listeners. Strong only while
  /// those listeners exist, which is what keeps them observable.
  observed: RefCell<Vec<v8::cppgc::Member<HostObject<AbortSignal>>>>,
}

unsafe impl v8::cppgc::GarbageCollected for AbortSignal {
  fn trace(&self, visitor: &mut v8::cppgc::Visitor) {
    v8::cppgc::GarbageCollected::trace(&self.base, visitor);
    // Drop the `Ref` before tracing. A GC callback can abort this signal.
    let reason = self
      .reason
      .try_borrow_mut()
      .ok()
      .and_then(|mut slot| slot.take());
    visitor.trace(&reason);
    if let Some(reason) = reason
      && let Ok(mut slot) = self.reason.try_borrow_mut()
      && slot.is_none()
    {
      *slot = Some(reason);
    }
    if let Ok(onabort) = self.onabort.try_borrow() {
      visitor.trace(&*onabort);
    }
    if let Ok(wrapper) = self.wrapper.try_borrow() {
      visitor.trace(&*wrapper);
    }
    if let Ok(sources) = self.source_signals.try_borrow() {
      for source in sources.iter() {
        visitor.trace(source);
      }
    }
    if let Ok(dependents) = self.dependent_signals.try_borrow() {
      for dependent in dependents.iter() {
        visitor.trace(dependent);
      }
    }
    if let Ok(observed) = self.observed.try_borrow() {
      for dependent in observed.iter() {
        visitor.trace(dependent);
      }
    }
  }

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"AbortSignal"
  }
}

impl AbortSignal {
  fn fresh() -> Self {
    Self {
      base: EventTarget::new(),
      aborted: Cell::new(false),
      dependent: Cell::new(false),
      reason: RefCell::new(None),
      onabort: RefCell::new(None),
      wrapper: RefCell::new(None),
      source_signals: RefCell::new(Vec::new()),
      dependent_signals: RefCell::new(Vec::new()),
      observed: RefCell::new(Vec::new()),
    }
  }

  fn aborted_with(
    scope: &mut v8::PinScope,
    reason: Option<v8::Local<v8::Value>>,
  ) -> Self {
    let reason = abort_reason(scope, reason);
    let signal = Self::fresh();
    signal.aborted.set(true);
    *signal.reason.borrow_mut() = Some(traced(scope, reason));
    signal
  }
}

#[op(inherit = EventTarget)]
impl AbortSignal {
  #[constructor]
  fn new() -> Result<AbortSignal, OpError> {
    Err(OpError::type_("Illegal constructor"))
  }

  #[getter]
  fn aborted(&self) -> bool {
    self.aborted.get()
  }

  #[getter]
  fn reason(&self, scope: &mut v8::PinScope) -> Js {
    match self
      .reason
      .borrow()
      .as_ref()
      .and_then(|reference| traced_local(scope, reference))
    {
      Some(value) => Js(v8::Global::new(scope, value)),
      None => undefined(scope),
    }
  }

  #[getter]
  fn onabort(&self, scope: &mut v8::PinScope) -> Js {
    js_or_null(scope, &self.onabort)
  }

  #[setter]
  fn onabort(&self, scope: &mut v8::PinScope, value: v8::Local<v8::Value>) {
    let previous = self
      .onabort
      .borrow()
      .as_ref()
      .and_then(|reference| traced_local(scope, reference));
    if let Some(previous) = previous {
      self.base.remove_callback(scope, "abort", previous, false);
    }
    if value.is_function() {
      self
        .base
        .add_callback(scope, "abort", value, false, false, false);
      *self.onabort.borrow_mut() = Some(traced(scope, value));
    } else {
      *self.onabort.borrow_mut() = None;
    }
    let wrapper = self
      .wrapper
      .borrow()
      .as_ref()
      .and_then(|reference| traced_local(scope, reference));
    if let Some(wrapper) = wrapper {
      sync_observers(scope, wrapper);
    }
  }

  #[rename("throwIfAborted")]
  fn throw_if_aborted(&self, scope: &mut v8::PinScope) -> Result<(), OpError> {
    if !self.aborted.get() {
      return Ok(());
    }
    let stored = self.reason.borrow_mut().take();
    let reason = stored
      .as_ref()
      .and_then(|reference| traced_local(scope, reference));
    if let Some(stored) = stored {
      let mut slot = self.reason.borrow_mut();
      if slot.is_none() {
        *slot = Some(stored);
      }
    }
    match reason {
      Some(reason) => Err(OpError::thrown(scope, reason)),
      None => Err(OpError::dom("AbortError", "The operation was aborted.")),
    }
  }

  #[static_method]
  fn abort(
    scope: &mut v8::PinScope,
    reason: Option<v8::Local<v8::Value>>,
  ) -> Js {
    let created = AbortSignal::aborted_with(scope, reason);
    let signal = adopt(scope, created);
    let value: v8::Local<v8::Value> = signal.into();
    Js(v8::Global::new(scope, value))
  }

  #[static_method]
  fn timeout(scope: &mut v8::PinScope, milliseconds: f64) -> Js {
    let signal = adopt(scope, Self::fresh());
    let function = v8::Function::new(scope, timeout_fired).unwrap();
    yuzuko_core::schedule_callback(
      scope,
      function,
      milliseconds,
      &[signal.into()],
    );
    let value: v8::Local<v8::Value> = signal.into();
    Js(v8::Global::new(scope, value))
  }

  #[static_method]
  fn any(
    scope: &mut v8::PinScope,
    signals: v8::Local<v8::Value>,
  ) -> Result<Js, OpError> {
    let signal = adopt(scope, Self::fresh());
    follow_signals(scope, signal, signals)?;
    let value: v8::Local<v8::Value> = signal.into();
    Ok(Js(v8::Global::new(scope, value)))
  }
}

pub struct AbortController {
  signal: RefCell<Option<v8::TracedReference<v8::Object>>>,
}

unsafe impl v8::cppgc::GarbageCollected for AbortController {
  fn trace(&self, visitor: &mut v8::cppgc::Visitor) {
    if let Ok(signal) = self.signal.try_borrow() {
      visitor.trace(&*signal);
    }
  }

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"AbortController"
  }
}

#[op]
impl AbortController {
  #[constructor]
  fn new(scope: &mut v8::PinScope) -> AbortController {
    let signal = adopt(scope, AbortSignal::fresh());
    AbortController {
      signal: RefCell::new(Some(traced(scope, signal))),
    }
  }

  #[getter]
  fn signal(&self, scope: &mut v8::PinScope) -> Js {
    match self
      .signal
      .borrow()
      .as_ref()
      .and_then(|reference| traced_local(scope, reference))
    {
      Some(signal) => {
        let value: v8::Local<v8::Value> = signal.into();
        Js(v8::Global::new(scope, value))
      }
      None => undefined(scope),
    }
  }

  fn abort(
    &self,
    scope: &mut v8::PinScope,
    reason: Option<v8::Local<v8::Value>>,
  ) {
    let Some(signal) = self
      .signal
      .borrow()
      .as_ref()
      .and_then(|reference| traced_local(scope, reference))
    else {
      return;
    };
    abort_signal(scope, signal, reason);
  }
}

pub(crate) fn install(scope: &mut v8::PinScope) {
  AbortSignal::install(scope);
  AbortController::install(scope);
}

pub(crate) fn is_aborted(
  scope: &mut v8::PinScope,
  signal: v8::Local<v8::Object>,
) -> Result<bool, OpError> {
  let Some(host) = (unsafe { host_ref::<AbortSignal>(scope, signal) }) else {
    return Err(OpError::type_("AbortSignal required"));
  };
  Ok(host.get().aborted.get())
}

pub(crate) fn abort_signal(
  scope: &mut v8::PinScope,
  signal: v8::Local<v8::Object>,
  reason: Option<v8::Local<v8::Value>>,
) {
  if is_aborted(scope, signal).unwrap_or(true) {
    return;
  }
  let reason = abort_reason(scope, reason);
  let reason = traced(scope, reason);
  mark_aborted(scope, signal, &reason);
  // Dependents are marked before this signal's abort event. Their own
  // abort events run after it, in link order.
  let dependents = live_dependents(scope, signal);
  for dependent in &dependents {
    let dependent = v8::Local::new(scope, dependent);
    mark_aborted(scope, dependent, &reason);
  }
  clear_graph(scope, signal);
  dispatch_abort(scope, signal);
  for dependent in &dependents {
    let dependent = v8::Local::new(scope, dependent);
    clear_graph(scope, dependent);
    dispatch_abort(scope, dependent);
  }
}

pub(crate) fn sync_observers(
  scope: &mut v8::PinScope,
  signal: v8::Local<v8::Object>,
) {
  let Some(host) = (unsafe { host_ref::<AbortSignal>(scope, signal) }) else {
    return;
  };
  if !host.get().dependent.get() || host.get().aborted.get() {
    return;
  }
  let (wanted, dependent, sources) = {
    let wanted = host.get().base.listens_for("abort");
    let Some(dependent) = (unsafe { host_gc::<AbortSignal>(scope, signal) })
    else {
      return;
    };
    let sources = live_sources(host.get());
    (wanted, dependent, sources)
  };
  for source in sources {
    let source = &unsafe { source.as_ref() }.value;
    if wanted {
      remember_observed(source, &dependent);
    } else {
      forget_observed(source, &dependent);
    }
  }
}

fn abort_reason<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  reason: Option<v8::Local<'_, v8::Value>>,
) -> v8::Local<'s, v8::Value> {
  if let Some(reason) = reason
    && !reason.is_undefined()
  {
    return yuzuko_core::op::root(scope, reason);
  }
  yuzuko_core::op::new_dom_exception(
    scope,
    "AbortError",
    "The operation was aborted.",
  )
  .map(Into::into)
  .unwrap_or_else(|| v8::undefined(scope).into())
}

fn timeout_fired(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut _ret: v8::ReturnValue,
) {
  let Ok(signal) = v8::Local::<v8::Object>::try_from(args.get(0)) else {
    return;
  };
  let reason = yuzuko_core::op::new_dom_exception(
    scope,
    "TimeoutError",
    "The operation was aborted due to timeout.",
  )
  .map(Into::into);
  abort_signal(scope, signal, reason);
}

fn adopt<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  signal: AbortSignal,
) -> v8::Local<'s, v8::Object> {
  let object = unsafe { yuzuko_core::op::wrap_instance(scope, signal) };
  if let Some(host) = unsafe { host_ref::<AbortSignal>(scope, object) } {
    *host.get().wrapper.borrow_mut() = Some(traced(scope, object));
  }
  object
}

fn follow_signals(
  scope: &mut v8::PinScope,
  signal: v8::Local<v8::Object>,
  signals: v8::Local<v8::Value>,
) -> Result<(), OpError> {
  let sources = read_signals(scope, signals)?;
  for source in &sources {
    let source = v8::Local::new(scope, source);
    if is_aborted(scope, source)? {
      let reason = signal_reason(scope, source);
      abort_signal(scope, signal, reason);
      return Ok(());
    }
  }
  if let Some(host) = unsafe { host_ref::<AbortSignal>(scope, signal) } {
    host.get().dependent.set(true);
  }
  for source in &sources {
    let source = v8::Local::new(scope, source);
    link_signal(scope, signal, source);
  }
  Ok(())
}

fn read_signals(
  scope: &mut v8::PinScope,
  signals: v8::Local<v8::Value>,
) -> Result<Vec<v8::Global<v8::Object>>, OpError> {
  let Ok(iterable) = v8::Local::<v8::Object>::try_from(signals) else {
    return Err(OpError::type_("AbortSignal.any requires an iterable"));
  };
  let symbol = v8::Symbol::get_iterator(scope);
  let Some(next) = iterable
    .get(scope, symbol.into())
    .and_then(|value| v8::Local::<v8::Function>::try_from(value).ok())
  else {
    return Err(OpError::type_("AbortSignal.any requires an iterable"));
  };
  let Some(iterator) = next
    .call(scope, iterable.into(), &[])
    .and_then(|value| v8::Local::<v8::Object>::try_from(value).ok())
  else {
    return Err(OpError::type_("AbortSignal.any requires an iterable"));
  };
  let mut sources = Vec::new();
  loop {
    let Some(step) = iterator
      .get(scope, key(scope, "next").into())
      .and_then(|value| v8::Local::<v8::Function>::try_from(value).ok())
      .and_then(|step| step.call(scope, iterator.into(), &[]))
      .and_then(|value| v8::Local::<v8::Object>::try_from(value).ok())
    else {
      return Err(OpError::type_("AbortSignal.any requires an iterable"));
    };
    let done = step
      .get(scope, key(scope, "done").into())
      .is_some_and(|value| value.boolean_value(scope));
    if done {
      return Ok(sources);
    }
    let Some(item) = step.get(scope, key(scope, "value").into()) else {
      return Err(OpError::type_("AbortSignal.any requires an iterable"));
    };
    let Ok(source) = v8::Local::<v8::Object>::try_from(item) else {
      return Err(OpError::type_("AbortSignal.any requires signals"));
    };
    if unsafe { host_ref::<AbortSignal>(scope, source) }.is_none() {
      return Err(OpError::type_("AbortSignal.any requires signals"));
    }
    sources.push(v8::Global::new(scope, source));
  }
}

fn link_signal(
  scope: &mut v8::PinScope,
  dependent: v8::Local<v8::Object>,
  source: v8::Local<v8::Object>,
) {
  let follows = unsafe { host_ref::<AbortSignal>(scope, source) }
    .is_some_and(|host| host.get().dependent.get());
  if follows {
    let roots = unsafe { host_ref::<AbortSignal>(scope, source) }
      .map(|host| live_sources(host.get()))
      .unwrap_or_default();
    for root in roots {
      link_root(scope, dependent, &root);
    }
    return;
  }
  let Some(root) = (unsafe { host_gc::<AbortSignal>(scope, source) }) else {
    return;
  };
  link_root(scope, dependent, &root);
}

fn link_root(
  scope: &mut v8::PinScope,
  dependent: v8::Local<v8::Object>,
  source: &v8::cppgc::UnsafePtr<HostObject<AbortSignal>>,
) {
  let Some(dependent_ptr) =
    (unsafe { host_gc::<AbortSignal>(scope, dependent) })
  else {
    return;
  };
  {
    let signal = &unsafe { dependent_ptr.as_ref() }.value;
    let mut sources = signal.source_signals.borrow_mut();
    if !weak_contains(&sources, source) {
      sources.push(v8::cppgc::WeakMember::new(source));
    }
  }
  {
    let signal = &unsafe { source.as_ref() }.value;
    let mut dependents = signal.dependent_signals.borrow_mut();
    if !weak_contains(&dependents, &dependent_ptr) {
      dependents.push(v8::cppgc::WeakMember::new(&dependent_ptr));
    }
  }
  sync_observers(scope, dependent);
}

fn weak_contains(
  list: &[v8::cppgc::WeakMember<HostObject<AbortSignal>>],
  signal: &v8::cppgc::UnsafePtr<HostObject<AbortSignal>>,
) -> bool {
  list
    .iter()
    .any(|member| member.get_rust_obj() == signal.get_rust_obj())
}

fn live_sources(
  signal: &AbortSignal,
) -> Vec<v8::cppgc::UnsafePtr<HostObject<AbortSignal>>> {
  let mut sources = Vec::new();
  for source in signal.source_signals.borrow().iter() {
    if unsafe { source.get() }.is_none() {
      continue;
    }
    if let Some(pointer) = unsafe { v8::cppgc::UnsafePtr::new(source) } {
      sources.push(pointer);
    }
  }
  sources
}

fn live_dependents(
  scope: &mut v8::PinScope,
  signal: v8::Local<v8::Object>,
) -> Vec<v8::Global<v8::Object>> {
  // Copy the pointers before touching V8. Tracing the weak list must not
  // run while this `RefCell` is borrowed.
  let pointers = {
    let Some(host) = (unsafe { host_ref::<AbortSignal>(scope, signal) }) else {
      return Vec::new();
    };
    let dependents = host.get().dependent_signals.borrow();
    let mut pointers = Vec::new();
    for dependent in dependents.iter() {
      if unsafe { dependent.get() }.is_none() {
        continue;
      }
      if let Some(pointer) = unsafe { v8::cppgc::UnsafePtr::new(dependent) } {
        pointers.push(pointer);
      }
    }
    pointers
  };
  let mut dependents = Vec::new();
  for dependent in pointers {
    let object = unsafe { dependent.as_ref() };
    if object.value.aborted.get() {
      continue;
    }
    let Some(wrapper) = object
      .value
      .wrapper
      .borrow()
      .as_ref()
      .and_then(|reference| traced_local(scope, reference))
    else {
      continue;
    };
    dependents.push(v8::Global::new(scope, wrapper));
  }
  dependents
}

fn remember_observed(
  source: &AbortSignal,
  dependent: &v8::cppgc::UnsafePtr<HostObject<AbortSignal>>,
) {
  let mut observed = source.observed.borrow_mut();
  if observed
    .iter()
    .any(|member| member.get_rust_obj() == dependent.get_rust_obj())
  {
    return;
  }
  observed.push(v8::cppgc::Member::new(dependent));
}

fn forget_observed(
  source: &AbortSignal,
  dependent: &v8::cppgc::UnsafePtr<HostObject<AbortSignal>>,
) {
  source
    .observed
    .borrow_mut()
    .retain(|member| member.get_rust_obj() != dependent.get_rust_obj());
}

fn mark_aborted(
  scope: &mut v8::PinScope,
  signal: v8::Local<v8::Object>,
  reason: &v8::TracedReference<v8::Value>,
) {
  let Some(host) = (unsafe { host_ref::<AbortSignal>(scope, signal) }) else {
    return;
  };
  if host.get().aborted.get() {
    return;
  }
  host.get().aborted.set(true);
  let Some(reason) = traced_local(scope, reason) else {
    return;
  };
  *host.get().reason.borrow_mut() = Some(traced(scope, reason));
}

fn clear_graph(scope: &mut v8::PinScope, signal: v8::Local<v8::Object>) {
  let Some(host) = (unsafe { host_ref::<AbortSignal>(scope, signal) }) else {
    return;
  };
  host.get().source_signals.borrow_mut().clear();
  host.get().dependent_signals.borrow_mut().clear();
  host.get().observed.borrow_mut().clear();
}

fn dispatch_abort(scope: &mut v8::PinScope, signal: v8::Local<v8::Object>) {
  let event = unsafe {
    yuzuko_core::op::wrap_instance(
      scope,
      Event::new("abort", false, false, false, false),
    )
  };
  let _ = target::dispatch(scope, signal, event);
}

fn signal_reason<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  source: v8::Local<v8::Object>,
) -> Option<v8::Local<'s, v8::Value>> {
  unsafe { host_ref::<AbortSignal>(scope, source) }.and_then(|host| {
    host
      .get()
      .reason
      .borrow()
      .as_ref()
      .and_then(|reference| traced_local(scope, reference))
  })
}

fn undefined(scope: &mut v8::PinScope) -> Js {
  let value: v8::Local<v8::Value> = v8::undefined(scope).into();
  Js(v8::Global::new(scope, value))
}

fn js_or_null(
  scope: &mut v8::PinScope,
  slot: &RefCell<Option<v8::TracedReference<v8::Value>>>,
) -> Js {
  let local = slot
    .borrow()
    .as_ref()
    .and_then(|reference| traced_local(scope, reference));
  match local {
    Some(value) => Js(v8::Global::new(scope, value)),
    None => Js::null(scope),
  }
}
