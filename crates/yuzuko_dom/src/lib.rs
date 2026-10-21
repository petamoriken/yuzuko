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
mod target;
mod ui;
mod util;
mod worker;

pub use event::{Event, define_is_trusted};
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
    yuzuko_core::set_inbound_handler(worker::drain);
    yuzuko_core::set_idle_hooks(yuzuko_core::IdleHooks {
      has_pending: idle::has_pending,
      next_wait: idle::next_wait,
      poll_timeout: idle::poll_timeout,
      poll_idle: idle::poll_idle,
    });
  });
}

fn install(scope: &mut v8::PinScope) {
  exception::install(scope);
  event::install(scope);
  ui::install(scope);
  target::install(scope);
  performance::install(scope);
  abort::install(scope);
  message::install(scope);
  reader::install(scope);
  report::install(scope);
  worker::install(scope);
  idle::install(scope);
  net::event_source::install(scope);
  net::websocket::install(scope);
  net::websocket_stream::install(scope);
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
  net::event_source::clear();
  net::websocket::clear();
  net::websocket_stream::clear();
  net::webtransport::clear();
}
