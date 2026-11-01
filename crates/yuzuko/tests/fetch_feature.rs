use std::path::PathBuf;
use std::process::Command;

fn workspace() -> PathBuf {
  PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn types() -> String {
  let output = Command::new(env!("CARGO_BIN_EXE_yuzuko"))
    .current_dir(workspace())
    .args(["run", "tests/cli/fetch_build.ts"])
    .output()
    .expect("spawn yuzuko");
  assert_eq!(
    output.status.code(),
    Some(0),
    "{}",
    String::from_utf8_lossy(&output.stderr)
  );
  String::from_utf8_lossy(&output.stdout).trim().to_string()
}

#[test]
fn fetch_surface_follows_its_features() {
  let http = if cfg!(feature = "fetch") {
    "function function function function object function function"
  } else {
    "undefined undefined undefined undefined undefined undefined undefined"
  };
  let sockets = if cfg!(feature = "socket") {
    "function function function"
  } else {
    "undefined undefined undefined"
  };
  assert_eq!(types(), format!("{http} function {sockets}"));
}
