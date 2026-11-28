//! Operating-system file operations.
//!
//! Nothing in this module touches V8. A caller on the isolate thread may run
//! a synchronous operation directly. An asynchronous operation runs these
//! functions on a background thread and converts the [`FsError`] before
//! returning to JavaScript.

use std::fs::{self, DirBuilder, File, OpenOptions};
use std::io::{self, Read, Seek, Write};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use super::entry;
use super::error::FsError;
use super::path::{self, Link};

pub(super) const MAX_BUFFER: usize = 2_147_483_647;

#[derive(Clone, Copy)]
pub(super) struct OpenFlags {
  pub read: bool,
  pub write: bool,
  pub append: bool,
  pub truncate: bool,
  pub create: bool,
  pub create_new: bool,
  pub mode: Option<u32>,
}

impl OpenFlags {
  pub(super) fn read_only() -> Self {
    Self {
      read: true,
      write: false,
      append: false,
      truncate: false,
      create: false,
      create_new: false,
      mode: None,
    }
  }

  /// `create` and `createSync`: write, create, and truncate.
  pub(super) fn create_file() -> Self {
    Self {
      read: false,
      write: true,
      append: false,
      truncate: true,
      create: true,
      create_new: false,
      mode: None,
    }
  }

  pub(super) fn writes(self) -> bool {
    self.write || self.append || self.truncate || self.create || self.create_new
  }
}

#[derive(Clone, Copy)]
pub(super) struct WriteFlags {
  pub append: bool,
  pub create: bool,
  pub create_new: bool,
  pub mode: Option<u32>,
}

impl Default for WriteFlags {
  fn default() -> Self {
    Self {
      append: false,
      create: true,
      create_new: false,
      mode: None,
    }
  }
}

#[derive(Clone, Copy, Default)]
pub(super) struct MkdirFlags {
  pub recursive: bool,
  pub mode: Option<u32>,
}

#[derive(Clone, Copy, Default)]
pub(super) struct RemoveFlags {
  pub recursive: bool,
}

pub(super) struct TempSpec {
  pub dir: Option<PathBuf>,
  pub prefix: String,
  pub suffix: String,
}

pub(super) struct Info {
  pub is_file: bool,
  pub is_directory: bool,
  pub is_symlink: bool,
  pub size: u64,
  pub mtime: Option<SystemTime>,
  pub atime: Option<SystemTime>,
  pub birthtime: Option<SystemTime>,
  pub unix: UnixStat,
}

pub(super) struct UnixStat {
  pub mode: u32,
  pub dev: u64,
  pub ino: u64,
  pub nlink: u64,
  pub uid: u32,
  pub gid: u32,
  pub blksize: u64,
  pub blocks: u64,
  pub is_block_device: bool,
  pub is_char_device: bool,
  pub is_fifo: bool,
  pub is_socket: bool,
}

pub(super) struct Entry {
  pub name: String,
  pub is_file: bool,
  pub is_directory: bool,
  pub is_symlink: bool,
}

pub(super) fn open_file(
  path: &Path,
  flags: OpenFlags,
) -> Result<File, FsError> {
  if flags.writes() {
    // The bundle guard resolves the path. The open below keeps the caller's
    // spelling, including a Windows alternate data stream.
    path::ensure_mutable(path, Link::Follow)?;
  }
  let mut options = OpenOptions::new();
  options.read(flags.read);
  options.write(flags.write || flags.append || flags.truncate);
  options.append(flags.append);
  // Truncate after the opened descriptor is confirmed, so a swapped symlink
  // is not cleared before the denial.
  if flags.create_new {
    options.create_new(true);
  } else if flags.create {
    options.create(true);
  }
  apply_file_mode(&mut options, flags.mode);
  let file = options.open(path).map_err(FsError::io)?;
  permit_opened(path, &file, flags.read, flags.writes())?;
  if flags.truncate {
    file.set_len(0).map_err(FsError::io)?;
  }
  Ok(file)
}

pub(super) fn read_bytes(path: &Path) -> Result<Vec<u8>, FsError> {
  let mut file = match File::open(path) {
    Ok(file) => file,
    Err(error) => {
      // Windows refuses to open a directory. That is still `IsADirectory`.
      if let Ok(meta) = fs::metadata(path)
        && meta.is_dir()
      {
        return Err(FsError::directory());
      }
      return Err(FsError::io(error));
    }
  };
  permit_opened(path, &file, true, false)?;
  let meta = file.metadata().map_err(FsError::io)?;
  if meta.is_dir() {
    return Err(FsError::directory());
  }
  let mut bytes = Vec::new();
  file.read_to_end(&mut bytes).map_err(FsError::io)?;
  Ok(bytes)
}

pub(super) fn read_text(path: &Path) -> Result<String, FsError> {
  let bytes = read_bytes(path)?;
  String::from_utf8(bytes).map_err(|_| FsError::utf8())
}

pub(super) fn write_bytes(
  path: &Path,
  bytes: &[u8],
  flags: WriteFlags,
) -> Result<(), FsError> {
  path::ensure_mutable(path, Link::Follow)?;
  let mut options = OpenOptions::new();
  options.write(true);
  options.append(flags.append);
  options.create(flags.create && !flags.create_new);
  options.create_new(flags.create_new);
  apply_file_mode(&mut options, flags.mode);
  let mut file = options.open(path).map_err(FsError::io)?;
  permit_opened(path, &file, false, true)?;
  if !flags.append && !flags.create_new {
    file.set_len(0).map_err(FsError::io)?;
  }
  file.write_all(bytes).map_err(FsError::io)
}

/// The descriptor is the file the call may use. A path lookup after the
/// permission check can follow a different symlink; this refuses that file
/// and names the requested path.
fn permit_opened(
  path: &Path,
  file: &File,
  read: bool,
  write: bool,
) -> Result<(), FsError> {
  // Bundle membership is not a permission grant. An open policy still must
  // not truncate a packaged file that was swapped in after the path check.
  if write {
    path::reject_opened_bundle(file)?;
  }
  if !read && !write {
    return Ok(());
  }
  // No installed policy allows every path. Reading the descriptor path
  // first would reject that file when `F_GETPATH` or `/proc` is unavailable.
  if crate::permissions::permissions_are_open() {
    return Ok(());
  }
  let real = match crate::fd_path::fd_path(file) {
    Ok(real) => real,
    Err(_) => return Err(host_denial(write, path)),
  };
  permit_real(path, &real, read, write)
}

/// Checks `real`, the path the operating system reports for the file the
/// call opened. A denial names the requested `path`.
fn permit_real(
  path: &Path,
  real: &Path,
  read: bool,
  write: bool,
) -> Result<(), FsError> {
  if read {
    match crate::permissions::path_is_granted(false, real) {
      Ok(true) => {}
      Ok(false) => return Err(host_denial(false, path)),
      Err(error) => return Err(host_error(error)),
    }
  }
  if write {
    match crate::permissions::path_is_granted(true, real) {
      Ok(true) => {}
      Ok(false) => return Err(host_denial(true, path)),
      Err(error) => return Err(host_error(error)),
    }
  }
  Ok(())
}

pub(super) fn host_denial(write: bool, path: &Path) -> FsError {
  host_error(crate::permissions::path_not_allowed(write, path))
}

pub(super) fn host_error(error: crate::op::OpError) -> FsError {
  match error {
    crate::op::OpError::Dom { name, message } => {
      FsError::Host { name, message }
    }
    crate::op::OpError::Type(message) | crate::op::OpError::Range(message) => {
      FsError::Type(message)
    }
    crate::op::OpError::Thrown(_) => FsError::failed(),
  }
}

pub(super) fn write_text(
  path: &Path,
  text: &str,
  flags: WriteFlags,
) -> Result<(), FsError> {
  write_bytes(path, text.as_bytes(), flags)
}

pub(super) fn truncate_path(path: &Path, len: u64) -> Result<(), FsError> {
  // The bundle guard follows the link. The open keeps the caller's path,
  // and the length changes only after that descriptor is confirmed.
  path::ensure_mutable(path, Link::Follow)?;
  let file = OpenOptions::new()
    .write(true)
    .open(path)
    .map_err(FsError::io)?;
  permit_opened(path, &file, false, true)?;
  file.set_len(len).map_err(FsError::io)
}

pub(super) fn mkdir_path(
  path: &Path,
  flags: MkdirFlags,
) -> Result<(), FsError> {
  let requested = path;
  let path = path::ensure_mutable(path, Link::Entry)?;
  if sandboxed() {
    return if flags.recursive {
      entry::mkdir_recursive(requested, &path, flags.mode)
    } else {
      entry::mkdir(
        &entry::open_parent(requested, &path, entry::WRITE)?,
        flags.mode,
      )
    };
  }
  if flags.recursive {
    mkdir_recursive(&path, flags.mode)
  } else {
    create_dir(&path, flags.mode).map_err(FsError::io)
  }
}

/// A policy is installed, so a directory-entry change goes through an
/// opened, checked parent directory.
fn sandboxed() -> bool {
  !crate::permissions::permissions_are_open()
}

pub(super) fn remove_path(
  path: &Path,
  flags: RemoveFlags,
) -> Result<(), FsError> {
  let requested = path;
  let path = path::ensure_mutable(path, Link::Entry)?;
  if sandboxed() {
    let need = entry::Need {
      read: false,
      read_tree: false,
      tree: flags.recursive,
    };
    let parent = entry::open_parent(requested, &path, need)?;
    return entry::remove(&parent, flags.recursive);
  }
  let meta = fs::symlink_metadata(&path).map_err(FsError::io)?;
  let file_type = meta.file_type();
  if file_type.is_symlink() {
    fs::remove_file(&path).map_err(FsError::io)
  } else if file_type.is_dir() {
    if flags.recursive {
      fs::remove_dir_all(&path).map_err(FsError::io)
    } else {
      fs::remove_dir(&path).map_err(FsError::io)
    }
  } else {
    fs::remove_file(&path).map_err(FsError::io)
  }
}

pub(super) fn rename_path(from: &Path, to: &Path) -> Result<(), FsError> {
  let (from_requested, to_requested) = (from, to);
  let from = path::ensure_mutable(from, Link::Entry)?;
  let to = path::ensure_mutable(to, Link::Entry)?;
  if sandboxed() {
    // The opened parent can name a different directory than the first check.
    // The source moves its descendants, so read and write both cover the
    // tree. The destination needs read of that entry and write of its tree.
    let source = entry::open_parent(
      from_requested,
      &from,
      entry::Need {
        read: true,
        read_tree: true,
        tree: true,
      },
    )?;
    let dest = entry::open_parent(
      to_requested,
      &to,
      entry::Need {
        read: true,
        read_tree: false,
        tree: true,
      },
    )?;
    return entry::rename(&source, &dest);
  }
  fs::rename(from, to).map_err(FsError::io)
}

pub(super) fn copy_path(from: &Path, to: &Path) -> Result<(), FsError> {
  // The bundle guard follows the destination link. Both opens keep the
  // caller's paths. Bytes are written only after each descriptor is
  // confirmed. `create` can still leave an empty destination.
  path::ensure_mutable(to, Link::Follow)?;
  let mut source = File::open(from).map_err(FsError::io)?;
  permit_opened(from, &source, true, false)?;
  let meta = source.metadata().map_err(FsError::io)?;
  if meta.is_dir() {
    return Err(FsError::directory());
  }
  let mut dest = OpenOptions::new()
    .write(true)
    .create(true)
    .truncate(false)
    .open(to)
    .map_err(FsError::io)?;
  permit_opened(to, &dest, false, true)?;
  // Truncating first would empty the file when both descriptors name it,
  // including two hard links.
  if same_file(&source, &dest)? {
    return Ok(());
  }
  dest.set_len(0).map_err(FsError::io)?;
  io::copy(&mut source, &mut dest).map_err(FsError::io)?;
  #[cfg(unix)]
  {
    dest
      .set_permissions(meta.permissions())
      .map_err(FsError::io)?;
  }
  Ok(())
}

/// Whether both descriptors name one file. A hard link counts.
fn same_file(left: &File, right: &File) -> Result<bool, FsError> {
  #[cfg(unix)]
  {
    use std::os::unix::fs::MetadataExt;
    let left = left.metadata().map_err(FsError::io)?;
    let right = right.metadata().map_err(FsError::io)?;
    return Ok(left.dev() == right.dev() && left.ino() == right.ino());
  }
  #[cfg(windows)]
  {
    use std::os::windows::fs::MetadataExt;
    let left = left.metadata().map_err(FsError::io)?;
    let right = right.metadata().map_err(FsError::io)?;
    return Ok(matches!(
      (
        left.volume_serial_number(),
        left.file_index(),
        right.volume_serial_number(),
        right.file_index(),
      ),
      (Some(left_volume), Some(left_index), Some(right_volume), Some(right_index))
        if left_volume == right_volume && left_index == right_index
    ));
  }
  #[cfg(not(any(unix, windows)))]
  {
    let _ = (left, right);
    Ok(false)
  }
}

pub(super) fn real_path(path: &Path) -> Result<String, FsError> {
  let canonical = crate::canonicalize(path).map_err(FsError::io)?;
  // A swap after the caller's check must not return the other file's path.
  match crate::permissions::path_is_granted(false, &canonical) {
    Ok(true) => Ok(canonical.to_string_lossy().into_owned()),
    Ok(false) => Err(host_denial(false, path)),
    Err(error) => Err(host_error(error)),
  }
}

pub(super) fn stat_path(path: &Path, follow: bool) -> Result<Info, FsError> {
  let meta = if follow {
    fs::metadata(path)
  } else {
    fs::symlink_metadata(path)
  }
  .map_err(FsError::io)?;
  Ok(Info::from_metadata(&meta))
}

pub(super) fn stat_file(file: &File) -> Result<Info, FsError> {
  let meta = file.metadata().map_err(FsError::io)?;
  Ok(Info::from_metadata(&meta))
}

pub(super) fn read_directory(path: &Path) -> Result<Vec<Entry>, FsError> {
  // A sandboxed listing reads the directory it opened and checked, so a
  // symlink swapped after the permission check cannot name another one.
  let out = if crate::permissions::permissions_are_open() {
    read_directory_path(path)?
  } else {
    read_directory_checked(path)?
  };
  if out.len() > i32::MAX as usize {
    return Err(FsError::failed());
  }
  Ok(out)
}

fn read_directory_path(path: &Path) -> Result<Vec<Entry>, FsError> {
  let entries = fs::read_dir(path).map_err(FsError::io)?;
  let mut out = Vec::new();
  for entry in entries {
    let entry = entry.map_err(FsError::io)?;
    let file_type = entry.file_type().map_err(FsError::io)?;
    out.push(Entry {
      name: entry.file_name().to_string_lossy().into_owned(),
      is_file: file_type.is_file(),
      is_directory: file_type.is_dir(),
      is_symlink: file_type.is_symlink(),
    });
  }
  Ok(out)
}

#[cfg(target_os = "linux")]
fn read_directory_checked(path: &Path) -> Result<Vec<Entry>, FsError> {
  use std::os::fd::AsRawFd;
  use std::os::unix::fs::OpenOptionsExt;
  let dir = OpenOptions::new()
    .read(true)
    .custom_flags(libc::O_DIRECTORY)
    .open(path)
    .map_err(FsError::io)?;
  permit_opened(path, &dir, true, false)?;
  // `/proc/self/fd/N` names the opened directory itself.
  read_directory_path(Path::new(&format!("/proc/self/fd/{}", dir.as_raw_fd())))
}

#[cfg(all(unix, not(target_os = "linux")))]
fn read_directory_checked(path: &Path) -> Result<Vec<Entry>, FsError> {
  use std::os::fd::{AsRawFd, IntoRawFd};
  use std::os::unix::ffi::OsStrExt;
  use std::os::unix::fs::OpenOptionsExt;
  let dir = OpenOptions::new()
    .read(true)
    .custom_flags(libc::O_DIRECTORY)
    .open(path)
    .map_err(FsError::io)?;
  permit_opened(path, &dir, true, false)?;
  let fd = dir.as_raw_fd();
  let stream_fd = dir.try_clone().map_err(FsError::io)?.into_raw_fd();
  let stream = unsafe { libc::fdopendir(stream_fd) };
  if stream.is_null() {
    let error = io::Error::last_os_error();
    unsafe { libc::close(stream_fd) };
    return Err(FsError::io(error));
  }
  struct Stream(*mut libc::DIR);
  impl Drop for Stream {
    fn drop(&mut self) {
      unsafe { libc::closedir(self.0) };
    }
  }
  let stream = Stream(stream);
  let mut out = Vec::new();
  loop {
    unsafe { *libc::__error() = 0 };
    let entry = unsafe { libc::readdir(stream.0) };
    if entry.is_null() {
      let error = io::Error::last_os_error();
      if error.raw_os_error().unwrap_or(0) != 0 {
        return Err(FsError::io(error));
      }
      break;
    }
    let name = unsafe { std::ffi::CStr::from_ptr((*entry).d_name.as_ptr()) };
    let bytes = name.to_bytes();
    if bytes == b"." || bytes == b".." {
      continue;
    }
    let mut kind = unsafe { (*entry).d_type };
    if kind == libc::DT_UNKNOWN {
      let mut stat = unsafe { std::mem::zeroed::<libc::stat>() };
      let rc = unsafe {
        libc::fstatat(fd, name.as_ptr(), &mut stat, libc::AT_SYMLINK_NOFOLLOW)
      };
      if rc != 0 {
        return Err(FsError::io(io::Error::last_os_error()));
      }
      kind = match stat.st_mode & libc::S_IFMT {
        libc::S_IFREG => libc::DT_REG,
        libc::S_IFDIR => libc::DT_DIR,
        libc::S_IFLNK => libc::DT_LNK,
        _ => libc::DT_UNKNOWN,
      };
    }
    out.push(Entry {
      name: std::ffi::OsStr::from_bytes(bytes)
        .to_string_lossy()
        .into_owned(),
      is_file: kind == libc::DT_REG,
      is_directory: kind == libc::DT_DIR,
      is_symlink: kind == libc::DT_LNK,
    });
  }
  Ok(out)
}

#[cfg(windows)]
fn read_directory_checked(path: &Path) -> Result<Vec<Entry>, FsError> {
  use std::os::windows::fs::OpenOptionsExt;
  use windows_sys::Win32::Storage::FileSystem::{
    FILE_FLAG_BACKUP_SEMANTICS, FILE_SHARE_READ, FILE_SHARE_WRITE,
  };
  // Without `FILE_SHARE_DELETE` the directory cannot be renamed or removed
  // while this handle is open, so its final path stays this directory.
  let dir = OpenOptions::new()
    .read(true)
    .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
    .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
    .open(path)
    .map_err(FsError::io)?;
  permit_opened(path, &dir, true, false)?;
  let real = crate::fd_path::fd_path(&dir).map_err(FsError::io)?;
  read_directory_path(&real)
}

#[cfg(not(any(unix, windows)))]
fn read_directory_checked(path: &Path) -> Result<Vec<Entry>, FsError> {
  read_directory_path(path)
}

pub(super) fn make_temp(
  directory: bool,
  spec: TempSpec,
) -> Result<String, FsError> {
  let dir = spec.dir.unwrap_or_else(std::env::temp_dir);
  let pid = std::process::id();
  for attempt in 0..64u32 {
    let nanos = SystemTime::now()
      .duration_since(SystemTime::UNIX_EPOCH)
      .map(|duration| duration.as_nanos())
      .unwrap_or(0);
    let name =
      format!("{}{nanos:x}-{pid}-{attempt:x}{}", spec.prefix, spec.suffix);
    let candidate = dir.join(name);
    let candidate = path::ensure_mutable(&candidate, Link::Entry)?;
    let created = if directory {
      create_dir(&candidate, Some(0o700))
    } else {
      let mut options = OpenOptions::new();
      apply_file_mode(&mut options, Some(0o600));
      options
        .write(true)
        .create_new(true)
        .open(&candidate)
        .map(|_| ())
    };
    match created {
      Ok(()) => return Ok(candidate.to_string_lossy().into_owned()),
      Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
      Err(error) => return Err(FsError::io(error)),
    }
  }
  Err(FsError::io(io::Error::new(
    io::ErrorKind::AlreadyExists,
    "Could not create a unique temporary path.",
  )))
}

pub(super) fn read_at(
  file: &mut File,
  requested: usize,
) -> Result<Vec<u8>, FsError> {
  let mut bytes = vec![0u8; requested];
  let count = file.read(&mut bytes).map_err(FsError::io)?;
  bytes.truncate(count);
  Ok(bytes)
}

pub(super) fn write_at(
  file: &mut File,
  bytes: &[u8],
) -> Result<usize, FsError> {
  // A pipe can accept fewer bytes than requested. Retry until the buffer
  // is written, including after `Interrupted`.
  file.write_all(bytes).map_err(FsError::io)?;
  Ok(bytes.len())
}

pub(super) fn seek_file(
  file: &mut File,
  from: io::SeekFrom,
) -> Result<u64, FsError> {
  file.seek(from).map_err(FsError::io)
}

pub(super) fn truncate_file(file: &mut File, len: u64) -> Result<(), FsError> {
  file.set_len(len).map_err(FsError::io)
}

pub(super) fn sync_file(file: &File, data_only: bool) -> Result<(), FsError> {
  if data_only {
    file.sync_data().map_err(FsError::io)
  } else {
    file.sync_all().map_err(FsError::io)
  }
}

pub(super) fn seek_from(
  offset: f64,
  whence: f64,
) -> Result<io::SeekFrom, FsError> {
  if whence != 0.0 && whence != 1.0 && whence != 2.0 {
    return Err(FsError::type_("whence must be 0, 1, or 2."));
  }
  if !offset.is_finite() {
    return Err(FsError::type_("The seek offset must be a finite number."));
  }
  if offset.fract() != 0.0 {
    return Err(FsError::type_("The seek offset must be an integer."));
  }
  if whence == 0.0 {
    if offset < 0.0 {
      return Err(FsError::type_("A seek from the start cannot be negative."));
    }
    Ok(io::SeekFrom::Start(offset as u64))
  } else if whence == 1.0 {
    Ok(io::SeekFrom::Current(offset as i64))
  } else {
    Ok(io::SeekFrom::End(offset as i64))
  }
}

pub(super) fn byte_length(value: f64) -> Result<u64, FsError> {
  if !value.is_finite() || value.fract() != 0.0 || value < 0.0 {
    return Err(FsError::type_(
      "The length must be an integer that is at least 0.",
    ));
  }
  Ok(value as u64)
}

pub(super) fn read_outcome(read: usize, requested: usize) -> Option<u32> {
  if read == 0 && requested > 0 {
    None
  } else {
    Some(u32::try_from(read).unwrap_or(u32::MAX))
  }
}

impl Info {
  fn from_metadata(meta: &fs::Metadata) -> Self {
    let file_type = meta.file_type();
    Self {
      is_file: meta.is_file(),
      is_directory: meta.is_dir(),
      is_symlink: file_type.is_symlink(),
      size: meta.len(),
      mtime: meta.modified().ok(),
      atime: meta.accessed().ok(),
      birthtime: meta.created().ok(),
      unix: unix_stat(meta),
    }
  }
}

fn mkdir_recursive(path: &Path, mode: Option<u32>) -> Result<(), FsError> {
  let mut pending = Vec::new();
  let mut current = path.to_path_buf();
  loop {
    match fs::metadata(&current) {
      Ok(meta) if meta.is_dir() => break,
      Ok(_) => {
        return Err(FsError::io(io::Error::new(
          io::ErrorKind::AlreadyExists,
          "File exists",
        )));
      }
      Err(error) if error.kind() == io::ErrorKind::NotFound => {
        let Some(name) = current.file_name().map(|name| name.to_os_string())
        else {
          break;
        };
        if !current.pop() {
          break;
        }
        pending.push(name);
      }
      Err(error) => return Err(FsError::io(error)),
    }
  }
  for name in pending.into_iter().rev() {
    current.push(name);
    match create_dir(&current, mode) {
      Ok(()) => {}
      Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
        match fs::metadata(&current) {
          Ok(meta) if meta.is_dir() => {}
          _ => return Err(FsError::io(error)),
        }
      }
      Err(error) => return Err(FsError::io(error)),
    }
  }
  Ok(())
}

fn create_dir(path: &Path, mode: Option<u32>) -> io::Result<()> {
  let mut builder = DirBuilder::new();
  apply_dir_mode(&mut builder, mode);
  builder.create(path)
}

fn apply_file_mode(options: &mut OpenOptions, mode: Option<u32>) {
  let Some(mode) = mode else {
    return;
  };
  #[cfg(unix)]
  {
    use std::os::unix::fs::OpenOptionsExt;
    options.mode(mode);
  }
  #[cfg(not(unix))]
  {
    let _ = (options, mode);
  }
}

fn apply_dir_mode(builder: &mut DirBuilder, mode: Option<u32>) {
  let Some(mode) = mode else {
    return;
  };
  #[cfg(unix)]
  {
    use std::os::unix::fs::DirBuilderExt;
    builder.mode(mode);
  }
  #[cfg(not(unix))]
  {
    let _ = (builder, mode);
  }
}

#[cfg(unix)]
fn unix_stat(meta: &fs::Metadata) -> UnixStat {
  use std::os::unix::fs::{FileTypeExt, MetadataExt};
  let file_type = meta.file_type();
  UnixStat {
    mode: meta.mode(),
    dev: meta.dev(),
    ino: meta.ino(),
    nlink: meta.nlink(),
    uid: meta.uid(),
    gid: meta.gid(),
    blksize: meta.blksize(),
    blocks: meta.blocks(),
    is_block_device: file_type.is_block_device(),
    is_char_device: file_type.is_char_device(),
    is_fifo: file_type.is_fifo(),
    is_socket: file_type.is_socket(),
  }
}

#[cfg(not(unix))]
fn unix_stat(_meta: &fs::Metadata) -> UnixStat {
  UnixStat {
    mode: 0,
    dev: 0,
    ino: 0,
    nlink: 0,
    uid: 0,
    gid: 0,
    blksize: 0,
    blocks: 0,
    is_block_device: false,
    is_char_device: false,
    is_fifo: false,
    is_socket: false,
  }
}

/// Seconds since the Unix epoch. Fractional seconds are kept.
#[derive(Clone, Copy)]
pub(super) struct FileTime {
  pub seconds: f64,
}

#[derive(Clone, Copy)]
pub(super) enum LinkKind {
  File,
  Dir,
  Junction,
}

/// Saved terminal mode for [`set_raw`]. The first raw transition stores it.
/// Later transitions start from that copy, and close restores it.
pub(super) struct TerminalState {
  #[cfg(unix)]
  original: Option<libc::termios>,
  #[cfg(windows)]
  original: Option<u32>,
  #[cfg(not(any(unix, windows)))]
  original: Option<()>,
}

impl TerminalState {
  pub(super) fn new() -> Self {
    Self { original: None }
  }
}

struct TimeParts {
  sec: i64,
  nsec: i64,
}

pub(super) fn chmod(path: &Path, mode: u32) -> Result<(), FsError> {
  let path = path::ensure_mutable(path, Link::Follow)?;
  #[cfg(unix)]
  {
    match metadata_target(&path)? {
      Target::Path(path) => {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(mode))
          .map_err(FsError::io)
      }
      #[cfg(target_os = "linux")]
      Target::Fd(file) => {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(proc_path(&file), fs::Permissions::from_mode(mode))
          .map_err(FsError::io)
      }
      #[cfg(not(target_os = "linux"))]
      Target::Fd(file) => {
        use std::os::unix::fs::PermissionsExt;
        file
          .set_permissions(fs::Permissions::from_mode(mode))
          .map_err(FsError::io)
      }
      #[cfg(not(target_os = "linux"))]
      Target::Entry { dir, name } => {
        use std::os::fd::AsRawFd;
        let rc = unsafe {
          libc::fchmodat(
            dir.as_raw_fd(),
            name.as_ptr(),
            mode as libc::mode_t,
            libc::AT_SYMLINK_NOFOLLOW,
          )
        };
        last_result(rc)
      }
    }
  }
  #[cfg(windows)]
  {
    let file = checked_attributes(&path)?;
    let meta = file.metadata().map_err(FsError::io)?;
    let mut permissions = meta.permissions();
    permissions.set_readonly(mode & 0o200 == 0);
    file.set_permissions(permissions).map_err(FsError::io)
  }
  #[cfg(not(any(unix, windows)))]
  {
    let _ = (path, mode);
    Err(FsError::not_supported(
      "chmod is not supported on this operating system.",
    ))
  }
}

/// The file a metadata change acts on. A sandboxed call opens the file
/// before the change and checks the path of that descriptor, so a symlink
/// swapped after the permission check cannot name another file.
#[cfg(unix)]
enum Target {
  /// No policy is installed. The path is used as given.
  Path(PathBuf),
  Fd(File),
  /// The file could not be opened without read access. The change uses this
  /// entry of the opened directory and does not follow a symlink there.
  #[cfg(not(target_os = "linux"))]
  Entry {
    dir: File,
    name: std::ffi::CString,
  },
}

#[cfg(target_os = "linux")]
fn metadata_target(path: &Path) -> Result<Target, FsError> {
  use std::os::unix::fs::OpenOptionsExt;
  if crate::permissions::permissions_are_open() {
    return Ok(Target::Path(path.to_path_buf()));
  }
  // `O_PATH` needs no read access to the file.
  let file = OpenOptions::new()
    .read(true)
    .custom_flags(libc::O_PATH)
    .open(path)
    .map_err(FsError::io)?;
  permit_opened(path, &file, false, true)?;
  Ok(Target::Fd(file))
}

#[cfg(all(unix, not(target_os = "linux")))]
fn metadata_target(path: &Path) -> Result<Target, FsError> {
  use std::os::unix::ffi::OsStrExt;
  use std::os::unix::fs::OpenOptionsExt;
  if crate::permissions::permissions_are_open() {
    return Ok(Target::Path(path.to_path_buf()));
  }
  let opened = OpenOptions::new()
    .read(true)
    .custom_flags(libc::O_NONBLOCK | libc::O_NOCTTY)
    .open(path);
  let denied = match opened {
    Ok(file) => {
      permit_opened(path, &file, false, true)?;
      return Ok(Target::Fd(file));
    }
    Err(error) if error.kind() == io::ErrorKind::PermissionDenied => error,
    Err(error) => return Err(FsError::io(error)),
  };
  let real = fs::canonicalize(path).map_err(FsError::io)?;
  let (Some(parent), Some(name)) = (real.parent(), real.file_name()) else {
    return Err(FsError::io(denied));
  };
  let dir = OpenOptions::new()
    .read(true)
    .custom_flags(libc::O_DIRECTORY)
    .open(parent)
    .map_err(|_| FsError::io(denied))?;
  let opened_dir =
    crate::fd_path::fd_path(&dir).map_err(|_| host_denial(true, path))?;
  permit_real(path, &opened_dir.join(name), false, true)?;
  let name = std::ffi::CString::new(name.as_bytes())
    .map_err(|_| FsError::io(io::Error::from(io::ErrorKind::InvalidInput)))?;
  Ok(Target::Entry { dir, name })
}

/// `/proc/self/fd/N` names the opened file itself.
#[cfg(target_os = "linux")]
fn proc_path(file: &File) -> PathBuf {
  use std::os::fd::AsRawFd;
  PathBuf::from(format!("/proc/self/fd/{}", file.as_raw_fd()))
}

#[cfg(unix)]
fn last_result(rc: libc::c_int) -> Result<(), FsError> {
  if rc == 0 {
    Ok(())
  } else {
    Err(FsError::io(io::Error::last_os_error()))
  }
}

/// Opens `path` for an attribute change and checks the opened file.
#[cfg(windows)]
fn checked_attributes(path: &Path) -> Result<File, FsError> {
  let file = File::from(open_attributes(path)?);
  permit_opened(path, &file, false, true)?;
  Ok(file)
}

pub(super) fn chown(
  path: &Path,
  uid: Option<u32>,
  gid: Option<u32>,
) -> Result<(), FsError> {
  #[cfg(windows)]
  {
    let _ = (path, uid, gid);
    Err(FsError::not_supported(
      "Changing the owner is not supported on Windows.",
    ))
  }
  #[cfg(unix)]
  {
    let path = path::ensure_mutable(path, Link::Follow)?;
    let uid = uid.unwrap_or(u32::MAX) as libc::uid_t;
    let gid = gid.unwrap_or(u32::MAX) as libc::gid_t;
    let rc = match metadata_target(&path)? {
      Target::Path(path) => {
        let path = path_cstring(&path)?;
        unsafe { libc::chown(path.as_ptr(), uid, gid) }
      }
      #[cfg(target_os = "linux")]
      Target::Fd(file) => {
        let path = path_cstring(&proc_path(&file))?;
        unsafe { libc::chown(path.as_ptr(), uid, gid) }
      }
      #[cfg(not(target_os = "linux"))]
      Target::Fd(file) => {
        use std::os::fd::AsRawFd;
        unsafe { libc::fchown(file.as_raw_fd(), uid, gid) }
      }
      #[cfg(not(target_os = "linux"))]
      Target::Entry { dir, name } => {
        use std::os::fd::AsRawFd;
        unsafe {
          libc::fchownat(
            dir.as_raw_fd(),
            name.as_ptr(),
            uid,
            gid,
            libc::AT_SYMLINK_NOFOLLOW,
          )
        }
      }
    };
    last_result(rc)
  }
  #[cfg(not(any(unix, windows)))]
  {
    let _ = (path, uid, gid);
    Err(FsError::not_supported(
      "Changing the owner is not supported on this operating system.",
    ))
  }
}

/// Returns the previous mask. On Unix, an omitted mask is read by setting
/// the mask to 0 and restoring it. That window is visible to other threads.
pub(super) fn umask(mask: Option<u32>) -> Result<u32, FsError> {
  #[cfg(unix)]
  {
    let previous = unsafe {
      match mask {
        Some(mask) => libc::umask(mask as libc::mode_t),
        None => {
          let previous = libc::umask(0);
          libc::umask(previous);
          previous
        }
      }
    };
    Ok(previous as u32)
  }
  #[cfg(not(unix))]
  {
    let _ = mask;
    Ok(0)
  }
}

pub(super) fn link(old: &Path, new: &Path) -> Result<(), FsError> {
  let (old_requested, new_requested) = (old, new);
  let old = path::ensure_mutable(old, Link::Entry)?;
  let new = path::ensure_mutable(new, Link::Entry)?;
  if sandboxed() {
    // A hard link needs read and write access to the old file.
    let read_write = entry::Need {
      read: true,
      read_tree: false,
      tree: false,
    };
    let source = entry::open_parent(old_requested, &old, read_write)?;
    let dest = entry::open_parent(new_requested, &new, entry::WRITE)?;
    return entry::link(&source, &dest);
  }
  fs::hard_link(old, new).map_err(FsError::io)
}

pub(super) fn symlink(
  old: &Path,
  new: &Path,
  kind: LinkKind,
) -> Result<(), FsError> {
  let requested = new;
  let new = path::ensure_mutable(new, Link::Entry)?;
  let parent = if sandboxed() {
    Some(entry::open_parent(requested, &new, entry::WRITE)?)
  } else {
    None
  };
  #[cfg(unix)]
  {
    let _ = kind;
    if let Some(parent) = &parent {
      return entry::symlink(old, parent);
    }
    std::os::unix::fs::symlink(old, new).map_err(FsError::io)
  }
  #[cfg(windows)]
  {
    // The parent stays open, so its final path still names this entry.
    let new = parent.as_ref().map_or(new, entry::Parent::real_path);
    match kind {
      LinkKind::File => {
        std::os::windows::fs::symlink_file(old, &new).map_err(FsError::io)
      }
      LinkKind::Dir => {
        std::os::windows::fs::symlink_dir(old, &new).map_err(FsError::io)
      }
      LinkKind::Junction => junction(&new, old),
    }
  }
  #[cfg(not(any(unix, windows)))]
  {
    let _ = (old, new, kind);
    Err(FsError::not_supported(
      "symlink is not supported on this operating system.",
    ))
  }
}

pub(super) fn read_link(path: &Path) -> Result<String, FsError> {
  let target = fs::read_link(path).map_err(FsError::io)?;
  Ok(target.to_string_lossy().replace('\0', "\u{FFFD}"))
}

pub(super) fn check_time(time: FileTime) -> Result<(), FsError> {
  let parts = split_time(time.seconds)?;
  #[cfg(windows)]
  {
    filetime_ticks(parts)?;
  }
  #[cfg(not(windows))]
  {
    let _ = parts;
  }
  Ok(())
}

pub(super) fn utime(
  path: &Path,
  atime: FileTime,
  mtime: FileTime,
) -> Result<(), FsError> {
  let path = path::ensure_mutable(path, Link::Follow)?;
  set_times_path(&path, atime, mtime)
}

pub(super) fn utime_file(
  file: &File,
  atime: FileTime,
  mtime: FileTime,
) -> Result<(), FsError> {
  #[cfg(not(any(unix, windows)))]
  {
    let _ = (file, atime, mtime);
    return Err(FsError::not_supported(
      "utime is not supported on this operating system.",
    ));
  }
  #[cfg(any(unix, windows))]
  {
    path::reject_opened_bundle(file)?;
    set_times_file(file, atime, mtime)
  }
}

pub(super) fn lock_file(file: &File, exclusive: bool) -> Result<(), FsError> {
  lock_with(file, exclusive, false).map(|_| ())
}

pub(super) fn try_lock_file(
  file: &File,
  exclusive: bool,
) -> Result<bool, FsError> {
  lock_with(file, exclusive, true)
}

pub(super) fn unlock_file(file: &File) -> Result<(), FsError> {
  #[cfg(unix)]
  {
    flock_retry(file, libc::LOCK_UN).map_err(FsError::io)
  }
  #[cfg(windows)]
  {
    windows_lock(file, false, false, true).map(|_| ())
  }
  #[cfg(not(any(unix, windows)))]
  {
    let _ = file;
    Err(FsError::not_supported(
      "lock is not supported on this operating system.",
    ))
  }
}

pub(super) fn is_terminal(file: &File) -> Result<bool, FsError> {
  #[cfg(unix)]
  {
    use std::os::fd::AsRawFd;
    Ok(unsafe { libc::isatty(file.as_raw_fd()) } == 1)
  }
  #[cfg(windows)]
  {
    Ok(console_mode(file).is_ok())
  }
  #[cfg(not(any(unix, windows)))]
  {
    let _ = file;
    Ok(false)
  }
}

pub(super) fn set_raw(
  file: &File,
  state: &mut TerminalState,
  raw: bool,
  cbreak: bool,
) -> Result<(), FsError> {
  #[cfg(unix)]
  {
    set_raw_unix(file, state, raw, cbreak)
  }
  #[cfg(windows)]
  {
    set_raw_windows(file, state, raw, cbreak)
  }
  #[cfg(not(any(unix, windows)))]
  {
    let _ = (file, state, raw, cbreak);
    Err(FsError::not_supported("The file is not a terminal."))
  }
}

pub(super) fn restore_terminal(
  file: &File,
  state: &mut TerminalState,
) -> Result<(), FsError> {
  #[cfg(unix)]
  {
    if let Some(original) = state.original.take() {
      return tcsetattr(file, &original);
    }
    Ok(())
  }
  #[cfg(windows)]
  {
    if let Some(original) = state.original.take() {
      return set_console_mode(file, original);
    }
    Ok(())
  }
  #[cfg(not(any(unix, windows)))]
  {
    let _ = (file, state);
    Ok(())
  }
}

fn split_time(seconds: f64) -> Result<TimeParts, FsError> {
  if !seconds.is_finite() {
    return Err(FsError::type_(
      "The time must be a Date or a finite number.",
    ));
  }
  let sec = seconds.floor();
  if sec < i64::MIN as f64 || sec >= 9223372036854775808.0 {
    return Err(out_of_range());
  }
  let mut tv_sec = sec as i64;
  let mut nsec = ((seconds - sec) * 1_000_000_000.0).round();
  if nsec >= 1_000_000_000.0 {
    if tv_sec == i64::MAX {
      return Err(out_of_range());
    }
    tv_sec += 1;
    nsec -= 1_000_000_000.0;
  }
  if !(0.0..1_000_000_000.0).contains(&nsec) {
    return Err(out_of_range());
  }
  Ok(TimeParts {
    sec: tv_sec,
    nsec: nsec as i64,
  })
}

fn out_of_range() -> FsError {
  FsError::type_("The time is out of range.")
}

fn lock_with(
  file: &File,
  exclusive: bool,
  nonblocking: bool,
) -> Result<bool, FsError> {
  #[cfg(unix)]
  {
    let mut operation = if exclusive {
      libc::LOCK_EX
    } else {
      libc::LOCK_SH
    };
    if nonblocking {
      operation |= libc::LOCK_NB;
    }
    match flock_retry(file, operation) {
      Ok(()) => Ok(true),
      Err(error) if nonblocking && contended(&error) => Ok(false),
      Err(error) => Err(FsError::io(error)),
    }
  }
  #[cfg(windows)]
  {
    windows_lock(file, exclusive, nonblocking, false)
  }
  #[cfg(not(any(unix, windows)))]
  {
    let _ = (file, exclusive, nonblocking);
    Err(FsError::not_supported(
      "lock is not supported on this operating system.",
    ))
  }
}

fn contended(error: &io::Error) -> bool {
  if error.kind() == io::ErrorKind::WouldBlock {
    return true;
  }
  match error.raw_os_error() {
    #[cfg(unix)]
    Some(code) if code == libc::EAGAIN || code == libc::EWOULDBLOCK => true,
    #[cfg(windows)]
    Some(33) => true,
    _ => false,
  }
}

#[cfg(unix)]
fn flock_retry(file: &File, operation: libc::c_int) -> io::Result<()> {
  use std::os::fd::AsRawFd;
  loop {
    let rc = unsafe { libc::flock(file.as_raw_fd(), operation) };
    if rc == 0 {
      return Ok(());
    }
    let error = io::Error::last_os_error();
    if error.kind() == io::ErrorKind::Interrupted {
      continue;
    }
    return Err(error);
  }
}

#[cfg(unix)]
pub(super) fn path_cstring(path: &Path) -> Result<std::ffi::CString, FsError> {
  use std::os::unix::ffi::OsStrExt;
  std::ffi::CString::new(path.as_os_str().as_bytes()).map_err(|_| {
    FsError::io(io::Error::new(
      io::ErrorKind::InvalidInput,
      "nul byte found in provided data",
    ))
  })
}

#[cfg(unix)]
fn set_times_path(
  path: &Path,
  atime: FileTime,
  mtime: FileTime,
) -> Result<(), FsError> {
  let times = [time_spec(atime)?, time_spec(mtime)?];
  let rc = match metadata_target(path)? {
    Target::Path(path) => {
      let path = path_cstring(&path)?;
      unsafe {
        libc::utimensat(libc::AT_FDCWD, path.as_ptr(), times.as_ptr(), 0)
      }
    }
    #[cfg(target_os = "linux")]
    Target::Fd(file) => {
      let path = path_cstring(&proc_path(&file))?;
      unsafe {
        libc::utimensat(libc::AT_FDCWD, path.as_ptr(), times.as_ptr(), 0)
      }
    }
    #[cfg(not(target_os = "linux"))]
    Target::Fd(file) => {
      use std::os::fd::AsRawFd;
      unsafe { libc::futimens(file.as_raw_fd(), times.as_ptr()) }
    }
    #[cfg(not(target_os = "linux"))]
    Target::Entry { dir, name } => {
      use std::os::fd::AsRawFd;
      unsafe {
        libc::utimensat(
          dir.as_raw_fd(),
          name.as_ptr(),
          times.as_ptr(),
          libc::AT_SYMLINK_NOFOLLOW,
        )
      }
    }
  };
  last_result(rc)
}

#[cfg(unix)]
fn set_times_file(
  file: &File,
  atime: FileTime,
  mtime: FileTime,
) -> Result<(), FsError> {
  use std::os::fd::AsRawFd;
  let times = [time_spec(atime)?, time_spec(mtime)?];
  let rc = unsafe { libc::futimens(file.as_raw_fd(), times.as_ptr()) };
  if rc == 0 {
    Ok(())
  } else {
    Err(FsError::io(io::Error::last_os_error()))
  }
}

#[cfg(unix)]
fn time_spec(time: FileTime) -> Result<libc::timespec, FsError> {
  let parts = split_time(time.seconds)?;
  let mut spec = unsafe { std::mem::zeroed::<libc::timespec>() };
  spec.tv_sec = parts.sec as libc::time_t;
  spec.tv_nsec = parts.nsec as libc::c_long;
  Ok(spec)
}

#[cfg(unix)]
fn set_raw_unix(
  file: &File,
  state: &mut TerminalState,
  raw: bool,
  cbreak: bool,
) -> Result<(), FsError> {
  if !raw {
    if let Some(original) = state.original.take() {
      return tcsetattr(file, &original);
    }
    return Ok(());
  }
  if unsafe { libc::isatty(std::os::fd::AsRawFd::as_raw_fd(file)) } != 1 {
    return Err(FsError::not_supported("The file is not a terminal."));
  }
  if state.original.is_none() {
    let original = tcgetattr(file)?;
    state.original = Some(copy_termios(&original));
  }
  let Some(original) = state.original.as_ref() else {
    return Err(FsError::not_supported("The file is not a terminal."));
  };
  let mut next = copy_termios(original);
  apply_raw(&mut next, cbreak);
  tcsetattr(file, &next)
}

#[cfg(unix)]
fn copy_termios(source: &libc::termios) -> libc::termios {
  // `termios` is a plain value without a `Copy` impl. It holds no pointers.
  unsafe { std::ptr::read(source) }
}

#[cfg(unix)]
fn apply_raw(termios: &mut libc::termios, cbreak: bool) {
  termios.c_iflag &=
    !(libc::BRKINT | libc::ICRNL | libc::INPCK | libc::ISTRIP | libc::IXON);
  termios.c_oflag &= !libc::OPOST;
  termios.c_cflag &= !libc::CSIZE;
  termios.c_cflag |= libc::CS8;
  let mut local = libc::ECHO | libc::ICANON | libc::IEXTEN;
  if !cbreak {
    local |= libc::ISIG;
  }
  termios.c_lflag &= !local;
  termios.c_cc[libc::VMIN] = 1;
  termios.c_cc[libc::VTIME] = 0;
}

#[cfg(unix)]
fn tcgetattr(file: &File) -> Result<libc::termios, FsError> {
  use std::os::fd::AsRawFd;
  let mut termios = unsafe { std::mem::zeroed::<libc::termios>() };
  let rc = unsafe { libc::tcgetattr(file.as_raw_fd(), &mut termios) };
  if rc == 0 {
    Ok(termios)
  } else {
    Err(FsError::io(io::Error::last_os_error()))
  }
}

#[cfg(unix)]
fn tcsetattr(file: &File, termios: &libc::termios) -> Result<(), FsError> {
  use std::os::fd::AsRawFd;
  let rc = unsafe { libc::tcsetattr(file.as_raw_fd(), libc::TCSANOW, termios) };
  if rc == 0 {
    Ok(())
  } else {
    Err(FsError::io(io::Error::last_os_error()))
  }
}

#[cfg(windows)]
fn set_times_path(
  path: &Path,
  atime: FileTime,
  mtime: FileTime,
) -> Result<(), FsError> {
  use std::os::windows::io::AsRawHandle;
  let file = checked_attributes(path)?;
  set_handle_times(file.as_raw_handle(), atime, mtime)
}

#[cfg(windows)]
fn set_times_file(
  file: &File,
  atime: FileTime,
  mtime: FileTime,
) -> Result<(), FsError> {
  use std::os::windows::io::AsRawHandle;
  use windows_sys::Win32::Storage::FileSystem::{
    FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
    FILE_WRITE_ATTRIBUTES, ReOpenFile,
  };
  match set_handle_times(file.as_raw_handle(), atime, mtime) {
    Err(error) if error.permission_denied() => {
      // `SetFileTime` needs `FILE_WRITE_ATTRIBUTES`, which a file opened for
      // reading lacks. Open the same file again with that right, as
      // `futimens` on a read-only descriptor does on Unix.
      let handle = unsafe {
        ReOpenFile(
          file.as_raw_handle() as windows_sys::Win32::Foundation::HANDLE,
          FILE_WRITE_ATTRIBUTES,
          FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
          0,
        )
      };
      let handle = owned_handle(handle).map_err(FsError::io)?;
      set_handle_times(handle.as_raw_handle(), atime, mtime)
    }
    other => other,
  }
}

#[cfg(windows)]
fn filetime_ticks(parts: TimeParts) -> Result<u64, FsError> {
  let scaled = parts.sec.checked_mul(10_000_000).ok_or_else(out_of_range)?;
  let from_unix = scaled
    .checked_add(parts.nsec / 100)
    .ok_or_else(out_of_range)?;
  const OFFSET: u64 = 116_444_736_000_000_000;
  if from_unix >= 0 {
    OFFSET
      .checked_add(from_unix as u64)
      .ok_or_else(out_of_range)
  } else {
    OFFSET
      .checked_sub(from_unix.unsigned_abs())
      .ok_or_else(out_of_range)
  }
}

#[cfg(windows)]
fn filetime_from(
  time: FileTime,
) -> Result<windows_sys::Win32::Foundation::FILETIME, FsError> {
  use windows_sys::Win32::Foundation::FILETIME;
  let ticks = filetime_ticks(split_time(time.seconds)?)?;
  Ok(FILETIME {
    dwLowDateTime: ticks as u32,
    dwHighDateTime: (ticks >> 32) as u32,
  })
}

#[cfg(windows)]
fn open_attributes(
  path: &Path,
) -> Result<std::os::windows::io::OwnedHandle, FsError> {
  use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAG_BACKUP_SEMANTICS, FILE_READ_ATTRIBUTES,
    FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
    FILE_WRITE_ATTRIBUTES, OPEN_EXISTING,
  };
  let wide = wide_null(path);
  let handle = unsafe {
    CreateFileW(
      wide.as_ptr(),
      FILE_READ_ATTRIBUTES | FILE_WRITE_ATTRIBUTES,
      FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
      std::ptr::null(),
      OPEN_EXISTING,
      FILE_FLAG_BACKUP_SEMANTICS,
      std::ptr::null_mut(),
    )
  };
  owned_handle(handle).map_err(FsError::io)
}

#[cfg(windows)]
fn set_handle_times(
  handle: std::os::windows::io::RawHandle,
  atime: FileTime,
  mtime: FileTime,
) -> Result<(), FsError> {
  use windows_sys::Win32::Storage::FileSystem::SetFileTime;
  let atime = filetime_from(atime)?;
  let mtime = filetime_from(mtime)?;
  let ok = unsafe {
    SetFileTime(
      handle as windows_sys::Win32::Foundation::HANDLE,
      std::ptr::null(),
      &atime,
      &mtime,
    )
  };
  if ok == 0 {
    Err(FsError::io(last_windows_error()))
  } else {
    Ok(())
  }
}

#[cfg(windows)]
/// `target` is the absolute path the permission check named.
fn junction(link: &Path, target: &Path) -> Result<(), FsError> {
  if !target.is_absolute() {
    return Err(FsError::failed());
  }
  fs::create_dir(link).map_err(FsError::io)?;
  let created = write_junction(link, target);
  if created.is_err() {
    let _ = fs::remove_dir(link);
  }
  created
}

#[cfg(windows)]
fn write_junction(link: &Path, target: &Path) -> Result<(), FsError> {
  use std::os::windows::io::AsRawHandle;
  use windows_sys::Win32::Foundation::GENERIC_WRITE;
  use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
    FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
  };
  use windows_sys::Win32::System::IO::DeviceIoControl;
  use windows_sys::Win32::System::Ioctl::FSCTL_SET_REPARSE_POINT;
  let buffer = junction_buffer(target)?;
  let wide = wide_null(link);
  let handle = unsafe {
    CreateFileW(
      wide.as_ptr(),
      GENERIC_WRITE,
      FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
      std::ptr::null(),
      OPEN_EXISTING,
      FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
      std::ptr::null_mut(),
    )
  };
  let owned = owned_handle(handle).map_err(FsError::io)?;
  let mut returned = 0u32;
  let ok = unsafe {
    DeviceIoControl(
      owned.as_raw_handle() as windows_sys::Win32::Foundation::HANDLE,
      FSCTL_SET_REPARSE_POINT,
      buffer.as_ptr().cast(),
      buffer.len() as u32,
      std::ptr::null_mut(),
      0,
      &mut returned,
      std::ptr::null_mut(),
    )
  };
  if ok == 0 {
    Err(FsError::io(last_windows_error()))
  } else {
    Ok(())
  }
}

#[cfg(windows)]
fn junction_buffer(target: &Path) -> Result<Vec<u8>, FsError> {
  let text = target.to_string_lossy().replace('\0', "\u{FFFD}");
  let print = if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
    format!(r"\\{rest}")
  } else if let Some(rest) = text.strip_prefix(r"\\?\") {
    rest.to_string()
  } else {
    text.clone()
  };
  let substitute = if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
    format!(r"\??\UNC\{rest}")
  } else if let Some(rest) = text.strip_prefix(r"\\?\") {
    format!(r"\??\{rest}")
  } else if let Some(rest) = text.strip_prefix(r"\\") {
    format!(r"\??\UNC\{rest}")
  } else {
    format!(r"\??\{text}")
  };
  let substitute: Vec<u16> = substitute.encode_utf16().collect();
  let print: Vec<u16> = print.encode_utf16().collect();
  let path_bytes = (substitute.len() + 1 + print.len()) * 2;
  let data_len = 8 + path_bytes;
  if data_len > u16::MAX as usize {
    return Err(FsError::io(io::Error::new(
      io::ErrorKind::InvalidInput,
      "The junction path is too long.",
    )));
  }
  let mut bytes = vec![0u8; 8 + data_len];
  bytes[0..4].copy_from_slice(&0xA000_0003u32.to_le_bytes());
  bytes[4..6].copy_from_slice(&(data_len as u16).to_le_bytes());
  bytes[8..10].copy_from_slice(&0u16.to_le_bytes());
  bytes[10..12].copy_from_slice(&((substitute.len() * 2) as u16).to_le_bytes());
  let print_offset = ((substitute.len() + 1) * 2) as u16;
  bytes[12..14].copy_from_slice(&print_offset.to_le_bytes());
  bytes[14..16].copy_from_slice(&((print.len() * 2) as u16).to_le_bytes());
  let mut cursor = 16;
  for unit in substitute {
    bytes[cursor..cursor + 2].copy_from_slice(&unit.to_le_bytes());
    cursor += 2;
  }
  cursor += 2;
  for unit in print {
    bytes[cursor..cursor + 2].copy_from_slice(&unit.to_le_bytes());
    cursor += 2;
  }
  Ok(bytes)
}

#[cfg(windows)]
fn windows_lock(
  file: &File,
  exclusive: bool,
  nonblocking: bool,
  unlock: bool,
) -> Result<bool, FsError> {
  use std::os::windows::io::AsRawHandle;
  use windows_sys::Win32::Foundation::GetLastError;
  use windows_sys::Win32::Storage::FileSystem::{
    LOCKFILE_EXCLUSIVE_LOCK, LOCKFILE_FAIL_IMMEDIATELY, LockFileEx,
    UnlockFileEx,
  };
  use windows_sys::Win32::System::IO::OVERLAPPED;
  let mut overlapped = OVERLAPPED::default();
  let handle = file.as_raw_handle() as windows_sys::Win32::Foundation::HANDLE;
  let ok = if unlock {
    unsafe { UnlockFileEx(handle, 0, u32::MAX, u32::MAX, &mut overlapped) }
  } else {
    let mut flags = 0u32;
    if exclusive {
      flags |= LOCKFILE_EXCLUSIVE_LOCK;
    }
    if nonblocking {
      flags |= LOCKFILE_FAIL_IMMEDIATELY;
    }
    unsafe { LockFileEx(handle, flags, 0, u32::MAX, u32::MAX, &mut overlapped) }
  };
  if ok != 0 {
    return Ok(true);
  }
  let code = unsafe { GetLastError() };
  let error = io::Error::from_raw_os_error(code as i32);
  if !unlock && nonblocking && contended(&error) {
    Ok(false)
  } else {
    Err(FsError::io(error))
  }
}

#[cfg(windows)]
fn console_mode(file: &File) -> io::Result<u32> {
  use std::os::windows::io::AsRawHandle;
  use windows_sys::Win32::System::Console::GetConsoleMode;
  let mut mode = 0u32;
  let ok = unsafe {
    GetConsoleMode(
      file.as_raw_handle() as windows_sys::Win32::Foundation::HANDLE,
      &mut mode,
    )
  };
  if ok == 0 {
    Err(last_windows_error())
  } else {
    Ok(mode)
  }
}

#[cfg(windows)]
fn set_console_mode(file: &File, mode: u32) -> Result<(), FsError> {
  use std::os::windows::io::AsRawHandle;
  use windows_sys::Win32::System::Console::SetConsoleMode;
  let ok = unsafe {
    SetConsoleMode(
      file.as_raw_handle() as windows_sys::Win32::Foundation::HANDLE,
      mode,
    )
  };
  if ok == 0 {
    Err(FsError::io(last_windows_error()))
  } else {
    Ok(())
  }
}

#[cfg(windows)]
fn set_raw_windows(
  file: &File,
  state: &mut TerminalState,
  raw: bool,
  cbreak: bool,
) -> Result<(), FsError> {
  const ENABLE_PROCESSED_INPUT: u32 = 1;
  const ENABLE_LINE_INPUT: u32 = 2;
  const ENABLE_ECHO_INPUT: u32 = 4;
  const ENABLE_VIRTUAL_TERMINAL_INPUT: u32 = 512;
  if !raw {
    if let Some(original) = state.original.take() {
      return set_console_mode(file, original);
    }
    return Ok(());
  }
  let current = console_mode(file)
    .map_err(|_| FsError::not_supported("The file is not a terminal."))?;
  if state.original.is_none() {
    state.original = Some(current);
  }
  let Some(original) = state.original else {
    return Err(FsError::not_supported("The file is not a terminal."));
  };
  let mut next = original
    & !(ENABLE_LINE_INPUT | ENABLE_ECHO_INPUT | ENABLE_PROCESSED_INPUT);
  if cbreak {
    next |= ENABLE_PROCESSED_INPUT;
  }
  next |= ENABLE_VIRTUAL_TERMINAL_INPUT;
  set_console_mode(file, next)
}

#[cfg(windows)]
fn wide_null(path: &Path) -> Vec<u16> {
  use std::os::windows::ffi::OsStrExt;
  let mut wide: Vec<u16> = path.as_os_str().encode_wide().collect();
  wide.push(0);
  wide
}

#[cfg(windows)]
fn owned_handle(
  handle: windows_sys::Win32::Foundation::HANDLE,
) -> io::Result<std::os::windows::io::OwnedHandle> {
  use std::os::windows::io::{FromRawHandle, OwnedHandle};
  use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
  if handle.is_null() || handle == INVALID_HANDLE_VALUE {
    return Err(last_windows_error());
  }
  Ok(unsafe { OwnedHandle::from_raw_handle(handle as _) })
}

#[cfg(windows)]
fn last_windows_error() -> io::Error {
  use windows_sys::Win32::Foundation::GetLastError;
  io::Error::from_raw_os_error(unsafe { GetLastError() } as i32)
}

#[cfg(not(any(unix, windows)))]
fn set_times_path(
  path: &Path,
  atime: FileTime,
  mtime: FileTime,
) -> Result<(), FsError> {
  let _ = (path, atime, mtime);
  Err(FsError::not_supported(
    "utime is not supported on this operating system.",
  ))
}

#[cfg(test)]
mod tests {
  use std::fs;
  use std::time::{SystemTime, UNIX_EPOCH};

  #[cfg(unix)]
  #[test]
  fn temporary_entries_are_private() {
    use std::os::unix::fs::PermissionsExt;
    for directory in [false, true] {
      let path = super::make_temp(
        directory,
        super::TempSpec {
          dir: None,
          prefix: "yuzuko-mode-".into(),
          suffix: String::new(),
        },
      )
      .unwrap();
      assert_eq!(
        std::fs::metadata(&path).unwrap().permissions().mode() & 0o077,
        0
      );
      if directory {
        std::fs::remove_dir(path).unwrap();
      } else {
        std::fs::remove_file(path).unwrap();
      }
    }
  }

  #[test]
  fn copy_and_truncate_use_the_opened_file() {
    let root = std::env::temp_dir().join(format!(
      "yuzuko-fs-copy-{}-{}",
      std::process::id(),
      SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    let from = root.join("from.txt");
    let to = root.join("to.txt");
    fs::write(&from, b"hello").unwrap();
    fs::write(&to, b"stale-tail").unwrap();
    super::copy_path(&from, &to).unwrap();
    assert_eq!(fs::read(&to).unwrap(), b"hello");
    super::truncate_path(&from, 2).unwrap();
    assert_eq!(fs::read(&from).unwrap(), b"he");
    super::copy_path(&from, &to).unwrap();
    assert_eq!(fs::read(&to).unwrap(), b"he");
    super::copy_path(&from, &from).unwrap();
    assert_eq!(fs::read(&from).unwrap(), b"he");
    let alias = root.join("alias.txt");
    fs::hard_link(&from, &alias).unwrap();
    super::copy_path(&alias, &from).unwrap();
    assert_eq!(fs::read(&from).unwrap(), b"he");
    assert_eq!(fs::read(&alias).unwrap(), b"he");
    let _ = fs::remove_dir_all(&root);
  }

  #[test]
  fn an_opened_bundle_file_is_not_written() {
    let root = std::env::temp_dir().join(format!(
      "yuzuko-fs-bundle-{}-{}",
      std::process::id(),
      SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
    ));
    let _ = fs::remove_dir_all(&root);
    let bundle = root.join("Bundle.app");
    let inside = bundle.join("secret.txt");
    let outside = root.join("outside.txt");
    fs::create_dir_all(&bundle).unwrap();
    fs::write(&inside, b"secret").unwrap();
    fs::write(&outside, b"out").unwrap();
    let _guard = super::super::replace_bundle_root(Some(&bundle));
    let error =
      super::write_bytes(&inside, b"nope", super::WriteFlags::default())
        .unwrap_err();
    assert!(
      matches!(
        error,
        super::FsError::Os {
          class: super::super::error::Class::NotSupported,
          ..
        }
      ),
      "{error:?}"
    );
    assert_eq!(fs::read(&inside).unwrap(), b"secret");
    let opened = fs::File::open(&inside).unwrap();
    let error = super::path::reject_opened_bundle(&opened).unwrap_err();
    assert!(
      matches!(
        error,
        super::FsError::Os {
          class: super::super::error::Class::NotSupported,
          ..
        }
      ),
      "{error:?}"
    );
    super::path::reject_opened_bundle(&fs::File::open(&outside).unwrap())
      .unwrap();
    super::write_bytes(&outside, b"yes", super::WriteFlags::default()).unwrap();
    assert_eq!(fs::read(&outside).unwrap(), b"yes");
    drop(_guard);
    let _ = fs::remove_dir_all(&root);
  }

  #[test]
  fn rename_rechecks_read_on_the_opened_parent() {
    if std::env::var_os("YUZUKO_RENAME_RECHECK").is_some() {
      rename_recheck_child();
      return;
    }
    let marker = std::env::temp_dir().join(format!(
      "yuzuko-rename-recheck-{}-{}",
      std::process::id(),
      SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
    ));
    let _ = fs::remove_file(&marker);
    let status = std::process::Command::new(std::env::current_exe().unwrap())
      .env("YUZUKO_RENAME_RECHECK", &marker)
      .args([
        "--exact",
        "fs::io::tests::rename_rechecks_read_on_the_opened_parent",
        "--test-threads",
        "1",
      ])
      .status()
      .unwrap();
    assert!(status.success(), "{status}");
    assert!(marker.is_file(), "the recheck child did not run");
    let _ = fs::remove_file(&marker);
  }

  fn rename_recheck_child() {
    let marker = std::env::var_os("YUZUKO_RENAME_RECHECK").unwrap();
    let root = std::env::temp_dir().join(format!(
      "yuzuko-rename-read-{}-{}",
      std::process::id(),
      SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
    ));
    let _ = fs::remove_dir_all(&root);
    let project = root.join("project");
    let source = project.join("src");
    fs::create_dir_all(source.join("keep")).unwrap();
    fs::write(source.join("secret"), b"hidden").unwrap();
    fs::write(source.join("keep").join("note"), b"n").unwrap();
    fs::create_dir_all(project.join("dst")).unwrap();
    let project = fs::canonicalize(&project).unwrap();
    let source = project.join("src");
    let dest = project.join("dst").join("moved");
    let mut spec = crate::permissions::PermissionSpec::default();
    spec
      .allow("read", Some(vec![project.display().to_string()]))
      .unwrap();
    spec
      .allow("write", Some(vec![project.display().to_string()]))
      .unwrap();
    spec
      .deny(
        "read",
        Some(vec![source.join("secret").display().to_string()]),
      )
      .unwrap();
    crate::permissions::install_permissions(&spec, &project).unwrap();
    let error = super::rename_path(&source, &dest).unwrap_err();
    assert!(
      matches!(error, super::FsError::Host { ref name, .. } if name == "NotCapable"),
      "{error:?}"
    );
    assert_eq!(fs::read(source.join("secret")).unwrap(), b"hidden");
    assert!(!dest.exists());
    fs::write(&marker, b"ok").unwrap();
    let _ = fs::remove_dir_all(&root);
  }
}
