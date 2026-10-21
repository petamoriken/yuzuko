//! Per-realm state. V8 callbacks reach it through a thread-local.
//!
//! The main realm stays on the main thread. A dedicated worker runs on its
//! own thread and enters only its own isolate.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Instant, SystemTime};

use oxc_resolver::Resolver;

use crate::platform::TaskQueue;

pub struct ModuleEntry {
  pub url: String,
  pub path: PathBuf,
  pub module: v8::Global<v8::Module>,
}

pub struct RegisteredTest {
  pub name: String,
  pub function: v8::Global<v8::Function>,
  pub ignore: bool,
  pub only: bool,
}

pub enum TimerCallback {
  Function(v8::Global<v8::Function>),
  Code(String),
  /// Report promise rejections that still have no handler.
  FlushRejections,
}

pub struct DeferredRejection {
  pub promise: v8::Global<v8::Promise>,
  pub reason: Option<v8::Global<v8::Value>>,
}

pub struct Timer {
  pub id: i32,
  pub generation: u32,
  pub when: Instant,
  pub repeat_ms: Option<f64>,
  pub callback: TimerCallback,
  pub args: Vec<v8::Global<v8::Value>>,
}

/// Which global this isolate is hosting.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RealmKind {
  Window,
  DedicatedWorker,
}

pub struct HostState {
  pub queue: Arc<TaskQueue>,
  pub kind: RealmKind,
  /// `0` is the main realm. Dedicated workers use a process-wide id.
  pub agent_id: u64,
  pub resolver: Resolver,
  pub modules: HashMap<i32, ModuleEntry>,
  pub strip_cache: HashMap<PathBuf, (SystemTime, u64, String)>,
  pub json_sources: HashMap<i32, String>,
  /// Compiled `WebAssembly.Module` objects, keyed by the module file URL.
  /// Source-phase imports return these without instantiating them.
  pub wasm_modules: HashMap<String, v8::Global<v8::Object>>,
  /// `WebAssembly.Instance` objects. Keys are the module URL and, after
  /// evaluation, the module namespace. Importers read `.exports` so a
  /// mutable global stays a `WebAssembly.Global`.
  pub wasm_instances: Option<v8::Global<v8::Map>>,
  /// Function templates for `#[op]` classes, so inheritance reuses one template.
  pub templates: HashMap<std::any::TypeId, v8::Global<v8::FunctionTemplate>>,
  pub unhandled: HashMap<i32, String>,
  /// Rejections waiting for the notification task. A handler attached before
  /// that task removes the entry and no event is fired.
  pub deferred_rejections: HashMap<i32, DeferredRejection>,
  pub rejection_notify_queued: bool,
  pub capture_log: bool,
  pub logs: Vec<String>,
  pub tests: Vec<RegisteredTest>,
  pub in_test: bool,
  pub test_error: Option<String>,
  pub timers: Vec<Timer>,
  pub timer_generation: HashMap<i32, u32>,
  pub next_timer_id: i32,
  pub timer_nesting: u32,
  pub time_origin: Instant,
  pub time_origin_ms: f64,
  pub rejection_hook: Option<v8::Global<v8::Function>>,
  pub dom_exception: Option<v8::Global<v8::Function>>,
  /// Directory of the entry module. `file:` fetch may not leave it.
  pub entry_dir: Option<PathBuf>,
  /// Absolute `file:` URL of the entry module. Relative fetch resolves here.
  pub entry_url: Option<String>,
}

thread_local! {
    static STATE: RefCell<Option<HostState>> = const { RefCell::new(None) };
}

impl HostState {
  pub(crate) fn empty(
    queue: Arc<TaskQueue>,
    kind: RealmKind,
    agent_id: u64,
  ) -> Self {
    let (time_origin, time_origin_ms) = time_origin();
    Self {
      queue,
      kind,
      agent_id,
      resolver: crate::loader::modules::resolver(),
      modules: HashMap::new(),
      strip_cache: HashMap::new(),
      json_sources: HashMap::new(),
      wasm_modules: HashMap::new(),
      wasm_instances: None,
      templates: HashMap::new(),
      unhandled: HashMap::new(),
      deferred_rejections: HashMap::new(),
      rejection_notify_queued: false,
      capture_log: false,
      logs: Vec::new(),
      tests: Vec::new(),
      in_test: false,
      test_error: None,
      timers: Vec::new(),
      timer_generation: HashMap::new(),
      next_timer_id: 0,
      timer_nesting: 0,
      time_origin,
      time_origin_ms,
      rejection_hook: None,
      dom_exception: None,
      entry_dir: None,
      entry_url: None,
    }
  }
}

pub fn enter(state: HostState) {
  STATE.with(|slot| *slot.borrow_mut() = Some(state));
}

pub fn exit() {
  STATE.with(|slot| *slot.borrow_mut() = None);
}

#[inline]
pub fn with<T>(f: impl FnOnce(&HostState) -> T) -> T {
  STATE.with(|slot| {
    let state = slot.borrow();
    f(state.as_ref().expect("runtime state"))
  })
}

#[inline]
pub fn with_mut<T>(f: impl FnOnce(&mut HostState) -> T) -> T {
  STATE.with(|slot| {
    let mut state = slot.borrow_mut();
    f(state.as_mut().expect("runtime state"))
  })
}

pub fn module_path(hash: i32) -> Option<PathBuf> {
  with(|state| state.modules.get(&hash).map(|entry| entry.path.clone()))
}

pub fn module_url(hash: i32) -> Option<String> {
  with(|state| state.modules.get(&hash).map(|entry| entry.url.clone()))
}

pub fn wasm_instances<'s>(
  scope: &mut v8::PinScope<'s, '_>,
) -> v8::Local<'s, v8::Map> {
  let existing = with(|state| state.wasm_instances.clone());
  if let Some(map) = existing {
    return v8::Local::new(scope, map);
  }
  let map = v8::Map::new(scope);
  let stored = v8::Global::new(scope, map);
  with_mut(|state| state.wasm_instances = Some(stored));
  map
}

#[inline]
pub fn queue() -> Arc<TaskQueue> {
  with(|state| Arc::clone(&state.queue))
}

#[inline]
pub fn is_dedicated_worker() -> bool {
  with(|state| state.kind == RealmKind::DedicatedWorker)
}

/// Whether this thread is a dedicated worker.
///
/// A thread that has not entered a realm is not one. Callers that record a
/// canvas draw use this so a unit test can mark the canvas without a realm.
#[inline]
pub fn on_dedicated_worker() -> bool {
  STATE.with(|slot| {
    slot
      .borrow()
      .as_ref()
      .is_some_and(|state| state.kind == RealmKind::DedicatedWorker)
  })
}

#[inline]
pub fn agent_id() -> u64 {
  with(|state| state.agent_id)
}

#[inline]
pub fn try_agent_id() -> Option<u64> {
  STATE.with(|slot| slot.borrow().as_ref().map(|state| state.agent_id))
}

pub fn entry_dir() -> Option<PathBuf> {
  with(|state| state.entry_dir.clone())
}

pub fn entry_url() -> Option<String> {
  with(|state| state.entry_url.clone())
}

/// Resolves relative `fetch` and `Request` URLs against `url`.
///
/// The WPT runner points network tests at `http://web-platform.test` while
/// `file:` stays sandboxed under the suite directory.
pub fn set_entry_url(url: String) {
  with_mut(|state| state.entry_url = Some(url));
}

/// Widens the `file:` sandbox to a directory without changing the entry URL.
///
/// The WPT runner uses this so a test can fetch `/images/...` under the suite
/// root while relative URLs still resolve against the test file.
pub fn set_sandbox_dir(path: &std::path::Path) {
  let absolute = crate::loader::modules::absolute(path)
    .unwrap_or_else(|_| path.to_path_buf());
  let dir = crate::loader::modules::canonicalize(&absolute).unwrap_or(absolute);
  with_mut(|state| {
    state.entry_dir = Some(dir);
  });
}

/// Records the entry file used to sandbox `file:` fetch.
pub fn set_file_root(path: &std::path::Path) {
  let absolute = crate::loader::modules::absolute(path)
    .unwrap_or_else(|_| path.to_path_buf());
  let file =
    crate::loader::modules::canonicalize(&absolute).unwrap_or(absolute);
  let dir = file
    .parent()
    .map(std::path::Path::to_path_buf)
    .unwrap_or_else(|| file.clone());
  let url = crate::loader::modules::file_url(&file).ok();
  with_mut(|state| {
    state.entry_dir = Some(dir);
    state.entry_url = url;
  });
}

/// Records the first failure of the test that is currently running.
///
/// Returns whether a test is running. Callers report the exception on the
/// page only when this is false.
pub fn note_test_error(message: impl Into<String>) -> bool {
  with_mut(|state| {
    if !state.in_test {
      return false;
    }
    if state.test_error.is_none() {
      state.test_error = Some(message.into());
    }
    true
  })
}

pub fn set_rejection_hook(
  scope: &mut v8::PinScope,
  function: v8::Local<v8::Function>,
) {
  let function = v8::Global::new(scope, function);
  with_mut(|state| state.rejection_hook = Some(function));
}

pub fn set_dom_exception(
  scope: &mut v8::PinScope,
  constructor: v8::Local<v8::Function>,
) {
  let constructor = v8::Global::new(scope, constructor);
  with_mut(|state| state.dom_exception = Some(constructor));
}

pub fn time_origin() -> (Instant, f64) {
  let instant = Instant::now();
  let millis = SystemTime::now()
    .duration_since(SystemTime::UNIX_EPOCH)
    .map(|duration| duration.as_secs_f64() * 1000.0)
    .unwrap_or(0.0);
  (instant, millis)
}
