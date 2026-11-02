use std::path::PathBuf;
use std::process::Command;

fn workspace() -> PathBuf {
  PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn kind(enabled: bool) -> &'static str {
  if enabled { "function" } else { "undefined" }
}

fn object(enabled: bool) -> &'static str {
  if enabled { "object" } else { "undefined" }
}

fn text(enabled: bool) -> &'static str {
  if enabled { "string" } else { "undefined" }
}

fn expected() -> String {
  let main = [
    "function",
    "function",
    "function",
    "function",
    "function",
    "function",
    "function",
    "function",
    "object",
    "function",
    "function",
    "function",
    kind(cfg!(feature = "fetch")),
    kind(cfg!(feature = "socket")),
    kind(cfg!(feature = "fs")),
    kind(cfg!(feature = "udp")),
    kind(cfg!(feature = "udp")),
    kind(cfg!(feature = "tcp")),
    kind(cfg!(feature = "tcp")),
    kind(cfg!(feature = "tcp")),
    kind(cfg!(feature = "tcp")),
    kind(cfg!(feature = "window")),
    kind(cfg!(feature = "window")),
    kind(cfg!(feature = "window")),
    kind(cfg!(feature = "image")),
    kind(cfg!(feature = "canvas")),
    text(cfg!(feature = "canvas")),
    object(cfg!(feature = "webgpu")),
    kind(cfg!(feature = "audio")),
    kind(cfg!(feature = "codec")),
    kind(cfg!(feature = "codec")),
    "object",
    "object",
    "function",
    "function",
    "function",
  ]
  .join(" ");
  let worker = [
    "function",
    kind(cfg!(feature = "fetch")),
    kind(cfg!(feature = "socket")),
    kind(cfg!(feature = "fs")),
    kind(cfg!(feature = "udp")),
    kind(cfg!(feature = "udp")),
    kind(cfg!(feature = "tcp")),
    kind(cfg!(feature = "tcp")),
    kind(cfg!(feature = "tcp")),
    kind(cfg!(feature = "tcp")),
    "undefined",
    kind(cfg!(feature = "window")),
    "function",
    "undefined",
    "object",
    "object",
    "function",
    "function",
    "function",
  ]
  .join(" ");
  format!("{main}\n{worker}")
}

#[test]
fn runtime_features_select_the_surface() {
  let output = Command::new(env!("CARGO_BIN_EXE_yuzuko"))
    .current_dir(workspace())
    .args(["run", "tests/cli/runtime_build.ts"])
    .output()
    .expect("spawn yuzuko");
  let stdout = String::from_utf8_lossy(&output.stdout);
  let stderr = String::from_utf8_lossy(&output.stderr);
  assert_eq!(
    (output.status.code(), stdout.trim()),
    (Some(0), expected().as_str()),
    "stderr:\n{stderr}"
  );
}
