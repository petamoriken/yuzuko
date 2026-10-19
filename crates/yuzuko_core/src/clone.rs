//! Host-object records inside `structuredClone`.
//!
//! Feature crates register a writer and a reader. Tags `1` and `2` are
//! `Blob` and `File`. Tag `3` is `DOMException`. Tags `4` through `10` are
//! the geometry interfaces. Tag `11` is `CryptoKey`. Other host objects stay
//! unserializable.

use std::sync::Mutex;

use crate::js;

pub enum WriteResult {
  Skip,
  Wrote,
  Failed,
}

pub enum ReadResult {
  Skip,
  Failed,
  Done(v8::Global<v8::Object>),
}

pub type CloneWriteFn = fn(
  &mut v8::PinScope,
  v8::Local<v8::Object>,
  &dyn v8::ValueSerializerHelper,
) -> WriteResult;

pub type CloneReadFn =
  fn(&mut v8::PinScope, u32, &dyn v8::ValueDeserializerHelper) -> ReadResult;

static WRITERS: Mutex<Vec<CloneWriteFn>> = Mutex::new(Vec::new());
static READERS: Mutex<Vec<CloneReadFn>> = Mutex::new(Vec::new());

thread_local! {
  static STRUCTURED_TRANSFER: std::cell::RefCell<Vec<v8::Global<v8::Object>>> =
    const { std::cell::RefCell::new(Vec::new()) };
  static CROSS_AGENT: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
  static CROSS_TARGET: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// Marks structured clone as a cross-isolate copy aimed at `target`.
pub fn set_cross_agent(enabled: bool, target: u64) {
  CROSS_AGENT.with(|cell| cell.set(enabled));
  CROSS_TARGET.with(|cell| cell.set(target));
}

pub fn cross_agent() -> bool {
  CROSS_AGENT.with(|cell| cell.get())
}

pub fn cross_target() -> u64 {
  CROSS_TARGET.with(|cell| cell.get())
}

pub(crate) fn take_structured_transfer() -> Vec<v8::Global<v8::Object>> {
  STRUCTURED_TRANSFER.with(|slot| std::mem::take(&mut *slot.borrow_mut()))
}

/// Objects the in-flight structured clone should transfer rather than copy.
pub fn set_structured_transfer(list: Vec<v8::Global<v8::Object>>) {
  STRUCTURED_TRANSFER.with(|slot| *slot.borrow_mut() = list);
}

pub fn clear_structured_transfer() {
  STRUCTURED_TRANSFER.with(|slot| slot.borrow_mut().clear());
}

pub fn is_structured_transfer(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
) -> bool {
  STRUCTURED_TRANSFER.with(|slot| {
    slot.borrow().iter().any(|item| {
      let item = v8::Local::new(scope, item);
      item.strict_equals(object.into())
    })
  })
}

pub fn add_host_clone(write: CloneWriteFn, read: CloneReadFn) {
  WRITERS.lock().expect("clone writers").push(write);
  READERS.lock().expect("clone readers").push(read);
}

pub(crate) fn writers() -> Vec<CloneWriteFn> {
  WRITERS.lock().expect("clone writers").clone()
}

pub(crate) fn readers() -> Vec<CloneReadFn> {
  READERS.lock().expect("clone readers").clone()
}

pub(crate) fn write_host_object(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  value_serializer: &dyn v8::ValueSerializerHelper,
) -> Option<bool> {
  for write in writers() {
    match write(scope, object, value_serializer) {
      WriteResult::Skip => {}
      WriteResult::Wrote => return Some(true),
      WriteResult::Failed => return None,
    }
  }
  reject_clone(scope);
  None
}

pub(crate) fn read_host_object<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  value_deserializer: &dyn v8::ValueDeserializerHelper,
) -> Option<v8::Local<'s, v8::Object>> {
  let mut tag = 0u32;
  if !value_deserializer.read_uint32(&mut tag) {
    reject_clone(scope);
    return None;
  }
  for read in readers() {
    match read(scope, tag, value_deserializer) {
      ReadResult::Skip => {}
      ReadResult::Failed => return None,
      ReadResult::Done(object) => {
        return Some(v8::Local::new(scope, object));
      }
    }
  }
  reject_clone(scope);
  None
}

pub fn reject_clone(scope: &mut v8::PinScope) {
  throw_data_clone(scope, "The object could not be cloned.");
}

pub(crate) fn throw_data_clone(scope: &mut v8::PinScope, message: &str) {
  let global = js::global(scope);
  let key = v8::String::new(scope, "DOMException").unwrap();
  if let Some(ctor) = global.get(scope, key.into())
    && let Ok(ctor) = v8::Local::<v8::Function>::try_from(ctor)
  {
    let message = v8::String::new(scope, message).unwrap();
    let name = v8::String::new(scope, "DataCloneError").unwrap();
    if let Some(error) =
      ctor.new_instance(scope, &[message.into(), name.into()])
    {
      scope.throw_exception(error.into());
      return;
    }
  }
  js::throw_type(scope, message);
}

pub fn write_bytes(helper: &dyn v8::ValueSerializerHelper, bytes: &[u8]) {
  helper.write_uint64(bytes.len() as u64);
  if !bytes.is_empty() {
    helper.write_raw_bytes(bytes);
  }
}

pub fn read_bytes(helper: &dyn v8::ValueDeserializerHelper) -> Option<Vec<u8>> {
  let mut len = 0u64;
  if !helper.read_uint64(&mut len) {
    return None;
  }
  let len = usize::try_from(len).ok()?;
  if len == 0 {
    return Some(Vec::new());
  }
  Some(helper.read_raw_bytes(len)?.to_vec())
}

pub fn write_text(helper: &dyn v8::ValueSerializerHelper, text: &str) -> bool {
  let bytes = text.as_bytes();
  let Ok(len) = u32::try_from(bytes.len()) else {
    return false;
  };
  helper.write_uint32(len);
  if len > 0 {
    helper.write_raw_bytes(bytes);
  }
  true
}

pub fn read_text(helper: &dyn v8::ValueDeserializerHelper) -> Option<String> {
  let mut len = 0u32;
  if !helper.read_uint32(&mut len) {
    return None;
  }
  if len == 0 {
    return Some(String::new());
  }
  let bytes = helper.read_raw_bytes(len as usize)?;
  String::from_utf8(bytes.to_vec()).ok()
}
