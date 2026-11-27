//! Built-ins that host code calls for a web API.
//!
//! Script can replace or delete `globalThis.AbortSignal`, `fetch`, and the
//! rest, and the host must not call the replacement. Each name is read once,
//! after every extension has installed and before any user code runs, and
//! later calls use that captured value.

/// The names to capture. A dotted name is a property of the object before
/// the dot.
const NAMES: &[&str] = &[
  "AbortController",
  "AbortSignal",
  "AbortSignal.any",
  "JSON.stringify",
  "QuotaExceededError",
  "ReadableStream",
  "Request",
  "WritableStream",
  "fetch",
];

/// Captures every name in [`NAMES`] that this realm defines.
pub(crate) fn capture(scope: &mut v8::PinScope) {
  let global: v8::Local<v8::Value> =
    scope.get_current_context().global(scope).into();
  for name in NAMES {
    let mut value = global;
    for part in name.split('.') {
      let Ok(object) = v8::Local::<v8::Object>::try_from(value) else {
        value = v8::undefined(scope).into();
        break;
      };
      let Some(key) = v8::String::new(scope, part) else {
        value = v8::undefined(scope).into();
        break;
      };
      value = object
        .get(scope, key.into())
        .unwrap_or_else(|| v8::undefined(scope).into());
    }
    if value.is_undefined() {
      continue;
    }
    let value = v8::Global::new(scope, value);
    crate::host::with_mut(|state| {
      state.intrinsics.insert(name, value);
    });
  }
}

/// The captured value of `name`, or `None` when this realm has none.
pub fn intrinsic<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  name: &str,
) -> Option<v8::Local<'s, v8::Value>> {
  let value = crate::host::with(|state| state.intrinsics.get(name).cloned())?;
  Some(v8::Local::new(scope, value))
}

/// The captured function `name`.
pub fn intrinsic_function<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  name: &str,
) -> Option<v8::Local<'s, v8::Function>> {
  v8::Local::<v8::Function>::try_from(intrinsic(scope, name)?).ok()
}
