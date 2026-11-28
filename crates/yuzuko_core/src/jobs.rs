//! Background work whose result runs on the main thread.
//!
//! Decoding and fetch must not call V8. The event loop drains each result
//! as a task and keeps running while a job is in flight. [`spawn`] shares
//! one process-wide pool. A job waits when every pool thread is busy.

use std::any::Any;
use std::cell::Cell;
use std::cell::RefCell;
use std::collections::HashMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
#[cfg(test)]
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, mpsc};

use crate::host;

/// Process-wide pool size. `docs/spec.md` states this number.
const POOL_SIZE: usize = 8;

type Work = Box<dyn FnOnce() + Send>;

static POOL: OnceLock<mpsc::Sender<Work>> = OnceLock::new();

#[cfg(test)]
static IN_FLIGHT: AtomicUsize = AtomicUsize::new(0);
#[cfg(test)]
static PEAK_IN_FLIGHT: AtomicUsize = AtomicUsize::new(0);

type JobCallback = Box<dyn FnOnce(Box<dyn Any + Send>)>;

thread_local! {
  static CALLBACKS: RefCell<HashMap<u64, JobCallback>> =
    RefCell::new(HashMap::new());
  /// The scope `poll` is running. Callbacks borrow it and must not store it.
  static ACTIVE: Cell<*mut ()> = const { Cell::new(std::ptr::null_mut()) };
}

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// Runs `work` on the host pool. `finish` runs later on the realm thread.
///
/// The pool has [`POOL_SIZE`] threads for the process. A job waits while
/// every thread is busy. A panic in `work` becomes `Err(())`. So does a job
/// the pool cannot accept; `work` then does not run.
pub fn spawn<T: Send + 'static>(
  work: impl FnOnce() -> T + Send + 'static,
  finish: impl FnOnce(&mut v8::PinScope, Result<T, ()>) + 'static,
) {
  let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
  CALLBACKS.with(|callbacks| {
    callbacks.borrow_mut().insert(
      id,
      Box::new(move |value| {
        let Ok(value) = value.downcast::<Result<T, ()>>() else {
          return;
        };
        with_active(|scope| finish(scope, *value));
      }),
    );
  });
  let queue = host::queue();
  queue.job_started();
  let worker = queue.clone();
  let queued = dispatch(move || {
    let value = catch_unwind(AssertUnwindSafe(work)).map_err(|_| ());
    worker.job_finished(id, Box::new(value));
  });
  if !queued {
    queue.job_finished(id, Box::new(Err::<T, ()>(())));
  }
}

/// Runs `work` like [`spawn`] and returns a promise for its result.
///
/// On the main thread `apply` turns `data` and a success into the
/// fulfillment value. A failure from `work` or `apply` rejects with
/// `reject`. A panic in `work` fails with `panicked`. A promise that cannot
/// be created is a `TypeError` with `no_promise`.
#[cfg(any(feature = "tcp", feature = "udp", feature = "http"))]
pub(crate) fn settle<T: Send + 'static, F: Send + 'static, D: 'static>(
  scope: &mut v8::PinScope,
  no_promise: &'static str,
  panicked: F,
  work: impl FnOnce() -> Result<T, F> + Send + 'static,
  data: D,
  apply: for<'s> fn(
    &mut v8::PinScope<'s, '_>,
    D,
    T,
  ) -> Result<v8::Local<'s, v8::Value>, F>,
  reject: for<'s> fn(&mut v8::PinScope<'s, '_>, &F) -> v8::Local<'s, v8::Value>,
) -> Result<crate::op::Js, crate::op::OpError> {
  let Some(resolver) = v8::PromiseResolver::new(scope) else {
    return Err(crate::op::OpError::type_(no_promise));
  };
  let promise = resolver.get_promise(scope);
  let promise_value: v8::Local<v8::Value> = promise.into();
  let promise_global = v8::Global::new(scope, promise_value);
  let resolver_global = v8::Global::new(scope, resolver);
  spawn(work, move |scope, result| {
    let resolver = v8::Local::new(scope, &resolver_global);
    let outcome = match result {
      Ok(value) => value,
      Err(()) => Err(panicked),
    };
    let mapped = match outcome {
      Ok(value) => apply(scope, data, value),
      Err(fail) => Err(fail),
    };
    match mapped {
      Ok(value) => {
        let _ = resolver.resolve(scope, value);
      }
      Err(fail) => {
        let error = reject(scope, &fail);
        let _ = resolver.reject(scope, error);
      }
    }
  });
  Ok(crate::op::Js(promise_global))
}

/// Registers `finish` and keeps the event loop awake until `notify`.
///
/// The worker that produces the result calls `notify` from any thread.
/// Dropping the handle without `notify` still delivers `finish`, so a body
/// pump cannot leave the event loop waiting.
pub fn listen(finish: impl FnOnce(&mut v8::PinScope) + 'static) -> Listen {
  let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
  CALLBACKS.with(|callbacks| {
    callbacks.borrow_mut().insert(
      id,
      Box::new(move |_value| {
        with_active(finish);
      }),
    );
  });
  let queue = host::queue();
  queue.job_started();
  Listen {
    id,
    queue,
    live: true,
  }
}

pub struct Listen {
  id: u64,
  queue: std::sync::Arc<crate::platform::TaskQueue>,
  live: bool,
}

impl Listen {
  pub fn notify(mut self) {
    self.complete();
  }

  fn complete(&mut self) {
    if self.live {
      self.live = false;
      self.queue.job_finished(self.id, Box::new(()));
    }
  }
}

impl Drop for Listen {
  fn drop(&mut self) {
    self.complete();
  }
}

/// Queues `finish` as a task. No background thread runs.
#[inline]
pub fn enqueue(finish: impl FnOnce(&mut v8::PinScope) + 'static) {
  let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
  CALLBACKS.with(|callbacks| {
    callbacks.borrow_mut().insert(
      id,
      Box::new(move |_value| {
        with_active(finish);
      }),
    );
  });
  host::queue().job_started();
  host::queue().job_finished(id, Box::new(()));
}

/// Drops callbacks for a realm that is going away. Late results are ignored.
pub fn clear() {
  CALLBACKS.with(|callbacks| callbacks.borrow_mut().clear());
}

/// Runs one completed job. Returns whether one was waiting.
#[inline]
pub fn poll(scope: &mut v8::PinScope) -> bool {
  let Some((id, value)) = host::queue().pop_job() else {
    return false;
  };
  let callback = CALLBACKS.with(|callbacks| callbacks.borrow_mut().remove(&id));
  if let Some(callback) = callback {
    let ptr = scope as *mut v8::PinScope as *mut ();
    let previous = ACTIVE.with(|slot| slot.replace(ptr));
    callback(value);
    ACTIVE.with(|slot| slot.set(previous));
  }
  true
}

fn with_active(finish: impl FnOnce(&mut v8::PinScope)) {
  ACTIVE.with(|slot| {
    let ptr = slot.get();
    assert!(!ptr.is_null(), "job callback outside the event loop");
    // Safety: `poll` set this to the scope it is currently borrowing, and
    // restores the previous pointer before it returns. The callback must not
    // keep a reference to the scope.
    let scope = unsafe { &mut *ptr.cast::<v8::PinScope>() };
    finish(scope);
  });
}

pub fn pending() -> bool {
  host::queue().jobs_pending()
}

fn pool() -> &'static mpsc::Sender<Work> {
  POOL.get_or_init(|| {
    let (sender, receiver) = mpsc::channel();
    let receiver = Arc::new(Mutex::new(receiver));
    let mut started = 0;
    for index in 0..POOL_SIZE {
      let receiver = Arc::clone(&receiver);
      if let Ok(handle) = std::thread::Builder::new()
        .name(format!("yuzuko-job-{index}"))
        .spawn(move || run_worker(receiver))
      {
        started += 1;
        // Detach. The worker runs until the process exits.
        drop(handle);
      }
    }
    // When no worker started, this is the last owner. Dropping it
    // disconnects `sender`, and `spawn` rejects the job.
    if started == 0 {
      drop(receiver);
    }
    sender
  })
}

fn dispatch(work: impl FnOnce() + Send + 'static) -> bool {
  pool().send(Box::new(work)).is_ok()
}

fn run_worker(inbox: Arc<Mutex<mpsc::Receiver<Work>>>) {
  loop {
    let work = {
      // `Receiver` is not `Sync`. The lock covers only the receive, so the
      // workers run jobs at the same time.
      let inbox = inbox.lock().unwrap_or_else(|error| error.into_inner());
      match inbox.recv() {
        Ok(work) => work,
        Err(_) => return,
      }
    };
    #[cfg(test)]
    let _running = JobRunning::enter();
    // `spawn` catches a panic inside the host call and still finishes the
    // job. This catch keeps the worker alive when that finish panics.
    let _ = catch_unwind(AssertUnwindSafe(work));
  }
}

#[cfg(test)]
struct JobRunning;

#[cfg(test)]
impl JobRunning {
  fn enter() -> Self {
    let now = IN_FLIGHT.fetch_add(1, Ordering::AcqRel) + 1;
    let _ = PEAK_IN_FLIGHT.fetch_max(now, Ordering::AcqRel);
    Self
  }
}

#[cfg(test)]
impl Drop for JobRunning {
  fn drop(&mut self) {
    IN_FLIGHT.fetch_sub(1, Ordering::AcqRel);
  }
}

#[cfg(test)]
fn peak_in_flight() -> usize {
  PEAK_IN_FLIGHT.load(Ordering::Acquire)
}

#[cfg(test)]
mod tests {
  use std::sync::atomic::{AtomicUsize, Ordering};
  use std::sync::{Arc, Condvar, Mutex};
  use std::time::{Duration, Instant};

  use super::{POOL_SIZE, dispatch, peak_in_flight};

  #[test]
  fn extra_jobs_wait_for_a_pool_thread() {
    let entered = Arc::new((Mutex::new(0_usize), Condvar::new()));
    let gate = Arc::new((Mutex::new(false), Condvar::new()));
    let finished = Arc::new(AtomicUsize::new(0));
    let total = POOL_SIZE + 4;
    for _ in 0..total {
      let entered = Arc::clone(&entered);
      let gate = Arc::clone(&gate);
      let finished = Arc::clone(&finished);
      assert!(dispatch(move || {
        {
          let mut count = entered.0.lock().expect("entered");
          *count += 1;
          entered.1.notify_all();
        }
        let mut open = gate.0.lock().expect("gate");
        while !*open {
          open = gate.1.wait(open).expect("gate");
        }
        finished.fetch_add(1, Ordering::Release);
        entered.1.notify_all();
      }));
    }

    let mut count = entered.0.lock().expect("entered");
    let start = Instant::now();
    while *count < POOL_SIZE {
      let (next, status) = entered
        .1
        .wait_timeout(count, Duration::from_secs(2))
        .expect("entered");
      count = next;
      assert!(!status.timed_out(), "pool threads did not start");
      assert!(start.elapsed() < Duration::from_secs(5));
    }
    let settle = Instant::now();
    while settle.elapsed() < Duration::from_millis(80) {
      let Some(remaining) =
        Duration::from_millis(80).checked_sub(settle.elapsed())
      else {
        break;
      };
      let (next, status) =
        entered.1.wait_timeout(count, remaining).expect("entered");
      count = next;
      if !status.timed_out() {
        assert_eq!(*count, POOL_SIZE, "a job ran without a free pool thread");
      }
    }
    assert_eq!(*count, POOL_SIZE);
    assert_eq!(peak_in_flight(), POOL_SIZE);
    drop(count);

    {
      let mut open = gate.0.lock().expect("gate");
      *open = true;
      gate.1.notify_all();
    }
    let mut count = entered.0.lock().expect("entered");
    let start = Instant::now();
    while finished.load(Ordering::Acquire) < total {
      let (next, status) = entered
        .1
        .wait_timeout(count, Duration::from_secs(2))
        .expect("entered");
      count = next;
      assert!(!status.timed_out(), "queued jobs did not run");
      assert!(start.elapsed() < Duration::from_secs(5));
    }
    drop(count);
    assert_eq!(finished.load(Ordering::Acquire), total);
    assert_eq!(peak_in_flight(), POOL_SIZE);

    let started = Arc::new(AtomicUsize::new(0));
    for _ in 0..POOL_SIZE {
      let started = Arc::clone(&started);
      assert!(dispatch(move || {
        started.fetch_add(1, Ordering::Release);
        panic!("host job");
      }));
    }
    let start = Instant::now();
    while started.load(Ordering::Acquire) < POOL_SIZE {
      assert!(
        start.elapsed() < Duration::from_secs(2),
        "panicking jobs did not start"
      );
      std::thread::sleep(Duration::from_millis(5));
    }
    let followed = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flag = Arc::clone(&followed);
    assert!(dispatch(move || {
      flag.store(true, Ordering::Release);
    }));
    let start = Instant::now();
    while !followed.load(Ordering::Acquire) {
      assert!(
        start.elapsed() < Duration::from_secs(2),
        "pool stopped after a panic"
      );
      std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(peak_in_flight(), POOL_SIZE);
  }
}
