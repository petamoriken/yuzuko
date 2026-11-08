//! Permission flags and the `permissions` object in `yuzuko.json`.

use std::path::{Path, PathBuf};

use serde::Deserialize;
use yuzuko_core::PermissionSpec;

#[derive(Clone, Debug)]
pub enum PermissionFlag {
  AllowAll,
  NoPrompt,
  Allow {
    name: &'static str,
    values: Option<Vec<String>>,
  },
  Deny {
    name: &'static str,
    values: Option<Vec<String>>,
  },
}

/// `None` when `token` is not a permission flag. An empty `=` value is an error.
pub fn parse_permission_flag(
  token: &str,
) -> Option<Result<PermissionFlag, String>> {
  if token == "-A" || token == "--allow-all" {
    return Some(Ok(PermissionFlag::AllowAll));
  }
  if token.starts_with("-A=") || token.starts_with("--allow-all=") {
    return Some(Err("--allow-all does not take a value.".to_string()));
  }
  if token == "--no-prompt" {
    return Some(Ok(PermissionFlag::NoPrompt));
  }
  if token.starts_with("--no-prompt=") {
    return Some(Err("--no-prompt does not take a value.".to_string()));
  }
  const ALLOW: &[(&str, &str)] = &[
    ("--allow-read", "read"),
    ("-R", "read"),
    ("--allow-write", "write"),
    ("-W", "write"),
    ("--allow-net", "net"),
    ("-N", "net"),
    ("--allow-env", "env"),
    ("-E", "env"),
    ("--allow-run", "run"),
    ("--allow-ffi", "ffi"),
  ];
  const DENY: &[(&str, &str)] = &[
    ("--deny-read", "read"),
    ("--deny-write", "write"),
    ("--deny-net", "net"),
    ("--deny-env", "env"),
    ("--deny-run", "run"),
    ("--deny-ffi", "ffi"),
  ];
  for (flag, name) in ALLOW {
    if let Some(values) = split_flag(token, flag) {
      return Some(values.map(|values| PermissionFlag::Allow { name, values }));
    }
  }
  for (flag, name) in DENY {
    if let Some(values) = split_flag(token, flag) {
      return Some(values.map(|values| PermissionFlag::Deny { name, values }));
    }
  }
  None
}

/// Apply allow flags, then `--allow-all`, then deny flags.
///
/// Relative filesystem paths are stored against `cwd`.
pub fn apply_permission_flags(
  spec: &mut PermissionSpec,
  flags: &[PermissionFlag],
  cwd: &Path,
) -> Result<(), String> {
  let mut allow_all = false;
  let mut allows = Vec::new();
  let mut denies = Vec::new();
  for flag in flags {
    match flag {
      PermissionFlag::AllowAll => allow_all = true,
      PermissionFlag::NoPrompt => {}
      PermissionFlag::Allow { name, values } => {
        allows.push((*name, values.clone()));
      }
      PermissionFlag::Deny { name, values } => {
        denies.push((*name, values.clone()));
      }
    }
  }
  for (name, values) in allows {
    spec.allow(name, absolutize(name, values, cwd)?)?;
  }
  if allow_all {
    for name in ["read", "write", "net", "env", "run", "ffi"] {
      spec.allow(name, None)?;
    }
  }
  for (name, values) in denies {
    spec.deny(name, absolutize(name, values, cwd)?)?;
  }
  Ok(())
}

/// `deny_omitted` denies every permission the object does not name.
///
/// That is the packaged-app rule. `yuzuko run` leaves an omitted name promptable.
pub fn apply_permission_config(
  spec: &mut PermissionSpec,
  value: &serde_json::Value,
  deny_omitted: bool,
) -> Result<(), String> {
  if !value.is_object() {
    return Err("permissions must be an object.".to_string());
  }
  let raw: RawPermissions = serde_json::from_value(value.clone())
    .map_err(|error| format!("permissions is invalid: {error}"))?;
  for (name, grant) in [
    ("read", raw.read),
    ("write", raw.write),
    ("net", raw.net),
    ("env", raw.env),
    ("run", raw.run),
    ("ffi", raw.ffi),
  ] {
    match grant {
      None if deny_omitted => spec.deny(name, None)?,
      None => {}
      Some(PermGrant::Flag(true)) => spec.allow(name, None)?,
      Some(PermGrant::Flag(false)) => spec.deny(name, None)?,
      Some(PermGrant::List(items)) => spec.allow(name, Some(items))?,
    }
  }
  Ok(())
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawPermissions {
  #[serde(default)]
  read: Option<PermGrant>,
  #[serde(default)]
  write: Option<PermGrant>,
  #[serde(default)]
  net: Option<PermGrant>,
  #[serde(default)]
  env: Option<PermGrant>,
  #[serde(default)]
  run: Option<PermGrant>,
  #[serde(default)]
  ffi: Option<PermGrant>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum PermGrant {
  Flag(bool),
  List(Vec<String>),
}

fn split_flag(
  token: &str,
  flag: &str,
) -> Option<Result<Option<Vec<String>>, String>> {
  if token == flag {
    return Some(Ok(None));
  }
  let prefix = format!("{flag}=");
  let Some(rest) = token.strip_prefix(&prefix) else {
    return None;
  };
  if rest.is_empty() {
    return Some(Err(format!("{flag} requires a value.")));
  }
  Some(split_list(rest).map(Some))
}

fn split_list(text: &str) -> Result<Vec<String>, String> {
  let mut items = Vec::new();
  let mut current = String::new();
  let mut chars = text.chars().peekable();
  while let Some(char) = chars.next() {
    if char == ',' {
      if chars.peek() == Some(&',') {
        chars.next();
        current.push(',');
      } else {
        if current.is_empty() {
          return Err("A permission list contains an empty item.".to_string());
        }
        items.push(std::mem::take(&mut current));
      }
    } else {
      current.push(char);
    }
  }
  if current.is_empty() {
    return Err("A permission list contains an empty item.".to_string());
  }
  items.push(current);
  Ok(items)
}

fn absolutize(
  name: &str,
  values: Option<Vec<String>>,
  cwd: &Path,
) -> Result<Option<Vec<String>>, String> {
  let Some(values) = values else {
    return Ok(None);
  };
  let mut absolute = Vec::with_capacity(values.len());
  for value in values {
    if is_path_rule(name, &value) {
      let path = Path::new(&value);
      if path.is_absolute() {
        absolute.push(value);
      } else {
        absolute.push(cwd.join(path).display().to_string());
      }
    } else {
      absolute.push(value);
    }
  }
  Ok(Some(absolute))
}

fn is_path_rule(name: &str, value: &str) -> bool {
  matches!(name, "read" | "write" | "ffi")
    || (name == "run" && (value.contains('/') || value.contains('\\')))
}

pub fn current_dir() -> Result<PathBuf, String> {
  std::env::current_dir()
    .map_err(|_| "The current directory could not be read.".to_string())
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn a_doubled_comma_is_one_item() {
    let flag = parse_permission_flag("--allow-read=a,,b,c")
      .unwrap()
      .unwrap();
    match flag {
      PermissionFlag::Allow { values, .. } => {
        assert_eq!(values.unwrap(), vec!["a,b".to_string(), "c".to_string()]);
      }
      other => panic!("{other:?}"),
    }
  }

  #[test]
  fn an_empty_value_is_rejected() {
    let error = parse_permission_flag("--allow-net=").unwrap().unwrap_err();
    assert!(error.contains("requires a value"), "{error}");
  }

  #[test]
  fn an_unknown_allow_is_not_a_permission_flag() {
    assert!(parse_permission_flag("--allow-foo").is_none());
  }

  #[test]
  fn omitted_names_deny_only_for_a_package() {
    let value = serde_json::json!({ "read": true });
    let mut run = PermissionSpec::default();
    apply_permission_config(&mut run, &value, false).unwrap();
    assert!(run.read.allow_all);
    assert!(!run.write.deny_all);
    let mut packaged = PermissionSpec::default();
    apply_permission_config(&mut packaged, &value, true).unwrap();
    assert!(packaged.read.allow_all);
    assert!(packaged.write.deny_all);
    assert!(packaged.net.deny_all);
  }
}
