//! `XMLHttpRequest` for header normalization tests.
//!
//! `open`, `setRequestHeader`, `send`, and `getResponseHeader` follow the
//! XHR specification. Forbidden headers stay allowed under the fetch posture.
//! `setRequestHeader` throws `SyntaxError` for a value that is not a header
//! value after HTTP whitespace is removed.

use std::cell::Cell;
use std::cell::RefCell;
use std::sync::Arc;
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
    }
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
    let event = match outcome {
      Ok(response) => {
        *self.response_body.borrow_mut() = collect_body(&response.body);
        *self.response_headers.borrow_mut() = response.headers;
        self.status_code.set(response.status);
        *self.status_text.borrow_mut() = response.status_text;
        self.state.set(DONE);
        "onload"
      }
      Err(HttpFailure::Abort | HttpFailure::Network(_)) => {
        self.state.set(DONE);
        "onerror"
      }
      Err(HttpFailure::Type(message)) => {
        return Err(OpError::type_(message));
      }
    };
    // `load` is a task. Callers assign `onload` after `send()` returns.
    let object = v8::Global::new(scope, object);
    yuzuko_core::enqueue(move |scope| {
      let object = v8::Local::new(scope, &object);
      call_handler(scope, object, event);
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
    crate::blob::decode_utf8(&self.response_body.borrow())
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
  name: &str,
) {
  let key = v8::String::new(scope, name).unwrap();
  let Some(value) = object.get(scope, key.into()) else {
    return;
  };
  let Ok(function) = v8::Local::<v8::Function>::try_from(value) else {
    return;
  };
  let _ = function.call(scope, object.into(), &[]);
}
