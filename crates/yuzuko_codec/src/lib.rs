//! WebCodecs decoders. H.264 stays on the platform hardware decoder.

mod audio;
mod backend;
mod classes;
mod color;
mod error;
#[cfg(any(test, target_os = "linux", target_os = "windows"))]
mod h264;
mod image;
mod platform;

use std::sync::Once;

pub fn register() {
  static ONCE: Once = Once::new();
  ONCE.call_once(|| {
    yuzuko_image::install_frame_lookup(classes::lookup_frame);
    yuzuko_core::add_extension(&install);
  });
}

fn install(scope: &mut v8::PinScope) {
  classes::EncodedVideoChunk::install(scope);
  classes::VideoFrame::install(scope);
  classes::VideoDecoder::install(scope);
  classes::EncodedAudioChunk::install(scope);
  classes::AudioData::install(scope);
  classes::AudioDecoder::install(scope);
  image::ImageDecoder::install(scope);
  image::ImageTrackList::install(scope);
  image::ImageTrack::install(scope);
}
