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

fn create() -> Result<SharedGpu, String> {
  let instance =
    wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
  let adapter = pollster::block_on(instance.request_adapter(
    &wgpu::RequestAdapterOptions {
      power_preference: wgpu::PowerPreference::default(),
      force_fallback_adapter: false,
      compatible_surface: None,
    },
  ))
  .map_err(|error| error.to_string())?;
  let (device, queue) =
    pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
      label: Some("yuzuko"),
      required_features: wgpu::Features::empty(),
      required_limits: wgpu::Limits::default(),
      experimental_features: wgpu::ExperimentalFeatures::disabled(),
      memory_hints: wgpu::MemoryHints::default(),
      trace: wgpu::Trace::Off,
    }))
    .map_err(|error| error.to_string())?;
  Ok(SharedGpu {
    instance,
    adapter,
    device,
    queue,
  })
}
