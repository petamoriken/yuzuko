//! `FormData` entries, multipart bodies, and `application/x-www-form-urlencoded`.

use std::cell::Cell;
use std::cell::RefCell;

use yuzuko_core::op::Js;
use yuzuko_core::op::OpError;
use yuzuko_core::op::host_ref;
use yuzuko_core::op::traced;
use yuzuko_core::op::traced_local;
use yuzuko_macros::op;

use crate::blob::Blob;
use crate::blob::File;

enum Part {
  Text(String),
  File {
    bytes: Vec<u8>,
    mime: String,
    filename: String,
  },
}

struct Entry {
  name: String,
  value: Part,
}

pub struct FormData {
  entries: RefCell<Vec<Entry>>,
  wrapper: RefCell<Option<v8::TracedReference<v8::Object>>>,
}

unsafe impl v8::cppgc::GarbageCollected for FormData {
  fn trace(&self, visitor: &mut v8::cppgc::Visitor) {
    if let Ok(wrapper) = self.wrapper.try_borrow() {
      visitor.trace(&*wrapper);
    }
  }

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"FormData"
  }
}

#[op]
impl FormData {
  #[constructor]
  fn new(
    _scope: &mut v8::PinScope,
    form: Option<v8::Local<v8::Value>>,
  ) -> Result<Self, OpError> {
    if form.is_some_and(|value| !value.is_null_or_undefined()) {
      return Err(OpError::type_(
        "FormData from an HTML form is not supported",
      ));
    }
    Ok(Self {
      entries: RefCell::new(Vec::new()),
      wrapper: RefCell::new(None),
    })
  }

  #[construct_hook]
  fn finish(&self, scope: &mut v8::PinScope, object: v8::Local<v8::Object>) {
    *self.wrapper.borrow_mut() = Some(traced(scope, object));
  }

  fn append(
    &self,
    scope: &mut v8::PinScope,
    name: String,
    value: v8::Local<v8::Value>,
    filename: Option<String>,
  ) -> Result<(), OpError> {
    // `read_part` runs a user `toString`, which may call back into this
    // FormData, so the entries are borrowed only after it returns.
    let value = read_part(scope, value, filename)?;
    self.entries.borrow_mut().push(Entry { name, value });
    Ok(())
  }

  fn delete(&self, name: String) {
    self.entries.borrow_mut().retain(|entry| entry.name != name);
  }

  fn get(&self, scope: &mut v8::PinScope, name: String) -> Js {
    let entries = self.entries.borrow();
    let Some(entry) = entries.iter().find(|entry| entry.name == name) else {
      let value: v8::Local<v8::Value> = v8::null(scope).into();
      return Js(v8::Global::new(scope, value));
    };
    part_value(scope, &entry.value)
  }

  fn get_all(&self, scope: &mut v8::PinScope, name: String) -> Js {
    let entries = self.entries.borrow();
    let matched = entries
      .iter()
      .filter(|entry| entry.name == name)
      .collect::<Vec<_>>();
    let array = v8::Array::new(scope, matched.len() as i32);
    for (index, entry) in matched.iter().enumerate() {
      let value = part_value(scope, &entry.value);
      let local = v8::Local::new(scope, &value.0);
      yuzuko_core::define_element(scope, array.into(), index as u32, local);
    }
    let value: v8::Local<v8::Value> = array.into();
    Js(v8::Global::new(scope, value))
  }

  fn has(&self, name: String) -> bool {
    self.entries.borrow().iter().any(|entry| entry.name == name)
  }

  fn set(
    &self,
    scope: &mut v8::PinScope,
    name: String,
    value: v8::Local<v8::Value>,
    filename: Option<String>,
  ) -> Result<(), OpError> {
    let part = read_part(scope, value, filename)?;
    let mut entries = self.entries.borrow_mut();
    entries.retain(|entry| entry.name != name);
    entries.push(Entry { name, value: part });
    Ok(())
  }

  fn for_each(
    &self,
    scope: &mut v8::PinScope,
    callback: v8::Local<v8::Function>,
    this_arg: Option<v8::Local<v8::Value>>,
  ) -> Result<(), OpError> {
    let this = this_arg.unwrap_or_else(|| v8::undefined(scope).into());
    let form = self
      .wrapper
      .borrow()
      .as_ref()
      .and_then(|reference| traced_local(scope, reference))
      .map(v8::Local::<v8::Value>::from)
      .unwrap_or_else(|| v8::undefined(scope).into());
    let snapshot = self
      .entries
      .borrow()
      .iter()
      .map(|entry| (entry.name.clone(), part_owned(&entry.value)))
      .collect::<Vec<_>>();
    for (name, part) in snapshot {
      let value = part_value(scope, &part);
      let value = v8::Local::new(scope, &value.0);
      let key = v8::String::new(scope, &name).unwrap();
      let returned = {
        v8::tc_scope!(let tc, scope);
        callback.call(tc, this, &[value, key.into(), form])
      };
      if returned.is_none() {
        return Err(OpError::type_("The FormData callback failed"));
      }
    }
    Ok(())
  }

  fn entries(&self, scope: &mut v8::PinScope) -> Result<Js, OpError> {
    iterator(scope, self, IterKind::Entries)
  }

  fn keys(&self, scope: &mut v8::PinScope) -> Result<Js, OpError> {
    iterator(scope, self, IterKind::Keys)
  }

  fn values(&self, scope: &mut v8::PinScope) -> Result<Js, OpError> {
    iterator(scope, self, IterKind::Values)
  }
}

#[derive(Clone, Copy)]
enum IterKind {
  Entries,
  Keys,
  Values,
}

struct FormIter {
  form: v8::TracedReference<v8::Object>,
  index: Cell<u32>,
  kind: IterKind,
}

unsafe impl v8::cppgc::GarbageCollected for FormIter {
  fn trace(&self, visitor: &mut v8::cppgc::Visitor) {
    visitor.trace(&self.form);
  }

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"FormData Iterator"
  }
}

#[op(name = "FormData Iterator")]
impl FormIter {
  fn next(&self, scope: &mut v8::PinScope) -> Result<Js, OpError> {
    let form = traced_local(scope, &self.form)
      .ok_or_else(|| OpError::type_("The FormData iterator is detached"))?;
    let host = unsafe { host_ref::<FormData>(scope, form) }
      .ok_or_else(|| OpError::type_("The FormData iterator is detached"))?;
    let index = self.index.get() as usize;
    let entry = host
      .get()
      .entries
      .borrow()
      .get(index)
      .map(|entry| (entry.name.clone(), part_owned(&entry.value)));
    let done = entry.is_none();
    if !done {
      self.index.set(self.index.get() + 1);
    }
    let value = match entry {
      Some((name, part)) => match self.kind {
        IterKind::Keys => {
          let text = v8::String::new(scope, &name).unwrap();
          text.into()
        }
        IterKind::Values => {
          let value = part_value(scope, &part);
          v8::Local::new(scope, &value.0)
        }
        IterKind::Entries => {
          let array = v8::Array::new(scope, 2);
          let key = v8::String::new(scope, &name).unwrap();
          let item = part_value(scope, &part);
          let item = v8::Local::new(scope, &item.0);
          yuzuko_core::define_element(scope, array.into(), 0, key.into());
          yuzuko_core::define_element(scope, array.into(), 1, item);
          array.into()
        }
      },
      None => v8::undefined(scope).into(),
    };
    Ok(pair(scope, done, value))
  }
}

fn iterator(
  scope: &mut v8::PinScope,
  form: &FormData,
  kind: IterKind,
) -> Result<Js, OpError> {
  let _ = FormIter::template(scope);
  let object = form
    .wrapper
    .borrow()
    .as_ref()
    .and_then(|reference| traced_local(scope, reference))
    .ok_or_else(|| OpError::type_("The FormData object is detached"))?;
  let object = yuzuko_core::op::root(scope, object);
  let form = traced(scope, object);
  let iter = unsafe {
    yuzuko_core::op::wrap_instance(
      scope,
      FormIter {
        form,
        index: Cell::new(0),
        kind,
      },
    )
  };
  let value: v8::Local<v8::Value> = iter.into();
  Ok(Js(v8::Global::new(scope, value)))
}

fn pair(
  scope: &mut v8::PinScope,
  done: bool,
  value: v8::Local<v8::Value>,
) -> Js {
  let object = v8::Object::new(scope);
  let done_key = v8::String::new(scope, "done").unwrap();
  let value_key = v8::String::new(scope, "value").unwrap();
  let _ = object.create_data_property(
    scope,
    done_key.into(),
    v8::Boolean::new(scope, done).into(),
  );
  let _ = object.create_data_property(scope, value_key.into(), value);
  let value: v8::Local<v8::Value> = object.into();
  Js(v8::Global::new(scope, value))
}

fn read_part(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
  filename: Option<String>,
) -> Result<Part, OpError> {
  if let Ok(object) = v8::Local::<v8::Object>::try_from(value) {
    if let Some(file) = unsafe { host_ref::<File>(scope, object) } {
      let (bytes, mime, name) = file.get().form_parts()?;
      return Ok(Part::File {
        bytes,
        mime,
        filename: filename.unwrap_or(name),
      });
    }
    if let Some(blob) = unsafe { host_ref::<Blob>(scope, object) } {
      let (bytes, mime) = blob.get().snapshot()?;
      return Ok(Part::File {
        bytes,
        mime,
        filename: filename.unwrap_or_else(|| "blob".to_string()),
      });
    }
  }
  let text = yuzuko_core::rust_string_or(scope, value, "");
  Ok(Part::Text(text))
}

fn part_owned(part: &Part) -> Part {
  match part {
    Part::Text(text) => Part::Text(text.clone()),
    Part::File {
      bytes,
      mime,
      filename,
    } => Part::File {
      bytes: bytes.clone(),
      mime: mime.clone(),
      filename: filename.clone(),
    },
  }
}

fn part_value(scope: &mut v8::PinScope, part: &Part) -> Js {
  match part {
    Part::Text(text) => {
      let text = v8::String::new(scope, text).unwrap();
      let value: v8::Local<v8::Value> = text.into();
      Js(v8::Global::new(scope, value))
    }
    Part::File {
      bytes,
      mime,
      filename,
    } => {
      let file = unsafe {
        yuzuko_core::op::wrap_instance(
          scope,
          File::from_owned(bytes.clone(), mime.clone(), filename.clone()),
        )
      };
      let value: v8::Local<v8::Value> = file.into();
      Js(v8::Global::new(scope, value))
    }
  }
}

#[cfg_attr(not(feature = "fetch"), allow(dead_code))]
pub fn encode(form: &FormData) -> (Vec<u8>, String) {
  if form.entries.borrow().is_empty() {
    return (
      Vec::new(),
      "multipart/form-data; boundary=yuzuko".to_string(),
    );
  }
  let mut boundary = [0u8; 8];
  let _ = getrandom::fill(&mut boundary);
  let boundary = boundary
    .iter()
    .map(|byte| format!("{byte:02x}"))
    .collect::<String>();
  let mut body = Vec::new();
  for entry in form.entries.borrow().iter() {
    body.extend_from_slice(b"--");
    body.extend_from_slice(boundary.as_bytes());
    body.extend_from_slice(b"\r\nContent-Disposition: form-data; name=\"");
    body.extend_from_slice(escape_quoted(&crlf(&entry.name)).as_bytes());
    body.extend_from_slice(b"\"");
    match &entry.value {
      Part::Text(text) => {
        body.extend_from_slice(b"\r\n\r\n");
        body.extend_from_slice(crlf(text).as_bytes());
      }
      Part::File {
        bytes,
        mime,
        filename,
      } => {
        body.extend_from_slice(b"; filename=\"");
        body.extend_from_slice(escape_quoted(filename).as_bytes());
        body.extend_from_slice(b"\"\r\nContent-Type: ");
        if mime.is_empty() {
          body.extend_from_slice(b"application/octet-stream");
        } else {
          body.extend_from_slice(mime.as_bytes());
        }
        body.extend_from_slice(b"\r\n\r\n");
        body.extend_from_slice(bytes);
      }
    }
    body.extend_from_slice(b"\r\n");
  }
  body.extend_from_slice(b"--");
  body.extend_from_slice(boundary.as_bytes());
  body.extend_from_slice(b"--\r\n");
  (body, format!("multipart/form-data; boundary={boundary}"))
}

/// HTML's multipart/form-data encoding replaces a lone CR or LF in a name
/// or a string value with CRLF.
#[cfg_attr(not(feature = "fetch"), allow(dead_code))]
fn crlf(text: &str) -> std::borrow::Cow<'_, str> {
  if !text.contains(['\r', '\n']) {
    return std::borrow::Cow::Borrowed(text);
  }
  let mut out = String::with_capacity(text.len() + 8);
  let mut chars = text.chars().peekable();
  while let Some(ch) = chars.next() {
    match ch {
      '\r' => {
        out.push_str("\r\n");
        if chars.peek() == Some(&'\n') {
          chars.next();
        }
      }
      '\n' => out.push_str("\r\n"),
      other => out.push(other),
    }
  }
  std::borrow::Cow::Owned(out)
}

/// A name or file name inside a quoted `Content-Disposition` parameter
/// escapes LF, CR, and `"` as `%0A`, `%0D`, and `%22`, so it cannot end the
/// parameter or the header.
#[cfg_attr(not(feature = "fetch"), allow(dead_code))]
fn escape_quoted(text: &str) -> String {
  text
    .replace('\n', "%0A")
    .replace('\r', "%0D")
    .replace('"', "%22")
}

/// Wraps a `FormData` the host built, such as a `formData()` result. The
/// constructor's hook does not run for it, so this records the wrapper that
/// iteration and `forEach` hand back to script.
#[cfg_attr(not(feature = "fetch"), allow(dead_code))]
pub fn wrap<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  form: FormData,
) -> v8::Local<'s, v8::Object> {
  let object = unsafe { yuzuko_core::op::wrap_instance(scope, form) };
  if let Some(host) =
    unsafe { yuzuko_core::op::host_ref::<FormData>(scope, object) }
  {
    *host.get().wrapper.borrow_mut() = Some(traced(scope, object));
  }
  object
}

#[cfg_attr(not(feature = "fetch"), allow(dead_code))]
pub fn parse(mime: &str, body: &[u8]) -> Result<FormData, OpError> {
  let lower = mime.to_ascii_lowercase();
  // The boundary is compared byte for byte, so it keeps its case.
  let entries = if lower.starts_with("multipart/form-data") {
    parse_multipart(mime, body)?
  } else if lower.starts_with("application/x-www-form-urlencoded") {
    parse_urlencoded(body)
  } else {
    return Err(OpError::type_("The response body is not form data"));
  };
  Ok(FormData {
    entries: RefCell::new(entries),
    wrapper: RefCell::new(None),
  })
}

/// The URL Standard `application/x-www-form-urlencoded` parser. It
/// percent-decodes bytes before the UTF-8 decode, so an escape can complete
/// a sequence that starts with a raw byte.
#[cfg_attr(not(feature = "fetch"), allow(dead_code))]
fn parse_urlencoded(body: &[u8]) -> Vec<Entry> {
  url::form_urlencoded::parse(body)
    .map(|(name, value)| Entry {
      name: name.into_owned(),
      value: Part::Text(value.into_owned()),
    })
    .collect()
}

#[cfg_attr(not(feature = "fetch"), allow(dead_code))]
fn parse_multipart(mime: &str, body: &[u8]) -> Result<Vec<Entry>, OpError> {
  let boundary = mime
    .split(';')
    .skip(1)
    .filter_map(|part| part.split_once('='))
    .find(|(key, _)| key.trim().eq_ignore_ascii_case("boundary"))
    .map(|(_, value)| unquote(value.trim()))
    .filter(|value| !value.is_empty())
    .ok_or_else(|| OpError::type_("The multipart body has no boundary"))?;
  if body.is_empty() {
    return Ok(Vec::new());
  }
  let incomplete = || OpError::type_("The multipart body is incomplete");
  let marker = [b"--", boundary.as_bytes()].concat();
  // Each later delimiter follows the CRLF that ends the previous part.
  let delimiter = [b"\r\n", marker.as_slice()].concat();
  let mut position = find(body, &marker, 0).ok_or_else(incomplete)?;
  position += marker.len();
  let mut entries = Vec::new();
  loop {
    if body[position..].starts_with(b"--") {
      return Ok(entries);
    }
    // Transport padding after a delimiter is ignored.
    while matches!(body.get(position), Some(b' ' | b'\t')) {
      position += 1;
    }
    if !body[position..].starts_with(b"\r\n") {
      return Err(incomplete());
    }
    position += 2;
    let (headers, content_start) = if body[position..].starts_with(b"\r\n") {
      (std::borrow::Cow::Borrowed(""), position + 2)
    } else {
      let head_end =
        find(body, b"\r\n\r\n", position).ok_or_else(incomplete)?;
      (
        String::from_utf8_lossy(&body[position..head_end]),
        head_end + 4,
      )
    };
    let content_end =
      find(body, &delimiter, content_start).ok_or_else(incomplete)?;
    let content = &body[content_start..content_end];
    position = content_end + delimiter.len();
    let Some(disposition) = header_value(&headers, "content-disposition")
    else {
      continue;
    };
    let name = disposition_param(disposition, "name").unwrap_or_default();
    if let Some(filename) = disposition_param(disposition, "filename") {
      let mime = header_value(&headers, "content-type")
        .map(str::to_string)
        .unwrap_or_default();
      entries.push(Entry {
        name,
        value: Part::File {
          bytes: content.to_vec(),
          mime,
          filename,
        },
      });
    } else {
      entries.push(Entry {
        name,
        value: Part::Text(String::from_utf8_lossy(content).into_owned()),
      });
    }
  }
}

#[cfg_attr(not(feature = "fetch"), allow(dead_code))]
fn find(haystack: &[u8], needle: &[u8], from: usize) -> Option<usize> {
  if needle.is_empty() || from > haystack.len() {
    return None;
  }
  haystack[from..]
    .windows(needle.len())
    .position(|window| window == needle)
    .map(|index| from + index)
}

#[cfg_attr(not(feature = "fetch"), allow(dead_code))]
fn header_value<'a>(headers: &'a str, name: &str) -> Option<&'a str> {
  headers.split("\r\n").find_map(|line| {
    let (key, value) = line.split_once(':')?;
    key.trim().eq_ignore_ascii_case(name).then(|| value.trim())
  })
}

/// A `Content-Disposition` parameter. The value may be quoted, and a quoted
/// value may contain `;`.
#[cfg_attr(not(feature = "fetch"), allow(dead_code))]
fn disposition_param(value: &str, name: &str) -> Option<String> {
  let mut rest = value.split_once(';')?.1;
  loop {
    let (key, after) = rest.split_once('=')?;
    let after = after.trim_start();
    let (raw, next) = if let Some(quoted) = after.strip_prefix('"') {
      let end = quoted.find('"')?;
      (&quoted[..end], &quoted[end + 1..])
    } else {
      match after.find(';') {
        Some(end) => (&after[..end], &after[end..]),
        None => (after, ""),
      }
    };
    if key.trim().eq_ignore_ascii_case(name) {
      return Some(raw.trim().to_string());
    }
    rest = next.split_once(';').map(|(_, rest)| rest).unwrap_or("");
    if rest.is_empty() {
      return None;
    }
  }
}

#[cfg_attr(not(feature = "fetch"), allow(dead_code))]
fn unquote(value: &str) -> String {
  value
    .strip_prefix('"')
    .and_then(|value| value.strip_suffix('"'))
    .unwrap_or(value)
    .to_string()
}

#[cfg_attr(not(feature = "fetch"), allow(dead_code))]
pub fn install(scope: &mut v8::PinScope) {
  FormData::install(scope);
  let iterator = FormIter::template(scope);
  crate::util::adopt_iterator_prototype(scope, iterator);
  let templ = FormData::template(scope);
  let ctor = templ.get_function(scope).unwrap();
  let key = v8::String::new(scope, "prototype").unwrap();
  let prototype = ctor.get(scope, key.into()).unwrap();
  let Ok(prototype) = v8::Local::<v8::Object>::try_from(prototype) else {
    return;
  };
  let entries = prototype
    .get(scope, v8::String::new(scope, "entries").unwrap().into())
    .unwrap();
  let symbol = v8::Symbol::get_iterator(scope);
  let _ = prototype.define_own_property(
    scope,
    symbol.into(),
    entries,
    v8::PropertyAttribute::DONT_ENUM,
  );
}
