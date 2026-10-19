//! `navigator.gpu`, adapters, devices, and queues.
//!
//! Every JavaScript device clones the process wgpu device. `destroy` loses
//! that JavaScript device and leaves the process device in place.

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use yuzuko_core::op::{Js, New, OpError};
use yuzuko_macros::op;

use crate::encode::{self, GpuCommandEncoder};
use crate::js;
use crate::resource::{self, GpuBuffer, GpuTexture};
use crate::state::{self, Handle};

pub struct Gpu;

unsafe impl v8::cppgc::GarbageCollected for Gpu {
  fn trace(&self, _visitor: &mut v8::cppgc::Visitor) {}

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"GPU"
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
    if force_fallback(scope, options) || !state::ensure_adapter() {
      return js::promise_null(scope);
    }
    let adapter = unsafe { yuzuko_core::op::wrap_instance(scope, GpuAdapter) };
    let value: v8::Local<v8::Value> = adapter.into();
    js::promise(scope, value)
  }

  fn get_preferred_canvas_format(&self) -> Result<String, OpError> {
    preferred_format().map(str::to_string)
  }

  #[getter]
  fn wgsl_language_features(&self, scope: &mut v8::PinScope) -> Js {
    let set = v8::Set::new(scope);
    let value: v8::Local<v8::Value> = set.into();
    Js(v8::Global::new(scope, value))
  }
}

pub struct GpuAdapter;

unsafe impl v8::cppgc::GarbageCollected for GpuAdapter {
  fn trace(&self, _visitor: &mut v8::cppgc::Visitor) {}

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"GPUAdapter"
  }
}

#[op(name = "GPUAdapter")]
impl GpuAdapter {
  #[getter]
  fn features(&self, scope: &mut v8::PinScope) -> Result<Js, OpError> {
    let features = gpu_parts()?.0.features();
    Ok(features_js(scope, features))
  }

  #[getter]
  fn limits(&self, scope: &mut v8::PinScope) -> Result<Js, OpError> {
    let limits = gpu_parts()?.0.limits();
    Ok(limits_js(scope, &limits))
  }

  #[getter]
  fn is_fallback_adapter(&self) -> bool {
    false
  }

  fn request_device(
    &self,
    scope: &mut v8::PinScope,
    descriptor: Option<v8::Local<v8::Value>>,
  ) -> Js {
    match open_device(scope, descriptor) {
      Ok(device) => {
        let object = unsafe { yuzuko_core::op::wrap_instance(scope, device) };
        let value: v8::Local<v8::Value> = object.into();
        js::promise(scope, value)
      }
      Err(error) => js::promise_rejected(scope, error),
    }
  }
}

pub struct GpuDevice {
  id: u64,
}

unsafe impl v8::cppgc::GarbageCollected for GpuDevice {
  fn trace(&self, _visitor: &mut v8::cppgc::Visitor) {}

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

#[op(name = "GPUDevice")]
impl GpuDevice {
  #[getter]
  fn label(&self) -> String {
    String::new()
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
    let features = self.require()?.device.features();
    Ok(features_js(scope, features))
  }

  #[getter]
  fn limits(&self, scope: &mut v8::PinScope) -> Result<Js, OpError> {
    let limits = self.require()?.device.limits();
    Ok(limits_js(scope, &limits))
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
    state::destroy(scope, self.id);
    Ok(())
  }

  fn create_buffer(
    &self,
    scope: &mut v8::PinScope,
    descriptor: v8::Local<v8::Value>,
  ) -> Result<New<GpuBuffer>, OpError> {
    resource::create_buffer(scope, &self.require()?, descriptor)
  }

  fn create_texture(
    &self,
    scope: &mut v8::PinScope,
    descriptor: v8::Local<v8::Value>,
  ) -> Result<New<GpuTexture>, OpError> {
    resource::create_texture(scope, &self.require()?, descriptor)
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
    resource::create_shader(scope, &self.require()?, descriptor)
  }

  fn create_bind_group_layout(
    &self,
    scope: &mut v8::PinScope,
    descriptor: v8::Local<v8::Value>,
  ) -> Result<New<resource::GpuBindGroupLayout>, OpError> {
    resource::create_bind_group_layout(scope, &self.require()?, descriptor)
  }

  fn create_pipeline_layout(
    &self,
    scope: &mut v8::PinScope,
    descriptor: v8::Local<v8::Value>,
  ) -> Result<New<resource::GpuPipelineLayout>, OpError> {
    resource::create_pipeline_layout(scope, &self.require()?, descriptor)
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
    resource::create_render_pipeline(scope, &self.require()?, descriptor)
  }

  fn create_command_encoder(
    &self,
    scope: &mut v8::PinScope,
    descriptor: Option<v8::Local<v8::Value>>,
  ) -> Result<New<GpuCommandEncoder>, OpError> {
    let handle = self.require()?;
    let label = descriptor.and_then(|value| {
      js::object(scope, value)
        .and_then(|object| js::get(scope, object, "label"))
        .and_then(|value| js::string(scope, value))
    });
    Ok(New(GpuCommandEncoder::create(&handle, label)))
  }

  fn create_render_bundle_encoder(&self) -> Result<(), OpError> {
    Err(js::unsupported("GPURenderBundleEncoder"))
  }

  fn create_compute_pipeline(&self) -> Result<(), OpError> {
    Err(js::unsupported("GPUComputePipeline"))
  }

  fn create_compute_pipeline_async(&self) -> Result<(), OpError> {
    Err(js::unsupported("GPUComputePipeline"))
  }

  fn create_query_set(&self) -> Result<(), OpError> {
    Err(js::unsupported("GPUQuerySet"))
  }

  fn push_error_scope(&self, filter: String) -> Result<(), OpError> {
    let filter = match filter.as_str() {
      "validation" => wgpu::ErrorFilter::Validation,
      "out-of-memory" => wgpu::ErrorFilter::OutOfMemory,
      "internal" => wgpu::ErrorFilter::Internal,
      _ => return Err(OpError::type_("The error filter is invalid")),
    };
    let handle = self.require()?;
    let guard = handle.device.push_error_scope(filter);
    state::push_scope(self.id, guard)
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
    String::new()
  }

  fn write_buffer(
    &self,
    scope: &mut v8::PinScope,
    buffer: v8::Local<v8::Value>,
    offset: u32,
    data: v8::Local<v8::Value>,
    data_offset: Option<u32>,
    size: Option<u32>,
  ) -> Result<(), OpError> {
    let buffer = resource::host_buffer(scope, buffer)
      .ok_or_else(|| OpError::type_("writeBuffer requires a GPUBuffer"))?;
    let buffer = buffer.alive()?;
    let bytes = yuzuko_winter::buffer_source(scope, data).ok_or_else(|| {
      OpError::type_("writeBuffer data must be a BufferSource")
    })?;
    let start = data_offset.unwrap_or(0) as usize;
    if start > bytes.len() {
      return Err(OpError::range("The buffer offset is out of range"));
    }
    let end = match size {
      Some(size) => start.saturating_add(size as usize),
      None => bytes.len(),
    };
    if end > bytes.len() {
      return Err(OpError::range("The buffer size is out of range"));
    }
    let handle = state::handle(self.device_id)?;
    handle
      .queue
      .write_buffer(&buffer, u64::from(offset), &bytes[start..end]);
    Ok(())
  }

  fn submit(
    &self,
    scope: &mut v8::PinScope,
    buffers: v8::Local<v8::Value>,
  ) -> Result<(), OpError> {
    let buffers = encode::command_buffers(scope, buffers)?;
    let handle = state::handle(self.device_id)?;
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

pub fn install(scope: &mut v8::PinScope) {
  Gpu::install(scope);
  GpuAdapter::install(scope);
  GpuDevice::install(scope);
  GpuQueue::install(scope);
  GpuValidationError::install(scope);
  GpuOutOfMemoryError::install(scope);
  GpuInternalError::install(scope);
  GpuBuffer::install(scope);
  GpuTexture::install(scope);
  resource::GpuTextureView::install(scope);
  resource::GpuSampler::install(scope);
  resource::GpuShaderModule::install(scope);
  resource::GpuBindGroupLayout::install(scope);
  resource::GpuBindGroup::install(scope);
  resource::GpuPipelineLayout::install(scope);
  resource::GpuRenderPipeline::install(scope);
  GpuCommandEncoder::install(scope);
  encode::GpuCommandBuffer::install(scope);
  encode::GpuRenderPass::install(scope);
  crate::canvas::GpuCanvasContext::install(scope);
  install_flags(scope);
  install_navigator(scope);
}

fn install_navigator(scope: &mut v8::PinScope) {
  let gpu = unsafe { yuzuko_core::op::wrap_instance(scope, Gpu) };
  let global = scope.get_current_context().global(scope);
  let nav_key = v8::String::new(scope, "navigator").unwrap();
  let Some(navigator) = global.get(scope, nav_key.into()) else {
    return;
  };
  let Ok(navigator) = v8::Local::<v8::Object>::try_from(navigator) else {
    return;
  };
  let key = v8::String::new(scope, "gpu").unwrap();
  navigator.set(scope, key.into(), gpu.into());
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
  let (device, queue, limits) =
    gpu_parts().map(|(_adapter, device, queue)| {
      let limits = device.limits();
      (device, queue, limits)
    })?;
  if let Some(object) = dictionary(scope, descriptor) {
    reject_features(scope, object)?;
    check_required_limits(scope, object, &limits)?;
  }
  let id = state::alloc_id();
  let lost = Arc::new(AtomicBool::new(false));
  let resolver = v8::PromiseResolver::new(scope).unwrap();
  let promise = resolver.get_promise(scope);
  let queue_object = unsafe {
    yuzuko_core::op::wrap_instance(scope, GpuQueue { device_id: id })
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
    },
  );
  Ok(GpuDevice { id })
}

fn pop_scope<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  id: u64,
) -> Result<v8::Local<'s, v8::Value>, OpError> {
  let guard = state::pop_scope(id)?;
  let handle = state::handle(id)?;
  let _ = handle.device.poll(wgpu::PollType::Wait {
    submission_index: None,
    timeout: Some(std::time::Duration::from_secs(5)),
  });
  let error = pollster::block_on(guard.pop());
  let Some(error) = error else {
    return Ok(v8::null(scope).into());
  };
  let message = error.to_string();
  let object = match error {
    wgpu::Error::OutOfMemory { .. } => unsafe {
      yuzuko_core::op::wrap_instance(scope, GpuOutOfMemoryError { message })
    },
    wgpu::Error::Internal { .. } => unsafe {
      yuzuko_core::op::wrap_instance(scope, GpuInternalError { message })
    },
    wgpu::Error::Validation { .. } => unsafe {
      yuzuko_core::op::wrap_instance(scope, GpuValidationError { message })
    },
  };
  Ok(object.into())
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

fn force_fallback(
  scope: &mut v8::PinScope,
  options: Option<v8::Local<v8::Value>>,
) -> bool {
  let Some(object) = dictionary(scope, options) else {
    return false;
  };
  js::get(scope, object, "forceFallbackAdapter")
    .is_some_and(|value| value.boolean_value(scope))
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
) -> Result<(), OpError> {
  let Some(value) = js::get(scope, descriptor, "requiredFeatures") else {
    return Ok(());
  };
  if value.is_null() {
    return Ok(());
  }
  let array = v8::Local::<v8::Array>::try_from(value)
    .map_err(|_| OpError::type_("requiredFeatures must be a sequence"))?;
  if array.length() == 0 {
    return Ok(());
  }
  Err(OpError::type_(
    "Required features are not enabled on the shared device",
  ))
}

fn check_required_limits(
  scope: &mut v8::PinScope,
  descriptor: v8::Local<v8::Object>,
  limits: &wgpu::Limits,
) -> Result<(), OpError> {
  let Some(value) = js::get(scope, descriptor, "requiredLimits") else {
    return Ok(());
  };
  let Some(requested) = js::object(scope, value) else {
    return Err(OpError::type_("requiredLimits must be an object"));
  };
  for (name, actual, lower_is_better) in limit_table(limits) {
    let Some(value) = js::get(scope, requested, name) else {
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
      "maxStorageTexturesPerShaderStage",
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
  limits: &wgpu::Limits,
) -> v8::Local<'s, v8::Object> {
  let object = v8::Object::new(scope);
  for (name, value, _) in limit_table(limits) {
    let key = v8::String::new(scope, name).unwrap();
    let number = v8::Number::new(scope, value);
    object.set(scope, key.into(), number.into());
  }
  object
}

fn features_js(scope: &mut v8::PinScope, features: wgpu::Features) -> Js {
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
  let value: v8::Local<v8::Value> = set.into();
  Js(v8::Global::new(scope, value))
}

fn limits_js(scope: &mut v8::PinScope, limits: &wgpu::Limits) -> Js {
  let object = limits_object(scope, limits);
  let value: v8::Local<v8::Value> = object.into();
  Js(v8::Global::new(scope, value))
}
