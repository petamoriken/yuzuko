use std::path::PathBuf;
use std::process::Command;

fn workspace() -> PathBuf {
  PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn gamepad_sequences_pass_under_yuzuko_test() {
  let output = Command::new(env!("CARGO_BIN_EXE_yuzuko"))
    .current_dir(workspace())
    .args(["test", "tests/gamepad_test.ts"])
    .env_remove("YUZUKO_HEADLESS")
    .env("YUZUKO_TEST_INPUT", "1")
    .output()
    .expect("spawn yuzuko");
  let stdout = String::from_utf8_lossy(&output.stdout);
  let stderr = String::from_utf8_lossy(&output.stderr);
  assert_eq!(output.status.code(), Some(0), "{stdout}\n{stderr}");
  assert!(
    stdout.contains("ok | 3 passed | 0 failed | 0 ignored"),
    "{stdout}\n{stderr}"
  );
}
