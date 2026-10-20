//! Application menu, status item, dock, dialogs, and clipboard.

pub(crate) mod clipboard;
pub(crate) mod dialog;
pub(crate) mod dock;
pub(crate) mod menu;
pub(crate) mod tray;

/// GTK for the Linux dialogs and the AppIndicator tray. winit does not run a
/// GLib main loop, so the window pump drains GLib events after winit's.
#[cfg(target_os = "linux")]
pub(crate) mod gtk_loop {
  use std::sync::Once;
  use std::sync::atomic::{AtomicBool, Ordering};

  static READY: Once = Once::new();
  static OK: AtomicBool = AtomicBool::new(false);

  /// Initializes GTK once on the main thread.
  pub fn ready() -> Result<(), String> {
    READY.call_once(|| {
      OK.store(gtk::init().is_ok(), Ordering::SeqCst);
    });
    if OK.load(Ordering::SeqCst) {
      Ok(())
    } else {
      Err("GTK could not start.".to_string())
    }
  }

  /// Whether GTK has started, so the pump must keep GLib moving.
  pub fn active() -> bool {
    OK.load(Ordering::SeqCst)
  }

  /// Runs the GLib events that are already due without blocking.
  pub fn drain() {
    if !active() {
      return;
    }
    while gtk::events_pending() {
      gtk::main_iteration_do(false);
    }
  }
}
