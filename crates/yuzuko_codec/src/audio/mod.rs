//! AAC and Opus selection for `AudioDecoder`.

mod aac;
mod opus;

use crate::error::DecodeError;
use aac::AacDecoder;
use opus::OpusStream;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AudioCodec {
  Aac,
  Opus,
}

pub struct Pcm {
  pub rate: f64,
  pub channels: Vec<Vec<f32>>,
  pub timestamp: f64,
}

enum Inner {
  Aac(AacDecoder),
  Opus(Box<OpusStream>),
}

pub struct AudioBackend {
  inner: Inner,
}

impl AudioBackend {
  pub fn open(
    codec: AudioCodec,
    description: &[u8],
    sample_rate: u32,
    channels: u32,
  ) -> Result<Self, DecodeError> {
    let inner = match codec {
      AudioCodec::Aac => {
        Inner::Aac(AacDecoder::open(description, sample_rate, channels)?)
      }
      AudioCodec::Opus => {
        Inner::Opus(Box::new(OpusStream::open(description, channels)?))
      }
    };
    Ok(Self { inner })
  }

  pub fn decode(
    &mut self,
    bytes: &[u8],
    timestamp: f64,
  ) -> Result<Option<Pcm>, DecodeError> {
    match &mut self.inner {
      Inner::Aac(decoder) => decoder.decode(bytes, timestamp).map(Some),
      Inner::Opus(decoder) => decoder.decode(bytes, timestamp),
    }
  }
}

/// `mp4a`, the MPEG-4 AAC object types Chrome accepts, and MPEG-2 AAC-LC.
pub(crate) fn aac_supported(codec: &str) -> bool {
  matches!(
    codec,
    "mp4a"
      | "mp4a.40.2"
      | "mp4a.40.02"
      | "mp4a.40.5"
      | "mp4a.40.05"
      | "mp4a.40.29"
      | "mp4a.67"
  )
}

#[cfg(test)]
mod tests {
  use super::{AudioBackend, AudioCodec, aac_supported};

  fn read(name: &str) -> Vec<u8> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
      .join("../../tests/wpt/suite/webcodecs")
      .join(name);
    std::fs::read(path).expect(name)
  }

  #[test]
  fn aac_strings_match_chrome() {
    assert!(aac_supported("mp4a"));
    assert!(aac_supported("mp4a.40.2"));
    assert!(aac_supported("mp4a.40.02"));
    assert!(aac_supported("mp4a.67"));
    assert!(!aac_supported("mp4a.FF.9"));
    assert!(!aac_supported("mp4a.69"));
    assert!(!aac_supported("opus"));
  }

  #[test]
  fn opus_preskip_moves_only_a_negative_timestamp() {
    let file = read("sfx-opus.ogg");
    let description = &file[28..47];
    let first = &file[185..635];
    let second = &file[635..903];
    let mut decoder =
      AudioBackend::open(AudioCodec::Opus, description, 48_000, 1)
        .expect("open");
    let early = decoder.decode(first, -42.0).expect("first").expect("pcm");
    assert_eq!(early.rate, 48_000.0);
    assert_eq!(early.channels.len(), 1);
    assert_eq!(early.channels[0].len(), 960 - 312);
    assert_eq!(early.timestamp, -42.0 + 6_500.0);
    let next = decoder.decode(second, 20_000.0 - 42.0).expect("second");
    let next = next.expect("pcm");
    assert_eq!(next.channels[0].len(), 960);
    assert_eq!(next.timestamp, 20_000.0 - 42.0);

    let mut positive =
      AudioBackend::open(AudioCodec::Opus, description, 44_100, 1)
        .expect("open");
    let kept = positive.decode(first, 42.0).expect("kept").expect("pcm");
    assert_eq!(kept.timestamp, 42.0);
    assert_eq!(kept.rate, 48_000.0);
    assert_eq!(kept.channels[0].len(), 960 - 312);
  }

  #[test]
  fn opus_without_a_description_rejects_more_than_two_channels() {
    let error = AudioBackend::open(AudioCodec::Opus, &[], 48_000, 16);
    assert!(error.is_err());
    let short = AudioBackend::open(AudioCodec::Opus, &[0; 9], 48_000, 3);
    assert!(short.is_err());
    let stereo = AudioBackend::open(AudioCodec::Opus, &[], 8_000, 2);
    assert!(stereo.is_ok());
  }

  #[test]
  fn adts_aac_decodes_without_a_description() {
    let file = read("sfx.adts");
    let mut decoder =
      AudioBackend::open(AudioCodec::Aac, &[], 48_000, 1).expect("open");
    let frame = &file[..248];
    let pcm = decoder.decode(frame, -42.0).expect("decode").expect("pcm");
    assert_eq!(pcm.rate, 48_000.0);
    assert_eq!(pcm.channels.len(), 1);
    assert!(!pcm.channels[0].is_empty());
    assert_eq!(pcm.timestamp, -42.0);
  }
}
