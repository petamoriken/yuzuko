//! `Request`. Bodies are buffered. A streamed request body is read from the
//! stream's queue when the stream was already filled.

use std::cell::Cell;
use std::cell::RefCell;

use yuzuko_core::op::Js;
use yuzuko_core::op::OpError;
use yuzuko_core::op::host_ref;
use yuzuko_core::op::traced;
use yuzuko_core::op::traced_local;
use yuzuko_macros::op;

use crate::blob::Blob;
use crate::bytes;
use crate::http::fetch::Extracted;
use crate::http::fetch::Headers;
use crate::http::fetch::extract_body;
use crate::http::fetch::read_header_init;

pub struct Prepared {
  pub method: String,
  pub url: String,
  pub headers: Vec<(String, String)>,
  pub body: Vec<u8>,
  pub redirect: String,
  pub signal: Option<v8::Global<v8::Object>>,
}

pub struct Request {
  method: String,
  url: String,
  redirect: String,
  cache: String,
  headers: RefCell<Option<v8::TracedReference<v8::Object>>>,
  body: RefCell<Option<Vec<u8>>>,
  stream: RefCell<Option<v8::TracedReference<v8::Object>>>,
  stream_id: Cell<u64>,
  consumed: Cell<bool>,
  mime: String,
  signal: RefCell<Option<v8::TracedReference<v8::Object>>>,
}

unsafe impl v8::cppgc::GarbageCollected for Request {
  fn trace(&self, visitor: &mut v8::cppgc::Visitor) {
    if let Ok(headers) = self.headers.try_borrow() {
      visitor.trace(&*headers);
    }
    if let Ok(stream) = self.stream.try_borrow() {
      visitor.trace(&*stream);
    }
    if let Ok(signal) = self.signal.try_borrow() {
      visitor.trace(&*signal);
    }
  }

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"Request"
  }
}

struct Built {
  request: Request,
}

#[op]
impl Request {
  #[constructor]
  fn new(
    scope: &mut v8::PinScope,
    input: v8::Local<v8::Value>,
    init: Option<v8::Local<v8::Value>>,
  ) -> Result<Self, OpError> {
    Ok(build(scope, input, init)?.request)
  }

  #[getter]
  fn method(&self) -> String {
    self.method.clone()
  }

  #[getter]
  fn url(&self) -> String {
    self.url.clone()
  }

  #[getter]
  fn redirect(&self) -> String {
    self.redirect.clone()
  }

  #[getter]
  fn cache(&self) -> String {
    self.cache.clone()
  }

  #[getter]
  fn duplex(&self) -> String {
    "half".to_string()
  }

  #[getter]
  fn headers(&self, scope: &mut v8::PinScope) -> Js {
    traced_or_undefined(scope, &self.headers)
  }

  #[getter]
  fn signal(&self, scope: &mut v8::PinScope) -> Js {
    traced_or_undefined(scope, &self.signal)
  }

  #[getter]
  fn body_used(&self) -> bool {
    self.body_is_used()
  }

  #[getter]
  fn body(&self, scope: &mut v8::PinScope) -> Js {
    if let Some(existing) = self.stream.borrow().as_ref()
      && let Some(object) = traced_local(scope, existing)
    {
      let value: v8::Local<v8::Value> = object.into();
      return Js(v8::Global::new(scope, value));
    }
    let Some(bytes) = self.body.borrow().clone() else {
      let value: v8::Local<v8::Value> = v8::null(scope).into();
      return Js(v8::Global::new(scope, value));
    };
    let object = crate::stream::stream_from_bytes(scope, bytes);
    self.cache_stream(scope, object.clone());
    let local = v8::Local::new(scope, &object);
    let value: v8::Local<v8::Value> = local.into();
    Js(v8::Global::new(scope, value))
  }

  fn clone(&self, scope: &mut v8::PinScope) -> Result<Js, OpError> {
    if self.body_is_used() {
      return Err(OpError::type_("Body has already been used"));
    }
    let headers = header_pairs(scope, &self.headers);
    let parent = {
      let borrowed = self.signal.borrow();
      borrowed
        .as_ref()
        .and_then(|reference| traced_local(scope, reference))
    };
    let signal = match parent {
      Some(parent) => dependent_signal(scope, &[parent])?,
      None => dependent_signal(scope, &[])?,
    };
    let stream_id = self.stream_id.get();
    let request = Request {
      method: self.method.clone(),
      url: self.url.clone(),
      redirect: self.redirect.clone(),
      cache: self.cache.clone(),
      headers: RefCell::new(None),
      body: RefCell::new(if stream_id == 0 {
        self.body.borrow().clone()
      } else {
        None
      }),
      stream: RefCell::new(None),
      stream_id: Cell::new(0),
      consumed: Cell::new(false),
      mime: self.mime.clone(),
      signal: RefCell::new(None),
    };
    let object = unsafe { yuzuko_core::op::wrap_instance(scope, request) };
    let host = unsafe { host_ref::<Request>(scope, object) }.unwrap();
    host.get().install_headers(scope, headers, None)?;
    *host.get().signal.borrow_mut() = Some(traced(scope, signal));
    if stream_id != 0 {
      let (left, right) = crate::stream::tee_pair(scope, stream_id)?;
      self.cache_stream(scope, left);
      host.get().cache_stream(scope, right);
    }
    let value: v8::Local<v8::Value> = object.into();
    Ok(Js(v8::Global::new(scope, value)))
  }

  #[promise]
  fn text(&self, scope: &mut v8::PinScope) -> Js {
    let stream_id = self.stream_id.get();
    if stream_id != 0 && !self.consumed.get() {
      return match crate::stream::read_stream_bytes(scope, stream_id) {
        Ok(crate::stream::StreamBody::Ready(bytes)) => {
          self.consumed.set(true);
          resolve_text(scope, bytes)
        }
        Ok(crate::stream::StreamBody::Pending(promise)) => {
          self.consumed.set(true);
          follow_request_text(scope, promise)
        }
        Err(error) => reject(scope, error),
      };
    }
    match self.take_body(scope) {
      Ok(bytes) => resolve_text(scope, bytes),
      Err(error) => reject(scope, error),
    }
  }

  #[promise]
  fn array_buffer(&self, scope: &mut v8::PinScope) -> Js {
    match self.take_body(scope) {
      Ok(bytes) => {
        let buffer = bytes::array_buffer(scope, bytes);
        let value: v8::Local<v8::Value> = buffer.into();
        resolve(scope, value)
      }
      Err(error) => reject(scope, error),
    }
  }

  #[promise]
  fn bytes(&self, scope: &mut v8::PinScope) -> Js {
    match self.take_body(scope) {
      Ok(bytes) => {
        let view = bytes::uint8_array(scope, bytes);
        let value: v8::Local<v8::Value> = view.into();
        resolve(scope, value)
      }
      Err(error) => reject(scope, error),
    }
  }

  #[promise]
  fn blob(&self, scope: &mut v8::PinScope) -> Js {
    let mime = crate::http::fetch::header_list_value(
      &header_pairs(scope, &self.headers),
      "content-type",
    );
    match self.take_body(scope) {
      Ok(bytes) => {
        let blob = unsafe {
          yuzuko_core::op::wrap_instance(scope, Blob::from_parts(bytes, mime))
        };
        let value: v8::Local<v8::Value> = blob.into();
        resolve(scope, value)
      }
      Err(error) => reject(scope, error),
    }
  }

  #[promise]
  fn json(&self, scope: &mut v8::PinScope) -> Js {
    match self.take_body(scope) {
      Ok(bytes) => {
        let text = crate::blob::decode_utf8(&bytes);
        let Some(source) = v8::String::new(scope, &text) else {
          return reject(scope, OpError::type_("JSON text is too large"));
        };
        match v8::json::parse(scope, source) {
          Some(value) => resolve(scope, value),
          None => reject_syntax(scope),
        }
      }
      Err(error) => reject(scope, error),
    }
  }

  #[rename("textStream")]
  fn text_stream(&self, scope: &mut v8::PinScope) -> Result<Js, OpError> {
    if self.body_is_used() {
      return Err(OpError::type_("Body has already been used"));
    }
    let null_body =
      self.stream.borrow().is_none() && self.body.borrow().is_none();
    if null_body {
      let stream = crate::stream::stream_from_bytes(scope, Vec::new());
      let local = v8::Local::new(scope, &stream);
      let value: v8::Local<v8::Value> = local.into();
      return Ok(Js(v8::Global::new(scope, value)));
    }
    let body = self.body_get_impl(scope);
    let local = v8::Local::new(scope, &body.0);
    let object = v8::Local::<v8::Object>::try_from(local)
      .map_err(|_| OpError::type_("The body is not a stream"))?;
    let decoded = crate::stream::text_stream_from_body(scope, object)?;
    let decoded = v8::Local::new(scope, &decoded);
    let value: v8::Local<v8::Value> = decoded.into();
    Ok(Js(v8::Global::new(scope, value)))
  }

  #[promise]
  fn form_data(&self, scope: &mut v8::PinScope) -> Js {
    let mime = crate::http::fetch::header_list_value(
      &header_pairs(scope, &self.headers),
      "content-type",
    );
    let null_body = self.stream_id.get() == 0 && self.body.borrow().is_none();
    if null_body && mime.to_ascii_lowercase().starts_with("multipart/form-data")
    {
      return reject(scope, OpError::type_("The multipart body is incomplete"));
    }
    match self.take_body(scope) {
      Ok(bytes) => match crate::http::form::parse(&mime, &bytes) {
        Ok(form) => {
          let object = unsafe { yuzuko_core::op::wrap_instance(scope, form) };
          let value: v8::Local<v8::Value> = object.into();
          resolve(scope, value)
        }
        Err(error) => reject(scope, error),
      },
      Err(error) => reject(scope, error),
    }
  }
}

impl Request {
  fn install_headers(
    &self,
    scope: &mut v8::PinScope,
    pairs: Vec<(String, String)>,
    content_type: Option<String>,
  ) -> Result<(), OpError> {
    let headers =
      Headers::from_pairs(pairs, crate::http::fetch::HeaderGuard::Request)?;
    if let Some(mime) = content_type
      && !headers.contains("content-type")
    {
      headers.append_pair("content-type".to_string(), mime)?;
    }
    let object = unsafe { yuzuko_core::op::wrap_instance(scope, headers) };
    *self.headers.borrow_mut() = Some(traced(scope, object));
    Ok(())
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

  fn body_is_used(&self) -> bool {
    self.consumed.get() || crate::stream::is_disturbed(self.stream_id.get())
  }

  fn take_body(&self, scope: &mut v8::PinScope) -> Result<Vec<u8>, OpError> {
    if self.consumed.get() || crate::stream::is_disturbed(self.stream_id.get())
    {
      return Err(OpError::type_("Body has already been used"));
    }
    let stream_id = self.stream_id.get();
    if stream_id != 0 {
      let bytes = match crate::stream::read_stream_bytes(scope, stream_id)? {
        crate::stream::StreamBody::Ready(bytes) => bytes,
        crate::stream::StreamBody::Pending(_) => {
          return Err(OpError::type_("The body is not buffered"));
        }
      };
      self.consumed.set(true);
      return Ok(bytes);
    }
    // A null body reads as empty and stays undisturbed.
    if self.body.borrow().is_none() {
      return Ok(Vec::new());
    }
    let bytes = self.body.borrow_mut().take().unwrap_or_default();
    self.consumed.set(true);
    Ok(bytes)
  }
}

pub fn is_request(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> bool {
  let Ok(object) = v8::Local::<v8::Object>::try_from(value) else {
    return false;
  };
  unsafe { host_ref::<Request>(scope, object) }.is_some()
}

/// Method and URL of a `Request`. Cache storage records these and not the
/// request's `AbortSignal`.
pub(crate) fn request_method_url(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<(String, String), OpError> {
  let object = v8::Local::<v8::Object>::try_from(value)
    .map_err(|_| OpError::type_("Cache.put requires a Request"))?;
  let request = unsafe { host_ref::<Request>(scope, object) }
    .ok_or_else(|| OpError::type_("Cache.put requires a Request"))?;
  let request = request.get();
  Ok((request.method.clone(), request.url.clone()))
}

pub(crate) fn ensure_default_headers(headers: &mut Vec<(String, String)>) {
  if !headers
    .iter()
    .any(|(name, _)| name.eq_ignore_ascii_case("accept"))
  {
    headers.push(("accept".to_string(), "*/*".to_string()));
  }
  if !headers
    .iter()
    .any(|(name, _)| name.eq_ignore_ascii_case("accept-language"))
  {
    headers.push(("accept-language".to_string(), "*".to_string()));
  }
  // A Range request is not compressed. Fetch sets Accept-Encoding to identity.
  if headers
    .iter()
    .any(|(name, _)| name.eq_ignore_ascii_case("range"))
    && !headers
      .iter()
      .any(|(name, _)| name.eq_ignore_ascii_case("accept-encoding"))
  {
    headers.push(("accept-encoding".to_string(), "identity".to_string()));
  }
}

pub fn prepare_fetch(
  scope: &mut v8::PinScope,
  input: v8::Local<v8::Value>,
  init: Option<v8::Local<v8::Value>>,
) -> Result<Prepared, OpError> {
  let built = build(scope, input, init)?;
  let signal = {
    let borrowed = built.request.signal.borrow();
    borrowed
      .as_ref()
      .and_then(|reference| traced_local(scope, reference))
      .map(|value| v8::Global::new(scope, value))
  };
  let mut headers = header_pairs(scope, &built.request.headers);
  if built.request.url.starts_with("http://")
    || built.request.url.starts_with("https://")
  {
    ensure_default_headers(&mut headers);
  }
  let body = if built.request.body.borrow().is_none()
    && built.request.stream_id.get() == 0
  {
    Vec::new()
  } else {
    built.request.take_body(scope)?
  };
  Ok(Prepared {
    method: built.request.method,
    url: built.request.url,
    headers,
    body,
    redirect: built.request.redirect,
    signal,
  })
}

fn build(
  scope: &mut v8::PinScope,
  input: v8::Local<v8::Value>,
  init: Option<v8::Local<v8::Value>>,
) -> Result<Built, OpError> {
  let init_object = match init {
    Some(value) if !value.is_null_or_undefined() => Some(
      v8::Local::<v8::Object>::try_from(value)
        .map_err(|_| OpError::type_("Request init is not an object"))?,
    ),
    _ => None,
  };
  let source = snapshot_source(scope, input)?;
  let mut method = source
    .as_ref()
    .map(|request| request.method.clone())
    .unwrap_or_else(|| "GET".to_string());
  if let Some(object) = init_object
    && let Some(value) = property(scope, object, "method")?
  {
    method = string_value(scope, value)?;
  }
  method = normalize_method(&method)?;
  let url = if let Some(source) = source.as_ref() {
    source.url.clone()
  } else {
    let text = string_value(scope, input)?;
    resolve_request_url(&text)?
  };
  let mut redirect = source
    .as_ref()
    .map(|request| request.redirect.clone())
    .unwrap_or_else(|| "follow".to_string());
  let mut cache = source
    .as_ref()
    .map(|request| request.cache.clone())
    .unwrap_or_else(|| "no-store".to_string());
  let mut duplex = None;
  if let Some(object) = init_object {
    if let Some(value) = property(scope, object, "redirect")? {
      redirect = string_value(scope, value)?;
    }
    if let Some(value) = property(scope, object, "cache")? {
      cache = string_value(scope, value)?;
    }
    if let Some(value) = property(scope, object, "window")?
      && !value.is_null()
    {
      return Err(OpError::type_("Window can only be null"));
    }
    if let Some(value) = property(scope, object, "priority")? {
      validate_priority(&string_value(scope, value)?)?;
    }
    if let Some(value) = property(scope, object, "integrity")? {
      let integrity = string_value(scope, value)?;
      if !integrity.is_empty() {
        return Err(OpError::dom(
          "NotSupportedError",
          "Subresource Integrity is not supported",
        ));
      }
    }
    if let Some(value) = property(scope, object, "duplex")? {
      let text = string_value(scope, value)?;
      if text != "half" {
        return Err(OpError::type_("The duplex mode is invalid"));
      }
      duplex = Some(text);
    }
  }
  validate_enums(&cache, &redirect)?;
  let init_headers = match init_object {
    Some(object) => property(scope, object, "headers")?,
    None => None,
  };
  let pairs = if let Some(value) = init_headers {
    read_header_init(scope, Some(value))?
  } else if let Some(source) = source.as_ref() {
    source.headers.clone()
  } else {
    Vec::new()
  };
  let init_body = match init_object {
    Some(object) => property(scope, object, "body")?
      .filter(|value| !value.is_null_or_undefined()),
    None => None,
  };
  let source_has_body = source.as_ref().is_some_and(|request| request.has_body);
  if (init_body.is_some() || source_has_body)
    && matches!(method.as_str(), "GET" | "HEAD")
  {
    return Err(OpError::type_("A GET or HEAD request cannot have a body"));
  }
  let mut took_source_body = false;
  let mut stream_body = None;
  let extracted = if init_body.is_some() {
    extract_body(scope, init_body)?
  } else if source_has_body {
    let source = source.as_ref().unwrap();
    if source.body_used || source.locked {
      return Err(OpError::type_("Body has already been used"));
    }
    took_source_body = true;
    if let Some(bytes) = source.body.clone() {
      Some(Extracted::Bytes(bytes, None))
    } else {
      stream_body = source.stream.clone();
      None
    }
  } else {
    None
  };
  let (bytes, content_type) = match extracted {
    Some(Extracted::Bytes(bytes, mime)) => (Some(bytes), mime),
    Some(Extracted::Stream(object, mime)) => {
      if duplex.as_deref() != Some("half") {
        return Err(OpError::type_("A streamed body requires duplex \"half\""));
      }
      let id = crate::stream::stream_id_of(scope, &object).unwrap_or(0);
      if id == 0
        || crate::stream::is_disturbed(id)
        || crate::stream::is_locked(id)
      {
        return Err(OpError::type_("Body has already been used"));
      }
      stream_body = Some(object);
      (None, mime)
    }
    None => (None, None),
  };
  let signal = request_signal(scope, init_object, source.as_ref())?;
  let (header_object, mime) = guarded_headers(scope, pairs, content_type)?;
  let request = Request {
    method,
    url,
    redirect,
    cache,
    headers: RefCell::new(Some(traced(
      scope,
      v8::Local::new(scope, &header_object),
    ))),
    body: RefCell::new(bytes),
    stream: RefCell::new(None),
    stream_id: Cell::new(0),
    consumed: Cell::new(false),
    mime,
    signal: RefCell::new(Some(traced(scope, v8::Local::new(scope, &signal)))),
  };
  if let Some(stream) = stream_body {
    request.cache_stream(scope, stream);
  }
  if took_source_body {
    disturb_source(scope, input);
  }
  Ok(Built { request })
}

pub(crate) fn validate_priority(value: &str) -> Result<(), OpError> {
  if matches!(value, "high" | "low" | "auto") {
    Ok(())
  } else {
    Err(OpError::type_("The priority is invalid"))
  }
}

struct SourceSnap {
  method: String,
  url: String,
  redirect: String,
  cache: String,
  has_body: bool,
  body_used: bool,
  locked: bool,
  body: Option<Vec<u8>>,
  stream: Option<v8::Global<v8::Object>>,
  headers: Vec<(String, String)>,
  signal: Option<v8::Global<v8::Object>>,
}

fn snapshot_source(
  scope: &mut v8::PinScope,
  input: v8::Local<v8::Value>,
) -> Result<Option<SourceSnap>, OpError> {
  let Some(host) = request_host(scope, input) else {
    return Ok(None);
  };
  let request = host.get();
  let signal = {
    let borrowed = request.signal.borrow();
    borrowed
      .as_ref()
      .and_then(|reference| traced_local(scope, reference))
      .map(|value| v8::Global::new(scope, value))
  };
  let headers = header_pairs(scope, &request.headers);
  Ok(Some(SourceSnap {
    method: request.method.clone(),
    url: request.url.clone(),
    redirect: request.redirect.clone(),
    cache: request.cache.clone(),
    has_body: request.body.borrow().is_some() || request.stream_id.get() != 0,
    body_used: request.body_is_used(),
    locked: crate::stream::is_locked(request.stream_id.get()),
    body: request.body.borrow().clone(),
    stream: request.stream.borrow().as_ref().and_then(|stored| {
      traced_local(scope, stored).map(|object| v8::Global::new(scope, object))
    }),
    headers,
    signal,
  }))
}

fn request_host(
  scope: &mut v8::PinScope,
  input: v8::Local<v8::Value>,
) -> Option<yuzuko_core::op::HostRef<Request>> {
  let object = v8::Local::<v8::Object>::try_from(input).ok()?;
  unsafe { host_ref::<Request>(scope, object) }
}

fn disturb_source(scope: &mut v8::PinScope, input: v8::Local<v8::Value>) {
  let Some(host) = request_host(scope, input) else {
    return;
  };
  host.get().consumed.set(true);
}

fn guarded_headers(
  scope: &mut v8::PinScope,
  pairs: Vec<(String, String)>,
  content_type: Option<String>,
) -> Result<(v8::Global<v8::Object>, String), OpError> {
  let headers =
    Headers::from_pairs(pairs, crate::http::fetch::HeaderGuard::Request)?;
  if let Some(mime) = content_type
    && !headers.contains("content-type")
  {
    headers.append_pair("content-type".to_string(), mime)?;
  }
  let mime = headers
    .snapshot()
    .into_iter()
    .find(|(name, _)| name == "content-type")
    .map(|(_, value)| value)
    .unwrap_or_default();
  let object = unsafe { yuzuko_core::op::wrap_instance(scope, headers) };
  Ok((v8::Global::new(scope, object), mime))
}

fn request_signal(
  scope: &mut v8::PinScope,
  init: Option<v8::Local<v8::Object>>,
  source: Option<&SourceSnap>,
) -> Result<v8::Global<v8::Object>, OpError> {
  let parent = if let Some(object) = init
    && let Some(value) = property(scope, object, "signal")?
  {
    if value.is_null() {
      None
    } else {
      let object = v8::Local::<v8::Object>::try_from(value)
        .map_err(|_| OpError::type_("The abort signal is invalid"))?;
      Some(object)
    }
  } else if let Some(source) = source {
    source
      .signal
      .as_ref()
      .map(|signal| v8::Local::new(scope, signal))
  } else {
    None
  };
  let signal = match parent {
    Some(parent) => dependent_signal(scope, &[parent])?,
    None => dependent_signal(scope, &[])?,
  };
  Ok(v8::Global::new(scope, signal))
}

pub(crate) fn normalize_method(method: &str) -> Result<String, OpError> {
  if method.is_empty()
    || !method.bytes().all(|byte| {
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
    return Err(OpError::type_("The method is invalid"));
  }
  if method.eq_ignore_ascii_case("delete")
    || method.eq_ignore_ascii_case("get")
    || method.eq_ignore_ascii_case("head")
    || method.eq_ignore_ascii_case("options")
    || method.eq_ignore_ascii_case("post")
    || method.eq_ignore_ascii_case("put")
  {
    Ok(method.to_ascii_uppercase())
  } else {
    Ok(method.to_string())
  }
}

pub(crate) fn validate_enums(
  cache: &str,
  redirect: &str,
) -> Result<(), OpError> {
  if !matches!(
    cache,
    "default"
      | "no-store"
      | "reload"
      | "no-cache"
      | "force-cache"
      | "only-if-cached"
  ) {
    return Err(OpError::type_("The request cache mode is invalid"));
  }
  if !matches!(redirect, "follow" | "error" | "manual") {
    return Err(OpError::type_("The request redirect mode is invalid"));
  }
  Ok(())
}

fn resolve_request_url(input: &str) -> Result<String, OpError> {
  let url = if let Ok(url) = url::Url::parse(input) {
    url
  } else {
    let Some(base) = yuzuko_core::entry_url() else {
      return Err(OpError::type_("The URL is invalid"));
    };
    let base = url::Url::parse(&base)
      .map_err(|_| OpError::type_("The URL is invalid"))?;
    base
      .join(input)
      .map_err(|_| OpError::type_("The URL is invalid"))?
  };
  if !url.username().is_empty() || url.password().is_some() {
    return Err(OpError::type_("The URL contains credentials"));
  }
  Ok(url.to_string())
}

fn dependent_signal<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  sources: &[v8::Local<'_, v8::Object>],
) -> Result<v8::Local<'s, v8::Object>, OpError> {
  let rooted = sources
    .iter()
    .map(|source| v8::Global::new(scope, *source))
    .collect::<Vec<_>>();
  let captured = {
    v8::tc_scope!(let tc, scope);
    let locals = rooted
      .iter()
      .map(|source| v8::Local::new(tc, source))
      .collect::<Vec<_>>();
    match call_abort_signal_any(tc, &locals) {
      Some(signal) => Ok(v8::Global::new(tc, signal)),
      None => Err(tc.exception().map(|value| v8::Global::new(tc, value))),
    }
  };
  match captured {
    Ok(signal) => Ok(v8::Local::new(scope, signal)),
    Err(Some(thrown)) => Err(OpError::Thrown(Box::new(thrown))),
    Err(None) => Err(OpError::type_("The abort signal is invalid")),
  }
}

fn call_abort_signal_any<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  sources: &[v8::Local<'s, v8::Object>],
) -> Option<v8::Local<'s, v8::Object>> {
  let global = scope.get_current_context().global(scope);
  let key = v8::String::new(scope, "AbortSignal")?;
  let ctor = global.get(scope, key.into())?;
  let ctor = v8::Local::<v8::Function>::try_from(ctor).ok()?;
  let name = v8::String::new(scope, "any")?;
  let any = ctor
    .get(scope, name.into())
    .and_then(|value| v8::Local::<v8::Function>::try_from(value).ok())?;
  let list = v8::Array::new(scope, sources.len() as i32);
  for (index, source) in sources.iter().enumerate() {
    let value: v8::Local<v8::Value> = (*source).into();
    list.set_index(scope, index as u32, value);
  }
  let list: v8::Local<v8::Value> = list.into();
  let value = any.call(scope, ctor.into(), &[list])?;
  v8::Local::<v8::Object>::try_from(value).ok()
}

fn header_pairs(
  scope: &mut v8::PinScope,
  headers: &RefCell<Option<v8::TracedReference<v8::Object>>>,
) -> Vec<(String, String)> {
  let object = {
    let borrowed = headers.borrow();
    match borrowed.as_ref() {
      Some(reference) => traced_local(scope, reference),
      None => None,
    }
  };
  let Some(object) = object else {
    return Vec::new();
  };
  unsafe { host_ref::<Headers>(scope, object) }
    .map(|headers| headers.get().snapshot())
    .unwrap_or_default()
}

fn property<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  object: v8::Local<v8::Object>,
  name: &str,
) -> Result<Option<v8::Local<'s, v8::Value>>, OpError> {
  let key = v8::String::new(scope, name).unwrap();
  match object.get(scope, key.into()) {
    Some(value) if !value.is_undefined() => Ok(Some(value)),
    Some(_) => Ok(None),
    None => Err(OpError::type_("The request init is invalid")),
  }
}

fn string_value(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<String, OpError> {
  value
    .to_string(scope)
    .map(|text| text.to_rust_string_lossy(scope))
    .ok_or_else(|| OpError::type_("The value is not a string"))
}

fn traced_or_undefined(
  scope: &mut v8::PinScope,
  reference: &RefCell<Option<v8::TracedReference<v8::Object>>>,
) -> Js {
  let value = reference
    .borrow()
    .as_ref()
    .and_then(|reference| traced_local(scope, reference))
    .map(v8::Local::<v8::Value>::from)
    .unwrap_or_else(|| v8::undefined(scope).into());
  Js(v8::Global::new(scope, value))
}

fn resolve(scope: &mut v8::PinScope, value: v8::Local<v8::Value>) -> Js {
  let resolver = v8::PromiseResolver::new(scope).unwrap();
  resolver.resolve(scope, value);
  let promise = resolver.get_promise(scope);
  let value: v8::Local<v8::Value> = promise.into();
  Js(v8::Global::new(scope, value))
}

fn follow_request_text(
  scope: &mut v8::PinScope,
  promise: v8::Global<v8::Promise>,
) -> Js {
  let resolver = v8::PromiseResolver::new(scope).unwrap();
  let outer = resolver.get_promise(scope);
  let stored = v8::Global::new(scope, resolver);
  crate::stream::when_stream_bytes(scope, promise, move |scope, result| {
    let resolver = v8::Local::new(scope, &stored);
    match result {
      Ok(bytes) => {
        let text = crate::blob::decode_utf8(&bytes);
        let text = v8::String::new(scope, &text).unwrap();
        resolver.resolve(scope, text.into());
      }
      Err(error) => {
        let error = v8::Local::new(scope, &error);
        resolver.reject(scope, error);
      }
    }
  });
  let value: v8::Local<v8::Value> = outer.into();
  Js(v8::Global::new(scope, value))
}

fn resolve_text(scope: &mut v8::PinScope, bytes: Vec<u8>) -> Js {
  let text = crate::blob::decode_utf8(&bytes);
  let text = v8::String::new(scope, &text).unwrap();
  let value: v8::Local<v8::Value> = text.into();
  resolve(scope, value)
}

fn reject_syntax(scope: &mut v8::PinScope) -> Js {
  let resolver = v8::PromiseResolver::new(scope).unwrap();
  let message = v8::String::new(scope, "The response is not JSON").unwrap();
  resolver.reject(scope, v8::Exception::syntax_error(scope, message));
  let promise = resolver.get_promise(scope);
  let value: v8::Local<v8::Value> = promise.into();
  Js(v8::Global::new(scope, value))
}

fn reject(scope: &mut v8::PinScope, error: OpError) -> Js {
  let resolver = v8::PromiseResolver::new(scope).unwrap();
  let (name, message) = match error {
    OpError::Dom { name, message } => (name, message),
    OpError::Range(message) => ("RangeError".to_string(), message),
    OpError::Type(message) => ("TypeError".to_string(), message),
    OpError::Thrown(_) => {
      ("TypeError".to_string(), "The operation failed".to_string())
    }
  };
  let exception = if name == "TypeError" {
    let text = v8::String::new(scope, &message).unwrap();
    v8::Exception::type_error(scope, text)
  } else if name == "RangeError" {
    let text = v8::String::new(scope, &message).unwrap();
    v8::Exception::range_error(scope, text)
  } else {
    yuzuko_core::op::new_dom_exception(scope, &name, &message)
      .map(v8::Local::<v8::Value>::from)
      .unwrap_or_else(|| {
        let text = v8::String::new(scope, &message).unwrap();
        v8::Exception::error(scope, text)
      })
  };
  resolver.reject(scope, exception);
  let promise = resolver.get_promise(scope);
  let value: v8::Local<v8::Value> = promise.into();
  Js(v8::Global::new(scope, value))
}

pub fn install(scope: &mut v8::PinScope) {
  Request::install(scope);
}
