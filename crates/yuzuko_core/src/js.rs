//! Small helpers for installing host functions.
//!
//! `set_value` and `set_function` perform `[[Set]]`. An ordinary object
//! created after user code can run uses `v8::Object::create_data_property`.

#[inline]
pub fn global<'a>(
  scope: &mut v8::PinScope<'a, '_>,
) -> v8::Local<'a, v8::Object> {
  scope.get_current_context().global(scope)
}

/// Installs `name` as a host function with `[[Set]]`.
///
/// Realm initialization only. A dictionary created after user code runs uses
/// `Object::create_data_property`. See `set_value`.
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

/// Assigns `name` with `[[Set]]`.
///
/// Realm initialization only: a property on `globalThis`, `Yuzuko`, or a
/// prototype. A dictionary, `toJSON` result, iterator result, or other
/// ordinary object created later uses `Object::create_data_property`, which
/// does not run an accessor on `Object.prototype`.
/// Defines element `index` of a host-made array or object as an own data
/// property, as CreateDataProperty does.
///
/// `Object::set_index` performs `[[Set]]`, so a setter that user code put on
/// `Array.prototype` or `Object.prototype` would receive the value and the
/// element would stay a hole.
pub fn define_element(
  scope: &mut v8::PinScope,
  target: v8::Local<v8::Object>,
  index: u32,
  value: v8::Local<v8::Value>,
) -> Option<bool> {
  let key = v8::String::new(scope, &index.to_string())?;
  target.create_data_property(scope, key.into(), value)
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

/// Canonical script URL at the top of the JavaScript stack.
///
/// A host callback with an empty stack returns `None`.
pub fn caller_module_url(scope: &mut v8::PinScope) -> Option<String> {
  let name = v8::StackTrace::current_script_name_or_source_url(scope)?;
  let text = name.to_rust_string_lossy(scope);
  if text.is_empty() { None } else { Some(text) }
}

#[inline]
pub fn rust_string(
  scope: &v8::PinScope,
  value: v8::Local<v8::Value>,
) -> String {
  rust_string_or(scope, value, "undefined")
}

/// `ToString`. `fallback` is used when the conversion fails.
#[inline]
pub fn rust_string_or(
  scope: &v8::PinScope,
  value: v8::Local<v8::Value>,
  fallback: &str,
) -> String {
  value
    .to_string(scope)
    .map(|text| text.to_rust_string_lossy(scope))
    .unwrap_or_else(|| fallback.to_string())
}

/// `ToString` as WTF-16 code units. `fallback` is used when the conversion fails.
#[inline]
pub fn wtf16_or(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
  fallback: &str,
) -> crate::op::Wtf16 {
  value
    .to_string(scope)
    .map(|text| crate::op::Wtf16::from_v8_string(scope, text))
    .unwrap_or_else(|| crate::op::Wtf16::from(fallback))
}

/// How a host-function installer reports an allocation failure.
enum InstallFailure {
  /// Panic with `create function {name}`.
  Panic,
  /// Leave the property unset.
  Skip,
}

/// Installs `name` as a host function.
///
/// `length` is the function's `length`. Calling it with `new` throws, and
/// the function's name is `name`. The property key is `name`.
#[inline]
pub fn define_function(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
  length: i32,
  callback: impl v8::MapFnTo<v8::FunctionCallback>,
) {
  let _ = install_host_function(
    scope,
    object,
    name,
    None,
    length,
    callback,
    InstallFailure::Panic,
  );
}

/// Installs `name` as a host function on `key`.
///
/// The function's name stays `name`. `key` may be a symbol, so the method is
/// not also stored under the string `name`.
#[inline]
pub fn define_function_key(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
  key: v8::Local<v8::Value>,
  length: i32,
  callback: impl v8::MapFnTo<v8::FunctionCallback>,
) {
  let _ = install_host_function(
    scope,
    object,
    name,
    Some(key),
    length,
    callback,
    InstallFailure::Panic,
  );
}

/// Installs `name` as a host function.
///
/// Returns `false`, and leaves the property unset, when V8 cannot allocate
/// the function or its name.
#[inline]
pub fn try_define_function(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
  length: i32,
  callback: impl v8::MapFnTo<v8::FunctionCallback>,
) -> bool {
  install_host_function(
    scope,
    object,
    name,
    None,
    length,
    callback,
    InstallFailure::Skip,
  )
}

/// Stores the function with `[[Set]]`. Realm initialization only.
fn install_host_function(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
  key: Option<v8::Local<v8::Value>>,
  length: i32,
  callback: impl v8::MapFnTo<v8::FunctionCallback>,
  failure: InstallFailure,
) -> bool {
  let Some(function) = v8::Function::builder(callback)
    .length(length)
    .constructor_behavior(v8::ConstructorBehavior::Throw)
    .build(scope)
  else {
    return host_function_failed(name, failure);
  };
  let Some(label) = v8::String::new(scope, name) else {
    return host_function_failed(name, failure);
  };
  function.set_name(label);
  object.set(scope, key.unwrap_or_else(|| label.into()), function.into());
  true
}

fn host_function_failed(name: &str, failure: InstallFailure) -> bool {
  match failure {
    InstallFailure::Panic => panic!("create function {name}"),
    InstallFailure::Skip => false,
  }
}

/// Own or inherited dictionary member. `undefined` is absent.
///
/// A throwing getter is returned. WebIDL does not treat that as a missing member.
#[inline]
pub fn dictionary_member<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  object: v8::Local<'_, v8::Object>,
  name: &str,
) -> Result<Option<v8::Local<'s, v8::Value>>, crate::op::OpError> {
  let object = crate::op::root(scope, object);
  let captured = {
    v8::tc_scope!(let tc, scope);
    let Some(key) = v8::String::new(tc, name) else {
      return Err(crate::op::OpError::type_(
        "The provided value is not readable",
      ));
    };
    match object.get(tc, key.into()) {
      Some(value) if value.is_undefined() => Ok(None),
      Some(value) => Ok(Some(v8::Global::new(tc, value))),
      None => Err(match tc.exception() {
        Some(thrown) => crate::op::OpError::thrown(tc, thrown),
        None => crate::op::OpError::type_("The provided value is not readable"),
      }),
    }
  };
  captured.map(|value| value.map(|value| v8::Local::new(scope, value)))
}

/// WebIDL `ToNumber`. `null` is `+0`. A thrown `valueOf` is returned.
#[inline]
pub fn to_number(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<f64, crate::op::OpError> {
  v8::tc_scope!(let tc, scope);
  match value.number_value(tc) {
    Some(number) => Ok(number),
    None => Err(match tc.exception() {
      Some(thrown) => crate::op::OpError::thrown(tc, thrown),
      None => crate::op::OpError::type_("The provided value is not a number"),
    }),
  }
}

/// WebIDL `double`: `ToNumber`, then reject a non-finite result.
#[inline]
pub fn finite_number(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
  not_finite: &'static str,
) -> Result<f64, crate::op::OpError> {
  let number = to_number(scope, value)?;
  if !number.is_finite() {
    return Err(crate::op::OpError::type_(not_finite));
  }
  Ok(number)
}

/// Exact integer span of a JavaScript number, `2^53 − 1`.
pub const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;

/// WebIDL `[EnforceRange] unsigned long`. Truncate toward zero, then range-check.
#[inline]
pub fn enforce_range_u32(number: f64) -> Option<u32> {
  if !number.is_finite() {
    return None;
  }
  let number = number.trunc();
  if number < 0.0 || number > f64::from(u32::MAX) {
    return None;
  }
  Some(number as u32)
}

/// WebIDL `[EnforceRange] long`. Truncate toward zero, then range-check.
#[inline]
pub fn enforce_range_i32(number: f64) -> Option<i32> {
  if !number.is_finite() {
    return None;
  }
  let number = number.trunc();
  if number < f64::from(i32::MIN) || number > f64::from(i32::MAX) {
    return None;
  }
  Some(number as i32)
}

/// WebIDL `[EnforceRange] unsigned long long`. The upper bound is `2^53 − 1`.
#[inline]
pub fn enforce_range_u64(number: f64) -> Option<u64> {
  if !number.is_finite() {
    return None;
  }
  let number = number.trunc();
  if number < 0.0 || number > MAX_SAFE_INTEGER {
    return None;
  }
  Some(number as u64)
}

/// WebIDL `[EnforceRange] long long`. The range is `±(2^53 − 1)`.
#[inline]
pub fn enforce_range_i64(number: f64) -> Option<i64> {
  if !number.is_finite() {
    return None;
  }
  let number = number.trunc();
  if number < -MAX_SAFE_INTEGER || number > MAX_SAFE_INTEGER {
    return None;
  }
  Some(number as i64)
}

/// WebIDL `unsigned long`: non-finite values become `0`, then modulo `2^32`.
#[inline]
pub fn modulo_u32(number: f64) -> u32 {
  if !number.is_finite() || number == 0.0 {
    return 0;
  }
  let modulo = number.trunc() % 4_294_967_296.0;
  let positive = if modulo < 0.0 {
    modulo + 4_294_967_296.0
  } else {
    modulo
  };
  positive as u32
}

/// WebIDL `unsigned long long` without `[EnforceRange]`.
///
/// The residue is the low 64 bits of the truncated integer. A float cast
/// saturates at `2^64`, so the bits come from the IEEE representation.
#[inline]
pub fn modulo_u64(number: f64) -> u64 {
  if !number.is_finite() {
    return 0;
  }
  let number = number.trunc();
  if number == 0.0 {
    return 0;
  }
  let bits = number.to_bits();
  let negative = bits >> 63 != 0;
  let exponent = ((bits >> 52) & 0x7ff) as i32 - 1023;
  let significand = (bits & ((1_u64 << 52) - 1)) | (1_u64 << 52);
  if exponent < 0 {
    return 0;
  }
  if exponent < 52 {
    let magnitude = significand >> (52 - exponent);
    return if negative {
      magnitude.wrapping_neg()
    } else {
      magnitude
    };
  }
  let shift = exponent - 52;
  if shift >= 64 {
    return 0;
  }
  let low = significand.wrapping_shl(shift as u32);
  if negative { low.wrapping_neg() } else { low }
}

/// WebIDL `long long` without `[EnforceRange]`.
#[inline]
pub fn modulo_i64(number: f64) -> i64 {
  modulo_u64(number) as i64
}

/// Owned bytes as an `ArrayBuffer`.
#[inline]
pub fn array_buffer<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  bytes: Vec<u8>,
) -> v8::Local<'s, v8::ArrayBuffer> {
  let store = v8::ArrayBuffer::new_backing_store_from_vec(bytes).make_shared();
  v8::ArrayBuffer::with_backing_store(scope, &store)
}

fn bytes_view<'s, T>(
  scope: &mut v8::PinScope<'s, '_>,
  bytes: Vec<u8>,
  element_size: usize,
  new: impl FnOnce(
    &mut v8::PinScope<'s, '_>,
    v8::Local<'s, v8::ArrayBuffer>,
    usize,
  ) -> Option<v8::Local<'s, T>>,
) -> Option<v8::Local<'s, T>> {
  let length = bytes.len() / element_size;
  let buffer = array_buffer(scope, bytes);
  new(scope, buffer, length)
}

/// Owned bytes as a `Uint8Array`. `None` when V8 cannot allocate the view.
#[inline]
pub fn uint8_array<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  bytes: Vec<u8>,
) -> Option<v8::Local<'s, v8::Uint8Array>> {
  bytes_view(scope, bytes, 1, |scope, buffer, length| {
    v8::Uint8Array::new(scope, buffer, 0, length)
  })
}

/// Owned bytes as a `Uint8ClampedArray`. `None` when V8 cannot allocate it.
#[inline]
pub fn uint8_clamped_array<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  bytes: Vec<u8>,
) -> Option<v8::Local<'s, v8::Uint8ClampedArray>> {
  bytes_view(scope, bytes, 1, |scope, buffer, length| {
    v8::Uint8ClampedArray::new(scope, buffer, 0, length)
  })
}

/// Owned little-endian `f32` bytes as a `Float32Array`.
#[inline]
pub fn float32_array<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  bytes: Vec<u8>,
) -> Option<v8::Local<'s, v8::Float32Array>> {
  bytes_view(scope, bytes, 4, |scope, buffer, length| {
    v8::Float32Array::new(scope, buffer, 0, length)
  })
}

/// Owned little-endian `f64` bytes as a `Float64Array`.
#[inline]
pub fn float64_array<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  bytes: Vec<u8>,
) -> Option<v8::Local<'s, v8::Float64Array>> {
  bytes_view(scope, bytes, 8, |scope, buffer, length| {
    v8::Float64Array::new(scope, buffer, 0, length)
  })
}

/// A view whose `buffer()` is missing.
#[derive(Clone, Copy)]
#[cfg_attr(
  not(any(
    feature = "fs",
    feature = "udp",
    feature = "tcp",
    feature = "http",
    feature = "child",
  )),
  allow(dead_code)
)]
pub(crate) enum MissingViewBuffer {
  /// Copy the view.
  Copy,
  /// Reject with the detached message.
  #[cfg_attr(not(feature = "child"), allow(dead_code))]
  Detached,
}

/// A backing store shorter than the reported length.
#[derive(Clone, Copy)]
#[cfg_attr(
  not(any(
    feature = "fs",
    feature = "udp",
    feature = "tcp",
    feature = "http",
    feature = "child",
  )),
  allow(dead_code)
)]
pub(crate) enum ShortCopy {
  /// Reject with the detached message.
  Error,
  /// Keep the bytes that were present.
  Truncate,
}

/// Limits and messages for [`copy_buffer_source`].
#[derive(Clone, Copy)]
#[cfg_attr(
  not(any(
    feature = "fs",
    feature = "udp",
    feature = "tcp",
    feature = "http",
    feature = "child",
  )),
  allow(dead_code)
)]
pub(crate) struct BufferCopy {
  pub max_bytes: usize,
  pub too_large: &'static str,
  pub detached: &'static str,
  pub not_source: &'static str,
  pub missing_view_buffer: MissingViewBuffer,
  pub short_copy: ShortCopy,
}

#[cfg_attr(
  not(any(
    feature = "fs",
    feature = "udp",
    feature = "tcp",
    feature = "http",
    feature = "child",
  )),
  allow(dead_code)
)]
impl BufferCopy {
  pub(crate) const fn strict(
    max_bytes: usize,
    too_large: &'static str,
    detached: &'static str,
    not_source: &'static str,
  ) -> Self {
    Self {
      max_bytes,
      too_large,
      detached,
      not_source,
      missing_view_buffer: MissingViewBuffer::Copy,
      short_copy: ShortCopy::Error,
    }
  }
}

/// Copies a `BufferSource`.
///
/// `SharedArrayBuffer`, then an `ArrayBufferView`, then an `ArrayBuffer`.
/// Anything else is `not_source`.
#[cfg_attr(
  not(any(
    feature = "fs",
    feature = "udp",
    feature = "tcp",
    feature = "http",
    feature = "child",
  )),
  allow(dead_code)
)]
pub(crate) fn copy_buffer_source(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
  limits: BufferCopy,
) -> Result<Vec<u8>, crate::op::OpError> {
  if let Ok(buffer) = v8::Local::<v8::SharedArrayBuffer>::try_from(value) {
    return copy_shared(&buffer, &limits);
  }
  if let Ok(view) = v8::Local::<v8::ArrayBufferView>::try_from(value) {
    return copy_view(scope, view, &limits);
  }
  if let Ok(buffer) = v8::Local::<v8::ArrayBuffer>::try_from(value) {
    return copy_array(&buffer, &limits);
  }
  Err(crate::op::OpError::type_(limits.not_source))
}

#[cfg_attr(
  not(any(
    feature = "fs",
    feature = "udp",
    feature = "tcp",
    feature = "http",
    feature = "child",
  )),
  allow(dead_code)
)]
fn copy_view(
  scope: &mut v8::PinScope,
  view: v8::Local<v8::ArrayBufferView>,
  limits: &BufferCopy,
) -> Result<Vec<u8>, crate::op::OpError> {
  let detached = match view.buffer(scope) {
    None => matches!(limits.missing_view_buffer, MissingViewBuffer::Detached),
    Some(buffer) => buffer.was_detached(),
  };
  if detached {
    return Err(crate::op::OpError::type_(limits.detached));
  }
  let length = view.byte_length();
  if length > limits.max_bytes {
    return Err(crate::op::OpError::type_(limits.too_large));
  }
  if length == 0 {
    return Ok(Vec::new());
  }
  let mut bytes = vec![0u8; length];
  let written = view.copy_contents(&mut bytes);
  if written != length {
    if matches!(limits.short_copy, ShortCopy::Error) {
      return Err(crate::op::OpError::type_(limits.detached));
    }
    bytes.truncate(written);
  }
  Ok(bytes)
}

#[cfg_attr(
  not(any(
    feature = "fs",
    feature = "udp",
    feature = "tcp",
    feature = "http",
    feature = "child",
  )),
  allow(dead_code)
)]
fn copy_shared(
  buffer: &v8::SharedArrayBuffer,
  limits: &BufferCopy,
) -> Result<Vec<u8>, crate::op::OpError> {
  let length = buffer.byte_length();
  if length > limits.max_bytes {
    return Err(crate::op::OpError::type_(limits.too_large));
  }
  if length == 0 {
    return Ok(Vec::new());
  }
  let mut bytes = vec![0u8; length];
  let store = buffer.get_backing_store();
  let Some(data) = store.data() else {
    return Err(crate::op::OpError::type_(limits.detached));
  };
  let count = length.min(store.byte_length());
  if count != length && matches!(limits.short_copy, ShortCopy::Error) {
    return Err(crate::op::OpError::type_(limits.detached));
  }
  // Safety: `count` fits the destination and the store. The shared
  // reference keeps the store alive for this copy.
  unsafe {
    std::ptr::copy_nonoverlapping(
      data.as_ptr().cast::<u8>(),
      bytes.as_mut_ptr(),
      count,
    );
  }
  bytes.truncate(count);
  Ok(bytes)
}

#[cfg_attr(
  not(any(
    feature = "fs",
    feature = "udp",
    feature = "tcp",
    feature = "http",
    feature = "child",
  )),
  allow(dead_code)
)]
fn copy_array(
  buffer: &v8::ArrayBuffer,
  limits: &BufferCopy,
) -> Result<Vec<u8>, crate::op::OpError> {
  if buffer.was_detached() {
    return Err(crate::op::OpError::type_(limits.detached));
  }
  let length = buffer.byte_length();
  if length > limits.max_bytes {
    return Err(crate::op::OpError::type_(limits.too_large));
  }
  if length == 0 {
    return Ok(Vec::new());
  }
  let mut bytes = vec![0u8; length];
  let Some(data) = buffer.data() else {
    return Err(crate::op::OpError::type_(limits.detached));
  };
  // Safety: `length` is the buffer's byte length and the destination
  // size. `buffer` keeps the store alive for this copy.
  unsafe {
    std::ptr::copy_nonoverlapping(
      data.as_ptr().cast::<u8>(),
      bytes.as_mut_ptr(),
      length,
    );
  }
  Ok(bytes)
}

/// Moves `globalThis[name]` onto `Yuzuko` when the property is present.
#[cfg_attr(
  not(any(
    feature = "fs",
    feature = "udp",
    feature = "tcp",
    feature = "http",
    feature = "child",
    feature = "ffi",
  )),
  allow(dead_code)
)]
pub(crate) fn relocate_constructor(scope: &mut v8::PinScope, name: &str) {
  let global = scope.get_current_context().global(scope);
  let Some(key) = v8::String::new(scope, name) else {
    return;
  };
  let Some(constructor) = global.get(scope, key.into()) else {
    return;
  };
  let _ = global.delete(scope, key.into());
  let yuzuko = namespace(scope, "Yuzuko");
  yuzuko.set(scope, key.into(), constructor);
}

/// Reads `object[name]`. A thrown getter becomes the error. A failed read
/// without an exception is a `TypeError` with `read_message`.
#[cfg(any(
  feature = "tcp",
  feature = "udp",
  feature = "http",
  feature = "ffi",
  feature = "child"
))]
pub(crate) fn read_field<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  object: v8::Local<'_, v8::Object>,
  name: &str,
  read_message: &str,
) -> Result<Option<v8::Local<'s, v8::Value>>, crate::op::OpError> {
  enum Outcome {
    Value(v8::Global<v8::Value>),
    Throw(v8::Global<v8::Value>),
    Missing,
  }
  let outcome = {
    v8::tc_scope!(let tc, scope);
    let Some(key) = v8::String::new(tc, name) else {
      return Err(crate::op::OpError::type_(read_message));
    };
    let object = v8::Local::new(tc, object);
    match object.get(tc, key.into()) {
      Some(value) => Outcome::Value(v8::Global::new(tc, value)),
      None => {
        if let Some(exception) = tc.exception() {
          Outcome::Throw(v8::Global::new(tc, exception))
        } else {
          Outcome::Missing
        }
      }
    }
  };
  match outcome {
    Outcome::Value(value) => Ok(Some(v8::Local::new(scope, value))),
    Outcome::Throw(exception) => Err(crate::op::OpError::thrown(
      scope,
      v8::Local::new(scope, exception),
    )),
    Outcome::Missing => Err(crate::op::OpError::type_(read_message)),
  }
}

/// Own enumerable string keys of `object`, converted to strings. A thrown
/// trap becomes the error. A failed read without an exception is a
/// `TypeError` with `read_message`.
#[cfg(any(feature = "ffi", feature = "child"))]
pub(crate) fn own_enumerable_names<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  object: v8::Local<'_, v8::Object>,
  read_message: &str,
) -> Result<v8::Local<'s, v8::Array>, crate::op::OpError> {
  enum Outcome {
    Names(v8::Global<v8::Array>),
    Throw(v8::Global<v8::Value>),
    Missing,
  }
  let outcome = {
    v8::tc_scope!(let tc, scope);
    let object = v8::Local::new(tc, object);
    let mut builder = v8::GetPropertyNamesArgsBuilder::new();
    let listed = builder
      .mode(v8::KeyCollectionMode::OwnOnly)
      .property_filter(
        v8::PropertyFilter::ONLY_ENUMERABLE | v8::PropertyFilter::SKIP_SYMBOLS,
      )
      .key_conversion(v8::KeyConversionMode::ConvertToString)
      .build();
    match object.get_own_property_names(tc, listed) {
      Some(names) => Outcome::Names(v8::Global::new(tc, names)),
      None => {
        if let Some(exception) = tc.exception() {
          Outcome::Throw(v8::Global::new(tc, exception))
        } else {
          Outcome::Missing
        }
      }
    }
  };
  match outcome {
    Outcome::Names(names) => Ok(v8::Local::new(scope, names)),
    Outcome::Throw(exception) => Err(crate::op::OpError::thrown(
      scope,
      v8::Local::new(scope, exception),
    )),
    Outcome::Missing => Err(crate::op::OpError::type_(read_message)),
  }
}

/// Reads `array[index]`. A thrown getter becomes the error. A failed read
/// without an exception is a `TypeError` with `read_message`.
#[cfg(any(feature = "ffi", feature = "child"))]
pub(crate) fn array_element<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  array: v8::Local<'_, v8::Array>,
  index: u32,
  read_message: &str,
) -> Result<v8::Local<'s, v8::Value>, crate::op::OpError> {
  enum Outcome {
    Value(v8::Global<v8::Value>),
    Throw(v8::Global<v8::Value>),
    Missing,
  }
  let outcome = {
    v8::tc_scope!(let tc, scope);
    let array = v8::Local::new(tc, array);
    match array.get_index(tc, index) {
      Some(value) => Outcome::Value(v8::Global::new(tc, value)),
      None => {
        if let Some(exception) = tc.exception() {
          Outcome::Throw(v8::Global::new(tc, exception))
        } else {
          Outcome::Missing
        }
      }
    }
  };
  match outcome {
    Outcome::Value(value) => Ok(v8::Local::new(scope, value)),
    Outcome::Throw(exception) => Err(crate::op::OpError::thrown(
      scope,
      v8::Local::new(scope, exception),
    )),
    Outcome::Missing => Err(crate::op::OpError::type_(read_message)),
  }
}

/// Whether `value` is an instance of the realm's current `globalThis.URL`.
pub(crate) fn is_url(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> bool {
  let global = scope.get_current_context().global(scope);
  let Some(key) = v8::String::new(scope, "URL") else {
    return false;
  };
  let Some(ctor) = global.get(scope, key.into()) else {
    return false;
  };
  let Ok(ctor) = v8::Local::<v8::Object>::try_from(ctor) else {
    return false;
  };
  value.instance_of(scope, ctor) == Some(true)
}

/// `ToString(object.href)`. A throw or a failed read is `None`; the
/// exception is not rethrown.
pub(crate) fn url_href(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
) -> Option<String> {
  let key = v8::String::new(scope, "href")?;
  v8::tc_scope!(let tc, scope);
  match object.get(tc, key.into()) {
    Some(value) if tc.exception().is_none() => match value.to_string(tc) {
      Some(text) if tc.exception().is_none() => {
        Some(text.to_rust_string_lossy(tc))
      }
      _ => None,
    },
    _ => None,
  }
}

/// `Error` whose message is `message`, or `TypeError` when the text cannot
/// be allocated.
#[cfg_attr(
  not(any(feature = "udp", feature = "http", feature = "ffi")),
  allow(dead_code)
)]
pub(crate) fn thrown_error(
  scope: &mut v8::PinScope,
  message: &str,
) -> crate::op::OpError {
  let Some(text) = v8::String::new(scope, message) else {
    return crate::op::OpError::type_(message);
  };
  let error = v8::Exception::error(scope, text);
  crate::op::OpError::thrown(scope, error)
}

/// `Error`. A failed message allocation yields `undefined`.
#[cfg_attr(
  not(any(feature = "udp", feature = "tcp", feature = "http")),
  allow(dead_code)
)]
pub(crate) fn error_exception<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  message: &str,
) -> v8::Local<'s, v8::Value> {
  generic_error(scope, message, StringFailure::Undefined)
}

/// `TypeError`. A failed message allocation yields `undefined`.
#[cfg_attr(not(feature = "udp"), allow(dead_code))]
pub(crate) fn type_exception<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  message: &str,
) -> v8::Local<'s, v8::Value> {
  builtin_error(scope, message, false, StringFailure::Undefined)
}

/// Primitive string. A string object and any other value are absent.
pub(crate) fn string_primitive(
  scope: &v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Option<String> {
  if !value.is_string() {
    return None;
  }
  let Ok(text) = v8::Local::<v8::String>::try_from(value) else {
    return None;
  };
  Some(text.to_rust_string_lossy(scope))
}

/// Calls `object[name]` when it is a function.
///
/// A missing name, a non-function, or a throw is ignored.
pub fn call_method(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
  args: &[v8::Local<v8::Value>],
) {
  v8::tc_scope!(let scope, scope);
  let object = crate::op::root(scope, object);
  let key = v8::String::new(scope, name).unwrap();
  let Some(value) = object.get(scope, key.into()) else {
    return;
  };
  let Ok(function) = v8::Local::<v8::Function>::try_from(value) else {
    return;
  };
  let function = crate::op::root(scope, function);
  let args = args
    .iter()
    .map(|arg| crate::op::root(scope, *arg))
    .collect::<Vec<_>>();
  let _ = function.call(scope, object.into(), &args);
}

/// What to produce when the realm has no `DOMException` constructor.
#[derive(Clone, Copy)]
pub enum DomFallback {
  /// `TypeError` whose message is the DOM message.
  Type,
  /// `Error` whose message is the DOM message.
  Error,
  /// `undefined`.
  Undefined,
}

/// What to produce when the exception message cannot be allocated.
#[derive(Clone, Copy)]
pub enum StringFailure {
  /// `String::new` failure panics.
  Unwrap,
  /// `String::new` failure yields `undefined`.
  Undefined,
}

/// `Type` becomes `TypeError`, `Range` becomes `RangeError`, and `Dom`
/// becomes a `DOMException`. `Thrown` is that value.
#[inline]
pub fn exception_value<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  error: crate::op::OpError,
  dom_fallback: DomFallback,
  string_failure: StringFailure,
) -> v8::Local<'s, v8::Value> {
  match error {
    crate::op::OpError::Type(message) => {
      builtin_error(scope, &message, false, string_failure)
    }
    crate::op::OpError::Range(message) => {
      builtin_error(scope, &message, true, string_failure)
    }
    crate::op::OpError::Dom { name, message } => {
      match crate::op::new_dom_exception(scope, &name, &message) {
        Some(error) => error.into(),
        None => match dom_fallback {
          DomFallback::Undefined => v8::undefined(scope).into(),
          DomFallback::Type => {
            builtin_error(scope, &message, false, string_failure)
          }
          DomFallback::Error => generic_error(scope, &message, string_failure),
        },
      }
    }
    crate::op::OpError::Thrown(value) => v8::Local::new(scope, *value),
  }
}

fn builtin_error<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  message: &str,
  range: bool,
  string_failure: StringFailure,
) -> v8::Local<'s, v8::Value> {
  let Some(text) = v8::String::new(scope, message) else {
    return string_failed(scope, string_failure);
  };
  if range {
    v8::Exception::range_error(scope, text)
  } else {
    v8::Exception::type_error(scope, text)
  }
}

fn generic_error<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  message: &str,
  string_failure: StringFailure,
) -> v8::Local<'s, v8::Value> {
  let Some(text) = v8::String::new(scope, message) else {
    return string_failed(scope, string_failure);
  };
  v8::Exception::error(scope, text)
}

fn string_failed<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  string_failure: StringFailure,
) -> v8::Local<'s, v8::Value> {
  match string_failure {
    StringFailure::Unwrap => {
      panic!("exception message");
    }
    StringFailure::Undefined => v8::undefined(scope).into(),
  }
}
