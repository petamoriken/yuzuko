//! `Yuzuko.watchFs` and `Yuzuko.FsWatcher`.
//!
//! The watcher is created on the calling thread, before its wait thread
//! starts, so the kernel can queue events immediately. The wait thread only
//! calls [`crate::jobs::Listen::notify`]. Promise resolvers stay in a
//! thread-local map and are never stored on the cppgc object.

#[cfg(unix)]
use std::collections::HashSet;
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle, ThreadId};

use yuzuko_macros::op;

use super::error::{self, FsError};

#[cfg(unix)]
const WATCH_CAP: usize = 4096;

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

thread_local! {
  static LIVE: std::cell::RefCell<HashMap<u64, Arc<WatchShared>>> =
    std::cell::RefCell::new(HashMap::new());
  static WAITERS: std::cell::RefCell<
    HashMap<u64, VecDeque<v8::Global<v8::PromiseResolver>>>,
  > = std::cell::RefCell::new(HashMap::new());
}

struct WatchShared {
  id: u64,
  state: Mutex<WatchState>,
}

struct WatchState {
  closed: bool,
  events: VecDeque<FsEvent>,
  waiter: Option<crate::jobs::Listen>,
  keepalive: Option<crate::jobs::Listen>,
  thread: Option<JoinHandle<()>>,
  stop: Option<StopHandle>,
}

struct FsEvent {
  kind: &'static str,
  paths: Vec<String>,
  rescan: bool,
}

impl FsEvent {
  fn kind(kind: &'static str, paths: Vec<String>) -> Self {
    Self {
      kind,
      paths,
      rescan: false,
    }
  }

  fn rescan() -> Self {
    Self {
      kind: "any",
      paths: Vec::new(),
      rescan: true,
    }
  }
}

enum Outcome {
  Event(FsEvent),
  Done,
  Wait,
}

#[cfg(unix)]
type StopHandle = std::io::PipeWriter;

#[cfg(windows)]
struct StopHandle(usize);

#[cfg(not(any(unix, windows)))]
struct StopHandle;

pub struct FsWatcher {
  id: u64,
  shared: Arc<WatchShared>,
  isolate: ThreadId,
}

unsafe impl v8::cppgc::GarbageCollected for FsWatcher {
  fn trace(&self, _visitor: &mut v8::cppgc::Visitor) {}

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"FsWatcher"
  }
}

impl Drop for FsWatcher {
  fn drop(&mut self) {
    let on_isolate = thread::current().id() == self.isolate;
    if !on_isolate {
      let closed = lock(&self.shared).closed;
      if closed {
        return;
      }
      shutdown(&self.shared, false);
      return;
    }
    shutdown(&self.shared, true);
  }
}

pub(super) fn start(
  paths: Vec<PathBuf>,
  recursive: bool,
) -> Result<FsWatcher, FsError> {
  #[cfg(not(any(
    target_os = "linux",
    target_os = "macos",
    target_os = "windows"
  )))]
  {
    let _ = (paths, recursive);
    return Err(FsError::not_supported(
      "watchFs is not supported on this operating system.",
    ));
  }
  #[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
  {
    #[cfg(windows)]
    if paths.len() > 60 {
      return Err(FsError::not_supported("Too many watch paths."));
    }
    for path in &paths {
      std::fs::metadata(path).map_err(FsError::io)?;
    }
    launch(paths, recursive)
  }
}

pub(super) fn clear() {
  let shareds = LIVE.with(|live| {
    live
      .borrow_mut()
      .drain()
      .map(|(_, shared)| shared)
      .collect::<Vec<_>>()
  });
  for shared in shareds {
    shutdown(&shared, false);
  }
  WAITERS.with(|waiters| waiters.borrow_mut().clear());
}

pub(super) fn async_iterator(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut ret: v8::ReturnValue,
) {
  let this = args.this();
  let found = unsafe { crate::op::host_ref::<FsWatcher>(scope, this) };
  if found.is_none() {
    error::throw(scope, FsError::type_("illegal invocation"));
    return;
  }
  ret.set(this.into());
}

#[op]
impl FsWatcher {
  #[promise]
  fn next(
    &self,
    scope: &mut v8::PinScope,
  ) -> Result<crate::op::Js, crate::op::OpError> {
    let outcome = {
      let mut state = lock(&self.shared);
      if let Some(event) = state.events.pop_front() {
        Outcome::Event(event)
      } else if state.closed {
        Outcome::Done
      } else {
        Outcome::Wait
      }
    };
    match outcome {
      Outcome::Event(event) => resolve_now(scope, Some(event)),
      Outcome::Done => resolve_now(scope, None),
      Outcome::Wait => queue_waiter(scope, self.id, &self.shared),
    }
  }

  fn close(&self, scope: &mut v8::PinScope) -> Result<(), crate::op::OpError> {
    if shutdown(&self.shared, true) {
      Ok(())
    } else {
      Err(FsError::watcher_closed().fail(scope))
    }
  }

  #[promise]
  #[rename("return")]
  fn return_(
    &self,
    scope: &mut v8::PinScope,
  ) -> Result<crate::op::Js, crate::op::OpError> {
    shutdown(&self.shared, true);
    resolve_now(scope, None)
  }
}

fn lock(shared: &WatchShared) -> std::sync::MutexGuard<'_, WatchState> {
  shared
    .state
    .lock()
    .unwrap_or_else(|error| error.into_inner())
}

fn shutdown(shared: &WatchShared, touch_live: bool) -> bool {
  let (thread, waiter, keepalive, stop) = {
    let mut state = lock(shared);
    if state.closed {
      return false;
    }
    state.closed = true;
    (
      state.thread.take(),
      state.waiter.take(),
      state.keepalive.take(),
      state.stop.take(),
    )
  };
  if let Some(mut stop) = stop {
    signal_stop(&mut stop);
  }
  if let Some(thread) = thread {
    let _ = thread.join();
  }
  drop(waiter);
  drop(keepalive);
  if touch_live {
    LIVE.with(|live| {
      live.borrow_mut().remove(&shared.id);
    });
  }
  true
}

/// Stores one wake [`crate::jobs::Listen`] for an idle queue.
///
/// Dropping a `Listen` also notifies, so this never drops a listen while
/// another is stored and the queue is empty. `flush` is safe to run twice.
fn arm(shared: &Arc<WatchShared>) {
  {
    let mut state = lock(shared);
    if state.waiter.is_some() && state.events.is_empty() && !state.closed {
      return;
    }
    if (!state.events.is_empty() || state.closed)
      && let Some(waiter) = state.waiter.take()
    {
      drop(state);
      waiter.notify();
      return;
    }
  }
  let listen_shared = Arc::clone(shared);
  let listen = crate::jobs::listen(move |scope| flush(scope, listen_shared));
  let mut state = lock(shared);
  if state.waiter.is_some() || !state.events.is_empty() || state.closed {
    drop(state);
    listen.notify();
  } else {
    state.waiter = Some(listen);
  }
}

fn flush(scope: &mut v8::PinScope, shared: Arc<WatchShared>) {
  loop {
    let Some(resolver) = pop_waiter(shared.id) else {
      return;
    };
    let outcome = {
      let mut state = lock(&shared);
      if let Some(event) = state.events.pop_front() {
        Outcome::Event(event)
      } else if state.closed {
        Outcome::Done
      } else {
        Outcome::Wait
      }
    };
    match outcome {
      Outcome::Wait => {
        push_front_waiter(shared.id, resolver);
        arm(&shared);
        return;
      }
      Outcome::Event(event) => settle_waiter(scope, resolver, Some(event)),
      Outcome::Done => settle_waiter(scope, resolver, None),
    }
  }
}

fn pop_waiter(id: u64) -> Option<v8::Global<v8::PromiseResolver>> {
  WAITERS.with(|waiters| {
    let mut map = waiters.borrow_mut();
    let queue = map.get_mut(&id)?;
    let resolver = queue.pop_front()?;
    if queue.is_empty() {
      map.remove(&id);
    }
    Some(resolver)
  })
}

fn push_front_waiter(id: u64, resolver: v8::Global<v8::PromiseResolver>) {
  WAITERS.with(|waiters| {
    waiters
      .borrow_mut()
      .entry(id)
      .or_default()
      .push_front(resolver);
  });
}

fn queue_waiter(
  scope: &mut v8::PinScope,
  id: u64,
  shared: &Arc<WatchShared>,
) -> Result<crate::op::Js, crate::op::OpError> {
  let Some(resolver) = v8::PromiseResolver::new(scope) else {
    return Err(FsError::failed().fail(scope));
  };
  let promise: v8::Local<v8::Value> = resolver.get_promise(scope).into();
  WAITERS.with(|waiters| {
    waiters
      .borrow_mut()
      .entry(id)
      .or_default()
      .push_back(v8::Global::new(scope, resolver));
  });
  arm(shared);
  Ok(crate::op::Js(v8::Global::new(scope, promise)))
}

fn resolve_now(
  scope: &mut v8::PinScope,
  event: Option<FsEvent>,
) -> Result<crate::op::Js, crate::op::OpError> {
  let Some(resolver) = v8::PromiseResolver::new(scope) else {
    return Err(FsError::failed().fail(scope));
  };
  let promise: v8::Local<v8::Value> = resolver.get_promise(scope).into();
  settle_local(scope, resolver, event);
  Ok(crate::op::Js(v8::Global::new(scope, promise)))
}

fn settle_waiter(
  scope: &mut v8::PinScope,
  resolver: v8::Global<v8::PromiseResolver>,
  event: Option<FsEvent>,
) {
  let resolver = v8::Local::new(scope, resolver);
  settle_local(scope, resolver, event);
}

fn settle_local(
  scope: &mut v8::PinScope,
  resolver: v8::Local<v8::PromiseResolver>,
  event: Option<FsEvent>,
) {
  match iterator_result(scope, event) {
    Ok(value) => {
      let _ = resolver.resolve(scope, value);
    }
    Err(error) => {
      let exception = error.exception(scope);
      let _ = resolver.reject(scope, exception);
    }
  }
}

fn iterator_result<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  event: Option<FsEvent>,
) -> Result<v8::Local<'s, v8::Value>, FsError> {
  let done = event.is_none();
  let value = match event {
    Some(event) => event_object(scope, &event)?,
    None => v8::undefined(scope).into(),
  };
  let object = v8::Object::new(scope);
  set_value(scope, object, "value", value)?;
  let done = v8::Boolean::new(scope, done);
  set_value(scope, object, "done", done.into())?;
  Ok(object.into())
}

fn event_object<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  event: &FsEvent,
) -> Result<v8::Local<'s, v8::Value>, FsError> {
  let object = v8::Object::new(scope);
  let kind = v8_string(scope, event.kind)?;
  set_value(scope, object, "kind", kind.into())?;
  let paths = v8::Array::new(scope, event.paths.len() as i32);
  for (index, path) in event.paths.iter().enumerate() {
    let text = v8_string(scope, path)?;
    if paths.set_index(scope, index as u32, text.into()).is_none() {
      return Err(FsError::failed());
    }
  }
  set_value(scope, object, "paths", paths.into())?;
  if event.rescan {
    let flag = v8_string(scope, "rescan")?;
    set_value(scope, object, "flag", flag.into())?;
  }
  Ok(object.into())
}

fn set_value(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
  value: v8::Local<v8::Value>,
) -> Result<(), FsError> {
  let key = v8_string(scope, name)?;
  object.set(scope, key.into(), value);
  Ok(())
}

fn v8_string<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  text: &str,
) -> Result<v8::Local<'s, v8::String>, FsError> {
  v8::String::new(scope, text).ok_or_else(FsError::failed)
}

fn push_event(shared: &WatchShared, event: FsEvent) {
  let waiter = {
    let mut state = lock(shared);
    if state.closed {
      return;
    }
    state.events.push_back(event);
    state.waiter.take()
  };
  if let Some(waiter) = waiter {
    waiter.notify();
  }
}

fn absolute_path(path: &Path) -> PathBuf {
  if path.is_absolute() {
    path.to_path_buf()
  } else {
    match std::env::current_dir() {
      Ok(cwd) => cwd.join(path),
      Err(_) => path.to_path_buf(),
    }
  }
}

fn display_path(path: &Path) -> String {
  absolute_path(path)
    .to_string_lossy()
    .replace('\0', "\u{FFFD}")
}

#[cfg(unix)]
fn dedup_key(path: &Path) -> PathBuf {
  std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

#[cfg(unix)]
fn signal_stop(stop: &mut StopHandle) {
  use std::io::Write;
  let byte = [1u8];
  loop {
    match stop.write(&byte) {
      Ok(_) => return,
      Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
      Err(_) => return,
    }
  }
}

#[cfg(windows)]
fn signal_stop(stop: &mut StopHandle) {
  unsafe {
    windows_sys::Win32::System::Threading::SetEvent(stop.0 as _);
  }
}

#[cfg(not(any(unix, windows)))]
fn signal_stop(_stop: &mut StopHandle) {}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn child_dirs(path: &Path, later: bool) -> Result<Vec<PathBuf>, FsError> {
  let entries = match std::fs::read_dir(path) {
    Ok(entries) => entries,
    Err(error) if later || error.kind() == std::io::ErrorKind::NotFound => {
      return Ok(Vec::new());
    }
    Err(error) => return Err(FsError::io(error)),
  };
  let mut children = Vec::new();
  for entry in entries {
    let entry = match entry {
      Ok(entry) => entry,
      Err(error) if later || error.kind() == std::io::ErrorKind::NotFound => {
        continue;
      }
      Err(error) => return Err(FsError::io(error)),
    };
    let file_type = match entry.file_type() {
      Ok(file_type) => file_type,
      Err(error) if later || error.kind() == std::io::ErrorKind::NotFound => {
        continue;
      }
      Err(error) => return Err(FsError::io(error)),
    };
    if file_type.is_symlink() || !file_type.is_dir() {
      continue;
    }
    children.push(entry.path());
  }
  Ok(children)
}

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
trait Backend: Send + 'static {
  fn run(self, shared: Arc<WatchShared>);
}

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
struct Prepared<B> {
  backend: B,
  stop: StopHandle,
}

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
fn launch(paths: Vec<PathBuf>, recursive: bool) -> Result<FsWatcher, FsError> {
  let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
  let shared = Arc::new(WatchShared {
    id,
    state: Mutex::new(WatchState {
      closed: false,
      events: VecDeque::new(),
      waiter: None,
      keepalive: None,
      thread: None,
      stop: None,
    }),
  });
  let prepared = prepare(&shared, &paths, recursive)?;
  let thread_shared = Arc::clone(&shared);
  let backend = prepared.backend;
  let handle = thread::Builder::new()
    .name("yuzuko-fs-watch".to_string())
    .spawn(move || {
      let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        backend.run(thread_shared);
      }));
    })
    .map_err(FsError::io)?;
  {
    let mut state = lock(&shared);
    state.thread = Some(handle);
    state.stop = Some(prepared.stop);
  }
  let keepalive = crate::jobs::listen(|_| {});
  {
    let mut state = lock(&shared);
    state.keepalive = Some(keepalive);
  }
  LIVE.with(|live| {
    live.borrow_mut().insert(id, Arc::clone(&shared));
  });
  Ok(FsWatcher {
    id,
    shared,
    isolate: thread::current().id(),
  })
}

#[cfg(target_os = "macos")]
fn prepare(
  shared: &Arc<WatchShared>,
  paths: &[PathBuf],
  recursive: bool,
) -> Result<Prepared<MacBackend>, FsError> {
  use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
  let kq = unsafe { libc::kqueue() };
  if kq < 0 {
    return Err(FsError::io(std::io::Error::last_os_error()));
  }
  let kq = unsafe { OwnedFd::from_raw_fd(kq) };
  let (reader, writer) = std::io::pipe().map_err(FsError::io)?;
  change_one(
    kq.as_raw_fd(),
    reader.as_raw_fd() as usize,
    libc::EVFILT_READ,
    libc::EV_ADD,
    0,
  )?;
  let mut backend = MacBackend {
    kq,
    reader,
    recursive,
    nodes: HashMap::new(),
    seen: HashSet::new(),
    cap_announced: false,
  };
  for path in paths {
    let meta = std::fs::metadata(path).map_err(FsError::io)?;
    if meta.is_dir() {
      backend.watch_tree(shared, path, true, false)?;
    } else {
      backend.add_path(shared, path, false, true, false)?;
    }
  }
  Ok(Prepared {
    backend,
    stop: writer,
  })
}

#[cfg(target_os = "macos")]
struct MacBackend {
  kq: std::os::fd::OwnedFd,
  reader: std::io::PipeReader,
  recursive: bool,
  nodes: HashMap<usize, MacNode>,
  seen: HashSet<PathBuf>,
  cap_announced: bool,
}

#[cfg(target_os = "macos")]
struct MacNode {
  path: PathBuf,
  is_dir: bool,
  _file: std::fs::File,
}

#[cfg(target_os = "macos")]
impl MacBackend {
  fn watch_tree(
    &mut self,
    shared: &WatchShared,
    path: &Path,
    required: bool,
    later: bool,
  ) -> Result<(), FsError> {
    let added = self.add_path(shared, path, true, required, later)?;
    if !added || !self.recursive || self.cap_announced {
      return Ok(());
    }
    for child in child_dirs(path, later)? {
      self.watch_tree(shared, &child, false, later)?;
    }
    Ok(())
  }

  fn add_path(
    &mut self,
    shared: &WatchShared,
    path: &Path,
    is_dir: bool,
    required: bool,
    later: bool,
  ) -> Result<bool, FsError> {
    if self.cap_announced {
      return Ok(false);
    }
    let report = absolute_path(path);
    let key = dedup_key(&report);
    if self.seen.contains(&key) {
      return Ok(false);
    }
    if self.nodes.len() >= WATCH_CAP {
      self.note_cap(shared);
      return Ok(false);
    }
    match open_evtonly(&report) {
      Ok(file) => {
        use std::os::fd::AsRawFd;
        let ident = file.as_raw_fd() as usize;
        change_one(
          self.kq.as_raw_fd(),
          ident,
          libc::EVFILT_VNODE,
          libc::EV_ADD | libc::EV_CLEAR,
          vnode_notes(),
        )?;
        self.seen.insert(key);
        self.nodes.insert(
          ident,
          MacNode {
            path: report,
            is_dir,
            _file: file,
          },
        );
        Ok(true)
      }
      Err(error)
        if !required
          && (later || error.kind() == std::io::ErrorKind::NotFound) =>
      {
        Ok(false)
      }
      Err(error) => Err(FsError::io(error)),
    }
  }

  fn note_cap(&mut self, shared: &WatchShared) {
    if self.cap_announced {
      return;
    }
    self.cap_announced = true;
    push_event(shared, FsEvent::rescan());
  }

  fn on_vnode(&mut self, ident: usize, fflags: u32, shared: &WatchShared) {
    let Some(node) = self.nodes.get(&ident) else {
      return;
    };
    let path = node.path.clone();
    let is_dir = node.is_dir;
    push_event(
      shared,
      FsEvent::kind(vnode_kind(fflags), vec![display_path(&path)]),
    );
    let scan = is_dir
      && self.recursive
      && fflags & (libc::NOTE_WRITE | libc::NOTE_EXTEND | libc::NOTE_LINK) != 0
      && fflags & (libc::NOTE_DELETE | libc::NOTE_REVOKE | libc::NOTE_RENAME)
        == 0;
    if !scan {
      return;
    }
    let children = child_dirs(&path, true).unwrap_or_default();
    for child in children {
      if self.watch_tree(shared, &child, false, true).is_err() {
        return;
      }
    }
  }
}

#[cfg(target_os = "macos")]
impl Backend for MacBackend {
  fn run(mut self, shared: Arc<WatchShared>) {
    use std::os::fd::AsRawFd;
    let kq = self.kq.as_raw_fd();
    loop {
      let mut events: [libc::kevent; 32] = unsafe { std::mem::zeroed() };
      let count = unsafe {
        libc::kevent(
          kq,
          std::ptr::null(),
          0,
          events.as_mut_ptr(),
          events.len() as libc::c_int,
          std::ptr::null(),
        )
      };
      if count < 0 {
        let error = std::io::Error::last_os_error();
        if error.kind() == std::io::ErrorKind::Interrupted {
          continue;
        }
        break;
      }
      let mut stop = false;
      for event in events.iter().take(count as usize) {
        let (ident, filter, flags, fflags, _) = read_kevent(event);
        if filter == libc::EVFILT_READ {
          stop = true;
          continue;
        }
        if flags & libc::EV_ERROR != 0 || filter != libc::EVFILT_VNODE {
          continue;
        }
        self.on_vnode(ident, fflags, &shared);
      }
      if stop {
        break;
      }
    }
    drop(self.reader);
  }
}

#[cfg(target_os = "macos")]
fn open_evtonly(path: &Path) -> std::io::Result<std::fs::File> {
  use std::os::unix::fs::OpenOptionsExt;
  std::fs::OpenOptions::new()
    .read(true)
    .custom_flags(libc::O_EVTONLY)
    .open(path)
}

#[cfg(target_os = "macos")]
fn vnode_notes() -> u32 {
  libc::NOTE_DELETE
    | libc::NOTE_WRITE
    | libc::NOTE_EXTEND
    | libc::NOTE_ATTRIB
    | libc::NOTE_LINK
    | libc::NOTE_RENAME
    | libc::NOTE_REVOKE
}

#[cfg(target_os = "macos")]
fn vnode_kind(fflags: u32) -> &'static str {
  if fflags & (libc::NOTE_DELETE | libc::NOTE_REVOKE) != 0 {
    "remove"
  } else if fflags & libc::NOTE_RENAME != 0 {
    "rename"
  } else if fflags & (libc::NOTE_WRITE | libc::NOTE_EXTEND) != 0 {
    "modify"
  } else {
    "other"
  }
}

#[cfg(target_os = "macos")]
fn change_one(
  kq: std::os::fd::RawFd,
  ident: usize,
  filter: i16,
  flags: u16,
  fflags: u32,
) -> Result<(), FsError> {
  let mut change = unsafe { std::mem::zeroed::<libc::kevent>() };
  write_kevent(&mut change, ident, filter, flags, fflags);
  // A null timeout waits forever. Registration only needs the change applied.
  let timeout = unsafe { std::mem::zeroed::<libc::timespec>() };
  loop {
    let mut out = unsafe { std::mem::zeroed::<libc::kevent>() };
    let rc = unsafe { libc::kevent(kq, &change, 1, &mut out, 1, &timeout) };
    if rc < 0 {
      let error = std::io::Error::last_os_error();
      if error.kind() == std::io::ErrorKind::Interrupted {
        continue;
      }
      return Err(FsError::io(error));
    }
    if rc > 0 {
      let (_, _, flags, _, data) = read_kevent(&out);
      if flags & libc::EV_ERROR != 0 && data != 0 {
        return Err(FsError::io(std::io::Error::from_raw_os_error(
          data as i32,
        )));
      }
    }
    return Ok(());
  }
}

/// `kevent` is packed. Field references are misaligned, so the bytes are
/// copied from the struct base.
#[cfg(target_os = "macos")]
fn write_kevent(
  event: &mut libc::kevent,
  ident: usize,
  filter: i16,
  flags: u16,
  fflags: u32,
) {
  unsafe {
    let base = (event as *mut libc::kevent).cast::<u8>();
    std::ptr::write_unaligned(base.cast::<usize>(), ident);
    let filter_at = base.add(std::mem::size_of::<usize>());
    std::ptr::write_unaligned(filter_at.cast::<i16>(), filter);
    std::ptr::write_unaligned(filter_at.add(2).cast::<u16>(), flags);
    std::ptr::write_unaligned(filter_at.add(4).cast::<u32>(), fflags);
  }
}

#[cfg(target_os = "macos")]
fn read_kevent(event: &libc::kevent) -> (usize, i16, u16, u32, isize) {
  unsafe {
    let base = (event as *const libc::kevent).cast::<u8>();
    let ident = std::ptr::read_unaligned(base.cast::<usize>());
    let filter_at = base.add(std::mem::size_of::<usize>());
    let filter = std::ptr::read_unaligned(filter_at.cast::<i16>());
    let flags = std::ptr::read_unaligned(filter_at.add(2).cast::<u16>());
    let fflags = std::ptr::read_unaligned(filter_at.add(4).cast::<u32>());
    let data =
      std::ptr::read_unaligned(filter_at.add(8).cast::<libc::intptr_t>());
    (ident, filter, flags, fflags, data as isize)
  }
}

#[cfg(target_os = "linux")]
fn prepare(
  shared: &Arc<WatchShared>,
  paths: &[PathBuf],
  recursive: bool,
) -> Result<Prepared<LinuxBackend>, FsError> {
  use std::os::fd::{FromRawFd, OwnedFd};
  let fd = unsafe { libc::inotify_init1(libc::IN_CLOEXEC | libc::IN_NONBLOCK) };
  if fd < 0 {
    return Err(FsError::io(std::io::Error::last_os_error()));
  }
  let fd = unsafe { OwnedFd::from_raw_fd(fd) };
  let (reader, writer) = std::io::pipe().map_err(FsError::io)?;
  let mut backend = LinuxBackend {
    fd,
    reader,
    recursive,
    wds: HashMap::new(),
    seen: HashSet::new(),
    cap_announced: false,
  };
  for path in paths {
    let meta = std::fs::metadata(path).map_err(FsError::io)?;
    if meta.is_dir() {
      backend.watch_tree(shared, path, true, false)?;
    } else {
      backend.add_path(shared, path, true, false)?;
    }
  }
  Ok(Prepared {
    backend,
    stop: writer,
  })
}

#[cfg(target_os = "linux")]
struct LinuxBackend {
  fd: std::os::fd::OwnedFd,
  reader: std::io::PipeReader,
  recursive: bool,
  wds: HashMap<i32, PathBuf>,
  seen: HashSet<PathBuf>,
  cap_announced: bool,
}

#[cfg(target_os = "linux")]
impl LinuxBackend {
  fn watch_tree(
    &mut self,
    shared: &WatchShared,
    path: &Path,
    required: bool,
    later: bool,
  ) -> Result<(), FsError> {
    let added = self.add_path(shared, path, required, later)?;
    if !added || !self.recursive || self.cap_announced {
      return Ok(());
    }
    for child in child_dirs(path, later)? {
      self.watch_tree(shared, &child, false, later)?;
    }
    Ok(())
  }

  fn add_path(
    &mut self,
    shared: &WatchShared,
    path: &Path,
    required: bool,
    later: bool,
  ) -> Result<bool, FsError> {
    use std::os::fd::AsRawFd;
    if self.cap_announced {
      return Ok(false);
    }
    let report = absolute_path(path);
    let key = dedup_key(&report);
    if self.seen.contains(&key) {
      return Ok(false);
    }
    if self.wds.len() >= WATCH_CAP {
      self.note_cap(shared);
      return Ok(false);
    }
    let text = super::io::path_cstring(&report)?;
    let wd = unsafe {
      libc::inotify_add_watch(self.fd.as_raw_fd(), text.as_ptr(), linux_mask())
    };
    if wd < 0 {
      let error = std::io::Error::last_os_error();
      if !required && (later || error.kind() == std::io::ErrorKind::NotFound) {
        return Ok(false);
      }
      return Err(FsError::io(error));
    }
    self.seen.insert(key);
    self.wds.insert(wd, report);
    Ok(true)
  }

  fn note_cap(&mut self, shared: &WatchShared) {
    if self.cap_announced {
      return;
    }
    self.cap_announced = true;
    push_event(shared, FsEvent::rescan());
  }
}

#[cfg(target_os = "linux")]
impl Backend for LinuxBackend {
  fn run(mut self, shared: Arc<WatchShared>) {
    use std::os::fd::AsRawFd;
    let notify = self.fd.as_raw_fd();
    let wake = self.reader.as_raw_fd();
    let mut buf = [0u8; 8192];
    loop {
      let mut fds = [
        libc::pollfd {
          fd: notify,
          events: libc::POLLIN as libc::c_short,
          revents: 0,
        },
        libc::pollfd {
          fd: wake,
          events: libc::POLLIN as libc::c_short,
          revents: 0,
        },
      ];
      let ready =
        unsafe { libc::poll(fds.as_mut_ptr(), 2 as libc::nfds_t, -1) };
      if ready < 0 {
        let error = std::io::Error::last_os_error();
        if error.kind() == std::io::ErrorKind::Interrupted {
          continue;
        }
        break;
      }
      let readable = libc::POLLIN as libc::c_short;
      if fds[0].revents & readable != 0 {
        self.drain(&shared, &mut buf);
      }
      if fds[1].revents & readable != 0 {
        break;
      }
    }
  }
}

#[cfg(target_os = "linux")]
impl LinuxBackend {
  fn drain(&mut self, shared: &WatchShared, buf: &mut [u8]) {
    use std::os::fd::AsRawFd;
    loop {
      let read = unsafe {
        libc::read(self.fd.as_raw_fd(), buf.as_mut_ptr().cast(), buf.len())
      };
      if read < 0 {
        let error = std::io::Error::last_os_error();
        if error.kind() == std::io::ErrorKind::Interrupted {
          continue;
        }
        break;
      }
      if read == 0 {
        break;
      }
      self.parse(shared, &buf[..read as usize]);
    }
  }

  fn parse(&mut self, shared: &WatchShared, buf: &[u8]) {
    let mut index = 0usize;
    let mut pending: HashMap<u32, String> = HashMap::new();
    while index + 16 <= buf.len() {
      let wd = i32::from_ne_bytes(buf[index..index + 4].try_into().unwrap());
      let mask =
        u32::from_ne_bytes(buf[index + 4..index + 8].try_into().unwrap());
      let cookie =
        u32::from_ne_bytes(buf[index + 8..index + 12].try_into().unwrap());
      let len =
        u32::from_ne_bytes(buf[index + 12..index + 16].try_into().unwrap())
          as usize;
      let next = match index.checked_add(16).and_then(|at| at.checked_add(len))
      {
        Some(next) if next <= buf.len() => next,
        _ => break,
      };
      let name = inotify_name(&buf[index + 16..next]);
      index = next;
      if mask & libc::IN_Q_OVERFLOW != 0 {
        push_event(shared, FsEvent::rescan());
        continue;
      }
      let ignored = mask & libc::IN_IGNORED != 0;
      if ignored && mask & !(libc::IN_IGNORED | libc::IN_ISDIR) == 0 {
        self.wds.remove(&wd);
        continue;
      }
      let Some(base) = self.wds.get(&wd).cloned() else {
        if ignored {
          self.wds.remove(&wd);
        }
        continue;
      };
      let path = if name.is_empty() {
        display_path(&base)
      } else {
        display_path(&base.join(&name))
      };
      match linux_class(mask) {
        LinuxClass::From if cookie != 0 => {
          pending.insert(cookie, path);
        }
        LinuxClass::To if cookie != 0 => {
          let paths = match pending.remove(&cookie) {
            Some(from) => vec![from, path],
            None => vec![path],
          };
          push_event(shared, FsEvent::kind("rename", paths));
        }
        class => {
          push_event(shared, FsEvent::kind(class.kind(), vec![path]));
        }
      }
      if self.recursive
        && mask & libc::IN_ISDIR != 0
        && mask & (libc::IN_CREATE | libc::IN_MOVED_TO) != 0
        && !name.is_empty()
      {
        let child = base.join(&name);
        let _ = self.watch_tree(shared, &child, false, true);
      }
      if ignored {
        self.wds.remove(&wd);
      }
    }
    for (_, path) in pending {
      push_event(shared, FsEvent::kind("rename", vec![path]));
    }
  }
}

#[cfg(target_os = "linux")]
enum LinuxClass {
  Remove,
  From,
  To,
  Create,
  Modify,
  Access,
  Other,
}

#[cfg(target_os = "linux")]
impl LinuxClass {
  fn kind(self) -> &'static str {
    match self {
      Self::Remove => "remove",
      Self::From | Self::To => "rename",
      Self::Create => "create",
      Self::Modify => "modify",
      Self::Access => "access",
      Self::Other => "other",
    }
  }
}

#[cfg(target_os = "linux")]
fn linux_class(mask: u32) -> LinuxClass {
  if mask & (libc::IN_DELETE | libc::IN_DELETE_SELF) != 0 {
    LinuxClass::Remove
  } else if mask & libc::IN_MOVED_FROM != 0 {
    LinuxClass::From
  } else if mask & libc::IN_MOVED_TO != 0 {
    LinuxClass::To
  } else if mask & libc::IN_CREATE != 0 {
    LinuxClass::Create
  } else if mask & (libc::IN_MODIFY | libc::IN_CLOSE_WRITE) != 0 {
    LinuxClass::Modify
  } else if mask & libc::IN_ACCESS != 0 {
    LinuxClass::Access
  } else {
    LinuxClass::Other
  }
}

#[cfg(target_os = "linux")]
fn linux_mask() -> u32 {
  libc::IN_CREATE
    | libc::IN_DELETE
    | libc::IN_DELETE_SELF
    | libc::IN_MODIFY
    | libc::IN_MOVED_FROM
    | libc::IN_MOVED_TO
    | libc::IN_ATTRIB
    | libc::IN_CLOSE_WRITE
    | libc::IN_ACCESS
}

#[cfg(target_os = "linux")]
fn inotify_name(bytes: &[u8]) -> String {
  let end = bytes
    .iter()
    .position(|byte| *byte == 0)
    .unwrap_or(bytes.len());
  String::from_utf8_lossy(&bytes[..end]).replace('\0', "\u{FFFD}")
}

#[cfg(windows)]
fn prepare(
  _shared: &Arc<WatchShared>,
  paths: &[PathBuf],
  recursive: bool,
) -> Result<Prepared<WinBackend>, FsError> {
  use std::os::windows::io::AsRawHandle;
  let stop = create_event(true)?;
  let raw = stop.as_raw_handle() as usize;
  let mut dirs = Vec::with_capacity(paths.len());
  for path in paths {
    let meta = std::fs::metadata(path).map_err(FsError::io)?;
    let report = absolute_path(path);
    let (directory, filter, subtree) = if meta.is_dir() {
      (report.clone(), None, recursive)
    } else {
      let Some(parent) = report.parent() else {
        return Err(FsError::not_supported(
          "The path has no parent directory.",
        ));
      };
      let Some(name) = report.file_name() else {
        return Err(FsError::not_supported(
          "The path has no parent directory.",
        ));
      };
      (
        parent.to_path_buf(),
        Some(name.to_string_lossy().replace('\0', "\u{FFFD}")),
        false,
      )
    };
    dirs.push(WinDir::open(report, directory, filter, subtree)?);
  }
  for dir in &mut dirs {
    issue_read(dir)?;
  }
  Ok(Prepared {
    backend: WinBackend { dirs, stop },
    stop: StopHandle(raw),
  })
}

#[cfg(windows)]
struct WinBackend {
  dirs: Vec<WinDir>,
  stop: std::os::windows::io::OwnedHandle,
}

#[cfg(windows)]
struct WinDir {
  report: PathBuf,
  directory: PathBuf,
  filter: Option<String>,
  recursive: bool,
  handle: std::os::windows::io::OwnedHandle,
  event: std::os::windows::io::OwnedHandle,
  overlapped: Box<windows_sys::Win32::System::IO::OVERLAPPED>,
  buffer: Vec<u8>,
  returned: u32,
  dead: bool,
}

#[cfg(windows)]
// SAFETY: `overlapped.hEvent` copies the handle owned by `event`. After
// `prepare` returns, only the watch thread uses this directory.
unsafe impl Send for WinDir {}

#[cfg(windows)]
impl WinDir {
  fn open(
    report: PathBuf,
    directory: PathBuf,
    filter: Option<String>,
    recursive: bool,
  ) -> Result<Self, FsError> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
      CreateFileW, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OVERLAPPED,
      FILE_LIST_DIRECTORY, FILE_SHARE_DELETE, FILE_SHARE_READ,
      FILE_SHARE_WRITE, OPEN_EXISTING,
    };
    let wide = wide_null(&directory);
    let handle = unsafe {
      CreateFileW(
        wide.as_ptr(),
        FILE_LIST_DIRECTORY,
        FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
        std::ptr::null(),
        OPEN_EXISTING,
        FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OVERLAPPED,
        std::ptr::null_mut(),
      )
    };
    let handle = owned_from(handle)?;
    let event = create_event(false)?;
    let mut overlapped =
      Box::new(windows_sys::Win32::System::IO::OVERLAPPED::default());
    overlapped.hEvent = event.as_raw_handle() as _;
    Ok(Self {
      report,
      directory,
      filter,
      recursive,
      handle,
      event,
      overlapped,
      buffer: vec![0u8; 64 * 1024],
      returned: 0,
      dead: false,
    })
  }
}

#[cfg(windows)]
impl Backend for WinBackend {
  fn run(mut self, shared: Arc<WatchShared>) {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Foundation::WAIT_OBJECT_0;
    use windows_sys::Win32::System::Threading::{
      INFINITE, WaitForMultipleObjects,
    };
    let mut handles = Vec::with_capacity(self.dirs.len() + 1);
    handles.push(self.stop.as_raw_handle() as _);
    for dir in &self.dirs {
      handles.push(dir.event.as_raw_handle() as _);
    }
    loop {
      let status = unsafe {
        WaitForMultipleObjects(
          handles.len() as u32,
          handles.as_ptr(),
          0,
          INFINITE,
        )
      };
      if status == WAIT_OBJECT_0 {
        break;
      }
      let index = status.wrapping_sub(WAIT_OBJECT_0) as usize;
      if index == 0 || index > self.dirs.len() {
        break;
      }
      let dir = &mut self.dirs[index - 1];
      if dir.dead {
        continue;
      }
      let mut transferred = 0u32;
      let ok = unsafe {
        windows_sys::Win32::System::IO::GetOverlappedResult(
          dir.handle.as_raw_handle() as _,
          dir.overlapped.as_ref(),
          &mut transferred,
          0,
        )
      };
      if ok == 0 {
        let code = last_error_code();
        if code == windows_sys::Win32::Foundation::ERROR_NOTIFY_ENUM_DIR {
          push_event(&shared, FsEvent::rescan());
        }
      } else {
        parse_notifications(dir, &shared, transferred);
      }
      if issue_read(dir).is_err() {
        dir.dead = true;
        push_event(&shared, FsEvent::rescan());
      }
    }
  }
}

#[cfg(windows)]
fn issue_read(dir: &mut WinDir) -> Result<(), FsError> {
  use std::os::windows::io::AsRawHandle;
  use windows_sys::Win32::Foundation::ERROR_IO_PENDING;
  use windows_sys::Win32::Storage::FileSystem::{
    FILE_NOTIFY_CHANGE_ATTRIBUTES, FILE_NOTIFY_CHANGE_DIR_NAME,
    FILE_NOTIFY_CHANGE_FILE_NAME, FILE_NOTIFY_CHANGE_LAST_ACCESS,
    FILE_NOTIFY_CHANGE_LAST_WRITE, FILE_NOTIFY_CHANGE_SIZE,
    ReadDirectoryChangesW,
  };
  let event = dir.overlapped.hEvent;
  *dir.overlapped = windows_sys::Win32::System::IO::OVERLAPPED::default();
  dir.overlapped.hEvent = event;
  let handle = dir.handle.as_raw_handle() as _;
  let buffer = dir.buffer.as_mut_ptr().cast();
  let length = dir.buffer.len() as u32;
  let returned = &mut dir.returned as *mut u32;
  let overlapped = dir.overlapped.as_mut() as *mut _;
  let ok = unsafe {
    ReadDirectoryChangesW(
      handle,
      buffer,
      length,
      if dir.recursive { 1 } else { 0 },
      FILE_NOTIFY_CHANGE_FILE_NAME
        | FILE_NOTIFY_CHANGE_DIR_NAME
        | FILE_NOTIFY_CHANGE_ATTRIBUTES
        | FILE_NOTIFY_CHANGE_SIZE
        | FILE_NOTIFY_CHANGE_LAST_WRITE
        | FILE_NOTIFY_CHANGE_LAST_ACCESS,
      returned,
      overlapped,
      None,
    )
  };
  if ok != 0 {
    return Ok(());
  }
  let code = last_error_code();
  if code == ERROR_IO_PENDING
    || code == windows_sys::Win32::Foundation::ERROR_NOTIFY_ENUM_DIR
  {
    return Ok(());
  }
  Err(FsError::io(std::io::Error::from_raw_os_error(code as i32)))
}

#[cfg(windows)]
fn parse_notifications(dir: &WinDir, shared: &WatchShared, transferred: u32) {
  use windows_sys::Win32::Storage::FileSystem::{
    FILE_ACTION_ADDED, FILE_ACTION_MODIFIED, FILE_ACTION_REMOVED,
    FILE_ACTION_RENAMED_NEW_NAME, FILE_ACTION_RENAMED_OLD_NAME,
  };
  let bytes = &dir.buffer[..transferred as usize];
  let mut offset = 0usize;
  let mut old_name: Option<String> = None;
  while offset + 12 <= bytes.len() {
    let next =
      u32::from_ne_bytes(bytes[offset..offset + 4].try_into().unwrap());
    let action =
      u32::from_ne_bytes(bytes[offset + 4..offset + 8].try_into().unwrap());
    let name_len =
      u32::from_ne_bytes(bytes[offset + 8..offset + 12].try_into().unwrap())
        as usize;
    let name_at = offset + 12;
    let name_end = match name_at.checked_add(name_len) {
      Some(end) if end <= bytes.len() => end,
      _ => break,
    };
    let relative = utf16_path(&bytes[name_at..name_end]);
    if next == 0 {
      offset = bytes.len();
    } else {
      offset = match offset.checked_add(next as usize) {
        Some(next_offset) if next_offset <= bytes.len() => next_offset,
        _ => bytes.len(),
      };
    }
    if !name_matches(dir.filter.as_deref(), &relative) {
      if next == 0 {
        break;
      }
      continue;
    }
    let path = if dir.filter.is_some() || relative.is_empty() {
      display_path(&dir.report)
    } else {
      display_path(&dir.directory.join(&relative))
    };
    if action == FILE_ACTION_RENAMED_OLD_NAME {
      if let Some(previous) = old_name.take() {
        push_event(shared, FsEvent::kind("rename", vec![previous]));
      }
      old_name = Some(path);
    } else if action == FILE_ACTION_RENAMED_NEW_NAME {
      let paths = match old_name.take() {
        Some(previous) => vec![previous, path],
        None => vec![path],
      };
      push_event(shared, FsEvent::kind("rename", paths));
    } else {
      if let Some(previous) = old_name.take() {
        push_event(shared, FsEvent::kind("rename", vec![previous]));
      }
      let kind = if action == FILE_ACTION_ADDED {
        "create"
      } else if action == FILE_ACTION_REMOVED {
        "remove"
      } else if action == FILE_ACTION_MODIFIED {
        "modify"
      } else {
        "other"
      };
      push_event(shared, FsEvent::kind(kind, vec![path]));
    }
    if next == 0 {
      break;
    }
  }
  if let Some(previous) = old_name {
    push_event(shared, FsEvent::kind("rename", vec![previous]));
  }
}

#[cfg(windows)]
fn name_matches(filter: Option<&str>, relative: &str) -> bool {
  let Some(filter) = filter else {
    return true;
  };
  if relative.is_empty() {
    return false;
  }
  let name = Path::new(relative)
    .file_name()
    .map(|name| name.to_string_lossy().into_owned())
    .unwrap_or_else(|| relative.to_string());
  name.eq_ignore_ascii_case(filter) || relative.eq_ignore_ascii_case(filter)
}

#[cfg(windows)]
fn utf16_path(bytes: &[u8]) -> String {
  let mut units = Vec::with_capacity(bytes.len() / 2);
  for pair in bytes.as_chunks::<2>().0 {
    units.push(u16::from_ne_bytes([pair[0], pair[1]]));
  }
  String::from_utf16_lossy(&units).replace('\0', "\u{FFFD}")
}

#[cfg(windows)]
fn wide_null(path: &Path) -> Vec<u16> {
  use std::os::windows::ffi::OsStrExt;
  let mut wide: Vec<u16> = path.as_os_str().encode_wide().collect();
  wide.push(0);
  wide
}

#[cfg(windows)]
fn create_event(
  manual: bool,
) -> Result<std::os::windows::io::OwnedHandle, FsError> {
  let handle = unsafe {
    windows_sys::Win32::System::Threading::CreateEventW(
      std::ptr::null(),
      if manual { 1 } else { 0 },
      0,
      std::ptr::null(),
    )
  };
  owned_from(handle)
}

#[cfg(windows)]
fn owned_from(
  handle: windows_sys::Win32::Foundation::HANDLE,
) -> Result<std::os::windows::io::OwnedHandle, FsError> {
  use std::os::windows::io::{FromRawHandle, OwnedHandle};
  use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
  if handle.is_null() || handle == INVALID_HANDLE_VALUE {
    return Err(FsError::io(std::io::Error::from_raw_os_error(
      last_error_code() as i32,
    )));
  }
  Ok(unsafe { OwnedHandle::from_raw_handle(handle as _) })
}

#[cfg(windows)]
fn last_error_code() -> u32 {
  unsafe { windows_sys::Win32::Foundation::GetLastError() }
}
