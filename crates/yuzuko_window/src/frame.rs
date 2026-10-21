//! Per-surface animation frames on the runtime event loop.

use std::time::{Duration, Instant};

use yuzuko_core::LoopInterest;

use crate::inbox;
use crate::input;
use crate::input::gamepad;
use crate::os;
use crate::surface;

/// A pump that waited this long was held until a display refresh.
///
/// The same amount is how early that refresh may land before the deadline.
/// A full frame of slack would run a slower window on a faster display's wake.
const REFRESH_WAIT: Duration = Duration::from_millis(2);

pub fn drive(scope: &mut v8::PinScope) -> bool {
  surface::apply_remote_presentation();
  // Run a frame that is already due before entering the OS loop. After a
  // present, macOS does not return from that loop until the next refresh,
  // so pumping first spends the refresh the callback was waiting for.
  if run_ready(scope, Instant::now()) {
    return true;
  }
  let started = Instant::now();
  os::pump(Some(Duration::ZERO));
  let now = Instant::now();
  // The refresh often lands a fraction of a millisecond before the frame
  // deadline. Treat that wake as the frame, or the next pump waits again.
  let deadline = if started.elapsed() > REFRESH_WAIT {
    now + REFRESH_WAIT
  } else {
    now
  };
  run_ready(scope, deadline)
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
  // The same refresh wait applies to a short frame timeout: a few hundred
  // microseconds still blocks until the next display refresh. Sleep outside
  // NSApplication when the next animation frame is what we are waiting for.
  if defer_os_pump(timeout) {
    yuzuko_core::wait_for_tasks(timeout);
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

/// `true` when a frame ran or input was delivered.
///
/// A `true` result is finished work: the caller must not spend another
/// display refresh before the event loop turns again.
fn run_ready(scope: &mut v8::PinScope, deadline: Instant) -> bool {
  crate::shell::menu::poll(scope);
  crate::shell::tray::poll(scope);
  crate::shell::dock::poll(scope);
  os::begin_present_turn();
  let mut did = gamepad::poll(scope);
  for id in inbox::ids() {
    if inbox::has_input(id) {
      input::flush(scope, id);
      did = true;
    }
    if inbox::frame_due(id, deadline) {
      surface::Window::run_frame(scope, id, deadline);
      did = true;
    }
  }
  // A canvas dirtied by the task, or by a frame that did not present it,
  // is shown once. A present that already ran this turn is left alone.
  if surface::present_pending() {
    did = true;
  }
  did
}

fn defer_os_pump(timeout: Option<Duration>) -> bool {
  let Some(timeout) = timeout else {
    return false;
  };
  // Other platforms honor a short pump timeout. macOS turns it into the
  // next refresh after a drawable has been presented, so a wait that is
  // only the rest of this frame sleeps outside NSApplication.
  cfg!(target_os = "macos")
    && inbox::next_frame_wait()
      .is_some_and(|frame| timeout <= frame.saturating_add(REFRESH_WAIT))
}
