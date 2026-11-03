//! `EventTarget`, events, `DOMException`, and `AbortController`.

mod abort;
mod animation;
mod event;
mod exception;
mod idle;
mod message;
mod net;
mod performance;
mod reader;
mod report;
mod stream_transfer;
mod target;
mod ui;
mod util;
mod worker;

pub use animation::set_interval_hook as set_worker_frame_interval_hook;
pub use event::{
  Event, define_is_trusted, dispatch_custom_on_path, dispatch_on_path,
};
pub use message::entangled_ports;
pub use report::report_error;
pub use target::EventTarget;
pub use target::dispatch;
pub use ui::{
  FiredComposition, FiredInput, FiredKey, FiredPointer, FiredWheel,
  dispatch_composition, dispatch_focus, dispatch_input, dispatch_key,
  dispatch_pointer, dispatch_pointer_through, dispatch_simple, dispatch_wheel,
};

use std::sync::Once;

pub fn register() {
  static ONCE: Once = Once::new();
  ONCE.call_once(|| {
    yuzuko_core::add_extension(&install);
    yuzuko_core::on_realm_drop(clear);
    exception::register_clone();
    message::register_clone();
    stream_transfer::register_clone();
    yuzuko_core::set_inbound_handler(worker::drain);
    yuzuko_core::set_idle_hooks(yuzuko_core::IdleHooks {
      has_pending: idle::has_pending,
      next_wait: idle::next_wait,
      poll_timeout: idle::poll_timeout,
      poll_idle: idle::poll_idle,
    });
    #[cfg(feature = "fetch")]
    yuzuko_winter::set_xhr_progress_hook(event::deliver_xhr_progress);
  });
}

fn install(scope: &mut v8::PinScope) {
  exception::install(scope);
  event::install(scope);
  #[cfg(feature = "window")]
  ui::install(scope);
  target::install(scope);
  performance::install(scope);
  abort::install(scope);
  message::install(scope);
  reader::install(scope);
  report::install(scope);
  worker::install(scope);
  idle::install(scope);
  #[cfg(feature = "fetch")]
  net::event_source::install(scope);
  #[cfg(feature = "socket")]
  net::websocket::install(scope);
  #[cfg(feature = "socket")]
  net::websocket_stream::install(scope);
  #[cfg(feature = "socket")]
  net::webtransport::install(scope);
}

/// Dispatch a trusted, non-bubbling `Event` at `target`.
pub fn dispatch_trusted_event(
  scope: &mut v8::PinScope,
  target: v8::Local<v8::Object>,
  type_name: &str,
) {
  let event = event::Event::new(type_name, false, false, false, true);
  let object = unsafe { yuzuko_core::op::wrap_instance(scope, event) };
  event::define_is_trusted(scope, object);
  let _ = target::dispatch(scope, target, object);
}

fn clear() {
  target::clear();
  message::clear();
  report::clear();
  event::clear_is_trusted();
  worker::clear();
  idle::clear();
  #[cfg(feature = "fetch")]
  net::event_source::clear();
  #[cfg(feature = "socket")]
  net::websocket::clear();
  #[cfg(feature = "socket")]
  net::websocket_stream::clear();
  #[cfg(feature = "socket")]
  net::webtransport::clear();
}
