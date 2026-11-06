//! Objects reachable from `globalThis` by own data properties.
//!
//! Hot reload uses this to see which host objects the running graph kept.
//! Getters are not called. A function is not entered, so a value captured by
//! a closure is invisible here.

use std::collections::HashMap;

/// Stop descending once this many objects have been visited.
const OBJECT_CAP: usize = 4096;
/// One object with more own properties than this aborts the walk.
const PROPERTY_CAP: u32 = 4096;

/// Visits objects reachable from `globalThis`.
///
/// Returns `false` when the walk stopped early. Callers that close unreached
/// objects should keep them all in that case.
pub fn walk_objects(
  scope: &mut v8::PinScope,
  mut visit: impl FnMut(&mut v8::PinScope, v8::Local<v8::Object>),
) -> bool {
  let root = crate::js::global(scope);
  let mut stack = vec![v8::Global::new(scope, root)];
  let mut seen: HashMap<i32, Vec<v8::Global<v8::Object>>> = HashMap::new();
  let mut visited = 0usize;
  while let Some(next) = stack.pop() {
    if visited >= OBJECT_CAP {
      return false;
    }
    let object = v8::Local::new(scope, &next);
    if !remember(scope, &mut seen, object) {
      continue;
    }
    visited += 1;
    visit(scope, object);
    if v8::Local::<v8::Value>::from(object).is_function() {
      continue;
    }
    match own_data_objects(scope, object) {
      Some(children) => stack.extend(children),
      None => return false,
    }
  }
  true
}

fn remember(
  scope: &mut v8::PinScope,
  seen: &mut HashMap<i32, Vec<v8::Global<v8::Object>>>,
  object: v8::Local<v8::Object>,
) -> bool {
  let hash = object.get_identity_hash().get();
  let list = seen.entry(hash).or_default();
  let value = v8::Local::<v8::Value>::from(object);
  for previous in list.iter() {
    let previous = v8::Local::new(scope, previous);
    if previous.strict_equals(value) {
      return false;
    }
  }
  list.push(v8::Global::new(scope, object));
  true
}

/// `None` when this object has too many own properties to walk safely.
fn own_data_objects(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
) -> Option<Vec<v8::Global<v8::Object>>> {
  let names = {
    v8::tc_scope!(let tc, scope);
    let object = v8::Local::new(tc, object);
    let mut builder = v8::GetPropertyNamesArgsBuilder::new();
    let args = builder
      .mode(v8::KeyCollectionMode::OwnOnly)
      .property_filter(v8::PropertyFilter::ONLY_ENUMERABLE)
      .index_filter(v8::IndexFilter::IncludeIndices)
      .key_conversion(v8::KeyConversionMode::ConvertToString)
      .build();
    match object.get_own_property_names(tc, args) {
      Some(names) => Some(v8::Global::new(tc, names)),
      None => None,
    }
  };
  let names = v8::Local::new(scope, names?);
  if names.length() > PROPERTY_CAP {
    return None;
  }
  let mut children = Vec::new();
  for index in 0..names.length() {
    let Some(child) = data_object_at(scope, object, names, index) else {
      continue;
    };
    children.push(child);
  }
  Some(children)
}

fn data_object_at(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  names: v8::Local<v8::Array>,
  index: u32,
) -> Option<v8::Global<v8::Object>> {
  let key = {
    v8::tc_scope!(let tc, scope);
    let names = v8::Local::new(tc, names);
    names
      .get_index(tc, index)
      .map(|key| v8::Global::new(tc, key))
  }?;
  let value = data_value(scope, object, v8::Local::new(scope, key))?;
  let object = v8::Local::<v8::Object>::try_from(value).ok()?;
  if v8::Local::<v8::Value>::from(object).is_function() {
    return None;
  }
  Some(v8::Global::new(scope, object))
}

/// The data-property value, or `None` for an accessor or a throwing read.
fn data_value<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  object: v8::Local<'_, v8::Object>,
  key: v8::Local<'_, v8::Value>,
) -> Option<v8::Local<'s, v8::Value>> {
  let descriptor = {
    v8::tc_scope!(let tc, scope);
    let object = v8::Local::new(tc, object);
    let key = v8::Local::new(tc, key);
    let Ok(key) = v8::Local::<v8::Name>::try_from(key) else {
      return None;
    };
    match object.get_own_property_descriptor(tc, key) {
      Some(value) if value.is_object() => Some(v8::Global::new(tc, value)),
      _ => None,
    }
  }?;
  let descriptor = v8::Local::new(scope, descriptor);
  let descriptor = v8::Local::<v8::Object>::try_from(descriptor).ok()?;
  if is_accessor(scope, descriptor) {
    return None;
  }
  let value_key = v8::String::new(scope, "value")?;
  let value = {
    v8::tc_scope!(let tc, scope);
    let descriptor = v8::Local::new(tc, descriptor);
    let key = v8::Local::new(tc, value_key);
    descriptor
      .get(tc, key.into())
      .map(|value| v8::Global::new(tc, value))
  }?;
  Some(v8::Local::new(scope, value))
}

fn is_accessor(
  scope: &mut v8::PinScope,
  descriptor: v8::Local<v8::Object>,
) -> bool {
  for name in ["get", "set"] {
    let Some(key) = v8::String::new(scope, name) else {
      return true;
    };
    v8::tc_scope!(let tc, scope);
    let descriptor = v8::Local::new(tc, descriptor);
    match descriptor.get(tc, key.into()) {
      Some(value) if value.is_undefined() => {}
      _ => return true,
    }
  }
  false
}
