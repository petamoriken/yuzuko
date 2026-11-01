//! `Yuzuko.FsFile`. The descriptor is shared with background operations.
//!
//! Closing waits for an in-flight operation on this file, then drops the
//! descriptor. Dropping the last handle closes it when the caller did not.

use std::fs::File;
use std::sync::{Arc, Mutex};

use yuzuko_macros::op;

use super::error::{self, FsError};
use super::io;
use super::settle;
use super::value;
use crate::op::{Js, OpError};

pub struct FsFile {
  inner: Arc<Mutex<Option<File>>>,
}

unsafe impl v8::cppgc::GarbageCollected for FsFile {
  fn trace(&self, _visitor: &mut v8::cppgc::Visitor) {}

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"FsFile"
  }
}

impl FsFile {
  pub(super) fn from_std(file: File) -> Self {
    Self {
      inner: Arc::new(Mutex::new(Some(file))),
    }
  }

  pub(super) fn close_now(&self) -> Result<(), FsError> {
    let mut guard =
      self.inner.lock().unwrap_or_else(|error| error.into_inner());
    if guard.take().is_none() {
      Err(FsError::closed())
    } else {
      Ok(())
    }
  }

  fn with_file<T>(
    &self,
    body: impl FnOnce(&mut File) -> Result<T, FsError>,
  ) -> Result<T, FsError> {
    lock_map(&self.inner, body)
  }
}

fn lock_map<T>(
  inner: &Mutex<Option<File>>,
  body: impl FnOnce(&mut File) -> Result<T, FsError>,
) -> Result<T, FsError> {
  let mut guard = inner.lock().unwrap_or_else(|error| error.into_inner());
  let Some(file) = guard.as_mut() else {
    return Err(FsError::closed());
  };
  body(file)
}

struct ReadPayload {
  bytes: Vec<u8>,
  requested: usize,
}

fn read_shared(
  inner: &Mutex<Option<File>>,
  requested: usize,
) -> Result<ReadPayload, FsError> {
  lock_map(inner, |file| {
    let bytes = io::read_at(file, requested)?;
    Ok(ReadPayload { bytes, requested })
  })
}

#[op]
impl FsFile {
  #[promise]
  fn read(
    &self,
    scope: &mut v8::PinScope,
    buffer: v8::Local<v8::Value>,
  ) -> Result<Js, OpError> {
    let view = value::require_view(scope, buffer);
    let (view, requested) = super::ready(scope, view)?;
    let inner = Arc::clone(&self.inner);
    settle!(
      scope,
      move || read_shared(&inner, requested),
      move |scope, resolver, result| {
        super::complete(scope, resolver, result, |scope, read: ReadPayload| {
          let view = v8::Local::new(scope, &view);
          value::write_view(scope, view, &read.bytes)?;
          value::count_value(
            scope,
            io::read_outcome(read.bytes.len(), read.requested),
          )
        })
      }
    )
  }

  fn read_sync(
    &self,
    scope: &mut v8::PinScope,
    buffer: v8::Local<v8::Value>,
  ) -> Result<Option<u32>, OpError> {
    let view = value::require_view(scope, buffer);
    let (view, requested) = super::ready(scope, view)?;
    let read = self.with_file(|file| io::read_at(file, requested));
    let read = super::ready(scope, read)?;
    let view = v8::Local::new(scope, view);
    let written = value::write_view(scope, view, &read);
    super::ready(scope, written)?;
    Ok(io::read_outcome(read.len(), requested))
  }

  #[promise]
  fn write(
    &self,
    scope: &mut v8::PinScope,
    data: v8::Local<v8::Value>,
  ) -> Result<Js, OpError> {
    let bytes = value::copy_source(scope, data);
    let bytes = super::ready(scope, bytes)?;
    let inner = Arc::clone(&self.inner);
    settle!(
      scope,
      move || { lock_map(&inner, |file| io::write_at(file, &bytes)) },
      |scope, resolver, result| {
        super::complete(scope, resolver, result, |scope, count| {
          value::number_value(scope, count as f64)
        })
      }
    )
  }

  fn write_sync(
    &self,
    scope: &mut v8::PinScope,
    data: v8::Local<v8::Value>,
  ) -> Result<u32, OpError> {
    let bytes = value::copy_source(scope, data);
    let bytes = super::ready(scope, bytes)?;
    let count = self.with_file(|file| io::write_at(file, &bytes));
    let count = super::ready(scope, count)?;
    Ok(u32::try_from(count).unwrap_or(u32::MAX))
  }

  #[promise]
  fn seek(
    &self,
    scope: &mut v8::PinScope,
    offset: f64,
    whence: f64,
  ) -> Result<Js, OpError> {
    let from = super::ready(scope, io::seek_from(offset, whence))?;
    let inner = Arc::clone(&self.inner);
    settle!(
      scope,
      move || lock_map(&inner, |file| io::seek_file(file, from)),
      |scope, resolver, result| {
        super::complete(scope, resolver, result, |scope, position| {
          value::number_value(scope, position as f64)
        })
      }
    )
  }

  fn seek_sync(
    &self,
    scope: &mut v8::PinScope,
    offset: f64,
    whence: f64,
  ) -> Result<f64, OpError> {
    let from = super::ready(scope, io::seek_from(offset, whence))?;
    let position =
      super::ready(scope, self.with_file(|file| io::seek_file(file, from)))?;
    Ok(position as f64)
  }

  #[promise]
  fn stat(&self, scope: &mut v8::PinScope) -> Result<Js, OpError> {
    let inner = Arc::clone(&self.inner);
    settle!(
      scope,
      move || lock_map(&inner, |file| io::stat_file(file)),
      |scope, resolver, result| {
        super::complete(scope, resolver, result, |scope, info| {
          value::info_value(scope, &info)
        })
      }
    )
  }

  fn stat_sync(&self, scope: &mut v8::PinScope) -> Result<Js, OpError> {
    let info = self.with_file(|file| io::stat_file(file));
    let info = super::ready(scope, info)?;
    let value = value::info_value(scope, &info);
    let value = super::ready(scope, value)?;
    Ok(Js(v8::Global::new(scope, value)))
  }

  #[promise]
  fn truncate(
    &self,
    scope: &mut v8::PinScope,
    len: Option<f64>,
  ) -> Result<Js, OpError> {
    let len = super::ready(scope, io::byte_length(len.unwrap_or(0.0)))?;
    let inner = Arc::clone(&self.inner);
    settle!(
      scope,
      move || lock_map(&inner, |file| io::truncate_file(file, len)),
      |scope, resolver, result| {
        super::complete(scope, resolver, result, value::undefined_value)
      }
    )
  }

  fn truncate_sync(
    &self,
    scope: &mut v8::PinScope,
    len: Option<f64>,
  ) -> Result<(), OpError> {
    let len = super::ready(scope, io::byte_length(len.unwrap_or(0.0)))?;
    super::ready(scope, self.with_file(|file| io::truncate_file(file, len)))
  }

  #[promise]
  fn sync(&self, scope: &mut v8::PinScope) -> Result<Js, OpError> {
    self.sync_kind(scope, false)
  }

  fn sync_sync(&self, scope: &mut v8::PinScope) -> Result<(), OpError> {
    super::ready(scope, self.with_file(|file| io::sync_file(file, false)))
  }

  #[promise]
  fn sync_data(&self, scope: &mut v8::PinScope) -> Result<Js, OpError> {
    self.sync_kind(scope, true)
  }

  fn sync_data_sync(&self, scope: &mut v8::PinScope) -> Result<(), OpError> {
    super::ready(scope, self.with_file(|file| io::sync_file(file, true)))
  }

  fn close(&self, scope: &mut v8::PinScope) -> Result<(), OpError> {
    super::ready(scope, self.close_now())
  }
}

impl FsFile {
  fn sync_kind(
    &self,
    scope: &mut v8::PinScope,
    data_only: bool,
  ) -> Result<Js, OpError> {
    let inner = Arc::clone(&self.inner);
    settle!(
      scope,
      move || lock_map(&inner, |file| io::sync_file(file, data_only)),
      |scope, resolver, result| {
        super::complete(scope, resolver, result, value::undefined_value)
      }
    )
  }
}

pub(super) fn dispose(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut _ret: v8::ReturnValue,
) {
  if let Err(error) = close_receiver(scope, args.this()) {
    error::throw(scope, error);
  }
}

pub(super) fn async_dispose(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut ret: v8::ReturnValue,
) {
  let Some(resolver) = v8::PromiseResolver::new(scope) else {
    error::throw(scope, FsError::failed());
    return;
  };
  ret.set(resolver.get_promise(scope).into());
  match close_receiver(scope, args.this()) {
    Ok(()) => {
      let _ = resolver.resolve(scope, v8::undefined(scope).into());
    }
    Err(error) => {
      let exception = error.exception(scope);
      let _ = resolver.reject(scope, exception);
    }
  }
}

fn close_receiver(
  scope: &mut v8::PinScope,
  this: v8::Local<v8::Object>,
) -> Result<(), FsError> {
  let Some(host) = (unsafe { crate::op::host_ref::<FsFile>(scope, this) })
  else {
    return Err(FsError::type_("illegal invocation"));
  };
  host.get().close_now()
}
