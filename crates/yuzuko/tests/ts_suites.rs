//! Runs the `tests/*_test.ts` files that no other harness runs. Each test
//! names the features its file uses, so a smaller build skips it.

#![allow(dead_code)]

use std::path::PathBuf;
use std::process::Command;

fn workspace() -> PathBuf {
  PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

struct Suite<'a> {
  file: &'a str,
  flags: &'a [&'a str],
  env: &'a [(&'a str, &'a str)],
  passed: usize,
}

const ALL: &[&str] = &["-A"];

fn run(suite: Suite<'_>) {
  let mut command = Command::new(env!("CARGO_BIN_EXE_yuzuko"));
  command
    .current_dir(workspace())
    .arg("test")
    .args(suite.flags)
    .arg(format!("tests/{}", suite.file))
    .env("YUZUKO_CANVAS", "cpu")
    .env_remove("YUZUKO_HEADLESS");
  for (key, value) in suite.env {
    command.env(key, value);
  }
  let output = command.output().expect("spawn yuzuko");
  let stdout = String::from_utf8_lossy(&output.stdout);
  let stderr = String::from_utf8_lossy(&output.stderr);
  assert_eq!(output.status.code(), Some(0), "{stdout}\n{stderr}");
  let summary = format!("ok | {} passed | 0 failed | 0 ignored", suite.passed);
  assert!(stdout.contains(&summary), "{stdout}\n{stderr}");
}

fn simple(file: &str, passed: usize) {
  run(Suite {
    file,
    flags: ALL,
    env: &[],
    passed,
  });
}

#[test]
fn build() {
  simple("build_test.ts", 2);
}

#[test]
#[cfg(feature = "canvas")]
fn canvas_metrics() {
  simple("canvas_metrics_test.ts", 4);
}

#[test]
#[cfg(all(feature = "child", feature = "fs"))]
fn child() {
  simple("child_test.ts", 9);
}

#[test]
fn compression() {
  simple("compression_test.ts", 1);
}

#[test]
fn defer_eager() {
  simple("defer_eager_test.ts", 1);
}

#[test]
fn defer_static() {
  simple("defer_static_test.ts", 9);
}

#[test]
#[cfg(all(feature = "ffi", feature = "child", feature = "fs"))]
fn ffi() {
  simple("ffi_test.ts", 3);
}

#[test]
#[cfg(all(feature = "window", feature = "fs"))]
fn file_drag() {
  // No read grant, so the test can see that a dropped file stays unreadable
  // through the file system API.
  run(Suite {
    file: "file_drag_test.ts",
    flags: &["--allow-write"],
    env: &[("YUZUKO_TEST_INPUT", "1")],
    passed: 5,
  });
}

#[test]
#[cfg(all(feature = "http", feature = "tcp"))]
fn http() {
  simple("http_test.ts", 3);
}

#[test]
#[cfg(all(feature = "image", feature = "canvas"))]
fn image() {
  simple("image_test.ts", 5);
}

#[test]
fn intl() {
  simple("intl_test.ts", 1);
}

#[test]
#[cfg(feature = "fetch")]
fn progress() {
  simple("progress_test.ts", 5);
}

#[test]
#[cfg(feature = "audio")]
fn proto() {
  simple("proto_test.ts", 2);
}

#[test]
#[cfg(feature = "signal")]
fn signal() {
  simple("signal_test.ts", 2);
}

#[test]
#[cfg(feature = "tcp")]
fn tcp() {
  simple("tcp_test.ts", 2);
}

#[test]
fn text_bytes() {
  simple("text_bytes_test.ts", 5);
}

#[test]
#[cfg(feature = "udp")]
fn udp() {
  simple("udp_test.ts", 2);
}

#[test]
#[cfg(feature = "window")]
fn wake_lock() {
  simple("wake_lock_test.ts", 2);
}

#[test]
#[cfg(all(
  feature = "webgpu",
  feature = "codec",
  feature = "audio",
  feature = "canvas",
  feature = "fetch"
))]
fn webidl_convert() {
  simple("webidl_convert_test.ts", 6);
}

#[test]
#[cfg(feature = "socket")]
fn webtransport() {
  simple("webtransport_test.ts", 3);
}
