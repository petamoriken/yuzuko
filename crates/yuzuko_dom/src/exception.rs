//! `DOMException`. The prototype chain joins `Error` after install because
//! cppgc inheritance cannot extend the Error intrinsic.

use yuzuko_core::op::{OpError, OpException, host_ref};
use yuzuko_macros::op;

use crate::util::key;

pub struct DOMException {
  message: String,
  name: String,
  code: u16,
}

unsafe impl v8::cppgc::GarbageCollected for DOMException {
  fn trace(&self, _visitor: &mut v8::cppgc::Visitor) {}

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"DOMException"
  }
}

#[op(name = "DOMException")]
impl DOMException {
  #[constructor]
  fn new(message: Option<String>, name: Option<String>) -> DOMException {
    let message = message.unwrap_or_default();
    let name = name.unwrap_or_else(|| "Error".to_string());
    let code = dom_code(&name);
    DOMException {
      message,
      name,
      code,
    }
  }

  #[construct_hook]
  fn finish(&self, scope: &mut v8::PinScope, object: v8::Local<v8::Object>) {
    define(
      scope,
      object,
      "message",
      &self.message,
      v8::PropertyAttribute::NONE,
    );
    define(
      scope,
      object,
      "name",
      &self.name,
      v8::PropertyAttribute::NONE,
    );
    let code = v8::Integer::new(scope, i32::from(self.code));
    let code_key = key(scope, "code");
    let _ = object.define_own_property(
      scope,
      code_key.into(),
      code.into(),
      v8::PropertyAttribute::READ_ONLY,
    );
    capture_stack(scope, object);
  }
}

pub(crate) fn install(scope: &mut v8::PinScope) {
  DOMException::install(scope);
  let constructor = DOMException::template(scope)
    .get_function(scope)
    .expect("DOMException");
  yuzuko_core::set_dom_exception(scope, constructor);
  let prototype = constructor
    .get(scope, key(scope, "prototype").into())
    .and_then(|value| v8::Local::<v8::Object>::try_from(value).ok());
  let global = scope.get_current_context().global(scope);
  let error_prototype = global
    .get(scope, key(scope, "Error").into())
    .and_then(|value| v8::Local::<v8::Object>::try_from(value).ok())
    .and_then(|error| error.get(scope, key(scope, "prototype").into()));
  if let (Some(prototype), Some(error_prototype)) = (prototype, error_prototype)
  {
    let _ = prototype.set_prototype(scope, error_prototype);
  }
  let constructor_object =
    v8::Local::<v8::Object>::try_from(v8::Local::<v8::Value>::from(
      constructor,
    ))
    .expect("DOMException");
  define_legacy_codes(scope, &constructor_object);
  if let Some(prototype) = constructor
    .get(scope, key(scope, "prototype").into())
    .and_then(|value| v8::Local::<v8::Object>::try_from(value).ok())
  {
    // Own getters throw when `this` is the prototype. Record conversion of
    // `DOMException.prototype` hits them and raises TypeError.
    for name in ["name", "message", "code"] {
      let _ = prototype.set_accessor(scope, key(scope, name).into(), brand_get);
    }
    define_legacy_codes(scope, &prototype);
  }
  install_quota_exceeded(scope);
}

pub struct QuotaExceededError {
  message: String,
}

unsafe impl v8::cppgc::GarbageCollected for QuotaExceededError {
  fn trace(&self, _visitor: &mut v8::cppgc::Visitor) {}

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"QuotaExceededError"
  }
}

#[op(name = "QuotaExceededError")]
impl QuotaExceededError {
  #[constructor]
  fn new(message: Option<String>) -> Self {
    Self {
      message: message.unwrap_or_default(),
    }
  }

  #[construct_hook]
  fn finish(&self, scope: &mut v8::PinScope, object: v8::Local<v8::Object>) {
    define(
      scope,
      object,
      "message",
      &self.message,
      v8::PropertyAttribute::NONE,
    );
    define(
      scope,
      object,
      "name",
      "QuotaExceededError",
      v8::PropertyAttribute::NONE,
    );
    let code = v8::Integer::new(scope, 22);
    let _ = object.define_own_property(
      scope,
      key(scope, "code").into(),
      code.into(),
      v8::PropertyAttribute::READ_ONLY,
    );
    let null = v8::null(scope);
    for name in ["quota", "requested"] {
      let _ = object.define_own_property(
        scope,
        key(scope, name).into(),
        null.into(),
        v8::PropertyAttribute::READ_ONLY,
      );
    }
    capture_stack(scope, object);
  }
}

fn install_quota_exceeded(scope: &mut v8::PinScope) {
  QuotaExceededError::install(scope);
  let Some(constructor) =
    QuotaExceededError::template(scope).get_function(scope)
  else {
    return;
  };
  let Some(prototype) = constructor
    .get(scope, key(scope, "prototype").into())
    .and_then(|value| v8::Local::<v8::Object>::try_from(value).ok())
  else {
    return;
  };
  let Some(parent) = DOMException::template(scope).get_function(scope) else {
    return;
  };
  let Some(parent_prototype) = parent
    .get(scope, key(scope, "prototype").into())
    .and_then(|value| v8::Local::<v8::Object>::try_from(value).ok())
  else {
    return;
  };
  let _ = prototype.set_prototype(scope, parent_prototype.into());
  let constructor_object =
    v8::Local::<v8::Object>::try_from(v8::Local::<v8::Value>::from(
      constructor,
    ))
    .expect("QuotaExceededError");
  define_legacy_codes(scope, &constructor_object);
  define_legacy_codes(scope, &prototype);
}

fn brand_get(
  scope: &mut v8::PinScope,
  _name: v8::Local<v8::Name>,
  _args: v8::PropertyCallbackArguments,
  _ret: v8::ReturnValue<v8::Value>,
) {
  OpException::throw(OpError::type_("Illegal invocation"), scope);
}

const LEGACY_CODES: &[(&str, i32)] = &[
  ("INDEX_SIZE_ERR", 1),
  ("DOMSTRING_SIZE_ERR", 2),
  ("HIERARCHY_REQUEST_ERR", 3),
  ("WRONG_DOCUMENT_ERR", 4),
  ("INVALID_CHARACTER_ERR", 5),
  ("NO_DATA_ALLOWED_ERR", 6),
  ("NO_MODIFICATION_ALLOWED_ERR", 7),
  ("NOT_FOUND_ERR", 8),
  ("NOT_SUPPORTED_ERR", 9),
  ("INUSE_ATTRIBUTE_ERR", 10),
  ("INVALID_STATE_ERR", 11),
  ("SYNTAX_ERR", 12),
  ("INVALID_MODIFICATION_ERR", 13),
  ("NAMESPACE_ERR", 14),
  ("INVALID_ACCESS_ERR", 15),
  ("VALIDATION_ERR", 16),
  ("TYPE_MISMATCH_ERR", 17),
  ("SECURITY_ERR", 18),
  ("NETWORK_ERR", 19),
  ("ABORT_ERR", 20),
  ("URL_MISMATCH_ERR", 21),
  ("QUOTA_EXCEEDED_ERR", 22),
  ("TIMEOUT_ERR", 23),
  ("INVALID_NODE_TYPE_ERR", 24),
  ("DATA_CLONE_ERR", 25),
];

fn define_legacy_codes(
  scope: &mut v8::PinScope,
  object: &v8::Local<v8::Object>,
) {
  for (name, code) in LEGACY_CODES {
    let number = v8::Integer::new(scope, *code);
    let _ = object.define_own_property(
      scope,
      key(scope, name).into(),
      number.into(),
      v8::PropertyAttribute::READ_ONLY,
    );
  }
}

fn define(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
  value: &str,
  attribute: v8::PropertyAttribute,
) {
  let Some(text) = v8::String::new(scope, value) else {
    return;
  };
  let _ = object.define_own_property(
    scope,
    key(scope, name).into(),
    text.into(),
    attribute,
  );
}

fn capture_stack(scope: &mut v8::PinScope, object: v8::Local<v8::Object>) {
  let global = scope.get_current_context().global(scope);
  let Some(error) = global
    .get(scope, key(scope, "Error").into())
    .and_then(|value| v8::Local::<v8::Object>::try_from(value).ok())
  else {
    return;
  };
  let Some(capture) = error
    .get(scope, key(scope, "captureStackTrace").into())
    .and_then(|value| v8::Local::<v8::Function>::try_from(value).ok())
  else {
    return;
  };
  let _ = capture.call(scope, error.into(), &[object.into()]);
}

/// Host clone tag. `1` and `2` are `Blob` and `File`.
const TAG_DOM_EXCEPTION: u32 = 3;

pub(crate) fn register_clone() {
  yuzuko_core::add_host_clone(write_exception_clone, read_exception_clone);
}

fn write_exception_clone(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  helper: &dyn v8::ValueSerializerHelper,
) -> yuzuko_core::WriteResult {
  // `object` is the live host value V8 asked us to serialize.
  let fields =
    unsafe { host_ref::<DOMException>(scope, object) }.map(|exception| {
      (
        exception.get().name.clone(),
        exception.get().message.clone(),
      )
    });
  let Some((name, message)) = fields else {
    return yuzuko_core::WriteResult::Skip;
  };
  let stack = match stack_text(scope, object) {
    Ok(stack) => stack,
    Err(()) => return yuzuko_core::WriteResult::Failed,
  };
  helper.write_uint32(TAG_DOM_EXCEPTION);
  if !yuzuko_core::write_text(helper, &name)
    || !yuzuko_core::write_text(helper, &message)
    || !yuzuko_core::write_text(helper, &stack)
  {
    yuzuko_core::reject_clone(scope);
    return yuzuko_core::WriteResult::Failed;
  }
  yuzuko_core::WriteResult::Wrote
}

fn read_exception_clone(
  scope: &mut v8::PinScope,
  tag: u32,
  helper: &dyn v8::ValueDeserializerHelper,
) -> yuzuko_core::ReadResult {
  if tag != TAG_DOM_EXCEPTION {
    return yuzuko_core::ReadResult::Skip;
  }
  let Some(name) = yuzuko_core::read_text(helper) else {
    yuzuko_core::reject_clone(scope);
    return yuzuko_core::ReadResult::Failed;
  };
  let Some(message) = yuzuko_core::read_text(helper) else {
    yuzuko_core::reject_clone(scope);
    return yuzuko_core::ReadResult::Failed;
  };
  let Some(stack) = yuzuko_core::read_text(helper) else {
    yuzuko_core::reject_clone(scope);
    return yuzuko_core::ReadResult::Failed;
  };
  let Some(object) = revive(scope, &message, &name, &stack) else {
    return yuzuko_core::ReadResult::Failed;
  };
  yuzuko_core::ReadResult::Done(v8::Global::new(scope, object))
}

fn stack_text(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
) -> Result<String, ()> {
  let Some(key) = v8::String::new(scope, "stack") else {
    return Err(());
  };
  let Some(value) = object.get(scope, key.into()) else {
    return Err(());
  };
  if value.is_null_or_undefined() {
    return Ok(String::new());
  }
  let Some(text) = value.to_string(scope) else {
    return Err(());
  };
  Ok(text.to_rust_string_lossy(scope))
}

fn revive<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  message: &str,
  name: &str,
  stack: &str,
) -> Option<v8::Local<'s, v8::Object>> {
  // The deserializer forbids JavaScript execution, so this cannot call the
  // constructor. Own data properties match what `finish` installs.
  let object = unsafe {
    yuzuko_core::op::wrap_instance(
      scope,
      DOMException {
        message: message.to_string(),
        name: name.to_string(),
        code: dom_code(name),
      },
    )
  };
  define(
    scope,
    object,
    "message",
    message,
    v8::PropertyAttribute::NONE,
  );
  define(scope, object, "name", name, v8::PropertyAttribute::NONE);
  let code = v8::Integer::new(scope, i32::from(dom_code(name)));
  let _ = object.define_own_property(
    scope,
    key(scope, "code").into(),
    code.into(),
    v8::PropertyAttribute::READ_ONLY,
  );
  if !stack.is_empty() {
    define(scope, object, "stack", stack, v8::PropertyAttribute::NONE);
  }
  Some(object)
}

fn dom_code(name: &str) -> u16 {
  match name {
    "IndexSizeError" => 1,
    "HierarchyRequestError" => 3,
    "WrongDocumentError" => 4,
    "InvalidCharacterError" => 5,
    "NoModificationAllowedError" => 7,
    "NotFoundError" => 8,
    "NotSupportedError" => 9,
    "InUseAttributeError" => 10,
    "InvalidStateError" => 11,
    "SyntaxError" => 12,
    "InvalidModificationError" => 13,
    "NamespaceError" => 14,
    "InvalidAccessError" => 15,
    "TypeMismatchError" => 17,
    "SecurityError" => 18,
    "NetworkError" => 19,
    "AbortError" => 20,
    "URLMismatchError" => 21,
    "QuotaExceededError" => 22,
    "TimeoutError" => 23,
    "InvalidNodeTypeError" => 24,
    "DataCloneError" => 25,
    _ => 0,
  }
}
