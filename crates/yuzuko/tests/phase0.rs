use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

fn workspace() -> PathBuf {
  PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn yuzuko(args: &[&str]) -> Output {
  Command::new(env!("CARGO_BIN_EXE_yuzuko"))
    .current_dir(workspace())
    .args(args)
    .output()
    .expect("spawn yuzuko")
}

fn text(bytes: &[u8]) -> String {
  String::from_utf8_lossy(bytes).into_owned()
}

#[test]
fn hello_prints_the_imported_greeting() {
  let output = yuzuko(&["run", "tests/cli/hello.ts"]);
  assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
  assert_eq!(text(&output.stdout), "hello from greeting\n");
}

#[test]
fn javascript_specifier_prefers_the_typescript_sibling() {
  let output = yuzuko(&["run", "tests/cli/via_js.ts"]);
  assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
  assert_eq!(text(&output.stdout), "hello from greeting\n");
}

#[test]
fn run_does_not_install_yuzuko_test() {
  let output = yuzuko(&["run", "tests/cli/typeof_yuzuko.ts"]);
  assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
  assert_eq!(
    text(&output.stdout),
    "undefined\nundefined\nfunction\nundefined\n"
  );
}

#[test]
fn test_installs_inject_only_when_enabled() {
  let hidden = Command::new(env!("CARGO_BIN_EXE_yuzuko"))
    .current_dir(workspace())
    .args(["test", "tests/inject_absent_test.ts"])
    .env_remove("YUZUKO_TEST_INPUT")
    .output()
    .expect("spawn yuzuko");
  assert_eq!(hidden.status.code(), Some(0), "{}", text(&hidden.stderr));
  assert!(
    text(&hidden.stdout).contains("ok | 1 passed | 0 failed | 0 ignored"),
    "{}",
    text(&hidden.stdout)
  );

  let missing = Command::new(env!("CARGO_BIN_EXE_yuzuko"))
    .current_dir(workspace())
    .args(["test", "tests/inject_present_test.ts"])
    .env_remove("YUZUKO_TEST_INPUT")
    .output()
    .expect("spawn yuzuko");
  assert_eq!(missing.status.code(), Some(1), "{}", text(&missing.stdout));

  let shown = Command::new(env!("CARGO_BIN_EXE_yuzuko"))
    .current_dir(workspace())
    .args(["test", "tests/inject_present_test.ts"])
    .env("YUZUKO_TEST_INPUT", "1")
    .output()
    .expect("spawn yuzuko");
  assert_eq!(shown.status.code(), Some(0), "{}", text(&shown.stderr));
  assert!(
    text(&shown.stdout).contains("ok | 1 passed | 0 failed | 0 ignored"),
    "{}",
    text(&shown.stdout)
  );

  let unexpected = Command::new(env!("CARGO_BIN_EXE_yuzuko"))
    .current_dir(workspace())
    .args(["test", "tests/inject_absent_test.ts"])
    .env("YUZUKO_TEST_INPUT", "1")
    .output()
    .expect("spawn yuzuko");
  assert_eq!(
    unexpected.status.code(),
    Some(1),
    "{}",
    text(&unexpected.stdout)
  );
}

#[test]
fn thrown_stack_names_the_typescript_file() {
  let output = yuzuko(&["run", "tests/cli/throw.ts"]);
  assert_eq!(output.status.code(), Some(1));
  let stderr = text(&output.stderr);
  assert!(stderr.contains("throw.ts"), "{stderr}");
  assert!(stderr.contains(":2"), "{stderr}");
}

#[test]
fn unhandled_rejection_exits_1() {
  let output = yuzuko(&["run", "tests/cli/reject.ts"]);
  assert_eq!(output.status.code(), Some(1));
  assert!(
    text(&output.stderr).contains("nope"),
    "{}",
    text(&output.stderr)
  );
}

#[test]
fn window_examples_stay_open() {
  for name in ["hello.ts", "breakout.ts", "paint.ts", "clock.ts"] {
    let path = format!("examples/{name}");
    let mut child = Command::new(env!("CARGO_BIN_EXE_yuzuko"))
      .current_dir(workspace())
      .args(["run", path.as_str()])
      .env("YUZUKO_HEADLESS", "1")
      .stdout(Stdio::piped())
      .stderr(Stdio::piped())
      .spawn()
      .unwrap_or_else(|error| panic!("spawn {name}: {error}"));
    let started = Instant::now();
    let status = loop {
      match child.try_wait() {
        Ok(Some(status)) => break Some(status),
        Ok(None) if started.elapsed() >= Duration::from_secs(2) => break None,
        Ok(None) => std::thread::sleep(Duration::from_millis(50)),
        Err(error) => panic!("{name}: {error}"),
      }
    };
    let _ = child.kill();
    let output = child
      .wait_with_output()
      .unwrap_or_else(|error| panic!("wait {name}: {error}"));
    let stderr = text(&output.stderr);
    let stdout = text(&output.stdout);
    assert!(
      status.is_none(),
      "{name} exited {:?}\n{stderr}{stdout}",
      status.and_then(|code| code.code())
    );
    assert!(
      stderr.is_empty() && stdout.is_empty(),
      "{name}\n{stderr}{stdout}"
    );
  }
}

#[test]
fn passing_tests_exit_0() {
  let output = yuzuko(&["test", "tests/pass_test.ts"]);
  assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
  let stdout = text(&output.stdout);
  assert!(stdout.contains("ok | addition\n"), "{stdout}");
  assert!(stdout.contains("ok | step\n"), "{stdout}");
  assert!(
    stdout.contains("ok | 2 passed | 0 failed | 0 ignored\n"),
    "{stdout}"
  );
}

#[test]
fn failing_tests_exit_1_and_print_the_summary() {
  let output = yuzuko(&["test", "tests/fail_test.ts"]);
  assert_eq!(output.status.code(), Some(1));
  let stdout = text(&output.stdout);
  assert!(stdout.contains("failed | fails\n"), "{stdout}");
  assert!(
    stdout.contains("ok | 0 passed | 1 failed | 0 ignored\n"),
    "{stdout}"
  );
  assert!(text(&output.stderr).contains("expected failure"));
}

#[test]
fn ignored_tests_do_not_fail() {
  let output = yuzuko(&["test", "tests/ignore_test.ts"]);
  assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
  let stdout = text(&output.stdout);
  assert!(stdout.contains("ignored | later\n"), "{stdout}");
  assert!(stdout.contains("ok | runs\n"), "{stdout}");
  assert!(
    stdout.contains("ok | 1 passed | 0 failed | 1 ignored\n"),
    "{stdout}"
  );
}

#[test]
fn only_exits_1_after_a_passing_run() {
  let output = yuzuko(&["test", "tests/only_test.ts"]);
  assert_eq!(output.status.code(), Some(1));
  let stdout = text(&output.stdout);
  assert!(stdout.contains("ignored | skipped\n"), "{stdout}");
  assert!(stdout.contains("ok | focused\n"), "{stdout}");
  assert!(
    text(&output.stderr)
      .contains("error: Test failed because the \"only\" option was used"),
    "{}",
    text(&output.stderr)
  );
}

#[test]
fn nested_test_throws_type_error() {
  let output = yuzuko(&["test", "tests/nested_test.ts"]);
  assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
  assert!(text(&output.stdout).contains("ok | nested\n"));
}

#[test]
fn filter_keeps_matching_names() {
  let output = yuzuko(&["test", "tests/pass_test.ts", "--filter", "addition"]);
  assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
  let stdout = text(&output.stdout);
  assert!(stdout.contains("ok | addition\n"), "{stdout}");
  assert!(!stdout.contains("step"), "{stdout}");
  assert!(
    stdout.contains("ok | 1 passed | 0 failed | 0 ignored\n"),
    "{stdout}"
  );
}

#[test]
fn pollution_tests_pass() {
  let output = yuzuko(&["test", "tests/pollution_test.ts"]);
  assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
  let stdout = text(&output.stdout);
  assert!(
    stdout.contains("ok | 8 passed | 0 failed | 0 ignored\n"),
    "{stdout}"
  );
}

#[test]
fn text_encoder_is_a_cppgc_class() {
  let output = yuzuko(&["test", "tests/encoding_test.ts"]);
  assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
  let stdout = text(&output.stdout);
  assert!(
    stdout.contains("ok | text encoder and decoder\n"),
    "{stdout}"
  );
}

#[test]
fn abort_signal_any_keeps_source_and_dependent_weak() {
  let output = yuzuko(&["test", "tests/abort_test.ts"]);
  assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
  let stdout = text(&output.stdout);
  assert!(
    stdout.contains("ok | 2 passed | 0 failed | 0 ignored\n"),
    "{stdout}"
  );
}

#[test]
fn structured_clone_copies_blob_file_and_dom_exception() {
  let output = yuzuko(&["test", "tests/clone_test.ts"]);
  assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
  let stdout = text(&output.stdout);
  assert!(
    stdout
      .contains("ok | structuredClone copies Blob, File, and DOMException\n"),
    "{stdout}"
  );
}

#[test]
fn microtask_runs_before_a_timer() {
  let output = yuzuko(&["test", "tests/order_test.ts"]);
  assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
  let stdout = text(&output.stdout);
  assert!(
    stdout.contains("ok | microtask runs before a timer\n"),
    "{stdout}"
  );
}

#[test]
fn usage_errors_exit_2() {
  // No command starts the REPL, which ends with status 0 at stdin EOF.
  assert_eq!(yuzuko(&[]).status.code(), Some(0));
  assert_eq!(yuzuko(&["run"]).status.code(), Some(2));
  let output = yuzuko(&["wpt"]);
  assert_eq!(output.status.code(), Some(2));
  assert!(
    text(&output.stderr).contains("unknown command wpt"),
    "{}",
    text(&output.stderr)
  );
}

#[test]
fn delayed_v8_housekeeping_does_not_keep_the_process_alive() {
  let mut child = Command::new(env!("CARGO_BIN_EXE_yuzuko"))
    .current_dir(workspace())
    .args(["run", "tests/fixtures/allocate_exit.ts"])
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .spawn()
    .expect("spawn yuzuko");
  let start = Instant::now();
  let status = loop {
    if let Some(status) = child.try_wait().expect("poll yuzuko") {
      break status;
    }
    // Without the fix the process never exits, so a generous limit still
    // catches it on a loaded machine.
    if start.elapsed() > Duration::from_secs(15) {
      let _ = child.kill();
      panic!("the script finished but the process stayed alive");
    }
    std::thread::sleep(Duration::from_millis(20));
  };
  let output = child.wait_with_output().expect("yuzuko output");
  assert!(status.success(), "{}", text(&output.stderr));
  assert_eq!(text(&output.stdout).trim(), "allocated");
}
