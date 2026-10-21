//! winit windows and an opaque wgpu clear. Headless sessions never call this.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use winit::application::ApplicationHandler;
use winit::dpi::{LogicalPosition, LogicalSize};
use winit::event::{
  DeviceEvent, ElementState, Ime, MouseButton, MouseScrollDelta, WindowEvent,
};
use winit::event_loop::{ActiveEventLoop, EventLoop, EventLoopProxy};
use winit::keyboard::ModifiersState;
use winit::platform::pump_events::EventLoopExtPumpEvents;
#[cfg(not(target_os = "macos"))]
use winit::window::UserAttentionType;
use winit::window::{
  CursorGrabMode, CursorIcon, Fullscreen, Window, WindowId, WindowLevel,
};

use crate::inbox::{
  self, PointerKind, Raw, RawIme, RawKey, RawPointer, RawWheel, Size,
};
use crate::input::keys;
use crate::session;

struct Wake;

struct PendingWindow {
  id: u64,
  title: String,
  width: f64,
  height: f64,
  resizable: bool,
  position: Option<(f64, f64)>,
  always_on_top: bool,
  decorations: bool,
  active: bool,
  visible: bool,
  maximized: bool,
  min_width: Option<f64>,
  min_height: Option<f64>,
  max_width: Option<f64>,
  max_height: Option<f64>,
  transparent: bool,
}

/// Options for one OS window. Size comes from the inbox, not from here.
pub struct OpenRequest {
  pub title: String,
  pub resizable: bool,
  /// Outer frame origin in CSS pixels. `None` centers on the primary monitor.
  pub position: Option<(f64, f64)>,
  pub always_on_top: bool,
  pub decorations: bool,
  pub active: bool,
  pub visible: bool,
  pub maximized: bool,
  pub min_width: Option<f64>,
  pub min_height: Option<f64>,
  pub max_width: Option<f64>,
  pub max_height: Option<f64>,
  pub transparent: bool,
}

/// Frame and client origins in CSS pixels. Y grows downward.
pub struct FrameOrigin {
  pub outer_x: f64,
  pub outer_y: f64,
  pub inner_x: f64,
  pub inner_y: f64,
}

struct Gpu {
  device: wgpu::Device,
  queue: wgpu::Queue,
  format: wgpu::TextureFormat,
  present_mode: wgpu::PresentMode,
  alpha: wgpu::CompositeAlphaMode,
}

struct HostWindow {
  /// Dropped in `Drop` before `window`. The `'static` lifetime is a lie: the
  /// surface borrows `window` and must not outlive it.
  surface: Option<wgpu::Surface<'static>>,
  window: Window,
  surface_id: u64,
  transparent: bool,
  /// Whether `surface` has been configured. wgpu panics when an unconfigured
  /// surface is asked for a texture.
  configured: bool,
  /// Asks for this window's monitor rate while an animation frame is armed.
  /// Invalidated before `window` is dropped.
  #[cfg(target_os = "macos")]
  frame_link: Option<objc2::rc::Retained<objc2::runtime::AnyObject>>,
}

impl Drop for HostWindow {
  fn drop(&mut self) {
    #[cfg(target_os = "macos")]
    frame_link::invalidate(&mut self.frame_link);
    self.surface.take();
  }
}

struct App {
  pending: Vec<PendingWindow>,
  windows: HashMap<WindowId, HostWindow>,
  by_surface: HashMap<u64, WindowId>,
  errors: HashMap<u64, String>,
  instance: Option<wgpu::Instance>,
  gpu: Option<Gpu>,
  gpu_failed: bool,
  modifiers: ModifiersState,
  cursor: PhysicalCursor,
}

struct PhysicalCursor {
  x: f64,
  y: f64,
  valid: bool,
}

struct OsLoop {
  event_loop: EventLoop<Wake>,
  proxy: EventLoopProxy<Wake>,
  app: App,
}

thread_local! {
  static OS: RefCell<Option<OsLoop>> = const { RefCell::new(None) };
  static IN_PUMP: Cell<bool> = const { Cell::new(false) };
  static LOCKED: Cell<Option<u64>> = const { Cell::new(None) };
}

struct PumpGuard;

impl Drop for PumpGuard {
  fn drop(&mut self) {
    IN_PUMP.with(|flag| flag.set(false));
  }
}

/// Device-pixel size of an open OS window. Headless windows have none.
pub fn current_size(id: u64) -> Option<crate::inbox::Size> {
  if session::headless() {
    return None;
  }
  OS.with(|slot| {
    let os = slot.borrow();
    let os = os.as_ref()?;
    let window_id = *os.app.by_surface.get(&id)?;
    let host = os.app.windows.get(&window_id)?;
    Some(size_of(&host.window))
  })
}

fn size_of(window: &Window) -> crate::inbox::Size {
  let scale = window.scale_factor();
  let physical = window.inner_size();
  let scale = if scale.is_finite() && scale > 0.0 {
    scale
  } else {
    1.0
  };
  crate::inbox::Size {
    css_width: physical.width as f64 / scale,
    css_height: physical.height as f64 / scale,
    scale,
    bitmap_width: physical.width as f64,
    bitmap_height: physical.height as f64,
  }
}

pub fn open(id: u64, request: OpenRequest) -> Result<(), String> {
  if session::headless() {
    return Ok(());
  }
  let size =
    inbox::size(id).ok_or_else(|| "The surface is closed.".to_string())?;
  ensure_loop()?;
  OS.with(|slot| {
    if let Some(os) = slot.borrow_mut().as_mut() {
      os.app.pending.push(PendingWindow {
        id,
        title: request.title,
        width: size.css_width,
        height: size.css_height,
        resizable: request.resizable,
        position: request.position,
        always_on_top: request.always_on_top,
        decorations: request.decorations,
        active: request.active,
        visible: request.visible,
        maximized: request.maximized,
        min_width: request.min_width,
        min_height: request.min_height,
        max_width: request.max_width,
        max_height: request.max_height,
        transparent: request.transparent,
      });
      // A zero-timeout pump after the first `resumed` can return before
      // `about_to_wait`. A user event still runs `create_pending`.
      let _ = os.proxy.send_event(Wake);
    }
  });
  pump(Some(Duration::ZERO));
  if !has_window(id) {
    // `resumed` already ran for an earlier window. Wait until
    // `about_to_wait` drains this one.
    pump(Some(Duration::from_millis(16)));
  }
  if let Some(error) = take_error(id) {
    return Err(error);
  }
  if !has_window(id) {
    return Err("The window was not created.".to_string());
  }
  Ok(())
}

/// Matches the macOS display link to whether this window has a frame armed.
///
/// Other platforms pace from the monitor period alone.
pub fn sync_frame_link(id: u64) {
  #[cfg(target_os = "macos")]
  {
    if session::headless() {
      return;
    }
    OS.with(|slot| {
      let mut guard = slot.borrow_mut();
      let Some(os) = guard.as_mut() else {
        return;
      };
      let Some(window_id) = os.app.by_surface.get(&id).copied() else {
        return;
      };
      os.app.sync_link(window_id);
    });
  }
  #[cfg(not(target_os = "macos"))]
  {
    let _ = id;
  }
}

pub fn close(id: u64) {
  if LOCKED.with(|locked| locked.get()) == Some(id) {
    unlock_pointer(id);
  }
  if session::headless() {
    return;
  }
  OS.with(|slot| {
    let mut guard = slot.borrow_mut();
    let Some(os) = guard.as_mut() else {
      return;
    };
    let Some(window_id) = os.app.by_surface.remove(&id) else {
      return;
    };
    os.app.windows.remove(&window_id);
  });
}

pub fn set_title(id: u64, title: &str) {
  with_window(id, |window| window.set_title(title));
}

/// Outer and client origins. `None` when this process has no OS window.
pub fn frame_origin(id: u64) -> Option<FrameOrigin> {
  with_window(id, origin_of).flatten()
}

/// Moves the frame's top-left corner. Returns true after a live window pump.
pub fn move_outer(id: u64, x: f64, y: f64) -> bool {
  let found = with_window(id, |window| {
    window.set_outer_position(LogicalPosition::new(x, y));
  });
  found.is_some() && pump_if_idle()
}

pub fn set_visible(id: u64, visible: bool) {
  with_window(id, |window| window.set_visible(visible));
  let _ = pump_if_idle();
}

pub fn focus(id: u64) {
  with_window(id, |window| window.focus_window());
  let _ = pump_if_idle();
}

pub fn is_maximized(id: u64) -> Option<bool> {
  with_window(id, |window| window.is_maximized())
}

pub fn set_always_on_top(id: u64, enabled: bool) {
  let level = if enabled {
    WindowLevel::AlwaysOnTop
  } else {
    WindowLevel::Normal
  };
  with_window(id, |window| window.set_window_level(level));
}

pub fn set_resizable(id: u64, enabled: bool) {
  with_window(id, |window| window.set_resizable(enabled));
}

/// Starts the OS loop when tray or dock code needs `NSApp` to be running.
pub fn ensure_running() -> Result<(), String> {
  ensure_loop()?;
  let _ = pump(Some(Duration::ZERO));
  Ok(())
}

/// Requests attention for one open OS window. Returns false when none exists.
#[cfg(not(target_os = "macos"))]
pub fn request_attention(critical: bool) -> bool {
  let kind = if critical {
    UserAttentionType::Critical
  } else {
    UserAttentionType::Informational
  };
  for id in inbox::ids() {
    if with_window(id, |window| {
      window.request_user_attention(Some(kind));
    })
    .is_some()
    {
      return true;
    }
  }
  false
}

fn pump_if_idle() -> bool {
  if session::headless() || IN_PUMP.with(|flag| flag.get()) {
    return false;
  }
  pump(Some(Duration::ZERO));
  true
}

fn origin_of(window: &Window) -> Option<FrameOrigin> {
  let scale = window.scale_factor();
  let scale = if scale.is_finite() && scale > 0.0 {
    scale
  } else {
    1.0
  };
  let outer = window.outer_position().ok()?.to_logical::<f64>(scale);
  let inner = window.inner_position().ok()?.to_logical::<f64>(scale);
  Some(FrameOrigin {
    outer_x: outer.x,
    outer_y: outer.y,
    inner_x: inner.x,
    inner_y: inner.y,
  })
}

fn chrome_attributes(
  request: &PendingWindow,
) -> winit::window::WindowAttributes {
  let level = if request.always_on_top {
    WindowLevel::AlwaysOnTop
  } else {
    WindowLevel::Normal
  };
  let mut attributes = Window::default_attributes()
    .with_title(request.title.as_str())
    .with_inner_size(LogicalSize::new(request.width, request.height))
    .with_resizable(request.resizable)
    .with_decorations(request.decorations)
    .with_active(request.active)
    .with_visible(request.visible)
    .with_maximized(request.maximized)
    .with_transparent(request.transparent)
    .with_window_level(level);
  if let Some((x, y)) = request.position {
    attributes = attributes.with_position(LogicalPosition::new(x, y));
  }
  if let Some(size) = inner_bound(request.min_width, request.min_height, 0.0) {
    attributes =
      attributes.with_min_inner_size(LogicalSize::new(size.0, size.1));
  }
  if let Some(size) =
    inner_bound(request.max_width, request.max_height, 100_000.0)
  {
    attributes =
      attributes.with_max_inner_size(LogicalSize::new(size.0, size.1));
  }
  crate::identity::apply(attributes)
}

/// One optional edge. The missing edge is `fill` so winit still receives a size.
fn inner_bound(
  width: Option<f64>,
  height: Option<f64>,
  fill: f64,
) -> Option<(f64, f64)> {
  if width.is_none() && height.is_none() {
    return None;
  }
  Some((width.unwrap_or(fill), height.unwrap_or(fill)))
}

// Present holds `OS` borrowed. Transparency is read from the canvas present
// hook, so it cannot look that map up again.
static TRANSPARENT_WINDOWS: std::sync::LazyLock<
  std::sync::Mutex<HashSet<u64>>,
> = std::sync::LazyLock::new(|| std::sync::Mutex::new(HashSet::new()));

/// Whether the canvas presented in this window should keep alpha.
pub fn canvas_is_transparent(canvas_id: u64) -> bool {
  let windows = TRANSPARENT_WINDOWS.lock().expect("transparent windows");
  if windows.contains(&canvas_id) {
    return true;
  }
  windows
    .iter()
    .any(|id| crate::surface::canvas_for_window(*id) == Some(canvas_id))
}

fn note_refresh_from(window: &Window, id: u64) {
  inbox::set_frame_interval(id, monitor_interval(window));
}

fn monitor_interval(window: &Window) -> Duration {
  inbox::interval_from_millihertz(monitor_millihertz(window))
}

fn monitor_millihertz(window: &Window) -> Option<u32> {
  #[cfg(target_os = "macos")]
  if let Some(rate) = frame_link::maximum_millihertz(window) {
    return Some(rate);
  }
  window
    .current_monitor()
    .or_else(|| window.primary_monitor())
    .and_then(|monitor| monitor.refresh_rate_millihertz())
}

/// Keeps a window's display link matched to its animation frames.
///
/// `CAMetalLayer` has no frame-rate property on macOS. A variable-refresh
/// display stays below its maximum until a `CADisplayLink` requests that
/// rate. The callback does not draw. The window still presents from its
/// rendering update, and Fifo presentation still waits for vertical sync.
#[cfg(target_os = "macos")]
mod frame_link {
  use std::cell::RefCell;
  use std::time::Duration;

  use objc2::rc::Retained;
  use objc2::runtime::{AnyObject, NSObject};
  use objc2::{AnyThread, define_class, msg_send, sel};
  use objc2_app_kit::NSView;
  use objc2_foundation::{NSObjectProtocol, NSRunLoop, NSRunLoopCommonModes};
  use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
  use winit::window::Window;

  use super::monitor_interval;
  use crate::inbox;

  define_class!(
    #[unsafe(super(NSObject))]
    #[ivars = ()]
    #[name = "YuzukoFrameLink"]
    struct FrameLinkTarget;

    impl FrameLinkTarget {
      #[unsafe(method(onDisplayLink:))]
      fn on_display_link(&self, _link: &AnyObject) {}
    }
  );

  impl FrameLinkTarget {
    fn new() -> Retained<Self> {
      let this = Self::alloc().set_ivars(());
      unsafe { msg_send![super(this), init] }
    }
  }

  thread_local! {
    static TARGET: RefCell<Option<Retained<FrameLinkTarget>>> =
      const { RefCell::new(None) };
  }

  fn target() -> Retained<FrameLinkTarget> {
    TARGET.with(|slot| {
      let mut slot = slot.borrow_mut();
      if let Some(target) = slot.as_ref() {
        return target.clone();
      }
      let target = FrameLinkTarget::new();
      *slot = Some(target.clone());
      target
    })
  }

  pub(super) fn maximum_millihertz(window: &Window) -> Option<u32> {
    let screen = ns_window(window)?.screen()?;
    if !screen.respondsToSelector(sel!(maximumFramesPerSecond)) {
      return None;
    }
    let fps = screen.maximumFramesPerSecond();
    u32::try_from(fps)
      .ok()
      .filter(|fps| *fps > 0)
      .map(|fps| fps.saturating_mul(1000))
  }

  pub(super) fn sync(host: &mut super::HostWindow) {
    if !inbox::frame_armed(host.surface_id) {
      invalidate(&mut host.frame_link);
      return;
    }
    if host.frame_link.is_none() {
      host.frame_link = make(&host.window);
    }
    if let Some(link) = host.frame_link.as_ref() {
      set_rate(link, monitor_interval(&host.window));
    }
  }

  pub(super) fn invalidate(slot: &mut Option<Retained<AnyObject>>) {
    let Some(link) = slot.take() else {
      return;
    };
    let _: () = unsafe { msg_send![&*link, invalidate] };
  }

  fn make(window: &Window) -> Option<Retained<AnyObject>> {
    let ns_window = ns_window(window)?;
    let selector = sel!(displayLinkWithTarget:selector:);
    if !ns_window.respondsToSelector(selector) {
      return None;
    }
    let target = target();
    let callback = sel!(onDisplayLink:);
    let link: Option<Retained<AnyObject>> = unsafe {
      msg_send![
        &*ns_window,
        displayLinkWithTarget: &*target,
        selector: callback
      ]
    };
    let link = link?;
    let run_loop = NSRunLoop::mainRunLoop();
    let mode = unsafe { NSRunLoopCommonModes };
    let _: () =
      unsafe { msg_send![&*link, addToRunLoop: &*run_loop, forMode: mode] };
    Some(link)
  }

  fn set_rate(link: &AnyObject, interval: Duration) {
    let selector = sel!(setPreferredFrameRateRange:);
    let responds: objc2::runtime::Bool =
      unsafe { msg_send![link, respondsToSelector: selector] };
    if !responds.as_bool() {
      return;
    }
    let hz = (1_000_000_000.0 / interval.as_nanos().max(1) as f64) as f32;
    let range = FrameRateRange {
      minimum: hz,
      maximum: hz,
      preferred: hz,
    };
    let _: () = unsafe { msg_send![link, setPreferredFrameRateRange: range] };
  }

  fn ns_window(window: &Window) -> Option<Retained<objc2_app_kit::NSWindow>> {
    let handle = window.window_handle().ok()?;
    let RawWindowHandle::AppKit(appkit) = handle.as_raw() else {
      return None;
    };
    let view =
      unsafe { Retained::retain(appkit.ns_view.as_ptr() as *mut NSView) }?;
    view.window()
  }

  #[repr(C)]
  #[derive(Clone, Copy)]
  struct FrameRateRange {
    minimum: f32,
    maximum: f32,
    preferred: f32,
  }

  // `CAFrameRateRange` is three floats. `@encode` is `{CAFrameRateRange=fff}`.
  unsafe impl objc2::encode::Encode for FrameRateRange {
    const ENCODING: objc2::Encoding = objc2::Encoding::Struct(
      "CAFrameRateRange",
      &[
        <f32 as objc2::encode::Encode>::ENCODING,
        <f32 as objc2::encode::Encode>::ENCODING,
        <f32 as objc2::encode::Encode>::ENCODING,
      ],
    );
  }
}

fn center_window(window: &Window) {
  let Some(monitor) = window
    .current_monitor()
    .or_else(|| window.primary_monitor())
  else {
    return;
  };
  let mon_pos = monitor.position();
  let mon_size = monitor.size();
  let outer = window.outer_size();
  let x = mon_pos.x + (mon_size.width as i32 - outer.width as i32) / 2;
  let y = mon_pos.y + (mon_size.height as i32 - outer.height as i32) / 2;
  window.set_outer_position(winit::dpi::PhysicalPosition::new(x, y));
}

/// Canonical cursor keyword, or `None` when `name` is not a CSS cursor.
pub fn canonical_cursor(name: &str) -> Option<String> {
  let name = name.trim().to_ascii_lowercase();
  if name == "none" {
    return Some(name);
  }
  CursorIcon::from_str(&name).ok()?;
  Some(name)
}

pub fn set_cursor(id: u64, name: &str) {
  let Some(name) = canonical_cursor(name) else {
    return;
  };
  let locked = LOCKED.with(|locked| locked.get()) == Some(id);
  with_window(id, |window| apply_cursor(window, &name, locked));
}

pub fn resize(id: u64, width: f64, height: f64) {
  with_window(id, |window| {
    let _ = window.request_inner_size(LogicalSize::new(width, height));
  });
}

pub fn set_ime(id: u64, allowed: bool) {
  with_window(id, |window| window.set_ime_allowed(allowed));
}

pub fn set_caret(id: u64, x: f64, y: f64, width: f64, height: f64) {
  with_window(id, |window| {
    window.set_ime_cursor_area(
      LogicalPosition::new(x, y),
      LogicalSize::new(width, height),
    );
  });
}

pub fn lock_pointer(id: u64) -> Result<(), String> {
  if !inbox::contains(id) {
    return Err("The surface is closed.".to_string());
  }
  if let Some(holder) = LOCKED.with(|locked| locked.get())
    && holder != id
  {
    return Err("The pointer is locked on another surface.".to_string());
  }
  if !session::headless() {
    let grabbed = with_window(id, |window| {
      window
        .set_cursor_grab(CursorGrabMode::Locked)
        .or_else(|_| window.set_cursor_grab(CursorGrabMode::Confined))
        .is_ok()
    });
    if grabbed != Some(true) {
      return Err("The pointer could not be locked.".to_string());
    }
    with_window(id, |window| window.set_cursor_visible(false));
  }
  LOCKED.with(|locked| locked.set(Some(id)));
  Ok(())
}

pub fn unlock_pointer(id: u64) {
  if LOCKED.with(|locked| locked.get()) != Some(id) {
    return;
  }
  LOCKED.with(|locked| locked.set(None));
  with_window(id, |window| {
    let _ = window.set_cursor_grab(CursorGrabMode::None);
    window.set_cursor_visible(true);
  });
}

pub fn set_fullscreen(id: u64, enabled: bool) -> Result<(), String> {
  if !inbox::contains(id) {
    return Err("The surface is closed.".to_string());
  }
  with_window(id, |window| {
    window.set_fullscreen(enabled.then_some(Fullscreen::Borderless(None)));
  });
  Ok(())
}

pub fn request_redraw(id: u64) {
  with_window(id, |window| window.request_redraw());
}

pub fn present(id: u64) {
  if session::headless() {
    return;
  }
  OS.with(|slot| {
    let mut guard = slot.borrow_mut();
    let Some(os) = guard.as_mut() else {
      return;
    };
    let Some(window_id) = os.app.by_surface.get(&id).copied() else {
      return;
    };
    os.app.present_window(window_id);
  });
}

pub fn shutdown() {
  yuzuko_core::set_task_waker(None);
  LOCKED.with(|locked| locked.set(None));
  OS.with(|slot| {
    *slot.borrow_mut() = None;
  });
}

/// Pumps the OS event loop. Returns false when no OS loop exists.
pub fn pump(timeout: Option<Duration>) -> bool {
  if session::headless() && OS.with(|slot| slot.borrow().is_none()) {
    return false;
  }
  if IN_PUMP.with(|flag| flag.get()) {
    return true;
  }
  OS.with(|slot| {
    let mut slot = slot.borrow_mut();
    let Some(os) = slot.as_mut() else {
      return false;
    };
    IN_PUMP.with(|flag| flag.set(true));
    let _guard = PumpGuard;
    let OsLoop {
      event_loop, app, ..
    } = os;
    // GLib events do not wake winit, so a GTK user bounds the sleep.
    #[cfg(target_os = "linux")]
    let timeout = if crate::shell::gtk_loop::active() {
      const GLIB_SLICE: Duration = Duration::from_millis(16);
      Some(timeout.map_or(GLIB_SLICE, |wait| wait.min(GLIB_SLICE)))
    } else {
      timeout
    };
    event_loop.pump_app_events(timeout, app);
    #[cfg(target_os = "linux")]
    crate::shell::gtk_loop::drain();
    true
  })
}

fn ensure_loop() -> Result<(), String> {
  if OS.with(|slot| slot.borrow().is_some()) {
    return Ok(());
  }
  let event_loop = EventLoop::<Wake>::with_user_event()
    .build()
    .map_err(|error| error.to_string())?;
  let proxy = event_loop.create_proxy();
  let waker = proxy.clone();
  yuzuko_core::set_task_waker(Some(Arc::new(move || {
    let _ = waker.send_event(Wake);
  })));
  OS.with(|slot| {
    *slot.borrow_mut() = Some(OsLoop {
      event_loop,
      proxy,
      app: App {
        pending: Vec::new(),
        windows: HashMap::new(),
        by_surface: HashMap::new(),
        errors: HashMap::new(),
        instance: None,
        gpu: None,
        gpu_failed: false,
        modifiers: ModifiersState::empty(),
        cursor: PhysicalCursor {
          x: 0.0,
          y: 0.0,
          valid: false,
        },
      },
    });
  });
  Ok(())
}

fn take_error(id: u64) -> Option<String> {
  OS.with(|slot| {
    slot
      .borrow_mut()
      .as_mut()
      .and_then(|os| os.app.errors.remove(&id))
  })
}

fn has_window(id: u64) -> bool {
  OS.with(|slot| {
    slot
      .borrow()
      .as_ref()
      .is_some_and(|os| os.app.by_surface.contains_key(&id))
  })
}

#[cfg(target_os = "windows")]
pub fn focused_hwnd() -> Option<isize> {
  let id = crate::activation::focused_id()?;
  with_window(id, hwnd_of).flatten()
}

#[cfg(target_os = "windows")]
fn hwnd_of(window: &Window) -> Option<isize> {
  use raw_window_handle::HasWindowHandle;
  let handle = window.window_handle().ok()?;
  match handle.as_raw() {
    raw_window_handle::RawWindowHandle::Win32(win) => Some(win.hwnd.get()),
    _ => None,
  }
}

pub(crate) fn with_window<T>(
  id: u64,
  body: impl FnOnce(&Window) -> T,
) -> Option<T> {
  OS.with(|slot| {
    let slot = slot.borrow();
    let os = slot.as_ref()?;
    let window_id = os.app.by_surface.get(&id)?;
    let host = os.app.windows.get(window_id)?;
    Some(body(&host.window))
  })
}

fn apply_cursor(window: &Window, name: &str, locked: bool) {
  window.set_cursor_visible(name != "none" && !locked);
  if let Ok(icon) = CursorIcon::from_str(name) {
    window.set_cursor(icon);
  }
}

impl App {
  fn create_pending(&mut self, event_loop: &ActiveEventLoop) {
    let pending = std::mem::take(&mut self.pending);
    for request in pending {
      let attributes = chrome_attributes(&request);
      match event_loop.create_window(attributes) {
        Ok(window) => {
          if request.position.is_none() {
            center_window(&window);
          }
          self.attach(request.id, window, request.transparent);
        }
        Err(error) => {
          self.errors.insert(request.id, error.to_string());
        }
      }
    }
  }

  fn attach(&mut self, id: u64, window: Window, transparent: bool) {
    if let Some(theme) = window.theme() {
      crate::prefs::note_scheme(theme == winit::window::Theme::Dark);
    }
    let window_id = window.id();
    let surface = self.make_surface(&window);
    self.windows.insert(
      window_id,
      HostWindow {
        surface,
        window,
        surface_id: id,
        transparent,
        configured: false,
        #[cfg(target_os = "macos")]
        frame_link: None,
      },
    );
    self.by_surface.insert(id, window_id);
    if transparent {
      TRANSPARENT_WINDOWS
        .lock()
        .expect("transparent windows")
        .insert(id);
    }
    self.publish_size(window_id);
    self.configure_surface(window_id);
    self.present_window(window_id);
    self.note_refresh(window_id);
  }

  fn make_surface(
    &mut self,
    window: &Window,
  ) -> Option<wgpu::Surface<'static>> {
    if self.instance.is_none()
      && let Some(instance) = crate::gpu::cloned_instance()
    {
      self.instance = Some(instance);
    }
    let instance = self.instance.get_or_insert_with(|| {
      wgpu::Instance::new(crate::gpu::instance_descriptor())
    });
    // The instance is created without a display handle. wgpu 29 then rejects
    // `from_window`, which leaves that handle empty, and the window stays the
    // unpainted view color. The window itself has the display handle.
    let target = match unsafe {
      wgpu::SurfaceTargetUnsafe::from_display_and_window(window, window)
    } {
      Ok(target) => target,
      Err(error) => {
        eprintln!("yuzuko: window surface: {error}");
        return None;
      }
    };
    let surface = match unsafe { instance.create_surface_unsafe(target) } {
      Ok(surface) => surface,
      Err(error) => {
        eprintln!("yuzuko: window surface: {error}");
        return None;
      }
    };
    // Safety: `HostWindow` drops this surface before its window.
    Some(unsafe {
      std::mem::transmute::<wgpu::Surface<'_>, wgpu::Surface<'static>>(surface)
    })
  }

  fn ensure_gpu(&mut self, window_id: WindowId) -> bool {
    if self.gpu.is_some() {
      return true;
    }
    if self.gpu_failed {
      return false;
    }
    let built = build_gpu(self.instance.as_ref(), self.windows.get(&window_id));
    match built {
      Ok(Some(gpu)) => {
        self.gpu = Some(gpu);
        true
      }
      Ok(None) => false,
      Err(message) => {
        self.note_gpu_failure(&message);
        false
      }
    }
  }

  fn note_gpu_failure(&mut self, message: &str) {
    if !self.gpu_failed {
      eprintln!("yuzuko: window will not clear: {message}");
    }
    self.gpu_failed = true;
  }

  fn note_refresh(&mut self, window_id: WindowId) {
    let Some(id) = self.windows.get(&window_id).map(|host| host.surface_id)
    else {
      return;
    };
    if let Some(host) = self.windows.get(&window_id) {
      note_refresh_from(&host.window, id);
    }
    #[cfg(target_os = "macos")]
    self.sync_link(window_id);
  }

  #[cfg(target_os = "macos")]
  fn sync_link(&mut self, window_id: WindowId) {
    if let Some(host) = self.windows.get_mut(&window_id) {
      frame_link::sync(host);
    }
  }

  fn publish_size(&mut self, window_id: WindowId) {
    let Some(host) = self.windows.get(&window_id) else {
      return;
    };
    let scale = host.window.scale_factor();
    let physical = host.window.inner_size();
    let scale = if scale.is_finite() && scale > 0.0 {
      scale
    } else {
      1.0
    };
    let size = Size {
      css_width: physical.width as f64 / scale,
      css_height: physical.height as f64 / scale,
      scale,
      bitmap_width: physical.width as f64,
      bitmap_height: physical.height as f64,
    };
    let id = host.surface_id;
    if inbox::set_size(id, size) {
      inbox::push(id, Raw::Resize);
    }
  }

  fn configure_surface(&mut self, window_id: WindowId) {
    if !self.ensure_gpu(window_id) {
      return;
    }
    let Some(surface_id) =
      self.windows.get(&window_id).map(|host| host.surface_id)
    else {
      return;
    };
    let Some(size) = inbox::size(surface_id) else {
      return;
    };
    let width = size.bitmap_width.round() as u32;
    let height = size.bitmap_height.round() as u32;
    if width == 0 || height == 0 {
      return;
    }
    let Some(gpu) = self.gpu.as_ref() else {
      return;
    };
    let config = wgpu::SurfaceConfiguration {
      usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
      format: gpu.format,
      width,
      height,
      present_mode: gpu.present_mode,
      alpha_mode: gpu.alpha,
      view_formats: vec![],
      desired_maximum_frame_latency: 2,
    };
    let Some(host) = self.windows.get_mut(&window_id) else {
      return;
    };
    let Some(surface) = host.surface.as_ref() else {
      return;
    };
    surface.configure(&gpu.device, &config);
    host.configured = true;
  }

  fn present_window(&mut self, window_id: WindowId) {
    if self.gpu.is_none() {
      self.configure_surface(window_id);
    }
    let Some(host) = self.windows.get(&window_id) else {
      return;
    };
    let window_surface = host.surface_id;
    let surface_id = crate::surface::canvas_for_window(window_surface)
      .unwrap_or(window_surface);
    let transparent = host.transparent;
    // Without a device, or before the first non-empty size, the surface was
    // never configured and has no texture to present.
    if !host.configured {
      return;
    }
    let Some(size) = inbox::size(window_surface) else {
      return;
    };
    let width = size.bitmap_width.round() as u32;
    let height = size.bitmap_height.round() as u32;
    if width == 0 || height == 0 {
      return;
    }
    let Some(gpu) = self.gpu.as_ref() else {
      return;
    };
    let device = gpu.device.clone();
    let queue = gpu.queue.clone();
    let format = gpu.format;
    // A readback waits on the GPU. Doing that while the swapchain texture is
    // held makes the next drawable time out after about a second.
    let canvas_ready =
      crate::canvas::stage_canvas(&device, &queue, surface_id, width, height);
    let webgpu_ready = !canvas_ready
      && crate::canvas::stage_webgpu(
        &device, &queue, surface_id, width, height,
      );
    let acquired = {
      let Some(host) = self.windows.get(&window_id) else {
        crate::canvas::discard_staged();
        return;
      };
      host.window.pre_present_notify();
      let Some(surface) = host.surface.as_ref() else {
        crate::canvas::discard_staged();
        return;
      };
      match surface.get_current_texture() {
        wgpu::CurrentSurfaceTexture::Success(frame)
        | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => {
          let size = frame.texture.size();
          if size.width == width && size.height == height {
            Ok(frame)
          } else {
            Err(true)
          }
        }
        wgpu::CurrentSurfaceTexture::Lost
        | wgpu::CurrentSurfaceTexture::Outdated => Err(true),
        wgpu::CurrentSurfaceTexture::Timeout
        | wgpu::CurrentSurfaceTexture::Occluded
        | wgpu::CurrentSurfaceTexture::Validation => Err(false),
      }
    };
    let frame = match acquired {
      Ok(frame) => frame,
      Err(true) => {
        crate::canvas::discard_staged();
        self.configure_surface(window_id);
        return;
      }
      Err(false) => {
        crate::canvas::discard_staged();
        return;
      }
    };
    let view = frame
      .texture
      .create_view(&wgpu::TextureViewDescriptor::default());
    if (canvas_ready
      && crate::canvas::present_canvas(
        &device, &queue, &view, surface_id, width, height, format,
      ))
      || (webgpu_ready
        && crate::canvas::present_webgpu(
          &device, &queue, &view, surface_id, width, height, format,
        ))
    {
      frame.present();
      return;
    }
    crate::canvas::discard_staged();
    let mut encoder =
      device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("yuzuko-clear"),
      });
    {
      let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("yuzuko-clear"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
          view: &view,
          depth_slice: None,
          resolve_target: None,
          ops: wgpu::Operations {
            load: wgpu::LoadOp::Clear(wgpu::Color {
              r: 0.0,
              g: 0.0,
              b: 0.0,
              a: if transparent { 0.0 } else { 1.0 },
            }),
            store: wgpu::StoreOp::Store,
          },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
      });
    }
    queue.submit([encoder.finish()]);
    frame.present();
  }

  fn on_window(&mut self, window_id: WindowId, event: WindowEvent) {
    let Some(id) = self.windows.get(&window_id).map(|host| host.surface_id)
    else {
      return;
    };
    match event {
      WindowEvent::CloseRequested => inbox::push(id, Raw::Close),
      WindowEvent::Destroyed => {
        self.by_surface.remove(&id);
        self.windows.remove(&window_id);
        TRANSPARENT_WINDOWS
          .lock()
          .expect("transparent windows")
          .remove(&id);
        inbox::push(id, Raw::Close);
      }
      WindowEvent::Focused(true) => inbox::push(id, Raw::Focus),
      WindowEvent::Focused(false) => inbox::push(id, Raw::Blur),
      WindowEvent::Occluded(occluded) => {
        inbox::push(id, Raw::Visibility(!occluded));
      }
      WindowEvent::Moved(_) => self.note_refresh(window_id),
      WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => {
        self.publish_size(window_id);
        self.configure_surface(window_id);
        self.note_refresh(window_id);
      }
      WindowEvent::ThemeChanged(theme) => {
        crate::prefs::note_scheme(theme == winit::window::Theme::Dark);
        inbox::push(id, crate::inbox::Raw::Media);
      }
      WindowEvent::RedrawRequested => self.present_window(window_id),
      WindowEvent::ModifiersChanged(modifiers) => {
        self.modifiers = modifiers.state();
      }
      WindowEvent::CursorMoved { position, .. } => {
        self.cursor.x = position.x;
        self.cursor.y = position.y;
        self.cursor.valid = true;
        if LOCKED.with(|locked| locked.get()) == Some(id) {
          return;
        }
        inbox::push(
          id,
          Raw::Pointer(self.pointer_at(window_id, PointerKind::Move, -1)),
        );
      }
      WindowEvent::MouseInput { state, button, .. } => {
        let kind = if state == ElementState::Pressed {
          PointerKind::Down
        } else {
          PointerKind::Up
        };
        inbox::push(
          id,
          Raw::Pointer(self.pointer_at(window_id, kind, mouse_button(button))),
        );
      }
      WindowEvent::MouseWheel { delta, .. } => {
        if let Some(wheel) = self.wheel_at(window_id, delta) {
          inbox::push(id, Raw::Wheel(wheel));
        }
      }
      WindowEvent::KeyboardInput {
        event,
        is_synthetic,
        ..
      } => {
        if is_synthetic {
          return;
        }
        inbox::push(id, Raw::Key(key_event(&event, self.modifiers)));
      }
      WindowEvent::Ime(ime) => {
        if !inbox::ime_allowed(id) {
          return;
        }
        if let Some(raw) = map_ime(ime) {
          inbox::push(id, Raw::Ime(raw));
        }
      }
      WindowEvent::Touch(touch) => {
        let kind = match touch.phase {
          winit::event::TouchPhase::Started => PointerKind::Down,
          winit::event::TouchPhase::Moved => PointerKind::Move,
          winit::event::TouchPhase::Ended => PointerKind::Up,
          winit::event::TouchPhase::Cancelled => PointerKind::Cancel,
        };
        let mut pointer = self.pointer_at(
          window_id,
          kind,
          if kind == PointerKind::Move { -1 } else { 0 },
        );
        pointer.pointer_type = "touch".to_string();
        pointer.pointer_id = 2 + (touch.id as u32 % 1_000_000) as i32;
        pointer.x = touch.location.x;
        pointer.y = touch.location.y;
        if let Some(force) = touch.force {
          pointer.pressure = Some(force.normalized().clamp(0.0, 1.0));
        }
        let (x, y, screen_x, screen_y) = point_of(
          self.windows.get(&window_id).map(|host| &host.window),
          touch.location.x,
          touch.location.y,
        );
        pointer.x = x;
        pointer.y = y;
        pointer.screen_x = screen_x;
        pointer.screen_y = screen_y;
        inbox::push(id, Raw::Pointer(pointer));
      }
      _ => {}
    }
  }

  fn pointer_at(
    &self,
    window_id: WindowId,
    kind: PointerKind,
    button: i32,
  ) -> RawPointer {
    let window = self.windows.get(&window_id).map(|host| &host.window);
    let (x, y, screen_x, screen_y) =
      point_of(window, self.cursor.x, self.cursor.y);
    let (ctrl, shift, alt, meta) = modifier_flags(self.modifiers);
    RawPointer {
      kind,
      pointer_id: 1,
      pointer_type: "mouse".to_string(),
      is_primary: true,
      x,
      y,
      screen_x,
      screen_y,
      button,
      pressure: None,
      movement_x: None,
      movement_y: None,
      ctrl,
      shift,
      alt,
      meta,
      tilt_x: 0,
      tilt_y: 0,
      twist: 0,
      width: 1.0,
      height: 1.0,
      tangential: 0.0,
    }
  }

  fn wheel_at(
    &self,
    window_id: WindowId,
    delta: MouseScrollDelta,
  ) -> Option<RawWheel> {
    let window = self.windows.get(&window_id).map(|host| &host.window);
    let scale = window.map(|window| window.scale_factor()).unwrap_or(1.0);
    let scale = if scale.is_finite() && scale > 0.0 {
      scale
    } else {
      1.0
    };
    // winit's positive axis moves content down and right, matching WheelEvent.
    let (delta_x, delta_y, delta_mode) = match delta {
      MouseScrollDelta::LineDelta(x, y) => (f64::from(x), f64::from(y), 1),
      MouseScrollDelta::PixelDelta(position) => {
        (position.x / scale, position.y / scale, 0)
      }
    };
    let (x, y, screen_x, screen_y) =
      point_of(window, self.cursor.x, self.cursor.y);
    let (ctrl, shift, alt, meta) = modifier_flags(self.modifiers);
    Some(RawWheel {
      x,
      y,
      screen_x,
      screen_y,
      delta_x,
      delta_y,
      delta_z: 0.0,
      delta_mode,
      ctrl,
      shift,
      alt,
      meta,
    })
  }

  fn on_motion(&mut self, delta: (f64, f64)) {
    let Some(id) = LOCKED.with(|locked| locked.get()) else {
      return;
    };
    let Some(window_id) = self.by_surface.get(&id).copied() else {
      return;
    };
    let mut pointer = self.pointer_at(window_id, PointerKind::Move, -1);
    pointer.movement_x = Some(delta.0);
    pointer.movement_y = Some(delta.1);
    inbox::push(id, Raw::Pointer(pointer));
  }
}

impl ApplicationHandler<Wake> for App {
  fn resumed(&mut self, event_loop: &ActiveEventLoop) {
    self.create_pending(event_loop);
  }

  fn window_event(
    &mut self,
    _event_loop: &ActiveEventLoop,
    window_id: WindowId,
    event: WindowEvent,
  ) {
    self.on_window(window_id, event);
  }

  fn user_event(&mut self, event_loop: &ActiveEventLoop, _event: Wake) {
    self.create_pending(event_loop);
  }

  fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
    self.create_pending(event_loop);
  }

  fn device_event(
    &mut self,
    _event_loop: &ActiveEventLoop,
    _device_id: winit::event::DeviceId,
    event: DeviceEvent,
  ) {
    if let DeviceEvent::MouseMotion { delta } = event {
      self.on_motion(delta);
    }
  }
}

fn build_gpu(
  instance: Option<&wgpu::Instance>,
  host: Option<&HostWindow>,
) -> Result<Option<Gpu>, String> {
  let Some(instance) = instance else {
    return Ok(None);
  };
  let Some(surface) = host.and_then(|host| host.surface.as_ref()) else {
    return Ok(None);
  };
  let (adapter, device, queue) = if let Some(parts) = crate::gpu::cloned_parts()
  {
    parts
  } else {
    let adapter = pollster::block_on(instance.request_adapter(
      &wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::default(),
        force_fallback_adapter: false,
        compatible_surface: Some(surface),
      },
    ))
    .map_err(|error| error.to_string())?;
    let (device, queue) = crate::gpu::request_device(&adapter)?;
    (adapter, device, queue)
  };
  let caps = surface.get_capabilities(&adapter);
  let format = choose_format(&caps.formats).ok_or_else(|| {
    "The window surface has no presentation format.".to_string()
  })?;
  let present_mode = caps
    .present_modes
    .first()
    .copied()
    .ok_or_else(|| "The window surface has no present mode.".to_string())?;
  let alpha = choose_alpha(&caps.alpha_modes)
    .ok_or_else(|| "The window surface has no alpha mode.".to_string())?;
  Ok(Some(Gpu {
    device,
    queue,
    format,
    present_mode,
    alpha,
  }))
}

fn choose_format(
  formats: &[wgpu::TextureFormat],
) -> Option<wgpu::TextureFormat> {
  const PREFERRED: &[wgpu::TextureFormat] = &[
    wgpu::TextureFormat::Bgra8Unorm,
    wgpu::TextureFormat::Rgba8Unorm,
    wgpu::TextureFormat::Bgra8UnormSrgb,
    wgpu::TextureFormat::Rgba8UnormSrgb,
  ];
  PREFERRED
    .iter()
    .copied()
    .find(|format| formats.contains(format))
    .or_else(|| formats.first().copied())
}

fn choose_alpha(
  modes: &[wgpu::CompositeAlphaMode],
) -> Option<wgpu::CompositeAlphaMode> {
  modes
    .iter()
    .copied()
    .find(|mode| *mode == wgpu::CompositeAlphaMode::PreMultiplied)
    .or_else(|| modes.first().copied())
}

/// CSS pixels for a physical window point. The origin is the client area's
/// top-left, and `y` grows downward, matching `PointerEvent`.
fn point_of(
  window: Option<&Window>,
  physical_x: f64,
  physical_y: f64,
) -> (f64, f64, f64, f64) {
  let scale = window.map(|window| window.scale_factor()).unwrap_or(1.0);
  let scale = if scale.is_finite() && scale > 0.0 {
    scale
  } else {
    1.0
  };
  let x = physical_x / scale;
  let y = physical_y / scale;
  let (screen_x, screen_y) =
    match window.and_then(|window| window.inner_position().ok()) {
      Some(origin) => (
        (origin.x as f64 + physical_x) / scale,
        (origin.y as f64 + physical_y) / scale,
      ),
      None => (x, y),
    };
  (x, y, screen_x, screen_y)
}

fn modifier_flags(state: ModifiersState) -> (bool, bool, bool, bool) {
  (
    state.control_key(),
    state.shift_key(),
    state.alt_key(),
    state.super_key(),
  )
}

fn mouse_button(button: MouseButton) -> i32 {
  match button {
    MouseButton::Left => 0,
    MouseButton::Middle => 1,
    MouseButton::Right => 2,
    MouseButton::Back => 3,
    MouseButton::Forward => 4,
    MouseButton::Other(code) => i32::from(code),
  }
}

fn key_event(
  event: &winit::event::KeyEvent,
  modifiers: ModifiersState,
) -> RawKey {
  let (ctrl, shift, alt, meta) = modifier_flags(modifiers);
  RawKey {
    down: event.state == ElementState::Pressed,
    key: keys::key_name(&event.logical_key),
    code: keys::code_name(event.physical_key).to_string(),
    location: keys::location_value(event.location),
    repeat: event.repeat,
    text: event.text.as_ref().map(|text| text.to_string()),
    ctrl,
    shift,
    alt,
    meta,
  }
}

fn map_ime(ime: Ime) -> Option<RawIme> {
  match ime {
    Ime::Enabled => Some(RawIme::Enabled),
    Ime::Preedit(text, _) => Some(RawIme::Preedit(text)),
    Ime::Commit(text) => Some(RawIme::Commit(text)),
    Ime::Disabled => Some(RawIme::Disabled),
  }
}
