//! Optional `Yuzuko` UDP datagrams.
//!
//! The `udp` feature installs `listenDatagram` and `Datagram` on the main
//! realm and on a dedicated worker. `send` and `receive` run on a background
//! thread and settle on that realm's event loop. A short read timeout lets
//! `close` finish a waiting receive when `shutdown` does not wake it.

use std::cell::RefCell;
use std::io::{self, ErrorKind};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Weak};
use std::time::Duration;

use yuzuko_macros::op;

use crate::op::{Js, New, OpError, OpException, ToV8};

const MAX_DATAGRAM: usize = 65507;
const RECV_CAP: usize = 65536;
const RECV_WAIT: Duration = Duration::from_millis(50);

const OPTIONS: &str = "Options must be an object.";
const OPTIONS_READ: &str = "The options could not be read.";
const ADDRESS: &str = "The address must be an object.";
const ADDRESS_READ: &str = "The address could not be read.";
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
const ENABLED: &str = "enabled must be a boolean.";
const MULTICAST: &str = "The address must be a multicast address.";
const IFACE_INDEX: &str = "The network interface must be an index.";
const IFACE_ADDR: &str = "The network interface must be an address.";
const BROADCAST_FAIL: &str = "The broadcast option could not be set.";
const JOIN_FAIL: &str = "The multicast group could not be joined.";
const LEAVE_FAIL: &str = "The multicast group could not be left.";
const CONNECT_FAIL: &str = "The socket could not be connected.";
const DISCONNECT_FAIL: &str = "The socket could not be disconnected.";
const NOT_CONNECTED: &str = "The socket is not connected.";

thread_local! {
  static OPEN: RefCell<Vec<Weak<Shared>>> = const { RefCell::new(Vec::new()) };
}

struct Shared {
  socket: UdpSocket,
  bound: SocketAddr,
  closed: AtomicBool,
  receiving: AtomicBool,
  connected: AtomicBool,
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

#[derive(Clone, Debug)]
enum Fail {
  Closed,
  Send,
  Receive,
  Class {
    class: crate::socket_error::Class,
    code: Option<&'static str>,
  },
  /// An operating-system failure with no socket class. `InvalidInput` is a
  /// `TypeError`. `code` is set when Deno would set it.
  Io {
    message: &'static str,
    code: Option<&'static str>,
    type_error: bool,
  },
  Denied {
    name: String,
    message: String,
  },
  Type(&'static str),
  Resolve,
  Connect,
}

impl Fail {
  fn message(&self) -> &'static str {
    match self {
      Self::Closed => CLOSED,
      Self::Send => SEND,
      Self::Receive => RECEIVE,
      Self::Class { class, .. } => class.message(),
      Self::Io { message, .. } => message,
      Self::Denied { .. } => SEND,
      Self::Type(message) => message,
      Self::Resolve => crate::dns::UNRESOLVED,
      Self::Connect => CONNECT_FAIL,
    }
  }
}

fn io_fail(error: io::Error, generic: Fail) -> Fail {
  match crate::socket_error::map_io(&error) {
    crate::socket_error::Mapped::Class { class, code } => {
      Fail::Class { class, code }
    }
    crate::socket_error::Mapped::Generic { code, type_error } => Fail::Io {
      message: generic.message(),
      code,
      type_error,
    },
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
  finish_udp(UdpSocket::bind(addr)?)
}

fn bind_udp_reuse(addr: SocketAddr) -> io::Result<Arc<Shared>> {
  finish_udp(crate::socket_bind::udp_socket(addr)?)
}

fn finish_udp(socket: UdpSocket) -> io::Result<Arc<Shared>> {
  socket.set_read_timeout(Some(RECV_WAIT))?;
  let bound = socket.local_addr()?;
  let shared = Arc::new(Shared {
    socket,
    bound,
    closed: AtomicBool::new(false),
    receiving: AtomicBool::new(false),
    connected: AtomicBool::new(false),
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
    Err(error) => Err(io_fail(error, Fail::Send)),
  }
}

fn send_connected(shared: &Shared, bytes: &[u8]) -> Result<usize, Fail> {
  if shared.is_closed() {
    return Err(Fail::Closed);
  }
  match shared.socket.send(bytes) {
    Ok(count) => Ok(count),
    Err(_) if shared.is_closed() => Err(Fail::Closed),
    Err(error) => Err(io_fail(error, Fail::Send)),
  }
}

fn connect_datagram(shared: &Shared, dest: SocketAddr) -> Result<(), Fail> {
  if shared.is_closed() {
    return Err(Fail::Closed);
  }
  match shared.socket.connect(dest) {
    Ok(()) => {
      shared.connected.store(true, Ordering::Release);
      Ok(())
    }
    Err(_) if shared.is_closed() => Err(Fail::Closed),
    Err(error) => Err(io_fail(error, Fail::Connect)),
  }
}

fn send_addr(
  target: &crate::socket_endpoint::Target,
  ipv4: bool,
) -> Result<SocketAddr, Fail> {
  match target {
    crate::socket_endpoint::Target::Addr { addr, .. } => Ok(*addr),
    crate::socket_endpoint::Target::Name { host, port } => {
      let addrs = match crate::dns::lookup(host, *port) {
        Ok(addrs) => addrs,
        Err(crate::dns::LookupError::Resolve) => return Err(Fail::Resolve),
        Err(crate::dns::LookupError::Denied { name, message }) => {
          return Err(Fail::Denied { name, message });
        }
      };
      addrs
        .into_iter()
        .find(|addr| addr.is_ipv4() == ipv4)
        .ok_or(Fail::Type(FAMILY))
    }
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
    addr: Option<v8::Local<v8::Value>>,
  ) -> Result<Js, OpError> {
    let bytes = copy_source(scope, data)?;
    let Some(addr) = addr else {
      if self.shared.is_closed() {
        return crate::errors::reject(scope, "BadResource", CLOSED, NO_PROMISE);
      }
      if !self.shared.connected.load(Ordering::Acquire) {
        return Err(crate::errors::exception(
          scope,
          "NotConnected",
          NOT_CONNECTED,
        ));
      }
      let shared = Arc::clone(&self.shared);
      return settle(scope, Fail::Send, move || {
        send_connected(&shared, &bytes).map(Deliver::Count)
      });
    };
    let target = crate::socket_endpoint::destination(
      scope,
      addr,
      1,
      None,
      PORT_DEST,
      ADDRESS,
      ADDRESS_READ,
    )?;
    if let crate::socket_endpoint::Target::Addr { addr, .. } = &target
      && self.shared.bound.is_ipv4() != addr.is_ipv4()
    {
      return Err(OpError::type_(FAMILY));
    }
    if self.shared.is_closed() {
      return crate::errors::reject(scope, "BadResource", CLOSED, NO_PROMISE);
    }
    let shared = Arc::clone(&self.shared);
    let ipv4 = self.shared.bound.is_ipv4();
    settle(scope, Fail::Send, move || {
      let dest = send_addr(&target, ipv4)?;
      send_datagram(&shared, &bytes, dest).map(Deliver::Count)
    })
  }

  fn connect(
    &self,
    scope: &mut v8::PinScope,
    addr: v8::Local<v8::Value>,
  ) -> Result<Js, OpError> {
    let target = crate::socket_endpoint::destination(
      scope,
      addr,
      1,
      None,
      PORT_DEST,
      ADDRESS,
      ADDRESS_READ,
    )?;
    if let crate::socket_endpoint::Target::Addr { addr, .. } = &target
      && self.shared.bound.is_ipv4() != addr.is_ipv4()
    {
      return Err(OpError::type_(FAMILY));
    }
    if self.shared.is_closed() {
      return crate::errors::reject(scope, "BadResource", CLOSED, NO_PROMISE);
    }
    let shared = Arc::clone(&self.shared);
    let ipv4 = self.shared.bound.is_ipv4();
    settle(scope, Fail::Connect, move || {
      let dest = send_addr(&target, ipv4)?;
      connect_datagram(&shared, dest)?;
      Ok(Deliver::Ready)
    })
  }

  fn disconnect(&self, scope: &mut v8::PinScope) -> Result<(), OpError> {
    if self.shared.is_closed() {
      return Err(crate::errors::exception(scope, "BadResource", CLOSED));
    }
    match disconnect_peer(&self.shared.socket) {
      Ok(()) => {
        self.shared.connected.store(false, Ordering::Release);
        Ok(())
      }
      Err(_) => Err(plain_error(scope, DISCONNECT_FAIL)),
    }
  }

  fn set_broadcast(
    &self,
    scope: &mut v8::PinScope,
    enabled: v8::Local<v8::Value>,
  ) -> Result<(), OpError> {
    if !enabled.is_boolean() {
      return Err(OpError::type_(ENABLED));
    }
    if self.shared.is_closed() {
      return Err(crate::errors::exception(scope, "BadResource", CLOSED));
    }
    match set_broadcast_opt(&self.shared.socket, enabled.is_true()) {
      Ok(()) => Ok(()),
      Err(_) => Err(plain_error(scope, BROADCAST_FAIL)),
    }
  }

  fn join_multicast(
    &self,
    scope: &mut v8::PinScope,
    address: v8::Local<v8::Value>,
    iface: Option<v8::Local<v8::Value>>,
  ) -> Result<(), OpError> {
    multicast(scope, &self.shared, address, iface, true)
  }

  fn leave_multicast(
    &self,
    scope: &mut v8::PinScope,
    address: v8::Local<v8::Value>,
    iface: Option<v8::Local<v8::Value>>,
  ) -> Result<(), OpError> {
    multicast(scope, &self.shared, address, iface, false)
  }

  fn receive(&self, scope: &mut v8::PinScope) -> Result<Js, OpError> {
    if self.shared.is_closed() {
      return crate::errors::reject(scope, "BadResource", CLOSED, NO_PROMISE);
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
      Err(crate::errors::exception(scope, "BadResource", CLOSED))
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
  let reuse = reuse_flag(scope, options)?;
  let shared = match if reuse {
    bind_udp_reuse(addr)
  } else {
    bind_udp(addr)
  } {
    Ok(shared) => shared,
    Err(error) => {
      return Err(crate::errors::io_exception(scope, &error, BIND));
    }
  };
  Ok(New(Datagram { shared }))
}

fn reuse_flag(
  scope: &mut v8::PinScope,
  options: v8::Local<v8::Value>,
) -> Result<bool, OpError> {
  if options.is_null_or_undefined() {
    return Ok(false);
  }
  let Ok(object) = v8::Local::<v8::Object>::try_from(options) else {
    return Ok(false);
  };
  let value = crate::socket_endpoint::read_field(
    scope,
    object,
    "reusePort",
    OPTIONS_READ,
  )?;
  crate::socket_bind::reuse_port(scope, value)
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
  Ready,
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
      Ok(Deliver::Ready) => Ok(v8::undefined(scope).into()),
      Err(fail) => Err(fail),
    };
    match mapped {
      Ok(value) => {
        let _ = resolver.resolve(scope, value);
      }
      Err(fail) => {
        let error = fail_value(scope, &fail);
        let _ = resolver.reject(scope, error);
      }
    }
  });
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
    Fail::Class { class, code } => {
      crate::errors::class_value(scope, *class, *code)
    }
    Fail::Io {
      message,
      code,
      type_error,
    } => crate::errors::coded_message(scope, message, *code, *type_error),
    Fail::Denied { name, message } => {
      crate::errors::denied_value(scope, name, message)
    }
    Fail::Type(message) => {
      let Some(text) = v8::String::new(scope, message) else {
        return v8::undefined(scope).into();
      };
      v8::Exception::type_error(scope, text)
    }
    other => error_value(scope, other.message()),
  }
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

fn plain_error(scope: &mut v8::PinScope, message: &str) -> OpError {
  let Some(text) = v8::String::new(scope, message) else {
    return OpError::type_(message);
  };
  let error = v8::Exception::error(scope, text);
  OpError::thrown(scope, error)
}

enum Membership {
  V4 { group: Ipv4Addr, iface: Ipv4Addr },
  V6 { group: Ipv6Addr, index: u32 },
}

fn multicast(
  scope: &mut v8::PinScope,
  shared: &Shared,
  address: v8::Local<v8::Value>,
  iface: Option<v8::Local<v8::Value>>,
  join: bool,
) -> Result<(), OpError> {
  let group = read_multicast(scope, address)?;
  if shared.bound.is_ipv4() != group.is_ipv4() {
    return Err(OpError::type_(FAMILY));
  }
  let membership = read_membership(scope, group, iface)?;
  if shared.is_closed() {
    return Err(crate::errors::exception(scope, "BadResource", CLOSED));
  }
  let failed = if join { JOIN_FAIL } else { LEAVE_FAIL };
  match set_membership(&shared.socket, &membership, join) {
    Ok(()) => Ok(()),
    Err(_) => Err(plain_error(scope, failed)),
  }
}

fn read_multicast(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<IpAddr, OpError> {
  if !value.is_string() {
    return Err(OpError::type_(MULTICAST));
  }
  let Ok(text) = v8::Local::<v8::String>::try_from(value) else {
    return Err(OpError::type_(MULTICAST));
  };
  let text = text.to_rust_string_lossy(scope);
  let Ok(addr) = text.parse::<IpAddr>() else {
    return Err(OpError::type_(MULTICAST));
  };
  if !addr.is_multicast() {
    return Err(OpError::type_(MULTICAST));
  }
  Ok(addr)
}

fn read_membership(
  scope: &mut v8::PinScope,
  group: IpAddr,
  iface: Option<v8::Local<v8::Value>>,
) -> Result<Membership, OpError> {
  let group_v4 = group.is_ipv4();
  let value = match iface {
    Some(value) if !value.is_null_or_undefined() => value,
    _ => {
      return Ok(match group {
        IpAddr::V4(group) => Membership::V4 {
          group,
          iface: Ipv4Addr::UNSPECIFIED,
        },
        IpAddr::V6(group) => Membership::V6 { group, index: 0 },
      });
    }
  };
  if value.is_string() {
    if !group_v4 {
      return Err(OpError::type_(IFACE_INDEX));
    }
    let Ok(text) = v8::Local::<v8::String>::try_from(value) else {
      return Err(OpError::type_(IFACE_ADDR));
    };
    let Ok(addr) = text.to_rust_string_lossy(scope).parse::<Ipv4Addr>() else {
      return Err(OpError::type_(IFACE_ADDR));
    };
    let IpAddr::V4(group) = group else {
      return Err(OpError::type_(IFACE_INDEX));
    };
    return Ok(Membership::V4 { group, iface: addr });
  }
  if value.is_number() {
    if group_v4 {
      return Err(OpError::type_(IFACE_ADDR));
    }
    let Some(number) = value.number_value(scope) else {
      return Err(OpError::type_(IFACE_INDEX));
    };
    if !number.is_finite()
      || number.fract() != 0.0
      || !(0.0..=4_294_967_295.0).contains(&number)
    {
      return Err(OpError::type_(IFACE_INDEX));
    }
    let IpAddr::V6(group) = group else {
      return Err(OpError::type_(IFACE_ADDR));
    };
    return Ok(Membership::V6 {
      group,
      index: number as u32,
    });
  }
  Err(OpError::type_(if group_v4 {
    IFACE_ADDR
  } else {
    IFACE_INDEX
  }))
}

/// Linux libc names the IPv6 group options `IPV6_ADD_MEMBERSHIP` and
/// `IPV6_DROP_MEMBERSHIP`. macOS names the same options `IPV6_JOIN_GROUP`
/// and `IPV6_LEAVE_GROUP`.
#[cfg(unix)]
fn ipv6_membership(join: bool) -> libc::c_int {
  #[cfg(target_os = "linux")]
  {
    if join {
      libc::IPV6_ADD_MEMBERSHIP
    } else {
      libc::IPV6_DROP_MEMBERSHIP
    }
  }
  #[cfg(not(target_os = "linux"))]
  {
    if join {
      libc::IPV6_JOIN_GROUP
    } else {
      libc::IPV6_LEAVE_GROUP
    }
  }
}

#[cfg(unix)]
fn set_broadcast_opt(socket: &UdpSocket, enabled: bool) -> io::Result<()> {
  set_int_opt(
    socket,
    libc::SOL_SOCKET,
    libc::SO_BROADCAST,
    if enabled { 1 } else { 0 },
  )
}

#[cfg(unix)]
fn set_membership(
  socket: &UdpSocket,
  membership: &Membership,
  join: bool,
) -> io::Result<()> {
  match membership {
    Membership::V4 { group, iface } => {
      let mut req: libc::ip_mreq = unsafe { std::mem::zeroed() };
      req.imr_multiaddr.s_addr = u32::from(*group).to_be();
      req.imr_interface.s_addr = u32::from(*iface).to_be();
      let name = if join {
        libc::IP_ADD_MEMBERSHIP
      } else {
        libc::IP_DROP_MEMBERSHIP
      };
      set_bytes(socket, libc::IPPROTO_IP, name, &req)
    }
    Membership::V6 { group, index } => {
      let mut req: libc::ipv6_mreq = unsafe { std::mem::zeroed() };
      req.ipv6mr_multiaddr.s6_addr = group.octets();
      req.ipv6mr_interface = *index as libc::c_uint;
      set_bytes(socket, libc::IPPROTO_IPV6, ipv6_membership(join), &req)
    }
  }
}

#[cfg(unix)]
fn disconnect_peer(socket: &UdpSocket) -> io::Result<()> {
  use std::os::fd::AsRawFd;
  let mut addr: libc::sockaddr = unsafe { std::mem::zeroed() };
  set_unspec_len(&mut addr);
  addr.sa_family = libc::AF_UNSPEC as libc::sa_family_t;
  let rc = unsafe {
    libc::connect(
      socket.as_raw_fd(),
      &addr,
      std::mem::size_of::<libc::sockaddr>() as libc::socklen_t,
    )
  };
  if rc == 0 {
    return Ok(());
  }
  let error = io::Error::last_os_error();
  match error.raw_os_error() {
    Some(code)
      if code == libc::EAFNOSUPPORT
        || code == libc::EINVAL
        || code == libc::ENOTCONN =>
    {
      Ok(())
    }
    _ => Err(error),
  }
}

#[cfg(all(
  unix,
  any(
    target_os = "macos",
    target_os = "ios",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd",
    target_os = "dragonfly",
  )
))]
fn set_unspec_len(addr: &mut libc::sockaddr) {
  addr.sa_len = std::mem::size_of::<libc::sockaddr>() as u8;
}

#[cfg(all(
  unix,
  not(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd",
    target_os = "dragonfly",
  ))
))]
fn set_unspec_len(_addr: &mut libc::sockaddr) {}

#[cfg(unix)]
fn set_int_opt(
  socket: &UdpSocket,
  level: libc::c_int,
  name: libc::c_int,
  value: libc::c_int,
) -> io::Result<()> {
  set_bytes(socket, level, name, &value)
}

#[cfg(unix)]
fn set_bytes<T>(
  socket: &UdpSocket,
  level: libc::c_int,
  name: libc::c_int,
  value: &T,
) -> io::Result<()> {
  use std::os::fd::AsRawFd;
  let rc = unsafe {
    libc::setsockopt(
      socket.as_raw_fd(),
      level,
      name,
      value as *const T as *const libc::c_void,
      std::mem::size_of::<T>() as libc::socklen_t,
    )
  };
  if rc == 0 {
    Ok(())
  } else {
    Err(io::Error::last_os_error())
  }
}

#[cfg(windows)]
fn set_broadcast_opt(socket: &UdpSocket, enabled: bool) -> io::Result<()> {
  use windows_sys::Win32::Networking::WinSock::{SO_BROADCAST, SOL_SOCKET};
  let value: i32 = if enabled { 1 } else { 0 };
  set_bytes(socket, SOL_SOCKET, SO_BROADCAST, &value)
}

#[cfg(windows)]
fn set_membership(
  socket: &UdpSocket,
  membership: &Membership,
  join: bool,
) -> io::Result<()> {
  use windows_sys::Win32::Networking::WinSock::{
    IP_ADD_MEMBERSHIP, IP_DROP_MEMBERSHIP, IP_MREQ, IPPROTO_IP, IPPROTO_IPV6,
    IPV6_JOIN_GROUP, IPV6_LEAVE_GROUP, IPV6_MREQ,
  };
  match membership {
    Membership::V4 { group, iface } => {
      let mut req = IP_MREQ::default();
      unsafe {
        req.imr_multiaddr.S_un.S_addr = u32::from(*group).to_be();
        req.imr_interface.S_un.S_addr = u32::from(*iface).to_be();
      }
      let name = if join {
        IP_ADD_MEMBERSHIP
      } else {
        IP_DROP_MEMBERSHIP
      };
      set_bytes(socket, IPPROTO_IP, name, &req)
    }
    Membership::V6 { group, index } => {
      let mut req = IPV6_MREQ::default();
      unsafe {
        req.ipv6mr_multiaddr.u.Byte = group.octets();
      }
      req.ipv6mr_interface = *index;
      let name = if join {
        IPV6_JOIN_GROUP
      } else {
        IPV6_LEAVE_GROUP
      };
      set_bytes(socket, IPPROTO_IPV6, name, &req)
    }
  }
}

#[cfg(windows)]
fn disconnect_peer(socket: &UdpSocket) -> io::Result<()> {
  use std::os::windows::io::AsRawSocket;
  use windows_sys::Win32::Networking::WinSock::{
    SOCKADDR, WSAEAFNOSUPPORT, WSAEINVAL, WSAENOTCONN, connect,
  };
  let addr = SOCKADDR::default();
  let rc = unsafe {
    connect(
      socket.as_raw_socket() as _,
      &addr,
      std::mem::size_of::<SOCKADDR>() as i32,
    )
  };
  if rc == 0 {
    return Ok(());
  }
  let error = io::Error::last_os_error();
  match error.raw_os_error() {
    Some(code)
      if code == WSAEAFNOSUPPORT
        || code == WSAEINVAL
        || code == WSAENOTCONN =>
    {
      Ok(())
    }
    _ => Err(error),
  }
}

#[cfg(windows)]
fn set_bytes<T>(
  socket: &UdpSocket,
  level: i32,
  name: i32,
  value: &T,
) -> io::Result<()> {
  use std::os::windows::io::AsRawSocket;
  use windows_sys::Win32::Networking::WinSock::setsockopt;
  let rc = unsafe {
    setsockopt(
      socket.as_raw_socket() as _,
      level,
      name,
      value as *const T as windows_sys::core::PCSTR,
      std::mem::size_of::<T>() as i32,
    )
  };
  if rc == 0 {
    Ok(())
  } else {
    Err(io::Error::last_os_error())
  }
}

#[cfg(not(any(unix, windows)))]
fn set_broadcast_opt(_socket: &UdpSocket, _enabled: bool) -> io::Result<()> {
  Err(io::Error::new(io::ErrorKind::Unsupported, BROADCAST_FAIL))
}

#[cfg(not(any(unix, windows)))]
fn set_membership(
  _socket: &UdpSocket,
  _membership: &Membership,
  _join: bool,
) -> io::Result<()> {
  Err(io::Error::new(io::ErrorKind::Unsupported, JOIN_FAIL))
}

#[cfg(not(any(unix, windows)))]
fn disconnect_peer(_socket: &UdpSocket) -> io::Result<()> {
  Err(io::Error::new(io::ErrorKind::Unsupported, DISCONNECT_FAIL))
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
