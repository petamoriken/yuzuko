//! `navigator.clipboard`, `ClipboardItem`, `ClipboardEvent`,
//! `ClipboardChangeEvent`, and the `DataTransfer` those events carry.
//!
//! Reads and writes require a recent user gesture on the focused window.
//! Text is published to the OS pasteboard. `image/png` bytes are kept so a
//! later `getType` returns the same PNG. A successful write fires
//! `clipboardchange` before its promise resolves. A cancelled host `copy` or
//! `cut` publishes `text/plain` the same way. `paste` is read-only. File
//! drag events use the same store. `setDragImage` is absent. A file added
//! during `copy` or `cut` is not written to the operating system.

use std::cell::{Cell, RefCell};

use yuzuko_core::op::Js;
use yuzuko_core::op::OpError;
use yuzuko_core::op::{traced, traced_local};
use yuzuko_dom::Event;
use yuzuko_dom::EventTarget;
use yuzuko_dom::define_is_trusted;
use yuzuko_macros::op;

use crate::activation;

const TEXT: &str = "text/plain";
const HTML: &str = "text/html";
const PNG: &str = "image/png";
const SVG: &str = "image/svg+xml";

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
  static LAST_SEEN: Cell<Option<u64>> = const { Cell::new(None) };
  static LINUX_SEQUENCE: Cell<u64> = const { Cell::new(0) };
  static PENDING: RefCell<Option<(Vec<String>, ChangeId)>> =
    const { RefCell::new(None) };
}

pub fn install(scope: &mut v8::PinScope) {
  Clipboard::install(scope);
  let constructor = Clipboard::template(scope)
    .get_function(scope)
    .expect("Clipboard");
  yuzuko_dom::define_prototype_event_handlers(
    scope,
    constructor,
    &["clipboardchange"],
  );
  ClipboardItem::install(scope);
  super::drag::prepare(scope);
  DataTransfer::install(scope);
  super::drag::install(scope);
  ClipboardEvent::install(scope);
  ClipboardChangeEvent::install(scope);
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
  let transfer = if type_name == "paste" {
    DataTransfer::from_paste()
  } else {
    DataTransfer::fresh(StoreMode::ReadWrite)
  };
  let transfer_obj = unsafe { yuzuko_core::op::wrap_instance(scope, transfer) };
  let event = ClipboardEvent {
    base: Event::new(type_name, true, true, false, true),
    clipboard_data: RefCell::new(Some(traced(scope, transfer_obj))),
  };
  let object = unsafe { yuzuko_core::op::wrap_instance(scope, event) };
  define_is_trusted(scope, object);
  // Script-constructed events never reach this path, so they cannot publish.
  let prevented = yuzuko_dom::dispatch(scope, target, object);
  if matches!(prevented, Ok(false))
    && (type_name == "copy" || type_name == "cut")
  {
    publish_cancelled_copy(scope, transfer_obj);
  }
}

/// Publish `text/plain` from a cancelled host `copy` or `cut`.
///
/// An empty string is present and replaces the pasteboard. A missing entry
/// leaves it unchanged. Other formats stay on the `DataTransfer`.
fn publish_cancelled_copy(
  scope: &mut v8::PinScope,
  transfer: v8::Local<v8::Object>,
) {
  let text = {
    let Some(host) =
      (unsafe { yuzuko_core::op::host_ref::<DataTransfer>(scope, transfer) })
    else {
      return;
    };
    host.get().plain_text()
  };
  let Some(text) = text else {
    return;
  };
  if store_text(text).is_ok() {
    announce(scope);
  }
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
  let Some((rgba, width, height)) = crate::png_rgba::decode(bytes) else {
    return Ok(());
  };
  with_board(|board| {
    board
      .set_image(arboard::ImageData {
        width: width as usize,
        height: height as usize,
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

fn current_item() -> ClipboardItem {
  let (text, png) = CONTENTS.with(|slot| {
    let contents = slot.borrow();
    (contents.text.clone(), contents.png.clone())
  });
  let mut parts = Vec::new();
  if let Some(text) = text {
    parts.push(plain_part(TEXT, Part::Text(text)));
  }
  if let Some(png) = png {
    parts.push(plain_part(
      PNG,
      Part::Bytes {
        bytes: png,
        type_: PNG.to_string(),
      },
    ));
  }
  ClipboardItem {
    parts: RefCell::new(parts),
    style: "unspecified".to_string(),
  }
}

fn plain_part(name: &str, part: Part) -> Representation {
  Representation {
    custom: false,
    serialized: name.to_string(),
    label: name.to_string(),
    part,
  }
}

enum Part {
  Text(String),
  Bytes { bytes: Vec<u8>, type_: String },
}

/// One constructor entry. `label` is the `types` string, including `web `.
struct Representation {
  custom: bool,
  serialized: String,
  label: String,
  part: Part,
}

struct ClipboardItem {
  parts: RefCell<Vec<Representation>>,
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
      .map(|part| part.label.clone())
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
    let Some(parsed) = clipboard_type(&type_name) else {
      return reject_dom(scope, "TypeError", "The clipboard type is not valid");
    };
    let found = self.parts.borrow().iter().find_map(|part| {
      if part.custom != parsed.custom || part.serialized != parsed.serialized {
        return None;
      }
      Some(match &part.part {
        Part::Text(text) => (text.as_bytes().to_vec(), part.label.clone()),
        Part::Bytes { bytes, type_ } => (bytes.clone(), type_.clone()),
      })
    });
    let Some((bytes, blob_type)) = found else {
      return reject_dom(scope, "NotFoundError", "The type is not available");
    };
    let blob = yuzuko_winter::new_script_blob(scope, bytes, &blob_type);
    resolve(scope, blob.into())
  }

  #[static_method]
  fn supports(type_name: String) -> bool {
    supports_clipboard_type(&type_name)
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
    announce(scope);
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
    for part in item.get().parts.borrow().iter() {
      match (part.label.as_str(), &part.part) {
        (TEXT, Part::Text(value)) => text = Some(value.clone()),
        (TEXT, Part::Bytes { bytes, .. }) => {
          text = Some(String::from_utf8_lossy(bytes).into_owned());
        }
        (PNG, Part::Bytes { bytes, .. }) => png = Some(bytes.clone()),
        (PNG, Part::Text(value)) => png = Some(value.as_bytes().to_vec()),
        _ => {}
      }
    }
    if text.is_none() && png.is_none() {
      return reject_dom(
        scope,
        "NotSupportedError",
        "The clipboard type is not supported",
      );
    }
    if let Err(message) = store_parts(text, png) {
      return reject_dom(scope, "NotAllowedError", &message);
    }
    announce(scope);
    resolve(scope, v8::undefined(scope).into())
  }
}

const URI_LIST: &str = "text/uri-list";

const DROP_EFFECTS: &[&str] = &["none", "copy", "link", "move"];

const ALLOWED_EFFECTS: &[&str] = &[
  "none",
  "copy",
  "copyLink",
  "copyMove",
  "link",
  "linkMove",
  "move",
  "all",
  "uninitialized",
];

/// Fixed for the life of one `DataTransfer`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum StoreMode {
  ReadWrite,
  ReadOnly,
  /// `dragenter`, `dragover`, and `dragleave`. Kinds and types are visible.
  /// Strings and file bytes are not.
  Protected,
}

enum ItemBody {
  Text(String),
  /// `None` while the store is protected, so the bytes are not created.
  File(Option<v8::TracedReference<v8::Object>>),
}

struct StoreItem {
  id: u64,
  type_: String,
  body: ItemBody,
}

/// Clipboard and drag data. The mode does not change.
///
/// Read/write accepts `setData`, `clearData`, and item changes. Read-only
/// and protected ignore them. `setDragImage` is not implemented.
pub(crate) struct DataTransfer {
  items: RefCell<Vec<StoreItem>>,
  mode: Cell<StoreMode>,
  drop_effect: RefCell<String>,
  effect_allowed: RefCell<String>,
  /// Reused until the item list changes. `setData` always replaces it.
  types: RefCell<Option<v8::TracedReference<v8::Object>>>,
  next_id: Cell<u64>,
  item_list: RefCell<Option<v8::TracedReference<v8::Object>>>,
  /// Kept after removal so a held item can report an empty kind and type.
  wrappers: RefCell<Vec<(u64, v8::TracedReference<v8::Object>)>>,
}

impl DataTransfer {
  fn fresh(mode: StoreMode) -> Self {
    Self {
      items: RefCell::new(Vec::new()),
      mode: Cell::new(mode),
      drop_effect: RefCell::new("none".to_string()),
      effect_allowed: RefCell::new("none".to_string()),
      types: RefCell::new(None),
      next_id: Cell::new(1),
      item_list: RefCell::new(None),
      wrappers: RefCell::new(Vec::new()),
    }
  }

  /// Read-only store seeded from the same text cache as `readText`.
  fn from_paste() -> Self {
    let transfer = Self::fresh(StoreMode::ReadOnly);
    if let Some(text) = CONTENTS.with(|slot| slot.borrow().text.clone()) {
      transfer.push_item(TEXT.to_string(), ItemBody::Text(text));
    }
    transfer
  }

  /// One file item per path. Protected items have no `File` yet.
  pub(crate) fn for_paths(
    scope: &mut v8::PinScope,
    paths: &[String],
    protected: bool,
  ) -> Self {
    let mode = if protected {
      StoreMode::Protected
    } else {
      StoreMode::ReadOnly
    };
    let transfer = Self::fresh(mode);
    *transfer.effect_allowed.borrow_mut() = "all".to_string();
    for path in paths {
      let path = std::path::Path::new(path);
      if protected {
        transfer.push_item(
          yuzuko_winter::path_media_type(path),
          ItemBody::File(None),
        );
      } else {
        let file = yuzuko_winter::file_from_path(scope, path);
        let type_ = unsafe {
          yuzuko_core::op::host_ref::<yuzuko_winter::File>(scope, file)
        }
        .map(|host| host.get().media_type())
        .unwrap_or_default();
        let reference = traced(scope, file);
        transfer.push_item(type_, ItemBody::File(Some(reference)));
      }
    }
    transfer
  }

  fn plain_text(&self) -> Option<String> {
    self
      .items
      .borrow()
      .iter()
      .find_map(|item| match &item.body {
        ItemBody::Text(text) if item.type_ == TEXT => Some(text.clone()),
        _ => None,
      })
  }

  pub(crate) fn writable(&self) -> bool {
    self.mode.get() == StoreMode::ReadWrite
  }

  fn protected(&self) -> bool {
    self.mode.get() == StoreMode::Protected
  }

  fn alloc_id(&self) -> u64 {
    let id = self.next_id.get();
    self.next_id.set(id.saturating_add(1));
    id
  }

  fn push_item(&self, type_: String, body: ItemBody) -> u64 {
    let id = self.alloc_id();
    self.items.borrow_mut().push(StoreItem { id, type_, body });
    id
  }

  fn invalidate_types(&self) {
    *self.types.borrow_mut() = None;
  }

  pub(crate) fn len(&self) -> usize {
    self.items.borrow().len()
  }

  pub(crate) fn id_at(&self, index: usize) -> Option<u64> {
    self.items.borrow().get(index).map(|item| item.id)
  }

  pub(crate) fn kind_of(&self, id: u64) -> String {
    let items = self.items.borrow();
    let Some(item) = items.iter().find(|item| item.id == id) else {
      return String::new();
    };
    match item.body {
      ItemBody::Text(_) => "string".to_string(),
      ItemBody::File(_) => "file".to_string(),
    }
  }

  pub(crate) fn type_of(&self, id: u64) -> String {
    self
      .items
      .borrow()
      .iter()
      .find(|item| item.id == id)
      .map(|item| item.type_.clone())
      .unwrap_or_default()
  }

  /// The captured string, or nothing when the item is not readable text.
  pub(crate) fn text_of(&self, id: u64) -> Option<String> {
    if self.protected() {
      return None;
    }
    self.items.borrow().iter().find_map(|item| {
      if item.id != id {
        return None;
      }
      match &item.body {
        ItemBody::Text(text) => Some(text.clone()),
        ItemBody::File(_) => None,
      }
    })
  }

  pub(crate) fn file_of<'s>(
    &self,
    scope: &mut v8::PinScope<'s, '_>,
    id: u64,
  ) -> Option<v8::Local<'s, v8::Object>> {
    if self.protected() {
      return None;
    }
    let items = self.items.borrow();
    let item = items.iter().find(|item| item.id == id)?;
    match &item.body {
      ItemBody::File(Some(reference)) => traced_local(scope, reference),
      _ => None,
    }
  }

  pub(crate) fn file_locals<'s>(
    &self,
    scope: &mut v8::PinScope<'s, '_>,
  ) -> Vec<v8::Local<'s, v8::Object>> {
    if self.protected() {
      return Vec::new();
    }
    self
      .items
      .borrow()
      .iter()
      .filter_map(|item| match &item.body {
        ItemBody::File(Some(reference)) => traced_local(scope, reference),
        _ => None,
      })
      .collect()
  }

  pub(crate) fn has_string_type(&self, type_: &str) -> bool {
    self
      .items
      .borrow()
      .iter()
      .any(|item| matches!(item.body, ItemBody::Text(_)) && item.type_ == type_)
  }

  pub(crate) fn append_text(&self, type_: String, data: String) -> u64 {
    let id = self.push_item(type_, ItemBody::Text(data));
    self.invalidate_types();
    id
  }

  pub(crate) fn append_file(
    &self,
    scope: &mut v8::PinScope,
    type_: String,
    file: v8::Local<'_, v8::Object>,
  ) -> u64 {
    let reference = traced(scope, file);
    let id = self.push_item(type_, ItemBody::File(Some(reference)));
    self.invalidate_types();
    id
  }

  /// `false` when `index` is past the end. That read does not change `types`.
  pub(crate) fn remove_at(&self, index: usize) -> bool {
    let mut items = self.items.borrow_mut();
    if index >= items.len() {
      return false;
    }
    items.remove(index);
    drop(items);
    self.invalidate_types();
    true
  }

  /// `false` when the list was already empty.
  pub(crate) fn clear_items(&self) -> bool {
    let mut items = self.items.borrow_mut();
    if items.is_empty() {
      return false;
    }
    items.clear();
    drop(items);
    self.invalidate_types();
    true
  }

  fn type_names(&self) -> Vec<String> {
    let items = self.items.borrow();
    let mut names = Vec::new();
    let mut files = false;
    for item in items.iter() {
      match &item.body {
        ItemBody::Text(_) => names.push(item.type_.clone()),
        ItemBody::File(_) => files = true,
      }
    }
    if files {
      names.push("Files".to_string());
    }
    names
  }

  pub(crate) fn item_list_local<'s>(
    &self,
    scope: &mut v8::PinScope<'s, '_>,
  ) -> Option<v8::Local<'s, v8::Object>> {
    self
      .item_list
      .borrow()
      .as_ref()
      .and_then(|reference| traced_local(scope, reference))
  }

  pub(crate) fn remember_item_list(
    &self,
    scope: &mut v8::PinScope,
    object: v8::Local<v8::Object>,
  ) {
    *self.item_list.borrow_mut() = Some(traced(scope, object));
  }

  pub(crate) fn wrapper_local<'s>(
    &self,
    scope: &mut v8::PinScope<'s, '_>,
    id: u64,
  ) -> Option<v8::Local<'s, v8::Object>> {
    let wrappers = self.wrappers.borrow();
    let (_, reference) = wrappers.iter().find(|(stored, _)| *stored == id)?;
    traced_local(scope, reference)
  }

  pub(crate) fn remember_wrapper(
    &self,
    scope: &mut v8::PinScope,
    id: u64,
    object: v8::Local<v8::Object>,
  ) {
    self.wrappers.borrow_mut().push((id, traced(scope, object)));
  }
}

unsafe impl v8::cppgc::GarbageCollected for DataTransfer {
  fn trace(&self, visitor: &mut v8::cppgc::Visitor) {
    if let Ok(types) = self.types.try_borrow() {
      visitor.trace(&*types);
    }
    if let Ok(items) = self.items.try_borrow() {
      for item in items.iter() {
        if let ItemBody::File(Some(reference)) = &item.body {
          visitor.trace(reference);
        }
      }
    }
    if let Ok(list) = self.item_list.try_borrow() {
      visitor.trace(&*list);
    }
    if let Ok(wrappers) = self.wrappers.try_borrow() {
      for (_, reference) in wrappers.iter() {
        visitor.trace(reference);
      }
    }
  }

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"DataTransfer"
  }
}

#[op]
impl DataTransfer {
  #[constructor]
  fn new() -> Self {
    Self::fresh(StoreMode::ReadWrite)
  }

  #[getter]
  fn drop_effect(&self) -> String {
    self.drop_effect.borrow().clone()
  }

  #[setter]
  fn drop_effect(&self, value: String) {
    if DROP_EFFECTS.contains(&value.as_str()) {
      *self.drop_effect.borrow_mut() = value;
    }
  }

  #[getter]
  fn effect_allowed(&self) -> String {
    self.effect_allowed.borrow().clone()
  }

  #[setter]
  fn effect_allowed(&self, value: String) {
    if !self.writable() {
      return;
    }
    if ALLOWED_EFFECTS.contains(&value.as_str()) {
      *self.effect_allowed.borrow_mut() = value;
    }
  }

  #[getter]
  fn items(
    &self,
    #[this] this: v8::Local<v8::Object>,
    scope: &mut v8::PinScope,
  ) -> Js {
    super::drag::items_object(scope, this, self)
  }

  #[getter]
  fn files(&self, scope: &mut v8::PinScope) -> Js {
    super::drag::files_object(scope, self)
  }

  #[getter]
  fn types(&self, scope: &mut v8::PinScope) -> Result<Js, OpError> {
    let cached = self
      .types
      .borrow()
      .as_ref()
      .and_then(|reference| traced_local(scope, reference));
    if let Some(object) = cached {
      return Ok(js_value(scope, object.into()));
    }
    let names = self.type_names();
    let array = v8::Array::new(scope, names.len() as i32);
    for (index, name) in names.iter().enumerate() {
      let Some(text) = v8::String::new(scope, name) else {
        return Err(OpError::type_("The data type could not be represented"));
      };
      array.set_index(scope, index as u32, text.into());
    }
    array.set_integrity_level(scope, v8::IntegrityLevel::Frozen);
    let object: v8::Local<v8::Object> = array.into();
    *self.types.borrow_mut() = Some(traced(scope, object));
    Ok(js_value(scope, object.into()))
  }

  fn get_data(&self, format: String) -> String {
    if self.protected() {
      return String::new();
    }
    let (format, convert_url) = canonical_format(&format);
    let items = self.items.borrow();
    let Some(data) = items.iter().find_map(|item| match &item.body {
      ItemBody::Text(data) if item.type_ == format => Some(data.clone()),
      _ => None,
    }) else {
      return String::new();
    };
    if convert_url { first_uri(&data) } else { data }
  }

  fn set_data(&self, format: String, data: String) {
    if !self.writable() {
      return;
    }
    let (format, _) = canonical_format(&format);
    {
      let mut items = self.items.borrow_mut();
      if let Some(index) = items.iter().position(|item| {
        matches!(item.body, ItemBody::Text(_)) && item.type_ == format
      }) {
        items.remove(index);
      }
    }
    self.push_item(format, ItemBody::Text(data));
    self.invalidate_types();
  }

  fn clear_data(&self, format: Option<String>) {
    if !self.writable() {
      return;
    }
    let mut items = self.items.borrow_mut();
    let before = items.len();
    match format {
      None => items.retain(|item| !matches!(item.body, ItemBody::Text(_))),
      Some(format) => {
        let (format, _) = canonical_format(&format);
        items.retain(|item| {
          !(matches!(item.body, ItemBody::Text(_)) && item.type_ == format)
        });
      }
    }
    let changed = items.len() != before;
    drop(items);
    if changed {
      self.invalidate_types();
    }
  }
}

/// ASCII-lowercase `format`, then map `text` and `url` to their MIME types.
///
/// `url` asks `getData` to return the first URI-list entry. `setData` and
/// `clearData` only use the mapped type.
pub(crate) fn canonical_format(format: &str) -> (String, bool) {
  let format = format.to_ascii_lowercase();
  match format.as_str() {
    "text" => (TEXT.to_string(), false),
    "url" => (URI_LIST.to_string(), true),
    _ => (format, false),
  }
}

/// First non-empty, non-comment line of a `text/uri-list` payload.
fn first_uri(list: &str) -> String {
  for line in list.split('\n') {
    let line = line.strip_suffix('\r').unwrap_or(line);
    if line.is_empty() || line.starts_with('#') {
      continue;
    }
    return line.to_string();
  }
  String::new()
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
      clipboard_data: RefCell::new(read_clipboard_data(scope, args)?),
    })
  }

  #[construct_hook]
  fn finish(&self, scope: &mut v8::PinScope, object: v8::Local<v8::Object>) {
    let _ = self;
    define_is_trusted(scope, object);
  }

  #[getter]
  fn clipboard_data(&self, scope: &mut v8::PinScope) -> Js {
    let value = self
      .clipboard_data
      .borrow()
      .as_ref()
      .and_then(|reference| traced_local(scope, reference))
      .map(v8::Local::<v8::Value>::from)
      .unwrap_or_else(|| v8::null(scope).into());
    js_value(scope, value)
  }
}

fn read_clipboard_data(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
) -> Result<Option<v8::TracedReference<v8::Object>>, OpError> {
  let Some(init) = event_init(args) else {
    return Ok(None);
  };
  let Ok(object) = v8::Local::<v8::Object>::try_from(init) else {
    return Ok(None);
  };
  let Some(value) = dict_field(scope, object, "clipboardData")? else {
    return Ok(None);
  };
  if value.is_null() {
    return Ok(None);
  }
  let Ok(data) = v8::Local::<v8::Object>::try_from(value) else {
    return Err(OpError::type_("clipboardData is not a DataTransfer"));
  };
  if unsafe { yuzuko_core::op::host_ref::<DataTransfer>(scope, data) }.is_none()
  {
    return Err(OpError::type_("clipboardData is not a DataTransfer"));
  }
  Ok(Some(traced(scope, data)))
}

/// The event init object, or nothing when the argument is missing or not an object.
fn event_init<'s>(
  args: &v8::FunctionCallbackArguments<'s>,
) -> Option<v8::Local<'s, v8::Value>> {
  if args.length() < 2 {
    return None;
  }
  let value = args.get(1);
  if value.is_null() || value.is_undefined() || !value.is_object() {
    None
  } else {
    Some(value)
  }
}

fn read_item_record(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<Vec<Representation>, OpError> {
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
    let name = yuzuko_core::rust_string_or(scope, key, "");
    let Some(parsed) = clipboard_type(&name) else {
      return Err(OpError::type_("The clipboard type is not valid"));
    };
    if parts.iter().any(|part: &Representation| {
      part.custom == parsed.custom && part.serialized == parsed.serialized
    }) {
      return Err(OpError::type_("The clipboard type is already present"));
    }
    let item = object
      .get(scope, key)
      .ok_or_else(|| OpError::type_("ClipboardItem data must be an object"))?;
    parts.push(Representation {
      custom: parsed.custom,
      serialized: parsed.serialized,
      label: parsed.label,
      part: read_part(scope, item)?,
    });
  }
  Ok(parts)
}

fn read_part(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<Part, OpError> {
  if value.is_string() {
    let text = yuzuko_core::rust_string_or(scope, value, "");
    return Ok(Part::Text(text));
  }
  match yuzuko_winter::snapshot_blob(scope, value) {
    Ok((bytes, type_)) => Ok(Part::Bytes { bytes, type_ }),
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
  let text = yuzuko_core::rust_string_or(scope, value, "");
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

struct ClipboardType {
  custom: bool,
  serialized: String,
  label: String,
}

/// A constructor key. `web ` marks a custom format. The MIME type may include
/// parameters. The label keeps that prefix because `Blob.type` for a string
/// value is the key, not the suffix.
fn clipboard_type(name: &str) -> Option<ClipboardType> {
  let (custom, body) = if let Some(rest) = name.strip_prefix("web ") {
    if rest.is_empty() {
      return None;
    }
    (true, rest)
  } else {
    (false, name)
  };
  let serialized = yuzuko_winter::serialized_mime(body)?;
  let label = if custom {
    format!("web {serialized}")
  } else {
    serialized.clone()
  };
  Some(ClipboardType {
    custom,
    serialized,
    label,
  })
}

/// Mandatory types, the optional types this runtime reports, and a `web `
/// custom format whose suffix is a MIME type without parameters.
fn supports_clipboard_type(name: &str) -> bool {
  if let Some(rest) = name.strip_prefix("web ") {
    return yuzuko_winter::serialized_mime(rest).is_some_and(|serialized| {
      serialized == rest && !serialized.contains(';')
    });
  }
  matches!(name, TEXT | HTML | PNG | URI_LIST | SVG)
}

#[derive(Clone, Copy)]
struct ChangeId {
  negative: bool,
  words: [u64; 2],
}

impl ChangeId {
  fn zero() -> Self {
    Self {
      negative: false,
      words: [0, 0],
    }
  }

  fn from_u64(value: u64) -> Self {
    Self {
      negative: false,
      words: [value, 0],
    }
  }
}

#[repr(C)]
struct ClipboardChangeEvent {
  base: Event,
  types: RefCell<Vec<String>>,
  change_id: ChangeId,
}

unsafe impl v8::cppgc::GarbageCollected for ClipboardChangeEvent {
  fn trace(&self, visitor: &mut v8::cppgc::Visitor) {
    v8::cppgc::GarbageCollected::trace(&self.base, visitor);
  }

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"ClipboardChangeEvent"
  }
}

#[op(inherit = Event)]
impl ClipboardChangeEvent {
  #[constructor]
  #[length(1)]
  fn new(
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<Self, OpError> {
    let base = Event::from_args(scope, args, false)?;
    let init = if args.length() > 1 {
      Some(args.get(1))
    } else {
      None
    };
    let (types, change_id) = parse_change_init(scope, init)?;
    Ok(Self {
      base,
      types: RefCell::new(types),
      change_id,
    })
  }

  #[construct_hook]
  fn finish(&self, scope: &mut v8::PinScope, object: v8::Local<v8::Object>) {
    let _ = self;
    define_is_trusted(scope, object);
  }

  #[getter]
  fn types(&self, scope: &mut v8::PinScope) -> Result<Js, OpError> {
    let names = self.types.borrow().clone();
    frozen_strings(scope, &names)
  }

  #[getter]
  fn change_id(&self, scope: &mut v8::PinScope) -> Js {
    let id = change_bigint(scope, self.change_id);
    js_value(scope, id.into())
  }
}

/// Records one external change while no window is focused.
pub fn deliver_pending(scope: &mut v8::PinScope) {
  let pending = PENDING.with(|slot| slot.borrow_mut().take());
  let Some((types, change_id)) = pending else {
    return;
  };
  dispatch_change(scope, &types, change_id);
}

/// The first observation records the sequence and does not fire.
pub fn poll(scope: &mut v8::PinScope) {
  let Some(sequence) = platform_sequence() else {
    return;
  };
  let previous = LAST_SEEN.with(|cell| cell.get());
  if previous.is_none() {
    LAST_SEEN.with(|cell| cell.set(Some(sequence)));
    return;
  }
  if previous == Some(sequence) {
    return;
  }
  LAST_SEEN.with(|cell| cell.set(Some(sequence)));
  let types = platform_mime_types();
  let change_id = ChangeId::from_u64(sequence);
  if activation::focused_id().is_some() {
    dispatch_change(scope, &types, change_id);
  } else {
    PENDING.with(|slot| {
      *slot.borrow_mut() = Some((types, change_id));
    });
  }
}

fn announce(scope: &mut v8::PinScope) {
  let types = content_types();
  let change_id = take_change_id();
  if activation::focused_id().is_some() {
    dispatch_change(scope, &types, change_id);
  } else {
    PENDING.with(|slot| {
      *slot.borrow_mut() = Some((types, change_id));
    });
  }
}

fn content_types() -> Vec<String> {
  CONTENTS.with(|slot| {
    let contents = slot.borrow();
    let mut types = Vec::new();
    if contents.text.is_some() {
      types.push(TEXT.to_string());
    }
    if contents.png.is_some() {
      types.push(PNG.to_string());
    }
    types
  })
}

fn take_change_id() -> ChangeId {
  if let Some(sequence) = platform_sequence() {
    LAST_SEEN.with(|cell| cell.set(Some(sequence)));
    return ChangeId::from_u64(sequence);
  }
  // Linux has no sequence that matches the pasteboard, so other
  // applications are not observed. Writes in this process count from 1.
  LINUX_SEQUENCE.with(|cell| {
    let next = cell.get().saturating_add(1).max(1);
    cell.set(next);
    ChangeId::from_u64(next)
  })
}

fn dispatch_change(
  scope: &mut v8::PinScope,
  types: &[String],
  change_id: ChangeId,
) {
  let Some(target) = CLIPBOARD.with(|slot| {
    slot
      .borrow()
      .as_ref()
      .map(|value| v8::Local::new(scope, value))
  }) else {
    return;
  };
  let event = ClipboardChangeEvent {
    base: Event::new("clipboardchange", false, false, false, true),
    types: RefCell::new(types.to_vec()),
    change_id,
  };
  let object = unsafe { yuzuko_core::op::wrap_instance(scope, event) };
  define_is_trusted(scope, object);
  let _ = yuzuko_dom::dispatch(scope, target, object);
}

fn parse_change_init(
  scope: &mut v8::PinScope,
  init: Option<v8::Local<v8::Value>>,
) -> Result<(Vec<String>, ChangeId), OpError> {
  let Some(init) = init.filter(|value| {
    !value.is_null() && !value.is_undefined() && !value.is_string()
  }) else {
    return Ok((Vec::new(), ChangeId::zero()));
  };
  if !init.is_object() {
    return Ok((Vec::new(), ChangeId::zero()));
  }
  let Ok(object) = v8::Local::<v8::Object>::try_from(init) else {
    return Ok((Vec::new(), ChangeId::zero()));
  };
  let types = match dict_field(scope, object, "types")? {
    Some(value) => read_type_list(scope, value)?,
    None => Vec::new(),
  };
  let change_id = match dict_field(scope, object, "changeId")? {
    Some(value) => read_change_id(scope, value)?,
    None => ChangeId::zero(),
  };
  Ok((types, change_id))
}

fn dict_field<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  object: v8::Local<'_, v8::Object>,
  name: &str,
) -> Result<Option<v8::Local<'s, v8::Value>>, OpError> {
  let Some(key) = v8::String::new(scope, name) else {
    return Err(OpError::type_(
      "ClipboardChangeEvent types must be a sequence.",
    ));
  };
  let captured = {
    v8::tc_scope!(let tc, scope);
    let object = yuzuko_core::op::root(tc, object);
    match object.get(tc, key.into()) {
      Some(value) => Ok(v8::Global::new(tc, value)),
      None => {
        let exception =
          tc.exception().unwrap_or_else(|| v8::undefined(tc).into());
        Err(v8::Global::new(tc, exception))
      }
    }
  };
  match captured {
    Ok(global) => {
      let value = v8::Local::new(scope, global);
      if value.is_undefined() {
        Ok(None)
      } else {
        Ok(Some(value))
      }
    }
    Err(exception) => {
      let exception = v8::Local::new(scope, exception);
      Err(OpError::thrown(scope, exception))
    }
  }
}

fn read_type_list(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<Vec<String>, OpError> {
  let Ok(list) = v8::Local::<v8::Array>::try_from(value) else {
    return Err(OpError::type_(
      "ClipboardChangeEvent types must be a sequence.",
    ));
  };
  let mut types = Vec::new();
  for index in 0..list.length() {
    let item = array_item(scope, list, index)?;
    types.push(crate::shell::dialog::js_string(scope, item)?);
  }
  Ok(types)
}

fn array_item<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  list: v8::Local<'_, v8::Array>,
  index: u32,
) -> Result<v8::Local<'s, v8::Value>, OpError> {
  let captured = {
    v8::tc_scope!(let tc, scope);
    let list = yuzuko_core::op::root(tc, list);
    match list.get_index(tc, index) {
      Some(value) => Ok(v8::Global::new(tc, value)),
      None => {
        if let Some(exception) = tc.exception() {
          Err(v8::Global::new(tc, exception))
        } else {
          let missing: v8::Local<v8::Value> = v8::undefined(tc).into();
          Ok(v8::Global::new(tc, missing))
        }
      }
    }
  };
  match captured {
    Ok(global) => Ok(v8::Local::new(scope, global)),
    Err(exception) => {
      let exception = v8::Local::new(scope, exception);
      Err(OpError::thrown(scope, exception))
    }
  }
}

fn read_change_id(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<ChangeId, OpError> {
  let captured = {
    v8::tc_scope!(let tc, scope);
    match value.to_big_int(tc) {
      Some(bigint) => Ok(v8::Global::new(tc, bigint)),
      None => {
        let exception =
          tc.exception().unwrap_or_else(|| v8::undefined(tc).into());
        Err(v8::Global::new(tc, exception))
      }
    }
  };
  let bigint = match captured {
    Ok(global) => v8::Local::new(scope, global),
    Err(exception) => {
      let exception = v8::Local::new(scope, exception);
      return Err(OpError::thrown(scope, exception));
    }
  };
  if bigint.word_count() > 2 {
    return Err(OpError::type_(
      "ClipboardChangeEvent changeId is out of range.",
    ));
  }
  let mut words = [0u64; 2];
  let (negative, _) = bigint.to_words_array(&mut words);
  Ok(ChangeId { negative, words })
}

fn change_bigint<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  id: ChangeId,
) -> v8::Local<'s, v8::BigInt> {
  if id.words == [0, 0] {
    return v8::BigInt::new_from_i64(scope, 0);
  }
  let count = if id.words[1] == 0 { 1 } else { 2 };
  v8::BigInt::new_from_words(scope, id.negative, &id.words[..count])
    .unwrap_or_else(|| v8::BigInt::new_from_i64(scope, 0))
}

fn frozen_strings(
  scope: &mut v8::PinScope,
  names: &[String],
) -> Result<Js, OpError> {
  let array = v8::Array::new(scope, names.len() as i32);
  for (index, name) in names.iter().enumerate() {
    let Some(text) = v8::String::new(scope, name) else {
      return Err(OpError::type_(
        "ClipboardChangeEvent types must be a sequence.",
      ));
    };
    array.set_index(scope, index as u32, text.into());
  }
  array.set_integrity_level(scope, v8::IntegrityLevel::Frozen);
  Ok(js_value(scope, array.into()))
}

fn platform_sequence() -> Option<u64> {
  #[cfg(target_os = "macos")]
  {
    Some(macos_change_count())
  }
  #[cfg(target_os = "windows")]
  {
    Some(windows_sequence())
  }
  #[cfg(not(any(target_os = "macos", target_os = "windows")))]
  {
    None
  }
}

fn platform_mime_types() -> Vec<String> {
  #[cfg(target_os = "macos")]
  {
    macos_mime_types()
  }
  #[cfg(target_os = "windows")]
  {
    windows_mime_types()
  }
  #[cfg(not(any(target_os = "macos", target_os = "windows")))]
  {
    Vec::new()
  }
}

#[cfg(target_os = "macos")]
fn macos_change_count() -> u64 {
  let board = objc2_app_kit::NSPasteboard::generalPasteboard();
  u64::try_from(board.changeCount()).unwrap_or(0)
}

#[cfg(target_os = "macos")]
fn macos_mime_types() -> Vec<String> {
  use objc2_app_kit::{
    NSPasteboardTypeHTML, NSPasteboardTypePNG, NSPasteboardTypeString,
  };
  let board = objc2_app_kit::NSPasteboard::generalPasteboard();
  let Some(types) = board.types() else {
    return Vec::new();
  };
  let mut plain = false;
  let mut html = false;
  let mut png = false;
  for index in 0..types.count() {
    let kind = types.objectAtIndex(index);
    unsafe {
      if kind.isEqualToString(NSPasteboardTypeString) {
        plain = true;
      }
      if kind.isEqualToString(NSPasteboardTypeHTML) {
        html = true;
      }
      if kind.isEqualToString(NSPasteboardTypePNG) {
        png = true;
      }
    }
  }
  let mut names = Vec::new();
  if plain {
    names.push(TEXT.to_string());
  }
  if html {
    names.push(HTML.to_string());
  }
  if png {
    names.push(PNG.to_string());
  }
  names
}

#[cfg(target_os = "windows")]
fn windows_sequence() -> u64 {
  use windows_sys::Win32::System::DataExchange::GetClipboardSequenceNumber;
  unsafe { u64::from(GetClipboardSequenceNumber()) }
}

#[cfg(target_os = "windows")]
fn windows_mime_types() -> Vec<String> {
  const CF_UNICODETEXT: u32 = 13;
  const CF_DIB: u32 = 8;
  let mut names = Vec::new();
  if clipboard_format_available(CF_UNICODETEXT) {
    names.push(TEXT.to_string());
  }
  if clipboard_format_available(html_format()) {
    names.push(HTML.to_string());
  }
  if clipboard_format_available(png_format())
    || clipboard_format_available(CF_DIB)
  {
    names.push(PNG.to_string());
  }
  names
}

#[cfg(target_os = "windows")]
fn clipboard_format_available(format: u32) -> bool {
  use windows_sys::Win32::System::DataExchange::IsClipboardFormatAvailable;
  format != 0 && unsafe { IsClipboardFormatAvailable(format) } != 0
}

#[cfg(target_os = "windows")]
fn png_format() -> u32 {
  use std::sync::OnceLock;
  static ATOM: OnceLock<u32> = OnceLock::new();
  *ATOM.get_or_init(|| register_clipboard_format("PNG"))
}

#[cfg(target_os = "windows")]
fn html_format() -> u32 {
  use std::sync::OnceLock;
  static ATOM: OnceLock<u32> = OnceLock::new();
  *ATOM.get_or_init(|| register_clipboard_format("HTML Format"))
}

#[cfg(target_os = "windows")]
fn register_clipboard_format(name: &str) -> u32 {
  use windows_sys::Win32::System::DataExchange::RegisterClipboardFormatW;
  let mut units: Vec<u16> = name.encode_utf16().collect();
  units.push(0);
  unsafe { RegisterClipboardFormatW(units.as_ptr()) }
}
