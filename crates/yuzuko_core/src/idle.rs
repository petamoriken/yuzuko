//! Hooks the main event loop uses to run `requestIdleCallback`.
//!
//! The lists and `IdleDeadline` live in `yuzuko_dom`. This module only stores
//! the function pointers so the core loop does not depend on that crate.

use std::sync::Mutex;
use std::time::Duration;

use crate::host;

pub struct IdleHooks {
  pub has_pending: fn() -> bool,
  pub next_wait: fn() -> Option<Duration>,
  pub poll_timeout: fn(&mut v8::PinScope) -> bool,
  pub poll_idle: fn(&mut v8::PinScope) -> bool,
}

type FrameDelay = fn() -> Option<Duration>;

static HOOKS: Mutex<Option<IdleHooks>> = Mutex::new(None);
static FRAME_DELAY: Mutex<Option<FrameDelay>> = Mutex::new(None);

pub fn set_idle_hooks(hooks: IdleHooks) {
  *HOOKS.lock().expect("idle hooks") = Some(hooks);
}

/// Delay until the next window animation frame, when a window has one queued.
pub fn set_idle_frame_delay(hook: FrameDelay) {
  *FRAME_DELAY.lock().expect("idle frame delay") = Some(hook);
}

pub fn idle_frame_delay() -> Option<Duration> {
  FRAME_DELAY
    .lock()
    .expect("idle frame delay")
    .and_then(|hook| hook())
}

pub fn idle_has_pending() -> bool {
  HOOKS
    .lock()
    .expect("idle hooks")
    .as_ref()
    .is_some_and(|hooks| (hooks.has_pending)())
}

pub fn idle_next_wait() -> Option<Duration> {
  HOOKS
    .lock()
    .expect("idle hooks")
    .as_ref()
    .and_then(|hooks| (hooks.next_wait)())
}

pub fn poll_idle_timeout(scope: &mut v8::PinScope) -> bool {
  let hook = HOOKS
    .lock()
    .expect("idle hooks")
    .as_ref()
    .map(|hooks| hooks.poll_timeout);
  hook.is_some_and(|hook| hook(scope))
}

pub fn poll_idle_period(scope: &mut v8::PinScope) -> bool {
  let hook = HOOKS
    .lock()
    .expect("idle hooks")
    .as_ref()
    .map(|hooks| hooks.poll_idle);
  hook.is_some_and(|hook| hook(scope))
}

/// Soonest host timer or queued task. Idle deadlines shrink to this.
pub fn next_host_delay() -> Option<Duration> {
  let timer = crate::timers::next_wait();
  let task = host::queue().next_wait();
  match (timer, task) {
    (Some(timer), Some(task)) => Some(timer.min(task)),
    (Some(timer), None) => Some(timer),
    (None, Some(task)) => Some(task),
    (None, None) => None,
  }
}
