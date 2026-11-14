//! Vorbis packet decode for `AudioDecoder`.
//!
//! `description` is Xiph extradata: lace values size every header except the
//! last, and the remaining bytes are that header. The identification header's
//! channel count and sample rate replace the configured values. One chunk is
//! one audio packet. The first packet after open or `reset_overlap` primes
//! the window and produces no audio. The next packet that produces samples
//! uses that priming timestamp, and a negative one increases by half of the
//! short block. Later packets copy their timestamp. Output is planar `f32`.

use std::panic::{AssertUnwindSafe, catch_unwind};

use lewton::audio::PreviousWindowRight;
use lewton::header::IdentHeader;
use lewton::header::SetupHeader;

use crate::error::DecodeError;

use super::Pcm;

pub struct VorbisStream {
  ident: IdentHeader,
  setup: SetupHeader,
  window: PreviousWindowRight,
  rate: f64,
  /// Microseconds in half of the short block. Added to a negative priming
  /// timestamp only.
  discard_us: f64,
  primed: bool,
  prime_timestamp: f64,
  /// The next emitted packet uses `prime_timestamp`.
  use_prime_timestamp: bool,
}

impl VorbisStream {
  pub fn open(
    description: &[u8],
    _sample_rate: u32,
    _channels: u32,
  ) -> Result<Self, DecodeError> {
    let packets = xiph_headers(description)?;
    let opened = catch_unwind(AssertUnwindSafe(|| open_headers(&packets)));
    match opened {
      Ok(Ok((ident, setup))) => {
        let rate = f64::from(ident.audio_sample_rate);
        let discard_us = half_short_block_us(ident.blocksize_0, rate);
        Ok(Self {
          ident,
          setup,
          window: PreviousWindowRight::new(),
          rate,
          discard_us,
          primed: false,
          prime_timestamp: 0.0,
          use_prime_timestamp: false,
        })
      }
      Ok(Err(error)) => Err(error),
      Err(_) => Err(DecodeError::Unsupported),
    }
  }

  pub fn decode(
    &mut self,
    bytes: &[u8],
    timestamp: f64,
  ) -> Result<Option<Pcm>, DecodeError> {
    let decoded = catch_unwind(AssertUnwindSafe(|| {
      lewton::audio::read_audio_packet(
        &self.ident,
        &self.setup,
        bytes,
        &mut self.window,
      )
    }));
    let packet = match decoded {
      Ok(Ok(packet)) => packet,
      Ok(Err(_)) | Err(_) => return Err(DecodeError::Rejected),
    };
    if !self.primed {
      self.primed = true;
      self.prime_timestamp = timestamp;
      self.use_prime_timestamp = true;
      return Ok(None);
    }
    if packet.is_empty() || packet.iter().all(|channel| channel.is_empty()) {
      return Ok(None);
    }
    let frames = packet[0].len();
    if packet.iter().any(|channel| channel.len() != frames) {
      return Err(DecodeError::Rejected);
    }
    let channels = packet
      .into_iter()
      .map(|channel| {
        channel
          .into_iter()
          .map(|sample| f32::from(sample) / 32768.0)
          .collect()
      })
      .collect();
    let timestamp = if self.use_prime_timestamp {
      self.use_prime_timestamp = false;
      if self.prime_timestamp < 0.0 {
        self.prime_timestamp + self.discard_us
      } else {
        self.prime_timestamp
      }
    } else {
      timestamp
    };
    Ok(Some(Pcm {
      rate: self.rate,
      channels,
      timestamp,
    }))
  }

  /// Drop the overlap window. The next packet primes the decoder again.
  pub fn reset_overlap(&mut self) {
    self.window = PreviousWindowRight::new();
    self.primed = false;
    self.use_prime_timestamp = false;
  }
}

fn half_short_block_us(exponent: u8, rate: f64) -> f64 {
  if exponent == 0 || rate <= 0.0 {
    return 0.0;
  }
  let samples = f64::from(1u32 << (exponent - 1));
  (samples / rate * 1_000_000.0).round()
}

fn open_headers(
  packets: &[Vec<u8>; 3],
) -> Result<(IdentHeader, SetupHeader), DecodeError> {
  let ident = lewton::header::read_header_ident(&packets[0])
    .map_err(|_| DecodeError::Unsupported)?;
  lewton::header::read_header_comment(&packets[1])
    .map_err(|_| DecodeError::Unsupported)?;
  let setup = lewton::header::read_header_setup(
    &packets[2],
    ident.audio_channels,
    (ident.blocksize_0, ident.blocksize_1),
  )
  .map_err(|_| DecodeError::Unsupported)?;
  Ok((ident, setup))
}

/// Three Vorbis headers packed as Xiph extradata.
///
/// The first byte is the number of lace values. A lace of 255 continues a
/// packet. Those laces size every header except the last, and the remaining
/// bytes are that last header. A lace table that already sizes every byte is
/// accepted as those packets. The packets are identification (`0x01`),
/// comment (`0x03`), and setup (`0x05`).
fn xiph_headers(description: &[u8]) -> Result<[Vec<u8>; 3], DecodeError> {
  if description.is_empty() {
    return Err(DecodeError::Unsupported);
  }
  let count = usize::from(description[0]);
  let Some(rest) = description.get(1..) else {
    return Err(DecodeError::Unsupported);
  };
  if rest.len() < count {
    return Err(DecodeError::Unsupported);
  }
  let (laces, payload) = rest.split_at(count);
  let mut packets = Vec::new();
  let mut offset = 0usize;
  let mut index = 0usize;
  while index < laces.len() {
    let mut size = 0usize;
    loop {
      let lace = usize::from(laces[index]);
      index += 1;
      size = size.saturating_add(lace);
      if lace != 255 {
        break;
      }
      if index == laces.len() {
        return Err(DecodeError::Unsupported);
      }
    }
    let end = offset.checked_add(size).ok_or(DecodeError::Unsupported)?;
    let packet = payload.get(offset..end).ok_or(DecodeError::Unsupported)?;
    packets.push(packet.to_vec());
    offset = end;
  }
  if offset < payload.len() {
    packets.push(payload[offset..].to_vec());
  }
  let [ident, comment, setup] = match <[Vec<u8>; 3]>::try_from(packets) {
    Ok(packets) => packets,
    Err(_) => return Err(DecodeError::Unsupported),
  };
  if ident.first() != Some(&0x01)
    || comment.first() != Some(&0x03)
    || setup.first() != Some(&0x05)
  {
    return Err(DecodeError::Unsupported);
  }
  Ok([ident, comment, setup])
}
