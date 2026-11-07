//! `Yuzuko.Menu`, `Yuzuko.setMenu`, and `Window.setContextMenu`.
//!
//! A selection dispatches `menuclick` on the owner. The application menu's
//! owner is `Yuzuko`. A context menu's owner is that window. macOS installs
//! the application menu on `NSApp`. Windows attaches it to the focused
//! window. Linux has no GTK window to host a bar or a context menu, so
//! `setMenu` and `showContextMenu` throw. `setContextMenu` only stores the
//! menu the right-click default action shows. `showContextMenu` shows the
//! menu it is given and does not replace that stored menu.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

use muda::IsMenuItem;
use yuzuko_core::op::{OpError, OpException, host_ref};
use yuzuko_dom::{Event, EventTarget, define_is_trusted, dispatch};
use yuzuko_macros::op;

use crate::activation;
use crate::inbox;

struct Item {
  label: String,
  action: Option<String>,
  submenu: Option<Vec<Item>>,
}

struct Menu {
  items: Vec<Item>,
}

unsafe impl v8::cppgc::GarbageCollected for Menu {
  fn trace(&self, _visitor: &mut v8::cppgc::Visitor) {}

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"Menu"
  }
}

#[op]
impl Menu {
  #[constructor]
  #[length(1)]
  fn new(
    scope: &mut v8::PinScope,
    items: v8::Local<v8::Value>,
  ) -> Result<Self, OpError> {
    Ok(Self {
      items: read_items(scope, items)?,
    })
  }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum MenuOwner {
  Application,
  Tray(u64),
  Dock,
  Window(u64),
}

std::thread_local! {
  static BAR: RefCell<Option<muda::Menu>> = const { RefCell::new(None) };
  static ROUTES: RefCell<HashMap<String, (MenuOwner, String)>> =
    RefCell::new(HashMap::new());
  static CONTEXT: RefCell<HashMap<u64, muda::Menu>> =
    RefCell::new(HashMap::new());
}

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

pub fn install(scope: &mut v8::PinScope) {
  let yuzuko = yuzuko_object(scope);
  Menu::install(scope);
  let global = scope.get_current_context().global(scope);
  let key = v8::String::new(scope, "Menu").unwrap();
  let ctor = global.get(scope, key.into()).unwrap();
  global.delete(scope, key.into());
  yuzuko.set(scope, key.into(), ctor);
  set_function(scope, yuzuko, "setMenu", set_menu);
}

pub fn clear() {
  clear_routes(MenuOwner::Application);
  BAR.with(|slot| slot.borrow_mut().take());
  let ids =
    CONTEXT.with(|slot| slot.borrow().keys().copied().collect::<Vec<_>>());
  for id in ids {
    clear_context(id);
  }
}

pub(crate) fn has_context(id: u64) -> bool {
  CONTEXT.with(|slot| slot.borrow().contains_key(&id))
}

pub(crate) fn install_context(
  id: u64,
  menu: muda::Menu,
  actions: HashMap<String, String>,
) {
  CONTEXT.with(|slot| {
    slot.borrow_mut().insert(id, menu);
  });
  set_routes(MenuOwner::Window(id), actions);
}

pub(crate) fn clear_context(id: u64) {
  CONTEXT.with(|slot| {
    slot.borrow_mut().remove(&id);
  });
  clear_routes(MenuOwner::Window(id));
}

/// Pops the menu stored by `setContextMenu`. A click is dispatched before
/// this returns. Linux has no GTK window to host the popup.
#[cfg(any(target_os = "macos", target_os = "windows"))]
pub(crate) fn show_context(
  scope: &mut v8::PinScope,
  id: u64,
  x: f64,
  y: f64,
) -> Result<(), String> {
  let Some(menu) = CONTEXT.with(|slot| slot.borrow().get(&id).cloned()) else {
    return Err("The window has no context menu.".to_string());
  };
  present_context(&menu, id, x, y)?;
  poll(scope);
  Ok(())
}

/// Pops `menu` without replacing the menu stored for this window.
/// Its actions route to the window only until the popup returns.
pub(crate) fn show_menu(
  scope: &mut v8::PinScope,
  id: u64,
  menu: &muda::Menu,
  actions: HashMap<String, String>,
  x: f64,
  y: f64,
) -> Result<(), String> {
  let temporary = remember_routes(MenuOwner::Window(id), actions);
  let presented = present_context(menu, id, x, y);
  if presented.is_ok() {
    poll(scope);
  }
  forget_route_ids(&temporary);
  presented
}

pub fn known(action: &str) -> bool {
  owner_of_action(action).is_some()
}

pub(crate) fn set_routes(owner: MenuOwner, actions: HashMap<String, String>) {
  ROUTES.with(|slot| {
    let mut routes = slot.borrow_mut();
    routes.retain(|_, (route_owner, _)| *route_owner != owner);
    for (id, action) in actions {
      routes.insert(id, (owner, action));
    }
  });
}

pub(crate) fn clear_routes(owner: MenuOwner) {
  set_routes(owner, HashMap::new());
}

fn remember_routes(
  owner: MenuOwner,
  actions: HashMap<String, String>,
) -> Vec<String> {
  let mut ids = Vec::with_capacity(actions.len());
  ROUTES.with(|slot| {
    let mut routes = slot.borrow_mut();
    for (id, action) in actions {
      ids.push(id.clone());
      routes.insert(id, (owner, action));
    }
  });
  ids
}

fn forget_route_ids(ids: &[String]) {
  if ids.is_empty() {
    return;
  }
  ROUTES.with(|slot| {
    let mut routes = slot.borrow_mut();
    for id in ids {
      routes.remove(id);
    }
  });
}

fn owner_of_action(action: &str) -> Option<MenuOwner> {
  ROUTES.with(|slot| {
    slot
      .borrow()
      .values()
      .find(|(_, stored)| stored == action)
      .map(|(owner, _)| *owner)
  })
}

fn route_id(id: &str) -> Option<(MenuOwner, String)> {
  ROUTES.with(|slot| slot.borrow().get(id).cloned())
}

pub fn sync_focus(id: u64) {
  let _ = id;
  #[cfg(target_os = "windows")]
  {
    let Some(menu) = BAR.with(|slot| slot.borrow().clone()) else {
      return;
    };
    let _ = present_windows(&menu);
  }
}

/// Native clicks become inbox events. `frame::drive` flushes them.
pub fn poll(scope: &mut v8::PinScope) {
  while let Ok(event) = muda::MenuEvent::receiver().try_recv() {
    let Some((owner, action)) = route_id(&event.id.0) else {
      continue;
    };
    match owner {
      MenuOwner::Application => {
        if let Some(id) = target_window() {
          inbox::push(id, inbox::Raw::Menu(action));
        } else {
          let target = yuzuko_object(scope);
          emit_menuclick(scope, target, &action);
        }
      }
      MenuOwner::Tray(id) => {
        crate::shell::tray::dispatch_menu(scope, id, &action)
      }
      MenuOwner::Dock => crate::shell::dock::dispatch_menu(scope, &action),
      MenuOwner::Window(id) => {
        if let Some(object) = inbox::object(scope, id) {
          emit_menuclick(scope, object, &action);
        }
      }
    }
  }
}

pub fn dispatch_click(scope: &mut v8::PinScope, action: &str) {
  match owner_of_action(action) {
    Some(MenuOwner::Application) | None => {
      let target = yuzuko_object(scope);
      emit_menuclick(scope, target, action);
    }
    Some(MenuOwner::Tray(id)) => {
      crate::shell::tray::dispatch_menu(scope, id, action)
    }
    Some(MenuOwner::Dock) => crate::shell::dock::dispatch_menu(scope, action),
    Some(MenuOwner::Window(id)) => {
      if let Some(object) = inbox::object(scope, id) {
        emit_menuclick(scope, object, action);
      }
    }
  }
}

pub(crate) fn emit_menuclick(
  scope: &mut v8::PinScope,
  target: v8::Local<v8::Object>,
  action: &str,
) {
  let event = Event::new("menuclick", false, false, false, true);
  let object = unsafe { yuzuko_core::op::wrap_instance(scope, event) };
  define_is_trusted(scope, object);
  let key = v8::String::new(scope, "action").unwrap();
  let value = v8::String::new(scope, action).unwrap();
  object.set(scope, key.into(), value.into());
  let _ = dispatch(scope, target, object);
}

fn set_menu(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut _ret: v8::ReturnValue,
) {
  if let Err(error) = install_menu(scope, args.get(0)) {
    OpException::throw(error, scope);
  }
}

fn install_menu(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<(), OpError> {
  let Ok(object) = v8::Local::<v8::Object>::try_from(value) else {
    return Err(OpError::type_("setMenu requires a Menu"));
  };
  let Some(host) = (unsafe { host_ref::<Menu>(scope, object) }) else {
    return Err(OpError::type_("setMenu requires a Menu"));
  };
  let (native, actions) =
    build_native(&host.get().items).map_err(OpError::type_)?;
  present(&native)
    .map_err(|message| OpError::dom("NotSupportedError", message))?;
  BAR.with(|slot| {
    slot.borrow_mut().take();
    *slot.borrow_mut() = Some(native);
  });
  set_routes(MenuOwner::Application, actions);
  Ok(())
}

fn build_loose(
  items: &[Item],
) -> Result<(muda::Menu, HashMap<String, String>), String> {
  let menu = muda::Menu::new();
  let mut actions = HashMap::new();
  for item in items {
    append_loose(&menu, item, &mut actions)?;
  }
  Ok((menu, actions))
}

fn append_loose(
  menu: &muda::Menu,
  item: &Item,
  actions: &mut HashMap<String, String>,
) -> Result<(), String> {
  if let Some(children) = &item.submenu {
    let submenu = muda::Submenu::new(&item.label, true);
    append_items(&submenu, children, actions)?;
    menu.append(&submenu).map_err(|error| error.to_string())?;
    return Ok(());
  }
  let Some(action) = &item.action else {
    return Err("A menu item needs an action or a submenu".to_string());
  };
  let id = next_id();
  actions.insert(id.clone(), action.clone());
  let leaf = muda::MenuItem::with_id(&id, &item.label, true, None);
  menu.append(&leaf).map_err(|error| error.to_string())?;
  Ok(())
}

pub(crate) fn context_from_object(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
  message: &str,
) -> Result<(muda::Menu, HashMap<String, String>), OpError> {
  let Ok(object) = v8::Local::<v8::Object>::try_from(value) else {
    return Err(OpError::type_(message));
  };
  let Some(host) = (unsafe { host_ref::<Menu>(scope, object) }) else {
    return Err(OpError::type_(message));
  };
  build_loose(&host.get().items).map_err(OpError::type_)
}

pub(crate) fn loose_from_object(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<(muda::Menu, HashMap<String, String>), OpError> {
  let Ok(object) = v8::Local::<v8::Object>::try_from(value) else {
    return Err(OpError::type_("setMenu requires a Menu or null."));
  };
  let Some(host) = (unsafe { host_ref::<Menu>(scope, object) }) else {
    return Err(OpError::type_("setMenu requires a Menu or null."));
  };
  build_loose(&host.get().items).map_err(OpError::type_)
}

fn build_native(
  items: &[Item],
) -> Result<(muda::Menu, HashMap<String, String>), String> {
  let menu = muda::Menu::new();
  let mut actions = HashMap::new();
  let mut entries: Vec<muda::Submenu> = Vec::new();
  for item in items {
    let Some(children) = &item.submenu else {
      return Err(
        "The application menu's top level must be a submenu".to_string(),
      );
    };
    let submenu = muda::Submenu::new(&item.label, true);
    append_items(&submenu, children, &mut actions)?;
    entries.push(submenu);
  }
  let refs: Vec<&dyn IsMenuItem> = entries
    .iter()
    .map(|entry| entry as &dyn IsMenuItem)
    .collect();
  menu
    .append_items(&refs)
    .map_err(|error| error.to_string())?;
  Ok((menu, actions))
}

fn append_items(
  parent: &muda::Submenu,
  items: &[Item],
  actions: &mut HashMap<String, String>,
) -> Result<(), String> {
  for item in items {
    if let Some(children) = &item.submenu {
      let submenu = muda::Submenu::new(&item.label, true);
      append_items(&submenu, children, actions)?;
      parent.append(&submenu).map_err(|error| error.to_string())?;
      continue;
    }
    let Some(action) = &item.action else {
      return Err("A menu item needs an action or a submenu".to_string());
    };
    let id = next_id();
    actions.insert(id.clone(), action.clone());
    let leaf = muda::MenuItem::with_id(&id, &item.label, true, None);
    parent.append(&leaf).map_err(|error| error.to_string())?;
  }
  Ok(())
}

fn present_context(
  menu: &muda::Menu,
  id: u64,
  x: f64,
  y: f64,
) -> Result<(), String> {
  #[cfg(target_os = "macos")]
  {
    show_context_mac(menu, id, x, y)
  }
  #[cfg(target_os = "windows")]
  {
    show_context_windows(menu, id, x, y)
  }
  #[cfg(not(any(target_os = "macos", target_os = "windows")))]
  {
    let _ = (menu, id, x, y);
    Err(
      "This window system has no GTK window to host a context menu."
        .to_string(),
    )
  }
}

#[cfg(target_os = "macos")]
fn show_context_mac(
  menu: &muda::Menu,
  id: u64,
  x: f64,
  y: f64,
) -> Result<(), String> {
  use muda::ContextMenu;
  use objc2::rc::Retained;
  use objc2_app_kit::NSView;
  use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
  let view = crate::os::with_window(id, |window| {
    let handle = window.window_handle().ok()?;
    match handle.as_raw() {
      RawWindowHandle::AppKit(appkit) => {
        Some(appkit.ns_view.as_ptr() as *mut NSView)
      }
      _ => None,
    }
  })
  .ok_or_else(|| "This window has no context menu.".to_string())?;
  let Some(view) = view else {
    return Err("This window has no context menu.".to_string());
  };
  let view = unsafe { Retained::retain(view) }
    .ok_or_else(|| "This window has no context menu.".to_string())?;
  if view.window().is_none() {
    return Err("This window has no context menu.".to_string());
  }
  let position =
    muda::dpi::Position::Logical(muda::dpi::LogicalPosition::new(x, y));
  let _ = unsafe {
    menu.show_context_menu_for_nsview(
      Retained::as_ptr(&view).cast(),
      Some(position),
    )
  };
  Ok(())
}

#[cfg(target_os = "windows")]
fn show_context_windows(
  menu: &muda::Menu,
  id: u64,
  x: f64,
  y: f64,
) -> Result<(), String> {
  use muda::ContextMenu;
  use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
  let hwnd = crate::os::with_window(id, |window| {
    let handle = window.window_handle().ok()?;
    match handle.as_raw() {
      RawWindowHandle::Win32(win) => Some(win.hwnd.get()),
      _ => None,
    }
  })
  .flatten()
  .ok_or_else(|| "This window has no context menu.".to_string())?;
  let position =
    muda::dpi::Position::Logical(muda::dpi::LogicalPosition::new(x, y));
  let _ = unsafe { menu.show_context_menu_for_hwnd(hwnd, Some(position)) };
  Ok(())
}

fn present(menu: &muda::Menu) -> Result<(), String> {
  #[cfg(target_os = "macos")]
  {
    menu.init_for_nsapp();
    Ok(())
  }
  #[cfg(target_os = "windows")]
  {
    present_windows(menu)
  }
  #[cfg(not(any(target_os = "macos", target_os = "windows")))]
  {
    let _ = menu;
    Err("This window system has no GTK window to host the menu bar".to_string())
  }
}

#[cfg(target_os = "windows")]
fn present_windows(menu: &muda::Menu) -> Result<(), String> {
  let Some(hwnd) = crate::os::focused_hwnd() else {
    return Ok(());
  };
  unsafe { menu.init_for_hwnd(hwnd) }.map_err(|error| error.to_string())
}

fn target_window() -> Option<u64> {
  if let Some(id) = activation::focused_id()
    && inbox::contains(id)
  {
    return Some(id);
  }
  inbox::ids().into_iter().next()
}

fn next_id() -> String {
  format!("yuzuko-{}", NEXT_ID.fetch_add(1, Ordering::Relaxed))
}

pub(crate) fn yuzuko_object<'s>(
  scope: &mut v8::PinScope<'s, '_>,
) -> v8::Local<'s, v8::Object> {
  let global = scope.get_current_context().global(scope);
  let key = v8::String::new(scope, "Yuzuko").unwrap();
  if let Some(value) = global.get(scope, key.into())
    && let Ok(object) = v8::Local::<v8::Object>::try_from(value)
    && unsafe { host_ref::<EventTarget>(scope, object) }.is_some()
  {
    return object;
  }
  let object =
    unsafe { yuzuko_core::op::wrap_instance(scope, EventTarget::new()) };
  global.set(scope, key.into(), object.into());
  object
}

fn read_items(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<Vec<Item>, OpError> {
  let Ok(list) = v8::Local::<v8::Array>::try_from(value) else {
    return Err(OpError::type_("Menu requires an array of items"));
  };
  let mut items = Vec::new();
  for index in 0..list.length() {
    let Some(value) = list.get_index(scope, index) else {
      return Err(OpError::type_("Menu requires an array of items"));
    };
    items.push(read_item(scope, value)?);
  }
  Ok(items)
}

fn read_item(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<Item, OpError> {
  let Ok(object) = v8::Local::<v8::Object>::try_from(value) else {
    return Err(OpError::type_("A menu item must be an object"));
  };
  let Some(label) = own_string(scope, object, "label") else {
    return Err(OpError::type_("A menu item needs a label"));
  };
  let submenu = own(scope, object, "submenu");
  let action = own_string(scope, object, "action");
  if let Some(submenu) = submenu {
    if action.is_some() {
      return Err(OpError::type_(
        "A menu item has either an action or a submenu",
      ));
    }
    return Ok(Item {
      label,
      action: None,
      submenu: Some(read_items(scope, submenu)?),
    });
  }
  let Some(action) = action else {
    return Err(OpError::type_("A menu item needs an action or a submenu"));
  };
  Ok(Item {
    label,
    action: Some(action),
    submenu: None,
  })
}

fn own<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  object: v8::Local<'_, v8::Object>,
  name: &str,
) -> Option<v8::Local<'s, v8::Value>> {
  let key = v8::String::new(scope, name)?;
  let object = yuzuko_core::op::root(scope, object);
  let value = object.get(scope, key.into())?;
  if value.is_null_or_undefined() {
    None
  } else {
    Some(value)
  }
}

fn set_function(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
  callback: impl v8::MapFnTo<v8::FunctionCallback>,
) {
  let function = v8::Function::new(scope, callback).unwrap();
  let key = v8::String::new(scope, name).unwrap();
  object.set(scope, key.into(), function.into());
}

fn own_string(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
) -> Option<String> {
  let value = own(scope, object, name)?;
  if !value.is_string() {
    return None;
  }
  value
    .to_string(scope)
    .map(|text| text.to_rust_string_lossy(scope))
}
