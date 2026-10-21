//! Yuzuko runtime core: V8 isolate, module loader, and test runner.

extern crate self as yuzuko_core;

mod clone;
mod extensions;
mod host;
mod idle;
mod jobs;
mod js;
mod line;
mod loader;
pub mod op;
mod parcel;
mod platform;
mod realm;
mod repl;
mod runtime;
mod timers;
mod worker;

pub use clone::{
  ReadResult, WriteResult, add_host_clone, clear_structured_transfer,
  cross_agent, cross_target, is_structured_transfer, read_bytes, read_text,
  reject_clone, set_structured_transfer, write_bytes, write_text,
};
pub use extensions::{add_extension, on_realm_drop};
pub use host::{
  agent_id, entry_dir, entry_url, is_dedicated_worker, note_test_error,
  on_dedicated_worker, set_dom_exception, set_file_root, set_rejection_hook,
  try_agent_id,
};
pub use idle::{
  IdleHooks, idle_frame_delay, idle_has_pending, idle_next_wait,
  next_host_delay, poll_idle_period, poll_idle_timeout, set_idle_frame_delay,
  set_idle_hooks,
};
pub use jobs::{Listen, enqueue, listen, pending as jobs_pending, spawn};
pub use js::{
  global, namespace, rust_string, set_function, set_value, throw_type,
};
pub use loader::modules::{canonicalize, file_url, module_graph};
pub use parcel::{Parcel, deserialize_parcel, serialize_parcel};
pub use realm::{
  clone_value, delete_object_prototype_proto, performance_now_ms,
  time_origin_ms,
};
pub use repl::run_repl;
pub use runtime::{
  ClassicRun, LoopInterest, RuntimeError, poll_task_poke, run_classic_scripts,
  run_classic_scripts_at, run_file, run_file_sandboxed, run_scripts, run_tests,
  set_loop_hooks, set_task_waker, wait_for_tasks,
};
pub use timers::{schedule_animation_callback, schedule_callback};
pub use worker::{
  Inbound, LocationParts, ScriptKind, WorkerScript, agent_alive, drain_inbound,
  eval_classic_in_worker, keeps_alive, load_worker_source, parent_agent,
  poke_agent, post_inbound, resolve_specifier, run_worker_frame_hook,
  set_inbound_handler, set_worker_frame_hook, spawn_worker, take_inbox,
  terminate_worker, worker_is_module, worker_location, worker_name,
};

#[cfg(test)]
mod tests;
