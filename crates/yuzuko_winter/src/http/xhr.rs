//! `XMLHttpRequest` for header normalization and response text.
//!
//! `open`, `setRequestHeader`, `send`, `overrideMimeType`, and
//! `getResponseHeader` follow the XHR specification. Forbidden headers stay
//! allowed under the fetch posture. `setRequestHeader` throws `SyntaxError`
//! for a value that is not a header value after HTTP whitespace is removed.
//! `responseText` uses the legacy decode hook: the `Content-Type` charset,
//! replaced by `overrideMimeType` when that MIME type has a charset, then a
//! BOM sniff. An XML charset prescan is not done.

use std::cell::Cell;
use std::cell::RefCell;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;
use std::time::Duration;
use std::time::Instant;

use yuzuko_core::op::Js;
use yuzuko_core::op::OpError;
use yuzuko_macros::op;

use crate::http::BodyNote;
use crate::http::HttpFailure;
use crate::http::fetch::header_list_value;
use crate::http::fetch::normalize_header;
use crate::http::fetch::normalize_value;
use crate::http::fetch::resolve_fetch_url;

const UNSENT: u8 = 0;
const OPENED: u8 = 1;
const DONE: u8 = 4;

/// Creates the `ProgressEvent` and calls the handler. `yuzuko_dom` installs it.
/// Absent, `onload` and `onerror` are called with no argument.
pub type XhrProgressHook = fn(
  &mut v8::PinScope,
  &v8::Global<v8::Object>,
  &v8::Global<v8::Function>,
  &str,
  bool,
  f64,
  f64,
);

static XHR_PROGRESS: Mutex<Option<XhrProgressHook>> = Mutex::new(None);

pub fn set_xhr_progress_hook(hook: XhrProgressHook) {
  *XHR_PROGRESS.lock().expect("xhr progress hook") = Some(hook);
}

fn xhr_progress_hook() -> Option<XhrProgressHook> {
  *XHR_PROGRESS.lock().expect("xhr progress hook")
}

struct ProgressNote {
  property: &'static str,
  length_computable: bool,
  loaded: f64,
  total: f64,
}

fn progress_note(property: &'static str, bytes: usize) -> ProgressNote {
  let success = property == "onload";
  let total = if success { bytes as f64 } else { 0.0 };
  ProgressNote {
    property,
    length_computable: success && bytes != 0,
    loaded: total,
    total,
  }
}

pub struct XMLHttpRequest {
  state: Cell<u8>,
  sent: Cell<bool>,
  status_code: Cell<u16>,
  method: RefCell<String>,
  url: RefCell<String>,
  status_text: RefCell<String>,
  headers: RefCell<Vec<(String, String)>>,
  response_headers: RefCell<Vec<(String, String)>>,
  response_body: RefCell<Vec<u8>>,
  /// Parsed override MIME type. `open` does not clear it.
  override_mime: RefCell<Option<crate::text::mime::Mime>>,
}

unsafe impl v8::cppgc::GarbageCollected for XMLHttpRequest {
  fn trace(&self, _visitor: &mut v8::cppgc::Visitor) {}

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"XMLHttpRequest"
  }
}

#[op]
impl XMLHttpRequest {
  #[constructor]
  fn new() -> Self {
    Self {
      state: Cell::new(UNSENT),
      sent: Cell::new(false),
      status_code: Cell::new(0),
      method: RefCell::new("GET".to_string()),
      url: RefCell::new(String::new()),
      status_text: RefCell::new(String::new()),
      headers: RefCell::new(Vec::new()),
      response_headers: RefCell::new(Vec::new()),
      response_body: RefCell::new(Vec::new()),
      override_mime: RefCell::new(None),
    }
  }

  #[rename("overrideMimeType")]
  fn override_mime_type(&self, mime: String) -> Result<(), OpError> {
    if self.state.get() == DONE {
      return Err(OpError::dom("InvalidStateError", "The request is done"));
    }
    *self.override_mime.borrow_mut() = Some(parsed_override(&mime));
    Ok(())
  }

  fn open(&self, method: String, url: String) -> Result<(), OpError> {
    let method = crate::http::request::normalize_method(&method)?;
    *self.method.borrow_mut() = method;
    *self.url.borrow_mut() = url;
    self.headers.borrow_mut().clear();
    self.sent.set(false);
    self.state.set(OPENED);
    Ok(())
  }

  #[rename("setRequestHeader")]
  fn set_request_header(
    &self,
    name: String,
    value: String,
  ) -> Result<(), OpError> {
    if self.state.get() != OPENED || self.sent.get() {
      return Err(OpError::dom("InvalidStateError", "The request is not open"));
    }
    let name = normalize_header(&name)
      .map_err(|_| OpError::dom("SyntaxError", "The header name is invalid"))?;
    let value = normalize_value(&value).map_err(|_| {
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

  fn send(
    &self,
    scope: &mut v8::PinScope,
    #[this] object: v8::Local<v8::Object>,
  ) -> Result<(), OpError> {
    if self.state.get() != OPENED || self.sent.get() {
      return Err(OpError::dom("InvalidStateError", "The request is not open"));
    }
    self.sent.set(true);
    let url = resolve_fetch_url(&self.url.borrow())?;
    let event = if url.scheme() == "data" {
      finish_data(self, &url)
    } else {
      let method = self.method.borrow().clone();
      let headers = self.headers.borrow().clone();
      let cancel = Arc::new(AtomicBool::new(false));
      let outcome = crate::http::perform(
        method,
        url.to_string(),
        headers,
        Vec::new(),
        "follow".to_string(),
        cancel,
      );
      match outcome {
        Ok(response) => {
          let body = collect_body(&response.body);
          let note = progress_note("onload", body.len());
          *self.response_body.borrow_mut() = body;
          *self.response_headers.borrow_mut() = response.headers;
          self.status_code.set(response.status);
          *self.status_text.borrow_mut() = response.status_text;
          self.state.set(DONE);
          note
        }
        Err(HttpFailure::Abort | HttpFailure::Network(_)) => {
          self.state.set(DONE);
          progress_note("onerror", 0)
        }
        Err(HttpFailure::Type(message)) => {
          return Err(OpError::type_(message));
        }
      }
    };
    // `load` is a task. Callers assign `onload` after `send()` returns.
    let object = v8::Global::new(scope, object);
    yuzuko_core::enqueue(move |scope| {
      let object = v8::Local::new(scope, &object);
      call_handler(scope, object, &event);
    });
    Ok(())
  }

  #[rename("getResponseHeader")]
  fn get_response_header(
    &self,
    scope: &mut v8::PinScope,
    name: String,
  ) -> Result<Js, OpError> {
    if self.state.get() != DONE {
      return Ok(null_js(scope));
    }
    let name = normalize_header(&name)?;
    if name == "set-cookie" || name == "set-cookie2" {
      return Ok(null_js(scope));
    }
    let headers = self.response_headers.borrow();
    if !headers.iter().any(|(key, _)| key == &name) {
      return Ok(null_js(scope));
    }
    let value = header_list_value(&headers, &name);
    let text = v8::String::new(scope, &value).unwrap();
    let local: v8::Local<v8::Value> = text.into();
    Ok(Js(v8::Global::new(scope, local)))
  }

  #[getter]
  fn status(&self) -> f64 {
    if self.state.get() != DONE {
      return 0.0;
    }
    f64::from(self.status_code.get())
  }

  #[getter]
  #[rename("statusText")]
  fn status_text(&self) -> String {
    if self.state.get() != DONE {
      return String::new();
    }
    self.status_text.borrow().clone()
  }

  #[getter]
  #[rename("responseText")]
  fn response_text(&self) -> String {
    if self.state.get() != DONE {
      return String::new();
    }
    decode_xhr_text(
      &self.response_body.borrow(),
      &self.response_headers.borrow(),
      self.override_mime.borrow().as_ref(),
    )
  }

  #[rename("getAllResponseHeaders")]
  fn get_all_response_headers(&self) -> String {
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

fn finish_data(xhr: &XMLHttpRequest, url: &url::Url) -> ProgressNote {
  match crate::text::mime::data_url(url) {
    Some((bytes, mime)) => {
      let note = progress_note("onload", bytes.len());
      *xhr.response_body.borrow_mut() = bytes;
      *xhr.response_headers.borrow_mut() =
        vec![("content-type".to_string(), mime)];
      xhr.status_code.set(200);
      *xhr.status_text.borrow_mut() = "OK".to_string();
      xhr.state.set(DONE);
      note
    }
    None => {
      xhr.response_body.borrow_mut().clear();
      xhr.response_headers.borrow_mut().clear();
      xhr.status_code.set(0);
      xhr.status_text.borrow_mut().clear();
      xhr.state.set(DONE);
      progress_note("onerror", 0)
    }
  }
}

fn null_js(scope: &mut v8::PinScope) -> Js {
  let local: v8::Local<v8::Value> = v8::null(scope).into();
  Js(v8::Global::new(scope, local))
}

fn collect_body(body: &crate::http::HttpBody) -> Vec<u8> {
  let start = Instant::now();
  let mut output = Vec::new();
  loop {
    match body.poll() {
      Some(BodyNote::End) | Some(BodyNote::Failed(_)) => return output,
      Some(BodyNote::Chunk(bytes)) => output.extend(bytes),
      None => {
        if start.elapsed() > Duration::from_secs(5) {
          return output;
        }
        std::thread::sleep(Duration::from_millis(1));
      }
    }
  }
}

fn call_handler(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  note: &ProgressNote,
) {
  let key = v8::String::new(scope, note.property).unwrap();
  let Some(value) = object.get(scope, key.into()) else {
    return;
  };
  let Ok(function) = v8::Local::<v8::Function>::try_from(value) else {
    return;
  };
  let Some(hook) = xhr_progress_hook() else {
    let _ = function.call(scope, object.into(), &[]);
    return;
  };
  let xhr = v8::Global::new(scope, object);
  let handler = v8::Global::new(scope, function);
  let type_name = note.property.strip_prefix("on").unwrap_or(note.property);
  hook(
    scope,
    &xhr,
    &handler,
    type_name,
    note.length_computable,
    note.loaded,
    note.total,
  );
}

#[cfg(test)]
mod tests {
  use super::decode_xhr_text;
  use crate::text::mime::parse;

  fn headers(value: &str) -> Vec<(String, String)> {
    vec![("content-type".to_string(), value.to_string())]
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
  fn data_url_body_uses_the_override_charset() {
    let url = url::Url::parse("data:text/plain,%41%42%43%A0").unwrap();
    let (bytes, mime) = crate::text::mime::data_url(&url).unwrap();
    assert_eq!(mime, "text/plain");
    let over = parse("text/plain; charset=replacement").unwrap();
    let text = decode_xhr_text(&bytes, &headers(&mime), Some(&over));
    assert_eq!(text, "\u{FFFD}");
  }
}
