//! A WebSocket client.
//!
//! The handshake is HTTP/1.1. Client frames are masked. Extension bits are
//! rejected, so `extensions` stays empty. `wss:` uses the webpki root set.

use std::collections::VecDeque;
use std::future::Future;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::pin::Pin;
use std::sync::{Arc, Condvar, Mutex};
use std::task::{Context, Poll, Waker};
use std::time::Duration;

use sha1::Sha1;
use sha2::Digest;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

const GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";
const MAX_MESSAGE: usize = 16 * 1024 * 1024;

pub enum Command {
  Text(String),
  Binary(Vec<u8>),
  Close { code: Option<u16>, reason: String },
  Abort,
}

#[derive(Clone, Debug)]
pub enum Note {
  Open {
    protocol: String,
  },
  Text(String),
  Binary(Vec<u8>),
  Flushed(u64),
  Error,
  Closed {
    code: u16,
    reason: String,
    clean: bool,
  },
}

type Wake = Arc<dyn Fn() + Send + Sync>;

#[derive(Clone)]
pub struct NoteQueue {
  notes: Arc<Mutex<VecDeque<Note>>>,
  wake: Arc<Mutex<Option<Wake>>>,
  ready: Arc<Condvar>,
}

impl NoteQueue {
  fn new() -> Self {
    Self {
      notes: Arc::new(Mutex::new(VecDeque::new())),
      wake: Arc::new(Mutex::new(None)),
      ready: Arc::new(Condvar::new()),
    }
  }

  pub fn push(&self, note: Note) {
    self.notes.lock().expect("websocket notes").push_back(note);
    self.ready.notify_all();
    let wake = self.wake.lock().expect("websocket wake").clone();
    if let Some(wake) = wake {
      wake();
    }
  }

  pub fn poll(&self) -> Option<Note> {
    self.notes.lock().expect("websocket notes").pop_front()
  }

  pub fn listen(&self, wake: Wake) {
    *self.wake.lock().expect("websocket wake") = Some(Arc::clone(&wake));
    if !self.notes.lock().expect("websocket notes").is_empty() {
      wake();
    }
  }
}

#[derive(Clone)]
pub struct Connection {
  pub notes: NoteQueue,
  commands: tokio::sync::mpsc::UnboundedSender<Command>,
}

thread_local! {
  static COMMANDS: std::cell::RefCell<
    Vec<tokio::sync::mpsc::UnboundedSender<Command>>,
  > = const { std::cell::RefCell::new(Vec::new()) };
}

pub fn abort_all() {
  let commands = COMMANDS.with(|slot| std::mem::take(&mut *slot.borrow_mut()));
  for commands in commands {
    let _ = commands.send(Command::Abort);
  }
}

impl Connection {
  pub fn command(&self, command: Command) {
    let _ = self.commands.send(command);
  }

  /// A connection that never reaches the network.
  pub fn failed() -> Self {
    let notes = NoteQueue::new();
    notes.push(Note::Error);
    notes.push(Note::Closed {
      code: 1006,
      reason: String::new(),
      clean: false,
    });
    let (commands, _) = tokio::sync::mpsc::unbounded_channel();
    Self { notes, commands }
  }
}

/// Address space of a connected peer. `targetAddressSpace` requires an exact
/// match. `private` is not a member of this set.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AddressSpace {
  Loopback,
  Local,
  Public,
}

pub fn address_space(ip: IpAddr) -> AddressSpace {
  match ip {
    IpAddr::V4(ip) => ipv4_space(ip),
    IpAddr::V6(ip) => {
      if let Some(mapped) = ip.to_ipv4_mapped() {
        return ipv4_space(mapped);
      }
      if ip.is_loopback() {
        AddressSpace::Loopback
      } else if ipv6_local(ip) {
        AddressSpace::Local
      } else {
        AddressSpace::Public
      }
    }
  }
}

fn ipv4_space(ip: Ipv4Addr) -> AddressSpace {
  if ip.is_loopback() {
    AddressSpace::Loopback
  } else if ip.is_private() || ip.is_link_local() || cgnat(ip) {
    AddressSpace::Local
  } else {
    AddressSpace::Public
  }
}

fn cgnat(ip: Ipv4Addr) -> bool {
  let octets = ip.octets();
  octets[0] == 100 && (octets[1] & 0xC0) == 64
}

fn ipv6_local(ip: Ipv6Addr) -> bool {
  let octets = ip.octets();
  let unique_local = octets[0] & 0xFE == 0xFC;
  let link_local = octets[0] == 0xFE && octets[1] & 0xC0 == 0x80;
  unique_local || link_local
}

/// Caps how many data messages a `WebSocketStream` reads ahead.
pub struct PullGate {
  inner: Mutex<PullInner>,
}

struct PullInner {
  permits: u32,
  waker: Option<Waker>,
}

impl PullGate {
  pub fn new() -> Arc<Self> {
    Arc::new(Self {
      inner: Mutex::new(PullInner {
        permits: 0,
        waker: None,
      }),
    })
  }

  pub fn grant(&self) {
    let mut inner = self.inner.lock().expect("pull gate");
    inner.permits = inner.permits.saturating_add(1);
    if let Some(waker) = inner.waker.take() {
      waker.wake();
    }
  }

  fn permits(&self) -> u32 {
    self.inner.lock().expect("pull gate").permits
  }

  pub(crate) fn take(&self) {
    let mut inner = self.inner.lock().expect("pull gate");
    inner.permits = inner.permits.saturating_sub(1);
  }

  pub(crate) fn wait(&self) -> PullWait<'_> {
    PullWait { gate: self }
  }
}

pub(crate) struct PullWait<'a> {
  gate: &'a PullGate,
}

impl Future for PullWait<'_> {
  type Output = ();

  fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
    let mut inner = self.gate.inner.lock().expect("pull gate");
    if inner.permits > 0 {
      return Poll::Ready(());
    }
    inner.waker = Some(cx.waker().clone());
    Poll::Pending
  }
}

pub fn connect(
  url: url::Url,
  protocols: Vec<String>,
  origin: String,
  target: Option<AddressSpace>,
  pull: Option<Arc<PullGate>>,
) -> Connection {
  let notes = NoteQueue::new();
  let (commands, inbox) = tokio::sync::mpsc::unbounded_channel();
  COMMANDS.with(|slot| slot.borrow_mut().push(commands.clone()));
  let queued = notes.clone();
  std::thread::spawn(move || {
    let runtime = match tokio::runtime::Builder::new_current_thread()
      .enable_all()
      .build()
    {
      Ok(runtime) => runtime,
      Err(_) => {
        fail(&queued);
        return;
      }
    };
    runtime
      .block_on(session(url, protocols, origin, target, pull, inbox, queued));
  });
  Connection { notes, commands }
}

pub fn environment_origin() -> String {
  let Some(entry) = yuzuko_core::entry_url() else {
    return "null".to_string();
  };
  let Ok(url) = url::Url::parse(&entry) else {
    return "null".to_string();
  };
  url.origin().ascii_serialization()
}

pub fn is_bad_port(port: u16) -> bool {
  const BAD: &[u16] = &[
    0, 1, 7, 9, 11, 13, 15, 17, 19, 20, 21, 22, 23, 25, 37, 42, 43, 53, 69, 77,
    79, 87, 95, 101, 102, 103, 104, 109, 110, 111, 113, 115, 117, 119, 123,
    135, 137, 139, 143, 161, 179, 389, 427, 465, 512, 513, 514, 515, 526, 530,
    531, 532, 540, 548, 554, 556, 563, 587, 601, 636, 989, 990, 993, 995, 1719,
    1720, 1723, 2049, 3659, 4045, 4190, 5060, 5061, 6000, 6566, 6665, 6666,
    6667, 6668, 6669, 6679, 6697, 10080,
  ];
  BAD.contains(&port)
}

pub fn valid_protocol(value: &str) -> bool {
  !value.is_empty()
    && value
      .bytes()
      .all(|byte| (0x21..=0x7E).contains(&byte) && !separator(byte))
}

fn separator(byte: u8) -> bool {
  matches!(
    byte,
    b'('
      | b')'
      | b'<'
      | b'>'
      | b'@'
      | b','
      | b';'
      | b':'
      | b'\\'
      | b'"'
      | b'/'
      | b'['
      | b']'
      | b'?'
      | b'='
      | b'{'
      | b'}'
      | b' '
      | 0x09
  )
}

async fn session(
  url: url::Url,
  protocols: Vec<String>,
  origin: String,
  target: Option<AddressSpace>,
  pull: Option<Arc<PullGate>>,
  mut inbox: tokio::sync::mpsc::UnboundedReceiver<Command>,
  notes: NoteQueue,
) {
  let stream = match open_stream(&url, target).await {
    Ok(stream) => stream,
    Err(()) => {
      fail(&notes);
      return;
    }
  };
  let key = random_key();
  let request = handshake_request(&url, &protocols, &origin, &key);
  let mut stream = stream;
  if stream.write_all(&request).await.is_err() {
    fail(&notes);
    return;
  }
  let head = tokio::select! {
    biased;
    _ = inbox.recv() => {
      fail(&notes);
      return;
    }
    head = read_head(&mut stream) => head,
  };
  let Ok((status, headers, rest)) = head else {
    fail(&notes);
    return;
  };
  if status != 101 || !accept_matches(&headers, &key) {
    fail(&notes);
    return;
  }
  let protocol = header(&headers, "sec-websocket-protocol")
    .unwrap_or("")
    .trim()
    .to_string();
  if !protocol_ok(&protocols, &protocol) {
    fail(&notes);
    return;
  }
  notes.push(Note::Open {
    protocol: protocol.clone(),
  });
  let failed = if let Some(pull) = pull {
    drive_flow(stream, rest, &mut inbox, &notes, pull)
      .await
      .is_err()
  } else {
    drive(stream, rest, &mut inbox, &notes).await.is_err()
  };
  if failed {
    fail(&notes);
  }
}

async fn open_stream(
  url: &url::Url,
  target: Option<AddressSpace>,
) -> Result<Stream, ()> {
  let host = url.host_str().ok_or(())?;
  let port = url.port_or_known_default().ok_or(())?;
  let tcp = tokio::time::timeout(
    Duration::from_secs(10),
    tokio::net::TcpStream::connect((host, port)),
  )
  .await
  .map_err(|_| ())?
  .map_err(|_| ())?;
  let peer = tcp.peer_addr().map_err(|_| ())?.ip();
  if target.is_some_and(|space| address_space(peer) != space) {
    return Err(());
  }
  if url.scheme() == "ws" {
    return Ok(Stream::Tcp(tcp));
  }
  let name = rustls::pki_types::ServerName::try_from(host.to_string())
    .map_err(|_| ())?;
  let connector = tls_connector();
  let tls = connector.connect(name, tcp).await.map_err(|_| ())?;
  Ok(Stream::Tls(Box::new(tls)))
}

enum Stream {
  Tcp(tokio::net::TcpStream),
  Tls(Box<tokio_rustls::client::TlsStream<tokio::net::TcpStream>>),
}

impl AsyncRead for Stream {
  fn poll_read(
    self: std::pin::Pin<&mut Self>,
    cx: &mut std::task::Context<'_>,
    buf: &mut tokio::io::ReadBuf<'_>,
  ) -> std::task::Poll<std::io::Result<()>> {
    match self.get_mut() {
      Stream::Tcp(stream) => std::pin::Pin::new(stream).poll_read(cx, buf),
      Stream::Tls(stream) => std::pin::Pin::new(stream).poll_read(cx, buf),
    }
  }
}

impl AsyncWrite for Stream {
  fn poll_write(
    self: std::pin::Pin<&mut Self>,
    cx: &mut std::task::Context<'_>,
    buf: &[u8],
  ) -> std::task::Poll<std::io::Result<usize>> {
    match self.get_mut() {
      Stream::Tcp(stream) => std::pin::Pin::new(stream).poll_write(cx, buf),
      Stream::Tls(stream) => std::pin::Pin::new(stream).poll_write(cx, buf),
    }
  }

  fn poll_flush(
    self: std::pin::Pin<&mut Self>,
    cx: &mut std::task::Context<'_>,
  ) -> std::task::Poll<std::io::Result<()>> {
    match self.get_mut() {
      Stream::Tcp(stream) => std::pin::Pin::new(stream).poll_flush(cx),
      Stream::Tls(stream) => std::pin::Pin::new(stream).poll_flush(cx),
    }
  }

  fn poll_shutdown(
    self: std::pin::Pin<&mut Self>,
    cx: &mut std::task::Context<'_>,
  ) -> std::task::Poll<std::io::Result<()>> {
    match self.get_mut() {
      Stream::Tcp(stream) => std::pin::Pin::new(stream).poll_shutdown(cx),
      Stream::Tls(stream) => std::pin::Pin::new(stream).poll_shutdown(cx),
    }
  }
}

fn tls_connector() -> tokio_rustls::TlsConnector {
  static CONFIG: std::sync::OnceLock<Arc<rustls::ClientConfig>> =
    std::sync::OnceLock::new();
  let config = CONFIG.get_or_init(|| {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let mut roots = rustls::RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    Arc::new(
      rustls::ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth(),
    )
  });
  tokio_rustls::TlsConnector::from(Arc::clone(config))
}

fn handshake_request(
  url: &url::Url,
  protocols: &[String],
  origin: &str,
  key: &str,
) -> Vec<u8> {
  let mut path = url.path().to_string();
  if path.is_empty() {
    path.push('/');
  }
  if let Some(query) = url.query() {
    path.push('?');
    path.push_str(query);
  }
  let host = match url.host() {
    Some(url::Host::Ipv6(address)) => format!("[{address}]"),
    Some(host) => host.to_string(),
    None => String::new(),
  };
  let port = url.port_or_known_default().unwrap_or(80);
  let default = matches!((url.scheme(), port), ("ws", 80) | ("wss", 443));
  let host_header = if default {
    host
  } else {
    format!("{host}:{port}")
  };
  let mut message = format!(
    "GET {path} HTTP/1.1\r\nHost: {host_header}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\nOrigin: {origin}\r\n"
  );
  if !protocols.is_empty() {
    message.push_str("Sec-WebSocket-Protocol: ");
    message.push_str(&protocols.join(", "));
    message.push_str("\r\n");
  }
  if !url.username().is_empty() || url.password().is_some() {
    let user = url.username();
    let pass = url.password().unwrap_or("");
    let token = base64_encode(format!("{user}:{pass}").as_bytes());
    message.push_str("Authorization: Basic ");
    message.push_str(&token);
    message.push_str("\r\n");
  }
  message.push_str("\r\n");
  message.into_bytes()
}

fn random_key() -> String {
  let mut bytes = [0u8; 16];
  getrandom::fill(&mut bytes).ok();
  base64_encode(&bytes)
}

fn accept_matches(headers: &[(String, String)], key: &str) -> bool {
  let Some(value) = header(headers, "sec-websocket-accept") else {
    return false;
  };
  let mut data = key.as_bytes().to_vec();
  data.extend(GUID.as_bytes());
  let digest = Sha1::digest(&data);
  value.trim() == base64_encode(&digest)
}

fn protocol_ok(offered: &[String], selected: &str) -> bool {
  if offered.is_empty() {
    return selected.is_empty();
  }
  offered.iter().any(|protocol| protocol == selected)
}

fn header<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
  headers
    .iter()
    .find(|(key, _)| key.eq_ignore_ascii_case(name))
    .map(|(_, value)| value.as_str())
}

async fn read_head<S: AsyncRead + Unpin>(
  stream: &mut S,
) -> Result<(u16, Vec<(String, String)>, Vec<u8>), ()> {
  let mut bytes = Vec::new();
  let mut buf = [0u8; 1024];
  let split = loop {
    if bytes.len() > 64 * 1024 {
      return Err(());
    }
    let read =
      tokio::time::timeout(Duration::from_secs(20), stream.read(&mut buf))
        .await
        .map_err(|_| ())?
        .map_err(|_| ())?;
    if read == 0 {
      return Err(());
    }
    bytes.extend(&buf[..read]);
    if let Some(index) = find_header_end(&bytes) {
      break index;
    }
  };
  let rest = bytes.split_off(split);
  let text = String::from_utf8_lossy(&bytes[..split.saturating_sub(4)]);
  let mut lines = text.split("\r\n");
  let status_line = lines.next().ok_or(())?;
  let status = status_line
    .split_whitespace()
    .nth(1)
    .and_then(|value| value.parse().ok())
    .ok_or(())?;
  let mut headers = Vec::new();
  for line in lines {
    if line.is_empty() {
      continue;
    }
    let Some((name, value)) = line.split_once(':') else {
      continue;
    };
    headers.push((name.trim().to_ascii_lowercase(), value.trim().to_string()));
  }
  Ok((status, headers, rest))
}

fn find_header_end(bytes: &[u8]) -> Option<usize> {
  bytes
    .windows(4)
    .position(|window| window == b"\r\n\r\n")
    .map(|index| index + 4)
}

async fn drive<S: AsyncRead + AsyncWrite + Unpin>(
  mut stream: S,
  rest: Vec<u8>,
  inbox: &mut tokio::sync::mpsc::UnboundedReceiver<Command>,
  notes: &NoteQueue,
) -> Result<(), ()> {
  let mut incoming = rest;
  let mut message = Partial::default();
  let mut sent_close = false;
  let mut buf = [0u8; 8192];
  loop {
    tokio::select! {
      biased;
      command = inbox.recv() => {
        match command {
          Some(Command::Abort) | None => return Err(()),
          Some(Command::Text(text)) => {
            write_data(&mut stream, 0x1, text.as_bytes()).await?;
            notes.push(Note::Flushed(text.len() as u64));
          }
          Some(Command::Binary(bytes)) => {
            let len = bytes.len() as u64;
            write_data(&mut stream, 0x2, &bytes).await?;
            notes.push(Note::Flushed(len));
          }
          Some(Command::Close { code, reason }) => {
            write_close(&mut stream, code, &reason).await?;
            sent_close = true;
          }
        }
      }
      read = stream.read(&mut buf) => {
        let read = read.map_err(|_| ())?;
        if read == 0 {
          return Err(());
        }
        incoming.extend(&buf[..read]);
        while let Some((frame, size)) = take_frame(&incoming)? {
          incoming.drain(..size);
          if frame.opcode >= 0x8 {
            if control(frame, &mut stream, notes, &mut sent_close).await? {
              return Ok(());
            }
            continue;
          }
          message.push(frame)?;
          if let Some(done) = message.take() {
            match done {
              0x1 => {
                let text = String::from_utf8(done_bytes(&message)).map_err(|_| ())?;
                message.clear();
                notes.push(Note::Text(text));
              }
              0x2 => {
                let bytes = done_bytes(&message);
                message.clear();
                notes.push(Note::Binary(bytes));
              }
              _ => return Err(()),
            }
          }
        }
      }
    }
  }
}

fn done_bytes(message: &Partial) -> Vec<u8> {
  message.payload.clone()
}

async fn control<S: AsyncWrite + Unpin>(
  frame: Frame,
  stream: &mut S,
  notes: &NoteQueue,
  sent_close: &mut bool,
) -> Result<bool, ()> {
  match frame.opcode {
    0x8 => {
      let (code, reason) = parse_close(&frame.payload)?;
      if !*sent_close {
        let echo = (code != 1005).then_some(code);
        write_close(stream, echo, &reason).await?;
      }
      notes.push(Note::Closed {
        code,
        reason,
        clean: true,
      });
      Ok(true)
    }
    0x9 => {
      write_frame(stream, 0xA, &frame.payload, true).await?;
      Ok(false)
    }
    0xA => Ok(false),
    _ => Err(()),
  }
}

fn parse_close(payload: &[u8]) -> Result<(u16, String), ()> {
  if payload.is_empty() {
    return Ok((1005, String::new()));
  }
  if payload.len() < 2 {
    return Err(());
  }
  let code = u16::from_be_bytes([payload[0], payload[1]]);
  let reason = std::str::from_utf8(&payload[2..]).map_err(|_| ())?;
  Ok((code, reason.to_string()))
}

async fn write_data<S: AsyncWrite + Unpin>(
  stream: &mut S,
  opcode: u8,
  payload: &[u8],
) -> Result<(), ()> {
  write_frame(stream, opcode, payload, true).await
}

async fn write_close<S: AsyncWrite + Unpin>(
  stream: &mut S,
  code: Option<u16>,
  reason: &str,
) -> Result<(), ()> {
  let mut payload = Vec::new();
  if let Some(code) = code {
    payload.extend(code.to_be_bytes());
    payload.extend(reason.as_bytes());
  }
  write_frame(stream, 0x8, &payload, true).await
}

async fn write_frame<S: AsyncWrite + Unpin>(
  stream: &mut S,
  opcode: u8,
  payload: &[u8],
  fin: bool,
) -> Result<(), ()> {
  let frame = encode_client_frame(opcode, payload, fin, random_mask());
  stream.write_all(&frame).await.map_err(|_| ())?;
  stream.flush().await.map_err(|_| ())
}

fn random_mask() -> [u8; 4] {
  let mut mask = [0u8; 4];
  getrandom::fill(&mut mask).ok();
  mask
}

fn encode_client_frame(
  opcode: u8,
  payload: &[u8],
  fin: bool,
  mask: [u8; 4],
) -> Vec<u8> {
  let mut frame = Vec::new();
  let head = if fin { 0x80 } else { 0 } | opcode;
  frame.push(head);
  let len = payload.len();
  if len < 126 {
    frame.push(0x80 | len as u8);
  } else if len <= u16::MAX as usize {
    frame.push(0x80 | 126);
    frame.extend((len as u16).to_be_bytes());
  } else {
    frame.push(0x80 | 127);
    frame.extend((len as u64).to_be_bytes());
  }
  frame.extend(mask);
  for (index, byte) in payload.iter().enumerate() {
    frame.push(byte ^ mask[index % 4]);
  }
  frame
}

struct Frame {
  fin: bool,
  opcode: u8,
  payload: Vec<u8>,
}

fn take_frame(bytes: &[u8]) -> Result<Option<(Frame, usize)>, ()> {
  if bytes.len() < 2 {
    return Ok(None);
  }
  let fin = bytes[0] & 0x80 != 0;
  if bytes[0] & 0x70 != 0 {
    return Err(());
  }
  let opcode = bytes[0] & 0x0F;
  let masked = bytes[1] & 0x80 != 0;
  if masked {
    return Err(());
  }
  let (len, mut offset) = match bytes[1] & 0x7F {
    126 => {
      if bytes.len() < 4 {
        return Ok(None);
      }
      (u16::from_be_bytes([bytes[2], bytes[3]]) as usize, 4)
    }
    127 => {
      if bytes.len() < 10 {
        return Ok(None);
      }
      let value = u64::from_be_bytes(bytes[2..10].try_into().map_err(|_| ())?);
      if value > MAX_MESSAGE as u64 {
        return Err(());
      }
      (value as usize, 10)
    }
    short => (short as usize, 2),
  };
  if len > MAX_MESSAGE {
    return Err(());
  }
  if bytes.len() < offset + len {
    return Ok(None);
  }
  let payload = bytes[offset..offset + len].to_vec();
  offset += len;
  let _ = masked;
  Ok(Some((
    Frame {
      fin,
      opcode,
      payload,
    },
    offset,
  )))
}

#[derive(Default)]
struct Partial {
  opcode: u8,
  payload: Vec<u8>,
  active: bool,
}

impl Partial {
  fn push(&mut self, frame: Frame) -> Result<(), ()> {
    if frame.opcode == 0 {
      if !self.active {
        return Err(());
      }
      self.payload.extend(frame.payload);
    } else {
      if self.active {
        return Err(());
      }
      if frame.opcode != 0x1 && frame.opcode != 0x2 {
        return Err(());
      }
      self.opcode = frame.opcode;
      self.payload = frame.payload;
      self.active = true;
    }
    if self.payload.len() > MAX_MESSAGE {
      return Err(());
    }
    if frame.fin {
      self.active = false;
    }
    Ok(())
  }

  fn take(&self) -> Option<u8> {
    if self.active || self.opcode == 0 {
      None
    } else {
      Some(self.opcode)
    }
  }

  fn clear(&mut self) {
    *self = Self::default();
  }
}

enum OutKind {
  Data(u64),
  Close,
  Pong,
}

struct Outgoing {
  frame: Vec<u8>,
  sent: usize,
  kind: OutKind,
}

/// Pull-mode session. Data messages are read only while the gate has a
/// permit, so a full readable queue stops the socket read.
async fn drive_flow<S>(
  stream: S,
  rest: Vec<u8>,
  inbox: &mut tokio::sync::mpsc::UnboundedReceiver<Command>,
  notes: &NoteQueue,
  gate: Arc<PullGate>,
) -> Result<(), ()>
where
  S: AsyncRead + AsyncWrite + Unpin,
{
  let (mut reader, mut writer) = tokio::io::split(stream);
  let mut incoming = rest;
  let mut message = Partial::default();
  let mut sent_close = false;
  let mut outgoing: Option<Outgoing> = None;
  let mut later = VecDeque::new();
  let mut pongs = VecDeque::new();
  let mut buf = [0u8; 8192];
  loop {
    if consume(&mut Flow {
      incoming: &mut incoming,
      message: &mut message,
      writer: &mut writer,
      outgoing: &outgoing,
      sent_close: &mut sent_close,
      gate: &gate,
      notes,
      pongs: &mut pongs,
    })
    .await?
    {
      return Ok(());
    }
    if outgoing.is_none() {
      if let Some(frame) = pongs.pop_front() {
        outgoing = Some(Outgoing {
          frame,
          sent: 0,
          kind: OutKind::Pong,
        });
        continue;
      }
      if let Some(command) = later.pop_front() {
        outgoing = Some(outgoing_from(command));
        continue;
      }
    }
    let allow_read = sent_close || gate.permits() > 0;
    if let Some(current) = outgoing.as_mut() {
      let mut finished = false;
      tokio::select! {
        biased;
        command = inbox.recv() => match command {
          Some(Command::Abort) | None => return Err(()),
          Some(command) => later.push_back(command),
        },
        read = reader.read(&mut buf), if allow_read => {
          let read = read.map_err(|_| ())?;
          if read == 0 {
            return Err(());
          }
          incoming.extend(&buf[..read]);
        }
        wrote = write_out(&mut writer, current) => {
          finished = wrote?;
        }
      }
      if finished {
        finish_out(outgoing.take(), &mut sent_close, notes);
      }
      continue;
    }
    if allow_read {
      tokio::select! {
        biased;
        command = inbox.recv() => {
          outgoing = Some(accept_command(command)?);
        }
        read = reader.read(&mut buf) => {
          let read = read.map_err(|_| ())?;
          if read == 0 {
            return Err(());
          }
          incoming.extend(&buf[..read]);
        }
      }
    } else {
      tokio::select! {
        biased;
        command = inbox.recv() => {
          outgoing = Some(accept_command(command)?);
        }
        _ = gate.wait() => {}
      }
    }
  }
}

fn accept_command(command: Option<Command>) -> Result<Outgoing, ()> {
  match command {
    Some(Command::Abort) | None => Err(()),
    Some(command) => Ok(outgoing_from(command)),
  }
}

fn finish_out(
  outgoing: Option<Outgoing>,
  sent_close: &mut bool,
  notes: &NoteQueue,
) {
  let Some(outgoing) = outgoing else {
    return;
  };
  match outgoing.kind {
    OutKind::Data(len) => notes.push(Note::Flushed(len)),
    OutKind::Close => *sent_close = true,
    OutKind::Pong => {}
  }
}

fn outgoing_from(command: Command) -> Outgoing {
  match command {
    Command::Text(text) => Outgoing {
      frame: encode_client_frame(0x1, text.as_bytes(), true, random_mask()),
      sent: 0,
      kind: OutKind::Data(text.len() as u64),
    },
    Command::Binary(bytes) => {
      let len = bytes.len() as u64;
      Outgoing {
        frame: encode_client_frame(0x2, &bytes, true, random_mask()),
        sent: 0,
        kind: OutKind::Data(len),
      }
    }
    Command::Close { code, reason } => Outgoing {
      frame: close_frame(code, &reason),
      sent: 0,
      kind: OutKind::Close,
    },
    Command::Abort => unreachable!("abort ends the session"),
  }
}

fn close_frame(code: Option<u16>, reason: &str) -> Vec<u8> {
  let mut payload = Vec::new();
  if let Some(code) = code {
    payload.extend(code.to_be_bytes());
    payload.extend(reason.as_bytes());
  }
  encode_client_frame(0x8, &payload, true, random_mask())
}

async fn write_out<W>(
  writer: &mut W,
  outgoing: &mut Outgoing,
) -> Result<bool, ()>
where
  W: AsyncWrite + Unpin,
{
  if outgoing.sent >= outgoing.frame.len() {
    writer.flush().await.map_err(|_| ())?;
    return Ok(true);
  }
  let written = writer
    .write(&outgoing.frame[outgoing.sent..])
    .await
    .map_err(|_| ())?;
  if written == 0 {
    return Err(());
  }
  outgoing.sent += written;
  if outgoing.sent < outgoing.frame.len() {
    return Ok(false);
  }
  writer.flush().await.map_err(|_| ())?;
  Ok(true)
}

struct Flow<'a, W> {
  incoming: &'a mut Vec<u8>,
  message: &'a mut Partial,
  writer: &'a mut W,
  outgoing: &'a Option<Outgoing>,
  sent_close: &'a mut bool,
  gate: &'a PullGate,
  notes: &'a NoteQueue,
  pongs: &'a mut VecDeque<Vec<u8>>,
}

async fn consume<W>(flow: &mut Flow<'_, W>) -> Result<bool, ()>
where
  W: AsyncWrite + Unpin,
{
  loop {
    let Some((frame, size)) = take_frame(flow.incoming)? else {
      return Ok(false);
    };
    if frame.opcode >= 0x8 {
      flow.incoming.drain(..size);
      if control_flow(
        frame,
        flow.writer,
        flow.outgoing,
        flow.sent_close,
        flow.notes,
        flow.pongs,
      )
      .await?
      {
        return Ok(true);
      }
      continue;
    }
    let allowed = *flow.sent_close || flow.gate.permits() > 0;
    if !allowed {
      return Ok(false);
    }
    flow.incoming.drain(..size);
    flow.message.push(frame)?;
    let Some(opcode) = flow.message.take() else {
      continue;
    };
    let payload = done_bytes(flow.message);
    flow.message.clear();
    match opcode {
      0x1 => {
        let text = String::from_utf8(payload).map_err(|_| ())?;
        flow.notes.push(Note::Text(text));
      }
      0x2 => flow.notes.push(Note::Binary(payload)),
      _ => return Err(()),
    }
    if !*flow.sent_close {
      flow.gate.take();
    }
  }
}

async fn control_flow<W>(
  frame: Frame,
  writer: &mut W,
  outgoing: &Option<Outgoing>,
  sent_close: &mut bool,
  notes: &NoteQueue,
  pongs: &mut VecDeque<Vec<u8>>,
) -> Result<bool, ()>
where
  W: AsyncWrite + Unpin,
{
  match frame.opcode {
    0x8 => {
      let (code, reason) = parse_close(&frame.payload)?;
      let incomplete = outgoing
        .as_ref()
        .is_some_and(|out| matches!(out.kind, OutKind::Data(_)));
      if outgoing.is_none() && !*sent_close {
        let echo = (code != 1005).then_some(code);
        let frame = close_frame(echo, &reason);
        let _ = writer.write_all(&frame).await;
        let _ = writer.flush().await;
        *sent_close = true;
      }
      notes.push(Note::Closed {
        code,
        reason,
        clean: !incomplete,
      });
      Ok(true)
    }
    0x9 => {
      let frame = encode_client_frame(0xA, &frame.payload, true, random_mask());
      pongs.push_back(frame);
      Ok(false)
    }
    0xA => Ok(false),
    _ => Err(()),
  }
}

fn fail(notes: &NoteQueue) {
  notes.push(Note::Error);
  notes.push(Note::Closed {
    code: 1006,
    reason: String::new(),
    clean: false,
  });
}

fn base64_encode(bytes: &[u8]) -> String {
  const TABLE: &[u8] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
  let mut out = String::new();
  let mut index = 0;
  while index + 3 <= bytes.len() {
    let value =
      u32::from_be_bytes([0, bytes[index], bytes[index + 1], bytes[index + 2]]);
    for shift in [18, 12, 6, 0] {
      out.push(TABLE[((value >> shift) & 0x3F) as usize] as char);
    }
    index += 3;
  }
  let rest = bytes.len() - index;
  if rest == 1 {
    let value = u32::from(bytes[index]) << 16;
    out.push(TABLE[((value >> 18) & 0x3F) as usize] as char);
    out.push(TABLE[((value >> 12) & 0x3F) as usize] as char);
    out.push('=');
    out.push('=');
  } else if rest == 2 {
    let value =
      (u32::from(bytes[index]) << 16) | (u32::from(bytes[index + 1]) << 8);
    out.push(TABLE[((value >> 18) & 0x3F) as usize] as char);
    out.push(TABLE[((value >> 12) & 0x3F) as usize] as char);
    out.push(TABLE[((value >> 6) & 0x3F) as usize] as char);
    out.push('=');
  }
  out
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn accept_key_matches_rfc6455() {
    let key = "dGhlIHNhbXBsZSBub25jZQ==";
    let mut data = key.as_bytes().to_vec();
    data.extend(GUID.as_bytes());
    let digest = Sha1::digest(&data);
    assert_eq!(base64_encode(&digest), "s3pPLMBiTxaQ9kYGzzhZRbK+xOo=");
  }

  #[test]
  fn local_echo_receives_text() {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let server = std::thread::spawn(move || {
      let (mut socket, _) = listener.accept().expect("accept");
      socket
        .set_read_timeout(Some(Duration::from_secs(2)))
        .expect("timeout");
      let mut buf = Vec::new();
      let mut tmp = [0u8; 1024];
      loop {
        let read = socket.read(&mut tmp).expect("read");
        buf.extend(&tmp[..read]);
        if buf.windows(4).any(|window| window == b"\r\n\r\n") {
          break;
        }
      }
      let text = String::from_utf8_lossy(&buf);
      let key = text
        .split("\r\n")
        .find_map(|line| line.strip_prefix("Sec-WebSocket-Key:"))
        .expect("key")
        .trim();
      let mut material = key.as_bytes().to_vec();
      material.extend(GUID.as_bytes());
      let accept = base64_encode(&Sha1::digest(&material));
      let response = format!(
        "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {accept}\r\n\r\n"
      );
      socket.write_all(response.as_bytes()).expect("write");
      let mut header = [0u8; 2];
      socket.read_exact(&mut header).expect("frame");
      let len = (header[1] & 0x7F) as usize;
      let mut mask = [0u8; 4];
      socket.read_exact(&mut mask).expect("mask");
      let mut payload = vec![0u8; len];
      socket.read_exact(&mut payload).expect("payload");
      for (index, byte) in payload.iter_mut().enumerate() {
        *byte ^= mask[index % 4];
      }
      let mut reply = vec![0x81, payload.len() as u8];
      reply.extend(payload);
      socket.write_all(&reply).expect("echo");
    });
    let url =
      url::Url::parse(&format!("ws://127.0.0.1:{port}/echo")).expect("url");
    let connection = connect(
      url,
      Vec::new(),
      "http://127.0.0.1".to_string(),
      Some(AddressSpace::Loopback),
      None,
    );
    let opened =
      wait_note(&connection, |note| matches!(note, Note::Open { .. }));
    assert!(opened, "handshake");
    connection.command(Command::Text("ping".to_string()));
    let echoed = wait_note(
      &connection,
      |note| matches!(note, Note::Text(text) if text == "ping"),
    );
    assert!(echoed, "echo");
    server.join().expect("server");
  }

  fn wait_note(
    connection: &Connection,
    mut ready: impl FnMut(&Note) -> bool,
  ) -> bool {
    let start = std::time::Instant::now();
    while start.elapsed() < Duration::from_secs(2) {
      while let Some(note) = connection.notes.poll() {
        if ready(&note) {
          return true;
        }
      }
      std::thread::sleep(Duration::from_millis(10));
    }
    false
  }

  #[test]
  fn server_text_frame_decodes() {
    let payload = b"hello";
    let mut frame = vec![0x81, payload.len() as u8];
    frame.extend(payload);
    let (decoded, size) = take_frame(&frame).expect("frame").expect("complete");
    assert_eq!(size, frame.len());
    assert!(decoded.fin);
    assert_eq!(decoded.opcode, 0x1);
    assert_eq!(decoded.payload, payload);
  }

  #[test]
  fn address_space_classifies_common_ranges() {
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
    assert_eq!(
      address_space(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1))),
      AddressSpace::Loopback
    );
    assert_eq!(
      address_space(IpAddr::V4(Ipv4Addr::new(10, 1, 2, 3))),
      AddressSpace::Local
    );
    assert_eq!(
      address_space(IpAddr::V4(Ipv4Addr::new(192, 168, 0, 1))),
      AddressSpace::Local
    );
    assert_eq!(
      address_space(IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1))),
      AddressSpace::Public
    );
    assert_eq!(
      address_space(IpAddr::V6(Ipv6Addr::LOCALHOST)),
      AddressSpace::Loopback
    );
  }

  #[test]
  fn public_target_rejects_loopback() {
    use std::io::Read;
    use std::net::TcpListener;
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let server = std::thread::spawn(move || {
      if let Ok((mut socket, _)) = listener.accept() {
        let mut buf = [0u8; 64];
        let _ = socket.read(&mut buf);
      }
    });
    let url =
      url::Url::parse(&format!("ws://127.0.0.1:{port}/echo")).expect("url");
    let connection = connect(
      url,
      Vec::new(),
      "null".to_string(),
      Some(AddressSpace::Public),
      None,
    );
    let mut saw_open = false;
    let mut saw_error = false;
    let start = std::time::Instant::now();
    while start.elapsed() < Duration::from_secs(2) && !saw_error {
      while let Some(note) = connection.notes.poll() {
        match note {
          Note::Open { .. } => saw_open = true,
          Note::Error => saw_error = true,
          _ => {}
        }
      }
      std::thread::sleep(Duration::from_millis(10));
    }
    assert!(saw_error, "mismatch fails");
    assert!(!saw_open, "mismatch does not open");
    server.join().expect("server");
  }

  #[test]
  fn pull_gate_delivers_one_message_at_a_time() {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let server = std::thread::spawn(move || {
      let (mut socket, _) = listener.accept().expect("accept");
      socket
        .set_read_timeout(Some(Duration::from_secs(2)))
        .expect("timeout");
      let mut buf = Vec::new();
      let mut tmp = [0u8; 1024];
      loop {
        let read = socket.read(&mut tmp).expect("read");
        buf.extend(&tmp[..read]);
        if buf.windows(4).any(|window| window == b"\r\n\r\n") {
          break;
        }
      }
      let text = String::from_utf8_lossy(&buf);
      let key = text
        .split("\r\n")
        .find_map(|line| line.strip_prefix("Sec-WebSocket-Key:"))
        .expect("key")
        .trim();
      let mut material = key.as_bytes().to_vec();
      material.extend(GUID.as_bytes());
      let accept = base64_encode(&Sha1::digest(&material));
      let mut response = format!(
        "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {accept}\r\n\r\n"
      )
      .into_bytes();
      response.extend(server_text("one"));
      response.extend(server_text("two"));
      socket.write_all(&response).expect("write");
    });
    let url =
      url::Url::parse(&format!("ws://127.0.0.1:{port}/echo")).expect("url");
    let gate = PullGate::new();
    let connection = connect(
      url,
      Vec::new(),
      "null".to_string(),
      None,
      Some(Arc::clone(&gate)),
    );
    assert!(
      wait_note(&connection, |note| matches!(note, Note::Open { .. })),
      "handshake"
    );
    std::thread::sleep(Duration::from_millis(50));
    assert!(connection.notes.poll().is_none(), "no read before pull");
    gate.grant();
    assert!(
      wait_note(
        &connection,
        |note| matches!(note, Note::Text(text) if text == "one")
      ),
      "first message"
    );
    std::thread::sleep(Duration::from_millis(50));
    assert!(connection.notes.poll().is_none(), "second waits for a pull");
    gate.grant();
    assert!(
      wait_note(
        &connection,
        |note| matches!(note, Note::Text(text) if text == "two")
      ),
      "second message"
    );
    server.join().expect("server");
  }

  fn server_text(payload: &str) -> Vec<u8> {
    let bytes = payload.as_bytes();
    let mut frame = vec![0x81, bytes.len() as u8];
    frame.extend(bytes);
    frame
  }
}
