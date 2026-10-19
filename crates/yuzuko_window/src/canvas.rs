//! `OffscreenCanvas`. Drawing contexts are installed by `yuzuko_canvas`.

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{LazyLock, Mutex};

use yuzuko_core::op::{Js, OpError};
use yuzuko_dom::EventTarget;
use yuzuko_macros::op;

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContextKind {
  None,
  TwoD,
  Bitmap,
  WebGpu,
}

/// Unpremultiplied canvas pixels. Absent when the canvas has no context.
pub struct CanvasTicket {
  pub width: f64,
  pub height: f64,
  pub slot: Option<Box<dyn Any + Send>>,
  /// Window whose bitmap this canvas presents, when control was transferred.
  pub placeholder: Option<crate::surface::Placeholder>,
}

pub struct CanvasSnapshot {
  pub width: u32,
  pub height: u32,
  pub display_p3: bool,
  pub float16: bool,
  pub bytes: Vec<u8>,
}

pub type GetContextFn = fn(
  &mut v8::PinScope,
  &OffscreenCanvas,
  v8::Global<v8::Object>,
  String,
  Option<v8::Global<v8::Value>>,
) -> Result<Js, OpError>;

pub type ConvertToBlobFn = fn(
  &mut v8::PinScope,
  u64,
  Option<v8::Global<v8::Value>>,
) -> Result<Js, OpError>;

pub struct CanvasHooks {
  pub get_context: GetContextFn,
  pub convert_to_blob: ConvertToBlobFn,
  pub transfer_to_image_bitmap:
    fn(&mut v8::PinScope, u64) -> Result<Js, OpError>,
  pub resized: fn(u64, f64, f64),
  pub snapshot: fn(u64) -> Option<CanvasSnapshot>,
  pub present: fn(
    &wgpu::Device,
    &wgpu::Queue,
    &wgpu::TextureView,
    u64,
    u32,
    u32,
    wgpu::TextureFormat,
  ) -> bool,
  pub dispose: fn(u64),
  pub take_slot: fn(u64) -> Option<Box<dyn Any + Send>>,
  pub put_slot: fn(u64, Box<dyn Any + Send>),
  pub install_presented: fn(u64, CanvasSnapshot),
}

fn hooks() -> Option<&'static CanvasHooks> {
  HOOKS.get()
}

pub struct WebGpuHooks {
  pub get_context: GetContextFn,
  pub present: fn(
    &wgpu::Device,
    &wgpu::Queue,
    &wgpu::TextureView,
    u64,
    u32,
    u32,
    wgpu::TextureFormat,
  ) -> bool,
  pub expire: fn(u64),
  pub resized: fn(u64),
  pub dispose: fn(u64),
}

fn webgpu_hooks() -> Option<&'static WebGpuHooks> {
  WEBGPU
    .try_with(|slot| slot.get().map(|hooks| unsafe { &*hooks }))
    .ok()
    .flatten()
}

static HOOKS: std::sync::OnceLock<CanvasHooks> = std::sync::OnceLock::new();

std::thread_local! {
  static WEBGPU: Cell<Option<*const WebGpuHooks>> = const { Cell::new(None) };
}

pub fn install_hooks(hooks: CanvasHooks) {
  let _ = HOOKS.set(hooks);
}

pub fn install_webgpu_hooks(hooks: WebGpuHooks) {
  let leaked = Box::leak(Box::new(hooks)) as *const WebGpuHooks;
  WEBGPU.with(|slot| slot.set(Some(leaked)));
}

pub fn present_webgpu(
  device: &wgpu::Device,
  queue: &wgpu::Queue,
  view: &wgpu::TextureView,
  id: u64,
  width: u32,
  height: u32,
  format: wgpu::TextureFormat,
) -> bool {
  webgpu_hooks().is_some_and(|hooks| {
    (hooks.present)(device, queue, view, id, width, height, format)
  })
}

pub(crate) fn webgpu_after_frame(id: u64) {
  if !crate::session::headless() {
    return;
  }
  if let Some(hooks) = webgpu_hooks() {
    (hooks.expire)(id);
  }
}

pub fn snapshot_canvas(id: u64) -> Option<CanvasSnapshot> {
  hooks().and_then(|hooks| (hooks.snapshot)(id))
}

pub fn install_presented_bitmap(id: u64, snapshot: CanvasSnapshot) {
  if let Some(hooks) = hooks() {
    (hooks.install_presented)(id, snapshot);
  }
}

pub fn reset_canvas_bitmap(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  width: f64,
  height: f64,
) {
  if let Some(host) =
    unsafe { yuzuko_core::op::host_ref::<OffscreenCanvas>(scope, object) }
  {
    host.get().reset_bitmap(width, height);
    return;
  }
  if let Some(host) = unsafe {
    yuzuko_core::op::host_ref::<crate::surface::WindowSurface>(scope, object)
  } {
    host.get().resize_bitmap(width, height);
  }
}

pub fn present_canvas(
  device: &wgpu::Device,
  queue: &wgpu::Queue,
  view: &wgpu::TextureView,
  id: u64,
  width: u32,
  height: u32,
  format: wgpu::TextureFormat,
) -> bool {
  hooks().is_some_and(|hooks| {
    (hooks.present)(device, queue, view, id, width, height, format)
  })
}

#[repr(C)]
pub struct OffscreenCanvas {
  pub base: EventTarget,
  id: u64,
  width: Cell<f64>,
  height: Cell<f64>,
  kind: Cell<ContextKind>,
  detached: Cell<bool>,
  context: RefCell<Option<v8::TracedReference<v8::Object>>>,
}

unsafe impl v8::cppgc::GarbageCollected for OffscreenCanvas {
  fn trace(&self, visitor: &mut v8::cppgc::Visitor) {
    v8::cppgc::GarbageCollected::trace(&self.base, visitor);
    if let Ok(context) = self.context.try_borrow()
      && let Some(context) = context.as_ref()
    {
      visitor.trace(context);
    }
  }

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"OffscreenCanvas"
  }
}

impl Drop for OffscreenCanvas {
  fn drop(&mut self) {
    if let Some(link) = crate::surface::take_placeholder(self.id) {
      crate::surface::queue_rebind(link);
    }
    if let Some(hooks) = hooks() {
      (hooks.dispose)(self.id);
    }
    if let Some(hooks) = webgpu_hooks() {
      (hooks.dispose)(self.id);
    }
  }
}

impl OffscreenCanvas {
  pub fn create(width: f64, height: f64) -> Self {
    Self {
      base: EventTarget::new(),
      id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
      width: Cell::new(width),
      height: Cell::new(height),
      kind: Cell::new(ContextKind::None),
      detached: Cell::new(false),
      context: RefCell::new(None),
    }
  }

  pub fn is_detached(&self) -> bool {
    self.detached.get()
  }

  fn ensure_attached(&self) -> Result<(), OpError> {
    if self.detached.get() {
      return Err(OpError::dom(
        "InvalidStateError",
        "The canvas has been transferred",
      ));
    }
    Ok(())
  }

  /// Moves the bitmap to another thread and neuters this canvas.
  pub fn detach_for_transfer(&self) -> CanvasTicket {
    let width = self.width.get();
    let height = self.height.get();
    let slot = hooks().and_then(|hooks| (hooks.take_slot)(self.id));
    let placeholder = crate::surface::take_placeholder(self.id);
    self.detached.set(true);
    self.width.set(0.0);
    self.height.set(0.0);
    self.kind.set(ContextKind::None);
    *self.context.borrow_mut() = None;
    CanvasTicket {
      width,
      height,
      slot,
      placeholder,
    }
  }

  pub fn id(&self) -> u64 {
    self.id
  }

  pub fn bitmap(&self) -> (f64, f64) {
    (self.width.get(), self.height.get())
  }

  pub fn context_kind(&self) -> ContextKind {
    self.kind.get()
  }

  /// Updates the bitmap size. Resets a drawing context when the size changes.
  pub fn set_bitmap(&self, width: f64, height: f64) {
    self.assign_bitmap(width, height, false);
  }

  /// Sets the bitmap size and resets a drawing context even when unchanged.
  pub fn reset_bitmap(&self, width: f64, height: f64) {
    self.assign_bitmap(width, height, true);
  }

  fn assign_bitmap(&self, width: f64, height: f64, always: bool) {
    let changed = self.width.get() != width || self.height.get() != height;
    self.width.set(width);
    self.height.set(height);
    if always || changed {
      if let Some(hooks) = hooks() {
        (hooks.resized)(self.id, width, height);
      }
      if let Some(hooks) = webgpu_hooks() {
        (hooks.resized)(self.id);
      }
    }
  }

  pub fn context_object<'s>(
    &self,
    scope: &mut v8::PinScope<'s, '_>,
  ) -> Option<v8::Local<'s, v8::Object>> {
    let context = self.context.borrow();
    let reference = context.as_ref()?;
    yuzuko_core::op::traced_local(scope, reference)
  }

  pub fn store_context(
    &self,
    scope: &mut v8::PinScope,
    kind: ContextKind,
    object: v8::Local<v8::Object>,
  ) {
    self.kind.set(kind);
    *self.context.borrow_mut() = Some(yuzuko_core::op::traced(scope, object));
  }

  pub fn set_width(&self, value: f64) -> Result<(), OpError> {
    self.ensure_attached()?;
    self.reset_bitmap(enforce_size(value)?, self.height.get());
    Ok(())
  }

  pub fn set_height(&self, value: f64) -> Result<(), OpError> {
    self.ensure_attached()?;
    self.reset_bitmap(self.width.get(), enforce_size(value)?);
    Ok(())
  }

  pub fn open_context(
    &self,
    scope: &mut v8::PinScope,
    this: v8::Local<v8::Object>,
    type_: String,
    options: Option<v8::Local<v8::Value>>,
  ) -> Result<Js, OpError> {
    self.ensure_attached()?;
    let this_global = v8::Global::new(scope, this);
    let options = options
      .filter(|value| !value.is_null_or_undefined())
      .map(|value| v8::Global::new(scope, value));
    if type_ == "webgpu" {
      let Some(hooks) = webgpu_hooks() else {
        return Ok(Js::null(scope));
      };
      return (hooks.get_context)(scope, self, this_global, type_, options);
    }
    let Some(hooks) = hooks() else {
      return Ok(Js::null(scope));
    };
    (hooks.get_context)(scope, self, this_global, type_, options)
  }

  pub fn to_image_bitmap(
    &self,
    scope: &mut v8::PinScope,
  ) -> Result<Js, OpError> {
    self.ensure_attached()?;
    let Some(hooks) = hooks() else {
      return Err(OpError::dom(
        "NotSupportedError",
        "transferToImageBitmap is not supported",
      ));
    };
    (hooks.transfer_to_image_bitmap)(scope, self.id)
  }

  pub fn to_blob(
    &self,
    scope: &mut v8::PinScope,
    options: Option<v8::Local<v8::Value>>,
  ) -> Result<Js, OpError> {
    self.ensure_attached()?;
    let Some(hooks) = hooks() else {
      return Err(OpError::dom(
        "NotSupportedError",
        "convertToBlob is not supported",
      ));
    };
    let options = options
      .filter(|value| !value.is_null_or_undefined())
      .map(|value| v8::Global::new(scope, value));
    (hooks.convert_to_blob)(scope, self.id, options)
  }
}

pub fn enforce_size(number: f64) -> Result<f64, OpError> {
  if !number.is_finite() {
    return Err(OpError::type_(
      "The provided value cannot be converted to 'unsigned long long'.",
    ));
  }
  let rounded = number.round_ties_even();
  if rounded < 0.0 || rounded > 9_007_199_254_740_991.0 {
    return Err(OpError::type_(
      "The provided value is outside the 'unsigned long long' range.",
    ));
  }
  Ok(rounded)
}

fn argument_number(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<f64, OpError> {
  <f64 as yuzuko_core::op::FromV8>::from_v8(scope, value)
}

#[op(inherit = EventTarget)]
impl OffscreenCanvas {
  #[constructor]
  fn new(
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<OffscreenCanvas, OpError> {
    if args.length() < 2 {
      return Err(OpError::type_(
        "OffscreenCanvas requires a width and a height",
      ));
    }
    let width = enforce_size(argument_number(scope, args.get(0))?)?;
    let height = enforce_size(argument_number(scope, args.get(1))?)?;
    Ok(OffscreenCanvas::create(width, height))
  }

  #[getter]
  fn width(&self) -> f64 {
    self.width.get()
  }

  #[setter]
  fn width(&self, value: f64) -> Result<(), OpError> {
    self.set_width(value)
  }

  #[getter]
  fn height(&self) -> f64 {
    self.height.get()
  }

  #[setter]
  fn height(&self, value: f64) -> Result<(), OpError> {
    self.set_height(value)
  }

  fn get_context(
    &self,
    scope: &mut v8::PinScope,
    #[this] this: v8::Local<v8::Object>,
    type_: String,
    options: Option<v8::Local<v8::Value>>,
  ) -> Result<Js, OpError> {
    self.open_context(scope, this, type_, options)
  }

  fn transfer_to_image_bitmap(
    &self,
    scope: &mut v8::PinScope,
  ) -> Result<Js, OpError> {
    self.to_image_bitmap(scope)
  }

  fn convert_to_blob(
    &self,
    scope: &mut v8::PinScope,
    options: Option<v8::Local<v8::Value>>,
  ) -> Result<Js, OpError> {
    self.to_blob(scope, options)
  }
}

/// Bitmap size of an `OffscreenCanvas`, including a `Window`.
pub fn offscreen_bitmap(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Option<(f64, f64)> {
  offscreen_canvas(scope, value).map(|(_, width, height)| (width, height))
}

/// Identity and bitmap size of an `OffscreenCanvas`.
pub fn offscreen_canvas(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Option<(u64, f64, f64)> {
  let object = v8::Local::<v8::Object>::try_from(value).ok()?;
  let host =
    unsafe { yuzuko_core::op::host_ref::<OffscreenCanvas>(scope, object) }?;
  let canvas = host.get();
  let (width, height) = canvas.bitmap();
  Some((canvas.id(), width, height))
}

const TAG_CANVAS: u32 = 13;

static CANVAS_TICKETS: LazyLock<Mutex<HashMap<u64, CanvasTicket>>> =
  LazyLock::new(|| Mutex::new(HashMap::new()));
static NEXT_TICKET: AtomicU64 = AtomicU64::new(1);

pub fn register_transfer() {
  yuzuko_core::add_host_clone(write_canvas, read_canvas);
}

fn write_canvas(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  helper: &dyn v8::ValueSerializerHelper,
) -> yuzuko_core::WriteResult {
  let Some(canvas) =
    (unsafe { yuzuko_core::op::host_ref::<OffscreenCanvas>(scope, object) })
  else {
    return yuzuko_core::WriteResult::Skip;
  };
  if unsafe {
    yuzuko_core::op::host_ref::<crate::surface::Window>(scope, object)
  }
  .is_some()
  {
    yuzuko_core::reject_clone(scope);
    return yuzuko_core::WriteResult::Failed;
  }
  if !yuzuko_core::is_structured_transfer(scope, object)
    || canvas.get().is_detached()
  {
    yuzuko_core::reject_clone(scope);
    return yuzuko_core::WriteResult::Failed;
  }
  let ticket = canvas.get().detach_for_transfer();
  let id = NEXT_TICKET.fetch_add(1, Ordering::Relaxed);
  CANVAS_TICKETS
    .lock()
    .expect("canvas transfer")
    .insert(id, ticket);
  helper.write_uint32(TAG_CANVAS);
  helper.write_uint64(id);
  yuzuko_core::WriteResult::Wrote
}

fn read_canvas(
  scope: &mut v8::PinScope,
  tag: u32,
  helper: &dyn v8::ValueDeserializerHelper,
) -> yuzuko_core::ReadResult {
  if tag != TAG_CANVAS {
    return yuzuko_core::ReadResult::Skip;
  }
  let mut id = 0u64;
  if !helper.read_uint64(&mut id) {
    yuzuko_core::reject_clone(scope);
    return yuzuko_core::ReadResult::Failed;
  }
  let Some(ticket) =
    CANVAS_TICKETS.lock().expect("canvas transfer").remove(&id)
  else {
    yuzuko_core::reject_clone(scope);
    return yuzuko_core::ReadResult::Failed;
  };
  let canvas = OffscreenCanvas::create(ticket.width, ticket.height);
  let canvas_id = canvas.id();
  if let Some(slot) = ticket.slot
    && let Some(hooks) = hooks()
  {
    (hooks.put_slot)(canvas_id, slot);
  }
  if let Some(placeholder) = ticket.placeholder {
    crate::surface::adopt_placeholder(canvas_id, placeholder);
  }
  let _ = OffscreenCanvas::template(scope);
  let wrapper = unsafe { yuzuko_core::op::wrap_instance(scope, canvas) };
  yuzuko_core::ReadResult::Done(v8::Global::new(scope, wrapper))
}
