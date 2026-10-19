//! Small helpers for installing host functions.

#[inline]
pub fn global<'a>(
  scope: &mut v8::PinScope<'a, '_>,
) -> v8::Local<'a, v8::Object> {
  scope.get_current_context().global(scope)
}

#[inline]
pub fn set_function(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
  callback: impl v8::MapFnTo<v8::FunctionCallback>,
) {
  let function = v8::Function::new(scope, callback)
    .unwrap_or_else(|| panic!("create function {name}"));
  let key = v8::String::new(scope, name).unwrap();
  object.set(scope, key.into(), function.into());
}

/// The object at `globalThis[name]`, created as a plain object when absent.
#[inline]
pub fn namespace<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  name: &str,
) -> v8::Local<'s, v8::Object> {
  let global = global(scope);
  let key = v8::String::new(scope, name).unwrap();
  if let Some(value) = global.get(scope, key.into())
    && !value.is_null_or_undefined()
    && let Ok(object) = v8::Local::<v8::Object>::try_from(value)
  {
    return object;
  }
  let object = v8::Object::new(scope);
  global.set(scope, key.into(), object.into());
  object
}

#[inline]
pub fn set_value(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
  value: v8::Local<v8::Value>,
) {
  let key = v8::String::new(scope, name).unwrap();
  object.set(scope, key.into(), value);
}

#[inline]
pub fn throw_type(scope: &mut v8::PinScope, message: &str) {
  let message = v8::String::new(scope, message).unwrap();
  let error = v8::Exception::type_error(scope, message);
  scope.throw_exception(error);
}

#[inline]
pub fn rust_string(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> String {
  value
    .to_string(scope)
    .map(|text| text.to_rust_string_lossy(scope))
    .unwrap_or_else(|| "undefined".to_string())
}
