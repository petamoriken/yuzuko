//! Web Audio on `globalThis`.

mod classes;
mod decode;
mod engine;
mod hardware;
mod nodes;
mod playback;
mod process;
mod util;

use std::sync::Once;

use yuzuko_core::op::{OpError, OpException};

pub fn register() {
  static ONCE: Once = Once::new();
  ONCE.call_once(|| {
    yuzuko_core::add_extension(&install);
    yuzuko_core::on_realm_drop(shutdown);
  });
}

fn install(scope: &mut v8::PinScope) {
  classes::install(scope);
  let yuzuko = yuzuko_core::namespace(scope, "Yuzuko");
  define_fn(scope, yuzuko, "audioOutputDevices", 0, audio_output_devices);
}

fn define_fn(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
  length: i32,
  callback: impl v8::MapFnTo<v8::FunctionCallback>,
) {
  let function = v8::Function::builder(callback)
    .length(length)
    .constructor_behavior(v8::ConstructorBehavior::Throw)
    .build(scope)
    .unwrap_or_else(|| panic!("create function {name}"));
  let key = v8::String::new(scope, name).unwrap();
  function.set_name(key);
  object.set(scope, key.into(), function.into());
}

fn audio_output_devices(
  scope: &mut v8::PinScope,
  _args: v8::FunctionCallbackArguments,
  mut ret: v8::ReturnValue,
) {
  match output_devices_value(scope) {
    Ok(value) => ret.set(value.into()),
    Err(error) => OpException::throw(error, scope),
  }
}

fn output_devices_value<'s>(
  scope: &mut v8::PinScope<'s, '_>,
) -> Result<v8::Local<'s, v8::Array>, OpError> {
  let names = playback::output_device_names().map_err(|_| {
    OpError::dom("NotSupportedError", "The audio outputs could not be listed")
  })?;
  let length = i32::try_from(names.len()).map_err(|_| {
    OpError::dom("NotSupportedError", "The audio outputs could not be listed")
  })?;
  let array = v8::Array::new(scope, length);
  for (index, name) in names.into_iter().enumerate() {
    let Some(text) = v8::String::new(scope, &name) else {
      return Err(OpError::dom(
        "NotSupportedError",
        "The audio outputs could not be listed",
      ));
    };
    let index = u32::try_from(index).map_err(|_| {
      OpError::dom("NotSupportedError", "The audio outputs could not be listed")
    })?;
    if array.set_index(scope, index, text.into()) != Some(true) {
      return Err(OpError::dom(
        "NotSupportedError",
        "The audio outputs could not be listed",
      ));
    }
  }
  Ok(array)
}

fn shutdown() {
  engine::clear();
  classes::clear();
}
