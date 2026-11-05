//! Dedicated worker agents.
//!
//! Each worker has its own thread, isolate, and task queue. The main realm
//! does not take the process runtime lock on behalf of that thread, so the
//! worker keeps running while the owner is busy.

use std::collections::{HashMap, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use url::Url;

use crate::host;
use crate::parcel::Parcel;
use crate::platform::TaskQueue;

pub const MAIN_AGENT: u64 = 0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScriptKind {
  Classic,
  Module,
}

pub struct WorkerScript {
  pub url: String,
  pub kind: ScriptKind,
  pub name: String,
  pub sandbox: Option<PathBuf>,
}

pub enum Inbound {
  Script {
    worker: u64,
    parcel: Parcel,
  },
  Port {
    port: u64,
    parcel: Parcel,
  },
  Failed {
    worker: u64,
    message: String,
    filename: String,
    line: u32,
    column: u32,
  },
}

struct Agent {
  queue: Arc<TaskQueue>,
  parent: u64,
  inbox: Mutex<VecDeque<Inbound>>,
  children: Mutex<Vec<u64>>,
  alive: AtomicBool,
  /// `close()` was called. The running script finishes, then this agent
  /// stops. It stays alive until that return so the owner still receives
  /// a `postMessage` from the script.
  closing: AtomicBool,
  module: AtomicBool,
  name: Mutex<String>,
  thread: Mutex<Option<JoinHandle<()>>>,
  isolate: Mutex<Option<v8::IsolateHandle>>,
}

static AGENTS: Mutex<Option<HashMap<u64, Arc<Agent>>>> = Mutex::new(None);
static NEXT_AGENT: AtomicU64 = AtomicU64::new(1);

type InboundHandler = fn(&mut v8::PinScope);
static HANDLER: Mutex<Option<InboundHandler>> = Mutex::new(None);

fn agents() -> std::sync::MutexGuard<'static, Option<HashMap<u64, Arc<Agent>>>>
{
  AGENTS.lock().expect("worker agents")
}

pub fn set_inbound_handler(handler: InboundHandler) {
  *HANDLER.lock().expect("worker handler") = Some(handler);
}

pub fn attach_main(queue: Arc<TaskQueue>) {
  let agent = Arc::new(Agent {
    queue,
    parent: MAIN_AGENT,
    inbox: Mutex::new(VecDeque::new()),
    children: Mutex::new(Vec::new()),
    alive: AtomicBool::new(true),
    closing: AtomicBool::new(false),
    module: AtomicBool::new(false),
    name: Mutex::new(String::new()),
    thread: Mutex::new(None),
    isolate: Mutex::new(None),
  });
  let mut agents = agents();
  let map = agents.get_or_insert_with(HashMap::new);
  map.insert(MAIN_AGENT, agent);
}

pub fn detach_main() {
  let ids: Vec<u64> = {
    let agents = agents();
    let Some(map) = agents.as_ref() else {
      return;
    };
    map.keys().copied().filter(|id| *id != MAIN_AGENT).collect()
  };
  for id in &ids {
    terminate_worker(*id);
  }
  for id in &ids {
    join_worker(*id);
  }
  let mut agents = agents();
  if let Some(map) = agents.as_mut() {
    map.clear();
  }
}

pub fn spawn_worker(parent: u64, script: WorkerScript) -> Result<u64, String> {
  let id = NEXT_AGENT.fetch_add(1, Ordering::Relaxed);
  let queue = TaskQueue::new();
  let agent = Arc::new(Agent {
    queue: Arc::clone(&queue),
    parent,
    inbox: Mutex::new(VecDeque::new()),
    children: Mutex::new(Vec::new()),
    alive: AtomicBool::new(true),
    closing: AtomicBool::new(false),
    module: AtomicBool::new(script.kind == ScriptKind::Module),
    name: Mutex::new(script.name.clone()),
    thread: Mutex::new(None),
    isolate: Mutex::new(None),
  });
  {
    let mut agents = agents();
    let map = agents.get_or_insert_with(HashMap::new);
    map.insert(id, Arc::clone(&agent));
    if let Some(parent_agent) = map.get(&parent) {
      parent_agent
        .children
        .lock()
        .expect("worker children")
        .push(id);
    }
  }
  let thread = std::thread::Builder::new()
    .name(format!("yuzuko-worker-{id}"))
    .spawn(move || {
      crate::runtime::run_worker_isolate(id, queue, script);
    })
    .map_err(|error| error.to_string())?;
  agent.thread.lock().expect("worker thread").replace(thread);
  Ok(id)
}

/// Stops every agent, including the main realm, without marking the main
/// realm dead. The caller has already chosen the process status.
pub(crate) fn interrupt_agents() {
  let agents = {
    let guard = agents();
    guard
      .as_ref()
      .map(|map| map.values().cloned().collect::<Vec<_>>())
      .unwrap_or_default()
  };
  for agent in agents {
    let handle = agent.isolate.lock().expect("worker isolate").clone();
    if let Some(handle) = handle {
      handle.terminate_execution();
    }
    agent.queue.poke();
  }
}

pub fn terminate_worker(id: u64) {
  let agent = {
    let agents = agents();
    agents.as_ref().and_then(|map| map.get(&id).cloned())
  };
  let Some(agent) = agent else {
    return;
  };
  if !agent.alive.swap(false, Ordering::SeqCst) {
    return;
  }
  let children = agent.children.lock().expect("worker children").clone();
  for child in children {
    terminate_worker(child);
  }
  agent.inbox.lock().expect("worker inbox").clear();
  if let Some(isolate) = agent.isolate.lock().expect("worker isolate").clone() {
    isolate.terminate_execution();
  }
  agent.queue.poke();
}

/// `DedicatedWorkerGlobalScope.close`.
///
/// The running script finishes. Further tasks on this worker do not run.
/// Unlike [`terminate_worker`], this does not abort the isolate, and the
/// agent stays alive until that script returns.
pub fn close_worker(id: u64) {
  let agent = {
    let agents = agents();
    agents.as_ref().and_then(|map| map.get(&id).cloned())
  };
  let Some(agent) = agent else {
    return;
  };
  if !agent.alive.load(Ordering::SeqCst) {
    return;
  }
  if agent.closing.swap(true, Ordering::SeqCst) {
    return;
  }
  let children = agent.children.lock().expect("worker children").clone();
  for child in children {
    terminate_worker(child);
  }
  agent.inbox.lock().expect("worker inbox").clear();
}

pub fn worker_closing(id: u64) -> bool {
  let agents = agents();
  agents
    .as_ref()
    .and_then(|map| map.get(&id))
    .is_some_and(|agent| agent.closing.load(Ordering::SeqCst))
}

pub(crate) fn set_isolate_handle(id: u64, handle: v8::IsolateHandle) {
  let agent = {
    let agents = agents();
    agents.as_ref().and_then(|map| map.get(&id).cloned())
  };
  let Some(agent) = agent else {
    return;
  };
  *agent.isolate.lock().expect("worker isolate") = Some(handle);
}

fn join_worker(id: u64) {
  let handle = {
    let agents = agents();
    let Some(agent) = agents.as_ref().and_then(|map| map.get(&id)) else {
      return;
    };
    agent.thread.lock().expect("worker thread").take()
  };
  if let Some(handle) = handle {
    let _ = handle.join();
  }
}

static FRAME_HOOK: Mutex<Option<fn(&mut v8::PinScope)>> = Mutex::new(None);

pub fn set_worker_frame_hook(hook: fn(&mut v8::PinScope)) {
  *FRAME_HOOK.lock().expect("worker frame hook") = Some(hook);
}

pub fn run_worker_frame_hook(scope: &mut v8::PinScope) {
  if let Some(hook) = *FRAME_HOOK.lock().expect("worker frame hook") {
    hook(scope);
  }
}

pub fn poke_agent(id: u64) {
  let slot = {
    let agents = agents();
    agents.as_ref().and_then(|map| map.get(&id).cloned())
  };
  if let Some(slot) = slot {
    slot.queue.poke();
  }
}

pub fn post_inbound(agent: u64, inbound: Inbound) {
  let slot = {
    let agents = agents();
    agents.as_ref().and_then(|map| map.get(&agent).cloned())
  };
  let Some(slot) = slot else {
    return;
  };
  let deliver_failure = matches!(inbound, Inbound::Failed { .. });
  if !slot.alive.load(Ordering::SeqCst) && !deliver_failure {
    return;
  }
  slot.inbox.lock().expect("worker inbox").push_back(inbound);
  slot.queue.poke();
}

pub fn take_inbox(agent: u64) -> Vec<Inbound> {
  let slot = {
    let agents = agents();
    agents.as_ref().and_then(|map| map.get(&agent).cloned())
  };
  let Some(slot) = slot else {
    return Vec::new();
  };
  slot.inbox.lock().expect("worker inbox").drain(..).collect()
}

pub(crate) fn inbox_pending(agent: u64) -> bool {
  let agents = agents();
  let Some(slot) = agents.as_ref().and_then(|map| map.get(&agent)) else {
    return false;
  };
  !slot.inbox.lock().expect("worker inbox").is_empty()
}

/// This realm has a worker message waiting and a handler that can deliver it.
pub(crate) fn inbound_ready() -> bool {
  has_inbound_handler() && inbox_pending(host::agent_id())
}

pub fn agent_alive(id: u64) -> bool {
  let agents = agents();
  agents
    .as_ref()
    .and_then(|map| map.get(&id))
    .is_some_and(|agent| agent.alive.load(Ordering::SeqCst))
}

pub fn keeps_alive(agent: u64) -> bool {
  let children = {
    let agents = agents();
    let Some(slot) = agents.as_ref().and_then(|map| map.get(&agent)) else {
      return false;
    };
    slot.children.lock().expect("worker children").clone()
  };
  children.into_iter().any(agent_alive)
}

pub fn parent_agent(id: u64) -> Option<u64> {
  let agents = agents();
  agents
    .as_ref()
    .and_then(|map| map.get(&id))
    .map(|agent| agent.parent)
}

pub fn worker_name() -> String {
  let id = host::agent_id();
  let agents = agents();
  agents
    .as_ref()
    .and_then(|map| map.get(&id))
    .map(|agent| agent.name.lock().expect("worker name").clone())
    .unwrap_or_default()
}

pub struct LocationParts {
  pub href: String,
  pub origin: String,
  pub protocol: String,
  pub host: String,
  pub hostname: String,
  pub port: String,
  pub pathname: String,
  pub search: String,
  pub hash: String,
}

pub fn worker_location() -> LocationParts {
  let href = host::entry_url().unwrap_or_default();
  let Ok(url) = Url::parse(&href) else {
    return LocationParts {
      href: href.clone(),
      origin: "null".to_string(),
      protocol: String::new(),
      host: String::new(),
      hostname: String::new(),
      port: String::new(),
      pathname: String::new(),
      search: String::new(),
      hash: String::new(),
    };
  };
  let origin = if url.scheme() == "file" {
    "null".to_string()
  } else {
    url.origin().ascii_serialization()
  };
  LocationParts {
    href: url.to_string(),
    origin,
    protocol: format!("{}:", url.scheme()),
    host: url.host_str().unwrap_or("").to_string()
      + url
        .port()
        .map(|port| format!(":{port}"))
        .as_deref()
        .unwrap_or(""),
    hostname: url.host_str().unwrap_or("").to_string(),
    port: url.port().map(|port| port.to_string()).unwrap_or_default(),
    pathname: url.path().to_string(),
    search: url
      .query()
      .map(|query| format!("?{query}"))
      .unwrap_or_default(),
    hash: url
      .fragment()
      .map(|fragment| format!("#{fragment}"))
      .unwrap_or_default(),
  }
}

pub fn worker_is_module() -> bool {
  let id = host::agent_id();
  let agents = agents();
  agents
    .as_ref()
    .and_then(|map| map.get(&id))
    .is_some_and(|agent| agent.module.load(Ordering::SeqCst))
}

pub(crate) fn has_inbound_handler() -> bool {
  HANDLER.lock().expect("worker handler").is_some()
}

pub fn drain_inbound(scope: &mut v8::PinScope) -> bool {
  let handler = *HANDLER.lock().expect("worker handler");
  let Some(handler) = handler else {
    return false;
  };
  let id = host::with(|state| state.agent_id);
  if !inbox_pending(id) {
    return false;
  }
  handler(scope);
  true
}

pub fn resolve_specifier(spec: &str) -> Result<String, String> {
  if spec.is_empty() {
    return Err("The worker URL is empty".to_string());
  }
  let base = host::entry_url()
    .ok_or_else(|| "The worker URL has no base".to_string())?;
  let base = Url::parse(&base).map_err(|error| error.to_string())?;
  let url = base.join(spec).map_err(|error| error.to_string())?;
  Ok(url.to_string())
}

pub struct LoadedSource {
  pub path: Option<PathBuf>,
  pub text: String,
}

pub fn eval_classic_in_worker(
  scope: &mut v8::PinScope,
  url: &str,
  source: &str,
) -> Result<(), String> {
  crate::runtime::eval_classic(scope, url, source)
    .map_err(|error| error.message)
}

pub fn load_worker_source(
  url: &str,
  sandbox: Option<&Path>,
) -> Result<LoadedSource, String> {
  if let Some(text) = classic_worker_source(url) {
    return Ok(LoadedSource { path: None, text });
  }
  let parsed = Url::parse(url).map_err(|error| error.to_string())?;
  if parsed.scheme() == "data" {
    let text = decode_data_url(url)?;
    return Ok(LoadedSource { path: None, text });
  }
  if parsed.scheme() == "blob" {
    let text = load_blob_script(url)?;
    return Ok(LoadedSource { path: None, text });
  }
  if parsed.scheme() == "file" {
    let path = parsed
      .to_file_path()
      .map_err(|()| format!("The worker URL is not a file path: {url}"))?;
    let text = fs::read_to_string(&path).map_err(|error| error.to_string())?;
    return Ok(LoadedSource {
      path: Some(path),
      text,
    });
  }
  if let Some(sandbox) = sandbox
    && let Some(path) = sandbox_file(&parsed, sandbox)
    && path.is_file()
  {
    let text = fs::read_to_string(&path).map_err(|error| error.to_string())?;
    return Ok(LoadedSource {
      path: Some(path),
      text,
    });
  }
  Err(format!("The worker script could not be loaded: {url}"))
}

type BlobScriptLoader = fn(&str) -> Result<String, String>;

static BLOB_SCRIPT: Mutex<Option<BlobScriptLoader>> = Mutex::new(None);

static CLASSIC_WORKER_SOURCE: Mutex<Option<(String, String)>> =
  Mutex::new(None);

/// Serves one classic worker URL from memory.
///
/// The WPT runner uses this so a worker `.any.js` keeps the test's `http:`
/// URL. A `data:` script URL cannot resolve a path-absolute `importScripts`.
/// `None` clears the override.
pub fn set_classic_worker_source(source: Option<(String, String)>) {
  *CLASSIC_WORKER_SOURCE.lock().expect("classic worker source") = source;
}

fn classic_worker_source(url: &str) -> Option<String> {
  let guard = CLASSIC_WORKER_SOURCE.lock().expect("classic worker source");
  let (expected, text) = guard.as_ref()?;
  if same_url(expected, url) {
    Some(text.clone())
  } else {
    None
  }
}

fn same_url(left: &str, right: &str) -> bool {
  if left == right {
    return true;
  }
  match (Url::parse(left), Url::parse(right)) {
    (Ok(left), Ok(right)) => left == right,
    _ => false,
  }
}

/// Installed by the blob URL table so a worker thread can read a URL
/// created on another thread.
pub fn set_blob_script_loader(loader: BlobScriptLoader) {
  *BLOB_SCRIPT.lock().expect("blob script loader") = Some(loader);
}

fn load_blob_script(url: &str) -> Result<String, String> {
  let loader = *BLOB_SCRIPT.lock().expect("blob script loader");
  let Some(loader) = loader else {
    return Err(format!("The worker script could not be loaded: {url}"));
  };
  loader(url)
}

fn decode_data_url(url: &str) -> Result<String, String> {
  let rest = url
    .strip_prefix("data:")
    .ok_or_else(|| "The data URL is invalid".to_string())?;
  let (meta, data) = rest
    .split_once(',')
    .ok_or_else(|| "The data URL has no payload".to_string())?;
  let decoded = percent_decode(data)?;
  if !meta_is_base64(meta) {
    return Ok(decoded);
  }
  // HTML data URLs percent-decode, then forgiving-base64-decode.
  let bytes = forgiving_base64(&decoded)?;
  String::from_utf8(bytes).map_err(|error| error.to_string())
}

fn meta_is_base64(meta: &str) -> bool {
  meta
    .rsplit(';')
    .next()
    .is_some_and(|part| part.trim().eq_ignore_ascii_case("base64"))
}

/// Infra forgiving-base64 decode. ASCII whitespace is ignored.
fn forgiving_base64(input: &str) -> Result<Vec<u8>, String> {
  let mut filtered: String = input
    .chars()
    .filter(|ch| !matches!(ch, ' ' | '\t' | '\n' | '\u{000c}' | '\r'))
    .collect();
  if filtered.len().is_multiple_of(4) {
    if filtered.ends_with("==") {
      filtered.truncate(filtered.len() - 2);
    } else if filtered.ends_with('=') {
      filtered.truncate(filtered.len() - 1);
    }
  }
  if filtered.is_empty() {
    return Ok(Vec::new());
  }
  if filtered.len() % 4 == 1 {
    return Err("The data URL is invalid".to_string());
  }
  let mut out = Vec::new();
  let mut buffer: u32 = 0;
  let mut bits = 0u32;
  for byte in filtered.bytes() {
    let value = u32::from(base64_digit(byte)?);
    buffer = (buffer << 6) | value;
    bits += 6;
    if bits >= 8 {
      bits -= 8;
      out.push((buffer >> bits) as u8);
      if bits == 0 {
        buffer = 0;
      } else {
        buffer &= (1 << bits) - 1;
      }
    }
  }
  Ok(out)
}

fn base64_digit(byte: u8) -> Result<u8, String> {
  match byte {
    b'A'..=b'Z' => Ok(byte - b'A'),
    b'a'..=b'z' => Ok(byte - b'a' + 26),
    b'0'..=b'9' => Ok(byte - b'0' + 52),
    b'+' => Ok(62),
    b'/' => Ok(63),
    _ => Err("The data URL is invalid".to_string()),
  }
}

fn percent_decode(input: &str) -> Result<String, String> {
  let bytes = input.as_bytes();
  let mut out = Vec::with_capacity(bytes.len());
  let mut index = 0;
  while index < bytes.len() {
    if bytes[index] == b'%' && index + 2 < bytes.len() {
      let hex = std::str::from_utf8(&bytes[index + 1..index + 3])
        .map_err(|error| error.to_string())?;
      let value =
        u8::from_str_radix(hex, 16).map_err(|error| error.to_string())?;
      out.push(value);
      index += 3;
    } else {
      out.push(bytes[index]);
      index += 1;
    }
  }
  String::from_utf8(out).map_err(|error| error.to_string())
}

fn sandbox_file(url: &Url, sandbox: &Path) -> Option<PathBuf> {
  let host = url.host_str()?;
  if host != "web-platform.test" && host != "localhost" && host != "127.0.0.1" {
    return None;
  }
  let relative = url.path().trim_start_matches('/');
  if relative.is_empty() {
    return None;
  }
  Some(sandbox.join(relative))
}

pub(crate) fn mark_dead(id: u64) {
  if let Some(agent) = agents().as_ref().and_then(|map| map.get(&id).cloned()) {
    agent.alive.store(false, Ordering::SeqCst);
  }
}

#[cfg(test)]
mod tests {
  use super::{decode_data_url, load_worker_source, set_classic_worker_source};
  use url::Url;

  #[test]
  fn base64_data_url_decodes_a_script() {
    let text = decode_data_url("data:text/javascript;base64,aGk=").unwrap();
    assert_eq!(text, "hi");
    let spaced = decode_data_url("data:text/javascript;base64,aG\nk=").unwrap();
    assert_eq!(spaced, "hi");
    let charset =
      decode_data_url("data:text/javascript;charset=utf-8;base64,aGk=")
        .unwrap();
    assert_eq!(charset, "hi");
  }

  #[test]
  fn plain_data_url_percent_decodes() {
    let text = decode_data_url("data:text/javascript,hi%21").unwrap();
    assert_eq!(text, "hi!");
  }

  #[test]
  fn invalid_base64_data_url_fails() {
    assert!(decode_data_url("data:;base64,****").is_err());
  }

  #[test]
  fn classic_worker_source_replaces_one_script_body() {
    struct Clear;
    impl Drop for Clear {
      fn drop(&mut self) {
        set_classic_worker_source(None);
      }
    }
    let _clear = Clear;
    let url = "http://web-platform.test:8000/workers/importscripts_mime.any.js";
    set_classic_worker_source(Some((url.to_string(), "done();".to_string())));
    let loaded = load_worker_source(url, None).unwrap();
    assert_eq!(loaded.text, "done();");
    assert!(loaded.path.is_none());
    assert!(
      load_worker_source("http://web-platform.test:8000/other.js", None)
        .is_err()
    );
    set_classic_worker_source(None);
    assert!(load_worker_source(url, None).is_err());
  }

  #[test]
  fn blob_url_origin_keeps_the_creator_origin() {
    let url = Url::parse(
      "blob:http://web-platform.test:8000/11111111-1111-4111-8111-111111111111",
    )
    .unwrap();
    assert_eq!(
      url.origin().ascii_serialization(),
      "http://web-platform.test:8000"
    );
  }
}
