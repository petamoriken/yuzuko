//! `setTimeout` and `setInterval` on the host task queue.

use std::time::{Duration, Instant};

use crate::host::{self, Timer, TimerCallback};
use crate::js;
use crate::loader::modules;

const MAX_DELAY_MS: f64 = 2_147_483_647.0;
const NESTED_CLAMP_MS: f64 = 4.0;
const NESTED_CLAMP_AFTER: u32 = 5;

pub fn install(scope: &mut v8::PinScope) {
  let global = js::global(scope);
  js::set_function(scope, global, "setTimeout", set_timeout);
  js::set_function(scope, global, "clearTimeout", clear_timer);
  js::set_function(scope, global, "setInterval", set_interval);
  js::set_function(scope, global, "clearInterval", clear_timer);
}

pub fn count() -> usize {
  host::with(|state| state.timers.len())
}

pub fn has_pending() -> bool {
  count() > 0
}

pub fn next_wait() -> Option<Duration> {
  host::with(|state| {
    state
      .timers
      .iter()
      .map(|timer| timer.when.saturating_duration_since(Instant::now()))
      .min()
  })
}

pub fn poll_due() -> Option<Timer> {
  take_due(|_| true)
}

/// Ids of timers whose deadline has already passed.
pub fn due_ids() -> Vec<i32> {
  host::with(|state| {
    let now = Instant::now();
    state
      .timers
      .iter()
      .filter(|timer| timer.when <= now)
      .map(|timer| timer.id)
      .collect()
  })
}

/// Removes one due timer whose id is in `allow`.
pub fn poll_due_in(allow: &[i32]) -> Option<Timer> {
  take_due(|id| allow.contains(&id))
}

fn take_due(allow: impl Fn(i32) -> bool) -> Option<Timer> {
  host::with_mut(|state| {
    let now = Instant::now();
    let index = state
      .timers
      .iter()
      .enumerate()
      .filter(|(_, timer)| timer.when <= now && allow(timer.id))
      .min_by_key(|(_, timer)| timer.when)
      .map(|(index, _)| index)?;
    Some(state.timers.remove(index))
  })
}

pub fn fire(scope: &mut v8::PinScope, mut timer: Timer) {
  host::with_mut(|state| state.timer_nesting += 1);
  {
    v8::tc_scope!(let tc, scope);
    let failed = match &timer.callback {
      TimerCallback::FlushRejections => {
        crate::runtime::flush_rejection_notifications(tc);
        false
      }
      TimerCallback::Function(function) => {
        let function = v8::Local::new(tc, function);
        let receiver = js::global(tc);
        let args: Vec<v8::Local<v8::Value>> = timer
          .args
          .iter()
          .map(|arg| v8::Local::new(tc, arg))
          .collect();
        function.call(tc, receiver.into(), &args).is_none()
      }
      TimerCallback::Code(code) => match v8::String::new(tc, code) {
        Some(source) => v8::Script::compile(tc, source, None)
          .and_then(|script| script.run(tc))
          .is_none(),
        None => true,
      },
    };
    if failed {
      let thrown = tc.exception().map(|value| v8::Global::new(tc, value));
      tc.reset();
      if host::with(|state| state.in_test) {
        let text = thrown.as_ref().map(|value| {
          let value = v8::Local::new(tc, value);
          modules::exception_stack(tc, value)
            .unwrap_or_else(|| "timer callback threw".to_string())
        });
        host::with_mut(|state| {
          state.test_error =
            Some(text.unwrap_or_else(|| "timer callback threw".to_string()));
        });
      } else if let Some(thrown) = thrown {
        let thrown = v8::Local::new(tc, thrown);
        report_uncaught(tc, thrown);
      }
    }
  }
  let nesting = host::with(|state| state.timer_nesting);
  host::with_mut(|state| {
    state.timer_nesting = state.timer_nesting.saturating_sub(1);
    let current = state.timer_generation.get(&timer.id).copied().unwrap_or(0);
    if let Some(repeat_ms) = timer.repeat_ms
      && current == timer.generation
    {
      let delay = clamp_delay(repeat_ms, nesting);
      timer.when = Instant::now() + duration_from_ms(delay);
      state.timers.push(timer);
    }
  });
}

fn report_uncaught(scope: &mut v8::PinScope, error: v8::Local<v8::Value>) {
  let global = js::global(scope);
  let key = v8::String::new(scope, "reportError").unwrap();
  let Some(report) = global
    .get(scope, key.into())
    .and_then(|value| v8::Local::<v8::Function>::try_from(value).ok())
  else {
    return;
  };
  let _ = report.call(scope, global.into(), &[error]);
}

fn set_timeout(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut ret: v8::ReturnValue,
) {
  schedule(scope, &args, false, &mut ret);
}

fn set_interval(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut ret: v8::ReturnValue,
) {
  schedule(scope, &args, true, &mut ret);
}

fn clear_timer(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut _ret: v8::ReturnValue,
) {
  let Some(id) = args.get(0).int32_value(scope) else {
    return;
  };
  host::with_mut(|state| {
    state.timers.retain(|timer| timer.id != id);
    *state.timer_generation.entry(id).or_insert(0) += 1;
  });
}

/// Queue the HTML unhandled-rejection notification ahead of user timers.
pub fn schedule_rejection_flush() {
  host::with_mut(|state| {
    if state.rejection_notify_queued {
      return;
    }
    state.rejection_notify_queued = true;
    state.next_timer_id += 1;
    let id = state.next_timer_id;
    let generation = state.timer_generation.get(&id).copied().unwrap_or(0);
    state.timers.push(Timer {
      id,
      generation,
      when: Instant::now(),
      repeat_ms: None,
      callback: TimerCallback::FlushRejections,
      args: Vec::new(),
    });
  });
}

pub fn schedule_callback(
  scope: &mut v8::PinScope,
  function: v8::Local<v8::Function>,
  delay_ms: f64,
  extra: &[v8::Local<v8::Value>],
) {
  let requested = if !delay_ms.is_finite() || delay_ms < 0.0 {
    0.0
  } else {
    delay_ms.min(MAX_DELAY_MS)
  };
  let nesting = host::with(|state| state.timer_nesting);
  let delay = clamp_delay(requested, nesting);
  let args = extra
    .iter()
    .copied()
    .map(|value| v8::Global::new(scope, value))
    .collect();
  host::with_mut(|state| {
    state.next_timer_id += 1;
    let id = state.next_timer_id;
    let generation = state.timer_generation.get(&id).copied().unwrap_or(0);
    state.timers.push(Timer {
      id,
      generation,
      when: Instant::now() + duration_from_ms(delay),
      repeat_ms: None,
      callback: TimerCallback::Function(v8::Global::new(scope, function)),
      args,
    });
  });
}

fn schedule(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
  repeat: bool,
  ret: &mut v8::ReturnValue,
) {
  if args.length() < 1 {
    js::throw_type(scope, "setTimeout requires a callback");
    return;
  }
  let callback = match timer_callback(scope, args.get(0)) {
    Some(callback) => callback,
    None => return,
  };
  let requested = delay_ms(scope, args.get(1));
  let nesting = host::with(|state| state.timer_nesting);
  let delay = clamp_delay(requested, nesting);
  let extra = (2..args.length())
    .map(|index| v8::Global::new(scope, args.get(index)))
    .collect();
  let id = host::with_mut(|state| {
    state.next_timer_id += 1;
    let id = state.next_timer_id;
    let generation = state.timer_generation.get(&id).copied().unwrap_or(0);
    state.timers.push(Timer {
      id,
      generation,
      when: Instant::now() + duration_from_ms(delay),
      repeat_ms: repeat.then_some(requested),
      callback,
      args: extra,
    });
    id
  });
  ret.set(v8::Integer::new(scope, id).into());
}

fn timer_callback(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Option<TimerCallback> {
  if let Ok(function) = v8::Local::<v8::Function>::try_from(value) {
    return Some(TimerCallback::Function(v8::Global::new(scope, function)));
  }
  // The string overload runs ToString, including a handler object's toString.
  let text = value.to_string(scope)?;
  Some(TimerCallback::Code(text.to_rust_string_lossy(scope)))
}

fn delay_ms(scope: &mut v8::PinScope, value: v8::Local<v8::Value>) -> f64 {
  if value.is_undefined() {
    return 0.0;
  }
  let Some(number) = value.number_value(scope) else {
    return 0.0;
  };
  // Web IDL `long`, then HTML clamps a negative timeout to 0.
  let timeout = to_int32(number);
  if timeout < 0 { 0.0 } else { f64::from(timeout) }
}

fn to_int32(number: f64) -> i32 {
  if !number.is_finite() || number == 0.0 {
    return 0;
  }
  let int = number.trunc();
  let mut bits = int % 4_294_967_296.0;
  if bits < 0.0 {
    bits += 4_294_967_296.0;
  }
  if bits >= 2_147_483_648.0 {
    (bits - 4_294_967_296.0) as i32
  } else {
    bits as i32
  }
}

fn clamp_delay(delay: f64, nesting: u32) -> f64 {
  if nesting >= NESTED_CLAMP_AFTER && delay < NESTED_CLAMP_MS {
    NESTED_CLAMP_MS
  } else {
    delay
  }
}

fn duration_from_ms(delay: f64) -> Duration {
  Duration::from_secs_f64((delay / 1000.0).max(0.0))
}
