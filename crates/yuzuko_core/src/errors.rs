//! `Yuzuko.errors` classes that exist outside the file-system feature.
//!
//! `NotCapable` is installed in every build. File-system classes stay on
//! the `fs` feature and publish their own object first; this module adds
//! properties to that object instead of replacing it.

use std::cell::RefCell;

thread_local! {
  static NOT_CAPABLE: RefCell<Option<v8::Global<v8::Function>>> =
    const { RefCell::new(None) };
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
  yuzuko.set(scope, errors_key.into(), object.into());
}

pub(crate) fn clear() {
  NOT_CAPABLE.with(|slot| *slot.borrow_mut() = None);
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
    "#])
  .expect("script");
  assert_eq!(
    lines,
    vec![
      "function number number object undefined undefined undefined function undefined",
      "version",
      "error",
      "absolute",
      "ids",
    ],
    "{lines:?}"
  );
}
