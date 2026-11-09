use std::path::PathBuf;
use std::process::Command;

fn workspace() -> PathBuf {
  PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn webcodecs_passes_under_yuzuko_test() {
  let output = Command::new(env!("CARGO_BIN_EXE_yuzuko"))
    .current_dir(workspace())
    .args(["test", "-A", "tests/webcodecs_test.ts"])
    .env_remove("YUZUKO_HEADLESS")
    .env_remove("YUZUKO_SKIP_GPU")
    .output()
    .expect("spawn yuzuko");
  let stdout = String::from_utf8_lossy(&output.stdout);
  let stderr = String::from_utf8_lossy(&output.stderr);
  assert_eq!(output.status.code(), Some(0), "{stdout}\n{stderr}");
  // Without a hardware H.264 decoder, the five picture tests are ignored.
  // The two AV1 tests run on every host, using rav1d when the session
  // does not open. macOS always has a VideoToolbox H.264 decoder.
  let summary = if cfg!(target_os = "macos") {
    vec!["ok | 20 passed | 0 failed | 0 ignored"]
  } else {
    vec![
      "ok | 20 passed | 0 failed | 0 ignored",
      "ok | 15 passed | 0 failed | 5 ignored",
    ]
  };
  assert!(
    summary.iter().any(|line| stdout.contains(line)),
    "{stdout}\n{stderr}"
  );
}
