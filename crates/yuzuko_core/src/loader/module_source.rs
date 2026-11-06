//! JavaScript `ModuleSource` for `import source`.
//!
//! The constructor is not a global. Instances inherit
//! `%AbstractModuleSource%.prototype`.
//!
//! Host stand-in until V8 ships `ModuleSource` through rusty_v8. Remove this
//! class when that lands.

use std::sync::Once;

use yuzuko_macros::op;

use crate::RuntimeError;
use crate::clone::{ReadResult, WriteResult};
use crate::host;
use crate::op::{OpError, host_ref, wrap_instance};

const TAG: u32 = 23;

pub struct ModuleSource {
  url: String,
}

unsafe impl v8::cppgc::GarbageCollected for ModuleSource {
  fn trace(&self, _visitor: &mut v8::cppgc::Visitor) {}

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"ModuleSource"
  }
}

#[op]
impl ModuleSource {
  #[constructor]
  fn new() -> Result<ModuleSource, OpError> {
    Err(OpError::type_("Illegal constructor"))
  }
}

pub fn install(scope: &mut v8::PinScope) {
  static CLONE: Once = Once::new();
  CLONE.call_once(|| {
    crate::add_host_clone(write_clone, read_clone);
  });
  ModuleSource::install(scope);
  splice_abstract_prototype(scope);
  hide_constructor(scope);
}

pub fn wrap<'a>(
  scope: &mut v8::PinScope<'a, '_>,
  url: &str,
) -> Result<v8::Local<'a, v8::Object>, RuntimeError> {
  if let Some(existing) = cached(scope, url) {
    return Ok(existing);
  }
  let object = unsafe {
    wrap_instance(
      scope,
      ModuleSource {
        url: url.to_string(),
      },
    )
  };
  host::with_mut(|state| {
    state
      .js_sources
      .insert(url.to_string(), v8::Global::new(scope, object));
  });
  Ok(object)
}

pub fn url_of(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Option<String> {
  let object = v8::Local::<v8::Object>::try_from(value).ok()?;
  let source = unsafe { host_ref::<ModuleSource>(scope, object) }?;
  Some(source.get().url.clone())
}

fn cached<'a>(
  scope: &mut v8::PinScope<'a, '_>,
  url: &str,
) -> Option<v8::Local<'a, v8::Object>> {
  let stored = host::with(|state| state.js_sources.get(url).cloned())?;
  Some(v8::Local::new(scope, stored))
}

fn splice_abstract_prototype(scope: &mut v8::PinScope) {
  let Some(abstract_ctor) = abstract_module_source(scope) else {
    return;
  };
  let ctor = ModuleSource::template(scope)
    .get_function(scope)
    .expect("ModuleSource");
  let _ = ctor.set_prototype(scope, abstract_ctor.into());
  let proto_key = v8::String::new(scope, "prototype").unwrap();
  let Some(proto) = ctor.get(scope, proto_key.into()) else {
    return;
  };
  let Ok(proto) = v8::Local::<v8::Object>::try_from(proto) else {
    return;
  };
  let Some(abstract_proto) = abstract_ctor.get(scope, proto_key.into()) else {
    return;
  };
  let _ = proto.set_prototype(scope, abstract_proto);
}

fn hide_constructor(scope: &mut v8::PinScope) {
  let global = scope.get_current_context().global(scope);
  let key = v8::String::new(scope, "ModuleSource").unwrap();
  let _ = global.delete(scope, key.into());
}

fn abstract_module_source<'s>(
  scope: &mut v8::PinScope<'s, '_>,
) -> Option<v8::Local<'s, v8::Object>> {
  let global = scope.get_current_context().global(scope);
  let wasm_key = v8::String::new(scope, "WebAssembly")?;
  let wasm = global.get(scope, wasm_key.into())?;
  let wasm = v8::Local::<v8::Object>::try_from(wasm).ok()?;
  let module_key = v8::String::new(scope, "Module")?;
  let wasm_module = wasm.get(scope, module_key.into())?;
  let wasm_module = v8::Local::<v8::Object>::try_from(wasm_module).ok()?;
  let proto = wasm_module.get_prototype(scope)?;
  v8::Local::<v8::Object>::try_from(proto).ok()
}

fn write_clone(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  helper: &dyn v8::ValueSerializerHelper,
) -> WriteResult {
  let Some(source) = (unsafe { host_ref::<ModuleSource>(scope, object) })
  else {
    return WriteResult::Skip;
  };
  helper.write_uint32(TAG);
  if !crate::write_text(helper, &source.get().url) {
    crate::reject_clone(scope);
    return WriteResult::Failed;
  }
  WriteResult::Wrote
}

fn read_clone(
  scope: &mut v8::PinScope,
  tag: u32,
  helper: &dyn v8::ValueDeserializerHelper,
) -> ReadResult {
  if tag != TAG {
    return ReadResult::Skip;
  }
  let Some(url) = crate::read_text(helper) else {
    crate::reject_clone(scope);
    return ReadResult::Failed;
  };
  match wrap(scope, &url) {
    Ok(object) => ReadResult::Done(v8::Global::new(scope, object)),
    Err(_) => {
      crate::reject_clone(scope);
      ReadResult::Failed
    }
  }
}
