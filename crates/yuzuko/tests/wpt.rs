//! Web-platform-tests.
//!
//! `YUZUKO_WPT_UPDATE=1` rewrites `tests/wpt/expectations`.
//! `YUZUKO_WPT_NO_IGNORE=1` also runs ignored files, including a platform
//! ignore for this host. `YUZUKO_WPT_FILTER` keeps paths that contain the
//! substring.
//!
//! `ignore` and `flaky` are a reason string, or an object of `linux`,
//! `macos`, and `windows` whose values are reasons. `flaky` retries that
//! platform up to three times.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use serde_json::Value;
use serde_json::json;

const DONE_KEY: &str = "__yuzukoWpt";

const GLOBAL_PRELUDE: &str = r#"
globalThis.GLOBAL = {
  isWindow: function() { return true; },
  isWorker: function() { return false; },
  isShadowRealm: function() { return false; }
};
// idlharness classifies the realm by the Window property. The runtime has
// no global Window constructor; this binding exists only for the test run.
if (!("Window" in globalThis)) {
  globalThis.Window = function Window() {};
}
"#;

const COMPLETION: &str = r#"
add_completion_callback(function(tests, status) {
  var results = [];
  for (var i = 0; i < tests.length; i++) {
    results.push({
      name: String(tests[i].name),
      status: tests[i].status,
      message: tests[i].message == null ? "" : String(tests[i].message)
    });
  }
  globalThis.__yuzukoWpt = JSON.stringify({
    status: status.status,
    message: status.message == null ? "" : String(status.message),
    tests: results
  });
});
"#;

/// Expectation filename, then the suite directories that file records.
const ENROLLED: &[(&str, &[&str])] = &[
  ("url", &["url"]),
  ("encoding", &["encoding"]),
  ("hr-time", &["hr-time"]),
  (
    "html",
    &[
      "html/webappapis/atob",
      "html/webappapis/timers",
      "html/webappapis/microtask-queuing",
      "html/webappapis/structured-clone",
      "html/canvas/element/manual/imagebitmap",
      "html/canvas/offscreen",
      "html/editing/dnd/datastore/datatransfer-constructor-001.html",
      "html/editing/dnd/datastore/datatransfer-getdata-url.html",
      "html/editing/dnd/datastore/datatransfer-text-plain-prefix-format.html",
      "html/editing/dnd/datastore/datatransfer-types.html",
    ],
  ),
  ("FileAPI", &["FileAPI"]),
  ("dom", &["dom/events", "dom/abort"]),
  ("wasm", &["wasm/jsapi", "wasm/webapi"]),
  ("console", &["console"]),
  ("uievents", &["uievents/constructors"]),
  ("pointerevents", &["pointerevents"]),
  ("fetch", &["fetch"]),
  ("streams", &["streams"]),
  ("compression", &["compression"]),
  ("urlpattern", &["urlpattern"]),
  ("WebCryptoAPI", &["WebCryptoAPI"]),
  ("mimesniff", &["mimesniff"]),
  ("css", &["css/geometry", "css/css-font-loading"]),
  ("gamepad", &["gamepad"]),
  ("clipboard-apis", &["clipboard-apis"]),
  ("screen-wake-lock", &["screen-wake-lock"]),
  ("workers", &["workers"]),
  ("webaudio", &["webaudio"]),
  ("requestidlecallback", &["requestidlecallback"]),
  ("eventsource", &["eventsource"]),
  ("websockets", &["websockets"]),
  ("webtransport", &["webtransport"]),
  ("webcodecs", &["webcodecs"]),
  (
    "imagebitmap-renderingcontext",
    &["imagebitmap-renderingcontext"],
  ),
  (
    "jpegxl",
    &[
      "jpegxl/imagedecoder.https.html",
      "jpegxl/imagedecoder-animation.https.html",
    ],
  ),
];

/// How many times a flaky file may run before the last attempt is reported.
const FLAKY_ATTEMPTS: u32 = 3;

/// One expectation leaf. `result` is what a host compares after it runs
/// the file. `ignore` skips the file before that comparison. `flaky`
/// retries the file until the result matches, up to [`FLAKY_ATTEMPTS`].
#[derive(Clone, Debug, PartialEq, Eq)]
struct Expect {
  result: FileResult,
  ignore: Note,
  flaky: Note,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum FileResult {
  Pass,
  Fail,
  Failures(Vec<String>),
}

/// An all-platform or per-platform annotation. Every arm carries a reason.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Note {
  No,
  All { reason: String },
  Platforms(BTreeMap<Platform, String>),
}

/// Names match `std::env::consts::OS` for the supported targets.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Platform {
  Linux,
  Macos,
  Windows,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
  Script,
  Html,
  /// The file runs in the worker global.
  Worker,
  Ignore,
}

struct Case {
  key: String,
  kind: Kind,
}

struct Subtest {
  name: String,
  status: i64,
  message: String,
}

enum Outcome {
  Error(String),
  Report {
    status: i64,
    message: String,
    tests: Vec<Subtest>,
  },
}

#[test]
fn wpt_expectations_match() {
  yuzuko_dom::register();
  yuzuko_winter::register();
  yuzuko_window::register();
  yuzuko_image::register();
  yuzuko_canvas::register();
  yuzuko_webgpu::register();
  yuzuko_audio::register();
  yuzuko_codec::register();
  let update = env_flag("YUZUKO_WPT_UPDATE");
  let no_ignore = env_flag("YUZUKO_WPT_NO_IGNORE");
  let host = Platform::host();
  let filter = std::env::var("YUZUKO_WPT_FILTER").ok();
  let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
  let suite = workspace.join("tests/wpt/suite");
  let expectations = workspace.join("tests/wpt/expectations");
  fs::create_dir_all(&expectations).expect("expectations directory");
  let harness = fs::read_to_string(suite.join("resources/testharness.js"))
    .expect("testharness.js");
  let _server = WptServer::ensure(&suite);
  let mut session = yuzuko_core::ClassicSession::new();
  let shared = SharedScripts {
    prelude: session.intern(GLOBAL_PRELUDE),
    harness: session.intern(harness),
    completion: session.intern(COMPLETION),
    load_event: session.intern(
      "if (typeof dispatchEvent === 'function') {\n  dispatchEvent(new Event('load'));\n}\n",
    ),
  };
  let mut mismatches = Vec::new();
  for (name, roots) in ENROLLED {
    let mut cases = Vec::new();
    for root in *roots {
      discover(&suite, root, &mut cases);
    }
    cases.retain(|case| {
      filter
        .as_ref()
        .is_none_or(|needle| case.key.contains(needle))
    });
    cases.sort_by(|left, right| left.key.cmp(&right.key));
    if cases.is_empty() {
      continue;
    }
    let path = expectations.join(format!("{name}.json"));
    let mut saved = read_expectations(&path, update);
    let mut seen = Vec::new();
    for case in &cases {
      seen.push(case.key.clone());
      let runner_skip = case.kind == Kind::Ignore;
      if runner_skip && !no_ignore {
        let path = suite.join(&case.key);
        if update {
          let next = preserve_runner_ignore(
            saved.get(&case.key),
            &skip_reason(&path, &case.key),
          );
          saved.insert(case.key.clone(), next);
        } else if !saved.get(&case.key).is_some_and(ignored_everywhere) {
          mismatches.push(format!(
            "{}: expected {{\"ignore\": \"reason\"}} for a file this runner skips",
            case.key
          ));
        }
        continue;
      }
      if !no_ignore
        && saved
          .get(&case.key)
          .is_some_and(|expect| noted_on(&expect.ignore, host))
      {
        continue;
      }
      let flaky = !update
        && saved
          .get(&case.key)
          .is_some_and(|expect| noted_on(&expect.flaky, host));
      let attempts = if flaky { FLAKY_ATTEMPTS } else { 1 };
      eprintln!("{}", case.key);
      let mut outcome = execute(&suite, case, &mut session, &shared);
      let mut matched = saved
        .get(&case.key)
        .is_some_and(|expect| expectation_matches(expect, &outcome));
      for attempt in 2..=attempts {
        if matched {
          break;
        }
        eprintln!("{} (attempt {attempt})", case.key);
        outcome = execute(&suite, case, &mut session, &shared);
        matched = saved
          .get(&case.key)
          .is_some_and(|expect| expectation_matches(expect, &outcome));
      }
      if update {
        let next = merge_observation(
          saved.get(&case.key),
          expect_of(&outcome),
          no_ignore,
          host,
        );
        saved.insert(case.key.clone(), next);
        continue;
      }
      match saved.get(&case.key) {
        Some(expected) if noted_on(&expected.ignore, host) => {
          mismatches.push(format!(
            "{}: ignored on {} but {}",
            case.key,
            host.as_str(),
            summarize(&outcome)
          ));
        }
        Some(expected) if expectation_matches(expected, &outcome) => {}
        Some(expected) => {
          let attempts = if flaky {
            format!(" after {FLAKY_ATTEMPTS} attempts")
          } else {
            String::new()
          };
          mismatches.push(format!(
            "{}: expected {} but {}{attempts}",
            case.key,
            format_expect(expected),
            summarize(&outcome)
          ));
        }
        None => mismatches.push(format!("{}: missing expectation", case.key)),
      }
    }
    if update {
      if filter.is_none() {
        saved.retain(|key, _| {
          seen.binary_search(key).is_ok()
            || !roots
              .iter()
              .any(|root| key.starts_with(&format!("{root}/")) || key == *root)
        });
      }
      write_expectations(&path, &saved);
      eprintln!("updated {}", path.display());
    }
  }
  if !mismatches.is_empty() {
    let shown = mismatches.len().min(40);
    panic!(
      "{} WPT expectation mismatch(es)\n{}\n{}",
      mismatches.len(),
      mismatches[..shown].join("\n"),
      if mismatches.len() > shown {
        format!("... and {} more", mismatches.len() - shown)
      } else {
        String::new()
      }
    );
  }
}

fn env_flag(name: &str) -> bool {
  std::env::var(name).ok().is_some_and(|value| value == "1")
}

fn discover(suite: &Path, root: &str, cases: &mut Vec<Case>) {
  let path = suite.join(root);
  if path.is_file() {
    push_case(suite, &path, cases);
    return;
  }
  let directory = path;
  let mut pending = vec![directory];
  while let Some(directory) = pending.pop() {
    let Ok(entries) = fs::read_dir(&directory) else {
      continue;
    };
    for entry in entries.flatten() {
      let path = entry.path();
      if path.is_dir() {
        if path
          .file_name()
          .is_some_and(|name| name == "resources" || name == "support")
        {
          continue;
        }
        pending.push(path);
        continue;
      }
      push_case(suite, &path, cases);
    }
  }
}

fn push_case(suite: &Path, path: &Path, cases: &mut Vec<Case>) {
  let Some(key) = relative_key(suite, path) else {
    return;
  };
  let Some(kind) = classify(path, &key) else {
    return;
  };
  cases.push(Case { key, kind });
}

fn relative_key(suite: &Path, path: &Path) -> Option<String> {
  let relative = path.strip_prefix(suite).ok()?;
  Some(
    relative
      .components()
      .map(|component| component.as_os_str().to_string_lossy())
      .collect::<Vec<_>>()
      .join("/"),
  )
}

fn classify(path: &Path, key: &str) -> Option<Kind> {
  let name = path.file_name()?.to_string_lossy();
  if key.contains(".sub.") || name.contains(".sub.") {
    return Some(Kind::Ignore);
  }
  if key.starts_with("webaudio/") && webaudio_ignored(path, key, &name) {
    return Some(Kind::Ignore);
  }
  if shared_or_service(key, &name) {
    return Some(Kind::Ignore);
  }
  if name.ends_with(".worker.js") {
    return Some(Kind::Worker);
  }
  if name.ends_with(".any.js") {
    let source = fs::read_to_string(path).unwrap_or_default();
    if window_variant(&source) {
      return Some(Kind::Script);
    }
    if dedicated_worker_variant(&source) {
      return Some(Kind::Worker);
    }
    return Some(Kind::Ignore);
  }
  if name.ends_with(".window.js") {
    return Some(Kind::Script);
  }
  if name.ends_with(".html") || name.ends_with(".xhtml") {
    let source = fs::read_to_string(path).unwrap_or_default();
    return Some(if html_can_run(&source) {
      Kind::Html
    } else {
      Kind::Ignore
    });
  }
  None
}

/// The file needs a media element, a `MediaStream`, `ScriptProcessorNode`,
/// or a permissions policy. `getOutputTimestamp` and `playbackStats` are
/// absent. Sink-id pages stay skipped because they need `enumerateDevices`.
/// The main global has no `requestAnimationFrame`.
/// Processing nodes, `AudioParam` automation, and `AudioListener` are part
/// of the runtime and are not skipped here.
fn webaudio_ignored(path: &Path, key: &str, name: &str) -> bool {
  webaudio_skip_reason(path, key, name).is_some()
}

fn webaudio_skip_reason(
  path: &Path,
  key: &str,
  name: &str,
) -> Option<&'static str> {
  let test_file = name.ends_with(".html")
    || name.ends_with(".xhtml")
    || name.ends_with(".window.js")
    || name.ends_with(".any.js")
    || name.ends_with(".worker.js");
  if !test_file {
    return None;
  }
  if key.contains("/the-mediaelementaudiosourcenode-interface/") {
    return Some("The runner has no media element audio source.");
  }
  if key.contains("/the-mediastreamaudiodestinationnode-interface/") {
    return Some("The runner has no MediaStreamAudioDestinationNode.");
  }
  if key.contains("/the-mediastreamaudiosourcenode-interface/") {
    return Some("The runner has no MediaStreamAudioSourceNode.");
  }
  if key.contains("/the-scriptprocessornode-interface/") {
    return Some("The runner has no ScriptProcessorNode.");
  }
  if key.contains("/media-playback-while-not-visible-permission-policy/") {
    return Some("The runner has no media playback permission policy.");
  }
  let lower = key.to_ascii_lowercase();
  if lower.contains("sinkid") {
    return Some("The runner has no setSinkId.");
  }
  if lower.contains("playbackstats") {
    return Some("The runner has no playbackStats.");
  }
  if lower.contains("getoutputtimestamp") {
    return Some("The runner has no getOutputTimestamp.");
  }
  if key.ends_with("/historical.html") {
    return Some("The runner does not implement historical Web Audio names.");
  }
  let Ok(source) = fs::read_to_string(path) else {
    return None;
  };
  // A dedicated worker has `requestAnimationFrame`. The main global does not.
  if !name.contains("worker") && source.contains("requestAnimationFrame") {
    return Some("The main global has no requestAnimationFrame.");
  }
  const NEEDLES: &[&str] = &[
    "MediaElementAudioSourceNode",
    "MediaStreamAudioDestinationNode",
    "MediaStreamAudioSourceNode",
    "ScriptProcessorNode",
    "createMediaElementSource",
    "createMediaStreamDestination",
    "createMediaStreamSource",
    "createScriptProcessor",
    "getOutputTimestamp",
    "playbackStats",
    "setSinkId",
  ];
  if NEEDLES.iter().any(|needle| source.contains(needle)) {
    return Some("The runner has no media element or MediaStream audio node.");
  }
  None
}

/// Why [`classify`] returns [`Kind::Ignore`].
fn skip_reason(path: &Path, key: &str) -> String {
  let name = path
    .file_name()
    .map(|name| name.to_string_lossy().into_owned())
    .unwrap_or_default();
  if key.contains(".sub.") || name.contains(".sub.") {
    return "The runner does not apply .sub. substitutions.".to_string();
  }
  if key.starts_with("webaudio/")
    && let Some(reason) = webaudio_skip_reason(path, key, &name)
  {
    return reason.to_string();
  }
  if shared_or_service(key, &name) {
    return "The runner has no shared worker or service worker.".to_string();
  }
  if name.ends_with(".any.js") {
    return "The runner does not run this .any.js global.".to_string();
  }
  let source = fs::read_to_string(path).unwrap_or_default();
  if !source.contains("testharness.js") {
    return "The file is not a testharness document.".to_string();
  }
  if source.contains("{{") {
    return "The runner does not apply server-side substitutions.".to_string();
  }
  let lower = source.to_ascii_lowercase();
  if lower.contains("document.") || lower.contains("document[") {
    return "The file needs a document.".to_string();
  }
  if contains_skipped_tag(&lower) {
    return "The file needs a layout element.".to_string();
  }
  "The runner skips this file.".to_string()
}

fn shared_or_service(key: &str, name: &str) -> bool {
  let key = key.to_ascii_lowercase();
  let name = name.to_ascii_lowercase();
  key.contains("service-worker")
    || key.contains("serviceworker")
    || name.contains("sharedworker")
    || key.contains("/sharedworker")
    || key.contains("sharedworker/")
}

/// `global=worker` or `dedicatedworker`, without a window or shell variant.
fn dedicated_worker_variant(source: &str) -> bool {
  let Some(value) = meta_value(source, "global") else {
    return false;
  };
  let mut worker = false;
  let mut window = false;
  for item in value.split(',') {
    match item.trim() {
      "worker" | "dedicatedworker" => worker = true,
      "window" | "jsshell" => window = true,
      _ => {}
    }
  }
  worker && !window
}

fn window_variant(source: &str) -> bool {
  match meta_value(source, "global") {
    None => true,
    Some(value) => value.split(',').any(|item| {
      let item = item.trim();
      item == "window" || item == "jsshell"
    }),
  }
}

fn html_can_run(source: &str) -> bool {
  if !source.contains("testharness.js") || source.contains("{{") {
    return false;
  }
  let lower = source.to_ascii_lowercase();
  if lower.contains("document.") || lower.contains("document[") {
    return false;
  }
  !contains_skipped_tag(&lower)
}

/// A layout element. The name must end at a tag boundary so a comparison
/// such as `i < inputData` is not treated as an `<input>` element.
fn contains_skipped_tag(lower: &str) -> bool {
  const TAGS: &[&str] = &[
    "<iframe", "<frame", "<canvas", "<svg", "<video", "<img", "<input",
    "<table", "<object", "<embed",
  ];
  TAGS.iter().any(|tag| skipped_tag(lower, tag))
}

fn skipped_tag(haystack: &str, tag: &str) -> bool {
  let mut rest = haystack;
  while let Some(index) = rest.find(tag) {
    let after = index + tag.len();
    let continues_name = rest.as_bytes().get(after).is_some_and(|byte| {
      byte.is_ascii_alphanumeric()
        || *byte == b'_'
        || *byte == b'-'
        || *byte == b':'
    });
    if !continues_name {
      return true;
    }
    rest = &rest[after..];
  }
  false
}

struct SharedScripts {
  prelude: yuzuko_core::InternedScript,
  harness: yuzuko_core::InternedScript,
  completion: yuzuko_core::InternedScript,
  load_event: yuzuko_core::InternedScript,
}

fn execute(
  suite: &Path,
  case: &Case,
  session: &mut yuzuko_core::ClassicSession,
  shared: &SharedScripts,
) -> Outcome {
  let path = suite.join(&case.key);
  let source = match fs::read_to_string(&path) {
    Ok(source) => source,
    Err(error) => return Outcome::Error(error.to_string()),
  };
  let queries = variant_queries(&source);
  let mut parts = Vec::with_capacity(queries.len());
  for query in &queries {
    parts.push(execute_query(
      suite, case, &path, &source, query, session, shared,
    ));
  }
  merge_outcomes(parts)
}

fn variant_queries(source: &str) -> Vec<String> {
  let found: Vec<String> = header_lines(source)
    .filter_map(|line| {
      line
        .strip_prefix("variant=")
        .map(|value| value.trim().to_string())
    })
    .collect();
  if found.is_empty() {
    vec![String::new()]
  } else {
    found
  }
}

fn execute_query(
  suite: &Path,
  case: &Case,
  path: &Path,
  source: &str,
  search: &str,
  session: &mut yuzuko_core::ClassicSession,
  shared: &SharedScripts,
) -> Outcome {
  let base = network_base(&case.key);
  let mut entry = base.map(|base| page_url(base, &case.key, search));
  let location = location_script(path, search, base, &case.key);
  let fetch = fetch_script(suite, base);
  let mut extra = Vec::new();
  let mut worker_scripts = None;
  // Holds the in-memory worker body until this query finishes.
  let _worker_source = match case.kind {
    Kind::Ignore => {
      return Outcome::Error("ignored file was executed".to_string());
    }
    Kind::Worker => {
      let url = format!("{WPT_HTTP}/{}", case.key);
      entry = Some(url.clone());
      let registered = if case.key.ends_with(".any.js") {
        let mut parts = Vec::new();
        if let Err(error) = push_meta_scripts(
          suite,
          &case.key,
          source,
          &mut parts,
          Some(WPT_HTTP),
        ) {
          return Outcome::Error(error);
        }
        parts.insert(0, session.interned_source(shared.harness).to_string());
        parts.push(source.to_string());
        if !source.contains("done()") {
          // Dedicated workers wait for an explicit `done()`. The WPT server
          // appends it when it wraps a worker `.any.js`.
          parts.push("done();\n".to_string());
        }
        // The script URL stays on the WPT origin. A data: URL cannot resolve
        // a path-absolute importScripts, so the harness body is served for
        // this http URL instead of being inlined into a data: worker.
        yuzuko_core::set_classic_worker_source(Some((
          url.clone(),
          parts.join("\n"),
        )));
        Some(ClassicWorkerSourceGuard)
      } else {
        None
      };
      worker_scripts = Some(vec![worker_parent(&url)]);
      registered
    }
    Kind::Script => {
      if let Err(error) =
        push_meta_scripts(suite, &case.key, source, &mut extra, base)
      {
        return Outcome::Error(error);
      }
      extra.push(source.to_string());
      None
    }
    Kind::Html => {
      if let Err(error) =
        push_html_scripts(suite, &case.key, source, &mut extra, base)
      {
        return Outcome::Error(error);
      }
      None
    }
  };
  let deadline = if timeout_long(source) {
    Duration::from_secs(30)
  } else if matches!(case.kind, Kind::Worker) {
    // Font and fetch work in a worker can still be in flight when the
    // owner marks the harness done. Give that a little room.
    Duration::from_secs(15)
  } else {
    Duration::from_secs(8)
  };
  let run = if let Some(scripts) = worker_scripts {
    session.run(
      &scripts,
      DONE_KEY,
      deadline,
      Some(path),
      Some(suite),
      entry.as_deref(),
    )
  } else {
    let mut parts = vec![
      yuzuko_core::ClassicPart::Shared(shared.prelude),
      yuzuko_core::ClassicPart::Source(&location),
      yuzuko_core::ClassicPart::Source(&fetch),
      yuzuko_core::ClassicPart::Shared(shared.harness),
      yuzuko_core::ClassicPart::Shared(shared.completion),
    ];
    for script in &extra {
      parts.push(yuzuko_core::ClassicPart::Source(script));
    }
    if case.kind == Kind::Html {
      // Window tests can wait for `load`. There is no document, so dispatch
      // it after the page scripts have registered listeners.
      parts.push(yuzuko_core::ClassicPart::Shared(shared.load_event));
    }
    session.run_parts(
      &parts,
      DONE_KEY,
      deadline,
      Some(path),
      Some(suite),
      entry.as_deref(),
    )
  };
  match run {
    Ok(run) => match run.error {
      Some(error) => Outcome::Error(error),
      None => match run.value {
        Some(json) => parse_report(&json),
        None => Outcome::Error("harness did not finish".to_string()),
      },
    },
    Err(error) => Outcome::Error(error.message),
  }
}

/// Parent page for a dedicated-worker test. The worker posts testharness
/// `complete` and this page stores the same JSON shape as `COMPLETION`.
fn worker_parent(url: &str) -> String {
  let url = serde_json::to_string(url).unwrap_or_else(|_| "\"\"".to_string());
  let start = format!("var worker = new Worker({url});");
  format!(
    r#"{start}
worker.onmessage = function(event) {{
  var data = event.data;
  if (!data || data.type !== "complete") {{
    return;
  }}
  var results = [];
  var tests = data.tests || [];
  for (var i = 0; i < tests.length; i++) {{
    var test = tests[i];
    results.push({{
      name: String(test.name),
      status: test.status,
      message: test.message == null ? "" : String(test.message)
    }});
  }}
  var status = data.status || {{}};
  globalThis.__yuzukoWpt = JSON.stringify({{
    status: status.status,
    message: status.message == null ? "" : String(status.message),
    tests: results
  }});
}};
worker.onerror = function(event) {{
  if (globalThis.__yuzukoWpt) {{
    return;
  }}
  globalThis.__yuzukoWpt = JSON.stringify({{
    status: 1,
    message: event && event.message ? String(event.message) : "worker error",
    tests: []
  }});
}};
"#
  )
}

const WPT_HTTP: &str = "http://web-platform.test:8000";

struct ClassicWorkerSourceGuard;

impl Drop for ClassicWorkerSourceGuard {
  fn drop(&mut self) {
    yuzuko_core::set_classic_worker_source(None);
  }
}

/// These suites request a network origin, so they use the WPT server
/// instead of `file:`.
fn network_base(key: &str) -> Option<&'static str> {
  if key.starts_with("fetch/")
    || key.starts_with("mimesniff/")
    || key.starts_with("wasm/webapi/")
    || key.starts_with("webaudio/")
    || key.starts_with("eventsource/")
    || key.starts_with("websockets/")
    || key.starts_with("webcodecs/")
    || key.starts_with("jpegxl/")
    || (key.starts_with("WebCryptoAPI/") && !key.contains(".https."))
    // The XMLHttpRequest variant fetches resources/single-byte-raw.py.
    || key == "encoding/single-byte-decoder.any.js"
    // receiving-worker.js loads `/resources/testharness.js`. A file: base
    // resolves that path from the filesystem root.
    || key == "streams/transferable/worker.html"
  {
    Some(WPT_HTTP)
  } else {
    None
  }
}

fn page_url(base: &str, key: &str, search: &str) -> String {
  let mut href = format!("{base}/{key}");
  if !search.is_empty() {
    if let Some(query) = search.strip_prefix('?') {
      href.push('?');
      href.push_str(query);
    } else {
      href.push('?');
      href.push_str(search);
    }
  }
  href
}

struct WptServer {
  child: Option<Child>,
}

/// Python 3.15 removed `http.server.CGIHTTPRequestHandler`. Bundled
/// pywebsocket3 still subclasses it. The suite `.python-version` can also
/// name an interpreter that is not installed, so probe candidates from the
/// suite directory and keep the first one that still has the class.
fn wpt_python(suite: &Path) -> String {
  const CANDIDATES: &[&str] = &[
    "python3.14",
    "python3.13",
    "python3.12",
    "python3.11",
    "python3.10",
    "python3.9",
    "python3",
    // The python.org installer for Windows names the interpreter `python`.
    "python",
  ];
  for candidate in CANDIDATES {
    let ok = Command::new(*candidate)
      .arg("-c")
      .arg(
        "import http.server as http\n\
         raise SystemExit(\
           0 if hasattr(http, 'CGIHTTPRequestHandler') else 1)",
      )
      .current_dir(suite)
      .stdin(Stdio::null())
      .stdout(Stdio::null())
      .stderr(Stdio::null())
      .status()
      .map(|status| status.success())
      .unwrap_or(false);
    if ok {
      return (*candidate).to_string();
    }
  }
  panic!("WPT server needs a Python with http.server.CGIHTTPRequestHandler");
}

impl WptServer {
  fn ensure(suite: &Path) -> Self {
    if port_open() {
      return Self { child: None };
    }
    let child = Command::new(wpt_python(suite))
      .arg("wpt")
      .arg("serve")
      .arg("--no-h2")
      .current_dir(suite)
      .stdin(Stdio::null())
      .stdout(Stdio::null())
      .stderr(Stdio::null())
      .spawn()
      .expect("start the WPT server");
    let deadline = std::time::Instant::now() + Duration::from_secs(120);
    while std::time::Instant::now() < deadline {
      if port_open() {
        return Self { child: Some(child) };
      }
      std::thread::sleep(Duration::from_millis(200));
    }
    let mut child = child;
    let _ = child.kill();
    let _ = child.wait();
    panic!("WPT server did not listen on 127.0.0.1:8000");
  }
}

impl Drop for WptServer {
  fn drop(&mut self) {
    if let Some(child) = self.child.as_mut() {
      let _ = child.kill();
      let _ = child.wait();
    }
  }
}

fn port_open() -> bool {
  std::net::TcpStream::connect_timeout(
    &std::net::SocketAddr::from(([127, 0, 0, 1], 8000)),
    Duration::from_millis(200),
  )
  .is_ok()
}

/// A root-relative URL such as `/images/red.png` is a file under the suite.
/// `file:` resolution would replace the path from the filesystem root, and
/// the sandbox would reject it.
fn fetch_script(suite: &Path, base: Option<&str>) -> String {
  let root = base
    .map(str::to_string)
    .unwrap_or_else(|| yuzuko_core::file_url(suite).unwrap_or_default());
  let root = root.trim_end_matches('/');
  let root = serde_json::to_string(root).unwrap_or_else(|_| "\"\"".to_string());
  format!(
    "\
(function () {{
  var root = {root};
  var original = globalThis.fetch;
  globalThis.fetch = function fetch(input) {{
    if (this != null && this !== globalThis) {{
      return Promise.reject(new TypeError(\"illegal invocation\"));
    }}
    if (arguments.length < 1) {{
      return Promise.reject(new TypeError(\"fetch requires a URL\"));
    }}
    var init = arguments[1];
    if (typeof input === \"string\" && input.charAt(0) === \"/\" && input.charAt(1) !== \"/\") {{
      input = root + input;
    }}
    return original.call(globalThis, input, init);
  }};
}})();
"
  )
}

/// Present only while a WPT file runs, so a variant query can read `search`.
fn location_script(
  path: &Path,
  search: &str,
  base: Option<&str>,
  key: &str,
) -> String {
  let (href, pathname, protocol, host, hostname, port, origin) =
    if let Some(base) = base {
      (
        page_url(base, key, search),
        format!("/{key}"),
        "http:".to_string(),
        "web-platform.test:8000".to_string(),
        "web-platform.test".to_string(),
        "8000".to_string(),
        WPT_HTTP.to_string(),
      )
    } else {
      (
        yuzuko_core::file_url(path).unwrap_or_default(),
        String::new(),
        "file:".to_string(),
        String::new(),
        String::new(),
        String::new(),
        "null".to_string(),
      )
    };
  let href =
    serde_json::to_string(&href).unwrap_or_else(|_| "\"\"".to_string());
  let pathname =
    serde_json::to_string(&pathname).unwrap_or_else(|_| "\"\"".to_string());
  let search =
    serde_json::to_string(search).unwrap_or_else(|_| "\"\"".to_string());
  let protocol =
    serde_json::to_string(&protocol).unwrap_or_else(|_| "\"\"".to_string());
  let host =
    serde_json::to_string(&host).unwrap_or_else(|_| "\"\"".to_string());
  let hostname =
    serde_json::to_string(&hostname).unwrap_or_else(|_| "\"\"".to_string());
  let port =
    serde_json::to_string(&port).unwrap_or_else(|_| "\"\"".to_string());
  let origin =
    serde_json::to_string(&origin).unwrap_or_else(|_| "\"\"".to_string());
  // This file is the non-secure-context check. Other http WPT, including
  // `.https.any.js` served on the plain port, still use `crypto.subtle`.
  let strip_subtle = if key == "WebCryptoAPI/historical.any.js" {
    "\
if (globalThis.Crypto && globalThis.Crypto.prototype) {
  delete globalThis.Crypto.prototype.subtle;
}
delete globalThis.SubtleCrypto;
delete globalThis.CryptoKey;
"
  } else {
    ""
  };
  format!(
    "\
globalThis.location = {{
  href: {href},
  search: {search},
  hash: \"\",
  pathname: {pathname},
  protocol: {protocol},
  host: {host},
  hostname: {hostname},
  port: {port},
  origin: {origin},
  toString: function() {{ return this.href; }}
}};
{strip_subtle}
"
  )
}

fn merge_outcomes(parts: Vec<Outcome>) -> Outcome {
  let mut tests = Vec::new();
  let mut status = 0;
  let mut message = String::new();
  for part in parts {
    match part {
      Outcome::Error(error) => return Outcome::Error(error),
      Outcome::Report {
        status: part_status,
        message: part_message,
        tests: part_tests,
      } => {
        if part_status != 0 && part_status != 3 {
          status = part_status;
          if message.is_empty() {
            message = part_message;
          }
        } else if message.is_empty() {
          message = part_message;
        }
        tests.extend(part_tests);
      }
    }
  }
  Outcome::Report {
    status,
    message,
    tests,
  }
}

fn push_meta_scripts(
  suite: &Path,
  key: &str,
  source: &str,
  scripts: &mut Vec<String>,
  base: Option<&str>,
) -> Result<(), String> {
  for script in meta_scripts(source) {
    push_support(suite, key, &script, scripts, base)?;
  }
  Ok(())
}

fn meta_scripts(source: &str) -> Vec<String> {
  let mut scripts = Vec::new();
  for line in header_lines(source) {
    let Some(value) = line.strip_prefix("script=") else {
      continue;
    };
    scripts.push(value.trim().to_string());
  }
  scripts
}

fn push_html_scripts(
  suite: &Path,
  key: &str,
  source: &str,
  scripts: &mut Vec<String>,
  base: Option<&str>,
) -> Result<(), String> {
  let lower = source.to_ascii_lowercase();
  let mut cursor = 0;
  while let Some(offset) = lower[cursor..].find("<script") {
    let start = cursor + offset;
    let Some(header_end) = lower[start..].find('>') else {
      break;
    };
    let header_end = start + header_end;
    let header = &source[start..=header_end];
    let header_lower = &lower[start..=header_end];
    if let Some(src) = script_src(header) {
      if !src.ends_with("testharness.js")
        && !src.ends_with("testharnessreport.js")
      {
        push_support(suite, key, &src, scripts, base)?;
      }
      cursor = header_end + 1;
      continue;
    }
    if header_lower.contains("src=") {
      cursor = header_end + 1;
      continue;
    }
    let Some(close) = lower[header_end..].find("</script") else {
      break;
    };
    let body_start = header_end + 1;
    let body_end = header_end + close;
    scripts.push(source[body_start..body_end].to_string());
    cursor = body_end;
  }
  Ok(())
}

fn script_src(header: &str) -> Option<String> {
  let lower = header.to_ascii_lowercase();
  let index = lower.find("src=")?;
  let rest = header[index + 4..].trim_start();
  let quote = rest.chars().next()?;
  if quote == '"' || quote == '\'' {
    let rest = &rest[1..];
    let end = rest.find(quote)?;
    return Some(rest[..end].to_string());
  }
  let end = rest
    .find(|ch: char| ch.is_whitespace() || ch == '>')
    .unwrap_or(rest.len());
  Some(rest[..end].to_string())
}

fn push_support(
  suite: &Path,
  key: &str,
  specifier: &str,
  scripts: &mut Vec<String>,
  base: Option<&str>,
) -> Result<(), String> {
  // wpt serve rewrites this historical name to the webidl2 build.
  let specifier = if specifier == "/resources/WebIDLParser.js" {
    "/resources/webidl2/lib/webidl2.js"
  } else {
    specifier
  };
  let path = if let Some(absolute) = specifier.strip_prefix('/') {
    suite.join(absolute)
  } else {
    let parent = Path::new(key).parent().unwrap_or(Path::new(""));
    suite.join(parent).join(specifier)
  };
  let text = fs::read_to_string(&path)
    .map_err(|_| format!("missing script {specifier}"))?;
  if text.contains("{{")
    && let Some(base) = base
    && let Some(substituted) = fetch_substituted(base, key, specifier)
  {
    scripts.push(substituted);
    return Ok(());
  }
  scripts.push(text);
  Ok(())
}

fn fetch_substituted(base: &str, key: &str, specifier: &str) -> Option<String> {
  let url = script_url(base, key, specifier)?;
  let output = Command::new("curl")
    .args(["-fsS", "--max-time", "10", &url])
    .output()
    .ok()?;
  if !output.status.success() {
    return None;
  }
  String::from_utf8(output.stdout).ok()
}

fn script_url(base: &str, key: &str, specifier: &str) -> Option<String> {
  let parts: Vec<&str> = if let Some(absolute) = specifier.strip_prefix('/') {
    absolute
      .split('/')
      .filter(|part| !part.is_empty())
      .collect()
  } else {
    let mut parts: Vec<&str> = Path::new(key)
      .parent()
      .unwrap_or(Path::new(""))
      .components()
      .filter_map(|component| component.as_os_str().to_str())
      .collect();
    for part in specifier.split('/') {
      if part.is_empty() || part == "." {
        continue;
      }
      if part == ".." {
        parts.pop();
      } else {
        parts.push(part);
      }
    }
    parts
  };
  Some(format!(
    "{}/{}",
    base.trim_end_matches('/'),
    parts.join("/")
  ))
}

fn timeout_long(source: &str) -> bool {
  meta_value(source, "timeout").is_some_and(|value| value == "long")
    || source.contains("content=\"long\"")
    || source.contains("content=long")
}

fn meta_value(source: &str, name: &str) -> Option<String> {
  let prefix = format!("{name}=");
  header_lines(source).find_map(|line| {
    line
      .strip_prefix(&prefix)
      .map(|value| value.trim().to_string())
  })
}

fn header_lines(source: &str) -> impl Iterator<Item = String> + '_ {
  source
    .lines()
    .map(|line| line.trim().to_string())
    .take_while(|line| {
      line.is_empty()
        || line.starts_with("//")
        || line.starts_with("/*")
        || line.starts_with('*')
    })
    .filter_map(|line| {
      let line = line.trim_start_matches("//").trim();
      line
        .strip_prefix("META:")
        .map(|value| value.trim().to_string())
    })
}

fn parse_report(json: &str) -> Outcome {
  let Ok(value) = serde_json::from_str::<Value>(json) else {
    return Outcome::Error(format!("bad harness report: {json}"));
  };
  let status = value.get("status").and_then(Value::as_i64).unwrap_or(1);
  let message = value
    .get("message")
    .and_then(Value::as_str)
    .unwrap_or("")
    .to_string();
  let mut tests = Vec::new();
  if let Some(list) = value.get("tests").and_then(Value::as_array) {
    for test in list {
      tests.push(Subtest {
        name: test
          .get("name")
          .and_then(Value::as_str)
          .unwrap_or("")
          .to_string(),
        status: test.get("status").and_then(Value::as_i64).unwrap_or(1),
        message: test
          .get("message")
          .and_then(Value::as_str)
          .unwrap_or("")
          .to_string(),
      });
    }
  }
  Outcome::Report {
    status,
    message,
    tests,
  }
}

impl Platform {
  fn host() -> Self {
    match std::env::consts::OS {
      "linux" => Self::Linux,
      "macos" => Self::Macos,
      "windows" => Self::Windows,
      other => panic!("unsupported WPT platform {other}"),
    }
  }

  fn as_str(self) -> &'static str {
    match self {
      Self::Linux => "linux",
      Self::Macos => "macos",
      Self::Windows => "windows",
    }
  }

  fn parse(name: &str) -> Option<Self> {
    match name {
      "linux" => Some(Self::Linux),
      "macos" => Some(Self::Macos),
      "windows" => Some(Self::Windows),
      _ => None,
    }
  }
}

fn ignored_everywhere(expect: &Expect) -> bool {
  matches!(expect.ignore, Note::All { .. })
}

fn noted_on(note: &Note, host: Platform) -> bool {
  match note {
    Note::No => false,
    Note::All { .. } => true,
    Note::Platforms(map) => map.contains_key(&host),
  }
}

/// Update keeps an all-platform reason. A platform map becomes an
/// all-platform ignore. When the platform reasons differ, `fallback` is
/// the reason.
fn preserve_runner_ignore(previous: Option<&Expect>, fallback: &str) -> Expect {
  let reason = match previous.map(|expect| &expect.ignore) {
    Some(Note::All { reason }) => reason.clone(),
    Some(Note::Platforms(map)) => shared_reason(map, fallback),
    _ => fallback.to_string(),
  };
  Expect {
    result: FileResult::Pass,
    ignore: Note::All { reason },
    flaky: Note::No,
  }
}

fn shared_reason(map: &BTreeMap<Platform, String>, fallback: &str) -> String {
  let mut found = None;
  for reason in map.values() {
    match &found {
      None => found = Some(reason.clone()),
      Some(previous) if previous == reason => {}
      Some(_) => return fallback.to_string(),
    }
  }
  found.unwrap_or_else(|| fallback.to_string())
}

/// Record `actual` for this host. Ignores for other hosts stay.
/// `no_ignore` drops the ignore that covered this host. `flaky` stays.
fn merge_observation(
  previous: Option<&Expect>,
  actual: FileResult,
  no_ignore: bool,
  host: Platform,
) -> Expect {
  let ignore = match previous.map(|expect| &expect.ignore) {
    Some(ignore) => retain_note(ignore, no_ignore, host),
    None => Note::No,
  };
  let flaky = previous
    .map(|expect| expect.flaky.clone())
    .unwrap_or(Note::No);
  Expect {
    result: actual,
    ignore,
    flaky,
  }
}

fn retain_note(note: &Note, drop_host: bool, host: Platform) -> Note {
  match note {
    Note::No => Note::No,
    Note::All { .. } if drop_host => Note::No,
    Note::All { .. } => note.clone(),
    Note::Platforms(map) => {
      let mut map = map.clone();
      if drop_host {
        map.remove(&host);
      }
      if map.is_empty() {
        Note::No
      } else {
        Note::Platforms(map)
      }
    }
  }
}

fn expect_of(outcome: &Outcome) -> FileResult {
  match outcome {
    Outcome::Error(_) => FileResult::Fail,
    Outcome::Report { status, tests, .. } => {
      if *status != 0 && *status != 3 {
        return FileResult::Fail;
      }
      let mut failed: Vec<String> = tests
        .iter()
        .filter(|test| test.status != 0 && test.status != 4)
        .map(|test| test.name.clone())
        .collect();
      failed.sort();
      failed.dedup();
      if failed.is_empty() {
        FileResult::Pass
      } else {
        FileResult::Failures(failed)
      }
    }
  }
}

fn expectation_matches(expect: &Expect, outcome: &Outcome) -> bool {
  match &expect.result {
    FileResult::Pass => expect_of(outcome) == FileResult::Pass,
    FileResult::Fail => expect_of(outcome) != FileResult::Pass,
    FileResult::Failures(expected) => match expect_of(outcome) {
      FileResult::Failures(actual) => expected == &actual,
      _ => false,
    },
  }
}

fn summarize(outcome: &Outcome) -> String {
  match outcome {
    Outcome::Error(error) => format!("error: {}", truncate(error)),
    Outcome::Report {
      status,
      message,
      tests,
    } => {
      let failed: Vec<&str> = tests
        .iter()
        .filter(|test| test.status != 0 && test.status != 4)
        .map(|test| test.name.as_str())
        .take(4)
        .collect();
      let detail = tests
        .iter()
        .find(|test| test.status != 0 && test.status != 4)
        .map(|test| test.message.as_str())
        .unwrap_or("");
      format!(
        "status {status} ({} subtests, failed {failed:?}) {} {}",
        tests.len(),
        truncate(message),
        truncate(detail)
      )
    }
  }
}

fn truncate(text: &str) -> String {
  let text = text.replace('\n', " ");
  if text.chars().count() > 180 {
    format!("{}…", text.chars().take(180).collect::<String>())
  } else {
    text
  }
}

fn format_expect(expect: &Expect) -> String {
  let mut result = match &expect.result {
    FileResult::Pass => "pass".to_string(),
    FileResult::Fail => "file failure".to_string(),
    FileResult::Failures(names) => format!("failures {names:?}"),
  };
  result = format_note(&result, "flaky", &expect.flaky);
  format_note(&result, "ignored", &expect.ignore)
}

fn format_note(result: &str, label: &str, note: &Note) -> String {
  match note {
    Note::No => result.to_string(),
    Note::All { reason } => {
      if result == "pass" {
        format!("{label} ({reason})")
      } else {
        format!("{result}, {label} ({reason})")
      }
    }
    Note::Platforms(map) => {
      format!("{result}, {label} on {}", format_platforms(map))
    }
  }
}

fn format_platforms(map: &BTreeMap<Platform, String>) -> String {
  let parts: Vec<String> = map
    .iter()
    .map(|(platform, reason)| format!("{} ({reason})", platform.as_str()))
    .collect();
  parts.join(", ")
}

fn read_expectations(path: &Path, update: bool) -> BTreeMap<String, Expect> {
  if !path.exists() {
    if update {
      return BTreeMap::new();
    }
    return BTreeMap::new();
  }
  let text = fs::read_to_string(path).unwrap_or_else(|error| {
    panic!("read {}: {error}", path.display());
  });
  let Ok(Value::Object(map)) = serde_json::from_str(&text) else {
    panic!("{} is not a JSON object", path.display());
  };
  let mut expectations = BTreeMap::new();
  for (key, value) in map {
    expectations.insert(key.clone(), parse_expect(&key, value));
  }
  expectations
}

fn parse_expect(key: &str, value: Value) -> Expect {
  match value {
    Value::Bool(true) => Expect {
      result: FileResult::Pass,
      ignore: Note::No,
      flaky: Note::No,
    },
    Value::Bool(false) => Expect {
      result: FileResult::Fail,
      ignore: Note::No,
      flaky: Note::No,
    },
    Value::Object(map) => parse_expect_object(key, map),
    other => panic!("{key}: unsupported expectation {other}"),
  }
}

fn parse_expect_object(
  key: &str,
  map: serde_json::Map<String, Value>,
) -> Expect {
  for name in map.keys() {
    match name.as_str() {
      "ignore" | "flaky" | "expectedFailures" | "fail" => {}
      "reason" => {
        panic!("{key}: write the reason as the value of ignore or flaky")
      }
      other => panic!("{key}: unsupported expectation field {other}"),
    }
  }
  let ignore = parse_mark(key, "ignore", map.get("ignore"));
  let flaky = parse_mark(key, "flaky", map.get("flaky"));
  if matches!(ignore, Note::All { .. }) && !matches!(flaky, Note::No) {
    panic!("{key}: an all-platform ignore cannot combine with flaky");
  }
  if matches!(flaky, Note::All { .. }) && !matches!(ignore, Note::No) {
    panic!("{key}: an all-platform flaky cannot combine with ignore");
  }
  if let (Note::Platforms(left), Note::Platforms(right)) = (&ignore, &flaky)
    && left.keys().any(|platform| right.contains_key(platform))
  {
    panic!("{key}: ignore and flaky name the same platform");
  }
  let has_failures = map.contains_key("expectedFailures");
  let has_fail = map.contains_key("fail");
  if has_failures && has_fail {
    panic!("{key}: fail and expectedFailures cannot combine");
  }
  if matches!(ignore, Note::All { .. }) && (has_failures || has_fail) {
    panic!("{key}: an all-platform ignore cannot set expectedFailures or fail");
  }
  let result = if has_fail {
    match map.get("fail") {
      Some(Value::Bool(true)) => FileResult::Fail,
      _ => panic!("{key}: fail must be true"),
    }
  } else if let Some(list) = map.get("expectedFailures") {
    let Some(list) = list.as_array() else {
      panic!("{key}: expectedFailures must be an array of strings");
    };
    let mut names = Vec::new();
    for item in list {
      let Some(name) = item.as_str() else {
        panic!("{key}: expectedFailures must be an array of strings");
      };
      names.push(name.to_string());
    }
    names.sort();
    names.dedup();
    FileResult::Failures(names)
  } else {
    FileResult::Pass
  };
  if matches!(result, FileResult::Fail)
    && matches!(ignore, Note::No)
    && matches!(flaky, Note::No)
  {
    panic!("{key}: use false for a whole-file failure");
  }
  Expect {
    result,
    ignore,
    flaky,
  }
}

fn parse_mark(key: &str, label: &str, value: Option<&Value>) -> Note {
  match value {
    None => Note::No,
    Some(Value::String(text)) => Note::All {
      reason: nonempty_reason(key, label, text),
    },
    Some(Value::Object(platforms)) => parse_platforms(key, label, platforms),
    Some(_) => {
      panic!("{key}: {label} must be a reason string or a platform object")
    }
  }
}

fn parse_platforms(
  key: &str,
  label: &str,
  value: &serde_json::Map<String, Value>,
) -> Note {
  if value.is_empty() {
    panic!("{key}: {label} lists no platform");
  }
  let mut platforms = BTreeMap::new();
  for (name, entry) in value {
    let Some(platform) = Platform::parse(name) else {
      panic!("{key}: platform must be \"linux\", \"macos\", or \"windows\"");
    };
    let Value::String(text) = entry else {
      panic!("{key}: {label} for {name} must be a reason string");
    };
    let reason = nonempty_reason(key, &format!("reason for {name}"), text);
    platforms.insert(platform, reason);
  }
  if platforms.len() == 3 {
    let mut values = platforms.values();
    let first = values.next().expect("platform").clone();
    if values.all(|reason| reason == &first) {
      return Note::All { reason: first };
    }
  }
  Note::Platforms(platforms)
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
  let text = serde_json::to_string_pretty(&Value::Object(map)).expect("json");
  fs::write(path, format!("{text}\n")).expect("write expectations");
}

fn expect_json(expect: &Expect) -> Value {
  if matches!(expect.ignore, Note::No) && matches!(expect.flaky, Note::No) {
    return match &expect.result {
      FileResult::Pass => Value::Bool(true),
      FileResult::Fail => Value::Bool(false),
      FileResult::Failures(names) => json!({"expectedFailures": names}),
    };
  }
  if matches!(expect.ignore, Note::All { .. }) {
    return note_object("ignore", &expect.ignore);
  }
  if matches!(expect.flaky, Note::All { .. })
    && matches!(expect.result, FileResult::Pass)
  {
    return note_object("flaky", &expect.flaky);
  }
  let mut map = serde_json::Map::new();
  match &expect.result {
    FileResult::Pass => {}
    FileResult::Fail => {
      map.insert("fail".to_string(), Value::Bool(true));
    }
    FileResult::Failures(names) => {
      map.insert("expectedFailures".to_string(), json!(names));
    }
  }
  insert_note(&mut map, "flaky", &expect.flaky);
  insert_note(&mut map, "ignore", &expect.ignore);
  Value::Object(map)
}

fn note_object(name: &str, note: &Note) -> Value {
  let Note::All { reason } = note else {
    unreachable!("all-platform note");
  };
  json!({ name: reason })
}

fn insert_note(
  map: &mut serde_json::Map<String, Value>,
  name: &str,
  note: &Note,
) {
  match note {
    Note::No => {}
    Note::All { reason } => {
      map.insert(name.to_string(), Value::String(reason.clone()));
    }
    Note::Platforms(platforms) => {
      map.insert(name.to_string(), platforms_json(platforms));
    }
  }
}

fn platforms_json(platforms: &BTreeMap<Platform, String>) -> Value {
  let mut map = serde_json::Map::new();
  for (platform, reason) in platforms {
    map.insert(platform.as_str().to_string(), Value::String(reason.clone()));
  }
  Value::Object(map)
}

#[test]
fn expectation_ignore_reason_round_trips() {
  let value = json!({"ignore": "Needs a document"});
  let expect = parse_expect("example.js", value.clone());
  assert!(ignored_everywhere(&expect));
  assert!(noted_on(&expect.ignore, Platform::Linux));
  assert!(noted_on(&expect.ignore, Platform::Macos));
  assert!(noted_on(&expect.ignore, Platform::Windows));
  assert_eq!(expect_json(&expect), value);
  let spaced =
    parse_expect("example.js", json!({"ignore": "  Needs a document  "}));
  assert_eq!(expect_json(&spaced), value);
}

#[test]
fn expectation_platform_ignore_round_trips() {
  let value = json!({
    "expectedFailures": ["interlaced"],
    "ignore": {
      "linux": "No VA-API",
      "windows": "No DXVA"
    }
  });
  let expect = parse_expect("example.js", value.clone());
  assert!(noted_on(&expect.ignore, Platform::Linux));
  assert!(noted_on(&expect.ignore, Platform::Windows));
  assert!(!noted_on(&expect.ignore, Platform::Macos));
  assert!(!ignored_everywhere(&expect));
  assert_eq!(expect_json(&expect), value);
  assert_eq!(
    expect_json(&parse_expect(
      "example.js",
      json!({"fail": true, "ignore": {"linux": "No VA-API"}}),
    )),
    json!({"fail": true, "ignore": {"linux": "No VA-API"}}),
  );
}

#[test]
fn expectation_bool_and_failures_round_trip() {
  assert_eq!(
    expect_json(&parse_expect("example.js", json!(true))),
    json!(true)
  );
  assert_eq!(
    expect_json(&parse_expect("example.js", json!(false))),
    json!(false)
  );
  assert_eq!(
    expect_json(&parse_expect(
      "example.js",
      json!({"ignore": "Needs a document"}),
    )),
    json!({"ignore": "Needs a document"})
  );
  let parsed =
    parse_expect("example.js", json!({"expectedFailures": ["b", "a", "a"]}));
  assert_eq!(
    expect_json(&parsed),
    json!({"expectedFailures": ["a", "b"]})
  );
}

#[test]
fn expectation_update_keeps_reason_and_other_platforms() {
  let documented =
    parse_expect("example.js", json!({"ignore": "Needs a document"}));
  assert_eq!(
    expect_json(&preserve_runner_ignore(
      Some(&documented),
      "The runner skips this file."
    )),
    json!({"ignore": "Needs a document"})
  );
  let linux =
    parse_expect("example.js", json!({"ignore": {"linux": "No VA-API"}}));
  assert_eq!(
    expect_json(&preserve_runner_ignore(
      Some(&linux),
      "The runner skips this file."
    )),
    json!({"ignore": "No VA-API"})
  );
  let split = parse_expect(
    "example.js",
    json!({
      "ignore": {"linux": "No VA-API", "windows": "No DXVA"}
    }),
  );
  assert_eq!(
    expect_json(&preserve_runner_ignore(
      Some(&split),
      "The runner skips this file."
    )),
    json!({"ignore": "The runner skips this file."})
  );

  let previous = parse_expect(
    "example.js",
    json!({"fail": true, "ignore": {"linux": "No VA-API"}}),
  );
  let observed = merge_observation(
    Some(&previous),
    FileResult::Failures(vec!["one".to_string()]),
    false,
    Platform::Macos,
  );
  assert_eq!(
    expect_json(&observed),
    json!({
      "expectedFailures": ["one"],
      "ignore": {"linux": "No VA-API"}
    })
  );
  let forced =
    merge_observation(Some(&previous), FileResult::Pass, true, Platform::Linux);
  assert_eq!(expect_json(&forced), json!(true));
  assert!(!expectation_matches(
    &previous,
    &Outcome::Report {
      status: 0,
      message: String::new(),
      tests: Vec::new(),
    }
  ));
  assert!(expectation_matches(
    &parse_expect("example.js", json!(false)),
    &Outcome::Error("boom".to_string())
  ));
}

#[test]
fn expectation_platform_names_match_the_host() {
  assert_eq!(Platform::host().as_str(), std::env::consts::OS);
}

#[test]
#[should_panic(
  expected = "platform must be \"linux\", \"macos\", or \"windows\""
)]
fn expectation_rejects_darwin_as_a_platform() {
  parse_expect("example.js", json!({"ignore": {"darwin": "VideoToolbox"}}));
}

#[test]
#[should_panic(expected = "ignore must not be empty")]
fn expectation_rejects_an_empty_reason() {
  parse_expect("example.js", json!({"ignore": "  "}));
}

#[test]
#[should_panic(expected = "write the reason as the value of ignore or flaky")]
fn expectation_rejects_a_sibling_reason() {
  parse_expect(
    "example.js",
    json!({"ignore": "No VA-API", "reason": "No VA-API"}),
  );
}

#[test]
#[should_panic(expected = "must be a reason string or a platform object")]
fn expectation_rejects_an_ignore_without_a_reason() {
  parse_expect("example.js", json!({"ignore": true}));
}

#[test]
#[should_panic(expected = "must be a reason string")]
fn expectation_rejects_a_platform_ignore_without_a_reason() {
  parse_expect("example.js", json!({"ignore": {"linux": true}}));
}

#[test]
fn expectation_flaky_round_trips() {
  let value = json!({
    "expectedFailures": ["Cookie should be set"],
    "flaky": {"macos": "The handshake does not finish."}
  });
  let expect = parse_expect("example.js", value.clone());
  assert!(noted_on(&expect.flaky, Platform::Macos));
  assert!(!noted_on(&expect.flaky, Platform::Linux));
  assert_eq!(expect_json(&expect), value);
  let everywhere = json!({"flaky": "The handshake does not finish."});
  assert_eq!(
    expect_json(&parse_expect("example.js", everywhere.clone())),
    everywhere
  );
  let repeated = json!({
    "flaky": {
      "linux": "The handshake does not finish.",
      "macos": "The handshake does not finish.",
      "windows": "The handshake does not finish."
    }
  });
  assert_eq!(
    expect_json(&parse_expect("example.js", repeated)),
    everywhere
  );
}

#[test]
fn expectation_update_keeps_flaky() {
  let previous = parse_expect(
    "example.js",
    json!({
      "flaky": {"macos": "The handshake does not finish."},
      "ignore": {"linux": "No VA-API"}
    }),
  );
  let observed = merge_observation(
    Some(&previous),
    FileResult::Pass,
    false,
    Platform::Windows,
  );
  assert_eq!(
    expect_json(&observed),
    json!({
      "flaky": {"macos": "The handshake does not finish."},
      "ignore": {"linux": "No VA-API"}
    })
  );
}

#[test]
#[should_panic(expected = "name the same platform")]
fn expectation_rejects_ignore_and_flaky_on_one_platform() {
  parse_expect(
    "example.js",
    json!({
      "ignore": {"macos": "Skipped."},
      "flaky": {"macos": "Sometimes hangs."}
    }),
  );
}

#[test]
#[should_panic(expected = "use false for a whole-file failure")]
fn expectation_rejects_fail_without_a_platform() {
  parse_expect("example.js", json!({"fail": true}));
}

#[test]
#[should_panic(
  expected = "an all-platform ignore cannot set expectedFailures or fail"
)]
fn expectation_rejects_failures_on_an_all_platform_ignore() {
  parse_expect(
    "example.js",
    json!({
      "ignore": "Needs a document",
      "expectedFailures": ["name"]
    }),
  );
}

#[test]
fn runner_skip_reasons_match_the_classifier() {
  let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
  let suite = root.join("tests/wpt/suite");
  let expectations = root.join("tests/wpt/expectations");
  for entry in fs::read_dir(&expectations).expect("expectations") {
    let path = entry.expect("entry").path();
    if !path.extension().is_some_and(|ext| ext == "json") {
      continue;
    }
    for (key, expect) in read_expectations(&path, false) {
      let file = suite.join(&key);
      if classify(&file, &key) != Some(Kind::Ignore) {
        continue;
      }
      let Note::All { reason } = &expect.ignore else {
        panic!("{key}: a runner skip is an all-platform ignore");
      };
      assert_eq!(reason, &skip_reason(&file, &key), "{key}");
      assert!(matches!(expect.flaky, Note::No), "{key}");
    }
  }
}

#[test]
fn expectation_files_parse() {
  let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
    .join("../../tests/wpt/expectations");
  let mut count = 0;
  for entry in fs::read_dir(&root).expect("expectations") {
    let path = entry.expect("entry").path();
    if path.extension().is_some_and(|ext| ext == "json") {
      count += read_expectations(&path, false).len();
    }
  }
  assert!(count > 0);
}
