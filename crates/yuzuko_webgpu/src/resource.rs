//! GPU buffers, textures, shaders, and pipeline objects.

#![allow(clippy::too_many_arguments)]

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use yuzuko_core::op::{Js, New, OpError};
use yuzuko_macros::op;

use crate::js;
use crate::state::Handle;

pub struct GpuBuffer {
  buffer: RefCell<Option<wgpu::Buffer>>,
  lost: Arc<AtomicBool>,
  size: u64,
  mapped: Cell<Option<Vec<u8>>>,
}

unsafe impl v8::cppgc::GarbageCollected for GpuBuffer {
  fn trace(&self, _visitor: &mut v8::cppgc::Visitor) {}

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"GPUBuffer"
  }
}

impl GpuBuffer {
  fn new(buffer: wgpu::Buffer, lost: Arc<AtomicBool>, size: u64) -> Self {
    Self {
      buffer: RefCell::new(Some(buffer)),
      lost,
      size,
      mapped: Cell::new(None),
    }
  }

  pub(crate) fn alive(&self) -> Result<wgpu::Buffer, OpError> {
    if self.lost.load(Ordering::SeqCst) {
      return Err(state_lost());
    }
    self.buffer.borrow().clone().ok_or_else(|| {
      OpError::dom("InvalidStateError", "The buffer is destroyed")
    })
  }
}

#[op(name = "GPUBuffer")]
impl GpuBuffer {
  #[getter]
  fn label(&self) -> String {
    String::new()
  }

  #[getter]
  fn size(&self) -> f64 {
    self.size as f64
  }

  fn destroy(&self) {
    self.mapped.set(None);
    *self.buffer.borrow_mut() = None;
  }

  fn map_async(&self, scope: &mut v8::PinScope, mode: u32) -> Js {
    let mapped = (|| {
      let buffer = self.alive()?;
      let mode = map_mode(mode)?;
      let handle = device_of(&self.lost)?;
      // `writeBuffer` stays in the queue until a submit. Mapping has to see it.
      handle.queue.submit([]);
      let (send, recv) = std::sync::mpsc::channel();
      buffer.slice(..).map_async(mode, move |result| {
        let _ = send.send(result);
      });
      poll_device(&handle.device);
      match recv.recv_timeout(std::time::Duration::from_secs(5)) {
        Ok(Ok(())) => {
          let view = buffer.slice(..).get_mapped_range();
          self.mapped.set(Some(view.to_vec()));
          Ok(())
        }
        _ => Err(OpError::dom("OperationError", "mapAsync failed")),
      }
    })();
    match mapped {
      Ok(()) => {
        let value: v8::Local<v8::Value> = v8::undefined(scope).into();
        js::promise(scope, value)
      }
      Err(error) => js::promise_type_error(scope, &error_text(error)),
    }
  }

  fn get_mapped_range(&self, scope: &mut v8::PinScope) -> Result<Js, OpError> {
    let Some(bytes) = self.mapped.take() else {
      return Err(OpError::dom(
        "InvalidStateError",
        "The buffer is not mapped",
      ));
    };
    self.mapped.set(Some(bytes.clone()));
    let buffer = yuzuko_winter::array_buffer(scope, bytes);
    let value: v8::Local<v8::Value> = buffer.into();
    Ok(Js(v8::Global::new(scope, value)))
  }

  fn unmap(&self) -> Result<(), OpError> {
    self.mapped.set(None);
    if let Some(buffer) = self.buffer.borrow().as_ref() {
      buffer.unmap();
    }
    Ok(())
  }
}

pub struct GpuTexture {
  texture: Rc<RefCell<Option<wgpu::Texture>>>,
  destroyed: Arc<AtomicBool>,
  lost: Arc<AtomicBool>,
  width: u32,
  height: u32,
  format: wgpu::TextureFormat,
}

unsafe impl v8::cppgc::GarbageCollected for GpuTexture {
  fn trace(&self, _visitor: &mut v8::cppgc::Visitor) {}

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"GPUTexture"
  }
}

impl GpuTexture {
  pub fn from_parts(
    texture: wgpu::Texture,
    destroyed: Arc<AtomicBool>,
    lost: Arc<AtomicBool>,
    width: u32,
    height: u32,
    format: wgpu::TextureFormat,
  ) -> Self {
    Self {
      texture: Rc::new(RefCell::new(Some(texture))),
      destroyed,
      lost,
      width,
      height,
      format,
    }
  }

  pub fn texture_slot(&self) -> Rc<RefCell<Option<wgpu::Texture>>> {
    Rc::clone(&self.texture)
  }

  pub fn destroyed_flag(&self) -> Arc<AtomicBool> {
    Arc::clone(&self.destroyed)
  }

  pub fn texture(&self) -> Result<wgpu::Texture, OpError> {
    if self.lost.load(Ordering::SeqCst) || self.destroyed.load(Ordering::SeqCst)
    {
      return Err(OpError::dom(
        "InvalidStateError",
        "The texture is destroyed",
      ));
    }
    self.texture.borrow().clone().ok_or_else(|| {
      OpError::dom("InvalidStateError", "The texture is destroyed")
    })
  }
}

#[op(name = "GPUTexture")]
impl GpuTexture {
  #[getter]
  fn label(&self) -> String {
    String::new()
  }

  #[getter]
  fn format(&self) -> String {
    format_name(self.format).to_string()
  }

  #[getter]
  fn width(&self) -> u32 {
    self.width
  }

  #[getter]
  fn height(&self) -> u32 {
    self.height
  }

  fn create_view(
    &self,
    scope: &mut v8::PinScope,
  ) -> Result<New<GpuTextureView>, OpError> {
    let texture = self.texture()?;
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let _ = scope;
    Ok(New(GpuTextureView {
      view: RefCell::new(Some(view)),
    }))
  }

  fn destroy(&self) {
    self.destroyed.store(true, Ordering::SeqCst);
    *self.texture.borrow_mut() = None;
  }
}

pub struct GpuTextureView {
  view: RefCell<Option<wgpu::TextureView>>,
}

unsafe impl v8::cppgc::GarbageCollected for GpuTextureView {
  fn trace(&self, _visitor: &mut v8::cppgc::Visitor) {}

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"GPUTextureView"
  }
}

impl GpuTextureView {
  pub fn view(&self) -> Result<wgpu::TextureView, OpError> {
    self.view.borrow().clone().ok_or_else(|| {
      OpError::dom("InvalidStateError", "The texture view is destroyed")
    })
  }
}

#[op(name = "GPUTextureView")]
impl GpuTextureView {
  #[getter]
  fn label(&self) -> String {
    String::new()
  }
}

pub struct GpuSampler {
  sampler: wgpu::Sampler,
}

unsafe impl v8::cppgc::GarbageCollected for GpuSampler {
  fn trace(&self, _visitor: &mut v8::cppgc::Visitor) {}

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"GPUSampler"
  }
}

impl GpuSampler {
  pub fn sampler(&self) -> &wgpu::Sampler {
    &self.sampler
  }
}

#[op(name = "GPUSampler")]
impl GpuSampler {
  #[getter]
  fn label(&self) -> String {
    String::new()
  }
}

pub struct GpuShaderModule {
  module: wgpu::ShaderModule,
}

unsafe impl v8::cppgc::GarbageCollected for GpuShaderModule {
  fn trace(&self, _visitor: &mut v8::cppgc::Visitor) {}

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"GPUShaderModule"
  }
}

impl GpuShaderModule {
  pub fn module(&self) -> &wgpu::ShaderModule {
    &self.module
  }
}

#[op(name = "GPUShaderModule")]
impl GpuShaderModule {
  #[getter]
  fn label(&self) -> String {
    String::new()
  }
}

pub struct GpuBindGroupLayout {
  layout: wgpu::BindGroupLayout,
}

unsafe impl v8::cppgc::GarbageCollected for GpuBindGroupLayout {
  fn trace(&self, _visitor: &mut v8::cppgc::Visitor) {}

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"GPUBindGroupLayout"
  }
}

impl GpuBindGroupLayout {
  pub fn layout(&self) -> &wgpu::BindGroupLayout {
    &self.layout
  }
}

#[op(name = "GPUBindGroupLayout")]
impl GpuBindGroupLayout {
  #[getter]
  fn label(&self) -> String {
    String::new()
  }
}

pub struct GpuBindGroup {
  group: wgpu::BindGroup,
}

unsafe impl v8::cppgc::GarbageCollected for GpuBindGroup {
  fn trace(&self, _visitor: &mut v8::cppgc::Visitor) {}

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"GPUBindGroup"
  }
}

impl GpuBindGroup {
  pub fn group(&self) -> &wgpu::BindGroup {
    &self.group
  }
}

#[op(name = "GPUBindGroup")]
impl GpuBindGroup {
  #[getter]
  fn label(&self) -> String {
    String::new()
  }
}

pub struct GpuPipelineLayout {
  layout: wgpu::PipelineLayout,
}

unsafe impl v8::cppgc::GarbageCollected for GpuPipelineLayout {
  fn trace(&self, _visitor: &mut v8::cppgc::Visitor) {}

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"GPUPipelineLayout"
  }
}

impl GpuPipelineLayout {
  pub fn layout(&self) -> &wgpu::PipelineLayout {
    &self.layout
  }
}

#[op(name = "GPUPipelineLayout")]
impl GpuPipelineLayout {
  #[getter]
  fn label(&self) -> String {
    String::new()
  }
}

pub struct GpuRenderPipeline {
  pipeline: wgpu::RenderPipeline,
}

unsafe impl v8::cppgc::GarbageCollected for GpuRenderPipeline {
  fn trace(&self, _visitor: &mut v8::cppgc::Visitor) {}

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"GPURenderPipeline"
  }
}

impl GpuRenderPipeline {
  pub fn pipeline(&self) -> &wgpu::RenderPipeline {
    &self.pipeline
  }
}

#[op(name = "GPURenderPipeline")]
impl GpuRenderPipeline {
  #[getter]
  fn label(&self) -> String {
    String::new()
  }

  fn get_bind_group_layout(&self, index: u32) -> New<GpuBindGroupLayout> {
    let layout = self.pipeline.get_bind_group_layout(index);
    New(GpuBindGroupLayout { layout })
  }
}

pub fn create_buffer(
  scope: &mut v8::PinScope,
  handle: &Handle,
  descriptor: v8::Local<v8::Value>,
) -> Result<New<GpuBuffer>, OpError> {
  let object = js::object(scope, descriptor)
    .ok_or_else(|| OpError::type_("createBuffer requires a descriptor"))?;
  let size = u64::from(js::required_u32(scope, object, "size")?);
  let usage = js::required_u32(scope, object, "usage")?;
  if size == 0 {
    return Err(OpError::range("GPUBuffer size must be non-zero"));
  }
  let label = optional_label(scope, object);
  let buffer = handle.device.create_buffer(&wgpu::BufferDescriptor {
    label: label.as_deref(),
    size,
    usage: wgpu::BufferUsages::from_bits_truncate(usage),
    mapped_at_creation: false,
  });
  Ok(New(GpuBuffer::new(buffer, Arc::clone(&handle.lost), size)))
}

pub fn create_texture(
  scope: &mut v8::PinScope,
  handle: &Handle,
  descriptor: v8::Local<v8::Value>,
) -> Result<New<GpuTexture>, OpError> {
  let object = js::object(scope, descriptor)
    .ok_or_else(|| OpError::type_("createTexture requires a descriptor"))?;
  let (width, height) = texture_size(scope, object)?;
  let format = texture_format(&required_string(scope, object, "format")?)?;
  let usage = js::required_u32(scope, object, "usage")?;
  let label = optional_label(scope, object);
  let texture = handle.device.create_texture(&wgpu::TextureDescriptor {
    label: label.as_deref(),
    size: wgpu::Extent3d {
      width,
      height,
      depth_or_array_layers: 1,
    },
    mip_level_count: 1,
    sample_count: 1,
    dimension: wgpu::TextureDimension::D2,
    format,
    usage: wgpu::TextureUsages::from_bits_truncate(usage),
    view_formats: &[],
  });
  Ok(New(GpuTexture::from_parts(
    texture,
    Arc::new(AtomicBool::new(false)),
    Arc::clone(&handle.lost),
    width,
    height,
    format,
  )))
}

pub fn create_sampler(
  scope: &mut v8::PinScope,
  handle: &Handle,
  descriptor: v8::Local<v8::Value>,
) -> Result<New<GpuSampler>, OpError> {
  let object = js::object(scope, descriptor);
  let mag = filter_of(scope, object, "magFilter")?;
  let min = filter_of(scope, object, "minFilter")?;
  let label = object.and_then(|object| optional_label(scope, object));
  let sampler = handle.device.create_sampler(&wgpu::SamplerDescriptor {
    label: label.as_deref(),
    address_mode_u: wgpu::AddressMode::ClampToEdge,
    address_mode_v: wgpu::AddressMode::ClampToEdge,
    address_mode_w: wgpu::AddressMode::ClampToEdge,
    mag_filter: mag,
    min_filter: min,
    mipmap_filter: wgpu::MipmapFilterMode::Nearest,
    lod_min_clamp: 0.0,
    lod_max_clamp: 32.0,
    compare: None,
    anisotropy_clamp: 1,
    border_color: None,
  });
  Ok(New(GpuSampler { sampler }))
}

pub fn create_shader(
  scope: &mut v8::PinScope,
  handle: &Handle,
  descriptor: v8::Local<v8::Value>,
) -> Result<New<GpuShaderModule>, OpError> {
  let object = js::object(scope, descriptor).ok_or_else(|| {
    OpError::type_("createShaderModule requires a descriptor")
  })?;
  let code = required_string(scope, object, "code")?;
  let label = optional_label(scope, object);
  let module = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
    handle
      .device
      .create_shader_module(wgpu::ShaderModuleDescriptor {
        label: label.as_deref(),
        source: wgpu::ShaderSource::Wgsl(code.into()),
      })
  }));
  let module = module.map_err(|_| {
    OpError::dom("SyntaxError", "The shader module failed to compile")
  })?;
  Ok(New(GpuShaderModule { module }))
}

pub fn create_bind_group_layout(
  scope: &mut v8::PinScope,
  handle: &Handle,
  descriptor: v8::Local<v8::Value>,
) -> Result<New<GpuBindGroupLayout>, OpError> {
  let object = js::object(scope, descriptor).ok_or_else(|| {
    OpError::type_("createBindGroupLayout requires a descriptor")
  })?;
  let entries = binding_layout_entries(scope, object)?;
  let label = optional_label(scope, object);
  let layout =
    handle
      .device
      .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: label.as_deref(),
        entries: &entries,
      });
  Ok(New(GpuBindGroupLayout { layout }))
}

pub fn create_pipeline_layout(
  scope: &mut v8::PinScope,
  handle: &Handle,
  descriptor: v8::Local<v8::Value>,
) -> Result<New<GpuPipelineLayout>, OpError> {
  let object = js::object(scope, descriptor).ok_or_else(|| {
    OpError::type_("createPipelineLayout requires a descriptor")
  })?;
  let layouts = layout_list(scope, object)?;
  let refs: Vec<Option<&wgpu::BindGroupLayout>> =
    layouts.iter().map(|layout| Some(layout.layout())).collect();
  let label = optional_label(scope, object);
  let layout =
    handle
      .device
      .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: label.as_deref(),
        bind_group_layouts: &refs,
        immediate_size: 0,
      });
  Ok(New(GpuPipelineLayout { layout }))
}

pub fn create_bind_group(
  scope: &mut v8::PinScope,
  handle: &Handle,
  descriptor: v8::Local<v8::Value>,
) -> Result<New<GpuBindGroup>, OpError> {
  let object = js::object(scope, descriptor)
    .ok_or_else(|| OpError::type_("createBindGroup requires a descriptor"))?;
  let layout_value = js::get(scope, object, "layout").ok_or_else(|| {
    OpError::type_("GPUBindGroupDescriptor.layout is required")
  })?;
  let layout = host_layout(scope, layout_value)?;
  let built = bind_entries(scope, object)?;
  let entries = built
    .iter()
    .map(|entry| wgpu::BindGroupEntry {
      binding: entry.binding,
      resource: entry.resource(),
    })
    .collect::<Vec<_>>();
  let label = optional_label(scope, object);
  let group = handle.device.create_bind_group(&wgpu::BindGroupDescriptor {
    label: label.as_deref(),
    layout: layout.layout(),
    entries: &entries,
  });
  Ok(New(GpuBindGroup { group }))
}

pub fn create_render_pipeline(
  scope: &mut v8::PinScope,
  handle: &Handle,
  descriptor: v8::Local<v8::Value>,
) -> Result<New<GpuRenderPipeline>, OpError> {
  let object = js::object(scope, descriptor).ok_or_else(|| {
    OpError::type_("createRenderPipeline requires a descriptor")
  })?;
  let vertex = js::get(scope, object, "vertex")
    .and_then(|value| js::object(scope, value))
    .ok_or_else(|| {
      OpError::type_("GPURenderPipelineDescriptor.vertex is required")
    })?;
  let module = shader_of(scope, vertex)?;
  let entry = js::get(scope, vertex, "entryPoint")
    .and_then(|value| js::string(scope, value));
  let attribute_sets = vertex_attributes(scope, vertex)?;
  let layouts = attribute_sets
    .iter()
    .map(|set| wgpu::VertexBufferLayout {
      array_stride: set.stride,
      step_mode: set.step,
      attributes: &set.attributes,
    })
    .collect::<Vec<_>>();
  let fragment = fragment_state(scope, object)?;
  let targets = fragment
    .as_ref()
    .map(|stage| stage.targets.clone())
    .unwrap_or_default();
  let target_refs = targets
    .iter()
    .map(|target| {
      Some(wgpu::ColorTargetState {
        format: *target,
        blend: None,
        write_mask: wgpu::ColorWrites::ALL,
      })
    })
    .collect::<Vec<_>>();
  let fragment_module;
  let fragment_entry;
  let fragment_state = if let Some(stage) = &fragment {
    fragment_module = stage.module.clone();
    fragment_entry = stage.entry.clone();
    Some(wgpu::FragmentState {
      module: &fragment_module,
      entry_point: fragment_entry.as_deref(),
      compilation_options: wgpu::PipelineCompilationOptions::default(),
      targets: &target_refs,
    })
  } else {
    None
  };
  let topology = topology_of(scope, object)?;
  let label = optional_label(scope, object);
  let explicit = explicit_layout(scope, object)?;
  let pipeline = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
    handle
      .device
      .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: label.as_deref(),
        layout: explicit.as_ref().map(GpuPipelineLayout::layout),
        vertex: wgpu::VertexState {
          module: module.module(),
          entry_point: entry.as_deref(),
          compilation_options: wgpu::PipelineCompilationOptions::default(),
          buffers: &layouts,
        },
        primitive: wgpu::PrimitiveState {
          topology,
          ..Default::default()
        },
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        fragment: fragment_state,
        multiview_mask: None,
        cache: None,
      })
  }));
  let pipeline = pipeline.map_err(|_| {
    OpError::dom("OperationError", "The render pipeline is invalid")
  })?;
  Ok(New(GpuRenderPipeline { pipeline }))
}

struct VertexSet {
  stride: u64,
  step: wgpu::VertexStepMode,
  attributes: Vec<wgpu::VertexAttribute>,
}

struct FragmentOwned {
  module: wgpu::ShaderModule,
  entry: Option<String>,
  targets: Vec<wgpu::TextureFormat>,
}

enum OwnedBinding {
  Buffer(wgpu::Buffer, u64, Option<u64>),
  View(wgpu::TextureView),
  Sampler(wgpu::Sampler),
}

struct BuiltEntry {
  binding: u32,
  owned: OwnedBinding,
}

impl BuiltEntry {
  fn resource(&self) -> wgpu::BindingResource<'_> {
    match &self.owned {
      OwnedBinding::Buffer(buffer, offset, size) => {
        wgpu::BindingResource::Buffer(wgpu::BufferBinding {
          buffer,
          offset: *offset,
          size: size.and_then(std::num::NonZeroU64::new),
        })
      }
      OwnedBinding::View(view) => wgpu::BindingResource::TextureView(view),
      OwnedBinding::Sampler(sampler) => wgpu::BindingResource::Sampler(sampler),
    }
  }
}

fn binding_layout_entries(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
) -> Result<Vec<wgpu::BindGroupLayoutEntry>, OpError> {
  let list = array_of(scope, object, "entries")?;
  let mut entries = Vec::new();
  for value in list {
    let entry = js::object(scope, value).ok_or_else(|| {
      OpError::type_("A bind group layout entry must be an object")
    })?;
    let binding = js::required_u32(scope, entry, "binding")?;
    let visibility = js::required_u32(scope, entry, "visibility")?;
    let ty = layout_binding_type(scope, entry)?;
    entries.push(wgpu::BindGroupLayoutEntry {
      binding,
      visibility: wgpu::ShaderStages::from_bits_truncate(visibility),
      ty,
      count: None,
    });
  }
  Ok(entries)
}

fn layout_binding_type(
  scope: &mut v8::PinScope,
  entry: v8::Local<v8::Object>,
) -> Result<wgpu::BindingType, OpError> {
  if js::get(scope, entry, "sampler").is_some() {
    return Ok(wgpu::BindingType::Sampler(
      wgpu::SamplerBindingType::Filtering,
    ));
  }
  if js::get(scope, entry, "texture").is_some() {
    return Ok(wgpu::BindingType::Texture {
      sample_type: wgpu::TextureSampleType::Float { filterable: true },
      view_dimension: wgpu::TextureViewDimension::D2,
      multisampled: false,
    });
  }
  if js::get(scope, entry, "buffer").is_some() {
    return Ok(wgpu::BindingType::Buffer {
      ty: wgpu::BufferBindingType::Uniform,
      has_dynamic_offset: false,
      min_binding_size: None,
    });
  }
  Err(js::unsupported("This bind group layout entry"))
}

fn bind_entries(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
) -> Result<Vec<BuiltEntry>, OpError> {
  let list = array_of(scope, object, "entries")?;
  let mut entries = Vec::new();
  for value in list {
    let entry = js::object(scope, value)
      .ok_or_else(|| OpError::type_("A bind group entry must be an object"))?;
    let binding = js::required_u32(scope, entry, "binding")?;
    let resource = js::get(scope, entry, "resource")
      .ok_or_else(|| OpError::type_("Bind group resource is required"))?;
    entries.push(BuiltEntry {
      binding,
      owned: owned_binding(scope, resource)?,
    });
  }
  Ok(entries)
}

fn owned_binding(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<OwnedBinding, OpError> {
  if let Some(view) = host_view(scope, value) {
    return Ok(OwnedBinding::View(view.view()?));
  }
  if let Some(sampler) = host_sampler(scope, value) {
    return Ok(OwnedBinding::Sampler(sampler.sampler().clone()));
  }
  if let Some(buffer) = host_buffer(scope, value) {
    return Ok(OwnedBinding::Buffer(buffer.alive()?, 0, None));
  }
  let Some(object) = js::object(scope, value) else {
    return Err(OpError::type_("The bind group resource is invalid"));
  };
  let buffer_value = js::get(scope, object, "buffer")
    .ok_or_else(|| OpError::type_("The bind group resource is invalid"))?;
  let buffer = host_buffer(scope, buffer_value)
    .ok_or_else(|| OpError::type_("The bind group resource is not a buffer"))?;
  let offset = u64::from(js::field_u32(scope, object, "offset")?.unwrap_or(0));
  let size = js::field_u32(scope, object, "size")?.map(u64::from);
  Ok(OwnedBinding::Buffer(buffer.alive()?, offset, size))
}

fn vertex_attributes(
  scope: &mut v8::PinScope,
  vertex: v8::Local<v8::Object>,
) -> Result<Vec<VertexSet>, OpError> {
  let Some(buffers) = js::get(scope, vertex, "buffers") else {
    return Ok(Vec::new());
  };
  let Ok(array) = v8::Local::<v8::Array>::try_from(buffers) else {
    return Err(OpError::type_("GPUVertexState.buffers must be an array"));
  };
  let mut sets = Vec::new();
  for index in 0..array.length() {
    let value = array.get_index(scope, index).unwrap();
    if value.is_null_or_undefined() {
      continue;
    }
    let layout = js::object(scope, value).ok_or_else(|| {
      OpError::type_("A vertex buffer layout must be an object")
    })?;
    let stride = u64::from(js::required_u32(scope, layout, "arrayStride")?);
    let step = match js::get(scope, layout, "stepMode")
      .and_then(|value| js::string(scope, value))
      .as_deref()
    {
      None | Some("vertex") => wgpu::VertexStepMode::Vertex,
      Some("instance") => wgpu::VertexStepMode::Instance,
      _ => {
        return Err(OpError::type_("The vertex step mode is invalid"));
      }
    };
    let mut attributes = Vec::new();
    if let Some(list) = js::get(scope, layout, "attributes") {
      let array = v8::Local::<v8::Array>::try_from(list)
        .map_err(|_| OpError::type_("Vertex attributes must be an array"))?;
      for index in 0..array.length() {
        let value = array.get_index(scope, index).unwrap();
        let attribute = js::object(scope, value).ok_or_else(|| {
          OpError::type_("A vertex attribute must be an object")
        })?;
        let format =
          vertex_format(&required_string(scope, attribute, "format")?)?;
        let offset =
          u64::from(js::field_u32(scope, attribute, "offset")?.unwrap_or(0));
        let shader_location =
          js::required_u32(scope, attribute, "shaderLocation")?;
        attributes.push(wgpu::VertexAttribute {
          format,
          offset,
          shader_location,
        });
      }
    }
    sets.push(VertexSet {
      stride,
      step,
      attributes,
    });
  }
  Ok(sets)
}

fn fragment_state(
  scope: &mut v8::PinScope,
  descriptor: v8::Local<v8::Object>,
) -> Result<Option<FragmentOwned>, OpError> {
  let Some(value) = js::get(scope, descriptor, "fragment") else {
    return Ok(None);
  };
  if value.is_null() {
    return Ok(None);
  }
  let fragment = js::object(scope, value)
    .ok_or_else(|| OpError::type_("GPUFragmentState must be an object"))?;
  let module = shader_of(scope, fragment)?;
  let entry = js::get(scope, fragment, "entryPoint")
    .and_then(|value| js::string(scope, value));
  let list = array_of(scope, fragment, "targets")?;
  let mut targets = Vec::new();
  for value in list {
    if value.is_null() {
      continue;
    }
    let target = js::object(scope, value)
      .ok_or_else(|| OpError::type_("A color target must be an object"))?;
    targets.push(texture_format(&required_string(scope, target, "format")?)?);
  }
  Ok(Some(FragmentOwned {
    module: module.module().clone(),
    entry,
    targets,
  }))
}

fn topology_of(
  scope: &mut v8::PinScope,
  descriptor: v8::Local<v8::Object>,
) -> Result<wgpu::PrimitiveTopology, OpError> {
  let Some(primitive) = js::get(scope, descriptor, "primitive") else {
    return Ok(wgpu::PrimitiveTopology::TriangleList);
  };
  let Some(object) = js::object(scope, primitive) else {
    return Ok(wgpu::PrimitiveTopology::TriangleList);
  };
  match js::get(scope, object, "topology")
    .and_then(|value| js::string(scope, value))
    .as_deref()
  {
    None | Some("triangle-list") => Ok(wgpu::PrimitiveTopology::TriangleList),
    Some("triangle-strip") => Ok(wgpu::PrimitiveTopology::TriangleStrip),
    Some("line-list") => Ok(wgpu::PrimitiveTopology::LineList),
    Some("line-strip") => Ok(wgpu::PrimitiveTopology::LineStrip),
    Some("point-list") => Ok(wgpu::PrimitiveTopology::PointList),
    _ => Err(OpError::type_("The primitive topology is invalid")),
  }
}

fn explicit_layout(
  scope: &mut v8::PinScope,
  descriptor: v8::Local<v8::Object>,
) -> Result<Option<GpuPipelineLayout>, OpError> {
  let Some(layout) = js::get(scope, descriptor, "layout") else {
    return Ok(None);
  };
  if layout.is_null() || js::string(scope, layout).as_deref() == Some("auto") {
    return Ok(None);
  }
  host_pipeline_layout(scope, layout).map(Some)
}

fn texture_size(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
) -> Result<(u32, u32), OpError> {
  let size = js::get(scope, object, "size")
    .ok_or_else(|| OpError::type_("GPUTextureDescriptor.size is required"))?;
  if let Ok(array) = v8::Local::<v8::Array>::try_from(size) {
    let width = js::index_u32(scope, array, 0)?;
    let height = if array.length() > 1 {
      js::index_u32(scope, array, 1)?
    } else {
      1
    };
    return Ok((width.max(1), height.max(1)));
  }
  let object = js::object(scope, size)
    .ok_or_else(|| OpError::type_("GPUTextureDescriptor.size is invalid"))?;
  let width = js::required_u32(scope, object, "width")?;
  let height = js::field_u32(scope, object, "height")?.unwrap_or(1);
  Ok((width, height))
}

pub fn texture_format(name: &str) -> Result<wgpu::TextureFormat, OpError> {
  match name {
    "bgra8unorm" => Ok(wgpu::TextureFormat::Bgra8Unorm),
    "rgba8unorm" => Ok(wgpu::TextureFormat::Rgba8Unorm),
    "bgra8unorm-srgb" => Ok(wgpu::TextureFormat::Bgra8UnormSrgb),
    "rgba8unorm-srgb" => Ok(wgpu::TextureFormat::Rgba8UnormSrgb),
    "rgba16float" => Ok(wgpu::TextureFormat::Rgba16Float),
    "r8unorm" => Ok(wgpu::TextureFormat::R8Unorm),
    "rg8unorm" => Ok(wgpu::TextureFormat::Rg8Unorm),
    _ => Err(OpError::type_("The texture format is invalid")),
  }
}

fn format_name(format: wgpu::TextureFormat) -> &'static str {
  match format {
    wgpu::TextureFormat::Bgra8Unorm => "bgra8unorm",
    wgpu::TextureFormat::Rgba8Unorm => "rgba8unorm",
    wgpu::TextureFormat::Bgra8UnormSrgb => "bgra8unorm-srgb",
    wgpu::TextureFormat::Rgba8UnormSrgb => "rgba8unorm-srgb",
    wgpu::TextureFormat::Rgba16Float => "rgba16float",
    wgpu::TextureFormat::R8Unorm => "r8unorm",
    wgpu::TextureFormat::Rg8Unorm => "rg8unorm",
    _ => "rgba8unorm",
  }
}

fn vertex_format(name: &str) -> Result<wgpu::VertexFormat, OpError> {
  match name {
    "float32" => Ok(wgpu::VertexFormat::Float32),
    "float32x2" => Ok(wgpu::VertexFormat::Float32x2),
    "float32x3" => Ok(wgpu::VertexFormat::Float32x3),
    "float32x4" => Ok(wgpu::VertexFormat::Float32x4),
    "uint32" => Ok(wgpu::VertexFormat::Uint32),
    "unorm8x4" => Ok(wgpu::VertexFormat::Unorm8x4),
    _ => Err(OpError::type_("The vertex format is invalid")),
  }
}

fn filter_of(
  scope: &mut v8::PinScope,
  object: Option<v8::Local<v8::Object>>,
  name: &str,
) -> Result<wgpu::FilterMode, OpError> {
  let Some(object) = object else {
    return Ok(wgpu::FilterMode::Nearest);
  };
  match js::get(scope, object, name)
    .and_then(|value| js::string(scope, value))
    .as_deref()
  {
    None | Some("nearest") => Ok(wgpu::FilterMode::Nearest),
    Some("linear") => Ok(wgpu::FilterMode::Linear),
    _ => Err(OpError::type_("The sampler filter is invalid")),
  }
}

fn map_mode(bits: u32) -> Result<wgpu::MapMode, OpError> {
  if bits & 1 != 0 {
    Ok(wgpu::MapMode::Read)
  } else if bits & 2 != 0 {
    Ok(wgpu::MapMode::Write)
  } else {
    Err(OpError::type_("GPUMapMode is invalid"))
  }
}

fn optional_label(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
) -> Option<String> {
  js::get(scope, object, "label").and_then(|value| js::string(scope, value))
}

fn required_string(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
) -> Result<String, OpError> {
  js::get(scope, object, name)
    .and_then(|value| js::string(scope, value))
    .ok_or_else(|| OpError::type_(format!("{name} is required")))
}

fn array_of<'a>(
  scope: &mut v8::PinScope,
  object: v8::Local<'a, v8::Object>,
  name: &str,
) -> Result<Vec<v8::Local<'a, v8::Value>>, OpError> {
  let Some(value) = js::get(scope, object, name) else {
    return Ok(Vec::new());
  };
  let array = v8::Local::<v8::Array>::try_from(value)
    .map_err(|_| OpError::type_(format!("{name} must be an array")))?;
  let mut values = Vec::new();
  for index in 0..array.length() {
    let Some(item) = array.get_index(scope, index) else {
      continue;
    };
    let item: v8::Local<'a, v8::Value> = unsafe {
      std::mem::transmute::<v8::Local<'_, v8::Value>, v8::Local<'a, v8::Value>>(
        item,
      )
    };
    values.push(item);
  }
  Ok(values)
}

fn layout_list(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
) -> Result<Vec<GpuBindGroupLayout>, OpError> {
  let mut layouts = Vec::new();
  for value in array_of(scope, object, "bindGroupLayouts")? {
    layouts.push(host_layout(scope, value)?);
  }
  Ok(layouts)
}

fn shader_of(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
) -> Result<GpuShaderModule, OpError> {
  let value = js::get(scope, object, "module")
    .ok_or_else(|| OpError::type_("A shader module is required"))?;
  host_shader(scope, value)
}

pub fn host_buffer<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  value: v8::Local<v8::Value>,
) -> Option<&'s GpuBuffer> {
  host_of(scope, value)
}

pub fn host_texture<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  value: v8::Local<v8::Value>,
) -> Option<&'s GpuTexture> {
  host_of(scope, value)
}

fn host_view<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  value: v8::Local<v8::Value>,
) -> Option<&'s GpuTextureView> {
  host_of(scope, value)
}

fn host_sampler<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  value: v8::Local<v8::Value>,
) -> Option<&'s GpuSampler> {
  host_of(scope, value)
}

fn host_of<'s, T: 'static>(
  scope: &mut v8::PinScope<'s, '_>,
  value: v8::Local<v8::Value>,
) -> Option<&'s T> {
  let object = v8::Local::<v8::Object>::try_from(value).ok()?;
  let host = unsafe { yuzuko_core::op::host_ref::<T>(scope, object) }?;
  let ptr = host.get() as *const T;
  // `value` stays in this handle scope, so the wrapper outlives `HostRef`.
  let _host = host;
  Some(unsafe { &*ptr })
}

fn host_shader(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<GpuShaderModule, OpError> {
  let object = v8::Local::<v8::Object>::try_from(value).map_err(|_| {
    OpError::type_("The provided value is not a GPUShaderModule")
  })?;
  let host =
    unsafe { yuzuko_core::op::host_ref::<GpuShaderModule>(scope, object) }
      .ok_or_else(|| {
        OpError::type_("The provided value is not a GPUShaderModule")
      })?;
  Ok(GpuShaderModule {
    module: host.get().module().clone(),
  })
}

fn host_layout(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<GpuBindGroupLayout, OpError> {
  let object = v8::Local::<v8::Object>::try_from(value).map_err(|_| {
    OpError::type_("The provided value is not a GPUBindGroupLayout")
  })?;
  let host =
    unsafe { yuzuko_core::op::host_ref::<GpuBindGroupLayout>(scope, object) }
      .ok_or_else(|| {
      OpError::type_("The provided value is not a GPUBindGroupLayout")
    })?;
  Ok(GpuBindGroupLayout {
    layout: host.get().layout().clone(),
  })
}

fn host_pipeline_layout(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<GpuPipelineLayout, OpError> {
  let object = v8::Local::<v8::Object>::try_from(value).map_err(|_| {
    OpError::type_("The provided value is not a GPUPipelineLayout")
  })?;
  let host =
    unsafe { yuzuko_core::op::host_ref::<GpuPipelineLayout>(scope, object) }
      .ok_or_else(|| {
        OpError::type_("The provided value is not a GPUPipelineLayout")
      })?;
  Ok(GpuPipelineLayout {
    layout: host.get().layout().clone(),
  })
}

pub fn host_bind_group<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  value: v8::Local<v8::Value>,
) -> Option<&'s GpuBindGroup> {
  host_of(scope, value)
}

pub fn host_pipeline<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  value: v8::Local<v8::Value>,
) -> Option<&'s GpuRenderPipeline> {
  host_of(scope, value)
}

fn device_of(lost: &AtomicBool) -> Result<Handle, OpError> {
  if lost.load(Ordering::SeqCst) {
    return Err(state_lost());
  }
  yuzuko_window::with_gpu(|_adapter, device, queue| Handle {
    device: device.clone(),
    queue: queue.clone(),
    lost: Arc::new(AtomicBool::new(false)),
  })
  .ok_or_else(state_lost)
}

fn poll_device(device: &wgpu::Device) {
  let _ = device.poll(wgpu::PollType::Wait {
    submission_index: None,
    timeout: Some(std::time::Duration::from_secs(5)),
  });
}

fn state_lost() -> OpError {
  OpError::dom("InvalidStateError", "The GPU device is lost")
}

fn error_text(error: OpError) -> String {
  match error {
    OpError::Type(message)
    | OpError::Range(message)
    | OpError::Dom { message, .. } => message,
    OpError::Thrown(_) => "The operation failed".to_string(),
  }
}
