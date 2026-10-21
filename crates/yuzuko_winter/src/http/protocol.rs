//! Loopback checks for cleartext HTTP/1.1, TLS HTTP/2, and Alt-Svc HTTP/3.
//!
//! Each server runs on its own thread. `perform` blocks the caller, so the
//! server cannot share that runtime. A stuck accept times out so the thread
//! does not keep the process alive.

use std::net::Ipv4Addr;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, mpsc};
use std::time::Duration;

use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};

use super::{BodyNote, HttpResponse, perform, perform_with_roots};

struct Identity {
  chain: Vec<CertificateDer<'static>>,
  key: PrivateKeyDer<'static>,
}

struct Material {
  chain: Vec<CertificateDer<'static>>,
  key: Vec<u8>,
  roots: rustls::RootCertStore,
}

impl Material {
  fn identity(&self) -> Identity {
    Identity {
      chain: self.chain.clone(),
      key: PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(self.key.clone())),
    }
  }
}

/// Drops the process-wide Alt-Svc entry even when the test panics.
struct ClearAltSvc(String);

impl Drop for ClearAltSvc {
  fn drop(&mut self) {
    super::alt_svc::invalidate(&self.0);
  }
}

fn install_ring() {
  let _ = rustls::crypto::ring::default_provider().install_default();
}

fn material() -> Material {
  install_ring();
  let ca_key = rcgen::KeyPair::generate().expect("ca key");
  let mut ca_params =
    rcgen::CertificateParams::new(Vec::<String>::new()).expect("ca params");
  ca_params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
  ca_params.key_usages = vec![
    rcgen::KeyUsagePurpose::KeyCertSign,
    rcgen::KeyUsagePurpose::CrlSign,
  ];
  ca_params
    .distinguished_name
    .push(rcgen::DnType::CommonName, "Yuzuko Test CA");
  let ca = ca_params.self_signed(&ca_key).expect("ca");

  let leaf_key = rcgen::KeyPair::generate().expect("leaf key");
  let mut leaf_params =
    rcgen::CertificateParams::new(vec!["127.0.0.1".to_string()])
      .expect("leaf params");
  leaf_params.is_ca = rcgen::IsCa::NoCa;
  leaf_params.key_usages = vec![rcgen::KeyUsagePurpose::DigitalSignature];
  leaf_params.extended_key_usages =
    vec![rcgen::ExtendedKeyUsagePurpose::ServerAuth];
  leaf_params
    .distinguished_name
    .push(rcgen::DnType::CommonName, "127.0.0.1");
  let leaf = leaf_params
    .signed_by(&leaf_key, &ca, &ca_key)
    .expect("leaf");

  let mut roots = rustls::RootCertStore::empty();
  roots.add(ca.der().clone()).expect("trust anchor");
  Material {
    chain: vec![leaf.der().clone(), ca.der().clone()],
    key: leaf_key.serialize_der(),
    roots,
  }
}

fn spawn_server<F, Fut>(server: F) -> u16
where
  F: FnOnce(mpsc::Sender<u16>) -> Fut + Send + 'static,
  Fut: std::future::Future<Output = ()> + Send + 'static,
{
  let (port_tx, port_rx) = mpsc::channel();
  std::thread::spawn(move || {
    let runtime = tokio::runtime::Builder::new_current_thread()
      .enable_all()
      .build()
      .expect("runtime");
    runtime.block_on(async move {
      let _ =
        tokio::time::timeout(Duration::from_secs(20), server(port_tx)).await;
    });
  });
  port_rx
    .recv_timeout(Duration::from_secs(5))
    .expect("server port")
}

fn fetch(url: &str, roots: Option<rustls::RootCertStore>) -> HttpResponse {
  let cancel = Arc::new(AtomicBool::new(false));
  let result = if let Some(roots) = roots {
    perform_with_roots(
      "GET".to_string(),
      url.to_string(),
      Vec::new(),
      Vec::new(),
      "follow".to_string(),
      cancel,
      roots,
    )
  } else {
    perform(
      "GET".to_string(),
      url.to_string(),
      Vec::new(),
      Vec::new(),
      "follow".to_string(),
      cancel,
    )
  };
  match result {
    Ok(response) => response,
    Err(super::HttpFailure::Abort) => panic!("the fetch was aborted"),
    Err(
      super::HttpFailure::Type(message) | super::HttpFailure::Network(message),
    ) => {
      panic!("{message}");
    }
  }
}

fn read_body(response: &HttpResponse) -> Vec<u8> {
  let mut bytes = Vec::new();
  loop {
    match response.body.wait_note(Duration::from_secs(5)) {
      Some(BodyNote::Chunk(chunk)) => bytes.extend(chunk),
      Some(BodyNote::End) => return bytes,
      Some(BodyNote::Failed(message)) => panic!("{message}"),
      None => panic!("the body timed out"),
    }
  }
}

fn tls_server(
  identity: Identity,
  alpn: Vec<Vec<u8>>,
  tls12: bool,
) -> rustls::ServerConfig {
  let provider = Arc::new(rustls::crypto::ring::default_provider());
  let versions: &[&rustls::SupportedProtocolVersion] = if tls12 {
    &[&rustls::version::TLS13, &rustls::version::TLS12]
  } else {
    &[&rustls::version::TLS13]
  };
  let mut config = rustls::ServerConfig::builder_with_provider(provider)
    .with_protocol_versions(versions)
    .expect("protocol versions")
    .with_no_client_auth()
    .with_single_cert(identity.chain, identity.key)
    .expect("server certificate");
  config.alpn_protocols = alpn;
  config
}

async fn serve_http1(port_tx: mpsc::Sender<u16>) {
  let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
    .await
    .expect("bind");
  port_tx
    .send(listener.local_addr().expect("local addr").port())
    .expect("port");
  let accepted =
    tokio::time::timeout(Duration::from_secs(15), listener.accept()).await;
  let Ok(Ok((mut stream, _))) = accepted else {
    return;
  };
  use tokio::io::{AsyncReadExt, AsyncWriteExt};
  let mut received = Vec::new();
  let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
  loop {
    if received.windows(4).any(|window| window == b"\r\n\r\n") {
      break;
    }
    let remaining =
      deadline.saturating_duration_since(tokio::time::Instant::now());
    if remaining.is_zero() {
      return;
    }
    let mut buf = [0u8; 1024];
    match tokio::time::timeout(remaining, stream.read(&mut buf)).await {
      Ok(Ok(0)) | Ok(Err(_)) | Err(_) => return,
      Ok(Ok(read)) => received.extend_from_slice(&buf[..read]),
    }
  }
  let body = b"hello";
  let head = format!(
    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
    body.len()
  );
  let _ = stream.write_all(head.as_bytes()).await;
  let _ = stream.write_all(body).await;
  let _ = stream.shutdown().await;
}

async fn serve_h2(
  identity: Identity,
  body: &'static [u8],
  alt_svc: Option<String>,
  port_tx: mpsc::Sender<u16>,
) {
  let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
    .await
    .expect("bind");
  port_tx
    .send(listener.local_addr().expect("local addr").port())
    .expect("port");
  let config = tls_server(identity, vec![b"h2".to_vec()], true);
  let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(config));
  let accepted =
    tokio::time::timeout(Duration::from_secs(15), listener.accept()).await;
  let Ok(Ok((stream, _))) = accepted else {
    return;
  };
  let Ok(tls) = acceptor.accept(stream).await else {
    return;
  };
  let io = hyper_util::rt::TokioIo::new(tls);
  let service = hyper::service::service_fn(
    move |_request: hyper::Request<hyper::body::Incoming>| {
      let alt_svc = alt_svc.clone();
      async move {
        let mut builder = hyper::Response::builder().status(200);
        if let Some(value) = alt_svc {
          builder = builder.header(hyper::header::ALT_SVC, value);
        }
        Ok::<_, std::convert::Infallible>(
          builder
            .body(http_body_util::Full::new(bytes::Bytes::from_static(body)))
            .expect("response"),
        )
      }
    },
  );
  let _ = hyper::server::conn::http2::Builder::new(
    hyper_util::rt::TokioExecutor::new(),
  )
  .serve_connection(io, service)
  .await;
}

async fn serve_h3(identity: Identity, port_tx: mpsc::Sender<u16>) {
  let config = tls_server(identity, vec![b"h3".to_vec()], false);
  let quic = quinn::crypto::rustls::QuicServerConfig::try_from(config)
    .expect("quic server config");
  let server = quinn::ServerConfig::with_crypto(Arc::new(quic));
  let endpoint = quinn::Endpoint::server(
    server,
    std::net::SocketAddr::from((Ipv4Addr::LOCALHOST, 0)),
  )
  .expect("quic endpoint");
  port_tx
    .send(endpoint.local_addr().expect("local addr").port())
    .expect("port");
  let incoming =
    tokio::time::timeout(Duration::from_secs(15), endpoint.accept()).await;
  let Ok(Some(incoming)) = incoming else {
    return;
  };
  let Ok(connection) = incoming.await else {
    return;
  };
  let mut connection = match h3::server::builder()
    .build::<_, bytes::Bytes>(h3_quinn::Connection::new(connection))
    .await
  {
    Ok(connection) => connection,
    Err(_) => return,
  };
  // `accept` drives the HTTP/3 connection while the spawned handler writes.
  loop {
    let accepted =
      tokio::time::timeout(Duration::from_secs(15), connection.accept()).await;
    let Ok(Ok(Some(resolver))) = accepted else {
      break;
    };
    tokio::spawn(async move {
      let Ok((_request, mut stream)) = resolver.resolve_request().await else {
        return;
      };
      let Ok(response) = hyper::Response::builder().status(200).body(()) else {
        return;
      };
      if stream.send_response(response).await.is_err() {
        return;
      }
      if stream
        .send_data(bytes::Bytes::from_static(b"from-h3"))
        .await
        .is_err()
      {
        return;
      }
      let _ = stream.finish().await;
    });
  }
}

#[test]
fn cleartext_http1_answers_content_length() {
  let port = spawn_server(serve_http1);
  let url = format!("http://127.0.0.1:{port}/");
  let response = fetch(&url, None);
  assert_eq!(response.status, 200);
  assert_eq!(read_body(&response), b"hello");
  let _ = response.body.stop();
}

#[test]
fn https_http2_only_server_answers() {
  let prepared = material();
  let roots = prepared.roots.clone();
  let port = spawn_server(move |port_tx| {
    serve_h2(prepared.identity(), b"from-h2", None, port_tx)
  });
  let url = format!("https://127.0.0.1:{port}/");
  let response = fetch(&url, Some(roots));
  assert_eq!(response.status, 200);
  assert_eq!(read_body(&response), b"from-h2");
  response.body.stop();
}

#[test]
fn https_http3_follows_alt_svc() {
  let prepared = material();
  let roots = prepared.roots.clone();
  let h3_material = Material {
    chain: prepared.chain.clone(),
    key: prepared.key.clone(),
    roots: prepared.roots.clone(),
  };
  let udp =
    spawn_server(move |port_tx| serve_h3(h3_material.identity(), port_tx));
  let tcp = spawn_server(move |port_tx| {
    serve_h2(
      prepared.identity(),
      b"from-h2",
      Some(format!("h3=\":{udp}\"")),
      port_tx,
    )
  });
  let url = format!("https://127.0.0.1:{tcp}/");
  let _clear = ClearAltSvc(url.clone());
  let first = fetch(&url, Some(roots.clone()));
  assert_eq!(first.status, 200);
  assert_eq!(read_body(&first), b"from-h2");
  first.body.stop();
  let second = fetch(&url, Some(roots));
  assert_eq!(second.status, 200);
  assert_eq!(read_body(&second), b"from-h3");
  second.body.stop();
}
