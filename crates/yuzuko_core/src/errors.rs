//! `Yuzuko.errors` classes.
//!
//! Every build installs the same constructors on the main realm and in a
//! dedicated worker. File-system, socket, and HTTP code construct from those
//! captured functions. Replacing a property later does not change the thrown
//! object.

use std::cell::RefCell;
use std::collections::HashMap;

use crate::op::{Js, OpError};
#[cfg(any(feature = "tcp", feature = "udp", feature = "http"))]
use crate::socket_error::Class;

thread_local! {
  static CONSTRUCTORS: RefCell<HashMap<&'static str, v8::Global<v8::Function>>> =
    RefCell::new(HashMap::new());
}

/// Insertion order of `Yuzuko.errors`. `NotCapable` is last, as in Deno.
pub(crate) const CLASSES: &[&str] = &[
  "NotFound",
  "PermissionDenied",
  "ConnectionRefused",
  "ConnectionReset",
  "ConnectionAborted",
  "NotConnected",
  "AddrInUse",
  "AddrNotAvailable",
  "BrokenPipe",
  "AlreadyExists",
  "InvalidData",
  "TimedOut",
  "Interrupted",
  "WriteZero",
  "WouldBlock",
  "UnexpectedEof",
  "BadResource",
  "Http",
  "Busy",
  "NotSupported",
  "FilesystemLoop",
  "IsADirectory",
  "NetworkUnreachable",
  "NotADirectory",
  "NotCapable",
];

const FACTORY: &str = r#"(function (name) {
  const ctor = class extends Error {
    constructor(message, options) {
      super(message, options);
      this.name = name;
    }
  };
  Object.defineProperty(ctor, "name", { value: name, configurable: true });
  return ctor;
})"#;

pub(crate) fn install(scope: &mut v8::PinScope) {
  let factory = factory(scope);
  let mut built = Vec::with_capacity(CLASSES.len());
  let object = v8::Object::new(scope);
  for name in CLASSES {
    let ctor = make_class(scope, factory, name);
    let key = v8::String::new(scope, name).expect("error class");
    object.set(scope, key.into(), ctor.into());
    built.push((*name, v8::Global::new(scope, ctor)));
  }
  CONSTRUCTORS.with(|map| {
    let mut map = map.borrow_mut();
    map.clear();
    map.extend(built);
  });
  let yuzuko = crate::namespace(scope, "Yuzuko");
  let errors_key = v8::String::new(scope, "errors").expect("errors");
  yuzuko.set(scope, errors_key.into(), object.into());
}

pub(crate) fn clear() {
  CONSTRUCTORS.with(|map| map.borrow_mut().clear());
}

/// The constructor captured at realm start. A missing constructor becomes a
/// plain `Error` whose `name` is `name`.
pub(crate) fn construct<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  name: &str,
  message: &str,
) -> v8::Local<'s, v8::Value> {
  let ctor = CONSTRUCTORS.with(|map| map.borrow().get(name).cloned());
  if let Some(ctor) = ctor {
    let ctor = v8::Local::new(scope, ctor);
    if let Some(text) = v8::String::new(scope, message) {
      let instance = {
        v8::tc_scope!(let tc, scope);
        ctor.new_instance(tc, &[text.into()])
      };
      if let Some(object) = instance {
        return object.into();
      }
    }
  }
  named_error(scope, name, message)
}

#[cfg_attr(
  not(any(feature = "tcp", feature = "udp", feature = "http")),
  allow(dead_code)
)]
pub(crate) fn exception(
  scope: &mut v8::PinScope,
  name: &str,
  message: &str,
) -> OpError {
  let value = construct(scope, name, message);
  OpError::thrown(scope, value)
}

#[cfg_attr(
  not(any(feature = "tcp", feature = "udp", feature = "http")),
  allow(dead_code)
)]
pub(crate) fn reject(
  scope: &mut v8::PinScope,
  name: &str,
  message: &str,
  no_promise: &str,
) -> Result<Js, OpError> {
  let Some(resolver) = v8::PromiseResolver::new(scope) else {
    return Err(OpError::type_(no_promise));
  };
  let error = construct(scope, name, message);
  let _ = resolver.reject(scope, error);
  let promise: v8::Local<v8::Value> = resolver.get_promise(scope).into();
  Ok(Js(v8::Global::new(scope, promise)))
}

/// The constructor captured at realm start, or a plain `Error` named
/// `NotCapable` when that constructor is gone.
pub(crate) fn new_not_capable<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  message: &str,
) -> Option<v8::Local<'s, v8::Object>> {
  let value = construct(scope, "NotCapable", message);
  v8::Local::<v8::Object>::try_from(value).ok()
}

#[cfg(any(feature = "tcp", feature = "udp", feature = "http"))]
pub(crate) fn class_value<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  class: Class,
  code: Option<&str>,
) -> v8::Local<'s, v8::Value> {
  let value = construct(scope, class.name(), class.message());
  attach_code(scope, value, code);
  value
}

#[cfg(any(feature = "tcp", feature = "udp"))]
pub(crate) fn denied_value<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  name: &str,
  message: &str,
) -> v8::Local<'s, v8::Value> {
  if name == "NotCapable" {
    return new_not_capable(scope, message)
      .map(v8::Local::<v8::Value>::from)
      .unwrap_or_else(|| v8::undefined(scope).into());
  }
  crate::op::new_dom_exception(scope, name, message)
    .map(v8::Local::<v8::Value>::from)
    .unwrap_or_else(|| named_error(scope, name, message))
}

/// A synchronous socket or HTTP failure. Classified kinds use the socket
/// class and keep `generic` only when the kind has no socket class.
/// `InvalidInput` is a `TypeError`. `code` is set when Deno would set it.
#[cfg(any(feature = "tcp", feature = "udp", feature = "http"))]
pub(crate) fn io_exception(
  scope: &mut v8::PinScope,
  error: &std::io::Error,
  generic: &str,
) -> OpError {
  let value = match crate::socket_error::map_io(error) {
    crate::socket_error::Mapped::Class { class, code } => {
      class_value(scope, class, code)
    }
    crate::socket_error::Mapped::Generic { code, type_error } => {
      coded_message(scope, generic, code, type_error)
    }
  };
  OpError::thrown(scope, value)
}

/// An operating-system failure whose message is fixed. The class and `code`
/// still follow Deno's `std::io::Error` mapping.
#[cfg_attr(not(feature = "child"), allow(dead_code))]
pub(crate) fn io_exception_message(
  scope: &mut v8::PinScope,
  error: &std::io::Error,
  message: &str,
) -> OpError {
  let class = crate::error_codes::io_class_name(error);
  let code = crate::error_codes::get_error_code(error);
  let value = if class == "TypeError" {
    coded_message(scope, message, code, true)
  } else if class == "Error" {
    coded_message(scope, message, code, false)
  } else {
    let value = construct(scope, class, message);
    attach_code(scope, value, code);
    value
  };
  OpError::thrown(scope, value)
}

/// Sets `code` when it is not already present, the same way Deno copies an
/// additional property onto a custom error.
#[cfg_attr(
  not(any(
    feature = "fs",
    feature = "child",
    feature = "tcp",
    feature = "udp",
    feature = "http",
  )),
  allow(dead_code)
)]
pub(crate) fn attach_code(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
  code: Option<&str>,
) {
  let Some(code) = code else {
    return;
  };
  let Ok(object) = v8::Local::<v8::Object>::try_from(value) else {
    return;
  };
  let Some(key) = v8::String::new(scope, "code") else {
    return;
  };
  if object.has(scope, key.into()).unwrap_or(false) {
    return;
  }
  let Some(text) = v8::String::new(scope, code) else {
    return;
  };
  object.set(scope, key.into(), text.into());
}

#[cfg_attr(
  not(any(
    feature = "child",
    feature = "tcp",
    feature = "udp",
    feature = "http",
  )),
  allow(dead_code)
)]
pub(crate) fn coded_message<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  message: &str,
  code: Option<&str>,
  type_error: bool,
) -> v8::Local<'s, v8::Value> {
  let Some(text) = v8::String::new(scope, message) else {
    return v8::undefined(scope).into();
  };
  let error = if type_error {
    v8::Exception::type_error(scope, text)
  } else {
    v8::Exception::error(scope, text)
  };
  attach_code(scope, error, code);
  error
}

fn factory<'s>(
  scope: &mut v8::PinScope<'s, '_>,
) -> v8::Local<'s, v8::Function> {
  let source = v8::String::new(scope, FACTORY).expect("error class");
  let script = v8::Script::compile(scope, source, None).expect("error class");
  let value = script.run(scope).expect("error class");
  v8::Local::<v8::Function>::try_from(value).expect("error class")
}

fn make_class<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  factory: v8::Local<v8::Function>,
  name: &str,
) -> v8::Local<'s, v8::Function> {
  let name = v8::String::new(scope, name).expect("error class");
  let receiver = v8::undefined(scope).into();
  let value = factory
    .call(scope, receiver, &[name.into()])
    .expect("error class");
  v8::Local::<v8::Function>::try_from(value).expect("error class")
}

fn named_error<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  name: &str,
  message: &str,
) -> v8::Local<'s, v8::Value> {
  let Some(text) = v8::String::new(scope, message) else {
    return v8::undefined(scope).into();
  };
  let error = v8::Exception::error(scope, text);
  let Ok(object) = v8::Local::<v8::Object>::try_from(error) else {
    return error;
  };
  let Some(key) = v8::String::new(scope, "name") else {
    return object.into();
  };
  let Some(value) = v8::String::new(scope, name) else {
    return object.into();
  };
  object.set(scope, key.into(), value.into());
  object.into()
}

#[cfg(all(test, not(feature = "fs")))]
#[test]
fn minimal_realm_has_identity_and_not_capable() {
  let lines = crate::run_scripts(&[r#"
      console.log([
        typeof Yuzuko.execPath,
        typeof Yuzuko.pid,
        typeof Yuzuko.ppid,
        typeof Yuzuko.version,
        typeof Yuzuko.stdin,
        typeof Yuzuko.stdout,
        typeof Yuzuko.stderr,
        typeof Yuzuko.errors.NotCapable,
        typeof Yuzuko.errors.NotFound,
        typeof Yuzuko.memoryUsage,
        typeof Yuzuko.noColor,
        typeof Yuzuko.refTimer,
        typeof Yuzuko.unrefTimer,
      ].join(" "));
      const version = Yuzuko.version;
      console.log(
        typeof version.yuzuko === "string" && version.yuzuko.length > 0 &&
          typeof version.v8 === "string" && version.v8.length > 0 &&
          !Object.prototype.hasOwnProperty.call(version, "typescript")
          ? "version"
          : "bad-version",
      );
      console.log(
        Yuzuko.errors.NotCapable.prototype instanceof Error ? "error" : "not-error",
      );
      const path = Yuzuko.execPath();
      const absolute = path.startsWith("/") || path.startsWith("\\\\") ||
        /^[A-Za-z]:[\\/]/.test(path);
      console.log(absolute ? "absolute" : "relative");
      console.log(
        Number.isInteger(Yuzuko.pid) && Yuzuko.pid > 0 &&
          Number.isInteger(Yuzuko.ppid) && Yuzuko.ppid > 0
          ? "ids"
          : "bad-ids",
      );
      const before = Yuzuko.memoryUsage();
      const buffer = new ArrayBuffer(8 * 1024 * 1024);
      const after = Yuzuko.memoryUsage();
      const keys = Object.keys(before);
      const finite = [before, after].every((sample) =>
        keys.every((key) => Number.isFinite(sample[key]) && sample[key] >= 0)
      );
      console.log(
        keys.join(",") === "rss,heapTotal,heapUsed,external" &&
          before.rss > 0 &&
          finite &&
          (after.heapUsed > before.heapUsed || after.external > before.external) &&
          before !== Yuzuko.memoryUsage() &&
          buffer.byteLength === 8 * 1024 * 1024
          ? "usage"
          : "bad-usage",
      );
      console.log(typeof Yuzuko.noColor === "boolean" ? "color" : "bad-color");
      Yuzuko.unrefTimer(-1);
      Yuzuko.unrefTimer(0);
      Yuzuko.refTimer(123456);
      console.log(
        typeof Yuzuko.refTimer === "function" &&
          typeof Yuzuko.unrefTimer === "function" &&
          Yuzuko.refTimer.length === 1 &&
          Yuzuko.unrefTimer.length === 1
          ? "timers"
          : "bad-timers",
      );
    "#])
  .expect("script");
  assert_eq!(
    lines,
    vec![
      "function number number object undefined undefined undefined function function function boolean function function",
      "version",
      "error",
      "absolute",
      "ids",
      "usage",
      "color",
      "timers",
    ],
    "{lines:?}"
  );
}

#[cfg(test)]
#[test]
fn error_classes_extend_error() {
  let lines = crate::run_scripts(&[r#"
      const names = [
        "NotFound",
        "PermissionDenied",
        "ConnectionRefused",
        "ConnectionReset",
        "ConnectionAborted",
        "NotConnected",
        "AddrInUse",
        "AddrNotAvailable",
        "BrokenPipe",
        "AlreadyExists",
        "InvalidData",
        "TimedOut",
        "Interrupted",
        "WriteZero",
        "WouldBlock",
        "UnexpectedEof",
        "BadResource",
        "Http",
        "Busy",
        "NotSupported",
        "FilesystemLoop",
        "IsADirectory",
        "NetworkUnreachable",
        "NotADirectory",
        "NotCapable",
      ];
      const order = Object.keys(Yuzuko.errors).join(",");
      const ready = names.every((name) => {
        const ctor = Yuzuko.errors[name];
        const error = new ctor("m", { cause: "c" });
        const empty = new ctor();
        const blank = new ctor(undefined);
        const nameDescriptor = Object.getOwnPropertyDescriptor(ctor, "name");
        return typeof ctor === "function" &&
          ctor.name === name &&
          ctor.length === 2 &&
          nameDescriptor.configurable === true &&
          nameDescriptor.writable === false &&
          error instanceof ctor &&
          error instanceof Error &&
          (typeof DOMException !== "function" || !(error instanceof DOMException)) &&
          error.name === name &&
          error.message === "m" &&
          error.cause === "c" &&
          !("code" in error) &&
          empty.message === "" &&
          blank.message === "";
      });
      console.log(ready && order === names.join(",") ? "classes" : "bad-classes");
      const captured = Yuzuko.errors.NotCapable;
      Yuzuko.errors.NotCapable = class extends Error {};
      const replaced = new Yuzuko.errors.NotCapable("later");
      const thrown = new captured("kept");
      console.log(
        thrown instanceof captured &&
          !(thrown instanceof Yuzuko.errors.NotCapable) &&
          replaced instanceof Yuzuko.errors.NotCapable
          ? "captured"
          : "replaced",
      );
    "#])
  .expect("script");
  assert_eq!(lines, vec!["classes", "captured"], "{lines:?}");
}

#[cfg(all(test, feature = "fs"))]
#[test]
fn missing_file_is_not_found_with_enoent() {
  let lines = crate::run_scripts(&[r#"
      let name = "missing";
      let code = "missing";
      let hit = "other";
      try {
        Yuzuko.readFileSync("/no/such/yuzuko-error-code");
      } catch (error) {
        name = error.name;
        code = error.code === undefined ? "none" : String(error.code);
        hit = error instanceof Yuzuko.errors.NotFound ? "class" : "other";
      }
      console.log(hit);
      console.log(name);
      console.log(code);
    "#])
  .expect("script");
  assert_eq!(lines, vec!["class", "NotFound", "ENOENT"], "{lines:?}");
}
