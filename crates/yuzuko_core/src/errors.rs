//! `Yuzuko.errors` classes that exist outside the file-system feature.
//!
//! `NotCapable` is installed in every build. Socket classes are installed
//! when `tcp` or `udp` is enabled. File-system classes stay on the `fs`
//! feature and publish their own object first; this module adds properties
//! to that object instead of replacing it.

use std::cell::RefCell;
#[cfg(any(feature = "tcp", feature = "udp"))]
use std::collections::HashMap;
#[cfg(any(feature = "tcp", feature = "udp"))]
use std::io::ErrorKind;

#[cfg(any(feature = "tcp", feature = "udp"))]
use crate::op::OpError;
#[cfg(any(feature = "tcp", feature = "udp"))]
use crate::socket_error::{self, Class};

thread_local! {
  static NOT_CAPABLE: RefCell<Option<v8::Global<v8::Function>>> =
    const { RefCell::new(None) };
}

#[cfg(any(feature = "tcp", feature = "udp"))]
thread_local! {
  static SOCKETS: RefCell<HashMap<Class, v8::Global<v8::Function>>> =
    RefCell::new(HashMap::new());
}

const FACTORY: &str = r#"(function (name) {
  const ctor = class extends Error {
    constructor(message) {
      super(message === undefined ? "" : String(message));
      this.name = name;
    }
  };
  Object.defineProperty(ctor, "name", { value: name });
  return ctor;
})"#;

pub(crate) fn install(scope: &mut v8::PinScope) {
  let factory = factory(scope);
  let not_capable = make_class(scope, factory, "NotCapable");
  NOT_CAPABLE.with(|slot| {
    *slot.borrow_mut() = Some(v8::Global::new(scope, not_capable));
  });
  let yuzuko = crate::namespace(scope, "Yuzuko");
  let errors_key = v8::String::new(scope, "errors").expect("errors");
  let object = match yuzuko.get(scope, errors_key.into()) {
    Some(value) => match v8::Local::<v8::Object>::try_from(value) {
      Ok(object) => object,
      Err(_) => v8::Object::new(scope),
    },
    None => v8::Object::new(scope),
  };
  let key = v8::String::new(scope, "NotCapable").expect("NotCapable");
  object.set(scope, key.into(), not_capable.into());
  #[cfg(any(feature = "tcp", feature = "udp"))]
  install_socket_classes(scope, factory, object);
  yuzuko.set(scope, errors_key.into(), object.into());
}

pub(crate) fn clear() {
  NOT_CAPABLE.with(|slot| *slot.borrow_mut() = None);
  #[cfg(any(feature = "tcp", feature = "udp"))]
  SOCKETS.with(|map| map.borrow_mut().clear());
}

/// The constructor captured at realm start, or a plain `Error` named
/// `NotCapable` when that constructor is gone.
pub(crate) fn new_not_capable<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  message: &str,
) -> Option<v8::Local<'s, v8::Object>> {
  let ctor = NOT_CAPABLE.with(|slot| slot.borrow().clone());
  if let Some(ctor) = ctor {
    let ctor = v8::Local::new(scope, ctor);
    if let Some(text) = v8::String::new(scope, message)
      && let Some(object) = ctor.new_instance(scope, &[text.into()])
    {
      return Some(object);
    }
  }
  named_object(scope, "NotCapable", message)
}

#[cfg(any(feature = "tcp", feature = "udp"))]
pub(crate) fn class_value<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  class: Class,
) -> v8::Local<'s, v8::Value> {
  let message = class.message();
  let ctor = SOCKETS.with(|map| map.borrow().get(&class).cloned());
  let Some(ctor) = ctor else {
    return named_error(scope, class.name(), message);
  };
  let ctor = v8::Local::new(scope, ctor);
  let Some(text) = v8::String::new(scope, message) else {
    return v8::undefined(scope).into();
  };
  let instance = {
    v8::tc_scope!(let tc, scope);
    ctor.new_instance(tc, &[text.into()])
  };
  match instance {
    Some(object) => object.into(),
    None => named_error(scope, class.name(), message),
  }
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

/// A synchronous socket failure. Classified kinds use the socket class.
/// Anything else is a plain `Error` with `generic`.
#[cfg(any(feature = "tcp", feature = "udp"))]
pub(crate) fn io_exception(
  scope: &mut v8::PinScope,
  kind: ErrorKind,
  generic: &str,
) -> OpError {
  if let Some(class) = socket_error::classify(kind) {
    let value = class_value(scope, class);
    return OpError::thrown(scope, value);
  }
  let Some(text) = v8::String::new(scope, generic) else {
    return OpError::type_(generic);
  };
  let error = v8::Exception::error(scope, text);
  OpError::thrown(scope, error)
}

#[cfg(any(feature = "tcp", feature = "udp"))]
fn install_socket_classes(
  scope: &mut v8::PinScope,
  factory: v8::Local<v8::Function>,
  object: v8::Local<v8::Object>,
) {
  let mut built = Vec::with_capacity(Class::INSTALLED.len());
  for class in Class::INSTALLED {
    let ctor = make_class(scope, factory, class.name());
    let key = v8::String::new(scope, class.name()).expect("socket error");
    object.set(scope, key.into(), ctor.into());
    built.push((class, v8::Global::new(scope, ctor)));
  }
  SOCKETS.with(|map| {
    let mut map = map.borrow_mut();
    map.clear();
    map.extend(built);
  });
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

fn named_object<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  name: &str,
  message: &str,
) -> Option<v8::Local<'s, v8::Object>> {
  let text = v8::String::new(scope, message)?;
  let error = v8::Exception::error(scope, text);
  let Ok(object) = v8::Local::<v8::Object>::try_from(error) else {
    return None;
  };
  let key = v8::String::new(scope, "name")?;
  let value = v8::String::new(scope, name)?;
  object.set(scope, key.into(), value.into());
  Some(object)
}

#[cfg(any(feature = "tcp", feature = "udp"))]
fn named_error<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  name: &str,
  message: &str,
) -> v8::Local<'s, v8::Value> {
  named_object(scope, name, message)
    .map(v8::Local::<v8::Value>::from)
    .unwrap_or_else(|| v8::undefined(scope).into())
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
      "function number number object undefined undefined undefined function undefined function boolean function function",
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
