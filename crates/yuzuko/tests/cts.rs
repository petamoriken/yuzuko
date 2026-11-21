//! WebGPU Conformance Test Suite.
//!
//! `tests/webgpu/suite` is the upstream gpuweb/cts pin. Build its standalone
//! output before this test, from that directory:
//!
//! ```sh
//! npm ci --ignore-scripts && npx grunt standalone
//! ```
//!
//! `tests/webgpu/expectations/<suite>.json` uses the WPT leaf schema:
//! `true`, `false`, or `{"ignore": ...}`. The file name is the component
//! before the first colon, so `unittests:` lands in `unittests.json` and
//! `webgpu:` lands in `webgpu.json`. `false` means the case did not pass.
//! `YUZUKO_CTS_UPDATE=1` rewrites the suite files that ran.
//! `YUZUKO_CTS_FILTER` runs that one query instead of the enrolled queries.
//! A filtered update keeps cases in that suite file that did not run.
//! `YUZUKO_CTS_NO_IGNORE=1` runs ignored cases. With update, it records this
//! host and drops this host's ignore when the case passes or does not pass.
//!
//! The entry module is [`RUNNER`] in this file. The cargo test writes it
//! for one `yuzuko run` and deletes it when that process returns.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;
use serde_json::json;

const MARKER: &str = "YUZUKO_CTS_JSON ";

/// Queries this cargo test runs unless `YUZUKO_CTS_FILTER` is set.
///
/// `unittests:*` exercises the harness. The `webgpu:` queries cover
/// validation and operation of the implemented buffer, sampler, texture,
/// texture view, bind-group layout, pipeline layout, error scope, query set,
/// shader module, compute pipeline, queue, and command encoding, plus
/// sampling, vertex formats, device loss, labels, and the IDL surface.
/// Shader execution, compatibility mode, capability-limit sweeps, and the
/// HTML canvas reftests stay out.
const ENROLLED: &[&str] = &[
  "unittests:*",
  "webgpu:api,validation,createSampler:*",
  "webgpu:api,validation,createTexture:*",
  "webgpu:api,validation,createView:*",
  "webgpu:api,validation,buffer,create:*",
  "webgpu:api,validation,buffer,destroy:*",
  "webgpu:api,validation,buffer,mapping:*",
  "webgpu:api,validation,createBindGroupLayout:*",
  "webgpu:api,validation,createPipelineLayout:*",
  "webgpu:api,validation,error_scope:*",
  "webgpu:api,validation,query_set,create:*",
  "webgpu:api,validation,query_set,destroy:*",
  "webgpu:api,validation,compute_pipeline:*",
  "webgpu:api,validation,shader_module,*",
  "webgpu:api,validation,queue,writeBuffer:*",
  "webgpu:api,validation,queue,submit:*",
  "webgpu:api,validation,queue,destroyed,buffer:*",
  "webgpu:api,validation,encoding,cmds,copyBufferToBuffer:*",
  "webgpu:api,validation,encoding,cmds,render,setVertexBuffer:*",
  "webgpu:api,validation,encoding,cmds,render,setIndexBuffer:*",
  "webgpu:api,operation,sampling,*",
  "webgpu:api,operation,buffers,map:*",
  "webgpu:api,operation,buffers,map_ArrayBuffer:*",
  "webgpu:api,operation,buffers,map_detach:*",
  "webgpu:api,operation,buffers,map_oom:*",
  "webgpu:api,operation,buffers,createBindGroup:*",
  "webgpu:api,operation,vertex_state,*",
  "webgpu:api,operation,command_buffer,copyBufferToBuffer:*",
  "webgpu:api,operation,command_buffer,basic:*",
  "webgpu:api,operation,device,*",
  "webgpu:api,operation,uncapturederror:*",
  "webgpu:api,operation,labels:*",
  "webgpu:api,operation,queue,writeBuffer:*",
  "webgpu:idl,constants,flags:*",
  "webgpu:idl,constructable:*",
  "webgpu:idl,javascript:*",
];

const PLATFORMS: [Platform; 3] =
  [Platform::Linux, Platform::Macos, Platform::Windows];

/// `pass` is a CTS pass. `false` in the file is every other finished status
/// except `skip`: `fail` and `warn`. `ignore` skips the case before it runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Observed {
  Pass,
  Fail,
  Skip,
  Ignored,
}

/// One expectation leaf. `pass` is what a host compares after it runs the
/// case. `ignore` skips the case before that comparison.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Expect {
  pass: bool,
  ignore: Note,
}

/// An annotation and the hosts it covers. Every arm carries a reason.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Note {
  No,
  All { reason: String },
  Clauses(Vec<Clause>),
}

/// One operating system covered by an ignore reason.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Clause {
  platform: Platform,
  reason: String,
}

/// Names match `std::env::consts::OS` for the supported targets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Platform {
  Linux,
  Macos,
  Windows,
}

#[test]
fn cts_expectations_match() {
  let update = env_flag("YUZUKO_CTS_UPDATE");
  let no_ignore = env_flag("YUZUKO_CTS_NO_IGNORE");
  let host = Platform::host();
  let filter = std::env::var("YUZUKO_CTS_FILTER")
    .ok()
    .filter(|value| !value.is_empty());
  let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
  let built =
    workspace.join("tests/webgpu/suite/out/common/internal/file_loader.js");
  assert!(
    built.is_file(),
    "missing {}. From tests/webgpu/suite, run: npm ci --ignore-scripts && npx grunt standalone",
    built.display()
  );

  let expectations = workspace.join("tests/webgpu/expectations");
  let queries: Vec<String> = match &filter {
    Some(query) => vec![query.clone()],
    None => ENROLLED.iter().copied().map(str::to_string).collect(),
  };
  let mut saved: BTreeMap<String, BTreeMap<String, Expect>> = BTreeMap::new();
  for query in &queries {
    let suite = suite_name(query).to_string();
    if saved.contains_key(&suite) {
      continue;
    }
    let path = expectations.join(format!("{suite}.json"));
    saved.insert(
      suite.clone(),
      read_expectations(&path, update, filter.is_some(), &suite),
    );
  }
  let skipped = if no_ignore {
    Vec::new()
  } else {
    saved
      .values()
      .flat_map(|cases| cases.iter())
      .filter(|(_, expect)| noted_on(&expect.ignore, host))
      .map(|(name, _)| name.clone())
      .collect::<Vec<_>>()
  };
  let observed = run_cases(&workspace, &queries, &skipped);

  if update {
    for (name, status) in &observed {
      let suite = suite_name(name).to_string();
      if !saved.contains_key(&suite) {
        let path = expectations.join(format!("{suite}.json"));
        saved
          .insert(suite.clone(), read_expectations(&path, true, true, &suite));
      }
      apply_observation(
        saved.get_mut(&suite).expect("suite"),
        name,
        status,
        no_ignore,
        host,
      );
    }
    for (suite, cases) in &mut saved {
      if filter.is_none() {
        cases.retain(|name, _| observed.contains_key(name));
      }
      let path = expectations.join(format!("{suite}.json"));
      write_expectations(&path, cases);
    }
    return;
  }

  let mut mismatches = Vec::new();
  for (name, status) in &observed {
    let suite = suite_name(name);
    match saved.get(suite).and_then(|cases| cases.get(name)) {
      Some(expect) => {
        if let Some(message) = mismatches_for(name, status, expect, host) {
          mismatches.push(message);
        }
      }
      None => mismatches.push(format!("{name}: unexpected case ({status})")),
    }
  }
  if filter.is_none() {
    for cases in saved.values() {
      for name in cases.keys() {
        if !observed.contains_key(name) {
          mismatches.push(format!("{name}: expected case did not run"));
        }
      }
    }
  }
  let shown = mismatches.len().min(40);
  assert!(
    mismatches.is_empty(),
    "{} CTS expectation mismatch(es)\n{}{}",
    mismatches.len(),
    mismatches[..shown].join("\n"),
    if mismatches.len() > shown {
      format!("\n... and {} more", mismatches.len() - shown)
    } else {
      String::new()
    }
  );
}

/// The file name is the component before the first colon.
/// `unittests:basic:test,sync:` is `unittests`. `webgpu:api,...` is `webgpu`.
fn suite_name(name: &str) -> &str {
  let Some((suite, rest)) = name.split_once(':') else {
    panic!("{name}: a CTS name has a suite before the first colon");
  };
  let file_name = !suite.is_empty()
    && !rest.is_empty()
    && suite.chars().all(|character| {
      character.is_ascii_alphanumeric() || character == '_' || character == '-'
    });
  if !file_name {
    panic!("{name}: the suite is not a file name");
  }
  suite
}

fn env_flag(name: &str) -> bool {
  std::env::var(name).ok().is_some_and(|value| value == "1")
}

/// Entry module. `YUZUKO_CTS_OUT` is the `file:` URL of
/// `tests/webgpu/suite/out`, without a trailing slash.
const RUNNER: &str = r#"
// Runs enrolled gpuweb/cts queries inside yuzuko and prints one JSON line.
// The CTS is the compiled standalone build at tests/webgpu/suite/out.
// Arguments are CTS queries, then an optional `--`, then case names to skip.
// A skipped case is recorded as "ignore" and is not run.
// With no arguments the query is unittests:*.

import { DefaultTestFileLoader } from "YUZUKO_CTS_OUT/common/internal/file_loader.js";
import { Logger } from "YUZUKO_CTS_OUT/common/internal/logging/logger.js";
import { parseQuery } from "YUZUKO_CTS_OUT/common/internal/query/parseQuery.js";

const args = Array.from(Yuzuko.args);
const split = args.indexOf("--");
const queryArgs = split === -1 ? args : args.slice(0, split);
const ignored = new Set(split === -1 ? [] : args.slice(split + 1));
const queries = queryArgs.length > 0 ? queryArgs : ["unittests:*"];
const loader = new DefaultTestFileLoader();
const log = new Logger();
const cases = [];

try {
  for (const text of queries) {
    const testcases = await loader.loadCases(parseQuery(text));
    for (const testcase of testcases) {
      const name = testcase.query.toString();
      if (ignored.has(name)) {
        cases.push({ name, status: "ignore" });
        continue;
      }
      const [recorder, result] = log.record(name);
      await testcase.run(recorder, []);
      const status = result.status;
      if (
        status !== "pass" &&
        status !== "fail" &&
        status !== "skip" &&
        status !== "warn"
      ) {
        throw new Error(`${name} ended as ${status}`);
      }
      cases.push({ name, status });
    }
  }
  if (cases.length === 0) {
    console.error("found no tests");
    Yuzuko.exit(1);
  } else {
    console.log(`YUZUKO_CTS_JSON ${JSON.stringify(cases)}`);
  }
} catch (error) {
  const message = error instanceof Error && error.stack ? error.stack : String(error);
  console.error(message);
  Yuzuko.exit(1);
}
"#;

/// The entry module for one `yuzuko run`. Dropped when the run finishes.
struct RunnerFile {
  path: PathBuf,
}

impl RunnerFile {
  fn create(out_dir: &Path) -> Self {
    let url = yuzuko_core::file_url(out_dir).expect("cts output url");
    let source = RUNNER.replace("YUZUKO_CTS_OUT", &url);
    let path = std::env::temp_dir()
      .join(format!("yuzuko-cts-runner-{}.js", std::process::id()));
    fs::write(&path, source).expect("write cts runner");
    Self { path }
  }
}

impl Drop for RunnerFile {
  fn drop(&mut self) {
    let _ = fs::remove_file(&self.path);
  }
}

fn run_cases(
  workspace: &Path,
  queries: &[String],
  skipped: &[String],
) -> BTreeMap<String, String> {
  let runner = RunnerFile::create(&workspace.join("tests/webgpu/suite/out"));
  let mut command = Command::new(env!("CARGO_BIN_EXE_yuzuko"));
  command
    .current_dir(workspace)
    .arg("run")
    .arg(&runner.path)
    .arg("--")
    .args(queries);
  if !skipped.is_empty() {
    command.arg("--").args(skipped);
  }
  let output = command.output().expect("spawn yuzuko");
  let stdout = String::from_utf8_lossy(&output.stdout);
  let stderr = String::from_utf8_lossy(&output.stderr);
  assert!(
    output.status.success(),
    "cts runner failed\n{stdout}\n{stderr}"
  );
  let observed = parse_results(&stdout);
  assert!(
    !observed.is_empty(),
    "the CTS query matched no cases\n{stdout}\n{stderr}"
  );
  observed
}

fn apply_observation(
  saved: &mut BTreeMap<String, Expect>,
  name: &str,
  status: &str,
  no_ignore: bool,
  host: Platform,
) {
  match classify(name, status) {
    Observed::Ignored => {
      if !saved.contains_key(name) {
        panic!("{name}: ignored case has no expectation");
      }
    }
    Observed::Skip => {
      let next = record_skip(saved.get(name), name);
      saved.insert(name.to_string(), next);
    }
    kind @ (Observed::Pass | Observed::Fail) => {
      let next = merge_observation(saved.get(name), kind, no_ignore, host);
      saved.insert(name.to_string(), next);
    }
  }
}

/// A CTS `skip` is not `true` or `false`. Keep a reason that is already
/// written. Otherwise the update stops so a person can write one.
fn record_skip(previous: Option<&Expect>, name: &str) -> Expect {
  match previous {
    Some(expect) if !matches!(expect.ignore, Note::No) => expect.clone(),
    _ => panic!("{name}: the case skipped. Write {{\"ignore\": \"reason\"}}."),
  }
}

fn read_expectations(
  path: &Path,
  update: bool,
  filtered: bool,
  suite: &str,
) -> BTreeMap<String, Expect> {
  if !path.exists() {
    if update {
      return BTreeMap::new();
    }
    panic!("read {}: the expectation file is missing", path.display());
  }
  let text = fs::read_to_string(path)
    .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
  let Ok(Value::Object(map)) = serde_json::from_str(&text) else {
    panic!("{} is not a JSON object", path.display());
  };
  // The previous file was `{queries, cases}`. A full update replaces it.
  // That shape has no ignore reasons.
  if is_legacy(&map) {
    if update && !filtered {
      return BTreeMap::new();
    }
    panic!("{} uses the old queries/cases schema", path.display());
  }
  let mut expectations = BTreeMap::new();
  for (key, value) in map {
    let key_suite = suite_name(&key);
    if key_suite != suite {
      panic!("{key}: this case belongs in {key_suite}.json");
    }
    expectations.insert(key.clone(), parse_expect(&key, value));
  }
  expectations
}

fn is_legacy(map: &serde_json::Map<String, Value>) -> bool {
  matches!(map.get("queries"), Some(Value::Array(_)))
    && matches!(map.get("cases"), Some(Value::Object(_)))
}

fn parse_results(stdout: &str) -> BTreeMap<String, String> {
  let line = stdout
    .lines()
    .rev()
    .find(|line| line.starts_with(MARKER))
    .unwrap_or_else(|| panic!("missing {MARKER} line\n{stdout}"));
  let cases: Vec<Value> = serde_json::from_str(&line[MARKER.len()..])
    .unwrap_or_else(|error| panic!("parse CTS results: {error}\n{line}"));
  let mut observed = BTreeMap::new();
  for case in cases {
    let name = case
      .get("name")
      .and_then(Value::as_str)
      .unwrap_or_else(|| panic!("CTS result has no name: {case}"))
      .to_string();
    let status = case
      .get("status")
      .and_then(Value::as_str)
      .unwrap_or_else(|| panic!("{name} has no status"))
      .to_string();
    classify(&name, &status);
    if observed.insert(name.clone(), status).is_some() {
      panic!("duplicate CTS case {name}");
    }
  }
  observed
}

fn classify(name: &str, status: &str) -> Observed {
  match status {
    "pass" => Observed::Pass,
    "fail" | "warn" => Observed::Fail,
    "skip" => Observed::Skip,
    "ignore" => Observed::Ignored,
    _ => panic!("{name} has status {status}"),
  }
}

/// `None` when the observation matches. `false` matches `fail` and `warn`.
fn mismatches_for(
  name: &str,
  status: &str,
  expect: &Expect,
  host: Platform,
) -> Option<String> {
  let kind = classify(name, status);
  if kind == Observed::Ignored {
    return if noted_on(&expect.ignore, host) {
      None
    } else {
      Some(format!(
        "{name}: skipped before run without an ignore for {}",
        host.as_str()
      ))
    };
  }
  if noted_on(&expect.ignore, host) {
    return Some(format!("{name}: ignored on {} but {status}", host.as_str()));
  }
  let matched = matches!(
    (kind, expect.pass),
    (Observed::Pass, true) | (Observed::Fail, false)
  );
  if matched {
    None
  } else {
    let expected = if expect.pass { "pass" } else { "did not pass" };
    Some(format!("{name}: expected {expected}, observed {status}"))
  }
}

impl Platform {
  fn host() -> Self {
    match std::env::consts::OS {
      "linux" => Self::Linux,
      "macos" => Self::Macos,
      "windows" => Self::Windows,
      other => panic!("unsupported CTS platform {other}"),
    }
  }

  fn as_str(self) -> &'static str {
    match self {
      Self::Linux => "linux",
      Self::Macos => "macos",
      Self::Windows => "windows",
    }
  }
}

fn noted_on(note: &Note, host: Platform) -> bool {
  match note {
    Note::No => false,
    Note::All { .. } => true,
    Note::Clauses(clauses) => {
      clauses.iter().any(|clause| clause.platform == host)
    }
  }
}

fn platform_index(platform: Platform) -> usize {
  match platform {
    Platform::Linux => 0,
    Platform::Macos => 1,
    Platform::Windows => 2,
  }
}

/// Record `observed` for this host. Ignores for other hosts stay.
/// `no_ignore` drops the ignore that covered this host. A failure is
/// `false` and does not keep `ignore`.
fn merge_observation(
  previous: Option<&Expect>,
  observed: Observed,
  no_ignore: bool,
  host: Platform,
) -> Expect {
  let ignore = match previous.map(|expect| &expect.ignore) {
    Some(ignore) if no_ignore => subtract_host(ignore, host),
    Some(ignore) => ignore.clone(),
    None => Note::No,
  };
  match observed {
    Observed::Pass => {
      if matches!(ignore, Note::All { .. }) {
        panic!("an all-host ignore does not combine with a result");
      }
      Expect { pass: true, ignore }
    }
    Observed::Fail => Expect {
      pass: false,
      ignore: Note::No,
    },
    Observed::Skip | Observed::Ignored => {
      panic!("a skip is not a recorded result")
    }
  }
}

/// Drop the operating system that just ran. An all-host reason is one
/// note, so dropping this host drops that note.
fn subtract_host(note: &Note, host: Platform) -> Note {
  match note {
    Note::No => Note::No,
    Note::All { .. } => Note::No,
    Note::Clauses(clauses) => {
      let mut slots = expand(clauses);
      slots[platform_index(host)] = None;
      compact(&slots)
    }
  }
}

fn expand(clauses: &[Clause]) -> [Option<String>; 3] {
  let mut slots: [Option<String>; 3] = [None, None, None];
  for clause in clauses {
    fill(
      &mut slots,
      clause.platform,
      clause.reason.clone(),
      "",
      "ignore",
    );
  }
  slots
}

fn parse_expect(key: &str, value: Value) -> Expect {
  match value {
    Value::Bool(pass) => Expect {
      pass,
      ignore: Note::No,
    },
    Value::Object(map) => parse_expect_object(key, map),
    other => panic!("{key}: unsupported expectation {other}"),
  }
}

fn parse_expect_object(
  key: &str,
  map: serde_json::Map<String, Value>,
) -> Expect {
  if map.contains_key("fail") {
    panic!("{key}: use false for a whole-file failure");
  }
  for name in map.keys() {
    match name.as_str() {
      "ignore" => {}
      "reason" => {
        panic!("{key}: write the reason as the value of ignore or flaky")
      }
      other => panic!("{key}: unsupported expectation field {other}"),
    }
  }
  let ignore = parse_mark(key, "ignore", map.get("ignore"));
  if matches!(ignore, Note::No) {
    panic!("{key}: unsupported expectation {}", Value::Object(map));
  }
  Expect { pass: true, ignore }
}

fn parse_mark(key: &str, label: &str, value: Option<&Value>) -> Note {
  match value {
    None => Note::No,
    Some(Value::String(text)) => Note::All {
      reason: nonempty_reason(key, label, text),
    },
    Some(Value::Object(conditions)) => parse_conditions(key, label, conditions),
    Some(_) => {
      panic!("{key}: {label} must be a reason string or a condition object")
    }
  }
}

fn parse_conditions(
  key: &str,
  label: &str,
  value: &serde_json::Map<String, Value>,
) -> Note {
  let mut reason = None;
  let mut operating_systems = None;
  for (name, entry) in value {
    match name.as_str() {
      "reason" => reason = Some(parse_reason_value(key, label, entry)),
      "os" => operating_systems = Some(parse_os_list(key, label, entry)),
      _ => panic!("{key}: {label} field must be reason or os"),
    }
  }
  let Some(reason) = reason else {
    panic!("{key}: {label} needs a reason");
  };
  let Some(platforms) = operating_systems else {
    panic!("{key}: {label} needs os");
  };
  let mut slots: [Option<String>; 3] = [None, None, None];
  for platform in platforms {
    fill(&mut slots, platform, reason.clone(), key, label);
  }
  compact(&slots)
}

fn parse_reason_value(key: &str, label: &str, entry: &Value) -> String {
  let Value::String(text) = entry else {
    panic!("{key}: {label} reason must be a string");
  };
  nonempty_reason(key, label, text)
}

fn parse_os_list(key: &str, label: &str, entry: &Value) -> Vec<Platform> {
  let Value::Array(items) = entry else {
    panic!("{key}: {label} os must be an array");
  };
  if items.is_empty() {
    panic!("{key}: {label} os must not be empty");
  }
  let mut seen = [false; 3];
  let mut platforms = Vec::new();
  for item in items {
    let platform = match item.as_str() {
      Some("linux") => Platform::Linux,
      Some("macos") => Platform::Macos,
      Some("windows") => Platform::Windows,
      _ => panic!("{key}: os must be \"linux\", \"macos\", or \"windows\""),
    };
    let index = platform_index(platform);
    if seen[index] {
      panic!("{key}: {label} names {} twice", platform.as_str());
    }
    seen[index] = true;
    platforms.push(platform);
  }
  platforms
}

fn fill(
  slots: &mut [Option<String>; 3],
  platform: Platform,
  reason: String,
  key: &str,
  label: &str,
) {
  let index = platform_index(platform);
  match &slots[index] {
    Some(previous) if previous != &reason => {
      panic!("{key}: {label} gives two reasons for one host");
    }
    Some(_) => {}
    None => slots[index] = Some(reason),
  }
}

fn compact(slots: &[Option<String>; 3]) -> Note {
  if slots.iter().all(Option::is_none) {
    return Note::No;
  }
  if slots.iter().all(Option::is_some)
    && let Some(reason) = shared_slot_reason(slots)
  {
    return Note::All { reason };
  }
  let mut clauses = Vec::new();
  for (index, slot) in slots.iter().enumerate() {
    let Some(reason) = slot else {
      continue;
    };
    clauses.push(Clause {
      platform: PLATFORMS[index],
      reason: reason.clone(),
    });
  }
  if clauses.is_empty() {
    Note::No
  } else {
    Note::Clauses(clauses)
  }
}

fn shared_slot_reason(slots: &[Option<String>]) -> Option<String> {
  let mut found = None;
  for slot in slots {
    let Some(reason) = slot else {
      continue;
    };
    match &found {
      None => found = Some(reason.clone()),
      Some(previous) if previous == reason => {}
      Some(_) => return None,
    }
  }
  found
}

fn nonempty_reason(key: &str, label: &str, text: &str) -> String {
  let text = text.trim();
  if text.is_empty() {
    panic!("{key}: {label} must not be empty");
  }
  text.to_string()
}

fn write_expectations(path: &Path, expectations: &BTreeMap<String, Expect>) {
  let mut map = serde_json::Map::new();
  for (key, expect) in expectations {
    map.insert(key.clone(), expect_json(expect));
  }
  let text = pretty_json(&Value::Object(map));
  if let Some(parent) = path.parent() {
    fs::create_dir_all(parent).expect("expectations directory");
  }
  fs::write(path, format!("{text}\n")).expect("write expectations");
}

fn expect_json(expect: &Expect) -> Value {
  match &expect.ignore {
    Note::All { reason } => json!({ "ignore": reason }),
    Note::No | Note::Clauses(_) if !expect.pass => Value::Bool(false),
    Note::No => Value::Bool(true),
    Note::Clauses(clauses) => json!({ "ignore": clauses_json(clauses) }),
  }
}

fn clauses_json(clauses: &[Clause]) -> Value {
  let slots = expand(clauses);
  let Some(reason) = shared_slot_reason(&slots) else {
    panic!("a condition records one reason");
  };
  let os: Vec<Value> = PLATFORMS
    .iter()
    .enumerate()
    .filter(|(index, _)| slots[*index].is_some())
    .map(|(_, platform)| Value::String(platform.as_str().to_string()))
    .collect();
  if os.len() == PLATFORMS.len() {
    panic!("a condition that covers every host is a string");
  }
  if os.is_empty() {
    panic!("a condition records one reason");
  }
  json!({ "reason": reason, "os": os })
}

/// `serde_json::Map` sorts keys. A condition object is written `reason`,
/// then `os`. Every other object keeps sorted keys, which is what
/// `to_string_pretty` emits.
fn pretty_json(value: &Value) -> String {
  let mut out = String::new();
  write_json(&mut out, value, 0);
  out
}

fn write_json(out: &mut String, value: &Value, indent: usize) {
  match value {
    Value::Null => out.push_str("null"),
    Value::Bool(flag) => out.push_str(if *flag { "true" } else { "false" }),
    Value::Number(number) => out.push_str(&number.to_string()),
    Value::String(text) => {
      out.push_str(&serde_json::to_string(text).expect("string"));
    }
    Value::Array(items) if items.is_empty() => out.push_str("[]"),
    Value::Array(items) => {
      out.push_str("[\n");
      for (index, item) in items.iter().enumerate() {
        if index > 0 {
          out.push_str(",\n");
        }
        push_indent(out, indent + 1);
        write_json(out, item, indent + 1);
      }
      out.push('\n');
      push_indent(out, indent);
      out.push(']');
    }
    Value::Object(map) if map.is_empty() => out.push_str("{}"),
    Value::Object(map) => {
      let keys = object_keys(map);
      out.push_str("{\n");
      for (index, key) in keys.iter().enumerate() {
        if index > 0 {
          out.push_str(",\n");
        }
        push_indent(out, indent + 1);
        out.push_str(&serde_json::to_string(key).expect("key"));
        out.push_str(": ");
        write_json(out, &map[key], indent + 1);
      }
      out.push('\n');
      push_indent(out, indent);
      out.push('}');
    }
  }
}

fn object_keys(map: &serde_json::Map<String, Value>) -> Vec<String> {
  let note = map.contains_key("reason")
    && map
      .keys()
      .all(|key| matches!(key.as_str(), "reason" | "os"));
  if note {
    ["reason", "os"]
      .into_iter()
      .filter(|key| map.contains_key(*key))
      .map(str::to_string)
      .collect()
  } else {
    map.keys().cloned().collect()
  }
}

fn push_indent(out: &mut String, level: usize) {
  for _ in 0..level {
    out.push_str("  ");
  }
}

#[test]
fn platform_names_match_the_host() {
  assert_eq!(Platform::host().as_str(), std::env::consts::OS);
}

#[test]
fn suite_name_is_the_leading_component() {
  assert_eq!(suite_name("unittests:basic:test,sync:"), "unittests");
  assert_eq!(
    suite_name("webgpu:api,validation,createSampler:*"),
    "webgpu"
  );
}

#[test]
#[should_panic(expected = "a CTS name has a suite before the first colon")]
fn suite_name_rejects_a_name_without_a_colon() {
  suite_name("unittests");
}

#[test]
#[should_panic(expected = "the suite is not a file name")]
fn suite_name_rejects_a_path_separator() {
  suite_name("web/gpu:api:*");
}

#[test]
fn expectation_bool_and_ignore_round_trip() {
  assert_eq!(expect_json(&parse_expect("case", json!(true))), json!(true));
  assert_eq!(
    expect_json(&parse_expect("case", json!(false))),
    json!(false)
  );
  let value = json!({"ignore": "No GPU."});
  let expect = parse_expect("case", value.clone());
  assert!(noted_on(&expect.ignore, Platform::Linux));
  assert!(noted_on(&expect.ignore, Platform::Macos));
  assert!(noted_on(&expect.ignore, Platform::Windows));
  assert_eq!(expect_json(&expect), value);
  let spaced = parse_expect("case", json!({"ignore": "  No GPU.  "}));
  assert_eq!(expect_json(&spaced), value);
}

#[test]
fn expectation_platform_ignore_round_trips() {
  let value = json!({
    "ignore": {
      "reason": "No GPU.",
      "os": ["linux", "windows"]
    }
  });
  let swapped = json!({
    "ignore": {
      "reason": "No GPU.",
      "os": ["windows", "linux"]
    }
  });
  let expect = parse_expect("case", swapped);
  assert!(noted_on(&expect.ignore, Platform::Linux));
  assert!(noted_on(&expect.ignore, Platform::Windows));
  assert!(!noted_on(&expect.ignore, Platform::Macos));
  assert!(expect.pass);
  assert_eq!(expect_json(&expect), value);
  let every = json!({
    "ignore": {
      "reason": "No GPU.",
      "os": ["windows", "macos", "linux"]
    }
  });
  assert_eq!(
    expect_json(&parse_expect("case", every)),
    json!({"ignore": "No GPU."})
  );
}

#[test]
fn condition_object_writes_reason_before_os() {
  let expect = parse_expect(
    "case",
    json!({"ignore": {"os": ["windows", "linux"], "reason": "No GPU."}}),
  );
  assert_eq!(
    pretty_json(&expect_json(&expect)),
    "{\n  \"ignore\": {\n    \"reason\": \"No GPU.\",\n    \"os\": [\n      \"linux\",\n      \"windows\"\n    ]\n  }\n}"
  );
}

#[test]
fn false_accepts_fail_and_warn_only() {
  let expect = parse_expect("case", json!(false));
  assert!(mismatches_for("case", "fail", &expect, Platform::Macos).is_none());
  assert!(mismatches_for("case", "warn", &expect, Platform::Macos).is_none());
  let message =
    mismatches_for("case", "pass", &expect, Platform::Macos).expect("pass");
  assert!(message.contains("expected did not pass, observed pass"));
  let message =
    mismatches_for("case", "skip", &expect, Platform::Macos).expect("skip");
  assert!(message.contains("observed skip"));
  let pass = parse_expect("case", json!(true));
  assert!(mismatches_for("case", "pass", &pass, Platform::Macos).is_none());
  assert!(mismatches_for("case", "warn", &pass, Platform::Macos).is_some());
  assert!(mismatches_for("case", "fail", &pass, Platform::Macos).is_some());
  assert!(mismatches_for("case", "skip", &pass, Platform::Macos).is_some());
}

#[test]
fn ignore_skips_this_host_and_expects_a_pass_on_the_others() {
  let expect = parse_expect(
    "case",
    json!({"ignore": {"reason": "No GPU.", "os": ["linux"]}}),
  );
  assert!(mismatches_for("case", "ignore", &expect, Platform::Linux).is_none());
  assert!(mismatches_for("case", "pass", &expect, Platform::Macos).is_none());
  assert!(mismatches_for("case", "fail", &expect, Platform::Macos).is_some());
  let ran =
    mismatches_for("case", "pass", &expect, Platform::Linux).expect("ran");
  assert!(ran.contains("ignored on linux but pass"));
}

#[test]
fn update_keeps_ignore_for_a_host_that_did_not_run() {
  let previous = parse_expect(
    "case",
    json!({"ignore": {"reason": "No GPU.", "os": ["linux", "windows"]}}),
  );
  let next =
    merge_observation(Some(&previous), Observed::Pass, false, Platform::Macos);
  assert_eq!(expect_json(&next), expect_json(&previous));
  let cleared =
    merge_observation(Some(&previous), Observed::Pass, true, Platform::Linux);
  assert_eq!(
    expect_json(&cleared),
    json!({"ignore": {"reason": "No GPU.", "os": ["windows"]}})
  );
  let failed =
    merge_observation(Some(&previous), Observed::Fail, false, Platform::Macos);
  assert_eq!(expect_json(&failed), json!(false));
  let everywhere = parse_expect("case", json!({"ignore": "No GPU."}));
  let dropped =
    merge_observation(Some(&everywhere), Observed::Pass, true, Platform::Macos);
  assert_eq!(expect_json(&dropped), json!(true));
}

#[test]
fn update_preserves_an_ignore_when_the_case_skips() {
  let previous = parse_expect("case", json!({"ignore": "No GPU."}));
  assert_eq!(
    expect_json(&record_skip(Some(&previous), "case")),
    json!({"ignore": "No GPU."})
  );
}

#[test]
#[should_panic(expected = "the case skipped. Write {\"ignore\": \"reason\"}.")]
fn update_rejects_a_skip_without_a_reason() {
  record_skip(None, "case");
}

#[test]
#[should_panic(expected = "os must be \"linux\", \"macos\", or \"windows\"")]
fn expectation_rejects_darwin_as_a_platform() {
  parse_expect(
    "case",
    json!({"ignore": {"reason": "VideoToolbox", "os": ["darwin"]}}),
  );
}

#[test]
#[should_panic(expected = "ignore must not be empty")]
fn expectation_rejects_an_empty_reason() {
  parse_expect("case", json!({"ignore": "  "}));
}

#[test]
#[should_panic(expected = "write the reason as the value of ignore or flaky")]
fn expectation_rejects_a_sibling_reason() {
  parse_expect("case", json!({"ignore": "No GPU.", "reason": "No GPU."}));
}

#[test]
#[should_panic(expected = "must be a reason string or a condition object")]
fn expectation_rejects_an_ignore_without_a_reason() {
  parse_expect("case", json!({"ignore": true}));
}

#[test]
#[should_panic(expected = "ignore needs a reason")]
fn expectation_rejects_a_condition_without_a_reason() {
  parse_expect("case", json!({"ignore": {"os": ["linux"]}}));
}

#[test]
#[should_panic(expected = "ignore field must be reason or os")]
fn expectation_rejects_a_gpu_field() {
  parse_expect(
    "case",
    json!({"ignore": {"reason": "No GPU.", "gpu": false}}),
  );
}

#[test]
#[should_panic(expected = "ignore needs os")]
fn expectation_rejects_a_reason_without_os() {
  parse_expect("case", json!({"ignore": {"reason": "No GPU."}}));
}

#[test]
#[should_panic(expected = "ignore os must not be empty")]
fn expectation_rejects_an_empty_os_list() {
  parse_expect("case", json!({"ignore": {"reason": "No GPU.", "os": []}}));
}

#[test]
#[should_panic(expected = "ignore os must be an array")]
fn expectation_rejects_an_os_string() {
  parse_expect(
    "case",
    json!({"ignore": {"reason": "No GPU.", "os": "linux"}}),
  );
}

#[test]
#[should_panic(expected = "ignore reason must be a string")]
fn expectation_rejects_a_reason_bool() {
  parse_expect("case", json!({"ignore": {"reason": true, "os": ["linux"]}}));
}

#[test]
#[should_panic(expected = "ignore names linux twice")]
fn expectation_rejects_a_repeated_os() {
  parse_expect(
    "case",
    json!({"ignore": {"reason": "No GPU.", "os": ["linux", "linux"]}}),
  );
}

#[test]
#[should_panic(expected = "unsupported expectation field flaky")]
fn expectation_rejects_flaky() {
  parse_expect("case", json!({"flaky": "Sometimes hangs."}));
}

#[test]
#[should_panic(expected = "unsupported expectation field expectedFailures")]
fn expectation_rejects_expected_failures() {
  parse_expect("case", json!({"expectedFailures": ["name"]}));
}

#[test]
#[should_panic(expected = "use false for a whole-file failure")]
fn expectation_rejects_fail_field() {
  parse_expect("case", json!({"fail": true}));
}

#[test]
#[should_panic(expected = "unsupported expectation \"pass\"")]
fn expectation_rejects_a_status_string() {
  parse_expect("case", json!("pass"));
}
