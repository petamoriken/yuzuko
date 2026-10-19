//! Process WebGPU devices. They share the leaked wgpu device with Canvas 2D.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use yuzuko_core::op::OpError;

pub struct Handle {
  pub device: wgpu::Device,
  pub queue: wgpu::Queue,
  pub lost: Arc<AtomicBool>,
}

pub struct LiveDevice {
  pub device: wgpu::Device,
  pub queue: wgpu::Queue,
  pub lost: Arc<AtomicBool>,
  pub lost_promise: v8::Global<v8::Promise>,
  pub lost_resolver: v8::Global<v8::PromiseResolver>,
  pub scopes: Vec<wgpu::ErrorScopeGuard>,
  pub queue_object: Option<v8::Global<v8::Object>>,
}

thread_local! {
  static DEVICES: RefCell<HashMap<u64, LiveDevice>> =
    RefCell::new(HashMap::new());
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

pub fn handle(id: u64) -> Result<Handle, OpError> {
  DEVICES.with(|devices| {
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
  })
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
) -> Result<(), OpError> {
  with_mut(id, |live| {
    live.scopes.push(guard);
    Ok(())
  })
}

pub fn pop_scope(id: u64) -> Result<wgpu::ErrorScopeGuard, OpError> {
  DEVICES.with(|devices| {
    let mut devices = devices.borrow_mut();
    let live = devices.get_mut(&id).ok_or_else(lost_error)?;
    if live.lost.load(Ordering::SeqCst) {
      return Err(lost_error());
    }
    live.scopes.pop().ok_or_else(|| {
      OpError::dom("OperationError", "There is no error scope on the stack")
    })
  })
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

pub fn clear() {
  DEVICES.with(|devices| devices.borrow_mut().clear());
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
  true
}

fn install_uncaptured(device: &wgpu::Device) {
  use std::sync::Once;
  static ONCE: Once = Once::new();
  ONCE.call_once(|| {
    device.on_uncaptured_error(Arc::new(|error| {
      eprintln!("yuzuko: uncaptured WebGPU error: {error}");
    }));
  });
}
