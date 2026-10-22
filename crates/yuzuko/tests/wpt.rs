//! Web-platform-tests.
//!
//! `YUZUKO_WPT_UPDATE=1` rewrites `tests/wpt/expectations`.
//! `YUZUKO_WPT_NO_IGNORE=1` also runs files marked `{"ignore": true}`.
//! `YUZUKO_WPT_FILTER` keeps paths that contain the substring.

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

#[derive(Clone, Debug, PartialEq, Eq)]
enum Expect {
  Pass,
  Fail,
  Ignore,
  Failures(Vec<String>),
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
  let filter = std::env::var("YUZUKO_WPT_FILTER").ok();
  let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
  let suite = workspace.join("tests/wpt/suite");
  let expectations = workspace.join("tests/wpt/expectations");
  fs::create_dir_all(&expectations).expect("expectations directory");
  let harness = fs::read_to_string(suite.join("resources/testharness.js"))
    .expect("testharness.js");
  let _server = WptServer::ensure(&suite);
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
      let ignored = case.kind == Kind::Ignore;
      if ignored && !no_ignore {
        if update {
          saved.insert(case.key.clone(), Expect::Ignore);
        } else if saved.get(&case.key) != Some(&Expect::Ignore) {
          mismatches.push(format!(
            "{}: expected {{\"ignore\": true}} for a file this runner skips",
            case.key
          ));
        }
        continue;
      }
      eprintln!("{}", case.key);
      let outcome = execute(&suite, &harness, case);
      let actual = expect_of(&outcome);
      if update {
        saved.insert(case.key.clone(), actual);
        continue;
      }
      match saved.get(&case.key) {
        Some(expected) if expectation_matches(expected, &outcome) => {}
        Some(expected) => {
          mismatches.push(format!(
            "{}: expected {} but {}",
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

/// The file needs a document-only node, a media element, a `MediaStream`,
/// a permissions policy, or `AudioParam` automation.
fn webaudio_ignored(path: &Path, key: &str, name: &str) -> bool {
  let test_file = name.ends_with(".html")
    || name.ends_with(".xhtml")
    || name.ends_with(".window.js")
    || name.ends_with(".any.js")
    || name.ends_with(".worker.js");
  if !test_file {
    return false;
  }
  const DIRS: &[&str] = &[
    "/the-analysernode-interface/",
    "/the-audioparam-interface/",
    "/the-biquadfilternode-interface/",
    "/the-channelmergernode-interface/",
    "/the-channelsplitternode-interface/",
    "/the-convolvernode-interface/",
    "/the-delaynode-interface/",
    "/the-dynamicscompressornode-interface/",
    "/the-iirfilternode-interface/",
    "/the-mediaelementaudiosourcenode-interface/",
    "/the-mediastreamaudiodestinationnode-interface/",
    "/the-mediastreamaudiosourcenode-interface/",
    "/the-pannernode-interface/",
    "/the-scriptprocessornode-interface/",
    "/the-stereopanner-interface/",
    "/the-waveshapernode-interface/",
    "/media-playback-while-not-visible-permission-policy/",
    "/processing-model/",
  ];
  if DIRS.iter().any(|dir| key.contains(dir)) {
    return true;
  }
  let lower = key.to_ascii_lowercase();
  if lower.contains("sinkid")
    || lower.contains("playbackstats")
    || lower.contains("getoutputtimestamp")
    || key.ends_with("/historical.html")
  {
    return true;
  }
  let Ok(source) = fs::read_to_string(path) else {
    return false;
  };
  const NEEDLES: &[&str] = &[
    "AnalyserNode",
    "AudioListener",
    "BiquadFilterNode",
    "ChannelMergerNode",
    "ChannelSplitterNode",
    "ConvolverNode",
    "DelayNode",
    "DynamicsCompressorNode",
    "IIRFilterNode",
    "MediaElementAudioSourceNode",
    "MediaStreamAudioDestinationNode",
    "MediaStreamAudioSourceNode",
    "PannerNode",
    "ScriptProcessorNode",
    "StereoPannerNode",
    "WaveShaperNode",
    "createAnalyser",
    "createBiquadFilter",
    "createChannelMerger",
    "createChannelSplitter",
    "createConvolver",
    "createDelay",
    "createDynamicsCompressor",
    "createIIRFilter",
    "createMediaElementSource",
    "createMediaStreamDestination",
    "createMediaStreamSource",
    "createPanner",
    "createScriptProcessor",
    "createStereoPanner",
    "createWaveShaper",
  ];
  NEEDLES.iter().any(|needle| source.contains(needle))
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
  const SKIP: &[&str] = &[
    "<iframe", "<frame", "<canvas", "<svg", "<video", "<img", "<input",
    "<table", "<object", "<embed",
  ];
  !SKIP.iter().any(|tag| lower.contains(tag))
}

fn execute(suite: &Path, harness: &str, case: &Case) -> Outcome {
  let path = suite.join(&case.key);
  let source = match fs::read_to_string(&path) {
    Ok(source) => source,
    Err(error) => return Outcome::Error(error.to_string()),
  };
  let queries = variant_queries(&source);
  let mut parts = Vec::with_capacity(queries.len());
  for query in &queries {
    parts.push(execute_query(suite, harness, case, &path, &source, query));
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
  harness: &str,
  case: &Case,
  path: &Path,
  source: &str,
  search: &str,
) -> Outcome {
  let base = network_base(&case.key);
  let mut entry = base.map(|base| page_url(base, &case.key, search));
  let mut scripts = vec![
    GLOBAL_PRELUDE.to_string(),
    location_script(path, search, base, &case.key),
    fetch_script(suite, base),
    harness.to_string(),
    COMPLETION.to_string(),
  ];
  match case.kind {
    Kind::Ignore => {
      return Outcome::Error("ignored file was executed".to_string());
    }
    Kind::Worker => {
      let url = format!("{WPT_HTTP}/{}", case.key);
      entry = Some(url.clone());
      let inline = if case.key.ends_with(".any.js") {
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
        parts.insert(0, harness.to_string());
        parts.push(source.to_string());
        if !source.contains("done()") {
          // Dedicated workers wait for an explicit `done()`. The WPT server
          // appends it when it wraps a worker `.any.js`.
          parts.push("done();\n".to_string());
        }
        Some(parts.join("\n"))
      } else {
        None
      };
      scripts = vec![worker_parent(&url, inline.as_deref())];
    }
    Kind::Script => {
      if let Err(error) =
        push_meta_scripts(suite, &case.key, source, &mut scripts, base)
      {
        return Outcome::Error(error);
      }
      scripts.push(source.to_string());
    }
    Kind::Html => {
      if let Err(error) =
        push_html_scripts(suite, &case.key, source, &mut scripts, base)
      {
        return Outcome::Error(error);
      }
      // Window tests can wait for `load`. There is no document, so dispatch
      // it after the page scripts have registered listeners.
      scripts.push(
        "if (typeof dispatchEvent === 'function') {\n  dispatchEvent(new Event('load'));\n}\n"
          .to_string(),
      );
    }
  }
  let deadline = if timeout_long(source) {
    Duration::from_secs(30)
  } else if matches!(case.kind, Kind::Worker) {
    // Font and fetch work in a worker can still be in flight when the
    // owner marks the harness done. Give that a little room.
    Duration::from_secs(15)
  } else {
    Duration::from_secs(8)
  };
  match yuzuko_core::run_classic_scripts_at(
    &scripts,
    DONE_KEY,
    deadline,
    Some(path),
    Some(suite),
    entry.as_deref(),
  ) {
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
fn worker_parent(url: &str, inline: Option<&str>) -> String {
  let start = if let Some(source) = inline {
    let source =
      serde_json::to_string(source).unwrap_or_else(|_| "\"\"".to_string());
    format!(
      "var source = {source};\nvar worker = new Worker(\"data:text/javascript;charset=utf-8,\" + encodeURIComponent(source));"
    )
  } else {
    let url = serde_json::to_string(url).unwrap_or_else(|_| "\"\"".to_string());
    format!("var worker = new Worker({url});")
  };
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

fn expect_of(outcome: &Outcome) -> Expect {
  match outcome {
    Outcome::Error(_) => Expect::Fail,
    Outcome::Report { status, tests, .. } => {
      if *status != 0 && *status != 3 {
        return Expect::Fail;
      }
      let mut failed: Vec<String> = tests
        .iter()
        .filter(|test| test.status != 0 && test.status != 4)
        .map(|test| test.name.clone())
        .collect();
      failed.sort();
      failed.dedup();
      if failed.is_empty() {
        Expect::Pass
      } else {
        Expect::Failures(failed)
      }
    }
  }
}

fn expectation_matches(expect: &Expect, outcome: &Outcome) -> bool {
  match expect {
    Expect::Ignore => false,
    Expect::Pass => expect_of(outcome) == Expect::Pass,
    Expect::Fail => expect_of(outcome) != Expect::Pass,
    Expect::Failures(expected) => match expect_of(outcome) {
      Expect::Failures(actual) => expected == &actual,
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
  match expect {
    Expect::Pass => "pass".to_string(),
    Expect::Fail => "file failure".to_string(),
    Expect::Ignore => "ignore".to_string(),
    Expect::Failures(names) => format!("failures {names:?}"),
  }
}

fn read_expectations(
  path: &Path,
  update: bool,
) -> std::collections::BTreeMap<String, Expect> {
  if !path.exists() {
    if update {
      return std::collections::BTreeMap::new();
    }
    return std::collections::BTreeMap::new();
  }
  let text = fs::read_to_string(path).unwrap_or_else(|error| {
    panic!("read {}: {error}", path.display());
  });
  let Ok(Value::Object(map)) = serde_json::from_str(&text) else {
    panic!("{} is not a JSON object", path.display());
  };
  let mut expectations = std::collections::BTreeMap::new();
  for (key, value) in map {
    expectations.insert(key, parse_expect(value));
  }
  expectations
}

fn parse_expect(value: Value) -> Expect {
  match value {
    Value::Bool(true) => Expect::Pass,
    Value::Bool(false) => Expect::Fail,
    Value::Object(map) => {
      if map.get("ignore").and_then(Value::as_bool) == Some(true) {
        return Expect::Ignore;
      }
      if let Some(list) = map.get("expectedFailures").and_then(Value::as_array)
      {
        let mut names = list
          .iter()
          .filter_map(Value::as_str)
          .map(str::to_string)
          .collect::<Vec<_>>();
        names.sort();
        names.dedup();
        return Expect::Failures(names);
      }
      panic!("unsupported expectation {map:?}");
    }
    other => panic!("unsupported expectation {other}"),
  }
}

fn write_expectations(
  path: &Path,
  expectations: &std::collections::BTreeMap<String, Expect>,
) {
  let mut map = serde_json::Map::new();
  for (key, expect) in expectations {
    map.insert(key.clone(), expect_json(expect));
  }
  let text = serde_json::to_string_pretty(&Value::Object(map)).expect("json");
  fs::write(path, format!("{text}\n")).expect("write expectations");
}

fn expect_json(expect: &Expect) -> Value {
  match expect {
    Expect::Pass => Value::Bool(true),
    Expect::Fail => Value::Bool(false),
    Expect::Ignore => json!({"ignore": true}),
    Expect::Failures(names) => json!({"expectedFailures": names}),
  }
}
