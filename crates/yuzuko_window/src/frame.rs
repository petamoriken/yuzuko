//! Per-surface animation frames on the runtime event loop.

use std::time::Duration;

use yuzuko_core::LoopInterest;

use crate::inbox;
use crate::input;
use crate::input::gamepad;
use crate::os;
use crate::surface;

pub fn drive(scope: &mut v8::PinScope) -> bool {
  surface::apply_remote_presentation();
  os::pump(Some(Duration::ZERO));
  crate::shell::menu::poll(scope);
  crate::shell::tray::poll(scope);
  crate::shell::dock::poll(scope);
  let mut did = gamepad::poll(scope);
  let ids = inbox::ids();
  let now = std::time::Instant::now();
  for id in ids {
    if inbox::has_input(id) {
      input::flush(scope, id);
      did = true;
    }
    if inbox::frame_due(id, now) {
      surface::Window::run_frame(scope, id);
      did = true;
    }
  }
  did
}

pub fn interest() -> LoopInterest {
  LoopInterest {
    keep_alive: inbox::any_open() || crate::shell::tray::any_open(),
    next_wait: inbox::next_frame_wait(),
  }
}

pub fn wait(timeout: Option<Duration>) {
  // A macOS pump drains a queued wake and then still blocks until `timeout`.
  // Honor a poke that already arrived before entering that wait.
  if yuzuko_core::poll_task_poke() {
    return;
  }
  let live = yuzuko_core::keeps_alive(yuzuko_core::agent_id());
  let long = timeout.is_none_or(|timeout| timeout > Duration::from_millis(5));
  if live && long {
    // Cap the OS wait while a worker is alive. A poke during `pump` does not
    // make that pump return, so the caller polls and reads the inbox.
    if !os::pump(Some(Duration::from_millis(5))) {
      yuzuko_core::wait_for_tasks(timeout);
    }
    return;
  }
  if !os::pump(timeout) {
    yuzuko_core::wait_for_tasks(timeout);
  }
}
