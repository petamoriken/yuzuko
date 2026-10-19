//! Conversions used by `#[op]`. Web APIs use V8's `ToString` and `ToNumber`.
//! `DOMException` is constructed from the realm's captured constructor.

use crate::host;

pub struct This(pub i32);

pub trait FromV8: Sized {
  fn from_v8(
    scope: &mut v8::PinScope,
    value: v8::Local<v8::Value>,
  ) -> Result<Self, OpError>;
}

pub trait ToV8 {
  fn to_v8(self, scope: &mut v8::PinScope, ret: &mut v8::ReturnValue);
}

pub trait OpException {
  fn throw(self, scope: &mut v8::PinScope);
}

#[derive(Debug)]
pub enum OpError {
  Type(String),
  Range(String),
  Dom {
    name: String,
    message: String,
  },
  /// An already-built exception, such as `AbortSignal.reason`.
  Thrown(Box<v8::Global<v8::Value>>),
}

impl OpError {
  #[inline]
  pub fn type_(message: impl Into<String>) -> Self {
    Self::Type(message.into())
  }

  #[inline]
  pub fn range(message: impl Into<String>) -> Self {
    Self::Range(message.into())
  }

  #[inline]
  pub fn dom(name: impl Into<String>, message: impl Into<String>) -> Self {
    Self::Dom {
      name: name.into(),
      message: message.into(),
    }
  }

  #[inline]
  pub fn thrown(scope: &mut v8::PinScope, value: v8::Local<v8::Value>) -> Self {
    Self::Thrown(Box::new(v8::Global::new(scope, value)))
  }
}

impl OpException for OpError {
  #[inline]
  fn throw(self, scope: &mut v8::PinScope) {
    match self {
      Self::Type(message) => throw_builtin(scope, message, false),
      Self::Range(message) => throw_builtin(scope, message, true),
      Self::Dom { name, message } => throw_dom(scope, &name, &message),
      Self::Thrown(value) => {
        let value = v8::Local::new(scope, *value);
        scope.throw_exception(value);
      }
    }
  }
}

#[inline]
pub fn this_id(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
) -> Result<i32, OpError> {
  let Some(field) = object.get_internal_field(scope, 0) else {
    return Err(OpError::type_("illegal invocation"));
  };
  let Ok(external) = v8::Local::<v8::External>::try_from(field) else {
    return Err(OpError::type_("illegal invocation"));
  };
  Ok(external.value() as isize as i32)
}

#[inline]
fn caught_exception(
  scope: &mut v8::PinnedRef<v8::TryCatch<v8::HandleScope>>,
) -> v8::Global<v8::Value> {
  let exception = scope
    .exception()
    .unwrap_or_else(|| v8::undefined(scope).into());
  v8::Global::new(scope, exception)
}

#[inline]
fn rethrow_exception(
  scope: &mut v8::PinScope,
  exception: v8::Global<v8::Value>,
) -> OpError {
  let exception = v8::Local::new(scope, exception);
  OpError::thrown(scope, exception)
}

impl FromV8 for String {
  #[inline]
  fn from_v8(
    scope: &mut v8::PinScope,
    value: v8::Local<v8::Value>,
  ) -> Result<Self, OpError> {
    let captured = {
      v8::tc_scope!(let tc, scope);
      match value.to_string(tc) {
        Some(text) => Ok(text.to_rust_string_lossy(tc)),
        None => Err(caught_exception(tc)),
      }
    };
    match captured {
      Ok(text) => Ok(text),
      Err(exception) => Err(rethrow_exception(scope, exception)),
    }
  }
}

impl FromV8 for f64 {
  #[inline]
  fn from_v8(
    scope: &mut v8::PinScope,
    value: v8::Local<v8::Value>,
  ) -> Result<Self, OpError> {
    let captured = {
      v8::tc_scope!(let tc, scope);
      match value.number_value(tc) {
        Some(number) => Ok(number),
        None => Err(caught_exception(tc)),
      }
    };
    match captured {
      Ok(number) => Ok(number),
      Err(exception) => Err(rethrow_exception(scope, exception)),
    }
  }
}

impl FromV8 for i32 {
  #[inline]
  fn from_v8(
    scope: &mut v8::PinScope,
    value: v8::Local<v8::Value>,
  ) -> Result<Self, OpError> {
    value
      .int32_value(scope)
      .ok_or_else(|| OpError::type_("failed to convert to an integer"))
  }
}

impl FromV8 for u32 {
  #[inline]
  fn from_v8(
    scope: &mut v8::PinScope,
    value: v8::Local<v8::Value>,
  ) -> Result<Self, OpError> {
    Ok(to_uint32(f64::from_v8(scope, value)?))
  }
}

/// WebIDL `unsigned long`: `ToNumber`, truncate, then modulo 2^32.
#[inline]
fn to_uint32(number: f64) -> u32 {
  if !number.is_finite() || number == 0.0 {
    return 0;
  }
  let modulo = number.trunc() % 4_294_967_296.0;
  let positive = if modulo < 0.0 {
    modulo + 4_294_967_296.0
  } else {
    modulo
  };
  positive as u32
}

impl FromV8 for bool {
  #[inline]
  fn from_v8(
    scope: &mut v8::PinScope,
    value: v8::Local<v8::Value>,
  ) -> Result<Self, OpError> {
    Ok(value.boolean_value(scope))
  }
}

impl<T: FromV8> FromV8 for Option<T> {
  #[inline]
  fn from_v8(
    scope: &mut v8::PinScope,
    value: v8::Local<v8::Value>,
  ) -> Result<Self, OpError> {
    if value.is_undefined() {
      Ok(None)
    } else {
      T::from_v8(scope, value).map(Some)
    }
  }
}

impl ToV8 for () {
  #[inline]
  fn to_v8(self, _scope: &mut v8::PinScope, _ret: &mut v8::ReturnValue) {}
}

impl ToV8 for String {
  #[inline]
  fn to_v8(self, scope: &mut v8::PinScope, ret: &mut v8::ReturnValue) {
    if let Some(value) = v8::String::new(scope, &self) {
      ret.set(value.into());
    }
  }
}

impl ToV8 for f64 {
  #[inline]
  fn to_v8(self, scope: &mut v8::PinScope, ret: &mut v8::ReturnValue) {
    ret.set(v8::Number::new(scope, self).into());
  }
}

impl ToV8 for i32 {
  #[inline]
  fn to_v8(self, scope: &mut v8::PinScope, ret: &mut v8::ReturnValue) {
    ret.set(v8::Integer::new(scope, self).into());
  }
}

impl ToV8 for u32 {
  #[inline]
  fn to_v8(self, scope: &mut v8::PinScope, ret: &mut v8::ReturnValue) {
    ret.set(v8::Number::new(scope, f64::from(self)).into());
  }
}

impl ToV8 for bool {
  #[inline]
  fn to_v8(self, scope: &mut v8::PinScope, ret: &mut v8::ReturnValue) {
    ret.set(v8::Boolean::new(scope, self).into());
  }
}

/// Owned bytes returned to JavaScript as a `Uint8Array`.
pub struct Bytes(pub Vec<u8>);

impl ToV8 for Bytes {
  #[inline]
  fn to_v8(self, scope: &mut v8::PinScope, ret: &mut v8::ReturnValue) {
    let length = self.0.len();
    let store =
      v8::ArrayBuffer::new_backing_store_from_vec(self.0).make_shared();
    let buffer = v8::ArrayBuffer::with_backing_store(scope, &store);
    if let Some(view) = v8::Uint8Array::new(scope, buffer, 0, length) {
      ret.set(view.into());
    }
  }
}

impl<T: ToV8> ToV8 for Option<T> {
  #[inline]
  fn to_v8(self, scope: &mut v8::PinScope, ret: &mut v8::ReturnValue) {
    match self {
      Some(value) => value.to_v8(scope, ret),
      None => ret.set(v8::null(scope).into()),
    }
  }
}

impl ToV8 for Vec<String> {
  #[inline]
  fn to_v8(self, scope: &mut v8::PinScope, ret: &mut v8::ReturnValue) {
    let array = v8::Array::new(scope, self.len() as i32);
    for (index, item) in self.iter().enumerate() {
      if let Some(text) = v8::String::new(scope, item) {
        array.set_index(scope, index as u32, text.into());
      }
    }
    ret.set(array.into());
  }
}

/// A cppgc instance created with the class's cached template.
pub struct New<T>(pub T);

impl<T> ToV8 for New<T>
where
  T: v8::cppgc::GarbageCollected + 'static,
{
  #[inline]
  fn to_v8(self, scope: &mut v8::PinScope, ret: &mut v8::ReturnValue) {
    let object = unsafe { wrap_instance(scope, self.0) };
    ret.set(object.into());
  }
}

/// An existing JavaScript value returned from an `#[op]`.
pub struct Js(pub v8::Global<v8::Value>);

impl Js {
  #[inline]
  pub fn null(scope: &mut v8::PinScope) -> Self {
    let null: v8::Local<v8::Value> = v8::null(scope).into();
    Self(v8::Global::new(scope, null))
  }
}

impl ToV8 for Js {
  #[inline]
  fn to_v8(self, scope: &mut v8::PinScope, ret: &mut v8::ReturnValue) {
    ret.set(v8::Local::new(scope, self.0));
  }
}

/// Constructor used when an `#[op]` class has no `#[constructor]`.
pub fn illegal_constructor(
  scope: &mut v8::PinScope,
  _args: v8::FunctionCallbackArguments,
  mut _ret: v8::ReturnValue,
) {
  OpException::throw(OpError::type_("Illegal constructor"), scope);
}

/// Every cppgc class shares one wrapper tag. The Rust type is stored beside
/// the value so a method can reject the wrong receiver without a unique tag.
const HOST_TAG: u16 = 1;

/// `value` starts at the same offset for every `T`, so a subclass can be
/// read through its `base` field. `#[op(inherit = Parent)]` requires
/// `#[repr(C)]` and a first field named `base`.
const HOST_VALUE_OFFSET: usize = 16;
const HOST_PAD: usize =
  HOST_VALUE_OFFSET - std::mem::size_of::<std::any::TypeId>();

#[repr(C, align(16))]
pub struct HostObject<T: v8::cppgc::GarbageCollected> {
  type_id: std::any::TypeId,
  _pad: [u8; HOST_PAD],
  pub value: T,
}

unsafe impl<T: v8::cppgc::GarbageCollected> v8::cppgc::GarbageCollected
  for HostObject<T>
{
  fn trace(&self, visitor: &mut v8::cppgc::Visitor) {
    self.value.trace(visitor);
  }

  fn get_name(&self) -> &'static std::ffi::CStr {
    self.value.get_name()
  }
}

#[inline]
pub fn cached_template<'s, T: 'static>(
  scope: &mut v8::PinScope<'s, '_>,
) -> Option<v8::Local<'s, v8::FunctionTemplate>> {
  let id = std::any::TypeId::of::<T>();
  let global = host::with(|state| state.templates.get(&id).cloned())?;
  Some(v8::Local::new(scope, global))
}

pub fn cache_template<T: 'static>(
  scope: &mut v8::PinScope,
  template: v8::Local<v8::FunctionTemplate>,
) {
  let id = std::any::TypeId::of::<T>();
  let global = v8::Global::new(scope, template);
  host::with_mut(|state| {
    state.templates.insert(id, global);
  });
}

/// Creates an instance of the cached class template and wraps `value` in it.
///
/// # Safety
///
/// `T` must be the class whose `template` was cached for this realm.
#[inline]
pub unsafe fn wrap_instance<'s, T>(
  scope: &mut v8::PinScope<'s, '_>,
  value: T,
) -> v8::Local<'s, v8::Object>
where
  T: v8::cppgc::GarbageCollected + 'static,
{
  unsafe { wrap_receiver(scope, None, value) }
}

/// Wraps `value` in `receiver` when that object already has internal fields.
///
/// A `class extends` call arrives with `new.target`'s object. Reusing it keeps
/// the subclass prototype. Other callers pass `None` and get a fresh instance.
///
/// # Safety
///
/// `T` must be the class whose `template` was cached for this realm.
pub unsafe fn wrap_receiver<'s, T>(
  scope: &mut v8::PinScope<'s, '_>,
  receiver: Option<v8::Local<'s, v8::Object>>,
  value: T,
) -> v8::Local<'s, v8::Object>
where
  T: v8::cppgc::GarbageCollected + 'static,
{
  let template = cached_template::<T>(scope).expect("class template");
  let object = match receiver {
    Some(object) => object,
    None => template
      .instance_template(scope)
      .new_instance(scope)
      .expect("class instance"),
  };
  let heap = scope.get_cpp_heap().expect("cppgc heap");
  let member = unsafe {
    v8::cppgc::make_garbage_collected(
      heap,
      HostObject {
        type_id: std::any::TypeId::of::<T>(),
        _pad: [0; HOST_PAD],
        value,
      },
    )
  };
  unsafe {
    v8::Object::wrap::<HOST_TAG, HostObject<T>>(scope, object, &member);
  }
  object
}

/// Records that `Derived`'s `base` field is a `Base` at `offset` bytes.
/// Parent templates register first, so a grandchild also reaches `Base`.
pub fn register_subclass<Derived: 'static, Base: 'static>(offset: usize) {
  let derived = std::any::TypeId::of::<Derived>();
  let base = std::any::TypeId::of::<Base>();
  let mut bases = subclass_bases();
  if bases.iter().any(|(found_derived, found_base, _)| {
    *found_derived == derived && *found_base == base
  }) {
    return;
  }
  let inherited: Vec<_> = bases
    .iter()
    .filter(|(found_derived, _, _)| *found_derived == base)
    .map(|(_, grand, grand_offset)| (derived, *grand, offset + grand_offset))
    .collect();
  bases.push((derived, base, offset));
  for entry in inherited {
    let seen = bases.iter().any(|(found_derived, found_base, _)| {
      *found_derived == entry.0 && *found_base == entry.1
    });
    if !seen {
      bases.push(entry);
    }
  }
}

/// Borrows the wrapped value, or the `base` field when `object` is a subclass.
///
/// The reference stays valid while `object` is on the stack and no garbage
/// collection moves the allocation.
pub struct HostRef<T> {
  _member: v8::cppgc::UnsafePtr<HostObject<Marker>>,
  ptr: *const T,
}

impl<T> HostRef<T> {
  pub fn get(&self) -> &T {
    unsafe { &*self.ptr }
  }
}

/// Same alignment as every `HostObject`, so unwrapping with this type still
/// points at the real wrapper. Only the shared `type_id` prefix is read.
#[repr(C, align(16))]
struct Marker;

unsafe impl v8::cppgc::GarbageCollected for Marker {
  fn trace(&self, _visitor: &mut v8::cppgc::Visitor) {}

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"Host"
  }
}

const _: () = {
  assert!(std::mem::offset_of!(HostObject<Marker>, value) == HOST_VALUE_OFFSET);
};

/// The cppgc allocation that wraps `T`, or `None` when `object` is not that
/// class.
///
/// # Safety
///
/// Keep the pointer on the stack, or move it into a `Member` or
/// `WeakMember`, before the next garbage collection.
pub unsafe fn host_gc<T>(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
) -> Option<v8::cppgc::UnsafePtr<HostObject<T>>>
where
  T: v8::cppgc::GarbageCollected + 'static,
{
  if !object.is_api_wrapper() {
    return None;
  }
  let marker = unsafe {
    v8::Object::unwrap::<HOST_TAG, HostObject<Marker>>(scope, object)
  }?;
  let raw =
    unsafe { marker.as_ref() } as *const HostObject<Marker> as *const u8;
  let id = unsafe { raw.cast::<std::any::TypeId>().read() };
  if id != std::any::TypeId::of::<T>() {
    return None;
  }
  unsafe { v8::Object::unwrap::<HOST_TAG, HostObject<T>>(scope, object) }
}

/// # Safety
///
/// `object` must be a live handle. The returned borrow lasts until `object`
/// is collected or moved.
#[inline]
pub unsafe fn host_ref<T: 'static>(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
) -> Option<HostRef<T>> {
  if !object.is_api_wrapper() {
    return None;
  }
  let member = unsafe {
    v8::Object::unwrap::<HOST_TAG, HostObject<Marker>>(scope, object)
  }?;
  // Every host object shares this prefix. The concrete value is not read
  // through `Marker`.
  let raw =
    unsafe { member.as_ref() } as *const HostObject<Marker> as *const u8;
  let id = unsafe { raw.cast::<std::any::TypeId>().read() };
  let value = unsafe { raw.add(HOST_VALUE_OFFSET) };
  let ptr = if id == std::any::TypeId::of::<T>() {
    value.cast::<T>()
  } else {
    let offset = base_offset(id, std::any::TypeId::of::<T>())?;
    unsafe { value.add(offset) }.cast::<T>()
  };
  Some(HostRef {
    _member: member,
    ptr,
  })
}

#[inline]
fn base_offset(
  derived: std::any::TypeId,
  base: std::any::TypeId,
) -> Option<usize> {
  subclass_bases()
    .iter()
    .find_map(|(found_derived, found_base, offset)| {
      (*found_derived == derived && *found_base == base).then_some(*offset)
    })
}

fn subclass_bases() -> std::sync::MutexGuard<
  'static,
  Vec<(std::any::TypeId, std::any::TypeId, usize)>,
> {
  use std::sync::Mutex;
  static BASES: Mutex<Vec<(std::any::TypeId, std::any::TypeId, usize)>> =
    Mutex::new(Vec::new());
  BASES.lock().expect("cppgc subclasses")
}

/// `TracedReference::new` wants a scope whose context parameter is `()`.
/// `PinScope` derefs to that view because the context is only a `PhantomData`.
/// The lifetime on a `Local` is the handle scope that allocated it. Callback
/// signatures erase that connection, so reattach the handle to `scope`.
#[inline]
pub fn root<'s, T>(
  scope: &v8::PinScope<'s, '_>,
  value: v8::Local<'_, T>,
) -> v8::Local<'s, T> {
  let _ = scope;
  unsafe { std::mem::transmute(value) }
}

#[inline]
pub fn traced<'s, T>(
  scope: &mut v8::PinScope<'s, '_>,
  value: v8::Local<'_, T>,
) -> v8::TracedReference<T> {
  let value = root(scope, value);
  let unit: &v8::PinScope<'s, '_, ()> = scope;
  v8::TracedReference::new(unit, value)
}

#[inline]
pub fn traced_local<'s, T>(
  scope: &mut v8::PinScope<'s, '_>,
  reference: &v8::TracedReference<T>,
) -> Option<v8::Local<'s, T>> {
  let unit: &v8::PinScope<'s, '_, ()> = scope;
  reference.get(unit)
}

pub fn new_dom_exception<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  name: &str,
  message: &str,
) -> Option<v8::Local<'s, v8::Object>> {
  let constructor = host::with(|state| state.dom_exception.clone())?;
  let constructor = v8::Local::new(scope, constructor);
  let message = v8::String::new(scope, message)?;
  let name = v8::String::new(scope, name)?;
  constructor.new_instance(scope, &[message.into(), name.into()])
}

fn throw_builtin(scope: &mut v8::PinScope, message: String, range: bool) {
  let Some(message) = v8::String::new(scope, &message) else {
    return;
  };
  let error = if range {
    v8::Exception::range_error(scope, message)
  } else {
    v8::Exception::type_error(scope, message)
  };
  scope.throw_exception(error);
}

fn throw_dom(scope: &mut v8::PinScope, name: &str, message: &str) {
  if let Some(error) = new_dom_exception(scope, name, message) {
    scope.throw_exception(error.into());
    return;
  }
  throw_builtin(scope, format!("{name}: {message}"), false);
}
