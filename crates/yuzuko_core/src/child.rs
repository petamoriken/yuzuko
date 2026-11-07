//! Optional `Yuzuko` child processes.
//!
//! The `child` feature installs `spawn` and `Child` on the main realm and on
//! a dedicated worker. The operating-system process is created on the caller
//! thread. Collected output, waiting for exit, and piped reads and writes run
//! in the background. Their promises settle on that realm's event loop.
//!
//! A pipe that fills, or a piped standard input left open while the child
//! reads until the end, blocks the child. `status` and `output` then wait
//! until the caller reads, closes standard input, or kills the process.

use std::cell::RefCell;
use std::collections::HashMap;
use std::io::{ErrorKind, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{
  Child as StdChild, ChildStderr, ChildStdin, ChildStdout, Command, ExitStatus,
  Stdio,
};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, Weak};

use yuzuko_macros::op;

use crate::op::{Js, OpError, OpException, wrap_instance};

const MAX_OUTPUT: usize = 1_048_576;
const MAX_ARGS: u32 = 4096;
const MAX_BUFFER: usize = 2_147_483_647;

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
const NOT_COLLECTED: &str = "The child output is not collected.";
const STDIN_MODE: &str =
  "The standard input must be \"null\", \"inherit\", or \"piped\".";
const STREAM_MODE: &str = "The standard stream must be \"null\", \"inherit\", \"piped\", or \"collect\".";
const STDIN_NOT_PIPED: &str = "The standard input is not piped.";
const STDOUT_NOT_PIPED: &str = "The standard output is not piped.";
const STDERR_NOT_PIPED: &str = "The standard error is not piped.";
const STDIN_CLOSED: &str = "The standard input is closed.";
const READ_WAITING: &str = "A read is already waiting.";
const WRITE_WAITING: &str = "A write is already waiting.";
const READ_FAIL: &str = "The bytes could not be read.";
const WRITE_FAIL: &str = "The bytes could not be written.";
const DATA: &str = "The data must be a BufferSource.";
const DETACHED: &str = "The buffer is detached.";
const BUFFER_TOO_LARGE: &str = "The buffer is too large.";
const ENV_TYPE: &str = "The environment must be an object.";
const NAME_TYPE: &str = "The name must be a string.";
const NAME_EMPTY: &str = "The name must not be empty.";
const NAME_RESERVED: &str = "The name must not contain `=` or NUL.";
const VALUE_TYPE: &str = "The value must be a string or null.";
const VALUE_NUL: &str = "The value must not contain NUL.";
const PATH_TYPE: &str = "The path must be a string or a file URL.";
const PATH_SCHEME: &str = "The URL scheme must be file.";
const PATH_URL: &str = "The URL is invalid.";
const PATH_FILE: &str = "The file URL is invalid.";
const PATH_NUL: &str = "The path must not contain NUL.";
const DIR_MISSING: &str = "The directory was not found.";
const DIR_KIND: &str = "The path is not a directory.";

static NEXT: AtomicU64 = AtomicU64::new(1);

thread_local! {
  static PENDING: RefCell<HashMap<u64, Pending>> =
    RefCell::new(HashMap::new());
  static OPEN: RefCell<Vec<Weak<Shared>>> = const { RefCell::new(Vec::new()) };
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum StdioMode {
  Null,
  Inherit,
  Piped,
  Collect,
}

impl StdioMode {
  fn name(self) -> &'static str {
    match self {
      Self::Null => "null",
      Self::Inherit => "inherit",
      Self::Piped => "piped",
      Self::Collect => "collect",
    }
  }

  fn as_stdio(self) -> Stdio {
    match self {
      Self::Null => Stdio::null(),
      Self::Inherit => Stdio::inherit(),
      Self::Piped | Self::Collect => Stdio::piped(),
    }
  }
}

struct IoSlot<T> {
  pipe: Option<T>,
  piped: bool,
  busy: bool,
  eof: bool,
  closed: bool,
}

struct Pending {
  result: Option<Result<Collected, OutputError>>,
  output_waiter: Option<v8::Global<v8::PromiseResolver>>,
  status_waiter: Option<v8::Global<v8::PromiseResolver>>,
  output_done: bool,
  status_done: bool,
}

struct Collected {
  code: Option<i32>,
  signal: Option<i32>,
  stdout: Vec<u8>,
  stderr: Vec<u8>,
}

struct StatusParts {
  code: Option<i32>,
  signal: Option<i32>,
}

#[derive(Clone, Copy)]
enum OutputError {
  Failed,
  TooLarge,
}

struct Shared {
  live: Mutex<Option<u32>>,
  stdin: Mutex<IoSlot<ChildStdin>>,
  stdout: Mutex<IoSlot<ChildStdout>>,
  stderr: Mutex<IoSlot<ChildStderr>>,
}

struct SpawnRequest {
  args: Vec<String>,
  stdin: StdioMode,
  stdout: StdioMode,
  stderr: StdioMode,
  env: Vec<EnvChange>,
  cwd: Option<PathBuf>,
}

struct EnvChange {
  name: String,
  value: Option<String>,
}

struct Delivery {
  output: Option<(
    v8::Global<v8::PromiseResolver>,
    Result<Collected, OutputError>,
  )>,
  status: Option<(
    v8::Global<v8::PromiseResolver>,
    Result<StatusParts, OutputError>,
  )>,
}

impl Delivery {
  fn empty() -> Self {
    Self {
      output: None,
      status: None,
    }
  }
}

enum Claim<T> {
  Ready(T),
  Pending,
  Missing,
}

enum Target {
  View(v8::Global<v8::ArrayBufferView>),
  Buffer(v8::Global<v8::ArrayBuffer>),
  Shared(v8::Global<v8::SharedArrayBuffer>),
}

struct ReadDone<T> {
  pipe: T,
  result: Result<Option<Vec<u8>>, ()>,
}

struct WriteDone<T> {
  pipe: T,
  result: Result<usize, ()>,
}

enum Started {
  Empty,
  Thread(std::thread::JoinHandle<Result<Vec<u8>, OutputError>>),
}

pub struct Child {
  shared: Arc<Shared>,
  pid: u32,
  id: u64,
  collects: bool,
  stdin_mode: StdioMode,
  stdout_mode: StdioMode,
  stderr_mode: StdioMode,
  output_promise: RefCell<Option<v8::Global<v8::Value>>>,
  status_promise: RefCell<Option<v8::Global<v8::Value>>>,
}

unsafe impl v8::cppgc::GarbageCollected for Child {
  fn trace(&self, _visitor: &mut v8::cppgc::Visitor) {}

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"Child"
  }
}

impl Drop for Child {
  fn drop(&mut self) {
    release_stdin(&self.shared);
  }
}

#[op(name = "Child")]
impl Child {
  #[getter]
  fn pid(&self) -> u32 {
    self.pid
  }

  #[getter]
  fn stdin_mode(&self) -> &'static str {
    self.stdin_mode.name()
  }

  #[getter]
  fn stdout_mode(&self) -> &'static str {
    self.stdout_mode.name()
  }

  #[getter]
  fn stderr_mode(&self) -> &'static str {
    self.stderr_mode.name()
  }

  fn output(&self, scope: &mut v8::PinScope) -> Result<Js, OpError> {
    if !self.collects {
      return Err(OpError::type_(NOT_COLLECTED));
    }
    promise_method(
      scope,
      &self.output_promise,
      self.id,
      claim_output,
      store_output_waiter,
      deliver_output,
    )
  }

  fn status(&self, scope: &mut v8::PinScope) -> Result<Js, OpError> {
    promise_method(
      scope,
      &self.status_promise,
      self.id,
      claim_status,
      store_status_waiter,
      deliver_status,
    )
  }

  fn read_stdout(
    &self,
    scope: &mut v8::PinScope,
    buffer: v8::Local<v8::Value>,
  ) -> Result<Js, OpError> {
    read_side(scope, &self.shared, buffer, stdout_slot, STDOUT_NOT_PIPED)
  }

  fn read_stderr(
    &self,
    scope: &mut v8::PinScope,
    buffer: v8::Local<v8::Value>,
  ) -> Result<Js, OpError> {
    read_side(scope, &self.shared, buffer, stderr_slot, STDERR_NOT_PIPED)
  }

  fn write_stdin(
    &self,
    scope: &mut v8::PinScope,
    data: v8::Local<v8::Value>,
  ) -> Result<Js, OpError> {
    write_stdin(scope, &self.shared, data)
  }

  fn close_stdin(&self) -> Result<(), OpError> {
    let mut slot = lock(&self.shared.stdin);
    if !slot.piped {
      return Err(OpError::type_(STDIN_NOT_PIPED));
    }
    if slot.closed {
      return Err(OpError::type_(STDIN_CLOSED));
    }
    slot.closed = true;
    if !slot.busy {
      slot.pipe.take();
    }
    Ok(())
  }

  fn kill(&self) {
    signal_live(&self.shared);
  }
}

pub(crate) fn install(scope: &mut v8::PinScope) {
  Child::install(scope);
  move_constructor(scope, "Child");
  install_streams(scope);
  let yuzuko = crate::namespace(scope, "Yuzuko");
  define_fn(scope, yuzuko, "spawn", 1, spawn_callback);
}

pub(crate) fn clear() {
  PENDING.with(|slots| slots.borrow_mut().clear());
  let open = OPEN.with(|open| std::mem::take(&mut *open.borrow_mut()));
  for weak in open {
    if let Some(shared) = weak.upgrade() {
      signal_live(&shared);
      release_stdin(&shared);
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
  let program = string_primitive(scope, args.get(0), COMMAND_TYPE)?;
  if program.is_empty() {
    return Err(OpError::type_(COMMAND_EMPTY));
  }
  if program.contains('\0') {
    return Err(OpError::type_(NUL));
  }
  let options = if args.length() == 1 {
    v8::undefined(scope).into()
  } else {
    args.get(1)
  };
  let request = read_options(scope, options)?;
  let mut command = Command::new(&program);
  command.args(&request.args);
  command.stdin(request.stdin.as_stdio());
  command.stdout(request.stdout.as_stdio());
  command.stderr(request.stderr.as_stdio());
  apply_env(&mut command, &request.env);
  if let Some(dir) = &request.cwd {
    command.current_dir(dir);
  }
  let mut child = command
    .spawn()
    .map_err(|_| plain_error(scope, SPAWN_FAIL))?;
  let pid = child.id();
  let (stdin_slot, _) = pipe_slot(request.stdin, child.stdin.take());
  let (stdout_slot, stdout_pipe) =
    pipe_slot(request.stdout, child.stdout.take());
  let (stderr_slot, stderr_pipe) =
    pipe_slot(request.stderr, child.stderr.take());
  let collects =
    request.stdout != StdioMode::Piped && request.stderr != StdioMode::Piped;
  let shared = Arc::new(Shared {
    live: Mutex::new(Some(pid)),
    stdin: Mutex::new(stdin_slot),
    stdout: Mutex::new(stdout_slot),
    stderr: Mutex::new(stderr_slot),
  });
  let id = NEXT.fetch_add(1, Ordering::Relaxed);
  PENDING.with(|slots| {
    slots.borrow_mut().insert(
      id,
      Pending {
        result: None,
        output_waiter: None,
        status_waiter: None,
        output_done: !collects,
        status_done: false,
      },
    );
  });
  OPEN.with(|open| open.borrow_mut().push(Arc::downgrade(&shared)));
  let collect_shared = Arc::clone(&shared);
  crate::jobs::spawn(
    move || collect(child, stdout_pipe, stderr_pipe, collect_shared),
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
        collects,
        stdin_mode: request.stdin,
        stdout_mode: request.stdout,
        stderr_mode: request.stderr,
        output_promise: RefCell::new(None),
        status_promise: RefCell::new(None),
      },
    )
  };
  Ok(object)
}

fn pipe_slot<T>(mode: StdioMode, pipe: Option<T>) -> (IoSlot<T>, Option<T>) {
  match mode {
    StdioMode::Piped => {
      let eof = pipe.is_none();
      (
        IoSlot {
          pipe,
          piped: true,
          busy: false,
          eof,
          closed: false,
        },
        None,
      )
    }
    StdioMode::Collect => (
      IoSlot {
        pipe: None,
        piped: false,
        busy: false,
        eof: false,
        closed: false,
      },
      pipe,
    ),
    StdioMode::Null | StdioMode::Inherit => (
      IoSlot {
        pipe: None,
        piped: false,
        busy: false,
        eof: false,
        closed: false,
      },
      None,
    ),
  }
}

fn promise_method<T, ClaimFn, StoreFn, DeliverFn>(
  scope: &mut v8::PinScope,
  slot: &RefCell<Option<v8::Global<v8::Value>>>,
  id: u64,
  claim: ClaimFn,
  store: StoreFn,
  deliver: DeliverFn,
) -> Result<Js, OpError>
where
  ClaimFn: Fn(u64) -> Claim<T>,
  StoreFn: FnOnce(u64, v8::Global<v8::PromiseResolver>) -> bool,
  DeliverFn: FnOnce(&mut v8::PinScope, v8::Local<v8::PromiseResolver>, T),
{
  if let Some(existing) = slot.borrow().as_ref() {
    let local = v8::Local::new(scope, existing);
    return Ok(Js(v8::Global::new(scope, local)));
  }
  let Some(resolver) = v8::PromiseResolver::new(scope) else {
    return Err(OpError::type_(FAILED));
  };
  let promise = resolver.get_promise(scope);
  let promise_value: v8::Local<v8::Value> = promise.into();
  slot
    .borrow_mut()
    .replace(v8::Global::new(scope, promise_value));
  match claim(id) {
    Claim::Ready(result) => deliver(scope, resolver, result),
    Claim::Pending => {
      if !store(id, v8::Global::new(scope, resolver)) {
        match claim(id) {
          Claim::Ready(result) => deliver(scope, resolver, result),
          Claim::Pending | Claim::Missing => {
            reject(scope, resolver, FAILED, false)
          }
        }
      }
    }
    Claim::Missing => reject(scope, resolver, FAILED, false),
  }
  Ok(Js(v8::Global::new(scope, promise_value)))
}

fn finish_output(
  scope: &mut v8::PinScope,
  id: u64,
  outcome: Result<Collected, OutputError>,
) {
  let delivery = PENDING.with(|slots| {
    let mut slots = slots.borrow_mut();
    let prepared = {
      let Some(pending) = slots.get_mut(&id) else {
        return Delivery::empty();
      };
      pending.result = Some(outcome);
      let output_waiter = pending.output_waiter.take();
      let status_waiter = pending.status_waiter.take();
      let mut output = None;
      let mut status = None;
      if let Some(waiter) = output_waiter {
        pending.output_done = true;
        if let Some(result) = pending.result.as_mut() {
          output = Some((waiter, take_output(result)));
        }
      }
      if let Some(waiter) = status_waiter {
        pending.status_done = true;
        if let Some(result) = pending.result.as_ref() {
          status = Some((waiter, status_view(result)));
        }
      }
      (output, status, pending.output_done && pending.status_done)
    };
    if prepared.2 {
      slots.remove(&id);
    }
    Delivery {
      output: prepared.0,
      status: prepared.1,
    }
  });
  if let Some((waiter, result)) = delivery.output {
    let resolver = v8::Local::new(scope, &waiter);
    deliver_output(scope, resolver, result);
  }
  if let Some((waiter, result)) = delivery.status {
    let resolver = v8::Local::new(scope, &waiter);
    deliver_status(scope, resolver, result);
  }
}

fn claim_output(id: u64) -> Claim<Result<Collected, OutputError>> {
  PENDING.with(|slots| {
    let mut slots = slots.borrow_mut();
    let ready = {
      let Some(pending) = slots.get_mut(&id) else {
        return Claim::Missing;
      };
      let Some(result) = pending.result.as_mut() else {
        return Claim::Pending;
      };
      let snapshot = take_output(result);
      pending.output_done = true;
      (snapshot, pending.status_done)
    };
    if ready.1 {
      slots.remove(&id);
    }
    Claim::Ready(ready.0)
  })
}

fn claim_status(id: u64) -> Claim<Result<StatusParts, OutputError>> {
  PENDING.with(|slots| {
    let mut slots = slots.borrow_mut();
    let ready = {
      let Some(pending) = slots.get_mut(&id) else {
        return Claim::Missing;
      };
      let Some(result) = pending.result.as_ref() else {
        return Claim::Pending;
      };
      let snapshot = status_view(result);
      pending.status_done = true;
      (snapshot, pending.output_done)
    };
    if ready.1 {
      slots.remove(&id);
    }
    Claim::Ready(ready.0)
  })
}

fn store_output_waiter(
  id: u64,
  waiter: v8::Global<v8::PromiseResolver>,
) -> bool {
  store_waiter(id, waiter, true)
}

fn store_status_waiter(
  id: u64,
  waiter: v8::Global<v8::PromiseResolver>,
) -> bool {
  store_waiter(id, waiter, false)
}

fn store_waiter(
  id: u64,
  waiter: v8::Global<v8::PromiseResolver>,
  output: bool,
) -> bool {
  PENDING.with(|slots| {
    let mut slots = slots.borrow_mut();
    let Some(pending) = slots.get_mut(&id) else {
      return false;
    };
    if pending.result.is_some() {
      return false;
    }
    if output {
      if pending.output_done {
        return false;
      }
      pending.output_waiter = Some(waiter);
    } else {
      if pending.status_done {
        return false;
      }
      pending.status_waiter = Some(waiter);
    }
    true
  })
}

fn take_output(
  result: &mut Result<Collected, OutputError>,
) -> Result<Collected, OutputError> {
  match result {
    Err(error) => Err(*error),
    Ok(collected) => Ok(Collected {
      code: collected.code,
      signal: collected.signal,
      stdout: std::mem::take(&mut collected.stdout),
      stderr: std::mem::take(&mut collected.stderr),
    }),
  }
}

fn status_view(
  result: &Result<Collected, OutputError>,
) -> Result<StatusParts, OutputError> {
  match result {
    Err(error) => Err(*error),
    Ok(collected) => Ok(StatusParts {
      code: collected.code,
      signal: collected.signal,
    }),
  }
}

fn deliver_output(
  scope: &mut v8::PinScope,
  resolver: v8::Local<v8::PromiseResolver>,
  result: Result<Collected, OutputError>,
) {
  match result {
    Ok(collected) => match output_value(scope, collected) {
      Some(value) => {
        let _ = resolver.resolve(scope, value);
      }
      None => reject(scope, resolver, FAILED, false),
    },
    Err(OutputError::TooLarge) => reject(scope, resolver, TOO_LARGE, false),
    Err(OutputError::Failed) => reject(scope, resolver, FAILED, false),
  }
}

fn deliver_status(
  scope: &mut v8::PinScope,
  resolver: v8::Local<v8::PromiseResolver>,
  result: Result<StatusParts, OutputError>,
) {
  match result {
    Ok(parts) => match status_value(scope, parts) {
      Some(value) => {
        let _ = resolver.resolve(scope, value);
      }
      None => reject(scope, resolver, FAILED, false),
    },
    Err(OutputError::TooLarge) => reject(scope, resolver, TOO_LARGE, false),
    Err(OutputError::Failed) => reject(scope, resolver, FAILED, false),
  }
}

fn reject(
  scope: &mut v8::PinScope,
  resolver: v8::Local<v8::PromiseResolver>,
  message: &str,
  type_error: bool,
) {
  let Some(text) = v8::String::new(scope, message) else {
    return;
  };
  let error = if type_error {
    v8::Exception::type_error(scope, text)
  } else {
    v8::Exception::error(scope, text)
  };
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

fn status_value<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  parts: StatusParts,
) -> Option<v8::Local<'s, v8::Value>> {
  let object = v8::Object::new(scope);
  let code = nullable_i32(scope, parts.code);
  let signal = nullable_i32(scope, parts.signal);
  set_value(scope, object, "code", code)?;
  set_value(scope, object, "signal", signal)?;
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

fn stdout_slot(shared: &Shared) -> &Mutex<IoSlot<ChildStdout>> {
  &shared.stdout
}

fn stderr_slot(shared: &Shared) -> &Mutex<IoSlot<ChildStderr>> {
  &shared.stderr
}

fn read_side<T>(
  scope: &mut v8::PinScope,
  shared: &Arc<Shared>,
  buffer: v8::Local<v8::Value>,
  slot_of: fn(&Shared) -> &Mutex<IoSlot<T>>,
  not_piped: &'static str,
) -> Result<Js, OpError>
where
  T: Read + Send + 'static,
{
  let (target, len) = classify(scope, buffer)?;
  let mut slot = lock(slot_of(shared));
  if !slot.piped {
    return Err(OpError::type_(not_piped));
  }
  if slot.busy {
    return Err(OpError::type_(READ_WAITING));
  }
  if len == 0 {
    drop(slot);
    return resolve_number(scope, 0.0, READ_FAIL);
  }
  if slot.eof {
    drop(slot);
    return resolve_null(scope, READ_FAIL);
  }
  let Some(pipe) = slot.pipe.take() else {
    slot.eof = true;
    drop(slot);
    return resolve_null(scope, READ_FAIL);
  };
  slot.busy = true;
  drop(slot);
  let Some(resolver) = v8::PromiseResolver::new(scope) else {
    let mut slot = lock(slot_of(shared));
    slot.pipe = Some(pipe);
    slot.busy = false;
    return Err(OpError::type_(READ_FAIL));
  };
  let promise = resolver.get_promise(scope);
  let promise_value: v8::Local<v8::Value> = promise.into();
  let resolver = v8::Global::new(scope, resolver);
  let shared = Arc::clone(shared);
  crate::jobs::spawn(
    move || {
      let mut pipe = pipe;
      let result = read_some(&mut pipe, len);
      ReadDone { pipe, result }
    },
    move |scope, done| {
      finish_read(scope, &shared, slot_of, done, target, resolver);
    },
  );
  Ok(Js(v8::Global::new(scope, promise_value)))
}

fn finish_read<T>(
  scope: &mut v8::PinScope,
  shared: &Shared,
  slot_of: fn(&Shared) -> &Mutex<IoSlot<T>>,
  done: Result<ReadDone<T>, ()>,
  target: Target,
  resolver: v8::Global<v8::PromiseResolver>,
) {
  let resolver = v8::Local::new(scope, &resolver);
  match done {
    Err(()) => {
      mark_eof(slot_of(shared));
      reject(scope, resolver, READ_FAIL, false);
    }
    Ok(done) => match done.result {
      Ok(None) => {
        mark_eof(slot_of(shared));
        drop(done.pipe);
        let _ = resolver.resolve(scope, v8::null(scope).into());
      }
      Ok(Some(bytes)) => {
        restore_reader(slot_of(shared), done.pipe);
        match write_target(scope, target, &bytes) {
          Ok(()) => {
            let count = v8::Number::new(scope, bytes.len() as f64);
            let _ = resolver.resolve(scope, count.into());
          }
          Err(()) => reject(scope, resolver, DETACHED, true),
        }
      }
      Err(()) => {
        restore_reader(slot_of(shared), done.pipe);
        reject(scope, resolver, READ_FAIL, false);
      }
    },
  }
}

fn mark_eof<T>(slot: &Mutex<IoSlot<T>>) {
  let mut slot = lock(slot);
  slot.busy = false;
  slot.eof = true;
  slot.pipe = None;
}

fn restore_reader<T>(slot: &Mutex<IoSlot<T>>, pipe: T) {
  let mut slot = lock(slot);
  slot.busy = false;
  slot.pipe = Some(pipe);
}

fn read_some(pipe: &mut impl Read, len: usize) -> Result<Option<Vec<u8>>, ()> {
  let mut buf = vec![0u8; len];
  loop {
    match pipe.read(&mut buf) {
      Ok(0) => return Ok(None),
      Ok(count) => {
        buf.truncate(count);
        return Ok(Some(buf));
      }
      Err(error) if error.kind() == ErrorKind::Interrupted => continue,
      Err(_) => return Err(()),
    }
  }
}

fn write_stdin(
  scope: &mut v8::PinScope,
  shared: &Arc<Shared>,
  data: v8::Local<v8::Value>,
) -> Result<Js, OpError> {
  let bytes = copy_bytes(scope, data)?;
  let mut slot = lock(&shared.stdin);
  if !slot.piped {
    return Err(OpError::type_(STDIN_NOT_PIPED));
  }
  if slot.closed {
    return Err(OpError::type_(STDIN_CLOSED));
  }
  if slot.busy {
    return Err(OpError::type_(WRITE_WAITING));
  }
  if bytes.is_empty() {
    drop(slot);
    return resolve_number(scope, 0.0, WRITE_FAIL);
  }
  let Some(pipe) = slot.pipe.take() else {
    slot.closed = true;
    drop(slot);
    return Err(OpError::type_(STDIN_CLOSED));
  };
  slot.busy = true;
  drop(slot);
  let Some(resolver) = v8::PromiseResolver::new(scope) else {
    let mut slot = lock(&shared.stdin);
    slot.pipe = Some(pipe);
    slot.busy = false;
    return Err(OpError::type_(WRITE_FAIL));
  };
  let promise = resolver.get_promise(scope);
  let promise_value: v8::Local<v8::Value> = promise.into();
  let resolver = v8::Global::new(scope, resolver);
  let shared = Arc::clone(shared);
  crate::jobs::spawn(
    move || {
      let mut pipe = pipe;
      let result = write_all(&mut pipe, &bytes);
      WriteDone { pipe, result }
    },
    move |scope, done| finish_write(scope, &shared, done, resolver),
  );
  Ok(Js(v8::Global::new(scope, promise_value)))
}

fn finish_write(
  scope: &mut v8::PinScope,
  shared: &Shared,
  done: Result<WriteDone<ChildStdin>, ()>,
  resolver: v8::Global<v8::PromiseResolver>,
) {
  let resolver = v8::Local::new(scope, &resolver);
  match done {
    Err(()) => {
      abandon_stdin(shared);
      reject(scope, resolver, WRITE_FAIL, false);
    }
    Ok(done) => {
      restore_stdin(shared, done.pipe);
      match done.result {
        Ok(count) => {
          let count = v8::Number::new(scope, count as f64);
          let _ = resolver.resolve(scope, count.into());
        }
        Err(()) => reject(scope, resolver, WRITE_FAIL, false),
      }
    }
  }
}

fn restore_stdin(shared: &Shared, pipe: ChildStdin) {
  let mut slot = lock(&shared.stdin);
  slot.busy = false;
  if slot.closed {
    drop(pipe);
    slot.pipe = None;
  } else {
    slot.pipe = Some(pipe);
  }
}

fn abandon_stdin(shared: &Shared) {
  let mut slot = lock(&shared.stdin);
  slot.busy = false;
  slot.closed = true;
  slot.pipe = None;
}

fn write_all(pipe: &mut impl Write, bytes: &[u8]) -> Result<usize, ()> {
  let mut written = 0;
  while written < bytes.len() {
    match pipe.write(&bytes[written..]) {
      Ok(0) => return Ok(written),
      Ok(count) => written += count,
      Err(error) if error.kind() == ErrorKind::Interrupted => continue,
      Err(_) => {
        if written > 0 {
          return Ok(written);
        }
        return Err(());
      }
    }
  }
  Ok(written)
}

fn release_stdin(shared: &Shared) {
  let mut slot = lock(&shared.stdin);
  if !slot.piped || slot.closed {
    return;
  }
  slot.closed = true;
  if !slot.busy {
    slot.pipe.take();
  }
}

fn resolve_number(
  scope: &mut v8::PinScope,
  value: f64,
  fail: &'static str,
) -> Result<Js, OpError> {
  let Some(resolver) = v8::PromiseResolver::new(scope) else {
    return Err(OpError::type_(fail));
  };
  let number = v8::Number::new(scope, value);
  let _ = resolver.resolve(scope, number.into());
  Ok(js_promise(scope, resolver))
}

fn resolve_null(
  scope: &mut v8::PinScope,
  fail: &'static str,
) -> Result<Js, OpError> {
  let Some(resolver) = v8::PromiseResolver::new(scope) else {
    return Err(OpError::type_(fail));
  };
  let _ = resolver.resolve(scope, v8::null(scope).into());
  Ok(js_promise(scope, resolver))
}

fn js_promise(
  scope: &mut v8::PinScope,
  resolver: v8::Local<v8::PromiseResolver>,
) -> Js {
  let promise = resolver.get_promise(scope);
  let value: v8::Local<v8::Value> = promise.into();
  Js(v8::Global::new(scope, value))
}

fn collect(
  mut child: StdChild,
  stdout: Option<ChildStdout>,
  stderr: Option<ChildStderr>,
  shared: Arc<Shared>,
) -> Result<Collected, OutputError> {
  let stdout_thread =
    match start_reader("yuzuko-child-stdout", stdout, Arc::clone(&shared)) {
      Ok(started) => started,
      Err(()) => return fail_child(child, &shared),
    };
  let stderr_thread =
    match start_reader("yuzuko-child-stderr", stderr, Arc::clone(&shared)) {
      Ok(started) => started,
      Err(()) => {
        signal_live(&shared);
        let _ = join_started(stdout_thread);
        return fail_child(child, &shared);
      }
    };
  let status = child.wait();
  *lock(&shared.live) = None;
  let stdout = join_started(stdout_thread);
  let stderr = join_started(stderr_thread);
  let status = status.map_err(|_| OutputError::Failed)?;
  combine(status, stdout, stderr)
}

fn start_reader<R>(
  name: &str,
  pipe: Option<R>,
  shared: Arc<Shared>,
) -> Result<Started, ()>
where
  R: Read + Send + 'static,
{
  let Some(pipe) = pipe else {
    return Ok(Started::Empty);
  };
  let handle = std::thread::Builder::new()
    .name(name.to_string())
    .spawn(move || read_pipe(pipe, &shared))
    .map_err(|_| ())?;
  Ok(Started::Thread(handle))
}

fn join_started(started: Started) -> Result<Vec<u8>, OutputError> {
  match started {
    Started::Empty => Ok(Vec::new()),
    Started::Thread(handle) => match handle.join() {
      Ok(value) => value,
      Err(_) => Err(OutputError::Failed),
    },
  }
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

fn read_options(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<SpawnRequest, OpError> {
  if value.is_null_or_undefined() {
    return Ok(SpawnRequest {
      args: Vec::new(),
      stdin: StdioMode::Null,
      stdout: StdioMode::Collect,
      stderr: StdioMode::Collect,
      env: Vec::new(),
      cwd: None,
    });
  }
  if !value.is_object() || value.is_array() {
    return Err(OpError::type_(OPTIONS));
  }
  let Ok(object) = v8::Local::<v8::Object>::try_from(value) else {
    return Err(OpError::type_(OPTIONS));
  };
  let args = read_args_field(scope, object)?;
  let stdin =
    read_mode(scope, object, "stdin", StdioMode::Null, false, STDIN_MODE)?;
  let stdout = read_mode(
    scope,
    object,
    "stdout",
    StdioMode::Collect,
    true,
    STREAM_MODE,
  )?;
  let stderr = read_mode(
    scope,
    object,
    "stderr",
    StdioMode::Collect,
    true,
    STREAM_MODE,
  )?;
  let env = read_env(scope, object)?;
  let cwd = read_cwd_field(scope, object)?;
  Ok(SpawnRequest {
    args,
    stdin,
    stdout,
    stderr,
    env,
    cwd,
  })
}

fn read_args_field(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
) -> Result<Vec<String>, OpError> {
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

fn read_mode(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
  default: StdioMode,
  allow_collect: bool,
  message: &'static str,
) -> Result<StdioMode, OpError> {
  let Some(field) = read_field(scope, object, name)? else {
    return Ok(default);
  };
  if field.is_null_or_undefined() {
    return Ok(default);
  }
  let text = string_primitive(scope, field, message)?;
  match text.as_str() {
    "null" => Ok(StdioMode::Null),
    "inherit" => Ok(StdioMode::Inherit),
    "piped" => Ok(StdioMode::Piped),
    "collect" if allow_collect => Ok(StdioMode::Collect),
    _ => Err(OpError::type_(message)),
  }
}

fn read_env(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
) -> Result<Vec<EnvChange>, OpError> {
  let Some(field) = read_field(scope, object, "env")? else {
    return Ok(Vec::new());
  };
  if field.is_null_or_undefined() {
    return Ok(Vec::new());
  }
  if !field.is_object() || field.is_array() {
    return Err(OpError::type_(ENV_TYPE));
  }
  let Ok(env) = v8::Local::<v8::Object>::try_from(field) else {
    return Err(OpError::type_(ENV_TYPE));
  };
  let names = own_names(scope, env)?;
  let mut changes = Vec::with_capacity(names.len());
  for name in names {
    if name.is_empty() {
      return Err(OpError::type_(NAME_EMPTY));
    }
    if name.as_bytes().contains(&b'=') || name.as_bytes().contains(&0) {
      return Err(OpError::type_(NAME_RESERVED));
    }
    let Some(value) = read_field(scope, env, &name)? else {
      return Err(OpError::type_(OPTIONS_READ));
    };
    if value.is_null() {
      changes.push(EnvChange { name, value: None });
      continue;
    }
    if !value.is_string() {
      return Err(OpError::type_(VALUE_TYPE));
    }
    let text = string_primitive(scope, value, VALUE_TYPE)?;
    if text.as_bytes().contains(&0) {
      return Err(OpError::type_(VALUE_NUL));
    }
    changes.push(EnvChange {
      name,
      value: Some(text),
    });
  }
  Ok(changes)
}

fn apply_env(command: &mut Command, env: &[EnvChange]) {
  for change in env {
    if change.value.is_none() {
      command.env_remove(&change.name);
    }
  }
  for change in env {
    if let Some(value) = &change.value {
      command.env(&change.name, value);
    }
  }
}

fn read_cwd_field(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
) -> Result<Option<PathBuf>, OpError> {
  let Some(field) = read_field(scope, object, "cwd")? else {
    return Ok(None);
  };
  let Some(path) = parse_cwd(scope, field)? else {
    return Ok(None);
  };
  ensure_directory(scope, &path)?;
  Ok(Some(path))
}

fn parse_cwd(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<Option<PathBuf>, OpError> {
  if value.is_null_or_undefined() {
    return Ok(None);
  }
  if value.is_string() {
    let text = string_primitive(scope, value, PATH_TYPE)?;
    if text.is_empty() {
      return Ok(None);
    }
    if text.as_bytes().contains(&0) {
      return Err(OpError::type_(PATH_NUL));
    }
    return Ok(Some(PathBuf::from(text)));
  }
  if is_url(scope, value) {
    let object = v8::Local::<v8::Object>::try_from(value)
      .map_err(|_| OpError::type_(PATH_URL))?;
    let href = href_of(scope, object).map_err(OpError::type_)?;
    let url = url::Url::parse(&href).map_err(|_| OpError::type_(PATH_URL))?;
    if url.scheme() != "file" {
      return Err(OpError::type_(PATH_SCHEME));
    }
    let path = url.to_file_path().map_err(|_| OpError::type_(PATH_FILE))?;
    if path.as_os_str().as_encoded_bytes().contains(&0) {
      return Err(OpError::type_(PATH_NUL));
    }
    if path.as_os_str().is_empty() {
      return Ok(None);
    }
    return Ok(Some(path));
  }
  Err(OpError::type_(PATH_TYPE))
}

fn ensure_directory(
  scope: &mut v8::PinScope,
  path: &Path,
) -> Result<(), OpError> {
  match std::fs::metadata(path) {
    Ok(info) if info.is_dir() => Ok(()),
    Ok(_) => Err(plain_error(scope, DIR_KIND)),
    Err(error) if error.kind() == ErrorKind::NotFound => {
      Err(plain_error(scope, DIR_MISSING))
    }
    Err(error) if error.kind() == ErrorKind::NotADirectory => {
      Err(plain_error(scope, DIR_KIND))
    }
    Err(_) => Err(plain_error(scope, SPAWN_FAIL)),
  }
}

fn is_url(scope: &mut v8::PinScope, value: v8::Local<v8::Value>) -> bool {
  let global = scope.get_current_context().global(scope);
  let Some(key) = v8::String::new(scope, "URL") else {
    return false;
  };
  let Some(ctor) = global.get(scope, key.into()) else {
    return false;
  };
  let Ok(ctor) = v8::Local::<v8::Object>::try_from(ctor) else {
    return false;
  };
  value.instance_of(scope, ctor) == Some(true)
}

fn href_of(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
) -> Result<String, &'static str> {
  let key = v8::String::new(scope, "href").expect("href");

  {
    v8::tc_scope!(let tc, scope);
    match object.get(tc, key.into()) {
      Some(value) if tc.exception().is_none() => match value.to_string(tc) {
        Some(text) if tc.exception().is_none() => {
          Ok(text.to_rust_string_lossy(tc))
        }
        _ => Err(PATH_URL),
      },
      _ => Err(PATH_URL),
    }
  }
}

fn own_names(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
) -> Result<Vec<String>, OpError> {
  enum Outcome {
    Names(v8::Global<v8::Array>),
    Throw(v8::Global<v8::Value>),
    Missing,
  }
  let outcome = {
    v8::tc_scope!(let tc, scope);
    let object = v8::Local::new(tc, object);
    let mut builder = v8::GetPropertyNamesArgsBuilder::new();
    let listed = builder
      .mode(v8::KeyCollectionMode::OwnOnly)
      .property_filter(
        v8::PropertyFilter::ONLY_ENUMERABLE | v8::PropertyFilter::SKIP_SYMBOLS,
      )
      .key_conversion(v8::KeyConversionMode::ConvertToString)
      .build();
    match object.get_own_property_names(tc, listed) {
      Some(names) => Outcome::Names(v8::Global::new(tc, names)),
      None => {
        if let Some(exception) = tc.exception() {
          Outcome::Throw(v8::Global::new(tc, exception))
        } else {
          Outcome::Missing
        }
      }
    }
  };
  let names = match outcome {
    Outcome::Names(names) => v8::Local::new(scope, names),
    Outcome::Throw(exception) => {
      return Err(OpError::thrown(scope, v8::Local::new(scope, exception)));
    }
    Outcome::Missing => return Err(OpError::type_(OPTIONS_READ)),
  };
  let length = names.length();
  let mut keys = Vec::with_capacity(length as usize);
  for index in 0..length {
    let value = array_element(scope, names, index)?;
    keys.push(string_primitive(scope, value, NAME_TYPE)?);
  }
  Ok(keys)
}

fn classify(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<(Target, usize), OpError> {
  if let Ok(buffer) = v8::Local::<v8::SharedArrayBuffer>::try_from(value) {
    let length = buffer.byte_length();
    if length > MAX_BUFFER {
      return Err(OpError::type_(BUFFER_TOO_LARGE));
    }
    return Ok((Target::Shared(v8::Global::new(scope, buffer)), length));
  }
  if let Ok(view) = v8::Local::<v8::ArrayBufferView>::try_from(value) {
    if view
      .buffer(scope)
      .is_none_or(|buffer| buffer.was_detached())
    {
      return Err(OpError::type_(DETACHED));
    }
    let length = view.byte_length();
    if length > MAX_BUFFER {
      return Err(OpError::type_(BUFFER_TOO_LARGE));
    }
    return Ok((Target::View(v8::Global::new(scope, view)), length));
  }
  if let Ok(buffer) = v8::Local::<v8::ArrayBuffer>::try_from(value) {
    if buffer.was_detached() {
      return Err(OpError::type_(DETACHED));
    }
    let length = buffer.byte_length();
    if length > MAX_BUFFER {
      return Err(OpError::type_(BUFFER_TOO_LARGE));
    }
    return Ok((Target::Buffer(v8::Global::new(scope, buffer)), length));
  }
  Err(OpError::type_(DATA))
}

fn copy_bytes(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<Vec<u8>, OpError> {
  if let Ok(buffer) = v8::Local::<v8::SharedArrayBuffer>::try_from(value) {
    return copy_shared(&buffer);
  }
  if let Ok(view) = v8::Local::<v8::ArrayBufferView>::try_from(value) {
    if view
      .buffer(scope)
      .is_none_or(|buffer| buffer.was_detached())
    {
      return Err(OpError::type_(DETACHED));
    }
    let length = view.byte_length();
    if length > MAX_BUFFER {
      return Err(OpError::type_(BUFFER_TOO_LARGE));
    }
    if length == 0 {
      return Ok(Vec::new());
    }
    let mut bytes = vec![0u8; length];
    if view.copy_contents(&mut bytes) != length {
      return Err(OpError::type_(DETACHED));
    }
    return Ok(bytes);
  }
  if let Ok(buffer) = v8::Local::<v8::ArrayBuffer>::try_from(value) {
    return copy_array(&buffer);
  }
  Err(OpError::type_(DATA))
}

fn copy_shared(buffer: &v8::SharedArrayBuffer) -> Result<Vec<u8>, OpError> {
  let length = buffer.byte_length();
  if length > MAX_BUFFER {
    return Err(OpError::type_(BUFFER_TOO_LARGE));
  }
  if length == 0 {
    return Ok(Vec::new());
  }
  let mut bytes = vec![0u8; length];
  let store = buffer.get_backing_store();
  let Some(data) = store.data() else {
    return Err(OpError::type_(DETACHED));
  };
  if store.byte_length() < length {
    return Err(OpError::type_(DETACHED));
  }
  // Safety: `length` fits the destination and the store. The shared
  // reference keeps the store alive for this copy.
  unsafe {
    std::ptr::copy_nonoverlapping(
      data.as_ptr().cast::<u8>(),
      bytes.as_mut_ptr(),
      length,
    );
  }
  Ok(bytes)
}

fn copy_array(buffer: &v8::ArrayBuffer) -> Result<Vec<u8>, OpError> {
  if buffer.was_detached() {
    return Err(OpError::type_(DETACHED));
  }
  let length = buffer.byte_length();
  if length > MAX_BUFFER {
    return Err(OpError::type_(BUFFER_TOO_LARGE));
  }
  if length == 0 {
    return Ok(Vec::new());
  }
  let mut bytes = vec![0u8; length];
  let Some(data) = buffer.data() else {
    return Err(OpError::type_(DETACHED));
  };
  // Safety: `length` is the buffer's byte length and the destination
  // size. `buffer` keeps the store alive for this copy.
  unsafe {
    std::ptr::copy_nonoverlapping(
      data.as_ptr().cast::<u8>(),
      bytes.as_mut_ptr(),
      length,
    );
  }
  Ok(bytes)
}

fn write_target(
  scope: &mut v8::PinScope,
  target: Target,
  bytes: &[u8],
) -> Result<(), ()> {
  match target {
    Target::View(global) => {
      let view = v8::Local::new(scope, &global);
      if view
        .buffer(scope)
        .is_none_or(|buffer| buffer.was_detached())
        || view.byte_length() < bytes.len()
      {
        return Err(());
      }
      if bytes.is_empty() {
        return Ok(());
      }
      let data = view.data();
      if data.is_null() {
        return Err(());
      }
      // Safety: `bytes` fits the view. `data()` includes the byte offset,
      // and `view` keeps the store alive for this copy.
      unsafe {
        std::ptr::copy_nonoverlapping(
          bytes.as_ptr(),
          data.cast::<u8>(),
          bytes.len(),
        );
      }
      Ok(())
    }
    Target::Buffer(global) => {
      let buffer = v8::Local::new(scope, &global);
      write_memory(
        buffer.was_detached(),
        buffer.byte_length(),
        buffer.data().map(std::ptr::NonNull::as_ptr),
        bytes,
      )
    }
    Target::Shared(global) => {
      let buffer = v8::Local::new(scope, &global);
      let store = buffer.get_backing_store();
      write_memory(
        false,
        buffer.byte_length(),
        store.data().map(std::ptr::NonNull::as_ptr),
        bytes,
      )
    }
  }
}

fn write_memory(
  detached: bool,
  length: usize,
  data: Option<*mut std::ffi::c_void>,
  bytes: &[u8],
) -> Result<(), ()> {
  if detached || length < bytes.len() {
    return Err(());
  }
  if bytes.is_empty() {
    return Ok(());
  }
  let Some(data) = data else {
    return Err(());
  };
  // Safety: `bytes` fits `length`, and the caller keeps the store alive.
  unsafe {
    std::ptr::copy_nonoverlapping(
      bytes.as_ptr(),
      data.cast::<u8>(),
      bytes.len(),
    );
  }
  Ok(())
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

fn install_streams(scope: &mut v8::PinScope) {
  let source = v8::String::new(scope, STREAMS).expect("child streams");
  let script = v8::Script::compile(scope, source, None).expect("child streams");
  script.run(scope).expect("child streams");
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
  mutex.lock().unwrap_or_else(|error| error.into_inner())
}

/// `stdin`, `stdout`, and `stderr` stay in JavaScript so this crate does not
/// depend on the stream implementation. A pull reads at most 16384 bytes.
const STREAMS: &str = r#"(function () {
  "use strict";
  const proto = globalThis.Yuzuko.Child.prototype;
  const stdinStreams = new WeakMap();
  const stdoutStreams = new WeakMap();
  const stderrStreams = new WeakMap();

  function requireChild(value) {
    if (!(value instanceof globalThis.Yuzuko.Child)) {
      throw new TypeError("illegal invocation");
    }
    return value;
  }

  function bytesOf(chunk) {
    if (chunk instanceof Uint8Array) {
      return chunk;
    }
    if (ArrayBuffer.isView(chunk)) {
      return new Uint8Array(chunk.buffer, chunk.byteOffset, chunk.byteLength);
    }
    if (chunk instanceof ArrayBuffer) {
      return new Uint8Array(chunk);
    }
    if (
      typeof SharedArrayBuffer === "function" &&
      chunk instanceof SharedArrayBuffer
    ) {
      return new Uint8Array(chunk);
    }
    throw new TypeError("The data must be a BufferSource.");
  }

  function installReadable(name, modeName, readName, streams) {
    Object.defineProperty(proto, name, {
      enumerable: true,
      configurable: true,
      get: function () {
        const self = requireChild(this);
        if (self[modeName] !== "piped") {
          return null;
        }
        const existing = streams.get(self);
        if (existing) {
          return existing;
        }
        const stream = new globalThis.ReadableStream({
          async pull(controller) {
            const buffer = new Uint8Array(16384);
            const n = await self[readName](buffer);
            if (n === null) {
              controller.close();
              return;
            }
            if (n === 0) {
              return;
            }
            controller.enqueue(buffer.subarray(0, n));
          },
        });
        streams.set(self, stream);
        return stream;
      },
    });
  }

  installReadable("stdout", "stdoutMode", "readStdout", stdoutStreams);
  installReadable("stderr", "stderrMode", "readStderr", stderrStreams);

  Object.defineProperty(proto, "stdin", {
    enumerable: true,
    configurable: true,
    get: function stdin() {
      const self = requireChild(this);
      if (self.stdinMode !== "piped") {
        return null;
      }
      const existing = stdinStreams.get(self);
      if (existing) {
        return existing;
      }
      const stream = new globalThis.WritableStream({
        async write(chunk) {
          const view = bytesOf(chunk);
          let offset = 0;
          while (offset < view.byteLength) {
            const slice = offset === 0 ? view : view.subarray(offset);
            const wrote = await self.writeStdin(slice);
            if (wrote === 0) {
              throw new TypeError("The write wrote no bytes.");
            }
            offset += wrote;
          }
        },
        close() {
          self.closeStdin();
        },
      });
      stdinStreams.set(self, stream);
      return stream;
    },
  });
})()"#;
