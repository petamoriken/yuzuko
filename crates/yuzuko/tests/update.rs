//! Update checks against a host bundle that contains the real `yuzuko`
//! binary.

use std::fs::{self, File};
use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use base64::Engine;
use base64::prelude::BASE64_STANDARD;
use ed25519_dalek::Signer;
use ed25519_dalek::SigningKey;

fn bin() -> PathBuf {
  PathBuf::from(env!("CARGO_BIN_EXE_yuzuko"))
}

fn text(bytes: &[u8]) -> String {
  String::from_utf8_lossy(bytes).replace('\r', "")
}

fn scratch(name: &str) -> PathBuf {
  let dir = std::env::temp_dir().join(format!(
    "yuzuko-update-{name}-{}-{}",
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

fn signing() -> SigningKey {
  SigningKey::from_bytes(&[42u8; 32])
}

fn encode_payload(version: &str, entry: &str, body: &str) -> Vec<u8> {
  let bytes = body.as_bytes();
  let mut out = Vec::new();
  for line in ["YUZUKO1", version, entry, "1"] {
    out.extend_from_slice(line.as_bytes());
    out.push(b'\n');
  }
  out.extend_from_slice(entry.as_bytes());
  out.push(b'\n');
  out.extend_from_slice(bytes.len().to_string().as_bytes());
  out.push(b'\n');
  out.extend_from_slice(bytes);
  out
}

fn fingerprint(path: &Path) -> (u64, u64) {
  let mut file = File::open(path).unwrap();
  let mut hash = 0xcbf29ce484222325u64;
  let mut len = 0u64;
  let mut buf = [0u8; 8192];
  loop {
    let count = file.read(&mut buf).unwrap();
    if count == 0 {
      break;
    }
    len += u64::try_from(count).unwrap();
    for byte in &buf[..count] {
      hash ^= u64::from(*byte);
      hash = hash.wrapping_mul(0x100000001b3);
    }
  }
  (len, hash)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
  Older,
  Equal,
  Missing,
  Bad,
  Drop,
  Truncate,
  Newer,
}

struct FeedServer {
  port: u16,
  mode: Arc<Mutex<Mode>>,
  hits: Arc<Mutex<Vec<String>>>,
  stop: Arc<AtomicBool>,
  thread: Option<std::thread::JoinHandle<()>>,
}

impl Drop for FeedServer {
  fn drop(&mut self) {
    self.stop.store(true, Ordering::SeqCst);
    let _ = TcpStream::connect(("127.0.0.1", self.port));
    if let Some(thread) = self.thread.take() {
      let _ = thread.join();
    }
  }
}

fn feed_server(payload: Vec<u8>, signature_b64: String) -> FeedServer {
  let listener = TcpListener::bind("127.0.0.1:0").unwrap();
  let port = listener.local_addr().unwrap().port();
  let mode = Arc::new(Mutex::new(Mode::Older));
  let hits = Arc::new(Mutex::new(Vec::new()));
  let stop = Arc::new(AtomicBool::new(false));
  let mode_thread = Arc::clone(&mode);
  let hits_thread = Arc::clone(&hits);
  let stop_thread = Arc::clone(&stop);
  let bad_b64 = BASE64_STANDARD.encode([9u8; 64]);
  let thread = std::thread::spawn(move || {
    loop {
      if stop_thread.load(Ordering::SeqCst) {
        break;
      }
      let Ok((mut stream, _)) = listener.accept() else {
        break;
      };
      if stop_thread.load(Ordering::SeqCst) {
        break;
      }
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
        if buf.len() > 8192 {
          break;
        }
      }
      let request = String::from_utf8_lossy(&buf);
      let path = request.split_whitespace().nth(1).unwrap_or("").to_string();
      if !path.is_empty() {
        hits_thread.lock().unwrap().push(path.clone());
      }
      let mode = *mode_thread.lock().unwrap();
      if mode == Mode::Drop {
        let _ = stream.shutdown(Shutdown::Both);
        continue;
      }
      if path.ends_with("/feed.json") {
        let body = match mode {
          Mode::Older => format!(
            r#"{{"version":"0.9.0","payload":"builds/1.1.0.yuzuko","signature":"{signature_b64}"}}"#
          ),
          Mode::Equal => format!(
            r#"{{"version":"1.0","payload":"builds/1.1.0.yuzuko","signature":"{signature_b64}"}}"#
          ),
          Mode::Missing => {
            r#"{"version":"9.9.9","payload":"builds/1.1.0.yuzuko"}"#.to_string()
          }
          Mode::Bad => format!(
            r#"{{"version":"1.1.0","payload":"builds/1.1.0.yuzuko","signature":"{bad_b64}"}}"#
          ),
          Mode::Truncate | Mode::Newer | Mode::Drop => format!(
            r#"{{"version":"1.1.0","payload":"builds/1.1.0.yuzuko","signature":"{signature_b64}"}}"#
          ),
        };
        let _ = stream.write_all(&http_ok(body.as_bytes()));
        continue;
      }
      if mode == Mode::Truncate {
        let mut response = format!(
          "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
          payload.len() + 48
        )
        .into_bytes();
        response.extend_from_slice(&payload);
        let _ = stream.write_all(&response);
        continue;
      }
      let _ = stream.write_all(&http_ok(&payload));
    }
  });
  FeedServer {
    port,
    mode,
    hits,
    stop,
    thread: Some(thread),
  }
}

fn http_ok(body: &[u8]) -> Vec<u8> {
  let mut response = format!(
    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
    body.len()
  )
  .into_bytes();
  response.extend_from_slice(body);
  response
}

fn new_hits(server: &FeedServer, mark: &mut usize) -> Vec<String> {
  let hits = server.hits.lock().unwrap();
  let next = hits[*mark..].to_vec();
  *mark = hits.len();
  next
}

/// The `--output` path `yuzuko package` accepts on this host.
fn bundle_name() -> &'static str {
  if cfg!(target_os = "macos") {
    "UpdateGame.app"
  } else {
    "UpdateGame"
  }
}

fn bundle_paths(app: &Path) -> (PathBuf, PathBuf) {
  if cfg!(target_os = "macos") {
    (
      app.join("Contents/Resources"),
      app.join("Contents/MacOS/update-game"),
    )
  } else if cfg!(windows) {
    (app.to_path_buf(), app.join("update-game.exe"))
  } else {
    (app.to_path_buf(), app.join("update-game"))
  }
}

fn launch(exe: &Path) -> (i32, String, String) {
  let cwd = exe.parent().unwrap().to_path_buf();
  let mut child = Command::new(exe)
    .current_dir(&cwd)
    .env_remove("YUZUKO_HEADLESS")
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .spawn()
    .expect("spawn packaged app");
  let start = Instant::now();
  loop {
    if let Some(status) = child.try_wait().unwrap() {
      let output = child.wait_with_output().unwrap();
      return (
        status.code().unwrap_or(1),
        text(&output.stdout),
        text(&output.stderr),
      );
    }
    if start.elapsed() > Duration::from_secs(60) {
      let _ = child.kill();
      let output = child.wait_with_output().unwrap();
      panic!(
        "packaged app hung\n{}{}",
        text(&output.stdout),
        text(&output.stderr)
      );
    }
    std::thread::sleep(Duration::from_millis(50));
  }
}

fn assert_old(status: i32, stdout: &str, stderr: &str) {
  assert_eq!(status, 0, "{stdout}\n{stderr}");
  assert_eq!(stdout.matches("token-old").count(), 2, "{stdout}");
  assert!(!stdout.contains("token-new"), "{stdout}");
}

fn assert_no_stage(resources: &Path) {
  assert!(!resources.join("Updates/payload").is_file());
  assert!(!resources.join("Updates/signature").is_file());
}

#[test]
fn packaged_app_updates_on_the_next_launch() {
  let payload =
    encode_payload("1.1.0", "src/main.ts", "console.log(\"token-new\");\n");
  let key = signing();
  let public = key.verifying_key().to_bytes();
  let signature = Signer::try_sign(&key, &payload).unwrap().to_bytes();
  let signature_b64 = BASE64_STANDARD.encode(signature);
  let server = feed_server(payload.clone(), signature_b64);
  let dir = scratch("app");
  let base = format!("http://127.0.0.1:{}/game", server.port);
  fs::write(
    dir.join("yuzuko.json"),
    format!(
      r#"{{
  "name": "Update Game",
  "version": "1.0.0",
  "release": {{
    "baseUrl": "{base}",
    "publicKey": "{}"
  }}
}}
"#,
      BASE64_STANDARD.encode(public)
    ),
  )
  .unwrap();
  fs::create_dir_all(dir.join("src")).unwrap();
  fs::write(
    dir.join("src/main.ts"),
    r#"console.log("token-old");
try {
  const status = await Yuzuko.updater.check("http://127.0.0.1:9/not-the-feed");
  console.log("status " + JSON.stringify(status));
  if (status.available) {
    await Yuzuko.updater.download("http://127.0.0.1:9/not-the-payload");
    console.log("downloaded");
  } else {
    console.log("not-available");
  }
} catch (error) {
  console.log("update-error " + error.name + " " + error.message);
}
console.log("token-old");
"#,
  )
  .unwrap();
  let app = dir.join(bundle_name());
  let packaged = Command::new(bin())
    .current_dir(&dir)
    .args(["package", "src/main.ts", "--output", app.to_str().unwrap()])
    .output()
    .unwrap();
  assert_eq!(
    packaged.status.code(),
    Some(0),
    "{}",
    text(&packaged.stderr)
  );
  let (resources, exe) = bundle_paths(&app);
  assert!(exe.is_file(), "missing {}", exe.display());
  let key_path = resources.join("yuzuko.update-key");
  assert_eq!(fs::read(&key_path).unwrap(), public);
  let manifest_path = resources.join("yuzuko.bundle.json");
  let original_manifest = fs::read_to_string(&manifest_path).unwrap();
  assert!(original_manifest.contains(&base), "{original_manifest}");
  assert!(
    original_manifest.contains("\"version\": \"1.0.0\""),
    "{original_manifest}"
  );
  if cfg!(target_os = "macos") {
    let plist = fs::read_to_string(app.join("Contents/Info.plist")).unwrap();
    assert!(plist.contains(&base), "{plist}");
    assert!(plist.contains("<string>1.0.0</string>"), "{plist}");
  }
  let source = resources.join("app/src/main.ts");
  assert!(fs::read_to_string(&source).unwrap().contains("token-old"));
  let exe_id = fingerprint(&exe);
  let mut mark = 0usize;

  let saved = original_manifest.clone();
  let mut value: serde_json::Value = serde_json::from_str(&saved).unwrap();
  value.as_object_mut().unwrap().remove("releaseBaseUrl");
  fs::write(
    &manifest_path,
    serde_json::to_string_pretty(&value).unwrap(),
  )
  .unwrap();
  let (status, stdout, stderr) = launch(&exe);
  assert_old(status, &stdout, &stderr);
  assert!(
    stdout.contains("update-error InvalidStateError There is no update feed."),
    "{stdout}\n{stderr}"
  );
  assert!(new_hits(&server, &mut mark).is_empty(), "contacted a host");
  fs::write(&manifest_path, &saved).unwrap();
  assert_no_stage(&resources);

  fs::remove_file(&key_path).unwrap();
  let (status, stdout, stderr) = launch(&exe);
  assert_old(status, &stdout, &stderr);
  assert!(
    stdout.contains(
      "update-error InvalidStateError There is no update trust anchor."
    ),
    "{stdout}\n{stderr}"
  );
  assert!(new_hits(&server, &mut mark).is_empty(), "contacted a host");
  fs::write(&key_path, public).unwrap();
  assert_no_stage(&resources);

  *server.mode.lock().unwrap() = Mode::Older;
  let (status, stdout, stderr) = launch(&exe);
  assert_old(status, &stdout, &stderr);
  assert!(stdout.contains("\"available\":false"), "{stdout}");
  assert!(stdout.contains("\"version\":\"1.0.0\""), "{stdout}");
  assert!(stdout.contains("not-available"), "{stdout}");
  let hits = new_hits(&server, &mut mark);
  assert_eq!(hits, vec!["/game/feed.json".to_string()], "{hits:?}");
  assert_no_stage(&resources);

  *server.mode.lock().unwrap() = Mode::Equal;
  let (status, stdout, stderr) = launch(&exe);
  assert_old(status, &stdout, &stderr);
  assert!(stdout.contains("\"available\":false"), "{stdout}");
  assert!(stdout.contains("\"version\":\"1.0.0\""), "{stdout}");
  assert!(stdout.contains("not-available"), "{stdout}");
  let hits = new_hits(&server, &mut mark);
  assert_eq!(hits, vec!["/game/feed.json".to_string()], "{hits:?}");
  assert_no_stage(&resources);
  assert_eq!(
    fs::read(&source).unwrap(),
    fs::read(dir.join("src/main.ts")).unwrap()
  );

  *server.mode.lock().unwrap() = Mode::Missing;
  let (status, stdout, stderr) = launch(&exe);
  assert_old(status, &stdout, &stderr);
  assert!(
    stdout
      .contains("update-error InvalidStateError The update feed is not valid."),
    "{stdout}\n{stderr}"
  );
  let hits = new_hits(&server, &mut mark);
  assert_eq!(hits, vec!["/game/feed.json".to_string()], "{hits:?}");
  assert_no_stage(&resources);

  *server.mode.lock().unwrap() = Mode::Bad;
  let (status, stdout, stderr) = launch(&exe);
  assert_old(status, &stdout, &stderr);
  assert!(
    stdout.contains(
      "update-error InvalidStateError The update signature is not valid."
    ),
    "{stdout}\n{stderr}"
  );
  let hits = new_hits(&server, &mut mark);
  assert_eq!(
    hits,
    vec![
      "/game/feed.json".to_string(),
      "/game/builds/1.1.0.yuzuko".to_string(),
    ],
    "{hits:?}"
  );
  assert_no_stage(&resources);

  *server.mode.lock().unwrap() = Mode::Drop;
  let (status, stdout, stderr) = launch(&exe);
  assert_old(status, &stdout, &stderr);
  assert!(
    stdout.contains(
      "update-error NetworkError The update could not be downloaded."
    ),
    "{stdout}\n{stderr}"
  );
  assert_no_stage(&resources);
  let _ = new_hits(&server, &mut mark);

  *server.mode.lock().unwrap() = Mode::Truncate;
  let (status, stdout, stderr) = launch(&exe);
  assert_old(status, &stdout, &stderr);
  assert!(
    stdout.contains(
      "update-error NetworkError The connection closed before the body ended."
    ),
    "{stdout}\n{stderr}"
  );
  let hits = new_hits(&server, &mut mark);
  assert_eq!(
    hits,
    vec![
      "/game/feed.json".to_string(),
      "/game/builds/1.1.0.yuzuko".to_string(),
    ],
    "{hits:?}"
  );
  assert_no_stage(&resources);
  assert!(fs::read_to_string(&source).unwrap().contains("token-old"));

  *server.mode.lock().unwrap() = Mode::Newer;
  let (status, stdout, stderr) = launch(&exe);
  assert_old(status, &stdout, &stderr);
  assert!(stdout.contains("\"available\":true"), "{stdout}");
  assert!(stdout.contains("\"version\":\"1.1.0\""), "{stdout}");
  assert!(stdout.contains("downloaded"), "{stdout}");
  let hits = new_hits(&server, &mut mark);
  assert!(
    hits.iter().all(|hit| !hit.contains("not-the-feed")),
    "{hits:?}"
  );
  assert_eq!(
    hits,
    vec![
      "/game/feed.json".to_string(),
      "/game/builds/1.1.0.yuzuko".to_string(),
    ],
    "{hits:?}"
  );
  assert_eq!(
    fs::read(resources.join("Updates/payload")).unwrap(),
    payload
  );
  assert_eq!(
    fs::read(resources.join("Updates/signature")).unwrap(),
    signature
  );
  assert!(fs::read_to_string(&source).unwrap().contains("token-old"));
  assert_eq!(fingerprint(&exe), exe_id);
  assert_eq!(fs::read(&key_path).unwrap(), public);

  let (status, stdout, stderr) = launch(&exe);
  assert_eq!(status, 0, "{stdout}\n{stderr}");
  assert!(stdout.contains("token-new"), "{stdout}");
  assert!(!stdout.contains("token-old"), "{stdout}");
  assert_eq!(stdout.matches("token-new").count(), 1, "{stdout}");
  assert!(fs::read_to_string(&source).unwrap().contains("token-new"));
  assert!(!resources.join("Updates/payload").exists());
  let manifest = fs::read_to_string(&manifest_path).unwrap();
  assert!(manifest.contains("\"version\": \"1.1.0\""), "{manifest}");
  assert!(manifest.contains(&base), "{manifest}");
  assert!(manifest.contains("src/main.ts"), "{manifest}");
  if cfg!(target_os = "macos") {
    let plist = fs::read_to_string(app.join("Contents/Info.plist")).unwrap();
    assert!(plist.contains("<string>1.0.0</string>"), "{plist}");
    assert!(!plist.contains("1.1.0"), "{plist}");
  }
  assert_eq!(fs::read(&key_path).unwrap(), public);
  assert_eq!(fingerprint(&exe), exe_id);
}

#[test]
fn unpackaged_run_and_repl_have_no_updater() {
  let dir = scratch("unpackaged");
  fs::write(
    dir.join("probe.ts"),
    "console.log(typeof Yuzuko.updater);\n",
  )
  .unwrap();
  for _ in 0..2 {
    let output = Command::new(bin())
      .current_dir(&dir)
      .args(["run", "probe.ts"])
      .output()
      .unwrap();
    assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
    assert_eq!(text(&output.stdout).trim(), "undefined");
  }

  let mut child = Command::new(bin())
    .current_dir(&dir)
    .arg("repl")
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .spawn()
    .unwrap();
  child
    .stdin
    .take()
    .unwrap()
    .write_all(b"typeof Yuzuko.updater\n")
    .unwrap();
  let output = child.wait_with_output().unwrap();
  assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
  let stdout = text(&output.stdout);
  assert!(
    stdout.lines().any(|line| line.trim() == "undefined"),
    "{stdout}"
  );
  assert!(
    !stdout.lines().any(|line| line.trim() == "object"),
    "{stdout}"
  );
}
