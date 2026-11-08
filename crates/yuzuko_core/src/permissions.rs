//! Process permissions for filesystem, network, environment, subprocess, and
//! native-library access.
//!
//! No installed policy allows every check. `yuzuko run` and `yuzuko repl`
//! install a sandbox. A packaged app installs one when its manifest has
//! `permissions`. `query` does not prompt. `request` and a gated operation
//! prompt when the policy says to and standard input is a terminal.

use std::cell::RefCell;
use std::io::{IsTerminal, Write};
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
  Path(PathBuf),
  Net { host: String, port: Option<u16> },
  Name(String),
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
}

impl Sandbox {
  fn empty(prompt: bool) -> Self {
    Self {
      gates: std::array::from_fn(|_| Gate::default()),
      prompt,
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
    Ok(sandbox)
  }

  fn classify(&self, kind: Kind, detail: &Detail) -> Result<State, OpError> {
    match detail {
      Detail::Any => Ok(self.unscoped(kind)),
      Detail::Path(path) => {
        let candidates = path_candidates(path)?;
        let states = candidates
          .iter()
          .map(|candidate| self.resource(kind, &Resource::Path(candidate)))
          .collect::<Vec<_>>();
        Ok(combine(&states))
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
          for candidate in session_candidates(path)? {
            gate
              .session_deny_paths
              .retain(|item| !paths_equal(item, &candidate));
            gate.session_allow_paths.push(candidate);
          }
          Ok(())
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
        Detail::Name(name) => {
          gate.session_deny_names.retain(|item| item != name);
          gate.session_allow_names.push(name.clone());
          Ok(())
        }
      },
      Answer::Deny => match detail {
        Detail::Any => {
          gate.session_deny_all = true;
          gate.session_allow_all = false;
          Ok(())
        }
        Detail::Path(path) => {
          for candidate in session_candidates(path)? {
            gate
              .session_allow_paths
              .retain(|item| !paths_equal(item, &candidate));
            gate.session_deny_paths.push(candidate);
          }
          Ok(())
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
        Detail::Name(name) => {
          gate.session_allow_names.retain(|item| item != name);
          gate.session_deny_names.push(name.clone());
          Ok(())
        }
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
        for candidate in session_candidates(path)? {
          gate
            .session_allow_paths
            .retain(|item| !paths_equal(item, &candidate));
          gate.session_deny_paths.push(candidate);
        }
        Ok(())
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
      Detail::Name(name) => {
        gate.session_allow_names.retain(|item| item != name);
        gate.session_deny_names.push(name.clone());
        Ok(())
      }
    }
  }

  fn grant_path(&mut self, kind: Kind, path: &Path) -> Result<(), OpError> {
    let candidates = session_candidates(path)?;
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

pub fn check_read(path: &Path) -> Result<(), OpError> {
  require(Kind::Read, &Detail::Path(path.to_path_buf()))
}

pub fn check_write(path: &Path) -> Result<(), OpError> {
  require(Kind::Write, &Detail::Path(path.to_path_buf()))
}

pub fn check_write_all() -> Result<(), OpError> {
  require(Kind::Write, &Detail::Any)
}

pub fn check_net(host: &str, port: u16) -> Result<(), OpError> {
  require(
    Kind::Net,
    &Detail::Net {
      host: host.to_string(),
      port: Some(port),
    },
  )
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

/// Like [`check_run`], resolving a relative command from `base`.
///
/// `spawn` uses this when `cwd` is set. The operating system resolves a
/// relative program from that directory, not from the process directory.
#[cfg(feature = "child")]
pub(crate) fn check_run_at(command: &str, base: &Path) -> Result<(), OpError> {
  let _base = RunBase::set(base);
  check_run(command)
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
  Ok(OpError::dom("NotAllowedError", message))
}

fn display(detail: &Detail) -> Result<String, OpError> {
  match detail {
    Detail::Any => Ok(String::new()),
    Detail::Path(path) => Ok(lexical_absolute(path)?.display().to_string()),
    Detail::Net { host, port } => Ok(format_host(host, *port)),
    Detail::Name(name) => Ok(name.clone()),
  }
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
      Component::ParentDir => {
        if matches!(out.components().next_back(), Some(Component::Normal(_))) {
          out.pop();
        }
      }
      other => out.push(other.as_os_str()),
    }
  }
  out
}

fn path_candidates(path: &Path) -> Result<Vec<PathBuf>, OpError> {
  let absolute = absolute_path(path)?;
  let lexical = normalize(&absolute);
  let mut found = vec![lexical.clone()];
  push_canonical(&mut found, &lexical);
  // The operating system follows a symlink and then applies `..`. Collapsing
  // `..` first would erase the link and allow a directory grant to escape.
  let resolved = operating_path(&absolute)?;
  let resolved = normalize(&strip_verbatim(resolved));
  if !found.iter().any(|item| paths_equal(item, &resolved)) {
    found.push(resolved.clone());
  }
  push_canonical(&mut found, &resolved);
  Ok(found)
}

/// The file a prompt yes or a dialog grant names. This is the lexical path
/// and its canonical spelling, not a different file reached by `..` after a
/// symlink.
fn session_candidates(path: &Path) -> Result<Vec<PathBuf>, OpError> {
  let lexical = lexical_absolute(path)?;
  let mut found = vec![lexical.clone()];
  push_canonical(&mut found, &lexical);
  Ok(found)
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

fn push_canonical(found: &mut Vec<PathBuf>, path: &Path) {
  let Some(real) = flexible_canonical(path) else {
    return;
  };
  let real = normalize(&strip_verbatim(real));
  if !found.iter().any(|item| paths_equal(item, &real)) {
    found.push(real);
  }
}

fn operating_path(path: &Path) -> Result<PathBuf, OpError> {
  if let Ok(real) = std::fs::canonicalize(path) {
    return Ok(real);
  }
  walk_operating(path, 0).map_err(|_| {
    OpError::dom("OperationError", "The path could not be resolved.")
  })
}

/// Follows symlinks and applies `..` to the resolved path. A missing
/// component cannot be a symlink, so the rest of the path is kept as named.
fn walk_operating(path: &Path, depth: u32) -> Result<PathBuf, ()> {
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
            let target = std::fs::read_link(&candidate).map_err(|_| ())?;
            let mut rest = if target.is_absolute() {
              target
            } else {
              out.join(target)
            };
            for later in components.iter().skip(index + 1) {
              rest.push(later);
            }
            return walk_operating(&rest, depth + 1);
          }
          Ok(_) => out.push(name),
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
      return PathBuf::from(rest.as_ref());
    }
  }
  path
}

fn paths_equal(left: &Path, right: &Path) -> bool {
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
    (Component::Normal(left), Component::Normal(right)) => os_eq(left, right),
    _ => false,
  }
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
  host.trim().to_ascii_lowercase()
}

fn format_host(host: &str, port: Option<u16>) -> String {
  let shown = if host.contains(':') {
    format!("[{host}]")
  } else {
    host.to_string()
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
  if rule.host != "*" && !host_eq(&rule.host, host) {
    return false;
  }
  rule.port.is_none_or(|value| value == port)
}

fn net_host_hit(rules: &[NetRule], host: &str) -> bool {
  rules.iter().any(|rule| {
    rule.port.is_none() && (rule.host == "*" || host_eq(&rule.host, host))
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

/// The file the operating system would execute for `path`.
fn executed_path(path: &Path) -> Option<PathBuf> {
  let absolute = absolute_path(path).ok()?;
  let resolved = operating_path(&absolute).ok()?;
  Some(normalize(&strip_verbatim(resolved)))
}

fn is_command_path(text: &str) -> bool {
  text.contains('/') || text.contains('\\')
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
  let desc = match descriptor(scope, args.get(0)) {
    Ok(desc) => desc,
    Err(error) => {
      error.throw(scope);
      return;
    }
  };
  let state = match interact(desc.kind, &desc.detail, false) {
    Ok(state) => state,
    Err(error) => {
      error.throw(scope);
      return;
    }
  };
  let object = state_object(scope, state);
  ret.set(resolved(scope, object.into()));
}

fn query_sync(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut ret: v8::ReturnValue,
) {
  let desc = match descriptor(scope, args.get(0)) {
    Ok(desc) => desc,
    Err(error) => {
      error.throw(scope);
      return;
    }
  };
  let state = match interact(desc.kind, &desc.detail, false) {
    Ok(state) => state,
    Err(error) => {
      error.throw(scope);
      return;
    }
  };
  ret.set(state_object(scope, state).into());
}

fn request(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut ret: v8::ReturnValue,
) {
  let desc = match descriptor(scope, args.get(0)) {
    Ok(desc) => desc,
    Err(error) => {
      error.throw(scope);
      return;
    }
  };
  let state = match interact(desc.kind, &desc.detail, true) {
    Ok(state) => state,
    Err(error) => {
      error.throw(scope);
      return;
    }
  };
  let object = state_object(scope, state);
  ret.set(resolved(scope, object.into()));
}

fn request_sync(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut ret: v8::ReturnValue,
) {
  let desc = match descriptor(scope, args.get(0)) {
    Ok(desc) => desc,
    Err(error) => {
      error.throw(scope);
      return;
    }
  };
  let state = match interact(desc.kind, &desc.detail, true) {
    Ok(state) => state,
    Err(error) => {
      error.throw(scope);
      return;
    }
  };
  ret.set(state_object(scope, state).into());
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
    let mut sandbox = Sandbox::compile(&spec, &entry).unwrap();
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
}
