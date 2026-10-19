//! `navigator.clipboard`, `ClipboardItem`, and `copy` / `cut` / `paste`.
//!
//! Reads and writes require a recent user gesture on the focused window.
//! Text is published to the OS pasteboard. `image/png` bytes are kept so a
//! later `getType` returns the same PNG.

use std::cell::RefCell;
use std::io::Cursor;

use yuzuko_core::op::Js;
use yuzuko_core::op::OpError;
use yuzuko_dom::Event;
use yuzuko_dom::EventTarget;
use yuzuko_dom::define_is_trusted;
use yuzuko_macros::op;

use crate::activation;

const TEXT: &str = "text/plain";
const PNG: &str = "image/png";

struct Contents {
  text: Option<String>,
  png: Option<Vec<u8>>,
}

std::thread_local! {
  static CONTENTS: RefCell<Contents> = const {
    RefCell::new(Contents {
      text: None,
      png: None,
    })
  };
  static BOARD: RefCell<Option<arboard::Clipboard>> =
    const { RefCell::new(None) };
  static CLIPBOARD: RefCell<Option<v8::Global<v8::Object>>> =
    const { RefCell::new(None) };
}

pub fn install(scope: &mut v8::PinScope) {
  Clipboard::install(scope);
  ClipboardItem::install(scope);
  ClipboardEvent::install(scope);
}

pub fn instance(scope: &mut v8::PinScope) -> Js {
  let existing = CLIPBOARD.with(|slot| {
    slot
      .borrow()
      .as_ref()
      .map(|value| v8::Local::new(scope, value))
  });
  let object = existing.unwrap_or_else(|| {
    let object =
      unsafe { yuzuko_core::op::wrap_instance(scope, Clipboard::new()) };
    CLIPBOARD.with(|slot| {
      *slot.borrow_mut() = Some(v8::Global::new(scope, object));
    });
    object
  });
  js_object(scope, object)
}

pub fn dispatch_command(
  scope: &mut v8::PinScope,
  target: v8::Local<v8::Object>,
  type_name: &str,
) {
  let event = ClipboardEvent {
    base: Event::new(type_name, true, true, false, true),
    clipboard_data: RefCell::new(None),
  };
  let object = unsafe { yuzuko_core::op::wrap_instance(scope, event) };
  define_is_trusted(scope, object);
  let _ = yuzuko_dom::dispatch(scope, target, object);
}

fn require_gesture(scope: &mut v8::PinScope) -> Result<(), Js> {
  if activation::allowed() {
    Ok(())
  } else {
    Err(reject_dom(
      scope,
      "NotAllowedError",
      "A user action on the focused window is required",
    ))
  }
}

fn store_text(text: String) -> Result<(), String> {
  publish_text(&text)?;
  CONTENTS.with(|slot| {
    let mut contents = slot.borrow_mut();
    contents.text = Some(text);
    contents.png = None;
  });
  Ok(())
}

fn store_parts(
  text: Option<String>,
  png: Option<Vec<u8>>,
) -> Result<(), String> {
  if let Some(text) = &text {
    publish_text(text)?;
  }
  if let Some(png) = &png {
    publish_png(png)?;
  }
  CONTENTS.with(|slot| {
    *slot.borrow_mut() = Contents { text, png };
  });
  Ok(())
}

fn publish_text(text: &str) -> Result<(), String> {
  with_board(|board| board.set_text(text).map_err(|error| error.to_string()))
}

fn publish_png(bytes: &[u8]) -> Result<(), String> {
  let Some((width, height, rgba)) = decode_png(bytes) else {
    return Ok(());
  };
  with_board(|board| {
    board
      .set_image(arboard::ImageData {
        width,
        height,
        bytes: rgba.into(),
      })
      .map_err(|error| error.to_string())
  })
}

fn with_board(
  body: impl FnOnce(&mut arboard::Clipboard) -> Result<(), String>,
) -> Result<(), String> {
  BOARD.with(|slot| {
    let mut slot = slot.borrow_mut();
    if slot.is_none() {
      *slot =
        Some(arboard::Clipboard::new().map_err(|error| error.to_string())?);
    }
    body(slot.as_mut().expect("clipboard"))
  })
}

fn decode_png(bytes: &[u8]) -> Option<(usize, usize, Vec<u8>)> {
  let mut decoder = png::Decoder::new(Cursor::new(bytes));
  decoder.set_transformations(
    png::Transformations::EXPAND | png::Transformations::STRIP_16,
  );
  let mut reader = decoder.read_info().ok()?;
  let mut buffer = vec![0; reader.output_buffer_size()?];
  let frame = reader.next_frame(&mut buffer).ok()?;
  let (color, width, height) = {
    let info = reader.info();
    (info.color_type, info.width as usize, info.height as usize)
  };
  let pixels = &buffer[..frame.buffer_size()];
  let rgba = match color {
    png::ColorType::Rgba => pixels.to_vec(),
    png::ColorType::Rgb => {
      let mut rgba = Vec::with_capacity(pixels.len() / 3 * 4);
      for chunk in pixels.chunks(3) {
        rgba.extend_from_slice(chunk);
        rgba.push(255);
      }
      rgba
    }
    png::ColorType::Grayscale => {
      let mut rgba = Vec::with_capacity(pixels.len() * 4);
      for sample in pixels {
        rgba.extend_from_slice(&[*sample, *sample, *sample, 255]);
      }
      rgba
    }
    png::ColorType::GrayscaleAlpha => {
      let mut rgba = Vec::with_capacity(pixels.len() / 2 * 4);
      for chunk in pixels.chunks(2) {
        rgba.extend_from_slice(&[chunk[0], chunk[0], chunk[0], chunk[1]]);
      }
      rgba
    }
    _ => return None,
  };
  Some((width, height, rgba))
}

fn current_item() -> ClipboardItem {
  let (text, png) = CONTENTS.with(|slot| {
    let contents = slot.borrow();
    (contents.text.clone(), contents.png.clone())
  });
  let mut parts = Vec::new();
  if let Some(text) = text {
    parts.push((TEXT.to_string(), Part::Text(text)));
  }
  if let Some(png) = png {
    parts.push((PNG.to_string(), Part::Bytes(png)));
  }
  ClipboardItem {
    parts: RefCell::new(parts),
    style: "unspecified".to_string(),
  }
}

enum Part {
  Text(String),
  Bytes(Vec<u8>),
}

struct ClipboardItem {
  parts: RefCell<Vec<(String, Part)>>,
  style: String,
}

unsafe impl v8::cppgc::GarbageCollected for ClipboardItem {
  fn trace(&self, _visitor: &mut v8::cppgc::Visitor) {}

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"ClipboardItem"
  }
}

#[op]
impl ClipboardItem {
  #[constructor]
  #[length(1)]
  fn new(
    scope: &mut v8::PinScope,
    items: v8::Local<v8::Value>,
    options: Option<v8::Local<v8::Value>>,
  ) -> Result<Self, OpError> {
    let parts = read_item_record(scope, items)?;
    if parts.is_empty() {
      return Err(OpError::type_("ClipboardItem requires data"));
    }
    Ok(Self {
      parts: RefCell::new(parts),
      style: presentation_style(scope, options),
    })
  }

  #[getter]
  fn presentation_style(&self) -> String {
    self.style.clone()
  }

  #[getter]
  fn types(&self, scope: &mut v8::PinScope) -> Js {
    let names = self
      .parts
      .borrow()
      .iter()
      .map(|(name, _)| name.clone())
      .collect::<Vec<_>>();
    let array = v8::Array::new(scope, names.len() as i32);
    for (index, name) in names.iter().enumerate() {
      let text = v8::String::new(scope, name).unwrap();
      array.set_index(scope, index as u32, text.into());
    }
    array.set_integrity_level(scope, v8::IntegrityLevel::Frozen);
    js_value(scope, array.into())
  }

  #[promise]
  fn get_type(&self, scope: &mut v8::PinScope, type_name: String) -> Js {
    if !well_formed_clipboard_type(&type_name) {
      return reject_dom(scope, "TypeError", "The clipboard type is not valid");
    }
    let found = self
      .parts
      .borrow()
      .iter()
      .find(|(name, _)| name == &type_name)
      .map(|(_, part)| match part {
        Part::Text(text) => text.as_bytes().to_vec(),
        Part::Bytes(bytes) => bytes.clone(),
      });
    let Some(bytes) = found else {
      return reject_dom(scope, "NotFoundError", "The type is not available");
    };
    let blob = yuzuko_winter::new_blob(scope, bytes, &type_name);
    resolve(scope, blob.into())
  }

  #[static_method]
  fn supports(type_name: String) -> bool {
    type_name == TEXT || type_name == PNG
  }
}

#[repr(C)]
struct Clipboard {
  base: EventTarget,
}

impl Clipboard {
  fn new() -> Self {
    Self {
      base: EventTarget::new(),
    }
  }
}

unsafe impl v8::cppgc::GarbageCollected for Clipboard {
  fn trace(&self, visitor: &mut v8::cppgc::Visitor) {
    v8::cppgc::GarbageCollected::trace(&self.base, visitor);
  }

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"Clipboard"
  }
}

#[op(inherit = EventTarget)]
impl Clipboard {
  #[constructor]
  fn new() -> Result<Self, OpError> {
    Err(OpError::type_("Illegal constructor"))
  }

  #[promise]
  fn read_text(&self, scope: &mut v8::PinScope) -> Js {
    let _ = self;
    if let Err(error) = require_gesture(scope) {
      return error;
    }
    let text =
      CONTENTS.with(|slot| slot.borrow().text.clone().unwrap_or_default());
    let text = v8::String::new(scope, &text).unwrap();
    resolve(scope, text.into())
  }

  #[promise]
  fn write_text(&self, scope: &mut v8::PinScope, data: String) -> Js {
    let _ = self;
    if let Err(error) = require_gesture(scope) {
      return error;
    }
    if let Err(message) = store_text(data) {
      return reject_dom(scope, "NotAllowedError", &message);
    }
    resolve(scope, v8::undefined(scope).into())
  }

  #[promise]
  fn read(&self, scope: &mut v8::PinScope) -> Js {
    let _ = self;
    if let Err(error) = require_gesture(scope) {
      return error;
    }
    let item_value = current_item();
    let item = unsafe { yuzuko_core::op::wrap_instance(scope, item_value) };
    let array = v8::Array::new(scope, 1);
    array.set_index(scope, 0, item.into());
    resolve(scope, array.into())
  }

  #[promise]
  fn write(&self, scope: &mut v8::PinScope, data: v8::Local<v8::Value>) -> Js {
    let _ = self;
    if let Err(error) = require_gesture(scope) {
      return error;
    }
    let Ok(list) = v8::Local::<v8::Array>::try_from(data) else {
      return reject_dom(
        scope,
        "TypeError",
        "Clipboard.write requires a sequence",
      );
    };
    if list.length() == 0 {
      return reject_dom(scope, "TypeError", "Clipboard.write requires data");
    }
    let Some(value) = list.get_index(scope, 0) else {
      return reject_dom(scope, "TypeError", "Clipboard.write requires data");
    };
    let Ok(object) = v8::Local::<v8::Object>::try_from(value) else {
      return reject_dom(
        scope,
        "TypeError",
        "Clipboard.write requires a ClipboardItem",
      );
    };
    let Some(item) =
      (unsafe { yuzuko_core::op::host_ref::<ClipboardItem>(scope, object) })
    else {
      return reject_dom(
        scope,
        "TypeError",
        "Clipboard.write requires a ClipboardItem",
      );
    };
    let mut text = None;
    let mut png = None;
    for (name, part) in item.get().parts.borrow().iter() {
      match (name.as_str(), part) {
        (TEXT, Part::Text(value)) => text = Some(value.clone()),
        (TEXT, Part::Bytes(bytes)) => {
          text = Some(String::from_utf8_lossy(bytes).into_owned());
        }
        (PNG, Part::Bytes(bytes)) => png = Some(bytes.clone()),
        (PNG, Part::Text(value)) => png = Some(value.as_bytes().to_vec()),
        _ => {}
      }
    }
    if let Err(message) = store_parts(text, png) {
      return reject_dom(scope, "NotAllowedError", &message);
    }
    resolve(scope, v8::undefined(scope).into())
  }
}

#[repr(C)]
struct ClipboardEvent {
  base: Event,
  clipboard_data: RefCell<Option<v8::TracedReference<v8::Object>>>,
}

unsafe impl v8::cppgc::GarbageCollected for ClipboardEvent {
  fn trace(&self, visitor: &mut v8::cppgc::Visitor) {
    v8::cppgc::GarbageCollected::trace(&self.base, visitor);
    if let Ok(data) = self.clipboard_data.try_borrow() {
      visitor.trace(&*data);
    }
  }

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"ClipboardEvent"
  }
}

#[op(inherit = Event)]
impl ClipboardEvent {
  #[constructor]
  #[length(1)]
  fn new(
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<Self, OpError> {
    Ok(Self {
      base: Event::from_args(scope, args, false)?,
      clipboard_data: RefCell::new(None),
    })
  }

  #[construct_hook]
  fn finish(&self, scope: &mut v8::PinScope, object: v8::Local<v8::Object>) {
    let _ = self;
    define_is_trusted(scope, object);
  }

  #[getter]
  fn clipboard_data(&self, scope: &mut v8::PinScope) -> Js {
    let _ = self;
    js_null(scope)
  }
}

fn read_item_record(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<Vec<(String, Part)>, OpError> {
  let object = v8::Local::<v8::Object>::try_from(value)
    .map_err(|_| OpError::type_("ClipboardItem data must be an object"))?;
  let names = object
    .get_own_property_names(scope, v8::GetPropertyNamesArgs::default())
    .ok_or_else(|| OpError::type_("ClipboardItem data must be an object"))?;
  let mut parts = Vec::new();
  for index in 0..names.length() {
    let key = names
      .get_index(scope, index)
      .ok_or_else(|| OpError::type_("ClipboardItem data must be an object"))?;
    let name = key
      .to_string(scope)
      .map(|text| text.to_rust_string_lossy(scope))
      .unwrap_or_default();
    if !well_formed_clipboard_type(&name) {
      return Err(OpError::type_("The clipboard type is not valid"));
    }
    if name != TEXT && name != PNG {
      return Err(OpError::dom(
        "NotSupportedError",
        "The clipboard type is not supported",
      ));
    }
    let item = object
      .get(scope, key)
      .ok_or_else(|| OpError::type_("ClipboardItem data must be an object"))?;
    parts.push((name, read_part(scope, item)?));
  }
  Ok(parts)
}

fn read_part(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<Part, OpError> {
  if value.is_string() {
    let text = value
      .to_string(scope)
      .map(|text| text.to_rust_string_lossy(scope))
      .unwrap_or_default();
    return Ok(Part::Text(text));
  }
  match yuzuko_winter::snapshot_blob(scope, value) {
    Ok((bytes, _)) => Ok(Part::Bytes(bytes)),
    Err(_) => Err(OpError::type_(
      "ClipboardItem data must be a string or a Blob",
    )),
  }
}

fn presentation_style(
  scope: &mut v8::PinScope,
  options: Option<v8::Local<v8::Value>>,
) -> String {
  let Some(options) = options.filter(|value| value.is_object()) else {
    return "unspecified".to_string();
  };
  let Ok(object) = v8::Local::<v8::Object>::try_from(options) else {
    return "unspecified".to_string();
  };
  let key = v8::String::new(scope, "presentationStyle").unwrap();
  let Some(value) = object.get(scope, key.into()) else {
    return "unspecified".to_string();
  };
  let text = value
    .to_string(scope)
    .map(|text| text.to_rust_string_lossy(scope))
    .unwrap_or_default();
  match text.as_str() {
    "inline" | "attachment" | "unspecified" => text,
    _ => "unspecified".to_string(),
  }
}

fn reject_dom(scope: &mut v8::PinScope, name: &str, message: &str) -> Js {
  let resolver = v8::PromiseResolver::new(scope).unwrap();
  let promise = resolver.get_promise(scope);
  let error = if name == "TypeError" {
    let message = v8::String::new(scope, message).unwrap();
    v8::Exception::type_error(scope, message)
  } else {
    yuzuko_core::op::new_dom_exception(scope, name, message)
      .map(v8::Local::<v8::Value>::from)
      .unwrap_or_else(|| {
        let message = v8::String::new(scope, message).unwrap();
        v8::Exception::type_error(scope, message)
      })
  };
  resolver.reject(scope, error);
  let promise: v8::Local<v8::Value> = promise.into();
  Js(v8::Global::new(scope, promise))
}

fn resolve(scope: &mut v8::PinScope, value: v8::Local<v8::Value>) -> Js {
  let resolver = v8::PromiseResolver::new(scope).unwrap();
  let promise = resolver.get_promise(scope);
  resolver.resolve(scope, value);
  let promise: v8::Local<v8::Value> = promise.into();
  Js(v8::Global::new(scope, promise))
}

fn js_object(scope: &mut v8::PinScope, object: v8::Local<v8::Object>) -> Js {
  let value: v8::Local<v8::Value> = object.into();
  Js(v8::Global::new(scope, value))
}

fn js_value(scope: &mut v8::PinScope, value: v8::Local<v8::Value>) -> Js {
  Js(v8::Global::new(scope, value))
}

fn well_formed_clipboard_type(name: &str) -> bool {
  let mime = if let Some(rest) = name.strip_prefix("web ") {
    if rest.is_empty() {
      return false;
    }
    rest
  } else {
    name
  };
  yuzuko_winter::is_mime_type(mime)
}

fn js_null(scope: &mut v8::PinScope) -> Js {
  let value: v8::Local<v8::Value> = v8::null(scope).into();
  Js(v8::Global::new(scope, value))
}
