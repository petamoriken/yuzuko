//! AAC, Opus, and Vorbis selection for `AudioDecoder`.

mod aac;
mod opus;
mod vorbis;

use crate::error::DecodeError;
use aac::AacDecoder;
use opus::OpusStream;
use vorbis::VorbisStream;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AudioCodec {
  Aac,
  Opus,
  Vorbis,
}

pub struct Pcm {
  pub rate: f64,
  pub channels: Vec<Vec<f32>>,
  pub timestamp: f64,
}

enum Inner {
  Aac(AacDecoder),
  Opus(Box<OpusStream>),
  Vorbis(Box<VorbisStream>),
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
      AudioCodec::Vorbis => Inner::Vorbis(Box::new(VorbisStream::open(
        description,
        sample_rate,
        channels,
      )?)),
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
      Inner::Vorbis(decoder) => decoder.decode(bytes, timestamp),
    }
  }

  /// Vorbis `flush` clears the overlap window. AAC and Opus keep their state.
  pub fn flush(&mut self) {
    if let Inner::Vorbis(decoder) = &mut self.inner {
      decoder.reset_overlap();
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

  fn ogg_packets(bytes: &[u8]) -> Vec<Vec<u8>> {
    let mut packets = Vec::new();
    let mut current = Vec::new();
    let mut offset = 0usize;
    while offset + 27 <= bytes.len() && &bytes[offset..offset + 4] == b"OggS" {
      if bytes[offset + 4] != 0 {
        break;
      }
      let segments = usize::from(bytes[offset + 26]);
      let table = offset + 27;
      let data = table + segments;
      if data > bytes.len() {
        break;
      }
      let mut cursor = data;
      for &lace in &bytes[table..data] {
        let lace = usize::from(lace);
        if cursor + lace > bytes.len() {
          return packets;
        }
        current.extend_from_slice(&bytes[cursor..cursor + lace]);
        cursor += lace;
        if lace < 255 {
          packets.push(std::mem::take(&mut current));
        }
      }
      offset = cursor;
    }
    if !current.is_empty() {
      packets.push(current);
    }
    packets
  }

  /// Lace every packet, including the last. The payload has no remainder.
  fn xiph_extradata(packets: &[Vec<u8>]) -> Vec<u8> {
    let mut laces = Vec::new();
    for packet in packets {
      let mut remaining = packet.len();
      loop {
        if remaining >= 255 {
          laces.push(255);
          remaining -= 255;
          continue;
        }
        laces.push(remaining as u8);
        break;
      }
    }
    assert!(laces.len() <= 255);
    let mut out = Vec::with_capacity(1 + laces.len());
    out.push(laces.len() as u8);
    out.extend_from_slice(&laces);
    for packet in packets {
      out.extend_from_slice(packet);
    }
    out
  }

  /// Lace every header but the last. The last header is the remaining bytes.
  fn xiph_extradata_implicit_last(packets: &[Vec<u8>]) -> Vec<u8> {
    assert!(packets.len() >= 2);
    let mut laces = Vec::new();
    for packet in &packets[..packets.len() - 1] {
      let mut remaining = packet.len();
      loop {
        if remaining >= 255 {
          laces.push(255);
          remaining -= 255;
          continue;
        }
        laces.push(remaining as u8);
        break;
      }
    }
    assert!(laces.len() <= 255);
    let mut out = Vec::with_capacity(1 + laces.len());
    out.push(laces.len() as u8);
    out.extend_from_slice(&laces);
    for packet in packets {
      out.extend_from_slice(packet);
    }
    out
  }

  #[test]
  fn vorbis_ogg_packet_decodes_from_xiph_headers() {
    let file = read("sfx-vorbis.ogg");
    let packets = ogg_packets(&file);
    assert!(packets.len() > 3, "{}", packets.len());
    assert_eq!(packets[0].first().copied(), Some(0x01));
    assert_eq!(packets[1].first().copied(), Some(0x03));
    assert_eq!(packets[2].first().copied(), Some(0x05));
    let rate = u32::from_le_bytes(packets[0][12..16].try_into().unwrap());
    let channels = packets[0][11];
    let description = xiph_extradata(&packets[..3]);
    let mut decoder =
      AudioBackend::open(AudioCodec::Vorbis, &description, 8_000, 2)
        .expect("open");
    let mut decoded = None;
    for packet in packets.iter().skip(3) {
      let pcm = decoder.decode(packet, 12.0).expect("decode");
      if let Some(pcm) = pcm
        && !pcm.channels.is_empty()
        && !pcm.channels[0].is_empty()
      {
        decoded = Some(pcm);
        break;
      }
    }
    let pcm = decoded.expect("pcm");
    assert_eq!(pcm.channels.len(), usize::from(channels));
    assert!(!pcm.channels[0].is_empty());
    assert_eq!(pcm.rate, f64::from(rate));
    assert_eq!(pcm.timestamp, 12.0);
    let implicit = xiph_extradata_implicit_last(&packets[..3]);
    assert!(implicit[0] < description[0]);
    let mut implicit_decoder =
      AudioBackend::open(AudioCodec::Vorbis, &implicit, 8_000, 2)
        .expect("implicit last header");
    let mut implicit_pcm = None;
    for packet in packets.iter().skip(3) {
      let pcm = implicit_decoder.decode(packet, 12.0).expect("decode");
      if let Some(pcm) = pcm
        && !pcm.channels.is_empty()
        && !pcm.channels[0].is_empty()
      {
        implicit_pcm = Some(pcm);
        break;
      }
    }
    let implicit_pcm = implicit_pcm.expect("implicit pcm");
    assert_eq!(implicit_pcm.channels.len(), pcm.channels.len());
    assert_eq!(implicit_pcm.channels[0].len(), pcm.channels[0].len());
    assert_eq!(implicit_pcm.timestamp, 12.0);
    assert!(
      pcm
        .channels
        .iter()
        .flatten()
        .all(|sample| sample.is_finite())
    );
    assert!(AudioBackend::open(AudioCodec::Vorbis, &[], rate, 1).is_err());
    let discard = (128.0_f64 / 48_000.0 * 1_000_000.0).round();
    let mut positive =
      AudioBackend::open(AudioCodec::Vorbis, &implicit, 1, 1).expect("open");
    assert!(positive.decode(&packets[3], 42.0).expect("prime").is_none());
    let first = positive
      .decode(&packets[4], discard + 42.0)
      .expect("first")
      .expect("pcm");
    assert_eq!(first.timestamp, 42.0);
    let mut saw_later = false;
    for (index, packet) in packets.iter().enumerate().skip(5) {
      let stamp = 90_000.0 + index as f64;
      let pcm = positive.decode(packet, stamp).expect("later");
      if let Some(pcm) = pcm
        && !pcm.channels.is_empty()
        && !pcm.channels[0].is_empty()
      {
        assert_eq!(pcm.timestamp, stamp);
        saw_later = true;
        break;
      }
    }
    assert!(saw_later);
    positive.flush();
    assert!(
      positive
        .decode(&packets[3], 7.0)
        .expect("reprime")
        .is_none()
    );
    let mut negative =
      AudioBackend::open(AudioCodec::Vorbis, &implicit, 1, 1).expect("open");
    assert!(
      negative
        .decode(&packets[3], -42.0)
        .expect("prime")
        .is_none()
    );
    let shifted = negative
      .decode(&packets[4], discard - 42.0)
      .expect("shifted")
      .expect("pcm");
    assert_eq!(shifted.timestamp, -42.0 + discard);
    assert!(
      AudioBackend::open(AudioCodec::Vorbis, &[1, 2, 3, 4], rate, 1).is_err()
    );
  }
}
