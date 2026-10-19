//! `Blob` and `File`.
//!
//! A blob is a list of shared chunks or a file span. It is not concatenated
//! into one buffer, and a file is not read until something pulls its bytes.
//! `stream()` yields those pieces. `text()`, `arrayBuffer()`, and `bytes()`
//! are the reads that assemble one buffer.

use std::cell::RefCell;
use std::collections::HashMap;
use std::io::{Read, Seek};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use yuzuko_core::op::{Js, New, OpError, host_ref};
use yuzuko_macros::op;

use crate::bytes;

struct ReadJob {
  resolver: v8::Global<v8::PromiseResolver>,
  bytes: Vec<u8>,
  kind: ReadKind,
}

enum ReadKind {
  Text,
  Buffer,
  Bytes,
}

struct ReadPromise(v8::Global<v8::Promise>);

impl yuzuko_core::op::ToV8 for ReadPromise {
  fn to_v8(self, scope: &mut v8::PinScope, ret: &mut v8::ReturnValue) {
    ret.set(v8::Local::new(scope, self.0).into());
  }
}

thread_local! {
  static JOBS: RefCell<Vec<ReadJob>> = const { RefCell::new(Vec::new()) };
}

pub fn clear_jobs() {
  JOBS.with(|jobs| jobs.borrow_mut().clear());
  BLOB_URLS.with(|urls| urls.borrow_mut().clear());
}

struct BlobUrlEntry {
  source: BlobSource,
  mime: String,
  revoked: bool,
  users: u32,
}

thread_local! {
  static BLOB_URLS: RefCell<HashMap<String, BlobUrlEntry>> =
    RefCell::new(HashMap::new());
}

static BLOB_URL_COUNTER: AtomicU64 = AtomicU64::new(1);

/// A live use of a blob URL, such as a fetch that already resolved it.
///
/// `revokeObjectURL` hides the URL from new lookups immediately. This lease
/// keeps the bytes until the use ends, then drops a revoked entry.
pub struct BlobUrlLease {
  url: String,
}

impl BlobUrlLease {
  pub(crate) fn source(&self) -> Option<(BlobSource, String)> {
    BLOB_URLS.with(|urls| {
      urls
        .borrow()
        .get(&self.url)
        .map(|entry| (entry.source.clone(), entry.mime.clone()))
    })
  }
}

impl Drop for BlobUrlLease {
  fn drop(&mut self) {
    release_blob_url(&self.url);
  }
}

/// `URL.createObjectURL`. The entry keeps the blob bytes alive until
/// `revokeObjectURL` and every lease are done, or the realm ends.
pub fn create_object_url(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<String, OpError> {
  let object = v8::Local::<v8::Object>::try_from(value)
    .map_err(|_| OpError::type_("createObjectURL requires a Blob"))?;
  let blob = unsafe { host_ref::<Blob>(scope, object) }
    .ok_or_else(|| OpError::type_("createObjectURL requires a Blob"))?;
  let source = blob.get().source.clone();
  let mime = blob.get().mime.clone();
  let origin = environment_origin(scope);
  let url = format!("blob:{origin}/{}", blob_uuid());
  let url = blob_url_key(&url).unwrap_or(url);
  insert_blob_url(url.clone(), source, mime);
  Ok(url)
}

/// Resolve `url` for a new use. A revoked URL does not lease.
pub fn lease_blob_url(url: &str) -> Option<BlobUrlLease> {
  let key = blob_url_key(url)?;
  BLOB_URLS.with(|urls| {
    let mut urls = urls.borrow_mut();
    let entry = urls.get_mut(&key)?;
    if entry.revoked {
      return None;
    }
    entry.users += 1;
    Some(BlobUrlLease { url: key })
  })
}

pub fn revoke_object_url(url: &str) {
  let Some(key) = blob_url_key(url) else {
    return;
  };
  BLOB_URLS.with(|urls| {
    let mut urls = urls.borrow_mut();
    let Some(entry) = urls.get_mut(&key) else {
      return;
    };
    entry.revoked = true;
    if entry.users == 0 {
      urls.remove(&key);
    }
  });
}

fn insert_blob_url(url: String, source: BlobSource, mime: String) {
  BLOB_URLS.with(|urls| {
    urls.borrow_mut().insert(
      url,
      BlobUrlEntry {
        source,
        mime,
        revoked: false,
        users: 0,
      },
    );
  });
}

fn release_blob_url(url: &str) {
  BLOB_URLS.with(|urls| {
    let mut urls = urls.borrow_mut();
    let Some(entry) = urls.get_mut(url) else {
      return;
    };
    entry.users = entry.users.saturating_sub(1);
    if entry.revoked && entry.users == 0 {
      urls.remove(url);
    }
  });
}

fn blob_url_key(url: &str) -> Option<String> {
  let parsed = url::Url::parse(url).ok()?;
  if parsed.scheme() != "blob" {
    return None;
  }
  Some(parsed.into())
}

fn environment_origin(scope: &mut v8::PinScope) -> String {
  let global = scope.get_current_context().global(scope);
  let Some(key) = v8::String::new(scope, "location") else {
    return "null".to_string();
  };
  let Some(location) = global
    .get(scope, key.into())
    .and_then(|value| v8::Local::<v8::Object>::try_from(value).ok())
  else {
    return "null".to_string();
  };
  let Some(key) = v8::String::new(scope, "origin") else {
    return "null".to_string();
  };
  let Some(origin) = location
    .get(scope, key.into())
    .filter(|value| value.is_string())
    .and_then(|value| value.to_string(scope))
  else {
    return "null".to_string();
  };
  let origin = origin.to_rust_string_lossy(scope);
  if origin.is_empty() {
    "null".to_string()
  } else {
    origin
  }
}

fn blob_uuid() -> String {
  let counter = BLOB_URL_COUNTER.fetch_add(1, Ordering::Relaxed);
  let nanos = SystemTime::now()
    .duration_since(UNIX_EPOCH)
    .map(|duration| duration.as_nanos())
    .unwrap_or(0);
  let mut bytes = [0u8; 16];
  bytes[..8].copy_from_slice(&nanos.to_le_bytes()[..8]);
  bytes[8..].copy_from_slice(&counter.to_le_bytes());
  bytes[6] = (bytes[6] & 0x0f) | 0x40;
  bytes[8] = (bytes[8] & 0x3f) | 0x80;
  format!(
    "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
    bytes[0],
    bytes[1],
    bytes[2],
    bytes[3],
    bytes[4],
    bytes[5],
    bytes[6],
    bytes[7],
    bytes[8],
    bytes[9],
    bytes[10],
    bytes[11],
    bytes[12],
    bytes[13],
    bytes[14],
    bytes[15],
  )
}

#[cfg(test)]
mod blob_url_tests {
  use super::{
    BLOB_URLS, BlobSource, blob_url_key, insert_blob_url, lease_blob_url,
    revoke_object_url,
  };

  fn reset() {
    BLOB_URLS.with(|urls| urls.borrow_mut().clear());
  }

  #[test]
  fn revoke_waits_until_the_last_user_finishes() {
    reset();
    let url = "blob:null/11111111-1111-4111-8111-111111111111";
    insert_blob_url(
      url.to_string(),
      BlobSource::from_vec(b"hi".to_vec()),
      "text/plain".to_string(),
    );
    let first = lease_blob_url(url).expect("first lease");
    let second = lease_blob_url(url).expect("second lease");
    revoke_object_url(url);
    assert!(lease_blob_url(url).is_none());
    assert_eq!(first.source().expect("source").0.read_all().unwrap(), b"hi");
    drop(first);
    let key = blob_url_key(url).expect("key");
    assert!(BLOB_URLS.with(|urls| urls.borrow().contains_key(&key)));
    drop(second);
    assert!(!BLOB_URLS.with(|urls| urls.borrow().contains_key(&key)));
    reset();
  }

  #[test]
  fn revoke_without_users_removes_the_entry() {
    reset();
    let url = "blob:null/22222222-2222-4222-8222-222222222222";
    insert_blob_url(
      url.to_string(),
      BlobSource::from_vec(Vec::new()),
      String::new(),
    );
    revoke_object_url(url);
    assert!(lease_blob_url(url).is_none());
    revoke_object_url("https://example.test/");
    reset();
  }
}

const STREAM_CHUNK: usize = 64 * 1024;

/// Shared blob storage. Chunks share their allocation. A file span keeps the
/// path and length and does not read the file.
#[derive(Clone)]
pub(crate) struct BlobSource {
  inner: Arc<SourceInner>,
  size: u64,
}

enum SourceInner {
  Chunks(Vec<::bytes::Bytes>),
  File {
    path: PathBuf,
    offset: u64,
    len: u64,
  },
  Parts(Vec<BlobSource>),
}

impl BlobSource {
  pub(crate) fn from_vec(bytes: Vec<u8>) -> Self {
    let size = bytes.len() as u64;
    let chunks = if bytes.is_empty() {
      Vec::new()
    } else {
      vec![::bytes::Bytes::from(bytes)]
    };
    Self {
      inner: Arc::new(SourceInner::Chunks(chunks)),
      size,
    }
  }

  pub(crate) fn file(path: PathBuf, len: u64) -> Self {
    Self {
      inner: Arc::new(SourceInner::File {
        path,
        offset: 0,
        len,
      }),
      size: len,
    }
  }

  pub(crate) fn from_list(mut parts: Vec<Self>) -> Self {
    if parts.len() == 1 {
      return parts.pop().unwrap();
    }
    let size = parts.iter().map(|part| part.size).sum();
    Self {
      inner: Arc::new(SourceInner::Parts(parts)),
      size,
    }
  }

  pub(crate) fn size(&self) -> u64 {
    self.size
  }

  pub(crate) fn read_all(&self) -> std::io::Result<Vec<u8>> {
    let mut out = Vec::new();
    let mut cursor = 0u64;
    while let Some(chunk) = self.read_next(cursor, STREAM_CHUNK)? {
      cursor += chunk.len() as u64;
      out.extend_from_slice(&chunk);
    }
    Ok(out)
  }

  /// The next slice at `cursor`, at most `max` bytes. `None` at the end.
  pub(crate) fn read_next(
    &self,
    cursor: u64,
    max: usize,
  ) -> std::io::Result<Option<::bytes::Bytes>> {
    if max == 0 || cursor >= self.size {
      return Ok(None);
    }
    let max = max.min((self.size - cursor) as usize);
    match &*self.inner {
      SourceInner::Chunks(chunks) => Ok(slice_chunks(chunks, cursor, max)),
      SourceInner::File { path, offset, len } => {
        let remain = (*len - cursor) as usize;
        read_file(path, offset + cursor, max.min(remain)).map(Some)
      }
      SourceInner::Parts(parts) => {
        let mut skip = cursor;
        for part in parts {
          if skip >= part.size {
            skip -= part.size;
            continue;
          }
          return part.read_next(skip, max);
        }
        Ok(None)
      }
    }
  }

  pub(crate) fn slice(&self, start: u64, end: u64) -> Self {
    let end = end.min(self.size);
    let start = start.min(end);
    let len = end - start;
    if start == 0 && len == self.size {
      return self.clone();
    }
    match &*self.inner {
      SourceInner::File { path, offset, .. } => Self {
        inner: Arc::new(SourceInner::File {
          path: path.clone(),
          offset: offset + start,
          len,
        }),
        size: len,
      },
      SourceInner::Chunks(chunks) => Self {
        inner: Arc::new(SourceInner::Chunks(slice_chunk_list(
          chunks, start, end,
        ))),
        size: len,
      },
      SourceInner::Parts(parts) => {
        let mut out = Vec::new();
        let mut pos = 0u64;
        for part in parts {
          let part_end = pos + part.size;
          if part_end > start && pos < end {
            let from = start.saturating_sub(pos);
            let to = end.min(part_end) - pos;
            out.push(part.slice(from, to));
          }
          pos = part_end;
          if pos >= end {
            break;
          }
        }
        let sliced = Self::from_list(out);
        Self {
          size: len,
          ..sliced
        }
      }
    }
  }
}

fn slice_chunks(
  chunks: &[::bytes::Bytes],
  cursor: u64,
  max: usize,
) -> Option<::bytes::Bytes> {
  let mut pos = 0u64;
  for chunk in chunks {
    let end = pos + chunk.len() as u64;
    if cursor < end {
      let start = (cursor - pos) as usize;
      let take = max.min(chunk.len() - start);
      return Some(chunk.slice(start..start + take));
    }
    pos = end;
  }
  None
}

fn slice_chunk_list(
  chunks: &[::bytes::Bytes],
  start: u64,
  end: u64,
) -> Vec<::bytes::Bytes> {
  let mut out = Vec::new();
  let mut pos = 0u64;
  for chunk in chunks {
    let chunk_end = pos + chunk.len() as u64;
    if chunk_end > start && pos < end {
      let from = start.saturating_sub(pos) as usize;
      let to = (end.min(chunk_end) - pos) as usize;
      if from < to {
        out.push(chunk.slice(from..to));
      }
    }
    pos = chunk_end;
    if pos >= end {
      break;
    }
  }
  out
}

fn read_file(
  path: &PathBuf,
  offset: u64,
  len: usize,
) -> std::io::Result<::bytes::Bytes> {
  let mut file = std::fs::File::open(path)?;
  file.seek(std::io::SeekFrom::Start(offset))?;
  let mut buf = vec![0u8; len];
  file.read_exact(&mut buf)?;
  Ok(::bytes::Bytes::from(buf))
}

pub struct Blob {
  source: BlobSource,
  /// `Blob.type`: MIME serialization.
  mime: String,
  /// Content-Type used when this blob is a fetch body.
  wire_type: String,
}

/// A `Blob` object whose bytes and type are already encoded.
pub fn new_blob<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  bytes: Vec<u8>,
  mime: &str,
) -> v8::Local<'s, v8::Object> {
  let blob = Blob::from_parts(bytes, normalize_type(mime));
  unsafe { yuzuko_core::op::wrap_instance(scope, blob) }
}

impl Blob {
  pub(crate) fn from_parts(bytes: Vec<u8>, mime: String) -> Self {
    Self::from_source(BlobSource::from_vec(bytes), mime)
  }

  pub(crate) fn from_source(source: BlobSource, mime: String) -> Self {
    let mime = crate::text::mime::blob_type(&mime);
    Self {
      source,
      wire_type: mime.clone(),
      mime,
    }
  }

  /// `type` is the MIME serialization. `wire_type` keeps the File API
  /// spelling, including the space in `a/b; c=d`, for fetch Content-Type.
  fn from_input(source: BlobSource, input: &str) -> Self {
    Self {
      source,
      mime: crate::text::mime::blob_type(input),
      wire_type: crate::text::mime::file_type(input),
    }
  }

  pub(crate) fn snapshot(&self) -> Result<(Vec<u8>, String), OpError> {
    let bytes = self
      .source
      .read_all()
      .map_err(|_| OpError::type_("The blob could not be read"))?;
    Ok((bytes, self.mime.clone()))
  }
}

impl File {
  pub(crate) fn form_parts(
    &self,
  ) -> Result<(Vec<u8>, String, String), OpError> {
    let (bytes, mime) = self.base.snapshot()?;
    Ok((bytes, mime, self.name.clone()))
  }

  pub(crate) fn from_owned(bytes: Vec<u8>, mime: String, name: String) -> Self {
    Self {
      base: Blob::from_parts(bytes, mime),
      name,
      last_modified: 0.0,
    }
  }
}

/// Bytes and type of a `Blob` or `File`, or `None` when `value` is neither.
pub fn blob_parts(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<Option<(Vec<u8>, String)>, OpError> {
  let Ok(object) = v8::Local::<v8::Object>::try_from(value) else {
    return Ok(None);
  };
  let Some(host) = (unsafe { host_ref::<Blob>(scope, object) }) else {
    return Ok(None);
  };
  let blob = host.get();
  let (bytes, _) = blob.snapshot()?;
  Ok(Some((bytes, blob.wire_type.clone())))
}

unsafe impl v8::cppgc::GarbageCollected for Blob {
  fn trace(&self, _visitor: &mut v8::cppgc::Visitor) {}

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"Blob"
  }
}

#[op]
impl Blob {
  #[constructor]
  fn new(
    scope: &mut v8::PinScope,
    parts: Option<v8::Local<v8::Value>>,
    options: Option<v8::Local<v8::Value>>,
  ) -> Result<Blob, OpError> {
    let pieces = read_pieces(scope, parts)?;
    let endings = endings_option(scope, options)?;
    let input = options_type(scope, options)?;
    let source = assemble(pieces, endings);
    Ok(Blob::from_input(source, &input))
  }

  #[getter]
  fn size(&self) -> f64 {
    self.source.size() as f64
  }

  #[getter]
  #[rename("type")]
  fn type_(&self) -> String {
    self.mime.clone()
  }

  fn slice(
    &self,
    scope: &mut v8::PinScope,
    start: Option<f64>,
    end: Option<f64>,
    content_type: Option<String>,
  ) -> Result<New<Blob>, OpError> {
    let length = i64::try_from(self.source.size()).unwrap_or(i64::MAX);
    let start = relative_index(start, length, 0);
    let end = relative_index(end, length, length);
    let start = start.max(0) as u64;
    let end = end.max(0) as u64;
    let input = content_type.unwrap_or_default();
    let _ = scope;
    Ok(New(Blob::from_input(self.source.slice(start, end), &input)))
  }

  fn text(&self, scope: &mut v8::PinScope) -> ReadPromise {
    queue_owned(scope, self, ReadKind::Text)
  }

  fn array_buffer(&self, scope: &mut v8::PinScope) -> ReadPromise {
    queue_owned(scope, self, ReadKind::Buffer)
  }

  fn bytes(&self, scope: &mut v8::PinScope) -> ReadPromise {
    queue_owned(scope, self, ReadKind::Bytes)
  }

  fn stream(&self, scope: &mut v8::PinScope) -> Js {
    let object = crate::stream::stream_from_source(scope, self.source.clone());
    let object = v8::Local::new(scope, &object);
    let value: v8::Local<v8::Value> = object.into();
    Js(v8::Global::new(scope, value))
  }
}

#[repr(C)]
pub struct File {
  base: Blob,
  name: String,
  last_modified: f64,
}

unsafe impl v8::cppgc::GarbageCollected for File {
  fn trace(&self, visitor: &mut v8::cppgc::Visitor) {
    v8::cppgc::GarbageCollected::trace(&self.base, visitor);
  }

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"File"
  }
}

#[op(inherit = Blob)]
impl File {
  #[constructor]
  fn new(
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<File, OpError> {
    if args.length() < 2 {
      return Err(OpError::type_("File requires a name"));
    }
    let pieces = if args.length() < 1 {
      Vec::new()
    } else {
      read_pieces(scope, Some(args.get(0)))?
    };
    let name = try_text(scope, args.get(1))?;
    let endings = if args.length() < 3 {
      Endings::Transparent
    } else {
      endings_option(scope, Some(args.get(2)))?
    };
    let source = assemble(pieces, endings);
    let (input, last_modified) = file_options(scope, args.get(2))?;
    Ok(File {
      base: Blob::from_input(source, &input),
      name,
      last_modified,
    })
  }

  #[getter]
  fn name(&self) -> String {
    self.name.clone()
  }

  #[getter]
  fn last_modified(&self) -> f64 {
    self.last_modified
  }
}

pub fn install(scope: &mut v8::PinScope) {
  Blob::install(scope);
  File::install(scope);
}

/// Host clone tags shared with `DOMException` (`3`).
const TAG_BLOB: u32 = 1;
const TAG_FILE: u32 = 2;

pub(crate) fn register_clone() {
  yuzuko_core::add_host_clone(write_blob_clone, read_blob_clone);
}

fn write_blob_clone(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  helper: &dyn v8::ValueSerializerHelper,
) -> yuzuko_core::WriteResult {
  // `object` is the live host value V8 asked us to serialize.
  let file = if let Some(file) = unsafe { host_ref::<File>(scope, object) } {
    let name = file.get().name.clone();
    let modified = file.get().last_modified;
    match file.get().base.snapshot() {
      Ok((bytes, mime)) => Some((bytes, mime, name, modified)),
      Err(_) => {
        yuzuko_core::reject_clone(scope);
        return yuzuko_core::WriteResult::Failed;
      }
    }
  } else {
    None
  };
  if let Some((bytes, mime, name, modified)) = file {
    helper.write_uint32(TAG_FILE);
    yuzuko_core::write_bytes(helper, &bytes);
    if !yuzuko_core::write_text(helper, &mime)
      || !yuzuko_core::write_text(helper, &name)
    {
      yuzuko_core::reject_clone(scope);
      return yuzuko_core::WriteResult::Failed;
    }
    helper.write_double(modified);
    return yuzuko_core::WriteResult::Wrote;
  }
  let Some(blob) = (unsafe { host_ref::<Blob>(scope, object) }) else {
    return yuzuko_core::WriteResult::Skip;
  };
  let (bytes, mime) = match blob.get().snapshot() {
    Ok(parts) => parts,
    Err(_) => {
      yuzuko_core::reject_clone(scope);
      return yuzuko_core::WriteResult::Failed;
    }
  };
  helper.write_uint32(TAG_BLOB);
  yuzuko_core::write_bytes(helper, &bytes);
  if !yuzuko_core::write_text(helper, &mime) {
    yuzuko_core::reject_clone(scope);
    return yuzuko_core::WriteResult::Failed;
  }
  yuzuko_core::WriteResult::Wrote
}

fn read_blob_clone(
  scope: &mut v8::PinScope,
  tag: u32,
  helper: &dyn v8::ValueDeserializerHelper,
) -> yuzuko_core::ReadResult {
  match tag {
    TAG_BLOB => read_cloned_blob(scope, helper),
    TAG_FILE => read_cloned_file(scope, helper),
    _ => yuzuko_core::ReadResult::Skip,
  }
}

fn read_cloned_blob(
  scope: &mut v8::PinScope,
  helper: &dyn v8::ValueDeserializerHelper,
) -> yuzuko_core::ReadResult {
  let Some((bytes, mime)) = read_blob_parts(helper) else {
    yuzuko_core::reject_clone(scope);
    return yuzuko_core::ReadResult::Failed;
  };
  let object = unsafe {
    yuzuko_core::op::wrap_instance(scope, Blob::from_parts(bytes, mime))
  };
  yuzuko_core::ReadResult::Done(v8::Global::new(scope, object))
}

fn read_cloned_file(
  scope: &mut v8::PinScope,
  helper: &dyn v8::ValueDeserializerHelper,
) -> yuzuko_core::ReadResult {
  let Some((bytes, mime)) = read_blob_parts(helper) else {
    yuzuko_core::reject_clone(scope);
    return yuzuko_core::ReadResult::Failed;
  };
  let Some(name) = yuzuko_core::read_text(helper) else {
    yuzuko_core::reject_clone(scope);
    return yuzuko_core::ReadResult::Failed;
  };
  let mut last_modified = 0.0;
  if !helper.read_double(&mut last_modified) {
    yuzuko_core::reject_clone(scope);
    return yuzuko_core::ReadResult::Failed;
  }
  let object = unsafe {
    yuzuko_core::op::wrap_instance(
      scope,
      File {
        base: Blob::from_parts(bytes, mime),
        name,
        last_modified,
      },
    )
  };
  yuzuko_core::ReadResult::Done(v8::Global::new(scope, object))
}

fn read_blob_parts(
  helper: &dyn v8::ValueDeserializerHelper,
) -> Option<(Vec<u8>, String)> {
  let bytes = yuzuko_core::read_bytes(helper)?;
  let mime = yuzuko_core::read_text(helper)?;
  Some((bytes, mime))
}

#[derive(Clone, Copy)]
enum Endings {
  Transparent,
  Native,
}

fn endings_option(
  scope: &mut v8::PinScope,
  options: Option<v8::Local<v8::Value>>,
) -> Result<Endings, OpError> {
  let Some(object) = options_dictionary(options)? else {
    return Ok(Endings::Transparent);
  };
  let captured = {
    v8::tc_scope!(let tc, scope);
    let key = v8::String::new(tc, "endings").unwrap();
    match object.get(tc, key.into()) {
      Some(value) => Ok(v8::Global::new(tc, value)),
      None => {
        let exception =
          tc.exception().unwrap_or_else(|| v8::undefined(tc).into());
        Err(v8::Global::new(tc, exception))
      }
    }
  };
  let value = match captured {
    Ok(value) => v8::Local::new(scope, value),
    Err(exception) => {
      let exception = v8::Local::new(scope, exception);
      return Err(OpError::thrown(scope, exception));
    }
  };
  if value.is_undefined() {
    return Ok(Endings::Transparent);
  }
  let text = bytes::string_value(scope, value);
  match text.as_str() {
    "transparent" => Ok(Endings::Transparent),
    "native" => Ok(Endings::Native),
    _ => Err(OpError::type_("Blob endings is not a valid value")),
  }
}

enum Piece {
  Source(BlobSource),
  Text(String),
}

fn read_pieces(
  scope: &mut v8::PinScope,
  parts: Option<v8::Local<v8::Value>>,
) -> Result<Vec<Piece>, OpError> {
  let Some(parts) = parts else {
    return Ok(Vec::new());
  };
  if parts.is_undefined() {
    return Ok(Vec::new());
  }
  if !parts.is_object() {
    return Err(OpError::type_("Blob parts must be an object"));
  }
  let mut pieces = Vec::new();
  for_each(scope, parts, |scope, item| {
    pieces.push(read_piece(scope, item)?);
    Ok(())
  })?;
  Ok(pieces)
}

fn read_piece(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<Piece, OpError> {
  if let Ok(object) = v8::Local::<v8::Object>::try_from(value)
    && let Some(blob) = unsafe { host_ref::<Blob>(scope, object) }
  {
    return Ok(Piece::Source(blob.get().source.clone()));
  }
  if let Some(bytes) = bytes::buffer_source(scope, value) {
    return Ok(Piece::Source(BlobSource::from_vec(bytes)));
  }
  Ok(Piece::Text(try_text(scope, value)?))
}

fn assemble(pieces: Vec<Piece>, endings: Endings) -> BlobSource {
  let mut parts = Vec::new();
  for piece in pieces {
    match piece {
      Piece::Source(source) => parts.push(source),
      Piece::Text(text) => {
        let text = match endings {
          Endings::Transparent => text,
          Endings::Native => native_newlines(&text),
        };
        parts.push(BlobSource::from_vec(text.into_bytes()));
      }
    }
  }
  BlobSource::from_list(parts)
}

fn try_text(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<String, OpError> {
  let captured = {
    v8::tc_scope!(let tc, scope);
    match value.to_string(tc) {
      Some(text) => Ok(text.to_rust_string_lossy(tc)),
      None => Err(caught_exception(tc)),
    }
  };
  match captured {
    Ok(text) => Ok(text),
    Err(exception) => Err(rethrow_exception(scope, exception)),
  }
}

fn native_newlines(text: &str) -> String {
  let newline = if cfg!(windows) { "\r\n" } else { "\n" };
  let mut out = String::with_capacity(text.len());
  let mut chars = text.chars().peekable();
  while let Some(ch) = chars.next() {
    if ch == '\r' {
      if chars.peek() == Some(&'\n') {
        chars.next();
      }
      out.push_str(newline);
    } else if ch == '\n' {
      out.push_str(newline);
    } else {
      out.push(ch);
    }
  }
  out
}

fn options_dictionary(
  value: Option<v8::Local<v8::Value>>,
) -> Result<Option<v8::Local<v8::Object>>, OpError> {
  let Some(value) = value else {
    return Ok(None);
  };
  if value.is_undefined() || value.is_null() {
    return Ok(None);
  }
  v8::Local::<v8::Object>::try_from(value)
    .map(Some)
    .map_err(|_| OpError::type_("Blob options must be an object"))
}

fn options_type(
  scope: &mut v8::PinScope,
  value: Option<v8::Local<v8::Value>>,
) -> Result<String, OpError> {
  let Some(object) = options_dictionary(value)? else {
    return Ok(String::new());
  };
  let type_value = call_get(scope, object, key(scope, "type").into())?;
  if type_value.is_undefined() {
    return Ok(String::new());
  }
  try_text(scope, type_value)
}

fn file_options(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<(String, f64), OpError> {
  let now = unix_now_ms();
  if value.is_undefined() || value.is_null() {
    return Ok((String::new(), now));
  }
  if !value.is_object() {
    return Err(OpError::type_("File options must be an object"));
  }
  let mime = options_type(scope, Some(value))?;
  let Ok(object) = v8::Local::<v8::Object>::try_from(value) else {
    return Ok((mime, now));
  };
  let modified_value =
    call_get(scope, object, key(scope, "lastModified").into())?;
  if modified_value.is_undefined() {
    return Ok((mime, now));
  }
  let number = try_number(scope, modified_value)?;
  let modified = if number.is_finite() { number } else { now };
  Ok((mime, modified))
}

/// Whole milliseconds since the Unix epoch, matching `Date.now()`.
fn unix_now_ms() -> f64 {
  SystemTime::now()
    .duration_since(UNIX_EPOCH)
    .map(|duration| duration.as_millis() as f64)
    .unwrap_or(0.0)
}

fn try_number(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<f64, OpError> {
  let captured = {
    v8::tc_scope!(let tc, scope);
    match value.number_value(tc) {
      Some(number) => Ok(number),
      None => Err(caught_exception(tc)),
    }
  };
  match captured {
    Ok(number) => Ok(number),
    Err(exception) => Err(rethrow_exception(scope, exception)),
  }
}

fn normalize_type(value: &str) -> String {
  crate::text::mime::blob_type(value)
}

fn queue_owned(
  scope: &mut v8::PinScope,
  blob: &Blob,
  kind: ReadKind,
) -> ReadPromise {
  match blob.snapshot() {
    Ok((bytes, _)) => queue_read(scope, bytes, kind),
    Err(_) => {
      let resolver = v8::PromiseResolver::new(scope).unwrap();
      let promise = resolver.get_promise(scope);
      let message =
        v8::String::new(scope, "The blob could not be read").unwrap();
      resolver.reject(scope, v8::Exception::type_error(scope, message));
      ReadPromise(v8::Global::new(scope, promise))
    }
  }
}

fn queue_read(
  scope: &mut v8::PinScope,
  bytes: Vec<u8>,
  kind: ReadKind,
) -> ReadPromise {
  let resolver = v8::PromiseResolver::new(scope).unwrap();
  let promise = resolver.get_promise(scope);
  JOBS.with(|jobs| {
    jobs.borrow_mut().push(ReadJob {
      resolver: v8::Global::new(scope, resolver),
      bytes,
      kind,
    });
  });
  let function = v8::Function::new(scope, finish_read).unwrap();
  scope.enqueue_microtask(function);
  ReadPromise(v8::Global::new(scope, promise))
}

fn finish_read(
  scope: &mut v8::PinScope,
  _args: v8::FunctionCallbackArguments,
  mut _ret: v8::ReturnValue,
) {
  let Some(job) = JOBS.with(|jobs| {
    let mut jobs = jobs.borrow_mut();
    if jobs.is_empty() {
      None
    } else {
      Some(jobs.remove(0))
    }
  }) else {
    return;
  };
  let resolver = v8::Local::new(scope, &job.resolver);
  let value: v8::Local<v8::Value> = match job.kind {
    ReadKind::Text => {
      let text = String::from_utf8_lossy(&job.bytes).into_owned();
      v8::String::new(scope, &text).unwrap().into()
    }
    ReadKind::Buffer => bytes::array_buffer(scope, job.bytes).into(),
    ReadKind::Bytes => bytes::uint8_array(scope, job.bytes).into(),
  };
  resolver.resolve(scope, value);
}

fn relative_index(value: Option<f64>, length: i64, default: i64) -> i64 {
  let Some(number) = value else {
    return default;
  };
  let number = clamp_long_long(number);
  if number < 0 {
    (length + number).max(0)
  } else {
    number.min(length)
  }
}

/// Web IDL `[Clamp] long long`. Ties round to even, and the 64-bit range is
/// the exact integer span of a Number, `±(2^53 − 1)`.
fn clamp_long_long(number: f64) -> i64 {
  if number.is_nan() || number == 0.0 {
    return 0;
  }
  const LIMIT: f64 = 9_007_199_254_740_991.0;
  let rounded = number.clamp(-LIMIT, LIMIT).round_ties_even();
  if rounded == 0.0 { 0 } else { rounded as i64 }
}

fn for_each(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
  mut visit: impl FnMut(
    &mut v8::PinScope,
    v8::Local<v8::Value>,
  ) -> Result<(), OpError>,
) -> Result<(), OpError> {
  let object = v8::Local::<v8::Object>::try_from(value)
    .map_err(|_| OpError::type_("Blob parts must be an object"))?;
  let symbol = v8::Symbol::get_iterator(scope);
  let method = call_get(scope, object, symbol.into())?;
  let method = v8::Local::<v8::Function>::try_from(method)
    .map_err(|_| OpError::type_("Blob parts must be iterable"))?;
  let iterator = call_function(scope, method, object.into(), &[])?;
  let iterator = v8::Local::<v8::Object>::try_from(iterator)
    .map_err(|_| OpError::type_("Blob parts iterator is not an object"))?;
  loop {
    let next_key = key(scope, "next");
    let next = call_get(scope, iterator, next_key.into())?;
    let next = v8::Local::<v8::Function>::try_from(next)
      .map_err(|_| OpError::type_("Blob parts iterator has no next"))?;
    let step = call_function(scope, next, iterator.into(), &[])?;
    let step = v8::Local::<v8::Object>::try_from(step).map_err(|_| {
      OpError::type_("Blob parts iterator result is not an object")
    })?;
    let done_key = key(scope, "done");
    let done = call_get(scope, step, done_key.into())?;
    if done.boolean_value(scope) {
      return Ok(());
    }
    let value_key = key(scope, "value");
    let item = call_get(scope, step, value_key.into())?;
    visit(scope, item)?;
  }
}

fn call_get<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  object: v8::Local<v8::Object>,
  property: v8::Local<v8::Value>,
) -> Result<v8::Local<'s, v8::Value>, OpError> {
  let captured = {
    v8::tc_scope!(let tc, scope);
    match object.get(tc, property) {
      Some(value) => Ok(v8::Global::new(tc, value)),
      None => Err(caught_exception(tc)),
    }
  };
  match captured {
    Ok(value) => Ok(v8::Local::new(scope, value)),
    Err(exception) => Err(rethrow_exception(scope, exception)),
  }
}

fn call_function<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  function: v8::Local<v8::Function>,
  receiver: v8::Local<v8::Value>,
  args: &[v8::Local<v8::Value>],
) -> Result<v8::Local<'s, v8::Value>, OpError> {
  let captured = {
    v8::tc_scope!(let tc, scope);
    match function.call(tc, receiver, args) {
      Some(value) => Ok(v8::Global::new(tc, value)),
      None => Err(caught_exception(tc)),
    }
  };
  match captured {
    Ok(value) => Ok(v8::Local::new(scope, value)),
    Err(exception) => Err(rethrow_exception(scope, exception)),
  }
}

fn caught_exception(
  scope: &mut v8::PinnedRef<v8::TryCatch<v8::HandleScope>>,
) -> v8::Global<v8::Value> {
  let exception = scope
    .exception()
    .unwrap_or_else(|| v8::undefined(scope).into());
  v8::Global::new(scope, exception)
}

fn rethrow_exception(
  scope: &mut v8::PinScope,
  exception: v8::Global<v8::Value>,
) -> OpError {
  let exception = v8::Local::new(scope, exception);
  OpError::thrown(scope, exception)
}

fn key<'s>(
  scope: &v8::PinScope<'s, '_>,
  text: &str,
) -> v8::Local<'s, v8::String> {
  v8::String::new(scope, text).unwrap()
}

/// Bytes and MIME type of a `Blob` or `File` argument.
pub fn snapshot_blob(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<(Vec<u8>, String), OpError> {
  let object = v8::Local::<v8::Object>::try_from(value)
    .map_err(|_| OpError::type_("FileReader requires a Blob"))?;
  let host = unsafe { host_ref::<Blob>(scope, object) }
    .ok_or_else(|| OpError::type_("FileReader requires a Blob"))?;
  host.get().snapshot()
}

/// `readAsText`: an explicit label wins, then the type's charset, then a BOM.
/// UTF-8 decode that strips a UTF-8 BOM and does not sniff UTF-16.
pub fn decode_utf8(bytes: &[u8]) -> String {
  decode_with(encoding_rs::UTF_8, bytes)
}

pub fn decode_blob_text(
  bytes: &[u8],
  mime: &str,
  encoding: Option<&str>,
) -> String {
  let label = encoding.or_else(|| charset_label(mime));
  if let Some(label) = label
    && let Some(encoding) =
      encoding_rs::Encoding::for_label_no_replacement(label.as_bytes())
  {
    return decode_with(encoding, bytes);
  }
  // `Encoding::decode` sniffs a UTF-8 or UTF-16 BOM and strips it.
  encoding_rs::UTF_8.decode(bytes).0.into_owned()
}

pub fn binary_string(bytes: &[u8]) -> String {
  bytes.iter().map(|byte| char::from(*byte)).collect()
}

pub fn data_url(bytes: &[u8], mime: &str) -> String {
  let mime = if mime.is_empty() {
    "application/octet-stream"
  } else {
    mime
  };
  format!("data:{mime};base64,{}", base64_encode(bytes))
}

fn charset_label(mime: &str) -> Option<&str> {
  for part in mime.split(';').skip(1) {
    let Some((name, value)) = part.split_once('=') else {
      continue;
    };
    if name.trim().eq_ignore_ascii_case("charset") {
      let value = value.trim().trim_matches('"');
      if !value.is_empty() {
        return Some(value);
      }
    }
  }
  None
}

fn decode_with(
  encoding: &'static encoding_rs::Encoding,
  bytes: &[u8],
) -> String {
  let mut decoder = encoding.new_decoder_with_bom_removal();
  let mut output = String::new();
  let extra = decoder
    .max_utf8_buffer_length(bytes.len())
    .unwrap_or(bytes.len().saturating_mul(3).saturating_add(16));
  output.reserve(extra);
  let _ = decoder.decode_to_string(bytes, &mut output, true);
  output
}

#[cfg(test)]
mod source_tests {
  use super::BlobSource;

  #[test]
  fn a_file_blob_reads_one_chunk_without_loading_the_rest() {
    let path = std::env::temp_dir().join("yuzuko-blob-source-test");
    let bytes = vec![7u8; 70_000];
    std::fs::write(&path, &bytes).unwrap();
    let source = BlobSource::file(path.clone(), bytes.len() as u64);
    assert_eq!(source.size(), bytes.len() as u64);
    let first = source.read_next(0, 64 * 1024).unwrap().unwrap();
    assert_eq!(first.len(), 64 * 1024);
    assert_eq!(&first[..], &bytes[..64 * 1024]);
    let sliced = source.slice(64 * 1024, bytes.len() as u64);
    assert_eq!(sliced.size(), bytes.len() as u64 - 64 * 1024);
    assert_eq!(sliced.read_all().unwrap(), bytes[64 * 1024..]);
    let _ = std::fs::remove_file(path);
  }
}

fn base64_encode(bytes: &[u8]) -> String {
  const TABLE: &[u8] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
  let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
  let mut index = 0;
  while index + 3 <= bytes.len() {
    let chunk = ((bytes[index] as u32) << 16)
      | ((bytes[index + 1] as u32) << 8)
      | bytes[index + 2] as u32;
    out.push(TABLE[((chunk >> 18) & 63) as usize] as char);
    out.push(TABLE[((chunk >> 12) & 63) as usize] as char);
    out.push(TABLE[((chunk >> 6) & 63) as usize] as char);
    out.push(TABLE[(chunk & 63) as usize] as char);
    index += 3;
  }
  let rest = bytes.len() - index;
  if rest == 1 {
    let chunk = (bytes[index] as u32) << 16;
    out.push(TABLE[((chunk >> 18) & 63) as usize] as char);
    out.push(TABLE[((chunk >> 12) & 63) as usize] as char);
    out.push('=');
    out.push('=');
  } else if rest == 2 {
    let chunk =
      ((bytes[index] as u32) << 16) | ((bytes[index + 1] as u32) << 8);
    out.push(TABLE[((chunk >> 18) & 63) as usize] as char);
    out.push(TABLE[((chunk >> 12) & 63) as usize] as char);
    out.push(TABLE[((chunk >> 6) & 63) as usize] as char);
    out.push('=');
  }
  out
}
