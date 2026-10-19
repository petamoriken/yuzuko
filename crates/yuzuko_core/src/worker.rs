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

fn inbox_pending(agent: u64) -> bool {
  let agents = agents();
  let Some(slot) = agents.as_ref().and_then(|map| map.get(&agent)) else {
    return false;
  };
  !slot.inbox.lock().expect("worker inbox").is_empty()
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
  let parsed = Url::parse(url).map_err(|error| error.to_string())?;
  if parsed.scheme() == "data" {
    let text = decode_data_url(url)?;
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

fn decode_data_url(url: &str) -> Result<String, String> {
  let rest = url
    .strip_prefix("data:")
    .ok_or_else(|| "The data URL is invalid".to_string())?;
  let (meta, data) = rest
    .split_once(',')
    .ok_or_else(|| "The data URL has no payload".to_string())?;
  if meta.contains(";base64") {
    return Err("Base64 data URLs are not supported for workers".to_string());
  }
  percent_decode(data)
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
