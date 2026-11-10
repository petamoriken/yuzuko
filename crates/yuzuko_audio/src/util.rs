//! Dictionary, promise, and PCM view helpers.

use yuzuko_core::op::OpError;

pub fn member<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  object: v8::Local<'_, v8::Object>,
  name: &str,
) -> Option<v8::Local<'s, v8::Value>> {
  let key = v8::String::new(scope, name)?;
  let value = object.get(scope, key.into())?;
  if value.is_undefined() {
    None
  } else {
    Some(value)
  }
}

pub fn rust_string(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> String {
  value
    .to_string(scope)
    .map(|text| text.to_rust_string_lossy(scope))
    .unwrap_or_default()
}

pub fn finite_number(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<f64, OpError> {
  if value.is_null() {
    return Err(OpError::type_("The value is null"));
  }
  let number = value
    .number_value(scope)
    .ok_or_else(|| OpError::type_("The value is not a number"))?;
  if !number.is_finite() {
    return Err(OpError::type_("The value is not finite"));
  }
  Ok(number)
}

pub fn dom_exception<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  name: &str,
  message: &str,
) -> v8::Local<'s, v8::Value> {
  yuzuko_core::op::new_dom_exception(scope, name, message)
    .map(|error| error.into())
    .unwrap_or_else(|| {
      let text = v8::String::new(scope, message).unwrap();
      v8::Exception::error(scope, text)
    })
}

pub fn f32_array<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  samples: &[f32],
) -> v8::Local<'s, v8::Float32Array> {
  let bytes = f32_to_bytes(samples);
  let store = v8::ArrayBuffer::new_backing_store_from_vec(bytes).make_shared();
  let buffer = v8::ArrayBuffer::with_backing_store(scope, &store);
  v8::Float32Array::new(scope, buffer, 0, samples.len()).unwrap()
}

pub fn read_f32_view(
  _scope: &mut v8::PinScope,
  view: v8::Local<v8::Float32Array>,
) -> Vec<f32> {
  let mut bytes = vec![0u8; view.byte_length()];
  let written = view.copy_contents(&mut bytes);
  bytes.truncate(written);
  bytes_to_f32(&bytes)
}

pub fn f32_to_bytes(samples: &[f32]) -> Vec<u8> {
  let mut bytes = vec![0u8; samples.len() * 4];
  if samples.is_empty() {
    return bytes;
  }
  unsafe {
    std::ptr::copy_nonoverlapping(
      samples.as_ptr().cast::<u8>(),
      bytes.as_mut_ptr(),
      bytes.len(),
    );
  }
  bytes
}

pub fn write_f32_view(
  scope: &mut v8::PinScope,
  view: v8::Local<v8::Float32Array>,
  samples: &[f32],
) {
  let count = samples.len().min(view.length());
  write_bytes(
    scope,
    view.into(),
    view.byte_offset(),
    &f32_to_bytes(&samples[..count]),
  );
}

pub fn write_u8_view(
  scope: &mut v8::PinScope,
  view: v8::Local<v8::Uint8Array>,
  samples: &[u8],
) {
  let count = samples.len().min(view.length());
  write_bytes(scope, view.into(), view.byte_offset(), &samples[..count]);
}

fn write_bytes(
  scope: &mut v8::PinScope,
  view: v8::Local<v8::ArrayBufferView>,
  offset: usize,
  bytes: &[u8],
) {
  if bytes.is_empty() {
    return;
  }
  let Some(buffer) = view.buffer(scope) else {
    return;
  };
  let Some(data) = buffer.data() else {
    return;
  };
  unsafe {
    std::ptr::copy_nonoverlapping(
      bytes.as_ptr(),
      data.as_ptr().cast::<u8>().add(offset),
      bytes.len(),
    );
  }
}

pub fn bytes_to_f32(bytes: &[u8]) -> Vec<f32> {
  let count = bytes.len() / 4;
  let mut samples = vec![0f32; count];
  if count == 0 {
    return samples;
  }
  unsafe {
    std::ptr::copy_nonoverlapping(
      bytes.as_ptr(),
      samples.as_mut_ptr().cast::<u8>(),
      count * 4,
    );
  }
  samples
}

/// `value` is a sequence or a Float32Array of finite floats.
pub fn read_f32_sequence(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<Vec<f32>, OpError> {
  if value.is_null_or_undefined() {
    return Ok(Vec::new());
  }
  if let Ok(view) = v8::Local::<v8::Float32Array>::try_from(value) {
    let samples = read_f32_view(scope, view);
    if samples.iter().any(|sample| !sample.is_finite()) {
      return Err(OpError::type_("The coefficient is not finite"));
    }
    return Ok(samples);
  }
  if let Ok(array) = v8::Local::<v8::Array>::try_from(value) {
    let mut samples = Vec::with_capacity(array.length() as usize);
    for index in 0..array.length() {
      let item = array.get_index(scope, index).ok_or_else(|| {
        OpError::type_("The coefficient list is not a sequence")
      })?;
      let number = finite_number(scope, item)?;
      samples.push(number as f32);
    }
    return Ok(samples);
  }
  read_iterable_f32(scope, value)
}

/// Web IDL sequence conversion. A `Float64Array` is not a JavaScript `Array`.
fn read_iterable_f32(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<Vec<f32>, OpError> {
  let object = v8::Local::<v8::Object>::try_from(value)
    .map_err(|_| OpError::type_("The coefficient list is not a sequence"))?;
  let symbol = v8::Symbol::get_iterator(scope);
  let Some(next) = object
    .get(scope, symbol.into())
    .and_then(|value| v8::Local::<v8::Function>::try_from(value).ok())
  else {
    return Err(OpError::type_("The coefficient list is not a sequence"));
  };
  let Some(iterator) = next
    .call(scope, object.into(), &[])
    .and_then(|value| v8::Local::<v8::Object>::try_from(value).ok())
  else {
    return Err(OpError::type_("The coefficient list is not a sequence"));
  };
  let next_key = v8::String::new(scope, "next").unwrap();
  let done_key = v8::String::new(scope, "done").unwrap();
  let value_key = v8::String::new(scope, "value").unwrap();
  let mut samples = Vec::new();
  loop {
    if samples.len() > 1024 {
      return Err(OpError::type_("The coefficient list is not a sequence"));
    }
    let Some(step) = iterator
      .get(scope, next_key.into())
      .and_then(|value| v8::Local::<v8::Function>::try_from(value).ok())
      .and_then(|step| step.call(scope, iterator.into(), &[]))
      .and_then(|value| v8::Local::<v8::Object>::try_from(value).ok())
    else {
      return Err(OpError::type_("The coefficient list is not a sequence"));
    };
    let done = step
      .get(scope, done_key.into())
      .is_some_and(|value| value.boolean_value(scope));
    if done {
      return Ok(samples);
    }
    let Some(item) = step.get(scope, value_key.into()) else {
      return Err(OpError::type_("The coefficient list is not a sequence"));
    };
    samples.push(finite_number(scope, item)? as f32);
  }
}
