//! Process entry points for `yuzuko run` and `yuzuko test`.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, Once, OnceLock};

use crate::host::{self, RealmKind, RegisteredTest};
use crate::loader::modules::{self, dynamic_import, instantiate_and_evaluate};
use crate::platform::{HostPlatform, TaskQueue};

static V8_INIT: Once = Once::new();
static QUEUE: OnceLock<Arc<TaskQueue>> = OnceLock::new();
static PLATFORM: OnceLock<v8::SharedRef<v8::Platform>> = OnceLock::new();
static RUNTIME: Mutex<()> = Mutex::new(());

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
}

impl RuntimeError {
  pub fn message(message: impl Into<String>) -> Self {
    Self {
      message: message.into(),
    }
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
  let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
  let sandbox = sandbox.map(|path| {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
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
  let deadline = std::time::Instant::now() + deadline;
  let base_url = base.and_then(|path| modules::file_url(path).ok());
  with_runtime(
    false,
    false,
    |_| {},
    |scope| {
      if let Some(path) = base {
        host::set_file_root(path);
      }
      if let Some(root) = sandbox {
        host::set_sandbox_dir(root);
      }
      if let Some(url) = entry_url {
        host::set_entry_url(url.to_string());
      }
      for script in scripts {
        v8::tc_scope!(let tc, scope);
        let Some(code) = v8::String::new(tc, script) else {
          return Ok(ClassicRun {
            value: None,
            error: Some("script is too large".to_string()),
          });
        };
        let origin = script_origin(tc, base_url.as_deref());
        let Some(compiled) = v8::Script::compile(tc, code, origin.as_ref())
        else {
          let text = modules::exception_text!(tc, "failed to compile script");
          return Ok(ClassicRun {
            value: None,
            error: Some(text),
          });
        };
        if compiled.run(tc).is_none() {
          let text = modules::exception_text!(tc, "failed to run script");
          return Ok(ClassicRun {
            value: None,
            error: Some(text),
          });
        }
      }
      pump_until(scope, deadline, done_key)?;
      Ok(ClassicRun {
        value: global_string(scope, done_key),
        error: None,
      })
    },
  )
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
    install_console(scope);
    install_wasm_async(scope);
    install_namespace_instance(scope);
    crate::timers::install(scope);
    crate::realm::install(scope);
    crate::extensions::install(scope);
    bind_wasm_response(scope);
    let result: Result<(), RuntimeError> = (|| {
      if !crate::worker::agent_alive(id) {
        return Ok(());
      }
      let loaded = crate::worker::load_worker_source(
        &script.url,
        script.sandbox.as_deref(),
      )
      .map_err(RuntimeError::message)?;
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
      if crate::worker::agent_alive(id) {
        report_worker_failure(id, &script.url, error.message);
      }
    } else {
      while crate::worker::agent_alive(id) {
        if !pump_once(scope) {
          if !crate::worker::agent_alive(id) {
            break;
          }
          queue.wait(None);
        }
      }
    }
    crate::jobs::clear();
    crate::extensions::cleanup();
  }
  crate::platform::unbind_isolate(token);
  host::exit();
}

fn report_worker_failure(id: u64, url: &str, message: String) {
  let Some(parent) = crate::worker::parent_agent(id) else {
    crate::worker::mark_dead(id);
    return;
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
    let local = v8::Local::new(scope, &promise);
    match local.state() {
      v8::PromiseState::Pending => {
        // The resolving reaction runs inside `pump_once`. That call can
        // then find an idle queue and return false, so read the state again
        // before treating the promise as stuck.
        if !pump_once(scope) && promise_is_pending(scope, &promise) {
          return Err(RuntimeError::message(
            "the event loop stopped while a promise was still pending"
              .to_string(),
          ));
        }
      }
      v8::PromiseState::Fulfilled => return Ok(()),
      v8::PromiseState::Rejected => return rejected(scope, local),
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
    if global_string(scope, done_key).is_some() {
      return Ok(());
    }
    // A worker reply can land as the deadline expires. Deliver what is
    // already queued before treating the harness as unfinished.
    let expired = std::time::Instant::now() >= deadline;
    scope.perform_microtask_checkpoint();
    if global_string(scope, done_key).is_some() {
      return Ok(());
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
    scope.perform_microtask_checkpoint();
    if global_string(scope, done_key).is_some() {
      return Ok(());
    }
    if crate::worker::drain_inbound(scope) {
      if global_string(scope, done_key).is_some()
        || std::time::Instant::now() >= deadline
      {
        return Ok(());
      }
      continue;
    }
    if expired {
      return Ok(());
    }
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
      && !interest.keep_alive
      && !workers
      && !crate::idle::idle_has_pending()
    {
      if wasm_in_flight() == 0 || std::time::Instant::now() >= deadline {
        return Ok(());
      }
      let remaining =
        deadline.saturating_duration_since(std::time::Instant::now());
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
        return Ok(());
      }
      loop_wait(Some(remaining));
      continue;
    };
    let remaining =
      deadline.saturating_duration_since(std::time::Instant::now());
    if remaining.is_zero() {
      return Ok(());
    }
    loop_wait(Some(wait.min(remaining)));
  }
}

pub(crate) fn pump(
  scope: &mut v8::PinScope<'_, '_>,
) -> Result<(), RuntimeError> {
  loop {
    if !pump_once(scope) {
      return Ok(());
    }
  }
}

pub(crate) fn pump_once(scope: &mut v8::PinScope<'_, '_>) -> bool {
  if crate::worker::drain_inbound(scope) {
    scope.perform_microtask_checkpoint();
    return true;
  }
  scope.perform_microtask_checkpoint();
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
  if !host::is_dedicated_worker() && drive_loop(scope) {
    return true;
  }
  if crate::idle::poll_idle_period(scope) {
    return true;
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
    || wasm_in_flight() > 0
    || interest.keep_alive
    || workers
    || crate::idle::idle_has_pending()
  {
    realm_wait(earliest_wait(
      earliest_wait(queue.next_wait(), crate::timers::next_wait()),
      earliest_wait(interest.next_wait, crate::idle::idle_next_wait()),
    ));
    return true;
  }
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

pub(crate) fn with_runtime<T>(
  capture_log: bool,
  test_mode: bool,
  prepare: impl FnOnce(&mut v8::PinScope<'_, '_>),
  body: impl FnOnce(&mut v8::PinScope<'_, '_>) -> Result<T, RuntimeError>,
) -> Result<T, RuntimeError> {
  let _guard = RUNTIME.lock().expect("runtime lock");
  WASM_IN_FLIGHT.with(|count| count.set(0));
  init_v8();
  let queue = QUEUE.get().expect("task queue").clone();
  crate::worker::attach_main(Arc::clone(&queue));
  let mut state = host::HostState::empty(queue, RealmKind::Window, 0);
  state.capture_log = capture_log;
  host::enter(state);
  let mut isolate = v8::Isolate::new(isolate_params());
  configure_isolate(&mut isolate);
  let result = {
    v8::scope!(let scope, &mut isolate);
    let context = v8::Context::new(scope, Default::default());
    let scope = &mut v8::ContextScope::new(scope, context);
    install_console(scope);
    install_wasm_async(scope);
    install_namespace_instance(scope);
    crate::timers::install(scope);
    crate::realm::install(scope);
    crate::extensions::install(scope);
    bind_wasm_response(scope);
    if test_mode {
      install_test_api(scope);
    }
    prepare(scope);
    let result = body(scope);
    crate::jobs::clear();
    crate::extensions::cleanup();
    result
  };
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
  let heap =
    v8::cppgc::Heap::create(platform, v8::cppgc::HeapCreateParams::default());
  v8::CreateParams::default().cpp_heap(heap)
}

fn platform_handle() -> v8::SharedRef<v8::Platform> {
  init_v8();
  PLATFORM.get().expect("platform").clone()
}

fn init_v8() {
  V8_INIT.call_once(|| {
    // Source-phase `import source` is how Wasm modules compile without
    // instantiating. V8 still gates the syntax on this flag.
    v8::V8::set_flags_from_string("--js-source-phase-imports");
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
  let found = if value.is_object() {
    let map = host::wasm_instances(scope);
    map.get(scope, value)
  } else {
    None
  };
  match found {
    Some(found) if !found.is_null_or_undefined() => ret.set(found),
    _ => crate::js::throw_type(
      scope,
      "WebAssembly.namespaceInstance requires a WebAssembly module namespace",
    ),
  }
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
  let line = format_console(scope, &args);
  emit(&line, false);
}

fn console_warn(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut _return: v8::ReturnValue,
) {
  let line = format_console(scope, &args);
  emit(&line, true);
}

fn console_trace(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut _return: v8::ReturnValue,
) {
  let detail = format_console(scope, &args);
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
    let mut parts = Vec::new();
    for index in 1..args.length() {
      parts.push(console_string(scope, args.get(index)));
    }
    parts.join(" ")
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
    let mut parts = Vec::new();
    for index in 1..args.length() {
      parts.push(console_string(scope, args.get(index)));
    }
    line.push(' ');
    line.push_str(&parts.join(" "));
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
    format_console(scope, &args)
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
  if stderr {
    eprintln!("{line}");
  } else {
    println!("{line}");
  }
}

fn format_console(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
) -> String {
  if args.length() == 0 {
    return String::new();
  }
  let first_value = args.get(0);
  // A symbol is not a format string. `ToString` on a symbol throws.
  let first = console_string(scope, first_value);
  if first_value.is_symbol() || !first.contains('%') {
    return join_args(scope, args, 0);
  }
  let chars: Vec<char> = first.chars().collect();
  let mut out = String::new();
  let mut next = 1;
  let mut index = 0;
  while index < chars.len() {
    if chars[index] == '%' && index + 1 < chars.len() {
      let spec = chars[index + 1];
      if spec == '%' {
        out.push('%');
        index += 2;
        continue;
      }
      if matches!(spec, 's' | 'd' | 'i' | 'f' | 'o' | 'O' | 'c') {
        if next < args.length() {
          out.push_str(&format_specifier(scope, spec, args.get(next)));
          next += 1;
        }
        index += 2;
        continue;
      }
    }
    out.push(chars[index]);
    index += 1;
  }
  while next < args.length() {
    out.push(' ');
    out.push_str(&console_string(scope, args.get(next)));
    next += 1;
  }
  out
}

fn join_args(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
  start: i32,
) -> String {
  let mut parts = Vec::new();
  for index in start..args.length() {
    parts.push(console_string(scope, args.get(index)));
  }
  parts.join(" ")
}

fn console_string(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> String {
  if let Ok(symbol) = v8::Local::<v8::Symbol>::try_from(value) {
    let description = symbol.description(scope);
    if description.is_undefined() {
      return "Symbol()".to_string();
    }
    let text = crate::js::rust_string(scope, description);
    return format!("Symbol({text})");
  }
  crate::js::rust_string(scope, value)
}

fn format_specifier(
  scope: &mut v8::PinScope,
  spec: char,
  value: v8::Local<v8::Value>,
) -> String {
  match spec {
    'd' | 'i' => value
      .number_value(scope)
      .map(|number| format!("{}", number as i64))
      .unwrap_or_else(|| "NaN".to_string()),
    'f' => value
      .number_value(scope)
      .map(|number| number.to_string())
      .unwrap_or_else(|| "NaN".to_string()),
    'c' => String::new(),
    _ => console_string(scope, value),
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
    modules::dynamic_import_source(scope, resource_name, specifier)
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
