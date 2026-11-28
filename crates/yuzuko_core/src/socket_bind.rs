//! Bind a listening socket before the operating system chooses its defaults.
//!
//! `std::net` sets `SO_REUSEADDR` on some platforms and does not expose
//! `SO_REUSEPORT`. An ordinary IPv4 bind stays on `TcpListener::bind` and
//! `UdpSocket::bind`. An IPv6 bind is created here so `IPV6_V6ONLY` is set
//! before `bind`: the Unix default is dual-stack, and `[::]` would accept
//! IPv4. `IPV6_V6ONLY` cannot be changed after `bind` on macOS.

use std::io;
use std::net::SocketAddr;

use crate::op::OpError;

const REUSE: &str = "reusePort must be a boolean.";
#[cfg_attr(unix, allow(dead_code))]
const WINDOWS_REUSE: &str = "Windows cannot reuse a listening port.";

/// `true` only for the boolean primitive `true`.
///
/// Omitted, `null`, `undefined`, and `false` stay on the ordinary bind.
/// Windows rejects `true` before a socket is created.
pub(crate) fn reuse_port(
  scope: &mut v8::PinScope,
  value: Option<v8::Local<v8::Value>>,
) -> Result<bool, OpError> {
  let Some(value) = value else {
    return Ok(false);
  };
  if value.is_null_or_undefined() {
    return Ok(false);
  }
  if !value.is_boolean() {
    return Err(OpError::type_(REUSE));
  }
  if !value.is_true() {
    return Ok(false);
  }
  #[cfg(windows)]
  {
    Err(crate::errors::exception(
      scope,
      "NotSupported",
      WINDOWS_REUSE,
    ))
  }
  #[cfg(not(windows))]
  {
    let _ = scope;
    Ok(true)
  }
}

#[cfg(unix)]
const LISTEN_BACKLOG: libc::c_int = 128;

#[cfg_attr(not(any(feature = "tcp", feature = "http")), allow(dead_code))]
#[cfg(unix)]
pub(crate) fn tcp_listener(
  addr: SocketAddr,
) -> io::Result<std::net::TcpListener> {
  let fd = open_bound(addr, libc::SOCK_STREAM, true, false)?;
  tcp_from_fd(fd)
}

/// An ordinary IPv6 TCP bind. `SO_REUSEADDR` matches `std`, and the socket
/// does not accept IPv4.
#[cfg_attr(not(any(feature = "tcp", feature = "http")), allow(dead_code))]
#[cfg(unix)]
pub(crate) fn tcp_listener_ipv6(
  addr: SocketAddr,
) -> io::Result<std::net::TcpListener> {
  let fd = open_bound(addr, libc::SOCK_STREAM, false, true)?;
  tcp_from_fd(fd)
}

#[cfg(unix)]
fn tcp_from_fd(fd: libc::c_int) -> io::Result<std::net::TcpListener> {
  use std::os::fd::FromRawFd;
  let rc = unsafe { libc::listen(fd, LISTEN_BACKLOG) };
  if rc != 0 {
    let error = io::Error::last_os_error();
    unsafe { libc::close(fd) };
    return Err(error);
  }
  Ok(unsafe { std::net::TcpListener::from_raw_fd(fd) })
}

#[cfg_attr(not(feature = "udp"), allow(dead_code))]
#[cfg(unix)]
pub(crate) fn udp_socket(addr: SocketAddr) -> io::Result<std::net::UdpSocket> {
  use std::os::fd::FromRawFd;
  let fd = open_bound(addr, libc::SOCK_DGRAM, true, false)?;
  Ok(unsafe { std::net::UdpSocket::from_raw_fd(fd) })
}

/// An ordinary IPv6 UDP bind. The socket does not accept IPv4.
#[cfg_attr(not(feature = "udp"), allow(dead_code))]
#[cfg(unix)]
pub(crate) fn udp_socket_ipv6(
  addr: SocketAddr,
) -> io::Result<std::net::UdpSocket> {
  use std::os::fd::FromRawFd;
  let fd = open_bound(addr, libc::SOCK_DGRAM, false, false)?;
  Ok(unsafe { std::net::UdpSocket::from_raw_fd(fd) })
}

#[cfg_attr(not(any(feature = "tcp", feature = "http")), allow(dead_code))]
#[cfg(not(unix))]
pub(crate) fn tcp_listener(
  addr: SocketAddr,
) -> io::Result<std::net::TcpListener> {
  let _ = addr;
  Err(io::Error::new(io::ErrorKind::Unsupported, WINDOWS_REUSE))
}

/// Windows defaults `IPV6_V6ONLY` to 1, so `std` already refuses IPv4.
#[cfg_attr(not(any(feature = "tcp", feature = "http")), allow(dead_code))]
#[cfg(not(unix))]
pub(crate) fn tcp_listener_ipv6(
  addr: SocketAddr,
) -> io::Result<std::net::TcpListener> {
  std::net::TcpListener::bind(addr)
}

#[cfg_attr(not(feature = "udp"), allow(dead_code))]
#[cfg(not(unix))]
pub(crate) fn udp_socket(addr: SocketAddr) -> io::Result<std::net::UdpSocket> {
  let _ = addr;
  Err(io::Error::new(io::ErrorKind::Unsupported, WINDOWS_REUSE))
}

/// Windows defaults `IPV6_V6ONLY` to 1, so `std` already refuses IPv4.
#[cfg_attr(not(feature = "udp"), allow(dead_code))]
#[cfg(not(unix))]
pub(crate) fn udp_socket_ipv6(
  addr: SocketAddr,
) -> io::Result<std::net::UdpSocket> {
  std::net::UdpSocket::bind(addr)
}

#[cfg(unix)]
fn open_bound(
  addr: SocketAddr,
  kind: libc::c_int,
  reuse_port: bool,
  reuse_addr: bool,
) -> io::Result<libc::c_int> {
  let domain = match addr {
    SocketAddr::V4(_) => libc::AF_INET,
    SocketAddr::V6(_) => libc::AF_INET6,
  };
  let fd = unsafe { libc::socket(domain, kind, 0) };
  if fd < 0 {
    return Err(io::Error::last_os_error());
  }
  let bound = (|| {
    set_cloexec(fd)?;
    if reuse_addr {
      set_int(fd, libc::SOL_SOCKET, libc::SO_REUSEADDR, 1)?;
    }
    if reuse_port {
      set_int(fd, libc::SOL_SOCKET, libc::SO_REUSEPORT, 1)?;
    }
    if addr.is_ipv6() {
      set_int(fd, libc::IPPROTO_IPV6, libc::IPV6_V6ONLY, 1)?;
    }
    bind_fd(fd, addr)?;
    Ok(fd)
  })();
  if bound.is_err() {
    unsafe { libc::close(fd) };
  }
  bound
}

#[cfg(unix)]
fn set_cloexec(fd: libc::c_int) -> io::Result<()> {
  let rc = unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) };
  if rc < 0 {
    Err(io::Error::last_os_error())
  } else {
    Ok(())
  }
}

#[cfg(unix)]
fn set_int(
  fd: libc::c_int,
  level: libc::c_int,
  name: libc::c_int,
  value: libc::c_int,
) -> io::Result<()> {
  let rc = unsafe {
    libc::setsockopt(
      fd,
      level,
      name,
      &value as *const libc::c_int as *const libc::c_void,
      std::mem::size_of::<libc::c_int>() as libc::socklen_t,
    )
  };
  if rc == 0 {
    Ok(())
  } else {
    Err(io::Error::last_os_error())
  }
}

#[cfg(unix)]
fn bind_fd(fd: libc::c_int, addr: SocketAddr) -> io::Result<()> {
  let rc = match addr {
    SocketAddr::V4(addr) => {
      let mut sa: libc::sockaddr_in = unsafe { std::mem::zeroed() };
      set_sin_len(&mut sa);
      sa.sin_family = libc::AF_INET as libc::sa_family_t;
      sa.sin_port = addr.port().to_be();
      sa.sin_addr = libc::in_addr {
        s_addr: u32::from(*addr.ip()).to_be(),
      };
      unsafe {
        libc::bind(
          fd,
          &sa as *const libc::sockaddr_in as *const libc::sockaddr,
          std::mem::size_of::<libc::sockaddr_in>() as libc::socklen_t,
        )
      }
    }
    SocketAddr::V6(addr) => {
      let mut sa: libc::sockaddr_in6 = unsafe { std::mem::zeroed() };
      set_sin6_len(&mut sa);
      sa.sin6_family = libc::AF_INET6 as libc::sa_family_t;
      sa.sin6_port = addr.port().to_be();
      sa.sin6_addr = libc::in6_addr {
        s6_addr: addr.ip().octets(),
      };
      sa.sin6_scope_id = addr.scope_id();
      unsafe {
        libc::bind(
          fd,
          &sa as *const libc::sockaddr_in6 as *const libc::sockaddr,
          std::mem::size_of::<libc::sockaddr_in6>() as libc::socklen_t,
        )
      }
    }
  };
  if rc == 0 {
    Ok(())
  } else {
    Err(io::Error::last_os_error())
  }
}

#[cfg(all(
  unix,
  any(
    target_os = "macos",
    target_os = "ios",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd",
    target_os = "dragonfly",
  )
))]
fn set_sin_len(sa: &mut libc::sockaddr_in) {
  sa.sin_len = std::mem::size_of::<libc::sockaddr_in>() as u8;
}

#[cfg(all(
  unix,
  not(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd",
    target_os = "dragonfly",
  ))
))]
fn set_sin_len(_sa: &mut libc::sockaddr_in) {}

#[cfg(all(
  unix,
  any(
    target_os = "macos",
    target_os = "ios",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd",
    target_os = "dragonfly",
  )
))]
fn set_sin6_len(sa: &mut libc::sockaddr_in6) {
  sa.sin6_len = std::mem::size_of::<libc::sockaddr_in6>() as u8;
}

#[cfg(all(
  unix,
  not(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd",
    target_os = "dragonfly",
  ))
))]
fn set_sin6_len(_sa: &mut libc::sockaddr_in6) {}

#[cfg(all(test, unix))]
mod tests {
  use std::net::{Ipv6Addr, SocketAddr, TcpStream, UdpSocket};
  use std::os::fd::AsRawFd;
  use std::time::Duration;

  use super::{tcp_listener_ipv6, udp_socket, udp_socket_ipv6};

  #[test]
  fn two_udp_sockets_share_an_ephemeral_port() {
    let first: UdpSocket =
      udp_socket("127.0.0.1:0".parse().unwrap()).expect("first bind");
    let port = first.local_addr().unwrap().port();
    assert_ne!(port, 0);
    let second = udp_socket(format!("127.0.0.1:{port}").parse().unwrap())
      .expect("second bind");
    assert_eq!(second.local_addr().unwrap().port(), port);
  }

  #[test]
  fn an_ipv6_tcp_listener_rejects_ipv4() {
    let listener = tcp_listener_ipv6("[::]:0".parse().unwrap()).unwrap();
    assert_eq!(only_v6(listener.as_raw_fd()), 1);
    let port = listener.local_addr().unwrap().port();
    let ipv4 = TcpStream::connect_timeout(
      &SocketAddr::from(([127, 0, 0, 1], port)),
      Duration::from_millis(500),
    );
    assert!(ipv4.is_err(), "{ipv4:?}");
    let ipv6 = TcpStream::connect_timeout(
      &SocketAddr::from((Ipv6Addr::LOCALHOST, port)),
      Duration::from_millis(500),
    );
    assert!(ipv6.is_ok(), "{ipv6:?}");
  }

  #[test]
  fn an_ipv6_udp_socket_rejects_ipv4() {
    let socket = udp_socket_ipv6("[::]:0".parse().unwrap()).unwrap();
    assert_eq!(only_v6(socket.as_raw_fd()), 1);
    let port = socket.local_addr().unwrap().port();
    socket
      .set_read_timeout(Some(Duration::from_millis(200)))
      .unwrap();
    let sender = UdpSocket::bind("127.0.0.1:0").unwrap();
    sender
      .send_to(b"v4", SocketAddr::from(([127, 0, 0, 1], port)))
      .unwrap();
    let mut buf = [0u8; 16];
    assert!(socket.recv_from(&mut buf).is_err());
    let v6 = UdpSocket::bind((Ipv6Addr::LOCALHOST, 0)).unwrap();
    v6.send_to(b"v6", SocketAddr::from((Ipv6Addr::LOCALHOST, port)))
      .unwrap();
    let (count, _) = socket.recv_from(&mut buf).unwrap();
    assert_eq!(&buf[..count], b"v6");
  }

  fn only_v6(fd: libc::c_int) -> libc::c_int {
    let mut value: libc::c_int = -1;
    let mut len = std::mem::size_of::<libc::c_int>() as libc::socklen_t;
    let rc = unsafe {
      libc::getsockopt(
        fd,
        libc::IPPROTO_IPV6,
        libc::IPV6_V6ONLY,
        &mut value as *mut libc::c_int as *mut libc::c_void,
        &mut len,
      )
    };
    assert_eq!(rc, 0);
    value
  }
}
