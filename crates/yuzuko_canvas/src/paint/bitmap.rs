//! `ImageBitmapRenderingContext`.

use std::cell::RefCell;

use yuzuko_core::op::{Js, OpError, traced_local};
use yuzuko_image::ImageBitmap;
use yuzuko_macros::op;

use crate::paint::record::ImageBuf;
use crate::paint::record::{self};

pub struct BitmapContext {
  id: u64,
  canvas: RefCell<v8::TracedReference<v8::Object>>,
}

unsafe impl v8::cppgc::GarbageCollected for BitmapContext {
  fn trace(&self, visitor: &mut v8::cppgc::Visitor) {
    if let Ok(canvas) = self.canvas.try_borrow() {
      visitor.trace(&*canvas);
    }
  }

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"ImageBitmapRenderingContext"
  }
}

impl BitmapContext {
  pub fn create(
    scope: &mut v8::PinScope,
    id: u64,
    canvas: v8::Local<v8::Object>,
  ) -> Self {
    Self {
      id,
      canvas: RefCell::new(yuzuko_core::op::traced(scope, canvas)),
    }
  }
}

#[op]
impl BitmapContext {
  #[getter]
  fn canvas(&self, scope: &mut v8::PinScope) -> Js {
    let value = yuzuko_core::op::traced_local(scope, &self.canvas.borrow())
      .map(v8::Local::<v8::Value>::from)
      .unwrap_or_else(|| v8::null(scope).into());
    Js(v8::Global::new(scope, value))
  }

  fn transfer_from_image_bitmap(
    &self,
    scope: &mut v8::PinScope,
    bitmap: v8::Local<v8::Value>,
  ) -> Result<(), OpError> {
    if bitmap.is_null() {
      if record::with_bitmap(self.id, |image| {
        image.bytes.fill(0);
        image.display_p3 = false;
        image.float16 = false;
        image.premultiplied = false;
      })
      .is_some()
      {
        yuzuko_window::note_canvas_draw(self.id);
      }
      return Ok(());
    }
    let object = v8::Local::<v8::Object>::try_from(bitmap).map_err(|_| {
      OpError::type_("ImageBitmapRenderingContext expected an ImageBitmap")
    })?;
    let host =
      unsafe { yuzuko_core::op::host_ref::<ImageBitmap>(scope, object) }
        .ok_or_else(|| {
          OpError::type_("ImageBitmapRenderingContext expected an ImageBitmap")
        })?;
    let Some(raster) = host.get().take_raster() else {
      return Err(OpError::dom(
        "InvalidStateError",
        "The ImageBitmap is detached",
      ));
    };
    let image = ImageBuf {
      width: raster.width,
      height: raster.height,
      display_p3: raster.color_space == yuzuko_image::ColorSpace::DisplayP3,
      float16: raster.format == yuzuko_image::SampleFormat::Float16,
      premultiplied: raster.premultiplied,
      bytes: raster.bytes,
    };
    if let Some(object) = traced_local(scope, &self.canvas.borrow()) {
      yuzuko_window::reset_canvas_bitmap(
        scope,
        object,
        f64::from(image.width),
        f64::from(image.height),
      );
    }
    if record::with_bitmap(self.id, |slot| {
      *slot = image;
    })
    .is_some()
    {
      yuzuko_window::note_canvas_draw(self.id);
    }
    Ok(())
  }
}
