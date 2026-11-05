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
    buffer: wgpu::Buffer,
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
}

struct ComputeRecord {
  timestamps: Option<TimestampWrite>,
  commands: Vec<ComputeCommand>,
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
}

struct BundleSlot {
  device: wgpu::Device,
  colors: Vec<Option<wgpu::TextureFormat>>,
  depth: Option<wgpu::RenderBundleDepthStencil>,
  sample_count: u32,
  commands: Vec<BundleCommand>,
}

enum BundleCommand {
  Pipeline(wgpu::RenderPipeline),
  Vertex {
    slot: u32,
    buffer: wgpu::Buffer,
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
  pub fn create(handle: &Handle, label: Option<String>) -> Self {
    let encoder =
      handle
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
          label: label.as_deref(),
        });
    let id = NEXT_ENCODER.fetch_add(1, Ordering::Relaxed);
    ENCODERS.with(|encoders| {
      encoders.borrow_mut().insert(
        id,
        EncoderSlot {
          encoder: Some(encoder),
          pass: None,
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
      }));
      Ok(())
    })?;
    Ok(New(GpuRenderPass {
      encoder: self.id,
      ended: Cell::new(false),
      label: ObjectLabel::new(resource::optional_label(scope, object)),
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
          resource::optional_label(scope, object),
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
      }));
      Ok(())
    })?;
    Ok(New(GpuComputePass {
      encoder: self.id,
      ended: Cell::new(false),
      label: ObjectLabel::new(label),
    }))
  }

  fn copy_buffer_to_buffer(
    &self,
    scope: &mut v8::PinScope,
    source: v8::Local<v8::Value>,
    source_offset: u32,
    destination: v8::Local<v8::Value>,
    destination_offset: u32,
    size: u32,
  ) -> Result<(), OpError> {
    if size == 0 {
      return Err(OpError::range("The copy size must be non-zero"));
    }
    let source =
      alive_buffer(scope, source, "The copy source is not a buffer")?;
    let destination =
      alive_buffer(scope, destination, "The copy destination is not a buffer")?;
    with_idle_encoder(self.id, |encoder| {
      encoder.copy_buffer_to_buffer(
        &source,
        u64::from(source_offset),
        &destination,
        u64::from(destination_offset),
        u64::from(size),
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
    let texture = js::get(scope, source, "texture")
      .and_then(|value| resource::host_texture(scope, value))
      .ok_or_else(|| OpError::type_("The copy source is not a texture"))?;
    let texture = texture.texture()?;
    let destination = js::object(scope, destination).ok_or_else(|| {
      OpError::type_("copyTextureToBuffer destination must be an object")
    })?;
    let buffer = js::get(scope, destination, "buffer")
      .and_then(|value| resource::host_buffer(scope, value))
      .ok_or_else(|| OpError::type_("The copy destination is not a buffer"))?;
    let buffer = buffer.alive()?;
    let offset =
      u64::from(js::field_u32(scope, destination, "offset")?.unwrap_or(0));
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

  fn resolve_query_set(
    &self,
    scope: &mut v8::PinScope,
    query_set: v8::Local<v8::Value>,
    first_query: u32,
    query_count: u32,
    destination: v8::Local<v8::Value>,
    destination_offset: u32,
  ) -> Result<(), OpError> {
    if query_count == 0 {
      return Err(OpError::range("The query count must be non-zero"));
    }
    let query_set =
      resource::host_query_set(scope, query_set).ok_or_else(|| {
        OpError::type_("resolveQuerySet requires a GPUQuerySet")
      })?;
    let destroyed = query_set.is_destroyed();
    let stored = query_set.query_count();
    let query_set = query_set.query_set();
    let destination = alive_buffer(
      scope,
      destination,
      "The query destination is not a buffer",
    )?;
    let end = first_query
      .checked_add(query_count)
      .ok_or_else(|| OpError::range("The query range is out of range"))?;
    // wgpu 29 has no `QuerySet::destroy`. An empty range does not overrun, so
    // a destroyed set resolves one index past the end. `u32::MAX` cannot, and
    // a misaligned offset is still a validation error.
    let (start, end, offset) = if !destroyed {
      (first_query, end, u64::from(destination_offset))
    } else if stored < u32::MAX {
      (stored, stored + 1, u64::from(destination_offset))
    } else {
      (first_query, end, 1)
    };
    with_idle_encoder(self.id, |encoder| {
      encoder.resolve_query_set(&query_set, start..end, &destination, offset);
      Ok(())
    })
  }

  fn push_debug_group(&self, label: String) -> Result<(), OpError> {
    with_idle_encoder(self.id, |encoder| {
      encoder.push_debug_group(&label);
      Ok(())
    })
  }

  fn pop_debug_group(&self) -> Result<(), OpError> {
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

  fn finish(
    &self,
    scope: &mut v8::PinScope,
    descriptor: Option<v8::Local<v8::Value>>,
  ) -> Result<New<GpuCommandBuffer>, OpError> {
    let encoder = ENCODERS.with(|encoders| {
      let mut encoders = encoders.borrow_mut();
      let slot = encoders.get_mut(&self.id).ok_or_else(|| {
        OpError::dom("InvalidStateError", "The command encoder is finished")
      })?;
      if slot.pass.is_some() {
        return Err(OpError::dom("InvalidStateError", "A pass is open"));
      }
      slot.encoder.take().ok_or_else(|| {
        OpError::dom("InvalidStateError", "The command encoder is finished")
      })
    })?;
    Ok(New(GpuCommandBuffer {
      buffer: RefCell::new(Some(encoder.finish())),
      label: ObjectLabel::new(descriptor_label(scope, descriptor)),
    }))
  }
}

pub struct GpuCommandBuffer {
  buffer: RefCell<Option<wgpu::CommandBuffer>>,
  label: ObjectLabel,
}

unsafe impl v8::cppgc::GarbageCollected for GpuCommandBuffer {
  fn trace(&self, _visitor: &mut v8::cppgc::Visitor) {}

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"GPUCommandBuffer"
  }
}

impl GpuCommandBuffer {
  pub fn take(&self) -> Option<wgpu::CommandBuffer> {
    self.buffer.borrow_mut().take()
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
    let (x, y, width, height, min_depth, max_depth) =
      viewport_args(x, y, width, height, min_depth, max_depth)?;
    self.push(PassCommand::Viewport {
      x,
      y,
      width,
      height,
      min_depth,
      max_depth,
    })
  }

  fn set_scissor_rect(
    &self,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
  ) -> Result<(), OpError> {
    self.push(PassCommand::Scissor {
      x,
      y,
      width,
      height,
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
    slot: u32,
    buffer: v8::Local<v8::Value>,
    offset: Option<u32>,
    size: Option<u32>,
  ) -> Result<(), OpError> {
    let (buffer, offset, size) = buffer_span(scope, buffer, offset, size)?;
    self.push(PassCommand::Vertex {
      slot,
      buffer,
      offset,
      size,
    })
  }

  fn set_index_buffer(
    &self,
    scope: &mut v8::PinScope,
    buffer: v8::Local<v8::Value>,
    format: String,
    offset: Option<u32>,
    size: Option<u32>,
  ) -> Result<(), OpError> {
    let format = resource::index_format(&format)?;
    let (buffer, offset, size) = buffer_span(scope, buffer, offset, size)?;
    self.push(PassCommand::Index {
      buffer,
      format,
      offset,
      size,
    })
  }

  fn set_bind_group(
    &self,
    scope: &mut v8::PinScope,
    index: u32,
    group: v8::Local<v8::Value>,
    offsets: Option<v8::Local<v8::Value>>,
    start: Option<u32>,
    length: Option<u32>,
  ) -> Result<(), OpError> {
    let (group, offsets) = bind_group(scope, group, offsets, start, length)?;
    self.push(PassCommand::Bind {
      index,
      group,
      offsets,
    })
  }

  fn draw(
    &self,
    vertices: u32,
    instances: Option<u32>,
    first_vertex: Option<u32>,
    first_instance: Option<u32>,
  ) -> Result<(), OpError> {
    self.push(PassCommand::Draw {
      vertices,
      instances: instances.unwrap_or(1),
      first_vertex: first_vertex.unwrap_or(0),
      first_instance: first_instance.unwrap_or(0),
    })
  }

  fn draw_indexed(
    &self,
    indices: u32,
    instances: Option<u32>,
    first_index: Option<u32>,
    base_vertex: Option<i32>,
    first_instance: Option<u32>,
  ) -> Result<(), OpError> {
    self.push(PassCommand::DrawIndexed {
      indices,
      instances: instances.unwrap_or(1),
      first_index: first_index.unwrap_or(0),
      base_vertex: base_vertex.unwrap_or(0),
      first_instance: first_instance.unwrap_or(0),
    })
  }

  fn draw_indirect(
    &self,
    scope: &mut v8::PinScope,
    buffer: v8::Local<v8::Value>,
    offset: u32,
  ) -> Result<(), OpError> {
    let buffer =
      alive_buffer(scope, buffer, "drawIndirect requires a GPUBuffer")?;
    self.push(PassCommand::DrawIndirect {
      buffer,
      offset: u64::from(offset),
    })
  }

  fn draw_indexed_indirect(
    &self,
    scope: &mut v8::PinScope,
    buffer: v8::Local<v8::Value>,
    offset: u32,
  ) -> Result<(), OpError> {
    let buffer =
      alive_buffer(scope, buffer, "drawIndexedIndirect requires a GPUBuffer")?;
    self.push(PassCommand::DrawIndexedIndirect {
      buffer,
      offset: u64::from(offset),
    })
  }

  fn begin_occlusion_query(&self, query_index: u32) -> Result<(), OpError> {
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
        .push(PassCommand::BeginOcclusion(query_index));
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
    for index in 0..array.length() {
      let value = array.get_index(scope, index).unwrap();
      let bundle = host_bundle(scope, value).ok_or_else(|| {
        OpError::type_("executeBundles requires GPURenderBundle objects")
      })?;
      recorded.push(bundle.bundle.clone());
    }
    self.push(PassCommand::Bundles(recorded))
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
    index: u32,
    group: v8::Local<v8::Value>,
    offsets: Option<v8::Local<v8::Value>>,
    start: Option<u32>,
    length: Option<u32>,
  ) -> Result<(), OpError> {
    let (group, offsets) = bind_group(scope, group, offsets, start, length)?;
    self.push(ComputeCommand::Bind {
      index,
      group,
      offsets,
    })
  }

  fn dispatch_workgroups(
    &self,
    x: u32,
    y: Option<u32>,
    z: Option<u32>,
  ) -> Result<(), OpError> {
    self.push(ComputeCommand::Dispatch {
      x,
      y: y.unwrap_or(1),
      z: z.unwrap_or(1),
    })
  }

  fn dispatch_workgroups_indirect(
    &self,
    scope: &mut v8::PinScope,
    buffer: v8::Local<v8::Value>,
    offset: u32,
  ) -> Result<(), OpError> {
    let buffer = alive_buffer(
      scope,
      buffer,
      "dispatchWorkgroupsIndirect requires a GPUBuffer",
    )?;
    self.push(ComputeCommand::DispatchIndirect {
      buffer,
      offset: u64::from(offset),
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
        Some(OpenPass::Compute(record)) => {
          record.commands.push(command);
          Ok(())
        }
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
        colors,
        depth,
        sample_count,
        commands: Vec::new(),
      },
    );
  });
  Ok(New(GpuRenderBundleEncoder {
    id,
    ended: Cell::new(false),
    label: ObjectLabel::new(resource::optional_label(scope, object)),
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
    slot: u32,
    buffer: v8::Local<v8::Value>,
    offset: Option<u32>,
    size: Option<u32>,
  ) -> Result<(), OpError> {
    let (buffer, offset, size) = buffer_span(scope, buffer, offset, size)?;
    self.push(BundleCommand::Vertex {
      slot,
      buffer,
      offset,
      size,
    })
  }

  fn set_index_buffer(
    &self,
    scope: &mut v8::PinScope,
    buffer: v8::Local<v8::Value>,
    format: String,
    offset: Option<u32>,
    size: Option<u32>,
  ) -> Result<(), OpError> {
    let format = resource::index_format(&format)?;
    let (buffer, offset, size) = buffer_span(scope, buffer, offset, size)?;
    self.push(BundleCommand::Index {
      buffer,
      format,
      offset,
      size,
    })
  }

  fn set_bind_group(
    &self,
    scope: &mut v8::PinScope,
    index: u32,
    group: v8::Local<v8::Value>,
    offsets: Option<v8::Local<v8::Value>>,
    start: Option<u32>,
    length: Option<u32>,
  ) -> Result<(), OpError> {
    let (group, offsets) = bind_group(scope, group, offsets, start, length)?;
    self.push(BundleCommand::Bind {
      index,
      group,
      offsets,
    })
  }

  fn draw(
    &self,
    vertices: u32,
    instances: Option<u32>,
    first_vertex: Option<u32>,
    first_instance: Option<u32>,
  ) -> Result<(), OpError> {
    self.push(BundleCommand::Draw {
      vertices,
      instances: instances.unwrap_or(1),
      first_vertex: first_vertex.unwrap_or(0),
      first_instance: first_instance.unwrap_or(0),
    })
  }

  fn draw_indexed(
    &self,
    indices: u32,
    instances: Option<u32>,
    first_index: Option<u32>,
    base_vertex: Option<i32>,
    first_instance: Option<u32>,
  ) -> Result<(), OpError> {
    self.push(BundleCommand::DrawIndexed {
      indices,
      instances: instances.unwrap_or(1),
      first_index: first_index.unwrap_or(0),
      base_vertex: base_vertex.unwrap_or(0),
      first_instance: first_instance.unwrap_or(0),
    })
  }

  fn draw_indirect(
    &self,
    scope: &mut v8::PinScope,
    buffer: v8::Local<v8::Value>,
    offset: u32,
  ) -> Result<(), OpError> {
    let buffer =
      alive_buffer(scope, buffer, "drawIndirect requires a GPUBuffer")?;
    self.push(BundleCommand::DrawIndirect {
      buffer,
      offset: u64::from(offset),
    })
  }

  fn draw_indexed_indirect(
    &self,
    scope: &mut v8::PinScope,
    buffer: v8::Local<v8::Value>,
    offset: u32,
  ) -> Result<(), OpError> {
    let buffer =
      alive_buffer(scope, buffer, "drawIndexedIndirect requires a GPUBuffer")?;
    self.push(BundleCommand::DrawIndexedIndirect {
      buffer,
      offset: u64::from(offset),
    })
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
    let bundle = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
      finish_bundle(slot)
    }));
    let bundle = bundle.map_err(|_| {
      OpError::dom("OperationError", "The render bundle is invalid")
    })?;
    Ok(New(GpuRenderBundle {
      bundle,
      label: ObjectLabel::new(descriptor_label(scope, descriptor)),
    }))
  }
}

impl GpuRenderBundleEncoder {
  fn push(&self, command: BundleCommand) -> Result<(), OpError> {
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
      slot.commands.push(command);
      Ok(())
    })
  }
}

pub struct GpuRenderBundle {
  bundle: wgpu::RenderBundle,
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
) -> Option<String> {
  let value = descriptor?;
  if value.is_null_or_undefined() {
    return None;
  }
  let object = js::object(scope, value)?;
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
      } => pass.set_vertex_buffer(*slot, buffer_slice(buffer, *offset, *size)),
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
    }
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
      } => {
        encoder.set_vertex_buffer(*slot, buffer_slice(buffer, *offset, *size))
      }
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

fn viewport_args(
  x: f64,
  y: f64,
  width: f64,
  height: f64,
  min_depth: f64,
  max_depth: f64,
) -> Result<(f32, f32, f32, f32, f32, f32), OpError> {
  let values = [x, y, width, height, min_depth, max_depth];
  if values.iter().any(|value| !value.is_finite()) {
    return Err(OpError::type_("setViewport arguments must be finite"));
  }
  if width < 0.0
    || height < 0.0
    || !(0.0..=1.0).contains(&min_depth)
    || !(0.0..=1.0).contains(&max_depth)
    || min_depth > max_depth
  {
    return Err(OpError::range("The viewport is out of range"));
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
    return Err(OpError::range("The viewport is out of range"));
  }
  Ok((cast[0], cast[1], cast[2], cast[3], cast[4], cast[5]))
}

fn color_targets(
  scope: &mut v8::PinScope,
  descriptor: v8::Local<v8::Object>,
) -> Result<Vec<Option<ColorTarget>>, OpError> {
  let value = js::get(scope, descriptor, "colorAttachments")
    .ok_or_else(|| OpError::type_("colorAttachments is required"))?;
  let array = v8::Local::<v8::Array>::try_from(value)
    .map_err(|_| OpError::type_("colorAttachments must be an array"))?;
  let mut colors = Vec::new();
  for index in 0..array.length() {
    let value = array.get_index(scope, index).unwrap();
    if value.is_null() {
      colors.push(None);
      continue;
    }
    let attachment = js::object(scope, value)
      .ok_or_else(|| OpError::type_("A color attachment must be an object"))?;
    let resolve = match js::get(scope, attachment, "resolveTarget") {
      Some(value) if !value.is_null_or_undefined() => {
        Some(resource::attachment_view(scope, value)?.0)
      }
      _ => None,
    };
    let view = js::get(scope, attachment, "view")
      .ok_or_else(|| OpError::type_("A color attachment view is required"))?;
    let (view, _) = resource::attachment_view(scope, view)?;
    let load = js::get(scope, attachment, "loadOp")
      .and_then(|value| js::string(scope, value))
      .unwrap_or_else(|| "load".to_string());
    let clear = if load == "clear" {
      let clear_value = js::get(scope, attachment, "clearValue");
      Some(clear_color(scope, clear_value))
    } else {
      None
    };
    let store = js::get(scope, attachment, "storeOp")
      .and_then(|value| js::string(scope, value))
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
  let Some(value) = js::get(scope, descriptor, "depthStencilAttachment") else {
    return Ok(None);
  };
  if value.is_null_or_undefined() {
    return Ok(None);
  }
  let attachment = js::object(scope, value)
    .ok_or_else(|| OpError::type_("A depth attachment must be an object"))?;
  let view = js::get(scope, attachment, "view")
    .filter(|value| !value.is_null_or_undefined())
    .ok_or_else(|| OpError::type_("A depth attachment view is required"))?;
  let (view, format) = resource::attachment_view(scope, view)?;
  let depth_ops = aspect_ops(
    scope,
    attachment,
    format.has_depth_aspect(),
    "depth",
    |scope, attachment| {
      js::get(scope, attachment, "depthClearValue")
        .and_then(|value| value.number_value(scope))
        .unwrap_or(0.0) as f32
    },
  )?;
  let stencil_ops = aspect_ops(
    scope,
    attachment,
    format.has_stencil_aspect(),
    "stencil",
    |scope, attachment| {
      js::field_u32(scope, attachment, "stencilClearValue")
        .ok()
        .flatten()
        .unwrap_or(0)
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
  clear_value: impl FnOnce(&mut v8::PinScope, v8::Local<v8::Object>) -> T,
) -> Result<Option<wgpu::Operations<T>>, OpError> {
  let read_only_name = format!("{prefix}ReadOnly");
  let load_name = format!("{prefix}LoadOp");
  let store_name = format!("{prefix}StoreOp");
  let read_only = js::get(scope, attachment, &read_only_name)
    .is_some_and(|value| value.boolean_value(scope));
  let load = js::get(scope, attachment, &load_name)
    .filter(|value| !value.is_null_or_undefined())
    .and_then(|value| js::string(scope, value));
  let store = js::get(scope, attachment, &store_name)
    .filter(|value| !value.is_null_or_undefined())
    .and_then(|value| js::string(scope, value));
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
    "clear" => wgpu::LoadOp::Clear(clear_value(scope, attachment)),
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
  let Some(value) = js::get(scope, descriptor, "occlusionQuerySet") else {
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
  let Some(value) = js::get(scope, descriptor, "timestampWrites") else {
    return Ok(None);
  };
  if value.is_null_or_undefined() {
    return Ok(None);
  }
  let writes = js::object(scope, value)
    .ok_or_else(|| OpError::type_("timestampWrites must be an object"))?;
  let query_value = js::get(scope, writes, "querySet")
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
  let value = js::get(scope, descriptor, "colorFormats")
    .ok_or_else(|| OpError::type_("colorFormats is required"))?;
  let array = v8::Local::<v8::Array>::try_from(value)
    .map_err(|_| OpError::type_("colorFormats must be an array"))?;
  let mut formats = Vec::new();
  for index in 0..array.length() {
    let value = array.get_index(scope, index).unwrap();
    if value.is_null() {
      formats.push(None);
      continue;
    }
    let name = js::string(scope, value)
      .ok_or_else(|| OpError::type_("A color format is required"))?;
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
  let Some(value) = js::get(scope, descriptor, "depthStencilFormat") else {
    return Ok(None);
  };
  if value.is_null_or_undefined() {
    return Ok(None);
  }
  let name = js::string(scope, value)
    .ok_or_else(|| OpError::type_("depthStencilFormat is invalid"))?;
  let format = resource::require_texture_format(
    features,
    resource::texture_format(&name)?,
  )?;
  let depth_read_only = js::get(scope, descriptor, "depthReadOnly")
    .is_some_and(|value| value.boolean_value(scope));
  let stencil_read_only = js::get(scope, descriptor, "stencilReadOnly")
    .is_some_and(|value| value.boolean_value(scope));
  Ok(Some(wgpu::RenderBundleDepthStencil {
    format,
    depth_read_only: depth_read_only || !format.has_depth_aspect(),
    stencil_read_only: stencil_read_only || !format.has_stencil_aspect(),
  }))
}

fn clear_color(
  scope: &mut v8::PinScope,
  value: Option<v8::Local<v8::Value>>,
) -> wgpu::Color {
  let Some(value) = value else {
    return wgpu::Color::TRANSPARENT;
  };
  if let Some(object) = js::object(scope, value) {
    let mut channel = |name, default| {
      js::get(scope, object, name)
        .and_then(|value| value.number_value(scope))
        .unwrap_or(default)
    };
    return wgpu::Color {
      r: channel("r", 0.0),
      g: channel("g", 0.0),
      b: channel("b", 0.0),
      a: channel("a", 1.0),
    };
  }
  wgpu::Color::TRANSPARENT
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

fn alive_buffer(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
  message: &str,
) -> Result<wgpu::Buffer, OpError> {
  let buffer = resource::host_buffer(scope, value)
    .ok_or_else(|| OpError::type_(message))?;
  buffer.alive()
}

fn buffer_span(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
  offset: Option<u32>,
  size: Option<u32>,
) -> Result<(wgpu::Buffer, u64, Option<u64>), OpError> {
  let buffer = alive_buffer(scope, value, "A GPUBuffer is required")?;
  Ok((buffer, u64::from(offset.unwrap_or(0)), size.map(u64::from)))
}

fn bind_group(
  scope: &mut v8::PinScope,
  group: v8::Local<v8::Value>,
  offsets: Option<v8::Local<v8::Value>>,
  start: Option<u32>,
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
  start: Option<u32>,
  length: Option<u32>,
) -> Result<Vec<u32>, OpError> {
  let object = js::object(scope, value)
    .ok_or_else(|| OpError::type_("Dynamic offsets must be a sequence"))?;
  let total = js::get(scope, object, "length")
    .and_then(|value| value.uint32_value(scope))
    .ok_or_else(|| OpError::type_("Dynamic offsets must be a sequence"))?;
  let start = start.unwrap_or(0);
  let length = length.unwrap_or(total.saturating_sub(start));
  if start.saturating_add(length) > total {
    return Err(OpError::range("The dynamic offset range is out of range"));
  }
  let mut offsets = Vec::with_capacity(length as usize);
  for index in start..start + length {
    let item = object.get_index(scope, index).unwrap();
    offsets.push(js::u32_value(scope, item)?);
  }
  Ok(offsets)
}

pub fn command_buffers(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<Vec<wgpu::CommandBuffer>, OpError> {
  let array = v8::Local::<v8::Array>::try_from(value).map_err(|_| {
    OpError::type_("submit requires a sequence of command buffers")
  })?;
  let mut buffers = Vec::new();
  for index in 0..array.length() {
    let value = array.get_index(scope, index).unwrap();
    let object = v8::Local::<v8::Object>::try_from(value).map_err(|_| {
      OpError::type_("submit requires GPUCommandBuffer objects")
    })?;
    let host =
      unsafe { yuzuko_core::op::host_ref::<GpuCommandBuffer>(scope, object) }
        .ok_or_else(|| {
        OpError::type_("submit requires GPUCommandBuffer objects")
      })?;
    let buffer = host.get().take().ok_or_else(|| {
      OpError::dom(
        "InvalidStateError",
        "The command buffer was already submitted",
      )
    })?;
    buffers.push(buffer);
  }
  Ok(buffers)
}
