//! Process entry points for `yuzuko run` and `yuzuko test`.

use std::any::TypeId;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, Once, OnceLock};

use crate::host::{self, RealmKind, RegisteredTest};
use crate::loader::modules::{self, dynamic_import, instantiate_and_evaluate};
use crate::platform::{HostPlatform, TaskQueue};

static V8_INIT: Once = Once::new();
static QUEUE: OnceLock<Arc<TaskQueue>> = OnceLock::new();
static PLATFORM: OnceLock<v8::SharedRef<v8::Platform>> = OnceLock::new();
static RUNTIME: Mutex<()> = Mutex::new(());
static RETAIN_HOST_LOOP: AtomicBool = AtomicBool::new(false);

struct ConsoleState {
  times: HashMap<String, f64>,
  counts: HashMap<String, u32>,
  depth: u32,
}

thread_local! {
  static WASM_IN_FLIGHT: Cell<i32> = const { Cell::new(0) };
  static CONSOLE: RefCell<ConsoleState> = RefCell::new(ConsoleState {
    times: HashMap::new(),
    counts: HashMap::new(),
    depth: 0,
  });
  /// The line editor is sitting on a prompt. The next console line starts
  /// on its own row, and [`editor_line_broke`] reports that.
  static EDITOR_LINE: Cell<bool> = const { Cell::new(false) };
  static EDITOR_BROKE: Cell<bool> = const { Cell::new(false) };
}

/// Marks that the Unix line editor owns the current terminal row.
pub(crate) fn editor_hold_line(hold: bool) {
  EDITOR_LINE.with(|flag| flag.set(hold));
  if hold {
    EDITOR_BROKE.with(|flag| flag.set(false));
  }
}

/// Whether console output moved off the prompt line since the last call.
pub(crate) fn editor_line_broke() -> bool {
  EDITOR_BROKE.with(|flag| flag.replace(false))
}

fn wasm_in_flight() -> i32 {
  WASM_IN_FLIGHT.with(|count| count.get())
}

fn wasm_enter(
  scope: &mut v8::PinScope,
  _args: v8::FunctionCallbackArguments,
  mut _ret: v8::ReturnValue,
) {
  let _ = scope;
  WASM_IN_FLIGHT.with(|count| count.set(count.get().saturating_add(1)));
}

fn wasm_leave(
  scope: &mut v8::PinScope,
  _args: v8::FunctionCallbackArguments,
  mut _ret: v8::ReturnValue,
) {
  let _ = scope;
  WASM_IN_FLIGHT.with(|count| count.set(count.get().saturating_sub(1)));
}

#[derive(Debug)]
pub struct RuntimeError {
  pub message: String,
  /// Set when `Yuzuko.exit` requested this status. The message stays empty.
  pub exit_code: Option<u8>,
  /// Set when a watch session should load this path again.
  pub reload_path: Option<PathBuf>,
  /// DOMException name when the failure is a web exception.
  pub dom_name: Option<String>,
}

impl RuntimeError {
  pub fn message(message: impl Into<String>) -> Self {
    Self {
      message: message.into(),
      exit_code: None,
      reload_path: None,
      dom_name: None,
    }
  }

  pub fn dom(name: impl Into<String>, message: impl Into<String>) -> Self {
    Self {
      message: message.into(),
      exit_code: None,
      reload_path: None,
      dom_name: Some(name.into()),
    }
  }

  pub fn exit(code: u8) -> Self {
    Self {
      message: String::new(),
      exit_code: Some(code),
      reload_path: None,
      dom_name: None,
    }
  }

  pub(crate) fn reload(path: PathBuf) -> Self {
    let message = path.display().to_string();
    Self {
      message,
      exit_code: None,
      reload_path: Some(path),
      dom_name: None,
    }
  }
}

/// How `yuzuko run` watches the module graph.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WatchMode {
  /// A new isolate and realm for each run.
  Restart,
  /// The same isolate and realm. The next generation is a new module graph.
  Hot,
}

/// Keeps the process window loop across `--watch` realm restarts.
///
/// winit often cannot build a second `EventLoop` in one process. A watch
/// session closes windows and leaves the loop in place.
pub fn set_retain_host_loop(retain: bool) {
  RETAIN_HOST_LOOP.store(retain, Ordering::Relaxed);
}

pub fn retain_host_loop() -> bool {
  RETAIN_HOST_LOOP.load(Ordering::Relaxed)
}

/// Maps a finished entry to a process status.
///
/// A normal return is 0. `Yuzuko.exit` returns its code and is not an error
/// to print. Any other failure stays an error.
pub fn status_of(result: Result<(), RuntimeError>) -> Result<u8, RuntimeError> {
  match result {
    Ok(()) => Ok(0),
    Err(error) => match error.exit_code {
      Some(code) => Ok(code),
      None => Err(error),
    },
  }
}

fn exiting() -> bool {
  crate::process::exit_code().is_some()
}

fn bail_if_exit() -> Result<(), RuntimeError> {
  match crate::process::exit_code() {
    Some(code) => Err(RuntimeError::exit(code)),
    None => Ok(()),
  }
}

impl std::fmt::Display for RuntimeError {
  fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    formatter.write_str(&self.message)
  }
}

impl std::error::Error for RuntimeError {}

pub fn run_file(path: &Path) -> Result<(), RuntimeError> {
  run_file_sandboxed(path, None)
}

/// Evaluates `path`. When `sandbox` is set, `file:` fetch may use that
/// directory instead of the entry file's parent. `import.meta.url` stays on
/// `path`.
pub fn run_file_sandboxed(
  path: &Path,
  sandbox: Option<&Path>,
) -> Result<(), RuntimeError> {
  let path = crate::loader::modules::canonicalize(path)
    .unwrap_or_else(|_| path.to_path_buf());
  let sandbox = sandbox.map(|path| {
    crate::loader::modules::canonicalize(path)
      .unwrap_or_else(|_| path.to_path_buf())
  });
  with_runtime(
    false,
    false,
    |_| {},
    |scope| {
      host::set_file_root(&path);
      if let Some(sandbox) = &sandbox {
        host::set_sandbox_dir(sandbox);
      }
      let module = modules::compile_entry(scope, &path)?;
      let module = v8::Local::new(scope, module);
      let value = instantiate_and_evaluate(scope, module)?;
      settle(scope, value)?;
      pump(scope)?;
      report_unhandled()
    },
  )
}

struct WatchSession {
  retain_loop: bool,
}

impl WatchSession {
  fn start(mode: WatchMode) -> Self {
    let retain_loop = mode == WatchMode::Restart;
    if retain_loop {
      set_retain_host_loop(true);
    }
    crate::watch::arm();
    Self { retain_loop }
  }
}

impl Drop for WatchSession {
  fn drop(&mut self) {
    crate::watch::disarm();
    if self.retain_loop {
      set_retain_host_loop(false);
    }
  }
}

/// Runs `path` until `Yuzuko.exit`. A missing file returns status 2.
pub fn run_file_watch(
  path: &Path,
  mode: WatchMode,
  clear_screen: bool,
) -> Result<u8, RuntimeError> {
  let path = crate::loader::modules::canonicalize(path)
    .unwrap_or_else(|_| path.to_path_buf());
  if !path.is_file() {
    eprintln!("cannot open entry {}", path.display());
    return Ok(2);
  }
  let _session = WatchSession::start(mode);
  match mode {
    WatchMode::Restart => run_watch_restart(&path, clear_screen),
    WatchMode::Hot => run_watch_hot(&path, clear_screen),
  }
}

fn run_watch_restart(
  path: &Path,
  clear_screen: bool,
) -> Result<u8, RuntimeError> {
  clear_watch_screen(clear_screen);
  eprintln!("Watcher Process started.");
  loop {
    let outcome = with_runtime(
      false,
      false,
      |_| {},
      |scope| {
        host::set_file_root(path);
        run_entry(scope, path)?;
        pump(scope)?;
        report_unhandled()
      },
    );
    match outcome {
      Ok(()) => {
        eprintln!("Watcher Process finished. Restarting on file change...");
        let changed = wait_for_file_change();
        clear_watch_screen(clear_screen);
        eprintln!(
          "Watcher Restarting! File change detected: {}",
          changed.display()
        );
      }
      Err(error) => {
        if let Some(code) = error.exit_code {
          return Ok(code);
        }
        if let Some(changed) = error.reload_path {
          clear_watch_screen(clear_screen);
          eprintln!(
            "Watcher Restarting! File change detected: {}",
            changed.display()
          );
          continue;
        }
        eprintln!("{error}");
        eprintln!("Watcher Process failed. Restarting on file change...");
        let changed = wait_for_file_change();
        clear_watch_screen(clear_screen);
        eprintln!(
          "Watcher Restarting! File change detected: {}",
          changed.display()
        );
      }
    }
  }
}

fn run_watch_hot(path: &Path, clear_screen: bool) -> Result<u8, RuntimeError> {
  clear_watch_screen(clear_screen);
  eprintln!("HMR Process started.");
  match with_runtime(
    false,
    false,
    |_| {},
    |scope| {
      host::set_file_root(path);
      drive_hot(scope, path, clear_screen)
    },
  ) {
    Ok(()) => Ok(0),
    Err(error) => match error.exit_code {
      Some(code) => Ok(code),
      None => Err(error),
    },
  }
}

fn drive_hot(
  scope: &mut v8::PinScope,
  path: &Path,
  clear_screen: bool,
) -> Result<(), RuntimeError> {
  let mut pending: Option<PathBuf> = None;
  loop {
    let changed = pending.take();
    let outcome = if let Some(changed_path) = changed.as_deref() {
      reload_entry(scope, path, changed_path, clear_screen)
    } else {
      run_entry(scope, path)
    };
    match outcome {
      Err(error) if error.exit_code.is_some() => return Err(error),
      Err(error) if error.reload_path.is_some() => {
        pending = error.reload_path;
        continue;
      }
      Err(error) => {
        eprintln!("{error}");
        let label = changed.as_deref().unwrap_or(path);
        eprintln!("HMR failed to reload {}.", label.display());
        if changed.is_none() {
          quiesce_for_reload(scope);
          modules::purge_generation(host::module_generation());
          // The failed generation's modules are gone, so a window or audio
          // context stays only when `globalThis` still points at it.
          crate::extensions::reload_settled(scope, false);
        }
      }
      Ok(()) => {
        if let Some(changed) = &changed {
          eprintln!("HMR Reloaded {}.", changed.display());
        }
      }
    }
    match pump(scope) {
      Err(error) if error.exit_code.is_some() => return Err(error),
      Err(error) if error.reload_path.is_some() => {
        pending = error.reload_path;
      }
      Err(error) => return Err(error),
      Ok(()) => {
        if let Err(error) = report_unhandled() {
          eprintln!("{error}");
        }
        if !crate::watch::active() {
          return Ok(());
        }
        pending = Some(wait_for_reload(scope)?);
      }
    }
  }
}

fn run_entry(
  scope: &mut v8::PinScope,
  path: &Path,
) -> Result<(), RuntimeError> {
  let module = modules::compile_entry(scope, path)?;
  let module = v8::Local::new(scope, module);
  let value = instantiate_and_evaluate(scope, module)?;
  settle(scope, value)
}

fn reload_entry(
  scope: &mut v8::PinScope,
  entry: &Path,
  changed: &Path,
  clear_screen: bool,
) -> Result<(), RuntimeError> {
  let plan = modules::plan_reload(scope, changed, entry);
  let previous = host::module_generation();
  let next = previous + 1;
  host::set_module_generation(next);
  host::set_reload_fresh(plan.fresh.clone());
  if let Err(error) = modules::compile_fresh(scope, &plan) {
    abandon_reload(next, previous);
    return Err(error);
  }
  // `import.meta` is initialized while the graph links, so the fresh instance
  // map has to be in place before `instantiate`. A failed link puts the
  // previous map back and leaves that program running.
  let saved_instances = host::with(|state| state.wasm_instances.clone());
  host::replace_wasm_instances(scope);
  if let Err(error) = modules::link_fresh(scope, &plan) {
    host::with_mut(|state| state.wasm_instances = saved_instances);
    abandon_reload(next, previous);
    return Err(error);
  }
  modules::merge_stable_wasm(scope, saved_instances.as_ref(), &plan.fresh);
  clear_watch_screen(clear_screen);
  quiesce_for_reload(scope);
  let evaluated = modules::evaluate_fresh(scope, &plan);
  modules::purge_replaced(next, &plan.fresh);
  modules::record_wasm_namespaces(scope);
  // Retain reads which modules were re-evaluated. Clear that set before
  // settling, so a dynamic import during the promise uses the live record.
  crate::extensions::reload_settled(scope, true);
  host::clear_reload_fresh();
  let values = evaluated?;
  for value in values {
    let value = v8::Local::new(scope, &value);
    settle(scope, value)?;
  }
  Ok(())
}

fn abandon_reload(next: u64, previous: u64) {
  modules::purge_generation(next);
  host::set_module_generation(previous);
  host::clear_reload_fresh();
}

fn wait_for_reload(scope: &mut v8::PinScope) -> Result<PathBuf, RuntimeError> {
  loop {
    bail_if_exit()?;
    if let Some(path) = crate::watch::poll() {
      return Ok(path);
    }
    if pump_once(scope) {
      continue;
    }
    bail_if_exit()?;
    if let Some(path) = crate::watch::poll() {
      return Ok(path);
    }
    realm_wait(Some(crate::watch::POLL));
  }
}

fn wait_for_file_change() -> PathBuf {
  loop {
    if let Some(path) = crate::watch::poll() {
      return path;
    }
    std::thread::sleep(crate::watch::POLL);
  }
}

fn clear_watch_screen(enabled: bool) {
  if !enabled || !std::io::stderr().is_terminal() {
    return;
  }
  let mut err = std::io::stderr();
  let _ = write!(err, "\x1B[H\x1B[2J\x1B[3J");
  let _ = err.flush();
}

/// Drops host resources from the previous evaluation without leaving the realm.
///
/// A module reload drops listeners, timers, workers, and GPU devices owned by
/// the modules being evaluated again. A module that stays keeps its window,
/// its audio device, and the host timers that drive playback. The first
/// evaluation's failure has no such set and clears the realm's host resources.
fn quiesce_for_reload(scope: &mut v8::PinScope) {
  if host::reload_targets_modules() {
    crate::worker::terminate_reloaded_workers();
    crate::timers::clear_reloaded();
    crate::extensions::reload();
    crate::extensions::reload_scope(scope);
    return;
  }
  crate::worker::terminate_child_workers();
  crate::timers::clear_all();
  #[cfg(feature = "fs")]
  crate::fs::clear();
  #[cfg(feature = "udp")]
  crate::udp::clear();
  #[cfg(feature = "tcp")]
  crate::tcp::clear();
  #[cfg(feature = "http")]
  crate::http_server::clear();
  #[cfg(feature = "signal")]
  crate::signal::release_listeners();
  #[cfg(feature = "child")]
  crate::child::clear();
  #[cfg(feature = "ffi")]
  crate::ffi::clear();
  crate::jobs::clear();
  host::with_mut(|state| {
    state.unhandled.clear();
    state.deferred_rejections.clear();
    state.deferred_imports.clear();
    state.rejection_notify_queued = false;
  });
  crate::extensions::reload();
  crate::extensions::reload_scope(scope);
}

pub fn run_scripts(sources: &[&str]) -> Result<Vec<String>, RuntimeError> {
  run_script_list(sources, |_| {})
}

/// Result of classic scripts evaluated in a fresh realm.
pub struct ClassicRun {
  /// `done_key` on `globalThis` when it is a string.
  pub value: Option<String>,
  /// A script failed to compile or threw before the event loop finished.
  pub error: Option<String>,
}

/// Evaluates classic scripts in order, then pumps tasks until `done_key` is a
/// string, the loop is idle, or `deadline` passes.
///
/// `base` is the file dynamic `import()` resolves against. Classic scripts
/// have no module URL of their own.
pub fn run_classic_scripts(
  scripts: &[String],
  done_key: &str,
  deadline: std::time::Duration,
  base: Option<&Path>,
  sandbox: Option<&Path>,
) -> Result<ClassicRun, RuntimeError> {
  run_classic_scripts_at(scripts, done_key, deadline, base, sandbox, None)
}

/// Same as [`run_classic_scripts`], but relative URLs use `entry_url` when
/// it is set. `file:` remains confined to `sandbox`.
pub fn run_classic_scripts_at(
  scripts: &[String],
  done_key: &str,
  deadline: std::time::Duration,
  base: Option<&Path>,
  sandbox: Option<&Path>,
  entry_url: Option<&str>,
) -> Result<ClassicRun, RuntimeError> {
  let mut session = ClassicSession::new();
  session.run(scripts, done_key, deadline, base, sandbox, entry_url)
}

/// One isolate that hosts a fresh realm for each classic-script run.
///
/// WPT keeps this for the suite so each testharness file is a new context
/// without creating a new isolate. Function templates and interned scripts
/// stay on the isolate across those realms.
pub struct ClassicSession {
  _lock: MutexGuard<'static, ()>,
  isolate: v8::OwnedIsolate,
  queue: Arc<TaskQueue>,
  in_realm: bool,
  runs: u32,
  templates: HashMap<TypeId, v8::Global<v8::FunctionTemplate>>,
  shared: Vec<SharedScript>,
}

/// Source compiled once on a [`ClassicSession`] and bound in later realms.
#[derive(Clone, Copy, Debug)]
pub struct InternedScript(u32);

/// One classic script in a [`ClassicSession::run_parts`] list.
pub enum ClassicPart<'a> {
  Shared(InternedScript),
  Source(&'a str),
}

struct SharedScript {
  source: String,
  unbound: Option<v8::Global<v8::UnboundScript>>,
}

const SHARED_ORIGIN: &str = "yuzuko:wpt";

impl ClassicSession {
  pub fn new() -> Self {
    let lock = RUNTIME.lock().expect("runtime lock");
    crate::process::clear_exit();
    WASM_IN_FLIGHT.with(|count| count.set(0));
    init_v8();
    let queue = QUEUE.get().expect("task queue").clone();
    queue.discard_v8_tasks();
    let mut isolate = v8::Isolate::new(isolate_params());
    configure_isolate(&mut isolate);
    Self {
      _lock: lock,
      isolate,
      queue,
      in_realm: false,
      runs: 0,
      templates: HashMap::new(),
      shared: Vec::new(),
    }
  }

  /// Compiles `source` on the first run and reuses that script in later realms.
  pub fn intern(&mut self, source: impl Into<String>) -> InternedScript {
    let id = u32::try_from(self.shared.len()).expect("interned scripts");
    self.shared.push(SharedScript {
      source: source.into(),
      unbound: None,
    });
    InternedScript(id)
  }

  pub fn interned_source(&self, script: InternedScript) -> &str {
    &self.shared[script.0 as usize].source
  }

  /// Evaluates classic scripts in a new realm on this isolate.
  pub fn run(
    &mut self,
    scripts: &[String],
    done_key: &str,
    deadline: std::time::Duration,
    base: Option<&Path>,
    sandbox: Option<&Path>,
    entry_url: Option<&str>,
  ) -> Result<ClassicRun, RuntimeError> {
    let parts: Vec<ClassicPart<'_>> = scripts
      .iter()
      .map(|script| ClassicPart::Source(script))
      .collect();
    self.run_parts(&parts, done_key, deadline, base, sandbox, entry_url)
  }

  /// Same as [`Self::run`], with interned scripts bound instead of recompiled.
  pub fn run_parts(
    &mut self,
    parts: &[ClassicPart<'_>],
    done_key: &str,
    deadline: std::time::Duration,
    base: Option<&Path>,
    sandbox: Option<&Path>,
    entry_url: Option<&str>,
  ) -> Result<ClassicRun, RuntimeError> {
    self.enter_realm();
    struct Guard<'a> {
      isolate: &'a mut v8::OwnedIsolate,
      queue: &'a Arc<TaskQueue>,
      in_realm: &'a mut bool,
      runs: &'a mut u32,
      templates: &'a mut HashMap<TypeId, v8::Global<v8::FunctionTemplate>>,
      shared: &'a mut Vec<SharedScript>,
    }
    impl Drop for Guard<'_> {
      fn drop(&mut self) {
        leave_realm(
          self.isolate,
          self.queue,
          self.in_realm,
          self.runs,
          self.templates,
        );
      }
    }
    let guard = Guard {
      isolate: &mut self.isolate,
      queue: &self.queue,
      in_realm: &mut self.in_realm,
      runs: &mut self.runs,
      templates: &mut self.templates,
      shared: &mut self.shared,
    };
    let deadline = std::time::Instant::now() + deadline;
    let base_url = base.and_then(|path| modules::file_url(path).ok());
    run_classic_body(
      guard.isolate,
      guard.shared,
      parts,
      done_key,
      deadline,
      base,
      sandbox,
      entry_url,
      base_url.as_deref(),
    )
  }

  fn enter_realm(&mut self) {
    crate::process::clear_exit();
    WASM_IN_FLIGHT.with(|count| count.set(0));
    self.queue.discard_v8_tasks();
    let mut state =
      host::HostState::empty(Arc::clone(&self.queue), RealmKind::Window, 0);
    state.templates = std::mem::take(&mut self.templates);
    host::enter(state);
    crate::worker::attach_main(Arc::clone(&self.queue));
    crate::worker::set_isolate_handle(
      crate::worker::MAIN_AGENT,
      self.isolate.thread_safe_handle(),
    );
    self.in_realm = true;
  }
}

impl Default for ClassicSession {
  fn default() -> Self {
    Self::new()
  }
}

impl Drop for ClassicSession {
  fn drop(&mut self) {
    if self.in_realm {
      teardown_realm();
      self.in_realm = false;
    }
    self.queue.discard_v8_tasks();
    crate::worker::detach_main();
    host::exit();
  }
}

fn leave_realm(
  isolate: &mut v8::OwnedIsolate,
  queue: &Arc<TaskQueue>,
  in_realm: &mut bool,
  runs: &mut u32,
  templates: &mut HashMap<TypeId, v8::Global<v8::FunctionTemplate>>,
) {
  if !*in_realm {
    return;
  }
  teardown_realm();
  queue.discard_v8_tasks();
  crate::worker::detach_main();
  *templates = host::with_mut(|state| std::mem::take(&mut state.templates));
  host::exit();
  isolate.clear_kept_objects();
  *runs += 1;
  if (*runs).is_multiple_of(32) {
    isolate.low_memory_notification();
  }
  *in_realm = false;
}

#[allow(clippy::too_many_arguments)]
fn run_classic_body(
  isolate: &mut v8::OwnedIsolate,
  shared: &mut [SharedScript],
  parts: &[ClassicPart<'_>],
  done_key: &str,
  deadline: std::time::Instant,
  base: Option<&Path>,
  sandbox: Option<&Path>,
  entry_url: Option<&str>,
  base_url: Option<&str>,
) -> Result<ClassicRun, RuntimeError> {
  v8::scope!(let scope, isolate);
  let context = v8::Context::new(scope, Default::default());
  let scope = &mut v8::ContextScope::new(scope, context);
  install_realm(scope, false);
  if let Some(path) = base {
    host::set_file_root(path);
  }
  if let Some(root) = sandbox {
    host::set_sandbox_dir(root);
  }
  if let Some(url) = entry_url {
    host::set_entry_url(url.to_string());
  }
  for part in parts {
    if let Some(run) = run_classic_part(scope, shared, part, base_url)? {
      return Ok(run);
    }
  }
  pump_until(scope, deadline, done_key)?;
  Ok(ClassicRun {
    value: global_string(scope, done_key),
    error: None,
  })
}

fn run_classic_part(
  scope: &mut v8::PinScope<'_, '_>,
  shared: &mut [SharedScript],
  part: &ClassicPart<'_>,
  base_url: Option<&str>,
) -> Result<Option<ClassicRun>, RuntimeError> {
  match part {
    ClassicPart::Shared(InternedScript(id)) => {
      let slot = &mut shared[*id as usize];
      v8::tc_scope!(let tc, scope);
      if let Some(unbound) = slot.unbound.as_ref() {
        let local = v8::Local::new(tc, unbound);
        let compiled = local.bind_to_current_context(tc);
        if compiled.run(tc).is_none() {
          bail_if_exit()?;
          let text = modules::exception_text!(tc, "failed to run script");
          return Ok(Some(ClassicRun {
            value: None,
            error: Some(text),
          }));
        }
        return Ok(None);
      }
      let Some(code) = v8::String::new(tc, &slot.source) else {
        return Ok(Some(ClassicRun {
          value: None,
          error: Some("script is too large".to_string()),
        }));
      };
      let origin = script_origin(tc, Some(SHARED_ORIGIN));
      let Some(compiled) = v8::Script::compile(tc, code, origin.as_ref())
      else {
        let text = modules::exception_text!(tc, "failed to compile script");
        return Ok(Some(ClassicRun {
          value: None,
          error: Some(text),
        }));
      };
      let unbound = compiled.get_unbound_script(tc);
      slot.unbound = Some(v8::Global::new(tc, unbound));
      if compiled.run(tc).is_none() {
        bail_if_exit()?;
        let text = modules::exception_text!(tc, "failed to run script");
        return Ok(Some(ClassicRun {
          value: None,
          error: Some(text),
        }));
      }
      Ok(None)
    }
    ClassicPart::Source(script) => {
      v8::tc_scope!(let tc, scope);
      let Some(code) = v8::String::new(tc, script) else {
        return Ok(Some(ClassicRun {
          value: None,
          error: Some("script is too large".to_string()),
        }));
      };
      let origin = script_origin(tc, base_url);
      let Some(compiled) = v8::Script::compile(tc, code, origin.as_ref())
      else {
        let text = modules::exception_text!(tc, "failed to compile script");
        return Ok(Some(ClassicRun {
          value: None,
          error: Some(text),
        }));
      };
      if compiled.run(tc).is_none() {
        bail_if_exit()?;
        let text = modules::exception_text!(tc, "failed to run script");
        return Ok(Some(ClassicRun {
          value: None,
          error: Some(text),
        }));
      }
      Ok(None)
    }
  }
}

#[cfg(test)]
pub(crate) fn run_scripts_after(
  prepare: impl FnOnce(&mut v8::PinScope<'_, '_>),
  sources: &[&str],
) -> Result<Vec<String>, RuntimeError> {
  run_script_list(sources, prepare)
}

pub(crate) fn eval_classic(
  scope: &mut v8::PinScope,
  url: &str,
  source: &str,
) -> Result<(), RuntimeError> {
  v8::tc_scope!(let tc, scope);
  let Some(code) = v8::String::new(tc, source) else {
    return Err(RuntimeError::message("script is too large".to_string()));
  };
  let origin = script_origin(tc, Some(url));
  let Some(compiled) = v8::Script::compile(tc, code, origin.as_ref()) else {
    let text = modules::exception_text!(tc, "failed to compile script");
    return Err(RuntimeError::message(text));
  };
  if compiled.run(tc).is_none() {
    bail_if_exit()?;
    let text = modules::exception_text!(tc, "failed to run script");
    return Err(RuntimeError::message(text));
  }
  Ok(())
}

/// Boots a dedicated worker isolate and runs `body` on its thread.
pub(crate) fn run_worker_isolate(
  id: u64,
  queue: Arc<TaskQueue>,
  script: crate::worker::WorkerScript,
) {
  init_v8();
  let mut state =
    host::HostState::empty(Arc::clone(&queue), RealmKind::DedicatedWorker, id);
  state.entry_dir = script.sandbox.clone();
  state.entry_url = Some(script.url.clone());
  host::enter(state);
  let mut isolate = v8::Isolate::new(isolate_params());
  let token = crate::platform::isolate_token(&isolate);
  crate::platform::bind_isolate(token, Arc::clone(&queue));
  let handle = isolate.thread_safe_handle();
  crate::worker::set_isolate_handle(id, handle);
  configure_isolate(&mut isolate);
  {
    v8::scope!(let scope, &mut isolate);
    let context = v8::Context::new(scope, Default::default());
    let scope = &mut v8::ContextScope::new(scope, context);
    install_realm(scope, false);
    let result: Result<(), RuntimeError> = (|| {
      if exiting() || !crate::worker::agent_alive(id) {
        return Ok(());
      }
      let loaded = crate::worker::load_worker_source(
        &script.url,
        script.sandbox.as_deref(),
      )
      .map_err(|error| match error {
        crate::worker::WorkerLoadError::Message(message) => {
          RuntimeError::message(message)
        }
        crate::worker::WorkerLoadError::Denied { name, message } => {
          RuntimeError::dom(name, message)
        }
      })?;
      if script.kind == crate::worker::ScriptKind::Module {
        let path = loaded.path.ok_or_else(|| {
          RuntimeError::message("A module worker needs a file path".to_string())
        })?;
        let module = modules::compile_entry(scope, &path)?;
        let module = v8::Local::new(scope, module);
        let value = instantiate_and_evaluate(scope, module)?;
        let _ = value;
      } else {
        eval_classic(scope, &script.url, &loaded.text)?;
      }
      Ok(())
    })();
    if let Err(error) = result {
      // `Yuzuko.exit` from this worker is process shutdown, not an error
      // event on the parent.
      if !exiting() && crate::worker::agent_alive(id) {
        report_worker_failure(id, &script.url, error);
      }
    } else {
      while crate::worker::agent_alive(id) && !crate::worker::worker_closing(id)
      {
        if exiting() {
          break;
        }
        if !pump_once(scope) {
          if exiting()
            || !crate::worker::agent_alive(id)
            || crate::worker::worker_closing(id)
          {
            break;
          }
          queue.wait(None);
        }
      }
    }
    // `close()` leaves the agent alive through the script so the owner can
    // receive its last `postMessage`. Mark it dead once that script returns.
    crate::worker::mark_dead(id);
    teardown_realm();
  }
  crate::platform::unbind_isolate(token);
  host::exit();
}

fn report_worker_failure(id: u64, url: &str, error: RuntimeError) {
  let Some(parent) = crate::worker::parent_agent(id) else {
    crate::worker::mark_dead(id);
    return;
  };
  let message = match error.dom_name {
    Some(name) => format!("{name}: {}", error.message),
    None => error.message,
  };
  crate::worker::post_inbound(
    parent,
    crate::worker::Inbound::Failed {
      worker: id,
      message,
      filename: url.to_string(),
      line: 0,
      column: 0,
    },
  );
  crate::worker::mark_dead(id);
}

pub(crate) fn script_origin<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  base_url: Option<&str>,
) -> Option<v8::ScriptOrigin<'s>> {
  let url = base_url?;
  let name = v8::String::new(scope, url)?;
  Some(v8::ScriptOrigin::new(
    scope,
    name.into(),
    0,
    0,
    false,
    -1,
    None,
    false,
    false,
    false,
    None,
  ))
}

fn run_script_list(
  sources: &[&str],
  prepare: impl FnOnce(&mut v8::PinScope<'_, '_>),
) -> Result<Vec<String>, RuntimeError> {
  with_runtime(true, false, prepare, |scope| {
    for source in sources {
      {
        v8::tc_scope!(let tc, scope);
        let Some(code) = v8::String::new(tc, source) else {
          return Err(RuntimeError::message("script is too large".to_string()));
        };
        let script = match v8::Script::compile(tc, code, None) {
          Some(script) => script,
          None => {
            let text = modules::exception_text!(tc, "failed to compile script");
            return Err(RuntimeError::message(text));
          }
        };
        if script.run(tc).is_none() {
          bail_if_exit()?;
          let text = modules::exception_text!(tc, "failed to run script");
          return Err(RuntimeError::message(text));
        }
      }
      // The script is one macrotask. Checkpoint microtasks before the next one.
      pump(scope)?;
    }
    let logs = host::with(|state| state.logs.clone());
    Ok(logs)
  })
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn event_loop_waits() -> usize {
  init_v8();
  QUEUE.get().map(|queue| queue.wait_count()).unwrap_or(0)
}

pub fn run_tests(
  paths: &[PathBuf],
  filter: Option<&str>,
) -> Result<i32, RuntimeError> {
  let files = collect_tests(paths);
  if files.is_empty() {
    println!("ok | 0 passed | 0 failed | 0 ignored");
    return Ok(0);
  }
  let mut passed = 0;
  let mut failed = 0;
  let mut ignored = 0;
  let mut only_seen = false;
  for file in files {
    let outcome = run_test_file(&file, filter)?;
    passed += outcome.passed;
    failed += outcome.failed;
    ignored += outcome.ignored;
    only_seen |= outcome.only_seen;
  }
  println!("ok | {passed} passed | {failed} failed | {ignored} ignored");
  if only_seen {
    eprintln!("error: Test failed because the \"only\" option was used");
    return Ok(1);
  }
  Ok(if failed == 0 { 0 } else { 1 })
}

struct TestOutcome {
  passed: u32,
  failed: u32,
  ignored: u32,
  only_seen: bool,
}

fn run_test_file(
  path: &Path,
  filter: Option<&str>,
) -> Result<TestOutcome, RuntimeError> {
  with_runtime(
    false,
    true,
    |_| {},
    |scope| {
      host::set_file_root(path);
      let module = modules::compile_entry(scope, path)?;
      let module = v8::Local::new(scope, module);
      let value = instantiate_and_evaluate(scope, module)?;
      settle(scope, value)?;
      pump(scope)?;
      report_unhandled()?;
      let tests = host::with_mut(|state| std::mem::take(&mut state.tests));
      let only_seen = tests.iter().any(|test| test.only);
      let mut passed = 0;
      let mut failed = 0;
      let mut ignored = 0;
      for test in tests {
        if filter.is_some_and(|needle| !test.name.contains(needle)) {
          continue;
        }
        if test.ignore || (only_seen && !test.only) {
          ignored += 1;
          println!("ignored | {}", test.name);
          continue;
        }
        match execute_test(scope, &test) {
          Ok(()) => {
            passed += 1;
            println!("ok | {}", test.name);
          }
          Err(error) if error.exit_code.is_some() => return Err(error),
          Err(error) => {
            failed += 1;
            println!("failed | {}", test.name);
            eprintln!("{}", error.message);
          }
        }
      }
      Ok(TestOutcome {
        passed,
        failed,
        ignored,
        only_seen,
      })
    },
  )
}

fn execute_test(
  scope: &mut v8::PinScope<'_, '_>,
  test: &RegisteredTest,
) -> Result<(), RuntimeError> {
  host::with_mut(|state| {
    state.in_test = true;
    state.test_error = None;
    state.unhandled.clear();
  });
  let delayed_before = host::queue().pending_delayed();
  let timers_before = crate::timers::count();
  let function = v8::Local::new(scope, &test.function);
  let receiver = scope.get_current_context().global(scope);
  let test_context = v8::Object::new(scope);
  let step = v8::Function::new(scope, step_callback).expect("step function");
  let step_name = v8::String::new(scope, "step").unwrap();
  test_context.set(scope, step_name.into(), step.into());
  let result = {
    v8::tc_scope!(let tc, scope);
    match function.call(tc, receiver.into(), &[test_context.into()]) {
      Some(value) => Ok(v8::Global::new(tc, value)),
      None => {
        bail_if_exit()?;
        let text = modules::exception_text!(tc, "test threw");
        Err(RuntimeError::message(text))
      }
    }
  };
  let outcome = (|| {
    let result = v8::Local::new(scope, result?);
    settle(scope, result)?;
    pump(scope)?;
    if host::queue().pending_delayed() > delayed_before
      || crate::timers::count() > timers_before
    {
      return Err(RuntimeError::message(format!(
        "{} left a timer running",
        test.name
      )));
    }
    if let Some(error) = host::with_mut(|state| state.test_error.take()) {
      return Err(RuntimeError::message(error));
    }
    report_unhandled()?;
    Ok(())
  })();
  host::with_mut(|state| state.in_test = false);
  outcome
}

fn settle(
  scope: &mut v8::PinScope<'_, '_>,
  value: v8::Local<v8::Value>,
) -> Result<(), RuntimeError> {
  let Ok(promise) = v8::Local::<v8::Promise>::try_from(value) else {
    return Ok(());
  };
  let promise = v8::Global::new(scope, promise);
  loop {
    bail_if_exit()?;
    if let Some(path) = crate::watch::poll() {
      return Err(RuntimeError::reload(path));
    }
    let local = v8::Local::new(scope, &promise);
    match local.state() {
      v8::PromiseState::Pending => {
        // The resolving reaction runs inside `pump_once`. That call can
        // then find an idle queue and return false, so read the state again
        // before treating the promise as stuck.
        if !pump_once(scope) {
          bail_if_exit()?;
          if let Some(path) = crate::watch::poll() {
            return Err(RuntimeError::reload(path));
          }
          if promise_is_pending(scope, &promise) {
            if crate::watch::active() {
              realm_wait(Some(crate::watch::POLL));
              continue;
            }
            return Err(RuntimeError::message(
              "the event loop stopped while a promise was still pending"
                .to_string(),
            ));
          }
        }
      }
      v8::PromiseState::Fulfilled => return Ok(()),
      v8::PromiseState::Rejected => {
        bail_if_exit()?;
        return rejected(scope, local);
      }
    }
  }
}

fn promise_is_pending(
  scope: &mut v8::PinScope,
  promise: &v8::Global<v8::Promise>,
) -> bool {
  v8::Local::new(scope, promise).state() == v8::PromiseState::Pending
}

fn rejected(
  scope: &mut v8::PinScope,
  promise: v8::Local<v8::Promise>,
) -> Result<(), RuntimeError> {
  let reason = promise.result(scope);
  let text = modules::exception_stack(scope, reason).unwrap_or_else(|| {
    reason
      .to_string(scope)
      .map(|value| value.to_rust_string_lossy(scope))
      .unwrap_or_else(|| "promise rejected".to_string())
  });
  Err(RuntimeError::message(text))
}

fn global_string(scope: &mut v8::PinScope, key: &str) -> Option<String> {
  let global = scope.get_current_context().global(scope);
  let key = v8::String::new(scope, key)?;
  let value = global.get(scope, key.into())?;
  if !value.is_string() {
    return None;
  }
  value
    .to_string(scope)
    .map(|text| text.to_rust_string_lossy(scope))
}

/// When to keep the event loop asleep, reported by a feature crate.
pub struct LoopInterest {
  /// An open surface (or similar host resource) is still alive.
  pub keep_alive: bool,
  /// Soonest moment that crate wants another turn. `None` waits until a task
  /// is posted.
  pub next_wait: Option<std::time::Duration>,
}

type DriveFn = fn(&mut v8::PinScope) -> bool;
type InterestFn = fn() -> LoopInterest;
type WaitFn = fn(Option<std::time::Duration>);

struct LoopHooks {
  drive: DriveFn,
  interest: InterestFn,
  wait: WaitFn,
}

fn default_drive(_scope: &mut v8::PinScope) -> bool {
  false
}

fn default_interest() -> LoopInterest {
  LoopInterest {
    keep_alive: false,
    next_wait: None,
  }
}

fn default_wait(timeout: Option<std::time::Duration>) {
  host::queue().wait(timeout);
}

fn loop_hooks() -> std::sync::MutexGuard<'static, LoopHooks> {
  use std::sync::Mutex;
  static HOOKS: Mutex<LoopHooks> = Mutex::new(LoopHooks {
    drive: default_drive,
    interest: default_interest,
    wait: default_wait,
  });
  HOOKS.lock().expect("event loop hooks")
}

/// Replaces the process-wide frame driver. Feature crates call this once.
pub fn set_loop_hooks(drive: DriveFn, interest: InterestFn, wait: WaitFn) {
  *loop_hooks() = LoopHooks {
    drive,
    interest,
    wait,
  };
}

pub fn set_task_waker(waker: Option<std::sync::Arc<dyn Fn() + Send + Sync>>) {
  host::queue().set_waker(waker);
}

/// Wakes the REPL editor when it is blocked on stdin.
///
/// [`set_task_waker`] stays the window event loop. A winit wake does not
/// interrupt `poll` on the terminal, so playback installs this beside it.
pub(crate) fn set_io_waker(
  waker: Option<std::sync::Arc<dyn Fn() + Send + Sync>>,
) {
  host::queue().set_io_waker(waker);
}

pub fn wait_for_tasks(timeout: Option<std::time::Duration>) {
  host::queue().wait(timeout);
}

/// Whether a worker [`crate::worker::post_inbound`] poked this agent since the
/// previous poll. Clears the flag.
pub fn poll_task_poke() -> bool {
  host::queue().poll_poke()
}

fn drive_loop(scope: &mut v8::PinScope) -> bool {
  let drive = loop_hooks().drive;
  drive(scope)
}

fn loop_interest() -> LoopInterest {
  let interest = loop_hooks().interest;
  interest()
}

pub(crate) fn loop_wait(timeout: Option<std::time::Duration>) {
  let wait = loop_hooks().wait;
  wait(timeout);
}

fn pump_until(
  scope: &mut v8::PinScope<'_, '_>,
  deadline: std::time::Instant,
  done_key: &str,
) -> Result<(), RuntimeError> {
  loop {
    bail_if_exit()?;
    if global_string(scope, done_key).is_some() {
      return bail_if_exit();
    }
    // A worker reply can land as the deadline expires. Deliver what is
    // already queued before treating the harness as unfinished.
    let expired = std::time::Instant::now() >= deadline;
    scope.perform_microtask_checkpoint();
    bail_if_exit()?;
    if global_string(scope, done_key).is_some() {
      return bail_if_exit();
    }
    if crate::idle::poll_idle_timeout(scope) {
      continue;
    }
    if let Some(timer) = crate::timers::poll_due() {
      crate::timers::fire(scope, timer);
      continue;
    }
    let queue = host::queue();
    if let Some(task) = queue.pop_ready() {
      task.run();
      continue;
    }
    if crate::jobs::poll(scope) {
      continue;
    }
    if poll_signals(scope) {
      scope.perform_microtask_checkpoint();
      continue;
    }
    scope.perform_microtask_checkpoint();
    bail_if_exit()?;
    if global_string(scope, done_key).is_some() {
      return bail_if_exit();
    }
    if crate::worker::drain_inbound(scope) {
      if global_string(scope, done_key).is_some()
        || std::time::Instant::now() >= deadline
      {
        return bail_if_exit();
      }
      continue;
    }
    // A worker can `postMessage` and then become not alive in the gap after
    // the drain above. Deliver that message before the loop goes idle.
    if crate::worker::inbound_ready() {
      continue;
    }
    if expired {
      return bail_if_exit();
    }
    bail_if_exit()?;
    if !host::is_dedicated_worker() && drive_loop(scope) {
      continue;
    }
    if crate::idle::poll_idle_period(scope) {
      continue;
    }
    let interest = loop_interest();
    let workers = crate::worker::keeps_alive(host::agent_id());
    if !crate::timers::has_pending()
      && queue.is_idle()
      && !crate::jobs::pending()
      && !signals_pending()
      && !interest.keep_alive
      && !workers
      && !crate::idle::idle_has_pending()
    {
      // The worker pushes this inbox before it stores `alive = false`.
      // `keeps_alive` already observed that store, so the push is visible.
      // Returning here would drop the owner's `message` event.
      if crate::worker::inbound_ready() {
        continue;
      }
      if wasm_in_flight() == 0 || std::time::Instant::now() >= deadline {
        return bail_if_exit();
      }
      let remaining =
        deadline.saturating_duration_since(std::time::Instant::now());
      bail_if_exit()?;
      loop_wait(Some(remaining));
      continue;
    }
    let Some(wait) = earliest_wait(
      earliest_wait(queue.next_wait(), crate::timers::next_wait()),
      earliest_wait(interest.next_wait, crate::idle::idle_next_wait()),
    ) else {
      let remaining =
        deadline.saturating_duration_since(std::time::Instant::now());
      if remaining.is_zero() {
        return bail_if_exit();
      }
      bail_if_exit()?;
      loop_wait(Some(remaining));
      continue;
    };
    let remaining =
      deadline.saturating_duration_since(std::time::Instant::now());
    if remaining.is_zero() {
      return bail_if_exit();
    }
    bail_if_exit()?;
    loop_wait(Some(wait.min(remaining)));
  }
}

pub(crate) fn pump(
  scope: &mut v8::PinScope<'_, '_>,
) -> Result<(), RuntimeError> {
  loop {
    if let Some(path) = crate::watch::poll() {
      return Err(RuntimeError::reload(path));
    }
    if !pump_once(scope) {
      bail_if_exit()?;
      if let Some(path) = crate::watch::poll() {
        return Err(RuntimeError::reload(path));
      }
      return Ok(());
    }
  }
}

/// One non-blocking turn for the REPL prompt.
///
/// Ready timers, host tasks, jobs, and window frames run. The returned delay
/// is how long the editor may block in the terminal before calling again.
/// `None` means nothing is scheduled, so the editor waits for a key. A host
/// job that has already finished returns zero, and the editor's wait is also
/// interrupted when that job finishes mid-poll.
pub(crate) fn poll_repl(
  scope: &mut v8::PinScope<'_, '_>,
) -> Option<std::time::Duration> {
  if exiting() {
    return Some(std::time::Duration::ZERO);
  }
  scope.perform_microtask_checkpoint();
  if exiting() {
    return Some(std::time::Duration::ZERO);
  }
  let _ = crate::worker::drain_inbound(scope);
  let _ = crate::idle::poll_idle_timeout(scope);
  if let Some(timer) = crate::timers::poll_due() {
    crate::timers::fire(scope, timer);
  }
  if exiting() {
    return Some(std::time::Duration::ZERO);
  }
  if let Some(task) = host::queue().pop_ready() {
    task.run();
  }
  if exiting() {
    return Some(std::time::Duration::ZERO);
  }
  let _ = crate::jobs::poll(scope);
  if exiting() {
    return Some(std::time::Duration::ZERO);
  }
  if poll_signals(scope) {
    return Some(std::time::Duration::ZERO);
  }
  if exiting() {
    return Some(std::time::Duration::ZERO);
  }
  if !host::is_dedicated_worker() {
    let _ = drive_loop(scope);
  }
  if exiting() {
    return Some(std::time::Duration::ZERO);
  }
  let _ = crate::idle::poll_idle_period(scope);
  scope.perform_microtask_checkpoint();
  if exiting() {
    return Some(std::time::Duration::ZERO);
  }
  repl_wait()
}

fn repl_wait() -> Option<std::time::Duration> {
  let queue = host::queue();
  // One playback quantum is one host job. The 16 ms cap is for a live loop
  // that has nothing ready yet, such as a listen the device has not fired.
  // Sleeping while a job is already queued lets the ring play out as silence.
  if queue.job_ready() {
    return Some(std::time::Duration::ZERO);
  }
  let interest = if host::is_dedicated_worker() {
    LoopInterest {
      keep_alive: false,
      next_wait: None,
    }
  } else {
    loop_interest()
  };
  let hint = earliest_wait(
    earliest_wait(queue.next_wait(), crate::timers::next_wait()),
    earliest_wait(interest.next_wait, crate::idle::idle_next_wait()),
  );
  let live = interest.keep_alive
    || !queue.is_idle()
    || crate::jobs::pending()
    || signals_pending()
    || crate::worker::keeps_alive(host::agent_id())
    || crate::idle::idle_has_pending()
    || wasm_in_flight() > 0;
  if live {
    Some(
      hint
        .unwrap_or(std::time::Duration::from_millis(16))
        .min(std::time::Duration::from_millis(16)),
    )
  } else {
    hint
  }
}

pub(crate) fn pump_once(scope: &mut v8::PinScope<'_, '_>) -> bool {
  if exiting() {
    return false;
  }
  if poll_signals(scope) {
    scope.perform_microtask_checkpoint();
    return true;
  }
  if crate::worker::drain_inbound(scope) {
    scope.perform_microtask_checkpoint();
    return true;
  }
  scope.perform_microtask_checkpoint();
  if exiting() {
    return false;
  }
  if crate::idle::poll_idle_timeout(scope) {
    return true;
  }
  if let Some(timer) = crate::timers::poll_due() {
    crate::timers::fire(scope, timer);
    return true;
  }
  let queue = host::queue();
  if let Some(task) = queue.pop_ready() {
    task.run();
    return true;
  }
  if crate::jobs::poll(scope) {
    return true;
  }
  scope.perform_microtask_checkpoint();
  if exiting() {
    return false;
  }
  if !host::is_dedicated_worker() && drive_loop(scope) {
    return true;
  }
  if crate::idle::poll_idle_period(scope) {
    return true;
  }
  if exiting() {
    return false;
  }
  let interest = if host::is_dedicated_worker() {
    LoopInterest {
      keep_alive: false,
      next_wait: None,
    }
  } else {
    loop_interest()
  };
  let workers = crate::worker::keeps_alive(host::agent_id());
  if crate::timers::has_pending()
    || !queue.is_idle()
    || crate::jobs::pending()
    || signals_pending()
    || wasm_in_flight() > 0
    || interest.keep_alive
    || workers
    || crate::idle::idle_has_pending()
  {
    realm_wait(earliest_wait(
      earliest_wait(
        earliest_wait(queue.next_wait(), crate::timers::next_wait()),
        earliest_wait(interest.next_wait, crate::idle::idle_next_wait()),
      ),
      crate::watch::poll_wait(),
    ));
    return true;
  }
  // Same ordering as `pump_until`: a `postMessage` that landed before the
  // worker died is still in this inbox.
  if crate::worker::inbound_ready() {
    return true;
  }
  false
}

#[cfg(feature = "signal")]
fn poll_signals(scope: &mut v8::PinScope) -> bool {
  crate::signal::poll(scope)
}

#[cfg(not(feature = "signal"))]
fn poll_signals(_scope: &mut v8::PinScope) -> bool {
  false
}

#[cfg(feature = "signal")]
fn signals_pending() -> bool {
  crate::signal::pending()
}

#[cfg(not(feature = "signal"))]
fn signals_pending() -> bool {
  false
}

fn realm_wait(timeout: Option<std::time::Duration>) {
  if host::is_dedicated_worker() {
    host::queue().wait(timeout);
  } else {
    loop_wait(timeout);
  }
}

fn earliest_wait(
  left: Option<std::time::Duration>,
  right: Option<std::time::Duration>,
) -> Option<std::time::Duration> {
  match (left, right) {
    (Some(left), Some(right)) => Some(left.min(right)),
    (Some(left), None) => Some(left),
    (None, Some(right)) => Some(right),
    (None, None) => None,
  }
}

fn report_unhandled() -> Result<(), RuntimeError> {
  let reasons =
    host::with(|state| state.unhandled.values().cloned().collect::<Vec<_>>());
  if reasons.is_empty() {
    return Ok(());
  }
  Err(RuntimeError::message(format!(
    "unhandled rejection: {}",
    reasons.join("\n")
  )))
}

#[cfg(feature = "signal")]
struct SignalTestGuard;

#[cfg(feature = "signal")]
impl Drop for SignalTestGuard {
  fn drop(&mut self) {
    crate::signal::set_test_process(false);
  }
}

pub(crate) fn with_runtime<T>(
  capture_log: bool,
  test_mode: bool,
  prepare: impl FnOnce(&mut v8::PinScope<'_, '_>),
  body: impl FnOnce(&mut v8::PinScope<'_, '_>) -> Result<T, RuntimeError>,
) -> Result<T, RuntimeError> {
  let _guard = RUNTIME.lock().expect("runtime lock");
  #[cfg(feature = "signal")]
  crate::signal::set_test_process(test_mode);
  #[cfg(feature = "signal")]
  let _signal_test = SignalTestGuard;
  crate::process::clear_exit();
  WASM_IN_FLIGHT.with(|count| count.set(0));
  init_v8();
  let queue = QUEUE.get().expect("task queue").clone();
  // The platform queue is process-wide. A previous isolate's housekeeping
  // tasks must not run on this one.
  queue.discard_v8_tasks();
  crate::worker::attach_main(Arc::clone(&queue));
  let mut state =
    host::HostState::empty(Arc::clone(&queue), RealmKind::Window, 0);
  state.capture_log = capture_log;
  host::enter(state);
  let mut isolate = v8::Isolate::new(isolate_params());
  crate::worker::set_isolate_handle(
    crate::worker::MAIN_AGENT,
    isolate.thread_safe_handle(),
  );
  configure_isolate(&mut isolate);
  let result = {
    v8::scope!(let scope, &mut isolate);
    let context = v8::Context::new(scope, Default::default());
    let scope = &mut v8::ContextScope::new(scope, context);
    install_realm(scope, test_mode);
    prepare(scope);
    let result = body(scope);
    teardown_realm();
    result
  };
  // Drop leftover V8 tasks while this isolate is still alive.
  queue.discard_v8_tasks();
  crate::worker::detach_main();
  host::exit();
  result
}

fn configure_isolate(isolate: &mut v8::Isolate) {
  isolate.set_capture_stack_trace_for_uncaught_exceptions(true, 32);
  isolate.set_microtasks_policy(v8::MicrotasksPolicy::Explicit);
  isolate.set_promise_reject_callback(on_promise_reject);
  isolate.set_wasm_async_resolve_promise_callback(on_wasm_async_resolve);
  isolate.set_host_initialize_import_meta_object_callback(on_import_meta);
  isolate.set_host_import_module_dynamically_callback(on_dynamic_import);
  isolate.set_host_import_module_with_phase_dynamically_callback(
    on_dynamic_import_phase,
  );
}

fn isolate_params() -> v8::CreateParams {
  let platform = platform_handle();
  // Concurrent marking calls `trace` on a background thread. Host objects
  // borrow `RefCell`s there, which races the mutator and panics with
  // "RefCell already borrowed". Incremental marking stays on this thread.
  // Sweeping finalizers already run here.
  let heap_params = v8::cppgc::HeapCreateParams {
    marking_support: v8::cppgc::MarkingType::Incremental,
    ..v8::cppgc::HeapCreateParams::default()
  };
  let heap = v8::cppgc::Heap::create(platform, heap_params);
  v8::CreateParams::default().cpp_heap(heap)
}

fn platform_handle() -> v8::SharedRef<v8::Platform> {
  init_v8();
  PLATFORM.get().expect("platform").clone()
}

fn init_v8() {
  V8_INIT.call_once(|| {
    // Source-phase `import source` for Wasm and JavaScript.
    // `import defer` is the deferred-evaluation syntax.
    // `transferToImmutable` makes the ArrayBuffer behind a bytes module.
    v8::V8::set_flags_from_string(
      "--js-source-phase-imports --js-defer-import-eval --js-immutable-arraybuffer",
    );
    // rusty_v8 links the full ICU dataset into the binary, so ECMA-402
    // `Intl` resolves locales beyond English without a separate icudtl.dat.
    let queue = TaskQueue::new();
    let _ = QUEUE.set(Arc::clone(&queue));
    let platform =
      v8::new_custom_platform(0, false, false, HostPlatform { queue })
        .make_shared();
    v8::V8::initialize_platform(platform.clone());
    // cppgc must see the platform before V8::initialize, which installs the
    // process-wide page allocator cppgc also claims.
    v8::cppgc::initialize_process(platform.clone());
    v8::V8::initialize();
    let _ = PLATFORM.set(platform);
  });
}

fn install_realm(scope: &mut v8::PinScope<'_, '_>, test_mode: bool) {
  install_console(scope);
  install_wasm_async(scope);
  crate::loader::module_source::install(scope);
  install_namespace_instance(scope);
  crate::timers::install(scope);
  crate::realm::install(scope);
  install_yuzuko_namespace(scope);
  crate::extensions::install(scope);
  #[cfg(feature = "fs")]
  crate::fs::install(scope);
  #[cfg(feature = "udp")]
  crate::udp::install(scope);
  #[cfg(feature = "tcp")]
  crate::tcp::install(scope);
  #[cfg(feature = "http")]
  crate::http_server::install(scope);
  #[cfg(feature = "signal")]
  crate::signal::install(scope);
  #[cfg(feature = "child")]
  crate::child::install(scope);
  #[cfg(feature = "ffi")]
  crate::ffi::install(scope);
  // The window extension replaces `Yuzuko` with an EventTarget. Process
  // members are installed after that replacement. A worker keeps the
  // original object.
  install_process(scope);
  bind_wasm_response(scope);
  if test_mode {
    install_test_api(scope);
  }
}

fn teardown_realm() {
  #[cfg(feature = "fs")]
  crate::fs::clear();
  #[cfg(feature = "udp")]
  crate::udp::clear();
  #[cfg(feature = "tcp")]
  crate::tcp::clear();
  #[cfg(feature = "http")]
  crate::http_server::clear();
  #[cfg(feature = "signal")]
  crate::signal::clear();
  #[cfg(feature = "child")]
  crate::child::clear();
  #[cfg(feature = "ffi")]
  crate::ffi::clear();
  crate::jobs::clear();
  crate::extensions::cleanup();
}

fn install_wasm_async(scope: &mut v8::PinScope<'_, '_>) {
  let global = crate::js::global(scope);
  let enter = v8::Function::new(scope, wasm_enter).expect("wasm enter");
  let leave = v8::Function::new(scope, wasm_leave).expect("wasm leave");
  let enter_key = v8::String::new(scope, "__yuzukoWasmEnter").unwrap();
  let leave_key = v8::String::new(scope, "__yuzukoWasmLeave").unwrap();
  global.set(scope, enter_key.into(), enter.into());
  global.set(scope, leave_key.into(), leave.into());
  // Intrinsics are closed over while the realm is still pristine. Later
  // assignments to Promise, Function.prototype, or Response.prototype must
  // not change compile, instantiate, or the streaming entry points.
  let source = v8::String::new(scope, WASM_ASYNC).unwrap();
  let script = v8::Script::compile(scope, source, None).expect("wasm async");
  if script.run(scope).is_none() {
    // Termination during worker startup must not abort the thread.
    if !host::is_dedicated_worker() {
      panic!("wasm async");
    }
  }
}

fn bind_wasm_response(scope: &mut v8::PinScope<'_, '_>) {
  let global = crate::js::global(scope);
  let key = v8::String::new(scope, "__yuzukoBindWasmResponse").unwrap();
  let Some(value) = global.get(scope, key.into()) else {
    return;
  };
  let Ok(function) = v8::Local::<v8::Function>::try_from(value) else {
    let _ = global.delete(scope, key.into());
    return;
  };
  // Response is installed by an extension, after the wrappers above.
  let _ = function.call(scope, global.into(), &[]);
  let _ = global.delete(scope, key.into());
}

const WASM_ASYNC: &str = r#"
  const reflectApply = Reflect.apply;
  const promiseThen = Promise.prototype.then;
  const promiseResolve = Promise.resolve;
  const PromiseCtor = Promise;
  const TypeErrorCtor = TypeError;
  const StringCtor = String;
  const trim = String.prototype.trim;
  const toLowerCase = String.prototype.toLowerCase;
  const defineProperty = Object.defineProperty;
  const wasmEnter = globalThis.__yuzukoWasmEnter;
  const wasmLeave = globalThis.__yuzukoWasmLeave;
  delete globalThis.__yuzukoWasmEnter;
  delete globalThis.__yuzukoWasmLeave;

  let responseArrayBuffer = null;
  let responseOk = null;
  let responseHeaders = null;
  let headersGet = null;

  function settle(promise) {
    return reflectApply(promiseThen, promise, [function(value) {
      wasmLeave();
      return value;
    }, function(error) {
      wasmLeave();
      throw error;
    }]);
  }

  function tracked(original, name) {
    function wrapped() {
      wasmEnter();
      try {
        const promise = reflectApply(original, this, arguments);
        return settle(promise);
      } catch (error) {
        wasmLeave();
        throw error;
      }
    }
    defineProperty(wrapped, "name", { value: name });
    defineProperty(wrapped, "length", { value: original.length });
    return wrapped;
  }

  function wasmResponseBytes(response) {
    if (
      response == null ||
      responseArrayBuffer == null ||
      responseOk == null ||
      responseHeaders == null ||
      headersGet == null
    ) {
      throw new TypeErrorCtor("WebAssembly streaming requires a Response");
    }
    if (reflectApply(responseOk, response, []) === false) {
      throw new TypeErrorCtor("The response is not ok");
    }
    const headers = reflectApply(responseHeaders, response, []);
    const mime = headers == null
      ? null
      : reflectApply(headersGet, headers, ["content-type"]);
    const mimeText = mime == null ? "" : StringCtor(mime);
    const normalized = reflectApply(
      toLowerCase,
      reflectApply(trim, mimeText, []),
      [],
    );
    if (normalized !== "application/wasm") {
      throw new TypeErrorCtor("The content type is not application/wasm");
    }
    return reflectApply(responseArrayBuffer, response, []);
  }

  const originalCompile = WebAssembly.compile;
  const originalInstantiate = WebAssembly.instantiate;
  WebAssembly.compileStreaming = function compileStreaming(source) {
    wasmEnter();
    try {
      const promise = reflectApply(promiseThen, reflectApply(
        promiseThen,
        reflectApply(promiseResolve, PromiseCtor, [source]),
        [wasmResponseBytes],
      ), [originalCompile]);
      return settle(promise);
    } catch (error) {
      wasmLeave();
      throw error;
    }
  };
  WebAssembly.instantiateStreaming = function instantiateStreaming(source) {
    const importObject = arguments[1];
    wasmEnter();
    try {
      const promise = reflectApply(promiseThen, reflectApply(
        promiseThen,
        reflectApply(promiseResolve, PromiseCtor, [source]),
        [wasmResponseBytes],
      ), [function(bytes) {
        return originalInstantiate(bytes, importObject);
      }]);
      return settle(promise);
    } catch (error) {
      wasmLeave();
      throw error;
    }
  };
  WebAssembly.compile = tracked(originalCompile, "compile");
  WebAssembly.instantiate = tracked(originalInstantiate, "instantiate");

  globalThis.__yuzukoBindWasmResponse = function() {
    delete globalThis.__yuzukoBindWasmResponse;
    if (typeof Response !== "function" || typeof Headers !== "function") {
      return;
    }
    const proto = Response.prototype;
    const describe = Object.getOwnPropertyDescriptor;
    responseArrayBuffer = proto.arrayBuffer;
    const ok = describe(proto, "ok");
    const headers = describe(proto, "headers");
    responseOk = ok && ok.get;
    responseHeaders = headers && headers.get;
    headersGet = Headers.prototype.get;
  };
"#;

fn install_namespace_instance(scope: &mut v8::PinScope<'_, '_>) {
  let global = crate::js::global(scope);
  let key = v8::String::new(scope, "WebAssembly").unwrap();
  let Some(value) = global.get(scope, key.into()) else {
    return;
  };
  let Ok(webassembly) = v8::Local::<v8::Object>::try_from(value) else {
    return;
  };
  let function = v8::Function::new(scope, namespace_instance)
    .expect("WebAssembly.namespaceInstance");
  let name = v8::String::new(scope, "namespaceInstance").unwrap();
  function.set_name(name);
  webassembly.set(scope, name.into(), function.into());
}

fn namespace_instance(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut ret: v8::ReturnValue,
) {
  let value = args.get(0);
  if let Some(found) = wasm_namespace_instance(scope, value) {
    ret.set(found);
    return;
  }
  // A deferred Wasm module evaluates after the import that first linked it.
  // Index namespaces that have stored an instance since the last pass.
  modules::record_wasm_namespaces(scope);
  if let Some(found) = wasm_namespace_instance(scope, value) {
    ret.set(found);
    return;
  }
  crate::js::throw_type(
    scope,
    "WebAssembly.namespaceInstance requires a WebAssembly module namespace",
  );
}

fn wasm_namespace_instance<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  value: v8::Local<'_, v8::Value>,
) -> Option<v8::Local<'s, v8::Value>> {
  if !value.is_object() {
    return None;
  }
  let map = host::wasm_instances(scope);
  let found = map.get(scope, value)?;
  if found.is_null_or_undefined() {
    None
  } else {
    Some(found)
  }
}

/// The `Yuzuko` object exists on the main realm and in a dedicated worker
/// even when no Cargo feature is enabled. Feature crates attach members
/// afterwards. An AudioWorklet processor context is created separately and
/// does not receive this object.
///
/// Process members are not installed here. On the main realm the window
/// extension replaces this object with an `EventTarget`, so
/// [`install_process`] runs after extensions.
fn install_yuzuko_namespace(scope: &mut v8::PinScope<'_, '_>) {
  let _yuzuko = crate::js::namespace(scope, "Yuzuko");
}

fn install_process(scope: &mut v8::PinScope<'_, '_>) {
  let yuzuko = crate::js::namespace(scope, "Yuzuko");
  crate::process::install(scope, yuzuko);
  crate::permissions::install(scope, yuzuko);
}

fn install_console(scope: &mut v8::PinScope<'_, '_>) {
  CONSOLE.with(|slot| {
    let mut state = slot.borrow_mut();
    state.times.clear();
    state.counts.clear();
    state.depth = 0;
  });
  // A Web IDL namespace object has an empty intermediate prototype.
  let prototype = v8::Object::new(scope);
  let console = v8::Object::new(scope);
  let _ = console.set_prototype(scope, prototype.into());
  crate::js::set_function(scope, console, "log", console_log);
  crate::js::set_function(scope, console, "info", console_log);
  crate::js::set_function(scope, console, "debug", console_log);
  crate::js::set_function(scope, console, "warn", console_warn);
  crate::js::set_function(scope, console, "error", console_warn);
  crate::js::set_function(scope, console, "trace", console_trace);
  crate::js::set_function(scope, console, "assert", console_assert);
  crate::js::set_function(scope, console, "time", console_time);
  crate::js::set_function(scope, console, "timeLog", console_time_log);
  crate::js::set_function(scope, console, "timeEnd", console_time_end);
  crate::js::set_function(scope, console, "count", console_count);
  crate::js::set_function(scope, console, "countReset", console_count_reset);
  crate::js::set_function(scope, console, "group", console_group);
  crate::js::set_function(scope, console, "groupCollapsed", console_group);
  crate::js::set_function(scope, console, "groupEnd", console_group_end);
  crate::js::set_function(scope, console, "dir", console_log);
  crate::js::set_function(scope, console, "dirxml", console_log);
  crate::js::set_function(scope, console, "table", console_log);
  crate::js::set_function(scope, console, "clear", console_clear);
  let tag = v8::String::new(scope, "console").unwrap();
  let symbol = v8::Symbol::get_to_string_tag(scope);
  let _ = console.define_own_property(
    scope,
    symbol.into(),
    tag.into(),
    v8::PropertyAttribute::READ_ONLY | v8::PropertyAttribute::DONT_ENUM,
  );
  let global = crate::js::global(scope);
  let key = v8::String::new(scope, "console").unwrap();
  let _ = global.define_own_property(
    scope,
    key.into(),
    console.into(),
    v8::PropertyAttribute::DONT_ENUM,
  );
}

fn console_clear(
  _scope: &mut v8::PinScope,
  _args: v8::FunctionCallbackArguments,
  mut _return: v8::ReturnValue,
) {
  // There is no console surface. The call still succeeds.
}

fn console_log(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut _return: v8::ReturnValue,
) {
  let _ = console_emit(scope, &args, 0, false);
}

fn console_warn(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut _return: v8::ReturnValue,
) {
  let _ = console_emit(scope, &args, 0, true);
}

fn console_trace(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut _return: v8::ReturnValue,
) {
  let Ok(detail) = console_format(scope, &args, 0) else {
    return;
  };
  let mut line = if detail.is_empty() {
    "Trace".to_string()
  } else {
    format!("Trace: {detail}")
  };
  if let Some(trace) = v8::StackTrace::current_stack_trace(scope, 16) {
    for index in 0..trace.get_frame_count() {
      let Some(frame) = trace.get_frame(scope, index) else {
        continue;
      };
      let name = frame
        .get_function_name(scope)
        .map(|text| text.to_rust_string_lossy(scope))
        .filter(|text| !text.is_empty())
        .unwrap_or_else(|| "<anonymous>".to_string());
      let script = frame
        .get_script_name(scope)
        .map(|text| text.to_rust_string_lossy(scope))
        .unwrap_or_else(|| "<anonymous>".to_string());
      line.push_str(&format!(
        "\n    at {name} ({script}:{}:{})",
        frame.get_line_number(),
        frame.get_column()
      ));
    }
  }
  emit(&line, true);
}

fn console_assert(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut _return: v8::ReturnValue,
) {
  if args.length() > 0 && args.get(0).boolean_value(scope) {
    return;
  }
  let detail = if args.length() <= 1 {
    String::new()
  } else {
    let Ok(detail) = console_format(scope, &args, 1) else {
      return;
    };
    detail
  };
  let line = if detail.is_empty() {
    "Assertion failed".to_string()
  } else {
    format!("Assertion failed: {detail}")
  };
  emit(&line, true);
}

fn console_label(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
) -> Result<String, ()> {
  if args.length() == 0 || args.get(0).is_undefined() {
    return Ok("default".to_string());
  }
  stringify_thrown(scope, args.get(0))
}

/// `ToString`, rethrowing a user `toString` exception.
fn stringify_thrown(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<String, ()> {
  let converted = {
    v8::tc_scope!(let tc, scope);
    match value.to_string(tc) {
      Some(text) => Ok(text.to_rust_string_lossy(tc)),
      None => Err(tc.exception().map(|thrown| v8::Global::new(tc, thrown))),
    }
  };
  match converted {
    Ok(text) => Ok(text),
    Err(Some(thrown)) => {
      let thrown = v8::Local::new(scope, &thrown);
      scope.throw_exception(thrown);
      Err(())
    }
    Err(None) => Err(()),
  }
}

fn console_time(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut _return: v8::ReturnValue,
) {
  let Ok(label) = console_label(scope, &args) else {
    return;
  };
  let duplicate = CONSOLE.with(|slot| {
    let mut state = slot.borrow_mut();
    if state.times.contains_key(&label) {
      return true;
    }
    state
      .times
      .insert(label.clone(), crate::performance_now_ms());
    false
  });
  if duplicate {
    emit(&format!("Timer '{label}' already exists"), true);
  }
}

fn console_time_log(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut _return: v8::ReturnValue,
) {
  let Ok(label) = console_label(scope, &args) else {
    return;
  };
  let started = CONSOLE.with(|slot| slot.borrow().times.get(&label).copied());
  let Some(started) = started else {
    emit(&format!("Timer '{label}' does not exist"), true);
    return;
  };
  let elapsed = crate::performance_now_ms() - started;
  let mut line = format!("{label}: {elapsed:.3} ms");
  if args.length() > 1 {
    let Ok(extra) = console_format(scope, &args, 1) else {
      return;
    };
    line.push(' ');
    line.push_str(&extra);
  }
  emit(&line, false);
}

fn console_time_end(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut _return: v8::ReturnValue,
) {
  let Ok(label) = console_label(scope, &args) else {
    return;
  };
  let started = CONSOLE.with(|slot| slot.borrow_mut().times.remove(&label));
  let Some(started) = started else {
    emit(&format!("Timer '{label}' does not exist"), true);
    return;
  };
  let elapsed = crate::performance_now_ms() - started;
  emit(&format!("{label}: {elapsed:.3} ms"), false);
}

fn console_count(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut _return: v8::ReturnValue,
) {
  let Ok(label) = console_label(scope, &args) else {
    return;
  };
  let count = CONSOLE.with(|slot| {
    let mut state = slot.borrow_mut();
    let count = state.counts.entry(label.clone()).or_insert(0);
    *count += 1;
    *count
  });
  emit(&format!("{label}: {count}"), false);
}

fn console_count_reset(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut _return: v8::ReturnValue,
) {
  let Ok(label) = console_label(scope, &args) else {
    return;
  };
  let missing = CONSOLE.with(|slot| {
    let mut state = slot.borrow_mut();
    match state.counts.get_mut(&label) {
      Some(count) => {
        *count = 0;
        false
      }
      None => true,
    }
  });
  if missing {
    emit(&format!("Count for '{label}' does not exist"), true);
  }
}

fn console_group(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut _return: v8::ReturnValue,
) {
  let label = if args.length() == 0 {
    "console.group".to_string()
  } else {
    let Ok(label) = console_format(scope, &args, 0) else {
      return;
    };
    label
  };
  emit(&label, false);
  CONSOLE.with(|slot| {
    let mut state = slot.borrow_mut();
    state.depth = state.depth.saturating_add(1);
  });
}

fn console_group_end(
  _scope: &mut v8::PinScope,
  _args: v8::FunctionCallbackArguments,
  mut _return: v8::ReturnValue,
) {
  CONSOLE.with(|slot| {
    let mut state = slot.borrow_mut();
    state.depth = state.depth.saturating_sub(1);
  });
}

fn emit(line: &str, stderr: bool) {
  let depth = CONSOLE.with(|slot| slot.borrow().depth);
  let line = if depth == 0 {
    line.to_string()
  } else {
    let prefix = "  ".repeat(depth as usize);
    line
      .lines()
      .map(|part| format!("{prefix}{part}"))
      .collect::<Vec<_>>()
      .join("\n")
  };
  let capture = host::with(|state| state.capture_log);
  if capture && !stderr {
    host::with_mut(|state| state.logs.push(line.to_string()));
    return;
  }
  if EDITOR_LINE.with(|flag| flag.get())
    && !EDITOR_BROKE.with(|flag| flag.get())
  {
    println!();
    EDITOR_BROKE.with(|flag| flag.set(true));
  }
  if stderr {
    eprintln!("{line}");
  } else {
    println!("{line}");
  }
}

fn console_emit(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
  start: i32,
  stderr: bool,
) -> Result<(), ()> {
  let line = console_format(scope, args, start)?;
  emit(&line, stderr);
  Ok(())
}

fn console_format(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
  start: i32,
) -> Result<String, ()> {
  match crate::inspect::format_callback_args(scope, args, start) {
    Ok(line) => Ok(line),
    Err(thrown) => {
      crate::inspect::rethrow(scope, thrown);
      Err(())
    }
  }
}

fn install_test_api(scope: &mut v8::PinScope<'_, '_>) {
  let yuzuko = crate::js::namespace(scope, "Yuzuko");
  let test = v8::Function::new(scope, register_test).expect("Yuzuko.test");
  let ignore = v8::Function::new(scope, register_ignored_test)
    .expect("Yuzuko.test.ignore");
  let only =
    v8::Function::new(scope, register_only_test).expect("Yuzuko.test.only");
  let ignore_name = v8::String::new(scope, "ignore").unwrap();
  let only_name = v8::String::new(scope, "only").unwrap();
  test.set(scope, ignore_name.into(), ignore.into());
  test.set(scope, only_name.into(), only.into());
  let test_name = v8::String::new(scope, "test").unwrap();
  yuzuko.set(scope, test_name.into(), test.into());

  let build = v8::Object::new(scope);
  let os = v8::String::new(scope, host_os()).unwrap();
  let os_name = v8::String::new(scope, "os").unwrap();
  build.set(scope, os_name.into(), os.into());
  let build_name = v8::String::new(scope, "build").unwrap();
  yuzuko.set(scope, build_name.into(), build.into());
}

fn host_os() -> &'static str {
  if cfg!(target_os = "macos") {
    "darwin"
  } else if cfg!(target_os = "windows") {
    "windows"
  } else if cfg!(target_os = "linux") {
    "linux"
  } else {
    "unknown"
  }
}

fn register_test(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut _return: v8::ReturnValue,
) {
  register(scope, &args, false, false);
}

fn register_ignored_test(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut _return: v8::ReturnValue,
) {
  register(scope, &args, true, false);
}

fn register_only_test(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut _return: v8::ReturnValue,
) {
  register(scope, &args, false, true);
}

fn register(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
  ignore: bool,
  only: bool,
) {
  if host::with(|state| state.in_test) {
    let message =
      v8::String::new(scope, "nested Yuzuko.test is not supported").unwrap();
    let error = v8::Exception::type_error(scope, message);
    scope.throw_exception(error);
    return;
  }
  let parsed = parse_registration(scope, args, ignore, only);
  match parsed {
    Ok(test) => host::with_mut(|state| {
      state.tests.push(test);
    }),
    Err(message) => {
      let text = v8::String::new(scope, &message).unwrap();
      let error = v8::Exception::type_error(scope, text);
      scope.throw_exception(error);
    }
  }
}

fn parse_registration(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
  ignore: bool,
  only: bool,
) -> Result<RegisteredTest, String> {
  if args.length() == 1 {
    let value = args.get(0);
    if let Ok(function) = v8::Local::<v8::Function>::try_from(value) {
      let name = function_name(scope, function)?;
      return Ok(make_test(scope, name, function, ignore, only));
    }
    if let Ok(object) = v8::Local::<v8::Object>::try_from(value) {
      return registration_from_object(scope, object, ignore, only);
    }
  }
  if args.length() == 2 {
    let name = args.get(0).to_rust_string_lossy(scope);
    let function = v8::Local::<v8::Function>::try_from(args.get(1))
      .map_err(|_| "Yuzuko.test expected a function".to_string())?;
    return Ok(make_test(scope, name, function, ignore, only));
  }
  if args.length() == 3 {
    let name = args.get(0).to_rust_string_lossy(scope);
    let options = v8::Local::<v8::Object>::try_from(args.get(1))
      .map_err(|_| "Yuzuko.test expected options".to_string())?;
    let function = v8::Local::<v8::Function>::try_from(args.get(2))
      .map_err(|_| "Yuzuko.test expected a function".to_string())?;
    let (ignore, only) = flags_from_object(scope, options, ignore, only);
    return Ok(make_test(scope, name, function, ignore, only));
  }
  Err(
    "Yuzuko.test expected a name and function, or an options object"
      .to_string(),
  )
}

fn registration_from_object(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  ignore: bool,
  only: bool,
) -> Result<RegisteredTest, String> {
  let name_key = v8::String::new(scope, "name").unwrap();
  let fn_key = v8::String::new(scope, "fn").unwrap();
  let name = object
    .get(scope, name_key.into())
    .ok_or("Yuzuko.test options need a name")?
    .to_rust_string_lossy(scope);
  let function = object
    .get(scope, fn_key.into())
    .ok_or("Yuzuko.test options need a fn")?;
  let function = v8::Local::<v8::Function>::try_from(function)
    .map_err(|_| "Yuzuko.test fn must be a function".to_string())?;
  let (ignore, only) = flags_from_object(scope, object, ignore, only);
  Ok(make_test(scope, name, function, ignore, only))
}

fn flags_from_object(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  ignore: bool,
  only: bool,
) -> (bool, bool) {
  let ignore_key = v8::String::new(scope, "ignore").unwrap();
  let only_key = v8::String::new(scope, "only").unwrap();
  let ignore = object
    .get(scope, ignore_key.into())
    .is_some_and(|value| value.is_true())
    || ignore;
  let only = object
    .get(scope, only_key.into())
    .is_some_and(|value| value.is_true())
    || only;
  (ignore, only)
}

fn function_name(
  scope: &mut v8::PinScope,
  function: v8::Local<v8::Function>,
) -> Result<String, String> {
  let name = function.get_name(scope).to_rust_string_lossy(scope);
  if name.is_empty() {
    Err(
      "a test function passed without a name must be a named function"
        .to_string(),
    )
  } else {
    Ok(name)
  }
}

fn make_test(
  scope: &mut v8::PinScope,
  name: String,
  function: v8::Local<v8::Function>,
  ignore: bool,
  only: bool,
) -> RegisteredTest {
  RegisteredTest {
    name,
    function: v8::Global::new(scope, function),
    ignore,
    only,
  }
}

fn step_callback(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut _return: v8::ReturnValue,
) {
  if args.length() < 2 {
    let message =
      v8::String::new(scope, "t.step expected a name and function").unwrap();
    let error = v8::Exception::type_error(scope, message);
    scope.throw_exception(error);
    return;
  }
  let name = args.get(0).to_rust_string_lossy(scope);
  let Ok(function) = v8::Local::<v8::Function>::try_from(args.get(1)) else {
    let message = v8::String::new(scope, "t.step expected a function").unwrap();
    let error = v8::Exception::type_error(scope, message);
    scope.throw_exception(error);
    return;
  };
  let global = scope.get_current_context().global(scope);
  {
    v8::tc_scope!(let tc, scope);
    if function.call(tc, global.into(), &[]).is_none() {
      let message =
        modules::exception_text!(tc, &format!("step {name} failed"));
      host::with_mut(|state| state.test_error = Some(message));
    }
  }
}

unsafe extern "C" fn on_wasm_async_resolve(
  _isolate: v8::UnsafeRawIsolatePtr,
  context: v8::Local<v8::Context>,
  resolver: v8::Local<v8::PromiseResolver>,
  result: v8::Local<v8::Value>,
  success: v8::WasmAsyncSuccess,
) {
  v8::callback_scope!(unsafe scope, context);
  let settled = if success == v8::WasmAsyncSuccess::Success {
    resolver.resolve(scope, result)
  } else {
    resolver.reject(scope, result)
  };
  let _ = settled;
}

unsafe extern "C" fn on_promise_reject(message: v8::PromiseRejectMessage) {
  v8::callback_scope!(unsafe scope, &message);
  let event = message.get_event();
  let promise = message.get_promise();
  let hash = promise.get_identity_hash().get();
  let reason_value = message.get_value();
  match event {
    v8::PromiseRejectEvent::PromiseRejectWithNoHandler => {
      let promise = v8::Global::new(scope, promise);
      let reason = reason_value.map(|value| v8::Global::new(scope, value));
      host::with_mut(|state| {
        state
          .deferred_rejections
          .insert(hash, host::DeferredRejection { promise, reason });
      });
      // HTML queues the notification as a task so a handler attached in the
      // same turn still prevents `unhandledrejection`.
      crate::timers::schedule_rejection_flush();
    }
    v8::PromiseRejectEvent::PromiseHandlerAddedAfterReject => {
      let still_deferred = host::with_mut(|state| {
        state.deferred_rejections.remove(&hash).is_some()
      });
      if still_deferred {
        return;
      }
      let notified = host::with(|state| state.unhandled.contains_key(&hash));
      if !notified {
        return;
      }
      dispatch_rejection(scope, event, promise, reason_value);
      host::with_mut(|state| {
        state.unhandled.remove(&hash);
      });
    }
    _ => {}
  }
}

pub(crate) fn flush_rejection_notifications(scope: &mut v8::PinScope) {
  let deferred = host::with_mut(|state| {
    state.rejection_notify_queued = false;
    std::mem::take(&mut state.deferred_rejections)
  });
  for (hash, rejection) in deferred {
    let promise = v8::Local::new(scope, &rejection.promise);
    let reason = rejection
      .reason
      .as_ref()
      .map(|value| v8::Local::new(scope, value));
    let text = reason
      .map(|value| {
        modules::exception_stack(scope, value).unwrap_or_else(|| {
          value
            .to_string(scope)
            .map(|text| text.to_rust_string_lossy(scope))
            .unwrap_or_else(|| "undefined".to_string())
        })
      })
      .unwrap_or_else(|| "undefined".to_string());
    host::with_mut(|state| {
      state.unhandled.insert(hash, text);
    });
    let prevented = dispatch_rejection(
      scope,
      v8::PromiseRejectEvent::PromiseRejectWithNoHandler,
      promise,
      reason,
    );
    if prevented {
      host::with_mut(|state| {
        state.unhandled.remove(&hash);
      });
    }
  }
}

fn dispatch_rejection(
  scope: &mut v8::PinScope,
  event: v8::PromiseRejectEvent,
  promise: v8::Local<v8::Promise>,
  reason: Option<v8::Local<v8::Value>>,
) -> bool {
  let type_name = match event {
    v8::PromiseRejectEvent::PromiseRejectWithNoHandler => "unhandledrejection",
    v8::PromiseRejectEvent::PromiseHandlerAddedAfterReject => {
      "rejectionhandled"
    }
    _ => return false,
  };
  let hook = host::with(|state| state.rejection_hook.clone());
  let Some(hook) = hook else {
    return false;
  };
  v8::tc_scope!(let tc, scope);
  let hook = v8::Local::new(tc, hook);
  let global = crate::js::global(tc);
  let name = v8::String::new(tc, type_name).unwrap();
  let reason = reason.unwrap_or_else(|| v8::undefined(tc).into());
  hook
    .call(tc, global.into(), &[name.into(), promise.into(), reason])
    .is_some_and(|value| value.is_true())
}

unsafe extern "C" fn on_import_meta(
  context: v8::Local<v8::Context>,
  module: v8::Local<v8::Module>,
  meta: v8::Local<v8::Object>,
) {
  v8::callback_scope!(unsafe scope, context);
  let scope = &mut v8::ContextScope::new(scope, context);
  let url =
    host::module_url(module.get_identity_hash().get()).unwrap_or_default();
  let key = v8::String::new(scope, "url").unwrap();
  let value = v8::String::new(scope, &url).unwrap();
  meta.create_data_property(scope, key.into(), value.into());
  let instances = host::wasm_instances(scope);
  let key = v8::String::new(scope, "wasmInstances").unwrap();
  meta.create_data_property(scope, key.into(), instances.into());
}

fn on_dynamic_import<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  _host_defined_options: v8::Local<'s, v8::Data>,
  resource_name: v8::Local<'s, v8::Value>,
  specifier: v8::Local<'s, v8::String>,
  import_attributes: v8::Local<'s, v8::FixedArray>,
) -> Option<v8::Local<'s, v8::Promise>> {
  dynamic_import(scope, resource_name, specifier, import_attributes)
}

fn on_dynamic_import_phase<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  _host_defined_options: v8::Local<'s, v8::Data>,
  resource_name: v8::Local<'s, v8::Value>,
  specifier: v8::Local<'s, v8::String>,
  phase: v8::ModuleImportPhase,
  import_attributes: v8::Local<'s, v8::FixedArray>,
) -> Option<v8::Local<'s, v8::Promise>> {
  if phase == v8::ModuleImportPhase::kSource {
    modules::dynamic_import_source(
      scope,
      resource_name,
      specifier,
      import_attributes,
    )
  } else if phase == v8::ModuleImportPhase::kDefer {
    modules::dynamic_import_defer(
      scope,
      resource_name,
      specifier,
      import_attributes,
    )
  } else {
    dynamic_import(scope, resource_name, specifier, import_attributes)
  }
}

fn collect_tests(paths: &[PathBuf]) -> Vec<PathBuf> {
  let mut files = Vec::new();
  if paths.is_empty() {
    collect_dir(Path::new("."), &mut files);
  } else {
    for path in paths {
      if path.is_dir() {
        collect_dir(path, &mut files);
      } else {
        files.push(path.clone());
      }
    }
  }
  files.sort();
  files
}

fn collect_dir(dir: &Path, files: &mut Vec<PathBuf>) {
  let entries = match std::fs::read_dir(dir) {
    Ok(entries) => entries,
    Err(_) => return,
  };
  for entry in entries.flatten() {
    let path = entry.path();
    let name = entry.file_name();
    let name = name.to_string_lossy();
    if path.is_dir() {
      if matches!(name.as_ref(), "target" | ".git" | "node_modules") {
        continue;
      }
      collect_dir(&path, files);
      continue;
    }
    if is_test_file(&name) {
      files.push(path);
    }
  }
}

fn is_test_file(name: &str) -> bool {
  name.ends_with("_test.js")
    || name.ends_with(".test.js")
    || name.ends_with("_test.ts")
    || name.ends_with(".test.ts")
}
