//! Value, promise, and path helpers shared by the window modules.

use yuzuko_core::op::{Js, OpError};

/// Holds `object` as the return value of an `#[op]`.
pub(crate) fn js_object(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
) -> Js {
  let value: v8::Local<v8::Value> = object.into();
  Js(v8::Global::new(scope, value))
}

/// A promise already resolved with `value`.
pub(crate) fn resolve(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Js {
  let resolver = v8::PromiseResolver::new(scope).unwrap();
  let promise = resolver.get_promise(scope);
  resolver.resolve(scope, value);
  let promise: v8::Local<v8::Value> = promise.into();
  Js(v8::Global::new(scope, promise))
}

/// A promise already rejected with a `TypeError` when `name` is
/// `"TypeError"`, and with a `DOMException` named `name` otherwise.
pub(crate) fn reject(
  scope: &mut v8::PinScope,
  name: &str,
  message: &str,
) -> Js {
  let resolver = v8::PromiseResolver::new(scope).unwrap();
  let promise = resolver.get_promise(scope);
  let error = if name == "TypeError" {
    let message = v8::String::new(scope, message).unwrap();
    v8::Exception::type_error(scope, message)
  } else {
    yuzuko_core::op::new_dom_exception(scope, name, message)
      .map(v8::Local::<v8::Value>::from)
      .unwrap_or_else(|| {
        let message = v8::String::new(scope, message).unwrap();
        v8::Exception::type_error(scope, message)
      })
  };
  resolver.reject(scope, error);
  let promise: v8::Local<v8::Value> = promise.into();
  Js(v8::Global::new(scope, promise))
}

/// `ToString(value)`. A thrown exception is rethrown.
pub(crate) fn js_string(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<String, OpError> {
  let captured = {
    v8::tc_scope!(let tc, scope);
    match value.to_string(tc) {
      Some(text) => Ok(text.to_rust_string_lossy(tc)),
      None => {
        let exception =
          tc.exception().unwrap_or_else(|| v8::undefined(tc).into());
        Err(v8::Global::new(tc, exception))
      }
    }
  };
  match captured {
    Ok(text) => Ok(text),
    Err(exception) => {
      let exception = v8::Local::new(scope, exception);
      Err(OpError::thrown(scope, exception))
    }
  }
}

/// `list[index]`. A thrown exception is rethrown.
pub(crate) fn array_item<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  list: v8::Local<'_, v8::Array>,
  index: u32,
) -> Result<v8::Local<'s, v8::Value>, OpError> {
  let captured = {
    v8::tc_scope!(let tc, scope);
    let list = yuzuko_core::op::root(tc, list);
    match list.get_index(tc, index) {
      Some(value) => Ok(v8::Global::new(tc, value)),
      None => {
        if let Some(exception) = tc.exception() {
          Err(v8::Global::new(tc, exception))
        } else {
          let missing: v8::Local<v8::Value> = v8::undefined(tc).into();
          Ok(v8::Global::new(tc, missing))
        }
      }
    }
  };
  match captured {
    Ok(global) => Ok(v8::Local::new(scope, global)),
    Err(exception) => {
      let exception = v8::Local::new(scope, exception);
      Err(OpError::thrown(scope, exception))
    }
  }
}

/// An absolute path for a dropped, hovered, or picked file.
///
/// A relative path is joined to the process current directory. The result is
/// not canonicalized. Non-UTF-8 bytes become U+FFFD.
pub(crate) fn absolute_path(path: std::path::PathBuf) -> String {
  let path = if path.is_absolute() {
    path
  } else {
    match std::env::current_dir() {
      Ok(dir) => dir.join(path),
      Err(_) => path,
    }
  };
  path.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
  use super::absolute_path;

  #[test]
  fn dropped_path_is_absolute() {
    let relative = absolute_path("yuzuko-rel.txt".into());
    let relative = std::path::PathBuf::from(relative);
    assert!(relative.is_absolute());
    assert!(relative.ends_with("yuzuko-rel.txt"));
    let kept = std::env::current_dir().unwrap().join("kept.txt");
    let absolute = absolute_path(kept.clone());
    assert_eq!(std::path::PathBuf::from(absolute), kept);
  }
}
