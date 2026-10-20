use std::path::PathBuf;
use std::process::Command;

fn workspace() -> PathBuf {
  PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn webcodecs_passes_under_yuzuko_test() {
  let output = Command::new(env!("CARGO_BIN_EXE_yuzuko"))
    .current_dir(workspace())
    .args(["test", "tests/webcodecs_test.ts"])
    .env_remove("YUZUKO_HEADLESS")
    .env_remove("YUZUKO_SKIP_GPU")
    .output()
    .expect("spawn yuzuko");
  let stdout = String::from_utf8_lossy(&output.stdout);
  let stderr = String::from_utf8_lossy(&output.stderr);
  assert_eq!(output.status.code(), Some(0), "{stdout}\n{stderr}");
  // Without a hardware H.264 decoder, the four picture tests are ignored.
  // macOS always has VideoToolbox.
  let summary = if cfg!(target_os = "macos") {
    vec!["ok | 9 passed | 0 failed | 0 ignored"]
  } else {
    vec![
      "ok | 9 passed | 0 failed | 0 ignored",
      "ok | 5 passed | 0 failed | 4 ignored",
    ]
  };
  assert!(
    summary.iter().any(|line| stdout.contains(line)),
    "{stdout}\n{stderr}"
  );
}
