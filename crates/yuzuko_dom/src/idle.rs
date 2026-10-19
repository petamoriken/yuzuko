//! `requestIdleCallback` on the main global.
//!
//! An idle period is at most 50ms and ends sooner when a timer, host task,
//! or window frame is due. A period does not start when less than 5ms
//! remains, so a short timer keeps the event loop busy. A positive `timeout`
//! still runs the callback, with `didTimeout` set.

use std::cell::{Cell, RefCell};
use std::time::{Duration, Instant};

use yuzuko_core::op::OpError;
use yuzuko_core::{
  is_dedicated_worker, next_host_delay, note_test_error, performance_now_ms,
  rust_string,
};
use yuzuko_macros::op;

use crate::report;

const MAX_IDLE_MS: f64 = 50.0;
const MIN_IDLE_MS: f64 = 5.0;

struct IdleRequest {
  id: u32,
  callback: v8::Global<v8::Function>,
  /// Absolute `performance.now()` millis. Absent when `timeout` was omitted
  /// or not positive.
  timeout_ms: Option<f64>,
}

struct Period {
  deadline_ms: f64,
  ends: Instant,
}

thread_local! {
  static NEXT_ID: Cell<u32> = const { Cell::new(1) };
  static PENDING: RefCell<Vec<IdleRequest>> = const { RefCell::new(Vec::new()) };
  static RUNNABLE: RefCell<Vec<IdleRequest>> = const { RefCell::new(Vec::new()) };
  static PERIOD: RefCell<Option<Period>> = const { RefCell::new(None) };
  static NOT_BEFORE: Cell<Option<Instant>> = const { Cell::new(None) };
}

pub(crate) fn install(scope: &mut v8::PinScope) {
  if is_dedicated_worker() {
    return;
  }
  IdleDeadline::install(scope);
  define(scope, "requestIdleCallback", request_idle_callback, 1);
  define(scope, "cancelIdleCallback", cancel_idle_callback, 1);
}

pub(crate) fn clear() {
  PENDING.with(|slot| slot.borrow_mut().clear());
  RUNNABLE.with(|slot| slot.borrow_mut().clear());
  PERIOD.with(|slot| *slot.borrow_mut() = None);
  NOT_BEFORE.with(|slot| slot.set(None));
  NEXT_ID.with(|slot| slot.set(1));
}

pub(crate) fn has_pending() -> bool {
  PENDING.with(|pending| {
    RUNNABLE.with(|runnable| {
      !pending.borrow().is_empty() || !runnable.borrow().is_empty()
    })
  })
}

pub(crate) fn next_wait() -> Option<Duration> {
  let now_ms = performance_now_ms();
  let now = Instant::now();
  let mut wait: Option<Duration> = earliest_timeout(now_ms);
  if has_pending()
    && let Some(at) = NOT_BEFORE.with(Cell::get)
    && now < at
  {
    wait = earlier(wait, Some(at.saturating_duration_since(now)));
  }
  if can_run_now() {
    wait = Some(Duration::ZERO);
  }
  wait
}

pub(crate) fn poll_timeout(scope: &mut v8::PinScope) -> bool {
  let Some(request) = take_expired() else {
    return false;
  };
  invoke(scope, request, performance_now_ms(), true);
  true
}

pub(crate) fn poll_idle(scope: &mut v8::PinScope) -> bool {
  if !can_run_now() {
    return false;
  }
  let now = Instant::now();
  let expired = PERIOD.with(|slot| {
    slot
      .borrow()
      .as_ref()
      .is_some_and(|period| now >= period.ends)
  });
  if expired {
    PERIOD.with(|slot| *slot.borrow_mut() = None);
  }
  let continue_period = PERIOD.with(|slot| {
    slot.borrow().as_ref().is_some_and(|period| {
      now < period.ends && !RUNNABLE.with(|run| run.borrow().is_empty())
    })
  });
  if !continue_period {
    if !gate_open() {
      return false;
    }
    PENDING.with(|pending| {
      RUNNABLE.with(|runnable| {
        runnable.borrow_mut().append(&mut pending.borrow_mut());
      });
    });
    if RUNNABLE.with(|slot| slot.borrow().is_empty()) {
      return false;
    }
    let budget = idle_budget_ms();
    let now_ms = performance_now_ms();
    let deadline_ms = now_ms + budget;
    let ends = Instant::now() + duration_ms(budget);
    NOT_BEFORE.with(|slot| slot.set(Some(ends)));
    PERIOD.with(|slot| {
      *slot.borrow_mut() = Some(Period { deadline_ms, ends });
    });
  }
  let deadline_ms = PERIOD.with(|slot| {
    slot
      .borrow()
      .as_ref()
      .map(|period| period.deadline_ms)
      .unwrap_or_else(|| performance_now_ms() + MAX_IDLE_MS)
  });
  let Some(request) = RUNNABLE.with(|slot| {
    let mut list = slot.borrow_mut();
    if list.is_empty() {
      None
    } else {
      Some(list.remove(0))
    }
  }) else {
    return false;
  };
  invoke(scope, request, deadline_ms, false);
  true
}

fn can_run_now() -> bool {
  if !has_pending() {
    return false;
  }
  let budget = idle_budget_ms();
  if budget < MIN_IDLE_MS {
    return false;
  }
  let now = Instant::now();
  let open = PERIOD.with(|slot| {
    slot
      .borrow()
      .as_ref()
      .is_some_and(|period| now < period.ends)
      && !RUNNABLE.with(|slot| slot.borrow().is_empty())
  });
  open || gate_open()
}

fn gate_open() -> bool {
  match NOT_BEFORE.with(Cell::get) {
    Some(at) => Instant::now() >= at,
    None => true,
  }
}

fn idle_budget_ms() -> f64 {
  match soonest_delay_ms() {
    Some(delay) => delay.clamp(0.0, MAX_IDLE_MS),
    None => MAX_IDLE_MS,
  }
}

fn soonest_delay_ms() -> Option<f64> {
  let mut best: Option<f64> = None;
  for wait in [next_host_delay(), yuzuko_core::idle_frame_delay()] {
    let Some(wait) = wait else {
      continue;
    };
    let ms = wait.as_secs_f64() * 1000.0;
    best = Some(best.map_or(ms, |current| current.min(ms)));
  }
  best
}

fn for_each_request(mut visit: impl FnMut(&IdleRequest)) {
  PENDING.with(|slot| {
    for item in slot.borrow().iter() {
      visit(item);
    }
  });
  RUNNABLE.with(|slot| {
    for item in slot.borrow().iter() {
      visit(item);
    }
  });
}

fn earliest_timeout(now_ms: f64) -> Option<Duration> {
  let mut soonest: Option<f64> = None;
  for_each_request(|item| {
    if let Some(at) = item.timeout_ms {
      soonest = Some(soonest.map_or(at, |current| current.min(at)));
    }
  });
  let at = soonest?;
  Some(duration_ms((at - now_ms).max(0.0)))
}

fn take_expired() -> Option<IdleRequest> {
  let now = performance_now_ms();
  let mut chosen: Option<(f64, u32)> = None;
  for_each_request(|item| {
    let Some(at) = item.timeout_ms else {
      return;
    };
    if at > now {
      return;
    }
    let replace = chosen.is_none_or(|(best_at, best_id)| {
      at < best_at || (at == best_at && item.id < best_id)
    });
    if replace {
      chosen = Some((at, item.id));
    }
  });
  remove_id(chosen?.1)
}

fn remove_id(id: u32) -> Option<IdleRequest> {
  let pull = |list: &RefCell<Vec<IdleRequest>>| {
    let mut list = list.borrow_mut();
    let index = list.iter().position(|item| item.id == id)?;
    Some(list.remove(index))
  };
  PENDING.with(pull).or_else(|| RUNNABLE.with(pull))
}

fn invoke(
  scope: &mut v8::PinScope,
  request: IdleRequest,
  deadline_ms: f64,
  timed_out: bool,
) {
  v8::tc_scope!(let scope, scope);
  let function = v8::Local::new(scope, request.callback);
  let _ = IdleDeadline::template(scope);
  let deadline = unsafe {
    yuzuko_core::op::wrap_instance(
      scope,
      IdleDeadline {
        timed_out: Cell::new(timed_out),
        deadline_ms,
      },
    )
  };
  let global = yuzuko_core::global(scope);
  let receiver: v8::Local<v8::Value> = global.into();
  let arg: v8::Local<v8::Value> = deadline.into();
  if function.call(scope, receiver, &[arg]).is_some() {
    return;
  }
  let Some(thrown) = scope.exception() else {
    return;
  };
  let thrown = v8::Global::new(scope, thrown);
  scope.reset();
  let thrown = v8::Local::new(scope, thrown);
  if !note_test_error(rust_string(scope, thrown)) {
    report::report_error(scope, thrown);
  }
}

/// Window operations reject a `this` value that is not the realm global.
/// V8 presents `null` and `undefined` as that global.
fn realm_global(scope: &mut v8::PinScope, this: v8::Local<v8::Object>) -> bool {
  let global = yuzuko_core::global(scope);
  if this.strict_equals(global.into()) {
    return true;
  }
  yuzuko_core::throw_type(scope, "Illegal invocation");
  false
}

fn define(
  scope: &mut v8::PinScope,
  name: &str,
  callback: impl v8::MapFnTo<v8::FunctionCallback>,
  length: i32,
) {
  let function = v8::Function::builder(callback)
    .length(length)
    .constructor_behavior(v8::ConstructorBehavior::Throw)
    .build(scope)
    .unwrap_or_else(|| panic!("create function {name}"));
  let key = v8::String::new(scope, name).unwrap();
  function.set_name(key);
  yuzuko_core::global(scope).set(scope, key.into(), function.into());
}

fn request_idle_callback(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut ret: v8::ReturnValue,
) {
  if !realm_global(scope, args.this()) {
    return;
  }
  if args.length() < 1 {
    yuzuko_core::throw_type(scope, "requestIdleCallback requires a callback");
    return;
  }
  let Ok(callback) = v8::Local::<v8::Function>::try_from(args.get(0)) else {
    yuzuko_core::throw_type(scope, "requestIdleCallback requires a callback");
    return;
  };
  let timeout_ms = if args.length() < 2 {
    None
  } else {
    match read_timeout(scope, args.get(1)) {
      Some(timeout) => timeout,
      None => return,
    }
  };
  let timeout_ms = timeout_ms.map(|delay| performance_now_ms() + delay);
  let id = NEXT_ID.with(|next| {
    let id = next.get().max(1);
    let wrapped = id.wrapping_add(1);
    next.set(if wrapped == 0 { 1 } else { wrapped });
    id
  });
  PENDING.with(|slot| {
    slot.borrow_mut().push(IdleRequest {
      id,
      callback: v8::Global::new(scope, callback),
      timeout_ms,
    });
  });
  ret.set(v8::Number::new(scope, f64::from(id)).into());
}

fn cancel_idle_callback(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut _ret: v8::ReturnValue,
) {
  if !realm_global(scope, args.this()) {
    return;
  }
  if args.length() < 1 {
    yuzuko_core::throw_type(scope, "cancelIdleCallback requires a handle");
    return;
  }
  let Some(id) = unsigned_long(scope, args.get(0)) else {
    return;
  };
  let _ = remove_id(id);
}

/// `Some(None)` is an empty dictionary. `None` means the conversion threw.
fn read_timeout(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Option<Option<f64>> {
  if value.is_null() || value.is_undefined() {
    return Some(None);
  }
  if !value.is_object() {
    yuzuko_core::throw_type(
      scope,
      "requestIdleCallback options must be an object",
    );
    return None;
  }
  let object = value.to_object(scope)?;
  let key = v8::String::new(scope, "timeout").unwrap();
  let property = object.get(scope, key.into())?;
  if property.is_undefined() {
    return Some(None);
  }
  let delay = unsigned_long(scope, property)?;
  if delay == 0 {
    Some(None)
  } else {
    Some(Some(f64::from(delay)))
  }
}

fn unsigned_long(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Option<u32> {
  let number = value.number_value(scope)?;
  if !number.is_finite() {
    yuzuko_core::throw_type(scope, "The timeout is not a finite number");
    return None;
  }
  let wrapped = number.trunc().rem_euclid(4_294_967_296.0);
  Some(wrapped as u32)
}

fn earlier(
  left: Option<Duration>,
  right: Option<Duration>,
) -> Option<Duration> {
  match (left, right) {
    (Some(left), Some(right)) => Some(left.min(right)),
    (Some(left), None) => Some(left),
    (None, Some(right)) => Some(right),
    (None, None) => None,
  }
}

fn duration_ms(ms: f64) -> Duration {
  Duration::from_secs_f64((ms / 1000.0).max(0.0))
}

#[repr(C)]
pub struct IdleDeadline {
  timed_out: Cell<bool>,
  deadline_ms: f64,
}

unsafe impl v8::cppgc::GarbageCollected for IdleDeadline {
  fn trace(&self, _visitor: &mut v8::cppgc::Visitor) {}

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"IdleDeadline"
  }
}

#[op]
impl IdleDeadline {
  #[constructor]
  fn new() -> Result<Self, OpError> {
    Err(OpError::type_("Illegal constructor"))
  }

  fn time_remaining(&self) -> f64 {
    if self.timed_out.get() {
      return 0.0;
    }
    let now = performance_now_ms();
    let mut deadline = self.deadline_ms;
    if let Some(delay) = soonest_delay_ms() {
      deadline = deadline.min(now + delay);
    }
    (deadline - now).max(0.0)
  }

  #[getter]
  fn did_timeout(&self) -> bool {
    self.timed_out.get()
  }
}
