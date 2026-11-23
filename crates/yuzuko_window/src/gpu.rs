//! The process wgpu device shared by Canvas 2D and window presentation.
//!
//! Present names an explicit [`wgpu::SurfaceColorSpace`] and never `Auto`.
//! `Auto` on `Rgba16Float` resolves to linear scRGB, which is a different
//! encoding from the canvas store.

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

/// `timestamp-query`, plus writes inside encoders and passes, when the
/// adapter has each capability. The inside-encoder and inside-pass bits are
/// wgpu features. They are not WebGPU feature names. A failed device request
/// retries with this set, so a rejected optional feature cannot drop them.
fn timestamp_feature(adapter: &wgpu::Adapter) -> wgpu::Features {
  const WANTED: [wgpu::Features; 3] = [
    wgpu::Features::TIMESTAMP_QUERY,
    wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS,
    wgpu::Features::TIMESTAMP_QUERY_INSIDE_PASSES,
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

/// Optional features the process device requests when the adapter has them.
/// A failed request retries with the timestamp set alone, then with none,
/// so a rejected feature cannot drop timestamps.
fn optional_features(adapter: &wgpu::Adapter) -> wgpu::Features {
  const REST: [wgpu::Features; 3] = [
    wgpu::Features::DEPTH_CLIP_CONTROL,
    wgpu::Features::DEPTH32FLOAT_STENCIL8,
    wgpu::Features::EXTERNAL_TEXTURE,
  ];
  let available = adapter.features();
  let mut features = timestamp_feature(adapter);
  for feature in REST {
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

/// What the canvas stores, before the surface capabilities are consulted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PresentRequest {
  SrgbUnorm8,
  DisplayP3Unorm8,
  /// Encoded components, including values outside 0..1. `display_p3` selects
  /// extended Display P3 over extended sRGB.
  Float16 {
    display_p3: bool,
  },
}

/// The swapchain encoding chosen for one present.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PresentTarget {
  SrgbUnorm8,
  DisplayP3Unorm8,
  ExtendedSrgb,
  ExtendedDisplayP3,
}

/// Format and color space to configure, plus the encoding the canvas must upload.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PresentChoice {
  pub format: wgpu::TextureFormat,
  pub color_space: wgpu::SurfaceColorSpace,
  pub target: PresentTarget,
}

/// Picks a `(format, SurfaceColorSpace)` pair the surface advertises.
///
/// Display P3 uses a non-`*Srgb` unorm8 format. An `*Srgb` format applies the
/// sRGB transfer on write, and the canvas samples are already encoded. A
/// float16 store uses `Rgba16Float` with the encoded extended space. Any pair
/// the surface does not list falls back to sRGB unorm8. The color space is
/// never `Auto`.
pub(crate) fn choose_present(
  caps: &wgpu::SurfaceCapabilities,
  request: PresentRequest,
) -> Option<PresentChoice> {
  match request {
    PresentRequest::SrgbUnorm8 => srgb_choice(caps),
    PresentRequest::DisplayP3Unorm8 => {
      unorm_choice(caps, wgpu::SurfaceColorSpace::DisplayP3)
        .or_else(|| srgb_choice(caps))
    }
    PresentRequest::Float16 { display_p3 } => {
      let (space, target) = if display_p3 {
        (
          wgpu::SurfaceColorSpace::ExtendedDisplayP3,
          PresentTarget::ExtendedDisplayP3,
        )
      } else {
        (
          wgpu::SurfaceColorSpace::ExtendedSrgb,
          PresentTarget::ExtendedSrgb,
        )
      };
      if supports(caps, wgpu::TextureFormat::Rgba16Float, space) {
        Some(PresentChoice {
          format: wgpu::TextureFormat::Rgba16Float,
          color_space: space,
          target,
        })
      } else {
        srgb_choice(caps)
      }
    }
  }
}

fn srgb_choice(caps: &wgpu::SurfaceCapabilities) -> Option<PresentChoice> {
  let format = srgb_format(caps)?;
  Some(PresentChoice {
    format,
    color_space: wgpu::SurfaceColorSpace::Srgb,
    target: PresentTarget::SrgbUnorm8,
  })
}

/// `Bgra8Unorm` or `Rgba8Unorm` paired with `space`. `*Srgb` formats are skipped.
fn unorm_choice(
  caps: &wgpu::SurfaceCapabilities,
  space: wgpu::SurfaceColorSpace,
) -> Option<PresentChoice> {
  const FORMATS: [wgpu::TextureFormat; 2] = [
    wgpu::TextureFormat::Bgra8Unorm,
    wgpu::TextureFormat::Rgba8Unorm,
  ];
  let format = FORMATS
    .into_iter()
    .find(|format| supports(caps, *format, space))?;
  let target = if space == wgpu::SurfaceColorSpace::DisplayP3 {
    PresentTarget::DisplayP3Unorm8
  } else {
    PresentTarget::SrgbUnorm8
  };
  Some(PresentChoice {
    format,
    color_space: space,
    target,
  })
}

fn srgb_format(
  caps: &wgpu::SurfaceCapabilities,
) -> Option<wgpu::TextureFormat> {
  const PREFERRED: [wgpu::TextureFormat; 4] = [
    wgpu::TextureFormat::Bgra8Unorm,
    wgpu::TextureFormat::Rgba8Unorm,
    wgpu::TextureFormat::Bgra8UnormSrgb,
    wgpu::TextureFormat::Rgba8UnormSrgb,
  ];
  if let Some(format) = PREFERRED
    .into_iter()
    .find(|format| supports(caps, *format, wgpu::SurfaceColorSpace::Srgb))
  {
    return Some(format);
  }
  if caps.format_capabilities.is_empty() {
    return PREFERRED
      .into_iter()
      .find(|format| caps.formats.contains(format))
      .or_else(|| caps.formats.first().copied());
  }
  caps
    .format_capabilities
    .iter()
    .find(|entry| entry.color_spaces.contains(wgpu::SurfaceColorSpaces::SRGB))
    .map(|entry| entry.format)
}

fn supports(
  caps: &wgpu::SurfaceCapabilities,
  format: wgpu::TextureFormat,
  space: wgpu::SurfaceColorSpace,
) -> bool {
  let Some(flag) = space.to_color_spaces() else {
    return false;
  };
  caps.color_spaces(format).contains(flag)
}

fn create() -> Result<SharedGpu, String> {
  let instance = wgpu::Instance::new(instance_descriptor());
  let adapter = pollster::block_on(instance.request_adapter(
    &wgpu::RequestAdapterOptions {
      power_preference: wgpu::PowerPreference::default(),
      force_fallback_adapter: false,
      compatible_surface: None,
      apply_limit_buckets: false,
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

#[cfg(test)]
mod tests {
  use super::PresentRequest;
  use super::PresentTarget;
  use super::choose_present;

  fn entry(
    format: wgpu::TextureFormat,
    spaces: wgpu::SurfaceColorSpaces,
  ) -> wgpu::SurfaceFormatCapabilities {
    wgpu::SurfaceFormatCapabilities {
      format,
      color_spaces: spaces,
    }
  }

  fn caps(
    entries: Vec<wgpu::SurfaceFormatCapabilities>,
  ) -> wgpu::SurfaceCapabilities {
    let formats = entries
      .iter()
      .filter(|entry| {
        entry.color_spaces.contains(wgpu::SurfaceColorSpaces::SRGB)
          || entry
            .color_spaces
            .contains(wgpu::SurfaceColorSpaces::EXTENDED_SRGB_LINEAR)
      })
      .map(|entry| entry.format)
      .collect();
    wgpu::SurfaceCapabilities {
      formats,
      format_capabilities: entries,
      present_modes: vec![wgpu::PresentMode::Fifo],
      alpha_modes: vec![wgpu::CompositeAlphaMode::Opaque],
      usages: wgpu::TextureUsages::RENDER_ATTACHMENT,
    }
  }

  #[test]
  fn present_names_an_explicit_color_space() {
    let spaces =
      wgpu::SurfaceColorSpaces::SRGB | wgpu::SurfaceColorSpaces::DISPLAY_P3;
    let wide = wgpu::SurfaceColorSpaces::EXTENDED_SRGB
      | wgpu::SurfaceColorSpaces::EXTENDED_DISPLAY_P3
      | wgpu::SurfaceColorSpaces::EXTENDED_SRGB_LINEAR;
    let caps = caps(vec![
      entry(
        wgpu::TextureFormat::Bgra8UnormSrgb,
        wgpu::SurfaceColorSpaces::SRGB | wgpu::SurfaceColorSpaces::DISPLAY_P3,
      ),
      entry(wgpu::TextureFormat::Bgra8Unorm, spaces),
      entry(wgpu::TextureFormat::Rgba16Float, wide),
    ]);

    let srgb = choose_present(&caps, PresentRequest::SrgbUnorm8).unwrap();
    assert_eq!(srgb.format, wgpu::TextureFormat::Bgra8Unorm);
    assert_eq!(srgb.color_space, wgpu::SurfaceColorSpace::Srgb);
    assert_eq!(srgb.target, PresentTarget::SrgbUnorm8);

    let p3 = choose_present(&caps, PresentRequest::DisplayP3Unorm8).unwrap();
    assert_eq!(p3.format, wgpu::TextureFormat::Bgra8Unorm);
    assert_eq!(p3.color_space, wgpu::SurfaceColorSpace::DisplayP3);
    assert_eq!(p3.target, PresentTarget::DisplayP3Unorm8);

    let extended =
      choose_present(&caps, PresentRequest::Float16 { display_p3: false })
        .unwrap();
    assert_eq!(extended.format, wgpu::TextureFormat::Rgba16Float);
    assert_eq!(extended.color_space, wgpu::SurfaceColorSpace::ExtendedSrgb);
    assert_eq!(extended.target, PresentTarget::ExtendedSrgb);

    let p3_float =
      choose_present(&caps, PresentRequest::Float16 { display_p3: true })
        .unwrap();
    assert_eq!(
      p3_float.color_space,
      wgpu::SurfaceColorSpace::ExtendedDisplayP3
    );
    assert_eq!(p3_float.target, PresentTarget::ExtendedDisplayP3);
    assert_ne!(p3_float.color_space, wgpu::SurfaceColorSpace::Auto);
    assert_ne!(
      p3_float.color_space,
      wgpu::SurfaceColorSpace::ExtendedSrgbLinear
    );
  }

  #[test]
  fn a_missing_pair_falls_back_to_srgb_unorm8() {
    let caps = caps(vec![
      entry(
        wgpu::TextureFormat::Rgba8Unorm,
        wgpu::SurfaceColorSpaces::SRGB,
      ),
      entry(
        wgpu::TextureFormat::Rgba16Float,
        wgpu::SurfaceColorSpaces::EXTENDED_SRGB_LINEAR,
      ),
      entry(
        wgpu::TextureFormat::Bgra8UnormSrgb,
        wgpu::SurfaceColorSpaces::DISPLAY_P3 | wgpu::SurfaceColorSpaces::SRGB,
      ),
    ]);
    let p3 = choose_present(&caps, PresentRequest::DisplayP3Unorm8).unwrap();
    assert_eq!(p3.target, PresentTarget::SrgbUnorm8);
    assert_eq!(p3.color_space, wgpu::SurfaceColorSpace::Srgb);
    assert_eq!(p3.format, wgpu::TextureFormat::Rgba8Unorm);

    let float16 =
      choose_present(&caps, PresentRequest::Float16 { display_p3: true })
        .unwrap();
    assert_eq!(float16.target, PresentTarget::SrgbUnorm8);
    assert_ne!(
      float16.color_space,
      wgpu::SurfaceColorSpace::ExtendedSrgbLinear
    );
  }
}
