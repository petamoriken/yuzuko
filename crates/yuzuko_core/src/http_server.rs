//! Optional `Yuzuko` HTTP server.
//!
//! The `http` feature installs `listenHttp`, `HttpServer`, and `HttpRequest`
//! on the main realm and on a dedicated worker. `accept` and `respond` run
//! on a background thread and settle on that realm's event loop. A short
//! timeout lets `close` finish a waiting accept when dropping the listening
//! socket does not wake the duplicate descriptor.

use std::cell::RefCell;
use std::io::{self, ErrorKind, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener as StdListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use yuzuko_macros::op;

use crate::op::{FromV8, Js, New, OpError, OpException, ToV8, wrap_instance};

const MAX_HEAD: usize = 65_536;
const MAX_BODY: usize = 1_048_576;
const MAX_TARGET: usize = 8_192;
const IO_WAIT: Duration = Duration::from_millis(50);
/// How long an accepted connection has to deliver its whole request. A
/// client that sends nothing, or sends slowly, cannot hold `accept`.
const REQUEST_WAIT: Duration = Duration::from_secs(10);

const OPTIONS: &str = "Options must be an object.";
const OPTIONS_READ: &str = "The options could not be read.";
const PORT_BIND: &str = "The port must be an integer from 0 through 65535.";
const BIND: &str = "The socket could not bind.";
const CLOSED: &str = "The socket is closed.";
const DATA: &str = "The data must be a string or a BufferSource.";
const DETACHED: &str = "The buffer is detached.";
const TOO_LARGE: &str = "The data is too large.";
const REQUEST_LARGE: &str = "The request is too large.";
const REQUEST: &str = "The request could not be read.";
const ACCEPT: &str = "The connection could not be accepted.";
const WRITE_ERR: &str = "The response could not be written.";
const BUSY_ACCEPT: &str = "An accept is already waiting.";
const BUSY_RESPOND: &str = "A response is already waiting.";
const ALREADY: &str = "The response was already sent.";
const STATUS: &str = "The status must be an integer from 200 through 599.";
const NO_BODY: &str = "A 204 or 304 response cannot have a body.";
const NO_PROMISE: &str = "The socket operation failed.";
const TEXT_TYPE: &str = "text/plain;charset=UTF-8";
const BYTE_TYPE: &str = "application/octet-stream";
const HEADERS: &str = "The headers must be an object.";
const TOO_MANY_HEADERS: &str = "There are too many headers.";
const HEADER_SET: &str = "The header cannot be set.";
const HEADER_NAME: &str = "The header name is invalid.";
const HEADER_VALUE: &str = "The header value is invalid.";
const MAX_HEADERS: usize = 64;

thread_local! {
  static OPEN: RefCell<Vec<Live>> = const { RefCell::new(Vec::new()) };
}

enum Live {
  Server(Weak<ServerShared>),
  Request(Weak<RequestShared>),
}

struct ServerShared {
  listener: Mutex<Option<StdListener>>,
  bound: SocketAddr,
  closed: AtomicBool,
  accepting: AtomicBool,
}

struct RequestShared {
  stream: Mutex<Option<TcpStream>>,
  method: String,
  url: String,
  body: Vec<u8>,
  closed: AtomicBool,
  sent: AtomicBool,
  responding: AtomicBool,
}

#[derive(Debug)]
struct RequestParts {
  stream: TcpStream,
  method: String,
  url: String,
  body: Vec<u8>,
}

struct Head {
  method: String,
  url: String,
  content_length: usize,
}

impl ServerShared {
  fn is_closed(&self) -> bool {
    self.closed.load(Ordering::Acquire)
  }

  /// Returns whether the server was already closed.
  fn mark_closed(&self) -> bool {
    let already = self.closed.swap(true, Ordering::AcqRel);
    if !already {
      let mut slot = lock(&self.listener);
      slot.take();
    }
    already
  }
}

impl RequestShared {
  fn is_closed(&self) -> bool {
    self.closed.load(Ordering::Acquire)
  }

  /// Returns whether the request was already closed.
  fn mark_closed(&self) -> bool {
    let already = self.closed.swap(true, Ordering::AcqRel);
    if !already {
      let stream = {
        let mut slot = lock(&self.stream);
        slot.take()
      };
      if let Some(stream) = stream {
        let _ = stream.shutdown(Shutdown::Both);
      }
    }
    already
  }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
  mutex.lock().unwrap_or_else(|error| error.into_inner())
}

struct FlagGuard<'a>(&'a AtomicBool);

impl Drop for FlagGuard<'_> {
  fn drop(&mut self) {
    self.0.store(false, Ordering::Release);
  }
}

#[derive(Clone, Copy, Debug)]
enum Fail {
  Closed,
  Accept,
  Request,
  TooLarge,
  Write,
  /// A write that returned zero. The class is `WriteZero`, and the message
  /// stays the HTTP sentence rather than the socket sentence.
  Zero,
  Class {
    class: crate::socket_error::Class,
    code: Option<&'static str>,
  },
  /// An operating-system failure with no socket class. `InvalidInput` is a
  /// `TypeError`. A request read that is not `InvalidInput` stays `Request`.
  Io {
    message: &'static str,
    code: Option<&'static str>,
    type_error: bool,
  },
}

impl Fail {
  fn message(self) -> &'static str {
    match self {
      Self::Closed => CLOSED,
      Self::Accept => ACCEPT,
      Self::Request => REQUEST,
      Self::TooLarge => REQUEST_LARGE,
      Self::Write | Self::Zero => WRITE_ERR,
      Self::Class { class, .. } => class.message(),
      Self::Io { message, .. } => message,
    }
  }
}

fn io_fail(error: io::Error, generic: Fail) -> Fail {
  match crate::socket_error::map_io(&error) {
    crate::socket_error::Mapped::Class { class, code } => {
      Fail::Class { class, code }
    }
    crate::socket_error::Mapped::Generic {
      type_error: false, ..
    } if matches!(generic, Fail::Request) => Fail::Request,
    crate::socket_error::Mapped::Generic { code, type_error } => Fail::Io {
      message: generic.message(),
      code,
      type_error,
    },
  }
}

enum Deliver {
  Request(RequestParts),
}

fn track(live: Live) {
  OPEN.with(|open| {
    let mut open = open.borrow_mut();
    open.retain(|item| match item {
      Live::Server(weak) => weak.strong_count() > 0,
      Live::Request(weak) => weak.strong_count() > 0,
    });
    open.push(live);
  });
}

pub(crate) fn clear() {
  let open = OPEN.with(|open| std::mem::take(&mut *open.borrow_mut()));
  for live in open {
    match live {
      Live::Server(weak) => {
        if let Some(shared) = weak.upgrade() {
          let _ = shared.mark_closed();
        }
      }
      Live::Request(weak) => {
        if let Some(shared) = weak.upgrade() {
          let _ = shared.mark_closed();
        }
      }
    }
  }
}

fn bind_http(addr: SocketAddr) -> io::Result<Arc<ServerShared>> {
  let listener = if addr.is_ipv6() {
    crate::socket_bind::tcp_listener_ipv6(addr)?
  } else {
    StdListener::bind(addr)?
  };
  finish_http(listener)
}

fn bind_http_reuse(addr: SocketAddr) -> io::Result<Arc<ServerShared>> {
  finish_http(crate::socket_bind::tcp_listener(addr)?)
}

fn finish_http(listener: StdListener) -> io::Result<Arc<ServerShared>> {
  listener.set_nonblocking(true)?;
  let bound = listener.local_addr()?;
  let shared = Arc::new(ServerShared {
    listener: Mutex::new(Some(listener)),
    bound,
    closed: AtomicBool::new(false),
    accepting: AtomicBool::new(false),
  });
  track(Live::Server(Arc::downgrade(&shared)));
  Ok(shared)
}

fn configure(stream: &TcpStream) -> io::Result<()> {
  stream.set_nonblocking(false)?;
  stream.set_read_timeout(Some(IO_WAIT))?;
  stream.set_write_timeout(Some(IO_WAIT))?;
  let _ = stream.set_nodelay(true);
  Ok(())
}

fn accept_request(shared: &ServerShared) -> Result<RequestParts, Fail> {
  accept_request_within(shared, REQUEST_WAIT)
}

fn accept_request_within(
  shared: &ServerShared,
  wait: Duration,
) -> Result<RequestParts, Fail> {
  if shared.is_closed() {
    return Err(Fail::Closed);
  }
  let listener = {
    let slot = lock(&shared.listener);
    let Some(listener) = slot.as_ref() else {
      return Err(Fail::Closed);
    };
    // Windows does not carry non-blocking mode to a duplicated socket, and a
    // blocking accept would not see `close`.
    let clone = listener
      .try_clone()
      .map_err(|error| io_fail(error, Fail::Accept))?;
    clone
      .set_nonblocking(true)
      .map_err(|error| io_fail(error, Fail::Accept))?;
    clone
  };
  let mut stream = loop {
    if shared.is_closed() {
      return Err(Fail::Closed);
    }
    match listener.accept() {
      Ok((stream, _peer)) => {
        if shared.is_closed() {
          return Err(Fail::Closed);
        }
        break stream;
      }
      Err(error) if crate::socket_endpoint::is_accept_retry(error.kind()) => {
        if shared.is_closed() {
          return Err(Fail::Closed);
        }
        std::thread::sleep(IO_WAIT);
      }
      Err(_) if shared.is_closed() => return Err(Fail::Closed),
      Err(error) => return Err(io_fail(error, Fail::Accept)),
    }
  };
  configure(&stream).map_err(|error| io_fail(error, Fail::Accept))?;
  let deadline = std::time::Instant::now() + wait;
  let parsed = read_request(&mut stream, &shared.closed, deadline)?;
  Ok(RequestParts {
    stream,
    method: parsed.method,
    url: parsed.url,
    body: parsed.body,
  })
}

struct Incoming {
  method: String,
  url: String,
  body: Vec<u8>,
}

fn read_request(
  stream: &mut TcpStream,
  closed: &AtomicBool,
  deadline: std::time::Instant,
) -> Result<Incoming, Fail> {
  let mut buf = Vec::new();
  let mut head: Option<(Head, usize)> = None;
  loop {
    if closed.load(Ordering::Acquire) {
      return Err(Fail::Closed);
    }
    if head.is_none() {
      head = parse_head(&buf)?;
    }
    if let Some((parsed, start)) = &head {
      let (start, length) = (*start, parsed.content_length);
      if buf.len() >= start + length {
        let (owned, _) = head.take().ok_or(Fail::Request)?;
        return Ok(Incoming {
          method: owned.method,
          url: owned.url,
          body: buf[start..start + length].to_vec(),
        });
      }
    }
    if std::time::Instant::now() >= deadline {
      return Err(Fail::Request);
    }
    let count = read_some(stream, &mut buf, closed, deadline)?;
    if count == 0 {
      return Err(if closed.load(Ordering::Acquire) {
        Fail::Closed
      } else {
        Fail::Request
      });
    }
  }
}

fn read_some(
  stream: &mut TcpStream,
  buf: &mut Vec<u8>,
  closed: &AtomicBool,
  deadline: std::time::Instant,
) -> Result<usize, Fail> {
  let mut tmp = [0u8; 8192];
  loop {
    if closed.load(Ordering::Acquire) {
      return Err(Fail::Closed);
    }
    match stream.read(&mut tmp) {
      Ok(0) if closed.load(Ordering::Acquire) => return Err(Fail::Closed),
      Ok(0) => return Ok(0),
      Ok(count) => {
        buf.extend_from_slice(&tmp[..count]);
        return Ok(count);
      }
      Err(error) if crate::socket_endpoint::is_io_retry(error.kind()) => {
        if closed.load(Ordering::Acquire) {
          return Err(Fail::Closed);
        }
        if std::time::Instant::now() >= deadline {
          return Err(Fail::Request);
        }
        if error.kind() == ErrorKind::WouldBlock {
          std::thread::sleep(IO_WAIT);
        }
      }
      Err(_) if closed.load(Ordering::Acquire) => return Err(Fail::Closed),
      Err(error) => return Err(io_fail(error, Fail::Request)),
    }
  }
}

/// Parses the request head with httparse once `buf` holds all of it.
/// Returns the head and the offset of the body, or `None` while the head is
/// still incomplete.
fn parse_head(buf: &[u8]) -> Result<Option<(Head, usize)>, Fail> {
  // The shortest header line is `a:` and a line end, so this many slots
  // cannot run out before the head reaches MAX_HEAD.
  let mut headers = vec![httparse::EMPTY_HEADER; MAX_HEAD / 3];
  let mut request = httparse::Request::new(&mut headers);
  let start = match request.parse(buf) {
    Ok(httparse::Status::Complete(start)) => start,
    Ok(httparse::Status::Partial) if buf.len() > MAX_HEAD => {
      return Err(Fail::TooLarge);
    }
    Ok(httparse::Status::Partial) => return Ok(None),
    Err(httparse::Error::TooManyHeaders) => return Err(Fail::TooLarge),
    Err(_) => return Err(Fail::Request),
  };
  if start > MAX_HEAD {
    return Err(Fail::TooLarge);
  }
  let (Some(method), Some(target)) = (request.method, request.path) else {
    return Err(Fail::Request);
  };
  if method.len() > 64 || !target.starts_with('/') || target.len() > MAX_TARGET
  {
    return Err(Fail::Request);
  }
  let mut content_length: Option<usize> = None;
  for header in request.headers.iter() {
    let value = std::str::from_utf8(header.value)
      .map_err(|_| Fail::Request)?
      .trim();
    if header.name.eq_ignore_ascii_case("content-length") {
      let parsed = parse_content_length(value)?;
      if content_length.is_some_and(|existing| existing != parsed) {
        return Err(Fail::Request);
      }
      content_length = Some(parsed);
    } else if header.name.eq_ignore_ascii_case("transfer-encoding")
      && !is_identity(value)
    {
      return Err(Fail::Request);
    }
  }
  let head = Head {
    method: method.to_string(),
    url: target.to_string(),
    content_length: content_length.unwrap_or(0),
  };
  Ok(Some((head, start)))
}

fn parse_content_length(value: &str) -> Result<usize, Fail> {
  if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
    return Err(Fail::Request);
  }
  let number: u64 = value.parse().map_err(|_| Fail::Request)?;
  if number > MAX_BODY as u64 {
    return Err(Fail::TooLarge);
  }
  Ok(number as usize)
}

fn is_identity(value: &str) -> bool {
  let mut saw = false;
  for coding in value.split(',') {
    let coding = coding.trim();
    if coding.is_empty() || !coding.eq_ignore_ascii_case("identity") {
      return false;
    }
    saw = true;
  }
  saw
}

fn is_token(value: &str) -> bool {
  !value.is_empty()
    && value.bytes().all(|byte| {
      matches!(
        byte,
        b'0'..=b'9'
          | b'A'..=b'Z'
          | b'a'..=b'z'
          | b'!'
          | b'#'
          | b'$'
          | b'%'
          | b'&'
          | b'\''
          | b'*'
          | b'+'
          | b'-'
          | b'.'
          | b'^'
          | b'_'
          | b'`'
          | b'|'
          | b'~'
      )
    })
}

fn response_headers(
  scope: &mut v8::PinScope,
  headers: Option<v8::Local<v8::Value>>,
  default_type: &str,
) -> Result<(String, Vec<(String, String)>), OpError> {
  let Some(value) = headers else {
    return Ok((default_type.to_owned(), Vec::new()));
  };
  if value.is_null_or_undefined() {
    return Ok((default_type.to_owned(), Vec::new()));
  }
  if value.is_array() || !value.is_object() {
    return Err(OpError::type_(HEADERS));
  }
  let Ok(object) = v8::Local::<v8::Object>::try_from(value) else {
    return Err(OpError::type_(HEADERS));
  };
  let names = header_names(scope, object)?;
  if names.len() > MAX_HEADERS {
    return Err(OpError::type_(TOO_MANY_HEADERS));
  }
  let mut content_type = default_type.to_owned();
  let mut extra = Vec::new();
  for name in names {
    if !is_token(&name) {
      return Err(OpError::type_(HEADER_NAME));
    }
    let field = header_field(scope, object, &name)?;
    let lower = name.to_ascii_lowercase();
    if matches!(
      lower.as_str(),
      "content-length" | "transfer-encoding" | "connection"
    ) {
      return Err(OpError::type_(HEADER_SET));
    }
    let text = header_text(scope, field)?;
    if lower == "content-type" {
      content_type = text;
    } else {
      extra.push((name, text));
    }
  }
  Ok((content_type, extra))
}

fn header_names(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
) -> Result<Vec<String>, OpError> {
  enum Outcome {
    Names(v8::Global<v8::Array>),
    Throw(v8::Global<v8::Value>),
    Missing,
  }
  let outcome = {
    v8::tc_scope!(let tc, scope);
    let object = v8::Local::new(tc, object);
    let mut builder = v8::GetPropertyNamesArgsBuilder::new();
    let listed = builder
      .mode(v8::KeyCollectionMode::OwnOnly)
      .property_filter(
        v8::PropertyFilter::ONLY_ENUMERABLE | v8::PropertyFilter::SKIP_SYMBOLS,
      )
      .key_conversion(v8::KeyConversionMode::ConvertToString)
      .build();
    match object.get_own_property_names(tc, listed) {
      Some(names) => Outcome::Names(v8::Global::new(tc, names)),
      None => {
        if let Some(exception) = tc.exception() {
          Outcome::Throw(v8::Global::new(tc, exception))
        } else {
          Outcome::Missing
        }
      }
    }
  };
  let names = match outcome {
    Outcome::Names(names) => v8::Local::new(scope, names),
    Outcome::Throw(exception) => {
      return Err(OpError::thrown(scope, v8::Local::new(scope, exception)));
    }
    Outcome::Missing => return Err(OpError::type_(OPTIONS_READ)),
  };
  let length = names.length();
  let mut keys = Vec::with_capacity(length as usize);
  for index in 0..length {
    let Some(value) = names.get_index(scope, index) else {
      return Err(OpError::type_(OPTIONS_READ));
    };
    if !value.is_string() {
      return Err(OpError::type_(HEADER_NAME));
    }
    let Ok(text) = v8::Local::<v8::String>::try_from(value) else {
      return Err(OpError::type_(HEADER_NAME));
    };
    keys.push(text.to_rust_string_lossy(scope));
  }
  Ok(keys)
}

fn header_field<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  object: v8::Local<'_, v8::Object>,
  name: &str,
) -> Result<v8::Local<'s, v8::Value>, OpError> {
  let Some(key) = v8::String::new(scope, name) else {
    return Err(OpError::type_(HEADER_NAME));
  };
  enum Outcome {
    Value(v8::Global<v8::Value>),
    Throw(v8::Global<v8::Value>),
    Missing,
  }
  let outcome = {
    v8::tc_scope!(let tc, scope);
    let object = v8::Local::new(tc, object);
    let key = v8::Local::new(tc, key);
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
    Outcome::Value(value) => Ok(v8::Local::new(scope, value)),
    Outcome::Throw(exception) => {
      Err(OpError::thrown(scope, v8::Local::new(scope, exception)))
    }
    Outcome::Missing => Err(OpError::type_(OPTIONS_READ)),
  }
}

fn header_text(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<String, OpError> {
  if !value.is_string() {
    return Err(OpError::type_(HEADER_VALUE));
  }
  let Ok(text) = v8::Local::<v8::String>::try_from(value) else {
    return Err(OpError::type_(HEADER_VALUE));
  };
  let text = text.to_rust_string_lossy(scope);
  if !header_text_ok(&text) {
    return Err(OpError::type_(HEADER_VALUE));
  }
  Ok(text)
}

fn header_text_ok(value: &str) -> bool {
  !value
    .bytes()
    .any(|byte| byte == b'\r' || byte == b'\n' || byte == 0)
}

/// RFC 9110 15.3.5 and 15.4.5: a 204 or 304 response ends after its
/// header section, and a 204 response does not send `Content-Length`.
fn bodiless(status: u16) -> bool {
  matches!(status, 204 | 304)
}

fn response_message(
  status: u16,
  content_type: &str,
  extra: &[(String, String)],
  body: &[u8],
) -> Vec<u8> {
  let mut head = format!("HTTP/1.1 {status} {}\r\n", reason(status));
  if !bodiless(status) {
    head.push_str(&format!(
      "Content-Length: {}\r\nContent-Type: {content_type}\r\n",
      body.len()
    ));
  }
  for (name, value) in extra {
    head.push_str(name);
    head.push_str(": ");
    head.push_str(value);
    head.push_str("\r\n");
  }
  head.push_str("Connection: close\r\n\r\n");
  let mut bytes = head.into_bytes();
  bytes.extend_from_slice(body);
  bytes
}

fn reason(status: u16) -> &'static str {
  match status {
    100 => "Continue",
    101 => "Switching Protocols",
    200 => "OK",
    201 => "Created",
    202 => "Accepted",
    204 => "No Content",
    206 => "Partial Content",
    301 => "Moved Permanently",
    302 => "Found",
    304 => "Not Modified",
    400 => "Bad Request",
    401 => "Unauthorized",
    403 => "Forbidden",
    404 => "Not Found",
    405 => "Method Not Allowed",
    408 => "Request Timeout",
    409 => "Conflict",
    411 => "Length Required",
    413 => "Content Too Large",
    500 => "Internal Server Error",
    501 => "Not Implemented",
    502 => "Bad Gateway",
    503 => "Service Unavailable",
    _ => "Status",
  }
}

fn send_response(
  stream: &mut TcpStream,
  closed: &AtomicBool,
  status: u16,
  content_type: &str,
  extra: &[(String, String)],
  body: &[u8],
) -> Result<(), Fail> {
  let bytes = response_message(status, content_type, extra, body);
  write_all(stream, &bytes, closed)?;
  let _ = stream.shutdown(Shutdown::Both);
  Ok(())
}

fn write_all(
  stream: &mut TcpStream,
  bytes: &[u8],
  closed: &AtomicBool,
) -> Result<(), Fail> {
  let mut offset = 0;
  while offset < bytes.len() {
    if closed.load(Ordering::Acquire) {
      return Err(Fail::Closed);
    }
    match stream.write(&bytes[offset..]) {
      Ok(0) => return Err(Fail::Zero),
      Ok(count) => offset += count,
      Err(error) if crate::socket_endpoint::is_io_retry(error.kind()) => {
        if closed.load(Ordering::Acquire) {
          return Err(Fail::Closed);
        }
        if error.kind() == ErrorKind::WouldBlock {
          std::thread::sleep(IO_WAIT);
        }
      }
      Err(_) if closed.load(Ordering::Acquire) => return Err(Fail::Closed),
      Err(error) => return Err(io_fail(error, Fail::Write)),
    }
  }
  Ok(())
}

fn write_shared(
  shared: &RequestShared,
  status: u16,
  content_type: &str,
  extra: &[(String, String)],
  body: &[u8],
) -> Result<(), Fail> {
  if shared.is_closed() {
    return Err(Fail::Closed);
  }
  let mut stream = {
    let slot = lock(&shared.stream);
    let Some(stream) = slot.as_ref() else {
      return Err(Fail::Closed);
    };
    stream
      .try_clone()
      .map_err(|error| io_fail(error, Fail::Write))?
  };
  send_response(
    &mut stream,
    &shared.closed,
    status,
    content_type,
    extra,
    body,
  )
}

pub struct Server {
  shared: Arc<ServerShared>,
}

unsafe impl v8::cppgc::GarbageCollected for Server {
  fn trace(&self, _visitor: &mut v8::cppgc::Visitor) {}

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"HttpServer"
  }
}

impl Drop for Server {
  fn drop(&mut self) {
    let _ = self.shared.mark_closed();
  }
}

pub struct Request {
  shared: Arc<RequestShared>,
}

unsafe impl v8::cppgc::GarbageCollected for Request {
  fn trace(&self, _visitor: &mut v8::cppgc::Visitor) {}

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"HttpRequest"
  }
}

impl Drop for Request {
  fn drop(&mut self) {
    let _ = self.shared.mark_closed();
  }
}

#[op(name = "HttpServer")]
impl Server {
  #[getter]
  fn addr(&self, scope: &mut v8::PinScope) -> Result<Js, OpError> {
    address_value(scope, self.shared.bound)
  }

  fn accept(&self, scope: &mut v8::PinScope) -> Result<Js, OpError> {
    if self.shared.is_closed() {
      return crate::errors::reject(scope, "BadResource", CLOSED, NO_PROMISE);
    }
    if self
      .shared
      .accepting
      .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
      .is_err()
    {
      return rejected(scope, BUSY_ACCEPT);
    }
    let shared = Arc::clone(&self.shared);
    let settled = settle(
      scope,
      Fail::Request,
      move || {
        let _guard = FlagGuard(&shared.accepting);
        accept_request(&shared).map(Deliver::Request)
      },
      Finish::Request,
    );
    if settled.is_err() {
      self.shared.accepting.store(false, Ordering::Release);
    }
    settled
  }

  fn close(&self, scope: &mut v8::PinScope) -> Result<(), OpError> {
    if self.shared.mark_closed() {
      Err(crate::errors::exception(scope, "BadResource", CLOSED))
    } else {
      Ok(())
    }
  }
}

#[op(name = "HttpRequest")]
impl Request {
  #[getter]
  fn method(&self, scope: &mut v8::PinScope) -> Result<Js, OpError> {
    string_value(scope, &self.shared.method)
  }

  #[getter]
  fn url(&self, scope: &mut v8::PinScope) -> Result<Js, OpError> {
    string_value(scope, &self.shared.url)
  }

  #[getter]
  fn body(&self, scope: &mut v8::PinScope) -> Result<Js, OpError> {
    let Some(value) = uint8_array(scope, self.shared.body.clone()) else {
      return Err(OpError::type_(REQUEST));
    };
    let value: v8::Local<v8::Value> = value;
    Ok(Js(v8::Global::new(scope, value)))
  }

  fn respond(
    &self,
    scope: &mut v8::PinScope,
    body: v8::Local<v8::Value>,
    status: Option<v8::Local<v8::Value>>,
    headers: Option<v8::Local<v8::Value>>,
  ) -> Result<Js, OpError> {
    let (bytes, default_type) = payload_bytes(scope, body)?;
    let status = match status {
      Some(value) => to_status(scope, value)?,
      None => 200,
    };
    if bodiless(status) && !bytes.is_empty() {
      return Err(OpError::type_(NO_BODY));
    }
    let (content_type, extra) = response_headers(scope, headers, default_type)?;
    if self.shared.responding.load(Ordering::Acquire) {
      return rejected(scope, BUSY_RESPOND);
    }
    if self.shared.sent.load(Ordering::Acquire) {
      return Err(plain_error(scope, ALREADY));
    }
    if self.shared.is_closed() {
      return crate::errors::reject(scope, "BadResource", CLOSED, NO_PROMISE);
    }
    if self
      .shared
      .responding
      .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
      .is_err()
    {
      return rejected(scope, BUSY_RESPOND);
    }
    let shared = Arc::clone(&self.shared);
    let settled =
      settle_respond(scope, shared, status, content_type, extra, bytes);
    if settled.is_err() {
      self.shared.responding.store(false, Ordering::Release);
    }
    settled
  }
}

enum Finish {
  Request,
}

pub(crate) fn install(scope: &mut v8::PinScope) {
  Server::install(scope);
  Request::install(scope);
  move_constructor(scope, "HttpServer");
  move_constructor(scope, "HttpRequest");
  let yuzuko = crate::namespace(scope, "Yuzuko");
  define_fn(scope, yuzuko, "listenHttp", 0, listen_http);
}

fn listen_http(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut ret: v8::ReturnValue,
) {
  let options = if args.length() == 0 {
    v8::undefined(scope).into()
  } else {
    args.get(0)
  };
  match listen_op(scope, options) {
    Ok(value) => value.to_v8(scope, &mut ret),
    Err(error) => OpException::throw(error, scope),
  }
}

fn listen_op(
  scope: &mut v8::PinScope,
  options: v8::Local<v8::Value>,
) -> Result<New<Server>, OpError> {
  let addr = if options.is_null_or_undefined() {
    crate::socket_endpoint::loopback_bind()?
  } else {
    crate::socket_endpoint::endpoint(
      scope,
      options,
      0,
      PORT_BIND,
      OPTIONS,
      OPTIONS_READ,
    )?
  };
  let reuse = crate::socket_endpoint::reuse_flag(scope, options, OPTIONS_READ)?;
  let shared = match if reuse {
    bind_http_reuse(addr)
  } else {
    bind_http(addr)
  } {
    Ok(shared) => shared,
    Err(error) => {
      return Err(crate::errors::io_exception(scope, &error, BIND));
    }
  };
  Ok(New(Server { shared }))
}

fn to_status(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<u16, OpError> {
  let number = f64::from_v8(scope, value)?;
  if !number.is_finite()
    || number.fract() != 0.0
    || !(200.0..=599.0).contains(&number)
  {
    return Err(OpError::type_(STATUS));
  }
  Ok(number as u16)
}

fn payload_bytes(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<(Vec<u8>, &'static str), OpError> {
  if value.is_string() {
    let text = String::from_v8(scope, value)?;
    if text.len() > MAX_BODY {
      return Err(OpError::type_(TOO_LARGE));
    }
    return Ok((text.into_bytes(), TEXT_TYPE));
  }
  Ok((copy_source(scope, value)?, BYTE_TYPE))
}

fn copy_source(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<Vec<u8>, OpError> {
  crate::js::copy_buffer_source(
    scope,
    value,
    crate::js::BufferCopy::strict(MAX_BODY, TOO_LARGE, DETACHED, DATA),
  )
}

fn address_value(
  scope: &mut v8::PinScope,
  addr: SocketAddr,
) -> Result<Js, OpError> {
  let object = v8::Object::new(scope);
  let Some(key) = v8::String::new(scope, "hostname") else {
    return Err(OpError::type_(OPTIONS_READ));
  };
  let Some(hostname) = v8::String::new(scope, &addr.ip().to_string()) else {
    return Err(OpError::type_(OPTIONS_READ));
  };
  let _ = object.create_data_property(scope, key.into(), hostname.into());
  let Some(key) = v8::String::new(scope, "port") else {
    return Err(OpError::type_(OPTIONS_READ));
  };
  let port = v8::Integer::new(scope, i32::from(addr.port()));
  let _ = object.create_data_property(scope, key.into(), port.into());
  let value: v8::Local<v8::Value> = object.into();
  Ok(Js(v8::Global::new(scope, value)))
}

fn string_value(scope: &mut v8::PinScope, text: &str) -> Result<Js, OpError> {
  let Some(value) = v8::String::new(scope, text) else {
    return Err(OpError::type_(REQUEST));
  };
  let value: v8::Local<v8::Value> = value.into();
  Ok(Js(v8::Global::new(scope, value)))
}

fn uint8_array<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  bytes: Vec<u8>,
) -> Option<v8::Local<'s, v8::Value>> {
  crate::js::uint8_array(scope, bytes).map(|view| view.into())
}

fn request_value<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  parts: RequestParts,
) -> Result<v8::Local<'s, v8::Value>, Fail> {
  let shared = Arc::new(RequestShared {
    stream: Mutex::new(Some(parts.stream)),
    method: parts.method,
    url: parts.url,
    body: parts.body,
    closed: AtomicBool::new(false),
    sent: AtomicBool::new(false),
    responding: AtomicBool::new(false),
  });
  track(Live::Request(Arc::downgrade(&shared)));
  // Safety: `Request::install` cached this realm's `HttpRequest` template.
  let object = unsafe { wrap_instance(scope, Request { shared }) };
  Ok(object.into())
}

fn settle(
  scope: &mut v8::PinScope,
  panicked: Fail,
  work: impl FnOnce() -> Result<Deliver, Fail> + Send + 'static,
  finish: Finish,
) -> Result<Js, OpError> {
  crate::jobs::settle(
    scope, NO_PROMISE, panicked, work, finish, apply, fail_value,
  )
}

fn apply<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  finish: Finish,
  value: Deliver,
) -> Result<v8::Local<'s, v8::Value>, Fail> {
  let Finish::Request = finish;
  let Deliver::Request(parts) = value;
  request_value(scope, parts)
}

fn settle_respond(
  scope: &mut v8::PinScope,
  shared: Arc<RequestShared>,
  status: u16,
  content_type: String,
  extra: Vec<(String, String)>,
  body: Vec<u8>,
) -> Result<Js, OpError> {
  let Some(resolver) = v8::PromiseResolver::new(scope) else {
    return Err(OpError::type_(NO_PROMISE));
  };
  let promise = resolver.get_promise(scope);
  let promise_value: v8::Local<v8::Value> = promise.into();
  let promise_global = v8::Global::new(scope, promise_value);
  let resolver_global = v8::Global::new(scope, resolver);
  let flag = Arc::clone(&shared);
  crate::jobs::spawn(
    move || write_shared(&shared, status, &content_type, &extra, &body),
    move |scope, result| {
      flag.responding.store(false, Ordering::Release);
      let resolver = v8::Local::new(scope, &resolver_global);
      let outcome = match result {
        Ok(value) => value,
        Err(()) => {
          let _ = flag.mark_closed();
          Err(Fail::Write)
        }
      };
      match outcome {
        Ok(()) => {
          flag.sent.store(true, Ordering::Release);
          {
            let mut slot = lock(&flag.stream);
            slot.take();
          }
          let _ = resolver.resolve(scope, v8::undefined(scope).into());
        }
        Err(fail) => {
          if !matches!(fail, Fail::Closed) {
            let _ = flag.mark_closed();
          }
          let error = fail_value(scope, &fail);
          let _ = resolver.reject(scope, error);
        }
      }
    },
  );
  Ok(Js(promise_global))
}

fn rejected(scope: &mut v8::PinScope, message: &str) -> Result<Js, OpError> {
  crate::errors::reject(scope, "Busy", message, NO_PROMISE)
}

fn fail_value<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  fail: &Fail,
) -> v8::Local<'s, v8::Value> {
  match fail {
    Fail::Closed => crate::errors::construct(scope, "BadResource", CLOSED),
    Fail::Request => crate::errors::construct(scope, "Http", REQUEST),
    Fail::Zero => crate::errors::construct(scope, "WriteZero", WRITE_ERR),
    Fail::Class { class, code } => {
      crate::errors::class_value(scope, *class, *code)
    }
    Fail::Io {
      message,
      code,
      type_error,
    } => crate::errors::coded_message(scope, message, *code, *type_error),
    other => error_value(scope, other.message()),
  }
}

use crate::js::define_function as define_fn;
use crate::js::error_exception as error_value;
use crate::js::relocate_constructor as move_constructor;
use crate::js::thrown_error as plain_error;

#[cfg(test)]
mod tests {
  use std::io::{Read, Write};
  use std::sync::atomic::AtomicBool;
  use std::time::Duration;

  use super::{
    Fail, TEXT_TYPE, accept_request, accept_request_within, bind_http,
    parse_head, write_shared,
  };

  #[test]
  fn request_heads_follow_http1_syntax() {
    let (head, start) =
      parse_head(b"POST /a?b HTTP/1.1\r\nContent-Length: 2\r\n\r\nhi")
        .unwrap()
        .unwrap();
    assert_eq!((head.method.as_str(), head.url.as_str()), ("POST", "/a?b"));
    assert_eq!((head.content_length, start), (2, 41));
    assert!(parse_head(b"GET / HTTP/1.1\r\nHost: a").unwrap().is_none());
    for bad in [
      &b"GET / HTTP/2.0\r\n\r\n"[..],
      b"GET http://a/ HTTP/1.1\r\n\r\n",
      b"G(T / HTTP/1.1\r\n\r\n",
      b"GET / HTTP/1.1\r\nBad Name: x\r\n\r\n",
      b"GET / HTTP/1.1\r\nA: b\r\n folded\r\n\r\n",
      b"GET / HTTP/1.1\r\nContent-Length: 1\r\nContent-Length: 2\r\n\r\n",
      b"GET / HTTP/1.1\r\nContent-Length: -1\r\n\r\n",
      b"GET / HTTP/1.1\r\nTransfer-Encoding: gzip\r\n\r\n",
    ] {
      assert!(
        matches!(parse_head(bad), Err(Fail::Request)),
        "{}",
        String::from_utf8_lossy(bad)
      );
    }
  }

  fn header_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|window| window == b"\r\n\r\n")
  }

  fn exchange(port: u16, request: &[u8]) -> Vec<u8> {
    let mut stream = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
      .set_read_timeout(Some(Duration::from_secs(2)))
      .unwrap();
    stream
      .set_write_timeout(Some(Duration::from_secs(2)))
      .unwrap();
    stream.write_all(request).unwrap();
    let mut buf = Vec::new();
    let mut tmp = [0u8; 1024];
    loop {
      match stream.read(&mut tmp) {
        Ok(0) => break,
        Ok(count) => buf.extend_from_slice(&tmp[..count]),
        Err(error) if error.kind() == std::io::ErrorKind::TimedOut => {
          panic!("response timed out: {buf:?}");
        }
        Err(error) => panic!("{error}"),
      }
    }
    buf
  }

  fn split_response(bytes: &[u8]) -> (u16, String, Vec<u8>) {
    let at =
      header_end(bytes).unwrap_or_else(|| panic!("no header in {bytes:?}"));
    let head = std::str::from_utf8(&bytes[..at]).unwrap().to_string();
    let status = head.split(' ').nth(1).unwrap().parse().unwrap();
    (status, head, bytes[at + 4..].to_vec())
  }

  fn request_shared(
    parts: super::RequestParts,
  ) -> std::sync::Arc<super::RequestShared> {
    let shared = std::sync::Arc::new(super::RequestShared {
      stream: std::sync::Mutex::new(Some(parts.stream)),
      method: parts.method,
      url: parts.url,
      body: parts.body,
      closed: AtomicBool::new(false),
      sent: AtomicBool::new(false),
      responding: AtomicBool::new(false),
    });
    super::track(super::Live::Request(std::sync::Arc::downgrade(&shared)));
    shared
  }

  #[test]
  fn local_server_answers_get_and_post() {
    let server = bind_http("127.0.0.1:0".parse().unwrap()).unwrap();
    let port = server.bound.port();
    assert_ne!(port, 0);
    let client = std::thread::spawn(move || {
      exchange(
        port,
        b"GET /hello?x=1 HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n",
      )
    });
    let parts = accept_request(&server).unwrap();
    assert_eq!(parts.method, "GET");
    assert_eq!(parts.url, "/hello?x=1");
    assert!(parts.body.is_empty());
    let shared = request_shared(parts);
    write_shared(&shared, 200, TEXT_TYPE, &[], b"pong").unwrap();
    let (status, head, body) = split_response(&client.join().unwrap());
    assert_eq!(status, 200);
    assert!(head.starts_with("HTTP/1.1 200 OK\r\n"), "{head}");
    assert!(head.contains("Content-Length: 4\r\n"), "{head}");
    assert!(
      head.contains("Content-Type: text/plain;charset=UTF-8\r\n"),
      "{head}"
    );
    assert!(head.ends_with("Connection: close"), "{head}");
    assert_eq!(body, b"pong");

    let client = std::thread::spawn(move || {
      exchange(
        port,
        b"POST /echo HTTP/1.0\r\nHost: 127.0.0.1\r\nContent-Length: 5\r\nTransfer-Encoding: identity\r\n\r\nhello",
      )
    });
    let parts = accept_request(&server).unwrap();
    assert_eq!(parts.method, "POST");
    assert_eq!(parts.url, "/echo");
    assert_eq!(parts.body, b"hello");
    let shared = request_shared(parts);
    write_shared(&shared, 201, "application/octet-stream", &[], b"hello")
      .unwrap();
    let (status, head, body) = split_response(&client.join().unwrap());
    assert_eq!(status, 201);
    assert!(head.contains("201 Created"), "{head}");
    assert_eq!(body, b"hello");
  }

  #[test]
  fn a_slow_or_silent_client_does_not_hold_accept() {
    let server = bind_http("127.0.0.1:0".parse().unwrap()).unwrap();
    let port = server.bound.port();
    let wait = Duration::from_millis(400);

    let silent = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
    let started = std::time::Instant::now();
    let result = accept_request_within(&server, wait);
    assert!(matches!(result, Err(Fail::Request)), "{result:?}");
    assert!(started.elapsed() < Duration::from_secs(3));
    drop(silent);

    // One byte at a time keeps the socket busy but never finishes.
    let slow = std::thread::spawn(move || {
      let mut stream =
        std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
      for byte in b"GET / HTTP/1.1\r\nHost: x\r\n".iter().cycle().take(40) {
        if stream.write_all(&[*byte]).is_err() {
          return;
        }
        std::thread::sleep(Duration::from_millis(30));
      }
    });
    let started = std::time::Instant::now();
    let result = accept_request_within(&server, wait);
    assert!(matches!(result, Err(Fail::Request)), "{result:?}");
    assert!(started.elapsed() < Duration::from_millis(1000));
    slow.join().unwrap();

    // The next client is served.
    let client = std::thread::spawn(move || {
      exchange(port, b"GET /next HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n")
    });
    let parts = accept_request(&server).unwrap();
    assert_eq!(parts.url, "/next");
    let shared = request_shared(parts);
    write_shared(&shared, 200, TEXT_TYPE, &[], b"ok").unwrap();
    let (status, _, body) = split_response(&client.join().unwrap());
    assert_eq!(status, 200);
    assert_eq!(body, b"ok");
  }

  #[test]
  fn a_204_or_304_response_has_no_body_headers() {
    for status in [204u16, 304] {
      let bytes = super::response_message(
        status,
        TEXT_TYPE,
        &[("ETag".to_string(), "\"a\"".to_string())],
        b"",
      );
      let text = String::from_utf8(bytes).unwrap();
      assert!(!text.contains("Content-Length"), "{text}");
      assert!(!text.contains("Content-Type"), "{text}");
      assert!(text.contains("ETag: \"a\"\r\n"), "{text}");
      assert!(text.ends_with("Connection: close\r\n\r\n"), "{text}");
    }
  }

  #[test]
  fn malformed_and_oversized_requests_fail() {
    let server = bind_http("127.0.0.1:0".parse().unwrap()).unwrap();
    let port = server.bound.port();
    let client = std::thread::spawn(move || {
      let mut stream =
        std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
      stream
        .set_write_timeout(Some(Duration::from_secs(2)))
        .unwrap();
      let _ = stream.write_all(b"nope\r\n\r\n");
    });
    let result = accept_request(&server);
    assert!(matches!(result, Err(Fail::Request)), "{result:?}");
    client.join().unwrap();

    let port = server.bound.port();
    let client = std::thread::spawn(move || {
      let mut stream =
        std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
      stream
        .set_write_timeout(Some(Duration::from_secs(2)))
        .unwrap();
      let _ = stream.write_all(
        b"POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n0\r\n\r\n",
      );
    });
    let result = accept_request(&server);
    assert!(matches!(result, Err(Fail::Request)), "{result:?}");
    client.join().unwrap();

    let port = server.bound.port();
    let client = std::thread::spawn(move || {
      let mut stream =
        std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
      stream
        .set_write_timeout(Some(Duration::from_secs(2)))
        .unwrap();
      let _ =
        stream.write_all(b"POST / HTTP/1.1\r\nContent-Length: 1048577\r\n\r\n");
    });
    let result = accept_request(&server);
    assert!(matches!(result, Err(Fail::TooLarge)), "{result:?}");
    client.join().unwrap();

    let port = server.bound.port();
    let client = std::thread::spawn(move || {
      let mut stream =
        std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
      stream
        .set_write_timeout(Some(Duration::from_secs(2)))
        .unwrap();
      let _ = stream.write_all(&vec![b'A'; 70_000]);
    });
    let result = accept_request(&server);
    assert!(matches!(result, Err(Fail::TooLarge)), "{result:?}");
    client.join().unwrap();
  }

  #[test]
  fn close_unblocks_accept() {
    let server = bind_http("127.0.0.1:0".parse().unwrap()).unwrap();
    let waiting = std::sync::Arc::clone(&server);
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
      let _ = tx.send(accept_request(&waiting));
    });
    std::thread::sleep(Duration::from_millis(30));
    assert!(!server.mark_closed());
    let result = rx
      .recv_timeout(Duration::from_secs(2))
      .expect("accept did not finish");
    assert!(matches!(result, Err(Fail::Closed)));
    assert!(server.mark_closed());
  }

  #[test]
  fn clear_closes_only_this_thread() {
    let server = bind_http("127.0.0.1:0".parse().unwrap()).unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
      super::clear();
      tx.send(()).unwrap();
    });
    rx.recv_timeout(Duration::from_secs(2))
      .expect("other thread did not finish");
    assert!(!server.is_closed());
    let port = server.bound.port();
    let client = std::thread::spawn(move || {
      exchange(
        port,
        b"GET / HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n",
      )
    });
    let parts = accept_request(&server).unwrap();
    let shared = request_shared(parts);
    write_shared(&shared, 200, TEXT_TYPE, &[], b"ok").unwrap();
    let (_status, _head, body) = split_response(&client.join().unwrap());
    assert_eq!(body, b"ok");
    super::clear();
    assert!(server.is_closed());
    assert!(shared.is_closed());
  }

  #[test]
  fn bind_rejects_an_unassigned_address() {
    let error = match bind_http("192.0.2.1:0".parse().unwrap()) {
      Err(error) => error,
      Ok(_) => panic!("bind succeeded"),
    };
    assert_eq!(
      error.kind(),
      std::io::ErrorKind::AddrNotAvailable,
      "{error}"
    );
  }
}
