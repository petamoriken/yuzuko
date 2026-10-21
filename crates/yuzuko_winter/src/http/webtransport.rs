//! A WebTransport session on its own QUIC connection.
//!
//! The I/O thread never touches V8. Notes cross to the realm that created
//! the session. Each session has its own connection; it is not pooled with
//! fetch's HTTP/3 connections. `serverCertificateHashes` replaces the webpki
//! roots for that handshake. An empty list uses the webpki roots.

use std::collections::{HashMap, VecDeque};
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};

use rustls::client::danger::{
  HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier,
};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{DigitallySignedStruct, SignatureScheme};
use sha2::{Digest, Sha256};

use super::websocket::PullGate;

static NEXT_STREAM: AtomicU64 = AtomicU64::new(1);

pub fn next_stream_id() -> u64 {
  NEXT_STREAM.fetch_add(1, Ordering::Relaxed)
}

#[derive(Clone, Copy, Debug)]
pub enum Congestion {
  Default,
  Throughput,
  LowLatency,
}

pub struct Options {
  pub congestion: Congestion,
  pub hashes: Vec<[u8; 32]>,
  /// The caller supplied at least one hash. Trust is then the hash list
  /// alone, including when every hash was skipped.
  pub hashes_only: bool,
  pub require_unreliable: bool,
}

pub enum Command {
  SendDatagram(Vec<u8>),
  PullDatagram,
  OpenBi { id: u64 },
  OpenUni { id: u64 },
  Write { id: u64, bytes: Vec<u8> },
  Finish { id: u64 },
  Reset { id: u64, code: u32 },
  Stop { id: u64, code: u32 },
  Pull { id: u64 },
  Close { code: u32, reason: String },
  Abort,
}

#[derive(Clone, Debug)]
pub enum Note {
  Ready {
    max_datagram: usize,
  },
  Closed {
    code: u32,
    reason: String,
    clean: bool,
    message: String,
  },
  Datagram(Vec<u8>),
  DatagramSent,
  DatagramFailed(String),
  BiOpened {
    id: u64,
  },
  UniOpened {
    id: u64,
  },
  IncomingBi {
    id: u64,
  },
  IncomingUni {
    id: u64,
  },
  Data {
    id: u64,
    bytes: Vec<u8>,
  },
  End {
    id: u64,
  },
  StreamError {
    id: u64,
    code: Option<u32>,
    message: String,
  },
  Wrote {
    id: u64,
  },
  WriteFailed {
    id: u64,
    code: Option<u32>,
    message: String,
  },
  Finished {
    id: u64,
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
    self
      .notes
      .lock()
      .expect("webtransport notes")
      .push_back(note);
    self.ready.notify_all();
    let wake = self.wake.lock().expect("webtransport wake").clone();
    if let Some(wake) = wake {
      wake();
    }
  }

  pub fn poll(&self) -> Option<Note> {
    self.notes.lock().expect("webtransport notes").pop_front()
  }

  pub fn listen(&self, wake: Wake) {
    *self.wake.lock().expect("webtransport wake") = Some(Arc::clone(&wake));
    if !self.notes.lock().expect("webtransport notes").is_empty() {
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
}

pub fn connect(url: url::Url, options: Options) -> Connection {
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
      Err(error) => {
        fail(&queued, error.to_string());
        return;
      }
    };
    runtime.block_on(session(url, options, inbox, queued));
  });
  Connection { notes, commands }
}

fn fail(notes: &NoteQueue, message: String) {
  notes.push(Note::Closed {
    code: 0,
    reason: String::new(),
    clean: false,
    message,
  });
}

async fn session(
  url: url::Url,
  options: Options,
  mut inbox: tokio::sync::mpsc::UnboundedReceiver<Command>,
  notes: NoteQueue,
) {
  let connecting = handshake(&url, &options);
  tokio::pin!(connecting);
  let mut early = Vec::new();
  let established = loop {
    tokio::select! {
      biased;
      result = &mut connecting => break result,
      command = inbox.recv() => {
        match command {
          None | Some(Command::Abort) | Some(Command::Close { .. }) => {
            fail(&notes, "The connection failed".to_string());
            return;
          }
          Some(command) => early.push(command),
        }
      }
    }
  };
  let (endpoint, session) = match established {
    Ok(established) => established,
    Err(message) => {
      fail(&notes, message);
      return;
    }
  };
  let max_datagram = session.max_datagram_size();
  if options.require_unreliable && max_datagram == 0 {
    fail(&notes, "Unreliable transport is unavailable".to_string());
    drop(session);
    drop(endpoint);
    return;
  }
  notes.push(Note::Ready { max_datagram });
  drive(session, inbox, notes, early).await;
  drop(endpoint);
}

async fn handshake(
  url: &url::Url,
  options: &Options,
) -> Result<(quinn::Endpoint, web_transport_quinn::Session), String> {
  super::install_tls();
  let host = url
    .host_str()
    .ok_or_else(|| "The URL has no host".to_string())?;
  let port = url.port_or_known_default().unwrap_or(443);
  let dial = format!("{host}:{port}");
  let mut looked = tokio::net::lookup_host(dial)
    .await
    .map_err(|error| error.to_string())?;
  let addr = looked
    .next()
    .ok_or_else(|| "The address lookup failed".to_string())?;
  let bind = match addr {
    SocketAddr::V4(_) => SocketAddr::from((Ipv4Addr::UNSPECIFIED, 0)),
    SocketAddr::V6(_) => SocketAddr::from((Ipv6Addr::UNSPECIFIED, 0)),
  };
  let endpoint =
    quinn::Endpoint::client(bind).map_err(|error| error.to_string())?;
  let config = client_config(options)?;
  let sni = host.trim_matches(|ch| ch == '[' || ch == ']');
  let connecting = endpoint
    .connect_with(config, addr, sni)
    .map_err(|error| error.to_string())?;
  let connection = connecting.await.map_err(|error| error.to_string())?;
  let session = web_transport_quinn::Session::connect(connection, url.clone())
    .await
    .map_err(|error| error.to_string())?;
  Ok((endpoint, session))
}

fn client_config(options: &Options) -> Result<quinn::ClientConfig, String> {
  let provider = Arc::new(rustls::crypto::ring::default_provider());
  let builder = rustls::ClientConfig::builder_with_provider(provider.clone())
    .with_protocol_versions(&[&rustls::version::TLS13])
    .map_err(|error| error.to_string())?;
  let mut crypto = if options.hashes_only {
    // An empty webpki store cannot be built. Signature checks use the
    // certificate key directly. The hash list is the only trust decision.
    let verifier = Arc::new(HashVerifier {
      algorithms: provider.signature_verification_algorithms,
      hashes: options.hashes.clone(),
    });
    builder
      .dangerous()
      .with_custom_certificate_verifier(verifier)
      .with_no_client_auth()
  } else {
    builder
      .with_root_certificates(rustls::RootCertStore {
        roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
      })
      .with_no_client_auth()
  };
  crypto.alpn_protocols = vec![web_transport_quinn::ALPN.as_bytes().to_vec()];
  let crypto = quinn::crypto::rustls::QuicClientConfig::try_from(crypto)
    .map_err(|error| error.to_string())?;
  let mut config = quinn::ClientConfig::new(Arc::new(crypto));
  let mut transport = quinn::TransportConfig::default();
  match options.congestion {
    Congestion::LowLatency => {
      transport.congestion_controller_factory(Arc::new(
        quinn::congestion::BbrConfig::default(),
      ));
    }
    Congestion::Throughput => {
      transport.congestion_controller_factory(Arc::new(
        quinn::congestion::CubicConfig::default(),
      ));
    }
    Congestion::Default => {}
  }
  config.transport_config(Arc::new(transport));
  Ok(config)
}

struct HashVerifier {
  algorithms: rustls::crypto::WebPkiSupportedAlgorithms,
  hashes: Vec<[u8; 32]>,
}

impl std::fmt::Debug for HashVerifier {
  fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    formatter
      .debug_struct("HashVerifier")
      .field("hashes", &self.hashes.len())
      .finish()
  }
}

impl ServerCertVerifier for HashVerifier {
  fn verify_server_cert(
    &self,
    end_entity: &CertificateDer<'_>,
    _intermediates: &[CertificateDer<'_>],
    _server_name: &ServerName<'_>,
    _ocsp_response: &[u8],
    _now: UnixTime,
  ) -> Result<ServerCertVerified, rustls::Error> {
    let digest = Sha256::digest(end_entity.as_ref());
    if self
      .hashes
      .iter()
      .any(|hash| hash.as_slice() == digest.as_slice())
    {
      return Ok(ServerCertVerified::assertion());
    }
    Err(rustls::Error::General(
      "The certificate hash does not match".into(),
    ))
  }

  fn verify_tls12_signature(
    &self,
    message: &[u8],
    cert: &CertificateDer<'_>,
    dss: &DigitallySignedStruct,
  ) -> Result<HandshakeSignatureValid, rustls::Error> {
    rustls::crypto::verify_tls12_signature(message, cert, dss, &self.algorithms)
  }

  fn verify_tls13_signature(
    &self,
    message: &[u8],
    cert: &CertificateDer<'_>,
    dss: &DigitallySignedStruct,
  ) -> Result<HandshakeSignatureValid, rustls::Error> {
    rustls::crypto::verify_tls13_signature(message, cert, dss, &self.algorithms)
  }

  fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
    self.algorithms.supported_schemes()
  }
}

enum Incoming {
  Datagram(bytes::Bytes),
  Bi(
    web_transport_quinn::SendStream,
    web_transport_quinn::RecvStream,
  ),
  Uni(web_transport_quinn::RecvStream),
}

struct Half {
  send: Option<web_transport_quinn::SendStream>,
  pull: Option<Arc<PullGate>>,
  stop: Option<tokio::sync::mpsc::UnboundedSender<u32>>,
}

async fn drive(
  session: web_transport_quinn::Session,
  mut inbox: tokio::sync::mpsc::UnboundedReceiver<Command>,
  notes: NoteQueue,
  early: Vec<Command>,
) {
  let (incoming_tx, mut incoming_rx) = tokio::sync::mpsc::unbounded_channel();
  let datagram_pull = PullGate::new();
  spawn_datagrams(
    session.clone(),
    Arc::clone(&datagram_pull),
    incoming_tx.clone(),
  );
  spawn_accept(session.clone(), incoming_tx);
  let mut halves: HashMap<u64, Half> = HashMap::new();
  let mut local_close: Option<(u32, String)> = None;
  let mut readers = true;
  for command in early {
    if handle(
      &session,
      &notes,
      &mut halves,
      &datagram_pull,
      &mut local_close,
      command,
    )
    .await
    {
      return;
    }
  }
  loop {
    tokio::select! {
      biased;
      error = session.closed() => {
        if let Some((code, reason)) = local_close {
          notes.push(Note::Closed {
            code,
            reason,
            clean: true,
            message: String::new(),
          });
        } else if let web_transport_quinn::SessionError::WebTransportError(
          web_transport_quinn::WebTransportError::Closed(code, reason),
        ) = error
        {
          notes.push(Note::Closed {
            code,
            reason,
            clean: true,
            message: String::new(),
          });
        } else {
          fail(&notes, error.to_string());
        }
        return;
      }
      command = inbox.recv() => {
        let Some(command) = command else {
          session.close(0, b"");
          let _ = session.closed().await;
          fail(&notes, "The connection failed".to_string());
          return;
        };
        if handle(
          &session,
          &notes,
          &mut halves,
          &datagram_pull,
          &mut local_close,
          command,
        )
        .await
        {
          return;
        }
      }
      incoming = incoming_rx.recv(), if readers => {
        let Some(incoming) = incoming else {
          readers = false;
          continue;
        };
        match incoming {
          Incoming::Datagram(bytes) => {
            notes.push(Note::Datagram(bytes.to_vec()));
          }
          Incoming::Bi(send, recv) => {
            let id = next_stream_id();
            arm_reader(id, recv, &session, &notes, &mut halves);
            if let Some(half) = halves.get_mut(&id) {
              half.send = Some(send);
            }
            notes.push(Note::IncomingBi { id });
          }
          Incoming::Uni(recv) => {
            let id = next_stream_id();
            arm_reader(id, recv, &session, &notes, &mut halves);
            notes.push(Note::IncomingUni { id });
          }
        }
      }
    }
  }
}

fn spawn_datagrams(
  session: web_transport_quinn::Session,
  pull: Arc<PullGate>,
  incoming: tokio::sync::mpsc::UnboundedSender<Incoming>,
) {
  tokio::spawn(async move {
    loop {
      let woke = tokio::select! {
        biased;
        _ = session.closed() => false,
        _ = pull.wait() => true,
      };
      if !woke {
        return;
      }
      pull.take();
      match session.read_datagram().await {
        Ok(bytes) => {
          if incoming.send(Incoming::Datagram(bytes)).is_err() {
            return;
          }
        }
        Err(_) => return,
      }
    }
  });
}

fn spawn_accept(
  session: web_transport_quinn::Session,
  incoming: tokio::sync::mpsc::UnboundedSender<Incoming>,
) {
  let uni_session = session.clone();
  let uni_incoming = incoming.clone();
  tokio::spawn(async move {
    loop {
      match uni_session.accept_uni().await {
        Ok(recv) => {
          if uni_incoming.send(Incoming::Uni(recv)).is_err() {
            return;
          }
        }
        Err(_) => return,
      }
    }
  });
  tokio::spawn(async move {
    loop {
      match session.accept_bi().await {
        Ok((send, recv)) => {
          if incoming.send(Incoming::Bi(send, recv)).is_err() {
            return;
          }
        }
        Err(_) => return,
      }
    }
  });
}

fn arm_reader(
  id: u64,
  recv: web_transport_quinn::RecvStream,
  session: &web_transport_quinn::Session,
  notes: &NoteQueue,
  halves: &mut HashMap<u64, Half>,
) {
  let pull = PullGate::new();
  let (stop, stop_rx) = tokio::sync::mpsc::unbounded_channel();
  halves.insert(
    id,
    Half {
      send: None,
      pull: Some(Arc::clone(&pull)),
      stop: Some(stop),
    },
  );
  let session = session.clone();
  let notes = notes.clone();
  tokio::spawn(async move {
    read_loop(id, recv, pull, stop_rx, session, notes).await;
  });
}

async fn read_loop(
  id: u64,
  mut recv: web_transport_quinn::RecvStream,
  pull: Arc<PullGate>,
  mut stop: tokio::sync::mpsc::UnboundedReceiver<u32>,
  session: web_transport_quinn::Session,
  notes: NoteQueue,
) {
  let mut buf = vec![0u8; 16 * 1024];
  loop {
    let woke = tokio::select! {
      biased;
      code = stop.recv() => {
        let _ = recv.stop(code.unwrap_or(0));
        return;
      }
      _ = session.closed() => return,
      _ = pull.wait() => true,
    };
    if !woke {
      return;
    }
    pull.take();
    let read = tokio::select! {
      biased;
      code = stop.recv() => Err(code.unwrap_or(0)),
      read = recv.read(&mut buf) => Ok(read),
    };
    let read = match read {
      Err(code) => {
        let _ = recv.stop(code);
        return;
      }
      Ok(read) => read,
    };
    match read {
      Ok(Some(size)) => {
        notes.push(Note::Data {
          id,
          bytes: buf[..size].to_vec(),
        });
      }
      Ok(None) => {
        notes.push(Note::End { id });
        return;
      }
      Err(web_transport_quinn::ReadError::Reset(code)) => {
        notes.push(Note::StreamError {
          id,
          code: Some(code),
          message: "The stream was reset".to_string(),
        });
        return;
      }
      Err(error) => {
        notes.push(Note::StreamError {
          id,
          code: None,
          message: error.to_string(),
        });
        return;
      }
    }
  }
}

/// `true` when the session has finished and the driver should return.
async fn handle(
  session: &web_transport_quinn::Session,
  notes: &NoteQueue,
  halves: &mut HashMap<u64, Half>,
  datagram_pull: &Arc<PullGate>,
  local_close: &mut Option<(u32, String)>,
  command: Command,
) -> bool {
  match command {
    Command::SendDatagram(bytes) => {
      match session.send_datagram(bytes::Bytes::from(bytes)) {
        Ok(()) => notes.push(Note::DatagramSent),
        Err(error) => notes.push(Note::DatagramFailed(error.to_string())),
      }
    }
    Command::PullDatagram => datagram_pull.grant(),
    Command::OpenBi { id } => match session.open_bi().await {
      Ok((send, recv)) => {
        arm_reader(id, recv, session, notes, halves);
        if let Some(half) = halves.get_mut(&id) {
          half.send = Some(send);
        }
        notes.push(Note::BiOpened { id });
      }
      Err(error) => notes.push(Note::StreamError {
        id,
        code: None,
        message: error.to_string(),
      }),
    },
    Command::OpenUni { id } => match session.open_uni().await {
      Ok(send) => {
        halves.insert(
          id,
          Half {
            send: Some(send),
            pull: None,
            stop: None,
          },
        );
        notes.push(Note::UniOpened { id });
      }
      Err(error) => notes.push(Note::StreamError {
        id,
        code: None,
        message: error.to_string(),
      }),
    },
    Command::Write { id, bytes } => {
      let Some(send) = halves.get_mut(&id).and_then(|half| half.send.as_mut())
      else {
        notes.push(Note::WriteFailed {
          id,
          code: None,
          message: "The stream is closed".to_string(),
        });
        return false;
      };
      match send.write_all(&bytes).await {
        Ok(()) => notes.push(Note::Wrote { id }),
        Err(web_transport_quinn::WriteError::Stopped(code)) => {
          notes.push(Note::WriteFailed {
            id,
            code: Some(code),
            message: "The stream was stopped".to_string(),
          });
        }
        Err(error) => notes.push(Note::WriteFailed {
          id,
          code: None,
          message: error.to_string(),
        }),
      }
    }
    Command::Finish { id } => {
      if let Some(send) =
        halves.get_mut(&id).and_then(|half| half.send.as_mut())
      {
        let _ = send.finish();
      }
      notes.push(Note::Finished { id });
    }
    Command::Reset { id, code } => {
      if let Some(send) =
        halves.get_mut(&id).and_then(|half| half.send.as_mut())
      {
        let _ = send.reset(code);
      }
      notes.push(Note::Finished { id });
    }
    Command::Stop { id, code } => {
      if let Some(stop) = halves.get(&id).and_then(|half| half.stop.as_ref()) {
        let _ = stop.send(code);
      }
    }
    Command::Pull { id } => {
      if let Some(pull) = halves.get(&id).and_then(|half| half.pull.as_ref()) {
        pull.grant();
      }
    }
    Command::Close { code, reason } => {
      if local_close.is_none() {
        // The capsule write and the QUIC close run on a spawned task.
        // Waiting here drives that task and records the code we sent.
        session.close(code, reason.as_bytes());
        *local_close = Some((code, reason));
      }
      let _ = session.closed().await;
      if let Some((code, reason)) = local_close.clone() {
        notes.push(Note::Closed {
          code,
          reason,
          clean: true,
          message: String::new(),
        });
      }
      return true;
    }
    Command::Abort => {
      session.close(0, b"");
      let _ = session.closed().await;
      fail(notes, "The connection failed".to_string());
      return true;
    }
  }
  false
}
