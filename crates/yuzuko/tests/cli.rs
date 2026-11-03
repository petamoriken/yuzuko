use std::fs;
use std::path::{Path, PathBuf};
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

fn is_lavapipe_banner(line: &str) -> bool {
  line
    .to_ascii_lowercase()
    .contains("lavapipe is not a conformant vulkan")
}

/// Mesa prints this when Linux opens lavapipe, which has no GPU.
fn without_lavapipe_banner(text: &str) -> String {
  let lines: Vec<&str> = text.lines().collect();
  if !lines.iter().copied().any(is_lavapipe_banner) {
    return text.to_string();
  }
  lines
    .into_iter()
    .filter(|line| !line.trim().is_empty() && !is_lavapipe_banner(line))
    .collect::<Vec<_>>()
    .join("\n")
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
    let raw_stderr = text(&output.stderr);
    let stderr = without_lavapipe_banner(&raw_stderr);
    let stdout = text(&output.stdout);
    assert!(
      status.is_none(),
      "{name} exited {:?}\n{raw_stderr}{stdout}",
      status.and_then(|code| code.code())
    );
    assert!(
      stderr.is_empty() && stdout.is_empty(),
      "{name}\n{raw_stderr}{stdout}"
    );
  }
}

#[test]
fn lavapipe_banner_is_not_example_output() {
  let banner = concat!(
    "WARNING: lavapipe is not a conformant vulkan implementation, ",
    "testing use only.\n"
  );
  assert_eq!(without_lavapipe_banner(banner), "");
  let mesa = concat!(
    "lavapipe: warning: lavapipe is not a conformant Vulkan ",
    "implementation, testing use only."
  );
  assert_eq!(without_lavapipe_banner(&format!("\n{mesa}\n\n")), "");
  assert_eq!(
    without_lavapipe_banner(&format!("{banner}real error\n")),
    "real error"
  );
  assert_eq!(without_lavapipe_banner("real error\n"), "real error\n");
  assert_eq!(without_lavapipe_banner("\n"), "\n");
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
    // A hang never exits. The limit is long enough for a loaded machine.
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

fn scratch(name: &str) -> PathBuf {
  let dir = std::env::temp_dir().join(format!(
    "yuzuko-process-{}-{}-{name}",
    std::process::id(),
    std::time::SystemTime::now()
      .duration_since(std::time::UNIX_EPOCH)
      .unwrap_or_default()
      .as_nanos()
  ));
  let _ = fs::remove_dir_all(&dir);
  fs::create_dir_all(&dir).unwrap();
  dir
}

fn run_deadline(
  args: &[&str],
  dir: &Path,
  env: &[(&str, &str)],
  limit: Duration,
) -> Output {
  let mut command = Command::new(env!("CARGO_BIN_EXE_yuzuko"));
  command.current_dir(dir).args(args);
  for (key, value) in env {
    command.env(key, value);
  }
  let mut child = command
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .spawn()
    .expect("spawn yuzuko");
  let start = Instant::now();
  loop {
    if child.try_wait().expect("poll yuzuko").is_some() {
      return child.wait_with_output().expect("yuzuko output");
    }
    if start.elapsed() > limit {
      let _ = child.kill();
      let output = child.wait_with_output().expect("killed yuzuko");
      panic!(
        "timed out after {limit:?}\nstdout:\n{}\nstderr:\n{}",
        text(&output.stdout),
        text(&output.stderr)
      );
    }
    std::thread::sleep(Duration::from_millis(20));
  }
}

fn field<'a>(stdout: &'a str, prefix: &str) -> &'a str {
  stdout
    .lines()
    .find_map(|line| line.strip_prefix(prefix))
    .unwrap_or_else(|| panic!("missing {prefix} in {stdout}"))
}

fn same_directory(printed: &str, expected: &Path) {
  let printed = fs::canonicalize(printed)
    .unwrap_or_else(|error| panic!("canonicalize {printed}: {error}"));
  let expected = fs::canonicalize(expected).unwrap();
  assert_eq!(printed, expected);
}

#[test]
fn run_records_script_arguments() {
  let script = "tests/cli/print_args.ts";
  let cases = [
    (
      &["run", script, "--", "--fullscreen", "a"][..],
      "[\"--fullscreen\",\"a\"]\nTypeError\n",
    ),
    (
      &["run", script, "--fullscreen"][..],
      "[\"--fullscreen\"]\nTypeError\n",
    ),
    (&["run", script, "--", "--"][..], "[\"--\"]\nTypeError\n"),
    (
      &["run", "--", script, "--", "x"][..],
      "[\"x\"]\nTypeError\n",
    ),
  ];
  for (args, expected) in cases {
    let output = yuzuko(args);
    assert_eq!(
      output.status.code(),
      Some(0),
      "{args:?}\n{}",
      text(&output.stderr)
    );
    assert_eq!(text(&output.stdout), expected, "{args:?}");
  }

  let missing = yuzuko(&["run", "--"]);
  assert_eq!(missing.status.code(), Some(2));
  assert!(
    text(&missing.stderr).contains("yuzuko run <entry.ts>"),
    "{}",
    text(&missing.stderr)
  );

  let dir = scratch("flag-entry");
  fs::write(
    dir.join("-entry.ts"),
    "console.log(JSON.stringify(Yuzuko.args));\n",
  )
  .unwrap();
  let ran = run_deadline(
    &["run", "--", "-entry.ts", "extra"],
    &dir,
    &[],
    Duration::from_secs(20),
  );
  assert_eq!(ran.status.code(), Some(0), "{}", text(&ran.stderr));
  assert_eq!(text(&ran.stdout), "[\"extra\"]\n");
  let rejected =
    run_deadline(&["run", "-entry.ts"], &dir, &[], Duration::from_secs(20));
  assert_eq!(rejected.status.code(), Some(2));
  assert!(
    text(&rejected.stderr).contains("unknown flag -entry.ts"),
    "{}",
    text(&rejected.stderr)
  );
}

#[test]
fn exit_returns_the_requested_status() {
  let script = "tests/cli/process_exit.ts";
  let limit = Duration::from_secs(20);
  let root = workspace();
  let run = |args: &[&str]| run_deadline(args, &root, &[], limit);

  let three = run(&["run", script, "code", "3"]);
  assert_eq!(three.status.code(), Some(3), "{}", text(&three.stderr));
  assert!(
    !text(&three.stdout).contains("after"),
    "{}",
    text(&three.stdout)
  );

  let extra = run(&["run", script, "code", "2"]);
  assert_eq!(extra.status.code(), Some(2), "{}", text(&extra.stderr));
  assert!(!text(&extra.stdout).contains("after"));

  let high = run(&["run", script, "code", "255"]);
  assert_eq!(high.status.code(), Some(255), "{}", text(&high.stderr));

  for mode in ["omit", "negzero", "undef"] {
    let output = run(&["run", script, mode]);
    assert_eq!(
      output.status.code(),
      Some(0),
      "{mode}\n{}",
      text(&output.stderr)
    );
    assert!(
      !text(&output.stdout).contains("after"),
      "{mode}\n{}",
      text(&output.stdout)
    );
  }

  let caught = run(&["run", script, "catch"]);
  assert_eq!(caught.status.code(), Some(4), "{}", text(&caught.stderr));
  let stdout = text(&caught.stdout);
  assert!(!stdout.contains("caught"), "{stdout}");
  assert!(!stdout.contains("still"), "{stdout}");
  assert!(!stdout.contains("after"), "{stdout}");

  let timed = run(&["run", script, "timer"]);
  assert_eq!(timed.status.code(), Some(7), "{}", text(&timed.stderr));

  let bad = run(&["run", script, "bad"]);
  assert_eq!(bad.status.code(), Some(0), "{}", text(&bad.stderr));
  let stdout = text(&bad.stdout);
  assert!(stdout.contains("stayed\n"), "{stdout}");
  assert!(
    stdout.contains("The exit code must be an integer from 0 through 255."),
    "{stdout}"
  );
  assert!(!stdout.contains("exited"), "{stdout}");
  assert_eq!(
    stdout
      .lines()
      .filter(|line| {
        *line == "The exit code must be an integer from 0 through 255."
      })
      .count(),
    11,
    "{stdout}"
  );

  for mode in ["worker", "worker-module"] {
    let output = run(&["run", script, mode]);
    assert_eq!(
      output.status.code(),
      Some(9),
      "{mode}\nstdout:\n{}\nstderr:\n{}",
      text(&output.stdout),
      text(&output.stderr)
    );
    let stdout = text(&output.stdout);
    assert!(!stdout.contains("worker-error"), "{stdout}");
    assert!(!stdout.contains("worker-message"), "{stdout}");
    assert!(!stdout.contains("worker-ran"), "{stdout}");
  }
}

#[test]
fn process_info_reads_env_and_changes_directory() {
  let script = workspace().join("tests/cli/process_info.ts");
  let script = script.to_str().unwrap();
  let info = run_deadline(
    &["run", script, "info"],
    &workspace(),
    &[("YUZUKO_PROCESS_PROBE", "from-parent")],
    Duration::from_secs(20),
  );
  assert_eq!(info.status.code(), Some(0), "{}", text(&info.stderr));
  let stdout = text(&info.stdout);
  assert_eq!(field(&stdout, "probe:"), "from-parent");
  assert_eq!(field(&stdout, "missing:"), "undefined");
  assert_eq!(field(&stdout, "lengths:"), "1,2,1,1,0,0,1,0");
  assert_eq!(field(&stdout, "has:"), "true");
  assert_eq!(field(&stdout, "get:"), "round");
  assert_eq!(field(&stdout, "snap:"), "round");
  assert_eq!(field(&stdout, "after:"), "undefined");
  assert_eq!(field(&stdout, "empty:"), "\"\"");
  assert_eq!(
    field(&stdout, "empty-name:"),
    "TypeError:The name must not be empty."
  );
  assert_eq!(
    field(&stdout, "equals:"),
    "TypeError:The name must not contain `=` or NUL."
  );
  assert_eq!(
    field(&stdout, "nul-name:"),
    "TypeError:The name must not contain `=` or NUL."
  );
  assert_eq!(
    field(&stdout, "nul-value:"),
    "TypeError:The value must not contain NUL."
  );
  assert_eq!(
    field(&stdout, "name-type:"),
    "TypeError:The name must be a string."
  );
  assert_eq!(
    field(&stdout, "value-type:"),
    "TypeError:The value must be a string."
  );
  assert_eq!(field(&stdout, "ok:"), "delete-missing");
  let case = field(&stdout, "case:");
  if cfg!(windows) {
    assert_eq!(case, "CaseValue");
  } else {
    assert_eq!(case, "undefined");
  }
  assert_eq!(field(&stdout, "proto-own:"), "true");
  assert_eq!(field(&stdout, "proto-value:"), "proto-value");
  assert_eq!(field(&stdout, "proto-same:"), "true");
  assert_eq!(field(&stdout, "worker-args:"), "[\"info\"]");
  same_directory(field(&stdout, "cwd:"), &workspace());

  let dir = scratch("chdir");
  fs::create_dir_all(dir.join("nested")).unwrap();
  fs::write(dir.join("nested/note.txt"), b"noted").unwrap();
  let changed = run_deadline(
    &["run", script, "chdir"],
    &dir,
    &[],
    Duration::from_secs(20),
  );
  assert_eq!(changed.status.code(), Some(0), "{}", text(&changed.stderr));
  let stdout = text(&changed.stdout);
  same_directory(field(&stdout, "before:"), &dir);
  same_directory(field(&stdout, "nested:"), &dir.join("nested"));
  same_directory(field(&stdout, "back:"), &dir);
  same_directory(field(&stdout, "empty:"), &dir);
  same_directory(field(&stdout, "dot:"), &dir);
  assert_eq!(field(&stdout, "note:"), "noted");
  assert_eq!(
    field(&stdout, "missing:"),
    "NotFoundError:The directory was not found."
  );
  assert_eq!(
    field(&stdout, "file:"),
    "InvalidStateError:The path is not a directory."
  );
  assert_eq!(
    field(&stdout, "nul:"),
    "TypeError:The path must not contain NUL."
  );
  assert_eq!(
    field(&stdout, "https:"),
    "TypeError:The URL scheme must be file."
  );
  assert_eq!(
    field(&stdout, "type:"),
    "TypeError:The path must be a string or a file URL."
  );
  assert_eq!(field(&stdout, "ok:"), "url");
}

#[test]
fn test_records_empty_args_and_exit_skips_the_summary() {
  let listed = yuzuko(&["test", "tests/process_args_test.ts"]);
  assert_eq!(listed.status.code(), Some(0), "{}", text(&listed.stderr));
  let stdout = text(&listed.stdout);
  assert!(
    stdout.contains("ok | 1 passed | 0 failed | 0 ignored"),
    "{stdout}"
  );

  let stopped = run_deadline(
    &["test", "tests/cli/exit_from_runner.ts"],
    &workspace(),
    &[],
    Duration::from_secs(20),
  );
  assert_eq!(stopped.status.code(), Some(6), "{}", text(&stopped.stderr));
  let stdout = text(&stopped.stdout);
  let stderr = text(&stopped.stderr);
  assert!(!stdout.contains("ok |"), "{stdout}");
  assert!(!stdout.contains("failed |"), "{stdout}");
  assert!(!stderr.contains("exit_from_runner"), "{stderr}");
  assert!(
    !stderr.to_ascii_lowercase().contains("uncaught"),
    "{stderr}"
  );
}

#[test]
fn console_inspects_url_headers_and_search_params() {
  let output = yuzuko(&["run", "tests/cli/console_inspect.js"]);
  assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
  let stdout = text(&output.stdout);
  assert!(
    stdout.contains(
      "URL {\n  href: \"https://example.com/a\",\n  origin: \"https://example.com\",\n  protocol: \"https:\",\n  username: \"\",\n  password: \"\",\n  host: \"example.com\",\n  hostname: \"example.com\",\n  port: \"\",\n  pathname: \"/a\",\n  hash: \"\",\n  search: \"\"\n}"
    ),
    "{stdout}"
  );
  let extra = stdout
    .split("extra\n")
    .nth(1)
    .unwrap_or("")
    .split("nested\n")
    .next()
    .unwrap_or("");
  assert!(!extra.contains("extra"), "{extra}");
  assert!(!extra.contains("searchParams"), "{extra}");
  assert!(stdout.contains("https://example.com/a"), "{stdout}");
  assert!(
    stdout.contains("Headers { a: \"3\", b: \"1, 3\", m: \"2\", z: \"1\" }"),
    "{stdout}"
  );
  assert!(
    stdout
      .contains("Headers { \"content-type\": \"text/plain\", \"x-a\": \"1\" }"),
    "{stdout}"
  );
  assert!(stdout.contains("Headers {}"), "{stdout}");
  assert!(
    stdout.contains("URLSearchParams { \"a\" => \"1\", \"b\" => \"2\" }"),
    "{stdout}"
  );
  assert!(
    stdout.contains(
      "URLSearchParams {\n  \"a\" => \"1\",\n  \"b\" => \"2\",\n  \"c\" => \"3\",\n  \"d\" => \"4\",\n  \"e\" => \"5\",\n  \"f\" => \"6\",\n  \"g\" => \"7\",\n  \"h\" => \"8\" }"
    ),
    "{stdout}"
  );
  assert!(
    stdout.contains("URLSearchParams { \"z\" => \"1\", \"a\" => \"2\" }"),
    "{stdout}"
  );
  assert!(stdout.contains("URLSearchParams {}"), "{stdout}");
  assert!(stdout.contains("MyURL {"), "{stdout}");
  assert!(stdout.contains("MyHeaders { a: \"1\" }"), "{stdout}");
  assert!(stdout.contains("MyParams { \"a\" => \"1\" }"), "{stdout}");
  assert!(
    stdout.contains("\n0\n") || stdout.ends_with("0\n"),
    "{stdout}"
  );
  assert!(!stdout.contains("Symbol("), "{stdout}");
  assert!(!stdout.contains("hook"), "{stdout}");
}

fn section<'a>(text: &'a str, name: &str) -> &'a str {
  let marker = format!("MARK {name}\n");
  let rest = text.split(&marker).nth(1).unwrap_or("");
  rest.split("\nMARK ").next().unwrap_or(rest)
}

#[test]
fn console_inspects_host_instances() {
  let output = yuzuko(&["run", "tests/cli/console_instances.js"]);
  assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
  let stdout = text(&output.stdout);
  let controller = section(&stdout, "controller");
  assert!(
    controller.contains(
      "AbortController {\n  signal: AbortSignal { aborted: false, reason: undefined, onabort: null }\n}"
    ),
    "{controller}"
  );
  let stopped = section(&stdout, "stopped");
  assert!(stopped.contains("aborted: true"), "{stopped}");
  assert!(stopped.contains("reason: \"stop\""), "{stopped}");
  assert!(stopped.contains("onabort: null"), "{stopped}");
  let subclass = section(&stdout, "subclass");
  assert!(subclass.contains("MyController {"), "{subclass}");
  assert!(subclass.contains("signal:"), "{subclass}");
  assert!(subclass.contains("more: 7"), "{subclass}");
  assert!(!subclass.contains("extra"), "{subclass}");
  let cycle = section(&stdout, "cycle");
  assert!(cycle.contains("<ref *1>"), "{cycle}");
  assert!(cycle.contains("[Circular *1]"), "{cycle}");
  assert!(cycle.contains("aborted: true"), "{cycle}");
  let prototype = section(&stdout, "prototype");
  assert!(prototype.contains("AbortController {"), "{prototype}");
  assert!(prototype.contains("signal: [Getter]"), "{prototype}");
  assert!(!prototype.contains("aborted:"), "{prototype}");
  let unbound = section(&stdout, "unbound");
  assert!(unbound.contains("signal: [Getter]"), "{unbound}");
  assert!(!unbound.contains("AbortSignal {"), "{unbound}");
  let event = section(&stdout, "event");
  assert!(event.contains("bubbles: true"), "{event}");
  assert!(event.contains("type: \"go\""), "{event}");
  assert!(event.contains("cancelable: false"), "{event}");
  assert!(
    event.find("bubbles:").unwrap() < event.find("type:").unwrap(),
    "{event}"
  );
  let custom = section(&stdout, "custom");
  assert!(custom.contains("detail: \"x\""), "{custom}");
  let myevent = section(&stdout, "myevent");
  assert!(myevent.contains("MyEvent {"), "{myevent}");
  assert!(myevent.contains("more: 3"), "{myevent}");
  assert!(!myevent.contains("extra"), "{myevent}");
  let message = section(&stdout, "message");
  assert!(message.contains("data: \"hi\""), "{message}");
  assert!(
    message.contains("origin: \"https://example.com\""),
    "{message}"
  );
  assert!(message.contains("lastEventId: \"1\""), "{message}");
  assert!(message.contains("source: null"), "{message}");
  assert!(message.contains("ports: []"), "{message}");
  let close = section(&stdout, "close");
  assert!(close.contains("wasClean: true"), "{close}");
  assert!(close.contains("code: 1000"), "{close}");
  assert!(close.contains("reason: \"bye\""), "{close}");
  let progress = section(&stdout, "progress");
  assert!(progress.contains("lengthComputable: true"), "{progress}");
  assert!(progress.contains("loaded: 3"), "{progress}");
  assert!(progress.contains("total: 8"), "{progress}");
  let pointer = section(&stdout, "pointer");
  assert!(pointer.contains("clientX: 4"), "{pointer}");
  assert!(pointer.contains("pointerId: 7"), "{pointer}");
  assert!(pointer.contains("bubbles: true"), "{pointer}");
  assert!(
    pointer.find("bubbles:").unwrap() < pointer.find("clientX:").unwrap(),
    "{pointer}"
  );
  assert_eq!(
    section(&stdout, "blob").trim(),
    "Blob { size: 2, type: \"text/plain\" }"
  );
  let file = section(&stdout, "file");
  assert!(file.contains("name: \"a.txt\""), "{file}");
  assert!(file.contains("lastModified:"), "{file}");
  assert!(file.contains("size: 2"), "{file}");
  assert!(file.contains("type: \"text/plain\""), "{file}");
  let myfile = section(&stdout, "myfile");
  assert!(myfile.contains("MyFile {"), "{myfile}");
  assert!(myfile.contains("more: 5"), "{myfile}");
  assert!(myfile.contains("name: \"a.txt\""), "{myfile}");
  assert!(myfile.contains("lastModified:"), "{myfile}");
  assert!(!myfile.contains("extra"), "{myfile}");
  assert_eq!(
    section(&stdout, "encoder").trim(),
    "TextEncoder { encoding: \"utf-8\" }"
  );
  assert_eq!(
    section(&stdout, "decoder").trim(),
    "TextDecoder { encoding: \"utf-8\", fatal: false, ignoreBOM: false }"
  );
  let request = section(&stdout, "request");
  assert!(request.contains("method: \"GET\""), "{request}");
  assert!(request.contains("bodyUsed: false"), "{request}");
  assert!(request.contains("redirect: \"follow\""), "{request}");
  assert!(request.contains("https://example.com"), "{request}");
  assert!(request.contains("Headers"), "{request}");
  assert!(request.contains("cache: \"no-store\""), "{request}");
  assert!(request.contains("duplex: \"half\""), "{request}");
  assert!(request.contains("signal: AbortSignal {"), "{request}");
  assert!(request.contains("body: null"), "{request}");
  let response = section(&stdout, "response");
  assert!(response.contains("status: 200"), "{response}");
  assert!(response.contains("ok: true"), "{response}");
  assert!(response.contains("ReadableStream {"), "{response}");
  assert!(response.contains("locked: false"), "{response}");
  assert!(response.contains("type: \"default\""), "{response}");
  assert_eq!(section(&stdout, "target").trim(), "EventTarget { a: 1 }");
  assert!(
    section(&stdout, "performance").contains("timeOrigin:"),
    "{stdout}"
  );
  let channel = section(&stdout, "channel");
  assert!(channel.contains("port1:"), "{channel}");
  assert!(channel.contains("port2:"), "{channel}");
  assert!(channel.contains("MessagePort {"), "{channel}");
  assert!(channel.contains("onmessage: null"), "{channel}");
  let reader = section(&stdout, "reader");
  assert!(reader.contains("readyState: 0"), "{reader}");
  assert!(reader.contains("result: null"), "{reader}");
  assert!(reader.contains("error: null"), "{reader}");
  assert_eq!(
    section(&stdout, "point").trim(),
    "DOMPoint { x: 1, y: 2, z: 3, w: 4 }"
  );
  let exception = section(&stdout, "exception");
  assert!(exception.contains("NotFoundError: boom"), "{exception}");
  assert!(!exception.contains("DOMException {"), "{exception}");
  let shadow = section(&stdout, "shadow");
  assert!(shadow.contains("signal: [Getter]"), "{shadow}");
  assert!(
    shadow.contains("\n0\n") || shadow.trim_end().ends_with("\n0"),
    "{shadow}"
  );
  let hook = section(&stdout, "hook");
  assert!(hook.contains("AbortSignal {"), "{hook}");
  assert!(!hook.contains("hook"), "{hook}");
  assert!(
    hook.contains("\n0\n") || hook.trim_end().ends_with("\n0"),
    "{hook}"
  );
  let deep = section(&stdout, "deep");
  assert!(deep.contains("AbortSignal {"), "{deep}");
  let deeper = section(&stdout, "deeper");
  assert!(deeper.contains("[Object]"), "{deeper}");
  assert!(!deeper.contains("AbortSignal"), "{deeper}");
}
