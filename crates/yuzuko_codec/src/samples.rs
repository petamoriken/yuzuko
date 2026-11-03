//! PCM layout and sample conversion for `AudioData`. No V8.
//!
//! Integer and float formats convert through the unit interval with the same
//! scales as Chromium's `FixedSampleTypeTraits`: unsigned 8-bit is centered
//! at 128, and the positive and negative sides use different divisors so the
//! extreme samples stay inside the destination type. Same-format copies move
//! the stored bytes and do not pass through that conversion.

/// WebCodecs allows at most 32 channels.
pub(crate) const MAX_CHANNELS: u32 = 32;
/// One plane stays within 16,777,216 frames.
pub(crate) const MAX_FRAMES: u32 = 16_777_216;
/// `s32` and `f32` are the widest sample. The product fits in `u32`.
pub(crate) const MAX_BYTES: usize =
  MAX_FRAMES as usize * MAX_CHANNELS as usize * 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SampleKind {
  U8,
  S16,
  S32,
  F32,
}

impl SampleKind {
  pub(crate) fn bytes(self) -> usize {
    match self {
      Self::U8 => 1,
      Self::S16 => 2,
      Self::S32 | Self::F32 => 4,
    }
  }

  fn name(self) -> &'static str {
    match self {
      Self::U8 => "u8",
      Self::S16 => "s16",
      Self::S32 => "s32",
      Self::F32 => "f32",
    }
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum BuildError {
  Rate,
  Frames,
  Channels,
  TooSmall,
  Limits,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CopyError {
  Plane,
  Offset,
  Count,
}

#[derive(Clone, Debug)]
pub(crate) struct AudioSamples {
  pub kind: SampleKind,
  pub planar: bool,
  pub channels: u32,
  pub frames: u32,
  pub rate: f64,
  pub bytes: Vec<u8>,
}

/// `copyTo` and `allocationSize` options after dictionary conversion.
#[derive(Clone, Copy, Debug)]
pub(crate) struct CopyRequest {
  pub plane: u32,
  pub frame_offset: u32,
  pub frame_count: Option<u32>,
  pub format: Option<(SampleKind, bool)>,
}

pub(crate) fn parse_format(name: &str) -> Option<(SampleKind, bool)> {
  let (base, planar) = name
    .strip_suffix("-planar")
    .map(|base| (base, true))
    .unwrap_or((name, false));
  let kind = match base {
    "u8" => SampleKind::U8,
    "s16" => SampleKind::S16,
    "s32" => SampleKind::S32,
    "f32" => SampleKind::F32,
    _ => return None,
  };
  Some((kind, planar))
}

impl AudioSamples {
  pub(crate) fn from_pcm(
    kind: SampleKind,
    planar: bool,
    channels: u32,
    frames: u32,
    rate: f64,
    data: &[u8],
  ) -> Result<Self, BuildError> {
    if !rate.is_finite() || rate <= 0.0 {
      return Err(BuildError::Rate);
    }
    if frames == 0 {
      return Err(BuildError::Frames);
    }
    if channels == 0 {
      return Err(BuildError::Channels);
    }
    if frames > MAX_FRAMES || channels > MAX_CHANNELS {
      return Err(BuildError::Limits);
    }
    let needed = byte_len(kind, channels, frames).ok_or(BuildError::Limits)?;
    if data.len() < needed {
      return Err(BuildError::TooSmall);
    }
    Ok(Self {
      kind,
      planar,
      channels,
      frames,
      rate,
      bytes: data[..needed].to_vec(),
    })
  }

  /// Decoder output is planar `f32`. A short channel is padded with silence
  /// so a mismatched plane cannot panic.
  pub(crate) fn from_planar_f32(channels: Vec<Vec<f32>>, rate: f64) -> Self {
    let count = u32::try_from(channels.len()).unwrap_or(MAX_CHANNELS);
    let count = count.min(MAX_CHANNELS);
    let frames = channels
      .iter()
      .take(count as usize)
      .map(Vec::len)
      .min()
      .unwrap_or(0)
      .min(MAX_FRAMES as usize);
    let mut bytes = Vec::with_capacity(frames * count as usize * 4);
    for channel in channels.iter().take(count as usize) {
      for sample in channel.iter().take(frames) {
        bytes.extend_from_slice(&sample.to_le_bytes());
      }
      let missing = frames.saturating_sub(channel.len());
      bytes.extend(std::iter::repeat_n(0, missing * 4));
    }
    Self {
      kind: SampleKind::F32,
      planar: true,
      channels: count,
      frames: u32::try_from(frames).unwrap_or(0),
      rate,
      bytes,
    }
  }

  pub(crate) fn from_parts(
    kind: SampleKind,
    planar: bool,
    channels: u32,
    frames: u32,
    rate: f64,
    bytes: Vec<u8>,
  ) -> Option<Self> {
    if channels > MAX_CHANNELS || frames > MAX_FRAMES || !rate.is_finite() {
      return None;
    }
    let expected = byte_len(kind, channels, frames)?;
    if bytes.len() != expected {
      return None;
    }
    Some(Self {
      kind,
      planar,
      channels,
      frames,
      rate,
      bytes,
    })
  }

  pub(crate) fn format_name(&self) -> String {
    let base = self.kind.name();
    if self.planar {
      format!("{base}-planar")
    } else {
      base.to_string()
    }
  }

  pub(crate) fn code(&self) -> u32 {
    let kind = match self.kind {
      SampleKind::U8 => 0,
      SampleKind::S16 => 1,
      SampleKind::S32 => 2,
      SampleKind::F32 => 3,
    };
    if self.planar { kind | 0x10 } else { kind }
  }

  pub(crate) fn duration_us(&self) -> f64 {
    if self.rate <= 0.0 || !self.rate.is_finite() || self.frames == 0 {
      return 0.0;
    }
    let micros = f64::from(self.frames) / self.rate * 1_000_000.0;
    if !micros.is_finite() || micros <= 0.0 {
      return 0.0;
    }
    let micros = micros.trunc();
    if micros >= u64::MAX as f64 {
      u64::MAX as f64
    } else {
      micros
    }
  }

  pub(crate) fn allocation_size(&self, request: &CopyRequest) -> Result<u32, CopyError> {
    let bytes = self.window(request)?.2;
    u32::try_from(bytes).map_err(|_| CopyError::Count)
  }

  pub(crate) fn copy_to(&self, request: &CopyRequest) -> Result<Vec<u8>, CopyError> {
    let (count, kind, bytes) = self.window(request)?;
    let (_, planar) = request.format.unwrap_or((self.kind, self.planar));
    let mut dest = vec![0u8; bytes];
    if self.kind == kind {
      self.copy_same(planar, request.plane, request.frame_offset, count, &mut dest)?;
    } else {
      self.copy_convert(
        kind,
        planar,
        request.plane,
        request.frame_offset,
        count,
        &mut dest,
      )?;
    }
    Ok(dest)
  }

  /// `(frame count, destination kind, byte length)`.
  fn window(&self, request: &CopyRequest) -> Result<(u32, SampleKind, usize), CopyError> {
    let (kind, planar) = request.format.unwrap_or((self.kind, self.planar));
    if planar {
      if request.plane >= self.channels {
        return Err(CopyError::Plane);
      }
    } else if request.plane != 0 {
      return Err(CopyError::Plane);
    }
    if request.frame_offset >= self.frames {
      return Err(CopyError::Offset);
    }
    let available = self.frames - request.frame_offset;
    let count = match request.frame_count {
      Some(count) if count > available => return Err(CopyError::Count),
      Some(count) => count,
      None => available,
    };
    let samples = if planar {
      count
    } else {
      count.checked_mul(self.channels).ok_or(CopyError::Count)?
    };
    let bytes = (samples as usize)
      .checked_mul(kind.bytes())
      .ok_or(CopyError::Count)?;
    Ok((count, kind, bytes))
  }

  fn copy_same(
    &self,
    dest_planar: bool,
    plane: u32,
    frame_offset: u32,
    count: u32,
    dest: &mut [u8],
  ) -> Result<(), CopyError> {
    let width = self.kind.bytes();
    if !dest_planar && !self.planar {
      let start = sample_offset(frame_offset, self.channels, width)?;
      let len = sample_offset(count, self.channels, width)?;
      return copy_range(&self.bytes, start, dest, len);
    }
    if dest_planar && self.planar {
      let start = plane_offset(plane, self.frames, frame_offset, width)?;
      let len = (count as usize).checked_mul(width).ok_or(CopyError::Count)?;
      return copy_range(&self.bytes, start, dest, len);
    }
    if dest_planar {
      for index in 0..count {
        let frame = frame_offset + index;
        let from = self.sample_index(plane, frame)?;
        let to = (index as usize)
          .checked_mul(width)
          .ok_or(CopyError::Count)?;
        copy_sample(&self.bytes, from, dest, to, width)?;
      }
      return Ok(());
    }
    for index in 0..count {
      let frame = frame_offset + index;
      for channel in 0..self.channels {
        let from = self.sample_index(channel, frame)?;
        let slot = (index as usize)
          .checked_mul(self.channels as usize)
          .and_then(|value| value.checked_add(channel as usize))
          .ok_or(CopyError::Count)?;
        let to = slot.checked_mul(width).ok_or(CopyError::Count)?;
        copy_sample(&self.bytes, from, dest, to, width)?;
      }
    }
    Ok(())
  }

  fn copy_convert(
    &self,
    dest_kind: SampleKind,
    dest_planar: bool,
    plane: u32,
    frame_offset: u32,
    count: u32,
    dest: &mut [u8],
  ) -> Result<(), CopyError> {
    let write_one = |dest: &mut [u8], slot: usize, channel: u32, frame: u32| {
      let from = self.sample_index(channel, frame)?;
      let unit = read_unit(&self.bytes, self.kind, from)?;
      write_unit(dest, dest_kind, slot, unit)
    };
    if dest_planar {
      for index in 0..count {
        write_one(dest, index as usize, plane, frame_offset + index)?;
      }
      return Ok(());
    }
    for index in 0..count {
      for channel in 0..self.channels {
        let slot = (index as usize)
          .checked_mul(self.channels as usize)
          .and_then(|value| value.checked_add(channel as usize))
          .ok_or(CopyError::Count)?;
        write_one(dest, slot, channel, frame_offset + index)?;
      }
    }
    Ok(())
  }

  fn sample_index(&self, channel: u32, frame: u32) -> Result<usize, CopyError> {
    let index = if self.planar {
      (channel as usize)
        .checked_mul(self.frames as usize)
        .and_then(|value| value.checked_add(frame as usize))
    } else {
      (frame as usize)
        .checked_mul(self.channels as usize)
        .and_then(|value| value.checked_add(channel as usize))
    };
    index.ok_or(CopyError::Count)
  }
}

pub(crate) fn kind_from_code(code: u32) -> Option<(SampleKind, bool)> {
  if code & !0x13 != 0 {
    return None;
  }
  let planar = code & 0x10 != 0;
  let kind = match code & 0x03 {
    0 => SampleKind::U8,
    1 => SampleKind::S16,
    2 => SampleKind::S32,
    3 => SampleKind::F32,
    _ => return None,
  };
  Some((kind, planar))
}

fn byte_len(kind: SampleKind, channels: u32, frames: u32) -> Option<usize> {
  (frames as usize)
    .checked_mul(channels as usize)
    .and_then(|samples| samples.checked_mul(kind.bytes()))
}

fn sample_offset(frames: u32, channels: u32, width: usize) -> Result<usize, CopyError> {
  (frames as usize)
    .checked_mul(channels as usize)
    .and_then(|samples| samples.checked_mul(width))
    .ok_or(CopyError::Count)
}

fn plane_offset(
  plane: u32,
  frames: u32,
  frame_offset: u32,
  width: usize,
) -> Result<usize, CopyError> {
  (plane as usize)
    .checked_mul(frames as usize)
    .and_then(|start| start.checked_add(frame_offset as usize))
    .and_then(|sample| sample.checked_mul(width))
    .ok_or(CopyError::Count)
}

fn copy_range(src: &[u8], start: usize, dest: &mut [u8], len: usize) -> Result<(), CopyError> {
  let end = start.checked_add(len).ok_or(CopyError::Count)?;
  let from = src.get(start..end).ok_or(CopyError::Count)?;
  let into = dest.get_mut(..len).ok_or(CopyError::Count)?;
  into.copy_from_slice(from);
  Ok(())
}

fn copy_sample(
  src: &[u8],
  from: usize,
  dest: &mut [u8],
  to: usize,
  width: usize,
) -> Result<(), CopyError> {
  let from = from.checked_mul(width).ok_or(CopyError::Count)?;
  copy_range(src, from, dest.get_mut(to..).ok_or(CopyError::Count)?, width)
}

fn read_unit(bytes: &[u8], kind: SampleKind, index: usize) -> Result<f32, CopyError> {
  let start = index
    .checked_mul(kind.bytes())
    .ok_or(CopyError::Count)?;
  match kind {
    SampleKind::U8 => {
      let sample = *bytes.get(start).ok_or(CopyError::Count)?;
      Ok(u8_to_unit(sample))
    }
    SampleKind::S16 => {
      let slice = bytes.get(start..start + 2).ok_or(CopyError::Count)?;
      let mut array = [0u8; 2];
      array.copy_from_slice(slice);
      Ok(s16_to_unit(i16::from_le_bytes(array)))
    }
    SampleKind::S32 => {
      let slice = bytes.get(start..start + 4).ok_or(CopyError::Count)?;
      let mut array = [0u8; 4];
      array.copy_from_slice(slice);
      Ok(s32_to_unit(i32::from_le_bytes(array)))
    }
    SampleKind::F32 => {
      let slice = bytes.get(start..start + 4).ok_or(CopyError::Count)?;
      let mut array = [0u8; 4];
      array.copy_from_slice(slice);
      Ok(f32::from_le_bytes(array))
    }
  }
}

fn write_unit(
  dest: &mut [u8],
  kind: SampleKind,
  index: usize,
  unit: f32,
) -> Result<(), CopyError> {
  let start = index
    .checked_mul(kind.bytes())
    .ok_or(CopyError::Count)?;
  match kind {
    SampleKind::U8 => {
      *dest.get_mut(start).ok_or(CopyError::Count)? = unit_to_u8(unit);
    }
    SampleKind::S16 => {
      let slice = dest.get_mut(start..start + 2).ok_or(CopyError::Count)?;
      slice.copy_from_slice(&unit_to_s16(unit).to_le_bytes());
    }
    SampleKind::S32 => {
      let slice = dest.get_mut(start..start + 4).ok_or(CopyError::Count)?;
      slice.copy_from_slice(&unit_to_s32(unit).to_le_bytes());
    }
    SampleKind::F32 => {
      let slice = dest.get_mut(start..start + 4).ok_or(CopyError::Count)?;
      slice.copy_from_slice(&unit.to_le_bytes());
    }
  }
  Ok(())
}

fn u8_to_unit(sample: u8) -> f32 {
  let offset = f32::from(sample) - 128.0;
  if offset < 0.0 {
    offset * (1.0 / 128.0)
  } else {
    offset * (1.0 / 127.0)
  }
}

fn s16_to_unit(sample: i16) -> f32 {
  let offset = f32::from(sample);
  if offset < 0.0 {
    offset * (1.0 / 32768.0)
  } else {
    offset * (1.0 / 32767.0)
  }
}

fn s32_to_unit(sample: i32) -> f32 {
  // `f32` rounds `i32::MAX` to `2^31`, so both sides share that divisor.
  (sample as f32) * (1.0 / 2147483648.0)
}

fn unit_to_u8(unit: f32) -> u8 {
  if unit < 0.0 {
    if unit <= -1.0 {
      return 0;
    }
    return (unit * 128.0 + 128.0) as u8;
  }
  if unit >= 1.0 {
    return 255;
  }
  (unit * 127.0 + 128.0) as u8
}

fn unit_to_s16(unit: f32) -> i16 {
  if unit < 0.0 {
    if unit <= -1.0 {
      return i16::MIN;
    }
    return (unit * 32768.0) as i16;
  }
  if unit >= 1.0 {
    return i16::MAX;
  }
  (unit * 32767.0) as i16
}

fn unit_to_s32(unit: f32) -> i32 {
  if unit < 0.0 {
    if unit <= -1.0 {
      return i32::MIN;
    }
    return (unit * 2147483648.0) as i32;
  }
  if unit >= 1.0 {
    return i32::MAX;
  }
  (unit * 2147483648.0) as i32
}

#[cfg(test)]
mod tests {
  use super::*;

  fn values(kind: &str) -> [f64; 6] {
    match kind {
      "u8" => [0.0, 255.0, 191.0, 64.0, 128.0, 256.0],
      "s16" => [-32768.0, 32767.0, 16383.0, -16384.0, 0.0, 65536.0],
      "s32" => [
        -2147483648.0,
        2147483647.0,
        1073741823.0,
        -1073741824.0,
        0.0,
        4294967296.0,
      ],
      "f32" => [-1.0, 1.0, 0.5, -0.5, 0.0, 16777216.0],
      _ => unreachable!(),
    }
  }

  fn epsilon(expected: f64, source: &str, dest: &str) -> f64 {
    if source == dest {
      return 0.0;
    }
    let dest_values = values(dest);
    if expected == dest_values[1] {
      if source == "u8" && dest == "s16" {
        return expected - 32511.0;
      }
      if source == "u8" && dest == "s32" {
        return expected - 2_130_706_432.0;
      }
      if source == "s16" && dest == "s32" {
        return expected - 2_147_418_112.0;
      }
    }
    let integer = |name: &str| name != "f32";
    if integer(source)
      && integer(dest)
      && (expected == dest_values[0] || expected == dest_values[4])
    {
      return 0.0;
    }
    if source == "f32" && integer(dest) && dest != "s32" {
      return 1.0;
    }
    let source_res = values(source)[5];
    let dest_res = dest_values[5];
    if source_res > dest_res {
      0.0
    } else {
      dest_res / source_res
    }
  }

  fn pack(kind: SampleKind, samples: &[f64]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for sample in samples {
      match kind {
        SampleKind::U8 => bytes.push(*sample as u8),
        SampleKind::S16 => bytes.extend_from_slice(&(*sample as i16).to_le_bytes()),
        SampleKind::S32 => bytes.extend_from_slice(&(*sample as i32).to_le_bytes()),
        SampleKind::F32 => bytes.extend_from_slice(&(*sample as f32).to_le_bytes()),
      }
    }
    bytes
  }

  fn unpack(kind: SampleKind, bytes: &[u8]) -> Vec<f64> {
    let width = kind.bytes();
    bytes
      .chunks(width)
      .map(|chunk| match kind {
        SampleKind::U8 => f64::from(chunk[0]),
        SampleKind::S16 => f64::from(i16::from_le_bytes(chunk.try_into().unwrap())),
        SampleKind::S32 => f64::from(i32::from_le_bytes(chunk.try_into().unwrap())),
        SampleKind::F32 => f64::from(f32::from_le_bytes(chunk.try_into().unwrap())),
      })
      .collect()
  }

  fn kind_of(name: &str) -> SampleKind {
    parse_format(name).unwrap().0
  }

  fn assert_close(actual: &[f64], expected: &[f64], source: &str, dest: &str, label: &str) {
    assert_eq!(actual.len(), expected.len(), "{label} length");
    for (index, (got, want)) in actual.iter().zip(expected).enumerate() {
      let eps = epsilon(*want, source, dest);
      assert!(
        (got - want).abs() <= eps,
        "{source}->{dest} {label}[{index}] got {got} want {want} eps {eps}",
      );
    }
  }

  #[test]
  fn wpt_conversion_matrix() {
    let input_index = [0, 4, 1, 0, 2, 3, 4, 1, 4, 4];
    let plane0 = [0, 1, 2, 4, 4];
    let plane1 = [4, 0, 3, 1, 4];
    let planar0 = [0, 4, 1, 0, 2];
    let planar1 = [3, 4, 1, 4, 4];
    let interleaved = [0, 3, 4, 4, 1, 1, 0, 4, 2, 4];
    for source_name in ["u8", "s16", "s32", "f32"] {
      for dest_name in ["u8", "s16", "s32", "f32"] {
        let source_kind = kind_of(source_name);
        let dest_kind = kind_of(dest_name);
        let source_values = values(source_name);
        let dest_values = values(dest_name);
        let input: Vec<f64> = input_index.iter().map(|index| source_values[*index]).collect();
        let stored = AudioSamples::from_pcm(
          source_kind,
          false,
          2,
          5,
          8000.0,
          &pack(source_kind, &input),
        )
        .unwrap();
        let request = |plane, planar| CopyRequest {
          plane,
          frame_offset: 0,
          frame_count: None,
          format: Some((dest_kind, planar)),
        };
        let left = unpack(dest_kind, &stored.copy_to(&request(0, true)).unwrap());
        let right = unpack(dest_kind, &stored.copy_to(&request(1, true)).unwrap());
        let both = unpack(dest_kind, &stored.copy_to(&request(0, false)).unwrap());
        let expect = |indexes: &[usize]| -> Vec<f64> {
          indexes.iter().map(|index| dest_values[*index]).collect()
        };
        assert_close(&left, &expect(&plane0), source_name, dest_name, "interleaved plane 0");
        assert_close(&right, &expect(&plane1), source_name, dest_name, "interleaved plane 1");
        assert_close(&both, &expect(&input_index), source_name, dest_name, "interleaved");

        let planar = AudioSamples::from_pcm(
          source_kind,
          true,
          2,
          5,
          8000.0,
          &pack(source_kind, &input),
        )
        .unwrap();
        let left = unpack(dest_kind, &planar.copy_to(&request(0, true)).unwrap());
        let right = unpack(dest_kind, &planar.copy_to(&request(1, true)).unwrap());
        let both = unpack(dest_kind, &planar.copy_to(&request(0, false)).unwrap());
        assert_close(&left, &expect(&planar0), source_name, dest_name, "planar plane 0");
        assert_close(&right, &expect(&planar1), source_name, dest_name, "planar plane 1");
        assert_close(&both, &expect(&interleaved), source_name, dest_name, "planar to interleaved");
      }
    }
  }

  #[test]
  fn same_format_keeps_values_outside_the_unit_interval() {
    let samples = AudioSamples::from_pcm(
      SampleKind::F32,
      false,
      1,
      2,
      48000.0,
      &pack(SampleKind::F32, &[2.0, -2.0]),
    )
    .unwrap();
    let copy = samples
      .copy_to(&CopyRequest {
        plane: 0,
        frame_offset: 0,
        frame_count: None,
        format: None,
      })
      .unwrap();
    assert_eq!(unpack(SampleKind::F32, &copy), vec![2.0, -2.0]);
    let clipped = samples
      .copy_to(&CopyRequest {
        plane: 0,
        frame_offset: 0,
        frame_count: None,
        format: Some((SampleKind::S16, false)),
      })
      .unwrap();
    assert_eq!(unpack(SampleKind::S16, &clipped), vec![32767.0, -32768.0]);
  }

  #[test]
  fn window_and_duration() {
    let samples = AudioSamples::from_pcm(
      SampleKind::S16,
      false,
      2,
      3,
      8000.0,
      &pack(SampleKind::S16, &[0.0, 1000.0, -1000.0, 2000.0, 3000.0, -3000.0]),
    )
    .unwrap();
    assert_eq!(samples.duration_us(), 375.0);
    let window = samples
      .copy_to(&CopyRequest {
        plane: 0,
        frame_offset: 1,
        frame_count: Some(2),
        format: Some((SampleKind::S16, true)),
      })
      .unwrap();
    assert_eq!(unpack(SampleKind::S16, &window), vec![-1000.0, 3000.0]);
    assert_eq!(
      samples.allocation_size(&CopyRequest {
        plane: 0,
        frame_offset: 1,
        frame_count: Some(2),
        format: Some((SampleKind::F32, false)),
      }),
      Ok(16),
    );
    assert_eq!(
      samples.copy_to(&CopyRequest {
        plane: 1,
        frame_offset: 0,
        frame_count: None,
        format: None,
      }),
      Err(CopyError::Plane),
    );
    assert_eq!(
      samples.copy_to(&CopyRequest {
        plane: 0,
        frame_offset: 3,
        frame_count: None,
        format: None,
      }),
      Err(CopyError::Offset),
    );
  }

  #[test]
  fn init_rejects_empty_and_short_buffers() {
    let err = AudioSamples::from_pcm(SampleKind::F32, true, 1, 0, 8000.0, &[]);
    assert_eq!(err.err(), Some(BuildError::Frames));
    let err = AudioSamples::from_pcm(SampleKind::U8, false, 1, 2, 8000.0, &[0]);
    assert_eq!(err.err(), Some(BuildError::TooSmall));
    let err = AudioSamples::from_pcm(SampleKind::U8, false, 1, 1, 0.0, &[0]);
    assert_eq!(err.err(), Some(BuildError::Rate));
  }
}
