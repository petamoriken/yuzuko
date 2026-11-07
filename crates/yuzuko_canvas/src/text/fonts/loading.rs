//! `FontFaceSet` `check`, `load`, `ready`, and loading events.
//!
//! Byte loads finish in the turn that requested them. The specification
//! queues `loading`, `loadingdone`, and `loadingerror` as tasks. This
//! runtime fires those events in that same turn, before the face promise
//! settles. `ready` fulfills before `loadingdone`.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

use cssparser::Parser;
use cssparser::Token;
use yuzuko_core::op::{Js, OpError, new_dom_exception, traced};

use super::FontFace;
use super::FontFaceSetLoadEvent;
use super::SET;
use super::Status;

static NEXT_LOAD: AtomicU64 = AtomicU64::new(1);

struct Book {
  loading: Vec<v8::Global<v8::Object>>,
  loaded: Vec<v8::Global<v8::Object>>,
  failed: Vec<v8::Global<v8::Object>>,
  ready: Option<v8::Global<v8::Promise>>,
  ready_resolver: Option<v8::Global<v8::PromiseResolver>>,
  ready_pending: bool,
  set: Option<v8::Global<v8::Object>>,
}

impl Default for Book {
  fn default() -> Self {
    Self {
      loading: Vec::new(),
      loaded: Vec::new(),
      failed: Vec::new(),
      ready: None,
      ready_resolver: None,
      ready_pending: false,
      set: None,
    }
  }
}

struct LoadJob {
  font: String,
  text: String,
  resolver: v8::Global<v8::PromiseResolver>,
}

std::thread_local! {
  static BOOK: RefCell<Book> = RefCell::new(Book::default());
  static LOAD_JOBS: RefCell<HashMap<u64, LoadJob>> = RefCell::new(HashMap::new());
  static WAITING: RefCell<HashMap<u64, v8::Global<v8::PromiseResolver>>> =
    RefCell::new(HashMap::new());
}

pub(super) fn clear() {
  BOOK.with(|slot| *slot.borrow_mut() = Book::default());
  LOAD_JOBS.with(|slot| slot.borrow_mut().clear());
  WAITING.with(|slot| slot.borrow_mut().clear());
}

pub(super) fn bind_set(scope: &mut v8::PinScope, set: v8::Local<v8::Object>) {
  let resolver = v8::PromiseResolver::new(scope).unwrap();
  let promise = resolver.get_promise(scope);
  resolver.resolve(scope, set.into());
  BOOK.with(|slot| {
    let mut book = slot.borrow_mut();
    *book = Book::default();
    book.set = Some(v8::Global::new(scope, set));
    book.ready = Some(v8::Global::new(scope, promise));
  });
}

pub(super) fn ready(scope: &mut v8::PinScope) -> Js {
  let stored = BOOK.with(|slot| slot.borrow().ready.clone());
  let Some(stored) = stored else {
    let resolver = v8::PromiseResolver::new(scope).unwrap();
    let promise = resolver.get_promise(scope);
    resolver.resolve(scope, v8::undefined(scope).into());
    return promise_js(scope, promise);
  };
  let promise = v8::Local::new(scope, stored);
  promise_js(scope, promise)
}

pub(super) fn check(
  scope: &mut v8::PinScope,
  font: &str,
  text: Option<&str>,
) -> Result<bool, OpError> {
  let text = text.unwrap_or(" ");
  let Some(query) = parse_font_query(font) else {
    return Err(OpError::dom("SyntaxError", "The font shorthand is invalid"));
  };
  Ok(
    matched_faces(scope, &query, text)
      .into_iter()
      .all(|face| face_loaded(scope, face)),
  )
}

pub(super) fn load(
  scope: &mut v8::PinScope,
  font: String,
  text: Option<String>,
) -> Js {
  let resolver = v8::PromiseResolver::new(scope).unwrap();
  let promise = resolver.get_promise(scope);
  let id = NEXT_LOAD.fetch_add(1, Ordering::Relaxed);
  LOAD_JOBS.with(|slot| {
    slot.borrow_mut().insert(
      id,
      LoadJob {
        font,
        text: text.unwrap_or_else(|| " ".to_string()),
        resolver: v8::Global::new(scope, resolver),
      },
    );
  });
  let data = v8::Number::new(scope, id as f64);
  let callback = v8::Function::builder(run_load)
    .data(data.into())
    .build(scope)
    .unwrap();
  queue_microtask(scope, callback);
  promise_js(scope, promise)
}

pub(super) fn note_loading(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
) {
  if !in_set(scope, object) {
    return;
  }
  let already = BOOK.with(|slot| {
    slot
      .borrow()
      .loading
      .iter()
      .any(|face| same(scope, face, object))
  });
  if already {
    return;
  }
  let starts = BOOK.with(|slot| {
    let mut book = slot.borrow_mut();
    let starts = book.loading.is_empty();
    book.loading.push(v8::Global::new(scope, object));
    starts
  });
  if starts {
    switch_to_loading(scope);
  }
}

pub(super) fn note_settled(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  loaded: bool,
) {
  let idle = BOOK.with(|slot| {
    let mut book = slot.borrow_mut();
    let before = book.loading.len();
    book.loading.retain(|face| !same(scope, face, object));
    let tracked = book.loading.len() != before;
    if tracked && in_set(scope, object) {
      let global = v8::Global::new(scope, object);
      if loaded {
        book.loaded.push(global);
      } else {
        book.failed.push(global);
      }
    }
    tracked && book.loading.is_empty()
  });
  if idle {
    switch_to_loaded(scope);
  }
}

pub(super) fn forget_face(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
) {
  let idle = BOOK.with(|slot| {
    let mut book = slot.borrow_mut();
    let before = book.loading.len();
    book.loading.retain(|face| !same(scope, face, object));
    book.loaded.retain(|face| !same(scope, face, object));
    book.failed.retain(|face| !same(scope, face, object));
    before > book.loading.len() && book.loading.is_empty()
  });
  if idle {
    switch_to_loaded(scope);
  }
}

/// Clears the loading lists. `true` when a load was in progress.
pub(super) fn take_pending() -> bool {
  BOOK.with(|slot| {
    let mut book = slot.borrow_mut();
    let pending = !book.loading.is_empty();
    book.loading.clear();
    book.loaded.clear();
    book.failed.clear();
    pending
  })
}

pub(super) fn switch_to_loaded(scope: &mut v8::PinScope) {
  let (loaded, failed, resolver, set) = BOOK.with(|slot| {
    let mut book = slot.borrow_mut();
    let loaded = std::mem::take(&mut book.loaded);
    let failed = std::mem::take(&mut book.failed);
    let resolver = if book.ready_pending {
      book.ready_pending = false;
      book.ready_resolver.take()
    } else {
      None
    };
    (loaded, failed, resolver, book.set.clone())
  });
  if let Some(resolver) = resolver {
    let resolver = v8::Local::new(scope, resolver);
    let value = set
      .as_ref()
      .map(|set| {
        let object = v8::Local::new(scope, set);
        v8::Local::<v8::Value>::from(object)
      })
      .unwrap_or_else(|| v8::undefined(scope).into());
    resolver.resolve(scope, value);
  }
  fire(scope, "loadingdone", &loaded);
  if !failed.is_empty() {
    fire(scope, "loadingerror", &failed);
  }
}

fn switch_to_loading(scope: &mut v8::PinScope) {
  let pending = BOOK.with(|slot| slot.borrow().ready_pending);
  if !pending {
    let resolver = v8::PromiseResolver::new(scope).unwrap();
    let promise = resolver.get_promise(scope);
    BOOK.with(|slot| {
      let mut book = slot.borrow_mut();
      book.ready = Some(v8::Global::new(scope, promise));
      book.ready_resolver = Some(v8::Global::new(scope, resolver));
      book.ready_pending = true;
    });
  }
  fire(scope, "loading", &[]);
}

fn fire(
  scope: &mut v8::PinScope,
  name: &str,
  faces: &[v8::Global<v8::Object>],
) {
  let Some(target) = BOOK.with(|slot| slot.borrow().set.clone()) else {
    return;
  };
  let target = v8::Local::new(scope, target);
  let kept = faces
    .iter()
    .filter(|face| in_set(scope, v8::Local::new(scope, *face)))
    .cloned()
    .collect::<Vec<_>>();
  let event = FontFaceSetLoadEvent {
    base: yuzuko_dom::Event::new(name, false, false, false, true),
    faces: RefCell::new(
      kept
        .iter()
        .map(|face| {
          let object = v8::Local::new(scope, face);
          let value = v8::Local::<v8::Value>::from(object);
          traced(scope, value)
        })
        .collect(),
    ),
  };
  let object = unsafe { yuzuko_core::op::wrap_instance(scope, event) };
  yuzuko_dom::define_is_trusted(scope, object);
  let _ = yuzuko_dom::dispatch(scope, target, object);
}

fn run_load(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut _ret: v8::ReturnValue,
) {
  let id = args.data().number_value(scope).unwrap_or(0.0).max(0.0) as u64;
  let Some(job) = LOAD_JOBS.with(|slot| slot.borrow_mut().remove(&id)) else {
    return;
  };
  let resolver = v8::Local::new(scope, &job.resolver);
  let Some(query) = parse_font_query(&job.font) else {
    reject_syntax(scope, resolver);
    return;
  };
  let faces = matched_faces(scope, &query, &job.text);
  let array = v8::Array::new(scope, faces.len() as i32);
  for (index, face) in faces.iter().enumerate() {
    let object = v8::Local::new(scope, face);
    let Some(host) =
      (unsafe { yuzuko_core::op::host_ref::<FontFace>(scope, object) })
    else {
      let message = v8::String::new(scope, "The font face is invalid").unwrap();
      resolver.reject(scope, v8::Exception::type_error(scope, message));
      return;
    };
    let promise = host.get().load_call_impl(scope);
    let value = v8::Local::new(scope, &promise.0);
    array.set_index(scope, index as u32, value);
  }
  if faces.is_empty() {
    resolver.resolve(scope, array.into());
    return;
  }
  let Some(all) = promise_all(scope, array) else {
    let message =
      v8::String::new(scope, "The font load could not start").unwrap();
    resolver.reject(scope, v8::Exception::type_error(scope, message));
    return;
  };
  let wait = NEXT_LOAD.fetch_add(1, Ordering::Relaxed);
  WAITING.with(|slot| {
    slot
      .borrow_mut()
      .insert(wait, v8::Global::new(scope, resolver));
  });
  let data = v8::Number::new(scope, wait as f64);
  let fulfill = v8::Function::builder(load_resolved)
    .data(data.into())
    .build(scope)
    .unwrap();
  let reject = v8::Function::builder(load_rejected)
    .data(data.into())
    .build(scope)
    .unwrap();
  let _ = all.then2(scope, fulfill, reject);
}

fn load_resolved(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut _ret: v8::ReturnValue,
) {
  let Some(stored) = take_wait(scope, &args) else {
    return;
  };
  let resolver = v8::Local::new(scope, stored);
  resolver.resolve(scope, args.get(0));
}

fn load_rejected(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut _ret: v8::ReturnValue,
) {
  let Some(stored) = take_wait(scope, &args) else {
    return;
  };
  let resolver = v8::Local::new(scope, stored);
  resolver.reject(scope, args.get(0));
}

fn take_wait(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
) -> Option<v8::Global<v8::PromiseResolver>> {
  let id = args.data().number_value(scope).unwrap_or(0.0).max(0.0) as u64;
  WAITING.with(|slot| slot.borrow_mut().remove(&id))
}

fn promise_js(scope: &mut v8::PinScope, promise: v8::Local<v8::Promise>) -> Js {
  let value: v8::Local<v8::Value> = promise.into();
  Js(v8::Global::new(scope, value))
}

fn reject_syntax(
  scope: &mut v8::PinScope,
  resolver: v8::Local<v8::PromiseResolver>,
) {
  let error =
    new_dom_exception(scope, "SyntaxError", "The font shorthand is invalid")
      .map(v8::Local::<v8::Value>::from)
      .unwrap_or_else(|| {
        let message =
          v8::String::new(scope, "The font shorthand is invalid").unwrap();
        v8::Exception::type_error(scope, message)
      });
  resolver.reject(scope, error);
}

fn promise_all<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  values: v8::Local<v8::Array>,
) -> Option<v8::Local<'s, v8::Promise>> {
  let global = scope.get_current_context().global(scope);
  let key = v8::String::new(scope, "Promise")?;
  let ctor = global.get(scope, key.into())?;
  let ctor = v8::Local::<v8::Object>::try_from(ctor).ok()?;
  let name = v8::String::new(scope, "all")?;
  let all = ctor.get(scope, name.into())?;
  let all = v8::Local::<v8::Function>::try_from(all).ok()?;
  let result = all.call(scope, ctor.into(), &[values.into()])?;
  v8::Local::<v8::Promise>::try_from(result).ok()
}

fn queue_microtask(
  scope: &mut v8::PinScope,
  callback: v8::Local<v8::Function>,
) {
  let global = scope.get_current_context().global(scope);
  let Some(key) = v8::String::new(scope, "queueMicrotask") else {
    return;
  };
  let Some(value) = global.get(scope, key.into()) else {
    return;
  };
  let Ok(function) = v8::Local::<v8::Function>::try_from(value) else {
    return;
  };
  let _ = function.call(scope, global.into(), &[callback.into()]);
}

fn matched_faces(
  scope: &mut v8::PinScope,
  query: &FontQuery,
  text: &str,
) -> Vec<v8::Global<v8::Object>> {
  if text.is_empty() || query.system {
    return Vec::new();
  }
  let mut faces = Vec::new();
  for family in &query.families {
    for face in select_family(scope, family, query) {
      if range_covers(&face.unicode, text) {
        faces.push(face.object);
      }
    }
  }
  faces
}

fn face_loaded(scope: &mut v8::PinScope, face: v8::Global<v8::Object>) -> bool {
  let object = v8::Local::new(scope, face);
  unsafe { yuzuko_core::op::host_ref::<FontFace>(scope, object) }
    .is_some_and(|face| face.get().status.get() == Status::Loaded)
}

struct Candidate {
  object: v8::Global<v8::Object>,
  style: StyleReq,
  weight: Axis,
  stretch: Axis,
  unicode: String,
}

fn select_family(
  scope: &mut v8::PinScope,
  family: &str,
  query: &FontQuery,
) -> Vec<Candidate> {
  let globals = SET.with(|slot| {
    slot
      .borrow()
      .iter()
      .filter_map(|item| item.face.clone())
      .collect::<Vec<_>>()
  });
  let mut faces = Vec::new();
  for global in globals {
    let object = v8::Local::new(scope, &global);
    let Some(face) =
      (unsafe { yuzuko_core::op::host_ref::<FontFace>(scope, object) })
    else {
      continue;
    };
    let face = face.get();
    if !face.family_raw.eq_ignore_ascii_case(family) {
      continue;
    }
    let descriptors = face.descriptors.borrow();
    faces.push(Candidate {
      object: global,
      style: parse_style(&descriptors.style),
      weight: parse_weight(&descriptors.weight),
      stretch: parse_stretch(&descriptors.width),
      unicode: descriptors.unicode_range.clone(),
    });
  }
  if faces.is_empty() {
    return faces;
  }
  let stretch_distance =
    |face: &Candidate| axis_distance(face.stretch, query.stretch);
  let best_stretch =
    faces.iter().map(stretch_distance).fold(f32::MAX, f32::min);
  faces.retain(|face| stretch_distance(face) == best_stretch);
  let best_style = faces
    .iter()
    .map(|face| style_rank(query.style, face.style))
    .min()
    .unwrap_or(0);
  faces.retain(|face| style_rank(query.style, face.style) == best_style);
  let weight_distance =
    |face: &Candidate| axis_distance(face.weight, f32::from(query.weight));
  let best_weight = faces.iter().map(weight_distance).fold(f32::MAX, f32::min);
  faces.retain(|face| weight_distance(face) == best_weight);
  faces
}

fn in_set(scope: &mut v8::PinScope, object: v8::Local<v8::Object>) -> bool {
  SET.with(|slot| {
    slot.borrow().iter().any(|item| {
      item.face.as_ref().is_some_and(|face| {
        v8::Local::new(scope, face).strict_equals(object.into())
      })
    })
  })
}

fn same(
  scope: &mut v8::PinScope,
  global: &v8::Global<v8::Object>,
  object: v8::Local<v8::Object>,
) -> bool {
  v8::Local::new(scope, global).strict_equals(object.into())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StyleReq {
  Normal,
  Italic,
  Oblique,
}

#[derive(Clone, Copy, Debug)]
struct Axis {
  start: f32,
  end: f32,
}

impl Axis {
  fn point(value: f32) -> Self {
    Self {
      start: value,
      end: value,
    }
  }
}

fn axis_distance(axis: Axis, target: f32) -> f32 {
  if target >= axis.start && target <= axis.end {
    0.0
  } else if target < axis.start {
    axis.start - target
  } else {
    target - axis.end
  }
}

fn style_rank(desired: StyleReq, face: StyleReq) -> u8 {
  match desired {
    StyleReq::Normal => match face {
      StyleReq::Normal => 0,
      StyleReq::Oblique => 1,
      StyleReq::Italic => 2,
    },
    StyleReq::Italic => match face {
      StyleReq::Italic => 0,
      StyleReq::Oblique => 1,
      StyleReq::Normal => 2,
    },
    StyleReq::Oblique => match face {
      StyleReq::Oblique => 0,
      StyleReq::Italic => 1,
      StyleReq::Normal => 2,
    },
  }
}

struct FontQuery {
  system: bool,
  style: StyleReq,
  weight: u16,
  stretch: f32,
  families: Vec<String>,
}

impl FontQuery {
  fn system() -> Self {
    Self {
      system: true,
      style: StyleReq::Normal,
      weight: 400,
      stretch: 100.0,
      families: Vec::new(),
    }
  }
}

fn parse_font_query(input: &str) -> Option<FontQuery> {
  let mut parser = Parser::new(input);
  if parser.is_exhausted() {
    return None;
  }
  let state = parser.state();
  if let Ok(ident) = parser.expect_ident() {
    let name = ident.to_ascii_lowercase();
    if parser.is_exhausted() {
      if is_css_wide(&name) || name == "default" {
        return None;
      }
      if is_system_font(&name) {
        return Some(FontQuery::system());
      }
    }
  }
  parser.reset(&state);
  let mut style = StyleReq::Normal;
  let mut weight = 400u16;
  let mut stretch = 100.0f32;
  while consume_prefix(&mut parser, &mut style, &mut weight, &mut stretch) {}
  if !consume_size(&mut parser) {
    return None;
  }
  let slash = parser.state();
  if parser.expect_delim('/').is_ok() {
    if !consume_line_height(&mut parser) {
      return None;
    }
  } else {
    parser.reset(&slash);
  }
  let families = parse_families(&mut parser)?;
  if families.is_empty() || !parser.is_exhausted() {
    return None;
  }
  Some(FontQuery {
    system: false,
    style,
    weight,
    stretch,
    families,
  })
}

fn consume_prefix(
  parser: &mut Parser,
  style: &mut StyleReq,
  weight: &mut u16,
  stretch: &mut f32,
) -> bool {
  let state = parser.state();
  if let Ok(ident) = parser.expect_ident() {
    let lower = ident.to_ascii_lowercase();
    if lower == "oblique" {
      *style = StyleReq::Oblique;
      let angle = parser.state();
      if !consume_angle(parser) {
        parser.reset(&angle);
      }
      return true;
    }
    if lower == "italic" {
      *style = StyleReq::Italic;
      return true;
    }
    if lower == "normal" || lower == "small-caps" {
      return true;
    }
    if let Some(value) = weight_keyword(&lower) {
      *weight = value;
      return true;
    }
    if let Some(value) = stretch_keyword(&lower) {
      *stretch = value;
      return true;
    }
    parser.reset(&state);
    return false;
  }
  parser.reset(&state);
  if let Ok(number) = parser.expect_number() {
    if number.is_finite() && (1.0..=1000.0).contains(&number) {
      *weight = number.round() as u16;
      return true;
    }
    parser.reset(&state);
    return false;
  }
  parser.reset(&state);
  let percent_at = parser.state();
  if let Some(percent) = consume_percent(parser) {
    let followed = {
      let look = parser.state();
      let size = consume_size(parser);
      parser.reset(&look);
      size
    };
    if followed && percent > 0.0 {
      *stretch = percent;
      return true;
    }
  }
  parser.reset(&percent_at);
  false
}

fn consume_size(parser: &mut Parser) -> bool {
  let state = parser.state();
  let kind = {
    let Ok(token) = parser.next() else {
      parser.reset(&state);
      return false;
    };
    match token {
      Token::Ident(name) => {
        let name = name.to_ascii_lowercase();
        if is_size_keyword(&name) {
          Some(true)
        } else {
          Some(false)
        }
      }
      Token::Dimension { value, unit, .. } => {
        Some(*value > 0.0 && value.is_finite() && is_length_unit(unit))
      }
      Token::Percentage { unit_value, .. } => {
        Some(*unit_value > 0.0 && unit_value.is_finite())
      }
      _ => Some(false),
    }
  };
  if kind == Some(true) {
    true
  } else {
    parser.reset(&state);
    false
  }
}

fn consume_line_height(parser: &mut Parser) -> bool {
  let state = parser.state();
  if let Ok(ident) = parser.expect_ident() {
    if ident.eq_ignore_ascii_case("normal") {
      return true;
    }
  }
  // `expect_ident` consumes the token even when it fails.
  parser.reset(&state);
  let kind = {
    let Ok(token) = parser.next() else {
      parser.reset(&state);
      return false;
    };
    match token {
      Token::Number { value, .. } => *value >= 0.0 && value.is_finite(),
      Token::Percentage { unit_value, .. } => *unit_value >= 0.0,
      Token::Dimension { value, unit, .. } => {
        *value > 0.0 && value.is_finite() && is_length_unit(unit)
      }
      _ => false,
    }
  };
  if kind {
    true
  } else {
    parser.reset(&state);
    false
  }
}

fn consume_angle(parser: &mut Parser) -> bool {
  let state = parser.state();
  let angle = {
    let Ok(token) = parser.next() else {
      parser.reset(&state);
      return false;
    };
    match token {
      Token::Dimension { unit, .. } => is_angle(unit),
      _ => false,
    }
  };
  if angle {
    true
  } else {
    parser.reset(&state);
    false
  }
}

fn consume_percent(parser: &mut Parser) -> Option<f32> {
  let state = parser.state();
  let value = {
    let Ok(token) = parser.next() else {
      parser.reset(&state);
      return None;
    };
    match token {
      Token::Percentage { unit_value, .. } => Some(*unit_value * 100.0),
      _ => None,
    }
  };
  if value.is_some() {
    value
  } else {
    parser.reset(&state);
    None
  }
}

fn parse_families(parser: &mut Parser) -> Option<Vec<String>> {
  let mut families = Vec::new();
  loop {
    families.push(parse_one_family(parser)?);
    if parser.is_exhausted() {
      return Some(families);
    }
    parser.expect_comma().ok()?;
    if parser.is_exhausted() {
      return None;
    }
  }
}

fn parse_one_family(parser: &mut Parser) -> Option<String> {
  let state = parser.state();
  if let Ok(text) = parser.expect_string() {
    return Some(text.to_string());
  }
  parser.reset(&state);
  let mut parts = Vec::new();
  loop {
    let state = parser.state();
    let Ok(ident) = parser.expect_ident() else {
      parser.reset(&state);
      break;
    };
    let ident = ident.to_string();
    if is_reserved_family(&ident) {
      return None;
    }
    parts.push(ident);
  }
  if parts.is_empty() {
    None
  } else {
    Some(parts.join(" "))
  }
}

fn is_size_keyword(name: &str) -> bool {
  matches!(
    name,
    "xx-small"
      | "x-small"
      | "small"
      | "medium"
      | "large"
      | "x-large"
      | "xx-large"
      | "xxx-large"
      | "larger"
      | "smaller"
  )
}

fn is_length_unit(unit: &str) -> bool {
  matches!(
    unit.to_ascii_lowercase().as_str(),
    "px"
      | "pt"
      | "pc"
      | "in"
      | "cm"
      | "mm"
      | "q"
      | "em"
      | "rem"
      | "ex"
      | "ch"
      | "cap"
      | "ic"
      | "lh"
      | "rlh"
      | "vw"
      | "vh"
      | "vi"
      | "vb"
      | "vmin"
      | "vmax"
      | "dvw"
      | "dvh"
      | "dvi"
      | "dvb"
      | "dvmin"
      | "dvmax"
      | "svw"
      | "svh"
      | "svi"
      | "svb"
      | "svmin"
      | "svmax"
      | "lvw"
      | "lvh"
      | "lvi"
      | "lvb"
      | "lvmin"
      | "lvmax"
      | "cqw"
      | "cqh"
      | "cqi"
      | "cqb"
      | "cqmin"
      | "cqmax"
  )
}

fn is_angle(unit: &str) -> bool {
  matches!(
    unit.to_ascii_lowercase().as_str(),
    "deg" | "rad" | "grad" | "turn"
  )
}

fn weight_keyword(name: &str) -> Option<u16> {
  match name {
    "normal" => Some(400),
    "bold" | "bolder" => Some(700),
    "lighter" => Some(100),
    _ => None,
  }
}

fn stretch_keyword(name: &str) -> Option<f32> {
  Some(match name {
    "ultra-condensed" => 50.0,
    "extra-condensed" => 62.5,
    "condensed" => 75.0,
    "semi-condensed" => 87.5,
    "normal" => 100.0,
    "semi-expanded" => 112.5,
    "expanded" => 125.0,
    "extra-expanded" => 150.0,
    "ultra-expanded" => 200.0,
    _ => return None,
  })
}

fn is_system_font(name: &str) -> bool {
  matches!(
    name,
    "caption"
      | "icon"
      | "menu"
      | "message-box"
      | "small-caption"
      | "status-bar"
  )
}

fn is_css_wide(name: &str) -> bool {
  matches!(
    name,
    "inherit" | "initial" | "unset" | "revert" | "revert-layer"
  )
}

fn is_reserved_family(name: &str) -> bool {
  let name = name.to_ascii_lowercase();
  is_css_wide(&name) || name == "default"
}

fn parse_style(value: &str) -> StyleReq {
  let value = value.trim().to_ascii_lowercase();
  if value.starts_with("italic") {
    StyleReq::Italic
  } else if value.starts_with("oblique") {
    StyleReq::Oblique
  } else {
    StyleReq::Normal
  }
}

fn parse_weight(value: &str) -> Axis {
  parse_axis(value, |token| weight_keyword(token).map(f32::from))
    .unwrap_or_else(|| Axis::point(400.0))
}

fn parse_stretch(value: &str) -> Axis {
  parse_axis(value, stretch_keyword).unwrap_or_else(|| Axis::point(100.0))
}

fn parse_axis(
  value: &str,
  keyword: impl Fn(&str) -> Option<f32>,
) -> Option<Axis> {
  let mut parts = value.split_whitespace();
  let start = axis_token(parts.next()?, &keyword)?;
  let end = if let Some(token) = parts.next() {
    axis_token(token, &keyword)?
  } else {
    start
  };
  if parts.next().is_some() || !start.is_finite() || !end.is_finite() {
    return None;
  }
  Some(if start <= end {
    Axis { start, end }
  } else {
    Axis {
      start: end,
      end: start,
    }
  })
}

fn axis_token(
  token: &str,
  keyword: &impl Fn(&str) -> Option<f32>,
) -> Option<f32> {
  if let Some(value) = keyword(token) {
    return Some(value);
  }
  if let Some(number) = token.strip_suffix('%') {
    let value = number.parse::<f32>().ok()?;
    return value.is_finite().then_some(value);
  }
  let value = token.parse::<f32>().ok()?;
  value.is_finite().then_some(value)
}

fn range_covers(range: &str, text: &str) -> bool {
  let spans = unicode_spans(range);
  text.chars().any(|ch| {
    let point = ch as u32;
    spans.iter().any(|span| point >= span.0 && point <= span.1)
  })
}

fn unicode_spans(range: &str) -> Vec<(u32, u32)> {
  range
    .split(',')
    .filter_map(|piece| unicode_span(piece.trim()))
    .collect()
}

fn unicode_span(piece: &str) -> Option<(u32, u32)> {
  let body = piece
    .get(2..)
    .filter(|_| piece.len() >= 2 && piece[..2].eq_ignore_ascii_case("u+"))?;
  if body.contains('?') {
    if body.contains('-')
      || body.is_empty()
      || body.len() > 6
      || !body.chars().all(|ch| ch.is_ascii_hexdigit() || ch == '?')
    {
      return None;
    }
    let start = u32::from_str_radix(&body.replace('?', "0"), 16).ok()?;
    let end = u32::from_str_radix(&body.replace('?', "f"), 16).ok()?;
    return (end <= 0x10_FFFF).then_some((start, end));
  }
  let (left, right) = body.split_once('-').unwrap_or((body, body));
  if left.is_empty() || right.is_empty() || left.len() > 6 || right.len() > 6 {
    return None;
  }
  let start = u32::from_str_radix(left, 16).ok()?;
  let end = u32::from_str_radix(right, 16).ok()?;
  (start <= end && end <= 0x10_FFFF).then_some((start, end))
}

#[cfg(test)]
mod tests {
  use super::StyleReq;
  use super::parse_font_query;
  use super::range_covers;

  #[test]
  fn parses_font_shorthand() {
    let query =
      parse_font_query("italic bold 16px/1.2 \"My Font\", sans-serif")
        .expect("shorthand");
    assert_eq!(query.style, StyleReq::Italic);
    assert_eq!(query.weight, 700);
    assert_eq!(query.families, ["My Font", "sans-serif"]);
    assert!(parse_font_query("inherit").is_none());
    assert!(parse_font_query("caption").expect("caption").system);
    assert_eq!(
      parse_font_query("bolder condensed 12pt Foo")
        .expect("relative")
        .weight,
      700
    );
    assert!(parse_font_query("16px").is_none());
    assert!(range_covers("U+41", "A"));
    assert!(!range_covers("U+41", "B"));
    assert!(range_covers("U+4?", "A"));
    assert!(range_covers("U+0-10FFFF", " "));
    assert!(!range_covers("U+41", ""));
  }
}
