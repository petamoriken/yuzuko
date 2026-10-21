//! WebGPU on the process wgpu device.
//!
//! The binding is handwritten. Generating the rest of the specification
//! would publish methods that succeed as no-ops, and those methods throw
//! `NotSupportedError` instead.

mod canvas;
mod device;
mod encode;
mod js;
mod resource;
mod state;

use std::sync::Once;

pub fn register() {
  static ONCE: Once = Once::new();
  ONCE.call_once(|| {
    yuzuko_window::install_webgpu_hooks(yuzuko_window::WebGpuHooks {
      get_context: canvas::get_context,
      stage: canvas::stage,
      present: canvas::present,
      discard_present: canvas::discard_staged,
      expire: canvas::expire,
      resized: canvas::resized,
      dispose: canvas::dispose,
    });
    yuzuko_core::add_extension(&device::install);
    yuzuko_core::on_realm_drop(shutdown);
  });
}

fn shutdown() {
  state::clear();
  canvas::clear();
}

pub(crate) fn device_id(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Option<u64> {
  device::device_id(scope, value)
}
