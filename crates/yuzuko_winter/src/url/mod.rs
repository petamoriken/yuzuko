//! `URL`, `URLSearchParams`, and `URLPattern`. Parsing is the `url` crate.
//! A URL and its `searchParams` share one `Rc<RefCell<url::Url>>`.

pub(crate) mod pattern;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use yuzuko_core::op::{
  Js, New, OpError, OpException, host_ref, traced, traced_local, wrap_instance,
};
use yuzuko_macros::op;

pub struct Url {
  shared: Rc<RefCell<url::Url>>,
  params: RefCell<Option<v8::TracedReference<v8::Object>>>,
}

unsafe impl v8::cppgc::GarbageCollected for Url {
  fn trace(&self, visitor: &mut v8::cppgc::Visitor) {
    if let Ok(params) = self.params.try_borrow()
      && let Some(params) = params.as_ref()
    {
      visitor.trace(params);
    }
  }

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"URL"
  }
}

impl Url {
  fn from_parsed(parsed: url::Url) -> Self {
    Self {
      shared: Rc::new(RefCell::new(parsed)),
      params: RefCell::new(None),
    }
  }

  fn href(&self) -> String {
    self.shared.borrow().as_str().to_string()
  }
}

#[op(name = "URL")]
impl Url {
  #[constructor]
  fn new(input: String, base: Option<String>) -> Result<Url, OpError> {
    let parsed = parse_url(&input, base.as_deref()).map_err(OpError::type_)?;
    Ok(Url::from_parsed(parsed))
  }

  #[static_method]
  #[rename("canParse")]
  fn can_parse(input: String, base: Option<String>) -> bool {
    parse_url(&input, base.as_deref()).is_ok()
  }

  #[static_method]
  #[rename("createObjectURL")]
  fn create_object_url(
    scope: &mut v8::PinScope,
    object: v8::Local<v8::Value>,
  ) -> Result<String, OpError> {
    crate::blob::create_object_url(scope, object)
  }

  #[static_method]
  #[rename("revokeObjectURL")]
  fn revoke_object_url(url: String) {
    crate::blob::revoke_object_url(&url);
  }

  #[static_method]
  fn parse(input: String, base: Option<String>) -> Option<New<Url>> {
    parse_url(&input, base.as_deref())
      .ok()
      .map(Url::from_parsed)
      .map(New)
  }

  #[getter]
  fn href(&self) -> String {
    self.href()
  }

  #[setter]
  fn href(&self, value: String) -> Result<(), OpError> {
    let parsed =
      parse_url(&value, None).map_err(|_| OpError::type_("Invalid URL"))?;
    *self.shared.borrow_mut() = parsed;
    Ok(())
  }

  #[getter]
  fn origin(&self) -> String {
    self.shared.borrow().origin().ascii_serialization()
  }

  #[getter]
  fn protocol(&self) -> String {
    format!("{}:", self.shared.borrow().scheme())
  }

  #[setter]
  fn protocol(&self, value: String) {
    let value = strip_url_space(&value);
    let scheme = value.trim_end_matches(':');
    let _ = self.shared.borrow_mut().set_scheme(scheme);
  }

  #[getter]
  fn username(&self) -> String {
    self.shared.borrow().username().to_string()
  }

  #[setter]
  fn username(&self, value: String) {
    let _ = self.shared.borrow_mut().set_username(&value);
  }

  #[getter]
  fn password(&self) -> String {
    self.shared.borrow().password().unwrap_or("").to_string()
  }

  #[setter]
  fn password(&self, value: String) {
    let _ = self.shared.borrow_mut().set_password(Some(&value));
  }

  #[getter]
  fn host(&self) -> String {
    host_text(&self.shared.borrow())
  }

  #[setter]
  fn host(&self, value: String) {
    let value = strip_url_space(&value);
    let _ = apply_host(&mut self.shared.borrow_mut(), &value, true);
  }

  #[getter]
  fn hostname(&self) -> String {
    self.shared.borrow().host_str().unwrap_or("").to_string()
  }

  #[setter]
  fn hostname(&self, value: String) {
    let value = strip_url_space(&value);
    let _ = apply_host(&mut self.shared.borrow_mut(), &value, false);
  }

  #[getter]
  fn port(&self) -> String {
    self
      .shared
      .borrow()
      .port()
      .map(|port| port.to_string())
      .unwrap_or_default()
  }

  #[setter]
  fn port(&self, value: String) {
    apply_port(&mut self.shared.borrow_mut(), &value);
  }

  #[getter]
  fn pathname(&self) -> String {
    self.shared.borrow().path().to_string()
  }

  #[setter]
  fn pathname(&self, value: String) {
    let value = strip_url_space(&value);
    self.shared.borrow_mut().set_path(&value);
  }

  #[getter]
  fn search(&self) -> String {
    self
      .shared
      .borrow()
      .query()
      .map(|query| format!("?{query}"))
      .unwrap_or_default()
  }

  #[setter]
  fn search(&self, value: String) {
    let mut url = self.shared.borrow_mut();
    if value.is_empty() {
      url.set_query(None);
      return;
    }
    let stripped = strip_url_space(&value);
    let query = stripped.strip_prefix('?').unwrap_or(&stripped);
    if query.is_empty() {
      url.set_query(None);
    } else {
      url.set_query(Some(query));
    }
  }

  #[getter]
  fn hash(&self) -> String {
    self
      .shared
      .borrow()
      .fragment()
      .map(|fragment| format!("#{fragment}"))
      .unwrap_or_default()
  }

  #[setter]
  fn hash(&self, value: String) {
    let mut url = self.shared.borrow_mut();
    if value.is_empty() {
      url.set_fragment(None);
      return;
    }
    let stripped = strip_url_space(&value);
    let fragment = stripped.strip_prefix('#').unwrap_or(&stripped);
    url.set_fragment(Some(fragment));
  }

  #[getter]
  fn search_params(&self, scope: &mut v8::PinScope) -> Js {
    if let Some(existing) = self
      .params
      .borrow()
      .as_ref()
      .and_then(|reference| traced_local(scope, reference))
    {
      let value: v8::Local<v8::Value> = existing.into();
      return Js(v8::Global::new(scope, value));
    }
    let params = UrlSearchParams {
      pairs: RefCell::new(Vec::new()),
      shared: RefCell::new(Some(Rc::clone(&self.shared))),
    };
    let object = unsafe { yuzuko_core::op::wrap_instance(scope, params) };
    *self.params.borrow_mut() = Some(traced(scope, object));
    let value: v8::Local<v8::Value> = object.into();
    Js(v8::Global::new(scope, value))
  }

  #[rename("toString")]
  fn stringify(&self) -> String {
    self.href()
  }

  #[rename("toJSON")]
  fn to_json(&self) -> String {
    self.href()
  }
}

pub struct UrlSearchParams {
  pairs: RefCell<Vec<(String, String)>>,
  shared: RefCell<Option<Rc<RefCell<url::Url>>>>,
}

unsafe impl v8::cppgc::GarbageCollected for UrlSearchParams {
  fn trace(&self, _visitor: &mut v8::cppgc::Visitor) {}

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"URLSearchParams"
  }
}

#[op(name = "URLSearchParams")]
impl UrlSearchParams {
  #[constructor]
  fn new(
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<UrlSearchParams, OpError> {
    if args.length() == 0 || args.get(0).is_undefined() {
      return Ok(UrlSearchParams::empty());
    }
    let init = args.get(0);
    // Primitives, including null, use the string parser. Objects use the
    // Web IDL union: iterable, then record.
    if !init.is_object() {
      let text = usv_string(scope, init)?;
      return Ok(UrlSearchParams::from_pairs(parse_pairs(query_text(&text))));
    }
    let object = v8::Local::<v8::Object>::try_from(init)
      .map_err(|_| OpError::type_("URLSearchParams init is not an object"))?;
    if let Some(method) = iterator_method(scope, object)? {
      return Ok(UrlSearchParams::from_pairs(read_sequence(
        scope, object, method,
      )?));
    }
    Ok(UrlSearchParams::from_pairs(read_record(scope, object)?))
  }

  #[getter]
  fn size(&self) -> i32 {
    self.pairs().len() as i32
  }

  fn append(&self, name: String, value: String) {
    self.mutate(|pairs| pairs.push((name, value)));
  }

  fn delete(&self, name: String, value: Option<String>) {
    self.mutate(|pairs| {
      pairs.retain(|(key, item)| {
        key != &name || value.as_ref().is_some_and(|value| item != value)
      });
    });
  }

  fn get(&self, name: String) -> Option<String> {
    self
      .pairs()
      .into_iter()
      .find(|(key, _)| key == &name)
      .map(|(_, value)| value)
  }

  #[rename("getAll")]
  fn get_all(&self, name: String) -> Vec<String> {
    self
      .pairs()
      .into_iter()
      .filter(|(key, _)| key == &name)
      .map(|(_, value)| value)
      .collect()
  }

  fn has(&self, name: String, value: Option<String>) -> bool {
    self.pairs().iter().any(|(key, item)| {
      key == &name && value.as_ref().is_none_or(|value| item == value)
    })
  }

  fn set(&self, name: String, value: String) {
    self.mutate(|pairs| {
      let mut seen = false;
      pairs.retain(|(key, _)| {
        if key != &name {
          return true;
        }
        if seen {
          return false;
        }
        seen = true;
        true
      });
      if let Some((_, item)) = pairs.iter_mut().find(|(key, _)| key == &name) {
        *item = value;
      } else {
        pairs.push((name, value));
      }
    });
  }

  fn sort(&self) {
    // URLSearchParams sorts by UTF-16 code units, not Unicode scalars.
    self.mutate(|pairs| {
      pairs.sort_by(|left, right| {
        left.0.encode_utf16().cmp(right.0.encode_utf16())
      });
    });
  }

  fn for_each(
    &self,
    scope: &mut v8::PinScope,
    #[this] object: v8::Local<v8::Object>,
    callback: v8::Local<v8::Value>,
    this_arg: Option<v8::Local<v8::Value>>,
  ) {
    let Ok(callback) = v8::Local::<v8::Function>::try_from(callback) else {
      OpException::throw(
        OpError::type_("URLSearchParams.forEach requires a function"),
        scope,
      );
      return;
    };
    let receiver = this_arg.unwrap_or_else(|| v8::undefined(scope).into());
    let pairs = self.pairs();
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
    iterator_object(scope, object, IterKind::Entries)
  }

  fn keys(
    &self,
    scope: &mut v8::PinScope,
    #[this] object: v8::Local<v8::Object>,
  ) -> Js {
    iterator_object(scope, object, IterKind::Keys)
  }

  fn values(
    &self,
    scope: &mut v8::PinScope,
    #[this] object: v8::Local<v8::Object>,
  ) -> Js {
    iterator_object(scope, object, IterKind::Values)
  }

  #[rename("toString")]
  fn stringify(&self) -> String {
    serialize_pairs(&self.pairs())
  }
}

impl UrlSearchParams {
  fn empty() -> Self {
    Self {
      pairs: RefCell::new(Vec::new()),
      shared: RefCell::new(None),
    }
  }

  fn from_pairs(pairs: Vec<(String, String)>) -> Self {
    Self {
      pairs: RefCell::new(pairs),
      shared: RefCell::new(None),
    }
  }

  fn pairs(&self) -> Vec<(String, String)> {
    if let Some(shared) = self.shared.borrow().clone() {
      parse_pairs(shared.borrow().query().unwrap_or(""))
    } else {
      self.pairs.borrow().clone()
    }
  }

  fn mutate(&self, body: impl FnOnce(&mut Vec<(String, String)>)) {
    if let Some(shared) = self.shared.borrow().clone() {
      let mut pairs = parse_pairs(shared.borrow().query().unwrap_or(""));
      body(&mut pairs);
      let query = serialize_pairs(&pairs);
      let mut url = shared.borrow_mut();
      if query.is_empty() {
        url.set_query(None);
      } else {
        url.set_query(Some(&query));
      }
    } else {
      body(&mut self.pairs.borrow_mut());
    }
  }
}

pub fn install(scope: &mut v8::PinScope) {
  Url::install(scope);
  UrlSearchParams::install(scope);
  let constructor = UrlSearchParams::template(scope)
    .get_function(scope)
    .expect("URLSearchParams");
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
}

fn parse_url(input: &str, base: Option<&str>) -> Result<url::Url, String> {
  let parsed = match base {
    Some(base) => {
      let base = url::Url::parse(base).map_err(|error| error.to_string())?;
      url::Url::options()
        .base_url(Some(&base))
        .parse(input)
        .map_err(|error| error.to_string())?
    }
    None => url::Url::parse(input).map_err(|error| error.to_string())?,
  };
  Ok(repair_opaque_space(parsed))
}

/// The URL parser encodes a space immediately before `?` or `#` in an opaque
/// path as `%20`. The `url` crate keeps that space and later strips it.
fn repair_opaque_space(parsed: url::Url) -> url::Url {
  if !parsed.cannot_be_a_base() {
    return parsed;
  }
  let text = parsed.as_str();
  let Some(index) = text.find(['?', '#']).filter(|index| *index > 0) else {
    return parsed;
  };
  if text.as_bytes()[index - 1] != b' ' {
    return parsed;
  }
  let mut repaired = String::with_capacity(text.len() + 2);
  repaired.push_str(&text[..index - 1]);
  repaired.push_str("%20");
  repaired.push_str(&text[index..]);
  url::Url::parse(&repaired).unwrap_or(parsed)
}

fn host_text(url: &url::Url) -> String {
  match (url.host_str(), url.port()) {
    (Some(host), Some(port)) => format!("{host}:{port}"),
    (Some(host), None) => host.to_string(),
    _ => String::new(),
  }
}

fn apply_host(url: &mut url::Url, value: &str, with_port: bool) -> bool {
  if value.is_empty() {
    return url.set_host(None).is_ok();
  }
  if with_port {
    if let Some(rest) = value.strip_prefix('[')
      && let Some((host, port)) = rest.split_once("]:")
    {
      if url.set_host(Some(&format!("[{host}]"))).is_err() {
        return false;
      }
      return set_port_text(url, port);
    }
    if let Some((host, port)) = value.rsplit_once(':')
      && !host.is_empty()
      && !port.contains(':')
    {
      if url.set_host(Some(host)).is_err() {
        return false;
      }
      return set_port_text(url, port);
    }
  }
  url.set_host(Some(value)).is_ok()
}

fn set_port_text(url: &mut url::Url, port: &str) -> bool {
  if port.is_empty() {
    url.set_port(None).is_ok()
  } else if let Ok(port) = port.parse::<u16>() {
    url.set_port(Some(port)).is_ok()
  } else {
    false
  }
}

fn parse_pairs(query: &str) -> Vec<(String, String)> {
  url::form_urlencoded::parse(query.as_bytes())
    .into_owned()
    .collect()
}

fn serialize_pairs(pairs: &[(String, String)]) -> String {
  let mut serializer = url::form_urlencoded::Serializer::new(String::new());
  for (name, value) in pairs {
    serializer.append_pair(name, value);
  }
  serializer.finish()
}

fn strip_url_space(value: &str) -> String {
  value
    .chars()
    .filter(|ch| !matches!(ch, '\t' | '\n' | '\r'))
    .collect()
}

/// Port state override: collect leading digits, then stop. A non-digit after
/// at least one digit still commits that port. No digits is failure.
fn apply_port(url: &mut url::Url, value: &str) {
  if value.is_empty() {
    let _ = url.set_port(None);
    return;
  }
  let stripped = strip_url_space(value);
  let mut port: u32 = 0;
  let mut saw_digit = false;
  for byte in stripped.bytes() {
    if !byte.is_ascii_digit() {
      break;
    }
    saw_digit = true;
    port = port
      .saturating_mul(10)
      .saturating_add(u32::from(byte - b'0'));
    if port > u32::from(u16::MAX) {
      return;
    }
  }
  if saw_digit {
    let _ = url.set_port(Some(port as u16));
  }
}

fn query_text(text: &str) -> &str {
  text.strip_prefix('?').unwrap_or(text)
}

fn usv_string(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<String, OpError> {
  let captured = {
    v8::tc_scope!(let tc, scope);
    match value.to_string(tc) {
      Some(text) => Capture::Value(text.to_rust_string_lossy(tc)),
      None => Capture::Thrown(exception_global(tc)),
    }
  };
  finish(scope, captured)
}

fn iterator_method<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  object: v8::Local<v8::Object>,
) -> Result<Option<v8::Local<'s, v8::Function>>, OpError> {
  let key = v8::Symbol::get_iterator(scope).into();
  let value = js_get(scope, object, key)?;
  let value = v8::Local::new(scope, value);
  if value.is_null() || value.is_undefined() {
    return Ok(None);
  }
  v8::Local::<v8::Function>::try_from(value)
    .map(Some)
    .map_err(|_| OpError::type_("@@iterator is not callable"))
}

fn read_sequence(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  method: v8::Local<v8::Function>,
) -> Result<Vec<(String, String)>, OpError> {
  let iter = open_iterator(scope, object.into(), method)?;
  let mut pairs = Vec::new();
  while let Some(item) = iter_next(scope, &iter)? {
    let item = v8::Local::new(scope, item);
    let mut values = read_usv_sequence(scope, item)?;
    if values.len() != 2 {
      return Err(OpError::type_(
        "URLSearchParams pair must contain exactly two items",
      ));
    }
    let value = values.pop().unwrap();
    let name = values.pop().unwrap();
    pairs.push((name, value));
  }
  Ok(pairs)
}

fn read_usv_sequence(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<Vec<String>, OpError> {
  let object = v8::Local::<v8::Object>::try_from(value).map_err(|_| {
    OpError::type_("URLSearchParams sequence element is not iterable")
  })?;
  let Some(method) = iterator_method(scope, object)? else {
    return Err(OpError::type_(
      "URLSearchParams sequence element is not iterable",
    ));
  };
  let iter = open_iterator(scope, object.into(), method)?;
  let mut values = Vec::new();
  while let Some(item) = iter_next(scope, &iter)? {
    let item = v8::Local::new(scope, item);
    values.push(usv_string(scope, item)?);
  }
  Ok(values)
}

fn read_record(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
) -> Result<Vec<(String, String)>, OpError> {
  let args = v8::GetPropertyNamesArgsBuilder::new()
    .property_filter(
      v8::PropertyFilter::ONLY_ENUMERABLE | v8::PropertyFilter::SKIP_SYMBOLS,
    )
    .key_conversion(v8::KeyConversionMode::ConvertToString)
    .build();
  let names = {
    v8::tc_scope!(let tc, scope);
    match object.get_own_property_names(tc, args) {
      Some(names) => Capture::Value(v8::Global::new(tc, names)),
      None => Capture::Thrown(exception_global(tc)),
    }
  };
  let names = finish(scope, names)?;
  let names = v8::Local::new(scope, names);
  let mut pairs = Vec::new();
  for index in 0..names.length() {
    let Some(key) = names.get_index(scope, index) else {
      return Err(OpError::type_("failed to read a record key"));
    };
    let name = usv_string(scope, key)?;
    let value = js_get(scope, object, key)?;
    let value = v8::Local::new(scope, value);
    let value = usv_string(scope, value)?;
    if let Some((_, existing)) = pairs.iter_mut().find(|(key, _)| key == &name)
    {
      *existing = value;
    } else {
      pairs.push((name, value));
    }
  }
  Ok(pairs)
}

struct JsIter {
  iterator: v8::Global<v8::Object>,
  next: v8::Global<v8::Function>,
}

fn open_iterator(
  scope: &mut v8::PinScope,
  target: v8::Local<v8::Value>,
  method: v8::Local<v8::Function>,
) -> Result<JsIter, OpError> {
  let iterator = call(scope, method, target, &[])?;
  let iterator = v8::Local::new(scope, iterator);
  let iterator = v8::Local::<v8::Object>::try_from(iterator)
    .map_err(|_| OpError::type_("iterator is not an object"))?;
  let next_key = v8::String::new(scope, "next").unwrap().into();
  let next = js_get(scope, iterator, next_key)?;
  let next = v8::Local::new(scope, next);
  let next = v8::Local::<v8::Function>::try_from(next)
    .map_err(|_| OpError::type_("iterator next is not a function"))?;
  Ok(JsIter {
    iterator: v8::Global::new(scope, iterator),
    next: v8::Global::new(scope, next),
  })
}

fn iter_next(
  scope: &mut v8::PinScope,
  iter: &JsIter,
) -> Result<Option<v8::Global<v8::Value>>, OpError> {
  let iterator = v8::Local::new(scope, &iter.iterator);
  let next = v8::Local::new(scope, &iter.next);
  let result = call(scope, next, iterator.into(), &[])?;
  let result = v8::Local::new(scope, result);
  let result = v8::Local::<v8::Object>::try_from(result)
    .map_err(|_| OpError::type_("iterator result is not an object"))?;
  let done_key = v8::String::new(scope, "done").unwrap().into();
  let done = js_get(scope, result, done_key)?;
  let done = v8::Local::new(scope, done);
  if done.boolean_value(scope) {
    return Ok(None);
  }
  let value_key = v8::String::new(scope, "value").unwrap().into();
  Ok(Some(js_get(scope, result, value_key)?))
}

fn call(
  scope: &mut v8::PinScope,
  function: v8::Local<v8::Function>,
  receiver: v8::Local<v8::Value>,
  args: &[v8::Local<v8::Value>],
) -> Result<v8::Global<v8::Value>, OpError> {
  let captured = {
    v8::tc_scope!(let tc, scope);
    match function.call(tc, receiver, args) {
      Some(value) => Capture::Value(v8::Global::new(tc, value)),
      None => Capture::Thrown(exception_global(tc)),
    }
  };
  finish(scope, captured)
}

fn js_get(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  key: v8::Local<v8::Value>,
) -> Result<v8::Global<v8::Value>, OpError> {
  let captured = {
    v8::tc_scope!(let tc, scope);
    match object.get(tc, key) {
      Some(value) => Capture::Value(v8::Global::new(tc, value)),
      None => Capture::Thrown(exception_global(tc)),
    }
  };
  finish(scope, captured)
}

enum Capture<T> {
  Value(T),
  Thrown(v8::Global<v8::Value>),
}

fn finish<T>(
  scope: &mut v8::PinScope,
  captured: Capture<T>,
) -> Result<T, OpError> {
  match captured {
    Capture::Value(value) => Ok(value),
    Capture::Thrown(exception) => {
      let exception = v8::Local::new(scope, exception);
      Err(OpError::thrown(scope, exception))
    }
  }
}

fn exception_global(
  scope: &mut v8::PinnedRef<v8::TryCatch<v8::HandleScope>>,
) -> v8::Global<v8::Value> {
  let exception = scope.exception().unwrap_or_else(|| {
    let message =
      v8::String::new(scope, "URLSearchParams conversion failed").unwrap();
    v8::Exception::type_error(scope, message)
  });
  v8::Global::new(scope, exception)
}

#[derive(Clone, Copy)]
enum IterKind {
  Entries,
  Keys,
  Values,
}

struct SearchIter {
  target: v8::TracedReference<v8::Object>,
  index: Cell<usize>,
  kind: IterKind,
}

unsafe impl v8::cppgc::GarbageCollected for SearchIter {
  fn trace(&self, visitor: &mut v8::cppgc::Visitor) {
    visitor.trace(&self.target);
  }

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"URLSearchParams Iterator"
  }
}

#[op(name = "URLSearchParams Iterator")]
impl SearchIter {
  fn next(&self, scope: &mut v8::PinScope) -> Js {
    let done = iterator_result(scope, true, v8::undefined(scope).into());
    let Some(object) = traced_local(scope, &self.target) else {
      return done;
    };
    let Some(host) = (unsafe { host_ref::<UrlSearchParams>(scope, object) })
    else {
      return done;
    };
    let pairs = host.get().pairs();
    let index = self.index.get();
    if index >= pairs.len() {
      return done;
    }
    self.index.set(index + 1);
    let (name, value) = &pairs[index];
    let Some(name) = v8::String::new(scope, name) else {
      return done;
    };
    let Some(value) = v8::String::new(scope, value) else {
      return done;
    };
    let item = match self.kind {
      IterKind::Entries => {
        let array = v8::Array::new(scope, 2);
        array.set_index(scope, 0, name.into());
        array.set_index(scope, 1, value.into());
        array.into()
      }
      IterKind::Keys => name.into(),
      IterKind::Values => value.into(),
    };
    iterator_result(scope, false, item)
  }
}

fn iterator_result(
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

fn iterator_object(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  kind: IterKind,
) -> Js {
  let _ = SearchIter::template(scope);
  let target = traced(scope, object);
  let iter = unsafe {
    wrap_instance(
      scope,
      SearchIter {
        target,
        index: Cell::new(0),
        kind,
      },
    )
  };
  let symbol = v8::Symbol::get_iterator(scope);
  let function = v8::Function::new(scope, iterate_self).unwrap();
  iter.set(scope, symbol.into(), function.into());
  let value: v8::Local<v8::Value> = iter.into();
  Js(v8::Global::new(scope, value))
}

fn iterate_self(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut ret: v8::ReturnValue,
) {
  let object = args.this();
  if unsafe { host_ref::<SearchIter>(scope, object) }.is_none() {
    OpException::throw(OpError::type_("Illegal invocation"), scope);
    return;
  }
  ret.set(object.into());
}
