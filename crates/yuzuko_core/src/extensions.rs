//! Realm extensions installed by feature crates. The binary registers them.

use std::sync::Mutex;

pub type Extension =
  &'static (dyn for<'s, 'i> Fn(&mut v8::PinScope<'s, 'i>) + Sync);

static EXTENSIONS: Mutex<Vec<Extension>> = Mutex::new(Vec::new());
static CLEANUPS: Mutex<Vec<fn()>> = Mutex::new(Vec::new());

pub fn add_extension(extension: Extension) {
  EXTENSIONS.lock().expect("extensions").push(extension);
}

pub fn on_realm_drop(cleanup: fn()) {
  CLEANUPS.lock().expect("cleanups").push(cleanup);
}

pub fn install(scope: &mut v8::PinScope) {
  let extensions = EXTENSIONS.lock().expect("extensions").clone();
  for extension in extensions {
    extension(scope);
  }
}

pub fn cleanup() {
  let cleanups = CLEANUPS.lock().expect("cleanups").clone();
  for cleanup in cleanups {
    cleanup();
  }
}
