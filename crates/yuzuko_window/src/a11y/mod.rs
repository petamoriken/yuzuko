//! Virtual accessibility nodes and the AccessKit tree for one process.

mod class;
mod forest;

use std::cell::RefCell;
use std::collections::HashMap;

use yuzuko_core::op::{OpError, OpException, host_ref};

use crate::inbox;
use crate::os;

pub(crate) use forest::ActionKind;

thread_local! {
  static WEAKS: RefCell<HashMap<u64, v8::Weak<v8::Object>>> =
    RefCell::new(HashMap::new());
}

pub(crate) fn install(scope: &mut v8::PinScope) {
  class::AccessibleNode::install(scope);
  let global = scope.get_current_context().global(scope);
  let key = v8::String::new(scope, "AccessibleNode").unwrap();
  let Some(constructor) = global.get(scope, key.into()) else {
    return;
  };
  let _ = global.delete(scope, key.into());
  let yuzuko = yuzuko_core::namespace(scope, "Yuzuko");
  yuzuko.set(scope, key.into(), constructor);
  install_surface(scope);
}

pub(crate) fn clear() {
  WEAKS.with(|slot| slot.borrow_mut().clear());
  if !yuzuko_core::is_dedicated_worker() {
    forest::clear();
  }
}

pub(crate) fn note_viewport(
  id: u64,
  title: String,
  css_width: f64,
  css_height: f64,
  scale: f64,
) {
  forest::note_viewport(id, title, css_width, css_height, scale);
}

pub(crate) fn note_size(id: u64, css_width: f64, css_height: f64, scale: f64) {
  forest::note_size(id, css_width, css_height, scale);
}

pub(crate) fn note_title(id: u64, title: String) {
  forest::note_title(id, title);
}

pub(crate) fn release_window(id: u64) {
  forest::release_window(id);
}

pub(crate) fn is_closed(id: u64) -> bool {
  forest::is_closed(id)
}

pub(crate) fn enqueue(request: accesskit::ActionRequest) {
  forest::enqueue(request);
}

pub(crate) fn full_tree(window: u64) -> accesskit::TreeUpdate {
  forest::full_tree(window)
}

pub(crate) fn node_id(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<u64, OpError> {
  let Ok(object) = v8::Local::<v8::Object>::try_from(value) else {
    return Err(OpError::type_("The node must be an AccessibleNode."));
  };
  let Some(host) =
    (unsafe { host_ref::<class::AccessibleNode>(scope, object) })
  else {
    return Err(OpError::type_("The node must be an AccessibleNode."));
  };
  Ok(host.get().id())
}

pub(crate) fn inject_and_drain(
  scope: &mut v8::PinScope,
  node: u64,
  kind: ActionKind,
  value: Option<f64>,
) {
  forest::push_pending(node, kind, value);
  drain(scope);
}

pub(crate) fn drain(scope: &mut v8::PinScope) -> bool {
  let pending = forest::take_pending();
  if pending.is_empty() {
    return false;
  }
  let mut windows = Vec::new();
  for item in pending {
    let path = event_path(scope, item.node);
    let name = event_name(item.kind);
    if item.kind == ActionKind::SetValue {
      let detail = item.value.map(|number| {
        let number: v8::Local<v8::Value> =
          v8::Number::new(scope, number).into();
        number
      });
      yuzuko_dom::dispatch_custom_on_path(scope, &path, name, detail);
    } else {
      yuzuko_dom::dispatch_on_path(scope, &path, name);
    }
    if !windows.contains(&item.window) {
      windows.push(item.window);
    }
  }
  for window in windows {
    os::publish_accessible(window);
  }
  true
}

pub(super) fn sync(scope: &mut v8::PinScope, ids: &[u64]) {
  for &id in ids {
    let (parent_id, child_ids) = forest::links(id);
    let parent = parent_id.and_then(|parent| object_local(scope, parent));
    let mut children = Vec::with_capacity(child_ids.len());
    for child in child_ids {
      if let Some(object) = object_local(scope, child) {
        children.push(object);
      }
    }
    let Some(object) = object_local(scope, id) else {
      continue;
    };
    let Some(host) =
      (unsafe { host_ref::<class::AccessibleNode>(scope, object) })
    else {
      continue;
    };
    host.get().store_links(scope, parent, &children);
  }
}

pub(super) fn sync_and_publish(scope: &mut v8::PinScope, ids: &[u64]) {
  sync(scope, ids);
  let mut windows = Vec::new();
  for &id in ids {
    if let Some(window) = forest::open_window(id)
      && !windows.contains(&window)
    {
      windows.push(window);
    }
  }
  for window in windows {
    os::publish_accessible(window);
  }
}

pub(super) fn republish(window: Option<u64>) {
  if let Some(window) = window {
    os::publish_accessible(window);
  }
}

pub(super) fn register_weak(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  id: u64,
) {
  let unit: &mut v8::PinScope<'_, '_, ()> = &mut *scope;
  let isolate: &mut v8::Isolate = &mut *unit;
  let weak = v8::Weak::with_guaranteed_finalizer(
    isolate,
    object,
    Box::new(move || forest::forget_author(id)),
  );
  WEAKS.with(|slot| {
    slot.borrow_mut().insert(id, weak);
  });
}

pub(super) fn object_local<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  id: u64,
) -> Option<v8::Local<'s, v8::Object>> {
  let weak = WEAKS.with(|slot| slot.borrow().get(&id).cloned())?;
  let view: &v8::PinScope<'s, '_, ()> = scope;
  weak.to_local(view)
}

fn fail(error: forest::Fail) -> OpError {
  match error {
    forest::Fail::AlreadyAttached => OpError::dom(
      "InvalidStateError",
      "An accessible root is already attached.",
    ),
    forest::Fail::Closed => {
      OpError::dom("InvalidStateError", "The window is closed.")
    }
    forest::Fail::NotFocusable => {
      OpError::dom("InvalidStateError", "The node is not focusable.")
    }
    forest::Fail::NotConnected => {
      OpError::dom("InvalidStateError", "The node is not connected.")
    }
    forest::Fail::Ancestor => {
      OpError::dom("HierarchyRequestError", "The node is an ancestor.")
    }
    forest::Fail::RootMove => OpError::dom(
      "HierarchyRequestError",
      "The accessible root cannot be moved.",
    ),
    forest::Fail::OffsetParent => OpError::dom(
      "HierarchyRequestError",
      "The offset parent is not an ancestor.",
    ),
    forest::Fail::NotChild => {
      OpError::dom("NotFoundError", "The node is not a child.")
    }
    forest::Fail::Missing => {
      OpError::type_("The node must be an AccessibleNode.")
    }
  }
}

fn install_surface(scope: &mut v8::PinScope) {
  let yuzuko = yuzuko_core::namespace(scope, "Yuzuko");
  let key = v8::String::new(scope, "WindowSurface").unwrap();
  let Some(constructor) = yuzuko.get(scope, key.into()) else {
    return;
  };
  let Ok(constructor) = v8::Local::<v8::Object>::try_from(constructor) else {
    return;
  };
  let proto_key = v8::String::new(scope, "prototype").unwrap();
  let Some(proto) = constructor.get(scope, proto_key.into()) else {
    return;
  };
  let Ok(proto) = v8::Local::<v8::Object>::try_from(proto) else {
    return;
  };
  let attach = v8::Function::builder(attach_accessible_root)
    .length(0)
    .constructor_behavior(v8::ConstructorBehavior::Throw)
    .build(scope)
    .expect("attachAccessibleRoot");
  let attach_name = v8::String::new(scope, "attachAccessibleRoot").unwrap();
  attach.set_name(attach_name);
  proto.set(scope, attach_name.into(), attach.into());

  let getter = v8::Function::builder(accessible_root)
    .length(0)
    .constructor_behavior(v8::ConstructorBehavior::Throw)
    .build(scope)
    .expect("accessibleRoot");
  let getter_name = v8::String::new(scope, "get accessibleRoot").unwrap();
  getter.set_name(getter_name);
  let root_key = v8::String::new(scope, "accessibleRoot").unwrap();
  let get: v8::Local<v8::Value> = getter.into();
  let set: v8::Local<v8::Value> = v8::undefined(scope).into();
  let mut descriptor = v8::PropertyDescriptor::new_from_get_set(get, set);
  descriptor.set_enumerable(true);
  descriptor.set_configurable(true);
  let _ = proto.define_property(scope, root_key.into(), &descriptor);
}

fn attach_accessible_root(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut ret: v8::ReturnValue,
) {
  if let Err(error) = attach_root(scope, &args, &mut ret) {
    OpException::throw(error, scope);
  }
}

fn attach_root(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
  ret: &mut v8::ReturnValue,
) -> Result<(), OpError> {
  let object = args.this();
  let id = {
    let Some(host) =
      (unsafe { host_ref::<crate::surface::WindowSurface>(scope, object) })
    else {
      return Err(OpError::type_("illegal invocation"));
    };
    host.get().window_id()
  };
  if !inbox::contains(id) || forest::is_closed(id) {
    return Err(fail(forest::Fail::Closed));
  }
  if forest::has_root(id) {
    return Err(fail(forest::Fail::AlreadyAttached));
  }
  let (node, node_id) = class::AccessibleNode::detached();
  let wrapped = unsafe { yuzuko_core::op::wrap_instance(scope, node) };
  register_weak(scope, wrapped, node_id);
  if let Err(error) = forest::attach_root(id, node_id) {
    forest::forget_author(node_id);
    return Err(fail(error));
  }
  if let Some(host) =
    unsafe { host_ref::<crate::surface::WindowSurface>(scope, object) }
  {
    host.get().store_accessible_root(scope, wrapped);
  }
  sync(scope, &[node_id]);
  os::publish_accessible(id);
  ret.set(wrapped.into());
  Ok(())
}

fn accessible_root(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut ret: v8::ReturnValue,
) {
  if let Err(error) = read_root(scope, &args, &mut ret) {
    OpException::throw(error, scope);
  }
}

fn read_root(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
  ret: &mut v8::ReturnValue,
) -> Result<(), OpError> {
  let object = args.this();
  let id = {
    let Some(host) =
      (unsafe { host_ref::<crate::surface::WindowSurface>(scope, object) })
    else {
      return Err(OpError::type_("illegal invocation"));
    };
    host.get().window_id()
  };
  if !inbox::contains(id) || forest::is_closed(id) {
    return Err(fail(forest::Fail::Closed));
  }
  let Some(root) = forest::root_id(id) else {
    ret.set(v8::null(scope).into());
    return Ok(());
  };
  match object_local(scope, root) {
    Some(node) => ret.set(node.into()),
    None => ret.set(v8::null(scope).into()),
  }
  Ok(())
}

fn event_path<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  node: u64,
) -> Vec<v8::Local<'s, v8::Object>> {
  let mut path = Vec::new();
  for id in forest::ancestor_ids(node) {
    let Some(object) = object_local(scope, id) else {
      break;
    };
    path.push(object);
  }
  path
}

fn event_name(kind: ActionKind) -> &'static str {
  match kind {
    ActionKind::Click => "accessibleclick",
    ActionKind::ContextMenu => "accessiblecontextmenu",
    ActionKind::Decrement => "accessibledecrement",
    ActionKind::Focus => "accessiblefocus",
    ActionKind::Increment => "accessibleincrement",
    ActionKind::ScrollIntoView => "accessiblescrollintoview",
    ActionKind::SetValue => "accessiblesetvalue",
  }
}
