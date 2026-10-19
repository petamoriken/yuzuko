//! Pure-Rust decoders for WAV, FLAC, MP3, and Ogg Vorbis.
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
    decode_vorbis(bytes)
  } else {
    decode_mp3(bytes)
  }
}

fn decode_wav(bytes: &[u8]) -> Result<Pcm, ()> {
  let reader = hound::WavReader::new(Cursor::new(bytes)).map_err(|_| ())?;
  let spec = reader.spec();
  let channels = spec.channels as usize;
  if channels == 0 {
    return Err(());
  }
  let rate = spec.sample_rate as f64;
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
  if channels == 0 {
    return Err(());
  }
  let shift = info.bits_per_sample.saturating_sub(1).min(62);
  let scale = (1i64 << shift) as f32;
  let mut channels_pcm = vec![Vec::new(); channels];
  for (index, sample) in reader.samples().enumerate() {
    let sample = sample.map_err(|_| ())?;
    channels_pcm[index % channels].push(sample as f32 / scale);
  }
  Ok(Pcm {
    sample_rate: rate,
    channels: channels_pcm,
  })
}

fn decode_vorbis(bytes: &[u8]) -> Result<Pcm, ()> {
  let mut reader = lewton::inside_ogg::OggStreamReader::new(Cursor::new(bytes))
    .map_err(|_| ())?;
  let channels = reader.ident_hdr.audio_channels as usize;
  let rate = reader.ident_hdr.audio_sample_rate as f64;
  if channels == 0 {
    return Err(());
  }
  let mut channels_pcm = vec![Vec::new(); channels];
  while let Some(packet) = reader.read_dec_packet_itl().map_err(|_| ())? {
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
  use symphonia::core::audio::SampleBuffer;
  use symphonia::core::codecs::DecoderOptions;
  use symphonia::core::errors::Error;
  use symphonia::core::formats::FormatOptions;
  use symphonia::core::io::MediaSourceStream;
  use symphonia::core::meta::MetadataOptions;
  use symphonia::core::probe::Hint;

  let source = MediaSourceStream::new(
    Box::new(Cursor::new(bytes.to_vec())),
    Default::default(),
  );
  let probed = symphonia::default::get_probe()
    .format(
      &Hint::new(),
      source,
      &FormatOptions::default(),
      &MetadataOptions::default(),
    )
    .map_err(|_| ())?;
  let mut format = probed.format;
  let track = format.default_track().ok_or(())?;
  let track_id = track.id;
  let rate = track.codec_params.sample_rate.ok_or(())? as f64;
  let params = track.codec_params.clone();
  let mut decoder = symphonia::default::get_codecs()
    .make(&params, &DecoderOptions::default())
    .map_err(|_| ())?;
  let mut sample_buf: Option<SampleBuffer<f32>> = None;
  let mut channels = 0usize;
  let mut interleaved = Vec::new();
  loop {
    let packet = match format.next_packet() {
      Ok(packet) => packet,
      Err(Error::IoError(_)) | Err(Error::ResetRequired) => break,
      Err(_) => break,
    };
    if packet.track_id() != track_id {
      continue;
    }
    let decoded = match decoder.decode(&packet) {
      Ok(decoded) => decoded,
      Err(Error::DecodeError(_)) => continue,
      Err(_) => break,
    };
    if sample_buf.is_none() {
      channels = decoded.spec().channels.count();
      sample_buf = Some(SampleBuffer::<f32>::new(
        decoded.capacity() as u64,
        *decoded.spec(),
      ));
    }
    let buffer = sample_buf.as_mut().ok_or(())?;
    buffer.copy_interleaved_ref(decoded);
    interleaved.extend_from_slice(buffer.samples());
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
