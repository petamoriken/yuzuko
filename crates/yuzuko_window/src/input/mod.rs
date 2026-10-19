//! Turns queued OS and test input into DOM events on one surface.

pub(crate) mod gamepad;
pub(crate) mod keys;

use std::cell::RefCell;
use std::time::Instant;

use yuzuko_core::op::host_ref;
use yuzuko_dom::{
  FiredComposition, FiredInput, FiredKey, FiredPointer, FiredWheel,
  dispatch_composition, dispatch_focus, dispatch_input, dispatch_key,
  dispatch_simple, dispatch_wheel,
};

use crate::activation;
use crate::inbox::{
  self, PointerKind, Raw, RawIme, RawKey, RawPointer, RawWheel,
};
use crate::shell::clipboard;
use crate::surface::Window;

const CLICK_MS: u128 = 500;
const CLICK_SLOP: f64 = 4.0;

struct Tracker {
  buttons: u16,
  last_x: f64,
  last_y: f64,
  has_last: bool,
  click_count: u32,
  last_click: Option<Instant>,
  last_click_x: f64,
  last_click_y: f64,
  last_click_button: i32,
  active: Vec<i32>,
  captured: Vec<i32>,
  preedit: String,
  ime_open: bool,
}

impl Tracker {
  fn new() -> Self {
    Self {
      buttons: 0,
      last_x: 0.0,
      last_y: 0.0,
      has_last: false,
      click_count: 0,
      last_click: None,
      last_click_x: 0.0,
      last_click_y: 0.0,
      last_click_button: -1,
      active: Vec::new(),
      captured: Vec::new(),
      preedit: String::new(),
      ime_open: false,
    }
  }
}

pub struct PointerState {
  tracker: RefCell<Tracker>,
}

impl PointerState {
  pub fn new() -> Self {
    Self {
      tracker: RefCell::new(Tracker::new()),
    }
  }

  pub fn composing(&self) -> bool {
    !self.tracker.borrow().preedit.is_empty()
  }

  pub fn end_composition(&self) -> Option<String> {
    let mut tracker = self.tracker.borrow_mut();
    if !tracker.ime_open && tracker.preedit.is_empty() {
      return None;
    }
    let data = tracker.preedit.clone();
    tracker.preedit.clear();
    tracker.ime_open = false;
    Some(data)
  }

  pub fn capture(&self, pointer_id: i32) -> Result<bool, ()> {
    let mut tracker = self.tracker.borrow_mut();
    if !tracker.active.contains(&pointer_id) {
      return Err(());
    }
    if tracker.captured.contains(&pointer_id) {
      return Ok(false);
    }
    tracker.captured.push(pointer_id);
    Ok(true)
  }

  pub fn release_capture(&self, pointer_id: i32) -> Result<(), ()> {
    let mut tracker = self.tracker.borrow_mut();
    let Some(index) = tracker.captured.iter().position(|id| *id == pointer_id)
    else {
      return Err(());
    };
    tracker.captured.remove(index);
    Ok(())
  }

  pub fn has_capture(&self, pointer_id: i32) -> bool {
    self.tracker.borrow().captured.contains(&pointer_id)
  }
}

enum Step {
  Pointer(FiredPointer),
  Key(FiredKey, Option<String>),
  Composition(FiredComposition),
  Input(FiredInput),
  Wheel(FiredWheel),
  Simple(&'static str),
  Clipboard(&'static str),
  Menu(String),
  Media,
  Visibility(bool),
  Close,
}

pub fn flush(scope: &mut v8::PinScope, id: u64) {
  loop {
    if !inbox::contains(id) {
      return;
    }
    let Some(raw) = inbox::pop(id) else {
      return;
    };
    let Some(object) = inbox::object(scope, id) else {
      return;
    };
    let steps = {
      let Some(host) = (unsafe { host_ref::<Window>(scope, object) }) else {
        return;
      };
      host.get().plan(raw)
    };
    for step in steps {
      if !inbox::contains(id) {
        return;
      }
      let Some(object) = inbox::object(scope, id) else {
        return;
      };
      match step {
        Step::Pointer(event) => {
          crate::surface::dispatch_pointer_on_window(scope, object, &event);
        }
        Step::Key(event, text) => {
          let allowed = dispatch_key(scope, object, &event);
          if allowed
            && event.type_name == "keydown"
            && let Some(text) = text
          {
            dispatch_input(
              scope,
              object,
              &FiredInput {
                input_type: "insertText".to_string(),
                data: text,
                is_composing: false,
              },
            );
          }
        }
        Step::Composition(event) => {
          dispatch_composition(scope, object, &event);
        }
        Step::Input(event) => {
          dispatch_input(scope, object, &event);
        }
        Step::Wheel(event) => {
          dispatch_wheel(scope, object, &event);
        }
        Step::Simple(name) => {
          if name == "focus" || name == "blur" {
            dispatch_focus(scope, object, name);
          } else {
            dispatch_simple(scope, object, name, false, false);
          }
          if name == "resize" {
            crate::media::refresh(scope, id);
          }
        }
        Step::Media => crate::media::refresh(scope, id),
        Step::Visibility(visible) => {
          Window::publish_visibility(scope, object, visible);
        }
        Step::Clipboard(name) => {
          clipboard::dispatch_command(scope, object, name);
        }
        Step::Menu(action) => {
          crate::shell::menu::dispatch_click(scope, &action);
        }
        Step::Close => Window::close_object(scope, object),
      }
    }
    scope.perform_microtask_checkpoint();
  }
}

impl Window {
  fn plan(&self, raw: Raw) -> Vec<Step> {
    activation::observe(self.id(), &raw);
    match raw {
      Raw::Pointer(pointer) => self.plan_pointer(pointer),
      Raw::Key(key) => self.plan_key(key),
      Raw::Ime(ime) => self.plan_ime(ime),
      Raw::Wheel(wheel) => vec![Step::Wheel(wheel_event(wheel))],
      Raw::Focus => vec![Step::Simple("focus")],
      Raw::Blur => vec![Step::Simple("blur")],
      Raw::Resize => {
        if let Some(size) = inbox::size(self.id()) {
          self.apply_size(size);
        }
        vec![Step::Simple("resize")]
      }
      Raw::Media => vec![Step::Media],
      Raw::Visibility(visible) => vec![Step::Visibility(visible)],
      Raw::Close => vec![Step::Close],
      Raw::Menu(action) => vec![Step::Menu(action)],
    }
  }

  fn plan_pointer(&self, first: RawPointer) -> Vec<Step> {
    let mut pointers = vec![first.clone()];
    if first.kind == PointerKind::Move {
      pointers.extend(inbox::pop_coalesced_moves(self.id(), first.pointer_id));
    }
    let last = pointers.last().cloned().unwrap_or(first);
    let mut tracker = self.pointer_state().tracker.borrow_mut();
    let mut steps = Vec::new();
    match last.kind {
      PointerKind::Move => {
        mark_active(&mut tracker, last.pointer_id);
        let mut sum_x = 0.0;
        let mut sum_y = 0.0;
        let mut coalesced = Vec::with_capacity(pointers.len());
        for pointer in &pointers {
          let movement = movement_of(&mut tracker, pointer);
          tracker.has_last = true;
          tracker.last_x = pointer.x;
          tracker.last_y = pointer.y;
          sum_x += movement.0;
          sum_y += movement.1;
          let mut child = pointer_event(
            "pointermove",
            pointer,
            -1,
            tracker.buttons,
            pressure_of(pointer, tracker.buttons),
            movement,
            0,
          );
          child.bubbles = false;
          child.cancelable = false;
          child.composed = false;
          coalesced.push(child);
        }
        let pressure = pressure_of(&last, tracker.buttons);
        let mut event = pointer_event(
          "pointermove",
          &last,
          -1,
          tracker.buttons,
          pressure,
          (sum_x, sum_y),
          0,
        );
        event.coalesced = coalesced;
        steps.push(Step::Pointer(event));
        let mut mouse = pointer_event(
          "mousemove",
          &last,
          0,
          tracker.buttons,
          pressure,
          (sum_x, sum_y),
          0,
        );
        mouse.coalesced.clear();
        steps.push(Step::Pointer(mouse));
      }
      PointerKind::Down => {
        if let Some(bit) = button_bit(last.button) {
          tracker.buttons |= bit;
        }
        tracker.has_last = true;
        tracker.last_x = last.x;
        tracker.last_y = last.y;
        mark_active(&mut tracker, last.pointer_id);
        let pressure = pressure_of(&last, tracker.buttons);
        steps.push(Step::Pointer(pointer_event(
          "pointerdown",
          &last,
          last.button,
          tracker.buttons,
          pressure,
          (0.0, 0.0),
          0,
        )));
        steps.push(Step::Pointer(pointer_event(
          "mousedown",
          &last,
          last.button,
          tracker.buttons,
          pressure,
          (0.0, 0.0),
          0,
        )));
      }
      PointerKind::Up => {
        let pressure = last.pressure.unwrap_or(0.0);
        if let Some(bit) = button_bit(last.button) {
          tracker.buttons &= !bit;
        }
        let buttons = tracker.buttons;
        steps.push(Step::Pointer(pointer_event(
          "pointerup",
          &last,
          last.button,
          buttons,
          pressure,
          (0.0, 0.0),
          0,
        )));
        steps.push(Step::Pointer(pointer_event(
          "mouseup",
          &last,
          last.button,
          buttons,
          pressure,
          (0.0, 0.0),
          0,
        )));
        if last.button == 0 {
          let detail = click_detail(&mut tracker, &last);
          steps.push(Step::Pointer(pointer_event(
            "click",
            &last,
            0,
            buttons,
            0.0,
            (0.0, 0.0),
            detail,
          )));
          if detail == 2 {
            steps.push(Step::Pointer(pointer_event(
              "dblclick",
              &last,
              0,
              buttons,
              0.0,
              (0.0, 0.0),
              detail,
            )));
          }
        } else {
          steps.push(Step::Pointer(pointer_event(
            "auxclick",
            &last,
            last.button,
            buttons,
            0.0,
            (0.0, 0.0),
            0,
          )));
          if last.button == 2 {
            steps.push(Step::Pointer(pointer_event(
              "contextmenu",
              &last,
              2,
              buttons,
              0.0,
              (0.0, 0.0),
              0,
            )));
          }
        }
        if release_capture(&mut tracker, last.pointer_id) {
          steps.push(Step::Pointer(pointer_event(
            "lostpointercapture",
            &last,
            last.button,
            buttons,
            0.0,
            (0.0, 0.0),
            0,
          )));
        }
        tracker.active.retain(|id| *id != last.pointer_id);
      }
      PointerKind::Cancel => {
        tracker.buttons = 0;
        let lost = release_capture(&mut tracker, last.pointer_id);
        tracker.active.retain(|id| *id != last.pointer_id);
        steps.push(Step::Pointer(pointer_event(
          "pointercancel",
          &last,
          last.button,
          0,
          0.0,
          (0.0, 0.0),
          0,
        )));
        if lost {
          steps.push(Step::Pointer(pointer_event(
            "lostpointercapture",
            &last,
            last.button,
            0,
            0.0,
            (0.0, 0.0),
            0,
          )));
        }
      }
    }
    steps
  }

  fn plan_key(&self, key: RawKey) -> Vec<Step> {
    let composing = self.pointer_state().composing();
    let text = if key.down { key.text.clone() } else { None };
    let command = activation::clipboard_command(&key);
    let mut steps = vec![Step::Key(
      FiredKey {
        type_name: if key.down {
          "keydown".to_string()
        } else {
          "keyup".to_string()
        },
        key: key.key,
        code: key.code,
        location: key.location,
        repeat: key.repeat,
        is_composing: composing,
        ctrl: key.ctrl,
        shift: key.shift,
        alt: key.alt,
        meta: key.meta,
      },
      text,
    )];
    if activation::focused(self.id())
      && let Some(command) = command
    {
      steps.push(Step::Clipboard(command));
    }
    steps
  }

  fn plan_ime(&self, ime: RawIme) -> Vec<Step> {
    if !inbox::ime_allowed(self.id()) {
      return Vec::new();
    }
    let mut tracker = self.pointer_state().tracker.borrow_mut();
    match ime {
      RawIme::Enabled => {
        if tracker.ime_open {
          return Vec::new();
        }
        tracker.ime_open = true;
        tracker.preedit.clear();
        vec![Step::Composition(FiredComposition {
          type_name: "compositionstart".to_string(),
          data: String::new(),
        })]
      }
      RawIme::Preedit(text) => {
        tracker.ime_open = true;
        tracker.preedit = text.clone();
        vec![
          Step::Composition(FiredComposition {
            type_name: "compositionupdate".to_string(),
            data: text.clone(),
          }),
          Step::Input(FiredInput {
            input_type: "insertCompositionText".to_string(),
            data: text,
            is_composing: true,
          }),
        ]
      }
      RawIme::Commit(text) => {
        tracker.preedit.clear();
        tracker.ime_open = false;
        vec![
          Step::Composition(FiredComposition {
            type_name: "compositionend".to_string(),
            data: text.clone(),
          }),
          Step::Input(FiredInput {
            input_type: "insertText".to_string(),
            data: text,
            is_composing: false,
          }),
        ]
      }
      RawIme::Disabled => {
        if !tracker.ime_open && tracker.preedit.is_empty() {
          return Vec::new();
        }
        let data = std::mem::take(&mut tracker.preedit);
        tracker.ime_open = false;
        vec![Step::Composition(FiredComposition {
          type_name: "compositionend".to_string(),
          data,
        })]
      }
    }
  }
}

fn mark_active(tracker: &mut Tracker, pointer_id: i32) {
  if !tracker.active.contains(&pointer_id) {
    tracker.active.push(pointer_id);
  }
}

fn release_capture(tracker: &mut Tracker, pointer_id: i32) -> bool {
  let before = tracker.captured.len();
  tracker.captured.retain(|id| *id != pointer_id);
  tracker.captured.len() != before
}

fn movement_of(tracker: &mut Tracker, pointer: &RawPointer) -> (f64, f64) {
  if let (Some(x), Some(y)) = (pointer.movement_x, pointer.movement_y) {
    return (x, y);
  }
  if tracker.has_last {
    (pointer.x - tracker.last_x, pointer.y - tracker.last_y)
  } else {
    (0.0, 0.0)
  }
}

fn pressure_of(pointer: &RawPointer, buttons: u16) -> f64 {
  if let Some(pressure) = pointer.pressure {
    return pressure;
  }
  if pointer.pointer_type == "mouse" {
    if buttons == 0 { 0.0 } else { 0.5 }
  } else if buttons == 0 {
    0.0
  } else {
    0.5
  }
}

fn button_bit(button: i32) -> Option<u16> {
  match button {
    0 => Some(1),
    1 => Some(4),
    2 => Some(2),
    3 => Some(8),
    4 => Some(16),
    _ => None,
  }
}

fn click_detail(tracker: &mut Tracker, pointer: &RawPointer) -> i32 {
  let now = Instant::now();
  let repeat = tracker.last_click.is_some_and(|last| {
    now.saturating_duration_since(last).as_millis() <= CLICK_MS
      && tracker.last_click_button == pointer.button
      && (pointer.x - tracker.last_click_x).abs() <= CLICK_SLOP
      && (pointer.y - tracker.last_click_y).abs() <= CLICK_SLOP
  });
  tracker.click_count = if repeat {
    tracker.click_count.saturating_add(1)
  } else {
    1
  };
  tracker.last_click = Some(now);
  tracker.last_click_x = pointer.x;
  tracker.last_click_y = pointer.y;
  tracker.last_click_button = pointer.button;
  tracker.click_count as i32
}

fn pointer_event(
  type_name: &str,
  pointer: &RawPointer,
  button: i32,
  buttons: u16,
  pressure: f64,
  movement: (f64, f64),
  detail: i32,
) -> FiredPointer {
  let mut event = FiredPointer::new(type_name);
  event.detail = detail;
  event.screen_x = pointer.screen_x;
  event.screen_y = pointer.screen_y;
  event.client_x = pointer.x;
  event.client_y = pointer.y;
  event.movement_x = movement.0;
  event.movement_y = movement.1;
  event.button = button;
  event.buttons = buttons;
  event.ctrl = pointer.ctrl;
  event.shift = pointer.shift;
  event.alt = pointer.alt;
  event.meta = pointer.meta;
  event.pointer_id = pointer.pointer_id;
  event.width = pointer.width;
  event.height = pointer.height;
  event.pressure = pressure;
  event.tangential = pointer.tangential;
  event.tilt_x = pointer.tilt_x;
  event.tilt_y = pointer.tilt_y;
  event.twist = pointer.twist;
  event.pointer_type = pointer.pointer_type.clone();
  event.is_primary = pointer.is_primary;
  if matches!(type_name, "gotpointercapture" | "lostpointercapture") {
    event.cancelable = false;
  }
  event
}

fn wheel_event(wheel: RawWheel) -> FiredWheel {
  FiredWheel {
    screen_x: wheel.screen_x,
    screen_y: wheel.screen_y,
    client_x: wheel.x,
    client_y: wheel.y,
    delta_x: wheel.delta_x,
    delta_y: wheel.delta_y,
    delta_z: wheel.delta_z,
    delta_mode: wheel.delta_mode,
    ctrl: wheel.ctrl,
    shift: wheel.shift,
    alt: wheel.alt,
    meta: wheel.meta,
  }
}

pub fn got_capture_event(pointer_id: i32) -> FiredPointer {
  let mut event = FiredPointer::new("gotpointercapture");
  event.pointer_id = pointer_id;
  event.button = -1;
  event.cancelable = false;
  event
}
