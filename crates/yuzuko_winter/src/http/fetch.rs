//! `fetch`, `Headers`, `Request`, and `Response`.
//!
//! The runtime uses the unrestricted fetch posture: no forbidden headers or
//! methods, no CORS, and `redirect: "manual"` returns the redirect response.
//! `file:` stays under the entry directory and is read when the body is pulled.
//! `http:` and `https:` use the network client.

use std::cell::Cell;
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use yuzuko_core::op::Js;
use yuzuko_core::op::New;
use yuzuko_core::op::OpError;
use yuzuko_core::op::traced;
use yuzuko_core::op::traced_local;
use yuzuko_macros::op;

use crate::blob::Blob;
use crate::bytes;
use crate::lease_blob_url;
use crate::stream::StreamBody;

struct AbortWatch {
  cancel: Arc<AtomicBool>,
  stream: Cell<u64>,
  reason: RefCell<Option<v8::Global<v8::Value>>>,
  signal: v8::Global<v8::Object>,
}

thread_local! {
  static ABORT_FLAGS: RefCell<HashMap<u64, AbortWatch>> =
    RefCell::new(HashMap::new());
}

static NEXT_ABORT: AtomicU64 = AtomicU64::new(1);

struct Loaded {
  source: crate::blob::BlobSource,
  mime: String,
  url: String,
  status: u16,
  status_text: &'static str,
  content_length: Option<u64>,
  content_range: Option<String>,
  /// Range responses include `Content-Type` even when the blob type is empty.
  force_type: bool,
}

impl Loaded {
  fn new(source: crate::blob::BlobSource, mime: String, url: String) -> Self {
    Self {
      source,
      mime,
      url,
      status: 200,
      status_text: "OK",
      content_length: None,
      content_range: None,
      force_type: false,
    }
  }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum HeaderGuard {
  None,
  Immutable,
  Request,
  RequestNoCors,
  Response,
}

pub struct Headers {
  pairs: RefCell<Vec<(String, String)>>,
  guard: HeaderGuard,
}

unsafe impl v8::cppgc::GarbageCollected for Headers {
  fn trace(&self, _visitor: &mut v8::cppgc::Visitor) {}

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"Headers"
  }
}

impl Headers {
  pub(crate) fn from_pairs(
    pairs: Vec<(String, String)>,
    guard: HeaderGuard,
  ) -> Result<Self, OpError> {
    let headers = Self {
      pairs: RefCell::new(Vec::new()),
      guard,
    };
    if guard == HeaderGuard::Immutable {
      *headers.pairs.borrow_mut() = pairs;
      return Ok(headers);
    }
    for (name, value) in pairs {
      headers.append_pair(name, value)?;
    }
    Ok(headers)
  }

  pub(crate) fn snapshot(&self) -> Vec<(String, String)> {
    self.pairs.borrow().clone()
  }

  pub(crate) fn contains(&self, name: &str) -> bool {
    self.pairs.borrow().iter().any(|(key, _)| key == name)
  }

  pub(crate) fn append_pair(
    &self,
    name: String,
    value: String,
  ) -> Result<(), OpError> {
    let name = normalize_header(&name)?;
    let value = normalize_value(&value)?;
    if !self.allow(&name, &value)? {
      return Ok(());
    }
    if self.guard == HeaderGuard::RequestNoCors {
      let temporary = self.combined(&name, &value);
      if !is_no_cors_safelisted(&name, &temporary) {
        return Ok(());
      }
    }
    self.pairs.borrow_mut().push((name, value));
    self.strip_privileged();
    Ok(())
  }

  pub(crate) fn set_pair(
    &self,
    name: String,
    value: String,
  ) -> Result<(), OpError> {
    let name = normalize_header(&name)?;
    let value = normalize_value(&value)?;
    if !self.allow(&name, &value)? {
      return Ok(());
    }
    if self.guard == HeaderGuard::RequestNoCors
      && !is_no_cors_safelisted(&name, &value)
    {
      return Ok(());
    }
    {
      let mut pairs = self.pairs.borrow_mut();
      pairs.retain(|(key, _)| key != &name);
      pairs.push((name, value));
    }
    self.strip_privileged();
    Ok(())
  }

  pub(crate) fn delete_name(&self, name: String) -> Result<(), OpError> {
    let name = normalize_header(&name)?;
    if !self.allow(&name, "")? {
      return Ok(());
    }
    if self.guard == HeaderGuard::RequestNoCors
      && !is_no_cors_name(&name)
      && name != "range"
    {
      return Ok(());
    }
    self.pairs.borrow_mut().retain(|(key, _)| key != &name);
    self.strip_privileged();
    Ok(())
  }

  fn allow(&self, name: &str, value: &str) -> Result<bool, OpError> {
    match self.guard {
      HeaderGuard::Immutable => Err(OpError::type_("Headers are immutable")),
      HeaderGuard::Request if is_forbidden_request_header(name, value) => {
        Ok(false)
      }
      _ => Ok(true),
    }
  }

  fn combined(&self, name: &str, value: &str) -> String {
    let pairs = self.pairs.borrow();
    if !pairs.iter().any(|(key, _)| key == name) {
      return value.to_string();
    }
    let current = pairs
      .iter()
      .filter(|(key, _)| key == name)
      .map(|(_, item)| item.as_str())
      .collect::<Vec<_>>()
      .join(", ");
    format!("{current}, {value}")
  }

  fn strip_privileged(&self) {
    if self.guard == HeaderGuard::RequestNoCors {
      self.pairs.borrow_mut().retain(|(key, _)| key != "range");
    }
  }
}

#[op]
impl Headers {
  #[constructor]
  fn new(
    scope: &mut v8::PinScope,
    init: Option<v8::Local<v8::Value>>,
  ) -> Result<Headers, OpError> {
    let pairs = read_header_init(scope, init)?;
    Headers::from_pairs(pairs, HeaderGuard::None)
  }

  fn get(&self, name: String) -> Result<JsPending, OpError> {
    let name = normalize_header(&name)?;
    let joined = self
      .pairs
      .borrow()
      .iter()
      .filter(|(key, _)| key == &name)
      .map(|(_, value)| value.as_str())
      .collect::<Vec<_>>()
      .join(", ");
    if joined.is_empty()
      && !self.pairs.borrow().iter().any(|(key, _)| key == &name)
    {
      Ok(JsPending::Null)
    } else {
      Ok(JsPending::Text(joined))
    }
  }

  fn has(&self, name: String) -> Result<bool, OpError> {
    let name = normalize_header(&name)?;
    Ok(self.pairs.borrow().iter().any(|(key, _)| key == &name))
  }

  fn set(&self, name: String, value: String) -> Result<(), OpError> {
    self.set_pair(name, value)
  }

  fn append(&self, name: String, value: String) -> Result<(), OpError> {
    self.append_pair(name, value)
  }

  fn delete(&self, name: String) -> Result<(), OpError> {
    self.delete_name(name)
  }

  #[rename("getSetCookie")]
  fn get_set_cookie(&self, scope: &mut v8::PinScope) -> Js {
    let pairs = self.pairs.borrow();
    let values = pairs
      .iter()
      .filter(|(name, _)| name == "set-cookie")
      .map(|(_, value)| value.as_str())
      .collect::<Vec<_>>();
    let array = v8::Array::new(scope, values.len() as i32);
    for (index, value) in values.iter().enumerate() {
      let text = v8::String::new(scope, value).unwrap();
      array.set_index(scope, index as u32, text.into());
    }
    let value: v8::Local<v8::Value> = array.into();
    Js(v8::Global::new(scope, value))
  }

  fn for_each(
    &self,
    scope: &mut v8::PinScope,
    #[this] object: v8::Local<v8::Object>,
    callback: v8::Local<v8::Value>,
    this_arg: Option<v8::Local<v8::Value>>,
  ) {
    let Ok(callback) = v8::Local::<v8::Function>::try_from(callback) else {
      yuzuko_core::op::OpException::throw(
        OpError::type_("Headers.forEach requires a function"),
        scope,
      );
      return;
    };
    let receiver = this_arg.unwrap_or_else(|| v8::undefined(scope).into());
    let pairs = sorted_combined(&self.pairs.borrow());
    for (name, value) in pairs {
      let Some(name) = v8::String::new(scope, &name) else {
        return;
      };
      let Some(value) = v8::String::new(scope, &value) else {
        return;
      };
      if callback
        .call(scope, receiver, &[value.into(), name.into(), object.into()])
        .is_none()
      {
        return;
      }
    }
  }

  fn entries(
    &self,
    scope: &mut v8::PinScope,
    #[this] object: v8::Local<v8::Object>,
  ) -> Js {
    header_iterator(scope, object, HeaderIterKind::Entries)
  }

  fn keys(
    &self,
    scope: &mut v8::PinScope,
    #[this] object: v8::Local<v8::Object>,
  ) -> Js {
    header_iterator(scope, object, HeaderIterKind::Keys)
  }

  fn values(
    &self,
    scope: &mut v8::PinScope,
    #[this] object: v8::Local<v8::Object>,
  ) -> Js {
    header_iterator(scope, object, HeaderIterKind::Values)
  }
}

enum JsPending {
  Null,
  Text(String),
}

#[derive(Clone, Copy)]
enum HeaderIterKind {
  Entries,
  Keys,
  Values,
}

struct HeaderIter {
  headers: v8::TracedReference<v8::Object>,
  index: Cell<usize>,
  kind: HeaderIterKind,
}

unsafe impl v8::cppgc::GarbageCollected for HeaderIter {
  fn trace(&self, visitor: &mut v8::cppgc::Visitor) {
    visitor.trace(&self.headers);
  }

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"Headers Iterator"
  }
}

#[op(name = "Headers Iterator")]
impl HeaderIter {
  fn next(&self, scope: &mut v8::PinScope) -> Result<Js, OpError> {
    let object = traced_local(scope, &self.headers)
      .ok_or_else(|| OpError::type_("illegal invocation"))?;
    let host = unsafe { yuzuko_core::op::host_ref::<Headers>(scope, object) }
      .ok_or_else(|| OpError::type_("illegal invocation"))?;
    // The index walks the sorted list as it exists on each step. A deletion
    // or insertion changes later slots, which is the iteration the tests lock.
    let pairs = sorted_combined(&host.get().pairs.borrow());
    let index = self.index.get();
    if index >= pairs.len() {
      return Ok(header_iterator_result(
        scope,
        true,
        v8::undefined(scope).into(),
      ));
    }
    self.index.set(index + 1);
    let (name, value) = &pairs[index];
    let Some(name) = v8::String::new(scope, name) else {
      return Ok(header_iterator_result(
        scope,
        true,
        v8::undefined(scope).into(),
      ));
    };
    let Some(value) = v8::String::new(scope, value) else {
      return Ok(header_iterator_result(
        scope,
        true,
        v8::undefined(scope).into(),
      ));
    };
    let item = match self.kind {
      HeaderIterKind::Entries => {
        let array = v8::Array::new(scope, 2);
        array.set_index(scope, 0, name.into());
        array.set_index(scope, 1, value.into());
        array.into()
      }
      HeaderIterKind::Keys => name.into(),
      HeaderIterKind::Values => value.into(),
    };
    Ok(header_iterator_result(scope, false, item))
  }
}

fn sorted_combined(pairs: &[(String, String)]) -> Vec<(String, String)> {
  let mut names = pairs
    .iter()
    .map(|(name, _)| name.as_str())
    .collect::<Vec<_>>();
  names.sort_unstable();
  names.dedup();
  let mut combined = Vec::new();
  for name in names {
    if name == "set-cookie" {
      for (key, value) in pairs {
        if key == name {
          combined.push((key.clone(), value.clone()));
        }
      }
      continue;
    }
    let value = pairs
      .iter()
      .filter(|(key, _)| key == name)
      .map(|(_, value)| value.as_str())
      .collect::<Vec<_>>()
      .join(", ");
    combined.push((name.to_string(), value));
  }
  combined
}

fn header_iterator(
  scope: &mut v8::PinScope,
  headers: v8::Local<v8::Object>,
  kind: HeaderIterKind,
) -> Js {
  let _ = HeaderIter::template(scope);
  let headers = traced(scope, headers);
  let iter = unsafe {
    yuzuko_core::op::wrap_instance(
      scope,
      HeaderIter {
        headers,
        index: Cell::new(0),
        kind,
      },
    )
  };
  let symbol = v8::Symbol::get_iterator(scope);
  let function = v8::Function::new(scope, iterate_header_self).unwrap();
  iter.set(scope, symbol.into(), function.into());
  let value: v8::Local<v8::Value> = iter.into();
  Js(v8::Global::new(scope, value))
}

fn header_iterator_result(
  scope: &mut v8::PinScope,
  done: bool,
  value: v8::Local<v8::Value>,
) -> Js {
  let object = v8::Object::new(scope);
  let done = v8::Boolean::new(scope, done);
  object.set(
    scope,
    v8::String::new(scope, "done").unwrap().into(),
    done.into(),
  );
  object.set(
    scope,
    v8::String::new(scope, "value").unwrap().into(),
    value,
  );
  let value: v8::Local<v8::Value> = object.into();
  Js(v8::Global::new(scope, value))
}

fn install_headers_iterator(scope: &mut v8::PinScope) {
  let constructor = Headers::template(scope)
    .get_function(scope)
    .expect("Headers");
  let Some(prototype) = constructor
    .get(scope, v8::String::new(scope, "prototype").unwrap().into())
    .and_then(|value| v8::Local::<v8::Object>::try_from(value).ok())
  else {
    return;
  };
  let entries_key = v8::String::new(scope, "entries").unwrap();
  let Some(entries) = prototype.get(scope, entries_key.into()) else {
    return;
  };
  let _ = prototype.define_own_property(
    scope,
    v8::Symbol::get_iterator(scope).into(),
    entries,
    v8::PropertyAttribute::DONT_ENUM,
  );
  install_header_iter_prototype(scope);
}

fn install_header_iter_prototype(scope: &mut v8::PinScope) {
  let Some(constructor) = HeaderIter::template(scope).get_function(scope)
  else {
    return;
  };
  let key = v8::String::new(scope, "prototype").unwrap();
  let Some(prototype) = constructor
    .get(scope, key.into())
    .and_then(|value| v8::Local::<v8::Object>::try_from(value).ok())
  else {
    return;
  };
  let Some(intrinsic) = intrinsic_iterator_prototype(scope) else {
    return;
  };
  let _ = prototype.set_prototype(scope, intrinsic.into());
}

fn intrinsic_iterator_prototype<'s>(
  scope: &mut v8::PinScope<'s, '_>,
) -> Option<v8::Local<'s, v8::Object>> {
  let array = v8::Array::new(scope, 0);
  let symbol = v8::Symbol::get_iterator(scope);
  let function = array.get(scope, symbol.into())?;
  let function = v8::Local::<v8::Function>::try_from(function).ok()?;
  let iterator = function.call(scope, array.into(), &[])?;
  let iterator = iterator.to_object(scope)?;
  let proto = iterator.get_prototype(scope)?;
  let proto = v8::Local::<v8::Object>::try_from(proto).ok()?;
  let intrinsic = proto.get_prototype(scope)?;
  v8::Local::<v8::Object>::try_from(intrinsic).ok()
}

fn iterate_header_self(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut ret: v8::ReturnValue,
) {
  let object = args.this();
  if unsafe { yuzuko_core::op::host_ref::<HeaderIter>(scope, object) }.is_none()
  {
    yuzuko_core::op::OpException::throw(
      OpError::type_("Illegal invocation"),
      scope,
    );
    return;
  }
  ret.set(object.into());
}

impl yuzuko_core::op::ToV8 for JsPending {
  fn to_v8(self, scope: &mut v8::PinScope, ret: &mut v8::ReturnValue) {
    match self {
      Self::Null => ret.set(v8::null(scope).into()),
      Self::Text(text) => {
        let text = v8::String::new(scope, &text).unwrap();
        ret.set(text.into());
      }
    }
  }
}

pub struct Response {
  status: u16,
  status_text: String,
  url: String,
  mime: String,
  redirected: bool,
  kind: &'static str,
  body: RefCell<Option<crate::blob::BlobSource>>,
  stream: RefCell<Option<v8::TracedReference<v8::Object>>>,
  stream_id: Cell<u64>,
  consumed: Cell<bool>,
  headers: RefCell<Option<v8::TracedReference<v8::Object>>>,
}

unsafe impl v8::cppgc::GarbageCollected for Response {
  fn trace(&self, visitor: &mut v8::cppgc::Visitor) {
    if let Ok(headers) = self.headers.try_borrow() {
      visitor.trace(&*headers);
    }
    if let Ok(stream) = self.stream.try_borrow() {
      visitor.trace(&*stream);
    }
  }

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"Response"
  }
}

impl Response {
  fn from_loaded(
    scope: &mut v8::PinScope,
    loaded: Loaded,
  ) -> Result<Response, OpError> {
    let mut pairs = Vec::new();
    if !loaded.mime.is_empty() || loaded.force_type {
      pairs.push(("content-type".to_string(), loaded.mime.clone()));
    }
    if let Some(length) = loaded.content_length {
      pairs.push(("content-length".to_string(), length.to_string()));
    }
    if let Some(range) = loaded.content_range {
      pairs.push(("content-range".to_string(), range));
    }
    let headers = unsafe {
      yuzuko_core::op::wrap_instance(
        scope,
        Headers::from_pairs(pairs, HeaderGuard::Immutable)?,
      )
    };
    Ok(Response {
      status: loaded.status,
      status_text: loaded.status_text.to_string(),
      url: loaded.url,
      mime: loaded.mime,
      redirected: false,
      kind: "default",
      body: RefCell::new(Some(loaded.source)),
      stream: RefCell::new(None),
      stream_id: Cell::new(0),
      consumed: Cell::new(false),
      headers: RefCell::new(Some(traced(scope, headers))),
    })
  }

  fn from_http(
    scope: &mut v8::PinScope,
    response: crate::http::HttpResponse,
    abort_id: Option<u64>,
  ) -> Result<Response, OpError> {
    let mime = response
      .headers
      .iter()
      .find(|(name, _)| name == "content-type")
      .map(|(_, value)| value.clone())
      .unwrap_or_default();
    let headers = unsafe {
      yuzuko_core::op::wrap_instance(
        scope,
        Headers::from_pairs(response.headers, HeaderGuard::Immutable)?,
      )
    };
    let null_body = response.null_body;
    if null_body {
      response.body.stop();
      return Ok(Response {
        status: response.status,
        status_text: response.status_text,
        url: response.url,
        mime,
        redirected: response.redirected,
        kind: "default",
        body: RefCell::new(None),
        stream: RefCell::new(None),
        stream_id: Cell::new(0),
        consumed: Cell::new(false),
        headers: RefCell::new(Some(traced(scope, headers))),
      });
    }
    let stream = crate::stream::open_byte_stream(scope);
    let stream_id = crate::stream::byte_stream_id(scope, &stream);
    if let Some(abort_id) = abort_id {
      bind_abort_stream(abort_id, stream_id);
    }
    arm_http_body(stream_id, response.body);
    let response = Response {
      status: response.status,
      status_text: response.status_text,
      url: response.url,
      mime,
      redirected: response.redirected,
      kind: "default",
      body: RefCell::new(None),
      stream: RefCell::new(None),
      stream_id: Cell::new(0),
      consumed: Cell::new(false),
      headers: RefCell::new(Some(traced(scope, headers))),
    };
    response.cache_stream(scope, stream);
    Ok(response)
  }

  fn clone_response(
    &self,
    scope: &mut v8::PinScope,
  ) -> Result<Response, OpError> {
    if self.consumed.get() || crate::stream::is_disturbed(self.stream_id.get())
    {
      return Err(OpError::type_("Body has already been used"));
    }
    self.ensure_stream(scope)?;
    let (left, right) = crate::stream::tee_pair(scope, self.stream_id.get())?;
    self.cache_stream(scope, left);
    let pairs = self.header_pairs(scope);
    let headers = unsafe {
      yuzuko_core::op::wrap_instance(
        scope,
        Headers::from_pairs(pairs, HeaderGuard::Immutable)?,
      )
    };
    let cloned = Response {
      status: self.status,
      status_text: self.status_text.clone(),
      url: self.url.clone(),
      mime: self.mime.clone(),
      redirected: self.redirected,
      kind: self.kind,
      body: RefCell::new(None),
      stream: RefCell::new(None),
      stream_id: Cell::new(0),
      consumed: Cell::new(false),
      headers: RefCell::new(Some(traced(scope, headers))),
    };
    cloned.cache_stream(scope, right);
    Ok(cloned)
  }

  fn ensure_stream(&self, scope: &mut v8::PinScope) -> Result<(), OpError> {
    if self.stream_id.get() != 0 {
      return Ok(());
    }
    let object = match self.body.borrow_mut().take() {
      Some(source) => crate::stream::stream_from_source(scope, source),
      None => {
        let object = crate::stream::open_byte_stream(scope);
        let id = crate::stream::byte_stream_id(scope, &object);
        crate::stream::finish_byte_stream(scope, id);
        object
      }
    };
    self.cache_stream(scope, object);
    Ok(())
  }

  fn header_pairs(&self, scope: &mut v8::PinScope) -> Vec<(String, String)> {
    let headers = self.headers.borrow();
    let Some(stored) = headers.as_ref() else {
      return Vec::new();
    };
    let Some(object) = traced_local(scope, stored) else {
      return Vec::new();
    };
    unsafe { yuzuko_core::op::host_ref::<Headers>(scope, object) }
      .map(|host| host.get().snapshot())
      .unwrap_or_default()
  }

  fn take_body(&self, scope: &mut v8::PinScope) -> Result<StreamBody, OpError> {
    let mut stream_id = self.stream_id.get();
    if let Some(error) = crate::stream::stream_error(stream_id) {
      return Err(OpError::Thrown(Box::new(error)));
    }
    if stream_id == 0 {
      let Some(source) = self.body.borrow_mut().take() else {
        if self.consumed.get() {
          return Err(OpError::type_("Body has already been used"));
        }
        // A null body reads as empty and stays undisturbed.
        return Ok(StreamBody::Ready(Vec::new()));
      };
      let object = crate::stream::stream_from_source(scope, source);
      self.cache_stream(scope, object);
      stream_id = self.stream_id.get();
    }
    if let Some(error) = crate::stream::stream_error(stream_id) {
      return Err(OpError::Thrown(Box::new(error)));
    }
    if self.consumed.get() || crate::stream::is_disturbed(stream_id) {
      return Err(OpError::type_("Body has already been used"));
    }
    let body = crate::stream::read_stream_bytes(scope, stream_id)?;
    self.consumed.set(true);
    Ok(body)
  }

  fn consume_as(
    &self,
    scope: &mut v8::PinScope,
    finish: impl FnOnce(&mut v8::PinScope, Vec<u8>, v8::Local<v8::PromiseResolver>)
    + 'static,
  ) -> ResponsePromise {
    match self.take_body(scope) {
      Err(error) => reject_consumed(scope, error),
      Ok(StreamBody::Ready(bytes)) => {
        settle_later(scope, move |scope, resolver| {
          finish(scope, bytes, resolver);
        })
      }
      Ok(StreamBody::Pending(promise)) => follow_body(scope, promise, finish),
    }
  }

  fn cache_stream(
    &self,
    scope: &mut v8::PinScope,
    object: v8::Global<v8::Object>,
  ) {
    let id = crate::stream::stream_id_of(scope, &object).unwrap_or(0);
    let local = v8::Local::new(scope, &object);
    *self.stream.borrow_mut() = Some(traced(scope, local));
    self.stream_id.set(id);
  }
}

struct ResponsePromise(v8::Global<v8::Promise>);

impl yuzuko_core::op::ToV8 for ResponsePromise {
  fn to_v8(self, scope: &mut v8::PinScope, ret: &mut v8::ReturnValue) {
    ret.set(v8::Local::new(scope, self.0).into());
  }
}

#[op]
impl Response {
  #[constructor]
  fn new(
    scope: &mut v8::PinScope,
    body: Option<v8::Local<v8::Value>>,
    init: Option<v8::Local<v8::Value>>,
  ) -> Result<Response, OpError> {
    let (status, status_text, mut pairs) = response_init(scope, init)?;
    let extracted = extract_body(scope, body)?;
    if extracted.is_some() && matches!(status, 101 | 204 | 205 | 304) {
      return Err(OpError::type_(
        "Response with a null body status cannot have a body",
      ));
    }
    let (bytes, default_type, stream) = match extracted {
      Some(Extracted::Bytes(bytes, default_type)) => {
        (Some(bytes), default_type, None)
      }
      Some(Extracted::Stream(object, default_type)) => {
        (None, default_type, Some(object))
      }
      None => (None, None, None),
    };
    if let Some(default_type) = default_type
      && !pairs
        .iter()
        .any(|(name, _)| name.eq_ignore_ascii_case("content-type"))
    {
      pairs.push(("content-type".to_string(), default_type));
    }
    let mime = pairs
      .iter()
      .find(|(name, _)| name == "content-type")
      .map(|(_, value)| value.clone())
      .unwrap_or_default();
    let headers = unsafe {
      yuzuko_core::op::wrap_instance(
        scope,
        Headers::from_pairs(pairs, HeaderGuard::Response)?,
      )
    };
    let response = Response {
      status,
      status_text,
      url: String::new(),
      mime,
      redirected: false,
      kind: "default",
      body: RefCell::new(bytes.map(crate::blob::BlobSource::from_vec)),
      stream: RefCell::new(None),
      stream_id: Cell::new(0),
      consumed: Cell::new(false),
      headers: RefCell::new(Some(traced(scope, headers))),
    };
    if let Some(stream) = stream {
      response.cache_stream(scope, stream);
    }
    Ok(response)
  }

  #[getter]
  fn status(&self) -> f64 {
    f64::from(self.status)
  }

  #[getter]
  fn status_text(&self) -> String {
    self.status_text.clone()
  }

  #[getter]
  fn ok(&self) -> bool {
    (200..300).contains(&self.status)
  }

  #[getter]
  fn url(&self) -> String {
    self.url.clone()
  }

  #[getter]
  fn redirected(&self) -> bool {
    self.redirected
  }

  #[getter]
  #[rename("type")]
  fn type_(&self) -> String {
    self.kind.to_string()
  }

  fn clone(&self, scope: &mut v8::PinScope) -> Result<Js, OpError> {
    let cloned = self.clone_response(scope)?;
    let object = unsafe { yuzuko_core::op::wrap_instance(scope, cloned) };
    let value: v8::Local<v8::Value> = object.into();
    Ok(Js(v8::Global::new(scope, value)))
  }

  #[getter]
  fn body_used(&self) -> bool {
    self.consumed.get() || crate::stream::is_disturbed(self.stream_id.get())
  }

  #[getter]
  fn body(&self, scope: &mut v8::PinScope) -> Js {
    if let Some(existing) = self.stream.borrow().as_ref()
      && let Some(object) = traced_local(scope, existing)
    {
      let value: v8::Local<v8::Value> = object.into();
      return Js(v8::Global::new(scope, value));
    }
    let Some(source) = self.body.borrow_mut().take() else {
      let value: v8::Local<v8::Value> = v8::null(scope).into();
      return Js(v8::Global::new(scope, value));
    };
    let object = crate::stream::stream_from_source(scope, source);
    self.cache_stream(scope, object.clone());
    let local = v8::Local::new(scope, &object);
    let value: v8::Local<v8::Value> = local.into();
    Js(v8::Global::new(scope, value))
  }

  #[getter]
  fn headers(&self, scope: &mut v8::PinScope) -> Js {
    let value = self
      .headers
      .borrow()
      .as_ref()
      .and_then(|headers| traced_local(scope, headers))
      .map(v8::Local::<v8::Value>::from)
      .unwrap_or_else(|| v8::undefined(scope).into());
    Js(v8::Global::new(scope, value))
  }

  #[promise]
  fn blob(&self, scope: &mut v8::PinScope) -> ResponsePromise {
    let mime = header_list_value(&self.header_pairs(scope), "content-type");
    self.consume_as(scope, move |scope, bytes, resolver| {
      let blob = unsafe {
        yuzuko_core::op::wrap_instance(scope, Blob::from_parts(bytes, mime))
      };
      resolver.resolve(scope, blob.into());
    })
  }

  #[promise]
  fn text(&self, scope: &mut v8::PinScope) -> ResponsePromise {
    self.consume_as(scope, |scope, bytes, resolver| {
      let text = crate::blob::decode_utf8(&bytes);
      let text = v8::String::new(scope, &text).unwrap();
      resolver.resolve(scope, text.into());
    })
  }

  #[promise]
  fn array_buffer(&self, scope: &mut v8::PinScope) -> ResponsePromise {
    self.consume_as(scope, |scope, bytes, resolver| {
      let buffer = bytes::array_buffer(scope, bytes);
      resolver.resolve(scope, buffer.into());
    })
  }

  #[promise]
  fn bytes(&self, scope: &mut v8::PinScope) -> ResponsePromise {
    self.consume_as(scope, |scope, bytes, resolver| {
      let view = bytes::uint8_array(scope, bytes);
      resolver.resolve(scope, view.into());
    })
  }

  #[promise]
  fn json(&self, scope: &mut v8::PinScope) -> ResponsePromise {
    self.consume_as(scope, |scope, bytes, resolver| {
      let text = crate::blob::decode_utf8(&bytes);
      let Some(source) = v8::String::new(scope, &text) else {
        reject_type(scope, resolver, "JSON text is too large");
        return;
      };
      match v8::json::parse(scope, source) {
        Some(value) => {
          resolver.resolve(scope, value);
        }
        None => reject_syntax(scope, resolver),
      }
    })
  }

  #[promise]
  fn form_data(&self, scope: &mut v8::PinScope) -> ResponsePromise {
    let mime = header_list_value(&self.header_pairs(scope), "content-type");
    let null_body = self.stream_id.get() == 0
      && self.stream.borrow().is_none()
      && self.body.borrow().is_none();
    if null_body && mime.to_ascii_lowercase().starts_with("multipart/form-data")
    {
      let resolver = v8::PromiseResolver::new(scope).unwrap();
      let promise = resolver.get_promise(scope);
      reject_type(scope, resolver, "The multipart body is incomplete");
      return ResponsePromise(v8::Global::new(scope, promise));
    }
    self.consume_as(scope, move |scope, bytes, resolver| {
      match crate::http::form::parse(&mime, &bytes) {
        Ok(form) => {
          let object = unsafe { yuzuko_core::op::wrap_instance(scope, form) };
          resolver.resolve(scope, object.into());
        }
        Err(error) => reject_op(scope, resolver, error),
      }
    })
  }

  #[rename("textStream")]
  fn text_stream(&self, scope: &mut v8::PinScope) -> Result<Js, OpError> {
    let used =
      self.consumed.get() || crate::stream::is_disturbed(self.stream_id.get());
    let null_body = self.stream.borrow().is_none()
      && self.body.borrow().is_none()
      && self.stream_id.get() == 0;
    let body = if null_body {
      Js::null(scope)
    } else {
      self.body_get_impl(scope)
    };
    text_stream_of(scope, used, null_body, body)
  }

  #[static_method]
  fn error(scope: &mut v8::PinScope) -> Result<New<Response>, OpError> {
    Ok(New(response_from_parts(
      scope,
      0,
      String::new(),
      Vec::new(),
      None,
      "error",
      HeaderGuard::Immutable,
    )?))
  }

  #[static_method]
  fn redirect(
    scope: &mut v8::PinScope,
    url: String,
    status: Option<f64>,
  ) -> Result<New<Response>, OpError> {
    let status = status.unwrap_or(302.0);
    if !status.is_finite() || status.fract() != 0.0 {
      return Err(OpError::range("The redirect status is invalid"));
    }
    let code = status as u16;
    if !matches!(code, 301 | 302 | 303 | 307 | 308) {
      return Err(OpError::range("The redirect status is invalid"));
    }
    let location = resolve_fetch_url(&url)?.to_string();
    Ok(New(response_from_parts(
      scope,
      code,
      String::new(),
      vec![("location".to_string(), location)],
      None,
      "default",
      HeaderGuard::Immutable,
    )?))
  }

  #[static_method]
  #[rename("json")]
  fn json_static(
    scope: &mut v8::PinScope,
    data: v8::Local<v8::Value>,
    init: Option<v8::Local<v8::Value>>,
  ) -> Result<New<Response>, OpError> {
    let bytes = json_bytes(scope, data)?;
    let (status, status_text, mut pairs) = response_init(scope, init)?;
    if matches!(status, 101 | 204 | 205 | 304) {
      return Err(OpError::type_(
        "Response with a null body status cannot have a body",
      ));
    }
    if !pairs
      .iter()
      .any(|(name, _)| name.eq_ignore_ascii_case("content-type"))
    {
      pairs.push(("content-type".to_string(), "application/json".to_string()));
    }
    Ok(New(response_from_parts(
      scope,
      status,
      status_text,
      pairs,
      Some(bytes),
      "default",
      HeaderGuard::Response,
    )?))
  }
}

fn json_bytes(
  scope: &mut v8::PinScope,
  data: v8::Local<v8::Value>,
) -> Result<Vec<u8>, OpError> {
  let global = scope.get_current_context().global(scope);
  let json_key = v8::String::new(scope, "JSON").unwrap();
  let stringify_key = v8::String::new(scope, "stringify").unwrap();
  let json = global
    .get(scope, json_key.into())
    .and_then(|value| v8::Local::<v8::Object>::try_from(value).ok())
    .ok_or_else(|| OpError::type_("JSON.stringify is unavailable"))?;
  let stringify = json
    .get(scope, stringify_key.into())
    .and_then(|value| v8::Local::<v8::Function>::try_from(value).ok())
    .ok_or_else(|| OpError::type_("JSON.stringify is unavailable"))?;
  let json = yuzuko_core::op::root(scope, json);
  let stringify = yuzuko_core::op::root(scope, stringify);
  let data = yuzuko_core::op::root(scope, data);
  let captured = {
    v8::tc_scope!(let tc, scope);
    let json = v8::Local::new(tc, &json);
    let stringify = v8::Local::new(tc, &stringify);
    let data = v8::Local::new(tc, &data);
    match stringify.call(tc, json.into(), &[data]) {
      Some(value) => {
        if value.is_undefined() {
          Err(None)
        } else {
          Ok(
            value
              .to_string(tc)
              .map(|text| text.to_rust_string_lossy(tc))
              .unwrap_or_default(),
          )
        }
      }
      None => Err(tc.exception().map(|value| v8::Global::new(tc, value))),
    }
  };
  match captured {
    Ok(text) => Ok(text.into_bytes()),
    Err(Some(thrown)) => Err(OpError::Thrown(Box::new(thrown))),
    Err(None) => {
      Err(OpError::type_("Response.json cannot serialize the value"))
    }
  }
}

fn text_stream_of(
  scope: &mut v8::PinScope,
  used: bool,
  null_body: bool,
  body: Js,
) -> Result<Js, OpError> {
  if used {
    return Err(OpError::type_("Body has already been used"));
  }
  if null_body {
    let stream = crate::stream::stream_from_bytes(scope, Vec::new());
    let local = v8::Local::new(scope, &stream);
    let value: v8::Local<v8::Value> = local.into();
    return Ok(Js(v8::Global::new(scope, value)));
  }
  let local = v8::Local::new(scope, &body.0);
  let object = v8::Local::<v8::Object>::try_from(local)
    .map_err(|_| OpError::type_("The body is not a stream"))?;
  let decoded = crate::stream::text_stream_from_body(scope, object)?;
  let local = v8::Local::new(scope, &decoded);
  let value: v8::Local<v8::Value> = local.into();
  Ok(Js(v8::Global::new(scope, value)))
}

fn response_from_parts(
  scope: &mut v8::PinScope,
  status: u16,
  status_text: String,
  pairs: Vec<(String, String)>,
  bytes: Option<Vec<u8>>,
  kind: &'static str,
  guard: HeaderGuard,
) -> Result<Response, OpError> {
  let mime = pairs
    .iter()
    .find(|(name, _)| name == "content-type")
    .map(|(_, value)| value.clone())
    .unwrap_or_default();
  let headers = unsafe {
    yuzuko_core::op::wrap_instance(scope, Headers::from_pairs(pairs, guard)?)
  };
  Ok(Response {
    status,
    status_text,
    url: String::new(),
    mime,
    redirected: false,
    kind,
    body: RefCell::new(bytes.map(crate::blob::BlobSource::from_vec)),
    stream: RefCell::new(None),
    stream_id: Cell::new(0),
    consumed: Cell::new(false),
    headers: RefCell::new(Some(traced(scope, headers))),
  })
}

fn follow_body(
  scope: &mut v8::PinScope,
  promise: v8::Global<v8::Promise>,
  finish: impl FnOnce(&mut v8::PinScope, Vec<u8>, v8::Local<v8::PromiseResolver>)
  + 'static,
) -> ResponsePromise {
  let resolver = v8::PromiseResolver::new(scope).unwrap();
  let outer = resolver.get_promise(scope);
  let stored = v8::Global::new(scope, resolver);
  crate::stream::when_stream_bytes(scope, promise, move |scope, result| {
    let resolver = v8::Local::new(scope, &stored);
    let resolver = yuzuko_core::op::root(scope, resolver);
    match result {
      Ok(bytes) => finish(scope, bytes, resolver),
      Err(error) => {
        let error = v8::Local::new(scope, &error);
        resolver.reject(scope, error);
      }
    }
  });
  ResponsePromise(v8::Global::new(scope, outer))
}

pub fn install(scope: &mut v8::PinScope) {
  Headers::install(scope);
  install_headers_iterator(scope);
  Response::install(scope);
  crate::http::request::install(scope);
  let global = scope.get_current_context().global(scope);
  let name = v8::String::new(scope, "fetch").unwrap();
  let function = v8::Function::builder(fetch)
    .length(1)
    .constructor_behavior(v8::ConstructorBehavior::Throw)
    .build(scope)
    .unwrap();
  function.set_name(name);
  global.set(scope, name.into(), function.into());
}

fn fetch(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut ret: v8::ReturnValue,
) {
  let global = scope.get_current_context().global(scope);
  let this = args.this();
  if !this.is_null_or_undefined() && !this.strict_equals(global.into()) {
    ret.set(rejected(
      scope,
      Reject::Type("illegal invocation".to_string()),
    ));
    return;
  }
  ret.set(start_fetch(scope, &args));
}

fn start_fetch<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  args: &v8::FunctionCallbackArguments,
) -> v8::Local<'s, v8::Value> {
  if args.length() < 1 {
    return rejected(scope, Reject::Type("fetch requires a URL".to_string()));
  }
  let init = if args.length() > 1 {
    Some(args.get(1))
  } else {
    None
  };
  if crate::http::request::is_request(scope, args.get(0)) {
    let request =
      match crate::http::request::prepare_fetch(scope, args.get(0), init) {
        Ok(request) => request,
        Err(error) => return rejected(scope, op_to_reject(error)),
      };
    return dispatch_prepared(scope, request);
  }
  let input = match to_string(scope, args.get(0)) {
    Ok(input) => input,
    Err(error) => return rejected(scope, error),
  };
  let url = match resolve_url(&input) {
    Ok(url) => url,
    Err(error) => return rejected(scope, error),
  };
  match url.scheme() {
    "http" | "https" => spawn_http(scope, url, args),
    "file" => spawn_file(scope, url),
    "blob" => {
      let headers = match blob_init_headers(scope, args) {
        Ok(headers) => headers,
        Err(error) => return rejected(scope, error),
      };
      load_blob(scope, &url, &headers)
    }
    "data" => spawn_data(scope, url),
    // `about:` is a network error for fetch. The fetch tests expect TypeError.
    "about" => rejected(
      scope,
      Reject::Type("The URL scheme is not supported".to_string()),
    ),
    _ => unsupported_scheme(scope),
  }
}

fn blob_init_headers(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
) -> Result<Vec<(String, String)>, Reject> {
  if args.length() < 2 {
    return Ok(Vec::new());
  }
  let init = args.get(1);
  if init.is_null_or_undefined() {
    return Ok(Vec::new());
  }
  let Ok(object) = v8::Local::<v8::Object>::try_from(init) else {
    return Ok(Vec::new());
  };
  let Some(headers) = http_property(scope, object, "headers")? else {
    return Ok(Vec::new());
  };
  read_header_init(scope, Some(headers)).map_err(|error| match error {
    OpError::Type(message) | OpError::Range(message) => Reject::Type(message),
    OpError::Dom { message, .. } => Reject::Type(message),
    OpError::Thrown(_) => Reject::Type("The headers are invalid".to_string()),
  })
}

fn unsupported_scheme<'s>(
  scope: &mut v8::PinScope<'s, '_>,
) -> v8::Local<'s, v8::Value> {
  rejected(
    scope,
    Reject::Dom(
      "NotSupportedError".to_string(),
      "The URL scheme is not supported".to_string(),
    ),
  )
}

fn op_to_reject(error: OpError) -> Reject {
  match error {
    OpError::Type(message) | OpError::Range(message) => Reject::Type(message),
    OpError::Dom { name, message } => Reject::Dom(name, message),
    OpError::Thrown(value) => Reject::Thrown(*value),
  }
}

pub(crate) fn resolve_fetch_url(input: &str) -> Result<url::Url, OpError> {
  resolve_url(input).map_err(|error| match error {
    Reject::Type(message) => OpError::type_(message),
    Reject::Dom(name, message) => OpError::dom(name, message),
    Reject::Thrown(_) => OpError::type_("The URL is invalid"),
  })
}

fn resolve_url(input: &str) -> Result<url::Url, Reject> {
  if let Ok(url) = url::Url::parse(input) {
    if !url.username().is_empty() || url.password().is_some() {
      return Err(Reject::Type("The URL contains credentials".to_string()));
    }
    return Ok(url);
  }
  let Some(base) = yuzuko_core::entry_url() else {
    return Err(Reject::Type("fetch has no entry URL".to_string()));
  };
  let base = url::Url::parse(&base)
    .map_err(|_| Reject::Type("fetch has no entry URL".to_string()))?;
  base
    .join(input)
    .map_err(|_| Reject::Type("The URL is invalid".to_string()))
}

fn spawn_http<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  url: url::Url,
  args: &v8::FunctionCallbackArguments,
) -> v8::Local<'s, v8::Value> {
  let init = if args.length() > 1 {
    Some(args.get(1))
  } else {
    None
  };
  let request = match read_http_init(scope, &url, init) {
    Ok(request) => request,
    Err(error) => return rejected(scope, error),
  };
  if request.cancel.load(Ordering::Relaxed) {
    return rejected(
      scope,
      request.abort_reason.unwrap_or_else(|| {
        Reject::Dom(
          "AbortError".to_string(),
          "The operation was aborted".to_string(),
        )
      }),
    );
  }
  let cancel = Arc::clone(&request.cancel);
  let abort_id = request.abort_id;
  promise_http(scope, abort_id, Arc::clone(&cancel), move || {
    crate::http::perform(
      request.method,
      request.url,
      request.headers,
      request.body,
      request.redirect,
      cancel,
    )
  })
}

struct HttpInit {
  method: String,
  url: String,
  headers: Vec<(String, String)>,
  body: Vec<u8>,
  redirect: String,
  cancel: Arc<AtomicBool>,
  abort_reason: Option<Reject>,
  abort_id: Option<u64>,
  request_stream: Option<v8::Global<v8::Object>>,
}

fn read_http_init(
  scope: &mut v8::PinScope,
  url: &url::Url,
  init: Option<v8::Local<v8::Value>>,
) -> Result<HttpInit, Reject> {
  let cancel = Arc::new(AtomicBool::new(false));
  let mut request = HttpInit {
    method: "GET".to_string(),
    url: url.to_string(),
    headers: Vec::new(),
    body: Vec::new(),
    redirect: "follow".to_string(),
    cancel: cancel.clone(),
    abort_reason: None,
    abort_id: None,
    request_stream: None,
  };
  let Some(init) = init.filter(|value| !value.is_null_or_undefined()) else {
    return Ok(request);
  };
  let Ok(object) = v8::Local::<v8::Object>::try_from(init) else {
    return Err(Reject::Type("Request init is not an object".to_string()));
  };
  if let Some(window) = http_property(scope, object, "window")?
    && !window.is_null()
  {
    return Err(Reject::Type("Window can only be null".to_string()));
  }
  if let Some(priority) = http_property(scope, object, "priority")? {
    crate::http::request::validate_priority(&js_string(scope, priority)?)
      .map_err(op_to_reject)?;
  }
  if let Some(method) = http_property(scope, object, "method")? {
    request.method =
      crate::http::request::normalize_method(&js_string(scope, method)?)
        .map_err(op_to_reject)?;
  }
  if let Some(redirect) = http_property(scope, object, "redirect")? {
    request.redirect = js_string(scope, redirect)?;
  }
  let cache = match http_property(scope, object, "cache")? {
    Some(value) => js_string(scope, value)?,
    None => "no-store".to_string(),
  };
  crate::http::request::validate_enums(&cache, &request.redirect)
    .map_err(op_to_reject)?;
  if let Some(headers) = http_property(scope, object, "headers")? {
    request.headers =
      read_header_init(scope, Some(headers)).map_err(|error| {
        Reject::Type(match error {
          OpError::Type(message) | OpError::Range(message) => message,
          OpError::Dom { message, .. } => message,
          OpError::Thrown(_) => "The headers are invalid".to_string(),
        })
      })?;
  }
  let mut saw_body = false;
  if let Some(body) = http_property(scope, object, "body")? {
    saw_body = true;
    match extract_body(scope, Some(body)).map_err(|error| {
      Reject::Type(match error {
        OpError::Type(message) | OpError::Range(message) => message,
        OpError::Dom { message, .. } => message,
        OpError::Thrown(_) => "The body is invalid".to_string(),
      })
    })? {
      Some(Extracted::Bytes(bytes, _)) => request.body = bytes,
      Some(Extracted::Stream(stream, _)) => {
        request.request_stream = Some(stream);
      }
      None => {}
    }
  }
  if saw_body && matches!(request.method.as_str(), "GET" | "HEAD") {
    return Err(Reject::Type(
      "A GET or HEAD request cannot have a body".to_string(),
    ));
  }
  crate::http::request::ensure_default_headers(&mut request.headers);
  if let Some(signal) = http_property(scope, object, "signal")? {
    watch_abort(scope, signal, &mut request)?;
  }
  if request.cancel.load(Ordering::Relaxed) {
    if let Some(stream) = request.request_stream.take() {
      let reason = abort_reason_value(scope, &request);
      crate::stream::cancel_stream_object(scope, &stream, reason);
    }
    return Ok(request);
  }
  if request.request_stream.take().is_some() {
    return Err(Reject::Type(
      "A streamed request body is not supported".to_string(),
    ));
  }
  let _ = cancel;
  Ok(request)
}

fn watch_abort(
  scope: &mut v8::PinScope,
  signal: v8::Local<v8::Value>,
  request: &mut HttpInit,
) -> Result<(), Reject> {
  let Ok(object) = v8::Local::<v8::Object>::try_from(signal) else {
    return Ok(());
  };
  let aborted = http_property(scope, object, "aborted")?
    .is_some_and(|value| value.boolean_value(scope));
  if aborted {
    request.cancel.store(true, Ordering::Relaxed);
    let reason = http_property(scope, object, "reason")?;
    request.abort_reason = Some(match reason {
      Some(reason) if !reason.is_null_or_undefined() => {
        Reject::Thrown(v8::Global::new(scope, reason))
      }
      _ => default_abort(),
    });
    return Ok(());
  }
  let add = object
    .get(
      scope,
      v8::String::new(scope, "addEventListener").unwrap().into(),
    )
    .and_then(|value| v8::Local::<v8::Function>::try_from(value).ok());
  let Some(add) = add else {
    return Ok(());
  };
  let id = NEXT_ABORT.fetch_add(1, Ordering::Relaxed);
  request.abort_id = Some(id);
  ABORT_FLAGS.with(|slot| {
    slot.borrow_mut().insert(
      id,
      AbortWatch {
        cancel: Arc::clone(&request.cancel),
        stream: Cell::new(0),
        reason: RefCell::new(None),
        signal: v8::Global::new(scope, object),
      },
    );
  });
  let callback = v8::Function::new(scope, on_fetch_abort).unwrap();
  let bind = callback
    .get(scope, v8::String::new(scope, "bind").unwrap().into())
    .and_then(|value| v8::Local::<v8::Function>::try_from(value).ok())
    .unwrap();
  let id_value = v8::Number::new(scope, id as f64);
  let id_value: v8::Local<v8::Value> = id_value.into();
  let bound = bind
    .call(
      scope,
      callback.into(),
      &[v8::undefined(scope).into(), id_value],
    )
    .and_then(|value| v8::Local::<v8::Function>::try_from(value).ok())
    .unwrap();
  let name = v8::String::new(scope, "abort").unwrap();
  let _ = add.call(scope, object.into(), &[name.into(), bound.into()]);
  Ok(())
}

fn on_fetch_abort(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut _ret: v8::ReturnValue,
) {
  let Some(id) = args.get(0).number_value(scope) else {
    return;
  };
  let id = id as u64;
  let found = ABORT_FLAGS.with(|slot| {
    let watches = slot.borrow();
    let watch = watches.get(&id)?;
    watch.cancel.store(true, Ordering::Relaxed);
    Some((watch.stream.get(), watch.signal.clone()))
  });
  let Some((stream, signal)) = found else {
    return;
  };
  let signal = v8::Local::new(scope, &signal);
  let reason = abort_reason_from_signal(scope, signal);
  ABORT_FLAGS.with(|slot| {
    if let Some(watch) = slot.borrow().get(&id) {
      *watch.reason.borrow_mut() = Some(v8::Global::new(scope, reason));
    }
  });
  if stream != 0 {
    crate::stream::error_byte_stream(scope, stream, reason);
  }
}

fn bind_abort_stream(abort_id: u64, stream: u64) {
  ABORT_FLAGS.with(|slot| {
    if let Some(watch) = slot.borrow().get(&abort_id) {
      watch.stream.set(stream);
    }
  });
}

fn stored_abort_reason(abort_id: Option<u64>) -> Reject {
  let reason = abort_id.and_then(|abort_id| {
    ABORT_FLAGS.with(|slot| {
      slot
        .borrow()
        .get(&abort_id)
        .and_then(|watch| watch.reason.borrow().clone())
    })
  });
  match reason {
    Some(reason) => Reject::Thrown(reason),
    None => default_abort(),
  }
}

fn abort_reason_from_signal<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  signal: v8::Local<v8::Object>,
) -> v8::Local<'s, v8::Value> {
  let key = v8::String::new(scope, "reason").unwrap();
  if let Some(reason) = signal.get(scope, key.into())
    && !reason.is_null_or_undefined()
  {
    return reason;
  }
  yuzuko_core::op::new_dom_exception(
    scope,
    "AbortError",
    "The operation was aborted",
  )
  .map(v8::Local::<v8::Value>::from)
  .unwrap_or_else(|| {
    let message = v8::String::new(scope, "The operation was aborted").unwrap();
    v8::Exception::error(scope, message)
  })
}

fn abort_reason_value<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  request: &HttpInit,
) -> v8::Local<'s, v8::Value> {
  if let Some(Reject::Thrown(reason)) = &request.abort_reason {
    return v8::Local::new(scope, reason);
  }
  yuzuko_core::op::new_dom_exception(
    scope,
    "AbortError",
    "The operation was aborted",
  )
  .map(v8::Local::<v8::Value>::from)
  .unwrap_or_else(|| {
    let message = v8::String::new(scope, "The operation was aborted").unwrap();
    v8::Exception::error(scope, message)
  })
}

fn arm_http_body(stream: u64, body: crate::http::HttpBody) {
  let retained = body.clone();
  let listen = yuzuko_core::listen(move |scope| {
    pump_http_body(scope, stream, retained);
  });
  let slot = Arc::new(Mutex::new(Some(listen)));
  let wake = Arc::clone(&slot);
  body.listen(Arc::new(move || {
    if let Some(listen) = wake.lock().expect("http body wake").take() {
      listen.notify();
    }
  }));
}

fn pump_http_body(
  scope: &mut v8::PinScope,
  stream: u64,
  body: crate::http::HttpBody,
) {
  while let Some(note) = body.poll() {
    if !crate::stream::byte_stream_open(stream) {
      body.stop();
      return;
    }
    match note {
      crate::http::BodyNote::Chunk(bytes) => {
        if !crate::stream::push_byte_chunk(scope, stream, bytes) {
          body.stop();
          return;
        }
      }
      crate::http::BodyNote::End => {
        crate::stream::finish_byte_stream(scope, stream);
        return;
      }
      crate::http::BodyNote::Failed(message) => {
        let message = v8::String::new(scope, &message).unwrap();
        let error = v8::Exception::type_error(scope, message);
        crate::stream::error_byte_stream(scope, stream, error);
        return;
      }
    }
  }
  if !crate::stream::byte_stream_open(stream) {
    body.stop();
    return;
  }
  arm_http_body(stream, body);
}

fn http_property<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  object: v8::Local<v8::Object>,
  name: &str,
) -> Result<Option<v8::Local<'s, v8::Value>>, Reject> {
  let key = v8::String::new(scope, name).unwrap();
  match object.get(scope, key.into()) {
    Some(value) if !value.is_undefined() => Ok(Some(value)),
    Some(_) => Ok(None),
    None => Err(Reject::Type("The request init is invalid".to_string())),
  }
}

fn js_string(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<String, Reject> {
  value
    .to_string(scope)
    .map(|text| text.to_rust_string_lossy(scope))
    .ok_or_else(|| Reject::Type("The request init is invalid".to_string()))
}

fn promise_http<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  abort_id: Option<u64>,
  cancel: Arc<AtomicBool>,
  work: impl FnOnce() -> Result<crate::http::HttpResponse, crate::http::HttpFailure>
  + Send
  + 'static,
) -> v8::Local<'s, v8::Value> {
  let resolver = v8::PromiseResolver::new(scope).unwrap();
  let promise = resolver.get_promise(scope);
  let resolver = v8::Global::new(scope, resolver);
  yuzuko_core::spawn(work, move |scope, result| {
    let resolver = v8::Local::new(scope, resolver);
    if cancel.load(Ordering::Relaxed) {
      if let Ok(Ok(response)) = &result {
        response.body.stop();
      }
      reject_value(scope, resolver, stored_abort_reason(abort_id));
      return;
    }
    match result {
      Ok(Ok(response)) => {
        match Response::from_http(scope, response, abort_id) {
          Ok(response) => {
            let object =
              unsafe { yuzuko_core::op::wrap_instance(scope, response) };
            resolver.resolve(scope, object.into());
          }
          Err(error) => reject_op(scope, resolver, error),
        }
      }
      Ok(Err(crate::http::HttpFailure::Abort)) => {
        reject_value(scope, resolver, stored_abort_reason(abort_id));
      }
      Ok(Err(crate::http::HttpFailure::Type(message))) => {
        reject_value(scope, resolver, Reject::Type(message));
      }
      Ok(Err(crate::http::HttpFailure::Network(_message))) => {
        reject_value(
          scope,
          resolver,
          Reject::Type("Failed to fetch".to_string()),
        );
      }
      Err(()) => reject_type(scope, resolver, "fetch failed"),
    }
  });
  promise.into()
}

fn spawn_file<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  url: url::Url,
) -> v8::Local<'s, v8::Value> {
  let path = match file_inside_entry(&url) {
    Ok(path) => path,
    Err(error) => return rejected(scope, error),
  };
  let href = url.to_string();
  let mime = mime_from_path(&path);
  promise_task(scope, move || {
    let meta = std::fs::metadata(&path)
      .map_err(|_| Reject::Type("The file could not be read".to_string()))?;
    if !meta.is_file() {
      return Err(Reject::Type("The file could not be read".to_string()));
    }
    Ok(Loaded::new(
      crate::blob::BlobSource::file(path, meta.len()),
      mime,
      href,
    ))
  })
}

fn load_blob<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  url: &url::Url,
  headers: &[(String, String)],
) -> v8::Local<'s, v8::Value> {
  let href = url.to_string();
  let Some(lease) = lease_blob_url(&href) else {
    return rejected(
      scope,
      Reject::Type("The blob URL is unknown".to_string()),
    );
  };
  let Some((source, mime)) = lease.source() else {
    return rejected(
      scope,
      Reject::Type("The blob URL is unknown".to_string()),
    );
  };
  drop(lease);
  let loaded = match ranged_blob(source, mime, href, headers) {
    Ok(loaded) => loaded,
    Err(()) => {
      return rejected(scope, Reject::Type("Failed to fetch".to_string()));
    }
  };
  promise_task(scope, move || Ok(loaded))
}

/// `Err` is a network error from an unsatisfiable or unparsable `Range`.
fn ranged_blob(
  source: crate::blob::BlobSource,
  mime: String,
  url: String,
  headers: &[(String, String)],
) -> Result<Loaded, ()> {
  let Some((start, end)) = blob_satisfying_range(headers, source.size())?
  else {
    return Ok(Loaded::new(source, mime, url));
  };
  let full = source.size();
  let sliced = source.slice(start, end.saturating_add(1));
  let length = sliced.size();
  Ok(Loaded {
    source: sliced,
    mime,
    url,
    status: 206,
    status_text: "Partial Content",
    content_length: Some(length),
    content_range: Some(format!("bytes {start}-{end}/{full}")),
    force_type: true,
  })
}

/// Inclusive range from a blob URL `Range` header.
/// `Ok(None)` when the header is absent.
fn blob_satisfying_range(
  headers: &[(String, String)],
  full_length: u64,
) -> Result<Option<(u64, u64)>, ()> {
  let Some(value) = combined_header(headers, "range") else {
    return Ok(None);
  };
  Ok(Some(parse_single_range(&value, full_length)?))
}

fn combined_header(headers: &[(String, String)], name: &str) -> Option<String> {
  let mut combined = String::new();
  let mut any = false;
  for (key, value) in headers {
    if !key.eq_ignore_ascii_case(name) {
      continue;
    }
    if any {
      combined.push_str(", ");
    }
    any = true;
    combined.push_str(value);
  }
  any.then_some(combined)
}

/// Fetch's "parse a single range header value" with `allowWhitespace`,
/// then the blob scheme steps that turn it into an inclusive byte range.
fn parse_single_range(value: &str, full_length: u64) -> Result<(u64, u64), ()> {
  let chars: Vec<char> = value.chars().collect();
  if chars.len() < 5 || chars[..5] != ['b', 'y', 't', 'e', 's'] {
    return Err(());
  }
  let mut position = 5;
  skip_tab_or_space(&chars, &mut position);
  if chars.get(position).copied() != Some('=') {
    return Err(());
  }
  position += 1;
  skip_tab_or_space(&chars, &mut position);
  let start = decimal_bound(&chars, &mut position);
  skip_tab_or_space(&chars, &mut position);
  if chars.get(position).copied() != Some('-') {
    return Err(());
  }
  position += 1;
  skip_tab_or_space(&chars, &mut position);
  let end = decimal_bound(&chars, &mut position);
  if position != chars.len() {
    return Err(());
  }
  resolve_range(start, end, full_length)
}

fn skip_tab_or_space(chars: &[char], position: &mut usize) {
  while chars
    .get(*position)
    .is_some_and(|ch| *ch == ' ' || *ch == '\t')
  {
    *position += 1;
  }
}

enum Bound {
  Absent,
  Number(u64),
  Huge,
}

fn decimal_bound(chars: &[char], position: &mut usize) -> Bound {
  let start = *position;
  while chars.get(*position).is_some_and(|ch| ch.is_ascii_digit()) {
    *position += 1;
  }
  if start == *position {
    return Bound::Absent;
  }
  let digits: String = chars[start..*position].iter().collect();
  match digits.parse::<u64>() {
    Ok(value) => Bound::Number(value),
    Err(_) => Bound::Huge,
  }
}

fn resolve_range(
  start: Bound,
  end: Bound,
  full_length: u64,
) -> Result<(u64, u64), ()> {
  match start {
    Bound::Huge => Err(()),
    Bound::Number(start) => {
      if let Bound::Number(end_value) = end
        && start > end_value
      {
        return Err(());
      }
      if start >= full_length {
        return Err(());
      }
      let end = match end {
        Bound::Number(end) if end < full_length => end,
        _ => full_length.checked_sub(1).ok_or(())?,
      };
      Ok((start, end))
    }
    Bound::Absent => match end {
      Bound::Absent => Err(()),
      Bound::Huge => {
        let end = full_length.checked_sub(1).ok_or(())?;
        Ok((0, end))
      }
      Bound::Number(suffix) => {
        if suffix == 0 {
          return Err(());
        }
        let end = full_length.checked_sub(1).ok_or(())?;
        let start = full_length.saturating_sub(suffix);
        Ok((start, end))
      }
    },
  }
}

fn spawn_data<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  url: url::Url,
) -> v8::Local<'s, v8::Value> {
  let href = url.to_string();
  promise_task(scope, move || {
    let (bytes, mime) = parse_data_url(&url)?;
    Ok(Loaded::new(
      crate::blob::BlobSource::from_vec(bytes),
      mime,
      href,
    ))
  })
}

fn file_inside_entry(url: &url::Url) -> Result<std::path::PathBuf, Reject> {
  let path = url
    .to_file_path()
    .map_err(|()| Reject::Type("The file URL is invalid".to_string()))?;
  let root = yuzuko_core::entry_dir()
    .ok_or_else(|| Reject::Type("fetch has no entry directory".to_string()))?;
  let normalized = normalize(&path);
  if !normalized.starts_with(&root) {
    return Err(Reject::Type(
      "The file URL is outside the entry directory".to_string(),
    ));
  }
  match std::fs::canonicalize(&path) {
    Ok(canonical) if canonical.starts_with(&root) => Ok(canonical),
    Ok(_) => Err(Reject::Type(
      "The file URL is outside the entry directory".to_string(),
    )),
    Err(_) => Ok(normalized),
  }
}

fn normalize(path: &Path) -> std::path::PathBuf {
  let mut out = std::path::PathBuf::new();
  for component in path.components() {
    match component {
      std::path::Component::ParentDir => {
        out.pop();
      }
      std::path::Component::CurDir => {}
      other => out.push(other),
    }
  }
  out
}

fn mime_from_path(path: &Path) -> String {
  let extension = path
    .extension()
    .and_then(|value| value.to_str())
    .unwrap_or("")
    .to_ascii_lowercase();
  match extension.as_str() {
    "png" => "image/png",
    "jpg" | "jpeg" => "image/jpeg",
    "webp" => "image/webp",
    "jxl" => "image/jxl",
    "gif" => "image/gif",
    "svg" => "image/svg+xml",
    "txt" => "text/plain",
    "html" | "htm" => "text/html",
    "json" => "application/json",
    "js" | "mjs" => "text/javascript",
    "wasm" => "application/wasm",
    _ => "application/octet-stream",
  }
  .to_string()
}

fn parse_data_url(url: &url::Url) -> Result<(Vec<u8>, String), Reject> {
  crate::text::mime::data_url(url)
    .ok_or_else(|| Reject::Type("The data URL is invalid".to_string()))
}

fn promise_task<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  work: impl FnOnce() -> Result<Loaded, Reject> + Send + 'static,
) -> v8::Local<'s, v8::Value> {
  let resolver = v8::PromiseResolver::new(scope).unwrap();
  let promise = resolver.get_promise(scope);
  let resolver = v8::Global::new(scope, resolver);
  yuzuko_core::spawn(work, move |scope, result| {
    let resolver = v8::Local::new(scope, resolver);
    match result {
      Ok(Ok(loaded)) => match Response::from_loaded(scope, loaded) {
        Ok(response) => {
          let object =
            unsafe { yuzuko_core::op::wrap_instance(scope, response) };
          resolver.resolve(scope, object.into());
        }
        Err(error) => reject_op(scope, resolver, error),
      },
      Ok(Err(error)) => reject_value(scope, resolver, error),
      Err(()) => reject_type(scope, resolver, "fetch failed"),
    }
  });
  promise.into()
}

fn reject_consumed(
  scope: &mut v8::PinScope,
  error: OpError,
) -> ResponsePromise {
  let resolver = v8::PromiseResolver::new(scope).unwrap();
  let promise = resolver.get_promise(scope);
  reject_op(scope, resolver, error);
  ResponsePromise(v8::Global::new(scope, promise))
}

fn settle_later(
  scope: &mut v8::PinScope,
  finish: impl FnOnce(&mut v8::PinScope, v8::Local<v8::PromiseResolver>) + 'static,
) -> ResponsePromise {
  let resolver = v8::PromiseResolver::new(scope).unwrap();
  let promise = resolver.get_promise(scope);
  let stored = v8::Global::new(scope, resolver);
  yuzuko_core::enqueue(move |scope| {
    let resolver = v8::Local::new(scope, stored);
    finish(scope, resolver);
  });
  ResponsePromise(v8::Global::new(scope, promise))
}

enum Reject {
  Type(String),
  Dom(String, String),
  Thrown(v8::Global<v8::Value>),
}

fn default_abort() -> Reject {
  Reject::Dom(
    "AbortError".to_string(),
    "The operation was aborted".to_string(),
  )
}

fn dispatch_prepared<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  prepared: crate::http::request::Prepared,
) -> v8::Local<'s, v8::Value> {
  let url = match resolve_url(&prepared.url) {
    Ok(url) => url,
    Err(error) => return rejected(scope, error),
  };
  match url.scheme() {
    "http" | "https" => spawn_prepared_http(scope, prepared, url),
    "file" => spawn_file(scope, url),
    "blob" => load_blob(scope, &url, &prepared.headers),
    "data" => spawn_data(scope, url),
    _ => unsupported_scheme(scope),
  }
}

fn spawn_prepared_http<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  prepared: crate::http::request::Prepared,
  url: url::Url,
) -> v8::Local<'s, v8::Value> {
  let cancel = Arc::new(AtomicBool::new(false));
  let mut request = HttpInit {
    method: prepared.method,
    url: url.to_string(),
    headers: prepared.headers,
    body: prepared.body,
    redirect: prepared.redirect,
    cancel: Arc::clone(&cancel),
    abort_reason: None,
    abort_id: None,
    request_stream: None,
  };
  if let Some(signal) = prepared.signal {
    let signal = v8::Local::new(scope, signal);
    if let Err(error) = watch_abort(scope, signal.into(), &mut request) {
      return rejected(scope, error);
    }
  }
  if request.cancel.load(Ordering::Relaxed) {
    return rejected(scope, request.abort_reason.unwrap_or_else(default_abort));
  }
  let cancel = Arc::clone(&request.cancel);
  let abort_id = request.abort_id;
  promise_http(scope, abort_id, Arc::clone(&cancel), move || {
    crate::http::perform(
      request.method,
      request.url,
      request.headers,
      request.body,
      request.redirect,
      cancel,
    )
  })
}

fn rejected<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  error: Reject,
) -> v8::Local<'s, v8::Value> {
  let resolver = v8::PromiseResolver::new(scope).unwrap();
  let promise = resolver.get_promise(scope);
  reject_value(scope, resolver, error);
  promise.into()
}

fn reject_value(
  scope: &mut v8::PinScope,
  resolver: v8::Local<v8::PromiseResolver>,
  error: Reject,
) {
  let exception = match error {
    Reject::Type(message) => {
      let message = v8::String::new(scope, &message).unwrap();
      v8::Exception::type_error(scope, message)
    }
    Reject::Dom(name, message) => {
      yuzuko_core::op::new_dom_exception(scope, &name, &message)
        .map(v8::Local::<v8::Value>::from)
        .unwrap_or_else(|| {
          let message = v8::String::new(scope, &message).unwrap();
          v8::Exception::error(scope, message)
        })
    }
    Reject::Thrown(value) => v8::Local::new(scope, value),
  };
  resolver.reject(scope, exception);
}

fn reject_type(
  scope: &mut v8::PinScope,
  resolver: v8::Local<v8::PromiseResolver>,
  message: &str,
) {
  reject_value(scope, resolver, Reject::Type(message.to_string()));
}

fn reject_op(
  scope: &mut v8::PinScope,
  resolver: v8::Local<v8::PromiseResolver>,
  error: OpError,
) {
  if let OpError::Thrown(value) = error {
    let value = v8::Local::new(scope, value.as_ref());
    resolver.reject(scope, value);
    return;
  }
  let message = match error {
    OpError::Type(message) | OpError::Range(message) => message,
    OpError::Dom { message, .. } => message,
    OpError::Thrown(_) => "fetch failed".to_string(),
  };
  reject_type(scope, resolver, &message);
}

fn reject_syntax(
  scope: &mut v8::PinScope,
  resolver: v8::Local<v8::PromiseResolver>,
) {
  let message = v8::String::new(scope, "The response is not JSON").unwrap();
  resolver.reject(scope, v8::Exception::syntax_error(scope, message));
}

fn to_string(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<String, Reject> {
  let captured = {
    v8::tc_scope!(let tc, scope);
    match value.to_string(tc) {
      Some(text) => Ok(text.to_rust_string_lossy(tc)),
      None => Err(()),
    }
  };
  captured.map_err(|()| Reject::Type("The URL is invalid".to_string()))
}

pub(crate) enum Extracted {
  Bytes(Vec<u8>, Option<String>),
  Stream(v8::Global<v8::Object>, Option<String>),
}

type ResponseInit = (u16, String, Vec<(String, String)>);

fn response_init(
  scope: &mut v8::PinScope,
  init: Option<v8::Local<v8::Value>>,
) -> Result<ResponseInit, OpError> {
  let Some(init) = init.filter(|value| !value.is_null_or_undefined()) else {
    return Ok((200, String::new(), Vec::new()));
  };
  let object = v8::Local::<v8::Object>::try_from(init)
    .map_err(|_| OpError::type_("Response init is not an object"))?;
  let status = match init_property(scope, object, "status")? {
    Some(value) => {
      let number = value.number_value(scope).unwrap_or(f64::NAN);
      if !number.is_finite() || !(200.0..=599.0).contains(&number) {
        return Err(OpError::range("The status is invalid"));
      }
      number as u16
    }
    None => 200,
  };
  let status_text = match init_property(scope, object, "statusText")? {
    Some(value) => {
      let text = value
        .to_string(scope)
        .map(|text| text.to_rust_string_lossy(scope))
        .unwrap_or_default();
      if text.chars().any(|ch| {
        let code = u32::from(ch);
        code == 0 || code == 0x0A || code == 0x0D || code > 0xFF
      }) {
        return Err(OpError::type_("The status text is invalid"));
      }
      text
    }
    None => String::new(),
  };
  let headers = match init_property(scope, object, "headers")? {
    Some(value) => read_header_init(scope, Some(value))?,
    None => Vec::new(),
  };
  Ok((status, status_text, headers))
}

pub(crate) fn extract_body(
  scope: &mut v8::PinScope,
  body: Option<v8::Local<v8::Value>>,
) -> Result<Option<Extracted>, OpError> {
  let Some(body) = body.filter(|value| !value.is_null_or_undefined()) else {
    return Ok(None);
  };
  if let Ok(object) = v8::Local::<v8::Object>::try_from(body)
    && let Some(form) = unsafe {
      yuzuko_core::op::host_ref::<crate::http::form::FormData>(scope, object)
    }
  {
    let (bytes, mime) = crate::http::form::encode(form.get());
    return Ok(Some(Extracted::Bytes(bytes, Some(mime))));
  }
  if let Some((bytes, mime)) = crate::blob::blob_parts(scope, body)? {
    let mime = (!mime.is_empty()).then_some(mime);
    return Ok(Some(Extracted::Bytes(bytes, mime)));
  }
  if let Some(bytes) = crate::bytes::buffer_source(scope, body) {
    return Ok(Some(Extracted::Bytes(bytes, None)));
  }
  if let Ok(object) = v8::Local::<v8::Object>::try_from(body) {
    if string_tag(scope, object).as_deref() == Some("URLSearchParams") {
      let text = object
        .to_string(scope)
        .map(|text| text.to_rust_string_lossy(scope))
        .unwrap_or_default();
      return Ok(Some(Extracted::Bytes(
        text.into_bytes(),
        Some("application/x-www-form-urlencoded;charset=UTF-8".to_string()),
      )));
    }
    if unsafe {
      yuzuko_core::op::host_ref::<crate::stream::ReadableStream>(scope, object)
    }
    .is_some()
    {
      let global = v8::Global::new(scope, object);
      let id = crate::stream::stream_id_of(scope, &global).unwrap_or(0);
      if crate::stream::is_disturbed(id) || crate::stream::is_locked(id) {
        return Err(OpError::type_("Body has already been used"));
      }
      return Ok(Some(Extracted::Stream(global, None)));
    }
  }
  let text = {
    v8::tc_scope!(let tc, scope);
    match body.to_string(tc) {
      Some(text) => Ok(text.to_rust_string_lossy(tc)),
      None => Err(tc.exception().map(|value| v8::Global::new(tc, value))),
    }
  };
  match text {
    Ok(text) => Ok(Some(Extracted::Bytes(
      text.into_bytes(),
      Some("text/plain;charset=UTF-8".to_string()),
    ))),
    Err(Some(thrown)) => Err(OpError::Thrown(Box::new(thrown))),
    Err(None) => Err(OpError::type_("The response body is invalid")),
  }
}

fn string_tag(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
) -> Option<String> {
  let symbol = v8::Symbol::get_to_string_tag(scope);
  let value = object.get(scope, symbol.into())?;
  value
    .to_string(scope)
    .map(|text| text.to_rust_string_lossy(scope))
}

fn init_property<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  object: v8::Local<v8::Object>,
  name: &str,
) -> Result<Option<v8::Local<'s, v8::Value>>, OpError> {
  let found = {
    v8::tc_scope!(let tc, scope);
    let key = v8::String::new(tc, name).unwrap();
    match object.get(tc, key.into()) {
      Some(value) => Ok(v8::Global::new(tc, value)),
      None => Err(tc.exception().map(|value| v8::Global::new(tc, value))),
    }
  };
  match found {
    Ok(value) => {
      let value = v8::Local::new(scope, value);
      if value.is_undefined() {
        Ok(None)
      } else {
        Ok(Some(value))
      }
    }
    Err(Some(thrown)) => Err(OpError::Thrown(Box::new(thrown))),
    Err(None) => Err(OpError::type_("The property access failed")),
  }
}

pub(crate) fn read_header_init(
  scope: &mut v8::PinScope,
  init: Option<v8::Local<v8::Value>>,
) -> Result<Vec<(String, String)>, OpError> {
  let Some(init) = init else {
    return Ok(Vec::new());
  };
  if init.is_null() {
    return Err(OpError::type_("Headers initialization cannot be null"));
  }
  if init.is_undefined() {
    return Ok(Vec::new());
  }
  let object = v8::Local::<v8::Object>::try_from(init)
    .map_err(|_| OpError::type_("Headers initialization is not an object"))?;
  // HeadersInit is a sequence when @@iterator is present, including a Headers
  // object whose iterator was replaced.
  if has_iterator(scope, object)? {
    return read_header_pairs(scope, object);
  }
  read_header_record(scope, object)
}

fn has_iterator(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
) -> Result<bool, OpError> {
  let symbol = v8::Symbol::get_iterator(scope);
  let value = js_get(scope, object, symbol.into())?;
  Ok(v8::Local::<v8::Function>::try_from(value).is_ok())
}

fn js_get<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  object: v8::Local<v8::Object>,
  key: v8::Local<v8::Value>,
) -> Result<v8::Local<'s, v8::Value>, OpError> {
  let object = v8::Global::new(scope, object);
  let key = v8::Global::new(scope, key);
  let captured = {
    v8::tc_scope!(let tc, scope);
    let object = v8::Local::new(tc, &object);
    let key = v8::Local::new(tc, &key);
    match object.get(tc, key) {
      Some(value) => Ok(v8::Global::new(tc, value)),
      None => Err(tc.exception().map(|value| v8::Global::new(tc, value))),
    }
  };
  match captured {
    Ok(value) => Ok(v8::Local::new(scope, value)),
    Err(Some(thrown)) => Err(OpError::Thrown(Box::new(thrown))),
    Err(None) => Err(OpError::type_("The property access failed")),
  }
}

fn call_js<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  function: v8::Local<v8::Function>,
  receiver: v8::Local<v8::Value>,
  args: &[v8::Local<v8::Value>],
) -> Result<v8::Local<'s, v8::Value>, OpError> {
  let function = v8::Global::new(scope, function);
  let receiver = v8::Global::new(scope, receiver);
  let args = args
    .iter()
    .map(|value| v8::Global::new(scope, *value))
    .collect::<Vec<_>>();
  let captured = {
    v8::tc_scope!(let tc, scope);
    let function = v8::Local::new(tc, &function);
    let receiver = v8::Local::new(tc, &receiver);
    let args = args
      .iter()
      .map(|value| v8::Local::new(tc, value))
      .collect::<Vec<_>>();
    match function.call(tc, receiver, &args) {
      Some(value) => Ok(v8::Global::new(tc, value)),
      None => Err(tc.exception().map(|value| v8::Global::new(tc, value))),
    }
  };
  match captured {
    Ok(value) => Ok(v8::Local::new(scope, value)),
    Err(Some(thrown)) => Err(OpError::Thrown(Box::new(thrown))),
    Err(None) => Err(OpError::type_("The operation failed")),
  }
}

fn latin1_string(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<String, OpError> {
  let captured = {
    v8::tc_scope!(let tc, scope);
    match value.to_string(tc) {
      Some(text) => {
        let text = text.to_rust_string_lossy(tc);
        if text.chars().any(|ch| u32::from(ch) > 0xFF) {
          Err(None)
        } else {
          Ok(text)
        }
      }
      None => Err(tc.exception().map(|value| v8::Global::new(tc, value))),
    }
  };
  match captured {
    Ok(text) => Ok(text),
    Err(Some(thrown)) => Err(OpError::Thrown(Box::new(thrown))),
    Err(None) => Err(OpError::type_("The string is not a ByteString")),
  }
}

fn realm_method<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  object_name: &str,
  method: &str,
) -> Result<(v8::Local<'s, v8::Object>, v8::Local<'s, v8::Function>), OpError> {
  let global = scope.get_current_context().global(scope);
  let object_key = v8::String::new(scope, object_name).unwrap();
  let object = global
    .get(scope, object_key.into())
    .and_then(|value| v8::Local::<v8::Object>::try_from(value).ok())
    .ok_or_else(|| OpError::type_("The header record is invalid"))?;
  let method_key = v8::String::new(scope, method).unwrap();
  let function = object
    .get(scope, method_key.into())
    .and_then(|value| v8::Local::<v8::Function>::try_from(value).ok())
    .ok_or_else(|| OpError::type_("The header record is invalid"))?;
  Ok((object, function))
}

fn read_header_pairs(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
) -> Result<Vec<(String, String)>, OpError> {
  let symbol = v8::Symbol::get_iterator(scope);
  let next = object
    .get(scope, symbol.into())
    .and_then(|value| v8::Local::<v8::Function>::try_from(value).ok())
    .ok_or_else(|| OpError::type_("Headers initialization is not iterable"))?;
  let iterator = next
    .call(scope, object.into(), &[])
    .and_then(|value| v8::Local::<v8::Object>::try_from(value).ok())
    .ok_or_else(|| OpError::type_("Headers initialization is not iterable"))?;
  let step_key = v8::String::new(scope, "next").unwrap();
  let mut pairs = Vec::new();
  for _ in 0..10_000 {
    let step = iterator
      .get(scope, step_key.into())
      .and_then(|value| v8::Local::<v8::Function>::try_from(value).ok())
      .and_then(|step| step.call(scope, iterator.into(), &[]))
      .and_then(|value| v8::Local::<v8::Object>::try_from(value).ok())
      .ok_or_else(|| {
        OpError::type_("Headers initialization is not iterable")
      })?;
    let done_key = v8::String::new(scope, "done").unwrap();
    let done = step
      .get(scope, done_key.into())
      .is_some_and(|value| value.boolean_value(scope));
    if done {
      return Ok(pairs);
    }
    let value_key = v8::String::new(scope, "value").unwrap();
    let value = step
      .get(scope, value_key.into())
      .ok_or_else(|| OpError::type_("A header pair is required"))?;
    let pair = v8::Local::<v8::Object>::try_from(value)
      .map_err(|_| OpError::type_("A header pair is required"))?;
    let length_key = v8::String::new(scope, "length").unwrap();
    let length = pair
      .get(scope, length_key.into())
      .and_then(|value| value.number_value(scope))
      .unwrap_or(-1.0);
    if length != 2.0 {
      return Err(OpError::type_("A header pair must have two items"));
    }
    let name = pair
      .get_index(scope, 0)
      .ok_or_else(|| OpError::type_("A header pair is required"))?;
    let header_value = pair
      .get_index(scope, 1)
      .ok_or_else(|| OpError::type_("A header pair is required"))?;
    let name = name
      .to_string(scope)
      .map(|text| text.to_rust_string_lossy(scope))
      .unwrap_or_default();
    let header_value = header_value
      .to_string(scope)
      .map(|text| text.to_rust_string_lossy(scope))
      .unwrap_or_default();
    // Keep the caller’s spelling. `append` byte-lowercases the stored name.
    let _ = normalize_header(&name)?;
    pairs.push((name, normalize_value(&header_value)?));
  }
  Err(OpError::type_("Headers initialization is too large"))
}

fn read_header_record(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
) -> Result<Vec<(String, String)>, OpError> {
  // Web IDL record conversion: OwnPropertyKeys, then GetOwnProperty, then
  // ByteString(key) before Get, skipping non-enumerable keys.
  let (reflect, own_keys) = realm_method(scope, "Reflect", "ownKeys")?;
  let keys_value = call_js(scope, own_keys, reflect.into(), &[object.into()])?;
  let keys = v8::Local::<v8::Array>::try_from(keys_value)
    .map_err(|_| OpError::type_("Reflect.ownKeys did not return an array"))?;
  let (object_ctor, get_descriptor) =
    realm_method(scope, "Object", "getOwnPropertyDescriptor")?;
  let mut pairs = Vec::new();
  for index in 0..keys.length() {
    let key = keys
      .get_index(scope, index)
      .ok_or_else(|| OpError::type_("A header name is required"))?;
    let descriptor = call_js(
      scope,
      get_descriptor,
      object_ctor.into(),
      &[object.into(), key],
    )?;
    if descriptor.is_undefined() || !descriptor_enumerable(scope, descriptor)? {
      continue;
    }
    let name = latin1_string(scope, key)?;
    let value = js_get(scope, object, key)?;
    let value = latin1_string(scope, value)?;
    let _ = normalize_header(&name)?;
    pairs.push((name, normalize_value(&value)?));
  }
  Ok(pairs)
}

fn descriptor_enumerable(
  scope: &mut v8::PinScope,
  descriptor: v8::Local<v8::Value>,
) -> Result<bool, OpError> {
  let object = v8::Local::<v8::Object>::try_from(descriptor)
    .map_err(|_| OpError::type_("The property descriptor is invalid"))?;
  let key = v8::String::new(scope, "enumerable").unwrap();
  let value = js_get(scope, object, key.into())?;
  Ok(value.boolean_value(scope))
}

pub(crate) fn normalize_header(name: &str) -> Result<String, OpError> {
  if name.is_empty()
    || !name.bytes().all(|byte| {
      byte.is_ascii_alphanumeric()
        || matches!(
          byte,
          b'!'
            | b'#'
            | b'$'
            | b'%'
            | b'&'
            | b'\''
            | b'*'
            | b'+'
            | b'-'
            | b'.'
            | b'^'
            | b'_'
            | b'`'
            | b'|'
            | b'~'
        )
    })
  {
    return Err(OpError::type_("The header name is invalid"));
  }
  Ok(name.to_ascii_lowercase())
}

/// Unrestricted fetch posture: no request header is forbidden.
fn is_forbidden_request_header(_name: &str, _value: &str) -> bool {
  false
}

fn is_no_cors_name(name: &str) -> bool {
  matches!(
    name,
    "accept" | "accept-language" | "content-language" | "content-type"
  )
}

fn is_no_cors_safelisted(name: &str, value: &str) -> bool {
  is_no_cors_name(name) && is_cors_safelisted(name, value)
}

fn is_cors_safelisted(name: &str, value: &str) -> bool {
  if value.len() > 128 {
    return false;
  }
  match name {
    "accept" => !value.bytes().any(cors_unsafe_byte),
    "accept-language" | "content-language" => {
      value.bytes().all(is_language_byte)
    }
    "content-type" => safelisted_content_type(value),
    _ => false,
  }
}

fn cors_unsafe_byte(byte: u8) -> bool {
  (byte < 0x20 && byte != b'\t')
    || matches!(
      byte,
      b'"'
        | b'('
        | b')'
        | b':'
        | b'<'
        | b'>'
        | b'?'
        | b'@'
        | b'['
        | b'\\'
        | b']'
        | b'{'
        | b'}'
        | 0x7f
    )
}

fn is_language_byte(byte: u8) -> bool {
  byte.is_ascii_alphanumeric()
    || matches!(byte, b' ' | b'*' | b',' | b'-' | b'.' | b';' | b'=')
}

fn safelisted_content_type(value: &str) -> bool {
  if value.bytes().any(cors_unsafe_byte) {
    return false;
  }
  let Some(essence) = value.split(';').next() else {
    return false;
  };
  let essence = essence.trim();
  let mut parts = essence.split('/');
  let Some(type_) = parts.next() else {
    return false;
  };
  let Some(subtype) = parts.next() else {
    return false;
  };
  if parts.next().is_some()
    || type_.is_empty()
    || subtype.is_empty()
    || !type_.bytes().all(is_token_byte)
    || !subtype.bytes().all(is_token_byte)
  {
    return false;
  }
  matches!(
    essence.to_ascii_lowercase().as_str(),
    "application/x-www-form-urlencoded" | "multipart/form-data" | "text/plain"
  )
}

fn is_token_byte(byte: u8) -> bool {
  byte.is_ascii_alphanumeric()
    || matches!(
      byte,
      b'!'
        | b'#'
        | b'$'
        | b'%'
        | b'&'
        | b'\''
        | b'*'
        | b'+'
        | b'-'
        | b'.'
        | b'^'
        | b'_'
        | b'`'
        | b'|'
        | b'~'
    )
}

fn is_http_whitespace(ch: char) -> bool {
  matches!(ch, '\t' | '\n' | '\r' | ' ')
}

/// Fetch header-value normalize: strip leading and trailing HTTP whitespace,
/// then reject NUL and HTTP newlines. Code points above U+00FF are not a
/// `ByteString`.
pub(crate) fn normalize_value(value: &str) -> Result<String, OpError> {
  if value.chars().any(|ch| u32::from(ch) > 0xFF) {
    return Err(OpError::type_("The header value is invalid"));
  }
  let value = value.trim_matches(is_http_whitespace);
  if value
    .chars()
    .any(|ch| ch == '\0' || ch == '\n' || ch == '\r')
  {
    return Err(OpError::type_("The header value is invalid"));
  }
  Ok(value.to_string())
}

pub(crate) fn header_list_value(
  pairs: &[(String, String)],
  name: &str,
) -> String {
  let mut combined = String::new();
  let mut any = false;
  for (key, value) in pairs {
    if key != name {
      continue;
    }
    if any {
      combined.push_str(", ");
    }
    any = true;
    combined.push_str(value);
  }
  combined
}

#[cfg(test)]
mod tests {
  use super::blob_satisfying_range;
  use super::ranged_blob;
  use crate::blob::BlobSource;

  fn headers(range: &str) -> Vec<(String, String)> {
    vec![("range".to_string(), range.to_string())]
  }

  fn slice_text(data: &str, range: &str) -> Result<String, ()> {
    let source = BlobSource::from_vec(data.as_bytes().to_vec());
    let loaded = ranged_blob(
      source,
      "text/plain".to_string(),
      "blob:null/test".to_string(),
      &headers(range),
    )?;
    assert_eq!(loaded.status, 206);
    assert_eq!(loaded.status_text, "Partial Content");
    assert!(loaded.force_type);
    let bytes = loaded.source.read_all().unwrap();
    let text = String::from_utf8(bytes).unwrap();
    let length = loaded.content_length.unwrap();
    assert_eq!(length, text.len() as u64);
    Ok(format!(
      "{text}|{}|{}",
      length,
      loaded.content_range.unwrap()
    ))
  }

  #[test]
  fn supported_blob_ranges_match_wpt() {
    let cases = [
      (
        "A simple Hello, World! example",
        "bytes=9-21",
        "Hello, World!|13|bytes 9-21/30",
      ),
      ("Range with no end", "bytes=11-", "no end|6|bytes 11-16/17"),
      (
        "Range with no start",
        "bytes=-8",
        "no start|8|bytes 11-18/19",
      ),
      (
        "A simple Hello, World! example",
        "bytes= \t9-21",
        "Hello, World!|13|bytes 9-21/30",
      ),
      (
        "Not much here",
        "bytes=4-100000000000",
        "much here|9|bytes 4-12/13",
      ),
      ("Not much here", "bytes=4-13", "much here|9|bytes 4-12/13"),
      (
        "Valid whitespace #1",
        "bytes=5 - 10",
        " white|6|bytes 5-10/19",
      ),
      (
        "Valid whitespace #2",
        "bytes=-\t 5",
        "ce #2|5|bytes 14-18/19",
      ),
      (
        "Valid whitespace #3",
        "bytes \t =\t 6-",
        "whitespace #3|13|bytes 6-18/19",
      ),
    ];
    for (data, range, expected) in cases {
      assert_eq!(slice_text(data, range).unwrap(), expected, "{range}");
    }
  }

  #[test]
  fn unsupported_blob_ranges_are_network_errors() {
    let ranges = [
      "",
      "byte=0-",
      "BYTES=0-5",
      "ByTes=0-5",
      "bytes",
      "bytes\t \t",
      "bytes=\u{000C}0-5",
      "bytes=0\u{000C}-5",
      "bytes=\u{000B}0-5",
      "bytes=0-5,15-",
      "bytes=0-5, 15-",
      "bytes=0-5,",
      "bytes=-",
      "bytes=10-5",
      "bytes=x-5",
      "bytes=5",
      "bytes=5-x",
      "bytes=+0-5",
      "bytes=+5-",
      "bytes=0-+5",
      "bytes=x",
      "bytes 5-",
      "5-",
      "bytes=100000-",
      "bytes=13-",
    ];
    for range in ranges {
      let error = slice_text("Not much here", range);
      assert!(error.is_err(), "{range}");
    }
  }

  #[test]
  fn missing_range_header_keeps_the_whole_blob() {
    let source = BlobSource::from_vec(b"whole".to_vec());
    assert_eq!(blob_satisfying_range(&[], source.size()).unwrap(), None);
    let loaded =
      ranged_blob(source, String::new(), "blob:null/test".to_string(), &[])
        .unwrap();
    assert_eq!(loaded.status, 200);
    assert!(!loaded.force_type);
    assert!(loaded.content_range.is_none());
    assert_eq!(loaded.source.read_all().unwrap(), b"whole");
  }

  #[test]
  fn repeated_range_headers_fail() {
    let headers = vec![
      ("Range".to_string(), "bytes=0-1".to_string()),
      ("range".to_string(), "bytes=2-3".to_string()),
    ];
    assert!(blob_satisfying_range(&headers, 10).is_err());
  }
}
