//! Optional `Yuzuko` foreign function calls.
//!
//! The `ffi` feature installs `dlopen` and `Library` on the main realm and
//! on a dedicated worker. The library is loaded on the caller thread. Symbol
//! calls use one fixed `extern "C"` function type. There is no `libffi`.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::ffi::{CString, c_void};
use std::sync::atomic::{AtomicU64, Ordering};

use yuzuko_macros::op;

use crate::op::{Js, OpError, OpException, wrap_instance};

const MAX_SYMBOLS: u32 = 256;

const ARITY: &str = "Not enough arguments";
const PATH_TYPE: &str = "The path must be a string.";
const PATH_EMPTY: &str = "The path must not be empty.";
const PATH_NUL: &str = "The path must not contain a null character.";
const SYMBOLS: &str = "The symbols must be an object.";
const SYMBOLS_READ: &str = "The symbols could not be read.";
const NAME_TYPE: &str = "The symbol name must be a string.";
const NAME_EMPTY: &str = "The symbol name must not be empty.";
const NAME_NUL: &str = "The symbol name must not contain a null character.";
const TOO_MANY: &str = "There are too many symbols.";
const DEFINITION: &str = "The symbol definition must be an object.";
const UNSUPPORTED: &str = "The function type is not supported.";
const OPEN_FAIL: &str = "The library could not be opened.";
const CLOSED: &str = "The library is closed.";
const ARGUMENT: &str = "The argument could not be converted.";

static NEXT: AtomicU64 = AtomicU64::new(1);

thread_local! {
  static SYMBOLS_MAP: RefCell<HashMap<u64, Symbol>> =
    RefCell::new(HashMap::new());
  static OPEN: RefCell<HashMap<u64, Opened>> = RefCell::new(HashMap::new());
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
  Void,
  I32,
  F64,
}

#[derive(Clone, Copy)]
enum Signature {
  NoneToVoid,
  NoneToI32,
  NoneToF64,
  I32ToI32,
  I32I32ToI32,
  I32ToVoid,
  I32I32ToVoid,
  F64ToF64,
  F64F64ToF64,
  F64ToVoid,
  F64F64ToVoid,
}

impl Signature {
  fn arity(self) -> i32 {
    match self {
      Self::NoneToVoid | Self::NoneToI32 | Self::NoneToF64 => 0,
      Self::I32ToI32 | Self::I32ToVoid | Self::F64ToF64 | Self::F64ToVoid => 1,
      Self::I32I32ToI32
      | Self::I32I32ToVoid
      | Self::F64F64ToF64
      | Self::F64F64ToVoid => 2,
    }
  }
}

struct Prepared {
  name: String,
  signature: Signature,
}

struct Symbol {
  pointer: usize,
  signature: Signature,
}

struct Resolved {
  id: u64,
  pointer: usize,
  signature: Signature,
}

struct Opened {
  handle: usize,
  ids: Vec<u64>,
}

struct Loaded {
  handle: Option<usize>,
}

impl Drop for Loaded {
  fn drop(&mut self) {
    if let Some(handle) = self.handle.take() {
      unload(handle);
    }
  }
}

struct CallArgs {
  i0: i32,
  i1: i32,
  f0: f64,
  f1: f64,
}

pub struct Library {
  id: u64,
  alive: Cell<bool>,
  symbols: v8::Global<v8::Object>,
}

unsafe impl v8::cppgc::GarbageCollected for Library {
  fn trace(&self, _visitor: &mut v8::cppgc::Visitor) {}

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"Library"
  }
}

impl Drop for Library {
  fn drop(&mut self) {
    if self.alive.replace(false) {
      release(self.id);
    }
  }
}

#[op(name = "Library")]
impl Library {
  #[getter]
  fn symbols(&self, scope: &mut v8::PinScope) -> Result<Js, OpError> {
    let local = v8::Local::new(scope, &self.symbols);
    let value: v8::Local<v8::Value> = local.into();
    Ok(Js(v8::Global::new(scope, value)))
  }

  fn close(&self, scope: &mut v8::PinScope) -> Result<(), OpError> {
    if !self.alive.replace(false) {
      return Err(plain_error(scope, CLOSED));
    }
    release(self.id);
    Ok(())
  }
}

pub(crate) fn install(scope: &mut v8::PinScope) {
  Library::install(scope);
  move_constructor(scope, "Library");
  let yuzuko = crate::namespace(scope, "Yuzuko");
  define_fn(scope, yuzuko, "dlopen", 2, dlopen_callback);
}

pub(crate) fn clear() {
  let ids = OPEN.with(|open| open.borrow().keys().copied().collect::<Vec<_>>());
  for id in ids {
    release(id);
  }
}

fn dlopen_callback(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut ret: v8::ReturnValue,
) {
  match dlopen_op(scope, &args) {
    Ok(object) => ret.set(object.into()),
    Err(error) => OpException::throw(error, scope),
  }
}

fn dlopen_op<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  args: &v8::FunctionCallbackArguments,
) -> Result<v8::Local<'s, v8::Object>, OpError> {
  if args.length() < 2 {
    return Err(OpError::type_(ARITY));
  }
  let path = string_primitive(scope, args.get(0), PATH_TYPE)?;
  if path.is_empty() {
    return Err(OpError::type_(PATH_EMPTY));
  }
  if path.contains('\0') {
    return Err(OpError::type_(PATH_NUL));
  }
  let prepared = read_symbols(scope, args.get(1))?;
  let handle =
    open_library(&path).ok_or_else(|| plain_error(scope, OPEN_FAIL))?;
  let mut loaded = Loaded {
    handle: Some(handle),
  };
  let Some(handle) = loaded.handle else {
    return Err(plain_error(scope, OPEN_FAIL));
  };
  let pointers = resolve_all(handle, &prepared)
    .ok_or_else(|| plain_error(scope, OPEN_FAIL))?;
  let (symbols, resolved) = build_symbols(scope, &prepared, &pointers)?;
  let library_id = next_id();
  let Some(native) = loaded.handle.take() else {
    return Err(plain_error(scope, OPEN_FAIL));
  };
  publish(library_id, native, &resolved);
  // Safety: `Library::install` cached this realm's `Library` template.
  let object = unsafe {
    wrap_instance(
      scope,
      Library {
        id: library_id,
        alive: Cell::new(true),
        symbols,
      },
    )
  };
  Ok(object)
}

fn symbol_callback(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut ret: v8::ReturnValue,
) {
  match symbol_op(scope, &args) {
    Ok(None) => ret.set(v8::undefined(scope).into()),
    Ok(Some(value)) => ret.set(v8::Number::new(scope, value).into()),
    Err(error) => OpException::throw(error, scope),
  }
}

fn symbol_op(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
) -> Result<Option<f64>, OpError> {
  let id = symbol_id(args)?;
  let Some(signature) = lookup_signature(id) else {
    return Err(plain_error(scope, CLOSED));
  };
  if args.length() < signature.arity() {
    return Err(OpError::type_(ARITY));
  }
  let converted = convert_args(scope, args, signature)?;
  let Some(pointer) = lookup_pointer(id) else {
    return Err(plain_error(scope, CLOSED));
  };
  Ok(invoke(pointer, signature, converted))
}

fn symbol_id(args: &v8::FunctionCallbackArguments) -> Result<u64, OpError> {
  let Ok(external) = v8::Local::<v8::External>::try_from(args.data()) else {
    return Err(OpError::type_("illegal invocation"));
  };
  let bits = external.value() as usize;
  if bits == 0 {
    return Err(OpError::type_("illegal invocation"));
  }
  Ok(bits as u64)
}

fn read_symbols(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<Vec<Prepared>, OpError> {
  if !value.is_object() {
    return Err(OpError::type_(SYMBOLS));
  }
  let Ok(object) = v8::Local::<v8::Object>::try_from(value) else {
    return Err(OpError::type_(SYMBOLS));
  };
  let names = own_names(scope, object)?;
  let mut prepared = Vec::with_capacity(names.len());
  for name in names {
    if name.is_empty() {
      return Err(OpError::type_(NAME_EMPTY));
    }
    if name.contains('\0') {
      return Err(OpError::type_(NAME_NUL));
    }
    let Some(field) = read_field(scope, object, &name)? else {
      return Err(OpError::type_(SYMBOLS_READ));
    };
    let signature = read_definition(scope, field)?;
    prepared.push(Prepared { name, signature });
  }
  Ok(prepared)
}

fn own_names(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
) -> Result<Vec<String>, OpError> {
  enum Outcome {
    Names(v8::Global<v8::Array>),
    Throw(v8::Global<v8::Value>),
    Missing,
  }
  let outcome = {
    v8::tc_scope!(let tc, scope);
    let object = v8::Local::new(tc, object);
    let mut builder = v8::GetPropertyNamesArgsBuilder::new();
    let listed = builder
      .mode(v8::KeyCollectionMode::OwnOnly)
      .property_filter(
        v8::PropertyFilter::ONLY_ENUMERABLE | v8::PropertyFilter::SKIP_SYMBOLS,
      )
      .key_conversion(v8::KeyConversionMode::ConvertToString)
      .build();
    match object.get_own_property_names(tc, listed) {
      Some(names) => Outcome::Names(v8::Global::new(tc, names)),
      None => {
        if let Some(exception) = tc.exception() {
          Outcome::Throw(v8::Global::new(tc, exception))
        } else {
          Outcome::Missing
        }
      }
    }
  };
  let names = match outcome {
    Outcome::Names(names) => v8::Local::new(scope, names),
    Outcome::Throw(exception) => {
      return Err(OpError::thrown(scope, v8::Local::new(scope, exception)));
    }
    Outcome::Missing => return Err(OpError::type_(SYMBOLS_READ)),
  };
  let length = names.length();
  if length > MAX_SYMBOLS {
    return Err(OpError::type_(TOO_MANY));
  }
  let mut keys = Vec::with_capacity(length as usize);
  for index in 0..length {
    let value = array_element(scope, names, index)?;
    keys.push(string_primitive(scope, value, NAME_TYPE)?);
  }
  Ok(keys)
}

fn read_definition(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<Signature, OpError> {
  if !value.is_object() || value.is_array() {
    return Err(OpError::type_(DEFINITION));
  }
  let Ok(object) = v8::Local::<v8::Object>::try_from(value) else {
    return Err(OpError::type_(DEFINITION));
  };
  let params = read_parameters(scope, object)?;
  let result = read_result(scope, object)?;
  signature(&params, result).ok_or_else(|| OpError::type_(UNSUPPORTED))
}

fn read_parameters(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
) -> Result<Vec<Kind>, OpError> {
  let Some(field) = read_field(scope, object, "parameters")? else {
    return Err(OpError::type_(SYMBOLS_READ));
  };
  if field.is_null_or_undefined() {
    return Ok(Vec::new());
  }
  let Ok(array) = v8::Local::<v8::Array>::try_from(field) else {
    return Err(OpError::type_(UNSUPPORTED));
  };
  let length = array.length();
  if length > 2 {
    return Err(OpError::type_(UNSUPPORTED));
  }
  let mut kinds = Vec::with_capacity(length as usize);
  for index in 0..length {
    let value = array_element(scope, array, index)?;
    let text = string_primitive(scope, value, UNSUPPORTED)?;
    kinds.push(parse_kind(&text).ok_or_else(|| OpError::type_(UNSUPPORTED))?);
  }
  Ok(kinds)
}

fn read_result(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
) -> Result<Kind, OpError> {
  let Some(field) = read_field(scope, object, "result")? else {
    return Err(OpError::type_(SYMBOLS_READ));
  };
  if field.is_null_or_undefined() {
    return Ok(Kind::Void);
  }
  let text = string_primitive(scope, field, UNSUPPORTED)?;
  match text.as_str() {
    "void" => Ok(Kind::Void),
    "i32" => Ok(Kind::I32),
    "f64" => Ok(Kind::F64),
    _ => Err(OpError::type_(UNSUPPORTED)),
  }
}

fn parse_kind(text: &str) -> Option<Kind> {
  match text {
    "i32" => Some(Kind::I32),
    "f64" => Some(Kind::F64),
    _ => None,
  }
}

fn signature(params: &[Kind], result: Kind) -> Option<Signature> {
  if params.len() > 2 {
    return None;
  }
  if params.len() > 1 && params.iter().any(|kind| *kind != params[0]) {
    return None;
  }
  let param = params.first().copied();
  match (params.len(), param, result) {
    (0, None, Kind::Void) => Some(Signature::NoneToVoid),
    (0, None, Kind::I32) => Some(Signature::NoneToI32),
    (0, None, Kind::F64) => Some(Signature::NoneToF64),
    (1, Some(Kind::I32), Kind::I32) => Some(Signature::I32ToI32),
    (2, Some(Kind::I32), Kind::I32) => Some(Signature::I32I32ToI32),
    (1, Some(Kind::I32), Kind::Void) => Some(Signature::I32ToVoid),
    (2, Some(Kind::I32), Kind::Void) => Some(Signature::I32I32ToVoid),
    (1, Some(Kind::F64), Kind::F64) => Some(Signature::F64ToF64),
    (2, Some(Kind::F64), Kind::F64) => Some(Signature::F64F64ToF64),
    (1, Some(Kind::F64), Kind::Void) => Some(Signature::F64ToVoid),
    (2, Some(Kind::F64), Kind::Void) => Some(Signature::F64F64ToVoid),
    _ => None,
  }
}

fn resolve_all(handle: usize, prepared: &[Prepared]) -> Option<Vec<usize>> {
  let mut pointers = Vec::with_capacity(prepared.len());
  for item in prepared {
    pointers.push(lookup(handle, &item.name)?);
  }
  Some(pointers)
}

fn build_symbols(
  scope: &mut v8::PinScope,
  prepared: &[Prepared],
  pointers: &[usize],
) -> Result<(v8::Global<v8::Object>, Vec<Resolved>), OpError> {
  let object = v8::Object::new(scope);
  let mut resolved = Vec::with_capacity(prepared.len());
  for (item, pointer) in prepared.iter().zip(pointers.iter().copied()) {
    let id = next_id();
    let Some(bits) = usize::try_from(id).ok() else {
      return Err(plain_error(scope, OPEN_FAIL));
    };
    let data = v8::External::new(scope, bits as *mut c_void);
    let Some(function) = v8::Function::builder(symbol_callback)
      .data(data.into())
      .length(item.signature.arity())
      .constructor_behavior(v8::ConstructorBehavior::Throw)
      .build(scope)
    else {
      return Err(plain_error(scope, OPEN_FAIL));
    };
    let Some(name) = v8::String::new(scope, &item.name) else {
      return Err(plain_error(scope, OPEN_FAIL));
    };
    function.set_name(name);
    if object.create_data_property(scope, name.into(), function.into())
      != Some(true)
    {
      return Err(plain_error(scope, OPEN_FAIL));
    }
    resolved.push(Resolved {
      id,
      pointer,
      signature: item.signature,
    });
  }
  Ok((v8::Global::new(scope, object), resolved))
}

fn publish(id: u64, handle: usize, resolved: &[Resolved]) {
  let mut ids = Vec::with_capacity(resolved.len());
  SYMBOLS_MAP.with(|symbols| {
    let mut symbols = symbols.borrow_mut();
    for item in resolved {
      ids.push(item.id);
      symbols.insert(
        item.id,
        Symbol {
          pointer: item.pointer,
          signature: item.signature,
        },
      );
    }
  });
  OPEN.with(|open| {
    open.borrow_mut().insert(id, Opened { handle, ids });
  });
}

fn release(id: u64) {
  let opened = OPEN.with(|open| open.borrow_mut().remove(&id));
  let Some(opened) = opened else {
    return;
  };
  SYMBOLS_MAP.with(|symbols| {
    let mut symbols = symbols.borrow_mut();
    for id in opened.ids {
      symbols.remove(&id);
    }
  });
  unload(opened.handle);
}

fn lookup_signature(id: u64) -> Option<Signature> {
  SYMBOLS_MAP
    .with(|symbols| symbols.borrow().get(&id).map(|symbol| symbol.signature))
}

fn lookup_pointer(id: u64) -> Option<usize> {
  SYMBOLS_MAP
    .with(|symbols| symbols.borrow().get(&id).map(|symbol| symbol.pointer))
}

fn convert_args(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
  signature: Signature,
) -> Result<CallArgs, OpError> {
  let mut call = CallArgs {
    i0: 0,
    i1: 0,
    f0: 0.0,
    f1: 0.0,
  };
  match signature {
    Signature::I32I32ToI32 | Signature::I32I32ToVoid => {
      call.i0 = to_i32(scope, args.get(0))?;
      call.i1 = to_i32(scope, args.get(1))?;
    }
    Signature::I32ToI32 | Signature::I32ToVoid => {
      call.i0 = to_i32(scope, args.get(0))?;
    }
    Signature::F64F64ToF64 | Signature::F64F64ToVoid => {
      call.f0 = to_f64(scope, args.get(0))?;
      call.f1 = to_f64(scope, args.get(1))?;
    }
    Signature::F64ToF64 | Signature::F64ToVoid => {
      call.f0 = to_f64(scope, args.get(0))?;
    }
    Signature::NoneToVoid | Signature::NoneToI32 | Signature::NoneToF64 => {}
  }
  Ok(call)
}

fn to_i32(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<i32, OpError> {
  enum Outcome {
    Value(i32),
    Throw(v8::Global<v8::Value>),
    Missing,
  }
  let outcome = {
    v8::tc_scope!(let tc, scope);
    let value = v8::Local::new(tc, value);
    match value.int32_value(tc) {
      Some(number) => Outcome::Value(number),
      None => {
        if let Some(exception) = tc.exception() {
          Outcome::Throw(v8::Global::new(tc, exception))
        } else {
          Outcome::Missing
        }
      }
    }
  };
  match outcome {
    Outcome::Value(number) => Ok(number),
    Outcome::Throw(exception) => {
      Err(OpError::thrown(scope, v8::Local::new(scope, exception)))
    }
    Outcome::Missing => Err(OpError::type_(ARGUMENT)),
  }
}

fn to_f64(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<f64, OpError> {
  enum Outcome {
    Value(f64),
    Throw(v8::Global<v8::Value>),
    Missing,
  }
  let outcome = {
    v8::tc_scope!(let tc, scope);
    let value = v8::Local::new(tc, value);
    match value.number_value(tc) {
      Some(number) => Outcome::Value(number),
      None => {
        if let Some(exception) = tc.exception() {
          Outcome::Throw(v8::Global::new(tc, exception))
        } else {
          Outcome::Missing
        }
      }
    }
  };
  match outcome {
    Outcome::Value(number) => Ok(number),
    Outcome::Throw(exception) => {
      Err(OpError::thrown(scope, v8::Local::new(scope, exception)))
    }
    Outcome::Missing => Err(OpError::type_(ARGUMENT)),
  }
}

fn invoke(pointer: usize, signature: Signature, args: CallArgs) -> Option<f64> {
  unsafe {
    match signature {
      Signature::NoneToVoid => {
        let function: unsafe extern "C" fn() = fn_ptr(pointer);
        function();
        None
      }
      Signature::NoneToI32 => {
        let function: unsafe extern "C" fn() -> i32 = fn_ptr(pointer);
        Some(i32_bits(function()))
      }
      Signature::NoneToF64 => {
        let function: unsafe extern "C" fn() -> f64 = fn_ptr(pointer);
        Some(function())
      }
      Signature::I32ToI32 => {
        let function: unsafe extern "C" fn(i32) -> i32 = fn_ptr(pointer);
        Some(i32_bits(function(args.i0)))
      }
      Signature::I32I32ToI32 => {
        let function: unsafe extern "C" fn(i32, i32) -> i32 = fn_ptr(pointer);
        Some(i32_bits(function(args.i0, args.i1)))
      }
      Signature::I32ToVoid => {
        let function: unsafe extern "C" fn(i32) = fn_ptr(pointer);
        function(args.i0);
        None
      }
      Signature::I32I32ToVoid => {
        let function: unsafe extern "C" fn(i32, i32) = fn_ptr(pointer);
        function(args.i0, args.i1);
        None
      }
      Signature::F64ToF64 => {
        let function: unsafe extern "C" fn(f64) -> f64 = fn_ptr(pointer);
        Some(function(args.f0))
      }
      Signature::F64F64ToF64 => {
        let function: unsafe extern "C" fn(f64, f64) -> f64 = fn_ptr(pointer);
        Some(function(args.f0, args.f1))
      }
      Signature::F64ToVoid => {
        let function: unsafe extern "C" fn(f64) = fn_ptr(pointer);
        function(args.f0);
        None
      }
      Signature::F64F64ToVoid => {
        let function: unsafe extern "C" fn(f64, f64) = fn_ptr(pointer);
        function(args.f0, args.f1);
        None
      }
    }
  }
}

fn i32_bits(value: i32) -> f64 {
  f64::from(value)
}

/// `pointer` is the address of an `extern "C"` function of type `T`.
fn fn_ptr<T>(pointer: usize) -> T {
  debug_assert_eq!(std::mem::size_of::<T>(), std::mem::size_of::<usize>());
  unsafe { std::mem::transmute_copy(&pointer) }
}

fn next_id() -> u64 {
  loop {
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    if id != 0 {
      return id;
    }
  }
}

fn read_field<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  object: v8::Local<'_, v8::Object>,
  name: &str,
) -> Result<Option<v8::Local<'s, v8::Value>>, OpError> {
  enum Outcome {
    Value(v8::Global<v8::Value>),
    Throw(v8::Global<v8::Value>),
    Missing,
  }
  let outcome = {
    v8::tc_scope!(let tc, scope);
    let Some(key) = v8::String::new(tc, name) else {
      return Err(OpError::type_(SYMBOLS_READ));
    };
    let object = v8::Local::new(tc, object);
    match object.get(tc, key.into()) {
      Some(value) => Outcome::Value(v8::Global::new(tc, value)),
      None => {
        if let Some(exception) = tc.exception() {
          Outcome::Throw(v8::Global::new(tc, exception))
        } else {
          Outcome::Missing
        }
      }
    }
  };
  match outcome {
    Outcome::Value(value) => Ok(Some(v8::Local::new(scope, value))),
    Outcome::Throw(exception) => {
      Err(OpError::thrown(scope, v8::Local::new(scope, exception)))
    }
    Outcome::Missing => Err(OpError::type_(SYMBOLS_READ)),
  }
}

fn array_element<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  array: v8::Local<'_, v8::Array>,
  index: u32,
) -> Result<v8::Local<'s, v8::Value>, OpError> {
  enum Outcome {
    Value(v8::Global<v8::Value>),
    Throw(v8::Global<v8::Value>),
    Missing,
  }
  let outcome = {
    v8::tc_scope!(let tc, scope);
    let array = v8::Local::new(tc, array);
    match array.get_index(tc, index) {
      Some(value) => Outcome::Value(v8::Global::new(tc, value)),
      None => {
        if let Some(exception) = tc.exception() {
          Outcome::Throw(v8::Global::new(tc, exception))
        } else {
          Outcome::Missing
        }
      }
    }
  };
  match outcome {
    Outcome::Value(value) => Ok(v8::Local::new(scope, value)),
    Outcome::Throw(exception) => {
      Err(OpError::thrown(scope, v8::Local::new(scope, exception)))
    }
    Outcome::Missing => Err(OpError::type_(SYMBOLS_READ)),
  }
}

fn string_primitive(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
  message: &'static str,
) -> Result<String, OpError> {
  if !value.is_string() {
    return Err(OpError::type_(message));
  }
  let Ok(text) = v8::Local::<v8::String>::try_from(value) else {
    return Err(OpError::type_(message));
  };
  Ok(text.to_rust_string_lossy(scope))
}

fn plain_error(scope: &mut v8::PinScope, message: &str) -> OpError {
  let Some(text) = v8::String::new(scope, message) else {
    return OpError::type_(message);
  };
  let error = v8::Exception::error(scope, text);
  OpError::thrown(scope, error)
}

fn move_constructor(scope: &mut v8::PinScope, name: &str) {
  let global = scope.get_current_context().global(scope);
  let Some(key) = v8::String::new(scope, name) else {
    return;
  };
  let Some(constructor) = global.get(scope, key.into()) else {
    return;
  };
  let _ = global.delete(scope, key.into());
  let yuzuko = crate::namespace(scope, "Yuzuko");
  yuzuko.set(scope, key.into(), constructor);
}

fn define_fn(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
  length: i32,
  callback: impl v8::MapFnTo<v8::FunctionCallback>,
) {
  let function = v8::Function::builder(callback)
    .length(length)
    .constructor_behavior(v8::ConstructorBehavior::Throw)
    .build(scope)
    .unwrap_or_else(|| panic!("create function {name}"));
  let key = v8::String::new(scope, name).expect(name);
  function.set_name(key);
  object.set(scope, key.into(), function.into());
}

#[cfg(unix)]
fn open_library(path: &str) -> Option<usize> {
  let path = CString::new(path).ok()?;
  let handle =
    unsafe { libc::dlopen(path.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL) };
  if handle.is_null() {
    None
  } else {
    Some(handle as usize)
  }
}

#[cfg(unix)]
fn lookup(handle: usize, name: &str) -> Option<usize> {
  if handle == 0 {
    return None;
  }
  let name = CString::new(name).ok()?;
  let symbol = unsafe { libc::dlsym(handle as *mut c_void, name.as_ptr()) };
  if symbol.is_null() {
    None
  } else {
    Some(symbol as usize)
  }
}

#[cfg(unix)]
fn unload(handle: usize) {
  if handle == 0 {
    return;
  }
  unsafe {
    libc::dlclose(handle as *mut c_void);
  }
}

#[cfg(windows)]
fn open_library(path: &str) -> Option<usize> {
  use windows_sys::Win32::System::LibraryLoader::LoadLibraryW;
  let mut wide: Vec<u16> = path.encode_utf16().collect();
  wide.push(0);
  let handle = unsafe { LoadLibraryW(wide.as_ptr()) };
  if handle.is_null() {
    None
  } else {
    Some(handle as usize)
  }
}

#[cfg(windows)]
fn lookup(handle: usize, name: &str) -> Option<usize> {
  use windows_sys::Win32::System::LibraryLoader::GetProcAddress;
  if handle == 0 {
    return None;
  }
  let name = CString::new(name).ok()?;
  let symbol = unsafe {
    GetProcAddress(handle as *mut c_void, name.as_ptr().cast::<u8>())
  }?;
  let pointer = symbol as *const () as usize;
  if pointer == 0 { None } else { Some(pointer) }
}

#[cfg(windows)]
fn unload(handle: usize) {
  use windows_sys::Win32::Foundation::FreeLibrary;
  if handle == 0 {
    return;
  }
  unsafe {
    let _ = FreeLibrary(handle as *mut c_void);
  }
}

#[cfg(not(any(unix, windows)))]
fn open_library(_path: &str) -> Option<usize> {
  None
}

#[cfg(not(any(unix, windows)))]
fn lookup(_handle: usize, _name: &str) -> Option<usize> {
  None
}

#[cfg(not(any(unix, windows)))]
fn unload(_handle: usize) {}
