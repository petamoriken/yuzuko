//! Names for `addSignalListener` and `Child.kill`.
//!
//! Numbers come from `libc` because `SIGUSR1` is 30 on macOS and 10 on Linux.
//! Fatal signals stay unnamed so a V8 crash handler is left in place.
//! `SIGKILL` and `SIGSTOP` are named and cannot be caught.

#[cfg(feature = "signal")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Listen {
  Index(usize),
  /// `SIGKILL` or `SIGSTOP`.
  Uncatchable,
  /// A catalog name this operating system will not install a handler for.
  #[cfg(windows)]
  Platform,
  Unknown,
}

#[cfg_attr(not(feature = "signal"), allow(dead_code))]
#[cfg(windows)]
pub(crate) const LISTEN_COUNT: usize = 2;

#[cfg_attr(not(feature = "signal"), allow(dead_code))]
#[cfg(any(target_os = "linux", target_os = "macos", target_os = "ios"))]
pub(crate) const LISTEN_COUNT: usize = 22;

#[cfg_attr(not(feature = "signal"), allow(dead_code))]
#[cfg(all(
  unix,
  not(any(target_os = "linux", target_os = "macos", target_os = "ios"))
))]
pub(crate) const LISTEN_COUNT: usize = 20;

#[cfg_attr(not(feature = "signal"), allow(dead_code))]
#[cfg(not(any(unix, windows)))]
pub(crate) const LISTEN_COUNT: usize = 0;

#[cfg(feature = "signal")]
pub(crate) fn listen(name: &str) -> Listen {
  if matches!(name, "SIGKILL" | "SIGSTOP") {
    return Listen::Uncatchable;
  }
  #[cfg(windows)]
  {
    windows_listen(name)
  }
  #[cfg(unix)]
  {
    match catch_index(name) {
      Some(index) => Listen::Index(index),
      None => Listen::Unknown,
    }
  }
  #[cfg(not(any(unix, windows)))]
  {
    let _ = name;
    Listen::Unknown
  }
}

#[cfg(all(feature = "signal", windows))]
fn windows_listen(name: &str) -> Listen {
  match name {
    "SIGINT" => Listen::Index(0),
    "SIGTERM" => Listen::Index(1),
    "SIGHUP" | "SIGQUIT" | "SIGPIPE" | "SIGALRM" | "SIGUSR1" | "SIGUSR2"
    | "SIGCHLD" | "SIGCONT" | "SIGTSTP" | "SIGTTIN" | "SIGTTOU" | "SIGURG"
    | "SIGXCPU" | "SIGXFSZ" | "SIGVTALRM" | "SIGPROF" | "SIGWINCH"
    | "SIGIO" | "SIGPWR" | "SIGSTKFLT" | "SIGINFO" | "SIGEMT" => {
      Listen::Platform
    }
    _ => Listen::Unknown,
  }
}

/// Catchable catalog index on this Unix. `SIGINT` is 1 and `SIGTERM` is 5.
#[cfg(all(unix, any(feature = "signal", feature = "child")))]
fn catch_index(name: &str) -> Option<usize> {
  Some(match name {
    "SIGHUP" => 0,
    "SIGINT" => 1,
    "SIGQUIT" => 2,
    "SIGPIPE" => 3,
    "SIGALRM" => 4,
    "SIGTERM" => 5,
    "SIGUSR1" => 6,
    "SIGUSR2" => 7,
    "SIGCHLD" => 8,
    "SIGCONT" => 9,
    "SIGTSTP" => 10,
    "SIGTTIN" => 11,
    "SIGTTOU" => 12,
    "SIGURG" => 13,
    "SIGXCPU" => 14,
    "SIGXFSZ" => 15,
    "SIGVTALRM" => 16,
    "SIGPROF" => 17,
    "SIGWINCH" => 18,
    "SIGIO" => 19,
    #[cfg(target_os = "linux")]
    "SIGPWR" => 20,
    #[cfg(target_os = "linux")]
    "SIGSTKFLT" => 21,
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    "SIGINFO" => 20,
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    "SIGEMT" => 21,
    _ => return None,
  })
}

#[cfg(all(unix, any(feature = "signal", feature = "child")))]
pub(crate) fn raw_number(index: usize) -> libc::c_int {
  match index {
    0 => libc::SIGHUP,
    1 => libc::SIGINT,
    2 => libc::SIGQUIT,
    3 => libc::SIGPIPE,
    4 => libc::SIGALRM,
    5 => libc::SIGTERM,
    6 => libc::SIGUSR1,
    7 => libc::SIGUSR2,
    8 => libc::SIGCHLD,
    9 => libc::SIGCONT,
    10 => libc::SIGTSTP,
    11 => libc::SIGTTIN,
    12 => libc::SIGTTOU,
    13 => libc::SIGURG,
    14 => libc::SIGXCPU,
    15 => libc::SIGXFSZ,
    16 => libc::SIGVTALRM,
    17 => libc::SIGPROF,
    18 => libc::SIGWINCH,
    19 => libc::SIGIO,
    #[cfg(target_os = "linux")]
    20 => libc::SIGPWR,
    #[cfg(target_os = "linux")]
    21 => libc::SIGSTKFLT,
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    20 => libc::SIGINFO,
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    21 => libc::SIGEMT,
    _ => 0,
  }
}

/// A signal `Child.kill` may send. Fatal faults stay out.
#[cfg(all(unix, feature = "child"))]
pub(crate) fn unix_send(name: &str) -> Option<libc::c_int> {
  if let Some(index) = catch_index(name) {
    let number = raw_number(index);
    return (number != 0).then_some(number);
  }
  Some(match name {
    "SIGKILL" => libc::SIGKILL,
    "SIGSTOP" => libc::SIGSTOP,
    "SIGABRT" => libc::SIGABRT,
    _ => return None,
  })
}
