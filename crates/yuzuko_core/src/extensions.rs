//! Realm extensions installed by feature crates. The binary registers them.

use std::sync::Mutex;

pub type Extension =
  &'static (dyn for<'s, 'i> Fn(&mut v8::PinScope<'s, 'i>) + Sync);

static EXTENSIONS: Mutex<Vec<Extension>> = Mutex::new(Vec::new());
static CLEANUPS: Mutex<Vec<fn()>> = Mutex::new(Vec::new());
static RELOADS: Mutex<Vec<fn()>> = Mutex::new(Vec::new());
static RELOAD_SCOPES: Mutex<Vec<ReloadScope>> = Mutex::new(Vec::new());
static RELOAD_SETTLED: Mutex<Vec<ReloadSettled>> = Mutex::new(Vec::new());

pub type ReloadScope =
  &'static (dyn for<'s, 'i> Fn(&mut v8::PinScope<'s, 'i>) + Sync);
pub type ReloadSettled =
  &'static (dyn for<'s, 'i> Fn(&mut v8::PinScope<'s, 'i>, bool) + Sync);

pub fn add_extension(extension: Extension) {
  EXTENSIONS.lock().expect("extensions").push(extension);
}

pub fn on_realm_drop(cleanup: fn()) {
  CLEANUPS.lock().expect("cleanups").push(cleanup);
}

/// Runs when `--watch-hmr` is about to evaluate the next graph.
///
/// The realm stays alive. This is not [`on_realm_drop`]. A reload must not
/// drop the process event loop. The hook has no scope; use
/// [`on_reload_scope`] to touch V8 objects.
pub fn on_reload(hook: fn()) {
  RELOADS.lock().expect("reloads").push(hook);
}

/// Like [`on_reload`], with the entered realm. Runs after every [`on_reload`].
pub fn on_reload_scope(hook: ReloadScope) {
  RELOAD_SCOPES.lock().expect("reloads").push(hook);
}

/// Runs after the new graph is evaluated and older module records are dropped.
///
/// `keep_current` is true when this generation's module records are still
/// live, so an object created by that evaluation stays even when the walk
/// cannot see the module binding. It is false when that generation was
/// purged. Only objects reachable from `globalThis` stay in that case.
pub fn on_reload_settled(hook: ReloadSettled) {
  RELOAD_SETTLED.lock().expect("reloads").push(hook);
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

pub fn reload() {
  let hooks = RELOADS.lock().expect("reloads").clone();
  for hook in hooks {
    hook();
  }
}

pub fn reload_scope(scope: &mut v8::PinScope) {
  let hooks = RELOAD_SCOPES.lock().expect("reloads").clone();
  for hook in hooks {
    hook(scope);
  }
}

pub fn reload_settled(scope: &mut v8::PinScope, keep_current: bool) {
  let hooks = RELOAD_SETTLED.lock().expect("reloads").clone();
  for hook in hooks {
    hook(scope, keep_current);
  }
}
