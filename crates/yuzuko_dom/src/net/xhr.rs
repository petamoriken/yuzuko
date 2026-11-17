//! `XMLHttpRequest`. An asynchronous completion dispatches
//! `readystatechange`, `loadstart`, `progress`, one of `load`, `error`, or
//! `timeout`, and `loadend`. A synchronous `send` dispatches
//! `readystatechange`, `load`, and `loadend` before it returns.

use std::cell::RefCell;

use yuzuko_core::op::{FromV8, Js, OpError, traced, traced_local};
use yuzuko_macros::op;
use yuzuko_winter::{XhrSend, XhrState, extract_xhr_body};

use crate::event::dispatch_progress;
use crate::target::EventTarget;

#[repr(C)]
pub struct XMLHttpRequest {
  base: EventTarget,
  inner: RefCell<XhrState>,
  onloadstart: RefCell<Option<v8::TracedReference<v8::Value>>>,
  onprogress: RefCell<Option<v8::TracedReference<v8::Value>>>,
  onload: RefCell<Option<v8::TracedReference<v8::Value>>>,
  onerror: RefCell<Option<v8::TracedReference<v8::Value>>>,
  onloadend: RefCell<Option<v8::TracedReference<v8::Value>>>,
  onabort: RefCell<Option<v8::TracedReference<v8::Value>>>,
  ontimeout: RefCell<Option<v8::TracedReference<v8::Value>>>,
  onreadystatechange: RefCell<Option<v8::TracedReference<v8::Value>>>,
}

unsafe impl v8::cppgc::GarbageCollected for XMLHttpRequest {
  fn trace(&self, visitor: &mut v8::cppgc::Visitor) {
    v8::cppgc::GarbageCollected::trace(&self.base, visitor);
    trace_slot(&self.onloadstart, visitor);
    trace_slot(&self.onprogress, visitor);
    trace_slot(&self.onload, visitor);
    trace_slot(&self.onerror, visitor);
    trace_slot(&self.onloadend, visitor);
    trace_slot(&self.onabort, visitor);
    trace_slot(&self.ontimeout, visitor);
    trace_slot(&self.onreadystatechange, visitor);
  }

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"XMLHttpRequest"
  }
}

#[op(inherit = EventTarget)]
impl XMLHttpRequest {
  #[constructor]
  fn new() -> Self {
    Self {
      base: EventTarget::new(),
      inner: RefCell::new(XhrState::new()),
      onloadstart: RefCell::new(None),
      onprogress: RefCell::new(None),
      onload: RefCell::new(None),
      onerror: RefCell::new(None),
      onloadend: RefCell::new(None),
      onabort: RefCell::new(None),
      ontimeout: RefCell::new(None),
      onreadystatechange: RefCell::new(None),
    }
  }

  /// `.length` stays 2. An omitted third argument is asynchronous. An
  /// explicit `undefined` is synchronous, because WebIDL boolean conversion
  /// of `undefined` is false.
  #[length(2)]
  fn open(
    &self,
    scope: &mut v8::PinScope,
    #[this] object: v8::Local<v8::Object>,
    args: &v8::FunctionCallbackArguments,
    method: String,
    url: String,
  ) -> Result<(), OpError> {
    let asynchronous = if args.length() < 3 {
      true
    } else {
      args.get(2).boolean_value(scope)
    };
    let username = optional_string(scope, args, 3)?;
    let password = optional_string(scope, args, 4)?;
    let fire = self.inner.borrow().open(
      &method,
      &url,
      asynchronous,
      username.as_deref(),
      password.as_deref(),
    )?;
    if fire {
      dispatch_readystatechange(scope, object);
    }
    Ok(())
  }

  #[rename("setRequestHeader")]
  fn set_request_header(
    &self,
    name: String,
    value: String,
  ) -> Result<(), OpError> {
    self.inner.borrow().set_request_header(&name, &value)
  }

  fn send(
    &self,
    scope: &mut v8::PinScope,
    #[this] object: v8::Local<v8::Object>,
    body: Option<v8::Local<v8::Value>>,
  ) -> Result<(), OpError> {
    // Extract before `send` stores the flag. `ToString` can reenter.
    // `GET` and `HEAD` skip extraction, so a value that would throw is ignored.
    // An asynchronous request stays `OPENED` until the queued task. A
    // synchronous request dispatches before this returns.
    let carries = self.inner.borrow().carries_body()?;
    let extracted = if carries {
      extract_xhr_body(scope, body)?
    } else {
      None
    };
    let sent = self.inner.borrow().send(extracted)?;
    match sent {
      XhrSend::Queued(generation) => {
        let object = v8::Global::new(scope, object);
        yuzuko_core::enqueue(move |scope| {
          let object = v8::Local::new(scope, &object);
          settle(scope, object, generation);
        });
      }
      XhrSend::Finished(generation, note) => {
        settle_sync(scope, object, generation, &note);
      }
    }
    Ok(())
  }

  fn abort(
    &self,
    scope: &mut v8::PinScope,
    #[this] object: v8::Local<v8::Object>,
  ) {
    if !self.inner.borrow().begin_abort() {
      return;
    }
    // `readyState` is `DONE` for these events. `finish_abort` then sets
    // `UNSENT` without another `readystatechange`.
    dispatch_readystatechange(scope, object);
    dispatch_stopped(scope, object, "abort");
    self.inner.borrow().finish_abort();
  }

  #[rename("overrideMimeType")]
  fn override_mime_type(&self, mime: String) -> Result<(), OpError> {
    self.inner.borrow().override_mime_type(&mime)
  }

  #[rename("getResponseHeader")]
  fn get_response_header(
    &self,
    scope: &mut v8::PinScope,
    name: String,
  ) -> Result<Js, OpError> {
    match self.inner.borrow().response_header(&name)? {
      Some(value) => {
        let text = v8::String::new(scope, &value).unwrap();
        let local: v8::Local<v8::Value> = text.into();
        Ok(Js(v8::Global::new(scope, local)))
      }
      None => Ok(Js::null(scope)),
    }
  }

  #[rename("getAllResponseHeaders")]
  fn get_all_response_headers(&self) -> String {
    self.inner.borrow().all_response_headers()
  }

  #[getter]
  fn status(&self) -> f64 {
    self.inner.borrow().status()
  }

  #[getter]
  #[rename("readyState")]
  fn ready_state(&self) -> f64 {
    f64::from(self.inner.borrow().ready_state())
  }

  #[getter]
  fn timeout(&self) -> f64 {
    f64::from(self.inner.borrow().timeout())
  }

  #[setter]
  fn timeout(
    &self,
    scope: &mut v8::PinScope,
    #[this] object: v8::Local<v8::Object>,
    value: v8::Local<v8::Value>,
  ) {
    let Some(millis) = unsigned_long(scope, value) else {
      return;
    };
    if !self.inner.borrow().set_timeout(millis) {
      return;
    }
    dispatch_readystatechange(scope, object);
    dispatch_stopped(scope, object, "timeout");
  }

  #[getter]
  #[rename("statusText")]
  fn status_text(&self) -> String {
    self.inner.borrow().status_text()
  }

  #[getter]
  #[rename("responseText")]
  fn response_text(&self) -> String {
    self.inner.borrow().response_text()
  }

  #[getter]
  fn onloadstart(&self, scope: &mut v8::PinScope) -> Js {
    handler_value(scope, &self.onloadstart)
  }

  #[setter]
  fn onloadstart(&self, scope: &mut v8::PinScope, value: v8::Local<v8::Value>) {
    set_handler(scope, &self.base, &self.onloadstart, "loadstart", value);
  }

  #[getter]
  fn onprogress(&self, scope: &mut v8::PinScope) -> Js {
    handler_value(scope, &self.onprogress)
  }

  #[setter]
  fn onprogress(&self, scope: &mut v8::PinScope, value: v8::Local<v8::Value>) {
    set_handler(scope, &self.base, &self.onprogress, "progress", value);
  }

  #[getter]
  fn onload(&self, scope: &mut v8::PinScope) -> Js {
    handler_value(scope, &self.onload)
  }

  #[setter]
  fn onload(&self, scope: &mut v8::PinScope, value: v8::Local<v8::Value>) {
    set_handler(scope, &self.base, &self.onload, "load", value);
  }

  #[getter]
  fn onerror(&self, scope: &mut v8::PinScope) -> Js {
    handler_value(scope, &self.onerror)
  }

  #[setter]
  fn onerror(&self, scope: &mut v8::PinScope, value: v8::Local<v8::Value>) {
    set_handler(scope, &self.base, &self.onerror, "error", value);
  }

  #[getter]
  fn onloadend(&self, scope: &mut v8::PinScope) -> Js {
    handler_value(scope, &self.onloadend)
  }

  #[setter]
  fn onloadend(&self, scope: &mut v8::PinScope, value: v8::Local<v8::Value>) {
    set_handler(scope, &self.base, &self.onloadend, "loadend", value);
  }

  #[getter]
  fn onabort(&self, scope: &mut v8::PinScope) -> Js {
    handler_value(scope, &self.onabort)
  }

  #[setter]
  fn onabort(&self, scope: &mut v8::PinScope, value: v8::Local<v8::Value>) {
    set_handler(scope, &self.base, &self.onabort, "abort", value);
  }

  #[getter]
  fn ontimeout(&self, scope: &mut v8::PinScope) -> Js {
    handler_value(scope, &self.ontimeout)
  }

  #[setter]
  fn ontimeout(&self, scope: &mut v8::PinScope, value: v8::Local<v8::Value>) {
    set_handler(scope, &self.base, &self.ontimeout, "timeout", value);
  }

  #[getter]
  fn onreadystatechange(&self, scope: &mut v8::PinScope) -> Js {
    handler_value(scope, &self.onreadystatechange)
  }

  #[setter]
  fn onreadystatechange(
    &self,
    scope: &mut v8::PinScope,
    value: v8::Local<v8::Value>,
  ) {
    set_handler(
      scope,
      &self.base,
      &self.onreadystatechange,
      "readystatechange",
      value,
    );
  }
}

pub(crate) fn install(scope: &mut v8::PinScope) {
  XMLHttpRequest::install(scope);
  let constructor = XMLHttpRequest::template(scope)
    .get_function(scope)
    .expect("XMLHttpRequest");
  let constructor = v8::Local::<v8::Object>::try_from(
    v8::Local::<v8::Value>::from(constructor),
  )
  .expect("XMLHttpRequest constructor");
  let prototype = constructor
    .get(scope, v8::String::new(scope, "prototype").unwrap().into())
    .and_then(|value| v8::Local::<v8::Object>::try_from(value).ok())
    .expect("XMLHttpRequest prototype");
  for (name, value) in [
    ("UNSENT", 0),
    ("OPENED", 1),
    ("HEADERS_RECEIVED", 2),
    ("LOADING", 3),
    ("DONE", 4),
  ] {
    define_constant(scope, constructor, name, value);
    define_constant(scope, prototype, name, value);
  }
}

fn optional_string(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
  index: i32,
) -> Result<Option<String>, OpError> {
  if index >= args.length() {
    return Ok(None);
  }
  let value = args.get(index);
  if value.is_null_or_undefined() {
    return Ok(None);
  }
  FromV8::from_v8(scope, value).map(Some)
}

/// Synchronous success. No `loadstart` and no `progress`. `open` during
/// `readystatechange` bumps the generation and stops the rest. `abort` of a
/// finished request does not, so `load` and `loadend` still run.
fn settle_sync(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  generation: u64,
  note: &yuzuko_winter::XhrProgress,
) {
  dispatch_readystatechange(scope, object);
  if !still_current(scope, object, generation) {
    return;
  }
  dispatch_progress(
    scope,
    object,
    note.type_name,
    note.length_computable,
    note.loaded,
    note.total,
  );
  if !still_current(scope, object, generation) {
    return;
  }
  dispatch_progress(
    scope,
    object,
    "loadend",
    note.length_computable,
    note.loaded,
    note.total,
  );
}

fn settle(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  generation: u64,
) {
  let note = {
    let Some(xhr) =
      (unsafe { yuzuko_core::op::host_ref::<XMLHttpRequest>(scope, object) })
    else {
      return;
    };
    xhr.get().inner.borrow().publish(generation)
  };
  let Some(note) = note else {
    return;
  };
  dispatch_readystatechange(scope, object);
  if !still_current(scope, object, generation) {
    return;
  }
  dispatch_progress(
    scope,
    object,
    "loadstart",
    note.length_computable,
    0.0,
    note.total,
  );
  if !still_current(scope, object, generation) {
    return;
  }
  if note.type_name == "load" && note.loaded > 0.0 {
    dispatch_progress(
      scope,
      object,
      "progress",
      note.length_computable,
      note.loaded,
      note.total,
    );
    if !still_current(scope, object, generation) {
      return;
    }
  }
  dispatch_progress(
    scope,
    object,
    note.type_name,
    note.length_computable,
    note.loaded,
    note.total,
  );
  if !still_current(scope, object, generation) {
    return;
  }
  dispatch_progress(
    scope,
    object,
    "loadend",
    note.length_computable,
    note.loaded,
    note.total,
  );
}

fn dispatch_readystatechange(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
) {
  crate::dispatch_trusted_event(scope, object, "readystatechange");
}

fn dispatch_stopped(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  type_name: &str,
) {
  dispatch_progress(scope, object, type_name, false, 0.0, 0.0);
  dispatch_progress(scope, object, "loadend", false, 0.0, 0.0);
}

fn still_current(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  generation: u64,
) -> bool {
  let Some(xhr) =
    (unsafe { yuzuko_core::op::host_ref::<XMLHttpRequest>(scope, object) })
  else {
    return false;
  };
  xhr.get().inner.borrow().is_current(generation)
}

fn define_constant(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
  value: i32,
) {
  let key = v8::String::new(scope, name).unwrap();
  let number = v8::Integer::new(scope, value);
  let _ = object.define_own_property(
    scope,
    key.into(),
    number.into(),
    v8::PropertyAttribute::READ_ONLY | v8::PropertyAttribute::DONT_DELETE,
  );
}

/// WebIDL unsigned long: `ToNumber`, then modulo 2^32. Non-finite values are 0.
fn unsigned_long(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Option<u32> {
  let number = value.number_value(scope)?;
  if !number.is_finite() || number == 0.0 {
    return Some(0);
  }
  let width = 4294967296.0;
  let mut modulo = number.trunc() % width;
  if modulo < 0.0 {
    modulo += width;
  }
  Some(modulo as u32)
}

fn trace_slot(
  slot: &RefCell<Option<v8::TracedReference<v8::Value>>>,
  visitor: &mut v8::cppgc::Visitor,
) {
  if let Ok(slot) = slot.try_borrow() {
    visitor.trace(&*slot);
  }
}

fn handler_value(
  scope: &mut v8::PinScope,
  slot: &RefCell<Option<v8::TracedReference<v8::Value>>>,
) -> Js {
  let value = slot
    .borrow()
    .as_ref()
    .and_then(|reference| traced_local(scope, reference))
    .unwrap_or_else(|| v8::null(scope).into());
  Js(v8::Global::new(scope, value))
}

fn set_handler(
  scope: &mut v8::PinScope,
  target: &EventTarget,
  slot: &RefCell<Option<v8::TracedReference<v8::Value>>>,
  type_name: &str,
  value: v8::Local<v8::Value>,
) {
  if let Some(previous) = slot.borrow().as_ref()
    && let Some(previous) = traced_local(scope, previous)
  {
    target.remove_callback(scope, type_name, previous, false);
  }
  if value.is_function() {
    target.add_callback(scope, type_name, value, false, false, false);
    *slot.borrow_mut() = Some(traced(scope, value));
  } else {
    *slot.borrow_mut() = None;
  }
}
