#[inline]
pub fn array_buffer<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  bytes: Vec<u8>,
) -> v8::Local<'s, v8::ArrayBuffer> {
  yuzuko_core::array_buffer(scope, bytes)
}

#[inline]
pub fn uint8_array<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  bytes: Vec<u8>,
) -> v8::Local<'s, v8::Uint8Array> {
  yuzuko_core::uint8_array(scope, bytes).unwrap()
}

#[inline]
pub fn buffer_source(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<Option<Vec<u8>>, yuzuko_core::op::OpError> {
  // Snapshot shared memory before later writes can change it.
  // A SharedArrayBuffer cannot be detached.
  if let Ok(buffer) = v8::Local::<v8::SharedArrayBuffer>::try_from(value) {
    return Ok(Some(shared_bytes(&buffer)));
  }
  if let Ok(view) = v8::Local::<v8::ArrayBufferView>::try_from(value) {
    // A detached view copies as the empty byte sequence.
    if view
      .buffer(scope)
      .is_none_or(|buffer| buffer.was_detached())
    {
      return Ok(Some(Vec::new()));
    }
    let mut bytes = vec![0u8; view.byte_length()];
    let written = view.copy_contents(&mut bytes);
    bytes.truncate(written);
    return Ok(Some(bytes));
  }
  if let Ok(buffer) = v8::Local::<v8::ArrayBuffer>::try_from(value) {
    if buffer.was_detached() {
      return Ok(Some(Vec::new()));
    }
    let length = buffer.byte_length();
    let mut bytes = vec![0u8; length];
    if let Some(data) = buffer.data() {
      unsafe {
        std::ptr::copy_nonoverlapping(
          data.as_ptr().cast::<u8>(),
          bytes.as_mut_ptr(),
          length,
        );
      }
    }
    return Ok(Some(bytes));
  }
  Ok(None)
}

#[inline]
fn shared_bytes(buffer: &v8::SharedArrayBuffer) -> Vec<u8> {
  let length = buffer.byte_length();
  let mut bytes = vec![0u8; length];
  if length == 0 {
    return bytes;
  }
  let store = buffer.get_backing_store();
  let Some(data) = store.data() else {
    return bytes;
  };
  // The store stays alive for this copy, and `length` is its current size.
  unsafe {
    std::ptr::copy_nonoverlapping(
      data.as_ptr().cast::<u8>(),
      bytes.as_mut_ptr(),
      length,
    );
  }
  bytes
}

#[inline]
pub fn string_value(
  scope: &v8::PinScope,
  value: v8::Local<v8::Value>,
) -> String {
  yuzuko_core::rust_string_or(scope, value, "")
}
