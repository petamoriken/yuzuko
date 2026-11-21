use std::path::PathBuf;
use std::time::Duration;

use crate::runtime::event_loop_waits;
use crate::runtime::run_scripts_after;
use crate::{ClassicPart, ClassicSession, run_file, run_scripts};

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
fn js_source_import_returns_module_source() {
  run_file(&workspace().join("tests/wasm/source_js.ts"))
    .expect("import source of TypeScript");
}

#[test]
fn js_source_import_does_not_evaluate() {
  run_file(&workspace().join("tests/source/no_eval.ts"))
    .expect("source phase skips evaluation");
}

#[test]
fn js_source_import_does_not_load_imports() {
  run_file(&workspace().join("tests/source/missing_dep.ts"))
    .expect("source phase skips imports");
}

#[test]
fn js_source_import_reports_a_syntax_error() {
  let error = run_file(&workspace().join("tests/source/syntax.ts"))
    .expect_err("import source of invalid TypeScript");
  assert!(
    error.message.contains("bad.ts") || error.message.contains("Unexpected"),
    "{}",
    error.message
  );
}

// `tests/defer_static_test.ts` passes under `yuzuko test` in its own
// process. `run_tests` here SIGSEGVs on Linux, SIGBUSes on macOS, and
// access-violates on Windows after other isolates in the same process.

#[test]
fn import_defer_eager_waits_for_namespace_observation() {
  let code =
    crate::run_tests(&[workspace().join("tests/defer_eager_test.ts")], None)
      .expect("defer eager");
  assert_eq!(code, 0);
}

#[test]
fn import_defer_in_javascript_waits() {
  run_file(&workspace().join("tests/defer/js_importer.js")).expect("js defer");
}

#[test]
fn import_defer_rejects_named_imports() {
  let error = run_file(&workspace().join("tests/defer/syntax_named.ts"))
    .expect_err("named defer");
  assert!(
    error.message.contains("deferred import"),
    "{}",
    error.message
  );
}

#[test]
fn import_defer_rejects_a_default_binding() {
  let error = run_file(&workspace().join("tests/defer/syntax_default.ts"))
    .expect_err("default defer");
  assert!(
    error.message.contains("deferred import"),
    "{}",
    error.message
  );
}

#[test]
fn import_defer_reports_a_missing_module() {
  let error = run_file(&workspace().join("tests/defer/missing.ts"))
    .expect_err("missing defer");
  assert!(
    error.message.contains("failed to resolve"),
    "{}",
    error.message
  );
}

#[test]
fn text_and_bytes_modules() {
  let code =
    crate::run_tests(&[workspace().join("tests/text_bytes_test.ts")], None)
      .expect("text and bytes");
  assert_eq!(code, 0);
}

#[test]
fn text_module_named_import_fails_to_link() {
  let error = run_file(&workspace().join("tests/text_bytes/named_text.ts"))
    .expect_err("named text");
  assert!(error.message.contains("value"), "{}", error.message);
}

#[test]
fn bytes_module_named_import_fails_to_link() {
  let error = run_file(&workspace().join("tests/text_bytes/named_bytes.ts"))
    .expect_err("named bytes");
  assert!(error.message.contains("value"), "{}", error.message);
}

#[test]
fn json_module_still_requires_the_attribute() {
  let error = run_file(&workspace().join("tests/text_bytes/json_plain.ts"))
    .expect_err("plain json");
  assert!(
    error.message.contains("type: \"json\""),
    "{}",
    error.message
  );
}

#[test]
fn import_source_of_a_text_file_is_rejected() {
  let error = run_file(&workspace().join("tests/text_bytes/source_text.ts"))
    .expect_err("import source text");
  assert!(error.message.contains("WebAssembly"), "{}", error.message);
}

#[test]
fn import_source_of_a_json_file_is_rejected() {
  let error = run_file(&workspace().join("tests/text_bytes/source_json.ts"))
    .expect_err("import source json");
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
    console.log(typeof window);
    console.log("window" in globalThis);
    "#])
  .expect("realm");
  assert_eq!(
    logs,
    vec![
      "1",
      "true",
      "string",
      "function",
      "true",
      "undefined",
      "false"
    ]
  );
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

fn logs_of(source: &str) -> Vec<String> {
  run_scripts(&[source]).unwrap_or_else(|error| {
    panic!("{}\n{source}", error.message);
  })
}

fn assert_logs(source: &str, expected: &[&str]) {
  let logs = logs_of(source);
  let mut mismatches = Vec::new();
  let len = logs.len().max(expected.len());
  for index in 0..len {
    let actual = logs.get(index).map(String::as_str).unwrap_or("<missing>");
    let want = expected.get(index).copied().unwrap_or("<missing>");
    if actual != want {
      mismatches.push(format!(
        "#{index}\n  actual: {actual:?}\n  expect: {want:?}"
      ));
    }
  }
  assert!(
    mismatches.is_empty(),
    "{}\n--- logs ---\n{}",
    mismatches.join("\n"),
    logs
      .iter()
      .enumerate()
      .map(|(index, line)| format!("{index}: {line}"))
      .collect::<Vec<_>>()
      .join("\n")
  );
}

#[test]
fn console_time_stamp_prints_a_marker() {
  let logs = logs_of(
    r#"
    console.timeStamp();
    console.timeStamp(undefined);
    console.timeStamp("mark", "extra");
    console.timeStamp(null);
    "#,
  );
  assert_eq!(logs.len(), 4, "{logs:?}");
  for line in &logs[..2] {
    assert!(
      line.starts_with("timeStamp @ ") && line.ends_with(" ms"),
      "{line:?}"
    );
  }
  assert!(
    logs[2].starts_with("timeStamp: mark @ ") && logs[2].ends_with(" ms"),
    "{:?}",
    logs[2]
  );
  assert!(!logs[2].contains("extra"), "{:?}", logs[2]);
  assert!(
    logs[3].starts_with("timeStamp: null @ ") && logs[3].ends_with(" ms"),
    "{:?}",
    logs[3]
  );
}

#[test]
fn console_time_stamp_rethrows_to_string() {
  let error = run_scripts(&[r#"
    console.timeStamp({ toString() { throw new Error("boom"); } });
    "#])
  .expect_err("toString");
  assert!(error.message.contains("boom"), "{}", error.message);
}

#[test]
fn console_prints_values() {
  assert_logs(
    r#"
    console.log();
    console.log(undefined);
    console.log(null);
    console.log(-0);
    console.log(true, false);
    console.log(1n);
    console.log([1], { b: 2 });
    console.log({ a: 1 });
    console.log({});
    class Empty {}
    console.log(new Empty());
    class Foo { a = 1 }
    console.log(new Foo());
    class Bar { n = 2 }
    console.log(new Bar());
    class MyArray extends Array {}
    console.log(new MyArray(1, 2));
    class MyMap extends Map {}
    console.log(new MyMap([["k", 1]]));
    class Box { [Symbol.iterator]() {} a = 1 }
    console.log(new Box());
    let calls = 0;
    const hidden = { a: 1 };
    Object.defineProperty(hidden, Symbol.iterator, { value() {} });
    for (const name of [
      "Deno.customInspect",
      "nodejs.util.inspect.custom",
    ]) {
      Object.defineProperty(hidden, Symbol.for(name), {
        value() { calls++; return "hook"; },
      });
    }
    console.log(hidden);
    console.log(calls);
    console.log({ [Symbol("a b")]: 1, [Symbol()]: 2 });
    console.log({ [Symbol("a-b")]: 1, [Symbol("class")]: 2 });
    console.log(Symbol("a b"));
    console.log(Symbol.iterator);
    console.log(Symbol());
    const tagged = { a: 1 };
    Object.defineProperty(tagged, Symbol.toStringTag, {
      get() { throw new Error("tag"); },
    });
    console.log(tagged);
    let gets = 0;
    console.log({ get x() { gets++; return 1; } });
    console.log(gets);
    let both = 0;
    console.log({
      get y() { both++; return 1; },
      set y(_value) { both++; },
    });
    console.log(both);
    let sets = 0;
    console.log({ set z(_value) { sets++; } });
    console.log(sets);
    const parent = { inherited: 1, own: 1 };
    const child = Object.create(parent);
    child.own = 2;
    console.log(child);
    console.log(Object.create({ toString() { return "hello"; } }));
    const cycle = { a: 1 };
    cycle.self = cycle;
    console.log(cycle);
    const cycleArray = [1];
    cycleArray.push(cycleArray);
    console.log(cycleArray);
    const cycleMap = new Map();
    cycleMap.set("k", cycleMap);
    console.log(cycleMap);
    console.log(new Proxy({ a: 1 }, {}));
    console.log(new Proxy(new Proxy([1, 2], {}), {}));
    console.log(Object.create(null));
    const nullNum = Object.create(null);
    nullNum.n = 1;
    console.log(nullNum);
    const widget = Object.create(null);
    widget.n = 1;
    widget[Symbol.toStringTag] = "Widget";
    console.log(widget);
    console.log({ b: 1, "10": "a", "2": "b", "1": "c" });
    console.log({ "": 1, "a b": 2, "3": 4 });
    console.log({ z: 1, a: 2, m: 3 });
    console.log({ a: 1, b: 2, c: 3, d: 4, e: 5, f: 6, g: 7, h: 8 });
    class OnlyGet { get a() { return 1 } }
    console.log(new OnlyGet());
    class Mixed { get a() { return 1 } b = 2 }
    console.log(new Mixed());
    class ParentGet { get a() { return 1 } }
    class ChildGet extends ParentGet {
      get b() { return 2 }
      c = 3
    }
    console.log(new ChildGet());
    class OnlySet { set a(_value) {} }
    console.log(new OnlySet());
    class BothAccessors {
      get a() { return 4 }
      set a(_value) {}
    }
    console.log(new BothAccessors());
    class Methods { m() { return 1 } get a() { return 2 } }
    console.log(new Methods());
    class EmptyProto { m() {} }
    console.log(EmptyProto.prototype);
    console.log(OnlyGet.prototype);
    let protoGets = 0;
    class Counted {
      get a() { protoGets++; return 1 }
    }
    console.log(Counted.prototype);
    console.log(protoGets);
    console.log(new Counted());
    console.log(protoGets);
    let boom = 0;
    class Boom {
      get a() { boom++; throw new Error("nope"); }
    }
    console.log(new Boom());
    console.log(boom);
    class BoomBoth {
      get a() { throw new Error("nope"); }
      set a(_value) {}
    }
    console.log(new BoomBoth());
    const withGetter = Object.create({ get inherited() { return 1 } });
    withGetter.own = 2;
    console.log(withGetter);
    let shadowGets = 0;
    class ThrowsOnProto {
      get a() { shadowGets++; throw new Error("proto"); }
    }
    class ChildThrows extends ThrowsOnProto {}
    console.log(ChildThrows.prototype);
    console.log(shadowGets);
    "#,
    &[
      "",
      "undefined",
      "null",
      "-0",
      "true false",
      "1n",
      "[ 1 ] { b: 2 }",
      "{ a: 1 }",
      "{}",
      "Empty {}",
      "Foo { a: 1 }",
      "Bar { n: 2 }",
      "MyArray(2) [ 1, 2 ]",
      "MyMap(1) [Map] { \"k\" => 1 }",
      "Box { a: 1 }",
      "{ a: 1 }",
      "0",
      "{ Symbol(a b): 1, Symbol(): 2 }",
      "{ Symbol(a-b): 1, Symbol(class): 2 }",
      "Symbol(\"a b\")",
      "Symbol(Symbol.iterator)",
      "Symbol()",
      "{ a: 1 }",
      "{ x: [Getter] }",
      "0",
      "{ y: [Getter/Setter] }",
      "0",
      "{ z: [Setter] }",
      "0",
      "{ own: 2 }",
      "{}",
      "<ref *1> { a: 1, self: [Circular *1] }",
      "<ref *1> [ 1, [Circular *1] ]",
      "<ref *1> Map(1) { \"k\" => [Circular *1] }",
      "{ a: 1 }",
      "[ 1, 2 ]",
      "[Object: null prototype] {}",
      "[Object: null prototype] { n: 1 }",
      "[Object: null prototype] [Widget] {\n  n: 1,\n  Symbol(Symbol.toStringTag): \"Widget\"\n}",
      "{ \"1\": \"c\", \"2\": \"b\", \"10\": \"a\", b: 1 }",
      "{ \"3\": 4, \"\": 1, \"a b\": 2 }",
      "{ z: 1, a: 2, m: 3 }",
      "{ a: 1, b: 2, c: 3, d: 4, e: 5, f: 6, g: 7, h: 8 }",
      "OnlyGet { a: 1 }",
      "Mixed { a: 1, b: 2 }",
      "ChildGet { a: 1, b: 2, c: 3 }",
      "OnlySet { a: [Setter] }",
      "BothAccessors { a: 4 }",
      "Methods { a: 2 }",
      "EmptyProto {}",
      "OnlyGet { a: [Getter] }",
      "Counted { a: [Getter] }",
      "0",
      "Counted { a: 1 }",
      "1",
      "Boom { a: [Getter] }",
      "1",
      "BoomBoth { a: [Getter/Setter] }",
      "{ inherited: 1, own: 2 }",
      "ChildThrows { a: [Getter] }",
      "0",
    ],
  );
}

#[test]
fn console_prints_layout() {
  let quote = {
    let mut text = String::from("{ a: '");
    text.push_str("\\x01\\t\\r\\n");
    text.push('"');
    text.push('\\');
    text.push('\\');
    text.push('\u{2028}');
    text.push_str("' }");
    text
  };
  let logs = logs_of(
    r#"
    console.log([1, 2]);
    console.log([]);
    console.log([1, , 3]);
    console.log([,,,,, 1]);
    console.log(Array(4));
    console.log([,]);
    const extra = [1];
    extra.b = 2;
    console.log(extra);
    console.log([0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11]);
    console.log(Array.from({ length: 120 }, (_, index) => index));
    console.log({ a: "\u0001\t\r\n\"\\\u2028" });
    console.log({ a: { b: { c: { d: { e: { f: 1 } } } } } });
    console.log({ a: { b: { c: { d: { e: {} } } } } });
    console.log({ text: "hello\n" + "x".repeat(80) });
    console.log({ s: "a".repeat(10001) });
    console.log("a".repeat(10001));
    "#,
  );
  assert_eq!(logs[0], "[ 1, 2 ]");
  assert_eq!(logs[1], "[]");
  assert_eq!(logs[2], "[ 1, <1 empty item>, 3 ]");
  assert_eq!(logs[3], "[ <5 empty items>, 1 ]");
  assert_eq!(logs[4], "[ <4 empty items> ]");
  assert_eq!(logs[5], "[ <1 empty item> ]");
  assert_eq!(logs[6], "[ 1, b: 2 ]");
  assert_eq!(
    logs[7],
    "[\n   0,  1, 2, 3, 4,\n   5,  6, 7, 8, 9,\n  10, 11\n]"
  );
  assert!(logs[8].contains("99"), "{}", logs[8]);
  assert!(logs[8].contains("... 20 more items"), "{}", logs[8]);
  assert!(!logs[8].contains(", 100"), "{}", logs[8]);
  assert!(!logs[8].contains(" 100,"), "{}", logs[8]);
  assert!(!logs[8].contains("\n  100"), "{}", logs[8]);
  assert_eq!(logs[9], quote);
  assert_eq!(
    logs[10],
    "{\n  a: {\n    b: { c: { d: { e: [Object] } } }\n  }\n}"
  );
  assert_eq!(
    logs[11],
    "{\n  a: {\n    b: { c: { d: { e: {} } } }\n  }\n}"
  );
  assert!(logs[12].contains(" +\n"), "{}", logs[12]);
  assert!(logs[13].contains("... 1 more character"), "{}", logs[13]);
  assert_eq!(logs[14].len(), 10001, "{}", logs[14].len());
  assert!(!logs[14].contains("more character"), "{}", &logs[14][..40]);
}

#[test]
fn console_prints_collections() {
  let logs = logs_of(
    r#"
    console.log(new Map([["a", 1], [2, "b"]]));
    console.log(new Set([1, "a", { b: 2 }]));
    console.log(new Set());
    console.log(new Map());
    console.log(new WeakMap());
    console.log(new WeakSet());
    console.log(new Map([["a", 1]]).entries());
    console.log(new Map([["a", 1]]).keys());
    console.log(new Set([1]).values());
    console.log([][Symbol.iterator]());
    console.log(new Promise(() => {}));
    console.log(Promise.resolve(1));
    console.log(Promise.resolve(undefined));
    const rejected = Promise.reject(new Error("boom"));
    rejected.catch(() => {});
    console.log(rejected);
    console.log(new Date(0));
    console.log(new Date(NaN));
    const dated = new Date(0);
    dated.x = 1;
    console.log(dated);
    console.log(/a.b/gi);
    const pattern = /a/;
    pattern.x = 1;
    console.log(pattern);
    function foo() {}
    console.log(foo);
    console.log(function () {});
    console.log(foo.bind(null));
    class Foo {}
    console.log(Foo);
    class Bar extends Array {}
    console.log(Bar);
    console.log(class {});
    function named() {}
    named.x = 1;
    console.log(named);
    console.log(Object);
    console.log(Math.abs);
    console.log(parseInt);
    async function af() {}
    console.log(af);
    function* g() {}
    console.log(g);
    async function* ag() {}
    console.log(ag);
    console.log(Object(1));
    console.log(Object(1n));
    console.log(Object(false));
    console.log(Object("ab"));
    console.log(Object(Symbol("a b")));
    const boxed = Object("ab");
    boxed.z = 1;
    console.log(boxed);
    console.log((function () { return arguments; })(1, 2));
    console.log(new Uint8Array([1, 2, 255]));
    console.log(new Uint8Array(0));
    console.log(new BigInt64Array([1n, -2n]));
    console.log(new Float64Array([1.5, NaN]));
    console.log(new Uint8Array(1000000));
    const bytes = new Uint8Array(new ArrayBuffer(5));
    bytes.set([0, 1, 2, 0xfe, 0xff]);
    console.log(bytes.buffer);
    console.log(new ArrayBuffer(0));
    console.log(new SharedArrayBuffer(2));
    const moved = new ArrayBuffer(4);
    if (typeof moved.transfer !== "function") {
      console.log("no-transfer");
    } else {
      moved.transfer();
      console.log(moved);
    }
    const view = new DataView(new ArrayBuffer(8));
    let viewCalls = 0;
    Object.defineProperty(view, "extra", {
      get() { viewCalls++; return 1; },
      enumerable: true,
    });
    console.log(view);
    console.log(viewCalls);
    class E extends Error {
      constructor() {
        super("boom");
        this.name = "E";
        this.code = 7;
      }
    }
    console.log(new E());
    console.log(new Error("boom", { cause: 5 }));
    console.log(new AggregateError([1], "agg"));
    const coded = new Error("deep");
    coded.code = 1;
    console.log({ a: { b: { c: { d: { e: coded } } } } });
    console.log({ a: { b: { c: { d: { e: new Error("bare") } } } } });
    class DeepE extends Error {
      constructor() {
        super("x");
        this.name = "DeepE";
        this.code = 1;
      }
    }
    console.log({ a: { b: { c: { d: { e: new DeepE() } } } } });
    "#,
  );
  assert_eq!(logs[0], "Map(2) { \"a\" => 1, 2 => \"b\" }");
  assert_eq!(logs[1], "Set(3) { 1, \"a\", { b: 2 } }");
  assert_eq!(logs[2], "Set(0) {}");
  assert_eq!(logs[3], "Map(0) {}");
  assert_eq!(logs[4], "WeakMap { <items unknown> }");
  assert_eq!(logs[5], "WeakSet { <items unknown> }");
  assert_eq!(logs[6], "[Map Entries] { [ \"a\", 1 ] }");
  assert_eq!(logs[7], "[Map Iterator] { \"a\" }");
  assert_eq!(logs[8], "[Set Iterator] { 1 }");
  assert_eq!(logs[9], "Object [Array Iterator] {}");
  assert_eq!(logs[10], "Promise { <pending> }");
  assert_eq!(logs[11], "Promise { 1 }");
  assert_eq!(logs[12], "Promise { undefined }");
  assert!(logs[13].contains("<rejected>"), "{}", logs[13]);
  assert!(logs[13].contains("Error: boom"), "{}", logs[13]);
  assert!(logs[13].contains("at "), "{}", logs[13]);
  assert_eq!(logs[14], "1970-01-01T00:00:00.000Z");
  assert_eq!(logs[15], "Invalid Date");
  assert_eq!(logs[16], "1970-01-01T00:00:00.000Z { x: 1 }");
  assert_eq!(logs[17], "/a.b/gi");
  assert_eq!(logs[18], "/a/ { x: 1 }");
  assert_eq!(logs[19], "[Function: foo]");
  assert_eq!(logs[20], "[Function (anonymous)]");
  assert_eq!(logs[21], "[Function: bound foo]");
  assert_eq!(logs[22], "[class Foo]");
  assert_eq!(logs[23], "[class Bar extends Array]");
  assert_eq!(logs[24], "[class (anonymous)]");
  assert_eq!(logs[25], "[Function: named] { x: 1 }");
  assert_eq!(logs[26], "[Function: Object]");
  assert_eq!(logs[27], "[Function: abs]");
  assert_eq!(logs[28], "[Function: parseInt]");
  assert_eq!(logs[29], "[AsyncFunction: af]");
  assert_eq!(logs[30], "[GeneratorFunction: g]");
  assert_eq!(logs[31], "[AsyncGeneratorFunction: ag]");
  assert_eq!(logs[32], "[Number: 1]");
  assert_eq!(logs[33], "[BigInt: 1n]");
  assert_eq!(logs[34], "[Boolean: false]");
  assert_eq!(logs[35], "[String: \"ab\"]");
  assert_eq!(logs[36], "[Symbol: Symbol(\"a b\")]");
  assert_eq!(logs[37], "[String: \"ab\"] { z: 1 }");
  assert_eq!(logs[38], "[Arguments] { \"0\": 1, \"1\": 2 }");
  assert_eq!(logs[39], "Uint8Array(3) [ 1, 2, 255 ]");
  assert_eq!(logs[40], "Uint8Array(0) []");
  assert_eq!(logs[41], "BigInt64Array(2) [ 1n, -2n ]");
  assert_eq!(logs[42], "Float64Array(2) [ 1.5, NaN ]");
  assert!(logs[43].contains("... 999900 more items"), "{}", logs[43]);
  assert_eq!(
    logs[44],
    "ArrayBuffer { [Uint8Contents]: <00 01 02 fe ff>, byteLength: 5 }"
  );
  assert_eq!(
    logs[45],
    "ArrayBuffer { [Uint8Contents]: <>, byteLength: 0 }"
  );
  assert_eq!(
    logs[46],
    "SharedArrayBuffer { [Uint8Contents]: <00 00>, byteLength: 2 }"
  );
  assert_eq!(
    logs[47], "ArrayBuffer { (detached), byteLength: 0 }",
    "transfer missing would log no-transfer"
  );
  assert!(logs[48].contains("byteLength: 8"), "{}", logs[48]);
  assert!(logs[48].contains("byteOffset: 0"), "{}", logs[48]);
  assert!(logs[48].contains("ArrayBuffer"), "{}", logs[48]);
  assert!(logs[48].contains("[Getter]"), "{}", logs[48]);
  assert_eq!(logs[49], "0");
  assert!(logs[50].starts_with("E: boom"), "{}", logs[50]);
  assert!(logs[50].contains("at "), "{}", logs[50]);
  assert!(logs[50].contains("code: 7"), "{}", logs[50]);
  assert!(logs[51].contains("[cause]: 5"), "{}", logs[51]);
  assert!(logs[52].contains("AggregateError"), "{}", logs[52]);
  assert!(logs[52].contains("errors"), "{}", logs[52]);
  assert!(logs[53].contains("e: [Error]"), "{}", logs[53]);
  assert!(logs[54].contains("Error: bare"), "{}", logs[54]);
  assert!(logs[54].contains("at "), "{}", logs[54]);
  assert!(logs[55].contains("e: [DeepE]"), "{}", logs[55]);
}

#[test]
fn console_prints_temporal_like_to_string() {
  assert_logs(
    r#"
    console.log(Temporal.PlainDate.from("2020-01-02"));
    console.log(Temporal.PlainTime.from("03:04:05.006007008"));
    console.log(Temporal.PlainDateTime.from("2020-01-02T03:04:05.006007008"));
    console.log(Temporal.ZonedDateTime.from(
      "2020-01-02T03:04:05.006007008+09:00[Asia/Tokyo][u-ca=japanese]"
    ));
    console.log(Temporal.Instant.from("2020-01-02T03:04:05.006007008Z"));
    console.log(Temporal.Instant.fromEpochMilliseconds(1736321470888));
    console.log(Temporal.Duration.from({
      years: 1,
      days: 2,
      hours: 3,
      minutes: 4,
      seconds: 5,
      milliseconds: 6,
      microseconds: 7,
      nanoseconds: 8,
    }));
    console.log(new Temporal.Duration());
    console.log(Temporal.Duration.from({ hours: -1, minutes: -2 }));
    console.log(Temporal.Duration.from("P1W"));
    console.log(Temporal.PlainMonthDay.from("01-02"));
    console.log(Temporal.PlainYearMonth.from("2020-01"));
    console.log(new Temporal.PlainDate(2021, 7, 1, "hebrew"));
    console.log(Temporal);
    console.log(Temporal.Now);
    const extra = Temporal.PlainDate.from("2020-01-02");
    extra.extra = 1;
    console.log(extra);
    class MyDate extends Temporal.PlainDate {
      get extra() { return 7; }
    }
    console.log(new MyDate(2020, 1, 2));
    const custom = Temporal.PlainDate.from("2020-01-02");
    custom.toString = () => "custom";
    console.log(custom);
    const numeric = Temporal.PlainDate.from("2020-01-02");
    numeric.toString = () => 5;
    console.log(numeric);
    const both = Temporal.PlainDate.from("2020-01-02");
    both.toString = () => ({});
    both.valueOf = () => "from-value";
    console.log(both);
    console.log({ d: Temporal.PlainDate.from("2020-01-02"), n: 1 });
    const repeated = Temporal.PlainDate.from("2020-01-02");
    console.log([repeated, repeated]);
    console.log(new Map([["a", repeated]]));
    console.log(new Proxy(repeated, {}));
    console.log("%o", repeated);
    console.log({ a: { b: { c: { d: { e: repeated } } } } });
    console.log(Object.setPrototypeOf(
      Temporal.PlainDate.from("2020-01-02"),
      null,
    ));
    const broken = Temporal.PlainDate.from("2020-01-02");
    broken.toString = () => { throw new Error("boom"); };
    try {
      console.log(broken);
      console.log("reached");
    } catch (error) {
      console.log(error.message);
    }
    const weird = Temporal.PlainDate.from("2020-01-02");
    weird.toString = () => ({});
    try {
      console.log(weird);
      console.log("reached");
    } catch (error) {
      console.log(error.name);
    }
    const sym = Temporal.PlainDate.from("2020-01-02");
    sym.toString = () => Symbol("x");
    try {
      console.log(sym);
      console.log("reached");
    } catch (error) {
      console.log(error.name + ": " + error.message);
    }
    "#,
    &[
      "2020-01-02",
      "03:04:05.006007008",
      "2020-01-02T03:04:05.006007008",
      "2020-01-02T03:04:05.006007008+09:00[Asia/Tokyo][u-ca=japanese]",
      "2020-01-02T03:04:05.006007008Z",
      "2025-01-08T07:31:10.888Z",
      "P1Y2DT3H4M5.006007008S",
      "PT0S",
      "-PT1H2M",
      "P1W",
      "01-02",
      "2020-01",
      "2021-07-01[u-ca=hebrew]",
      "Object [Temporal] {}",
      "Object [Temporal.Now] {}",
      "2020-01-02",
      "2020-01-02",
      "custom",
      "5",
      "from-value",
      "{ d: 2020-01-02, n: 1 }",
      "[ 2020-01-02, 2020-01-02 ]",
      "Map(1) { \"a\" => 2020-01-02 }",
      "2020-01-02",
      "2020-01-02",
      "{\n  a: {\n    b: { c: { d: { e: 2020-01-02 } } }\n  }\n}",
      "[PlainDate: null prototype] {}",
      "boom",
      "TypeError",
      "TypeError: Cannot convert a Symbol value to a string",
    ],
  );
  let logs = logs_of(
    r#"
    console.log(Temporal.PlainDate.prototype);
    console.log({
      [Symbol.toStringTag]: "Temporal.PlainDate",
      toString() { return "nope"; },
    });
    "#,
  );
  assert!(
    logs[0].contains("calendarId: [Getter]"),
    "prototype keeps accessors\n{}",
    logs[0]
  );
  assert!(logs[0].contains("[Temporal.PlainDate]"), "{}", logs[0]);
  assert!(
    logs[1].contains("Object [Temporal.PlainDate]"),
    "{}",
    logs[1]
  );
  assert!(
    logs[1].contains("toString: [Function: toString]"),
    "{}",
    logs[1]
  );
  assert!(
    logs[1].contains("Symbol(Symbol.toStringTag): \"Temporal.PlainDate\""),
    "{}",
    logs[1]
  );
  assert!(!logs[1].contains("nope"), "{}", logs[1]);
}

#[test]
fn console_formats_strings() {
  let logs = logs_of(
    r#"
    console.log("%s");
    console.log("%s", { a: 1 });
    const proto = { toString() { return "%s %s"; } };
    console.log(Object.create(proto), "A", "B");
    console.log("%d", { valueOf() { return 3.9; } });
    console.log("%d", "10px");
    console.log("%d %i %f", Symbol("a"), Symbol("b"), Symbol("c"));
    console.log("%f", Infinity);
    const circular = {};
    circular.a = circular;
    console.log("%j", circular);
    console.log("%j", { a: 1 });
    console.log("%j", undefined);
    console.log("%j %s", function () {}, "ok");
    console.log("%j", Symbol("a"));
    console.log("100%%", "x");
    console.log("100%", 1);
    console.log("%s %o", "only");
    console.log("%q %s", "A", "B");
    console.log("%c%s", "red", "x");
    console.log("%o", { a: 1 });
    console.log("%s", "a", { b: 1 });
    let name = "";
    try {
      console.log("%s", Symbol("a"));
    } catch (error) {
      name = error.name;
    }
    console.log(name);
    console.log(Symbol("a"));
    try {
      console.log("%s", { toString() { throw new Error("nope"); } });
    } catch (error) {
      console.log("caught-tostring");
    }
    try {
      console.log("%d", { valueOf() { throw new Error("bad"); } });
    } catch (error) {
      console.log("caught-valueof");
    }
    try {
      console.group("%s", { toString() { throw new Error("nope"); } });
    } catch (error) {}
    console.log("plain");
    console.group();
    console.log("inside");
    console.groupEnd();
    console.log("after-group");
    console.trace("here");
    console.assert(false, { a: 1 });
    console.assert(true, "hidden");
    console.log("after-assert");
    console.dir({ a: 1 });
    console.dirxml({ a: 1 });
    console.table({ a: 1 });
    console.groupCollapsed("title");
    console.log("collapsed");
    console.groupEnd();
    console.time("t");
    console.timeLog("t", { a: 1 });
    let labelCalls = 0;
    const label = { toString() { labelCalls++; return "lab"; } };
    console.count(label);
    console.log(labelCalls);
    try {
      console.count({ toString() { throw new Error("label"); } });
    } catch (error) {
      console.log("label-threw");
    }
    "#,
  );
  assert_eq!(logs[0], "%s");
  assert_eq!(logs[1], "[object Object]");
  assert_eq!(logs[2], "{} A B");
  assert_eq!(logs[3], "3");
  assert_eq!(logs[4], "NaN");
  assert_eq!(logs[5], "NaN NaN NaN");
  assert_eq!(logs[6], "inf");
  assert_eq!(logs[7], "[Circular]");
  assert_eq!(logs[8], "{\"a\":1}");
  assert_eq!(logs[9], "%j");
  assert_eq!(logs[10], "%j ok");
  assert_eq!(logs[11], "%j");
  assert_eq!(logs[12], "100% x");
  assert_eq!(logs[13], "100% 1");
  assert_eq!(logs[14], "only %o");
  assert_eq!(logs[15], "%q A B");
  assert_eq!(logs[16], "x");
  assert_eq!(logs[17], "{ a: 1 }");
  assert_eq!(logs[18], "a { b: 1 }");
  assert_eq!(logs[19], "TypeError");
  assert_eq!(logs[20], "Symbol(a)");
  assert_eq!(logs[21], "caught-tostring");
  assert_eq!(logs[22], "caught-valueof");
  assert_eq!(logs[23], "plain");
  assert_eq!(logs[24], "console.group");
  assert_eq!(logs[25], "  inside");
  assert_eq!(logs[26], "after-group");
  assert_eq!(logs[27], "after-assert");
  assert_eq!(logs[28], "{ a: 1 }");
  assert_eq!(logs[29], "{ a: 1 }");
  assert_eq!(logs[30], "{ a: 1 }");
  assert_eq!(logs[31], "title");
  assert_eq!(logs[32], "  collapsed");
  assert!(logs[33].contains("t: "), "{}", logs[33]);
  assert!(logs[33].contains("ms { a: 1 }"), "{}", logs[33]);
  assert_eq!(logs[34], "lab: 1");
  assert_eq!(logs[35], "1");
  assert_eq!(logs[36], "label-threw");
}

#[test]
fn console_rejects_a_revoked_proxy() {
  let logs = logs_of(
    r#"
    let name = "";
    let message = "";
    try {
      const holder = Proxy.revocable({ a: 1 }, {});
      holder.revoke();
      console.log(holder.proxy);
    } catch (error) {
      name = error.name;
      message = error.message;
    }
    console.log(name);
    console.log(message);
    "#,
  );
  assert_eq!(
    logs,
    vec!["TypeError", "Cannot inspect a proxy that has been revoked.",]
  );
}

#[test]
fn classic_session_uses_a_fresh_realm_per_run() {
  let mut session = ClassicSession::new();
  let first = session
    .run(
      &["globalThis.mark = 1; globalThis.__done = '1';".to_string()],
      "__done",
      Duration::from_secs(1),
      None,
      None,
      None,
    )
    .expect("first realm");
  assert_eq!(first.value.as_deref(), Some("1"));
  assert!(first.error.is_none());
  let second = session
    .run(
      &[
        "globalThis.__done = globalThis.mark === 1 ? 'leaked' : 'fresh';"
          .to_string(),
      ],
      "__done",
      Duration::from_secs(1),
      None,
      None,
      None,
    )
    .expect("second realm");
  assert_eq!(second.value.as_deref(), Some("fresh"));
}

#[test]
fn classic_session_drops_timers_between_realms() {
  let mut session = ClassicSession::new();
  session
    .run(
      &["setTimeout(() => { globalThis.fired = 1; }, 0); globalThis.__done = '1';".to_string()],
      "__done",
      Duration::from_secs(1),
      None,
      None,
      None,
    )
    .expect("arm timer");
  let second = session
    .run(
      &[
        "globalThis.__done = globalThis.fired === 1 ? 'leaked' : 'fresh';"
          .to_string(),
      ],
      "__done",
      Duration::from_secs(1),
      None,
      None,
      None,
    )
    .expect("second realm");
  assert_eq!(second.value.as_deref(), Some("fresh"));
}

#[test]
fn classic_session_binds_interned_scripts_in_later_realms() {
  let mut session = ClassicSession::new();
  let done = session.intern("globalThis.__done = 'ok';");
  let first = session
    .run_parts(
      &[ClassicPart::Shared(done)],
      "__done",
      Duration::from_secs(1),
      None,
      None,
      None,
    )
    .expect("first realm");
  assert_eq!(first.value.as_deref(), Some("ok"));
  let second = session
    .run_parts(
      &[ClassicPart::Shared(done)],
      "__done",
      Duration::from_secs(1),
      None,
      None,
      None,
    )
    .expect("second realm");
  assert_eq!(second.value.as_deref(), Some("ok"));
}

fn probe_host(
  _scope: &mut v8::PinScope,
  _args: v8::FunctionCallbackArguments,
  _ret: v8::ReturnValue,
) {
}

fn property_text(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
  name: &str,
) -> String {
  let object = v8::Local::<v8::Object>::try_from(value).expect(name);
  let field = crate::js::dictionary_member(scope, object, name)
    .expect(name)
    .expect(name);
  crate::js::rust_string(scope, field)
}

#[test]
fn host_binding_helpers_follow_their_contract() {
  crate::runtime::with_runtime(
    false,
    false,
    |_| {},
    |scope| {
      let object = v8::Object::new(scope);
      let yes = v8::String::new(scope, "yes").unwrap();
      let present = v8::String::new(scope, "present").unwrap();
      object.set(scope, yes.into(), present.into());
      let no = v8::String::new(scope, "no").unwrap();
      object.set(scope, no.into(), v8::undefined(scope).into());
      let found = crate::js::dictionary_member(scope, object, "yes")
        .unwrap()
        .unwrap();
      assert_eq!(crate::js::rust_string(scope, found), "present");
      assert!(
        crate::js::dictionary_member(scope, object, "no")
          .unwrap()
          .is_none()
      );
      let throwing =
        v8::String::new(scope, "({ get yes() { throw new Error('nope'); } })")
          .unwrap();
      let throwing = v8::Script::compile(scope, throwing, None)
        .unwrap()
        .run(scope)
        .unwrap();
      let throwing = v8::Local::<v8::Object>::try_from(throwing).unwrap();
      assert!(matches!(
        crate::js::dictionary_member(scope, throwing, "yes"),
        Err(crate::op::OpError::Thrown(_))
      ));

      let text = v8::String::new(scope, "hello").unwrap();
      assert_eq!(crate::js::rust_string(scope, text.into()), "hello");
      let source =
        v8::String::new(scope, "({ toString() { return 'custom'; } })")
          .unwrap();
      let custom = v8::Script::compile(scope, source, None)
        .unwrap()
        .run(scope)
        .unwrap();
      assert_eq!(crate::js::rust_string(scope, custom), "custom");
      {
        v8::tc_scope!(let tc, scope);
        let source =
          v8::String::new(tc, "({ toString() { throw new Error('no'); } })")
            .unwrap();
        let value = v8::Script::compile(tc, source, None)
          .unwrap()
          .run(tc)
          .unwrap();
        assert_eq!(crate::js::rust_string(tc, value), "undefined");
        assert_eq!(crate::js::rust_string_or(tc, value, ""), "");
      }

      let finite = v8::Number::new(scope, 1.5);
      assert_eq!(
        crate::js::finite_number(scope, finite.into(), "not finite").unwrap(),
        1.5
      );
      let nan = v8::Number::new(scope, f64::NAN);
      assert!(
        crate::js::finite_number(scope, nan.into(), "not finite").is_err()
      );
      let infinity = v8::Number::new(scope, f64::INFINITY);
      assert!(
        crate::js::finite_number(scope, infinity.into(), "not finite").is_err()
      );
      let null = v8::null(scope);
      assert_eq!(
        crate::js::finite_number(scope, null.into(), "not finite").unwrap(),
        0.0
      );
      let value_of = v8::String::new(
        scope,
        "({ valueOf() { throw new Error('valueOf'); } })",
      )
      .unwrap();
      let value_of = v8::Script::compile(scope, value_of, None)
        .unwrap()
        .run(scope)
        .unwrap();
      assert!(matches!(
        crate::js::to_number(scope, value_of),
        Err(crate::op::OpError::Thrown(_))
      ));
      assert_eq!(crate::js::enforce_range_u32(-0.9), Some(0));
      assert_eq!(
        crate::js::enforce_range_u32(4_294_967_295.9),
        Some(u32::MAX)
      );
      assert_eq!(crate::js::enforce_range_u32(-1.0), None);
      assert_eq!(crate::js::enforce_range_u32(f64::NAN), None);
      assert_eq!(
        crate::js::enforce_range_u64(crate::js::MAX_SAFE_INTEGER),
        Some(9_007_199_254_740_991)
      );
      assert_eq!(
        crate::js::enforce_range_u64(crate::js::MAX_SAFE_INTEGER + 1.0),
        None
      );
      assert_eq!(crate::js::enforce_range_u64(-0.9), Some(0));
      assert_eq!(
        crate::js::enforce_range_i64(-crate::js::MAX_SAFE_INTEGER),
        Some(-9_007_199_254_740_991)
      );
      assert_eq!(
        crate::js::enforce_range_i64(-(crate::js::MAX_SAFE_INTEGER + 1.0)),
        None
      );
      assert_eq!(crate::js::modulo_u32(-1.0), u32::MAX);
      assert_eq!(crate::js::modulo_u32(f64::NAN), 0);
      assert_eq!(crate::js::modulo_i64(-1.9), -1);
      assert_eq!(crate::js::modulo_i64(f64::NAN), 0);
      assert_eq!(crate::js::modulo_i64(9_223_372_036_854_775_808.0), i64::MIN);
      assert_eq!(crate::js::modulo_u64(-1.0), u64::MAX);
      assert_eq!(crate::js::modulo_u64(18_446_744_073_709_551_616.0), 0);
      assert_eq!(crate::js::modulo_u64(18_446_744_073_709_555_712.0), 4096);

      let view = crate::js::uint8_array(scope, vec![1, 2, 3, 4]).unwrap();
      let mut bytes = [0u8; 4];
      assert_eq!(view.copy_contents(&mut bytes), 4);
      assert_eq!(bytes, [1, 2, 3, 4]);

      let global = crate::js::global(scope);
      crate::js::define_function(scope, global, "probeHost", 2, probe_host);
      let probe =
        v8::String::new(scope, "probeHost.length + ' ' + probeHost.name")
          .unwrap();
      let probe = v8::Script::compile(scope, probe, None)
        .unwrap()
        .run(scope)
        .unwrap();
      assert_eq!(crate::js::rust_string(scope, probe), "2 probeHost");

      let holder = v8::Object::new(scope);
      let description = v8::String::new(scope, "dispose").unwrap();
      let symbol = v8::Symbol::new(scope, Some(description));
      crate::js::define_function_key(
        scope,
        holder,
        "dispose",
        symbol.into(),
        0,
        probe_host,
      );
      assert!(
        crate::js::dictionary_member(scope, holder, "dispose")
          .unwrap()
          .is_none()
      );
      let installed = holder.get(scope, symbol.into()).expect("symbol method");
      assert_eq!(property_text(scope, installed, "name"), "dispose");
      assert_eq!(property_text(scope, installed, "length"), "0");

      let posted = v8::Object::new(scope);
      assert!(crate::js::try_define_function(
        scope,
        posted,
        "postMessage",
        1,
        probe_host,
      ));
      let posted_fn =
        crate::js::dictionary_member(scope, posted, "postMessage")
          .unwrap()
          .unwrap();
      assert_eq!(property_text(scope, posted_fn, "name"), "postMessage");
      assert_eq!(property_text(scope, posted_fn, "length"), "1");

      let missing = crate::js::exception_value(
        scope,
        crate::op::OpError::dom("InvalidStateError", "gone"),
        crate::js::DomFallback::Undefined,
        crate::js::StringFailure::Unwrap,
      );
      assert!(missing.is_undefined());
      let type_fallback = crate::js::exception_value(
        scope,
        crate::op::OpError::dom("InvalidStateError", "gone"),
        crate::js::DomFallback::Type,
        crate::js::StringFailure::Unwrap,
      );
      assert_eq!(property_text(scope, type_fallback, "name"), "TypeError");
      assert_eq!(property_text(scope, type_fallback, "message"), "gone");
      let error_fallback = crate::js::exception_value(
        scope,
        crate::op::OpError::dom("InvalidStateError", "gone"),
        crate::js::DomFallback::Error,
        crate::js::StringFailure::Unwrap,
      );
      assert_eq!(property_text(scope, error_fallback, "name"), "Error");
      assert_eq!(property_text(scope, error_fallback, "message"), "gone");

      let source = v8::String::new(
        scope,
        concat!(
          "function DOMException(message, name) { ",
          "this.message = message; this.name = name; }\n",
          "DOMException",
        ),
      )
      .unwrap();
      let constructor = v8::Script::compile(scope, source, None)
        .unwrap()
        .run(scope)
        .unwrap();
      let constructor =
        v8::Local::<v8::Function>::try_from(constructor).unwrap();
      crate::set_dom_exception(scope, constructor);
      let dom = crate::js::exception_value(
        scope,
        crate::op::OpError::dom("InvalidStateError", "gone"),
        crate::js::DomFallback::Type,
        crate::js::StringFailure::Unwrap,
      );
      assert_eq!(property_text(scope, dom, "name"), "InvalidStateError");
      assert_eq!(property_text(scope, dom, "message"), "gone");
      let type_error = crate::js::exception_value(
        scope,
        crate::op::OpError::type_("bad type"),
        crate::js::DomFallback::Type,
        crate::js::StringFailure::Unwrap,
      );
      assert_eq!(property_text(scope, type_error, "name"), "TypeError");
      assert_eq!(property_text(scope, type_error, "message"), "bad type");
      let range_error = crate::js::exception_value(
        scope,
        crate::op::OpError::range("bad range"),
        crate::js::DomFallback::Type,
        crate::js::StringFailure::Unwrap,
      );
      assert_eq!(property_text(scope, range_error, "name"), "RangeError");
      assert_eq!(property_text(scope, range_error, "message"), "bad range");
      Ok(())
    },
  )
  .expect("host binding helpers");
}

#[test]
fn shared_buffer_and_exception_helpers_follow_their_contract() {
  crate::runtime::with_runtime(
    false,
    false,
    |_| {},
    |scope| {
      let limits = crate::js::BufferCopy::strict(
        8,
        "too large",
        "detached",
        "not a buffer",
      );
      let view = crate::js::uint8_array(scope, vec![9, 8, 7]).unwrap();
      let copied =
        crate::js::copy_buffer_source(scope, view.into(), limits).unwrap();
      assert_eq!(copied, vec![9, 8, 7]);
      let mut child_limits = limits;
      child_limits.missing_view_buffer = crate::js::MissingViewBuffer::Detached;
      let child_view = crate::js::uint8_array(scope, vec![4, 5]).unwrap();
      let child_copied =
        crate::js::copy_buffer_source(scope, child_view.into(), child_limits)
          .unwrap();
      assert_eq!(child_copied, vec![4, 5]);

      let text = v8::String::new(scope, "no").unwrap();
      match crate::js::copy_buffer_source(scope, text.into(), limits) {
        Err(crate::op::OpError::Type(message)) => {
          assert_eq!(message, "not a buffer");
        }
        other => panic!("expected a type error: {other:?}"),
      }
      let wide = crate::js::uint8_array(scope, vec![1, 2, 3]).unwrap();
      let mut small = limits;
      small.max_bytes = 2;
      match crate::js::copy_buffer_source(scope, wide.into(), small) {
        Err(crate::op::OpError::Type(message)) => {
          assert_eq!(message, "too large");
        }
        other => panic!("expected a size error: {other:?}"),
      }
      let buffer = crate::js::array_buffer(scope, vec![1, 2, 3, 4]);
      assert_eq!(buffer.byte_length(), 4);
      let _ = buffer.detach(None);
      match crate::js::copy_buffer_source(scope, buffer.into(), limits) {
        Err(crate::op::OpError::Type(message)) => {
          assert_eq!(message, "detached");
        }
        other => panic!("expected a detached error: {other:?}"),
      }

      let samples = 1.5f32.to_le_bytes();
      let floats = crate::js::float32_array(scope, samples.to_vec()).unwrap();
      let mut narrow = [0u8; 4];
      assert_eq!(floats.copy_contents(&mut narrow), 4);
      assert_eq!(narrow, samples);
      let wide_bits = 2.5f64.to_le_bytes();
      let doubles =
        crate::js::float64_array(scope, wide_bits.to_vec()).unwrap();
      let mut wide_bytes = [0u8; 8];
      assert_eq!(doubles.copy_contents(&mut wide_bytes), 8);
      assert_eq!(wide_bytes, wide_bits);
      let clamped = crate::js::uint8_clamped_array(scope, vec![1, 2]).unwrap();
      let mut bytes = [0u8; 2];
      assert_eq!(clamped.copy_contents(&mut bytes), 2);
      assert_eq!(bytes, [1, 2]);

      let plain = crate::js::error_exception(scope, "plain");
      assert_eq!(property_text(scope, plain, "name"), "Error");
      assert_eq!(property_text(scope, plain, "message"), "plain");
      let typed = crate::js::type_exception(scope, "typed");
      assert_eq!(property_text(scope, typed, "name"), "TypeError");
      assert_eq!(property_text(scope, typed, "message"), "typed");
      match crate::js::thrown_error(scope, "thrown plain") {
        crate::op::OpError::Thrown(value) => {
          let value = v8::Local::new(scope, *value);
          assert_eq!(property_text(scope, value, "name"), "Error");
          assert_eq!(property_text(scope, value, "message"), "thrown plain");
        }
        other => panic!("expected a thrown error: {other:?}"),
      }

      let word = v8::String::new(scope, "abc").unwrap();
      assert_eq!(
        crate::js::string_primitive(scope, word.into()).as_deref(),
        Some("abc")
      );
      let number = v8::Integer::new(scope, 1);
      assert!(crate::js::string_primitive(scope, number.into()).is_none());

      let global = crate::js::global(scope);
      crate::js::define_function(scope, global, "ProbeCtor", 0, probe_host);
      crate::js::relocate_constructor(scope, "ProbeCtor");
      assert!(
        crate::js::dictionary_member(scope, global, "ProbeCtor")
          .unwrap()
          .is_none()
      );
      let yuzuko = crate::js::namespace(scope, "Yuzuko");
      let moved = crate::js::dictionary_member(scope, yuzuko, "ProbeCtor")
        .unwrap()
        .unwrap();
      assert_eq!(property_text(scope, moved, "name"), "ProbeCtor");

      let source = v8::String::new(
        scope,
        "({ mark(value) { globalThis.marked = value; } })",
      )
      .unwrap();
      let object = v8::Script::compile(scope, source, None)
        .unwrap()
        .run(scope)
        .unwrap();
      let object = v8::Local::<v8::Object>::try_from(object).unwrap();
      let arg = v8::String::new(scope, "ping").unwrap();
      crate::js::call_method(scope, object, "mark", &[arg.into()]);
      let marked = crate::js::dictionary_member(scope, global, "marked")
        .unwrap()
        .unwrap();
      assert_eq!(crate::js::rust_string(scope, marked), "ping");
      crate::js::call_method(scope, object, "missing", &[]);
      Ok(())
    },
  )
  .expect("shared buffer helpers");
}
