//! `Yuzuko.AccessibleNode`.

use std::cell::RefCell;

use yuzuko_core::op::{FromV8, Js, OpError, host_ref, traced};
use yuzuko_dom::EventTarget;
use yuzuko_macros::op;

use super::forest::{self, Author};

#[repr(C)]
pub(crate) struct AccessibleNode {
  pub base: EventTarget,
  id: u64,
  parent: RefCell<Option<v8::TracedReference<v8::Object>>>,
  children: RefCell<Vec<v8::TracedReference<v8::Object>>>,
}

unsafe impl v8::cppgc::GarbageCollected for AccessibleNode {
  fn trace(&self, visitor: &mut v8::cppgc::Visitor) {
    v8::cppgc::GarbageCollected::trace(&self.base, visitor);
    if let Ok(parent) = self.parent.try_borrow()
      && let Some(parent) = parent.as_ref()
    {
      visitor.trace(parent);
    }
    if let Ok(children) = self.children.try_borrow() {
      for child in children.iter() {
        visitor.trace(child);
      }
    }
  }

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"AccessibleNode"
  }
}

impl AccessibleNode {
  pub(super) fn detached() -> (Self, u64) {
    let id = forest::create();
    (
      Self {
        base: EventTarget::new(),
        id,
        parent: RefCell::new(None),
        children: RefCell::new(Vec::new()),
      },
      id,
    )
  }

  pub(super) fn id(&self) -> u64 {
    self.id
  }

  pub(super) fn store_links(
    &self,
    scope: &mut v8::PinScope,
    parent: Option<v8::Local<v8::Object>>,
    children: &[v8::Local<v8::Object>],
  ) {
    let parent = parent.map(|value| traced(scope, value));
    let children = children
      .iter()
      .copied()
      .map(|value| traced(scope, value))
      .collect();
    *self.parent.borrow_mut() = parent;
    *self.children.borrow_mut() = children;
  }

  fn read<T: Default>(&self, read: impl FnOnce(&Author) -> T) -> T {
    forest::map(self.id, read)
  }

  fn write_author(
    &self,
    write: impl FnOnce(&mut Author),
  ) -> Result<(), OpError> {
    let window = forest::update(self.id, write);
    super::republish(window);
    Ok(())
  }

  fn set_text(
    &self,
    scope: &mut v8::PinScope,
    value: v8::Local<v8::Value>,
    write: impl FnOnce(&mut Author, Option<String>),
  ) -> Result<(), OpError> {
    let text = optional_string(scope, value)?;
    self.write_author(|author| write(author, text))
  }

  fn set_token(
    &self,
    scope: &mut v8::PinScope,
    value: v8::Local<v8::Value>,
    allowed: &[&str],
    write: impl FnOnce(&mut Author, Option<String>),
  ) -> Result<(), OpError> {
    let text = optional_token(scope, value, allowed)?;
    self.write_author(|author| write(author, text))
  }

  fn set_flag(
    &self,
    scope: &mut v8::PinScope,
    value: v8::Local<v8::Value>,
    write: impl FnOnce(&mut Author, Option<bool>),
  ) -> Result<(), OpError> {
    let flag = optional_bool(scope, value)?;
    self.write_author(|author| write(author, flag))
  }

  fn set_finite(
    &self,
    scope: &mut v8::PinScope,
    value: v8::Local<v8::Value>,
    write: impl FnOnce(&mut Author, Option<f64>),
  ) -> Result<(), OpError> {
    let number = optional_finite(scope, value)?;
    self.write_author(|author| write(author, number))
  }

  fn set_long(
    &self,
    scope: &mut v8::PinScope,
    value: v8::Local<v8::Value>,
    write: impl FnOnce(&mut Author, Option<i32>),
  ) -> Result<(), OpError> {
    let number = optional_long(scope, value)?;
    self.write_author(|author| write(author, number))
  }

  fn set_length(
    &self,
    scope: &mut v8::PinScope,
    value: v8::Local<v8::Value>,
    write: impl FnOnce(&mut Author, Option<f64>),
  ) -> Result<(), OpError> {
    let length = optional_offset(scope, value)?;
    self.write_author(|author| write(author, length))
  }

  fn assign_offset_parent(&self, parent: Option<u64>) -> Result<(), OpError> {
    match forest::set_offset_parent(self.id, parent) {
      Ok(window) => {
        super::republish(window);
        Ok(())
      }
      Err(forest::Fail::Missing) => Err(OpError::type_(
        "The offset parent must be an AccessibleNode.",
      )),
      Err(error) => Err(super::fail(error)),
    }
  }
}

#[op(inherit = EventTarget)]
impl AccessibleNode {
  #[constructor]
  fn new() -> Self {
    Self::detached().0
  }

  #[construct_hook]
  fn finish(&self, scope: &mut v8::PinScope, object: v8::Local<v8::Object>) {
    super::register_weak(scope, object, self.id);
  }

  fn append_child(
    &self,
    scope: &mut v8::PinScope,
    node: v8::Local<v8::Value>,
  ) -> Result<Js, OpError> {
    let child = super::node_id(scope, node)?;
    let touched = forest::append_child(self.id, child).map_err(super::fail)?;
    super::sync_and_publish(scope, &touched);
    Ok(value_js(scope, node))
  }

  fn insert_before(
    &self,
    scope: &mut v8::PinScope,
    node: v8::Local<v8::Value>,
    child: Option<v8::Local<v8::Value>>,
  ) -> Result<Js, OpError> {
    let node_id = super::node_id(scope, node)?;
    let before = match child {
      None => None,
      Some(value) if value.is_null() || value.is_undefined() => None,
      Some(value) => Some(super::node_id(scope, value)?),
    };
    let touched =
      forest::insert_before(self.id, node_id, before).map_err(super::fail)?;
    super::sync_and_publish(scope, &touched);
    Ok(value_js(scope, node))
  }

  fn replace_child(
    &self,
    scope: &mut v8::PinScope,
    node: v8::Local<v8::Value>,
    child: v8::Local<v8::Value>,
  ) -> Result<Js, OpError> {
    let node_id = super::node_id(scope, node)?;
    let child_id = super::node_id(scope, child)?;
    let touched =
      forest::replace_child(self.id, node_id, child_id).map_err(super::fail)?;
    super::sync_and_publish(scope, &touched);
    Ok(value_js(scope, child))
  }

  fn remove_child(
    &self,
    scope: &mut v8::PinScope,
    node: v8::Local<v8::Value>,
  ) -> Result<Js, OpError> {
    let child = super::node_id(scope, node)?;
    let touched = forest::remove_child(self.id, child).map_err(super::fail)?;
    super::sync_and_publish(scope, &touched);
    Ok(value_js(scope, node))
  }

  fn focus(&self) -> Result<(), OpError> {
    let window = forest::focus(self.id).map_err(super::fail)?;
    super::republish(Some(window));
    Ok(())
  }

  #[getter]
  fn role(&self) -> Option<String> {
    self.read(|author| author.role.clone())
  }

  #[setter]
  fn role(
    &self,
    scope: &mut v8::PinScope,
    value: v8::Local<v8::Value>,
  ) -> Result<(), OpError> {
    self.set_text(scope, value, |author, text| author.role = text)
  }

  #[getter]
  fn name(&self) -> Option<String> {
    self.read(|author| author.name.clone())
  }

  #[setter]
  fn name(
    &self,
    scope: &mut v8::PinScope,
    value: v8::Local<v8::Value>,
  ) -> Result<(), OpError> {
    self.set_text(scope, value, |author, text| author.name = text)
  }

  #[getter]
  fn description(&self) -> Option<String> {
    self.read(|author| author.description.clone())
  }

  #[setter]
  fn description(
    &self,
    scope: &mut v8::PinScope,
    value: v8::Local<v8::Value>,
  ) -> Result<(), OpError> {
    self.set_text(scope, value, |author, text| author.description = text)
  }

  #[getter]
  fn role_description(&self) -> Option<String> {
    self.read(|author| author.role_description.clone())
  }

  #[setter]
  fn role_description(
    &self,
    scope: &mut v8::PinScope,
    value: v8::Local<v8::Value>,
  ) -> Result<(), OpError> {
    self.set_text(scope, value, |author, text| author.role_description = text)
  }

  #[getter]
  fn value_text(&self) -> Option<String> {
    self.read(|author| author.value_text.clone())
  }

  #[setter]
  fn value_text(
    &self,
    scope: &mut v8::PinScope,
    value: v8::Local<v8::Value>,
  ) -> Result<(), OpError> {
    self.set_text(scope, value, |author, text| author.value_text = text)
  }

  #[getter]
  fn checked(&self) -> Option<String> {
    self.read(|author| author.checked.clone())
  }

  #[setter]
  fn checked(
    &self,
    scope: &mut v8::PinScope,
    value: v8::Local<v8::Value>,
  ) -> Result<(), OpError> {
    self.set_token(scope, value, &["true", "false", "mixed"], |author, text| {
      author.checked = text
    })
  }

  #[getter]
  fn pressed(&self) -> Option<String> {
    self.read(|author| author.pressed.clone())
  }

  #[setter]
  fn pressed(
    &self,
    scope: &mut v8::PinScope,
    value: v8::Local<v8::Value>,
  ) -> Result<(), OpError> {
    self.set_token(scope, value, &["true", "false", "mixed"], |author, text| {
      author.pressed = text
    })
  }

  #[getter]
  fn disabled(&self) -> Option<bool> {
    self.read(|author| author.disabled)
  }

  #[setter]
  fn disabled(
    &self,
    scope: &mut v8::PinScope,
    value: v8::Local<v8::Value>,
  ) -> Result<(), OpError> {
    self.set_flag(scope, value, |author, flag| author.disabled = flag)
  }

  #[getter]
  fn expanded(&self) -> Option<bool> {
    self.read(|author| author.expanded)
  }

  #[setter]
  fn expanded(
    &self,
    scope: &mut v8::PinScope,
    value: v8::Local<v8::Value>,
  ) -> Result<(), OpError> {
    self.set_flag(scope, value, |author, flag| author.expanded = flag)
  }

  #[getter]
  fn hidden(&self) -> Option<bool> {
    self.read(|author| author.hidden)
  }

  #[setter]
  fn hidden(
    &self,
    scope: &mut v8::PinScope,
    value: v8::Local<v8::Value>,
  ) -> Result<(), OpError> {
    self.set_flag(scope, value, |author, flag| author.hidden = flag)
  }

  #[getter]
  fn selected(&self) -> Option<bool> {
    self.read(|author| author.selected)
  }

  #[setter]
  fn selected(
    &self,
    scope: &mut v8::PinScope,
    value: v8::Local<v8::Value>,
  ) -> Result<(), OpError> {
    self.set_flag(scope, value, |author, flag| author.selected = flag)
  }

  #[getter]
  fn busy(&self) -> Option<bool> {
    self.read(|author| author.busy)
  }

  #[setter]
  fn busy(
    &self,
    scope: &mut v8::PinScope,
    value: v8::Local<v8::Value>,
  ) -> Result<(), OpError> {
    self.set_flag(scope, value, |author, flag| author.busy = flag)
  }

  #[getter]
  fn modal(&self) -> Option<bool> {
    self.read(|author| author.modal)
  }

  #[setter]
  fn modal(
    &self,
    scope: &mut v8::PinScope,
    value: v8::Local<v8::Value>,
  ) -> Result<(), OpError> {
    self.set_flag(scope, value, |author, flag| author.modal = flag)
  }

  #[getter]
  fn required(&self) -> Option<bool> {
    self.read(|author| author.required)
  }

  #[setter]
  fn required(
    &self,
    scope: &mut v8::PinScope,
    value: v8::Local<v8::Value>,
  ) -> Result<(), OpError> {
    self.set_flag(scope, value, |author, flag| author.required = flag)
  }

  #[getter]
  fn read_only(&self) -> Option<bool> {
    self.read(|author| author.read_only)
  }

  #[setter]
  fn read_only(
    &self,
    scope: &mut v8::PinScope,
    value: v8::Local<v8::Value>,
  ) -> Result<(), OpError> {
    self.set_flag(scope, value, |author, flag| author.read_only = flag)
  }

  #[getter]
  fn multiselectable(&self) -> Option<bool> {
    self.read(|author| author.multiselectable)
  }

  #[setter]
  fn multiselectable(
    &self,
    scope: &mut v8::PinScope,
    value: v8::Local<v8::Value>,
  ) -> Result<(), OpError> {
    self.set_flag(scope, value, |author, flag| author.multiselectable = flag)
  }

  #[getter]
  fn multiline(&self) -> Option<bool> {
    self.read(|author| author.multiline)
  }

  #[setter]
  fn multiline(
    &self,
    scope: &mut v8::PinScope,
    value: v8::Local<v8::Value>,
  ) -> Result<(), OpError> {
    self.set_flag(scope, value, |author, flag| author.multiline = flag)
  }

  #[getter]
  fn focusable(&self) -> bool {
    self.read(|author| author.focusable)
  }

  #[setter]
  fn focusable(&self, value: bool) -> Result<(), OpError> {
    self.write_author(|author| author.focusable = value)
  }

  #[getter]
  fn value_now(&self) -> Option<f64> {
    self.read(|author| author.value_now)
  }

  #[setter]
  fn value_now(
    &self,
    scope: &mut v8::PinScope,
    value: v8::Local<v8::Value>,
  ) -> Result<(), OpError> {
    self.set_finite(scope, value, |author, number| author.value_now = number)
  }

  #[getter]
  fn value_min(&self) -> Option<f64> {
    self.read(|author| author.value_min)
  }

  #[setter]
  fn value_min(
    &self,
    scope: &mut v8::PinScope,
    value: v8::Local<v8::Value>,
  ) -> Result<(), OpError> {
    self.set_finite(scope, value, |author, number| author.value_min = number)
  }

  #[getter]
  fn value_max(&self) -> Option<f64> {
    self.read(|author| author.value_max)
  }

  #[setter]
  fn value_max(
    &self,
    scope: &mut v8::PinScope,
    value: v8::Local<v8::Value>,
  ) -> Result<(), OpError> {
    self.set_finite(scope, value, |author, number| author.value_max = number)
  }

  #[getter]
  fn col_count(&self) -> Option<i32> {
    self.read(|author| author.col_count)
  }

  #[setter]
  fn col_count(
    &self,
    scope: &mut v8::PinScope,
    value: v8::Local<v8::Value>,
  ) -> Result<(), OpError> {
    self.set_long(scope, value, |author, number| author.col_count = number)
  }

  #[getter]
  fn row_count(&self) -> Option<i32> {
    self.read(|author| author.row_count)
  }

  #[setter]
  fn row_count(
    &self,
    scope: &mut v8::PinScope,
    value: v8::Local<v8::Value>,
  ) -> Result<(), OpError> {
    self.set_long(scope, value, |author, number| author.row_count = number)
  }

  #[getter]
  fn col_index(&self) -> Option<i32> {
    self.read(|author| author.col_index)
  }

  #[setter]
  fn col_index(
    &self,
    scope: &mut v8::PinScope,
    value: v8::Local<v8::Value>,
  ) -> Result<(), OpError> {
    self.set_long(scope, value, |author, number| author.col_index = number)
  }

  #[getter]
  fn col_span(&self) -> Option<i32> {
    self.read(|author| author.col_span)
  }

  #[setter]
  fn col_span(
    &self,
    scope: &mut v8::PinScope,
    value: v8::Local<v8::Value>,
  ) -> Result<(), OpError> {
    self.set_long(scope, value, |author, number| author.col_span = number)
  }

  #[getter]
  fn row_index(&self) -> Option<i32> {
    self.read(|author| author.row_index)
  }

  #[setter]
  fn row_index(
    &self,
    scope: &mut v8::PinScope,
    value: v8::Local<v8::Value>,
  ) -> Result<(), OpError> {
    self.set_long(scope, value, |author, number| author.row_index = number)
  }

  #[getter]
  fn row_span(&self) -> Option<i32> {
    self.read(|author| author.row_span)
  }

  #[setter]
  fn row_span(
    &self,
    scope: &mut v8::PinScope,
    value: v8::Local<v8::Value>,
  ) -> Result<(), OpError> {
    self.set_long(scope, value, |author, number| author.row_span = number)
  }

  #[getter]
  fn level(&self) -> Option<i32> {
    self.read(|author| author.level)
  }

  #[setter]
  fn level(
    &self,
    scope: &mut v8::PinScope,
    value: v8::Local<v8::Value>,
  ) -> Result<(), OpError> {
    self.set_long(scope, value, |author, number| author.level = number)
  }

  #[getter]
  fn set_size(&self) -> Option<i32> {
    self.read(|author| author.set_size)
  }

  #[setter]
  fn set_size(
    &self,
    scope: &mut v8::PinScope,
    value: v8::Local<v8::Value>,
  ) -> Result<(), OpError> {
    self.set_long(scope, value, |author, number| author.set_size = number)
  }

  #[getter]
  fn pos_in_set(&self) -> Option<i32> {
    self.read(|author| author.pos_in_set)
  }

  #[setter]
  fn pos_in_set(
    &self,
    scope: &mut v8::PinScope,
    value: v8::Local<v8::Value>,
  ) -> Result<(), OpError> {
    self.set_long(scope, value, |author, number| author.pos_in_set = number)
  }

  #[getter]
  fn live(&self) -> Option<String> {
    self.read(|author| author.live.clone())
  }

  #[setter]
  fn live(
    &self,
    scope: &mut v8::PinScope,
    value: v8::Local<v8::Value>,
  ) -> Result<(), OpError> {
    self.set_token(
      scope,
      value,
      &["off", "polite", "assertive"],
      |author, text| author.live = text,
    )
  }

  #[getter]
  fn orientation(&self) -> Option<String> {
    self.read(|author| author.orientation.clone())
  }

  #[setter]
  fn orientation(
    &self,
    scope: &mut v8::PinScope,
    value: v8::Local<v8::Value>,
  ) -> Result<(), OpError> {
    self.set_token(scope, value, &["horizontal", "vertical"], |author, text| {
      author.orientation = text
    })
  }

  #[getter]
  fn offset_left(&self) -> Option<String> {
    format_px(self.read(|author| author.offset_left))
  }

  #[setter]
  fn offset_left(
    &self,
    scope: &mut v8::PinScope,
    value: v8::Local<v8::Value>,
  ) -> Result<(), OpError> {
    self.set_length(scope, value, |author, length| author.offset_left = length)
  }

  #[getter]
  fn offset_top(&self) -> Option<String> {
    format_px(self.read(|author| author.offset_top))
  }

  #[setter]
  fn offset_top(
    &self,
    scope: &mut v8::PinScope,
    value: v8::Local<v8::Value>,
  ) -> Result<(), OpError> {
    self.set_length(scope, value, |author, length| author.offset_top = length)
  }

  #[getter]
  fn offset_width(&self) -> Option<String> {
    format_px(self.read(|author| author.offset_width))
  }

  #[setter]
  fn offset_width(
    &self,
    scope: &mut v8::PinScope,
    value: v8::Local<v8::Value>,
  ) -> Result<(), OpError> {
    self.set_length(scope, value, |author, length| author.offset_width = length)
  }

  #[getter]
  fn offset_height(&self) -> Option<String> {
    format_px(self.read(|author| author.offset_height))
  }

  #[setter]
  fn offset_height(
    &self,
    scope: &mut v8::PinScope,
    value: v8::Local<v8::Value>,
  ) -> Result<(), OpError> {
    self
      .set_length(scope, value, |author, length| author.offset_height = length)
  }

  #[getter]
  fn offset_parent(&self, scope: &mut v8::PinScope) -> Js {
    node_js(scope, self.read(|author| author.offset_parent))
  }

  #[setter]
  fn offset_parent(
    &self,
    scope: &mut v8::PinScope,
    value: v8::Local<v8::Value>,
  ) -> Result<(), OpError> {
    if value.is_null() || value.is_undefined() {
      return self.assign_offset_parent(None);
    }
    let parent = {
      let Ok(object) = v8::Local::<v8::Object>::try_from(value) else {
        return Err(OpError::type_(
          "The offset parent must be an AccessibleNode.",
        ));
      };
      let Some(host) = (unsafe { host_ref::<AccessibleNode>(scope, object) })
      else {
        return Err(OpError::type_(
          "The offset parent must be an AccessibleNode.",
        ));
      };
      host.get().id()
    };
    self.assign_offset_parent(Some(parent))
  }

  #[getter]
  fn parent(&self, scope: &mut v8::PinScope) -> Js {
    node_js(scope, forest::links(self.id).0)
  }

  #[getter]
  fn first_child(&self, scope: &mut v8::PinScope) -> Js {
    node_js(scope, forest::edge_child(self.id, false))
  }

  #[getter]
  fn last_child(&self, scope: &mut v8::PinScope) -> Js {
    node_js(scope, forest::edge_child(self.id, true))
  }

  #[getter]
  fn previous_sibling(&self, scope: &mut v8::PinScope) -> Js {
    node_js(scope, forest::neighbor(self.id, true))
  }

  #[getter]
  fn next_sibling(&self, scope: &mut v8::PinScope) -> Js {
    node_js(scope, forest::neighbor(self.id, false))
  }
}

fn value_js(scope: &mut v8::PinScope, value: v8::Local<v8::Value>) -> Js {
  Js(v8::Global::new(scope, value))
}

fn node_js(scope: &mut v8::PinScope, id: Option<u64>) -> Js {
  match id.and_then(|id| super::object_local(scope, id)) {
    Some(object) => {
      let value = v8::Local::<v8::Value>::from(object);
      Js(v8::Global::new(scope, value))
    }
    None => Js::null(scope),
  }
}

fn optional_string(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<Option<String>, OpError> {
  if value.is_null() || value.is_undefined() {
    Ok(None)
  } else {
    String::from_v8(scope, value).map(Some)
  }
}

fn optional_token(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
  allowed: &[&str],
) -> Result<Option<String>, OpError> {
  let Some(text) = optional_string(scope, value)? else {
    return Ok(None);
  };
  if allowed.contains(&text.as_str()) {
    Ok(Some(text))
  } else {
    Err(OpError::type_("The value is not a valid token."))
  }
}

fn optional_bool(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<Option<bool>, OpError> {
  if value.is_null() || value.is_undefined() {
    Ok(None)
  } else {
    Ok(Some(value.boolean_value(scope)))
  }
}

fn optional_finite(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<Option<f64>, OpError> {
  if value.is_null() || value.is_undefined() {
    return Ok(None);
  }
  let number = f64::from_v8(scope, value)?;
  if number.is_finite() {
    Ok(Some(number))
  } else {
    Err(OpError::type_("The value must be a finite number."))
  }
}

fn optional_long(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<Option<i32>, OpError> {
  if value.is_null() || value.is_undefined() {
    Ok(None)
  } else {
    i32::from_v8(scope, value).map(Some)
  }
}

fn optional_offset(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<Option<f64>, OpError> {
  if value.is_null() || value.is_undefined() {
    return Ok(None);
  }
  if value.is_number() {
    let number = value.number_value(scope).unwrap_or(f64::NAN);
    if number.is_finite() {
      return Ok(Some(number));
    }
    return Err(OpError::type_("The offset must be a CSS pixel length."));
  }
  if value.is_string() {
    let text = value.to_rust_string_lossy(scope);
    if let Some(number) = parse_px(&text) {
      return Ok(Some(number));
    }
  }
  Err(OpError::type_("The offset must be a CSS pixel length."))
}

fn format_px(value: Option<f64>) -> Option<String> {
  let value = value?;
  let text = if value.fract() == 0.0 && value.abs() < 1e15 {
    format!("{}px", value as i64)
  } else {
    format!("{value:?}px")
  };
  Some(text)
}

fn parse_px(text: &str) -> Option<f64> {
  let text = text.trim();
  if text.is_empty() {
    return None;
  }
  let body = if has_px_suffix(text) {
    &text[..text.len() - 2]
  } else {
    text
  };
  if !is_plain_number(body) {
    return None;
  }
  body.parse().ok()
}

fn has_px_suffix(text: &str) -> bool {
  let bytes = text.as_bytes();
  if bytes.len() < 2 {
    return false;
  }
  let end = bytes.len();
  bytes[end - 2].eq_ignore_ascii_case(&b'p')
    && bytes[end - 1].eq_ignore_ascii_case(&b'x')
}

fn is_plain_number(text: &str) -> bool {
  let bytes = text.as_bytes();
  if bytes.is_empty() {
    return false;
  }
  let mut index = 0;
  if bytes[0] == b'+' || bytes[0] == b'-' {
    index = 1;
  }
  if index >= bytes.len() {
    return false;
  }
  let mut saw_digit = false;
  let mut saw_dot = false;
  while index < bytes.len() {
    let byte = bytes[index];
    if byte.is_ascii_digit() {
      saw_digit = true;
    } else if byte == b'.' && !saw_dot {
      saw_dot = true;
    } else {
      return false;
    }
    index += 1;
  }
  saw_digit
}

#[cfg(test)]
mod tests {
  use super::parse_px;

  #[test]
  fn parses_css_pixel_lengths() {
    assert_eq!(parse_px("30"), Some(30.0));
    assert_eq!(parse_px("30px"), Some(30.0));
    assert_eq!(parse_px("30PX"), Some(30.0));
    assert_eq!(parse_px("30Px"), Some(30.0));
    assert_eq!(parse_px(" 30px "), Some(30.0));
    assert_eq!(parse_px("30.5"), Some(30.5));
    assert_eq!(parse_px("30.5px"), Some(30.5));
    assert_eq!(parse_px("-4px"), Some(-4.0));
    assert_eq!(parse_px("+30.5px"), Some(30.5));
    assert_eq!(parse_px("30 px"), None);
    assert_eq!(parse_px("30em"), None);
    assert_eq!(parse_px("30e1"), None);
    assert_eq!(parse_px("30E1"), None);
    assert_eq!(parse_px("30e1px"), None);
    assert_eq!(parse_px(""), None);
    assert_eq!(parse_px("px"), None);
    assert_eq!(parse_px("0x10"), None);
    assert_eq!(parse_px("++30"), None);
  }
}
