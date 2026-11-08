//! Resolve a host and keep the addresses the net policy allows.
//!
//! The caller has already checked the host string. After DNS, a loopback,
//! private, link-local, unique-local, shared, multicast, broadcast, or
//! unspecified address is checked again. A denied address is not returned.

use std::net::{IpAddr, SocketAddr};

use yuzuko_core::op::OpError;

pub(super) enum ResolveFailure {
  Type(String),
  Network(String),
  Denied { name: String, message: String },
}

pub(super) async fn checked_addrs(
  host: &str,
  port: u16,
) -> Result<Vec<SocketAddr>, ResolveFailure> {
  let addrs = lookup(host, port).await?;
  yuzuko_core::retain_resolved_addresses(addrs).map_err(from_op)
}

fn from_op(error: OpError) -> ResolveFailure {
  match error {
    OpError::Dom { name, message } => ResolveFailure::Denied { name, message },
    OpError::Type(message) | OpError::Range(message) => {
      ResolveFailure::Type(message)
    }
    OpError::Thrown(_) => {
      ResolveFailure::Type("The URL is invalid".to_string())
    }
  }
}

fn strip_brackets(host: &str) -> &str {
  host
    .strip_prefix('[')
    .and_then(|rest| rest.strip_suffix(']'))
    .filter(|inner| !inner.is_empty())
    .unwrap_or(host)
}

async fn lookup(
  host: &str,
  port: u16,
) -> Result<Vec<SocketAddr>, ResolveFailure> {
  let bare = strip_brackets(host.trim());
  if bare.is_empty() {
    return Err(ResolveFailure::Type("The URL has no host".to_string()));
  }
  if let Ok(ip) = bare.parse::<IpAddr>() {
    return Ok(vec![SocketAddr::new(ip, port)]);
  }
  let dial = if bare.contains(':') {
    format!("[{bare}]:{port}")
  } else {
    format!("{bare}:{port}")
  };
  // `getaddrinfo` runs on the blocking pool while `env.set` cannot enter
  // `setenv`. Holding the lock across `lookup_host` would not cover the
  // pool thread that actually calls libc.
  let looked_up = tokio::task::spawn_blocking(move || {
    yuzuko_core::with_env(|| {
      use std::net::ToSocketAddrs;
      dial
        .to_socket_addrs()
        .map(|found| found.collect::<Vec<_>>())
    })
  })
  .await
  .map_err(|error| ResolveFailure::Network(error.to_string()))?
  .map_err(|error| ResolveFailure::Network(error.to_string()))?;
  Ok(looked_up)
}
