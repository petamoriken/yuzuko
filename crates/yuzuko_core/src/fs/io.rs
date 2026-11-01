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

#[derive(Clone, Copy)]
pub(super) struct MkdirFlags {
  pub recursive: bool,
  pub mode: Option<u32>,
}

impl Default for MkdirFlags {
  fn default() -> Self {
    Self {
      recursive: false,
      mode: None,
    }
  }
}

#[derive(Clone, Copy)]
pub(super) struct RemoveFlags {
  pub recursive: bool,
}

impl Default for RemoveFlags {
  fn default() -> Self {
    Self { recursive: false }
  }
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
  let path = if flags.writes() {
    path::ensure_mutable(path, Link::Follow)?
  } else {
    path.to_path_buf()
  };
  let mut options = OpenOptions::new();
  options.read(flags.read);
  options.write(flags.write || flags.append);
  options.append(flags.append);
  if flags.truncate {
    options.truncate(true);
  }
  if flags.create_new {
    options.create_new(true);
  } else if flags.create {
    options.create(true);
  }
  apply_file_mode(&mut options, flags.mode);
  options.open(&path).map_err(FsError::io)
}

pub(super) fn read_bytes(path: &Path) -> Result<Vec<u8>, FsError> {
  match fs::metadata(path) {
    Ok(meta) if meta.is_dir() => return Err(FsError::directory()),
    Ok(_) => {}
    Err(error) => return Err(FsError::io(error)),
  }
  fs::read(path).map_err(FsError::io)
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
  let path = path::ensure_mutable(path, Link::Follow)?;
  let mut options = OpenOptions::new();
  options.write(true);
  options.append(flags.append);
  options.truncate(!flags.append && !flags.create_new);
  options.create(flags.create && !flags.create_new);
  options.create_new(flags.create_new);
  apply_file_mode(&mut options, flags.mode);
  let mut file = options.open(&path).map_err(FsError::io)?;
  file.write_all(bytes).map_err(FsError::io)
}

pub(super) fn write_text(
  path: &Path,
  text: &str,
  flags: WriteFlags,
) -> Result<(), FsError> {
  write_bytes(path, text.as_bytes(), flags)
}

pub(super) fn truncate_path(path: &Path, len: u64) -> Result<(), FsError> {
  let path = path::ensure_mutable(path, Link::Follow)?;
  let file = OpenOptions::new()
    .write(true)
    .open(&path)
    .map_err(FsError::io)?;
  file.set_len(len).map_err(FsError::io)
}

pub(super) fn mkdir_path(
  path: &Path,
  flags: MkdirFlags,
) -> Result<(), FsError> {
  let path = path::ensure_mutable(path, Link::Entry)?;
  if flags.recursive {
    mkdir_recursive(&path, flags.mode)
  } else {
    create_dir(&path, flags.mode).map_err(FsError::io)
  }
}

pub(super) fn remove_path(
  path: &Path,
  flags: RemoveFlags,
) -> Result<(), FsError> {
  let path = path::ensure_mutable(path, Link::Entry)?;
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
  let from = path::ensure_mutable(from, Link::Entry)?;
  let to = path::ensure_mutable(to, Link::Entry)?;
  fs::rename(from, to).map_err(FsError::io)
}

pub(super) fn copy_path(from: &Path, to: &Path) -> Result<(), FsError> {
  let to = path::ensure_mutable(to, Link::Follow)?;
  fs::copy(from, &to).map(|_| ()).map_err(FsError::io)
}

pub(super) fn real_path(path: &Path) -> Result<String, FsError> {
  let canonical = crate::canonicalize(path).map_err(FsError::io)?;
  Ok(canonical.to_string_lossy().into_owned())
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
  if out.len() > i32::MAX as usize {
    return Err(FsError::failed());
  }
  Ok(out)
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
      fs::create_dir(&candidate)
    } else {
      OpenOptions::new()
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
  file.write(bytes).map_err(FsError::io)
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
