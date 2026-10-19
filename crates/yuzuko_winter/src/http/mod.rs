//! `http:` and `https:` fetches. Redirects are followed here. The worker
//! thread never touches V8.
//!
//! The fetch promise resolves when the response headers arrive. A side
//! thread keeps the body connection open and forwards chunks until the
//! caller sets `cancel`, which drops the socket.

mod content_coding;
mod request;

pub(crate) mod cache;
pub(crate) mod fetch;
pub(crate) mod form;
pub(crate) mod sse;
pub(crate) mod websocket;
pub(crate) mod xhr;

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, Once};
use std::time::Duration;

use http_body_util::BodyExt;
use hyper_util::client::legacy::Client;
use hyper_util::rt::TokioExecutor;

pub struct HttpResponse {
  pub status: u16,
  pub status_text: String,
  pub url: String,
  pub redirected: bool,
  pub headers: Vec<(String, String)>,
  pub body: HttpBody,
  pub null_body: bool,
}

pub enum BodyNote {
  Chunk(Vec<u8>),
  End,
  Failed(String),
}

type BodyWake = Arc<dyn Fn() + Send + Sync>;

pub struct HttpBody {
  notes: Arc<Mutex<VecDeque<BodyNote>>>,
  wake: Arc<Mutex<Option<BodyWake>>>,
  cancel: Arc<AtomicBool>,
  ready: Arc<Condvar>,
}

impl HttpBody {
  fn new(cancel: Arc<AtomicBool>) -> Self {
    Self {
      notes: Arc::new(Mutex::new(VecDeque::new())),
      wake: Arc::new(Mutex::new(None)),
      cancel,
      ready: Arc::new(Condvar::new()),
    }
  }

  pub fn poll(&self) -> Option<BodyNote> {
    self.notes.lock().expect("http body").pop_front()
  }

  /// Blocks until a note is queued, `stop` runs, or `timeout` elapses.
  pub fn wait_note(&self, timeout: Duration) -> Option<BodyNote> {
    let mut notes = self.notes.lock().expect("http body");
    let deadline = std::time::Instant::now() + timeout;
    loop {
      if let Some(note) = notes.pop_front() {
        return Some(note);
      }
      if self.cancel.load(Ordering::Relaxed) {
        return None;
      }
      let now = std::time::Instant::now();
      if now >= deadline {
        return None;
      }
      let (guard, _) = self
        .ready
        .wait_timeout(notes, deadline.saturating_duration_since(now))
        .expect("http body");
      notes = guard;
    }
  }

  fn queued(&self) -> usize {
    self.notes.lock().expect("http body").len()
  }

  /// Calls `wake` from the body thread when a note is waiting.
  ///
  /// If notes are already queued, `wake` runs on this thread.
  pub fn listen(&self, wake: BodyWake) {
    *self.wake.lock().expect("http body wake") = Some(Arc::clone(&wake));
    if !self.notes.lock().expect("http body").is_empty() {
      wake();
    }
  }

  pub fn stop(&self) {
    self.cancel.store(true, Ordering::Relaxed);
    self.ready.notify_all();
  }
}

impl Clone for HttpBody {
  fn clone(&self) -> Self {
    Self {
      notes: Arc::clone(&self.notes),
      wake: Arc::clone(&self.wake),
      cancel: Arc::clone(&self.cancel),
      ready: Arc::clone(&self.ready),
    }
  }
}

impl HttpBody {
  fn emit(&self, note: BodyNote) {
    self.notes.lock().expect("http body").push_back(note);
    self.ready.notify_all();
    let wake = self.wake.lock().expect("http body wake").clone();
    if let Some(wake) = wake {
      wake();
    }
  }
}

pub enum HttpFailure {
  Abort,
  Type(String),
  Network(String),
}

pub fn perform(
  method: String,
  url: String,
  headers: Vec<(String, String)>,
  body: Vec<u8>,
  redirect: String,
  cancel: Arc<AtomicBool>,
) -> Result<HttpResponse, HttpFailure> {
  let headers = combine_request_headers(headers);
  let (sender, receiver) = std::sync::mpsc::channel();
  std::thread::spawn(move || {
    let runtime = match tokio::runtime::Builder::new_current_thread()
      .enable_all()
      .build()
    {
      Ok(runtime) => runtime,
      Err(error) => {
        let _ = sender.send(Err(HttpFailure::Network(error.to_string())));
        return;
      }
    };
    let outcome = runtime.block_on(exchange(
      method,
      url,
      headers,
      body,
      redirect,
      cancel,
      sender.clone(),
    ));
    if let Err(error) = outcome {
      let _ = sender.send(Err(error));
    }
  });
  receiver.recv().unwrap_or_else(|_| {
    Err(HttpFailure::Network("The connection failed".to_string()))
  })
}

async fn exchange(
  mut method: String,
  mut url: String,
  headers: Vec<(String, String)>,
  mut body: Vec<u8>,
  redirect: String,
  cancel: Arc<AtomicBool>,
  sender: std::sync::mpsc::Sender<Result<HttpResponse, HttpFailure>>,
) -> Result<(), HttpFailure> {
  let mut redirected = false;
  let mut redirects = 0u32;
  if url.starts_with("http://")
    && exchange_http1(
      &mut method,
      &mut url,
      &headers,
      &mut body,
      &redirect,
      &mut redirected,
      &mut redirects,
      &cancel,
      &sender,
    )
    .await?
  {
    return Ok(());
  }
  install_tls();
  let connector = hyper_rustls::HttpsConnectorBuilder::new()
    .with_webpki_roots()
    .https_or_http()
    .enable_http1()
    .build();
  let client = Client::builder(TokioExecutor::new()).build(connector);
  loop {
    if cancel.load(Ordering::Relaxed) {
      return Err(HttpFailure::Abort);
    }
    if header_values_need_raw(&headers) {
      let raw = raw_http1(&method, &url, &headers, &body, &cancel).await?;
      let mut cursor = ExchangeCursor {
        method: &mut method,
        url: &mut url,
        body: &mut body,
        redirect: &redirect,
        redirected: &mut redirected,
        redirects: &mut redirects,
        cancel: &cancel,
        sender: &sender,
      };
      if finish_exchange(raw, &mut cursor)? {
        return Ok(());
      }
      continue;
    }
    let request = build_request(&method, &url, &headers, &body)?;
    let response = tokio::select! {
      biased;
      _ = wait_cancel(cancel.clone()) => {
        return Err(HttpFailure::Abort);
      }
      response = client.request(request) => response,
    };
    let response = match response {
      Ok(response) => response,
      Err(error) => {
        let idempotent = method.eq_ignore_ascii_case("GET")
          || method.eq_ignore_ascii_case("HEAD");
        if idempotent && url.starts_with("http://") {
          let raw = raw_http1(&method, &url, &headers, &body, &cancel).await?;
          let mut cursor = ExchangeCursor {
            method: &mut method,
            url: &mut url,
            body: &mut body,
            redirect: &redirect,
            redirected: &mut redirected,
            redirects: &mut redirects,
            cancel: &cancel,
            sender: &sender,
          };
          if finish_exchange(raw, &mut cursor)? {
            return Ok(());
          }
          continue;
        }
        return Err(HttpFailure::Network(error.to_string()));
      }
    };
    let status = response.status().as_u16();
    if is_redirect(status) {
      match redirect.as_str() {
        "error" => {
          return Err(HttpFailure::Type(
            "The redirect was rejected".to_string(),
          ));
        }
        "manual" => {
          deliver(&method, response, url, false, cancel, &sender).await?;
          return Ok(());
        }
        _ => {
          let location = response.headers().get("location");
          let Some(location) = location else {
            deliver(&method, response, url, redirected, cancel, &sender)
              .await?;
            return Ok(());
          };
          let location = redirect_location(location.as_bytes());
          if location.is_empty() {
            return Err(HttpFailure::Type(
              "The redirect URL is invalid".to_string(),
            ));
          }
          note_redirect(&mut redirects)?;
          let next = url::Url::parse(&url)
            .map_err(|_| HttpFailure::Type("The URL is invalid".to_string()))?
            .join(&location)
            .map_err(|_| {
              HttpFailure::Type("The redirect URL is invalid".to_string())
            })?;
          if matches!(status, 303)
            || (matches!(status, 301 | 302)
              && method.eq_ignore_ascii_case("POST"))
          {
            method = "GET".to_string();
            body.clear();
          }
          url = next.to_string();
          redirected = true;
          continue;
        }
      }
    }
    deliver(&method, response, url, redirected, cancel, &sender).await?;
    return Ok(());
  }
}

fn note_redirect(redirects: &mut u32) -> Result<(), HttpFailure> {
  if *redirects >= 20 {
    return Err(HttpFailure::Type(
      "The redirect count exceeded 20".to_string(),
    ));
  }
  *redirects += 1;
  Ok(())
}

fn is_null_body(method: &str, status: u16) -> bool {
  method.eq_ignore_ascii_case("HEAD") || matches!(status, 101 | 204 | 205 | 304)
}

async fn deliver(
  method: &str,
  response: hyper::Response<hyper::body::Incoming>,
  url: String,
  redirected: bool,
  cancel: Arc<AtomicBool>,
  sender: &std::sync::mpsc::Sender<Result<HttpResponse, HttpFailure>>,
) -> Result<(), HttpFailure> {
  let status = response.status().as_u16();
  let status_text = response
    .status()
    .canonical_reason()
    .unwrap_or("")
    .to_string();
  let headers = response
    .headers()
    .iter()
    .map(|(name, value)| {
      (
        name.as_str().to_ascii_lowercase(),
        String::from_utf8_lossy(value.as_bytes()).into_owned(),
      )
    })
    .collect();
  let incoming = response.into_body();
  let body = HttpBody::new(cancel);
  let null_body = is_null_body(method, status);
  let mut headers = headers;
  let decoder = if null_body {
    None
  } else {
    crate::http::content_coding::prepare(&mut headers)
  };
  if sender
    .send(Ok(HttpResponse {
      status,
      status_text,
      url,
      redirected,
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
    return Ok(());
  }
  forward_body(incoming, body, decoder).await;
  Ok(())
}

async fn forward_body(
  mut incoming: hyper::body::Incoming,
  body: HttpBody,
  mut decoder: Option<crate::http::content_coding::ContentDecoder>,
) {
  loop {
    let frame = tokio::select! {
      biased;
      _ = wait_cancel(Arc::clone(&body.cancel)) => {
        return;
      }
      frame = incoming.frame() => frame,
    };
    match frame {
      Some(Ok(frame)) => {
        let Ok(data) = frame.into_data() else {
          continue;
        };
        if let Some(decoder) = decoder.as_mut() {
          match decoder.push(&data, false) {
            Ok(bytes) if !bytes.is_empty() => {
              body.emit(BodyNote::Chunk(bytes));
            }
            Ok(_) => {}
            Err(message) => {
              body.emit(BodyNote::Failed(message));
              return;
            }
          }
        } else if !data.is_empty() {
          body.emit(BodyNote::Chunk(data.to_vec()));
        }
      }
      Some(Err(error)) => {
        body.emit(BodyNote::Failed(error.to_string()));
        return;
      }
      None => {
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
        return;
      }
    }
  }
}

fn build_request(
  method: &str,
  url: &str,
  headers: &[(String, String)],
  body: &[u8],
) -> Result<hyper::Request<http_body_util::Full<bytes::Bytes>>, HttpFailure> {
  let mut builder = hyper::Request::builder().method(method).uri(url);
  for (name, value) in headers {
    builder = builder.header(name, value);
  }
  builder
    .body(http_body_util::Full::new(bytes::Bytes::copy_from_slice(
      body,
    )))
    .map_err(|error| HttpFailure::Type(error.to_string()))
}

fn is_redirect(status: u16) -> bool {
  matches!(status, 301 | 302 | 303 | 307 | 308)
}

/// One wire header per name, using the first spelling. `set-cookie` stays
/// repeated. Values join with `", "`.
fn combine_request_headers(
  headers: Vec<(String, String)>,
) -> Vec<(String, String)> {
  let mut combined = Vec::new();
  for (name, value) in headers {
    if name.eq_ignore_ascii_case("set-cookie") {
      combined.push((name, value));
      continue;
    }
    if let Some((_, existing)) = combined
      .iter_mut()
      .find(|(stored, _)| stored.eq_ignore_ascii_case(&name))
    {
      existing.push_str(", ");
      existing.push_str(&value);
    } else {
      combined.push((name, value));
    }
  }
  combined
}

fn trim_http_whitespace(value: &str) -> String {
  value
    .trim_matches(|ch: char| matches!(ch, '\t' | '\n' | '\r' | ' '))
    .to_string()
}

async fn wait_cancel(cancel: Arc<AtomicBool>) {
  loop {
    if cancel.load(Ordering::Relaxed) {
      return;
    }
    tokio::time::sleep(Duration::from_millis(10)).await;
  }
}

fn install_tls() {
  static ONCE: Once = Once::new();
  ONCE.call_once(|| {
    let _ = rustls::crypto::ring::default_provider().install_default();
  });
}

struct RawHttp {
  status: u16,
  status_text: String,
  headers: Vec<(String, String)>,
  body: Vec<u8>,
}

/// `http` crate header values reject bytes outside tab and 0x20..=0x7E
/// except 0x7F. Fetch allows those bytes, so the exchange is written on
/// the socket and parsed without that check.
fn header_values_need_raw(headers: &[(String, String)]) -> bool {
  headers.iter().any(|(name, value)| {
    // `http` lowercases names. Fetch echoes the first spelling.
    name.bytes().any(|byte| byte.is_ascii_uppercase())
      || value.chars().any(|ch| {
        let code = u32::from(ch);
        if code > 0xFF {
          return true;
        }
        let byte = code as u8;
        !(byte >= 32 && byte != 127 || byte == b'\t')
      })
  })
}

struct ExchangeCursor<'a> {
  method: &'a mut String,
  url: &'a mut String,
  body: &'a mut Vec<u8>,
  redirect: &'a str,
  redirected: &'a mut bool,
  redirects: &'a mut u32,
  cancel: &'a Arc<AtomicBool>,
  sender: &'a std::sync::mpsc::Sender<Result<HttpResponse, HttpFailure>>,
}

fn finish_exchange(
  raw: RawHttp,
  cursor: &mut ExchangeCursor<'_>,
) -> Result<bool, HttpFailure> {
  if !is_redirect(raw.status) {
    deliver_buffered(
      raw,
      cursor.method,
      cursor.url.clone(),
      *cursor.redirected,
      Arc::clone(cursor.cancel),
      cursor.sender,
    )?;
    return Ok(true);
  }
  match cursor.redirect {
    "error" => Err(HttpFailure::Type("The redirect was rejected".to_string())),
    "manual" => {
      deliver_buffered(
        raw,
        cursor.method,
        cursor.url.clone(),
        false,
        Arc::clone(cursor.cancel),
        cursor.sender,
      )?;
      Ok(true)
    }
    _ => {
      let Some(location) = raw
        .headers
        .iter()
        .find(|(name, _)| name == "location")
        .map(|(_, value)| value.clone())
      else {
        deliver_buffered(
          raw,
          cursor.method,
          cursor.url.clone(),
          *cursor.redirected,
          Arc::clone(cursor.cancel),
          cursor.sender,
        )?;
        return Ok(true);
      };
      if location.is_empty() {
        return Err(HttpFailure::Type(
          "The redirect URL is invalid".to_string(),
        ));
      }
      note_redirect(cursor.redirects)?;
      let location = redirect_location_text(&location);
      let next = url::Url::parse(cursor.url)
        .map_err(|_| HttpFailure::Type("The URL is invalid".to_string()))?
        .join(&location)
        .map_err(|_| {
          HttpFailure::Type("The redirect URL is invalid".to_string())
        })?;
      if matches!(raw.status, 303)
        || (matches!(raw.status, 301 | 302)
          && cursor.method.eq_ignore_ascii_case("POST"))
      {
        *cursor.method = "GET".to_string();
        cursor.body.clear();
      }
      *cursor.url = next.to_string();
      *cursor.redirected = true;
      Ok(false)
    }
  }
}

fn deliver_buffered(
  raw: RawHttp,
  method: &str,
  url: String,
  redirected: bool,
  cancel: Arc<AtomicBool>,
  sender: &std::sync::mpsc::Sender<Result<HttpResponse, HttpFailure>>,
) -> Result<(), HttpFailure> {
  let null_body = is_null_body(method, raw.status);
  let mut headers = raw.headers;
  let decoder = if null_body {
    None
  } else {
    crate::http::content_coding::prepare(&mut headers)
  };
  let body = HttpBody::new(cancel);
  if null_body {
    body.emit(BodyNote::End);
  } else if let Some(mut decoder) = decoder {
    match decoder.finish(&raw.body) {
      Ok(bytes) => {
        if !bytes.is_empty() {
          body.emit(BodyNote::Chunk(bytes));
        }
        body.emit(BodyNote::End);
      }
      Err(message) => body.emit(BodyNote::Failed(message)),
    }
  } else {
    if !raw.body.is_empty() {
      body.emit(BodyNote::Chunk(raw.body));
    }
    body.emit(BodyNote::End);
  }
  sender
    .send(Ok(HttpResponse {
      status: raw.status,
      status_text: raw.status_text,
      url,
      redirected,
      headers,
      body,
      null_body,
    }))
    .map_err(|_| HttpFailure::Network("The fetch was dropped".to_string()))
}

/// `true` when the final response was delivered. `false` when a redirect
/// left an `https:` URL for the Hyper client.
#[allow(clippy::too_many_arguments)]
async fn exchange_http1(
  method: &mut String,
  url: &mut String,
  headers: &[(String, String)],
  body: &mut Vec<u8>,
  redirect: &str,
  redirected: &mut bool,
  redirects: &mut u32,
  cancel: &Arc<AtomicBool>,
  sender: &std::sync::mpsc::Sender<Result<HttpResponse, HttpFailure>>,
) -> Result<bool, HttpFailure> {
  use tokio::io::AsyncWriteExt;
  use tokio::net::TcpStream;

  // Retry 421 once. The first socket stays open until this function
  // returns so its local port is not reused: the partition test counts a
  // connection by client address and port.
  let mut retried_421 = false;
  let mut held_421: Option<TcpStream> = None;
  while url.starts_with("http://") {
    if cancel.load(Ordering::Relaxed) {
      return Err(HttpFailure::Abort);
    }
    let parsed = url::Url::parse(url)
      .map_err(|_| HttpFailure::Type("The URL is invalid".to_string()))?;
    let host = parsed
      .host_str()
      .ok_or_else(|| HttpFailure::Type("The URL has no host".to_string()))?;
    let port = parsed.port_or_known_default().unwrap_or(80);
    let mut stream = TcpStream::connect((host, port))
      .await
      .map_err(|error| HttpFailure::Network(error.to_string()))?;
    let message = raw_request(method, &parsed, headers, body);
    stream
      .write_all(&message)
      .await
      .map_err(|error| HttpFailure::Network(error.to_string()))?;
    let (mut parsed_head, mut rest) =
      read_raw_head(&mut stream, cancel).await?;
    while (100..200).contains(&parsed_head.status) && parsed_head.status != 101
    {
      match next_http1_head(&mut stream, rest, cancel).await {
        Ok((next, next_rest)) => {
          parsed_head = next;
          rest = next_rest;
        }
        Err(_) => {
          return Err(HttpFailure::Type("Failed to fetch".to_string()));
        }
      }
    }
    if parsed_head.status == 421 && !retried_421 {
      retried_421 = true;
      held_421 = Some(stream);
      continue;
    }
    // The second response is in hand, so the first port can close.
    drop(held_421.take());
    if !is_redirect(parsed_head.status) {
      deliver_raw(
        &mut stream,
        parsed_head,
        rest,
        method,
        url.clone(),
        *redirected,
        Arc::clone(cancel),
        sender,
      )
      .await?;
      return Ok(true);
    }
    match redirect {
      "error" => {
        return Err(HttpFailure::Type("The redirect was rejected".to_string()));
      }
      "manual" => {
        deliver_raw(
          &mut stream,
          parsed_head,
          rest,
          method,
          url.clone(),
          false,
          Arc::clone(cancel),
          sender,
        )
        .await?;
        return Ok(true);
      }
      _ => {}
    }
    let location = parsed_head
      .headers
      .iter()
      .find(|(name, _)| name == "location")
      .map(|(_, value)| value.clone());
    let Some(location) = location else {
      deliver_raw(
        &mut stream,
        parsed_head,
        rest,
        method,
        url.clone(),
        *redirected,
        Arc::clone(cancel),
        sender,
      )
      .await?;
      return Ok(true);
    };
    if location.is_empty() {
      return Err(HttpFailure::Type("The redirect URL is invalid".to_string()));
    }
    note_redirect(redirects)?;
    let next =
      parsed
        .join(&redirect_location_text(&location))
        .map_err(|_| {
          HttpFailure::Type("The redirect URL is invalid".to_string())
        })?;
    if matches!(parsed_head.status, 303)
      || (matches!(parsed_head.status, 301 | 302)
        && method.eq_ignore_ascii_case("POST"))
    {
      *method = "GET".to_string();
      body.clear();
    }
    *url = next.to_string();
    *redirected = true;
    drop(stream);
    drop(parsed_head);
    drop(rest);
  }
  Ok(false)
}

fn raw_request(
  method: &str,
  parsed: &url::Url,
  headers: &[(String, String)],
  body: &[u8],
) -> Vec<u8> {
  let mut path = parsed.path().to_string();
  if path.is_empty() {
    path.push('/');
  }
  if let Some(query) = parsed.query() {
    path.push('?');
    path.push_str(query);
  }
  let mut message = Vec::new();
  message.extend(method.as_bytes());
  message.extend(b" ");
  message.extend(path.as_bytes());
  message.extend(b" HTTP/1.1\r\n");
  let mut saw_host = false;
  let mut saw_length = false;
  for (name, value) in headers {
    if name.eq_ignore_ascii_case("host") {
      saw_host = true;
    }
    if name.eq_ignore_ascii_case("content-length") {
      saw_length = true;
    }
    message.extend(name.as_bytes());
    message.extend(b": ");
    message.extend(
      latin1_bytes(value).unwrap_or_else(|_| value.as_bytes().to_vec()),
    );
    message.extend(b"\r\n");
  }
  if !saw_host {
    let host = parsed.host_str().unwrap_or("");
    let host_value = match parsed.port() {
      Some(explicit) => format!("{host}:{explicit}"),
      None => host.to_string(),
    };
    message.extend(format!("Host: {host_value}\r\n").into_bytes());
  }
  if !saw_length {
    message.extend(format!("Content-Length: {}\r\n", body.len()).into_bytes());
  }
  message.extend(b"Connection: close\r\n\r\n");
  message.extend(body);
  message
}

async fn next_http1_head(
  stream: &mut tokio::net::TcpStream,
  mut pending: Vec<u8>,
  cancel: &Arc<AtomicBool>,
) -> Result<(RawHttp, Vec<u8>), HttpFailure> {
  use tokio::io::AsyncReadExt;
  let mut tmp = [0u8; 8192];
  loop {
    if let Some((split, separator)) = find_header_end(&pending) {
      let header_bytes = pending[..split].to_vec();
      let rest = pending[split + separator..].to_vec();
      let parsed = parse_raw_headers(&header_bytes)?;
      return Ok((parsed, rest));
    }
    if cancel.load(Ordering::Relaxed) {
      return Err(HttpFailure::Abort);
    }
    let read = stream
      .read(&mut tmp)
      .await
      .map_err(|error| HttpFailure::Network(error.to_string()))?;
    if read == 0 {
      return Err(HttpFailure::Network(
        "The connection closed before the headers arrived".to_string(),
      ));
    }
    pending.extend_from_slice(&tmp[..read]);
  }
}

async fn read_raw_head(
  stream: &mut tokio::net::TcpStream,
  cancel: &Arc<AtomicBool>,
) -> Result<(RawHttp, Vec<u8>), HttpFailure> {
  use tokio::io::AsyncReadExt;
  let mut buffered = Vec::new();
  let mut tmp = [0u8; 8192];
  let (split, separator) = loop {
    if cancel.load(Ordering::Relaxed) {
      return Err(HttpFailure::Abort);
    }
    let read = stream
      .read(&mut tmp)
      .await
      .map_err(|error| HttpFailure::Network(error.to_string()))?;
    if read == 0 {
      if buffered.is_empty() {
        return Err(HttpFailure::Network(
          "The connection closed before the headers arrived".to_string(),
        ));
      }
      let parsed = parse_raw_headers(&buffered)?;
      return Ok((parsed, Vec::new()));
    }
    buffered.extend_from_slice(&tmp[..read]);
    if let Some(found) = find_header_end(&buffered) {
      break found;
    }
    if buffered.len() > 1024 * 1024 {
      return Err(HttpFailure::Type(
        "The response headers are too large".to_string(),
      ));
    }
  };
  let header_bytes = buffered[..split].to_vec();
  let rest = buffered[split + separator..].to_vec();
  let parsed = parse_raw_headers(&header_bytes)?;
  Ok((parsed, rest))
}

#[allow(clippy::too_many_arguments)]
async fn deliver_raw(
  stream: &mut tokio::net::TcpStream,
  raw: RawHttp,
  rest: Vec<u8>,
  method: &str,
  url: String,
  redirected: bool,
  cancel: Arc<AtomicBool>,
  sender: &std::sync::mpsc::Sender<Result<HttpResponse, HttpFailure>>,
) -> Result<(), HttpFailure> {
  let null_body = is_null_body(method, raw.status);
  let transport = raw.headers.clone();
  let mut headers = raw.headers;
  let decoder = if null_body {
    None
  } else {
    crate::http::content_coding::prepare(&mut headers)
  };
  let body = HttpBody::new(Arc::clone(&cancel));
  if null_body {
    body.emit(BodyNote::End);
  }
  sender
    .send(Ok(HttpResponse {
      status: raw.status,
      status_text: raw.status_text,
      url,
      redirected,
      headers: headers.clone(),
      body: body.clone(),
      null_body,
    }))
    .map_err(|_| HttpFailure::Network("The fetch was dropped".to_string()))?;
  if null_body {
    return Ok(());
  }
  if let Err(error) =
    forward_raw_body(stream, &transport, rest, body.clone(), decoder, &cancel)
      .await
  {
    let HttpFailure::Abort = error else {
      body.emit(BodyNote::Failed(failure_message(&error)));
      return Ok(());
    };
  }
  Ok(())
}

fn failure_message(error: &HttpFailure) -> String {
  match error {
    HttpFailure::Abort => "The operation was aborted".to_string(),
    HttpFailure::Type(message) | HttpFailure::Network(message) => {
      message.clone()
    }
  }
}

async fn forward_raw_body(
  stream: &mut tokio::net::TcpStream,
  headers: &[(String, String)],
  mut pending: Vec<u8>,
  body: HttpBody,
  mut decoder: Option<crate::http::content_coding::ContentDecoder>,
  cancel: &Arc<AtomicBool>,
) -> Result<(), HttpFailure> {
  use tokio::io::AsyncReadExt;
  let chunked = headers.iter().any(|(name, value)| {
    name == "transfer-encoding"
      && value
        .to_ascii_lowercase()
        .split(',')
        .any(|item| item.trim() == "chunked")
  });
  if chunked {
    return forward_chunked(stream, pending, body, decoder, cancel).await;
  }
  if let Some(length) = agreed_content_length(headers) {
    let mut received = 0usize;
    if pending.len() > length {
      pending.truncate(length);
    }
    if length == 0 {
      let _ = emit_decoded(&body, decoder.as_mut(), &[], true);
      return Ok(());
    }
    if !pending.is_empty() {
      received += pending.len();
      if !emit_decoded(&body, decoder.as_mut(), &pending, received == length) {
        return Ok(());
      }
      if received == length {
        return Ok(());
      }
    }
    let mut tmp = [0u8; 8192];
    while received < length {
      if cancel.load(Ordering::Relaxed) {
        return Err(HttpFailure::Abort);
      }
      while body.queued() > 8 {
        if cancel.load(Ordering::Relaxed) {
          return Err(HttpFailure::Abort);
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
      }
      let read = stream
        .read(&mut tmp)
        .await
        .map_err(|error| HttpFailure::Network(error.to_string()))?;
      if read == 0 {
        return Err(HttpFailure::Network(
          "The connection closed before the body ended".to_string(),
        ));
      }
      let take = read.min(length - received);
      received += take;
      if !emit_decoded(
        &body,
        decoder.as_mut(),
        &tmp[..take],
        received == length,
      ) {
        return Ok(());
      }
    }
    return Ok(());
  }
  if !pending.is_empty()
    && !emit_decoded(&body, decoder.as_mut(), &pending, false)
  {
    return Ok(());
  }
  let mut tmp = [0u8; 8192];
  loop {
    if cancel.load(Ordering::Relaxed) {
      return Err(HttpFailure::Abort);
    }
    while body.queued() > 8 {
      if cancel.load(Ordering::Relaxed) {
        return Err(HttpFailure::Abort);
      }
      tokio::time::sleep(Duration::from_millis(5)).await;
    }
    let read = stream
      .read(&mut tmp)
      .await
      .map_err(|error| HttpFailure::Network(error.to_string()))?;
    if read == 0 {
      let _ = emit_decoded(&body, decoder.as_mut(), &[], true);
      return Ok(());
    }
    if !emit_decoded(&body, decoder.as_mut(), &tmp[..read], false) {
      return Ok(());
    }
  }
}

async fn forward_chunked(
  stream: &mut tokio::net::TcpStream,
  mut pending: Vec<u8>,
  body: HttpBody,
  mut decoder: Option<crate::http::content_coding::ContentDecoder>,
  cancel: &Arc<AtomicBool>,
) -> Result<(), HttpFailure> {
  loop {
    while body.queued() > 8 {
      if cancel.load(Ordering::Relaxed) {
        return Err(HttpFailure::Abort);
      }
      tokio::time::sleep(Duration::from_millis(5)).await;
    }
    let line = match read_line(stream, &mut pending, cancel).await {
      Ok(line) => line,
      Err(error) => {
        body.emit(BodyNote::Failed(failure_message(&error)));
        return Ok(());
      }
    };
    let line = latin1_string(&line);
    let size_text = line.split(';').next().unwrap_or("").trim();
    let Ok(size) = usize::from_str_radix(size_text, 16) else {
      body.emit(BodyNote::Failed("A chunk size is invalid".to_string()));
      return Ok(());
    };
    if size == 0 {
      loop {
        match read_line(stream, &mut pending, cancel).await {
          Ok(trailer) if trailer.is_empty() => break,
          Ok(_) => {}
          Err(_) => break,
        }
      }
      let _ = emit_decoded(&body, decoder.as_mut(), &[], true);
      return Ok(());
    }
    let data = match take_bytes(stream, &mut pending, size, cancel).await {
      Ok(data) => data,
      Err(error) => {
        body.emit(BodyNote::Failed(failure_message(&error)));
        return Ok(());
      }
    };
    let ending = match take_bytes(stream, &mut pending, 2, cancel).await {
      Ok(ending) => ending,
      Err(error) => {
        body.emit(BodyNote::Failed(failure_message(&error)));
        return Ok(());
      }
    };
    if ending.as_slice() != b"\r\n" {
      body.emit(BodyNote::Failed("A chunk ending is invalid".to_string()));
      return Ok(());
    }
    if !emit_decoded(&body, decoder.as_mut(), &data, false) {
      return Ok(());
    }
  }
}

fn agreed_content_length(headers: &[(String, String)]) -> Option<usize> {
  let mut found = None;
  for (name, value) in headers {
    if name != "content-length" {
      continue;
    }
    let parsed = value.trim().parse::<usize>().ok()?;
    match found {
      None => found = Some(parsed),
      Some(previous) if previous != parsed => return None,
      Some(_) => {}
    }
  }
  found
}

fn emit_decoded(
  body: &HttpBody,
  decoder: Option<&mut crate::http::content_coding::ContentDecoder>,
  bytes: &[u8],
  end: bool,
) -> bool {
  let Some(decoder) = decoder else {
    if !bytes.is_empty() {
      body.emit(BodyNote::Chunk(bytes.to_vec()));
    }
    if end {
      body.emit(BodyNote::End);
    }
    return true;
  };
  match decoder.push(bytes, end) {
    Ok(decoded) => {
      if !decoded.is_empty() {
        body.emit(BodyNote::Chunk(decoded));
      }
      if end {
        body.emit(BodyNote::End);
      }
      true
    }
    Err(message) => {
      body.emit(BodyNote::Failed(message));
      false
    }
  }
}

fn redirect_location(bytes: &[u8]) -> String {
  const HEX: &[u8; 16] = b"0123456789ABCDEF";
  let mut out = String::new();
  for &byte in bytes {
    if byte < 0x80 {
      out.push(char::from(byte));
    } else {
      out.push('%');
      out.push(char::from(HEX[(byte >> 4) as usize]));
      out.push(char::from(HEX[(byte & 0x0F) as usize]));
    }
  }
  out
}

fn redirect_location_text(value: &str) -> String {
  let bytes = value
    .chars()
    .map(|ch| u32::from(ch) as u8)
    .collect::<Vec<_>>();
  redirect_location(&bytes)
}

async fn raw_http1(
  method: &str,
  url: &str,
  headers: &[(String, String)],
  body: &[u8],
  cancel: &Arc<AtomicBool>,
) -> Result<RawHttp, HttpFailure> {
  let outcome = tokio::time::timeout(
    Duration::from_secs(15),
    raw_http1_exchange(method, url, headers, body, cancel),
  )
  .await;
  match outcome {
    Ok(result) => result,
    Err(_) => Err(HttpFailure::Network("The connection timed out".to_string())),
  }
}

async fn raw_http1_exchange(
  method: &str,
  url: &str,
  headers: &[(String, String)],
  body: &[u8],
  cancel: &Arc<AtomicBool>,
) -> Result<RawHttp, HttpFailure> {
  use tokio::io::AsyncReadExt;
  use tokio::io::AsyncWriteExt;

  let parsed = url::Url::parse(url)
    .map_err(|_| HttpFailure::Type("The URL is invalid".to_string()))?;
  if parsed.scheme() != "http" {
    return Err(HttpFailure::Type(
      "The URL scheme is not supported".to_string(),
    ));
  }
  let host = parsed
    .host_str()
    .ok_or_else(|| HttpFailure::Type("The URL has no host".to_string()))?;
  let port = parsed.port_or_known_default().unwrap_or(80);
  if cancel.load(Ordering::Relaxed) {
    return Err(HttpFailure::Abort);
  }
  let mut stream = tokio::net::TcpStream::connect((host, port))
    .await
    .map_err(|error| HttpFailure::Network(error.to_string()))?;
  let mut path = parsed.path().to_string();
  if path.is_empty() {
    path.push('/');
  }
  if let Some(query) = parsed.query() {
    path.push('?');
    path.push_str(query);
  }
  let mut message = Vec::new();
  message.extend(method.as_bytes());
  message.extend(b" ");
  message.extend(path.as_bytes());
  message.extend(b" HTTP/1.1\r\n");
  let mut saw_host = false;
  let mut saw_length = false;
  for (name, value) in headers {
    if name.eq_ignore_ascii_case("host") {
      saw_host = true;
    }
    if name.eq_ignore_ascii_case("content-length") {
      saw_length = true;
    }
    message.extend(name.as_bytes());
    message.extend(b": ");
    message.extend(latin1_bytes(value)?);
    message.extend(b"\r\n");
  }
  if !saw_host {
    let host_value = match parsed.port() {
      Some(explicit) => format!("{host}:{explicit}"),
      None => host.to_string(),
    };
    message.extend(format!("Host: {host_value}\r\n").into_bytes());
  }
  if !saw_length {
    message.extend(format!("Content-Length: {}\r\n", body.len()).into_bytes());
  }
  message.extend(b"Connection: close\r\n\r\n");
  message.extend(body);
  stream
    .write_all(&message)
    .await
    .map_err(|error| HttpFailure::Network(error.to_string()))?;
  let mut buffered = Vec::new();
  let mut tmp = [0u8; 8192];
  let split = loop {
    if cancel.load(Ordering::Relaxed) {
      return Err(HttpFailure::Abort);
    }
    let read = stream
      .read(&mut tmp)
      .await
      .map_err(|error| HttpFailure::Network(error.to_string()))?;
    if read == 0 {
      if buffered.is_empty() {
        return Err(HttpFailure::Network(
          "The connection closed before the headers arrived".to_string(),
        ));
      }
      // Fixtures such as `*.asis` end the header block by closing the socket.
      let mut parsed = parse_raw_headers(&buffered)?;
      parsed.body = Vec::new();
      return Ok(parsed);
    }
    buffered.extend_from_slice(&tmp[..read]);
    if let Some(found) = find_header_end(&buffered) {
      break found;
    }
    if buffered.len() > 1024 * 1024 {
      return Err(HttpFailure::Type(
        "The response headers are too large".to_string(),
      ));
    }
  };
  let (split, separator) = split;
  let header_bytes = buffered[..split].to_vec();
  let rest = buffered[split + separator..].to_vec();
  let mut parsed = parse_raw_headers(&header_bytes)?;
  parsed.body =
    read_raw_body(&mut stream, &parsed.headers, rest, cancel).await?;
  Ok(parsed)
}

fn latin1_bytes(value: &str) -> Result<Vec<u8>, HttpFailure> {
  let mut bytes = Vec::with_capacity(value.len());
  for ch in value.chars() {
    let code = u32::from(ch);
    if code > 0xFF {
      return Err(HttpFailure::Type("The header value is invalid".to_string()));
    }
    bytes.push(code as u8);
  }
  Ok(bytes)
}

fn latin1_string(bytes: &[u8]) -> String {
  bytes.iter().copied().map(char::from).collect()
}

fn find_header_end(bytes: &[u8]) -> Option<(usize, usize)> {
  if let Some(index) = bytes.windows(4).position(|window| window == b"\r\n\r\n")
  {
    return Some((index, 4));
  }
  bytes
    .windows(2)
    .position(|window| window == b"\n\n")
    .map(|index| (index, 2))
}

fn parse_raw_headers(bytes: &[u8]) -> Result<RawHttp, HttpFailure> {
  let lines = split_crlf(bytes);
  let Some(status_line) = lines.first() else {
    return Err(HttpFailure::Network(
      "The status line is missing".to_string(),
    ));
  };
  let status_line = latin1_string(status_line);
  let mut parts = status_line.splitn(3, ' ');
  let _version = parts.next();
  let code = parts
    .next()
    .and_then(|text| text.parse::<u16>().ok())
    .ok_or_else(|| {
      HttpFailure::Network("The status line is invalid".to_string())
    })?;
  let status_text = parts.next().unwrap_or("").to_string();
  let mut headers = Vec::new();
  for line in lines.iter().skip(1) {
    if line.is_empty() {
      continue;
    }
    let Some(colon) = line.iter().position(|byte| *byte == b':') else {
      return Err(HttpFailure::Network(
        "A response header is invalid".to_string(),
      ));
    };
    let name = latin1_string(&line[..colon]).trim().to_ascii_lowercase();
    let mut value = &line[colon + 1..];
    while value
      .first()
      .is_some_and(|byte| *byte == b' ' || *byte == b'\t')
    {
      value = &value[1..];
    }
    if value.contains(&0) {
      return Err(HttpFailure::Type("The header value is invalid".to_string()));
    }
    headers.push((name, trim_http_whitespace(&latin1_string(value))));
  }
  Ok(RawHttp {
    status: code,
    status_text,
    headers,
    body: Vec::new(),
  })
}

fn split_crlf(bytes: &[u8]) -> Vec<&[u8]> {
  let mut lines = Vec::new();
  let mut start = 0;
  let mut index = 0;
  while index < bytes.len() {
    if bytes[index] == b'\n' {
      let mut end = index;
      if end > start && bytes[end - 1] == b'\r' {
        end -= 1;
      }
      lines.push(&bytes[start..end]);
      index += 1;
      start = index;
    } else {
      index += 1;
    }
  }
  if start < bytes.len() {
    let mut end = bytes.len();
    if end > start && bytes[end - 1] == b'\r' {
      end -= 1;
    }
    lines.push(&bytes[start..end]);
  }
  lines
}

async fn read_raw_body(
  stream: &mut tokio::net::TcpStream,
  headers: &[(String, String)],
  rest: Vec<u8>,
  cancel: &Arc<AtomicBool>,
) -> Result<Vec<u8>, HttpFailure> {
  let chunked = headers.iter().any(|(name, value)| {
    name == "transfer-encoding"
      && value
        .to_ascii_lowercase()
        .split(',')
        .any(|item| item.trim() == "chunked")
  });
  if chunked {
    return read_chunked(stream, rest, cancel).await;
  }
  if let Some(length) = headers.iter().find_map(|(name, value)| {
    if name == "content-length" {
      value.trim().parse::<usize>().ok()
    } else {
      None
    }
  }) {
    return read_limited(stream, rest, length, cancel).await;
  }
  read_until_end(stream, rest, cancel).await
}

async fn read_limited(
  stream: &mut tokio::net::TcpStream,
  mut body: Vec<u8>,
  length: usize,
  cancel: &Arc<AtomicBool>,
) -> Result<Vec<u8>, HttpFailure> {
  use tokio::io::AsyncReadExt;
  if body.len() > length {
    body.truncate(length);
    return Ok(body);
  }
  let mut tmp = [0u8; 8192];
  while body.len() < length {
    if cancel.load(Ordering::Relaxed) {
      return Err(HttpFailure::Abort);
    }
    let read = stream
      .read(&mut tmp)
      .await
      .map_err(|error| HttpFailure::Network(error.to_string()))?;
    if read == 0 {
      return Err(HttpFailure::Network(
        "The connection closed before the body ended".to_string(),
      ));
    }
    let need = length - body.len();
    body.extend_from_slice(&tmp[..read.min(need)]);
  }
  Ok(body)
}

async fn read_until_end(
  stream: &mut tokio::net::TcpStream,
  mut body: Vec<u8>,
  cancel: &Arc<AtomicBool>,
) -> Result<Vec<u8>, HttpFailure> {
  use tokio::io::AsyncReadExt;
  let mut tmp = [0u8; 8192];
  loop {
    if cancel.load(Ordering::Relaxed) {
      return Err(HttpFailure::Abort);
    }
    let read = stream
      .read(&mut tmp)
      .await
      .map_err(|error| HttpFailure::Network(error.to_string()))?;
    if read == 0 {
      return Ok(body);
    }
    body.extend_from_slice(&tmp[..read]);
    if body.len() > 64 * 1024 * 1024 {
      return Err(HttpFailure::Type("The response body is too large".into()));
    }
  }
}

async fn read_chunked(
  stream: &mut tokio::net::TcpStream,
  mut pending: Vec<u8>,
  cancel: &Arc<AtomicBool>,
) -> Result<Vec<u8>, HttpFailure> {
  let mut body = Vec::new();
  loop {
    let line = read_line(stream, &mut pending, cancel).await?;
    let line = latin1_string(&line);
    let size_text = line.split(';').next().unwrap_or("").trim();
    let size = usize::from_str_radix(size_text, 16).map_err(|_| {
      HttpFailure::Network("A chunk size is invalid".to_string())
    })?;
    if size == 0 {
      loop {
        let trailer = read_line(stream, &mut pending, cancel).await?;
        if trailer.is_empty() {
          break;
        }
      }
      return Ok(body);
    }
    let data = take_bytes(stream, &mut pending, size, cancel).await?;
    body.extend(data);
    let ending = take_bytes(stream, &mut pending, 2, cancel).await?;
    if ending.as_slice() != b"\r\n" {
      return Err(HttpFailure::Network(
        "A chunk ending is invalid".to_string(),
      ));
    }
  }
}

async fn read_line(
  stream: &mut tokio::net::TcpStream,
  pending: &mut Vec<u8>,
  cancel: &Arc<AtomicBool>,
) -> Result<Vec<u8>, HttpFailure> {
  use tokio::io::AsyncReadExt;
  let mut line = Vec::new();
  let mut tmp = [0u8; 1];
  loop {
    let byte = if !pending.is_empty() {
      pending.remove(0)
    } else {
      if cancel.load(Ordering::Relaxed) {
        return Err(HttpFailure::Abort);
      }
      let read = stream
        .read(&mut tmp)
        .await
        .map_err(|error| HttpFailure::Network(error.to_string()))?;
      if read == 0 {
        return Err(HttpFailure::Network(
          "The connection closed before the body ended".to_string(),
        ));
      }
      tmp[0]
    };
    if byte == b'\n' {
      if line.last() == Some(&b'\r') {
        line.pop();
      }
      return Ok(line);
    }
    line.push(byte);
  }
}

async fn take_bytes(
  stream: &mut tokio::net::TcpStream,
  pending: &mut Vec<u8>,
  length: usize,
  cancel: &Arc<AtomicBool>,
) -> Result<Vec<u8>, HttpFailure> {
  use tokio::io::AsyncReadExt;
  let mut out = Vec::with_capacity(length);
  while out.len() < length {
    if !pending.is_empty() {
      let need = length - out.len();
      let take = need.min(pending.len());
      out.extend(pending.drain(..take));
      continue;
    }
    if cancel.load(Ordering::Relaxed) {
      return Err(HttpFailure::Abort);
    }
    let mut tmp = [0u8; 8192];
    let read = stream
      .read(&mut tmp)
      .await
      .map_err(|error| HttpFailure::Network(error.to_string()))?;
    if read == 0 {
      return Err(HttpFailure::Network(
        "The connection closed before the body ended".to_string(),
      ));
    }
    pending.extend_from_slice(&tmp[..read]);
  }
  Ok(out)
}
