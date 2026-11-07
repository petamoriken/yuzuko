//! WebCodecs decoders. H.264 stays on the platform hardware decoder.
//! AV1 uses that hardware decoder when it accepts the configuration.

mod audio;
mod av1;
// The OBU parser is only referenced by the Linux VA-API decoder and its tests.
#[cfg(any(test, target_os = "linux"))]
mod av1_obu;
mod backend;
mod classes;
mod clone;
mod color;
mod demux;
mod error;
mod frame;
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
