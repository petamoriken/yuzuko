//! Transient user activation on the focused `Window`.
//!
//! Clipboard reads and writes require a recent pointer or key press on that
//! focused surface. The gesture stays valid for five seconds.

use std::cell::Cell;
use std::time::Duration;
use std::time::Instant;

use crate::inbox::PointerKind;
use crate::inbox::Raw;
use crate::inbox::RawKey;

const GESTURE: Duration = Duration::from_secs(5);

#[derive(Clone, Copy)]
struct State {
  focused: Option<u64>,
  gesture_window: Option<u64>,
  gesture_at: Option<Instant>,
}

std::thread_local! {
  static STATE: Cell<State> = const {
    Cell::new(State {
      focused: None,
      gesture_window: None,
      gesture_at: None,
    })
  };
}

pub fn observe(id: u64, raw: &Raw) {
  match raw {
    Raw::Focus => focus(id),
    Raw::Blur => blur(id),
    Raw::Pointer(pointer) if pointer.kind == PointerKind::Down => {
      gesture(id);
    }
    Raw::Key(key) if key.down => gesture(id),
    _ => {}
  }
}

pub fn closed(id: u64) {
  blur(id);
}

pub fn focused(id: u64) -> bool {
  STATE.with(|slot| slot.get().focused == Some(id))
}

pub fn focused_id() -> Option<u64> {
  STATE.with(|slot| slot.get().focused)
}

/// A pointer or key press on the focused window, still inside the gesture
/// window.
pub fn allowed() -> bool {
  STATE.with(|slot| {
    let state = slot.get();
    matches!(
      (state.focused, state.gesture_window, state.gesture_at),
      (Some(focused), Some(gesture), Some(at))
        if focused == gesture && at.elapsed() < GESTURE
    )
  })
}

pub fn clipboard_command(key: &RawKey) -> Option<&'static str> {
  if !key.down || key.alt || key.repeat || key.shift {
    return None;
  }
  if !(key.ctrl || key.meta) {
    return None;
  }
  match key.key.to_ascii_lowercase().as_str() {
    "c" => Some("copy"),
    "x" => Some("cut"),
    "v" => Some("paste"),
    _ => None,
  }
}

fn focus(id: u64) {
  STATE.with(|slot| {
    let mut state = slot.get();
    state.focused = Some(id);
    slot.set(state);
  });
  crate::shell::menu::sync_focus(id);
}

fn blur(id: u64) {
  STATE.with(|slot| {
    let mut state = slot.get();
    if state.focused == Some(id) {
      state.focused = None;
    }
    slot.set(state);
  });
}

fn gesture(id: u64) {
  STATE.with(|slot| {
    let mut state = slot.get();
    if state.focused == Some(id) {
      state.gesture_window = Some(id);
      state.gesture_at = Some(Instant::now());
    }
    slot.set(state);
  });
}
