//! Helpers shared by the network classes.

use std::sync::{Arc, Mutex};

#[cfg(feature = "socket")]
use std::cell::RefCell;

#[cfg(feature = "socket")]
use yuzuko_core::op::{Js, OpError, traced_local};

use crate::event::{Event, define_is_trusted};
use crate::target;
use crate::util::key;

/// A one-shot wake for a background note queue. The first call schedules
/// `pump` on this realm's event loop; later calls do nothing.
pub(super) fn one_shot_wake(
  label: &'static str,
  pump: impl FnOnce(&mut v8::PinScope) + 'static,
) -> Arc<dyn Fn() + Send + Sync> {
  let listen = yuzuko_core::listen(pump);
  let slot = Mutex::new(Some(listen));
  Arc::new(move || {
    if let Some(listen) = slot.lock().expect(label).take() {
      listen.notify();
    }
  })
}

/// Dispatches a trusted `Event` that does not bubble and is not cancelable.
pub(super) fn dispatch_simple(
  scope: &mut v8::PinScope,
  target: v8::Local<v8::Object>,
  name: &str,
) {
  let event = Event::new(name, false, false, false, true);
  let object = unsafe { yuzuko_core::op::wrap_instance(scope, event) };
  define_is_trusted(scope, object);
  let _ = target::dispatch(scope, target, object);
}

pub(super) fn define_constant(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
  value: i32,
) {
  let number = v8::Integer::new(scope, value);
  let _ = object.define_own_property(
    scope,
    key(scope, name).into(),
    number.into(),
    v8::PropertyAttribute::READ_ONLY | v8::PropertyAttribute::DONT_DELETE,
  );
}

/// Parses `input` against the entry URL and requires one of `schemes` and
/// no fragment.
#[cfg(feature = "socket")]
pub(super) fn parse_url(
  input: &str,
  schemes: &[&str],
  scheme_message: &str,
) -> Result<url::Url, OpError> {
  let url = if let Ok(url) = url::Url::parse(input) {
    url
  } else if let Some(base) = yuzuko_core::entry_url() {
    let base = url::Url::parse(&base)
      .map_err(|_| OpError::dom("SyntaxError", "The URL is invalid"))?;
    base
      .join(input)
      .map_err(|_| OpError::dom("SyntaxError", "The URL is invalid"))?
  } else {
    return Err(OpError::dom("SyntaxError", "The URL is invalid"));
  };
  if !schemes.contains(&url.scheme()) {
    return Err(OpError::dom("SyntaxError", scheme_message));
  }
  if url.fragment().is_some() {
    return Err(OpError::dom("SyntaxError", "The URL has a fragment"));
  }
  Ok(url)
}

#[cfg(feature = "socket")]
pub(super) fn resolver_pair<'s>(
  scope: &mut v8::PinScope<'s, '_>,
) -> (
  v8::Local<'s, v8::PromiseResolver>,
  v8::Local<'s, v8::Promise>,
) {
  let resolver = v8::PromiseResolver::new(scope).unwrap();
  let promise = resolver.get_promise(scope);
  (resolver, promise)
}

#[cfg(feature = "socket")]
pub(super) fn resolved_promise<'s>(
  scope: &mut v8::PinScope<'s, '_>,
) -> v8::Local<'s, v8::Promise> {
  let (resolver, promise) = resolver_pair(scope);
  let value = v8::undefined(scope).into();
  resolver.resolve(scope, value);
  promise
}

#[cfg(feature = "socket")]
pub(super) fn promise_slot(
  scope: &mut v8::PinScope,
  slot: &RefCell<Option<v8::TracedReference<v8::Promise>>>,
) -> Js {
  let promise = slot
    .borrow()
    .as_ref()
    .and_then(|reference| traced_local(scope, reference));
  match promise {
    Some(promise) => {
      let value: v8::Local<v8::Value> = promise.into();
      Js(v8::Global::new(scope, value))
    }
    None => Js::null(scope),
  }
}

#[cfg(feature = "socket")]
pub(super) fn resolve_slot(
  scope: &mut v8::PinScope,
  slot: &RefCell<Option<v8::TracedReference<v8::PromiseResolver>>>,
  value: v8::Local<v8::Value>,
) {
  let Some(reference) = slot.borrow_mut().take() else {
    return;
  };
  resolve_moved(scope, reference, value);
}

#[cfg(feature = "socket")]
pub(super) fn reject_slot(
  scope: &mut v8::PinScope,
  slot: &RefCell<Option<v8::TracedReference<v8::PromiseResolver>>>,
  value: v8::Local<v8::Value>,
) {
  let Some(reference) = slot.borrow_mut().take() else {
    return;
  };
  reject_moved(scope, reference, value);
}

#[cfg(feature = "socket")]
pub(super) fn resolve_moved(
  scope: &mut v8::PinScope,
  reference: v8::TracedReference<v8::PromiseResolver>,
  value: v8::Local<v8::Value>,
) {
  let Some(resolver) = traced_local(scope, &reference) else {
    return;
  };
  let resolver = yuzuko_core::op::root(scope, resolver);
  let value = yuzuko_core::op::root(scope, value);
  v8::tc_scope!(let scope, scope);
  resolver.resolve(scope, value);
}

#[cfg(feature = "socket")]
pub(super) fn reject_moved(
  scope: &mut v8::PinScope,
  reference: v8::TracedReference<v8::PromiseResolver>,
  value: v8::Local<v8::Value>,
) {
  let Some(resolver) = traced_local(scope, &reference) else {
    return;
  };
  let resolver = yuzuko_core::op::root(scope, resolver);
  let value = yuzuko_core::op::root(scope, value);
  v8::tc_scope!(let scope, scope);
  resolver.reject(scope, value);
}

#[cfg(feature = "socket")]
pub(super) fn mark_handled(
  scope: &mut v8::PinScope,
  slot: &RefCell<Option<v8::TracedReference<v8::Promise>>>,
) {
  if let Some(promise) = slot
    .borrow()
    .as_ref()
    .and_then(|reference| traced_local(scope, reference))
  {
    promise.mark_as_handled();
  }
}

#[cfg(feature = "socket")]
pub(super) fn trace_slot<T>(
  slot: &RefCell<Option<v8::TracedReference<T>>>,
  visitor: &mut v8::cppgc::Visitor,
) {
  if let Ok(slot) = slot.try_borrow() {
    visitor.trace(&*slot);
  }
}

#[cfg(feature = "socket")]
pub(super) fn set(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
  value: v8::Local<v8::Value>,
) {
  let _ = object.create_data_property(scope, key(scope, name).into(), value);
}

#[cfg(feature = "socket")]
pub(super) fn uint8_array<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  bytes: Vec<u8>,
) -> v8::Local<'s, v8::Uint8Array> {
  yuzuko_core::uint8_array(scope, bytes).unwrap()
}

#[cfg(feature = "socket")]
pub(super) fn defined<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  object: v8::Local<v8::Object>,
  name: &str,
) -> Result<Option<v8::Local<'s, v8::Value>>, OpError> {
  yuzuko_core::dictionary_member(scope, object, name)
}

/// Whether a chunk is a `SharedArrayBuffer`, a resizable `ArrayBuffer`, or a
/// view on one.
#[cfg(feature = "socket")]
pub(super) fn unacceptable_buffer(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> bool {
  if value.is_shared_array_buffer() {
    return true;
  }
  if let Ok(buffer) = v8::Local::<v8::ArrayBuffer>::try_from(value) {
    return shared_or_resizable(buffer);
  }
  if let Ok(view) = v8::Local::<v8::ArrayBufferView>::try_from(value) {
    let Some(buffer) = view.buffer(scope) else {
      return true;
    };
    return shared_or_resizable(buffer);
  }
  false
}

#[cfg(feature = "socket")]
fn shared_or_resizable(buffer: v8::Local<v8::ArrayBuffer>) -> bool {
  let store = buffer.get_backing_store();
  store.is_shared() || store.is_resizable_by_user_javascript()
}

#[cfg(feature = "socket")]
pub(super) fn clamp_unsigned_short(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> u16 {
  let number = value.number_value(scope).unwrap_or(f64::NAN);
  if number.is_nan() {
    return 0;
  }
  number.clamp(0.0, 65535.0).round_ties_even() as u16
}

#[cfg(feature = "socket")]
pub(super) fn invalid_state<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  message: &str,
) -> v8::Local<'s, v8::Value> {
  yuzuko_core::op::new_dom_exception(scope, "InvalidStateError", message)
    .map(Into::into)
    .unwrap_or_else(|| v8::undefined(scope).into())
}

/// Defines the own `message`, `name`, and `code` properties of a
/// `DOMException` subclass instance made by the host.
#[cfg(feature = "socket")]
pub(super) fn stamp_exception(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
  message: &str,
) {
  if let Some(text) = v8::String::new(scope, message) {
    let _ = object.define_own_property(
      scope,
      key(scope, "message").into(),
      text.into(),
      v8::PropertyAttribute::NONE,
    );
  }
  if let Some(text) = v8::String::new(scope, name) {
    let _ = object.define_own_property(
      scope,
      key(scope, "name").into(),
      text.into(),
      v8::PropertyAttribute::NONE,
    );
  }
  let code = v8::Integer::new(scope, 0);
  let _ = object.define_own_property(
    scope,
    key(scope, "code").into(),
    code.into(),
    v8::PropertyAttribute::READ_ONLY,
  );
}

/// Sets the prototype of `template`'s `prototype` object to
/// `DOMException.prototype`.
#[cfg(feature = "socket")]
pub(super) fn link_exception(
  scope: &mut v8::PinScope,
  template: v8::Local<v8::FunctionTemplate>,
) {
  let Some(constructor) = template.get_function(scope) else {
    return;
  };
  let Some(prototype) = constructor
    .get(scope, key(scope, "prototype").into())
    .and_then(|value| v8::Local::<v8::Object>::try_from(value).ok())
  else {
    return;
  };
  let Some(parent) =
    crate::exception::DOMException::template(scope).get_function(scope)
  else {
    return;
  };
  let Some(parent_prototype) = parent
    .get(scope, key(scope, "prototype").into())
    .and_then(|value| v8::Local::<v8::Object>::try_from(value).ok())
  else {
    return;
  };
  let _ = prototype.set_prototype(scope, parent_prototype.into());
}
