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
  error::install(scope);
  publish_file(scope);
  ops::define(scope);
  install_seek_mode(scope);
  error::publish(scope);
  install_dispose(scope);
}

pub(crate) fn clear() {
  error::clear();
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

fn publish_file(scope: &mut v8::PinScope) {
  file::FsFile::install(scope);
  let global = scope.get_current_context().global(scope);
  let Some(key) = v8::String::new(scope, "FsFile") else {
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
  let Some(proto) = fs_file_prototype(scope) else {
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

fn fs_file_prototype<'s>(
  scope: &mut v8::PinScope<'s, '_>,
) -> Option<v8::Local<'s, v8::Object>> {
  let yuzuko = crate::namespace(scope, "Yuzuko");
  let key = v8::String::new(scope, "FsFile")?;
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
