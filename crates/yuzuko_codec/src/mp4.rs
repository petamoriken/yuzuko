//! Progressive MP4 samples for `Yuzuko.demuxMp4`.
//!
//! `moov` sample tables locate `avc1`, `avc3`, and `mp4a` samples. The sample
//! bytes are copied as stored. Fragmented movies are rejected. One edit-list
//! entry with a media rate of 1 shifts timestamps.

const MAX_SAMPLES: u32 = 2_000_000;

const EMPTY: Error = Error {
  message: "The MP4 data is empty",
};
const INVALID: Error = Error {
  message: "The MP4 file is not a valid container",
};
const NO_MOVIE: Error = Error {
  message: "The MP4 file has no movie",
};
const NO_TRACK: Error = Error {
  message: "The MP4 file has no H.264 or AAC track",
};
const FRAGMENTED: Error = Error {
  message: "Fragmented MP4 is not supported",
};
const OUTSIDE: Error = Error {
  message: "The MP4 sample is outside the file",
};
const CHANGES: Error = Error {
  message: "The MP4 track changes sample entries",
};
const ENCRYPTED: Error = Error {
  message: "Encrypted MP4 is not supported",
};
const NO_AVCC: Error = Error {
  message: "The H.264 track has no avcC",
};
const NO_ASC: Error = Error {
  message: "The AAC track has no AudioSpecificConfig",
};
const TOO_MANY: Error = Error {
  message: "The MP4 track has too many samples",
};

const AAC_RATES: [u32; 13] = [
  96000, 88200, 64000, 48000, 44100, 32000, 24000, 22050, 16000, 12000, 11025,
  8000, 7350,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Error {
  message: &'static str,
}

impl Error {
  pub(crate) fn message(self) -> &'static str {
    self.message
  }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Movie {
  pub(crate) video: Vec<VideoTrack>,
  pub(crate) audio: Vec<AudioTrack>,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct VideoTrack {
  pub(crate) codec: String,
  pub(crate) description: Vec<u8>,
  pub(crate) coded_width: u32,
  pub(crate) coded_height: u32,
  pub(crate) samples: Vec<Sample>,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct AudioTrack {
  pub(crate) codec: String,
  pub(crate) description: Vec<u8>,
  pub(crate) sample_rate: u32,
  pub(crate) channels: u32,
  pub(crate) samples: Vec<Sample>,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Sample {
  pub(crate) key: bool,
  pub(crate) timestamp: i64,
  pub(crate) duration: i64,
  pub(crate) data: Vec<u8>,
}

pub(crate) fn parse(bytes: &[u8]) -> Result<Movie, Error> {
  if bytes.is_empty() {
    return Err(EMPTY);
  }
  let mut moov = None;
  let mut moof = false;
  let mut offset = 0;
  while offset < bytes.len() {
    let raw = read_box(bytes, offset)?;
    match &raw.kind {
      b"moov" => {
        if moov.is_some() {
          return Err(INVALID);
        }
        moov = Some(raw.payload);
      }
      b"moof" => moof = true,
      _ => {}
    }
    offset = raw.next;
  }
  let Some(moov) = moov else {
    if moof {
      return Err(FRAGMENTED);
    }
    return Err(NO_MOVIE);
  };
  let movie = parse_moov(bytes, moov, moof)?;
  let has_samples = movie.video.iter().any(|track| !track.samples.is_empty())
    || movie.audio.iter().any(|track| !track.samples.is_empty());
  if !has_samples && moof {
    return Err(FRAGMENTED);
  }
  if movie.video.is_empty() && movie.audio.is_empty() {
    return Err(NO_TRACK);
  }
  Ok(movie)
}

struct RawBox<'a> {
  kind: [u8; 4],
  payload: &'a [u8],
  next: usize,
}

fn read_box(data: &[u8], start: usize) -> Result<RawBox<'_>, Error> {
  let rest = data.get(start..).ok_or(INVALID)?;
  if rest.len() < 8 {
    return Err(INVALID);
  }
  let size32 = u32::from_be_bytes([rest[0], rest[1], rest[2], rest[3]]);
  let kind = [rest[4], rest[5], rest[6], rest[7]];
  // Size 1 is a 64-bit largesize. Size 0 runs to the end of the parent.
  let (header, size) = if size32 == 1 {
    if rest.len() < 16 {
      return Err(INVALID);
    }
    let large = u64::from_be_bytes([
      rest[8], rest[9], rest[10], rest[11], rest[12], rest[13], rest[14],
      rest[15],
    ]);
    (16usize, large)
  } else if size32 == 0 {
    (8usize, rest.len() as u64)
  } else {
    (8usize, u64::from(size32))
  };
  if size < header as u64 {
    return Err(INVALID);
  }
  let size = usize::try_from(size).map_err(|_| INVALID)?;
  let end = start.checked_add(size).ok_or(INVALID)?;
  if end > data.len() {
    return Err(INVALID);
  }
  let payload = data.get(start + header..end).ok_or(INVALID)?;
  Ok(RawBox {
    kind,
    payload,
    next: end,
  })
}

fn children(data: &[u8]) -> Result<Vec<([u8; 4], &[u8])>, Error> {
  let mut boxes = Vec::new();
  let mut offset = 0;
  while offset < data.len() {
    let raw = read_box(data, offset)?;
    if raw.next <= offset {
      return Err(INVALID);
    }
    boxes.push((raw.kind, raw.payload));
    offset = raw.next;
  }
  Ok(boxes)
}

fn only<'a>(
  boxes: &[([u8; 4], &'a [u8])],
  kind: &[u8; 4],
) -> Result<Option<&'a [u8]>, Error> {
  let mut found = None;
  for (key, payload) in boxes {
    if key == kind {
      if found.is_some() {
        return Err(INVALID);
      }
      found = Some(*payload);
    }
  }
  Ok(found)
}

fn full_box(payload: &[u8]) -> Result<(u8, &[u8]), Error> {
  if payload.len() < 4 {
    return Err(INVALID);
  }
  Ok((payload[0], &payload[4..]))
}

struct Cursor<'a> {
  data: &'a [u8],
  pos: usize,
}

impl<'a> Cursor<'a> {
  fn new(data: &'a [u8]) -> Self {
    Self { data, pos: 0 }
  }

  fn remaining(&self) -> &'a [u8] {
    &self.data[self.pos..]
  }

  fn take(&mut self, size: usize) -> Result<&'a [u8], Error> {
    let end = self.pos.checked_add(size).ok_or(INVALID)?;
    let slice = self.data.get(self.pos..end).ok_or(INVALID)?;
    self.pos = end;
    Ok(slice)
  }

  fn skip(&mut self, size: usize) -> Result<(), Error> {
    self.take(size).map(|_| ())
  }

  fn u16(&mut self) -> Result<u16, Error> {
    let bytes = self.take(2)?;
    Ok(u16::from_be_bytes([bytes[0], bytes[1]]))
  }

  fn u32(&mut self) -> Result<u32, Error> {
    let bytes = self.take(4)?;
    Ok(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
  }

  fn i32(&mut self) -> Result<i32, Error> {
    let bytes = self.take(4)?;
    Ok(i32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
  }

  fn u64(&mut self) -> Result<u64, Error> {
    let bytes = self.take(8)?;
    Ok(u64::from_be_bytes([
      bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6],
      bytes[7],
    ]))
  }

  fn i64(&mut self) -> Result<i64, Error> {
    Ok(self.u64()? as i64)
  }
}

fn parse_moov(file: &[u8], moov: &[u8], moof: bool) -> Result<Movie, Error> {
  let boxes = children(moov)?;
  let mut video = Vec::new();
  let mut audio = Vec::new();
  for (kind, payload) in boxes {
    if kind != *b"trak" {
      continue;
    }
    match parse_trak(file, payload, moof)? {
      Some(Track::Video(track)) => video.push(track),
      Some(Track::Audio(track)) => audio.push(track),
      None => {}
    }
  }
  Ok(Movie { video, audio })
}

enum Track {
  Video(VideoTrack),
  Audio(AudioTrack),
}

fn parse_trak(
  file: &[u8],
  trak: &[u8],
  moof: bool,
) -> Result<Option<Track>, Error> {
  let boxes = children(trak)?;
  let edit = match only(&boxes, b"edts")? {
    Some(edts) => parse_edit(edts)?,
    None => 0,
  };
  let Some(mdia) = only(&boxes, b"mdia")? else {
    return Ok(None);
  };
  let boxes = children(mdia)?;
  let Some(hdlr) = only(&boxes, b"hdlr")? else {
    return Ok(None);
  };
  let handler = handler_of(hdlr)?;
  if handler != *b"vide" && handler != *b"soun" {
    return Ok(None);
  }
  let Some(mdhd) = only(&boxes, b"mdhd")? else {
    return Err(INVALID);
  };
  let timescale = timescale_of(mdhd)?;
  let Some(minf) = only(&boxes, b"minf")? else {
    return Err(INVALID);
  };
  let minf = children(minf)?;
  let Some(stbl) = only(&minf, b"stbl")? else {
    return Err(INVALID);
  };
  let stbl = children(stbl)?;
  let Some(stsd) = only(&stbl, b"stsd")? else {
    return Err(INVALID);
  };
  let Some((desc, entry)) = parse_stsd(stsd, handler)? else {
    return Ok(None);
  };
  let tables = parse_tables(&stbl, moof)?;
  materialize(file, desc, entry, timescale, edit, tables)
}

fn handler_of(payload: &[u8]) -> Result<[u8; 4], Error> {
  let (version, body) = full_box(payload)?;
  if version > 1 || body.len() < 8 {
    return Err(INVALID);
  }
  Ok([body[4], body[5], body[6], body[7]])
}

fn timescale_of(payload: &[u8]) -> Result<u32, Error> {
  let (version, body) = full_box(payload)?;
  let mut cursor = Cursor::new(body);
  let scale = if version == 0 {
    cursor.skip(8)?;
    cursor.u32()?
  } else if version == 1 {
    cursor.skip(16)?;
    cursor.u32()?
  } else {
    return Err(INVALID);
  };
  if scale == 0 {
    return Err(INVALID);
  }
  Ok(scale)
}

fn parse_edit(edts: &[u8]) -> Result<i64, Error> {
  let boxes = children(edts)?;
  let Some(elst) = only(&boxes, b"elst")? else {
    return Ok(0);
  };
  let (version, body) = full_box(elst)?;
  let mut cursor = Cursor::new(body);
  let count = cursor.u32()?;
  if count != 1 {
    return Ok(0);
  }
  let (media_time, rate, fraction) = if version == 0 {
    let _duration = cursor.u32()?;
    let media = cursor.u32()?;
    let rate = cursor.u16()?;
    let fraction = cursor.u16()?;
    if media == u32::MAX {
      return Ok(0);
    }
    (i64::from(media), rate, fraction)
  } else if version == 1 {
    let _duration = cursor.u64()?;
    let media = cursor.i64()?;
    let rate = cursor.u16()?;
    let fraction = cursor.u16()?;
    if media < 0 {
      return Ok(0);
    }
    (media, rate, fraction)
  } else {
    return Err(INVALID);
  };
  if rate != 1 || fraction != 0 {
    return Ok(0);
  }
  Ok(media_time)
}

enum Entry {
  Video(VideoEntry),
  Audio(AudioEntry),
}

struct VideoEntry {
  codec: String,
  description: Vec<u8>,
  width: u32,
  height: u32,
}

struct AudioEntry {
  codec: String,
  description: Vec<u8>,
  sample_rate: u32,
  channels: u32,
}

fn parse_stsd(
  payload: &[u8],
  handler: [u8; 4],
) -> Result<Option<(u32, Entry)>, Error> {
  let (version, body) = full_box(payload)?;
  if version != 0 {
    return Err(INVALID);
  }
  let mut cursor = Cursor::new(body);
  let count = cursor.u32()?;
  if count > 64 {
    return Err(INVALID);
  }
  let entries = children(cursor.remaining())?;
  if entries.len() != count as usize {
    return Err(INVALID);
  }
  let mut chosen = None;
  for (index, (kind, payload)) in entries.iter().enumerate() {
    let desc = index as u32 + 1;
    let parsed = match (&handler, kind) {
      (b"vide", b"avc1" | b"avc3") => {
        Some(Entry::Video(parse_visual(payload, kind)?))
      }
      (b"soun", b"mp4a") => Some(Entry::Audio(parse_audio(payload)?)),
      _ => None,
    };
    if let Some(parsed) = parsed
      && chosen.is_none()
    {
      chosen = Some((desc, parsed));
    }
  }
  Ok(chosen)
}

fn parse_visual(payload: &[u8], kind: &[u8; 4]) -> Result<VideoEntry, Error> {
  if payload.len() < 78 {
    return Err(INVALID);
  }
  let width = u32::from(u16::from_be_bytes([payload[24], payload[25]]));
  let height = u32::from(u16::from_be_bytes([payload[26], payload[27]]));
  let boxes = children(&payload[78..])?;
  if boxes.iter().any(|(key, _)| key == b"sinf") {
    return Err(ENCRYPTED);
  }
  let Some(record) = boxes
    .iter()
    .find(|(key, _)| key == b"avcC")
    .map(|(_, p)| *p)
  else {
    return Err(NO_AVCC);
  };
  if record.len() < 4 || record[0] != 1 {
    return Err(INVALID);
  }
  let name = if kind == b"avc1" { "avc1" } else { "avc3" };
  let codec =
    format!("{name}.{:02X}{:02X}{:02X}", record[1], record[2], record[3]);
  Ok(VideoEntry {
    codec,
    description: record.to_vec(),
    width,
    height,
  })
}

fn parse_audio(payload: &[u8]) -> Result<AudioEntry, Error> {
  if payload.len() < 28 {
    return Err(INVALID);
  }
  let version = u16::from_be_bytes([payload[8], payload[9]]);
  let entry_channels =
    u32::from(u16::from_be_bytes([payload[16], payload[17]]));
  let entry_rate =
    u32::from_be_bytes([payload[24], payload[25], payload[26], payload[27]])
      >> 16;
  let child_at = if version == 1 { 44 } else { 28 };
  if payload.len() < child_at {
    return Err(INVALID);
  }
  let boxes = children(&payload[child_at..])?;
  let esds = find_esds(&boxes)?;
  let (object_type, specific) = parse_esds(esds)?;
  let (audio_object, asc_rate, asc_channels) = parse_asc(&specific)?;
  let sample_rate = if asc_rate != 0 { asc_rate } else { entry_rate };
  if sample_rate == 0 {
    return Err(INVALID);
  }
  let channels = asc_channels.unwrap_or(entry_channels);
  let codec = format!("mp4a.{object_type:02x}.{audio_object}");
  Ok(AudioEntry {
    codec,
    description: specific,
    sample_rate,
    channels,
  })
}

fn find_esds<'a>(boxes: &[([u8; 4], &'a [u8])]) -> Result<&'a [u8], Error> {
  let mut esds = None;
  for (kind, payload) in boxes {
    if kind == b"sinf" {
      return Err(ENCRYPTED);
    }
    if kind == b"esds" && esds.is_none() {
      esds = Some(*payload);
    }
    if kind == b"wave" {
      let inner = children(payload)?;
      if inner.iter().any(|(key, _)| key == b"sinf") {
        return Err(ENCRYPTED);
      }
      if esds.is_none()
        && let Some((_, found)) = inner.iter().find(|(key, _)| key == b"esds")
      {
        esds = Some(*found);
      }
    }
  }
  esds.ok_or(NO_ASC)
}

struct Descriptor<'a> {
  tag: u8,
  content: &'a [u8],
  total: usize,
}

fn descriptor(data: &[u8]) -> Result<Descriptor<'_>, Error> {
  if data.is_empty() {
    return Err(INVALID);
  }
  let tag = data[0];
  let mut size = 0usize;
  let mut index = 1;
  for round in 0..4 {
    let byte = *data.get(index).ok_or(INVALID)?;
    index += 1;
    size = (size << 7) | usize::from(byte & 0x7f);
    if byte & 0x80 == 0 {
      break;
    }
    if round == 3 {
      return Err(INVALID);
    }
  }
  let end = index.checked_add(size).ok_or(INVALID)?;
  if end > data.len() {
    return Err(INVALID);
  }
  Ok(Descriptor {
    tag,
    content: &data[index..end],
    total: end,
  })
}

fn parse_esds(payload: &[u8]) -> Result<(u8, Vec<u8>), Error> {
  let body = if payload.first() == Some(&0x03) {
    payload
  } else {
    payload.get(4..).ok_or(INVALID)?
  };
  let es = descriptor(body)?;
  if es.tag != 0x03 || es.content.len() < 3 {
    return Err(INVALID);
  }
  let mut index = 3usize;
  let flags = es.content[2];
  if flags & 0x80 != 0 {
    index = index.checked_add(2).ok_or(INVALID)?;
  }
  if flags & 0x40 != 0 {
    let length = usize::from(*es.content.get(index).ok_or(INVALID)?);
    index = index.checked_add(1 + length).ok_or(INVALID)?;
  }
  if flags & 0x20 != 0 {
    index = index.checked_add(2).ok_or(INVALID)?;
  }
  let rest = es.content.get(index..).ok_or(INVALID)?;
  let decoder = descriptor(rest)?;
  if decoder.tag != 0x04 || decoder.content.len() < 13 {
    return Err(INVALID);
  }
  let object_type = decoder.content[0];
  let mut at = 13usize;
  let mut specific = None;
  while at < decoder.content.len() {
    let inner = descriptor(&decoder.content[at..])?;
    if inner.tag == 0x05 && specific.is_none() {
      specific = Some(inner.content.to_vec());
    }
    at = at.checked_add(inner.total).ok_or(INVALID)?;
  }
  let specific = specific.filter(|bytes| !bytes.is_empty()).ok_or(NO_ASC)?;
  Ok((object_type, specific))
}

struct Bits<'a> {
  data: &'a [u8],
  pos: usize,
}

impl<'a> Bits<'a> {
  fn take(&mut self, count: usize) -> Result<u32, Error> {
    if count > 24 {
      return Err(INVALID);
    }
    let mut value = 0u32;
    for _ in 0..count {
      let byte = *self.data.get(self.pos / 8).ok_or(INVALID)?;
      let shift = 7 - (self.pos % 8);
      value = (value << 1) | u32::from((byte >> shift) & 1);
      self.pos += 1;
    }
    Ok(value)
  }
}

fn parse_asc(data: &[u8]) -> Result<(u32, u32, Option<u32>), Error> {
  let mut bits = Bits { data, pos: 0 };
  let mut audio_object = bits.take(5)?;
  if audio_object == 31 {
    audio_object = bits.take(6)? + 32;
  }
  let index = bits.take(4)?;
  let rate = if index == 15 {
    bits.take(24)?
  } else {
    *AAC_RATES.get(index as usize).ok_or(INVALID)?
  };
  let channels = match bits.take(4)? {
    index @ 1..=6 => Some(index),
    7 => Some(8),
    _ => None,
  };
  Ok((audio_object, rate, channels))
}

struct Tables {
  sizes: Option<Sizes>,
  offsets: Option<Vec<u64>>,
  stts: Option<Vec<Run>>,
  stsc: Option<Vec<Stsc>>,
  stss: Option<Vec<u32>>,
  ctts: Option<Vec<Run>>,
  moof: bool,
}

#[derive(Debug)]
enum Sizes {
  Constant { size: u32, count: u32 },
  Table(Vec<u32>),
}

impl Sizes {
  fn count(&self) -> Result<u32, Error> {
    match self {
      Self::Constant { count, .. } => Ok(*count),
      Self::Table(sizes) => u32::try_from(sizes.len()).map_err(|_| INVALID),
    }
  }

  fn at(&self, index: usize) -> Result<u32, Error> {
    match self {
      Self::Constant { size, count } if index < *count as usize => Ok(*size),
      Self::Table(sizes) => sizes.get(index).copied().ok_or(INVALID),
      _ => Err(INVALID),
    }
  }
}

struct Run {
  count: u32,
  value: i64,
}

struct Stsc {
  first: u32,
  per: u32,
  desc: u32,
}

fn capped(count: u32) -> Result<u32, Error> {
  if count > MAX_SAMPLES {
    Err(TOO_MANY)
  } else {
    Ok(count)
  }
}

fn parse_tables(
  stbl: &[([u8; 4], &[u8])],
  moof: bool,
) -> Result<Tables, Error> {
  let stsz = only(stbl, b"stsz")?;
  let stz2 = only(stbl, b"stz2")?;
  let sizes = match (stsz, stz2) {
    (Some(_), Some(_)) => return Err(INVALID),
    (Some(payload), None) => Some(parse_stsz(payload)?),
    (None, Some(payload)) => Some(parse_stz2(payload)?),
    (None, None) => None,
  };
  let stco = only(stbl, b"stco")?;
  let co64 = only(stbl, b"co64")?;
  let offsets = match (stco, co64) {
    (Some(_), Some(_)) => return Err(INVALID),
    (Some(payload), None) => Some(parse_offsets(payload, false)?),
    (None, Some(payload)) => Some(parse_offsets(payload, true)?),
    (None, None) => None,
  };
  let stts = only(stbl, b"stts")?
    .map(|payload| parse_runs(payload, false))
    .transpose()?;
  let stsc = only(stbl, b"stsc")?.map(parse_stsc).transpose()?;
  let stss = only(stbl, b"stss")?.map(parse_sync).transpose()?;
  let ctts = only(stbl, b"ctts")?
    .map(|payload| parse_runs(payload, true))
    .transpose()?;
  Ok(Tables {
    sizes,
    offsets,
    stts,
    stsc,
    stss,
    ctts,
    moof,
  })
}

fn parse_stsz(payload: &[u8]) -> Result<Sizes, Error> {
  let (version, body) = full_box(payload)?;
  if version != 0 {
    return Err(INVALID);
  }
  let mut cursor = Cursor::new(body);
  let sample_size = cursor.u32()?;
  let count = capped(cursor.u32()?)?;
  if sample_size != 0 {
    if !cursor.remaining().is_empty() {
      return Err(INVALID);
    }
    return Ok(Sizes::Constant {
      size: sample_size,
      count,
    });
  }
  let mut sizes = Vec::with_capacity(count as usize);
  for _ in 0..count {
    sizes.push(cursor.u32()?);
  }
  if !cursor.remaining().is_empty() {
    return Err(INVALID);
  }
  Ok(Sizes::Table(sizes))
}

fn parse_stz2(payload: &[u8]) -> Result<Sizes, Error> {
  let (version, body) = full_box(payload)?;
  if version != 0 {
    return Err(INVALID);
  }
  let mut cursor = Cursor::new(body);
  cursor.skip(3)?;
  let field = cursor.take(1)?[0];
  let count = capped(cursor.u32()?)?;
  let mut sizes = Vec::with_capacity(count as usize);
  match field {
    4 => {
      let bytes = cursor.take((count as usize).div_ceil(2))?;
      for index in 0..count as usize {
        let byte = bytes[index / 2];
        let nibble = if index % 2 == 0 {
          byte >> 4
        } else {
          byte & 0x0f
        };
        sizes.push(u32::from(nibble));
      }
    }
    8 => {
      for _ in 0..count {
        sizes.push(u32::from(cursor.take(1)?[0]));
      }
    }
    16 => {
      for _ in 0..count {
        sizes.push(u32::from(cursor.u16()?));
      }
    }
    _ => return Err(INVALID),
  }
  if !cursor.remaining().is_empty() {
    return Err(INVALID);
  }
  Ok(Sizes::Table(sizes))
}

fn parse_offsets(payload: &[u8], wide: bool) -> Result<Vec<u64>, Error> {
  let (version, body) = full_box(payload)?;
  if version != 0 {
    return Err(INVALID);
  }
  let mut cursor = Cursor::new(body);
  let count = capped(cursor.u32()?)?;
  let mut offsets = Vec::with_capacity(count as usize);
  for _ in 0..count {
    let offset = if wide {
      cursor.u64()?
    } else {
      u64::from(cursor.u32()?)
    };
    offsets.push(offset);
  }
  if !cursor.remaining().is_empty() {
    return Err(INVALID);
  }
  Ok(offsets)
}

fn parse_runs(payload: &[u8], composition: bool) -> Result<Vec<Run>, Error> {
  let (version, body) = full_box(payload)?;
  if composition {
    if version > 1 {
      return Err(INVALID);
    }
  } else if version != 0 {
    return Err(INVALID);
  }
  let mut cursor = Cursor::new(body);
  let count = capped(cursor.u32()?)?;
  let mut runs = Vec::with_capacity(count as usize);
  let signed = composition && version == 1;
  for _ in 0..count {
    let samples = cursor.u32()?;
    let value = if signed {
      i64::from(cursor.i32()?)
    } else {
      i64::from(cursor.u32()?)
    };
    runs.push(Run {
      count: samples,
      value,
    });
  }
  if !cursor.remaining().is_empty() {
    return Err(INVALID);
  }
  Ok(runs)
}

fn parse_stsc(payload: &[u8]) -> Result<Vec<Stsc>, Error> {
  let (version, body) = full_box(payload)?;
  if version != 0 {
    return Err(INVALID);
  }
  let mut cursor = Cursor::new(body);
  let count = capped(cursor.u32()?)?;
  let mut rows = Vec::with_capacity(count as usize);
  for _ in 0..count {
    rows.push(Stsc {
      first: cursor.u32()?,
      per: cursor.u32()?,
      desc: cursor.u32()?,
    });
  }
  if !cursor.remaining().is_empty() {
    return Err(INVALID);
  }
  Ok(rows)
}

fn parse_sync(payload: &[u8]) -> Result<Vec<u32>, Error> {
  let (version, body) = full_box(payload)?;
  if version != 0 {
    return Err(INVALID);
  }
  let mut cursor = Cursor::new(body);
  let count = capped(cursor.u32()?)?;
  let mut numbers = Vec::with_capacity(count as usize);
  for _ in 0..count {
    numbers.push(cursor.u32()?);
  }
  if !cursor.remaining().is_empty() {
    return Err(INVALID);
  }
  numbers.sort_unstable();
  numbers.dedup();
  Ok(numbers)
}

fn sum_counts(runs: &[Run]) -> Result<u64, Error> {
  let mut total = 0u64;
  for run in runs {
    total = total.checked_add(u64::from(run.count)).ok_or(INVALID)?;
  }
  Ok(total)
}

struct Runs {
  entries: Vec<Run>,
  index: usize,
  left: u32,
  value: i64,
}

impl Runs {
  fn new(entries: Vec<Run>) -> Self {
    Self {
      entries,
      index: 0,
      left: 0,
      value: 0,
    }
  }

  fn next(&mut self) -> Result<i64, Error> {
    while self.left == 0 {
      let Some(run) = self.entries.get(self.index) else {
        return Err(INVALID);
      };
      let count = run.count;
      let value = run.value;
      self.index += 1;
      if count == 0 {
        continue;
      }
      self.left = count;
      self.value = value;
    }
    self.left -= 1;
    Ok(self.value)
  }
}

fn materialize(
  file: &[u8],
  desc: u32,
  entry: Entry,
  timescale: u32,
  edit: i64,
  tables: Tables,
) -> Result<Option<Track>, Error> {
  let count = match &tables.sizes {
    Some(sizes) => sizes.count()?,
    None => 0,
  };
  if let Some(runs) = &tables.stts {
    if sum_counts(runs)? != u64::from(count) {
      return Err(INVALID);
    }
  } else if count != 0 {
    return Err(INVALID);
  }
  if let Some(runs) = &tables.ctts {
    if sum_counts(runs)? != u64::from(count) {
      return Err(INVALID);
    }
  }
  if let Some(sync) = &tables.stss {
    if sync.iter().any(|number| *number == 0 || *number > count) {
      return Err(INVALID);
    }
  }
  if count == 0 {
    return Ok(Some(track_with(entry, Vec::new())));
  }
  let Tables {
    sizes,
    offsets,
    stts,
    stsc,
    stss,
    ctts,
    moof,
  } = tables;
  let Some(offsets) = offsets.filter(|offsets| !offsets.is_empty()) else {
    if moof {
      return Err(FRAGMENTED);
    }
    return Err(INVALID);
  };
  let Some(stsc) = stsc else {
    return Err(INVALID);
  };
  let Some(stts) = stts else {
    return Err(INVALID);
  };
  let Some(sizes) = sizes else {
    return Err(INVALID);
  };
  let plan = chunk_plan(&stsc, &offsets, desc, count)?;
  let audio = matches!(entry, Entry::Audio(_));
  let mut samples = Vec::with_capacity(count as usize);
  let mut left = count;
  let mut dts = 0i128;
  let mut times = Runs::new(stts);
  let mut compositions = ctts.map(Runs::new);
  let mut number = 1u32;
  for (mut cursor, per) in plan {
    // Stop before a huge samples-per-chunk value can spin.
    if per > left {
      return Err(INVALID);
    }
    for _ in 0..per {
      let size = sizes.at(samples.len())?;
      let start = usize::try_from(cursor).map_err(|_| OUTSIDE)?;
      let next = cursor.checked_add(u64::from(size)).ok_or(OUTSIDE)?;
      let end = usize::try_from(next).map_err(|_| OUTSIDE)?;
      if end > file.len() {
        return Err(OUTSIDE);
      }
      let delta = times.next()?;
      let composition = match &mut compositions {
        Some(runs) => runs.next()?,
        None => 0,
      };
      let pts = dts
        .checked_add(i128::from(composition))
        .and_then(|sum| sum.checked_sub(i128::from(edit)))
        .ok_or(INVALID)?;
      // Each value is truncated on its own, so durations may not add up
      // to the next timestamp.
      let timestamp = ticks_to_us(pts, timescale)?;
      let duration = ticks_to_us(i128::from(delta), timescale)?;
      let key = if audio {
        true
      } else {
        match &stss {
          None => true,
          Some(list) => list.binary_search(&number).is_ok(),
        }
      };
      samples.push(Sample {
        key,
        timestamp,
        duration,
        data: file[start..end].to_vec(),
      });
      dts = dts.checked_add(i128::from(delta)).ok_or(INVALID)?;
      cursor = next;
      left -= 1;
      number = number.checked_add(1).ok_or(INVALID)?;
    }
  }
  if left != 0 {
    return Err(INVALID);
  }
  Ok(Some(track_with(entry, samples)))
}

fn chunk_plan(
  rows: &[Stsc],
  offsets: &[u64],
  chosen: u32,
  count: u32,
) -> Result<Vec<(u64, u32)>, Error> {
  if rows.is_empty() || rows[0].first != 1 {
    return Err(INVALID);
  }
  for pair in rows.windows(2) {
    if pair[1].first <= pair[0].first {
      return Err(INVALID);
    }
  }
  let chunk_count = u32::try_from(offsets.len()).map_err(|_| INVALID)?;
  let Some(last) = rows.last() else {
    return Err(INVALID);
  };
  if last.first > chunk_count {
    return Err(INVALID);
  }
  let mut plan = Vec::with_capacity(offsets.len());
  let mut entry = 0usize;
  let mut remaining = count;
  for chunk in 1..=chunk_count {
    while entry + 1 < rows.len() && rows[entry + 1].first <= chunk {
      entry += 1;
    }
    let row = &rows[entry];
    if chunk < row.first || row.per == 0 || row.desc == 0 {
      return Err(INVALID);
    }
    if row.desc != chosen {
      return Err(CHANGES);
    }
    if row.per > remaining {
      return Err(INVALID);
    }
    remaining -= row.per;
    plan.push((offsets[(chunk - 1) as usize], row.per));
  }
  if remaining != 0 {
    return Err(INVALID);
  }
  Ok(plan)
}

fn track_with(entry: Entry, samples: Vec<Sample>) -> Track {
  match entry {
    Entry::Video(entry) => Track::Video(VideoTrack {
      codec: entry.codec,
      description: entry.description,
      coded_width: entry.width,
      coded_height: entry.height,
      samples,
    }),
    Entry::Audio(entry) => Track::Audio(AudioTrack {
      codec: entry.codec,
      description: entry.description,
      sample_rate: entry.sample_rate,
      channels: entry.channels,
      samples,
    }),
  }
}

fn ticks_to_us(ticks: i128, timescale: u32) -> Result<i64, Error> {
  if timescale == 0 {
    return Err(INVALID);
  }
  let micros =
    ticks.checked_mul(1_000_000).ok_or(INVALID)? / i128::from(timescale);
  i64::try_from(micros).map_err(|_| INVALID)
}

#[cfg(test)]
mod tests {
  use super::{parse, parse_offsets, parse_stsz, parse_stz2, timescale_of};

  const AVCC: &[u8] = &[
    0x01, 0x42, 0xE0, 0x1E, 0xFF, 0xE1, 0x00, 0x01, 0x67, 0x01, 0x00, 0x01,
    0x68,
  ];
  const ESDS: &[u8] = &[
    0x00, 0x00, 0x00, 0x00, 0x03, 0x19, 0x00, 0x01, 0x00, 0x04, 0x14, 0x40,
    0x15, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x05, 0x02, 0x12, 0x10, 0x06, 0x01, 0x02,
  ];
  const INVALID: &str = "The MP4 file is not a valid container";
  const NO_MOVIE: &str = "The MP4 file has no movie";
  const NO_TRACK: &str = "The MP4 file has no H.264 or AAC track";
  const FRAGMENTED: &str = "Fragmented MP4 is not supported";
  const OUTSIDE: &str = "The MP4 sample is outside the file";
  const CHANGES: &str = "The MP4 track changes sample entries";
  const ENCRYPTED: &str = "Encrypted MP4 is not supported";
  const NO_AVCC: &str = "The H.264 track has no avcC";
  const NO_ASC: &str = "The AAC track has no AudioSpecificConfig";
  const TOO_MANY: &str = "The MP4 track has too many samples";

  fn msg(bytes: &[u8]) -> &'static str {
    parse(bytes).unwrap_err().message()
  }

  fn ok(bytes: &[u8]) -> super::Movie {
    parse(bytes).unwrap_or_else(|error| panic!("{}", error.message()))
  }

  fn box4(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
    let size = u32::try_from(8 + payload.len()).unwrap();
    let mut out = size.to_be_bytes().to_vec();
    out.extend_from_slice(kind);
    out.extend_from_slice(payload);
    out
  }

  fn full(kind: &[u8; 4], version: u8, body: &[u8]) -> Vec<u8> {
    let mut payload = vec![version, 0, 0, 0];
    payload.extend_from_slice(body);
    box4(kind, &payload)
  }

  fn concat(parts: impl IntoIterator<Item = Vec<u8>>) -> Vec<u8> {
    parts.into_iter().flatten().collect()
  }

  fn largesize(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
    let size = 16 + payload.len() as u64;
    let mut out = 1u32.to_be_bytes().to_vec();
    out.extend_from_slice(kind);
    out.extend_from_slice(&size.to_be_bytes());
    out.extend_from_slice(payload);
    out
  }

  fn ftyp() -> Vec<u8> {
    let mut payload = b"isom".to_vec();
    payload.extend_from_slice(&0u32.to_be_bytes());
    payload.extend_from_slice(b"isom");
    box4(b"ftyp", &payload)
  }

  fn mdhd(timescale: u32) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(&0u32.to_be_bytes());
    body.extend_from_slice(&0u32.to_be_bytes());
    body.extend_from_slice(&timescale.to_be_bytes());
    body.extend_from_slice(&0u32.to_be_bytes());
    body.extend_from_slice(&0u16.to_be_bytes());
    body.extend_from_slice(&0u16.to_be_bytes());
    full(b"mdhd", 0, &body)
  }

  fn hdlr(handler: &[u8; 4]) -> Vec<u8> {
    let mut body = vec![0u8; 4];
    body.extend_from_slice(handler);
    body.extend_from_slice(&[0u8; 13]);
    full(b"hdlr", 0, &body)
  }

  fn trak(
    handler: &[u8; 4],
    timescale: u32,
    tables: &[u8],
    edit: Option<Vec<u8>>,
  ) -> Vec<u8> {
    let mut body = Vec::new();
    if let Some(edts) = edit {
      body.extend(edts);
    }
    let mdia = concat([
      mdhd(timescale),
      hdlr(handler),
      box4(b"minf", &box4(b"stbl", tables)),
    ]);
    body.extend(box4(b"mdia", &mdia));
    box4(b"trak", &body)
  }

  fn container(media: &[u8], traks: Vec<Vec<u8>>, tail: &[u8]) -> Vec<u8> {
    let mut file = box4(b"mdat", media);
    let mut body = Vec::new();
    for trak in traks {
      body.extend(trak);
    }
    file.extend(box4(b"moov", &body));
    file.extend_from_slice(tail);
    file
  }

  fn movie_of(
    handler: &[u8; 4],
    timescale: u32,
    media: &[u8],
    tables: Vec<u8>,
    edit: Option<Vec<u8>>,
    tail: &[u8],
  ) -> Vec<u8> {
    container(media, vec![trak(handler, timescale, &tables, edit)], tail)
  }

  fn stsd(entries: &[Vec<u8>]) -> Vec<u8> {
    let mut body = (entries.len() as u32).to_be_bytes().to_vec();
    for entry in entries {
      body.extend(entry);
    }
    full(b"stsd", 0, &body)
  }

  fn stts(runs: &[(u32, u32)]) -> Vec<u8> {
    let mut body = (runs.len() as u32).to_be_bytes().to_vec();
    for (count, delta) in runs {
      body.extend_from_slice(&count.to_be_bytes());
      body.extend_from_slice(&delta.to_be_bytes());
    }
    full(b"stts", 0, &body)
  }

  fn stsc(rows: &[(u32, u32, u32)]) -> Vec<u8> {
    let mut body = (rows.len() as u32).to_be_bytes().to_vec();
    for (first, per, desc) in rows {
      body.extend_from_slice(&first.to_be_bytes());
      body.extend_from_slice(&per.to_be_bytes());
      body.extend_from_slice(&desc.to_be_bytes());
    }
    full(b"stsc", 0, &body)
  }

  fn stsz(sizes: &[u32]) -> Vec<u8> {
    let mut body = 0u32.to_be_bytes().to_vec();
    body.extend_from_slice(&(sizes.len() as u32).to_be_bytes());
    for size in sizes {
      body.extend_from_slice(&size.to_be_bytes());
    }
    full(b"stsz", 0, &body)
  }

  fn stsz_constant(size: u32, count: u32) -> Vec<u8> {
    let mut body = size.to_be_bytes().to_vec();
    body.extend_from_slice(&count.to_be_bytes());
    full(b"stsz", 0, &body)
  }

  fn stco(offsets: &[u32]) -> Vec<u8> {
    let mut body = (offsets.len() as u32).to_be_bytes().to_vec();
    for offset in offsets {
      body.extend_from_slice(&offset.to_be_bytes());
    }
    full(b"stco", 0, &body)
  }

  fn co64(offsets: &[u64]) -> Vec<u8> {
    let mut body = (offsets.len() as u32).to_be_bytes().to_vec();
    for offset in offsets {
      body.extend_from_slice(&offset.to_be_bytes());
    }
    full(b"co64", 0, &body)
  }

  fn stss(numbers: &[u32]) -> Vec<u8> {
    let mut body = (numbers.len() as u32).to_be_bytes().to_vec();
    for number in numbers {
      body.extend_from_slice(&number.to_be_bytes());
    }
    full(b"stss", 0, &body)
  }

  fn ctts(version: u8, rows: &[(u32, i32)]) -> Vec<u8> {
    let mut body = (rows.len() as u32).to_be_bytes().to_vec();
    for &(count, offset) in rows {
      body.extend_from_slice(&count.to_be_bytes());
      let bytes = if version == 0 {
        u32::try_from(offset).unwrap().to_be_bytes()
      } else {
        offset.to_be_bytes()
      };
      body.extend_from_slice(&bytes);
    }
    full(b"ctts", version, &body)
  }

  fn stz2(field: u8, sizes: &[u32]) -> Vec<u8> {
    let mut body = vec![0, 0, 0, field];
    body.extend_from_slice(&(sizes.len() as u32).to_be_bytes());
    match field {
      4 => {
        let mut index = 0;
        while index < sizes.len() {
          let high = sizes[index] as u8;
          let low = sizes.get(index + 1).copied().unwrap_or(0) as u8;
          body.push((high << 4) | (low & 0x0f));
          index += 2;
        }
      }
      8 => {
        for size in sizes {
          body.push(*size as u8);
        }
      }
      16 => {
        for size in sizes {
          body.extend_from_slice(&(*size as u16).to_be_bytes());
        }
      }
      _ => panic!("field"),
    }
    full(b"stz2", 0, &body)
  }

  fn visual(
    kind: &[u8; 4],
    width: u16,
    height: u16,
    children: &[u8],
  ) -> Vec<u8> {
    let mut payload = vec![0u8; 78];
    payload[7] = 1;
    payload[24..26].copy_from_slice(&width.to_be_bytes());
    payload[26..28].copy_from_slice(&height.to_be_bytes());
    payload.extend_from_slice(children);
    box4(kind, &payload)
  }

  fn avc_entry() -> Vec<u8> {
    visual(b"avc1", 16, 32, &box4(b"avcC", AVCC))
  }

  fn audio_entry(
    version: u16,
    channels: u16,
    rate: u32,
    child: &[u8],
  ) -> Vec<u8> {
    let prefix = if version == 1 { 44 } else { 28 };
    let mut payload = vec![0u8; prefix];
    payload[7] = 1;
    payload[8..10].copy_from_slice(&version.to_be_bytes());
    payload[16..18].copy_from_slice(&channels.to_be_bytes());
    payload[19] = 16;
    payload[24..28].copy_from_slice(&(rate << 16).to_be_bytes());
    payload.extend_from_slice(child);
    box4(b"mp4a", &payload)
  }

  fn esds_asc(asc: &[u8]) -> Vec<u8> {
    assert!(asc.len() < 100);
    let mut decoder = vec![0x40, 0x15, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    decoder.push(0x05);
    decoder.push(u8::try_from(asc.len()).unwrap());
    decoder.extend_from_slice(asc);
    decoder.extend_from_slice(&[0x06, 0x01, 0x02]);
    let mut es = vec![0x00, 0x01, 0x00, 0x04];
    es.push(u8::try_from(decoder.len()).unwrap());
    es.extend(decoder);
    let mut payload = vec![0, 0, 0, 0, 0x03];
    payload.push(u8::try_from(es.len()).unwrap());
    payload.extend(es);
    box4(b"esds", &payload)
  }

  fn edts_v0(entries: &[(u32, u32, u16, u16)]) -> Vec<u8> {
    let mut body = (entries.len() as u32).to_be_bytes().to_vec();
    for &(duration, media, rate, fraction) in entries {
      body.extend_from_slice(&duration.to_be_bytes());
      body.extend_from_slice(&media.to_be_bytes());
      body.extend_from_slice(&rate.to_be_bytes());
      body.extend_from_slice(&fraction.to_be_bytes());
    }
    box4(b"edts", &full(b"elst", 0, &body))
  }

  fn edts_v1(media_time: i64, rate: u16) -> Vec<u8> {
    let mut body = 1u32.to_be_bytes().to_vec();
    body.extend_from_slice(&0u64.to_be_bytes());
    body.extend_from_slice(&media_time.to_be_bytes());
    body.extend_from_slice(&rate.to_be_bytes());
    body.extend_from_slice(&0u16.to_be_bytes());
    box4(b"edts", &full(b"elst", 1, &body))
  }

  fn scaled_video(
    samples: &[&[u8]],
    delta: u32,
    scale: u32,
    edit: Option<Vec<u8>>,
    extra: &[Vec<u8>],
  ) -> Vec<u8> {
    let media: Vec<u8> = samples
      .iter()
      .flat_map(|sample| sample.iter().copied())
      .collect();
    let sizes: Vec<u32> =
      samples.iter().map(|sample| sample.len() as u32).collect();
    let count = u32::try_from(samples.len()).unwrap();
    let mut parts = vec![
      stsd(&[avc_entry()]),
      stts(&[(count, delta)]),
      stsc(&[(1, count, 1)]),
      stsz(&sizes),
      stco(&[8]),
    ];
    parts.extend(extra.iter().cloned());
    movie_of(b"vide", scale, &media, concat(parts), edit, &[])
  }

  fn video_trak_at(offset: u32) -> Vec<u8> {
    let tables = concat([
      stsd(&[avc_entry()]),
      stts(&[(2, 40)]),
      stsc(&[(1, 2, 1)]),
      stsz(&[5, 5]),
      stco(&[offset]),
      stss(&[1]),
    ]);
    trak(b"vide", 1000, &tables, None)
  }

  fn audio_trak_at(offset: u32) -> Vec<u8> {
    let entry = audio_entry(0, 1, 48_000, &box4(b"esds", ESDS));
    let tables = concat([
      stsd(&[entry]),
      stts(&[(3, 1024)]),
      stsc(&[(1, 3, 1)]),
      stsz(&[2, 3, 1]),
      stco(&[offset]),
    ]);
    trak(b"soun", 44_100, &tables, None)
  }

  fn progressive() -> Vec<u8> {
    let mut media = Vec::new();
    media.extend_from_slice(&[0, 0, 0, 1, 0x65]);
    media.extend_from_slice(&[0, 0, 0, 1, 0x41]);
    media.extend_from_slice(&[0x11, 0x22]);
    media.extend_from_slice(&[0x33, 0x44, 0x55]);
    media.extend_from_slice(&[0x66]);
    let head = ftyp();
    let video_at = u32::try_from(head.len() + 8).unwrap();
    let audio_at = video_at + 10;
    let moov = box4(
      b"moov",
      &concat([video_trak_at(video_at), audio_trak_at(audio_at)]),
    );
    let mut file = head;
    file.extend(box4(b"mdat", &media));
    file.extend(moov);
    file
  }

  fn empty_video(tail: &[u8]) -> Vec<u8> {
    let tables = concat([stsd(&[avc_entry()]), stsz(&[])]);
    movie_of(b"vide", 1000, &[], tables, None, tail)
  }

  fn audio_config(entry: Vec<u8>) -> Vec<u8> {
    let tables = concat([stsd(&[entry]), stsz(&[])]);
    movie_of(b"soun", 44_100, &[], tables, None, &[])
  }

  fn tables(parts: Vec<Vec<u8>>) -> Vec<u8> {
    let media = [0u8, 0, 0, 1, 0x65];
    movie_of(b"vide", 1000, &media, concat(parts), None, &[])
  }

  #[test]
  fn esds_box_matches_the_hand_checked_descriptor() {
    assert_eq!(&esds_asc(&[0x12, 0x10])[8..], ESDS);
  }

  #[test]
  fn progressive_samples_keep_bytes_and_truncated_timestamps() {
    assert_eq!(ftyp().len(), 20);
    let movie = ok(&progressive());
    assert_eq!(movie.video.len(), 1);
    assert_eq!(movie.audio.len(), 1);
    let video = &movie.video[0];
    assert_eq!(video.codec, "avc1.42E01E");
    assert_eq!(video.description, AVCC);
    assert_eq!(video.coded_width, 16);
    assert_eq!(video.coded_height, 32);
    assert_eq!(video.samples.len(), 2);
    assert!(video.samples[0].key);
    assert!(!video.samples[1].key);
    assert_eq!(video.samples[0].timestamp, 0);
    assert_eq!(video.samples[1].timestamp, 40_000);
    assert_eq!(video.samples[0].duration, 40_000);
    assert_eq!(video.samples[1].duration, 40_000);
    assert_eq!(video.samples[0].data, [0, 0, 0, 1, 0x65]);
    assert_eq!(video.samples[1].data, [0, 0, 0, 1, 0x41]);
    let audio = &movie.audio[0];
    assert_eq!(audio.codec, "mp4a.40.2");
    assert_eq!(audio.description, [0x12, 0x10]);
    assert_eq!(audio.sample_rate, 44_100);
    assert_eq!(audio.channels, 2);
    assert_eq!(audio.samples.len(), 3);
    assert!(audio.samples.iter().all(|sample| sample.key));
    assert_eq!(audio.samples[0].timestamp, 0);
    assert_eq!(audio.samples[1].timestamp, 23_219);
    assert_eq!(audio.samples[2].timestamp, 46_439);
    assert_eq!(audio.samples[0].duration, 23_219);
    assert_eq!(audio.samples[1].duration, 23_219);
    assert_eq!(audio.samples[2].duration, 23_219);
    assert_eq!(audio.samples[0].data, [0x11, 0x22]);
    assert_eq!(audio.samples[1].data, [0x33, 0x44, 0x55]);
    assert_eq!(audio.samples[2].data, [0x66]);
  }

  #[test]
  fn fixture_matches_the_builder() {
    let path = std::path::Path::new(concat!(
      env!("CARGO_MANIFEST_DIR"),
      "/../../tests/fixtures/media/progressive.mp4"
    ));
    let stored = std::fs::read(path).expect("fixture");
    assert_eq!(stored, progressive());
  }

  #[test]
  fn composition_offsets_and_one_edit_shift_timestamps() {
    let shifted = ok(&scaled_video(
      &[&[0x65], &[0x41]],
      100,
      1000,
      None,
      &[ctts(0, &[(1, 100), (1, 0)])],
    ));
    let video = &shifted.video[0];
    assert_eq!(video.samples[0].timestamp, 100_000);
    assert_eq!(video.samples[1].timestamp, 100_000);
    assert_eq!(video.samples[0].duration, 100_000);
    assert_eq!(video.samples[1].duration, 100_000);
    assert_eq!(video.samples[0].data, [0x65]);
    assert_eq!(video.samples[1].data, [0x41]);

    let negative = ok(&scaled_video(
      &[&[0x65]],
      100,
      1000,
      None,
      &[ctts(1, &[(1, -10)])],
    ));
    assert_eq!(negative.video[0].samples[0].timestamp, -10_000);
    assert_eq!(negative.video[0].samples[0].duration, 100_000);

    let edited = ok(&scaled_video(
      &[&[0x65], &[0x41]],
      100,
      1000,
      Some(edts_v0(&[(0, 100, 1, 0)])),
      &[],
    ));
    assert_eq!(edited.video[0].samples[0].timestamp, -100_000);
    assert_eq!(edited.video[0].samples[1].timestamp, 0);
    assert_eq!(edited.video[0].samples[0].duration, 100_000);
    assert_eq!(edited.video[0].samples[1].duration, 100_000);

    let skipped = ok(&movie_of(
      b"vide",
      1000,
      &[0x11, 0x22],
      concat([
        stsd(&[avc_entry()]),
        stts(&[(0, 99), (2, 40)]),
        stsc(&[(1, 2, 1)]),
        stsz(&[1, 1]),
        stco(&[8]),
      ]),
      None,
      &[],
    ));
    assert_eq!(skipped.video[0].samples[0].timestamp, 0);
    assert_eq!(skipped.video[0].samples[1].timestamp, 40_000);
    assert_eq!(skipped.video[0].samples[0].duration, 40_000);
  }

  #[test]
  fn other_edit_lists_stay_unused() {
    let pair = |edit| {
      let movie = ok(&scaled_video(&[&[1], &[2]], 100, 1000, Some(edit), &[]));
      let samples = &movie.video[0].samples;
      (samples[0].timestamp, samples[1].timestamp)
    };
    assert_eq!(pair(edts_v0(&[(0, 100, 2, 0)])), (0, 100_000));
    assert_eq!(pair(edts_v0(&[(0, 100, 1, 1)])), (0, 100_000));
    assert_eq!(pair(edts_v0(&[(0, 100, 1, 0), (0, 0, 1, 0)])), (0, 100_000));
    assert_eq!(pair(edts_v0(&[(0, u32::MAX, 1, 0)])), (0, 100_000));
    assert_eq!(pair(edts_v1(-1, 1)), (0, 100_000));
    assert_eq!(pair(edts_v1(100, 1)), (-100_000, 0));
  }

  #[test]
  fn sync_samples_follow_stss() {
    let keys = |extra: &[Vec<u8>]| {
      let movie = ok(&scaled_video(&[&[1], &[2]], 40, 1000, None, extra));
      let samples = &movie.video[0].samples;
      (samples[0].key, samples[1].key)
    };
    assert_eq!(keys(&[]), (true, true));
    assert_eq!(keys(&[stss(&[])]), (false, false));
    assert_eq!(keys(&[stss(&[2, 1])]), (true, true));
    assert_eq!(keys(&[stss(&[1])]), (true, false));

    let frames = [0x11u8, 0x22, 0x33];
    let entry = audio_entry(0, 2, 44_100, &box4(b"esds", ESDS));
    let movie = ok(&movie_of(
      b"soun",
      44_100,
      &frames,
      concat([
        stsd(&[entry]),
        stts(&[(2, 1024)]),
        stsc(&[(1, 2, 1)]),
        stsz(&[2, 1]),
        stco(&[8]),
        stss(&[2]),
      ]),
      None,
      &[],
    ));
    assert!(movie.audio[0].samples.iter().all(|sample| sample.key));
  }

  #[test]
  fn compact_sizes_and_wide_offsets() {
    let constant = ok(&movie_of(
      b"vide",
      1000,
      &[9, 8, 7, 6],
      concat([
        stsd(&[avc_entry()]),
        stts(&[(1, 10)]),
        stsc(&[(1, 1, 1)]),
        stsz_constant(4, 1),
        stco(&[8]),
      ]),
      None,
      &[],
    ));
    assert_eq!(constant.video[0].samples[0].data, [9, 8, 7, 6]);
    assert_eq!(constant.video[0].samples[0].duration, 10_000);

    let nibbles = ok(&movie_of(
      b"vide",
      1000,
      &[1, 2, 3, 4, 5],
      concat([
        stsd(&[avc_entry()]),
        stts(&[(2, 10)]),
        stsc(&[(1, 2, 1)]),
        stz2(4, &[2, 3]),
        stco(&[8]),
      ]),
      None,
      &[],
    ));
    assert_eq!(nibbles.video[0].samples[0].data, [1, 2]);
    assert_eq!(nibbles.video[0].samples[1].data, [3, 4, 5]);

    let wide = ok(&movie_of(
      b"vide",
      1000,
      &[0xAB],
      concat([
        stsd(&[avc_entry()]),
        stts(&[(1, 1000)]),
        stsc(&[(1, 1, 1)]),
        stz2(8, &[1]),
        co64(&[8]),
      ]),
      None,
      &[],
    ));
    assert_eq!(wide.video[0].samples[0].data, [0xAB]);
    assert_eq!(wide.video[0].samples[0].timestamp, 0);
    assert_eq!(wide.video[0].samples[0].duration, 1_000_000);
    assert!(wide.video[0].samples[0].key);

    let wide16 = ok(&movie_of(
      b"vide",
      1000,
      &[1, 2, 3, 4, 5],
      concat([
        stsd(&[avc_entry()]),
        stts(&[(1, 1)]),
        stsc(&[(1, 1, 1)]),
        stz2(16, &[5]),
        co64(&[8]),
      ]),
      None,
      &[],
    ));
    assert_eq!(wide16.video[0].samples[0].data, [1, 2, 3, 4, 5]);
  }

  #[test]
  fn audio_config_prefers_the_decoder_specific_info() {
    let movie = ok(&audio_config(audio_entry(
      0,
      1,
      48_000,
      &esds_asc(&[0x12, 0x10]),
    )));
    assert_eq!(movie.audio[0].codec, "mp4a.40.2");
    assert_eq!(movie.audio[0].sample_rate, 44_100);
    assert_eq!(movie.audio[0].channels, 2);
    assert_eq!(movie.audio[0].description, [0x12, 0x10]);

    let wave = ok(&audio_config(audio_entry(
      0,
      1,
      48_000,
      &box4(b"wave", &esds_asc(&[0x12, 0x10])),
    )));
    assert_eq!(wave.audio[0].sample_rate, 44_100);
    assert_eq!(wave.audio[0].channels, 2);

    let quicktime = ok(&audio_config(audio_entry(
      1,
      1,
      48_000,
      &esds_asc(&[0x12, 0x10]),
    )));
    assert_eq!(quicktime.audio[0].codec, "mp4a.40.2");
    assert_eq!(quicktime.audio[0].sample_rate, 44_100);

    let raw = ok(&audio_config(audio_entry(
      0,
      1,
      48_000,
      &box4(b"esds", &ESDS[4..]),
    )));
    assert_eq!(raw.audio[0].description, [0x12, 0x10]);

    let seven = ok(&audio_config(audio_entry(
      0,
      1,
      48_000,
      &esds_asc(&[0x12, 0x38]),
    )));
    assert_eq!(seven.audio[0].channels, 8);
    assert_eq!(seven.audio[0].sample_rate, 44_100);

    let program = ok(&audio_config(audio_entry(
      0,
      3,
      48_000,
      &esds_asc(&[0x12, 0x00]),
    )));
    assert_eq!(program.audio[0].channels, 3);

    let neither = ok(&audio_config(audio_entry(
      0,
      0,
      48_000,
      &esds_asc(&[0x12, 0x00]),
    )));
    assert_eq!(neither.audio[0].channels, 0);
    assert_eq!(neither.audio[0].sample_rate, 44_100);

    let explicit = ok(&audio_config(audio_entry(
      0,
      1,
      48_000,
      &esds_asc(&[0x17, 0x80, 0x18, 0x1C, 0x88]),
    )));
    assert_eq!(explicit.audio[0].sample_rate, 12_345);
    assert_eq!(explicit.audio[0].channels, 1);

    let fallback = ok(&audio_config(audio_entry(
      0,
      1,
      48_000,
      &esds_asc(&[0x17, 0x80, 0x00, 0x00, 0x08]),
    )));
    assert_eq!(fallback.audio[0].sample_rate, 48_000);

    let avc3 = visual(b"avc3", 8, 4, &box4(b"avcC", AVCC));
    let movie = ok(&movie_of(
      b"vide",
      1000,
      &[],
      concat([stsd(&[avc3]), stsz(&[])]),
      None,
      &[],
    ));
    assert_eq!(movie.video[0].codec, "avc3.42E01E");
    assert_eq!(movie.video[0].coded_width, 8);
    assert_eq!(movie.video[0].coded_height, 4);
  }

  #[test]
  fn encrypted_entries_are_rejected() {
    assert_eq!(
      msg(&audio_config(audio_entry(
        0,
        2,
        44_100,
        &box4(b"sinf", &[])
      ))),
      ENCRYPTED
    );
    assert_eq!(
      msg(&audio_config(audio_entry(
        0,
        2,
        44_100,
        &concat([box4(b"esds", ESDS), box4(b"sinf", &[])]),
      ))),
      ENCRYPTED
    );
    assert_eq!(
      msg(&audio_config(audio_entry(
        0,
        2,
        44_100,
        &box4(b"wave", &box4(b"sinf", &[])),
      ))),
      ENCRYPTED
    );
    let visual = visual(b"avc1", 16, 32, &box4(b"sinf", &[]));
    assert_eq!(
      msg(&movie_of(
        b"vide",
        1000,
        &[],
        concat([stsd(&[visual]), stsz(&[])]),
        None,
        &[],
      )),
      ENCRYPTED
    );
  }

  #[test]
  fn fragmented_movies_are_rejected() {
    assert_eq!(msg(&box4(b"moof", &[])), FRAGMENTED);
    assert_eq!(msg(&empty_video(&box4(b"moof", &[]))), FRAGMENTED);
    let declared = movie_of(
      b"vide",
      1000,
      &[0, 0, 0, 1, 0x65],
      concat([
        stsd(&[avc_entry()]),
        stts(&[(1, 40)]),
        stsc(&[(1, 1, 1)]),
        stsz(&[5]),
      ]),
      None,
      &box4(b"moof", &[]),
    );
    assert_eq!(msg(&declared), FRAGMENTED);
    assert_eq!(
      msg(&movie_of(
        b"vide",
        1000,
        &[0, 0, 0, 1, 0x65],
        concat([
          stsd(&[avc_entry()]),
          stts(&[(1, 40)]),
          stsc(&[(1, 1, 1)]),
          stsz(&[5]),
        ]),
        None,
        &[],
      )),
      INVALID
    );
    let mut kept = progressive();
    kept.extend(box4(b"moof", &[]));
    let movie = ok(&kept);
    assert_eq!(movie.video[0].samples.len(), 2);
    assert_eq!(movie.audio[0].samples.len(), 3);
  }

  #[test]
  fn unsupported_and_empty_tracks() {
    let movie = ok(&empty_video(&[]));
    assert_eq!(movie.video.len(), 1);
    assert!(movie.audio.is_empty());
    assert_eq!(movie.video[0].codec, "avc1.42E01E");
    assert_eq!(movie.video[0].description, AVCC);
    assert_eq!(movie.video[0].coded_width, 16);
    assert_eq!(movie.video[0].coded_height, 32);
    assert!(movie.video[0].samples.is_empty());

    let garbage = box4(b"stts", &[1]);
    let skipped = trak(
      b"vide",
      1000,
      &concat([stsd(&[box4(b"vp09", &[0u8; 8])]), garbage.clone()]),
      None,
    );
    assert_eq!(msg(&container(&[], vec![skipped.clone()], &[])), NO_TRACK);
    let (media, kept) = {
      let sample = [0u8, 0, 0, 1, 0x65];
      let trak = trak(
        b"vide",
        1000,
        &concat([
          stsd(&[avc_entry()]),
          stts(&[(1, 40)]),
          stsc(&[(1, 1, 1)]),
          stsz(&[5]),
          stco(&[8]),
        ]),
        None,
      );
      (sample.to_vec(), trak)
    };
    let movie = ok(&container(&media, vec![skipped, kept], &[]));
    assert_eq!(movie.video.len(), 1);
    assert_eq!(movie.video[0].samples[0].data, media);

    assert_eq!(
      msg(&movie_of(
        b"VIDE",
        1000,
        &[],
        concat([stsd(&[avc_entry()]), stsz(&[])]),
        None,
        &[],
      )),
      NO_TRACK
    );
    assert_eq!(
      msg(&container(&[], vec![box4(b"trak", &[])], &[])),
      NO_TRACK
    );
    assert_eq!(msg(&box4(b"moov", &[])), NO_TRACK);
    assert_eq!(msg(&box4(b"free", &[])), NO_MOVIE);
    assert_eq!(msg(&[]), "The MP4 data is empty");

    let minf = box4(b"minf", &box4(b"stbl", &stsd(&[avc_entry()])));
    let mdia = box4(b"mdia", &concat([hdlr(b"vide"), minf.clone()]));
    assert_eq!(
      msg(&container(&[], vec![box4(b"trak", &mdia)], &[])),
      INVALID
    );
    let header = mdhd(1000);
    let mdia = box4(
      b"mdia",
      &concat([header.clone(), header, hdlr(b"vide"), minf]),
    );
    assert_eq!(
      msg(&container(&[], vec![box4(b"trak", &mdia)], &[])),
      INVALID
    );
    let bare = box4(
      b"mdia",
      &concat([
        mdhd(1000),
        hdlr(b"vide"),
        box4(b"minf", &box4(b"stbl", &[])),
      ]),
    );
    assert_eq!(
      msg(&container(&[], vec![box4(b"trak", &bare)], &[])),
      INVALID
    );

    let mut doubled = box4(b"moov", &[]);
    doubled.extend(box4(b"moov", &[]));
    assert_eq!(msg(&doubled), INVALID);
    let mut wide = largesize(b"free", &[]);
    wide.extend(box4(b"moov", &[]));
    assert_eq!(msg(&wide), NO_TRACK);
    let mut tail = progressive();
    tail.extend_from_slice(&[0, 0, 0, 0, b'f', b'r', b'e', b'e', 0xFF]);
    let movie = ok(&tail);
    assert_eq!(movie.video[0].samples.len(), 2);
    assert_eq!(movie.audio[0].samples[2].timestamp, 46_439);
  }

  #[test]
  fn malformed_tables_and_boxes() {
    assert_eq!(msg(&[0, 0, 0, 1]), INVALID);
    let mut truncated = box4(b"moov", &[]);
    truncated[0..4].copy_from_slice(&100u32.to_be_bytes());
    assert_eq!(msg(&truncated), INVALID);
    assert_eq!(
      msg(&movie_of(
        b"vide",
        0,
        &[],
        concat([stsd(&[avc_entry()]), stsz(&[])]),
        None,
        &[],
      )),
      INVALID
    );
    assert_eq!(
      msg(&movie_of(
        b"vide",
        1000,
        &[],
        concat([stsd(&[visual(b"avc1", 16, 32, &[])]), stsz(&[])]),
        None,
        &[],
      )),
      NO_AVCC
    );
    assert_eq!(
      msg(&movie_of(
        b"vide",
        1000,
        &[],
        concat([
          stsd(&[visual(b"avc1", 16, 32, &box4(b"avcC", &[0, 1, 2, 3]))]),
          stsz(&[]),
        ]),
        None,
        &[],
      )),
      INVALID
    );
    assert_eq!(
      msg(&movie_of(
        b"vide",
        1000,
        &[],
        concat([
          stsd(&[visual(b"avc1", 16, 32, &box4(b"avcC", &[1, 2]))]),
          stsz(&[]),
        ]),
        None,
        &[],
      )),
      INVALID
    );
    assert_eq!(
      msg(&movie_of(
        b"vide",
        1000,
        &[],
        concat([stsd(&[box4(b"avc1", &[0u8; 10])]), stsz(&[])]),
        None,
        &[],
      )),
      INVALID
    );
    assert_eq!(msg(&audio_config(audio_entry(0, 2, 44_100, &[]))), NO_ASC);
    assert_eq!(
      msg(&audio_config(audio_entry(
        0,
        2,
        44_100,
        &box4(b"esds", &[0, 0, 0, 0]),
      ))),
      INVALID
    );
    assert_eq!(
      msg(&audio_config(audio_entry(
        0,
        2,
        0,
        &esds_asc(&[0x17, 0x80, 0x00, 0x00, 0x08]),
      ))),
      INVALID
    );
    assert_eq!(
      msg(&audio_config(audio_entry(
        0,
        2,
        44_100,
        &esds_asc(&[0x16, 0xC0])
      ))),
      INVALID
    );
    assert_eq!(
      msg(&audio_config(audio_entry(0, 2, 44_100, &esds_asc(&[0x12])))),
      INVALID
    );
    assert_eq!(
      msg(&movie_of(
        b"vide",
        1000,
        &[0, 0, 0, 1, 0x65],
        concat([
          stsd(&[avc_entry()]),
          stts(&[(1, 40)]),
          stsc(&[(1, 1, 1)]),
          stsz(&[5]),
          stco(&[4_000_000_000]),
        ]),
        None,
        &[],
      )),
      OUTSIDE
    );
    assert_eq!(
      msg(&tables(vec![
        stsd(&[avc_entry()]),
        stts(&[(1, 40)]),
        stsc(&[(1, 2, 1)]),
        stsz(&[5, 5]),
        stco(&[8]),
      ])),
      INVALID
    );
    assert_eq!(
      msg(&tables(vec![
        stsd(&[avc_entry()]),
        stts(&[(1, 40)]),
        stsc(&[(1, 0, 1)]),
        stsz(&[5]),
        stco(&[8]),
      ])),
      INVALID
    );
    assert_eq!(
      msg(&tables(vec![
        stsd(&[avc_entry()]),
        stts(&[(1, 40)]),
        stsc(&[(2, 1, 1)]),
        stsz(&[5]),
        stco(&[8]),
      ])),
      INVALID
    );
    assert_eq!(
      msg(&tables(vec![
        stsd(&[avc_entry()]),
        stts(&[(1, 40)]),
        stsc(&[(1, 1, 0)]),
        stsz(&[5]),
        stco(&[8]),
      ])),
      INVALID
    );
    assert_eq!(
      msg(&tables(vec![
        stsd(&[avc_entry()]),
        stts(&[(1, 40)]),
        stsc(&[(1, u32::MAX, 1)]),
        stsz(&[5]),
        stco(&[8]),
      ])),
      INVALID
    );
    let entry = avc_entry();
    assert_eq!(
      msg(&tables(vec![
        stsd(&[entry.clone(), entry]),
        stts(&[(1, 40)]),
        stsc(&[(1, 1, 2)]),
        stsz(&[5]),
        stco(&[8]),
      ])),
      CHANGES
    );
    let chosen = ok(&tables(vec![
      stsd(&[box4(b"mp4v", &[0u8; 8]), avc_entry()]),
      stts(&[(1, 40)]),
      stsc(&[(1, 1, 2)]),
      stsz(&[5]),
      stco(&[8]),
    ]));
    assert_eq!(chosen.video[0].codec, "avc1.42E01E");
    assert_eq!(chosen.video[0].samples[0].data, [0, 0, 0, 1, 0x65]);
    assert_eq!(
      msg(&scaled_video(&[&[1]], 40, 1000, None, &[stts(&[(1, 40)])])),
      INVALID
    );
    assert_eq!(
      msg(&scaled_video(&[&[1]], 40, 1000, None, &[stz2(8, &[1])])),
      INVALID
    );
    assert_eq!(
      msg(&scaled_video(&[&[1]], 40, 1000, None, &[co64(&[8])])),
      INVALID
    );
    assert_eq!(
      msg(&scaled_video(&[&[1], &[2]], 40, 1000, None, &[stss(&[0])])),
      INVALID
    );
    assert_eq!(
      msg(&scaled_video(&[&[1], &[2]], 40, 1000, None, &[stss(&[3])])),
      INVALID
    );
  }

  #[test]
  fn huge_tables_fail_before_allocation() {
    let mut constant = vec![0, 0, 0, 0];
    constant.extend_from_slice(&1u32.to_be_bytes());
    constant.extend_from_slice(&2_000_001u32.to_be_bytes());
    assert_eq!(parse_stsz(&constant).unwrap_err().message(), TOO_MANY);
    constant[8..12].copy_from_slice(&2_000_000u32.to_be_bytes());
    assert!(parse_stsz(&constant).is_ok());

    let mut compact = vec![0, 0, 0, 0, 0, 0, 0, 8];
    compact.extend_from_slice(&2_000_001u32.to_be_bytes());
    assert_eq!(parse_stz2(&compact).unwrap_err().message(), TOO_MANY);

    let mut offsets = vec![0, 0, 0, 0];
    offsets.extend_from_slice(&2_000_001u32.to_be_bytes());
    assert_eq!(
      parse_offsets(&offsets, false).unwrap_err().message(),
      TOO_MANY
    );

    let mut body = vec![0xFFu8; 16];
    body.extend_from_slice(&48_000u32.to_be_bytes());
    let mut payload = vec![1, 0, 0, 0];
    payload.extend(body);
    assert_eq!(timescale_of(&payload).unwrap(), 48_000);
    assert!(timescale_of(&[2, 0, 0, 0]).is_err());
  }
}
