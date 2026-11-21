//! Request state for `XMLHttpRequest`.
//!
//! The JavaScript class lives in `yuzuko_dom` and is an `EventTarget`.
//! `open`, `setRequestHeader`, `send`, `abort`, `timeout`, `readyState`,
//! `overrideMimeType`, and `getResponseHeader` follow the XHR specification.
//! Forbidden request headers stay allowed under the fetch posture.
//! `CONNECT`, `TRACE`, and `TRACK` throw `SecurityError`. `open` parses the
//! URL and stores a username and password when the URL has a host. An
//! asynchronous `send` collects the body on this thread and leaves the state
//! `OPENED` with `send()` invoked. The caller publishes `DONE` from a later
//! task. A synchronous `send` publishes `DONE` before it returns. A
//! synchronous failure throws and does not dispatch. `responseText` uses the
//! legacy decode
//! hook: the `Content-Type` charset, replaced by `overrideMimeType` when that
//! MIME type has a charset, then a BOM sniff. An XML charset prescan is not
//! done.

use std::cell::Cell;
use std::cell::RefCell;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::thread::JoinHandle;
use std::time::Duration;
use std::time::Instant;

use yuzuko_core::op::OpError;
use yuzuko_core::op::host_ref;

use crate::http::BodyNote;
use crate::http::HttpFailure;
use crate::http::fetch::header_list_value;
use crate::http::fetch::normalize_header;
use crate::http::fetch::normalize_value;

const UNSENT: u8 = 0;
const OPENED: u8 = 1;
const DONE: u8 = 4;

/// Counts for the terminal `load` or `error` event queued after `send`.
#[derive(Debug)]
pub struct XhrProgress {
  pub type_name: &'static str,
  pub length_computable: bool,
  pub loaded: f64,
  pub total: f64,
}

fn progress_note(type_name: &'static str, bytes: usize) -> XhrProgress {
  let success = type_name == "load";
  let total = if success { bytes as f64 } else { 0.0 };
  XhrProgress {
    type_name,
    length_computable: success && bytes != 0,
    loaded: total,
    total,
  }
}

pub struct XhrState {
  state: Cell<u8>,
  sent: Cell<bool>,
  /// Bumped when `open`, `abort`, or a late `timeout` drops the queued task.
  generation: Cell<u64>,
  timeout: Cell<u32>,
  /// `open(..., false)`. A synchronous `send` settles before it returns.
  synchronous: Cell<bool>,
  started: Cell<Option<Instant>>,
  /// Terminal event waiting for the completion task.
  pending: RefCell<Option<XhrProgress>>,
  status_code: Cell<u16>,
  method: RefCell<String>,
  url: RefCell<url::Url>,
  status_text: RefCell<String>,
  headers: RefCell<Vec<(String, String)>>,
  response_headers: RefCell<Vec<(String, String)>>,
  response_body: RefCell<Vec<u8>>,
  /// Parsed override MIME type. `open` does not clear it.
  override_mime: RefCell<Option<crate::text::mime::Mime>>,
}

/// Bytes `send` transmits, plus the `Content-Type` extracted from the value.
pub struct XhrBody {
  bytes: Vec<u8>,
  content_type: Option<String>,
  /// A string or `URLSearchParams` rewrites a non-UTF-8 author charset.
  rewrite_charset: bool,
}

/// What the caller does after `send` has collected the response.
#[derive(Debug)]
pub enum XhrSend {
  /// Asynchronous. Queue a task that publishes this generation.
  Queued(u64),
  /// Synchronous. `readyState` is already `DONE`. Dispatch these events.
  Finished(u64, XhrProgress),
}

impl XhrState {
  pub fn new() -> Self {
    Self {
      state: Cell::new(UNSENT),
      sent: Cell::new(false),
      generation: Cell::new(0),
      timeout: Cell::new(0),
      synchronous: Cell::new(false),
      started: Cell::new(None),
      pending: RefCell::new(None),
      status_code: Cell::new(0),
      method: RefCell::new("GET".to_string()),
      url: RefCell::new(url::Url::parse("data:,").expect("data url")),
      status_text: RefCell::new(String::new()),
      headers: RefCell::new(Vec::new()),
      response_headers: RefCell::new(Vec::new()),
      response_body: RefCell::new(Vec::new()),
      override_mime: RefCell::new(None),
    }
  }

  pub fn ready_state(&self) -> u8 {
    self.state.get()
  }

  pub fn timeout(&self) -> u32 {
    self.timeout.get()
  }

  /// Stores `timeout`. When `send()` has been invoked and the new value has
  /// already elapsed, settles the request as `timeout` and returns true.
  /// The caller dispatches `readystatechange`, `timeout`, and `loadend`.
  pub fn set_timeout(&self, value: u32) -> bool {
    self.timeout.set(value);
    if value == 0 || self.state.get() != OPENED || !self.sent.get() {
      return false;
    }
    let Some(started) = self.started.get() else {
      return false;
    };
    if started.elapsed() < Duration::from_millis(u64::from(value)) {
      return false;
    }
    self.fail_request();
    true
  }

  pub fn override_mime_type(&self, mime: &str) -> Result<(), OpError> {
    if self.state.get() == DONE {
      return Err(OpError::dom("InvalidStateError", "The request is done"));
    }
    *self.override_mime.borrow_mut() = Some(parsed_override(mime));
    Ok(())
  }

  /// Returns whether `readyState` changed, so the caller can dispatch
  /// `readystatechange`. A request that is already `OPENED` stays `OPENED`.
  ///
  /// `asynchronous` false settles inside `send`. `username` and `password`
  /// are written when the parsed URL has a host and is not `file:`.
  pub fn open(
    &self,
    method: &str,
    url: &str,
    asynchronous: bool,
    username: Option<&str>,
    password: Option<&str>,
  ) -> Result<bool, OpError> {
    let method = xhr_method(method)?;
    let mut url = parse_xhr_url(url)?;
    apply_userinfo(&mut url, username, password);
    *self.method.borrow_mut() = method;
    *self.url.borrow_mut() = url;
    self.headers.borrow_mut().clear();
    self.sent.set(false);
    self.started.set(None);
    self.synchronous.set(!asynchronous);
    self.bump();
    self.clear_response();
    let fire = self.state.get() != OPENED;
    self.state.set(OPENED);
    Ok(fire)
  }

  /// `InvalidStateError` when `send` cannot start. `true` when the method
  /// carries a body. `GET` and `HEAD` do not.
  pub fn carries_body(&self) -> Result<bool, OpError> {
    if self.state.get() != OPENED || self.sent.get() {
      return Err(OpError::dom("InvalidStateError", "The request is not open"));
    }
    let method = self.method.borrow();
    Ok(method.as_str() != "GET" && method.as_str() != "HEAD")
  }

  pub fn set_request_header(
    &self,
    name: &str,
    value: &str,
  ) -> Result<(), OpError> {
    if self.state.get() != OPENED || self.sent.get() {
      return Err(OpError::dom("InvalidStateError", "The request is not open"));
    }
    let name = normalize_header(name)
      .map_err(|_| OpError::dom("SyntaxError", "The header name is invalid"))?;
    let value = normalize_value(value).map_err(|_| {
      OpError::dom("SyntaxError", "The header value is invalid")
    })?;
    let mut headers = self.headers.borrow_mut();
    if let Some((_, existing)) =
      headers.iter_mut().find(|(key, _)| key == &name)
    {
      existing.push_str(", ");
      existing.push_str(&value);
    } else {
      headers.push((name, value));
    }
    Ok(())
  }

  /// Loads the response on this thread. An asynchronous request stays
  /// `OPENED` and hides the response until the caller publishes `DONE`.
  /// A synchronous request publishes `DONE` here, or throws `NetworkError`
  /// or `TimeoutError` without an event.
  pub fn send(&self, body: Option<XhrBody>) -> Result<XhrSend, OpError> {
    if self.state.get() != OPENED || self.sent.get() {
      return Err(OpError::dom("InvalidStateError", "The request is not open"));
    }
    let method = self.method.borrow().clone();
    let body = if method == "GET" || method == "HEAD" {
      None
    } else {
      body
    };
    if let Some(body) = &body {
      apply_content_type(
        &mut self.headers.borrow_mut(),
        body.content_type.as_deref(),
        body.rewrite_charset,
      );
    }
    let bytes = body.map(|body| body.bytes).unwrap_or_default();
    self.sent.set(true);
    self.started.set(Some(Instant::now()));
    let generation = self.generation.get();
    match self.load(&bytes) {
      Ok(note) => {
        let note = if self.deadline_exceeded() {
          self.clear_response();
          progress_note("timeout", 0)
        } else {
          note
        };
        if self.synchronous.get() {
          return self.finish_sync(generation, note);
        }
        *self.pending.borrow_mut() = Some(note);
        Ok(XhrSend::Queued(generation))
      }
      Err(error) => {
        self.sent.set(false);
        self.started.set(None);
        Err(error)
      }
    }
  }

  fn finish_sync(
    &self,
    generation: u64,
    note: XhrProgress,
  ) -> Result<XhrSend, OpError> {
    match note.type_name {
      "timeout" => {
        self.clear_response();
        self.sent.set(false);
        self.state.set(DONE);
        Err(OpError::dom("TimeoutError", "The request timed out"))
      }
      "error" => {
        self.clear_response();
        self.sent.set(false);
        self.state.set(DONE);
        Err(OpError::dom("NetworkError", "The request failed"))
      }
      _ => {
        *self.pending.borrow_mut() = Some(note);
        let Some(published) = self.publish(generation) else {
          return Err(OpError::type_("The request failed"));
        };
        Ok(XhrSend::Finished(generation, published))
      }
    }
  }

  /// Publishes a still-current `send` and returns its terminal event.
  /// `readyState` is `DONE` afterwards. `status` and `responseText` stay
  /// empty unless the event is `load`.
  pub fn publish(&self, generation: u64) -> Option<XhrProgress> {
    if self.generation.get() != generation || !self.sent.get() {
      return None;
    }
    let note = self.pending.borrow_mut().take()?;
    self.sent.set(false);
    if note.type_name != "load" {
      self.clear_response();
    }
    self.state.set(DONE);
    Some(note)
  }

  /// The completion task keeps dispatching while this generation is current.
  /// `abort()` of a finished request does not bump it, so `loadend` still runs.
  /// `open()` does bump it and the rest of that task stops.
  pub fn is_current(&self, generation: u64) -> bool {
    self.generation.get() == generation
  }

  /// `true` when the caller should dispatch `readystatechange`, `abort`, and
  /// `loadend` while `readyState` is `DONE`, then call `finish_abort`.
  pub fn begin_abort(&self) -> bool {
    if self.state.get() == OPENED && self.sent.get() {
      self.fail_request();
      return true;
    }
    if self.state.get() == DONE {
      self.state.set(UNSENT);
      self.clear_response();
    }
    false
  }

  /// The `abort()` step that follows the abort events. A listener that
  /// already left `DONE` keeps the state it set.
  pub fn finish_abort(&self) {
    if self.state.get() == DONE {
      self.state.set(UNSENT);
      self.clear_response();
    }
  }

  fn load(&self, body: &[u8]) -> Result<XhrProgress, OpError> {
    let url = self.url.borrow().clone();
    yuzuko_core::authorize_url(&url)?;
    if url.scheme() == "data" {
      return Ok(finish_data(self, &url));
    }
    let method = self.method.borrow().clone();
    let mut headers = self.headers.borrow().clone();
    attach_authorization(&mut headers, &url);
    let cancel = Arc::new(AtomicBool::new(false));
    let _guard = TimeoutGuard::arm(self.timeout.get(), &cancel);
    match crate::http::perform(
      method,
      without_credentials(&url),
      headers,
      body.to_vec(),
      "follow".to_string(),
      Arc::clone(&cancel),
    ) {
      Ok(response) => {
        match collect_body(&response.body, self.deadline(), &cancel) {
          Ok(body) => {
            let note = progress_note("load", body.len());
            *self.response_body.borrow_mut() = body;
            *self.response_headers.borrow_mut() = response.headers;
            self.status_code.set(response.status);
            *self.status_text.borrow_mut() = response.status_text;
            Ok(note)
          }
          Err(()) => {
            self.clear_response();
            Ok(progress_note("timeout", 0))
          }
        }
      }
      Err(HttpFailure::Abort) if self.timeout.get() != 0 => {
        self.clear_response();
        Ok(progress_note("timeout", 0))
      }
      Err(HttpFailure::Abort | HttpFailure::Network(_)) => {
        self.clear_response();
        Ok(progress_note("error", 0))
      }
      Err(HttpFailure::Type(message)) => Err(OpError::type_(message)),
      Err(HttpFailure::Denied { name, message }) => {
        Err(OpError::dom(name, message))
      }
    }
  }

  fn deadline(&self) -> Option<Instant> {
    let limit = self.timeout.get();
    if limit == 0 {
      return None;
    }
    self
      .started
      .get()
      .map(|started| started + Duration::from_millis(u64::from(limit)))
  }

  fn deadline_exceeded(&self) -> bool {
    self
      .deadline()
      .is_some_and(|deadline| Instant::now() >= deadline)
  }

  fn fail_request(&self) {
    self.clear_response();
    self.sent.set(false);
    self.bump();
    self.state.set(DONE);
  }

  fn bump(&self) {
    self.generation.set(self.generation.get().wrapping_add(1));
  }

  fn clear_response(&self) {
    self.pending.borrow_mut().take();
    self.response_body.borrow_mut().clear();
    self.response_headers.borrow_mut().clear();
    self.status_code.set(0);
    self.status_text.borrow_mut().clear();
  }

  pub fn response_header(&self, name: &str) -> Result<Option<String>, OpError> {
    if self.state.get() != DONE {
      return Ok(None);
    }
    let name = normalize_header(name)?;
    if name == "set-cookie" || name == "set-cookie2" {
      return Ok(None);
    }
    let headers = self.response_headers.borrow();
    if !headers.iter().any(|(key, _)| key == &name) {
      return Ok(None);
    }
    Ok(Some(header_list_value(&headers, &name)))
  }

  pub fn status(&self) -> f64 {
    if self.state.get() != DONE {
      return 0.0;
    }
    f64::from(self.status_code.get())
  }

  pub fn status_text(&self) -> String {
    if self.state.get() != DONE {
      return String::new();
    }
    self.status_text.borrow().clone()
  }

  pub fn response_text(&self) -> String {
    if self.state.get() != DONE {
      return String::new();
    }
    decode_xhr_text(
      &self.response_body.borrow(),
      &self.response_headers.borrow(),
      self.override_mime.borrow().as_ref(),
    )
  }

  pub fn all_response_headers(&self) -> String {
    if self.state.get() != DONE {
      return String::new();
    }
    let headers = self.response_headers.borrow();
    let mut names = Vec::new();
    for (name, _) in headers.iter() {
      if name == "set-cookie" || name == "set-cookie2" {
        continue;
      }
      if !names.iter().any(|stored: &String| stored == name) {
        names.push(name.clone());
      }
    }
    names.sort();
    let mut out = String::new();
    for name in names {
      out.push_str(&name);
      out.push_str(": ");
      out.push_str(&header_list_value(&headers, &name));
      out.push_str("\r\n");
    }
    out
  }
}

impl Default for XhrState {
  fn default() -> Self {
    Self::new()
  }
}

/// ByteString failures are `TypeError`. Forbidden methods are `SecurityError`
/// before normalization. Any other token failure is `SyntaxError`. `fetch`
/// reports every token failure as `TypeError` and does not reject these methods.
fn xhr_method(method: &str) -> Result<String, OpError> {
  if method.chars().any(|ch| u32::from(ch) > 0x00FF) {
    return Err(OpError::type_("The method is invalid"));
  }
  if method.eq_ignore_ascii_case("connect")
    || method.eq_ignore_ascii_case("trace")
    || method.eq_ignore_ascii_case("track")
  {
    return Err(OpError::dom("SecurityError", "The method is forbidden"));
  }
  crate::http::request::normalize_method(method)
    .map_err(|_| OpError::dom("SyntaxError", "The method is invalid"))
}

/// Accepts a URL that contains credentials. `fetch` rejects those.
fn parse_xhr_url(input: &str) -> Result<url::Url, OpError> {
  if let Ok(url) = url::Url::parse(input) {
    return Ok(url);
  }
  let Some(base) = yuzuko_core::try_entry_url() else {
    return Err(OpError::dom("SyntaxError", "The URL is invalid"));
  };
  let base = url::Url::parse(&base)
    .map_err(|_| OpError::dom("SyntaxError", "The URL is invalid"))?;
  base
    .join(input)
    .map_err(|_| OpError::dom("SyntaxError", "The URL is invalid"))
}

/// `file:` and a URL with no host ignore both arguments. The `url` crate
/// cannot store an empty password, so that argument removes one instead.
fn apply_userinfo(
  url: &mut url::Url,
  username: Option<&str>,
  password: Option<&str>,
) {
  if url.host().is_none() || url.scheme() == "file" {
    return;
  }
  if let Some(username) = username {
    let _ = url.set_username(username);
  }
  if let Some(password) = password {
    let _ = url.set_password(Some(password));
  }
}

fn apply_content_type(
  headers: &mut Vec<(String, String)>,
  content_type: Option<&str>,
  rewrite_charset: bool,
) {
  if let Some((_, value)) =
    headers.iter_mut().find(|(name, _)| name == "content-type")
  {
    if rewrite_charset && let Some(rewritten) = force_utf8_charset(value) {
      *value = rewritten;
    }
    return;
  }
  if let Some(content_type) = content_type {
    headers.push(("content-type".to_string(), content_type.to_string()));
  }
}

/// `Some` when `value` has a charset parameter other than UTF-8.
fn force_utf8_charset(value: &str) -> Option<String> {
  let mut mime = crate::text::mime::parse(value)?;
  let charset = mime.parameter("charset")?.to_string();
  if charset.eq_ignore_ascii_case("utf-8") {
    return None;
  }
  mime.set_parameter("charset", "UTF-8");
  Some(crate::text::mime::serialize(&mime))
}

fn attach_authorization(headers: &mut Vec<(String, String)>, url: &url::Url) {
  if headers.iter().any(|(name, _)| name == "authorization") {
    return;
  }
  if let Some(value) = basic_authorization(url) {
    headers.push(("authorization".to_string(), value));
  }
}

/// `Basic` of the percent-decoded userinfo. An empty username and a null
/// password produce no header. A null password is sent as an empty password.
fn basic_authorization(url: &url::Url) -> Option<String> {
  let username = percent_decode(url.username());
  let password = url.password().map(percent_decode);
  if username.is_empty() && password.is_none() {
    return None;
  }
  let mut raw = username.into_bytes();
  raw.push(b':');
  raw.extend(password.unwrap_or_default().into_bytes());
  let encoded = crate::text::base64::encode(&raw);
  Some(format!("Basic {encoded}"))
}

/// The URL passed to the HTTP client. Userinfo is not a request target.
fn without_credentials(url: &url::Url) -> String {
  let mut stripped = url.clone();
  let _ = stripped.set_password(None);
  let _ = stripped.set_username("");
  stripped.to_string()
}

fn percent_decode(input: &str) -> String {
  let bytes = input.as_bytes();
  let mut out = Vec::new();
  let mut index = 0;
  while index < bytes.len() {
    if bytes[index] == b'%'
      && index + 2 < bytes.len()
      && let (Some(high), Some(low)) =
        (hex_digit(bytes[index + 1]), hex_digit(bytes[index + 2]))
    {
      out.push((high << 4) | low);
      index += 3;
      continue;
    }
    out.push(bytes[index]);
    index += 1;
  }
  String::from_utf8_lossy(&out).into_owned()
}

fn hex_digit(byte: u8) -> Option<u8> {
  match byte {
    b'0'..=b'9' => Some(byte - b'0'),
    b'a'..=b'f' => Some(byte - b'a' + 10),
    b'A'..=b'F' => Some(byte - b'A' + 10),
    _ => None,
  }
}

/// `XMLHttpRequestBodyInit`. `null` and `undefined` are no body. `Document`
/// is not implemented. A platform object outside this union throws, and so
/// does a shared or detached buffer. `GET` and `HEAD` do not call this.
pub fn extract_xhr_body(
  scope: &mut v8::PinScope,
  body: Option<v8::Local<v8::Value>>,
) -> Result<Option<XhrBody>, OpError> {
  let Some(body) = body.filter(|value| !value.is_null_or_undefined()) else {
    return Ok(None);
  };
  if let Ok(object) = v8::Local::<v8::Object>::try_from(body) {
    if let Some(form) =
      unsafe { host_ref::<crate::http::form::FormData>(scope, object) }
    {
      let (bytes, mime) = crate::http::form::encode(form.get());
      return Ok(Some(XhrBody {
        bytes,
        content_type: Some(mime),
        rewrite_charset: false,
      }));
    }
    if let Some((bytes, mime)) = crate::blob::blob_parts(scope, body)? {
      return Ok(Some(XhrBody {
        bytes,
        content_type: (!mime.is_empty()).then_some(mime),
        rewrite_charset: false,
      }));
    }
    if let Some(params) =
      unsafe { host_ref::<crate::url::UrlSearchParams>(scope, object) }
    {
      return Ok(Some(XhrBody {
        bytes: params.get().serialized().into_bytes(),
        content_type: Some(
          "application/x-www-form-urlencoded;charset=UTF-8".to_string(),
        ),
        rewrite_charset: true,
      }));
    }
  }
  // `SharedArrayBuffer` reports `is_api_wrapper` in this V8 build. Buffer
  // checks come first so it is not rejected as an unrelated platform object.
  if shared_buffer(body) {
    return Err(OpError::type_("The buffer is shared"));
  }
  if detached_buffer(scope, body) {
    return Err(OpError::type_("The buffer is detached."));
  }
  if let Some(bytes) = crate::bytes::buffer_source(scope, body)? {
    return Ok(Some(XhrBody {
      bytes,
      content_type: None,
      rewrite_charset: false,
    }));
  }
  if let Ok(object) = v8::Local::<v8::Object>::try_from(body)
    && object.is_api_wrapper()
  {
    return Err(OpError::type_(
      "The provided value is not of type 'XMLHttpRequestBodyInit'.",
    ));
  }
  let text = {
    v8::tc_scope!(let tc, scope);
    match body.to_string(tc) {
      Some(text) => Ok(text.to_rust_string_lossy(tc)),
      None => Err(tc.exception().map(|value| v8::Global::new(tc, value))),
    }
  };
  match text {
    Ok(text) => Ok(Some(XhrBody {
      bytes: text.into_bytes(),
      content_type: Some("text/plain;charset=UTF-8".to_string()),
      rewrite_charset: true,
    })),
    Err(Some(thrown)) => Err(OpError::Thrown(Box::new(thrown))),
    Err(None) => Err(OpError::type_(
      "The provided value is not of type 'XMLHttpRequestBodyInit'.",
    )),
  }
}

fn shared_buffer(value: v8::Local<v8::Value>) -> bool {
  value.is_shared_array_buffer()
    || v8::Local::<v8::ArrayBufferView>::try_from(value)
      .ok()
      .and_then(|view| view.get_backing_store())
      .is_some_and(|store| store.is_shared())
}

fn detached_buffer(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> bool {
  if let Ok(view) = v8::Local::<v8::ArrayBufferView>::try_from(value) {
    return view
      .buffer(scope)
      .is_some_and(|buffer| buffer.was_detached());
  }
  if let Ok(buffer) = v8::Local::<v8::ArrayBuffer>::try_from(value) {
    return buffer.was_detached();
  }
  false
}

fn parsed_override(mime: &str) -> crate::text::mime::Mime {
  crate::text::mime::parse(mime).unwrap_or_else(|| {
    crate::text::mime::parse("application/octet-stream").expect("octet-stream")
  })
}

fn response_mime(headers: &[(String, String)]) -> crate::text::mime::Mime {
  let xml = || crate::text::mime::parse("text/xml").expect("text/xml");
  let present = headers.iter().any(|(name, _)| name == "content-type");
  if !present {
    return xml();
  }
  crate::text::mime::parse(&header_list_value(headers, "content-type"))
    .unwrap_or_else(xml)
}

/// Legacy Encoding Standard decode used by `responseText`.
///
/// `Encoding::for_label` keeps the replacement encoding. `Encoding::decode`
/// sniffs a leading BOM and then decodes with replacement.
fn decode_xhr_text(
  bytes: &[u8],
  headers: &[(String, String)],
  override_mime: Option<&crate::text::mime::Mime>,
) -> String {
  let mut label = response_mime(headers)
    .parameter("charset")
    .map(str::to_owned);
  if let Some(mime) = override_mime
    && let Some(charset) = mime.parameter("charset")
  {
    label = Some(charset.to_owned());
  }
  let encoding = label
    .as_deref()
    .and_then(|label| encoding_rs::Encoding::for_label(label.as_bytes()))
    .unwrap_or(encoding_rs::UTF_8);
  encoding.decode(bytes).0.into_owned()
}

fn finish_data(xhr: &XhrState, url: &url::Url) -> XhrProgress {
  match crate::text::mime::data_url(url) {
    Some((bytes, mime)) => {
      let note = progress_note("load", bytes.len());
      *xhr.response_body.borrow_mut() = bytes;
      *xhr.response_headers.borrow_mut() =
        vec![("content-type".to_string(), mime)];
      xhr.status_code.set(200);
      *xhr.status_text.borrow_mut() = "OK".to_string();
      note
    }
    None => {
      xhr.response_body.borrow_mut().clear();
      xhr.response_headers.borrow_mut().clear();
      xhr.status_code.set(0);
      xhr.status_text.borrow_mut().clear();
      progress_note("error", 0)
    }
  }
}

/// `Err` means the XHR timeout elapsed. The five-second cap still returns
/// the bytes collected so far when no timeout is set.
fn collect_body(
  body: &crate::http::HttpBody,
  deadline: Option<Instant>,
  cancel: &AtomicBool,
) -> Result<Vec<u8>, ()> {
  let start = Instant::now();
  let mut output = Vec::new();
  loop {
    if cancel.load(Ordering::Relaxed)
      || deadline.is_some_and(|limit| Instant::now() >= limit)
    {
      cancel.store(true, Ordering::Relaxed);
      body.stop();
      return Err(());
    }
    match body.poll() {
      Some(BodyNote::End) | Some(BodyNote::Failed(_)) => return Ok(output),
      Some(BodyNote::Chunk(bytes)) => output.extend(bytes),
      None => {
        if start.elapsed() > Duration::from_secs(5) {
          return Ok(output);
        }
        std::thread::sleep(Duration::from_millis(1));
      }
    }
  }
}

/// Sets `cancel` if `timeout_ms` elapses before `send` finishes the fetch.
struct TimeoutGuard {
  stop: Option<mpsc::Sender<()>>,
  worker: Option<JoinHandle<()>>,
}

impl TimeoutGuard {
  fn arm(timeout_ms: u32, cancel: &Arc<AtomicBool>) -> Self {
    if timeout_ms == 0 {
      return Self {
        stop: None,
        worker: None,
      };
    }
    let (sender, receiver) = mpsc::channel();
    let cancel = Arc::clone(cancel);
    let limit = Duration::from_millis(u64::from(timeout_ms));
    let worker = std::thread::spawn(move || {
      if receiver.recv_timeout(limit).is_err() {
        cancel.store(true, Ordering::Relaxed);
      }
    });
    Self {
      stop: Some(sender),
      worker: Some(worker),
    }
  }
}

impl Drop for TimeoutGuard {
  fn drop(&mut self) {
    if let Some(stop) = self.stop.take() {
      let _ = stop.send(());
    }
    if let Some(worker) = self.worker.take() {
      let _ = worker.join();
    }
  }
}

#[cfg(test)]
mod tests {
  use yuzuko_core::op::OpError;

  use super::decode_xhr_text;
  use crate::text::mime::parse;

  fn headers(value: &str) -> Vec<(String, String)> {
    vec![("content-type".to_string(), value.to_string())]
  }

  fn open_async(xhr: &super::XhrState, method: &str, url: &str) -> bool {
    xhr.open(method, url, true, None, None).unwrap()
  }

  fn queued(xhr: &super::XhrState) -> u64 {
    match xhr.send(None).unwrap() {
      super::XhrSend::Queued(generation) => generation,
      super::XhrSend::Finished(_, _) => panic!("synchronous send"),
    }
  }

  fn expect_dom(error: OpError, name: &str, message: &str) {
    match error {
      OpError::Dom {
        name: got,
        message: got_message,
      } => {
        assert_eq!(got, name);
        assert_eq!(got_message, message);
      }
      other => panic!("expected {name}, got {other:?}"),
    }
  }

  fn text_body(bytes: &str, rewrite: bool) -> super::XhrBody {
    super::XhrBody {
      bytes: bytes.as_bytes().to_vec(),
      content_type: Some("text/plain;charset=UTF-8".to_string()),
      rewrite_charset: rewrite,
    }
  }

  #[test]
  fn replacement_override_decodes_to_one_character() {
    let mime = parse("text/plain; charset=\"iso-2022-kr\"").unwrap();
    let text = decode_xhr_text(&[0x41, 0x42, 0xA0], &[], Some(&mime));
    assert_eq!(text, "\u{FFFD}");
    assert_eq!(decode_xhr_text(&[], &[], Some(&mime)), "");
  }

  #[test]
  fn unknown_label_is_utf8_and_a_bom_switches_encoding() {
    let utf7 = parse("text/plain; charset=\"utf-7\"").unwrap();
    assert_eq!(decode_xhr_text(b"+AD4", &[], Some(&utf7)), "+AD4");
    let utf32 = parse("text/plain; charset=UTF-32").unwrap();
    let bom = [
      0xFF, 0xFE, 0x00, 0x00, 0x41, 0x00, 0x00, 0x00, 0x42, 0x00, 0x00, 0x00,
    ];
    assert_eq!(
      decode_xhr_text(&bom, &[], Some(&utf32)),
      "\u{0000}\u{0041}\u{0000}\u{0042}\u{0000}"
    );
    let no_bom = [0x41, 0x00, 0x00, 0x00, 0x42, 0x00, 0x00, 0xC2, 0x80];
    assert_eq!(
      decode_xhr_text(&no_bom, &[], Some(&utf32)),
      "\u{0041}\u{0000}\u{0000}\u{0000}\u{0042}\u{0000}\u{0000}\u{0080}"
    );
  }

  #[test]
  fn override_charset_replaces_the_content_type() {
    let over = parse("text/plain; charset=windows-1252").unwrap();
    let text = decode_xhr_text(
      &[0x80],
      &headers("text/plain;charset=ibm866"),
      Some(&over),
    );
    assert_eq!(text, "\u{20AC}");
    let ibm =
      decode_xhr_text(&[0x80], &headers("text/plain;charset=ibm866"), None);
    assert_ne!(ibm, "\u{20AC}");
  }

  #[test]
  fn open_fires_only_when_the_state_changes() {
    let xhr = super::XhrState::new();
    assert_eq!(xhr.ready_state(), 0);
    assert!(!xhr.begin_abort());
    assert_eq!(xhr.ready_state(), 0);
    assert!(open_async(&xhr, "GET", "data:text/plain,hi"));
    assert_eq!(xhr.ready_state(), 1);
    assert!(!open_async(&xhr, "GET", "data:text/plain,yo"));
    assert!(!xhr.begin_abort());
    assert_eq!(xhr.ready_state(), 1);
  }

  #[test]
  fn a_timeout_longer_than_a_data_url_still_loads() {
    let xhr = super::XhrState::new();
    open_async(&xhr, "GET", "data:text/plain,hi");
    assert!(!xhr.set_timeout(1000));
    let generation = queued(&xhr);
    assert_eq!(xhr.ready_state(), 1);
    assert_eq!(xhr.response_text(), "");
    let note = xhr.publish(generation).unwrap();
    assert_eq!(note.type_name, "load");
    assert_eq!(xhr.ready_state(), 4);
    assert_eq!(xhr.response_text(), "hi");
  }

  #[test]
  fn abort_after_send_drops_the_queued_load() {
    let xhr = super::XhrState::new();
    open_async(&xhr, "GET", "data:text/plain,hi");
    let generation = queued(&xhr);
    assert!(xhr.begin_abort());
    assert_eq!(xhr.ready_state(), 4);
    assert_eq!(xhr.response_text(), "");
    xhr.finish_abort();
    assert_eq!(xhr.ready_state(), 0);
    assert!(xhr.publish(generation).is_none());
  }

  #[test]
  fn a_timeout_assigned_after_send_drops_the_queued_load() {
    let xhr = super::XhrState::new();
    open_async(&xhr, "GET", "data:text/plain,hi");
    let generation = queued(&xhr);
    std::thread::sleep(std::time::Duration::from_millis(5));
    assert!(xhr.set_timeout(1));
    assert_eq!(xhr.ready_state(), 4);
    assert_eq!(xhr.response_text(), "");
    assert!(xhr.publish(generation).is_none());
    assert!(!xhr.set_timeout(1));
  }

  #[test]
  fn data_url_body_uses_the_override_charset() {
    let url = url::Url::parse("data:text/plain,%41%42%43%A0").unwrap();
    let (bytes, mime) = crate::text::mime::data_url(&url).unwrap();
    assert_eq!(mime, "text/plain");
    let over = parse("text/plain; charset=replacement").unwrap();
    let text = decode_xhr_text(&bytes, &headers(&mime), Some(&over));
    assert_eq!(text, "\u{FFFD}");
  }

  #[test]
  fn a_rejected_open_keeps_the_previous_request() {
    let xhr = super::XhrState::new();
    assert!(open_async(&xhr, "GET", "data:text/plain,hi"));
    xhr.set_request_header("X-Test", "1").unwrap();
    for method in ["TRACE", "trace", "CONNECT", "TRACK", "CoNnEcT"] {
      expect_dom(
        xhr
          .open(method, "https://example.test/no", true, None, None)
          .unwrap_err(),
        "SecurityError",
        "The method is forbidden",
      );
      assert_eq!(xhr.ready_state(), 1);
      assert_eq!(xhr.url.borrow().as_str(), "data:text/plain,hi");
      assert_eq!(xhr.headers.borrow().len(), 1);
    }
    expect_dom(
      xhr.open("", "data:,", true, None, None).unwrap_err(),
      "SyntaxError",
      "The method is invalid",
    );
    expect_dom(
      xhr.open("GET", "http://[", true, None, None).unwrap_err(),
      "SyntaxError",
      "The URL is invalid",
    );
    match xhr.open("GET\u{100}", "data:,", true, None, None) {
      Err(OpError::Type(message)) => {
        assert_eq!(message, "The method is invalid")
      }
      other => panic!("{other:?}"),
    }
    assert_eq!(xhr.ready_state(), 1);
    assert_eq!(xhr.url.borrow().as_str(), "data:text/plain,hi");
    assert_eq!(xhr.method.borrow().as_str(), "GET");
  }

  #[test]
  fn userinfo_becomes_basic_and_is_stripped_from_the_target() {
    let xhr = super::XhrState::new();
    xhr
      .open(
        "GET",
        "https://example.com/a",
        true,
        Some("user"),
        Some("pass"),
      )
      .unwrap();
    assert_eq!(xhr.url.borrow().username(), "user");
    assert_eq!(xhr.url.borrow().password(), Some("pass"));
    let encoded = crate::text::base64::encode(b"user:pass");
    assert_eq!(encoded, "dXNlcjpwYXNz");
    let mut headers = Vec::new();
    super::attach_authorization(&mut headers, &xhr.url.borrow());
    assert_eq!(
      headers,
      vec![("authorization".to_string(), format!("Basic {encoded}"))]
    );
    assert_eq!(
      super::without_credentials(&xhr.url.borrow()),
      "https://example.com/a"
    );
    assert_eq!(xhr.url.borrow().username(), "user");

    xhr
      .open("GET", "https://kept:secret@example.com/a", true, None, None)
      .unwrap();
    assert_eq!(xhr.url.borrow().username(), "kept");
    assert_eq!(xhr.url.borrow().password(), Some("secret"));

    xhr
      .open("GET", "https://example.com/a", true, Some("a b"), Some("x"))
      .unwrap();
    assert_eq!(xhr.url.borrow().username(), "a%20b");
    assert_eq!(
      super::basic_authorization(&xhr.url.borrow()).unwrap(),
      format!("Basic {}", crate::text::base64::encode(b"a b:x"))
    );

    xhr
      .open("GET", "https://example.com/a", true, Some("user"), Some(""))
      .unwrap();
    assert_eq!(xhr.url.borrow().username(), "user");
    assert_eq!(xhr.url.borrow().password(), None);
    assert_eq!(
      super::basic_authorization(&xhr.url.borrow()).unwrap(),
      format!("Basic {}", crate::text::base64::encode(b"user:"))
    );

    xhr
      .open("GET", "https://example.com/a", true, None, Some("secret"))
      .unwrap();
    assert_eq!(xhr.url.borrow().as_str(), "https://:secret@example.com/a");
    assert_eq!(
      super::basic_authorization(&xhr.url.borrow()).unwrap(),
      format!("Basic {}", crate::text::base64::encode(b":secret"))
    );
    assert_eq!(
      super::without_credentials(&xhr.url.borrow()),
      "https://example.com/a"
    );

    let mut file = url::Url::parse("file:///tmp/a").unwrap();
    super::apply_userinfo(&mut file, Some("user"), Some("pass"));
    assert!(file.username().is_empty());
    assert_eq!(file.password(), None);
    xhr
      .open(
        "GET",
        "data:text/plain,hi",
        true,
        Some("user"),
        Some("pass"),
      )
      .unwrap();
    assert!(xhr.url.borrow().username().is_empty());
    assert_eq!(xhr.url.borrow().password(), None);
  }

  #[test]
  fn an_author_authorization_wins_over_the_url() {
    let xhr = super::XhrState::new();
    xhr
      .open("POST", "https://user:pass@example.com/a", true, None, None)
      .unwrap();
    xhr.set_request_header("Authorization", "Bearer t").unwrap();
    let mut headers = xhr.headers.borrow().clone();
    super::attach_authorization(&mut headers, &xhr.url.borrow());
    assert_eq!(
      headers,
      vec![("authorization".to_string(), "Bearer t".to_string())]
    );
  }

  #[test]
  fn content_type_follows_the_body_unless_the_author_set_one() {
    let xhr = super::XhrState::new();
    open_async(&xhr, "POST", "data:,");
    xhr.send(Some(text_body("hi", true))).unwrap();
    assert_eq!(
      xhr.headers.borrow().as_slice(),
      [(
        "content-type".to_string(),
        "text/plain;charset=UTF-8".to_string()
      )]
    );

    open_async(&xhr, "POST", "data:,");
    xhr
      .set_request_header("Content-Type", "text/plain;charset=iso-8859-1")
      .unwrap();
    xhr.send(Some(text_body("hi", true))).unwrap();
    assert_eq!(xhr.headers.borrow()[0].1, "text/plain;charset=UTF-8");

    open_async(&xhr, "POST", "data:,");
    xhr
      .set_request_header("Content-Type", "text/plain; charset=UTF-8")
      .unwrap();
    xhr.send(Some(text_body("hi", true))).unwrap();
    assert_eq!(xhr.headers.borrow()[0].1, "text/plain; charset=UTF-8");

    open_async(&xhr, "POST", "data:,");
    xhr
      .set_request_header("Content-Type", "text/plain")
      .unwrap();
    xhr
      .send(Some(super::XhrBody {
        bytes: b"png".to_vec(),
        content_type: Some("image/png".to_string()),
        rewrite_charset: false,
      }))
      .unwrap();
    assert_eq!(xhr.headers.borrow()[0].1, "text/plain");

    open_async(&xhr, "POST", "data:,");
    xhr
      .send(Some(super::XhrBody {
        bytes: b"png".to_vec(),
        content_type: Some("image/png".to_string()),
        rewrite_charset: false,
      }))
      .unwrap();
    assert_eq!(xhr.headers.borrow()[0].1, "image/png");

    open_async(&xhr, "POST", "data:,");
    xhr
      .send(Some(super::XhrBody {
        bytes: b"bin".to_vec(),
        content_type: None,
        rewrite_charset: false,
      }))
      .unwrap();
    assert!(xhr.headers.borrow().is_empty());
  }

  #[test]
  fn get_and_head_ignore_the_body_and_a_post_keeps_the_data_payload() {
    let xhr = super::XhrState::new();
    open_async(&xhr, "GET", "data:text/plain,hi");
    xhr
      .set_request_header("Content-Type", "text/plain;charset=iso-8859-1")
      .unwrap();
    let generation = match xhr.send(Some(text_body("nope", true))).unwrap() {
      super::XhrSend::Queued(generation) => generation,
      super::XhrSend::Finished(_, _) => panic!("synchronous send"),
    };
    assert_eq!(xhr.headers.borrow()[0].1, "text/plain;charset=iso-8859-1");
    xhr.publish(generation).unwrap();
    assert_eq!(xhr.response_text(), "hi");

    open_async(&xhr, "HEAD", "data:text/plain,hi");
    queued(&xhr);
    assert!(xhr.headers.borrow().is_empty());

    open_async(&xhr, "post", "data:text/plain,hi");
    assert!(xhr.carries_body().unwrap());
    assert_eq!(xhr.method.borrow().as_str(), "POST");
    let generation = match xhr.send(Some(text_body("sent", true))).unwrap() {
      super::XhrSend::Queued(generation) => generation,
      super::XhrSend::Finished(_, _) => panic!("synchronous send"),
    };
    xhr.publish(generation).unwrap();
    assert_eq!(xhr.response_text(), "hi");
    assert_eq!(xhr.headers.borrow()[0].1, "text/plain;charset=UTF-8");
  }

  #[test]
  fn a_synchronous_send_finishes_or_throws_before_it_returns() {
    let xhr = super::XhrState::new();
    assert!(
      xhr
        .open("GET", "data:text/plain,hi", false, None, None)
        .unwrap()
    );
    match xhr.send(None).unwrap() {
      super::XhrSend::Finished(_, note) => {
        assert_eq!(note.type_name, "load");
        assert_eq!(xhr.ready_state(), 4);
        assert_eq!(xhr.response_text(), "hi");
      }
      super::XhrSend::Queued(_) => panic!("queued"),
    }

    assert!(
      xhr
        .open("GET", "data:text/plain", false, None, None)
        .unwrap()
    );
    expect_dom(
      xhr.send(None).unwrap_err(),
      "NetworkError",
      "The request failed",
    );
    assert_eq!(xhr.ready_state(), 4);
    assert_eq!(xhr.response_text(), "");

    xhr.open("GET", "data:,", false, None, None).unwrap();
    expect_dom(
      xhr
        .finish_sync(xhr.generation.get(), super::progress_note("timeout", 0))
        .unwrap_err(),
      "TimeoutError",
      "The request timed out",
    );
    assert_eq!(xhr.ready_state(), 4);
    assert_eq!(xhr.response_text(), "");
    expect_dom(
      xhr
        .finish_sync(xhr.generation.get(), super::progress_note("error", 0))
        .unwrap_err(),
      "NetworkError",
      "The request failed",
    );
  }
}
