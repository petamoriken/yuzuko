//! Dedicated worker agents.
//!
//! Each worker has its own thread, isolate, and task queue. The main realm
//! does not take the process runtime lock on behalf of that thread, so the
//! worker keeps running while the owner is busy.

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
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
    /// `MessagePort` ids from the caller's transfer list, in list order.
    ports: Vec<u64>,
  },
  Port {
    port: u64,
    parcel: Parcel,
    /// Ports in the sender's transfer list, in that order.
    ports: Vec<u64>,
  },
  Failed {
    worker: u64,
    message: String,
    filename: String,
    line: u32,
    column: u32,
    /// The worker script failed to parse. The owner gets a plain `error` event.
    parse: bool,
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
static WORKER_OWNERS: LazyLock<Mutex<HashMap<u64, String>>> =
  LazyLock::new(|| Mutex::new(HashMap::new()));

/// Records the module URL that constructed this dedicated worker.
pub fn set_worker_owner(id: u64, owner: String) {
  WORKER_OWNERS
    .lock()
    .expect("worker owners")
    .insert(id, owner);
}

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

/// Stops dedicated workers and keeps the main agent.
///
/// [`detach_main`] also drops the main agent. A hot reload still needs it.
pub(crate) fn terminate_child_workers() {
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
    map.retain(|id, _| *id == MAIN_AGENT);
    if let Some(main) = map.get(&MAIN_AGENT) {
      main.children.lock().expect("worker children").clear();
    }
  }
  WORKER_OWNERS.lock().expect("worker owners").clear();
}

/// Stops dedicated workers created by a module this reload evaluates again.
pub(crate) fn terminate_reloaded_workers() {
  let owned = {
    let owners = WORKER_OWNERS.lock().expect("worker owners");
    owners
      .iter()
      .filter(|(_, owner)| host::module_reloaded(owner))
      .map(|(id, _)| *id)
      .collect::<Vec<_>>()
  };
  let mut drop_ids = Vec::new();
  for id in owned {
    for child in worker_tree(id) {
      if !drop_ids.contains(&child) {
        drop_ids.push(child);
      }
    }
  }
  for id in &drop_ids {
    terminate_worker(*id);
  }
  for id in &drop_ids {
    join_worker(*id);
  }
  {
    let mut agents = agents();
    if let Some(map) = agents.as_mut() {
      for id in &drop_ids {
        map.remove(id);
      }
      for agent in map.values() {
        agent
          .children
          .lock()
          .expect("worker children")
          .retain(|child| !drop_ids.contains(child));
      }
    }
  }
  WORKER_OWNERS
    .lock()
    .expect("worker owners")
    .retain(|id, _| !drop_ids.contains(id));
}

fn worker_tree(id: u64) -> Vec<u64> {
  let children = {
    let agents = agents();
    agents
      .as_ref()
      .and_then(|map| map.get(&id))
      .map(|agent| agent.children.lock().expect("worker children").clone())
      .unwrap_or_default()
  };
  let mut ids = vec![id];
  for child in children {
    ids.extend(worker_tree(child));
  }
  ids
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
  let Some(agent) = agent_by_id(id) else {
    return;
  };
  let parent = agent_by_id(agent.parent);
  let children = agent.children.lock().expect("worker children").clone();
  for child in children {
    terminate_worker(child);
  }
  {
    // Same lock `post_inbound` holds across the sender's alive check and the
    // parent push. A post either lands before the retain or sees `alive` false.
    let mut inbox = agent.inbox.lock().expect("worker inbox");
    if !agent.alive.swap(false, Ordering::SeqCst) {
      return;
    }
    inbox.clear();
    if agent.parent != id
      && let Some(parent) = parent.as_ref()
    {
      parent
        .inbox
        .lock()
        .expect("worker inbox")
        .retain(|item| match item {
          Inbound::Script { worker: from, .. } => *from != id,
          _ => true,
        });
    }
  }
  if let Some(isolate) = agent.isolate.lock().expect("worker isolate").clone() {
    isolate.terminate_execution();
  }
  agent.queue.poke();
}

fn agent_by_id(id: u64) -> Option<Arc<Agent>> {
  let agents = agents();
  agents.as_ref().and_then(|map| map.get(&id).cloned())
}

/// `DedicatedWorkerGlobalScope.close`.
///
/// The running script finishes. Tasks already queued, and later tasks, do
/// not run. A `postMessage` in the script that called `close()` still
/// reaches the owner. Unlike [`terminate_worker`], this does not abort the
/// isolate, and the agent stays alive until that script returns.
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
  // Outbound `postMessage` names the sender in `Script.worker` and the parent
  // as the target. Hold the sender lock, the same one `terminate_worker`
  // holds while it clears that queue, so the post cannot land after the drop.
  if let Inbound::Script { worker, .. } = &inbound
    && *worker != agent
    && let Some(sender) = agent_by_id(*worker)
  {
    let _sender = sender.inbox.lock().expect("worker inbox");
    if !sender.alive.load(Ordering::SeqCst) {
      return;
    }
    enqueue_inbound(agent, inbound);
    return;
  }
  enqueue_inbound(agent, inbound);
}

fn enqueue_inbound(agent: u64, inbound: Inbound) {
  let Some(slot) = agent_by_id(agent) else {
    return;
  };
  let deliver_failure = matches!(inbound, Inbound::Failed { .. });
  {
    // The alive check shares the inbox lock with `terminate_worker`'s clear,
    // so a post that lost the race is not queued after the inbox was emptied.
    let mut inbox = slot.inbox.lock().expect("worker inbox");
    if !slot.alive.load(Ordering::SeqCst) && !deliver_failure {
      return;
    }
    inbox.push_back(inbound);
  }
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

/// Why a worker or `importScripts` URL was rejected.
pub enum WorkerUrlError {
  /// `TypeError`. The entry URL is missing.
  Type(String),
  /// `DOMException` `SyntaxError`. The URL did not parse.
  Syntax(String),
}

impl WorkerUrlError {
  pub fn into_op(self) -> crate::op::OpError {
    match self {
      Self::Type(message) => crate::op::OpError::type_(message),
      Self::Syntax(message) => crate::op::OpError::dom("SyntaxError", message),
    }
  }
}

pub fn resolve_specifier(spec: &str) -> Result<String, WorkerUrlError> {
  let base = host::entry_url().ok_or_else(|| {
    WorkerUrlError::Type("The worker URL has no base".to_string())
  })?;
  let base = Url::parse(&base)
    .map_err(|error| WorkerUrlError::Syntax(error.to_string()))?;
  let url = base
    .join(spec)
    .map_err(|error| WorkerUrlError::Syntax(error.to_string()))?;
  Ok(url.to_string())
}

pub struct LoadedSource {
  pub path: Option<PathBuf>,
  pub text: String,
}

#[derive(Debug)]
pub enum WorkerLoadError {
  Message(String),
  Denied { name: String, message: String },
}

impl From<String> for WorkerLoadError {
  fn from(message: String) -> Self {
    Self::Message(message)
  }
}

/// Why `importScripts` stopped on one script.
pub enum ImportScriptError {
  /// The script did not parse. `line` and `column` are zero-based when known.
  Parse {
    message: String,
    line: u32,
    column: u32,
  },
  /// The script threw. The caller rethrows `error` or reports it.
  Thrown {
    error: v8::Global<v8::Value>,
    line: u32,
    column: u32,
  },
  /// `Yuzuko.exit` ran inside the script.
  Exit,
  /// `Worker.terminate` aborted the script. The caller must not report it.
  Terminated,
}

/// Where an `importScripts` failure should be reported.
pub struct ScriptSite {
  pub filename: String,
  pub line: u32,
  pub column: u32,
}

thread_local! {
  static SCRIPT_SITE: RefCell<Option<ScriptSite>> = const { RefCell::new(None) };
}

pub fn note_script_site(filename: impl Into<String>, line: u32, column: u32) {
  SCRIPT_SITE.with(|slot| {
    *slot.borrow_mut() = Some(ScriptSite {
      filename: filename.into(),
      line,
      column,
    });
  });
}

pub fn has_script_site() -> bool {
  SCRIPT_SITE.with(|slot| slot.borrow().is_some())
}

pub fn take_script_site() -> Option<ScriptSite> {
  SCRIPT_SITE.with(|slot| slot.borrow_mut().take())
}

pub fn clear_script_site() {
  SCRIPT_SITE.with(|slot| *slot.borrow_mut() = None);
}

pub fn eval_classic_in_worker(
  scope: &mut v8::PinScope,
  url: &str,
  source: &str,
) -> Result<(), ImportScriptError> {
  crate::runtime::eval_classic_import(scope, url, source)
}

pub fn load_worker_source(
  url: &str,
  sandbox: Option<&Path>,
) -> Result<LoadedSource, WorkerLoadError> {
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
    if let Err(error) = crate::permissions::check_read(&path) {
      return Err(worker_permission(error));
    }
    let text = read_granted_script(&path)?;
    return Ok(LoadedSource {
      path: Some(path),
      text,
    });
  }
  // `127.0.0.1`, `localhost`, and `web-platform.test` map onto the entry
  // directory for WPT. The read is the same check as a `file:` worker,
  // including the canonical target, so a symlink cannot escape the grant.
  if let Some(sandbox) = sandbox
    && let Some(path) = sandbox_file(&parsed, sandbox)
  {
    if let Err(error) = crate::permissions::check_read(&path) {
      return Err(worker_permission(error));
    }
    if path.is_file() {
      let text = read_granted_script(&path)?;
      return Ok(LoadedSource {
        path: Some(path),
        text,
      });
    }
  }
  Err(format!("The worker script could not be loaded: {url}").into())
}

fn worker_permission(error: crate::op::OpError) -> WorkerLoadError {
  match error {
    crate::op::OpError::Dom { name, message } => {
      WorkerLoadError::Denied { name, message }
    }
    crate::op::OpError::Type(message) | crate::op::OpError::Range(message) => {
      WorkerLoadError::Message(message)
    }
    crate::op::OpError::Thrown(_) => WorkerLoadError::Message(
      "The worker script could not be loaded.".to_string(),
    ),
  }
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
  // Fetch's data: URL processor serializes the URL without its fragment.
  // A serialized URL starts its fragment at the first `#`.
  let url = url.split_once('#').map_or(url, |(before, _)| before);
  let rest = url
    .strip_prefix("data:")
    .ok_or_else(|| "The data URL is invalid".to_string())?;
  let (meta, data) = rest
    .split_once(',')
    .ok_or_else(|| "The data URL has no payload".to_string())?;
  let decoded = crate::percent::decode(data);
  if !meta_is_base64(meta) {
    return Ok(String::from_utf8_lossy(&decoded).into_owned());
  }
  // Fetch data URLs percent-decode, then forgiving-base64-decode the
  // isomorphic decoding. A non-ASCII byte is not base64 either way.
  let decoded: String = decoded.iter().copied().map(char::from).collect();
  let bytes = crate::base64::decode_forgiving(&decoded)
    .map_err(|_| "The data URL is invalid".to_string())?;
  Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// Reads a script whose path passed the read check. Classic scripts use
/// UTF-8 decode with U+FFFD for an invalid byte.
///
/// The file is opened once, and the file the descriptor names is checked
/// again, so a symlink swapped after the check cannot supply another file.
fn read_granted_script(path: &Path) -> Result<String, WorkerLoadError> {
  use std::io::Read;
  let mut file = fs::File::open(path).map_err(|error| error.to_string())?;
  if !crate::permissions::permissions_are_open() {
    let denied =
      || worker_permission(crate::permissions::path_not_allowed(false, path));
    let real = crate::fd_path::fd_path(&file).map_err(|_| denied())?;
    match crate::permissions::path_is_granted(false, &real) {
      Ok(true) => {}
      Ok(false) => return Err(denied()),
      Err(error) => return Err(worker_permission(error)),
    }
  }
  let mut bytes = Vec::new();
  file
    .read_to_end(&mut bytes)
    .map_err(|error| error.to_string())?;
  Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn meta_is_base64(meta: &str) -> bool {
  meta
    .rsplit(';')
    .next()
    .is_some_and(|part| part.trim().eq_ignore_ascii_case("base64"))
}

fn sandbox_file(url: &Url, sandbox: &Path) -> Option<PathBuf> {
  let host = url.host_str()?;
  if host != "web-platform.test" && host != "localhost" && host != "127.0.0.1" {
    return None;
  }
  // Each URL path segment is one percent-decoded file name under the entry
  // directory. A segment that would name another directory, a root, or a
  // drive does not map to a file.
  let mut path = sandbox.to_path_buf();
  let mut named = false;
  for segment in url.path_segments()? {
    let bytes = crate::percent::decode(segment);
    let name = String::from_utf8(bytes).ok()?;
    if name.is_empty() || name == "." || name == ".." {
      return None;
    }
    if name.contains(['/', '\\', '\0']) {
      return None;
    }
    if cfg!(windows) && name.contains(':') {
      return None;
    }
    path.push(name);
    named = true;
  }
  named.then_some(path)
}

pub(crate) fn mark_dead(id: u64) {
  if let Some(agent) = agents().as_ref().and_then(|map| map.get(&id).cloned()) {
    agent.alive.store(false, Ordering::SeqCst);
  }
}

#[cfg(test)]
mod tests {
  use super::{
    decode_data_url, load_worker_source, sandbox_file,
    set_classic_worker_source,
  };
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
  fn a_fragment_is_not_part_of_the_payload() {
    let text =
      decode_data_url("data:text/javascript,postMessage(1)#x").unwrap();
    assert_eq!(text, "postMessage(1)");
    let text =
      decode_data_url("data:text/javascript;base64,aGk=#frag").unwrap();
    assert_eq!(text, "hi");
    let text = decode_data_url("data:text/javascript,a%23b#c").unwrap();
    assert_eq!(text, "a#b");
  }

  #[test]
  fn a_sandbox_url_path_is_percent_decoded() {
    let sandbox = std::path::Path::new("/entry");
    let file = |text: &str| sandbox_file(&Url::parse(text).unwrap(), sandbox);
    assert_eq!(
      file("http://localhost/my%20worker.js"),
      Some(sandbox.join("my worker.js"))
    );
    assert_eq!(
      file("http://127.0.0.1:8000/dir/%E3%81%82.js"),
      Some(sandbox.join("dir").join("\u{3042}.js"))
    );
    assert_eq!(file("http://localhost/a%2Fb.js"), None);
    assert_eq!(file("http://localhost/a%5Cb.js"), None);
    assert_eq!(file("http://localhost/a%00.js"), None);
    // The URL parser resolves `%2e%2e` as `..`, so this stays inside.
    assert_eq!(
      file("http://localhost/%2e%2e/x.js"),
      Some(sandbox.join("x.js"))
    );
    assert_eq!(file("http://localhost/"), None);
    assert_eq!(file("http://example.com/x.js"), None);
    if cfg!(windows) {
      assert_eq!(file("http://localhost/C:/x.js"), None);
    }
  }

  #[test]
  fn invalid_percent_escape_is_kept() {
    let text = decode_data_url("data:text/javascript,a%zz%4").unwrap();
    assert_eq!(text, "a%zz%4");
    let text = decode_data_url("data:text/javascript,%+1").unwrap();
    assert_eq!(text, "%+1");
  }

  #[test]
  fn invalid_base64_data_url_fails() {
    assert!(decode_data_url("data:;base64,****").is_err());
  }

  #[test]
  fn invalid_utf8_data_url_uses_the_replacement_character() {
    let text = decode_data_url("data:text/javascript,%FF").unwrap();
    assert_eq!(text, "\u{FFFD}");
    let base64 = decode_data_url("data:text/javascript;base64,/w==").unwrap();
    assert_eq!(base64, "\u{FFFD}");
  }

  #[test]
  fn empty_worker_url_keeps_the_base() {
    let base = Url::parse("file:///tmp/page.html").unwrap();
    assert_eq!(base.join("").unwrap().as_str(), "file:///tmp/page.html");
    assert!(base.join("http://invalid:123$").is_err());
  }

  #[test]
  fn classic_file_replaces_invalid_utf8() {
    let path = std::env::temp_dir()
      .join(format!("yuzuko-worker-utf8-{}", std::process::id()));
    std::fs::write(&path, b"postMessage(\"\xff\")").unwrap();
    let url = Url::from_file_path(&path).unwrap();
    let loaded = load_worker_source(url.as_str(), None).unwrap();
    assert_eq!(loaded.text, "postMessage(\"\u{FFFD}\")");
    let _ = std::fs::remove_file(&path);
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
