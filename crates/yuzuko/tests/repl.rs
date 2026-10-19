//! `yuzuko repl` through the real binary. Tab and Up need a terminal.

use std::fs;
use std::io::{Read, Write};
#[cfg(unix)]
use std::os::unix::io::FromRawFd;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

fn bin() -> PathBuf {
  PathBuf::from(env!("CARGO_BIN_EXE_yuzuko"))
}

fn text(bytes: &[u8]) -> String {
  String::from_utf8_lossy(bytes).into_owned()
}

fn scratch(name: &str) -> PathBuf {
  let dir = std::env::temp_dir().join(format!(
    "yuzuko-repl-{}-{}-{name}",
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

fn printed_line(text: &str, line: &str) -> bool {
  text.lines().any(|item| item == line)
}

fn printed_id(text: &str, prefix: &str) -> bool {
  text.lines().any(|line| {
    let Some(rest) = line.trim().strip_prefix(prefix) else {
      return false;
    };
    !rest.is_empty() && rest.chars().all(|ch| ch.is_ascii_digit())
  })
}

/// Runs `script` after the first prompt. The duration starts once that prompt
/// is visible, so process startup is not counted as timer delay.
fn eval_after_prompt(dir: &Path, script: &str) -> (String, String, Duration) {
  let mut child = Command::new(bin())
    .current_dir(dir)
    .arg("repl")
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .spawn()
    .expect("spawn yuzuko repl");
  let mut stdin = child.stdin.take().unwrap();
  let mut stdout = child.stdout.take().unwrap();
  let mut stderr = child.stderr.take().unwrap();
  let mut seen = String::new();
  let startup = Instant::now();
  let mut byte = [0u8; 1];
  while !seen.contains("> ") {
    if startup.elapsed() > Duration::from_secs(20) {
      panic!("no prompt\n{seen}");
    }
    let count = stdout.read(&mut byte).unwrap();
    assert_ne!(count, 0, "eof before prompt\n{seen}");
    seen.push(byte[0] as char);
  }
  let started = Instant::now();
  stdin.write_all(script.as_bytes()).unwrap();
  drop(stdin);
  let mut rest = Vec::new();
  stdout.read_to_end(&mut rest).unwrap();
  let mut err = Vec::new();
  stderr.read_to_end(&mut err).unwrap();
  let status = child.wait().unwrap();
  assert_eq!(status.code(), Some(0), "{}", text(&err));
  seen.push_str(&text(&rest));
  (seen, text(&err), started.elapsed())
}

fn pipe_repl(dir: &Path, input: &str) -> Output {
  let mut child = Command::new(bin())
    .current_dir(dir)
    .arg("repl")
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .spawn()
    .expect("spawn yuzuko repl");
  child
    .stdin
    .take()
    .unwrap()
    .write_all(input.as_bytes())
    .unwrap();
  child.wait_with_output().expect("repl output")
}

#[test]
fn yuzuko_without_a_command_starts_the_repl() {
  let dir = scratch("bare");
  let mut child = Command::new(bin())
    .current_dir(&dir)
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .spawn()
    .expect("spawn yuzuko");
  child.stdin.take().unwrap().write_all(b"1 + 1\n").unwrap();
  let output = child.wait_with_output().expect("repl output");
  assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
  assert!(
    text(&output.stdout).contains('2'),
    "stdout: {}\nstderr: {}",
    text(&output.stdout),
    text(&output.stderr)
  );
}

#[test]
fn help_prints_usage_without_a_session() {
  let output = Command::new(bin())
    .arg("--help")
    .output()
    .expect("spawn yuzuko --help");
  assert_eq!(output.status.code(), Some(0));
  let stderr = text(&output.stderr);
  assert!(stderr.contains("yuzuko run <entry.ts>"), "{stderr}");
  assert!(stderr.contains("yuzuko repl"), "{stderr}");
  assert!(
    !text(&output.stdout).contains("> "),
    "stdout started a repl: {}",
    text(&output.stdout)
  );
}

#[test]
fn future_timers_do_not_block_the_next_line() {
  let dir = scratch("future");
  let script = concat!(
    "\"id:\" + setTimeout(() => console.log(\"later-timer\"), 5000)\n",
    "1 + 1\n",
    "\"iv:\" + setInterval(() => console.log(\"later-interval\"), 5000)\n",
    "6 + 1\n",
    "setTimeout(() => console.log(\"due-timer\"), 0)\n",
    "await new Promise((r) => setTimeout(() => r(\"zero-await\"), 0))\n",
  );
  let (seen, err, elapsed) = eval_after_prompt(&dir, script);
  assert!(
    elapsed < Duration::from_secs(1),
    "repl blocked for {elapsed:?}"
  );
  assert!(printed_id(&seen, "id:"), "{seen}");
  assert!(printed_id(&seen, "iv:"), "{seen}");
  assert!(seen.contains("\n2\n"), "{seen}");
  assert!(seen.contains("\n7\n"), "{seen}");
  assert!(seen.contains("due-timer"), "{seen}");
  assert!(seen.contains("zero-await"), "{seen}");
  assert!(!seen.contains("later-timer"), "{seen}");
  assert!(!seen.contains("later-interval"), "{seen}");
  assert!(!format!("{seen}{err}").contains("panicked"));
}

#[test]
fn zero_delay_rearm_does_not_block_the_next_line() {
  let dir = scratch("rearm");
  let script = concat!(
    "\"a:\" + setInterval(() => {}, 0)\n",
    "1 + 1\n",
    "\"b:\" + setInterval(() => {})\n",
    "1 + 1\n",
    "\"c:\" + setTimeout(function f(){ setTimeout(f, 0) }, 0)\n",
    "1 + 1\n",
    "setTimeout(() => console.log(\"due-still\"), 0)\n",
  );
  let (seen, err, elapsed) = eval_after_prompt(&dir, script);
  assert!(
    elapsed < Duration::from_secs(1),
    "repl blocked for {elapsed:?}\n{seen}"
  );
  assert!(printed_id(&seen, "a:"), "{seen}");
  assert!(printed_id(&seen, "b:"), "{seen}");
  assert!(printed_id(&seen, "c:"), "{seen}");
  assert!(
    seen.matches("\n2\n").count() >= 3,
    "next lines did not run\n{seen}"
  );
  assert!(seen.contains("due-still"), "{seen}");
  assert!(!format!("{seen}{err}").contains("panicked"));
}

#[test]
fn piped_repl_prints_two_on_two_launches() {
  let dir = scratch("pipe");
  for _ in 0..2 {
    let output = pipe_repl(&dir, "1 + 1\n");
    assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
    assert!(
      text(&output.stdout).contains('2'),
      "stdout: {}\nstderr: {}",
      text(&output.stdout),
      text(&output.stderr)
    );
    let combined = format!("{}{}", text(&output.stdout), text(&output.stderr));
    assert!(!combined.contains("panicked"), "{combined}");
  }
}

#[cfg(unix)]
struct Pty {
  primary: i32,
  child: std::process::Child,
  text: String,
}

#[cfg(unix)]
impl Pty {
  fn spawn(dir: &Path) -> Self {
    Self::open(dir, false)
  }

  fn open(dir: &Path, headless: bool) -> Self {
    let mut primary = 0;
    let mut secondary = 0;
    let opened = unsafe {
      libc::openpty(
        &mut primary,
        &mut secondary,
        std::ptr::null_mut(),
        std::ptr::null_mut(),
        std::ptr::null_mut(),
      )
    };
    assert_eq!(opened, 0, "openpty");
    let mut command = Command::new(bin());
    command.arg("repl").current_dir(dir);
    if headless {
      command.env("YUZUKO_HEADLESS", "1");
    }
    let child = command
      .stdin(unsafe { Stdio::from_raw_fd(libc::dup(secondary)) })
      .stdout(unsafe { Stdio::from_raw_fd(libc::dup(secondary)) })
      .stderr(unsafe { Stdio::from_raw_fd(libc::dup(secondary)) })
      .spawn()
      .expect("spawn repl");
    unsafe { libc::close(secondary) };
    Self {
      primary,
      child,
      text: String::new(),
    }
  }

  fn send(&mut self, bytes: &[u8]) {
    let mut offset = 0;
    while offset < bytes.len() {
      let wrote = unsafe {
        libc::write(
          self.primary,
          bytes[offset..].as_ptr().cast(),
          bytes.len() - offset,
        )
      };
      assert!(wrote > 0, "pty write\n{}", self.text);
      offset += wrote as usize;
    }
  }

  fn pull(&mut self, timeout_ms: i32) {
    let mut poll = libc::pollfd {
      fd: self.primary,
      events: libc::POLLIN,
      revents: 0,
    };
    let ready = unsafe { libc::poll(&mut poll, 1, timeout_ms) };
    if ready <= 0 {
      return;
    }
    let mut buf = [0u8; 4096];
    let count =
      unsafe { libc::read(self.primary, buf.as_mut_ptr().cast(), buf.len()) };
    if count > 0 {
      let chunk =
        String::from_utf8_lossy(&buf[..count as usize]).replace('\r', "");
      self.text.push_str(&chunk);
    }
  }

  fn wait_until(&mut self, pred: impl Fn(&str) -> bool, timeout: Duration) {
    let start = Instant::now();
    while start.elapsed() < timeout {
      if pred(&self.text) {
        return;
      }
      if let Some(status) = self.child.try_wait().unwrap() {
        self.pull(50);
        panic!("repl exited {:?}\n{}", status.code(), self.text);
      }
      self.pull(100);
    }
    panic!("timeout\n{}", self.text);
  }

  fn wait_exit(&mut self) -> i32 {
    let start = Instant::now();
    loop {
      if let Some(status) = self.child.try_wait().unwrap() {
        self.pull(50);
        return status.code().unwrap_or(1);
      }
      if start.elapsed() > Duration::from_secs(10) {
        let _ = self.child.kill();
        panic!("repl did not exit\n{}", self.text);
      }
      self.pull(100);
    }
  }
}

#[cfg(unix)]
impl Drop for Pty {
  fn drop(&mut self) {
    unsafe { libc::close(self.primary) };
    if self.child.try_wait().ok().flatten().is_none() {
      let _ = self.child.kill();
      let _ = self.child.wait();
    }
  }
}

#[cfg(unix)]
#[test]
fn terminal_session_completes_and_recalls_history() {
  let dir = scratch("term");
  fs::write(
    dir.join("mod.ts"),
    "export const mark = \"repl-import-mark\";\n",
  )
  .unwrap();
  let mut pty = Pty::spawn(&dir);
  let step = Duration::from_secs(15);
  pty.wait_until(|text| text.contains("> "), step);

  pty.send(b"console.log(\"repl-log\")\n");
  pty.wait_until(|text| printed_line(text, "repl-log"), step);

  pty.send(b"\"yuzuko:\" + typeof Yuzuko\n");
  pty.wait_until(|text| text.contains("yuzuko:object"), step);

  pty.send(b"\"test:\" + typeof Yuzuko.test\n");
  pty.wait_until(|text| text.contains("test:undefined"), step);

  pty.send(b"1 + 1\n");
  pty.wait_until(|text| text.contains("\n2\n"), step);

  let before = pty.text.len();
  pty.send(b"const labeled: number = 3\n");
  pty.wait_until(|text| text[before..].contains("> "), step);
  let before = pty.text.len();
  pty.send(b"labeled\n");
  pty.wait_until(|text| text[before..].contains("\n3\n"), step);

  let before = pty.text.len();
  pty.send(b"await Promise.resolve(7)\n");
  pty.wait_until(|text| text[before..].contains("\n7\n"), step);

  pty.send(b"(await import(\"./mod.ts\")).mark\n");
  pty.wait_until(|text| text.contains("repl-import-mark"), step);

  pty.send(b"queueMicrotask(() => console.log(\"repl-micro\"))\n");
  pty.wait_until(|text| printed_line(text, "repl-micro"), step);

  pty.send(b"setTimeout(() => console.log(\"repl-timer\"), 0)\n");
  pty.wait_until(|text| printed_line(text, "repl-timer"), step);

  pty.send(
    b"await new Promise((r) => setTimeout(() => r(\"zero-await\"), 0))\n",
  );
  pty.wait_until(|text| printed_line(text, "zero-await"), step);

  pty.send(b"throw new Error(\"repl-boom\")\n");
  pty.wait_until(|text| text.contains("Error: repl-boom"), step);
  let before = pty.text.len();
  pty.send(b"1\n");
  pty.wait_until(|text| text[before..].contains("\n1\n"), step);

  let before = pty.text.len();
  pty.send(b"(1 +\n");
  pty.wait_until(|text| text[before..].contains("... "), step);
  assert!(
    !pty.text[before..].contains("\n2\n"),
    "closed early\n{}",
    &pty.text[before..]
  );
  pty.send(b"1)\n");
  pty.wait_until(|text| text[before..].contains("\n2\n"), step);

  let before = pty.text.len();
  pty.send(b")\n");
  pty.wait_until(|text| text[before..].contains("SyntaxError"), step);
  let before = pty.text.len();
  pty.send(b"9\n");
  pty.wait_until(|text| text[before..].contains("\n9\n"), step);

  let before = pty.text.len();
  pty.send(b"Ma\t");
  pty.wait_until(|text| text[before..].contains("Math"), step);
  pty.send(b"\x03");
  pty.wait_until(|text| text[before..].contains("^C"), step);

  let before = pty.text.len();
  pty.send(b"Math.\t");
  pty.wait_until(|text| text[before..].contains("abs"), step);
  pty.send(b"\x03");
  pty.wait_until(|text| text[before..].contains("^C"), step);

  pty.send(b"const marker = 1\n");
  pty.wait_until(|text| text.contains("const marker = 1"), step);
  let before = pty.text.len();
  pty.send(b"mar\t");
  pty.wait_until(|text| text[before..].contains("marker"), step);
  pty.send(b"\x03");
  pty.wait_until(|text| text[before..].contains("^C"), step);

  let before = pty.text.len();
  pty.send(b"zzz\t\n");
  pty.wait_until(|text| text[before..].contains("ReferenceError"), step);

  let before = pty.text.len();
  pty.send(b"40 + 2\n");
  pty.wait_until(|text| text[before..].contains("\n42\n"), step);
  let seen = pty.text.matches("42").count();
  pty.send(b"\x1b[A\n");
  pty.wait_until(|text| text.matches("42").count() > seen, step);

  let before = pty.text.len();
  pty.send(b"\"id:\" + setTimeout(() => console.log(\"later-timer\"), 5000)\n");
  pty.wait_until(
    |text| printed_id(&text[before..], "id:"),
    Duration::from_secs(1),
  );
  assert_eq!(
    pty.text[before..]
      .lines()
      .filter(|line| *line == "later-timer")
      .count(),
    0,
    "delayed timeout ran\n{}",
    &pty.text[before..]
  );
  let before = pty.text.len();
  pty.send(b"1 + 1\n");
  pty.wait_until(
    |text| text[before..].contains("\n2\n"),
    Duration::from_secs(1),
  );
  assert_eq!(
    pty
      .text
      .lines()
      .filter(|line| *line == "later-timer")
      .count(),
    0,
    "{}",
    pty.text
  );

  let before = pty.text.len();
  pty.send(
    b"\"iv:\" + setInterval(() => console.log(\"later-interval\"), 5000)\n",
  );
  pty.wait_until(
    |text| printed_id(&text[before..], "iv:"),
    Duration::from_secs(1),
  );
  assert_eq!(
    pty.text[before..]
      .lines()
      .filter(|line| *line == "later-interval")
      .count(),
    0,
    "delayed interval ran\n{}",
    &pty.text[before..]
  );
  let before = pty.text.len();
  pty.send(b"6 + 1\n");
  pty.wait_until(
    |text| text[before..].contains("\n7\n"),
    Duration::from_secs(1),
  );
  assert_eq!(
    pty
      .text
      .lines()
      .filter(|line| *line == "later-interval")
      .count(),
    0,
    "{}",
    pty.text
  );

  pty.send(b"\x04");
  let code = pty.wait_exit();
  assert_eq!(code, 0, "{}", pty.text);
  assert!(!pty.text.contains("panicked"), "{}", pty.text);
}

#[cfg(unix)]
#[test]
fn event_loop_runs_while_the_prompt_waits() {
  let dir = scratch("live");
  let mut pty = Pty::open(&dir, true);
  let step = Duration::from_secs(15);
  pty.wait_until(|text| text.contains("> "), step);

  pty.send(b"setTimeout(() => console.log(\"while-waiting\"), 40)\n");
  pty.wait_until(|text| printed_line(text, "while-waiting"), step);

  pty.send(
    b"const w = new Yuzuko.Window(); w.requestAnimationFrame(() => console.log(\"while-frame\"))\n",
  );
  pty.wait_until(|text| printed_line(text, "while-frame"), step);

  let before = pty.text.len();
  pty.send(b"8\n");
  pty.wait_until(|text| text[before..].contains("\n8\n"), step);

  pty.send(b"\x04");
  let code = pty.wait_exit();
  assert_eq!(code, 0, "{}", pty.text);
}

#[cfg(unix)]
#[test]
fn terminal_cursor_keys_edit_before_submit() {
  let dir = scratch("keys");
  let mut pty = Pty::spawn(&dir);
  let step = Duration::from_secs(15);
  pty.wait_until(|text| text.contains("> "), step);

  pty.send(b"12\x1b[D+\n");
  pty.wait_until(
    |text| text.contains("\x1b[J> 12\x1b[3C") && text.contains("\n3\n"),
    step,
  );

  let before = pty.text.len();
  pty.send(b"12\x013\n");
  pty.wait_until(|text| text[before..].contains("\n312\n"), step);

  let before = pty.text.len();
  pty.send(b"12\x01\x053\n");
  pty.wait_until(|text| text[before..].contains("\n123\n"), step);

  let before = pty.text.len();
  pty.send(b"15\x02+\x06\n");
  pty.wait_until(|text| text[before..].contains("\n6\n"), step);

  let before = pty.text.len();
  pty.send(b"9+1\x1bb\x7f\n");
  pty.wait_until(|text| text[before..].contains("\n91\n"), step);

  let before = pty.text.len();
  pty.send(b"99\x157\n");
  pty.wait_until(|text| text[before..].contains("\n7\n"), step);

  let before = pty.text.len();
  pty.send(b"12\x01\x0b4\n");
  pty.wait_until(|text| text[before..].contains("\n4\n"), step);

  let before = pty.text.len();
  pty.send(b"12\x15\x197\n");
  pty.wait_until(|text| text[before..].contains("\n127\n"), step);

  let before = pty.text.len();
  pty.send(b"99 1\x17\n");
  pty.wait_until(|text| text[before..].contains("\n99\n"), step);

  pty.send(b"\x04");
  let code = pty.wait_exit();
  assert_eq!(code, 0, "{}", pty.text);
}
