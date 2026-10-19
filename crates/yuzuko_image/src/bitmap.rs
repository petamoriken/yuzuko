//! `ImageBitmap`. The color space and sample format stay inside the bitmap.

use std::cell::Cell;
use std::cell::RefCell;

use yuzuko_macros::op;

use crate::raster::Raster;

pub struct ImageBitmap {
  detached: Cell<bool>,
  raster: RefCell<Option<Raster>>,
}

unsafe impl v8::cppgc::GarbageCollected for ImageBitmap {
  fn trace(&self, _visitor: &mut v8::cppgc::Visitor) {}

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"ImageBitmap"
  }
}

impl ImageBitmap {
  pub fn from_raster(raster: Raster) -> Self {
    Self {
      detached: Cell::new(false),
      raster: RefCell::new(Some(raster)),
    }
  }

  pub fn is_detached(&self) -> bool {
    self.detached.get()
  }

  pub fn copy_raster(&self) -> Option<Raster> {
    self.raster.borrow().clone()
  }

  /// Detaches the bitmap and returns its pixels.
  pub fn take_raster(&self) -> Option<Raster> {
    if self.detached.get() {
      return None;
    }
    self.detached.set(true);
    self.raster.borrow_mut().take()
  }
}

#[op]
impl ImageBitmap {
  #[getter]
  fn width(&self) -> f64 {
    if self.detached.get() {
      return 0.0;
    }
    self
      .raster
      .borrow()
      .as_ref()
      .map(|raster| f64::from(raster.width))
      .unwrap_or(0.0)
  }

  #[getter]
  fn height(&self) -> f64 {
    if self.detached.get() {
      return 0.0;
    }
    self
      .raster
      .borrow()
      .as_ref()
      .map(|raster| f64::from(raster.height))
      .unwrap_or(0.0)
  }

  fn close(&self) {
    self.detached.set(true);
    *self.raster.borrow_mut() = None;
  }
}
