//! `Yuzuko.ComputedAccessibleNode`, a snapshot of the published tree.

use std::cell::RefCell;
use std::collections::HashMap;

use yuzuko_core::op::{Js, OpError};
use yuzuko_macros::op;

use super::class::format_px;
use super::forest::{self, ComputedNode};

thread_local! {
  static SNAPSHOTS: RefCell<HashMap<u64, HashMap<u64, ComputedNode>>> =
    RefCell::new(HashMap::new());
  static OBJECTS: RefCell<HashMap<(u64, u64), v8::Weak<v8::Object>>> =
    RefCell::new(HashMap::new());
}

pub(super) fn clear() {
  SNAPSHOTS.with(|slot| slot.borrow_mut().clear());
  OBJECTS.with(|slot| slot.borrow_mut().clear());
}

pub(super) fn refresh(window: u64) {
  let nodes = forest::snapshot(window);
  SNAPSHOTS.with(|slot| slot.borrow_mut().insert(window, nodes));
}

pub(super) fn contains(window: u64, id: u64) -> bool {
  SNAPSHOTS.with(|slot| {
    slot
      .borrow()
      .get(&window)
      .is_some_and(|nodes| nodes.contains_key(&id))
  })
}

pub(super) fn object<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  window: u64,
  id: u64,
) -> v8::Local<'s, v8::Object> {
  if let Some(existing) = existing_local(scope, window, id) {
    return existing;
  }
  let node = ComputedAccessibleNode { window, id };
  let wrapped = unsafe { yuzuko_core::op::wrap_instance(scope, node) };
  remember(scope, wrapped, window, id);
  wrapped
}

fn existing_local<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  window: u64,
  id: u64,
) -> Option<v8::Local<'s, v8::Object>> {
  let weak = OBJECTS.with(|slot| slot.borrow().get(&(window, id)).cloned())?;
  let view: &v8::PinScope<'s, '_, ()> = scope;
  if let Some(local) = weak.to_local(view) {
    return Some(local);
  }
  OBJECTS.with(|slot| slot.borrow_mut().remove(&(window, id)));
  None
}

fn remember(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  window: u64,
  id: u64,
) {
  let unit: &mut v8::PinScope<'_, '_, ()> = &mut *scope;
  let isolate: &mut v8::Isolate = &mut *unit;
  let weak = v8::Weak::with_guaranteed_finalizer(
    isolate,
    object,
    Box::new(move || {
      OBJECTS.with(|slot| slot.borrow_mut().remove(&(window, id)));
    }),
  );
  OBJECTS.with(|slot| {
    slot.borrow_mut().insert((window, id), weak);
  });
}

pub(crate) struct ComputedAccessibleNode {
  window: u64,
  id: u64,
}

unsafe impl v8::cppgc::GarbageCollected for ComputedAccessibleNode {
  fn trace(&self, _visitor: &mut v8::cppgc::Visitor) {}

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"ComputedAccessibleNode"
  }
}

impl ComputedAccessibleNode {
  fn node(&self) -> Option<ComputedNode> {
    SNAPSHOTS.with(|slot| {
      slot
        .borrow()
        .get(&self.window)
        .and_then(|nodes| nodes.get(&self.id))
        .cloned()
    })
  }

  fn text(
    &self,
    pick: impl Fn(&ComputedNode) -> Option<String>,
  ) -> Option<String> {
    self.node().as_ref().and_then(pick)
  }

  fn flag(&self, pick: impl Fn(&ComputedNode) -> Option<bool>) -> Option<bool> {
    self.node().as_ref().and_then(pick)
  }

  fn number(&self, pick: impl Fn(&ComputedNode) -> Option<f64>) -> Option<f64> {
    self.node().as_ref().and_then(pick)
  }

  fn long(&self, pick: impl Fn(&ComputedNode) -> Option<i32>) -> Option<i32> {
    self.node().as_ref().and_then(pick)
  }

  fn link(&self, scope: &mut v8::PinScope, id: Option<u64>) -> Js {
    let Some(id) = id else {
      return Js::null(scope);
    };
    if !contains(self.window, id) {
      return Js::null(scope);
    }
    let object = object(scope, self.window, id);
    let value = v8::Local::<v8::Value>::from(object);
    Js(v8::Global::new(scope, value))
  }

  fn parent_id(&self) -> Option<u64> {
    self.node().and_then(|node| node.parent)
  }

  fn offset_parent_id(&self) -> Option<u64> {
    self.node().and_then(|node| node.offset_parent)
  }

  fn edge_child(&self, last: bool) -> Option<u64> {
    let node = self.node()?;
    if last {
      node.children.last().copied()
    } else {
      node.children.first().copied()
    }
  }

  fn sibling(&self, previous: bool) -> Option<u64> {
    let node = self.node()?;
    let parent = node.parent?;
    SNAPSHOTS.with(|slot| {
      let slot = slot.borrow();
      let children = &slot.get(&self.window)?.get(&parent)?.children;
      let index = children.iter().position(|child| *child == self.id)?;
      let next = if previous {
        index.checked_sub(1)?
      } else {
        index + 1
      };
      children.get(next).copied()
    })
  }

  fn px(&self, pick: impl Fn(&ComputedNode) -> f64) -> Option<String> {
    self.node().map(|node| {
      format_px(Some(pick(&node))).unwrap_or_else(|| "0px".to_string())
    })
  }
}

#[op]
impl ComputedAccessibleNode {
  #[constructor]
  fn new() -> Result<Self, OpError> {
    Err(OpError::type_("Illegal constructor"))
  }

  #[getter]
  fn role(&self) -> Option<String> {
    self.text(|node| Some(node.role.clone()))
  }

  #[getter]
  fn name(&self) -> Option<String> {
    self.text(|node| node.name.clone())
  }

  #[getter]
  fn description(&self) -> Option<String> {
    self.text(|node| node.description.clone())
  }

  #[getter]
  fn role_description(&self) -> Option<String> {
    self.text(|node| node.role_description.clone())
  }

  #[getter]
  fn value_text(&self) -> Option<String> {
    self.text(|node| node.value_text.clone())
  }

  #[getter]
  fn checked(&self) -> Option<String> {
    self.text(|node| node.checked.clone())
  }

  #[getter]
  fn pressed(&self) -> Option<String> {
    self.text(|node| node.pressed.clone())
  }

  #[getter]
  fn disabled(&self) -> Option<bool> {
    self.flag(|node| node.disabled)
  }

  #[getter]
  fn expanded(&self) -> Option<bool> {
    self.flag(|node| node.expanded)
  }

  #[getter]
  fn hidden(&self) -> Option<bool> {
    self.flag(|node| node.hidden)
  }

  #[getter]
  fn selected(&self) -> Option<bool> {
    self.flag(|node| node.selected)
  }

  #[getter]
  fn busy(&self) -> Option<bool> {
    self.flag(|node| node.busy)
  }

  #[getter]
  fn modal(&self) -> Option<bool> {
    self.flag(|node| node.modal)
  }

  #[getter]
  fn required(&self) -> Option<bool> {
    self.flag(|node| node.required)
  }

  #[getter]
  fn read_only(&self) -> Option<bool> {
    self.flag(|node| node.read_only)
  }

  #[getter]
  fn multiselectable(&self) -> Option<bool> {
    self.flag(|node| node.multiselectable)
  }

  #[getter]
  fn multiline(&self) -> Option<bool> {
    self.flag(|node| node.multiline)
  }

  #[getter]
  fn value_now(&self) -> Option<f64> {
    self.number(|node| node.value_now)
  }

  #[getter]
  fn value_min(&self) -> Option<f64> {
    self.number(|node| node.value_min)
  }

  #[getter]
  fn value_max(&self) -> Option<f64> {
    self.number(|node| node.value_max)
  }

  #[getter]
  fn col_count(&self) -> Option<i32> {
    self.long(|node| node.col_count)
  }

  #[getter]
  fn row_count(&self) -> Option<i32> {
    self.long(|node| node.row_count)
  }

  #[getter]
  fn col_index(&self) -> Option<i32> {
    self.long(|node| node.col_index)
  }

  #[getter]
  fn col_span(&self) -> Option<i32> {
    self.long(|node| node.col_span)
  }

  #[getter]
  fn row_index(&self) -> Option<i32> {
    self.long(|node| node.row_index)
  }

  #[getter]
  fn row_span(&self) -> Option<i32> {
    self.long(|node| node.row_span)
  }

  #[getter]
  fn level(&self) -> Option<i32> {
    self.long(|node| node.level)
  }

  #[getter]
  fn set_size(&self) -> Option<i32> {
    self.long(|node| node.set_size)
  }

  #[getter]
  fn pos_in_set(&self) -> Option<i32> {
    self.long(|node| node.pos_in_set)
  }

  #[getter]
  fn live(&self) -> Option<String> {
    self.text(|node| node.live.clone())
  }

  #[getter]
  fn orientation(&self) -> Option<String> {
    self.text(|node| node.orientation.clone())
  }

  #[getter]
  fn focusable(&self) -> bool {
    self.node().is_some_and(|node| node.focusable)
  }

  #[getter]
  fn offset_left(&self) -> Option<String> {
    self.px(|node| node.offset_left)
  }

  #[getter]
  fn offset_top(&self) -> Option<String> {
    self.px(|node| node.offset_top)
  }

  #[getter]
  fn offset_width(&self) -> Option<String> {
    self.px(|node| node.offset_width)
  }

  #[getter]
  fn offset_height(&self) -> Option<String> {
    self.px(|node| node.offset_height)
  }

  #[getter]
  fn offset_parent(&self, scope: &mut v8::PinScope) -> Js {
    self.link(scope, self.offset_parent_id())
  }

  #[getter]
  fn parent(&self, scope: &mut v8::PinScope) -> Js {
    self.link(scope, self.parent_id())
  }

  #[getter]
  fn first_child(&self, scope: &mut v8::PinScope) -> Js {
    self.link(scope, self.edge_child(false))
  }

  #[getter]
  fn last_child(&self, scope: &mut v8::PinScope) -> Js {
    self.link(scope, self.edge_child(true))
  }

  #[getter]
  fn previous_sibling(&self, scope: &mut v8::PinScope) -> Js {
    self.link(scope, self.sibling(true))
  }

  #[getter]
  fn next_sibling(&self, scope: &mut v8::PinScope) -> Js {
    self.link(scope, self.sibling(false))
  }

  #[promise]
  fn ensure_up_to_date(&self, scope: &mut v8::PinScope) -> Js {
    super::queue_refresh(scope, self.window)
  }
}
