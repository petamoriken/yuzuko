//! Optional `Yuzuko` file system.
//!
//! The `fs` feature installs these names on the main realm and on a dedicated
//! worker. A synchronous call blocks that realm's thread. An asynchronous
//! call uses [`crate::jobs`] and settles on the realm event loop.
//!
//! A packaged launch records the bundle root. JavaScript may read that tree
//! and cannot modify it. `yuzuko run` records no root.

mod error;
mod file;
mod io;
mod ops;
mod path;
mod value;
mod watch;

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use error::FsError as Error;

static BUNDLE: Mutex<Option<PathBuf>> = Mutex::new(None);

/// Records the packaged application root. Later file operations in this
/// process refuse to modify that directory. The root is not cleared when a
/// realm exits, so a worker sees the same root as the main realm.
pub fn set_process_bundle_root(path: &Path) {
  let root =
    crate::canonicalize(path).unwrap_or_else(|_| path::lexical_absolute(path));
  *BUNDLE.lock().unwrap_or_else(|error| error.into_inner()) = Some(root);
}

pub(super) fn bundle_root() -> Option<PathBuf> {
  BUNDLE
    .lock()
    .unwrap_or_else(|error| error.into_inner())
    .clone()
}

pub(crate) fn install(scope: &mut v8::PinScope) {
  publish_class(scope, "FsFile", file::FsFile::install);
  publish_class(scope, "FsWatcher", watch::FsWatcher::install);
  ops::define(scope);
  install_seek_mode(scope);
  install_dispose(scope);
  install_async_iterator(scope);
  install_streams(scope);
  install_stdio(scope);
}

pub(crate) fn clear() {
  watch::clear();
}

fn ready<T>(
  scope: &mut v8::PinScope,
  result: Result<T, Error>,
) -> Result<T, crate::op::OpError> {
  result.map_err(|error| error.fail(scope))
}

fn complete<'scope, T>(
  scope: &mut v8::PinScope<'scope, '_>,
  resolver: v8::Local<'scope, v8::PromiseResolver>,
  result: Result<T, Error>,
  map: impl FnOnce(
    &mut v8::PinScope<'scope, '_>,
    T,
  ) -> Result<v8::Local<'scope, v8::Value>, Error>,
) {
  let value = match result {
    Ok(value) => map(scope, value),
    Err(error) => Err(error),
  };
  match value {
    Ok(value) => {
      let _ = resolver.resolve(scope, value);
    }
    Err(error) => {
      let exception = error.exception(scope);
      let _ = resolver.reject(scope, exception);
    }
  }
}

macro_rules! settle {
  ($scope:expr, $work:expr, $finish:expr) => {{
    match ::v8::PromiseResolver::new($scope) {
      None => Err(crate::fs::error::FsError::failed().fail($scope)),
      Some(__resolver) => {
        let __promise = __resolver.get_promise($scope);
        let __promise_value: ::v8::Local<::v8::Value> = __promise.into();
        let __promise_global = ::v8::Global::new($scope, __promise_value);
        let __resolver_global = ::v8::Global::new($scope, __resolver);
        crate::jobs::spawn($work, move |scope, result| {
          let resolver = ::v8::Local::new(scope, &__resolver_global);
          let outcome = match result {
            Ok(value) => value,
            Err(()) => Err(crate::fs::error::FsError::failed()),
          };
          let finish = $finish;
          finish(scope, resolver, outcome);
        });
        Ok(crate::op::Js(__promise_global))
      }
    }
  }};
}
pub(super) use settle;

fn publish_class(
  scope: &mut v8::PinScope,
  name: &str,
  install: fn(&mut v8::PinScope),
) {
  install(scope);
  let global = scope.get_current_context().global(scope);
  let Some(key) = v8::String::new(scope, name) else {
    return;
  };
  let Some(constructor) = global.get(scope, key.into()) else {
    return;
  };
  let _ = global.delete(scope, key.into());
  let yuzuko = crate::namespace(scope, "Yuzuko");
  yuzuko.set(scope, key.into(), constructor);
}

fn install_seek_mode(scope: &mut v8::PinScope) {
  let object = v8::Object::new(scope);
  for (name, value) in [("Start", 0_i32), ("Current", 1), ("End", 2)] {
    let key = v8::String::new(scope, name).unwrap();
    let number = v8::Integer::new(scope, value);
    object.set(scope, key.into(), number.into());
  }
  let yuzuko = crate::namespace(scope, "Yuzuko");
  let key = v8::String::new(scope, "SeekMode").unwrap();
  yuzuko.set(scope, key.into(), object.into());
}

fn install_dispose(scope: &mut v8::PinScope) {
  let Some(proto) = class_prototype(scope, "FsFile") else {
    return;
  };
  if let Some(symbol) = global_symbol(scope, "dispose") {
    set_symbol_method(scope, proto, symbol, "dispose", file::dispose);
  }
  if let Some(symbol) = global_symbol(scope, "asyncDispose") {
    set_symbol_method(
      scope,
      proto,
      symbol,
      "asyncDispose",
      file::async_dispose,
    );
  }
}

fn install_async_iterator(scope: &mut v8::PinScope) {
  let Some(proto) = class_prototype(scope, "FsWatcher") else {
    return;
  };
  if let Some(symbol) = global_symbol(scope, "asyncIterator") {
    set_symbol_method(
      scope,
      proto,
      symbol,
      "asyncIterator",
      watch::async_iterator,
    );
  }
}

enum StdSlot {
  In,
  Out,
  Err,
}

/// Duplicates a process standard stream onto `Yuzuko`.
///
/// `close` drops the duplicate. The process descriptor stays open. A failed
/// duplicate omits that property.
fn install_stdio(scope: &mut v8::PinScope) {
  let yuzuko = crate::namespace(scope, "Yuzuko");
  for (name, slot) in [
    ("stdin", StdSlot::In),
    ("stdout", StdSlot::Out),
    ("stderr", StdSlot::Err),
  ] {
    let Some(file) = duplicate_stdio(slot) else {
      continue;
    };
    // Safety: `FsFile::install` cached this realm's template earlier in
    // `install`.
    let object =
      unsafe { crate::op::wrap_instance(scope, file::FsFile::from_std(file)) };
    let Some(key) = v8::String::new(scope, name) else {
      continue;
    };
    yuzuko.set(scope, key.into(), object.into());
  }
}

#[cfg(unix)]
fn duplicate_stdio(slot: StdSlot) -> Option<std::fs::File> {
  use std::os::fd::FromRawFd;

  let fd = match slot {
    StdSlot::In => 0,
    StdSlot::Out => 1,
    StdSlot::Err => 2,
  };
  // Safety: `fd` is a process standard descriptor. `dup` returns a new
  // descriptor, or -1. `from_raw_fd` owns that new descriptor. `FD_CLOEXEC`
  // keeps a later spawn from inheriting it.
  unsafe {
    let duped = libc::dup(fd);
    if duped < 0 {
      return None;
    }
    libc::fcntl(duped, libc::F_SETFD, libc::FD_CLOEXEC);
    Some(std::fs::File::from_raw_fd(duped))
  }
}

#[cfg(windows)]
fn duplicate_stdio(slot: StdSlot) -> Option<std::fs::File> {
  use std::os::windows::io::{FromRawHandle, RawHandle};

  use windows_sys::Win32::Foundation::{
    DUPLICATE_SAME_ACCESS, DuplicateHandle, HANDLE, INVALID_HANDLE_VALUE,
  };
  use windows_sys::Win32::System::Console::{
    GetStdHandle, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE,
  };
  use windows_sys::Win32::System::Threading::GetCurrentProcess;

  let which = match slot {
    StdSlot::In => STD_INPUT_HANDLE,
    StdSlot::Out => STD_OUTPUT_HANDLE,
    StdSlot::Err => STD_ERROR_HANDLE,
  };
  // Safety: the standard handle belongs to this process. The duplicate is
  // not inheritable. `from_raw_handle` owns it.
  unsafe {
    let handle = GetStdHandle(which);
    if handle.is_null() || handle == INVALID_HANDLE_VALUE {
      return None;
    }
    let mut duped: HANDLE = std::ptr::null_mut();
    let ok = DuplicateHandle(
      GetCurrentProcess(),
      handle,
      GetCurrentProcess(),
      &mut duped,
      0,
      0,
      DUPLICATE_SAME_ACCESS,
    );
    if ok == 0 || duped.is_null() {
      return None;
    }
    Some(std::fs::File::from_raw_handle(duped as RawHandle))
  }
}

#[cfg(not(any(unix, windows)))]
fn duplicate_stdio(_slot: StdSlot) -> Option<std::fs::File> {
  None
}

fn install_streams(scope: &mut v8::PinScope) {
  let source = v8::String::new(scope, STREAMS).expect("file streams");
  let script = v8::Script::compile(scope, source, None).expect("file streams");
  script.run(scope).expect("file streams");
}

fn class_prototype<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  name: &str,
) -> Option<v8::Local<'s, v8::Object>> {
  let yuzuko = crate::namespace(scope, "Yuzuko");
  let key = v8::String::new(scope, name)?;
  let constructor = yuzuko.get(scope, key.into())?;
  let constructor = v8::Local::<v8::Object>::try_from(constructor).ok()?;
  let proto_key = v8::String::new(scope, "prototype")?;
  let proto = constructor.get(scope, proto_key.into())?;
  v8::Local::<v8::Object>::try_from(proto).ok()
}

fn global_symbol<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  name: &str,
) -> Option<v8::Local<'s, v8::Symbol>> {
  let global = scope.get_current_context().global(scope);
  let symbol_key = v8::String::new(scope, "Symbol")?;
  let symbol_obj = global.get(scope, symbol_key.into())?;
  let symbol_obj = v8::Local::<v8::Object>::try_from(symbol_obj).ok()?;
  let key = v8::String::new(scope, name)?;
  let value = symbol_obj.get(scope, key.into())?;
  if !value.is_symbol() {
    return None;
  }
  v8::Local::<v8::Symbol>::try_from(value).ok()
}

fn set_symbol_method(
  scope: &mut v8::PinScope,
  proto: v8::Local<v8::Object>,
  symbol: v8::Local<v8::Symbol>,
  name: &str,
  callback: impl v8::MapFnTo<v8::FunctionCallback>,
) {
  let function = v8::Function::builder(callback)
    .length(0)
    .constructor_behavior(v8::ConstructorBehavior::Throw)
    .build(scope)
    .unwrap_or_else(|| panic!("create function {name}"));
  let label = v8::String::new(scope, name).unwrap();
  function.set_name(label);
  proto.set(scope, symbol.into(), function.into());
}

/// `readable` and `writable` stay in JavaScript so this crate does not depend
/// on the stream implementation. A pull reads at most 16384 bytes.
const STREAMS: &str = r#"(function () {
  "use strict";
  const proto = globalThis.Yuzuko.FsFile.prototype;
  const readableStreams = new WeakMap();
  const writableStreams = new WeakMap();

  function requireFile(value) {
    if (!(value instanceof globalThis.Yuzuko.FsFile)) {
      throw new TypeError("illegal invocation");
    }
    return value;
  }

  function bytesOf(chunk) {
    if (chunk instanceof Uint8Array) {
      return chunk;
    }
    if (ArrayBuffer.isView(chunk)) {
      return new Uint8Array(chunk.buffer, chunk.byteOffset, chunk.byteLength);
    }
    if (chunk instanceof ArrayBuffer) {
      return new Uint8Array(chunk);
    }
    if (
      typeof SharedArrayBuffer === "function" &&
      chunk instanceof SharedArrayBuffer
    ) {
      return new Uint8Array(chunk);
    }
    throw new TypeError("Data must be a BufferSource.");
  }

  Object.defineProperty(proto, "readable", {
    enumerable: true,
    configurable: true,
    get: function readable() {
      const self = requireFile(this);
      const existing = readableStreams.get(self);
      if (existing) {
        return existing;
      }
      const stream = new globalThis.ReadableStream({
        async pull(controller) {
          const buffer = new Uint8Array(16384);
          const n = await self.read(buffer);
          if (n === null) {
            controller.close();
            return;
          }
          if (n === 0) {
            return;
          }
          controller.enqueue(buffer.subarray(0, n));
        },
      });
      readableStreams.set(self, stream);
      return stream;
    },
  });

  Object.defineProperty(proto, "writable", {
    enumerable: true,
    configurable: true,
    get: function writable() {
      const self = requireFile(this);
      const existing = writableStreams.get(self);
      if (existing) {
        return existing;
      }
      const stream = new globalThis.WritableStream({
        async write(chunk) {
          const view = bytesOf(chunk);
          let offset = 0;
          while (offset < view.byteLength) {
            const slice = offset === 0 ? view : view.subarray(offset);
            const wrote = await self.write(slice);
            if (wrote === 0) {
              throw new TypeError("The write wrote no bytes.");
            }
            offset += wrote;
          }
        },
      });
      writableStreams.set(self, stream);
      return stream;
    },
  });
})()"#;
