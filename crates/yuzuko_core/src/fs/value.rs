//! JavaScript values for the file-system API.
//!
//! Argument checks run on the isolate thread, so a `TypeError` is thrown
//! before an asynchronous operation starts. Bytes are copied out of V8
//! before a background thread sees them.

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use super::error::FsError;
use super::io::{
  self, Entry, Info, MkdirFlags, OpenFlags, RemoveFlags, TempSpec, WriteFlags,
};

const MODE_MAX: f64 = 262_143.0;

pub(super) fn parse_path(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<PathBuf, FsError> {
  if value.is_string() {
    let Some(text) = value.to_string(scope) else {
      return Err(FsError::type_("The path must be a string or a file URL."));
    };
    return Ok(PathBuf::from(text.to_rust_string_lossy(scope)));
  }
  if is_url(scope, value) {
    let object = v8::Local::<v8::Object>::try_from(value)
      .map_err(|_| FsError::type_("The URL is invalid."))?;
    let href = href_of(scope, object)?;
    let url = url::Url::parse(&href)
      .map_err(|_| FsError::type_("The URL is invalid."))?;
    if url.scheme() != "file" {
      return Err(FsError::type_("The URL scheme must be file."));
    }
    return url
      .to_file_path()
      .map_err(|()| FsError::type_("The file URL is invalid."));
  }
  Err(FsError::type_("The path must be a string or a file URL."))
}

pub(super) fn ignore_options(
  _scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<(), FsError> {
  if value.is_null_or_undefined()
    || v8::Local::<v8::Object>::try_from(value).is_ok()
  {
    Ok(())
  } else {
    Err(FsError::type_("Options must be an object."))
  }
}

pub(super) fn text_arg(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<String, FsError> {
  if !value.is_string() {
    return Err(FsError::type_("Text must be a string."));
  }
  let Some(text) = value.to_string(scope) else {
    return Err(FsError::type_("Text must be a string."));
  };
  Ok(text.to_rust_string_lossy(scope))
}

pub(super) fn length_arg(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<u64, FsError> {
  if value.is_undefined() {
    return Ok(0);
  }
  let Some(number) = finite_number(scope, value) else {
    return Err(FsError::type_(
      "The length must be an integer that is at least 0.",
    ));
  };
  io::byte_length(number)
}

pub(super) fn open_flags(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<OpenFlags, FsError> {
  if value.is_null_or_undefined() {
    return Ok(OpenFlags::read_only());
  }
  let object = object_of(value)?;
  let flags = OpenFlags {
    read: bool_field(scope, object, "read", true)?,
    write: bool_field(scope, object, "write", false)?,
    append: bool_field(scope, object, "append", false)?,
    truncate: bool_field(scope, object, "truncate", false)?,
    create: bool_field(scope, object, "create", false)?,
    create_new: bool_field(scope, object, "createNew", false)?,
    mode: mode_field(scope, object)?,
  };
  if flags.truncate && !flags.write && !flags.append {
    return Err(FsError::type_("truncate requires write access."));
  }
  if (flags.create || flags.create_new) && !flags.write && !flags.append {
    return Err(FsError::type_("create requires write or append access."));
  }
  if !flags.read && !flags.write && !flags.append {
    return Err(FsError::type_(
      "The file must be opened for reading or writing.",
    ));
  }
  if flags.append && flags.truncate {
    return Err(FsError::type_("append and truncate cannot both be set."));
  }
  Ok(flags)
}

pub(super) fn write_flags(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<WriteFlags, FsError> {
  if value.is_null_or_undefined() {
    return Ok(WriteFlags::default());
  }
  let object = object_of(value)?;
  Ok(WriteFlags {
    append: bool_field(scope, object, "append", false)?,
    create: bool_field(scope, object, "create", true)?,
    create_new: bool_field(scope, object, "createNew", false)?,
    mode: mode_field(scope, object)?,
  })
}

pub(super) fn mkdir_flags(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<MkdirFlags, FsError> {
  if value.is_null_or_undefined() {
    return Ok(MkdirFlags::default());
  }
  let object = object_of(value)?;
  Ok(MkdirFlags {
    recursive: bool_field(scope, object, "recursive", false)?,
    mode: mode_field(scope, object)?,
  })
}

pub(super) fn remove_flags(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<RemoveFlags, FsError> {
  if value.is_null_or_undefined() {
    return Ok(RemoveFlags::default());
  }
  let object = object_of(value)?;
  Ok(RemoveFlags {
    recursive: bool_field(scope, object, "recursive", false)?,
  })
}

pub(super) fn temp_spec(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<TempSpec, FsError> {
  if value.is_null_or_undefined() {
    return Ok(TempSpec {
      dir: None,
      prefix: String::new(),
      suffix: String::new(),
    });
  }
  let object = object_of(value)?;
  let dir = optional_path(scope, object, "dir")?;
  let prefix = name_piece(
    scope,
    object,
    "prefix",
    "The prefix must be a string.",
    "The prefix must not contain a path separator or NUL.",
  )?;
  let suffix = name_piece(
    scope,
    object,
    "suffix",
    "The suffix must be a string.",
    "The suffix must not contain a path separator or NUL.",
  )?;
  Ok(TempSpec {
    dir,
    prefix,
    suffix,
  })
}

pub(super) fn copy_source(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<Vec<u8>, FsError> {
  if let Ok(buffer) = v8::Local::<v8::SharedArrayBuffer>::try_from(value) {
    return shared_copy(&buffer);
  }
  if let Ok(view) = v8::Local::<v8::ArrayBufferView>::try_from(value) {
    if view
      .buffer(scope)
      .is_some_and(|buffer| buffer.was_detached())
    {
      return Err(FsError::type_("The buffer is detached."));
    }
    let length = view.byte_length();
    if length > io::MAX_BUFFER {
      return Err(FsError::type_("The buffer is too large."));
    }
    let mut bytes = vec![0u8; length];
    if length == 0 {
      return Ok(bytes);
    }
    let written = view.copy_contents(&mut bytes);
    bytes.truncate(written);
    return Ok(bytes);
  }
  if let Ok(buffer) = v8::Local::<v8::ArrayBuffer>::try_from(value) {
    return array_copy(&buffer);
  }
  Err(FsError::type_("Data must be a BufferSource."))
}

pub(super) fn require_view(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<(v8::Global<v8::ArrayBufferView>, usize), FsError> {
  let Ok(view) = v8::Local::<v8::ArrayBufferView>::try_from(value) else {
    return Err(FsError::type_("The buffer must be a Uint8Array."));
  };
  if view
    .buffer(scope)
    .is_some_and(|buffer| buffer.was_detached())
  {
    return Err(FsError::type_("The buffer is detached."));
  }
  let length = view.byte_length();
  if length > io::MAX_BUFFER {
    return Err(FsError::type_("The buffer is too large."));
  }
  Ok((v8::Global::new(scope, view), length))
}

pub(super) fn write_view(
  scope: &mut v8::PinScope,
  view: v8::Local<v8::ArrayBufferView>,
  bytes: &[u8],
) -> Result<usize, FsError> {
  if view
    .buffer(scope)
    .is_some_and(|buffer| buffer.was_detached())
  {
    return Err(FsError::type_("The buffer is detached."));
  }
  let count = bytes.len().min(view.byte_length());
  if count == 0 {
    return Ok(0);
  }
  let data = view.data();
  if data.is_null() {
    return Err(FsError::type_("The buffer is detached."));
  }
  // Safety: `count` is within the view and the source. `data()` already
  // includes the view's byte offset, and `view` keeps the store alive.
  unsafe {
    std::ptr::copy_nonoverlapping(bytes.as_ptr(), data.cast::<u8>(), count);
  }
  Ok(count)
}

pub(super) fn bytes_value<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  bytes: Vec<u8>,
) -> Result<v8::Local<'s, v8::Value>, FsError> {
  let length = bytes.len();
  if length > i32::MAX as usize {
    return Err(FsError::failed());
  }
  let store = v8::ArrayBuffer::new_backing_store_from_vec(bytes).make_shared();
  let buffer = v8::ArrayBuffer::with_backing_store(scope, &store);
  let Some(view) = v8::Uint8Array::new(scope, buffer, 0, length) else {
    return Err(FsError::failed());
  };
  Ok(view.into())
}

pub(super) fn string_value<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  text: &str,
) -> Result<v8::Local<'s, v8::Value>, FsError> {
  let Some(value) = v8::String::new(scope, text) else {
    return Err(FsError::failed());
  };
  Ok(value.into())
}

pub(super) fn number_value<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  number: f64,
) -> Result<v8::Local<'s, v8::Value>, FsError> {
  Ok(v8::Number::new(scope, number).into())
}

pub(super) fn count_value<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  count: Option<u32>,
) -> Result<v8::Local<'s, v8::Value>, FsError> {
  let value = match count {
    Some(count) => v8::Number::new(scope, f64::from(count)).into(),
    None => v8::null(scope).into(),
  };
  Ok(value)
}

pub(super) fn undefined_value<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  _unit: (),
) -> Result<v8::Local<'s, v8::Value>, FsError> {
  Ok(v8::undefined(scope).into())
}

pub(super) fn info_value<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  info: &Info,
) -> Result<v8::Local<'s, v8::Value>, FsError> {
  let object = v8::Object::new(scope);
  set_bool(scope, object, "isFile", info.is_file);
  set_bool(scope, object, "isDirectory", info.is_directory);
  set_bool(scope, object, "isSymlink", info.is_symlink);
  set_f64(scope, object, "size", info.size as f64);
  let mtime = time_value(scope, info.mtime);
  let atime = time_value(scope, info.atime);
  let birthtime = time_value(scope, info.birthtime);
  set(scope, object, "mtime", mtime);
  set(scope, object, "atime", atime);
  set(scope, object, "birthtime", birthtime);
  set_f64(scope, object, "mode", f64::from(info.unix.mode));
  set_f64(scope, object, "dev", info.unix.dev as f64);
  set_f64(scope, object, "ino", info.unix.ino as f64);
  set_f64(scope, object, "nlink", info.unix.nlink as f64);
  set_f64(scope, object, "uid", f64::from(info.unix.uid));
  set_f64(scope, object, "gid", f64::from(info.unix.gid));
  set_f64(scope, object, "blksize", info.unix.blksize as f64);
  set_f64(scope, object, "blocks", info.unix.blocks as f64);
  set_bool(scope, object, "isBlockDevice", info.unix.is_block_device);
  set_bool(scope, object, "isCharDevice", info.unix.is_char_device);
  set_bool(scope, object, "isFifo", info.unix.is_fifo);
  set_bool(scope, object, "isSocket", info.unix.is_socket);
  Ok(object.into())
}

pub(super) fn entries_value<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  entries: &[Entry],
) -> Result<v8::Local<'s, v8::Value>, FsError> {
  let array = v8::Array::new(scope, entries.len() as i32);
  for (index, entry) in entries.iter().enumerate() {
    let object = v8::Object::new(scope);
    set_string(scope, object, "name", &entry.name)?;
    set_bool(scope, object, "isFile", entry.is_file);
    set_bool(scope, object, "isDirectory", entry.is_directory);
    set_bool(scope, object, "isSymlink", entry.is_symlink);
    array.set_index(scope, index as u32, object.into());
  }
  Ok(array.into())
}

fn is_url(scope: &mut v8::PinScope, value: v8::Local<v8::Value>) -> bool {
  let global = scope.get_current_context().global(scope);
  let Some(key) = v8::String::new(scope, "URL") else {
    return false;
  };
  let Some(ctor) = global.get(scope, key.into()) else {
    return false;
  };
  let Ok(ctor) = v8::Local::<v8::Object>::try_from(ctor) else {
    return false;
  };
  value.instance_of(scope, ctor) == Some(true)
}

fn href_of(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
) -> Result<String, FsError> {
  let key = v8::String::new(scope, "href").unwrap();
  let read = {
    v8::tc_scope!(let tc, scope);
    match object.get(tc, key.into()) {
      Some(value) if tc.exception().is_none() => match value.to_string(tc) {
        Some(text) if tc.exception().is_none() => {
          Ok(text.to_rust_string_lossy(tc))
        }
        _ => Err(()),
      },
      _ => Err(()),
    }
  };
  read.map_err(|()| FsError::type_("The URL is invalid."))
}

fn object_of(
  value: v8::Local<v8::Value>,
) -> Result<v8::Local<v8::Object>, FsError> {
  v8::Local::<v8::Object>::try_from(value)
    .map_err(|_| FsError::type_("Options must be an object."))
}

fn get_field<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  object: v8::Local<v8::Object>,
  name: &str,
) -> Result<v8::Local<'s, v8::Value>, FsError> {
  let key = v8::String::new(scope, name).unwrap();
  let read = {
    v8::tc_scope!(let tc, scope);
    match object.get(tc, key.into()) {
      Some(value) if tc.exception().is_none() => Ok(v8::Global::new(tc, value)),
      _ => Err(()),
    }
  };
  match read {
    Ok(value) => Ok(v8::Local::new(scope, value)),
    Err(()) => Err(FsError::type_("The options could not be read.")),
  }
}

fn bool_field(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
  default: bool,
) -> Result<bool, FsError> {
  let value = get_field(scope, object, name)?;
  if value.is_undefined() {
    return Ok(default);
  }
  let read = {
    v8::tc_scope!(let tc, scope);
    let flag = value.boolean_value(tc);
    if tc.exception().is_some() {
      None
    } else {
      Some(flag)
    }
  };
  read.ok_or_else(|| FsError::type_("The options could not be read."))
}

fn mode_field(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
) -> Result<Option<u32>, FsError> {
  let value = get_field(scope, object, "mode")?;
  if value.is_null_or_undefined() {
    return Ok(None);
  }
  let Some(number) = finite_number(scope, value) else {
    return Err(FsError::type_(
      "mode must be an integer from 0 through 262143.",
    ));
  };
  if number.fract() != 0.0 || !(0.0..=MODE_MAX).contains(&number) {
    return Err(FsError::type_(
      "mode must be an integer from 0 through 262143.",
    ));
  }
  Ok(Some(number as u32))
}

fn optional_path(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
) -> Result<Option<PathBuf>, FsError> {
  let value = get_field(scope, object, name)?;
  if value.is_null_or_undefined() {
    Ok(None)
  } else {
    parse_path(scope, value).map(Some)
  }
}

fn name_piece(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
  type_message: &str,
  separator_message: &str,
) -> Result<String, FsError> {
  let value = get_field(scope, object, name)?;
  if value.is_undefined() {
    return Ok(String::new());
  }
  if !value.is_string() {
    return Err(FsError::type_(type_message));
  }
  let Some(text) = value.to_string(scope) else {
    return Err(FsError::type_(type_message));
  };
  let text = text.to_rust_string_lossy(scope);
  if text.contains(['/', '\\', '\0']) {
    return Err(FsError::type_(separator_message));
  }
  Ok(text)
}

fn finite_number(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Option<f64> {
  let number = {
    v8::tc_scope!(let tc, scope);
    let number = value.number_value(tc);
    if tc.exception().is_some() {
      None
    } else {
      number
    }
  }?;
  number.is_finite().then_some(number)
}

fn shared_copy(buffer: &v8::SharedArrayBuffer) -> Result<Vec<u8>, FsError> {
  let length = buffer.byte_length();
  if length > io::MAX_BUFFER {
    return Err(FsError::type_("The buffer is too large."));
  }
  let mut bytes = vec![0u8; length];
  if length == 0 {
    return Ok(bytes);
  }
  let store = buffer.get_backing_store();
  let Some(data) = store.data() else {
    return Err(FsError::type_("The buffer is detached."));
  };
  let count = length.min(store.byte_length());
  // Safety: `count` fits the destination and the store. The shared
  // reference keeps the store alive for this copy.
  unsafe {
    std::ptr::copy_nonoverlapping(
      data.as_ptr().cast::<u8>(),
      bytes.as_mut_ptr(),
      count,
    );
  }
  bytes.truncate(count);
  Ok(bytes)
}

fn array_copy(buffer: &v8::ArrayBuffer) -> Result<Vec<u8>, FsError> {
  if buffer.was_detached() {
    return Err(FsError::type_("The buffer is detached."));
  }
  let length = buffer.byte_length();
  if length > io::MAX_BUFFER {
    return Err(FsError::type_("The buffer is too large."));
  }
  let mut bytes = vec![0u8; length];
  if length == 0 {
    return Ok(bytes);
  }
  let Some(data) = buffer.data() else {
    return Err(FsError::type_("The buffer is detached."));
  };
  // Safety: `length` is the buffer's byte length and the destination
  // size. `buffer` keeps the store alive for this copy.
  unsafe {
    std::ptr::copy_nonoverlapping(
      data.as_ptr().cast::<u8>(),
      bytes.as_mut_ptr(),
      length,
    );
  }
  Ok(bytes)
}

fn time_value<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  time: Option<SystemTime>,
) -> v8::Local<'s, v8::Value> {
  let Some(time) = time else {
    return v8::null(scope).into();
  };
  let Ok(duration) = time.duration_since(UNIX_EPOCH) else {
    return v8::null(scope).into();
  };
  let millis = duration.as_secs_f64() * 1000.0;
  match v8::Date::new(scope, millis) {
    Some(date) => date.into(),
    None => v8::null(scope).into(),
  }
}

fn set(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
  value: v8::Local<v8::Value>,
) {
  let key = v8::String::new(scope, name).unwrap();
  object.set(scope, key.into(), value);
}

fn set_bool(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
  value: bool,
) {
  set(scope, object, name, v8::Boolean::new(scope, value).into());
}

fn set_f64(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
  value: f64,
) {
  set(scope, object, name, v8::Number::new(scope, value).into());
}

fn set_string(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
  text: &str,
) -> Result<(), FsError> {
  let key = v8::String::new(scope, name).ok_or_else(FsError::failed)?;
  let value = v8::String::new(scope, text).ok_or_else(FsError::failed)?;
  object.set(scope, key.into(), value.into());
  Ok(())
}
