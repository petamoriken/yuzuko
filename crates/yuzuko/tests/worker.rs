use std::path::PathBuf;
use std::process::Command;

fn workspace() -> PathBuf {
  PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn dedicated_workers_pass_under_yuzuko_test() {
  let output = Command::new(env!("CARGO_BIN_EXE_yuzuko"))
    .current_dir(workspace())
    .args(["test", "tests/worker_test.ts"])
    .env_remove("YUZUKO_HEADLESS")
    .output()
    .expect("spawn yuzuko");
  let stdout = String::from_utf8_lossy(&output.stdout);
  let stderr = String::from_utf8_lossy(&output.stderr);
  println!("{stdout}");
  assert_eq!(output.status.code(), Some(0), "{stdout}\n{stderr}");
  assert!(stdout.contains("ok | "), "{stdout}\n{stderr}");
  assert!(stdout.contains("0 failed"), "{stdout}\n{stderr}");
}
