//! Small conversions shared by the DOM classes.

use yuzuko_core::op::OpError;

pub(crate) fn key<'s>(
  scope: &v8::PinScope<'s, '_>,
  text: &str,
) -> v8::Local<'s, v8::String> {
  v8::String::new(scope, text).unwrap()
}

pub(crate) fn string_value(
  scope: &v8::PinScope,
  value: v8::Local<v8::Value>,
) -> String {
  value
    .to_string(scope)
    .map(|text| text.to_rust_string_lossy(scope))
    .unwrap_or_default()
}

/// `ToString`, rethrowing the JavaScript exception unchanged.
pub(crate) fn try_string(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<String, OpError> {
  let captured = {
    v8::tc_scope!(let tc, scope);
    match value.to_string(tc) {
      Some(text) => Ok(text.to_rust_string_lossy(tc)),
      None => {
        let exception =
          tc.exception().unwrap_or_else(|| v8::undefined(tc).into());
        Err(v8::Global::new(tc, exception))
      }
    }
  };
  match captured {
    Ok(text) => Ok(text),
    Err(exception) => {
      let exception = v8::Local::new(scope, exception);
      Err(OpError::thrown(scope, exception))
    }
  }
}

pub(crate) fn is_global(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
) -> bool {
  let global = scope.get_current_context().global(scope);
  object.strict_equals(global.into())
}

pub(crate) fn dict_get<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  init: Option<v8::Local<'_, v8::Value>>,
  name: &str,
) -> Option<v8::Local<'s, v8::Value>> {
  let init = init.filter(|value| value.is_object())?;
  let init = yuzuko_core::op::root(scope, init);
  let object = v8::Local::<v8::Object>::try_from(init).ok()?;
  object.get(scope, key(scope, name).into())
}

pub(crate) fn dict_bool(
  scope: &mut v8::PinScope,
  init: Option<v8::Local<v8::Value>>,
  name: &str,
) -> bool {
  dict_get(scope, init, name).is_some_and(|value| value.boolean_value(scope))
}

pub(crate) fn dict_number(
  scope: &mut v8::PinScope,
  init: Option<v8::Local<v8::Value>>,
  name: &str,
) -> f64 {
  let Some(value) = dict_get(scope, init, name) else {
    return 0.0;
  };
  if value.is_null() || value.is_undefined() {
    return 0.0;
  }
  value.number_value(scope).unwrap_or(f64::NAN)
}

pub(crate) fn dict_string(
  scope: &mut v8::PinScope,
  init: Option<v8::Local<v8::Value>>,
  name: &str,
) -> String {
  let Some(value) = dict_get(scope, init, name) else {
    return String::new();
  };
  if value.is_null() || value.is_undefined() {
    return String::new();
  }
  string_value(scope, value)
}

pub(crate) fn has_own(
  scope: &mut v8::PinScope,
  init: Option<v8::Local<v8::Value>>,
  name: &str,
) -> bool {
  let Some(init) = init.filter(|value| value.is_object()) else {
    return false;
  };
  let Ok(object) = v8::Local::<v8::Object>::try_from(init) else {
    return false;
  };
  object
    .has_own_property(scope, key(scope, name).into())
    .unwrap_or(false)
}

pub(crate) fn init_arg<'s>(
  args: &v8::FunctionCallbackArguments<'s>,
) -> Option<v8::Local<'s, v8::Value>> {
  if args.length() < 2 {
    return None;
  }
  let value = args.get(1);
  if value.is_null() || value.is_undefined() {
    None
  } else {
    Some(value)
  }
}
