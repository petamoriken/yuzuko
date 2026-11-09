//! Hostname and port checked before a TCP or UDP socket call.
//!
//! `listenTcp`, `connectTcp`, `listenDatagram`, and `Datagram.send` read the
//! same object shape. Each call keeps its own error text. The net check and
//! the address parse run once, in that order, for every call.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};

#[cfg(any(feature = "tcp", feature = "udp"))]
use crate::op::FromV8;
use crate::op::OpError;

const HOSTNAME: &str = "The hostname must be an IP address or \"localhost\".";

pub(crate) fn loopback_bind() -> Result<SocketAddr, OpError> {
  crate::permissions::check_net("127.0.0.1", 0)?;
  Ok(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))
}

#[cfg(any(feature = "tcp", feature = "udp"))]
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

pub(crate) fn checked_host(
  hostname: &str,
  port: u16,
) -> Result<SocketAddr, OpError> {
  crate::permissions::check_net(hostname, port)?;
  let ip = parse_host(hostname)?;
  Ok(SocketAddr::new(ip, port))
}

#[cfg(any(feature = "tcp", feature = "udp"))]
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

#[cfg(any(feature = "tcp", feature = "udp"))]
fn read_field<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  object: v8::Local<'_, v8::Object>,
  name: &str,
  read_message: &str,
) -> Result<Option<v8::Local<'s, v8::Value>>, OpError> {
  enum Outcome {
    Value(v8::Global<v8::Value>),
    Throw(v8::Global<v8::Value>),
    Missing,
  }
  let outcome = {
    v8::tc_scope!(let tc, scope);
    let Some(key) = v8::String::new(tc, name) else {
      return Err(OpError::type_(read_message));
    };
    let object = v8::Local::new(tc, object);
    match object.get(tc, key.into()) {
      Some(value) => Outcome::Value(v8::Global::new(tc, value)),
      None => {
        if let Some(exception) = tc.exception() {
          Outcome::Throw(v8::Global::new(tc, exception))
        } else {
          Outcome::Missing
        }
      }
    }
  };
  match outcome {
    Outcome::Value(value) => Ok(Some(v8::Local::new(scope, value))),
    Outcome::Throw(exception) => {
      Err(OpError::thrown(scope, v8::Local::new(scope, exception)))
    }
    Outcome::Missing => Err(OpError::type_(read_message)),
  }
}

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
