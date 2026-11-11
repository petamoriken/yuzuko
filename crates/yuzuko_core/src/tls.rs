//! `connectTls`, `startTls`, and `TlsConn`.
//!
//! The handshake finishes before the promise resolves. Certificate
//! verification uses the web PKI roots plus any `caCerts` PEM the caller
//! passed. `startTls` takes an open `TcpConn` only after the net check.

use std::cell::RefCell;
use std::io::{self, ErrorKind, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use rustls::client::ClientConnection;
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, ServerName};
use rustls::{ClientConfig, RootCertStore, StreamOwned};
use yuzuko_macros::op;

use crate::op::{FromV8, Js, OpError, OpException, ToV8, wrap_instance};
use crate::socket_endpoint::Target;

const IO_WAIT: Duration = Duration::from_millis(50);
const HANDSHAKE_ROUNDS: u8 = 64;

const OPTIONS: &str = "Options must be an object.";
const OPTIONS_READ: &str = "The options could not be read.";
const PORT: &str = "The port must be an integer from 1 through 65535.";
const HOSTNAME: &str = "The hostname must be an IP address or \"localhost\".";
const HOST_INVALID: &str = "The hostname is invalid.";
const CERTS: &str = "caCerts must be an array of PEM strings.";
const CERT: &str = "The certificate is not valid PEM.";
const CONN: &str = "The connection must be a TcpConn.";
const CLOSED: &str = "The socket is closed.";
const BUSY_READ: &str = "A read is already waiting.";
const BUSY_WRITE: &str = "A write is already waiting.";
const NO_PROMISE: &str = "The socket operation failed.";
const ARITY: &str = "Not enough arguments";

thread_local! {
  static OPEN: RefCell<Vec<Weak<Shared>>> = const { RefCell::new(Vec::new()) };
}

struct Shared {
  stream: Mutex<Option<StreamOwned<ClientConnection, TcpStream>>>,
  local: SocketAddr,
  remote: SocketAddr,
  closed: AtomicBool,
  reading: AtomicBool,
  writing: AtomicBool,
}

impl Shared {
  fn is_closed(&self) -> bool {
    self.closed.load(Ordering::Acquire)
  }

  /// Returns whether the connection was already closed.
  fn mark_closed(&self) -> bool {
    let already = self.closed.swap(true, Ordering::AcqRel);
    if !already {
      let stream = {
        let mut slot = lock(&self.stream);
        slot.take()
      };
      if let Some(stream) = stream {
        let _ = stream.sock.shutdown(Shutdown::Both);
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

struct Parts {
  stream: StreamOwned<ClientConnection, TcpStream>,
  local: SocketAddr,
  remote: SocketAddr,
}

enum ReadOut {
  Eof,
  Bytes(Vec<u8>),
}

pub struct TlsConn {
  shared: Arc<Shared>,
}

unsafe impl v8::cppgc::GarbageCollected for TlsConn {
  fn trace(&self, _visitor: &mut v8::cppgc::Visitor) {}

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"TlsConn"
  }
}

impl Drop for TlsConn {
  fn drop(&mut self) {
    let _ = self.shared.mark_closed();
  }
}

#[op(name = "TlsConn")]
impl TlsConn {
  #[getter]
  fn local_addr(&self, scope: &mut v8::PinScope) -> Result<Js, OpError> {
    crate::tcp::address_value(scope, self.shared.local)
  }

  #[getter]
  fn remote_addr(&self, scope: &mut v8::PinScope) -> Result<Js, OpError> {
    crate::tcp::address_value(scope, self.shared.remote)
  }

  fn read(
    &self,
    scope: &mut v8::PinScope,
    buffer: v8::Local<v8::Value>,
  ) -> Result<Js, OpError> {
    let (dest, length) = crate::tcp::read_destination(scope, buffer)?;
    if length == 0 {
      return resolved_count(scope, 0);
    }
    if self.shared.is_closed() {
      return rejected_message(scope, CLOSED);
    }
    if self
      .shared
      .reading
      .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
      .is_err()
    {
      return rejected_message(scope, BUSY_READ);
    }
    let shared = Arc::clone(&self.shared);
    let settled = settle(
      scope,
      crate::tcp::Fail::Read,
      move || {
        let _guard = FlagGuard(&shared.reading);
        read_tls(&shared, length)
      },
      Finish::Read(dest),
    );
    if settled.is_err() {
      self.shared.reading.store(false, Ordering::Release);
    }
    settled
  }

  fn write(
    &self,
    scope: &mut v8::PinScope,
    data: v8::Local<v8::Value>,
  ) -> Result<Js, OpError> {
    let bytes = crate::tcp::copy_source(scope, data)?;
    if bytes.is_empty() {
      return resolved_count(scope, 0);
    }
    if self.shared.is_closed() {
      return rejected_message(scope, CLOSED);
    }
    if self
      .shared
      .writing
      .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
      .is_err()
    {
      return rejected_message(scope, BUSY_WRITE);
    }
    let shared = Arc::clone(&self.shared);
    let settled = settle(
      scope,
      crate::tcp::Fail::Write,
      move || {
        let _guard = FlagGuard(&shared.writing);
        write_tls(&shared, &bytes).map(Deliver::Count)
      },
      Finish::Count,
    );
    if settled.is_err() {
      self.shared.writing.store(false, Ordering::Release);
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

pub(crate) fn install(scope: &mut v8::PinScope) {
  TlsConn::install(scope);
  move_constructor(scope, "TlsConn");
  let yuzuko = crate::namespace(scope, "Yuzuko");
  define_fn(scope, yuzuko, "connectTls", 1, connect_tls);
  define_fn(scope, yuzuko, "startTls", 2, start_tls);
}

pub(crate) fn clear() {
  let open = OPEN.with(|open| std::mem::take(&mut *open.borrow_mut()));
  for weak in open {
    if let Some(shared) = weak.upgrade() {
      let _ = shared.mark_closed();
    }
  }
}

fn connect_tls(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut ret: v8::ReturnValue,
) {
  match connect_op(scope, &args) {
    Ok(value) => value.to_v8(scope, &mut ret),
    Err(error) => OpException::throw(error, scope),
  }
}

fn start_tls(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut ret: v8::ReturnValue,
) {
  match start_op(scope, &args) {
    Ok(value) => value.to_v8(scope, &mut ret),
    Err(error) => OpException::throw(error, scope),
  }
}

fn connect_op(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
) -> Result<Js, OpError> {
  if args.length() < 1 {
    return Err(OpError::type_(ARITY));
  }
  let value = args.get(0);
  if !value.is_object() {
    return Err(OpError::type_(OPTIONS));
  }
  let Ok(object) = v8::Local::<v8::Object>::try_from(value) else {
    return Err(OpError::type_(OPTIONS));
  };
  // Validate certificates before the net check so a bad PEM does not prompt.
  let ders = ca_certs(scope, object)?;
  let target = crate::socket_endpoint::destination(
    scope,
    value,
    1,
    Some(443),
    PORT,
    OPTIONS,
    OPTIONS_READ,
  )?;
  let host = match &target {
    Target::Addr { host, .. } | Target::Name { host, .. } => host.clone(),
  };
  let name =
    ServerName::try_from(host).map_err(|_| OpError::type_(HOST_INVALID))?;
  settle(
    scope,
    crate::tcp::Fail::Connect,
    move || {
      open_tls(target, name, ders).map(|parts| Deliver::Conn(Box::new(parts)))
    },
    Finish::Conn,
  )
}

fn start_op(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
) -> Result<Js, OpError> {
  if args.length() < 2 {
    return Err(OpError::type_(ARITY));
  }
  let value = args.get(0);
  let Ok(conn_object) = v8::Local::<v8::Object>::try_from(value) else {
    return Err(OpError::type_(CONN));
  };
  let port = {
    // Safety: `conn_object` is a live handle. The borrow ends with this block.
    let handle =
      unsafe { crate::op::host_ref::<crate::tcp::Conn>(scope, conn_object) }
        .ok_or_else(|| OpError::type_(CONN))?;
    handle.get().remote_port()
  };
  let options = args.get(1);
  if !options.is_object() {
    return Err(OpError::type_(OPTIONS));
  }
  let Ok(options) = v8::Local::<v8::Object>::try_from(options) else {
    return Err(OpError::type_(OPTIONS));
  };
  let ders = ca_certs(scope, options)?;
  let host = match crate::socket_endpoint::read_field(
    scope,
    options,
    "hostname",
    OPTIONS_READ,
  )? {
    Some(value) if !value.is_undefined() => String::from_v8(scope, value)?,
    _ => "127.0.0.1".to_string(),
  };
  crate::permissions::check_net(&host, port)?;
  let host = host.trim().to_string();
  if host.is_empty() || host.contains('[') || host.contains(']') {
    return Err(OpError::type_(HOSTNAME));
  }
  let name =
    ServerName::try_from(host).map_err(|_| OpError::type_(HOST_INVALID))?;
  let Some(resolver) = v8::PromiseResolver::new(scope) else {
    return Err(OpError::type_(NO_PROMISE));
  };
  let taken = {
    // Safety: the handle is still live. `take_stream` returns an owned socket.
    let handle =
      unsafe { crate::op::host_ref::<crate::tcp::Conn>(scope, conn_object) }
        .ok_or_else(|| OpError::type_(CONN))?;
    handle.get().take_stream()
  };
  let (stream, local, remote) = match taken {
    Ok(parts) => parts,
    Err(crate::tcp::TakeError::BusyRead) => {
      return Err(OpError::type_(BUSY_READ));
    }
    Err(crate::tcp::TakeError::BusyWrite) => {
      return Err(OpError::type_(BUSY_WRITE));
    }
    Err(crate::tcp::TakeError::Closed) => {
      return Err(plain_error(scope, CLOSED));
    }
  };
  let promise = resolver.get_promise(scope);
  let promise_value: v8::Local<v8::Value> = promise.into();
  let promise_global = v8::Global::new(scope, promise_value);
  let resolver_global = v8::Global::new(scope, resolver);
  crate::jobs::spawn(
    move || {
      handshake(stream, name, ders).map(|stream| Parts {
        stream,
        local,
        remote,
      })
    },
    move |scope, result| {
      let resolver = v8::Local::new(scope, &resolver_global);
      let outcome = match result {
        Ok(value) => value.map(|parts| Deliver::Conn(Box::new(parts))),
        Err(()) => Err(crate::tcp::Fail::Handshake),
      };
      let mapped = match outcome {
        Ok(value) => apply(scope, Finish::Conn, value),
        Err(fail) => Err(fail),
      };
      match mapped {
        Ok(value) => {
          let _ = resolver.resolve(scope, value);
        }
        Err(fail) => {
          let error = crate::tcp::fail_value(scope, &fail);
          let _ = resolver.reject(scope, error);
        }
      }
    },
  );
  Ok(Js(promise_global))
}

fn ca_certs(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
) -> Result<Vec<CertificateDer<'static>>, OpError> {
  let value = match crate::socket_endpoint::read_field(
    scope,
    object,
    "caCerts",
    OPTIONS_READ,
  )? {
    Some(value) if !value.is_null_or_undefined() => value,
    _ => return Ok(Vec::new()),
  };
  let Ok(array) = v8::Local::<v8::Array>::try_from(value) else {
    return Err(OpError::type_(CERTS));
  };
  install_ring();
  let mut ders = Vec::new();
  let mut check = RootCertStore::empty();
  for index in 0..array.length() {
    let Some(item) = array.get_index(scope, index) else {
      return Err(OpError::type_(CERTS));
    };
    if !item.is_string() {
      return Err(OpError::type_(CERTS));
    }
    let Some(text) = item.to_string(scope) else {
      return Err(OpError::type_(CERTS));
    };
    let text = text.to_rust_string_lossy(scope);
    for der in parse_pem(&text)? {
      check.add(der.clone()).map_err(|_| OpError::type_(CERT))?;
      ders.push(der);
    }
  }
  Ok(ders)
}

fn parse_pem(text: &str) -> Result<Vec<CertificateDer<'static>>, OpError> {
  let mut out = Vec::new();
  for item in CertificateDer::pem_slice_iter(text.as_bytes()) {
    let der = item.map_err(|_| OpError::type_(CERT))?;
    out.push(der.into_owned());
  }
  if out.is_empty() {
    return Err(OpError::type_(CERT));
  }
  Ok(out)
}

fn open_tls(
  target: Target,
  name: ServerName<'static>,
  ders: Vec<CertificateDer<'static>>,
) -> Result<Parts, crate::tcp::Fail> {
  let parts = crate::tcp::connect_target(target)?;
  let stream = handshake(parts.stream, name, ders)?;
  Ok(Parts {
    stream,
    local: parts.local,
    remote: parts.remote,
  })
}

fn handshake(
  mut tcp: TcpStream,
  name: ServerName<'static>,
  ders: Vec<CertificateDer<'static>>,
) -> Result<StreamOwned<ClientConnection, TcpStream>, crate::tcp::Fail> {
  let _ = tcp.set_read_timeout(None);
  let _ = tcp.set_write_timeout(None);
  install_ring();
  let mut roots = RootCertStore::empty();
  roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
  for der in ders {
    if roots.add(der).is_err() {
      return Err(crate::tcp::Fail::Handshake);
    }
  }
  let config = ClientConfig::builder()
    .with_root_certificates(roots)
    .with_no_client_auth();
  let mut conn = ClientConnection::new(Arc::new(config), name)
    .map_err(|_| crate::tcp::Fail::Handshake)?;
  let mut rounds = 0u8;
  while conn.is_handshaking() {
    if rounds == HANDSHAKE_ROUNDS {
      return Err(crate::tcp::Fail::Handshake);
    }
    rounds += 1;
    match conn.complete_io(&mut tcp) {
      Ok((0, 0)) if conn.is_handshaking() => {
        return Err(crate::tcp::Fail::Handshake);
      }
      Ok(_) => {}
      Err(error) => return Err(handshake_fail(error)),
    }
  }
  if tcp.set_read_timeout(Some(IO_WAIT)).is_err()
    || tcp.set_write_timeout(Some(IO_WAIT)).is_err()
  {
    return Err(crate::tcp::Fail::Handshake);
  }
  let _ = tcp.set_nodelay(true);
  Ok(StreamOwned::new(conn, tcp))
}

fn handshake_fail(error: io::Error) -> crate::tcp::Fail {
  if error.kind() == ErrorKind::InvalidData {
    return crate::tcp::Fail::Handshake;
  }
  match crate::socket_error::classify(error.kind()) {
    Some(class) => crate::tcp::Fail::Class(class),
    None => crate::tcp::Fail::Handshake,
  }
}

fn install_ring() {
  let _ = rustls::crypto::ring::default_provider().install_default();
}

fn read_tls(shared: &Shared, max: usize) -> Result<Deliver, crate::tcp::Fail> {
  if shared.is_closed() {
    return Err(crate::tcp::Fail::Closed);
  }
  let mut stream = {
    let mut slot = lock(&shared.stream);
    let Some(stream) = slot.take() else {
      return Err(crate::tcp::Fail::Closed);
    };
    stream
  };
  let mut buf = vec![0u8; max];
  let outcome = loop {
    if shared.is_closed() {
      break Err(crate::tcp::Fail::Closed);
    }
    match stream.read(&mut buf) {
      Ok(0) if shared.is_closed() => break Err(crate::tcp::Fail::Closed),
      Ok(0) => break Ok(ReadOut::Eof),
      Ok(_) if shared.is_closed() => break Err(crate::tcp::Fail::Closed),
      Ok(count) => {
        buf.truncate(count);
        break Ok(ReadOut::Bytes(buf));
      }
      Err(error) if crate::tcp::is_io_retry(error.kind()) => {
        if shared.is_closed() {
          break Err(crate::tcp::Fail::Closed);
        }
        if error.kind() == ErrorKind::WouldBlock {
          std::thread::sleep(IO_WAIT);
        }
      }
      Err(_) if shared.is_closed() => break Err(crate::tcp::Fail::Closed),
      Err(error) => break Err(io_fail(error, crate::tcp::Fail::Read)),
    }
  };
  restore(shared, stream);
  outcome.map(|value| match value {
    ReadOut::Eof => Deliver::Eof,
    ReadOut::Bytes(bytes) => Deliver::Bytes(bytes),
  })
}

fn write_tls(shared: &Shared, bytes: &[u8]) -> Result<usize, crate::tcp::Fail> {
  if shared.is_closed() {
    return Err(crate::tcp::Fail::Closed);
  }
  let mut stream = {
    let mut slot = lock(&shared.stream);
    let Some(stream) = slot.take() else {
      return Err(crate::tcp::Fail::Closed);
    };
    stream
  };
  let mut offset = 0;
  let outcome = loop {
    if offset == bytes.len() {
      match stream.flush() {
        Ok(()) => break Ok(offset),
        Err(error) if crate::tcp::is_io_retry(error.kind()) => {
          if shared.is_closed() {
            break Err(crate::tcp::Fail::Closed);
          }
          if error.kind() == ErrorKind::WouldBlock {
            std::thread::sleep(IO_WAIT);
          }
          continue;
        }
        Err(_) if shared.is_closed() => break Err(crate::tcp::Fail::Closed),
        Err(error) => break Err(io_fail(error, crate::tcp::Fail::Write)),
      }
    }
    if shared.is_closed() {
      break Err(crate::tcp::Fail::Closed);
    }
    match stream.write(&bytes[offset..]) {
      Ok(0) if shared.is_closed() => break Err(crate::tcp::Fail::Closed),
      Ok(0) => break Err(crate::tcp::Fail::Write),
      Ok(_) if shared.is_closed() => break Err(crate::tcp::Fail::Closed),
      Ok(count) => offset += count,
      Err(error) if crate::tcp::is_io_retry(error.kind()) => {
        if shared.is_closed() {
          break Err(crate::tcp::Fail::Closed);
        }
        if error.kind() == ErrorKind::WouldBlock {
          std::thread::sleep(IO_WAIT);
        }
      }
      Err(_) if shared.is_closed() => break Err(crate::tcp::Fail::Closed),
      Err(error) => break Err(io_fail(error, crate::tcp::Fail::Write)),
    }
  };
  restore(shared, stream);
  outcome
}

fn restore(shared: &Shared, stream: StreamOwned<ClientConnection, TcpStream>) {
  let mut slot = lock(&shared.stream);
  if shared.is_closed() {
    let _ = stream.sock.shutdown(Shutdown::Both);
  } else {
    *slot = Some(stream);
  }
}

fn io_fail(error: io::Error, generic: crate::tcp::Fail) -> crate::tcp::Fail {
  match crate::socket_error::classify(error.kind()) {
    Some(class) => crate::tcp::Fail::Class(class),
    None => generic,
  }
}

fn track(shared: &Arc<Shared>) {
  OPEN.with(|open| {
    let mut open = open.borrow_mut();
    open.retain(|weak| weak.strong_count() > 0);
    open.push(Arc::downgrade(shared));
  });
}

enum Deliver {
  Count(usize),
  Eof,
  Bytes(Vec<u8>),
  Conn(Box<Parts>),
}

enum Finish {
  Count,
  Read(crate::tcp::Destination),
  Conn,
}

fn apply<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  finish: Finish,
  value: Deliver,
) -> Result<v8::Local<'s, v8::Value>, crate::tcp::Fail> {
  match (finish, value) {
    (Finish::Count, Deliver::Count(count)) => {
      Ok(v8::Number::new(scope, count as f64).into())
    }
    (Finish::Read(_), Deliver::Eof) => Ok(v8::null(scope).into()),
    (Finish::Read(dest), Deliver::Bytes(bytes)) => {
      let count = crate::tcp::write_dest(scope, dest, &bytes)?;
      Ok(v8::Number::new(scope, count as f64).into())
    }
    (Finish::Conn, Deliver::Conn(parts)) => conn_value(scope, *parts),
    _ => Err(crate::tcp::Fail::Read),
  }
}

fn conn_value<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  parts: Parts,
) -> Result<v8::Local<'s, v8::Value>, crate::tcp::Fail> {
  let shared = Arc::new(Shared {
    stream: Mutex::new(Some(parts.stream)),
    local: parts.local,
    remote: parts.remote,
    closed: AtomicBool::new(false),
    reading: AtomicBool::new(false),
    writing: AtomicBool::new(false),
  });
  track(&shared);
  // Safety: `TlsConn::install` cached this realm's template.
  let object = unsafe { wrap_instance(scope, TlsConn { shared }) };
  Ok(object.into())
}

fn settle(
  scope: &mut v8::PinScope,
  panicked: crate::tcp::Fail,
  work: impl FnOnce() -> Result<Deliver, crate::tcp::Fail> + Send + 'static,
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
        let error = crate::tcp::fail_value(scope, &fail);
        let _ = resolver.reject(scope, error);
      }
    }
  });
  Ok(Js(promise_global))
}

fn resolved_count(
  scope: &mut v8::PinScope,
  count: usize,
) -> Result<Js, OpError> {
  let Some(resolver) = v8::PromiseResolver::new(scope) else {
    return Err(OpError::type_(NO_PROMISE));
  };
  let value = v8::Number::new(scope, count as f64).into();
  let _ = resolver.resolve(scope, value);
  let promise: v8::Local<v8::Value> = resolver.get_promise(scope).into();
  Ok(Js(v8::Global::new(scope, promise)))
}

fn rejected_message(
  scope: &mut v8::PinScope,
  message: &str,
) -> Result<Js, OpError> {
  let Some(resolver) = v8::PromiseResolver::new(scope) else {
    return Err(OpError::type_(NO_PROMISE));
  };
  let error = plain_message(scope, message);
  let _ = resolver.reject(scope, error);
  let promise: v8::Local<v8::Value> = resolver.get_promise(scope).into();
  Ok(Js(v8::Global::new(scope, promise)))
}

fn plain_message<'s>(
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
