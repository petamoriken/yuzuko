//! Optional `VideoFrame` pixel lookup. `yuzuko_codec` installs it.

use std::sync::OnceLock;

use crate::raster::Raster;

pub enum FramePixels {
  NotFrame,
  Closed,
  Ready(Raster),
}

pub type FrameLookup =
  fn(&mut v8::PinScope, v8::Global<v8::Value>) -> FramePixels;

static LOOKUP: OnceLock<FrameLookup> = OnceLock::new();

pub fn install_frame_lookup(lookup: FrameLookup) {
  let _ = LOOKUP.set(lookup);
}

pub fn frame_pixels(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> FramePixels {
  match LOOKUP.get().copied() {
    Some(lookup) => lookup(scope, v8::Global::new(scope, value)),
    None => FramePixels::NotFrame,
  }
}
