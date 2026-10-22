//! Per-surface queue and drawing size. OS callbacks only touch this module.

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

/// 60 Hz. Headless windows and an unknown monitor rate use this period.
pub const FRAME: Duration = Duration::from_nanos(16_666_667);

const MIN_REFRESH_MHZ: u32 = 10_000;
const MAX_REFRESH_MHZ: u32 = 1_000_000;

/// One frame at `rate` millihertz. Unknown and out-of-range rates are 60 Hz.
pub(crate) fn interval_from_millihertz(rate: Option<u32>) -> Duration {
  let Some(rate) = rate else {
    return FRAME;
  };
  // Keep the documented 60 Hz period identical to [`FRAME`].
  if rate == 60_000 {
    return FRAME;
  }
  if !(MIN_REFRESH_MHZ..=MAX_REFRESH_MHZ).contains(&rate) {
    return FRAME;
  }
  // nanoseconds = 1e12 / millihertz.
  Duration::from_nanos(1_000_000_000_000 / u64::from(rate))
}

#[derive(Clone, Copy)]
pub struct Size {
  pub css_width: f64,
  pub css_height: f64,
  pub scale: f64,
  pub bitmap_width: f64,
  pub bitmap_height: f64,
}

impl Size {
  pub fn from_css(css_width: f64, css_height: f64, scale: f64) -> Self {
    let scale = if scale.is_finite() && scale > 0.0 {
      scale
    } else {
      1.0
    };
    Self {
      css_width,
      css_height,
      scale,
      bitmap_width: bitmap_px(css_width, scale),
      bitmap_height: bitmap_px(css_height, scale),
    }
  }
}

pub fn bitmap_px(css: f64, scale: f64) -> f64 {
  let value = (css * scale).round();
  if value.is_finite() && value > 0.0 {
    value
  } else {
    0.0
  }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PointerKind {
  Move,
  Down,
  Up,
  Cancel,
}

#[derive(Clone)]
pub struct RawPointer {
  pub kind: PointerKind,
  pub pointer_id: i32,
  pub pointer_type: String,
  pub is_primary: bool,
  pub x: f64,
  pub y: f64,
  pub screen_x: f64,
  pub screen_y: f64,
  pub button: i32,
  pub pressure: Option<f64>,
  pub movement_x: Option<f64>,
  pub movement_y: Option<f64>,
  pub ctrl: bool,
  pub shift: bool,
  pub alt: bool,
  pub meta: bool,
  pub tilt_x: i32,
  pub tilt_y: i32,
  pub twist: i32,
  pub width: f64,
  pub height: f64,
  pub tangential: f64,
}

#[derive(Clone)]
pub struct RawKey {
  pub down: bool,
  pub key: String,
  pub code: String,
  pub location: u32,
  pub repeat: bool,
  pub text: Option<String>,
  pub ctrl: bool,
  pub shift: bool,
  pub alt: bool,
  pub meta: bool,
}

#[derive(Clone)]
pub enum RawIme {
  Enabled,
  Preedit(String),
  Commit(String),
  Disabled,
}

#[derive(Clone)]
pub struct RawWheel {
  pub x: f64,
  pub y: f64,
  pub screen_x: f64,
  pub screen_y: f64,
  pub delta_x: f64,
  pub delta_y: f64,
  pub delta_z: f64,
  pub delta_mode: u32,
  pub ctrl: bool,
  pub shift: bool,
  pub alt: bool,
  pub meta: bool,
}

#[derive(Clone)]
pub enum Raw {
  Pointer(RawPointer),
  Key(RawKey),
  Ime(RawIme),
  Wheel(RawWheel),
  Focus,
  Blur,
  Resize,
  Media,
  /// `true` when the window is at least partly on screen.
  Visibility(bool),
  Close,
  Menu(String),
}

struct Pending {
  size: Size,
}

struct Slot {
  object: v8::Global<v8::Object>,
  size: Size,
  queue: VecDeque<Raw>,
  frame_at: Option<Instant>,
  frame_interval: Duration,
  ime: bool,
}

thread_local! {
  static PENDING: RefCell<HashMap<u64, Pending>> =
    RefCell::new(HashMap::new());
  static SLOTS: RefCell<HashMap<u64, Slot>> = RefCell::new(HashMap::new());
}

/// Monitor period of each open window, readable from a worker thread.
///
/// `SLOTS` stays on the main thread. `bind` publishes 60 Hz,
/// `set_frame_interval` replaces it, and `forget` removes the window.
static INTERVALS: LazyLock<Mutex<HashMap<u64, Duration>>> =
  LazyLock::new(|| Mutex::new(HashMap::new()));

pub(crate) fn publish_frame_interval(id: u64, interval: Duration) {
  INTERVALS
    .lock()
    .expect("frame intervals")
    .insert(id, interval);
}

pub(crate) fn retract_frame_interval(id: u64) {
  INTERVALS.lock().expect("frame intervals").remove(&id);
}

pub(crate) fn published_frame_interval(id: u64) -> Option<Duration> {
  INTERVALS.lock().expect("frame intervals").get(&id).copied()
}

pub fn prepare(id: u64, size: Size) {
  PENDING.with(|pending| {
    pending.borrow_mut().insert(id, Pending { size });
  });
}

pub fn bind(scope: &mut v8::PinScope, id: u64, object: v8::Local<v8::Object>) {
  let pending = PENDING.with(|pending| pending.borrow_mut().remove(&id));
  let Some(pending) = pending else {
    return;
  };
  let object = v8::Global::new(scope, object);
  SLOTS.with(|slots| {
    slots.borrow_mut().insert(
      id,
      Slot {
        object,
        size: pending.size,
        queue: VecDeque::new(),
        frame_at: None,
        frame_interval: FRAME,
        ime: false,
      },
    );
  });
  publish_frame_interval(id, FRAME);
}

pub fn forget(id: u64) -> Option<Size> {
  retract_frame_interval(id);
  PENDING.with(|pending| pending.borrow_mut().remove(&id));
  SLOTS.with(|slots| slots.borrow_mut().remove(&id).map(|slot| slot.size))
}

pub fn clear() {
  INTERVALS.lock().expect("frame intervals").clear();
  PENDING.with(|pending| pending.borrow_mut().clear());
  SLOTS.with(|slots| slots.borrow_mut().clear());
}

pub fn contains(id: u64) -> bool {
  SLOTS.with(|slots| slots.borrow().contains_key(&id))
}

pub fn ids() -> Vec<u64> {
  SLOTS.with(|slots| slots.borrow().keys().copied().collect())
}

pub fn object<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  id: u64,
) -> Option<v8::Local<'s, v8::Object>> {
  let global = SLOTS
    .with(|slots| slots.borrow().get(&id).map(|slot| slot.object.clone()))?;
  Some(v8::Local::new(scope, global))
}

pub fn size(id: u64) -> Option<Size> {
  SLOTS.with(|slots| slots.borrow().get(&id).map(|slot| slot.size))
}

pub fn set_size(id: u64, size: Size) -> bool {
  SLOTS.with(|slots| {
    let mut slots = slots.borrow_mut();
    let Some(slot) = slots.get_mut(&id) else {
      return false;
    };
    let changed = slot.size.css_width != size.css_width
      || slot.size.css_height != size.css_height
      || slot.size.scale != size.scale
      || slot.size.bitmap_width != size.bitmap_width
      || slot.size.bitmap_height != size.bitmap_height;
    slot.size = size;
    changed
  })
}

pub fn set_ime(id: u64, allowed: bool) {
  SLOTS.with(|slots| {
    if let Some(slot) = slots.borrow_mut().get_mut(&id) {
      slot.ime = allowed;
    }
  });
}

pub fn ime_allowed(id: u64) -> bool {
  SLOTS.with(|slots| slots.borrow().get(&id).is_some_and(|slot| slot.ime))
}

pub fn push(id: u64, event: Raw) {
  SLOTS.with(|slots| {
    if let Some(slot) = slots.borrow_mut().get_mut(&id) {
      slot.queue.push_back(event);
    }
  });
}

pub fn pop(id: u64) -> Option<Raw> {
  SLOTS.with(|slots| {
    slots
      .borrow_mut()
      .get_mut(&id)
      .and_then(|slot| slot.queue.pop_front())
  })
}

/// Removes queued moves for `pointer_id` that follow the event just popped.
pub fn pop_coalesced_moves(id: u64, pointer_id: i32) -> Vec<RawPointer> {
  SLOTS.with(|slots| {
    let mut slots = slots.borrow_mut();
    let Some(slot) = slots.get_mut(&id) else {
      return Vec::new();
    };
    let mut moves = Vec::new();
    while matches!(
      slot.queue.front(),
      Some(Raw::Pointer(pointer))
        if pointer.kind == PointerKind::Move
          && pointer.pointer_id == pointer_id
    ) {
      if let Some(Raw::Pointer(pointer)) = slot.queue.pop_front() {
        moves.push(pointer);
      }
    }
    moves
  })
}

pub fn has_input(id: u64) -> bool {
  SLOTS.with(|slots| {
    slots
      .borrow()
      .get(&id)
      .is_some_and(|slot| !slot.queue.is_empty())
  })
}

pub fn arm_frame(id: u64) {
  SLOTS.with(|slots| {
    if let Some(slot) = slots.borrow_mut().get_mut(&id)
      && slot.frame_at.is_none()
    {
      let interval = slot.frame_interval;
      slot.frame_at = Some(Instant::now() + interval);
    }
  });
}

/// Stores the monitor period used the next time a frame is armed.
///
/// An already armed deadline is left in place. Returns whether the period
/// changed.
pub fn set_frame_interval(id: u64, interval: Duration) -> bool {
  let changed = SLOTS.with(|slots| {
    let mut slots = slots.borrow_mut();
    let Some(slot) = slots.get_mut(&id) else {
      return false;
    };
    if slot.frame_interval == interval {
      return false;
    }
    slot.frame_interval = interval;
    true
  });
  if changed {
    publish_frame_interval(id, interval);
  }
  changed
}

pub fn clear_frame(id: u64) {
  SLOTS.with(|slots| {
    if let Some(slot) = slots.borrow_mut().get_mut(&id) {
      slot.frame_at = None;
    }
  });
}

/// Whether a callback is waiting for this window's next frame. Only the
/// macOS display link asks.
#[cfg(target_os = "macos")]
pub fn frame_armed(id: u64) -> bool {
  SLOTS.with(|slots| {
    slots
      .borrow()
      .get(&id)
      .is_some_and(|slot| slot.frame_at.is_some())
  })
}

pub fn frame_due(id: u64, now: Instant) -> bool {
  SLOTS.with(|slots| {
    slots
      .borrow()
      .get(&id)
      .is_some_and(|slot| slot.frame_at.is_some_and(|at| at <= now))
  })
}

pub fn take_due_frame(id: u64, now: Instant) -> bool {
  SLOTS.with(|slots| {
    let mut slots = slots.borrow_mut();
    let Some(slot) = slots.get_mut(&id) else {
      return false;
    };
    let due = slot.frame_at.is_some_and(|at| at <= now);
    if due {
      slot.frame_at = None;
    }
    due
  })
}

pub fn next_frame_wait() -> Option<Duration> {
  SLOTS.with(|slots| {
    let slots = slots.borrow();
    let mut wait: Option<Duration> = None;
    let now = Instant::now();
    for slot in slots.values() {
      if !slot.queue.is_empty() {
        return Some(Duration::ZERO);
      }
      if let Some(at) = slot.frame_at {
        let remaining = at.saturating_duration_since(now);
        wait = Some(match wait {
          Some(current) => current.min(remaining),
          None => remaining,
        });
      }
    }
    wait
  })
}

pub fn any_open() -> bool {
  SLOTS.with(|slots| !slots.borrow().is_empty())
}

#[cfg(test)]
mod tests {
  use super::{FRAME, interval_from_millihertz};
  use std::time::Duration;

  #[test]
  fn unknown_refresh_is_60_hz() {
    assert_eq!(interval_from_millihertz(None), FRAME);
    assert_eq!(interval_from_millihertz(Some(0)), FRAME);
    assert_eq!(interval_from_millihertz(Some(9_999)), FRAME);
    assert_eq!(interval_from_millihertz(Some(1_000_001)), FRAME);
  }

  #[test]
  fn refresh_period_matches_millihertz() {
    assert_eq!(interval_from_millihertz(Some(60_000)), FRAME);
    assert_eq!(
      interval_from_millihertz(Some(120_000)),
      Duration::from_nanos(8_333_333)
    );
    assert_eq!(
      interval_from_millihertz(Some(144_000)),
      Duration::from_nanos(6_944_444)
    );
    assert_eq!(
      interval_from_millihertz(Some(165_000)),
      Duration::from_nanos(6_060_606)
    );
    assert_eq!(
      interval_from_millihertz(Some(240_000)),
      Duration::from_nanos(4_166_666)
    );
    assert_eq!(
      interval_from_millihertz(Some(10_000)),
      Duration::from_millis(100)
    );
    assert_eq!(
      interval_from_millihertz(Some(1_000_000)),
      Duration::from_millis(1)
    );
  }
}
