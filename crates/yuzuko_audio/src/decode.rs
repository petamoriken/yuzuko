//! Pure-Rust decoders for WAV, FLAC, MP3, Ogg Vorbis, and Ogg Opus.
//!
//! A panic inside a decoder becomes `InvalidStateError` at the Web Audio
//! boundary. Bad bytes are an `EncodingError`.

use std::io::Cursor;
use std::panic::{AssertUnwindSafe, catch_unwind};

#[derive(Clone, Debug)]
pub struct Pcm {
  pub sample_rate: f64,
  pub channels: Vec<Vec<f32>>,
}

#[derive(Debug)]
pub enum DecodeFailure {
  Encoding,
  Panic,
}

/// The channel count `AudioBuffer` accepts.
const MAX_CHANNELS: usize = 32;
/// The frame count `AudioBuffer` accepts.
const MAX_FRAMES: usize = 20_000_000;
/// Samples across every channel. A small compressed file can describe far
/// more audio than this, so decoding stops here.
const MAX_SAMPLES: usize = 160_000_000;

/// The channel count and rate of `AudioBuffer`.
fn check_format(channels: usize, rate: f64) -> Result<(), ()> {
  if (1..=MAX_CHANNELS).contains(&channels)
    && (3000.0..=768000.0).contains(&rate)
  {
    Ok(())
  } else {
    Err(())
  }
}

/// Fails once a decoder has produced more samples than an `AudioBuffer`
/// may hold.
struct Budget {
  channels: usize,
  samples: usize,
}

impl Budget {
  fn new(channels: usize) -> Self {
    Self {
      channels,
      samples: 0,
    }
  }

  fn take(&mut self, samples: usize) -> Result<(), ()> {
    self.samples = self.samples.checked_add(samples).ok_or(())?;
    if self.samples > MAX_SAMPLES
      || self.samples / self.channels.max(1) > MAX_FRAMES
    {
      return Err(());
    }
    Ok(())
  }
}

pub fn decode(bytes: &[u8]) -> Result<Pcm, DecodeFailure> {
  let owned = bytes.to_vec();
  let decoded = catch_unwind(AssertUnwindSafe(|| decode_inner(&owned)));
  match decoded {
    Ok(Ok(pcm)) => {
      if pcm.channels.is_empty()
        || pcm.channels.iter().all(|channel| channel.is_empty())
        || !pcm.sample_rate.is_finite()
        || pcm.sample_rate <= 0.0
      {
        Err(DecodeFailure::Encoding)
      } else {
        Ok(pcm)
      }
    }
    Ok(Err(())) => Err(DecodeFailure::Encoding),
    Err(_) => Err(DecodeFailure::Panic),
  }
}

fn decode_inner(bytes: &[u8]) -> Result<Pcm, ()> {
  if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WAVE" {
    decode_wav(bytes)
  } else if bytes.starts_with(b"fLaC") {
    decode_flac(bytes)
  } else if bytes.starts_with(b"OggS") {
    if looks_like_opus(bytes) {
      decode_opus(bytes)
    } else {
      decode_vorbis(bytes)
    }
  } else {
    decode_mp3(bytes)
  }
}

fn decode_wav(bytes: &[u8]) -> Result<Pcm, ()> {
  let reader = hound::WavReader::new(Cursor::new(bytes)).map_err(|_| ())?;
  let spec = reader.spec();
  let channels = spec.channels as usize;
  let rate = spec.sample_rate as f64;
  check_format(channels, rate)?;
  Budget::new(channels).take(reader.len() as usize)?;
  let interleaved = match spec.sample_format {
    hound::SampleFormat::Float => reader
      .into_samples::<f32>()
      .collect::<Result<Vec<_>, _>>()
      .map_err(|_| ())?,
    hound::SampleFormat::Int => {
      let shift = u32::from(spec.bits_per_sample.saturating_sub(1).min(31));
      let scale = (1u32 << shift) as f32;
      reader
        .into_samples::<i32>()
        .map(|sample| sample.map(|sample| sample as f32 / scale))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| ())?
    }
  };
  Ok(deinterleave(interleaved, channels, rate))
}

fn decode_flac(bytes: &[u8]) -> Result<Pcm, ()> {
  let mut reader =
    claxon::FlacReader::new(Cursor::new(bytes)).map_err(|_| ())?;
  let info = reader.streaminfo();
  let channels = info.channels as usize;
  let rate = info.sample_rate as f64;
  check_format(channels, rate)?;
  let shift = info.bits_per_sample.saturating_sub(1).min(62);
  let scale = (1i64 << shift) as f32;
  let mut channels_pcm = vec![Vec::new(); channels];
  let mut budget = Budget::new(channels);
  for (index, sample) in reader.samples().enumerate() {
    let sample = sample.map_err(|_| ())?;
    budget.take(1)?;
    channels_pcm[index % channels].push(sample as f32 / scale);
  }
  Ok(Pcm {
    sample_rate: rate,
    channels: channels_pcm,
  })
}

fn looks_like_opus(bytes: &[u8]) -> bool {
  let head = &bytes[..bytes.len().min(512)];
  head.windows(8).any(|window| window == b"OpusHead")
}

fn decode_opus(bytes: &[u8]) -> Result<Pcm, ()> {
  opus_fits(bytes)?;
  let (interleaved, head) = ruopus::decode_ogg_opus(bytes).map_err(|_| ())?;
  let channels = usize::from(head.channel_count);
  check_format(channels, 48_000.0)?;
  // Opus playback is 48 kHz. `input_sample_rate` is original-file metadata.
  Ok(deinterleave(interleaved, channels, 48_000.0))
}

/// `decode_ogg_opus` decodes the whole stream at once. The packet headers
/// give each packet's duration, so the output size is checked first.
fn opus_fits(bytes: &[u8]) -> Result<(), ()> {
  let mut reader = ruopus::ogg::OggOpusReader::new(bytes).map_err(|_| ())?;
  let channels = usize::from(reader.head().channel_count);
  check_format(channels, 48_000.0)?;
  let mut budget = Budget::new(channels);
  while let Some(packet) = reader.next() {
    let Ok(parsed) = ruopus::Packet::parse(&packet.data) else {
      continue;
    };
    let frames = parsed.duration().as_micros() * 48 / 1000;
    let samples = usize::try_from(frames).map_err(|_| ())?;
    budget.take(samples.checked_mul(channels).ok_or(())?)?;
  }
  Ok(())
}

fn decode_vorbis(bytes: &[u8]) -> Result<Pcm, ()> {
  let mut reader = lewton::inside_ogg::OggStreamReader::new(Cursor::new(bytes))
    .map_err(|_| ())?;
  let channels = reader.ident_hdr.audio_channels as usize;
  let rate = reader.ident_hdr.audio_sample_rate as f64;
  check_format(channels, rate)?;
  let mut channels_pcm = vec![Vec::new(); channels];
  let mut budget = Budget::new(channels);
  while let Some(packet) = reader.read_dec_packet_itl().map_err(|_| ())? {
    budget.take(packet.len())?;
    for (index, sample) in packet.into_iter().enumerate() {
      channels_pcm[index % channels].push(sample as f32 / 32768.0);
    }
  }
  Ok(Pcm {
    sample_rate: rate,
    channels: channels_pcm,
  })
}

fn decode_mp3(bytes: &[u8]) -> Result<Pcm, ()> {
  use symphonia::core::codecs::audio::AudioDecoderOptions;
  use symphonia::core::errors::Error;
  use symphonia::core::formats::FormatOptions;
  use symphonia::core::formats::TrackType;
  use symphonia::core::formats::probe::Hint;
  use symphonia::core::io::MediaSourceStream;
  use symphonia::core::meta::MetadataOptions;

  let source = MediaSourceStream::new(
    Box::new(Cursor::new(bytes.to_vec())),
    Default::default(),
  );
  let mut format = symphonia::default::get_probe()
    .probe(
      &Hint::new(),
      source,
      FormatOptions::default(),
      MetadataOptions::default(),
    )
    .map_err(|_| ())?;
  let track = format.default_track(TrackType::Audio).ok_or(())?;
  let track_id = track.id;
  let audio = track
    .codec_params
    .as_ref()
    .and_then(|params| params.audio())
    .ok_or(())?;
  let rate = audio.sample_rate.ok_or(())? as f64;
  check_format(1, rate)?;
  let params = audio.clone();
  let mut decoder = symphonia::default::get_codecs()
    .make_audio_decoder(&params, &AudioDecoderOptions::default())
    .map_err(|_| ())?;
  let mut channels = 0usize;
  let mut interleaved = Vec::new();
  let mut budget = Budget::new(1);
  while let Ok(Some(packet)) = format.next_packet() {
    if packet.track_id != track_id {
      continue;
    }
    let decoded = match decoder.decode(&packet) {
      Ok(decoded) => decoded,
      Err(Error::DecodeError(_)) => continue,
      Err(_) => break,
    };
    if channels == 0 {
      channels = decoded.spec().channels().count();
      check_format(channels, rate)?;
      budget = Budget::new(channels);
    }
    let mut samples = Vec::<f32>::new();
    decoded.copy_to_vec_interleaved(&mut samples);
    budget.take(samples.len())?;
    interleaved.extend(samples);
  }
  if channels == 0 {
    return Err(());
  }
  Ok(deinterleave(interleaved, channels, rate))
}

fn deinterleave(interleaved: Vec<f32>, channels: usize, rate: f64) -> Pcm {
  let mut planar = vec![Vec::new(); channels];
  for (index, sample) in interleaved.into_iter().enumerate() {
    planar[index % channels].push(sample);
  }
  Pcm {
    sample_rate: rate,
    channels: planar,
  }
}

/// One sample of a sine at `frequency` Hz. `frame` is the output index.
#[cfg(test)]
fn sine_sample(frequency: f64, sample_rate: f64, frame: u64) -> f32 {
  let omega = std::f64::consts::TAU * frequency / sample_rate;
  (omega * frame as f64).sin() as f32
}

#[cfg(test)]
mod tests {
  use super::{decode, sine_sample};

  /// A 16-bit PCM WAV header that claims `data_len` bytes of samples,
  /// followed by `body`.
  fn wav(channels: u16, rate: u32, data_len: u32, body: &[u8]) -> Vec<u8> {
    let block = channels.saturating_mul(2);
    let mut out = Vec::new();
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&channels.to_le_bytes());
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * u32::from(block)).to_le_bytes());
    out.extend_from_slice(&block.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    out.extend_from_slice(body);
    out
  }

  #[test]
  fn a_format_audio_buffer_rejects_is_an_encoding_error() {
    let ok = wav(2, 44100, 8, &[0; 8]);
    assert!(decode(&ok).is_ok());
    for (label, bytes) in [
      ("33 channels", wav(33, 44100, 66, &[0; 66])),
      ("1 Hz", wav(1, 1, 2, &[0; 2])),
      ("800 kHz", wav(1, 800_000, 2, &[0; 2])),
      ("too many frames", wav(1, 44100, 50_000_000, &[0; 16])),
    ] {
      assert!(
        matches!(decode(&bytes), Err(super::DecodeFailure::Encoding)),
        "{label}"
      );
    }
  }

  #[test]
  fn the_budget_stops_at_the_audio_buffer_limits() {
    let mut budget = super::Budget::new(1);
    assert!(budget.take(super::MAX_FRAMES).is_ok());
    assert!(budget.take(1).is_err());
    let mut budget = super::Budget::new(32);
    assert!(budget.take(super::MAX_SAMPLES).is_ok());
    assert!(budget.take(1).is_err());
  }

  fn fixture(name: &str) -> Vec<u8> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
      .join("../../tests/fixtures/audio")
      .join(name);
    std::fs::read(path).expect(name)
  }

  fn assert_tone(name: &str) {
    let pcm = decode(&fixture(name)).expect(name);
    assert!(
      (pcm.sample_rate - 44100.0).abs() < 1.0,
      "{name} rate {}",
      pcm.sample_rate
    );
    assert!(!pcm.channels.is_empty(), "{name} channels");
    let peak = pcm.channels[0]
      .iter()
      .fold(0.0f32, |peak, sample| peak.max(sample.abs()));
    assert!(peak > 0.05, "{name} peak {peak}");
  }

  #[test]
  fn fixtures_decode_to_a_tone() {
    assert_tone("tone.wav");
    assert_tone("tone.flac");
    assert_tone("tone.mp3");
    assert_tone("tone.ogg");
    assert_opus_sine();
  }

  fn assert_opus_sine() {
    let bytes = fixture("tone.opus");
    assert_opus_pcm(&decode(&bytes).expect("tone.opus"));
    // `OpusHead.input_sample_rate` is original-file metadata.
    let mut retagged = bytes;
    stamp_opus_input_rate(&mut retagged, 44_100);
    let retagged = decode(&retagged).expect("retagged opus");
    assert!(
      (retagged.sample_rate - 48_000.0).abs() < 1.0,
      "retagged rate {}",
      retagged.sample_rate
    );
    assert_eq!(retagged.channels.len(), 1, "retagged channels");
  }

  fn assert_opus_pcm(pcm: &super::Pcm) {
    assert!(
      (pcm.sample_rate - 48_000.0).abs() < 1.0,
      "opus rate {}",
      pcm.sample_rate
    );
    assert_eq!(pcm.channels.len(), 1, "opus channels");
    let channel = &pcm.channels[0];
    // 100 ms at 48 kHz is 4800 frames. This 96 kb/s stream records a
    // 120-sample pre-skip and does not drain another frame after it.
    assert_eq!(channel.len(), 4_680, "opus length {}", channel.len());
    for (frame, sample) in channel.iter().enumerate() {
      let expected = sine_sample(440.0, 48_000.0, frame as u64) * 0.5;
      let delta = (sample - expected).abs();
      assert!(
        delta < 0.02,
        "opus frame {frame} got {sample} expected {expected}"
      );
    }
  }

  /// Writes `rate` into the identification header and repairs that page's CRC.
  fn stamp_opus_input_rate(bytes: &mut [u8], rate: u32) {
    assert!(bytes.starts_with(b"OggS"), "opus page");
    let head = bytes
      .windows(8)
      .position(|window| window == b"OpusHead")
      .expect("OpusHead");
    bytes[head + 12..head + 16].copy_from_slice(&rate.to_le_bytes());
    let segments = usize::from(bytes[26]);
    let body = 27 + segments;
    let payload: usize =
      bytes[27..body].iter().map(|seg| usize::from(*seg)).sum();
    let total = body + payload;
    let crc = ogg_crc(&bytes[..22], &bytes[26..total]);
    bytes[22..26].copy_from_slice(&crc.to_le_bytes());
  }

  fn ogg_crc(prefix: &[u8], rest: &[u8]) -> u32 {
    const POLY: u32 = 0x04C1_1DB7;
    let mut table = [0u32; 256];
    for (index, slot) in table.iter_mut().enumerate() {
      let mut value = (index as u32) << 24;
      for _ in 0..8 {
        value = if value & 0x8000_0000 != 0 {
          (value << 1) ^ POLY
        } else {
          value << 1
        };
      }
      *slot = value;
    }
    fn update(table: &[u32; 256], mut crc: u32, data: &[u8]) -> u32 {
      for byte in data {
        crc = (crc << 8) ^ table[((crc >> 24) as u8 ^ byte) as usize];
      }
      crc
    }
    update(
      &table,
      update(&table, update(&table, 0, prefix), &[0, 0, 0, 0]),
      rest,
    )
  }

  #[test]
  fn sine_is_periodic_and_not_silent() {
    let mut samples = Vec::new();
    for frame in 0..200 {
      samples.push(sine_sample(480.0, 48000.0, frame));
    }
    assert!(samples.iter().any(|sample| sample.abs() > 0.5));
    for index in 0..100 {
      let delta = (samples[index] - samples[index + 100]).abs();
      assert!(delta < 1.0e-4, "frame {index} delta {delta}");
    }
  }
}
