//! `Yuzuko.memoryUsage`, `Yuzuko.noColor`, `Yuzuko.refTimer`, and
//! `Yuzuko.unrefTimer` on the real `yuzuko` binary.
//!
//! The child is the binary this test built. A pipe is not a terminal, so a
//! missing grant throws instead of prompting.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

fn bin() -> PathBuf {
  PathBuf::from(env!("CARGO_BIN_EXE_yuzuko"))
}

fn scratch(name: &str) -> PathBuf {
  let dir = std::env::temp_dir().join(format!(
    "yuzuko-runtime-usage-{}-{}-{name}",
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
    .env_remove("NO_COLOR")
    .env_remove("FORCE_COLOR")
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
  command
}

fn wait_for(mut child: std::process::Child, limit: Duration) -> Output {
  let start = Instant::now();
  loop {
    if child.try_wait().unwrap().is_some() {
      return child.wait_with_output().unwrap();
    }
    if start.elapsed() > limit {
      let _ = child.kill();
      let output = child.wait_with_output().unwrap();
      panic!(
        "timed out after {}ms\nstdout:\n{}\nstderr:\n{}",
        limit.as_millis(),
        text(&output.stdout),
        text(&output.stderr)
      );
    }
    std::thread::sleep(Duration::from_millis(20));
  }
}

fn finish(output: &Output) -> String {
  let stdout = text(&output.stdout);
  let stderr = text(&output.stderr);
  assert_eq!(output.status.code(), Some(0), "{stderr}\n{stdout}");
  stdout
}

fn write_script(dir: &Path, name: &str, source: &str) -> PathBuf {
  let path = dir.join(name);
  fs::write(&path, source).unwrap();
  path
}

const USAGE: &str = r#"
const before = Yuzuko.memoryUsage();
const held = new ArrayBuffer(8 * 1024 * 1024);
const after = Yuzuko.memoryUsage();
const keys = Object.keys(before);
const finite = [before, after].every((sample) =>
  keys.every((key) => Number.isFinite(sample[key]) && sample[key] >= 0),
);
const grew = after.heapUsed > before.heapUsed || after.external > before.external;
const workerSource = [
  "const before = Yuzuko.memoryUsage();",
  "const held = new ArrayBuffer(8 * 1024 * 1024);",
  "const after = Yuzuko.memoryUsage();",
  "const keys = Object.keys(before);",
  "const snapshot = Yuzuko.noColor;",
  "Yuzuko.noColor = !snapshot;",
  "Yuzuko.unrefTimer(-1);",
  "Yuzuko.unrefTimer(0);",
  "Yuzuko.refTimer(123456);",
  "const finite = [before, after].every((sample) => keys.every((key) => Number.isFinite(sample[key]) && sample[key] >= 0));",
  "const grew = after.heapUsed > before.heapUsed || after.external > before.external;",
  "postMessage([",
  "  typeof Deno,",
  "  typeof Yuzuko.memoryUsage,",
  "  snapshot,",
  "  Yuzuko.noColor,",
  "  typeof Yuzuko.refTimer,",
  "  typeof Yuzuko.unrefTimer,",
  "  typeof Yuzuko.stdin,",
  "  keys.join(','),",
  "  before.rss > 0,",
  "  finite,",
  "  grew,",
  "  before !== Yuzuko.memoryUsage(),",
  "  held.byteLength,",
  "  Yuzuko.refTimer.length,",
  "  Yuzuko.unrefTimer.length,",
  "].join(' '));",
  "close();",
].join("\n");
const worker = new Worker(
  "data:text/javascript," + encodeURIComponent(workerSource),
);
const workerLine = await new Promise((resolve, reject) => {
  worker.onmessage = (event) => resolve(event.data);
  worker.onerror = (event) => {
    reject(new Error(String(event.message || "worker error")));
  };
});
worker.terminate();
console.log([
  typeof Deno,
  typeof Yuzuko.memoryUsage,
  Yuzuko.noColor,
  typeof Yuzuko.refTimer,
  typeof Yuzuko.unrefTimer,
  typeof Yuzuko.stdin,
  keys.join(","),
  before.rss > 0,
  finite,
  grew,
  before !== Yuzuko.memoryUsage(),
  held.byteLength,
  Yuzuko.refTimer.length,
  Yuzuko.unrefTimer.length,
].join(" "));
console.log(workerLine);
"#;

fn usage_lines(stdin: &str) -> String {
  // deno, memoryUsage, noColor, refTimer, unrefTimer, stdin, keys, rss,
  // finite, grew, fresh, byteLength, refTimer.length, unrefTimer.length.
  let main = format!(
    "undefined function false function function {stdin} rss,heapTotal,heapUsed,external true true true true 8388608 1 1"
  );
  // The worker also reports the boolean it assigned over its own property.
  // That assignment stays in the worker. The snapshot still matches main.
  let worker = format!(
    "undefined function false true function function {stdin} rss,heapTotal,heapUsed,external true true true true 8388608 1 1"
  );
  format!("{main}\n{worker}\n")
}

#[test]
fn memory_usage_and_no_color_match_across_launches() {
  let dir = scratch("usage");
  write_script(&dir, "main.ts", USAGE);
  let stdin = if cfg!(feature = "fs") {
    "object"
  } else {
    "undefined"
  };
  let mut previous = None;
  for _ in 0..2 {
    let child = command(&dir, &["run", "--no-prompt", "main.ts"])
      .spawn()
      .expect("spawn yuzuko");
    let stdout = finish(&wait_for(child, Duration::from_secs(20)));
    assert_eq!(stdout, usage_lines(stdin), "{stdout}");
    if let Some(earlier) = &previous {
      assert_eq!(&stdout, earlier);
    }
    previous = Some(stdout);
  }
}

#[test]
fn no_color_follows_the_process_start_value() {
  let dir = scratch("color");
  write_script(
    &dir,
    "read.ts",
    "console.log(Yuzuko.noColor ? 'yes' : 'no');\n",
  );
  write_script(
    &dir,
    "mutate.ts",
    r#"const first = Yuzuko.noColor;
Yuzuko.env.set("NO_COLOR", "changed");
const second = Yuzuko.noColor;
Yuzuko.env.delete("NO_COLOR");
const third = Yuzuko.noColor;
Yuzuko.env.set("NO_COLOR", "");
const fourth = Yuzuko.noColor;
console.log([first, second, third, fourth].join(" "));
"#,
  );

  for _ in 0..2 {
    let child = command(&dir, &["run", "--no-prompt", "read.ts"])
      .env("FORCE_COLOR", "1")
      .spawn()
      .expect("spawn yuzuko");
    let stdout = finish(&wait_for(child, Duration::from_secs(20)));
    assert_eq!(stdout.trim(), "no");
  }

  for _ in 0..2 {
    let child = command(&dir, &["run", "--no-prompt", "read.ts"])
      .env("NO_COLOR", "")
      .env("FORCE_COLOR", "1")
      .spawn()
      .expect("spawn yuzuko");
    let stdout = finish(&wait_for(child, Duration::from_secs(20)));
    assert_eq!(stdout.trim(), "no");
  }

  for _ in 0..2 {
    let child = command(&dir, &["run", "--no-prompt", "read.ts"])
      .env("NO_COLOR", "1")
      .env("FORCE_COLOR", "1")
      .spawn()
      .expect("spawn yuzuko");
    let stdout = finish(&wait_for(child, Duration::from_secs(20)));
    assert_eq!(stdout.trim(), "yes");
  }

  for _ in 0..2 {
    let child =
      command(&dir, &["run", "--no-prompt", "--allow-env", "mutate.ts"])
        .env("NO_COLOR", "1")
        .spawn()
        .expect("spawn yuzuko");
    let stdout = finish(&wait_for(child, Duration::from_secs(20)));
    assert_eq!(stdout.trim(), "true true true true");
  }
}

const UNREF: &str = r#"
const timeout = setTimeout(() => {
  console.log("timeout");
}, 400);
const interval = setInterval(() => {
  console.log("interval");
}, 400);
Yuzuko.unrefTimer(timeout);
Yuzuko.unrefTimer(interval);
Yuzuko.unrefTimer(-1);
Yuzuko.unrefTimer(0);
Yuzuko.unrefTimer(1.8);
Yuzuko.refTimer(123456);
Yuzuko.unrefTimer(Symbol());
Yuzuko.refTimer({
  valueOf() {
    throw new Error("hook");
  },
});
"#;

const REF: &str = r#"
const id = setTimeout(() => {
  console.log("timeout");
}, 200);
Yuzuko.unrefTimer(id);
Yuzuko.refTimer(id);
"#;

const CLEAR: &str = r#"
const dropped = setTimeout(() => {
  console.log("dropped");
}, 50);
Yuzuko.unrefTimer(dropped);
clearTimeout(dropped);
setTimeout(() => {
  console.log("held");
}, 120);
"#;

const INTERVAL: &str = r#"
let seen = 0;
const id = setInterval(() => {
  seen += 1;
  Yuzuko.unrefTimer(id);
  console.log(seen === 1 ? "once" : "again");
}, 40);
"#;

const ZERO_TIMEOUT: &str = r#"
const id = setTimeout(() => {
  console.log("ran");
}, 0);
Yuzuko.unrefTimer(id);
"#;

const ZERO_INTERVAL: &str = r#"
const id = setInterval(() => {
  console.log("ran");
}, 0);
Yuzuko.unrefTimer(id);
"#;

const ZERO_HELD: &str = r#"
const id = setTimeout(() => {
  console.log("due");
}, 0);
Yuzuko.unrefTimer(id);
setTimeout(() => {
  console.log("hold");
}, 80);
"#;

fn timer_stdout(dir: &Path, script: &str) -> String {
  let child = command(dir, &["run", "--no-prompt", script])
    .spawn()
    .expect("spawn yuzuko");
  // Debug startup can take longer than the timer delay. A broken unref
  // still exits after the callback and fails the stdout check. This limit
  // only catches a process that never becomes idle.
  finish(&wait_for(child, Duration::from_secs(8)))
}

#[test]
fn unreferenced_timers_do_not_keep_the_process_alive() {
  let dir = scratch("timers");
  write_script(&dir, "unref.ts", UNREF);
  write_script(&dir, "ref.ts", REF);
  write_script(&dir, "clear.ts", CLEAR);
  write_script(&dir, "interval.ts", INTERVAL);
  write_script(&dir, "zero-timeout.ts", ZERO_TIMEOUT);
  write_script(&dir, "zero-interval.ts", ZERO_INTERVAL);
  write_script(&dir, "zero-held.ts", ZERO_HELD);
  for _ in 0..2 {
    assert_eq!(timer_stdout(&dir, "unref.ts").trim(), "");
    assert_eq!(timer_stdout(&dir, "ref.ts").trim(), "timeout");
    assert_eq!(timer_stdout(&dir, "clear.ts").trim(), "held");
    assert_eq!(timer_stdout(&dir, "interval.ts").trim(), "once");
    assert_eq!(timer_stdout(&dir, "zero-timeout.ts").trim(), "");
    assert_eq!(timer_stdout(&dir, "zero-interval.ts").trim(), "");
    assert_eq!(timer_stdout(&dir, "zero-held.ts").trim(), "due\nhold");
  }
}
