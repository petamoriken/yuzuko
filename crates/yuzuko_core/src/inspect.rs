//! Object printer for `console` and the REPL.
//!
//! The layout follows Deno's console inspector. It does not call a
//! user inspection hook. Non-enumerable symbol properties are omitted.
//! A host class instance prints that class's attributes. A getter on a
//! user class is printed too. A getter that throws prints as `[Getter]`.
//! `console` leaves the text uncolored. A REPL completion can paint it.
//! A Temporal instance prints as `ToString`. The REPL paints that text cyan.

use std::sync::Mutex;

use unicode_width::UnicodeWidthStr;

use crate::color::{self, Ink};

const DEPTH: i32 = 4;
const COMPACT: i32 = 3;
const BREAK_LENGTH: usize = 80;
const MAX_ARRAY_LENGTH: usize = 100;
const MAX_STRING_LENGTH: usize = 10_000;
const ITERABLE_LIMIT: usize = 100;
const MIN_LINE_LENGTH: usize = 16;

const OBJECT_TYPE: u8 = 0;
const ARRAY_TYPE: u8 = 1;
const ARRAY_EXTRAS_TYPE: u8 = 2;

const QUOTES: [&str; 3] = ["\"", "'", "`"];

/// Classes whose instances print as `ToString`.
const TEMPORAL_TYPES: [&str; 8] = [
  "Instant",
  "ZonedDateTime",
  "PlainDate",
  "PlainTime",
  "PlainDateTime",
  "PlainYearMonth",
  "PlainMonthDay",
  "Duration",
];

/// Attributes `console` prints for a host class, in that order.
///
/// `#[op(inspect = [...])]` registers [`InspectFields::List`].
/// `#[op(inspect_after = [...])]` registers [`InspectFields::After`].
/// The class `install` function calls [`register_inspect`].
#[derive(Clone, Copy)]
pub enum InspectFields {
  /// These names replace any ancestor list.
  List(&'static [&'static str]),
  /// These names follow the nearest ancestor list.
  After(&'static [&'static str]),
}

static REGISTRY: Mutex<Vec<(&'static str, InspectFields)>> =
  Mutex::new(Vec::new());

/// Records the attributes `console` prints for `name`.
///
/// The class macro calls this from `install`. A later registration
/// for the same name replaces the earlier one.
pub fn register_inspect(name: &'static str, fields: InspectFields) {
  let mut registry = REGISTRY.lock().expect("inspect fields");
  if let Some(slot) = registry.iter_mut().find(|(key, _)| *key == name) {
    slot.1 = fields;
    return;
  }
  registry.push((name, fields));
}

fn registered(name: &str) -> Option<InspectFields> {
  REGISTRY
    .lock()
    .expect("inspect fields")
    .iter()
    .find(|(key, _)| *key == name)
    .map(|(_, fields)| *fields)
}

/// `URL` is formatted by [`format_url`] and is not a generic interface.
fn url_field_names() -> &'static [&'static str] {
  match registered("URL") {
    Some(InspectFields::List(fields) | InspectFields::After(fields)) => fields,
    None => &[],
  }
}

fn listed_registration(name: &str) -> Option<InspectFields> {
  if name == "URL" {
    return None;
  }
  registered(name)
}

fn is_listed_name(name: &str) -> bool {
  listed_registration(name).is_some()
}

pub(crate) struct Thrown(pub v8::Global<v8::Value>);

pub(crate) fn rethrow(scope: &mut v8::PinScope, thrown: Thrown) {
  let thrown = v8::Local::new(scope, thrown.0);
  scope.throw_exception(thrown);
}

pub(crate) fn format_value(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<String, Thrown> {
  format_value_colored(scope, value, false)
}

pub(crate) fn format_value_colored(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
  colors: bool,
) -> Result<String, Thrown> {
  let mut ctx = Ctx {
    colors,
    ..Ctx::default()
  };
  format_with(scope, &mut ctx, value, 0)
}

pub(crate) fn format_callback_args(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
  start: i32,
) -> Result<String, Thrown> {
  let mut values = Vec::new();
  for index in start..args.length() {
    values.push(args.get(index));
  }
  let mut ctx = Ctx::default();
  format_args(scope, &mut ctx, &values)
}

struct Ctx {
  indent: i32,
  current_depth: i32,
  seen: Vec<v8::Global<v8::Value>>,
  circular: Vec<(v8::Global<v8::Value>, usize)>,
  circular_set: bool,
  circular_error_message: Option<String>,
  /// The value being printed is `Constructor.prototype`. Inherited
  /// accessors are shown and not called.
  prototype_object: bool,
  /// REPL completions set this. `console` leaves it false.
  colors: bool,
}

impl Default for Ctx {
  fn default() -> Self {
    Self {
      indent: 0,
      current_depth: 0,
      seen: Vec::new(),
      circular: Vec::new(),
      circular_set: false,
      circular_error_message: None,
      prototype_object: false,
      colors: false,
    }
  }
}

enum Key {
  Name(String),
  Symbol(v8::Global<v8::Symbol>),
}

#[derive(Clone, Copy)]
enum Kind {
  None,
  Array,
  Set,
  Map,
  TypedArray,
  MapIterator,
  SetIterator,
  Promise,
  Weak,
  Namespace,
  ArrayBuffer,
}

struct OwnedDesc {
  value: Option<v8::Global<v8::Value>>,
  getter: bool,
  setter: bool,
  enumerable: bool,
}

fn type_error(scope: &mut v8::PinScope, message: &str) -> Thrown {
  let message = v8::String::new(scope, message).unwrap();
  let error = v8::Exception::type_error(scope, message);
  Thrown(v8::Global::new(scope, error))
}

/// `exception()` exists on a try/catch scope. A plain handle scope has no
/// pending-exception accessor, so the caught value is read at the call.
macro_rules! stash_exception {
  ($tc:ident) => {{
    if let Some(exception) = $tc.exception() {
      Thrown(v8::Global::new($tc, exception))
    } else {
      let message = v8::String::new($tc, "unknown exception").unwrap();
      let error = v8::Exception::error($tc, message);
      Thrown(v8::Global::new($tc, error))
    }
  }};
}

fn v8_str<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  text: &str,
) -> v8::Local<'s, v8::String> {
  v8::String::new(scope, text)
    .unwrap_or_else(|| v8::String::new(scope, "").unwrap())
}

fn spaces(count: i32) -> String {
  " ".repeat(count.max(0) as usize)
}

fn utf16_length(text: &str) -> usize {
  text.chars().map(char::len_utf16).sum()
}

fn slice_utf16(text: &str, end: usize) -> String {
  let mut out = String::new();
  let mut index = 0usize;
  for ch in text.chars() {
    let len = ch.len_utf16();
    if index >= end {
      break;
    }
    if index + len <= end {
      out.push(ch);
    }
    index += len;
  }
  out
}

fn display_width(text: &str) -> usize {
  UnicodeWidthStr::width(color::strip_sgr(text).as_ref())
}

/// UTF-16 length of the visible text. Color bytes do not count.
fn visible_utf16(text: &str) -> usize {
  utf16_length(color::strip_sgr(text).as_ref())
}

fn meta_escape(ch: char) -> Option<&'static str> {
  Some(match ch as u32 {
    0x00 => "\\x00",
    0x01 => "\\x01",
    0x02 => "\\x02",
    0x03 => "\\x03",
    0x04 => "\\x04",
    0x05 => "\\x05",
    0x06 => "\\x06",
    0x07 => "\\x07",
    0x08 => "\\b",
    0x09 => "\\t",
    0x0a => "\\n",
    0x0b => "\\x0B",
    0x0c => "\\f",
    0x0d => "\\r",
    0x0e => "\\x0E",
    0x0f => "\\x0F",
    0x10 => "\\x10",
    0x11 => "\\x11",
    0x12 => "\\x12",
    0x13 => "\\x13",
    0x14 => "\\x14",
    0x15 => "\\x15",
    0x16 => "\\x16",
    0x17 => "\\x17",
    0x18 => "\\x18",
    0x19 => "\\x19",
    0x1a => "\\x1A",
    0x1b => "\\x1B",
    0x1c => "\\x1C",
    0x1d => "\\x1D",
    0x1e => "\\x1E",
    0x1f => "\\x1F",
    0x27 => "\\'",
    0x5c => "\\\\",
    0x7f => "\\x7F",
    0x80 => "\\x80",
    0x81 => "\\x81",
    0x82 => "\\x82",
    0x83 => "\\x83",
    0x84 => "\\x84",
    0x85 => "\\x85",
    0x86 => "\\x86",
    0x87 => "\\x87",
    0x88 => "\\x88",
    0x89 => "\\x89",
    0x8a => "\\x8A",
    0x8b => "\\x8B",
    0x8c => "\\x8C",
    0x8d => "\\x8D",
    0x8e => "\\x8E",
    0x8f => "\\x8F",
    0x90 => "\\x90",
    0x91 => "\\x91",
    0x92 => "\\x92",
    0x93 => "\\x93",
    0x94 => "\\x94",
    0x95 => "\\x95",
    0x96 => "\\x96",
    0x97 => "\\x97",
    0x98 => "\\x98",
    0x99 => "\\x99",
    0x9a => "\\x9A",
    0x9b => "\\x9B",
    0x9c => "\\x9C",
    0x9d => "\\x9D",
    0x9e => "\\x9E",
    0x9f => "\\x9F",
    _ => return None,
  })
}

fn escape_meta_chars(text: &str) -> String {
  let mut out = String::with_capacity(text.len());
  for ch in text.chars() {
    let code = ch as u32;
    if code <= 0x1f
      || code == 0x27
      || code == 0x5c
      || (0x7f..=0x9f).contains(&code)
    {
      if let Some(escaped) = meta_escape(ch) {
        out.push_str(escaped);
        continue;
      }
    }
    out.push(ch);
  }
  out
}

fn replace_escape_sequences(text: &str) -> String {
  let mut out = String::with_capacity(text.len());
  for ch in text.chars() {
    let escaped = match ch {
      '\u{8}' => Some("\\b"),
      '\u{c}' => Some("\\f"),
      '\n' => Some("\\n"),
      '\r' => Some("\\r"),
      '\t' => Some("\\t"),
      '\u{b}' => Some("\\v"),
      _ => None,
    };
    if let Some(escaped) = escaped {
      out.push_str(escaped);
      continue;
    }
    let code = ch as u32;
    if code <= 0x1f || (0x7f..=0x9f).contains(&code) {
      out.push_str(&format!("\\x{code:02x}"));
      continue;
    }
    out.push(ch);
  }
  out
}

fn quote_string(text: &str) -> String {
  let quote = QUOTES
    .iter()
    .find(|quote| !text.contains(*quote))
    .copied()
    .unwrap_or(QUOTES[0]);
  let quote_char = quote.chars().next();
  let mut escaped = String::with_capacity(text.len());
  for ch in text.chars() {
    if Some(ch) == quote_char || ch == '\\' {
      escaped.push('\\');
    }
    escaped.push(ch);
  }
  let escaped = replace_escape_sequences(&escaped);
  format!("{quote}{escaped}{quote}")
}

fn symbol_description_plain(description: &str) -> bool {
  let mut chars = description.chars();
  match chars.next() {
    Some(ch) if ch.is_ascii_alphabetic() || ch == '_' => {}
    _ => return false,
  }
  chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '.')
}

fn is_identifier_like(key: &str) -> bool {
  let mut chars = key.chars();
  match chars.next() {
    Some(ch) if ch.is_ascii_alphabetic() || ch == '_' => {}
    _ => return false,
  }
  chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

fn is_canonical_index(key: &str) -> bool {
  if key == "0" {
    return true;
  }
  let mut chars = key.chars();
  match chars.next() {
    Some(ch) if ('1'..='9').contains(&ch) => {}
    _ => return false,
  }
  chars.all(|ch| ch.is_ascii_digit())
}

fn same(
  scope: &mut v8::PinScope,
  global: &v8::Global<v8::Value>,
  value: v8::Local<v8::Value>,
) -> bool {
  v8::Local::new(scope, global).strict_equals(value)
}

fn seen_contains(
  scope: &mut v8::PinScope,
  ctx: &Ctx,
  value: v8::Local<v8::Value>,
) -> bool {
  ctx.seen.iter().any(|seen| same(scope, seen, value))
}

fn try_get<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  object: v8::Local<v8::Object>,
  key: v8::Local<v8::Value>,
) -> Result<v8::Local<'s, v8::Value>, Thrown> {
  let result = {
    v8::tc_scope!(let tc, scope);
    match object.get(tc, key) {
      Some(value) => Ok(v8::Global::new(tc, value)),
      None => Err(stash_exception!(tc)),
    }
  };
  result.map(|value| v8::Local::new(scope, value))
}

fn try_get_index<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  object: v8::Local<v8::Object>,
  index: u32,
) -> Result<v8::Local<'s, v8::Value>, Thrown> {
  let result = {
    v8::tc_scope!(let tc, scope);
    match object.get_index(tc, index) {
      Some(value) => Ok(v8::Global::new(tc, value)),
      None => Err(stash_exception!(tc)),
    }
  };
  result.map(|value| v8::Local::new(scope, value))
}

fn try_call<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  function: v8::Local<v8::Function>,
  receiver: v8::Local<v8::Value>,
  args: &[v8::Local<v8::Value>],
) -> Result<v8::Local<'s, v8::Value>, Thrown> {
  let result = {
    v8::tc_scope!(let tc, scope);
    match function.call(tc, receiver, args) {
      Some(value) => Ok(v8::Global::new(tc, value)),
      None => Err(stash_exception!(tc)),
    }
  };
  result.map(|value| v8::Local::new(scope, value))
}

fn try_to_string(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<String, Thrown> {
  let result = {
    v8::tc_scope!(let tc, scope);
    match value.to_string(tc) {
      Some(text) => Ok(text.to_rust_string_lossy(tc)),
      None => Err(stash_exception!(tc)),
    }
  };
  result
}

fn rust_primitive(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> String {
  if let Ok(text) = v8::Local::<v8::String>::try_from(value) {
    return text.to_rust_string_lossy(scope);
  }
  value.to_rust_string_lossy(scope)
}

fn as_object<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  value: v8::Local<'_, v8::Value>,
) -> Result<v8::Local<'s, v8::Object>, Thrown> {
  let object = match v8::Local::<v8::Object>::try_from(value) {
    Ok(object) => object,
    Err(_) => {
      return Err(type_error(
        scope,
        "Cannot inspect a value that is not an object.",
      ));
    }
  };
  Ok(v8::Local::new(scope, object))
}

fn as_function<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  value: v8::Local<'_, v8::Value>,
) -> Result<v8::Local<'s, v8::Function>, Thrown> {
  let function = match v8::Local::<v8::Function>::try_from(value) {
    Ok(function) => function,
    Err(_) => {
      return Err(type_error(
        scope,
        "Cannot inspect a value that is not a function.",
      ));
    }
  };
  Ok(v8::Local::new(scope, function))
}

fn prototype_method<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  ctor: &str,
  method: &str,
) -> Result<v8::Local<'s, v8::Function>, Thrown> {
  let global = crate::js::global(scope);
  let ctor_key = v8_str(scope, ctor);
  let ctor_value = try_get(scope, global, ctor_key.into())?;
  let ctor_object = as_object(scope, ctor_value)?;
  let prototype_key = v8_str(scope, "prototype");
  let prototype = try_get(scope, ctor_object, prototype_key.into())?;
  let prototype = as_object(scope, prototype)?;
  let method_key = v8_str(scope, method);
  let method = try_get(scope, prototype, method_key.into())?;
  as_function(scope, method)
}

fn prototype_call<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  ctor: &str,
  method: &str,
  receiver: v8::Local<v8::Value>,
) -> Result<v8::Local<'s, v8::Value>, Thrown> {
  let function = prototype_method(scope, ctor, method)?;
  try_call(scope, function, receiver, &[])
}

fn global_function(
  scope: &mut v8::PinScope,
  name: &str,
) -> Option<v8::Global<v8::Function>> {
  let global = crate::js::global(scope);
  let key = v8_str(scope, name);
  let found = {
    v8::tc_scope!(let tc, scope);
    match global.get(tc, key.into()) {
      Some(value) => v8::Local::<v8::Function>::try_from(value)
        .ok()
        .map(|function| v8::Global::new(tc, function)),
      None => None,
    }
  };
  found
}

fn instance_of_name(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
  name: &str,
) -> bool {
  let Some(ctor) = global_function(scope, name) else {
    return false;
  };
  let ctor = v8::Local::new(scope, ctor);
  let result = {
    v8::tc_scope!(let tc, scope);
    let object: v8::Local<v8::Object> = ctor.into();
    value.instance_of(tc, object)
  };
  matches!(result, Some(true))
}

fn number_from(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<f64, Thrown> {
  let result = {
    v8::tc_scope!(let tc, scope);
    match value.number_value(tc) {
      Some(number) => Ok(number),
      None => Err(stash_exception!(tc)),
    }
  };
  result
}

fn number_display(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<String, Thrown> {
  let number = number_from(scope, value)?;
  if number == 0.0 && number.is_sign_negative() {
    return Ok("-0".to_string());
  }
  let printed = v8::Number::new(scope, number);
  Ok(printed.to_rust_string_lossy(scope))
}

fn format_bigint(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<String, Thrown> {
  Ok(format!("{}n", try_to_string(scope, value)?))
}

fn symbol_text(
  scope: &mut v8::PinScope,
  symbol: v8::Local<v8::Symbol>,
) -> String {
  let description = symbol.description(scope);
  if description.is_undefined() {
    "Symbol()".to_string()
  } else {
    format!("Symbol({})", description.to_rust_string_lossy(scope))
  }
}

fn maybe_quote_symbol(
  scope: &mut v8::PinScope,
  symbol: v8::Local<v8::Symbol>,
) -> String {
  let description = symbol.description(scope);
  if description.is_undefined() {
    return symbol_text(scope, symbol);
  }
  let text = description.to_rust_string_lossy(scope);
  if symbol_description_plain(&text) {
    return symbol_text(scope, symbol);
  }
  format!("Symbol({})", quote_string(&text))
}

fn format_primitive(
  scope: &mut v8::PinScope,
  ctx: &Ctx,
  value: v8::Local<v8::Value>,
  color: bool,
) -> Result<String, Thrown> {
  if value.is_string() {
    return format_string(scope, ctx, value, color);
  }
  if value.is_number() {
    let text = number_display(scope, value)?;
    return Ok(color::paint(Ink::Number, &text, color));
  }
  if value.is_big_int() {
    let text = format_bigint(scope, value)?;
    return Ok(color::paint(Ink::Number, &text, color));
  }
  if value.is_boolean() {
    let text = if value.is_true() { "true" } else { "false" };
    return Ok(color::paint(Ink::Number, text, color));
  }
  if value.is_undefined() {
    return Ok(color::paint(Ink::Undefined, "undefined", color));
  }
  if value.is_null() {
    return Ok(color::paint(Ink::Null, "null", color));
  }
  let symbol = v8::Local::<v8::Symbol>::try_from(value)
    .map_err(|_| type_error(scope, "Cannot inspect this value."))?;
  Ok(color::paint(
    Ink::String,
    &maybe_quote_symbol(scope, symbol),
    color,
  ))
}

fn format_string(
  scope: &mut v8::PinScope,
  ctx: &Ctx,
  value: v8::Local<v8::Value>,
  color: bool,
) -> Result<String, Thrown> {
  let text = v8::Local::<v8::String>::try_from(value)
    .map(|text| text.to_rust_string_lossy(scope))
    .unwrap_or_default();
  let utf16_len = v8::Local::<v8::String>::try_from(value)
    .map(|text| text.length())
    .unwrap_or_else(|_| utf16_length(&text));
  let mut body = text;
  let mut trailer = String::new();
  if utf16_len > MAX_STRING_LENGTH {
    let remaining = utf16_len - MAX_STRING_LENGTH;
    body = slice_utf16(&body, MAX_STRING_LENGTH);
    trailer = format!(
      "... {remaining} more character{}",
      if remaining > 1 { "s" } else { "" }
    );
  }
  let body_len = utf16_length(&body);
  if body_len > MIN_LINE_LENGTH
    && body_len > BREAK_LENGTH.saturating_sub(ctx.indent.max(0) as usize + 4)
  {
    let mut parts = Vec::new();
    let mut current = String::new();
    for ch in body.chars() {
      current.push(ch);
      if ch == '\n' {
        parts.push(std::mem::take(&mut current));
      }
    }
    if !current.is_empty() || parts.is_empty() {
      parts.push(current);
    }
    let quoted: Vec<String> = parts
      .iter()
      .map(|line| color::paint(Ink::String, &quote_string(line), color))
      .collect();
    let joiner = format!(" +\n{}", spaces(ctx.indent + 2));
    return Ok(quoted.join(&joiner) + &trailer);
  }
  let quoted = color::paint(Ink::String, &quote_string(&body), color);
  Ok(quoted + &trailer)
}

fn format_with(
  scope: &mut v8::PinScope,
  ctx: &mut Ctx,
  value: v8::Local<v8::Value>,
  recurse: i32,
) -> Result<String, Thrown> {
  if value.is_null() {
    return Ok(color::paint(Ink::Null, "null", ctx.colors));
  }
  if !value.is_object() && !value.is_function() {
    return format_primitive(scope, ctx, value, ctx.colors);
  }
  let value = unwrap_proxy(scope, value)?;
  // A prototype fails `instanceof`, so it keeps the ordinary object layout.
  if is_temporal(scope, value)? {
    let text = try_to_string(scope, value)?;
    return Ok(color::paint(Ink::Temporal, &text, ctx.colors));
  }
  if is_web_record(scope, value) {
    if seen_contains(scope, ctx, value) {
      return Ok(circular_marker(scope, ctx, value));
    }
    return format_web(scope, ctx, value);
  }
  if seen_contains(scope, ctx, value) {
    return Ok(circular_marker(scope, ctx, value));
  }
  format_raw(scope, ctx, value, recurse)
}

fn unwrap_proxy<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  value: v8::Local<'_, v8::Value>,
) -> Result<v8::Local<'s, v8::Value>, Thrown> {
  let mut value = v8::Local::new(scope, value);
  for _ in 0..1024 {
    let Ok(proxy) = v8::Local::<v8::Proxy>::try_from(value) else {
      return Ok(value);
    };
    if proxy.is_revoked() {
      return Err(type_error(
        scope,
        "Cannot inspect a proxy that has been revoked.",
      ));
    }
    let target = proxy.get_target(scope);
    if target.is_null() {
      return Err(type_error(
        scope,
        "Cannot inspect a proxy that has been revoked.",
      ));
    }
    value = target;
  }
  Ok(value)
}

fn circular_marker(
  scope: &mut v8::PinScope,
  ctx: &mut Ctx,
  value: v8::Local<v8::Value>,
) -> String {
  let index = if !ctx.circular_set {
    ctx.circular_set = true;
    ctx.circular.push((v8::Global::new(scope, value), 1));
    1
  } else if let Some((_, index)) = ctx
    .circular
    .iter()
    .find(|(seen, _)| same(scope, seen, value))
  {
    *index
  } else {
    let index = ctx.circular.len() + 1;
    ctx.circular.push((v8::Global::new(scope, value), index));
    index
  };
  color::paint(Ink::Special, &format!("[Circular *{index}]"), ctx.colors)
}

fn apply_ref(
  scope: &mut v8::PinScope,
  ctx: &Ctx,
  value: v8::Local<v8::Value>,
  base: &str,
) -> String {
  if !ctx.circular_set {
    return base.to_string();
  }
  let Some((_, index)) = ctx
    .circular
    .iter()
    .find(|(seen, _)| same(scope, seen, value))
  else {
    return base.to_string();
  };
  let reference =
    color::paint(Ink::Special, &format!("<ref *{index}>"), ctx.colors);
  if base.is_empty() {
    reference
  } else {
    format!("{reference} {base}")
  }
}

fn is_temporal(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<bool, Thrown> {
  let global = crate::js::global(scope);
  let key = v8_str(scope, "Temporal");
  let temporal = try_get(scope, global, key.into())?;
  let Ok(temporal) = v8::Local::<v8::Object>::try_from(temporal) else {
    return Ok(false);
  };
  for name in TEMPORAL_TYPES {
    let key = v8_str(scope, name);
    let ctor = try_get(scope, temporal, key.into())?;
    let Ok(ctor) = v8::Local::<v8::Object>::try_from(ctor) else {
      continue;
    };
    let hit = {
      v8::tc_scope!(let tc, scope);
      match value.instance_of(tc, ctor) {
        Some(hit) => Ok(hit),
        None => Err(stash_exception!(tc)),
      }
    }?;
    if hit {
      return Ok(true);
    }
  }
  Ok(false)
}

fn is_web_record(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> bool {
  instance_of_name(scope, value, "URL")
    || instance_of_name(scope, value, "Headers")
    || instance_of_name(scope, value, "URLSearchParams")
}

fn format_web(
  scope: &mut v8::PinScope,
  ctx: &mut Ctx,
  value: v8::Local<v8::Value>,
) -> Result<String, Thrown> {
  ctx.seen.push(v8::Global::new(scope, value));
  let indent = ctx.indent;
  let formatted = if instance_of_name(scope, value, "URL") {
    format_url(scope, ctx, value)
  } else if instance_of_name(scope, value, "Headers") {
    format_headers(scope, ctx, value)
  } else {
    format_search_params(scope, ctx, value)
  };
  ctx.indent = indent;
  ctx.seen.pop();
  let formatted = formatted?;
  Ok(apply_ref(scope, ctx, value, &formatted))
}

fn constructor_label(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
  fallback: &str,
) -> Result<String, Thrown> {
  match constructor_name(scope, value)? {
    Some(name) if !name.is_empty() => Ok(name),
    _ => Ok(fallback.to_string()),
  }
}

fn prefix_braces(name: &str, body: &str) -> String {
  format!("{name} {body}")
}

fn format_url(
  scope: &mut v8::PinScope,
  ctx: &mut Ctx,
  value: v8::Local<v8::Value>,
) -> Result<String, Thrown> {
  let object = as_object(scope, value)?;
  let names = url_field_names();
  let mut fields = Vec::with_capacity(names.len());
  for name in names.iter().copied() {
    let key = v8_str(scope, name);
    let field = try_get(scope, object, key.into())?;
    fields.push((name, v8::Global::new(scope, field)));
  }
  let synthetic = v8::Object::new(scope);
  for (name, field) in fields {
    let key = v8_str(scope, name);
    let field = v8::Local::new(scope, field);
    synthetic.set(scope, key.into(), field);
  }
  let body = format_with(scope, ctx, synthetic.into(), 0)?;
  let name = constructor_label(scope, value, "URL")?;
  Ok(prefix_braces(&name, &body))
}

fn iterate_entries(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
) -> Result<Vec<(v8::Global<v8::Value>, v8::Global<v8::Value>)>, Thrown> {
  let entries_key = v8_str(scope, "entries");
  let entries = try_get(scope, object, entries_key.into())?;
  let entries = as_function(scope, entries)?;
  let iterator = try_call(scope, entries, object.into(), &[])?;
  let iterator = as_object(scope, iterator)?;
  let next_key = v8_str(scope, "next");
  let next = try_get(scope, iterator, next_key.into())?;
  let next = as_function(scope, next)?;
  let mut pairs = Vec::new();
  for _ in 0..1_000_000 {
    let step = try_call(scope, next, iterator.into(), &[])?;
    let step = as_object(scope, step)?;
    let done_key = v8_str(scope, "done");
    let done = try_get(scope, step, done_key.into())?;
    if done.boolean_value(scope) {
      break;
    }
    let value_key = v8_str(scope, "value");
    let pair = try_get(scope, step, value_key.into())?;
    let pair = as_object(scope, pair)?;
    let key = try_get_index(scope, pair, 0)?;
    let item = try_get_index(scope, pair, 1)?;
    pairs.push((v8::Global::new(scope, key), v8::Global::new(scope, item)));
  }
  Ok(pairs)
}

fn format_headers(
  scope: &mut v8::PinScope,
  ctx: &mut Ctx,
  value: v8::Local<v8::Value>,
) -> Result<String, Thrown> {
  let object = as_object(scope, value)?;
  let pairs = iterate_entries(scope, object)?;
  let mut grouped: Vec<(String, Vec<v8::Global<v8::Value>>)> = Vec::new();
  for (key, item) in pairs {
    let key = v8::Local::new(scope, key);
    let name = try_to_string(scope, key)?;
    if let Some((_, values)) =
      grouped.iter_mut().find(|(existing, _)| existing == &name)
    {
      values.push(item);
    } else {
      grouped.push((name, vec![item]));
    }
  }
  grouped
    .sort_by(|left, right| left.0.encode_utf16().cmp(right.0.encode_utf16()));
  let synthetic = v8::Object::new(scope);
  for (name, values) in grouped {
    let stored = if values.len() == 1 {
      v8::Local::new(scope, &values[0])
    } else {
      let mut parts = Vec::new();
      for value in &values {
        let value = v8::Local::new(scope, value);
        parts.push(try_to_string(scope, value)?);
      }
      v8_str(scope, &parts.join(", ")).into()
    };
    let key = v8_str(scope, &name);
    synthetic.set(scope, key.into(), stored);
  }
  let body = format_with(scope, ctx, synthetic.into(), 0)?;
  let name = constructor_label(scope, value, "Headers")?;
  Ok(prefix_braces(&name, &body))
}

fn format_search_params(
  scope: &mut v8::PinScope,
  ctx: &mut Ctx,
  value: v8::Local<v8::Value>,
) -> Result<String, Thrown> {
  let object = as_object(scope, value)?;
  let pairs = iterate_entries(scope, object)?;
  let name = constructor_label(scope, value, "URLSearchParams")?;
  if pairs.is_empty() {
    return Ok(format!("{name} {{}}"));
  }
  let mut printed = Vec::with_capacity(pairs.len());
  for (key, item) in pairs {
    let key = v8::Local::new(scope, key);
    let item = v8::Local::new(scope, item);
    let key = format_with(scope, ctx, key, 0)?;
    let item = format_with(scope, ctx, item, 0)?;
    printed.push(format!("{key} => {item}"));
  }
  let body = printed.join(", ");
  let one = format!("{name} {{ {body} }}");
  if visible_utf16(&one) <= BREAK_LENGTH {
    return Ok(one);
  }
  Ok(format!("{name} {{\n  {} }}", printed.join(",\n  ")))
}

struct InterfaceKeys {
  names: Vec<String>,
  listed: bool,
}

/// The closest registered prototype supplies the attribute list.
///
/// `List` replaces ancestors, so `File` does not repeat `Blob`.
/// `After` follows the nearest `List`, parent names before child
/// names, so an event subclass adds its fields after `Event`.
/// Prototypes with no registration are skipped. The index is that
/// closest prototype. A name the class does not implement is dropped
/// by the caller when the property is missing.
fn resolve_fields(
  steps: &[Option<InspectFields>],
) -> Option<(usize, Vec<&'static str>)> {
  let index = steps.iter().position(|step| step.is_some())?;
  let mut extras = Vec::new();
  for step in &steps[index..] {
    match step {
      Some(InspectFields::After(fields)) => extras.push(*fields),
      Some(InspectFields::List(fields)) => {
        let mut out = Vec::with_capacity(fields.len());
        out.extend_from_slice(fields);
        for extra in extras.iter().rev() {
          out.extend_from_slice(extra);
        }
        return Some((index, out));
      }
      None => {}
    }
  }
  let mut out = Vec::new();
  for extra in extras.iter().rev() {
    out.extend_from_slice(extra);
  }
  Some((index, out))
}

fn object_prototype(
  scope: &mut v8::PinScope,
) -> Result<Option<v8::Global<v8::Value>>, Thrown> {
  let global = crate::js::global(scope);
  let object_key = v8_str(scope, "Object");
  let object = try_get(scope, global, object_key.into())?;
  let object = as_object(scope, object)?;
  let prototype_key = v8_str(scope, "prototype");
  let prototype = try_get(scope, object, prototype_key.into())?;
  if prototype.is_null() || prototype.is_undefined() {
    return Ok(None);
  }
  Ok(Some(v8::Global::new(scope, prototype)))
}

fn prototype_chain(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
) -> Result<Vec<v8::Global<v8::Object>>, Thrown> {
  let root = object_prototype(scope)?;
  let mut chain = Vec::new();
  let mut current = v8::Global::new(scope, object);
  for _ in 0..64 {
    let object = v8::Local::new(scope, &current);
    let prototype = match object.get_prototype(scope) {
      Some(prototype) if !prototype.is_null() && !prototype.is_undefined() => {
        prototype
      }
      _ => break,
    };
    if let Some(root) = &root
      && same(scope, root, prototype)
    {
      break;
    }
    let Ok(prototype) = v8::Local::<v8::Object>::try_from(prototype) else {
      break;
    };
    if prototype.strict_equals(object.into()) {
      break;
    }
    chain.push(v8::Global::new(scope, prototype));
    current = v8::Global::new(scope, prototype);
  }
  Ok(chain)
}

/// `(constructor name, native function)`. A user class is not native.
fn prototype_kind(
  scope: &mut v8::PinScope,
  prototype: v8::Local<v8::Object>,
) -> Result<Option<(String, bool)>, Thrown> {
  let key = v8_str(scope, "constructor");
  let descriptor = {
    v8::tc_scope!(let tc, scope);
    match prototype.get_own_property_descriptor(tc, key.into()) {
      Some(value) if value.is_object() => Ok(Some(v8::Global::new(tc, value))),
      Some(_) => Ok(None),
      None => Err(stash_exception!(tc)),
    }
  }?;
  let Some(descriptor) = descriptor else {
    return Ok(None);
  };
  let descriptor = v8::Local::new(scope, descriptor);
  let descriptor = v8::Local::<v8::Object>::try_from(descriptor).unwrap();
  let Some(value) = field_global(scope, descriptor, "value")? else {
    return Ok(None);
  };
  let value = v8::Local::new(scope, value);
  if !value.is_function() {
    return Ok(None);
  }
  let function = v8::Local::<v8::Function>::try_from(value).unwrap();
  let name = function.get_name(scope).to_rust_string_lossy(scope);
  if name.is_empty() {
    return Ok(None);
  }
  let native = match prototype_call(scope, "Function", "toString", value) {
    Ok(text) => try_to_string(scope, text)
      .unwrap_or_default()
      .contains("[native code]"),
    Err(_) => false,
  };
  Ok(Some((name, native)))
}

fn has_property(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
) -> Result<bool, Thrown> {
  let key = v8_str(scope, name);
  let found = {
    v8::tc_scope!(let tc, scope);
    match object.has(tc, key.into()) {
      Some(found) => Ok(found),
      None => Err(stash_exception!(tc)),
    }
  }?;
  Ok(found)
}

fn push_unique(names: &mut Vec<String>, name: String) {
  if !names.iter().any(|existing| existing == &name) {
    names.push(name);
  }
}

fn accessor_names(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
) -> Result<Vec<String>, Thrown> {
  let keys = property_names(
    scope,
    object,
    v8::PropertyFilter::SKIP_SYMBOLS,
    v8::IndexFilter::IncludeIndices,
    v8::KeyConversionMode::ConvertToString,
  )?;
  let mut names = Vec::new();
  for key in keys {
    let Key::Name(name) = key else {
      continue;
    };
    if name == "constructor" {
      continue;
    }
    let Some(descriptor) =
      own_descriptor(scope, object, &Key::Name(name.clone()))?
    else {
      continue;
    };
    if descriptor.getter || descriptor.setter {
      names.push(name);
    }
  }
  Ok(names)
}

fn interface_keys(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<Option<InterfaceKeys>, Thrown> {
  let object = as_object(scope, value)?;
  let chain = prototype_chain(scope, object)?;
  let mut steps = Vec::with_capacity(chain.len());
  for prototype in &chain {
    let prototype = v8::Local::new(scope, prototype);
    let fields = match prototype_kind(scope, prototype)? {
      Some((name, true)) => listed_registration(&name),
      _ => None,
    };
    steps.push(fields);
  }
  if let Some((index, fields)) = resolve_fields(&steps) {
    let mut names = Vec::new();
    for field in fields {
      if has_property(scope, object, field)? {
        push_unique(&mut names, field.to_string());
      }
    }
    // Closer prototypes, parent before child, including a user subclass.
    for step in (0..index).rev() {
      let prototype = v8::Local::new(scope, &chain[step]);
      for name in accessor_names(scope, prototype)? {
        push_unique(&mut names, name);
      }
    }
    return Ok(Some(InterfaceKeys {
      names,
      listed: true,
    }));
  }
  let mut names = Vec::new();
  for prototype in chain.iter().rev() {
    let prototype = v8::Local::new(scope, prototype);
    for name in accessor_names(scope, prototype)? {
      push_unique(&mut names, name);
    }
  }
  if names.is_empty() {
    return Ok(None);
  }
  Ok(Some(InterfaceKeys {
    names,
    listed: false,
  }))
}

/// `true` when `keys` is the filtered attribute list of a known interface.
fn apply_interface_keys(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
  keys: &mut Vec<Key>,
) -> Result<bool, Thrown> {
  let Some(interface) = interface_keys(scope, value)? else {
    return Ok(false);
  };
  if interface.listed {
    *keys = interface.names.into_iter().map(Key::Name).collect();
    return Ok(true);
  }
  let mut merged: Vec<Key> =
    interface.names.into_iter().map(Key::Name).collect();
  for key in keys.drain(..) {
    let duplicate = match &key {
      Key::Name(name) => merged
        .iter()
        .any(|existing| matches!(existing, Key::Name(found) if found == name)),
      Key::Symbol(_) => false,
    };
    if !duplicate {
      merged.push(key);
    }
  }
  *keys = merged;
  Ok(false)
}

struct KeyAugment {
  listed: bool,
  /// Shown when `value` is `Constructor.prototype` and that name is not
  /// `Object`.
  label: Option<String>,
  is_prototype: bool,
}

fn push_unique_key(keys: &mut Vec<Key>, name: String) {
  if keys
    .iter()
    .any(|key| matches!(key, Key::Name(found) if found == &name))
  {
    return;
  }
  keys.push(Key::Name(name));
}

/// `Some(name)` when `object` is `name.prototype`, including an empty name.
fn prototype_object_name(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
) -> Result<Option<String>, Thrown> {
  let key = v8_str(scope, "constructor");
  let ctor = try_get(scope, object, key.into())?;
  if !ctor.is_function() {
    return Ok(None);
  }
  let function = v8::Local::<v8::Function>::try_from(ctor).unwrap();
  let name = function.get_name(scope).to_rust_string_lossy(scope);
  let ctor = v8::Local::<v8::Object>::try_from(ctor).unwrap();
  let proto_key = v8_str(scope, "prototype");
  let prototype = try_get(scope, ctor, proto_key.into())?;
  if !prototype.strict_equals(object.into()) {
    return Ok(None);
  }
  Ok(Some(name))
}

fn augment_keys(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
  keys: &mut Vec<Key>,
) -> Result<KeyAugment, Thrown> {
  let listed = apply_interface_keys(scope, value, keys)?;
  let object = as_object(scope, value)?;
  let Some(name) = prototype_object_name(scope, object)? else {
    return Ok(KeyAugment {
      listed,
      label: None,
      is_prototype: false,
    });
  };
  for accessor in accessor_names(scope, object)? {
    push_unique_key(keys, accessor);
  }
  let label = if name.is_empty() || name == "Object" {
    None
  } else {
    Some(name)
  };
  Ok(KeyAugment {
    listed,
    label,
    is_prototype: true,
  })
}

fn inherited_descriptor(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  key: &Key,
) -> Result<Option<OwnedDesc>, Thrown> {
  for prototype in prototype_chain(scope, object)? {
    let prototype = v8::Local::new(scope, &prototype);
    if let Some(descriptor) = own_descriptor(scope, prototype, key)? {
      return Ok(Some(descriptor));
    }
  }
  Ok(None)
}

fn accessor_marker(getter: bool, setter: bool) -> &'static str {
  if getter && setter {
    "[Getter/Setter]"
  } else if getter {
    "[Getter]"
  } else {
    "[Setter]"
  }
}

fn format_raw(
  scope: &mut v8::PinScope,
  ctx: &mut Ctx,
  value: v8::Local<v8::Value>,
  recurse: i32,
) -> Result<String, Thrown> {
  let object = as_object(scope, value)?;
  let constructor = constructor_name(scope, value)?;
  let mut tag = to_string_tag(scope, object);
  // A host interface stamps its name on `Symbol.toStringTag`. A subclass
  // keeps its own constructor name, without `[AbortController]` beside it.
  if constructor.as_deref() != Some(tag.as_str()) && is_listed_name(&tag) {
    tag.clear();
  }
  let mut base = String::new();
  let mut kind = Kind::None;
  let mut keys = Vec::new();
  let mut listed_interface = false;
  let mut prototype_object = false;
  let mut extras = OBJECT_TYPE;
  let mut braces = ("{".to_string(), "}".to_string());
  let mut generic = true;
  let has_iterator = {
    let symbol = v8::Symbol::get_iterator(scope);
    v8::tc_scope!(let tc, scope);
    object.has(tc, symbol.into()).unwrap_or(false)
  };
  if has_iterator || constructor.is_none() {
    generic = false;
    if value.is_array() {
      let array = v8::Local::<v8::Array>::try_from(value).unwrap();
      let length = array.length();
      let prefix = if constructor.as_deref() != Some("Array") || !tag.is_empty()
      {
        get_prefix(
          constructor.as_deref(),
          &tag,
          "Array",
          &format!("({length})"),
        )
      } else {
        String::new()
      };
      keys = non_index_keys(scope, object)?;
      braces = (format!("{prefix}["), "]".to_string());
      if length == 0 && keys.is_empty() {
        return Ok(format!("{}]", braces.0));
      }
      extras = ARRAY_EXTRAS_TYPE;
      kind = Kind::Array;
    } else if value.is_set() {
      let set = v8::Local::<v8::Set>::try_from(value).unwrap();
      let size = set.size();
      let prefix =
        get_prefix(constructor.as_deref(), &tag, "Set", &format!("({size})"));
      keys = own_keys(scope, value)?;
      if size == 0 && keys.is_empty() {
        return Ok(format!("{prefix}{{}}"));
      }
      braces = (format!("{prefix}{{"), "}".to_string());
      kind = Kind::Set;
    } else if value.is_map() {
      let map = v8::Local::<v8::Map>::try_from(value).unwrap();
      let size = map.size();
      let prefix =
        get_prefix(constructor.as_deref(), &tag, "Map", &format!("({size})"));
      keys = own_keys(scope, value)?;
      if size == 0 && keys.is_empty() {
        return Ok(format!("{prefix}{{}}"));
      }
      braces = (format!("{prefix}{{"), "}".to_string());
      kind = Kind::Map;
    } else if value.is_typed_array() {
      let typed = v8::Local::<v8::TypedArray>::try_from(value).unwrap();
      let size = typed.length();
      let prefix =
        get_prefix(constructor.as_deref(), &tag, "", &format!("({size})"));
      keys = non_index_keys(scope, object)?;
      braces = (format!("{prefix}["), "]".to_string());
      if size == 0 && keys.is_empty() {
        return Ok(format!("{}]", braces.0));
      }
      extras = ARRAY_EXTRAS_TYPE;
      kind = Kind::TypedArray;
    } else if value.is_map_iterator() {
      keys = own_keys(scope, value)?;
      braces = iterator_braces("Map", &tag);
      kind = Kind::MapIterator;
    } else if value.is_set_iterator() {
      keys = own_keys(scope, value)?;
      braces = iterator_braces("Set", &tag);
      kind = Kind::SetIterator;
    } else {
      generic = true;
    }
  }
  if generic {
    keys = own_keys(scope, value)?;
    braces = ("{".to_string(), "}".to_string());
    let constructor_name = constructor.as_deref();
    if constructor_name == Some("Object") {
      let augment = augment_keys(scope, value, &mut keys)?;
      listed_interface = augment.listed;
      prototype_object = augment.is_prototype;
      if let Some(name) = augment.label.as_deref() {
        let style = ctx_style(scope, value, Some(name), &tag);
        if keys.is_empty() {
          return Ok(format!("{style}{{}}"));
        }
        braces.0 = format!("{style}{{");
      } else {
        if value.is_arguments_object() {
          braces.0 = "[Arguments] {".to_string();
        } else if !tag.is_empty() {
          braces.0 =
            format!("{}{{", get_prefix(constructor_name, &tag, "Object", ""));
        }
        if keys.is_empty() {
          return Ok(format!("{}}}", braces.0));
        }
      }
    } else if value.is_function() {
      base = function_base(scope, value, constructor_name, &tag)?;
      if keys.is_empty() {
        return Ok(color::paint(Ink::Special, &base, ctx.colors));
      }
    } else if value.is_reg_exp() {
      base = regexp_text(scope, value)?;
      let prefix = get_prefix(constructor_name, &tag, "RegExp", "");
      if prefix != "RegExp " {
        base = format!("{prefix}{base}");
      }
      if keys.is_empty() || recurse > DEPTH {
        return Ok(color::paint(Ink::Regexp, &base, ctx.colors));
      }
    } else if value.is_date() {
      let date = v8::Local::<v8::Date>::try_from(value).unwrap();
      let time = date.value_of();
      base = if time.is_nan() {
        "Invalid Date".to_string()
      } else {
        date_to_iso_string(time)
      };
      let prefix = get_prefix(constructor_name, &tag, "Date", "");
      if prefix != "Date " {
        base = format!("{prefix}{base}");
      }
      if keys.is_empty() {
        return Ok(color::paint(Ink::Date, &base, ctx.colors));
      }
    } else if value.is_native_error() || instance_of_name(scope, value, "Error")
    {
      base = format_error(scope, ctx, object, &mut keys)?;
      if keys.is_empty() {
        return Ok(base);
      }
    } else if value.is_array_buffer() || value.is_shared_array_buffer() {
      let array_type = if value.is_array_buffer() {
        "ArrayBuffer"
      } else {
        "SharedArrayBuffer"
      };
      let prefix = get_prefix(constructor_name, &tag, array_type, "");
      braces.0 = format!("{prefix}{{");
      let mut prefixed = vec![Key::Name("byteLength".to_string())];
      prefixed.append(&mut keys);
      keys = prefixed;
      kind = Kind::ArrayBuffer;
    } else if value.is_data_view() {
      braces.0 =
        format!("{}{{", get_prefix(constructor_name, &tag, "DataView", ""));
      let mut prefixed = vec![
        Key::Name("byteLength".to_string()),
        Key::Name("byteOffset".to_string()),
        Key::Name("buffer".to_string()),
      ];
      prefixed.append(&mut keys);
      keys = prefixed;
    } else if value.is_promise() {
      braces.0 =
        format!("{}{{", get_prefix(constructor_name, &tag, "Promise", ""));
      kind = Kind::Promise;
    } else if value.is_weak_set() {
      braces.0 =
        format!("{}{{", get_prefix(constructor_name, &tag, "WeakSet", ""));
      kind = Kind::Weak;
    } else if value.is_weak_map() {
      braces.0 =
        format!("{}{{", get_prefix(constructor_name, &tag, "WeakMap", ""));
      kind = Kind::Weak;
    } else if value.is_module_namespace_object() {
      braces.0 =
        format!("{}{{", get_prefix(constructor_name, &tag, "Module", ""));
      kind = Kind::Namespace;
    } else if is_boxed(value) {
      base = boxed_base(scope, ctx, value, &mut keys, constructor_name, &tag)?;
      if keys.is_empty() {
        return Ok(base);
      }
    } else {
      let augment = augment_keys(scope, value, &mut keys)?;
      listed_interface = augment.listed;
      prototype_object = augment.is_prototype;
      let style = ctx_style(
        scope,
        value,
        augment.label.as_deref().or(constructor_name),
        &tag,
      );
      if keys.is_empty() {
        return Ok(format!("{style}{{}}"));
      }
      braces.0 = format!("{style}{{");
    }
  }

  if recurse > DEPTH && !listed_interface {
    return Ok(collapsed(
      scope,
      value,
      constructor.as_deref(),
      &tag,
      ctx.colors,
    ));
  }
  let recurse = recurse + 1;
  ctx.seen.push(v8::Global::new(scope, value));
  ctx.current_depth = recurse;
  let saved_prototype = ctx.prototype_object;
  ctx.prototype_object = prototype_object;
  let output = (|| -> Result<Vec<String>, Thrown> {
    let mut output = match kind {
      Kind::None => Vec::new(),
      Kind::Array => format_array(scope, ctx, value, recurse)?,
      Kind::Set => format_set(scope, ctx, value, recurse)?,
      Kind::Map => format_map(scope, ctx, value, recurse)?,
      Kind::TypedArray => format_typed_array(scope, ctx, value)?,
      Kind::MapIterator | Kind::SetIterator => {
        format_iterator(scope, ctx, object, &mut braces, recurse)?
      }
      Kind::Promise => format_promise(scope, ctx, value, recurse)?,
      Kind::Weak => {
        vec![color::paint(Ink::Special, "<items unknown>", ctx.colors)]
      }
      Kind::Namespace => {
        format_namespace(scope, ctx, value, &mut keys, recurse)?
      }
      Kind::ArrayBuffer => format_array_buffer(scope, value, ctx.colors)?,
    };
    for key in &keys {
      output.push(format_property(scope, ctx, value, recurse, key, extras)?);
    }
    Ok(output)
  })();
  ctx.prototype_object = saved_prototype;
  ctx.seen.pop();
  let output = output?;
  let base = apply_ref(scope, ctx, value, &base);
  reduce_to_single_string(
    scope,
    ctx,
    output,
    &base,
    &braces,
    extras,
    recurse,
    Some(value),
  )
}

fn collapsed(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
  constructor: Option<&str>,
  tag: &str,
  colors: bool,
) -> String {
  let style = ctx_style(scope, value, constructor, tag);
  let name = style.trim_end_matches(' ');
  let text = if constructor.is_some() {
    format!("[{name}]")
  } else {
    name.to_string()
  };
  color::paint(Ink::Special, &text, colors)
}

fn get_prefix(
  constructor: Option<&str>,
  tag: &str,
  fallback: &str,
  size: &str,
) -> String {
  match constructor {
    None => {
      if !tag.is_empty() && fallback != tag {
        format!("[{fallback}{size}: null prototype] [{tag}] ")
      } else {
        format!("[{fallback}{size}: null prototype] ")
      }
    }
    Some(constructor) => {
      if !tag.is_empty() && constructor != tag {
        format!("{constructor}{size} [{tag}] ")
      } else {
        format!("{constructor}{size} ")
      }
    }
  }
}

fn ctx_style(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
  constructor: Option<&str>,
  tag: &str,
) -> String {
  let mut fallback = String::new();
  if constructor.is_none() {
    if let Ok(object) = v8::Local::<v8::Object>::try_from(value) {
      fallback = object.get_constructor_name().to_rust_string_lossy(scope);
    }
    if fallback == tag {
      fallback = "Object".to_string();
    }
  }
  get_prefix(constructor, tag, &fallback, "")
}

fn to_string_tag(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
) -> String {
  let symbol = v8::Symbol::get_to_string_tag(scope);
  let tag = {
    v8::tc_scope!(let tc, scope);
    match object.get(tc, symbol.into()) {
      Some(value) if value.is_string() => Some(value.to_rust_string_lossy(tc)),
      _ => None,
    }
  };
  tag.unwrap_or_default()
}

fn constructor_name(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<Option<String>, Thrown> {
  let original = v8::Global::new(scope, value);
  let mut current = v8::Global::new(scope, value);
  let mut saw_null = false;
  for _ in 0..64 {
    let cur = v8::Local::new(scope, &current);
    if !(cur.is_object() || cur.is_function()) {
      break;
    }
    let object = v8::Local::<v8::Object>::try_from(cur).unwrap();
    if let Some(name) = own_constructor(scope, object, &original)? {
      return Ok(Some(name));
    }
    match object.get_prototype(scope) {
      Some(prototype) if !prototype.is_null() && !prototype.is_undefined() => {
        current = v8::Global::new(scope, prototype);
      }
      _ => {
        saw_null = true;
        break;
      }
    }
  }
  if saw_null {
    return Ok(None);
  }
  let original = v8::Local::new(scope, &original);
  let object = v8::Local::<v8::Object>::try_from(original).unwrap();
  Ok(Some(
    object.get_constructor_name().to_rust_string_lossy(scope),
  ))
}

fn own_constructor(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  original: &v8::Global<v8::Value>,
) -> Result<Option<String>, Thrown> {
  let key = v8_str(scope, "constructor");
  let descriptor = {
    v8::tc_scope!(let tc, scope);
    match object.get_own_property_descriptor(tc, key.into()) {
      Some(value) if value.is_object() => Ok(Some(v8::Global::new(tc, value))),
      Some(_) => Ok(None),
      None => Err(stash_exception!(tc)),
    }
  }?;
  let Some(descriptor) = descriptor else {
    return Ok(None);
  };
  let descriptor = v8::Local::new(scope, descriptor);
  let descriptor = v8::Local::<v8::Object>::try_from(descriptor).unwrap();
  let value = {
    v8::tc_scope!(let tc, scope);
    let key = v8_str(tc, "value");
    match descriptor.get(tc, key.into()) {
      Some(value) => Ok(v8::Global::new(tc, value)),
      None => Err(stash_exception!(tc)),
    }
  }?;
  let value = v8::Local::new(scope, value);
  if !value.is_function() {
    return Ok(None);
  }
  let function = v8::Local::<v8::Function>::try_from(value).unwrap();
  let name = function.get_name(scope).to_rust_string_lossy(scope);
  if name.is_empty() {
    return Ok(None);
  }
  let receiver = v8::Local::new(scope, original);
  let is_instance = {
    v8::tc_scope!(let tc, scope);
    let function: v8::Local<v8::Object> = function.into();
    receiver.instance_of(tc, function)
  };
  if matches!(is_instance, Some(true)) {
    Ok(Some(name))
  } else {
    Ok(None)
  }
}

fn own_keys(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<Vec<Key>, Thrown> {
  let object = as_object(scope, value)?;
  let module = value.is_module_namespace_object();
  let symbols = property_names(
    scope,
    object,
    v8::PropertyFilter::SKIP_STRINGS,
    v8::IndexFilter::IncludeIndices,
    v8::KeyConversionMode::KeepNumbers,
  )?;
  let mut filter = v8::PropertyFilter::SKIP_SYMBOLS;
  if !module {
    filter = filter | v8::PropertyFilter::ONLY_ENUMERABLE;
  }
  let mut keys = property_names(
    scope,
    object,
    filter,
    v8::IndexFilter::IncludeIndices,
    v8::KeyConversionMode::ConvertToString,
  )?;
  for symbol in symbols {
    let Key::Symbol(symbol) = symbol else {
      continue;
    };
    if symbol_enumerable(scope, object, &symbol)? {
      keys.push(Key::Symbol(symbol));
    }
  }
  if value.is_native_error() || instance_of_name(scope, value, "Error") {
    keys.retain(|key| !matches!(key, Key::Name(name) if name == "cause"));
  }
  Ok(keys)
}

fn non_index_keys(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
) -> Result<Vec<Key>, Thrown> {
  property_names(
    scope,
    object,
    v8::PropertyFilter::ONLY_ENUMERABLE,
    v8::IndexFilter::SkipIndices,
    v8::KeyConversionMode::KeepNumbers,
  )
}

fn property_names(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  filter: v8::PropertyFilter,
  index_filter: v8::IndexFilter,
  conversion: v8::KeyConversionMode,
) -> Result<Vec<Key>, Thrown> {
  let names = {
    v8::tc_scope!(let tc, scope);
    match object.get_property_names(
      tc,
      v8::GetPropertyNamesArgs {
        mode: v8::KeyCollectionMode::OwnOnly,
        property_filter: filter,
        index_filter,
        key_conversion: conversion,
      },
    ) {
      Some(names) => {
        let mut keys = Vec::new();
        for index in 0..names.length() {
          let Some(value) = names.get_index(tc, index) else {
            continue;
          };
          if let Ok(symbol) = v8::Local::<v8::Symbol>::try_from(value) {
            keys.push(Key::Symbol(v8::Global::new(tc, symbol)));
          } else {
            keys.push(Key::Name(value.to_rust_string_lossy(tc)));
          }
        }
        Ok(keys)
      }
      None => Err(stash_exception!(tc)),
    }
  };
  names
}

fn symbol_enumerable(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  symbol: &v8::Global<v8::Symbol>,
) -> Result<bool, Thrown> {
  let symbol = v8::Local::new(scope, symbol);
  let name: v8::Local<v8::Name> = symbol.into();
  let flag = {
    v8::tc_scope!(let tc, scope);
    match object.get_own_property_descriptor(tc, name) {
      Some(descriptor) => {
        let enumerable = v8::Local::<v8::Object>::try_from(descriptor)
          .ok()
          .and_then(|descriptor| {
            let key = v8_str(tc, "enumerable");
            descriptor.get(tc, key.into())
          });
        Ok(enumerable.is_some_and(|value| value.is_true()))
      }
      None => Err(stash_exception!(tc)),
    }
  };
  flag
}

fn iterator_braces(kind: &str, tag: &str) -> (String, String) {
  let expected = format!("{kind} Iterator");
  let mut tag = tag.to_string();
  if tag != expected {
    if !tag.is_empty() {
      tag.push_str("] [");
    }
    tag.push_str(&expected);
  }
  (format!("[{tag}] {{"), "}".to_string())
}

fn mark_entries(braces: &str) -> String {
  if let Some(stripped) = braces.strip_suffix(" Iterator] {") {
    format!("{stripped} Entries] {{")
  } else {
    braces.to_string()
  }
}

fn more_items(remaining: usize) -> String {
  format!(
    "... {remaining} more item{}",
    if remaining > 1 { "s" } else { "" }
  )
}

fn format_array(
  scope: &mut v8::PinScope,
  ctx: &mut Ctx,
  value: v8::Local<v8::Value>,
  recurse: i32,
) -> Result<Vec<String>, Thrown> {
  let array = v8::Local::<v8::Array>::try_from(value).unwrap();
  let object: v8::Local<v8::Object> = array.into();
  let length = array.length() as usize;
  let limit = MAX_ARRAY_LENGTH.min(length);
  let mut output = Vec::new();
  for index in 0..limit {
    let key = index.to_string();
    let present = {
      let name = v8_str(scope, &key);
      v8::tc_scope!(let tc, scope);
      match object.has_own_property(tc, name.into()) {
        Some(present) => Ok(present),
        None => Err(stash_exception!(tc)),
      }
    }?;
    if !present {
      return format_sparse(scope, ctx, value, recurse, limit, output, index);
    }
    output.push(format_property(
      scope,
      ctx,
      value,
      recurse,
      &Key::Name(key),
      ARRAY_TYPE,
    )?);
  }
  if length > limit {
    output.push(more_items(length - limit));
  }
  Ok(output)
}

fn format_sparse(
  scope: &mut v8::PinScope,
  ctx: &mut Ctx,
  value: v8::Local<v8::Value>,
  recurse: i32,
  max_length: usize,
  mut output: Vec<String>,
  start: usize,
) -> Result<Vec<String>, Thrown> {
  let array = v8::Local::<v8::Array>::try_from(value).unwrap();
  let object: v8::Local<v8::Object> = array.into();
  let keys = property_names(
    scope,
    object,
    v8::PropertyFilter::ONLY_ENUMERABLE | v8::PropertyFilter::SKIP_SYMBOLS,
    v8::IndexFilter::IncludeIndices,
    v8::KeyConversionMode::ConvertToString,
  )?;
  let keys: Vec<String> = keys
    .into_iter()
    .filter_map(|key| match key {
      Key::Name(name) => Some(name),
      Key::Symbol(_) => None,
    })
    .collect();
  let mut index = start;
  let mut cursor = start;
  while cursor < keys.len() && output.len() < max_length {
    let key = &keys[cursor];
    let parsed = key.parse::<f64>().unwrap_or(f64::NAN);
    if parsed > (u32::MAX - 1) as f64 {
      break;
    }
    if index.to_string() != *key {
      if !is_canonical_index(key) {
        break;
      }
      let empty = parsed as usize - index;
      output.push(empty_items(empty, ctx.colors));
      index = parsed as usize;
      if output.len() == max_length {
        break;
      }
    }
    output.push(format_property(
      scope,
      ctx,
      value,
      recurse,
      &Key::Name(key.clone()),
      ARRAY_TYPE,
    )?);
    index += 1;
    cursor += 1;
  }
  let remaining = array.length() as i64 - index as i64;
  if output.len() != max_length {
    if remaining > 0 {
      output.push(empty_items(remaining as usize, ctx.colors));
    }
  } else if remaining > 0 {
    output.push(more_items(remaining as usize));
  }
  Ok(output)
}

fn empty_items(count: usize, colors: bool) -> String {
  color::paint(
    Ink::Undefined,
    &format!("<{count} empty item{}>", if count > 1 { "s" } else { "" }),
    colors,
  )
}

fn format_set(
  scope: &mut v8::PinScope,
  ctx: &mut Ctx,
  value: v8::Local<v8::Value>,
  recurse: i32,
) -> Result<Vec<String>, Thrown> {
  ctx.indent += 2;
  let result = (|| {
    let set = v8::Local::<v8::Set>::try_from(value).unwrap();
    let entries = set.as_array(scope);
    let length = set.size();
    let limit = ITERABLE_LIMIT.min(length);
    let mut output = Vec::new();
    for index in 0..limit {
      let item = try_get_index(scope, entries.into(), index as u32)?;
      output.push(format_with(scope, ctx, item, recurse)?);
    }
    if length > limit {
      output.push(more_items(length - limit));
    }
    Ok(output)
  })();
  ctx.indent -= 2;
  result
}

fn format_map(
  scope: &mut v8::PinScope,
  ctx: &mut Ctx,
  value: v8::Local<v8::Value>,
  recurse: i32,
) -> Result<Vec<String>, Thrown> {
  ctx.indent += 2;
  let result = (|| {
    let map = v8::Local::<v8::Map>::try_from(value).unwrap();
    let entries = map.as_array(scope);
    let length = map.size();
    let limit = ITERABLE_LIMIT.min(length);
    let mut output = Vec::new();
    for index in 0..limit {
      let key = try_get_index(scope, entries.into(), (index * 2) as u32)?;
      let item = try_get_index(scope, entries.into(), (index * 2 + 1) as u32)?;
      let key = format_with(scope, ctx, key, recurse)?;
      let item = format_with(scope, ctx, item, recurse)?;
      output.push(format!("{key} => {item}"));
    }
    if length > limit {
      output.push(more_items(length - limit));
    }
    Ok(output)
  })();
  ctx.indent -= 2;
  result
}

fn format_typed_array(
  scope: &mut v8::PinScope,
  ctx: &mut Ctx,
  value: v8::Local<v8::Value>,
) -> Result<Vec<String>, Thrown> {
  let typed = v8::Local::<v8::TypedArray>::try_from(value).unwrap();
  let object: v8::Local<v8::Object> = typed.into();
  let length = typed.length();
  let limit = MAX_ARRAY_LENGTH.min(length);
  let bigint = value.is_big_int64_array() || value.is_big_uint64_array();
  let mut output = Vec::with_capacity(limit);
  for index in 0..limit {
    let element = try_get_index(scope, object, index as u32)?;
    let text = if bigint {
      format_bigint(scope, element)?
    } else {
      number_display(scope, element)?
    };
    output.push(color::paint(Ink::Number, &text, ctx.colors));
  }
  if length > limit {
    output.push(more_items(length - limit));
  }
  Ok(output)
}

fn format_iterator(
  scope: &mut v8::PinScope,
  ctx: &mut Ctx,
  object: v8::Local<v8::Object>,
  braces: &mut (String, String),
  recurse: i32,
) -> Result<Vec<String>, Thrown> {
  let (entries, key_value) = object.preview_entries(scope);
  let Some(entries) = entries else {
    return Ok(Vec::new());
  };
  ctx.indent += 2;
  let result = (|| {
    if key_value {
      braces.0 = mark_entries(&braces.0);
      let length = entries.length() as usize / 2;
      let limit = MAX_ARRAY_LENGTH.min(length);
      let mut output = Vec::new();
      for index in 0..limit {
        let key = try_get_index(scope, entries.into(), (index * 2) as u32)?;
        let item =
          try_get_index(scope, entries.into(), (index * 2 + 1) as u32)?;
        let key = format_with(scope, ctx, key, recurse)?;
        let item = format_with(scope, ctx, item, recurse)?;
        let pair = reduce_to_single_string(
          scope,
          ctx,
          vec![key, item],
          "",
          &("[".to_string(), "]".to_string()),
          ARRAY_EXTRAS_TYPE,
          recurse,
          None,
        )?;
        output.push(pair);
      }
      if length > limit {
        output.push(more_items(length - limit));
      }
      Ok(output)
    } else {
      let length = entries.length() as usize;
      let limit = MAX_ARRAY_LENGTH.min(length);
      let mut output = Vec::new();
      for index in 0..limit {
        let item = try_get_index(scope, entries.into(), index as u32)?;
        output.push(format_with(scope, ctx, item, recurse)?);
      }
      if length > limit {
        output.push(more_items(length - limit));
      }
      Ok(output)
    }
  })();
  ctx.indent -= 2;
  result
}

fn format_promise(
  scope: &mut v8::PinScope,
  ctx: &mut Ctx,
  value: v8::Local<v8::Value>,
  recurse: i32,
) -> Result<Vec<String>, Thrown> {
  let promise = v8::Local::<v8::Promise>::try_from(value).unwrap();
  match promise.state() {
    v8::PromiseState::Pending => {
      Ok(vec![color::paint(Ink::Special, "<pending>", ctx.colors)])
    }
    state => {
      let result = promise.result(scope);
      ctx.indent += 2;
      let formatted = format_with(scope, ctx, result, recurse);
      ctx.indent -= 2;
      let formatted = formatted?;
      if state == v8::PromiseState::Rejected {
        let rejected = color::paint(Ink::Special, "<rejected>", ctx.colors);
        Ok(vec![format!("{rejected} {formatted}")])
      } else {
        Ok(vec![formatted])
      }
    }
  }
}

fn format_namespace(
  scope: &mut v8::PinScope,
  ctx: &mut Ctx,
  value: v8::Local<v8::Value>,
  keys: &mut Vec<Key>,
  recurse: i32,
) -> Result<Vec<String>, Thrown> {
  let mut output = Vec::new();
  for key in keys.iter() {
    match format_property(scope, ctx, value, recurse, key, OBJECT_TYPE) {
      Ok(text) => output.push(text),
      Err(_) => {
        let name = property_name(scope, key, ctx.colors)?;
        let marker = color::paint(Ink::Special, "<uninitialized>", ctx.colors);
        output.push(format!("{name}: {marker}"));
      }
    }
  }
  keys.clear();
  Ok(output)
}

fn format_array_buffer(
  _scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
  colors: bool,
) -> Result<Vec<String>, Thrown> {
  if let Ok(buffer) = v8::Local::<v8::ArrayBuffer>::try_from(value) {
    if buffer.was_detached() {
      return Ok(vec![color::paint(Ink::Special, "(detached)", colors)]);
    }
    let length = buffer.byte_length();
    let shown = MAX_ARRAY_LENGTH.min(length);
    let bytes = copy_optional(buffer.data(), shown);
    return Ok(vec![contents_line(&bytes, length, colors)]);
  }
  if let Ok(buffer) = v8::Local::<v8::SharedArrayBuffer>::try_from(value) {
    let length = buffer.byte_length();
    let shown = MAX_ARRAY_LENGTH.min(length);
    let store = buffer.get_backing_store();
    let available = shown.min(store.byte_length());
    let bytes = copy_optional(store.data(), available);
    return Ok(vec![contents_line(&bytes, length, colors)]);
  }
  Ok(vec![contents_line(&[], 0, colors)])
}

fn copy_optional(
  data: Option<std::ptr::NonNull<std::ffi::c_void>>,
  len: usize,
) -> Vec<u8> {
  let Some(data) = data else {
    return Vec::new();
  };
  if len == 0 {
    return Vec::new();
  }
  let mut bytes = vec![0u8; len];
  // Safety: `len` is clamped to the buffer's byte length and the local
  // keeps the store alive for this copy.
  unsafe {
    std::ptr::copy_nonoverlapping(
      data.as_ptr().cast::<u8>(),
      bytes.as_mut_ptr(),
      len,
    );
  }
  bytes
}

fn contents_line(bytes: &[u8], length: usize, colors: bool) -> String {
  let mut hex = String::new();
  for (index, byte) in bytes.iter().enumerate() {
    if index > 0 {
      hex.push(' ');
    }
    hex.push_str(&format!("{byte:02x}"));
  }
  let remaining = length.saturating_sub(bytes.len());
  if remaining > 0 {
    hex.push_str(&format!(
      " ... {remaining} more byte{}",
      if remaining > 1 { "s" } else { "" }
    ));
  }
  let label = color::paint(Ink::Special, "[Uint8Contents]", colors);
  format!("{label}: <{hex}>")
}

fn format_error(
  scope: &mut v8::PinScope,
  ctx: &Ctx,
  error: v8::Local<v8::Object>,
  keys: &mut Vec<Key>,
) -> Result<String, Thrown> {
  let stack = stack_string(scope, error)?;
  let message = swallowed_get(scope, error, "message");
  let name = swallowed_get(scope, error, "name");
  if !keys.is_empty() {
    remove_key(keys, "stack");
    if let Some(message) = &message {
      let message = v8::Local::new(scope, message);
      let hide = if message.is_string() {
        stack.contains(&message.to_rust_string_lossy(scope))
      } else {
        true
      };
      if hide {
        remove_key(keys, "message");
      }
    }
    if let Some(name) = &name {
      let name = v8::Local::new(scope, name);
      let hide = if name.is_string() {
        stack.contains(&name.to_rust_string_lossy(scope))
      } else {
        true
      };
      if hide {
        remove_key(keys, "name");
      }
    }
  }
  let has_cause = {
    let key = v8_str(scope, "cause");
    v8::tc_scope!(let tc, scope);
    error.has(tc, key.into()).unwrap_or(false)
  };
  if has_cause
    && !keys
      .iter()
      .any(|key| matches!(key, Key::Name(name) if name == "cause"))
  {
    keys.push(Key::Name("cause".to_string()));
  }
  let errors = swallowed_get(scope, error, "errors");
  if let Some(errors) = errors {
    let errors = v8::Local::new(scope, errors);
    if errors.is_array()
      && !keys
        .iter()
        .any(|key| matches!(key, Key::Name(name) if name == "errors"))
    {
      keys.push(Key::Name("errors".to_string()));
    }
  }
  let mut stack = stack;
  if ctx.indent != 0 {
    stack = stack.replace('\n', &format!("\n{}", spaces(ctx.indent)));
  }
  Ok(stack)
}

fn stack_string(
  scope: &mut v8::PinScope,
  error: v8::Local<v8::Object>,
) -> Result<String, Thrown> {
  if let Some(stack) = swallowed_get(scope, error, "stack") {
    let stack = v8::Local::new(scope, stack);
    if stack.is_string() && stack.boolean_value(scope) {
      return Ok(stack.to_rust_string_lossy(scope));
    }
  }
  match prototype_call(scope, "Error", "toString", error.into()) {
    Ok(text) => try_to_string(scope, text),
    Err(_) => Ok("Error".to_string()),
  }
}

fn swallowed_get(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
) -> Option<v8::Global<v8::Value>> {
  let key = v8_str(scope, name);
  let value = {
    v8::tc_scope!(let tc, scope);
    object
      .get(tc, key.into())
      .map(|value| v8::Global::new(tc, value))
  };
  value
}

fn remove_key(keys: &mut Vec<Key>, name: &str) {
  if let Some(index) = keys
    .iter()
    .position(|key| matches!(key, Key::Name(key) if key == name))
  {
    keys.remove(index);
  }
}

fn function_base(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
  constructor: Option<&str>,
  tag: &str,
) -> Result<String, Thrown> {
  let stringified = match prototype_call(scope, "Function", "toString", value) {
    Ok(text) => {
      try_to_string(scope, text).unwrap_or_else(|_| "[Function]".to_string())
    }
    Err(_) => return Ok("[Function]".to_string()),
  };
  if stringified == "[Function]" {
    return Ok(stringified);
  }
  if stringified.starts_with("class") && stringified.ends_with('}') {
    let slice = &stringified[5..stringified.len() - 1];
    if let Some(bracket) = slice.find('{') {
      if !slice[..bracket].contains('(')
        || matches_class_regexp(&strip_comments(slice))
      {
        return class_base(scope, value, constructor, tag);
      }
    }
  }
  let mut ty = "Function".to_string();
  if value.is_generator_function() {
    ty = format!("Generator{ty}");
  }
  if value.is_async_function() {
    ty = format!("Async{ty}");
  }
  let mut base = format!("[{ty}");
  if constructor.is_none() {
    base.push_str(" (null prototype)");
  }
  let object = as_object(scope, value)?;
  let name = function_name(scope, object)?;
  if name.is_empty() {
    base.push_str(" (anonymous)");
  } else {
    base.push_str(&format!(": {name}"));
  }
  base.push(']');
  if let Some(ctor) = constructor {
    if ctor != ty {
      base.push_str(&format!(" {ctor}"));
    }
  }
  if !tag.is_empty() && constructor != Some(tag) {
    base.push_str(&format!(" [{tag}]"));
  }
  Ok(base)
}

fn function_name(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
) -> Result<String, Thrown> {
  let read = {
    let key = v8_str(scope, "name");
    v8::tc_scope!(let tc, scope);
    match object.get(tc, key.into()) {
      Some(value) if value.is_string() => Ok(value.to_rust_string_lossy(tc)),
      Some(_) => Ok(String::new()),
      None => Err(()),
    }
  };
  if let Ok(name) = read {
    return Ok(name);
  }
  let function = v8::Local::<v8::Function>::try_from(object).ok();
  Ok(
    function
      .map(|function| function.get_name(scope).to_rust_string_lossy(scope))
      .unwrap_or_default(),
  )
}

fn class_base(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
  constructor: Option<&str>,
  tag: &str,
) -> Result<String, Thrown> {
  let object = as_object(scope, value)?;
  let name = function_name(scope, object)?;
  let name = if name.is_empty() {
    "(anonymous)".to_string()
  } else {
    name
  };
  let mut base = format!("class {name}");
  if let Some(ctor) = constructor {
    if ctor != "Function" {
      base.push_str(&format!(" [{ctor}]"));
    }
  }
  if !tag.is_empty() && constructor != Some(tag) {
    base.push_str(&format!(" [{tag}]"));
  }
  if constructor.is_some() {
    if let Some(prototype) = object.get_prototype(scope) {
      if let Ok(prototype) = v8::Local::<v8::Object>::try_from(prototype) {
        let super_name = {
          v8::tc_scope!(let tc, scope);
          let key = v8_str(tc, "name");
          prototype
            .get(tc, key.into())
            .filter(|value| value.is_string())
            .map(|value| value.to_rust_string_lossy(tc))
            .unwrap_or_default()
        };
        if !super_name.is_empty() {
          base.push_str(&format!(" extends {super_name}"));
        }
      }
    }
  } else {
    base.push_str(" extends [null prototype]");
  }
  Ok(format!("[{base}]"))
}

fn strip_comments(text: &str) -> String {
  let bytes = text.as_bytes();
  let mut out = String::with_capacity(text.len());
  let mut index = 0;
  while index < bytes.len() {
    if bytes[index] == b'/'
      && index + 1 < bytes.len()
      && bytes[index + 1] == b'/'
    {
      if let Some(newline) = text[index..].find('\n') {
        out.push_str("undefined");
        index += newline + 1;
        continue;
      }
    }
    if bytes[index] == b'/'
      && index + 1 < bytes.len()
      && bytes[index + 1] == b'*'
    {
      if let Some(end) = text[index + 2..].find("*/") {
        out.push_str("undefined");
        index += 2 + end + 2;
        continue;
      }
    }
    let ch = text[index..].chars().next().unwrap();
    out.push(ch);
    index += ch.len_utf8();
  }
  out
}

fn matches_class_regexp(text: &str) -> bool {
  let mut chars = text.chars();
  match chars.next() {
    Some(ch) if ch.is_whitespace() => {}
    _ => return false,
  }
  for ch in text.chars().skip(1) {
    match ch {
      '{' => return true,
      '(' => return false,
      _ => {}
    }
  }
  false
}

fn regexp_text(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<String, Thrown> {
  let text = prototype_call(scope, "RegExp", "toString", value)?;
  try_to_string(scope, text)
}

fn is_boxed(value: v8::Local<v8::Value>) -> bool {
  value.is_number_object()
    || value.is_string_object()
    || value.is_boolean_object()
    || value.is_big_int_object()
    || value.is_symbol_object()
}

fn boxed_base(
  scope: &mut v8::PinScope,
  ctx: &Ctx,
  value: v8::Local<v8::Value>,
  keys: &mut Vec<Key>,
  constructor: Option<&str>,
  tag: &str,
) -> Result<String, Thrown> {
  let (ty, method, primitive_ctor) = if value.is_number_object() {
    ("Number", "valueOf", "Number")
  } else if value.is_string_object() {
    ("String", "valueOf", "String")
  } else if value.is_boolean_object() {
    ("Boolean", "valueOf", "Boolean")
  } else if value.is_big_int_object() {
    ("BigInt", "valueOf", "BigInt")
  } else {
    ("Symbol", "valueOf", "Symbol")
  };
  let primitive = prototype_call(scope, primitive_ctor, method, value)?;
  if value.is_string_object() {
    let len = v8::Local::<v8::String>::try_from(primitive)
      .map(|text| text.length())
      .unwrap_or(0);
    keys.retain(|key| !string_index_key(key, len));
  }
  let mut base = format!("[{ty}");
  if Some(ty) != constructor {
    match constructor {
      None => base.push_str(" (null prototype)"),
      Some(name) => base.push_str(&format!(" ({name})")),
    }
  }
  let inner = format_primitive(scope, ctx, primitive, false)?;
  base.push_str(&format!(": {inner}]"));
  if !tag.is_empty() && Some(tag) != constructor {
    base.push_str(&format!(" [{tag}]"));
  }
  if !keys.is_empty() {
    return Ok(base);
  }
  let ink = match ty {
    "String" | "Symbol" => Ink::String,
    _ => Ink::Number,
  };
  Ok(color::paint(ink, &base, ctx.colors))
}

fn string_index_key(key: &Key, len: usize) -> bool {
  let Key::Name(name) = key else {
    return false;
  };
  if !is_canonical_index(name) {
    return false;
  }
  name.parse::<usize>().is_ok_and(|index| index < len)
}

fn civil_from_days(z: i64) -> (i64, u32, u32) {
  let z = z + 719468;
  let era = if z >= 0 { z } else { z - 146096 } / 146097;
  let doe = (z - era * 146097) as u64;
  let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
  let y = yoe as i64 + era * 400;
  let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
  let mp = (5 * doy + 2) / 153;
  let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
  let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
  (if m <= 2 { y + 1 } else { y }, m, d)
}

fn date_to_iso_string(time: f64) -> String {
  let ms_total = time.floor() as i64;
  let days = ms_total.div_euclid(86_400_000);
  let ms_of_day = ms_total.rem_euclid(86_400_000);
  let (year, month, day) = civil_from_days(days);
  let hours = ms_of_day / 3_600_000;
  let minutes = (ms_of_day % 3_600_000) / 60_000;
  let seconds = (ms_of_day % 60_000) / 1000;
  let ms = ms_of_day % 1000;
  let year = if (0..=9999).contains(&year) {
    format!("{year:04}")
  } else if year < 0 {
    format!("-{:06}", -year)
  } else {
    format!("+{year:06}")
  };
  format!(
    "{year}-{month:02}-{day:02}T{hours:02}:{minutes:02}:{seconds:02}.{ms:03}Z"
  )
}

fn format_property(
  scope: &mut v8::PinScope,
  ctx: &mut Ctx,
  value: v8::Local<v8::Value>,
  recurse: i32,
  key: &Key,
  ty: u8,
) -> Result<String, Thrown> {
  let object = as_object(scope, value)?;
  let descriptor = own_descriptor(scope, object, key)?;
  let enumerable = descriptor.as_ref().map(|desc| desc.enumerable);
  let formatted = if let Some(descriptor) = descriptor {
    if let Some(stored) = descriptor.value {
      let stored = v8::Local::new(scope, stored);
      let indent = ctx.indent;
      ctx.indent += 2;
      let formatted = format_with(scope, ctx, stored, recurse);
      ctx.indent = indent;
      formatted?
    } else if descriptor.getter && descriptor.setter {
      "[Getter/Setter]".to_string()
    } else if descriptor.getter {
      "[Getter]".to_string()
    } else if descriptor.setter {
      "[Setter]".to_string()
    } else {
      "undefined".to_string()
    }
  } else {
    let inherited = inherited_descriptor(scope, object, key)?;
    let marker = inherited
      .as_ref()
      .map(|descriptor| accessor_marker(descriptor.getter, descriptor.setter));
    let skip_call = inherited.as_ref().is_some_and(|descriptor| {
      (descriptor.getter || descriptor.setter)
        && (!descriptor.getter || ctx.prototype_object)
    });
    if skip_call {
      marker.unwrap_or("[Getter]").to_string()
    } else {
      let key_value = key_to_value(scope, key)?;
      match try_get(scope, object, key_value) {
        Ok(stored) => {
          let indent = ctx.indent;
          ctx.indent += 2;
          let formatted = format_with(scope, ctx, stored, recurse);
          ctx.indent = indent;
          formatted?
        }
        Err(thrown) => {
          if inherited.is_some_and(|descriptor| descriptor.getter) {
            marker.unwrap_or("[Getter]").to_string()
          } else {
            return Err(thrown);
          }
        }
      }
    }
  };
  // The marker comparison uses the uncolored text. Painting happens
  // after that, including a raw descriptor `undefined`.
  let is_accessor = matches!(
    formatted.as_str(),
    "[Getter]" | "[Setter]" | "[Getter/Setter]"
  );
  let formatted = if is_accessor {
    color::paint(Ink::Special, &formatted, ctx.colors)
  } else if ctx.colors && formatted == "undefined" {
    color::paint(Ink::Undefined, &formatted, true)
  } else {
    formatted
  };
  if ty == ARRAY_TYPE {
    return Ok(formatted);
  }
  let mut name = property_name(scope, key, ctx.colors)?;
  // Class accessors and a host attribute shadowed by an own accessor
  // are non-enumerable. Brackets would hide that the name is the same
  // field an instance prints. Non-enumerable data, such as `cause`,
  // stays in brackets.
  if enumerable == Some(false) && !ctx.prototype_object && !is_accessor {
    name = format!("[{name}]");
  }
  Ok(format!("{name}: {formatted}"))
}

fn property_name(
  scope: &mut v8::PinScope,
  key: &Key,
  colors: bool,
) -> Result<String, Thrown> {
  match key {
    Key::Symbol(symbol) => {
      let symbol = v8::Local::new(scope, symbol);
      let text = escape_meta_chars(&symbol_text(scope, symbol));
      Ok(color::paint(Ink::String, &text, colors))
    }
    Key::Name(name) => {
      if is_identifier_like(name) {
        if name == "__proto__" {
          Ok("['__proto__']".to_string())
        } else {
          Ok(name.clone())
        }
      } else {
        Ok(color::paint(Ink::String, &quote_string(name), colors))
      }
    }
  }
}

fn key_to_value<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  key: &Key,
) -> Result<v8::Local<'s, v8::Value>, Thrown> {
  match key {
    Key::Name(name) => Ok(v8_str(scope, name).into()),
    Key::Symbol(symbol) => Ok(v8::Local::new(scope, symbol).into()),
  }
}

fn own_descriptor(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  key: &Key,
) -> Result<Option<OwnedDesc>, Thrown> {
  let name = match key {
    Key::Name(name) => {
      let name = v8_str(scope, name);
      let name: v8::Local<v8::Name> = name.into();
      name
    }
    Key::Symbol(symbol) => {
      let symbol = v8::Local::new(scope, symbol);
      let name: v8::Local<v8::Name> = symbol.into();
      name
    }
  };
  let descriptor = {
    v8::tc_scope!(let tc, scope);
    match object.get_own_property_descriptor(tc, name) {
      Some(value) if value.is_object() => Ok(Some(v8::Global::new(tc, value))),
      Some(_) => Ok(None),
      None => Err(stash_exception!(tc)),
    }
  }?;
  let Some(descriptor) = descriptor else {
    return Ok(None);
  };
  let descriptor = v8::Local::new(scope, descriptor);
  let descriptor = v8::Local::<v8::Object>::try_from(descriptor).unwrap();
  let value = field_global(scope, descriptor, "value")?;
  let getter = field_global(scope, descriptor, "get")?;
  let setter = field_global(scope, descriptor, "set")?;
  let enumerable = field_global(scope, descriptor, "enumerable")?;
  let value_defined = value
    .as_ref()
    .is_some_and(|value| !v8::Local::new(scope, value).is_undefined());
  let getter_fn = getter
    .as_ref()
    .is_some_and(|value| v8::Local::new(scope, value).is_function());
  let setter_fn = setter
    .as_ref()
    .is_some_and(|value| v8::Local::new(scope, value).is_function());
  let enumerable = enumerable
    .as_ref()
    .is_none_or(|value| v8::Local::new(scope, value).boolean_value(scope));
  Ok(Some(OwnedDesc {
    value: if value_defined { value } else { None },
    getter: !value_defined && getter_fn,
    setter: !value_defined && setter_fn,
    enumerable,
  }))
}

fn field_global(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
) -> Result<Option<v8::Global<v8::Value>>, Thrown> {
  let key = v8_str(scope, name);
  let result = {
    v8::tc_scope!(let tc, scope);
    match object.get(tc, key.into()) {
      Some(value) => Ok(Some(v8::Global::new(tc, value))),
      None => Err(stash_exception!(tc)),
    }
  };
  result
}

fn is_below_break_length(output: &[String], start: usize, base: &str) -> bool {
  let mut total = output.len() + start;
  if total + output.len() > BREAK_LENGTH {
    return false;
  }
  for item in output {
    total += visible_utf16(item);
    if total > BREAK_LENGTH {
      return false;
    }
  }
  base.is_empty() || !base.contains('\n')
}

fn reduce_to_single_string(
  scope: &mut v8::PinScope,
  ctx: &mut Ctx,
  mut output: Vec<String>,
  base: &str,
  braces: &(String, String),
  extras: u8,
  recurse: i32,
  value: Option<v8::Local<v8::Value>>,
) -> Result<String, Thrown> {
  if COMPACT >= 1 {
    let entries = output.len();
    if extras == ARRAY_EXTRAS_TYPE && entries > 6 {
      output = group_array_elements(scope, ctx, output, value);
    }
    if ctx.current_depth - recurse < COMPACT && entries == output.len() {
      let start = output.len()
        + ctx.indent.max(0) as usize
        + visible_utf16(&braces.0)
        + visible_utf16(base)
        + 10;
      if is_below_break_length(&output, start, base) {
        let joined = output.join(", ");
        if !joined.contains('\n') {
          let base_part = if base.is_empty() {
            String::new()
          } else {
            format!("{base} ")
          };
          return Ok(format!("{base_part}{} {joined} {}", braces.0, braces.1));
        }
      }
    }
  }
  let indentation = format!("\n{}", spaces(ctx.indent));
  let base_part = if base.is_empty() {
    String::new()
  } else {
    format!("{base} ")
  };
  Ok(format!(
    "{base_part}{}{indentation}  {}{indentation}{}",
    braces.0,
    output.join(&format!(",{indentation}  ")),
    braces.1
  ))
}

fn pad_utf16(text: &str, target: usize, pad_start: bool) -> String {
  let len = utf16_length(text);
  if len >= target {
    return text.to_string();
  }
  let padding = " ".repeat(target - len);
  if pad_start {
    format!("{padding}{text}")
  } else {
    format!("{text}{padding}")
  }
}

fn group_array_elements(
  scope: &mut v8::PinScope,
  ctx: &Ctx,
  output: Vec<String>,
  value: Option<v8::Local<v8::Value>>,
) -> Vec<String> {
  let mut total_length = 0usize;
  let mut max_length = 0usize;
  let mut output_length = output.len();
  if MAX_ARRAY_LENGTH < output.len() {
    output_length -= 1;
  }
  let separator_space = 2usize;
  let mut data_len = vec![0usize; output_length];
  for (index, item) in output.iter().take(output_length).enumerate() {
    let len = display_width(item);
    data_len[index] = len;
    total_length += len + separator_space;
    if max_length < len {
      max_length = len;
    }
  }
  let actual_max = max_length + separator_space;
  if actual_max == 0 {
    return output;
  }
  if ((actual_max * 3 + ctx.indent.max(0) as usize) as f64)
    < BREAK_LENGTH as f64
    && ((total_length as f64) / (actual_max as f64) > 5.0 || max_length <= 6)
  {
    let approx_char_heights = 2.5f64;
    let bias_inner =
      (actual_max as f64) - (total_length as f64) / (output.len() as f64);
    if bias_inner < 0.0 {
      return output;
    }
    let average_bias = bias_inner.sqrt();
    let biased_max = ((actual_max as f64) - 3.0 - average_bias).max(1.0);
    let columns_f =
      ((approx_char_heights * biased_max * (output_length as f64)).sqrt()
        / biased_max)
        .round();
    let columns = columns_f
      .min(
        ((BREAK_LENGTH as f64 - ctx.indent.max(0) as f64)
          / (actual_max as f64))
          .floor(),
      )
      .min(COMPACT as f64 * 4.0)
      .min(15.0);
    if columns <= 1.0 {
      return output;
    }
    let columns = columns as usize;
    let mut max_line_length = Vec::with_capacity(columns);
    for column in 0..columns {
      let mut line_max = 0usize;
      let mut index = column;
      while index < output.len() {
        if data_len.get(index).copied().unwrap_or(0) > line_max {
          line_max = data_len[index];
        }
        index += columns;
      }
      max_line_length.push(line_max + separator_space);
    }
    let mut pad_start = true;
    if let Some(value) = value {
      if let Ok(object) = v8::Local::<v8::Object>::try_from(value) {
        v8::tc_scope!(let tc, scope);
        for index in 0..output.len() {
          let element = object.get_index(tc, index as u32);
          let numeric = element
            .map(|element| element.is_number() || element.is_big_int())
            .unwrap_or(false);
          if !numeric {
            pad_start = false;
            break;
          }
        }
      } else {
        pad_start = false;
      }
    }
    let mut grouped = Vec::new();
    let mut index = 0usize;
    while index < output_length {
      let end = (index + columns).min(output_length);
      let mut line = String::new();
      let mut column = index;
      while column < end - 1 {
        // `data_len` is the visible width. The raw UTF-16 length
        // still includes color bytes, so those bytes cancel here.
        let padding = max_line_length[column - index]
          + utf16_length(&output[column])
          - data_len[column];
        let item = format!("{}, ", output[column]);
        line.push_str(&pad_utf16(&item, padding, pad_start));
        column += 1;
      }
      if pad_start {
        let padding = max_line_length[column - index]
          + utf16_length(&output[column])
          - data_len[column]
          - separator_space;
        line.push_str(&pad_utf16(&output[column], padding, true));
      } else {
        line.push_str(&output[column]);
      }
      grouped.push(line);
      index += columns;
    }
    if MAX_ARRAY_LENGTH < output.len() {
      grouped.push(output[output_length].clone());
    }
    return grouped;
  }
  output
}

fn format_args(
  scope: &mut v8::PinScope,
  ctx: &mut Ctx,
  args: &[v8::Local<v8::Value>],
) -> Result<String, Thrown> {
  let mut index = 0usize;
  let mut out = String::new();
  if args.len() > 1 && args[0].is_string() {
    let first = rust_primitive(scope, args[0]);
    let units: Vec<u16> = first.encode_utf16().collect();
    index = 1;
    let mut copied = 0usize;
    let mut cursor = 0usize;
    while cursor + 1 < units.len() {
      if units[cursor] == b'%' as u16 {
        cursor += 1;
        let ch = units[cursor];
        if index < args.len() {
          let formatted = format_specifier(scope, ctx, ch, args[index])?;
          if formatted.consumed {
            index += 1;
          }
          if let Some(text) = formatted.text {
            out.push_str(&String::from_utf16_lossy(&units[copied..cursor - 1]));
            out.push_str(&text);
            copied = cursor + 1;
          }
        }
        if ch == b'%' as u16 {
          out.push_str(&String::from_utf16_lossy(&units[copied..cursor - 1]));
          out.push('%');
          copied = cursor + 1;
        }
      }
      cursor += 1;
    }
    out.push_str(&String::from_utf16_lossy(&units[copied..]));
  }
  while index < args.len() {
    if index > 0 {
      out.push(' ');
    }
    if args[index].is_string() {
      out.push_str(&rust_primitive(scope, args[index]));
    } else {
      out.push_str(&format_with(scope, ctx, args[index], 0)?);
    }
    index += 1;
  }
  Ok(out)
}

struct Specifier {
  text: Option<String>,
  consumed: bool,
}

fn format_specifier(
  scope: &mut v8::PinScope,
  ctx: &mut Ctx,
  ch: u16,
  value: v8::Local<v8::Value>,
) -> Result<Specifier, Thrown> {
  if ch >= 128 {
    return Ok(Specifier {
      text: None,
      consumed: false,
    });
  }
  let ch = ch as u8 as char;
  match ch {
    's' => Ok(Specifier {
      text: Some(try_to_string(scope, value)?),
      consumed: true,
    }),
    'd' | 'i' => Ok(Specifier {
      text: Some(format_integer(scope, value)?),
      consumed: true,
    }),
    'f' => Ok(Specifier {
      text: Some(format_float(scope, value)?),
      consumed: true,
    }),
    'j' => {
      if value.is_undefined() || value.is_function() || value.is_symbol() {
        Ok(Specifier {
          text: None,
          consumed: true,
        })
      } else {
        Ok(Specifier {
          text: Some(json_stringify(scope, ctx, value)?),
          consumed: true,
        })
      }
    }
    'O' | 'o' => Ok(Specifier {
      text: Some(format_with(scope, ctx, value, 0)?),
      consumed: true,
    }),
    'c' => Ok(Specifier {
      text: Some(String::new()),
      consumed: true,
    }),
    _ => Ok(Specifier {
      text: None,
      consumed: false,
    }),
  }
}

fn format_integer(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<String, Thrown> {
  if value.is_symbol() {
    return Ok("NaN".to_string());
  }
  let number = number_from(scope, value)?;
  if number.is_nan() {
    Ok("NaN".to_string())
  } else {
    Ok(format!("{}", number as i64))
  }
}

fn format_float(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<String, Thrown> {
  if value.is_symbol() {
    return Ok("NaN".to_string());
  }
  let number = number_from(scope, value)?;
  if number.is_nan() {
    Ok("NaN".to_string())
  } else {
    Ok(number.to_string())
  }
}

fn json_stringify(
  scope: &mut v8::PinScope,
  ctx: &mut Ctx,
  value: v8::Local<v8::Value>,
) -> Result<String, Thrown> {
  let result = {
    v8::tc_scope!(let tc, scope);
    match v8::json::stringify(tc, value) {
      Some(text) => Ok(text.to_rust_string_lossy(tc)),
      None => Err(stash_exception!(tc)),
    }
  };
  match result {
    Ok(text) => Ok(text),
    Err(thrown) => {
      if ctx.circular_error_message.is_none() {
        ctx.circular_error_message = Some(circular_json_message(scope));
      }
      if is_circular_json(scope, &thrown, ctx.circular_error_message.as_deref())
      {
        Ok("[Circular]".to_string())
      } else {
        Err(thrown)
      }
    }
  }
}

fn circular_json_message(scope: &mut v8::PinScope) -> String {
  let sample = v8::Object::new(scope);
  let key = v8_str(scope, "a");
  sample.set(scope, key.into(), sample.into());
  let message = {
    v8::tc_scope!(let tc, scope);
    match v8::json::stringify(tc, sample.into()) {
      Some(_) => String::new(),
      None => tc
        .exception()
        .and_then(|exception| v8::Local::<v8::Object>::try_from(exception).ok())
        .and_then(|object| {
          let key = v8_str(tc, "message");
          object.get(tc, key.into())
        })
        .map(|message| message.to_rust_string_lossy(tc))
        .unwrap_or_default(),
    }
  };
  message.split('\n').next().unwrap_or("").to_string()
}

fn is_circular_json(
  scope: &mut v8::PinScope,
  thrown: &Thrown,
  expected: Option<&str>,
) -> bool {
  let Some(expected) = expected else {
    return false;
  };
  let exception = v8::Local::new(scope, &thrown.0);
  let Ok(object) = v8::Local::<v8::Object>::try_from(exception) else {
    return false;
  };
  let name_key = v8_str(scope, "name");
  let Some(name) = object.get(scope, name_key.into()) else {
    return false;
  };
  if name.to_rust_string_lossy(scope) != "TypeError" {
    return false;
  }
  let message_key = v8_str(scope, "message");
  let Some(message) = object.get(scope, message_key.into()) else {
    return false;
  };
  let message = message.to_rust_string_lossy(scope);
  message.split('\n').next().unwrap_or("") == expected
}

#[cfg(test)]
mod field_tests {
  use super::{InspectFields, resolve_fields};

  #[test]
  fn inspect_fields_replace_the_ancestor() {
    let blob = &["size", "type"][..];
    let file = &["name", "size", "type"][..];
    let steps = [
      Some(InspectFields::List(file)),
      Some(InspectFields::List(blob)),
    ];
    let (index, fields) = resolve_fields(&steps).unwrap();
    assert_eq!(index, 0);
    assert_eq!(fields, ["name", "size", "type"]);
  }

  #[test]
  fn inspect_fields_follow_the_ancestor() {
    let event = &["type"][..];
    let data = &["data"][..];
    let more = &["more"][..];
    let steps = [
      Some(InspectFields::After(more)),
      Some(InspectFields::After(data)),
      Some(InspectFields::List(event)),
    ];
    let (index, fields) = resolve_fields(&steps).unwrap();
    assert_eq!(index, 0);
    assert_eq!(fields, ["type", "data", "more"]);
  }

  #[test]
  fn inspect_fields_skip_unlisted_prototypes() {
    let event = &["bubbles", "type"][..];
    let steps = [None, None, Some(InspectFields::List(event))];
    let (index, fields) = resolve_fields(&steps).unwrap();
    assert_eq!(index, 2);
    assert_eq!(fields, ["bubbles", "type"]);
    assert!(resolve_fields(&[None, None]).is_none());
  }
}

#[cfg(test)]
mod quote_tests {
  use super::{is_identifier_like, quote_string, symbol_description_plain};

  #[test]
  fn quotes_pick_the_first_absent_mark() {
    assert_eq!(quote_string("hello"), "\"hello\"");
    assert_eq!(quote_string("he\"llo"), "'he\"llo'");
    assert_eq!(quote_string("he\"l'lo"), "`he\"l'lo`");
    assert_eq!(quote_string("a\nb"), "\"a\\nb\"");
    assert_eq!(quote_string("a\\b"), "\"a\\\\b\"");
  }

  #[test]
  fn identifiers_exclude_dollar_and_symbol_dots() {
    assert!(is_identifier_like("class"));
    assert!(!is_identifier_like("$foo"));
    assert!(symbol_description_plain("Symbol.iterator"));
    assert!(!symbol_description_plain("a b"));
  }
}
