//! Application bundles for macOS, Windows, and Linux.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Serialize;

use crate::bundle::config::{Config, ConfigError, Icon};

#[derive(Clone, Copy)]
pub enum Target {
  Macos,
  Windows,
  Linux,
}

#[derive(Serialize)]
struct BundleFile {
  name: String,
  version: String,
  identifier: String,
  entry: String,
  #[serde(rename = "releaseBaseUrl", skip_serializing_if = "Option::is_none")]
  release_base_url: Option<String>,
}

pub struct BundleLaunch {
  pub entry: PathBuf,
  pub sandbox: PathBuf,
  pub name: String,
  pub identifier: Option<String>,
  pub resources: PathBuf,
}

/// When this executable is inside a package, return its entry.
///
/// A previous launch's staged update is applied first, so this launch runs
/// the new `app/` tree. The executable itself is not replaced.
pub fn bundled_launch() -> Option<BundleLaunch> {
  let resources = locate_resources()?;
  crate::bundle::update::apply_pending(&resources);
  let manifest = crate::bundle::update::read_bundle(&resources).ok()?;
  let sandbox = resources.join("app");
  let entry =
    crate::bundle::update::push_relative(&sandbox, &manifest.entry).ok()?;
  Some(BundleLaunch {
    entry,
    sandbox,
    name: manifest.name,
    identifier: manifest.identifier,
    resources,
  })
}

/// macOS `Contents/Resources`, or the directory that holds the executable
/// and `yuzuko.bundle.json` on Windows and Linux.
fn locate_resources() -> Option<PathBuf> {
  let exe = std::env::current_exe().ok()?;
  let parent = exe.parent()?;
  if parent.file_name().and_then(|name| name.to_str()) == Some("MacOS") {
    let contents = parent.parent()?;
    if contents.file_name().and_then(|name| name.to_str()) == Some("Contents") {
      let resources = contents.join("Resources");
      if resources.join("yuzuko.bundle.json").is_file() {
        return Some(resources);
      }
    }
  }
  if parent.join("yuzuko.bundle.json").is_file() {
    return Some(parent.to_path_buf());
  }
  None
}

pub fn package(
  config: &Config,
  target: Target,
  runtime: &Path,
  output: &Path,
) -> Result<(), ConfigError> {
  if !runtime.is_file() {
    return Err(ConfigError::new(format!(
      "runtime was not found: {}",
      runtime.display()
    )));
  }
  // Reject a graph, asset, or symlink that leaves the config directory
  // before creating the output parent or copying the runtime.
  validate_bundle(config)?;
  let parent = output.parent().filter(|path| !path.as_os_str().is_empty());
  if let Some(parent) = parent {
    fs::create_dir_all(parent).map_err(|error| {
      ConfigError::new(format!("{}: {error}", parent.display()))
    })?;
  }
  let temp = temp_output(output);
  if temp.exists() {
    remove_path(&temp)?;
  }
  let wrote = match target {
    Target::Macos => write_macos(config, runtime, &temp),
    Target::Windows => write_directory(config, target, runtime, &temp),
    Target::Linux => write_directory(config, target, runtime, &temp),
  };
  if let Err(error) = wrote {
    let _ = remove_path(&temp);
    return Err(error);
  }
  if output.exists() {
    remove_path(output)?;
  }
  fs::rename(&temp, output).map_err(|error| {
    let _ = remove_path(&temp);
    ConfigError::new(format!("cannot replace {}: {error}", output.display()))
  })
}

fn temp_output(output: &Path) -> PathBuf {
  let name = output
    .file_name()
    .and_then(|name| name.to_str())
    .unwrap_or("bundle");
  let parent = output.parent().unwrap_or_else(|| Path::new("."));
  parent.join(format!(".{name}.partial"))
}

fn write_macos(
  config: &Config,
  runtime: &Path,
  output: &Path,
) -> Result<(), ConfigError> {
  let exe_name = crate::bundle::config::slug(&config.name)?;
  let macos = output.join("Contents/MacOS");
  let resources = output.join("Contents/Resources");
  fs::create_dir_all(&macos)?;
  fs::create_dir_all(&resources)?;
  let executable = macos.join(&exe_name);
  copy_runtime(runtime, &executable)?;
  let app_root = resources.join("app");
  let entry = copy_tree(config, &app_root)?;
  write_manifest(config, &entry, &resources.join("yuzuko.bundle.json"))?;
  write_trust_anchor(config, &resources)?;
  if let Some(icon) = &config.icons.macos {
    let icns = resources.join("AppIcon.icns");
    match icon {
      Icon::File(path) => fs::copy(path, &icns).map(|_| ())?,
      Icon::Pngs(pngs) => fs::write(&icns, icns_from_pngs(pngs)?)?,
    }
  }
  fs::write(
    output.join("Contents/Info.plist"),
    info_plist(config, &exe_name, config.icons.macos.is_some()),
  )?;
  let identity = config
    .codesign_identity
    .clone()
    .filter(|identity| identity != "-")
    .unwrap_or_else(|| "-".to_string());
  sign(output, &identity, &config.identifier)
}

fn write_directory(
  config: &Config,
  target: Target,
  runtime: &Path,
  output: &Path,
) -> Result<(), ConfigError> {
  fs::create_dir_all(output)?;
  let exe_name = crate::bundle::config::slug(&config.name)?;
  let executable = match target {
    Target::Windows => output.join(format!("{exe_name}.exe")),
    _ => output.join(&exe_name),
  };
  copy_runtime(runtime, &executable)?;
  if matches!(target, Target::Windows) {
    let mut image = fs::read(&executable)?;
    if gui_subsystem(&mut image) {
      fs::write(&executable, image)?;
      if let Some(Icon::File(path)) = &config.icons.windows {
        embed_windows_icon(&executable, path)?;
      }
    }
  }
  let app_root = output.join("app");
  let entry = copy_tree(config, &app_root)?;
  write_manifest(config, &entry, &output.join("yuzuko.bundle.json"))?;
  write_trust_anchor(config, output)?;
  match target {
    Target::Windows => {
      fs::write(output.join("AppUserModelID"), &config.identifier)?;
      if let Some(Icon::File(path)) = &config.icons.windows {
        fs::copy(path, output.join("AppIcon.ico")).map(|_| ())?;
      }
    }
    Target::Linux => {
      write_linux_icons(config, output)?;
      // The runtime sets the X11 `WM_CLASS` and the Wayland `app_id` to the
      // identifier, so `StartupWMClass` ties the window to this entry.
      let desktop = format!(
        "[Desktop Entry]\nType=Application\nName={}\nExec={exe_name}\nStartupWMClass={}\n",
        config.name.replace(['\n', '\r'], " "),
        config.identifier
      );
      fs::write(
        output.join(format!("{}.desktop", config.identifier)),
        desktop,
      )?;
    }
    Target::Macos => {}
  }
  Ok(())
}

/// Writes `icons.windows` into the executable so Explorer, a taskbar pin,
/// and a shortcut show it. The resources are written with the Windows
/// resource API, so only a Windows host embeds them, and a Windows bundle is
/// packaged on Windows.
fn embed_windows_icon(
  executable: &Path,
  ico: &Path,
) -> Result<(), ConfigError> {
  let bytes = fs::read(ico)?;
  crate::bundle::icon::parse_ico(&bytes)?;
  #[cfg(windows)]
  crate::bundle::icon::embed(executable, &bytes)?;
  #[cfg(not(windows))]
  let _ = executable;
  Ok(())
}

/// Marks a PE image as a Windows GUI program, as `editbin /SUBSYSTEM:WINDOWS`
/// does. A double-clicked bundle then opens no console window. Standard
/// handles a parent passes in, such as pipes, still work. Returns whether the
/// image was a console PE image and was changed.
pub(crate) fn gui_subsystem(image: &mut [u8]) -> bool {
  const CONSOLE: u16 = 3;
  const GUI: u16 = 2;
  let read_u32 = |at: usize| {
    image
      .get(at..at + 4)
      .map(|bytes| u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
  };
  if image.get(0..2) != Some(b"MZ") {
    return false;
  }
  let Some(header) = read_u32(0x3c).map(|offset| offset as usize) else {
    return false;
  };
  if image.get(header..header + 4) != Some(b"PE\0\0") {
    return false;
  }
  // The optional header follows the 4-byte signature and the 20-byte file
  // header. `Subsystem` is at offset 68 for both PE32 and PE32+.
  let at = header + 24 + 68;
  let Some(field) = image.get_mut(at..at + 2) else {
    return false;
  };
  if u16::from_le_bytes([field[0], field[1]]) != CONSOLE {
    return false;
  }
  field.copy_from_slice(&GUI.to_le_bytes());
  true
}

fn write_linux_icons(
  config: &Config,
  output: &Path,
) -> Result<(), ConfigError> {
  let Some(icon) = &config.icons.linux else {
    return Ok(());
  };
  let icons = output.join("icons");
  fs::create_dir_all(&icons)?;
  match icon {
    Icon::File(path) => {
      let name = path
        .file_name()
        .ok_or_else(|| ConfigError::new("linux icon has no file name."))?;
      fs::copy(path, icons.join(name)).map(|_| ())?;
    }
    Icon::Pngs(pngs) => {
      for (size, path) in pngs {
        fs::copy(path, icons.join(format!("{size}.png"))).map(|_| ())?;
      }
    }
  }
  Ok(())
}

fn validate_bundle(config: &Config) -> Result<(), ConfigError> {
  let entry = config
    .entry
    .clone()
    .ok_or_else(|| ConfigError::new("yuzuko package needs an entry path."))?;
  let mut files = yuzuko_core::module_graph(&entry)
    .map_err(|error| ConfigError::new(error.to_string()))?;
  files.extend(expand_assets(&config.directory, &config.assets)?);
  files.push(entry);
  for file in files {
    relative_path(&config.directory, &file)?;
  }
  Ok(())
}

fn copy_tree(config: &Config, app_root: &Path) -> Result<String, ConfigError> {
  fs::create_dir_all(app_root)?;
  let entry = config
    .entry
    .clone()
    .ok_or_else(|| ConfigError::new("yuzuko package needs an entry path."))?;
  let mut files = yuzuko_core::module_graph(&entry)
    .map_err(|error| ConfigError::new(error.to_string()))?;
  files.extend(expand_assets(&config.directory, &config.assets)?);
  files.push(entry.clone());
  files.sort();
  files.dedup();
  let mut relative_entry = None;
  for file in files {
    let relative = relative_path(&config.directory, &file)?;
    if file == entry {
      relative_entry = Some(relative.clone());
    }
    let dest = app_root.join(&relative);
    if let Some(parent) = dest.parent() {
      fs::create_dir_all(parent)?;
    }
    fs::copy(&file, &dest)?;
  }
  relative_entry.ok_or_else(|| ConfigError::new("the entry was not copied."))
}

fn expand_assets(
  root: &Path,
  patterns: &[String],
) -> Result<Vec<PathBuf>, ConfigError> {
  let mut files = Vec::new();
  for pattern in patterns {
    let parts: Vec<&str> = pattern
      .split('/')
      .filter(|part| !part.is_empty() && *part != ".")
      .collect();
    walk_glob(root, root, &parts, &mut files)?;
  }
  Ok(files)
}

fn walk_glob(
  root: &Path,
  dir: &Path,
  parts: &[&str],
  files: &mut Vec<PathBuf>,
) -> Result<(), ConfigError> {
  if parts.is_empty() {
    return Ok(());
  }
  let head = parts[0];
  let rest = &parts[1..];
  if head == "**" {
    if rest.is_empty() {
      collect_files(root, dir, files)?;
      return Ok(());
    }
    walk_glob(root, dir, rest, files)?;
    for entry in read_sorted(dir)? {
      if entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false) {
        walk_glob(root, &entry.path(), parts, files)?;
      }
    }
    return Ok(());
  }
  for entry in read_sorted(dir)? {
    let name = entry.file_name();
    let name = name.to_string_lossy();
    if !segment_matches(head, &name) {
      continue;
    }
    let path = contained(root, &entry.path())?;
    if rest.is_empty() {
      if path.is_file() {
        files.push(path);
      }
    } else if path.is_dir() {
      walk_glob(root, &path, rest, files)?;
    }
  }
  Ok(())
}

fn collect_files(
  root: &Path,
  dir: &Path,
  files: &mut Vec<PathBuf>,
) -> Result<(), ConfigError> {
  for entry in read_sorted(dir)? {
    let path = contained(root, &entry.path())?;
    if path.is_dir() {
      collect_files(root, &path, files)?;
    } else if path.is_file() {
      files.push(path);
    }
  }
  Ok(())
}

fn contained(root: &Path, path: &Path) -> Result<PathBuf, ConfigError> {
  let canonical =
    yuzuko_core::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
  let root =
    yuzuko_core::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
  if canonical.starts_with(&root) {
    Ok(canonical)
  } else {
    Err(ConfigError::new(format!(
      "asset path escapes the config directory: {}",
      path.display()
    )))
  }
}

fn read_sorted(dir: &Path) -> Result<Vec<fs::DirEntry>, ConfigError> {
  let mut entries = fs::read_dir(dir)
    .map_err(|error| ConfigError::new(format!("{}: {error}", dir.display())))?
    .collect::<Result<Vec<_>, _>>()
    .map_err(|error| ConfigError::new(format!("{}: {error}", dir.display())))?;
  entries.sort_by_key(|entry| entry.file_name());
  Ok(entries)
}

fn segment_matches(pattern: &str, name: &str) -> bool {
  if pattern == "*" {
    return true;
  }
  if !pattern.contains('*') {
    return pattern == name;
  }
  let parts: Vec<&str> = pattern.split('*').collect();
  if !name.starts_with(parts[0]) {
    return false;
  }
  let mut rest = &name[parts[0].len()..];
  for (index, part) in parts.iter().enumerate().skip(1) {
    if index + 1 == parts.len() {
      return rest.ends_with(part);
    }
    let Some(found) = rest.find(part) else {
      return false;
    };
    rest = &rest[found + part.len()..];
  }
  true
}

fn relative_path(root: &Path, path: &Path) -> Result<String, ConfigError> {
  let root =
    yuzuko_core::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
  let path =
    yuzuko_core::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
  let relative = path.strip_prefix(&root).map_err(|_| {
    ConfigError::new(format!(
      "module is outside the config directory: {}",
      path.display()
    ))
  })?;
  Ok(
    relative
      .components()
      .map(|component| component.as_os_str().to_string_lossy().into_owned())
      .collect::<Vec<_>>()
      .join("/"),
  )
}

fn write_trust_anchor(config: &Config, dir: &Path) -> Result<(), ConfigError> {
  let Some(key) = config.release_public_key else {
    return Ok(());
  };
  // Apply never rewrites this file. The feed cannot replace the anchor.
  fs::write(dir.join("yuzuko.update-key"), key)?;
  Ok(())
}

fn write_manifest(
  config: &Config,
  entry: &str,
  path: &Path,
) -> Result<(), ConfigError> {
  let file = BundleFile {
    name: config.name.clone(),
    version: config.version.clone(),
    identifier: config.identifier.clone(),
    entry: entry.to_string(),
    release_base_url: config.release_base_url.clone(),
  };
  let text = serde_json::to_string_pretty(&file)
    .map_err(|error| ConfigError::new(format!("bundle manifest: {error}")))?;
  fs::write(path, text)?;
  Ok(())
}

fn copy_runtime(from: &Path, to: &Path) -> Result<(), ConfigError> {
  fs::copy(from, to)?;
  #[cfg(unix)]
  {
    use std::os::unix::fs::PermissionsExt;
    let mut permissions = fs::metadata(to)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(to, permissions)?;
  }
  Ok(())
}

fn sign(
  app: &Path,
  identity: &str,
  identifier: &str,
) -> Result<(), ConfigError> {
  let output = Command::new("codesign")
    .args(["--force", "--sign", identity, "--identifier", identifier])
    .arg(app)
    .output()
    .map_err(|error| ConfigError::new(format!("codesign: {error}")))?;
  if output.status.success() {
    Ok(())
  } else {
    let stderr = String::from_utf8_lossy(&output.stderr);
    Err(ConfigError::new(format!(
      "codesign rejected the identity: {stderr}"
    )))
  }
}

fn info_plist(config: &Config, executable: &str, icon: bool) -> String {
  let mut plist = format!(
    "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \
\"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
<plist version=\"1.0\"><dict>\n\
<key>CFBundlePackageType</key><string>APPL</string>\n\
<key>CFBundleExecutable</key><string>{}</string>\n\
<key>CFBundleIdentifier</key><string>{}</string>\n\
<key>CFBundleName</key><string>{}</string>\n\
<key>CFBundleDisplayName</key><string>{}</string>\n\
<key>CFBundleShortVersionString</key><string>{}</string>\n\
<key>CFBundleVersion</key><string>{}</string>\n\
<key>CFBundleInfoDictionaryVersion</key><string>6.0</string>\n",
    xml(executable),
    xml(&config.identifier),
    xml(&config.name),
    xml(&config.name),
    xml(&config.version),
    xml(&config.version),
  );
  if icon {
    plist.push_str("<key>CFBundleIconFile</key><string>AppIcon</string>\n");
  }
  if let Some(url) = &config.release_base_url {
    plist.push_str(&format!(
      "<key>YuzukoReleaseBaseURL</key><string>{}</string>\n",
      xml(url)
    ));
  }
  plist.push_str("</dict></plist>\n");
  plist
}

fn xml(text: &str) -> String {
  text
    .replace('&', "&amp;")
    .replace('<', "&lt;")
    .replace('>', "&gt;")
    .replace('"', "&quot;")
}

fn icns_from_pngs(pngs: &[(u32, PathBuf)]) -> Result<Vec<u8>, ConfigError> {
  let mut body = Vec::new();
  for (size, path) in pngs {
    let kind = icns_type(*size).ok_or_else(|| {
      ConfigError::new(format!("icon size {size} is not supported."))
    })?;
    let png = fs::read(path)?;
    let length = u32::try_from(png.len() + 8)
      .map_err(|_| ConfigError::new("icon file is too large."))?;
    body.extend_from_slice(&kind);
    body.extend_from_slice(&length.to_be_bytes());
    body.extend_from_slice(&png);
  }
  let total = u32::try_from(body.len() + 8)
    .map_err(|_| ConfigError::new("icon file is too large."))?;
  let mut file = Vec::with_capacity(body.len() + 8);
  file.extend_from_slice(b"icns");
  file.extend_from_slice(&total.to_be_bytes());
  file.extend(body);
  Ok(file)
}

fn icns_type(size: u32) -> Option<[u8; 4]> {
  match size {
    16 => Some(*b"icp4"),
    32 => Some(*b"icp5"),
    64 => Some(*b"icp6"),
    128 => Some(*b"ic07"),
    256 => Some(*b"ic08"),
    512 => Some(*b"ic09"),
    1024 => Some(*b"ic10"),
    _ => None,
  }
}

fn remove_path(path: &Path) -> Result<(), ConfigError> {
  if !path.exists() {
    return Ok(());
  }
  if path.is_dir() {
    fs::remove_dir_all(path)
  } else {
    fs::remove_file(path)
  }
  .map_err(|error| ConfigError::new(format!("{}: {error}", path.display())))
}

impl From<std::io::Error> for ConfigError {
  fn from(error: std::io::Error) -> Self {
    Self {
      message: error.to_string(),
    }
  }
}

#[cfg(test)]
mod tests {
  use super::gui_subsystem;

  fn image(subsystem: u16) -> Vec<u8> {
    let mut image = vec![0u8; 0x200];
    image[0..2].copy_from_slice(b"MZ");
    image[0x3c..0x40].copy_from_slice(&0x80u32.to_le_bytes());
    image[0x80..0x84].copy_from_slice(b"PE\0\0");
    let at = 0x80 + 24 + 68;
    image[at..at + 2].copy_from_slice(&subsystem.to_le_bytes());
    image
  }

  #[test]
  fn console_image_becomes_gui() {
    let mut console = image(3);
    assert!(gui_subsystem(&mut console));
    assert_eq!(console, image(2));
  }

  #[test]
  fn other_bytes_stay_unchanged() {
    let mut gui = image(2);
    assert!(!gui_subsystem(&mut gui));
    assert_eq!(gui, image(2));
    let mut text = b"STANDIN-RUNTIME".to_vec();
    assert!(!gui_subsystem(&mut text));
    assert_eq!(text, b"STANDIN-RUNTIME");
    let mut truncated = image(3);
    truncated.truncate(0x90);
    assert!(!gui_subsystem(&mut truncated));
  }
}
