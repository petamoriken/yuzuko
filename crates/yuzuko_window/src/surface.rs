//! `Yuzuko.Window` and `Yuzuko.WindowSurface`.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{LazyLock, Mutex};

use yuzuko_core::op::{Js, OpError, host_ref, traced, traced_local};
use yuzuko_core::performance_now_ms;
use yuzuko_dom::{EventTarget, dispatch_focus, dispatch_simple};
use yuzuko_macros::op;

use crate::activation;
use crate::canvas::OffscreenCanvas;
use crate::fit::{self, ImageRendering, ObjectFit, PresentStyle};
use crate::inbox::{self, Raw, Size};
use crate::input::{self, PointerState};
use crate::os::{self, OpenRequest};
use crate::session;

static DEFAULT_TITLE: std::sync::Mutex<String> =
  std::sync::Mutex::new(String::new());

/// Title used when a `Window` constructor omits `title`.
pub fn set_default_title(title: &str) {
  *DEFAULT_TITLE.lock().expect("default title") = title.to_string();
}

pub fn default_title() -> String {
  let title = DEFAULT_TITLE.lock().expect("default title");
  if title.is_empty() {
    "yuzuko".to_string()
  } else {
    title.clone()
  }
}

static NEXT_WINDOW: AtomicU64 = AtomicU64::new(1);

thread_local! {
  static CANVAS_STYLE: RefCell<HashMap<u64, PresentStyle>> =
    RefCell::new(HashMap::new());
  static WINDOW_CANVAS: RefCell<HashMap<u64, u64>> = RefCell::new(HashMap::new());
}

/// How a window surface is presented. An unknown canvas fills the target,
/// which is how a bare present used to stretch.
pub fn canvas_style(canvas_id: u64) -> PresentStyle {
  CANVAS_STYLE.with(|styles| {
    styles
      .borrow()
      .get(&canvas_id)
      .copied()
      .unwrap_or(PresentStyle {
        fit: ObjectFit::Fill,
        rendering: ImageRendering::Auto,
      })
  })
}

fn remember_style(canvas_id: u64, style: PresentStyle) {
  CANVAS_STYLE.with(|styles| {
    styles.borrow_mut().insert(canvas_id, style);
  });
}

fn update_style(canvas_id: u64, edit: impl Fn(&mut PresentStyle)) {
  CANVAS_STYLE.with(|styles| {
    let mut styles = styles.borrow_mut();
    let style = styles.entry(canvas_id).or_insert(PresentStyle {
      fit: ObjectFit::ScaleDown,
      rendering: ImageRendering::Auto,
    });
    edit(style);
  });
}

fn forget_fit(canvas_id: u64) {
  CANVAS_STYLE.with(|styles| {
    styles.borrow_mut().remove(&canvas_id);
  });
}

pub fn canvas_for_window(window_id: u64) -> Option<u64> {
  WINDOW_CANVAS.with(|slots| slots.borrow().get(&window_id).copied())
}

fn bind_window_canvas(window_id: u64, canvas_id: u64) {
  WINDOW_CANVAS.with(|slots| {
    slots.borrow_mut().insert(window_id, canvas_id);
  });
}

fn unbind_window(window_id: u64) {
  WINDOW_CANVAS.with(|slots| {
    slots.borrow_mut().remove(&window_id);
  });
}

#[derive(Clone, Copy)]
pub struct Placeholder {
  pub window_id: u64,
  pub surface_id: u64,
}

static PLACEHOLDERS: LazyLock<Mutex<HashMap<u64, Placeholder>>> =
  LazyLock::new(|| Mutex::new(HashMap::new()));
static COMMITS: LazyLock<Mutex<HashMap<u64, crate::canvas::CanvasSnapshot>>> =
  LazyLock::new(|| Mutex::new(HashMap::new()));
static REBINDS: LazyLock<Mutex<Vec<Placeholder>>> =
  LazyLock::new(|| Mutex::new(Vec::new()));

pub fn link_placeholder(canvas_id: u64, link: Placeholder) {
  PLACEHOLDERS
    .lock()
    .expect("placeholders")
    .insert(canvas_id, link);
}

pub fn take_placeholder(canvas_id: u64) -> Option<Placeholder> {
  PLACEHOLDERS
    .lock()
    .expect("placeholders")
    .remove(&canvas_id)
}

pub fn adopt_placeholder(canvas_id: u64, link: Placeholder) {
  link_placeholder(canvas_id, link);
  if let Some(snapshot) = crate::canvas::snapshot_canvas(canvas_id) {
    COMMITS
      .lock()
      .expect("commits")
      .insert(link.surface_id, snapshot);
  }
  queue_rebind(link);
}

pub fn queue_rebind(link: Placeholder) {
  REBINDS.lock().expect("rebinds").push(link);
  poke_owner();
}

pub fn commit_worker_frames(_scope: &mut v8::PinScope) {
  let links = PLACEHOLDERS
    .lock()
    .expect("placeholders")
    .iter()
    .map(|(id, link)| (*id, *link))
    .collect::<Vec<_>>();
  if links.is_empty() {
    return;
  }
  let mut commits = COMMITS.lock().expect("commits");
  let mut wrote = false;
  for (canvas_id, link) in links {
    if let Some(snapshot) = crate::canvas::snapshot_canvas(canvas_id) {
      commits.insert(link.surface_id, snapshot);
      wrote = true;
    }
  }
  drop(commits);
  if wrote {
    poke_owner();
  }
}

pub fn apply_remote_presentation() {
  let rebinds = std::mem::take(&mut *REBINDS.lock().expect("rebinds"));
  for link in rebinds {
    bind_window_canvas(link.window_id, link.surface_id);
    crate::os::request_redraw(link.window_id);
  }
  let commits = std::mem::take(&mut *COMMITS.lock().expect("commits"));
  for (id, snapshot) in commits {
    crate::canvas::install_presented_bitmap(id, snapshot);
  }
}

fn dialog_unsupported(message: impl Into<String>) -> OpError {
  OpError::dom("NotSupportedError", message)
}

fn poke_owner() {
  let Some(current) = yuzuko_core::try_agent_id() else {
    return;
  };
  let owner = yuzuko_core::parent_agent(current).unwrap_or(current);
  yuzuko_core::poke_agent(owner);
}

struct Frame {
  id: i32,
  callback: v8::TracedReference<v8::Function>,
}

#[repr(C)]
pub struct Window {
  pub base: EventTarget,
  id: u64,
  canvas_id: Cell<u64>,
  surface: RefCell<Option<v8::TracedReference<v8::Object>>>,
  title: RefCell<String>,
  cursor: RefCell<String>,
  resizable: Cell<bool>,
  scale: Cell<f64>,
  css_width: Cell<f64>,
  css_height: Cell<f64>,
  next_frame: Cell<i32>,
  frames: RefCell<Vec<Frame>>,
  queries: RefCell<Vec<v8::TracedReference<v8::Object>>>,
  pointers: PointerState,
  closing: Cell<bool>,
  ime: Cell<bool>,
  locked: Cell<bool>,
  fullscreen: Cell<bool>,
  outer_x: Cell<f64>,
  outer_y: Cell<f64>,
  inner_x: Cell<f64>,
  inner_y: Cell<f64>,
  visible: Cell<bool>,
  always_on_top: Cell<bool>,
  decorations: Cell<bool>,
  active: Cell<bool>,
  maximized: Cell<bool>,
  min_width: Cell<Option<f64>>,
  min_height: Cell<Option<f64>>,
  max_width: Cell<Option<f64>>,
  max_height: Cell<Option<f64>>,
  transparent: Cell<bool>,
  positioned: Cell<bool>,
}

unsafe impl v8::cppgc::GarbageCollected for Window {
  fn trace(&self, visitor: &mut v8::cppgc::Visitor) {
    v8::cppgc::GarbageCollected::trace(&self.base, visitor);
    if let Ok(surface) = self.surface.try_borrow()
      && let Some(surface) = surface.as_ref()
    {
      visitor.trace(surface);
    }
    if let Ok(frames) = self.frames.try_borrow() {
      for frame in frames.iter() {
        visitor.trace(&frame.callback);
      }
    }
    if let Ok(queries) = self.queries.try_borrow() {
      for query in queries.iter() {
        visitor.trace(query);
      }
    }
  }

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"Window"
  }
}

impl Window {
  pub(crate) fn id(&self) -> u64 {
    self.id
  }

  pub(crate) fn pointer_state(&self) -> &PointerState {
    &self.pointers
  }

  pub(crate) fn media_lists(
    &self,
  ) -> &RefCell<Vec<v8::TracedReference<v8::Object>>> {
    &self.queries
  }

  pub(crate) fn close_object(
    scope: &mut v8::PinScope,
    object: v8::Local<v8::Object>,
  ) {
    let id = {
      let Some(host) = (unsafe { host_ref::<Self>(scope, object) }) else {
        return;
      };
      let surface = host.get();
      if surface.closing.get() || !inbox::contains(surface.id) {
        return;
      }
      surface.closing.set(true);
      surface.id
    };
    let proceed = dispatch_simple(scope, object, "close", false, true);
    if !proceed {
      if let Some(host) = unsafe { host_ref::<Self>(scope, object) } {
        host.get().closing.set(false);
      }
      return;
    }
    // `closing` stays set so a visibility listener cannot re-enter teardown.
    Self::publish_visibility(scope, object, false);
    let Some(host) = (unsafe { host_ref::<Self>(scope, object) }) else {
      return;
    };
    let surface = host.get();
    surface.closing.set(false);
    if let Some(size) = inbox::size(id) {
      surface.apply_size(size);
    }
    forget_fit(surface.canvas_id.get());
    unbind_window(surface.id);
    surface.frames.borrow_mut().clear();
    surface.locked.set(false);
    surface.fullscreen.set(false);
    inbox::clear_frame(id);
    inbox::forget(id);
    crate::activation::closed(id);
    os::close(id);
  }

  pub(crate) fn publish_visibility(
    scope: &mut v8::PinScope,
    window: v8::Local<v8::Object>,
    visible: bool,
  ) {
    let Some(surface) = surface_object(scope, window) else {
      return;
    };
    let changed = {
      let Some(host) = (unsafe { host_ref::<WindowSurface>(scope, surface) })
      else {
        return;
      };
      if host.get().visible.get() == visible {
        false
      } else {
        host.get().visible.set(visible);
        true
      }
    };
    if changed {
      dispatch_simple(scope, surface, "visibilitychange", false, false);
    }
  }

  pub(crate) fn run_frame(
    scope: &mut v8::PinScope,
    id: u64,
    now: std::time::Instant,
  ) {
    if !inbox::take_due_frame(id, now) {
      return;
    }
    let Some(object) = inbox::object(scope, id) else {
      return;
    };
    let callbacks: Vec<(i32, v8::Global<v8::Function>)> = {
      let Some(host) = (unsafe { host_ref::<Self>(scope, object) }) else {
        return;
      };
      let frames = std::mem::take(&mut *host.get().frames.borrow_mut());
      frames
        .iter()
        .filter_map(|frame| {
          let function = yuzuko_core::op::traced_local(scope, &frame.callback)?;
          Some((frame.id, v8::Global::new(scope, function)))
        })
        .collect()
    };
    for (_id, callback) in callbacks {
      if !inbox::contains(id) {
        break;
      }
      let Some(object) = inbox::object(scope, id) else {
        break;
      };
      let thrown = {
        v8::tc_scope!(let tc, scope);
        let function = v8::Local::new(tc, &callback);
        let time = v8::Number::new(tc, performance_now_ms());
        let failed = function.call(tc, object.into(), &[time.into()]).is_none();
        if failed {
          tc.exception().map(|value| v8::Global::new(tc, value))
        } else {
          None
        }
      };
      if let Some(thrown) = thrown {
        report_frame_error(scope, thrown);
      }
    }
    scope.perform_microtask_checkpoint();
    if inbox::contains(id) {
      os::present(id);
      crate::canvas::webgpu_after_frame(id);
    }
  }

  pub(crate) fn apply_size(&self, size: Size) {
    self.css_width.set(size.css_width);
    self.css_height.set(size.css_height);
    self.scale.set(size.scale);
  }

  fn live(&self) -> Option<Size> {
    inbox::size(self.id)
  }

  fn require_open(&self) -> Result<(), OpError> {
    if inbox::contains(self.id) {
      Ok(())
    } else {
      Err(OpError::dom("InvalidStateError", "The surface is closed."))
    }
  }

  fn require_dialog(&self) -> Result<(), OpError> {
    self.require_open()?;
    if crate::session::headless() {
      Err(dialog_unsupported("This process has no dialog."))
    } else {
      Ok(())
    }
  }

  fn client_css(&self) -> (f64, f64) {
    self
      .live()
      .map(|size| (size.css_width, size.css_height))
      .unwrap_or_else(|| (self.css_width.get(), self.css_height.get()))
  }

  fn resize_client(
    &self,
    scope: &mut v8::PinScope,
    width: f64,
    height: f64,
  ) -> Result<(), OpError> {
    self.require_open()?;
    if !width.is_finite() || !height.is_finite() {
      return Err(OpError::type_(
        "Window size must be a finite, non-negative number.",
      ));
    }
    let width = limit_edge(width, self.min_width.get(), self.max_width.get());
    let height =
      limit_edge(height, self.min_height.get(), self.max_height.get());
    let width = css_length(width)?;
    let height = css_length(height)?;
    let scale = self.css_scale();
    let size = Size::from_css(width, height, scale);
    inbox::set_size(self.id, size);
    self.apply_size(size);
    os::resize(self.id, width, height);
    if let Some(object) = inbox::object(scope, self.id) {
      dispatch_simple(scope, object, "resize", false, false);
    }
    crate::media::refresh(scope, self.id);
    Ok(())
  }

  fn css_scale(&self) -> f64 {
    self
      .live()
      .map(|size| size.scale)
      .unwrap_or_else(|| self.scale.get())
  }

  fn assign_title(&self, title: String) -> Result<(), OpError> {
    self.require_open()?;
    os::set_title(self.id, &title);
    *self.title.borrow_mut() = title;
    Ok(())
  }

  fn client_origin(&self) -> (f64, f64) {
    if !inbox::contains(self.id) {
      return (0.0, 0.0);
    }
    self.refresh_origin();
    (self.inner_x.get(), self.inner_y.get())
  }

  fn frame_origin(&self) -> (f64, f64) {
    if !inbox::contains(self.id) {
      return (0.0, 0.0);
    }
    self.refresh_origin();
    (self.outer_x.get(), self.outer_y.get())
  }

  fn refresh_origin(&self) {
    let Some(origin) = os::frame_origin(self.id) else {
      return;
    };
    self.outer_x.set(origin.outer_x);
    self.outer_y.set(origin.outer_y);
    self.inner_x.set(origin.inner_x);
    self.inner_y.set(origin.inner_y);
  }

  fn place_outer(&self, x: f64, y: f64) {
    let inset_x = self.inner_x.get() - self.outer_x.get();
    let inset_y = self.inner_y.get() - self.outer_y.get();
    self.outer_x.set(x);
    self.outer_y.set(y);
    self.inner_x.set(x + inset_x);
    self.inner_y.set(y + inset_y);
    if os::move_outer(self.id, x, y) {
      self.refresh_origin();
    }
  }

  fn focus_headless(&self, scope: &mut v8::PinScope) {
    if activation::focused(self.id) {
      return;
    }
    if let Some(previous) = activation::focused_id()
      && previous != self.id
      && let Some(object) = inbox::object(scope, previous)
    {
      activation::observe(previous, &Raw::Blur);
      dispatch_focus(scope, object, "blur");
    }
    activation::observe(self.id, &Raw::Focus);
    if let Some(object) = inbox::object(scope, self.id) {
      dispatch_focus(scope, object, "focus");
    }
  }
}

fn report_frame_error(scope: &mut v8::PinScope, thrown: v8::Global<v8::Value>) {
  let value = v8::Local::new(scope, &thrown);
  let text = exception_text(scope, value);
  if yuzuko_core::note_test_error(text) {
    return;
  }
  let global = scope.get_current_context().global(scope);
  let key = v8::String::new(scope, "reportError").unwrap();
  let Some(report) = global
    .get(scope, key.into())
    .and_then(|value| v8::Local::<v8::Function>::try_from(value).ok())
  else {
    return;
  };
  let _ = report.call(scope, global.into(), &[value]);
}

fn exception_text(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> String {
  if let Ok(object) = v8::Local::<v8::Object>::try_from(value)
    && let Some(key) = v8::String::new(scope, "stack")
    && let Some(stack) = object.get(scope, key.into())
    && stack.is_string()
  {
    let text = stack.to_rust_string_lossy(scope);
    if !text.is_empty() {
      return text;
    }
  }
  value
    .to_string(scope)
    .map(|text| text.to_rust_string_lossy(scope))
    .unwrap_or_else(|| "animation frame callback threw".to_string())
}

fn css_length(number: f64) -> Result<f64, OpError> {
  if !number.is_finite() || number < 0.0 {
    return Err(OpError::type_(
      "Window size must be a finite, non-negative number.",
    ));
  }
  Ok(number)
}

fn object_value<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  object: v8::Local<'_, v8::Object>,
  name: &str,
) -> Option<v8::Local<'s, v8::Value>> {
  let object = yuzuko_core::op::root(scope, object);
  let key = v8::String::new(scope, name)?;
  let value = object.get(scope, key.into())?;
  if value.is_undefined() {
    None
  } else {
    Some(value)
  }
}

struct WindowOptions {
  width: f64,
  height: f64,
  title: String,
  resizable: bool,
  x: Option<f64>,
  y: Option<f64>,
  always_on_top: bool,
  decorations: bool,
  active: bool,
  visible: bool,
  maximized: bool,
  min_width: Option<f64>,
  min_height: Option<f64>,
  max_width: Option<f64>,
  max_height: Option<f64>,
  transparent: bool,
}

fn parse_options(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
) -> Result<WindowOptions, OpError> {
  let (default_width, default_height) = session::default_size();
  if args.length() < 1 || args.get(0).is_null_or_undefined() {
    return Ok(WindowOptions {
      width: default_width,
      height: default_height,
      title: default_title(),
      resizable: false,
      x: None,
      y: None,
      always_on_top: false,
      decorations: true,
      active: true,
      visible: true,
      maximized: false,
      min_width: None,
      min_height: None,
      max_width: None,
      max_height: None,
      transparent: false,
    });
  }
  let Ok(object) = v8::Local::<v8::Object>::try_from(args.get(0)) else {
    return Err(OpError::type_("Window options must be an object"));
  };
  let width = match object_value(scope, object, "width") {
    Some(value) => {
      css_length(<f64 as yuzuko_core::op::FromV8>::from_v8(scope, value)?)?
    }
    None => default_width,
  };
  let height = match object_value(scope, object, "height") {
    Some(value) => {
      css_length(<f64 as yuzuko_core::op::FromV8>::from_v8(scope, value)?)?
    }
    None => default_height,
  };
  let title = match object_value(scope, object, "title") {
    Some(value) => <String as yuzuko_core::op::FromV8>::from_v8(scope, value)?,
    None => default_title(),
  };
  let resizable = option_bool(scope, object, "resizable", false)?;
  let x = option_coord(scope, object, "x")?;
  let y = option_coord(scope, object, "y")?;
  let always_on_top = option_bool(scope, object, "alwaysOnTop", false)?;
  let decorations = option_bool(scope, object, "decorations", true)?;
  let active = option_bool(scope, object, "active", true)?;
  let visible = option_bool(scope, object, "visible", true)?;
  let maximized = option_bool(scope, object, "maximized", false)?;
  let min_width = option_length(scope, object, "minWidth")?;
  let min_height = option_length(scope, object, "minHeight")?;
  let max_width = option_length(scope, object, "maxWidth")?;
  let max_height = option_length(scope, object, "maxHeight")?;
  check_span(min_width, max_width, "width")?;
  check_span(min_height, max_height, "height")?;
  let transparent = option_bool(scope, object, "transparent", false)?;
  Ok(WindowOptions {
    width,
    height,
    title,
    resizable,
    x,
    y,
    always_on_top,
    decorations,
    active,
    visible,
    maximized,
    min_width,
    min_height,
    max_width,
    max_height,
    transparent,
  })
}

fn option_length(
  scope: &mut v8::PinScope,
  object: v8::Local<'_, v8::Object>,
  name: &str,
) -> Result<Option<f64>, OpError> {
  match object_value(scope, object, name) {
    Some(value) => {
      let number = <f64 as yuzuko_core::op::FromV8>::from_v8(scope, value)?;
      Ok(Some(css_length(number)?))
    }
    None => Ok(None),
  }
}

fn check_span(
  min: Option<f64>,
  max: Option<f64>,
  axis: &str,
) -> Result<(), OpError> {
  if let (Some(min), Some(max)) = (min, max)
    && min > max
  {
    return Err(OpError::type_(format!(
      "Window {axis} minimum is above its maximum.",
    )));
  }
  Ok(())
}

fn optional_number(scope: &mut v8::PinScope, value: Option<f64>) -> Js {
  let local: v8::Local<v8::Value> = match value {
    Some(number) => v8::Number::new(scope, number).into(),
    None => v8::undefined(scope).into(),
  };
  Js(v8::Global::new(scope, local))
}

fn limit_edge(value: f64, min: Option<f64>, max: Option<f64>) -> f64 {
  let mut value = value;
  if let Some(min) = min {
    value = value.max(min);
  }
  if let Some(max) = max {
    value = value.min(max);
  }
  value
}

fn option_bool(
  scope: &mut v8::PinScope,
  object: v8::Local<'_, v8::Object>,
  name: &str,
  default: bool,
) -> Result<bool, OpError> {
  match object_value(scope, object, name) {
    Some(value) => <bool as yuzuko_core::op::FromV8>::from_v8(scope, value),
    None => Ok(default),
  }
}

fn option_coord(
  scope: &mut v8::PinScope,
  object: v8::Local<'_, v8::Object>,
  name: &str,
) -> Result<Option<f64>, OpError> {
  match object_value(scope, object, name) {
    Some(value) => {
      let number = <f64 as yuzuko_core::op::FromV8>::from_v8(scope, value)?;
      Ok(Some(finite_coord(number)?))
    }
    None => Ok(None),
  }
}

fn finite_coord(number: f64) -> Result<f64, OpError> {
  if !number.is_finite() {
    return Err(OpError::type_("Window position must be a finite number."));
  }
  Ok(number)
}

fn promise(
  scope: &mut v8::PinScope,
  result: Result<(), (String, String)>,
) -> Js {
  let Some(resolver) = v8::PromiseResolver::new(scope) else {
    return Js::null(scope);
  };
  let promise = resolver.get_promise(scope);
  match result {
    Ok(()) => {
      let undefined: v8::Local<v8::Value> = v8::undefined(scope).into();
      let _ = resolver.resolve(scope, undefined);
    }
    Err((name, message)) => {
      if let Some(error) =
        yuzuko_core::op::new_dom_exception(scope, &name, &message)
      {
        let error: v8::Local<v8::Value> = error.into();
        let _ = resolver.reject(scope, error);
      }
    }
  }
  let value: v8::Local<v8::Value> = promise.into();
  Js(v8::Global::new(scope, value))
}

fn closed_promise(scope: &mut v8::PinScope) -> Js {
  promise(
    scope,
    Err((
      "InvalidStateError".to_string(),
      "The surface is closed.".to_string(),
    )),
  )
}

#[op(inherit = EventTarget)]
impl Window {
  #[constructor]
  fn new(
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<Window, OpError> {
    let options = parse_options(scope, args)?;
    let width = limit_edge(options.width, options.min_width, options.max_width);
    let height =
      limit_edge(options.height, options.min_height, options.max_height);
    let size = Size::from_css(width, height, 1.0);
    let id = NEXT_WINDOW.fetch_add(1, Ordering::Relaxed);
    inbox::prepare(id, size);
    let positioned = options.x.is_some() || options.y.is_some();
    let origin_x = options.x.unwrap_or(0.0);
    let origin_y = options.y.unwrap_or(0.0);
    Ok(Window {
      base: EventTarget::new(),
      id,
      canvas_id: Cell::new(0),
      surface: RefCell::new(None),
      title: RefCell::new(options.title),
      cursor: RefCell::new("default".to_string()),
      resizable: Cell::new(options.resizable),
      scale: Cell::new(1.0),
      css_width: Cell::new(width),
      css_height: Cell::new(height),
      next_frame: Cell::new(0),
      frames: RefCell::new(Vec::new()),
      queries: RefCell::new(Vec::new()),
      pointers: PointerState::new(),
      closing: Cell::new(false),
      ime: Cell::new(false),
      locked: Cell::new(false),
      fullscreen: Cell::new(false),
      outer_x: Cell::new(origin_x),
      outer_y: Cell::new(origin_y),
      inner_x: Cell::new(origin_x),
      inner_y: Cell::new(origin_y),
      visible: Cell::new(options.visible),
      always_on_top: Cell::new(options.always_on_top),
      decorations: Cell::new(options.decorations),
      active: Cell::new(options.active),
      maximized: Cell::new(options.maximized),
      min_width: Cell::new(options.min_width),
      min_height: Cell::new(options.min_height),
      max_width: Cell::new(options.max_width),
      max_height: Cell::new(options.max_height),
      transparent: Cell::new(options.transparent),
      positioned: Cell::new(positioned),
    })
  }

  #[construct_hook]
  fn finish(
    &self,
    scope: &mut v8::PinScope,
    object: v8::Local<v8::Object>,
  ) -> Result<(), OpError> {
    let css = Size::from_css(self.css_width.get(), self.css_height.get(), 1.0);
    let canvas =
      WindowSurface::open(css.bitmap_width, css.bitmap_height, self.id);
    self.canvas_id.set(canvas.id());
    let wrapped =
      unsafe { yuzuko_core::op::wrap_instance::<WindowSurface>(scope, canvas) };
    inbox::bind(scope, self.id, object);
    let title = self.title.borrow().clone();
    let position = self
      .positioned
      .get()
      .then_some((self.outer_x.get(), self.outer_y.get()));
    if let Err(message) = os::open(
      self.id,
      OpenRequest {
        title,
        resizable: self.resizable.get(),
        position,
        always_on_top: self.always_on_top.get(),
        decorations: self.decorations.get(),
        active: self.active.get(),
        visible: self.visible.get(),
        maximized: self.maximized.get(),
        min_width: self.min_width.get(),
        min_height: self.min_height.get(),
        max_width: self.max_width.get(),
        max_height: self.max_height.get(),
        transparent: self.transparent.get(),
      },
    ) {
      inbox::forget(self.id);
      forget_fit(self.canvas_id.get());
      return Err(OpError::type_(message));
    }
    self.refresh_origin();
    // The surface is device pixels at creation. A later resize does not
    // change it. On a 2x display this is twice the CSS size on each edge.
    if let Some(size) = os::current_size(self.id) {
      let _ = inbox::set_size(self.id, size);
    }
    if let Some(size) = inbox::size(self.id) {
      self.apply_size(size);
      if let Some(host) = unsafe { host_ref::<WindowSurface>(scope, wrapped) } {
        host
          .get()
          .resize_bitmap(size.bitmap_width, size.bitmap_height);
      }
    }
    *self.surface.borrow_mut() = Some(traced(scope, wrapped));
    Ok(())
  }

  #[getter]
  fn surface(&self, scope: &mut v8::PinScope) -> Js {
    let stored = self.surface.borrow();
    let Some(reference) = stored.as_ref() else {
      return Js::null(scope);
    };
    let Some(value) = traced_local(scope, reference) else {
      return Js::null(scope);
    };
    let value: v8::Local<v8::Value> = value.into();
    Js(v8::Global::new(scope, value))
  }

  #[getter]
  fn resizable(&self) -> bool {
    self.resizable.get()
  }

  #[setter]
  fn resizable(&self, enabled: bool) -> Result<(), OpError> {
    self.require_open()?;
    self.resizable.set(enabled);
    os::set_resizable(self.id, enabled);
    Ok(())
  }

  #[getter]
  fn client_width(&self) -> f64 {
    self
      .live()
      .map(|size| size.css_width)
      .unwrap_or_else(|| self.css_width.get())
  }

  #[getter]
  fn client_height(&self) -> f64 {
    self
      .live()
      .map(|size| size.css_height)
      .unwrap_or_else(|| self.css_height.get())
  }

  #[getter]
  fn device_pixel_ratio(&self) -> f64 {
    self.css_scale()
  }

  #[getter]
  fn screen_x(&self) -> f64 {
    self.client_origin().0
  }

  #[getter]
  fn screen_y(&self) -> f64 {
    self.client_origin().1
  }

  #[getter]
  fn screen_left(&self) -> f64 {
    self.client_origin().0
  }

  #[getter]
  fn screen_top(&self) -> f64 {
    self.client_origin().1
  }

  #[getter]
  fn inner_x(&self) -> f64 {
    self.client_origin().0
  }

  #[getter]
  fn inner_y(&self) -> f64 {
    self.client_origin().1
  }

  #[getter]
  fn outer_x(&self) -> f64 {
    self.frame_origin().0
  }

  #[getter]
  fn outer_y(&self) -> f64 {
    self.frame_origin().1
  }

  #[getter]
  fn visible(&self) -> bool {
    inbox::contains(self.id) && self.visible.get()
  }

  #[getter]
  fn always_on_top(&self) -> bool {
    self.always_on_top.get()
  }

  #[setter]
  fn always_on_top(&self, enabled: bool) -> Result<(), OpError> {
    self.require_open()?;
    self.always_on_top.set(enabled);
    os::set_always_on_top(self.id, enabled);
    Ok(())
  }

  #[getter]
  fn decorations(&self) -> bool {
    self.decorations.get()
  }

  #[getter]
  fn active(&self) -> bool {
    self.active.get()
  }

  #[getter]
  fn transparent(&self) -> bool {
    self.transparent.get()
  }

  #[getter]
  fn maximized(&self) -> bool {
    os::is_maximized(self.id).unwrap_or(self.maximized.get())
  }

  #[getter]
  fn min_width(&self, scope: &mut v8::PinScope) -> Js {
    optional_number(scope, self.min_width.get())
  }

  #[getter]
  fn min_height(&self, scope: &mut v8::PinScope) -> Js {
    optional_number(scope, self.min_height.get())
  }

  #[getter]
  fn max_width(&self, scope: &mut v8::PinScope) -> Js {
    optional_number(scope, self.max_width.get())
  }

  #[getter]
  fn max_height(&self, scope: &mut v8::PinScope) -> Js {
    optional_number(scope, self.max_height.get())
  }

  fn move_to(&self, x: f64, y: f64) -> Result<(), OpError> {
    self.require_open()?;
    self.place_outer(finite_coord(x)?, finite_coord(y)?);
    Ok(())
  }

  fn move_by(&self, x: f64, y: f64) -> Result<(), OpError> {
    self.require_open()?;
    let x = finite_coord(x)?;
    let y = finite_coord(y)?;
    self.refresh_origin();
    let next_x = finite_coord(self.outer_x.get() + x)?;
    let next_y = finite_coord(self.outer_y.get() + y)?;
    self.place_outer(next_x, next_y);
    Ok(())
  }

  fn resize_by(
    &self,
    scope: &mut v8::PinScope,
    x: f64,
    y: f64,
  ) -> Result<(), OpError> {
    let (width, height) = self.client_css();
    self.resize_client(
      scope,
      width + finite_coord(x)?,
      height + finite_coord(y)?,
    )
  }

  fn show(&self) -> Result<(), OpError> {
    self.require_open()?;
    self.visible.set(true);
    os::set_visible(self.id, true);
    Ok(())
  }

  fn hide(&self) -> Result<(), OpError> {
    self.require_open()?;
    self.visible.set(false);
    os::set_visible(self.id, false);
    Ok(())
  }

  fn focus(&self, scope: &mut v8::PinScope) -> Result<(), OpError> {
    self.require_open()?;
    if session::headless() {
      self.focus_headless(scope);
      return Ok(());
    }
    os::focus(self.id);
    for id in inbox::ids() {
      if inbox::has_input(id) {
        input::flush(scope, id);
      }
    }
    Ok(())
  }

  #[getter]
  fn cursor(&self) -> String {
    self.cursor.borrow().clone()
  }

  #[setter]
  fn cursor(&self, cursor: String) -> Result<(), OpError> {
    self.require_open()?;
    // An unrecognized name is ignored, as a CSS `cursor` keyword would be.
    let Some(cursor) = os::canonical_cursor(&cursor) else {
      return Ok(());
    };
    os::set_cursor(self.id, &cursor);
    *self.cursor.borrow_mut() = cursor;
    Ok(())
  }

  fn request_animation_frame(
    &self,
    scope: &mut v8::PinScope,
    callback: v8::Local<v8::Function>,
  ) -> Result<i32, OpError> {
    // `next_frame` starts at 0 and is incremented before it is stored, so 0
    // is never a live handle. Returning it must not arm another redraw.
    if !inbox::contains(self.id) {
      return Ok(0);
    }
    let id = self.next_frame.get().saturating_add(1);
    self.next_frame.set(id);
    self.frames.borrow_mut().push(Frame {
      id,
      callback: traced(scope, callback),
    });
    inbox::arm_frame(self.id);
    Ok(id)
  }

  fn cancel_animation_frame(&self, id: i32) {
    self.frames.borrow_mut().retain(|frame| frame.id != id);
    if self.frames.borrow().is_empty() {
      inbox::clear_frame(self.id);
    }
  }

  fn resize_to(
    &self,
    scope: &mut v8::PinScope,
    width: f64,
    height: f64,
  ) -> Result<(), OpError> {
    self.resize_client(scope, width, height)
  }

  fn match_media(
    &self,
    scope: &mut v8::PinScope,
    query: String,
  ) -> Result<Js, OpError> {
    self.require_open()?;
    let object = crate::media::open(scope, self.id, query);
    self.queries.borrow_mut().push(traced(scope, object));
    let value: v8::Local<v8::Value> = object.into();
    Ok(Js(v8::Global::new(scope, value)))
  }

  fn alert(
    &self,
    scope: &mut v8::PinScope,
    message: v8::Local<v8::Value>,
  ) -> Result<(), OpError> {
    let message = crate::shell::dialog::js_string(scope, message)?;
    self.require_dialog()?;
    let title = self.title.borrow().clone();
    crate::shell::dialog::alert(self.id, &title, &message)
      .map_err(dialog_unsupported)
  }

  fn confirm(
    &self,
    scope: &mut v8::PinScope,
    message: v8::Local<v8::Value>,
  ) -> Result<bool, OpError> {
    let message = crate::shell::dialog::js_string(scope, message)?;
    self.require_dialog()?;
    let title = self.title.borrow().clone();
    crate::shell::dialog::confirm(self.id, &title, &message)
      .map_err(dialog_unsupported)
  }

  fn prompt(
    &self,
    scope: &mut v8::PinScope,
    message: v8::Local<v8::Value>,
    default_value: Option<v8::Local<v8::Value>>,
  ) -> Result<Js, OpError> {
    let message = crate::shell::dialog::js_string(scope, message)?;
    let default_value = match default_value {
      Some(value) if !value.is_undefined() => {
        crate::shell::dialog::js_string(scope, value)?
      }
      _ => String::new(),
    };
    self.require_dialog()?;
    let title = self.title.borrow().clone();
    match crate::shell::dialog::prompt(
      self.id,
      &title,
      &message,
      &default_value,
    )
    .map_err(dialog_unsupported)?
    {
      Some(text) => {
        let text = v8::String::new(scope, &text).unwrap();
        let value: v8::Local<v8::Value> = text.into();
        Ok(Js(v8::Global::new(scope, value)))
      }
      None => Ok(Js::null(scope)),
    }
  }

  fn close(&self, scope: &mut v8::PinScope) {
    let Some(object) = inbox::object(scope, self.id) else {
      return;
    };
    Self::close_object(scope, object);
  }

  fn set_ime_allowed(
    &self,
    scope: &mut v8::PinScope,
    allowed: bool,
  ) -> Result<(), OpError> {
    self.require_open()?;
    if !allowed
      && let Some(data) = self.pointers.end_composition()
      && let Some(object) = inbox::object(scope, self.id)
    {
      yuzuko_dom::dispatch_composition(
        scope,
        object,
        &yuzuko_dom::FiredComposition {
          type_name: "compositionend".to_string(),
          data,
        },
      );
    }
    self.ime.set(allowed);
    inbox::set_ime(self.id, allowed);
    os::set_ime(self.id, allowed);
    Ok(())
  }

  fn set_composition_caret(
    &self,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
  ) -> Result<(), OpError> {
    self.require_open()?;
    os::set_caret(self.id, x, y, width, height);
    Ok(())
  }

  fn request_pointer_lock(&self, scope: &mut v8::PinScope) -> Js {
    let Some(object) = inbox::object(scope, self.id) else {
      return closed_promise(scope);
    };
    if self.locked.get() {
      return promise(scope, Ok(()));
    }
    match os::lock_pointer(self.id) {
      Ok(()) => {
        self.locked.set(true);
        dispatch_simple(scope, object, "pointerlockchange", false, false);
        promise(scope, Ok(()))
      }
      Err(message) => {
        dispatch_simple(scope, object, "pointerlockerror", false, false);
        promise(scope, Err(("InvalidStateError".to_string(), message)))
      }
    }
  }

  fn exit_pointer_lock(&self, scope: &mut v8::PinScope) -> Js {
    let Some(object) = inbox::object(scope, self.id) else {
      return closed_promise(scope);
    };
    if self.locked.get() {
      os::unlock_pointer(self.id);
      self.locked.set(false);
      dispatch_simple(scope, object, "pointerlockchange", false, false);
    }
    promise(scope, Ok(()))
  }

  fn request_fullscreen(&self, scope: &mut v8::PinScope) -> Js {
    if inbox::object(scope, self.id).is_none() {
      return closed_promise(scope);
    }
    if self.fullscreen.get() {
      return promise(scope, Ok(()));
    }
    match os::set_fullscreen(self.id, true) {
      Ok(()) => {
        self.fullscreen.set(true);
        promise(scope, Ok(()))
      }
      Err(message) => {
        promise(scope, Err(("InvalidStateError".to_string(), message)))
      }
    }
  }

  fn exit_fullscreen(&self, scope: &mut v8::PinScope) -> Js {
    if inbox::object(scope, self.id).is_none() {
      return closed_promise(scope);
    }
    if self.fullscreen.get() {
      let _ = os::set_fullscreen(self.id, false);
      self.fullscreen.set(false);
    }
    promise(scope, Ok(()))
  }

  fn set_pointer_capture(
    &self,
    scope: &mut v8::PinScope,
    pointer_id: i32,
  ) -> Result<(), OpError> {
    self.require_open()?;
    match self.pointers.capture(pointer_id) {
      Ok(false) => Ok(()),
      Ok(true) => {
        if let Some(object) = inbox::object(scope, self.id) {
          dispatch_pointer_on_window(
            scope,
            object,
            &input::got_capture_event(pointer_id),
          );
        }
        Ok(())
      }
      Err(()) => {
        Err(OpError::dom("NotFoundError", "The pointer is not active."))
      }
    }
  }

  fn release_pointer_capture(
    &self,
    scope: &mut v8::PinScope,
    pointer_id: i32,
  ) -> Result<(), OpError> {
    self.require_open()?;
    self.pointers.release_capture(pointer_id).map_err(|_| {
      OpError::dom("NotFoundError", "The pointer is not captured.")
    })?;
    if let Some(object) = inbox::object(scope, self.id) {
      let mut event = input::got_capture_event(pointer_id);
      event.type_name = "lostpointercapture".to_string();
      dispatch_pointer_on_window(scope, object, &event);
    }
    Ok(())
  }

  fn has_pointer_capture(&self, pointer_id: i32) -> bool {
    self.pointers.has_capture(pointer_id)
  }
}

pub(crate) fn dispatch_pointer_on_window(
  scope: &mut v8::PinScope,
  window: v8::Local<v8::Object>,
  fired: &yuzuko_dom::FiredPointer,
) {
  if let Some(surface) = surface_object(scope, window) {
    yuzuko_dom::dispatch_pointer_through(scope, surface, &[window], fired);
  } else {
    yuzuko_dom::dispatch_pointer(scope, window, fired);
  }
}

fn surface_object<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  window: v8::Local<'_, v8::Object>,
) -> Option<v8::Local<'s, v8::Object>> {
  let host = unsafe { host_ref::<Window>(scope, window) }?;
  let stored = host.get().surface.borrow();
  let reference = stored.as_ref()?;
  traced_local(scope, reference)
}

#[repr(C)]
pub struct WindowSurface {
  pub base: EventTarget,
  canvas: OffscreenCanvas,
  fit: Cell<ObjectFit>,
  rendering: Cell<ImageRendering>,
  window_id: u64,
  visible: Cell<bool>,
  transferred: Cell<bool>,
}

unsafe impl v8::cppgc::GarbageCollected for WindowSurface {
  fn trace(&self, visitor: &mut v8::cppgc::Visitor) {
    v8::cppgc::GarbageCollected::trace(&self.base, visitor);
    v8::cppgc::GarbageCollected::trace(&self.canvas, visitor);
  }

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"WindowSurface"
  }
}

impl WindowSurface {
  fn open(width: f64, height: f64, window_id: u64) -> Self {
    let canvas = OffscreenCanvas::create(width, height);
    remember_style(
      canvas.id(),
      PresentStyle {
        fit: ObjectFit::ScaleDown,
        rendering: ImageRendering::Auto,
      },
    );
    bind_window_canvas(window_id, canvas.id());
    Self {
      base: EventTarget::new(),
      canvas,
      fit: Cell::new(ObjectFit::ScaleDown),
      rendering: Cell::new(ImageRendering::Auto),
      window_id,
      visible: Cell::new(true),
      transferred: Cell::new(false),
    }
  }

  fn id(&self) -> u64 {
    self.canvas.id()
  }

  /// The canvas this surface draws into.
  pub(crate) fn drawing_canvas(&self) -> &OffscreenCanvas {
    &self.canvas
  }

  pub(crate) fn resize_bitmap(&self, width: f64, height: f64) {
    if self.transferred.get() {
      return;
    }
    self.canvas.set_bitmap(width, height);
  }

  fn restyle(&self, edit: impl Fn(&mut PresentStyle)) {
    update_style(self.id(), &edit);
    if let Some(presented) = canvas_for_window(self.window_id) {
      update_style(presented, &edit);
    }
  }

  fn require_direct(&self) -> Result<(), OpError> {
    if self.transferred.get() {
      Err(OpError::dom(
        "InvalidStateError",
        "The surface control has been transferred",
      ))
    } else {
      Ok(())
    }
  }
}

#[op(name = "WindowSurface", inherit = EventTarget)]
impl WindowSurface {
  #[constructor]
  fn new() -> Result<Self, OpError> {
    Err(OpError::type_("Illegal constructor"))
  }

  #[getter]
  fn width(&self) -> f64 {
    self.canvas.bitmap().0
  }

  #[setter]
  fn width(&self, value: f64) -> Result<(), OpError> {
    self.require_direct()?;
    self.canvas.set_width(value)
  }

  #[getter]
  fn height(&self) -> f64 {
    self.canvas.bitmap().1
  }

  #[setter]
  fn height(&self, value: f64) -> Result<(), OpError> {
    self.require_direct()?;
    self.canvas.set_height(value)
  }

  #[getter]
  fn title(&self, scope: &mut v8::PinScope) -> String {
    let Some(object) = inbox::object(scope, self.window_id) else {
      return String::new();
    };
    let Some(host) = (unsafe { host_ref::<Window>(scope, object) }) else {
      return String::new();
    };
    host.get().title.borrow().clone()
  }

  #[setter]
  fn title(
    &self,
    scope: &mut v8::PinScope,
    title: String,
  ) -> Result<(), OpError> {
    let Some(object) = inbox::object(scope, self.window_id) else {
      return Err(OpError::dom("InvalidStateError", "The surface is closed."));
    };
    let Some(host) = (unsafe { host_ref::<Window>(scope, object) }) else {
      return Err(OpError::dom("InvalidStateError", "The surface is closed."));
    };
    host.get().assign_title(title)
  }

  fn get_context(
    &self,
    scope: &mut v8::PinScope,
    #[this] this: v8::Local<v8::Object>,
    type_: String,
    options: Option<v8::Local<v8::Value>>,
  ) -> Result<Js, OpError> {
    self.require_direct()?;
    self.canvas.open_context(scope, this, type_, options)
  }

  fn transfer_to_image_bitmap(
    &self,
    scope: &mut v8::PinScope,
  ) -> Result<Js, OpError> {
    self.require_direct()?;
    self.canvas.to_image_bitmap(scope)
  }

  fn convert_to_blob(
    &self,
    scope: &mut v8::PinScope,
    options: Option<v8::Local<v8::Value>>,
  ) -> Result<Js, OpError> {
    self.require_direct()?;
    self.canvas.to_blob(scope, options)
  }

  fn transfer_control_to_offscreen(
    &self,
    scope: &mut v8::PinScope,
  ) -> Result<Js, OpError> {
    self.require_direct()?;
    if self.canvas.context_kind() != crate::canvas::ContextKind::None {
      return Err(OpError::dom(
        "InvalidStateError",
        "The surface already has a rendering context",
      ));
    }
    let (width, height) = self.canvas.bitmap();
    let offscreen = OffscreenCanvas::create(width, height);
    let offscreen_id = offscreen.id();
    self.transferred.set(true);
    link_placeholder(
      offscreen_id,
      Placeholder {
        window_id: self.window_id,
        surface_id: self.id(),
      },
    );
    remember_style(offscreen_id, canvas_style(self.id()));
    bind_window_canvas(self.window_id, offscreen_id);
    let _ = OffscreenCanvas::template(scope);
    let wrapper = unsafe { yuzuko_core::op::wrap_instance(scope, offscreen) };
    let value: v8::Local<v8::Value> = wrapper.into();
    Ok(Js(v8::Global::new(scope, value)))
  }

  #[getter]
  fn object_fit(&self) -> String {
    fit::name(self.fit.get()).to_string()
  }

  #[setter]
  fn object_fit(&self, value: String) -> Result<(), OpError> {
    let Some(next) = fit::parse(&value) else {
      return Ok(());
    };
    self.fit.set(next);
    self.restyle(|style| style.fit = next);
    Ok(())
  }

  #[getter]
  fn image_rendering(&self) -> String {
    fit::rendering_name(self.rendering.get()).to_string()
  }

  #[setter]
  fn image_rendering(&self, value: String) -> Result<(), OpError> {
    let Some(next) = fit::parse_rendering(&value) else {
      return Ok(());
    };
    self.rendering.set(next);
    self.restyle(|style| style.rendering = next);
    Ok(())
  }

  #[getter]
  fn visibility_state(&self) -> String {
    if self.visible.get() && inbox::contains(self.window_id) {
      "visible".to_string()
    } else {
      "hidden".to_string()
    }
  }
}

fn publish(
  scope: &mut v8::PinScope,
  install: fn(&mut v8::PinScope),
  name: &str,
) {
  install(scope);
  let global = scope.get_current_context().global(scope);
  let Some(key) = v8::String::new(scope, name) else {
    return;
  };
  let Some(constructor) = global.get(scope, key.into()) else {
    return;
  };
  let _ = global.delete(scope, key.into());
  let yuzuko = yuzuko_core::namespace(scope, "Yuzuko");
  yuzuko.set(scope, key.into(), constructor);
}

pub fn install(scope: &mut v8::PinScope) {
  // `install` defines accessors and also publishes the constructor on
  // `globalThis`. Neither class is a global `Window`.
  crate::media::install(scope);
  publish(scope, WindowSurface::install, "WindowSurface");
  publish(scope, Window::install, "Window");
}
