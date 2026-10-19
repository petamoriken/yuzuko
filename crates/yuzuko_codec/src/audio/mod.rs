//! AAC selection for `AudioDecoder`.

mod aac;

use crate::error::DecodeError;
use aac::{AacDecoder, Pcm};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AudioCodec {
  Aac,
}

pub struct AudioBackend {
  inner: AacDecoder,
}

impl AudioBackend {
  pub fn open(
    codec: AudioCodec,
    description: &[u8],
    sample_rate: u32,
    channels: u32,
  ) -> Result<Self, DecodeError> {
    match codec {
      AudioCodec::Aac => Ok(Self {
        inner: AacDecoder::open(description, sample_rate, channels)?,
      }),
    }
  }

  pub fn decode(&mut self, bytes: &[u8]) -> Result<Option<Pcm>, DecodeError> {
    self.inner.decode(bytes).map(Some)
  }
}
