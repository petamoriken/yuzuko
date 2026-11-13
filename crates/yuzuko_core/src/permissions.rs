//! Process permissions for filesystem, network, environment, subprocess, and
//! native-library access.
//!
//! No installed policy allows every check. `yuzuko run`, `yuzuko repl`, and
//! `yuzuko test` install a sandbox. A packaged app installs one when its
//! manifest has `permissions`. `query` does not prompt. `request` and a
//! gated operation prompt when the policy says to and standard input is a
//! terminal.

use std::borrow::Cow;
use std::cell::RefCell;
use std::ffi::OsStr;
#[cfg(feature = "child")]
use std::ffi::OsString;
use std::io::{IsTerminal, Write};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::path::{Component, Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use crate::op::{OpError, OpException};

static MODE: Mutex<Mode> = Mutex::new(Mode::Open);

enum Mode {
  Open,
  Sandbox(Box<Sandbox>),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
  Read = 0,
  Write = 1,
  Net = 2,
  Env = 3,
  Run = 4,
  Ffi = 5,
}

impl Kind {
  fn name(self) -> &'static str {
    match self {
      Self::Read => "read",
      Self::Write => "write",
      Self::Net => "net",
      Self::Env => "env",
      Self::Run => "run",
      Self::Ffi => "ffi",
    }
  }

  fn parse(name: &str) -> Option<Self> {
    match name {
      "read" => Some(Self::Read),
      "write" => Some(Self::Write),
      "net" => Some(Self::Net),
      "env" => Some(Self::Env),
      "run" => Some(Self::Run),
      "ffi" => Some(Self::Ffi),
      _ => None,
    }
  }

  fn detail_field(self) -> &'static str {
    match self {
      Self::Read | Self::Write | Self::Ffi => "path",
      Self::Net => "host",
      Self::Env => "variable",
      Self::Run => "command",
    }
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
  Granted,
  Denied,
  Prompt,
}

impl State {
  fn as_str(self) -> &'static str {
    match self {
      Self::Granted => "granted",
      Self::Denied => "denied",
      Self::Prompt => "prompt",
    }
  }
}

enum Detail {
  Any,
  /// Follow every symlink, including the final component.
  Path(PathBuf),
  /// The directory entry. A final symlink is not resolved.
  Entry(PathBuf),
  /// [`Entry`](Self::Entry), and a deny of a descendant blocks the call.
  Tree(PathBuf),
  Net {
    host: String,
    port: Option<u16>,
  },
  Name(String),
}

/// Whether the final path component is resolved before the rules are applied.
#[derive(Clone, Copy, PartialEq, Eq)]
enum PathLink {
  /// Follow every symlink, including the final component.
  Follow,
  /// Follow symlinks in ancestors. A final symlink stays that entry.
  Entry,
}

enum Resource<'a> {
  Path(&'a Path),
  Net { host: &'a str, port: u16 },
  Name(&'a str),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Answer {
  Once,
  All,
  Deny,
}

#[derive(Clone, Debug, Default)]
pub struct PermissionRules {
  pub allow_all: bool,
  pub allow: Vec<String>,
  pub deny_all: bool,
  pub deny: Vec<String>,
}

/// Allow and deny rules collected from flags and `yuzuko.json`.
///
/// `prompt` is whether a missing grant may ask on a terminal. Relative
/// filesystem paths are resolved against the root passed to
/// [`install_permissions`].
#[derive(Clone, Debug, Default)]
pub struct PermissionSpec {
  pub read: PermissionRules,
  pub write: PermissionRules,
  pub net: PermissionRules,
  pub env: PermissionRules,
  pub run: PermissionRules,
  pub ffi: PermissionRules,
  pub prompt: bool,
}

impl PermissionSpec {
  pub fn allow_all() -> Self {
    let mut spec = Self::default();
    for rules in spec.each_mut() {
      rules.allow_all = true;
    }
    spec
  }

  pub fn allow(
    &mut self,
    name: &str,
    values: Option<Vec<String>>,
  ) -> Result<(), String> {
    let rules = self.rules_mut(name)?;
    match values {
      None => rules.allow_all = true,
      Some(values) => rules.allow.extend(values),
    }
    Ok(())
  }

  pub fn deny(
    &mut self,
    name: &str,
    values: Option<Vec<String>>,
  ) -> Result<(), String> {
    let rules = self.rules_mut(name)?;
    match values {
      None => rules.deny_all = true,
      Some(values) => rules.deny.extend(values),
    }
    Ok(())
  }

  fn rules_mut(&mut self, name: &str) -> Result<&mut PermissionRules, String> {
    match name {
      "read" => Ok(&mut self.read),
      "write" => Ok(&mut self.write),
      "net" => Ok(&mut self.net),
      "env" => Ok(&mut self.env),
      "run" => Ok(&mut self.run),
      "ffi" => Ok(&mut self.ffi),
      _ => Err("The permission name is not supported.".to_string()),
    }
  }

  fn each_mut(&mut self) -> [&mut PermissionRules; 6] {
    [
      &mut self.read,
      &mut self.write,
      &mut self.net,
      &mut self.env,
      &mut self.run,
      &mut self.ffi,
    ]
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct NetRule {
  host: String,
  port: Option<u16>,
}

#[derive(Clone, Debug, Default)]
struct Gate {
  allow_all: bool,
  deny_all: bool,
  revoked: bool,
  session_allow_all: bool,
  session_deny_all: bool,
  allow_paths: Vec<PathBuf>,
  deny_paths: Vec<PathBuf>,
  session_allow_paths: Vec<PathBuf>,
  session_deny_paths: Vec<PathBuf>,
  allow_nets: Vec<NetRule>,
  deny_nets: Vec<NetRule>,
  session_allow_nets: Vec<NetRule>,
  session_deny_nets: Vec<NetRule>,
  allow_names: Vec<String>,
  deny_names: Vec<String>,
  session_allow_names: Vec<String>,
  session_deny_names: Vec<String>,
}

#[derive(Debug)]
struct Sandbox {
  gates: [Gate; 6],
  prompt: bool,
  /// Directories on `PATH` when this policy was installed. A bare run
  /// grant is the executable found here, not a `PATH` the script sets later.
  #[cfg(feature = "child")]
  command_path: Vec<PathBuf>,
}

impl Sandbox {
  fn empty(prompt: bool) -> Self {
    Self {
      gates: std::array::from_fn(|_| Gate::default()),
      prompt,
      #[cfg(feature = "child")]
      command_path: Vec::new(),
    }
  }

  fn allow_all(prompt: bool) -> Self {
    let mut sandbox = Self::empty(prompt);
    for gate in &mut sandbox.gates {
      gate.allow_all = true;
    }
    sandbox
  }

  fn compile(spec: &PermissionSpec, root: &Path) -> Result<Self, String> {
    let root = lexical_absolute(root).map_err(error_text)?;
    let mut sandbox = Self::empty(spec.prompt);
    fill(
      &mut sandbox.gates[Kind::Read as usize],
      Kind::Read,
      &spec.read,
      &root,
    )?;
    fill(
      &mut sandbox.gates[Kind::Write as usize],
      Kind::Write,
      &spec.write,
      &root,
    )?;
    fill(
      &mut sandbox.gates[Kind::Net as usize],
      Kind::Net,
      &spec.net,
      &root,
    )?;
    fill(
      &mut sandbox.gates[Kind::Env as usize],
      Kind::Env,
      &spec.env,
      &root,
    )?;
    fill(
      &mut sandbox.gates[Kind::Run as usize],
      Kind::Run,
      &spec.run,
      &root,
    )?;
    fill(
      &mut sandbox.gates[Kind::Ffi as usize],
      Kind::Ffi,
      &spec.ffi,
      &root,
    )?;
    #[cfg(feature = "child")]
    {
      sandbox.command_path = capture_command_path();
    }
    Ok(sandbox)
  }

  fn classify(&self, kind: Kind, detail: &Detail) -> Result<State, OpError> {
    match detail {
      Detail::Any => Ok(self.unscoped(kind)),
      Detail::Path(path) => {
        self.classify_path(kind, path, PathLink::Follow, false)
      }
      Detail::Entry(path) => {
        self.classify_path(kind, path, PathLink::Entry, false)
      }
      Detail::Tree(path) => {
        self.classify_path(kind, path, PathLink::Entry, true)
      }
      Detail::Net { host, port: None } => Ok(self.net_host(kind, host)),
      Detail::Net {
        host,
        port: Some(port),
      } => Ok(self.resource(
        kind,
        &Resource::Net {
          host: host.as_str(),
          port: *port,
        },
      )),
      Detail::Name(name) => Ok(self.resource(kind, &Resource::Name(name))),
    }
  }

  fn unscoped(&self, kind: Kind) -> State {
    let gate = &self.gates[kind as usize];
    if gate.deny_all {
      return State::Denied;
    }
    if gate.session_deny_all {
      return State::Denied;
    }
    if gate.session_allow_all || (gate.allow_all && !gate.revoked) {
      return State::Granted;
    }
    State::Prompt
  }

  fn net_host(&self, kind: Kind, host: &str) -> State {
    let gate = &self.gates[kind as usize];
    if gate.deny_all || net_host_hit(&gate.deny_nets, host) {
      return State::Denied;
    }
    if gate.session_deny_all || net_host_hit(&gate.session_deny_nets, host) {
      return State::Denied;
    }
    if gate.session_allow_all || net_host_hit(&gate.session_allow_nets, host) {
      return State::Granted;
    }
    if !gate.revoked && (gate.allow_all || net_host_hit(&gate.allow_nets, host))
    {
      return State::Granted;
    }
    State::Prompt
  }

  fn classify_path(
    &self,
    kind: Kind,
    path: &Path,
    link: PathLink,
    strict: bool,
  ) -> Result<State, OpError> {
    let found = path_candidates(path, link)?;
    let states = found
      .paths
      .iter()
      .map(|candidate| self.resource(kind, &Resource::Path(candidate)))
      .collect::<Vec<_>>();
    let mut state = combine(&states);
    // A failed walk must not skip the operating-system candidate, and
    // it must not surface a distinct error that reveals the file.
    if !found.resolved && state == State::Granted {
      state = State::Denied;
    }
    // A recursive delete would remove a denied descendant. One path does
    // not, so only the strict check looks inside the tree.
    if strict
      && state != State::Denied
      && self.descendant_denied(kind, &found.paths)
    {
      state = State::Denied;
    }
    Ok(state)
  }

  fn descendant_denied(&self, kind: Kind, candidates: &[PathBuf]) -> bool {
    let gate = &self.gates[kind as usize];
    gate
      .deny_paths
      .iter()
      .chain(gate.session_deny_paths.iter())
      .any(|deny| {
        same_entry_spellings(deny).iter().any(|form| {
          candidates.iter().any(|candidate| {
            path_covers(candidate, form) && !paths_equal(candidate, form)
          })
        })
      })
  }

  fn resource(&self, kind: Kind, resource: &Resource<'_>) -> State {
    let gate = &self.gates[kind as usize];
    if gate.deny_all
      || hits(
        kind,
        &gate.deny_paths,
        &gate.deny_nets,
        &gate.deny_names,
        resource,
      )
    {
      return State::Denied;
    }
    if gate.session_deny_all
      || hits(
        kind,
        &gate.session_deny_paths,
        &gate.session_deny_nets,
        &gate.session_deny_names,
        resource,
      )
    {
      return State::Denied;
    }
    if gate.session_allow_all
      || hits(
        kind,
        &gate.session_allow_paths,
        &gate.session_allow_nets,
        &gate.session_allow_names,
        resource,
      )
    {
      return State::Granted;
    }
    if !gate.revoked
      && (gate.allow_all
        || hits(
          kind,
          &gate.allow_paths,
          &gate.allow_nets,
          &gate.allow_names,
          resource,
        ))
    {
      return State::Granted;
    }
    State::Prompt
  }

  fn remember(
    &mut self,
    kind: Kind,
    detail: &Detail,
    answer: Answer,
  ) -> Result<(), OpError> {
    let gate = &mut self.gates[kind as usize];
    match answer {
      Answer::All => {
        gate.session_allow_all = true;
        gate.session_deny_all = false;
        gate.revoked = false;
        Ok(())
      }
      Answer::Once => match detail {
        Detail::Any => {
          gate.session_allow_all = true;
          gate.session_deny_all = false;
          gate.revoked = false;
          Ok(())
        }
        Detail::Path(path) => {
          record_session_paths(gate, path, PathLink::Follow, true)
        }
        Detail::Entry(path) | Detail::Tree(path) => {
          record_session_paths(gate, path, PathLink::Entry, true)
        }
        Detail::Net { host, port } => {
          let rule = NetRule {
            host: normalize_host(host),
            port: *port,
          };
          gate.session_deny_nets.retain(|item| item != &rule);
          gate.session_allow_nets.push(rule);
          Ok(())
        }
        Detail::Name(name) => record_session_name(gate, kind, name, true),
      },
      Answer::Deny => match detail {
        Detail::Any => {
          gate.session_deny_all = true;
          gate.session_allow_all = false;
          Ok(())
        }
        Detail::Path(path) => {
          record_session_paths(gate, path, PathLink::Follow, false)
        }
        Detail::Entry(path) | Detail::Tree(path) => {
          record_session_paths(gate, path, PathLink::Entry, false)
        }
        Detail::Net { host, port } => {
          let rule = NetRule {
            host: normalize_host(host),
            port: *port,
          };
          gate.session_allow_nets.retain(|item| item != &rule);
          gate.session_deny_nets.push(rule);
          Ok(())
        }
        Detail::Name(name) => record_session_name(gate, kind, name, false),
      },
    }
  }

  fn revoke(&mut self, kind: Kind, detail: &Detail) -> Result<(), OpError> {
    let gate = &mut self.gates[kind as usize];
    match detail {
      Detail::Any => {
        gate.revoked = true;
        gate.session_allow_all = false;
        gate.session_deny_all = false;
        gate.session_allow_paths.clear();
        gate.session_deny_paths.clear();
        gate.session_allow_nets.clear();
        gate.session_deny_nets.clear();
        gate.session_allow_names.clear();
        gate.session_deny_names.clear();
        Ok(())
      }
      Detail::Path(path) => {
        record_session_paths(gate, path, PathLink::Follow, false)
      }
      Detail::Entry(path) | Detail::Tree(path) => {
        record_session_paths(gate, path, PathLink::Entry, false)
      }
      Detail::Net { host, port } => {
        let rule = NetRule {
          host: normalize_host(host),
          port: *port,
        };
        gate.session_allow_nets.retain(|item| item != &rule);
        gate.session_deny_nets.push(rule);
        Ok(())
      }
      Detail::Name(name) => record_session_name(gate, kind, name, false),
    }
  }

  fn grant_path(&mut self, kind: Kind, path: &Path) -> Result<(), OpError> {
    let candidates = session_candidates(path, PathLink::Follow)?;
    // A sticky deny blocks the grant. A previous prompt denial is only a
    // session deny, so a later dialog can replace it.
    let gate = &self.gates[kind as usize];
    if gate.deny_all {
      return Ok(());
    }
    for candidate in &candidates {
      if hits(kind, &gate.deny_paths, &[], &[], &Resource::Path(candidate)) {
        return Ok(());
      }
    }
    let gate = &mut self.gates[kind as usize];
    for candidate in candidates {
      gate
        .session_deny_paths
        .retain(|item| !paths_equal(item, &candidate));
      if !gate
        .session_allow_paths
        .iter()
        .any(|item| paths_equal(item, &candidate))
      {
        gate.session_allow_paths.push(candidate);
      }
    }
    Ok(())
  }
}

fn fill(
  gate: &mut Gate,
  kind: Kind,
  rules: &PermissionRules,
  root: &Path,
) -> Result<(), String> {
  gate.allow_all = rules.allow_all;
  gate.deny_all = rules.deny_all;
  store(gate, kind, true, &rules.allow, root)?;
  store(gate, kind, false, &rules.deny, root)?;
  Ok(())
}

fn store(
  gate: &mut Gate,
  kind: Kind,
  allow: bool,
  values: &[String],
  root: &Path,
) -> Result<(), String> {
  for value in values {
    match kind {
      Kind::Read | Kind::Write | Kind::Ffi => {
        let path = resolve_path(kind, root, value)?;
        if allow {
          gate.allow_paths.push(path);
        } else {
          gate.deny_paths.push(path);
        }
      }
      Kind::Net => {
        let rule = parse_net(value)?;
        if allow {
          gate.allow_nets.push(rule);
        } else {
          gate.deny_nets.push(rule);
        }
      }
      Kind::Env => {
        let name = parse_env(value)?;
        if allow {
          gate.allow_names.push(name);
        } else {
          gate.deny_names.push(name);
        }
      }
      Kind::Run => {
        let name = parse_run(root, value)?;
        if allow {
          gate.allow_names.push(name);
        } else {
          gate.deny_names.push(name);
        }
      }
    }
  }
  Ok(())
}

fn hits(
  kind: Kind,
  paths: &[PathBuf],
  nets: &[NetRule],
  names: &[String],
  resource: &Resource<'_>,
) -> bool {
  match resource {
    Resource::Path(path) => paths.iter().any(|grant| path_covers(grant, path)),
    Resource::Net { host, port } => {
      nets.iter().any(|rule| net_covers(rule, host, *port))
    }
    Resource::Name(name) => match kind {
      Kind::Env => names.iter().any(|rule| env_covers(rule, name)),
      Kind::Run => names.iter().any(|rule| run_covers(rule, name)),
      _ => false,
    },
  }
}

fn combine(states: &[State]) -> State {
  if states.contains(&State::Denied) {
    return State::Denied;
  }
  if !states.is_empty() && states.iter().all(|state| *state == State::Granted) {
    return State::Granted;
  }
  State::Prompt
}

fn lock() -> MutexGuard<'static, Mode> {
  MODE.lock().unwrap_or_else(|error| error.into_inner())
}

fn error_text(error: OpError) -> String {
  match error {
    OpError::Type(message)
    | OpError::Range(message)
    | OpError::Dom { message, .. } => message,
    OpError::Thrown(_) => {
      "The permission path could not be resolved.".to_string()
    }
  }
}

/// Reject `spec` when a path, host, or name cannot be stored.
pub fn validate_permissions(
  spec: &PermissionSpec,
  root: &Path,
) -> Result<(), String> {
  Sandbox::compile(spec, root).map(|_| ())
}

/// Whether this process has not installed a sandbox.
///
/// An open process allows every path, so a caller can skip naming an opened
/// file. A sandbox still checks that file, including one that grants every
/// path.
#[cfg(any(test, feature = "fs"))]
pub(crate) fn permissions_are_open() -> bool {
  matches!(*lock(), Mode::Open)
}

/// Replace the process policy. An open process allows every check until this
/// runs.
pub fn install_permissions(
  spec: &PermissionSpec,
  root: &Path,
) -> Result<(), String> {
  let sandbox = Sandbox::compile(spec, root)?;
  *lock() = Mode::Sandbox(Box::new(sandbox));
  Ok(())
}

/// Whether `path` is granted. This does not ask.
///
/// A call that already opened a file descriptor uses this on the file it
/// opened. A missing grant stays closed, so a swapped symlink cannot prompt
/// for the other file.
#[cfg(feature = "fs")]
pub fn path_is_granted(write: bool, path: &Path) -> Result<bool, OpError> {
  let kind = if write { Kind::Write } else { Kind::Read };
  Ok(
    interact(kind, &Detail::Path(path.to_path_buf()), false)? == State::Granted,
  )
}

/// `NotCapable` naming `path`, the path the caller requested.
#[cfg(feature = "fs")]
pub fn path_not_allowed(write: bool, path: &Path) -> OpError {
  let kind = if write { Kind::Write } else { Kind::Read };
  denied(kind, &Detail::Path(path.to_path_buf())).unwrap_or_else(|error| error)
}

pub fn check_read(path: &Path) -> Result<(), OpError> {
  require(Kind::Read, &Detail::Path(path.to_path_buf()))
}

/// Read of the directory entry. A final symlink is not resolved, so the
/// grant covers the link and does not have to cover its target.
pub fn check_read_entry(path: &Path) -> Result<(), OpError> {
  require(Kind::Read, &Detail::Entry(path.to_path_buf()))
}

pub fn check_write(path: &Path) -> Result<(), OpError> {
  require(Kind::Write, &Detail::Path(path.to_path_buf()))
}

/// Write of the directory entry. A final symlink is not resolved.
pub fn check_write_entry(path: &Path) -> Result<(), OpError> {
  require(Kind::Write, &Detail::Entry(path.to_path_buf()))
}

/// Write of a tree. A final symlink is not resolved, and a deny of a
/// descendant blocks the call.
pub fn check_write_tree(path: &Path) -> Result<(), OpError> {
  require(Kind::Write, &Detail::Tree(path.to_path_buf()))
}

pub fn check_write_all() -> Result<(), OpError> {
  require(Kind::Write, &Detail::Any)
}

pub fn check_net(host: &str, port: u16) -> Result<(), OpError> {
  require(
    Kind::Net,
    &Detail::Net {
      host: normalize_host(host),
      port: Some(port),
    },
  )
}

/// Net access to `host` with no port. A grant of one port does not match.
pub fn check_net_host(host: &str) -> Result<(), OpError> {
  require(
    Kind::Net,
    &Detail::Net {
      host: normalize_host(host),
      port: None,
    },
  )
}

/// Keeps addresses the net policy allows this connection to use.
///
/// A globally routable address is kept. A loopback, private, link-local,
/// unique-local, site-local, NAT64, shared, multicast, broadcast, or
/// unspecified address is checked again with [`check_net`]. An IPv4-mapped
/// IPv6 address is that IPv4 address. When every address is rejected, the
/// error is the first denial. An empty input stays empty.
pub fn retain_resolved_addresses(
  addrs: Vec<SocketAddr>,
) -> Result<Vec<SocketAddr>, OpError> {
  let mut kept = Vec::new();
  let mut denial = None;
  for addr in addrs {
    let Some(host) = restricted_host(addr.ip()) else {
      kept.push(addr);
      continue;
    };
    match check_net(&host, addr.port()) {
      Ok(()) => kept.push(addr),
      Err(error) => {
        if denial.is_none() {
          denial = Some(error);
        }
      }
    }
  }
  if kept.is_empty()
    && let Some(error) = denial
  {
    return Err(error);
  }
  Ok(kept)
}

/// Read-checks the path a symbolic link will name.
///
/// An absolute target is checked as written. A relative target is joined to
/// the parent of `new`, which is the directory POSIX uses. A junction is
/// checked as written because its reparse text is joined to the current
/// directory. The stored link text is unchanged.
#[cfg(feature = "fs")]
pub(crate) fn check_symlink_target(
  old: &Path,
  new: &Path,
  junction: bool,
) -> Result<(), OpError> {
  check_read(&symlink_target(old, new, junction)?)
}

#[cfg(feature = "fs")]
fn symlink_target(
  old: &Path,
  new: &Path,
  junction: bool,
) -> Result<PathBuf, OpError> {
  if junction || old.is_absolute() {
    return Ok(old.to_path_buf());
  }
  let new = absolute_path(new)?;
  let parent = new.parent().filter(|parent| !parent.as_os_str().is_empty());
  let parent = match parent {
    Some(parent) => parent.to_path_buf(),
    None => current_dir()?,
  };
  Ok(parent.join(old))
}

pub fn check_env(name: &str) -> Result<(), OpError> {
  require(Kind::Env, &Detail::Name(name.to_string()))
}

pub fn check_env_all() -> Result<(), OpError> {
  require(Kind::Env, &Detail::Any)
}

pub fn check_run(command: &str) -> Result<(), OpError> {
  require(Kind::Run, &Detail::Name(command.to_string()))
}

/// One `options.env` entry. `None` removes the name.
#[cfg(feature = "child")]
pub(crate) struct SpawnEnv<'a> {
  pub name: &'a str,
  pub value: Option<&'a str>,
}

/// The program `spawn` may execute.
///
/// A bare name is resolved through the child's `PATH` and allowed only when
/// that file is the one the name found on `PATH` at installation. A path is
/// the absolute file the run check accepted. When `run` is granted as a
/// whole, the returned path is the caller's command and loader variables are
/// left in place. Otherwise an `LD_` or `DYLD_` variable rejects the child.
#[cfg(feature = "child")]
pub(crate) fn resolve_spawn(
  command: &str,
  cwd: Option<&Path>,
  env: &[SpawnEnv<'_>],
) -> Result<PathBuf, OpError> {
  if is_command_path(command) {
    // Resolve once and check that file. Resolving again for `exec` would let
    // a symlink swapped between the two run a file the check never saw.
    // The denial names the caller's command, so it does not reveal where a
    // symlink outside the grant leads.
    let executed = with_run_base(cwd, || absolute_command(Path::new(command)))?;
    if check_run(&executed.to_string_lossy()).is_err() {
      return Err(denied(Kind::Run, &Detail::Name(command.to_string()))?);
    }
    if run_is_whole() {
      return Ok(PathBuf::from(command));
    }
    if loader_env_present(env) {
      return Err(denied(Kind::Run, &Detail::Any)?);
    }
    return Ok(executed);
  }
  check_run(command)?;
  if run_is_whole() {
    return Ok(PathBuf::from(command));
  }
  if loader_env_present(env) {
    return Err(denied(Kind::Run, &Detail::Any)?);
  }
  let base = match cwd {
    Some(path) => path.to_path_buf(),
    None => current_dir()?,
  };
  let trusted = find_command(command, &startup_command_path());
  let executed = find_command(command, &effective_command_path(env, &base));
  match (trusted, executed) {
    (Some(trusted), Some(executed)) if paths_equal(&trusted, &executed) => {
      Ok(executed)
    }
    (_, Some(executed)) => Err(denied(
      Kind::Run,
      &Detail::Name(executed.display().to_string()),
    )?),
    _ => Err(denied(Kind::Run, &Detail::Name(command.to_string()))?),
  }
}

#[cfg(feature = "child")]
fn with_run_base<T>(base: Option<&Path>, body: impl FnOnce() -> T) -> T {
  let _guard = base.map(RunBase::set);
  body()
}

#[cfg(feature = "child")]
fn run_is_whole() -> bool {
  let guard = lock();
  match &*guard {
    Mode::Open => true,
    Mode::Sandbox(sandbox) => {
      let gate = &sandbox.gates[Kind::Run as usize];
      if gate.deny_all || gate.session_deny_all {
        return false;
      }
      gate.session_allow_all || (gate.allow_all && !gate.revoked)
    }
  }
}

#[cfg(feature = "child")]
fn startup_command_path() -> Vec<PathBuf> {
  let guard = lock();
  match &*guard {
    Mode::Sandbox(sandbox) => sandbox.command_path.clone(),
    Mode::Open => Vec::new(),
  }
}

#[cfg(feature = "child")]
fn capture_command_path() -> Vec<PathBuf> {
  let Ok(cwd) = std::env::current_dir() else {
    return Vec::new();
  };
  let Some(raw) = std::env::var_os("PATH") else {
    return Vec::new();
  };
  absolute_path_entries(&raw, &cwd)
}

#[cfg(feature = "child")]
fn effective_command_path(env: &[SpawnEnv<'_>], base: &Path) -> Vec<PathBuf> {
  let mut raw = std::env::var_os("PATH");
  for change in env {
    if change.value.is_none() && env_eq(change.name, "PATH") {
      raw = None;
    }
  }
  for change in env {
    if let Some(value) = change.value
      && env_eq(change.name, "PATH")
    {
      raw = Some(OsString::from(value));
    }
  }
  let Some(raw) = raw else {
    return Vec::new();
  };
  absolute_path_entries(&raw, base)
}

#[cfg(feature = "child")]
fn absolute_path_entries(raw: &OsStr, base: &Path) -> Vec<PathBuf> {
  let mut dirs = Vec::new();
  for entry in std::env::split_paths(raw) {
    if entry.as_os_str().is_empty() {
      dirs.push(base.to_path_buf());
      continue;
    }
    if entry.is_absolute() {
      dirs.push(entry);
    } else {
      dirs.push(base.join(entry));
    }
  }
  dirs
}

#[cfg(feature = "child")]
fn find_command(name: &str, dirs: &[PathBuf]) -> Option<PathBuf> {
  let file_name = command_file_name(name);
  for dir in dirs {
    let candidate = dir.join(&file_name);
    if !command_file_exists(&candidate) {
      continue;
    }
    return executed_path(&candidate).or(Some(candidate));
  }
  None
}

#[cfg(feature = "child")]
fn command_file_name(name: &str) -> String {
  // Rust's Windows process spawn appends `.exe` when the name has no
  // extension and then searches `PATH` for that file.
  #[cfg(windows)]
  if Path::new(name).extension().is_none() {
    return format!("{name}.exe");
  }
  name.to_string()
}

#[cfg(feature = "child")]
fn command_file_exists(path: &Path) -> bool {
  let Ok(meta) = std::fs::metadata(path) else {
    return false;
  };
  meta.is_file() && command_file_executable(&meta)
}

#[cfg(all(feature = "child", unix))]
fn command_file_executable(meta: &std::fs::Metadata) -> bool {
  use std::os::unix::fs::PermissionsExt;
  meta.permissions().mode() & 0o111 != 0
}

#[cfg(all(feature = "child", not(unix)))]
fn command_file_executable(_meta: &std::fs::Metadata) -> bool {
  true
}

#[cfg(feature = "child")]
fn loader_env_present(env: &[SpawnEnv<'_>]) -> bool {
  let mut present: Vec<OsString> =
    std::env::vars_os().map(|(name, _)| name).collect();
  for change in env {
    if change.value.is_none() {
      present.retain(|name| !env_os_eq(name, change.name));
    }
  }
  if present.iter().any(|name| is_loader_name(name)) {
    return true;
  }
  env.iter().any(|change| {
    change.value.is_some() && is_loader_name(OsStr::new(change.name))
  })
}

#[cfg(feature = "child")]
fn is_loader_name(name: &OsStr) -> bool {
  let Some(text) = name.to_str() else {
    return false;
  };
  starts_with_ascii_ignore_case(text, "LD_")
    || starts_with_ascii_ignore_case(text, "DYLD_")
}

#[cfg(feature = "child")]
fn starts_with_ascii_ignore_case(text: &str, prefix: &str) -> bool {
  text.len() >= prefix.len()
    && text.as_bytes()[..prefix.len()].eq_ignore_ascii_case(prefix.as_bytes())
}

#[cfg(feature = "child")]
fn env_os_eq(left: &OsStr, right: &str) -> bool {
  let Some(left) = left.to_str() else {
    return false;
  };
  env_eq(left, right)
}

pub fn check_ffi(path: &Path) -> Result<(), OpError> {
  let text = path.to_string_lossy();
  // A slash-free name is searched by the dynamic linker. Checking it as
  // `cwd/name` would let a directory grant load a system library.
  if !text.contains('/') && !text.contains('\\') {
    return require(Kind::Ffi, &Detail::Any);
  }
  require(Kind::Ffi, &Detail::Path(path.to_path_buf()))
}

/// `file:` requires read access. `http:`, `https:`, `ws:`, and `wss:` require
/// net access. Other schemes are not gated here.
pub fn authorize_url(url: &url::Url) -> Result<(), OpError> {
  match url.scheme() {
    "http" | "https" | "ws" | "wss" => {
      let host = url.host_str().unwrap_or("");
      let port = url.port_or_known_default().unwrap_or(0);
      check_net(host, port)
    }
    "file" => {
      let path = url
        .to_file_path()
        .map_err(|()| OpError::type_("The file URL is invalid."))?;
      check_read(&path)
    }
    _ => Ok(()),
  }
}

/// Record a session read grant for a path the user chose, unless a deny
/// rule names it.
pub fn grant_read(path: &Path) {
  grant(Kind::Read, path);
}

/// Record a session write grant for a path the user chose, unless a deny
/// rule names it.
pub fn grant_write(path: &Path) {
  grant(Kind::Write, path);
}

fn grant(kind: Kind, path: &Path) {
  let mut guard = lock();
  let Mode::Sandbox(sandbox) = &mut *guard else {
    return;
  };
  let _ = sandbox.grant_path(kind, path);
}

fn require(kind: Kind, detail: &Detail) -> Result<(), OpError> {
  match interact(kind, detail, true)? {
    State::Granted => Ok(()),
    State::Denied | State::Prompt => Err(denied(kind, detail)?),
  }
}

fn interact(
  kind: Kind,
  detail: &Detail,
  prompt_now: bool,
) -> Result<State, OpError> {
  let mut guard = lock();
  if matches!(*guard, Mode::Open) {
    return Ok(State::Granted);
  }
  let Mode::Sandbox(sandbox) = &mut *guard else {
    return Ok(State::Granted);
  };
  let state = sandbox.classify(kind, detail)?;
  if state != State::Prompt || !prompt_now {
    return Ok(state);
  }
  // A missing terminal, like --no-prompt, denies this call and leaves the
  // queried state at prompt.
  if !sandbox.prompt || !std::io::stdin().is_terminal() {
    return Ok(State::Denied);
  }
  let text = display(detail)?;
  let Some(answer) = ask(kind, &text) else {
    return Ok(State::Denied);
  };
  settle_answer(sandbox, kind, detail, answer)
}

fn settle_answer(
  sandbox: &mut Sandbox,
  kind: Kind,
  detail: &Detail,
  answer: Answer,
) -> Result<State, OpError> {
  // A stored `*` would be read back as a wildcard rule, so a yes to `env`
  // `FOO*` or `net` `*:80` would cover names never asked about. That answer
  // applies to this call only.
  if answer != Answer::All && is_wildcard_request(kind, detail) {
    return Ok(match answer {
      Answer::Once => State::Granted,
      Answer::All | Answer::Deny => State::Denied,
    });
  }
  sandbox.remember(kind, detail, answer)?;
  // A yes stores the requested file, not a different file reached by
  // following a symlink and then `..`. Classify again so that yes cannot
  // open the other file.
  Ok(match answer {
    Answer::Deny => State::Denied,
    Answer::All => State::Granted,
    Answer::Once => sandbox.classify(kind, detail)?,
  })
}

/// A requested name that a stored rule would read as a wildcard.
fn is_wildcard_request(kind: Kind, detail: &Detail) -> bool {
  match (kind, detail) {
    (Kind::Env, Detail::Name(name)) => name.ends_with('*'),
    (Kind::Net, Detail::Net { host, .. }) => host == "*",
    _ => false,
  }
}

fn revoke_detail(kind: Kind, detail: &Detail) -> Result<State, OpError> {
  let mut guard = lock();
  if matches!(*guard, Mode::Open) {
    *guard = Mode::Sandbox(Box::new(Sandbox::allow_all(false)));
  }
  let Mode::Sandbox(sandbox) = &mut *guard else {
    return Ok(State::Granted);
  };
  sandbox.revoke(kind, detail)?;
  sandbox.classify(kind, detail)
}

fn denied(kind: Kind, detail: &Detail) -> Result<OpError, OpError> {
  let text = display(detail)?;
  let name = kind.name();
  let message = if text.is_empty() {
    format!("Requires {name} access, run again with the --allow-{name} flag")
  } else {
    format!(
      "Requires {name} access to \"{text}\", run again with the --allow-{name} flag"
    )
  };
  Ok(OpError::dom("NotCapable", message))
}

fn display(detail: &Detail) -> Result<String, OpError> {
  let text = match detail {
    Detail::Any => String::new(),
    Detail::Path(path) | Detail::Entry(path) | Detail::Tree(path) => {
      lexical_absolute(path)?.display().to_string()
    }
    Detail::Net { host, port } => format_host(host, *port),
    Detail::Name(name) => name.clone(),
  };
  Ok(sanitize_prompt(&text))
}

/// Keep a prompt on one line. Control characters, quotes, backslashes,
/// bidirectional overrides, the Arabic letter mark, and line and paragraph
/// separators are escaped so the requested name cannot redraw the terminal.
/// Other format characters, including ZWJ, stay.
fn sanitize_prompt(text: &str) -> String {
  let mut out = String::with_capacity(text.len());
  for char in text.chars() {
    match char {
      '\\' => out.push_str("\\\\"),
      '"' => out.push_str("\\\""),
      '\n' => out.push_str("\\n"),
      '\r' => out.push_str("\\r"),
      '\t' => out.push_str("\\t"),
      char if char.is_control() || is_prompt_mark(char) => {
        out.push_str(&format!("\\u{{{:x}}}", u32::from(char)));
      }
      char => out.push(char),
    }
  }
  out
}

/// Characters that can reorder or break a prompt line and are not controls.
///
/// ZWJ (U+200D) is a format character and is not in this set.
fn is_prompt_mark(char: char) -> bool {
  matches!(
    char,
    '\u{061C}'
      | '\u{200E}'..='\u{200F}'
      | '\u{2028}'..='\u{202E}'
      | '\u{2066}'..='\u{2069}'
  )
}

fn ask(kind: Kind, detail: &str) -> Option<Answer> {
  if !std::io::stdin().is_terminal() {
    return None;
  }
  let name = kind.name();
  let access = if detail.is_empty() {
    format!("{name} access")
  } else {
    format!("{name} access to \"{detail}\"")
  };
  eprint!(
    "┏ ⚠ Yuzuko requests {access}.\n\
     ┠─ To allow: run again with --allow-{name}\n\
     ┗ Allow? [y/n/a] (y = yes, n = no, a = allow all {name}) "
  );
  let _ = std::io::stderr().flush();
  // Drop keystrokes and injected input that arrived before the question.
  // A failure here still reads the next line.
  discard_pending_input();
  let mut line = String::new();
  if std::io::stdin().read_line(&mut line).is_err() {
    return None;
  }
  Some(match line.trim().to_ascii_lowercase().as_str() {
    "y" | "yes" => Answer::Once,
    "a" | "all" => Answer::All,
    _ => Answer::Deny,
  })
}

/// Discard unread terminal input so it cannot answer the prompt.
fn discard_pending_input() {
  #[cfg(unix)]
  discard_fd(libc::STDIN_FILENO);
  #[cfg(windows)]
  {
    use windows_sys::Win32::System::Console::{
      FlushConsoleInputBuffer, GetStdHandle, STD_INPUT_HANDLE,
    };
    // SAFETY: the process standard-input handle. This clears a console
    // input queue and fails on any other handle. The prompt is still read.
    unsafe {
      FlushConsoleInputBuffer(GetStdHandle(STD_INPUT_HANDLE));
    }
  }
}

/// `TCIFLUSH` on `fd`. A failure leaves the next line to be read.
#[cfg(unix)]
fn discard_fd(fd: i32) {
  // SAFETY: `fd` is a terminal descriptor owned by this process. `TCIFLUSH`
  // discards input that has been received and not read.
  unsafe {
    libc::tcflush(fd, libc::TCIFLUSH);
  }
}

fn lexical_absolute(path: &Path) -> Result<PathBuf, OpError> {
  let absolute = if path.is_absolute() {
    path.to_path_buf()
  } else {
    let current = std::env::current_dir().map_err(|_| {
      OpError::dom("OperationError", "The current directory could not be read.")
    })?;
    current.join(path)
  };
  Ok(normalize(&absolute))
}

fn normalize(path: &Path) -> PathBuf {
  let mut out = PathBuf::new();
  for component in path.components() {
    match component {
      Component::CurDir => {}
      Component::ParentDir => pop_parent(&mut out),
      Component::Normal(name) => push_normal(&mut out, name),
      other => out.push(other.as_os_str()),
    }
  }
  out
}

fn pop_parent(out: &mut PathBuf) {
  if matches!(out.components().next_back(), Some(Component::Normal(_))) {
    out.pop();
  }
}

fn push_normal(out: &mut PathBuf, name: &OsStr) {
  #[cfg(windows)]
  match interpret_normal(name) {
    Interpreted::Current => {}
    Interpreted::Parent => pop_parent(out),
    Interpreted::Name(name) => out.push(name),
  }
  #[cfg(not(windows))]
  out.push(name);
}

struct PathCandidates {
  paths: Vec<PathBuf>,
  /// False when the walk could not name the file the OS would open.
  resolved: bool,
}

fn path_candidates(
  path: &Path,
  link: PathLink,
) -> Result<PathCandidates, OpError> {
  let absolute = absolute_path(path)?;
  let lexical = normalize(&absolute);
  let mut paths = vec![lexical.clone()];
  push_canonical(&mut paths, &lexical, link);
  // The operating system follows a symlink and then applies `..`. Collapsing
  // `..` first would erase the link and allow a directory grant to escape.
  // `Entry` still follows a parent symlink. It leaves a final symlink as
  // that directory entry, so the grant covers the link and not its target.
  let Some(resolved) = operating_path(&absolute, link) else {
    return Ok(PathCandidates {
      paths,
      resolved: false,
    });
  };
  let resolved = normalize(&strip_verbatim(resolved));
  if !paths.iter().any(|item| paths_equal(item, &resolved)) {
    paths.push(resolved.clone());
  }
  push_canonical(&mut paths, &resolved, link);
  Ok(PathCandidates {
    paths,
    resolved: true,
  })
}

/// The file a prompt yes or a dialog grant names. This is the lexical path
/// and its canonical spelling, not a different file reached by `..` after a
/// symlink.
fn session_candidates(
  path: &Path,
  link: PathLink,
) -> Result<Vec<PathBuf>, OpError> {
  let lexical = lexical_absolute(path)?;
  let mut found = vec![lexical.clone()];
  push_canonical(&mut found, &lexical, link);
  Ok(found)
}

fn record_session_paths(
  gate: &mut Gate,
  path: &Path,
  link: PathLink,
  allow: bool,
) -> Result<(), OpError> {
  for candidate in session_candidates(path, link)? {
    if allow {
      gate
        .session_deny_paths
        .retain(|item| !paths_equal(item, &candidate));
      gate.session_allow_paths.push(candidate);
    } else {
      gate
        .session_allow_paths
        .retain(|item| !paths_equal(item, &candidate));
      gate.session_deny_paths.push(candidate);
    }
  }
  Ok(())
}

fn absolute_path(path: &Path) -> Result<PathBuf, OpError> {
  if path.is_absolute() {
    return Ok(path.to_path_buf());
  }
  let current = match run_base() {
    Some(base) if base.is_absolute() => base,
    Some(base) => current_dir()?.join(base),
    None => current_dir()?,
  };
  Ok(current.join(path))
}

fn current_dir() -> Result<PathBuf, OpError> {
  std::env::current_dir().map_err(|_| {
    OpError::dom("OperationError", "The current directory could not be read.")
  })
}

/// The absolute path a check and the following operating-system call share.
///
/// A relative path is joined to the current directory. `.` and `..` stay in
/// the result, so opening it still follows a symlink. A later `chdir` does
/// not retarget the call.
pub(crate) fn anchor_path(path: &Path) -> Result<PathBuf, OpError> {
  if path.as_os_str().is_empty() {
    return current_dir();
  }
  if path.is_absolute() {
    return Ok(path.to_path_buf());
  }
  // `C:name` has a prefix and is not absolute. Joining it would drop the
  // current directory. `std::path::absolute` asks the operating system for
  // that drive's directory. `Path::prefix` is private.
  #[cfg(windows)]
  if path
    .components()
    .next()
    .is_some_and(|component| matches!(component, Component::Prefix(_)))
  {
    return std::path::absolute(path).map_err(|_| {
      OpError::dom("OperationError", "The current directory could not be read.")
    });
  }
  Ok(current_dir()?.join(path))
}

fn push_canonical(found: &mut Vec<PathBuf>, path: &Path, link: PathLink) {
  let real = if link == PathLink::Entry && final_is_symlink(path) {
    entry_spelling(path)
  } else {
    flexible_canonical(path)
  };
  let Some(real) = real else {
    return;
  };
  let real = normalize(&strip_verbatim(real));
  if !found.iter().any(|item| paths_equal(item, &real)) {
    found.push(real);
  }
}

fn operating_path(path: &Path, link: PathLink) -> Option<PathBuf> {
  // Canonicalize follows a final symlink. An entry check must not, or the
  // target would become a candidate.
  if (link == PathLink::Follow || !final_is_symlink(path))
    && let Ok(real) = std::fs::canonicalize(path)
  {
    return Some(real);
  }
  // NotFound keeps a lexical tail. Any other error, including ENOTDIR,
  // means the walk cannot name the file. The caller denies that shape
  // instead of reporting a distinct resolution error.
  walk_operating(path, 0, link).ok()
}

/// Follows symlinks and applies `..` to the resolved path. A missing
/// component cannot be a symlink, so the rest of the path is kept as named.
fn walk_operating(
  path: &Path,
  depth: u32,
  link: PathLink,
) -> Result<PathBuf, ()> {
  if depth > 40 {
    return Err(());
  }
  let components: Vec<Component> = path.components().collect();
  let mut out = PathBuf::new();
  let mut index = 0;
  while index < components.len() {
    match components[index] {
      Component::CurDir => {}
      Component::ParentDir => {
        out.pop();
      }
      Component::Prefix(_) | Component::RootDir => out.push(components[index]),
      Component::Normal(name) => {
        let candidate = out.join(name);
        match std::fs::symlink_metadata(&candidate) {
          Ok(meta) if meta.file_type().is_symlink() => {
            let final_entry = link == PathLink::Entry
              && components[index + 1..]
                .iter()
                .all(|component| matches!(component, Component::CurDir));
            if final_entry {
              out.push(name);
              return Ok(out);
            }
            let target = std::fs::read_link(&candidate).map_err(|_| ())?;
            let mut rest = if target.is_absolute() {
              target
            } else {
              out.join(target)
            };
            for later in components.iter().skip(index + 1) {
              rest.push(later);
            }
            return walk_operating(&rest, depth + 1, link);
          }
          Ok(meta) => {
            out.push(name);
            // A file cannot contain a normal child. Windows reports that
            // child as NotFound. `..` still leaves the file.
            if !meta.file_type().is_dir() {
              for component in components.iter().skip(index + 1) {
                match component {
                  Component::CurDir => {}
                  Component::ParentDir => break,
                  _ => return Err(()),
                }
              }
            }
          }
          Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            out.push(name);
            for later in components.iter().skip(index + 1) {
              match later {
                Component::ParentDir => {
                  out.pop();
                }
                Component::CurDir => {}
                other => out.push(other),
              }
            }
            return Ok(out);
          }
          Err(_) => return Err(()),
        }
      }
    }
    index += 1;
  }
  Ok(out)
}

/// The parent and final name, ignoring a trailing `.`.
fn split_final(path: &Path) -> Option<(PathBuf, std::ffi::OsString)> {
  let components: Vec<Component> = path.components().collect();
  let mut end = components.len();
  while end > 0 && matches!(components[end - 1], Component::CurDir) {
    end -= 1;
  }
  let Component::Normal(name) = components.get(end.wrapping_sub(1))? else {
    return None;
  };
  let mut parent = PathBuf::new();
  for component in &components[..end - 1] {
    parent.push(component.as_os_str());
  }
  if parent.as_os_str().is_empty() {
    parent.push(".");
  }
  Some((parent, name.to_os_string()))
}

fn final_is_symlink(path: &Path) -> bool {
  let Some((parent, name)) = split_final(path) else {
    return false;
  };
  std::fs::symlink_metadata(parent.join(name))
    .is_ok_and(|meta| meta.file_type().is_symlink())
}

/// Canonical spelling of the directory entry. A final symlink is not resolved.
fn entry_spelling(path: &Path) -> Option<PathBuf> {
  let (parent, name) = split_final(path)?;
  let parent = std::fs::canonicalize(parent).ok()?;
  Some(parent.join(name))
}

/// Stored deny spellings of one entry, without following a final symlink.
fn same_entry_spellings(path: &Path) -> Vec<PathBuf> {
  let mut forms = vec![normalize(&strip_verbatim(path.to_path_buf()))];
  if let Some(real) = entry_spelling(path) {
    let real = normalize(&strip_verbatim(real));
    if !forms.iter().any(|item| paths_equal(item, &real)) {
      forms.push(real);
    }
  }
  forms
}

fn flexible_canonical(path: &Path) -> Option<PathBuf> {
  if let Ok(real) = std::fs::canonicalize(path) {
    return Some(real);
  }
  let parent = path.parent()?;
  let name = path.file_name()?;
  if parent.as_os_str().is_empty() {
    return None;
  }
  let parent = std::fs::canonicalize(parent).ok()?;
  Some(parent.join(name))
}

fn strip_verbatim(path: PathBuf) -> PathBuf {
  #[cfg(windows)]
  {
    let text = path.to_string_lossy();
    if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
      return PathBuf::from(format!(r"\\{rest}"));
    }
    if let Some(rest) = text.strip_prefix(r"\\?\") {
      return PathBuf::from(rest);
    }
  }
  path
}

fn paths_equal(left: &Path, right: &Path) -> bool {
  let left = strip_verbatim(left.to_path_buf());
  let right = strip_verbatim(right.to_path_buf());
  let mut left = left.components();
  let mut right = right.components();
  loop {
    match (left.next(), right.next()) {
      (None, None) => return true,
      (Some(a), Some(b)) if component_eq(a, b) => {}
      _ => return false,
    }
  }
}

fn path_covers(grant: &Path, request: &Path) -> bool {
  // `\\?\` and `\\?\UNC\` are two spellings of one Windows path.
  let grant = strip_verbatim(grant.to_path_buf());
  let request = strip_verbatim(request.to_path_buf());
  let mut grant = grant.components();
  let mut request = request.components();
  loop {
    match (grant.next(), request.next()) {
      (None, _) => return true,
      (Some(_), None) => return false,
      (Some(left), Some(right)) if component_eq(left, right) => {}
      _ => return false,
    }
  }
}

fn component_eq(left: Component<'_>, right: Component<'_>) -> bool {
  match (left, right) {
    (Component::Prefix(left), Component::Prefix(right)) => {
      os_eq(left.as_os_str(), right.as_os_str())
    }
    (Component::RootDir, Component::RootDir)
    | (Component::CurDir, Component::CurDir)
    | (Component::ParentDir, Component::ParentDir) => true,
    (Component::Normal(left), Component::Normal(right)) => {
      os_eq(compared_name(left).as_ref(), compared_name(right).as_ref())
    }
    _ => false,
  }
}

fn compared_name(name: &OsStr) -> Cow<'_, OsStr> {
  #[cfg(windows)]
  {
    let name = match interpret_normal(name) {
      Interpreted::Current => OsStr::new(".").to_os_string(),
      Interpreted::Parent => OsStr::new("..").to_os_string(),
      Interpreted::Name(name) => name,
    };
    Cow::Owned(name)
  }
  #[cfg(not(windows))]
  {
    Cow::Borrowed(name)
  }
}

/// Win32 deletes trailing dots and spaces, and `name:stream` is `name`.
#[cfg(windows)]
enum Interpreted {
  Current,
  Parent,
  Name(std::ffi::OsString),
}

#[cfg(windows)]
fn interpret_normal(name: &OsStr) -> Interpreted {
  use std::os::windows::ffi::{OsStrExt, OsStringExt};
  let mut units: Vec<u16> = name.encode_wide().collect();
  if let Some(index) = units.iter().position(|unit| *unit == u16::from(b':')) {
    units.truncate(index);
  }
  // Stop at `.` or `..`, so `.. ` compares as `..` instead of becoming empty.
  let dot = u16::from(b'.');
  while units
    .last()
    .is_some_and(|unit| *unit == dot || *unit == u16::from(b' '))
    && units.as_slice() != [dot]
    && units.as_slice() != [dot, dot]
  {
    units.pop();
  }
  if units.is_empty() {
    return Interpreted::Name(name.to_os_string());
  }
  if units.as_slice() == [u16::from(b'.')] {
    return Interpreted::Current;
  }
  if units.as_slice() == [u16::from(b'.'), u16::from(b'.')] {
    return Interpreted::Parent;
  }
  Interpreted::Name(std::ffi::OsString::from_wide(&units))
}

fn os_eq(left: &std::ffi::OsStr, right: &std::ffi::OsStr) -> bool {
  if cfg!(any(windows, target_os = "macos")) {
    left.eq_ignore_ascii_case(right)
  } else {
    left == right
  }
}

fn resolve_path(
  kind: Kind,
  root: &Path,
  text: &str,
) -> Result<PathBuf, String> {
  let text = text.trim();
  if text.is_empty() {
    return Err(format!(
      "A {} permission path must not be empty.",
      kind.name()
    ));
  }
  if text.contains('\0') {
    return Err(format!(
      "A {} permission path must not contain NUL.",
      kind.name()
    ));
  }
  let path = Path::new(text);
  let joined = if path.is_absolute() {
    path.to_path_buf()
  } else {
    root.join(path)
  };
  Ok(normalize(&joined))
}

fn parse_net(text: &str) -> Result<NetRule, String> {
  let text = text.trim();
  if text.is_empty() {
    return Err("A net permission must not be empty.".to_string());
  }
  if let Some(rest) = text.strip_prefix('[') {
    let Some((host, after)) = rest.split_once(']') else {
      return Err(format!("The net permission \"{text}\" is invalid."));
    };
    if host.is_empty() {
      return Err(format!("The net permission \"{text}\" is invalid."));
    }
    let port = if after.is_empty() {
      None
    } else if let Some(port) = after.strip_prefix(':') {
      Some(parse_port(port)?)
    } else {
      return Err(format!("The net permission \"{text}\" is invalid."));
    };
    return Ok(NetRule {
      host: normalize_host(host),
      port,
    });
  }
  if let Some((host, port)) = text.rsplit_once(':')
    && !host.is_empty()
    && !host.contains(':')
    && !port.is_empty()
    && port.chars().all(|char| char.is_ascii_digit())
  {
    return Ok(NetRule {
      host: normalize_host(host),
      port: Some(parse_port(port)?),
    });
  }
  Ok(NetRule {
    host: normalize_host(text),
    port: None,
  })
}

fn parse_port(text: &str) -> Result<u16, String> {
  let port = text
    .parse::<u16>()
    .map_err(|_| format!("The net permission port \"{text}\" is invalid."))?;
  Ok(port)
}

fn normalize_host(host: &str) -> String {
  let host = host.trim();
  // `url`'s host for IPv6 is `[::1]`. Rules are stored without brackets.
  let bare = if let Some(inner) = host
    .strip_prefix('[')
    .and_then(|rest| rest.strip_suffix(']'))
    && !inner.is_empty()
  {
    inner.trim()
  } else {
    host
  };
  let lower = bare.to_ascii_lowercase();
  // A parseable address has one spelling. `2001:0db8::1` and
  // `::ffff:127.0.0.1` compare as `2001:db8::1` and `127.0.0.1`.
  // A zone id does not parse and stays in the string.
  match lower.parse::<IpAddr>() {
    Ok(addr) => canonical_ip(addr).to_string(),
    Err(_) => domain_host(&lower).unwrap_or(lower),
  }
}

/// ASCII spelling of a domain. A trailing DNS root dot is removed, and a
/// Unicode name becomes its A-label. An address is handled before this runs,
/// so `localhost` stays a name and is not `127.0.0.1`.
fn domain_host(host: &str) -> Option<String> {
  if host.is_empty()
    || host.contains(':')
    || host.contains('/')
    || host.contains('\\')
  {
    return None;
  }
  let url::Host::Domain(domain) = url::Host::parse(host).ok()? else {
    return None;
  };
  let domain = domain.trim_end_matches('.').to_ascii_lowercase();
  if domain.is_empty() {
    None
  } else {
    Some(domain)
  }
}

fn canonical_ip(addr: IpAddr) -> IpAddr {
  match addr {
    IpAddr::V4(addr) => IpAddr::V4(addr),
    IpAddr::V6(addr) => match addr.to_ipv4_mapped() {
      Some(mapped) => IpAddr::V4(mapped),
      None => IpAddr::V6(addr),
    },
  }
}

/// The host to check when `addr` is not a globally routable destination.
fn restricted_host(addr: IpAddr) -> Option<String> {
  let addr = canonical_ip(addr);
  if !is_restricted_ip(addr) {
    return None;
  }
  Some(addr.to_string())
}

fn is_restricted_ip(addr: IpAddr) -> bool {
  match addr {
    IpAddr::V4(addr) => {
      addr.is_loopback()
        || addr.is_private()
        || addr.is_link_local()
        || addr.is_broadcast()
        || addr.is_unspecified()
        || addr.is_multicast()
        || is_shared_v4(addr)
    }
    IpAddr::V6(addr) => {
      addr.is_loopback()
        || addr.is_unicast_link_local()
        || addr.is_unique_local()
        || addr.is_unspecified()
        || addr.is_multicast()
        || is_site_local(addr)
        || is_nat64(addr)
    }
  }
}

/// `100.64.0.0/10`, the shared carrier-grade range. `Ipv4Addr::is_shared`
/// is unstable.
fn is_shared_v4(addr: Ipv4Addr) -> bool {
  let [first, second, _, _] = addr.octets();
  first == 100 && (second & 0b1100_0000) == 0b0100_0000
}

/// `fec0::/10`, the deprecated site-local prefix.
fn is_site_local(addr: Ipv6Addr) -> bool {
  let [first, second, ..] = addr.octets();
  first == 0xfe && (second & 0xc0) == 0xc0
}

/// `64:ff9b::/96`, the well-known NAT64 prefix.
fn is_nat64(addr: Ipv6Addr) -> bool {
  let octets = addr.octets();
  octets[..12] == [0x00, 0x64, 0xff, 0x9b, 0, 0, 0, 0, 0, 0, 0, 0]
}

fn format_host(host: &str, port: Option<u16>) -> String {
  let host = normalize_host(host);
  let shown = if host.contains(':') {
    format!("[{host}]")
  } else {
    host
  };
  match port {
    Some(port) => format!("{shown}:{port}"),
    None => shown,
  }
}

fn host_eq(left: &str, right: &str) -> bool {
  left.eq_ignore_ascii_case(right)
}

fn net_covers(rule: &NetRule, host: &str, port: u16) -> bool {
  let host = normalize_host(host);
  if rule.host != "*" && !host_eq(&rule.host, &host) {
    return false;
  }
  rule.port.is_none_or(|value| value == port)
}

fn net_host_hit(rules: &[NetRule], host: &str) -> bool {
  let host = normalize_host(host);
  rules.iter().any(|rule| {
    rule.port.is_none() && (rule.host == "*" || host_eq(&rule.host, &host))
  })
}

fn parse_env(text: &str) -> Result<String, String> {
  let text = text.trim();
  if text.is_empty() {
    return Err("An env permission must not be empty.".to_string());
  }
  if text.contains('\0') {
    return Err("An env permission must not contain NUL.".to_string());
  }
  Ok(text.to_string())
}

fn parse_run(root: &Path, text: &str) -> Result<String, String> {
  let text = text.trim();
  if text.is_empty() {
    return Err("A run permission must not be empty.".to_string());
  }
  if text.contains('\0') {
    return Err("A run permission must not contain NUL.".to_string());
  }
  if text.contains('/') || text.contains('\\') {
    Ok(resolve_path(Kind::Run, root, text)?.display().to_string())
  } else {
    Ok(text.to_string())
  }
}

fn env_covers(rule: &str, name: &str) -> bool {
  if let Some(prefix) = rule.strip_suffix('*') {
    return env_starts(name, prefix);
  }
  env_eq(rule, name)
}

fn env_eq(left: &str, right: &str) -> bool {
  if cfg!(windows) {
    left.eq_ignore_ascii_case(right)
  } else {
    left == right
  }
}

fn env_starts(name: &str, prefix: &str) -> bool {
  if cfg!(windows) {
    name.len() >= prefix.len()
      && name
        .get(..prefix.len())
        .is_some_and(|head| head.eq_ignore_ascii_case(prefix))
  } else {
    name.starts_with(prefix)
  }
}

fn run_covers(rule: &str, command: &str) -> bool {
  if is_command_path(rule) {
    // A bare command is found through `PATH`. Comparing it with `cwd/name`
    // would let a path grant run a different file.
    if !is_command_path(command) {
      return false;
    }
    let Ok(rule_path) = lexical_absolute(Path::new(rule)) else {
      return false;
    };
    // The grant matches only the file `exec` would run. A symlink, or a
    // `..` that follows one, is a different path and does not match.
    let Some(executed) = executed_path(Path::new(command)) else {
      return false;
    };
    return paths_equal(&rule_path, &executed);
  }
  // A bare grant matches a bare command, not a path with that file name.
  if is_command_path(command) {
    return false;
  }
  command_names_equal(rule, command)
}

thread_local! {
  static RUN_BASE: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
}

/// Resolves a relative program from `spawn`'s `cwd` for this check only.
struct RunBase {
  previous: Option<PathBuf>,
}

impl RunBase {
  fn set(base: &Path) -> Self {
    let previous = RUN_BASE.with(|slot| slot.replace(Some(base.to_path_buf())));
    Self { previous }
  }
}

impl Drop for RunBase {
  fn drop(&mut self) {
    let previous = self.previous.take();
    RUN_BASE.with(|slot| *slot.borrow_mut() = previous);
  }
}

fn run_base() -> Option<PathBuf> {
  RUN_BASE.with(|slot| slot.borrow().clone())
}

fn executed_path(path: &Path) -> Option<PathBuf> {
  let absolute = absolute_path(path).ok()?;
  let resolved = operating_path(&absolute, PathLink::Follow)?;
  Some(normalize(&strip_verbatim(resolved)))
}

/// Absolute program for a scoped path command.
///
/// The file the operating system would execute is used when it can be
/// named. A missing file is still absolute, from the current directory or
/// from `spawn`'s `cwd`, so the child does not resolve a relative path
/// after `chdir`. `run` granted as a whole does not use this.
#[cfg(any(test, feature = "child"))]
fn absolute_command(path: &Path) -> Result<PathBuf, OpError> {
  if let Some(found) = executed_path(path) {
    return Ok(found);
  }
  if path.as_os_str().is_empty() {
    return current_dir();
  }
  if path.is_absolute() {
    return Ok(path.to_path_buf());
  }
  // A drive-relative path such as `C:name` is resolved from that drive's
  // directory. A normal relative path uses `spawn`'s `cwd` through
  // [`absolute_path`].
  #[cfg(windows)]
  if path
    .components()
    .next()
    .is_some_and(|component| matches!(component, Component::Prefix(_)))
  {
    return std::path::absolute(path).map_err(|_| {
      OpError::dom("OperationError", "The current directory could not be read.")
    });
  }
  absolute_path(path)
}

fn is_command_path(text: &str) -> bool {
  text.contains('/') || text.contains('\\')
}

/// The string a prompt answer or `revoke` stores for `name`.
///
/// A run command that contains a separator is the absolute file resolved
/// now. `absolute_path` uses the current directory, or `spawn`'s cwd when
/// this check set it. The executed file is stored when it is that path or
/// its canonical spelling, so `/var` and `/private/var` stay one grant. A
/// different file reached by a symlink and then `..` is not stored. A bare
/// command and an env name stay the caller's string.
fn session_name(kind: Kind, name: &str) -> Result<String, OpError> {
  if kind != Kind::Run || !is_command_path(name) {
    return Ok(name.to_string());
  }
  let lexical = normalize(&absolute_path(Path::new(name))?);
  let mut allowed = vec![lexical.clone()];
  push_canonical(&mut allowed, &lexical, PathLink::Follow);
  if let Some(executed) = executed_path(Path::new(name))
    && allowed.iter().any(|item| paths_equal(item, &executed))
  {
    return Ok(executed.display().to_string());
  }
  Ok(lexical.display().to_string())
}

fn record_session_name(
  gate: &mut Gate,
  kind: Kind,
  name: &str,
  allow: bool,
) -> Result<(), OpError> {
  let stored = session_name(kind, name)?;
  gate
    .session_allow_names
    .retain(|item| !session_name_hit(kind, item, name, &stored));
  gate
    .session_deny_names
    .retain(|item| !session_name_hit(kind, item, name, &stored));
  if allow {
    gate.session_allow_names.push(stored);
  } else {
    gate.session_deny_names.push(stored);
  }
  Ok(())
}

/// True when `item` is this answer's string or another spelling of the same
/// requested file. A later directory does not make `./tool` cover a file
/// granted earlier. A symlink followed by `..` does not drop a grant of the
/// other file, because that comparison uses the stored path.
fn session_name_hit(
  kind: Kind,
  item: &str,
  requested: &str,
  stored: &str,
) -> bool {
  if item == requested || item == stored {
    return true;
  }
  if kind != Kind::Run || !is_command_path(requested) {
    return false;
  }
  run_covers(item, stored) || run_covers(stored, item)
}

fn command_names_equal(rule: &str, command: &str) -> bool {
  let owned;
  let command = match Path::new(command).file_name() {
    Some(name) => {
      owned = name.to_string_lossy().into_owned();
      owned.as_str()
    }
    None => command,
  };
  if cfg!(windows) {
    strip_exe(rule).eq_ignore_ascii_case(strip_exe(command))
  } else {
    rule == command
  }
}

fn strip_exe(name: &str) -> &str {
  let bytes = name.as_bytes();
  if bytes.len() > 4 && bytes[bytes.len() - 4] == b'.' {
    let suffix = &name[name.len() - 3..];
    if suffix.eq_ignore_ascii_case("exe") {
      return &name[..name.len() - 4];
    }
  }
  name
}

pub(crate) fn install(scope: &mut v8::PinScope, yuzuko: v8::Local<v8::Object>) {
  let permissions = v8::Object::new(scope);
  define(scope, permissions, "query", query);
  define(scope, permissions, "querySync", query_sync);
  define(scope, permissions, "request", request);
  define(scope, permissions, "requestSync", request_sync);
  define(scope, permissions, "revoke", revoke);
  define(scope, permissions, "revokeSync", revoke_sync);
  crate::js::set_value(scope, yuzuko, "permissions", permissions.into());
}

fn define(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
  callback: impl v8::MapFnTo<v8::FunctionCallback>,
) {
  let function = v8::Function::builder(callback)
    .length(1)
    .constructor_behavior(v8::ConstructorBehavior::Throw)
    .build(scope)
    .unwrap_or_else(|| panic!("create function {name}"));
  let key = v8::String::new(scope, name).unwrap();
  function.set_name(key);
  object.set(scope, key.into(), function.into());
}

struct Desc {
  kind: Kind,
  detail: Detail,
}

fn query(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut ret: v8::ReturnValue,
) {
  let Ok(state) = permission_state(scope, args.get(0), false) else {
    return;
  };
  let object = state_object(scope, state);
  ret.set(resolved(scope, object.into()));
}

fn query_sync(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut ret: v8::ReturnValue,
) {
  let Ok(state) = permission_state(scope, args.get(0), false) else {
    return;
  };
  ret.set(state_object(scope, state).into());
}

fn request(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut ret: v8::ReturnValue,
) {
  let Ok(state) = permission_state(scope, args.get(0), true) else {
    return;
  };
  let object = state_object(scope, state);
  ret.set(resolved(scope, object.into()));
}

fn request_sync(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut ret: v8::ReturnValue,
) {
  let Ok(state) = permission_state(scope, args.get(0), true) else {
    return;
  };
  ret.set(state_object(scope, state).into());
}

fn permission_state(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
  prompt_now: bool,
) -> Result<State, ()> {
  let desc = match descriptor(scope, value) {
    Ok(desc) => desc,
    Err(error) => {
      error.throw(scope);
      return Err(());
    }
  };
  match interact(desc.kind, &desc.detail, prompt_now) {
    Ok(state) => Ok(state),
    Err(error) => {
      error.throw(scope);
      Err(())
    }
  }
}

fn revoke(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut ret: v8::ReturnValue,
) {
  if let Err(error) = descriptor(scope, args.get(0))
    .and_then(|desc| revoke_detail(desc.kind, &desc.detail).map(|_| ()))
  {
    error.throw(scope);
    return;
  }
  ret.set(resolved(scope, v8::undefined(scope).into()));
}

fn revoke_sync(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut _ret: v8::ReturnValue,
) {
  if let Err(error) = descriptor(scope, args.get(0))
    .and_then(|desc| revoke_detail(desc.kind, &desc.detail).map(|_| ()))
  {
    error.throw(scope);
  }
}

fn descriptor(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<Desc, OpError> {
  if value.is_null_or_undefined() || !value.is_object() || value.is_array() {
    return Err(OpError::type_(
      "The permission descriptor must be an object.",
    ));
  }
  let object = v8::Local::<v8::Object>::try_from(value).map_err(|_| {
    OpError::type_("The permission descriptor must be an object.")
  })?;
  let name = required_string(
    scope,
    object,
    "name",
    "The permission name must be a string.",
  )?;
  let kind = Kind::parse(&name)
    .ok_or_else(|| OpError::type_("The permission name is not supported."))?;
  let field = kind.detail_field();
  let message = match kind {
    Kind::Read | Kind::Write | Kind::Ffi => {
      "The permission path must be a string."
    }
    Kind::Net => "The permission host must be a string.",
    Kind::Env => "The permission variable must be a string.",
    Kind::Run => "The permission command must be a string.",
  };
  let text = optional_string(scope, object, field, message)?;
  let detail = match text {
    None => Detail::Any,
    Some(text) if text.is_empty() => {
      return Err(OpError::type_(message));
    }
    Some(text) => parse_detail(kind, &text)?,
  };
  Ok(Desc { kind, detail })
}

fn parse_detail(kind: Kind, text: &str) -> Result<Detail, OpError> {
  match kind {
    Kind::Read | Kind::Write | Kind::Ffi => {
      if text.contains('\0') {
        return Err(OpError::type_(
          "The permission path must not contain NUL.",
        ));
      }
      Ok(Detail::Path(PathBuf::from(text)))
    }
    Kind::Net => {
      let rule = parse_net(text).map_err(OpError::type_)?;
      Ok(Detail::Net {
        host: rule.host,
        port: rule.port,
      })
    }
    Kind::Env | Kind::Run => {
      if text.contains('\0') {
        return Err(OpError::type_(
          "The permission name must not contain NUL.",
        ));
      }
      Ok(Detail::Name(text.to_string()))
    }
  }
}

fn required_string(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
  message: &str,
) -> Result<String, OpError> {
  match optional_string(scope, object, name, message)? {
    Some(text) => Ok(text),
    None => Err(OpError::type_(message)),
  }
}

fn optional_string(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
  message: &str,
) -> Result<Option<String>, OpError> {
  let key = v8::String::new(scope, name).ok_or_else(|| {
    OpError::type_("The permission descriptor could not be read.")
  })?;
  let value = object.get(scope, key.into()).ok_or_else(|| {
    OpError::type_("The permission descriptor could not be read.")
  })?;
  if value.is_undefined() {
    return Ok(None);
  }
  if !value.is_string() {
    return Err(OpError::type_(message));
  }
  let text = value.to_string(scope).ok_or_else(|| {
    OpError::type_("The permission descriptor could not be read.")
  })?;
  Ok(Some(text.to_rust_string_lossy(scope)))
}

fn state_object<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  state: State,
) -> v8::Local<'s, v8::Object> {
  let object = v8::Object::new(scope);
  let text = v8::String::new(scope, state.as_str()).unwrap();
  crate::js::set_value(scope, object, "state", text.into());
  object
}

fn resolved<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  value: v8::Local<'s, v8::Value>,
) -> v8::Local<'s, v8::Value> {
  let Some(resolver) = v8::PromiseResolver::new(scope) else {
    return value;
  };
  resolver.resolve(scope, value);
  resolver.get_promise(scope).into()
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn an_open_process_has_no_path_policy() {
    assert!(permissions_are_open());
  }

  #[test]
  fn a_missing_relative_command_stays_absolute() {
    let root = std::env::temp_dir().join(format!(
      "yuzuko-perm-anchor-{}-{}",
      std::process::id(),
      std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
    ));
    let _ = std::fs::remove_dir_all(&root);
    let sub = root.join("sub");
    std::fs::create_dir_all(sub.join("nested")).unwrap();
    let sub = std::fs::canonicalize(&sub).unwrap();
    let _base = RunBase::set(&sub);
    let missing = absolute_command(Path::new("nested/missing")).unwrap();
    assert!(missing.is_absolute());
    assert!(paths_equal(&missing, &sub.join("nested").join("missing")));
    let tool = sub.join("nested").join("tool");
    std::fs::write(&tool, b"x").unwrap();
    let found = absolute_command(Path::new("nested/tool")).unwrap();
    assert!(paths_equal(&found, &std::fs::canonicalize(&tool).unwrap()));
    let _ = std::fs::remove_dir_all(&root);
  }

  #[test]
  fn an_entry_check_names_the_link_and_a_tree_deny_blocks_remove() {
    let root = std::env::temp_dir().join(format!(
      "yuzuko-perm-entry-{}-{}",
      std::process::id(),
      std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
    ));
    let _ = std::fs::remove_dir_all(&root);
    let project = root.join("project");
    let outside = root.join("outside");
    std::fs::create_dir_all(project.join("keep")).unwrap();
    std::fs::create_dir_all(project.join("secret")).unwrap();
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join("secret.txt"), b"SECRET").unwrap();
    std::fs::write(project.join("keep").join("note"), b"n").unwrap();
    std::fs::write(project.join("secret").join("hidden"), b"h").unwrap();
    let project = std::fs::canonicalize(&project).unwrap();
    #[cfg(unix)]
    {
      let outside = std::fs::canonicalize(&outside).unwrap();
      std::os::unix::fs::symlink(
        outside.join("secret.txt"),
        project.join("link"),
      )
      .unwrap();
    }
    let mut spec = PermissionSpec::default();
    spec
      .allow("read", Some(vec![project.display().to_string()]))
      .unwrap();
    spec
      .allow("write", Some(vec![project.display().to_string()]))
      .unwrap();
    spec
      .deny(
        "write",
        Some(vec![project.join("secret").display().to_string()]),
      )
      .unwrap();
    let sandbox = Sandbox::compile(&spec, &project).unwrap();
    assert_eq!(
      sandbox
        .classify(Kind::Write, &Detail::Path(project.clone()))
        .unwrap(),
      State::Granted
    );
    assert_eq!(
      sandbox
        .classify(Kind::Write, &Detail::Tree(project.clone()))
        .unwrap(),
      State::Denied
    );
    assert_eq!(
      sandbox
        .classify(
          Kind::Write,
          &Detail::Entry(project.join("keep").join("note")),
        )
        .unwrap(),
      State::Granted
    );
    assert_eq!(
      sandbox
        .classify(
          Kind::Write,
          &Detail::Entry(project.join("secret").join("hidden")),
        )
        .unwrap(),
      State::Denied
    );
    #[cfg(unix)]
    {
      let link = project.join("link");
      assert_eq!(
        sandbox
          .classify(Kind::Read, &Detail::Entry(link.clone()))
          .unwrap(),
        State::Granted
      );
      assert_eq!(
        sandbox
          .classify(Kind::Read, &Detail::Path(link.clone()))
          .unwrap(),
        State::Prompt
      );
      let through = link.join("..").join("secret.txt");
      assert_ne!(
        sandbox
          .classify(Kind::Read, &Detail::Entry(through))
          .unwrap(),
        State::Granted
      );
      assert_eq!(
        sandbox
          .classify(Kind::Write, &Detail::Entry(link.clone()))
          .unwrap(),
        State::Granted
      );
      assert_eq!(
        sandbox.classify(Kind::Write, &Detail::Tree(link)).unwrap(),
        State::Granted
      );
    }
    let _ = std::fs::remove_dir_all(&root);
  }

  #[test]
  fn a_directory_grant_covers_descendants_only() {
    let root = std::env::temp_dir();
    let root = std::fs::canonicalize(&root).unwrap_or(root);
    let project =
      root.join(format!("yuzuko-perm-grant-{}", std::process::id()));
    let mut spec = PermissionSpec::default();
    spec
      .allow("read", Some(vec![project.display().to_string()]))
      .unwrap();
    let sandbox = Sandbox::compile(&spec, &root).unwrap();
    let inside = sandbox
      .classify(Kind::Read, &Detail::Path(project.join("note")))
      .unwrap();
    let mut outside_name = project.file_name().unwrap().to_os_string();
    outside_name.push("2");
    let outside = sandbox
      .classify(
        Kind::Read,
        &Detail::Path(root.join(outside_name).join("note")),
      )
      .unwrap();
    let sibling = sandbox
      .classify(Kind::Read, &Detail::Path(root.join("yuzuko-perm-other")))
      .unwrap();
    assert_eq!(inside, State::Granted);
    assert_eq!(outside, State::Prompt);
    assert_eq!(sibling, State::Prompt);
  }

  #[test]
  fn deny_wins_over_a_broader_allow() {
    let mut spec = PermissionSpec::default();
    spec.allow("read", None).unwrap();
    spec.deny("read", Some(vec!["/etc".to_string()])).unwrap();
    let sandbox = Sandbox::compile(&spec, Path::new("/")).unwrap();
    let secret = sandbox
      .classify(Kind::Read, &Detail::Path(PathBuf::from("/etc/passwd")))
      .unwrap();
    let other = sandbox
      .classify(Kind::Read, &Detail::Path(PathBuf::from("/tmp/note")))
      .unwrap();
    assert_eq!(secret, State::Denied);
    assert_eq!(other, State::Granted);
  }

  #[test]
  fn net_without_a_port_allows_every_port() {
    let mut spec = PermissionSpec::default();
    spec
      .allow(
        "net",
        Some(vec!["example.com".to_string(), "*:443".to_string()]),
      )
      .unwrap();
    let sandbox = Sandbox::compile(&spec, Path::new("/")).unwrap();
    let https = sandbox
      .classify(
        Kind::Net,
        &Detail::Net {
          host: "Example.com".to_string(),
          port: Some(443),
        },
      )
      .unwrap();
    let other_port = sandbox
      .classify(
        Kind::Net,
        &Detail::Net {
          host: "example.com".to_string(),
          port: Some(80),
        },
      )
      .unwrap();
    let starred = sandbox
      .classify(
        Kind::Net,
        &Detail::Net {
          host: "other.test".to_string(),
          port: Some(443),
        },
      )
      .unwrap();
    let missed = sandbox
      .classify(
        Kind::Net,
        &Detail::Net {
          host: "other.test".to_string(),
          port: Some(80),
        },
      )
      .unwrap();
    assert_eq!(https, State::Granted);
    assert_eq!(other_port, State::Granted);
    assert_eq!(starred, State::Granted);
    assert_eq!(missed, State::Prompt);
  }

  #[test]
  fn env_prefix_and_run_basename_match() {
    let mut spec = PermissionSpec::default();
    spec
      .allow("env", Some(vec!["AWS_*".to_string(), "PATH".to_string()]))
      .unwrap();
    spec.allow("run", Some(vec!["git".to_string()])).unwrap();
    let sandbox = Sandbox::compile(&spec, Path::new("/")).unwrap();
    assert_eq!(
      sandbox
        .classify(Kind::Env, &Detail::Name("AWS_REGION".to_string()))
        .unwrap(),
      State::Granted
    );
    assert_eq!(
      sandbox
        .classify(Kind::Env, &Detail::Name("HOME".to_string()))
        .unwrap(),
      State::Prompt
    );
    assert_eq!(
      sandbox.classify(Kind::Env, &Detail::Any).unwrap(),
      State::Prompt
    );
    assert_eq!(
      sandbox
        .classify(Kind::Run, &Detail::Name("git".to_string()))
        .unwrap(),
      State::Granted
    );
    assert_eq!(
      sandbox
        .classify(Kind::Run, &Detail::Name("/usr/bin/git".to_string()))
        .unwrap(),
      State::Prompt
    );
    assert_eq!(
      sandbox
        .classify(Kind::Run, &Detail::Name("git.exe".to_string()))
        .unwrap(),
      if cfg!(windows) {
        State::Granted
      } else {
        State::Prompt
      }
    );
    assert_eq!(
      sandbox
        .classify(Kind::Run, &Detail::Name("curl".to_string()))
        .unwrap(),
      State::Prompt
    );
  }

  #[test]
  fn a_path_run_rule_matches_that_path_only() {
    let path = if cfg!(windows) {
      r"C:\Windows\System32\cmd.exe"
    } else {
      "/usr/bin/git"
    };
    let mut spec = PermissionSpec::default();
    spec.allow("run", Some(vec![path.to_string()])).unwrap();
    let sandbox = Sandbox::compile(&spec, Path::new("/")).unwrap();
    assert_eq!(
      sandbox
        .classify(Kind::Run, &Detail::Name(path.to_string()))
        .unwrap(),
      State::Granted
    );
    assert_eq!(
      sandbox
        .classify(Kind::Run, &Detail::Name("git".to_string()))
        .unwrap(),
      State::Prompt
    );
    let other = if cfg!(windows) {
      r"C:\other\cmd.exe"
    } else {
      "/tmp/git"
    };
    assert_eq!(
      sandbox
        .classify(Kind::Run, &Detail::Name(other.to_string()))
        .unwrap(),
      State::Prompt
    );
  }

  #[test]
  fn a_bare_command_does_not_match_a_path_run_rule() {
    let dir = std::env::temp_dir().join(format!(
      "yuzuko-perm-bare-{}-{}",
      std::process::id(),
      std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let dir = std::fs::canonicalize(&dir).unwrap();
    let tool = dir.join("tool");
    std::fs::write(&tool, b"echo\n").unwrap();
    let mut spec = PermissionSpec::default();
    spec
      .allow("run", Some(vec![tool.display().to_string()]))
      .unwrap();
    let sandbox = Sandbox::compile(&spec, &dir).unwrap();
    let _base = RunBase::set(&dir);
    assert_eq!(
      sandbox
        .classify(Kind::Run, &Detail::Name("tool".to_string()))
        .unwrap(),
      State::Prompt
    );
    assert_eq!(
      sandbox
        .classify(Kind::Run, &Detail::Name("./tool".to_string()))
        .unwrap(),
      State::Granted
    );
    assert_eq!(
      sandbox
        .classify(Kind::Run, &Detail::Name(tool.display().to_string()))
        .unwrap(),
      State::Granted
    );
    let _ = std::fs::remove_dir_all(&dir);
  }

  #[test]
  fn a_prompt_answer_for_a_wildcard_name_is_not_stored() {
    let root = std::env::temp_dir();
    let mut sandbox =
      Sandbox::compile(&PermissionSpec::default(), &root).unwrap();
    for name in ["*", "SECRET*"] {
      let asked = Detail::Name(name.to_string());
      assert_eq!(
        settle_answer(&mut sandbox, Kind::Env, &asked, Answer::Once).unwrap(),
        State::Granted
      );
      assert_eq!(
        settle_answer(&mut sandbox, Kind::Env, &asked, Answer::Deny).unwrap(),
        State::Denied
      );
    }
    let other = Detail::Name("SECRET".to_string());
    assert_eq!(sandbox.classify(Kind::Env, &other).unwrap(), State::Prompt);
    let any = Detail::Net {
      host: "*".to_string(),
      port: Some(80),
    };
    assert_eq!(
      settle_answer(&mut sandbox, Kind::Net, &any, Answer::Once).unwrap(),
      State::Granted
    );
    let host = Detail::Net {
      host: "example.com".to_string(),
      port: Some(80),
    };
    assert_eq!(sandbox.classify(Kind::Net, &host).unwrap(), State::Prompt);
    assert_eq!(
      settle_answer(&mut sandbox, Kind::Env, &other, Answer::Once).unwrap(),
      State::Granted
    );
    assert_eq!(sandbox.classify(Kind::Env, &other).unwrap(), State::Granted);
  }

  #[cfg(unix)]
  #[test]
  fn a_spawned_path_is_resolved_once_for_the_check_and_exec() {
    let root = std::env::temp_dir().join(format!(
      "yuzuko-perm-run-once-{}-{}",
      std::process::id(),
      std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let root = std::fs::canonicalize(&root).unwrap();
    let allowed = root.join("allowed.sh");
    let evil = root.join("evil.sh");
    std::fs::write(&allowed, b"#!/bin/sh\n").unwrap();
    std::fs::write(&evil, b"#!/bin/sh\n").unwrap();
    let link = root.join("l");
    std::os::unix::fs::symlink(&allowed, &link).unwrap();
    let mut spec = PermissionSpec::default();
    spec
      .allow("run", Some(vec![allowed.display().to_string()]))
      .unwrap();
    let sandbox = Sandbox::compile(&spec, &root).unwrap();
    let executed = absolute_command(&link).unwrap();
    assert!(paths_equal(&executed, &allowed));
    // The link now names another file. The checked path is still the file
    // that `exec` receives.
    std::fs::remove_file(&link).unwrap();
    std::os::unix::fs::symlink(&evil, &link).unwrap();
    let checked = Detail::Name(executed.display().to_string());
    assert_eq!(
      sandbox.classify(Kind::Run, &checked).unwrap(),
      State::Granted
    );
    assert!(paths_equal(&executed, &allowed));
    let swapped = Detail::Name(link.display().to_string());
    assert_ne!(
      sandbox.classify(Kind::Run, &swapped).unwrap(),
      State::Granted
    );
    let _ = std::fs::remove_dir_all(&root);
  }

  #[test]
  fn a_prompt_yes_for_a_relative_command_stays_on_that_file() {
    let root = std::env::temp_dir().join(format!(
      "yuzuko-perm-prompt-run-{}-{}",
      std::process::id(),
      std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
    ));
    let _ = std::fs::remove_dir_all(&root);
    let sub = root.join("sub");
    let other = root.join("other");
    std::fs::create_dir_all(&sub).unwrap();
    std::fs::create_dir_all(&other).unwrap();
    std::fs::write(sub.join("tool"), b"sub\n").unwrap();
    std::fs::write(other.join("tool"), b"other\n").unwrap();
    let sub = std::fs::canonicalize(&sub).unwrap();
    let other = std::fs::canonicalize(&other).unwrap();
    let sub_tool = sub.join("tool");
    let other_tool = other.join("tool");
    let mut sandbox =
      Sandbox::compile(&PermissionSpec::default(), &sub).unwrap();
    {
      let _base = RunBase::set(&sub);
      sandbox
        .remember(Kind::Run, &Detail::Name("./tool".to_string()), Answer::Once)
        .unwrap();
      assert_eq!(
        sandbox
          .classify(Kind::Run, &Detail::Name("./tool".to_string()))
          .unwrap(),
        State::Granted
      );
      assert_eq!(
        sandbox
          .classify(Kind::Run, &Detail::Name(sub_tool.display().to_string()))
          .unwrap(),
        State::Granted
      );
      assert_eq!(
        sandbox
          .classify(Kind::Run, &Detail::Name("tool".to_string()))
          .unwrap(),
        State::Prompt
      );
    }
    {
      let _base = RunBase::set(&other);
      assert_eq!(
        sandbox
          .classify(Kind::Run, &Detail::Name("./tool".to_string()))
          .unwrap(),
        State::Prompt
      );
      assert_eq!(
        sandbox
          .classify(Kind::Run, &Detail::Name(sub_tool.display().to_string()))
          .unwrap(),
        State::Granted
      );
      sandbox
        .remember(Kind::Run, &Detail::Name("./tool".to_string()), Answer::Deny)
        .unwrap();
      assert_eq!(
        sandbox
          .classify(Kind::Run, &Detail::Name("./tool".to_string()))
          .unwrap(),
        State::Denied
      );
      assert_eq!(
        sandbox
          .classify(Kind::Run, &Detail::Name(other_tool.display().to_string()),)
          .unwrap(),
        State::Denied
      );
    }
    {
      let _base = RunBase::set(&sub);
      assert_eq!(
        sandbox
          .classify(Kind::Run, &Detail::Name("./tool".to_string()))
          .unwrap(),
        State::Granted
      );
      sandbox
        .revoke(Kind::Run, &Detail::Name("./tool".to_string()))
        .unwrap();
      assert_eq!(
        sandbox
          .classify(Kind::Run, &Detail::Name("./tool".to_string()))
          .unwrap(),
        State::Denied
      );
    }
    {
      let _base = RunBase::set(&other);
      assert_eq!(
        sandbox
          .classify(Kind::Run, &Detail::Name("./tool".to_string()))
          .unwrap(),
        State::Denied
      );
    }
    sandbox
      .remember(Kind::Env, &Detail::Name("./tool".to_string()), Answer::Once)
      .unwrap();
    assert_eq!(
      sandbox
        .classify(Kind::Env, &Detail::Name("./tool".to_string()))
        .unwrap(),
      State::Granted
    );
    assert_eq!(
      sandbox
        .classify(Kind::Env, &Detail::Name(sub_tool.display().to_string()))
        .unwrap(),
      State::Prompt
    );
    #[cfg(unix)]
    {
      let outside = root.join("outside");
      std::fs::create_dir_all(outside.join("bin")).unwrap();
      let outside_tool = outside.join("tool");
      std::fs::write(&outside_tool, b"out\n").unwrap();
      let outside_tool = std::fs::canonicalize(&outside_tool).unwrap();
      std::os::unix::fs::symlink(outside.join("bin"), sub.join("link"))
        .unwrap();
      let mut escape =
        Sandbox::compile(&PermissionSpec::default(), &sub).unwrap();
      let _base = RunBase::set(&sub);
      let command = "link/../tool".to_string();
      escape
        .remember(Kind::Run, &Detail::Name(command.clone()), Answer::Once)
        .unwrap();
      assert_eq!(
        escape.classify(Kind::Run, &Detail::Name(command)).unwrap(),
        State::Prompt
      );
      assert_eq!(
        escape
          .classify(
            Kind::Run,
            &Detail::Name(outside_tool.display().to_string()),
          )
          .unwrap(),
        State::Prompt
      );
      assert_eq!(
        escape
          .classify(Kind::Run, &Detail::Name(sub_tool.display().to_string()))
          .unwrap(),
        State::Granted
      );
    }
    let _ = std::fs::remove_dir_all(&root);
  }

  #[test]
  fn revoke_masks_an_allow_until_it_is_granted_again() {
    let spec = PermissionSpec::allow_all();
    let mut sandbox = Sandbox::compile(&spec, Path::new("/")).unwrap();
    sandbox.revoke(Kind::Net, &Detail::Any).unwrap();
    assert_eq!(
      sandbox.classify(Kind::Net, &Detail::Any).unwrap(),
      State::Prompt
    );
    sandbox
      .remember(Kind::Net, &Detail::Any, Answer::All)
      .unwrap();
    assert_eq!(
      sandbox.classify(Kind::Net, &Detail::Any).unwrap(),
      State::Granted
    );
    let _ = spec;
  }

  #[test]
  fn a_directory_ffi_grant_does_not_grant_the_whole_kind() {
    let root = std::env::temp_dir();
    let root = std::fs::canonicalize(&root).unwrap_or(root);
    let project = root.join(format!("yuzuko-perm-ffi-{}", std::process::id()));
    let mut spec = PermissionSpec::default();
    spec
      .allow("ffi", Some(vec![project.display().to_string()]))
      .unwrap();
    let sandbox = Sandbox::compile(&spec, &root).unwrap();
    assert_eq!(
      sandbox.classify(Kind::Ffi, &Detail::Any).unwrap(),
      State::Prompt
    );
    assert_eq!(
      sandbox
        .classify(Kind::Ffi, &Detail::Path(project.join("libfoo.dylib")))
        .unwrap(),
      State::Granted
    );
    assert_eq!(
      sandbox
        .classify(
          Kind::Ffi,
          &Detail::Path(PathBuf::from("/usr/lib/libSystem.B.dylib")),
        )
        .unwrap(),
      State::Prompt
    );
    let all =
      Sandbox::compile(&PermissionSpec::allow_all(), Path::new("/")).unwrap();
    assert_eq!(
      all.classify(Kind::Ffi, &Detail::Any).unwrap(),
      State::Granted
    );
  }

  #[test]
  fn a_dotdot_after_a_symlink_is_not_granted() {
    let entry = std::env::temp_dir().join(format!(
      "yuzuko-perm-dot-{}-{}",
      std::process::id(),
      std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
    ));
    let outside = std::env::temp_dir().join(format!(
      "yuzuko-perm-dot-out-{}-{}",
      std::process::id(),
      std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
    ));
    let _ = std::fs::remove_dir_all(&entry);
    let _ = std::fs::remove_dir_all(&outside);
    std::fs::create_dir_all(entry.join("sub")).unwrap();
    std::fs::create_dir_all(outside.join("a").join("b")).unwrap();
    std::fs::write(outside.join("a").join("ident.env"), "secret").unwrap();
    std::fs::write(entry.join("note.txt"), "plain").unwrap();
    let entry = std::fs::canonicalize(&entry).unwrap_or(entry);
    let outside = std::fs::canonicalize(&outside).unwrap_or(outside);
    #[cfg(unix)]
    std::os::unix::fs::symlink(
      outside.join("a").join("b"),
      entry.join("sub").join("link"),
    )
    .unwrap();
    let mut spec = PermissionSpec::default();
    spec
      .allow("read", Some(vec![entry.display().to_string()]))
      .unwrap();
    spec
      .allow("write", Some(vec![entry.display().to_string()]))
      .unwrap();
    spec
      .allow("ffi", Some(vec![entry.display().to_string()]))
      .unwrap();
    let sandbox = Sandbox::compile(&spec, &entry).unwrap();
    let benign = entry.join("sub").join("..").join("note.txt");
    assert_eq!(
      sandbox.classify(Kind::Read, &Detail::Path(benign)).unwrap(),
      State::Granted
    );
    let benign_write = entry.join("sub").join("..").join("new-note.txt");
    assert_eq!(
      sandbox
        .classify(Kind::Write, &Detail::Path(benign_write))
        .unwrap(),
      State::Granted
    );
    #[cfg(unix)]
    {
      let mut sandbox = sandbox;
      let escape = entry.join("sub").join("link").join("..").join("ident.env");
      assert_eq!(
        sandbox
          .classify(Kind::Read, &Detail::Path(escape.clone()))
          .unwrap(),
        State::Prompt
      );
      assert_eq!(
        sandbox
          .classify(Kind::Ffi, &Detail::Path(escape.clone()))
          .unwrap(),
        State::Prompt
      );
      let missing = entry
        .join("sub")
        .join("link")
        .join("..")
        .join("new")
        .join("file.txt");
      assert_eq!(
        sandbox
          .classify(Kind::Write, &Detail::Path(missing))
          .unwrap(),
        State::Prompt
      );
      sandbox
        .remember(Kind::Read, &Detail::Path(escape.clone()), Answer::Once)
        .unwrap();
      assert_eq!(
        sandbox.classify(Kind::Read, &Detail::Path(escape)).unwrap(),
        State::Prompt
      );
      let direct = entry.join("sub").join("link");
      assert_eq!(
        sandbox.classify(Kind::Read, &Detail::Path(direct)).unwrap(),
        State::Prompt
      );
      std::os::unix::fs::symlink(
        outside.join("a").join("ident.env"),
        entry.join("sub").join("filelink"),
      )
      .unwrap();
      let through_file = entry
        .join("sub")
        .join("filelink")
        .join("..")
        .join("ident.env");
      assert_eq!(
        sandbox
          .classify(Kind::Read, &Detail::Path(through_file))
          .unwrap(),
        State::Prompt
      );
      let inside_tool = entry.join("sub").join("inside-tool.sh");
      let outside_tool = outside.join("a").join("inside-tool.sh");
      std::fs::write(&inside_tool, "inside").unwrap();
      std::fs::write(&outside_tool, "outside").unwrap();
      let mut inside_run = PermissionSpec::default();
      inside_run
        .allow("run", Some(vec![inside_tool.display().to_string()]))
        .unwrap();
      let inside_run = Sandbox::compile(&inside_run, &entry).unwrap();
      assert_eq!(
        inside_run
          .classify(
            Kind::Run,
            &Detail::Name(inside_tool.display().to_string()),
          )
          .unwrap(),
        State::Granted
      );
      let escape_run = entry
        .join("sub")
        .join("link")
        .join("..")
        .join("inside-tool.sh");
      assert_eq!(
        inside_run
          .classify(Kind::Run, &Detail::Name(escape_run.display().to_string()))
          .unwrap(),
        State::Prompt
      );
      let mut outside_run = PermissionSpec::default();
      outside_run
        .allow("run", Some(vec![outside_tool.display().to_string()]))
        .unwrap();
      let outside_run = Sandbox::compile(&outside_run, &entry).unwrap();
      assert_eq!(
        outside_run
          .classify(Kind::Run, &Detail::Name(escape_run.display().to_string()))
          .unwrap(),
        State::Granted
      );
      std::os::unix::fs::symlink(
        &inside_tool,
        entry.join("sub").join("tool-link"),
      )
      .unwrap();
      let mut link_run = PermissionSpec::default();
      let tool_link = entry.join("sub").join("tool-link");
      link_run
        .allow("run", Some(vec![tool_link.display().to_string()]))
        .unwrap();
      let link_run = Sandbox::compile(&link_run, &entry).unwrap();
      assert_eq!(
        link_run
          .classify(Kind::Run, &Detail::Name(tool_link.display().to_string()))
          .unwrap(),
        State::Prompt
      );
      assert_eq!(
        inside_run
          .classify(Kind::Run, &Detail::Name(tool_link.display().to_string()))
          .unwrap(),
        State::Granted
      );
      let evil_link = entry.join("sub").join("evil-link");
      std::os::unix::fs::symlink(&outside_tool, &evil_link).unwrap();
      let mut evil_run = PermissionSpec::default();
      evil_run
        .allow("run", Some(vec![evil_link.display().to_string()]))
        .unwrap();
      let evil_run = Sandbox::compile(&evil_run, &entry).unwrap();
      assert_eq!(
        evil_run
          .classify(Kind::Run, &Detail::Name(evil_link.display().to_string()))
          .unwrap(),
        State::Prompt
      );
      let _base = RunBase::set(&entry.join("sub"));
      assert_eq!(
        inside_run
          .classify(
            Kind::Run,
            &Detail::Name("link/../inside-tool.sh".to_string()),
          )
          .unwrap(),
        State::Prompt
      );
      assert_eq!(
        inside_run
          .classify(Kind::Run, &Detail::Name("./inside-tool.sh".to_string()))
          .unwrap(),
        State::Granted
      );
      assert_eq!(
        outside_run
          .classify(
            Kind::Run,
            &Detail::Name("link/../inside-tool.sh".to_string()),
          )
          .unwrap(),
        State::Granted
      );
    }
    let _ = std::fs::remove_dir_all(&entry);
    let _ = std::fs::remove_dir_all(&outside);
  }

  #[test]
  fn an_invalid_host_is_rejected() {
    let mut spec = PermissionSpec::default();
    spec.allow("net", Some(vec!["[::1".to_string()])).unwrap();
    let error = Sandbox::compile(&spec, Path::new("/")).unwrap_err();
    assert!(error.contains("invalid"), "{error}");
  }

  #[test]
  fn an_ipv6_rule_matches_a_bracketed_host() {
    let mut spec = PermissionSpec::default();
    spec
      .deny("net", Some(vec!["[::1]:8080".to_string()]))
      .unwrap();
    spec
      .allow("net", Some(vec!["[2001:db8::1]".to_string()]))
      .unwrap();
    let sandbox = Sandbox::compile(&spec, Path::new("/")).unwrap();
    assert_eq!(
      sandbox
        .classify(
          Kind::Net,
          &Detail::Net {
            host: "[::1]".to_string(),
            port: Some(8080),
          },
        )
        .unwrap(),
      State::Denied
    );
    assert_eq!(
      sandbox
        .classify(
          Kind::Net,
          &Detail::Net {
            host: "::1".to_string(),
            port: Some(80),
          },
        )
        .unwrap(),
      State::Prompt
    );
    assert_eq!(
      sandbox
        .classify(
          Kind::Net,
          &Detail::Net {
            host: "[2001:DB8::1]".to_string(),
            port: Some(443),
          },
        )
        .unwrap(),
      State::Granted
    );
    assert_eq!(
      display(&Detail::Net {
        host: "[::1]".to_string(),
        port: Some(8080),
      })
      .unwrap(),
      "[::1]:8080"
    );
  }

  #[test]
  fn a_failed_walk_does_not_reveal_the_file() {
    let dir = std::env::temp_dir().join(format!(
      "yuzuko-perm-walk-{}-{}",
      std::process::id(),
      std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("secret");
    std::fs::write(&file, "hidden").unwrap();
    let sandbox = Sandbox::compile(&PermissionSpec::default(), &dir).unwrap();
    assert_eq!(
      sandbox
        .classify(Kind::Read, &Detail::Path(file.join("probe")))
        .unwrap(),
      State::Prompt
    );
    assert_eq!(
      sandbox
        .classify(Kind::Read, &Detail::Path(dir.join("absent").join("probe")),)
        .unwrap(),
      State::Prompt
    );
    let allowed = Sandbox::allow_all(false);
    assert_eq!(
      allowed
        .classify(Kind::Read, &Detail::Path(file.clone()))
        .unwrap(),
      State::Granted
    );
    assert_eq!(
      allowed
        .classify(Kind::Read, &Detail::Path(file.join("probe")))
        .unwrap(),
      State::Denied
    );
    let _ = std::fs::remove_dir_all(&dir);
  }

  #[test]
  fn the_prompt_text_escapes_controls() {
    let shown =
      display(&Detail::Name("note\n\u{1b}[1A\"x\\y".to_string())).unwrap();
    assert!(!shown.contains('\n'), "{shown}");
    assert!(!shown.contains('\u{1b}'), "{shown}");
    assert_eq!(shown, "note\\n\\u{1b}[1A\\\"x\\\\y");
  }

  #[test]
  fn the_prompt_text_escapes_bidi_controls() {
    let shown = display(&Detail::Name(
      "safe\u{202E}txt.exe\u{200E}\u{2066}".to_string(),
    ))
    .unwrap();
    assert!(!shown.contains('\u{202E}'), "{shown}");
    assert!(!shown.contains('\u{200E}'), "{shown}");
    assert!(!shown.contains('\u{2066}'), "{shown}");
    assert_eq!(shown, "safe\\u{202e}txt.exe\\u{200e}\\u{2066}");
    let marks =
      display(&Detail::Name("a\u{061C}b\u{2028}c\u{2029}d".to_string()))
        .unwrap();
    assert!(!marks.contains('\u{061C}'), "{marks}");
    assert!(!marks.contains('\u{2028}'), "{marks}");
    assert!(!marks.contains('\u{2029}'), "{marks}");
    assert_eq!(marks, "a\\u{61c}b\\u{2028}c\\u{2029}d");
    let zwj = display(&Detail::Name("a\u{200D}b".to_string())).unwrap();
    assert_eq!(zwj, "a\u{200D}b");
  }

  #[cfg(unix)]
  #[test]
  fn queued_terminal_input_is_discarded_before_the_answer() {
    let mut primary = 0;
    let mut secondary = 0;
    let opened = unsafe {
      libc::openpty(
        &mut primary,
        &mut secondary,
        std::ptr::null_mut(),
        std::ptr::null_mut(),
        std::ptr::null_mut(),
      )
    };
    assert_eq!(opened, 0, "openpty");
    let _close = PtyFds { primary, secondary };
    let queued = b"y\n";
    let wrote =
      unsafe { libc::write(primary, queued.as_ptr().cast(), queued.len()) };
    assert_eq!(wrote, queued.len() as isize);
    assert!(
      input_waiting(secondary, 1000),
      "queued answer did not arrive"
    );
    discard_fd(secondary);
    assert!(
      !input_waiting(secondary, 0),
      "queued answer survived the discard"
    );
    let next = b"n\n";
    let wrote =
      unsafe { libc::write(primary, next.as_ptr().cast(), next.len()) };
    assert_eq!(wrote, next.len() as isize);
    assert!(
      input_waiting(secondary, 1000),
      "input after the discard did not arrive"
    );
  }

  #[cfg(unix)]
  struct PtyFds {
    primary: i32,
    secondary: i32,
  }

  #[cfg(unix)]
  impl Drop for PtyFds {
    fn drop(&mut self) {
      unsafe {
        libc::close(self.primary);
        libc::close(self.secondary);
      }
    }
  }

  #[cfg(unix)]
  fn input_waiting(fd: i32, timeout_ms: i32) -> bool {
    let mut poll = libc::pollfd {
      fd,
      events: libc::POLLIN,
      revents: 0,
    };
    (unsafe { libc::poll(&mut poll, 1, timeout_ms) }) > 0
  }

  #[test]
  fn an_ip_literal_has_one_spelling() {
    assert_eq!(normalize_host("[2001:0DB8::1]"), "2001:db8::1");
    assert_eq!(normalize_host("2001:0db8::1"), "2001:db8::1");
    assert_eq!(normalize_host("::ffff:127.0.0.1"), "127.0.0.1");
    assert_eq!(normalize_host("[::ffff:7f00:1]"), "127.0.0.1");
    assert_eq!(normalize_host("::1"), "::1");
    assert_eq!(normalize_host("Fe80::1%ETH0"), "fe80::1%eth0");
    assert_eq!(normalize_host("evil.com."), "evil.com");
    assert_eq!(normalize_host("Evil.COM."), "evil.com");
    assert_eq!(normalize_host("localhost."), "localhost");
    assert_ne!(normalize_host("localhost"), "127.0.0.1");
    assert_eq!(normalize_host("例え.jp"), normalize_host("xn--r8jz45g.jp"));
    assert_eq!(normalize_host("例え.jp."), normalize_host("xn--r8jz45g.jp"));
    assert_eq!(
      display(&Detail::Net {
        host: "::ffff:127.0.0.1".to_string(),
        port: Some(9),
      })
      .unwrap(),
      "127.0.0.1:9"
    );
    assert_eq!(
      display(&Detail::Net {
        host: "2001:0db8::1".to_string(),
        port: None,
      })
      .unwrap(),
      "[2001:db8::1]"
    );
    let mut spec = PermissionSpec::default();
    spec
      .deny(
        "net",
        Some(vec![
          "[2001:0db8::1]".to_string(),
          "[::ffff:127.0.0.1]".to_string(),
        ]),
      )
      .unwrap();
    let sandbox = Sandbox::compile(&spec, Path::new("/")).unwrap();
    assert_eq!(
      sandbox
        .classify(
          Kind::Net,
          &Detail::Net {
            host: "2001:db8::1".to_string(),
            port: Some(9),
          },
        )
        .unwrap(),
      State::Denied
    );
    assert_eq!(
      sandbox
        .classify(
          Kind::Net,
          &Detail::Net {
            host: "127.0.0.1".to_string(),
            port: Some(80),
          },
        )
        .unwrap(),
      State::Denied
    );
    assert_eq!(
      sandbox
        .classify(
          Kind::Net,
          &Detail::Net {
            host: "example.com".to_string(),
            port: Some(80),
          },
        )
        .unwrap(),
      State::Prompt
    );
  }

  #[test]
  fn a_restricted_address_is_checked_again() {
    assert_eq!(
      restricted_host("127.0.0.1".parse().unwrap()).as_deref(),
      Some("127.0.0.1")
    );
    assert_eq!(
      restricted_host("::ffff:127.0.0.1".parse().unwrap()).as_deref(),
      Some("127.0.0.1")
    );
    assert_eq!(
      restricted_host("10.1.2.3".parse().unwrap()).as_deref(),
      Some("10.1.2.3")
    );
    assert_eq!(
      restricted_host("169.254.169.254".parse().unwrap()).as_deref(),
      Some("169.254.169.254")
    );
    assert_eq!(
      restricted_host("100.64.0.1".parse().unwrap()).as_deref(),
      Some("100.64.0.1")
    );
    assert_eq!(
      restricted_host("::1".parse().unwrap()).as_deref(),
      Some("::1")
    );
    assert!(restricted_host("fc00::1".parse().unwrap()).is_some());
    assert!(restricted_host("fe80::1".parse().unwrap()).is_some());
    assert!(restricted_host("8.8.8.8".parse().unwrap()).is_none());
    assert!(restricted_host("2001:db8::1".parse().unwrap()).is_none());
    assert!(restricted_host("fec0::1".parse().unwrap()).is_some());
    assert!(restricted_host("64:ff9b::8.8.8.8".parse().unwrap()).is_some());
    assert!(restricted_host("64:ff9b::1".parse().unwrap()).is_some());
    assert!(restricted_host("64:ff9b:1::".parse().unwrap()).is_none());
    assert!(restricted_host("::ffff:8.8.8.8".parse().unwrap()).is_none());
    assert!(restricted_host("100.127.255.255".parse().unwrap()).is_some());
    assert!(restricted_host("100.63.0.1".parse().unwrap()).is_none());
    assert!(restricted_host("100.128.0.1".parse().unwrap()).is_none());
    assert_eq!(
      restricted_host("224.0.0.1".parse().unwrap()).as_deref(),
      Some("224.0.0.1")
    );
    assert_eq!(
      restricted_host("255.255.255.255".parse().unwrap()).as_deref(),
      Some("255.255.255.255")
    );
    assert_eq!(
      restricted_host("0.0.0.0".parse().unwrap()).as_deref(),
      Some("0.0.0.0")
    );
    assert_eq!(
      restricted_host("::".parse().unwrap()).as_deref(),
      Some("::")
    );
  }

  #[cfg(feature = "fs")]
  #[test]
  fn a_relative_symlink_is_checked_from_the_new_parent() {
    let new = std::env::temp_dir().join("dirB").join("link");
    let relative =
      symlink_target(Path::new("target.txt"), &new, false).unwrap();
    assert_eq!(
      relative,
      std::env::temp_dir().join("dirB").join("target.txt")
    );
    let nested =
      symlink_target(Path::new("../secret.txt"), &new, false).unwrap();
    assert_eq!(
      nested,
      std::env::temp_dir()
        .join("dirB")
        .join("..")
        .join("secret.txt")
    );
    let old = std::env::temp_dir().join("dirA").join("target.txt");
    let absolute = symlink_target(&old, &new, false).unwrap();
    assert_eq!(absolute, old);
    let junction = symlink_target(Path::new("target.txt"), &new, true).unwrap();
    assert_eq!(junction, PathBuf::from("target.txt"));
  }

  #[cfg(windows)]
  #[test]
  fn a_windows_deny_covers_a_trailing_dot_and_a_stream() {
    let dir = std::env::temp_dir()
      .join(format!("yuzuko-perm-ads-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let secret = dir.join("secret.txt");
    let mut spec = PermissionSpec::default();
    spec
      .deny("write", Some(vec![secret.display().to_string()]))
      .unwrap();
    let sandbox = Sandbox::compile(&spec, &dir).unwrap();
    for name in [
      "secret.txt.",
      "secret.txt... ",
      "secret.txt ",
      "SECRET.TXT.",
      "secret.txt:hidden",
      "secret.txt:hidden:$DATA",
      "secret.txt::$DATA",
    ] {
      assert_eq!(
        sandbox
          .classify(Kind::Write, &Detail::Path(dir.join(name)))
          .unwrap(),
        State::Denied,
        "{name}"
      );
    }
    assert_eq!(
      sandbox
        .classify(Kind::Write, &Detail::Path(dir.join("other.txt")))
        .unwrap(),
      State::Prompt
    );
    assert_eq!(
      sandbox
        .classify(Kind::Write, &Detail::Path(dir.join("...")))
        .unwrap(),
      State::Prompt
    );
    assert_eq!(
      sandbox
        .classify(Kind::Write, &Detail::Path(dir.join(":stream")))
        .unwrap(),
      State::Prompt
    );
    let mut dotted = PermissionSpec::default();
    dotted
      .deny(
        "write",
        Some(vec![dir.join("secret.txt.").display().to_string()]),
      )
      .unwrap();
    let dotted = Sandbox::compile(&dotted, &dir).unwrap();
    assert_eq!(
      dotted.classify(Kind::Write, &Detail::Path(secret)).unwrap(),
      State::Denied
    );
    let mut allowed = PermissionSpec::default();
    let project = dir.join("project");
    std::fs::create_dir_all(&project).unwrap();
    allowed
      .allow("read", Some(vec![project.display().to_string()]))
      .unwrap();
    let allowed = Sandbox::compile(&allowed, &dir).unwrap();
    for parent in [".. ", "... "] {
      let escaped = project.join(parent).join("sibling");
      assert_ne!(
        allowed
          .classify(Kind::Read, &Detail::Path(escaped))
          .unwrap(),
        State::Granted,
        "{parent:?}"
      );
    }
    let _ = std::fs::remove_dir_all(&dir);
  }
}
