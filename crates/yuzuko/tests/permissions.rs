//! `yuzuko run`, `yuzuko test`, `yuzuko repl`, and a packaged launch enforce
//! the permission policy.

#![cfg(feature = "fs")]

use std::fs;
#[cfg(feature = "fetch")]
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
#[cfg(feature = "fetch")]
use std::sync::Arc;
#[cfg(feature = "fetch")]
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

fn bin() -> PathBuf {
  PathBuf::from(env!("CARGO_BIN_EXE_yuzuko"))
}

fn scratch(name: &str) -> PathBuf {
  let dir = std::env::temp_dir().join(format!(
    "yuzuko-permissions-{}-{}-{name}",
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

fn write(path: &Path, text: &str) {
  if let Some(parent) = path.parent() {
    fs::create_dir_all(parent).unwrap();
  }
  fs::write(path, text).unwrap();
}

fn text(bytes: &[u8]) -> String {
  String::from_utf8_lossy(bytes).into_owned()
}

fn run(dir: &Path, args: &[&str]) -> Output {
  run_env(dir, args, &[])
}

fn run_env(dir: &Path, args: &[&str], env: &[(&str, &str)]) -> Output {
  let mut command = Command::new(bin());
  command
    .current_dir(dir)
    .args(args)
    .env("YUZUKO_HEADLESS", "1")
    .stdin(Stdio::null())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped());
  // A scoped run grant rejects an inherited `LD_*` or `DYLD_*` variable.
  // Drop those names here so a successful spawn is not tied to the shell
  // that launched the test. An explicit entry below can set one again.
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
  for (key, value) in env {
    command.env(key, value);
  }
  let mut child = command.spawn().expect("spawn yuzuko");
  let start = Instant::now();
  let timeout = if args.first() == Some(&"package") {
    Duration::from_secs(90)
  } else {
    Duration::from_secs(20)
  };
  loop {
    if child.try_wait().unwrap().is_some() {
      return child.wait_with_output().unwrap();
    }
    if start.elapsed() > timeout {
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

const READ_NOTE: &str = r#"const text = await Yuzuko.readTextFile("note.txt");
let outside = "blocked";
try {
  await Yuzuko.readTextFile("../other/secret.txt");
  outside = "leaked";
} catch (error) {
  outside = error.name;
}
console.log(text.trim());
console.log(outside);
"#;

#[test]
fn run_without_a_grant_throws_not_allowed() {
  let dir = scratch("deny");
  write(&dir.join("note.txt"), "hello\n");
  write(
    &dir.join("main.ts"),
    r#"try {
  await Yuzuko.readTextFile("note.txt");
  console.log("allowed");
} catch (error) {
  console.log(error.name + ":" + error.message);
}
"#,
  );
  let output = run(&dir, &["run", "main.ts"]);
  assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
  let stdout = text(&output.stdout);
  assert!(
    stdout.contains("NotAllowedError:Requires read access"),
    "{stdout}"
  );
  assert!(stdout.contains("--allow-read"), "{stdout}");
}

#[test]
fn a_missing_component_does_not_reveal_the_file() {
  let dir = scratch("oracle");
  write(&dir.join("secret"), "hidden-oracle\n");
  write(
    &dir.join("main.ts"),
    r#"async function probe(path) {
  try {
    await Yuzuko.readTextFile(path);
    console.log("opened");
  } catch (error) {
    console.log(error.name);
  }
}
await probe("secret/probe");
await probe("absent/probe");
"#,
  );
  let output = run(&dir, &["run", "--no-prompt", "main.ts"]);
  assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
  let stdout = text(&output.stdout);
  assert_eq!(stdout, "NotAllowedError\nNotAllowedError\n");
  assert!(!stdout.contains("hidden-oracle"), "{stdout}");
  assert!(!stdout.contains("OperationError"), "{stdout}");
}

#[cfg(feature = "fetch")]
#[test]
fn an_ipv6_deny_matches_a_bracketed_url() {
  let dir = scratch("ipv6");
  write(
    &dir.join("main.ts"),
    r#"try {
  await fetch("http://[::1]:9/");
  console.log("opened");
} catch (error) {
  console.log(error.name + ":" + error.message);
}
"#,
  );
  let denied =
    run(&dir, &["run", "--no-prompt", "--deny-net=[::1]", "main.ts"]);
  assert_eq!(denied.status.code(), Some(0), "{}", text(&denied.stderr));
  let stdout = text(&denied.stdout);
  assert!(
    stdout.contains("NotAllowedError") && stdout.contains("[::1]"),
    "{stdout}"
  );
  assert!(!stdout.contains("[[::1]]"), "{stdout}");
  let allowed = run(
    &dir,
    &["run", "--no-prompt", "--allow-net=[::1]", "main.ts"],
  );
  assert_eq!(allowed.status.code(), Some(0), "{}", text(&allowed.stderr));
  let stdout = text(&allowed.stdout);
  assert!(!stdout.contains("NotAllowedError"), "{stdout}");
}

#[test]
fn a_hard_link_needs_write_access_to_the_old_file() {
  let root = scratch("link");
  let secret = root.join("secret.txt");
  let out = root.join("out");
  fs::create_dir_all(&out).unwrap();
  write(&secret, "original\n");
  let linked = out.join("link.txt");
  write(
    &root.join("main.ts"),
    r#"const [oldPath, newPath] = Yuzuko.args;
try {
  await Yuzuko.link(oldPath, newPath);
  await Yuzuko.writeTextFile(newPath, "changed\n");
  console.log(await Yuzuko.readTextFile(oldPath));
} catch (error) {
  console.log(error.name);
}
"#,
  );
  let secret_arg = secret.display().to_string();
  let linked_arg = linked.display().to_string();
  let read = format!("--allow-read={secret_arg}");
  let write_out = format!("--allow-write={}", out.display());
  let denied = run(
    &root,
    &[
      "run",
      "--no-prompt",
      &read,
      &write_out,
      "main.ts",
      &secret_arg,
      &linked_arg,
    ],
  );
  assert_eq!(denied.status.code(), Some(0), "{}", text(&denied.stderr));
  assert_eq!(text(&denied.stdout), "NotAllowedError\n");
  assert!(!linked.exists());
  assert_eq!(fs::read_to_string(&secret).unwrap(), "original\n");

  let write_old = format!("--allow-write={secret_arg}");
  let allowed = run(
    &root,
    &[
      "run",
      "--no-prompt",
      &read,
      &write_old,
      &write_out,
      "main.ts",
      &secret_arg,
      &linked_arg,
    ],
  );
  assert_eq!(allowed.status.code(), Some(0), "{}", text(&allowed.stderr));
  assert_eq!(text(&allowed.stdout).trim(), "changed");
  assert_eq!(fs::read_to_string(&secret).unwrap(), "changed\n");
}

#[test]
fn a_relative_symlink_is_checked_from_the_link_directory() {
  let root = scratch("symlink");
  let dir_a = root.join("dirA");
  let dir_b = root.join("dirB");
  fs::create_dir_all(&dir_a).unwrap();
  fs::create_dir_all(&dir_b).unwrap();
  write(&dir_a.join("target.txt"), "from-a\n");
  write(&dir_b.join("target.txt"), "from-b\n");
  write(
    &dir_a.join("main.ts"),
    r#"const newPath = Yuzuko.args[0];
try {
  await Yuzuko.symlink("target.txt", newPath);
  console.log("linked");
} catch (error) {
  console.log(error.name);
}
"#,
  );
  let link = dir_b.join("link");
  let link_arg = link.display().to_string();
  let read_a = format!("--allow-read={}", dir_a.join("target.txt").display());
  let write_b = format!("--allow-write={}", dir_b.display());
  let denied = run(
    &dir_a,
    &[
      "run",
      "--no-prompt",
      &read_a,
      &write_b,
      "main.ts",
      &link_arg,
    ],
  );
  assert_eq!(denied.status.code(), Some(0), "{}", text(&denied.stderr));
  assert_eq!(text(&denied.stdout), "NotAllowedError\n");
  assert!(fs::read_link(&link).is_err());

  let read_b = format!("--allow-read={}", dir_b.display());
  let allowed = run(
    &dir_a,
    &[
      "run",
      "--no-prompt",
      &read_b,
      &write_b,
      "main.ts",
      &link_arg,
    ],
  );
  assert_eq!(allowed.status.code(), Some(0), "{}", text(&allowed.stderr));
  let stdout = text(&allowed.stdout);
  assert!(!stdout.contains("NotAllowedError"), "{stdout}");
  if stdout.contains("linked") {
    assert_eq!(fs::read_link(&link).unwrap(), Path::new("target.txt"));
  } else {
    assert!(stdout.contains("PermissionDenied"), "{stdout}");
  }
}

#[cfg(feature = "fetch")]
fn fetch_probe(url: &str) -> String {
  format!(
    r#"try {{
  await fetch({url:?});
  console.log("opened");
}} catch (error) {{
  console.log(error.name + ":" + error.message);
}}
"#
  )
}

#[cfg(feature = "fetch")]
#[test]
fn an_ipv4_mapped_address_uses_the_ipv4_grant() {
  let dir = scratch("mapped");
  write(
    &dir.join("main.ts"),
    &fetch_probe("http://[::ffff:127.0.0.1]:9/"),
  );
  let denied = run(
    &dir,
    &["run", "--no-prompt", "--deny-net=127.0.0.1", "main.ts"],
  );
  assert_eq!(denied.status.code(), Some(0), "{}", text(&denied.stderr));
  let stdout = text(&denied.stdout);
  assert!(
    stdout.contains("NotAllowedError") && stdout.contains("127.0.0.1"),
    "{stdout}"
  );
  assert!(!stdout.contains("::ffff"), "{stdout}");

  let allowed = run(
    &dir,
    &["run", "--no-prompt", "--allow-net=127.0.0.1", "main.ts"],
  );
  assert_eq!(allowed.status.code(), Some(0), "{}", text(&allowed.stderr));
  assert!(
    !text(&allowed.stdout).contains("NotAllowedError"),
    "{}",
    text(&allowed.stdout)
  );
}

#[cfg(feature = "fetch")]
#[test]
fn a_deny_of_a_mapped_address_covers_the_ipv4_host() {
  let dir = scratch("mapped-rule");
  write(&dir.join("main.ts"), &fetch_probe("http://127.0.0.1:9/"));
  let denied = run(
    &dir,
    &[
      "run",
      "--no-prompt",
      "--deny-net=[::ffff:127.0.0.1]",
      "main.ts",
    ],
  );
  assert_eq!(denied.status.code(), Some(0), "{}", text(&denied.stderr));
  let stdout = text(&denied.stdout);
  assert!(
    stdout.contains("NotAllowedError") && stdout.contains("127.0.0.1"),
    "{stdout}"
  );
}

#[cfg(feature = "fetch")]
#[test]
fn an_ipv6_deny_matches_the_canonical_spelling() {
  let dir = scratch("ipv6-canonical");
  write(
    &dir.join("main.ts"),
    &fetch_probe("http://[2001:db8::1]:9/"),
  );
  let denied = run(
    &dir,
    &["run", "--no-prompt", "--deny-net=[2001:0db8::1]", "main.ts"],
  );
  assert_eq!(denied.status.code(), Some(0), "{}", text(&denied.stderr));
  let stdout = text(&denied.stdout);
  assert!(
    stdout.contains("NotAllowedError") && stdout.contains("[2001:db8::1]"),
    "{stdout}"
  );
}

#[cfg(feature = "fetch")]
#[test]
fn an_allowed_name_cannot_connect_to_a_loopback_address() {
  let dir = scratch("rebind");
  write(
    &dir.join("main.ts"),
    r#"for (const url of ["http://localhost:9/", "https://localhost:9/"]) {
  try {
    await fetch(url);
    console.log(url + " opened");
  } catch (error) {
    console.log(url + " " + error.name + ":" + error.message);
  }
}
"#,
  );
  let denied = run(
    &dir,
    &["run", "--no-prompt", "--allow-net=localhost", "main.ts"],
  );
  assert_eq!(denied.status.code(), Some(0), "{}", text(&denied.stderr));
  let stdout = text(&denied.stdout);
  assert!(
    stdout.contains("http://localhost:9/ NotAllowedError"),
    "{stdout}"
  );
  assert!(
    stdout.contains("https://localhost:9/ NotAllowedError"),
    "{stdout}"
  );
  assert!(
    stdout.contains("127.0.0.1") || stdout.contains("[::1]"),
    "{stdout}"
  );

  let allowed = run(
    &dir,
    &[
      "run",
      "--no-prompt",
      "--allow-net=localhost,127.0.0.1,::1",
      "main.ts",
    ],
  );
  assert_eq!(allowed.status.code(), Some(0), "{}", text(&allowed.stderr));
  let stdout = text(&allowed.stdout);
  assert!(!stdout.contains("NotAllowedError"), "{stdout}");
  assert!(stdout.contains("http://localhost:9/"), "{stdout}");
  assert!(stdout.contains("https://localhost:9/"), "{stdout}");
}

#[cfg(feature = "socket")]
#[test]
fn an_allowed_name_cannot_open_a_socket_to_a_loopback_address() {
  use std::io::ErrorKind;
  use std::net::{TcpListener, UdpSocket};
  use std::sync::Arc;
  use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
  use std::thread::JoinHandle;

  let hits = Arc::new(AtomicUsize::new(0));
  let stop = Arc::new(AtomicBool::new(false));
  let v4 = TcpListener::bind("127.0.0.1:0").unwrap();
  let port = v4.local_addr().unwrap().port();
  let mut listeners = vec![v4];
  if let Ok(v6) = TcpListener::bind(format!("[::1]:{port}")) {
    listeners.push(v6);
  }
  let mut threads: Vec<JoinHandle<()>> = Vec::new();
  for listener in listeners {
    listener.set_nonblocking(true).unwrap();
    let hits = Arc::clone(&hits);
    let stop = Arc::clone(&stop);
    threads.push(std::thread::spawn(move || {
      while !stop.load(Ordering::Relaxed) {
        match listener.accept() {
          Ok((stream, _)) => {
            hits.fetch_add(1, Ordering::SeqCst);
            let _ = stream.shutdown(std::net::Shutdown::Both);
          }
          Err(error) if error.kind() == ErrorKind::WouldBlock => {
            std::thread::sleep(Duration::from_millis(10));
          }
          Err(_) => break,
        }
      }
    }));
  }
  let mut udp = vec![UdpSocket::bind(("127.0.0.1", port)).unwrap()];
  if let Ok(v6) = UdpSocket::bind(format!("[::1]:{port}")) {
    udp.push(v6);
  }
  for socket in &udp {
    socket.set_nonblocking(true).unwrap();
  }

  let dir = scratch("socket-rebind");
  let ws_script = format!(
    r#"const port = {port};
await new Promise((resolve) => {{
  const ws = new WebSocket("ws://localhost:" + port + "/");
  let opened = false;
  ws.addEventListener("open", () => {{
    opened = true;
    ws.close();
  }});
  ws.addEventListener("close", () => {{
    console.log(opened ? "ws opened" : "ws closed");
    resolve();
  }});
}});
"#
  );
  write(
    &dir.join("deny.ts"),
    &format!(
      r#"{ws_script}try {{
  const transport = new WebTransport("https://localhost:{port}/");
  await transport.ready;
  console.log("wt opened");
}} catch (error) {{
  console.log("wt " + error.name + ":" + error.message);
}}
"#
    ),
  );
  write(&dir.join("allow.ts"), &ws_script);
  let denied = run(
    &dir,
    &["run", "--no-prompt", "--allow-net=localhost", "deny.ts"],
  );
  assert_eq!(denied.status.code(), Some(0), "{}", text(&denied.stderr));
  let stdout = text(&denied.stdout);
  assert!(stdout.contains("ws closed"), "{stdout}");
  assert!(!stdout.contains("ws opened"), "{stdout}");
  assert!(stdout.contains("wt WebTransportError:"), "{stdout}");
  assert!(
    stdout.contains("127.0.0.1") || stdout.contains("[::1]"),
    "{stdout}"
  );
  assert_eq!(hits.load(Ordering::SeqCst), 0, "{stdout}");
  let mut packets = 0;
  let mut buf = [0u8; 2048];
  for socket in &udp {
    while socket.recv(&mut buf).is_ok() {
      packets += 1;
    }
  }
  assert_eq!(packets, 0, "{stdout}");

  let allowed = run(
    &dir,
    &[
      "run",
      "--no-prompt",
      "--allow-net=localhost,127.0.0.1,::1",
      "allow.ts",
    ],
  );
  assert_eq!(allowed.status.code(), Some(0), "{}", text(&allowed.stderr));
  let stdout = text(&allowed.stdout);
  assert!(
    stdout.contains("ws closed") || stdout.contains("ws opened"),
    "{stdout}"
  );
  assert!(
    hits.load(Ordering::SeqCst) > 0,
    "stdout={stdout} hits={}",
    hits.load(Ordering::SeqCst)
  );

  stop.store(true, Ordering::Relaxed);
  for thread in threads {
    let _ = thread.join();
  }
}

#[cfg(feature = "fetch")]
#[test]
fn an_event_source_reconnect_checks_the_host_again() {
  use std::io::{ErrorKind, Read, Write};
  use std::net::{Shutdown, TcpListener};
  use std::sync::atomic::AtomicUsize;

  let listener = TcpListener::bind("127.0.0.1:0").unwrap();
  let port = listener.local_addr().unwrap().port();
  listener.set_nonblocking(true).unwrap();
  let hits = Arc::new(AtomicUsize::new(0));
  let counting = Arc::clone(&hits);
  let server = std::thread::spawn(move || {
    let deadline = Instant::now() + Duration::from_secs(8);
    while Instant::now() < deadline {
      match listener.accept() {
        Ok((mut stream, _)) => {
          counting.fetch_add(1, Ordering::SeqCst);
          let _ = stream.set_read_timeout(Some(Duration::from_millis(200)));
          let mut buf = [0u8; 1024];
          let _ = stream.read(&mut buf);
          let _ = stream.write_all(
            b"HTTP/1.1 200 OK\r\n\
Content-Type: text/event-stream\r\n\
Connection: close\r\n\
\r\n\
data: hi\n\n",
          );
          let _ = stream.shutdown(Shutdown::Both);
        }
        Err(error)
          if error.kind() == ErrorKind::WouldBlock
            || error.kind() == ErrorKind::Interrupted =>
        {
          std::thread::sleep(Duration::from_millis(20));
        }
        Err(_) => break,
      }
    }
  });
  let dir = scratch("event-source-revoke");
  write(
    &dir.join("main.ts"),
    &format!(
      r#"let errors = 0;
const source = new EventSource("http://localhost:{port}/");
source.onopen = () => console.log("open");
source.onerror = () => {{
  errors += 1;
  Yuzuko.permissions.revokeSync({{ name: "net", host: "localhost" }});
  console.log("error " + errors);
}};
setTimeout(() => {{
  Yuzuko.permissions.revokeSync({{ name: "net", host: "localhost" }});
}}, 400);
await new Promise((resolve) => setTimeout(resolve, 4500));
source.close();
console.log("closed " + errors);
"#
    ),
  );
  let output = run(
    &dir,
    &[
      "run",
      "--no-prompt",
      "--allow-net=localhost,127.0.0.1,::1",
      "main.ts",
    ],
  );
  let _ = server.join();
  assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
  let stdout = text(&output.stdout);
  assert!(stdout.contains("open"), "{stdout}");
  assert!(stdout.contains("error 2"), "{stdout}");
  // `127.0.0.1` stays granted. Only the `localhost` name is revoked, so a
  // reconnect that skipped the host check would still be accepted here.
  assert_eq!(hits.load(Ordering::SeqCst), 1, "{stdout}");
}

#[cfg(unix)]
#[test]
fn a_symlink_swap_does_not_return_the_other_file() {
  use std::sync::Arc;
  use std::sync::atomic::{AtomicBool, Ordering};

  let root = scratch("swap");
  let proj = root.join("proj");
  let outside = root.join("outside");
  fs::create_dir_all(&proj).unwrap();
  fs::create_dir_all(&outside).unwrap();
  let inside = proj.join("inside.txt");
  let secret = outside.join("secret.txt");
  fs::write(&inside, "INSIDE\n").unwrap();
  fs::write(&secret, "OUTSIDE\n").unwrap();
  let link = proj.join("link");
  std::os::unix::fs::symlink(&inside, &link).unwrap();
  write(
    &proj.join("once.ts"),
    r#"console.log(Yuzuko.readTextFileSync("link"));
"#,
  );
  let once = run(
    &proj,
    &[
      "run",
      "--no-prompt",
      "--allow-read=.",
      "--allow-write=.",
      "once.ts",
    ],
  );
  assert_eq!(once.status.code(), Some(0), "{}", text(&once.stderr));
  assert_eq!(text(&once.stdout).trim(), "INSIDE");

  let stop = Arc::new(AtomicBool::new(false));
  let swapping = Arc::clone(&stop);
  let link_swap = link.clone();
  let inside_swap = inside.clone();
  let secret_swap = secret.clone();
  let swapper = std::thread::spawn(move || {
    let mut to_outside = true;
    while !swapping.load(Ordering::Relaxed) {
      let target = if to_outside {
        &secret_swap
      } else {
        &inside_swap
      };
      let temporary = link_swap.with_extension("tmp");
      let _ = fs::remove_file(&temporary);
      if std::os::unix::fs::symlink(target, &temporary).is_ok() {
        let _ = fs::rename(&temporary, &link_swap);
      }
      to_outside = !to_outside;
    }
  });
  write(
    &proj.join("race.ts"),
    r#"for (let i = 0; i < 40; i++) {
  try {
    console.log("R " + JSON.stringify(Yuzuko.readTextFileSync("link")));
  } catch (error) {
    console.log("R " + error.name);
  }
  try {
    Yuzuko.writeTextFileSync("link", "NEW\n");
    console.log("W ok");
  } catch (error) {
    console.log("W " + error.name);
  }
}
"#,
  );
  let raced = run(
    &proj,
    &[
      "run",
      "--no-prompt",
      "--allow-read=.",
      "--allow-write=.",
      "race.ts",
    ],
  );
  stop.store(true, Ordering::Relaxed);
  let _ = swapper.join();
  assert_eq!(raced.status.code(), Some(0), "{}", text(&raced.stderr));
  let stdout = text(&raced.stdout);
  assert!(!stdout.contains("OUTSIDE"), "{stdout}");
  assert_eq!(fs::read_to_string(&secret).unwrap(), "OUTSIDE\n");
}

#[cfg(unix)]
#[test]
fn real_path_does_not_return_a_path_outside_the_grant() {
  use std::sync::Arc;
  use std::sync::atomic::{AtomicBool, Ordering};

  let root = scratch("real-path");
  let proj = root.join("proj");
  let outside = root.join("outside");
  fs::create_dir_all(&proj).unwrap();
  fs::create_dir_all(&outside).unwrap();
  let inside = proj.join("inside.txt");
  let secret = outside.join("secret.txt");
  fs::write(&inside, "INSIDE\n").unwrap();
  fs::write(&secret, "OUTSIDE\n").unwrap();
  let link = proj.join("link");
  std::os::unix::fs::symlink(&inside, &link).unwrap();
  write(
    &proj.join("once.ts"),
    r#"console.log(Yuzuko.realPathSync("inside.txt"));
console.log(Yuzuko.realPathSync("link"));
"#,
  );
  let once = run(&proj, &["run", "--no-prompt", "--allow-read=.", "once.ts"]);
  assert_eq!(once.status.code(), Some(0), "{}", text(&once.stderr));
  let stdout = text(&once.stdout);
  assert!(stdout.contains("inside.txt"), "{stdout}");
  assert!(!stdout.contains("secret"), "{stdout}");

  std::os::unix::fs::symlink(&secret, proj.join("out")).unwrap();
  write(
    &proj.join("out.ts"),
    r#"try {
  console.log("PATH " + Yuzuko.realPathSync("out"));
} catch (error) {
  console.log("ERR " + error.name);
}
"#,
  );
  let denied = run(&proj, &["run", "--no-prompt", "--allow-read=.", "out.ts"]);
  assert_eq!(denied.status.code(), Some(0), "{}", text(&denied.stderr));
  let denied_out = text(&denied.stdout);
  assert!(denied_out.contains("ERR NotAllowedError"), "{denied_out}");
  assert!(!denied_out.contains("secret"), "{denied_out}");

  let stop = Arc::new(AtomicBool::new(false));
  let swapping = Arc::clone(&stop);
  let link_swap = link.clone();
  let inside_swap = inside.clone();
  let secret_swap = secret.clone();
  let swapper = std::thread::spawn(move || {
    let mut to_outside = true;
    while !swapping.load(Ordering::Relaxed) {
      let target = if to_outside {
        &secret_swap
      } else {
        &inside_swap
      };
      let temporary = link_swap.with_extension("tmp");
      let _ = fs::remove_file(&temporary);
      if std::os::unix::fs::symlink(target, &temporary).is_ok() {
        let _ = fs::rename(&temporary, &link_swap);
      }
      to_outside = !to_outside;
    }
  });
  write(
    &proj.join("race.ts"),
    r#"for (let i = 0; i < 40; i++) {
  try {
    console.log("P " + Yuzuko.realPathSync("link"));
  } catch (error) {
    console.log("P " + error.name);
  }
}
"#,
  );
  let raced = run(&proj, &["run", "--no-prompt", "--allow-read=.", "race.ts"]);
  stop.store(true, Ordering::Relaxed);
  let _ = swapper.join();
  assert_eq!(raced.status.code(), Some(0), "{}", text(&raced.stderr));
  let raced_out = text(&raced.stdout);
  assert!(!raced_out.contains("secret"), "{raced_out}");
}

#[cfg(feature = "ffi")]
#[test]
fn a_bad_symbol_is_rejected_before_the_ffi_grant() {
  let dir = scratch("ffi-order");
  write(
    &dir.join("main.ts"),
    r#"async function probe(label, symbols) {
  try {
    await Yuzuko.dlopen("libunused", symbols);
    console.log(label + " opened");
  } catch (error) {
    console.log(label + " " + error.name + " " + error.message);
  }
}
await probe("bad", { nope: "nope" });
await probe("empty", {});
"#,
  );
  let output = run(&dir, &["run", "--no-prompt", "main.ts"]);
  assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
  let stdout = text(&output.stdout);
  assert!(
    stdout.contains("bad TypeError The symbol definition must be an object."),
    "{stdout}"
  );
  assert!(
    stdout.contains(
      "empty NotAllowedError Requires ffi access, run again with the --allow-ffi flag"
    ),
    "{stdout}"
  );
}

#[test]
fn allow_read_flag_permits_the_current_directory_only() {
  let root = scratch("flag");
  let work = root.join("work");
  fs::create_dir_all(&work).unwrap();
  fs::create_dir_all(root.join("other")).unwrap();
  write(&work.join("note.txt"), "hello\n");
  write(&root.join("other/secret.txt"), "secret\n");
  write(&work.join("main.ts"), READ_NOTE);
  let work = fs::canonicalize(&work).unwrap();
  let output = run(&work, &["run", "--allow-read=.", "main.ts"]);
  assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
  assert_eq!(text(&output.stdout), "hello\nNotAllowedError\n");
}

#[test]
fn deny_wins_over_allow_all() {
  let dir = scratch("deny-all");
  write(&dir.join("note.txt"), "hello\n");
  write(
    &dir.join("main.ts"),
    r#"try {
  console.log(await Yuzuko.readTextFile("note.txt"));
} catch (error) {
  console.log(error.name);
}
"#,
  );
  let output = run(&dir, &["run", "-A", "--deny-read", "main.ts"]);
  assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
  assert_eq!(text(&output.stdout), "NotAllowedError\n");
}

#[test]
fn config_paths_are_relative_to_the_config_directory() {
  let root = scratch("config");
  let work = root.join("work");
  fs::create_dir_all(root.join("other")).unwrap();
  write(&work.join("note.txt"), "hello\n");
  write(&root.join("other/secret.txt"), "secret\n");
  write(
    &work.join("yuzuko.json"),
    r#"{ "name": "Scoped", "permissions": { "read": ["./note.txt"], "env": false } }
"#,
  );
  write(
    &work.join("main.ts"),
    r#"let envName = "env-ok";
try {
  Yuzuko.env.get("PATH");
} catch (error) {
  envName = error.name;
}
const text = await Yuzuko.readTextFile("note.txt");
let outside = "blocked";
try {
  await Yuzuko.readTextFile("../other/secret.txt");
  outside = "leaked";
} catch (error) {
  outside = error.name;
}
console.log(text.trim());
console.log(outside);
console.log(envName);
"#,
  );
  let work = fs::canonicalize(&work).unwrap();
  let output = run(&work, &["run", "main.ts"]);
  assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
  assert_eq!(
    text(&output.stdout),
    "hello\nNotAllowedError\nNotAllowedError\n"
  );
}

#[test]
fn request_without_a_terminal_leaves_the_query_at_prompt() {
  let dir = scratch("prompt");
  write(&dir.join("note.txt"), "hello\n");
  write(
    &dir.join("main.ts"),
    r#"const before = Yuzuko.permissions.querySync({ name: "read", path: "note.txt" });
const asked = Yuzuko.permissions.requestSync({ name: "read", path: "note.txt" });
const after = Yuzuko.permissions.querySync({ name: "read", path: "note.txt" });
const broad = Yuzuko.permissions.querySync({ name: "read" });
let bad = "ok";
try {
  Yuzuko.permissions.querySync({ name: "read", path: 1 });
} catch (error) {
  bad = error.name;
}
console.log(before.state);
console.log(asked.state);
console.log(after.state);
console.log(broad.state);
console.log(bad);
"#,
  );
  let quiet = run(&dir, &["run", "--no-prompt", "main.ts"]);
  assert_eq!(quiet.status.code(), Some(0), "{}", text(&quiet.stderr));
  assert_eq!(
    text(&quiet.stdout),
    "prompt\ndenied\nprompt\nprompt\nTypeError\n"
  );
  let granted = run(&dir, &["run", "-A", "--no-prompt", "main.ts"]);
  assert_eq!(granted.status.code(), Some(0), "{}", text(&granted.stderr));
  assert_eq!(
    text(&granted.stdout),
    "granted\ngranted\ngranted\ngranted\nTypeError\n"
  );
}

#[test]
fn a_worker_uses_the_process_policy() {
  let dir = scratch("worker");
  write(&dir.join("note.txt"), "hello\n");
  write(
    &dir.join("worker.ts"),
    r#"try {
  await Yuzuko.readTextFile("note.txt");
  postMessage("allowed");
} catch (error) {
  postMessage(error.name);
}
self.close();
"#,
  );
  write(
    &dir.join("main.ts"),
    r#"const worker = new Worker(new URL("./worker.ts", import.meta.url), { type: "module" });
const reply = await new Promise((resolve) => {
  worker.onmessage = (event) => resolve(event.data);
});
console.log(reply);
worker.terminate();
"#,
  );
  let output = run(&dir, &["run", "--allow-read=worker.ts", "main.ts"]);
  assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
  assert_eq!(text(&output.stdout), "NotAllowedError\n");
}

#[test]
fn repl_accepts_permission_flags_only() {
  let dir = scratch("repl");
  let extra = run(&dir, &["repl", "extra"]);
  assert_eq!(extra.status.code(), Some(2), "{}", text(&extra.stderr));
  assert!(
    text(&extra.stderr).contains("yuzuko repl"),
    "{}",
    text(&extra.stderr)
  );
  let allowed = run(&dir, &["repl", "--allow-env", "--no-prompt"]);
  assert_eq!(allowed.status.code(), Some(0), "{}", text(&allowed.stderr));
}

#[test]
fn test_starts_closed_and_an_allow_does_not_grant_the_rest() {
  let root = scratch("test");
  let work = root.join("work");
  fs::create_dir_all(&work).unwrap();
  fs::create_dir_all(root.join("other")).unwrap();
  write(&work.join("note.txt"), "hello\n");
  write(&root.join("other/secret.txt"), "hidden-oracle\n");
  write(
    &work.join("yuzuko.json"),
    "{ \"permissions\": { \"read\": true } }\n",
  );
  write(
    &work.join("read_test.ts"),
    r#"Yuzuko.test("reads a file", async () => {
  const text = await Yuzuko.readTextFile("note.txt");
  if (text !== "hello\n") {
    throw new Error(text);
  }
});
"#,
  );
  write(
    &work.join("outside_test.ts"),
    r#"Yuzuko.test("stays inside the grant", async () => {
  const text = await Yuzuko.readTextFile("note.txt");
  if (text !== "hello\n") {
    throw new Error(text);
  }
  try {
    await Yuzuko.readTextFile("../other/secret.txt");
    throw new Error("leaked");
  } catch (error) {
    if (error.name !== "NotAllowedError") {
      throw error;
    }
  }
});
"#,
  );
  let work = fs::canonicalize(&work).unwrap();
  let closed = run(&work, &["test", "read_test.ts"]);
  assert_ne!(closed.status.code(), Some(0));
  let combined = format!("{}{}", text(&closed.stdout), text(&closed.stderr));
  assert!(combined.contains("NotAllowedError"), "{combined}");
  assert!(!combined.contains("hidden-oracle"), "{combined}");
  let granted = run(&work, &["test", "--allow-read=.", "read_test.ts"]);
  assert_eq!(granted.status.code(), Some(0), "{}", text(&granted.stderr));
  assert!(
    text(&granted.stdout).contains("1 passed"),
    "{}",
    text(&granted.stdout)
  );
  let net_only = run(&work, &["test", "--allow-net=127.0.0.1", "read_test.ts"]);
  assert_ne!(net_only.status.code(), Some(0));
  let combined =
    format!("{}{}", text(&net_only.stdout), text(&net_only.stderr));
  assert!(combined.contains("NotAllowedError"), "{combined}");
  let all = run(&work, &["test", "--allow-all", "read_test.ts"]);
  assert_eq!(all.status.code(), Some(0), "{}", text(&all.stderr));
  let denied = run(
    &work,
    &["test", "--allow-all", "--deny-read", "read_test.ts"],
  );
  assert_ne!(denied.status.code(), Some(0));
  let combined = format!("{}{}", text(&denied.stdout), text(&denied.stderr));
  assert!(combined.contains("NotAllowedError"), "{combined}");
  let outside = run(&work, &["test", "--allow-read=.", "outside_test.ts"]);
  assert_eq!(outside.status.code(), Some(0), "{}", text(&outside.stderr));
  let outside_text =
    format!("{}{}", text(&outside.stdout), text(&outside.stderr));
  assert!(outside_text.contains("1 passed"), "{outside_text}");
  assert!(!outside_text.contains("hidden-oracle"), "{outside_text}");
}

#[test]
fn an_unknown_permission_is_rejected() {
  let dir = scratch("unknown");
  write(&dir.join("main.ts"), "console.log(1)\n");
  write(
    &dir.join("yuzuko.json"),
    "{ \"permissions\": { \"sys\": true } }\n",
  );
  let output = run(&dir, &["run", "main.ts"]);
  assert_eq!(output.status.code(), Some(1), "{}", text(&output.stdout));
  assert!(
    text(&output.stderr).contains("permissions"),
    "{}",
    text(&output.stderr)
  );
  let flag = run(&dir, &["run", "--allow-foo", "main.ts"]);
  assert_eq!(flag.status.code(), Some(2));
  assert!(
    text(&flag.stderr).contains("unknown flag --allow-foo"),
    "{}",
    text(&flag.stderr)
  );
  let empty = run(&dir, &["run", "--allow-read=", "main.ts"]);
  assert_eq!(empty.status.code(), Some(2));
  assert!(
    text(&empty.stderr).contains("requires a value"),
    "{}",
    text(&empty.stderr)
  );
}

#[test]
fn package_writes_permissions_and_enforces_them() {
  let dir = scratch("package");
  write(
    &dir.join("main.ts"),
    r#"let readName = "read-ok";
try {
  await Yuzuko.readTextFile("outside.txt");
} catch (error) {
  readName = error.name;
}
let pathName = "path-ok";
try {
  Yuzuko.env.get("PATH");
} catch (error) {
  pathName = error.name;
}
let homeName = "home-ok";
try {
  Yuzuko.env.get("YUZUKO_PERMISSION_ABSENT");
} catch (error) {
  homeName = error.name;
}
console.log(readName);
console.log(pathName);
console.log(homeName);
"#,
  );
  write(
    &dir.join("yuzuko.json"),
    r#"{
  "name": "Perm",
  "permissions": { "read": false, "env": ["PATH"] }
}
"#,
  );
  let bundle = if cfg!(target_os = "macos") {
    dir.join("Perm.app")
  } else {
    dir.join("Perm")
  };
  let packaged = run(
    &dir,
    &["package", "main.ts", "--output", bundle.to_str().unwrap()],
  );
  assert_eq!(
    packaged.status.code(),
    Some(0),
    "{}",
    text(&packaged.stderr)
  );
  let manifest = if cfg!(target_os = "macos") {
    bundle.join("Contents/Resources/yuzuko.bundle.json")
  } else {
    bundle.join("yuzuko.bundle.json")
  };
  let manifest = fs::read_to_string(&manifest).unwrap();
  let manifest: serde_json::Value = serde_json::from_str(&manifest).unwrap();
  assert_eq!(manifest["permissions"]["read"], serde_json::json!(false));
  assert_eq!(manifest["permissions"]["env"][0], "PATH");
  let exe = if cfg!(target_os = "macos") {
    bundle.join("Contents/MacOS/perm")
  } else if cfg!(windows) {
    bundle.join("perm.exe")
  } else {
    bundle.join("perm")
  };
  let mut child = Command::new(&exe)
    .current_dir(&dir)
    .stdin(Stdio::null())
    .env_remove("YUZUKO_HEADLESS")
    .env_remove("YUZUKO_SKIP_GPU")
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .spawn()
    .expect("spawn bundle");
  let start = Instant::now();
  let output = loop {
    if child.try_wait().unwrap().is_some() {
      break child.wait_with_output().unwrap();
    }
    if start.elapsed() > Duration::from_secs(30) {
      let _ = child.kill();
      panic!("bundle did not exit");
    }
    std::thread::sleep(Duration::from_millis(50));
  };
  assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
  assert_eq!(
    text(&output.stdout),
    "NotAllowedError\npath-ok\nNotAllowedError\n"
  );
}

#[test]
fn raw_module_imports_require_read() {
  let dir = scratch("raw-import");
  write(&dir.join("lib.ts"), "export const marker = \"lib-ok\";\n");
  write(&dir.join("secret.txt"), "CANARY-SECRET\n");
  write(&dir.join("secret.json"), "{\"token\":\"CANARY-JSON\"}\n");
  write(&dir.join("trick.js"), "export const marker = \"ran\";\n");
  write(
    &dir.join("main.ts"),
    r#"import { marker } from "./lib.ts";
console.log(marker);
let readName = "read-ok";
try {
  await Yuzuko.readTextFile("secret.txt");
} catch (error) {
  readName = error.name;
}
console.log("read:" + readName);
async function load(type, url) {
  try {
    await import(url, { with: { type } });
    console.log(type + ":loaded");
  } catch (error) {
    console.log(type + ":" + error.name);
  }
}
await load("text", "./secret.txt");
await load("json", "./secret.json");
await load("bytes", "./secret.txt");
await load("json", "./trick.js");
"#,
  );
  let denied = run(&dir, &["run", "--no-prompt", "main.ts"]);
  assert_eq!(denied.status.code(), Some(0), "{}", text(&denied.stderr));
  assert_eq!(
    text(&denied.stdout),
    "lib-ok\nread:NotAllowedError\ntext:NotAllowedError\njson:NotAllowedError\nbytes:NotAllowedError\njson:NotAllowedError\n"
  );

  write(
    &dir.join("allow.ts"),
    r#"import { marker } from "./lib.ts";
console.log(marker);
const text = await import("./secret.txt", { with: { type: "text" } });
console.log("text:" + JSON.stringify(text.default));
const json = await import("./secret.json", { with: { type: "json" } });
console.log("json:" + JSON.stringify(json.default));
const bytes = await import("./secret.txt", { with: { type: "bytes" } });
console.log("bytes:" + JSON.stringify(new TextDecoder().decode(bytes.default)));
"#,
  );
  let allowed = run(
    &dir,
    &[
      "run",
      "--no-prompt",
      "--allow-read=secret.txt,secret.json",
      "allow.ts",
    ],
  );
  assert_eq!(allowed.status.code(), Some(0), "{}", text(&allowed.stderr));
  assert_eq!(
    text(&allowed.stdout),
    "lib-ok\ntext:\"CANARY-SECRET\\n\"\njson:{\"token\":\"CANARY-JSON\"}\nbytes:\"CANARY-SECRET\\n\"\n"
  );
}

#[test]
fn a_file_worker_requires_read() {
  let dir = scratch("file-worker");
  write(&dir.join("secret.txt"), "postMessage(\"LEAKED\");\n");
  write(
    &dir.join("main.ts"),
    r#"const worker = new Worker(new URL("./secret.txt", import.meta.url));
const result = await new Promise((resolve) => {
  worker.onmessage = (event) => resolve("msg:" + event.data);
  worker.onerror = (event) => resolve("err:" + event.message);
});
console.log(result);
worker.terminate();
"#,
  );
  let output = run(&dir, &["run", "--no-prompt", "main.ts"]);
  assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
  let stdout = text(&output.stdout);
  assert!(
    stdout.starts_with("err:NotAllowedError:"),
    "{stdout}\n{}",
    text(&output.stderr)
  );
  assert!(!stdout.contains("LEAKED"), "{stdout}");
}

#[test]
fn a_sandboxed_http_worker_requires_read() {
  let dir = scratch("http-worker");
  let outside = scratch("http-outside");
  write(
    &dir.join("ident.env"),
    "API_KEY=sk_live_CanarySecretValue\n",
  );
  write(
    &outside.join("ident.env"),
    "API_KEY=sk_live_CanaryOutsideValue\n",
  );
  write(&dir.join("worker.js"), "postMessage(\"ran\");\n");
  let linked = {
    #[cfg(unix)]
    {
      std::os::unix::fs::symlink(
        outside.join("ident.env"),
        dir.join("linked.js"),
      )
      .is_ok()
    }
    #[cfg(not(unix))]
    {
      false
    }
  };
  let link_probe = if linked {
    "await probe(\"link\", \"http://127.0.0.1/linked.js\");\n"
  } else {
    ""
  };
  write(
    &dir.join("main.ts"),
    &format!(
      r#"async function probe(label, url) {{
  const worker = new Worker(url);
  const result = await new Promise((resolve) => {{
    worker.onmessage = (event) => resolve("msg " + event.data);
    worker.onerror = (event) => resolve("err " + String(event.message).replaceAll("\n", " | "));
    setTimeout(() => resolve("timeout"), 1500);
  }});
  console.log(label + " " + result);
  worker.terminate();
}}
await probe("loop", "http://127.0.0.1/ident.env");
await probe("localhost", "http://localhost/ident.env");
await probe("wpt", "http://web-platform.test/ident.env");
await probe("port", "http://127.0.0.1:9/ident.env");
await probe("https", "https://127.0.0.1/ident.env");
await probe("ws", "ws://localhost/ident.env");
{link_probe}"#,
      link_probe = link_probe,
    ),
  );
  let denied = run(&dir, &["run", "--no-prompt", "main.ts"]);
  assert_eq!(denied.status.code(), Some(0), "{}", text(&denied.stderr));
  let stdout = text(&denied.stdout);
  for label in ["loop", "localhost", "wpt", "port", "https", "ws"] {
    assert!(
      stdout
        .lines()
        .any(|line| line.starts_with(&format!("{label} err NotAllowedError:"))),
      "missing {label}\n{stdout}\n{}",
      text(&denied.stderr)
    );
  }
  if linked {
    assert!(
      stdout
        .lines()
        .any(|line| line.starts_with("link err NotAllowedError:")),
      "{stdout}"
    );
  }
  assert!(!stdout.contains("sk_live"), "{stdout}");
  assert!(!stdout.contains("ran"), "{stdout}");

  write(
    &dir.join("main.ts"),
    &format!(
      r#"async function probe(label, url) {{
  const worker = new Worker(url);
  const result = await new Promise((resolve) => {{
    worker.onmessage = (event) => resolve("msg " + event.data);
    worker.onerror = (event) => resolve("err " + String(event.message).replaceAll("\n", " | "));
    setTimeout(() => resolve("timeout"), 1500);
  }});
  console.log(label + " " + result);
  worker.terminate();
}}
await probe("worker", "http://127.0.0.1/worker.js");
{link_probe}"#,
      link_probe = link_probe,
    ),
  );
  let allowed = run(&dir, &["run", "--no-prompt", "--allow-read=.", "main.ts"]);
  assert_eq!(allowed.status.code(), Some(0), "{}", text(&allowed.stderr));
  let stdout = text(&allowed.stdout);
  assert!(
    stdout.lines().any(|line| line == "worker msg ran"),
    "{stdout}\n{}",
    text(&allowed.stderr)
  );
  if linked {
    assert!(
      stdout
        .lines()
        .any(|line| line.starts_with("link err NotAllowedError:")),
      "{stdout}"
    );
    assert!(!stdout.contains("sk_live"), "{stdout}");
  }
}

#[test]
fn import_scripts_requires_read() {
  let dir = scratch("import-scripts");
  write(&dir.join("secret.txt"), "postMessage(\"LEAKED\");\n");
  write(
    &dir.join("worker.js"),
    r#"try {
  importScripts(new URL("./secret.txt", location.href).href);
  postMessage("loaded");
} catch (error) {
  postMessage(error.name + ":" + (error instanceof DOMException));
}
"#,
  );
  write(
    &dir.join("main.ts"),
    r#"const worker = new Worker(new URL("./worker.js", import.meta.url));
const reply = await new Promise((resolve) => {
  worker.onmessage = (event) => resolve(event.data);
  worker.onerror = (event) => resolve("err:" + event.message);
});
console.log(reply);
worker.terminate();
"#,
  );
  let output = run(
    &dir,
    &["run", "--no-prompt", "--allow-read=worker.js", "main.ts"],
  );
  assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
  assert_eq!(text(&output.stdout), "NotAllowedError:true\n");
}

#[cfg(feature = "fetch")]
fn serve_until(
  listener: std::net::TcpListener,
  response: &[u8],
  stop: &Arc<AtomicBool>,
) -> bool {
  listener.set_nonblocking(true).unwrap();
  let start = Instant::now();
  let mut accepted = None;
  while start.elapsed() < Duration::from_secs(15)
    && !stop.load(Ordering::Relaxed)
  {
    match listener.accept() {
      Ok((stream, _)) => {
        accepted = Some(stream);
        break;
      }
      Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
        std::thread::sleep(Duration::from_millis(20));
      }
      Err(_) => return false,
    }
  }
  let Some(mut stream) = accepted else {
    return false;
  };
  stream.set_nonblocking(false).ok();
  let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
  let mut buf = Vec::new();
  let mut tmp = [0u8; 1024];
  loop {
    match stream.read(&mut tmp) {
      Ok(0) => break,
      Ok(count) => {
        buf.extend_from_slice(&tmp[..count]);
        if buf.windows(4).any(|window| window == b"\r\n\r\n") {
          break;
        }
      }
      Err(_) => break,
    }
  }
  let _ = stream.write_all(response);
  let _ = stream.flush();
  true
}

#[cfg(feature = "fetch")]
#[test]
fn a_redirect_is_checked_before_the_next_connection() {
  let secret = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
  let bounce = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
  let secret_port = secret.local_addr().unwrap().port();
  let bounce_port = bounce.local_addr().unwrap().port();
  let stop = Arc::new(AtomicBool::new(false));
  let secret_stop = Arc::clone(&stop);
  let secret_hit = Arc::new(AtomicBool::new(false));
  let secret_flag = Arc::clone(&secret_hit);
  let secret_thread = std::thread::spawn(move || {
    let body = b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\nConnection: close\r\n\r\nCANARY-NET";
    if serve_until(secret, body, &secret_stop) {
      secret_flag.store(true, Ordering::Relaxed);
    }
  });
  let bounce_stop = Arc::clone(&stop);
  let bounce_thread = std::thread::spawn(move || {
    let location = format!("http://127.0.0.1:{secret_port}/secret");
    let response = format!(
      "HTTP/1.1 302 Found\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
    );
    let _ = serve_until(bounce, response.as_bytes(), &bounce_stop);
  });
  let dir = scratch("redirect");
  write(
    &dir.join("main.ts"),
    r#"const [secret, bounce] = Yuzuko.args;
async function probe(url) {
  try {
    const response = await fetch(url);
    return "body:" + await response.text();
  } catch (error) {
    return error.name;
  }
}
console.log(await probe(secret));
console.log(await probe(bounce));
"#,
  );
  let allow = format!("--allow-net=127.0.0.1:{bounce_port}");
  let secret_url = format!("http://127.0.0.1:{secret_port}/secret");
  let bounce_url = format!("http://127.0.0.1:{bounce_port}/");
  let output = run(
    &dir,
    &[
      "run",
      "--no-prompt",
      allow.as_str(),
      "main.ts",
      secret_url.as_str(),
      bounce_url.as_str(),
    ],
  );
  stop.store(true, Ordering::Relaxed);
  let _ = secret_thread.join();
  let _ = bounce_thread.join();
  assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
  assert_eq!(
    text(&output.stdout),
    "NotAllowedError\nNotAllowedError\n",
    "stdout:\n{}\nstderr:\n{}",
    text(&output.stdout),
    text(&output.stderr)
  );
  assert!(
    !secret_hit.load(Ordering::Relaxed),
    "the denied hop was contacted"
  );
}

#[cfg(feature = "child")]
#[test]
fn spawn_requires_the_whole_environment_and_a_path_grant() {
  let dir = scratch("spawn");
  write(
    &dir.join("deny-env.ts"),
    r#"try {
  await Yuzuko.spawn("printenv", { args: ["YUZUKO_PERM_CANARY"] });
  console.log("started");
} catch (error) {
  console.log(error.name + ":" + error.message);
}
"#,
  );
  let denied = run(
    &dir,
    &["run", "--no-prompt", "--allow-run=printenv", "deny-env.ts"],
  );
  assert_eq!(denied.status.code(), Some(0), "{}", text(&denied.stderr));
  let stdout = text(&denied.stdout);
  assert!(stdout.contains("NotAllowedError:"), "{stdout}");
  assert!(stdout.contains("--allow-env"), "{stdout}");
  assert!(!stdout.contains("started"), "{stdout}");
  assert!(!stdout.contains("could not be started"), "{stdout}");

  let partial = run(
    &dir,
    &[
      "run",
      "--no-prompt",
      "--allow-run=printenv",
      "--allow-env=PATH",
      "deny-env.ts",
    ],
  );
  assert_eq!(partial.status.code(), Some(0), "{}", text(&partial.stderr));
  let stdout = text(&partial.stdout);
  assert!(stdout.contains("--allow-env"), "{stdout}");
  assert!(!stdout.contains("started"), "{stdout}");

  let program = dir.join(if cfg!(windows) {
    "printenv.exe"
  } else {
    "printenv"
  });
  write(
    &dir.join("deny-path.ts"),
    r#"try {
  await Yuzuko.spawn(Yuzuko.args[0]);
  console.log("started");
} catch (error) {
  console.log(error.name + ":" + error.message);
}
"#,
  );
  let path_denied = run(
    &dir,
    &[
      "run",
      "--no-prompt",
      "--allow-run=printenv",
      "deny-path.ts",
      program.to_str().unwrap(),
    ],
  );
  assert_eq!(
    path_denied.status.code(),
    Some(0),
    "{}",
    text(&path_denied.stderr)
  );
  let stdout = text(&path_denied.stdout);
  assert!(stdout.contains("Requires run access"), "{stdout}");
  assert!(!stdout.contains("could not be started"), "{stdout}");
  assert!(!stdout.contains("started"), "{stdout}");

  let (grant, script) = if cfg!(windows) {
    (
      "--allow-run=cmd",
      r#"const child = Yuzuko.spawn("cmd", { args: ["/d", "/c", "set", "YUZUKO_PERM_CANARY"] });
const output = await child.output();
console.log(new TextDecoder().decode(output.stdout));
"#,
    )
  } else {
    (
      "--allow-run=printenv",
      r#"const child = Yuzuko.spawn("printenv", { args: ["YUZUKO_PERM_CANARY"] });
const output = await child.output();
console.log(new TextDecoder().decode(output.stdout));
"#,
    )
  };
  write(&dir.join("allow.ts"), script);
  let allowed = run_env(
    &dir,
    &["run", "--no-prompt", grant, "--allow-env", "allow.ts"],
    &[("YUZUKO_PERM_CANARY", "CANARY-ENV")],
  );
  assert_eq!(allowed.status.code(), Some(0), "{}", text(&allowed.stderr));
  let stdout = text(&allowed.stdout);
  assert!(
    stdout.contains("YUZUKO_PERM_CANARY=CANARY-ENV")
      || stdout.contains("CANARY-ENV"),
    "{stdout}\n{}",
    text(&allowed.stderr)
  );
}

#[test]
fn an_async_file_call_keeps_the_path_from_the_check() {
  let parent = scratch("chdir-race");
  let proj = parent.join("proj");
  fs::create_dir_all(proj.join("sub")).unwrap();
  write(&parent.join("secret.txt"), "OUTSIDE\n");
  write(&parent.join("note.txt"), "OLD\n");
  write(&proj.join("secret.txt"), "INSIDE\n");
  write(&proj.join("note.txt"), "OLD\n");
  write(
    &proj.join("main.ts"),
    r#"for (let i = 0; i < 20; i += 1) {
  Yuzuko.chdir("sub");
  const pending = Yuzuko.readTextFile("../secret.txt");
  Yuzuko.chdir("..");
  const found = (await pending).trim();
  if (found !== "INSIDE") {
    console.log("LEAK " + found);
    break;
  }
}
console.log("read INSIDE");
for (let i = 0; i < 20; i += 1) {
  Yuzuko.chdir("sub");
  const pending = Yuzuko.writeTextFile("../note.txt", "NEW");
  Yuzuko.chdir("..");
  await pending;
}
console.log("wrote");
"#,
  );
  let output = run(
    &proj,
    &[
      "run",
      "--no-prompt",
      "--allow-read=.",
      "--allow-write=.",
      "main.ts",
    ],
  );
  let stdout = text(&output.stdout);
  let stderr = text(&output.stderr);
  assert_eq!(output.status.code(), Some(0), "{stderr}\n{stdout}");
  assert!(stdout.contains("read INSIDE"), "{stdout}\n{stderr}");
  assert!(stdout.contains("wrote"), "{stdout}\n{stderr}");
  assert!(!stdout.contains("LEAK"), "{stdout}");
  assert!(!stdout.contains("OUTSIDE"), "{stdout}");
  assert_eq!(fs::read_to_string(proj.join("note.txt")).unwrap(), "NEW");
  assert_eq!(
    fs::read_to_string(parent.join("note.txt")).unwrap(),
    "OLD\n"
  );
  let _ = fs::remove_dir_all(&parent);
}

#[cfg(feature = "child")]
#[test]
fn a_scoped_run_rejects_a_loader_variable() {
  let dir = scratch("run-loader");
  let (command, args) = if cfg!(windows) {
    ("cmd", r#"["/d", "/c", "echo SAFE"]"#)
  } else {
    ("echo", r#"["SAFE"]"#)
  };
  let grant = format!("--allow-run={command}");
  write(
    &dir.join("main.ts"),
    &format!(
      r#"async function probe(label, env) {{
  try {{
    const child = Yuzuko.spawn({command:?}, {{ args: {args}, env }});
    const output = await child.output();
    console.log(label + " OK " + new TextDecoder().decode(output.stdout).trim());
  }} catch (error) {{
    console.log(label + " " + error.name + " " + error.message);
  }}
}}
await probe("plain", {{}});
await probe("preload", {{ LD_PRELOAD: "x" }});
await probe("dyld", {{ DYLD_INSERT_LIBRARIES: "x" }});
await probe("lower", {{ ld_preload: "x" }});
Yuzuko.env.set("LD_PRELOAD", "x");
await probe("inherited", {{}});
Yuzuko.env.delete("LD_PRELOAD");
await probe("restored", {{}});
Yuzuko.env.set("DYLD_INSERT_LIBRARIES", "x");
await probe("removed", {{ DYLD_INSERT_LIBRARIES: null }});
"#,
      command = command,
      args = args,
    ),
  );
  let output = run(
    &dir,
    &[
      "run",
      "--no-prompt",
      "--allow-env",
      grant.as_str(),
      "main.ts",
    ],
  );
  let stdout = text(&output.stdout);
  let stderr = text(&output.stderr);
  assert_eq!(output.status.code(), Some(0), "{stderr}\n{stdout}");
  for label in ["plain", "restored", "removed"] {
    assert!(
      stdout
        .lines()
        .any(|line| line == format!("{label} OK SAFE")),
      "missing {label}\n{stdout}\n{stderr}"
    );
  }
  let denied =
    "NotAllowedError Requires run access, run again with the --allow-run flag";
  for label in ["preload", "dyld", "lower", "inherited"] {
    assert!(
      stdout
        .lines()
        .any(|line| line == format!("{label} {denied}")),
      "missing {label}\n{stdout}\n{stderr}"
    );
  }
  assert!(!stdout.contains("PWNED"), "{stdout}");

  write(
    &dir.join("whole.ts"),
    &format!(
      r#"try {{
  const child = Yuzuko.spawn({command:?}, {{
    args: {args},
    env: {{ LD_PRELOAD: "", DYLD_INSERT_LIBRARIES: "" }},
  }});
  const output = await child.output();
  console.log("whole OK " + new TextDecoder().decode(output.stdout).trim());
}} catch (error) {{
  console.log("whole " + error.name + " " + error.message);
}}
"#,
      command = command,
      args = args,
    ),
  );
  let whole = run(
    &dir,
    &[
      "run",
      "--no-prompt",
      "--allow-run",
      "--allow-env",
      "whole.ts",
    ],
  );
  let stdout = text(&whole.stdout);
  let stderr = text(&whole.stderr);
  assert_eq!(whole.status.code(), Some(0), "{stderr}\n{stdout}");
  assert!(
    stdout.lines().any(|line| line == "whole OK SAFE"),
    "{stdout}\n{stderr}"
  );
  let _ = fs::remove_dir_all(&dir);
}

#[cfg(all(unix, feature = "child"))]
#[test]
fn a_bare_run_grant_stays_on_the_startup_executable() {
  use std::os::unix::fs::PermissionsExt;
  let dir = scratch("run-path");
  let evil = dir.join("evil-bin");
  fs::create_dir_all(&evil).unwrap();
  let fake = evil.join("echo");
  write(&fake, "#!/bin/sh\necho PWNED\n");
  let mut permissions = fs::metadata(&fake).unwrap().permissions();
  permissions.set_mode(0o755);
  fs::set_permissions(&fake, permissions).unwrap();
  let evil_js = serde_json::to_string(&evil.display().to_string()).unwrap();
  write(
    &dir.join("main.ts"),
    &format!(
      r#"async function probe(label, env) {{
  try {{
    const child = Yuzuko.spawn("echo", {{ args: ["SAFE"], env }});
    const output = await child.output();
    console.log(label + " OK " + new TextDecoder().decode(output.stdout).trim());
  }} catch (error) {{
    console.log(label + " " + error.name + " " + error.message);
  }}
}}
await probe("same", {{}});
await probe("swapped", {{ PATH: {evil_js} }});
Yuzuko.env.set("PATH", {evil_js});
await probe("assigned", {{}});
"#,
      evil_js = evil_js,
    ),
  );
  let output = run(
    &dir,
    &[
      "run",
      "--no-prompt",
      "--allow-env",
      "--allow-run=echo",
      "main.ts",
    ],
  );
  let stdout = text(&output.stdout);
  let stderr = text(&output.stderr);
  assert_eq!(output.status.code(), Some(0), "{stderr}\n{stdout}");
  assert!(
    stdout.lines().any(|line| line == "same OK SAFE"),
    "{stdout}\n{stderr}"
  );
  for label in ["swapped", "assigned"] {
    let line = stdout
      .lines()
      .find(|line| line.starts_with(label))
      .unwrap_or("");
    assert!(
      line.starts_with(&format!("{label} NotAllowedError")),
      "{line}\n{stdout}\n{stderr}"
    );
    assert!(line.contains("evil-bin"), "{line}");
    assert!(!line.contains("PWNED"), "{line}");
  }
  assert!(!stdout.contains("PWNED"), "{stdout}");
  assert!(!stderr.contains("PWNED"), "{stderr}");
  let _ = fs::remove_dir_all(&dir);
}

#[cfg(all(unix, feature = "child"))]
#[test]
fn a_path_run_rule_does_not_match_a_bare_command() {
  use std::os::unix::fs::PermissionsExt;
  let dir = scratch("run-bare");
  let tool = dir.join("tool");
  let evil = dir.join("evil-bin");
  fs::create_dir_all(&evil).unwrap();
  for (path, body) in [
    (&tool, "#!/bin/sh\necho REAL\n"),
    (&evil.join("tool"), "#!/bin/sh\necho PWNED\n"),
  ] {
    write(path, body);
    let mut permissions = fs::metadata(path).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).unwrap();
  }
  let allow = format!("--allow-run={}", tool.display());
  let evil_js = serde_json::to_string(&evil.display().to_string()).unwrap();
  write(
    &dir.join("main.ts"),
    &format!(
      r#"async function probe(label, run) {{
  try {{
    console.log(label + " OK " + (await run()));
  }} catch (error) {{
    console.log(label + " " + error.name + " " + error.message);
  }}
}}
const text = async (child) => new TextDecoder().decode((await child.output()).stdout).trim();
await probe("bare", async () => text(Yuzuko.spawn("tool", {{ env: {{ PATH: {evil_js} }} }})));
await probe("direct", async () => text(Yuzuko.spawn("./tool")));
"#,
      evil_js = evil_js,
    ),
  );
  let output = run(
    &dir,
    &[
      "run",
      "--no-prompt",
      "--allow-env",
      allow.as_str(),
      "main.ts",
    ],
  );
  let stdout = text(&output.stdout);
  let stderr = text(&output.stderr);
  assert_eq!(output.status.code(), Some(0), "{stderr}\n{stdout}");
  assert!(
    stdout.lines().any(|line| {
      line.starts_with("bare NotAllowedError")
        && line.contains("Requires run access to \"tool\"")
    }),
    "{stdout}\n{stderr}"
  );
  assert!(
    stdout.lines().any(|line| line == "direct OK REAL"),
    "{stdout}\n{stderr}"
  );
  assert!(!stdout.contains("PWNED"), "{stdout}");
  assert!(!stderr.contains("PWNED"), "{stderr}");
  let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_non_script_import_is_not_evaluated() {
  let dir = scratch("eval-import");
  let outside = scratch("eval-outside");
  write(&dir.join("lib.ts"), "export const marker = \"lib-ok\";\n");
  write(
    &dir.join("ident.env"),
    "API_KEY=sk_live_CanarySecretValue\n",
  );
  write(
    &dir.join("export.txt"),
    "export default \"CANARY-EXPORT\";\n",
  );
  write(
    &dir.join("credentials.JSON"),
    "export default \"CANARY-JSON-CASE\";\n",
  );
  write(
    &outside.join("ident.env"),
    "API_KEY=sk_live_CanaryOutsideValue\n",
  );
  write(
    &outside.join("export.txt"),
    "export default \"CANARY-OUTSIDE-EXPORT\";\n",
  );
  let linked = {
    #[cfg(unix)]
    {
      std::os::unix::fs::symlink(
        outside.join("ident.env"),
        dir.join("linked.ts"),
      )
      .is_ok()
    }
    #[cfg(not(unix))]
    {
      false
    }
  };
  let outside_export =
    url::Url::from_file_path(outside.join("export.txt")).unwrap();
  let url_literal = serde_json::to_string(&outside_export.to_string()).unwrap();
  let link_probe = if linked {
    "await probe(\"link\", () => import(\"./linked.ts\"));\n"
  } else {
    ""
  };
  write(
    &dir.join("worker.js"),
    r#"try {
  const loaded = await import("./export.txt");
  postMessage("LOADED " + JSON.stringify(loaded.default));
} catch (error) {
  postMessage(error.name);
}
"#,
  );
  write(
    &dir.join("main.ts"),
    &format!(
      r#"import {{ marker }} from "./lib.ts";
console.log(marker);
async function probe(label, run) {{
  try {{
    const value = await run();
    const exported = value && value.default;
    console.log(label + " LOADED " + JSON.stringify(exported));
  }} catch (error) {{
    console.log(label + " " + error.name);
  }}
}}
await probe("read", () => Yuzuko.readTextFile("ident.env"));
await probe("text", () => import("./ident.env", {{ with: {{ type: "text" }} }}));
await probe("css", () => import("./ident.env", {{ with: {{ type: "css" }} }}));
await probe("ident", () => import("./ident.env"));
await probe("export", () => import("./export.txt"));
await probe("json-case", () => import("./credentials.JSON"));
await probe("abs", () => import({url_literal}));
{link_probe}const worker = new Worker(new URL("./worker.js", import.meta.url), {{ type: "module" }});
const reply = await new Promise((resolve) => {{
  worker.onmessage = (event) => resolve(event.data);
  worker.onerror = (event) => resolve("err:" + event.message);
}});
console.log("worker " + reply);
worker.terminate();
"#,
      url_literal = url_literal,
      link_probe = link_probe,
    ),
  );
  let output = run(
    &dir,
    &["run", "--no-prompt", "--allow-read=worker.js", "main.ts"],
  );
  assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
  let stdout = text(&output.stdout);
  let expected = [
    "lib-ok",
    "read NotAllowedError",
    "text NotAllowedError",
    "css TypeError",
    "ident TypeError",
    "export TypeError",
    "json-case TypeError",
    "abs TypeError",
    "worker TypeError",
  ];
  for line in expected {
    assert!(
      stdout.lines().any(|item| item == line),
      "missing {line}\n{stdout}\n{}",
      text(&output.stderr)
    );
  }
  if linked {
    assert!(
      stdout.lines().any(|item| item == "link TypeError"),
      "{stdout}"
    );
  }
  for secret in [
    "sk_live",
    "CANARY-EXPORT",
    "CANARY-JSON-CASE",
    "CANARY-OUTSIDE",
    "LOADED",
  ] {
    assert!(!stdout.contains(secret), "{secret} in\n{stdout}");
  }
}

#[cfg(feature = "ffi")]
#[test]
fn a_directory_ffi_grant_does_not_search_for_a_library() {
  let dir = scratch("ffi-search");
  let bare = if cfg!(windows) {
    "kernel32.dll"
  } else if cfg!(target_os = "macos") {
    "libSystem.B.dylib"
  } else {
    "libc.so.6"
  };
  let absolute = if cfg!(windows) {
    r"C:\Windows\System32\kernel32.dll"
  } else {
    "/usr/lib/libSystem.B.dylib"
  };
  let bare_literal = serde_json::to_string(bare).unwrap();
  let absolute_literal = serde_json::to_string(absolute).unwrap();
  write(
    &dir.join("main.ts"),
    &format!(
      r#"async function probe(label, run) {{
  try {{
    await run();
    console.log(label + " OPENED");
  }} catch (error) {{
    console.log(label + " " + error.name + " " + error.message);
  }}
}}
await probe("env", () => Yuzuko.env.get("YUZUKO_PERM_CANARY"));
await probe("abs", () => Yuzuko.dlopen({absolute_literal}, {{}}));
await probe("bare", () => Yuzuko.dlopen({bare_literal}, {{}}));
await probe("rel", () => Yuzuko.dlopen("./no-such-library", {{}}));
"#,
      absolute_literal = absolute_literal,
      bare_literal = bare_literal,
    ),
  );
  let denied = run_env(
    &dir,
    &["run", "--no-prompt", "--allow-ffi=.", "main.ts"],
    &[("YUZUKO_PERM_CANARY", "CANARY-ENV")],
  );
  assert_eq!(denied.status.code(), Some(0), "{}", text(&denied.stderr));
  let stdout = text(&denied.stdout);
  assert!(
    stdout.lines().any(|line| line.starts_with(
      "env NotAllowedError Requires env access to \"YUZUKO_PERM_CANARY\""
    )),
    "{stdout}"
  );
  let shown_absolute = absolute.replace('\\', "\\\\");
  assert!(
    stdout.lines().any(|line| {
      line.starts_with("abs NotAllowedError Requires ffi access to \"")
        && line.contains(&shown_absolute)
    }),
    "{stdout}"
  );
  assert!(
    stdout.lines().any(|line| {
      line == "bare NotAllowedError Requires ffi access, run again with the --allow-ffi flag"
    }),
    "{stdout}"
  );
  assert!(
    stdout
      .lines()
      .any(|line| line == "rel Error The library could not be opened."),
    "{stdout}"
  );
  assert!(!stdout.contains("CANARY-ENV"), "{stdout}");
  assert!(!stdout.contains("OPENED"), "{stdout}");

  let allowed = run(&dir, &["run", "--no-prompt", "--allow-ffi", "main.ts"]);
  assert_eq!(allowed.status.code(), Some(0), "{}", text(&allowed.stderr));
  let stdout = text(&allowed.stdout);
  assert!(
    stdout.lines().any(|line| line == "bare OPENED"),
    "{stdout}\n{}",
    text(&allowed.stderr)
  );
}

#[cfg(unix)]
#[test]
fn a_dotdot_after_a_symlink_stays_inside_the_grant() {
  let entry = scratch("dotdot");
  let outside = scratch("yz-out-canary");
  fs::create_dir_all(outside.join("a").join("b")).unwrap();
  write(
    &outside.join("a").join("ident.env"),
    "API_KEY=sk_live_CanaryOutsideValue\n",
  );
  write(&outside.join("a").join("CANARY-DIR-ENTRY"), "listed\n");
  write(&entry.join("inside-root.txt"), "BENIGN-ROOT\n");
  write(
    &entry.join("keep").join("sub").join("ident.env"),
    "API_KEY=INSIDE-BENIGN\n",
  );
  write(
    &entry.join("real.ts"),
    "export const value = \"script-ok\";\n",
  );
  std::os::unix::fs::symlink(
    outside.join("a").join("b"),
    entry.join("keep").join("sub").join("link"),
  )
  .unwrap();
  std::os::unix::fs::symlink(
    outside.join("a").join("ident.env"),
    entry.join("linked.env"),
  )
  .unwrap();
  std::os::unix::fs::symlink(entry.join("real.ts"), entry.join("alias.ts"))
    .unwrap();
  std::os::unix::fs::symlink(
    Path::new("/usr/lib"),
    entry.join("keep").join("usr"),
  )
  .unwrap();
  std::os::unix::fs::symlink(outside.join("a"), entry.join("secrets")).unwrap();
  let inside_tool = entry.join("keep").join("sub").join("inside-tool.sh");
  let outside_tool = outside.join("a").join("inside-tool.sh");
  write(&inside_tool, "#!/bin/sh\necho INSIDE-TOOL\n");
  write(&outside_tool, "#!/bin/sh\necho CANARY-RUN\n");
  for tool in [&inside_tool, &outside_tool] {
    let mut permissions = fs::metadata(tool).unwrap().permissions();
    use std::os::unix::fs::PermissionsExt;
    permissions.set_mode(0o755);
    fs::set_permissions(tool, permissions).unwrap();
  }
  let run_tool = format!("--allow-run={}", inside_tool.display());
  let ffi_probe = if cfg!(feature = "ffi") {
    r#"
await probe("ffi", () => {
  const lib = Yuzuko.dlopen("keep/usr/../lib/libSystem.B.dylib", {
    symbols: { getenv: { parameters: ["cstring"], result: "cstring" } },
  });
  return lib.symbols.getenv("YUZUKO_PERM_CANARY");
});
await probe("ffi-direct", () => Yuzuko.dlopen("keep/usr/libSystem.B.dylib", {}));
"#
  } else {
    ""
  };
  let spawn_probe = if cfg!(feature = "child") {
    r#"
await probe("spawn", async () => {
  const child = Yuzuko.spawn("pwd", { cwd: "keep/sub/link/.." });
  const output = await child.output();
  return new TextDecoder().decode(output.stdout);
});
await probe("direct-run", async () => {
  const child = Yuzuko.spawn("keep/sub/inside-tool.sh");
  const output = await child.output();
  return new TextDecoder().decode(output.stdout).trim();
});
await probe("escape-run", async () => {
  const child = Yuzuko.spawn("keep/sub/link/../inside-tool.sh");
  const output = await child.output();
  return new TextDecoder().decode(output.stdout).trim();
});
"#
  } else {
    ""
  };
  write(
    &entry.join("main.ts"),
    &format!(
      r#"async function probe(label, run) {{
  try {{
    const value = await run();
    console.log(label + " OK " + value);
  }} catch (error) {{
    console.log(label + " " + error.name + " " + error.message);
  }}
}}
await probe("benign", async () => (await Yuzuko.readTextFile("keep/../inside-root.txt")).trim());
await probe("inside", async () => (await Yuzuko.readTextFile("keep/sub/ident.env")).trim());
await probe("escape", () => Yuzuko.readTextFile("keep/sub/link/../ident.env"));
await probe("bytes", () => Yuzuko.readFile("keep/sub/link/../ident.env"));
await probe("dir", () => Yuzuko.readDir("keep/sub/link/.."));
await probe("real", () => Yuzuko.realPath("keep/sub/link/../ident.env"));
await probe("stat", () => Yuzuko.stat("keep/sub/link/../ident.env"));
await probe("write", () => Yuzuko.writeTextFile("keep/sub/link/../pwned-canary.txt", "PWNED"));
await probe("copy", () => Yuzuko.copyFile("keep/sub/link/../ident.env", "keep/sub/link/../copied-canary.txt"));
await probe("chdir", () => {{
  Yuzuko.chdir("keep/sub/link/..");
  return Yuzuko.cwd();
}});
await probe("direct", () => Yuzuko.readTextFile("linked.env"));
await probe("linktext", () => Yuzuko.readLink("linked.env"));
await probe("import", () => import("./linked.env"));
await probe("import-text", () => import("./linked.env", {{ with: {{ type: "text" }} }}));
await probe("import-dot", () => import("./keep/sub/link/../ident.env"));
await probe("import-dir", () => import("./secrets/ident.env"));
await probe("alias", async () => (await import("./alias.ts")).value);
console.log("query " + Yuzuko.permissions.querySync({{ name: "read", path: "keep/sub/link/../ident.env" }}).state);
{ffi_probe}{spawn_probe}"#,
      ffi_probe = ffi_probe,
      spawn_probe = spawn_probe,
    ),
  );
  let mut args =
    vec!["run", "--no-prompt", "--allow-read=.", "--allow-write=."];
  if cfg!(feature = "ffi") {
    args.push("--allow-ffi=.");
  }
  if cfg!(feature = "child") {
    args.push("--allow-run=pwd");
    args.push(&run_tool);
    args.push("--allow-env");
  }
  args.push("main.ts");
  let output = run_env(&entry, &args, &[("YUZUKO_PERM_CANARY", "CANARY-ENV")]);
  let stdout = text(&output.stdout);
  let stderr = text(&output.stderr);
  assert_eq!(output.status.code(), Some(0), "{stderr}\n{stdout}");
  let lines = [
    "benign OK BENIGN-ROOT",
    "inside OK API_KEY=INSIDE-BENIGN",
    "escape NotAllowedError",
    "bytes NotAllowedError",
    "dir NotAllowedError",
    "real NotAllowedError",
    "stat NotAllowedError",
    "write NotAllowedError",
    "copy NotAllowedError",
    "chdir NotAllowedError",
    "direct NotAllowedError",
    "linktext NotAllowedError",
    "import TypeError",
    "import-text NotAllowedError",
    "import-dot TypeError",
    "import-dir TypeError",
    "alias OK script-ok",
    "query prompt",
  ];
  for line in lines {
    assert!(
      stdout.lines().any(|item| item.starts_with(line)),
      "missing {line}\n{stdout}\n{stderr}"
    );
  }
  assert!(
    stdout.contains("linked.env must be imported with { type: \"text\" }"),
    "{stdout}"
  );
  assert!(
    stdout.contains("import-text NotAllowedError Requires read access to"),
    "{stdout}"
  );
  assert!(
    stdout.lines().any(
      |item| item.starts_with("import-text ") && item.contains("linked.env")
    ),
    "{stdout}"
  );
  if cfg!(feature = "ffi") {
    for line in ["ffi NotAllowedError", "ffi-direct NotAllowedError"] {
      assert!(
        stdout.lines().any(|item| item.starts_with(line)),
        "missing {line}\n{stdout}\n{stderr}"
      );
    }
  }
  if cfg!(feature = "child") {
    assert!(
      stdout
        .lines()
        .any(|item| item.starts_with("spawn NotAllowedError")),
      "{stdout}\n{stderr}"
    );
    assert!(
      stdout
        .lines()
        .any(|item| item == "direct-run OK INSIDE-TOOL"),
      "{stdout}\n{stderr}"
    );
    assert!(
      stdout
        .lines()
        .any(|item| item.starts_with("escape-run NotAllowedError")),
      "{stdout}\n{stderr}"
    );
  }
  for secret in [
    "sk_live",
    "yz-out-canary",
    "CANARY-DIR-ENTRY",
    "CANARY-ENV",
    "CANARY-RUN",
    "PWNED",
  ] {
    assert!(!stdout.contains(secret), "{secret} in\n{stdout}");
    assert!(!stderr.contains(secret), "{secret} in\n{stderr}");
  }
  assert!(!outside.join("a").join("pwned-canary.txt").exists());
  assert!(!outside.join("a").join("copied-canary.txt").exists());
  assert!(
    !entry
      .join("keep")
      .join("sub")
      .join("pwned-canary.txt")
      .exists()
  );
  let _ = fs::remove_dir_all(&entry);
  let _ = fs::remove_dir_all(&outside);
}

#[cfg(all(unix, feature = "child"))]
#[test]
fn a_relative_spawn_runs_the_child_directory() {
  use std::os::unix::fs::PermissionsExt;
  let entry = scratch("spawn-cwd");
  let outside = scratch("spawn-cwd-out");
  let outside_dir = outside.join("a");
  fs::create_dir_all(entry.join("keep/sub")).unwrap();
  fs::create_dir_all(outside_dir.join("b")).unwrap();
  fs::create_dir_all(entry.join("sub")).unwrap();
  let benign = entry.join("inside-tool.sh");
  let legit = entry.join("sub").join("tool.sh");
  let outside_tool = outside_dir.join("inside-tool.sh");
  let outside_direct = outside_dir.join("direct.sh");
  for (path, line) in [
    (&benign, "INSIDE-TOOL"),
    (&legit, "LEGIT-CWD"),
    (&outside_tool, "CANARY-RUN"),
    (&outside_direct, "CANARY-DIRECT"),
  ] {
    write(path, &format!("#!/bin/sh\necho {line}\n"));
    let mut permissions = fs::metadata(path).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).unwrap();
  }
  std::os::unix::fs::symlink(
    outside_dir.join("b"),
    entry.join("keep/sub/link"),
  )
  .unwrap();
  let tool_link = entry.join("keep/sub/tool-link");
  std::os::unix::fs::symlink(&outside_tool, &tool_link).unwrap();
  std::os::unix::fs::symlink(&outside_dir, entry.join("secrets")).unwrap();
  write(
    &entry.join("main.ts"),
    r#"async function probe(label, run) {
  try {
    const value = await run();
    console.log(label + " OK " + value);
  } catch (error) {
    console.log(label + " " + error.name + " " + error.message);
  }
}
const text = async (child) => new TextDecoder().decode((await child.output()).stdout).trim();
await probe("cwd-escape", async () => text(Yuzuko.spawn("link/../inside-tool.sh", { cwd: "keep/sub" })));
await probe("cwd-legit", async () => text(Yuzuko.spawn("./tool.sh", { cwd: "sub" })));
await probe("direct", async () => text(Yuzuko.spawn(BENIGN)));
await probe("link-run", async () => text(Yuzuko.spawn("keep/sub/tool-link")));
await probe("dir-link", async () => text(Yuzuko.spawn("secrets/direct.sh")));
await probe("missing-cwd", async () => text(Yuzuko.spawn("pwd", { cwd: "missing-dir" })));
await probe("hidden-missing", async () => text(Yuzuko.spawn("pwd", { cwd: HIDDEN })));
await probe("hidden-exists", async () => text(Yuzuko.spawn("pwd", { cwd: OUTSIDE })));
"#,
  );
  let source = fs::read_to_string(entry.join("main.ts")).unwrap();
  let hidden = format!("/tmp/yuzuko-no-such-spawn-{}", std::process::id());
  let source = source
    .replace("BENIGN", &format!("\"{}\"", benign.display()))
    .replace("HIDDEN", &format!("\"{}\"", hidden))
    .replace("OUTSIDE", &format!("\"{}\"", outside_dir.display()));
  write(&entry.join("main.ts"), &source);
  let allow_benign = format!("--allow-run={}", benign.display());
  let allow_legit = format!("--allow-run={}", legit.display());
  let output = run(
    &entry,
    &[
      "run",
      "--no-prompt",
      "--allow-read=.",
      "--allow-env",
      "--allow-run=pwd",
      &allow_benign,
      &allow_legit,
      "main.ts",
    ],
  );
  let stdout = text(&output.stdout);
  let stderr = text(&output.stderr);
  assert_eq!(output.status.code(), Some(0), "{stderr}\n{stdout}");
  for line in [
    "cwd-escape NotAllowedError",
    "cwd-legit OK LEGIT-CWD",
    "direct OK INSIDE-TOOL",
    "link-run NotAllowedError",
    "dir-link NotAllowedError",
    "missing-cwd Error The directory was not found.",
    "hidden-missing NotAllowedError",
    "hidden-exists NotAllowedError",
  ] {
    assert!(
      stdout.lines().any(|item| item.starts_with(line)),
      "missing {line}\n{stdout}\n{stderr}"
    );
  }
  assert!(
    stdout.contains("Requires run access to \"link/../inside-tool.sh\""),
    "{stdout}"
  );
  for label in ["cwd-escape", "link-run", "dir-link", "cwd-legit", "direct"] {
    let line = stdout
      .lines()
      .find(|item| item.starts_with(label))
      .unwrap_or("");
    assert!(
      !line.contains("spawn-cwd-out"),
      "{label} names the other directory\n{line}"
    );
  }
  for secret in ["CANARY-RUN", "CANARY-DIRECT"] {
    assert!(!stdout.contains(secret), "{secret} in\n{stdout}");
    assert!(!stderr.contains(secret), "{secret} in\n{stderr}");
  }

  write(
    &entry.join("link.ts"),
    r#"try {
  const child = Yuzuko.spawn("keep/sub/tool-link");
  const output = await child.output();
  console.log("link-grant OK " + new TextDecoder().decode(output.stdout).trim());
} catch (error) {
  console.log("link-grant " + error.name + " " + error.message);
}
try {
  const child = Yuzuko.spawn("secrets/direct.sh");
  const output = await child.output();
  console.log("dir-grant OK " + new TextDecoder().decode(output.stdout).trim());
} catch (error) {
  console.log("dir-grant " + error.name + " " + error.message);
}
"#,
  );
  let allow_link = format!("--allow-run={}", tool_link.display());
  let allow_dir = format!(
    "--allow-run={}",
    entry.join("secrets").join("direct.sh").display()
  );
  let granted = run(
    &entry,
    &[
      "run",
      "--no-prompt",
      "--allow-env",
      &allow_link,
      &allow_dir,
      "link.ts",
    ],
  );
  let stdout = text(&granted.stdout);
  let stderr = text(&granted.stderr);
  assert_eq!(granted.status.code(), Some(0), "{stderr}\n{stdout}");
  assert!(
    stdout
      .lines()
      .any(|item| item.starts_with("link-grant NotAllowedError")),
    "{stdout}\n{stderr}"
  );
  assert!(
    stdout
      .lines()
      .any(|item| item.starts_with("dir-grant NotAllowedError")),
    "{stdout}\n{stderr}"
  );
  assert!(stdout.contains("keep/sub/tool-link"), "{stdout}");
  for secret in ["CANARY-RUN", "CANARY-DIRECT", "spawn-cwd-out"] {
    assert!(!stdout.contains(secret), "{secret} in\n{stdout}");
    assert!(!stderr.contains(secret), "{secret} in\n{stderr}");
  }
  let _ = fs::remove_dir_all(&entry);
  let _ = fs::remove_dir_all(&outside);
}
