//! `navigator.gpu`, adapters, devices, and queues.
//!
//! Every JavaScript device clones the process wgpu device. `destroy` loses
//! that JavaScript device and leaves the process device in place.

use std::cell::RefCell;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use yuzuko_core::op::{Js, New, OpError, OpException};
use yuzuko_dom::{Event, EventTarget, define_is_trusted, dispatch};
use yuzuko_macros::op;

use crate::encode::{self, GpuCommandEncoder};
use crate::js;
use crate::label::ObjectLabel;
use crate::resource::{self, GpuBuffer, GpuTexture};
use crate::state::{self, Handle};

pub struct Gpu {
  language_features: RefCell<Option<v8::TracedReference<v8::Object>>>,
}

unsafe impl v8::cppgc::GarbageCollected for Gpu {
  fn trace(&self, visitor: &mut v8::cppgc::Visitor) {
    trace_slot(&self.language_features, visitor);
  }

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"GPU"
  }
}

impl Gpu {
  fn new() -> Self {
    Self {
      language_features: RefCell::new(None),
    }
  }
}

#[op(name = "GPU")]
impl Gpu {
  fn request_adapter(
    &self,
    scope: &mut v8::PinScope,
    options: Option<v8::Local<v8::Value>>,
  ) -> Js {
    if yuzuko_window::gpu_foreign() {
      return js::promise_rejected(
        scope,
        OpError::dom(
          "InvalidStateError",
          "The GPU device belongs to another thread",
        ),
      );
    }
    match adapter_choice(scope, options) {
      Ok(true) => {
        let adapter =
          unsafe { yuzuko_core::op::wrap_instance(scope, GpuAdapter::new()) };
        let value: v8::Local<v8::Value> = adapter.into();
        js::promise(scope, value)
      }
      Ok(false) => js::promise_null(scope),
      Err(error) => js::promise_rejected(scope, error),
    }
  }

  fn get_preferred_canvas_format(&self) -> Result<String, OpError> {
    preferred_format().map(str::to_string)
  }

  #[getter]
  fn wgsl_language_features(&self, scope: &mut v8::PinScope) -> Js {
    if let Some(cached) = recalled_js(scope, &self.language_features) {
      return cached;
    }
    let object = language_features(scope);
    remember_js(scope, &self.language_features, object)
  }
}

pub struct GpuAdapter {
  features: RefCell<Option<v8::TracedReference<v8::Object>>>,
  limits: RefCell<Option<v8::TracedReference<v8::Object>>>,
  info: RefCell<Option<v8::TracedReference<v8::Object>>>,
}

unsafe impl v8::cppgc::GarbageCollected for GpuAdapter {
  fn trace(&self, visitor: &mut v8::cppgc::Visitor) {
    trace_slot(&self.features, visitor);
    trace_slot(&self.limits, visitor);
    trace_slot(&self.info, visitor);
  }

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"GPUAdapter"
  }
}

impl GpuAdapter {
  fn new() -> Self {
    Self {
      features: RefCell::new(None),
      limits: RefCell::new(None),
      info: RefCell::new(None),
    }
  }
}

#[op(name = "GPUAdapter")]
impl GpuAdapter {
  #[getter]
  fn features(&self, scope: &mut v8::PinScope) -> Result<Js, OpError> {
    if let Some(cached) = recalled_js(scope, &self.features) {
      return Ok(cached);
    }
    // The process has one device. `requestDevice` can enable only the
    // features that device already has, so the adapter lists that set.
    let features = gpu_parts()?.1.features();
    let object = features_object(scope, features);
    Ok(remember_js(scope, &self.features, object))
  }

  #[getter]
  fn limits(&self, scope: &mut v8::PinScope) -> Result<Js, OpError> {
    if let Some(cached) = recalled_js(scope, &self.limits) {
      return Ok(cached);
    }
    let _ = gpu_parts()?;
    let object = limits_object(scope);
    Ok(remember_js(scope, &self.limits, object))
  }

  #[getter]
  fn is_fallback_adapter(&self) -> bool {
    false
  }

  #[getter]
  fn info(&self, scope: &mut v8::PinScope) -> Result<Js, OpError> {
    if let Some(cached) = recalled_js(scope, &self.info) {
      return Ok(cached);
    }
    let object = adapter_info_object(scope)?;
    Ok(remember_js(scope, &self.info, object))
  }

  fn request_device(
    &self,
    scope: &mut v8::PinScope,
    descriptor: Option<v8::Local<v8::Value>>,
  ) -> Js {
    match open_device(scope, descriptor) {
      Ok(device) => {
        let id = device.id;
        let object = unsafe { yuzuko_core::op::wrap_instance(scope, device) };
        state::register_wrapper(scope, id, object);
        let value: v8::Local<v8::Value> = object.into();
        js::promise(scope, value)
      }
      Err(error) => js::promise_rejected(scope, error),
    }
  }
}

#[repr(C)]
pub struct GpuDevice {
  base: EventTarget,
  id: u64,
  label: ObjectLabel,
}

unsafe impl v8::cppgc::GarbageCollected for GpuDevice {
  fn trace(&self, visitor: &mut v8::cppgc::Visitor) {
    v8::cppgc::GarbageCollected::trace(&self.base, visitor);
  }

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"GPUDevice"
  }
}

impl GpuDevice {
  fn require(&self) -> Result<Handle, OpError> {
    if yuzuko_window::gpu_foreign() {
      return Err(OpError::dom(
        "InvalidStateError",
        "The GPU device belongs to another thread",
      ));
    }
    state::handle(self.id)
  }
}

pub fn device_id(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Option<u64> {
  let object = v8::Local::<v8::Object>::try_from(value).ok()?;
  let host = unsafe { yuzuko_core::op::host_ref::<GpuDevice>(scope, object) }?;
  Some(host.get().id)
}

#[op(name = "GPUDevice", inherit = EventTarget)]
impl GpuDevice {
  #[getter]
  fn label(&self) -> String {
    self.label.get()
  }

  #[setter]
  fn label(&self, value: String) {
    self.label.set(value);
  }

  #[getter]
  fn lost(&self, scope: &mut v8::PinScope) -> Result<Js, OpError> {
    let promise = state::lost_promise(self.id).ok_or_else(|| {
      OpError::dom("InvalidStateError", "The GPU device is lost")
    })?;
    let local = v8::Local::new(scope, &promise);
    let value: v8::Local<v8::Value> = local.into();
    Ok(Js(v8::Global::new(scope, value)))
  }

  #[getter]
  fn features(&self, scope: &mut v8::PinScope) -> Result<Js, OpError> {
    device_member(scope, self.id, state::CachedObject::Features, |scope| {
      let features = self.require()?.device.features();
      Ok(features_object(scope, features))
    })
  }

  #[getter]
  fn limits(&self, scope: &mut v8::PinScope) -> Result<Js, OpError> {
    device_member(scope, self.id, state::CachedObject::Limits, |scope| {
      let _ = self.require()?;
      Ok(limits_object(scope))
    })
  }

  #[getter]
  fn adapter_info(&self, scope: &mut v8::PinScope) -> Result<Js, OpError> {
    device_member(scope, self.id, state::CachedObject::AdapterInfo, |scope| {
      adapter_info_object(scope)
    })
  }

  #[getter]
  fn queue(&self, scope: &mut v8::PinScope) -> Result<Js, OpError> {
    let object = state::queue_object(self.id)?;
    let local = v8::Local::new(scope, &object);
    let value: v8::Local<v8::Value> = local.into();
    Ok(Js(v8::Global::new(scope, value)))
  }

  fn destroy(&self, scope: &mut v8::PinScope) -> Result<(), OpError> {
    if yuzuko_window::gpu_foreign() {
      return Err(OpError::dom(
        "InvalidStateError",
        "The GPU device belongs to another thread",
      ));
    }
    resource::detach_device_mappings(scope, self.id);
    state::destroy(scope, self.id);
    Ok(())
  }

  fn create_buffer(
    &self,
    scope: &mut v8::PinScope,
    descriptor: v8::Local<v8::Value>,
  ) -> Result<New<GpuBuffer>, OpError> {
    resource::create_buffer(scope, self.id, &self.require()?, descriptor)
  }

  fn create_texture(
    &self,
    scope: &mut v8::PinScope,
    descriptor: v8::Local<v8::Value>,
  ) -> Result<New<GpuTexture>, OpError> {
    resource::create_texture(scope, self.id, &self.require()?, descriptor)
  }

  fn create_sampler(
    &self,
    scope: &mut v8::PinScope,
    descriptor: Option<v8::Local<v8::Value>>,
  ) -> Result<New<resource::GpuSampler>, OpError> {
    let value = descriptor.unwrap_or_else(|| v8::undefined(scope).into());
    resource::create_sampler(scope, &self.require()?, value)
  }

  fn create_shader_module(
    &self,
    scope: &mut v8::PinScope,
    descriptor: v8::Local<v8::Value>,
  ) -> Result<New<resource::GpuShaderModule>, OpError> {
    resource::create_shader(scope, self.id, &self.require()?, descriptor)
  }

  fn create_bind_group_layout(
    &self,
    scope: &mut v8::PinScope,
    descriptor: v8::Local<v8::Value>,
  ) -> Result<New<resource::GpuBindGroupLayout>, OpError> {
    resource::create_bind_group_layout(
      scope,
      self.id,
      &self.require()?,
      descriptor,
    )
  }

  fn create_pipeline_layout(
    &self,
    scope: &mut v8::PinScope,
    descriptor: v8::Local<v8::Value>,
  ) -> Result<New<resource::GpuPipelineLayout>, OpError> {
    resource::create_pipeline_layout(
      scope,
      self.id,
      &self.require()?,
      descriptor,
    )
  }

  fn create_bind_group(
    &self,
    scope: &mut v8::PinScope,
    descriptor: v8::Local<v8::Value>,
  ) -> Result<New<resource::GpuBindGroup>, OpError> {
    resource::create_bind_group(scope, &self.require()?, descriptor)
  }

  fn create_render_pipeline(
    &self,
    scope: &mut v8::PinScope,
    descriptor: v8::Local<v8::Value>,
  ) -> Result<New<resource::GpuRenderPipeline>, OpError> {
    resource::create_render_pipeline(
      scope,
      self.id,
      &self.require()?,
      descriptor,
    )
  }

  fn create_command_encoder(
    &self,
    scope: &mut v8::PinScope,
    descriptor: Option<v8::Local<v8::Value>>,
  ) -> Result<New<GpuCommandEncoder>, OpError> {
    let handle = self.require()?;
    let label = match descriptor {
      Some(value) if !value.is_null_or_undefined() => {
        match js::object(scope, value) {
          Some(object) => resource::optional_label(scope, object)?,
          None => None,
        }
      }
      _ => None,
    };
    Ok(New(GpuCommandEncoder::create(self.id, &handle, label)))
  }

  fn create_render_bundle_encoder(
    &self,
    scope: &mut v8::PinScope,
    descriptor: v8::Local<v8::Value>,
  ) -> Result<New<encode::GpuRenderBundleEncoder>, OpError> {
    encode::create_bundle_encoder(scope, self.id, &self.require()?, descriptor)
  }

  fn create_compute_pipeline(
    &self,
    scope: &mut v8::PinScope,
    descriptor: v8::Local<v8::Value>,
  ) -> Result<New<resource::GpuComputePipeline>, OpError> {
    resource::create_compute_pipeline(
      scope,
      self.id,
      &self.require()?,
      descriptor,
    )
  }

  fn create_compute_pipeline_async(
    &self,
    scope: &mut v8::PinScope,
    descriptor: v8::Local<v8::Value>,
  ) -> Js {
    async_pipeline(scope, self.id, descriptor, AsyncPipeline::Compute)
  }

  fn create_render_pipeline_async(
    &self,
    scope: &mut v8::PinScope,
    descriptor: v8::Local<v8::Value>,
  ) -> Js {
    async_pipeline(scope, self.id, descriptor, AsyncPipeline::Render)
  }

  fn create_query_set(
    &self,
    scope: &mut v8::PinScope,
    descriptor: v8::Local<v8::Value>,
  ) -> Result<New<resource::GpuQuerySet>, OpError> {
    resource::create_query_set(scope, self.id, &self.require()?, descriptor)
  }

  fn import_external_texture(
    &self,
    scope: &mut v8::PinScope,
    descriptor: v8::Local<v8::Value>,
  ) -> Result<New<resource::GpuExternalTexture>, OpError> {
    Ok(New(crate::canvas::import_external_texture(
      scope,
      &self.require()?,
      descriptor,
    )?))
  }

  fn push_error_scope(&self, filter: String) -> Result<(), OpError> {
    let kind = match filter.as_str() {
      "validation" => state::ScopeKind::Validation,
      "out-of-memory" => state::ScopeKind::OutOfMemory,
      "internal" => state::ScopeKind::Internal,
      _ => return Err(OpError::type_("The error filter is invalid")),
    };
    let handle = self.require()?;
    let guard = handle.device.push_error_scope(kind.wgpu());
    state::push_scope(self.id, guard, kind)
  }

  fn pop_error_scope(&self, scope: &mut v8::PinScope) -> Js {
    match pop_scope(scope, self.id) {
      Ok(value) => js::promise(scope, value),
      Err(error) => js::promise_rejected(scope, error),
    }
  }
}

pub struct GpuQueue {
  device_id: u64,
  label: ObjectLabel,
}

unsafe impl v8::cppgc::GarbageCollected for GpuQueue {
  fn trace(&self, _visitor: &mut v8::cppgc::Visitor) {}

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"GPUQueue"
  }
}

#[op(name = "GPUQueue")]
impl GpuQueue {
  #[getter]
  fn label(&self) -> String {
    self.label.get()
  }

  #[setter]
  fn label(&self, value: String) {
    self.label.set(value);
  }

  fn write_buffer(
    &self,
    scope: &mut v8::PinScope,
    buffer: v8::Local<v8::Value>,
    offset: js::GpuSize64,
    data: v8::Local<v8::Value>,
    data_offset: Option<js::GpuSize64>,
    size: Option<js::GpuSize64>,
  ) -> Result<(), OpError> {
    let buffer = resource::host_buffer(scope, buffer)
      .ok_or_else(|| OpError::type_("writeBuffer requires a GPUBuffer"))?;
    let bytes = write_contents(
      scope,
      data,
      data_offset.map(|value| value.0),
      size.map(|value| value.0),
    )?;
    if buffer.device_id() != self.device_id {
      state::report_validation(
        self.device_id,
        "The buffer belongs to another device",
      );
      return Ok(());
    }
    let gpu = buffer.alive()?;
    if buffer.is_destroyed() {
      state::report_validation(buffer.device_id(), "The buffer is destroyed");
      return Ok(());
    }
    let handle = state::handle(self.device_id)?;
    handle.queue.write_buffer(&gpu, offset.0, &bytes);
    Ok(())
  }

  fn submit(
    &self,
    scope: &mut v8::PinScope,
    buffers: v8::Local<v8::Value>,
  ) -> Result<(), OpError> {
    let commands = encode::command_buffers(scope, buffers)?;
    let handle = state::handle(self.device_id)?;
    let mut rejected = false;
    let mut buffers = Vec::new();
    for command in commands {
      if command.device_id != self.device_id || command.rejected {
        rejected = true;
      }
      if let Some(buffer) = command.buffer {
        buffers.push(buffer);
      }
    }
    if rejected {
      state::report_validation(
        self.device_id,
        "The command buffer submission is invalid",
      );
      return Ok(());
    }
    handle.queue.submit(buffers);
    Ok(())
  }

  fn copy_external_image_to_texture(
    &self,
    scope: &mut v8::PinScope,
    source: v8::Local<v8::Value>,
    destination: v8::Local<v8::Value>,
    size: v8::Local<v8::Value>,
  ) -> Result<(), OpError> {
    let handle = state::handle(self.device_id)?;
    crate::canvas::copy_external_image(
      scope,
      &handle,
      source,
      destination,
      size,
    )
  }

  fn write_texture(
    &self,
    _scope: &mut v8::PinScope,
    destination: v8::Local<v8::Value>,
    data: v8::Local<v8::Value>,
    data_layout: v8::Local<v8::Value>,
    size: v8::Local<v8::Value>,
  ) -> Result<(), OpError> {
    let _ = (destination, data, data_layout, size);
    let _ = state::handle(self.device_id)?;
    Err(js::unsupported("GPUQueue.writeTexture"))
  }

  fn on_submitted_work_done(&self, scope: &mut v8::PinScope) -> Js {
    // After the device is lost, queue operations that report back appear to
    // succeed. Rejecting here skips the caller's `popErrorScope`.
    if state::device_is_lost(self.device_id) {
      return js::promise(scope, v8::undefined(scope).into());
    }
    let handle = match state::handle(self.device_id) {
      Ok(handle) => handle,
      Err(error) => return js::promise_rejected(scope, error),
    };
    // The callback runs on the polling thread, before `poll` returns.
    // Resolve after the wait even when it did not run, so the promise
    // cannot stay pending.
    handle.queue.on_submitted_work_done(|| {});
    let _ = handle.device.poll(wgpu::PollType::Wait {
      submission_index: None,
      timeout: Some(std::time::Duration::from_secs(5)),
    });
    js::promise(scope, v8::undefined(scope).into())
  }
}

pub struct GpuValidationError {
  message: String,
}

unsafe impl v8::cppgc::GarbageCollected for GpuValidationError {
  fn trace(&self, _visitor: &mut v8::cppgc::Visitor) {}

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"GPUValidationError"
  }
}

#[op(name = "GPUValidationError")]
impl GpuValidationError {
  #[constructor]
  fn new(message: String) -> Self {
    Self { message }
  }

  #[getter]
  fn message(&self) -> String {
    self.message.clone()
  }
}

pub struct GpuOutOfMemoryError {
  message: String,
}

unsafe impl v8::cppgc::GarbageCollected for GpuOutOfMemoryError {
  fn trace(&self, _visitor: &mut v8::cppgc::Visitor) {}

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"GPUOutOfMemoryError"
  }
}

#[op(name = "GPUOutOfMemoryError")]
impl GpuOutOfMemoryError {
  #[constructor]
  fn new(message: String) -> Self {
    Self { message }
  }

  #[getter]
  fn message(&self) -> String {
    self.message.clone()
  }
}

pub struct GpuInternalError {
  message: String,
}

unsafe impl v8::cppgc::GarbageCollected for GpuInternalError {
  fn trace(&self, _visitor: &mut v8::cppgc::Visitor) {}

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"GPUInternalError"
  }
}

#[op(name = "GPUInternalError")]
impl GpuInternalError {
  #[constructor]
  fn new(message: String) -> Self {
    Self { message }
  }

  #[getter]
  fn message(&self) -> String {
    self.message.clone()
  }
}

#[repr(C)]
pub struct GpuUncapturedErrorEvent {
  base: Event,
  error: v8::TracedReference<v8::Object>,
}

unsafe impl v8::cppgc::GarbageCollected for GpuUncapturedErrorEvent {
  fn trace(&self, visitor: &mut v8::cppgc::Visitor) {
    v8::cppgc::GarbageCollected::trace(&self.base, visitor);
    visitor.trace(&self.error);
  }

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"GPUUncapturedErrorEvent"
  }
}

#[op(name = "GPUUncapturedErrorEvent", inherit = Event)]
impl GpuUncapturedErrorEvent {
  #[constructor]
  #[length(1)]
  fn new(
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<Self, OpError> {
    let base = Event::from_args(scope, args, false)?;
    let error = gpu_error_argument(scope, args)?;
    Ok(Self {
      base,
      error: yuzuko_core::op::traced(scope, error),
    })
  }

  #[construct_hook]
  fn finish(&self, scope: &mut v8::PinScope, object: v8::Local<v8::Object>) {
    let _ = self;
    define_is_trusted(scope, object);
  }

  #[getter]
  fn error(&self, scope: &mut v8::PinScope) -> Js {
    match yuzuko_core::op::traced_local(scope, &self.error) {
      Some(value) => {
        let value: v8::Local<v8::Value> = value.into();
        Js(v8::Global::new(scope, value))
      }
      None => Js::null(scope),
    }
  }
}

pub fn install(scope: &mut v8::PinScope) {
  yuzuko_core::add_array_buffer_transfer_block(
    resource::mapping_blocks_transfer,
  );
  Gpu::install(scope);
  GpuAdapter::install(scope);
  GpuDevice::install(scope);
  let constructor = GpuDevice::template(scope)
    .get_function(scope)
    .expect("GPUDevice");
  yuzuko_dom::define_prototype_event_handlers(
    scope,
    constructor,
    &["uncapturederror"],
  );
  GpuQueue::install(scope);
  GpuValidationError::install(scope);
  GpuOutOfMemoryError::install(scope);
  GpuInternalError::install(scope);
  GpuUncapturedErrorEvent::install(scope);
  GpuBuffer::install(scope);
  GpuTexture::install(scope);
  resource::GpuTextureView::install(scope);
  resource::GpuSampler::install(scope);
  resource::GpuShaderModule::install(scope);
  resource::GpuBindGroupLayout::install(scope);
  resource::GpuBindGroup::install(scope);
  resource::GpuPipelineLayout::install(scope);
  resource::GpuRenderPipeline::install(scope);
  resource::GpuComputePipeline::install(scope);
  resource::GpuQuerySet::install(scope);
  resource::GpuExternalTexture::install(scope);
  GpuCommandEncoder::install(scope);
  encode::GpuCommandBuffer::install(scope);
  encode::GpuRenderPass::install(scope);
  encode::GpuComputePass::install(scope);
  encode::GpuRenderBundleEncoder::install(scope);
  encode::GpuRenderBundle::install(scope);
  crate::canvas::GpuCanvasContext::install(scope);
  install_flags(scope);
  install_navigator(scope);
  install_webgpu_globals(scope);
}

/// `GPUPipelineError` is a `DOMException`, so it is installed as a script
/// after that constructor exists. `GPUSupportedLimits` is an interface object
/// with an illegal constructor and no `maxImmediateSize`. `GPUError` is an
/// illegal constructor. The three error interfaces inherit from it.
const WEBGPU_GLOBALS: &str = r#"
(() => {
  function GPUError() {
    throw new TypeError("Illegal constructor");
  }
  Object.defineProperty(globalThis, "GPUError", {
    value: GPUError,
    writable: true,
    enumerable: false,
    configurable: true,
  });
  Object.setPrototypeOf(GPUValidationError.prototype, GPUError.prototype);
  Object.setPrototypeOf(GPUOutOfMemoryError.prototype, GPUError.prototype);
  Object.setPrototypeOf(GPUInternalError.prototype, GPUError.prototype);
  class GPUPipelineError extends DOMException {
    constructor(message = "", options) {
      if (typeof options !== "object" || options === null) {
        throw new TypeError("GPUPipelineError options are required");
      }
      const reason = options.reason;
      if (reason !== "validation" && reason !== "internal") {
        throw new TypeError("The pipeline error reason is invalid");
      }
      const text = message === undefined ? "" : String(message);
      super(text, "GPUPipelineError");
      Object.defineProperty(this, "reason", {
        value: reason,
        enumerable: true,
        configurable: true,
        writable: false,
      });
    }
  }
  function GPUSupportedLimits() {
    throw new TypeError("Illegal constructor");
  }
  Object.defineProperty(globalThis, "GPUPipelineError", {
    value: GPUPipelineError,
    writable: true,
    enumerable: false,
    configurable: true,
  });
  Object.defineProperty(globalThis, "GPUSupportedLimits", {
    value: GPUSupportedLimits,
    writable: true,
    enumerable: false,
    configurable: true,
  });
})();
"#;

fn install_webgpu_globals(scope: &mut v8::PinScope) {
  let source = v8::String::new(scope, WEBGPU_GLOBALS).expect("webgpu globals");
  let script =
    v8::Script::compile(scope, source, None).expect("webgpu globals");
  script.run(scope).expect("webgpu globals");
  install_limit_getters(scope);
}

fn install_navigator(scope: &mut v8::PinScope) {
  let gpu = unsafe { yuzuko_core::op::wrap_instance(scope, Gpu::new()) };
  let global = scope.get_current_context().global(scope);
  let nav_key = v8::String::new(scope, "navigator").unwrap();
  let Some(navigator) = global.get(scope, nav_key.into()) else {
    return;
  };
  let Ok(navigator) = v8::Local::<v8::Object>::try_from(navigator) else {
    return;
  };
  let key = v8::String::new(scope, "gpu").unwrap();
  let mut descriptor =
    v8::PropertyDescriptor::new_from_value_writable(gpu.into(), false);
  descriptor.set_enumerable(true);
  descriptor.set_configurable(true);
  let _ = navigator.define_property(scope, key.into(), &descriptor);
}

fn install_flags(scope: &mut v8::PinScope) {
  let global = scope.get_current_context().global(scope);
  define_flags(
    scope,
    global,
    "GPUBufferUsage",
    &[
      ("MAP_READ", wgpu::BufferUsages::MAP_READ.bits()),
      ("MAP_WRITE", wgpu::BufferUsages::MAP_WRITE.bits()),
      ("COPY_SRC", wgpu::BufferUsages::COPY_SRC.bits()),
      ("COPY_DST", wgpu::BufferUsages::COPY_DST.bits()),
      ("INDEX", wgpu::BufferUsages::INDEX.bits()),
      ("VERTEX", wgpu::BufferUsages::VERTEX.bits()),
      ("UNIFORM", wgpu::BufferUsages::UNIFORM.bits()),
      ("STORAGE", wgpu::BufferUsages::STORAGE.bits()),
      ("INDIRECT", wgpu::BufferUsages::INDIRECT.bits()),
      ("QUERY_RESOLVE", wgpu::BufferUsages::QUERY_RESOLVE.bits()),
    ],
  );
  define_flags(
    scope,
    global,
    "GPUTextureUsage",
    &[
      ("COPY_SRC", wgpu::TextureUsages::COPY_SRC.bits()),
      ("COPY_DST", wgpu::TextureUsages::COPY_DST.bits()),
      (
        "TEXTURE_BINDING",
        wgpu::TextureUsages::TEXTURE_BINDING.bits(),
      ),
      (
        "STORAGE_BINDING",
        wgpu::TextureUsages::STORAGE_BINDING.bits(),
      ),
      (
        "RENDER_ATTACHMENT",
        wgpu::TextureUsages::RENDER_ATTACHMENT.bits(),
      ),
      (
        "TRANSIENT_ATTACHMENT",
        wgpu::TextureUsages::TRANSIENT_ATTACHMENT.bits(),
      ),
    ],
  );
  define_flags(
    scope,
    global,
    "GPUShaderStage",
    &[
      ("VERTEX", wgpu::ShaderStages::VERTEX.bits()),
      ("FRAGMENT", wgpu::ShaderStages::FRAGMENT.bits()),
      ("COMPUTE", wgpu::ShaderStages::COMPUTE.bits()),
    ],
  );
  define_flags(
    scope,
    global,
    "GPUColorWrite",
    &[
      ("RED", wgpu::ColorWrites::RED.bits()),
      ("GREEN", wgpu::ColorWrites::GREEN.bits()),
      ("BLUE", wgpu::ColorWrites::BLUE.bits()),
      ("ALPHA", wgpu::ColorWrites::ALPHA.bits()),
      ("ALL", wgpu::ColorWrites::ALL.bits()),
    ],
  );
  define_flags(
    scope,
    global,
    "GPUMapMode",
    &[("READ", 0x0001), ("WRITE", 0x0002)],
  );
}

fn define_flags(
  scope: &mut v8::PinScope,
  global: v8::Local<v8::Object>,
  name: &str,
  pairs: &[(&str, u32)],
) {
  let flags = js::flag_object(scope, pairs);
  let object = v8::Local::new(scope, &flags);
  let key = v8::String::new(scope, name).unwrap();
  global.set(scope, key.into(), object.into());
}

fn open_device(
  scope: &mut v8::PinScope,
  descriptor: Option<v8::Local<v8::Value>>,
) -> Result<GpuDevice, OpError> {
  if yuzuko_window::gpu_foreign() {
    return Err(OpError::dom(
      "InvalidStateError",
      "The GPU device belongs to another thread",
    ));
  }
  if !state::ensure_adapter() {
    return Err(OpError::dom(
      "OperationError",
      "The GPU adapter is unavailable",
    ));
  }
  let (device, queue, limits, features) =
    gpu_parts().map(|(_adapter, device, queue)| {
      let limits = device.limits();
      let features = device.features();
      (device, queue, limits, features)
    })?;
  let mut device_label = None;
  let mut queue_label = None;
  if let Some(object) = dictionary(scope, descriptor) {
    reject_features(scope, object, features)?;
    check_required_limits(scope, object, &limits)?;
    device_label = js::get(scope, object, "label")?
      .map(|value| js::string(scope, value))
      .transpose()?;
    queue_label = queue_label_of(scope, object)?;
  }
  let id = state::alloc_id();
  let lost = Arc::new(AtomicBool::new(false));
  let resolver = v8::PromiseResolver::new(scope).unwrap();
  let promise = resolver.get_promise(scope);
  let queue_object = unsafe {
    yuzuko_core::op::wrap_instance(
      scope,
      GpuQueue {
        device_id: id,
        label: ObjectLabel::new(queue_label),
      },
    )
  };
  state::insert(
    id,
    state::LiveDevice {
      device,
      queue,
      lost,
      lost_promise: v8::Global::new(scope, promise),
      lost_resolver: v8::Global::new(scope, resolver),
      scopes: Vec::new(),
      queue_object: Some(v8::Global::new(scope, queue_object)),
      features_object: None,
      limits_object: None,
      adapter_info: None,
      owner: yuzuko_core::caller_module_url(scope),
    },
  );
  Ok(GpuDevice {
    base: EventTarget::new(),
    id,
    label: ObjectLabel::new(device_label),
  })
}

enum ScopeFailure {
  Validation(String),
  OutOfMemory(String),
  Internal(String),
}

impl ScopeFailure {
  fn reason(&self) -> &'static str {
    match self {
      Self::Validation(_) => "validation",
      Self::OutOfMemory(_) | Self::Internal(_) => "internal",
    }
  }

  fn message(&self) -> &str {
    match self {
      Self::Validation(message)
      | Self::OutOfMemory(message)
      | Self::Internal(message) => message,
    }
  }
}

fn finish_scope(id: u64) -> Result<Option<ScopeFailure>, OpError> {
  let popped = state::pop_scope(id)?;
  let lost = state::device_is_lost(id);
  let device = state::wgpu_device(id)?;
  let _ = device.poll(wgpu::PollType::Wait {
    submission_index: None,
    timeout: Some(std::time::Duration::from_secs(5)),
  });
  // Newer scopes from other devices sit above this one on the single wgpu
  // stack. Pop them first, keep the error on that device, and push the scope
  // again after this one is gone.
  let state::PoppedScope {
    guard,
    kind,
    synthetic,
    captured,
    mut displaced,
  } = popped;
  let mut restore = Vec::new();
  for item in displaced.take() {
    let depth = restore
      .iter()
      .filter(|(other, _)| *other == item.id)
      .count();
    let error = pollster::block_on(item.guard.pop());
    if let Some(error) = error {
      state::store_captured(item.id, depth, capture_from_wgpu(&error));
    }
    restore.push((item.id, item.kind));
  }
  let error = pollster::block_on(guard.pop());
  let mut repush_error = None;
  for (other, kind) in restore.into_iter().rev() {
    if let Err(error) = state::repush_scope(other, kind) {
      repush_error.get_or_insert(error);
    }
  }
  if let Some(error) = repush_error {
    return Err(error);
  }
  // The scope stack is already popped. A lost device resolves `null`.
  if lost {
    return Ok(None);
  }
  if let Some(captured) = captured {
    return Ok(Some(failure_from_capture(captured)));
  }
  if let Some(error) = error {
    return Ok(Some(failure_from_wgpu(error)));
  }
  Ok(synthetic.map(|message| failure_from_kind(kind, message)))
}

fn failure_from_wgpu(error: wgpu::Error) -> ScopeFailure {
  let message = error.to_string();
  match error {
    wgpu::Error::OutOfMemory { .. } => ScopeFailure::OutOfMemory(message),
    wgpu::Error::Internal { .. } => ScopeFailure::Internal(message),
    wgpu::Error::Validation { .. } => ScopeFailure::Validation(message),
  }
}

fn capture_from_wgpu(error: &wgpu::Error) -> state::ScopeCapture {
  let message = error.to_string();
  let kind = match error {
    wgpu::Error::OutOfMemory { .. } => state::ScopeKind::OutOfMemory,
    wgpu::Error::Internal { .. } => state::ScopeKind::Internal,
    wgpu::Error::Validation { .. } => state::ScopeKind::Validation,
  };
  state::ScopeCapture { kind, message }
}

fn failure_from_capture(capture: state::ScopeCapture) -> ScopeFailure {
  failure_from_kind(capture.kind, capture.message)
}

fn failure_from_kind(kind: state::ScopeKind, message: String) -> ScopeFailure {
  match kind {
    state::ScopeKind::Validation => ScopeFailure::Validation(message),
    state::ScopeKind::OutOfMemory => ScopeFailure::OutOfMemory(message),
    state::ScopeKind::Internal => ScopeFailure::Internal(message),
  }
}

fn pop_scope<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  id: u64,
) -> Result<v8::Local<'s, v8::Value>, OpError> {
  let Some(failure) = finish_scope(id)? else {
    return Ok(v8::null(scope).into());
  };
  let message = failure.message().to_string();
  let object = match failure {
    ScopeFailure::OutOfMemory(_) => unsafe {
      yuzuko_core::op::wrap_instance(scope, GpuOutOfMemoryError { message })
    },
    ScopeFailure::Internal(_) => unsafe {
      yuzuko_core::op::wrap_instance(scope, GpuInternalError { message })
    },
    ScopeFailure::Validation(_) => unsafe {
      yuzuko_core::op::wrap_instance(scope, GpuValidationError { message })
    },
  };
  Ok(object.into())
}

enum AsyncPipeline {
  Compute,
  Render,
}

/// Push a validation scope, create the pipeline, and pop that scope.
/// The promise is already settled. A validation or internal error from the
/// scope rejects `GPUPipelineError`. `TypeError` rejects the original error.
fn async_pipeline(
  scope: &mut v8::PinScope,
  id: u64,
  descriptor: v8::Local<v8::Value>,
  kind: AsyncPipeline,
) -> Js {
  let handle = match state::handle(id) {
    Ok(handle) => handle,
    Err(error) => return js::promise_rejected(scope, error),
  };
  let guard = handle
    .device
    .push_error_scope(wgpu::ErrorFilter::Validation);
  if let Err(error) = state::push_scope(id, guard, state::ScopeKind::Validation)
  {
    return js::promise_rejected(scope, error);
  }
  let created = match kind {
    AsyncPipeline::Compute => resource::create_compute_pipeline(
      scope, id, &handle, descriptor,
    )
    .map(|New(pipeline)| {
      let object = unsafe { yuzuko_core::op::wrap_instance(scope, pipeline) };
      v8::Local::<v8::Value>::from(object)
    }),
    AsyncPipeline::Render => resource::create_render_pipeline(
      scope, id, &handle, descriptor,
    )
    .map(|New(pipeline)| {
      let object = unsafe { yuzuko_core::op::wrap_instance(scope, pipeline) };
      object.into()
    }),
  };
  let failure = match finish_scope(id) {
    Ok(failure) => failure,
    Err(error) => {
      if let Err(created) = created
        && !is_operation_error(&created)
      {
        return js::promise_rejected(scope, created);
      }
      return js::promise_rejected(scope, error);
    }
  };
  match created {
    Err(error) if !is_operation_error(&error) => {
      js::promise_rejected(scope, error)
    }
    Err(error) => {
      let (message, reason) = pipeline_rejection(&error, failure.as_ref());
      reject_pipeline(scope, &message, reason)
    }
    Ok(value) => match failure {
      Some(failure) => {
        let message = failure.message().to_string();
        reject_pipeline(scope, &message, failure.reason())
      }
      None => js::promise(scope, value),
    },
  }
}

fn is_operation_error(error: &OpError) -> bool {
  matches!(error, OpError::Dom { name, .. } if name == "OperationError")
}

fn pipeline_rejection(
  error: &OpError,
  failure: Option<&ScopeFailure>,
) -> (String, &'static str) {
  if let Some(failure) = failure {
    return (failure.message().to_string(), failure.reason());
  }
  let message = match error {
    OpError::Dom { message, .. } => message.clone(),
    OpError::Type(message) | OpError::Range(message) => message.clone(),
    OpError::Thrown(_) => "The pipeline is invalid".to_string(),
  };
  (message, "validation")
}

fn reject_pipeline(
  scope: &mut v8::PinScope,
  message: &str,
  reason: &str,
) -> Js {
  match pipeline_error_value(scope, message, reason) {
    Some(error) => js::promise_rejected_value(scope, error),
    None => js::promise_rejected(
      scope,
      OpError::dom("OperationError", message.to_string()),
    ),
  }
}

fn pipeline_error_value<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  message: &str,
  reason: &str,
) -> Option<v8::Local<'s, v8::Value>> {
  let global = scope.get_current_context().global(scope);
  let key = v8::String::new(scope, "GPUPipelineError")?;
  let constructor = global.get(scope, key.into())?;
  let constructor = v8::Local::<v8::Function>::try_from(constructor).ok()?;
  let message = v8::String::new(scope, message)?;
  let options = v8::Object::new(scope);
  let reason_key = v8::String::new(scope, "reason")?;
  let reason_value = v8::String::new(scope, reason)?;
  let _ = options.set(scope, reason_key.into(), reason_value.into());
  let arguments = [v8::Local::<v8::Value>::from(message), options.into()];
  constructor
    .new_instance(scope, &arguments)
    .map(v8::Local::from)
}

fn write_contents(
  scope: &mut v8::PinScope,
  data: v8::Local<v8::Value>,
  data_offset: Option<u64>,
  size: Option<u64>,
) -> Result<Vec<u8>, OpError> {
  let bytes = yuzuko_winter::buffer_source(scope, data)?.ok_or_else(|| {
    OpError::type_("writeBuffer data must be an AllowSharedBufferSource")
  })?;
  let element = source_element_size(data);
  let elements = (bytes.len() / element) as u64;
  let data_offset = data_offset.unwrap_or(0);
  if data_offset > elements {
    return Err(OpError::dom(
      "OperationError",
      "The write is outside the data",
    ));
  }
  let count = size.unwrap_or(elements - data_offset);
  let end = data_offset.checked_add(count).ok_or_else(|| {
    OpError::dom("OperationError", "The write is outside the data")
  })?;
  if end > elements {
    return Err(OpError::dom(
      "OperationError",
      "The write is outside the data",
    ));
  }
  let byte_len = (count as usize).saturating_mul(element);
  if !byte_len.is_multiple_of(4) {
    return Err(OpError::dom(
      "OperationError",
      "The write is outside the data",
    ));
  }
  let start = (data_offset as usize).saturating_mul(element);
  Ok(bytes[start..start + byte_len].to_vec())
}

fn source_element_size(value: v8::Local<v8::Value>) -> usize {
  if value.is_float64_array()
    || value.is_big_int64_array()
    || value.is_big_uint64_array()
  {
    8
  } else if value.is_float32_array()
    || value.is_uint32_array()
    || value.is_int32_array()
  {
    4
  } else if value.is_float16_array()
    || value.is_uint16_array()
    || value.is_int16_array()
  {
    2
  } else {
    1
  }
}

fn adapter_info_object<'s>(
  scope: &mut v8::PinScope<'s, '_>,
) -> Result<v8::Local<'s, v8::Object>, OpError> {
  let info = gpu_parts()?.0.get_info();
  let object = v8::Object::new(scope);
  let set = |name: &str, value: v8::Local<v8::Value>| {
    let key = v8::String::new(scope, name).unwrap();
    object.set(scope, key.into(), value);
  };
  let hex = |id: u32| {
    if id == 0 {
      String::new()
    } else {
      format!("{id:04x}")
    }
  };
  let description = if info.name.is_empty() {
    info.driver_info
  } else {
    info.name
  };
  set("architecture", v8::String::new(scope, "").unwrap().into());
  set(
    "description",
    v8::String::new(scope, &description).unwrap().into(),
  );
  let device = hex(info.device);
  set("device", v8::String::new(scope, &device).unwrap().into());
  set("isFallbackAdapter", v8::Boolean::new(scope, false).into());
  set(
    "subgroupMaxSize",
    v8::Number::new(scope, f64::from(info.subgroup_max_size)).into(),
  );
  set(
    "subgroupMinSize",
    v8::Number::new(scope, f64::from(info.subgroup_min_size)).into(),
  );
  let vendor = hex(info.vendor);
  set("vendor", v8::String::new(scope, &vendor).unwrap().into());
  Ok(object)
}

fn gpu_parts() -> Result<(wgpu::Adapter, wgpu::Device, wgpu::Queue), OpError> {
  yuzuko_window::with_gpu(|adapter, device, queue| {
    (adapter.clone(), device.clone(), queue.clone())
  })
  .ok_or_else(|| {
    OpError::dom("InvalidStateError", "The GPU adapter is unavailable")
  })
}

fn preferred_format() -> Result<&'static str, OpError> {
  if !state::ensure_adapter() {
    return Err(OpError::dom(
      "InvalidStateError",
      "No GPU adapter is available",
    ));
  }
  let backend = gpu_parts()?.0.get_info().backend;
  Ok(match backend {
    wgpu::Backend::Metal | wgpu::Backend::Dx12 | wgpu::Backend::Gl => {
      "bgra8unorm"
    }
    _ => "rgba8unorm",
  })
}

/// `Ok(true)` is the process adapter. `Ok(false)` resolves `null`.
fn adapter_choice(
  scope: &mut v8::PinScope,
  options: Option<v8::Local<v8::Value>>,
) -> Result<bool, OpError> {
  let Some(object) = dictionary(scope, options) else {
    return Ok(state::ensure_adapter());
  };
  if let Some(value) = js::try_get(scope, object, "featureLevel")? {
    let value = v8::Local::new(scope, value);
    match js::try_string(scope, value)?.as_str() {
      "core" => {}
      "compatibility" => return Ok(false),
      _ => return Err(OpError::type_("The feature level is invalid")),
    }
  }
  if let Some(value) = js::try_get(scope, object, "xrCompatible")? {
    let value = v8::Local::new(scope, value);
    if value.boolean_value(scope) {
      return Ok(false);
    }
  }
  if let Some(value) = js::try_get(scope, object, "forceFallbackAdapter")? {
    let value = v8::Local::new(scope, value);
    if value.boolean_value(scope) {
      return Ok(false);
    }
  }
  Ok(state::ensure_adapter())
}

fn queue_label_of(
  scope: &mut v8::PinScope,
  descriptor: v8::Local<v8::Object>,
) -> Result<Option<String>, OpError> {
  let Some(value) = js::try_get(scope, descriptor, "defaultQueue")? else {
    return Ok(None);
  };
  let value = v8::Local::new(scope, value);
  if value.is_null() || !value.is_object() {
    return Err(OpError::type_("defaultQueue must be an object"));
  }
  let queue = v8::Local::<v8::Object>::try_from(value)
    .map_err(|_| OpError::type_("defaultQueue must be an object"))?;
  let Some(label) = js::try_get(scope, queue, "label")? else {
    return Ok(None);
  };
  let label = v8::Local::new(scope, label);
  Ok(Some(js::try_string(scope, label)?))
}

fn dictionary<'a>(
  scope: &mut v8::PinScope,
  value: Option<v8::Local<'a, v8::Value>>,
) -> Option<v8::Local<'a, v8::Object>> {
  let value = value?;
  if value.is_null_or_undefined() {
    return None;
  }
  js::object(scope, value)
}

fn reject_features(
  scope: &mut v8::PinScope,
  descriptor: v8::Local<v8::Object>,
  enabled: wgpu::Features,
) -> Result<(), OpError> {
  let Some(value) = js::get(scope, descriptor, "requiredFeatures")? else {
    return Ok(());
  };
  if value.is_null() {
    return Ok(());
  }
  for name in feature_sequence(scope, value)? {
    let feature = name
      .parse::<wgpu::Features>()
      .map_err(|_| OpError::type_("The feature name is invalid"))?;
    // `external-texture` parses as a native wgpu feature. It is not a
    // WebGPU feature name, so a script cannot require it.
    if !wgpu::Features::all_webgpu_mask().contains(feature) {
      return Err(OpError::type_("The feature name is invalid"));
    }
    if !enabled.contains(feature) {
      return Err(OpError::type_(
        "Required features are not enabled on the shared device",
      ));
    }
  }
  Ok(())
}

fn check_required_limits(
  scope: &mut v8::PinScope,
  descriptor: v8::Local<v8::Object>,
  limits: &wgpu::Limits,
) -> Result<(), OpError> {
  let Some(value) = js::get(scope, descriptor, "requiredLimits")? else {
    return Ok(());
  };
  let Some(requested) = js::object(scope, value) else {
    return Err(OpError::type_("requiredLimits must be an object"));
  };
  for (name, actual, lower_is_better) in limit_table(limits) {
    let Some(value) = js::get(scope, requested, name)? else {
      continue;
    };
    let number = js::finite_number(scope, value)?;
    let exceeds = if lower_is_better {
      number < actual
    } else {
      number > actual
    };
    if exceeds {
      return Err(OpError::type_(format!(
        "The required limit {name} is not supported"
      )));
    }
  }
  Ok(())
}

fn limit_table(limits: &wgpu::Limits) -> Vec<(&'static str, f64, bool)> {
  let plus = limits
    .max_bind_groups
    .saturating_add(limits.max_vertex_buffers);
  vec![
    (
      "maxTextureDimension1D",
      f64::from(limits.max_texture_dimension_1d),
      false,
    ),
    (
      "maxTextureDimension2D",
      f64::from(limits.max_texture_dimension_2d),
      false,
    ),
    (
      "maxTextureDimension3D",
      f64::from(limits.max_texture_dimension_3d),
      false,
    ),
    (
      "maxTextureArrayLayers",
      f64::from(limits.max_texture_array_layers),
      false,
    ),
    ("maxBindGroups", f64::from(limits.max_bind_groups), false),
    ("maxBindGroupsPlusVertexBuffers", f64::from(plus), false),
    (
      "maxBindingsPerBindGroup",
      f64::from(limits.max_bindings_per_bind_group),
      false,
    ),
    (
      "maxDynamicUniformBuffersPerPipelineLayout",
      f64::from(limits.max_dynamic_uniform_buffers_per_pipeline_layout),
      false,
    ),
    (
      "maxDynamicStorageBuffersPerPipelineLayout",
      f64::from(limits.max_dynamic_storage_buffers_per_pipeline_layout),
      false,
    ),
    (
      "maxSampledTexturesPerShaderStage",
      f64::from(limits.max_sampled_textures_per_shader_stage),
      false,
    ),
    (
      "maxSamplersPerShaderStage",
      f64::from(limits.max_samplers_per_shader_stage),
      false,
    ),
    (
      "maxStorageBuffersPerShaderStage",
      f64::from(limits.max_storage_buffers_per_shader_stage),
      false,
    ),
    (
      "maxStorageBuffersInVertexStage",
      f64::from(limits.max_storage_buffers_per_shader_stage),
      false,
    ),
    (
      "maxStorageBuffersInFragmentStage",
      f64::from(limits.max_storage_buffers_per_shader_stage),
      false,
    ),
    (
      "maxStorageTexturesPerShaderStage",
      f64::from(limits.max_storage_textures_per_shader_stage),
      false,
    ),
    (
      "maxStorageTexturesInVertexStage",
      f64::from(limits.max_storage_textures_per_shader_stage),
      false,
    ),
    (
      "maxStorageTexturesInFragmentStage",
      f64::from(limits.max_storage_textures_per_shader_stage),
      false,
    ),
    (
      "maxUniformBuffersPerShaderStage",
      f64::from(limits.max_uniform_buffers_per_shader_stage),
      false,
    ),
    (
      "maxUniformBufferBindingSize",
      limits.max_uniform_buffer_binding_size as f64,
      false,
    ),
    (
      "maxStorageBufferBindingSize",
      limits.max_storage_buffer_binding_size as f64,
      false,
    ),
    (
      "maxVertexBuffers",
      f64::from(limits.max_vertex_buffers),
      false,
    ),
    ("maxBufferSize", limits.max_buffer_size as f64, false),
    (
      "maxVertexAttributes",
      f64::from(limits.max_vertex_attributes),
      false,
    ),
    (
      "maxVertexBufferArrayStride",
      f64::from(limits.max_vertex_buffer_array_stride),
      false,
    ),
    (
      "maxInterStageShaderVariables",
      f64::from(limits.max_inter_stage_shader_variables),
      false,
    ),
    (
      "maxColorAttachments",
      f64::from(limits.max_color_attachments),
      false,
    ),
    (
      "maxColorAttachmentBytesPerSample",
      f64::from(limits.max_color_attachment_bytes_per_sample),
      false,
    ),
    (
      "maxComputeWorkgroupStorageSize",
      f64::from(limits.max_compute_workgroup_storage_size),
      false,
    ),
    (
      "maxComputeInvocationsPerWorkgroup",
      f64::from(limits.max_compute_invocations_per_workgroup),
      false,
    ),
    (
      "maxComputeWorkgroupSizeX",
      f64::from(limits.max_compute_workgroup_size_x),
      false,
    ),
    (
      "maxComputeWorkgroupSizeY",
      f64::from(limits.max_compute_workgroup_size_y),
      false,
    ),
    (
      "maxComputeWorkgroupSizeZ",
      f64::from(limits.max_compute_workgroup_size_z),
      false,
    ),
    (
      "maxComputeWorkgroupsPerDimension",
      f64::from(limits.max_compute_workgroups_per_dimension),
      false,
    ),
    (
      "minUniformBufferOffsetAlignment",
      f64::from(limits.min_uniform_buffer_offset_alignment),
      true,
    ),
    (
      "minStorageBufferOffsetAlignment",
      f64::from(limits.min_storage_buffer_offset_alignment),
      true,
    ),
  ]
}

fn limits_object<'s>(
  scope: &mut v8::PinScope<'s, '_>,
) -> v8::Local<'s, v8::Object> {
  let object = v8::Object::new(scope);
  if let Some(prototype) = limits_prototype(scope) {
    let _ = object.set_prototype(scope, prototype.into());
  }
  object
}

fn limits_prototype<'s>(
  scope: &mut v8::PinScope<'s, '_>,
) -> Option<v8::Local<'s, v8::Object>> {
  let global = scope.get_current_context().global(scope);
  let key = v8::String::new(scope, "GPUSupportedLimits")?;
  let constructor = global.get(scope, key.into())?;
  let constructor = v8::Local::<v8::Object>::try_from(constructor).ok()?;
  let prototype_key = v8::String::new(scope, "prototype")?;
  constructor
    .get(scope, prototype_key.into())
    .and_then(|value| v8::Local::<v8::Object>::try_from(value).ok())
}

fn features_object<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  features: wgpu::Features,
) -> v8::Local<'s, v8::Object> {
  let set = v8::Set::new(scope);
  let web = features & wgpu::Features::all_webgpu_mask();
  for feature in web.iter() {
    let Some(name) = feature.as_str() else {
      continue;
    };
    let Some(text) = v8::String::new(scope, name) else {
      continue;
    };
    set.add(scope, text.into());
  }
  set.into()
}

/// Limit attributes are prototype getters. Their values stay the shared
/// device's limits. `maxBindGroupsPlusVertexBuffers` is the sum of the
/// bind-group and vertex-buffer limits.
fn install_limit_getters(scope: &mut v8::PinScope) {
  let Some(proto) = limits_prototype(scope) else {
    return;
  };
  for (name, _, _) in limit_table(&wgpu::Limits::default()) {
    let Some(key) = v8::String::new(scope, name) else {
      continue;
    };
    let Some(getter) = v8::Function::builder(limit_getter)
      .length(0)
      .constructor_behavior(v8::ConstructorBehavior::Throw)
      .data(key.into())
      .build(scope)
    else {
      continue;
    };
    let Some(label) = v8::String::new(scope, &format!("get {name}")) else {
      continue;
    };
    getter.set_name(label);
    let get: v8::Local<v8::Value> = getter.into();
    let set: v8::Local<v8::Value> = v8::undefined(scope).into();
    let mut descriptor = v8::PropertyDescriptor::new_from_get_set(get, set);
    descriptor.set_enumerable(true);
    descriptor.set_configurable(true);
    let _ = proto.define_property(scope, key.into(), &descriptor);
  }
}

fn limit_getter(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut ret: v8::ReturnValue,
) {
  let name = args.data().to_rust_string_lossy(scope);
  match limit_value(&name) {
    Ok(value) => ret.set(v8::Number::new(scope, value).into()),
    Err(error) => OpException::throw(error, scope),
  }
}

fn limit_value(name: &str) -> Result<f64, OpError> {
  let (_, device, _) = gpu_parts()?;
  let limits = device.limits();
  limit_table(&limits)
    .into_iter()
    .find(|(limit, _, _)| *limit == name)
    .map(|(_, value, _)| value)
    .ok_or_else(|| OpError::type_("The limit is unknown"))
}

fn language_features<'s>(
  scope: &mut v8::PinScope<'s, '_>,
) -> v8::Local<'s, v8::Object> {
  let set = v8::Set::new(scope);
  for name in [
    "readonly_and_readwrite_storage_textures",
    "packed_4x8_integer_dot_product",
    "pointer_composite_access",
  ] {
    let Some(text) = v8::String::new(scope, name) else {
      continue;
    };
    set.add(scope, text.into());
  }
  set.into()
}

fn device_member<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  id: u64,
  kind: state::CachedObject,
  create: impl FnOnce(
    &mut v8::PinScope<'s, '_>,
  ) -> Result<v8::Local<'s, v8::Object>, OpError>,
) -> Result<Js, OpError> {
  if let Some(cached) = state::cached_object(id, kind)? {
    return Ok(object_js(scope, &cached));
  }
  let object = create(scope)?;
  let stored = state::store_cached(id, kind, v8::Global::new(scope, object))?;
  Ok(object_js(scope, &stored))
}

fn object_js(scope: &mut v8::PinScope, object: &v8::Global<v8::Object>) -> Js {
  let local = v8::Local::new(scope, object);
  let value: v8::Local<v8::Value> = local.into();
  Js(v8::Global::new(scope, value))
}

fn recalled_js(
  scope: &mut v8::PinScope,
  slot: &RefCell<Option<v8::TracedReference<v8::Object>>>,
) -> Option<Js> {
  let local = {
    let borrowed = slot.borrow();
    let reference = borrowed.as_ref()?;
    yuzuko_core::op::traced_local(scope, reference)?
  };
  let value: v8::Local<v8::Value> = local.into();
  Some(Js(v8::Global::new(scope, value)))
}

fn remember_js(
  scope: &mut v8::PinScope,
  slot: &RefCell<Option<v8::TracedReference<v8::Object>>>,
  object: v8::Local<v8::Object>,
) -> Js {
  *slot.borrow_mut() = Some(yuzuko_core::op::traced(scope, object));
  let value: v8::Local<v8::Value> = object.into();
  Js(v8::Global::new(scope, value))
}

fn trace_slot(
  slot: &RefCell<Option<v8::TracedReference<v8::Object>>>,
  visitor: &mut v8::cppgc::Visitor,
) {
  if let Ok(slot) = slot.try_borrow()
    && let Some(reference) = slot.as_ref()
  {
    visitor.trace(reference);
  }
}

fn feature_sequence(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<Vec<String>, OpError> {
  let Some(object) = value.to_object(scope) else {
    return Err(OpError::type_("requiredFeatures must be a sequence"));
  };
  let symbol = v8::Symbol::get_iterator(scope);
  let Some(method) = object.get(scope, symbol.into()) else {
    return Err(OpError::type_("requiredFeatures must be a sequence"));
  };
  let Ok(method) = v8::Local::<v8::Function>::try_from(method) else {
    return Err(OpError::type_("requiredFeatures must be a sequence"));
  };
  let Some(iterator) = method.call(scope, object.into(), &[]) else {
    return Err(OpError::type_("requiredFeatures must be a sequence"));
  };
  let Ok(iterator) = v8::Local::<v8::Object>::try_from(iterator) else {
    return Err(OpError::type_("requiredFeatures must be a sequence"));
  };
  let next_key = v8::String::new(scope, "next").unwrap();
  let done_key = v8::String::new(scope, "done").unwrap();
  let value_key = v8::String::new(scope, "value").unwrap();
  let mut names = Vec::new();
  loop {
    let Some(next) = iterator.get(scope, next_key.into()) else {
      return Err(OpError::type_("requiredFeatures must be a sequence"));
    };
    let Ok(next) = v8::Local::<v8::Function>::try_from(next) else {
      return Err(OpError::type_("requiredFeatures must be a sequence"));
    };
    let Some(step) = next.call(scope, iterator.into(), &[]) else {
      return Err(OpError::type_("requiredFeatures must be a sequence"));
    };
    let Ok(step) = v8::Local::<v8::Object>::try_from(step) else {
      return Err(OpError::type_("requiredFeatures must be a sequence"));
    };
    let done = step
      .get(scope, done_key.into())
      .is_some_and(|done| done.boolean_value(scope));
    if done {
      return Ok(names);
    }
    if names.len() == 1024 {
      return Err(OpError::type_("requiredFeatures must be a sequence"));
    }
    let Some(item) = step.get(scope, value_key.into()) else {
      return Err(OpError::type_("requiredFeatures must be a sequence"));
    };
    names.push(js::string(scope, item)?);
  }
}

pub(crate) fn dispatch_uncaptured(
  scope: &mut v8::PinScope,
  kind: state::UncapturedKind,
  message: String,
) {
  let ids = state::wrapper_ids();
  if ids.is_empty() {
    eprintln!("yuzuko: uncaptured WebGPU error: {message}");
    return;
  }
  let mut delivered = false;
  for id in ids {
    let Some(target) = state::wrapper_local(scope, id) else {
      continue;
    };
    let error = gpu_error_object(scope, kind, message.clone());
    let event = GpuUncapturedErrorEvent {
      base: Event::new("uncapturederror", false, false, false, true),
      error: yuzuko_core::op::traced(scope, error),
    };
    let object = unsafe { yuzuko_core::op::wrap_instance(scope, event) };
    define_is_trusted(scope, object);
    let _ = dispatch(scope, target, object);
    delivered = true;
  }
  if !delivered {
    eprintln!("yuzuko: uncaptured WebGPU error: {message}");
  }
}

fn gpu_error_object<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  kind: state::UncapturedKind,
  message: String,
) -> v8::Local<'s, v8::Object> {
  unsafe {
    match kind {
      state::UncapturedKind::OutOfMemory => {
        yuzuko_core::op::wrap_instance(scope, GpuOutOfMemoryError { message })
      }
      state::UncapturedKind::Internal => {
        yuzuko_core::op::wrap_instance(scope, GpuInternalError { message })
      }
      state::UncapturedKind::Validation => {
        yuzuko_core::op::wrap_instance(scope, GpuValidationError { message })
      }
    }
  }
}

fn gpu_error_argument<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  args: &v8::FunctionCallbackArguments,
) -> Result<v8::Local<'s, v8::Object>, OpError> {
  if args.length() < 2 {
    return Err(OpError::type_("GPUUncapturedErrorEvent requires an error"));
  }
  let init = args.get(1);
  let Ok(init) = v8::Local::<v8::Object>::try_from(init) else {
    return Err(OpError::type_("GPUUncapturedErrorEvent requires an error"));
  };
  let Some(error) = js::get(scope, init, "error")? else {
    return Err(OpError::type_("GPUUncapturedErrorEvent requires an error"));
  };
  if error.is_null_or_undefined() {
    return Err(OpError::type_("GPUUncapturedErrorEvent requires an error"));
  }
  let Ok(error) = v8::Local::<v8::Object>::try_from(error) else {
    return Err(OpError::type_(
      "GPUUncapturedErrorEvent requires a GPUError",
    ));
  };
  if !is_gpu_error(scope, error) {
    return Err(OpError::type_(
      "GPUUncapturedErrorEvent requires a GPUError",
    ));
  }
  Ok(yuzuko_core::op::root(scope, error))
}

fn is_gpu_error(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
) -> bool {
  unsafe {
    if yuzuko_core::op::host_ref::<GpuValidationError>(scope, object).is_some()
    {
      return true;
    }
    if yuzuko_core::op::host_ref::<GpuOutOfMemoryError>(scope, object).is_some()
    {
      return true;
    }
    yuzuko_core::op::host_ref::<GpuInternalError>(scope, object).is_some()
  }
}
