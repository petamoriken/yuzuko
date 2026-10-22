//! `navigator.getGamepads()` and `gamepadconnected` / `gamepaddisconnected`
//! on `globalThis`. Already-connected devices are exposed without waiting
//! for a button press. Dual-rumble plays when the device supports force
//! feedback.

use std::cell::RefCell;
use std::collections::HashMap;
use std::mem::ManuallyDrop;
use std::time::Duration;

use gilrs::Axis;
use gilrs::Button;
use gilrs::Gilrs;
use gilrs::MappingSource;
use gilrs::ff::BaseEffect;
use gilrs::ff::BaseEffectType;
use gilrs::ff::Effect;
use gilrs::ff::EffectBuilder;
use gilrs::ff::Repeat;
use gilrs::ff::Replay;
use gilrs::ff::Ticks;
use yuzuko_core::op::Js;
use yuzuko_core::op::OpError;
use yuzuko_core::op::OpException;
use yuzuko_dom::Event;
use yuzuko_dom::define_is_trusted;
use yuzuko_macros::op;

use crate::TEST_INPUT;
use crate::inbox;

const STANDARD_BUTTONS: [Button; 17] = [
  Button::South,
  Button::East,
  Button::West,
  Button::North,
  Button::LeftTrigger,
  Button::RightTrigger,
  Button::LeftTrigger2,
  Button::RightTrigger2,
  Button::Select,
  Button::Start,
  Button::LeftThumb,
  Button::RightThumb,
  Button::DPadUp,
  Button::DPadDown,
  Button::DPadLeft,
  Button::DPadRight,
  Button::Mode,
];

const STANDARD_AXES: [Axis; 4] = [
  Axis::LeftStickX,
  Axis::LeftStickY,
  Axis::RightStickX,
  Axis::RightStickY,
];

/// The Gamepad spec recommends capping `startDelay + duration` at 5 seconds.
const MAX_EFFECT_MS: u64 = 5_000;

#[derive(Clone, Copy, PartialEq)]
struct ButtonReading {
  pressed: bool,
  touched: bool,
  value: f64,
}

struct Playing {
  generation: u64,
  resolver: v8::Global<v8::PromiseResolver>,
  timer: i32,
}

struct Pad {
  object: v8::Global<v8::Object>,
  buttons: Vec<v8::Global<v8::Object>>,
  actuator: Option<v8::Global<v8::Object>>,
  effect_list: v8::Global<v8::Value>,
  id: String,
  index: i32,
  connected: bool,
  exposed: bool,
  timestamp: f64,
  mapping: String,
  axes: Vec<f64>,
  readings: Vec<ButtonReading>,
  dual_rumble: bool,
  gilrs_id: Option<gilrs::GamepadId>,
  play_seq: u64,
  playing: Option<Playing>,
  effect: Option<Effect>,
}

struct Store {
  slots: Vec<Option<u64>>,
  pads: HashMap<u64, Pad>,
  gilrs_ids: HashMap<gilrs::GamepadId, u64>,
  next: u64,
  scanned: bool,
  handlers: HashMap<String, v8::Global<v8::Value>>,
}

struct Devices {
  /// Not dropped. On Windows, gilrs joins its input thread in `Drop`, and a
  /// thread-local destructor at exit runs after Windows has already ended
  /// that thread, which aborts the process.
  gilrs: Option<ManuallyDrop<Gilrs>>,
  failed: bool,
}

thread_local! {
  static STORE: RefCell<Store> = RefCell::new(Store {
    slots: Vec::new(),
    pads: HashMap::new(),
    gilrs_ids: HashMap::new(),
    next: 1,
    scanned: false,
    handlers: HashMap::new(),
  });
  static DEVICES: RefCell<Devices> =
    const { RefCell::new(Devices { gilrs: None, failed: false }) };
}

pub fn clear() {
  let timers = STORE.with(|slot| {
    let mut store = slot.borrow_mut();
    let mut timers = Vec::new();
    for pad in store.pads.values_mut() {
      if let Some(playing) = pad.playing.take() {
        timers.push(playing.timer);
      }
      if let Some(effect) = pad.effect.take() {
        let _ = effect.stop();
      }
    }
    *store = Store {
      slots: Vec::new(),
      pads: HashMap::new(),
      gilrs_ids: HashMap::new(),
      next: 1,
      scanned: false,
      handlers: HashMap::new(),
    };
    timers
  });
  for timer in timers {
    yuzuko_core::cancel_host(timer);
  }
}

pub fn poll(scope: &mut v8::PinScope) -> bool {
  if TEST_INPUT.load(std::sync::atomic::Ordering::SeqCst) {
    return false;
  }
  let scan = STORE.with(|slot| {
    let store = slot.borrow();
    !store.scanned || inbox::any_open()
  });
  if !scan {
    return false;
  }
  STORE.with(|slot| slot.borrow_mut().scanned = true);
  let Some(snaps) = read_devices() else {
    return false;
  };
  apply_snapshots(scope, snaps)
}

pub fn install(scope: &mut v8::PinScope) {
  Gamepad::install(scope);
  GamepadButton::install(scope);
  GamepadHapticActuator::install(scope);
  GamepadPose::install(scope);
  GamepadEvent::install(scope);
  Navigator::install(scope);
  attach_navigator(scope);
  install_handler(scope, "ongamepadconnected", "gamepadconnected");
  install_handler(scope, "ongamepaddisconnected", "gamepaddisconnected");
}

pub fn install_test(scope: &mut v8::PinScope, api: v8::Local<v8::Object>) {
  let function = v8::Function::builder(test_gamepad)
    .length(1)
    .constructor_behavior(v8::ConstructorBehavior::Throw)
    .build(scope)
    .expect("gamepad");
  let name = v8::String::new(scope, "gamepad").unwrap();
  api.set(scope, name.into(), function.into());
}

fn test_gamepad(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut _ret: v8::ReturnValue,
) {
  if let Err(error) = apply_test(scope, args.get(0)) {
    OpException::throw(error, scope);
  }
}

fn apply_test(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<(), OpError> {
  let Ok(record) = v8::Local::<v8::Object>::try_from(value) else {
    return Err(OpError::type_("gamepad requires an object"));
  };
  let kind = object_string(scope, record, "type").unwrap_or_default();
  match kind.as_str() {
    "connect" => {
      let mapping = object_string(scope, record, "mapping")
        .unwrap_or_else(|| "standard".to_string());
      let id = object_string(scope, record, "id")
        .unwrap_or_else(|| "Test Gamepad".to_string());
      let mut axes = object_numbers(scope, record, "axes")?;
      let mut readings = object_buttons(scope, record, "buttons")?;
      if mapping == "standard" {
        if axes.len() < STANDARD_AXES.len() {
          axes.resize(STANDARD_AXES.len(), 0.0);
        }
        if readings.len() < STANDARD_BUTTONS.len() {
          readings.resize(
            STANDARD_BUTTONS.len(),
            ButtonReading {
              pressed: false,
              touched: false,
              value: 0.0,
            },
          );
        }
      }
      // Test pads have no gilrs device. `vibration: true` lists dual-rumble
      // and runs the promise state machine without a motor.
      let dual_rumble =
        object_flag(scope, record, "vibration").unwrap_or(false);
      connect_pad(scope, id, mapping, axes, readings, dual_rumble, None);
      Ok(())
    }
    "update" => {
      let index = object_index(scope, record)?;
      let axes = object_numbers(scope, record, "axes")?;
      let readings = object_buttons(scope, record, "buttons")?;
      let token = token_at(index)
        .ok_or_else(|| OpError::type_("The gamepad index is not connected"))?;
      STORE.with(|slot| {
        let mut store = slot.borrow_mut();
        let Some(pad) = store.pads.get_mut(&token) else {
          return;
        };
        if !axes.is_empty() {
          pad.axes = axes;
        }
        if !readings.is_empty() {
          pad.readings = readings;
        }
        pad.timestamp = yuzuko_core::performance_now_ms();
      });
      refresh_buttons(scope, token);
      Ok(())
    }
    "disconnect" => {
      let index = object_index(scope, record)?;
      let token = token_at(index)
        .ok_or_else(|| OpError::type_("The gamepad index is not connected"))?;
      disconnect_pad(scope, token);
      Ok(())
    }
    _ => Err(OpError::type_(
      "gamepad type must be connect, update, or disconnect",
    )),
  }
}

struct Snapshot {
  gid: gilrs::GamepadId,
  id: String,
  mapping: String,
  axes: Vec<f64>,
  readings: Vec<ButtonReading>,
  dual_rumble: bool,
}

fn read_devices() -> Option<Vec<Snapshot>> {
  DEVICES.with(|slot| {
    let mut devices = slot.borrow_mut();
    if devices.failed {
      return None;
    }
    if devices.gilrs.is_none() {
      match Gilrs::new() {
        Ok(gilrs) => devices.gilrs = Some(ManuallyDrop::new(gilrs)),
        Err(_) => {
          devices.failed = true;
          return None;
        }
      }
    }
    let gilrs = devices.gilrs.as_mut().unwrap();
    for _ in 0..64 {
      if gilrs.next_event().is_none() {
        break;
      }
    }
    let snaps = gilrs
      .gamepads()
      .filter(|(_, pad)| pad.is_connected())
      .map(|(_, pad)| snapshot(&pad))
      .collect();
    Some(snaps)
  })
}

fn snapshot(pad: &gilrs::Gamepad) -> Snapshot {
  let standard = pad.mapping_source() != MappingSource::None;
  let id = match (pad.vendor_id(), pad.product_id()) {
    (Some(vendor), Some(product)) => format!(
      "{} (Vendor: {vendor:04x} Product: {product:04x})",
      pad.name()
    ),
    _ => pad.name().to_string(),
  };
  let (mapping, axes, readings) = if standard {
    (
      "standard".to_string(),
      STANDARD_AXES
        .into_iter()
        .map(|axis| {
          pad
            .axis_data(axis)
            .map(|data| f64::from(data.value()))
            .unwrap_or(0.0)
        })
        .collect(),
      STANDARD_BUTTONS
        .into_iter()
        .map(|button| button_reading(pad, button))
        .collect(),
    )
  } else {
    (
      String::new(),
      pad
        .state()
        .axes()
        .map(|(_, data)| f64::from(data.value()))
        .collect(),
      pad
        .state()
        .buttons()
        .map(|(_, data)| reading_from(data.is_pressed(), data.value()))
        .collect(),
    )
  };
  Snapshot {
    gid: pad.id(),
    id,
    mapping,
    axes,
    readings,
    dual_rumble: pad.is_ff_supported(),
  }
}

fn button_reading(pad: &gilrs::Gamepad, button: Button) -> ButtonReading {
  match pad.button_data(button) {
    Some(data) => reading_from(data.is_pressed(), data.value()),
    None => ButtonReading {
      pressed: false,
      touched: false,
      value: 0.0,
    },
  }
}

fn reading_from(pressed: bool, value: f32) -> ButtonReading {
  let value = f64::from(value);
  ButtonReading {
    pressed,
    touched: pressed || value > 0.0,
    value,
  }
}

fn apply_snapshots(scope: &mut v8::PinScope, snaps: Vec<Snapshot>) -> bool {
  let seen: Vec<gilrs::GamepadId> = snaps.iter().map(|snap| snap.gid).collect();
  let gone = STORE.with(|slot| {
    slot
      .borrow()
      .gilrs_ids
      .keys()
      .copied()
      .filter(|id| !seen.contains(id))
      .collect::<Vec<_>>()
  });
  let mut fired = false;
  for gid in gone {
    let token = STORE.with(|slot| slot.borrow_mut().gilrs_ids.remove(&gid));
    if let Some(token) = token {
      fired |= disconnect_pad(scope, token);
    }
  }
  for snap in snaps {
    let existing =
      STORE.with(|slot| slot.borrow().gilrs_ids.get(&snap.gid).copied());
    if let Some(token) = existing {
      fired |= update_from_snapshot(scope, token, &snap);
    } else {
      let token = connect_pad(
        scope,
        snap.id.clone(),
        snap.mapping.clone(),
        snap.axes.clone(),
        snap.readings.clone(),
        snap.dual_rumble,
        Some(snap.gid),
      );
      STORE.with(|slot| {
        slot.borrow_mut().gilrs_ids.insert(snap.gid, token);
      });
      fired = true;
    }
  }
  fired
}

fn update_from_snapshot(
  scope: &mut v8::PinScope,
  token: u64,
  snap: &Snapshot,
) -> bool {
  let changed = STORE.with(|slot| {
    let mut store = slot.borrow_mut();
    let Some(pad) = store.pads.get_mut(&token) else {
      return false;
    };
    if pad.axes == snap.axes && pad.readings == snap.readings {
      return false;
    }
    pad.axes = snap.axes.clone();
    pad.readings = snap.readings.clone();
    pad.timestamp = yuzuko_core::performance_now_ms();
    true
  });
  if changed {
    refresh_buttons(scope, token);
  }
  false
}

fn connect_pad(
  scope: &mut v8::PinScope,
  id: String,
  mapping: String,
  axes: Vec<f64>,
  readings: Vec<ButtonReading>,
  dual_rumble: bool,
  gilrs_id: Option<gilrs::GamepadId>,
) -> u64 {
  let (token, index) = STORE.with(|slot| {
    let mut store = slot.borrow_mut();
    let token = store.next;
    store.next += 1;
    let index = match store.slots.iter().position(|slot| slot.is_none()) {
      Some(index) => index,
      None => {
        store.slots.push(None);
        store.slots.len() - 1
      }
    };
    store.slots[index] = Some(token);
    (token, index)
  });
  let gamepad =
    unsafe { yuzuko_core::op::wrap_instance(scope, Gamepad { token }) };
  let object = v8::Global::new(scope, gamepad);
  let mut buttons = Vec::with_capacity(readings.len());
  for index in 0..readings.len() {
    let button = unsafe {
      yuzuko_core::op::wrap_instance(
        scope,
        GamepadButton {
          token,
          index: index as u32,
        },
      )
    };
    buttons.push(v8::Global::new(scope, button));
  }
  let actuator = unsafe {
    yuzuko_core::op::wrap_instance(scope, GamepadHapticActuator { token })
  };
  let effect_list = effect_names(scope, dual_rumble);
  STORE.with(|slot| {
    slot.borrow_mut().pads.insert(
      token,
      Pad {
        object,
        buttons,
        actuator: Some(v8::Global::new(scope, actuator)),
        effect_list,
        id,
        index: index as i32,
        connected: true,
        exposed: true,
        timestamp: yuzuko_core::performance_now_ms(),
        mapping,
        axes,
        readings,
        dual_rumble,
        gilrs_id,
        play_seq: 0,
        playing: None,
        effect: None,
      },
    );
  });
  fire(scope, "gamepadconnected", token);
  token
}

fn disconnect_pad(scope: &mut v8::PinScope, token: u64) -> bool {
  preempt_playing(token);
  let exposed = STORE.with(|slot| {
    let mut store = slot.borrow_mut();
    let Some(pad) = store.pads.get_mut(&token) else {
      return false;
    };
    if !pad.connected {
      return false;
    }
    pad.connected = false;
    pad.timestamp = yuzuko_core::performance_now_ms();
    pad.exposed
  });
  if !exposed {
    return false;
  }
  fire(scope, "gamepaddisconnected", token);
  STORE.with(|slot| {
    let mut store = slot.borrow_mut();
    if let Some(pad) = store.pads.get(&token) {
      let index = pad.index as usize;
      if store.slots.get(index) == Some(&Some(token)) {
        store.slots[index] = None;
      }
    }
    while store.slots.last() == Some(&None) {
      store.slots.pop();
    }
  });
  true
}

fn refresh_buttons(scope: &mut v8::PinScope, token: u64) {
  let count = STORE.with(|slot| {
    slot
      .borrow()
      .pads
      .get(&token)
      .map(|pad| pad.readings.len())
      .unwrap_or(0)
  });
  let have = STORE.with(|slot| {
    slot
      .borrow()
      .pads
      .get(&token)
      .map(|pad| pad.buttons.len())
      .unwrap_or(0)
  });
  if have == count {
    return;
  }
  let mut buttons = Vec::with_capacity(count);
  for index in 0..count {
    let button = unsafe {
      yuzuko_core::op::wrap_instance(
        scope,
        GamepadButton {
          token,
          index: index as u32,
        },
      )
    };
    buttons.push(v8::Global::new(scope, button));
  }
  STORE.with(|slot| {
    if let Some(pad) = slot.borrow_mut().pads.get_mut(&token) {
      pad.buttons = buttons;
    }
  });
}

fn fire(scope: &mut v8::PinScope, type_name: &str, token: u64) {
  let Some(gamepad) = pad_object(scope, token) else {
    return;
  };
  let event = GamepadEvent {
    base: Event::new(type_name, false, false, false, true),
    gamepad: RefCell::new(Some(yuzuko_core::op::traced(scope, gamepad))),
  };
  let object = unsafe { yuzuko_core::op::wrap_instance(scope, event) };
  define_is_trusted(scope, object);
  let global = scope.get_current_context().global(scope);
  let _ = yuzuko_dom::dispatch(scope, global, object);
}

fn token_at(index: i32) -> Option<u64> {
  if index < 0 {
    return None;
  }
  STORE.with(|slot| slot.borrow().slots.get(index as usize).copied().flatten())
}

fn pad_object<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  token: u64,
) -> Option<v8::Local<'s, v8::Object>> {
  STORE.with(|slot| {
    slot
      .borrow()
      .pads
      .get(&token)
      .map(|pad| v8::Local::new(scope, &pad.object))
  })
}

fn attach_navigator(scope: &mut v8::PinScope) {
  let global = scope.get_current_context().global(scope);
  let key = v8::String::new(scope, "navigator").unwrap();
  let previous = global.get(scope, key.into());
  let navigator = unsafe { yuzuko_core::op::wrap_instance(scope, Navigator) };
  if let Some(previous) =
    previous.and_then(|value| v8::Local::<v8::Object>::try_from(value).ok())
  {
    for name in ["userAgent", "platform"] {
      let name = v8::String::new(scope, name).unwrap();
      if let Some(value) = previous.get(scope, name.into()) {
        navigator.set(scope, name.into(), value);
      }
    }
  }
  global.set(scope, key.into(), navigator.into());
}

fn install_handler(scope: &mut v8::PinScope, attribute: &str, event: &str) {
  let event_name = v8::String::new(scope, event).unwrap();
  let getter = v8::Function::builder(read_handler)
    .data(event_name.into())
    .length(0)
    .constructor_behavior(v8::ConstructorBehavior::Throw)
    .build(scope)
    .expect("getter");
  let setter = v8::Function::builder(write_handler)
    .data(event_name.into())
    .length(1)
    .constructor_behavior(v8::ConstructorBehavior::Throw)
    .build(scope)
    .expect("setter");
  let key = v8::String::new(scope, attribute).unwrap();
  let global = scope.get_current_context().global(scope);
  let mut descriptor =
    v8::PropertyDescriptor::new_from_get_set(getter.into(), setter.into());
  descriptor.set_enumerable(true);
  descriptor.set_configurable(true);
  let _ = global.define_property(scope, key.into(), &descriptor);
}

fn read_handler(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut ret: v8::ReturnValue,
) {
  let event = args.data().to_rust_string_lossy(scope);
  let value = STORE.with(|slot| {
    slot
      .borrow()
      .handlers
      .get(&event)
      .map(|value| v8::Local::new(scope, value))
  });
  match value {
    Some(value) => ret.set(value),
    None => ret.set(v8::null(scope).into()),
  }
}

fn write_handler(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut _ret: v8::ReturnValue,
) {
  let event = args.data().to_rust_string_lossy(scope);
  let next = args.get(0);
  let previous = STORE.with(|slot| slot.borrow_mut().handlers.remove(&event));
  if let Some(previous) = previous {
    let previous = v8::Local::new(scope, &previous);
    call_listener(scope, "removeEventListener", &event, previous);
  }
  if next.is_function() {
    STORE.with(|slot| {
      slot
        .borrow_mut()
        .handlers
        .insert(event.clone(), v8::Global::new(scope, next));
    });
    call_listener(scope, "addEventListener", &event, next);
  }
}

fn call_listener(
  scope: &mut v8::PinScope,
  method: &str,
  event: &str,
  callback: v8::Local<v8::Value>,
) {
  let global = scope.get_current_context().global(scope);
  let Some(function) = global
    .get(scope, v8::String::new(scope, method).unwrap().into())
    .and_then(|value| v8::Local::<v8::Function>::try_from(value).ok())
  else {
    return;
  };
  let event = v8::String::new(scope, event).unwrap();
  let _ = function.call(scope, global.into(), &[event.into(), callback]);
}

fn object_string(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
) -> Option<String> {
  let value =
    object.get(scope, v8::String::new(scope, name).unwrap().into())?;
  if value.is_undefined() || value.is_null() {
    return None;
  }
  value
    .to_string(scope)
    .map(|text| text.to_rust_string_lossy(scope))
}

fn object_index(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
) -> Result<i32, OpError> {
  let Some(value) =
    object.get(scope, v8::String::new(scope, "index").unwrap().into())
  else {
    return Err(OpError::type_("The gamepad index is missing"));
  };
  let Some(number) = value.number_value(scope) else {
    return Err(OpError::type_("The gamepad index is missing"));
  };
  if !number.is_finite() || number < 0.0 {
    return Err(OpError::type_("The gamepad index is invalid"));
  }
  Ok(number as i32)
}

fn object_numbers(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
) -> Result<Vec<f64>, OpError> {
  let Some(value) =
    object.get(scope, v8::String::new(scope, name).unwrap().into())
  else {
    return Ok(Vec::new());
  };
  if value.is_undefined() || value.is_null() {
    return Ok(Vec::new());
  }
  let Ok(list) = v8::Local::<v8::Array>::try_from(value) else {
    return Err(OpError::type_(format!("The {name} value is not an array")));
  };
  let mut numbers = Vec::with_capacity(list.length() as usize);
  for index in 0..list.length() {
    let Some(item) = list.get_index(scope, index) else {
      numbers.push(0.0);
      continue;
    };
    numbers.push(item.number_value(scope).unwrap_or(0.0));
  }
  Ok(numbers)
}

fn object_buttons(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
) -> Result<Vec<ButtonReading>, OpError> {
  let Some(value) =
    object.get(scope, v8::String::new(scope, name).unwrap().into())
  else {
    return Ok(Vec::new());
  };
  if value.is_undefined() || value.is_null() {
    return Ok(Vec::new());
  }
  let Ok(list) = v8::Local::<v8::Array>::try_from(value) else {
    return Err(OpError::type_(format!("The {name} value is not an array")));
  };
  let mut readings = Vec::with_capacity(list.length() as usize);
  for index in 0..list.length() {
    let Some(item) = list.get_index(scope, index) else {
      readings.push(ButtonReading {
        pressed: false,
        touched: false,
        value: 0.0,
      });
      continue;
    };
    if item.is_number() {
      let number = item.number_value(scope).unwrap_or(0.0);
      let pressed = number > 0.0;
      readings.push(ButtonReading {
        pressed,
        touched: pressed,
        value: number,
      });
      continue;
    }
    let Ok(record) = v8::Local::<v8::Object>::try_from(item) else {
      return Err(OpError::type_("A gamepad button must be an object"));
    };
    let value = object_numbers_one(scope, record);
    let pressed = object_flag(scope, record, "pressed").unwrap_or(value > 0.0);
    let touched =
      object_flag(scope, record, "touched").unwrap_or(pressed || value > 0.0);
    readings.push(ButtonReading {
      pressed,
      touched,
      value,
    });
  }
  Ok(readings)
}

fn object_numbers_one(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
) -> f64 {
  object
    .get(scope, v8::String::new(scope, "value").unwrap().into())
    .and_then(|value| value.number_value(scope))
    .unwrap_or(0.0)
}

fn object_flag(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
) -> Option<bool> {
  let value =
    object.get(scope, v8::String::new(scope, name).unwrap().into())?;
  if value.is_undefined() {
    return None;
  }
  Some(value.boolean_value(scope))
}

fn frozen_strings<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  values: &[&str],
) -> v8::Local<'s, v8::Array> {
  let array = v8::Array::new(scope, values.len() as i32);
  for (index, value) in values.iter().enumerate() {
    let text = v8::String::new(scope, value).unwrap();
    array.set_index(scope, index as u32, text.into());
  }
  array.set_integrity_level(scope, v8::IntegrityLevel::Frozen);
  array
}

fn effect_names(
  scope: &mut v8::PinScope,
  dual_rumble: bool,
) -> v8::Global<v8::Value> {
  let names: &[&str] = if dual_rumble { &["dual-rumble"] } else { &[] };
  let array = frozen_strings(scope, names);
  let value: v8::Local<v8::Value> = array.into();
  v8::Global::new(scope, value)
}

fn frozen_numbers<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  values: &[f64],
) -> v8::Local<'s, v8::Array> {
  let array = v8::Array::new(scope, values.len() as i32);
  for (index, value) in values.iter().enumerate() {
    let number = v8::Number::new(scope, *value);
    array.set_index(scope, index as u32, number.into());
  }
  array.set_integrity_level(scope, v8::IntegrityLevel::Frozen);
  array
}

fn frozen_objects<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  values: &[v8::Global<v8::Object>],
) -> v8::Local<'s, v8::Array> {
  let array = v8::Array::new(scope, values.len() as i32);
  for (index, value) in values.iter().enumerate() {
    let object = v8::Local::new(scope, value);
    array.set_index(scope, index as u32, object.into());
  }
  array.set_integrity_level(scope, v8::IntegrityLevel::Frozen);
  array
}

fn js_array(scope: &mut v8::PinScope, array: v8::Local<v8::Array>) -> Js {
  let value: v8::Local<v8::Value> = array.into();
  Js(v8::Global::new(scope, value))
}

fn js_null(scope: &mut v8::PinScope) -> Js {
  let value: v8::Local<v8::Value> = v8::null(scope).into();
  Js(v8::Global::new(scope, value))
}

fn js_object(scope: &mut v8::PinScope, object: v8::Local<v8::Object>) -> Js {
  let value: v8::Local<v8::Value> = object.into();
  Js(v8::Global::new(scope, value))
}

fn reject_dom(scope: &mut v8::PinScope, name: &str, message: &str) -> Js {
  let resolver = v8::PromiseResolver::new(scope).unwrap();
  let promise = resolver.get_promise(scope);
  let error = yuzuko_core::op::new_dom_exception(scope, name, message)
    .map(v8::Local::<v8::Value>::from)
    .unwrap_or_else(|| {
      let message = v8::String::new(scope, message).unwrap();
      v8::Exception::type_error(scope, message)
    });
  resolver.reject(scope, error);
  let promise: v8::Local<v8::Value> = promise.into();
  Js(v8::Global::new(scope, promise))
}

fn resolve_text(scope: &mut v8::PinScope, text: &str) -> Js {
  let resolver = v8::PromiseResolver::new(scope).unwrap();
  let promise = resolver.get_promise(scope);
  let text = v8::String::new(scope, text).unwrap();
  resolver.resolve(scope, text.into());
  let promise: v8::Local<v8::Value> = promise.into();
  Js(v8::Global::new(scope, promise))
}

fn pad_connected(token: u64) -> bool {
  STORE.with(|slot| {
    slot
      .borrow()
      .pads
      .get(&token)
      .is_some_and(|pad| pad.connected)
  })
}

pub struct Gamepad {
  token: u64,
}

unsafe impl v8::cppgc::GarbageCollected for Gamepad {
  fn trace(&self, _visitor: &mut v8::cppgc::Visitor) {}

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"Gamepad"
  }
}

#[op]
impl Gamepad {
  #[constructor]
  fn new() -> Result<Self, OpError> {
    Err(OpError::type_("Illegal constructor"))
  }

  #[getter]
  fn id(&self) -> String {
    STORE.with(|slot| {
      slot
        .borrow()
        .pads
        .get(&self.token)
        .map(|pad| pad.id.clone())
        .unwrap_or_default()
    })
  }

  #[getter]
  fn index(&self) -> i32 {
    STORE.with(|slot| {
      slot
        .borrow()
        .pads
        .get(&self.token)
        .map(|pad| pad.index)
        .unwrap_or(0)
    })
  }

  #[getter]
  fn connected(&self) -> bool {
    pad_connected(self.token)
  }

  #[getter]
  fn timestamp(&self) -> f64 {
    STORE.with(|slot| {
      slot
        .borrow()
        .pads
        .get(&self.token)
        .map(|pad| pad.timestamp)
        .unwrap_or(0.0)
    })
  }

  #[getter]
  fn mapping(&self) -> String {
    STORE.with(|slot| {
      slot
        .borrow()
        .pads
        .get(&self.token)
        .map(|pad| pad.mapping.clone())
        .unwrap_or_default()
    })
  }

  #[getter]
  fn axes(&self, scope: &mut v8::PinScope) -> Js {
    let axes = STORE.with(|slot| {
      slot
        .borrow()
        .pads
        .get(&self.token)
        .map(|pad| pad.axes.clone())
        .unwrap_or_default()
    });
    let array = frozen_numbers(scope, &axes);
    js_array(scope, array)
  }

  #[getter]
  fn buttons(&self, scope: &mut v8::PinScope) -> Js {
    let buttons = STORE.with(|slot| {
      slot
        .borrow()
        .pads
        .get(&self.token)
        .map(|pad| pad.buttons.clone())
        .unwrap_or_default()
    });
    let array = frozen_objects(scope, &buttons);
    js_array(scope, array)
  }

  #[getter]
  fn touches(&self, scope: &mut v8::PinScope) -> Js {
    let array = frozen_objects(scope, &[]);
    js_array(scope, array)
  }

  #[getter]
  fn vibration_actuator(&self, scope: &mut v8::PinScope) -> Js {
    let actuator = STORE.with(|slot| {
      slot.borrow().pads.get(&self.token).and_then(|pad| {
        pad
          .actuator
          .as_ref()
          .map(|actuator| v8::Local::new(scope, actuator))
      })
    });
    match actuator {
      Some(actuator) => js_object(scope, actuator),
      None => js_null(scope),
    }
  }

  #[getter]
  fn hand(&self) -> String {
    String::new()
  }

  #[getter]
  fn haptic_actuators(&self, scope: &mut v8::PinScope) -> Js {
    let actuator = STORE.with(|slot| {
      slot
        .borrow()
        .pads
        .get(&self.token)
        .and_then(|pad| pad.actuator.clone())
    });
    let values = actuator.into_iter().collect::<Vec<_>>();
    let array = frozen_objects(scope, &values);
    js_array(scope, array)
  }

  #[getter]
  fn pose(&self, scope: &mut v8::PinScope) -> Js {
    let _ = self;
    js_null(scope)
  }
}

pub struct GamepadButton {
  token: u64,
  index: u32,
}

unsafe impl v8::cppgc::GarbageCollected for GamepadButton {
  fn trace(&self, _visitor: &mut v8::cppgc::Visitor) {}

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"GamepadButton"
  }
}

#[op]
impl GamepadButton {
  #[constructor]
  fn new() -> Result<Self, OpError> {
    Err(OpError::type_("Illegal constructor"))
  }

  #[getter]
  fn pressed(&self) -> bool {
    reading(self.token, self.index).is_some_and(|reading| reading.pressed)
  }

  #[getter]
  fn touched(&self) -> bool {
    reading(self.token, self.index).is_some_and(|reading| reading.touched)
  }

  #[getter]
  fn value(&self) -> f64 {
    reading(self.token, self.index)
      .map(|reading| reading.value)
      .unwrap_or(0.0)
  }
}

fn reading(token: u64, index: u32) -> Option<ButtonReading> {
  STORE.with(|slot| {
    slot
      .borrow()
      .pads
      .get(&token)
      .and_then(|pad| pad.readings.get(index as usize).copied())
  })
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum EffectKind {
  Dual,
  Trigger,
}

struct EffectParams {
  delay_ms: u64,
  duration_ms: u64,
  strong: f64,
  weak: f64,
}

fn disconnected(scope: &mut v8::PinScope) -> Js {
  reject_dom(scope, "InvalidStateError", "The gamepad is disconnected")
}

fn reject_type(scope: &mut v8::PinScope, message: &str) -> Js {
  let resolver = v8::PromiseResolver::new(scope).unwrap();
  let promise = resolver.get_promise(scope);
  let message = v8::String::new(scope, message).unwrap();
  let error = v8::Exception::type_error(scope, message);
  resolver.reject(scope, error);
  let promise: v8::Local<v8::Value> = promise.into();
  Js(v8::Global::new(scope, promise))
}

fn open_promise(
  scope: &mut v8::PinScope,
) -> (v8::Global<v8::PromiseResolver>, Js) {
  let resolver = v8::PromiseResolver::new(scope).unwrap();
  let promise = resolver.get_promise(scope);
  let resolver = v8::Global::new(scope, resolver);
  let promise: v8::Local<v8::Value> = promise.into();
  (resolver, Js(v8::Global::new(scope, promise)))
}

fn resolve_resolver(
  scope: &mut v8::PinScope,
  resolver: v8::Global<v8::PromiseResolver>,
  text: &str,
) {
  let resolver = v8::Local::new(scope, resolver);
  let text = v8::String::new(scope, text).unwrap();
  resolver.resolve(scope, text.into());
}

fn reject_resolver(
  scope: &mut v8::PinScope,
  resolver: v8::Global<v8::PromiseResolver>,
  name: &str,
  message: &str,
) {
  let resolver = v8::Local::new(scope, resolver);
  let error = yuzuko_core::op::new_dom_exception(scope, name, message)
    .map(v8::Local::<v8::Value>::from)
    .unwrap_or_else(|| {
      let message = v8::String::new(scope, message).unwrap();
      v8::Exception::type_error(scope, message)
    });
  resolver.reject(scope, error);
}

/// WebIDL `unsigned long long`, then the 5 second cap.
///
/// A negative value wraps to a huge unsigned integer, which the cap clamps.
fn effect_millis(number: f64) -> Result<u64, ()> {
  if !number.is_finite() {
    return Err(());
  }
  let trunc = number.trunc();
  if trunc < 0.0 || trunc >= MAX_EFFECT_MS as f64 {
    return Ok(MAX_EFFECT_MS);
  }
  Ok(trunc as u64)
}

fn clamp_effect(start_delay: u64, duration: u64) -> (u64, u64) {
  let start_delay = start_delay.min(MAX_EFFECT_MS);
  let duration = duration.min(MAX_EFFECT_MS - start_delay);
  (start_delay, duration)
}

fn unit_interval(number: f64) -> Result<f64, ()> {
  if number.is_finite() && (0.0..=1.0).contains(&number) {
    Ok(number)
  } else {
    Err(())
  }
}

fn motor_magnitude(value: f64) -> u16 {
  let scaled = (value * 65535.0).round();
  if scaled <= 0.0 {
    0
  } else if scaled >= 65535.0 {
    65535
  } else {
    scaled as u16
  }
}

fn effect_kind(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<EffectKind, ()> {
  let text = {
    v8::tc_scope!(let tc, scope);
    match value.to_string(tc) {
      Some(text) => Ok(text.to_rust_string_lossy(tc)),
      None => Err(()),
    }
  }?;
  match text.as_str() {
    "dual-rumble" => Ok(EffectKind::Dual),
    "trigger-rumble" => Ok(EffectKind::Trigger),
    _ => Err(()),
  }
}

fn effect_parameters(
  scope: &mut v8::PinScope,
  kind: EffectKind,
  params: Option<v8::Local<v8::Value>>,
) -> Result<EffectParams, ()> {
  let Some(value) = params.filter(|value| !value.is_null_or_undefined()) else {
    return Ok(EffectParams {
      delay_ms: 0,
      duration_ms: 0,
      strong: 0.0,
      weak: 0.0,
    });
  };
  let Ok(object) = v8::Local::<v8::Object>::try_from(value) else {
    return Err(());
  };
  let duration_ms = dictionary_millis(scope, object, "duration")?.unwrap_or(0);
  let delay_ms = dictionary_millis(scope, object, "startDelay")?.unwrap_or(0);
  let strong =
    dictionary_unit(scope, object, "strongMagnitude")?.unwrap_or(0.0);
  let weak = dictionary_unit(scope, object, "weakMagnitude")?.unwrap_or(0.0);
  if kind == EffectKind::Trigger {
    let _ = dictionary_unit(scope, object, "leftTrigger")?.unwrap_or(0.0);
    let _ = dictionary_unit(scope, object, "rightTrigger")?.unwrap_or(0.0);
  }
  Ok(EffectParams {
    delay_ms,
    duration_ms,
    strong,
    weak,
  })
}

fn dictionary_millis(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
) -> Result<Option<u64>, ()> {
  match dictionary_number(scope, object, name)? {
    Some(number) => effect_millis(number).map(Some),
    None => Ok(None),
  }
}

fn dictionary_unit(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
) -> Result<Option<f64>, ()> {
  match dictionary_number(scope, object, name)? {
    Some(number) => unit_interval(number).map(Some),
    None => Ok(None),
  }
}

fn dictionary_number(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
) -> Result<Option<f64>, ()> {
  v8::tc_scope!(let tc, scope);
  let Some(key) = v8::String::new(tc, name) else {
    return Err(());
  };
  match object.get(tc, key.into()) {
    None => Err(()),
    Some(value) if value.is_undefined() => Ok(None),
    Some(value) => match value.number_value(tc) {
      Some(number) => Ok(Some(number)),
      None => Err(()),
    },
  }
}

fn effect_supported(token: u64, kind: EffectKind) -> bool {
  if kind != EffectKind::Dual {
    return false;
  }
  STORE.with(|slot| {
    slot
      .borrow()
      .pads
      .get(&token)
      .is_some_and(|pad| pad.dual_rumble)
  })
}

fn start_effect(
  scope: &mut v8::PinScope,
  token: u64,
  kind: EffectKind,
  params: EffectParams,
) -> Js {
  if !pad_connected(token) {
    return disconnected(scope);
  }
  preempt_playing(token);
  if !effect_supported(token, kind) {
    return reject_dom(
      scope,
      "NotSupportedError",
      "The haptic effect is not supported",
    );
  }
  let (delay_ms, duration_ms) =
    clamp_effect(params.delay_ms, params.duration_ms);
  let (resolver, promise) = open_promise(scope);
  let uploaded = start_motors(
    token,
    motor_magnitude(params.strong),
    motor_magnitude(params.weak),
    delay_ms as u32,
    duration_ms as u32,
  );
  let effect = match uploaded {
    Ok(effect) => effect,
    Err(()) => {
      reject_resolver(
        scope,
        resolver,
        "NotSupportedError",
        "The haptic effect is not supported",
      );
      return promise;
    }
  };
  let Some(generation) = next_generation(token) else {
    if let Some(effect) = effect {
      let _ = effect.stop();
    }
    reject_resolver(
      scope,
      resolver,
      "InvalidStateError",
      "The gamepad is disconnected",
    );
    return promise;
  };
  let timer = yuzuko_core::schedule_host(
    Duration::from_millis(delay_ms + duration_ms),
    move |scope| complete_playing(scope, token, generation),
  );
  if !store_playing(token, generation, resolver.clone(), timer, effect) {
    yuzuko_core::cancel_host(timer);
    reject_resolver(
      scope,
      resolver,
      "InvalidStateError",
      "The gamepad is disconnected",
    );
  }
  promise
}

fn next_generation(token: u64) -> Option<u64> {
  STORE.with(|slot| {
    let mut store = slot.borrow_mut();
    let pad = store.pads.get_mut(&token)?;
    pad.play_seq = pad.play_seq.wrapping_add(1);
    Some(pad.play_seq)
  })
}

fn store_playing(
  token: u64,
  generation: u64,
  resolver: v8::Global<v8::PromiseResolver>,
  timer: i32,
  effect: Option<Effect>,
) -> bool {
  STORE.with(|slot| {
    let mut store = slot.borrow_mut();
    let Some(pad) = store.pads.get_mut(&token) else {
      if let Some(effect) = effect {
        let _ = effect.stop();
      }
      return false;
    };
    pad.playing = Some(Playing {
      generation,
      resolver,
      timer,
    });
    pad.effect = effect;
    true
  })
}

fn complete_playing(scope: &mut v8::PinScope, token: u64, generation: u64) {
  let taken = STORE.with(|slot| {
    let mut store = slot.borrow_mut();
    let pad = store.pads.get_mut(&token)?;
    if pad
      .playing
      .as_ref()
      .is_none_or(|playing| playing.generation != generation)
    {
      return None;
    }
    let playing = pad.playing.take()?;
    let effect = pad.effect.take();
    Some((playing.resolver, effect))
  });
  let Some((resolver, effect)) = taken else {
    return;
  };
  if let Some(effect) = effect {
    let _ = effect.stop();
  }
  resolve_resolver(scope, resolver, "complete");
}

fn preempt_playing(token: u64) {
  let Some((resolver, timer, effect)) = take_playing(token) else {
    return;
  };
  yuzuko_core::cancel_host(timer);
  if let Some(effect) = effect {
    let _ = effect.stop();
  }
  yuzuko_core::enqueue(move |scope| {
    resolve_resolver(scope, resolver, "preempted");
  });
}

fn take_playing(
  token: u64,
) -> Option<(v8::Global<v8::PromiseResolver>, i32, Option<Effect>)> {
  STORE.with(|slot| {
    let mut store = slot.borrow_mut();
    let pad = store.pads.get_mut(&token)?;
    let playing = pad.playing.take()?;
    let effect = pad.effect.take();
    Some((playing.resolver, playing.timer, effect))
  })
}

/// `EffectBuilder` repeats forever unless `Repeat::For` is set. A
/// `play_for` of zero ticks also makes gilrs divide by zero, so a silent
/// effect stays on the host timer.
fn start_motors(
  token: u64,
  strong: u16,
  weak: u16,
  delay_ms: u32,
  duration_ms: u32,
) -> Result<Option<Effect>, ()> {
  if duration_ms == 0 || (strong == 0 && weak == 0) {
    return Ok(None);
  }
  let gid = STORE
    .with(|slot| slot.borrow().pads.get(&token).and_then(|pad| pad.gilrs_id));
  let Some(gid) = gid else {
    return Ok(None);
  };
  DEVICES.with(|slot| {
    let mut devices = slot.borrow_mut();
    let Some(gilrs) = devices.gilrs.as_deref_mut() else {
      return Err(());
    };
    let replay = Replay {
      after: Ticks::from_ms(delay_ms),
      play_for: Ticks::from_ms(duration_ms),
      with_delay: Ticks::from_ms(0),
    };
    if replay.play_for == Ticks::from_ms(0) {
      return Ok(None);
    }
    let span = replay.after + replay.play_for;
    let mut builder = EffectBuilder::new();
    let effect = builder
      .repeat(Repeat::For(span))
      .gamepads(&[gid])
      .add_effect(BaseEffect {
        kind: BaseEffectType::Strong { magnitude: strong },
        scheduling: replay,
        envelope: Default::default(),
      })
      .add_effect(BaseEffect {
        kind: BaseEffectType::Weak { magnitude: weak },
        scheduling: replay,
        envelope: Default::default(),
      })
      .finish(gilrs)
      .map_err(|_| ())?;
    effect.play().map_err(|_| ())?;
    Ok(Some(effect))
  })
}

pub struct GamepadHapticActuator {
  token: u64,
}

unsafe impl v8::cppgc::GarbageCollected for GamepadHapticActuator {
  fn trace(&self, _visitor: &mut v8::cppgc::Visitor) {}

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"GamepadHapticActuator"
  }
}

#[op]
impl GamepadHapticActuator {
  #[constructor]
  fn new() -> Result<Self, OpError> {
    Err(OpError::type_("Illegal constructor"))
  }

  #[getter]
  fn effects(&self, scope: &mut v8::PinScope) -> Js {
    let effects = STORE.with(|slot| {
      slot
        .borrow()
        .pads
        .get(&self.token)
        .map(|pad| pad.effect_list.clone())
    });
    match effects {
      Some(effects) => Js(effects),
      None => {
        let array = frozen_strings(scope, &[]);
        js_array(scope, array)
      }
    }
  }

  #[promise]
  fn play_effect(
    &self,
    scope: &mut v8::PinScope,
    effect_type: v8::Local<v8::Value>,
    params: Option<v8::Local<v8::Value>>,
  ) -> Js {
    if !pad_connected(self.token) {
      return disconnected(scope);
    }
    let kind = match effect_kind(scope, effect_type) {
      Ok(kind) => kind,
      Err(()) => {
        return reject_type(scope, "The haptic effect type is invalid");
      }
    };
    let params = match effect_parameters(scope, kind, params) {
      Ok(params) => params,
      Err(()) => {
        return reject_type(scope, "The haptic effect parameters are invalid");
      }
    };
    start_effect(scope, self.token, kind, params)
  }

  #[promise]
  fn reset(&self, scope: &mut v8::PinScope) -> Js {
    if !pad_connected(self.token) {
      return disconnected(scope);
    }
    preempt_playing(self.token);
    resolve_text(scope, "complete")
  }

  #[promise]
  fn pulse(&self, scope: &mut v8::PinScope, value: f64, duration: f64) -> Js {
    if !pad_connected(self.token) {
      return disconnected(scope);
    }
    if !value.is_finite()
      || !(0.0..=1.0).contains(&value)
      || !duration.is_finite()
      || duration < 0.0
    {
      return reject_type(scope, "The haptic effect parameters are invalid");
    }
    let duration_ms = effect_millis(duration).unwrap_or(MAX_EFFECT_MS);
    start_effect(
      scope,
      self.token,
      EffectKind::Dual,
      EffectParams {
        delay_ms: 0,
        duration_ms,
        strong: value,
        weak: value,
      },
    )
  }
}

pub struct GamepadPose;

unsafe impl v8::cppgc::GarbageCollected for GamepadPose {
  fn trace(&self, _visitor: &mut v8::cppgc::Visitor) {}

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"GamepadPose"
  }
}

#[op]
impl GamepadPose {
  #[constructor]
  fn new() -> Result<Self, OpError> {
    Err(OpError::type_("Illegal constructor"))
  }

  #[getter]
  fn has_orientation(&self) -> bool {
    false
  }

  #[getter]
  fn has_position(&self) -> bool {
    false
  }

  #[getter]
  fn position(&self, scope: &mut v8::PinScope) -> Js {
    let _ = self;
    js_null(scope)
  }

  #[getter]
  fn linear_velocity(&self, scope: &mut v8::PinScope) -> Js {
    let _ = self;
    js_null(scope)
  }

  #[getter]
  fn linear_acceleration(&self, scope: &mut v8::PinScope) -> Js {
    let _ = self;
    js_null(scope)
  }

  #[getter]
  fn orientation(&self, scope: &mut v8::PinScope) -> Js {
    let _ = self;
    js_null(scope)
  }

  #[getter]
  fn angular_velocity(&self, scope: &mut v8::PinScope) -> Js {
    let _ = self;
    js_null(scope)
  }

  #[getter]
  fn angular_acceleration(&self, scope: &mut v8::PinScope) -> Js {
    let _ = self;
    js_null(scope)
  }
}

pub struct Navigator;

unsafe impl v8::cppgc::GarbageCollected for Navigator {
  fn trace(&self, _visitor: &mut v8::cppgc::Visitor) {}

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"Navigator"
  }
}

#[op]
impl Navigator {
  #[constructor]
  fn new() -> Result<Self, OpError> {
    Err(OpError::type_("Illegal constructor"))
  }

  #[getter]
  fn clipboard(&self, scope: &mut v8::PinScope) -> Js {
    let _ = self;
    crate::shell::clipboard::instance(scope)
  }

  fn get_gamepads(&self, scope: &mut v8::PinScope) -> Js {
    let _ = self;
    let slots = STORE.with(|slot| slot.borrow().slots.clone());
    let array = v8::Array::new(scope, slots.len() as i32);
    for (index, token) in slots.into_iter().enumerate() {
      let value = match token.and_then(|token| pad_object(scope, token)) {
        Some(object) => {
          let value: v8::Local<v8::Value> = object.into();
          value
        }
        None => v8::null(scope).into(),
      };
      array.set_index(scope, index as u32, value);
    }
    js_array(scope, array)
  }
}

#[repr(C)]
pub struct GamepadEvent {
  base: Event,
  gamepad: RefCell<Option<v8::TracedReference<v8::Object>>>,
}

unsafe impl v8::cppgc::GarbageCollected for GamepadEvent {
  fn trace(&self, visitor: &mut v8::cppgc::Visitor) {
    v8::cppgc::GarbageCollected::trace(&self.base, visitor);
    if let Ok(gamepad) = self.gamepad.try_borrow() {
      visitor.trace(&*gamepad);
    }
  }

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"GamepadEvent"
  }
}

#[op(inherit = Event)]
impl GamepadEvent {
  #[constructor]
  #[length(1)]
  fn new(
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<Self, OpError> {
    let base = Event::from_args(scope, args, false)?;
    let init = if args.length() > 1 {
      Some(args.get(1))
    } else {
      None
    };
    let gamepad = read_gamepad_init(scope, init)?;
    Ok(Self {
      base,
      gamepad: RefCell::new(gamepad),
    })
  }

  #[construct_hook]
  fn finish(&self, scope: &mut v8::PinScope, object: v8::Local<v8::Object>) {
    let _ = self;
    define_is_trusted(scope, object);
  }

  #[getter]
  fn gamepad(&self, scope: &mut v8::PinScope) -> Js {
    let gamepad = self
      .gamepad
      .borrow()
      .as_ref()
      .and_then(|reference| yuzuko_core::op::traced_local(scope, reference));
    match gamepad {
      Some(gamepad) => js_object(scope, gamepad),
      None => js_null(scope),
    }
  }
}

fn read_gamepad_init(
  scope: &mut v8::PinScope,
  init: Option<v8::Local<v8::Value>>,
) -> Result<Option<v8::TracedReference<v8::Object>>, OpError> {
  let Some(init) = init.filter(|value| value.is_object()) else {
    return Ok(None);
  };
  let Ok(object) = v8::Local::<v8::Object>::try_from(init) else {
    return Ok(None);
  };
  let Some(value) =
    object.get(scope, v8::String::new(scope, "gamepad").unwrap().into())
  else {
    return Err(OpError::type_("GamepadEvent init is invalid"));
  };
  if value.is_undefined() || value.is_null() {
    return Ok(None);
  }
  let Ok(gamepad) = v8::Local::<v8::Object>::try_from(value) else {
    return Err(OpError::type_("GamepadEvent requires a Gamepad"));
  };
  if unsafe { yuzuko_core::op::host_ref::<Gamepad>(scope, gamepad) }.is_none() {
    return Err(OpError::type_("GamepadEvent requires a Gamepad"));
  }
  Ok(Some(yuzuko_core::op::traced(scope, gamepad)))
}

#[cfg(test)]
mod tests {
  use super::{MAX_EFFECT_MS, clamp_effect, effect_millis, motor_magnitude};

  #[test]
  fn effect_time_truncates_and_caps_the_total() {
    assert_eq!(effect_millis(10.9), Ok(10));
    assert_eq!(effect_millis(0.0), Ok(0));
    assert_eq!(effect_millis(-1.0), Ok(MAX_EFFECT_MS));
    assert_eq!(effect_millis(6_000.0), Ok(MAX_EFFECT_MS));
    assert!(effect_millis(f64::NAN).is_err());
    assert!(effect_millis(f64::INFINITY).is_err());
    assert_eq!(clamp_effect(2_000, 4_000), (2_000, 3_000));
    assert_eq!(clamp_effect(5_000, 10), (5_000, 0));
    assert_eq!(motor_magnitude(0.0), 0);
    assert_eq!(motor_magnitude(1.0), 65535);
    assert_eq!(motor_magnitude(0.5), 32768);
  }
}
