//! Optional `Yuzuko` signal listeners.
//!
//! The `signal` feature installs `addSignalListener` and
//! `removeSignalListener` on the main realm and on a dedicated worker. The
//! operating-system handler only records the signal. Listeners run later on
//! the realm thread, so the handler never calls V8.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::time::{Duration, Instant};

use crate::platform::TaskQueue;

const ARITY: &str = "Not enough arguments";
const SIGNAL_TYPE: &str = "The signal must be a string.";
const CANNOT_CATCH: &str = "The signal cannot be caught.";
const NOT_SUPPORTED: &str = "The signal is not supported.";
#[cfg_attr(not(windows), allow(dead_code))]
const WINDOWS_SIGNAL: &str = "Windows cannot catch this signal.";
const LISTENER: &str = "The listener must be a function.";
const CAUGHT_FAIL: &str = "The signal could not be caught.";
const NOT_CAUGHT: &str = "The signal is not caught.";
const NOT_DELIVERED: &str = "The signal could not be delivered.";
const DELIVER_WAIT: Duration = Duration::from_secs(2);

static TEST_PROCESS: AtomicBool = AtomicBool::new(false);
static REALMS: Mutex<Vec<Weak<RealmSlot>>> = Mutex::new(Vec::new());

thread_local! {
  static LOCAL: std::cell::RefCell<Option<LocalRealm>> =
    const { std::cell::RefCell::new(None) };
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Sig(u8);

impl Sig {
  fn index(self) -> usize {
    usize::from(self.0)
  }
}

struct RealmSlot {
  pending: Mutex<VecDeque<Sig>>,
  generation: AtomicU64,
  counts: [AtomicUsize; crate::posix_signal::LISTEN_COUNT],
  queue: Arc<TaskQueue>,
}

struct LocalRealm {
  slot: Arc<RealmSlot>,
  listeners: [Vec<v8::Global<v8::Function>>; crate::posix_signal::LISTEN_COUNT],
}

impl RealmSlot {
  fn count(&self, sig: Sig) -> usize {
    self.counts[sig.index()].load(Ordering::Acquire)
  }

  fn increment(&self, sig: Sig) {
    self.counts[sig.index()].fetch_add(1, Ordering::Release);
  }

  fn decrement(&self, sig: Sig) {
    self.counts[sig.index()].fetch_sub(1, Ordering::Release);
  }
}

impl LocalRealm {
  fn listeners(&self, sig: Sig) -> &Vec<v8::Global<v8::Function>> {
    &self.listeners[sig.index()]
  }

  fn listeners_mut(&mut self, sig: Sig) -> &mut Vec<v8::Global<v8::Function>> {
    &mut self.listeners[sig.index()]
  }
}

pub(crate) fn set_test_process(enabled: bool) {
  TEST_PROCESS.store(enabled, Ordering::Relaxed);
}

pub(crate) fn install(scope: &mut v8::PinScope) {
  let slot = Arc::new(RealmSlot {
    pending: Mutex::new(VecDeque::new()),
    generation: AtomicU64::new(0),
    counts: std::array::from_fn(|_| AtomicUsize::new(0)),
    queue: crate::host::queue(),
  });
  register_slot(&slot);
  LOCAL.with(|local| {
    *local.borrow_mut() = Some(LocalRealm {
      slot,
      listeners: std::array::from_fn(|_| Vec::new()),
    });
  });
  let yuzuko = crate::js::namespace(scope, "Yuzuko");
  define_fn(scope, yuzuko, "addSignalListener", 2, add_listener);
  define_fn(scope, yuzuko, "removeSignalListener", 2, remove_listener);
  if TEST_PROCESS.load(Ordering::Relaxed) {
    define_fn(scope, yuzuko, "raiseSignal", 1, raise_signal);
  }
}

pub(crate) fn clear() {
  let local = LOCAL.with(|slot| slot.borrow_mut().take());
  let Some(local) = local else {
    return;
  };
  release_counts(&local);
  unregister_slot(&local.slot);
}

/// Drops listeners and keeps the realm slot.
///
/// [`clear`] removes the slot. A hot reload still accepts `addSignalListener`.
pub(crate) fn release_listeners() {
  LOCAL.with(|slot| {
    let mut local = slot.borrow_mut();
    let Some(realm) = local.as_mut() else {
      return;
    };
    release_counts(realm);
    for listeners in &mut realm.listeners {
      listeners.clear();
    }
    realm.slot.pending.lock().expect("signal queue").clear();
  });
}

fn release_counts(realm: &LocalRealm) {
  for (index, count) in realm.slot.counts.iter().enumerate() {
    let count = count.swap(0, Ordering::AcqRel);
    if count > 0 {
      os::release(Sig(index as u8), count);
    }
  }
}

/// Runs listeners for signals recorded for this realm.
///
/// Returns whether any listener ran. A registered listener does not keep
/// the event loop alive; a recorded delivery does, until this drains it.
pub(crate) fn poll(scope: &mut v8::PinScope) -> bool {
  let pending = LOCAL.with(|slot| {
    let local = slot.borrow();
    let Some(realm) = local.as_ref() else {
      return Vec::new();
    };
    let mut queue = lock(&realm.slot.pending);
    queue.drain(..).collect::<Vec<_>>()
  });
  if pending.is_empty() {
    return false;
  }
  for sig in pending {
    dispatch(scope, sig);
  }
  true
}

pub(crate) fn pending() -> bool {
  LOCAL.with(|slot| {
    let local = slot.borrow();
    local
      .as_ref()
      .is_some_and(|realm| !lock(&realm.slot.pending).is_empty())
  })
}

fn publish(sig: Sig) {
  let realms = {
    let mut realms = lock(&REALMS);
    realms.retain(|weak| weak.strong_count() > 0);
    realms.iter().filter_map(Weak::upgrade).collect::<Vec<_>>()
  };
  for realm in realms {
    if realm.count(sig) == 0 {
      continue;
    }
    {
      let mut pending = lock(&realm.pending);
      pending.push_back(sig);
    }
    realm.generation.fetch_add(1, Ordering::Release);
    realm.queue.poke();
  }
}

fn register_slot(slot: &Arc<RealmSlot>) {
  lock(&REALMS).push(Arc::downgrade(slot));
}

fn unregister_slot(slot: &Arc<RealmSlot>) {
  lock(&REALMS)
    .retain(|weak| weak.upgrade().is_none_or(|live| !Arc::ptr_eq(&live, slot)));
}

fn dispatch(scope: &mut v8::PinScope, sig: Sig) {
  let functions = LOCAL.with(|slot| {
    slot
      .borrow()
      .as_ref()
      .map(|realm| realm.listeners(sig).clone())
      .unwrap_or_default()
  });
  for function in functions {
    if !registered(scope, sig, &function) {
      continue;
    }
    call_listener(scope, &function);
  }
}

fn registered(
  scope: &mut v8::PinScope,
  sig: Sig,
  function: &v8::Global<v8::Function>,
) -> bool {
  LOCAL.with(|slot| {
    let local = slot.borrow();
    let Some(realm) = local.as_ref() else {
      return false;
    };
    let current = v8::Local::new(scope, function);
    realm.listeners(sig).iter().any(|existing| {
      v8::Local::new(scope, existing).strict_equals(current.into())
    })
  })
}

fn call_listener(
  scope: &mut v8::PinScope,
  function: &v8::Global<v8::Function>,
) {
  v8::tc_scope!(let tc, scope);
  let function = v8::Local::new(tc, function);
  let receiver = v8::undefined(tc);
  let failed = function.call(tc, receiver.into(), &[]).is_none();
  if !failed {
    return;
  }
  let thrown = tc.exception().map(|value| v8::Global::new(tc, value));
  tc.reset();
  if crate::host::with(|state| state.in_test) {
    let text = thrown.as_ref().map(|value| {
      let value = v8::Local::new(tc, value);
      crate::loader::modules::exception_stack(tc, value)
        .unwrap_or_else(|| "signal listener threw".to_string())
    });
    crate::host::with_mut(|state| {
      state.test_error =
        Some(text.unwrap_or_else(|| "signal listener threw".to_string()));
    });
  } else if let Some(thrown) = thrown {
    let thrown = v8::Local::new(tc, thrown);
    report_uncaught(tc, thrown);
  }
}

fn report_uncaught(scope: &mut v8::PinScope, error: v8::Local<v8::Value>) {
  let global = crate::js::global(scope);
  let Some(key) = v8::String::new(scope, "reportError") else {
    return;
  };
  let Some(report) = global
    .get(scope, key.into())
    .and_then(|value| v8::Local::<v8::Function>::try_from(value).ok())
  else {
    return;
  };
  let _ = report.call(scope, global.into(), &[error]);
}

fn add_listener(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut _ret: v8::ReturnValue,
) {
  if args.length() < 2 {
    crate::js::throw_type(scope, ARITY);
    return;
  }
  let Some(sig) = read_signal(scope, args.get(0)) else {
    return;
  };
  let Some(function) = read_listener(scope, args.get(1)) else {
    return;
  };
  let duplicate = LOCAL.with(|slot| {
    let local = slot.borrow();
    let Some(realm) = local.as_ref() else {
      return false;
    };
    realm.listeners(sig).iter().any(|existing| {
      v8::Local::new(scope, existing).strict_equals(function.into())
    })
  });
  if duplicate {
    return;
  }
  if os::retain(sig).is_err() {
    throw_error(scope, CAUGHT_FAIL);
    return;
  }
  let global = v8::Global::new(scope, function);
  let stored = LOCAL.with(|slot| {
    let mut local = slot.borrow_mut();
    let Some(realm) = local.as_mut() else {
      return false;
    };
    realm.listeners_mut(sig).push(global);
    realm.slot.increment(sig);
    true
  });
  if !stored {
    os::release(sig, 1);
    throw_error(scope, CAUGHT_FAIL);
  }
}

fn remove_listener(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut _ret: v8::ReturnValue,
) {
  if args.length() < 2 {
    crate::js::throw_type(scope, ARITY);
    return;
  }
  let Some(sig) = read_signal(scope, args.get(0)) else {
    return;
  };
  let Some(function) = read_listener(scope, args.get(1)) else {
    return;
  };
  let removed = LOCAL.with(|slot| {
    let mut local = slot.borrow_mut();
    let Some(realm) = local.as_mut() else {
      return false;
    };
    let list = realm.listeners_mut(sig);
    let Some(index) = list.iter().position(|existing| {
      v8::Local::new(scope, existing).strict_equals(function.into())
    }) else {
      return false;
    };
    list.remove(index);
    realm.slot.decrement(sig);
    true
  });
  if removed {
    os::release(sig, 1);
  }
}

fn raise_signal(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut _ret: v8::ReturnValue,
) {
  if args.length() < 1 {
    crate::js::throw_type(scope, ARITY);
    return;
  }
  let Some(sig) = read_signal(scope, args.get(0)) else {
    return;
  };
  let generation = LOCAL.with(|slot| {
    let local = slot.borrow();
    let realm = local.as_ref()?;
    if realm.slot.count(sig) == 0 {
      return None;
    }
    Some(realm.slot.generation.load(Ordering::Acquire))
  });
  let Some(generation) = generation else {
    throw_error(scope, NOT_CAUGHT);
    return;
  };
  if os::raise(sig).is_err() {
    throw_error(scope, NOT_DELIVERED);
    return;
  }
  let start = Instant::now();
  loop {
    let current = LOCAL.with(|slot| {
      slot
        .borrow()
        .as_ref()
        .map(|realm| realm.slot.generation.load(Ordering::Acquire))
    });
    if current != Some(generation) {
      return;
    }
    if start.elapsed() > DELIVER_WAIT {
      throw_error(scope, NOT_DELIVERED);
      return;
    }
    std::thread::yield_now();
  }
}

fn read_signal(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Option<Sig> {
  if !value.is_string() {
    crate::js::throw_type(scope, SIGNAL_TYPE);
    return None;
  }
  let Ok(text) = v8::Local::<v8::String>::try_from(value) else {
    crate::js::throw_type(scope, SIGNAL_TYPE);
    return None;
  };
  match crate::posix_signal::listen(text.to_rust_string_lossy(scope).as_str()) {
    crate::posix_signal::Listen::Index(index) => Some(Sig(index as u8)),
    crate::posix_signal::Listen::Uncatchable => {
      crate::js::throw_type(scope, CANNOT_CATCH);
      None
    }
    #[cfg(windows)]
    crate::posix_signal::Listen::Platform => {
      let error =
        crate::errors::construct(scope, "NotSupported", WINDOWS_SIGNAL);
      scope.throw_exception(error);
      None
    }
    crate::posix_signal::Listen::Unknown => {
      crate::js::throw_type(scope, NOT_SUPPORTED);
      None
    }
  }
}

fn read_listener<'a>(
  scope: &mut v8::PinScope,
  value: v8::Local<'a, v8::Value>,
) -> Option<v8::Local<'a, v8::Function>> {
  match v8::Local::<v8::Function>::try_from(value) {
    Ok(function) => Some(function),
    Err(_) => {
      crate::js::throw_type(scope, LISTENER);
      None
    }
  }
}

fn throw_error(scope: &mut v8::PinScope, message: &str) {
  let Some(text) = v8::String::new(scope, message) else {
    return;
  };
  let error = v8::Exception::error(scope, text);
  scope.throw_exception(error);
}

fn define_fn(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
  length: i32,
  callback: impl v8::MapFnTo<v8::FunctionCallback>,
) {
  let function = v8::Function::builder(callback)
    .length(length)
    .constructor_behavior(v8::ConstructorBehavior::Throw)
    .build(scope)
    .unwrap_or_else(|| panic!("create function {name}"));
  let key = v8::String::new(scope, name).expect(name);
  function.set_name(key);
  object.set(scope, key.into(), function.into());
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
  mutex.lock().unwrap_or_else(|error| error.into_inner())
}

#[cfg(unix)]
mod os {
  use std::sync::Mutex;
  use std::sync::atomic::{AtomicI32, Ordering};
  use std::time::Duration;

  use super::{Sig, lock};

  struct Hooks {
    counts: [usize; crate::posix_signal::LISTEN_COUNT],
    saved: [Option<libc::sigaction>; crate::posix_signal::LISTEN_COUNT],
    started: bool,
  }

  static HOOKS: Mutex<Hooks> = Mutex::new(Hooks {
    counts: [0; crate::posix_signal::LISTEN_COUNT],
    saved: [None; crate::posix_signal::LISTEN_COUNT],
    started: false,
  });
  static WRITE_FD: AtomicI32 = AtomicI32::new(-1);
  static READ_FD: AtomicI32 = AtomicI32::new(-1);

  pub(super) fn retain(sig: Sig) -> Result<(), ()> {
    let mut hooks = lock(&HOOKS);
    if !hooks.started {
      start_watcher(&mut hooks)?;
    }
    let index = sig.index();
    if hooks.counts[index] == 0 {
      install(sig, &mut hooks)?;
    }
    hooks.counts[index] += 1;
    Ok(())
  }

  pub(super) fn release(sig: Sig, count: usize) {
    let mut hooks = lock(&HOOKS);
    let index = sig.index();
    hooks.counts[index] = hooks.counts[index].saturating_sub(count);
    if hooks.counts[index] == 0 {
      restore(sig, &mut hooks);
    }
  }

  pub(super) fn raise(sig: Sig) -> Result<(), ()> {
    unsafe {
      // A blocked signal would stay pending and the handler would not run
      // before `raise` returns. Unblock it on this thread so the write to
      // the pipe happens before the caller waits for delivery.
      let mut set: libc::sigset_t = std::mem::zeroed();
      if libc::sigemptyset(&mut set) != 0 {
        return Err(());
      }
      if libc::sigaddset(&mut set, raw(sig)) != 0 {
        return Err(());
      }
      libc::pthread_sigmask(libc::SIG_UNBLOCK, &set, std::ptr::null_mut());
      if libc::raise(raw(sig)) == 0 {
        Ok(())
      } else {
        Err(())
      }
    }
  }

  fn raw(sig: Sig) -> libc::c_int {
    crate::posix_signal::raw_number(sig.index())
  }

  /// Records one byte and returns. `write` is async-signal-safe; V8 and
  /// mutexes are not.
  extern "C" fn handle(sig: libc::c_int) {
    let fd = WRITE_FD.load(Ordering::Relaxed);
    if fd < 0 || !(0..=255).contains(&sig) {
      return;
    }
    let byte = sig as u8;
    unsafe {
      libc::write(fd, &byte as *const u8 as *const libc::c_void, 1);
    }
  }

  fn start_watcher(hooks: &mut Hooks) -> Result<(), ()> {
    let mut fds = [0; 2];
    if unsafe { libc::pipe(fds.as_mut_ptr()) } != 0 {
      return Err(());
    }
    unsafe {
      cloexec(fds[0]);
      cloexec(fds[1]);
      nonblock(fds[1]);
    }
    WRITE_FD.store(fds[1], Ordering::Relaxed);
    READ_FD.store(fds[0], Ordering::Relaxed);
    let spawned = std::thread::Builder::new()
      .name("yuzuko-signal".to_string())
      .spawn(watch);
    if spawned.is_err() {
      unsafe {
        libc::close(fds[0]);
        libc::close(fds[1]);
      }
      WRITE_FD.store(-1, Ordering::Relaxed);
      READ_FD.store(-1, Ordering::Relaxed);
      return Err(());
    }
    hooks.started = true;
    Ok(())
  }

  fn watch() {
    let fd = READ_FD.load(Ordering::Relaxed);
    let mut buf = [0u8; 64];
    loop {
      let read = unsafe {
        libc::read(fd, buf.as_mut_ptr().cast::<libc::c_void>(), buf.len())
      };
      if read < 0 {
        let interrupted =
          std::io::Error::last_os_error().raw_os_error() == Some(libc::EINTR);
        if !interrupted {
          std::thread::sleep(Duration::from_millis(10));
        }
        continue;
      }
      if read == 0 {
        std::thread::sleep(Duration::from_millis(10));
        continue;
      }
      for byte in &buf[..read as usize] {
        if let Some(sig) = from_byte(*byte) {
          super::publish(sig);
        }
      }
    }
  }

  fn from_byte(byte: u8) -> Option<Sig> {
    let number = libc::c_int::from(byte);
    (0..crate::posix_signal::LISTEN_COUNT)
      .find(|&index| crate::posix_signal::raw_number(index) == number)
      .map(|index| Sig(index as u8))
  }

  fn install(sig: Sig, hooks: &mut Hooks) -> Result<(), ()> {
    unsafe {
      let mut action: libc::sigaction = std::mem::zeroed();
      action.sa_sigaction = handle as *const () as libc::sighandler_t;
      action.sa_flags = libc::SA_RESTART;
      if libc::sigemptyset(&mut action.sa_mask) != 0 {
        return Err(());
      }
      let mut old: libc::sigaction = std::mem::zeroed();
      if libc::sigaction(raw(sig), &action, &mut old) != 0 {
        return Err(());
      }
      hooks.saved[sig.index()] = Some(old);
    }
    Ok(())
  }

  fn restore(sig: Sig, hooks: &mut Hooks) {
    let Some(old) = hooks.saved[sig.index()].take() else {
      return;
    };
    unsafe {
      libc::sigaction(raw(sig), &old, std::ptr::null_mut());
    }
  }

  unsafe fn cloexec(fd: libc::c_int) {
    unsafe {
      libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC);
    }
  }

  unsafe fn nonblock(fd: libc::c_int) {
    unsafe {
      let flags = libc::fcntl(fd, libc::F_GETFL);
      if flags >= 0 {
        libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK);
      }
    }
  }

  #[cfg(test)]
  fn current_handler(sig: libc::c_int) -> libc::sighandler_t {
    let mut action: libc::sigaction = unsafe { std::mem::zeroed() };
    let rc = unsafe { libc::sigaction(sig, std::ptr::null(), &mut action) };
    assert_eq!(rc, 0, "read handler");
    action.sa_sigaction
  }

  #[cfg(test)]
  mod tests {
    use std::sync::Arc;
    use std::sync::atomic::Ordering;
    use std::time::{Duration, Instant};

    use super::super::{RealmSlot, Sig, register_slot, unregister_slot};
    use super::{current_handler, raise, release, retain};
    use crate::platform::TaskQueue;

    struct Cleanup {
      first: Arc<RealmSlot>,
      second: Arc<RealmSlot>,
    }

    impl Drop for Cleanup {
      fn drop(&mut self) {
        for slot in [&self.first, &self.second] {
          for count in &slot.counts {
            count.store(0, Ordering::Release);
          }
        }
        for index in 0..crate::posix_signal::LISTEN_COUNT {
          release(Sig(index as u8), usize::MAX);
        }
        unregister_slot(&self.first);
        unregister_slot(&self.second);
      }
    }

    fn slot() -> Arc<RealmSlot> {
      Arc::new(RealmSlot {
        pending: std::sync::Mutex::new(std::collections::VecDeque::new()),
        generation: std::sync::atomic::AtomicU64::new(0),
        counts: std::array::from_fn(|_| std::sync::atomic::AtomicUsize::new(0)),
        queue: TaskQueue::new(),
      })
    }

    fn wait_past(slot: &RealmSlot, seen: u64) {
      let start = Instant::now();
      while slot.generation.load(Ordering::Acquire) == seen {
        assert!(
          start.elapsed() < Duration::from_secs(2),
          "signal was not queued"
        );
        std::thread::yield_now();
      }
    }

    fn drain(slot: &RealmSlot) -> Vec<Sig> {
      super::super::lock(&slot.pending).drain(..).collect()
    }

    #[test]
    fn caught_signals_are_queued_and_the_handler_is_restored() {
      let before_int = current_handler(libc::SIGINT);
      let before_term = current_handler(libc::SIGTERM);
      let kept = Cleanup {
        first: slot(),
        second: slot(),
      };
      register_slot(&kept.first);
      register_slot(&kept.second);

      let sigint = Sig(1);
      let sigterm = Sig(5);
      retain(sigint).expect("catch SIGINT");
      retain(sigint).expect("second SIGINT listener");
      kept.first.counts[sigint.index()].store(1, Ordering::Release);
      kept.second.counts[sigint.index()].store(1, Ordering::Release);
      raise(sigint).expect("raise SIGINT");
      wait_past(&kept.first, 0);
      wait_past(&kept.second, 0);
      assert_eq!(drain(&kept.first), vec![sigint]);
      assert_eq!(drain(&kept.second), vec![sigint]);

      release(sigint, 1);
      raise(sigint).expect("raise SIGINT again");
      wait_past(&kept.second, 1);
      assert_eq!(drain(&kept.first), vec![sigint]);
      assert_eq!(drain(&kept.second), vec![sigint]);

      kept.first.counts[sigint.index()].store(0, Ordering::Release);
      kept.second.counts[sigint.index()].store(0, Ordering::Release);
      release(sigint, 1);
      assert_eq!(current_handler(libc::SIGINT), before_int);

      retain(sigterm).expect("catch SIGTERM");
      kept.first.counts[sigterm.index()].store(1, Ordering::Release);
      let seen = kept.first.generation.load(Ordering::Acquire);
      raise(sigterm).expect("raise SIGTERM");
      wait_past(&kept.first, seen);
      assert_eq!(drain(&kept.first), vec![sigterm]);
      assert!(drain(&kept.second).is_empty());
      kept.first.counts[sigterm.index()].store(0, Ordering::Release);
      release(sigterm, 1);
      assert_eq!(current_handler(libc::SIGTERM), before_term);

      let sighup = Sig(0);
      let before_hup = current_handler(libc::SIGHUP);
      retain(sighup).expect("catch SIGHUP");
      kept.first.counts[sighup.index()].store(1, Ordering::Release);
      let seen = kept.first.generation.load(Ordering::Acquire);
      raise(sighup).expect("raise SIGHUP");
      wait_past(&kept.first, seen);
      assert_eq!(drain(&kept.first), vec![sighup]);
      kept.first.counts[sighup.index()].store(0, Ordering::Release);
      release(sighup, 1);
      assert_eq!(current_handler(libc::SIGHUP), before_hup);
    }
  }
}

#[cfg(windows)]
mod os {
  use std::sync::Mutex;

  use super::{Sig, lock};
  use windows_sys::Win32::System::Console::{
    CTRL_BREAK_EVENT, CTRL_C_EVENT, GenerateConsoleCtrlEvent,
    SetConsoleCtrlHandler,
  };

  struct Hooks {
    counts: [usize; crate::posix_signal::LISTEN_COUNT],
    installed: bool,
  }

  static HOOKS: Mutex<Hooks> = Mutex::new(Hooks {
    counts: [0; crate::posix_signal::LISTEN_COUNT],
    installed: false,
  });

  pub(super) fn retain(sig: Sig) -> Result<(), ()> {
    let mut hooks = lock(&HOOKS);
    if !hooks.installed {
      let ok = unsafe { SetConsoleCtrlHandler(Some(handle), 1) };
      if ok == 0 {
        return Err(());
      }
      hooks.installed = true;
    }
    hooks.counts[sig.index()] += 1;
    Ok(())
  }

  pub(super) fn release(sig: Sig, count: usize) {
    let mut hooks = lock(&HOOKS);
    let index = sig.index();
    hooks.counts[index] = hooks.counts[index].saturating_sub(count);
    if hooks.installed && hooks.counts.iter().all(|count| *count == 0) {
      unsafe {
        SetConsoleCtrlHandler(Some(handle), 0);
      }
      hooks.installed = false;
    }
  }

  pub(super) fn raise(sig: Sig) -> Result<(), ()> {
    let event = match sig.0 {
      0 => CTRL_C_EVENT,
      1 => CTRL_BREAK_EVENT,
      _ => return Err(()),
    };
    let ok = unsafe { GenerateConsoleCtrlEvent(event, 0) };
    if ok == 0 { Err(()) } else { Ok(()) }
  }

  /// Console handlers run on a normal thread, unlike a Unix signal handler.
  unsafe extern "system" fn handle(ctrl: u32) -> i32 {
    let sig = match ctrl {
      CTRL_C_EVENT => Sig(0),
      CTRL_BREAK_EVENT => Sig(1),
      _ => return 0,
    };
    if !listening(sig) {
      return 0;
    }
    super::publish(sig);
    1
  }

  fn listening(sig: Sig) -> bool {
    let hooks = lock(&HOOKS);
    hooks.counts[sig.index()] > 0
  }
}

#[cfg(not(any(unix, windows)))]
mod os {
  use super::Sig;

  pub(super) fn retain(_sig: Sig) -> Result<(), ()> {
    Err(())
  }

  pub(super) fn release(_sig: Sig, _count: usize) {}

  pub(super) fn raise(_sig: Sig) -> Result<(), ()> {
    Err(())
  }
}
