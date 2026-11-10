//! Opus packet decode for `AudioDecoder`. One or two channels. Output is
//! always 48000 Hz. A description is an OpusHead. Pre-skip drops samples
//! from the start of the stream. A negative timestamp advances by the
//! discarded duration. A non-negative timestamp does not.

use ruopus::OpusDecoder;
use ruopus::ogg::OpusHead;

use crate::error::DecodeError;

use super::Pcm;

pub struct OpusStream {
  decoder: OpusDecoder,
  channels: usize,
  skip: u32,
}

impl OpusStream {
  pub fn open(description: &[u8], channels: u32) -> Result<Self, DecodeError> {
    if !(1..=2).contains(&channels) {
      return Err(DecodeError::Unsupported);
    }
    let skip = if description.is_empty() {
      0
    } else {
      let head =
        OpusHead::parse(description).map_err(|_| DecodeError::Unsupported)?;
      if u32::from(head.channel_count) != channels {
        return Err(DecodeError::Unsupported);
      }
      u32::from(head.pre_skip)
    };
    Ok(Self {
      decoder: OpusDecoder::new(channels as usize),
      channels: channels as usize,
      skip,
    })
  }

  pub fn decode(
    &mut self,
    bytes: &[u8],
    timestamp: f64,
  ) -> Result<Option<Pcm>, DecodeError> {
    let pcm = self
      .decoder
      .decode_packet(bytes)
      .map_err(|_| DecodeError::Rejected)?;
    let channels = self.channels;
    if channels == 0 || !pcm.len().is_multiple_of(channels) {
      return Err(DecodeError::Rejected);
    }
    let frames = pcm.len() / channels;
    let skip = (self.skip as usize).min(frames);
    self.skip -= skip as u32;
    let timestamp = if timestamp < 0.0 {
      timestamp + (skip as f64) * 1_000_000.0 / 48_000.0
    } else {
      timestamp
    };
    if skip == frames {
      return Ok(None);
    }
    let mut planar = vec![Vec::with_capacity(frames - skip); channels];
    for frame in skip..frames {
      for channel in 0..channels {
        planar[channel].push(pcm[frame * channels + channel]);
      }
    }
    Ok(Some(Pcm {
      rate: 48_000.0,
      channels: planar,
      timestamp,
    }))
  }
}
