//! Optional `Yuzuko` TCP sockets.
//!
//! The `tcp` feature installs `listenTcp`, `connectTcp`, `TcpListener`, and
//! `TcpConn` on the main realm and on a dedicated worker. `accept`,
//! `connect`, `read`, and `write` run on a background thread and settle on
//! that realm's event loop. A short timeout lets `close` finish a waiting
//! call when dropping the socket does not wake the duplicate descriptor.

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

const MAX_BUFFER: usize = 2_147_483_647;
const IO_WAIT: Duration = Duration::from_millis(50);

const OPTIONS: &str = "Options must be an object.";
const OPTIONS_READ: &str = "The options could not be read.";
const ADDRESS: &str = "The address must be an object.";
const ADDRESS_READ: &str = "The address could not be read.";
const HOSTNAME: &str = "The hostname must be an IP address or \"localhost\".";
const PORT_BIND: &str = "The port must be an integer from 0 through 65535.";
const PORT_DEST: &str = "The port must be an integer from 1 through 65535.";
const BIND: &str = "The socket could not bind.";
const CLOSED: &str = "The socket is closed.";
const DATA: &str = "The data must be a BufferSource.";
const DETACHED: &str = "The buffer is detached.";
const TOO_LARGE: &str = "The buffer is too large.";
const CONNECT: &str = "The connection could not be opened.";
const ACCEPT: &str = "The connection could not be accepted.";
const READ_ERR: &str = "The bytes could not be read.";
const WRITE_ERR: &str = "The bytes could not be written.";
const BUSY_ACCEPT: &str = "An accept is already waiting.";
const BUSY_READ: &str = "A read is already waiting.";
const BUSY_WRITE: &str = "A write is already waiting.";
const NO_PROMISE: &str = "The socket operation failed.";
const ARITY: &str = "Not enough arguments";

thread_local! {
  static OPEN: RefCell<Vec<Live>> = RefCell::new(Vec::new());
}

enum Live {
  Listener(Weak<ListenerShared>),
  Conn(Weak<ConnShared>),
}

struct ListenerShared {
  listener: Mutex<Option<StdListener>>,
  bound: SocketAddr,
  closed: AtomicBool,
  accepting: AtomicBool,
}

struct ConnShared {
  stream: Mutex<Option<TcpStream>>,
  local: SocketAddr,
  remote: SocketAddr,
  closed: AtomicBool,
  reading: AtomicBool,
  writing: AtomicBool,
}

impl ListenerShared {
  fn is_closed(&self) -> bool {
    self.closed.load(Ordering::Acquire)
  }

  /// Returns whether the listener was already closed.
  fn mark_closed(&self) -> bool {
    let already = self.closed.swap(true, Ordering::AcqRel);
    if !already {
      let mut slot = lock(&self.listener);
      slot.take();
    }
    already
  }
}

impl ConnShared {
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

struct ConnParts {
  stream: TcpStream,
  local: SocketAddr,
  remote: SocketAddr,
}

#[derive(Clone, Copy, Debug)]
enum Fail {
  Closed,
  Connect,
  Accept,
  Read,
  Write,
  Detached,
}

impl Fail {
  fn message(self) -> &'static str {
    match self {
      Self::Closed => CLOSED,
      Self::Connect => CONNECT,
      Self::Accept => ACCEPT,
      Self::Read => READ_ERR,
      Self::Write => WRITE_ERR,
      Self::Detached => DETACHED,
    }
  }
}

enum Deliver {
  Count(usize),
  Eof,
  Bytes(Vec<u8>),
  Conn(ConnParts),
}

enum Destination {
  View(v8::Global<v8::ArrayBufferView>),
  Buffer(v8::Global<v8::ArrayBuffer>),
  Shared(v8::Global<v8::SharedArrayBuffer>),
}

enum Finish {
  Count,
  Read(Destination),
  Conn,
}

fn track(live: Live) {
  OPEN.with(|open| {
    let mut open = open.borrow_mut();
    open.retain(|item| match item {
      Live::Listener(weak) => weak.strong_count() > 0,
      Live::Conn(weak) => weak.strong_count() > 0,
    });
    open.push(live);
  });
}

fn track_listener(shared: &Arc<ListenerShared>) {
  track(Live::Listener(Arc::downgrade(shared)));
}

fn track_conn(shared: &Arc<ConnShared>) {
  track(Live::Conn(Arc::downgrade(shared)));
}

pub(crate) fn clear() {
  let open = OPEN.with(|open| std::mem::take(&mut *open.borrow_mut()));
  for live in open {
    match live {
      Live::Listener(weak) => {
        if let Some(shared) = weak.upgrade() {
          let _ = shared.mark_closed();
        }
      }
      Live::Conn(weak) => {
        if let Some(shared) = weak.upgrade() {
          let _ = shared.mark_closed();
        }
      }
    }
  }
}

fn bind_tcp(addr: SocketAddr) -> io::Result<Arc<ListenerShared>> {
  let listener = StdListener::bind(addr)?;
  listener.set_nonblocking(true)?;
  let bound = listener.local_addr()?;
  let shared = Arc::new(ListenerShared {
    listener: Mutex::new(Some(listener)),
    bound,
    closed: AtomicBool::new(false),
    accepting: AtomicBool::new(false),
  });
  track_listener(&shared);
  Ok(shared)
}

fn configure(stream: &TcpStream) -> io::Result<()> {
  stream.set_nonblocking(false)?;
  stream.set_read_timeout(Some(IO_WAIT))?;
  stream.set_write_timeout(Some(IO_WAIT))?;
  let _ = stream.set_nodelay(true);
  Ok(())
}

fn accept_conn(shared: &ListenerShared) -> Result<ConnParts, Fail> {
  if shared.is_closed() {
    return Err(Fail::Closed);
  }
  let listener = {
    let slot = lock(&shared.listener);
    let Some(listener) = slot.as_ref() else {
      return Err(Fail::Closed);
    };
    listener.try_clone().map_err(|_| Fail::Accept)?
  };
  loop {
    if shared.is_closed() {
      return Err(Fail::Closed);
    }
    match listener.accept() {
      Ok((stream, peer)) => {
        if shared.is_closed() {
          return Err(Fail::Closed);
        }
        configure(&stream).map_err(|_| Fail::Accept)?;
        let local = stream.local_addr().map_err(|_| Fail::Accept)?;
        return Ok(ConnParts {
          stream,
          local,
          remote: peer,
        });
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
  }
}

fn connect_conn(dest: SocketAddr) -> Result<ConnParts, Fail> {
  let stream = TcpStream::connect(dest).map_err(|_| Fail::Connect)?;
  configure(&stream).map_err(|_| Fail::Connect)?;
  let local = stream.local_addr().map_err(|_| Fail::Connect)?;
  let remote = stream.peer_addr().map_err(|_| Fail::Connect)?;
  Ok(ConnParts {
    stream,
    local,
    remote,
  })
}

fn read_conn(shared: &ConnShared, max: usize) -> Result<Deliver, Fail> {
  if shared.is_closed() {
    return Err(Fail::Closed);
  }
  let mut stream = {
    let slot = lock(&shared.stream);
    let Some(stream) = slot.as_ref() else {
      return Err(Fail::Closed);
    };
    stream.try_clone().map_err(|_| Fail::Read)?
  };
  let mut buf = vec![0u8; max];
  loop {
    if shared.is_closed() {
      return Err(Fail::Closed);
    }
    match stream.read(&mut buf) {
      Ok(0) if shared.is_closed() => return Err(Fail::Closed),
      Ok(0) => return Ok(Deliver::Eof),
      Ok(count) if shared.is_closed() => {
        let _ = count;
        return Err(Fail::Closed);
      }
      Ok(count) => {
        buf.truncate(count);
        return Ok(Deliver::Bytes(buf));
      }
      Err(error) if is_io_retry(error.kind()) => {
        if shared.is_closed() {
          return Err(Fail::Closed);
        }
        if error.kind() == ErrorKind::WouldBlock {
          std::thread::sleep(IO_WAIT);
        }
      }
      Err(_) if shared.is_closed() => return Err(Fail::Closed),
      Err(_) => return Err(Fail::Read),
    }
  }
}

fn write_conn(shared: &ConnShared, bytes: &[u8]) -> Result<usize, Fail> {
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
  let mut offset = 0;
  while offset < bytes.len() {
    if shared.is_closed() {
      return Err(Fail::Closed);
    }
    match stream.write(&bytes[offset..]) {
      Ok(0) if shared.is_closed() => return Err(Fail::Closed),
      Ok(0) => return Err(Fail::Write),
      Ok(count) if shared.is_closed() => {
        let _ = count;
        return Err(Fail::Closed);
      }
      Ok(count) => offset += count,
      Err(error) if is_io_retry(error.kind()) => {
        if shared.is_closed() {
          return Err(Fail::Closed);
        }
        if error.kind() == ErrorKind::WouldBlock {
          std::thread::sleep(IO_WAIT);
        }
      }
      Err(_) if shared.is_closed() => return Err(Fail::Closed),
      Err(_) => return Err(Fail::Write),
    }
  }
  Ok(offset)
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

pub struct Listener {
  shared: Arc<ListenerShared>,
}

unsafe impl v8::cppgc::GarbageCollected for Listener {
  fn trace(&self, _visitor: &mut v8::cppgc::Visitor) {}

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"TcpListener"
  }
}

impl Drop for Listener {
  fn drop(&mut self) {
    let _ = self.shared.mark_closed();
  }
}

pub struct Conn {
  shared: Arc<ConnShared>,
}

unsafe impl v8::cppgc::GarbageCollected for Conn {
  fn trace(&self, _visitor: &mut v8::cppgc::Visitor) {}

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"TcpConn"
  }
}

impl Drop for Conn {
  fn drop(&mut self) {
    let _ = self.shared.mark_closed();
  }
}

#[op(name = "TcpListener")]
impl Listener {
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
      Fail::Accept,
      move || {
        let _guard = FlagGuard(&shared.accepting);
        accept_conn(&shared).map(Deliver::Conn)
      },
      Finish::Conn,
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

#[op(name = "TcpConn")]
impl Conn {
  #[getter]
  fn local_addr(&self, scope: &mut v8::PinScope) -> Result<Js, OpError> {
    address_value(scope, self.shared.local)
  }

  #[getter]
  fn remote_addr(&self, scope: &mut v8::PinScope) -> Result<Js, OpError> {
    address_value(scope, self.shared.remote)
  }

  fn read(
    &self,
    scope: &mut v8::PinScope,
    buffer: v8::Local<v8::Value>,
  ) -> Result<Js, OpError> {
    let (dest, length) = read_destination(scope, buffer)?;
    if length == 0 {
      return resolved_count(scope, 0);
    }
    if self.shared.is_closed() {
      return rejected(scope, CLOSED);
    }
    if self
      .shared
      .reading
      .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
      .is_err()
    {
      return rejected(scope, BUSY_READ);
    }
    let shared = Arc::clone(&self.shared);
    let settled = settle(
      scope,
      Fail::Read,
      move || {
        let _guard = FlagGuard(&shared.reading);
        read_conn(&shared, length)
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
    let bytes = copy_source(scope, data)?;
    if bytes.is_empty() {
      return resolved_count(scope, 0);
    }
    if self.shared.is_closed() {
      return rejected(scope, CLOSED);
    }
    if self
      .shared
      .writing
      .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
      .is_err()
    {
      return rejected(scope, BUSY_WRITE);
    }
    let shared = Arc::clone(&self.shared);
    let settled = settle(
      scope,
      Fail::Write,
      move || {
        let _guard = FlagGuard(&shared.writing);
        write_conn(&shared, &bytes).map(Deliver::Count)
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
  Listener::install(scope);
  Conn::install(scope);
  move_constructor(scope, "TcpListener");
  move_constructor(scope, "TcpConn");
  let yuzuko = crate::namespace(scope, "Yuzuko");
  define_fn(scope, yuzuko, "listenTcp", 0, listen_tcp);
  define_fn(scope, yuzuko, "connectTcp", 1, connect_tcp);
}

fn listen_tcp(
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

fn connect_tcp(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut ret: v8::ReturnValue,
) {
  match connect_op(scope, &args) {
    Ok(value) => value.to_v8(scope, &mut ret),
    Err(error) => OpException::throw(error, scope),
  }
}

fn listen_op(
  scope: &mut v8::PinScope,
  options: v8::Local<v8::Value>,
) -> Result<New<Listener>, OpError> {
  let addr = if options.is_null_or_undefined() {
    SocketAddr::from((Ipv4Addr::LOCALHOST, 0))
  } else {
    endpoint(scope, options, 0, PORT_BIND, OPTIONS, OPTIONS_READ)?
  };
  let shared = bind_tcp(addr).map_err(|_| plain_error(scope, BIND))?;
  Ok(New(Listener { shared }))
}

fn connect_op(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
) -> Result<Js, OpError> {
  if args.length() < 1 {
    return Err(OpError::type_(ARITY));
  }
  let dest = endpoint(scope, args.get(0), 1, PORT_DEST, ADDRESS, ADDRESS_READ)?;
  settle(
    scope,
    Fail::Connect,
    move || connect_conn(dest).map(Deliver::Conn),
    Finish::Conn,
  )
}

fn endpoint(
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
  let ip = parse_host(&hostname)?;
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

fn address_value(
  scope: &mut v8::PinScope,
  addr: SocketAddr,
) -> Result<Js, OpError> {
  let object = v8::Object::new(scope);
  address_fields(scope, object, &addr.ip().to_string(), addr.port())
    .map_err(|_| OpError::type_(ADDRESS_READ))?;
  let value: v8::Local<v8::Value> = object.into();
  Ok(Js(v8::Global::new(scope, value)))
}

fn address_fields(
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
  object.set(scope, key.into(), value.into());
  let Some(key) = v8::String::new(scope, "port") else {
    return Err(());
  };
  let port = v8::Integer::new(scope, i32::from(port));
  object.set(scope, key.into(), port.into());
  Ok(())
}

fn read_destination(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<(Destination, usize), OpError> {
  if let Ok(buffer) = v8::Local::<v8::SharedArrayBuffer>::try_from(value) {
    let length = buffer.byte_length();
    if length > MAX_BUFFER {
      return Err(OpError::type_(TOO_LARGE));
    }
    return Ok((Destination::Shared(v8::Global::new(scope, buffer)), length));
  }
  if let Ok(view) = v8::Local::<v8::ArrayBufferView>::try_from(value) {
    if view
      .buffer(scope)
      .is_some_and(|buffer| buffer.was_detached())
    {
      return Err(OpError::type_(DETACHED));
    }
    let length = view.byte_length();
    if length > MAX_BUFFER {
      return Err(OpError::type_(TOO_LARGE));
    }
    return Ok((Destination::View(v8::Global::new(scope, view)), length));
  }
  if let Ok(buffer) = v8::Local::<v8::ArrayBuffer>::try_from(value) {
    if buffer.was_detached() {
      return Err(OpError::type_(DETACHED));
    }
    let length = buffer.byte_length();
    if length > MAX_BUFFER {
      return Err(OpError::type_(TOO_LARGE));
    }
    return Ok((Destination::Buffer(v8::Global::new(scope, buffer)), length));
  }
  Err(OpError::type_(DATA))
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
    if length > MAX_BUFFER {
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
  if length > MAX_BUFFER {
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
  if length > MAX_BUFFER {
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

fn write_dest(
  scope: &mut v8::PinScope,
  dest: Destination,
  bytes: &[u8],
) -> Result<usize, Fail> {
  match dest {
    Destination::View(global) => {
      let view = v8::Local::new(scope, &global);
      if view
        .buffer(scope)
        .is_some_and(|buffer| buffer.was_detached())
        || view.byte_length() < bytes.len()
      {
        return Err(Fail::Detached);
      }
      if bytes.is_empty() {
        return Ok(0);
      }
      let data = view.data();
      if data.is_null() {
        return Err(Fail::Detached);
      }
      // Safety: `bytes` fits the view. `data()` includes the byte offset,
      // and `view` keeps the store alive for this copy.
      unsafe {
        std::ptr::copy_nonoverlapping(
          bytes.as_ptr(),
          data.cast::<u8>(),
          bytes.len(),
        );
      }
      Ok(bytes.len())
    }
    Destination::Buffer(global) => {
      let buffer = v8::Local::new(scope, &global);
      write_buffer(
        buffer.was_detached(),
        buffer.byte_length(),
        buffer.data().map(std::ptr::NonNull::as_ptr),
        bytes,
      )
    }
    Destination::Shared(global) => {
      let buffer = v8::Local::new(scope, &global);
      let store = buffer.get_backing_store();
      write_buffer(
        false,
        buffer.byte_length(),
        store.data().map(std::ptr::NonNull::as_ptr),
        bytes,
      )
    }
  }
}

fn write_buffer(
  detached: bool,
  length: usize,
  data: Option<*mut std::ffi::c_void>,
  bytes: &[u8],
) -> Result<usize, Fail> {
  if detached || length < bytes.len() {
    return Err(Fail::Detached);
  }
  if bytes.is_empty() {
    return Ok(0);
  }
  let Some(data) = data else {
    return Err(Fail::Detached);
  };
  // Safety: `bytes` fits `length`, and the caller keeps the store alive.
  unsafe {
    std::ptr::copy_nonoverlapping(
      bytes.as_ptr(),
      data.cast::<u8>(),
      bytes.len(),
    );
  }
  Ok(bytes.len())
}

fn apply<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  finish: Finish,
  value: Deliver,
) -> Result<v8::Local<'s, v8::Value>, Fail> {
  match (finish, value) {
    (Finish::Count, Deliver::Count(count)) => {
      Ok(v8::Number::new(scope, count as f64).into())
    }
    (Finish::Read(_), Deliver::Eof) => Ok(v8::null(scope).into()),
    (Finish::Read(dest), Deliver::Bytes(bytes)) => {
      let count = write_dest(scope, dest, &bytes)?;
      Ok(v8::Number::new(scope, count as f64).into())
    }
    (Finish::Conn, Deliver::Conn(parts)) => conn_value(scope, parts),
    _ => Err(Fail::Read),
  }
}

fn conn_value<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  parts: ConnParts,
) -> Result<v8::Local<'s, v8::Value>, Fail> {
  let shared = Arc::new(ConnShared {
    stream: Mutex::new(Some(parts.stream)),
    local: parts.local,
    remote: parts.remote,
    closed: AtomicBool::new(false),
    reading: AtomicBool::new(false),
    writing: AtomicBool::new(false),
  });
  track_conn(&shared);
  // Safety: `Conn::install` cached this realm's `TcpConn` template.
  let object = unsafe { wrap_instance(scope, Conn { shared }) };
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
  use std::time::Duration;

  use super::{Fail, accept_conn, bind_tcp, read_conn, write_conn};

  fn read_exact(conn: &super::ConnShared, length: usize) -> Vec<u8> {
    let mut got = Vec::new();
    while got.len() < length {
      match read_conn(conn, length - got.len()).unwrap() {
        super::Deliver::Bytes(chunk) => got.extend(chunk),
        super::Deliver::Eof => panic!("eof at {}", got.len()),
        _ => panic!("unexpected read"),
      }
    }
    got
  }

  #[test]
  fn local_sockets_exchange_bytes() {
    let listener = bind_tcp("127.0.0.1:0".parse().unwrap()).unwrap();
    let port = listener.bound.port();
    assert_ne!(port, 0);
    let payload: Vec<u8> = (0..1000).map(|index| (index % 256) as u8).collect();
    let sent = payload.clone();
    let client = std::thread::spawn(move || {
      let mut stream =
        std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
      stream.write_all(&sent).unwrap();
      let mut reply = [0u8; 4];
      stream.read_exact(&mut reply).unwrap();
      assert_eq!(&reply, b"pong");
    });
    let conn = accept_conn(&listener).unwrap();
    let shared = std::sync::Arc::new(super::ConnShared {
      stream: std::sync::Mutex::new(Some(conn.stream)),
      local: conn.local,
      remote: conn.remote,
      closed: std::sync::atomic::AtomicBool::new(false),
      reading: std::sync::atomic::AtomicBool::new(false),
      writing: std::sync::atomic::AtomicBool::new(false),
    });
    super::track_conn(&shared);
    let got = read_exact(&shared, payload.len());
    assert_eq!(got, payload);
    assert_eq!(shared.remote.ip().to_string(), "127.0.0.1");
    assert_eq!(write_conn(&shared, b"pong").unwrap(), 4);
    client.join().unwrap();
  }

  #[test]
  fn close_unblocks_accept_and_read() {
    let listener = bind_tcp("127.0.0.1:0".parse().unwrap()).unwrap();
    let waiting = std::sync::Arc::clone(&listener);
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
      let _ = tx.send(accept_conn(&waiting));
    });
    std::thread::sleep(Duration::from_millis(30));
    assert!(!listener.mark_closed());
    let result = rx
      .recv_timeout(Duration::from_secs(2))
      .expect("accept did not finish");
    assert!(matches!(result, Err(Fail::Closed)));
    assert!(listener.mark_closed());

    let listener = bind_tcp("127.0.0.1:0".parse().unwrap()).unwrap();
    let port = listener.bound.port();
    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
      let stream = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
      ready_tx.send(stream).unwrap();
    });
    let conn = accept_conn(&listener).unwrap();
    let _peer = ready_rx
      .recv_timeout(Duration::from_secs(2))
      .expect("client did not connect");
    let shared = std::sync::Arc::new(super::ConnShared {
      stream: std::sync::Mutex::new(Some(conn.stream)),
      local: conn.local,
      remote: conn.remote,
      closed: std::sync::atomic::AtomicBool::new(false),
      reading: std::sync::atomic::AtomicBool::new(false),
      writing: std::sync::atomic::AtomicBool::new(false),
    });
    let reading = std::sync::Arc::clone(&shared);
    let (read_tx, read_rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
      let _ = read_tx.send(read_conn(&reading, 8));
    });
    std::thread::sleep(Duration::from_millis(30));
    assert!(!shared.mark_closed());
    let read = read_rx
      .recv_timeout(Duration::from_secs(2))
      .expect("read did not finish");
    assert!(matches!(read, Err(Fail::Closed)));
    assert!(matches!(write_conn(&shared, &[1]), Err(Fail::Closed)));
    assert!(shared.mark_closed());
  }

  #[test]
  fn clear_closes_only_this_thread() {
    let listener = bind_tcp("127.0.0.1:0".parse().unwrap()).unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
      super::clear();
      tx.send(()).unwrap();
    });
    rx.recv_timeout(Duration::from_secs(2))
      .expect("other thread did not finish");
    assert!(!listener.is_closed());
    let port = listener.bound.port();
    let client = std::thread::spawn(move || {
      let mut stream =
        std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
      stream.write_all(&[7]).unwrap();
    });
    let conn = accept_conn(&listener).unwrap();
    let shared = std::sync::Arc::new(super::ConnShared {
      stream: std::sync::Mutex::new(Some(conn.stream)),
      local: conn.local,
      remote: conn.remote,
      closed: std::sync::atomic::AtomicBool::new(false),
      reading: std::sync::atomic::AtomicBool::new(false),
      writing: std::sync::atomic::AtomicBool::new(false),
    });
    super::track_conn(&shared);
    let got = read_exact(&shared, 1);
    assert_eq!(got, vec![7]);
    client.join().unwrap();
    super::clear();
    assert!(listener.is_closed());
    assert!(shared.is_closed());
  }

  #[test]
  fn bind_rejects_an_unassigned_address() {
    let error = match bind_tcp("192.0.2.1:0".parse().unwrap()) {
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
