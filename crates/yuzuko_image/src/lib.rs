//! Image decoders, `ImageData`, `ImageBitmap`, and `createImageBitmap`.

mod bitmap;
mod create;
mod data;
mod frame;
mod raster;

use std::sync::Once;

pub use bitmap::ImageBitmap;
pub use data::ImageData;
pub use frame::{FramePixels, frame_pixels, install_frame_lookup};
pub use raster::color::{convert, convert_encoded};
pub use raster::decode::{
  DecodeError as ImageDecodeError, decode as decode_image,
};
pub use raster::{
  ColorSpace, PixelError, Raster, SampleFormat, orient, premultiply,
  unpremultiply,
};

pub fn register() {
  static ONCE: Once = Once::new();
  ONCE.call_once(|| {
    yuzuko_core::add_extension(&install);
  });
}

fn install(scope: &mut v8::PinScope) {
  data::ImageData::install(scope);
  bitmap::ImageBitmap::install(scope);
  create::install(scope);
}
