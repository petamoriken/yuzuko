//! DNS hostnames, `resolveDns`, TLS, and socket error classes.
//!
//! `localhost.` keeps the trailing root dot, so the lookup is real DNS.
//! The bare name `localhost` is hard-wired to `127.0.0.1` and is not used
//! here as proof of resolution.

#![cfg(all(feature = "tcp", feature = "udp"))]

use std::collections::BTreeSet;
use std::fs;
use std::io::{ErrorKind, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream, ToSocketAddrs, UdpSocket};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::{Arc, Once};
use std::thread;
use std::time::{Duration, Instant};

use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use rustls::{ServerConfig, ServerConnection};

fn bin() -> PathBuf {
  PathBuf::from(env!("CARGO_BIN_EXE_yuzuko"))
}

fn scratch(name: &str) -> PathBuf {
  let dir = std::env::temp_dir().join(format!(
    "yuzuko-net-name-{}-{}-{name}",
    std::process::id(),
    std::time::SystemTime::now()
      .duration_since(std::time::UNIX_EPOCH)
      .unwrap_or_default()
      .as_nanos()
  ));
  let _ = fs::remove_dir_all(&dir);
  fs::create_dir_all(&dir).unwrap();
  fs::canonicalize(&dir).unwrap_or(dir)
}

fn text(bytes: &[u8]) -> String {
  String::from_utf8_lossy(bytes).into_owned()
}

fn run(dir: &Path, args: &[&str]) -> Output {
  let mut command = Command::new(bin());
  command
    .current_dir(dir)
    .args(args)
    .env("YUZUKO_HEADLESS", "1")
    .stdin(Stdio::null())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped());
  for (key, _) in std::env::vars_os() {
    let Some(name) = key.to_str() else {
      continue;
    };
    let bytes = name.as_bytes();
    let loader = (bytes.len() >= 3 && bytes[..3].eq_ignore_ascii_case(b"LD_"))
      || (bytes.len() >= 5 && bytes[..5].eq_ignore_ascii_case(b"DYLD_"));
    if loader {
      command.env_remove(&key);
    }
  }
  let mut child = command.spawn().expect("spawn yuzuko");
  let start = Instant::now();
  loop {
    if child.try_wait().unwrap().is_some() {
      return child.wait_with_output().unwrap();
    }
    if start.elapsed() > Duration::from_secs(30) {
      let _ = child.kill();
      let output = child.wait_with_output().unwrap();
      panic!(
        "timed out\nstdout:\n{}\nstderr:\n{}",
        text(&output.stdout),
        text(&output.stderr)
      );
    }
    std::thread::sleep(Duration::from_millis(50));
  }
}

fn stdout_of(output: &Output) -> String {
  let stdout = text(&output.stdout);
  let stderr = text(&output.stderr);
  assert_eq!(output.status.code(), Some(0), "{stderr}\n{stdout}");
  stdout
}

fn bind_tcp() -> TcpListener {
  let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
  listener.set_nonblocking(true).expect("nonblocking");
  listener
}

/// `SO_LINGER` of zero aborts the connection on close, so the peer's next
/// write is `BrokenPipe`, `ConnectionReset`, or `UnexpectedEof`.
fn reset_on_close(stream: &TcpStream) {
  #[cfg(unix)]
  {
    use std::os::fd::AsRawFd;
    let linger = libc::linger {
      l_onoff: 1,
      l_linger: 0,
    };
    // Safety: the descriptor belongs to `stream`. The option is a local value.
    let rc = unsafe {
      libc::setsockopt(
        stream.as_raw_fd(),
        libc::SOL_SOCKET,
        libc::SO_LINGER,
        &linger as *const libc::linger as *const libc::c_void,
        std::mem::size_of::<libc::linger>() as libc::socklen_t,
      )
    };
    assert_eq!(rc, 0, "linger");
  }
  #[cfg(windows)]
  {
    use std::os::windows::io::AsRawSocket;
    use windows_sys::Win32::Networking::WinSock::{
      LINGER, SO_LINGER, SOL_SOCKET, setsockopt,
    };
    let linger = LINGER {
      l_onoff: 1,
      l_linger: 0,
    };
    // Safety: the socket belongs to `stream`. The option is a local value.
    let rc = unsafe {
      setsockopt(
        stream.as_raw_socket() as usize,
        SOL_SOCKET,
        SO_LINGER,
        &linger as *const LINGER as windows_sys::core::PCSTR,
        std::mem::size_of::<LINGER>() as i32,
      )
    };
    assert_eq!(rc, 0, "linger");
  }
}

fn accept_for(
  listener: &TcpListener,
  wait: Duration,
) -> std::io::Result<TcpStream> {
  let deadline = Instant::now() + wait;
  loop {
    match listener.accept() {
      Ok((stream, _)) => {
        // The listener is nonblocking, so the accepted socket is too.
        // `read_exact` then returns `WouldBlock` and the dropped peer is
        // `ConnectionAborted` on Windows.
        stream.set_nonblocking(false)?;
        return Ok(stream);
      }
      Err(error) if accept_retry(error.kind()) => {
        if Instant::now() >= deadline {
          return Err(std::io::Error::new(ErrorKind::TimedOut, "accept"));
        }
        thread::sleep(Duration::from_millis(20));
      }
      Err(error) => return Err(error),
    }
  }
}

/// Windows `accept` reports `ConnectionAborted` when the peer resets before
/// the call. The runtime retries that kind.
fn accept_retry(kind: ErrorKind) -> bool {
  matches!(
    kind,
    ErrorKind::WouldBlock
      | ErrorKind::TimedOut
      | ErrorKind::Interrupted
      | ErrorKind::ConnectionAborted
      | ErrorKind::ConnectionReset
  )
}

fn family(v6: bool) -> BTreeSet<String> {
  ("localhost.", 9u16)
    .to_socket_addrs()
    .expect("localhost.")
    .filter(|addr| addr.is_ipv6() == v6)
    .map(|addr| addr.ip().to_string())
    .collect()
}

fn strings(value: &serde_json::Value) -> BTreeSet<String> {
  let Some(items) = value.as_array() else {
    return BTreeSet::new();
  };
  items
    .iter()
    .filter_map(|item| item.as_str().map(str::to_string))
    .collect()
}

#[test]
fn a_dns_name_connects_resolves_and_sends() {
  let tcp = bind_tcp();
  let tcp_port = tcp.local_addr().unwrap().port();
  let udp = UdpSocket::bind("127.0.0.1:0").expect("udp");
  let udp_port = udp.local_addr().unwrap().port();
  let (tx, rx) = std::sync::mpsc::channel();
  thread::spawn(move || {
    let stream = match accept_for(&tcp, Duration::from_secs(20)) {
      Ok(stream) => stream,
      Err(error) => {
        let _ = tx.send(Err(error.to_string()));
        return;
      }
    };
    let mut stream = stream;
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let mut buf = [0u8; 9];
    match stream.read_exact(&mut buf) {
      Ok(()) => {
        let _ = stream.write_all(b"name-pong");
        let _ = tx.send(Ok(buf.to_vec()));
      }
      Err(error) => {
        let _ = tx.send(Err(error.to_string()));
      }
    }
  });

  let dir = scratch("dns");
  let script = r#"const tcpPort = Number(Yuzuko.args[0]);
const udpPort = Number(Yuzuko.args[1]);
const enc = new TextEncoder();
const dec = new TextDecoder();
const conn = await Yuzuko.connectTcp({ hostname: "localhost.", port: tcpPort });
await conn.write(enc.encode("name-ping"));
const buf = new Uint8Array(9);
let got = 0;
while (got < 9) {
  const n = await conn.read(buf.subarray(got));
  if (n === null) {
    break;
  }
  got += n;
}
const tcpText = dec.decode(buf.subarray(0, got));
conn.close();
const sock = Yuzuko.listenDatagram({ hostname: "127.0.0.1", port: 0 });
const sent = await sock.send(enc.encode("udp-ping"), {
  hostname: "localhost.",
  port: udpPort,
});
sock.close();
let mx = "";
let mxReturned = "unset";
try {
  mxReturned = Yuzuko.resolveDns("localhost.", "MX");
  mx = "returned";
} catch (error) {
  mx = error instanceof TypeError ? error.message : error.name;
}
let empty = "";
try {
  Yuzuko.resolveDns("", "A");
  empty = "returned";
} catch (error) {
  empty = error instanceof TypeError ? error.message : "other";
}
let nonString = "";
try {
  Yuzuko.resolveDns(1, "A");
  nonString = "returned";
} catch (error) {
  nonString = error instanceof TypeError ? error.message : "other";
}
let missing = "";
try {
  const value = await Yuzuko.resolveDns("yuzuko-no-such.invalid.", "A");
  missing = "resolved " + JSON.stringify(value);
} catch (error) {
  missing = error.name + " " + error.message;
}
let connectMissing = "";
try {
  await Yuzuko.connectTcp({
    hostname: "yuzuko-no-such.invalid.",
    port: 9,
  });
  connectMissing = "connected";
} catch (error) {
  connectMissing = error.name + " " + error.message;
}
const a = await Yuzuko.resolveDns("localhost.", "A");
const aaaa = await Yuzuko.resolveDns("localhost.", "AAAA");
const report = {
  tcpText,
  sent,
  mx,
  mxPromise: mxReturned instanceof Promise,
  empty,
  nonString,
  missing,
  connectMissing,
  a,
  aaaa,
};
Yuzuko.stdout.writeSync(enc.encode(JSON.stringify(report)));
"#;
  fs::write(dir.join("main.ts"), script).unwrap();
  let output = run(
    &dir,
    &[
      "run",
      "--allow-net",
      "--no-prompt",
      "main.ts",
      &tcp_port.to_string(),
      &udp_port.to_string(),
    ],
  );
  let stdout = stdout_of(&output);
  let report: serde_json::Value = serde_json::from_str(stdout.trim())
    .unwrap_or_else(|error| panic!("report {error}\n{stdout}"));
  let received = rx
    .recv_timeout(Duration::from_secs(5))
    .expect("tcp payload");
  assert_eq!(received.expect("tcp read"), b"name-ping");
  assert_eq!(report["tcpText"], "name-pong", "{stdout}");
  assert_eq!(report["sent"], 8, "{stdout}");
  udp.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
  let mut datagram = [0u8; 16];
  let (count, _) = udp.recv_from(&mut datagram).expect("udp payload");
  assert_eq!(&datagram[..count], b"udp-ping");
  assert_eq!(
    report["mx"], "The record type must be \"A\" or \"AAAA\".",
    "{stdout}"
  );
  assert_eq!(report["mxPromise"], false, "{stdout}");
  assert_eq!(report["empty"], "The host must not be empty.", "{stdout}");
  assert_eq!(
    report["nonString"], "The host must be a string.",
    "{stdout}"
  );
  assert_eq!(
    report["missing"], "Error The host could not be resolved.",
    "{stdout}"
  );
  assert_eq!(
    report["connectMissing"], "Error The host could not be resolved.",
    "{stdout}"
  );
  assert_eq!(strings(&report["a"]), family(false), "{stdout}");
  assert_eq!(strings(&report["aaaa"]), family(true), "{stdout}");
  assert!(
    !strings(&report["a"]).iter().any(|addr| addr.contains(':')),
    "{stdout}"
  );
  assert!(
    strings(&report["aaaa"])
      .iter()
      .all(|addr| addr.contains(':')),
    "{stdout}"
  );
}

fn install_ring() {
  static ONCE: Once = Once::new();
  ONCE.call_once(|| {
    let _ = rustls::crypto::ring::default_provider().install_default();
  });
}

struct Material {
  chain: Vec<CertificateDer<'static>>,
  key: PrivateKeyDer<'static>,
  ca_pem: String,
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
  let issuer = rcgen::Issuer::from_params(&ca_params, &ca_key);
  let leaf_key = rcgen::KeyPair::generate().expect("leaf key");
  let mut leaf_params =
    rcgen::CertificateParams::new(vec!["localhost".to_string()])
      .expect("leaf params");
  leaf_params.is_ca = rcgen::IsCa::NoCa;
  leaf_params.key_usages = vec![rcgen::KeyUsagePurpose::DigitalSignature];
  leaf_params.extended_key_usages =
    vec![rcgen::ExtendedKeyUsagePurpose::ServerAuth];
  leaf_params
    .distinguished_name
    .push(rcgen::DnType::CommonName, "localhost");
  let leaf = leaf_params.signed_by(&leaf_key, &issuer).expect("leaf");
  Material {
    chain: vec![leaf.der().clone(), ca.der().clone()],
    key: PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(
      leaf_key.serialize_der(),
    )),
    ca_pem: ca.pem(),
  }
}

fn exchange(stream: &mut TcpStream, config: &Arc<ServerConfig>) -> bool {
  // `accept` on a nonblocking listener returns a nonblocking socket. rustls
  // treats `EAGAIN` as a failed handshake, so the accepted socket blocks.
  if stream.set_nonblocking(false).is_err() {
    return false;
  }
  let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
  let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));
  let Ok(mut conn) = ServerConnection::new(Arc::clone(config)) else {
    return false;
  };
  let mut tls = rustls::Stream::new(&mut conn, stream);
  let mut buf = [0u8; 8];
  if tls.read_exact(&mut buf).is_err() || &buf != b"tls-ping" {
    return false;
  }
  tls.write_all(b"tls-pong").is_ok() && tls.flush().is_ok()
}

#[test]
fn tls_handshake_exchanges_application_bytes() {
  let material = material();
  let config = Arc::new(
    ServerConfig::builder()
      .with_no_client_auth()
      .with_single_cert(material.chain, material.key)
      .expect("server config"),
  );
  let listener = bind_tcp();
  let port = listener.local_addr().unwrap().port();
  let config_thread = Arc::clone(&config);
  thread::spawn(move || {
    let deadline = Instant::now() + Duration::from_secs(25);
    let mut ok = 0u8;
    while ok < 2 && Instant::now() < deadline {
      let Ok(mut stream) = accept_for(&listener, Duration::from_secs(5)) else {
        continue;
      };
      if exchange(&mut stream, &config_thread) {
        ok += 1;
      }
    }
  });
  let dir = scratch("tls");
  let ca = serde_json::to_string(&material.ca_pem).unwrap();
  let script = r#"const port = __PORT__;
const ca = __CA__;
const enc = new TextEncoder();
const dec = new TextDecoder();
async function readAll(conn) {
  const buf = new Uint8Array(8);
  let got = 0;
  while (got < 8) {
    const n = await conn.read(buf.subarray(got));
    if (n === null) {
      break;
    }
    got += n;
  }
  return dec.decode(buf.subarray(0, got));
}
let unverified = "";
try {
  const conn = await Yuzuko.connectTls({ hostname: "localhost", port });
  unverified = "connected " + (await readAll(conn));
  conn.close();
} catch (error) {
  unverified = error.name + " " + error.message;
}
const direct = await Yuzuko.connectTls({
  hostname: "localhost",
  port,
  caCerts: [ca],
});
await direct.write(enc.encode("tls-ping"));
const directText = await readAll(direct);
const directRemote = direct.remoteAddr.hostname;
direct.close();
const tcp = await Yuzuko.connectTcp({ hostname: "127.0.0.1", port });
const started = await Yuzuko.startTls(tcp, {
  hostname: "localhost",
  caCerts: [ca],
});
await started.write(enc.encode("tls-ping"));
const startedText = await readAll(started);
const startedRemote = started.remoteAddr.hostname;
started.close();
const report = {
  unverified,
  directText,
  directRemote,
  startedText,
  startedRemote,
};
Yuzuko.stdout.writeSync(enc.encode(JSON.stringify(report)));
"#;
  let script = script
    .replace("__PORT__", &port.to_string())
    .replace("__CA__", &ca);
  fs::write(dir.join("main.ts"), script).unwrap();
  let output = run(&dir, &["run", "--allow-net", "--no-prompt", "main.ts"]);
  let stdout = stdout_of(&output);
  let report: serde_json::Value = serde_json::from_str(stdout.trim())
    .unwrap_or_else(|error| panic!("report {error}\n{stdout}"));
  assert_eq!(
    report["unverified"], "Error The TLS handshake failed.",
    "{stdout}"
  );
  assert_eq!(report["directText"], "tls-pong", "{stdout}");
  assert_eq!(report["directRemote"], "127.0.0.1", "{stdout}");
  assert_eq!(report["startedText"], "tls-pong", "{stdout}");
  assert_eq!(report["startedRemote"], "127.0.0.1", "{stdout}");
}

#[test]
fn socket_errors_use_their_classes() {
  let fin = bind_tcp();
  let fin_port = fin.local_addr().unwrap().port();
  thread::spawn(move || {
    if let Ok(mut stream) = accept_for(&fin, Duration::from_secs(20)) {
      let _ = stream.shutdown(Shutdown::Write);
      thread::sleep(Duration::from_millis(400));
      let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
      let mut buf = [0u8; 1];
      let _ = stream.read(&mut buf);
    }
  });
  let reset = bind_tcp();
  let reset_port = reset.local_addr().unwrap().port();
  thread::spawn(move || {
    if let Ok(stream) = accept_for(&reset, Duration::from_secs(20)) {
      reset_on_close(&stream);
      drop(stream);
    }
  });
  let dir = scratch("classes");
  let script = r#"const finPort = Number(Yuzuko.args[0]);
const resetPort = Number(Yuzuko.args[1]);
const names = [
  "AddrInUse",
  "AddrNotAvailable",
  "ConnectionRefused",
  "ConnectionReset",
  "ConnectionAborted",
  "NotConnected",
  "TimedOut",
  "BrokenPipe",
  "UnexpectedEof",
  "NetworkUnreachable",
  "WriteZero",
  "BadResource",
  "Http",
  "WouldBlock",
  "NotCapable",
];
const installed = names.every((name) => {
  const ctor = Yuzuko.errors[name];
  return typeof ctor === "function" && ctor.prototype instanceof Error;
});
const first = Yuzuko.listenTcp({ hostname: "127.0.0.1", port: 0 });
const busyPort = first.addr.port;
let addrInUse = "bound";
try {
  const second = Yuzuko.listenTcp({ hostname: "127.0.0.1", port: busyPort });
  second.close();
} catch (error) {
  addrInUse = [
    error.name,
    error.message,
    error instanceof Yuzuko.errors.AddrInUse,
    error instanceof DOMException,
  ].join(" ");
}
first.close();
const held = Yuzuko.listenTcp({ hostname: "127.0.0.1", port: 0 });
const closedPort = held.addr.port;
held.close();
let refused = "connected";
try {
  await Yuzuko.connectTcp({ hostname: "127.0.0.1", port: closedPort });
} catch (error) {
  refused = [
    error.name,
    error.message,
    error instanceof Yuzuko.errors.ConnectionRefused,
    error instanceof DOMException,
  ].join(" ");
}
const ending = await Yuzuko.connectTcp({ hostname: "127.0.0.1", port: finPort });
const ended = await ending.read(new Uint8Array(8));
ending.close();
const peer = await Yuzuko.connectTcp({ hostname: "127.0.0.1", port: resetPort });
let writeError = "none";
for (let attempt = 0; attempt < 40; attempt += 1) {
  try {
    await peer.write(new Uint8Array(4096));
  } catch (error) {
    const ctor = Yuzuko.errors[error.name];
    writeError = [
      error.name,
      error.message,
      error instanceof ctor,
      error instanceof DOMException,
    ].join(" ");
    break;
  }
  await new Promise((resolve) => setTimeout(resolve, 50));
}
const report = { installed, addrInUse, refused, ended, writeError };
Yuzuko.stdout.writeSync(new TextEncoder().encode(JSON.stringify(report)));
"#;
  fs::write(dir.join("main.ts"), script).unwrap();
  let output = run(
    &dir,
    &[
      "run",
      "--allow-net",
      "--no-prompt",
      "main.ts",
      &fin_port.to_string(),
      &reset_port.to_string(),
    ],
  );
  let stdout = stdout_of(&output);
  let report: serde_json::Value = serde_json::from_str(stdout.trim())
    .unwrap_or_else(|error| panic!("report {error}\n{stdout}"));
  assert_eq!(report["installed"], true, "{stdout}");
  assert_eq!(
    report["addrInUse"], "AddrInUse The address is already in use. true false",
    "{stdout}"
  );
  assert_eq!(
    report["refused"],
    "ConnectionRefused The connection was refused. true false",
    "{stdout}"
  );
  assert_eq!(report["ended"], serde_json::Value::Null, "{stdout}");
  let write_error = report["writeError"].as_str().unwrap_or("");
  let class = write_error.split(' ').next().unwrap_or("");
  let message = match class {
    "BrokenPipe" => "The socket pipe is broken.",
    "ConnectionReset" => "The connection was reset.",
    "UnexpectedEof" => "The socket reached the end of the stream.",
    _ => "",
  };
  assert_eq!(
    write_error,
    format!("{class} {message} true false"),
    "{stdout}"
  );
}

#[test]
fn a_missing_net_grant_throws_not_capable() {
  let dir = scratch("deny");
  fs::write(
    dir.join("main.ts"),
    r#"function describe(error, returned) {
  return [
    returned,
    error.name,
    error instanceof Yuzuko.errors.NotCapable,
    error instanceof Error,
    error instanceof DOMException,
    String(error.message).includes("--allow-net"),
  ].join(" ");
}
const lines = [];
let returned = "unset";
try {
  returned = Yuzuko.connectTcp({ hostname: "127.0.0.1", port: 9 });
  lines.push("connect returned");
} catch (error) {
  lines.push("connect " + describe(error, returned));
}
returned = "unset";
try {
  returned = Yuzuko.resolveDns("example.com", "A");
  lines.push("resolve returned");
} catch (error) {
  lines.push("resolve " + describe(error, returned));
}
returned = "unset";
try {
  returned = Yuzuko.listenTcp();
  lines.push("listen returned");
} catch (error) {
  lines.push("listen " + describe(error, returned));
}
returned = "unset";
try {
  returned = Yuzuko.connectTls({ hostname: "127.0.0.1", port: 9 });
  lines.push("tls returned");
} catch (error) {
  lines.push("tls " + describe(error, returned));
}
console.log(lines.join("\n"));
"#,
  )
  .unwrap();
  let output = run(&dir, &["run", "--no-prompt", "main.ts"]);
  let stdout = stdout_of(&output);
  for label in ["connect", "resolve", "listen", "tls"] {
    assert!(
      stdout
        .lines()
        .any(|line| line
          == format!("{label} unset NotCapable true true false true")),
      "{label} in\n{stdout}"
    );
  }
}

#[test]
fn a_localhost_grant_does_not_cover_resolved_addresses() {
  let dir = scratch("localhost-grant");
  fs::write(
    dir.join("main.ts"),
    r#"function describe(error) {
  return [
    error.name,
    error instanceof Yuzuko.errors.NotCapable,
    error instanceof Error,
    error instanceof DOMException,
    String(error.message).includes("--allow-net"),
  ].join(" ");
}
const lines = [];
try {
  const pending = Yuzuko.resolveDns("localhost.", "A");
  if (!(pending instanceof Promise)) {
    lines.push("resolve sync");
  } else {
    const value = await pending;
    lines.push("resolve " + JSON.stringify(value));
  }
} catch (error) {
  lines.push("resolve " + describe(error));
}
try {
  const pending = Yuzuko.connectTcp({ hostname: "localhost.", port: 9 });
  if (!(pending instanceof Promise)) {
    lines.push("connect sync");
  } else {
    await pending;
    lines.push("connect opened");
  }
} catch (error) {
  lines.push("connect " + describe(error));
}
console.log(lines.join("\n"));
"#,
  )
  .unwrap();
  let output = run(
    &dir,
    &["run", "--allow-net=localhost", "--no-prompt", "main.ts"],
  );
  let stdout = stdout_of(&output);
  let resolved = stdout
    .lines()
    .find_map(|line| line.strip_prefix("resolve "))
    .unwrap_or_else(|| {
      panic!("resolve in\n{stdout}\n{}", text(&output.stderr))
    });
  let value: serde_json::Value = serde_json::from_str(resolved)
    .unwrap_or_else(|error| panic!("resolve {error} in\n{stdout}"));
  let expected = family(false);
  assert!(!expected.is_empty(), "localhost. has no A records");
  assert_eq!(strings(&value), expected, "{stdout}");
  assert!(
    stdout
      .lines()
      .any(|line| line == "connect NotCapable true true false true"),
    "connect in\n{stdout}\n{}",
    text(&output.stderr)
  );
}
