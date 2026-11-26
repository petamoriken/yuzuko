//! Credential headers across fetch redirects.

use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

fn scratch(name: &str) -> PathBuf {
  let dir = std::env::temp_dir().join(format!(
    "yuzuko-fetch-redirect-{}-{}-{name}",
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

/// Reads one request head and returns it with lowercase header names.
fn read_head(stream: &mut TcpStream) -> String {
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
  String::from_utf8_lossy(&head).to_ascii_lowercase()
}

fn reply(stream: &mut TcpStream, status: &str, location: Option<&str>) {
  let mut message = format!("HTTP/1.1 {status}\r\n");
  if let Some(location) = location {
    message.push_str(&format!("Location: {location}\r\n"));
  }
  message.push_str("Content-Length: 2\r\nConnection: close\r\n\r\nok");
  let _ = stream.write_all(message.as_bytes());
}

#[test]
fn a_cross_origin_redirect_drops_credentials() {
  let first = TcpListener::bind("127.0.0.1:0").unwrap();
  let other = TcpListener::bind("127.0.0.1:0").unwrap();
  let first_port = first.local_addr().unwrap().port();
  let other_port = other.local_addr().unwrap().port();

  let same_origin = thread::spawn(move || {
    let mut heads = Vec::new();
    for _ in 0..3 {
      let (mut stream, _) = first.accept().unwrap();
      let head = read_head(&mut stream);
      if head.starts_with("get /cross ") {
        let to = format!("http://127.0.0.1:{other_port}/landed");
        reply(&mut stream, "302 Found", Some(&to));
      } else if head.starts_with("get /same ") {
        reply(&mut stream, "302 Found", Some("/kept"));
      } else {
        reply(&mut stream, "200 OK", None);
      }
      heads.push(head);
    }
    heads
  });
  let cross_origin = thread::spawn(move || {
    let (mut stream, _) = other.accept().unwrap();
    let head = read_head(&mut stream);
    reply(&mut stream, "200 OK", None);
    head
  });

  let dir = scratch("cross");
  fs::write(
    dir.join("main.ts"),
    format!(
      r#"const headers = {{
  authorization: "Bearer secret",
  cookie: "session=secret",
  "proxy-authorization": "Basic secret",
  "x-kept": "yes",
}};
const cross = await fetch("http://127.0.0.1:{first_port}/cross", {{ headers }});
console.log("cross " + cross.status + " " + cross.redirected);
const same = await fetch("http://127.0.0.1:{first_port}/same", {{ headers }});
console.log("same " + same.status + " " + same.redirected);
"#
    ),
  )
  .unwrap();
  let output = Command::new(env!("CARGO_BIN_EXE_yuzuko"))
    .current_dir(&dir)
    .args(["run", "-A", "main.ts"])
    .env("YUZUKO_HEADLESS", "1")
    .stdin(Stdio::null())
    .output()
    .unwrap();
  let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
  let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
  assert!(output.status.success(), "{stdout}\n{stderr}");
  assert!(stdout.contains("cross 200 true"), "{stdout}\n{stderr}");
  assert!(stdout.contains("same 200 true"), "{stdout}\n{stderr}");

  let landed = cross_origin.join().unwrap();
  assert!(landed.starts_with("get /landed "), "{landed}");
  assert!(landed.contains("x-kept: yes"), "{landed}");
  assert!(!landed.contains("authorization:"), "{landed}");
  assert!(!landed.contains("cookie:"), "{landed}");

  let heads = same_origin.join().unwrap();
  let kept = heads
    .iter()
    .find(|head| head.starts_with("get /kept "))
    .expect("same-origin hop");
  assert!(kept.contains("authorization: bearer secret"), "{kept}");
  assert!(kept.contains("cookie: session=secret"), "{kept}");
  assert!(kept.contains("proxy-authorization: basic secret"), "{kept}");
  let _ = fs::remove_dir_all(&dir);
}
