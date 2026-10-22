//! Dictionary and promise helpers for the WebGPU binding.

use yuzuko_core::op::{Js, OpError};

pub fn promise(scope: &mut v8::PinScope, value: v8::Local<v8::Value>) -> Js {
  let resolver = v8::PromiseResolver::new(scope).unwrap();
  resolver.resolve(scope, value);
  let promise = resolver.get_promise(scope);
  let promise: v8::Local<v8::Value> = promise.into();
  Js(v8::Global::new(scope, promise))
}

pub fn promise_null(scope: &mut v8::PinScope) -> Js {
  let value: v8::Local<v8::Value> = v8::null(scope).into();
  promise(scope, value)
}

pub fn promise_type_error(scope: &mut v8::PinScope, message: &str) -> Js {
  let resolver = v8::PromiseResolver::new(scope).unwrap();
  let text = v8::String::new(scope, message).unwrap();
  let error = v8::Exception::type_error(scope, text);
  resolver.reject(scope, error);
  let promise = resolver.get_promise(scope);
  let promise: v8::Local<v8::Value> = promise.into();
  Js(v8::Global::new(scope, promise))
}

pub fn object<'a>(
  _scope: &mut v8::PinScope,
  value: v8::Local<'a, v8::Value>,
) -> Option<v8::Local<'a, v8::Object>> {
  v8::Local::<v8::Object>::try_from(value).ok()
}

pub fn get<'a>(
  scope: &mut v8::PinScope,
  object: v8::Local<'a, v8::Object>,
  name: &str,
) -> Option<v8::Local<'a, v8::Value>> {
  let key = v8::String::new(scope, name)?;
  let value = object.get(scope, key.into())?;
  if value.is_undefined() {
    return None;
  }
  // Detach the handle from this mutable borrow so callers can use `scope`.
  Some(unsafe {
    std::mem::transmute::<v8::Local<'_, v8::Value>, v8::Local<'a, v8::Value>>(
      value,
    )
  })
}

pub fn string(
  scope: &v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Option<String> {
  if value.is_null_or_undefined() {
    return None;
  }
  value
    .to_string(scope)
    .map(|text| text.to_rust_string_lossy(scope))
}

pub fn finite_number(
  scope: &v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<f64, OpError> {
  let number = value
    .number_value(scope)
    .ok_or_else(|| OpError::type_("The provided value is not a number"))?;
  if !number.is_finite() {
    return Err(OpError::type_("The provided value is not finite"));
  }
  Ok(number)
}

pub fn u32_value(
  scope: &v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<u32, OpError> {
  let number = finite_number(scope, value)?;
  if number < 0.0 || number > f64::from(u32::MAX) {
    return Err(OpError::range("The provided value is out of range"));
  }
  Ok(number as u32)
}

pub fn field_u32(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
) -> Result<Option<u32>, OpError> {
  let Some(value) = get(scope, object, name) else {
    return Ok(None);
  };
  u32_value(scope, value).map(Some)
}

pub fn required_u32(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
) -> Result<u32, OpError> {
  field_u32(scope, object, name)?
    .ok_or_else(|| OpError::type_(format!("{name} is required")))
}

pub fn index_u32(
  scope: &mut v8::PinScope,
  array: v8::Local<v8::Array>,
  index: u32,
) -> Result<u32, OpError> {
  let Some(value) = array.get_index(scope, index) else {
    return Ok(0);
  };
  u32_value(scope, value)
}

pub fn set_u32(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
  value: u32,
) {
  let key = v8::String::new(scope, name).unwrap();
  let number = v8::Integer::new_from_unsigned(scope, value);
  object.set(scope, key.into(), number.into());
}

pub fn flag_object(
  scope: &mut v8::PinScope,
  pairs: &[(&str, u32)],
) -> v8::Global<v8::Object> {
  let object = v8::Object::new(scope);
  for (name, value) in pairs {
    set_u32(scope, object, name, *value);
  }
  v8::Global::new(scope, object)
}

pub fn unsupported(what: &str) -> OpError {
  OpError::dom("NotSupportedError", format!("{what} is not implemented"))
}

pub fn promise_rejected(scope: &mut v8::PinScope, error: OpError) -> Js {
  let resolver = v8::PromiseResolver::new(scope).unwrap();
  let exception = exception_value(scope, error);
  resolver.reject(scope, exception);
  let promise = resolver.get_promise(scope);
  let promise: v8::Local<v8::Value> = promise.into();
  Js(v8::Global::new(scope, promise))
}

fn exception_value<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  error: OpError,
) -> v8::Local<'s, v8::Value> {
  match error {
    OpError::Type(message) => {
      let text = v8::String::new(scope, &message).unwrap();
      v8::Exception::type_error(scope, text)
    }
    OpError::Range(message) => {
      let text = v8::String::new(scope, &message).unwrap();
      v8::Exception::range_error(scope, text)
    }
    OpError::Dom { name, message } => {
      yuzuko_core::op::new_dom_exception(scope, &name, &message)
        .map(v8::Local::<v8::Value>::from)
        .unwrap_or_else(|| {
          let text = v8::String::new(scope, &message).unwrap();
          v8::Exception::type_error(scope, text)
        })
    }
    OpError::Thrown(value) => v8::Local::new(scope, value.as_ref()),
  }
}
