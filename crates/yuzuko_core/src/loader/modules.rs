//! Module graph: oxc resolution, TypeScript erasure, and V8 module records.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use oxc_resolver::{
  EnforceExtension, ModuleType, ResolveOptions, Resolver, TsconfigDiscovery,
};
use url::Url;

use crate::RuntimeError;
use crate::host::{self, ModuleEntry};
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
      if let Some(exception) = tc.exception() {
        return Ok(ModuleRun::Thrown(v8::Global::new(tc, exception)));
      }
      let text = exception_text!(tc, "failed to instantiate module");
      return Err(RuntimeError::message(text));
    }
    match module.evaluate(tc) {
      Some(value) => ModuleRun::Value(v8::Global::new(tc, value)),
      None => {
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
/// Point each evaluated module namespace at that instance.
fn record_wasm_namespaces(scope: &mut v8::PinScope) {
  let stored = host::with(|state| state.wasm_instances.clone());
  let Some(stored) = stored else {
    return;
  };
  let modules = host::with(|state| {
    state
      .modules
      .values()
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
    if !matches!(
      module.get_status(),
      v8::ModuleStatus::Instantiated
        | v8::ModuleStatus::Evaluating
        | v8::ModuleStatus::Evaluated
        | v8::ModuleStatus::Errored
    ) {
      continue;
    }
    let namespace = module.get_module_namespace();
    let namespace = v8::Global::new(scope, namespace);
    let namespace = v8::Local::new(scope, namespace);
    if map.set(scope, namespace, instance).is_none() {
      return;
    }
  }
}

fn compile_path<'a>(
  scope: &mut v8::PinScope<'a, '_>,
  path: &Path,
  url: &str,
  attributes: Option<&str>,
) -> Result<v8::Local<'a, v8::Module>, RuntimeError> {
  if let Some(existing) = existing_module(scope, url) {
    return Ok(existing);
  }
  let extension = path.extension().and_then(|ext| ext.to_str()).unwrap_or("");
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
  remember(scope, url, path, module);
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
  let hash = module.get_identity_hash().get();
  host::with_mut(|state| {
    state.json_sources.insert(hash, text.to_string());
  });
  remember(scope, url, path, module);
  Ok(module)
}

fn evaluate_json<'s>(
  context: v8::Local<'s, v8::Context>,
  module: v8::Local<'s, v8::Module>,
) -> Option<v8::Local<'s, v8::Value>> {
  v8::callback_scope!(unsafe scope, context);
  let scope = &mut v8::ContextScope::new(scope, context);
  let hash = module.get_identity_hash().get();
  let text = host::with(|state| {
    state
      .json_sources
      .get(&hash)
      .cloned()
      .unwrap_or_else(|| "null".to_string())
  });
  let source = v8::String::new(scope, &text)?;
  let value = v8::json::parse(scope, source)?;
  let name = v8::String::new(scope, "default")?;
  module.set_synthetic_module_export(scope, name, value)?;
  Some(v8::undefined(scope).into())
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
  _import_attributes: v8::Local<'s, v8::FixedArray>,
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
  match load_wasm_source(scope, &referrer_path, &spec) {
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

fn load_wasm_source<'a>(
  scope: &mut v8::PinScope<'a, '_>,
  referrer: &Path,
  specifier: &str,
) -> Result<v8::Local<'a, v8::Object>, RuntimeError> {
  let (path, url) = locate(referrer, specifier)?;
  let extension = path.extension().and_then(|ext| ext.to_str()).unwrap_or("");
  if extension != "wasm" {
    return Err(RuntimeError::message(
      "import source is only supported for WebAssembly modules".to_string(),
    ));
  }
  let bytes = fs::read(&path).map_err(|err| {
    RuntimeError::message(format!("{}: {err}", path.display()))
  })?;
  crate::loader::wasm_module::source_object(scope, &url, &bytes)
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
) -> Option<v8::Local<'a, v8::Module>> {
  let global = host::with(|state| {
    state
      .modules
      .values()
      .find(|entry| entry.url == url)
      .map(|entry| entry.module.clone())
  })?;
  Some(v8::Local::new(scope, global))
}

fn remember(
  scope: &mut v8::PinScope<'_, '_>,
  url: &str,
  path: &Path,
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
) -> Option<v8::Local<'s, v8::Promise>> {
  let resolver = v8::PromiseResolver::new(scope)?;
  let promise = resolver.get_promise(scope);
  let spec = specifier.to_rust_string_lossy(scope);
  let referrer = resource_name.to_rust_string_lossy(scope);
  let referrer_path =
    url_to_path(&referrer).or_else(|_| absolute(Path::new(&referrer)));
  let loaded =
    referrer_path.and_then(|path| load_wasm_source(scope, &path, &spec));
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

pub fn absolute(path: &Path) -> Result<PathBuf, RuntimeError> {
  if path.is_absolute() {
    Ok(path.to_path_buf())
  } else {
    let cwd = std::env::current_dir()
      .map_err(|error| RuntimeError::message(error.to_string()))?;
    Ok(cwd.join(path))
  }
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

/// Files in the static module graph of `entry`, including `entry`.
///
/// Dynamic `import()` and `Worker` specifiers are not followed. Type-only
/// imports are not followed.
pub fn module_graph(entry: &Path) -> Result<Vec<PathBuf>, RuntimeError> {
  let entry = absolute(entry)?;
  let resolver = resolver();
  let mut pending = vec![entry];
  let mut seen = std::collections::BTreeSet::new();
  let mut files = Vec::new();
  while let Some(path) = pending.pop() {
    if !seen.insert(path.clone()) {
      continue;
    }
    files.push(path.clone());
    let extension = path.extension().and_then(|ext| ext.to_str()).unwrap_or("");
    if !matches!(extension, "js" | "mjs" | "cjs" | "ts" | "mts") {
      continue;
    }
    let source = fs::read_to_string(&path).map_err(|error| {
      RuntimeError::message(format!("{}: {error}", path.display()))
    })?;
    for specifier in static_specifiers(&path, &source)? {
      pending.push(resolve_with(&resolver, &path, &specifier)?);
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

fn static_specifiers(
  path: &Path,
  source: &str,
) -> Result<Vec<String>, RuntimeError> {
  let allocator = oxc_allocator::Allocator::default();
  let source_type = oxc_span::SourceType::from_path(path).map_err(|error| {
    RuntimeError::message(format!("{}: {error}", path.display()))
  })?;
  let parsed = oxc_parser::Parser::new(&allocator, source, source_type).parse();
  if let Some(error) = parsed.diagnostics.first() {
    return Err(RuntimeError::message(format!(
      "{}: {error}",
      path.display()
    )));
  }
  let mut specs = Vec::new();
  for statement in &parsed.program.body {
    collect_specifiers(statement, &mut specs);
  }
  Ok(specs)
}

fn collect_specifiers(
  statement: &oxc_ast::ast::Statement<'_>,
  specs: &mut Vec<String>,
) {
  use oxc_ast::ast::{ImportOrExportKind, Statement};
  match statement {
    Statement::ImportDeclaration(declaration) => {
      if declaration.import_kind == ImportOrExportKind::Type {
        return;
      }
      specs.push(declaration.source.value.to_string());
    }
    Statement::ExportAllDeclaration(declaration) => {
      if declaration.export_kind == ImportOrExportKind::Type {
        return;
      }
      specs.push(declaration.source.value.to_string());
    }
    Statement::ExportFromDeclaration(declaration) => {
      if declaration.export_kind == ImportOrExportKind::Type {
        return;
      }
      specs.push(declaration.source.value.to_string());
    }
    _ => {}
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
