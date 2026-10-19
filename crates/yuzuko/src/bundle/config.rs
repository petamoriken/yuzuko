//! `yuzuko.json` and `yuzuko.jsonc`.

use std::fs;
use std::path::{Component, Path, PathBuf};

use serde::Deserialize;

#[derive(Debug)]
pub struct ConfigError {
  pub message: String,
}

impl ConfigError {
  pub fn new(message: impl Into<String>) -> Self {
    Self {
      message: message.into(),
    }
  }
}

impl std::fmt::Display for ConfigError {
  fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    self.message.fmt(formatter)
  }
}

#[derive(Debug, Clone)]
pub struct Config {
  pub directory: PathBuf,
  pub name: String,
  pub version: String,
  pub identifier: String,
  pub entry: Option<PathBuf>,
  pub assets: Vec<String>,
  pub icons: Icons,
  pub output: OutputPaths,
  pub codesign_identity: Option<String>,
  pub release_base_url: Option<String>,
  pub release_public_key: Option<[u8; 32]>,
}

#[derive(Debug, Clone, Default)]
pub struct Icons {
  pub macos: Option<Icon>,
  pub windows: Option<Icon>,
  pub linux: Option<Icon>,
}

#[derive(Debug, Clone)]
pub enum Icon {
  File(PathBuf),
  Pngs(Vec<(u32, PathBuf)>),
}

#[derive(Debug, Clone, Default)]
pub struct OutputPaths {
  pub macos: Option<PathBuf>,
  pub windows: Option<PathBuf>,
  pub linux: Option<PathBuf>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawConfig {
  name: Option<String>,
  version: Option<String>,
  identifier: Option<String>,
  icons: Option<RawIcons>,
  assets: Option<Vec<String>>,
  output: Option<RawOutput>,
  macos: Option<RawMacos>,
  release: Option<RawRelease>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawIcons {
  macos: Option<RawIcon>,
  windows: Option<RawIcon>,
  linux: Option<RawIcon>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum RawIcon {
  Path(String),
  Pngs(Vec<RawPng>),
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawPng {
  path: String,
  size: u32,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawOutput {
  macos: Option<String>,
  windows: Option<String>,
  linux: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawMacos {
  #[serde(rename = "codesignIdentity")]
  codesign_identity: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRelease {
  #[serde(rename = "baseUrl")]
  base_url: Option<String>,
  #[serde(rename = "publicKey")]
  public_key: Option<String>,
}

/// Nearest `yuzuko.json` or `yuzuko.jsonc`, walking from `start` toward root.
pub fn discover(start: &Path) -> Result<Option<PathBuf>, ConfigError> {
  let start = if start.is_dir() {
    start.to_path_buf()
  } else {
    start
      .parent()
      .ok_or_else(|| ConfigError::new("The entry has no directory."))?
      .to_path_buf()
  };
  let mut dir = absolute(&start)?;
  loop {
    let json = dir.join("yuzuko.json");
    let jsonc = dir.join("yuzuko.jsonc");
    match (json.is_file(), jsonc.is_file()) {
      (true, true) => {
        return Err(ConfigError::new(format!(
          "{} contains both yuzuko.json and yuzuko.jsonc",
          dir.display()
        )));
      }
      (true, false) => return Ok(Some(json)),
      (false, true) => return Ok(Some(jsonc)),
      (false, false) => match dir.parent() {
        Some(parent) if parent != dir => dir = parent.to_path_buf(),
        _ => return Ok(None),
      },
    }
  }
}

/// Config used by `yuzuko run` to fill an omitted window title.
///
/// A missing file yields `None`.
pub fn load_for_run(entry: &Path) -> Result<Option<Config>, ConfigError> {
  let Some(path) = discover(entry)? else {
    return Ok(None);
  };
  Ok(Some(read_config(&path)?))
}

/// Config for `yuzuko package`. `entry` is the module path from the command.
pub fn load_package(
  entry: &Path,
  config_path: Option<&Path>,
) -> Result<Config, ConfigError> {
  let path = match config_path {
    Some(path) => absolute(path)?,
    None => discover(entry)?
      .ok_or_else(|| ConfigError::new("yuzuko.json was not found."))?,
  };
  if !path.is_file() {
    return Err(ConfigError::new(format!(
      "config file was not found: {}",
      path.display()
    )));
  }
  let mut config = read_config(&path)?;
  let resolved = inside(&config.directory, &normalize(&absolute(entry)?))?;
  if !resolved.is_file() {
    return Err(ConfigError::new(format!(
      "entry file was not found: {}",
      resolved.display()
    )));
  }
  config.entry = Some(resolved);
  Ok(config)
}

fn read_config(path: &Path) -> Result<Config, ConfigError> {
  let text = fs::read_to_string(path).map_err(|error| {
    ConfigError::new(format!("{}: {error}", path.display()))
  })?;
  let jsonc = path
    .extension()
    .and_then(|ext| ext.to_str())
    .is_some_and(|ext| ext == "jsonc");
  let text = if jsonc { relax_jsonc(&text)? } else { text };
  let raw: RawConfig = serde_json::from_str(&text).map_err(|error| {
    ConfigError::new(format!("{}: {error}", path.display()))
  })?;
  let directory = path
    .parent()
    .ok_or_else(|| ConfigError::new("The config file has no directory."))?
    .to_path_buf();
  let name = match raw.name {
    Some(name) if !name.is_empty() => name,
    Some(_) => {
      return Err(ConfigError::new("name must not be empty."));
    }
    None => directory
      .file_name()
      .and_then(|name| name.to_str())
      .unwrap_or("app")
      .to_string(),
  };
  let version = match raw.version {
    Some(version) if !version.is_empty() => version,
    Some(_) => {
      return Err(ConfigError::new("version must not be empty."));
    }
    None => "0.0.0".to_string(),
  };
  if !crate::bundle::update::version_is_ordered(&version) {
    return Err(ConfigError::new(
      "version must be dot-separated non-negative integers.",
    ));
  }
  let identifier = match raw.identifier {
    Some(identifier) => {
      check_identifier(&identifier)?;
      identifier
    }
    None => {
      let slug = slug(&name)?;
      format!("com.yuzuko.{slug}")
    }
  };
  let mut assets = Vec::new();
  for pattern in raw.assets.unwrap_or_default() {
    if !glob_stays_inside(&pattern) {
      return Err(ConfigError::new(format!(
        "asset path escapes the config directory: {pattern}"
      )));
    }
    assets.push(pattern);
  }
  let icons = raw
    .icons
    .map(|icons| read_icons(&directory, icons))
    .transpose()?
    .unwrap_or_default();
  let output = raw
    .output
    .map(|output| read_output(&directory, output))
    .transpose()?
    .unwrap_or_default();
  let codesign_identity = raw.macos.and_then(|macos| macos.codesign_identity);
  let (release_base_url, release_public_key) = match raw.release {
    Some(release) => {
      let base_url = match release.base_url {
        Some(url) => {
          check_base_url(&url)?;
          Some(url)
        }
        None => None,
      };
      let public_key = match release.public_key {
        Some(text) => {
          Some(crate::bundle::update::decode_public_key(&text).map_err(|()| {
            ConfigError::new(
              "release.publicKey must be standard base64 of a 32-byte Ed25519 public key.",
            )
          })?)
        }
        None => None,
      };
      (base_url, public_key)
    }
    None => (None, None),
  };
  Ok(Config {
    directory,
    name,
    version,
    identifier,
    entry: None,
    assets,
    icons,
    output,
    codesign_identity,
    release_base_url,
    release_public_key,
  })
}

fn read_icons(root: &Path, icons: RawIcons) -> Result<Icons, ConfigError> {
  Ok(Icons {
    macos: icons
      .macos
      .map(|icon| read_icon(root, "macos", icon, "icns"))
      .transpose()?,
    windows: icons
      .windows
      .map(|icon| read_icon(root, "windows", icon, "ico"))
      .transpose()?,
    linux: icons
      .linux
      .map(|icon| read_icon(root, "linux", icon, "png"))
      .transpose()?,
  })
}

fn read_icon(
  root: &Path,
  platform: &str,
  icon: RawIcon,
  extension: &str,
) -> Result<Icon, ConfigError> {
  match icon {
    RawIcon::Pngs(_) if platform == "windows" => Err(ConfigError::new(
      "icons.windows does not accept a PNG array.",
    )),
    RawIcon::Pngs(pngs) => {
      if pngs.is_empty() {
        return Err(ConfigError::new(format!(
          "icons.{platform} needs at least one PNG."
        )));
      }
      let mut files = Vec::new();
      for png in pngs {
        if png.size == 0
          || !matches!(png.size, 16 | 32 | 64 | 128 | 256 | 512 | 1024)
        {
          return Err(ConfigError::new(format!(
            "icons.{platform} size {} is not supported.",
            png.size
          )));
        }
        let path = resolve_path(root, &png.path)?;
        if !extension_is(&path, "png") {
          return Err(ConfigError::new(format!(
            "icons.{platform} array entries must be PNG files."
          )));
        }
        if !path.is_file() {
          return Err(ConfigError::new(format!(
            "icon file was not found: {}",
            path.display()
          )));
        }
        files.push((png.size, path));
      }
      Ok(Icon::Pngs(files))
    }
    RawIcon::Path(path) => {
      let path = resolve_path(root, &path)?;
      if !extension_is(&path, extension) {
        return Err(ConfigError::new(format!(
          "icons.{platform} must be a .{extension} file."
        )));
      }
      if !path.is_file() {
        return Err(ConfigError::new(format!(
          "icon file was not found: {}",
          path.display()
        )));
      }
      Ok(Icon::File(path))
    }
  }
}

fn read_output(
  root: &Path,
  output: RawOutput,
) -> Result<OutputPaths, ConfigError> {
  Ok(OutputPaths {
    macos: output
      .macos
      .map(|path| output_path(root, &path, "macos"))
      .transpose()?,
    windows: output
      .windows
      .map(|path| output_path(root, &path, "windows"))
      .transpose()?,
    linux: output
      .linux
      .map(|path| output_path(root, &path, "linux"))
      .transpose()?,
  })
}

fn output_path(
  root: &Path,
  path: &str,
  platform: &str,
) -> Result<PathBuf, ConfigError> {
  reject_package_extension(path)?;
  let resolved = resolve_path(root, path)?;
  match platform {
    "macos" if !extension_is(&resolved, "app") => {
      Err(ConfigError::new("output.macos must end in .app."))
    }
    _ => Ok(resolved),
  }
}

pub fn reject_package_extension(path: &str) -> Result<(), ConfigError> {
  let lower = path.to_ascii_lowercase();
  for extension in [".dmg", ".msi", ".deb", ".rpm", ".appimage"] {
    if lower.ends_with(extension) {
      return Err(ConfigError::new(format!(
        "output {path} is not produced in this phase."
      )));
    }
  }
  Ok(())
}

fn check_base_url(url: &str) -> Result<(), ConfigError> {
  let rest = url
    .strip_prefix("https://")
    .or_else(|| url.strip_prefix("http://"));
  let Some(rest) = rest else {
    return Err(ConfigError::new(
      "release.baseUrl must be an absolute http or https URL.",
    ));
  };
  let host = rest.split(['/', '?', '#']).next().unwrap_or("");
  if host.is_empty() || host.contains(' ') {
    return Err(ConfigError::new(
      "release.baseUrl must be an absolute http or https URL.",
    ));
  }
  Ok(())
}

fn check_identifier(identifier: &str) -> Result<(), ConfigError> {
  let labels: Vec<&str> = identifier.split('.').collect();
  if labels.len() < 2 {
    return Err(ConfigError::new("identifier needs at least two labels."));
  }
  for label in labels {
    let mut chars = label.chars();
    let Some(first) = chars.next() else {
      return Err(ConfigError::new("identifier has an empty label."));
    };
    if !first.is_ascii_alphabetic() {
      return Err(ConfigError::new(format!(
        "identifier label \"{label}\" must start with a letter."
      )));
    }
    if !chars.all(|char| char.is_ascii_alphanumeric() || char == '-') {
      return Err(ConfigError::new(format!(
        "identifier label \"{label}\" has an invalid character."
      )));
    }
  }
  Ok(())
}

pub fn slug(name: &str) -> Result<String, ConfigError> {
  let mut out = String::new();
  let mut dash = false;
  for char in name.chars() {
    if char.is_ascii_alphabetic() {
      if dash && !out.is_empty() {
        out.push('-');
      }
      dash = false;
      out.push(char.to_ascii_lowercase());
    } else {
      dash = true;
    }
  }
  let out = out.trim_matches('-').to_string();
  if out.is_empty() {
    return Err(ConfigError::new(format!(
      "cannot build an identifier from \"{name}\"."
    )));
  }
  Ok(out)
}

fn resolve_path(root: &Path, relative: &str) -> Result<PathBuf, ConfigError> {
  if relative.is_empty() {
    return Err(ConfigError::new("a config path is empty."));
  }
  if relative.contains('\\') {
    return Err(ConfigError::new(format!(
      "config path must use / separators: {relative}"
    )));
  }
  if Path::new(relative).is_absolute() {
    return Err(ConfigError::new(format!(
      "config path escapes the config directory: {relative}"
    )));
  }
  if !glob_stays_inside(relative) {
    return Err(ConfigError::new(format!(
      "config path escapes the config directory: {relative}"
    )));
  }
  let joined = root.join(relative);
  inside(root, &normalize(&joined))
}

fn inside(root: &Path, path: &Path) -> Result<PathBuf, ConfigError> {
  let root = absolute(root)?;
  if path.starts_with(&root) {
    Ok(path.to_path_buf())
  } else {
    Err(ConfigError::new(format!(
      "config path escapes the config directory: {}",
      path.display()
    )))
  }
}

fn glob_stays_inside(pattern: &str) -> bool {
  let mut depth = 0i32;
  for part in pattern.split('/') {
    if part.is_empty() || part == "." {
      continue;
    }
    if part == ".." {
      depth -= 1;
      if depth < 0 {
        return false;
      }
    } else {
      depth += 1;
    }
  }
  true
}

fn extension_is(path: &Path, extension: &str) -> bool {
  path
    .extension()
    .and_then(|value| value.to_str())
    .is_some_and(|value| value.eq_ignore_ascii_case(extension))
}

fn absolute(path: &Path) -> Result<PathBuf, ConfigError> {
  let joined = if path.is_absolute() {
    path.to_path_buf()
  } else {
    let cwd = std::env::current_dir().map_err(|error| {
      ConfigError::new(format!("current directory: {error}"))
    })?;
    cwd.join(path)
  };
  // `--config ../yuzuko.json` must resolve before the directory is compared.
  Ok(normalize(&joined))
}

fn normalize(path: &Path) -> PathBuf {
  let mut out = PathBuf::new();
  for component in path.components() {
    match component {
      Component::ParentDir => {
        out.pop();
      }
      Component::CurDir => {}
      other => out.push(other),
    }
  }
  out
}

fn relax_jsonc(input: &str) -> Result<String, ConfigError> {
  let input = input.strip_prefix('\u{feff}').unwrap_or(input);
  let chars: Vec<char> = input.chars().collect();
  let mut stripped = String::new();
  let mut index = 0;
  let mut in_string = false;
  let mut escape = false;
  while index < chars.len() {
    let char = chars[index];
    if in_string {
      stripped.push(char);
      if escape {
        escape = false;
      } else if char == '\\' {
        escape = true;
      } else if char == '"' {
        in_string = false;
      }
      index += 1;
      continue;
    }
    if char == '"' {
      in_string = true;
      stripped.push(char);
      index += 1;
      continue;
    }
    if char == '/' && chars.get(index + 1) == Some(&'/') {
      index += 2;
      while index < chars.len() && chars[index] != '\n' {
        index += 1;
      }
      continue;
    }
    if char == '/' && chars.get(index + 1) == Some(&'*') {
      index += 2;
      let mut closed = false;
      while index + 1 < chars.len() {
        if chars[index] == '*' && chars[index + 1] == '/' {
          index += 2;
          closed = true;
          break;
        }
        index += 1;
      }
      if !closed {
        return Err(ConfigError::new("unclosed comment in yuzuko.jsonc."));
      }
      continue;
    }
    stripped.push(char);
    index += 1;
  }
  let chars: Vec<char> = stripped.chars().collect();
  let mut cleaned = String::new();
  index = 0;
  in_string = false;
  escape = false;
  while index < chars.len() {
    let char = chars[index];
    if in_string {
      cleaned.push(char);
      if escape {
        escape = false;
      } else if char == '\\' {
        escape = true;
      } else if char == '"' {
        in_string = false;
      }
      index += 1;
      continue;
    }
    if char == '"' {
      in_string = true;
      cleaned.push(char);
      index += 1;
      continue;
    }
    if char == ',' {
      let mut next = index + 1;
      while next < chars.len() && chars[next].is_whitespace() {
        next += 1;
      }
      if next < chars.len() && (chars[next] == '}' || chars[next] == ']') {
        index += 1;
        continue;
      }
    }
    cleaned.push(char);
    index += 1;
  }
  Ok(cleaned)
}

#[cfg(test)]
mod tests {
  use super::slug;

  #[test]
  fn game_name_slug_is_my_game() {
    assert_eq!(slug("My Game").unwrap(), "my-game");
    // Digits are separators, then hyphens are trimmed.
    assert_eq!(slug("Game2").unwrap(), "game");
    assert_eq!(slug("A1B").unwrap(), "a-b");
    assert!(slug("123").is_err());
    assert!(slug("!!!").is_err());
  }
}
