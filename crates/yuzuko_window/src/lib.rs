//! `Yuzuko.Window`, headless frames, and OS windows.

mod activation;
mod canvas;
mod fit;
mod frame;
mod gpu;
mod identity;
mod inbox;
mod input;
mod media;
mod os;
mod png_rgba;
mod prefs;
mod session;
mod shell;
mod surface;
mod test_api;

use std::sync::Once;
use std::sync::atomic::{AtomicBool, Ordering};

static TEST_INPUT: AtomicBool = AtomicBool::new(false);

pub fn register() {
  static ONCE: Once = Once::new();
  ONCE.call_once(|| {
    yuzuko_core::add_extension(&install);
    yuzuko_core::on_realm_drop(shutdown);
    yuzuko_core::set_loop_hooks(frame::drive, frame::interest, frame::wait);
    yuzuko_core::set_worker_frame_hook(surface::commit_worker_frames);
    yuzuko_core::set_idle_frame_delay(inbox::next_frame_wait);
    canvas::register_transfer();
  });
}

/// `YUZUKO_HEADLESS=1` also skips OS window creation. Omitted constructor
/// `width` and `height` are 1280×720 CSS pixels.
pub fn configure(headless: bool) {
  session::configure(headless);
}

/// Marks a `yuzuko test` process. Real gamepads are not polled. `Yuzuko.__test`
/// is installed only when `YUZUKO_TEST_INPUT=1`.
pub fn set_test_input(enabled: bool) {
  TEST_INPUT.store(enabled, Ordering::SeqCst);
}

fn test_inject_enabled() -> bool {
  std::env::var("YUZUKO_TEST_INPUT").ok().as_deref() == Some("1")
}

pub use canvas::{
  CanvasHooks, CanvasSnapshot, ContextKind, OffscreenCanvas, WebGpuHooks,
  install_hooks, install_presented_bitmap, install_webgpu_hooks,
  offscreen_bitmap, offscreen_canvas, present_canvas, present_webgpu,
  reset_canvas_bitmap, snapshot_canvas,
};
pub use fit::{
  ImageRendering, ObjectFit, PresentStyle, composite, composite_pad,
  rendering_is_smooth,
};
pub use gpu::{
  ensure as ensure_gpu, failed as gpu_failed, foreign as gpu_foreign,
  with_device, with_gpu,
};
pub use identity::set_app_identity;
pub use os::{canvas_is_transparent, request_redraw};
pub use session::headless;
pub use surface::{canvas_style, default_title, set_default_title};

fn install(scope: &mut v8::PinScope) {
  canvas::OffscreenCanvas::install(scope);
  if yuzuko_core::is_dedicated_worker() {
    return;
  }
  shell::menu::install(scope);
  shell::tray::install(scope);
  shell::dock::install(scope);
  surface::install(scope);
  shell::clipboard::install(scope);
  input::gamepad::install(scope);
  if TEST_INPUT.load(Ordering::SeqCst) && test_inject_enabled() {
    test_api::install(scope);
  }
}

fn shutdown() {
  shell::tray::clear();
  shell::dock::clear();
  shell::menu::clear();
  os::shutdown();
  inbox::clear();
  input::gamepad::clear();
}
