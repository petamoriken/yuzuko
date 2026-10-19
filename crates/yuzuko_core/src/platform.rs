//! V8 platform that delivers foreground tasks to the runtime event loop.

use std::any::Any;
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, LazyLock, Mutex};
use std::time::{Duration, Instant};

use v8::PlatformImpl;

struct ReadyQueue {
  tasks: VecDeque<v8::Task>,
  /// Set by [`TaskQueue::poke`] under this mutex so a waiter cannot miss it.
  poked: bool,
}

/// Queue of V8 foreground tasks. Posts from any thread wake the event loop.
pub struct TaskQueue {
  ready: Mutex<ReadyQueue>,
  delayed: Mutex<Vec<(Instant, v8::Task)>>,
  cvar: Condvar,
  waits: AtomicUsize,
  /// Wakes a host event loop that is blocked outside this condvar.
  waker: Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
  /// Results from background work, drained on the main thread.
  jobs: Mutex<VecDeque<(u64, Box<dyn Any + Send>)>>,
  /// Background jobs that have not finished yet.
  job_count: AtomicUsize,
}

impl TaskQueue {
  pub fn new() -> Arc<Self> {
    Arc::new(Self {
      ready: Mutex::new(ReadyQueue {
        tasks: VecDeque::new(),
        poked: false,
      }),
      delayed: Mutex::new(Vec::new()),
      cvar: Condvar::new(),
      waits: AtomicUsize::new(0),
      waker: Mutex::new(None),
      jobs: Mutex::new(VecDeque::new()),
      job_count: AtomicUsize::new(0),
    })
  }

  pub fn job_started(&self) {
    self.job_count.fetch_add(1, Ordering::SeqCst);
  }

  /// Queues `value` for the main thread and wakes the event loop.
  ///
  /// The ready-queue lock is held across the notify so a waiter cannot miss
  /// the wake between its emptiness check and `wait`.
  pub fn job_finished(&self, id: u64, value: Box<dyn Any + Send>) {
    {
      let _ready = self.ready.lock().expect("task queue");
      self.jobs.lock().expect("job queue").push_back((id, value));
      self.job_count.fetch_sub(1, Ordering::SeqCst);
      self.cvar.notify_one();
    }
    self.wake_host();
  }

  pub fn pop_job(&self) -> Option<(u64, Box<dyn Any + Send>)> {
    self.jobs.lock().expect("job queue").pop_front()
  }

  pub fn jobs_pending(&self) -> bool {
    self.job_count.load(Ordering::SeqCst) > 0
      || !self.jobs.lock().expect("job queue").is_empty()
  }

  pub fn set_waker(&self, waker: Option<Arc<dyn Fn() + Send + Sync>>) {
    *self.waker.lock().expect("task waker") = waker;
  }

  fn notify(&self) {
    self.cvar.notify_one();
    self.wake_host();
  }

  fn wake_host(&self) {
    let waker = self.waker.lock().expect("task waker").clone();
    if let Some(waker) = waker {
      waker();
    }
  }

  fn job_is_ready(&self) -> bool {
    !self.jobs.lock().expect("job queue").is_empty()
  }

  pub fn push(&self, task: v8::Task) {
    self.ready.lock().expect("task queue").tasks.push_back(task);
    self.notify();
  }

  /// Wakes a waiter that is blocked in [`Self::wait`] without posting a task.
  ///
  /// The flag is stored under the same mutex as the wait condition. A poke
  /// that arrives before `wait` must still be observed, or a worker message
  /// sits unread until the caller's deadline.
  pub fn poke(&self) {
    {
      let mut ready = self.ready.lock().expect("task queue");
      ready.poked = true;
      self.cvar.notify_one();
    }
    self.wake_host();
  }

  /// Returns whether [`Self::poke`] ran since the previous poll, and clears it.
  pub fn poll_poke(&self) -> bool {
    let mut ready = self.ready.lock().expect("task queue");
    let poked = ready.poked;
    ready.poked = false;
    poked
  }

  pub fn push_delayed(&self, task: v8::Task, delay_secs: f64) {
    let when = Instant::now() + Duration::from_secs_f64(delay_secs.max(0.0));
    self
      .delayed
      .lock()
      .expect("delayed task queue")
      .push((when, task));
    self.notify();
  }

  pub fn pop_ready(&self) -> Option<v8::Task> {
    if let Some(task) = self.ready.lock().expect("task queue").tasks.pop_front()
    {
      return Some(task);
    }
    let mut delayed = self.delayed.lock().expect("delayed task queue");
    let now = Instant::now();
    let pos = delayed
      .iter()
      .enumerate()
      .filter(|(_, (when, _))| *when <= now)
      .min_by_key(|(_, (when, _))| *when)
      .map(|(index, _)| index)?;
    Some(delayed.swap_remove(pos).1)
  }

  /// Times the event loop blocked in [`Self::wait`] waiting for a task.
  #[cfg_attr(not(test), allow(dead_code))]
  pub fn wait_count(&self) -> usize {
    self.waits.load(Ordering::SeqCst)
  }

  pub fn pending_delayed(&self) -> usize {
    self.delayed.lock().expect("delayed task queue").len()
  }

  pub fn next_wait(&self) -> Option<Duration> {
    if !self.ready.lock().expect("task queue").tasks.is_empty() {
      return Some(Duration::ZERO);
    }
    let delayed = self.delayed.lock().expect("delayed task queue");
    delayed
      .iter()
      .map(|(when, _)| when.saturating_duration_since(Instant::now()))
      .min()
  }

  pub fn is_idle(&self) -> bool {
    self.ready.lock().expect("task queue").tasks.is_empty()
      && self.delayed.lock().expect("delayed task queue").is_empty()
  }

  pub fn wait(&self, timeout: Option<Duration>) {
    // `wait_timeout` on macOS often wakes several milliseconds late. A 1 ms
    // timer then misses the web-platform test that expects a sub-4 ms delay.
    if let Some(timeout) = timeout
      && timeout <= Duration::from_millis(5)
    {
      let start = Instant::now();
      while start.elapsed() < timeout {
        let mut ready = self.ready.lock().expect("task queue");
        if !ready.tasks.is_empty() || ready.poked || self.job_is_ready() {
          ready.poked = false;
          return;
        }
        drop(ready);
        // Yielding here gives the scheduler a chance to delay a 1 ms timer
        // past the 4 ms web-platform bound. Stay on the core instead.
        std::hint::spin_loop();
      }
      return;
    }
    let mut ready = self.ready.lock().expect("task queue");
    if !ready.tasks.is_empty() || ready.poked || self.job_is_ready() {
      ready.poked = false;
      return;
    }
    self.waits.fetch_add(1, Ordering::SeqCst);
    let mut ready = match timeout {
      Some(duration) => {
        self
          .cvar
          .wait_timeout(ready, duration)
          .expect("task queue wait")
          .0
      }
      None => self.cvar.wait(ready).expect("task queue wait"),
    };
    ready.poked = false;
  }
}

pub struct HostPlatform {
  pub queue: Arc<TaskQueue>,
}

static ISOLATE_QUEUES: LazyLock<Mutex<HashMap<usize, Arc<TaskQueue>>>> =
  LazyLock::new(|| Mutex::new(HashMap::new()));

/// Routes V8 foreground tasks for `isolate` onto `queue`.
///
/// `token` is [`isolate_token`]. The main isolate stays on the platform
/// queue, so it does not need a binding.
pub fn bind_isolate(token: usize, queue: Arc<TaskQueue>) {
  ISOLATE_QUEUES
    .lock()
    .expect("isolate queues")
    .insert(token, queue);
}

pub fn unbind_isolate(token: usize) {
  ISOLATE_QUEUES
    .lock()
    .expect("isolate queues")
    .remove(&token);
}

/// Address of the V8 isolate. `post_task` receives the same pointer.
pub fn isolate_token(isolate: &v8::Isolate) -> usize {
  let raw = unsafe { isolate.as_raw_isolate_ptr() };
  assert_eq!(
    std::mem::size_of::<v8::UnsafeRawIsolatePtr>(),
    std::mem::size_of::<usize>()
  );
  unsafe { std::mem::transmute_copy(&raw) }
}

impl HostPlatform {
  fn queue_for(&self, isolate: *mut std::ffi::c_void) -> Arc<TaskQueue> {
    if !isolate.is_null()
      && let Some(queue) = ISOLATE_QUEUES
        .lock()
        .expect("isolate queues")
        .get(&(isolate as usize))
    {
      return Arc::clone(queue);
    }
    Arc::clone(&self.queue)
  }
}

impl PlatformImpl for HostPlatform {
  fn post_task(&self, isolate: *mut std::ffi::c_void, task: v8::Task) {
    self.queue_for(isolate).push(task);
  }

  fn post_non_nestable_task(
    &self,
    isolate: *mut std::ffi::c_void,
    task: v8::Task,
  ) {
    self.post_task(isolate, task);
  }

  fn post_delayed_task(
    &self,
    isolate: *mut std::ffi::c_void,
    task: v8::Task,
    delay_secs: f64,
  ) {
    self.queue_for(isolate).push_delayed(task, delay_secs);
  }

  fn post_non_nestable_delayed_task(
    &self,
    isolate: *mut std::ffi::c_void,
    task: v8::Task,
    delay_secs: f64,
  ) {
    self.post_delayed_task(isolate, task, delay_secs);
  }
}

#[cfg(test)]
mod tests {
  use std::time::{Duration, Instant};

  use super::TaskQueue;

  #[test]
  fn poke_before_wait_returns() {
    let queue = TaskQueue::new();
    queue.poke();
    let started = Instant::now();
    queue.wait(Some(Duration::from_secs(2)));
    assert!(started.elapsed() < Duration::from_millis(200));
  }
}
