//! File-system exceptions.
//!
//! Typed failures become the constructors captured at startup. A `TypeError`
//! stays a `TypeError`. An operating-system failure that has no dedicated
//! class becomes a plain `Error`.

use std::cell::RefCell;
use std::collections::HashMap;
use std::io;

use crate::op::{OpError, OpException};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum Class {
  NotFound,
  AlreadyExists,
  PermissionDenied,
  IsADirectory,
  NotADirectory,
  NotSupported,
  Busy,
  Interrupted,
  InvalidData,
  BadResource,
  FilesystemLoop,
  Generic,
}

impl Class {
  pub(super) const INSTALLED: [Class; 11] = [
    Class::NotFound,
    Class::AlreadyExists,
    Class::PermissionDenied,
    Class::IsADirectory,
    Class::NotADirectory,
    Class::NotSupported,
    Class::Busy,
    Class::Interrupted,
    Class::InvalidData,
    Class::BadResource,
    Class::FilesystemLoop,
  ];

  pub(super) fn name(self) -> &'static str {
    match self {
      Class::NotFound => "NotFound",
      Class::AlreadyExists => "AlreadyExists",
      Class::PermissionDenied => "PermissionDenied",
      Class::IsADirectory => "IsADirectory",
      Class::NotADirectory => "NotADirectory",
      Class::NotSupported => "NotSupported",
      Class::Busy => "Busy",
      Class::Interrupted => "Interrupted",
      Class::InvalidData => "InvalidData",
      Class::BadResource => "BadResource",
      Class::FilesystemLoop => "FilesystemLoop",
      Class::Generic => "Error",
    }
  }
}

thread_local! {
  static CONSTRUCTORS: RefCell<HashMap<Class, v8::Global<v8::Function>>> =
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

pub(super) fn install(scope: &mut v8::PinScope) {
  let source = v8::String::new(scope, FACTORY).expect("file error class");
  let script =
    v8::Script::compile(scope, source, None).expect("file error class");
  let factory = script.run(scope).expect("file error class");
  let factory =
    v8::Local::<v8::Function>::try_from(factory).expect("file error class");
  let mut built = Vec::with_capacity(Class::INSTALLED.len());
  for class in Class::INSTALLED {
    let name = v8::String::new(scope, class.name()).expect("file error class");
    let receiver = v8::undefined(scope).into();
    let value = factory
      .call(scope, receiver, &[name.into()])
      .expect("file error class");
    let ctor =
      v8::Local::<v8::Function>::try_from(value).expect("file error class");
    built.push((class, v8::Global::new(scope, ctor)));
  }
  CONSTRUCTORS.with(|map| {
    let mut map = map.borrow_mut();
    map.clear();
    map.extend(built);
  });
}

pub(super) fn publish(scope: &mut v8::PinScope) {
  let pairs = CONSTRUCTORS.with(|map| {
    map
      .borrow()
      .iter()
      .map(|(class, ctor)| (*class, ctor.clone()))
      .collect::<Vec<_>>()
  });
  let object = v8::Object::new(scope);
  for (class, ctor) in pairs {
    let key = v8::String::new(scope, class.name()).expect("file error class");
    let ctor = v8::Local::new(scope, ctor);
    object.set(scope, key.into(), ctor.into());
  }
  let yuzuko = crate::namespace(scope, "Yuzuko");
  let key = v8::String::new(scope, "errors").expect("file error class");
  yuzuko.set(scope, key.into(), object.into());
}

pub(super) fn clear() {
  CONSTRUCTORS.with(|map| map.borrow_mut().clear());
}

#[derive(Debug)]
pub(super) enum FsError {
  Type(String),
  Os { class: Class, message: String },
}

impl FsError {
  pub(super) fn type_(message: impl Into<String>) -> Self {
    Self::Type(message.into())
  }

  pub(super) fn io(error: io::Error) -> Self {
    let class = if filesystem_loop(&error) {
      Class::FilesystemLoop
    } else {
      match error.kind() {
        io::ErrorKind::NotFound => Class::NotFound,
        io::ErrorKind::AlreadyExists => Class::AlreadyExists,
        io::ErrorKind::PermissionDenied => Class::PermissionDenied,
        io::ErrorKind::IsADirectory => Class::IsADirectory,
        io::ErrorKind::NotADirectory => Class::NotADirectory,
        io::ErrorKind::Unsupported
        | io::ErrorKind::NotSeekable
        | io::ErrorKind::ReadOnlyFilesystem => Class::NotSupported,
        io::ErrorKind::ResourceBusy | io::ErrorKind::ExecutableFileBusy => {
          Class::Busy
        }
        io::ErrorKind::Interrupted => Class::Interrupted,
        io::ErrorKind::InvalidData => Class::InvalidData,
        _ => Class::Generic,
      }
    };
    Self::Os {
      class,
      message: error.to_string(),
    }
  }

  pub(super) fn bundle() -> Self {
    Self::Os {
      class: Class::NotSupported,
      message: "Cannot modify the application bundle.".to_string(),
    }
  }

  pub(super) fn closed() -> Self {
    Self::Os {
      class: Class::BadResource,
      message: "The file is closed.".to_string(),
    }
  }

  pub(super) fn directory() -> Self {
    Self::Os {
      class: Class::IsADirectory,
      message: "The path is a directory.".to_string(),
    }
  }

  pub(super) fn utf8() -> Self {
    Self::Os {
      class: Class::InvalidData,
      message: "The file is not valid UTF-8.".to_string(),
    }
  }

  pub(super) fn failed() -> Self {
    Self::Os {
      class: Class::Generic,
      message: "The file operation failed.".to_string(),
    }
  }

  pub(super) fn fail(self, scope: &mut v8::PinScope) -> OpError {
    let exception = self.exception(scope);
    OpError::thrown(scope, exception)
  }

  pub(super) fn exception<'s>(
    self,
    scope: &mut v8::PinScope<'s, '_>,
  ) -> v8::Local<'s, v8::Value> {
    match self {
      Self::Type(message) => type_error(scope, &message),
      Self::Os {
        class: Class::Generic,
        message,
      } => plain_error(scope, &message),
      Self::Os { class, message } => construct(scope, class, &message),
    }
  }
}

/// `ErrorKind::FilesystemLoop` is still unstable. The operating-system code
/// is the stable signal: Unix `ELOOP`, Windows `ERROR_CANT_RESOLVE_FILENAME`.
fn filesystem_loop(error: &io::Error) -> bool {
  match error.raw_os_error() {
    #[cfg(unix)]
    Some(code) if code == libc::ELOOP => true,
    #[cfg(windows)]
    Some(1921) => true,
    _ => false,
  }
}

fn type_error<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  message: &str,
) -> v8::Local<'s, v8::Value> {
  let message = v8::String::new(scope, message).unwrap();
  v8::Exception::type_error(scope, message)
}

fn plain_error<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  message: &str,
) -> v8::Local<'s, v8::Value> {
  let message = v8::String::new(scope, message).unwrap();
  v8::Exception::error(scope, message)
}

fn construct<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  class: Class,
  message: &str,
) -> v8::Local<'s, v8::Value> {
  let ctor = CONSTRUCTORS.with(|map| map.borrow().get(&class).cloned());
  let Some(ctor) = ctor else {
    return plain_error(scope, message);
  };
  let ctor = v8::Local::new(scope, ctor);
  let text = v8::String::new(scope, message).unwrap();
  let instance = {
    v8::tc_scope!(let tc, scope);
    ctor.new_instance(tc, &[text.into()])
  };
  match instance {
    Some(object) => object.into(),
    None => plain_error(scope, message),
  }
}

pub(super) fn throw(scope: &mut v8::PinScope, error: FsError) {
  OpException::throw(error.fail(scope), scope);
}
