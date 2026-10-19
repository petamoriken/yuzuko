//! `FontFace` and `Yuzuko.fonts`.
//!
//! There is no `document`, so the set is not `document.fonts` and not
//! `globalThis.fonts`. The worker phase installs `self.fonts`.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::io::Read;

use flate2::read::ZlibDecoder;
use yuzuko_core::op::{
  Js, OpError, host_ref, new_dom_exception, traced, traced_local,
};
use yuzuko_dom::Event;
use yuzuko_macros::op;

use crate::text;

static NEXT_JOB: std::sync::atomic::AtomicU64 =
  std::sync::atomic::AtomicU64::new(1);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Status {
  Unloaded,
  Loading,
  Loaded,
  Error,
}

enum Source {
  Bytes(Vec<u8>),
  Css(String),
}

struct Descriptors {
  style: String,
  weight: String,
  width: String,
  unicode_range: String,
  feature_settings: String,
  display: String,
  ascent_override: String,
  descent_override: String,
  line_gap_override: String,
  size_adjust: String,
  variation_settings: String,
}

impl Default for Descriptors {
  fn default() -> Self {
    Self {
      style: "normal".to_string(),
      weight: "normal".to_string(),
      width: "normal".to_string(),
      unicode_range: "U+0-10FFFF".to_string(),
      feature_settings: "normal".to_string(),
      display: "auto".to_string(),
      ascent_override: "normal".to_string(),
      descent_override: "normal".to_string(),
      line_gap_override: "normal".to_string(),
      size_adjust: "100%".to_string(),
      variation_settings: "normal".to_string(),
    }
  }
}

struct FontFace {
  family_raw: String,
  family_css: String,
  source: Source,
  status: Cell<Status>,
  sfnt: RefCell<Option<Vec<u8>>>,
  descriptors: RefCell<Descriptors>,
  object: RefCell<Option<v8::TracedReference<v8::Object>>>,
  resolver: RefCell<Option<v8::TracedReference<v8::PromiseResolver>>>,
  promise: RefCell<Option<v8::TracedReference<v8::Promise>>>,
}

unsafe impl v8::cppgc::GarbageCollected for FontFace {
  fn trace(&self, visitor: &mut v8::cppgc::Visitor) {
    if let Ok(object) = self.object.try_borrow() {
      visitor.trace(&*object);
    }
    if let Ok(resolver) = self.resolver.try_borrow() {
      visitor.trace(&*resolver);
    }
    if let Ok(promise) = self.promise.try_borrow() {
      visitor.trace(&*promise);
    }
  }

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"FontFace"
  }
}

#[op]
impl FontFace {
  #[constructor]
  #[length(2)]
  fn new(
    scope: &mut v8::PinScope,
    family: String,
    source: v8::Local<v8::Value>,
    descriptors: Option<v8::Local<v8::Value>>,
  ) -> Result<Self, OpError> {
    let source = read_source(scope, source)?;
    let descriptors = read_descriptors(scope, descriptors)?;
    Ok(Self {
      family_css: serialize_family(&family),
      family_raw: family,
      source,
      status: Cell::new(Status::Unloaded),
      sfnt: RefCell::new(None),
      descriptors: RefCell::new(descriptors),
      object: RefCell::new(None),
      resolver: RefCell::new(None),
      promise: RefCell::new(None),
    })
  }

  #[construct_hook]
  fn finish(&self, scope: &mut v8::PinScope, object: v8::Local<v8::Object>) {
    *self.object.borrow_mut() = Some(traced(scope, object));
  }

  #[getter]
  fn family(&self) -> String {
    self.family_css.clone()
  }

  #[getter]
  fn style(&self) -> String {
    self.descriptors.borrow().style.clone()
  }

  #[getter]
  fn weight(&self) -> String {
    self.descriptors.borrow().weight.clone()
  }

  #[getter]
  fn stretch(&self) -> String {
    self.descriptors.borrow().width.clone()
  }

  #[setter]
  fn stretch(&self, value: String) -> Result<(), OpError> {
    self.set_width(value)
  }

  #[getter]
  fn width(&self) -> String {
    self.descriptors.borrow().width.clone()
  }

  #[setter]
  fn width(&self, value: String) -> Result<(), OpError> {
    self.set_width(value)
  }

  #[getter]
  fn unicode_range(&self) -> String {
    self.descriptors.borrow().unicode_range.clone()
  }

  #[getter]
  fn feature_settings(&self) -> String {
    self.descriptors.borrow().feature_settings.clone()
  }

  #[getter]
  fn display(&self) -> String {
    self.descriptors.borrow().display.clone()
  }

  #[getter]
  fn ascent_override(&self) -> String {
    self.descriptors.borrow().ascent_override.clone()
  }

  #[getter]
  fn descent_override(&self) -> String {
    self.descriptors.borrow().descent_override.clone()
  }

  #[getter]
  fn line_gap_override(&self) -> String {
    self.descriptors.borrow().line_gap_override.clone()
  }

  #[getter]
  fn size_adjust(&self) -> String {
    self.descriptors.borrow().size_adjust.clone()
  }

  #[getter]
  fn variation_settings(&self) -> String {
    self.descriptors.borrow().variation_settings.clone()
  }

  #[getter]
  fn status(&self) -> String {
    match self.status.get() {
      Status::Unloaded => "unloaded",
      Status::Loading => "loading",
      Status::Loaded => "loaded",
      Status::Error => "error",
    }
    .to_string()
  }

  #[getter]
  fn loaded(&self, scope: &mut v8::PinScope) -> Js {
    self.ensure_promise(scope)
  }

  #[promise]
  fn load(&self, scope: &mut v8::PinScope) -> Js {
    let promise = self.ensure_promise(scope);
    if self.status.get() == Status::Loaded
      || self.status.get() == Status::Loading
    {
      return promise;
    }
    self.status.set(Status::Loading);
    if let Err(error) = self.start(scope) {
      self.fail(scope, &error.0, &error.1);
    }
    self.ensure_promise(scope)
  }
}

impl FontFace {
  fn set_width(&self, value: String) -> Result<(), OpError> {
    if !valid_width(&value) {
      return Err(OpError::dom("SyntaxError", "The font width is invalid"));
    }
    self.descriptors.borrow_mut().width = value;
    Ok(())
  }

  fn ensure_promise(&self, scope: &mut v8::PinScope) -> Js {
    if let Some(promise) = self.promise.borrow().as_ref()
      && let Some(promise) = traced_local(scope, promise)
    {
      let value: v8::Local<v8::Value> = promise.into();
      return Js(v8::Global::new(scope, value));
    }
    let resolver = v8::PromiseResolver::new(scope).unwrap();
    let promise = resolver.get_promise(scope);
    *self.resolver.borrow_mut() = Some(traced(scope, resolver));
    *self.promise.borrow_mut() = Some(traced(scope, promise));
    if self.status.get() == Status::Loaded {
      self.resolve(scope);
    }
    let value: v8::Local<v8::Value> = promise.into();
    Js(v8::Global::new(scope, value))
  }

  fn start(&self, scope: &mut v8::PinScope) -> Result<(), (String, String)> {
    match &self.source {
      Source::Bytes(bytes) => {
        self.finish_bytes(scope, bytes.clone());
        Ok(())
      }
      Source::Css(css) => {
        let Some(url) = source_url(css) else {
          return Err((
            "NetworkError".to_string(),
            "The font source is not a URL".to_string(),
          ));
        };
        let Some(object) = self.face_object(scope) else {
          return Err((
            "NetworkError".to_string(),
            "The font could not be loaded".to_string(),
          ));
        };
        start_fetch(scope, object, &url);
        Ok(())
      }
    }
  }

  fn finish_bytes(&self, scope: &mut v8::PinScope, bytes: Vec<u8>) {
    let sfnt = match decode_sfnt(&bytes) {
      Ok(sfnt) if text::font_is_usable(&self.family_raw, &sfnt) => sfnt,
      _ => {
        self.fail(scope, "SyntaxError", "The font data is invalid");
        return;
      }
    };
    *self.sfnt.borrow_mut() = Some(sfnt);
    self.status.set(Status::Loaded);
    self.resolve(scope);
    sync_set(scope);
  }

  fn resolve(&self, scope: &mut v8::PinScope) {
    let Some(resolver) = self.resolver.borrow_mut().take() else {
      return;
    };
    let Some(resolver) = traced_local(scope, &resolver) else {
      return;
    };
    let value = self
      .face_object(scope)
      .map(v8::Local::<v8::Value>::from)
      .unwrap_or_else(|| v8::undefined(scope).into());
    resolver.resolve(scope, value);
  }

  fn fail(&self, scope: &mut v8::PinScope, name: &str, message: &str) {
    self.status.set(Status::Error);
    let Some(resolver) = self.resolver.borrow_mut().take() else {
      return;
    };
    let Some(resolver) = traced_local(scope, &resolver) else {
      return;
    };
    let error = new_dom_exception(scope, name, message)
      .map(v8::Local::<v8::Value>::from)
      .unwrap_or_else(|| {
        let message = v8::String::new(scope, message).unwrap();
        v8::Exception::type_error(scope, message)
      });
    resolver.reject(scope, error);
  }

  fn face_object<'s>(
    &self,
    scope: &mut v8::PinScope<'s, '_>,
  ) -> Option<v8::Local<'s, v8::Object>> {
    self
      .object
      .borrow()
      .as_ref()
      .and_then(|object| traced_local(scope, object))
  }
}

struct FontFaceSet;

unsafe impl v8::cppgc::GarbageCollected for FontFaceSet {
  fn trace(&self, _visitor: &mut v8::cppgc::Visitor) {}

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"FontFaceSet"
  }
}

#[op]
impl FontFaceSet {
  #[constructor]
  fn new() -> Result<Self, OpError> {
    Err(OpError::type_("Illegal constructor"))
  }

  fn add(
    &self,
    scope: &mut v8::PinScope,
    value: v8::Local<v8::Value>,
  ) -> Result<(), OpError> {
    let _ = self;
    let object = font_face_object(scope, value)?;
    SET.with(|slot| {
      let mut set = slot.borrow_mut();
      if !set
        .iter()
        .any(|item| v8::Local::new(scope, item).strict_equals(object.into()))
      {
        set.push(v8::Global::new(scope, object));
      }
    });
    sync_set(scope);
    Ok(())
  }

  fn delete(
    &self,
    scope: &mut v8::PinScope,
    value: v8::Local<v8::Value>,
  ) -> bool {
    let _ = self;
    let Ok(object) = v8::Local::<v8::Object>::try_from(value) else {
      return false;
    };
    let removed = SET.with(|slot| {
      let mut set = slot.borrow_mut();
      let before = set.len();
      set.retain(|item| {
        !v8::Local::new(scope, item).strict_equals(object.into())
      });
      set.len() != before
    });
    if removed {
      sync_set(scope);
    }
    removed
  }

  fn has(&self, scope: &mut v8::PinScope, value: v8::Local<v8::Value>) -> bool {
    let _ = self;
    let Ok(object) = v8::Local::<v8::Object>::try_from(value) else {
      return false;
    };
    SET.with(|slot| {
      slot
        .borrow()
        .iter()
        .any(|item| v8::Local::new(scope, item).strict_equals(object.into()))
    })
  }

  fn clear(&self, scope: &mut v8::PinScope) {
    let _ = self;
    SET.with(|slot| slot.borrow_mut().clear());
    sync_set(scope);
  }

  #[getter]
  fn size(&self) -> u32 {
    let _ = self;
    SET.with(|slot| slot.borrow().len() as u32)
  }

  #[getter]
  fn status(&self, scope: &mut v8::PinScope) -> String {
    let _ = self;
    if any_loading(scope) {
      "loading".to_string()
    } else {
      "loaded".to_string()
    }
  }
}

#[repr(C)]
struct FontFaceSetLoadEvent {
  base: Event,
  faces: RefCell<Vec<v8::TracedReference<v8::Value>>>,
}

unsafe impl v8::cppgc::GarbageCollected for FontFaceSetLoadEvent {
  fn trace(&self, visitor: &mut v8::cppgc::Visitor) {
    v8::cppgc::GarbageCollected::trace(&self.base, visitor);
    if let Ok(faces) = self.faces.try_borrow() {
      for face in &*faces {
        visitor.trace(face);
      }
    }
  }

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"FontFaceSetLoadEvent"
  }
}

#[op(inherit = Event)]
impl FontFaceSetLoadEvent {
  #[constructor]
  #[length(1)]
  fn new(
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<Self, OpError> {
    Ok(Self {
      base: Event::from_args(scope, args, false)?,
      faces: RefCell::new(read_face_list(scope, args)?),
    })
  }

  #[getter]
  fn fontfaces(&self, scope: &mut v8::PinScope) -> Js {
    let faces = self.faces.borrow();
    let array = v8::Array::new(scope, faces.len() as i32);
    for (index, face) in faces.iter().enumerate() {
      if let Some(value) = traced_local(scope, face) {
        array.set_index(scope, index as u32, value);
      }
    }
    let value: v8::Local<v8::Value> = array.into();
    Js(v8::Global::new(scope, value))
  }
}

struct Job {
  face: v8::Global<v8::Object>,
}

std::thread_local! {
  static SET: RefCell<Vec<v8::Global<v8::Object>>> =
    const { RefCell::new(Vec::new()) };
  static JOBS: RefCell<HashMap<u64, Job>> = RefCell::new(HashMap::new());
}

pub fn install(scope: &mut v8::PinScope) {
  FontFace::install(scope);
  FontFaceSet::install(scope);
  FontFaceSetLoadEvent::install(scope);
  let set = unsafe { yuzuko_core::op::wrap_instance(scope, FontFaceSet) };
  let key = v8::String::new(scope, "fonts").unwrap();
  if yuzuko_core::is_dedicated_worker() {
    let global = scope.get_current_context().global(scope);
    global.set(scope, key.into(), set.into());
    return;
  }
  let yuzuko = yuzuko_core::namespace(scope, "Yuzuko");
  yuzuko.set(scope, key.into(), set.into());
}

pub fn clear() {
  SET.with(|slot| slot.borrow_mut().clear());
  JOBS.with(|slot| slot.borrow_mut().clear());
  text::set_loaded_faces(&[]);
}

fn sync_set(scope: &mut v8::PinScope) {
  let globals = SET.with(|slot| slot.borrow().clone());
  let mut faces = Vec::new();
  for global in globals {
    let object = v8::Local::new(scope, &global);
    let Some(face) = (unsafe { host_ref::<FontFace>(scope, object) }) else {
      continue;
    };
    let face = face.get();
    if face.status.get() != Status::Loaded {
      continue;
    }
    let Some(bytes) = face.sfnt.borrow().clone() else {
      continue;
    };
    faces.push((face.family_raw.clone(), bytes));
  }
  text::set_loaded_faces(&faces);
}

fn any_loading(scope: &mut v8::PinScope) -> bool {
  let globals = SET.with(|slot| slot.borrow().clone());
  globals.iter().any(|global| {
    let object = v8::Local::new(scope, global);
    unsafe { host_ref::<FontFace>(scope, object) }
      .is_some_and(|face| face.get().status.get() == Status::Loading)
  })
}

fn font_face_object<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  value: v8::Local<'_, v8::Value>,
) -> Result<v8::Local<'s, v8::Object>, OpError> {
  let object = v8::Local::<v8::Object>::try_from(value)
    .map_err(|_| OpError::type_("FontFaceSet.add requires a FontFace"))?;
  if unsafe { host_ref::<FontFace>(scope, object) }.is_none() {
    return Err(OpError::type_("FontFaceSet.add requires a FontFace"));
  }
  Ok(yuzuko_core::op::root(scope, object))
}

fn read_source(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<Source, OpError> {
  if let Some(bytes) = yuzuko_winter::buffer_source(scope, value) {
    return Ok(Source::Bytes(bytes));
  }
  if value.is_string() {
    let text = value
      .to_string(scope)
      .map(|text| text.to_rust_string_lossy(scope))
      .unwrap_or_default();
    return Ok(Source::Css(text));
  }
  Err(OpError::type_(
    "FontFace source must be a BinaryData or a string",
  ))
}

fn read_descriptors(
  scope: &mut v8::PinScope,
  value: Option<v8::Local<v8::Value>>,
) -> Result<Descriptors, OpError> {
  let mut descriptors = Descriptors::default();
  let Some(value) = value.filter(|value| value.is_object()) else {
    return Ok(descriptors);
  };
  let Ok(object) = v8::Local::<v8::Object>::try_from(value) else {
    return Ok(descriptors);
  };
  if let Some(style) = own_string(scope, object, "style") {
    descriptors.style = style;
  }
  if let Some(weight) = own_string(scope, object, "weight") {
    descriptors.weight = weight;
  }
  let stretch = own_string(scope, object, "stretch");
  let width = own_string(scope, object, "width");
  if let Some(width) = width.or(stretch) {
    if !valid_width(&width) {
      return Err(OpError::dom("SyntaxError", "The font width is invalid"));
    }
    descriptors.width = width;
  }
  if let Some(value) = own_string(scope, object, "unicodeRange") {
    descriptors.unicode_range = value;
  }
  if let Some(value) = own_string(scope, object, "featureSettings") {
    descriptors.feature_settings = value;
  }
  if let Some(value) = own_string(scope, object, "display") {
    descriptors.display = value;
  }
  if let Some(value) = own_string(scope, object, "ascentOverride") {
    descriptors.ascent_override = value;
  }
  if let Some(value) = own_string(scope, object, "descentOverride") {
    descriptors.descent_override = value;
  }
  if let Some(value) = own_string(scope, object, "lineGapOverride") {
    descriptors.line_gap_override = value;
  }
  if let Some(value) = own_string(scope, object, "sizeAdjust") {
    descriptors.size_adjust = value;
  }
  if let Some(value) = own_string(scope, object, "variationSettings") {
    descriptors.variation_settings = serialize_variation(&value);
  }
  Ok(descriptors)
}

fn own_string(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
) -> Option<String> {
  let key = v8::String::new(scope, name)?;
  let value = object.get(scope, key.into())?;
  if !value.is_string() {
    return None;
  }
  value
    .to_string(scope)
    .map(|text| text.to_rust_string_lossy(scope))
}

fn valid_width(value: &str) -> bool {
  matches!(
    value,
    "normal"
      | "ultra-condensed"
      | "extra-condensed"
      | "condensed"
      | "semi-condensed"
      | "semi-expanded"
      | "expanded"
      | "extra-expanded"
      | "ultra-expanded"
  ) || value
    .strip_suffix('%')
    .and_then(|number| number.parse::<f64>().ok())
    .is_some_and(|number| number.is_finite() && number > 0.0)
}

fn serialize_variation(value: &str) -> String {
  if value == "normal" {
    return value.to_string();
  }
  value.replace('\'', "\"")
}

fn serialize_family(name: &str) -> String {
  if valid_unquoted_family(name) {
    name.to_string()
  } else {
    format!("\"{}\"", escape_css(name))
  }
}

fn valid_unquoted_family(name: &str) -> bool {
  if name.is_empty() || is_generic_or_keyword(name) {
    return false;
  }
  let mut parts = name.split(' ');
  parts.all(|part| !part.is_empty() && css_ident(part))
}

fn is_generic_or_keyword(name: &str) -> bool {
  matches!(
    name,
    "serif"
      | "sans-serif"
      | "cursive"
      | "fantasy"
      | "monospace"
      | "system-ui"
      | "ui-serif"
      | "ui-sans-serif"
      | "ui-monospace"
      | "ui-rounded"
      | "emoji"
      | "math"
      | "fangsong"
      | "inherit"
      | "initial"
      | "unset"
      | "revert"
      | "revert-layer"
  )
}

fn css_ident(value: &str) -> bool {
  let mut chars = value.chars();
  let Some(first) = chars.next() else {
    return false;
  };
  if !ident_start(first) {
    return false;
  }
  chars.all(ident_continue)
}

fn ident_start(ch: char) -> bool {
  ch == '_' || ch.is_ascii_alphabetic() || !ch.is_ascii()
}

fn ident_continue(ch: char) -> bool {
  ident_start(ch) || ch.is_ascii_digit() || ch == '-'
}

fn escape_css(value: &str) -> String {
  let mut out = String::new();
  for ch in value.chars() {
    if ch == '"' || ch == '\\' {
      out.push('\\');
    }
    out.push(ch);
  }
  out
}

fn source_url(css: &str) -> Option<String> {
  let lower = css.to_ascii_lowercase();
  if let Some(index) = lower.find("url(") {
    let rest = css[index + 4..].trim_start();
    if let Some(rest) = rest.strip_prefix('"') {
      let end = rest.find('"')?;
      return Some(rest[..end].to_string());
    }
    if let Some(rest) = rest.strip_prefix('\'') {
      let end = rest.find('\'')?;
      return Some(rest[..end].to_string());
    }
    let end = rest.find(')')?;
    return Some(rest[..end].trim().to_string());
  }
  let trimmed = css.trim();
  if trimmed.is_empty() || trimmed.contains([' ', ',']) {
    None
  } else {
    Some(trimmed.to_string())
  }
}

fn read_face_list(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
) -> Result<Vec<v8::TracedReference<v8::Value>>, OpError> {
  if args.length() < 2 || args.get(1).is_undefined() {
    return Ok(Vec::new());
  }
  let Ok(object) = v8::Local::<v8::Object>::try_from(args.get(1)) else {
    return Err(OpError::type_("FontFaceSetLoadEvent init is invalid"));
  };
  let key = v8::String::new(scope, "fontfaces").unwrap();
  let Some(value) = object.get(scope, key.into()) else {
    return Ok(Vec::new());
  };
  if value.is_undefined() {
    return Ok(Vec::new());
  }
  let Ok(list) = v8::Local::<v8::Array>::try_from(value) else {
    return Err(OpError::type_("fontfaces must be a sequence"));
  };
  let mut faces = Vec::new();
  for index in 0..list.length() {
    let Some(value) = list.get_index(scope, index) else {
      continue;
    };
    faces.push(traced(scope, value));
  }
  Ok(faces)
}

fn start_fetch(
  scope: &mut v8::PinScope,
  face: v8::Local<v8::Object>,
  url: &str,
) {
  let id = NEXT_JOB.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
  JOBS.with(|slot| {
    slot.borrow_mut().insert(
      id,
      Job {
        face: v8::Global::new(scope, face),
      },
    );
  });
  let global = scope.get_current_context().global(scope);
  let key = v8::String::new(scope, "fetch").unwrap();
  let Some(fetch) = global.get(scope, key.into()) else {
    fail_job(scope, id, "NetworkError", "fetch is unavailable");
    return;
  };
  let Ok(fetch) = v8::Local::<v8::Function>::try_from(fetch) else {
    fail_job(scope, id, "NetworkError", "fetch is unavailable");
    return;
  };
  let url = v8::String::new(scope, url).unwrap();
  let Some(result) = fetch.call(scope, global.into(), &[url.into()]) else {
    fail_job(scope, id, "NetworkError", "The font could not be fetched");
    return;
  };
  let Ok(promise) = v8::Local::<v8::Promise>::try_from(result) else {
    fail_job(scope, id, "NetworkError", "The font could not be fetched");
    return;
  };
  let data = v8::Number::new(scope, id as f64);
  let fulfill = v8::Function::builder(fetch_fulfilled)
    .data(data.into())
    .build(scope)
    .unwrap();
  let reject = v8::Function::builder(fetch_rejected)
    .data(data.into())
    .build(scope)
    .unwrap();
  let _ = promise.then2(scope, fulfill, reject);
}

fn fetch_fulfilled(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut _ret: v8::ReturnValue,
) {
  let id = job_id(scope, &args);
  let response = args.get(0);
  let Ok(object) = v8::Local::<v8::Object>::try_from(response) else {
    fail_job(scope, id, "NetworkError", "The font response is invalid");
    return;
  };
  if !own_bool(scope, object, "ok").unwrap_or(false) {
    fail_job(scope, id, "NetworkError", "The font response failed");
    return;
  }
  let Some(result) = call_method(scope, object, "arrayBuffer") else {
    fail_job(scope, id, "NetworkError", "The font body could not be read");
    return;
  };
  let Ok(promise) = v8::Local::<v8::Promise>::try_from(result) else {
    fail_job(scope, id, "NetworkError", "The font body could not be read");
    return;
  };
  let data = v8::Number::new(scope, id as f64);
  let fulfill = v8::Function::builder(bytes_fulfilled)
    .data(data.into())
    .build(scope)
    .unwrap();
  let reject = v8::Function::builder(fetch_rejected)
    .data(data.into())
    .build(scope)
    .unwrap();
  let _ = promise.then2(scope, fulfill, reject);
}

fn bytes_fulfilled(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut _ret: v8::ReturnValue,
) {
  let id = job_id(scope, &args);
  let Some(bytes) = yuzuko_winter::buffer_source(scope, args.get(0)) else {
    fail_job(scope, id, "NetworkError", "The font body could not be read");
    return;
  };
  let Some(job) = JOBS.with(|slot| slot.borrow_mut().remove(&id)) else {
    return;
  };
  let object = v8::Local::new(scope, &job.face);
  let Some(face) = (unsafe { host_ref::<FontFace>(scope, object) }) else {
    return;
  };
  face.get().finish_bytes(scope, bytes);
}

fn fetch_rejected(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut _ret: v8::ReturnValue,
) {
  let id = job_id(scope, &args);
  let _ = args.get(0);
  fail_job(scope, id, "NetworkError", "The font could not be fetched");
}

fn fail_job(scope: &mut v8::PinScope, id: u64, name: &str, message: &str) {
  let Some(job) = JOBS.with(|slot| slot.borrow_mut().remove(&id)) else {
    return;
  };
  let object = v8::Local::new(scope, &job.face);
  let Some(face) = (unsafe { host_ref::<FontFace>(scope, object) }) else {
    return;
  };
  face.get().fail(scope, name, message);
}

fn job_id(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
) -> u64 {
  args.data().number_value(scope).unwrap_or(0.0).max(0.0) as u64
}

fn call_method<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  object: v8::Local<'_, v8::Object>,
  name: &str,
) -> Option<v8::Local<'s, v8::Value>> {
  let key = v8::String::new(scope, name)?;
  let value = object.get(scope, key.into())?;
  let function = v8::Local::<v8::Function>::try_from(value).ok()?;
  function.call(scope, object.into(), &[])
}

fn own_bool(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
) -> Option<bool> {
  let key = v8::String::new(scope, name)?;
  let value = object.get(scope, key.into())?;
  Some(value.is_true())
}

fn decode_sfnt(bytes: &[u8]) -> Result<Vec<u8>, String> {
  let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
    decode_sfnt_inner(bytes)
  }));
  match result {
    Ok(inner) => inner,
    Err(_) => Err("The font data is invalid".to_string()),
  }
}

fn decode_sfnt_inner(bytes: &[u8]) -> Result<Vec<u8>, String> {
  if bytes.len() < 4 {
    return Err("The font data is invalid".to_string());
  }
  let magic = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
  match magic {
    0x0001_0000 | 0x4F54_544F | 0x7472_7565 | 0x7479_7031 => Ok(bytes.to_vec()),
    0x774F_4646 => decode_woff(bytes),
    0x774F_4632 => decode_woff2(bytes),
    _ => Err("The font data is invalid".to_string()),
  }
}

fn decode_woff(bytes: &[u8]) -> Result<Vec<u8>, String> {
  if bytes.len() < 44 {
    return Err("The font data is invalid".to_string());
  }
  let flavor = read_u32(bytes, 4)?;
  let num = read_u16(bytes, 12)?;
  let mut tables = Vec::with_capacity(num as usize);
  let mut cursor = 44usize;
  for _ in 0..num {
    if cursor + 20 > bytes.len() {
      return Err("The font data is invalid".to_string());
    }
    let tag = read_u32(bytes, cursor)?;
    let offset = read_u32(bytes, cursor + 4)? as usize;
    let comp = read_u32(bytes, cursor + 8)? as usize;
    let orig = read_u32(bytes, cursor + 12)? as usize;
    let checksum = read_u32(bytes, cursor + 16)?;
    if offset.saturating_add(comp) > bytes.len() {
      return Err("The font data is invalid".to_string());
    }
    let slice = &bytes[offset..offset + comp];
    let data = if comp == orig {
      slice.to_vec()
    } else {
      let mut decoder = ZlibDecoder::new(slice);
      let mut data = Vec::new();
      decoder
        .read_to_end(&mut data)
        .map_err(|_| "The font data is invalid".to_string())?;
      if data.len() != orig {
        return Err("The font data is invalid".to_string());
      }
      data
    };
    tables.push((tag, checksum, data));
    cursor += 20;
  }
  tables.sort_by_key(|(tag, _, _)| *tag);
  let (search, entry, shift) = sfnt_search(num);
  let mut out = Vec::new();
  out.extend_from_slice(&flavor.to_be_bytes());
  out.extend_from_slice(&num.to_be_bytes());
  out.extend_from_slice(&search.to_be_bytes());
  out.extend_from_slice(&entry.to_be_bytes());
  out.extend_from_slice(&shift.to_be_bytes());
  let mut offset = 12 + num as u32 * 16;
  let mut directory = Vec::new();
  let mut body = Vec::new();
  for (tag, checksum, data) in &tables {
    directory.extend_from_slice(&tag.to_be_bytes());
    directory.extend_from_slice(&checksum.to_be_bytes());
    directory.extend_from_slice(&offset.to_be_bytes());
    directory.extend_from_slice(&(data.len() as u32).to_be_bytes());
    body.extend_from_slice(data);
    let pad = (4 - data.len() % 4) % 4;
    body.extend(std::iter::repeat_n(0, pad));
    offset += (data.len() + pad) as u32;
  }
  out.extend_from_slice(&directory);
  out.extend_from_slice(&body);
  Ok(out)
}

const WOFF2_TAGS: [&[u8; 4]; 63] = [
  b"cmap", b"head", b"hhea", b"hmtx", b"maxp", b"name", b"OS/2", b"post",
  b"cvt ", b"fpgm", b"glyf", b"loca", b"prep", b"CFF ", b"VORG", b"EBDT",
  b"EBLC", b"gasp", b"hdmx", b"kern", b"LTSH", b"PCLT", b"VDMX", b"vhea",
  b"vmtx", b"BASE", b"GDEF", b"GPOS", b"GSUB", b"EBSC", b"JSTF", b"MATH",
  b"CBDT", b"CBLC", b"COLR", b"CPAL", b"SVG ", b"sbix", b"acnt", b"avar",
  b"bdat", b"bloc", b"bsln", b"cvar", b"fdsc", b"feat", b"fmtx", b"fvar",
  b"gvar", b"hsty", b"just", b"lcar", b"mort", b"morx", b"opbd", b"prop",
  b"trak", b"Zapf", b"Silf", b"Glat", b"Gloc", b"Feat", b"Sill",
];

fn decode_woff2(bytes: &[u8]) -> Result<Vec<u8>, String> {
  if bytes.len() < 48 {
    return Err("The font data is invalid".to_string());
  }
  let flavor = read_u32(bytes, 4)?;
  let num = read_u16(bytes, 12)?;
  let compressed_size = read_u32(bytes, 20)? as usize;
  let mut cursor = 48usize;
  let mut tables = Vec::with_capacity(num as usize);
  for _ in 0..num {
    let flags = *bytes
      .get(cursor)
      .ok_or_else(|| "The font data is invalid".to_string())?;
    cursor += 1;
    let index = flags & 0x3F;
    let version = flags & 0xC0;
    let tag = if index == 0x3F {
      let tag = read_u32(bytes, cursor)?;
      cursor += 4;
      tag
    } else {
      let known = WOFF2_TAGS
        .get(index as usize)
        .ok_or_else(|| "The font data is invalid".to_string())?;
      u32::from_be_bytes(**known)
    };
    let orig = read_base128(bytes, &mut cursor)?;
    let tag_bytes = tag.to_be_bytes();
    let glyf_or_loca = tag_bytes == *b"glyf" || tag_bytes == *b"loca";
    let transformed = if glyf_or_loca {
      version != 0xC0
    } else {
      version != 0
    };
    if transformed {
      return Err("The WOFF2 table transform is not supported".to_string());
    }
    tables.push((tag, orig as usize));
  }
  if cursor + compressed_size > bytes.len() {
    return Err("The font data is invalid".to_string());
  }
  let mut plain = Vec::new();
  let mut decoder =
    brotli::Decompressor::new(&bytes[cursor..cursor + compressed_size], 4096);
  decoder
    .read_to_end(&mut plain)
    .map_err(|_| "The font data is invalid".to_string())?;
  let mut offset = 0usize;
  let mut rebuilt = Vec::new();
  for (tag, length) in tables {
    let end = offset + length;
    if end > plain.len() {
      return Err("The font data is invalid".to_string());
    }
    rebuilt.push((tag, plain[offset..end].to_vec()));
    offset = end;
  }
  if offset != plain.len() {
    return Err("The font data is invalid".to_string());
  }
  rebuilt.sort_by_key(|(tag, _)| *tag);
  write_sfnt(flavor, &mut rebuilt)
}

fn write_sfnt(
  flavor: u32,
  tables: &mut [(u32, Vec<u8>)],
) -> Result<Vec<u8>, String> {
  if let Some((_, head)) = tables
    .iter_mut()
    .find(|(tag, _)| tag.to_be_bytes() == *b"head")
    && head.len() >= 12
  {
    head[8..12].copy_from_slice(&0u32.to_be_bytes());
  }
  let num = u16::try_from(tables.len())
    .map_err(|_| "The font data is invalid".to_string())?;
  let (search, entry, shift) = sfnt_search(num);
  let mut directory = Vec::new();
  let mut body = Vec::new();
  let mut offset = 12u32 + u32::from(num) * 16;
  for (tag, data) in tables.iter() {
    let checksum = checksum(data);
    directory.extend_from_slice(&tag.to_be_bytes());
    directory.extend_from_slice(&checksum.to_be_bytes());
    directory.extend_from_slice(&offset.to_be_bytes());
    directory.extend_from_slice(&(data.len() as u32).to_be_bytes());
    body.extend_from_slice(data);
    let pad = (4 - data.len() % 4) % 4;
    body.extend(std::iter::repeat_n(0, pad));
    offset += (data.len() + pad) as u32;
  }
  let mut header = Vec::new();
  header.extend_from_slice(&flavor.to_be_bytes());
  header.extend_from_slice(&num.to_be_bytes());
  header.extend_from_slice(&search.to_be_bytes());
  header.extend_from_slice(&entry.to_be_bytes());
  header.extend_from_slice(&shift.to_be_bytes());
  let mut font = header;
  font.extend_from_slice(&directory);
  font.extend_from_slice(&body);
  let adjustment = 0xB1B0_AFBA_u32.wrapping_sub(checksum(&font));
  if let Some(index) = tables
    .iter()
    .position(|(tag, _)| tag.to_be_bytes() == *b"head")
  {
    let head_offset = 12 + tables.len() * 16;
    let mut cursor = 0usize;
    for (table_index, (_, data)) in tables.iter().enumerate() {
      if table_index == index {
        let at = head_offset + cursor + 8;
        if at + 4 <= font.len() {
          font[at..at + 4].copy_from_slice(&adjustment.to_be_bytes());
        }
        break;
      }
      cursor += data.len() + (4 - data.len() % 4) % 4;
    }
  }
  Ok(font)
}

fn checksum(data: &[u8]) -> u32 {
  let mut sum = 0u32;
  let mut index = 0;
  while index + 4 <= data.len() {
    sum = sum.wrapping_add(u32::from_be_bytes([
      data[index],
      data[index + 1],
      data[index + 2],
      data[index + 3],
    ]));
    index += 4;
  }
  if index < data.len() {
    let mut tail = [0u8; 4];
    tail[..data.len() - index].copy_from_slice(&data[index..]);
    sum = sum.wrapping_add(u32::from_be_bytes(tail));
  }
  sum
}

fn read_base128(bytes: &[u8], cursor: &mut usize) -> Result<u32, String> {
  let mut result = 0u32;
  for i in 0..5 {
    let byte = *bytes
      .get(*cursor)
      .ok_or_else(|| "The font data is invalid".to_string())?;
    *cursor += 1;
    if i == 0 && byte == 0x80 {
      return Err("The font data is invalid".to_string());
    }
    if result & 0xFE00_0000 != 0 {
      return Err("The font data is invalid".to_string());
    }
    result = (result << 7) | u32::from(byte & 0x7F);
    if byte & 0x80 == 0 {
      return Ok(result);
    }
  }
  Err("The font data is invalid".to_string())
}

fn sfnt_search(num: u16) -> (u16, u16, u16) {
  let mut power = 1u16;
  let mut entry = 0u16;
  while power.saturating_mul(2) <= num {
    power *= 2;
    entry += 1;
  }
  let search = power.saturating_mul(16);
  let shift = num.saturating_mul(16).saturating_sub(search);
  (search, entry, shift)
}

fn read_u16(bytes: &[u8], offset: usize) -> Result<u16, String> {
  let slice = bytes
    .get(offset..offset + 2)
    .ok_or_else(|| "The font data is invalid".to_string())?;
  Ok(u16::from_be_bytes([slice[0], slice[1]]))
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32, String> {
  let slice = bytes
    .get(offset..offset + 4)
    .ok_or_else(|| "The font data is invalid".to_string())?;
  Ok(u32::from_be_bytes([slice[0], slice[1], slice[2], slice[3]]))
}
