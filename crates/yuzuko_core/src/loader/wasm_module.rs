//! Wasm ES module integration.
//!
//! An evaluation import compiles the bytes, then evaluates a JavaScript
//! wrapper that instantiates them. A source-phase import returns the
//! `WebAssembly.Module` and does not resolve the Wasm module's imports.
//! The wrapper records the instance through [`BRIDGE_SPECIFIER`], which
//! user modules cannot import.
//!
//! `wasm:js-string` and `wasm:js/string-constants` are compile-time imports.
//! `WasmModuleObject::compile` has no options argument, so those bytes go
//! through the `WebAssembly.Module` constructor instead.

use crate::RuntimeError;
use crate::host;
use crate::loader::modules::exception_text;

struct Import {
  module: String,
  name: String,
  kind: String,
}

struct Export {
  name: String,
  kind: String,
}

/// Module specifier of the instance table. A NUL keeps it out of ordinary
/// source. Only a `.wasm` evaluation wrapper may import it.
pub(crate) const BRIDGE_SPECIFIER: &str = "\0yuzuko-wasm-instance";

pub(crate) fn bridge_module<'a>(
  scope: &mut v8::PinScope<'a, '_>,
) -> Result<v8::Local<'a, v8::Module>, RuntimeError> {
  if let Some(existing) = host::with(|state| state.wasm_bridge.clone()) {
    return Ok(v8::Local::new(scope, existing));
  }
  let name = v8::String::new(scope, BRIDGE_SPECIFIER).expect("bridge name");
  let note = v8::String::new(scope, "noteInstance").expect("noteInstance");
  let exports =
    v8::String::new(scope, "instanceExports").expect("instanceExports");
  let module = v8::Module::create_synthetic_module(
    scope,
    name,
    &[note, exports],
    evaluate_bridge,
  );
  let stored = v8::Global::new(scope, module);
  host::with_mut(|state| state.wasm_bridge = Some(stored));
  Ok(module)
}

fn evaluate_bridge<'s>(
  context: v8::Local<'s, v8::Context>,
  module: v8::Local<'s, v8::Module>,
) -> Option<v8::Local<'s, v8::Value>> {
  v8::callback_scope!(unsafe outer, context);
  {
    let scope = &mut v8::ContextScope::new(outer, context);
    let note = v8::Function::new(scope, note_instance)?;
    let note_name = v8::String::new(scope, "noteInstance")?;
    note.set_name(note_name);
    module.set_synthetic_module_export(scope, note_name, note.into())?;
    let exports = v8::Function::new(scope, instance_exports)?;
    let exports_name = v8::String::new(scope, "instanceExports")?;
    exports.set_name(exports_name);
    module.set_synthetic_module_export(scope, exports_name, exports.into())?;
  }
  crate::loader::modules::evaluated(outer)
}

fn note_instance(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  _ret: v8::ReturnValue,
) {
  let map = host::wasm_instances(scope);
  let _ = map.set(scope, args.get(0), args.get(1));
}

fn instance_exports(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut ret: v8::ReturnValue,
) {
  let map = host::wasm_instances(scope);
  let Some(found) = map.get(scope, args.get(0)) else {
    return;
  };
  if found.is_null_or_undefined() {
    return;
  }
  let Ok(object) = v8::Local::<v8::Object>::try_from(found) else {
    return;
  };
  let Some(key) = v8::String::new(scope, "exports") else {
    return;
  };
  let Some(exports) = object.get(scope, key.into()) else {
    return;
  };
  ret.set(exports);
}

pub(crate) fn wrapper_source(
  scope: &mut v8::PinScope,
  url: &str,
  bytes: &[u8],
) -> Result<String, RuntimeError> {
  let module = compile_cached(scope, url, bytes)?;
  let (imports, exports) = descriptors(scope, module)?;
  Ok(render(url, &imports, &exports))
}

pub(crate) fn source_object<'a>(
  scope: &mut v8::PinScope<'a, '_>,
  url: &str,
  bytes: &[u8],
) -> Result<v8::Local<'a, v8::Object>, RuntimeError> {
  compile_cached(scope, url, bytes)
}

fn compile_cached<'a>(
  scope: &mut v8::PinScope<'a, '_>,
  url: &str,
  bytes: &[u8],
) -> Result<v8::Local<'a, v8::Object>, RuntimeError> {
  let key = host::source_cache_key(url);
  if let Some(module) = cached(scope, &key) {
    return Ok(module);
  }
  let module = {
    v8::tc_scope!(let tc, scope);
    let compiled = if needs_compile_time_imports(bytes) {
      compile_with_options(tc, bytes)
    } else {
      v8::WasmModuleObject::compile(tc, bytes).map(|module| {
        let module: v8::Local<v8::Object> = module.into();
        module
      })
    };
    let Some(module) = compiled else {
      let text = exception_text!(tc, "invalid WebAssembly module");
      return Err(RuntimeError::message(text));
    };
    v8::Global::new(tc, module)
  };
  host::with_mut(|state| {
    state.wasm_modules.insert(key.clone(), module.clone());
  });
  cached(scope, &key).ok_or_else(|| {
    RuntimeError::message(format!("failed to compile WebAssembly module {url}"))
  })
}

fn needs_compile_time_imports(bytes: &[u8]) -> bool {
  contains_bytes(bytes, b"wasm:js-string")
    || contains_bytes(bytes, b"wasm:js/string-constants")
}

fn contains_bytes(haystack: &[u8], needle: &[u8]) -> bool {
  haystack
    .windows(needle.len())
    .any(|window| window == needle)
}

fn compile_with_options<'a>(
  scope: &mut v8::PinScope<'a, '_>,
  bytes: &[u8],
) -> Option<v8::Local<'a, v8::Object>> {
  let constructor = module_constructor(scope)?;
  let options = compile_options(scope, bytes)?;
  let bytes = uint8_array(scope, bytes)?;
  constructor.new_instance(scope, &[bytes.into(), options.into()])
}

fn module_constructor<'a>(
  scope: &mut v8::PinScope<'a, '_>,
) -> Option<v8::Local<'a, v8::Function>> {
  let global = scope.get_current_context().global(scope);
  let key = v8::String::new(scope, "WebAssembly")?;
  let webassembly = global.get(scope, key.into())?;
  let webassembly = v8::Local::<v8::Object>::try_from(webassembly).ok()?;
  let key = v8::String::new(scope, "Module")?;
  let constructor = webassembly.get(scope, key.into())?;
  v8::Local::<v8::Function>::try_from(constructor).ok()
}

fn compile_options<'a>(
  scope: &mut v8::PinScope<'a, '_>,
  bytes: &[u8],
) -> Option<v8::Local<'a, v8::Object>> {
  let options = v8::Object::new(scope);
  if contains_bytes(bytes, b"wasm:js-string") {
    let name = v8::String::new(scope, "js-string")?;
    let builtins = v8::Array::new_with_elements(scope, &[name.into()]);
    let key = v8::String::new(scope, "builtins")?;
    options.create_data_property(scope, key.into(), builtins.into())?;
  }
  if contains_bytes(bytes, b"wasm:js/string-constants") {
    let key = v8::String::new(scope, "importedStringConstants")?;
    let value = v8::String::new(scope, "wasm:js/string-constants")?;
    options.create_data_property(scope, key.into(), value.into())?;
  }
  Some(options)
}

fn uint8_array<'a>(
  scope: &mut v8::PinScope<'a, '_>,
  bytes: &[u8],
) -> Option<v8::Local<'a, v8::Uint8Array>> {
  crate::js::uint8_array(scope, bytes.to_vec())
}

fn cached<'a>(
  scope: &mut v8::PinScope<'a, '_>,
  key: &str,
) -> Option<v8::Local<'a, v8::Object>> {
  let module = host::with(|state| state.wasm_modules.get(key).cloned())?;
  Some(v8::Local::new(scope, module))
}

fn descriptors(
  scope: &mut v8::PinScope,
  module: v8::Local<v8::Object>,
) -> Result<(Vec<Import>, Vec<Export>), RuntimeError> {
  let imports = call_module_static(scope, module, "imports")?;
  let exports = call_module_static(scope, module, "exports")?;
  Ok((read_imports(scope, imports)?, read_exports(scope, exports)?))
}

fn call_module_static<'a>(
  scope: &mut v8::PinScope<'a, '_>,
  module: v8::Local<v8::Object>,
  name: &str,
) -> Result<v8::Local<'a, v8::Array>, RuntimeError> {
  let value = {
    v8::tc_scope!(let tc, scope);
    let global = tc.get_current_context().global(tc);
    let webassembly_key = v8::String::new(tc, "WebAssembly").unwrap();
    let Some(webassembly) = global.get(tc, webassembly_key.into()) else {
      return Err(RuntimeError::message(
        "WebAssembly is not available".to_string(),
      ));
    };
    let Ok(webassembly) = v8::Local::<v8::Object>::try_from(webassembly) else {
      return Err(RuntimeError::message(
        "WebAssembly is not available".to_string(),
      ));
    };
    let module_key = v8::String::new(tc, "Module").unwrap();
    let Some(constructor) = webassembly.get(tc, module_key.into()) else {
      return Err(RuntimeError::message(
        "WebAssembly.Module is not available".to_string(),
      ));
    };
    let Ok(constructor) = v8::Local::<v8::Object>::try_from(constructor) else {
      return Err(RuntimeError::message(
        "WebAssembly.Module is not available".to_string(),
      ));
    };
    let method_key = v8::String::new(tc, name).unwrap();
    let Some(method) = constructor.get(tc, method_key.into()) else {
      return Err(RuntimeError::message(format!(
        "WebAssembly.Module.{name} is not available"
      )));
    };
    let Ok(method) = v8::Local::<v8::Function>::try_from(method) else {
      return Err(RuntimeError::message(format!(
        "WebAssembly.Module.{name} is not available"
      )));
    };
    let Some(value) = method.call(tc, constructor.into(), &[module.into()])
    else {
      let text = exception_text!(tc, "failed to read WebAssembly module");
      return Err(RuntimeError::message(text));
    };
    let Ok(array) = v8::Local::<v8::Array>::try_from(value) else {
      return Err(RuntimeError::message(
        "WebAssembly module descriptor is not an array".to_string(),
      ));
    };
    v8::Global::new(tc, array)
  };
  Ok(v8::Local::new(scope, value))
}

fn read_imports(
  scope: &mut v8::PinScope,
  array: v8::Local<v8::Array>,
) -> Result<Vec<Import>, RuntimeError> {
  let mut imports = Vec::new();
  for index in 0..array.length() {
    let Some(item) = array.get_index(scope, index) else {
      continue;
    };
    let Ok(item) = v8::Local::<v8::Object>::try_from(item) else {
      continue;
    };
    imports.push(Import {
      module: property(scope, item, "module"),
      name: property(scope, item, "name"),
      kind: property(scope, item, "kind"),
    });
  }
  Ok(imports)
}

fn read_exports(
  scope: &mut v8::PinScope,
  array: v8::Local<v8::Array>,
) -> Result<Vec<Export>, RuntimeError> {
  let mut exports = Vec::new();
  for index in 0..array.length() {
    let Some(item) = array.get_index(scope, index) else {
      continue;
    };
    let Ok(item) = v8::Local::<v8::Object>::try_from(item) else {
      continue;
    };
    exports.push(Export {
      name: property(scope, item, "name"),
      kind: property(scope, item, "kind"),
    });
  }
  Ok(exports)
}

fn property(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
) -> String {
  let key = v8::String::new(scope, name).unwrap();
  object
    .get(scope, key.into())
    .and_then(|value| value.to_string(scope))
    .map(|value| value.to_rust_string_lossy(scope))
    .unwrap_or_default()
}

fn render(url: &str, imports: &[Import], exports: &[Export]) -> String {
  if reserved_name(imports, exports) {
    return "throw new WebAssembly.LinkError(\"reserved WebAssembly name\");\n"
      .to_string();
  }

  let mut source = String::new();
  source.push_str("import { noteInstance, instanceExports } from ");
  source.push_str(&js_string(BRIDGE_SPECIFIER));
  source.push_str(";\n");
  source.push_str("import source wasmMod from ");
  source.push_str(&js_string(url));
  source.push_str(";\n");

  let mut groups: Vec<(&str, Vec<&Import>)> = Vec::new();
  for import in imports {
    if is_compile_time_module(&import.module) {
      continue;
    }
    if let Some(group) = groups
      .iter_mut()
      .find(|(module, _)| *module == import.module)
    {
      group.1.push(import);
    } else {
      groups.push((&import.module, vec![import]));
    }
  }

  let global_import = imports.iter().any(|import| {
    import.kind == "global" && !is_compile_time_module(&import.module)
  });
  if global_import {
    source.push_str(
      "const wasmGlobalExports = (specifier) => {\n  \
       let href;\n  try {\n    \
       href = new URL(specifier, import.meta.url).href;\n  \
       } catch {\n    return undefined;\n  }\n  \
       return instanceExports(href);\n};\n",
    );
  }

  for (group_index, (_, group)) in groups.iter().enumerate() {
    source.push_str("import { ");
    for (name_index, import) in group.iter().enumerate() {
      if name_index > 0 {
        source.push_str(", ");
      }
      source.push_str(&js_string(&import.name));
      source.push_str(" as import_");
      source.push_str(&group_index.to_string());
      source.push('_');
      source.push_str(&name_index.to_string());
    }
    source.push_str(" } from ");
    source.push_str(&js_string(&group[0].module));
    source.push_str(";\n");
  }

  if groups.is_empty() {
    source.push_str("const modInstance = new WebAssembly.Instance(wasmMod);\n");
  } else {
    source.push_str("const importsObject = {\n");
    for (group_index, (module, group)) in groups.iter().enumerate() {
      source.push_str("  ");
      source.push_str(&js_string(module));
      source.push_str(": {\n");
      for (name_index, import) in group.iter().enumerate() {
        source.push_str("    ");
        source.push_str(&js_string(&import.name));
        source.push_str(": ");
        source.push_str(&import_value(group_index, name_index, import));
        source.push_str(",\n");
      }
      source.push_str("  },\n");
    }
    source.push_str("};\n");
    source.push_str(
      "const modInstance = new WebAssembly.Instance(wasmMod, importsObject);\n",
    );
  }

  let global_exports: Vec<usize> = exports
    .iter()
    .enumerate()
    .filter(|(_, export)| export.kind == "global")
    .map(|(index, _)| index)
    .collect();
  source.push_str("noteInstance(import.meta.url, modInstance);\n");
  let refresh = !global_exports.is_empty()
    && exports.iter().any(|export| export.kind == "function");
  for (index, export) in exports.iter().enumerate() {
    write_export(&mut source, index, export, refresh);
  }
  if refresh {
    source.push_str("function refreshWasmGlobals() {\n");
    for index in &global_exports {
      let export = &exports[*index];
      source.push_str("  try { export");
      source.push_str(&index.to_string());
      source.push_str(" = modInstance.exports[");
      source.push_str(&js_string(&export.name));
      source.push_str("].value; } catch {}\n");
    }
    source.push_str("}\n");
  }
  source
}

fn reserved_name(imports: &[Import], exports: &[Export]) -> bool {
  imports.iter().any(|import| {
    import.module == BRIDGE_SPECIFIER
      || is_reserved(&import.name)
      || (is_reserved(&import.module)
        && !is_compile_time_module(&import.module))
  }) || exports.iter().any(|export| is_reserved(&export.name))
}

fn is_compile_time_module(name: &str) -> bool {
  name == "wasm:js-string" || name == "wasm:js/string-constants"
}

fn is_reserved(name: &str) -> bool {
  name.starts_with("wasm:") || name.starts_with("wasm-js:")
}

fn import_value(
  group_index: usize,
  name_index: usize,
  import: &Import,
) -> String {
  let binding = format!("import_{group_index}_{name_index}");
  if import.kind != "global" {
    return binding;
  }
  let specifier = js_string(&import.module);
  let name = js_string(&import.name);
  format!("(wasmGlobalExports({specifier}) ?? {{}})[{name}] ?? {binding}")
}

fn write_export(
  source: &mut String,
  index: usize,
  export: &Export,
  refresh: bool,
) {
  let name = js_string(&export.name);
  if export.kind == "global" {
    source.push_str("let export");
    source.push_str(&index.to_string());
    source.push_str(";\ntry { export");
    source.push_str(&index.to_string());
    source.push_str(" = modInstance.exports[");
    source.push_str(&name);
    source.push_str("].value; } catch {}\n");
  } else if export.kind == "function" && refresh {
    source.push_str("const export");
    source.push_str(&index.to_string());
    source
      .push_str(" = function() {\n  try {\n    return modInstance.exports[");
    source.push_str(&name);
    source.push_str("].apply(this, arguments);\n  } finally {\n    refreshWasmGlobals();\n  }\n};\n");
  } else {
    source.push_str("const export");
    source.push_str(&index.to_string());
    source.push_str(" = modInstance.exports[");
    source.push_str(&name);
    source.push_str("];\n");
  }
  if export.name == "default" {
    source.push_str("export default export");
    source.push_str(&index.to_string());
    source.push_str(";\n");
  } else {
    source.push_str("export { export");
    source.push_str(&index.to_string());
    source.push_str(" as ");
    source.push_str(&name);
    source.push_str(" };\n");
  }
}

fn js_string(value: &str) -> String {
  let mut out = String::from("\"");
  for ch in value.chars() {
    match ch {
      '\\' => out.push_str("\\\\"),
      '"' => out.push_str("\\\""),
      '\n' => out.push_str("\\n"),
      '\r' => out.push_str("\\r"),
      '\u{2028}' => out.push_str("\\u2028"),
      '\u{2029}' => out.push_str("\\u2029"),
      ch if ch.is_control() => {
        out.push_str(&format!("\\u{:04x}", ch as u32));
      }
      ch => out.push(ch),
    }
  }
  out.push('"');
  out
}
