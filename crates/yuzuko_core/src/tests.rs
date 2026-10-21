use std::path::PathBuf;

use crate::runtime::event_loop_waits;
use crate::runtime::run_scripts_after;
use crate::{run_file, run_scripts};

fn workspace() -> PathBuf {
  PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn promise_microtask_runs_before_the_next_macrotask() {
  let logs = run_scripts(&[
    "console.log('sync'); Promise.resolve().then(() => console.log('micro'));",
    "console.log('next');",
  ])
  .expect("scripts");
  assert_eq!(logs, vec!["sync", "micro", "next"]);
}

#[test]
fn foreground_task_wakes_the_event_loop() {
  let before = event_loop_waits();
  let logs = run_scripts(&[
        r#"
        console.log("before");
        const buffer = new SharedArrayBuffer(4);
        const view = new Int32Array(buffer);
        Atomics.waitAsync(view, 0, 0, 150).value.then(() => console.log("woke"));
        "#,
    ])
    .expect("waitAsync");
  assert_eq!(logs, vec!["before", "woke"]);
  assert!(
    event_loop_waits() > before,
    "the event loop returned without waiting for the foreground task"
  );
}

#[test]
fn hello_runs_a_relative_typescript_import() {
  run_file(&workspace().join("tests/cli/hello.ts")).expect("hello.ts");
}

#[test]
fn javascript_extension_resolves_to_typescript() {
  run_file(&workspace().join("tests/cli/via_js.ts")).expect("via_js.ts");
}

#[test]
fn thrown_error_names_the_typescript_file_and_line() {
  let error =
    run_file(&workspace().join("tests/cli/throw.ts")).expect_err("throw.ts");
  assert!(
    error.message.contains("throw.ts"),
    "stack should name the file: {}",
    error.message
  );
  assert!(
    error.message.contains(":2"),
    "stack should name the line: {}",
    error.message
  );
}

#[test]
fn timer_runs_after_microtasks() {
  let logs = run_scripts(&[r#"
    console.log("sync");
    queueMicrotask(() => console.log("micro"));
    setTimeout(() => console.log("timer"), 0);
    "#])
  .expect("timers");
  assert_eq!(logs, vec!["sync", "micro", "timer"]);
}

#[test]
fn wasm_compile_resolves_a_module() {
  let logs = run_scripts(&[r#"
    const bytes = new Uint8Array([
      0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00,
    ]);
    Promise.all([
      WebAssembly.compile().then(
        () => "missing-ok",
        (error) => "missing-" + error.name,
      ),
      WebAssembly.compile(new Uint8Array()).then(
        () => "empty-ok",
        (error) => "empty-" + error.name,
      ),
      WebAssembly.compile(bytes).then(
        (module) => "module-" + (module instanceof WebAssembly.Module),
        (error) => "module-" + error.name,
      ),
    ]).then((results) => console.log(results.join(" ")));
    "#])
  .expect("wasm compile");
  assert_eq!(
    logs,
    vec!["missing-TypeError empty-CompileError module-true"]
  );
}

#[test]
fn wasm_direct_import_calls_an_export() {
  run_file(&workspace().join("tests/wasm/direct.ts")).expect("direct wasm");
}

#[test]
fn wasm_import_of_typescript_resolves() {
  run_file(&workspace().join("tests/wasm/import_ts.ts")).expect("wasm ts");
}

#[test]
fn wasm_source_import_does_not_evaluate_dependencies() {
  run_file(&workspace().join("tests/wasm/source.ts")).expect("import source");
}

#[test]
fn wasm_source_import_of_typescript_is_rejected() {
  let error = run_file(&workspace().join("tests/wasm/source_js.ts"))
    .expect_err("import source of TypeScript");
  assert!(error.message.contains("WebAssembly"), "{}", error.message);
}

#[test]
fn realm_globals_cover_phase_one_basics() {
  let logs = run_scripts(&[r#"
    const original = { n: 1, nested: { ok: true } };
    const copy = structuredClone(original);
    original.nested.ok = false;
    console.log(copy.n);
    console.log(copy.nested.ok);
    console.log(typeof navigator.userAgent);
    console.log(typeof performance.now);
    console.log(self === globalThis);
    "#])
  .expect("realm");
  assert_eq!(logs, vec!["1", "true", "string", "function", "true"]);
}

#[test]
fn unhandled_rejection_fails() {
  let error =
    run_file(&workspace().join("tests/cli/reject.ts")).expect_err("reject.ts");
  assert!(
    error.message.contains("nope"),
    "rejection should include the reason: {}",
    error.message
  );
}

struct Counter {
  value: std::cell::Cell<i32>,
}

unsafe impl v8::cppgc::GarbageCollected for Counter {
  fn trace(&self, _visitor: &mut v8::cppgc::Visitor) {}

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"Counter"
  }
}

#[yuzuko_macros::op]
impl Counter {
  #[constructor]
  fn new(start: Option<i32>) -> Counter {
    Counter {
      value: std::cell::Cell::new(start.unwrap_or(0)),
    }
  }

  #[getter]
  fn value(&self) -> i32 {
    self.value.get()
  }

  #[setter]
  fn value(&self, value: i32) {
    self.value.set(value);
  }

  fn bump(&self, by: i32) -> i32 {
    let next = self.value.get() + by;
    self.value.set(next);
    next
  }

  #[static_method]
  fn answer() -> i32 {
    42
  }
}

#[repr(C)]
struct Child {
  base: Counter,
  extra: std::cell::Cell<i32>,
}

unsafe impl v8::cppgc::GarbageCollected for Child {
  fn trace(&self, visitor: &mut v8::cppgc::Visitor) {
    v8::cppgc::GarbageCollected::trace(&self.base, visitor);
  }

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"Child"
  }
}

#[yuzuko_macros::op(inherit = Counter)]
impl Child {
  #[constructor]
  fn new(start: Option<i32>) -> Child {
    Child {
      base: Counter {
        value: std::cell::Cell::new(start.unwrap_or(0)),
      },
      extra: std::cell::Cell::new(7),
    }
  }

  #[getter]
  fn extra(&self) -> i32 {
    self.extra.get()
  }
}

#[test]
fn op_class_wraps_a_cppgc_object() {
  let logs = run_scripts_after(
    Counter::install,
    &[r#"
      const counter = new Counter(1);
      if (counter.value !== 1) throw new Error("start");
      counter.value = 4;
      if (counter.bump(2) !== 6) throw new Error("bump");
      if (Counter.answer() !== 42) throw new Error("static");
      if (!(counter instanceof Counter)) throw new Error("instanceof");
      let illegal = false;
      try {
        Counter.prototype.bump.call({});
      } catch (error) {
        illegal = error instanceof TypeError;
      }
      if (!illegal) throw new Error("receiver");
      let constructed = false;
      try {
        Counter();
      } catch (error) {
        constructed = error instanceof TypeError;
      }
      if (!constructed) throw new Error("new");
      console.log("ok");
    "#],
  )
  .expect("op class");
  assert_eq!(logs, vec!["ok"]);
}

#[test]
fn op_subclass_reuses_the_base_methods() {
  let logs = run_scripts_after(
    |scope| {
      Counter::install(scope);
      Child::install(scope);
    },
    &[r#"
      const child = new Child(3);
      if (child.value !== 3) throw new Error("value");
      if (child.bump(1) !== 4) throw new Error("bump");
      if (child.extra !== 7) throw new Error("extra");
      if (!(child instanceof Child)) throw new Error("child");
      if (!(child instanceof Counter)) throw new Error("base");
      console.log("ok");
    "#],
  )
  .expect("subclass");
  assert_eq!(logs, vec!["ok"]);
}
