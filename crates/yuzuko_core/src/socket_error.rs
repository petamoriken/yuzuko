//! Operating-system socket failures that have a dedicated `Yuzuko.errors` class.
//!
//! `listenTcp`, `connectTcp`, `listenDatagram`, and `Datagram.send` call
//! [`classify`]. A poll timeout is retried before this runs, so a 50ms wait
//! does not become `TimedOut`.

use std::io::ErrorKind;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Class {
  AddrInUse,
  ConnectionRefused,
  ConnectionReset,
  TimedOut,
  BrokenPipe,
  UnexpectedEof,
  NetworkUnreachable,
}

impl Class {
  #[cfg_attr(not(any(feature = "tcp", feature = "udp")), allow(dead_code))]
  pub(crate) const INSTALLED: [Class; 7] = [
    Class::AddrInUse,
    Class::ConnectionRefused,
    Class::ConnectionReset,
    Class::TimedOut,
    Class::BrokenPipe,
    Class::UnexpectedEof,
    Class::NetworkUnreachable,
  ];

  pub(crate) fn name(self) -> &'static str {
    match self {
      Class::AddrInUse => "AddrInUse",
      Class::ConnectionRefused => "ConnectionRefused",
      Class::ConnectionReset => "ConnectionReset",
      Class::TimedOut => "TimedOut",
      Class::BrokenPipe => "BrokenPipe",
      Class::UnexpectedEof => "UnexpectedEof",
      Class::NetworkUnreachable => "NetworkUnreachable",
    }
  }

  pub(crate) fn message(self) -> &'static str {
    match self {
      Class::AddrInUse => "The address is already in use.",
      Class::ConnectionRefused => "The connection was refused.",
      Class::ConnectionReset => "The connection was reset.",
      Class::TimedOut => "The socket operation timed out.",
      Class::BrokenPipe => "The socket pipe is broken.",
      Class::UnexpectedEof => "The socket reached the end of the stream.",
      Class::NetworkUnreachable => "The network is unreachable.",
    }
  }
}

/// Maps a socket `ErrorKind` onto a class. Other kinds, including
/// `WouldBlock` and `Interrupted`, stay unclassified.
pub(crate) fn classify(kind: ErrorKind) -> Option<Class> {
  match kind {
    ErrorKind::AddrInUse => Some(Class::AddrInUse),
    ErrorKind::ConnectionRefused => Some(Class::ConnectionRefused),
    ErrorKind::ConnectionReset => Some(Class::ConnectionReset),
    ErrorKind::TimedOut => Some(Class::TimedOut),
    ErrorKind::BrokenPipe => Some(Class::BrokenPipe),
    ErrorKind::UnexpectedEof => Some(Class::UnexpectedEof),
    ErrorKind::NetworkUnreachable => Some(Class::NetworkUnreachable),
    _ => None,
  }
}

#[cfg(test)]
mod tests {
  use std::io::ErrorKind;

  use super::{Class, classify};

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
    assert_eq!(classify(ErrorKind::AddrNotAvailable), None);
  }
}
