//! Process arguments, environment, current directory, and cooperative exit.
//!
//! These names are installed with the `Yuzuko` object in every realm that
//! receives it, including a build with no Cargo features. `Yuzuko.exit` does
//! not call `std::process::exit`. It records a status and stops each isolate
//! so the event loop can return that status.

use std::path::PathBuf;
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::{Mutex, MutexGuard, RwLock, RwLockReadGuard, RwLockWriteGuard};

use crate::op::OpException;

static EXIT: AtomicI32 = AtomicI32::new(-1);
static ARGS: Mutex<Vec<String>> = Mutex::new(Vec::new());
/// Shares environment reads and host lookup, and excludes changes.
///
/// `getaddrinfo` and `getenv` take the read side and may overlap. `setenv`,
/// `unsetenv`, and `chdir` take the write side.
static IO: RwLock<()> = RwLock::new(());

const EXIT_CODE: &str = "The exit code must be an integer from 0 through 255.";
const NAME_TYPE: &str = "The name must be a string.";
const NAME_EMPTY: &str = "The name must not be empty.";
const NAME_RESERVED: &str = "The name must not contain `=` or NUL.";
const VALUE_TYPE: &str = "The value must be a string.";
const VALUE_NUL: &str = "The value must not contain NUL.";
const PATH_TYPE: &str = "The path must be a string or a file URL.";
const PATH_SCHEME: &str = "The URL scheme must be file.";
const PATH_URL: &str = "The URL is invalid.";
const PATH_FILE: &str = "The file URL is invalid.";
const PATH_NUL: &str = "The path must not contain NUL.";
const CWD_FAILED: &str = "The current directory could not be read.";
const DIR_MISSING: &str = "The directory was not found.";
const DIR_KIND: &str = "The path is not a directory.";
const DIR_DENIED: &str = "The directory could not be changed.";

pub(crate) fn exit_code() -> Option<u8> {
  u8::try_from(EXIT.load(Ordering::SeqCst)).ok()
}

pub(crate) fn clear_exit() {
  EXIT.store(-1, Ordering::SeqCst);
}

/// Records `code` when no exit has been requested yet, then stops every
/// agent. A later call keeps the first status.
pub(crate) fn request_exit(code: u8) {
  if EXIT
    .compare_exchange(-1, i32::from(code), Ordering::SeqCst, Ordering::SeqCst)
    .is_ok()
  {
    crate::worker::interrupt_agents();
  }
}

/// Replaces the arguments the next realm install copies into `Yuzuko.args`.
pub fn set_args(args: Vec<String>) {
  *lock(&ARGS) = args;
}

pub(crate) fn install(scope: &mut v8::PinScope, yuzuko: v8::Local<v8::Object>) {
  install_args(scope, yuzuko);
  let env = v8::Object::new(scope);
  define_fn(scope, env, "get", 1, env_get);
  define_fn(scope, env, "set", 2, env_set);
  define_fn(scope, env, "delete", 1, env_delete);
  define_fn(scope, env, "has", 1, env_has);
  define_fn(scope, env, "toObject", 0, env_to_object);
  let env_key = v8::String::new(scope, "env").expect("env");
  yuzuko.set(scope, env_key.into(), env.into());
  define_fn(scope, yuzuko, "cwd", 0, cwd);
  define_fn(scope, yuzuko, "chdir", 1, chdir);
  define_fn(scope, yuzuko, "exit", 0, exit);
}

fn install_args(scope: &mut v8::PinScope, yuzuko: v8::Local<v8::Object>) {
  let args = lock(&ARGS).clone();
  let array = v8::Array::new(scope, args.len() as i32);
  for (index, arg) in args.iter().enumerate() {
    let Some(text) = v8::String::new(scope, arg) else {
      continue;
    };
    let _ = array.set_index(scope, index as u32, text.into());
  }
  let _ = array.set_integrity_level(scope, v8::IntegrityLevel::Frozen);
  let key = v8::String::new(scope, "args").expect("args");
  yuzuko.set(scope, key.into(), array.into());
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

fn exit(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut _ret: v8::ReturnValue,
) {
  let Ok(code) = exit_byte(scope, args.get(0)) else {
    crate::js::throw_type(scope, EXIT_CODE);
    return;
  };
  request_exit(code);
  scope.terminate_execution();
  // Termination is delivered at a JavaScript stack check. A host call does
  // not check, so the statement after `Yuzuko.exit` would still run. Entering
  // this function reaches a check before the callback returns. `try` / `catch`
  // does not observe that exception.
  if let Some(source) = v8::String::new(scope, "(function(){})()")
    && let Some(script) = v8::Script::compile(scope, source, None)
  {
    let _ = script.run(scope);
  }
}

fn exit_byte(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<u8, ()> {
  if value.is_undefined() {
    return Ok(0);
  }
  if !value.is_number() {
    return Err(());
  }
  let Some(code) = value.number_value(scope) else {
    return Err(());
  };
  if !code.is_finite() || !(0.0..=255.0).contains(&code) || code.fract() != 0.0
  {
    return Err(());
  }
  // `-0` compares equal to `0`.
  Ok(code as u8)
}

fn env_get(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut ret: v8::ReturnValue,
) {
  let Some(name) = env_name(scope, args.get(0)) else {
    return;
  };
  if let Err(error) = crate::permissions::check_env(&name) {
    error.throw(scope);
    return;
  }
  let value = {
    let _guard = io_read();
    std::env::var_os(&name).map(|value| value.to_string_lossy().into_owned())
  };
  let Some(value) = value else {
    return;
  };
  set_string(scope, &mut ret, &value);
}

fn env_set(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut _ret: v8::ReturnValue,
) {
  let Some(name) = env_name(scope, args.get(0)) else {
    return;
  };
  let Some(value) = env_value(scope, args.get(1)) else {
    return;
  };
  if let Err(error) = crate::permissions::check_env(&name) {
    error.throw(scope);
    return;
  }
  let _guard = io_write();
  // SAFETY: `name` is not empty and contains neither `=` nor NUL, and
  // `value` contains no NUL, so these calls do not panic on those inputs.
  // The write lock keeps this `setenv` off `getaddrinfo` and off another
  // change. A lookup or an environment read may overlap another read.
  // Rust documents that `set_var` can race with a thread that does not
  // take this lock; this does not stop that thread.
  unsafe {
    std::env::set_var(&name, &value);
  }
}

fn env_delete(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut _ret: v8::ReturnValue,
) {
  let Some(name) = env_name(scope, args.get(0)) else {
    return;
  };
  if let Err(error) = crate::permissions::check_env(&name) {
    error.throw(scope);
    return;
  }
  let _guard = io_write();
  // SAFETY: see [`env_set`]. A missing name is a successful removal.
  unsafe {
    std::env::remove_var(&name);
  }
}

fn env_has(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut ret: v8::ReturnValue,
) {
  let Some(name) = env_name(scope, args.get(0)) else {
    return;
  };
  if let Err(error) = crate::permissions::check_env(&name) {
    error.throw(scope);
    return;
  }
  let present = {
    let _guard = io_read();
    std::env::var_os(&name).is_some()
  };
  ret.set(v8::Boolean::new(scope, present).into());
}

fn env_to_object(
  scope: &mut v8::PinScope,
  _args: v8::FunctionCallbackArguments,
  mut ret: v8::ReturnValue,
) {
  if let Err(error) = crate::permissions::check_env_all() {
    error.throw(scope);
    return;
  }
  let pairs = {
    let _guard = io_read();
    std::env::vars_os()
      .map(|(key, value)| {
        (
          key.to_string_lossy().into_owned(),
          value.to_string_lossy().into_owned(),
        )
      })
      .collect::<Vec<_>>()
  };
  let object = v8::Object::new(scope);
  for (key, value) in pairs {
    let Some(key) = v8::String::new(scope, &key) else {
      throw_dom(scope, "OperationError", "The value could not be read.");
      return;
    };
    let Some(value) = v8::String::new(scope, &value) else {
      throw_dom(scope, "OperationError", "The value could not be read.");
      return;
    };
    // `define_own_property` keeps a key named `__proto__` as an own data
    // property. `Object::set` would change the prototype instead.
    let _ = object.define_own_property(
      scope,
      key.into(),
      value.into(),
      v8::PropertyAttribute::NONE,
    );
  }
  ret.set(object.into());
}

fn env_name(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Option<String> {
  let Some(name) = string_primitive(scope, value) else {
    crate::js::throw_type(scope, NAME_TYPE);
    return None;
  };
  if name.is_empty() {
    crate::js::throw_type(scope, NAME_EMPTY);
    return None;
  }
  if name.as_bytes().contains(&b'=') || name.as_bytes().contains(&0) {
    crate::js::throw_type(scope, NAME_RESERVED);
    return None;
  }
  Some(name)
}

fn env_value(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Option<String> {
  let Some(value) = string_primitive(scope, value) else {
    crate::js::throw_type(scope, VALUE_TYPE);
    return None;
  };
  if value.as_bytes().contains(&0) {
    crate::js::throw_type(scope, VALUE_NUL);
    return None;
  }
  Some(value)
}

fn string_primitive(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Option<String> {
  if !value.is_string() {
    return None;
  }
  value
    .to_string(scope)
    .map(|text| text.to_rust_string_lossy(scope))
}

fn cwd(
  scope: &mut v8::PinScope,
  _args: v8::FunctionCallbackArguments,
  mut ret: v8::ReturnValue,
) {
  let path = {
    let _guard = io_read();
    std::env::current_dir()
  };
  let Ok(path) = path else {
    throw_dom(scope, "OperationError", CWD_FAILED);
    return;
  };
  set_string(scope, &mut ret, &path.to_string_lossy());
}

fn set_string(scope: &mut v8::PinScope, ret: &mut v8::ReturnValue, text: &str) {
  let Some(text) = v8::String::new(scope, text) else {
    throw_dom(scope, "OperationError", "The value could not be read.");
    return;
  };
  ret.set(text.into());
}

fn chdir(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut _ret: v8::ReturnValue,
) {
  let path = match parse_directory(scope, args.get(0)) {
    Ok(path) => path,
    Err(message) => {
      crate::js::throw_type(scope, message);
      return;
    }
  };
  if path.as_os_str().is_empty() {
    return;
  }
  if path.as_os_str().as_encoded_bytes().contains(&0) {
    crate::js::throw_type(scope, PATH_NUL);
    return;
  }
  // Join a relative path before the check. The same absolute path is the
  // directory this call sets, so a later change cannot retarget it.
  let path = match crate::permissions::anchor_path(&path) {
    Ok(path) => path,
    Err(error) => {
      error.throw(scope);
      return;
    }
  };
  if let Err(error) = crate::permissions::check_read(&path) {
    error.throw(scope);
    return;
  }
  let guard = io_write();
  match std::fs::metadata(&path) {
    Ok(info) if info.is_dir() => {}
    Ok(_) => {
      drop(guard);
      throw_dom(scope, "InvalidStateError", DIR_KIND);
      return;
    }
    Err(error) => {
      let (name, message) = directory_error(&error);
      drop(guard);
      throw_dom(scope, name, message);
      return;
    }
  }
  if let Err(error) = std::env::set_current_dir(&path) {
    let (name, message) = directory_error(&error);
    drop(guard);
    throw_dom(scope, name, message);
  }
}

fn directory_error(error: &std::io::Error) -> (&'static str, &'static str) {
  match error.kind() {
    std::io::ErrorKind::NotFound => ("NotFoundError", DIR_MISSING),
    std::io::ErrorKind::NotADirectory => ("InvalidStateError", DIR_KIND),
    std::io::ErrorKind::PermissionDenied => ("NotAllowedError", DIR_DENIED),
    _ => ("OperationError", DIR_DENIED),
  }
}

fn parse_directory(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<PathBuf, &'static str> {
  if value.is_string() {
    let Some(text) = value.to_string(scope) else {
      return Err(PATH_TYPE);
    };
    return Ok(PathBuf::from(text.to_rust_string_lossy(scope)));
  }
  if is_url(scope, value) {
    let object =
      v8::Local::<v8::Object>::try_from(value).map_err(|_| PATH_URL)?;
    let href = href_of(scope, object)?;
    let url = url::Url::parse(&href).map_err(|_| PATH_URL)?;
    if url.scheme() != "file" {
      return Err(PATH_SCHEME);
    }
    return url.to_file_path().map_err(|_| PATH_FILE);
  }
  Err(PATH_TYPE)
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

fn throw_dom(scope: &mut v8::PinScope, name: &str, message: &str) {
  if let Some(error) = crate::op::new_dom_exception(scope, name, message) {
    scope.throw_exception(error.into());
    return;
  }
  crate::js::throw_type(scope, &format!("{name}: {message}"));
}

fn io_read() -> RwLockReadGuard<'static, ()> {
  IO.read().unwrap_or_else(|poison| poison.into_inner())
}

fn io_write() -> RwLockWriteGuard<'static, ()> {
  IO.write().unwrap_or_else(|poison| poison.into_inner())
}

/// Runs `body` while no caller is inside `setenv`, `unsetenv`, or `chdir`.
///
/// `getaddrinfo` is not safe beside those libc calls. Lookups and
/// environment reads share the read side and may overlap. `Yuzuko.env.set`,
/// `delete`, and `chdir` take the write side.
pub fn with_env<T>(body: impl FnOnce() -> T) -> T {
  let _guard = io_read();
  body()
}

fn lock<T>(mutex: &'static Mutex<T>) -> MutexGuard<'static, T> {
  mutex.lock().unwrap_or_else(|poison| poison.into_inner())
}

#[cfg(test)]
mod tests {
  use super::EXIT_CODE;

  fn logs(source: &str) -> Vec<String> {
    crate::run_scripts(&[source]).expect("script")
  }

  fn exited(source: &str) -> u8 {
    let error = crate::run_scripts(&[source]).expect_err("exit");
    error.exit_code.expect("exit status")
  }

  #[test]
  fn host_lookups_overlap() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::time::{Duration, Instant};

    let entered = Arc::new(AtomicUsize::new(0));
    let release = Arc::new(AtomicBool::new(false));
    let mut threads = Vec::new();
    for _ in 0..2 {
      let entered = Arc::clone(&entered);
      let release = Arc::clone(&release);
      threads.push(std::thread::spawn(move || {
        super::with_env(|| {
          entered.fetch_add(1, Ordering::SeqCst);
          let start = Instant::now();
          while !release.load(Ordering::SeqCst) {
            if start.elapsed() > Duration::from_secs(2) {
              break;
            }
            std::thread::sleep(Duration::from_millis(5));
          }
        });
      }));
    }
    let start = Instant::now();
    while entered.load(Ordering::SeqCst) < 2
      && start.elapsed() < Duration::from_secs(1)
    {
      std::thread::sleep(Duration::from_millis(5));
    }
    let overlapped = entered.load(Ordering::SeqCst) == 2;
    release.store(true, Ordering::SeqCst);
    for thread in threads {
      thread.join().unwrap();
    }
    assert!(overlapped);
  }

  #[test]
  fn exit_requests_the_status() {
    assert_eq!(exited("Yuzuko.exit(3)"), 3);
    assert_eq!(exited("Yuzuko.exit()"), 0);
    assert_eq!(exited("Yuzuko.exit(-0)"), 0);
    assert_eq!(exited("Yuzuko.exit(undefined)"), 0);
    assert_eq!(exited("Yuzuko.exit(255)"), 255);
    assert_eq!(exited("Yuzuko.exit(2, 'no')"), 2);
  }

  #[test]
  fn exit_is_not_caught() {
    let error = crate::run_scripts(&[r#"
      try {
        Yuzuko.exit(4);
        console.log("still");
      } catch (error) {
        console.log("caught");
      }
      console.log("after");
      "#])
    .expect_err("exit");
    assert_eq!(error.exit_code, Some(4));
    assert!(error.message.is_empty(), "{}", error.message);
  }

  #[test]
  fn bad_exit_codes_do_not_stop_the_process() {
    let logged = logs(
      r#"
      const samples = [1.5, -1, 256, "1", NaN, Infinity, null, true, false, Object(1), new Number(1)];
      for (const sample of samples) {
        try {
          Yuzuko.exit(sample);
          console.log("exited");
        } catch (error) {
          console.log(error.name + ":" + error.message);
        }
      }
      console.log("stayed");
      "#,
    );
    let expected = format!("TypeError:{EXIT_CODE}");
    assert_eq!(
      logged
        .iter()
        .filter(|line| line.as_str() == expected)
        .count(),
      11,
      "{logged:?}"
    );
    assert_eq!(logged.last().map(String::as_str), Some("stayed"));
    assert!(!logged.iter().any(|line| line == "exited"));
  }

  #[test]
  fn exit_stops_a_live_interval() {
    let error = crate::run_scripts(&[r#"
      setInterval(() => {}, 1000);
      setTimeout(() => Yuzuko.exit(7), 20);
      "#])
    .expect_err("exit");
    assert_eq!(error.exit_code, Some(7));
  }

  #[test]
  fn args_array_is_frozen_and_writable() {
    let logged = logs(
      r#"
      "use strict";
      const original = Yuzuko.args;
      try {
        Yuzuko.args.push("no");
        console.log("pushed");
      } catch (error) {
        console.log(error.name);
      }
      console.log(String(Object.isFrozen(Yuzuko.args)));
      Yuzuko.args = ["x"];
      console.log(String(Yuzuko.args[0]));
      console.log(String(original.length));
      "#,
    );
    assert_eq!(logged, vec!["TypeError", "true", "x", "0"]);
  }

  #[test]
  fn function_lengths() {
    let logged = logs(
      r#"
      console.log([
        Yuzuko.env.get.length,
        Yuzuko.env.set.length,
        Yuzuko.env.delete.length,
        Yuzuko.env.has.length,
        Yuzuko.env.toObject.length,
        Yuzuko.cwd.length,
        Yuzuko.chdir.length,
        Yuzuko.exit.length,
      ].join(","));
      "#,
    );
    assert_eq!(logged, vec!["1,2,1,1,0,0,1,0"]);
  }

  #[test]
  fn env_rejects_names_and_values_before_mutation() {
    let logged = logs(
      r#"
      function check(fn) {
        try {
          fn();
          console.log("ok");
        } catch (error) {
          console.log(error.message);
        }
      }
      check(() => Yuzuko.env.get(1));
      check(() => Yuzuko.env.get(""));
      check(() => Yuzuko.env.get("A=B"));
      check(() => Yuzuko.env.get("A\0B"));
      check(() => Yuzuko.env.set("Name", 1));
      check(() => Yuzuko.env.set("Name", "a\0b"));
      check(() => Yuzuko.env.has(null));
      check(() => Yuzuko.env.delete(false));
      "#,
    );
    assert_eq!(
      logged,
      vec![
        "The name must be a string.",
        "The name must not be empty.",
        "The name must not contain `=` or NUL.",
        "The name must not contain `=` or NUL.",
        "The value must be a string.",
        "The value must not contain NUL.",
        "The name must be a string.",
        "The name must be a string.",
      ]
    );
  }
}
