use std::path::PathBuf;
use std::process::Command;
#[cfg(target_os = "linux")]
use std::process::Stdio;
#[cfg(target_os = "linux")]
use std::time::{Duration, Instant};

fn workspace() -> PathBuf {
  PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn context_menu_click_passes_under_yuzuko_test() {
  let output = Command::new(env!("CARGO_BIN_EXE_yuzuko"))
    .current_dir(workspace())
    .args(["test", "-A", "tests/context_menu_test.ts"])
    .env_remove("YUZUKO_HEADLESS")
    .env("YUZUKO_TEST_INPUT", "1")
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

#[cfg(target_os = "linux")]
#[test]
fn show_context_menu_needs_a_gtk_window() {
  let mut child = Command::new(env!("CARGO_BIN_EXE_yuzuko"))
    .current_dir(workspace())
    .args(["run", "tests/fixtures/context_menu_linux.ts"])
    .env_remove("YUZUKO_HEADLESS")
    .env_remove("YUZUKO_SKIP_GPU")
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .spawn()
    .expect("spawn yuzuko");
  let start = Instant::now();
  let status = loop {
    if let Some(status) = child.try_wait().expect("poll context menu") {
      break status;
    }
    if start.elapsed() > Duration::from_secs(12) {
      let _ = child.kill();
      panic!("context menu did not return");
    }
    std::thread::sleep(Duration::from_millis(40));
  };
  let output = child.wait_with_output().expect("context menu output");
  let stdout = String::from_utf8_lossy(&output.stdout);
  let stderr = String::from_utf8_lossy(&output.stderr);
  assert!(status.success(), "{stdout}\n{stderr}");
  assert_eq!(
    stdout,
    "NotSupportedError\nThis window system has no GTK window to host a context menu.\n",
    "{stderr}"
  );
}
