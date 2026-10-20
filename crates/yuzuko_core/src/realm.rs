//! Realm globals that are not tied to a later feature crate.

use v8::ValueDeserializerHelper;
use v8::ValueSerializerHelper;
use yuzuko_macros::op;

use crate::clone;
use crate::host;
use crate::js::{self, rust_string};

/// Deletes the Annex B accessor `Object.prototype.__proto__`.
///
/// `{ __proto__: null }` and `Object.setPrototypeOf` still set [[Prototype]].
/// Assigning the property writes an own data property.
pub fn delete_object_prototype_proto(scope: &mut v8::PinScope) {
  let object = v8::Object::new(scope);
  let Some(prototype) = object.get_prototype(scope) else {
    return;
  };
  let Ok(prototype) = v8::Local::<v8::Object>::try_from(prototype) else {
    return;
  };
  let Some(key) = v8::String::new(scope, "__proto__") else {
    return;
  };
  let _ = prototype.delete(scope, key.into());
}

pub fn install(scope: &mut v8::PinScope) {
  delete_object_prototype_proto(scope);
  let global = js::global(scope);
  js::set_value(scope, global, "self", global.into());
  if !host::is_dedicated_worker() {
    js::set_value(scope, global, "window", global.into());
  }
  js::set_function(scope, global, "queueMicrotask", queue_microtask);
  js::set_function(scope, global, "structuredClone", structured_clone);
  js::set_function(scope, global, "reportError", report_error);
  install_performance(scope, global);
  install_navigator(scope, global);
}

fn queue_microtask(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut _ret: v8::ReturnValue,
) {
  let Ok(function) = v8::Local::<v8::Function>::try_from(args.get(0)) else {
    js::throw_type(scope, "queueMicrotask requires a function");
    return;
  };
  scope.enqueue_microtask(function);
}

fn report_error(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut _ret: v8::ReturnValue,
) {
  let text = if args.length() == 0 {
    "undefined".to_string()
  } else {
    rust_string(scope, args.get(0))
  };
  eprintln!("reported error: {text}");
}

fn install_performance(
  scope: &mut v8::PinScope,
  global: v8::Local<v8::Object>,
) {
  let performance = v8::Object::new(scope);
  js::set_function(scope, performance, "now", performance_now);
  let millis = host::with(|state| state.time_origin_ms);
  js::set_value(
    scope,
    performance,
    "timeOrigin",
    v8::Number::new(scope, millis).into(),
  );
  js::set_value(scope, global, "performance", performance.into());
}

pub fn performance_now_ms() -> f64 {
  host::with(|state| state.time_origin.elapsed().as_secs_f64() * 1000.0)
}

pub fn time_origin_ms() -> f64 {
  host::with(|state| state.time_origin_ms)
}

#[op]
fn performance_now() -> f64 {
  performance_now_ms()
}

fn install_navigator(scope: &mut v8::PinScope, global: v8::Local<v8::Object>) {
  let navigator = v8::Object::new(scope);
  let agent = v8::String::new(scope, "Yuzuko/0.1.0").unwrap();
  js::set_value(scope, navigator, "userAgent", agent.into());
  let platform = v8::String::new(scope, navigator_platform()).unwrap();
  js::set_value(scope, navigator, "platform", platform.into());
  js::set_value(scope, global, "navigator", navigator.into());
}

/// Historical `navigator.platform` tokens. Endings tests treat a `Win` prefix
/// as CRLF and anything else as LF.
fn navigator_platform() -> &'static str {
  match std::env::consts::OS {
    "windows" => "Win32",
    "macos" => "MacIntel",
    "linux" => "Linux",
    _ => "Unknown",
  }
}

pub fn clone_value<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  value: v8::Local<v8::Value>,
) -> Result<v8::Local<'s, v8::Value>, Option<v8::Global<v8::Value>>> {
  let cloned = {
    v8::tc_scope!(let tc, scope);
    let serializer = v8::ValueSerializer::new(tc, Box::new(CloneHost));
    serializer.write_header();
    let context = tc.get_current_context();
    if serializer.write_value(context, value).is_none() {
      return Err(tc.exception().map(|thrown| v8::Global::new(tc, thrown)));
    }
    let bytes = serializer.release();
    let deserializer =
      v8::ValueDeserializer::new(tc, Box::new(CloneHost), &bytes);
    let context = tc.get_current_context();
    if deserializer.read_header(context).is_none() {
      return Err(tc.exception().map(|thrown| v8::Global::new(tc, thrown)));
    }
    let Some(cloned) = deserializer.read_value(context) else {
      return Err(tc.exception().map(|thrown| v8::Global::new(tc, thrown)));
    };
    v8::Global::new(tc, cloned)
  };
  Ok(v8::Local::new(scope, cloned))
}

fn structured_clone(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut ret: v8::ReturnValue,
) {
  if args.length() < 1 {
    js::throw_type(scope, "structuredClone requires a value");
    return;
  }
  let transfers = match transfer_list(scope, args.get(1)) {
    Ok(transfers) => transfers,
    Err(()) => return,
  };
  match clone_value(scope, args.get(0)) {
    Ok(cloned) => {
      for buffer in &transfers {
        let buffer = v8::Local::new(scope, buffer);
        let _ = buffer.detach(None);
      }
      ret.set(cloned);
    }
    Err(Some(thrown)) => {
      let thrown = v8::Local::new(scope, thrown);
      scope.throw_exception(thrown);
    }
    Err(None) => clone::reject_clone(scope),
  }
}

/// `structuredClone`'s `transfer` list. ArrayBuffers are detached after the
/// value is copied. Other transferable types are rejected.
fn transfer_list(
  scope: &mut v8::PinScope,
  options: v8::Local<v8::Value>,
) -> Result<Vec<v8::Global<v8::ArrayBuffer>>, ()> {
  if options.is_null_or_undefined() {
    return Ok(Vec::new());
  }
  let Ok(object) = v8::Local::<v8::Object>::try_from(options) else {
    js::throw_type(scope, "The structuredClone options are not an object");
    return Err(());
  };
  let key = v8::String::new(scope, "transfer").unwrap();
  let Some(transfer) = object.get(scope, key.into()) else {
    return Err(());
  };
  if transfer.is_undefined() {
    return Ok(Vec::new());
  }
  let Ok(list) = v8::Local::<v8::Array>::try_from(transfer) else {
    js::throw_type(scope, "The structuredClone transfer list is not an array");
    return Err(());
  };
  let mut buffers = Vec::new();
  for index in 0..list.length() {
    let Some(item) = list.get_index(scope, index) else {
      return Err(());
    };
    let Ok(buffer) = v8::Local::<v8::ArrayBuffer>::try_from(item) else {
      clone::throw_data_clone(scope, "The value cannot be transferred");
      return Err(());
    };
    if buffer.was_detached() || !buffer.is_detachable() {
      clone::throw_data_clone(scope, "The ArrayBuffer cannot be transferred");
      return Err(());
    }
    let duplicate = buffers.iter().any(|existing| {
      let existing: v8::Local<v8::ArrayBuffer> =
        v8::Local::new(scope, existing);
      existing.strict_equals(buffer.into())
    });
    if duplicate {
      clone::throw_data_clone(
        scope,
        "The ArrayBuffer is already in the transfer list",
      );
      return Err(());
    }
    buffers.push(v8::Global::new(scope, buffer));
  }
  Ok(buffers)
}

struct CloneHost;

impl v8::ValueSerializerImpl for CloneHost {
  fn throw_data_clone_error<'s>(
    &self,
    scope: &mut v8::PinScope<'s, '_>,
    message: v8::Local<'s, v8::String>,
  ) {
    clone::throw_data_clone(scope, &message.to_rust_string_lossy(scope));
  }

  fn has_custom_host_object(&self, _isolate: &v8::Isolate) -> bool {
    true
  }

  fn is_host_object<'s>(
    &self,
    _scope: &mut v8::PinScope<'s, '_>,
    object: v8::Local<'s, v8::Object>,
  ) -> Option<bool> {
    Some(object.is_api_wrapper())
  }

  fn write_host_object<'s>(
    &self,
    scope: &mut v8::PinScope<'s, '_>,
    object: v8::Local<'s, v8::Object>,
    value_serializer: &dyn v8::ValueSerializerHelper,
  ) -> Option<bool> {
    clone::write_host_object(scope, object, value_serializer)
  }
}

impl v8::ValueDeserializerImpl for CloneHost {
  fn read_host_object<'s>(
    &self,
    scope: &mut v8::PinScope<'s, '_>,
    value_deserializer: &dyn v8::ValueDeserializerHelper,
  ) -> Option<v8::Local<'s, v8::Object>> {
    clone::read_host_object(scope, value_deserializer)
  }
}
