//! `Yuzuko.__test` input injection. Installed for `yuzuko test` when
//! `YUZUKO_TEST_INPUT=1`.

use yuzuko_core::op::{OpError, OpException, host_ref};

use crate::inbox::{
  self, PointerKind, Raw, RawIme, RawKey, RawPointer, RawWheel,
};
use crate::input;
use crate::input::gamepad;
use crate::surface::Window;

pub fn install(scope: &mut v8::PinScope) {
  let yuzuko = yuzuko_core::namespace(scope, "Yuzuko");
  let api = v8::Object::new(scope);
  let inject = v8::Function::new(scope, inject).expect("inject");
  let name = v8::String::new(scope, "inject").unwrap();
  api.set(scope, name.into(), inject.into());
  gamepad::install_test(scope, api);
  let key = v8::String::new(scope, "__test").unwrap();
  yuzuko.set(scope, key.into(), api.into());
}

fn inject(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut _ret: v8::ReturnValue,
) {
  if let Err(error) = inject_events(scope, &args) {
    OpException::throw(error, scope);
  }
}

fn inject_events(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
) -> Result<(), OpError> {
  let Ok(object) = v8::Local::<v8::Object>::try_from(args.get(0)) else {
    return Err(OpError::type_("inject requires a Window"));
  };
  let Some(host) = (unsafe { host_ref::<Window>(scope, object) }) else {
    return Err(OpError::type_("inject requires a Window"));
  };
  let id = host.get().id();
  if !inbox::contains(id) {
    return Err(OpError::dom("InvalidStateError", "The surface is closed."));
  }
  let Ok(list) = v8::Local::<v8::Array>::try_from(args.get(1)) else {
    return Err(OpError::type_("inject requires an array of events"));
  };
  for index in 0..list.length() {
    let Some(item) = list.get_index(scope, index) else {
      continue;
    };
    let Ok(record) = v8::Local::<v8::Object>::try_from(item) else {
      return Err(OpError::type_("inject events must be objects"));
    };
    if prop_string(scope, record, "kind").as_deref() == Some("trayclick") {
      crate::shell::tray::dispatch_primary_click(scope)?;
      continue;
    }
    inbox::push(id, read_event(scope, record)?);
  }
  input::flush(scope, id);
  Ok(())
}

fn read_event(
  scope: &mut v8::PinScope,
  record: v8::Local<v8::Object>,
) -> Result<Raw, OpError> {
  let kind = prop_string(scope, record, "kind").unwrap_or_default();
  match kind.as_str() {
    "pointer" => Ok(Raw::Pointer(read_pointer(scope, record)?)),
    "key" => Ok(Raw::Key(read_key(scope, record)?)),
    "ime" => Ok(Raw::Ime(read_ime(scope, record)?)),
    "wheel" => Ok(Raw::Wheel(read_wheel(scope, record)?)),
    "focus" => Ok(Raw::Focus),
    "blur" => Ok(Raw::Blur),
    "resize" => Ok(Raw::Resize),
    "visibility" => {
      let state = prop_string(scope, record, "state").unwrap_or_default();
      match state.as_str() {
        "visible" => Ok(Raw::Visibility(true)),
        "hidden" => Ok(Raw::Visibility(false)),
        _ => Err(OpError::type_("visibility state must be visible or hidden")),
      }
    }
    "menu" => {
      let Some(action) = prop_string(scope, record, "action") else {
        return Err(OpError::type_("menu inject requires an action"));
      };
      if !crate::shell::menu::known(&action) {
        return Err(OpError::type_("The menu action is not installed"));
      }
      Ok(Raw::Menu(action))
    }
    other => Err(OpError::type_(format!("unknown inject kind {other}"))),
  }
}

fn read_pointer(
  scope: &mut v8::PinScope,
  record: v8::Local<v8::Object>,
) -> Result<RawPointer, OpError> {
  let type_name = prop_string(scope, record, "type")
    .unwrap_or_else(|| "pointermove".to_string());
  let kind = match type_name.as_str() {
    "pointerdown" => PointerKind::Down,
    "pointerup" => PointerKind::Up,
    "pointercancel" => PointerKind::Cancel,
    _ => PointerKind::Move,
  };
  let button = prop_number(scope, record, "button")?
    .map(|value| value as i32)
    .unwrap_or(if kind == PointerKind::Move { -1 } else { 0 });
  let x = prop_number(scope, record, "x")?.unwrap_or(0.0);
  let y = prop_number(scope, record, "y")?.unwrap_or(0.0);
  Ok(RawPointer {
    kind,
    pointer_id: prop_number(scope, record, "pointerId")?
      .map(|value| value as i32)
      .unwrap_or(1),
    pointer_type: prop_string(scope, record, "pointerType")
      .unwrap_or_else(|| "mouse".to_string()),
    is_primary: prop_bool(scope, record, "isPrimary").unwrap_or(true),
    x,
    y,
    screen_x: prop_number(scope, record, "screenX")?.unwrap_or(x),
    screen_y: prop_number(scope, record, "screenY")?.unwrap_or(y),
    button,
    pressure: prop_number(scope, record, "pressure")?,
    movement_x: prop_number(scope, record, "movementX")?,
    movement_y: prop_number(scope, record, "movementY")?,
    ctrl: prop_bool(scope, record, "ctrlKey").unwrap_or(false),
    shift: prop_bool(scope, record, "shiftKey").unwrap_or(false),
    alt: prop_bool(scope, record, "altKey").unwrap_or(false),
    meta: prop_bool(scope, record, "metaKey").unwrap_or(false),
    tilt_x: prop_number(scope, record, "tiltX")?.unwrap_or(0.0) as i32,
    tilt_y: prop_number(scope, record, "tiltY")?.unwrap_or(0.0) as i32,
    twist: prop_number(scope, record, "twist")?.unwrap_or(0.0) as i32,
    width: prop_number(scope, record, "width")?.unwrap_or(1.0),
    height: prop_number(scope, record, "height")?.unwrap_or(1.0),
    tangential: prop_number(scope, record, "tangentialPressure")?
      .unwrap_or(0.0),
  })
}

fn read_key(
  scope: &mut v8::PinScope,
  record: v8::Local<v8::Object>,
) -> Result<RawKey, OpError> {
  let type_name =
    prop_string(scope, record, "type").unwrap_or_else(|| "keydown".to_string());
  Ok(RawKey {
    down: type_name != "keyup",
    key: prop_string(scope, record, "key").unwrap_or_default(),
    code: prop_string(scope, record, "code")
      .unwrap_or_else(|| "Unidentified".to_string()),
    location: prop_number(scope, record, "location")?.unwrap_or(0.0) as u32,
    repeat: prop_bool(scope, record, "repeat").unwrap_or(false),
    text: prop_string(scope, record, "text"),
    ctrl: prop_bool(scope, record, "ctrlKey").unwrap_or(false),
    shift: prop_bool(scope, record, "shiftKey").unwrap_or(false),
    alt: prop_bool(scope, record, "altKey").unwrap_or(false),
    meta: prop_bool(scope, record, "metaKey").unwrap_or(false),
  })
}

fn read_ime(
  scope: &mut v8::PinScope,
  record: v8::Local<v8::Object>,
) -> Result<RawIme, OpError> {
  let action = prop_string(scope, record, "action").unwrap_or_default();
  let data = prop_string(scope, record, "data").unwrap_or_default();
  match action.as_str() {
    "enabled" => Ok(RawIme::Enabled),
    "preedit" => Ok(RawIme::Preedit(data)),
    "commit" => Ok(RawIme::Commit(data)),
    "disabled" => Ok(RawIme::Disabled),
    other => Err(OpError::type_(format!("unknown ime action {other}"))),
  }
}

fn read_wheel(
  scope: &mut v8::PinScope,
  record: v8::Local<v8::Object>,
) -> Result<RawWheel, OpError> {
  let x = prop_number(scope, record, "x")?.unwrap_or(0.0);
  let y = prop_number(scope, record, "y")?.unwrap_or(0.0);
  Ok(RawWheel {
    x,
    y,
    screen_x: prop_number(scope, record, "screenX")?.unwrap_or(x),
    screen_y: prop_number(scope, record, "screenY")?.unwrap_or(y),
    delta_x: prop_number(scope, record, "deltaX")?.unwrap_or(0.0),
    delta_y: prop_number(scope, record, "deltaY")?.unwrap_or(0.0),
    delta_z: prop_number(scope, record, "deltaZ")?.unwrap_or(0.0),
    delta_mode: prop_number(scope, record, "deltaMode")?.unwrap_or(0.0) as u32,
    ctrl: prop_bool(scope, record, "ctrlKey").unwrap_or(false),
    shift: prop_bool(scope, record, "shiftKey").unwrap_or(false),
    alt: prop_bool(scope, record, "altKey").unwrap_or(false),
    meta: prop_bool(scope, record, "metaKey").unwrap_or(false),
  })
}

fn prop<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  object: v8::Local<'_, v8::Object>,
  name: &str,
) -> Option<v8::Local<'s, v8::Value>> {
  let key = v8::String::new(scope, name)?;
  let object = yuzuko_core::op::root(scope, object);
  let value = object.get(scope, key.into())?;
  if value.is_undefined() {
    None
  } else {
    Some(value)
  }
}

fn prop_string(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
) -> Option<String> {
  let value = prop(scope, object, name)?;
  if value.is_null() {
    return None;
  }
  Some(value.to_rust_string_lossy(scope))
}

fn prop_number(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
) -> Result<Option<f64>, OpError> {
  let Some(value) = prop(scope, object, name) else {
    return Ok(None);
  };
  if value.is_null() {
    return Ok(None);
  }
  <f64 as yuzuko_core::op::FromV8>::from_v8(scope, value).map(Some)
}

fn prop_bool(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
) -> Option<bool> {
  prop(scope, object, name).map(|value| value.boolean_value(scope))
}
