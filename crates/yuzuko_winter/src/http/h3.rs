//! One HTTP/3 request on a fresh QUIC connection.
//!
//! Discovery is the `Alt-Svc` cache. A failure to connect, or a 10 second
//! wait for response headers, drops that cache entry so the caller can use
//! the HTTP/2 or HTTP/1.1 client. The body is read after that budget. The
//! connection is not reused for another fetch.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use bytes::Buf;
use rustls::pki_types::ServerName;

use super::alt_svc;
use super::{BodyNote, HttpBody, HttpFailure, HttpResponse};

pub(super) enum Step {
  Delivered,
  Redirected,
  Skip,
}

pub(super) struct Attempt<'a> {
  pub method: &'a mut String,
  pub url: &'a mut String,
  pub headers: &'a [(String, String)],
  pub body: &'a mut Vec<u8>,
  pub redirect: &'a str,
  pub redirected: &'a mut bool,
  pub redirects: &'a mut u32,
  pub cancel: &'a Arc<AtomicBool>,
  pub sender: &'a std::sync::mpsc::Sender<Result<HttpResponse, HttpFailure>>,
  pub roots: Option<&'a rustls::RootCertStore>,
}

const HEADER_BUDGET: Duration = Duration::from_secs(10);

type H3Stream =
  h3::client::RequestStream<h3_quinn::BidiStream<bytes::Bytes>, bytes::Bytes>;
type H3Driver = h3::client::Connection<h3_quinn::Connection, bytes::Bytes>;
type H3Send = h3::client::SendRequest<h3_quinn::OpenStreams, bytes::Bytes>;

pub(super) async fn attempt(
  mut attempt: Attempt<'_>,
) -> Result<Step, HttpFailure> {
  if alt_svc::lookup(attempt.url).is_none() {
    return Ok(Step::Skip);
  }
  let mut allow_421_retry = true;
  loop {
    if attempt.cancel.load(Ordering::Relaxed) {
      return Err(HttpFailure::Abort);
    }
    let head = response_head(&attempt, allow_421_retry);
    match tokio::time::timeout(HEADER_BUDGET, head).await {
      Ok(Ok(Head::Retry421)) => {
        allow_421_retry = false;
        continue;
      }
      Ok(Ok(Head::Ready(opened))) => {
        return finish_head(&mut attempt, opened).await;
      }
      Ok(Err(HttpFailure::Abort)) => return Err(HttpFailure::Abort),
      Ok(Err(HttpFailure::Type(message))) => {
        return Err(HttpFailure::Type(message));
      }
      Ok(Err(HttpFailure::Network(_))) | Err(_) => {
        alt_svc::invalidate(attempt.url);
        return Ok(Step::Skip);
      }
    }
  }
}

enum Head {
  Ready(Opened),
  /// `421` before any body was delivered. The caller opens another connection.
  Retry421,
}

struct Opened {
  /// Dropping the endpoint closes the QUIC connection, so it outlives the body.
  #[allow(dead_code)]
  endpoint: quinn::Endpoint,
  response: hyper::Response<()>,
  stream: H3Stream,
  driver: H3Driver,
  /// The last `SendRequest` keeps the connection open until the body ends.
  #[allow(dead_code)]
  send_request: H3Send,
}

async fn response_head(
  attempt: &Attempt<'_>,
  allow_421_retry: bool,
) -> Result<Head, HttpFailure> {
  let Some((host, port)) = alt_svc::lookup(attempt.url) else {
    return Err(HttpFailure::Network("no HTTP/3 alternative".to_string()));
  };
  let opened = open(attempt, &host, port).await?;
  alt_svc::observe(attempt.url, opened.response.headers());
  if opened.response.status().as_u16() == 421 && allow_421_retry {
    drop(opened);
    return Ok(Head::Retry421);
  }
  Ok(Head::Ready(opened))
}

async fn finish_head(
  attempt: &mut Attempt<'_>,
  opened: Opened,
) -> Result<Step, HttpFailure> {
  let status = opened.response.status().as_u16();
  if super::is_redirect(status) {
    return redirect(attempt, opened).await;
  }
  deliver(attempt, opened).await?;
  Ok(Step::Delivered)
}

async fn open(
  attempt: &Attempt<'_>,
  host: &str,
  port: u16,
) -> Result<Opened, HttpFailure> {
  super::install_tls();
  let server_name = alt_svc::server_name(attempt.url)
    .ok_or_else(|| HttpFailure::Type("The URL is invalid".to_string()))?;
  // Quinn checks the name before the handshake. An unbracketed IPv6 literal
  // is not a server name; the dial host keeps the brackets.
  let sni = server_name.trim_matches(|ch| ch == '[' || ch == ']');
  if ServerName::try_from(sni).is_err() {
    return Err(HttpFailure::Network(
      "The HTTP/3 server name is invalid".to_string(),
    ));
  }
  let addr = alt_svc::resolve(host, port).await.ok_or_else(|| {
    HttpFailure::Network("HTTP/3 address lookup failed".into())
  })?;
  let bind = match addr {
    std::net::SocketAddr::V4(_) => {
      std::net::SocketAddr::from((std::net::Ipv4Addr::UNSPECIFIED, 0))
    }
    std::net::SocketAddr::V6(_) => {
      std::net::SocketAddr::from((std::net::Ipv6Addr::UNSPECIFIED, 0))
    }
  };
  let mut endpoint = quinn::Endpoint::client(bind)
    .map_err(|error| HttpFailure::Network(error.to_string()))?;
  let crypto = quic_config(attempt.roots)?;
  endpoint.set_default_client_config(crypto);
  let connecting = endpoint
    .connect(addr, sni)
    .map_err(|error| HttpFailure::Network(error.to_string()))?;
  let connection = tokio::select! {
    biased;
    _ = super::wait_cancel(Arc::clone(attempt.cancel)) => {
      return Err(HttpFailure::Abort);
    }
    connection = connecting => connection.map_err(|error| {
      HttpFailure::Network(error.to_string())
    })?,
  };
  let h3_conn = h3_quinn::Connection::new(connection);
  let (mut driver, mut send_request) = h3::client::new(h3_conn)
    .await
    .map_err(|error| HttpFailure::Network(error.to_string()))?;
  let request = build_request(attempt)?;
  let (response, stream) = {
    let request_fut = async {
      let mut stream = send_request.send_request(request).await?;
      if !attempt.body.is_empty() {
        stream
          .send_data(bytes::Bytes::copy_from_slice(attempt.body))
          .await?;
      }
      stream.finish().await?;
      let response = stream.recv_response().await?;
      Ok::<_, h3::error::StreamError>((response, stream))
    };
    tokio::pin!(request_fut);
    tokio::select! {
      biased;
      _ = super::wait_cancel(Arc::clone(attempt.cancel)) => {
        return Err(HttpFailure::Abort);
      }
      result = &mut request_fut => {
        result.map_err(|error| HttpFailure::Network(error.to_string()))?
      }
      closed = std::future::poll_fn(|cx| driver.poll_close(cx)) => {
        return Err(HttpFailure::Network(closed.to_string()));
      }
    }
  };
  Ok(Opened {
    endpoint,
    response,
    stream,
    driver,
    send_request,
  })
}

fn build_request(
  attempt: &Attempt<'_>,
) -> Result<hyper::Request<()>, HttpFailure> {
  let mut builder = hyper::Request::builder()
    .method(attempt.method.as_str())
    .uri(attempt.url.as_str());
  for (name, value) in attempt.headers {
    builder = builder.header(name, value);
  }
  builder
    .body(())
    .map_err(|error| HttpFailure::Type(error.to_string()))
}

fn quic_config(
  roots: Option<&rustls::RootCertStore>,
) -> Result<quinn::ClientConfig, HttpFailure> {
  let provider = Arc::new(rustls::crypto::ring::default_provider());
  let mut crypto = rustls::ClientConfig::builder_with_provider(provider)
    .with_protocol_versions(&[&rustls::version::TLS13])
    .map_err(|error| HttpFailure::Network(error.to_string()))?
    .with_root_certificates(root_store(roots))
    .with_no_client_auth();
  crypto.alpn_protocols = vec![b"h3".to_vec()];
  let crypto = quinn::crypto::rustls::QuicClientConfig::try_from(crypto)
    .map_err(|error| HttpFailure::Network(error.to_string()))?;
  Ok(quinn::ClientConfig::new(Arc::new(crypto)))
}

fn root_store(roots: Option<&rustls::RootCertStore>) -> rustls::RootCertStore {
  if let Some(roots) = roots {
    return roots.clone();
  }
  rustls::RootCertStore {
    roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
  }
}

async fn redirect(
  attempt: &mut Attempt<'_>,
  opened: Opened,
) -> Result<Step, HttpFailure> {
  let status = opened.response.status().as_u16();
  match attempt.redirect {
    "error" => Err(HttpFailure::Type("The redirect was rejected".to_string())),
    "manual" => {
      deliver(attempt, opened).await?;
      Ok(Step::Delivered)
    }
    _ => {
      let location = opened
        .response
        .headers()
        .get(hyper::header::LOCATION)
        .map(|value| super::redirect_location(value.as_bytes()));
      let Some(location) = location else {
        deliver(attempt, opened).await?;
        return Ok(Step::Delivered);
      };
      drop(opened);
      if location.is_empty() {
        return Err(HttpFailure::Type(
          "The redirect URL is invalid".to_string(),
        ));
      }
      super::note_redirect(attempt.redirects)?;
      let next = url::Url::parse(attempt.url)
        .map_err(|_| HttpFailure::Type("The URL is invalid".to_string()))?
        .join(&location)
        .map_err(|_| {
          HttpFailure::Type("The redirect URL is invalid".to_string())
        })?;
      if matches!(status, 303)
        || (matches!(status, 301 | 302)
          && attempt.method.eq_ignore_ascii_case("POST"))
      {
        *attempt.method = "GET".to_string();
        attempt.body.clear();
      }
      *attempt.url = next.to_string();
      *attempt.redirected = true;
      Ok(Step::Redirected)
    }
  }
}

async fn deliver(
  attempt: &Attempt<'_>,
  mut opened: Opened,
) -> Result<(), HttpFailure> {
  let status = opened.response.status().as_u16();
  let status_text = opened
    .response
    .status()
    .canonical_reason()
    .unwrap_or("")
    .to_string();
  let mut headers = opened
    .response
    .headers()
    .iter()
    .map(|(name, value)| {
      (
        name.as_str().to_ascii_lowercase(),
        String::from_utf8_lossy(value.as_bytes()).into_owned(),
      )
    })
    .collect::<Vec<_>>();
  let body = HttpBody::new(Arc::clone(attempt.cancel));
  let null_body = super::is_null_body(attempt.method, status);
  let decoder = if null_body {
    None
  } else {
    super::content_coding::prepare(&mut headers)
  };
  if attempt
    .sender
    .send(Ok(HttpResponse {
      status,
      status_text,
      url: attempt.url.clone(),
      redirected: *attempt.redirected,
      headers,
      body: body.clone(),
      null_body,
    }))
    .is_err()
  {
    return Ok(());
  }
  if null_body {
    body.emit(BodyNote::End);
    drop(opened);
    return Ok(());
  }
  forward(&mut opened, body, decoder).await;
  drop(opened);
  Ok(())
}

async fn forward(
  opened: &mut Opened,
  body: HttpBody,
  mut decoder: Option<super::content_coding::ContentDecoder>,
) {
  loop {
    let stream = &mut opened.stream;
    let driver = &mut opened.driver;
    tokio::select! {
      biased;
      _ = super::wait_cancel(Arc::clone(&body.cancel)) => return,
      chunk = stream.recv_data() => {
        match chunk {
          Ok(Some(mut data)) => {
            let mut bytes = vec![0; data.remaining()];
            data.copy_to_slice(&mut bytes);
            if !push_chunk(&mut decoder, &body, &bytes) {
              return;
            }
          }
          Ok(None) => {
            finish_decoder(&mut decoder, &body);
            return;
          }
          Err(error) => {
            if error.is_h3_no_error() {
              finish_decoder(&mut decoder, &body);
            } else {
              body.emit(BodyNote::Failed(error.to_string()));
            }
            return;
          }
        }
      }
      closed = std::future::poll_fn(|cx| driver.poll_close(cx)) => {
        if closed.is_h3_no_error() {
          finish_decoder(&mut decoder, &body);
        } else {
          body.emit(BodyNote::Failed(closed.to_string()));
        }
        return;
      }
    }
  }
}

fn push_chunk(
  decoder: &mut Option<super::content_coding::ContentDecoder>,
  body: &HttpBody,
  bytes: &[u8],
) -> bool {
  if let Some(decoder) = decoder.as_mut() {
    match decoder.push(bytes, false) {
      Ok(decoded) if !decoded.is_empty() => {
        body.emit(BodyNote::Chunk(decoded));
        true
      }
      Ok(_) => true,
      Err(message) => {
        body.emit(BodyNote::Failed(message));
        false
      }
    }
  } else if !bytes.is_empty() {
    body.emit(BodyNote::Chunk(bytes.to_vec()));
    true
  } else {
    true
  }
}

fn finish_decoder(
  decoder: &mut Option<super::content_coding::ContentDecoder>,
  body: &HttpBody,
) {
  if let Some(decoder) = decoder.as_mut() {
    match decoder.push(&[], true) {
      Ok(bytes) => {
        if !bytes.is_empty() {
          body.emit(BodyNote::Chunk(bytes));
        }
        body.emit(BodyNote::End);
      }
      Err(message) => body.emit(BodyNote::Failed(message)),
    }
  } else {
    body.emit(BodyNote::End);
  }
}
