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
    self.entries.borrow_mut().push(Entry {
      name,
      value: read_part(scope, value, filename)?,
    });
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
      array.set_index(scope, index as u32, local);
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
    c"FormDataIterator"
  }
}

#[op]
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
          array.set_index(scope, 0, key.into());
          array.set_index(scope, 1, item);
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
  object.set(scope, done_key.into(), v8::Boolean::new(scope, done).into());
  object.set(scope, value_key.into(), value);
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
  let text = value
    .to_string(scope)
    .map(|text| text.to_rust_string_lossy(scope))
    .unwrap_or_default();
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
    body.extend_from_slice(entry.name.as_bytes());
    body.extend_from_slice(b"\"");
    match &entry.value {
      Part::Text(text) => {
        body.extend_from_slice(b"\r\n\r\n");
        body.extend_from_slice(text.as_bytes());
      }
      Part::File {
        bytes,
        mime,
        filename,
      } => {
        body.extend_from_slice(b"; filename=\"");
        body.extend_from_slice(filename.as_bytes());
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

pub fn parse(mime: &str, body: &[u8]) -> Result<FormData, OpError> {
  let mime = mime.to_ascii_lowercase();
  let entries = if mime.starts_with("multipart/form-data") {
    parse_multipart(&mime, body)?
  } else if mime.starts_with("application/x-www-form-urlencoded") {
    parse_urlencoded(body)
  } else {
    return Err(OpError::type_("The response body is not form data"));
  };
  Ok(FormData {
    entries: RefCell::new(entries),
    wrapper: RefCell::new(None),
  })
}

fn parse_urlencoded(body: &[u8]) -> Vec<Entry> {
  let text = String::from_utf8_lossy(body);
  text
    .split('&')
    .filter(|pair| !pair.is_empty())
    .map(|pair| {
      let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
      Entry {
        name: decode_component(name),
        value: Part::Text(decode_component(value)),
      }
    })
    .collect()
}

fn parse_multipart(mime: &str, body: &[u8]) -> Result<Vec<Entry>, OpError> {
  let boundary = mime
    .split(';')
    .filter_map(|part| part.trim().strip_prefix("boundary="))
    .next()
    .map(|value| value.trim_matches('"').to_string())
    .ok_or_else(|| OpError::type_("The multipart body has no boundary"))?;
  let marker = format!("--{boundary}");
  let text = String::from_utf8_lossy(body);
  if text.is_empty() {
    return Ok(Vec::new());
  }
  let closing = format!("{marker}--");
  if !text.contains(&closing) {
    return Err(OpError::type_("The multipart body is incomplete"));
  }
  let mut entries = Vec::new();
  for part in text.split(&marker).skip(1) {
    let part = part.trim_start_matches("\r\n");
    if part.starts_with("--") || part.is_empty() {
      continue;
    }
    let Some((headers, content)) = part.split_once("\r\n\r\n") else {
      continue;
    };
    let content = content.strip_suffix("\r\n").unwrap_or(content);
    let name = header_param(headers, "name").unwrap_or_default();
    if let Some(filename) = header_param(headers, "filename") {
      let mime = headers
        .lines()
        .find(|line| line.to_ascii_lowercase().starts_with("content-type:"))
        .and_then(|line| line.split_once(':'))
        .map(|(_, value)| value.trim().to_string())
        .unwrap_or_default();
      entries.push(Entry {
        name,
        value: Part::File {
          bytes: content.as_bytes().to_vec(),
          mime,
          filename,
        },
      });
    } else {
      entries.push(Entry {
        name,
        value: Part::Text(content.to_string()),
      });
    }
  }
  Ok(entries)
}

fn header_param(headers: &str, name: &str) -> Option<String> {
  let line = headers.lines().find(|line| {
    line
      .to_ascii_lowercase()
      .starts_with("content-disposition:")
  })?;
  let needle = format!("{name}=\"");
  let rest = line.split(&needle).nth(1)?;
  Some(rest.split('"').next().unwrap_or("").to_string())
}

fn decode_component(value: &str) -> String {
  let value = value.replace('+', " ");
  percent_decode(&value)
}

fn percent_decode(input: &str) -> String {
  let bytes = input.as_bytes();
  let mut out = Vec::new();
  let mut index = 0;
  while index < bytes.len() {
    if bytes[index] == b'%'
      && index + 2 < bytes.len()
      && let (Some(high), Some(low)) =
        (hex(bytes[index + 1]), hex(bytes[index + 2]))
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

fn hex(byte: u8) -> Option<u8> {
  match byte {
    b'0'..=b'9' => Some(byte - b'0'),
    b'a'..=b'f' => Some(byte - b'a' + 10),
    b'A'..=b'F' => Some(byte - b'A' + 10),
    _ => None,
  }
}

pub fn install(scope: &mut v8::PinScope) {
  FormData::install(scope);
  let _ = FormIter::template(scope);
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
