//! Directory-entry changes on an opened parent directory.
//!
//! The permission check reads the path. A sandboxed `remove`, `rename`,
//! `mkdir`, `link`, or `symlink` then opens the parent directory, checks the
//! real path of that descriptor joined with the final name, and changes the
//! entry through the descriptor. A parent swapped for a symlink after the
//! check cannot move the change outside the grant.

use std::ffi::OsString;
use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};

use super::error::FsError;
use super::io::{host_denial, host_error};

/// An opened parent directory whose real path joined with `name` passed the
/// write check.
pub(super) struct Parent {
  /// On Windows the open handle keeps the directory from being renamed.
  #[cfg_attr(windows, allow(dead_code))]
  dir: File,
  /// Windows names the entry by this final path while `dir` is open.
  #[cfg_attr(not(windows), allow(dead_code))]
  real: PathBuf,
  name: OsString,
}

impl Parent {
  /// The checked path of the entry.
  #[cfg_attr(not(windows), allow(dead_code))]
  pub(super) fn real_path(&self) -> PathBuf {
    self.real.join(&self.name)
  }
}

fn invalid() -> FsError {
  FsError::io(io::Error::from(io::ErrorKind::InvalidInput))
}

/// What the opened entry is checked for.
#[derive(Clone, Copy)]
pub(super) struct Need {
  /// Read as well as write, as a hard link needs for the old file.
  pub read: bool,
  /// A read deny of a descendant refuses the change. Used for the source of
  /// a rename, whose descendants move with it.
  pub read_tree: bool,
  /// A write deny of a descendant refuses the change, as for a recursive
  /// remove or either path of a rename.
  pub tree: bool,
}

pub(super) const WRITE: Need = Need {
  read: false,
  read_tree: false,
  tree: false,
};

/// Checks the directory entry `real`. A final symlink is not followed, so a
/// link is checked as the link. A denial names `requested`.
fn permit_entry(
  requested: &Path,
  real: &Path,
  need: Need,
) -> Result<(), FsError> {
  if need.read {
    match crate::permissions::entry_is_granted(false, real, need.read_tree) {
      Ok(true) => {}
      Ok(false) => return Err(host_denial(false, requested)),
      Err(error) => return Err(host_error(error)),
    }
  }
  match crate::permissions::entry_is_granted(true, real, need.tree) {
    Ok(true) => Ok(()),
    Ok(false) => Err(host_denial(true, requested)),
    Err(error) => Err(host_error(error)),
  }
}

/// Opens the parent of `path` and checks the entry. A denial names
/// `requested`.
pub(super) fn open_parent(
  requested: &Path,
  path: &Path,
  need: Need,
) -> Result<Parent, FsError> {
  let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else {
    return Err(invalid());
  };
  let parent = if parent.as_os_str().is_empty() {
    Path::new(".")
  } else {
    parent
  };
  let dir = open_dir(parent)?;
  let real =
    crate::fd_path::fd_path(&dir).map_err(|_| host_denial(true, requested))?;
  permit_entry(requested, &real.join(name), need)?;
  Ok(Parent {
    dir,
    real,
    name: name.to_os_string(),
  })
}

#[cfg(unix)]
fn open_dir(path: &Path) -> Result<File, FsError> {
  use std::os::unix::fs::OpenOptionsExt;
  fs::OpenOptions::new()
    .read(true)
    .custom_flags(libc::O_DIRECTORY | libc::O_CLOEXEC)
    .open(path)
    .map_err(FsError::io)
}

/// Without `FILE_SHARE_DELETE` the directory cannot be renamed or removed
/// while the handle is open.
#[cfg(windows)]
fn open_dir(path: &Path) -> Result<File, FsError> {
  use std::os::windows::fs::OpenOptionsExt;
  use windows_sys::Win32::Storage::FileSystem::{
    FILE_FLAG_BACKUP_SEMANTICS, FILE_READ_ATTRIBUTES, FILE_SHARE_READ,
    FILE_SHARE_WRITE,
  };
  fs::OpenOptions::new()
    .access_mode(FILE_READ_ATTRIBUTES)
    .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
    .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
    .open(path)
    .map_err(FsError::io)
}

#[cfg(unix)]
mod at {
  use std::ffi::{CStr, CString};
  use std::fs::File;
  use std::io;
  use std::os::fd::{AsRawFd, FromRawFd, RawFd};
  use std::os::unix::ffi::OsStrExt;

  use super::{FsError, Parent};

  pub(super) fn name(parent: &Parent) -> Result<CString, FsError> {
    CString::new(parent.name.as_bytes()).map_err(|_| super::invalid())
  }

  pub(super) fn check(rc: libc::c_int) -> io::Result<()> {
    if rc == 0 {
      Ok(())
    } else {
      Err(io::Error::last_os_error())
    }
  }

  fn is_dir(dir: RawFd, name: &CStr) -> io::Result<bool> {
    let mut stat = unsafe { std::mem::zeroed::<libc::stat>() };
    check(unsafe {
      libc::fstatat(dir, name.as_ptr(), &mut stat, libc::AT_SYMLINK_NOFOLLOW)
    })?;
    Ok((stat.st_mode & libc::S_IFMT) == libc::S_IFDIR)
  }

  /// Opens the directory `name` in `dir` without following a symlink.
  pub(super) fn open_child(dir: RawFd, name: &CStr) -> io::Result<File> {
    let fd = unsafe {
      libc::openat(
        dir,
        name.as_ptr(),
        libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
      )
    };
    if fd < 0 {
      return Err(io::Error::last_os_error());
    }
    Ok(unsafe { File::from_raw_fd(fd) })
  }

  fn child_names(dir: &File) -> io::Result<Vec<CString>> {
    let copy = unsafe { libc::dup(dir.as_raw_fd()) };
    if copy < 0 {
      return Err(io::Error::last_os_error());
    }
    let stream = unsafe { libc::fdopendir(copy) };
    if stream.is_null() {
      let error = io::Error::last_os_error();
      unsafe { libc::close(copy) };
      return Err(error);
    }
    let mut names = Vec::new();
    loop {
      let entry = unsafe { libc::readdir(stream) };
      if entry.is_null() {
        break;
      }
      let name = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) };
      if name.to_bytes() != b"." && name.to_bytes() != b".." {
        names.push(name.to_owned());
      }
    }
    unsafe { libc::closedir(stream) };
    Ok(names)
  }

  /// Removes `name` from `dir`. A directory is emptied through descriptors
  /// opened without following a symlink.
  pub(super) fn remove(
    dir: RawFd,
    name: &CStr,
    recursive: bool,
  ) -> io::Result<()> {
    if !is_dir(dir, name)? {
      return check(unsafe { libc::unlinkat(dir, name.as_ptr(), 0) });
    }
    if recursive {
      let child = open_child(dir, name)?;
      for entry in child_names(&child)? {
        remove(child.as_raw_fd(), &entry, true)?;
      }
    }
    check(unsafe { libc::unlinkat(dir, name.as_ptr(), libc::AT_REMOVEDIR) })
  }
}

pub(super) fn remove(parent: &Parent, recursive: bool) -> Result<(), FsError> {
  #[cfg(unix)]
  {
    use std::os::fd::AsRawFd;
    let name = at::name(parent)?;
    at::remove(parent.dir.as_raw_fd(), &name, recursive).map_err(FsError::io)
  }
  #[cfg(windows)]
  {
    // The final path names no reparse point, and the parent stays open.
    let path = parent.real_path();
    let meta = fs::symlink_metadata(&path).map_err(FsError::io)?;
    let result = if meta.file_type().is_symlink() || !meta.is_dir() {
      if meta.is_dir() {
        fs::remove_dir(&path)
      } else {
        fs::remove_file(&path)
      }
    } else if recursive {
      fs::remove_dir_all(&path)
    } else {
      fs::remove_dir(&path)
    };
    result.map_err(FsError::io)
  }
}

pub(super) fn rename(from: &Parent, to: &Parent) -> Result<(), FsError> {
  #[cfg(unix)]
  {
    use std::os::fd::AsRawFd;
    let (old, new) = (at::name(from)?, at::name(to)?);
    at::check(unsafe {
      libc::renameat(
        from.dir.as_raw_fd(),
        old.as_ptr(),
        to.dir.as_raw_fd(),
        new.as_ptr(),
      )
    })
    .map_err(FsError::io)
  }
  #[cfg(windows)]
  {
    fs::rename(from.real_path(), to.real_path()).map_err(FsError::io)
  }
}

pub(super) fn link(old: &Parent, new: &Parent) -> Result<(), FsError> {
  #[cfg(unix)]
  {
    use std::os::fd::AsRawFd;
    let (source, dest) = (at::name(old)?, at::name(new)?);
    at::check(unsafe {
      libc::linkat(
        old.dir.as_raw_fd(),
        source.as_ptr(),
        new.dir.as_raw_fd(),
        dest.as_ptr(),
        0,
      )
    })
    .map_err(FsError::io)
  }
  #[cfg(windows)]
  {
    fs::hard_link(old.real_path(), new.real_path()).map_err(FsError::io)
  }
}

/// Creates a symlink at the entry. On Windows the caller creates it at
/// [`Parent::real_path`].
#[cfg(unix)]
pub(super) fn symlink(target: &Path, at: &Parent) -> Result<(), FsError> {
  use std::os::fd::AsRawFd;
  let target = super::io::path_cstring(target)?;
  let name = at::name(at)?;
  at::check(unsafe {
    libc::symlinkat(target.as_ptr(), at.dir.as_raw_fd(), name.as_ptr())
  })
  .map_err(FsError::io)
}

fn dir_mode(mode: Option<u32>) -> u32 {
  mode.unwrap_or(0o777)
}

pub(super) fn mkdir(parent: &Parent, mode: Option<u32>) -> Result<(), FsError> {
  #[cfg(unix)]
  {
    use std::os::fd::AsRawFd;
    let name = at::name(parent)?;
    at::check(unsafe {
      libc::mkdirat(
        parent.dir.as_raw_fd(),
        name.as_ptr(),
        dir_mode(mode) as libc::mode_t,
      )
    })
    .map_err(FsError::io)
  }
  #[cfg(windows)]
  {
    let _ = dir_mode(mode);
    fs::create_dir(parent.real_path()).map_err(FsError::io)
  }
}

/// `mkdir -p` from the deepest existing ancestor. That ancestor is opened
/// and checked, and each new directory is opened without following a
/// symlink before the next one is made inside it.
pub(super) fn mkdir_recursive(
  requested: &Path,
  path: &Path,
  mode: Option<u32>,
) -> Result<(), FsError> {
  let mut base = path.to_path_buf();
  let mut pending = Vec::new();
  loop {
    match fs::symlink_metadata(&base) {
      Ok(_) => break,
      Err(error) if error.kind() == io::ErrorKind::NotFound => {
        let Some(name) = base.file_name().map(|name| name.to_os_string())
        else {
          return Err(FsError::io(error));
        };
        pending.push(name);
        if !base.pop() {
          return Err(FsError::io(error));
        }
      }
      Err(error) => return Err(FsError::io(error)),
    }
  }
  if pending.is_empty() {
    return match fs::metadata(path) {
      Ok(meta) if meta.is_dir() => Ok(()),
      Ok(_) => Err(FsError::io(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "File exists",
      ))),
      Err(error) => Err(FsError::io(error)),
    };
  }
  pending.reverse();
  let mut dir = open_dir(&base)?;
  let real =
    crate::fd_path::fd_path(&dir).map_err(|_| host_denial(true, requested))?;
  let target = pending
    .iter()
    .fold(real.clone(), |path, name| path.join(name));
  permit_entry(requested, &target, WRITE)?;
  #[cfg(unix)]
  {
    use std::os::fd::AsRawFd;
    use std::os::unix::ffi::OsStrExt;
    for name in &pending {
      let name =
        std::ffi::CString::new(name.as_bytes()).map_err(|_| invalid())?;
      let rc = unsafe {
        libc::mkdirat(
          dir.as_raw_fd(),
          name.as_ptr(),
          dir_mode(mode) as libc::mode_t,
        )
      };
      if rc != 0 {
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::AlreadyExists {
          return Err(FsError::io(error));
        }
      }
      dir = at::open_child(dir.as_raw_fd(), &name).map_err(FsError::io)?;
    }
    Ok(())
  }
  #[cfg(windows)]
  {
    let _ = mode;
    let mut current = real;
    for name in &pending {
      current.push(name);
      match fs::create_dir(&current) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(FsError::io(error)),
      }
      // A junction made in place of the new directory is refused.
      let meta = fs::symlink_metadata(&current).map_err(FsError::io)?;
      if !meta.is_dir() || meta.file_type().is_symlink() {
        return Err(host_denial(true, requested));
      }
      dir = open_dir(&current)?;
    }
    drop(dir);
    Ok(())
  }
}
