//! The path the operating system reports for an open file.
//!
//! A permission check names a path before the file is opened. A call that
//! must not read another file, as when a symlink is swapped between the
//! check and the open, checks this path of the file it opened.

#![allow(dead_code)]

#[cfg(target_os = "linux")]
use std::fs;
use std::fs::File;
use std::io;
use std::path::PathBuf;

#[cfg(target_os = "macos")]
pub(crate) fn fd_path(file: &File) -> io::Result<PathBuf> {
  use std::os::fd::AsRawFd;
  let mut buf = [0u8; 1024];
  let rc = unsafe {
    libc::fcntl(
      file.as_raw_fd(),
      libc::F_GETPATH,
      buf.as_mut_ptr().cast::<libc::c_char>(),
    )
  };
  if rc == -1 {
    return Err(io::Error::last_os_error());
  }
  let end = buf.iter().position(|byte| *byte == 0).unwrap_or(buf.len());
  let text = String::from_utf8_lossy(&buf[..end]).replace('\0', "\u{FFFD}");
  Ok(PathBuf::from(text))
}

#[cfg(target_os = "linux")]
pub(crate) fn fd_path(file: &File) -> io::Result<PathBuf> {
  use std::os::fd::AsRawFd;
  fs::read_link(format!("/proc/self/fd/{}", file.as_raw_fd()))
}

#[cfg(all(unix, not(any(target_os = "macos", target_os = "linux"))))]
pub(crate) fn fd_path(_file: &File) -> io::Result<PathBuf> {
  Err(io::Error::new(
    io::ErrorKind::Unsupported,
    "The file path is unavailable.",
  ))
}

#[cfg(windows)]
pub(crate) fn fd_path(file: &File) -> io::Result<PathBuf> {
  use std::os::windows::io::AsRawHandle;
  use windows_sys::Win32::Storage::FileSystem::{
    GetFinalPathNameByHandleW, VOLUME_NAME_DOS,
  };
  let mut buf = vec![0u16; 32768];
  let wrote = unsafe {
    GetFinalPathNameByHandleW(
      file.as_raw_handle() as windows_sys::Win32::Foundation::HANDLE,
      buf.as_mut_ptr(),
      buf.len() as u32,
      VOLUME_NAME_DOS,
    )
  };
  if wrote == 0 || wrote as usize >= buf.len() {
    return Err(io::Error::last_os_error());
  }
  let text = String::from_utf16_lossy(&buf[..wrote as usize]);
  let text = if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
    format!(r"\\{rest}")
  } else if let Some(rest) = text.strip_prefix(r"\\?\") {
    rest.to_string()
  } else {
    text
  };
  Ok(PathBuf::from(text))
}
