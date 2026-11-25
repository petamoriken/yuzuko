//! Value, promise, and attribute helpers shared by the WinterTC modules.

use yuzuko_core::op::Js;
use yuzuko_core::op::OpError;

/// `ToString(value)`. A thrown conversion becomes a `TypeError`.
pub(crate) fn string_value(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<String, OpError> {
  value
    .to_string(scope)
    .map(|text| text.to_rust_string_lossy(scope))
    .ok_or_else(|| OpError::type_("The value is not a string"))
}

/// A promise already resolved with `value`.
pub(crate) fn resolve(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Js {
  let resolver = v8::PromiseResolver::new(scope).unwrap();
  resolver.resolve(scope, value);
  let promise = resolver.get_promise(scope);
  let value: v8::Local<v8::Value> = promise.into();
  Js(v8::Global::new(scope, value))
}

/// A promise already rejected with `error`. A thrown value becomes a
/// `TypeError`.
pub(crate) fn reject(scope: &mut v8::PinScope, error: OpError) -> Js {
  let resolver = v8::PromiseResolver::new(scope).unwrap();
  let exception = match error {
    OpError::Thrown(_) => {
      let text = v8::String::new(scope, "The operation failed").unwrap();
      v8::Exception::type_error(scope, text)
    }
    other => yuzuko_core::exception_value(
      scope,
      other,
      yuzuko_core::DomFallback::Error,
      yuzuko_core::StringFailure::Unwrap,
    ),
  };
  resolver.reject(scope, exception);
  let promise = resolver.get_promise(scope);
  let value: v8::Local<v8::Value> = promise.into();
  Js(v8::Global::new(scope, value))
}

/// An iterator result object `{ done, value }`.
pub(crate) fn iterator_result(
  scope: &mut v8::PinScope,
  done: bool,
  value: v8::Local<v8::Value>,
) -> Js {
  let object = v8::Object::new(scope);
  let done = v8::Boolean::new(scope, done);
  let _ = object.create_data_property(
    scope,
    v8::String::new(scope, "done").unwrap().into(),
    done.into(),
  );
  let _ = object.create_data_property(
    scope,
    v8::String::new(scope, "value").unwrap().into(),
    value,
  );
  let value: v8::Local<v8::Value> = object.into();
  Js(v8::Global::new(scope, value))
}

/// Getter for a global attribute whose value is the function's data.
///
/// The receiver is `undefined`, `null`, or the global object. Any other
/// receiver throws a `TypeError`.
pub(crate) fn global_attribute(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut ret: v8::ReturnValue,
) {
  let global = scope.get_current_context().global(scope);
  let this = args.this();
  if this.is_null_or_undefined() || this.strict_equals(global.into()) {
    ret.set(args.data());
    return;
  }
  yuzuko_core::op::OpException::throw(
    OpError::type_("illegal invocation"),
    scope,
  );
}
