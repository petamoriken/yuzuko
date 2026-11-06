//! Path normalization and the packaged-bundle write guard.
//!
//! The guard receives the root as an argument so tests can exercise it
//! without changing process state. Callers that serve JavaScript read the
//! root recorded for this process.

use std::io;
use std::path::{Component, Path, PathBuf};

use super::error::FsError;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Link {
  /// Resolve the final component, including a symlink.
  Follow,
  /// Keep the final directory entry, so a symlink is not followed.
  Entry,
}

pub(super) fn lexical(path: &Path) -> PathBuf {
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

pub(super) fn lexical_absolute(path: &Path) -> PathBuf {
  let absolute = if path.is_absolute() {
    path.to_path_buf()
  } else {
    match std::env::current_dir() {
      Ok(cwd) => cwd.join(path),
      Err(_) => path.to_path_buf(),
    }
  };
  lexical(&absolute)
}

/// Whether `path` is the bundle `root` or a path inside it.
///
/// `Follow` resolves a final symlink. `Entry` keeps that directory entry, so
/// a symlink stored outside the bundle is outside even when its target is
/// inside. `..` is applied after symlink resolution, matching the operating
/// system.
#[cfg(test)]
pub(super) fn targets_bundle(
  root: &Path,
  path: &Path,
  link: Link,
) -> Result<bool, FsError> {
  let resolved = resolve_path(path, link)?;
  Ok(inside(root, &resolved))
}

/// Resolves `path` and rejects a target inside the packaged bundle.
///
/// The returned path is the one the caller should pass to the operating
/// system. With no bundle root, the path is still resolved.
pub(super) fn ensure_mutable(
  path: &Path,
  link: Link,
) -> Result<PathBuf, FsError> {
  let resolved = resolve_path(path, link)?;
  if let Some(root) = super::bundle_root()
    && inside(&root, &resolved)
  {
    return Err(FsError::bundle());
  }
  Ok(resolved)
}

fn inside(root: &Path, path: &Path) -> bool {
  path == root || path.starts_with(root)
}

/// Rejects `path` when a bundle root is set and the path is inside it.
///
/// A path that cannot be canonicalized is rejected too. With no bundle root
/// this is a no-op, so callers that already know the root is absent can skip
/// the call.
pub(super) fn ensure_outside_bundle(path: &Path) -> Result<(), FsError> {
  let Some(root) = super::bundle_root() else {
    return Ok(());
  };
  let canonical = crate::canonicalize(path).map_err(|_| FsError::bundle())?;
  if inside(&root, &canonical) {
    return Err(FsError::bundle());
  }
  Ok(())
}

fn resolve_path(path: &Path, link: Link) -> Result<PathBuf, FsError> {
  let absolute = if path.is_absolute() {
    path.to_path_buf()
  } else {
    std::env::current_dir().map_err(FsError::io)?.join(path)
  };
  if link == Link::Follow
    && let Ok(canonical) = crate::canonicalize(&absolute)
  {
    return Ok(canonical);
  }
  let walked = walk(&absolute, link, 0)?;
  Ok(pin_existing_prefix(walked, link))
}

/// Canonicalizes the longest prefix that exists.
///
/// `walk` keeps the caller's spelling. Windows can spell that prefix as an
/// 8.3 short name, and a case-insensitive volume can spell it in another
/// case. The bundle root is canonical, so either spelling would look outside
/// the bundle. `Entry` leaves a final symlink as that directory entry.
fn pin_existing_prefix(path: PathBuf, link: Link) -> PathBuf {
  if link == Link::Entry
    && std::fs::symlink_metadata(&path)
      .is_ok_and(|meta| meta.file_type().is_symlink())
  {
    return pin_parent(&path);
  }
  canonicalize_existing_prefix(path)
}

fn pin_parent(path: &Path) -> PathBuf {
  let Some(name) = path.file_name() else {
    return path.to_path_buf();
  };
  let Some(parent) = path.parent() else {
    return path.to_path_buf();
  };
  if parent.as_os_str().is_empty() {
    return path.to_path_buf();
  }
  match crate::canonicalize(parent) {
    Ok(canonical) => canonical.join(name),
    Err(_) => path.to_path_buf(),
  }
}

fn canonicalize_existing_prefix(path: PathBuf) -> PathBuf {
  let mut suffix = Vec::new();
  let mut current = path.clone();
  loop {
    match crate::canonicalize(&current) {
      Ok(canonical) => {
        let mut out = canonical;
        for part in suffix.iter().rev() {
          out.push(part);
        }
        return out;
      }
      Err(error) if error.kind() == io::ErrorKind::NotFound => {
        let Some(name) = current.file_name() else {
          return path;
        };
        let name = name.to_os_string();
        let mut parent = current.clone();
        if !parent.pop() || parent == current {
          return path;
        }
        suffix.push(name);
        current = parent;
      }
      Err(_) => return path,
    }
  }
}

/// Follows symlinks and applies `..` to the resolved path. A missing tail
/// cannot contain a symlink, so `..` there only pops path components.
fn walk(path: &Path, link: Link, depth: u32) -> Result<PathBuf, FsError> {
  if depth > 40 {
    return Err(too_many_links());
  }
  let components: Vec<Component> = path.components().collect();
  let mut out = PathBuf::new();
  let mut index = 0;
  while index < components.len() {
    let component = components[index];
    let is_final = index + 1 == components.len();
    match component {
      Component::CurDir => {}
      Component::ParentDir => {
        out.pop();
      }
      Component::Prefix(_) | Component::RootDir => out.push(component),
      Component::Normal(name) => {
        if is_final && link == Link::Entry {
          out.push(name);
        } else {
          let candidate = out.join(name);
          match std::fs::symlink_metadata(&candidate) {
            Ok(meta) if meta.file_type().is_symlink() => {
              let target =
                std::fs::read_link(&candidate).map_err(FsError::io)?;
              let mut rest = if target.is_absolute() {
                target
              } else {
                out.join(target)
              };
              for later in components.iter().skip(index + 1) {
                rest.push(later);
              }
              return walk(&rest, link, depth + 1);
            }
            Ok(_) => out.push(name),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
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
            Err(error) => return Err(FsError::io(error)),
          }
        }
      }
    }
    index += 1;
  }
  Ok(out)
}

fn too_many_links() -> FsError {
  #[cfg(unix)]
  let code = libc::ELOOP;
  #[cfg(not(unix))]
  let code = 1921;
  FsError::io(io::Error::from_raw_os_error(code))
}

#[cfg(test)]
mod tests {
  use std::path::PathBuf;
  use std::sync::atomic::{AtomicU64, Ordering};
  use std::time::{SystemTime, UNIX_EPOCH};

  use super::{Link, targets_bundle};

  struct Fixture {
    path: PathBuf,
    root: PathBuf,
  }

  impl Fixture {
    fn new() -> Self {
      static NEXT: AtomicU64 = AtomicU64::new(0);
      let tick = NEXT.fetch_add(1, Ordering::Relaxed);
      let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
      let path = std::env::temp_dir().join(format!(
        "yuzuko-fs-{}-{}-{}",
        std::process::id(),
        nanos,
        tick
      ));
      std::fs::create_dir_all(&path).unwrap();
      let bundle = path.join("bundle");
      std::fs::create_dir(&bundle).unwrap();
      let root = crate::canonicalize(&bundle).unwrap();
      Self { path, root }
    }
  }

  impl Drop for Fixture {
    fn drop(&mut self) {
      let _ = std::fs::remove_dir_all(&self.path);
    }
  }

  #[test]
  fn bundle_membership_uses_canonical_paths() {
    let fixture = Fixture::new();
    let save = fixture.root.join("save.json");
    assert!(targets_bundle(&fixture.root, &save, Link::Follow).unwrap());
    assert!(targets_bundle(&fixture.root, &save, Link::Entry).unwrap());
    assert!(
      targets_bundle(&fixture.root, &fixture.root, Link::Follow).unwrap()
    );
    assert!(targets_bundle(&fixture.root, &fixture.root, Link::Entry).unwrap());

    let outside = fixture.path.join("missing").join("save.json");
    assert!(!targets_bundle(&fixture.root, &outside, Link::Follow).unwrap());

    let mut cousin = fixture.root.file_name().unwrap().to_os_string();
    cousin.push("-extra");
    let cousin = fixture
      .root
      .parent()
      .unwrap()
      .join(cousin)
      .join("save.json");
    assert!(!targets_bundle(&fixture.root, &cousin, Link::Follow).unwrap());

    let outside_dir = fixture.path.join("outside");
    std::fs::create_dir(&outside_dir).unwrap();
    let sneaky = outside_dir
      .join("..")
      .join(fixture.root.file_name().unwrap())
      .join("save.json");
    assert!(targets_bundle(&fixture.root, &sneaky, Link::Follow).unwrap());
  }

  /// A case-insensitive volume keeps the created spelling in `canonicalize`
  /// and accepts another spelling in `symlink_metadata`. The bundle root uses
  /// the created spelling, so the lookup has to land on that same path.
  #[cfg(any(target_os = "macos", target_os = "windows"))]
  #[test]
  fn bundle_membership_folds_the_directory_spelling() {
    let fixture = Fixture::new();
    let created = fixture.path.join("BundleCase");
    std::fs::create_dir(&created).unwrap();
    let root = crate::canonicalize(&created).unwrap();
    let created_name = root.file_name().unwrap().to_string_lossy();
    let spelled = created_name.to_lowercase();
    assert_ne!(
      spelled,
      created_name.as_ref(),
      "this volume does not preserve the created spelling"
    );
    let save = fixture.path.join(&spelled).join("save.json");
    assert!(targets_bundle(&root, &save, Link::Follow).unwrap());
    assert!(targets_bundle(&root, &save, Link::Entry).unwrap());

    let outside = fixture.path.join("outside");
    std::fs::create_dir(&outside).unwrap();
    let sneaky = outside.join("..").join(&spelled).join("save.json");
    assert!(targets_bundle(&root, &sneaky, Link::Follow).unwrap());

    let cousin = fixture
      .path
      .join(format!("{spelled}-extra"))
      .join("save.json");
    assert!(!targets_bundle(&root, &cousin, Link::Follow).unwrap());
  }

  #[cfg(unix)]
  #[test]
  fn symlink_follow_enters_the_bundle_and_the_entry_does_not() {
    let fixture = Fixture::new();
    let outside = fixture.path.join("outside");
    std::fs::create_dir(&outside).unwrap();
    let link = outside.join("link");
    std::os::unix::fs::symlink(&fixture.root, &link).unwrap();
    assert!(
      targets_bundle(&fixture.root, &link.join("save.json"), Link::Follow)
        .unwrap()
    );
    assert!(!targets_bundle(&fixture.root, &link, Link::Entry).unwrap());
    assert!(targets_bundle(&fixture.root, &link, Link::Follow).unwrap());

    let deep = fixture.root.join("nested").join("deep");
    std::fs::create_dir_all(&deep).unwrap();
    let into_bundle = outside.join("into-bundle");
    std::os::unix::fs::symlink(&deep, &into_bundle).unwrap();
    let slipped = into_bundle
      .join("foo")
      .join("..")
      .join("..")
      .join("leaf.txt");
    assert!(targets_bundle(&fixture.root, &slipped, Link::Follow).unwrap());
    assert!(targets_bundle(&fixture.root, &slipped, Link::Entry).unwrap());

    let escaped = link.join("..").join("sibling.txt");
    assert!(!targets_bundle(&fixture.root, &escaped, Link::Follow).unwrap());

    let loop_path = outside.join("loop");
    std::os::unix::fs::symlink("loop", &loop_path).unwrap();
    assert!(targets_bundle(&fixture.root, &loop_path, Link::Follow).is_err());
  }
}
