//! Connect only to addresses the net policy allows.
//!
//! The host string is checked before this runs. After DNS, a loopback,
//! private, link-local, unique-local, shared, multicast, broadcast, or
//! unspecified address is checked again. The socket is an address that
//! passed that check.

use std::future::Future;
use std::net::{IpAddr, SocketAddr};
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use hyper::Uri;
use hyper_util::rt::TokioIo;
use tokio::net::TcpStream;
use tower_service::Service;

use super::HttpFailure;

#[derive(Clone, Copy, Debug)]
pub(super) struct GuardedHttp;

#[derive(Debug)]
struct NetDenied {
  name: String,
  message: String,
}

impl std::fmt::Display for NetDenied {
  fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    formatter.write_str(&self.message)
  }
}

impl std::error::Error for NetDenied {}

fn denied_io(name: String, message: String) -> std::io::Error {
  std::io::Error::new(
    std::io::ErrorKind::PermissionDenied,
    NetDenied { name, message },
  )
}

pub(super) fn denied_from(
  error: &(dyn std::error::Error + 'static),
) -> Option<HttpFailure> {
  let mut current = Some(error);
  while let Some(error) = current {
    if let Some(denied) = error.downcast_ref::<NetDenied>() {
      return Some(HttpFailure::Denied {
        name: denied.name.clone(),
        message: denied.message.clone(),
      });
    }
    if let Some(io_error) = error.downcast_ref::<std::io::Error>()
      && let Some(denied) = io_error
        .get_ref()
        .and_then(|inner| inner.downcast_ref::<NetDenied>())
    {
      return Some(HttpFailure::Denied {
        name: denied.name.clone(),
        message: denied.message.clone(),
      });
    }
    current = error.source();
  }
  None
}

fn failure_from(error: yuzuko_core::op::OpError) -> HttpFailure {
  match error {
    yuzuko_core::op::OpError::Dom { name, message } => {
      HttpFailure::Denied { name, message }
    }
    yuzuko_core::op::OpError::Type(message)
    | yuzuko_core::op::OpError::Range(message) => HttpFailure::Type(message),
    yuzuko_core::op::OpError::Thrown(_) => {
      HttpFailure::Type("The URL is invalid".to_string())
    }
  }
}

pub(super) async fn checked_addrs(
  host: &str,
  port: u16,
) -> Result<Vec<SocketAddr>, HttpFailure> {
  let addrs = lookup(host, port).await?;
  yuzuko_core::retain_resolved_addresses(addrs).map_err(failure_from)
}

pub(super) async fn connect_tcp(
  host: &str,
  port: u16,
) -> Result<TcpStream, HttpFailure> {
  let addrs = checked_addrs(host, port).await?;
  connect_addrs(addrs).await
}

fn strip_brackets(host: &str) -> &str {
  host
    .strip_prefix('[')
    .and_then(|rest| rest.strip_suffix(']'))
    .filter(|inner| !inner.is_empty())
    .unwrap_or(host)
}

async fn lookup(host: &str, port: u16) -> Result<Vec<SocketAddr>, HttpFailure> {
  let bare = strip_brackets(host.trim());
  if bare.is_empty() {
    return Err(HttpFailure::Type("The URL has no host".to_string()));
  }
  if let Ok(ip) = bare.parse::<IpAddr>() {
    return Ok(vec![SocketAddr::new(ip, port)]);
  }
  let dial = if bare.contains(':') {
    format!("[{bare}]:{port}")
  } else {
    format!("{bare}:{port}")
  };
  let looked_up = tokio::net::lookup_host(dial)
    .await
    .map_err(|error| HttpFailure::Network(error.to_string()))?;
  Ok(looked_up.collect())
}

async fn connect_addrs(
  addrs: Vec<SocketAddr>,
) -> Result<TcpStream, HttpFailure> {
  if addrs.is_empty() {
    return Err(HttpFailure::Network(
      "The address lookup returned no usable address".to_string(),
    ));
  }
  let first_v6 = addrs[0].is_ipv6();
  let mut preferred = Vec::new();
  let mut fallback = Vec::new();
  for addr in addrs {
    if addr.is_ipv6() == first_v6 {
      preferred.push(addr);
    } else {
      fallback.push(addr);
    }
  }
  let connected = if fallback.is_empty() {
    connect_sequence(preferred).await
  } else {
    race_families(preferred, fallback).await
  };
  connected.map_err(|error| HttpFailure::Network(error.to_string()))
}

async fn connect_sequence(
  addrs: Vec<SocketAddr>,
) -> Result<TcpStream, std::io::Error> {
  let mut last = None;
  for addr in addrs {
    match TcpStream::connect(addr).await {
      Ok(stream) => {
        let _ = stream.set_nodelay(true);
        return Ok(stream);
      }
      Err(error) => last = Some(error),
    }
  }
  Err(last.unwrap_or_else(|| {
    std::io::Error::new(
      std::io::ErrorKind::AddrNotAvailable,
      "The address lookup returned no usable address",
    )
  }))
}

/// Start the other address family if the first has not connected in 300ms.
async fn race_families(
  preferred: Vec<SocketAddr>,
  fallback: Vec<SocketAddr>,
) -> Result<TcpStream, std::io::Error> {
  let mut primary = tokio::spawn(connect_sequence(preferred));
  let mut secondary = tokio::spawn(async move {
    tokio::time::sleep(Duration::from_millis(300)).await;
    connect_sequence(fallback).await
  });
  let mut primary_error = None;
  let mut secondary_error = None;
  loop {
    tokio::select! {
      result = &mut primary, if primary_error.is_none() => {
        match result {
          Ok(Ok(stream)) => {
            secondary.abort();
            return Ok(stream);
          }
          Ok(Err(error)) => primary_error = Some(error),
          Err(error) => {
            primary_error = Some(std::io::Error::other(error.to_string()));
          }
        }
      }
      result = &mut secondary, if secondary_error.is_none() => {
        match result {
          Ok(Ok(stream)) => {
            primary.abort();
            return Ok(stream);
          }
          Ok(Err(error)) => secondary_error = Some(error),
          Err(error) => {
            secondary_error = Some(std::io::Error::other(error.to_string()));
          }
        }
      }
    }
    if let (Some(error), Some(_)) = (&primary_error, &secondary_error) {
      return Err(std::io::Error::new(error.kind(), error.to_string()));
    }
  }
}

fn host_port(uri: &Uri) -> Result<(String, u16), std::io::Error> {
  let host = uri.host().ok_or_else(|| {
    std::io::Error::new(std::io::ErrorKind::InvalidInput, "The URL has no host")
  })?;
  let port = uri
    .port_u16()
    .unwrap_or(if uri.scheme_str() == Some("https") {
      443
    } else {
      80
    });
  Ok((host.to_string(), port))
}

impl Service<Uri> for GuardedHttp {
  type Response = TokioIo<TcpStream>;
  type Error = std::io::Error;
  type Future = Pin<
    Box<dyn Future<Output = Result<TokioIo<TcpStream>, std::io::Error>> + Send>,
  >;

  fn poll_ready(
    &mut self,
    _context: &mut Context<'_>,
  ) -> Poll<Result<(), Self::Error>> {
    Poll::Ready(Ok(()))
  }

  fn call(&mut self, uri: Uri) -> Self::Future {
    Box::pin(async move {
      let (host, port) = host_port(&uri)?;
      match connect_tcp(&host, port).await {
        Ok(stream) => Ok(TokioIo::new(stream)),
        Err(HttpFailure::Denied { name, message }) => {
          Err(denied_io(name, message))
        }
        Err(HttpFailure::Abort) => {
          Err(std::io::Error::other("The connection was aborted"))
        }
        Err(HttpFailure::Type(message) | HttpFailure::Network(message)) => {
          Err(std::io::Error::other(message))
        }
      }
    })
  }
}

#[cfg(test)]
mod tests {
  use super::HttpFailure;

  /// A sized error whose source is the boxed connector error. This is the
  /// shape `hyper_util`'s client error uses.
  #[derive(Debug)]
  struct ConnectLayer(Box<dyn std::error::Error + Send + Sync>);

  impl std::fmt::Display for ConnectLayer {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
      formatter.write_str("client error (Connect)")
    }
  }

  impl std::error::Error for ConnectLayer {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
      Some(self.0.as_ref())
    }
  }

  #[test]
  fn a_boxed_denial_stays_a_denial() {
    let error = super::denied_io(
      "NotAllowedError".to_string(),
      "Requires net access to \"127.0.0.1:9\"".to_string(),
    );
    let HttpFailure::Denied { name, message } =
      super::denied_from(&error).expect("io denial")
    else {
      panic!("not a denial");
    };
    assert_eq!(name, "NotAllowedError");
    assert!(message.contains("127.0.0.1"), "{message}");

    let layer = ConnectLayer(Box::new(error));
    let Some(HttpFailure::Denied { name, message }) =
      super::denied_from(&layer)
    else {
      panic!("not a denial");
    };
    assert_eq!(name, "NotAllowedError");
    assert!(message.contains("127.0.0.1"), "{message}");
  }
}
