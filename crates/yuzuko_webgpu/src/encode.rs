//! Command encoders. A render pass records commands and replays them on end.

use std::cell::{Cell, RefCell};
use std::sync::atomic::{AtomicU64, Ordering};

use yuzuko_core::op::{New, OpError};
use yuzuko_macros::op;

use crate::js;
use crate::resource;
use crate::state::Handle;

static NEXT_ENCODER: AtomicU64 = AtomicU64::new(1);

struct EncoderSlot {
  encoder: Option<wgpu::CommandEncoder>,
  pass: Option<PassRecord>,
}

struct PassRecord {
  colors: Vec<ColorTarget>,
  commands: Vec<PassCommand>,
}

struct ColorTarget {
  view: wgpu::TextureView,
  clear: Option<wgpu::Color>,
  store: bool,
}

enum PassCommand {
  Pipeline(wgpu::RenderPipeline),
  Vertex {
    slot: u32,
    buffer: wgpu::Buffer,
    offset: u64,
    size: Option<u64>,
  },
  Bind {
    index: u32,
    group: wgpu::BindGroup,
  },
  Draw {
    vertices: u32,
    instances: u32,
    first_vertex: u32,
    first_instance: u32,
  },
}

std::thread_local! {
  static ENCODERS: RefCell<std::collections::HashMap<u64, EncoderSlot>> =
    RefCell::new(std::collections::HashMap::new());
}

pub struct GpuCommandEncoder {
  id: u64,
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
    Self { id }
  }
}

#[op(name = "GPUCommandEncoder")]
impl GpuCommandEncoder {
  #[getter]
  fn label(&self) -> String {
    String::new()
  }

  fn begin_render_pass(
    &self,
    scope: &mut v8::PinScope,
    descriptor: v8::Local<v8::Value>,
  ) -> Result<New<GpuRenderPass>, OpError> {
    let object = js::object(scope, descriptor)
      .ok_or_else(|| OpError::type_("beginRenderPass requires a descriptor"))?;
    if js::get(scope, object, "depthStencilAttachment")
      .is_some_and(|value| !value.is_null_or_undefined())
    {
      return Err(js::unsupported("depth and stencil attachments"));
    }
    if js::get(scope, object, "occlusionQuerySet")
      .is_some_and(|value| !value.is_null_or_undefined())
    {
      return Err(js::unsupported("occlusion queries"));
    }
    if js::get(scope, object, "timestampWrites")
      .is_some_and(|value| !value.is_null_or_undefined())
    {
      return Err(js::unsupported("timestamp queries"));
    }
    let colors = color_targets(scope, object)?;
    ENCODERS.with(|encoders| {
      let mut encoders = encoders.borrow_mut();
      let slot = encoders.get_mut(&self.id).ok_or_else(|| {
        OpError::dom("InvalidStateError", "The command encoder is finished")
      })?;
      if slot.pass.is_some() {
        return Err(OpError::dom(
          "InvalidStateError",
          "A render pass is already open",
        ));
      }
      slot.pass = Some(PassRecord {
        colors,
        commands: Vec::new(),
      });
      Ok(())
    })?;
    Ok(New(GpuRenderPass {
      encoder: self.id,
      ended: Cell::new(false),
    }))
  }

  fn begin_compute_pass(&self) -> Result<(), OpError> {
    Err(js::unsupported("GPUComputePassEncoder"))
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
    ENCODERS.with(|encoders| {
      let mut encoders = encoders.borrow_mut();
      let slot = encoders.get_mut(&self.id).ok_or_else(|| {
        OpError::dom("InvalidStateError", "The command encoder is finished")
      })?;
      if slot.pass.is_some() {
        return Err(OpError::dom("InvalidStateError", "A render pass is open"));
      }
      let encoder = slot.encoder.as_mut().ok_or_else(|| {
        OpError::dom("InvalidStateError", "The command encoder is finished")
      })?;
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

  fn finish(&self) -> Result<New<GpuCommandBuffer>, OpError> {
    let encoder = ENCODERS.with(|encoders| {
      let mut encoders = encoders.borrow_mut();
      let slot = encoders.get_mut(&self.id).ok_or_else(|| {
        OpError::dom("InvalidStateError", "The command encoder is finished")
      })?;
      if slot.pass.is_some() {
        return Err(OpError::dom("InvalidStateError", "A render pass is open"));
      }
      slot.encoder.take().ok_or_else(|| {
        OpError::dom("InvalidStateError", "The command encoder is finished")
      })
    })?;
    Ok(New(GpuCommandBuffer {
      buffer: RefCell::new(Some(encoder.finish())),
    }))
  }
}

pub struct GpuCommandBuffer {
  buffer: RefCell<Option<wgpu::CommandBuffer>>,
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
    String::new()
  }
}

pub struct GpuRenderPass {
  encoder: u64,
  ended: Cell<bool>,
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
    String::new()
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
    let buffer = resource::host_buffer(scope, buffer)
      .ok_or_else(|| OpError::type_("setVertexBuffer requires a GPUBuffer"))?;
    self.push(PassCommand::Vertex {
      slot,
      buffer: buffer.alive()?,
      offset: u64::from(offset.unwrap_or(0)),
      size: size.map(u64::from),
    })
  }

  fn set_bind_group(
    &self,
    scope: &mut v8::PinScope,
    index: u32,
    group: v8::Local<v8::Value>,
  ) -> Result<(), OpError> {
    let group = resource::host_bind_group(scope, group)
      .ok_or_else(|| OpError::type_("setBindGroup requires a GPUBindGroup"))?;
    self.push(PassCommand::Bind {
      index,
      group: group.group().clone(),
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
      let record = slot.pass.take().ok_or_else(|| {
        OpError::dom("InvalidStateError", "The render pass has ended")
      })?;
      let encoder = slot.encoder.as_mut().ok_or_else(|| {
        OpError::dom("InvalidStateError", "The command encoder is finished")
      })?;
      replay(encoder, record);
      Ok(())
    })
  }
}

impl GpuRenderPass {
  fn push(&self, command: PassCommand) -> Result<(), OpError> {
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
      let pass = slot.pass.as_mut().ok_or_else(|| {
        OpError::dom("InvalidStateError", "The render pass has ended")
      })?;
      pass.commands.push(command);
      Ok(())
    })
  }
}

fn replay(encoder: &mut wgpu::CommandEncoder, record: PassRecord) {
  let attachments = record
    .colors
    .iter()
    .map(|color| {
      Some(wgpu::RenderPassColorAttachment {
        view: &color.view,
        depth_slice: None,
        resolve_target: None,
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
  let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
    label: Some("yuzuko-webgpu"),
    color_attachments: &attachments,
    depth_stencil_attachment: None,
    timestamp_writes: None,
    occlusion_query_set: None,
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
      } => {
        let slice = match size {
          Some(size) => buffer.slice(*offset..*offset + *size),
          None => buffer.slice(*offset..),
        };
        pass.set_vertex_buffer(*slot, slice);
      }
      PassCommand::Bind { index, group } => {
        pass.set_bind_group(*index, group, &[]);
      }
      PassCommand::Draw {
        vertices,
        instances,
        first_vertex,
        first_instance,
      } => pass.draw(
        *first_vertex..*first_vertex + *vertices,
        *first_instance..*first_instance + *instances,
      ),
    }
  }
}

fn color_targets(
  scope: &mut v8::PinScope,
  descriptor: v8::Local<v8::Object>,
) -> Result<Vec<ColorTarget>, OpError> {
  let value = js::get(scope, descriptor, "colorAttachments")
    .ok_or_else(|| OpError::type_("colorAttachments is required"))?;
  let array = v8::Local::<v8::Array>::try_from(value)
    .map_err(|_| OpError::type_("colorAttachments must be an array"))?;
  let mut colors = Vec::new();
  for index in 0..array.length() {
    let value = array.get_index(scope, index).unwrap();
    if value.is_null() {
      continue;
    }
    let attachment = js::object(scope, value)
      .ok_or_else(|| OpError::type_("A color attachment must be an object"))?;
    let view = js::get(scope, attachment, "view")
      .and_then(|value| {
        let object = v8::Local::<v8::Object>::try_from(value).ok()?;
        let host = unsafe {
          yuzuko_core::op::host_ref::<resource::GpuTextureView>(scope, object)
        }?;
        Some(host.get().view())
      })
      .ok_or_else(|| OpError::type_("A color attachment view is required"))??;
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
    colors.push(ColorTarget { view, clear, store });
  }
  if colors.is_empty() {
    return Err(OpError::type_("A render pass needs a color attachment"));
  }
  Ok(colors)
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
