//! UI Events and Pointer Events. `Window` dispatches the trusted ones.
//!
//! Constructors are installed with the `window` feature. The dispatch entry
//! points stay available to that crate. Without the feature, nothing in a
//! runtime reaches the constructors, and the unit tests below still compile.
#![cfg_attr(not(feature = "window"), allow(dead_code))]

use std::cell::{Cell, RefCell};

use yuzuko_core::op::{FromV8, Js, OpError, host_ref, traced, traced_local};
use yuzuko_macros::op;

use crate::event::{self, Event};
use crate::target;
use crate::util::{self, key};

const PI: f64 = std::f64::consts::PI;

#[derive(Clone, Copy)]
struct Modifiers {
  ctrl: bool,
  shift: bool,
  alt: bool,
  meta: bool,
  alt_graph: bool,
  caps_lock: bool,
}

fn modifier_active(mods: Modifiers, name: &str) -> bool {
  match name {
    "Alt" => mods.alt,
    "Control" => mods.ctrl,
    "Shift" => mods.shift,
    "Meta" => mods.meta,
    "AltGraph" => mods.alt_graph,
    "CapsLock" => mods.caps_lock,
    // UI Events' virtual Accel modifier is Meta on macOS and Control elsewhere.
    "Accel" => {
      if cfg!(target_os = "macos") {
        mods.meta
      } else {
        mods.ctrl
      }
    }
    _ => false,
  }
}

fn read_modifiers(
  scope: &mut v8::PinScope,
  init: Option<v8::Local<v8::Value>>,
) -> Modifiers {
  Modifiers {
    ctrl: util::dict_bool(scope, init, "ctrlKey"),
    shift: util::dict_bool(scope, init, "shiftKey"),
    alt: util::dict_bool(scope, init, "altKey"),
    meta: util::dict_bool(scope, init, "metaKey"),
    alt_graph: util::dict_bool(scope, init, "modifierAltGraph"),
    caps_lock: util::dict_bool(scope, init, "modifierCapsLock"),
  }
}

fn idl_long(number: f64) -> i32 {
  if !number.is_finite() {
    return 0;
  }
  let trunc = number.trunc();
  if (i32::MIN as f64) <= trunc && trunc <= i32::MAX as f64 {
    return trunc as i32;
  }
  let wrapped = trunc.rem_euclid(4294967296.0) as u32;
  wrapped as i32
}

fn idl_unsigned(number: f64) -> u32 {
  if !number.is_finite() {
    return 0;
  }
  number.trunc().rem_euclid(4294967296.0) as u32
}

fn raw_number(
  scope: &mut v8::PinScope,
  init: Option<v8::Local<v8::Value>>,
  name: &str,
) -> f64 {
  let Some(value) = util::dict_get(scope, init, name) else {
    return 0.0;
  };
  if value.is_null() || value.is_undefined() {
    return 0.0;
  }
  value.number_value(scope).unwrap_or(f64::NAN)
}

fn optional_number(
  scope: &mut v8::PinScope,
  init: Option<v8::Local<v8::Value>>,
  name: &str,
) -> Option<f64> {
  if !util::has_own(scope, init, name) {
    return None;
  }
  let value = util::dict_get(scope, init, name)?;
  if value.is_undefined() {
    return None;
  }
  if value.is_null() {
    return Some(0.0);
  }
  value.number_value(scope)
}

fn number_or(
  scope: &mut v8::PinScope,
  init: Option<v8::Local<v8::Value>>,
  name: &str,
  default: f64,
) -> f64 {
  optional_number(scope, init, name).unwrap_or(default)
}

fn optional_long(
  scope: &mut v8::PinScope,
  init: Option<v8::Local<v8::Value>>,
  name: &str,
) -> Option<i32> {
  optional_number(scope, init, name).map(idl_long)
}

fn floor_coord(value: f64) -> f64 {
  value.floor()
}

fn js_round(number: f64) -> i32 {
  if !number.is_finite() {
    return 0;
  }
  (number + 0.5).floor() as i32
}

/// Spec tiltX/tiltY → azimuth/altitude. Boundary cases match the Pointer Events
/// examples, which compare with `===`.
pub(crate) fn tilt_to_spherical(tilt_x: i32, tilt_y: i32) -> (f64, f64) {
  let tilt_x = tilt_x as f64;
  let tilt_y = tilt_y as f64;
  let tilt_x_rad = tilt_x * PI / 180.0;
  let tilt_y_rad = tilt_y * PI / 180.0;
  let azimuth = if tilt_x == 0.0 {
    if tilt_y > 0.0 {
      PI / 2.0
    } else if tilt_y < 0.0 {
      3.0 * PI / 2.0
    } else {
      0.0
    }
  } else if tilt_y == 0.0 {
    if tilt_x < 0.0 { PI } else { 0.0 }
  } else if tilt_x.abs() == 90.0 || tilt_y.abs() == 90.0 {
    0.0
  } else {
    let mut azimuth = tilt_y_rad.tan().atan2(tilt_x_rad.tan());
    if azimuth < 0.0 {
      azimuth += 2.0 * PI;
    }
    azimuth
  };
  let altitude = if tilt_x.abs() == 90.0 || tilt_y.abs() == 90.0 {
    0.0
  } else if tilt_x == 0.0 {
    PI / 2.0 - tilt_y_rad.abs()
  } else if tilt_y == 0.0 {
    PI / 2.0 - tilt_x_rad.abs()
  } else {
    let denom = (tilt_x_rad.tan().powi(2) + tilt_y_rad.tan().powi(2)).sqrt();
    (1.0 / denom).atan()
  };
  (azimuth, altitude)
}

pub(crate) fn spherical_to_tilt(azimuth: f64, altitude: f64) -> (i32, i32) {
  let mut tilt_x_rad = 0.0;
  let mut tilt_y_rad = 0.0;
  if altitude == 0.0 {
    if azimuth == 0.0 || azimuth == 2.0 * PI {
      tilt_x_rad = PI / 2.0;
    }
    if azimuth == PI / 2.0 {
      tilt_y_rad = PI / 2.0;
    }
    if azimuth == PI {
      tilt_x_rad = -PI / 2.0;
    }
    if azimuth == 3.0 * PI / 2.0 {
      tilt_y_rad = -PI / 2.0;
    }
    if azimuth > 0.0 && azimuth < PI / 2.0 {
      tilt_x_rad = PI / 2.0;
      tilt_y_rad = PI / 2.0;
    }
    if azimuth > PI / 2.0 && azimuth < PI {
      tilt_x_rad = -PI / 2.0;
      tilt_y_rad = PI / 2.0;
    }
    if azimuth > PI && azimuth < 3.0 * PI / 2.0 {
      tilt_x_rad = -PI / 2.0;
      tilt_y_rad = -PI / 2.0;
    }
    if azimuth > 3.0 * PI / 2.0 && azimuth < 2.0 * PI {
      tilt_x_rad = PI / 2.0;
      tilt_y_rad = -PI / 2.0;
    }
  }
  if altitude != 0.0 {
    let tan_alt = altitude.tan();
    tilt_x_rad = (azimuth.cos() / tan_alt).atan();
    tilt_y_rad = (azimuth.sin() / tan_alt).atan();
  }
  let rad_to_deg = 180.0 / PI;
  (
    js_round(tilt_x_rad * rad_to_deg),
    js_round(tilt_y_rad * rad_to_deg),
  )
}

struct Orientation {
  tilt_x: i32,
  tilt_y: i32,
  azimuth: f64,
  altitude: f64,
}

fn orientation_from_init(
  scope: &mut v8::PinScope,
  init: Option<v8::Local<v8::Value>>,
) -> Orientation {
  let tilt_x = optional_long(scope, init, "tiltX");
  let tilt_y = optional_long(scope, init, "tiltY");
  let azimuth = optional_number(scope, init, "azimuthAngle");
  let altitude = optional_number(scope, init, "altitudeAngle");
  let tilt_set = tilt_x.is_some() || tilt_y.is_some();
  let sphere_set = azimuth.is_some() || altitude.is_some();
  if tilt_set && sphere_set {
    return Orientation {
      tilt_x: tilt_x.unwrap_or(0),
      tilt_y: tilt_y.unwrap_or(0),
      azimuth: azimuth.unwrap_or(0.0),
      altitude: altitude.unwrap_or(PI / 2.0),
    };
  }
  if tilt_set {
    let tilt_x = tilt_x.unwrap_or(0);
    let tilt_y = tilt_y.unwrap_or(0);
    let (azimuth, altitude) = tilt_to_spherical(tilt_x, tilt_y);
    return Orientation {
      tilt_x,
      tilt_y,
      azimuth,
      altitude,
    };
  }
  if sphere_set {
    let azimuth = azimuth.unwrap_or(0.0);
    let altitude = altitude.unwrap_or(PI / 2.0);
    let (tilt_x, tilt_y) = spherical_to_tilt(azimuth, altitude);
    return Orientation {
      tilt_x,
      tilt_y,
      azimuth,
      altitude,
    };
  }
  Orientation {
    tilt_x: 0,
    tilt_y: 0,
    azimuth: 0.0,
    altitude: PI / 2.0,
  }
}

fn read_view(
  scope: &mut v8::PinScope,
  init: Option<v8::Local<v8::Value>>,
) -> Result<Option<v8::TracedReference<v8::Object>>, OpError> {
  if !util::has_own(scope, init, "view") {
    return Ok(None);
  }
  let Some(value) = util::dict_get(scope, init, "view") else {
    return Ok(None);
  };
  if value.is_null() || value.is_undefined() {
    return Ok(None);
  }
  let Ok(object) = v8::Local::<v8::Object>::try_from(value) else {
    return Err(OpError::type_(
      "The provided value is not of type 'Window'.",
    ));
  };
  if !util::is_global(scope, object) {
    return Err(OpError::type_(
      "The provided value is not of type 'Window'.",
    ));
  }
  Ok(Some(traced(scope, object)))
}

fn read_related(
  scope: &mut v8::PinScope,
  init: Option<v8::Local<v8::Value>>,
) -> Result<Option<v8::TracedReference<v8::Object>>, OpError> {
  if !util::has_own(scope, init, "relatedTarget") {
    return Ok(None);
  }
  let Some(value) = util::dict_get(scope, init, "relatedTarget") else {
    return Ok(None);
  };
  if value.is_null() || value.is_undefined() {
    return Ok(None);
  }
  let Ok(object) = v8::Local::<v8::Object>::try_from(value) else {
    return Err(OpError::type_(
      "The provided value is not of type 'EventTarget'.",
    ));
  };
  Ok(Some(traced(scope, object)))
}

fn window_view(
  scope: &mut v8::PinScope,
) -> Option<v8::TracedReference<v8::Object>> {
  let global = scope.get_current_context().global(scope);
  Some(traced(scope, global))
}

fn traced_object(
  scope: &mut v8::PinScope,
  slot: &RefCell<Option<v8::TracedReference<v8::Object>>>,
) -> Js {
  let local = slot
    .borrow()
    .as_ref()
    .and_then(|reference| traced_local(scope, reference));
  match local {
    Some(object) => {
      let value: v8::Local<v8::Value> = object.into();
      Js(v8::Global::new(scope, value))
    }
    None => Js::null(scope),
  }
}

fn trace_optional(
  visitor: &mut v8::cppgc::Visitor,
  slot: &RefCell<Option<v8::TracedReference<v8::Object>>>,
) {
  if let Ok(slot) = slot.try_borrow() {
    visitor.trace(&*slot);
  }
}

fn trace_list(
  visitor: &mut v8::cppgc::Visitor,
  slot: &RefCell<Vec<v8::TracedReference<v8::Object>>>,
) {
  if let Ok(list) = slot.try_borrow() {
    for item in list.iter() {
      visitor.trace(item);
    }
  }
}

fn event_list(
  scope: &mut v8::PinScope,
  slot: &RefCell<Vec<v8::TracedReference<v8::Object>>>,
) -> Js {
  let count = slot.borrow().len();
  let mut held = Vec::with_capacity(count);
  for index in 0..count {
    let local = {
      let list = slot.borrow();
      traced_local(scope, &list[index])
    };
    if let Some(local) = local {
      held.push(v8::Global::new(scope, local));
    }
  }
  let array = v8::Array::new(scope, held.len() as i32);
  for (index, item) in held.iter().enumerate() {
    let value: v8::Local<v8::Value> = v8::Local::new(scope, item).into();
    array.set_index(scope, index as u32, value);
  }
  let value: v8::Local<v8::Value> = array.into();
  Js(v8::Global::new(scope, value))
}

#[repr(C)]
pub struct UIEvent {
  base: Event,
  view: RefCell<Option<v8::TracedReference<v8::Object>>>,
  detail: Cell<i32>,
}

unsafe impl v8::cppgc::GarbageCollected for UIEvent {
  fn trace(&self, visitor: &mut v8::cppgc::Visitor) {
    v8::cppgc::GarbageCollected::trace(&self.base, visitor);
    trace_optional(visitor, &self.view);
  }

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"UIEvent"
  }
}

fn ui_from_args(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
) -> Result<UIEvent, OpError> {
  let base = Event::from_args(scope, args, false)?;
  let init = util::init_arg(args);
  Ok(UIEvent {
    base,
    view: RefCell::new(read_view(scope, init)?),
    detail: Cell::new(idl_long(raw_number(scope, init, "detail"))),
  })
}

impl UIEvent {
  fn assign_ui(
    &self,
    detail: i32,
    view: Option<v8::TracedReference<v8::Object>>,
  ) {
    self.detail.set(detail);
    *self.view.borrow_mut() = view;
  }
}

#[op(inherit = Event)]
impl UIEvent {
  #[constructor]
  fn new(
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<UIEvent, OpError> {
    ui_from_args(scope, args)
  }

  #[construct_hook]
  fn finish(&self, scope: &mut v8::PinScope, object: v8::Local<v8::Object>) {
    let _ = self;
    event::define_is_trusted(scope, object);
  }

  #[getter]
  fn view(&self, scope: &mut v8::PinScope) -> Js {
    traced_object(scope, &self.view)
  }

  #[getter]
  fn detail(&self) -> i32 {
    self.detail.get()
  }
}

#[repr(C)]
pub struct FocusEvent {
  base: UIEvent,
  related: RefCell<Option<v8::TracedReference<v8::Object>>>,
}

unsafe impl v8::cppgc::GarbageCollected for FocusEvent {
  fn trace(&self, visitor: &mut v8::cppgc::Visitor) {
    v8::cppgc::GarbageCollected::trace(&self.base, visitor);
    trace_optional(visitor, &self.related);
  }

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"FocusEvent"
  }
}

#[op(inherit = UIEvent)]
impl FocusEvent {
  #[constructor]
  fn new(
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<FocusEvent, OpError> {
    let ui = ui_from_args(scope, args)?;
    let init = util::init_arg(args);
    Ok(FocusEvent {
      base: ui,
      related: RefCell::new(read_related(scope, init)?),
    })
  }

  #[construct_hook]
  fn finish(&self, scope: &mut v8::PinScope, object: v8::Local<v8::Object>) {
    let _ = self;
    event::define_is_trusted(scope, object);
  }

  #[getter]
  fn related_target(&self, scope: &mut v8::PinScope) -> Js {
    traced_object(scope, &self.related)
  }
}

#[repr(C)]
struct MouseState {
  screen_x: f64,
  screen_y: f64,
  client_x: f64,
  client_y: f64,
  movement_x: f64,
  movement_y: f64,
  button: i32,
  buttons: u16,
  modifiers: Modifiers,
}

#[repr(C)]
pub struct MouseEvent {
  base: UIEvent,
  state: RefCell<MouseState>,
  related: RefCell<Option<v8::TracedReference<v8::Object>>>,
}

unsafe impl v8::cppgc::GarbageCollected for MouseEvent {
  fn trace(&self, visitor: &mut v8::cppgc::Visitor) {
    v8::cppgc::GarbageCollected::trace(&self.base, visitor);
    trace_optional(visitor, &self.related);
  }

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"MouseEvent"
  }
}

impl MouseEvent {
  fn from_init(
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<MouseEvent, OpError> {
    let ui = ui_from_args(scope, args)?;
    let init = util::init_arg(args);
    Ok(MouseEvent {
      base: ui,
      state: RefCell::new(MouseState {
        screen_x: raw_number(scope, init, "screenX"),
        screen_y: raw_number(scope, init, "screenY"),
        client_x: raw_number(scope, init, "clientX"),
        client_y: raw_number(scope, init, "clientY"),
        movement_x: raw_number(scope, init, "movementX"),
        movement_y: raw_number(scope, init, "movementY"),
        button: idl_long(raw_number(scope, init, "button")),
        buttons: idl_unsigned(raw_number(scope, init, "buttons")) as u16,
        modifiers: read_modifiers(scope, init),
      }),
      related: RefCell::new(read_related(scope, init)?),
    })
  }
}

#[op(inherit = UIEvent)]
impl MouseEvent {
  #[constructor]
  #[length(1)]
  fn new(
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<MouseEvent, OpError> {
    MouseEvent::from_init(scope, args)
  }

  #[construct_hook]
  fn finish(&self, scope: &mut v8::PinScope, object: v8::Local<v8::Object>) {
    let _ = self;
    event::define_is_trusted(scope, object);
  }

  #[getter]
  fn screen_x(&self) -> f64 {
    floor_coord(self.state.borrow().screen_x)
  }

  #[getter]
  fn screen_y(&self) -> f64 {
    floor_coord(self.state.borrow().screen_y)
  }

  #[getter]
  fn client_x(&self) -> f64 {
    floor_coord(self.state.borrow().client_x)
  }

  #[getter]
  fn client_y(&self) -> f64 {
    floor_coord(self.state.borrow().client_y)
  }

  #[getter]
  fn page_x(&self) -> f64 {
    floor_coord(self.state.borrow().client_x)
  }

  #[getter]
  fn page_y(&self) -> f64 {
    floor_coord(self.state.borrow().client_y)
  }

  #[getter]
  fn offset_x(&self) -> f64 {
    floor_coord(self.state.borrow().client_x)
  }

  #[getter]
  fn offset_y(&self) -> f64 {
    floor_coord(self.state.borrow().client_y)
  }

  /// Same CSS pixels as `offsetX`. There is no stacking context to offset from.
  #[getter]
  fn layer_x(&self) -> f64 {
    floor_coord(self.state.borrow().client_x)
  }

  /// Same CSS pixels as `offsetY`.
  #[getter]
  fn layer_y(&self) -> f64 {
    floor_coord(self.state.borrow().client_y)
  }

  #[getter]
  fn movement_x(&self) -> f64 {
    self.state.borrow().movement_x
  }

  #[getter]
  fn movement_y(&self) -> f64 {
    self.state.borrow().movement_y
  }

  #[getter]
  fn button(&self) -> i32 {
    self.state.borrow().button
  }

  #[getter]
  fn buttons(&self) -> i32 {
    i32::from(self.state.borrow().buttons)
  }

  #[getter]
  fn ctrl_key(&self) -> bool {
    self.state.borrow().modifiers.ctrl
  }

  #[getter]
  fn shift_key(&self) -> bool {
    self.state.borrow().modifiers.shift
  }

  #[getter]
  fn alt_key(&self) -> bool {
    self.state.borrow().modifiers.alt
  }

  #[getter]
  fn meta_key(&self) -> bool {
    self.state.borrow().modifiers.meta
  }

  #[getter]
  fn related_target(&self, scope: &mut v8::PinScope) -> Js {
    traced_object(scope, &self.related)
  }

  fn get_modifier_state(&self, key: String) -> bool {
    let modifiers = self.state.borrow().modifiers;
    modifier_active(modifiers, &key)
  }

  /// Legacy initializer. Argument 10 is `altKey` and argument 11 is `shiftKey`.
  #[length(1)]
  fn init_mouse_event(
    &self,
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<(), OpError> {
    let type_name = util::try_string(scope, args.get(0))?;
    let bubbles = flag_arg(scope, args, 1);
    let cancelable = flag_arg(scope, args, 2);
    let view = view_arg(scope, args, 3)?;
    let detail = long_arg(scope, args, 4)?;
    let screen_x = double_arg(scope, args, 5)?;
    let screen_y = double_arg(scope, args, 6)?;
    let client_x = double_arg(scope, args, 7)?;
    let client_y = double_arg(scope, args, 8)?;
    let ctrl = flag_arg(scope, args, 9);
    let alt = flag_arg(scope, args, 10);
    let shift = flag_arg(scope, args, 11);
    let meta = flag_arg(scope, args, 12);
    let button = long_arg(scope, args, 13)?;
    let related = related_arg(scope, args, 14)?;
    if !self.base.base.init_type(type_name, bubbles, cancelable) {
      return Ok(());
    }
    let view = view.map(|object| traced(scope, object));
    self.base.assign_ui(detail, view);
    {
      let mut state = self.state.borrow_mut();
      state.screen_x = screen_x;
      state.screen_y = screen_y;
      state.client_x = client_x;
      state.client_y = client_y;
      state.button = button;
      state.modifiers.ctrl = ctrl;
      state.modifiers.alt = alt;
      state.modifiers.shift = shift;
      state.modifiers.meta = meta;
    }
    *self.related.borrow_mut() = related.map(|object| traced(scope, object));
    Ok(())
  }
}

#[repr(C)]
pub struct WheelEvent {
  base: MouseEvent,
  delta_x: f64,
  delta_y: f64,
  delta_z: f64,
  delta_mode: u32,
  momentum: bool,
}

unsafe impl v8::cppgc::GarbageCollected for WheelEvent {
  fn trace(&self, visitor: &mut v8::cppgc::Visitor) {
    v8::cppgc::GarbageCollected::trace(&self.base, visitor);
  }

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"WheelEvent"
  }
}

#[op(inherit = MouseEvent)]
impl WheelEvent {
  #[constructor]
  #[length(1)]
  fn new(
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<WheelEvent, OpError> {
    let base = MouseEvent::from_init(scope, args)?;
    let init = util::init_arg(args);
    Ok(WheelEvent {
      base,
      delta_x: raw_number(scope, init, "deltaX"),
      delta_y: raw_number(scope, init, "deltaY"),
      delta_z: raw_number(scope, init, "deltaZ"),
      delta_mode: idl_unsigned(raw_number(scope, init, "deltaMode")),
      momentum: util::dict_bool(scope, init, "momentum"),
    })
  }

  #[construct_hook]
  fn finish(&self, scope: &mut v8::PinScope, object: v8::Local<v8::Object>) {
    let _ = self;
    event::define_is_trusted(scope, object);
  }

  #[getter]
  fn delta_x(&self) -> f64 {
    self.delta_x
  }

  #[getter]
  fn delta_y(&self) -> f64 {
    self.delta_y
  }

  #[getter]
  fn delta_z(&self) -> f64 {
    self.delta_z
  }

  #[getter]
  fn delta_mode(&self) -> f64 {
    f64::from(self.delta_mode)
  }

  #[getter]
  fn momentum(&self) -> bool {
    self.momentum
  }
}

/// A constructed drag event. The runtime does not run an HTML drag session.
#[repr(C)]
pub struct DragEvent {
  base: MouseEvent,
  data_transfer: RefCell<Option<v8::TracedReference<v8::Object>>>,
}

unsafe impl v8::cppgc::GarbageCollected for DragEvent {
  fn trace(&self, visitor: &mut v8::cppgc::Visitor) {
    v8::cppgc::GarbageCollected::trace(&self.base, visitor);
    trace_optional(visitor, &self.data_transfer);
  }

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"DragEvent"
  }
}

#[op(inherit = MouseEvent)]
impl DragEvent {
  #[constructor]
  #[length(1)]
  fn new(
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<DragEvent, OpError> {
    let base = MouseEvent::from_init(scope, args)?;
    let init = util::init_arg(args);
    Ok(DragEvent {
      base,
      data_transfer: RefCell::new(read_data_transfer(scope, init)?),
    })
  }

  #[construct_hook]
  fn finish(&self, scope: &mut v8::PinScope, object: v8::Local<v8::Object>) {
    let _ = self;
    event::define_is_trusted(scope, object);
  }

  #[getter]
  fn data_transfer(&self, scope: &mut v8::PinScope) -> Js {
    traced_object(scope, &self.data_transfer)
  }
}

#[repr(C)]
pub struct KeyboardEvent {
  base: UIEvent,
  key: String,
  code: String,
  location: u32,
  repeat: bool,
  is_composing: bool,
  modifiers: Modifiers,
  char_code: u32,
  key_code: u32,
  which: u32,
}

unsafe impl v8::cppgc::GarbageCollected for KeyboardEvent {
  fn trace(&self, visitor: &mut v8::cppgc::Visitor) {
    v8::cppgc::GarbageCollected::trace(&self.base, visitor);
  }

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"KeyboardEvent"
  }
}

#[op(inherit = UIEvent)]
impl KeyboardEvent {
  #[constructor]
  fn new(
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<KeyboardEvent, OpError> {
    let base = ui_from_args(scope, args)?;
    let init = util::init_arg(args);
    Ok(KeyboardEvent {
      base,
      key: util::dict_string(scope, init, "key"),
      code: util::dict_string(scope, init, "code"),
      location: idl_unsigned(raw_number(scope, init, "location")),
      repeat: util::dict_bool(scope, init, "repeat"),
      is_composing: util::dict_bool(scope, init, "isComposing"),
      modifiers: read_modifiers(scope, init),
      char_code: idl_unsigned(raw_number(scope, init, "charCode")),
      key_code: idl_unsigned(raw_number(scope, init, "keyCode")),
      which: idl_unsigned(raw_number(scope, init, "which")),
    })
  }

  #[construct_hook]
  fn finish(&self, scope: &mut v8::PinScope, object: v8::Local<v8::Object>) {
    let _ = self;
    event::define_is_trusted(scope, object);
  }

  #[getter]
  fn key(&self) -> String {
    self.key.clone()
  }

  #[getter]
  fn code(&self) -> String {
    self.code.clone()
  }

  #[getter]
  fn location(&self) -> f64 {
    f64::from(self.location)
  }

  #[getter]
  fn repeat(&self) -> bool {
    self.repeat
  }

  #[getter]
  fn is_composing(&self) -> bool {
    self.is_composing
  }

  #[getter]
  fn ctrl_key(&self) -> bool {
    self.modifiers.ctrl
  }

  #[getter]
  fn shift_key(&self) -> bool {
    self.modifiers.shift
  }

  #[getter]
  fn alt_key(&self) -> bool {
    self.modifiers.alt
  }

  #[getter]
  fn meta_key(&self) -> bool {
    self.modifiers.meta
  }

  #[getter]
  fn char_code(&self) -> f64 {
    f64::from(self.char_code)
  }

  #[getter]
  fn key_code(&self) -> f64 {
    f64::from(self.key_code)
  }

  #[getter]
  fn which(&self) -> f64 {
    f64::from(self.which)
  }

  fn get_modifier_state(&self, key: String) -> bool {
    modifier_active(self.modifiers, &key)
  }
}

#[repr(C)]
pub struct CompositionEvent {
  base: UIEvent,
  data: String,
}

unsafe impl v8::cppgc::GarbageCollected for CompositionEvent {
  fn trace(&self, visitor: &mut v8::cppgc::Visitor) {
    v8::cppgc::GarbageCollected::trace(&self.base, visitor);
  }

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"CompositionEvent"
  }
}

#[op(inherit = UIEvent)]
impl CompositionEvent {
  #[constructor]
  fn new(
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<CompositionEvent, OpError> {
    let base = ui_from_args(scope, args)?;
    let init = util::init_arg(args);
    Ok(CompositionEvent {
      base,
      data: util::dict_string(scope, init, "data"),
    })
  }

  #[construct_hook]
  fn finish(&self, scope: &mut v8::PinScope, object: v8::Local<v8::Object>) {
    let _ = self;
    event::define_is_trusted(scope, object);
  }

  #[getter]
  fn data(&self) -> String {
    self.data.clone()
  }
}

#[repr(C)]
pub struct InputEvent {
  base: UIEvent,
  data: Option<String>,
  is_composing: Cell<bool>,
  input_type: String,
}

unsafe impl v8::cppgc::GarbageCollected for InputEvent {
  fn trace(&self, visitor: &mut v8::cppgc::Visitor) {
    v8::cppgc::GarbageCollected::trace(&self.base, visitor);
  }

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"InputEvent"
  }
}

fn input_data(
  scope: &mut v8::PinScope,
  init: Option<v8::Local<v8::Value>>,
) -> Option<String> {
  if !util::has_own(scope, init, "data") {
    return None;
  }
  let value = util::dict_get(scope, init, "data")?;
  if value.is_null() || value.is_undefined() {
    return None;
  }
  Some(util::string_value(scope, value))
}

#[op(inherit = UIEvent)]
impl InputEvent {
  #[constructor]
  fn new(
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<InputEvent, OpError> {
    let base = ui_from_args(scope, args)?;
    let init = util::init_arg(args);
    Ok(InputEvent {
      base,
      data: input_data(scope, init),
      is_composing: Cell::new(util::dict_bool(scope, init, "isComposing")),
      input_type: util::dict_string(scope, init, "inputType"),
    })
  }

  #[construct_hook]
  fn finish(&self, scope: &mut v8::PinScope, object: v8::Local<v8::Object>) {
    let _ = self;
    event::define_is_trusted(scope, object);
  }

  #[getter]
  fn data(&self, scope: &mut v8::PinScope) -> Js {
    match &self.data {
      Some(text) => {
        let Some(value) = v8::String::new(scope, text) else {
          return Js::null(scope);
        };
        let value: v8::Local<v8::Value> = value.into();
        Js(v8::Global::new(scope, value))
      }
      None => Js::null(scope),
    }
  }

  #[getter]
  fn is_composing(&self) -> bool {
    self.is_composing.get()
  }

  #[getter]
  fn input_type(&self) -> String {
    self.input_type.clone()
  }
}

#[repr(C)]
pub struct PointerEvent {
  base: MouseEvent,
  pointer_id: i32,
  width: f64,
  height: f64,
  pressure: f64,
  tangential: f64,
  tilt_x: i32,
  tilt_y: i32,
  twist: i32,
  azimuth: f64,
  altitude: f64,
  pointer_type: String,
  is_primary: bool,
  persistent_device_id: i32,
  coalesced: RefCell<Vec<v8::TracedReference<v8::Object>>>,
  predicted: RefCell<Vec<v8::TracedReference<v8::Object>>>,
}

unsafe impl v8::cppgc::GarbageCollected for PointerEvent {
  fn trace(&self, visitor: &mut v8::cppgc::Visitor) {
    v8::cppgc::GarbageCollected::trace(&self.base, visitor);
    trace_list(visitor, &self.coalesced);
    trace_list(visitor, &self.predicted);
  }

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"PointerEvent"
  }
}

impl PointerEvent {
  fn expose(&self, value: f64) -> f64 {
    let name = self.base.base.base.event_name();
    if matches!(name.as_str(), "click" | "auxclick" | "contextmenu") {
      floor_coord(value)
    } else {
      value
    }
  }
}

fn read_pointer_list(
  scope: &mut v8::PinScope,
  init: Option<v8::Local<v8::Value>>,
  name: &str,
) -> Result<Vec<v8::TracedReference<v8::Object>>, OpError> {
  if !util::has_own(scope, init, name) {
    return Ok(Vec::new());
  }
  let Some(value) = util::dict_get(scope, init, name) else {
    return Ok(Vec::new());
  };
  if value.is_null() || value.is_undefined() {
    return Ok(Vec::new());
  }
  let Ok(array) = v8::Local::<v8::Array>::try_from(value) else {
    return Err(OpError::type_("PointerEvent event list must be an array"));
  };
  let mut events = Vec::new();
  for index in 0..array.length() {
    let Some(item) = array.get_index(scope, index) else {
      continue;
    };
    let Ok(object) = v8::Local::<v8::Object>::try_from(item) else {
      return Err(OpError::type_(
        "PointerEvent event list must contain PointerEvent objects",
      ));
    };
    if unsafe { host_ref::<PointerEvent>(scope, object) }.is_none() {
      return Err(OpError::type_(
        "PointerEvent event list must contain PointerEvent objects",
      ));
    }
    events.push(traced(scope, object));
  }
  Ok(events)
}

#[op(inherit = MouseEvent)]
impl PointerEvent {
  #[constructor]
  #[length(1)]
  fn new(
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<PointerEvent, OpError> {
    let base = MouseEvent::from_init(scope, args)?;
    let init = util::init_arg(args);
    let orientation = orientation_from_init(scope, init);
    Ok(PointerEvent {
      base,
      pointer_id: idl_long(raw_number(scope, init, "pointerId")),
      width: number_or(scope, init, "width", 1.0),
      height: number_or(scope, init, "height", 1.0),
      pressure: number_or(scope, init, "pressure", 0.0),
      tangential: number_or(scope, init, "tangentialPressure", 0.0),
      tilt_x: orientation.tilt_x,
      tilt_y: orientation.tilt_y,
      twist: idl_long(raw_number(scope, init, "twist")),
      azimuth: orientation.azimuth,
      altitude: orientation.altitude,
      pointer_type: util::dict_string(scope, init, "pointerType"),
      is_primary: util::dict_bool(scope, init, "isPrimary"),
      persistent_device_id: idl_long(raw_number(
        scope,
        init,
        "persistentDeviceId",
      )),
      coalesced: RefCell::new(read_pointer_list(
        scope,
        init,
        "coalescedEvents",
      )?),
      predicted: RefCell::new(read_pointer_list(
        scope,
        init,
        "predictedEvents",
      )?),
    })
  }

  #[construct_hook]
  fn finish(&self, scope: &mut v8::PinScope, object: v8::Local<v8::Object>) {
    let _ = self;
    event::define_is_trusted(scope, object);
  }

  #[getter]
  fn screen_x(&self) -> f64 {
    self.expose(self.base.state.borrow().screen_x)
  }

  #[getter]
  fn screen_y(&self) -> f64 {
    self.expose(self.base.state.borrow().screen_y)
  }

  #[getter]
  fn client_x(&self) -> f64 {
    self.expose(self.base.state.borrow().client_x)
  }

  #[getter]
  fn client_y(&self) -> f64 {
    self.expose(self.base.state.borrow().client_y)
  }

  #[getter]
  fn page_x(&self) -> f64 {
    self.expose(self.base.state.borrow().client_x)
  }

  #[getter]
  fn page_y(&self) -> f64 {
    self.expose(self.base.state.borrow().client_y)
  }

  #[getter]
  fn offset_x(&self) -> f64 {
    self.expose(self.base.state.borrow().client_x)
  }

  #[getter]
  fn offset_y(&self) -> f64 {
    self.expose(self.base.state.borrow().client_y)
  }

  #[getter]
  fn pointer_id(&self) -> i32 {
    self.pointer_id
  }

  #[getter]
  fn width(&self) -> f64 {
    self.width
  }

  #[getter]
  fn height(&self) -> f64 {
    self.height
  }

  #[getter]
  fn pressure(&self) -> f64 {
    self.pressure
  }

  #[getter]
  fn tangential_pressure(&self) -> f64 {
    self.tangential
  }

  #[getter]
  fn tilt_x(&self) -> i32 {
    self.tilt_x
  }

  #[getter]
  fn tilt_y(&self) -> i32 {
    self.tilt_y
  }

  #[getter]
  fn twist(&self) -> i32 {
    self.twist
  }

  #[getter]
  fn azimuth_angle(&self) -> f64 {
    self.azimuth
  }

  #[getter]
  fn altitude_angle(&self) -> f64 {
    self.altitude
  }

  #[getter]
  fn pointer_type(&self) -> String {
    self.pointer_type.clone()
  }

  #[getter]
  fn is_primary(&self) -> bool {
    self.is_primary
  }

  #[getter]
  fn persistent_device_id(&self) -> i32 {
    self.persistent_device_id
  }

  fn get_coalesced_events(&self, scope: &mut v8::PinScope) -> Js {
    event_list(scope, &self.coalesced)
  }

  fn get_predicted_events(&self, scope: &mut v8::PinScope) -> Js {
    event_list(scope, &self.predicted)
  }
}

/// A trusted pointer or compatibility mouse event built by the window crate.
pub struct FiredPointer {
  pub type_name: String,
  pub bubbles: bool,
  pub cancelable: bool,
  pub composed: bool,
  pub detail: i32,
  pub screen_x: f64,
  pub screen_y: f64,
  pub client_x: f64,
  pub client_y: f64,
  pub movement_x: f64,
  pub movement_y: f64,
  pub button: i32,
  pub buttons: u16,
  pub ctrl: bool,
  pub shift: bool,
  pub alt: bool,
  pub meta: bool,
  pub pointer_id: i32,
  pub width: f64,
  pub height: f64,
  pub pressure: f64,
  pub tangential: f64,
  pub tilt_x: i32,
  pub tilt_y: i32,
  pub twist: i32,
  pub pointer_type: String,
  pub is_primary: bool,
  pub coalesced: Vec<FiredPointer>,
}

impl FiredPointer {
  pub fn new(type_name: impl Into<String>) -> Self {
    Self {
      type_name: type_name.into(),
      bubbles: true,
      cancelable: true,
      composed: true,
      detail: 0,
      screen_x: 0.0,
      screen_y: 0.0,
      client_x: 0.0,
      client_y: 0.0,
      movement_x: 0.0,
      movement_y: 0.0,
      button: 0,
      buttons: 0,
      ctrl: false,
      shift: false,
      alt: false,
      meta: false,
      pointer_id: 1,
      width: 1.0,
      height: 1.0,
      pressure: 0.0,
      tangential: 0.0,
      tilt_x: 0,
      tilt_y: 0,
      twist: 0,
      pointer_type: "mouse".to_string(),
      is_primary: true,
      coalesced: Vec::new(),
    }
  }
}

fn mouse_state_from(fired: &FiredPointer) -> MouseState {
  MouseState {
    screen_x: fired.screen_x,
    screen_y: fired.screen_y,
    client_x: fired.client_x,
    client_y: fired.client_y,
    movement_x: fired.movement_x,
    movement_y: fired.movement_y,
    button: fired.button,
    buttons: fired.buttons,
    modifiers: Modifiers {
      ctrl: fired.ctrl,
      shift: fired.shift,
      alt: fired.alt,
      meta: fired.meta,
      alt_graph: false,
      caps_lock: false,
    },
  }
}

fn pointer_from_fired(
  scope: &mut v8::PinScope,
  fired: &FiredPointer,
  child: bool,
) -> PointerEvent {
  let mut coalesced = Vec::new();
  if !child {
    for item in &fired.coalesced {
      coalesced.push(seal_pointer(scope, item, true));
    }
  }
  let (azimuth, altitude) = tilt_to_spherical(fired.tilt_x, fired.tilt_y);
  PointerEvent {
    base: MouseEvent {
      base: UIEvent {
        base: Event::new(
          fired.type_name.clone(),
          fired.bubbles,
          fired.cancelable,
          fired.composed,
          true,
        ),
        view: RefCell::new(window_view(scope)),
        detail: Cell::new(fired.detail),
      },
      state: RefCell::new(mouse_state_from(fired)),
      related: RefCell::new(None),
    },
    pointer_id: fired.pointer_id,
    width: fired.width,
    height: fired.height,
    pressure: fired.pressure,
    tangential: fired.tangential,
    tilt_x: fired.tilt_x,
    tilt_y: fired.tilt_y,
    twist: fired.twist,
    azimuth,
    altitude,
    pointer_type: fired.pointer_type.clone(),
    is_primary: fired.is_primary,
    persistent_device_id: 0,
    coalesced: RefCell::new(coalesced),
    predicted: RefCell::new(Vec::new()),
  }
}

fn seal_pointer(
  scope: &mut v8::PinScope,
  fired: &FiredPointer,
  child: bool,
) -> v8::TracedReference<v8::Object> {
  let event = pointer_from_fired(scope, fired, child);
  // The class template was cached when the constructor was installed.
  let object = unsafe { yuzuko_core::op::wrap_instance(scope, event) };
  event::define_is_trusted(scope, object);
  traced(scope, object)
}

fn dispatch_sealed(
  scope: &mut v8::PinScope,
  target: v8::Local<v8::Object>,
  event: v8::Local<v8::Object>,
) -> bool {
  event::define_is_trusted(scope, event);
  target::dispatch(scope, target, event).unwrap_or(false)
}

/// Dispatches `fired` at `target`. Returns whether the default action may run.
pub fn dispatch_pointer(
  scope: &mut v8::PinScope,
  target: v8::Local<v8::Object>,
  fired: &FiredPointer,
) -> bool {
  dispatch_pointer_through(scope, target, &[], fired)
}

/// Dispatches `fired` at `target`, then through `ancestors` from parent to root.
pub fn dispatch_pointer_through(
  scope: &mut v8::PinScope,
  target: v8::Local<v8::Object>,
  ancestors: &[v8::Local<v8::Object>],
  fired: &FiredPointer,
) -> bool {
  let event = pointer_from_fired(scope, fired, false);
  let object = unsafe { yuzuko_core::op::wrap_instance(scope, event) };
  event::define_is_trusted(scope, object);
  let mut path = Vec::with_capacity(1 + ancestors.len());
  path.push(target);
  path.extend_from_slice(ancestors);
  target::dispatch_path(scope, &path, object).unwrap_or(false)
}

pub struct FiredKey {
  pub type_name: String,
  pub key: String,
  pub code: String,
  pub location: u32,
  pub repeat: bool,
  pub is_composing: bool,
  pub ctrl: bool,
  pub shift: bool,
  pub alt: bool,
  pub meta: bool,
}

pub fn dispatch_key(
  scope: &mut v8::PinScope,
  target: v8::Local<v8::Object>,
  fired: &FiredKey,
) -> bool {
  let event = KeyboardEvent {
    base: UIEvent {
      base: Event::new(fired.type_name.clone(), true, true, true, true),
      view: RefCell::new(window_view(scope)),
      detail: Cell::new(0),
    },
    key: fired.key.clone(),
    code: fired.code.clone(),
    location: fired.location,
    repeat: fired.repeat,
    is_composing: fired.is_composing,
    modifiers: Modifiers {
      ctrl: fired.ctrl,
      shift: fired.shift,
      alt: fired.alt,
      meta: fired.meta,
      alt_graph: false,
      caps_lock: false,
    },
    char_code: 0,
    key_code: 0,
    which: 0,
  };
  let object = unsafe { yuzuko_core::op::wrap_instance(scope, event) };
  dispatch_sealed(scope, target, object)
}

pub struct FiredComposition {
  pub type_name: String,
  pub data: String,
}

pub fn dispatch_composition(
  scope: &mut v8::PinScope,
  target: v8::Local<v8::Object>,
  fired: &FiredComposition,
) -> bool {
  let cancelable = fired.type_name != "compositionend";
  let event = CompositionEvent {
    base: UIEvent {
      base: Event::new(fired.type_name.clone(), true, cancelable, true, true),
      view: RefCell::new(window_view(scope)),
      detail: Cell::new(0),
    },
    data: fired.data.clone(),
  };
  let object = unsafe { yuzuko_core::op::wrap_instance(scope, event) };
  dispatch_sealed(scope, target, object)
}

pub struct FiredInput {
  pub input_type: String,
  pub data: String,
  pub is_composing: bool,
}

pub fn dispatch_input(
  scope: &mut v8::PinScope,
  target: v8::Local<v8::Object>,
  fired: &FiredInput,
) -> bool {
  let event = InputEvent {
    base: UIEvent {
      base: Event::new("beforeinput", true, true, true, true),
      view: RefCell::new(window_view(scope)),
      detail: Cell::new(0),
    },
    data: Some(fired.data.clone()),
    is_composing: Cell::new(fired.is_composing),
    input_type: fired.input_type.clone(),
  };
  let object = unsafe { yuzuko_core::op::wrap_instance(scope, event) };
  dispatch_sealed(scope, target, object)
}

pub struct FiredWheel {
  pub screen_x: f64,
  pub screen_y: f64,
  pub client_x: f64,
  pub client_y: f64,
  pub delta_x: f64,
  pub delta_y: f64,
  pub delta_z: f64,
  pub delta_mode: u32,
  pub ctrl: bool,
  pub shift: bool,
  pub alt: bool,
  pub meta: bool,
}

pub fn dispatch_wheel(
  scope: &mut v8::PinScope,
  target: v8::Local<v8::Object>,
  fired: &FiredWheel,
) -> bool {
  let event = WheelEvent {
    base: MouseEvent {
      base: UIEvent {
        base: Event::new("wheel", true, true, true, true),
        view: RefCell::new(window_view(scope)),
        detail: Cell::new(0),
      },
      state: RefCell::new(MouseState {
        screen_x: fired.screen_x,
        screen_y: fired.screen_y,
        client_x: fired.client_x,
        client_y: fired.client_y,
        movement_x: 0.0,
        movement_y: 0.0,
        button: 0,
        buttons: 0,
        modifiers: Modifiers {
          ctrl: fired.ctrl,
          shift: fired.shift,
          alt: fired.alt,
          meta: fired.meta,
          alt_graph: false,
          caps_lock: false,
        },
      }),
      related: RefCell::new(None),
    },
    delta_x: fired.delta_x,
    delta_y: fired.delta_y,
    delta_z: fired.delta_z,
    delta_mode: fired.delta_mode,
    momentum: false,
  };
  let object = unsafe { yuzuko_core::op::wrap_instance(scope, event) };
  dispatch_sealed(scope, target, object)
}

pub fn dispatch_simple(
  scope: &mut v8::PinScope,
  target: v8::Local<v8::Object>,
  type_name: &str,
  bubbles: bool,
  cancelable: bool,
) -> bool {
  let event = Event::new(type_name, bubbles, cancelable, false, true);
  let object = unsafe { yuzuko_core::op::wrap_instance(scope, event) };
  dispatch_sealed(scope, target, object)
}

/// `focus` and `blur` on a `Yuzuko.Window`. `relatedTarget` stays null.
pub fn dispatch_focus(
  scope: &mut v8::PinScope,
  target: v8::Local<v8::Object>,
  type_name: &str,
) -> bool {
  let event = FocusEvent {
    base: UIEvent {
      base: Event::new(type_name, false, false, true, true),
      view: RefCell::new(window_view(scope)),
      detail: Cell::new(0),
    },
    related: RefCell::new(None),
  };
  let object = unsafe { yuzuko_core::op::wrap_instance(scope, event) };
  dispatch_sealed(scope, target, object)
}

/// One contact point in a `TouchEvent`.
#[repr(C)]
pub struct Touch {
  identifier: i32,
  target: v8::TracedReference<v8::Object>,
  screen_x: f64,
  screen_y: f64,
  client_x: f64,
  client_y: f64,
  page_x: f64,
  page_y: f64,
  radius_x: f64,
  radius_y: f64,
  rotation_angle: f64,
  force: f64,
}

unsafe impl v8::cppgc::GarbageCollected for Touch {
  fn trace(&self, visitor: &mut v8::cppgc::Visitor) {
    visitor.trace(&self.target);
  }

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"Touch"
  }
}

#[op]
impl Touch {
  #[constructor]
  #[length(1)]
  fn new(
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<Touch, OpError> {
    let init = touch_init(args)?;
    let identifier = require_dict(scope, init, "identifier")?;
    let identifier = idl_long(f64::from_v8(scope, identifier)?);
    let target = require_dict(scope, init, "target")?;
    let target = event_target(scope, target)?;
    let target = traced(scope, target);
    let screen_x = dict_unrestricted(scope, init, "screenX")?;
    let screen_y = dict_unrestricted(scope, init, "screenY")?;
    let client_x = dict_unrestricted(scope, init, "clientX")?;
    let client_y = dict_unrestricted(scope, init, "clientY")?;
    let page_x = dict_unrestricted(scope, init, "pageX")?;
    let page_y = dict_unrestricted(scope, init, "pageY")?;
    let radius_x = dict_unrestricted(scope, init, "radiusX")?;
    let radius_y = dict_unrestricted(scope, init, "radiusY")?;
    let rotation_angle = dict_unrestricted(scope, init, "rotationAngle")?;
    let force = dict_unrestricted(scope, init, "force")?;
    Ok(Touch {
      identifier,
      target,
      screen_x,
      screen_y,
      client_x,
      client_y,
      page_x,
      page_y,
      radius_x,
      radius_y,
      rotation_angle,
      force,
    })
  }

  #[getter]
  fn identifier(&self) -> i32 {
    self.identifier
  }

  #[getter]
  fn target(&self, scope: &mut v8::PinScope) -> Js {
    list_value(scope, &self.target)
  }

  #[getter]
  fn screen_x(&self) -> f64 {
    self.screen_x
  }

  #[getter]
  fn screen_y(&self) -> f64 {
    self.screen_y
  }

  #[getter]
  fn client_x(&self) -> f64 {
    self.client_x
  }

  #[getter]
  fn client_y(&self) -> f64 {
    self.client_y
  }

  #[getter]
  fn page_x(&self) -> f64 {
    self.page_x
  }

  #[getter]
  fn page_y(&self) -> f64 {
    self.page_y
  }

  #[getter]
  fn radius_x(&self) -> f64 {
    self.radius_x
  }

  #[getter]
  fn radius_y(&self) -> f64 {
    self.radius_y
  }

  #[getter]
  fn rotation_angle(&self) -> f64 {
    self.rotation_angle
  }

  #[getter]
  fn force(&self) -> f64 {
    self.force
  }
}

/// Immutable list of `Touch` objects. `new TouchList()` throws.
#[repr(C)]
pub struct TouchList {
  touches: RefCell<Vec<v8::TracedReference<v8::Object>>>,
}

unsafe impl v8::cppgc::GarbageCollected for TouchList {
  fn trace(&self, visitor: &mut v8::cppgc::Visitor) {
    trace_list(visitor, &self.touches);
  }

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"TouchList"
  }
}

#[op]
impl TouchList {
  #[getter]
  fn length(&self) -> u32 {
    self.touches.borrow().len() as u32
  }

  fn item(&self, scope: &mut v8::PinScope, index: u32) -> Js {
    let local = {
      let list = self.touches.borrow();
      let Some(reference) = list.get(index as usize) else {
        return Js::null(scope);
      };
      traced_local(scope, reference)
    };
    match local {
      Some(object) => {
        let value: v8::Local<v8::Value> = object.into();
        Js(v8::Global::new(scope, value))
      }
      None => Js::null(scope),
    }
  }
}

#[repr(C)]
pub struct TouchEvent {
  base: UIEvent,
  touches: v8::TracedReference<v8::Object>,
  target_touches: v8::TracedReference<v8::Object>,
  changed_touches: v8::TracedReference<v8::Object>,
  modifiers: Modifiers,
}

unsafe impl v8::cppgc::GarbageCollected for TouchEvent {
  fn trace(&self, visitor: &mut v8::cppgc::Visitor) {
    v8::cppgc::GarbageCollected::trace(&self.base, visitor);
    visitor.trace(&self.touches);
    visitor.trace(&self.target_touches);
    visitor.trace(&self.changed_touches);
  }

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"TouchEvent"
  }
}

#[op(inherit = UIEvent)]
impl TouchEvent {
  #[constructor]
  #[length(1)]
  fn new(
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<TouchEvent, OpError> {
    let base = ui_from_args(scope, args)?;
    let init = util::init_arg(args);
    let touches = make_touch_list(scope, init, "touches")?;
    let touches = traced(scope, touches);
    let target_touches = make_touch_list(scope, init, "targetTouches")?;
    let target_touches = traced(scope, target_touches);
    let changed_touches = make_touch_list(scope, init, "changedTouches")?;
    let changed_touches = traced(scope, changed_touches);
    let modifiers = read_modifiers(scope, init);
    Ok(TouchEvent {
      base,
      touches,
      target_touches,
      changed_touches,
      modifiers,
    })
  }

  #[construct_hook]
  fn finish(&self, scope: &mut v8::PinScope, object: v8::Local<v8::Object>) {
    let _ = self;
    event::define_is_trusted(scope, object);
  }

  #[getter]
  fn touches(&self, scope: &mut v8::PinScope) -> Js {
    list_value(scope, &self.touches)
  }

  #[getter]
  fn target_touches(&self, scope: &mut v8::PinScope) -> Js {
    list_value(scope, &self.target_touches)
  }

  #[getter]
  fn changed_touches(&self, scope: &mut v8::PinScope) -> Js {
    list_value(scope, &self.changed_touches)
  }

  #[getter]
  fn ctrl_key(&self) -> bool {
    self.modifiers.ctrl
  }

  #[getter]
  fn shift_key(&self) -> bool {
    self.modifiers.shift
  }

  #[getter]
  fn alt_key(&self) -> bool {
    self.modifiers.alt
  }

  #[getter]
  fn meta_key(&self) -> bool {
    self.modifiers.meta
  }
}

fn list_value(
  scope: &mut v8::PinScope,
  reference: &v8::TracedReference<v8::Object>,
) -> Js {
  match traced_local(scope, reference) {
    Some(object) => {
      let value: v8::Local<v8::Value> = object.into();
      Js(v8::Global::new(scope, value))
    }
    None => Js::null(scope),
  }
}

fn touch_init<'s>(
  args: &v8::FunctionCallbackArguments<'s>,
) -> Result<v8::Local<'s, v8::Object>, OpError> {
  if args.length() < 1 || args.get(0).is_null_or_undefined() {
    return Err(OpError::type_("TouchInit.identifier is required"));
  }
  let Ok(init) = v8::Local::<v8::Object>::try_from(args.get(0)) else {
    return Err(OpError::type_(
      "The provided value is not of type 'TouchInit'.",
    ));
  };
  Ok(init)
}

fn require_dict<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  init: v8::Local<'_, v8::Object>,
  name: &str,
) -> Result<v8::Local<'s, v8::Value>, OpError> {
  let value = get_property(scope, init, name)?;
  if value.is_undefined() {
    return Err(OpError::type_(format!("TouchInit.{name} is required")));
  }
  Ok(value)
}

fn dict_unrestricted(
  scope: &mut v8::PinScope,
  init: v8::Local<v8::Object>,
  name: &str,
) -> Result<f64, OpError> {
  let value = get_property(scope, init, name)?;
  if value.is_undefined() {
    return Ok(0.0);
  }
  f64::from_v8(scope, value)
}

fn event_target<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  value: v8::Local<'_, v8::Value>,
) -> Result<v8::Local<'s, v8::Object>, OpError> {
  let Ok(object) = v8::Local::<v8::Object>::try_from(value) else {
    return Err(OpError::type_(
      "The provided value is not of type 'EventTarget'.",
    ));
  };
  let global = scope.get_current_context().global(scope);
  let is_target = global
    .get(scope, key(scope, "EventTarget").into())
    .and_then(|value| v8::Local::<v8::Function>::try_from(value).ok())
    .and_then(|constructor| object.instance_of(scope, constructor.into()))
    .unwrap_or(false);
  if !is_target {
    return Err(OpError::type_(
      "The provided value is not of type 'EventTarget'.",
    ));
  }
  Ok(yuzuko_core::op::root(scope, object))
}

fn make_touch_list<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  init: Option<v8::Local<'_, v8::Value>>,
  name: &str,
) -> Result<v8::Local<'s, v8::Object>, OpError> {
  let touches = read_touch_sequence(scope, init, name)?;
  let list = TouchList {
    touches: RefCell::new(touches),
  };
  Ok(unsafe { yuzuko_core::op::wrap_instance(scope, list) })
}

fn read_touch_sequence(
  scope: &mut v8::PinScope,
  init: Option<v8::Local<v8::Value>>,
  name: &str,
) -> Result<Vec<v8::TracedReference<v8::Object>>, OpError> {
  let Some(value) = util::dict_get(scope, init, name) else {
    return Ok(Vec::new());
  };
  if value.is_undefined() {
    return Ok(Vec::new());
  }
  let Ok(iterable) = v8::Local::<v8::Object>::try_from(value) else {
    return Err(OpError::type_(
      "The provided value cannot be converted to a sequence.",
    ));
  };
  let symbol = v8::Symbol::get_iterator(scope);
  let method = get_keyed(scope, iterable, symbol.into())?;
  let Ok(method) = v8::Local::<v8::Function>::try_from(method) else {
    return Err(OpError::type_(
      "The provided value cannot be converted to a sequence.",
    ));
  };
  let iterator = invoke(scope, method, iterable.into(), &[])?;
  let Ok(iterator) = v8::Local::<v8::Object>::try_from(iterator) else {
    return Err(OpError::type_(
      "The provided value cannot be converted to a sequence.",
    ));
  };
  let mut touches = Vec::new();
  loop {
    let next = get_property(scope, iterator, "next")?;
    let Ok(next) = v8::Local::<v8::Function>::try_from(next) else {
      return Err(OpError::type_(
        "The provided value cannot be converted to a sequence.",
      ));
    };
    let step = invoke(scope, next, iterator.into(), &[])?;
    let Ok(step) = v8::Local::<v8::Object>::try_from(step) else {
      return Err(OpError::type_(
        "The provided value cannot be converted to a sequence.",
      ));
    };
    let done = get_property(scope, step, "done")?;
    if done.boolean_value(scope) {
      return Ok(touches);
    }
    let item = get_property(scope, step, "value")?;
    let Ok(object) = v8::Local::<v8::Object>::try_from(item) else {
      return Err(OpError::type_("The provided value is not of type 'Touch'."));
    };
    if unsafe { host_ref::<Touch>(scope, object) }.is_none() {
      return Err(OpError::type_("The provided value is not of type 'Touch'."));
    }
    touches.push(traced(scope, object));
  }
}

fn get_property<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  object: v8::Local<'_, v8::Object>,
  name: &str,
) -> Result<v8::Local<'s, v8::Value>, OpError> {
  let key = key(scope, name);
  get_keyed(scope, object, key.into())
}

fn get_keyed<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  object: v8::Local<'_, v8::Object>,
  key: v8::Local<'_, v8::Value>,
) -> Result<v8::Local<'s, v8::Value>, OpError> {
  let captured = {
    v8::tc_scope!(let tc, scope);
    let object = yuzuko_core::op::root(tc, object);
    let key = yuzuko_core::op::root(tc, key);
    match object.get(tc, key) {
      Some(value) => Ok(v8::Global::new(tc, value)),
      None => {
        let exception =
          tc.exception().unwrap_or_else(|| v8::undefined(tc).into());
        Err(v8::Global::new(tc, exception))
      }
    }
  };
  match captured {
    Ok(value) => Ok(v8::Local::new(scope, value)),
    Err(exception) => {
      Err(OpError::thrown(scope, v8::Local::new(scope, exception)))
    }
  }
}

fn invoke<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  function: v8::Local<'_, v8::Function>,
  receiver: v8::Local<'_, v8::Value>,
  args: &[v8::Local<'_, v8::Value>],
) -> Result<v8::Local<'s, v8::Value>, OpError> {
  let captured = {
    v8::tc_scope!(let tc, scope);
    let function = yuzuko_core::op::root(tc, function);
    let receiver = yuzuko_core::op::root(tc, receiver);
    let args: Vec<v8::Local<v8::Value>> = args
      .iter()
      .copied()
      .map(|arg| yuzuko_core::op::root(tc, arg))
      .collect();
    match function.call(tc, receiver, &args) {
      Some(value) => Ok(v8::Global::new(tc, value)),
      None => {
        let exception =
          tc.exception().unwrap_or_else(|| v8::undefined(tc).into());
        Err(v8::Global::new(tc, exception))
      }
    }
  };
  match captured {
    Ok(value) => Ok(v8::Local::new(scope, value)),
    Err(exception) => {
      Err(OpError::thrown(scope, v8::Local::new(scope, exception)))
    }
  }
}

fn touch_list_count(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
) -> u32 {
  unsafe { host_ref::<TouchList>(scope, object) }
    .map(|host| host.get().touches.borrow().len() as u32)
    .unwrap_or(0)
}

fn touch_list_at<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  object: v8::Local<'_, v8::Object>,
  index: u32,
) -> Option<v8::Local<'s, v8::Object>> {
  let host = unsafe { host_ref::<TouchList>(scope, object) }?;
  let list = host.get().touches.borrow();
  let reference = list.get(index as usize)?;
  traced_local(scope, reference)
}

fn touch_list_get_index(
  scope: &mut v8::PinScope,
  index: u32,
  args: v8::PropertyCallbackArguments,
  mut ret: v8::ReturnValue<v8::Value>,
) -> v8::Intercepted {
  let Some(touch) = touch_list_at(scope, args.holder(), index) else {
    return v8::Intercepted::kNo;
  };
  ret.set(touch.into());
  v8::Intercepted::kYes
}

fn touch_list_query_index(
  scope: &mut v8::PinScope,
  index: u32,
  args: v8::PropertyCallbackArguments,
  mut ret: v8::ReturnValue<v8::Integer>,
) -> v8::Intercepted {
  if index >= touch_list_count(scope, args.holder()) {
    return v8::Intercepted::kNo;
  }
  // `PropertyAttribute::READ_ONLY`: enumerable and configurable, not writable.
  ret.set_int32(1);
  v8::Intercepted::kYes
}

fn touch_list_reject_index(
  scope: &mut v8::PinScope,
  index: u32,
  args: v8::PropertyCallbackArguments,
  mut ret: v8::ReturnValue<v8::Boolean>,
) -> v8::Intercepted {
  if index >= touch_list_count(scope, args.holder()) {
    return v8::Intercepted::kNo;
  }
  ret.set_bool(false);
  v8::Intercepted::kYes
}

fn touch_list_set_index(
  scope: &mut v8::PinScope,
  index: u32,
  _value: v8::Local<v8::Value>,
  args: v8::PropertyCallbackArguments,
  ret: v8::ReturnValue<v8::Boolean>,
) -> v8::Intercepted {
  touch_list_reject_index(scope, index, args, ret)
}

fn touch_list_delete_index(
  scope: &mut v8::PinScope,
  index: u32,
  args: v8::PropertyCallbackArguments,
  ret: v8::ReturnValue<v8::Boolean>,
) -> v8::Intercepted {
  touch_list_reject_index(scope, index, args, ret)
}

fn touch_list_enumerate(
  scope: &mut v8::PinScope,
  args: v8::PropertyCallbackArguments,
  mut ret: v8::ReturnValue<v8::Array>,
) {
  let count = touch_list_count(scope, args.holder());
  let indices: Vec<v8::Local<v8::Value>> = (0..count)
    .map(|index| v8::Integer::new_from_unsigned(scope, index).into())
    .collect();
  ret.set(v8::Array::new_with_elements(scope, &indices));
}

fn install_touch_list_indices(scope: &mut v8::PinScope) {
  // `GetFunction` instantiates the template. `SetHandler` after that is a V8
  // fatal, and later realms reuse the isolate-cached template.
  if yuzuko_core::op::cached_template::<TouchList>(scope).is_some() {
    return;
  }
  let templ = TouchList::template(scope);
  templ.instance_template(scope).set_indexed_property_handler(
    v8::IndexedPropertyHandlerConfiguration::new()
      .getter(touch_list_get_index)
      .query(touch_list_query_index)
      .setter(touch_list_set_index)
      .deleter(touch_list_delete_index)
      .enumerator(touch_list_enumerate)
      .flags(v8::PropertyHandlerFlags::HAS_NO_SIDE_EFFECT),
  );
}

fn flag_arg(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
  index: i32,
) -> bool {
  present_arg(args, index).is_some_and(|value| value.boolean_value(scope))
}

fn double_arg(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
  index: i32,
) -> Result<f64, OpError> {
  let Some(value) = present_arg(args, index) else {
    return Ok(0.0);
  };
  f64::from_v8(scope, value)
}

fn long_arg(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
  index: i32,
) -> Result<i32, OpError> {
  Ok(idl_long(double_arg(scope, args, index)?))
}

fn present_arg<'s>(
  args: &v8::FunctionCallbackArguments<'s>,
  index: i32,
) -> Option<v8::Local<'s, v8::Value>> {
  if index < 0 || index >= args.length() {
    return None;
  }
  let value = args.get(index);
  if value.is_undefined() {
    None
  } else {
    Some(value)
  }
}

fn view_arg<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  args: &v8::FunctionCallbackArguments<'_>,
  index: i32,
) -> Result<Option<v8::Local<'s, v8::Object>>, OpError> {
  let Some(value) = present_arg(args, index) else {
    return Ok(None);
  };
  if value.is_null() {
    return Ok(None);
  }
  let Ok(object) = v8::Local::<v8::Object>::try_from(value) else {
    return Err(OpError::type_(
      "The provided value is not of type 'Window'.",
    ));
  };
  if !util::is_global(scope, object) {
    return Err(OpError::type_(
      "The provided value is not of type 'Window'.",
    ));
  }
  Ok(Some(yuzuko_core::op::root(scope, object)))
}

fn related_arg<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  args: &v8::FunctionCallbackArguments<'_>,
  index: i32,
) -> Result<Option<v8::Local<'s, v8::Object>>, OpError> {
  let Some(value) = present_arg(args, index) else {
    return Ok(None);
  };
  if value.is_null() {
    return Ok(None);
  }
  let Ok(object) = v8::Local::<v8::Object>::try_from(value) else {
    return Err(OpError::type_(
      "The provided value is not of type 'EventTarget'.",
    ));
  };
  Ok(Some(yuzuko_core::op::root(scope, object)))
}

fn read_data_transfer(
  scope: &mut v8::PinScope,
  init: Option<v8::Local<v8::Value>>,
) -> Result<Option<v8::TracedReference<v8::Object>>, OpError> {
  let Some(value) = util::dict_get(scope, init, "dataTransfer") else {
    return Ok(None);
  };
  if value.is_null() || value.is_undefined() {
    return Ok(None);
  }
  let Ok(object) = v8::Local::<v8::Object>::try_from(value) else {
    return Err(OpError::type_(
      "The provided value is not of type 'DataTransfer'.",
    ));
  };
  let global = scope.get_current_context().global(scope);
  let is_transfer = global
    .get(scope, key(scope, "DataTransfer").into())
    .and_then(|value| v8::Local::<v8::Function>::try_from(value).ok())
    .and_then(|constructor| object.instance_of(scope, constructor.into()))
    .unwrap_or(false);
  if !is_transfer {
    return Err(OpError::type_(
      "The provided value is not of type 'DataTransfer'.",
    ));
  }
  Ok(Some(traced(scope, object)))
}

pub(crate) fn install(scope: &mut v8::PinScope) {
  UIEvent::install(scope);
  FocusEvent::install(scope);
  MouseEvent::install(scope);
  DragEvent::install(scope);
  WheelEvent::install(scope);
  Touch::install(scope);
  install_touch_list_indices(scope);
  TouchList::install(scope);
  TouchEvent::install(scope);
  KeyboardEvent::install(scope);
  CompositionEvent::install(scope);
  InputEvent::install(scope);
  PointerEvent::install(scope);
  define_constants(
    scope,
    "KeyboardEvent",
    &[
      ("DOM_KEY_LOCATION_STANDARD", 0),
      ("DOM_KEY_LOCATION_LEFT", 1),
      ("DOM_KEY_LOCATION_RIGHT", 2),
      ("DOM_KEY_LOCATION_NUMPAD", 3),
    ],
  );
  define_constants(
    scope,
    "WheelEvent",
    &[
      ("DOM_DELTA_PIXEL", 0),
      ("DOM_DELTA_LINE", 1),
      ("DOM_DELTA_PAGE", 2),
    ],
  );
}

fn define_constants(
  scope: &mut v8::PinScope,
  class_name: &str,
  pairs: &[(&str, i32)],
) {
  let global = scope.get_current_context().global(scope);
  let Some(constructor) = global.get(scope, key(scope, class_name).into())
  else {
    return;
  };
  let Ok(constructor) = v8::Local::<v8::Object>::try_from(constructor) else {
    return;
  };
  let Some(prototype) = constructor.get(scope, key(scope, "prototype").into())
  else {
    return;
  };
  let Ok(prototype) = v8::Local::<v8::Object>::try_from(prototype) else {
    return;
  };
  for (name, value) in pairs {
    let number = v8::Integer::new(scope, *value);
    let name = key(scope, name);
    let attribute =
      v8::PropertyAttribute::READ_ONLY | v8::PropertyAttribute::DONT_DELETE;
    let _ = constructor.define_own_property(
      scope,
      name.into(),
      number.into(),
      attribute,
    );
    let _ = prototype.define_own_property(
      scope,
      name.into(),
      number.into(),
      v8::PropertyAttribute::READ_ONLY | v8::PropertyAttribute::DONT_DELETE,
    );
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn tilt_boundaries_match_the_pointer_events_examples() {
    let cases = [
      (0, 0, 0.0, PI / 2.0),
      (0, 90, PI / 2.0, 0.0),
      (0, -90, 3.0 * PI / 2.0, 0.0),
      (90, 0, 0.0, 0.0),
      (90, 90, 0.0, 0.0),
      (-90, 0, PI, 0.0),
      (0, 45, PI / 2.0, PI / 4.0),
      (0, -45, 3.0 * PI / 2.0, PI / 4.0),
      (45, 0, 0.0, PI / 4.0),
      (-45, 0, PI, PI / 4.0),
    ];
    for (tilt_x, tilt_y, azimuth, altitude) in cases {
      let (got_azimuth, got_altitude) = tilt_to_spherical(tilt_x, tilt_y);
      assert_eq!(
        got_azimuth.to_bits(),
        azimuth.to_bits(),
        "{tilt_x},{tilt_y}"
      );
      assert_eq!(
        got_altitude.to_bits(),
        altitude.to_bits(),
        "{tilt_x},{tilt_y}"
      );
    }
  }

  #[test]
  fn spherical_boundaries_round_to_integer_tilt() {
    let cases = [
      (0.0, 0.0, 90, 0),
      (0.0, PI / 4.0, 45, 0),
      (0.0, PI / 2.0, 0, 0),
      (PI / 2.0, 0.0, 0, 90),
      (PI / 2.0, PI / 4.0, 0, 45),
      (PI, 0.0, -90, 0),
      (PI, PI / 4.0, -45, 0),
      (3.0 * PI / 2.0, 0.0, 0, -90),
      (3.0 * PI / 2.0, PI / 4.0, 0, -45),
    ];
    for (azimuth, altitude, tilt_x, tilt_y) in cases {
      assert_eq!(
        spherical_to_tilt(azimuth, altitude),
        (tilt_x, tilt_y),
        "{azimuth},{altitude}"
      );
    }
  }
}
