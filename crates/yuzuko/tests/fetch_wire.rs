//! Malformed HTTP/1 responses reject a fetch without ending the process.

use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::thread;
use std::time::Duration;

fn scratch(name: &str) -> PathBuf {
  let dir = std::env::temp_dir().join(format!(
    "yuzuko-fetch-wire-{}-{}-{name}",
    std::process::id(),
    std::time::SystemTime::now()
      .duration_since(std::time::UNIX_EPOCH)
      .unwrap_or_default()
      .as_nanos()
  ));
  let _ = fs::remove_dir_all(&dir);
  fs::create_dir_all(&dir).unwrap();
  dir
}

fn read_head(stream: &mut TcpStream) {
  stream
    .set_read_timeout(Some(Duration::from_secs(10)))
    .unwrap();
  let mut head = Vec::new();
  let mut byte = [0u8; 1];
  while !head.ends_with(b"\r\n\r\n") {
    match stream.read(&mut byte) {
      Ok(1) => head.push(byte[0]),
      _ => break,
    }
  }
}

/// Serves `responses` to successive connections, then runs `script`, which
/// can read the port from `PORT`.
fn serve(name: &str, responses: Vec<Vec<u8>>, script: &str) -> Output {
  serve_holding(name, responses, Duration::from_millis(100), script)
}

/// Like `serve`, but keeps each connection open for `hold` after writing,
/// so a client that waits for more bytes is still waiting.
fn serve_holding(
  name: &str,
  responses: Vec<Vec<u8>>,
  hold: Duration,
  script: &str,
) -> Output {
  let listener = TcpListener::bind("127.0.0.1:0").unwrap();
  let port = listener.local_addr().unwrap().port();
  let server = thread::spawn(move || {
    let mut connections = Vec::new();
    for response in responses {
      let Ok((mut stream, _)) = listener.accept() else {
        break;
      };
      // Each connection has its own thread, so a held one does not delay
      // the next accept.
      connections.push(thread::spawn(move || {
        read_head(&mut stream);
        let _ = stream.write_all(&response);
        let _ = stream.flush();
        thread::sleep(hold);
      }));
    }
    for connection in connections {
      let _ = connection.join();
    }
  });
  let dir = scratch(name);
  fs::write(
    dir.join("main.ts"),
    format!("const PORT = {port};\n{script}"),
  )
  .unwrap();
  let output = Command::new(env!("CARGO_BIN_EXE_yuzuko"))
    .current_dir(&dir)
    .args(["run", "-A", "main.ts"])
    .env("YUZUKO_HEADLESS", "1")
    .stdin(Stdio::null())
    .output()
    .unwrap();
  let _ = server.join();
  let _ = fs::remove_dir_all(&dir);
  output
}

#[test]
fn a_huge_chunk_size_rejects_the_body() {
  let mut responses = Vec::new();
  for size in ["7FFFFFFFFFFFFFFF", "FFFFFFFFFFFFFFFF"] {
    let response = format!(
      "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\
       Connection: close\r\n\r\n{size}\r\nabc"
    );
    responses.push(response.into_bytes());
  }
  let output = serve(
    "huge-chunk",
    responses,
    r#"for (const label of ["large", "max"]) {
  try {
    const response = await fetch(`http://127.0.0.1:${PORT}/${label}`);
    await response.arrayBuffer();
    console.log(label + " read");
  } catch (error) {
    console.log(label + " " + error.name);
  }
}
console.log("alive");
"#,
  );
  let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
  let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
  assert!(output.status.success(), "{stdout}\n{stderr}");
  assert!(stdout.contains("large TypeError"), "{stdout}\n{stderr}");
  assert!(stdout.contains("max TypeError"), "{stdout}\n{stderr}");
  assert!(stdout.contains("alive"), "{stdout}\n{stderr}");
}

#[test]
fn an_unterminated_line_or_head_rejects_without_waiting() {
  let filler = "a".repeat(2 * 1024 * 1024);
  let line =
    format!("HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n{filler}");
  let head =
    format!("HTTP/1.1 100 Continue\r\n\r\nHTTP/1.1 200 OK\r\nX: {filler}");
  let output = serve_holding(
    "unterminated",
    vec![line.into_bytes(), head.into_bytes()],
    Duration::from_secs(30),
    r#"for (const label of ["line", "head"]) {
  const started = Date.now();
  try {
    const response = await fetch(`http://127.0.0.1:${PORT}/${label}`);
    await response.arrayBuffer();
    console.log(label + " read");
  } catch (error) {
    console.log(label + " " + error.name);
  }
  // The server holds the connection for 30 seconds after the bytes.
  console.log(label + (Date.now() - started < 20000 ? " prompt" : " waited"));
}
"#,
  );
  let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
  let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
  assert!(output.status.success(), "{stdout}\n{stderr}");
  assert!(stdout.contains("line TypeError"), "{stdout}\n{stderr}");
  assert!(stdout.contains("head TypeError"), "{stdout}\n{stderr}");
  assert!(stdout.contains("line prompt"), "{stdout}\n{stderr}");
  assert!(stdout.contains("head prompt"), "{stdout}\n{stderr}");
}

#[test]
fn an_unread_body_holds_the_server_until_it_is_read() {
  use std::sync::Arc;
  use std::sync::atomic::{AtomicUsize, Ordering};
  let listener = TcpListener::bind("127.0.0.1:0").unwrap();
  let port = listener.local_addr().unwrap().port();
  let written = Arc::new(AtomicUsize::new(0));
  let counter = Arc::clone(&written);
  thread::spawn(move || {
    let Ok((mut stream, _)) = listener.accept() else {
      return;
    };
    read_head(&mut stream);
    let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n");
    let block = vec![b'x'; 64 * 1024];
    while stream.write_all(&block).is_ok() {
      counter.fetch_add(block.len(), Ordering::Relaxed);
    }
  });
  let dir = scratch("unread");
  fs::write(
    dir.join("main.ts"),
    format!(
      r#"const response = await fetch("http://127.0.0.1:{port}/");
await new Promise((resolve) => setTimeout(resolve, 2000));
console.log("waited");
await new Promise((resolve) => setTimeout(resolve, 500));
const reader = response.body!.getReader();
let total = 0;
while (total < 8 * 1024 * 1024) {{
  const {{ value, done }} = await reader.read();
  if (done) break;
  total += value.byteLength;
}}
console.log("read " + (total >= 8 * 1024 * 1024));
await reader.cancel();
console.log("canceled");
"#
    ),
  )
  .unwrap();
  let mut child = Command::new(env!("CARGO_BIN_EXE_yuzuko"))
    .current_dir(&dir)
    .args(["run", "-A", "main.ts"])
    .env("YUZUKO_HEADLESS", "1")
    .stdin(Stdio::null())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .spawn()
    .unwrap();
  // Sample the server when the script has waited two seconds without
  // reading.
  use std::io::BufRead;
  let mut lines = std::io::BufReader::new(child.stdout.take().unwrap());
  let mut stdout = String::new();
  let mut held = None;
  let mut line = String::new();
  while lines.read_line(&mut line).unwrap_or(0) > 0 {
    if line.trim() == "waited" {
      held = Some(written.load(Ordering::Relaxed));
    }
    stdout.push_str(&line);
    line.clear();
  }
  let output = child.wait_with_output().unwrap();
  let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
  let held = held.expect("the script did not wait");
  let _ = fs::remove_dir_all(&dir);
  assert!(output.status.success(), "{stdout}\n{stderr}");
  assert!(stdout.contains("read true"), "{stdout}\n{stderr}");
  assert!(stdout.contains("canceled"), "{stdout}\n{stderr}");
  // The 1 MiB stream queue, the producer's chunks, and the socket buffers
  // hold a few megabytes. An eager pump keeps taking bytes.
  assert!(held < 12 * 1024 * 1024, "{held} bytes sent before a read");
}
