use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn workspace() -> PathBuf {
  PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn text(bytes: &[u8]) -> String {
  String::from_utf8_lossy(bytes).into_owned()
}

fn yuzuko(args: &[&str], skip_gpu: bool) -> std::process::Output {
  let mut command = Command::new(env!("CARGO_BIN_EXE_yuzuko"));
  command.current_dir(workspace()).args(args);
  command.env_remove("YUZUKO_HEADLESS");
  command.env_remove("YUZUKO_CANVAS");
  if skip_gpu {
    command.env("YUZUKO_SKIP_GPU", "1");
  } else {
    command.env_remove("YUZUKO_SKIP_GPU");
  }
  command.output().expect("spawn yuzuko")
}

#[test]
fn webgpu_draws_when_an_adapter_is_available() {
  let output = yuzuko(&["test", "tests/webgpu_test.ts"], false);
  let stdout = text(&output.stdout);
  let stderr = text(&output.stderr);
  assert!(output.status.success(), "{stdout}\n{stderr}");
  assert!(
    stdout.contains("webgpu-ran"),
    "adapter was null\n{stdout}\n{stderr}"
  );
  assert!(!stdout.contains("webgpu-skipped"), "{stdout}");
  assert!(stdout.contains("6 passed"), "{stdout}\n{stderr}");
}

#[test]
fn skip_gpu_resolves_a_null_adapter() {
  let output = yuzuko(&["test", "tests/webgpu_test.ts"], true);
  let stdout = text(&output.stdout);
  let stderr = text(&output.stderr);
  assert!(output.status.success(), "{stdout}\n{stderr}");
  assert!(stdout.contains("webgpu-skipped"), "{stdout}\n{stderr}");
  assert!(!stdout.contains("webgpu-ran"), "{stdout}");
  assert!(stdout.contains("6 passed"), "{stdout}\n{stderr}");
}

#[test]
fn two_surfaces_present_webgpu() {
  let mut command = Command::new(env!("CARGO_BIN_EXE_yuzuko"));
  command
    .current_dir(workspace())
    .args(["run", "tests/fixtures/show_webgpu.ts"])
    .env_remove("YUZUKO_HEADLESS")
    .env_remove("YUZUKO_SKIP_GPU")
    .env_remove("YUZUKO_CANVAS")
    .stdout(Stdio::piped())
    .stderr(Stdio::piped());
  let mut child = command.spawn().expect("spawn yuzuko");
  let start = Instant::now();
  let status = loop {
    if let Some(status) = child.try_wait().expect("poll window") {
      break status;
    }
    if start.elapsed() > Duration::from_secs(20) {
      let _ = child.kill();
      panic!("WebGPU windows did not close");
    }
    std::thread::sleep(Duration::from_millis(30));
  };
  let output = child.wait_with_output().expect("window output");
  let stdout = text(&output.stdout);
  let stderr = text(&output.stderr);
  assert!(status.success(), "{stderr}\n{stdout}");
  assert!(stdout.contains("presented\n"), "{stdout}\n{stderr}");
  assert!(stdout.contains("window-closed\n"), "{stdout}");
  assert!(!stdout.contains("draw-failed"), "{stdout}\n{stderr}");
}
