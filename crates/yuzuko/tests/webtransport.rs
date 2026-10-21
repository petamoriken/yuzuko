//! Loopback WebTransport session. The server thread has its own runtime so
//! the V8 pump is not the task that accepts the QUIC connection.

use std::sync::mpsc;
use std::time::Duration;

use rustls::pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer};
use sha2::{Digest, Sha256};

const SCRIPT: &str = r#"
(async () => {
  try {
    const url = "__URL__";
    const hash = new Uint8Array([__HASH__]);
    function assert(condition, message) {
      if (!condition) {
        throw new Error(message);
      }
    }
    let syntax = false;
    try {
      new WebTransport("http://127.0.0.1/");
    } catch (error) {
      syntax = error.name === "SyntaxError";
    }
    assert(syntax, "http scheme");
    let bytesMode = false;
    try {
      new WebTransport(url, { datagramsReadableType: "bytes" });
    } catch (error) {
      bytesMode = error.name === "NotSupportedError";
    }
    assert(bytesMode, "bytes mode");
    let protocols = false;
    try {
      new WebTransport(url, { protocols: ["a"] });
    } catch (error) {
      protocols = error.name === "NotSupportedError";
    }
    assert(protocols, "protocols");
    let congestion = false;
    try {
      new WebTransport(url, { congestionControl: "nope" });
    } catch (error) {
      congestion = error instanceof TypeError;
    }
    assert(congestion, "congestion");
    const failed = new WebTransportError("boom", {
      source: "session",
      streamErrorCode: 9,
    });
    assert(failed.name === "WebTransportError", "error name");
    assert(failed instanceof DOMException, "instanceof");
    assert(failed.source === "session", "source");
    assert(failed.streamErrorCode === 9, "streamErrorCode");
    assert(failed.code === 0, "code");
    assert(failed.message === "boom", "message");
    const plain = new WebTransportError("x");
    assert(plain.source === "stream", "default source");
    assert(plain.streamErrorCode === null, "default streamErrorCode");

    const transport = new WebTransport(url, {
      serverCertificateHashes: [{ algorithm: "sha-256", value: hash }],
    });
    assert(transport.reliability === "pending", "pending");
    assert(transport.congestionControl === "default", "congestionControl");
    await transport.ready;
    assert(transport.reliability === "supports-unreliable", "unreliable");

    const datagram = (async () => {
      const writer = transport.datagrams.writable.getWriter();
      const reader = transport.datagrams.readable.getReader();
      const payload = new Uint8Array([1, 2, 3, 4]);
      const written = writer.write(payload);
      const chunk = await reader.read();
      assert(chunk.done === false, "datagram done");
      assert(chunk.value instanceof Uint8Array, "datagram type");
      assert(
        chunk.value.length === 4 &&
          chunk.value[0] === 1 &&
          chunk.value[3] === 4,
        "datagram bytes",
      );
      await written;
    })();
    const stream = (async () => {
      const pair = await transport.createBidirectionalStream();
      const writer = pair.writable.getWriter();
      const reader = pair.readable.getReader();
      const payload = new Uint8Array([9, 8, 7]);
      const written = writer.write(payload);
      await writer.close();
      await written;
      const chunks = [];
      while (true) {
        const chunk = await reader.read();
        if (chunk.done) {
          break;
        }
        chunks.push(...chunk.value);
      }
      assert(chunks.join(",") === "9,8,7", "stream bytes " + chunks);
    })();
    await Promise.all([datagram, stream]);
    transport.close({ closeCode: 7, reason: "bye" });
    const info = await transport.closed;
    assert(info.closeCode === 7, "closeCode " + info.closeCode);
    assert(info.reason === "bye", "reason " + info.reason);
    globalThis.done = "ok";
  } catch (error) {
    globalThis.done = error && error.stack ? String(error.stack) : String(error);
  }
})();
"#;

#[test]
fn session_echoes_datagram_and_stream() {
  let _ = rustls::crypto::ring::default_provider().install_default();
  let (port, hash) = start_server();
  let url = format!("https://127.0.0.1:{port}/");
  let listed = hash
    .iter()
    .map(|byte| byte.to_string())
    .collect::<Vec<_>>()
    .join(",");
  let script = SCRIPT.replace("__URL__", &url).replace("__HASH__", &listed);
  yuzuko_dom::register();
  yuzuko_winter::register();
  let run = yuzuko_core::run_classic_scripts(
    &[script],
    "done",
    Duration::from_secs(15),
    None,
    None,
  )
  .expect("runtime");
  if let Some(error) = &run.error {
    panic!("{error}");
  }
  let value = run.value.unwrap_or_else(|| "missing done".to_string());
  assert_eq!(value, "ok");
}

fn start_server() -> (u16, [u8; 32]) {
  let key = rcgen::KeyPair::generate().expect("key");
  let mut params = rcgen::CertificateParams::new(vec!["127.0.0.1".to_string()])
    .expect("params");
  params.is_ca = rcgen::IsCa::NoCa;
  params.key_usages = vec![rcgen::KeyUsagePurpose::DigitalSignature];
  params.extended_key_usages = vec![rcgen::ExtendedKeyUsagePurpose::ServerAuth];
  let cert = params.self_signed(&key).expect("cert");
  let der = cert.der().clone();
  let digest = Sha256::digest(der.as_ref());
  let mut hash = [0u8; 32];
  hash.copy_from_slice(&digest);
  let chain = vec![der];
  let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key.serialize_der()));
  let (port_tx, port_rx) = mpsc::channel();
  std::thread::spawn(move || {
    let runtime = tokio::runtime::Builder::new_current_thread()
      .enable_all()
      .build()
      .expect("runtime");
    runtime.block_on(async move {
      let _ = tokio::time::timeout(
        Duration::from_secs(20),
        serve(chain, key, port_tx),
      )
      .await;
    });
  });
  let port = port_rx
    .recv_timeout(Duration::from_secs(5))
    .expect("server port");
  (port, hash)
}

async fn serve(
  chain: Vec<rustls::pki_types::CertificateDer<'static>>,
  key: PrivateKeyDer<'static>,
  port_tx: mpsc::Sender<u16>,
) {
  let mut server = web_transport_quinn::ServerBuilder::new()
    .with_addr(std::net::SocketAddr::from((
      std::net::Ipv4Addr::LOCALHOST,
      0,
    )))
    .with_certificate(chain, key)
    .expect("server");
  port_tx
    .send(server.local_addr().expect("local addr").port())
    .expect("port");
  let Some(request) = server.accept().await else {
    return;
  };
  let session = match request.ok().await {
    Ok(session) => session,
    Err(_) => return,
  };
  let datagrams = tokio::spawn({
    let session = session.clone();
    async move {
      loop {
        let Ok(bytes) = session.read_datagram().await else {
          return;
        };
        if session.send_datagram(bytes).is_err() {
          return;
        }
      }
    }
  });
  let streams = tokio::spawn({
    let session = session.clone();
    async move {
      loop {
        let Ok((mut send, mut recv)) = session.accept_bi().await else {
          return;
        };
        tokio::spawn(async move {
          let mut collected = Vec::new();
          let mut buf = [0u8; 2048];
          loop {
            match recv.read(&mut buf).await {
              Ok(Some(read)) if read > 0 => {
                collected.extend_from_slice(&buf[..read]);
              }
              Ok(Some(_)) => {}
              Ok(None) => break,
              Err(_) => return,
            }
          }
          if send.write_all(&collected).await.is_err() {
            return;
          }
          let _ = send.finish();
        });
      }
    }
  });
  let _ = session.closed().await;
  datagrams.abort();
  streams.abort();
}
