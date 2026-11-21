//! GPU buffers, textures, shaders, and pipeline objects.

#![allow(clippy::too_many_arguments)]

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use yuzuko_core::op::{Js, New, OpError};
use yuzuko_macros::op;

use crate::js;
use crate::label::ObjectLabel;
use crate::state::Handle;

struct MappedView {
  offset: u64,
  size: u64,
  buffer: v8::Global<v8::ArrayBuffer>,
}

struct Mapped {
  offset: u64,
  size: u64,
  /// `GPUMapMode` bits. Read is 1 and write is 2.
  mode: u32,
  bytes: Vec<u8>,
  views: Vec<MappedView>,
  /// The wgpu buffer is mapped, so `unmap` must call `Buffer::unmap`.
  gpu_mapped: bool,
}

pub struct GpuBuffer {
  buffer: RefCell<Option<wgpu::Buffer>>,
  lost: Arc<AtomicBool>,
  device_id: u64,
  size: u64,
  /// Usage bits passed to `createBuffer`, kept after `destroy`.
  usage: u32,
  valid: bool,
  destroyed: Cell<bool>,
  mapped: RefCell<Option<Mapped>>,
  label: ObjectLabel,
}

unsafe impl v8::cppgc::GarbageCollected for GpuBuffer {
  fn trace(&self, _visitor: &mut v8::cppgc::Visitor) {}

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"GPUBuffer"
  }
}

impl GpuBuffer {
  fn new(
    buffer: wgpu::Buffer,
    lost: Arc<AtomicBool>,
    device_id: u64,
    size: u64,
    label: Option<String>,
    usage: u32,
    valid: bool,
  ) -> Self {
    Self {
      buffer: RefCell::new(Some(buffer)),
      lost,
      device_id,
      size,
      usage,
      valid,
      destroyed: Cell::new(false),
      mapped: RefCell::new(None),
      label: ObjectLabel::new(label),
    }
  }

  fn release_mapping(&self, scope: &mut v8::PinScope, writeback: bool) {
    let Some(mut mapped) = self.mapped.borrow_mut().take() else {
      return;
    };
    if writeback {
      absorb_views(scope, &mut mapped);
    }
    let Some(buffer) = self.buffer.borrow().clone() else {
      detach_views(scope, &mapped);
      return;
    };
    let read = (mapped.mode & MAP_READ_BIT) != 0;
    let write = (mapped.mode & MAP_WRITE_BIT) != 0;
    if writeback && write && !read && mapped.size > 0 {
      let end = mapped.offset + mapped.size;
      if let Ok(mut view) =
        buffer.slice(mapped.offset..end).get_mapped_range_mut()
      {
        view.copy_from_slice(&mapped.bytes);
      }
    }
    if mapped.gpu_mapped {
      buffer.unmap();
    }
    if writeback
      && read
      && write
      && mapped.size > 0
      && buffer.usage().contains(wgpu::BufferUsages::COPY_DST)
      && let Ok(handle) = device_of(&self.lost)
    {
      handle
        .queue
        .write_buffer(&buffer, mapped.offset, &mapped.bytes);
    }
    detach_views(scope, &mapped);
  }

  pub(crate) fn alive(&self) -> Result<wgpu::Buffer, OpError> {
    if self.lost.load(Ordering::SeqCst) {
      return Err(state_lost());
    }
    self.buffer.borrow().clone().ok_or_else(|| {
      OpError::dom("InvalidStateError", "The buffer is destroyed")
    })
  }

  pub(crate) fn is_destroyed(&self) -> bool {
    self.destroyed.get()
  }

  pub(crate) fn is_valid(&self) -> bool {
    self.valid
  }

  pub(crate) fn byte_size(&self) -> u64 {
    self.size
  }

  pub(crate) fn usage_bits(&self) -> u32 {
    self.usage
  }

  pub(crate) fn device_id(&self) -> u64 {
    self.device_id
  }

  fn fail_map(&self, message: &str) -> OpError {
    crate::state::report_validation(self.device_id, message);
    OpError::dom("OperationError", message.to_string())
  }
}

#[op(name = "GPUBuffer")]
impl GpuBuffer {
  #[getter]
  fn label(&self) -> String {
    self.label.get()
  }

  #[setter]
  fn label(&self, value: String) {
    self.label.set(value);
  }

  #[getter]
  fn size(&self) -> f64 {
    self.size as f64
  }

  #[getter]
  fn map_state(&self) -> String {
    if self.mapped.borrow().is_some() {
      "mapped".to_string()
    } else {
      "unmapped".to_string()
    }
  }

  #[getter]
  fn usage(&self) -> u32 {
    self.usage
  }

  fn destroy(&self, scope: &mut v8::PinScope) {
    self.release_mapping(scope, false);
    if !self.destroyed.replace(true)
      && let Some(buffer) = self.buffer.borrow().as_ref()
    {
      buffer.destroy();
    }
  }

  fn map_async(
    &self,
    scope: &mut v8::PinScope,
    mode: u32,
    offset: Option<js::GpuSize64>,
    size: Option<js::GpuSize64>,
  ) -> Js {
    let mapped = (|| {
      if self.lost.load(Ordering::SeqCst) {
        return Err(OpError::dom("AbortError", "The GPU device is lost"));
      }
      if (mode & !MAP_MODE_MASK) != 0 || (mode & MAP_MODE_MASK) == 0 {
        return Err(OpError::type_("GPUMapMode is invalid"));
      }
      if self.destroyed.get() {
        return Err(self.fail_map("The buffer is destroyed"));
      }
      if self.mapped.borrow().is_some() {
        return Err(self.fail_map("The buffer is already mapped"));
      }
      if !self.valid {
        return Err(self.fail_map("The buffer is invalid"));
      }
      let buffer = self.alive()?;
      let usage = self.usage;
      if (mode & MAP_READ_BIT) != 0 && (usage & MAP_READ_BIT) == 0 {
        return Err(self.fail_map("The buffer is not mappable for reading"));
      }
      if (mode & MAP_WRITE_BIT) != 0 && (usage & MAP_WRITE_BIT) == 0 {
        return Err(self.fail_map("The buffer is not mappable for writing"));
      }
      let offset = offset.map(|value| value.0).unwrap_or(0);
      let size =
        match checked_map_size(self.size, offset, size.map(|value| value.0)) {
          Ok(size) => size,
          Err(error) => {
            crate::state::report_validation(
              self.device_id,
              "The map range is outside the buffer",
            );
            return Err(error);
          }
        };
      let end = offset + size;
      let host_mode = if (mode & MAP_READ_BIT) != 0 {
        wgpu::MapMode::Read
      } else {
        wgpu::MapMode::Write
      };
      let handle = device_of(&self.lost)?;
      // `writeBuffer` stays in the queue until a submit. Mapping has to see it.
      handle.queue.submit([]);
      let (send, recv) = std::sync::mpsc::channel();
      buffer
        .slice(offset..end)
        .map_async(host_mode, move |result| {
          let _ = send.send(result);
        });
      poll_device(&handle.device);
      match recv.recv_timeout(std::time::Duration::from_secs(5)) {
        Ok(Ok(())) => {
          let bytes = match buffer.slice(offset..end).get_mapped_range() {
            Ok(view) => view.to_vec(),
            Err(_) => {
              buffer.unmap();
              return Err(self.fail_map("mapAsync failed"));
            }
          };
          *self.mapped.borrow_mut() = Some(Mapped {
            offset,
            size,
            mode,
            bytes,
            views: Vec::new(),
            gpu_mapped: true,
          });
          Ok(())
        }
        _ => Err(self.fail_map("mapAsync failed")),
      }
    })();
    match mapped {
      Ok(()) => {
        let value: v8::Local<v8::Value> = v8::undefined(scope).into();
        js::promise(scope, value)
      }
      Err(error) => js::promise_rejected(scope, error),
    }
  }

  fn get_mapped_range(
    &self,
    scope: &mut v8::PinScope,
    offset: Option<js::GpuSize64>,
    size: Option<js::GpuSize64>,
  ) -> Result<Js, OpError> {
    let mut slot = self.mapped.borrow_mut();
    let Some(mapped) = slot.as_mut() else {
      return Err(OpError::dom("OperationError", "The buffer is not mapped"));
    };
    let offset = offset.map(|value| value.0).unwrap_or(0);
    if !offset.is_multiple_of(8) {
      return Err(OpError::dom(
        "OperationError",
        "The mapped range offset must be a multiple of 8",
      ));
    }
    if offset > mapped.size {
      return Err(OpError::dom(
        "OperationError",
        "The mapped range is outside the mapping",
      ));
    }
    let size = match size {
      Some(size) => size.0,
      None => mapped.size - offset,
    };
    let end = offset.saturating_add(size);
    if !size.is_multiple_of(4) || end > mapped.size {
      return Err(OpError::dom(
        "OperationError",
        "The mapped range is outside the mapping",
      ));
    }
    for view in &mapped.views {
      let view_end = view.offset + view.size;
      if offset < view_end && view.offset < end {
        return Err(OpError::dom(
          "OperationError",
          "The mapped ranges overlap",
        ));
      }
    }
    let bytes = if size == 0 {
      Vec::new()
    } else {
      let start = offset as usize;
      mapped.bytes[start..start + size as usize].to_vec()
    };
    let buffer = yuzuko_winter::array_buffer(scope, bytes);
    mapped.views.push(MappedView {
      offset,
      size,
      buffer: v8::Global::new(scope, buffer),
    });
    let value: v8::Local<v8::Value> = buffer.into();
    Ok(Js(v8::Global::new(scope, value)))
  }

  fn unmap(&self, scope: &mut v8::PinScope) -> Result<(), OpError> {
    self.release_mapping(scope, true);
    Ok(())
  }
}

impl Drop for GpuBuffer {
  fn drop(&mut self) {
    let mapped = self.mapped.borrow_mut().take();
    if mapped.is_some_and(|mapped| mapped.gpu_mapped)
      && let Some(buffer) = self.buffer.borrow().as_ref()
    {
      buffer.unmap();
    }
  }
}

pub struct GpuTexture {
  texture: Rc<RefCell<Option<wgpu::Texture>>>,
  destroyed: Arc<AtomicBool>,
  lost: Arc<AtomicBool>,
  width: u32,
  height: u32,
  format: wgpu::TextureFormat,
  sample_count: u32,
  /// Usage bits of the texture that was created, kept after `destroy`.
  usage: u32,
  label: ObjectLabel,
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
    label: Option<String>,
    sample_count: u32,
    usage: u32,
  ) -> Self {
    Self {
      texture: Rc::new(RefCell::new(Some(texture))),
      destroyed,
      lost,
      width,
      height,
      format,
      sample_count,
      usage,
      label: ObjectLabel::new(label),
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
    self.label.get()
  }

  #[setter]
  fn label(&self, value: String) {
    self.label.set(value);
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

  #[getter]
  fn depth_or_array_layers(&self) -> u32 {
    1
  }

  #[getter]
  fn dimension(&self) -> String {
    "2d".to_string()
  }

  #[getter]
  fn mip_level_count(&self) -> u32 {
    1
  }

  #[getter]
  fn sample_count(&self) -> u32 {
    self.sample_count
  }

  #[getter]
  fn usage(&self) -> u32 {
    self.usage
  }

  fn create_view(
    &self,
    scope: &mut v8::PinScope,
    descriptor: Option<v8::Local<v8::Value>>,
  ) -> Result<New<GpuTextureView>, OpError> {
    let parts = match descriptor {
      Some(value) if !value.is_null_or_undefined() => {
        texture_view_parts(scope, value)?
      }
      _ => TextureViewParts::default(),
    };
    let texture = self.texture()?;
    let format = match parts.format {
      Some(view_format) => {
        let features = device_of(&self.lost)?.device.features();
        require_texture_format(features, view_format)?
      }
      None => self.format,
    };
    let view = texture.create_view(&wgpu::TextureViewDescriptor {
      label: parts.label.as_deref(),
      format: parts.format.map(|_| format),
      dimension: parts.dimension,
      usage: None,
      aspect: parts.aspect,
      base_mip_level: parts.base_mip_level,
      mip_level_count: parts.mip_level_count,
      base_array_layer: parts.base_array_layer,
      array_layer_count: parts.array_layer_count,
    });
    Ok(New(GpuTextureView {
      view: RefCell::new(Some(view)),
      format,
      label: ObjectLabel::new(parts.label),
    }))
  }

  fn destroy(&self) {
    self.destroyed.store(true, Ordering::SeqCst);
    *self.texture.borrow_mut() = None;
  }
}

pub struct GpuTextureView {
  view: RefCell<Option<wgpu::TextureView>>,
  format: wgpu::TextureFormat,
  label: ObjectLabel,
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

  pub fn format(&self) -> wgpu::TextureFormat {
    self.format
  }
}

#[op(name = "GPUTextureView")]
impl GpuTextureView {
  #[getter]
  fn label(&self) -> String {
    self.label.get()
  }

  #[setter]
  fn label(&self, value: String) {
    self.label.set(value);
  }
}

pub struct GpuSampler {
  sampler: wgpu::Sampler,
  label: ObjectLabel,
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
    self.label.get()
  }

  #[setter]
  fn label(&self, value: String) {
    self.label.set(value);
  }
}

pub struct GpuShaderModule {
  module: wgpu::ShaderModule,
  label: ObjectLabel,
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
    self.label.get()
  }

  #[setter]
  fn label(&self, value: String) {
    self.label.set(value);
  }

  fn get_compilation_info(&self, scope: &mut v8::PinScope) -> Js {
    js::promise_rejected(
      scope,
      js::unsupported("GPUShaderModule.getCompilationInfo"),
    )
  }
}

pub struct GpuBindGroupLayout {
  layout: wgpu::BindGroupLayout,
  label: ObjectLabel,
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
    self.label.get()
  }

  #[setter]
  fn label(&self, value: String) {
    self.label.set(value);
  }
}

pub struct GpuBindGroup {
  group: wgpu::BindGroup,
  label: ObjectLabel,
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
    self.label.get()
  }

  #[setter]
  fn label(&self, value: String) {
    self.label.set(value);
  }
}

pub struct GpuPipelineLayout {
  layout: wgpu::PipelineLayout,
  label: ObjectLabel,
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
    self.label.get()
  }

  #[setter]
  fn label(&self, value: String) {
    self.label.set(value);
  }
}

pub struct GpuRenderPipeline {
  pipeline: wgpu::RenderPipeline,
  label: ObjectLabel,
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
    self.label.get()
  }

  #[setter]
  fn label(&self, value: String) {
    self.label.set(value);
  }

  fn get_bind_group_layout(&self, index: u32) -> New<GpuBindGroupLayout> {
    let layout = self.pipeline.get_bind_group_layout(index);
    New(GpuBindGroupLayout {
      layout,
      label: ObjectLabel::new(None),
    })
  }
}

pub struct GpuComputePipeline {
  pipeline: wgpu::ComputePipeline,
  label: ObjectLabel,
}

unsafe impl v8::cppgc::GarbageCollected for GpuComputePipeline {
  fn trace(&self, _visitor: &mut v8::cppgc::Visitor) {}

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"GPUComputePipeline"
  }
}

impl GpuComputePipeline {
  pub fn pipeline(&self) -> &wgpu::ComputePipeline {
    &self.pipeline
  }
}

#[op(name = "GPUComputePipeline")]
impl GpuComputePipeline {
  #[getter]
  fn label(&self) -> String {
    self.label.get()
  }

  #[setter]
  fn label(&self, value: String) {
    self.label.set(value);
  }

  fn get_bind_group_layout(&self, index: u32) -> New<GpuBindGroupLayout> {
    let layout = self.pipeline.get_bind_group_layout(index);
    New(GpuBindGroupLayout {
      layout,
      label: ObjectLabel::new(None),
    })
  }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum QueryKind {
  Occlusion,
  Timestamp,
}

pub struct GpuQuerySet {
  set: wgpu::QuerySet,
  kind: QueryKind,
  count: u32,
  label: ObjectLabel,
  /// wgpu 29 has no `QuerySet::destroy`. A later resolve is recorded out of
  /// range so the error scope still reports a validation error.
  destroyed: Cell<bool>,
}

unsafe impl v8::cppgc::GarbageCollected for GpuQuerySet {
  fn trace(&self, _visitor: &mut v8::cppgc::Visitor) {}

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"GPUQuerySet"
  }
}

impl GpuQuerySet {
  pub fn query_set(&self) -> wgpu::QuerySet {
    self.set.clone()
  }

  pub fn kind(&self) -> QueryKind {
    self.kind
  }

  pub fn is_destroyed(&self) -> bool {
    self.destroyed.get()
  }

  pub fn query_count(&self) -> u32 {
    self.count
  }
}

#[op(name = "GPUQuerySet")]
impl GpuQuerySet {
  #[getter]
  fn label(&self) -> String {
    self.label.get()
  }

  #[setter]
  fn label(&self, value: String) {
    self.label.set(value);
  }

  #[getter]
  fn count(&self) -> u32 {
    self.count
  }

  #[getter]
  #[rename("type")]
  fn query_type(&self) -> String {
    match self.kind {
      QueryKind::Occlusion => "occlusion".to_string(),
      QueryKind::Timestamp => "timestamp".to_string(),
    }
  }

  fn destroy(&self) {
    self.destroyed.set(true);
  }
}

pub struct GpuExternalTexture {
  external: wgpu::ExternalTexture,
  /// The external texture stores this view's id. Dropping the view removes
  /// that id, so the plane and the view stay alive with the JavaScript object.
  _plane: wgpu::Texture,
  _view: wgpu::TextureView,
  label: ObjectLabel,
}

unsafe impl v8::cppgc::GarbageCollected for GpuExternalTexture {
  fn trace(&self, _visitor: &mut v8::cppgc::Visitor) {}

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"GPUExternalTexture"
  }
}

impl GpuExternalTexture {
  pub(crate) fn new(
    external: wgpu::ExternalTexture,
    plane: wgpu::Texture,
    view: wgpu::TextureView,
    label: Option<String>,
  ) -> Self {
    Self {
      external,
      _plane: plane,
      _view: view,
      label: ObjectLabel::new(label),
    }
  }

  pub fn external(&self) -> &wgpu::ExternalTexture {
    &self.external
  }
}

#[op(name = "GPUExternalTexture")]
impl GpuExternalTexture {
  #[getter]
  fn label(&self) -> String {
    self.label.get()
  }

  #[setter]
  fn label(&self, value: String) {
    self.label.set(value);
  }
}

pub fn create_buffer(
  scope: &mut v8::PinScope,
  device_id: u64,
  handle: &Handle,
  descriptor: v8::Local<v8::Value>,
) -> Result<New<GpuBuffer>, OpError> {
  let object = js::object(scope, descriptor)
    .ok_or_else(|| OpError::type_("createBuffer requires a descriptor"))?;
  let size = js::required_u64(scope, object, "size")?;
  let raw_usage = js::required_u32(scope, object, "usage")?;
  let mapped_at_creation = match js::get(scope, object, "mappedAtCreation")? {
    Some(value) => value.boolean_value(scope),
    None => false,
  };
  if mapped_at_creation && !size.is_multiple_of(4) {
    return Err(OpError::range(
      "GPUBuffer size must be a multiple of 4 when mappedAtCreation is true",
    ));
  }
  let unknown = raw_usage & !BUFFER_USAGE_MASK != 0;
  let usage = if raw_usage == 0 || unknown {
    wgpu::BufferUsages::empty()
  } else {
    wgpu::BufferUsages::from_bits_truncate(raw_usage)
  };
  let over_max = size > handle.device.limits().max_buffer_size;
  let valid = raw_usage != 0
    && !unknown
    && !buffer_usage_mismatch(raw_usage)
    && !over_max;
  let label = optional_label(scope, object)?;
  // A buffer above `maxBufferSize` is invalid. Do not ask wgpu to map it.
  let host_map = mapped_at_creation && !over_max;
  let buffer = handle.device.create_buffer(&wgpu::BufferDescriptor {
    label: label.as_deref(),
    size,
    usage,
    mapped_at_creation: host_map,
  });
  let gpu = GpuBuffer::new(
    buffer,
    Arc::clone(&handle.lost),
    device_id,
    size,
    label,
    raw_usage,
    valid,
  );
  if host_map && let Some(buffer) = gpu.buffer.borrow().clone() {
    let (bytes, gpu_mapped) = mapped_creation_bytes(&buffer, size);
    *gpu.mapped.borrow_mut() = Some(Mapped {
      offset: 0,
      size,
      mode: MAP_WRITE_BIT,
      bytes,
      views: Vec::new(),
      gpu_mapped,
    });
  }
  Ok(New(gpu))
}

pub fn create_texture(
  scope: &mut v8::PinScope,
  handle: &Handle,
  descriptor: v8::Local<v8::Value>,
) -> Result<New<GpuTexture>, OpError> {
  let object = js::object(scope, descriptor)
    .ok_or_else(|| OpError::type_("createTexture requires a descriptor"))?;
  let (width, height) = texture_size(scope, object)?;
  let features = handle.device.features();
  let format = texture_format(&required_string(scope, object, "format")?)?;
  let format = require_texture_format(features, format)?;
  let view_formats = view_format_list(scope, object, features)?;
  let usage = wgpu::TextureUsages::from_bits_truncate(js::required_u32(
    scope, object, "usage",
  )?);
  let sample_count = allowed_sample_count(
    js::field_u32(scope, object, "sampleCount")?.unwrap_or(1),
  )?;
  let label = optional_label(scope, object)?;
  let texture = handle.device.create_texture(&wgpu::TextureDescriptor {
    label: label.as_deref(),
    size: wgpu::Extent3d {
      width,
      height,
      depth_or_array_layers: 1,
    },
    mip_level_count: 1,
    sample_count,
    dimension: wgpu::TextureDimension::D2,
    format,
    usage,
    view_formats: &view_formats,
  });
  Ok(New(GpuTexture::from_parts(
    texture,
    Arc::new(AtomicBool::new(false)),
    Arc::clone(&handle.lost),
    width,
    height,
    format,
    label,
    sample_count,
    usage.bits(),
  )))
}

pub fn create_sampler(
  scope: &mut v8::PinScope,
  handle: &Handle,
  descriptor: v8::Local<v8::Value>,
) -> Result<New<GpuSampler>, OpError> {
  if !descriptor.is_null_or_undefined()
    && js::object(scope, descriptor).is_none()
  {
    return Err(OpError::type_("GPUSamplerDescriptor must be an object"));
  }
  let object = js::object(scope, descriptor);
  let mag = filter_of(scope, object, "magFilter")?;
  let min = filter_of(scope, object, "minFilter")?;
  let mipmap = mipmap_filter_of(scope, object)?;
  let address_u = address_mode_of(scope, object, "addressModeU")?;
  let address_v = address_mode_of(scope, object, "addressModeV")?;
  let address_w = address_mode_of(scope, object, "addressModeW")?;
  let lod_min = lod_clamp_of(scope, object, "lodMinClamp", 0.0)?;
  let lod_max = lod_clamp_of(scope, object, "lodMaxClamp", 32.0)?;
  let anisotropy = anisotropy_of(scope, object)?;
  if let Some(object) = object
    && js::get(scope, object, "compare")?.is_some_and(|value| !value.is_null())
  {
    return Err(js::unsupported("GPUSamplerDescriptor.compare"));
  }
  let label = match object {
    Some(object) => optional_label(scope, object)?,
    None => None,
  };
  let sampler = handle.device.create_sampler(&wgpu::SamplerDescriptor {
    label: label.as_deref(),
    address_mode_u: address_u,
    address_mode_v: address_v,
    address_mode_w: address_w,
    mag_filter: mag,
    min_filter: min,
    mipmap_filter: mipmap,
    lod_min_clamp: lod_min,
    lod_max_clamp: lod_max,
    compare: None,
    anisotropy_clamp: anisotropy,
    border_color: None,
  });
  Ok(New(GpuSampler {
    sampler,
    label: ObjectLabel::new(label),
  }))
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
  check_compilation_hints(scope, object)?;
  let label = optional_label(scope, object)?;
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
  Ok(New(GpuShaderModule {
    module,
    label: ObjectLabel::new(label),
  }))
}

pub fn create_bind_group_layout(
  scope: &mut v8::PinScope,
  handle: &Handle,
  descriptor: v8::Local<v8::Value>,
) -> Result<New<GpuBindGroupLayout>, OpError> {
  let object = js::object(scope, descriptor).ok_or_else(|| {
    OpError::type_("createBindGroupLayout requires a descriptor")
  })?;
  let entries =
    binding_layout_entries(scope, object, handle.device.features())?;
  let label = optional_label(scope, object)?;
  let layout =
    handle
      .device
      .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: label.as_deref(),
        entries: &entries,
      });
  Ok(New(GpuBindGroupLayout {
    layout,
    label: ObjectLabel::new(label),
  }))
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
  let label = optional_label(scope, object)?;
  let immediate = js::field_u32(scope, object, "immediateSize")?.unwrap_or(0);
  if immediate != 0 {
    return Err(OpError::dom(
      "NotSupportedError",
      "immediates are not enabled",
    ));
  }
  let layout =
    handle
      .device
      .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: label.as_deref(),
        bind_group_layouts: &refs,
        immediate_size: 0,
      });
  Ok(New(GpuPipelineLayout {
    layout,
    label: ObjectLabel::new(label),
  }))
}

pub fn create_bind_group(
  scope: &mut v8::PinScope,
  handle: &Handle,
  descriptor: v8::Local<v8::Value>,
) -> Result<New<GpuBindGroup>, OpError> {
  let object = js::object(scope, descriptor)
    .ok_or_else(|| OpError::type_("createBindGroup requires a descriptor"))?;
  let layout_value = js::get(scope, object, "layout")?.ok_or_else(|| {
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
  let label = optional_label(scope, object)?;
  let group = handle.device.create_bind_group(&wgpu::BindGroupDescriptor {
    label: label.as_deref(),
    layout: layout.layout(),
    entries: &entries,
  });
  Ok(New(GpuBindGroup {
    group,
    label: ObjectLabel::new(label),
  }))
}

pub fn create_render_pipeline(
  scope: &mut v8::PinScope,
  handle: &Handle,
  descriptor: v8::Local<v8::Value>,
) -> Result<New<GpuRenderPipeline>, OpError> {
  let object = js::object(scope, descriptor).ok_or_else(|| {
    OpError::type_("createRenderPipeline requires a descriptor")
  })?;
  let vertex = js::get(scope, object, "vertex")?
    .and_then(|value| js::object(scope, value))
    .ok_or_else(|| {
      OpError::type_("GPURenderPipelineDescriptor.vertex is required")
    })?;
  let module = shader_of(scope, vertex)?;
  let entry = js::get(scope, vertex, "entryPoint")?
    .map(|value| js::string(scope, value))
    .transpose()?;
  let vertex_constants = pipeline_constants(scope, vertex)?;
  let vertex_pairs = constant_pairs(&vertex_constants);
  let attribute_sets = vertex_attributes(scope, vertex)?;
  let layouts = attribute_sets
    .iter()
    .map(|set| {
      set.as_ref().map(|set| wgpu::VertexBufferLayout {
        array_stride: set.stride,
        step_mode: set.step,
        attributes: &set.attributes,
      })
    })
    .collect::<Vec<_>>();
  let features = handle.device.features();
  let fragment = fragment_state(scope, object, features)?;
  let fragment_pairs = match &fragment {
    Some(stage) => constant_pairs(&stage.constants),
    None => Vec::new(),
  };
  let targets = fragment
    .as_ref()
    .map(|stage| stage.targets.clone())
    .unwrap_or_default();
  let target_refs = targets
    .iter()
    .map(|target| {
      target.as_ref().map(|target| wgpu::ColorTargetState {
        format: target.format,
        blend: target.blend,
        write_mask: target.write_mask,
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
      compilation_options: wgpu::PipelineCompilationOptions {
        constants: &fragment_pairs,
        ..Default::default()
      },
      targets: &target_refs,
    })
  } else {
    None
  };
  let primitive = primitive_state(scope, object, features)?;
  let depth_stencil = depth_stencil_state(scope, object, features)?;
  let multisample = multisample_state(scope, object)?;
  let label = optional_label(scope, object)?;
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
          compilation_options: wgpu::PipelineCompilationOptions {
            constants: &vertex_pairs,
            ..Default::default()
          },
          buffers: &layouts,
        },
        primitive,
        depth_stencil: depth_stencil.clone(),
        multisample,
        fragment: fragment_state,
        multiview_mask: None,
        cache: None,
      })
  }));
  let pipeline = pipeline.map_err(|_| {
    OpError::dom("OperationError", "The render pipeline is invalid")
  })?;
  Ok(New(GpuRenderPipeline {
    pipeline,
    label: ObjectLabel::new(label),
  }))
}

pub fn create_compute_pipeline(
  scope: &mut v8::PinScope,
  handle: &Handle,
  descriptor: v8::Local<v8::Value>,
) -> Result<New<GpuComputePipeline>, OpError> {
  let object = js::object(scope, descriptor).ok_or_else(|| {
    OpError::type_("createComputePipeline requires a descriptor")
  })?;
  let compute = js::get(scope, object, "compute")?
    .and_then(|value| js::object(scope, value))
    .ok_or_else(|| {
      OpError::type_("GPUComputePipelineDescriptor.compute is required")
    })?;
  let module = shader_of(scope, compute)?;
  let entry = js::get(scope, compute, "entryPoint")?
    .map(|value| js::string(scope, value))
    .transpose()?;
  let constants = pipeline_constants(scope, compute)?;
  let pairs = constant_pairs(&constants);
  let label = optional_label(scope, object)?;
  let explicit = explicit_layout(scope, object)?;
  let pipeline = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
    handle
      .device
      .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: label.as_deref(),
        layout: explicit.as_ref().map(GpuPipelineLayout::layout),
        module: module.module(),
        entry_point: entry.as_deref(),
        compilation_options: wgpu::PipelineCompilationOptions {
          constants: &pairs,
          ..Default::default()
        },
        cache: None,
      })
  }));
  let pipeline = pipeline.map_err(|_| {
    OpError::dom("OperationError", "The compute pipeline is invalid")
  })?;
  Ok(New(GpuComputePipeline {
    pipeline,
    label: ObjectLabel::new(label),
  }))
}

pub fn create_query_set(
  scope: &mut v8::PinScope,
  handle: &Handle,
  descriptor: v8::Local<v8::Value>,
) -> Result<New<GpuQuerySet>, OpError> {
  let object = js::object(scope, descriptor)
    .ok_or_else(|| OpError::type_("createQuerySet requires a descriptor"))?;
  let kind = match required_string(scope, object, "type")?.as_str() {
    "occlusion" => QueryKind::Occlusion,
    "timestamp" => QueryKind::Timestamp,
    _ => return Err(OpError::type_("The query type is invalid")),
  };
  if kind == QueryKind::Timestamp
    && !handle
      .device
      .features()
      .contains(wgpu::Features::TIMESTAMP_QUERY)
  {
    return Err(OpError::dom(
      "NotSupportedError",
      "timestamp-query is not enabled",
    ));
  }
  let count = js::required_u32(scope, object, "count")?;
  let label = optional_label(scope, object)?;
  let ty = match kind {
    QueryKind::Occlusion => wgpu::QueryType::Occlusion,
    QueryKind::Timestamp => wgpu::QueryType::Timestamp,
  };
  let set = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
    handle.device.create_query_set(&wgpu::QuerySetDescriptor {
      label: label.as_deref(),
      ty,
      count,
    })
  }));
  let set = set
    .map_err(|_| OpError::dom("OperationError", "The query set is invalid"))?;
  Ok(New(GpuQuerySet {
    set,
    kind,
    count,
    label: ObjectLabel::new(label),
    destroyed: Cell::new(false),
  }))
}

struct VertexSet {
  stride: u64,
  step: wgpu::VertexStepMode,
  attributes: Vec<wgpu::VertexAttribute>,
}

#[derive(Clone)]
struct ColorTargetOwned {
  format: wgpu::TextureFormat,
  blend: Option<wgpu::BlendState>,
  write_mask: wgpu::ColorWrites,
}

#[derive(Clone)]
struct FragmentOwned {
  module: wgpu::ShaderModule,
  entry: Option<String>,
  targets: Vec<Option<ColorTargetOwned>>,
  constants: Vec<(String, f64)>,
}

enum OwnedBinding {
  Buffer(wgpu::Buffer, u64, Option<u64>),
  View(wgpu::TextureView),
  Sampler(wgpu::Sampler),
  External(wgpu::ExternalTexture),
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
      OwnedBinding::External(external) => {
        wgpu::BindingResource::ExternalTexture(external)
      }
    }
  }
}

fn binding_layout_entries(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  features: wgpu::Features,
) -> Result<Vec<wgpu::BindGroupLayoutEntry>, OpError> {
  let list = array_of(scope, object, "entries")?;
  let mut entries = Vec::new();
  for value in list {
    let entry = js::object(scope, value).ok_or_else(|| {
      OpError::type_("A bind group layout entry must be an object")
    })?;
    let binding = js::required_u32(scope, entry, "binding")?;
    let visibility = js::required_u32(scope, entry, "visibility")?;
    let ty = layout_binding_type(scope, entry, features)?;
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
  features: wgpu::Features,
) -> Result<wgpu::BindingType, OpError> {
  let buffer = present_object(scope, entry, "buffer")?;
  let sampler = present_object(scope, entry, "sampler")?;
  let texture = present_object(scope, entry, "texture")?;
  let storage = present_object(scope, entry, "storageTexture")?;
  let external = js::get(scope, entry, "externalTexture")?
    .is_some_and(|value| !value.is_null_or_undefined());
  let chosen = [
    buffer.is_some(),
    sampler.is_some(),
    texture.is_some(),
    storage.is_some(),
    external,
  ]
  .into_iter()
  .filter(|present| *present)
  .count();
  if chosen != 1 {
    return Err(OpError::type_(
      "A bind group layout entry needs one binding type",
    ));
  }
  if external {
    if !features.contains(wgpu::Features::EXTERNAL_TEXTURE) {
      return Err(js::unsupported("externalTexture"));
    }
    return Ok(wgpu::BindingType::ExternalTexture);
  }
  if let Some(buffer) = buffer {
    return buffer_binding(scope, buffer);
  }
  if let Some(sampler) = sampler {
    return Ok(wgpu::BindingType::Sampler(sampler_binding(scope, sampler)?));
  }
  if let Some(texture) = texture {
    return texture_binding(scope, texture);
  }
  storage_binding(scope, storage.unwrap(), features)
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
    let resource = js::get(scope, entry, "resource")?
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
  if let Some(external) = host_external(scope, value) {
    return Ok(OwnedBinding::External(external.external().clone()));
  }
  let Some(object) = js::object(scope, value) else {
    return Err(OpError::type_("The bind group resource is invalid"));
  };
  let buffer_value = js::get(scope, object, "buffer")?
    .ok_or_else(|| OpError::type_("The bind group resource is invalid"))?;
  let buffer = host_buffer(scope, buffer_value)
    .ok_or_else(|| OpError::type_("The bind group resource is not a buffer"))?;
  let offset = js::field_u64(scope, object, "offset")?.unwrap_or(0);
  let size = js::field_u64(scope, object, "size")?;
  Ok(OwnedBinding::Buffer(buffer.alive()?, offset, size))
}

fn vertex_attributes(
  scope: &mut v8::PinScope,
  vertex: v8::Local<v8::Object>,
) -> Result<Vec<Option<VertexSet>>, OpError> {
  let Some(buffers) = js::get(scope, vertex, "buffers")? else {
    return Ok(Vec::new());
  };
  let Ok(array) = v8::Local::<v8::Array>::try_from(buffers) else {
    return Err(OpError::type_("GPUVertexState.buffers must be an array"));
  };
  let mut sets = Vec::new();
  for index in 0..array.length() {
    let value = js::read_index(scope, array, index)?;
    if value.is_null_or_undefined() {
      sets.push(None);
      continue;
    }
    let layout = js::object(scope, value).ok_or_else(|| {
      OpError::type_("A vertex buffer layout must be an object")
    })?;
    let stride = js::required_u64(scope, layout, "arrayStride")?;
    let step = match js::get(scope, layout, "stepMode")?
      .map(|value| js::string(scope, value))
      .transpose()?
      .as_deref()
    {
      None | Some("vertex") => wgpu::VertexStepMode::Vertex,
      Some("instance") => wgpu::VertexStepMode::Instance,
      _ => {
        return Err(OpError::type_("The vertex step mode is invalid"));
      }
    };
    let mut attributes = Vec::new();
    if let Some(list) = js::get(scope, layout, "attributes")? {
      let array = v8::Local::<v8::Array>::try_from(list)
        .map_err(|_| OpError::type_("Vertex attributes must be an array"))?;
      for index in 0..array.length() {
        let value = js::read_index(scope, array, index)?;
        let attribute = js::object(scope, value).ok_or_else(|| {
          OpError::type_("A vertex attribute must be an object")
        })?;
        let format =
          vertex_format(&required_string(scope, attribute, "format")?)?;
        let offset = js::required_u64(scope, attribute, "offset")?;
        let shader_location =
          js::required_u32(scope, attribute, "shaderLocation")?;
        attributes.push(wgpu::VertexAttribute {
          format,
          offset,
          shader_location,
        });
      }
    }
    sets.push(Some(VertexSet {
      stride,
      step,
      attributes,
    }));
  }
  Ok(sets)
}

fn blend_state(
  scope: &mut v8::PinScope,
  target: v8::Local<v8::Object>,
  features: wgpu::Features,
) -> Result<Option<wgpu::BlendState>, OpError> {
  let Some(value) = js::get(scope, target, "blend")? else {
    return Ok(None);
  };
  if value.is_null_or_undefined() {
    return Ok(None);
  }
  let object = js::object(scope, value)
    .ok_or_else(|| OpError::type_("GPUBlendState must be an object"))?;
  Ok(Some(wgpu::BlendState {
    color: blend_component(scope, object, "color", features)?,
    alpha: blend_component(scope, object, "alpha", features)?,
  }))
}

fn blend_component(
  scope: &mut v8::PinScope,
  blend: v8::Local<v8::Object>,
  name: &str,
  features: wgpu::Features,
) -> Result<wgpu::BlendComponent, OpError> {
  let value =
    js::get(scope, blend, name)?.filter(|value| !value.is_null_or_undefined());
  let Some(value) = value else {
    return Err(OpError::type_(format!("GPUBlendState.{name} is required")));
  };
  let object = js::object(scope, value).ok_or_else(|| {
    OpError::type_(format!("GPUBlendState.{name} must be an object"))
  })?;
  let operation = match js::get(scope, object, "operation")?
    .map(|value| js::string(scope, value))
    .transpose()?
    .as_deref()
  {
    None | Some("add") => wgpu::BlendOperation::Add,
    Some("subtract") => wgpu::BlendOperation::Subtract,
    Some("reverse-subtract") => wgpu::BlendOperation::ReverseSubtract,
    Some("min") => wgpu::BlendOperation::Min,
    Some("max") => wgpu::BlendOperation::Max,
    _ => return Err(OpError::type_("The blend operation is invalid")),
  };
  Ok(wgpu::BlendComponent {
    src_factor: blend_factor(scope, object, "srcFactor", features, true)?,
    dst_factor: blend_factor(scope, object, "dstFactor", features, false)?,
    operation,
  })
}

fn blend_factor(
  scope: &mut v8::PinScope,
  component: v8::Local<v8::Object>,
  name: &str,
  features: wgpu::Features,
  source: bool,
) -> Result<wgpu::BlendFactor, OpError> {
  let text = js::get(scope, component, name)?
    .map(|value| js::string(scope, value))
    .transpose()?;
  let factor = match text.as_deref() {
    None if source => wgpu::BlendFactor::One,
    None => wgpu::BlendFactor::Zero,
    Some("zero") => wgpu::BlendFactor::Zero,
    Some("one") => wgpu::BlendFactor::One,
    Some("src") => wgpu::BlendFactor::Src,
    Some("one-minus-src") => wgpu::BlendFactor::OneMinusSrc,
    Some("src-alpha") => wgpu::BlendFactor::SrcAlpha,
    Some("one-minus-src-alpha") => wgpu::BlendFactor::OneMinusSrcAlpha,
    Some("dst") => wgpu::BlendFactor::Dst,
    Some("one-minus-dst") => wgpu::BlendFactor::OneMinusDst,
    Some("dst-alpha") => wgpu::BlendFactor::DstAlpha,
    Some("one-minus-dst-alpha") => wgpu::BlendFactor::OneMinusDstAlpha,
    Some("src-alpha-saturated") => wgpu::BlendFactor::SrcAlphaSaturated,
    Some("constant") => wgpu::BlendFactor::Constant,
    Some("one-minus-constant") => wgpu::BlendFactor::OneMinusConstant,
    Some("src1") => wgpu::BlendFactor::Src1,
    Some("one-minus-src1") => wgpu::BlendFactor::OneMinusSrc1,
    Some("src1-alpha") => wgpu::BlendFactor::Src1Alpha,
    Some("one-minus-src1-alpha") => wgpu::BlendFactor::OneMinusSrc1Alpha,
    _ => return Err(OpError::type_("The blend factor is invalid")),
  };
  if factor.ref_second_blend_source()
    && !features.contains(wgpu::Features::DUAL_SOURCE_BLENDING)
  {
    return Err(js::unsupported("Dual source blending"));
  }
  Ok(factor)
}

fn fragment_state(
  scope: &mut v8::PinScope,
  descriptor: v8::Local<v8::Object>,
  features: wgpu::Features,
) -> Result<Option<FragmentOwned>, OpError> {
  let Some(value) = js::get(scope, descriptor, "fragment")? else {
    return Ok(None);
  };
  if value.is_null() {
    return Ok(None);
  }
  let fragment = js::object(scope, value)
    .ok_or_else(|| OpError::type_("GPUFragmentState must be an object"))?;
  let module = shader_of(scope, fragment)?;
  let entry = js::get(scope, fragment, "entryPoint")?
    .map(|value| js::string(scope, value))
    .transpose()?;
  let constants = pipeline_constants(scope, fragment)?;
  let list = array_of(scope, fragment, "targets")?;
  let mut targets = Vec::new();
  for value in list {
    if value.is_null() {
      targets.push(None);
      continue;
    }
    let target = js::object(scope, value)
      .ok_or_else(|| OpError::type_("A color target must be an object"))?;
    let format = texture_format(&required_string(scope, target, "format")?)?;
    let format = require_texture_format(features, format)?;
    let blend = blend_state(scope, target, features)?;
    let write_mask = match js::field_u32(scope, target, "writeMask")? {
      Some(bits) => wgpu::ColorWrites::from_bits_truncate(bits),
      None => wgpu::ColorWrites::ALL,
    };
    targets.push(Some(ColorTargetOwned {
      format,
      blend,
      write_mask,
    }));
  }
  Ok(Some(FragmentOwned {
    module: module.module().clone(),
    entry,
    targets,
    constants,
  }))
}

fn optional_enum(
  scope: &mut v8::PinScope,
  object: Option<v8::Local<v8::Object>>,
  name: &str,
) -> Result<Option<String>, OpError> {
  let Some(object) = object else {
    return Ok(None);
  };
  match js::get(scope, object, name)? {
    Some(value) => Ok(Some(js::string(scope, value)?)),
    None => Ok(None),
  }
}

fn primitive_state(
  scope: &mut v8::PinScope,
  descriptor: v8::Local<v8::Object>,
  features: wgpu::Features,
) -> Result<wgpu::PrimitiveState, OpError> {
  let object = js::get(scope, descriptor, "primitive")?
    .filter(|value| !value.is_null_or_undefined())
    .and_then(|value| js::object(scope, value));
  let topology = match optional_enum(scope, object, "topology")?.as_deref() {
    None | Some("triangle-list") => wgpu::PrimitiveTopology::TriangleList,
    Some("triangle-strip") => wgpu::PrimitiveTopology::TriangleStrip,
    Some("line-list") => wgpu::PrimitiveTopology::LineList,
    Some("line-strip") => wgpu::PrimitiveTopology::LineStrip,
    Some("point-list") => wgpu::PrimitiveTopology::PointList,
    _ => return Err(OpError::type_("The primitive topology is invalid")),
  };
  let cull_mode = match optional_enum(scope, object, "cullMode")?.as_deref() {
    None | Some("none") => None,
    Some("front") => Some(wgpu::Face::Front),
    Some("back") => Some(wgpu::Face::Back),
    _ => return Err(OpError::type_("The cull mode is invalid")),
  };
  let front_face = match optional_enum(scope, object, "frontFace")?.as_deref() {
    None | Some("ccw") => wgpu::FrontFace::Ccw,
    Some("cw") => wgpu::FrontFace::Cw,
    _ => return Err(OpError::type_("The front face is invalid")),
  };
  let strip_index_format =
    match optional_enum(scope, object, "stripIndexFormat")?.as_deref() {
      None => None,
      Some("uint16") => Some(wgpu::IndexFormat::Uint16),
      Some("uint32") => Some(wgpu::IndexFormat::Uint32),
      _ => return Err(OpError::type_("The strip index format is invalid")),
    };
  let unclipped_depth = match object {
    Some(object) => js::get(scope, object, "unclippedDepth")?
      .is_some_and(|value| value.boolean_value(scope)),
    None => false,
  };
  if unclipped_depth && !features.contains(wgpu::Features::DEPTH_CLIP_CONTROL) {
    return Err(js::unsupported("unclippedDepth"));
  }
  Ok(wgpu::PrimitiveState {
    topology,
    strip_index_format,
    front_face,
    cull_mode,
    unclipped_depth,
    polygon_mode: wgpu::PolygonMode::Fill,
    conservative: false,
  })
}

fn depth_stencil_state(
  scope: &mut v8::PinScope,
  descriptor: v8::Local<v8::Object>,
  features: wgpu::Features,
) -> Result<Option<wgpu::DepthStencilState>, OpError> {
  let Some(value) = js::get(scope, descriptor, "depthStencil")? else {
    return Ok(None);
  };
  if value.is_null_or_undefined() {
    return Ok(None);
  }
  let object = js::object(scope, value)
    .ok_or_else(|| OpError::type_("depthStencil must be an object"))?;
  let format = texture_format(&required_string(scope, object, "format")?)?;
  let format = require_texture_format(features, format)?;
  let depth_write_enabled = if format.has_depth_aspect() {
    Some(
      js::get(scope, object, "depthWriteEnabled")?
        .ok_or_else(|| OpError::type_("depthWriteEnabled is required"))?
        .boolean_value(scope),
    )
  } else if js::get(scope, object, "depthWriteEnabled")?
    .is_some_and(|value| !value.is_null_or_undefined())
  {
    return Err(OpError::type_("The texture format has no depth aspect"));
  } else {
    None
  };
  let depth_compare = if format.has_depth_aspect() {
    Some(compare_function(
      scope,
      object,
      "depthCompare",
      wgpu::CompareFunction::Always,
    )?)
  } else if js::get(scope, object, "depthCompare")?
    .is_some_and(|value| !value.is_null_or_undefined())
  {
    return Err(OpError::type_("The texture format has no depth aspect"));
  } else {
    None
  };
  let front = stencil_face(scope, object, "stencilFront")?;
  let back = stencil_face(scope, object, "stencilBack")?;
  let read_mask =
    js::field_u32(scope, object, "stencilReadMask")?.unwrap_or(0xFFFF_FFFF);
  let write_mask =
    js::field_u32(scope, object, "stencilWriteMask")?.unwrap_or(0xFFFF_FFFF);
  Ok(Some(wgpu::DepthStencilState {
    format,
    depth_write_enabled,
    depth_compare,
    stencil: wgpu::StencilState {
      front,
      back,
      read_mask,
      write_mask,
    },
    bias: wgpu::DepthBiasState {
      constant: field_i32(scope, object, "depthBias")?,
      slope_scale: field_f32(scope, object, "depthBiasSlopeScale")?,
      clamp: field_f32(scope, object, "depthBiasClamp")?,
    },
  }))
}

fn multisample_state(
  scope: &mut v8::PinScope,
  descriptor: v8::Local<v8::Object>,
) -> Result<wgpu::MultisampleState, OpError> {
  let Some(value) = js::get(scope, descriptor, "multisample")? else {
    return Ok(wgpu::MultisampleState::default());
  };
  if value.is_null_or_undefined() {
    return Ok(wgpu::MultisampleState::default());
  }
  let object = js::object(scope, value)
    .ok_or_else(|| OpError::type_("multisample must be an object"))?;
  let count =
    allowed_sample_count(js::field_u32(scope, object, "count")?.unwrap_or(1))?;
  let alpha_to_coverage_enabled =
    js::get(scope, object, "alphaToCoverageEnabled")?
      .is_some_and(|value| value.boolean_value(scope));
  let mask = js::field_u32(scope, object, "mask")?
    .map(u64::from)
    .unwrap_or(!0);
  Ok(wgpu::MultisampleState {
    count,
    mask,
    alpha_to_coverage_enabled,
  })
}

fn compare_function(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
  default: wgpu::CompareFunction,
) -> Result<wgpu::CompareFunction, OpError> {
  match js::get(scope, object, name)?
    .map(|value| js::string(scope, value))
    .transpose()?
    .as_deref()
  {
    None => Ok(default),
    Some("never") => Ok(wgpu::CompareFunction::Never),
    Some("less") => Ok(wgpu::CompareFunction::Less),
    Some("equal") => Ok(wgpu::CompareFunction::Equal),
    Some("less-equal") => Ok(wgpu::CompareFunction::LessEqual),
    Some("greater") => Ok(wgpu::CompareFunction::Greater),
    Some("not-equal") => Ok(wgpu::CompareFunction::NotEqual),
    Some("greater-equal") => Ok(wgpu::CompareFunction::GreaterEqual),
    Some("always") => Ok(wgpu::CompareFunction::Always),
    _ => Err(OpError::type_("The compare function is invalid")),
  }
}

fn stencil_face(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
) -> Result<wgpu::StencilFaceState, OpError> {
  let Some(value) = js::get(scope, object, name)? else {
    return Ok(wgpu::StencilFaceState::IGNORE);
  };
  if value.is_null_or_undefined() {
    return Ok(wgpu::StencilFaceState::IGNORE);
  }
  let face = js::object(scope, value)
    .ok_or_else(|| OpError::type_("A stencil face must be an object"))?;
  Ok(wgpu::StencilFaceState {
    compare: compare_function(
      scope,
      face,
      "compare",
      wgpu::CompareFunction::Always,
    )?,
    fail_op: stencil_op(scope, face, "failOp")?,
    depth_fail_op: stencil_op(scope, face, "depthFailOp")?,
    pass_op: stencil_op(scope, face, "passOp")?,
  })
}

fn stencil_op(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
) -> Result<wgpu::StencilOperation, OpError> {
  match js::get(scope, object, name)?
    .map(|value| js::string(scope, value))
    .transpose()?
    .as_deref()
  {
    None | Some("keep") => Ok(wgpu::StencilOperation::Keep),
    Some("zero") => Ok(wgpu::StencilOperation::Zero),
    Some("replace") => Ok(wgpu::StencilOperation::Replace),
    Some("invert") => Ok(wgpu::StencilOperation::Invert),
    Some("increment-clamp") => Ok(wgpu::StencilOperation::IncrementClamp),
    Some("decrement-clamp") => Ok(wgpu::StencilOperation::DecrementClamp),
    Some("increment-wrap") => Ok(wgpu::StencilOperation::IncrementWrap),
    Some("decrement-wrap") => Ok(wgpu::StencilOperation::DecrementWrap),
    _ => Err(OpError::type_("The stencil operation is invalid")),
  }
}

/// `[EnforceRange] long` (`GPUDepthBias`).
fn field_i32(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
) -> Result<i32, OpError> {
  let Some(value) = js::get(scope, object, name)? else {
    return Ok(0);
  };
  let number = yuzuko_core::to_number(scope, value)?;
  yuzuko_core::enforce_range_i32(number)
    .ok_or_else(|| OpError::type_("The provided value is out of range"))
}

/// WebIDL `float`. Absent is `0`.
fn field_f32(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
) -> Result<f32, OpError> {
  let Some(value) = js::get(scope, object, name)? else {
    return Ok(0.0);
  };
  js::f32_value(scope, value)
}

fn present_object<'a>(
  scope: &mut v8::PinScope,
  entry: v8::Local<'a, v8::Object>,
  name: &str,
) -> Result<Option<v8::Local<'a, v8::Object>>, OpError> {
  let Some(value) = js::get(scope, entry, name)? else {
    return Ok(None);
  };
  if value.is_null_or_undefined() {
    return Ok(None);
  }
  js::object(scope, value)
    .map(Some)
    .ok_or_else(|| OpError::type_(format!("{name} must be an object")))
}

fn buffer_binding(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
) -> Result<wgpu::BindingType, OpError> {
  let ty = match js::get(scope, object, "type")?
    .map(|value| js::string(scope, value))
    .transpose()?
    .as_deref()
  {
    None | Some("uniform") => wgpu::BufferBindingType::Uniform,
    Some("storage") => wgpu::BufferBindingType::Storage { read_only: false },
    Some("read-only-storage") => {
      wgpu::BufferBindingType::Storage { read_only: true }
    }
    _ => return Err(OpError::type_("The buffer binding type is invalid")),
  };
  let has_dynamic_offset = js::get(scope, object, "hasDynamicOffset")?
    .is_some_and(|value| value.boolean_value(scope));
  // `0`, including `null` converted through `ToNumber`, means no minimum.
  let min_binding_size = js::field_u64(scope, object, "minBindingSize")?
    .and_then(std::num::NonZeroU64::new);
  Ok(wgpu::BindingType::Buffer {
    ty,
    has_dynamic_offset,
    min_binding_size,
  })
}

fn sampler_binding(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
) -> Result<wgpu::SamplerBindingType, OpError> {
  match js::get(scope, object, "type")?
    .map(|value| js::string(scope, value))
    .transpose()?
    .as_deref()
  {
    None | Some("filtering") => Ok(wgpu::SamplerBindingType::Filtering),
    Some("non-filtering") => Ok(wgpu::SamplerBindingType::NonFiltering),
    Some("comparison") => Ok(wgpu::SamplerBindingType::Comparison),
    _ => Err(OpError::type_("The sampler binding type is invalid")),
  }
}

fn texture_binding(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
) -> Result<wgpu::BindingType, OpError> {
  let sample_type = match js::get(scope, object, "sampleType")?
    .map(|value| js::string(scope, value))
    .transpose()?
    .as_deref()
  {
    None | Some("float") => wgpu::TextureSampleType::Float { filterable: true },
    Some("unfilterable-float") => {
      wgpu::TextureSampleType::Float { filterable: false }
    }
    Some("depth") => wgpu::TextureSampleType::Depth,
    Some("sint") => wgpu::TextureSampleType::Sint,
    Some("uint") => wgpu::TextureSampleType::Uint,
    _ => return Err(OpError::type_("The texture sample type is invalid")),
  };
  Ok(wgpu::BindingType::Texture {
    sample_type,
    view_dimension: view_dimension(scope, object)?,
    multisampled: js::get(scope, object, "multisampled")?
      .is_some_and(|value| value.boolean_value(scope)),
  })
}

fn storage_binding(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  features: wgpu::Features,
) -> Result<wgpu::BindingType, OpError> {
  let access = js::get(scope, object, "access")?
    .map(|value| js::string(scope, value))
    .transpose()?;
  let access = match access.as_deref() {
    None | Some("write-only") => wgpu::StorageTextureAccess::WriteOnly,
    Some("read-only") => wgpu::StorageTextureAccess::ReadOnly,
    Some("read-write") => wgpu::StorageTextureAccess::ReadWrite,
    Some("atomic") => wgpu::StorageTextureAccess::Atomic,
    _ => {
      return Err(OpError::type_("The storage texture access is invalid"));
    }
  };
  let format = texture_format(&required_string(scope, object, "format")?)?;
  let format = require_texture_format(features, format)?;
  Ok(wgpu::BindingType::StorageTexture {
    access,
    format,
    view_dimension: view_dimension(scope, object)?,
  })
}

/// WebGPU sample counts are 1 and 4.
pub fn allowed_sample_count(count: u32) -> Result<u32, OpError> {
  if count == 1 || count == 4 {
    Ok(count)
  } else {
    Err(OpError::type_("The sample count is invalid"))
  }
}

fn view_dimension(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
) -> Result<wgpu::TextureViewDimension, OpError> {
  match js::get(scope, object, "viewDimension")?
    .map(|value| js::string(scope, value))
    .transpose()?
    .as_deref()
  {
    None => Ok(wgpu::TextureViewDimension::D2),
    Some(name) => texture_view_dimension(name),
  }
}

fn texture_view_dimension(
  name: &str,
) -> Result<wgpu::TextureViewDimension, OpError> {
  match name {
    "1d" => Ok(wgpu::TextureViewDimension::D1),
    "2d" => Ok(wgpu::TextureViewDimension::D2),
    "2d-array" => Ok(wgpu::TextureViewDimension::D2Array),
    "cube" => Ok(wgpu::TextureViewDimension::Cube),
    "cube-array" => Ok(wgpu::TextureViewDimension::CubeArray),
    "3d" => Ok(wgpu::TextureViewDimension::D3),
    _ => Err(OpError::type_("The texture view dimension is invalid")),
  }
}

fn texture_aspect(name: &str) -> Result<wgpu::TextureAspect, OpError> {
  match name {
    "all" => Ok(wgpu::TextureAspect::All),
    "depth-only" => Ok(wgpu::TextureAspect::DepthOnly),
    "stencil-only" => Ok(wgpu::TextureAspect::StencilOnly),
    _ => Err(OpError::type_("The texture aspect is invalid")),
  }
}

#[derive(Default)]
struct TextureViewParts {
  label: Option<String>,
  format: Option<wgpu::TextureFormat>,
  dimension: Option<wgpu::TextureViewDimension>,
  aspect: wgpu::TextureAspect,
  base_mip_level: u32,
  mip_level_count: Option<u32>,
  base_array_layer: u32,
  array_layer_count: Option<u32>,
}

/// WebIDL members of `GPUTextureViewDescriptor`, before the texture is used.
fn texture_view_parts(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<TextureViewParts, OpError> {
  let object = js::object(scope, value).ok_or_else(|| {
    OpError::type_("GPUTextureViewDescriptor must be an object")
  })?;
  if let Some(swizzle) = js::get(scope, object, "swizzle")?
    && !swizzle.is_null()
  {
    let text = js::string(scope, swizzle)?;
    if text != "rgba" {
      return Err(js::unsupported("GPUTextureViewDescriptor.swizzle"));
    }
  }
  if js::get(scope, object, "usage")?.is_some() {
    return Err(js::unsupported("GPUTextureViewDescriptor.usage"));
  }
  let format = match js::get(scope, object, "format")?
    .map(|value| js::string(scope, value))
    .transpose()?
    .as_deref()
  {
    None => None,
    Some(name) => Some(texture_format(name)?),
  };
  let dimension = match js::get(scope, object, "dimension")?
    .map(|value| js::string(scope, value))
    .transpose()?
    .as_deref()
  {
    None => None,
    Some(name) => Some(texture_view_dimension(name)?),
  };
  let aspect = match js::get(scope, object, "aspect")?
    .map(|value| js::string(scope, value))
    .transpose()?
    .as_deref()
  {
    None => wgpu::TextureAspect::All,
    Some(name) => texture_aspect(name)?,
  };
  Ok(TextureViewParts {
    label: optional_label(scope, object)?,
    format,
    dimension,
    aspect,
    base_mip_level: js::field_u32(scope, object, "baseMipLevel")?.unwrap_or(0),
    mip_level_count: js::field_u32(scope, object, "mipLevelCount")?,
    base_array_layer: js::field_u32(scope, object, "baseArrayLayer")?
      .unwrap_or(0),
    array_layer_count: js::field_u32(scope, object, "arrayLayerCount")?,
  })
}

fn view_format_list(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  features: wgpu::Features,
) -> Result<Vec<wgpu::TextureFormat>, OpError> {
  let Some(list) = js::get(scope, object, "viewFormats")? else {
    return Ok(Vec::new());
  };
  let array = v8::Local::<v8::Array>::try_from(list)
    .map_err(|_| OpError::type_("viewFormats must be an array"))?;
  let mut formats = Vec::with_capacity(array.length() as usize);
  for index in 0..array.length() {
    let item = js::read_index(scope, array, index)?;
    let name = js::string(scope, item)?;
    formats.push(require_texture_format(features, texture_format(&name)?)?);
  }
  Ok(formats)
}

fn explicit_layout(
  scope: &mut v8::PinScope,
  descriptor: v8::Local<v8::Object>,
) -> Result<Option<GpuPipelineLayout>, OpError> {
  let Some(layout) = js::get(scope, descriptor, "layout")? else {
    return Ok(None);
  };
  if layout.is_null() || js::string(scope, layout)? == "auto" {
    return Ok(None);
  }
  host_pipeline_layout(scope, layout).map(Some)
}

fn texture_size(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
) -> Result<(u32, u32), OpError> {
  let size = js::get(scope, object, "size")?
    .ok_or_else(|| OpError::type_("GPUTextureDescriptor.size is required"))?;
  if let Ok(array) = v8::Local::<v8::Array>::try_from(size) {
    let width = js::index_u32(scope, array, 0)?;
    let height = if array.length() > 1 {
      js::index_u32(scope, array, 1)?
    } else {
      1
    };
    return Ok((width, height));
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
    "r8unorm" => Ok(wgpu::TextureFormat::R8Unorm),
    "rg8unorm" => Ok(wgpu::TextureFormat::Rg8Unorm),
    "rgba16float" => Ok(wgpu::TextureFormat::Rgba16Float),
    "r16float" => Ok(wgpu::TextureFormat::R16Float),
    "rg16float" => Ok(wgpu::TextureFormat::Rg16Float),
    "r32float" => Ok(wgpu::TextureFormat::R32Float),
    "rg32float" => Ok(wgpu::TextureFormat::Rg32Float),
    "rgba32float" => Ok(wgpu::TextureFormat::Rgba32Float),
    "r8uint" => Ok(wgpu::TextureFormat::R8Uint),
    "r8sint" => Ok(wgpu::TextureFormat::R8Sint),
    "r16uint" => Ok(wgpu::TextureFormat::R16Uint),
    "r16sint" => Ok(wgpu::TextureFormat::R16Sint),
    "r32uint" => Ok(wgpu::TextureFormat::R32Uint),
    "r32sint" => Ok(wgpu::TextureFormat::R32Sint),
    "rg8uint" => Ok(wgpu::TextureFormat::Rg8Uint),
    "rg8sint" => Ok(wgpu::TextureFormat::Rg8Sint),
    "rg16uint" => Ok(wgpu::TextureFormat::Rg16Uint),
    "rg16sint" => Ok(wgpu::TextureFormat::Rg16Sint),
    "rg32uint" => Ok(wgpu::TextureFormat::Rg32Uint),
    "rg32sint" => Ok(wgpu::TextureFormat::Rg32Sint),
    "rgba8uint" => Ok(wgpu::TextureFormat::Rgba8Uint),
    "rgba8sint" => Ok(wgpu::TextureFormat::Rgba8Sint),
    "rgba16uint" => Ok(wgpu::TextureFormat::Rgba16Uint),
    "rgba16sint" => Ok(wgpu::TextureFormat::Rgba16Sint),
    "rgba32uint" => Ok(wgpu::TextureFormat::Rgba32Uint),
    "rgba32sint" => Ok(wgpu::TextureFormat::Rgba32Sint),
    "depth16unorm" => Ok(wgpu::TextureFormat::Depth16Unorm),
    "depth24plus" => Ok(wgpu::TextureFormat::Depth24Plus),
    "depth24plus-stencil8" => Ok(wgpu::TextureFormat::Depth24PlusStencil8),
    "depth32float" => Ok(wgpu::TextureFormat::Depth32Float),
    "depth32float-stencil8" => Ok(wgpu::TextureFormat::Depth32FloatStencil8),
    "stencil8" => Ok(wgpu::TextureFormat::Stencil8),
    _ => Err(OpError::type_("The texture format is invalid")),
  }
}

/// `depth32float-stencil8` is a real format only when the device enabled it.
pub fn require_texture_format(
  features: wgpu::Features,
  format: wgpu::TextureFormat,
) -> Result<wgpu::TextureFormat, OpError> {
  if format == wgpu::TextureFormat::Depth32FloatStencil8
    && !features.contains(wgpu::Features::DEPTH32FLOAT_STENCIL8)
  {
    return Err(js::unsupported("depth32float-stencil8"));
  }
  Ok(format)
}

pub(crate) fn format_name(format: wgpu::TextureFormat) -> &'static str {
  match format {
    wgpu::TextureFormat::Bgra8Unorm => "bgra8unorm",
    wgpu::TextureFormat::Rgba8Unorm => "rgba8unorm",
    wgpu::TextureFormat::Bgra8UnormSrgb => "bgra8unorm-srgb",
    wgpu::TextureFormat::Rgba8UnormSrgb => "rgba8unorm-srgb",
    wgpu::TextureFormat::R8Unorm => "r8unorm",
    wgpu::TextureFormat::Rg8Unorm => "rg8unorm",
    wgpu::TextureFormat::Rgba16Float => "rgba16float",
    wgpu::TextureFormat::R16Float => "r16float",
    wgpu::TextureFormat::Rg16Float => "rg16float",
    wgpu::TextureFormat::R32Float => "r32float",
    wgpu::TextureFormat::Rg32Float => "rg32float",
    wgpu::TextureFormat::Rgba32Float => "rgba32float",
    wgpu::TextureFormat::R8Uint => "r8uint",
    wgpu::TextureFormat::R8Sint => "r8sint",
    wgpu::TextureFormat::R16Uint => "r16uint",
    wgpu::TextureFormat::R16Sint => "r16sint",
    wgpu::TextureFormat::R32Uint => "r32uint",
    wgpu::TextureFormat::R32Sint => "r32sint",
    wgpu::TextureFormat::Rg8Uint => "rg8uint",
    wgpu::TextureFormat::Rg8Sint => "rg8sint",
    wgpu::TextureFormat::Rg16Uint => "rg16uint",
    wgpu::TextureFormat::Rg16Sint => "rg16sint",
    wgpu::TextureFormat::Rg32Uint => "rg32uint",
    wgpu::TextureFormat::Rg32Sint => "rg32sint",
    wgpu::TextureFormat::Rgba8Uint => "rgba8uint",
    wgpu::TextureFormat::Rgba8Sint => "rgba8sint",
    wgpu::TextureFormat::Rgba16Uint => "rgba16uint",
    wgpu::TextureFormat::Rgba16Sint => "rgba16sint",
    wgpu::TextureFormat::Rgba32Uint => "rgba32uint",
    wgpu::TextureFormat::Rgba32Sint => "rgba32sint",
    wgpu::TextureFormat::Depth16Unorm => "depth16unorm",
    wgpu::TextureFormat::Depth24Plus => "depth24plus",
    wgpu::TextureFormat::Depth24PlusStencil8 => "depth24plus-stencil8",
    wgpu::TextureFormat::Depth32Float => "depth32float",
    wgpu::TextureFormat::Depth32FloatStencil8 => "depth32float-stencil8",
    wgpu::TextureFormat::Stencil8 => "stencil8",
    _ => "rgba8unorm",
  }
}

fn vertex_format(name: &str) -> Result<wgpu::VertexFormat, OpError> {
  match name {
    "uint8" => Ok(wgpu::VertexFormat::Uint8),
    "uint8x2" => Ok(wgpu::VertexFormat::Uint8x2),
    "uint8x4" => Ok(wgpu::VertexFormat::Uint8x4),
    "sint8" => Ok(wgpu::VertexFormat::Sint8),
    "sint8x2" => Ok(wgpu::VertexFormat::Sint8x2),
    "sint8x4" => Ok(wgpu::VertexFormat::Sint8x4),
    "unorm8" => Ok(wgpu::VertexFormat::Unorm8),
    "unorm8x2" => Ok(wgpu::VertexFormat::Unorm8x2),
    "unorm8x4" => Ok(wgpu::VertexFormat::Unorm8x4),
    "snorm8" => Ok(wgpu::VertexFormat::Snorm8),
    "snorm8x2" => Ok(wgpu::VertexFormat::Snorm8x2),
    "snorm8x4" => Ok(wgpu::VertexFormat::Snorm8x4),
    "uint16" => Ok(wgpu::VertexFormat::Uint16),
    "uint16x2" => Ok(wgpu::VertexFormat::Uint16x2),
    "uint16x4" => Ok(wgpu::VertexFormat::Uint16x4),
    "sint16" => Ok(wgpu::VertexFormat::Sint16),
    "sint16x2" => Ok(wgpu::VertexFormat::Sint16x2),
    "sint16x4" => Ok(wgpu::VertexFormat::Sint16x4),
    "unorm16" => Ok(wgpu::VertexFormat::Unorm16),
    "unorm16x2" => Ok(wgpu::VertexFormat::Unorm16x2),
    "unorm16x4" => Ok(wgpu::VertexFormat::Unorm16x4),
    "snorm16" => Ok(wgpu::VertexFormat::Snorm16),
    "snorm16x2" => Ok(wgpu::VertexFormat::Snorm16x2),
    "snorm16x4" => Ok(wgpu::VertexFormat::Snorm16x4),
    "float16" => Ok(wgpu::VertexFormat::Float16),
    "float16x2" => Ok(wgpu::VertexFormat::Float16x2),
    "float16x4" => Ok(wgpu::VertexFormat::Float16x4),
    "float32" => Ok(wgpu::VertexFormat::Float32),
    "float32x2" => Ok(wgpu::VertexFormat::Float32x2),
    "float32x3" => Ok(wgpu::VertexFormat::Float32x3),
    "float32x4" => Ok(wgpu::VertexFormat::Float32x4),
    "uint32" => Ok(wgpu::VertexFormat::Uint32),
    "uint32x2" => Ok(wgpu::VertexFormat::Uint32x2),
    "uint32x3" => Ok(wgpu::VertexFormat::Uint32x3),
    "uint32x4" => Ok(wgpu::VertexFormat::Uint32x4),
    "sint32" => Ok(wgpu::VertexFormat::Sint32),
    "sint32x2" => Ok(wgpu::VertexFormat::Sint32x2),
    "sint32x3" => Ok(wgpu::VertexFormat::Sint32x3),
    "sint32x4" => Ok(wgpu::VertexFormat::Sint32x4),
    "unorm10-10-10-2" => Ok(wgpu::VertexFormat::Unorm10_10_10_2),
    "unorm8x4-bgra" => Ok(wgpu::VertexFormat::Unorm8x4Bgra),
    // wgpu 30 has no packed signed 10-10-10-2 vertex format.
    "snorm10-10-10-2" => Err(js::unsupported("snorm10-10-10-2")),
    _ => Err(OpError::type_("The vertex format is invalid")),
  }
}

fn filter_of(
  scope: &mut v8::PinScope,
  object: Option<v8::Local<v8::Object>>,
  name: &str,
) -> Result<wgpu::FilterMode, OpError> {
  match sampler_string(scope, object, name)?.as_deref() {
    None | Some("nearest") => Ok(wgpu::FilterMode::Nearest),
    Some("linear") => Ok(wgpu::FilterMode::Linear),
    _ => Err(OpError::type_("The sampler filter is invalid")),
  }
}

fn mipmap_filter_of(
  scope: &mut v8::PinScope,
  object: Option<v8::Local<v8::Object>>,
) -> Result<wgpu::MipmapFilterMode, OpError> {
  match sampler_string(scope, object, "mipmapFilter")?.as_deref() {
    None | Some("nearest") => Ok(wgpu::MipmapFilterMode::Nearest),
    Some("linear") => Ok(wgpu::MipmapFilterMode::Linear),
    _ => Err(OpError::type_("The sampler filter is invalid")),
  }
}

fn address_mode_of(
  scope: &mut v8::PinScope,
  object: Option<v8::Local<v8::Object>>,
  name: &str,
) -> Result<wgpu::AddressMode, OpError> {
  match sampler_string(scope, object, name)?.as_deref() {
    None | Some("clamp-to-edge") => Ok(wgpu::AddressMode::ClampToEdge),
    Some("repeat") => Ok(wgpu::AddressMode::Repeat),
    Some("mirror-repeat") => Ok(wgpu::AddressMode::MirrorRepeat),
    _ => Err(OpError::type_("The address mode is invalid")),
  }
}

fn lod_clamp_of(
  scope: &mut v8::PinScope,
  object: Option<v8::Local<v8::Object>>,
  name: &str,
  default: f32,
) -> Result<f32, OpError> {
  let Some(object) = object else {
    return Ok(default);
  };
  Ok(js::field_f32(scope, object, name)?.unwrap_or(default))
}

fn anisotropy_of(
  scope: &mut v8::PinScope,
  object: Option<v8::Local<v8::Object>>,
) -> Result<u16, OpError> {
  let Some(object) = object else {
    return Ok(1);
  };
  let Some(value) = js::get(scope, object, "maxAnisotropy")? else {
    return Ok(1);
  };
  js::clamp_u16(scope, value)
}

fn sampler_string(
  scope: &mut v8::PinScope,
  object: Option<v8::Local<v8::Object>>,
  name: &str,
) -> Result<Option<String>, OpError> {
  let Some(object) = object else {
    return Ok(None);
  };
  js::get(scope, object, name)?
    .map(|value| js::string(scope, value))
    .transpose()
}

const MAP_READ_BIT: u32 = 0x0001;
const MAP_WRITE_BIT: u32 = 0x0002;
const COPY_SRC_BIT: u32 = 0x0004;
const COPY_DST_BIT: u32 = 0x0008;
const MAP_MODE_MASK: u32 = MAP_READ_BIT | MAP_WRITE_BIT;
const BUFFER_USAGE_MASK: u32 = 0x03FF;

fn buffer_usage_mismatch(usage: u32) -> bool {
  let read = (usage & MAP_READ_BIT) != 0
    && (usage & !(MAP_READ_BIT | COPY_DST_BIT)) != 0;
  let write = (usage & MAP_WRITE_BIT) != 0
    && (usage & !(MAP_WRITE_BIT | COPY_SRC_BIT)) != 0;
  read || write
}

/// Bytes of a `mappedAtCreation` buffer. Size 0 is mapped and is not sliced.
/// An error buffer that wgpu did not map still exposes a zeroed range.
fn mapped_creation_bytes(buffer: &wgpu::Buffer, size: u64) -> (Vec<u8>, bool) {
  if size == 0 {
    return (Vec::new(), true);
  }
  match buffer.slice(0..size).get_mapped_range() {
    Ok(view) => (view.to_vec(), true),
    Err(_) => {
      buffer.unmap();
      let len = usize::try_from(size).unwrap_or(0);
      (vec![0; len], false)
    }
  }
}

fn checked_map_size(
  buffer_size: u64,
  offset: u64,
  size: Option<u64>,
) -> Result<u64, OpError> {
  if !offset.is_multiple_of(8) || offset > buffer_size {
    return Err(OpError::dom(
      "OperationError",
      "The map offset must be a multiple of 8 inside the buffer",
    ));
  }
  let size = size.unwrap_or(buffer_size - offset);
  if size == 0 || !size.is_multiple_of(4) || offset + size > buffer_size {
    return Err(OpError::dom(
      "OperationError",
      "The map size must be a non-zero multiple of 4 inside the buffer",
    ));
  }
  Ok(size)
}

fn absorb_views(scope: &mut v8::PinScope, mapped: &mut Mapped) {
  for view in &mapped.views {
    let local = v8::Local::new(scope, &view.buffer);
    if local.was_detached() {
      continue;
    }
    let length = local.byte_length();
    let start = view.offset as usize;
    let end = start.saturating_add(length);
    if length != view.size as usize || end > mapped.bytes.len() {
      continue;
    }
    let Some(data) = local.data() else {
      continue;
    };
    unsafe {
      std::ptr::copy_nonoverlapping(
        data.as_ptr().cast::<u8>(),
        mapped.bytes[start..end].as_mut_ptr(),
        length,
      );
    }
  }
}

fn detach_views(scope: &mut v8::PinScope, mapped: &Mapped) {
  for view in &mapped.views {
    let local = v8::Local::new(scope, &view.buffer);
    if !local.was_detached() {
      let _ = local.detach(None);
    }
  }
}

pub(crate) fn optional_label(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
) -> Result<Option<String>, OpError> {
  js::get(scope, object, "label")?
    .map(|value| js::string(scope, value))
    .transpose()
}

fn required_string(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
) -> Result<String, OpError> {
  js::get(scope, object, name)?
    .map(|value| js::string(scope, value))
    .transpose()?
    .ok_or_else(|| OpError::type_(format!("{name} is required")))
}

fn check_compilation_hints(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
) -> Result<(), OpError> {
  let Some(value) = js::try_get(scope, object, "compilationHints")? else {
    return Ok(());
  };
  let value = v8::Local::new(scope, value);
  let array = v8::Local::<v8::Array>::try_from(value)
    .map_err(|_| OpError::type_("compilationHints must be an array"))?;
  for index in 0..array.length() {
    let item = js::read_index(scope, array, index)?;
    let hint = js::object(scope, item)
      .ok_or_else(|| OpError::type_("A compilation hint must be an object"))?;
    let entry = js::try_get(scope, hint, "entryPoint")?.ok_or_else(|| {
      OpError::type_("A compilation hint entryPoint is required")
    })?;
    let entry = v8::Local::new(scope, entry);
    let _entry = js::try_string(scope, entry)?;
    let Some(layout) = js::try_get(scope, hint, "layout")? else {
      continue;
    };
    let layout = v8::Local::new(scope, layout);
    if layout.is_null() {
      return Err(OpError::type_("A compilation hint layout is invalid"));
    }
    if layout.is_string() {
      if js::try_string(scope, layout)? != "auto" {
        return Err(OpError::type_("A compilation hint layout is invalid"));
      }
    } else if host_pipeline_layout(scope, layout).is_err() {
      return Err(OpError::type_("A compilation hint layout is invalid"));
    }
  }
  Ok(())
}

/// Enumerable own string keys. A value is a finite WebIDL double.
fn pipeline_constants(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
) -> Result<Vec<(String, f64)>, OpError> {
  let Some(value) = js::try_get(scope, object, "constants")? else {
    return Ok(Vec::new());
  };
  let value = v8::Local::new(scope, value);
  if value.is_null() || !value.is_object() {
    return Err(OpError::type_("Pipeline constants must be an object"));
  }
  let record = v8::Local::<v8::Object>::try_from(value)
    .map_err(|_| OpError::type_("Pipeline constants must be an object"))?;
  let args = v8::GetPropertyNamesArgsBuilder::new()
    .property_filter(
      v8::PropertyFilter::ONLY_ENUMERABLE | v8::PropertyFilter::SKIP_SYMBOLS,
    )
    .key_conversion(v8::KeyConversionMode::ConvertToString)
    .build();
  let names = {
    v8::tc_scope!(let tc, scope);
    match record.get_own_property_names(tc, args) {
      Some(names) => Ok(v8::Global::new(tc, names)),
      None => Err(match tc.exception() {
        Some(thrown) => OpError::thrown(tc, thrown),
        None => OpError::type_("Pipeline constants must be an object"),
      }),
    }
  }?;
  let names = v8::Local::new(scope, names);
  let mut constants = Vec::new();
  for index in 0..names.length() {
    let key = js::read_index(scope, names, index)?;
    let name = js::try_string(scope, key)?;
    let number = {
      v8::tc_scope!(let tc, scope);
      let value = match record.get(tc, key) {
        Some(value) => value,
        None => {
          return Err(match tc.exception() {
            Some(thrown) => OpError::thrown(tc, thrown),
            None => OpError::type_("Pipeline constants must be an object"),
          });
        }
      };
      match value.number_value(tc) {
        Some(number) if number.is_finite() => number,
        Some(_) => {
          return Err(OpError::type_("The provided value is not finite"));
        }
        None => {
          return Err(match tc.exception() {
            Some(thrown) => OpError::thrown(tc, thrown),
            None => OpError::type_("The provided value is not a number"),
          });
        }
      }
    };
    constants.push((name, number));
  }
  Ok(constants)
}

fn constant_pairs(constants: &[(String, f64)]) -> Vec<(&str, f64)> {
  constants
    .iter()
    .map(|(name, value)| (name.as_str(), *value))
    .collect()
}

fn array_of<'a>(
  scope: &mut v8::PinScope,
  object: v8::Local<'a, v8::Object>,
  name: &str,
) -> Result<Vec<v8::Local<'a, v8::Value>>, OpError> {
  let Some(value) = js::get(scope, object, name)? else {
    return Ok(Vec::new());
  };
  let array = v8::Local::<v8::Array>::try_from(value)
    .map_err(|_| OpError::type_(format!("{name} must be an array")))?;
  let mut values = Vec::new();
  for index in 0..array.length() {
    let item = js::read_index(scope, array, index)?;
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
  let value = js::get(scope, object, "module")?
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

fn host_external<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  value: v8::Local<v8::Value>,
) -> Option<&'s GpuExternalTexture> {
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
    label: ObjectLabel::new(None),
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
    label: ObjectLabel::new(None),
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
    label: ObjectLabel::new(None),
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

pub fn host_compute_pipeline<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  value: v8::Local<v8::Value>,
) -> Option<&'s GpuComputePipeline> {
  host_of(scope, value)
}

pub fn host_query_set<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  value: v8::Local<v8::Value>,
) -> Option<&'s GpuQuerySet> {
  host_of(scope, value)
}

pub fn attachment_view(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<(wgpu::TextureView, wgpu::TextureFormat), OpError> {
  if let Some(view) = host_view(scope, value) {
    return Ok((view.view()?, view.format()));
  }
  if let Some(texture) = host_texture(scope, value) {
    let format = texture.format;
    let texture = texture.texture()?;
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    return Ok((view, format));
  }
  Err(OpError::type_("A texture view is required"))
}

pub fn index_format(name: &str) -> Result<wgpu::IndexFormat, OpError> {
  match name {
    "uint16" => Ok(wgpu::IndexFormat::Uint16),
    "uint32" => Ok(wgpu::IndexFormat::Uint32),
    _ => Err(OpError::type_("The index format is invalid")),
  }
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
