//! WebCodecs decoders. H.264 stays on the platform hardware decoder.

mod audio;
mod backend;
mod classes;
mod clone;
mod color;
mod demux;
mod error;
mod frame;
#[cfg(any(test, target_os = "linux", target_os = "windows"))]
mod h264;
mod image;
mod layout;
mod mp4;
mod pcm;
mod platform;
mod samples;

use std::sync::Once;

pub fn register() {
  static ONCE: Once = Once::new();
  ONCE.call_once(|| {
    clone::register();
    yuzuko_image::install_frame_lookup(classes::lookup_frame);
    yuzuko_core::add_extension(&install);
  });
}

fn install(scope: &mut v8::PinScope) {
  classes::EncodedVideoChunk::install(scope);
  classes::VideoFrame::install(scope);
  classes::VideoColorSpace::install(scope);
  classes::VideoDecoder::install(scope);
  classes::EncodedAudioChunk::install(scope);
  classes::AudioData::install(scope);
  classes::AudioDecoder::install(scope);
  image::ImageDecoder::install(scope);
  image::ImageTrackList::install(scope);
  image::ImageTrack::install(scope);
  demux::install(scope);
}
