//! AAC-LC decode. A present description is one raw frame plus an
//! AudioSpecificConfig. An empty description is ADTS: the first header
//! builds that config, and every header is removed before the frame.

use std::panic::{AssertUnwindSafe, catch_unwind};

use symphonia::core::codecs::audio::AudioCodecParameters;
use symphonia::core::codecs::audio::AudioDecoder;
use symphonia::core::codecs::audio::AudioDecoderOptions;
use symphonia::core::codecs::audio::well_known::CODEC_ID_AAC;
use symphonia::core::packet::Packet;
use symphonia::core::units::Duration;
use symphonia::core::units::Timestamp;

use crate::error::DecodeError;

use super::Pcm;

const RATES: [u32; 13] = [
  96_000, 88_200, 64_000, 48_000, 44_100, 32_000, 24_000, 22_050, 16_000,
  12_000, 11_025, 8_000, 7_350,
];

pub struct AacDecoder {
  decoder: Option<Box<dyn AudioDecoder>>,
  /// Empty description: every chunk starts with an ADTS header.
  adts: bool,
  /// Configured count, used when an ADTS header leaves the channel index 0.
  channels: u32,
}

impl AacDecoder {
  pub fn open(
    description: &[u8],
    sample_rate: u32,
    channel_count: u32,
  ) -> Result<Self, DecodeError> {
    if sample_rate == 0 || channel_count == 0 {
      return Err(DecodeError::Rejected);
    }
    if description.is_empty() {
      return Ok(Self {
        decoder: None,
        adts: true,
        channels: channel_count,
      });
    }
    let opened = catch_unwind(AssertUnwindSafe(|| {
      open_inner(description, sample_rate, channel_count)
    }));
    let decoder = match opened {
      Ok(result) => result?,
      Err(_) => return Err(DecodeError::Panic),
    };
    Ok(Self {
      decoder: Some(decoder),
      adts: false,
      channels: channel_count,
    })
  }

  pub fn decode(
    &mut self,
    bytes: &[u8],
    timestamp: f64,
  ) -> Result<Pcm, DecodeError> {
    let payload = if self.adts {
      let (asc, payload, rate, channels) = split_adts(bytes)?;
      if self.decoder.is_none() {
        let channels = if channels == 0 {
          self.channels
        } else {
          channels
        };
        let opened =
          catch_unwind(AssertUnwindSafe(|| open_inner(&asc, rate, channels)));
        self.decoder = Some(match opened {
          Ok(result) => result?,
          Err(_) => return Err(DecodeError::Panic),
        });
        self.channels = channels;
      }
      payload
    } else if bytes.is_empty() {
      return Err(DecodeError::Rejected);
    } else {
      bytes.to_vec()
    };
    let Some(decoder) = self.decoder.as_mut() else {
      return Err(DecodeError::Rejected);
    };
    match catch_unwind(AssertUnwindSafe(|| {
      decode_frame(decoder, &payload, timestamp)
    })) {
      Ok(result) => result,
      Err(_) => Err(DecodeError::Panic),
    }
  }
}

fn open_inner(
  description: &[u8],
  sample_rate: u32,
  channel_count: u32,
) -> Result<Box<dyn AudioDecoder>, DecodeError> {
  let mut params = AudioCodecParameters::new();
  params
    .for_codec(CODEC_ID_AAC)
    .with_sample_rate(sample_rate)
    .with_extra_data(description.to_vec().into_boxed_slice());
  if let Some(channels) = channel_mask(channel_count) {
    params.with_channels(channels);
  }
  symphonia::default::get_codecs()
    .make_audio_decoder(&params, &AudioDecoderOptions::default())
    .map_err(|_| DecodeError::Rejected)
}

fn channel_mask(count: u32) -> Option<symphonia::core::audio::Channels> {
  use symphonia::core::audio::layouts::CHANNEL_LAYOUT_MONO;
  use symphonia::core::audio::layouts::CHANNEL_LAYOUT_STEREO;
  match count {
    1 => Some(CHANNEL_LAYOUT_MONO),
    2 => Some(CHANNEL_LAYOUT_STEREO),
    _ => None,
  }
}

fn decode_frame(
  decoder: &mut Box<dyn AudioDecoder>,
  bytes: &[u8],
  timestamp: f64,
) -> Result<Pcm, DecodeError> {
  if bytes.is_empty() {
    return Err(DecodeError::Rejected);
  }
  let packet =
    Packet::new(0, Timestamp::ZERO, Duration::new(1024), bytes.to_vec());
  let decoded = decoder.decode(&packet).map_err(|_| DecodeError::Rejected)?;
  let channels = decoded.spec().channels().count();
  let mut samples = Vec::<f32>::new();
  decoded.copy_to_vec_interleaved(&mut samples);
  if channels == 0 || samples.is_empty() {
    return Err(DecodeError::Rejected);
  }
  let mut planar = vec![Vec::new(); channels];
  for (index, sample) in samples.into_iter().enumerate() {
    planar[index % channels].push(sample);
  }
  Ok(Pcm {
    rate: f64::from(decoded.spec().rate()),
    channels: planar,
    timestamp,
  })
}

/// One ADTS frame. `blocks` other than 0 is rejected: the WPT file stores
/// one raw frame per header.
fn split_adts(
  bytes: &[u8],
) -> Result<(Vec<u8>, Vec<u8>, u32, u32), DecodeError> {
  if bytes.len() < 7 || bytes[0] != 0xff || bytes[1] & 0xf0 != 0xf0 {
    return Err(DecodeError::Rejected);
  }
  let protection_absent = bytes[1] & 0x01 == 1;
  let profile = bytes[2] >> 6;
  let freq = (bytes[2] >> 2) & 0x0f;
  let channel = u32::from(((bytes[2] & 0x01) << 2) | (bytes[3] >> 6));
  let frame_length = (usize::from(bytes[3] & 0x03) << 11)
    | (usize::from(bytes[4]) << 3)
    | (usize::from(bytes[5]) >> 5);
  let blocks = bytes[6] & 0x03;
  let header = if protection_absent { 7 } else { 9 };
  if blocks != 0
    || frame_length < header
    || frame_length > bytes.len()
    || usize::from(freq) >= RATES.len()
  {
    return Err(DecodeError::Rejected);
  }
  let payload = bytes[header..frame_length].to_vec();
  if payload.is_empty() {
    return Err(DecodeError::Rejected);
  }
  let aot = u16::from(profile) + 1;
  let bits = (aot << 11) | (u16::from(freq) << 7) | ((channel as u16) << 3);
  let asc = vec![(bits >> 8) as u8, (bits & 0xff) as u8];
  Ok((asc, payload, RATES[usize::from(freq)], channel))
}
