//! Process WebGPU devices. They share the leaked wgpu device with Canvas 2D.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread::ThreadId;

use yuzuko_core::op::OpError;

pub struct Handle {
  pub device: wgpu::Device,
  pub queue: wgpu::Queue,
  pub lost: Arc<AtomicBool>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ScopeKind {
  Validation,
  OutOfMemory,
  Internal,
}

impl ScopeKind {
  pub fn wgpu(self) -> wgpu::ErrorFilter {
    match self {
      Self::Validation => wgpu::ErrorFilter::Validation,
      Self::OutOfMemory => wgpu::ErrorFilter::OutOfMemory,
      Self::Internal => wgpu::ErrorFilter::Internal,
    }
  }
}

pub struct ScopeCapture {
  pub kind: ScopeKind,
  pub message: String,
}

pub struct ErrorScope {
  pub kind: ScopeKind,
  pub synthetic: Option<String>,
  /// A wgpu error taken while this scope was moved out from under another
  /// device. The re-pushed guard is empty.
  pub captured: Option<ScopeCapture>,
}

/// Guards for every JavaScript device, in the order they were pushed.
///
/// The process has one wgpu device, so a scope has to be popped newest
/// first even when several JavaScript devices still hold scopes. Dropping a
/// device's guard vector oldest-first panics inside wgpu.
struct ScopeStack {
  guards: Vec<(u64, wgpu::ErrorScopeGuard)>,
}

impl Drop for ScopeStack {
  fn drop(&mut self) {
    while let Some((_, guard)) = self.guards.pop() {
      drop(guard.pop());
    }
  }
}

pub struct DisplacedScope {
  pub id: u64,
  pub guard: wgpu::ErrorScopeGuard,
  pub kind: ScopeKind,
}

/// Newer scopes from other devices, newest first.
///
/// Drop pops these guards before the caller's own guard. wgpu panics if an
/// older scope is popped first.
pub struct DisplacedScopes {
  items: Vec<DisplacedScope>,
}

impl DisplacedScopes {
  fn push(&mut self, item: DisplacedScope) {
    self.items.push(item);
  }

  fn taken(&self, id: u64) -> usize {
    self.items.iter().filter(|item| item.id == id).count()
  }

  pub fn take(&mut self) -> Vec<DisplacedScope> {
    std::mem::take(&mut self.items)
  }
}

impl Drop for DisplacedScopes {
  fn drop(&mut self) {
    for item in self.items.drain(..) {
      drop(item.guard.pop());
    }
  }
}

pub struct PoppedScope {
  /// Dropped before `guard`.
  pub displaced: DisplacedScopes,
  pub guard: wgpu::ErrorScopeGuard,
  pub kind: ScopeKind,
  pub synthetic: Option<String>,
  pub captured: Option<ScopeCapture>,
}

pub struct LiveDevice {
  pub device: wgpu::Device,
  pub queue: wgpu::Queue,
  pub lost: Arc<AtomicBool>,
  pub lost_promise: v8::Global<v8::Promise>,
  pub lost_resolver: v8::Global<v8::PromiseResolver>,
  pub scopes: Vec<ErrorScope>,
  pub queue_object: Option<v8::Global<v8::Object>>,
  pub features_object: Option<v8::Global<v8::Object>>,
  pub limits_object: Option<v8::Global<v8::Object>>,
  pub adapter_info: Option<v8::Global<v8::Object>>,
  pub owner: Option<String>,
  /// Core defaults, raised by own `requiredLimits` properties.
  pub reported_limits: Vec<f64>,
  /// `maxBufferSize` from `reported_limits`.
  pub max_buffer_size: f64,
}

#[derive(Clone, Copy)]
pub enum CachedObject {
  Features,
  Limits,
  AdapterInfo,
}

thread_local! {
  static DEVICES: RefCell<HashMap<u64, LiveDevice>> =
    RefCell::new(HashMap::new());
  static SCOPES: RefCell<ScopeStack> =
    const { RefCell::new(ScopeStack { guards: Vec::new() }) };
}

static NEXT_DEVICE: AtomicU64 = AtomicU64::new(1);

pub fn alloc_id() -> u64 {
  NEXT_DEVICE.fetch_add(1, Ordering::Relaxed)
}

pub fn insert(id: u64, live: LiveDevice) {
  DEVICES.with(|devices| {
    devices.borrow_mut().insert(id, live);
  });
}

pub fn reported_limit(id: u64, index: usize) -> Result<f64, OpError> {
  DEVICES.with(|devices| {
    let devices = devices.borrow();
    let live = devices.get(&id).ok_or_else(lost_error)?;
    live
      .reported_limits
      .get(index)
      .copied()
      .ok_or_else(|| OpError::type_("The limit is unknown"))
  })
}

pub fn max_buffer_size(id: u64) -> Result<f64, OpError> {
  DEVICES.with(|devices| {
    let devices = devices.borrow();
    let live = devices.get(&id).ok_or_else(lost_error)?;
    Ok(live.max_buffer_size)
  })
}

pub fn handle(id: u64) -> Result<Handle, OpError> {
  let handle = DEVICES.with(|devices| {
    let devices = devices.borrow();
    let live = devices.get(&id).ok_or_else(lost_error)?;
    if live.lost.load(Ordering::SeqCst) {
      return Err(lost_error());
    }
    Ok(Handle {
      device: live.device.clone(),
      queue: live.queue.clone(),
      lost: Arc::clone(&live.lost),
    })
  })?;
  note_gpu_entry();
  Ok(handle)
}

pub fn with_mut<T>(
  id: u64,
  body: impl FnOnce(&mut LiveDevice) -> Result<T, OpError>,
) -> Result<T, OpError> {
  DEVICES.with(|devices| {
    let mut devices = devices.borrow_mut();
    let live = devices.get_mut(&id).ok_or_else(lost_error)?;
    if live.lost.load(Ordering::SeqCst) {
      return Err(lost_error());
    }
    body(live)
  })
}

pub fn lost_promise(id: u64) -> Option<v8::Global<v8::Promise>> {
  DEVICES.with(|devices| {
    devices
      .borrow()
      .get(&id)
      .map(|live| v8::Global::clone(&live.lost_promise))
  })
}

pub fn destroy(scope: &mut v8::PinScope, id: u64) {
  let resolver = DEVICES.with(|devices| {
    let mut devices = devices.borrow_mut();
    let live = devices.get_mut(&id)?;
    if live.lost.swap(true, Ordering::SeqCst) {
      return None;
    }
    Some(live.lost_resolver.clone())
  });
  let Some(resolver) = resolver else {
    return;
  };
  let resolver = v8::Local::new(scope, &resolver);
  let info = lost_info(scope, "destroyed");
  resolver.resolve(scope, info);
}

pub fn push_scope(
  id: u64,
  guard: wgpu::ErrorScopeGuard,
  kind: ScopeKind,
) -> Result<(), OpError> {
  with_mut(id, |live| {
    live.scopes.push(ErrorScope {
      kind,
      synthetic: None,
      captured: None,
    });
    Ok(())
  })?;
  SCOPES.with(|scopes| {
    scopes.borrow_mut().guards.push((id, guard));
  });
  Ok(())
}

/// Pushes a wgpu scope for a JavaScript scope that is already on the stack.
pub fn repush_scope(id: u64, kind: ScopeKind) -> Result<(), OpError> {
  let device = wgpu_device(id)?;
  let guard = device.push_error_scope(kind.wgpu());
  SCOPES.with(|scopes| {
    scopes.borrow_mut().guards.push((id, guard));
  });
  Ok(())
}

/// Records `capture` on the scope `depth_from_top` below the newest.
/// The first error wins.
pub fn store_captured(id: u64, depth_from_top: usize, capture: ScopeCapture) {
  DEVICES.with(|devices| {
    let mut devices = devices.borrow_mut();
    let Some(live) = devices.get_mut(&id) else {
      return;
    };
    let Some(index) = live.scopes.len().checked_sub(depth_from_top + 1) else {
      return;
    };
    if let Some(scope) = live.scopes.get_mut(index)
      && scope.captured.is_none()
    {
      scope.captured = Some(capture);
    }
  });
}

/// The wgpu device, including after `destroy`. Error scopes still have to be
/// popped, because every JavaScript device shares one wgpu device.
pub fn wgpu_device(id: u64) -> Result<wgpu::Device, OpError> {
  DEVICES.with(|devices| {
    devices
      .borrow()
      .get(&id)
      .map(|live| live.device.clone())
      .ok_or_else(lost_error)
  })
}

pub fn device_is_lost(id: u64) -> bool {
  DEVICES.with(|devices| {
    devices
      .borrow()
      .get(&id)
      .is_none_or(|live| live.lost.load(Ordering::SeqCst))
  })
}

pub fn pop_scope(id: u64) -> Result<PoppedScope, OpError> {
  let none =
    || OpError::dom("OperationError", "There is no error scope on the stack");
  let has_scope = DEVICES.with(|devices| {
    let devices = devices.borrow();
    let live = devices.get(&id).ok_or_else(lost_error)?;
    Ok(!live.scopes.is_empty())
  })?;
  if !has_scope {
    return Err(none());
  }
  // One wgpu device, so every JavaScript scope shares one stack. Pop newer
  // scopes from other devices first. The caller saves their errors and
  // pushes those scopes again.
  let mut displaced = DisplacedScopes { items: Vec::new() };
  let guard = loop {
    let Some((owner, guard)) =
      SCOPES.with(|scopes| scopes.borrow_mut().guards.pop())
    else {
      return Err(none());
    };
    if owner == id {
      break guard;
    }
    let kind = DEVICES.with(|devices| {
      let devices = devices.borrow();
      let taken = displaced.taken(owner);
      devices.get(&owner).and_then(|live| {
        live
          .scopes
          .len()
          .checked_sub(taken + 1)
          .and_then(|index| live.scopes.get(index).map(|scope| scope.kind))
      })
    });
    let Some(kind) = kind else {
      // The device is gone. This guard is the newest one, so pop it now.
      drop(guard.pop());
      continue;
    };
    displaced.push(DisplacedScope {
      id: owner,
      guard,
      kind,
    });
  };
  let (kind, synthetic, captured) = DEVICES
    .with(|devices| {
      devices.borrow_mut().get_mut(&id).and_then(|live| {
        live
          .scopes
          .pop()
          .map(|scope| (scope.kind, scope.synthetic, scope.captured))
      })
    })
    .ok_or_else(none)?;
  Ok(PoppedScope {
    displaced,
    guard,
    kind,
    synthetic,
    captured,
  })
}

/// Record one validation error on the device timeline.
///
/// The innermost validation scope keeps the first synthetic message. A later
/// error for that scope is dropped. With no matching scope, the error is
/// uncaptured.
pub fn report_validation(id: u64, message: &str) {
  report_synthetic(id, ScopeKind::Validation, message);
}

pub fn report_out_of_memory(id: u64, message: &str) {
  report_synthetic(id, ScopeKind::OutOfMemory, message);
}

fn report_synthetic(id: u64, kind: ScopeKind, message: &str) {
  let captured = DEVICES.with(|devices| {
    let mut devices = devices.borrow_mut();
    let Some(live) = devices.get_mut(&id) else {
      return false;
    };
    if live.lost.load(Ordering::SeqCst) {
      return false;
    }
    for scope in live.scopes.iter_mut().rev() {
      if scope.kind != kind {
        continue;
      }
      if scope.synthetic.is_none() {
        scope.synthetic = Some(message.to_string());
      }
      return true;
    }
    false
  });
  if !captured {
    let kind = match kind {
      ScopeKind::Validation => UncapturedKind::Validation,
      ScopeKind::OutOfMemory => UncapturedKind::OutOfMemory,
      ScopeKind::Internal => UncapturedKind::Internal,
    };
    queue_uncaptured(kind, message.to_string());
  }
}

pub fn queue_object(id: u64) -> Result<v8::Global<v8::Object>, OpError> {
  DEVICES.with(|devices| {
    let devices = devices.borrow();
    let live = devices.get(&id).ok_or_else(lost_error)?;
    live
      .queue_object
      .clone()
      .ok_or_else(|| OpError::dom("InvalidStateError", "The GPU queue is gone"))
  })
}

pub fn cached_object(
  id: u64,
  kind: CachedObject,
) -> Result<Option<v8::Global<v8::Object>>, OpError> {
  DEVICES.with(|devices| {
    let devices = devices.borrow();
    let live = devices.get(&id).ok_or_else(lost_error)?;
    if live.lost.load(Ordering::SeqCst) {
      return Err(lost_error());
    }
    let slot = match kind {
      CachedObject::Features => &live.features_object,
      CachedObject::Limits => &live.limits_object,
      CachedObject::AdapterInfo => &live.adapter_info,
    };
    Ok(slot.clone())
  })
}

/// Stores `object` the first time. A later call returns the stored object.
pub fn store_cached(
  id: u64,
  kind: CachedObject,
  object: v8::Global<v8::Object>,
) -> Result<v8::Global<v8::Object>, OpError> {
  DEVICES.with(|devices| {
    let mut devices = devices.borrow_mut();
    let live = devices.get_mut(&id).ok_or_else(lost_error)?;
    if live.lost.load(Ordering::SeqCst) {
      return Err(lost_error());
    }
    let slot = match kind {
      CachedObject::Features => &mut live.features_object,
      CachedObject::Limits => &mut live.limits_object,
      CachedObject::AdapterInfo => &mut live.adapter_info,
    };
    if slot.is_none() {
      *slot = Some(object);
    }
    slot.clone().ok_or_else(lost_error)
  })
}

pub fn clear() {
  SCOPES.with(|scopes| {
    let mut scopes = scopes.borrow_mut();
    while let Some((_, guard)) = scopes.guards.pop() {
      drop(guard.pop());
    }
  });
  DEVICES.with(|devices| devices.borrow_mut().clear());
  WRAPPERS.with(|wrappers| wrappers.borrow_mut().clear());
  pending_errors().clear();
}

/// Drops devices created by a module this reload evaluates again.
///
/// The lost promise is not resolved. Later use reports the device as lost,
/// the same way [`clear`] drops the map.
pub fn drop_reloaded() {
  DEVICES.with(|devices| {
    devices.borrow_mut().retain(|_, live| {
      live
        .owner
        .as_deref()
        .is_none_or(|owner| !yuzuko_core::module_reloaded(owner))
    });
  });
}

fn lost_info<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  reason: &str,
) -> v8::Local<'s, v8::Value> {
  let object = v8::Object::new(scope);
  let reason_key = v8::String::new(scope, "reason").unwrap();
  let reason_value = v8::String::new(scope, reason).unwrap();
  object.set(scope, reason_key.into(), reason_value.into());
  let message_key = v8::String::new(scope, "message").unwrap();
  let message = v8::String::new(scope, "The device was destroyed").unwrap();
  object.set(scope, message_key.into(), message.into());
  object.into()
}

fn lost_error() -> OpError {
  OpError::dom("InvalidStateError", "The GPU device is lost")
}

pub fn skip_gpu() -> bool {
  std::env::var("YUZUKO_SKIP_GPU").ok().as_deref() == Some("1")
}

pub fn ensure_adapter() -> bool {
  if skip_gpu() {
    return false;
  }
  if !yuzuko_window::ensure_gpu() {
    return false;
  }
  let _ = yuzuko_window::with_gpu(|_adapter, device, _queue| {
    install_uncaptured(device);
  });
  note_gpu_entry();
  true
}

#[derive(Clone, Copy)]
pub enum UncapturedKind {
  Validation,
  OutOfMemory,
  Internal,
}

struct PendingError {
  kind: UncapturedKind,
  message: String,
}

static ISOLATE: Mutex<Option<ThreadId>> = Mutex::new(None);
static PENDING: Mutex<Vec<PendingError>> = Mutex::new(Vec::new());

thread_local! {
  static WRAPPERS: RefCell<HashMap<u64, v8::Weak<v8::Object>>> =
    RefCell::new(HashMap::new());
  static DRAINING: Cell<bool> = const { Cell::new(false) };
  static DRAIN_QUEUED: Cell<bool> = const { Cell::new(false) };
}

pub fn register_wrapper(
  scope: &mut v8::PinScope,
  id: u64,
  object: v8::Local<v8::Object>,
) {
  let unit: &mut v8::PinScope<'_, '_, ()> = &mut *scope;
  let isolate: &mut v8::Isolate = &mut *unit;
  let weak = v8::Weak::with_guaranteed_finalizer(
    isolate,
    object,
    Box::new(move || {
      WRAPPERS.with(|wrappers| {
        wrappers.borrow_mut().remove(&id);
      });
    }),
  );
  WRAPPERS.with(|wrappers| {
    wrappers.borrow_mut().insert(id, weak);
  });
}

pub fn wrapper_ids() -> Vec<u64> {
  WRAPPERS.with(|wrappers| wrappers.borrow().keys().copied().collect())
}

pub fn wrapper_local<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  id: u64,
) -> Option<v8::Local<'s, v8::Object>> {
  let weak = WRAPPERS.with(|wrappers| wrappers.borrow().get(&id).cloned())?;
  let view: &v8::PinScope<'s, '_, ()> = scope;
  weak.to_local(view)
}

fn install_uncaptured(device: &wgpu::Device) {
  use std::sync::Once;
  static ONCE: Once = Once::new();
  ONCE.call_once(|| {
    *isolate_thread() = Some(std::thread::current().id());
    device.on_uncaptured_error(Arc::new(record_uncaptured));
  });
}

fn record_uncaptured(error: wgpu::Error) {
  let message = error.to_string();
  let kind = match error {
    wgpu::Error::OutOfMemory { .. } => UncapturedKind::OutOfMemory,
    wgpu::Error::Internal { .. } => UncapturedKind::Internal,
    wgpu::Error::Validation { .. } => UncapturedKind::Validation,
  };
  queue_uncaptured(kind, message);
}

fn queue_uncaptured(kind: UncapturedKind, message: String) {
  pending_errors().push(PendingError { kind, message });
  if on_isolate_thread() {
    schedule_drain();
  }
}

fn note_gpu_entry() {
  if on_isolate_thread() && !pending_errors().is_empty() {
    schedule_drain();
  }
}

fn schedule_drain() {
  if DRAINING.with(Cell::get) {
    return;
  }
  if DRAIN_QUEUED.with(|queued| queued.replace(true)) {
    return;
  }
  yuzuko_core::enqueue(|scope| {
    DRAIN_QUEUED.with(|queued| queued.set(false));
    drain_uncaptured(scope);
  });
}

fn drain_uncaptured(scope: &mut v8::PinScope) {
  if DRAINING.with(|draining| draining.replace(true)) {
    return;
  }
  loop {
    let batch: Vec<PendingError> = pending_errors().drain(..).collect();
    if batch.is_empty() {
      break;
    }
    for error in batch {
      crate::device::dispatch_uncaptured(scope, error.kind, error.message);
    }
  }
  DRAINING.with(|draining| draining.set(false));
  if !pending_errors().is_empty() {
    schedule_drain();
  }
}

fn pending_errors() -> std::sync::MutexGuard<'static, Vec<PendingError>> {
  PENDING.lock().unwrap_or_else(|error| error.into_inner())
}

fn isolate_thread() -> std::sync::MutexGuard<'static, Option<ThreadId>> {
  ISOLATE.lock().unwrap_or_else(|error| error.into_inner())
}

fn on_isolate_thread() -> bool {
  *isolate_thread() == Some(std::thread::current().id())
}
