//! JavaScript `GPUObjectBase.label`. wgpu keeps the label it was given at
//! creation. Assigning the attribute updates only this string.

use std::cell::RefCell;

#[derive(Default)]
pub(crate) struct ObjectLabel(RefCell<String>);

impl ObjectLabel {
  pub(crate) fn new(label: Option<String>) -> Self {
    Self(RefCell::new(label.unwrap_or_default()))
  }

  pub(crate) fn get(&self) -> String {
    self.0.borrow().clone()
  }

  pub(crate) fn set(&self, value: String) {
    *self.0.borrow_mut() = value;
  }
}
