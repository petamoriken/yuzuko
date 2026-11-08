use std::path::PathBuf;
use std::process::Command;

fn workspace() -> PathBuf {
  PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn canvas_composites_and_shadows_pass() {
  let output = Command::new(env!("CARGO_BIN_EXE_yuzuko"))
    .current_dir(workspace())
    .args(["test", "-A", "tests/canvas_composite_test.ts"])
    .env("YUZUKO_CANVAS", "cpu")
    .env_remove("YUZUKO_HEADLESS")
    .output()
    .expect("spawn yuzuko");
  let stdout = String::from_utf8_lossy(&output.stdout);
  let stderr = String::from_utf8_lossy(&output.stderr);
  assert_eq!(output.status.code(), Some(0), "{stdout}\n{stderr}");
  assert!(
    stdout.contains("ok | 17 passed | 0 failed | 0 ignored"),
    "{stdout}\n{stderr}"
  );
}
