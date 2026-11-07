//! Optional `Yuzuko` HTTP server.
//!
//! The `http` feature installs `listenHttp`, `HttpServer`, and `HttpRequest`
//! on the main realm and on a dedicated worker. `accept` and `respond` run
//! on a background thread and settle on that realm's event loop. A short
//! timeout lets `close` finish a waiting accept when dropping the listening
//! socket does not wake the duplicate descriptor.

use std::cell::RefCell;
use std::io::{self, ErrorKind, Read, Write};
use std::net::{
  IpAddr, Ipv4Addr, Shutdown, SocketAddr, TcpListener as StdListener, TcpStream,
};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use yuzuko_macros::op;

use crate::op::{FromV8, Js, New, OpError, OpException, ToV8, wrap_instance};

const MAX_HEAD: usize = 65_536;
const MAX_BODY: usize = 1_048_576;
const MAX_TARGET: usize = 8_192;
const IO_WAIT: Duration = Duration::from_millis(50);

const OPTIONS: &str = "Options must be an object.";
const OPTIONS_READ: &str = "The options could not be read.";
const HOSTNAME: &str = "The hostname must be an IP address or \"localhost\".";
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
const STATUS: &str = "The status must be an integer from 100 through 599.";
const NO_PROMISE: &str = "The socket operation failed.";
const TEXT_TYPE: &str = "text/plain;charset=UTF-8";
const BYTE_TYPE: &str = "application/octet-stream";

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
}

impl Fail {
  fn message(self) -> &'static str {
    match self {
      Self::Closed => CLOSED,
      Self::Accept => ACCEPT,
      Self::Request => REQUEST,
      Self::TooLarge => REQUEST_LARGE,
      Self::Write => WRITE_ERR,
    }
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
  let listener = StdListener::bind(addr)?;
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
    let clone = listener.try_clone().map_err(|_| Fail::Accept)?;
    clone.set_nonblocking(true).map_err(|_| Fail::Accept)?;
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
      Err(error) if is_accept_retry(error.kind()) => {
        if shared.is_closed() {
          return Err(Fail::Closed);
        }
        std::thread::sleep(IO_WAIT);
      }
      Err(_) if shared.is_closed() => return Err(Fail::Closed),
      Err(_) => return Err(Fail::Accept),
    }
  };
  configure(&stream).map_err(|_| Fail::Accept)?;
  let parsed = read_request(&mut stream, &shared.closed)?;
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
) -> Result<Incoming, Fail> {
  let mut buf = Vec::new();
  let mut header_at: Option<usize> = None;
  let mut head: Option<Head> = None;
  loop {
    if closed.load(Ordering::Acquire) {
      return Err(Fail::Closed);
    }
    if header_at.is_none()
      && let Some(at) = header_end(&buf)
    {
      if at > MAX_HEAD {
        return Err(Fail::TooLarge);
      }
      let text = std::str::from_utf8(&buf[..at]).map_err(|_| Fail::Request)?;
      head = Some(parse_head(text)?);
      header_at = Some(at);
    } else if header_at.is_none() && buf.len() > MAX_HEAD {
      return Err(Fail::TooLarge);
    }
    let ready = head
      .as_ref()
      .map(|parsed| (header_at, parsed.content_length));
    if let Some((Some(at), length)) = ready {
      let start = at + 4;
      if buf.len() >= start + length {
        let owned = head.take().ok_or(Fail::Request)?;
        return Ok(Incoming {
          method: owned.method,
          url: owned.url,
          body: buf[start..start + length].to_vec(),
        });
      }
    }
    let count = read_some(stream, &mut buf, closed)?;
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
      Err(error) if is_io_retry(error.kind()) => {
        if closed.load(Ordering::Acquire) {
          return Err(Fail::Closed);
        }
        if error.kind() == ErrorKind::WouldBlock {
          std::thread::sleep(IO_WAIT);
        }
      }
      Err(_) if closed.load(Ordering::Acquire) => return Err(Fail::Closed),
      Err(_) => return Err(Fail::Request),
    }
  }
}

fn header_end(buf: &[u8]) -> Option<usize> {
  buf.windows(4).position(|window| window == b"\r\n\r\n")
}

fn parse_head(head: &str) -> Result<Head, Fail> {
  let mut lines = head.split("\r\n");
  let Some(request) = lines.next() else {
    return Err(Fail::Request);
  };
  if request.is_empty() {
    return Err(Fail::Request);
  }
  let mut parts = request.split(' ');
  let (Some(method), Some(target), Some(version)) =
    (parts.next(), parts.next(), parts.next())
  else {
    return Err(Fail::Request);
  };
  if parts.next().is_some()
    || !is_token(method)
    || method.len() > 64
    || !target.starts_with('/')
    || target.len() > MAX_TARGET
    || target.bytes().any(|byte| byte == 0)
    || (version != "HTTP/1.0" && version != "HTTP/1.1")
  {
    return Err(Fail::Request);
  }
  let mut content_length: Option<usize> = None;
  for line in lines {
    if line.is_empty() || line.starts_with(' ') || line.starts_with('\t') {
      return Err(Fail::Request);
    }
    let Some((name, value)) = line.split_once(':') else {
      return Err(Fail::Request);
    };
    if !is_token(name) {
      return Err(Fail::Request);
    }
    let value = value.trim();
    if name.eq_ignore_ascii_case("content-length") {
      let parsed = parse_content_length(value)?;
      if content_length.is_some_and(|existing| existing != parsed) {
        return Err(Fail::Request);
      }
      content_length = Some(parsed);
    } else if name.eq_ignore_ascii_case("transfer-encoding")
      && !is_identity(value)
    {
      return Err(Fail::Request);
    }
  }
  Ok(Head {
    method: method.to_string(),
    url: target.to_string(),
    content_length: content_length.unwrap_or(0),
  })
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

fn is_accept_retry(kind: ErrorKind) -> bool {
  matches!(
    kind,
    ErrorKind::WouldBlock
      | ErrorKind::TimedOut
      | ErrorKind::Interrupted
      | ErrorKind::ConnectionAborted
      | ErrorKind::ConnectionReset
  )
}

fn is_io_retry(kind: ErrorKind) -> bool {
  matches!(
    kind,
    ErrorKind::WouldBlock | ErrorKind::TimedOut | ErrorKind::Interrupted
  )
}

fn response_message(status: u16, content_type: &str, body: &[u8]) -> Vec<u8> {
  let mut head = format!(
    "HTTP/1.1 {status} {}\r\nContent-Length: {}\r\nContent-Type: ",
    reason(status),
    body.len(),
  );
  head.push_str(content_type);
  head.push_str("\r\nConnection: close\r\n\r\n");
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
  body: &[u8],
) -> Result<(), Fail> {
  let bytes = response_message(status, content_type, body);
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
      Ok(0) => return Err(Fail::Write),
      Ok(count) => offset += count,
      Err(error) if is_io_retry(error.kind()) => {
        if closed.load(Ordering::Acquire) {
          return Err(Fail::Closed);
        }
        if error.kind() == ErrorKind::WouldBlock {
          std::thread::sleep(IO_WAIT);
        }
      }
      Err(_) if closed.load(Ordering::Acquire) => return Err(Fail::Closed),
      Err(_) => return Err(Fail::Write),
    }
  }
  Ok(())
}

fn write_shared(
  shared: &RequestShared,
  status: u16,
  content_type: &str,
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
    stream.try_clone().map_err(|_| Fail::Write)?
  };
  send_response(&mut stream, &shared.closed, status, content_type, body)
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
      return rejected(scope, CLOSED);
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
      Err(plain_error(scope, CLOSED))
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
  ) -> Result<Js, OpError> {
    let (bytes, content_type) = payload_bytes(scope, body)?;
    let status = match status {
      Some(value) => to_status(scope, value)?,
      None => 200,
    };
    if self.shared.responding.load(Ordering::Acquire) {
      return rejected(scope, BUSY_RESPOND);
    }
    if self.shared.sent.load(Ordering::Acquire) {
      return Err(plain_error(scope, ALREADY));
    }
    if self.shared.is_closed() {
      return rejected(scope, CLOSED);
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
    let settled = settle_respond(scope, shared, status, content_type, bytes);
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
    SocketAddr::from((Ipv4Addr::LOCALHOST, 0))
  } else {
    endpoint(scope, options)?
  };
  let shared = bind_http(addr).map_err(|_| plain_error(scope, BIND))?;
  Ok(New(Server { shared }))
}

fn endpoint(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<SocketAddr, OpError> {
  if !value.is_object() {
    return Err(OpError::type_(OPTIONS));
  }
  let Ok(object) = v8::Local::<v8::Object>::try_from(value) else {
    return Err(OpError::type_(OPTIONS));
  };
  let hostname = match read_field(scope, object, "hostname")? {
    Some(value) if !value.is_undefined() => String::from_v8(scope, value)?,
    _ => "127.0.0.1".to_string(),
  };
  let port = match read_field(scope, object, "port")? {
    Some(value) if !value.is_undefined() => to_port(scope, value)?,
    _ => 0,
  };
  let ip = parse_host(&hostname)?;
  Ok(SocketAddr::new(ip, port))
}

fn to_port(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<u16, OpError> {
  let number = f64::from_v8(scope, value)?;
  if !number.is_finite()
    || number.fract() != 0.0
    || !(0.0..=65535.0).contains(&number)
  {
    return Err(OpError::type_(PORT_BIND));
  }
  Ok(number as u16)
}

fn to_status(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<u16, OpError> {
  let number = f64::from_v8(scope, value)?;
  if !number.is_finite()
    || number.fract() != 0.0
    || !(100.0..=599.0).contains(&number)
  {
    return Err(OpError::type_(STATUS));
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

fn read_field<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  object: v8::Local<'_, v8::Object>,
  name: &str,
) -> Result<Option<v8::Local<'s, v8::Value>>, OpError> {
  enum Outcome {
    Value(v8::Global<v8::Value>),
    Throw(v8::Global<v8::Value>),
    Missing,
  }
  let outcome = {
    v8::tc_scope!(let tc, scope);
    let Some(key) = v8::String::new(tc, name) else {
      return Err(OpError::type_(OPTIONS_READ));
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
    Outcome::Missing => Err(OpError::type_(OPTIONS_READ)),
  }
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
  if let Ok(buffer) = v8::Local::<v8::SharedArrayBuffer>::try_from(value) {
    return shared_copy(&buffer);
  }
  if let Ok(view) = v8::Local::<v8::ArrayBufferView>::try_from(value) {
    if view
      .buffer(scope)
      .is_some_and(|buffer| buffer.was_detached())
    {
      return Err(OpError::type_(DETACHED));
    }
    let length = view.byte_length();
    if length > MAX_BODY {
      return Err(OpError::type_(TOO_LARGE));
    }
    if length == 0 {
      return Ok(Vec::new());
    }
    let mut bytes = vec![0u8; length];
    let written = view.copy_contents(&mut bytes);
    if written != length {
      return Err(OpError::type_(DETACHED));
    }
    return Ok(bytes);
  }
  if let Ok(buffer) = v8::Local::<v8::ArrayBuffer>::try_from(value) {
    return array_copy(&buffer);
  }
  Err(OpError::type_(DATA))
}

fn shared_copy(buffer: &v8::SharedArrayBuffer) -> Result<Vec<u8>, OpError> {
  let length = buffer.byte_length();
  if length > MAX_BODY {
    return Err(OpError::type_(TOO_LARGE));
  }
  if length == 0 {
    return Ok(Vec::new());
  }
  let mut bytes = vec![0u8; length];
  let store = buffer.get_backing_store();
  let Some(data) = store.data() else {
    return Err(OpError::type_(DETACHED));
  };
  let count = length.min(store.byte_length());
  if count != length {
    return Err(OpError::type_(DETACHED));
  }
  // Safety: `count` fits the destination and the store. The shared
  // reference keeps the store alive for this copy.
  unsafe {
    std::ptr::copy_nonoverlapping(
      data.as_ptr().cast::<u8>(),
      bytes.as_mut_ptr(),
      count,
    );
  }
  Ok(bytes)
}

fn array_copy(buffer: &v8::ArrayBuffer) -> Result<Vec<u8>, OpError> {
  if buffer.was_detached() {
    return Err(OpError::type_(DETACHED));
  }
  let length = buffer.byte_length();
  if length > MAX_BODY {
    return Err(OpError::type_(TOO_LARGE));
  }
  if length == 0 {
    return Ok(Vec::new());
  }
  let mut bytes = vec![0u8; length];
  let Some(data) = buffer.data() else {
    return Err(OpError::type_(DETACHED));
  };
  // Safety: `length` is the buffer's byte length and the destination
  // size. `buffer` keeps the store alive for this copy.
  unsafe {
    std::ptr::copy_nonoverlapping(
      data.as_ptr().cast::<u8>(),
      bytes.as_mut_ptr(),
      length,
    );
  }
  Ok(bytes)
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
  object.set(scope, key.into(), hostname.into());
  let Some(key) = v8::String::new(scope, "port") else {
    return Err(OpError::type_(OPTIONS_READ));
  };
  let port = v8::Integer::new(scope, i32::from(addr.port()));
  object.set(scope, key.into(), port.into());
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
  let length = bytes.len();
  let store = v8::ArrayBuffer::new_backing_store_from_vec(bytes).make_shared();
  let buffer = v8::ArrayBuffer::with_backing_store(scope, &store);
  let view = v8::Uint8Array::new(scope, buffer, 0, length)?;
  Some(view.into())
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
  let Some(resolver) = v8::PromiseResolver::new(scope) else {
    return Err(OpError::type_(NO_PROMISE));
  };
  let promise = resolver.get_promise(scope);
  let promise_value: v8::Local<v8::Value> = promise.into();
  let promise_global = v8::Global::new(scope, promise_value);
  let resolver_global = v8::Global::new(scope, resolver);
  crate::jobs::spawn(work, move |scope, result| {
    let resolver = v8::Local::new(scope, &resolver_global);
    let outcome = match result {
      Ok(value) => value,
      Err(()) => Err(panicked),
    };
    let mapped = match outcome {
      Ok(value) => apply(scope, finish, value),
      Err(fail) => Err(fail),
    };
    match mapped {
      Ok(value) => {
        let _ = resolver.resolve(scope, value);
      }
      Err(fail) => {
        let error = error_value(scope, fail.message());
        let _ = resolver.reject(scope, error);
      }
    }
  });
  Ok(Js(promise_global))
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
  content_type: &'static str,
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
    move || write_shared(&shared, status, content_type, &body),
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
          let error = error_value(scope, fail.message());
          let _ = resolver.reject(scope, error);
        }
      }
    },
  );
  Ok(Js(promise_global))
}

fn rejected(scope: &mut v8::PinScope, message: &str) -> Result<Js, OpError> {
  let Some(resolver) = v8::PromiseResolver::new(scope) else {
    return Err(OpError::type_(NO_PROMISE));
  };
  let error = error_value(scope, message);
  let _ = resolver.reject(scope, error);
  let promise: v8::Local<v8::Value> = resolver.get_promise(scope).into();
  Ok(Js(v8::Global::new(scope, promise)))
}

fn error_value<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  message: &str,
) -> v8::Local<'s, v8::Value> {
  let Some(text) = v8::String::new(scope, message) else {
    return v8::undefined(scope).into();
  };
  v8::Exception::error(scope, text)
}

fn plain_error(scope: &mut v8::PinScope, message: &str) -> OpError {
  let Some(text) = v8::String::new(scope, message) else {
    return OpError::type_(message);
  };
  let error = v8::Exception::error(scope, text);
  OpError::thrown(scope, error)
}

fn move_constructor(scope: &mut v8::PinScope, name: &str) {
  let global = scope.get_current_context().global(scope);
  let Some(key) = v8::String::new(scope, name) else {
    return;
  };
  let Some(constructor) = global.get(scope, key.into()) else {
    return;
  };
  let _ = global.delete(scope, key.into());
  let yuzuko = crate::namespace(scope, "Yuzuko");
  yuzuko.set(scope, key.into(), constructor);
}

fn define_fn(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
  length: i32,
  callback: impl v8::MapFnTo<v8::FunctionCallback>,
) {
  let function = v8::Function::builder(callback)
    .length(length)
    .constructor_behavior(v8::ConstructorBehavior::Throw)
    .build(scope)
    .unwrap_or_else(|| panic!("create function {name}"));
  let key = v8::String::new(scope, name).unwrap();
  function.set_name(key);
  object.set(scope, key.into(), function.into());
}

#[cfg(test)]
mod tests {
  use std::io::{Read, Write};
  use std::sync::atomic::AtomicBool;
  use std::time::Duration;

  use super::{
    Fail, TEXT_TYPE, accept_request, bind_http, header_end, write_shared,
  };

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
    write_shared(&shared, 200, TEXT_TYPE, b"pong").unwrap();
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
    write_shared(&shared, 201, "application/octet-stream", b"hello").unwrap();
    let (status, head, body) = split_response(&client.join().unwrap());
    assert_eq!(status, 201);
    assert!(head.contains("201 Created"), "{head}");
    assert_eq!(body, b"hello");
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
    write_shared(&shared, 200, TEXT_TYPE, b"ok").unwrap();
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
