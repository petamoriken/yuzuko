use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn workspace() -> PathBuf {
  PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn yuzuko(args: &[&str]) -> std::process::Output {
  Command::new(env!("CARGO_BIN_EXE_yuzuko"))
    .current_dir(workspace())
    .args(args)
    .env_remove("YUZUKO_HEADLESS")
    .output()
    .expect("spawn yuzuko")
}

fn text(bytes: &[u8]) -> String {
  String::from_utf8_lossy(bytes).into_owned()
}

#[test]
fn headless_env_exits_when_the_last_window_closes() {
  let output = Command::new(env!("CARGO_BIN_EXE_yuzuko"))
    .current_dir(workspace())
    .args(["run", "tests/fixtures/close_last_surface.ts"])
    .env("YUZUKO_HEADLESS", "1")
    .output()
    .expect("spawn yuzuko");
  assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
  assert_eq!(text(&output.stdout), "both\n");
}

#[test]
fn omitted_constructor_size_is_1280_by_720() {
  let output = Command::new(env!("CARGO_BIN_EXE_yuzuko"))
    .current_dir(workspace())
    .args(["run", "tests/fixtures/print_size.ts"])
    .env("YUZUKO_HEADLESS", "1")
    .output()
    .expect("spawn yuzuko");
  assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
  assert_eq!(text(&output.stdout), "1280x720\n10x11\n");
}

#[test]
fn run_rejects_removed_window_flags() {
  let headless =
    yuzuko(&["run", "--headless", "tests/fixtures/close_last_surface.ts"]);
  assert_eq!(headless.status.code(), Some(2));
  assert!(
    text(&headless.stderr).contains("unknown flag --headless"),
    "{}",
    text(&headless.stderr)
  );
  let size =
    yuzuko(&["run", "--size", "400x250", "tests/fixtures/print_size.ts"]);
  assert_eq!(size.status.code(), Some(2));
  assert!(
    text(&size.stderr).contains("unknown flag --size"),
    "{}",
    text(&size.stderr)
  );
}

#[test]
fn input_sequences_pass_under_yuzuko_test() {
  let output = yuzuko(&["test", "tests/window_test.ts"]);
  assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
  let stdout = text(&output.stdout);
  assert!(
    stdout.contains("ok | 9 passed | 0 failed | 0 ignored"),
    "{stdout}\n{}",
    text(&output.stderr)
  );
}

#[test]
fn macos_opens_an_os_window() {
  let mut child = Command::new(env!("CARGO_BIN_EXE_yuzuko"))
    .current_dir(workspace())
    .args(["run", "tests/fixtures/show_window.ts"])
    .env_remove("YUZUKO_HEADLESS")
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .spawn()
    .expect("spawn yuzuko");
  let start = Instant::now();
  let status = loop {
    if let Some(status) = child.try_wait().expect("poll window") {
      break status;
    }
    if start.elapsed() > Duration::from_secs(20) {
      let _ = child.kill();
      panic!("OS window did not close");
    }
    std::thread::sleep(Duration::from_millis(30));
  };
  let output = child.wait_with_output().expect("window output");
  assert!(status.success(), "{}", text(&output.stderr));
  let stdout = text(&output.stdout);
  let mut lines = stdout.lines();
  assert_eq!(lines.next(), Some("320x200"), "{stdout}");
  let surface = lines.next().unwrap_or("");
  let physical = lines.next().unwrap_or("");
  assert_eq!(surface, physical, "{stdout}");
  assert_eq!(lines.next(), Some("true"), "{stdout}");
  let delta = lines.next().unwrap_or("");
  let mut parts = delta.split(',');
  let dx: f64 = parts.next().unwrap_or("").parse().unwrap_or(999.0);
  let dy: f64 = parts.next().unwrap_or("").parse().unwrap_or(999.0);
  assert!(
    (dx - 30.0).abs() < 2.0 && (dy + 10.0).abs() < 2.0,
    "{stdout}"
  );
  assert_eq!(lines.next(), Some("true"), "{stdout}");
  assert_eq!(lines.next(), Some("true"), "{stdout}");
  assert_eq!(lines.next(), Some("true"), "{stdout}");
  assert_eq!(lines.next(), Some("renamed"), "{stdout}");
  assert_eq!(lines.next(), Some("false"), "{stdout}");
  assert_eq!(lines.next(), Some("true"), "{stdout}");
  assert_eq!(lines.next(), Some("330x180"), "{stdout}");
  assert!(stdout.contains("window-closed\n"), "{stdout}");
}
