//! Operating-system socket failures that have a dedicated `Yuzuko.errors` class.
//!
//! `listenTcp`, `connectTcp`, `listenDatagram`, `Datagram.send`, and
//! `listenHttp` call [`classify`]. A poll timeout is retried before this
//! runs, so a 50ms wait does not become `TimedOut`. `WouldBlock` and
//! `Interrupted` stay unclassified here; a file-system call maps them itself.
//! [`map_io`] keeps that table, turns `InvalidInput` into a `TypeError`, and
//! attaches the errno name Deno would set.

use std::io::ErrorKind;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Class {
  AddrInUse,
  AddrNotAvailable,
  ConnectionRefused,
  ConnectionReset,
  ConnectionAborted,
  NotConnected,
  TimedOut,
  BrokenPipe,
  UnexpectedEof,
  NetworkUnreachable,
  WriteZero,
}

#[cfg_attr(
  not(any(feature = "tcp", feature = "udp", feature = "http")),
  allow(dead_code)
)]
impl Class {
  pub(crate) fn name(self) -> &'static str {
    match self {
      Class::AddrInUse => "AddrInUse",
      Class::AddrNotAvailable => "AddrNotAvailable",
      Class::ConnectionRefused => "ConnectionRefused",
      Class::ConnectionReset => "ConnectionReset",
      Class::ConnectionAborted => "ConnectionAborted",
      Class::NotConnected => "NotConnected",
      Class::TimedOut => "TimedOut",
      Class::BrokenPipe => "BrokenPipe",
      Class::UnexpectedEof => "UnexpectedEof",
      Class::NetworkUnreachable => "NetworkUnreachable",
      Class::WriteZero => "WriteZero",
    }
  }

  pub(crate) fn message(self) -> &'static str {
    match self {
      Class::AddrInUse => "The address is already in use.",
      Class::AddrNotAvailable => "The address is not available.",
      Class::ConnectionRefused => "The connection was refused.",
      Class::ConnectionReset => "The connection was reset.",
      Class::ConnectionAborted => "The connection was aborted.",
      Class::NotConnected => "The socket is not connected.",
      Class::TimedOut => "The socket operation timed out.",
      Class::BrokenPipe => "The socket pipe is broken.",
      Class::UnexpectedEof => "The socket reached the end of the stream.",
      Class::NetworkUnreachable => "The network is unreachable.",
      Class::WriteZero => "The bytes could not be written.",
    }
  }
}

/// Maps a socket `ErrorKind` onto a class. Other kinds, including
/// `WouldBlock` and `Interrupted`, stay unclassified.
#[cfg_attr(
  not(any(feature = "tcp", feature = "udp", feature = "http")),
  allow(dead_code)
)]
pub(crate) fn classify(kind: ErrorKind) -> Option<Class> {
  match kind {
    ErrorKind::AddrInUse => Some(Class::AddrInUse),
    ErrorKind::AddrNotAvailable => Some(Class::AddrNotAvailable),
    ErrorKind::ConnectionRefused => Some(Class::ConnectionRefused),
    ErrorKind::ConnectionReset => Some(Class::ConnectionReset),
    ErrorKind::ConnectionAborted => Some(Class::ConnectionAborted),
    ErrorKind::NotConnected => Some(Class::NotConnected),
    ErrorKind::TimedOut => Some(Class::TimedOut),
    ErrorKind::BrokenPipe => Some(Class::BrokenPipe),
    ErrorKind::UnexpectedEof => Some(Class::UnexpectedEof),
    ErrorKind::NetworkUnreachable => Some(Class::NetworkUnreachable),
    ErrorKind::WriteZero => Some(Class::WriteZero),
    _ => None,
  }
}

/// A socket operating-system error after [`classify`].
///
/// Kinds outside the socket table stay [`Mapped::Generic`]. `InvalidInput`
/// is a `TypeError`. `code` is the errno name Deno would set.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(
  not(any(feature = "tcp", feature = "udp", feature = "http")),
  allow(dead_code)
)]
pub(crate) enum Mapped {
  Class {
    class: Class,
    code: Option<&'static str>,
  },
  Generic {
    code: Option<&'static str>,
    type_error: bool,
  },
}

/// Maps one operating-system error for a socket or HTTP call.
#[cfg_attr(
  not(any(feature = "tcp", feature = "udp", feature = "http")),
  allow(dead_code)
)]
pub(crate) fn map_io(error: &std::io::Error) -> Mapped {
  let code = crate::error_codes::get_error_code(error);
  if error.kind() == ErrorKind::InvalidInput {
    return Mapped::Generic {
      code,
      type_error: true,
    };
  }
  if let Some(class) = classify(error.kind()) {
    return Mapped::Class { class, code };
  }
  Mapped::Generic {
    code,
    type_error: false,
  }
}

#[cfg(test)]
mod tests {
  use std::io::ErrorKind;

  use super::{Class, Mapped, classify, map_io};

  #[test]
  fn socket_path_classifies_timed_out_and_unreachable() {
    assert_eq!(
      classify(ErrorKind::TimedOut).map(Class::name),
      Some("TimedOut")
    );
    assert_eq!(
      classify(ErrorKind::TimedOut).map(Class::message),
      Some("The socket operation timed out.")
    );
    assert_eq!(
      classify(ErrorKind::NetworkUnreachable).map(Class::name),
      Some("NetworkUnreachable")
    );
    assert_eq!(
      classify(ErrorKind::NetworkUnreachable).map(Class::message),
      Some("The network is unreachable.")
    );
    assert_eq!(classify(ErrorKind::AddrInUse), Some(Class::AddrInUse));
    assert_eq!(
      classify(ErrorKind::ConnectionRefused),
      Some(Class::ConnectionRefused)
    );
    assert_eq!(
      classify(ErrorKind::ConnectionReset),
      Some(Class::ConnectionReset)
    );
    assert_eq!(classify(ErrorKind::BrokenPipe), Some(Class::BrokenPipe));
    assert_eq!(
      classify(ErrorKind::UnexpectedEof),
      Some(Class::UnexpectedEof)
    );
    assert_eq!(classify(ErrorKind::WouldBlock), None);
    assert_eq!(classify(ErrorKind::Interrupted), None);
    assert_eq!(
      classify(ErrorKind::AddrNotAvailable),
      Some(Class::AddrNotAvailable)
    );
    assert_eq!(
      classify(ErrorKind::AddrNotAvailable).map(Class::message),
      Some("The address is not available.")
    );
    assert_eq!(
      classify(ErrorKind::ConnectionAborted),
      Some(Class::ConnectionAborted)
    );
    assert_eq!(
      classify(ErrorKind::ConnectionAborted).map(Class::message),
      Some("The connection was aborted.")
    );
    assert_eq!(classify(ErrorKind::NotConnected), Some(Class::NotConnected));
    assert_eq!(
      classify(ErrorKind::NotConnected).map(Class::message),
      Some("The socket is not connected.")
    );
    assert_eq!(classify(ErrorKind::WriteZero), Some(Class::WriteZero));
    assert_eq!(
      classify(ErrorKind::WriteZero).map(Class::message),
      Some("The bytes could not be written.")
    );
  }

  #[test]
  fn map_io_keeps_socket_classes_and_marks_invalid_input() {
    let invalid = std::io::Error::new(ErrorKind::InvalidInput, "bad");
    assert_eq!(
      map_io(&invalid),
      Mapped::Generic {
        code: Some("EINVAL"),
        type_error: true,
      }
    );
    let used = std::io::Error::new(ErrorKind::AddrInUse, "used");
    assert_eq!(
      map_io(&used),
      Mapped::Class {
        class: Class::AddrInUse,
        code: Some("EADDRINUSE"),
      }
    );
    let waiting = std::io::Error::new(ErrorKind::WouldBlock, "wait");
    assert_eq!(
      map_io(&waiting),
      Mapped::Generic {
        code: Some("EWOULDBLOCK"),
        type_error: false,
      }
    );
    let stopped = std::io::Error::new(ErrorKind::Interrupted, "stop");
    assert_eq!(
      map_io(&stopped),
      Mapped::Generic {
        code: Some("EINTR"),
        type_error: false,
      }
    );
  }
}
