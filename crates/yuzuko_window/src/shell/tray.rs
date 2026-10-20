//! `Yuzuko.Tray`. One status item per instance.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

use tray_icon::{
  Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent,
};
use v8::cppgc::GarbageCollected;
use yuzuko_core::op::{Js, OpError};
use yuzuko_dom::{Event, EventTarget, define_is_trusted, dispatch};
use yuzuko_macros::op;

use crate::shell::menu::{self, MenuOwner};

struct Native {
  icon: TrayIcon,
  menu_set: bool,
}

std::thread_local! {
  static NATIVES: RefCell<HashMap<u64, Native>> =
    RefCell::new(HashMap::new());
  static OBJECTS: RefCell<HashMap<u64, v8::Global<v8::Object>>> =
    RefCell::new(HashMap::new());
}

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

#[repr(C)]
pub struct Tray {
  base: EventTarget,
  id: u64,
  open: Cell<bool>,
}

unsafe impl GarbageCollected for Tray {
  fn trace(&self, visitor: &mut v8::cppgc::Visitor) {
    v8::cppgc::GarbageCollected::trace(&self.base, visitor);
  }

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"Tray"
  }
}

#[op(inherit = EventTarget)]
impl Tray {
  #[constructor]
  fn new() -> Result<Self, OpError> {
    crate::os::ensure_running().map_err(unsupported)?;
    // tray-icon builds a GTK AppIndicator on Linux.
    #[cfg(target_os = "linux")]
    crate::shell::gtk_loop::ready().map_err(unsupported)?;
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let icon = blank_icon().map_err(unsupported)?;
    let native = TrayIconBuilder::new()
      .with_id(id.to_string())
      .with_icon(icon)
      .with_menu_on_left_click(false)
      .build()
      .map_err(|error| unsupported(error.to_string()))?;
    NATIVES.with(|slot| {
      slot.borrow_mut().insert(
        id,
        Native {
          icon: native,
          menu_set: false,
        },
      );
    });
    Ok(Self {
      base: EventTarget::new(),
      id,
      open: Cell::new(true),
    })
  }

  #[construct_hook]
  fn finish(&self, scope: &mut v8::PinScope, object: v8::Local<v8::Object>) {
    OBJECTS.with(|slot| {
      slot
        .borrow_mut()
        .insert(self.id, v8::Global::new(scope, object));
    });
  }

  fn set_icon(
    &self,
    scope: &mut v8::PinScope,
    bytes: v8::Local<v8::Value>,
    options: Option<v8::Local<v8::Value>>,
  ) -> Result<(), OpError> {
    self.require_open()?;
    let template = template_flag(scope, options)?;
    #[cfg(not(target_os = "macos"))]
    if template {
      return Err(unsupported(
        "This system has no template tray icon.".to_string(),
      ));
    }
    let bytes = copy_bytes(bytes)?;
    let (rgba, width, height) = decode_png(&bytes).map_err(OpError::type_)?;
    let icon = Icon::from_rgba(rgba, width, height)
      .map_err(|error| OpError::type_(error.to_string()))?;
    with_native(self.id, |native| {
      #[cfg(target_os = "macos")]
      let result = if template {
        native.icon.set_icon_templated(Some(icon))
      } else {
        native.icon.set_icon(Some(icon))
      };
      #[cfg(not(target_os = "macos"))]
      let result = {
        let _ = template;
        native.icon.set_icon(Some(icon))
      };
      result.map_err(|error| unsupported(error.to_string()))
    })?;
    Ok(())
  }

  fn set_tooltip(
    &self,
    scope: &mut v8::PinScope,
    tooltip: v8::Local<v8::Value>,
  ) -> Result<(), OpError> {
    self.require_open()?;
    #[cfg(target_os = "linux")]
    {
      let _ = (scope, tooltip);
      Err(unsupported("This system has no tray tooltip.".to_string()))
    }
    #[cfg(not(target_os = "linux"))]
    {
      let text = if tooltip.is_null() || tooltip.is_undefined() {
        None
      } else if tooltip.is_string() {
        tooltip
          .to_string(scope)
          .map(|text| text.to_rust_string_lossy(scope))
      } else {
        return Err(OpError::type_("setTooltip requires a string or null."));
      };
      with_native(self.id, |native| {
        native
          .icon
          .set_tooltip(text)
          .map_err(|error| unsupported(error.to_string()))
      })
    }
  }

  fn set_menu(
    &self,
    scope: &mut v8::PinScope,
    value: v8::Local<v8::Value>,
  ) -> Result<(), OpError> {
    self.require_open()?;
    if value.is_null() || value.is_undefined() {
      #[cfg(target_os = "linux")]
      {
        return Err(unsupported(
          "This system cannot remove a tray menu.".to_string(),
        ));
      }
      #[cfg(not(target_os = "linux"))]
      {
        menu::clear_routes(MenuOwner::Tray(self.id));
        with_native(self.id, |native| {
          native.menu_set = false;
          native.icon.set_menu(None);
          Ok(())
        })?;
        return Ok(());
      }
    }
    #[cfg(target_os = "linux")]
    {
      let replacing = NATIVES.with(|slot| {
        slot
          .borrow()
          .get(&self.id)
          .is_some_and(|native| native.menu_set)
      });
      if replacing {
        return Err(unsupported(
          "This system cannot replace a tray menu.".to_string(),
        ));
      }
    }
    let (native, actions) = menu::loose_from_object(scope, value)?;
    with_native(self.id, |slot| {
      slot.icon.set_menu(Some(Box::new(native)));
      slot.menu_set = true;
      Ok(())
    })?;
    menu::set_routes(MenuOwner::Tray(self.id), actions);
    Ok(())
  }

  fn close(&self) {
    if !self.open.get() {
      return;
    }
    self.open.set(false);
    remove_native(self.id);
  }

  #[getter]
  fn bounds(&self, scope: &mut v8::PinScope) -> Js {
    if !self.open.get() {
      return Js::null(scope);
    }
    let rect = NATIVES.with(|slot| {
      slot
        .borrow()
        .get(&self.id)
        .and_then(|native| native.icon.rect())
    });
    let Some(rect) = rect else {
      return Js::null(scope);
    };
    if rect.size.width == 0 && rect.size.height == 0 {
      return Js::null(scope);
    }
    let scale = screen_scale();
    let object = v8::Object::new(scope);
    set_number(scope, object, "x", rect.position.x / scale);
    set_number(scope, object, "y", rect.position.y / scale);
    set_number(scope, object, "width", f64::from(rect.size.width) / scale);
    set_number(scope, object, "height", f64::from(rect.size.height) / scale);
    let value: v8::Local<v8::Value> = object.into();
    Js(v8::Global::new(scope, value))
  }
}

pub fn install(scope: &mut v8::PinScope) {
  Tray::install(scope);
  let yuzuko = menu::yuzuko_object(scope);
  let key = v8::String::new(scope, "Tray").unwrap();
  let global = scope.get_current_context().global(scope);
  let ctor = global.get(scope, key.into()).unwrap();
  global.delete(scope, key.into());
  yuzuko.set(scope, key.into(), ctor);
}

pub fn any_open() -> bool {
  NATIVES.with(|slot| !slot.borrow().is_empty())
}

pub fn clear() {
  NATIVES.with(|slot| {
    let ids = slot.borrow().keys().copied().collect::<Vec<_>>();
    for id in ids {
      menu::clear_routes(MenuOwner::Tray(id));
    }
    slot.borrow_mut().clear();
  });
  OBJECTS.with(|slot| slot.borrow_mut().clear());
}

pub fn poll(scope: &mut v8::PinScope) {
  while let Ok(event) = TrayIconEvent::receiver().try_recv() {
    let TrayIconEvent::Click {
      id,
      button,
      button_state,
      ..
    } = event
    else {
      continue;
    };
    if button != MouseButton::Left || button_state != MouseButtonState::Up {
      continue;
    }
    let Ok(id) = id.0.parse::<u64>() else {
      continue;
    };
    dispatch_click(scope, id);
  }
}

pub fn dispatch_menu(scope: &mut v8::PinScope, id: u64, action: &str) {
  let Some(target) = object(scope, id) else {
    return;
  };
  menu::emit_menuclick(scope, target, action);
}

pub fn dispatch_primary_click(scope: &mut v8::PinScope) -> Result<(), OpError> {
  let id = NATIVES.with(|slot| slot.borrow().keys().next().copied());
  let Some(id) = id else {
    return Err(OpError::type_("There is no open tray."));
  };
  dispatch_click(scope, id);
  Ok(())
}

fn dispatch_click(scope: &mut v8::PinScope, id: u64) {
  let Some(target) = object(scope, id) else {
    return;
  };
  let event = Event::new("click", false, false, false, true);
  let object = unsafe { yuzuko_core::op::wrap_instance(scope, event) };
  define_is_trusted(scope, object);
  let _ = dispatch(scope, target, object);
}

fn object<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  id: u64,
) -> Option<v8::Local<'s, v8::Object>> {
  OBJECTS.with(|slot| {
    slot
      .borrow()
      .get(&id)
      .map(|global| v8::Local::new(scope, global))
  })
}

fn require_open_id(id: u64) -> Result<(), OpError> {
  if NATIVES.with(|slot| slot.borrow().contains_key(&id)) {
    Ok(())
  } else {
    Err(OpError::dom("InvalidStateError", "The tray is closed."))
  }
}

impl Tray {
  fn require_open(&self) -> Result<(), OpError> {
    if self.open.get() {
      require_open_id(self.id)
    } else {
      Err(OpError::dom("InvalidStateError", "The tray is closed."))
    }
  }
}

fn with_native<T>(
  id: u64,
  body: impl FnOnce(&mut Native) -> Result<T, OpError>,
) -> Result<T, OpError> {
  NATIVES.with(|slot| {
    let mut natives = slot.borrow_mut();
    let Some(native) = natives.get_mut(&id) else {
      return Err(OpError::dom("InvalidStateError", "The tray is closed."));
    };
    body(native)
  })
}

fn remove_native(id: u64) {
  menu::clear_routes(MenuOwner::Tray(id));
  NATIVES.with(|slot| {
    slot.borrow_mut().remove(&id);
  });
  OBJECTS.with(|slot| {
    slot.borrow_mut().remove(&id);
  });
}

fn blank_icon() -> Result<Icon, String> {
  Icon::from_rgba(vec![0, 0, 0, 0], 1, 1).map_err(|error| error.to_string())
}

fn unsupported(message: String) -> OpError {
  OpError::dom("NotSupportedError", message)
}

fn template_flag(
  scope: &mut v8::PinScope,
  options: Option<v8::Local<v8::Value>>,
) -> Result<bool, OpError> {
  let Some(options) = options else {
    return Ok(false);
  };
  if options.is_undefined() {
    return Ok(false);
  }
  let Ok(object) = v8::Local::<v8::Object>::try_from(options) else {
    return Err(OpError::type_("Tray icon options must be an object."));
  };
  let key = v8::String::new(scope, "template").unwrap();
  let Some(value) = object.get(scope, key.into()) else {
    return Ok(false);
  };
  if value.is_undefined() {
    return Ok(false);
  }
  if !value.is_boolean() {
    return Err(OpError::type_("template must be a boolean."));
  }
  Ok(value.is_true())
}

fn copy_bytes(value: v8::Local<v8::Value>) -> Result<Vec<u8>, OpError> {
  if let Ok(view) = v8::Local::<v8::ArrayBufferView>::try_from(value) {
    let length = view.byte_length();
    let mut bytes = vec![0; length];
    let copied = view.copy_contents(&mut bytes);
    if copied != length {
      return Err(OpError::type_("The tray icon bytes could not be read."));
    }
    return Ok(bytes);
  }
  if let Ok(buffer) = v8::Local::<v8::ArrayBuffer>::try_from(value) {
    let length = buffer.byte_length();
    let mut bytes = vec![0; length];
    let store = buffer.get_backing_store();
    let Some(data) = store.data() else {
      if length != 0 {
        return Err(OpError::type_("The tray icon bytes could not be read."));
      }
      return Ok(bytes);
    };
    if length != 0 {
      unsafe {
        std::ptr::copy_nonoverlapping(
          data.as_ptr().cast::<u8>(),
          bytes.as_mut_ptr(),
          length,
        );
      }
    }
    return Ok(bytes);
  }
  Err(OpError::type_(
    "setIcon requires a BufferSource of PNG bytes.",
  ))
}

fn decode_png(bytes: &[u8]) -> Result<(Vec<u8>, u32, u32), String> {
  let (rgba, width, height) = crate::png_rgba::decode(bytes)
    .ok_or_else(|| "The tray icon must be a PNG.".to_string())?;
  if width == 0 || height == 0 || width > 1024 || height > 1024 {
    return Err("The tray icon is too large.".to_string());
  }
  Ok((rgba, width, height))
}

fn screen_scale() -> f64 {
  #[cfg(target_os = "macos")]
  {
    if let Some(scale) = crate::shell::dock::main_screen_scale() {
      return scale;
    }
  }
  1.0
}

fn set_number(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
  value: f64,
) {
  let key = v8::String::new(scope, name).unwrap();
  let number = v8::Number::new(scope, value);
  object.set(scope, key.into(), number.into());
}
