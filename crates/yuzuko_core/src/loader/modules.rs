//! Module graph: oxc resolution, TypeScript erasure, and V8 module records.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use oxc_resolver::{
  EnforceExtension, ModuleType, ResolveOptions, Resolver, TsconfigDiscovery,
};
use url::Url;

use crate::RuntimeError;
use crate::host::{
  self, DeferredImport, ModuleEntry, ModuleRecordKind, SyntheticSource,
};
use crate::loader::typescript::prepare_source;

macro_rules! exception_text {
  ($tc:ident, $fallback:expr) => {{
    let exception = $tc.exception();
    let message = $tc.message();
    let trace = $tc.stack_trace();
    crate::loader::modules::format_exception(
      &mut *$tc, exception, message, trace, $fallback,
    )
  }};
}

pub(crate) use exception_text;

/// `Error.prototype.stack` when the thrown value has one.
pub(crate) fn exception_stack(
  scope: &mut v8::PinScope,
  exception: v8::Local<v8::Value>,
) -> Option<String> {
  let object = v8::Local::<v8::Object>::try_from(exception).ok()?;
  let key = v8::String::new(scope, "stack")?;
  let stack = object.get(scope, key.into())?;
  if !stack.is_string() {
    return None;
  }
  let text = stack.to_string(scope)?.to_rust_string_lossy(scope);
  if text.is_empty() { None } else { Some(text) }
}

pub(crate) fn format_exception(
  scope: &mut v8::PinScope,
  exception: Option<v8::Local<v8::Value>>,
  message: Option<v8::Local<v8::Message>>,
  trace: Option<v8::Local<v8::Value>>,
  fallback: &str,
) -> String {
  if let Some(exception) = exception
    && let Some(stack) = exception_stack(scope, exception)
  {
    return stack;
  }
  if let Some(trace) = trace
    && let Some(text) = trace
      .to_string(scope)
      .map(|value| value.to_rust_string_lossy(scope))
    && !text.is_empty()
  {
    return text;
  }
  let text = exception
    .and_then(|value| value.to_string(scope))
    .map(|value| value.to_rust_string_lossy(scope))
    .unwrap_or_else(|| fallback.to_string());
  let Some(message) = message else {
    return text;
  };
  let resource = message
    .get_script_resource_name(scope)
    .and_then(|value| value.to_string(scope))
    .map(|value| value.to_rust_string_lossy(scope))
    .unwrap_or_else(|| "(unknown)".to_string());
  match message.get_line_number(scope) {
    Some(line) => format!("{resource}:{line}: {text}"),
    None => format!("{resource}: {text}"),
  }
}

pub fn resolver() -> Resolver {
  Resolver::new(ResolveOptions {
    tsconfig: Some(TsconfigDiscovery::Auto),
    condition_names: vec!["import".into(), "default".into()],
    extensions: vec![
      ".ts".into(),
      ".mts".into(),
      ".js".into(),
      ".mjs".into(),
      ".json".into(),
      ".wasm".into(),
    ],
    extension_alias: vec![
      (".js".into(), vec![".ts".into(), ".js".into()]),
      (".mjs".into(), vec![".mts".into(), ".mjs".into()]),
    ],
    enforce_extension: EnforceExtension::Disabled,
    main_fields: vec!["module".into(), "main".into()],
    module_type: true,
    ..ResolveOptions::default()
  })
}

pub fn compile_entry(
  scope: &mut v8::PinScope<'_, '_>,
  path: &Path,
) -> Result<v8::Global<v8::Module>, RuntimeError> {
  let path = absolute(path)?;
  let url = file_url(&path)?;
  let module = compile_path(scope, &path, &url, None)?;
  Ok(v8::Global::new(scope, module))
}

enum ModuleRun {
  Value(v8::Global<v8::Value>),
  Thrown(v8::Global<v8::Value>),
}

pub fn instantiate_and_evaluate<'a>(
  scope: &mut v8::PinScope<'a, '_>,
  module: v8::Local<'a, v8::Module>,
) -> Result<v8::Local<'a, v8::Value>, RuntimeError> {
  match run_module(scope, module)? {
    ModuleRun::Value(value) => Ok(v8::Local::new(scope, value)),
    ModuleRun::Thrown(thrown) => {
      if let Some(code) = crate::process::exit_code() {
        return Err(RuntimeError::exit(code));
      }
      let thrown = v8::Local::new(scope, thrown);
      let text = exception_stack(scope, thrown).unwrap_or_else(|| {
        thrown
          .to_string(scope)
          .map(|text| text.to_rust_string_lossy(scope))
          .unwrap_or_else(|| "failed to evaluate module".to_string())
      });
      Err(RuntimeError::message(text))
    }
  }
}

fn run_module(
  scope: &mut v8::PinScope,
  module: v8::Local<v8::Module>,
) -> Result<ModuleRun, RuntimeError> {
  let outcome = {
    v8::tc_scope!(let tc, scope);
    if module
      .instantiate_module2(tc, resolve_callback, source_callback)
      .is_none()
    {
      if let Some(code) = crate::process::exit_code() {
        return Err(RuntimeError::exit(code));
      }
      if let Some(exception) = tc.exception() {
        return Ok(ModuleRun::Thrown(v8::Global::new(tc, exception)));
      }
      let text = exception_text!(tc, "failed to instantiate module");
      return Err(RuntimeError::message(text));
    }
    match module.evaluate(tc) {
      Some(value) => ModuleRun::Value(v8::Global::new(tc, value)),
      None => {
        if let Some(code) = crate::process::exit_code() {
          return Err(RuntimeError::exit(code));
        }
        if let Some(exception) = tc.exception() {
          ModuleRun::Thrown(v8::Global::new(tc, exception))
        } else {
          let text = exception_text!(tc, "failed to evaluate module");
          return Err(RuntimeError::message(text));
        }
      }
    }
  };
  if matches!(&outcome, ModuleRun::Value(_)) {
    record_wasm_namespaces(scope);
  }
  Ok(outcome)
}

/// Sync Wasm wrappers store their instance before `evaluate` returns.
/// Point the evaluated namespace and the deferred namespace at that instance.
/// `WebAssembly.namespaceInstance` calls this again on a miss, so a module
/// that evaluates later through `import defer` is still found.
pub(crate) fn record_wasm_namespaces(scope: &mut v8::PinScope) {
  let stored = host::with(|state| state.wasm_instances.clone());
  let Some(stored) = stored else {
    return;
  };
  let modules = host::with(|state| {
    state
      .modules
      .values()
      .filter(|entry| entry.kind == ModuleRecordKind::Source)
      .map(|entry| (entry.url.clone(), entry.module.clone()))
      .collect::<Vec<_>>()
  });
  v8::tc_scope!(let scope, scope);
  let map = v8::Local::new(scope, stored);
  if map.size() == 0 {
    return;
  }
  for (url, module) in modules {
    let Some(key) = v8::String::new(scope, &url) else {
      return;
    };
    let Some(instance) = map.get(scope, key.into()) else {
      return;
    };
    if instance.is_null_or_undefined() {
      continue;
    }
    let module = v8::Local::new(scope, module);
    if !module_linked(module) {
      continue;
    }
    let namespace = module.get_module_namespace();
    let namespace = v8::Global::new(scope, namespace);
    let namespace = v8::Local::new(scope, namespace);
    if map.set(scope, namespace, instance).is_none() {
      return;
    }
    let deferred =
      module.get_module_namespace_with_phase(v8::ModuleImportPhase::kDefer);
    let deferred = v8::Global::new(scope, deferred);
    let deferred = v8::Local::new(scope, deferred);
    if map.set(scope, deferred, instance).is_none() {
      return;
    }
  }
}

fn module_linked(module: v8::Local<v8::Module>) -> bool {
  matches!(
    module.get_status(),
    v8::ModuleStatus::Instantiated
      | v8::ModuleStatus::Evaluating
      | v8::ModuleStatus::Evaluated
      | v8::ModuleStatus::Errored
  )
}

fn compile_path<'a>(
  scope: &mut v8::PinScope<'a, '_>,
  path: &Path,
  url: &str,
  attributes: Option<&str>,
) -> Result<v8::Local<'a, v8::Module>, RuntimeError> {
  let extension = path.extension().and_then(|ext| ext.to_str()).unwrap_or("");
  let kind = module_kind(attributes, extension);
  if let Some(existing) = existing_module(scope, url, kind) {
    return Ok(existing);
  }
  // `type` wins over the extension, so `file.wasm` with `{ type: "text" }`
  // is the file text and `file.json` with `{ type: "bytes" }` is the raw bytes.
  if kind == ModuleRecordKind::Text {
    return synthetic_text(scope, url, path);
  }
  if kind == ModuleRecordKind::Bytes {
    return synthetic_bytes(scope, url, path);
  }
  if extension == "wasm" {
    let bytes = fs::read(path).map_err(|err| {
      RuntimeError::message(format!("{}: {err}", path.display()))
    })?;
    let source =
      crate::loader::wasm_module::wrapper_source(scope, url, &bytes)?;
    return compile_prepared(scope, url, path, &source);
  }
  if path.to_string_lossy().ends_with(".d.ts")
    || path.to_string_lossy().ends_with(".d.mts")
  {
    return Err(RuntimeError::message(format!(
      "cannot execute declaration file {}",
      path.display()
    )));
  }
  if extension == "json" {
    if attributes != Some("json") {
      return Err(RuntimeError::message(format!(
        "JSON module {} must be imported with {{ type: \"json\" }}",
        path.display()
      )));
    }
    let text = fs::read_to_string(path).map_err(|err| {
      RuntimeError::message(format!("{}: {err}", path.display()))
    })?;
    return synthetic_json(scope, url, path, &text);
  }

  let bytes = fs::read(path).map_err(|err| {
    RuntimeError::message(format!("{}: {err}", path.display()))
  })?;
  let source = String::from_utf8(bytes).map_err(|_| {
    RuntimeError::message(format!("{} is not valid UTF-8", path.display()))
  })?;
  let code = cached_source(path, &source)?;
  compile_prepared(scope, url, path, &code)
}

fn module_kind(attributes: Option<&str>, extension: &str) -> ModuleRecordKind {
  match attributes {
    Some("text") => ModuleRecordKind::Text,
    Some("bytes") => ModuleRecordKind::Bytes,
    Some("json") if extension == "json" => ModuleRecordKind::Json,
    _ => ModuleRecordKind::Source,
  }
}

fn compile_prepared<'a>(
  scope: &mut v8::PinScope<'a, '_>,
  url: &str,
  path: &Path,
  code: &str,
) -> Result<v8::Local<'a, v8::Module>, RuntimeError> {
  let module = {
    v8::tc_scope!(let tc, scope);
    let Some(text) = v8::String::new(tc, code) else {
      return Err(RuntimeError::message(format!(
        "{} is too large to compile",
        path.display()
      )));
    };
    let name = v8::String::new(tc, url).expect("module url");
    let origin = v8::ScriptOrigin::new(
      tc,
      name.into(),
      0,
      0,
      false,
      -1,
      None,
      false,
      false,
      true,
      None,
    );
    let mut source = v8::script_compiler::Source::new(text, Some(&origin));
    match v8::script_compiler::compile_module(tc, &mut source) {
      Some(module) => v8::Global::new(tc, module),
      None => {
        let text = exception_text!(tc, &format!("failed to compile {url}"));
        return Err(RuntimeError::message(text));
      }
    }
  };
  let module = v8::Local::new(scope, module);
  remember(scope, url, path, ModuleRecordKind::Source, module);
  Ok(module)
}

fn synthetic_json<'a>(
  scope: &mut v8::PinScope<'a, '_>,
  url: &str,
  path: &Path,
  text: &str,
) -> Result<v8::Local<'a, v8::Module>, RuntimeError> {
  let name = v8::String::new(scope, url).expect("module url");
  let export_name = v8::String::new(scope, "default").expect("default");
  let module = v8::Module::create_synthetic_module(
    scope,
    name,
    &[export_name],
    evaluate_json,
  );
  Ok(bind_synthetic(
    scope,
    url,
    path,
    ModuleRecordKind::Json,
    module,
    SyntheticSource::Json(text.to_string()),
  ))
}

fn synthetic_text<'a>(
  scope: &mut v8::PinScope<'a, '_>,
  url: &str,
  path: &Path,
) -> Result<v8::Local<'a, v8::Module>, RuntimeError> {
  let bytes = read_bytes(path)?;
  let text = String::from_utf8_lossy(&bytes).into_owned();
  let name = v8::String::new(scope, url).expect("module url");
  let export_name = v8::String::new(scope, "default").expect("default");
  let module = v8::Module::create_synthetic_module(
    scope,
    name,
    &[export_name],
    evaluate_text,
  );
  Ok(bind_synthetic(
    scope,
    url,
    path,
    ModuleRecordKind::Text,
    module,
    SyntheticSource::Text(text),
  ))
}

fn synthetic_bytes<'a>(
  scope: &mut v8::PinScope<'a, '_>,
  url: &str,
  path: &Path,
) -> Result<v8::Local<'a, v8::Module>, RuntimeError> {
  let bytes = read_bytes(path)?;
  let name = v8::String::new(scope, url).expect("module url");
  let export_name = v8::String::new(scope, "default").expect("default");
  let module = v8::Module::create_synthetic_module(
    scope,
    name,
    &[export_name],
    evaluate_bytes,
  );
  Ok(bind_synthetic(
    scope,
    url,
    path,
    ModuleRecordKind::Bytes,
    module,
    SyntheticSource::Bytes(bytes),
  ))
}

fn read_bytes(path: &Path) -> Result<Vec<u8>, RuntimeError> {
  fs::read(path)
    .map_err(|err| RuntimeError::message(format!("{}: {err}", path.display())))
}

fn bind_synthetic<'a>(
  scope: &mut v8::PinScope<'a, '_>,
  url: &str,
  path: &Path,
  kind: ModuleRecordKind,
  module: v8::Local<'a, v8::Module>,
  payload: SyntheticSource,
) -> v8::Local<'a, v8::Module> {
  let hash = module.get_identity_hash().get();
  host::with_mut(|state| {
    state.synthetic_sources.insert(hash, payload);
  });
  remember(scope, url, path, kind, module);
  module
}

fn synthetic_payload(hash: i32) -> Option<SyntheticSource> {
  host::with(|state| state.synthetic_sources.get(&hash).cloned())
}

fn evaluate_json<'s>(
  context: v8::Local<'s, v8::Context>,
  module: v8::Local<'s, v8::Module>,
) -> Option<v8::Local<'s, v8::Value>> {
  v8::callback_scope!(unsafe scope, context);
  let scope = &mut v8::ContextScope::new(scope, context);
  let hash = module.get_identity_hash().get();
  let text = match synthetic_payload(hash) {
    Some(SyntheticSource::Json(text)) => text,
    _ => "null".to_string(),
  };
  let source = v8::String::new(scope, &text)?;
  let value = v8::json::parse(scope, source)?;
  let name = v8::String::new(scope, "default")?;
  module.set_synthetic_module_export(scope, name, value)?;
  Some(v8::undefined(scope).into())
}

fn evaluate_text<'s>(
  context: v8::Local<'s, v8::Context>,
  module: v8::Local<'s, v8::Module>,
) -> Option<v8::Local<'s, v8::Value>> {
  v8::callback_scope!(unsafe scope, context);
  let scope = &mut v8::ContextScope::new(scope, context);
  let hash = module.get_identity_hash().get();
  let SyntheticSource::Text(text) = synthetic_payload(hash)? else {
    return None;
  };
  let value = v8::String::new(scope, &text)?;
  let name = v8::String::new(scope, "default")?;
  module.set_synthetic_module_export(scope, name, value.into())?;
  Some(v8::undefined(scope).into())
}

fn evaluate_bytes<'s>(
  context: v8::Local<'s, v8::Context>,
  module: v8::Local<'s, v8::Module>,
) -> Option<v8::Local<'s, v8::Value>> {
  v8::callback_scope!(unsafe scope, context);
  let scope = &mut v8::ContextScope::new(scope, context);
  let hash = module.get_identity_hash().get();
  let SyntheticSource::Bytes(bytes) = synthetic_payload(hash)? else {
    return None;
  };
  let view = immutable_uint8_array(scope, &bytes)?;
  let name = v8::String::new(scope, "default")?;
  module.set_synthetic_module_export(scope, name, view.into())?;
  Some(v8::undefined(scope).into())
}

/// rusty_v8 does not expose `MakeImmutable`. `transferToImmutable` detaches
/// the buffer it is called on and returns the immutable buffer the view uses.
fn immutable_uint8_array<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  bytes: &[u8],
) -> Option<v8::Local<'s, v8::Uint8Array>> {
  let buffer = v8::ArrayBuffer::new(scope, bytes.len());
  if !bytes.is_empty() {
    let data = buffer.data()?;
    // `data` addresses `bytes.len()` bytes of a buffer that is still attached.
    unsafe {
      std::ptr::copy_nonoverlapping(
        bytes.as_ptr(),
        data.as_ptr() as *mut u8,
        bytes.len(),
      );
    }
  }
  let key = v8::String::new(scope, "transferToImmutable")?;
  let method = buffer.get(scope, key.into())?;
  let function = v8::Local::<v8::Function>::try_from(method).ok()?;
  let transferred = function.call(scope, buffer.into(), &[])?;
  let immutable = v8::Local::<v8::ArrayBuffer>::try_from(transferred).ok()?;
  v8::Uint8Array::new(scope, immutable, 0, bytes.len())
}

fn resolve_callback<'s>(
  context: v8::Local<'s, v8::Context>,
  specifier: v8::Local<'s, v8::String>,
  import_attributes: v8::Local<'s, v8::FixedArray>,
  referrer: v8::Local<'s, v8::Module>,
) -> Option<v8::Local<'s, v8::Module>> {
  v8::callback_scope!(unsafe scope, context);
  let scope = &mut v8::ContextScope::new(scope, context);
  let spec = specifier.to_rust_string_lossy(scope);
  let referrer_hash = referrer.get_identity_hash().get();
  let referrer_path = host::module_path(referrer_hash);
  let Some(referrer_path) = referrer_path else {
    throw_type(scope, "cannot resolve an import without a referrer");
    return None;
  };
  let attribute = import_type(scope, import_attributes);
  let compiled =
    resolve_and_compile(scope, &referrer_path, &spec, attribute.as_deref());
  match compiled {
    Ok(module) => {
      // The module handle is allocated in this callback's scope, which
      // stays on the stack until V8 copies the returned local.
      Some(unsafe {
        std::mem::transmute::<v8::Local<v8::Module>, v8::Local<'s, v8::Module>>(
          module,
        )
      })
    }
    Err(error) => {
      throw_type(scope, &error.message);
      None
    }
  }
}

fn source_callback<'s>(
  context: v8::Local<'s, v8::Context>,
  specifier: v8::Local<'s, v8::String>,
  import_attributes: v8::Local<'s, v8::FixedArray>,
  referrer: v8::Local<'s, v8::Module>,
) -> Option<v8::Local<'s, v8::Object>> {
  v8::callback_scope!(unsafe scope, context);
  let scope = &mut v8::ContextScope::new(scope, context);
  let spec = specifier.to_rust_string_lossy(scope);
  let referrer_hash = referrer.get_identity_hash().get();
  let Some(referrer_path) = host::module_path(referrer_hash) else {
    throw_type(scope, "cannot resolve an import without a referrer");
    return None;
  };
  let attribute = import_type(scope, import_attributes);
  match load_source(scope, &referrer_path, &spec, attribute.as_deref()) {
    Ok(module) => {
      // The module object is allocated in this callback's scope, which
      // stays on the stack until V8 copies the returned local.
      Some(unsafe {
        std::mem::transmute::<v8::Local<v8::Object>, v8::Local<'s, v8::Object>>(
          module,
        )
      })
    }
    Err(error) => {
      throw_type(scope, &error.message);
      None
    }
  }
}

const UNSUPPORTED_SOURCE_PHASE: &str = concat!(
  "import source is only supported for JavaScript, TypeScript, ",
  "and WebAssembly modules",
);

fn load_source<'a>(
  scope: &mut v8::PinScope<'a, '_>,
  referrer: &Path,
  specifier: &str,
  attribute: Option<&str>,
) -> Result<v8::Local<'a, v8::Object>, RuntimeError> {
  if matches!(attribute, Some("text" | "bytes" | "json")) {
    return Err(RuntimeError::message(UNSUPPORTED_SOURCE_PHASE));
  }
  let (path, url) = locate(referrer, specifier)?;
  let extension = path.extension().and_then(|ext| ext.to_str()).unwrap_or("");
  if extension == "wasm" {
    let bytes = fs::read(&path).map_err(|err| {
      RuntimeError::message(format!("{}: {err}", path.display()))
    })?;
    return crate::loader::wasm_module::source_object(scope, &url, &bytes);
  }
  if matches!(extension, "js" | "mjs" | "cjs" | "ts" | "mts") {
    compile_path(scope, &path, &url, None)?;
    return crate::loader::module_source::wrap(scope, &url);
  }
  Err(RuntimeError::message(UNSUPPORTED_SOURCE_PHASE))
}

fn resolve_and_compile<'a>(
  scope: &mut v8::PinScope<'a, '_>,
  referrer: &Path,
  specifier: &str,
  attribute: Option<&str>,
) -> Result<v8::Local<'a, v8::Module>, RuntimeError> {
  let (path, url) = locate(referrer, specifier)?;
  compile_path(scope, &path, &url, attribute)
}

fn locate(
  referrer: &Path,
  specifier: &str,
) -> Result<(PathBuf, String), RuntimeError> {
  if specifier.starts_with("node:") {
    return Err(RuntimeError::message(format!(
      "node: imports are not supported ({specifier})"
    )));
  }
  if specifier.starts_with("file:") {
    let path = url_to_path(specifier)?;
    let url = file_url(&path)?;
    return Ok((path, url));
  }
  let resolved =
    host::with(|state| state.resolver.resolve_file(referrer, specifier))
      .map_err(|error| {
        RuntimeError::message(format!(
          "failed to resolve \"{specifier}\" from {}: {error}",
          referrer.display()
        ))
      })?;
  if matches!(
    resolved.module_type(),
    Some(ModuleType::CommonJs | ModuleType::Addon)
  ) {
    return Err(RuntimeError::message(format!(
      "cannot load {} as an ES module",
      resolved.path().display()
    )));
  }
  let path = resolved.path().to_path_buf();
  let url = file_url(&path)?;
  Ok((path, url))
}

fn existing_module<'a>(
  scope: &mut v8::PinScope<'a, '_>,
  url: &str,
  kind: ModuleRecordKind,
) -> Option<v8::Local<'a, v8::Module>> {
  let global = host::with(|state| {
    state
      .modules
      .values()
      .find(|entry| entry.url == url && entry.kind == kind)
      .map(|entry| entry.module.clone())
  })?;
  Some(v8::Local::new(scope, global))
}

fn remember(
  scope: &mut v8::PinScope<'_, '_>,
  url: &str,
  path: &Path,
  kind: ModuleRecordKind,
  module: v8::Local<v8::Module>,
) {
  let hash = module.get_identity_hash().get();
  let global = v8::Global::new(scope, module);
  host::with_mut(|state| {
    state.modules.insert(
      hash,
      ModuleEntry {
        url: url.to_string(),
        path: path.to_path_buf(),
        kind,
        module: global,
      },
    );
  });
}

fn cached_source(path: &Path, source: &str) -> Result<String, RuntimeError> {
  let meta = fs::metadata(path).map_err(|error| {
    RuntimeError::message(format!("{}: {error}", path.display()))
  })?;
  let modified = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
  let len = meta.len();
  if let Some(code) = host::with(|state| {
    state.strip_cache.get(path).and_then(|(time, size, code)| {
      (*time == modified && *size == len).then(|| code.clone())
    })
  }) {
    return Ok(code);
  }
  let code = prepare_source(path, source)?;
  host::with_mut(|state| {
    state
      .strip_cache
      .insert(path.to_path_buf(), (modified, len, code.clone()));
  });
  Ok(code)
}

fn swallow_rejection(
  _scope: &mut v8::PinScope,
  _args: v8::FunctionCallbackArguments,
  mut _ret: v8::ReturnValue,
) {
}

fn import_type(
  scope: &mut v8::PinScope<'_, '_>,
  attributes: v8::Local<v8::FixedArray>,
) -> Option<String> {
  let len = attributes.length();
  let mut index = 0;
  while index + 1 < len {
    let key = attributes.get(scope, index)?;
    let value = attributes.get(scope, index + 1)?;
    let Ok(key) = v8::Local::<v8::String>::try_from(key) else {
      index += 1;
      continue;
    };
    let Ok(value) = v8::Local::<v8::String>::try_from(value) else {
      index += 1;
      continue;
    };
    if key.to_rust_string_lossy(scope) == "type" {
      return Some(value.to_rust_string_lossy(scope));
    }
    index += 2;
  }
  None
}

pub fn dynamic_import<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  resource_name: v8::Local<'s, v8::Value>,
  specifier: v8::Local<'s, v8::String>,
  import_attributes: v8::Local<'s, v8::FixedArray>,
) -> Option<v8::Local<'s, v8::Promise>> {
  let resolver = v8::PromiseResolver::new(scope)?;
  let promise = resolver.get_promise(scope);
  let spec = specifier.to_rust_string_lossy(scope);
  let referrer = resource_name.to_rust_string_lossy(scope);
  let attribute = import_type(scope, import_attributes);
  let referrer_path =
    url_to_path(&referrer).or_else(|_| absolute(Path::new(&referrer)));
  let loaded = referrer_path.and_then(|path| {
    resolve_and_compile(scope, &path, &spec, attribute.as_deref())
  });
  match loaded {
    Ok(module) => match run_module(scope, module) {
      Ok(ModuleRun::Thrown(thrown)) => {
        let reason = v8::Local::new(scope, thrown);
        resolver.reject(scope, reason);
      }
      Ok(ModuleRun::Value(value)) => {
        let value = v8::Local::new(scope, value);
        if let Ok(evaluation) = v8::Local::<v8::Promise>::try_from(value)
          && evaluation.state() == v8::PromiseState::Rejected
        {
          // `import()` must reject with the evaluation exception. Marking
          // the evaluation promise handled keeps it off `unhandledrejection`.
          evaluation.mark_as_handled();
          let reason = evaluation.result(scope);
          if let Some(swallow) = v8::Function::new(scope, swallow_rejection) {
            let _ = evaluation.catch(scope, swallow);
          }
          resolver.reject(scope, reason);
        } else {
          let namespace = module.get_module_namespace();
          resolver.resolve(scope, namespace);
        }
      }
      Err(error) => {
        let message = v8::String::new(scope, &error.message).unwrap();
        let exception = v8::Exception::error(scope, message);
        resolver.reject(scope, exception);
      }
    },
    Err(error) => {
      let message = v8::String::new(scope, &error.message).unwrap();
      let exception = v8::Exception::type_error(scope, message);
      resolver.reject(scope, exception);
    }
  }
  Some(promise)
}

pub fn dynamic_import_source<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  resource_name: v8::Local<'s, v8::Value>,
  specifier: v8::Local<'s, v8::String>,
  import_attributes: v8::Local<'s, v8::FixedArray>,
) -> Option<v8::Local<'s, v8::Promise>> {
  let resolver = v8::PromiseResolver::new(scope)?;
  let promise = resolver.get_promise(scope);
  let spec = specifier.to_rust_string_lossy(scope);
  let referrer = resource_name.to_rust_string_lossy(scope);
  let attribute = import_type(scope, import_attributes);
  let referrer_path =
    url_to_path(&referrer).or_else(|_| absolute(Path::new(&referrer)));
  let loaded = referrer_path
    .and_then(|path| load_source(scope, &path, &spec, attribute.as_deref()));
  match loaded {
    Ok(module) => {
      resolver.resolve(scope, module.into());
    }
    Err(error) => {
      let message = v8::String::new(scope, &error.message).unwrap();
      let exception = v8::Exception::type_error(scope, message);
      resolver.reject(scope, exception);
    }
  }
  Some(promise)
}

/// `import.defer()`. Link the graph and evaluate asynchronous dependencies.
/// The returned promise fulfills with the deferred namespace. The module body
/// stays unevaluated until that namespace is observed.
pub fn dynamic_import_defer<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  resource_name: v8::Local<'s, v8::Value>,
  specifier: v8::Local<'s, v8::String>,
  import_attributes: v8::Local<'s, v8::FixedArray>,
) -> Option<v8::Local<'s, v8::Promise>> {
  let resolver = v8::PromiseResolver::new(scope)?;
  let promise = resolver.get_promise(scope);
  let spec = specifier.to_rust_string_lossy(scope);
  let referrer = resource_name.to_rust_string_lossy(scope);
  let attribute = import_type(scope, import_attributes);
  let referrer_path =
    url_to_path(&referrer).or_else(|_| absolute(Path::new(&referrer)));
  let loaded = referrer_path.and_then(|path| {
    resolve_and_compile(scope, &path, &spec, attribute.as_deref())
  });
  let module = match loaded {
    Ok(module) => module,
    Err(error) => {
      reject_message(scope, resolver, &error.message, true);
      return Some(promise);
    }
  };
  if let Err(fail) = link_module(scope, module) {
    reject_defer(scope, resolver, fail);
    return Some(promise);
  }
  let evaluation = match defer_evaluation(scope, module) {
    Ok(value) => value,
    Err(fail) => {
      reject_defer(scope, resolver, fail);
      return Some(promise);
    }
  };
  let evaluation = v8::Local::new(scope, evaluation);
  let Ok(inner) = v8::Local::<v8::Promise>::try_from(evaluation) else {
    reject_message(
      scope,
      resolver,
      "import.defer did not return a promise",
      true,
    );
    return Some(promise);
  };
  match inner.state() {
    v8::PromiseState::Rejected => {
      settle_rejected_defer(scope, resolver, inner);
    }
    v8::PromiseState::Fulfilled => {
      resolve_deferred_namespace(scope, resolver, module);
    }
    v8::PromiseState::Pending => {
      watch_deferred_import(scope, resolver, module, inner);
    }
  }
  Some(promise)
}

enum DeferFail {
  Thrown(v8::Global<v8::Value>),
  Message(String),
}

fn link_module(
  scope: &mut v8::PinScope,
  module: v8::Local<v8::Module>,
) -> Result<(), DeferFail> {
  v8::tc_scope!(let tc, scope);
  if module
    .instantiate_module2(tc, resolve_callback, source_callback)
    .is_some()
  {
    return Ok(());
  }
  if let Some(exception) = tc.exception() {
    return Err(DeferFail::Thrown(v8::Global::new(tc, exception)));
  }
  Err(DeferFail::Message(exception_text!(
    tc,
    "failed to instantiate module"
  )))
}

fn defer_evaluation(
  scope: &mut v8::PinScope,
  module: v8::Local<v8::Module>,
) -> Result<v8::Global<v8::Value>, DeferFail> {
  v8::tc_scope!(let tc, scope);
  match module.evaluate_for_import_defer(tc) {
    Some(value) => Ok(v8::Global::new(tc, value)),
    None => {
      if let Some(exception) = tc.exception() {
        Err(DeferFail::Thrown(v8::Global::new(tc, exception)))
      } else {
        Err(DeferFail::Message(exception_text!(
          tc,
          "failed to defer module evaluation"
        )))
      }
    }
  }
}

fn reject_message(
  scope: &mut v8::PinScope,
  resolver: v8::Local<v8::PromiseResolver>,
  message: &str,
  type_error: bool,
) {
  let message = v8::String::new(scope, message).unwrap();
  let exception = if type_error {
    v8::Exception::type_error(scope, message)
  } else {
    v8::Exception::error(scope, message)
  };
  resolver.reject(scope, exception);
}

fn reject_defer(
  scope: &mut v8::PinScope,
  resolver: v8::Local<v8::PromiseResolver>,
  fail: DeferFail,
) {
  match fail {
    DeferFail::Thrown(thrown) => {
      let reason = v8::Local::new(scope, thrown);
      resolver.reject(scope, reason);
    }
    DeferFail::Message(text) => {
      reject_message(scope, resolver, &text, false);
    }
  }
}

fn settle_rejected_defer(
  scope: &mut v8::PinScope,
  resolver: v8::Local<v8::PromiseResolver>,
  inner: v8::Local<v8::Promise>,
) {
  // The user promise rejects with the same reason. Mark the inner promise
  // handled so it does not also fire `unhandledrejection`.
  inner.mark_as_handled();
  if let Some(swallow) = v8::Function::new(scope, swallow_rejection) {
    let _ = inner.catch(scope, swallow);
  }
  let reason = inner.result(scope);
  resolver.reject(scope, reason);
}

fn resolve_deferred_namespace(
  scope: &mut v8::PinScope,
  resolver: v8::Local<v8::PromiseResolver>,
  module: v8::Local<v8::Module>,
) {
  let Some(namespace) = deferred_namespace(scope, module) else {
    reject_message(
      scope,
      resolver,
      "deferred module namespace is not available",
      true,
    );
    return;
  };
  resolver.resolve(scope, namespace);
}

fn deferred_namespace<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  module: v8::Local<v8::Module>,
) -> Option<v8::Local<'s, v8::Value>> {
  if !module_linked(module) {
    return None;
  }
  // The namespace cell is stored on the module before this returns, so the
  // raw local stays reachable from `module` until it is rooted.
  let namespace =
    module.get_module_namespace_with_phase(v8::ModuleImportPhase::kDefer);
  let namespace = v8::Global::new(scope, namespace);
  Some(v8::Local::new(scope, namespace))
}

fn watch_deferred_import(
  scope: &mut v8::PinScope,
  resolver: v8::Local<v8::PromiseResolver>,
  module: v8::Local<v8::Module>,
  inner: v8::Local<v8::Promise>,
) {
  let id = alloc_deferred_import(scope, resolver, module);
  let data = v8::Integer::new(scope, id);
  let Some(fulfill) = v8::Function::builder(defer_import_fulfilled)
    .data(data.into())
    .build(scope)
  else {
    abandon_deferred_import(scope, resolver, inner, id);
    return;
  };
  let Some(reject) = v8::Function::builder(defer_import_rejected)
    .data(data.into())
    .build(scope)
  else {
    abandon_deferred_import(scope, resolver, inner, id);
    return;
  };
  if inner.then2(scope, fulfill, reject).is_none() {
    abandon_deferred_import(scope, resolver, inner, id);
  }
}

fn abandon_deferred_import(
  scope: &mut v8::PinScope,
  resolver: v8::Local<v8::PromiseResolver>,
  inner: v8::Local<v8::Promise>,
  id: i32,
) {
  drop_deferred_import(id);
  inner.mark_as_handled();
  if let Some(swallow) = v8::Function::new(scope, swallow_rejection) {
    let _ = inner.catch(scope, swallow);
  }
  reject_message(scope, resolver, "failed to defer module evaluation", false);
}

fn alloc_deferred_import(
  scope: &mut v8::PinScope,
  resolver: v8::Local<v8::PromiseResolver>,
  module: v8::Local<v8::Module>,
) -> i32 {
  let resolver = v8::Global::new(scope, resolver);
  let module = v8::Global::new(scope, module);
  host::with_mut(|state| {
    loop {
      let id = state.next_deferred_import;
      state.next_deferred_import = state.next_deferred_import.wrapping_add(1);
      if id != 0 && !state.deferred_imports.contains_key(&id) {
        state
          .deferred_imports
          .insert(id, DeferredImport { resolver, module });
        return id;
      }
    }
  })
}

fn take_deferred_import(id: i32) -> Option<DeferredImport> {
  host::with_mut(|state| state.deferred_imports.remove(&id))
}

fn drop_deferred_import(id: i32) {
  host::with_mut(|state| {
    state.deferred_imports.remove(&id);
  });
}

fn deferred_import_id(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
) -> Option<i32> {
  let data = args.data();
  if !data.is_int32() {
    return None;
  }
  data.int32_value(scope)
}

fn defer_import_fulfilled(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut _ret: v8::ReturnValue,
) {
  // A throw here would reject the native `then` promise with no handler.
  let Some(id) = deferred_import_id(scope, &args) else {
    return;
  };
  let Some(job) = take_deferred_import(id) else {
    return;
  };
  let resolver = v8::Local::new(scope, job.resolver);
  let module = v8::Local::new(scope, job.module);
  resolve_deferred_namespace(scope, resolver, module);
}

fn defer_import_rejected(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut _ret: v8::ReturnValue,
) {
  let Some(id) = deferred_import_id(scope, &args) else {
    return;
  };
  let Some(job) = take_deferred_import(id) else {
    return;
  };
  let resolver = v8::Local::new(scope, job.resolver);
  resolver.reject(scope, args.get(0));
}

pub fn absolute(path: &Path) -> Result<PathBuf, RuntimeError> {
  if path.is_absolute() {
    Ok(path.to_path_buf())
  } else {
    let cwd = std::env::current_dir()
      .map_err(|error| RuntimeError::message(error.to_string()))?;
    Ok(cwd.join(path))
  }
}

/// `std::fs::canonicalize` without the Windows verbatim prefix.
///
/// Windows returns `\\?\C:\dir` and `\\?\UNC\server\share`. A `file:` URL
/// converts back to `C:\dir` and `\\server\share`, so a sandbox check that
/// compares the two forms would reject every file. Other systems return the
/// canonical path unchanged.
pub fn canonicalize(path: &Path) -> std::io::Result<PathBuf> {
  let canonical = std::fs::canonicalize(path)?;
  Ok(strip_verbatim(canonical))
}

#[cfg(windows)]
fn strip_verbatim(path: PathBuf) -> PathBuf {
  use std::path::{Component, Prefix};
  let mut components = path.components();
  let Some(Component::Prefix(prefix)) = components.next() else {
    return path;
  };
  let mut plain = match prefix.kind() {
    Prefix::VerbatimDisk(letter) => {
      PathBuf::from(format!("{}:\\", char::from(letter)))
    }
    Prefix::VerbatimUNC(server, share) => {
      let mut root = std::ffi::OsString::from(r"\\");
      root.push(server);
      root.push(r"\");
      root.push(share);
      root.push(r"\");
      PathBuf::from(root)
    }
    _ => return path,
  };
  for component in components {
    if !matches!(component, Component::RootDir) {
      plain.push(component);
    }
  }
  plain
}

#[cfg(not(windows))]
fn strip_verbatim(path: PathBuf) -> PathBuf {
  path
}

pub fn file_url(path: &Path) -> Result<String, RuntimeError> {
  let absolute = absolute(path)?;
  Url::from_file_path(&absolute)
    .map(|url| url.to_string())
    .map_err(|()| {
      RuntimeError::message(format!(
        "cannot form a file URL for {}",
        absolute.display()
      ))
    })
}

/// Files in the module graph of `entry`, including `entry`.
///
/// Static imports and re-exports are followed, including `import defer`
/// and `import source`. Type-only imports are left out. An import whose
/// attribute is `type: "text"` or `type: "bytes"` copies the file and
/// leaves its source unread. `import()`, `import.defer()`, and
/// `import.source()` are followed when the argument is one concrete
/// specifier: a string literal, a template literal with no substitution,
/// those strings joined by `+`, `import.meta.resolve` of such a specifier,
/// or `new URL(specifier, import.meta.url)`, including `.href` and
/// `.toString()`. A dynamic specifier that does not resolve is left out.
/// A computed specifier and a `Worker` URL are left out.
pub fn module_graph(entry: &Path) -> Result<Vec<PathBuf>, RuntimeError> {
  let entry = absolute(entry)?;
  let resolver = resolver();
  let mut pending = vec![(entry, true)];
  let mut copied = std::collections::BTreeSet::new();
  let mut parsed = std::collections::BTreeSet::new();
  let mut files = Vec::new();
  while let Some((path, parse)) = pending.pop() {
    if copied.insert(path.clone()) {
      files.push(path.clone());
    }
    if !parse || !parsed.insert(path.clone()) {
      continue;
    }
    let extension = path.extension().and_then(|ext| ext.to_str()).unwrap_or("");
    if !matches!(extension, "js" | "mjs" | "cjs" | "ts" | "mts") {
      continue;
    }
    let source = fs::read_to_string(&path).map_err(|error| {
      RuntimeError::message(format!("{}: {error}", path.display()))
    })?;
    for edge in graph::edges(&path, &source)? {
      match resolve_with(&resolver, &path, &edge.specifier) {
        Ok(resolved) => {
          if edge.dynamic && !resolved.is_file() {
            continue;
          }
          pending.push((resolved, edge.parse));
        }
        Err(_) if edge.dynamic => {}
        Err(error) => return Err(error),
      }
    }
  }
  Ok(files)
}

fn resolve_with(
  resolver: &Resolver,
  referrer: &Path,
  specifier: &str,
) -> Result<PathBuf, RuntimeError> {
  if specifier.starts_with("node:") {
    return Err(RuntimeError::message(format!(
      "node: imports are not supported ({specifier})"
    )));
  }
  if specifier.starts_with("file:") {
    return url_to_path(specifier);
  }
  if specifier.contains(':')
    && !specifier.starts_with('.')
    && !specifier.starts_with('/')
  {
    return Err(RuntimeError::message(format!(
      "cannot bundle \"{specifier}\""
    )));
  }
  let resolved =
    resolver
      .resolve_file(referrer, specifier)
      .map_err(|error| {
        RuntimeError::message(format!(
          "failed to resolve \"{specifier}\" from {}: {error}",
          referrer.display()
        ))
      })?;
  if matches!(
    resolved.module_type(),
    Some(ModuleType::CommonJs | ModuleType::Addon)
  ) {
    return Err(RuntimeError::message(format!(
      "cannot load {} as an ES module",
      resolved.path().display()
    )));
  }
  Ok(resolved.path().to_path_buf())
}

mod graph {
  use std::path::Path;

  use oxc_ast::ast::{
    BinaryOperator, CallExpression, ExportAllDeclaration,
    ExportFromDeclaration, Expression, ImportAttributeKey, ImportDeclaration,
    ImportExpression, ImportOrExportKind, NewExpression, ObjectPropertyKind,
    PropertyKey, PropertyKind, StaticMemberExpression, StringLiteral,
    TemplateLiteral, WithClause, WithClauseKeyword,
  };
  use oxc_ast_visit::Visit;
  use oxc_ast_visit::walk::{
    walk_export_all_declaration, walk_export_from_declaration,
    walk_import_declaration, walk_import_expression,
  };

  use crate::RuntimeError;

  pub(super) struct Edge {
    pub specifier: String,
    pub dynamic: bool,
    /// False for `type: "text"` and `type: "bytes"`. The file is copied
    /// and its imports stay unread.
    pub parse: bool,
  }

  pub(super) fn edges(
    path: &Path,
    source: &str,
  ) -> Result<Vec<Edge>, RuntimeError> {
    let allocator = oxc_allocator::Allocator::default();
    let source_type =
      oxc_span::SourceType::from_path(path).map_err(|error| {
        RuntimeError::message(format!("{}: {error}", path.display()))
      })?;
    let parsed =
      oxc_parser::Parser::new(&allocator, source, source_type).parse();
    if let Some(error) = parsed.diagnostics.first() {
      return Err(RuntimeError::message(format!(
        "{}: {error}",
        path.display()
      )));
    }
    let mut visitor = Visitor { edges: Vec::new() };
    visitor.visit_program(&parsed.program);
    Ok(visitor.edges)
  }

  struct Visitor {
    edges: Vec<Edge>,
  }

  impl Visitor {
    fn push_static(
      &mut self,
      kind: ImportOrExportKind,
      source: &StringLiteral<'_>,
      with_clause: Option<&WithClause<'_>>,
    ) {
      if kind == ImportOrExportKind::Type {
        return;
      }
      self.edges.push(Edge {
        specifier: source.value.to_string(),
        dynamic: false,
        parse: !clause_is_raw(with_clause),
      });
    }
  }

  impl<'a> Visit<'a> for Visitor {
    fn visit_import_declaration(
      &mut self,
      declaration: &ImportDeclaration<'a>,
    ) {
      self.push_static(
        declaration.import_kind,
        &declaration.source,
        declaration.with_clause.as_deref(),
      );
      walk_import_declaration(self, declaration);
    }

    fn visit_export_from_declaration(
      &mut self,
      declaration: &ExportFromDeclaration<'a>,
    ) {
      self.push_static(
        declaration.export_kind,
        &declaration.source,
        declaration.with_clause.as_deref(),
      );
      walk_export_from_declaration(self, declaration);
    }

    fn visit_export_all_declaration(
      &mut self,
      declaration: &ExportAllDeclaration<'a>,
    ) {
      self.push_static(
        declaration.export_kind,
        &declaration.source,
        declaration.with_clause.as_deref(),
      );
      walk_export_all_declaration(self, declaration);
    }

    fn visit_import_expression(&mut self, expression: &ImportExpression<'a>) {
      if let Some(specifier) = concrete_specifier(&expression.source) {
        self.edges.push(Edge {
          specifier,
          dynamic: true,
          parse: !dynamic_import_is_raw(expression.options.as_ref()),
        });
      }
      walk_import_expression(self, expression);
    }
  }

  /// `with { type: "text" }` or `with { type: "bytes" }`.
  fn clause_is_raw(clause: Option<&WithClause<'_>>) -> bool {
    let Some(clause) = clause else {
      return false;
    };
    if clause.keyword != WithClauseKeyword::With {
      return false;
    }
    clause
      .with_entries
      .iter()
      .find_map(|entry| {
        let key = match &entry.key {
          ImportAttributeKey::Identifier(name) => name.name.as_str(),
          ImportAttributeKey::StringLiteral(literal) => literal.value.as_str(),
        };
        (key == "type").then_some(entry.value.value.as_str())
      })
      .is_some_and(|value| matches!(value, "text" | "bytes"))
  }

  /// `import(specifier, { with: { type: "text" | "bytes" } })` when the
  /// options value is an object literal.
  fn dynamic_import_is_raw(options: Option<&Expression<'_>>) -> bool {
    let Some(options) = options else {
      return false;
    };
    let Some(with_value) = object_property(options, "with") else {
      return false;
    };
    let Some(type_value) = object_property(with_value, "type") else {
      return false;
    };
    let Expression::StringLiteral(literal) = type_value else {
      return false;
    };
    matches!(literal.value.as_str(), "text" | "bytes")
  }

  /// The value of the first property named `name` in an object literal.
  /// A spread or a computed key hides the rest of the object.
  fn object_property<'a>(
    object: &'a Expression<'a>,
    name: &str,
  ) -> Option<&'a Expression<'a>> {
    let Expression::ObjectExpression(object) = peel(object) else {
      return None;
    };
    for property in &object.properties {
      let ObjectPropertyKind::ObjectProperty(property) = property else {
        return None;
      };
      if property.computed {
        return None;
      }
      let key = match &property.key {
        PropertyKey::StaticIdentifier(ident) => ident.name.as_str(),
        PropertyKey::StringLiteral(literal) => literal.value.as_str(),
        _ => return None,
      };
      if key != name {
        continue;
      }
      if property.method
        || property.shorthand
        || property.kind != PropertyKind::Init
      {
        return None;
      }
      return Some(peel(&property.value));
    }
    None
  }

  /// One specifier an `import()` argument can name without execution.
  fn concrete_specifier(expression: &Expression<'_>) -> Option<String> {
    match peel(expression) {
      Expression::StringLiteral(literal) => Some(literal.value.to_string()),
      Expression::TemplateLiteral(literal) => template_specifier(literal),
      Expression::BinaryExpression(binary)
        if binary.operator == BinaryOperator::Addition =>
      {
        let left = concrete_specifier(&binary.left)?;
        let right = concrete_specifier(&binary.right)?;
        Some(left + &right)
      }
      Expression::SequenceExpression(sequence) => {
        sequence.expressions.last().and_then(concrete_specifier)
      }
      Expression::CallExpression(call) => call_specifier(call),
      Expression::NewExpression(new) => new_url_specifier(new),
      Expression::StaticMemberExpression(member) => href_specifier(member),
      _ => None,
    }
  }

  fn peel<'a>(mut expression: &'a Expression<'a>) -> &'a Expression<'a> {
    loop {
      expression = match expression {
        Expression::ParenthesizedExpression(inner) => &inner.expression,
        Expression::TSAsExpression(inner) => &inner.expression,
        Expression::TSSatisfiesExpression(inner) => &inner.expression,
        Expression::TSTypeAssertion(inner) => &inner.expression,
        Expression::TSNonNullExpression(inner) => &inner.expression,
        _ => return expression,
      };
    }
  }

  fn template_specifier(literal: &TemplateLiteral<'_>) -> Option<String> {
    if !literal.expressions.is_empty() {
      return None;
    }
    let mut specifier = String::new();
    for quasi in &literal.quasis {
      specifier.push_str(quasi.value.cooked.as_ref()?.as_str());
    }
    Some(specifier)
  }

  fn call_specifier(call: &CallExpression<'_>) -> Option<String> {
    if call.optional {
      return None;
    }
    let Expression::StaticMemberExpression(member) = peel(&call.callee) else {
      return None;
    };
    if member.optional {
      return None;
    }
    match member.property.name.as_str() {
      "resolve" if is_import_meta(&member.object) => resolve_specifier(call),
      "toString" if call.arguments.is_empty() => new_url_from(&member.object),
      _ => None,
    }
  }

  fn resolve_specifier(call: &CallExpression<'_>) -> Option<String> {
    let mut arguments = call.arguments.iter();
    let specifier = concrete_specifier(arguments.next()?.as_expression()?)?;
    match arguments.next() {
      None => Some(specifier),
      Some(parent) => {
        if arguments.next().is_some()
          || !is_import_meta_url(parent.as_expression()?)
        {
          None
        } else {
          Some(specifier)
        }
      }
    }
  }

  fn href_specifier(member: &StaticMemberExpression<'_>) -> Option<String> {
    if member.optional || member.property.name.as_str() != "href" {
      return None;
    }
    new_url_from(&member.object)
  }

  fn new_url_from(expression: &Expression<'_>) -> Option<String> {
    match peel(expression) {
      Expression::NewExpression(new) => new_url_specifier(new),
      _ => None,
    }
  }

  fn new_url_specifier(new: &NewExpression<'_>) -> Option<String> {
    let Expression::Identifier(identifier) = peel(&new.callee) else {
      return None;
    };
    if identifier.name.as_str() != "URL" {
      return None;
    }
    let mut arguments = new.arguments.iter();
    let specifier = concrete_specifier(arguments.next()?.as_expression()?)?;
    let base = arguments.next()?.as_expression()?;
    if arguments.next().is_some() || !is_import_meta_url(base) {
      return None;
    }
    Some(specifier)
  }

  fn is_import_meta(expression: &Expression<'_>) -> bool {
    matches!(peel(expression), Expression::ImportMeta(_))
  }

  fn is_import_meta_url(expression: &Expression<'_>) -> bool {
    let Expression::StaticMemberExpression(member) = peel(expression) else {
      return false;
    };
    !member.optional
      && member.property.name.as_str() == "url"
      && is_import_meta(&member.object)
  }
}

fn url_to_path(specifier: &str) -> Result<PathBuf, RuntimeError> {
  let url = Url::parse(specifier).map_err(|error| {
    RuntimeError::message(format!("invalid module URL {specifier}: {error}"))
  })?;
  if url.scheme() != "file" {
    return Err(RuntimeError::message(format!(
      "only file URLs can be imported ({specifier})"
    )));
  }
  url.to_file_path().map_err(|()| {
    RuntimeError::message(format!("invalid file URL {specifier}"))
  })
}

fn throw_type(scope: &mut v8::PinScope<'_, '_>, message: &str) {
  let message = v8::String::new(scope, message).unwrap();
  let error = v8::Exception::type_error(scope, message);
  scope.throw_exception(error);
}

#[cfg(all(test, windows))]
mod verbatim_tests {
  use std::path::{Path, PathBuf};

  use super::{canonicalize, strip_verbatim};

  #[test]
  fn verbatim_prefixes_become_plain_paths() {
    assert_eq!(
      strip_verbatim(PathBuf::from(r"\\?\C:\games\app\main.ts")),
      PathBuf::from(r"C:\games\app\main.ts")
    );
    assert_eq!(
      strip_verbatim(PathBuf::from(r"\\?\UNC\host\share\app")),
      PathBuf::from(r"\\host\share\app")
    );
    assert_eq!(
      strip_verbatim(PathBuf::from(r"C:\plain")),
      PathBuf::from(r"C:\plain")
    );
  }

  #[test]
  fn canonical_paths_match_file_urls() {
    let here = canonicalize(Path::new(".")).unwrap();
    let url = url::Url::from_directory_path(&here).unwrap();
    assert!(url.to_file_path().unwrap().starts_with(&here), "{here:?}");
  }
}

#[cfg(test)]
mod graph_tests {
  use std::fs;
  use std::path::{Path, PathBuf};

  use super::module_graph;

  struct TempDir(PathBuf);

  impl Drop for TempDir {
    fn drop(&mut self) {
      let _ = fs::remove_dir_all(&self.0);
    }
  }

  fn scratch() -> TempDir {
    use std::sync::atomic::{AtomicU64, Ordering};
    // Parallel tests can start in the same clock tick. A counter keeps each
    // directory unique so one test cannot overwrite another's entry.
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir()
      .join(format!("yuzuko-graph-{}-{id}", std::process::id()));
    fs::create_dir_all(&path).unwrap();
    TempDir(path)
  }

  fn write(dir: &Path, name: &str, text: &str) {
    let path = dir.join(name);
    if let Some(parent) = path.parent() {
      fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, text).unwrap();
  }

  const ENTRY: &str = r#"
import { value } from "./static.ts";
import defer * as deferred from "./deferred.ts";
import source wasm from "./add.wasm";
import type { Missing } from "./missing-type.ts";
export type { Missing as Again } from "./missing-type-export.ts";
export type * from "./missing-type-all.ts";
export { reexported } from "./reexport.ts";
export * from "./star.ts";
import("./dynamic.ts");
import(import.meta.resolve("./meta.ts"));
import(import.meta.resolve("./meta-parent.ts", import.meta.url));
import(import.meta.resolve("./joined" + "-meta.ts"));
import.defer("./defer-dyn.ts");
import.source("./source.wasm");
import(`./template.ts`);
import(`./esca\u0070e.ts`);
import("./plus" + ".ts");
import(new URL("./url.ts", import.meta.url).href);
import(new URL("./url-string.ts", import.meta.url).toString());
import(new URL("./url-bare.ts", import.meta.url));
import((new URL("./url-paren.ts", import.meta.url)).href);
import(new URL("./url-as.ts", import.meta.url).href as string);
import("./alias.js");
import("./data.json", { with: { type: "json" } });
import(("skip", "./sequence.ts"));
import("./asserted.ts" as string);
import(("./satisfies.ts" satisfies string));
import(("./nonnull.ts"!));
import((<string>"./angle.ts"));
import((import.meta.resolve(("./paren.ts"))));
import("./js-importer.js");
import(import("./nested-outer.ts"));
const bound = "./bound.ts";
import(bound);
import(`./dir/${bound}.ts`);
import(`./${"folded"}.ts`);
import(flag ? "./branch-a.ts" : "./branch-b.ts");
import(import.meta.resolve("./other-parent.ts", "./elsewhere.ts"));
import(import.meta.resolve("./three.ts", import.meta.url, "extra"));
import(import.meta.resolve?.("./optional.ts"));
import.meta.resolve("./alone.ts");
new URL("./alone-url.ts", import.meta.url);
new Worker("./worker.ts");
new Worker(new URL("./worker-url.ts", import.meta.url));
import(new URL("./to-string-args.ts", import.meta.url).toString("utf-8"));
import(new URL("./one-arg.ts"));
import("./missing-dynamic.ts");
import("node:fs");
import("https://example.invalid/mod.js");
import("not-a-real-package");
void value;
void deferred;
void wasm;
"#;

  #[test]
  fn module_graph_includes_concrete_dynamic_imports() {
    let dir = scratch();
    let root = &dir.0;
    for name in [
      "static-dep.ts",
      "deferred-dep.ts",
      "reexport.ts",
      "star.ts",
      "dynamic-dep.ts",
      "dynamic-nested.ts",
      "meta-parent.ts",
      "meta-static.ts",
      "joined-meta.ts",
      "defer-dyn.ts",
      "template.ts",
      "escape.ts",
      "plus.ts",
      "url.ts",
      "url-string.ts",
      "url-bare.ts",
      "url-paren.ts",
      "url-as.ts",
      "alias.ts",
      "sequence.ts",
      "asserted.ts",
      "satisfies.ts",
      "nonnull.ts",
      "angle.ts",
      "paren.ts",
      "from-js.ts",
      "nested-outer.ts",
      "file-url.ts",
      "alone.ts",
      "alone-url.ts",
      "bound.ts",
      "branch-a.ts",
      "branch-b.ts",
      "folded.ts",
      "other-parent.ts",
      "three.ts",
      "optional.ts",
      "worker.ts",
      "worker-url.ts",
      "to-string-args.ts",
      "one-arg.ts",
      "dir/not-scanned.ts",
    ] {
      write(root, name, "export {};\n");
    }
    write(
      root,
      "static.ts",
      "import \"./static-dep.ts\";\nexport const value = 1;\n",
    );
    write(
      root,
      "deferred.ts",
      "import \"./deferred-dep.ts\";\nexport const deferred = 1;\n",
    );
    write(
      root,
      "dynamic.ts",
      "import \"./dynamic-dep.ts\";\nimport(\"./dynamic-nested.ts\");\n",
    );
    write(
      root,
      "meta.ts",
      "import \"./meta-static.ts\";\nexport const meta = 1;\n",
    );
    write(root, "js-importer.js", "import.defer(\"./from-js.ts\");\n");
    write(root, "data.json", "{ \"ok\": true }\n");
    fs::write(root.join("add.wasm"), [0xff, 0xfe]).unwrap();
    fs::write(root.join("source.wasm"), [0xff, 0xfe]).unwrap();
    let present = url::Url::from_file_path(root.join("file-url.ts")).unwrap();
    let missing =
      url::Url::from_file_path(root.join("absent-file-url.ts")).unwrap();
    write(
      root,
      "entry.ts",
      &format!("{ENTRY}\nimport(\"{present}\");\nimport(\"{missing}\");\n"),
    );

    let files = module_graph(&root.join("entry.ts")).unwrap();
    let mut names: Vec<_> = files
      .iter()
      .map(|path| path.file_name().unwrap().to_str().unwrap().to_string())
      .collect();
    names.sort();
    assert_eq!(
      names,
      [
        "add.wasm",
        "alias.ts",
        "angle.ts",
        "asserted.ts",
        "data.json",
        "defer-dyn.ts",
        "deferred-dep.ts",
        "deferred.ts",
        "dynamic-dep.ts",
        "dynamic-nested.ts",
        "dynamic.ts",
        "entry.ts",
        "escape.ts",
        "file-url.ts",
        "from-js.ts",
        "joined-meta.ts",
        "js-importer.js",
        "meta-parent.ts",
        "meta-static.ts",
        "meta.ts",
        "nested-outer.ts",
        "nonnull.ts",
        "paren.ts",
        "plus.ts",
        "reexport.ts",
        "satisfies.ts",
        "sequence.ts",
        "source.wasm",
        "star.ts",
        "static-dep.ts",
        "static.ts",
        "template.ts",
        "url-as.ts",
        "url-bare.ts",
        "url-paren.ts",
        "url-string.ts",
        "url.ts",
      ]
    );
  }

  #[test]
  fn module_graph_follows_javascript_source_imports() {
    let dir = scratch();
    write(
      &dir.0,
      "entry.ts",
      "import source worker from \"./worker.ts\";\nvoid worker;\n",
    );
    write(&dir.0, "worker.ts", "import \"./dep.ts\";\nexport {};\n");
    write(&dir.0, "dep.ts", "export const n = 1;\n");
    write(&dir.0, "unused.ts", "export {};\n");
    let files = module_graph(&dir.0.join("entry.ts")).unwrap();
    let mut names: Vec<_> = files
      .iter()
      .map(|path| path.file_name().unwrap().to_str().unwrap().to_string())
      .collect();
    names.sort();
    assert_eq!(names, ["dep.ts", "entry.ts", "worker.ts"]);
  }

  #[test]
  fn module_graph_rejects_a_missing_static_import() {
    let dir = scratch();
    write(&dir.0, "entry.ts", "import \"./missing.ts\";\n");
    let error = module_graph(&dir.0.join("entry.ts")).unwrap_err();
    assert!(error.to_string().contains("missing.ts"), "{error}");
  }

  #[test]
  fn module_graph_rejects_a_static_import_from_a_dynamic_module() {
    let dir = scratch();
    write(&dir.0, "entry.ts", "import(\"./present.ts\");\n");
    write(&dir.0, "present.ts", "import \"./missing.ts\";\n");
    let error = module_graph(&dir.0.join("entry.ts")).unwrap_err();
    assert!(error.to_string().contains("missing.ts"), "{error}");
  }

  #[test]
  fn module_graph_copies_text_and_bytes_without_parsing() {
    let dir = scratch();
    let root = &dir.0;
    write(
      root,
      "entry.ts",
      r#"
import "./text.js" with { type: "text" };
import "./bytes.ts" with { type: "bytes" };
import("./dyn.js", { with: { type: "text" } });
import.defer("./defer.js", { with: { type: "bytes" } });
import("./extra.js", { with: { extra: "x", type: "bytes" } });
import("./keyed.js", { "with": { "type": "text" } });
export { value } from "./export.js" with { type: "text" };
export * from "./star.js" with { type: "bytes" };
import("./missing-dynamic.js", { with: { type: "text" } });
"#,
    );
    for name in [
      "text.js",
      "bytes.ts",
      "dyn.js",
      "defer.js",
      "extra.js",
      "keyed.js",
      "export.js",
      "star.js",
    ] {
      write(root, name, "import \"./missing-inside.ts\";\n");
    }
    let files = module_graph(&root.join("entry.ts")).unwrap();
    let mut names: Vec<_> = files
      .iter()
      .map(|path| path.file_name().unwrap().to_str().unwrap().to_string())
      .collect();
    names.sort();
    assert_eq!(
      names,
      [
        "bytes.ts",
        "defer.js",
        "dyn.js",
        "entry.ts",
        "export.js",
        "extra.js",
        "keyed.js",
        "star.js",
        "text.js",
      ]
    );
  }

  #[test]
  fn module_graph_parses_a_text_file_imported_as_javascript() {
    let dir = scratch();
    write(
      &dir.0,
      "entry.ts",
      "import \"./mod.js\" with { type: \"text\" };\nimport \"./mod.js\";\n",
    );
    write(&dir.0, "mod.js", "import \"./missing.ts\";\n");
    let error = module_graph(&dir.0.join("entry.ts")).unwrap_err();
    assert!(error.to_string().contains("missing.ts"), "{error}");
  }

  #[test]
  fn module_graph_parses_javascript_with_other_attributes() {
    let dir = scratch();
    write(
      &dir.0,
      "entry.ts",
      "import \"./mod.js\" with { type: \"css\" };\n",
    );
    write(&dir.0, "mod.js", "import \"./missing.ts\";\n");
    let error = module_graph(&dir.0.join("entry.ts")).unwrap_err();
    assert!(error.to_string().contains("missing.ts"), "{error}");

    let dir = scratch();
    write(
      &dir.0,
      "entry.ts",
      "import(\"./mod.js\", { with: { type: \"json\" } });\n",
    );
    write(&dir.0, "mod.js", "import \"./missing.ts\";\n");
    let error = module_graph(&dir.0.join("entry.ts")).unwrap_err();
    assert!(error.to_string().contains("missing.ts"), "{error}");
  }

  #[test]
  fn module_graph_rejects_a_missing_static_text_import() {
    let dir = scratch();
    write(
      &dir.0,
      "entry.ts",
      "import \"./missing.ts\" with { type: \"text\" };\n",
    );
    let error = module_graph(&dir.0.join("entry.ts")).unwrap_err();
    assert!(error.to_string().contains("missing.ts"), "{error}");
  }
}
