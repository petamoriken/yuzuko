//! Web Audio on `globalThis`.

mod classes;
mod decode;
mod engine;
mod hardware;
mod playback;
mod util;

use std::sync::Once;

pub fn register() {
  static ONCE: Once = Once::new();
  ONCE.call_once(|| {
    yuzuko_core::add_extension(&install);
    yuzuko_core::on_realm_drop(shutdown);
  });
}

fn install(scope: &mut v8::PinScope) {
  classes::install(scope);
}

fn shutdown() {
  engine::clear();
  classes::clear();
}
