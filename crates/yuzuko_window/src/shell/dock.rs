//! `Yuzuko.dock`. One process-wide dock or taskbar control.

use std::cell::RefCell;

use yuzuko_core::op::{OpError, OpException};
use yuzuko_dom::Event;

use crate::shell::menu::{self, MenuOwner};

#[cfg(target_os = "macos")]
use std::sync::atomic::{AtomicBool, Ordering};

#[cfg(target_os = "macos")]
std::thread_local! {
  static DOCK_MENU: RefCell<Option<muda::Menu>> = const { RefCell::new(None) };
  static REOPEN: RefCell<Option<bool>> = const { RefCell::new(None) };
}

#[cfg(target_os = "macos")]
static HOOKS: AtomicBool = AtomicBool::new(false);

pub fn install(scope: &mut v8::PinScope) {
  let yuzuko = menu::yuzuko_object(scope);
  let object = unsafe {
    yuzuko_core::op::wrap_instance(scope, yuzuko_dom::EventTarget::new())
  };
  set_method(scope, object, "setBadge", 1, set_badge);
  set_method(scope, object, "setVisible", 1, set_visible);
  set_method(scope, object, "setMenu", 1, set_menu);
  set_method(scope, object, "bounce", 0, bounce);
  let key = v8::String::new(scope, "dock").unwrap();
  yuzuko.set(scope, key.into(), object.into());
}

pub fn clear() {
  menu::clear_routes(MenuOwner::Dock);
  #[cfg(target_os = "macos")]
  DOCK_MENU.with(|slot| slot.borrow_mut().take());
}

pub fn poll(scope: &mut v8::PinScope) {
  #[cfg(target_os = "macos")]
  {
    let pending = REOPEN.with(|slot| slot.borrow_mut().take());
    if let Some(has_visible) = pending {
      dispatch_reopen(scope, has_visible);
    }
  }
  #[cfg(not(target_os = "macos"))]
  {
    let _ = scope;
  }
}

pub fn dispatch_menu(scope: &mut v8::PinScope, action: &str) {
  let target = dock_object(scope);
  menu::emit_menuclick(scope, target, action);
}

#[cfg(target_os = "macos")]
pub fn main_screen_scale() -> Option<f64> {
  use objc2::MainThreadMarker;
  use objc2_app_kit::NSScreen;
  let mtm = MainThreadMarker::new()?;
  let screen = NSScreen::mainScreen(mtm)?;
  let scale = screen.backingScaleFactor();
  if scale.is_finite() && scale > 0.0 {
    Some(scale)
  } else {
    None
  }
}

#[cfg(not(target_os = "macos"))]
pub fn main_screen_scale() -> Option<f64> {
  None
}

fn set_badge(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut _ret: v8::ReturnValue,
) {
  if let Err(error) = apply_badge(scope, args.get(0)) {
    OpException::throw(error, scope);
  }
}

fn set_visible(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut _ret: v8::ReturnValue,
) {
  if let Err(error) = apply_visible(args.get(0)) {
    OpException::throw(error, scope);
  }
}

fn set_menu(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut _ret: v8::ReturnValue,
) {
  if let Err(error) = apply_menu(scope, args.get(0)) {
    OpException::throw(error, scope);
  }
}

fn bounce(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut _ret: v8::ReturnValue,
) {
  if let Err(error) = apply_bounce(args.get(0), args.length() > 0) {
    OpException::throw(error, scope);
  }
}

fn apply_badge(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<(), OpError> {
  let text = if value.is_null() || value.is_undefined() {
    String::new()
  } else if value.is_string() {
    value
      .to_string(scope)
      .map(|text| text.to_rust_string_lossy(scope))
      .unwrap_or_default()
  } else {
    return Err(OpError::type_("setBadge requires a string or null."));
  };
  #[cfg(target_os = "macos")]
  {
    use objc2::MainThreadMarker;
    use objc2_app_kit::NSApplication;
    use objc2_foundation::NSString;
    let Some(mtm) = MainThreadMarker::new() else {
      return Err(unsupported("Dock methods run on the main thread."));
    };
    let app = NSApplication::sharedApplication(mtm);
    if text.is_empty() {
      app.dockTile().setBadgeLabel(None);
    } else {
      app
        .dockTile()
        .setBadgeLabel(Some(&NSString::from_str(&text)));
    }
    Ok(())
  }
  #[cfg(not(target_os = "macos"))]
  {
    let _ = text;
    Err(unsupported("This system has no dock badge."))
  }
}

fn apply_visible(value: v8::Local<v8::Value>) -> Result<(), OpError> {
  if !value.is_boolean() {
    return Err(OpError::type_("setVisible requires a boolean."));
  }
  let visible = value.is_true();
  #[cfg(target_os = "macos")]
  {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};
    let Some(mtm) = MainThreadMarker::new() else {
      return Err(unsupported("Dock methods run on the main thread."));
    };
    let app = NSApplication::sharedApplication(mtm);
    let policy = if visible {
      NSApplicationActivationPolicy::Regular
    } else {
      NSApplicationActivationPolicy::Accessory
    };
    if app.setActivationPolicy(policy) {
      Ok(())
    } else {
      Err(unsupported("The system rejected the activation policy."))
    }
  }
  #[cfg(not(target_os = "macos"))]
  {
    let _ = visible;
    Err(unsupported("This system cannot change dock visibility."))
  }
}

fn apply_menu(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<(), OpError> {
  #[cfg(not(target_os = "macos"))]
  {
    let _ = (scope, value);
    return Err(unsupported("This system has no dock menu."));
  }
  #[cfg(target_os = "macos")]
  {
    if value.is_null() || value.is_undefined() {
      menu::clear_routes(MenuOwner::Dock);
      DOCK_MENU.with(|slot| slot.borrow_mut().take());
      install_hooks()?;
      return Ok(());
    }
    let (native, actions) = menu::loose_from_object(scope, value)?;
    // Build the native menu before storing it, so the Dock delegate can see it.
    let _ = native.items();
    DOCK_MENU.with(|slot| *slot.borrow_mut() = Some(native));
    menu::set_routes(MenuOwner::Dock, actions);
    install_hooks()?;
    Ok(())
  }
}

fn apply_bounce(
  value: v8::Local<v8::Value>,
  present: bool,
) -> Result<(), OpError> {
  let critical = if !present || value.is_undefined() {
    false
  } else if value.is_boolean() {
    value.is_true()
  } else {
    return Err(OpError::type_("bounce requires a boolean."));
  };
  #[cfg(target_os = "macos")]
  {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSApplication, NSRequestUserAttentionType};
    let Some(mtm) = MainThreadMarker::new() else {
      return Err(unsupported("Dock methods run on the main thread."));
    };
    let app = NSApplication::sharedApplication(mtm);
    let kind = if critical {
      NSRequestUserAttentionType::CriticalRequest
    } else {
      NSRequestUserAttentionType::InformationalRequest
    };
    let _ = app.requestUserAttention(kind);
    let _ = install_hooks();
    Ok(())
  }
  #[cfg(not(target_os = "macos"))]
  {
    if crate::os::request_attention(critical) {
      Ok(())
    } else {
      Err(unsupported(
        "This system has no window to request attention.",
      ))
    }
  }
}

#[cfg(target_os = "macos")]
fn install_hooks() -> Result<(), OpError> {
  use objc2::runtime::{AnyClass, AnyObject, Imp};
  use objc2::{MainThreadMarker, sel};
  use objc2_app_kit::NSApplication;
  crate::os::ensure_running().map_err(unsupported)?;
  if HOOKS.swap(true, Ordering::SeqCst) {
    return Ok(());
  }
  let Some(mtm) = MainThreadMarker::new() else {
    HOOKS.store(false, Ordering::SeqCst);
    return Err(unsupported("Dock methods run on the main thread."));
  };
  let app = NSApplication::sharedApplication(mtm);
  let Some(delegate) = app.delegate() else {
    HOOKS.store(false, Ordering::SeqCst);
    return Err(unsupported("The application delegate is not installed."));
  };
  let class = unsafe {
    let object = &*(objc2::rc::Retained::as_ptr(&delegate) as *const AnyObject);
    &mut *(object.class() as *const AnyClass as *mut AnyClass)
  };
  let dock_sel = sel!(applicationDockMenu:);
  if class.instance_method(dock_sel).is_none() {
    let imp: Imp = unsafe { std::mem::transmute(dock_menu as *const ()) };
    let added = unsafe {
      objc2::ffi::class_addMethod(class, dock_sel, imp, c"@@:@".as_ptr())
    };
    if !added.as_bool() {
      HOOKS.store(false, Ordering::SeqCst);
      return Err(unsupported("The dock menu could not be installed."));
    }
  }
  let reopen_sel = sel!(applicationShouldHandleReopen:hasVisibleWindows:);
  if class.instance_method(reopen_sel).is_none() {
    let imp: Imp = unsafe { std::mem::transmute(reopen as *const ()) };
    let encoding = if cfg!(target_arch = "aarch64") {
      c"B@:@B"
    } else {
      c"c@:@c"
    };
    let added = unsafe {
      objc2::ffi::class_addMethod(class, reopen_sel, imp, encoding.as_ptr())
    };
    if !added.as_bool() {
      HOOKS.store(false, Ordering::SeqCst);
      return Err(unsupported(
        "The dock reopen handler could not be installed.",
      ));
    }
  }
  Ok(())
}

#[cfg(target_os = "macos")]
unsafe extern "C-unwind" fn dock_menu(
  _this: *mut objc2::runtime::AnyObject,
  _cmd: objc2::runtime::Sel,
  _sender: *mut objc2::runtime::AnyObject,
) -> *mut objc2::runtime::AnyObject {
  use muda::ContextMenu;
  DOCK_MENU.with(|slot| {
    slot
      .borrow()
      .as_ref()
      .map(|menu| menu.ns_menu())
      .unwrap_or(std::ptr::null_mut())
      .cast()
  })
}

#[cfg(target_os = "macos")]
unsafe extern "C-unwind" fn reopen(
  _this: *mut objc2::runtime::AnyObject,
  _cmd: objc2::runtime::Sel,
  _sender: *mut objc2::runtime::AnyObject,
  has_visible: objc2::runtime::Bool,
) -> objc2::runtime::Bool {
  REOPEN.with(|slot| {
    *slot.borrow_mut() = Some(has_visible.as_bool());
  });
  objc2::runtime::Bool::NO
}

#[cfg(target_os = "macos")]
fn dispatch_reopen(scope: &mut v8::PinScope, has_visible: bool) {
  let target = dock_object(scope);
  let event = Event::new("reopen", false, false, false, true);
  let object = unsafe { yuzuko_core::op::wrap_instance(scope, event) };
  yuzuko_dom::define_is_trusted(scope, object);
  let key = v8::String::new(scope, "hasVisibleWindows").unwrap();
  let value = v8::Boolean::new(scope, has_visible);
  object.set(scope, key.into(), value.into());
  let _ = yuzuko_dom::dispatch(scope, target, object);
}

fn dock_object<'s>(
  scope: &mut v8::PinScope<'s, '_>,
) -> v8::Local<'s, v8::Object> {
  let yuzuko = menu::yuzuko_object(scope);
  let key = v8::String::new(scope, "dock").unwrap();
  let value = yuzuko.get(scope, key.into()).unwrap();
  v8::Local::<v8::Object>::try_from(value).expect("Yuzuko.dock")
}

fn unsupported(message: impl Into<String>) -> OpError {
  OpError::dom("NotSupportedError", message)
}

fn set_method(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
  length: i32,
  callback: impl v8::MapFnTo<v8::FunctionCallback>,
) {
  let function = v8::Function::new(scope, callback).unwrap();
  let key = v8::String::new(scope, "length").unwrap();
  let length = v8::Integer::new(scope, length);
  function.set(scope, key.into(), length.into());
  let key = v8::String::new(scope, name).unwrap();
  object.set(scope, key.into(), function.into());
}
