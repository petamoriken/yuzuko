//! Optional `Yuzuko` child processes.
//!
//! The `child` feature installs `spawn` and `Child` on the main realm and on
//! a dedicated worker. The operating-system process is created on the caller
//! thread. Reading its output and waiting for it run in the background, and
//! the promise settles on that realm's event loop.

use std::cell::RefCell;
use std::collections::HashMap;
use std::io::{ErrorKind, Read};
use std::process::{
  Child as StdChild, ChildStderr, ChildStdout, Command, ExitStatus, Stdio,
};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, Weak};

use yuzuko_macros::op;

use crate::op::{Js, OpError, OpException, wrap_instance};

const MAX_OUTPUT: usize = 1_048_576;
const MAX_ARGS: u32 = 4096;

const ARITY: &str = "Not enough arguments";
const COMMAND_TYPE: &str = "The command must be a string.";
const COMMAND_EMPTY: &str = "The command must not be empty.";
const OPTIONS: &str = "Options must be an object.";
const OPTIONS_READ: &str = "The options could not be read.";
const ARGS: &str = "The arguments must be an array of strings.";
const TOO_MANY: &str = "There are too many arguments.";
const NUL: &str =
  "The command and arguments must not contain a null character.";
const SPAWN_FAIL: &str = "The child process could not be started.";
const FAILED: &str = "The child process failed.";
const TOO_LARGE: &str = "The child output is too large.";

static NEXT: AtomicU64 = AtomicU64::new(1);

thread_local! {
  static PENDING: RefCell<HashMap<u64, Pending>> =
    RefCell::new(HashMap::new());
  static OPEN: RefCell<Vec<Weak<Shared>>> = RefCell::new(Vec::new());
}

struct Pending {
  result: Option<Result<Collected, OutputError>>,
  waiter: Option<v8::Global<v8::PromiseResolver>>,
}

struct Collected {
  code: Option<i32>,
  signal: Option<i32>,
  stdout: Vec<u8>,
  stderr: Vec<u8>,
}

enum OutputError {
  Failed,
  TooLarge,
}

struct Shared {
  live: Mutex<Option<u32>>,
}

pub struct Child {
  shared: Arc<Shared>,
  pid: u32,
  id: u64,
  promise: RefCell<Option<v8::Global<v8::Value>>>,
}

unsafe impl v8::cppgc::GarbageCollected for Child {
  fn trace(&self, _visitor: &mut v8::cppgc::Visitor) {}

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"Child"
  }
}

#[op(name = "Child")]
impl Child {
  #[getter]
  fn pid(&self) -> u32 {
    self.pid
  }

  fn output(&self, scope: &mut v8::PinScope) -> Result<Js, OpError> {
    if let Some(existing) = self.promise.borrow().as_ref() {
      let local = v8::Local::new(scope, existing);
      return Ok(Js(v8::Global::new(scope, local)));
    }
    let Some(resolver) = v8::PromiseResolver::new(scope) else {
      return Err(OpError::type_(FAILED));
    };
    let promise = resolver.get_promise(scope);
    let promise_value: v8::Local<v8::Value> = promise.into();
    self
      .promise
      .borrow_mut()
      .replace(v8::Global::new(scope, promise_value));
    match claim_result(self.id) {
      Claim::Ready(result) => deliver(scope, resolver, result),
      Claim::Pending => {
        if !store_waiter(self.id, v8::Global::new(scope, resolver)) {
          reject(scope, resolver, FAILED);
        }
      }
      Claim::Missing => reject(scope, resolver, FAILED),
    }
    Ok(Js(v8::Global::new(scope, promise_value)))
  }

  fn kill(&self) {
    signal_live(&self.shared);
  }
}

pub(crate) fn install(scope: &mut v8::PinScope) {
  Child::install(scope);
  move_constructor(scope, "Child");
  let yuzuko = crate::namespace(scope, "Yuzuko");
  define_fn(scope, yuzuko, "spawn", 1, spawn_callback);
}

pub(crate) fn clear() {
  PENDING.with(|slots| slots.borrow_mut().clear());
  let open = OPEN.with(|open| std::mem::take(&mut *open.borrow_mut()));
  for weak in open {
    if let Some(shared) = weak.upgrade() {
      signal_live(&shared);
    }
  }
}

fn spawn_callback(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut ret: v8::ReturnValue,
) {
  match spawn_op(scope, &args) {
    Ok(object) => ret.set(object.into()),
    Err(error) => OpException::throw(error, scope),
  }
}

fn spawn_op<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  args: &v8::FunctionCallbackArguments,
) -> Result<v8::Local<'s, v8::Object>, OpError> {
  if args.length() < 1 {
    return Err(OpError::type_(ARITY));
  }
  let command = string_primitive(scope, args.get(0), COMMAND_TYPE)?;
  if command.is_empty() {
    return Err(OpError::type_(COMMAND_EMPTY));
  }
  if command.contains('\0') {
    return Err(OpError::type_(NUL));
  }
  let options = if args.length() == 1 {
    v8::undefined(scope).into()
  } else {
    args.get(1)
  };
  let child_args = read_args(scope, options)?;
  let mut command = Command::new(&command);
  command
    .args(&child_args)
    .stdin(Stdio::null())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped());
  let mut child = command
    .spawn()
    .map_err(|_| plain_error(scope, SPAWN_FAIL))?;
  let pid = child.id();
  let stdout = child.stdout.take();
  let stderr = child.stderr.take();
  let shared = Arc::new(Shared {
    live: Mutex::new(Some(pid)),
  });
  let id = NEXT.fetch_add(1, Ordering::Relaxed);
  PENDING.with(|slots| {
    slots.borrow_mut().insert(
      id,
      Pending {
        result: None,
        waiter: None,
      },
    );
  });
  OPEN.with(|open| open.borrow_mut().push(Arc::downgrade(&shared)));
  let collect_shared = Arc::clone(&shared);
  crate::jobs::spawn(
    move || collect(child, stdout, stderr, collect_shared),
    move |scope, result| {
      let outcome = match result {
        Ok(value) => value,
        Err(()) => Err(OutputError::Failed),
      };
      finish_output(scope, id, outcome);
    },
  );
  // Safety: `Child::install` cached this realm's `Child` template.
  let object = unsafe {
    wrap_instance(
      scope,
      Child {
        shared,
        pid,
        id,
        promise: RefCell::new(None),
      },
    )
  };
  Ok(object)
}

fn finish_output(
  scope: &mut v8::PinScope,
  id: u64,
  outcome: Result<Collected, OutputError>,
) {
  let waiter = PENDING.with(|slots| {
    let mut slots = slots.borrow_mut();
    let Some(pending) = slots.get_mut(&id) else {
      return None;
    };
    pending.waiter.take()
  });
  match waiter {
    Some(waiter) => {
      PENDING.with(|slots| slots.borrow_mut().remove(&id));
      let resolver = v8::Local::new(scope, &waiter);
      deliver(scope, resolver, outcome);
    }
    None => {
      PENDING.with(|slots| {
        if let Some(pending) = slots.borrow_mut().get_mut(&id) {
          pending.result = Some(outcome);
        }
      });
    }
  }
}

enum Claim {
  Ready(Result<Collected, OutputError>),
  Pending,
  Missing,
}

fn claim_result(id: u64) -> Claim {
  PENDING.with(|slots| {
    let mut slots = slots.borrow_mut();
    if !slots.contains_key(&id) {
      return Claim::Missing;
    }
    let ready = slots.get_mut(&id).and_then(|pending| pending.result.take());
    if ready.is_some() {
      slots.remove(&id);
    }
    match ready {
      Some(result) => Claim::Ready(result),
      None => Claim::Pending,
    }
  })
}

fn store_waiter(id: u64, waiter: v8::Global<v8::PromiseResolver>) -> bool {
  PENDING.with(|slots| {
    let mut slots = slots.borrow_mut();
    let Some(pending) = slots.get_mut(&id) else {
      return false;
    };
    if pending.result.is_some() {
      return false;
    }
    pending.waiter = Some(waiter);
    true
  })
}

fn deliver(
  scope: &mut v8::PinScope,
  resolver: v8::Local<v8::PromiseResolver>,
  result: Result<Collected, OutputError>,
) {
  match result {
    Ok(collected) => match output_value(scope, collected) {
      Some(value) => {
        let _ = resolver.resolve(scope, value);
      }
      None => reject(scope, resolver, FAILED),
    },
    Err(OutputError::TooLarge) => reject(scope, resolver, TOO_LARGE),
    Err(OutputError::Failed) => reject(scope, resolver, FAILED),
  }
}

fn reject(
  scope: &mut v8::PinScope,
  resolver: v8::Local<v8::PromiseResolver>,
  message: &str,
) {
  let Some(text) = v8::String::new(scope, message) else {
    return;
  };
  let error = v8::Exception::error(scope, text);
  let _ = resolver.reject(scope, error);
}

fn output_value<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  collected: Collected,
) -> Option<v8::Local<'s, v8::Value>> {
  let object = v8::Object::new(scope);
  let code = nullable_i32(scope, collected.code);
  let signal = nullable_i32(scope, collected.signal);
  let stdout = bytes_value(scope, collected.stdout)?;
  let stderr = bytes_value(scope, collected.stderr)?;
  set_value(scope, object, "code", code)?;
  set_value(scope, object, "signal", signal)?;
  set_value(scope, object, "stdout", stdout)?;
  set_value(scope, object, "stderr", stderr)?;
  Some(object.into())
}

fn set_value(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
  value: v8::Local<v8::Value>,
) -> Option<()> {
  let key = v8::String::new(scope, name)?;
  object.set(scope, key.into(), value);
  Some(())
}

fn nullable_i32<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  value: Option<i32>,
) -> v8::Local<'s, v8::Value> {
  match value {
    Some(number) => v8::Integer::new(scope, number).into(),
    None => v8::null(scope).into(),
  }
}

fn bytes_value<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  bytes: Vec<u8>,
) -> Option<v8::Local<'s, v8::Value>> {
  let length = bytes.len();
  let store = v8::ArrayBuffer::new_backing_store_from_vec(bytes).make_shared();
  let buffer = v8::ArrayBuffer::with_backing_store(scope, &store);
  let view = v8::Uint8Array::new(scope, buffer, 0, length)?;
  Some(view.into())
}

fn collect(
  mut child: StdChild,
  stdout: Option<ChildStdout>,
  stderr: Option<ChildStderr>,
  shared: Arc<Shared>,
) -> Result<Collected, OutputError> {
  let stdout_shared = Arc::clone(&shared);
  let stdout_thread = match std::thread::Builder::new()
    .name("yuzuko-child-stdout".to_string())
    .spawn(move || read_optional(stdout, &stdout_shared))
  {
    Ok(handle) => handle,
    Err(_) => return fail_child(child, &shared),
  };
  let stderr_shared = Arc::clone(&shared);
  let stderr_thread = match std::thread::Builder::new()
    .name("yuzuko-child-stderr".to_string())
    .spawn(move || read_optional(stderr, &stderr_shared))
  {
    Ok(handle) => handle,
    Err(_) => {
      signal_live(&shared);
      let _ = stdout_thread.join();
      return fail_child(child, &shared);
    }
  };
  let status = child.wait();
  *lock(&shared.live) = None;
  let stdout = join_read(stdout_thread);
  let stderr = join_read(stderr_thread);
  let status = status.map_err(|_| OutputError::Failed)?;
  combine(status, stdout, stderr)
}

fn fail_child(
  mut child: StdChild,
  shared: &Shared,
) -> Result<Collected, OutputError> {
  signal_live(shared);
  let _ = child.wait();
  *lock(&shared.live) = None;
  Err(OutputError::Failed)
}

fn join_read(
  handle: std::thread::JoinHandle<Result<Vec<u8>, OutputError>>,
) -> Result<Vec<u8>, OutputError> {
  match handle.join() {
    Ok(value) => value,
    Err(_) => Err(OutputError::Failed),
  }
}

fn combine(
  status: ExitStatus,
  stdout: Result<Vec<u8>, OutputError>,
  stderr: Result<Vec<u8>, OutputError>,
) -> Result<Collected, OutputError> {
  if matches!(stdout, Err(OutputError::TooLarge))
    || matches!(stderr, Err(OutputError::TooLarge))
  {
    return Err(OutputError::TooLarge);
  }
  let stdout = stdout?;
  let stderr = stderr?;
  let (code, signal) = exit_parts(status);
  Ok(Collected {
    code,
    signal,
    stdout,
    stderr,
  })
}

fn read_optional(
  pipe: Option<impl Read>,
  shared: &Shared,
) -> Result<Vec<u8>, OutputError> {
  match pipe {
    Some(pipe) => read_pipe(pipe, shared),
    None => Ok(Vec::new()),
  }
}

fn read_pipe(
  mut pipe: impl Read,
  shared: &Shared,
) -> Result<Vec<u8>, OutputError> {
  let mut buf = Vec::new();
  let mut chunk = [0u8; 8192];
  loop {
    match pipe.read(&mut chunk) {
      Ok(0) => return Ok(buf),
      Ok(count) => {
        if buf.len().saturating_add(count) > MAX_OUTPUT {
          signal_live(shared);
          return Err(OutputError::TooLarge);
        }
        buf.extend_from_slice(&chunk[..count]);
      }
      Err(error) if error.kind() == ErrorKind::Interrupted => continue,
      Err(_) => {
        signal_live(shared);
        return Err(OutputError::Failed);
      }
    }
  }
}

fn exit_parts(status: ExitStatus) -> (Option<i32>, Option<i32>) {
  #[cfg(unix)]
  {
    use std::os::unix::process::ExitStatusExt;
    (status.code(), status.signal())
  }
  #[cfg(not(unix))]
  {
    (status.code(), None)
  }
}

fn signal_live(shared: &Shared) {
  let pid = *lock(&shared.live);
  let Some(pid) = pid else {
    return;
  };
  signal_pid(pid);
}

#[cfg(unix)]
fn signal_pid(pid: u32) {
  if pid == 0 {
    return;
  }
  unsafe {
    libc::kill(pid as i32, libc::SIGKILL);
  }
}

#[cfg(windows)]
fn signal_pid(pid: u32) {
  if pid == 0 {
    return;
  }
  use windows_sys::Win32::Foundation::CloseHandle;
  use windows_sys::Win32::System::Threading::{
    OpenProcess, PROCESS_TERMINATE, TerminateProcess,
  };
  unsafe {
    let handle = OpenProcess(PROCESS_TERMINATE, 0, pid);
    if handle.is_null() {
      return;
    }
    let _ = TerminateProcess(handle, 1);
    let _ = CloseHandle(handle);
  }
}

#[cfg(not(any(unix, windows)))]
fn signal_pid(_pid: u32) {}

fn read_args(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<Vec<String>, OpError> {
  if value.is_null_or_undefined() {
    return Ok(Vec::new());
  }
  if !value.is_object() || value.is_array() {
    return Err(OpError::type_(OPTIONS));
  }
  let Ok(object) = v8::Local::<v8::Object>::try_from(value) else {
    return Err(OpError::type_(OPTIONS));
  };
  let Some(field) = read_field(scope, object, "args")? else {
    return Ok(Vec::new());
  };
  if field.is_null_or_undefined() {
    return Ok(Vec::new());
  }
  let Ok(array) = v8::Local::<v8::Array>::try_from(field) else {
    return Err(OpError::type_(ARGS));
  };
  let length = array.length();
  if length > MAX_ARGS {
    return Err(OpError::type_(TOO_MANY));
  }
  let mut args = Vec::with_capacity(length as usize);
  for index in 0..length {
    let element = array_element(scope, array, index)?;
    let text = string_primitive(scope, element, ARGS)?;
    if text.contains('\0') {
      return Err(OpError::type_(NUL));
    }
    args.push(text);
  }
  Ok(args)
}

fn read_field<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  object: v8::Local<'_, v8::Object>,
  name: &str,
) -> Result<Option<v8::Local<'s, v8::Value>>, OpError> {
  enum Outcome {
    Value(v8::Global<v8::Value>),
    Throw(v8::Global<v8::Value>),
    Missing,
  }
  let outcome = {
    v8::tc_scope!(let tc, scope);
    let Some(key) = v8::String::new(tc, name) else {
      return Err(OpError::type_(OPTIONS_READ));
    };
    let object = v8::Local::new(tc, object);
    match object.get(tc, key.into()) {
      Some(value) => Outcome::Value(v8::Global::new(tc, value)),
      None => {
        if let Some(exception) = tc.exception() {
          Outcome::Throw(v8::Global::new(tc, exception))
        } else {
          Outcome::Missing
        }
      }
    }
  };
  match outcome {
    Outcome::Value(value) => Ok(Some(v8::Local::new(scope, value))),
    Outcome::Throw(exception) => {
      Err(OpError::thrown(scope, v8::Local::new(scope, exception)))
    }
    Outcome::Missing => Err(OpError::type_(OPTIONS_READ)),
  }
}

fn array_element<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  array: v8::Local<'_, v8::Array>,
  index: u32,
) -> Result<v8::Local<'s, v8::Value>, OpError> {
  enum Outcome {
    Value(v8::Global<v8::Value>),
    Throw(v8::Global<v8::Value>),
    Missing,
  }
  let outcome = {
    v8::tc_scope!(let tc, scope);
    let array = v8::Local::new(tc, array);
    match array.get_index(tc, index) {
      Some(value) => Outcome::Value(v8::Global::new(tc, value)),
      None => {
        if let Some(exception) = tc.exception() {
          Outcome::Throw(v8::Global::new(tc, exception))
        } else {
          Outcome::Missing
        }
      }
    }
  };
  match outcome {
    Outcome::Value(value) => Ok(v8::Local::new(scope, value)),
    Outcome::Throw(exception) => {
      Err(OpError::thrown(scope, v8::Local::new(scope, exception)))
    }
    Outcome::Missing => Err(OpError::type_(OPTIONS_READ)),
  }
}

fn string_primitive(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
  message: &'static str,
) -> Result<String, OpError> {
  if !value.is_string() {
    return Err(OpError::type_(message));
  }
  let Ok(text) = v8::Local::<v8::String>::try_from(value) else {
    return Err(OpError::type_(message));
  };
  Ok(text.to_rust_string_lossy(scope))
}

fn plain_error(scope: &mut v8::PinScope, message: &str) -> OpError {
  let Some(text) = v8::String::new(scope, message) else {
    return OpError::type_(message);
  };
  let error = v8::Exception::error(scope, text);
  OpError::thrown(scope, error)
}

fn move_constructor(scope: &mut v8::PinScope, name: &str) {
  let global = scope.get_current_context().global(scope);
  let Some(key) = v8::String::new(scope, name) else {
    return;
  };
  let Some(constructor) = global.get(scope, key.into()) else {
    return;
  };
  let _ = global.delete(scope, key.into());
  let yuzuko = crate::namespace(scope, "Yuzuko");
  yuzuko.set(scope, key.into(), constructor);
}

fn define_fn(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
  length: i32,
  callback: impl v8::MapFnTo<v8::FunctionCallback>,
) {
  let function = v8::Function::builder(callback)
    .length(length)
    .constructor_behavior(v8::ConstructorBehavior::Throw)
    .build(scope)
    .unwrap_or_else(|| panic!("create function {name}"));
  let key = v8::String::new(scope, name).expect(name);
  function.set_name(key);
  object.set(scope, key.into(), function.into());
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
  mutex.lock().unwrap_or_else(|error| error.into_inner())
}
