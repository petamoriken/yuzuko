//! Process-wide file stamps for `yuzuko run --watch` and `--watch-hmr`.
//!
//! The store outlives an isolate so a failed run can still wait on the paths
//! the loader noted. `note` is a no-op unless a watch session is armed.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant, SystemTime};

/// How often an armed event loop wakes to stat watched files.
pub(crate) const POLL: Duration = Duration::from_millis(50);

/// Quiet time after the last observed change before a reload is reported.
const DEBOUNCE: Duration = Duration::from_millis(200);

static ARMED: AtomicBool = AtomicBool::new(false);
static STORE: LazyLock<Mutex<Store>> = LazyLock::new(|| {
  Mutex::new(Store {
    armed: false,
    pending_since: None,
    files: HashMap::new(),
  })
});

#[derive(Clone, Copy, PartialEq, Eq)]
struct Stamp {
  modified: SystemTime,
  len: u64,
  exists: bool,
}

struct Watched {
  /// Stamp last handed to the runner. A later `poll` ignores it.
  baseline: Stamp,
  /// Stamp last seen on disk. A change here restarts the quiet timer.
  latest: Stamp,
}

struct Store {
  armed: bool,
  pending_since: Option<Instant>,
  files: HashMap<PathBuf, Watched>,
}

fn lock() -> std::sync::MutexGuard<'static, Store> {
  STORE.lock().expect("module watcher")
}

pub(crate) fn arm() {
  let mut store = lock();
  store.files.clear();
  store.pending_since = None;
  store.armed = true;
  ARMED.store(true, Ordering::Release);
}

pub(crate) fn disarm() {
  let mut store = lock();
  store.armed = false;
  store.files.clear();
  store.pending_since = None;
  ARMED.store(false, Ordering::Release);
}

pub(crate) fn active() -> bool {
  ARMED.load(Ordering::Acquire)
}

/// `Some(POLL)` while a watch session is armed, so the event loop wakes.
pub(crate) fn poll_wait() -> Option<Duration> {
  active().then_some(POLL)
}

/// Records `path` at its current stamp. An existing path keeps its baseline.
pub(crate) fn note(path: &Path) {
  if !active() {
    return;
  }
  let mut store = lock();
  if !store.armed || store.files.contains_key(path) {
    return;
  }
  let stamp = stamp_of(path);
  store.files.insert(
    path.to_path_buf(),
    Watched {
      baseline: stamp,
      latest: stamp,
    },
  );
}

/// One path whose stamp has been quiet for [`DEBOUNCE`], if any.
///
/// The returned change is consumed: every file's baseline becomes its latest
/// stamp, so the same bytes do not reload again.
pub(crate) fn poll() -> Option<PathBuf> {
  if !active() {
    return None;
  }
  let mut store = lock();
  if !store.armed {
    return None;
  }
  let now = Instant::now();
  let mut changed_now = false;
  let mut pending: Option<PathBuf> = None;
  for (path, watched) in &mut store.files {
    let stamp = stamp_of(path);
    if stamp != watched.latest {
      watched.latest = stamp;
      changed_now = true;
    }
    if watched.latest != watched.baseline
      && pending.as_ref().is_none_or(|current| path < current)
    {
      pending = Some(path.clone());
    }
  }
  let Some(pending) = pending else {
    store.pending_since = None;
    return None;
  };
  if changed_now || store.pending_since.is_none() {
    store.pending_since = Some(now);
  }
  let since = store.pending_since.expect("watch debounce");
  if now.duration_since(since) < DEBOUNCE {
    return None;
  }
  for watched in store.files.values_mut() {
    watched.baseline = watched.latest;
  }
  store.pending_since = None;
  Some(pending)
}

fn stamp_of(path: &Path) -> Stamp {
  match std::fs::metadata(path) {
    Ok(meta) => Stamp {
      modified: meta.modified().unwrap_or(SystemTime::UNIX_EPOCH),
      len: meta.len(),
      exists: true,
    },
    Err(_) => Stamp {
      modified: SystemTime::UNIX_EPOCH,
      len: 0,
      exists: false,
    },
  }
}
