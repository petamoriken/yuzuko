//! File-system exceptions.
//!
//! Typed failures become the constructors captured at startup. A `TypeError`
//! stays a `TypeError`. An operating-system failure that has no dedicated
//! class becomes a plain `Error`.

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
  Interrupted,
  InvalidData,
  BadResource,
  FilesystemLoop,
  ConnectionRefused,
  ConnectionReset,
  ConnectionAborted,
  NotConnected,
  AddrInUse,
  AddrNotAvailable,
  BrokenPipe,
  TimedOut,
  WriteZero,
  UnexpectedEof,
  WouldBlock,
  NetworkUnreachable,
  /// `ErrorKind::InvalidInput`. Deno uses the built-in `TypeError`.
  Type,
  Generic,
}

impl Class {
  pub(super) fn name(self) -> &'static str {
    match self {
      Class::NotFound => "NotFound",
      Class::AlreadyExists => "AlreadyExists",
      Class::PermissionDenied => "PermissionDenied",
      Class::IsADirectory => "IsADirectory",
      Class::NotADirectory => "NotADirectory",
      Class::NotSupported => "NotSupported",
      Class::Interrupted => "Interrupted",
      Class::InvalidData => "InvalidData",
      Class::BadResource => "BadResource",
      Class::FilesystemLoop => "FilesystemLoop",
      Class::ConnectionRefused => "ConnectionRefused",
      Class::ConnectionReset => "ConnectionReset",
      Class::ConnectionAborted => "ConnectionAborted",
      Class::NotConnected => "NotConnected",
      Class::AddrInUse => "AddrInUse",
      Class::AddrNotAvailable => "AddrNotAvailable",
      Class::BrokenPipe => "BrokenPipe",
      Class::TimedOut => "TimedOut",
      Class::WriteZero => "WriteZero",
      Class::UnexpectedEof => "UnexpectedEof",
      Class::WouldBlock => "WouldBlock",
      Class::NetworkUnreachable => "NetworkUnreachable",
      Class::Type => "TypeError",
      Class::Generic => "Error",
    }
  }

  fn from_io(error: &io::Error) -> Self {
    match crate::error_codes::io_class_name(error) {
      "NotFound" => Self::NotFound,
      "AlreadyExists" => Self::AlreadyExists,
      "PermissionDenied" => Self::PermissionDenied,
      "IsADirectory" => Self::IsADirectory,
      "NotADirectory" => Self::NotADirectory,
      "Interrupted" => Self::Interrupted,
      "InvalidData" => Self::InvalidData,
      "BadResource" => Self::BadResource,
      "FilesystemLoop" => Self::FilesystemLoop,
      "ConnectionRefused" => Self::ConnectionRefused,
      "ConnectionReset" => Self::ConnectionReset,
      "ConnectionAborted" => Self::ConnectionAborted,
      "NotConnected" => Self::NotConnected,
      "AddrInUse" => Self::AddrInUse,
      "AddrNotAvailable" => Self::AddrNotAvailable,
      "BrokenPipe" => Self::BrokenPipe,
      "TimedOut" => Self::TimedOut,
      "WriteZero" => Self::WriteZero,
      "UnexpectedEof" => Self::UnexpectedEof,
      "WouldBlock" => Self::WouldBlock,
      "NetworkUnreachable" => Self::NetworkUnreachable,
      "TypeError" => Self::Type,
      _ => Self::Generic,
    }
  }
}

#[derive(Debug)]
pub(super) enum FsError {
  Type(String),
  Os {
    class: Class,
    message: String,
    code: Option<&'static str>,
  },
  /// A host denial, such as `NotCapable` for a file opened after a swap.
  Host {
    name: String,
    message: String,
  },
}

impl FsError {
  /// Whether the operating system refused the access.
  #[cfg_attr(not(windows), allow(dead_code))]
  pub(super) fn permission_denied(&self) -> bool {
    matches!(
      self,
      Self::Os {
        class: Class::PermissionDenied,
        ..
      }
    )
  }

  pub(super) fn type_(message: impl Into<String>) -> Self {
    Self::Type(message.into())
  }

  pub(super) fn io(error: io::Error) -> Self {
    let code = crate::error_codes::get_error_code(&error);
    let class = Class::from_io(&error);
    Self::Os {
      class,
      message: error.to_string(),
      code,
    }
  }

  pub(super) fn bundle() -> Self {
    Self::Os {
      class: Class::NotSupported,
      message: "Cannot modify the application bundle.".to_string(),
      code: None,
    }
  }

  pub(super) fn closed() -> Self {
    Self::Os {
      class: Class::BadResource,
      message: "The file is closed.".to_string(),
      code: None,
    }
  }

  pub(super) fn not_supported(message: impl Into<String>) -> Self {
    Self::Os {
      class: Class::NotSupported,
      message: message.into(),
      code: None,
    }
  }

  pub(super) fn watcher_closed() -> Self {
    Self::Os {
      class: Class::BadResource,
      message: "The watcher is closed.".to_string(),
      code: None,
    }
  }

  pub(super) fn directory() -> Self {
    Self::Os {
      class: Class::IsADirectory,
      message: "The path is a directory.".to_string(),
      code: None,
    }
  }

  pub(super) fn utf8() -> Self {
    Self::Os {
      class: Class::InvalidData,
      message: "The file is not valid UTF-8.".to_string(),
      code: None,
    }
  }

  pub(super) fn failed() -> Self {
    Self::Os {
      class: Class::Generic,
      message: "The file operation failed.".to_string(),
      code: None,
    }
  }

  pub(super) fn fail(self, scope: &mut v8::PinScope) -> OpError {
    match self {
      Self::Host { name, message } => OpError::dom(name, message),
      other => {
        let exception = other.exception(scope);
        OpError::thrown(scope, exception)
      }
    }
  }

  pub(super) fn exception<'s>(
    self,
    scope: &mut v8::PinScope<'s, '_>,
  ) -> v8::Local<'s, v8::Value> {
    match self {
      Self::Type(message) => type_error(scope, &message),
      Self::Host { name, message } => {
        crate::op::new_dom_exception(scope, &name, &message)
          .map(v8::Local::into)
          .unwrap_or_else(|| plain_error(scope, &message))
      }
      Self::Os {
        class,
        message,
        code,
      } => {
        let value = match class {
          Class::Generic => plain_error(scope, &message),
          Class::Type => type_error(scope, &message),
          class => construct(scope, class, &message),
        };
        crate::errors::attach_code(scope, value, code);
        value
      }
    }
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
  crate::errors::construct(scope, class.name(), message)
}

pub(super) fn throw(scope: &mut v8::PinScope, error: FsError) {
  OpException::throw(error.fail(scope), scope);
}
