//! The process wgpu device shared by Canvas 2D and window presentation.
//!
//! wgpu 29 does not expose `SurfaceColorSpace`. Presentation therefore picks
//! an explicit unorm8 swapchain format and never a float16 surface.

use std::cell::{Cell, RefCell};
use std::sync::Mutex;
use std::thread::ThreadId;

struct SharedGpu {
  instance: wgpu::Instance,
  adapter: wgpu::Adapter,
  device: wgpu::Device,
  queue: wgpu::Queue,
}

thread_local! {
  // Leaked for the process. Dropping wgpu during thread-local teardown
  // panics because other locals are already gone.
  static GPU: RefCell<Option<&'static SharedGpu>> = const { RefCell::new(None) };
  static FAILED: Cell<bool> = const { Cell::new(false) };
}

static OWNER: Mutex<Option<ThreadId>> = Mutex::new(None);

/// The wgpu device already belongs to another thread.
pub fn foreign() -> bool {
  OWNER
    .lock()
    .expect("gpu owner")
    .is_some_and(|owner| owner != std::thread::current().id())
}

pub fn failed() -> bool {
  FAILED.with(Cell::get)
}

pub fn ensure() -> bool {
  if foreign() || failed() {
    return false;
  }
  if GPU.with(|slot| slot.borrow().is_some()) {
    return true;
  }
  let mut owner_slot = OWNER.lock().expect("gpu owner");
  if owner_slot.is_some() && *owner_slot != Some(std::thread::current().id()) {
    return false;
  }
  match create() {
    Ok(gpu) => {
      let gpu = Box::leak(Box::new(gpu));
      GPU.with(|slot| *slot.borrow_mut() = Some(gpu));
      *owner_slot = Some(std::thread::current().id());
      true
    }
    Err(message) => {
      FAILED.with(|flag| flag.set(true));
      if std::env::var("YUZUKO_CANVAS").ok().as_deref() == Some("gpu") {
        eprintln!("yuzuko: canvas renderer is cpu: {message}");
      }
      false
    }
  }
}

pub fn with_device<T>(
  body: impl FnOnce(&wgpu::Device, &wgpu::Queue) -> T,
) -> Option<T> {
  GPU.with(|slot| {
    let guard = slot.borrow();
    let gpu = guard.as_ref()?;
    Some(body(&gpu.device, &gpu.queue))
  })
}

pub fn with_gpu<T>(
  body: impl FnOnce(&wgpu::Adapter, &wgpu::Device, &wgpu::Queue) -> T,
) -> Option<T> {
  GPU.with(|slot| {
    let guard = slot.borrow();
    let gpu = guard.as_ref()?;
    Some(body(&gpu.adapter, &gpu.device, &gpu.queue))
  })
}

pub(crate) fn cloned_instance() -> Option<wgpu::Instance> {
  GPU.with(|slot| slot.borrow().as_ref().map(|gpu| gpu.instance.clone()))
}

pub(crate) fn cloned_parts()
-> Option<(wgpu::Adapter, wgpu::Device, wgpu::Queue)> {
  GPU.with(|slot| {
    let guard = slot.borrow();
    let gpu = guard.as_ref()?;
    Some((gpu.adapter.clone(), gpu.device.clone(), gpu.queue.clone()))
  })
}

/// Instance options for the process device and window surfaces.
///
/// A debug build leaves out debug labels unless `WGPU_DEBUG` asks for them.
/// lavapipe in Mesa 21 advertises `VK_EXT_debug_utils` without the command
/// buffer label functions, and wgpu aborts when it records a label there.
pub(crate) fn instance_descriptor() -> wgpu::InstanceDescriptor {
  let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
  descriptor.flags = (wgpu::InstanceFlags::from_build_config()
    - wgpu::InstanceFlags::DEBUG)
    .with_env();
  descriptor
}

/// The limits requested for the process device: wgpu's defaults when the
/// adapter supports them, otherwise everything the adapter offers. A
/// downlevel adapter, such as a GL driver with two color attachments, would
/// reject the defaults.
pub(crate) fn device_limits(adapter: &wgpu::Adapter) -> wgpu::Limits {
  let supported = adapter.limits();
  let defaults = wgpu::Limits::default();
  if defaults.check_limits(&supported) {
    defaults
  } else {
    supported
  }
}

/// `timestamp-query` when the adapter has it.
fn timestamp_feature(adapter: &wgpu::Adapter) -> wgpu::Features {
  if adapter.features().contains(wgpu::Features::TIMESTAMP_QUERY) {
    wgpu::Features::TIMESTAMP_QUERY
  } else {
    wgpu::Features::empty()
  }
}

/// Optional features the process device requests when the adapter has them.
/// A failed request retries with `timestamp-query` alone, then with none,
/// so a rejected feature cannot drop timestamps.
fn optional_features(adapter: &wgpu::Adapter) -> wgpu::Features {
  const WANTED: [wgpu::Features; 4] = [
    wgpu::Features::TIMESTAMP_QUERY,
    wgpu::Features::DEPTH_CLIP_CONTROL,
    wgpu::Features::DEPTH32FLOAT_STENCIL8,
    wgpu::Features::EXTERNAL_TEXTURE,
  ];
  let available = adapter.features();
  let mut features = wgpu::Features::empty();
  for feature in WANTED {
    if available.contains(feature) {
      features |= feature;
    }
  }
  features
}

fn request_with(
  adapter: &wgpu::Adapter,
  features: wgpu::Features,
) -> Result<(wgpu::Device, wgpu::Queue), String> {
  pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
    label: Some("yuzuko"),
    required_features: features,
    required_limits: device_limits(adapter),
    experimental_features: wgpu::ExperimentalFeatures::disabled(),
    memory_hints: wgpu::MemoryHints::default(),
    trace: wgpu::Trace::Off,
  }))
  .map_err(|error| error.to_string())
}

pub(crate) fn request_device(
  adapter: &wgpu::Adapter,
) -> Result<(wgpu::Device, wgpu::Queue), String> {
  let wanted = optional_features(adapter);
  let timestamp = timestamp_feature(adapter);
  let attempts = [wanted, timestamp, wgpu::Features::empty()];
  let mut last = String::from("The GPU device could not be created");
  let mut previous = None;
  for features in attempts {
    if previous == Some(features) {
      continue;
    }
    previous = Some(features);
    match request_with(adapter, features) {
      Ok(parts) => return Ok(parts),
      Err(error) => last = error,
    }
  }
  Err(last)
}

fn create() -> Result<SharedGpu, String> {
  let instance = wgpu::Instance::new(instance_descriptor());
  let adapter = pollster::block_on(instance.request_adapter(
    &wgpu::RequestAdapterOptions {
      power_preference: wgpu::PowerPreference::default(),
      force_fallback_adapter: false,
      compatible_surface: None,
    },
  ))
  .map_err(|error| error.to_string())?;
  let (device, queue) = request_device(&adapter)?;
  Ok(SharedGpu {
    instance,
    adapter,
    device,
    queue,
  })
}
