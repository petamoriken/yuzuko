//! AAC-LC decode. The bytes are one raw frame plus an AudioSpecificConfig.
//! This crate does not demux a container.

use std::panic::{AssertUnwindSafe, catch_unwind};

use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{
  CODEC_TYPE_AAC, CodecParameters, Decoder, DecoderOptions,
};
use symphonia::core::formats::Packet;

use crate::error::DecodeError;

pub struct AacDecoder {
  decoder: Box<dyn Decoder>,
}

pub struct Pcm {
  pub rate: f64,
  pub channels: Vec<Vec<f32>>,
}

impl AacDecoder {
  pub fn open(
    description: &[u8],
    sample_rate: u32,
    channel_count: u32,
  ) -> Result<Self, DecodeError> {
    if description.is_empty() || sample_rate == 0 || channel_count == 0 {
      return Err(DecodeError::Rejected);
    }
    let opened = catch_unwind(AssertUnwindSafe(|| {
      open_inner(description, sample_rate, channel_count)
    }));
    match opened {
      Ok(result) => result,
      Err(_) => Err(DecodeError::Panic),
    }
  }

  pub fn decode(&mut self, bytes: &[u8]) -> Result<Pcm, DecodeError> {
    let decoder = &mut self.decoder;
    match catch_unwind(AssertUnwindSafe(|| decode_frame(decoder, bytes))) {
      Ok(result) => result,
      Err(_) => Err(DecodeError::Panic),
    }
  }
}

fn open_inner(
  description: &[u8],
  sample_rate: u32,
  channel_count: u32,
) -> Result<AacDecoder, DecodeError> {
  let mut params = CodecParameters::new();
  params
    .for_codec(CODEC_TYPE_AAC)
    .with_sample_rate(sample_rate)
    .with_extra_data(description.to_vec().into_boxed_slice());
  if let Some(channels) = channel_mask(channel_count) {
    params.with_channels(channels);
  }
  let decoder = symphonia::default::get_codecs()
    .make(&params, &DecoderOptions::default())
    .map_err(|_| DecodeError::Rejected)?;
  Ok(AacDecoder { decoder })
}

fn channel_mask(count: u32) -> Option<symphonia::core::audio::Channels> {
  use symphonia::core::audio::Channels;
  match count {
    1 => Some(Channels::FRONT_LEFT),
    2 => Some(Channels::FRONT_LEFT | Channels::FRONT_RIGHT),
    _ => None,
  }
}

fn decode_frame(
  decoder: &mut Box<dyn Decoder>,
  bytes: &[u8],
) -> Result<Pcm, DecodeError> {
  if bytes.is_empty() {
    return Err(DecodeError::Rejected);
  }
  let packet = Packet::new_from_slice(0, 0, 1024, bytes);
  let decoded = decoder.decode(&packet).map_err(|_| DecodeError::Rejected)?;
  let spec = *decoded.spec();
  let mut buffer = SampleBuffer::<f32>::new(decoded.capacity() as u64, spec);
  buffer.copy_interleaved_ref(decoded);
  let channels = spec.channels.count();
  if channels == 0 || buffer.samples().is_empty() {
    return Err(DecodeError::Rejected);
  }
  let mut planar = vec![Vec::new(); channels];
  for (index, sample) in buffer.samples().iter().copied().enumerate() {
    planar[index % channels].push(sample);
  }
  Ok(Pcm {
    rate: f64::from(spec.rate),
    channels: planar,
  })
}
