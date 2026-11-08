//! `Yuzuko` file-system functions.
//!
//! Synchronous functions block the calling thread. Asynchronous functions
//! copy their arguments, run the operating-system call on a background
//! thread, and settle the promise on this realm's event loop.

use super::file::FsFile;
use super::io::{self, OpenFlags};
use super::value;
use super::watch::FsWatcher;
use super::{complete, ready, settle};
use crate::op::{Bytes, Js, New, OpError, OpException, ToV8};

pub(super) fn define(scope: &mut v8::PinScope) {
  let yuzuko = crate::namespace(scope, "Yuzuko");
  define_fn(scope, yuzuko, "open", 1, open);
  define_fn(scope, yuzuko, "openSync", 1, open_sync);
  define_fn(scope, yuzuko, "create", 1, create);
  define_fn(scope, yuzuko, "createSync", 1, create_sync);
  define_fn(scope, yuzuko, "readFile", 1, read_file);
  define_fn(scope, yuzuko, "readFileSync", 1, read_file_sync);
  define_fn(scope, yuzuko, "readTextFile", 1, read_text_file);
  define_fn(scope, yuzuko, "readTextFileSync", 1, read_text_file_sync);
  define_fn(scope, yuzuko, "writeFile", 2, write_file);
  define_fn(scope, yuzuko, "writeFileSync", 2, write_file_sync);
  define_fn(scope, yuzuko, "writeTextFile", 2, write_text_file);
  define_fn(scope, yuzuko, "writeTextFileSync", 2, write_text_file_sync);
  define_fn(scope, yuzuko, "mkdir", 1, mkdir);
  define_fn(scope, yuzuko, "mkdirSync", 1, mkdir_sync);
  define_fn(scope, yuzuko, "remove", 1, remove);
  define_fn(scope, yuzuko, "removeSync", 1, remove_sync);
  define_fn(scope, yuzuko, "rename", 2, rename);
  define_fn(scope, yuzuko, "renameSync", 2, rename_sync);
  define_fn(scope, yuzuko, "stat", 1, stat);
  define_fn(scope, yuzuko, "statSync", 1, stat_sync);
  define_fn(scope, yuzuko, "lstat", 1, lstat);
  define_fn(scope, yuzuko, "lstatSync", 1, lstat_sync);
  define_fn(scope, yuzuko, "readDir", 1, read_dir);
  define_fn(scope, yuzuko, "readDirSync", 1, read_dir_sync);
  define_fn(scope, yuzuko, "copyFile", 2, copy_file);
  define_fn(scope, yuzuko, "copyFileSync", 2, copy_file_sync);
  define_fn(scope, yuzuko, "realPath", 1, real_path);
  define_fn(scope, yuzuko, "realPathSync", 1, real_path_sync);
  define_fn(scope, yuzuko, "truncate", 1, truncate);
  define_fn(scope, yuzuko, "truncateSync", 1, truncate_sync);
  define_fn(scope, yuzuko, "makeTempDir", 0, make_temp_dir);
  define_fn(scope, yuzuko, "makeTempDirSync", 0, make_temp_dir_sync);
  define_fn(scope, yuzuko, "makeTempFile", 0, make_temp_file);
  define_fn(scope, yuzuko, "makeTempFileSync", 0, make_temp_file_sync);
  define_fn(scope, yuzuko, "chmod", 2, chmod);
  define_fn(scope, yuzuko, "chmodSync", 2, chmod_sync);
  define_fn(scope, yuzuko, "chown", 3, chown);
  define_fn(scope, yuzuko, "chownSync", 3, chown_sync);
  define_fn(scope, yuzuko, "umask", 0, umask);
  define_fn(scope, yuzuko, "link", 2, link);
  define_fn(scope, yuzuko, "linkSync", 2, link_sync);
  define_fn(scope, yuzuko, "symlink", 2, symlink);
  define_fn(scope, yuzuko, "symlinkSync", 2, symlink_sync);
  define_fn(scope, yuzuko, "readLink", 1, read_link);
  define_fn(scope, yuzuko, "readLinkSync", 1, read_link_sync);
  define_fn(scope, yuzuko, "utime", 3, utime);
  define_fn(scope, yuzuko, "utimeSync", 3, utime_sync);
  define_fn(scope, yuzuko, "watchFs", 1, watch_fs);
}

fn define_fn(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
  length: i32,
  callback: impl v8::MapFnTo<v8::FunctionCallback>,
) {
  let function = v8::Function::builder(callback)
    .length(length)
    .constructor_behavior(v8::ConstructorBehavior::Throw)
    .build(scope)
    .unwrap_or_else(|| panic!("create function {name}"));
  let key = v8::String::new(scope, name).unwrap();
  function.set_name(key);
  object.set(scope, key.into(), function.into());
}

fn present<T: ToV8>(
  scope: &mut v8::PinScope,
  ret: &mut v8::ReturnValue,
  result: Result<T, OpError>,
) {
  match result {
    Ok(value) => value.to_v8(scope, ret),
    Err(error) => OpException::throw(error, scope),
  }
}

macro_rules! callback {
  ($name:ident, $op:ident) => {
    fn $name(
      scope: &mut v8::PinScope,
      args: v8::FunctionCallbackArguments,
      mut ret: v8::ReturnValue,
    ) {
      let result = $op(scope, &args);
      present(scope, &mut ret, result);
    }
  };
}

fn path_only(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
) -> Result<std::path::PathBuf, OpError> {
  let path = value::parse_path(scope, args.get(0));
  let path = ready(scope, path)?;
  let options = value::ignore_options(scope, args.get(1));
  ready(scope, options)?;
  // The job opens this absolute path. A later chdir cannot retarget it.
  let path = crate::permissions::anchor_path(&path)?;
  crate::permissions::check_read(&path)?;
  Ok(path)
}

fn one_path(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
) -> Result<std::path::PathBuf, OpError> {
  let path = value::parse_path(scope, args.get(0));
  let path = ready(scope, path)?;
  crate::permissions::anchor_path(&path)
}

fn two_paths(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
) -> Result<(std::path::PathBuf, std::path::PathBuf), OpError> {
  let from = value::parse_path(scope, args.get(0));
  let from = ready(scope, from)?;
  let to = value::parse_path(scope, args.get(1));
  let to = ready(scope, to)?;
  Ok((from, to))
}

fn two_anchored(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
) -> Result<(std::path::PathBuf, std::path::PathBuf), OpError> {
  let (from, to) = two_paths(scope, args)?;
  Ok((
    crate::permissions::anchor_path(&from)?,
    crate::permissions::anchor_path(&to)?,
  ))
}

fn ready_open_flags(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<OpenFlags, OpError> {
  let flags = value::open_flags(scope, value);
  ready(scope, flags)
}

fn ready_copy(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<Vec<u8>, OpError> {
  let bytes = value::copy_source(scope, value);
  ready(scope, bytes)
}

fn ready_write_flags(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<io::WriteFlags, OpError> {
  let flags = value::write_flags(scope, value);
  ready(scope, flags)
}

fn ready_text(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<String, OpError> {
  let text = value::text_arg(scope, value);
  ready(scope, text)
}

fn ready_mkdir_flags(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<io::MkdirFlags, OpError> {
  let flags = value::mkdir_flags(scope, value);
  ready(scope, flags)
}

fn ready_remove_flags(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<io::RemoveFlags, OpError> {
  let flags = value::remove_flags(scope, value);
  ready(scope, flags)
}

fn ready_length(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<u64, OpError> {
  let len = value::length_arg(scope, value);
  ready(scope, len)
}

fn ready_temp(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<io::TempSpec, OpError> {
  let spec = value::temp_spec(scope, value);
  ready(scope, spec)
}

fn permit_open(
  path: &std::path::Path,
  flags: OpenFlags,
) -> Result<(), OpError> {
  if flags.read {
    crate::permissions::check_read(path)?;
  }
  if flags.writes() {
    crate::permissions::check_write(path)?;
  }
  Ok(())
}

fn permit_temp(spec: &mut io::TempSpec) -> Result<(), OpError> {
  match &spec.dir {
    Some(dir) => {
      let dir = crate::permissions::anchor_path(dir)?;
      crate::permissions::check_write(&dir)?;
      spec.dir = Some(dir);
      Ok(())
    }
    None => crate::permissions::check_write(&std::env::temp_dir()),
  }
}

callback!(open, open_op);
fn open_op(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
) -> Result<Js, OpError> {
  let path = one_path(scope, args)?;
  let flags = ready_open_flags(scope, args.get(1))?;
  permit_open(&path, flags)?;
  settle!(
    scope,
    move || io::open_file(&path, flags),
    |scope, resolver, result| {
      complete(scope, resolver, result, |scope, file| {
        let object =
          unsafe { crate::op::wrap_instance(scope, FsFile::from_std(file)) };
        Ok(object.into())
      })
    }
  )
}

callback!(open_sync, open_sync_op);
fn open_sync_op(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
) -> Result<New<FsFile>, OpError> {
  let path = one_path(scope, args)?;
  let flags = ready_open_flags(scope, args.get(1))?;
  permit_open(&path, flags)?;
  let file = ready(scope, io::open_file(&path, flags))?;
  Ok(New(FsFile::from_std(file)))
}

callback!(create, create_op);
fn create_op(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
) -> Result<Js, OpError> {
  let path = one_path(scope, args)?;
  let flags = OpenFlags::create_file();
  crate::permissions::check_write(&path)?;
  settle!(
    scope,
    move || io::open_file(&path, flags),
    |scope, resolver, result| {
      complete(scope, resolver, result, |scope, file| {
        let object =
          unsafe { crate::op::wrap_instance(scope, FsFile::from_std(file)) };
        Ok(object.into())
      })
    }
  )
}

callback!(create_sync, create_sync_op);
fn create_sync_op(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
) -> Result<New<FsFile>, OpError> {
  let path = one_path(scope, args)?;
  crate::permissions::check_write(&path)?;
  let file = ready(scope, io::open_file(&path, OpenFlags::create_file()))?;
  Ok(New(FsFile::from_std(file)))
}

callback!(read_file, read_file_op);
fn read_file_op(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
) -> Result<Js, OpError> {
  let path = path_only(scope, args)?;
  settle!(
    scope,
    move || io::read_bytes(&path),
    |scope, resolver, result| {
      complete(scope, resolver, result, value::bytes_value)
    }
  )
}

callback!(read_file_sync, read_file_sync_op);
fn read_file_sync_op(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
) -> Result<Bytes, OpError> {
  let path = path_only(scope, args)?;
  ready(scope, io::read_bytes(&path).map(Bytes))
}

callback!(read_text_file, read_text_file_op);
fn read_text_file_op(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
) -> Result<Js, OpError> {
  let path = path_only(scope, args)?;
  settle!(
    scope,
    move || io::read_text(&path),
    |scope, resolver, result| {
      complete(scope, resolver, result, |scope, text: String| {
        value::string_value(scope, &text)
      })
    }
  )
}

callback!(read_text_file_sync, read_text_file_sync_op);
fn read_text_file_sync_op(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
) -> Result<String, OpError> {
  let path = path_only(scope, args)?;
  ready(scope, io::read_text(&path))
}

callback!(write_file, write_file_op);
fn write_file_op(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
) -> Result<Js, OpError> {
  let path = one_path(scope, args)?;
  let bytes = ready_copy(scope, args.get(1))?;
  let flags = ready_write_flags(scope, args.get(2))?;
  crate::permissions::check_write(&path)?;
  settle!(
    scope,
    move || io::write_bytes(&path, &bytes, flags),
    |scope, resolver, result| {
      complete(scope, resolver, result, value::undefined_value)
    }
  )
}

callback!(write_file_sync, write_file_sync_op);
fn write_file_sync_op(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
) -> Result<(), OpError> {
  let path = one_path(scope, args)?;
  let bytes = ready_copy(scope, args.get(1))?;
  let flags = ready_write_flags(scope, args.get(2))?;
  crate::permissions::check_write(&path)?;
  ready(scope, io::write_bytes(&path, &bytes, flags))
}

callback!(write_text_file, write_text_file_op);
fn write_text_file_op(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
) -> Result<Js, OpError> {
  let path = one_path(scope, args)?;
  let text = ready_text(scope, args.get(1))?;
  let flags = ready_write_flags(scope, args.get(2))?;
  crate::permissions::check_write(&path)?;
  settle!(
    scope,
    move || io::write_text(&path, &text, flags),
    |scope, resolver, result| {
      complete(scope, resolver, result, value::undefined_value)
    }
  )
}

callback!(write_text_file_sync, write_text_file_sync_op);
fn write_text_file_sync_op(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
) -> Result<(), OpError> {
  let path = one_path(scope, args)?;
  let text = ready_text(scope, args.get(1))?;
  let flags = ready_write_flags(scope, args.get(2))?;
  crate::permissions::check_write(&path)?;
  ready(scope, io::write_text(&path, &text, flags))
}

callback!(mkdir, mkdir_op);
fn mkdir_op(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
) -> Result<Js, OpError> {
  let path = one_path(scope, args)?;
  let flags = ready_mkdir_flags(scope, args.get(1))?;
  crate::permissions::check_write(&path)?;
  settle!(
    scope,
    move || io::mkdir_path(&path, flags),
    |scope, resolver, result| {
      complete(scope, resolver, result, value::undefined_value)
    }
  )
}

callback!(mkdir_sync, mkdir_sync_op);
fn mkdir_sync_op(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
) -> Result<(), OpError> {
  let path = one_path(scope, args)?;
  let flags = ready_mkdir_flags(scope, args.get(1))?;
  crate::permissions::check_write(&path)?;
  ready(scope, io::mkdir_path(&path, flags))
}

callback!(remove, remove_op);
fn remove_op(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
) -> Result<Js, OpError> {
  let path = one_path(scope, args)?;
  let flags = ready_remove_flags(scope, args.get(1))?;
  crate::permissions::check_write(&path)?;
  settle!(
    scope,
    move || io::remove_path(&path, flags),
    |scope, resolver, result| {
      complete(scope, resolver, result, value::undefined_value)
    }
  )
}

callback!(remove_sync, remove_sync_op);
fn remove_sync_op(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
) -> Result<(), OpError> {
  let path = one_path(scope, args)?;
  let flags = ready_remove_flags(scope, args.get(1))?;
  crate::permissions::check_write(&path)?;
  ready(scope, io::remove_path(&path, flags))
}

callback!(rename, rename_op);
fn rename_op(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
) -> Result<Js, OpError> {
  let (from, to) = two_anchored(scope, args)?;
  crate::permissions::check_read(&from)?;
  crate::permissions::check_write(&from)?;
  crate::permissions::check_read(&to)?;
  crate::permissions::check_write(&to)?;
  settle!(
    scope,
    move || io::rename_path(&from, &to),
    |scope, resolver, result| {
      complete(scope, resolver, result, value::undefined_value)
    }
  )
}

callback!(rename_sync, rename_sync_op);
fn rename_sync_op(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
) -> Result<(), OpError> {
  let (from, to) = two_anchored(scope, args)?;
  crate::permissions::check_read(&from)?;
  crate::permissions::check_write(&from)?;
  crate::permissions::check_read(&to)?;
  crate::permissions::check_write(&to)?;
  ready(scope, io::rename_path(&from, &to))
}

callback!(stat, stat_op);
fn stat_op(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
) -> Result<Js, OpError> {
  stat_kind(scope, args, true)
}

callback!(lstat, lstat_op);
fn lstat_op(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
) -> Result<Js, OpError> {
  stat_kind(scope, args, false)
}

fn stat_kind(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
  follow: bool,
) -> Result<Js, OpError> {
  let path = path_only(scope, args)?;
  settle!(
    scope,
    move || io::stat_path(&path, follow),
    |scope, resolver, result| {
      complete(scope, resolver, result, |scope, info| {
        value::info_value(scope, &info)
      })
    }
  )
}

callback!(stat_sync, stat_sync_op);
fn stat_sync_op(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
) -> Result<Js, OpError> {
  stat_sync_kind(scope, args, true)
}

callback!(lstat_sync, lstat_sync_op);
fn lstat_sync_op(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
) -> Result<Js, OpError> {
  stat_sync_kind(scope, args, false)
}

fn stat_sync_kind(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
  follow: bool,
) -> Result<Js, OpError> {
  let path = path_only(scope, args)?;
  let info = io::stat_path(&path, follow);
  let info = ready(scope, info)?;
  let value = value::info_value(scope, &info);
  let value = ready(scope, value)?;
  Ok(Js(v8::Global::new(scope, value)))
}

callback!(read_dir, read_dir_op);
fn read_dir_op(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
) -> Result<Js, OpError> {
  let path = path_only(scope, args)?;
  settle!(
    scope,
    move || io::read_directory(&path),
    |scope, resolver, result| {
      complete(scope, resolver, result, |scope, entries: Vec<io::Entry>| {
        value::entries_value(scope, &entries)
      })
    }
  )
}

callback!(read_dir_sync, read_dir_sync_op);
fn read_dir_sync_op(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
) -> Result<Js, OpError> {
  let path = path_only(scope, args)?;
  let entries = io::read_directory(&path);
  let entries = ready(scope, entries)?;
  let value = value::entries_value(scope, &entries);
  let value = ready(scope, value)?;
  Ok(Js(v8::Global::new(scope, value)))
}

callback!(copy_file, copy_file_op);
fn copy_file_op(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
) -> Result<Js, OpError> {
  let (from, to) = two_anchored(scope, args)?;
  crate::permissions::check_read(&from)?;
  crate::permissions::check_write(&to)?;
  settle!(
    scope,
    move || io::copy_path(&from, &to),
    |scope, resolver, result| {
      complete(scope, resolver, result, value::undefined_value)
    }
  )
}

callback!(copy_file_sync, copy_file_sync_op);
fn copy_file_sync_op(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
) -> Result<(), OpError> {
  let (from, to) = two_anchored(scope, args)?;
  crate::permissions::check_read(&from)?;
  crate::permissions::check_write(&to)?;
  ready(scope, io::copy_path(&from, &to))
}

callback!(real_path, real_path_op);
fn real_path_op(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
) -> Result<Js, OpError> {
  let path = path_only(scope, args)?;
  settle!(
    scope,
    move || io::real_path(&path),
    |scope, resolver, result| {
      complete(scope, resolver, result, |scope, path: String| {
        value::string_value(scope, &path)
      })
    }
  )
}

callback!(real_path_sync, real_path_sync_op);
fn real_path_sync_op(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
) -> Result<String, OpError> {
  let path = path_only(scope, args)?;
  ready(scope, io::real_path(&path))
}

callback!(truncate, truncate_op);
fn truncate_op(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
) -> Result<Js, OpError> {
  let path = one_path(scope, args)?;
  let len = ready_length(scope, args.get(1))?;
  crate::permissions::check_write(&path)?;
  settle!(
    scope,
    move || io::truncate_path(&path, len),
    |scope, resolver, result| {
      complete(scope, resolver, result, value::undefined_value)
    }
  )
}

callback!(truncate_sync, truncate_sync_op);
fn truncate_sync_op(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
) -> Result<(), OpError> {
  let path = one_path(scope, args)?;
  let len = ready_length(scope, args.get(1))?;
  crate::permissions::check_write(&path)?;
  ready(scope, io::truncate_path(&path, len))
}

callback!(make_temp_dir, make_temp_dir_op);
fn make_temp_dir_op(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
) -> Result<Js, OpError> {
  temp_kind(scope, args, true)
}

callback!(make_temp_file, make_temp_file_op);
fn make_temp_file_op(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
) -> Result<Js, OpError> {
  temp_kind(scope, args, false)
}

fn temp_kind(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
  directory: bool,
) -> Result<Js, OpError> {
  let mut spec = ready_temp(scope, args.get(0))?;
  permit_temp(&mut spec)?;
  settle!(
    scope,
    move || io::make_temp(directory, spec),
    |scope, resolver, result| {
      complete(scope, resolver, result, |scope, path: String| {
        value::string_value(scope, &path)
      })
    }
  )
}

callback!(make_temp_dir_sync, make_temp_dir_sync_op);
fn make_temp_dir_sync_op(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
) -> Result<String, OpError> {
  let mut spec = ready_temp(scope, args.get(0))?;
  permit_temp(&mut spec)?;
  ready(scope, io::make_temp(true, spec))
}

callback!(make_temp_file_sync, make_temp_file_sync_op);
fn make_temp_file_sync_op(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
) -> Result<String, OpError> {
  let mut spec = ready_temp(scope, args.get(0))?;
  permit_temp(&mut spec)?;
  ready(scope, io::make_temp(false, spec))
}

fn ready_mode(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<u32, OpError> {
  let mode = value::mode_arg(scope, value);
  ready(scope, mode)
}

fn chown_args(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
) -> Result<(std::path::PathBuf, Option<u32>, Option<u32>), OpError> {
  let path = one_path(scope, args)?;
  let uid = value::id_arg(
    scope,
    args.get(1),
    "The user id must be an integer or null.",
  );
  let uid = ready(scope, uid)?;
  let gid = value::id_arg(
    scope,
    args.get(2),
    "The group id must be an integer or null.",
  );
  let gid = ready(scope, gid)?;
  crate::permissions::check_write(&path)?;
  Ok((path, uid, gid))
}

fn symlink_args(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
) -> Result<(std::path::PathBuf, std::path::PathBuf, io::LinkKind), OpError> {
  let (old, new) = two_paths(scope, args)?;
  let kind = value::link_kind(scope, args.get(2));
  let kind = ready(scope, kind)?;
  // The stored target stays the caller's string. The new path is absolute
  // so a later chdir cannot create the link somewhere else.
  let new = crate::permissions::anchor_path(&new)?;
  // A relative symbolic link is resolved from the new link's directory.
  // A junction stores a target joined to the current directory.
  let junction = matches!(kind, io::LinkKind::Junction);
  crate::permissions::check_symlink_target(&old, &new, junction)?;
  crate::permissions::check_write(&new)?;
  Ok((old, new, kind))
}

fn utime_args(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
) -> Result<(std::path::PathBuf, io::FileTime, io::FileTime), OpError> {
  let path = one_path(scope, args)?;
  let atime = value::time_arg(scope, args.get(1));
  let atime = ready(scope, atime)?;
  let mtime = value::time_arg(scope, args.get(2));
  let mtime = ready(scope, mtime)?;
  ready(scope, io::check_time(atime))?;
  ready(scope, io::check_time(mtime))?;
  crate::permissions::check_write(&path)?;
  Ok((path, atime, mtime))
}

callback!(chmod, chmod_op);
fn chmod_op(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
) -> Result<Js, OpError> {
  let path = one_path(scope, args)?;
  let mode = ready_mode(scope, args.get(1))?;
  crate::permissions::check_write(&path)?;
  settle!(
    scope,
    move || io::chmod(&path, mode),
    |scope, resolver, result| {
      complete(scope, resolver, result, value::undefined_value)
    }
  )
}

callback!(chmod_sync, chmod_sync_op);
fn chmod_sync_op(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
) -> Result<(), OpError> {
  let path = one_path(scope, args)?;
  let mode = ready_mode(scope, args.get(1))?;
  crate::permissions::check_write(&path)?;
  ready(scope, io::chmod(&path, mode))
}

callback!(chown, chown_op);
fn chown_op(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
) -> Result<Js, OpError> {
  let (path, uid, gid) = chown_args(scope, args)?;
  settle!(
    scope,
    move || io::chown(&path, uid, gid),
    |scope, resolver, result| {
      complete(scope, resolver, result, value::undefined_value)
    }
  )
}

callback!(chown_sync, chown_sync_op);
fn chown_sync_op(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
) -> Result<(), OpError> {
  let (path, uid, gid) = chown_args(scope, args)?;
  ready(scope, io::chown(&path, uid, gid))
}

callback!(umask, umask_op);
fn umask_op(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
) -> Result<u32, OpError> {
  let value = args.get(0);
  let mask = if value.is_undefined() {
    None
  } else {
    Some(ready_mode(scope, value)?)
  };
  crate::permissions::check_write_all()?;
  ready(scope, io::umask(mask))
}

callback!(link, link_op);
fn link_op(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
) -> Result<Js, OpError> {
  let (old, new) = two_anchored(scope, args)?;
  crate::permissions::check_read(&old)?;
  // The new name is the same inode. Writing it changes the old file.
  crate::permissions::check_write(&old)?;
  crate::permissions::check_write(&new)?;
  settle!(
    scope,
    move || io::link(&old, &new),
    |scope, resolver, result| {
      complete(scope, resolver, result, value::undefined_value)
    }
  )
}

callback!(link_sync, link_sync_op);
fn link_sync_op(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
) -> Result<(), OpError> {
  let (old, new) = two_anchored(scope, args)?;
  crate::permissions::check_read(&old)?;
  // The new name is the same inode. Writing it changes the old file.
  crate::permissions::check_write(&old)?;
  crate::permissions::check_write(&new)?;
  ready(scope, io::link(&old, &new))
}

callback!(symlink, symlink_op);
fn symlink_op(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
) -> Result<Js, OpError> {
  let (old, new, kind) = symlink_args(scope, args)?;
  settle!(
    scope,
    move || io::symlink(&old, &new, kind),
    |scope, resolver, result| {
      complete(scope, resolver, result, value::undefined_value)
    }
  )
}

callback!(symlink_sync, symlink_sync_op);
fn symlink_sync_op(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
) -> Result<(), OpError> {
  let (old, new, kind) = symlink_args(scope, args)?;
  ready(scope, io::symlink(&old, &new, kind))
}

callback!(read_link, read_link_op);
fn read_link_op(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
) -> Result<Js, OpError> {
  let path = one_path(scope, args)?;
  crate::permissions::check_read(&path)?;
  settle!(
    scope,
    move || io::read_link(&path),
    |scope, resolver, result| {
      complete(scope, resolver, result, |scope, text: String| {
        value::string_value(scope, &text)
      })
    }
  )
}

callback!(read_link_sync, read_link_sync_op);
fn read_link_sync_op(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
) -> Result<String, OpError> {
  let path = one_path(scope, args)?;
  crate::permissions::check_read(&path)?;
  ready(scope, io::read_link(&path))
}

callback!(utime, utime_op);
fn utime_op(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
) -> Result<Js, OpError> {
  let (path, atime, mtime) = utime_args(scope, args)?;
  settle!(
    scope,
    move || io::utime(&path, atime, mtime),
    |scope, resolver, result| {
      complete(scope, resolver, result, value::undefined_value)
    }
  )
}

callback!(utime_sync, utime_sync_op);
fn utime_sync_op(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
) -> Result<(), OpError> {
  let (path, atime, mtime) = utime_args(scope, args)?;
  ready(scope, io::utime(&path, atime, mtime))
}

callback!(watch_fs, watch_fs_op);
fn watch_fs_op(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
) -> Result<New<FsWatcher>, OpError> {
  let paths = value::watch_paths(scope, args.get(0));
  let paths = ready(scope, paths)?;
  let mut paths = paths;
  for path in &mut paths {
    *path = crate::permissions::anchor_path(path)?;
    crate::permissions::check_read(path)?;
  }
  let recursive = value::watch_recursive(scope, args.get(1));
  let recursive = ready(scope, recursive)?;
  let watcher = ready(scope, super::watch::start(paths, recursive))?;
  Ok(New(watcher))
}
