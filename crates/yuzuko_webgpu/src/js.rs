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
) -> Result<Option<v8::Local<'a, v8::Value>>, OpError> {
  // Detach the handle from this mutable borrow so callers can use `scope`.
  yuzuko_core::dictionary_member(scope, object, name).map(|value| {
    value.map(|value| unsafe {
      std::mem::transmute::<v8::Local<'_, v8::Value>, v8::Local<'a, v8::Value>>(
        value,
      )
    })
  })
}

/// WebIDL `DOMString`. `null` converts to `"null"`. A throwing `toString` is returned.
pub fn string(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<String, OpError> {
  try_string(scope, value)
}

/// `ToString`, keeping a throw from `toString` as the rejection.
pub fn try_string(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<String, OpError> {
  v8::tc_scope!(let tc, scope);
  let Some(text) = value.to_string(tc) else {
    return Err(match tc.exception() {
      Some(thrown) => OpError::thrown(tc, thrown),
      None => OpError::type_("The provided value is not a string"),
    });
  };
  Ok(text.to_rust_string_lossy(tc))
}

/// Own property get. `undefined` is absent. A throwing getter is returned.
pub fn try_get(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
) -> Result<Option<v8::Global<v8::Value>>, OpError> {
  v8::tc_scope!(let tc, scope);
  let Some(key) = v8::String::new(tc, name) else {
    return Err(OpError::type_("The provided value is not readable"));
  };
  match object.get(tc, key.into()) {
    Some(value) if value.is_undefined() => Ok(None),
    Some(value) => Ok(Some(v8::Global::new(tc, value))),
    None => Err(match tc.exception() {
      Some(thrown) => OpError::thrown(tc, thrown),
      None => OpError::type_("The provided value is not readable"),
    }),
  }
}

pub fn finite_number(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<f64, OpError> {
  yuzuko_core::finite_number(scope, value, "The provided value is not finite")
}

/// `[EnforceRange] unsigned long`, after `ToNumber`.
pub fn u32_value(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<u32, OpError> {
  let number = yuzuko_core::to_number(scope, value)?;
  yuzuko_core::enforce_range_u32(number)
    .ok_or_else(|| OpError::type_("The provided value is out of range"))
}

/// `[EnforceRange] unsigned long long` (`GPUSize64`). The upper bound is `2^53 − 1`.
pub fn u64_value(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<u64, OpError> {
  let number = yuzuko_core::to_number(scope, value)?;
  yuzuko_core::enforce_range_u64(number)
    .ok_or_else(|| OpError::type_("The provided value is out of range"))
}

/// WebIDL `float`. Non-finite values and magnitudes outside `f32` throw `TypeError`.
pub fn f32_value(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<f32, OpError> {
  let number = yuzuko_core::to_number(scope, value)?;
  let limit = f64::from(f32::MAX);
  if !number.is_finite() || number < -limit || number > limit {
    return Err(OpError::type_("The provided value is out of range"));
  }
  Ok(number as f32)
}

pub fn field_f32(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
) -> Result<Option<f32>, OpError> {
  let Some(value) = get(scope, object, name)? else {
    return Ok(None);
  };
  f32_value(scope, value).map(Some)
}

/// WebIDL `[Clamp] unsigned short`. Non-finite values throw `TypeError`.
pub fn clamp_u16(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<u16, OpError> {
  let number = yuzuko_core::to_number(scope, value)?;
  if !number.is_finite() {
    return Err(OpError::type_("The provided value is out of range"));
  }
  let number = number.clamp(0.0, f64::from(u16::MAX));
  Ok(number.round_ties_even() as u16)
}

pub fn field_u32(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
) -> Result<Option<u32>, OpError> {
  let Some(value) = get(scope, object, name)? else {
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

pub fn field_u64(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
) -> Result<Option<u64>, OpError> {
  let Some(value) = get(scope, object, name)? else {
    return Ok(None);
  };
  u64_value(scope, value).map(Some)
}

pub fn required_u64(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
) -> Result<u64, OpError> {
  field_u64(scope, object, name)?
    .ok_or_else(|| OpError::type_(format!("{name} is required")))
}

pub fn index_u32(
  scope: &mut v8::PinScope,
  array: v8::Local<v8::Array>,
  index: u32,
) -> Result<u32, OpError> {
  let value = read_index(scope, array, index)?;
  u32_value(scope, value)
}

pub trait GetIndex {
  fn get_at<'s>(
    &self,
    scope: &mut v8::PinScope<'s, '_>,
    index: u32,
  ) -> Option<v8::Local<'s, v8::Value>>;
}

impl GetIndex for v8::Local<'_, v8::Object> {
  fn get_at<'s>(
    &self,
    scope: &mut v8::PinScope<'s, '_>,
    index: u32,
  ) -> Option<v8::Local<'s, v8::Value>> {
    self.get_index(scope, index)
  }
}

impl GetIndex for v8::Local<'_, v8::Array> {
  fn get_at<'s>(
    &self,
    scope: &mut v8::PinScope<'s, '_>,
    index: u32,
  ) -> Option<v8::Local<'s, v8::Value>> {
    self.get_index(scope, index)
  }
}

/// Array element. A throwing getter is returned. A hole is `undefined`.
pub fn read_index<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  object: impl GetIndex,
  index: u32,
) -> Result<v8::Local<'s, v8::Value>, OpError> {
  let captured = {
    v8::tc_scope!(let tc, scope);
    match object.get_at(tc, index) {
      Some(value) => Ok(v8::Global::new(tc, value)),
      None => Err(match tc.exception() {
        Some(thrown) => OpError::thrown(tc, thrown),
        None => OpError::type_("The provided value is not readable"),
      }),
    }
  };
  captured.map(|value| v8::Local::new(scope, value))
}

/// `[EnforceRange] unsigned long long` for an `#[op]` argument.
pub struct GpuSize64(pub u64);

impl yuzuko_core::op::FromV8 for GpuSize64 {
  fn from_v8(
    scope: &mut v8::PinScope,
    value: v8::Local<v8::Value>,
  ) -> Result<Self, OpError> {
    u64_value(scope, value).map(GpuSize64)
  }
}

/// `[EnforceRange] unsigned long` (`GPUSize32`).
pub struct GpuSize32(pub u32);

impl yuzuko_core::op::FromV8 for GpuSize32 {
  fn from_v8(
    scope: &mut v8::PinScope,
    value: v8::Local<v8::Value>,
  ) -> Result<Self, OpError> {
    u32_value(scope, value).map(GpuSize32)
  }
}

/// `[EnforceRange] unsigned long` (`GPUIndex32`).
pub struct GpuIndex32(pub u32);

impl yuzuko_core::op::FromV8 for GpuIndex32 {
  fn from_v8(
    scope: &mut v8::PinScope,
    value: v8::Local<v8::Value>,
  ) -> Result<Self, OpError> {
    u32_value(scope, value).map(GpuIndex32)
  }
}

/// `[EnforceRange] unsigned long` (`GPUIntegerCoordinate`).
pub struct GpuIntegerCoordinate(pub u32);

impl yuzuko_core::op::FromV8 for GpuIntegerCoordinate {
  fn from_v8(
    scope: &mut v8::PinScope,
    value: v8::Local<v8::Value>,
  ) -> Result<Self, OpError> {
    u32_value(scope, value).map(GpuIntegerCoordinate)
  }
}

/// `[EnforceRange] long` (`GPUSignedOffset32`).
pub struct GpuSignedOffset32(pub i32);

impl yuzuko_core::op::FromV8 for GpuSignedOffset32 {
  fn from_v8(
    scope: &mut v8::PinScope,
    value: v8::Local<v8::Value>,
  ) -> Result<Self, OpError> {
    let number = yuzuko_core::to_number(scope, value)?;
    yuzuko_core::enforce_range_i32(number)
      .map(GpuSignedOffset32)
      .ok_or_else(|| OpError::type_("The provided value is out of range"))
  }
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
  let exception = exception_value(scope, error);
  promise_rejected_value(scope, exception)
}

pub fn promise_rejected_value(
  scope: &mut v8::PinScope,
  error: v8::Local<v8::Value>,
) -> Js {
  let resolver = v8::PromiseResolver::new(scope).unwrap();
  resolver.reject(scope, error);
  let promise = resolver.get_promise(scope);
  let promise: v8::Local<v8::Value> = promise.into();
  Js(v8::Global::new(scope, promise))
}

fn exception_value<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  error: OpError,
) -> v8::Local<'s, v8::Value> {
  yuzuko_core::exception_value(
    scope,
    error,
    yuzuko_core::DomFallback::Type,
    yuzuko_core::StringFailure::Unwrap,
  )
}
