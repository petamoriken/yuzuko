//! Optional `Yuzuko` foreign function calls.
//!
//! The `ffi` feature installs `dlopen` and `Library` on the main realm and
//! on a dedicated worker. The library is loaded on the caller thread. There
//! is no `libffi`. `f64` calls use one homogeneous `extern "C"` shape.
//! Integer-class calls (`i32`, `pointer`, `buffer`, and `cstring`) pass each
//! argument in a pointer-sized register. An `i32` is zero-extended, and only
//! the low 32 bits of an `i32` result are read. On the supported ABIs the
//! callee reads those low 32 bits (`edi` / `ecx` / `wN`).

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
const DATA: &str = "The data must be a BufferSource.";
const DETACHED: &str = "The buffer is detached.";
const TOO_LARGE: &str = "The buffer is too large.";
const STRING_TYPE: &str = "The string must be a string.";
const STRING_NUL: &str = "The string must not contain a null character.";
const CSTRING: &str = "The result is not a C string.";
const MAX_PARAMS: u32 = 4;
const MAX_BUFFER: usize = 2_147_483_647;
const MAX_CSTRING: usize = 1_048_576;
const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;

static NEXT: AtomicU64 = AtomicU64::new(1);

thread_local! {
  static SYMBOLS_MAP: RefCell<HashMap<u64, Symbol>> =
    RefCell::new(HashMap::new());
  static OPEN: RefCell<HashMap<u64, Opened>> = RefCell::new(HashMap::new());
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Param {
  I32,
  F64,
  Pointer,
  Buffer,
  CString,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Return {
  Void,
  I32,
  F64,
  Pointer,
  CString,
}

#[derive(Clone, Copy)]
struct Signature {
  params: [Param; 4],
  len: u8,
  result: Return,
}

impl Signature {
  fn arity(self) -> i32 {
    i32::from(self.len)
  }

  fn is_float(self) -> bool {
    self.result == Return::F64
      || (0..self.len)
        .any(|index| self.params[usize::from(index)] == Param::F64)
  }
}

enum Returned {
  Undefined,
  Number(f64),
  Bigint(u64),
  Text(String),
  Null,
}

struct IntegerCall {
  words: [usize; 4],
  strings: Vec<CString>,
  stores: Vec<v8::SharedRef<v8::BackingStore>>,
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
  crate::permissions::check_ffi(std::path::Path::new(&path))?;
  let handle =
    open_library(&path)?.ok_or_else(|| plain_error(scope, OPEN_FAIL))?;
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
    Ok(returned) => {
      if let Err(error) = set_returned(scope, &mut ret, returned) {
        OpException::throw(error, scope);
      }
    }
    Err(error) => OpException::throw(error, scope),
  }
}

fn symbol_op(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
) -> Result<Returned, OpError> {
  let id = symbol_id(args)?;
  let Some(signature) = lookup_signature(id) else {
    return Err(plain_error(scope, CLOSED));
  };
  if args.length() < signature.arity() {
    return Err(OpError::type_(ARITY));
  }
  if signature.is_float() {
    return call_float(scope, args, id, signature);
  }
  call_integer(scope, args, id, signature)
}

fn set_returned(
  scope: &mut v8::PinScope,
  ret: &mut v8::ReturnValue,
  returned: Returned,
) -> Result<(), OpError> {
  match returned {
    Returned::Undefined => ret.set(v8::undefined(scope).into()),
    Returned::Number(value) => ret.set(v8::Number::new(scope, value).into()),
    Returned::Bigint(value) => {
      let bigint = v8::BigInt::new_from_u64(scope, value);
      ret.set(bigint.into());
    }
    Returned::Null => ret.set(v8::null(scope).into()),
    Returned::Text(text) => {
      let Some(value) = v8::String::new(scope, &text) else {
        return Err(plain_error(scope, CSTRING));
      };
      ret.set(value.into());
    }
  }
  Ok(())
}

fn call_float(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
  id: u64,
  signature: Signature,
) -> Result<Returned, OpError> {
  let mut values = [0.0, 0.0];
  for index in 0..signature.len {
    values[usize::from(index)] = to_f64(scope, args.get(i32::from(index)))?;
  }
  let Some(pointer) = lookup_pointer(id) else {
    return Err(plain_error(scope, CLOSED));
  };
  Ok(invoke_float(pointer, signature, values))
}

fn call_integer(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
  id: u64,
  signature: Signature,
) -> Result<Returned, OpError> {
  // `i32` and pointer conversion can run JavaScript and may close the
  // library or detach a buffer. Take buffer pointers only after that, and
  // only while the library is still open. `call` keeps those pointers
  // alive until the result has been copied.
  let mut call = prepare_integer(scope, args, signature)?;
  let Some(pointer) = lookup_pointer(id) else {
    return Err(plain_error(scope, CLOSED));
  };
  fill_buffers(scope, args, signature, &mut call)?;
  let len = usize::from(signature.len);
  let raw = call_usize(pointer, &call.words[..len]);
  let returned = integer_result(scope, signature.result, raw)?;
  drop(call);
  Ok(returned)
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
  signature(params, result).ok_or_else(|| OpError::type_(UNSUPPORTED))
}

fn read_parameters(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
) -> Result<([Param; 4], u8), OpError> {
  let Some(field) = read_field(scope, object, "parameters")? else {
    return Err(OpError::type_(SYMBOLS_READ));
  };
  if field.is_null_or_undefined() {
    return Ok(([Param::I32; 4], 0));
  }
  let Ok(array) = v8::Local::<v8::Array>::try_from(field) else {
    return Err(OpError::type_(UNSUPPORTED));
  };
  let length = array.length();
  if length > MAX_PARAMS {
    return Err(OpError::type_(UNSUPPORTED));
  }
  let mut params = [Param::I32; 4];
  for index in 0..length {
    let value = array_element(scope, array, index)?;
    let text = string_primitive(scope, value, UNSUPPORTED)?;
    params[index as usize] =
      parse_param(&text).ok_or_else(|| OpError::type_(UNSUPPORTED))?;
  }
  Ok((params, length as u8))
}

fn read_result(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
) -> Result<Return, OpError> {
  let Some(field) = read_field(scope, object, "result")? else {
    return Err(OpError::type_(SYMBOLS_READ));
  };
  if field.is_null_or_undefined() {
    return Ok(Return::Void);
  }
  let text = string_primitive(scope, field, UNSUPPORTED)?;
  parse_return(&text).ok_or_else(|| OpError::type_(UNSUPPORTED))
}

fn parse_param(text: &str) -> Option<Param> {
  match text {
    "i32" => Some(Param::I32),
    "f64" => Some(Param::F64),
    "pointer" => Some(Param::Pointer),
    "buffer" => Some(Param::Buffer),
    "cstring" => Some(Param::CString),
    _ => None,
  }
}

fn parse_return(text: &str) -> Option<Return> {
  match text {
    "void" => Some(Return::Void),
    "i32" => Some(Return::I32),
    "f64" => Some(Return::F64),
    "pointer" => Some(Return::Pointer),
    "cstring" => Some(Return::CString),
    _ => None,
  }
}

fn signature(params: ([Param; 4], u8), result: Return) -> Option<Signature> {
  let (params, len) = params;
  let signature = Signature {
    params,
    len,
    result,
  };
  if !signature.is_float() {
    return Some(signature);
  }
  if len > 2 || (result != Return::Void && result != Return::F64) {
    return None;
  }
  for index in 0..len {
    if params[usize::from(index)] != Param::F64 {
      return None;
    }
  }
  Some(signature)
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

fn prepare_integer(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
  signature: Signature,
) -> Result<IntegerCall, OpError> {
  let mut call = IntegerCall {
    words: [0; 4],
    strings: Vec::new(),
    stores: Vec::new(),
  };
  for index in 0..signature.len {
    let param = signature.params[usize::from(index)];
    let value = args.get(i32::from(index));
    call.words[usize::from(index)] = match param {
      Param::I32 => {
        let number = to_i32(scope, value)?;
        number as u32 as usize
      }
      Param::Pointer => pointer_word(value)?,
      Param::CString => cstring_word(scope, value, &mut call.strings)?,
      Param::Buffer => 0,
      Param::F64 => unreachable!("float parameter on an integer call"),
    };
  }
  Ok(call)
}

fn fill_buffers(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
  signature: Signature,
  call: &mut IntegerCall,
) -> Result<(), OpError> {
  for index in 0..signature.len {
    if signature.params[usize::from(index)] != Param::Buffer {
      continue;
    }
    let (word, store) = buffer_word(scope, args.get(i32::from(index)))?;
    call.words[usize::from(index)] = word;
    if let Some(store) = store {
      call.stores.push(store);
    }
  }
  Ok(())
}

fn pointer_word(value: v8::Local<v8::Value>) -> Result<usize, OpError> {
  if value.is_null() {
    return Ok(0);
  }
  if value.is_big_int() {
    let Ok(bigint) = v8::Local::<v8::BigInt>::try_from(value) else {
      return Err(OpError::type_(ARGUMENT));
    };
    let (bits, lossless) = bigint.u64_value();
    if !lossless {
      return Err(OpError::type_(ARGUMENT));
    }
    return usize::try_from(bits).map_err(|_| OpError::type_(ARGUMENT));
  }
  if value.is_number() {
    let Ok(number) = v8::Local::<v8::Number>::try_from(value) else {
      return Err(OpError::type_(ARGUMENT));
    };
    return number_pointer(number.value());
  }
  Err(OpError::type_(ARGUMENT))
}

fn number_pointer(number: f64) -> Result<usize, OpError> {
  if !number.is_finite() || number < 0.0 || number > MAX_SAFE_INTEGER {
    return Err(OpError::type_(ARGUMENT));
  }
  if number.fract() != 0.0 {
    return Err(OpError::type_(ARGUMENT));
  }
  usize::try_from(number as u64).map_err(|_| OpError::type_(ARGUMENT))
}

fn cstring_word(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
  strings: &mut Vec<CString>,
) -> Result<usize, OpError> {
  if !value.is_string() {
    return Err(OpError::type_(STRING_TYPE));
  }
  let text = string_primitive(scope, value, STRING_TYPE)?;
  let cstring = CString::new(text).map_err(|_| OpError::type_(STRING_NUL))?;
  let pointer = cstring.as_ptr() as usize;
  strings.push(cstring);
  Ok(pointer)
}

fn buffer_word(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<(usize, Option<v8::SharedRef<v8::BackingStore>>), OpError> {
  if let Ok(buffer) = v8::Local::<v8::SharedArrayBuffer>::try_from(value) {
    return shared_word(&buffer);
  }
  if let Ok(view) = v8::Local::<v8::ArrayBufferView>::try_from(value) {
    return view_word(scope, view);
  }
  if let Ok(buffer) = v8::Local::<v8::ArrayBuffer>::try_from(value) {
    return array_word(&buffer);
  }
  Err(OpError::type_(DATA))
}

fn shared_word(
  buffer: &v8::SharedArrayBuffer,
) -> Result<(usize, Option<v8::SharedRef<v8::BackingStore>>), OpError> {
  let length = buffer.byte_length();
  if length > MAX_BUFFER {
    return Err(OpError::type_(TOO_LARGE));
  }
  let store = buffer.get_backing_store();
  let pointer = store.data().map(|data| data.as_ptr() as usize).unwrap_or(0);
  if length > 0 && pointer == 0 {
    return Err(OpError::type_(DETACHED));
  }
  Ok((pointer, Some(store)))
}

fn view_word(
  scope: &mut v8::PinScope,
  view: v8::Local<v8::ArrayBufferView>,
) -> Result<(usize, Option<v8::SharedRef<v8::BackingStore>>), OpError> {
  if view
    .buffer(scope)
    .is_none_or(|buffer| buffer.was_detached())
  {
    return Err(OpError::type_(DETACHED));
  }
  let length = view.byte_length();
  if length > MAX_BUFFER {
    return Err(OpError::type_(TOO_LARGE));
  }
  let store = view.get_backing_store();
  let pointer = view.data() as usize;
  if length > 0 && (store.is_none() || pointer == 0) {
    return Err(OpError::type_(DETACHED));
  }
  Ok((pointer, store))
}

fn array_word(
  buffer: &v8::ArrayBuffer,
) -> Result<(usize, Option<v8::SharedRef<v8::BackingStore>>), OpError> {
  if buffer.was_detached() {
    return Err(OpError::type_(DETACHED));
  }
  let length = buffer.byte_length();
  if length > MAX_BUFFER {
    return Err(OpError::type_(TOO_LARGE));
  }
  let store = buffer.get_backing_store();
  let pointer = buffer
    .data()
    .map(|data| data.as_ptr() as usize)
    .unwrap_or(0);
  if length > 0 && pointer == 0 {
    return Err(OpError::type_(DETACHED));
  }
  Ok((pointer, Some(store)))
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

fn integer_result(
  scope: &mut v8::PinScope,
  result: Return,
  raw: usize,
) -> Result<Returned, OpError> {
  match result {
    Return::Void => Ok(Returned::Undefined),
    Return::I32 => Ok(Returned::Number(f64::from(raw as u32 as i32))),
    Return::Pointer => Ok(Returned::Bigint(raw as u64)),
    Return::CString => match read_cstring(raw) {
      Ok(None) => Ok(Returned::Null),
      Ok(Some(text)) => Ok(Returned::Text(text)),
      Err(()) => Err(plain_error(scope, CSTRING)),
    },
    Return::F64 => unreachable!("float result on an integer call"),
  }
}

/// `pointer` addresses an integer-class `extern "C"` function of `args.len()`
/// parameters. Each argument occupies one pointer-sized register. An `i32`
/// is already zero-extended, and the callee reads the low 32 bits. The
/// returned register is the full word; an `i32` result keeps only its low
/// 32 bits. A `void` callee is called as a function that returns a word,
/// and that word is ignored.
///
/// On the supported ABIs the caller cleans the stack. Windows x64 shadow
/// space comes from this `extern "C"` call. Only the declared arguments are
/// passed.
fn call_usize(pointer: usize, args: &[usize]) -> usize {
  unsafe {
    match args {
      [] => {
        let function: unsafe extern "C" fn() -> usize = fn_ptr(pointer);
        function()
      }
      [a] => {
        let function: unsafe extern "C" fn(usize) -> usize = fn_ptr(pointer);
        function(*a)
      }
      [a, b] => {
        let function: unsafe extern "C" fn(usize, usize) -> usize =
          fn_ptr(pointer);
        function(*a, *b)
      }
      [a, b, c] => {
        let function: unsafe extern "C" fn(usize, usize, usize) -> usize =
          fn_ptr(pointer);
        function(*a, *b, *c)
      }
      [a, b, c, d] => {
        let function: unsafe extern "C" fn(
          usize,
          usize,
          usize,
          usize,
        ) -> usize = fn_ptr(pointer);
        function(*a, *b, *c, *d)
      }
      _ => unreachable!("integer call arity"),
    }
  }
}

fn invoke_float(
  pointer: usize,
  signature: Signature,
  args: [f64; 2],
) -> Returned {
  unsafe {
    match (signature.len, signature.result) {
      (0, Return::Void) => {
        let function: unsafe extern "C" fn() = fn_ptr(pointer);
        function();
        Returned::Undefined
      }
      (0, Return::F64) => {
        let function: unsafe extern "C" fn() -> f64 = fn_ptr(pointer);
        Returned::Number(function())
      }
      (1, Return::F64) => {
        let function: unsafe extern "C" fn(f64) -> f64 = fn_ptr(pointer);
        Returned::Number(function(args[0]))
      }
      (2, Return::F64) => {
        let function: unsafe extern "C" fn(f64, f64) -> f64 = fn_ptr(pointer);
        Returned::Number(function(args[0], args[1]))
      }
      (1, Return::Void) => {
        let function: unsafe extern "C" fn(f64) = fn_ptr(pointer);
        function(args[0]);
        Returned::Undefined
      }
      (2, Return::Void) => {
        let function: unsafe extern "C" fn(f64, f64) = fn_ptr(pointer);
        function(args[0], args[1]);
        Returned::Undefined
      }
      _ => unreachable!("float call shape"),
    }
  }
}

/// Reads UTF-8 bytes at `pointer` until NUL, at most `MAX_CSTRING` bytes.
///
/// `Ok(None)` is the null pointer. `Err(())` means no NUL appeared inside
/// the cap. An invalid address may fault; that is the FFI contract. The
/// scan does not use `CStr::from_ptr`, which would read without a cap.
fn read_cstring(pointer: usize) -> Result<Option<String>, ()> {
  if pointer == 0 {
    return Ok(None);
  }
  unsafe {
    let start = pointer as *const u8;
    let mut len = 0usize;
    while len < MAX_CSTRING {
      if *start.add(len) == 0 {
        let bytes = std::slice::from_raw_parts(start, len);
        return Ok(Some(String::from_utf8_lossy(bytes).into_owned()));
      }
      len += 1;
    }
  }
  Err(())
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
fn open_library(path: &str) -> Result<Option<usize>, OpError> {
  let Ok(path) = CString::new(path) else {
    return Ok(None);
  };
  let handle =
    unsafe { libc::dlopen(path.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL) };
  if handle.is_null() {
    Ok(None)
  } else {
    Ok(Some(handle as usize))
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
fn open_library(path: &str) -> Result<Option<usize>, OpError> {
  use std::os::windows::ffi::OsStrExt;

  use windows_sys::Win32::System::LibraryLoader::{
    LOAD_WITH_ALTERED_SEARCH_PATH, LoadLibraryExW, LoadLibraryW,
  };
  let load = library_load(path)?;
  let mut wide: Vec<u16> = load.path.as_os_str().encode_wide().collect();
  wide.push(0);
  // SAFETY: `wide` is a NUL-terminated path. `LOAD_WITH_ALTERED_SEARCH_PATH`
  // is passed only with an absolute path. The reserved file handle is null.
  let handle = unsafe {
    if load.altered {
      LoadLibraryExW(
        wide.as_ptr(),
        std::ptr::null_mut(),
        LOAD_WITH_ALTERED_SEARCH_PATH,
      )
    } else {
      LoadLibraryW(wide.as_ptr())
    }
  };
  if handle.is_null() {
    Ok(None)
  } else {
    Ok(Some(handle as usize))
  }
}

/// The path Windows loads, and whether the default DLL search is disabled.
///
/// A relative path such as `sub\x.dll` would otherwise match
/// `System32\sub\x.dll`. The absolute path is the one the permission check
/// named. A bare name still uses the loader search, which requires `ffi`
/// granted as a whole.
#[cfg(windows)]
struct LibraryLoad {
  path: std::path::PathBuf,
  altered: bool,
}

#[cfg(windows)]
fn library_load(path: &str) -> Result<LibraryLoad, OpError> {
  if !path.contains('/') && !path.contains('\\') {
    return Ok(LibraryLoad {
      path: std::path::PathBuf::from(path),
      altered: false,
    });
  }
  Ok(LibraryLoad {
    path: crate::permissions::anchor_path(std::path::Path::new(path))?,
    altered: true,
  })
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
fn open_library(_path: &str) -> Result<Option<usize>, OpError> {
  Ok(None)
}

#[cfg(not(any(unix, windows)))]
fn lookup(_handle: usize, _name: &str) -> Option<usize> {
  None
}

#[cfg(not(any(unix, windows)))]
fn unload(_handle: usize) {}

#[cfg(all(test, windows))]
mod tests {
  use super::library_load;

  #[test]
  fn a_relative_library_path_is_absolute() {
    let current = std::env::current_dir().unwrap();
    let relative = library_load(r"sub\x.dll").unwrap();
    assert!(relative.altered);
    assert!(relative.path.is_absolute());
    assert_eq!(relative.path, current.join(r"sub\x.dll"));

    let slashed = library_load("sub/x.dll").unwrap();
    assert!(slashed.altered);
    assert_eq!(slashed.path, current.join("sub/x.dll"));

    let bare = library_load("x.dll").unwrap();
    assert!(!bare.altered);
    assert_eq!(bare.path, std::path::Path::new("x.dll"));

    let absolute = current.join(r"sub\x.dll");
    let loaded = library_load(absolute.to_str().unwrap()).unwrap();
    assert!(loaded.altered);
    assert_eq!(loaded.path, absolute);

    let prefix: String = current.to_string_lossy().chars().take(2).collect();
    if prefix.ends_with(':') {
      let drive_relative = format!("{prefix}sub\\x.dll");
      let loaded = library_load(&drive_relative).unwrap();
      assert!(loaded.altered);
      assert_eq!(loaded.path, current.join(r"sub\x.dll"));
    }
  }
}
