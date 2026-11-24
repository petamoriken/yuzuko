//! Command encoders. Passes record commands and replay them on end.
//! A render bundle records the same way and builds the wgpu encoder in
//! `finish`, because `RenderBundleEncoder` cannot be stored across calls.

use std::cell::{Cell, RefCell};
use std::sync::atomic::{AtomicU64, Ordering};

use yuzuko_core::op::{New, OpError};
use yuzuko_macros::op;

use crate::js;
use crate::label::ObjectLabel;
use crate::resource::{self, QueryKind};
use crate::state::Handle;

static NEXT_ENCODER: AtomicU64 = AtomicU64::new(1);
static NEXT_BUNDLE: AtomicU64 = AtomicU64::new(1);

struct EncoderSlot {
  encoder: Option<wgpu::CommandEncoder>,
  pass: Option<OpenPass>,
  features: wgpu::Features,
  device_id: u64,
  invalid: bool,
  submit_destroyed: bool,
  /// `pushDebugGroup` depth. An empty `popDebugGroup` invalidates the encoder.
  debug_depth: u32,
  max_texture_dimension_2d: u32,
  max_vertex_buffers: u32,
}

enum OpenPass {
  Render(PassRecord),
  Compute(ComputeRecord),
}

struct PassRecord {
  colors: Vec<Option<ColorTarget>>,
  depth: Option<DepthTarget>,
  occlusion: Option<wgpu::QuerySet>,
  occlusion_open: bool,
  timestamps: Option<TimestampWrite>,
  commands: Vec<PassCommand>,
  max_draws: u64,
  draws: u64,
  draws_exceeded: bool,
  invalid: bool,
}

struct ColorTarget {
  view: wgpu::TextureView,
  resolve: Option<wgpu::TextureView>,
  clear: Option<wgpu::Color>,
  store: bool,
}

struct DepthTarget {
  view: wgpu::TextureView,
  depth_ops: Option<wgpu::Operations<f32>>,
  stencil_ops: Option<wgpu::Operations<u32>>,
}

struct TimestampWrite {
  query_set: wgpu::QuerySet,
  beginning: Option<u32>,
  end: Option<u32>,
}

enum PassCommand {
  Pipeline(wgpu::RenderPipeline),
  Vertex {
    slot: u32,
    buffer: Option<wgpu::Buffer>,
    offset: u64,
    size: Option<u64>,
  },
  Index {
    buffer: wgpu::Buffer,
    format: wgpu::IndexFormat,
    offset: u64,
    size: Option<u64>,
  },
  Bind {
    index: u32,
    group: Option<wgpu::BindGroup>,
    offsets: Vec<u32>,
  },
  Draw {
    vertices: u32,
    instances: u32,
    first_vertex: u32,
    first_instance: u32,
  },
  DrawIndexed {
    indices: u32,
    instances: u32,
    first_index: u32,
    base_vertex: i32,
    first_instance: u32,
  },
  DrawIndirect {
    buffer: wgpu::Buffer,
    offset: u64,
  },
  DrawIndexedIndirect {
    buffer: wgpu::Buffer,
    offset: u64,
  },
  BeginOcclusion(u32),
  EndOcclusion,
  Bundles(Vec<wgpu::RenderBundle>),
  Viewport {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    min_depth: f32,
    max_depth: f32,
  },
  Scissor {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
  },
  PushDebug(String),
  PopDebug,
  Marker(String),
  WriteTimestamp {
    query_set: wgpu::QuerySet,
    index: u32,
  },
  BlendConstant(wgpu::Color),
  StencilReference(u32),
}

struct ComputeRecord {
  timestamps: Option<TimestampWrite>,
  commands: Vec<ComputeCommand>,
  invalid: bool,
}

enum ComputeCommand {
  Pipeline(wgpu::ComputePipeline),
  Bind {
    index: u32,
    group: Option<wgpu::BindGroup>,
    offsets: Vec<u32>,
  },
  Dispatch {
    x: u32,
    y: u32,
    z: u32,
  },
  DispatchIndirect {
    buffer: wgpu::Buffer,
    offset: u64,
  },
  PushDebug(String),
  PopDebug,
  Marker(String),
  WriteTimestamp {
    query_set: wgpu::QuerySet,
    index: u32,
  },
}

struct BundleSlot {
  device: wgpu::Device,
  device_id: u64,
  colors: Vec<Option<wgpu::TextureFormat>>,
  depth: Option<wgpu::RenderBundleDepthStencil>,
  sample_count: u32,
  commands: Vec<BundleCommand>,
  invalid: bool,
  max_vertex_buffers: u32,
}

enum BundleCommand {
  Pipeline(wgpu::RenderPipeline),
  Vertex {
    slot: u32,
    buffer: Option<wgpu::Buffer>,
    offset: u64,
    size: Option<u64>,
  },
  Index {
    buffer: wgpu::Buffer,
    format: wgpu::IndexFormat,
    offset: u64,
    size: Option<u64>,
  },
  Bind {
    index: u32,
    group: Option<wgpu::BindGroup>,
    offsets: Vec<u32>,
  },
  Draw {
    vertices: u32,
    instances: u32,
    first_vertex: u32,
    first_instance: u32,
  },
  DrawIndexed {
    indices: u32,
    instances: u32,
    first_index: u32,
    base_vertex: i32,
    first_instance: u32,
  },
  DrawIndirect {
    buffer: wgpu::Buffer,
    offset: u64,
  },
  DrawIndexedIndirect {
    buffer: wgpu::Buffer,
    offset: u64,
  },
}

std::thread_local! {
  static ENCODERS: RefCell<std::collections::HashMap<u64, EncoderSlot>> =
    RefCell::new(std::collections::HashMap::new());
  static BUNDLES: RefCell<std::collections::HashMap<u64, BundleSlot>> =
    RefCell::new(std::collections::HashMap::new());
}

pub struct GpuCommandEncoder {
  id: u64,
  label: ObjectLabel,
}

unsafe impl v8::cppgc::GarbageCollected for GpuCommandEncoder {
  fn trace(&self, _visitor: &mut v8::cppgc::Visitor) {}

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"GPUCommandEncoder"
  }
}

impl Drop for GpuCommandEncoder {
  fn drop(&mut self) {
    ENCODERS.with(|encoders| {
      encoders.borrow_mut().remove(&self.id);
    });
  }
}

impl GpuCommandEncoder {
  pub fn create(
    device_id: u64,
    handle: &Handle,
    label: Option<String>,
  ) -> Self {
    let encoder =
      handle
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
          label: label.as_deref(),
        });
    let limits = crate::state::validation_limits(device_id)
      .unwrap_or_else(|_| handle.device.limits());
    let id = NEXT_ENCODER.fetch_add(1, Ordering::Relaxed);
    ENCODERS.with(|encoders| {
      encoders.borrow_mut().insert(
        id,
        EncoderSlot {
          encoder: Some(encoder),
          pass: None,
          features: handle.device.features(),
          device_id,
          invalid: false,
          submit_destroyed: false,
          debug_depth: 0,
          max_texture_dimension_2d: limits.max_texture_dimension_2d,
          max_vertex_buffers: limits.max_vertex_buffers,
        },
      );
    });
    Self {
      id,
      label: ObjectLabel::new(label),
    }
  }
}

#[op(name = "GPUCommandEncoder")]
impl GpuCommandEncoder {
  #[getter]
  fn label(&self) -> String {
    self.label.get()
  }

  #[setter]
  fn label(&self, value: String) {
    self.label.set(value);
  }

  fn begin_render_pass(
    &self,
    scope: &mut v8::PinScope,
    descriptor: v8::Local<v8::Value>,
  ) -> Result<New<GpuRenderPass>, OpError> {
    let object = js::object(scope, descriptor)
      .ok_or_else(|| OpError::type_("beginRenderPass requires a descriptor"))?;
    let colors = color_targets(scope, object)?;
    let depth = depth_target(scope, object)?;
    if colors.iter().all(Option::is_none) && depth.is_none() {
      return Err(OpError::type_(
        "A render pass needs a color or depth attachment",
      ));
    }
    let occlusion = occlusion_query_set(scope, object)?;
    let timestamps = timestamp_writes(scope, object)?;
    let max_draws = max_draw_count(scope, object)?;
    ENCODERS.with(|encoders| {
      let mut encoders = encoders.borrow_mut();
      let slot = encoders.get_mut(&self.id).ok_or_else(|| {
        OpError::dom("InvalidStateError", "The command encoder is finished")
      })?;
      if slot.pass.is_some() {
        return Err(OpError::dom(
          "InvalidStateError",
          "A pass is already open",
        ));
      }
      slot.pass = Some(OpenPass::Render(PassRecord {
        colors,
        depth,
        occlusion,
        occlusion_open: false,
        timestamps,
        commands: Vec::new(),
        max_draws,
        draws: 0,
        draws_exceeded: false,
        invalid: false,
      }));
      Ok(())
    })?;
    Ok(New(GpuRenderPass {
      encoder: self.id,
      ended: Cell::new(false),
      label: ObjectLabel::new(resource::optional_label(scope, object)?),
    }))
  }

  fn begin_compute_pass(
    &self,
    scope: &mut v8::PinScope,
    descriptor: Option<v8::Local<v8::Value>>,
  ) -> Result<New<GpuComputePass>, OpError> {
    let (timestamps, label) = match descriptor {
      Some(value) if !value.is_null_or_undefined() => {
        let object = js::object(scope, value).ok_or_else(|| {
          OpError::type_("beginComputePass descriptor must be an object")
        })?;
        (
          timestamp_writes(scope, object)?,
          resource::optional_label(scope, object)?,
        )
      }
      _ => (None, None),
    };
    ENCODERS.with(|encoders| {
      let mut encoders = encoders.borrow_mut();
      let slot = encoders.get_mut(&self.id).ok_or_else(|| {
        OpError::dom("InvalidStateError", "The command encoder is finished")
      })?;
      if slot.pass.is_some() {
        return Err(OpError::dom(
          "InvalidStateError",
          "A pass is already open",
        ));
      }
      slot.pass = Some(OpenPass::Compute(ComputeRecord {
        timestamps,
        commands: Vec::new(),
        invalid: false,
      }));
      Ok(())
    })?;
    Ok(New(GpuComputePass {
      encoder: self.id,
      ended: Cell::new(false),
      label: ObjectLabel::new(label),
    }))
  }

  #[length(2)]
  fn copy_buffer_to_buffer(
    &self,
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<(), OpError> {
    let argc = args.length();
    if argc < 2 {
      return Err(OpError::type_("Not enough arguments"));
    }
    let source =
      host_gpu_buffer(scope, args.get(0), "The copy source is not a buffer")?;
    let second = args.get(1);
    let (source_offset, destination, destination_offset, size) =
      if resource::host_buffer(scope, second).is_some() {
        let size = if argc >= 3 && !args.get(2).is_undefined() {
          Some(js::u64_value(scope, args.get(2))?)
        } else {
          None
        };
        (0, second, 0, size)
      } else {
        if argc < 4 {
          return Err(OpError::type_("Not enough arguments"));
        }
        let source_offset = js::u64_value(scope, second)?;
        let destination_offset = js::u64_value(scope, args.get(3))?;
        let size = if argc >= 5 && !args.get(4).is_undefined() {
          Some(js::u64_value(scope, args.get(4))?)
        } else {
          None
        };
        (source_offset, args.get(2), destination_offset, size)
      };
    let destination = host_gpu_buffer(
      scope,
      destination,
      "The copy destination is not a buffer",
    )?;
    source.alive()?;
    destination.alive()?;
    let encoder_device = encoder_device(self.id)?;
    let mismatch = source.device_id() != encoder_device
      || destination.device_id() != encoder_device;
    // An invalid buffer fails at finish, including when the other buffer is
    // destroyed. A destroyed buffer alone fails at submit.
    if mismatch || !source.is_valid() || !destination.is_valid() {
      return invalidate_encoder(self.id);
    }
    if source.is_destroyed() || destination.is_destroyed() {
      return note_submit_destroyed(self.id);
    }
    let destination_buffer = destination.alive()?;
    let source_buffer = source.alive()?;
    with_idle_encoder(self.id, |encoder| {
      encoder.copy_buffer_to_buffer(
        &source_buffer,
        source_offset,
        &destination_buffer,
        destination_offset,
        size,
      );
      Ok(())
    })
  }

  fn copy_texture_to_buffer(
    &self,
    scope: &mut v8::PinScope,
    source: v8::Local<v8::Value>,
    destination: v8::Local<v8::Value>,
    size: v8::Local<v8::Value>,
  ) -> Result<(), OpError> {
    let source = js::object(scope, source).ok_or_else(|| {
      OpError::type_("copyTextureToBuffer source must be an object")
    })?;
    let texture = js::get(scope, source, "texture")?
      .and_then(|value| resource::host_texture(scope, value))
      .ok_or_else(|| OpError::type_("The copy source is not a texture"))?;
    let texture = texture.texture()?;
    let destination = js::object(scope, destination).ok_or_else(|| {
      OpError::type_("copyTextureToBuffer destination must be an object")
    })?;
    let buffer = js::get(scope, destination, "buffer")?
      .and_then(|value| resource::host_buffer(scope, value))
      .ok_or_else(|| OpError::type_("The copy destination is not a buffer"))?;
    let buffer = buffer.alive()?;
    let offset = js::field_u64(scope, destination, "offset")?.unwrap_or(0);
    let bytes_per_row = js::field_u32(scope, destination, "bytesPerRow")?;
    let (width, height) = copy_size(scope, size)?;
    with_idle_encoder(self.id, |encoder| {
      encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
          buffer: &buffer,
          layout: wgpu::TexelCopyBufferLayout {
            offset,
            bytes_per_row,
            rows_per_image: None,
          },
        },
        wgpu::Extent3d {
          width,
          height,
          depth_or_array_layers: 1,
        },
      );
      Ok(())
    })
  }

  fn clear_buffer(
    &self,
    scope: &mut v8::PinScope,
    buffer: v8::Local<v8::Value>,
    offset: Option<js::GpuSize64>,
    size: Option<js::GpuSize64>,
  ) -> Result<(), OpError> {
    let buffer =
      host_gpu_buffer(scope, buffer, "clearBuffer requires a GPUBuffer")?;
    buffer.alive()?;
    let encoder_device = encoder_device(self.id)?;
    if buffer.device_id() != encoder_device || !buffer.is_valid() {
      return invalidate_encoder(self.id);
    }
    if buffer.is_destroyed() {
      return note_submit_destroyed(self.id);
    }
    let gpu = buffer.alive()?;
    let offset = offset.map(|value| value.0).unwrap_or(0);
    let size = size.map(|value| value.0);
    with_idle_encoder(self.id, |encoder| {
      encoder.clear_buffer(&gpu, offset, size);
      Ok(())
    })
  }

  fn copy_buffer_to_texture(
    &self,
    scope: &mut v8::PinScope,
    source: v8::Local<v8::Value>,
    destination: v8::Local<v8::Value>,
    size: v8::Local<v8::Value>,
  ) -> Result<(), OpError> {
    let source = js::object(scope, source).ok_or_else(|| {
      OpError::type_("copyBufferToTexture source must be an object")
    })?;
    let buffer = js::get(scope, source, "buffer")?
      .and_then(|value| resource::host_buffer(scope, value))
      .ok_or_else(|| OpError::type_("The copy source is not a buffer"))?;
    let buffer = buffer.alive()?;
    let offset = js::field_u64(scope, source, "offset")?.unwrap_or(0);
    let bytes_per_row = js::field_u32(scope, source, "bytesPerRow")?;
    let rows_per_image = js::field_u32(scope, source, "rowsPerImage")?;
    let destination = js::object(scope, destination).ok_or_else(|| {
      OpError::type_("copyBufferToTexture destination must be an object")
    })?;
    let texture = js::get(scope, destination, "texture")?
      .and_then(|value| resource::host_texture(scope, value))
      .ok_or_else(|| OpError::type_("The copy destination is not a texture"))?;
    let texture = texture.texture()?;
    let mip_level = js::field_u32(scope, destination, "mipLevel")?.unwrap_or(0);
    let origin = copy_origin(scope, destination)?;
    let aspect = match js::get(scope, destination, "aspect")?
      .map(|value| js::string(scope, value))
      .transpose()?
      .as_deref()
    {
      None => wgpu::TextureAspect::All,
      Some(name) => resource::texture_aspect(name)?,
    };
    let extent = copy_extent(scope, size)?;
    with_idle_encoder(self.id, |encoder| {
      encoder.copy_buffer_to_texture(
        wgpu::TexelCopyBufferInfo {
          buffer: &buffer,
          layout: wgpu::TexelCopyBufferLayout {
            offset,
            bytes_per_row,
            rows_per_image,
          },
        },
        wgpu::TexelCopyTextureInfo {
          texture: &texture,
          mip_level,
          origin,
          aspect,
        },
        extent,
      );
      Ok(())
    })
  }

  fn copy_texture_to_texture(
    &self,
    scope: &mut v8::PinScope,
    source: v8::Local<v8::Value>,
    destination: v8::Local<v8::Value>,
    copy_size: v8::Local<v8::Value>,
  ) -> Result<(), OpError> {
    let source = copy_texture(scope, source, "source")?;
    let destination = copy_texture(scope, destination, "destination")?;
    let extent = copy_extent(scope, copy_size)?;
    with_idle_encoder(self.id, |encoder| {
      encoder.copy_texture_to_texture(
        wgpu::TexelCopyTextureInfo {
          texture: &source.texture,
          mip_level: source.mip_level,
          origin: source.origin,
          aspect: source.aspect,
        },
        wgpu::TexelCopyTextureInfo {
          texture: &destination.texture,
          mip_level: destination.mip_level,
          origin: destination.origin,
          aspect: destination.aspect,
        },
        extent,
      );
      Ok(())
    })
  }

  fn resolve_query_set(
    &self,
    scope: &mut v8::PinScope,
    query_set: v8::Local<v8::Value>,
    first_query: js::GpuSize32,
    query_count: js::GpuSize32,
    destination: v8::Local<v8::Value>,
    destination_offset: js::GpuSize64,
  ) -> Result<(), OpError> {
    let query_set =
      resource::host_query_set(scope, query_set).ok_or_else(|| {
        OpError::type_("resolveQuerySet requires a GPUQuerySet")
      })?;
    let destination = host_gpu_buffer(
      scope,
      destination,
      "The query destination is not a buffer",
    )?;
    destination.alive()?;
    let first = first_query.0;
    let count = query_count.0;
    if query_set.is_destroyed() {
      let stored = query_set.query_count();
      let query_set = query_set.query_set();
      let destination = destination.alive()?;
      // wgpu has no `QuerySet::destroy`. An empty range does not overrun, so
      // a destroyed set resolves one index past the end. `u32::MAX` cannot.
      let (start, end, offset) = if stored < u32::MAX {
        (stored, stored + 1, destination_offset.0)
      } else {
        (first, first.saturating_add(count), 1)
      };
      return with_idle_encoder(self.id, |encoder| {
        encoder.resolve_query_set(&query_set, start..end, &destination, offset);
        Ok(())
      });
    }
    let stored = query_set.query_count();
    let end = first.checked_add(count);
    let range_ok = end.is_some_and(|end| first < stored && end <= stored);
    let bytes = u64::from(count).saturating_mul(8);
    let offset = destination_offset.0;
    let fits = offset.is_multiple_of(256)
      && offset.saturating_add(bytes) <= destination.byte_size();
    let usage_ok = destination.is_valid()
      && destination.usage_bits() & wgpu::BufferUsages::QUERY_RESOLVE.bits()
        != 0;
    if range_ok && fits && usage_ok && destination.is_destroyed() {
      return note_submit_destroyed(self.id);
    }
    if !range_ok || !fits || !usage_ok {
      return invalidate_encoder(self.id);
    }
    let query_set = query_set.query_set();
    let destination = destination.alive()?;
    let end = first + count;
    with_idle_encoder(self.id, |encoder| {
      encoder.resolve_query_set(&query_set, first..end, &destination, offset);
      Ok(())
    })
  }

  fn push_debug_group(&self, label: String) -> Result<(), OpError> {
    with_idle_encoder(self.id, |encoder| {
      encoder.push_debug_group(&label);
      Ok(())
    })?;
    ENCODERS.with(|encoders| {
      if let Some(slot) = encoders.borrow_mut().get_mut(&self.id) {
        slot.debug_depth = slot.debug_depth.saturating_add(1);
      }
    });
    Ok(())
  }

  fn pop_debug_group(&self) -> Result<(), OpError> {
    let skip_wgpu = ENCODERS.with(|encoders| {
      let mut encoders = encoders.borrow_mut();
      let slot = encoders.get_mut(&self.id).ok_or_else(|| {
        OpError::dom("InvalidStateError", "The command encoder is finished")
      })?;
      if slot.pass.is_some() {
        return Err(OpError::dom("InvalidStateError", "A pass is open"));
      }
      if slot.encoder.is_none() {
        return Err(OpError::dom(
          "InvalidStateError",
          "The command encoder is finished",
        ));
      }
      if slot.debug_depth == 0 {
        slot.invalid = true;
        Ok(true)
      } else {
        slot.debug_depth -= 1;
        Ok(false)
      }
    })?;
    if skip_wgpu {
      return Ok(());
    }
    with_idle_encoder(self.id, |encoder| {
      encoder.pop_debug_group();
      Ok(())
    })
  }

  fn insert_debug_marker(&self, label: String) -> Result<(), OpError> {
    with_idle_encoder(self.id, |encoder| {
      encoder.insert_debug_marker(&label);
      Ok(())
    })
  }

  fn write_timestamp(
    &self,
    scope: &mut v8::PinScope,
    query_set: v8::Local<v8::Value>,
    query_index: js::GpuSize32,
  ) -> Result<(), OpError> {
    require_timestamp_write(
      encoder_features(self.id)?,
      wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS,
    )?;
    let (set, index) = timestamp_query(scope, query_set, query_index.0)?;
    with_idle_encoder(self.id, |encoder| {
      encoder.write_timestamp(&set, index);
      Ok(())
    })
  }

  fn finish(
    &self,
    scope: &mut v8::PinScope,
    descriptor: Option<v8::Local<v8::Value>>,
  ) -> Result<New<GpuCommandBuffer>, OpError> {
    let (encoder, invalid, device_id, submit_destroyed) =
      ENCODERS.with(|encoders| {
        let mut encoders = encoders.borrow_mut();
        let slot = encoders.get_mut(&self.id).ok_or_else(|| {
          OpError::dom("InvalidStateError", "The command encoder is finished")
        })?;
        if slot.pass.is_some() {
          return Err(OpError::dom("InvalidStateError", "A pass is open"));
        }
        let encoder = slot.encoder.take().ok_or_else(|| {
          OpError::dom("InvalidStateError", "The command encoder is finished")
        })?;
        Ok((encoder, slot.invalid, slot.device_id, slot.submit_destroyed))
      })?;
    if invalid {
      crate::state::report_validation(
        device_id,
        "The command encoder is invalid",
      );
    }
    Ok(New(GpuCommandBuffer {
      buffer: RefCell::new(Some(encoder.finish())),
      submit_destroyed: Cell::new(submit_destroyed),
      invalid: Cell::new(invalid),
      device_id,
      label: ObjectLabel::new(descriptor_label(scope, descriptor)?),
    }))
  }
}

pub struct GpuCommandBuffer {
  buffer: RefCell<Option<wgpu::CommandBuffer>>,
  submit_destroyed: Cell<bool>,
  invalid: Cell<bool>,
  device_id: u64,
  label: ObjectLabel,
}

unsafe impl v8::cppgc::GarbageCollected for GpuCommandBuffer {
  fn trace(&self, _visitor: &mut v8::cppgc::Visitor) {}

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"GPUCommandBuffer"
  }
}

impl GpuCommandBuffer {
  pub fn take(&self) -> Option<(wgpu::CommandBuffer, bool, bool)> {
    let buffer = self.buffer.borrow_mut().take()?;
    Some((buffer, self.submit_destroyed.get(), self.invalid.get()))
  }

  pub fn device_id(&self) -> u64 {
    self.device_id
  }
}

#[op(name = "GPUCommandBuffer")]
impl GpuCommandBuffer {
  #[getter]
  fn label(&self) -> String {
    self.label.get()
  }

  #[setter]
  fn label(&self, value: String) {
    self.label.set(value);
  }
}

pub struct GpuRenderPass {
  encoder: u64,
  ended: Cell<bool>,
  label: ObjectLabel,
}

unsafe impl v8::cppgc::GarbageCollected for GpuRenderPass {
  fn trace(&self, _visitor: &mut v8::cppgc::Visitor) {}

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"GPURenderPassEncoder"
  }
}

#[op(name = "GPURenderPassEncoder")]
impl GpuRenderPass {
  #[getter]
  fn label(&self) -> String {
    self.label.get()
  }

  #[setter]
  fn label(&self, value: String) {
    self.label.set(value);
  }

  fn set_viewport(
    &self,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    min_depth: f64,
    max_depth: f64,
  ) -> Result<(), OpError> {
    let values = viewport_finite(x, y, width, height, min_depth, max_depth)?;
    let max_dimension = encoder_limits(self.encoder)?.0;
    self.with_render(|record| {
      match viewport_cast(values, max_dimension) {
        Some((x, y, width, height, min_depth, max_depth)) => {
          record.commands.push(PassCommand::Viewport {
            x,
            y,
            width,
            height,
            min_depth,
            max_depth,
          });
        }
        None => record.invalid = true,
      }
      Ok(())
    })
  }

  fn set_scissor_rect(
    &self,
    x: js::GpuIntegerCoordinate,
    y: js::GpuIntegerCoordinate,
    width: js::GpuIntegerCoordinate,
    height: js::GpuIntegerCoordinate,
  ) -> Result<(), OpError> {
    self.push(PassCommand::Scissor {
      x: x.0,
      y: y.0,
      width: width.0,
      height: height.0,
    })
  }

  fn set_pipeline(
    &self,
    scope: &mut v8::PinScope,
    pipeline: v8::Local<v8::Value>,
  ) -> Result<(), OpError> {
    let pipeline =
      resource::host_pipeline(scope, pipeline).ok_or_else(|| {
        OpError::type_("setPipeline requires a GPURenderPipeline")
      })?;
    self.push(PassCommand::Pipeline(pipeline.pipeline().clone()))
  }

  fn set_vertex_buffer(
    &self,
    scope: &mut v8::PinScope,
    slot: js::GpuIndex32,
    buffer: v8::Local<v8::Value>,
    offset: Option<js::GpuSize64>,
    size: Option<js::GpuSize64>,
  ) -> Result<(), OpError> {
    let max_vertex_buffers = encoder_limits(self.encoder)?.1;
    let device_id = encoder_device(self.encoder)?;
    let bind = vertex_bind(
      scope,
      slot.0,
      buffer,
      offset,
      size,
      max_vertex_buffers,
      device_id,
    )?;
    self.with_render(|record| {
      push_vertex(record, bind);
      Ok(())
    })
  }

  fn set_index_buffer(
    &self,
    scope: &mut v8::PinScope,
    buffer: v8::Local<v8::Value>,
    format: String,
    offset: Option<js::GpuSize64>,
    size: Option<js::GpuSize64>,
  ) -> Result<(), OpError> {
    let device_id = encoder_device(self.encoder)?;
    let bind = index_bind(scope, buffer, &format, offset, size, device_id)?;
    self.with_render(|record| {
      push_index(record, bind);
      Ok(())
    })
  }

  fn set_bind_group(
    &self,
    scope: &mut v8::PinScope,
    index: js::GpuIndex32,
    group: v8::Local<v8::Value>,
    offsets: Option<v8::Local<v8::Value>>,
    start: Option<js::GpuSize64>,
    length: Option<js::GpuSize32>,
  ) -> Result<(), OpError> {
    let (group, offsets) = bind_group(
      scope,
      group,
      offsets,
      start.map(|value| value.0),
      length.map(|value| value.0),
    )?;
    self.push(PassCommand::Bind {
      index: index.0,
      group,
      offsets,
    })
  }

  fn draw(
    &self,
    vertices: js::GpuSize32,
    instances: Option<js::GpuSize32>,
    first_vertex: Option<js::GpuSize32>,
    first_instance: Option<js::GpuSize32>,
  ) -> Result<(), OpError> {
    self.push_draw(PassCommand::Draw {
      vertices: vertices.0,
      instances: instances.map(|value| value.0).unwrap_or(1),
      first_vertex: first_vertex.map(|value| value.0).unwrap_or(0),
      first_instance: first_instance.map(|value| value.0).unwrap_or(0),
    })
  }

  fn draw_indexed(
    &self,
    indices: js::GpuSize32,
    instances: Option<js::GpuSize32>,
    first_index: Option<js::GpuSize32>,
    base_vertex: Option<js::GpuSignedOffset32>,
    first_instance: Option<js::GpuSize32>,
  ) -> Result<(), OpError> {
    self.push_draw(PassCommand::DrawIndexed {
      indices: indices.0,
      instances: instances.map(|value| value.0).unwrap_or(1),
      first_index: first_index.map(|value| value.0).unwrap_or(0),
      base_vertex: base_vertex.map(|value| value.0).unwrap_or(0),
      first_instance: first_instance.map(|value| value.0).unwrap_or(0),
    })
  }

  fn draw_indirect(
    &self,
    scope: &mut v8::PinScope,
    buffer: v8::Local<v8::Value>,
    offset: js::GpuSize64,
  ) -> Result<(), OpError> {
    let bind = indirect_bind(
      scope,
      buffer,
      offset.0,
      16,
      "drawIndirect requires a GPUBuffer",
    )?;
    self.with_render(|record| {
      match bind {
        Some((buffer, offset)) => {
          note_draws(record, 1);
          record
            .commands
            .push(PassCommand::DrawIndirect { buffer, offset });
        }
        None => record.invalid = true,
      }
      Ok(())
    })
  }

  fn draw_indexed_indirect(
    &self,
    scope: &mut v8::PinScope,
    buffer: v8::Local<v8::Value>,
    offset: js::GpuSize64,
  ) -> Result<(), OpError> {
    let bind = indirect_bind(
      scope,
      buffer,
      offset.0,
      20,
      "drawIndexedIndirect requires a GPUBuffer",
    )?;
    self.with_render(|record| {
      match bind {
        Some((buffer, offset)) => {
          note_draws(record, 1);
          record
            .commands
            .push(PassCommand::DrawIndexedIndirect { buffer, offset });
        }
        None => record.invalid = true,
      }
      Ok(())
    })
  }

  fn begin_occlusion_query(
    &self,
    query_index: js::GpuSize32,
  ) -> Result<(), OpError> {
    self.with_render(|record| {
      if record.occlusion.is_none() {
        return Err(OpError::dom(
          "InvalidStateError",
          "The render pass has no occlusion query set",
        ));
      }
      if record.occlusion_open {
        return Err(OpError::dom(
          "InvalidStateError",
          "An occlusion query is already open",
        ));
      }
      record.occlusion_open = true;
      record
        .commands
        .push(PassCommand::BeginOcclusion(query_index.0));
      Ok(())
    })
  }

  fn end_occlusion_query(&self) -> Result<(), OpError> {
    self.with_render(|record| {
      if !record.occlusion_open {
        return Err(OpError::dom(
          "InvalidStateError",
          "An occlusion query is not open",
        ));
      }
      record.occlusion_open = false;
      record.commands.push(PassCommand::EndOcclusion);
      Ok(())
    })
  }

  fn execute_bundles(
    &self,
    scope: &mut v8::PinScope,
    bundles: v8::Local<v8::Value>,
  ) -> Result<(), OpError> {
    let array = v8::Local::<v8::Array>::try_from(bundles)
      .map_err(|_| OpError::type_("executeBundles requires a sequence"))?;
    let mut recorded = Vec::new();
    let mut draws = 0u64;
    for index in 0..array.length() {
      let value = js::read_index(scope, array, index)?;
      let bundle = host_bundle(scope, value).ok_or_else(|| {
        OpError::type_("executeBundles requires GPURenderBundle objects")
      })?;
      draws = draws.saturating_add(bundle.draws);
      recorded.push(bundle.bundle.clone());
    }
    self.with_render(|record| {
      note_draws(record, draws);
      record.commands.push(PassCommand::Bundles(recorded));
      Ok(())
    })
  }

  fn push_debug_group(&self, label: String) -> Result<(), OpError> {
    self.push(PassCommand::PushDebug(label))
  }

  fn pop_debug_group(&self) -> Result<(), OpError> {
    self.push(PassCommand::PopDebug)
  }

  fn insert_debug_marker(&self, label: String) -> Result<(), OpError> {
    self.push(PassCommand::Marker(label))
  }

  fn write_timestamp(
    &self,
    scope: &mut v8::PinScope,
    query_set: v8::Local<v8::Value>,
    query_index: js::GpuSize32,
  ) -> Result<(), OpError> {
    require_timestamp_write(
      encoder_features(self.encoder)?,
      wgpu::Features::TIMESTAMP_QUERY_INSIDE_PASSES,
    )?;
    let (set, index) = timestamp_query(scope, query_set, query_index.0)?;
    self.push(PassCommand::WriteTimestamp {
      query_set: set,
      index,
    })
  }

  fn set_blend_constant(
    &self,
    scope: &mut v8::PinScope,
    color: v8::Local<v8::Value>,
  ) -> Result<(), OpError> {
    let color = blend_color(scope, color)?;
    self.push(PassCommand::BlendConstant(color))
  }

  fn set_stencil_reference(&self, reference: u32) -> Result<(), OpError> {
    self.push(PassCommand::StencilReference(reference))
  }

  fn end(&self) -> Result<(), OpError> {
    if self.ended.get() {
      return Err(OpError::dom(
        "InvalidStateError",
        "The render pass has ended",
      ));
    }
    self.ended.set(true);
    ENCODERS.with(|encoders| {
      let mut encoders = encoders.borrow_mut();
      let slot = encoders.get_mut(&self.encoder).ok_or_else(|| {
        OpError::dom("InvalidStateError", "The command encoder is finished")
      })?;
      let record = match slot.pass.take() {
        Some(OpenPass::Render(record)) => record,
        _ => {
          return Err(OpError::dom(
            "InvalidStateError",
            "The render pass has ended",
          ));
        }
      };
      if record.occlusion_open {
        return Err(OpError::dom(
          "InvalidStateError",
          "An occlusion query is still open",
        ));
      }
      if record.invalid {
        slot.invalid = true;
      }
      let encoder = slot.encoder.as_mut().ok_or_else(|| {
        OpError::dom("InvalidStateError", "The command encoder is finished")
      })?;
      let replayed =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
          replay_render(encoder, &record)
        }));
      replayed.map_err(|_| {
        OpError::dom("OperationError", "The render pass is invalid")
      })
    })
  }
}

impl GpuRenderPass {
  fn push(&self, command: PassCommand) -> Result<(), OpError> {
    self.with_render(|record| {
      record.commands.push(command);
      Ok(())
    })
  }

  fn push_draw(&self, command: PassCommand) -> Result<(), OpError> {
    self.with_render(|record| {
      note_draws(record, 1);
      record.commands.push(command);
      Ok(())
    })
  }

  fn with_render(
    &self,
    body: impl FnOnce(&mut PassRecord) -> Result<(), OpError>,
  ) -> Result<(), OpError> {
    if self.ended.get() {
      return Err(OpError::dom(
        "InvalidStateError",
        "The render pass has ended",
      ));
    }
    ENCODERS.with(|encoders| {
      let mut encoders = encoders.borrow_mut();
      let slot = encoders.get_mut(&self.encoder).ok_or_else(|| {
        OpError::dom("InvalidStateError", "The command encoder is finished")
      })?;
      match slot.pass.as_mut() {
        Some(OpenPass::Render(record)) => body(record),
        _ => Err(OpError::dom(
          "InvalidStateError",
          "The render pass has ended",
        )),
      }
    })
  }
}

pub struct GpuComputePass {
  encoder: u64,
  ended: Cell<bool>,
  label: ObjectLabel,
}

unsafe impl v8::cppgc::GarbageCollected for GpuComputePass {
  fn trace(&self, _visitor: &mut v8::cppgc::Visitor) {}

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"GPUComputePassEncoder"
  }
}

#[op(name = "GPUComputePassEncoder")]
impl GpuComputePass {
  #[getter]
  fn label(&self) -> String {
    self.label.get()
  }

  #[setter]
  fn label(&self, value: String) {
    self.label.set(value);
  }

  fn set_pipeline(
    &self,
    scope: &mut v8::PinScope,
    pipeline: v8::Local<v8::Value>,
  ) -> Result<(), OpError> {
    let pipeline = resource::host_compute_pipeline(scope, pipeline)
      .ok_or_else(|| {
        OpError::type_("setPipeline requires a GPUComputePipeline")
      })?;
    self.push(ComputeCommand::Pipeline(pipeline.pipeline().clone()))
  }

  fn set_bind_group(
    &self,
    scope: &mut v8::PinScope,
    index: js::GpuIndex32,
    group: v8::Local<v8::Value>,
    offsets: Option<v8::Local<v8::Value>>,
    start: Option<js::GpuSize64>,
    length: Option<js::GpuSize32>,
  ) -> Result<(), OpError> {
    let (group, offsets) = bind_group(
      scope,
      group,
      offsets,
      start.map(|value| value.0),
      length.map(|value| value.0),
    )?;
    self.push(ComputeCommand::Bind {
      index: index.0,
      group,
      offsets,
    })
  }

  fn dispatch_workgroups(
    &self,
    x: js::GpuSize32,
    y: Option<js::GpuSize32>,
    z: Option<js::GpuSize32>,
  ) -> Result<(), OpError> {
    self.push(ComputeCommand::Dispatch {
      x: x.0,
      y: y.map(|value| value.0).unwrap_or(1),
      z: z.map(|value| value.0).unwrap_or(1),
    })
  }

  fn dispatch_workgroups_indirect(
    &self,
    scope: &mut v8::PinScope,
    buffer: v8::Local<v8::Value>,
    offset: js::GpuSize64,
  ) -> Result<(), OpError> {
    let bind = indirect_bind(
      scope,
      buffer,
      offset.0,
      12,
      "dispatchWorkgroupsIndirect requires a GPUBuffer",
    )?;
    self.with_compute(|record| {
      match bind {
        Some((buffer, offset)) => {
          record
            .commands
            .push(ComputeCommand::DispatchIndirect { buffer, offset });
        }
        None => record.invalid = true,
      }
      Ok(())
    })
  }

  fn push_debug_group(&self, label: String) -> Result<(), OpError> {
    self.push(ComputeCommand::PushDebug(label))
  }

  fn pop_debug_group(&self) -> Result<(), OpError> {
    self.push(ComputeCommand::PopDebug)
  }

  fn insert_debug_marker(&self, label: String) -> Result<(), OpError> {
    self.push(ComputeCommand::Marker(label))
  }

  fn write_timestamp(
    &self,
    scope: &mut v8::PinScope,
    query_set: v8::Local<v8::Value>,
    query_index: js::GpuSize32,
  ) -> Result<(), OpError> {
    require_timestamp_write(
      encoder_features(self.encoder)?,
      wgpu::Features::TIMESTAMP_QUERY_INSIDE_PASSES,
    )?;
    let (set, index) = timestamp_query(scope, query_set, query_index.0)?;
    self.push(ComputeCommand::WriteTimestamp {
      query_set: set,
      index,
    })
  }

  fn end(&self) -> Result<(), OpError> {
    if self.ended.get() {
      return Err(OpError::dom(
        "InvalidStateError",
        "The compute pass has ended",
      ));
    }
    self.ended.set(true);
    ENCODERS.with(|encoders| {
      let mut encoders = encoders.borrow_mut();
      let slot = encoders.get_mut(&self.encoder).ok_or_else(|| {
        OpError::dom("InvalidStateError", "The command encoder is finished")
      })?;
      let record = match slot.pass.take() {
        Some(OpenPass::Compute(record)) => record,
        _ => {
          return Err(OpError::dom(
            "InvalidStateError",
            "The compute pass has ended",
          ));
        }
      };
      if record.invalid {
        slot.invalid = true;
      }
      let encoder = slot.encoder.as_mut().ok_or_else(|| {
        OpError::dom("InvalidStateError", "The command encoder is finished")
      })?;
      let replayed =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
          replay_compute(encoder, &record)
        }));
      replayed.map_err(|_| {
        OpError::dom("OperationError", "The compute pass is invalid")
      })
    })
  }
}

impl GpuComputePass {
  fn push(&self, command: ComputeCommand) -> Result<(), OpError> {
    self.with_compute(|record| {
      record.commands.push(command);
      Ok(())
    })
  }

  fn with_compute(
    &self,
    body: impl FnOnce(&mut ComputeRecord) -> Result<(), OpError>,
  ) -> Result<(), OpError> {
    if self.ended.get() {
      return Err(OpError::dom(
        "InvalidStateError",
        "The compute pass has ended",
      ));
    }
    ENCODERS.with(|encoders| {
      let mut encoders = encoders.borrow_mut();
      let slot = encoders.get_mut(&self.encoder).ok_or_else(|| {
        OpError::dom("InvalidStateError", "The command encoder is finished")
      })?;
      match slot.pass.as_mut() {
        Some(OpenPass::Compute(record)) => body(record),
        _ => Err(OpError::dom(
          "InvalidStateError",
          "The compute pass has ended",
        )),
      }
    })
  }
}

pub struct GpuRenderBundleEncoder {
  id: u64,
  ended: Cell<bool>,
  label: ObjectLabel,
}

unsafe impl v8::cppgc::GarbageCollected for GpuRenderBundleEncoder {
  fn trace(&self, _visitor: &mut v8::cppgc::Visitor) {}

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"GPURenderBundleEncoder"
  }
}

impl Drop for GpuRenderBundleEncoder {
  fn drop(&mut self) {
    BUNDLES.with(|bundles| {
      bundles.borrow_mut().remove(&self.id);
    });
  }
}

pub fn create_bundle_encoder(
  scope: &mut v8::PinScope,
  device_id: u64,
  handle: &Handle,
  descriptor: v8::Local<v8::Value>,
) -> Result<New<GpuRenderBundleEncoder>, OpError> {
  let object = js::object(scope, descriptor).ok_or_else(|| {
    OpError::type_("createRenderBundleEncoder requires a descriptor")
  })?;
  let features = handle.device.features();
  let colors = bundle_color_formats(scope, object, features)?;
  let depth = bundle_depth(scope, object, features)?;
  if colors.iter().all(Option::is_none) && depth.is_none() {
    return Err(OpError::type_(
      "A render bundle needs a color or depth format",
    ));
  }
  let sample_count = resource::allowed_sample_count(
    js::field_u32(scope, object, "sampleCount")?.unwrap_or(1),
  )?;
  let id = NEXT_BUNDLE.fetch_add(1, Ordering::Relaxed);
  BUNDLES.with(|bundles| {
    bundles.borrow_mut().insert(
      id,
      BundleSlot {
        device: handle.device.clone(),
        device_id,
        colors,
        depth,
        sample_count,
        commands: Vec::new(),
        invalid: false,
        max_vertex_buffers: crate::state::validation_limits(device_id)
          .map(|limits| limits.max_vertex_buffers)
          .unwrap_or_else(|_| handle.device.limits().max_vertex_buffers),
      },
    );
  });
  Ok(New(GpuRenderBundleEncoder {
    id,
    ended: Cell::new(false),
    label: ObjectLabel::new(resource::optional_label(scope, object)?),
  }))
}

#[op(name = "GPURenderBundleEncoder")]
impl GpuRenderBundleEncoder {
  #[getter]
  fn label(&self) -> String {
    self.label.get()
  }

  #[setter]
  fn label(&self, value: String) {
    self.label.set(value);
  }

  fn set_pipeline(
    &self,
    scope: &mut v8::PinScope,
    pipeline: v8::Local<v8::Value>,
  ) -> Result<(), OpError> {
    let pipeline =
      resource::host_pipeline(scope, pipeline).ok_or_else(|| {
        OpError::type_("setPipeline requires a GPURenderPipeline")
      })?;
    self.push(BundleCommand::Pipeline(pipeline.pipeline().clone()))
  }

  fn set_vertex_buffer(
    &self,
    scope: &mut v8::PinScope,
    slot: js::GpuIndex32,
    buffer: v8::Local<v8::Value>,
    offset: Option<js::GpuSize64>,
    size: Option<js::GpuSize64>,
  ) -> Result<(), OpError> {
    let max_vertex_buffers = self.limits()?;
    let device_id = self.device_id()?;
    let bind = vertex_bind(
      scope,
      slot.0,
      buffer,
      offset,
      size,
      max_vertex_buffers,
      device_id,
    )?;
    self.with_slot(|slot| {
      match bind {
        VertexBind::Invalid => slot.invalid = true,
        VertexBind::Skip => {}
        VertexBind::Unset { slot: index } => {
          slot.commands.push(BundleCommand::Vertex {
            slot: index,
            buffer: None,
            offset: 0,
            size: None,
          });
        }
        VertexBind::Bind {
          slot: index,
          buffer,
          offset,
          size,
        } => slot.commands.push(BundleCommand::Vertex {
          slot: index,
          buffer: Some(buffer),
          offset,
          size,
        }),
      }
      Ok(())
    })
  }

  fn set_index_buffer(
    &self,
    scope: &mut v8::PinScope,
    buffer: v8::Local<v8::Value>,
    format: String,
    offset: Option<js::GpuSize64>,
    size: Option<js::GpuSize64>,
  ) -> Result<(), OpError> {
    let device_id = self.device_id()?;
    let bind = index_bind(scope, buffer, &format, offset, size, device_id)?;
    self.with_slot(|slot| {
      match bind {
        IndexBind::Invalid => slot.invalid = true,
        IndexBind::Skip => {}
        IndexBind::Bind {
          buffer,
          format,
          offset,
          size,
        } => slot.commands.push(BundleCommand::Index {
          buffer,
          format,
          offset,
          size,
        }),
      }
      Ok(())
    })
  }

  fn set_bind_group(
    &self,
    scope: &mut v8::PinScope,
    index: js::GpuIndex32,
    group: v8::Local<v8::Value>,
    offsets: Option<v8::Local<v8::Value>>,
    start: Option<js::GpuSize64>,
    length: Option<js::GpuSize32>,
  ) -> Result<(), OpError> {
    let (group, offsets) = bind_group(
      scope,
      group,
      offsets,
      start.map(|value| value.0),
      length.map(|value| value.0),
    )?;
    self.push(BundleCommand::Bind {
      index: index.0,
      group,
      offsets,
    })
  }

  fn draw(
    &self,
    vertices: js::GpuSize32,
    instances: Option<js::GpuSize32>,
    first_vertex: Option<js::GpuSize32>,
    first_instance: Option<js::GpuSize32>,
  ) -> Result<(), OpError> {
    self.push(BundleCommand::Draw {
      vertices: vertices.0,
      instances: instances.map(|value| value.0).unwrap_or(1),
      first_vertex: first_vertex.map(|value| value.0).unwrap_or(0),
      first_instance: first_instance.map(|value| value.0).unwrap_or(0),
    })
  }

  fn draw_indexed(
    &self,
    indices: js::GpuSize32,
    instances: Option<js::GpuSize32>,
    first_index: Option<js::GpuSize32>,
    base_vertex: Option<js::GpuSignedOffset32>,
    first_instance: Option<js::GpuSize32>,
  ) -> Result<(), OpError> {
    self.push(BundleCommand::DrawIndexed {
      indices: indices.0,
      instances: instances.map(|value| value.0).unwrap_or(1),
      first_index: first_index.map(|value| value.0).unwrap_or(0),
      base_vertex: base_vertex.map(|value| value.0).unwrap_or(0),
      first_instance: first_instance.map(|value| value.0).unwrap_or(0),
    })
  }

  fn draw_indirect(
    &self,
    scope: &mut v8::PinScope,
    buffer: v8::Local<v8::Value>,
    offset: js::GpuSize64,
  ) -> Result<(), OpError> {
    let bind = indirect_bind(
      scope,
      buffer,
      offset.0,
      16,
      "drawIndirect requires a GPUBuffer",
    )?;
    self.with_slot(|slot| {
      match bind {
        Some((buffer, offset)) => {
          slot
            .commands
            .push(BundleCommand::DrawIndirect { buffer, offset });
        }
        None => slot.invalid = true,
      }
      Ok(())
    })
  }

  fn draw_indexed_indirect(
    &self,
    scope: &mut v8::PinScope,
    buffer: v8::Local<v8::Value>,
    offset: js::GpuSize64,
  ) -> Result<(), OpError> {
    let bind = indirect_bind(
      scope,
      buffer,
      offset.0,
      20,
      "drawIndexedIndirect requires a GPUBuffer",
    )?;
    self.with_slot(|slot| {
      match bind {
        Some((buffer, offset)) => {
          slot
            .commands
            .push(BundleCommand::DrawIndexedIndirect { buffer, offset });
        }
        None => slot.invalid = true,
      }
      Ok(())
    })
  }

  fn push_debug_group(&self, label: String) -> Result<(), OpError> {
    let _ = label;
    self.ensure_recording()?;
    Err(js::unsupported("GPURenderBundleEncoder.pushDebugGroup"))
  }

  fn pop_debug_group(&self) -> Result<(), OpError> {
    self.ensure_recording()?;
    Err(js::unsupported("GPURenderBundleEncoder.popDebugGroup"))
  }

  fn insert_debug_marker(&self, label: String) -> Result<(), OpError> {
    let _ = label;
    self.ensure_recording()?;
    Err(js::unsupported("GPURenderBundleEncoder.insertDebugMarker"))
  }

  fn set_viewport(
    &self,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    min_depth: f64,
    max_depth: f64,
  ) -> Result<(), OpError> {
    viewport_finite(x, y, width, height, min_depth, max_depth)?;
    self.ensure_recording()?;
    Err(js::unsupported("GPURenderBundleEncoder.setViewport"))
  }

  fn set_scissor_rect(
    &self,
    x: js::GpuIntegerCoordinate,
    y: js::GpuIntegerCoordinate,
    width: js::GpuIntegerCoordinate,
    height: js::GpuIntegerCoordinate,
  ) -> Result<(), OpError> {
    let _ = (x, y, width, height);
    self.ensure_recording()?;
    Err(js::unsupported("GPURenderBundleEncoder.setScissorRect"))
  }

  fn finish(
    &self,
    scope: &mut v8::PinScope,
    descriptor: Option<v8::Local<v8::Value>>,
  ) -> Result<New<GpuRenderBundle>, OpError> {
    if self.ended.get() {
      return Err(OpError::dom(
        "InvalidStateError",
        "The render bundle encoder has finished",
      ));
    }
    self.ended.set(true);
    let slot = BUNDLES.with(|bundles| {
      bundles.borrow_mut().remove(&self.id).ok_or_else(|| {
        OpError::dom(
          "InvalidStateError",
          "The render bundle encoder has finished",
        )
      })
    })?;
    let draws = bundle_draw_count(&slot);
    if slot.invalid {
      crate::state::report_validation(
        slot.device_id,
        "The render bundle encoder is invalid",
      );
    }
    let bundle = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
      finish_bundle(slot)
    }));
    let bundle = bundle.map_err(|_| {
      OpError::dom("OperationError", "The render bundle is invalid")
    })?;
    Ok(New(GpuRenderBundle {
      bundle,
      draws,
      label: ObjectLabel::new(descriptor_label(scope, descriptor)?),
    }))
  }
}

impl GpuRenderBundleEncoder {
  fn ensure_recording(&self) -> Result<(), OpError> {
    if self.ended.get() {
      return Err(OpError::dom(
        "InvalidStateError",
        "The render bundle encoder has finished",
      ));
    }
    BUNDLES.with(|bundles| {
      if bundles.borrow().contains_key(&self.id) {
        Ok(())
      } else {
        Err(OpError::dom(
          "InvalidStateError",
          "The render bundle encoder has finished",
        ))
      }
    })
  }

  fn limits(&self) -> Result<u32, OpError> {
    BUNDLES.with(|bundles| {
      bundles
        .borrow()
        .get(&self.id)
        .map(|slot| slot.max_vertex_buffers)
        .ok_or_else(|| {
          OpError::dom(
            "InvalidStateError",
            "The render bundle encoder has finished",
          )
        })
    })
  }

  fn device_id(&self) -> Result<u64, OpError> {
    BUNDLES.with(|bundles| {
      bundles
        .borrow()
        .get(&self.id)
        .map(|slot| slot.device_id)
        .ok_or_else(|| {
          OpError::dom(
            "InvalidStateError",
            "The render bundle encoder has finished",
          )
        })
    })
  }

  fn push(&self, command: BundleCommand) -> Result<(), OpError> {
    self.with_slot(|slot| {
      slot.commands.push(command);
      Ok(())
    })
  }

  fn with_slot(
    &self,
    body: impl FnOnce(&mut BundleSlot) -> Result<(), OpError>,
  ) -> Result<(), OpError> {
    if self.ended.get() {
      return Err(OpError::dom(
        "InvalidStateError",
        "The render bundle encoder has finished",
      ));
    }
    BUNDLES.with(|bundles| {
      let mut bundles = bundles.borrow_mut();
      let slot = bundles.get_mut(&self.id).ok_or_else(|| {
        OpError::dom(
          "InvalidStateError",
          "The render bundle encoder has finished",
        )
      })?;
      body(slot)
    })
  }
}

pub struct GpuRenderBundle {
  bundle: wgpu::RenderBundle,
  draws: u64,
  label: ObjectLabel,
}

unsafe impl v8::cppgc::GarbageCollected for GpuRenderBundle {
  fn trace(&self, _visitor: &mut v8::cppgc::Visitor) {}

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"GPURenderBundle"
  }
}

#[op(name = "GPURenderBundle")]
impl GpuRenderBundle {
  #[getter]
  fn label(&self) -> String {
    self.label.get()
  }

  #[setter]
  fn label(&self, value: String) {
    self.label.set(value);
  }
}

fn descriptor_label(
  scope: &mut v8::PinScope,
  descriptor: Option<v8::Local<v8::Value>>,
) -> Result<Option<String>, OpError> {
  let Some(value) = descriptor else {
    return Ok(None);
  };
  if value.is_null_or_undefined() {
    return Ok(None);
  }
  let Some(object) = js::object(scope, value) else {
    return Ok(None);
  };
  resource::optional_label(scope, object)
}

fn host_bundle<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  value: v8::Local<v8::Value>,
) -> Option<&'s GpuRenderBundle> {
  let object = v8::Local::<v8::Object>::try_from(value).ok()?;
  let host =
    unsafe { yuzuko_core::op::host_ref::<GpuRenderBundle>(scope, object) }?;
  let ptr = host.get() as *const GpuRenderBundle;
  let _host = host;
  Some(unsafe { &*ptr })
}

fn replay_render(encoder: &mut wgpu::CommandEncoder, record: &PassRecord) {
  let colors = record
    .colors
    .iter()
    .map(|color| {
      color.as_ref().map(|color| wgpu::RenderPassColorAttachment {
        view: &color.view,
        depth_slice: None,
        resolve_target: color.resolve.as_ref(),
        ops: wgpu::Operations {
          load: match color.clear {
            Some(color) => wgpu::LoadOp::Clear(color),
            None => wgpu::LoadOp::Load,
          },
          store: if color.store {
            wgpu::StoreOp::Store
          } else {
            wgpu::StoreOp::Discard
          },
        },
      })
    })
    .collect::<Vec<_>>();
  let depth =
    record
      .depth
      .as_ref()
      .map(|depth| wgpu::RenderPassDepthStencilAttachment {
        view: &depth.view,
        depth_ops: depth.depth_ops,
        stencil_ops: depth.stencil_ops,
      });
  let timestamps =
    record
      .timestamps
      .as_ref()
      .map(|write| wgpu::RenderPassTimestampWrites {
        query_set: &write.query_set,
        beginning_of_pass_write_index: write.beginning,
        end_of_pass_write_index: write.end,
      });
  let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
    label: Some("yuzuko-webgpu"),
    color_attachments: &colors,
    depth_stencil_attachment: depth,
    timestamp_writes: timestamps,
    occlusion_query_set: record.occlusion.as_ref(),
    multiview_mask: None,
  });
  for command in &record.commands {
    match command {
      PassCommand::Pipeline(pipeline) => pass.set_pipeline(pipeline),
      PassCommand::Vertex {
        slot,
        buffer,
        offset,
        size,
      } => match buffer {
        Some(buffer) => {
          pass.set_vertex_buffer(*slot, buffer_slice(buffer, *offset, *size))
        }
        None => pass.set_vertex_buffer(*slot, None),
      },
      PassCommand::Index {
        buffer,
        format,
        offset,
        size,
      } => pass.set_index_buffer(buffer_slice(buffer, *offset, *size), *format),
      PassCommand::Bind {
        index,
        group,
        offsets,
      } => pass.set_bind_group(*index, group.as_ref(), offsets),
      PassCommand::Draw {
        vertices,
        instances,
        first_vertex,
        first_instance,
      } => pass.draw(
        *first_vertex..*first_vertex + *vertices,
        *first_instance..*first_instance + *instances,
      ),
      PassCommand::DrawIndexed {
        indices,
        instances,
        first_index,
        base_vertex,
        first_instance,
      } => pass.draw_indexed(
        *first_index..*first_index + *indices,
        *base_vertex,
        *first_instance..*first_instance + *instances,
      ),
      PassCommand::DrawIndirect { buffer, offset } => {
        pass.draw_indirect(buffer, *offset);
      }
      PassCommand::DrawIndexedIndirect { buffer, offset } => {
        pass.draw_indexed_indirect(buffer, *offset);
      }
      PassCommand::BeginOcclusion(index) => pass.begin_occlusion_query(*index),
      PassCommand::EndOcclusion => pass.end_occlusion_query(),
      PassCommand::Bundles(bundles) => pass.execute_bundles(bundles.iter()),
      PassCommand::Viewport {
        x,
        y,
        width,
        height,
        min_depth,
        max_depth,
      } => pass.set_viewport(*x, *y, *width, *height, *min_depth, *max_depth),
      PassCommand::Scissor {
        x,
        y,
        width,
        height,
      } => pass.set_scissor_rect(*x, *y, *width, *height),
      PassCommand::PushDebug(label) => pass.push_debug_group(label),
      PassCommand::PopDebug => pass.pop_debug_group(),
      PassCommand::Marker(label) => pass.insert_debug_marker(label),
      PassCommand::WriteTimestamp { query_set, index } => {
        pass.write_timestamp(query_set, *index);
      }
      PassCommand::BlendConstant(color) => pass.set_blend_constant(*color),
      PassCommand::StencilReference(reference) => {
        pass.set_stencil_reference(*reference);
      }
    }
  }
  if record.draws_exceeded {
    // wgpu does not count draws. An out-of-range scissor is the validation
    // error an error scope observes. The message names the scissor.
    pass.set_scissor_rect(0, 0, u32::MAX, u32::MAX);
  }
}

fn replay_compute(encoder: &mut wgpu::CommandEncoder, record: &ComputeRecord) {
  let timestamps =
    record
      .timestamps
      .as_ref()
      .map(|write| wgpu::ComputePassTimestampWrites {
        query_set: &write.query_set,
        beginning_of_pass_write_index: write.beginning,
        end_of_pass_write_index: write.end,
      });
  let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
    label: Some("yuzuko-webgpu"),
    timestamp_writes: timestamps,
  });
  for command in &record.commands {
    match command {
      ComputeCommand::Pipeline(pipeline) => pass.set_pipeline(pipeline),
      ComputeCommand::Bind {
        index,
        group,
        offsets,
      } => pass.set_bind_group(*index, group.as_ref(), offsets),
      ComputeCommand::Dispatch { x, y, z } => {
        pass.dispatch_workgroups(*x, *y, *z)
      }
      ComputeCommand::DispatchIndirect { buffer, offset } => {
        pass.dispatch_workgroups_indirect(buffer, *offset);
      }
      ComputeCommand::PushDebug(label) => pass.push_debug_group(label),
      ComputeCommand::PopDebug => pass.pop_debug_group(),
      ComputeCommand::Marker(label) => pass.insert_debug_marker(label),
      ComputeCommand::WriteTimestamp { query_set, index } => {
        pass.write_timestamp(query_set, *index);
      }
    }
  }
}

fn finish_bundle(slot: BundleSlot) -> wgpu::RenderBundle {
  let mut encoder = slot.device.create_render_bundle_encoder(
    &wgpu::RenderBundleEncoderDescriptor {
      label: None,
      color_formats: &slot.colors,
      depth_stencil: slot.depth,
      sample_count: slot.sample_count,
      multiview: None,
    },
  );
  for command in &slot.commands {
    match command {
      BundleCommand::Pipeline(pipeline) => encoder.set_pipeline(pipeline),
      BundleCommand::Vertex {
        slot,
        buffer,
        offset,
        size,
      } => match buffer {
        Some(buffer) => {
          encoder.set_vertex_buffer(*slot, buffer_slice(buffer, *offset, *size))
        }
        None => encoder.set_vertex_buffer(*slot, None),
      },
      BundleCommand::Index {
        buffer,
        format,
        offset,
        size,
      } => {
        encoder.set_index_buffer(buffer_slice(buffer, *offset, *size), *format)
      }
      BundleCommand::Bind {
        index,
        group,
        offsets,
      } => encoder.set_bind_group(*index, group.as_ref(), offsets),
      BundleCommand::Draw {
        vertices,
        instances,
        first_vertex,
        first_instance,
      } => encoder.draw(
        *first_vertex..*first_vertex + *vertices,
        *first_instance..*first_instance + *instances,
      ),
      BundleCommand::DrawIndexed {
        indices,
        instances,
        first_index,
        base_vertex,
        first_instance,
      } => encoder.draw_indexed(
        *first_index..*first_index + *indices,
        *base_vertex,
        *first_instance..*first_instance + *instances,
      ),
      BundleCommand::DrawIndirect { buffer, offset } => {
        encoder.draw_indirect(buffer, *offset);
      }
      BundleCommand::DrawIndexedIndirect { buffer, offset } => {
        encoder.draw_indexed_indirect(buffer, *offset);
      }
    }
  }
  encoder.finish(&wgpu::RenderBundleDescriptor { label: None })
}

fn buffer_slice(
  buffer: &wgpu::Buffer,
  offset: u64,
  size: Option<u64>,
) -> wgpu::BufferSlice<'_> {
  match size {
    Some(size) => buffer.slice(offset..offset + size),
    None => buffer.slice(offset..),
  }
}

fn encoder_features(id: u64) -> Result<wgpu::Features, OpError> {
  ENCODERS.with(|encoders| {
    encoders
      .borrow()
      .get(&id)
      .map(|slot| slot.features)
      .ok_or_else(|| {
        OpError::dom("InvalidStateError", "The command encoder is finished")
      })
  })
}

fn require_timestamp_write(
  features: wgpu::Features,
  inside: wgpu::Features,
) -> Result<(), OpError> {
  if features.contains(wgpu::Features::TIMESTAMP_QUERY | inside) {
    Ok(())
  } else {
    Err(OpError::dom(
      "NotSupportedError",
      "timestamp-query is not enabled",
    ))
  }
}

/// A destroyed set has no wgpu destroy. Recording one past the last query
/// makes the later submit a validation error. `u32::MAX` is still in range.
fn timestamp_query(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
  query_index: u32,
) -> Result<(wgpu::QuerySet, u32), OpError> {
  let query = resource::host_query_set(scope, value)
    .ok_or_else(|| OpError::type_("writeTimestamp requires a GPUQuerySet"))?;
  if query.kind() != QueryKind::Timestamp {
    return Err(OpError::type_(
      "writeTimestamp requires a timestamp query set",
    ));
  }
  let index = if query.is_destroyed() {
    query.query_count()
  } else {
    query_index
  };
  Ok((query.query_set(), index))
}

fn with_idle_encoder(
  id: u64,
  body: impl FnOnce(&mut wgpu::CommandEncoder) -> Result<(), OpError>,
) -> Result<(), OpError> {
  ENCODERS.with(|encoders| {
    let mut encoders = encoders.borrow_mut();
    let slot = encoders.get_mut(&id).ok_or_else(|| {
      OpError::dom("InvalidStateError", "The command encoder is finished")
    })?;
    if slot.pass.is_some() {
      return Err(OpError::dom("InvalidStateError", "A pass is open"));
    }
    let encoder = slot.encoder.as_mut().ok_or_else(|| {
      OpError::dom("InvalidStateError", "The command encoder is finished")
    })?;
    body(encoder)
  })
}

fn note_draws(record: &mut PassRecord, count: u64) {
  if count == 0 {
    return;
  }
  let next = record.draws.saturating_add(count);
  if next > record.max_draws {
    record.draws_exceeded = true;
  }
  record.draws = next;
}

fn bundle_draw_count(slot: &BundleSlot) -> u64 {
  slot
    .commands
    .iter()
    .filter(|command| {
      matches!(
        command,
        BundleCommand::Draw { .. }
          | BundleCommand::DrawIndexed { .. }
          | BundleCommand::DrawIndirect { .. }
          | BundleCommand::DrawIndexedIndirect { .. }
      )
    })
    .count() as u64
}

const DEFAULT_MAX_DRAWS: u64 = 50_000_000;

fn max_draw_count(
  scope: &mut v8::PinScope,
  descriptor: v8::Local<v8::Object>,
) -> Result<u64, OpError> {
  let Some(value) = js::get(scope, descriptor, "maxDrawCount")? else {
    return Ok(DEFAULT_MAX_DRAWS);
  };
  js::u64_value(scope, value)
}

fn viewport_finite(
  x: f64,
  y: f64,
  width: f64,
  height: f64,
  min_depth: f64,
  max_depth: f64,
) -> Result<[f64; 6], OpError> {
  let values = [x, y, width, height, min_depth, max_depth];
  if values.iter().any(|value| !value.is_finite()) {
    return Err(OpError::type_("setViewport arguments must be finite"));
  }
  Ok(values)
}

fn viewport_cast(
  values: [f64; 6],
  max_texture_dimension_2d: u32,
) -> Option<(f32, f32, f32, f32, f32, f32)> {
  let max_dimension = f64::from(max_texture_dimension_2d);
  let max_range = max_dimension * 2.0;
  let [x, y, width, height, min_depth, max_depth] = values;
  let inside = x >= -max_range
    && y >= -max_range
    && (0.0..=max_dimension).contains(&width)
    && (0.0..=max_dimension).contains(&height)
    && x + width <= max_range - 1.0
    && y + height <= max_range - 1.0
    && (0.0..=1.0).contains(&min_depth)
    && (0.0..=1.0).contains(&max_depth)
    && min_depth <= max_depth;
  if !inside {
    return None;
  }
  let cast = [
    x as f32,
    y as f32,
    width as f32,
    height as f32,
    min_depth as f32,
    max_depth as f32,
  ];
  if cast.iter().any(|value| !value.is_finite()) {
    return None;
  }
  Some((cast[0], cast[1], cast[2], cast[3], cast[4], cast[5]))
}

fn color_targets(
  scope: &mut v8::PinScope,
  descriptor: v8::Local<v8::Object>,
) -> Result<Vec<Option<ColorTarget>>, OpError> {
  let value = js::get(scope, descriptor, "colorAttachments")?
    .ok_or_else(|| OpError::type_("colorAttachments is required"))?;
  let array = v8::Local::<v8::Array>::try_from(value)
    .map_err(|_| OpError::type_("colorAttachments must be an array"))?;
  let mut colors = Vec::new();
  for index in 0..array.length() {
    let value = js::read_index(scope, array, index)?;
    if value.is_null() {
      colors.push(None);
      continue;
    }
    let attachment = js::object(scope, value)
      .ok_or_else(|| OpError::type_("A color attachment must be an object"))?;
    if js::get(scope, attachment, "depthSlice")?
      .is_some_and(|value| !value.is_null())
    {
      return Err(js::unsupported("GPURenderPassColorAttachment.depthSlice"));
    }
    let resolve = match js::get(scope, attachment, "resolveTarget")? {
      Some(value) if !value.is_null_or_undefined() => {
        Some(resource::attachment_view(scope, value)?.0)
      }
      _ => None,
    };
    let view = js::get(scope, attachment, "view")?
      .ok_or_else(|| OpError::type_("A color attachment view is required"))?;
    let (view, _) = resource::attachment_view(scope, view)?;
    let load = js::get(scope, attachment, "loadOp")?
      .map(|value| js::string(scope, value))
      .transpose()?
      .unwrap_or_else(|| "load".to_string());
    let clear = if load == "clear" {
      let clear_value = js::get(scope, attachment, "clearValue")?;
      Some(clear_color(scope, clear_value)?)
    } else {
      None
    };
    let store = js::get(scope, attachment, "storeOp")?
      .map(|value| js::string(scope, value))
      .transpose()?
      .as_deref()
      != Some("discard");
    colors.push(Some(ColorTarget {
      view,
      resolve,
      clear,
      store,
    }));
  }
  Ok(colors)
}

fn depth_target(
  scope: &mut v8::PinScope,
  descriptor: v8::Local<v8::Object>,
) -> Result<Option<DepthTarget>, OpError> {
  let Some(value) = js::get(scope, descriptor, "depthStencilAttachment")?
  else {
    return Ok(None);
  };
  if value.is_null_or_undefined() {
    return Ok(None);
  }
  let attachment = js::object(scope, value)
    .ok_or_else(|| OpError::type_("A depth attachment must be an object"))?;
  let view = js::get(scope, attachment, "view")?
    .filter(|value| !value.is_null_or_undefined())
    .ok_or_else(|| OpError::type_("A depth attachment view is required"))?;
  let (view, format) = resource::attachment_view(scope, view)?;
  let depth_ops = aspect_ops(
    scope,
    attachment,
    format.has_depth_aspect(),
    "depth",
    |scope, attachment| {
      let value = js::get(scope, attachment, "depthClearValue")?;
      Ok(
        value
          .and_then(|item| item.number_value(scope))
          .unwrap_or(0.0) as f32,
      )
    },
  )?;
  let stencil_ops = aspect_ops(
    scope,
    attachment,
    format.has_stencil_aspect(),
    "stencil",
    |scope, attachment| {
      Ok(js::field_u32(scope, attachment, "stencilClearValue")?.unwrap_or(0))
    },
  )?;
  Ok(Some(DepthTarget {
    view,
    depth_ops,
    stencil_ops,
  }))
}

fn aspect_ops<T: Copy>(
  scope: &mut v8::PinScope,
  attachment: v8::Local<v8::Object>,
  has_aspect: bool,
  prefix: &str,
  clear_value: impl FnOnce(
    &mut v8::PinScope,
    v8::Local<v8::Object>,
  ) -> Result<T, OpError>,
) -> Result<Option<wgpu::Operations<T>>, OpError> {
  let read_only_name = format!("{prefix}ReadOnly");
  let load_name = format!("{prefix}LoadOp");
  let store_name = format!("{prefix}StoreOp");
  let read_only = js::get(scope, attachment, &read_only_name)?
    .is_some_and(|value| value.boolean_value(scope));
  let load = js::get(scope, attachment, &load_name)?
    .map(|value| js::string(scope, value))
    .transpose()?;
  let store = js::get(scope, attachment, &store_name)?
    .map(|value| js::string(scope, value))
    .transpose()?;
  if !has_aspect {
    if read_only || load.is_some() || store.is_some() {
      return Err(OpError::type_(format!(
        "The texture format has no {prefix} aspect"
      )));
    }
    return Ok(None);
  }
  if read_only {
    if load.is_some() || store.is_some() {
      return Err(OpError::type_(format!(
        "{load_name} is not allowed when {read_only_name} is true"
      )));
    }
    return Ok(None);
  }
  let load =
    load.ok_or_else(|| OpError::type_(format!("{load_name} is required")))?;
  let store =
    store.ok_or_else(|| OpError::type_(format!("{store_name} is required")))?;
  let load = match load.as_str() {
    "load" => wgpu::LoadOp::Load,
    "clear" => wgpu::LoadOp::Clear(clear_value(scope, attachment)?),
    _ => return Err(OpError::type_(format!("{load_name} is invalid"))),
  };
  let store = match store.as_str() {
    "store" => wgpu::StoreOp::Store,
    "discard" => wgpu::StoreOp::Discard,
    _ => return Err(OpError::type_(format!("{store_name} is invalid"))),
  };
  Ok(Some(wgpu::Operations { load, store }))
}

fn occlusion_query_set(
  scope: &mut v8::PinScope,
  descriptor: v8::Local<v8::Object>,
) -> Result<Option<wgpu::QuerySet>, OpError> {
  let Some(value) = js::get(scope, descriptor, "occlusionQuerySet")? else {
    return Ok(None);
  };
  if value.is_null_or_undefined() {
    return Ok(None);
  }
  let query = resource::host_query_set(scope, value)
    .ok_or_else(|| OpError::type_("occlusionQuerySet must be a GPUQuerySet"))?;
  if query.kind() != QueryKind::Occlusion {
    return Err(OpError::type_(
      "occlusionQuerySet must be an occlusion query set",
    ));
  }
  Ok(Some(query.query_set()))
}

fn timestamp_writes(
  scope: &mut v8::PinScope,
  descriptor: v8::Local<v8::Object>,
) -> Result<Option<TimestampWrite>, OpError> {
  let Some(value) = js::get(scope, descriptor, "timestampWrites")? else {
    return Ok(None);
  };
  if value.is_null_or_undefined() {
    return Ok(None);
  }
  let writes = js::object(scope, value)
    .ok_or_else(|| OpError::type_("timestampWrites must be an object"))?;
  let query_value = js::get(scope, writes, "querySet")?
    .ok_or_else(|| OpError::type_("timestampWrites.querySet is required"))?;
  let query =
    resource::host_query_set(scope, query_value).ok_or_else(|| {
      OpError::type_("timestampWrites.querySet must be a GPUQuerySet")
    })?;
  if query.kind() != QueryKind::Timestamp {
    return Err(OpError::type_(
      "timestampWrites.querySet must be a timestamp query set",
    ));
  }
  let beginning = js::field_u32(scope, writes, "beginningOfPassWriteIndex")?;
  let end = js::field_u32(scope, writes, "endOfPassWriteIndex")?;
  if beginning.is_none() && end.is_none() {
    return Err(OpError::type_("A timestamp write needs an index"));
  }
  if beginning.is_some() && beginning == end {
    return Err(OpError::type_("Timestamp write indices must differ"));
  }
  Ok(Some(TimestampWrite {
    query_set: query.query_set(),
    beginning,
    end,
  }))
}

fn bundle_color_formats(
  scope: &mut v8::PinScope,
  descriptor: v8::Local<v8::Object>,
  features: wgpu::Features,
) -> Result<Vec<Option<wgpu::TextureFormat>>, OpError> {
  let value = js::get(scope, descriptor, "colorFormats")?
    .ok_or_else(|| OpError::type_("colorFormats is required"))?;
  let array = v8::Local::<v8::Array>::try_from(value)
    .map_err(|_| OpError::type_("colorFormats must be an array"))?;
  let mut formats = Vec::new();
  for index in 0..array.length() {
    let value = js::read_index(scope, array, index)?;
    if value.is_null() {
      formats.push(None);
      continue;
    }
    let name = js::string(scope, value)?;
    let format = resource::texture_format(&name)?;
    formats.push(Some(resource::require_texture_format(features, format)?));
  }
  Ok(formats)
}

fn bundle_depth(
  scope: &mut v8::PinScope,
  descriptor: v8::Local<v8::Object>,
  features: wgpu::Features,
) -> Result<Option<wgpu::RenderBundleDepthStencil>, OpError> {
  let Some(value) = js::get(scope, descriptor, "depthStencilFormat")? else {
    return Ok(None);
  };
  if value.is_null_or_undefined() {
    return Ok(None);
  }
  let name = js::string(scope, value)?;
  let format = resource::require_texture_format(
    features,
    resource::texture_format(&name)?,
  )?;
  let depth_read_only = js::get(scope, descriptor, "depthReadOnly")?
    .is_some_and(|value| value.boolean_value(scope));
  let stencil_read_only = js::get(scope, descriptor, "stencilReadOnly")?
    .is_some_and(|value| value.boolean_value(scope));
  Ok(Some(wgpu::RenderBundleDepthStencil {
    format,
    depth_read_only: depth_read_only || !format.has_depth_aspect(),
    stencil_read_only: stencil_read_only || !format.has_stencil_aspect(),
  }))
}

fn blend_color(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<wgpu::Color, OpError> {
  // An array is an object in V8. The sequence form is checked first.
  let Some(object) = js::object(scope, value) else {
    return Err(OpError::type_("The blend constant is invalid"));
  };
  if let Ok(array) = v8::Local::<v8::Array>::try_from(value) {
    return sequence_color(scope, object, array.length());
  }
  if value.is_typed_array()
    && let Ok(typed) = v8::Local::<v8::TypedArray>::try_from(value)
  {
    let length = u32::try_from(typed.length()).unwrap_or(u32::MAX);
    return sequence_color(scope, object, length);
  }
  Ok(wgpu::Color {
    r: required_channel(scope, object, "r")?,
    g: required_channel(scope, object, "g")?,
    b: required_channel(scope, object, "b")?,
    a: optional_channel(scope, object, "a", 1.0)?,
  })
}

fn sequence_color(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  length: u32,
) -> Result<wgpu::Color, OpError> {
  if length != 3 && length != 4 {
    return Err(OpError::type_("The blend constant is invalid"));
  }
  Ok(wgpu::Color {
    r: indexed_channel(scope, object, 0)?,
    g: indexed_channel(scope, object, 1)?,
    b: indexed_channel(scope, object, 2)?,
    a: if length == 4 {
      indexed_channel(scope, object, 3)?
    } else {
      1.0
    },
  })
}

fn indexed_channel(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  index: u32,
) -> Result<f64, OpError> {
  let value = object
    .get_index(scope, index)
    .ok_or_else(|| OpError::type_("The blend constant is invalid"))?;
  js::finite_number(scope, value)
}

fn required_channel(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
) -> Result<f64, OpError> {
  let value = js::get(scope, object, name)?
    .ok_or_else(|| OpError::type_("The blend constant is invalid"))?;
  js::finite_number(scope, value)
}

fn optional_channel(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
  default: f64,
) -> Result<f64, OpError> {
  match js::get(scope, object, name)? {
    Some(value) => js::finite_number(scope, value),
    None => Ok(default),
  }
}

fn clear_color(
  scope: &mut v8::PinScope,
  value: Option<v8::Local<v8::Value>>,
) -> Result<wgpu::Color, OpError> {
  let Some(value) = value else {
    return Ok(wgpu::Color::TRANSPARENT);
  };
  if let Some(object) = js::object(scope, value) {
    return Ok(wgpu::Color {
      r: color_channel(scope, object, "r", 0.0)?,
      g: color_channel(scope, object, "g", 0.0)?,
      b: color_channel(scope, object, "b", 0.0)?,
      a: color_channel(scope, object, "a", 1.0)?,
    });
  }
  Ok(wgpu::Color::TRANSPARENT)
}

fn color_channel(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
  default: f64,
) -> Result<f64, OpError> {
  let Some(value) = js::get(scope, object, name)? else {
    return Ok(default);
  };
  js::finite_number(scope, value)
}

struct CopyTexture {
  texture: wgpu::Texture,
  mip_level: u32,
  origin: wgpu::Origin3d,
  aspect: wgpu::TextureAspect,
}

fn copy_texture(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
  role: &str,
) -> Result<CopyTexture, OpError> {
  let copy = js::object(scope, value).ok_or_else(|| {
    OpError::type_(format!("copyTextureToTexture {role} must be an object"))
  })?;
  let texture = js::get(scope, copy, "texture")?
    .and_then(|value| resource::host_texture(scope, value))
    .ok_or_else(|| {
      OpError::type_(format!("The copy {role} is not a texture"))
    })?;
  let texture = texture.texture()?;
  let mip_level = js::field_u32(scope, copy, "mipLevel")?.unwrap_or(0);
  let origin = copy_origin(scope, copy)?;
  let aspect = match js::get(scope, copy, "aspect")?
    .map(|value| js::string(scope, value))
    .transpose()?
    .as_deref()
  {
    None => wgpu::TextureAspect::All,
    Some(name) => resource::texture_aspect(name)?,
  };
  Ok(CopyTexture {
    texture,
    mip_level,
    origin,
    aspect,
  })
}

pub(crate) fn copy_origin(
  scope: &mut v8::PinScope,
  copy: v8::Local<v8::Object>,
) -> Result<wgpu::Origin3d, OpError> {
  let Some(origin) = js::get(scope, copy, "origin")? else {
    return Ok(wgpu::Origin3d::ZERO);
  };
  if origin.is_null_or_undefined() {
    return Ok(wgpu::Origin3d::ZERO);
  }
  if let Ok(array) = v8::Local::<v8::Array>::try_from(origin) {
    let x = js::index_u32(scope, array, 0)?;
    let y = if array.length() > 1 {
      js::index_u32(scope, array, 1)?
    } else {
      0
    };
    let z = if array.length() > 2 {
      js::index_u32(scope, array, 2)?
    } else {
      0
    };
    return Ok(wgpu::Origin3d { x, y, z });
  }
  let object = js::object(scope, origin)
    .ok_or_else(|| OpError::type_("The copy origin is invalid"))?;
  Ok(wgpu::Origin3d {
    x: js::field_u32(scope, object, "x")?.unwrap_or(0),
    y: js::field_u32(scope, object, "y")?.unwrap_or(0),
    z: js::field_u32(scope, object, "z")?.unwrap_or(0),
  })
}

pub(crate) fn copy_extent(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<wgpu::Extent3d, OpError> {
  if let Ok(array) = v8::Local::<v8::Array>::try_from(value) {
    let width = js::index_u32(scope, array, 0)?;
    let height = if array.length() > 1 {
      js::index_u32(scope, array, 1)?
    } else {
      1
    };
    let depth = if array.length() > 2 {
      js::index_u32(scope, array, 2)?
    } else {
      1
    };
    return Ok(wgpu::Extent3d {
      width,
      height,
      depth_or_array_layers: depth,
    });
  }
  let object = js::object(scope, value)
    .ok_or_else(|| OpError::type_("The copy size is invalid"))?;
  Ok(wgpu::Extent3d {
    width: js::required_u32(scope, object, "width")?,
    height: js::field_u32(scope, object, "height")?.unwrap_or(1),
    depth_or_array_layers: js::field_u32(scope, object, "depthOrArrayLayers")?
      .unwrap_or(1),
  })
}

fn copy_size(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<(u32, u32), OpError> {
  if let Ok(array) = v8::Local::<v8::Array>::try_from(value) {
    let width = js::index_u32(scope, array, 0)?;
    let height = if array.length() > 1 {
      js::index_u32(scope, array, 1)?
    } else {
      1
    };
    return Ok((width, height));
  }
  let object = js::object(scope, value)
    .ok_or_else(|| OpError::type_("The copy size is invalid"))?;
  let width = js::required_u32(scope, object, "width")?;
  let height = js::field_u32(scope, object, "height")?.unwrap_or(1);
  Ok((width, height))
}

fn host_gpu_buffer<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  value: v8::Local<v8::Value>,
  message: &str,
) -> Result<&'s resource::GpuBuffer, OpError> {
  resource::host_buffer(scope, value).ok_or_else(|| OpError::type_(message))
}

fn encoder_device(id: u64) -> Result<u64, OpError> {
  ENCODERS.with(|encoders| {
    encoders
      .borrow()
      .get(&id)
      .map(|slot| slot.device_id)
      .ok_or_else(|| {
        OpError::dom("InvalidStateError", "The command encoder is finished")
      })
  })
}

fn encoder_limits(id: u64) -> Result<(u32, u32), OpError> {
  ENCODERS.with(|encoders| {
    encoders
      .borrow()
      .get(&id)
      .map(|slot| (slot.max_texture_dimension_2d, slot.max_vertex_buffers))
      .ok_or_else(|| {
        OpError::dom("InvalidStateError", "The command encoder is finished")
      })
  })
}

fn invalidate_encoder(id: u64) -> Result<(), OpError> {
  ENCODERS.with(|encoders| {
    let mut encoders = encoders.borrow_mut();
    let slot = encoders.get_mut(&id).ok_or_else(|| {
      OpError::dom("InvalidStateError", "The command encoder is finished")
    })?;
    if slot.pass.is_some() {
      return Err(OpError::dom("InvalidStateError", "A pass is open"));
    }
    slot.invalid = true;
    Ok(())
  })
}

fn note_submit_destroyed(id: u64) -> Result<(), OpError> {
  ENCODERS.with(|encoders| {
    let mut encoders = encoders.borrow_mut();
    let slot = encoders.get_mut(&id).ok_or_else(|| {
      OpError::dom("InvalidStateError", "The command encoder is finished")
    })?;
    if slot.pass.is_some() {
      return Err(OpError::dom("InvalidStateError", "A pass is open"));
    }
    slot.submit_destroyed = true;
    Ok(())
  })
}

enum VertexBind {
  Invalid,
  /// wgpu panics on a zero-length slice. Offset 0 unsets the slot. A non-zero
  /// offset leaves the previous binding in place.
  Skip,
  Unset {
    slot: u32,
  },
  Bind {
    slot: u32,
    buffer: wgpu::Buffer,
    offset: u64,
    size: Option<u64>,
  },
}

enum IndexBind {
  Invalid,
  Skip,
  Bind {
    buffer: wgpu::Buffer,
    format: wgpu::IndexFormat,
    offset: u64,
    size: Option<u64>,
  },
}

#[allow(clippy::too_many_arguments)]
fn vertex_bind(
  scope: &mut v8::PinScope,
  slot: u32,
  value: v8::Local<v8::Value>,
  offset: Option<js::GpuSize64>,
  size: Option<js::GpuSize64>,
  max_vertex_buffers: u32,
  device_id: u64,
) -> Result<VertexBind, OpError> {
  let offset = offset.map(|value| value.0).unwrap_or(0);
  if value.is_null() {
    let size = size.map(|value| value.0).unwrap_or(0);
    if !offset.is_multiple_of(4)
      || offset.saturating_add(size) > 0
      || slot >= max_vertex_buffers
    {
      return Ok(VertexBind::Invalid);
    }
    return Ok(VertexBind::Unset { slot });
  }
  let host = host_gpu_buffer(scope, value, "A GPUBuffer is required")?;
  if host.device_id() != device_id {
    return Ok(VertexBind::Invalid);
  }
  let buffer = host.alive()?;
  let buffer_size = host.byte_size();
  let requested = size.as_ref().map(|value| value.0);
  let size_value =
    requested.unwrap_or_else(|| buffer_size.saturating_sub(offset));
  let in_range = offset.is_multiple_of(4)
    && offset.saturating_add(size_value) <= buffer_size
    && slot < max_vertex_buffers;
  let usable = host.is_valid()
    && host.usage_bits() & wgpu::BufferUsages::VERTEX.bits() != 0;
  if !in_range || !usable {
    return Ok(VertexBind::Invalid);
  }
  if size_value == 0 {
    if offset == 0 {
      return Ok(VertexBind::Unset { slot });
    }
    return Ok(VertexBind::Skip);
  }
  Ok(VertexBind::Bind {
    slot,
    buffer,
    offset,
    size: requested,
  })
}

fn push_vertex(record: &mut PassRecord, bind: VertexBind) {
  match bind {
    VertexBind::Invalid => record.invalid = true,
    VertexBind::Skip => {}
    VertexBind::Unset { slot } => record.commands.push(PassCommand::Vertex {
      slot,
      buffer: None,
      offset: 0,
      size: None,
    }),
    VertexBind::Bind {
      slot,
      buffer,
      offset,
      size,
    } => record.commands.push(PassCommand::Vertex {
      slot,
      buffer: Some(buffer),
      offset,
      size,
    }),
  }
}

fn index_bind(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
  format: &str,
  offset: Option<js::GpuSize64>,
  size: Option<js::GpuSize64>,
  device_id: u64,
) -> Result<IndexBind, OpError> {
  let format = resource::index_format(format)?;
  let host = host_gpu_buffer(scope, value, "A GPUBuffer is required")?;
  if host.device_id() != device_id {
    return Ok(IndexBind::Invalid);
  }
  let buffer = host.alive()?;
  let offset = offset.map(|value| value.0).unwrap_or(0);
  let buffer_size = host.byte_size();
  let requested = size.as_ref().map(|value| value.0);
  let size_value =
    requested.unwrap_or_else(|| buffer_size.saturating_sub(offset));
  let align = match format {
    wgpu::IndexFormat::Uint16 => 2,
    wgpu::IndexFormat::Uint32 => 4,
  };
  let in_range = offset.is_multiple_of(align)
    && offset.saturating_add(size_value) <= buffer_size;
  let usable = host.is_valid()
    && host.usage_bits() & wgpu::BufferUsages::INDEX.bits() != 0;
  if !in_range || !usable {
    return Ok(IndexBind::Invalid);
  }
  if size_value == 0 {
    return Ok(IndexBind::Skip);
  }
  Ok(IndexBind::Bind {
    buffer,
    format,
    offset,
    size: requested,
  })
}

fn push_index(record: &mut PassRecord, bind: IndexBind) {
  match bind {
    IndexBind::Invalid => record.invalid = true,
    IndexBind::Skip => {}
    IndexBind::Bind {
      buffer,
      format,
      offset,
      size,
    } => record.commands.push(PassCommand::Index {
      buffer,
      format,
      offset,
      size,
    }),
  }
}

fn indirect_bind(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
  offset: u64,
  bytes: u64,
  message: &str,
) -> Result<Option<(wgpu::Buffer, u64)>, OpError> {
  let host = host_gpu_buffer(scope, value, message)?;
  let buffer = host.alive()?;
  let in_range = offset.is_multiple_of(4)
    && offset.saturating_add(bytes) <= host.byte_size();
  let usable = host.is_valid()
    && host.usage_bits() & wgpu::BufferUsages::INDIRECT.bits() != 0;
  if !in_range || !usable {
    return Ok(None);
  }
  Ok(Some((buffer, offset)))
}

fn bind_group(
  scope: &mut v8::PinScope,
  group: v8::Local<v8::Value>,
  offsets: Option<v8::Local<v8::Value>>,
  start: Option<u64>,
  length: Option<u32>,
) -> Result<(Option<wgpu::BindGroup>, Vec<u32>), OpError> {
  let offsets = match offsets {
    Some(value) if !value.is_null_or_undefined() => {
      offset_list(scope, value, start, length)?
    }
    _ => Vec::new(),
  };
  if group.is_null_or_undefined() {
    if !offsets.is_empty() {
      return Err(OpError::type_(
        "A null bind group cannot have dynamic offsets",
      ));
    }
    return Ok((None, offsets));
  }
  let group = resource::host_bind_group(scope, group)
    .ok_or_else(|| OpError::type_("setBindGroup requires a GPUBindGroup"))?;
  Ok((Some(group.group().clone()), offsets))
}

fn offset_list(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
  start: Option<u64>,
  length: Option<u32>,
) -> Result<Vec<u32>, OpError> {
  let object = js::object(scope, value)
    .ok_or_else(|| OpError::type_("Dynamic offsets must be a sequence"))?;
  let total_value = js::get(scope, object, "length")?
    .ok_or_else(|| OpError::type_("Dynamic offsets must be a sequence"))?;
  let total =
    yuzuko_core::modulo_u32(yuzuko_core::to_number(scope, total_value)?);
  let start = start.unwrap_or(0);
  if start > u64::from(total) {
    return Err(OpError::range("The dynamic offset range is out of range"));
  }
  let start = u32::try_from(start)
    .map_err(|_| OpError::range("The dynamic offset range is out of range"))?;
  let length = length.unwrap_or(total.saturating_sub(start));
  if start.saturating_add(length) > total {
    return Err(OpError::range("The dynamic offset range is out of range"));
  }
  let mut offsets = Vec::with_capacity(length as usize);
  for index in start..start + length {
    let item = js::read_index(scope, object, index)?;
    offsets.push(js::u32_value(scope, item)?);
  }
  Ok(offsets)
}

pub struct SubmittedCommand {
  pub buffer: Option<wgpu::CommandBuffer>,
  pub device_id: u64,
  /// Already submitted, listed twice, finished invalid, or uses a destroyed
  /// resource. The JavaScript command buffer is still consumed.
  pub rejected: bool,
}

pub fn command_buffers(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<Vec<SubmittedCommand>, OpError> {
  let array = v8::Local::<v8::Array>::try_from(value).map_err(|_| {
    OpError::type_("submit requires a sequence of command buffers")
  })?;
  let mut seen = Vec::<*const GpuCommandBuffer>::new();
  let mut commands = Vec::new();
  for index in 0..array.length() {
    let value = js::read_index(scope, array, index)?;
    let object = v8::Local::<v8::Object>::try_from(value).map_err(|_| {
      OpError::type_("submit requires GPUCommandBuffer objects")
    })?;
    let host =
      unsafe { yuzuko_core::op::host_ref::<GpuCommandBuffer>(scope, object) }
        .ok_or_else(|| {
        OpError::type_("submit requires GPUCommandBuffer objects")
      })?;
    let ptr = host.get() as *const GpuCommandBuffer;
    let device_id = host.get().device_id();
    if seen.contains(&ptr) {
      commands.push(SubmittedCommand {
        buffer: None,
        device_id,
        rejected: true,
      });
      continue;
    }
    seen.push(ptr);
    match host.get().take() {
      Some((buffer, submit_destroyed, invalid)) => {
        commands.push(SubmittedCommand {
          buffer: Some(buffer),
          device_id,
          rejected: submit_destroyed || invalid,
        });
      }
      None => commands.push(SubmittedCommand {
        buffer: None,
        device_id,
        rejected: true,
      }),
    }
  }
  Ok(commands)
}
