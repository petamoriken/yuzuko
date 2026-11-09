//! Yuzuko runtime core: V8 isolate, module loader, and test runner.

extern crate self as yuzuko_core;

#[cfg(feature = "child")]
mod child;
mod clone;
mod color;
mod extensions;
#[cfg(feature = "ffi")]
mod ffi;
#[cfg(feature = "fs")]
mod fs;
mod host;
#[cfg(feature = "http")]
mod http_server;
mod idle;
mod inspect;
mod jobs;
mod js;
mod line;
mod loader;
pub mod op;
mod parcel;
mod permissions;
mod platform;
mod process;
mod reach;
mod realm;
mod repl;
mod runtime;
#[cfg(feature = "signal")]
mod signal;
#[cfg(any(feature = "tcp", feature = "udp", feature = "http"))]
mod socket_endpoint;
#[cfg(feature = "tcp")]
mod tcp;
mod timers;
#[cfg(feature = "udp")]
mod udp;
mod watch;
mod worker;

pub use clone::{
  CloneDiscardFn, CloneEnterFn, CloneFinisher, ReadResult,
  StructuredTransferGuard, TransferKind, TransferList, TransferReject,
  TransferStatus, WriteResult, add_clone_discard, add_clone_enter,
  add_clone_finisher, add_host_clone, add_host_detach, add_host_transfer,
  clear_structured_transfer, collect_transfers, cross_agent, cross_target,
  detach_structured_transfers, is_structured_transfer, read_bytes, read_text,
  reject_clone, set_structured_transfer, write_bytes, write_text,
};
pub use extensions::{
  add_extension, on_realm_drop, on_reload, on_reload_scope, on_reload_settled,
};
#[cfg(feature = "fs")]
pub use fs::set_process_bundle_root;
pub use host::{
  agent_id, entry_dir, entry_url, is_dedicated_worker, module_generation,
  module_reloaded, note_test_error, on_dedicated_worker, reload_keeps_module,
  reload_targets_modules, set_dom_exception, set_file_root, set_rejection_hook,
  try_agent_id,
};
pub use idle::{
  IdleHooks, idle_frame_delay, idle_has_pending, idle_next_wait,
  next_host_delay, poll_idle_period, poll_idle_timeout, set_idle_frame_delay,
  set_idle_hooks,
};
pub use inspect::{InspectFields, register_inspect};
pub use jobs::{Listen, enqueue, listen, pending as jobs_pending, spawn};
pub use js::{
  caller_module_url, global, namespace, rust_string, set_function, set_value,
  throw_type,
};
pub use loader::module_source::url_of as module_source_url;
pub use loader::modules::{canonicalize, file_url, module_graph};
pub use parcel::{Parcel, deserialize_parcel, serialize_parcel};
pub use permissions::{
  PermissionRules, PermissionSpec, authorize_url, check_env, check_env_all,
  check_ffi, check_net, check_read, check_run, check_write, check_write_all,
  grant_read, grant_write, install_permissions, retain_resolved_addresses,
  validate_permissions,
};
pub use process::{set_args, with_env};
pub use reach::walk_objects;
pub use realm::{
  clone_value, delete_intl_v8_break_iterator, delete_object_prototype_proto,
  performance_now_ms, time_origin_ms,
};
pub use repl::run_repl;
pub use runtime::{
  ClassicPart, ClassicRun, ClassicSession, InternedScript, LoopInterest,
  RuntimeError, WatchMode, poll_task_poke, retain_host_loop,
  run_classic_scripts, run_classic_scripts_at, run_file, run_file_sandboxed,
  run_file_watch, run_scripts, run_tests, set_loop_hooks, set_retain_host_loop,
  set_task_waker, status_of, wait_for_tasks,
};
pub use timers::{
  cancel_host, schedule_animation_callback, schedule_callback, schedule_host,
};
pub use worker::{
  Inbound, LocationParts, ScriptKind, WorkerLoadError, WorkerScript,
  agent_alive, close_worker, drain_inbound, eval_classic_in_worker,
  keeps_alive, load_worker_source, parent_agent, poke_agent, post_inbound,
  resolve_specifier, run_worker_frame_hook, set_blob_script_loader,
  set_classic_worker_source, set_inbound_handler, set_worker_frame_hook,
  set_worker_owner, spawn_worker, take_inbox, terminate_worker,
  worker_is_module, worker_location, worker_name,
};

#[cfg(test)]
mod tests;
