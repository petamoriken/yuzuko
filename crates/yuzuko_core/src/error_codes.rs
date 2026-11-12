//! Errno names and `std::io::Error` classes, matching Deno's `deno_error` 0.7.1.
//!
//! `code` prefers the raw operating-system error. A kind fallback is used only
//! when that raw code is absent. Unix `ELOOP` and Windows error 1921 are
//! `FilesystemLoop` before the kind match, so a loop is not reported as a
//! plain `Error` when the kind is `Other`.

use std::io::ErrorKind;

/// The JavaScript class Deno uses for this operating-system error.
///
/// `"TypeError"` and `"Error"` are the built-in constructors. Every other
/// name is a `Yuzuko.errors` class.
pub(crate) fn io_class_name(error: &std::io::Error) -> &'static str {
  if filesystem_loop(error) {
    return "FilesystemLoop";
  }
  match error.kind() {
    ErrorKind::NotFound => "NotFound",
    ErrorKind::PermissionDenied => "PermissionDenied",
    ErrorKind::ConnectionRefused => "ConnectionRefused",
    ErrorKind::ConnectionReset => "ConnectionReset",
    ErrorKind::ConnectionAborted => "ConnectionAborted",
    ErrorKind::NotConnected => "NotConnected",
    ErrorKind::AddrInUse => "AddrInUse",
    ErrorKind::AddrNotAvailable => "AddrNotAvailable",
    ErrorKind::BrokenPipe => "BrokenPipe",
    ErrorKind::AlreadyExists => "AlreadyExists",
    ErrorKind::InvalidInput => "TypeError",
    ErrorKind::InvalidData => "InvalidData",
    ErrorKind::TimedOut => "TimedOut",
    ErrorKind::Interrupted => "Interrupted",
    ErrorKind::WriteZero => "WriteZero",
    ErrorKind::UnexpectedEof => "UnexpectedEof",
    ErrorKind::Other => "Error",
    ErrorKind::WouldBlock => "WouldBlock",
    ErrorKind::IsADirectory => "IsADirectory",
    ErrorKind::NetworkUnreachable => "NetworkUnreachable",
    ErrorKind::NotADirectory => "NotADirectory",
    kind => {
      if format!("{kind:?}") == "FilesystemLoop" {
        "FilesystemLoop"
      } else {
        "Error"
      }
    }
  }
}

/// `ErrorKind::FilesystemLoop` may still be matched by its debug name. The
/// operating-system code is the stable signal: Unix `ELOOP`, Windows
/// `ERROR_CANT_RESOLVE_FILENAME`.
fn filesystem_loop(error: &std::io::Error) -> bool {
  match error.raw_os_error() {
    #[cfg(unix)]
    Some(code) if code == libc::ELOOP => true,
    #[cfg(windows)]
    Some(1921) => true,
    _ => false,
  }
}

/// The `code` string Deno attaches, or `None` when Deno would omit it.
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
pub(crate) fn get_error_code(error: &std::io::Error) -> Option<&'static str> {
  let code = match error.raw_os_error() {
    #[cfg(any(unix, windows))]
    Some(code) => get_os_error_code(code),
    #[cfg(not(any(unix, windows)))]
    Some(_) => return None,
    None => get_io_error_code(error),
  };
  if code.is_empty() { None } else { Some(code) }
}

fn get_io_error_code(error: &std::io::Error) -> &'static str {
  match error.kind() {
    ErrorKind::AddrInUse => "EADDRINUSE",
    ErrorKind::AddrNotAvailable => "EADDRNOTAVAIL",
    ErrorKind::ConnectionAborted => "ECONNABORTED",
    ErrorKind::ConnectionRefused => "ECONNREFUSED",
    ErrorKind::ConnectionReset => "ECONNRESET",
    ErrorKind::AlreadyExists => "EEXIST",
    ErrorKind::Interrupted => "EINTR",
    ErrorKind::InvalidInput => "EINVAL",
    ErrorKind::NotFound => "ENOENT",
    ErrorKind::OutOfMemory => "ENOMEM",
    ErrorKind::Unsupported => "ENOSYS",
    ErrorKind::NotConnected => "ENOTCONN",
    ErrorKind::BrokenPipe => "EPIPE",
    ErrorKind::TimedOut => "ETIMEDOUT",
    // Collides with `EPERM` when the raw code is absent.
    ErrorKind::PermissionDenied => "EACCES",
    // Collides with `EAGAIN` when the raw code is absent.
    ErrorKind::WouldBlock => "EWOULDBLOCK",
    _ => "",
  }
}

#[cfg(unix)]
fn get_os_error_code(errno: i32) -> &'static str {
  match errno {
    libc::E2BIG => "E2BIG",
    libc::EACCES => "EACCES",
    libc::EADDRINUSE => "EADDRINUSE",
    libc::EADDRNOTAVAIL => "EADDRNOTAVAIL",
    libc::EAFNOSUPPORT => "EAFNOSUPPORT",
    libc::EAGAIN => "EAGAIN",
    libc::EALREADY => "EALREADY",
    libc::EBADF => "EBADF",
    libc::EBUSY => "EBUSY",
    libc::ECANCELED => "ECANCELED",
    libc::ECONNABORTED => "ECONNABORTED",
    libc::ECONNREFUSED => "ECONNREFUSED",
    libc::ECONNRESET => "ECONNRESET",
    libc::EEXIST => "EEXIST",
    libc::EFAULT => "EFAULT",
    libc::EHOSTUNREACH => "EHOSTUNREACH",
    libc::EINVAL => "EINVAL",
    libc::EIO => "EIO",
    libc::EISCONN => "EISCONN",
    libc::EISDIR => "EISDIR",
    libc::ELOOP => "ELOOP",
    libc::EMFILE => "EMFILE",
    libc::EMSGSIZE => "EMSGSIZE",
    libc::ENAMETOOLONG => "ENAMETOOLONG",
    libc::ENETUNREACH => "ENETUNREACH",
    libc::ENOBUFS => "ENOBUFS",
    libc::ENOENT => "ENOENT",
    libc::ENOMEM => "ENOMEM",
    libc::ENOSPC => "ENOSPC",
    libc::ENOTCONN => "ENOTCONN",
    libc::ENOTDIR => "ENOTDIR",
    libc::ENOTEMPTY => "ENOTEMPTY",
    libc::ENOTSOCK => "ENOTSOCK",
    libc::ENOTSUP => "ENOTSUP",
    libc::EPERM => "EPERM",
    libc::EPIPE => "EPIPE",
    libc::EPROTONOSUPPORT => "EPROTONOSUPPORT",
    libc::EROFS => "EROFS",
    libc::ETIMEDOUT => "ETIMEDOUT",
    libc::EXDEV => "EXDEV",
    libc::ESOCKTNOSUPPORT => "ESOCKTNOSUPPORT",
    _ => "",
  }
}

#[cfg(windows)]
fn get_os_error_code(errno: i32) -> &'static str {
  match errno {
    998 => "EACCES",
    10013 => "EACCES",
    1920 => "EACCES",
    1227 => "EADDRINUSE",
    10048 => "EADDRINUSE",
    10049 => "EADDRNOTAVAIL",
    10047 => "EAFNOSUPPORT",
    10035 => "EAGAIN",
    10037 => "EALREADY",
    1004 => "EBADF",
    6 => "EBADF",
    33 => "EBUSY",
    231 => "EBUSY",
    32 => "EBUSY",
    995 => "ECANCELED",
    10004 => "ECANCELED",
    1236 => "ECONNABORTED",
    10053 => "ECONNABORTED",
    1225 => "ECONNREFUSED",
    10061 => "ECONNREFUSED",
    64 => "ECONNRESET",
    10054 => "ECONNRESET",
    183 => "EEXIST",
    80 => "EEXIST",
    111 => "EFAULT",
    10014 => "EFAULT",
    1232 => "EHOSTUNREACH",
    10065 => "EHOSTUNREACH",
    122 => "EINVAL",
    13 => "EINVAL",
    87 => "EINVAL",
    1464 => "EINVAL",
    10022 => "EINVAL",
    10046 => "EINVAL",
    1102 => "EIO",
    1111 => "EIO",
    23 => "EIO",
    1166 => "EIO",
    1165 => "EIO",
    1393 => "EIO",
    1129 => "EIO",
    1101 => "EIO",
    31 => "EIO",
    1106 => "EIO",
    1117 => "EIO",
    1104 => "EIO",
    205 => "EIO",
    110 => "EIO",
    1103 => "EIO",
    156 => "EIO",
    10056 => "EISCONN",
    1921 => "ELOOP",
    4 => "EMFILE",
    10024 => "EMFILE",
    10040 => "EMSGSIZE",
    206 => "ENAMETOOLONG",
    1231 => "ENETUNREACH",
    10051 => "ENETUNREACH",
    10055 => "ENOBUFS",
    161 => "ENOENT",
    267 => "ENOENT",
    203 => "ENOENT",
    2 => "ENOENT",
    123 => "ENOENT",
    15 => "ENOENT",
    4392 => "ENOENT",
    126 => "ENOENT",
    3 => "ENOENT",
    11001 => "ENOENT",
    11004 => "ENOENT",
    8 => "ENOMEM",
    14 => "ENOMEM",
    82 => "ENOSPC",
    112 => "ENOSPC",
    277 => "ENOSPC",
    1100 => "ENOSPC",
    39 => "ENOSPC",
    2250 => "ENOTCONN",
    10057 => "ENOTCONN",
    145 => "ENOTEMPTY",
    10038 => "ENOTSOCK",
    50 => "ENOTSUP",
    5 => "EPERM",
    1314 => "EPERM",
    230 => "EPIPE",
    232 => "EPIPE",
    233 => "EPIPE",
    10058 => "EPIPE",
    10043 => "EPROTONOSUPPORT",
    19 => "EROFS",
    121 => "ETIMEDOUT",
    10060 => "ETIMEDOUT",
    17 => "EXDEV",
    1 => "EISDIR",
    208 => "E2BIG",
    10044 => "ESOCKTNOSUPPORT",
    _ => "",
  }
}

#[cfg(test)]
mod tests {
  use std::io::{Error, ErrorKind};

  use super::{get_error_code, io_class_name};

  #[test]
  fn kind_fallback_matches_deno() {
    let missing = Error::new(ErrorKind::NotFound, "missing");
    assert_eq!(io_class_name(&missing), "NotFound");
    assert_eq!(get_error_code(&missing), Some("ENOENT"));

    let invalid = Error::new(ErrorKind::InvalidInput, "bad");
    assert_eq!(io_class_name(&invalid), "TypeError");
    assert_eq!(get_error_code(&invalid), Some("EINVAL"));

    let unsupported = Error::new(ErrorKind::Unsupported, "no");
    assert_eq!(io_class_name(&unsupported), "Error");
    assert_eq!(get_error_code(&unsupported), Some("ENOSYS"));

    let readonly = Error::new(ErrorKind::ReadOnlyFilesystem, "ro");
    assert_eq!(io_class_name(&readonly), "Error");
    assert_eq!(get_error_code(&readonly), None);

    let busy = Error::new(ErrorKind::ResourceBusy, "busy");
    assert_eq!(io_class_name(&busy), "Error");
    assert_eq!(get_error_code(&busy), None);
  }

  #[cfg(unix)]
  #[test]
  fn unix_raw_codes_prefer_the_errno() {
    let missing = Error::from_raw_os_error(libc::ENOENT);
    assert_eq!(io_class_name(&missing), "NotFound");
    assert_eq!(get_error_code(&missing), Some("ENOENT"));

    let looped = Error::from_raw_os_error(libc::ELOOP);
    assert_eq!(io_class_name(&looped), "FilesystemLoop");
    assert_eq!(get_error_code(&looped), Some("ELOOP"));

    let readonly = Error::from_raw_os_error(libc::EROFS);
    assert_eq!(io_class_name(&readonly), "Error");
    assert_eq!(get_error_code(&readonly), Some("EROFS"));

    let busy = Error::from_raw_os_error(libc::EBUSY);
    assert_eq!(io_class_name(&busy), "Error");
    assert_eq!(get_error_code(&busy), Some("EBUSY"));

    let denied = Error::from_raw_os_error(libc::EPERM);
    assert_eq!(get_error_code(&denied), Some("EPERM"));
  }

  #[cfg(windows)]
  #[test]
  fn windows_raw_codes_use_deno_names() {
    let missing = Error::from_raw_os_error(2);
    assert_eq!(get_error_code(&missing), Some("ENOENT"));
    let looped = Error::from_raw_os_error(1921);
    assert_eq!(io_class_name(&looped), "FilesystemLoop");
    assert_eq!(get_error_code(&looped), Some("ELOOP"));
    let readonly = Error::from_raw_os_error(19);
    assert_eq!(get_error_code(&readonly), Some("EROFS"));
  }
}
