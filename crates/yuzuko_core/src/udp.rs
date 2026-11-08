//! Optional `Yuzuko` UDP datagrams.
//!
//! The `udp` feature installs `listenDatagram` and `Datagram` on the main
//! realm and on a dedicated worker. `send` and `receive` run on a background
//! thread and settle on that realm's event loop. A short read timeout lets
//! `close` finish a waiting receive when `shutdown` does not wake it.

use std::cell::RefCell;
use std::io::{self, ErrorKind};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Weak};
use std::time::Duration;

use yuzuko_macros::op;

use crate::op::{FromV8, Js, New, OpError, OpException, ToV8};

const MAX_DATAGRAM: usize = 65507;
const RECV_CAP: usize = 65536;
const RECV_WAIT: Duration = Duration::from_millis(50);

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
const TOO_LARGE: &str = "The datagram is too large.";
const FAMILY: &str = "The address family does not match the socket.";
const SEND: &str = "The datagram could not be sent.";
const RECEIVE: &str = "The datagram could not be received.";
const BUSY: &str = "A receive is already waiting.";
const NO_PROMISE: &str = "The socket operation failed.";

thread_local! {
  static OPEN: RefCell<Vec<Weak<Shared>>> = const { RefCell::new(Vec::new()) };
}

struct Shared {
  socket: UdpSocket,
  bound: SocketAddr,
  closed: AtomicBool,
  receiving: AtomicBool,
}

impl Shared {
  fn is_closed(&self) -> bool {
    self.closed.load(Ordering::Acquire)
  }

  /// Returns whether the socket was already closed.
  fn mark_closed(&self) -> bool {
    let already = self.closed.swap(true, Ordering::AcqRel);
    if !already {
      wake_recv(&self.socket);
    }
    already
  }
}

/// Wakes a blocked `recv`. An unconnected UDP socket may refuse the call.
/// The read timeout still finishes the receive.
fn wake_recv(socket: &UdpSocket) {
  #[cfg(unix)]
  {
    use std::os::fd::AsRawFd;
    // Safety: the descriptor belongs to `socket` and stays open. This
    // only asks the kernel to wake a blocked receive.
    let _ = unsafe { libc::shutdown(socket.as_raw_fd(), libc::SHUT_RDWR) };
  }
  #[cfg(windows)]
  {
    use std::os::windows::io::AsRawSocket;
    use windows_sys::Win32::Networking::WinSock::{SD_BOTH, shutdown};
    // Safety: the handle belongs to `socket` and stays open. This only
    // asks Winsock to wake a blocked receive.
    let _ = unsafe { shutdown(socket.as_raw_socket() as usize, SD_BOTH) };
  }
  #[cfg(not(any(unix, windows)))]
  {
    let _ = socket;
  }
}

struct ReceiveGuard<'a>(&'a Shared);

impl Drop for ReceiveGuard<'_> {
  fn drop(&mut self) {
    self.0.receiving.store(false, Ordering::Release);
  }
}

struct Packet {
  bytes: Vec<u8>,
  hostname: String,
  port: u16,
}

#[derive(Clone, Copy, Debug)]
enum Fail {
  Closed,
  Send,
  Receive,
}

impl Fail {
  fn message(self) -> &'static str {
    match self {
      Self::Closed => CLOSED,
      Self::Send => SEND,
      Self::Receive => RECEIVE,
    }
  }
}

fn track(shared: &Arc<Shared>) {
  OPEN.with(|open| {
    let mut open = open.borrow_mut();
    open.retain(|weak| weak.strong_count() > 0);
    open.push(Arc::downgrade(shared));
  });
}

pub(crate) fn clear() {
  let open = OPEN.with(|open| std::mem::take(&mut *open.borrow_mut()));
  for weak in open {
    if let Some(shared) = weak.upgrade() {
      let _ = shared.mark_closed();
    }
  }
}

fn bind_udp(addr: SocketAddr) -> io::Result<Arc<Shared>> {
  let socket = UdpSocket::bind(addr)?;
  socket.set_read_timeout(Some(RECV_WAIT))?;
  let bound = socket.local_addr()?;
  let shared = Arc::new(Shared {
    socket,
    bound,
    closed: AtomicBool::new(false),
    receiving: AtomicBool::new(false),
  });
  track(&shared);
  Ok(shared)
}

fn send_datagram(
  shared: &Shared,
  bytes: &[u8],
  dest: SocketAddr,
) -> Result<usize, Fail> {
  if shared.is_closed() {
    return Err(Fail::Closed);
  }
  match shared.socket.send_to(bytes, dest) {
    Ok(count) => Ok(count),
    Err(_) if shared.is_closed() => Err(Fail::Closed),
    Err(_) => Err(Fail::Send),
  }
}

fn recv_datagram(shared: &Shared) -> Result<Packet, Fail> {
  if shared.is_closed() {
    return Err(Fail::Closed);
  }
  let mut buf = vec![0u8; RECV_CAP];
  loop {
    if shared.is_closed() {
      return Err(Fail::Closed);
    }
    match shared.socket.recv_from(&mut buf) {
      Ok((count, addr)) => {
        let count = count.min(buf.len());
        return Ok(Packet {
          bytes: buf[..count].to_vec(),
          hostname: addr.ip().to_string(),
          port: addr.port(),
        });
      }
      Err(error) if is_retry(error.kind()) => continue,
      // Windows reports an ICMP unreachable as a connection reset. UDP has
      // no connection, so the receive keeps waiting.
      Err(error) if error.kind() == ErrorKind::ConnectionReset => {
        if shared.is_closed() {
          return Err(Fail::Closed);
        }
        std::thread::sleep(RECV_WAIT);
        continue;
      }
      Err(_) if shared.is_closed() => return Err(Fail::Closed),
      Err(_) => return Err(Fail::Receive),
    }
  }
}

fn is_retry(kind: ErrorKind) -> bool {
  matches!(
    kind,
    ErrorKind::WouldBlock | ErrorKind::TimedOut | ErrorKind::Interrupted
  )
}

pub struct Datagram {
  shared: Arc<Shared>,
}

unsafe impl v8::cppgc::GarbageCollected for Datagram {
  fn trace(&self, _visitor: &mut v8::cppgc::Visitor) {}

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"Datagram"
  }
}

impl Drop for Datagram {
  fn drop(&mut self) {
    let _ = self.shared.mark_closed();
  }
}

#[op]
impl Datagram {
  #[getter]
  fn addr(&self, scope: &mut v8::PinScope) -> Result<Js, OpError> {
    let object = v8::Object::new(scope);
    let bound = self.shared.bound;
    address_fields(scope, object, &bound.ip().to_string(), bound.port())
      .map_err(|_| OpError::type_(ADDRESS_READ))?;
    let value: v8::Local<v8::Value> = object.into();
    Ok(Js(v8::Global::new(scope, value)))
  }

  fn send(
    &self,
    scope: &mut v8::PinScope,
    data: v8::Local<v8::Value>,
    addr: v8::Local<v8::Value>,
  ) -> Result<Js, OpError> {
    let bytes = copy_source(scope, data)?;
    let dest = endpoint(scope, addr, 1, PORT_DEST, ADDRESS, ADDRESS_READ)?;
    if self.shared.bound.is_ipv4() != dest.is_ipv4() {
      return Err(OpError::type_(FAMILY));
    }
    if self.shared.is_closed() {
      return rejected(scope, CLOSED);
    }
    let shared = Arc::clone(&self.shared);
    settle(scope, Fail::Send, move || {
      send_datagram(&shared, &bytes, dest).map(Deliver::Count)
    })
  }

  fn receive(&self, scope: &mut v8::PinScope) -> Result<Js, OpError> {
    if self.shared.is_closed() {
      return rejected(scope, CLOSED);
    }
    if self
      .shared
      .receiving
      .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
      .is_err()
    {
      return rejected(scope, BUSY);
    }
    let shared = Arc::clone(&self.shared);
    let settled = settle(scope, Fail::Receive, move || {
      let _guard = ReceiveGuard(&shared);
      recv_datagram(&shared).map(Deliver::Packet)
    });
    if settled.is_err() {
      self.shared.receiving.store(false, Ordering::Release);
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
  Datagram::install(scope);
  move_constructor(scope, "Datagram");
  let yuzuko = crate::namespace(scope, "Yuzuko");
  define_fn(scope, yuzuko, "listenDatagram", 0, listen_datagram);
}

fn listen_datagram(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut ret: v8::ReturnValue,
) {
  match listen_op(scope, args.get(0)) {
    Ok(value) => value.to_v8(scope, &mut ret),
    Err(error) => OpException::throw(error, scope),
  }
}

fn listen_op(
  scope: &mut v8::PinScope,
  options: v8::Local<v8::Value>,
) -> Result<New<Datagram>, OpError> {
  let addr = if options.is_null_or_undefined() {
    crate::permissions::check_net("127.0.0.1", 0)?;
    SocketAddr::from((Ipv4Addr::LOCALHOST, 0))
  } else {
    endpoint(scope, options, 0, PORT_BIND, OPTIONS, OPTIONS_READ)?
  };
  let shared = bind_udp(addr).map_err(|_| plain_error(scope, BIND))?;
  Ok(New(Datagram { shared }))
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
  crate::permissions::check_net(&hostname, port)?;
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
    if length > MAX_DATAGRAM {
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
  if length > MAX_DATAGRAM {
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
  if length > MAX_DATAGRAM {
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

fn packet_value<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  packet: Packet,
) -> Result<v8::Local<'s, v8::Value>, Fail> {
  let object = v8::Object::new(scope);
  let Some(data) = uint8_array(scope, packet.bytes) else {
    return Err(Fail::Receive);
  };
  let Some(key) = v8::String::new(scope, "data") else {
    return Err(Fail::Receive);
  };
  object.set(scope, key.into(), data);
  if address_fields(scope, object, &packet.hostname, packet.port).is_err() {
    return Err(Fail::Receive);
  }
  Ok(object.into())
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

fn count_value<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  count: usize,
) -> Result<v8::Local<'s, v8::Value>, Fail> {
  Ok(v8::Number::new(scope, count as f64).into())
}

enum Deliver {
  Count(usize),
  Packet(Packet),
}

fn settle(
  scope: &mut v8::PinScope,
  panicked: Fail,
  work: impl FnOnce() -> Result<Deliver, Fail> + Send + 'static,
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
      Ok(Deliver::Count(count)) => count_value(scope, count),
      Ok(Deliver::Packet(packet)) => packet_value(scope, packet),
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
  use std::time::Duration;

  use super::{Fail, bind_udp, recv_datagram, send_datagram};

  #[test]
  fn local_sockets_exchange_one_datagram() {
    let left = bind_udp("127.0.0.1:0".parse().unwrap()).unwrap();
    let right = bind_udp("127.0.0.1:0".parse().unwrap()).unwrap();
    let payload: Vec<u8> = (0..1000).map(|index| (index % 256) as u8).collect();
    let sent = send_datagram(&right, &payload, left.bound).unwrap();
    assert_eq!(sent, payload.len());
    let packet = recv_datagram(&left).unwrap();
    assert_eq!(packet.bytes, payload);
    assert_eq!(packet.hostname, "127.0.0.1");
    assert_eq!(packet.port, right.bound.port());
    assert_ne!(left.bound.port(), 0);
  }

  #[test]
  fn close_unblocks_receive() {
    let socket = bind_udp("127.0.0.1:0".parse().unwrap()).unwrap();
    let waiting = std::sync::Arc::clone(&socket);
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
      let _ = tx.send(recv_datagram(&waiting));
    });
    std::thread::sleep(Duration::from_millis(30));
    assert!(!socket.mark_closed());
    let result = rx
      .recv_timeout(Duration::from_secs(2))
      .expect("receive did not finish");
    assert!(matches!(result, Err(Fail::Closed)));
    assert!(matches!(
      send_datagram(&socket, &[1], socket.bound),
      Err(Fail::Closed)
    ));
    assert!(socket.mark_closed());
  }

  #[test]
  fn clear_closes_only_this_thread() {
    let socket = bind_udp("127.0.0.1:0".parse().unwrap()).unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
      super::clear();
      tx.send(()).unwrap();
    });
    rx.recv_timeout(Duration::from_secs(2))
      .expect("other thread did not finish");
    assert!(!socket.is_closed());
    let other = bind_udp("127.0.0.1:0".parse().unwrap()).unwrap();
    let sent = send_datagram(&other, &[7], socket.bound).unwrap();
    assert_eq!(sent, 1);
    let packet = recv_datagram(&socket).unwrap();
    assert_eq!(packet.bytes, vec![7]);
    super::clear();
    assert!(socket.is_closed());
    assert!(other.is_closed());
  }

  #[test]
  fn bind_rejects_an_unassigned_address() {
    let error = match bind_udp("192.0.2.1:0".parse().unwrap()) {
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
