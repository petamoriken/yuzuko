//! Cross-isolate structured clone used by `postMessage`.
//!
//! `SharedArrayBuffer` keeps its backing store. A transferred `ArrayBuffer`
//! is copied and detached on the sender. Host objects use the registered
//! clone writers.

use std::cell::RefCell;
use std::rc::Rc;

use v8::ValueDeserializerHelper;
use v8::ValueSerializerHelper;

use crate::clone::{self, cross_agent};

/// `SharedRef<BackingStore>` is not `Send` in rusty_v8. The C++
/// `shared_ptr` and the shared bytes are safe to move onto the receiving
/// isolate's thread.
pub struct SharedStore(v8::SharedRef<v8::BackingStore>);

unsafe impl Send for SharedStore {}

impl SharedStore {
  fn new(store: v8::SharedRef<v8::BackingStore>) -> Self {
    Self(store)
  }

  pub fn backing(&self) -> &v8::SharedRef<v8::BackingStore> {
    &self.0
  }

  fn data_ptr(&self) -> Option<usize> {
    self.0.data().map(|ptr| ptr.as_ptr() as usize)
  }
}

#[derive(Default)]
pub struct Parcel {
  pub bytes: Vec<u8>,
  pub buffers: Vec<(u32, Vec<u8>)>,
  pub shared: Vec<SharedStore>,
}

struct CrossHost {
  shared: Rc<RefCell<Vec<SharedStore>>>,
}

impl v8::ValueSerializerImpl for CrossHost {
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

  fn get_shared_array_buffer_id<'s>(
    &self,
    _scope: &mut v8::PinScope<'s, '_>,
    shared_array_buffer: v8::Local<'s, v8::SharedArrayBuffer>,
  ) -> Option<u32> {
    let store = v8::SharedArrayBuffer::get_backing_store(&shared_array_buffer);
    let wrapped = SharedStore::new(store);
    let mut list = self.shared.borrow_mut();
    if let Some(ptr) = wrapped.data_ptr()
      && let Some(index) =
        list.iter().position(|item| item.data_ptr() == Some(ptr))
    {
      return Some(index as u32);
    }
    list.push(wrapped);
    Some((list.len() - 1) as u32)
  }
}

struct CrossRead {
  shared: Vec<SharedStore>,
}

impl v8::ValueDeserializerImpl for CrossRead {
  fn read_host_object<'s>(
    &self,
    scope: &mut v8::PinScope<'s, '_>,
    value_deserializer: &dyn v8::ValueDeserializerHelper,
  ) -> Option<v8::Local<'s, v8::Object>> {
    clone::read_host_object(scope, value_deserializer)
  }

  fn get_shared_array_buffer_from_id<'s>(
    &self,
    scope: &mut v8::PinScope<'s, '_>,
    transfer_id: u32,
  ) -> Option<v8::Local<'s, v8::SharedArrayBuffer>> {
    let store = self.shared.get(transfer_id as usize)?;
    Some(v8::SharedArrayBuffer::with_backing_store(
      scope,
      store.backing(),
    ))
  }
}

struct CrossGuard {
  enabled: bool,
  target: u64,
  transfer: Vec<v8::Global<v8::Object>>,
}

impl Drop for CrossGuard {
  fn drop(&mut self) {
    clone::set_cross_agent(self.enabled, self.target);
    clone::set_structured_transfer(std::mem::take(&mut self.transfer));
  }
}

/// Serializes `value` for another isolate.
///
/// `objects` are host objects in the transfer list. `buffers` are
/// `ArrayBuffer`s to detach after a successful write.
pub fn serialize_parcel(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
  objects: &[v8::Global<v8::Object>],
  buffers: &[v8::Global<v8::ArrayBuffer>],
  target_agent: u64,
) -> Result<Parcel, Option<v8::Global<v8::Value>>> {
  let _guard = CrossGuard {
    enabled: cross_agent(),
    target: clone::cross_target(),
    transfer: clone::take_structured_transfer(),
  };
  clone::set_cross_agent(true, target_agent);
  clone::set_structured_transfer(objects.to_vec());
  let shared = Rc::new(RefCell::new(Vec::new()));
  let copied = {
    v8::tc_scope!(let tc, scope);
    let serializer = v8::ValueSerializer::new(
      tc,
      Box::new(CrossHost {
        shared: Rc::clone(&shared),
      }),
    );
    serializer.write_header();
    let mut copied = Vec::new();
    for (index, buffer) in buffers.iter().enumerate() {
      let buffer = v8::Local::new(tc, buffer);
      if buffer.was_detached() {
        clone::throw_data_clone(tc, "The ArrayBuffer cannot be transferred");
        return Err(tc.exception().map(|thrown| v8::Global::new(tc, thrown)));
      }
      copied.push((index as u32, copy_buffer(buffer)));
      serializer.transfer_array_buffer(index as u32, buffer);
    }
    let context = tc.get_current_context();
    if serializer.write_value(context, value).is_none() {
      return Err(tc.exception().map(|thrown| v8::Global::new(tc, thrown)));
    }
    let bytes = serializer.release();
    (bytes, copied)
  };
  for buffer in buffers {
    let buffer = v8::Local::new(scope, buffer);
    let _ = buffer.detach(None);
  }
  let shared = std::mem::take(&mut *shared.borrow_mut());
  Ok(Parcel {
    bytes: copied.0,
    buffers: copied.1,
    shared,
  })
}

pub fn deserialize_parcel<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  parcel: Parcel,
) -> Result<v8::Local<'s, v8::Value>, Option<v8::Global<v8::Value>>> {
  let value = {
    v8::tc_scope!(let tc, scope);
    let deserializer = v8::ValueDeserializer::new(
      tc,
      Box::new(CrossRead {
        shared: parcel.shared,
      }),
      &parcel.bytes,
    );
    for (id, bytes) in &parcel.buffers {
      let buffer = buffer_from_bytes(tc, bytes);
      deserializer.transfer_array_buffer(*id, buffer);
    }
    let context = tc.get_current_context();
    if deserializer.read_header(context).is_none() {
      return Err(tc.exception().map(|thrown| v8::Global::new(tc, thrown)));
    }
    let Some(value) = deserializer.read_value(context) else {
      return Err(tc.exception().map(|thrown| v8::Global::new(tc, thrown)));
    };
    v8::Global::new(tc, value)
  };
  Ok(v8::Local::new(scope, value))
}

fn copy_buffer(buffer: v8::Local<v8::ArrayBuffer>) -> Vec<u8> {
  let length = buffer.byte_length();
  if length == 0 {
    return Vec::new();
  }
  let Some(ptr) = buffer.data() else {
    return vec![0; length];
  };
  let bytes =
    unsafe { std::slice::from_raw_parts(ptr.as_ptr() as *const u8, length) };
  bytes.to_vec()
}

fn buffer_from_bytes<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  bytes: &[u8],
) -> v8::Local<'s, v8::ArrayBuffer> {
  let buffer = v8::ArrayBuffer::new(scope, bytes.len());
  if !bytes.is_empty()
    && let Some(ptr) = buffer.data()
  {
    unsafe {
      std::ptr::copy_nonoverlapping(
        bytes.as_ptr(),
        ptr.as_ptr() as *mut u8,
        bytes.len(),
      );
    }
  }
  buffer
}
