//! Cross-isolate structured clone used by `postMessage`.
//!
//! `SharedArrayBuffer` keeps its backing store. A transferred `ArrayBuffer`
//! moves that store and is detached on the sender. The receiver builds a new
//! `ArrayBuffer` over the same store, so `resizable` and `maxByteLength`
//! stay. Host objects use the registered clone writers.

use std::cell::RefCell;
use std::rc::Rc;

use v8::ValueDeserializerHelper;
use v8::ValueSerializerHelper;

use crate::clone::{self, cross_agent};

/// `SharedRef<BackingStore>` is not `Send` in rusty_v8. The C++
/// `shared_ptr` and the bytes are safe to move onto the receiving isolate's
/// thread. A transferred `ArrayBuffer` is detached before the parcel is
/// sent, so the receiver is the next JavaScript owner of that store.
pub struct SharedStore(v8::SharedRef<v8::BackingStore>);

unsafe impl Send for SharedStore {}

impl SharedStore {
  pub fn new(store: v8::SharedRef<v8::BackingStore>) -> Self {
    Self(store)
  }

  pub fn backing(&self) -> &v8::SharedRef<v8::BackingStore> {
    &self.0
  }

  fn data_ptr(&self) -> Option<usize> {
    self.0.data().map(|ptr| ptr.as_ptr() as usize)
  }
}

/// Drops parked host allocations the receiver did not keep.
struct Releases(Vec<Box<dyn FnOnce() + Send>>);

impl Drop for Releases {
  fn drop(&mut self) {
    for release in self.0.drain(..) {
      release();
    }
  }
}

pub struct Parcel {
  pub bytes: Vec<u8>,
  /// Transferred `ArrayBuffer` stores, keyed by the serializer's transfer id.
  pub buffers: Vec<(u32, SharedStore)>,
  pub shared: Vec<SharedStore>,
  /// Host pixel and sample allocations. Dropped with the parcel, after a
  /// successful read has cloned the ones it kept. Nothing reads the field:
  /// `Releases` runs when the parcel drops.
  #[allow(dead_code)]
  releases: Releases,
  /// Wire bytes for each `WebAssembly.Module` in this parcel.
  ///
  /// The serializer writes only an id. The receiver is another isolate, so
  /// the bytes travel with the parcel and are compiled there. Ids are
  /// 1-based indexes into this list.
  wasm: Vec<Vec<u8>>,
}

/// Owns the resource frame for one `serialize_parcel`.
///
/// A failure runs the releases before the function returns, so the source
/// objects stay intact. Success moves the releases onto the parcel.
struct ResourceGuard {
  armed: bool,
}

impl ResourceGuard {
  fn open() -> Self {
    clone::begin_resource_frame();
    Self { armed: true }
  }

  fn commit(mut self) -> Vec<Box<dyn FnOnce() + Send>> {
    self.armed = false;
    clone::end_resource_frame(false)
  }
}

impl Drop for ResourceGuard {
  fn drop(&mut self) {
    if self.armed {
      clone::end_resource_frame(true);
    }
  }
}

struct CrossHost {
  shared: Rc<RefCell<Vec<SharedStore>>>,
  /// Wire bytes kept until `serialize_parcel` moves them onto the parcel.
  wasm: Rc<RefCell<Vec<Vec<u8>>>>,
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

  fn get_wasm_module_transfer_id(
    &self,
    _scope: &mut v8::PinScope<'_, '_>,
    module: v8::Local<v8::WasmModuleObject>,
  ) -> Option<u32> {
    // Returning `None` writes nothing and drops the module. Keep the bytes
    // so the other agent can compile a new module. A transfer list still
    // rejects the module before this callback.
    let compiled = module.get_compiled_module();
    let bytes = compiled.get_wire_bytes_ref().to_vec();
    drop(compiled);
    let mut modules = self.wasm.borrow_mut();
    let id = u32::try_from(modules.len().checked_add(1)?).ok()?;
    modules.push(bytes);
    Some(id)
  }
}

struct CrossRead {
  shared: Vec<SharedStore>,
  wasm: Vec<Vec<u8>>,
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

  fn get_wasm_module_from_id<'s>(
    &self,
    scope: &mut v8::PinScope<'s, '_>,
    clone_id: u32,
  ) -> Option<v8::Local<'s, v8::WasmModuleObject>> {
    let index = usize::try_from(clone_id).ok()?.checked_sub(1)?;
    let bytes = self.wasm.get(index)?;
    v8::WasmModuleObject::compile(scope, bytes)
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
/// `ArrayBuffer`s whose backing stores move. They detach after a successful
/// write.
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
  let resources = ResourceGuard::open();
  let shared = Rc::new(RefCell::new(Vec::new()));
  let wasm = Rc::new(RefCell::new(Vec::new()));
  let moved = {
    v8::tc_scope!(let tc, scope);
    let serializer = v8::ValueSerializer::new(
      tc,
      Box::new(CrossHost {
        shared: Rc::clone(&shared),
        wasm: Rc::clone(&wasm),
      }),
    );
    serializer.write_header();
    let mut moved = Vec::new();
    for (index, buffer) in buffers.iter().enumerate() {
      let buffer = v8::Local::new(tc, buffer);
      if buffer.was_detached() || !buffer.is_detachable() {
        clone::throw_data_clone(tc, "The ArrayBuffer cannot be transferred");
        return Err(tc.exception().map(|thrown| v8::Global::new(tc, thrown)));
      }
      let Ok(id) = u32::try_from(index) else {
        clone::throw_data_clone(tc, "The ArrayBuffer cannot be transferred");
        return Err(tc.exception().map(|thrown| v8::Global::new(tc, thrown)));
      };
      moved.push((id, SharedStore::new(buffer.get_backing_store())));
      serializer.transfer_array_buffer(id, buffer);
    }
    let context = tc.get_current_context();
    if serializer.write_value(context, value).is_none() {
      return Err(tc.exception().map(|thrown| v8::Global::new(tc, thrown)));
    }
    if !crate::realm::transfers_still_attached(tc, buffers) {
      return Err(tc.exception().map(|thrown| v8::Global::new(tc, thrown)));
    }
    let bytes = serializer.release();
    (bytes, moved)
  };
  let releases = resources.commit();
  clone::detach_structured_transfers(scope);
  for buffer in buffers {
    let buffer = v8::Local::new(scope, buffer);
    let _ = buffer.detach(None);
  }
  let shared = std::mem::take(&mut *shared.borrow_mut());
  let wasm = std::mem::take(&mut *wasm.borrow_mut());
  Ok(Parcel {
    bytes: moved.0,
    buffers: moved.1,
    shared,
    releases: Releases(releases),
    wasm,
  })
}

pub fn deserialize_parcel<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  parcel: Parcel,
) -> Result<v8::Local<'s, v8::Value>, Option<v8::Global<v8::Value>>> {
  // Same rule as `clone_value`: finish only after the deserializer is gone.
  // A port queue calls this again from inside `ReadHostObject`. That nested
  // exit must not run JavaScript.
  let session = clone::CloneSession::enter();
  let value = {
    v8::tc_scope!(let tc, scope);
    let wasm = parcel.wasm;
    let deserializer = v8::ValueDeserializer::new(
      tc,
      Box::new(CrossRead {
        shared: parcel.shared,
        wasm,
      }),
      &parcel.bytes,
    );
    for (id, store) in &parcel.buffers {
      let buffer = v8::ArrayBuffer::with_backing_store(tc, store.backing());
      deserializer.transfer_array_buffer(*id, buffer);
    }
    let context = tc.get_current_context();
    if deserializer.read_header(context).is_none() {
      Err(tc.exception().map(|thrown| v8::Global::new(tc, thrown)))
    } else {
      match clone::read_host_value(tc, |_| deserializer.read_value(context)) {
        clone::HostRead::Done(value) => Ok(v8::Global::new(tc, value)),
        clone::HostRead::Rejected(message) => {
          tc.reset();
          clone::throw_data_clone(tc, &message);
          Err(tc.exception().map(|thrown| v8::Global::new(tc, thrown)))
        }
        clone::HostRead::Failed => {
          Err(tc.exception().map(|thrown| v8::Global::new(tc, thrown)))
        }
      }
    }
  };
  let success = value.is_ok();
  session.exit(scope, success)?;
  match value {
    Ok(value) => Ok(v8::Local::new(scope, value)),
    Err(error) => Err(error),
  }
}
