//! Piped standard streams, `execPath`, `pid`, `ppid`, and `version`.
//!
//! The child is the `yuzuko` binary this test built. A pipe is not a
//! terminal, so the permission prompt is not involved.

#![cfg(feature = "fs")]

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

fn bin() -> PathBuf {
  PathBuf::from(env!("CARGO_BIN_EXE_yuzuko"))
}

fn scratch(name: &str) -> PathBuf {
  let dir = std::env::temp_dir().join(format!(
    "yuzuko-process-io-{}-{}-{name}",
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

fn command(dir: &Path, args: &[&str]) -> Command {
  let mut command = Command::new(bin());
  command
    .current_dir(dir)
    .args(args)
    .env("YUZUKO_HEADLESS", "1")
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
  command
}

fn wait(mut child: std::process::Child) -> Output {
  let start = Instant::now();
  loop {
    if child.try_wait().unwrap().is_some() {
      return child.wait_with_output().unwrap();
    }
    if start.elapsed() > Duration::from_secs(20) {
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

fn finish(output: &Output) -> (String, String) {
  let stdout = text(&output.stdout);
  let stderr = text(&output.stderr);
  assert_eq!(output.status.code(), Some(0), "{stderr}\n{stdout}");
  (stdout, stderr)
}

#[test]
fn stdin_bytes_are_written_to_stdout() {
  let dir = scratch("bytes");
  fs::write(
    dir.join("main.ts"),
    r#"const parts = [];
for (;;) {
  const buf = new Uint8Array(8);
  const n = Yuzuko.stdin.readSync(buf);
  if (n === null) {
    break;
  }
  parts.push(buf.slice(0, n));
}
const size = parts.reduce((sum, part) => sum + part.length, 0);
const out = new Uint8Array(size);
let offset = 0;
for (const part of parts) {
  out.set(part, offset);
  offset += part.length;
}
Yuzuko.stdout.writeSync(out);
"#,
  )
  .unwrap();
  let mut child = command(&dir, &["run", "--allow-all", "main.ts"])
    .stdin(Stdio::piped())
    .spawn()
    .expect("spawn yuzuko");
  {
    let mut stdin = child.stdin.take().expect("stdin");
    stdin.write_all(b"hello-stdio").unwrap();
  }
  let output = wait(child);
  let stderr = text(&output.stderr);
  assert_eq!(output.status.code(), Some(0), "{stderr}");
  assert_eq!(output.stdout, b"hello-stdio", "{stderr}");
}

#[test]
fn exec_path_pid_ppid_and_version_match_the_process() {
  let dir = scratch("identity");
  fs::write(
    dir.join("worker.ts"),
    r#"self.onmessage = () => {
  const version = Yuzuko.version;
  postMessage({
    execPath: Yuzuko.execPath(),
    pid: Yuzuko.pid,
    ppid: Yuzuko.ppid,
    keys: Object.keys(version),
    yuzuko: version.yuzuko,
    v8: version.v8,
    frozen: Object.isFrozen(version),
    typescript: Object.prototype.hasOwnProperty.call(version, "typescript"),
    notCapable: typeof Yuzuko.errors.NotCapable,
  });
};
"#,
  )
  .unwrap();
  fs::write(
    dir.join("main.ts"),
    r#"const pid = Yuzuko.pid;
const ppid = Yuzuko.ppid;
Yuzuko.pid = 1;
Yuzuko.ppid = 1;
const assigned = Yuzuko.pid === 1 && Yuzuko.ppid === 1;
Yuzuko.pid = pid;
Yuzuko.ppid = ppid;
const version = Yuzuko.version;
const versionDescriptor = Object.getOwnPropertyDescriptor(Yuzuko, "version");
let rawName = "";
let rawMessage = "";
try {
  Yuzuko.stdin.setRaw(true);
} catch (error) {
  rawName = error.name;
  rawMessage = error.message;
}
const worker = new Worker(new URL("./worker.ts", import.meta.url), {
  type: "module",
});
const fromWorker = await new Promise((resolve, reject) => {
  worker.onmessage = (event) => resolve(event.data);
  worker.onerror = (event) => {
    reject(new Error(String(event.message || "worker error")));
  };
  worker.postMessage("go");
});
worker.terminate();
const report = {
  execPath: Yuzuko.execPath(),
  pid: Yuzuko.pid,
  ppid: Yuzuko.ppid,
  assigned,
  keys: Object.keys(version),
  yuzuko: version.yuzuko,
  v8: version.v8,
  frozen: Object.isFrozen(version),
  typescript: Object.prototype.hasOwnProperty.call(version, "typescript"),
  versionWritable: versionDescriptor.writable === true,
  stdinTerminal: Yuzuko.stdin.isTerminal(),
  stdoutTerminal: Yuzuko.stdout.isTerminal(),
  stderrTerminal: Yuzuko.stderr.isTerminal(),
  readable: typeof Yuzuko.stdin.readable,
  writable: typeof Yuzuko.stdout.writable,
  rawName,
  rawMessage,
  worker: fromWorker,
};
Yuzuko.stdout.writeSync(new TextEncoder().encode(JSON.stringify(report)));
"#,
  )
  .unwrap();
  let child = command(&dir, &["run", "--allow-all", "main.ts"])
    .stdin(Stdio::null())
    .spawn()
    .expect("spawn yuzuko");
  let id = child.id();
  let output = wait(child);
  let (stdout, stderr) = finish(&output);
  let report: serde_json::Value = serde_json::from_str(stdout.trim())
    .unwrap_or_else(|error| {
      panic!("report {error}\n{stdout}\n{stderr}");
    });
  let reported = report["execPath"].as_str().unwrap_or("");
  let reported =
    fs::canonicalize(reported).unwrap_or_else(|_| PathBuf::from(reported));
  let executable = fs::canonicalize(bin()).unwrap_or_else(|_| bin());
  assert_eq!(reported, executable, "{stdout}");
  assert_eq!(report["pid"].as_u64(), Some(u64::from(id)), "{stdout}");
  assert_eq!(
    report["ppid"].as_u64(),
    Some(u64::from(std::process::id())),
    "{stdout}"
  );
  assert_eq!(report["assigned"], true, "{stdout}");
  assert_eq!(
    report["keys"],
    serde_json::json!(["yuzuko", "v8"]),
    "{stdout}"
  );
  assert_eq!(
    report["yuzuko"].as_str(),
    Some(env!("CARGO_PKG_VERSION")),
    "{stdout}"
  );
  let v8 = report["v8"].as_str().unwrap_or("");
  assert!(!v8.is_empty(), "{stdout}");
  assert_eq!(report["frozen"], true, "{stdout}");
  assert_eq!(report["typescript"], false, "{stdout}");
  assert_eq!(report["versionWritable"], true, "{stdout}");
  assert_eq!(report["stdinTerminal"], false, "{stdout}");
  assert_eq!(report["stdoutTerminal"], false, "{stdout}");
  assert_eq!(report["stderrTerminal"], false, "{stdout}");
  assert_eq!(report["readable"], "object", "{stdout}");
  assert_eq!(report["writable"], "object", "{stdout}");
  assert_eq!(report["rawName"], "NotSupported", "{stdout}");
  assert_eq!(
    report["rawMessage"], "The file is not a terminal.",
    "{stdout}"
  );
  let worker = &report["worker"];
  assert_eq!(
    worker["execPath"].as_str(),
    Some(report["execPath"].as_str().unwrap())
  );
  let worker_path = worker["execPath"].as_str().unwrap_or("");
  let worker_path = fs::canonicalize(worker_path)
    .unwrap_or_else(|_| PathBuf::from(worker_path));
  assert_eq!(worker_path, executable, "{stdout}");
  assert_eq!(worker["pid"].as_u64(), Some(u64::from(id)), "{stdout}");
  assert_eq!(
    worker["ppid"].as_u64(),
    Some(u64::from(std::process::id())),
    "{stdout}"
  );
  assert_eq!(
    worker["keys"],
    serde_json::json!(["yuzuko", "v8"]),
    "{stdout}"
  );
  assert_eq!(worker["yuzuko"].as_str(), Some(env!("CARGO_PKG_VERSION")));
  assert_eq!(worker["v8"].as_str(), Some(v8));
  assert_eq!(worker["frozen"], true, "{stdout}");
  assert_eq!(worker["typescript"], false, "{stdout}");
  assert_eq!(worker["notCapable"], "function", "{stdout}");
}

#[test]
fn stderr_close_keeps_the_process_stream() {
  let dir = scratch("stderr");
  fs::write(
    dir.join("main.ts"),
    r#"Yuzuko.stderr.writeSync(new TextEncoder().encode("before\n"));
Yuzuko.stderr.close();
let name = "";
try {
  Yuzuko.stderr.close();
} catch (error) {
  name = error.name;
}
Yuzuko.stdout.writeSync(new TextEncoder().encode(name));
console.error("after");
"#,
  )
  .unwrap();
  let child = command(&dir, &["run", "--allow-all", "main.ts"])
    .stdin(Stdio::null())
    .spawn()
    .expect("spawn yuzuko");
  let output = wait(child);
  let stdout = text(&output.stdout);
  let stderr = text(&output.stderr);
  assert_eq!(output.status.code(), Some(0), "{stderr}\n{stdout}");
  assert_eq!(stdout, "BadResource", "{stderr}");
  assert!(
    stderr.contains("before") && stderr.contains("after"),
    "{stderr}"
  );
}
