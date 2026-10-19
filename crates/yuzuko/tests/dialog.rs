use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn workspace() -> PathBuf {
  PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn headless_dialogs_throw_not_supported() {
  let output = Command::new(env!("CARGO_BIN_EXE_yuzuko"))
    .current_dir(workspace())
    .args(["test", "tests/dialog_test.ts"])
    .env_remove("YUZUKO_HEADLESS")
    .output()
    .expect("spawn yuzuko");
  let stdout = String::from_utf8_lossy(&output.stdout);
  let stderr = String::from_utf8_lossy(&output.stderr);
  assert_eq!(output.status.code(), Some(0), "{stdout}\n{stderr}");
  assert!(
    stdout.contains("ok | 1 passed | 0 failed | 0 ignored"),
    "{stdout}\n{stderr}"
  );
}

#[test]
fn macos_dialogs_accept_the_default_button() {
  let mut child = Command::new(env!("CARGO_BIN_EXE_yuzuko"))
    .current_dir(workspace())
    .args(["run", "tests/fixtures/dialog_alert.ts"])
    .env_remove("YUZUKO_HEADLESS")
    .env_remove("YUZUKO_SKIP_GPU")
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .spawn()
    .expect("spawn yuzuko");
  std::thread::sleep(Duration::from_millis(900));
  for _ in 0..3 {
    let _ = Command::new("osascript")
      .arg("-e")
      .arg("tell application \"System Events\" to keystroke return")
      .status();
    std::thread::sleep(Duration::from_millis(500));
  }
  let start = Instant::now();
  let status = loop {
    if let Some(status) = child.try_wait().expect("poll dialog") {
      break status;
    }
    if start.elapsed() > Duration::from_secs(12) {
      let _ = child.kill();
      panic!("dialog did not close");
    }
    std::thread::sleep(Duration::from_millis(40));
  };
  let output = child.wait_with_output().expect("dialog output");
  let stdout = String::from_utf8_lossy(&output.stdout);
  let stderr = String::from_utf8_lossy(&output.stderr);
  assert!(status.success(), "{stdout}\n{stderr}");
  assert_eq!(stdout, "yes\nAda\nafter\n", "{stderr}");
}
