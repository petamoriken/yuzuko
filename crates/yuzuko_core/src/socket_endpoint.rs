//! Hostname and port checked before a TCP or UDP socket call.
//!
//! `listenTcp`, `connectTcp`, `listenDatagram`, and `Datagram.send` read the
//! same object shape. Each call keeps its own error text. The net check and
//! the address parse run once, in that order, for every call.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use crate::op::FromV8;
use crate::op::OpError;

const HOSTNAME: &str = "The hostname must be an IP address or \"localhost\".";

pub(crate) fn loopback_bind() -> Result<SocketAddr, OpError> {
  crate::permissions::check_net("127.0.0.1", 0)?;
  Ok(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))
}

pub(crate) fn endpoint(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
  min_port: u16,
  port_message: &'static str,
  object_message: &'static str,
  read_message: &'static str,
) -> Result<SocketAddr, OpError> {
  if !value.is_object() {
    return Err(OpError::type_(object_message));
  }
  let Ok(object) = v8::Local::<v8::Object>::try_from(value) else {
    return Err(OpError::type_(object_message));
  };
  let hostname = match read_field(scope, object, "hostname", read_message)? {
    Some(value) if !value.is_undefined() => String::from_v8(scope, value)?,
    _ => "127.0.0.1".to_string(),
  };
  let port = match read_field(scope, object, "port", read_message)? {
    Some(value) if !value.is_undefined() => {
      to_port(scope, value, min_port, port_message)?
    }
    _ if min_port > 0 => return Err(OpError::type_(port_message)),
    _ => 0,
  };
  checked_host(&hostname, port)
}

/// A connect or send destination.
///
/// `localhost` and an IP literal are [`Target::Addr`]. Any other non-empty
/// name is [`Target::Name`]. `host` is the trimmed caller text, so TLS can
/// use it as the server name after `localhost` was rewritten to `127.0.0.1`.
#[cfg(any(feature = "tcp", feature = "udp"))]
pub(crate) enum Target {
  Addr { addr: SocketAddr, host: String },
  Name { host: String, port: u16 },
}

/// Reads a connect or send address.
///
/// The net check uses the caller host before `localhost` is rewritten. An
/// omitted port uses `default_port` when it is set. Otherwise a `min_port`
/// above 0 makes an omitted port an error. An empty or bracketed host throws
/// after the net check.
#[cfg(any(feature = "tcp", feature = "udp"))]
pub(crate) fn destination(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
  min_port: u16,
  default_port: Option<u16>,
  port_message: &'static str,
  object_message: &'static str,
  read_message: &'static str,
) -> Result<Target, OpError> {
  if !value.is_object() {
    return Err(OpError::type_(object_message));
  }
  let Ok(object) = v8::Local::<v8::Object>::try_from(value) else {
    return Err(OpError::type_(object_message));
  };
  let hostname = match read_field(scope, object, "hostname", read_message)? {
    Some(value) if !value.is_undefined() => String::from_v8(scope, value)?,
    _ => "127.0.0.1".to_string(),
  };
  let port = match read_field(scope, object, "port", read_message)? {
    Some(value) if !value.is_undefined() => {
      to_port(scope, value, min_port, port_message)?
    }
    _ => match default_port {
      Some(port) => port,
      None if min_port > 0 => return Err(OpError::type_(port_message)),
      None => 0,
    },
  };
  crate::permissions::check_net(&hostname, port)?;
  let host = hostname.trim().to_string();
  if host.is_empty() || host.contains('[') || host.contains(']') {
    return Err(OpError::type_(HOSTNAME));
  }
  if host.eq_ignore_ascii_case("localhost") {
    return Ok(Target::Addr {
      addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port),
      host,
    });
  }
  if let Ok(ip) = host.parse::<IpAddr>() {
    return Ok(Target::Addr {
      addr: SocketAddr::new(ip, port),
      host,
    });
  }
  Ok(Target::Name { host, port })
}

pub(crate) fn checked_host(
  hostname: &str,
  port: u16,
) -> Result<SocketAddr, OpError> {
  crate::permissions::check_net(hostname, port)?;
  let ip = parse_host(hostname)?;
  Ok(SocketAddr::new(ip, port))
}

fn to_port(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
  min_port: u16,
  message: &str,
) -> Result<u16, OpError> {
  let number = f64::from_v8(scope, value)?;
  if !number.is_finite()
    || number.fract() != 0.0
    || number < f64::from(min_port)
    || number > 65535.0
  {
    return Err(OpError::type_(message));
  }
  Ok(number as u16)
}

fn parse_host(hostname: &str) -> Result<IpAddr, OpError> {
  if hostname.eq_ignore_ascii_case("localhost") {
    return Ok(IpAddr::V4(Ipv4Addr::LOCALHOST));
  }
  hostname
    .parse::<IpAddr>()
    .map_err(|_| OpError::type_(HOSTNAME))
}

pub(crate) use crate::js::read_field;

#[cfg(all(test, feature = "tcp", feature = "udp"))]
mod tests {
  #[test]
  fn tcp_and_udp_reject_the_same_host() {
    let lines = crate::run_scripts(&[r#"
        let tcp = "";
        let udp = "";
        try {
          Yuzuko.listenTcp({ hostname: "not-an-ip", port: 9 });
        } catch (error) {
          tcp = error.name + " " + error.message;
        }
        try {
          Yuzuko.listenDatagram({ hostname: "not-an-ip", port: 9 });
        } catch (error) {
          udp = error.name + " " + error.message;
        }
        console.log(tcp);
        console.log(udp);
      "#])
    .expect("script");
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert_eq!(lines[0], lines[1]);
    assert!(!lines[0].is_empty(), "{lines:?}");
  }
}

/// Reads `options.reusePort`. A missing or non-object `options` is `false`.
pub(crate) fn reuse_flag(
  scope: &mut v8::PinScope,
  options: v8::Local<v8::Value>,
  read_message: &str,
) -> Result<bool, OpError> {
  if options.is_null_or_undefined() {
    return Ok(false);
  }
  let Ok(object) = v8::Local::<v8::Object>::try_from(options) else {
    return Ok(false);
  };
  let value = crate::js::read_field(scope, object, "reusePort", read_message)?;
  crate::socket_bind::reuse_port(scope, value)
}

/// Whether a nonblocking socket call should be tried again.
#[inline]
pub(crate) fn is_io_retry(kind: std::io::ErrorKind) -> bool {
  use std::io::ErrorKind;
  matches!(
    kind,
    ErrorKind::WouldBlock | ErrorKind::TimedOut | ErrorKind::Interrupted
  )
}

/// Whether a blocking `accept` should be tried again.
#[cfg(any(feature = "tcp", feature = "http"))]
pub(crate) fn is_accept_retry(kind: std::io::ErrorKind) -> bool {
  use std::io::ErrorKind;
  matches!(
    kind,
    ErrorKind::WouldBlock
      | ErrorKind::TimedOut
      | ErrorKind::Interrupted
      | ErrorKind::ConnectionAborted
      | ErrorKind::ConnectionReset
  )
}

/// Defines `hostname` and `port` as own data properties of `object`.
#[cfg(any(feature = "tcp", feature = "udp"))]
pub(crate) fn address_fields(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  hostname: &str,
  port: u16,
) -> Result<(), ()> {
  let Some(key) = v8::String::new(scope, "hostname") else {
    return Err(());
  };
  let Some(value) = v8::String::new(scope, hostname) else {
    return Err(());
  };
  let _ = object.create_data_property(scope, key.into(), value.into());
  let Some(key) = v8::String::new(scope, "port") else {
    return Err(());
  };
  let port = v8::Integer::new(scope, i32::from(port));
  let _ = object.create_data_property(scope, key.into(), port.into());
  Ok(())
}
