//! Background work whose result runs on the main thread.
//!
//! Decoding and fetch must not call V8. The event loop drains each result
//! as a task and keeps running while a job is in flight.

use std::any::Any;
use std::cell::Cell;
use std::cell::RefCell;
use std::collections::HashMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::host;

type JobCallback = Box<dyn FnOnce(Box<dyn Any + Send>)>;

thread_local! {
  static CALLBACKS: RefCell<HashMap<u64, JobCallback>> =
    RefCell::new(HashMap::new());
  /// The scope `poll` is running. Callbacks borrow it and must not store it.
  static ACTIVE: Cell<*mut ()> = const { Cell::new(std::ptr::null_mut()) };
}

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// Runs `work` on a new thread. `finish` runs later on the main thread.
///
/// A panic in `work` becomes `Err(())`. So does a thread the operating
/// system refuses to create; `work` then does not run.
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
  let started = std::thread::Builder::new().spawn(move || {
    let value = catch_unwind(AssertUnwindSafe(work)).map_err(|_| ());
    worker.job_finished(id, Box::new(value));
  });
  if started.is_err() {
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
