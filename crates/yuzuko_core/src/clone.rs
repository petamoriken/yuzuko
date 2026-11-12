//! Host-object records inside `structuredClone`.
//!
//! Feature crates register a writer and a reader. Tags `1` and `2` are
//! `Blob` and `File`. Tag `3` is `DOMException`. Tags `4` through `10` are
//! the geometry interfaces. Tag `11` is `CryptoKey`. Tag `12` is
//! `MessagePort` and tag `13` is `OffscreenCanvas`; both are transfer-only.
//! Tag `14` is `ImageBitmap`. Tag `15` is `ImageData`. Tag `16` is
//! `AudioData`. Tag `17` is `VideoFrame`. Tags `18` and `19` are
//! `EncodedAudioChunk` and `EncodedVideoChunk`. Tag `20` is
//! `ReadableStream`, tag `21` is `WritableStream`, and tag `22` is
//! `TransformStream`. All three are transfer-only. Tag `23` is
//! `ModuleSource`. A transferable also
//! registers a probe. `ImageBitmap`, `AudioData`, and `VideoFrame` register
//! a detacher that runs after a successful serialize. Finishers run when a
//! clone leaves V8. A nested read can still be inside `ReadHostObject`, so
//! JavaScript runs only from the outermost finisher.

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

/// Whether a host object may appear in a transfer list.
pub enum TransferStatus {
  Skip,
  Ready,
  Detached,
}

/// How a transfer list treats an `ArrayBuffer` view.
#[derive(Clone, Copy)]
pub enum TransferKind {
  /// `postMessage` moves the view's underlying buffer.
  Message,
  /// `structuredClone` accepts transferable platform objects only.
  StructuredClone,
  /// An owning stream uses the stream duplicate message.
  Owning,
}

pub enum TransferReject {
  /// The runtime already has a pending exception.
  Pending,
  DataClone(&'static str),
}

pub struct TransferList {
  pub objects: Vec<v8::Global<v8::Object>>,
  pub buffers: Vec<v8::Global<v8::ArrayBuffer>>,
}

pub type TransferProbeFn =
  fn(&mut v8::PinScope, v8::Local<v8::Object>) -> TransferStatus;

pub type HostDetachFn = fn(&mut v8::PinScope, v8::Local<v8::Object>);

static PROBES: Mutex<Vec<TransferProbeFn>> = Mutex::new(Vec::new());
static DETACHERS: Mutex<Vec<HostDetachFn>> = Mutex::new(Vec::new());

/// Restores the previous transfer list when dropped.
#[must_use]
pub struct StructuredTransferGuard {
  previous: Vec<v8::Global<v8::Object>>,
}

impl StructuredTransferGuard {
  pub fn install(next: Vec<v8::Global<v8::Object>>) -> Self {
    let previous = take_structured_transfer();
    set_structured_transfer(next);
    Self { previous }
  }
}

impl Drop for StructuredTransferGuard {
  fn drop(&mut self) {
    set_structured_transfer(std::mem::take(&mut self.previous));
  }
}

thread_local! {
  static STRUCTURED_TRANSFER: std::cell::RefCell<Vec<v8::Global<v8::Object>>> =
    const { std::cell::RefCell::new(Vec::new()) };
  static CROSS_AGENT: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
  static CROSS_TARGET: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
  static CLONE_DEPTH: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
  /// Wire bytes for `WebAssembly.Module` values cloned in the current session.
  /// Serializer and deserializer are different host objects, so the bytes live
  /// here. Each session pushes a frame. Ids are 1-based indexes into that frame.
  static WASM_TRANSFER: std::cell::RefCell<Vec<Vec<Vec<u8>>>> =
    const { std::cell::RefCell::new(Vec::new()) };
  /// Backing stores for `SharedArrayBuffer` values in the current session.
  /// Ids are 0-based. Recording happens only while [`AllowSharedArrayBufferClone`]
  /// is entered, so `structuredClone` still rejects a shared buffer.
  static SAB_TRANSFER: std::cell::RefCell<
    Vec<Vec<v8::SharedRef<v8::BackingStore>>>,
  > = const { std::cell::RefCell::new(Vec::new()) };
  static SHARE_SAB: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
  /// Depth of `ValueDeserializer::read_value`. `ReadObject` forbids JavaScript.
  static HOST_READ: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
  /// `DataCloneError` message recorded while `HOST_READ` is non-zero.
  /// The outermost read throws it after V8 allows JavaScript again.
  static DEFERRED_CLONE_ERROR: std::cell::RefCell<Option<String>> =
    const { std::cell::RefCell::new(None) };
}

/// Aligned embedder slot for "this context installed `Blob` and `File`".
///
/// The public index sits past V8's internal slots. No other embedder data
/// uses it. `delete globalThis.Blob` leaves the mark in place. A worklet
/// context never sets it, so a `Blob` there fails the read.
const BLOB_REALM_SLOT: i32 = 0;
static BLOB_REALM_MARK: usize = 0;

/// Records that the current context can deserialize `Blob` and `File`.
pub fn mark_blob_realm(scope: &mut v8::PinScope) {
  let context = scope.get_current_context();
  unsafe {
    context.set_aligned_pointer_in_embedder_data(
      BLOB_REALM_SLOT,
      std::ptr::from_ref(&BLOB_REALM_MARK)
        .cast::<std::ffi::c_void>()
        .cast_mut(),
    );
  }
}

pub fn blob_realm(scope: &mut v8::PinScope) -> bool {
  let context = scope.get_current_context();
  let mark = context.get_aligned_pointer_from_embedder_data(BLOB_REALM_SLOT);
  mark
    == std::ptr::from_ref(&BLOB_REALM_MARK)
      .cast::<std::ffi::c_void>()
      .cast_mut()
}

struct HostReadGuard;

impl HostReadGuard {
  fn enter() -> Self {
    HOST_READ.with(|cell| {
      let depth = cell.get();
      if depth == 0 {
        DEFERRED_CLONE_ERROR.with(|slot| *slot.borrow_mut() = None);
      }
      cell.set(depth.saturating_add(1));
    });
    Self
  }
}

impl Drop for HostReadGuard {
  fn drop(&mut self) {
    HOST_READ.with(|cell| cell.set(cell.get().saturating_sub(1)));
  }
}

fn host_read_depth() -> u32 {
  HOST_READ.with(|cell| cell.get())
}

fn note_deferred_clone_error(message: &str) {
  DEFERRED_CLONE_ERROR.with(|slot| {
    let mut deferred = slot.borrow_mut();
    if deferred.is_none() {
      *deferred = Some(message.to_string());
    }
  });
}

fn take_deferred_clone_error() -> Option<String> {
  if host_read_depth() > 0 {
    return None;
  }
  DEFERRED_CLONE_ERROR.with(|slot| slot.borrow_mut().take())
}

pub(crate) enum HostRead<'s> {
  Done(v8::Local<'s, v8::Value>),
  /// The outermost read should throw `DataCloneError` with this message.
  Rejected(String),
  Failed,
}

/// Runs `read` while host callbacks must not construct `DOMException`.
pub(crate) fn read_host_value<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  read: impl FnOnce(&mut v8::PinScope<'s, '_>) -> Option<v8::Local<'s, v8::Value>>,
) -> HostRead<'s> {
  let value = {
    let _guard = HostReadGuard::enter();
    read(scope)
  };
  if let Some(message) = take_deferred_clone_error() {
    HostRead::Rejected(message)
  } else if let Some(value) = value {
    HostRead::Done(value)
  } else {
    HostRead::Failed
  }
}

pub(crate) fn begin_wasm_transfer() {
  WASM_TRANSFER.with(|slot| slot.borrow_mut().push(Vec::new()));
}

pub(crate) fn end_wasm_transfer() {
  WASM_TRANSFER.with(|slot| {
    slot.borrow_mut().pop();
  });
}

/// Stores module wire bytes for this session. `0` means there is no frame.
pub(crate) fn note_wasm_module(bytes: Vec<u8>) -> u32 {
  WASM_TRANSFER.with(|slot| {
    let mut frames = slot.borrow_mut();
    let Some(frame) = frames.last_mut() else {
      return 0;
    };
    frame.push(bytes);
    frame.len() as u32
  })
}

pub(crate) fn wasm_module_bytes(id: u32) -> Option<Vec<u8>> {
  if id == 0 {
    return None;
  }
  WASM_TRANSFER.with(|slot| {
    let frames = slot.borrow();
    let frame = frames.last()?;
    frame.get(id as usize - 1).cloned()
  })
}

pub(crate) fn begin_sab_transfer() {
  SAB_TRANSFER.with(|slot| slot.borrow_mut().push(Vec::new()));
}

pub(crate) fn end_sab_transfer() {
  SAB_TRANSFER.with(|slot| {
    slot.borrow_mut().pop();
  });
}

/// Lets `MessagePort.postMessage` share a `SharedArrayBuffer` with its peer.
///
/// `structuredClone` and `processorOptions` leave this unset, so those paths
/// still fail the clone. The depth restores on drop, including a nested post.
pub struct AllowSharedArrayBufferClone {
  previous: u32,
}

impl AllowSharedArrayBufferClone {
  pub fn enter() -> Self {
    let previous = SHARE_SAB.with(|cell| {
      let depth = cell.get();
      cell.set(depth.saturating_add(1));
      depth
    });
    Self { previous }
  }
}

impl Drop for AllowSharedArrayBufferClone {
  fn drop(&mut self) {
    SHARE_SAB.with(|cell| cell.set(self.previous));
  }
}

pub(crate) fn note_shared_array_buffer(
  buffer: &v8::SharedArrayBuffer,
) -> Option<u32> {
  if SHARE_SAB.with(|cell| cell.get()) == 0 {
    return None;
  }
  let store = v8::SharedArrayBuffer::get_backing_store(buffer);
  let ptr = store.data().map(|data| data.as_ptr() as usize);
  SAB_TRANSFER.with(|slot| {
    let mut frames = slot.borrow_mut();
    let frame = frames.last_mut()?;
    if let Some(ptr) = ptr
      && let Some(index) = frame.iter().position(|item| {
        item.data().map(|data| data.as_ptr() as usize) == Some(ptr)
      })
    {
      return Some(index as u32);
    }
    frame.push(store);
    Some((frame.len() - 1) as u32)
  })
}

pub(crate) fn shared_array_buffer_from_id<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  id: u32,
) -> Option<v8::Local<'s, v8::SharedArrayBuffer>> {
  SAB_TRANSFER.with(|slot| {
    let frames = slot.borrow();
    let store = frames.last()?.get(id as usize)?;
    Some(v8::SharedArrayBuffer::with_backing_store(scope, store))
  })
}

/// Called when `clone_value` or `deserialize_parcel` begins, including a
/// nested call.
pub type CloneEnterFn = fn();

/// Drops bookkeeping when a clone session unwinds before `exit`.
pub type CloneDiscardFn = fn();

/// Called when a structured clone leaves V8.
///
/// `success` is false when that clone failed. `outermost` is false while a
/// nested `deserialize_parcel` can still be inside `ReadHostObject`; run
/// JavaScript only when `outermost` is true.
pub type CloneFinisher = fn(
  &mut v8::PinScope,
  bool,
  bool,
) -> Result<(), Option<v8::Global<v8::Value>>>;

static ENTERS: Mutex<Vec<CloneEnterFn>> = Mutex::new(Vec::new());
static DISCARDS: Mutex<Vec<CloneDiscardFn>> = Mutex::new(Vec::new());
static FINISHERS: Mutex<Vec<CloneFinisher>> = Mutex::new(Vec::new());

pub fn add_clone_enter(enter: CloneEnterFn) {
  ENTERS.lock().expect("clone enters").push(enter);
}

pub fn add_clone_discard(discard: CloneDiscardFn) {
  DISCARDS.lock().expect("clone discards").push(discard);
}

pub fn add_clone_finisher(finisher: CloneFinisher) {
  FINISHERS.lock().expect("clone finishers").push(finisher);
}

fn enter_hooks() -> Vec<CloneEnterFn> {
  ENTERS.lock().expect("clone enters").clone()
}

fn discard_hooks() -> Vec<CloneDiscardFn> {
  DISCARDS.lock().expect("clone discards").clone()
}

fn finisher_hooks() -> Vec<CloneFinisher> {
  FINISHERS.lock().expect("clone finishers").clone()
}

/// Pairs clone depth with finishers. Dropped sessions discard without
/// running JavaScript.
#[must_use]
pub(crate) struct CloneSession {
  live: bool,
}

impl CloneSession {
  pub(crate) fn enter() -> Self {
    CLONE_DEPTH.with(|cell| cell.set(cell.get().saturating_add(1)));
    begin_wasm_transfer();
    begin_sab_transfer();
    for enter in enter_hooks() {
      enter();
    }
    Self { live: true }
  }

  pub(crate) fn exit(
    mut self,
    scope: &mut v8::PinScope,
    success: bool,
  ) -> Result<(), Option<v8::Global<v8::Value>>> {
    self.live = false;
    end_wasm_transfer();
    end_sab_transfer();
    let depth = CLONE_DEPTH.with(|cell| {
      let next = cell.get().saturating_sub(1);
      cell.set(next);
      next
    });
    let outermost = depth == 0;
    for finisher in finisher_hooks() {
      finisher(scope, success, outermost)?;
    }
    Ok(())
  }
}

impl Drop for CloneSession {
  fn drop(&mut self) {
    if !self.live {
      return;
    }
    self.live = false;
    end_wasm_transfer();
    end_sab_transfer();
    CLONE_DEPTH.with(|cell| cell.set(cell.get().saturating_sub(1)));
    for discard in discard_hooks() {
      discard();
    }
  }
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

pub fn add_host_transfer(probe: TransferProbeFn) {
  PROBES.lock().expect("clone probes").push(probe);
}

pub fn add_host_detach(detach: HostDetachFn) {
  DETACHERS.lock().expect("clone detachers").push(detach);
}

fn probes() -> Vec<TransferProbeFn> {
  PROBES.lock().expect("clone probes").clone()
}

fn detachers() -> Vec<HostDetachFn> {
  DETACHERS.lock().expect("clone detachers").clone()
}

/// First registered probe that recognizes `object` wins.
pub fn host_transfer_status(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
) -> TransferStatus {
  for probe in probes() {
    match probe(scope, object) {
      TransferStatus::Skip => {}
      status => return status,
    }
  }
  TransferStatus::Skip
}

/// Closes transferred `ImageBitmap`, `AudioData`, and `VideoFrame` values.
///
/// Call this after serialization succeeds and before the transfer list is
/// restored. `MessagePort` and `OffscreenCanvas` move inside their writers.
pub fn detach_structured_transfers(scope: &mut v8::PinScope) {
  let objects = STRUCTURED_TRANSFER.with(|slot| slot.borrow().clone());
  let detachers = detachers();
  for object in objects {
    let object = v8::Local::new(scope, object);
    for detach in &detachers {
      detach(scope, object);
    }
  }
}

/// Splits a transfer list into host objects and `ArrayBuffer`s.
pub fn collect_transfers(
  scope: &mut v8::PinScope,
  list: v8::Local<v8::Array>,
  kind: TransferKind,
) -> Result<TransferList, TransferReject> {
  let mut objects = Vec::new();
  let mut buffers = Vec::new();
  for index in 0..list.length() {
    let Some(item) = list.get_index(scope, index) else {
      return Err(TransferReject::Pending);
    };
    if item.is_null_or_undefined() {
      return Err(TransferReject::DataClone("The value cannot be transferred"));
    }
    if item.is_shared_array_buffer() {
      let message = match kind {
        TransferKind::Message => "A SharedArrayBuffer cannot be transferred",
        TransferKind::StructuredClone | TransferKind::Owning => {
          "The value cannot be transferred"
        }
      };
      return Err(TransferReject::DataClone(message));
    }
    if let Ok(buffer) = v8::Local::<v8::ArrayBuffer>::try_from(item) {
      push_buffer(scope, &mut buffers, buffer, kind)?;
      continue;
    }
    if let Ok(view) = v8::Local::<v8::ArrayBufferView>::try_from(item) {
      match kind {
        TransferKind::Message => {
          let Some(buffer) = view.buffer(scope) else {
            return Err(TransferReject::DataClone(
              "The ArrayBuffer cannot be transferred",
            ));
          };
          if buffer.is_shared_array_buffer() {
            return Err(TransferReject::DataClone(
              "A SharedArrayBuffer cannot be transferred",
            ));
          }
          push_buffer(scope, &mut buffers, buffer, kind)?;
          continue;
        }
        TransferKind::StructuredClone | TransferKind::Owning => {
          return Err(TransferReject::DataClone(
            "The value cannot be transferred",
          ));
        }
      }
    }
    let Ok(object) = v8::Local::<v8::Object>::try_from(item) else {
      return Err(TransferReject::DataClone("The value cannot be transferred"));
    };
    if !object.is_api_wrapper() {
      return Err(TransferReject::DataClone("The value cannot be transferred"));
    }
    match host_transfer_status(scope, object) {
      TransferStatus::Skip => {
        return Err(TransferReject::DataClone(
          "The value cannot be transferred",
        ));
      }
      TransferStatus::Detached => {
        return Err(TransferReject::DataClone(
          "The object could not be cloned.",
        ));
      }
      TransferStatus::Ready => {}
    }
    if objects.iter().any(|existing| {
      let existing: v8::Local<v8::Object> = v8::Local::new(scope, existing);
      existing.strict_equals(object.into())
    }) {
      let message = match kind {
        TransferKind::Owning => "The value is already in the transfer list",
        TransferKind::Message | TransferKind::StructuredClone => {
          "The object is already in the transfer list"
        }
      };
      return Err(TransferReject::DataClone(message));
    }
    objects.push(v8::Global::new(scope, object));
  }
  Ok(TransferList { objects, buffers })
}

fn push_buffer(
  scope: &mut v8::PinScope,
  buffers: &mut Vec<v8::Global<v8::ArrayBuffer>>,
  buffer: v8::Local<v8::ArrayBuffer>,
  kind: TransferKind,
) -> Result<(), TransferReject> {
  if buffer.was_detached() || !buffer.is_detachable() {
    return Err(TransferReject::DataClone(
      "The ArrayBuffer cannot be transferred",
    ));
  }
  if buffers.iter().any(|existing| {
    let existing: v8::Local<v8::ArrayBuffer> = v8::Local::new(scope, existing);
    existing.strict_equals(buffer.into())
  }) {
    let message = match kind {
      TransferKind::Owning => "The value is already in the transfer list",
      TransferKind::Message | TransferKind::StructuredClone => {
        "The ArrayBuffer is already in the transfer list"
      }
    };
    return Err(TransferReject::DataClone(message));
  }
  buffers.push(v8::Global::new(scope, buffer));
  Ok(())
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
  if host_read_depth() > 0 {
    // `ValueDeserializer::ReadObject` opens `DisallowJavascriptExecution`.
    // `DOMException`'s constructor captures a stack through `Function::Call`
    // and that aborts the isolate. The caller returns failure; V8 then raises
    // its own deserialization error without running JavaScript. The outermost
    // `read_host_value` replaces that error with `DataCloneError`.
    note_deferred_clone_error(message);
    return;
  }
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
