//! Realm globals that are not tied to a later feature crate.

use v8::ValueDeserializerHelper;
use v8::ValueSerializerHelper;
use yuzuko_macros::op;

use crate::clone;
use crate::host;
use crate::js::{self, rust_string};

/// Deletes the Annex B accessor `Object.prototype.__proto__`.
///
/// `{ __proto__: null }` and `Object.setPrototypeOf` still set [[Prototype]].
/// Assigning the property writes an own data property.
pub fn delete_object_prototype_proto(scope: &mut v8::PinScope) {
  let object = v8::Object::new(scope);
  let Some(prototype) = object.get_prototype(scope) else {
    return;
  };
  let Ok(prototype) = v8::Local::<v8::Object>::try_from(prototype) else {
    return;
  };
  let Some(key) = v8::String::new(scope, "__proto__") else {
    return;
  };
  let _ = prototype.delete(scope, key.into());
}

/// Removes V8's non-standard `Intl.v8BreakIterator`.
///
/// ECMA-402 exposes breaks through `Intl.Segmenter`. Node.js and Deno leave
/// this constructor off `Intl`.
pub fn delete_intl_v8_break_iterator(scope: &mut v8::PinScope) {
  let global = js::global(scope);
  let Some(intl_key) = v8::String::new(scope, "Intl") else {
    return;
  };
  let Some(intl) = global.get(scope, intl_key.into()) else {
    return;
  };
  let Ok(intl) = v8::Local::<v8::Object>::try_from(intl) else {
    return;
  };
  let Some(key) = v8::String::new(scope, "v8BreakIterator") else {
    return;
  };
  let _ = intl.delete(scope, key.into());
}

pub fn install(scope: &mut v8::PinScope) {
  delete_object_prototype_proto(scope);
  delete_intl_v8_break_iterator(scope);
  let global = js::global(scope);
  // `self` names this realm. Assignment does not replace it.
  let self_key = v8::String::new(scope, "self").unwrap();
  let _ = global.define_own_property(
    scope,
    self_key.into(),
    global.into(),
    v8::PropertyAttribute::READ_ONLY,
  );
  // No document, COOP, or COEP, so the agent is not cross-origin isolated.
  js::set_value(
    scope,
    global,
    "crossOriginIsolated",
    v8::Boolean::new(scope, false).into(),
  );
  js::set_function(scope, global, "queueMicrotask", queue_microtask);
  js::set_function(scope, global, "structuredClone", structured_clone);
  js::set_function(scope, global, "reportError", report_error);
  install_performance(scope, global);
  install_navigator(scope, global);
}

fn queue_microtask(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut _ret: v8::ReturnValue,
) {
  let Ok(callback) = v8::Local::<v8::Function>::try_from(args.get(0)) else {
    js::throw_type(scope, "queueMicrotask requires a function");
    return;
  };
  let data: v8::Local<v8::Value> = callback.into();
  let Some(function) = v8::Function::builder(run_queued_microtask)
    .data(data)
    .build(scope)
  else {
    scope.enqueue_microtask(callback);
    return;
  };
  scope.enqueue_microtask(function);
}

/// Runs a `queueMicrotask` callback. A throw is reported on this realm.
fn run_queued_microtask(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut _ret: v8::ReturnValue,
) {
  let Ok(callback) = v8::Local::<v8::Function>::try_from(args.data()) else {
    return;
  };
  v8::tc_scope!(let tc, scope);
  let receiver: v8::Local<v8::Value> = v8::undefined(tc).into();
  if callback.call(tc, receiver, &[]).is_some() {
    return;
  }
  let Some(thrown) = tc.exception() else {
    return;
  };
  let thrown = v8::Global::new(tc, thrown);
  tc.reset();
  let thrown = v8::Local::new(tc, thrown);
  let message = crate::loader::modules::exception_stack(tc, thrown)
    .unwrap_or_else(|| "queueMicrotask callback threw".to_string());
  if host::note_test_error(message) {
    return;
  }
  report_uncaught(tc, thrown);
}

/// Calls `globalThis.reportError(error)` when it is a function.
pub(crate) fn report_uncaught(
  scope: &mut v8::PinScope,
  error: v8::Local<v8::Value>,
) {
  let global = js::global(scope);
  let Some(key) = v8::String::new(scope, "reportError") else {
    return;
  };
  let Some(report) = global
    .get(scope, key.into())
    .and_then(|value| v8::Local::<v8::Function>::try_from(value).ok())
  else {
    return;
  };
  let _ = report.call(scope, global.into(), &[error]);
}

fn report_error(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut _ret: v8::ReturnValue,
) {
  let text = if args.length() == 0 {
    "undefined".to_string()
  } else {
    rust_string(scope, args.get(0))
  };
  eprintln!("reported error: {text}");
}

fn install_performance(
  scope: &mut v8::PinScope,
  global: v8::Local<v8::Object>,
) {
  let performance = v8::Object::new(scope);
  js::set_function(scope, performance, "now", performance_now);
  let millis = host::with(|state| state.time_origin_ms);
  js::set_value(
    scope,
    performance,
    "timeOrigin",
    v8::Number::new(scope, millis).into(),
  );
  js::set_value(scope, global, "performance", performance.into());
}

pub fn performance_now_ms() -> f64 {
  host::with(|state| state.time_origin.elapsed().as_secs_f64() * 1000.0)
}

pub fn time_origin_ms() -> f64 {
  host::with(|state| state.time_origin_ms)
}

#[op]
fn performance_now() -> f64 {
  performance_now_ms()
}

fn install_navigator(scope: &mut v8::PinScope, global: v8::Local<v8::Object>) {
  let navigator = v8::Object::new(scope);
  let agent =
    v8::String::new(scope, concat!("Yuzuko/", env!("CARGO_PKG_VERSION")))
      .unwrap();
  define_readonly_data(scope, navigator, "userAgent", agent.into());
  let platform = v8::String::new(scope, navigator_platform()).unwrap();
  define_readonly_data(scope, navigator, "platform", platform.into());
  let cores = v8::Integer::new_from_unsigned(scope, hardware_concurrency());
  define_readonly_data(scope, navigator, "hardwareConcurrency", cores.into());
  // A dedicated worker installs these on `WorkerNavigator.prototype`. An own
  // accessor would shadow that and fail the IDL harness.
  if !host::is_dedicated_worker() {
    define_navigator_getter(scope, navigator, "language", navigator_language);
    define_navigator_getter(scope, navigator, "languages", navigator_languages);
    define_navigator_getter(
      scope,
      navigator,
      "appCodeName",
      navigator_app_code_name,
    );
    define_navigator_getter(scope, navigator, "appName", navigator_app_name);
    define_navigator_getter(
      scope,
      navigator,
      "appVersion",
      navigator_app_version,
    );
    define_navigator_getter(scope, navigator, "product", navigator_product);
    define_navigator_getter(scope, navigator, "onLine", navigator_on_line);
  }
  js::set_value(scope, global, "navigator", navigator.into());
}

/// Own data. Strict assignment throws. Enumerable so `for-in` sees it.
fn define_readonly_data(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
  value: v8::Local<v8::Value>,
) {
  let key = v8::String::new(scope, name).unwrap();
  let mut descriptor =
    v8::PropertyDescriptor::new_from_value_writable(value, false);
  descriptor.set_enumerable(true);
  descriptor.set_configurable(true);
  let _ = object.define_property(scope, key.into(), &descriptor);
}

fn define_navigator_getter(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
  callback: impl v8::MapFnTo<v8::FunctionCallback>,
) {
  let getter = v8::Function::builder(callback)
    .length(0)
    .constructor_behavior(v8::ConstructorBehavior::Throw)
    .build(scope)
    .unwrap();
  let label = format!("get {name}");
  let getter_name = v8::String::new(scope, &label).unwrap();
  getter.set_name(getter_name);
  let key = v8::String::new(scope, name).unwrap();
  let get: v8::Local<v8::Value> = getter.into();
  let set: v8::Local<v8::Value> = v8::undefined(scope).into();
  let mut descriptor = v8::PropertyDescriptor::new_from_get_set(get, set);
  descriptor.set_enumerable(true);
  descriptor.set_configurable(true);
  let _ = object.define_property(scope, key.into(), &descriptor);
}

fn navigator_language(
  scope: &mut v8::PinScope,
  _args: v8::FunctionCallbackArguments,
  mut ret: v8::ReturnValue,
) {
  let text = v8::String::new(scope, &crate::primary_language()).unwrap();
  ret.set(text.into());
}

fn navigator_languages(
  scope: &mut v8::PinScope,
  _args: v8::FunctionCallbackArguments,
  mut ret: v8::ReturnValue,
) {
  ret.set(crate::languages_array(scope).into());
}

fn navigator_app_code_name(
  scope: &mut v8::PinScope,
  _args: v8::FunctionCallbackArguments,
  mut ret: v8::ReturnValue,
) {
  ret.set(v8::String::new(scope, "Mozilla").unwrap().into());
}

fn navigator_app_name(
  scope: &mut v8::PinScope,
  _args: v8::FunctionCallbackArguments,
  mut ret: v8::ReturnValue,
) {
  ret.set(v8::String::new(scope, "Netscape").unwrap().into());
}

fn navigator_app_version(
  scope: &mut v8::PinScope,
  _args: v8::FunctionCallbackArguments,
  mut ret: v8::ReturnValue,
) {
  ret.set(v8::String::new(scope, "5.0").unwrap().into());
}

fn navigator_product(
  scope: &mut v8::PinScope,
  _args: v8::FunctionCallbackArguments,
  mut ret: v8::ReturnValue,
) {
  ret.set(v8::String::new(scope, "Gecko").unwrap().into());
}

fn navigator_on_line(
  scope: &mut v8::PinScope,
  _args: v8::FunctionCallbackArguments,
  mut ret: v8::ReturnValue,
) {
  ret.set(v8::Boolean::new(scope, crate::on_line()).into());
}

/// Logical processors available to this process.
///
/// HTML requires a positive integer and uses 1 when the count is unknown.
fn hardware_concurrency() -> u32 {
  let Ok(count) = std::thread::available_parallelism() else {
    return 1;
  };
  u32::try_from(count.get()).unwrap_or(u32::MAX).max(1)
}

/// Historical `navigator.platform` tokens. Endings tests treat a `Win` prefix
/// as CRLF and anything else as LF.
fn navigator_platform() -> &'static str {
  match std::env::consts::OS {
    "windows" => "Win32",
    "macos" => "MacIntel",
    "linux" => "Linux",
    _ => "Unknown",
  }
}

/// Replaces V8's plain `SharedArrayBuffer` clone error with `DataCloneError`.
///
/// The check runs after `write_value` returns. Constructing `DOMException`
/// inside the serializer delegate can run while JavaScript is disallowed.
macro_rules! promote_shared_array_buffer_error {
  ($tc:ident) => {
    if let Some(thrown) = $tc.exception()
      && clone::unshared_array_buffer_error($tc, thrown)
    {
      $tc.reset();
      clone::throw_data_clone(
        $tc,
        "The SharedArrayBuffer could not be cloned.",
      );
    }
  };
}

/// Which half of a cross-context structured clone failed.
pub enum ClonePhase {
  Serialize(Option<v8::Global<v8::Value>>),
  Deserialize(Option<v8::Global<v8::Value>>),
}

/// Serializes `value` in the current context and deserializes it in
/// `destination`. A serialize failure and a deserialize failure stay distinct
/// so the caller can reject the constructor or queue `processorerror`.
pub fn clone_into_context<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  value: v8::Local<v8::Value>,
  destination: v8::Local<v8::Context>,
) -> Result<v8::Local<'s, v8::Value>, ClonePhase> {
  let session = clone::CloneSession::enter();
  let bytes = {
    v8::tc_scope!(let tc, scope);
    let serializer = v8::ValueSerializer::new(tc, Box::new(CloneHost));
    serializer.write_header();
    let context = tc.get_current_context();
    if serializer.write_value(context, value).is_none() {
      promote_shared_array_buffer_error!(tc);
      Err(ClonePhase::Serialize(
        tc.exception().map(|thrown| v8::Global::new(tc, thrown)),
      ))
    } else {
      Ok(serializer.release())
    }
  };
  let bytes = match bytes {
    Ok(bytes) => bytes,
    Err(phase) => {
      let _ = session.exit(scope, false);
      return Err(phase);
    }
  };
  let cloned = {
    let scope = &mut v8::ContextScope::new(scope, destination);
    v8::tc_scope!(let tc, scope);
    let deserializer =
      v8::ValueDeserializer::new(tc, Box::new(CloneHost), &bytes);
    let context = tc.get_current_context();
    if deserializer.read_header(context).is_none() {
      Err(ClonePhase::Deserialize(
        tc.exception().map(|thrown| v8::Global::new(tc, thrown)),
      ))
    } else {
      match clone::read_host_value(tc, |_| deserializer.read_value(context)) {
        clone::HostRead::Done(cloned) => Ok(v8::Global::new(tc, cloned)),
        clone::HostRead::Rejected(message) => {
          tc.reset();
          clone::throw_data_clone(tc, &message);
          Err(ClonePhase::Deserialize(
            tc.exception().map(|thrown| v8::Global::new(tc, thrown)),
          ))
        }
        clone::HostRead::Failed => Err(ClonePhase::Deserialize(
          tc.exception().map(|thrown| v8::Global::new(tc, thrown)),
        )),
      }
    }
  };
  let success = cloned.is_ok();
  let exit = session.exit(scope, success);
  match (cloned, exit) {
    (Err(phase), _) => Err(phase),
    (Ok(_), Err(error)) => Err(ClonePhase::Deserialize(error)),
    (Ok(value), Ok(())) => Ok(v8::Local::new(scope, value)),
  }
}

pub fn clone_value<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  value: v8::Local<v8::Value>,
) -> Result<v8::Local<'s, v8::Value>, Option<v8::Global<v8::Value>>> {
  clone_value_with_buffers(scope, value, &[])
}

/// Structured-clones `value` in this realm.
///
/// `buffers` move. The serializer records each id, and the clone receives a
/// new `ArrayBuffer` over that backing store, so `resizable` and
/// `maxByteLength` stay. The caller detaches the sources after `Ok`. A
/// failure leaves them attached.
pub fn clone_value_with_buffers<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  value: v8::Local<v8::Value>,
  buffers: &[v8::Global<v8::ArrayBuffer>],
) -> Result<v8::Local<'s, v8::Value>, Option<v8::Global<v8::Value>>> {
  // Finish outside this block. `ReadHostObject` rejects JavaScript, and the
  // deserializer has to be gone before a finisher runs.
  let session = clone::CloneSession::enter();
  let cloned = {
    v8::tc_scope!(let tc, scope);
    let serializer = v8::ValueSerializer::new(tc, Box::new(CloneHost));
    serializer.write_header();
    match moved_array_buffers(tc, &serializer, buffers) {
      Err(()) => Err(tc.exception().map(|thrown| v8::Global::new(tc, thrown))),
      Ok(stores) => {
        let context = tc.get_current_context();
        if serializer.write_value(context, value).is_none() {
          promote_shared_array_buffer_error!(tc);
          Err(tc.exception().map(|thrown| v8::Global::new(tc, thrown)))
        } else {
          let bytes = serializer.release();
          let deserializer =
            v8::ValueDeserializer::new(tc, Box::new(CloneHost), &bytes);
          install_moved_array_buffers(tc, &deserializer, &stores);
          let context = tc.get_current_context();
          if deserializer.read_header(context).is_none() {
            Err(tc.exception().map(|thrown| v8::Global::new(tc, thrown)))
          } else {
            match clone::read_host_value(tc, |_| {
              deserializer.read_value(context)
            }) {
              clone::HostRead::Done(cloned) => Ok(v8::Global::new(tc, cloned)),
              clone::HostRead::Rejected(message) => {
                tc.reset();
                clone::throw_data_clone(tc, &message);
                Err(tc.exception().map(|thrown| v8::Global::new(tc, thrown)))
              }
              clone::HostRead::Failed => {
                Err(tc.exception().map(|thrown| v8::Global::new(tc, thrown)))
              }
            }
          }
        }
      }
    }
  };
  let success = cloned.is_ok();
  session.exit(scope, success)?;
  match cloned {
    Ok(value) => Ok(v8::Local::new(scope, value)),
    Err(error) => Err(error),
  }
}

/// Registers `buffers` for transfer and keeps their backing stores.
///
/// The store is taken before `detach`. A detached or non-detachable buffer
/// throws `DataCloneError` and registers nothing further.
fn moved_array_buffers(
  scope: &mut v8::PinScope,
  serializer: &v8::ValueSerializer,
  buffers: &[v8::Global<v8::ArrayBuffer>],
) -> Result<Vec<v8::SharedRef<v8::BackingStore>>, ()> {
  let mut stores = Vec::with_capacity(buffers.len());
  for (index, buffer) in buffers.iter().enumerate() {
    let buffer = v8::Local::new(scope, buffer);
    if buffer.was_detached() || !buffer.is_detachable() {
      clone::throw_data_clone(scope, "The ArrayBuffer cannot be transferred");
      return Err(());
    }
    let Ok(id) = u32::try_from(index) else {
      clone::throw_data_clone(scope, "The ArrayBuffer cannot be transferred");
      return Err(());
    };
    stores.push(buffer.get_backing_store());
    serializer.transfer_array_buffer(id, buffer);
  }
  Ok(stores)
}

fn install_moved_array_buffers(
  scope: &mut v8::PinScope,
  deserializer: &v8::ValueDeserializer,
  stores: &[v8::SharedRef<v8::BackingStore>],
) {
  for (index, store) in stores.iter().enumerate() {
    let Ok(id) = u32::try_from(index) else {
      break;
    };
    let buffer = v8::ArrayBuffer::with_backing_store(scope, store);
    deserializer.transfer_array_buffer(id, buffer);
  }
}

fn structured_clone(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut ret: v8::ReturnValue,
) {
  if args.length() < 1 {
    js::throw_type(scope, "structuredClone requires a value");
    return;
  }
  let transfers = match transfer_list(scope, args.get(1)) {
    Ok(transfers) => transfers,
    Err(()) => return,
  };
  let cloned = {
    let _guard = clone::StructuredTransferGuard::install(transfers.objects);
    match clone_value_with_buffers(scope, args.get(0), &transfers.buffers) {
      Ok(cloned) => {
        clone::detach_structured_transfers(scope);
        cloned
      }
      Err(Some(thrown)) => {
        let thrown = v8::Local::new(scope, thrown);
        scope.throw_exception(thrown);
        return;
      }
      Err(None) => {
        clone::reject_clone(scope);
        return;
      }
    }
  };
  for buffer in &transfers.buffers {
    let buffer = v8::Local::new(scope, buffer);
    let _ = buffer.detach(None);
  }
  ret.set(cloned);
}

/// `structuredClone`'s `transfer` list. Array buffers move with the clone.
/// `ImageBitmap`, `AudioData`, and `VideoFrame` move their pixels or samples
/// and detach after that clone succeeds.
fn transfer_list(
  scope: &mut v8::PinScope,
  options: v8::Local<v8::Value>,
) -> Result<clone::TransferList, ()> {
  if options.is_null_or_undefined() {
    return Ok(clone::TransferList {
      objects: Vec::new(),
      buffers: Vec::new(),
    });
  }
  let Ok(object) = v8::Local::<v8::Object>::try_from(options) else {
    js::throw_type(scope, "The structuredClone options are not an object");
    return Err(());
  };
  let key = v8::String::new(scope, "transfer").unwrap();
  let Some(transfer) = object.get(scope, key.into()) else {
    return Err(());
  };
  if transfer.is_undefined() {
    return Ok(clone::TransferList {
      objects: Vec::new(),
      buffers: Vec::new(),
    });
  }
  let Ok(list) = v8::Local::<v8::Array>::try_from(transfer) else {
    js::throw_type(scope, "The structuredClone transfer list is not an array");
    return Err(());
  };
  match clone::collect_transfers(
    scope,
    list,
    clone::TransferKind::StructuredClone,
  ) {
    Ok(transfers) => Ok(transfers),
    Err(clone::TransferReject::Pending) => Err(()),
    Err(clone::TransferReject::DataClone(message)) => {
      clone::throw_data_clone(scope, message);
      Err(())
    }
    Err(clone::TransferReject::Type(message)) => {
      js::throw_type(scope, message);
      Err(())
    }
  }
}

struct CloneHost;

impl v8::ValueSerializerImpl for CloneHost {
  fn throw_data_clone_error<'s>(
    &self,
    scope: &mut v8::PinScope<'s, '_>,
    message: v8::Local<'s, v8::String>,
  ) {
    clone::throw_data_clone(scope, &message.to_rust_string_lossy(scope));
  }

  fn has_custom_host_object(&self, _isolate: &v8::Isolate) -> bool {
    true
  }

  fn is_host_object<'s>(
    &self,
    _scope: &mut v8::PinScope<'s, '_>,
    object: v8::Local<'s, v8::Object>,
  ) -> Option<bool> {
    Some(object.is_api_wrapper())
  }

  fn write_host_object<'s>(
    &self,
    scope: &mut v8::PinScope<'s, '_>,
    object: v8::Local<'s, v8::Object>,
    value_serializer: &dyn v8::ValueSerializerHelper,
  ) -> Option<bool> {
    clone::write_host_object(scope, object, value_serializer)
  }

  fn get_wasm_module_transfer_id(
    &self,
    _scope: &mut v8::PinScope<'_, '_>,
    module: v8::Local<v8::WasmModuleObject>,
  ) -> Option<u32> {
    let compiled = module.get_compiled_module();
    let bytes = compiled.get_wire_bytes_ref().to_vec();
    drop(compiled);
    let id = clone::note_wasm_module(bytes);
    if id == 0 { None } else { Some(id) }
  }

  fn get_shared_array_buffer_id<'s>(
    &self,
    _scope: &mut v8::PinScope<'s, '_>,
    shared_array_buffer: v8::Local<'s, v8::SharedArrayBuffer>,
  ) -> Option<u32> {
    clone::note_shared_array_buffer(&shared_array_buffer)
  }
}

impl v8::ValueDeserializerImpl for CloneHost {
  fn read_host_object<'s>(
    &self,
    scope: &mut v8::PinScope<'s, '_>,
    value_deserializer: &dyn v8::ValueDeserializerHelper,
  ) -> Option<v8::Local<'s, v8::Object>> {
    clone::read_host_object(scope, value_deserializer)
  }

  fn get_wasm_module_from_id<'s>(
    &self,
    scope: &mut v8::PinScope<'s, '_>,
    id: u32,
  ) -> Option<v8::Local<'s, v8::WasmModuleObject>> {
    let bytes = clone::wasm_module_bytes(id)?;
    v8::WasmModuleObject::compile(scope, &bytes)
  }

  fn get_shared_array_buffer_from_id<'s>(
    &self,
    scope: &mut v8::PinScope<'s, '_>,
    id: u32,
  ) -> Option<v8::Local<'s, v8::SharedArrayBuffer>> {
    clone::shared_array_buffer_from_id(scope, id)
  }
}

#[cfg(test)]
mod tests {
  use super::hardware_concurrency;

  #[test]
  fn hardware_concurrency_is_positive() {
    assert!(hardware_concurrency() >= 1);
  }
}
