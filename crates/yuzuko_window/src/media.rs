//! `Window.matchMedia` and a Media Queries Level 4 subset.

use std::cell::{Cell, RefCell};

use yuzuko_core::op::{Js, OpError, host_ref, traced, traced_local};
use yuzuko_dom::{
  Event, EventTarget, define_is_trusted, dispatch, report_error,
};
use yuzuko_macros::op;

use crate::inbox;
use crate::prefs::{self, Contrast, Gamut, Scheme};

#[derive(Clone, Debug)]
enum Query {
  False,
  True,
  Not(Box<Query>),
  And(Vec<Query>),
  Or(Vec<Query>),
  Width(Numeric),
  Height(Numeric),
  Resolution(Numeric),
  Portrait(bool),
  Gamut(u8),
  Dark(bool),
  ReduceMotion(bool),
  Contrast(u8),
  Pointer(u8),
  BoolWidth,
  BoolHeight,
  BoolResolution,
  BoolPointer,
  BoolScheme,
  BoolMotion,
  BoolContrast,
}

#[derive(Clone, Copy, Debug)]
struct Bound {
  value: f64,
  inclusive: bool,
}

#[derive(Clone, Copy, Debug)]
struct Numeric {
  lower: Option<Bound>,
  upper: Option<Bound>,
}

#[derive(Clone, Copy, Debug)]
pub struct Snapshot {
  pub width: f64,
  pub height: f64,
  pub dpr: f64,
  pub gamut: u8,
  pub dark: bool,
  pub reduce_motion: bool,
  pub contrast: u8,
  pub pointer: u8,
}

pub fn snapshot(window_id: u64) -> Option<Snapshot> {
  let size = inbox::size(window_id)?;
  let prefs = prefs::current();
  Some(Snapshot {
    width: size.css_width,
    height: size.css_height,
    dpr: size.scale,
    // 2 is rec2020. This host reports srgb or p3.
    gamut: match prefs.gamut {
      Gamut::Srgb => 0,
      Gamut::P3 => 1,
    },
    dark: prefs.scheme == Scheme::Dark,
    reduce_motion: prefs.reduced_motion,
    // 1 is less and 3 is custom. This host reports neither.
    contrast: match prefs.contrast {
      Contrast::NoPreference => 0,
      Contrast::More => 2,
    },
    // This runtime is a desktop window, so the primary pointer is fine.
    pointer: 2,
  })
}

pub fn refresh(scope: &mut v8::PinScope, window_id: u64) {
  let Some(state) = snapshot(window_id) else {
    return;
  };
  let Some(window) = inbox::object(scope, window_id) else {
    return;
  };
  let lists = list_roots(scope, window);
  for list_global in lists {
    let list_object = v8::Local::new(scope, &list_global);
    let Some(pending) = pending_change(scope, list_object, &state) else {
      continue;
    };
    deliver(scope, list_object, pending);
  }
}

struct Pending<'s> {
  matches: bool,
  media: String,
  onchange: Option<v8::Local<'s, v8::Function>>,
  legacy: Vec<v8::Local<'s, v8::Function>>,
}

fn list_roots(
  scope: &mut v8::PinScope,
  window: v8::Local<v8::Object>,
) -> Vec<v8::Global<v8::Object>> {
  let count = {
    let Some(host) =
      (unsafe { host_ref::<crate::surface::Window>(scope, window) })
    else {
      return Vec::new();
    };
    host.get().media_lists().borrow().len()
  };
  let mut locals = Vec::new();
  for index in 0..count {
    let Some(host) =
      (unsafe { host_ref::<crate::surface::Window>(scope, window) })
    else {
      break;
    };
    let local = {
      let lists = host.get().media_lists().borrow();
      lists
        .get(index)
        .and_then(|reference| traced_local(scope, reference))
    };
    if let Some(local) = local {
      locals.push(local);
    }
  }
  locals
    .into_iter()
    .map(|local| v8::Global::new(scope, local))
    .collect()
}

fn pending_change<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  list_object: v8::Local<'s, v8::Object>,
  state: &Snapshot,
) -> Option<Pending<'s>> {
  let host = unsafe { host_ref::<MediaQueryList>(scope, list_object) }?;
  let list = host.get();
  let now = evaluate(&list.query, state);
  if now == list.matches.get() {
    return None;
  }
  list.matches.set(now);
  let media = list.media.borrow().clone();
  let onchange = {
    let stored = list.onchange.borrow();
    stored
      .as_ref()
      .and_then(|handler| traced_local(scope, handler))
  };
  let legacy = {
    let stored = list.legacy.borrow();
    stored
      .iter()
      .filter_map(|callback| traced_local(scope, callback))
      .collect()
  };
  Some(Pending {
    matches: now,
    media,
    onchange,
    legacy,
  })
}

fn deliver(
  scope: &mut v8::PinScope,
  list_object: v8::Local<v8::Object>,
  pending: Pending<'_>,
) {
  let event = MediaQueryListEvent {
    base: Event::new("change", false, false, false, true),
    matches: Cell::new(pending.matches),
    media: RefCell::new(pending.media),
  };
  let event_object = unsafe { yuzuko_core::op::wrap_instance(scope, event) };
  define_is_trusted(scope, event_object);
  let _ = dispatch(scope, list_object, event_object);
  if let Some(function) = pending.onchange {
    call_callback(scope, function, list_object, event_object.into());
  }
  for function in pending.legacy {
    call_callback(scope, function, list_object, list_object.into());
  }
}

fn call_callback(
  scope: &mut v8::PinScope,
  function: v8::Local<v8::Function>,
  this: v8::Local<v8::Object>,
  argument: v8::Local<v8::Value>,
) {
  let thrown = {
    v8::tc_scope!(let tc, scope);
    let called = function.call(tc, this.into(), &[argument]).is_some();
    if called && !tc.has_caught() {
      None
    } else {
      let error = tc.exception().map(|value| v8::Global::new(tc, value));
      tc.reset();
      error
    }
  };
  if let Some(error) = thrown {
    let error = v8::Local::new(scope, error);
    report_error(scope, error);
  }
}

fn evaluate(query: &Query, state: &Snapshot) -> bool {
  match query {
    Query::False => false,
    Query::True => true,
    Query::Not(inner) => !evaluate(inner, state),
    Query::And(items) => items.iter().all(|item| evaluate(item, state)),
    Query::Or(items) => items.iter().any(|item| evaluate(item, state)),
    Query::Width(test) => in_range(state.width, test),
    Query::Height(test) => in_range(state.height, test),
    Query::Resolution(test) => in_range(state.dpr, test),
    Query::Portrait(portrait) => {
      let is_portrait = state.height >= state.width;
      *portrait == is_portrait
    }
    Query::Gamut(want) => state.gamut >= *want,
    Query::Dark(dark) => state.dark == *dark,
    Query::ReduceMotion(reduce) => state.reduce_motion == *reduce,
    Query::Contrast(want) => state.contrast == *want,
    Query::Pointer(want) => state.pointer == *want,
    Query::BoolWidth => state.width != 0.0,
    Query::BoolHeight => state.height != 0.0,
    Query::BoolResolution => state.dpr != 0.0,
    Query::BoolPointer => state.pointer != 0,
    Query::BoolScheme => true,
    Query::BoolMotion => state.reduce_motion,
    Query::BoolContrast => state.contrast != 0,
  }
}

fn in_range(value: f64, test: &Numeric) -> bool {
  if !value.is_finite() {
    return false;
  }
  if let Some(lower) = test.lower
    && !above(value, lower)
  {
    return false;
  }
  if let Some(upper) = test.upper
    && !below(value, upper)
  {
    return false;
  }
  true
}

fn above(value: f64, bound: Bound) -> bool {
  if bound.inclusive {
    value + 1e-9 >= bound.value
  } else {
    value > bound.value
  }
}

fn below(value: f64, bound: Bound) -> bool {
  if bound.inclusive {
    value - 1e-9 <= bound.value
  } else {
    value < bound.value
  }
}

fn parse(input: &str) -> Query {
  let mut parser = Parser::new(input);
  parser.skip();
  if parser.eof() {
    return Query::True;
  }
  let mut parts = vec![parse_media_query(&mut parser)];
  while parser.eat(",") {
    parts.push(parse_media_query(&mut parser));
  }
  parser.skip();
  if !parser.eof() {
    return Query::False;
  }
  if parts.len() == 1 {
    parts.pop().unwrap_or(Query::False)
  } else {
    Query::Or(parts)
  }
}

fn parse_media_query(parser: &mut Parser) -> Query {
  parser.skip();
  if parser.starts_with('(') || parser.keyword_then_paren("not") {
    return parse_condition(parser);
  }
  let negated = parser.eat_keyword("not");
  let _ = parser.eat_keyword("only");
  let Some(kind) = parser.ident() else {
    return Query::False;
  };
  let base = match kind.to_ascii_lowercase().as_str() {
    "all" | "screen" => Query::True,
    _ => Query::False,
  };
  parser.skip();
  let query = if parser.eat_keyword("and") {
    Query::And(vec![base, parse_condition(parser)])
  } else {
    base
  };
  if negated {
    Query::Not(Box::new(query))
  } else {
    query
  }
}

fn parse_condition(parser: &mut Parser) -> Query {
  parser.skip();
  if parser.eat_keyword("not") {
    return Query::Not(Box::new(parse_in_parens(parser)));
  }
  let first = parse_in_parens(parser);
  parser.skip();
  if parser.starts_keyword("and") {
    let mut items = vec![first];
    while parser.eat_keyword("and") {
      items.push(parse_in_parens(parser));
      parser.skip();
    }
    if parser.starts_keyword("or") {
      return Query::False;
    }
    Query::And(items)
  } else if parser.starts_keyword("or") {
    let mut items = vec![first];
    while parser.eat_keyword("or") {
      items.push(parse_in_parens(parser));
      parser.skip();
    }
    if parser.starts_keyword("and") {
      return Query::False;
    }
    Query::Or(items)
  } else {
    first
  }
}

fn parse_in_parens(parser: &mut Parser) -> Query {
  parser.skip();
  if !parser.eat("(") {
    return Query::False;
  }
  parser.skip();
  let query = if parser.starts_keyword("not") || parser.starts_with('(') {
    parse_condition(parser)
  } else {
    parse_feature(parser)
  };
  parser.skip();
  if !parser.eat(")") {
    return Query::False;
  }
  query
}

fn parse_feature(parser: &mut Parser) -> Query {
  parser.skip();
  if parser.looking_at_number() {
    return parse_value_first(parser);
  }
  let Some(name) = parser.ident() else {
    return Query::False;
  };
  parser.skip();
  if parser.eat(":") || parser.eat("=") {
    let Some(value) = parser.value() else {
      return Query::False;
    };
    return plain_feature(&name, value);
  }
  if let Some(op) = parser.operator() {
    let Some(value) = parser.value() else {
      return Query::False;
    };
    return range_feature(&name, op, value);
  }
  boolean_feature(&name)
}

fn parse_value_first(parser: &mut Parser) -> Query {
  let Some(left) = parser.value() else {
    return Query::False;
  };
  let Some(op) = parser.operator() else {
    return Query::False;
  };
  let Some(name) = parser.ident() else {
    return Query::False;
  };
  parser.skip();
  if let Some(op2) = parser.operator() {
    let Some(right) = parser.value() else {
      return Query::False;
    };
    return between_feature(left, op, &name, op2, right);
  }
  let flipped = match op {
    Op::Lt => Op::Gt,
    Op::Le => Op::Ge,
    Op::Gt => Op::Lt,
    Op::Ge => Op::Le,
    Op::Eq => Op::Eq,
  };
  range_feature(&name, flipped, left)
}

fn plain_feature(name: &str, value: Value) -> Query {
  let name = name.to_ascii_lowercase();
  if let Some(rest) = name.strip_prefix("-webkit-min-") {
    return range_feature(rest, Op::Ge, value);
  }
  if let Some(rest) = name.strip_prefix("-webkit-max-") {
    return range_feature(rest, Op::Le, value);
  }
  if let Some(rest) = name.strip_prefix("min-") {
    return range_feature(rest, Op::Ge, value);
  }
  if let Some(rest) = name.strip_prefix("max-") {
    return range_feature(rest, Op::Le, value);
  }
  range_feature(&name, Op::Eq, value)
}

fn range_feature(name: &str, op: Op, value: Value) -> Query {
  let name = name.to_ascii_lowercase();
  let Some(test) = numeric_test(&name, op, value.clone()) else {
    if op == Op::Eq {
      return discrete_eq(&name, &value);
    }
    return Query::False;
  };
  match name.as_str() {
    "width" => Query::Width(test),
    "height" => Query::Height(test),
    "resolution" | "device-pixel-ratio" | "-webkit-device-pixel-ratio" => {
      Query::Resolution(test)
    }
    _ => Query::False,
  }
}

fn between_feature(
  left: Value,
  op: Op,
  name: &str,
  op2: Op,
  right: Value,
) -> Query {
  // `100px <= width <= 400px` and `400px >= width >= 100px`.
  let (lower_op, lower_value, upper_op, upper_value) = match (op, op2) {
    (Op::Lt | Op::Le, Op::Lt | Op::Le) => (op, left, op2, right),
    (Op::Gt | Op::Ge, Op::Gt | Op::Ge) => (op2, right, op, left),
    _ => return Query::False,
  };
  let lower = Bound {
    value: match scalar(name, lower_value) {
      Some(value) => value,
      None => return Query::False,
    },
    inclusive: matches!(lower_op, Op::Le | Op::Ge),
  };
  let upper = Bound {
    value: match scalar(name, upper_value) {
      Some(value) => value,
      None => return Query::False,
    },
    inclusive: matches!(upper_op, Op::Le | Op::Ge),
  };
  let test = Numeric {
    lower: Some(lower),
    upper: Some(upper),
  };
  match name.to_ascii_lowercase().as_str() {
    "width" => Query::Width(test),
    "height" => Query::Height(test),
    "resolution" | "device-pixel-ratio" | "-webkit-device-pixel-ratio" => {
      Query::Resolution(test)
    }
    _ => Query::False,
  }
}

fn numeric_test(name: &str, op: Op, value: Value) -> Option<Numeric> {
  let value = scalar(name, value)?;
  let bound = Bound {
    value,
    inclusive: matches!(op, Op::Le | Op::Ge | Op::Eq),
  };
  let test = match op {
    Op::Gt | Op::Ge => Numeric {
      lower: Some(bound),
      upper: None,
    },
    Op::Lt | Op::Le => Numeric {
      lower: None,
      upper: Some(bound),
    },
    Op::Eq => Numeric {
      lower: Some(bound),
      upper: Some(bound),
    },
  };
  Some(test)
}

fn scalar(name: &str, value: Value) -> Option<f64> {
  let name = name.to_ascii_lowercase();
  let resolution = name == "resolution"
    || name == "device-pixel-ratio"
    || name == "-webkit-device-pixel-ratio"
    || name.ends_with("-resolution")
    || name.ends_with("device-pixel-ratio");
  if resolution {
    return value.as_dppx(name.contains("device-pixel-ratio"));
  }
  value.as_px()
}

fn discrete_eq(name: &str, value: &Value) -> Query {
  let Some(ident) = value.as_ident() else {
    return Query::False;
  };
  let ident = ident.to_ascii_lowercase();
  match name {
    "orientation" => match ident.as_str() {
      "portrait" => Query::Portrait(true),
      "landscape" => Query::Portrait(false),
      _ => Query::False,
    },
    "color-gamut" => match ident.as_str() {
      "srgb" => Query::Gamut(0),
      "p3" => Query::Gamut(1),
      "rec2020" => Query::Gamut(2),
      _ => Query::False,
    },
    "prefers-color-scheme" => match ident.as_str() {
      "dark" => Query::Dark(true),
      "light" => Query::Dark(false),
      _ => Query::False,
    },
    "prefers-reduced-motion" => match ident.as_str() {
      "reduce" => Query::ReduceMotion(true),
      "no-preference" => Query::ReduceMotion(false),
      _ => Query::False,
    },
    "prefers-contrast" => match ident.as_str() {
      "no-preference" => Query::Contrast(0),
      "less" => Query::Contrast(1),
      "more" => Query::Contrast(2),
      "custom" => Query::Contrast(3),
      _ => Query::False,
    },
    "pointer" => match ident.as_str() {
      "none" => Query::Pointer(0),
      "coarse" => Query::Pointer(1),
      "fine" => Query::Pointer(2),
      _ => Query::False,
    },
    _ => Query::False,
  }
}

fn boolean_feature(name: &str) -> Query {
  match name.to_ascii_lowercase().as_str() {
    "width" => Query::BoolWidth,
    "height" => Query::BoolHeight,
    "resolution" | "device-pixel-ratio" | "-webkit-device-pixel-ratio" => {
      Query::BoolResolution
    }
    "pointer" => Query::BoolPointer,
    "prefers-color-scheme" => Query::BoolScheme,
    "prefers-reduced-motion" => Query::BoolMotion,
    "prefers-contrast" => Query::BoolContrast,
    _ => Query::False,
  }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Op {
  Lt,
  Le,
  Gt,
  Ge,
  Eq,
}

#[derive(Clone)]
enum Value {
  Px(f64),
  Dppx(f64),
  Bare(f64),
  Ident(String),
}

impl Value {
  fn as_px(&self) -> Option<f64> {
    match self {
      Value::Px(value) => Some(*value),
      Value::Bare(value) if *value == 0.0 => Some(0.0),
      _ => None,
    }
  }

  fn as_dppx(&self, bare_ok: bool) -> Option<f64> {
    match self {
      Value::Dppx(value) => Some(*value),
      Value::Bare(value) if bare_ok => Some(*value),
      _ => None,
    }
  }

  fn as_ident(&self) -> Option<&str> {
    match self {
      Value::Ident(value) => Some(value),
      _ => None,
    }
  }
}

struct Parser<'a> {
  input: &'a str,
  index: usize,
}

impl<'a> Parser<'a> {
  fn new(input: &'a str) -> Self {
    Self { input, index: 0 }
  }

  fn eof(&self) -> bool {
    self.index >= self.input.len()
  }

  fn skip(&mut self) {
    while let Some(char) = self.peek() {
      if char.is_ascii_whitespace() {
        self.index += char.len_utf8();
      } else {
        break;
      }
    }
  }

  fn peek(&self) -> Option<char> {
    self.input[self.index..].chars().next()
  }

  fn starts_with(&self, char: char) -> bool {
    self.peek() == Some(char)
  }

  fn eat(&mut self, token: &str) -> bool {
    self.skip();
    if self.input[self.index..].starts_with(token) {
      self.index += token.len();
      true
    } else {
      false
    }
  }

  fn starts_keyword(&mut self, word: &str) -> bool {
    let saved = self.index;
    let matched = self.eat_keyword(word);
    self.index = saved;
    matched
  }

  fn keyword_then_paren(&mut self, word: &str) -> bool {
    let saved = self.index;
    if !self.eat_keyword(word) {
      self.index = saved;
      return false;
    }
    self.skip();
    let paren = self.starts_with('(');
    self.index = saved;
    paren
  }

  fn eat_keyword(&mut self, word: &str) -> bool {
    let saved = self.index;
    self.skip();
    match self.ident() {
      Some(ident) if ident.eq_ignore_ascii_case(word) => true,
      _ => {
        self.index = saved;
        false
      }
    }
  }

  fn ident(&mut self) -> Option<String> {
    self.skip();
    let bytes = self.input.as_bytes();
    if self.index >= bytes.len() {
      return None;
    }
    let first = bytes[self.index] as char;
    if first == '-' {
      let next = bytes.get(self.index + 1).copied().unwrap_or(0) as char;
      if next.is_ascii_digit() || next == '.' {
        return None;
      }
    } else if !first.is_ascii_alphabetic() && first != '_' {
      return None;
    }
    let start = self.index;
    self.index += 1;
    while self.index < bytes.len() {
      let char = bytes[self.index] as char;
      if char.is_ascii_alphanumeric() || char == '-' || char == '_' {
        self.index += 1;
      } else {
        break;
      }
    }
    Some(self.input[start..self.index].to_string())
  }

  fn looking_at_number(&mut self) -> bool {
    let saved = self.index;
    self.skip();
    let result = self.peek_number();
    self.index = saved;
    result
  }

  fn peek_number(&self) -> bool {
    let bytes = self.input.as_bytes();
    let mut index = self.index;
    if index < bytes.len() && (bytes[index] == b'-' || bytes[index] == b'+') {
      index += 1;
    }
    if index < bytes.len() && bytes[index] == b'.' {
      return bytes
        .get(index + 1)
        .is_some_and(|byte| byte.is_ascii_digit());
    }
    bytes.get(index).is_some_and(|byte| byte.is_ascii_digit())
  }

  fn operator(&mut self) -> Option<Op> {
    self.skip();
    if self.eat("<=") {
      Some(Op::Le)
    } else if self.eat(">=") {
      Some(Op::Ge)
    } else if self.eat("<") {
      Some(Op::Lt)
    } else if self.eat(">") {
      Some(Op::Gt)
    } else if self.eat("=") {
      Some(Op::Eq)
    } else {
      None
    }
  }

  fn value(&mut self) -> Option<Value> {
    self.skip();
    if self.looking_at_number() {
      let number = self.number()?;
      let saved = self.index;
      self.skip();
      if let Some(unit) = self.ident() {
        let px = match unit.to_ascii_lowercase().as_str() {
          "px" => Some(number),
          "em" | "rem" => Some(number * 16.0),
          _ => None,
        };
        if let Some(px) = px {
          return Some(Value::Px(px));
        }
        let dppx = match unit.to_ascii_lowercase().as_str() {
          "dppx" | "x" => Some(number),
          "dpi" => Some(number / 96.0),
          "dpcm" => Some(number * 2.54 / 96.0),
          _ => None,
        };
        if let Some(dppx) = dppx {
          return Some(Value::Dppx(dppx));
        }
        self.index = saved;
      }
      return Some(Value::Bare(number));
    }
    self.ident().map(Value::Ident)
  }

  fn number(&mut self) -> Option<f64> {
    self.skip();
    let start = self.index;
    let bytes = self.input.as_bytes();
    if self.index < bytes.len()
      && (bytes[self.index] == b'-' || bytes[self.index] == b'+')
    {
      self.index += 1;
    }
    let mut saw = false;
    while self.index < bytes.len() && bytes[self.index].is_ascii_digit() {
      saw = true;
      self.index += 1;
    }
    if self.index < bytes.len() && bytes[self.index] == b'.' {
      self.index += 1;
      while self.index < bytes.len() && bytes[self.index].is_ascii_digit() {
        saw = true;
        self.index += 1;
      }
    }
    if !saw {
      self.index = start;
      return None;
    }
    self.input[start..self.index].parse().ok()
  }
}

#[repr(C)]
struct MediaQueryList {
  base: EventTarget,
  window_id: u64,
  media: RefCell<String>,
  query: Query,
  matches: Cell<bool>,
  onchange: RefCell<Option<v8::TracedReference<v8::Function>>>,
  legacy: RefCell<Vec<v8::TracedReference<v8::Function>>>,
}

unsafe impl v8::cppgc::GarbageCollected for MediaQueryList {
  fn trace(&self, visitor: &mut v8::cppgc::Visitor) {
    v8::cppgc::GarbageCollected::trace(&self.base, visitor);
    if let Ok(handler) = self.onchange.try_borrow()
      && let Some(handler) = handler.as_ref()
    {
      visitor.trace(handler);
    }
    if let Ok(legacy) = self.legacy.try_borrow() {
      for callback in legacy.iter() {
        visitor.trace(callback);
      }
    }
  }

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"MediaQueryList"
  }
}

#[op(inherit = EventTarget)]
impl MediaQueryList {
  #[constructor]
  fn new() -> Result<Self, OpError> {
    Err(OpError::type_("Illegal constructor"))
  }

  #[getter]
  fn matches(&self) -> bool {
    snapshot(self.window_id)
      .map(|state| evaluate(&self.query, &state))
      .unwrap_or_else(|| self.matches.get())
  }

  #[getter]
  fn media(&self) -> String {
    self.media.borrow().clone()
  }

  #[getter]
  fn onchange(&self, scope: &mut v8::PinScope) -> Js {
    let stored = self.onchange.borrow();
    let Some(handler) = stored.as_ref() else {
      return Js::null(scope);
    };
    let Some(function) = traced_local(scope, handler) else {
      return Js::null(scope);
    };
    let value: v8::Local<v8::Value> = function.into();
    Js(v8::Global::new(scope, value))
  }

  #[setter]
  fn onchange(&self, scope: &mut v8::PinScope, value: v8::Local<v8::Value>) {
    if value.is_null() || value.is_undefined() {
      *self.onchange.borrow_mut() = None;
      return;
    }
    if let Ok(function) = v8::Local::<v8::Function>::try_from(value) {
      *self.onchange.borrow_mut() = Some(traced(scope, function));
    }
  }

  fn add_listener(
    &self,
    scope: &mut v8::PinScope,
    callback: v8::Local<v8::Value>,
  ) {
    let Ok(function) = v8::Local::<v8::Function>::try_from(callback) else {
      return;
    };
    let mut legacy = self.legacy.borrow_mut();
    if legacy.iter().any(|item| {
      traced_local(scope, item)
        .is_some_and(|current| current.strict_equals(function.into()))
    }) {
      return;
    }
    legacy.push(traced(scope, function));
  }

  fn remove_listener(
    &self,
    scope: &mut v8::PinScope,
    callback: v8::Local<v8::Value>,
  ) {
    let Ok(function) = v8::Local::<v8::Function>::try_from(callback) else {
      return;
    };
    self.legacy.borrow_mut().retain(|item| {
      traced_local(scope, item)
        .is_none_or(|current| !current.strict_equals(function.into()))
    });
  }
}

#[repr(C)]
struct MediaQueryListEvent {
  base: Event,
  matches: Cell<bool>,
  media: RefCell<String>,
}

unsafe impl v8::cppgc::GarbageCollected for MediaQueryListEvent {
  fn trace(&self, visitor: &mut v8::cppgc::Visitor) {
    v8::cppgc::GarbageCollected::trace(&self.base, visitor);
  }

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"MediaQueryListEvent"
  }
}

#[op(inherit = Event)]
impl MediaQueryListEvent {
  #[constructor]
  fn new() -> Result<Self, OpError> {
    Err(OpError::type_("Illegal constructor"))
  }

  #[getter]
  fn matches(&self) -> bool {
    self.matches.get()
  }

  #[getter]
  fn media(&self) -> String {
    self.media.borrow().clone()
  }
}

pub fn install(scope: &mut v8::PinScope) {
  MediaQueryList::install(scope);
  MediaQueryListEvent::install(scope);
}

pub fn open<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  window_id: u64,
  query: String,
) -> v8::Local<'s, v8::Object> {
  let parsed = parse(&query);
  let matches = snapshot(window_id)
    .map(|state| evaluate(&parsed, &state))
    .unwrap_or(false);
  let list = MediaQueryList {
    base: EventTarget::new(),
    window_id,
    media: RefCell::new(query),
    query: parsed,
    matches: Cell::new(matches),
    onchange: RefCell::new(None),
    legacy: RefCell::new(Vec::new()),
  };
  unsafe { yuzuko_core::op::wrap_instance(scope, list) }
}

#[cfg(test)]
mod tests {
  use super::{Query, Snapshot, evaluate, parse};

  fn desktop() -> Snapshot {
    Snapshot {
      width: 400.0,
      height: 200.0,
      dpr: 2.0,
      gamut: 1,
      dark: true,
      reduce_motion: false,
      contrast: 0,
      pointer: 2,
    }
  }

  fn matches(query: &str) -> bool {
    evaluate(&parse(query), &desktop())
  }

  #[test]
  fn empty_query_matches() {
    assert!(matches(""));
    assert!(matches("screen"));
    assert!(!matches("print"));
    assert!(matches("not print"));
  }

  #[test]
  fn width_ranges_and_level_4_inequalities() {
    assert!(matches("(min-width: 300px)"));
    assert!(!matches("(max-width: 100px)"));
    assert!(matches("(width >= 400px)"));
    assert!(matches("(width > 399px)"));
    assert!(!matches("(width > 400px)"));
    assert!(matches("(200px <= width <= 500px)"));
    assert!(matches("(500px >= width >= 200px)"));
    assert!(!matches("(100px <= width < 400px)"));
    assert!(matches("(min-width: 0)"));
    assert!(matches("(resolution >= 2dppx)"));
    assert!(matches("(min-resolution: 192dpi)"));
    assert!(matches("(device-pixel-ratio >= 2)"));
    assert!(matches("(-webkit-min-device-pixel-ratio: 2)"));
    assert!(!matches("(resolution < 1x)"));
  }

  #[test]
  fn discrete_features() {
    assert!(matches("(orientation: landscape)"));
    assert!(!matches("(orientation: portrait)"));
    assert!(matches("(color-gamut: srgb)"));
    assert!(matches("(color-gamut: p3)"));
    assert!(!matches("(color-gamut: rec2020)"));
    assert!(matches("(prefers-color-scheme: dark)"));
    assert!(matches("(prefers-reduced-motion: no-preference)"));
    assert!(matches("(prefers-contrast: no-preference)"));
    assert!(matches("(pointer: fine)"));
    assert!(!matches("(pointer: coarse)"));
    assert!(matches("(pointer)"));
    assert!(!matches("(orientation > landscape)"));
    assert!(!matches("(min-orientation: landscape)"));
  }

  #[test]
  fn combinators_and_invalid_queries() {
    assert!(matches("(min-width: 100px) and (orientation: landscape)"));
    assert!(matches("(orientation: portrait), (pointer: fine)"));
    assert!(!matches("(width < 10px) or (orientation: portrait)"));
    assert!(!matches("(((("));
    assert!(!matches("(unknown-feature: 1)"));
    assert!(!matches_query(&Query::False));
  }

  fn matches_query(query: &Query) -> bool {
    evaluate(query, &desktop())
  }
}
