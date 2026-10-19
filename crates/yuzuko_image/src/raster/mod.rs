//! Bitmap storage, geometry, color conversion, and decoding.

pub(crate) mod color;
pub(crate) mod decode;

use half::f16;

pub const MAX_EDGE: u32 = 16_384;
pub const MAX_PIXELS: u64 = 100_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColorSpace {
  Srgb,
  DisplayP3,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SampleFormat {
  Unorm8,
  Float16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResizeQuality {
  Nearest,
  Triangle,
}

#[derive(Clone, Debug)]
pub struct Raster {
  pub width: u32,
  pub height: u32,
  pub color_space: ColorSpace,
  pub format: SampleFormat,
  pub premultiplied: bool,
  /// RGBA, tightly packed. Unorm8 is one byte per channel. Float16 is
  /// little-endian binary16.
  pub bytes: Vec<u8>,
}

#[derive(Clone, Copy, Debug)]
pub struct Crop {
  pub sx: i32,
  pub sy: i32,
  pub sw: i32,
  pub sh: i32,
}

#[derive(Debug, PartialEq, Eq)]
pub enum PixelError {
  TooLarge,
  Empty,
}

pub fn within_limits(width: u32, height: u32) -> bool {
  width > 0
    && height > 0
    && width <= MAX_EDGE
    && height <= MAX_EDGE
    && u64::from(width) * u64::from(height) <= MAX_PIXELS
}

impl Raster {
  pub fn new(
    width: u32,
    height: u32,
    color_space: ColorSpace,
    format: SampleFormat,
  ) -> Result<Self, PixelError> {
    if !within_limits(width, height) {
      return Err(PixelError::TooLarge);
    }
    let channels = u64::from(width) * u64::from(height) * 4;
    let bytes_len = match format {
      SampleFormat::Unorm8 => channels,
      SampleFormat::Float16 => channels * 2,
    };
    let len = usize::try_from(bytes_len).map_err(|_| PixelError::TooLarge)?;
    Ok(Self {
      width,
      height,
      color_space,
      format,
      premultiplied: false,
      bytes: vec![0; len],
    })
  }

  pub fn from_rgba8(
    width: u32,
    height: u32,
    color_space: ColorSpace,
    rgba: &[u8],
  ) -> Result<Self, PixelError> {
    if rgba.len() != width as usize * height as usize * 4 {
      return Err(PixelError::Empty);
    }
    if !within_limits(width, height) {
      return Err(PixelError::TooLarge);
    }
    Ok(Self {
      width,
      height,
      color_space,
      format: SampleFormat::Unorm8,
      premultiplied: false,
      bytes: rgba.to_vec(),
    })
  }

  pub fn from_float16(
    width: u32,
    height: u32,
    color_space: ColorSpace,
    samples: &[f64],
  ) -> Result<Self, PixelError> {
    if samples.len() != width as usize * height as usize * 4 {
      return Err(PixelError::Empty);
    }
    let mut raster =
      Self::new(width, height, color_space, SampleFormat::Float16)?;
    for (index, sample) in samples.iter().enumerate() {
      raster.put_channel(index, *sample);
    }
    Ok(raster)
  }

  #[inline]
  fn stride(&self) -> usize {
    match self.format {
      SampleFormat::Unorm8 => 1,
      SampleFormat::Float16 => 2,
    }
  }

  #[inline]
  pub(crate) fn channel(&self, index: usize) -> f64 {
    let offset = index * self.stride();
    match self.format {
      SampleFormat::Unorm8 => f64::from(self.bytes[offset]),
      SampleFormat::Float16 => {
        let bits =
          u16::from_le_bytes([self.bytes[offset], self.bytes[offset + 1]]);
        f64::from(f16::from_bits(bits))
      }
    }
  }

  #[inline]
  pub(crate) fn put_channel(&mut self, index: usize, value: f64) {
    let offset = index * self.stride();
    match self.format {
      SampleFormat::Unorm8 => {
        let rounded = value.round().clamp(0.0, 255.0);
        self.bytes[offset] = rounded as u8;
      }
      SampleFormat::Float16 => {
        let bits = f16::from_f64(value).to_bits();
        let bytes = bits.to_le_bytes();
        self.bytes[offset] = bytes[0];
        self.bytes[offset + 1] = bytes[1];
      }
    }
  }

  #[inline]
  pub fn sample(&self, x: i64, y: i64) -> [f64; 4] {
    if x < 0
      || y < 0
      || x >= i64::from(self.width)
      || y >= i64::from(self.height)
    {
      return [0.0; 4];
    }
    let pixel = (y as u32 * self.width + x as u32) as usize * 4;
    [
      self.channel(pixel),
      self.channel(pixel + 1),
      self.channel(pixel + 2),
      self.channel(pixel + 3),
    ]
  }

  #[inline]
  pub fn write_sample(&mut self, x: i64, y: i64, pixel: [f64; 4]) {
    if x < 0
      || y < 0
      || x >= i64::from(self.width)
      || y >= i64::from(self.height)
    {
      return;
    }
    self.write_pixel(x as u32, y as u32, pixel);
  }

  #[inline]
  fn write_pixel(&mut self, x: u32, y: u32, pixel: [f64; 4]) {
    let index = (y * self.width + x) as usize * 4;
    for (offset, channel) in pixel.into_iter().enumerate() {
      self.put_channel(index + offset, channel);
    }
  }

  #[cfg_attr(not(test), allow(dead_code))]
  #[inline]
  pub fn pixel_u8(&self, x: u32, y: u32) -> [u8; 4] {
    let sample = self.sample(i64::from(x), i64::from(y));
    match self.format {
      SampleFormat::Unorm8 => sample.map(|channel| channel as u8),
      SampleFormat::Float16 => {
        sample.map(|channel| (channel * 255.0).round().clamp(0.0, 255.0) as u8)
      }
    }
  }

  #[cfg_attr(not(test), allow(dead_code))]
  #[inline]
  pub fn pixel_f64(&self, x: u32, y: u32) -> [f64; 4] {
    self.sample(i64::from(x), i64::from(y))
  }
}

pub fn orient(raster: Raster, orientation: u8) -> Result<Raster, PixelError> {
  let (width, height) = match orientation {
    5..=8 => (raster.height, raster.width),
    _ => (raster.width, raster.height),
  };
  if orientation <= 1 {
    return Ok(raster);
  }
  let mut output =
    Raster::new(width, height, raster.color_space, raster.format)?;
  output.premultiplied = raster.premultiplied;
  for y in 0..height {
    for x in 0..width {
      let (sx, sy) = source_of(orientation, x, y, raster.width, raster.height);
      output.write_pixel(x, y, raster.sample(i64::from(sx), i64::from(sy)));
    }
  }
  Ok(output)
}

/// Source pixel copied into output `(x, y)` for an EXIF orientation.
fn source_of(
  orientation: u8,
  x: u32,
  y: u32,
  width: u32,
  height: u32,
) -> (u32, u32) {
  let last_x = width.saturating_sub(1);
  let last_y = height.saturating_sub(1);
  match orientation {
    2 => (last_x - x, y),
    3 => (last_x - x, last_y - y),
    4 => (x, last_y - y),
    5 => (y, x),
    6 => (y, last_y - x),
    7 => (last_x - y, last_y - x),
    8 => (last_x - y, x),
    _ => (x, y),
  }
}

pub fn flip_y(raster: Raster) -> Result<Raster, PixelError> {
  orient(raster, 4)
}

pub fn crop_resize(
  raster: &Raster,
  crop: Option<Crop>,
  resize_width: Option<u32>,
  resize_height: Option<u32>,
  quality: ResizeQuality,
) -> Result<Raster, PixelError> {
  let (origin_x, origin_y, crop_w, crop_h) = match crop {
    Some(crop) => normalize_crop(crop)?,
    None => (0, 0, raster.width, raster.height),
  };
  if crop_w == 0 || crop_h == 0 {
    return Err(PixelError::Empty);
  }
  let (width, height) =
    output_size(crop_w, crop_h, resize_width, resize_height)?;
  if !within_limits(width, height) {
    return Err(PixelError::TooLarge);
  }
  let mut output =
    Raster::new(width, height, raster.color_space, raster.format)?;
  output.premultiplied = raster.premultiplied;
  for y in 0..height {
    for x in 0..width {
      let pixel = match quality {
        ResizeQuality::Nearest => {
          let sx = origin_x + nearest_index(x, width, crop_w);
          let sy = origin_y + nearest_index(y, height, crop_h);
          raster.sample(sx, sy)
        }
        ResizeQuality::Triangle => triangle_sample(
          raster,
          Triangle {
            origin_x,
            origin_y,
            crop_w,
            crop_h,
            dest_w: width,
            dest_h: height,
          },
          x,
          y,
        ),
      };
      output.write_pixel(x, y, pixel);
    }
  }
  Ok(output)
}

fn normalize_crop(crop: Crop) -> Result<(i64, i64, u32, u32), PixelError> {
  let mut x = i64::from(crop.sx);
  let mut y = i64::from(crop.sy);
  let mut w = i64::from(crop.sw);
  let mut h = i64::from(crop.sh);
  if w < 0 {
    x += w;
    w = -w;
  }
  if h < 0 {
    y += h;
    h = -h;
  }
  if w == 0 || h == 0 {
    return Err(PixelError::Empty);
  }
  let Ok(width) = u32::try_from(w) else {
    return Err(PixelError::TooLarge);
  };
  let Ok(height) = u32::try_from(h) else {
    return Err(PixelError::TooLarge);
  };
  Ok((x, y, width, height))
}

fn output_size(
  crop_w: u32,
  crop_h: u32,
  resize_width: Option<u32>,
  resize_height: Option<u32>,
) -> Result<(u32, u32), PixelError> {
  let width = match (resize_width, resize_height) {
    (Some(width), _) => width,
    (None, Some(height)) => scale_ceil(crop_w, height, crop_h)?,
    (None, None) => crop_w,
  };
  let height = match (resize_width, resize_height) {
    (_, Some(height)) => height,
    (Some(width), None) => scale_ceil(crop_h, width, crop_w)?,
    (None, None) => crop_h,
  };
  if width == 0 || height == 0 {
    return Err(PixelError::Empty);
  }
  Ok((width, height))
}

fn scale_ceil(length: u32, numer: u32, denom: u32) -> Result<u32, PixelError> {
  if denom == 0 {
    return Err(PixelError::Empty);
  }
  let value = u64::from(length) * u64::from(numer);
  let rounded = value.div_ceil(u64::from(denom));
  u32::try_from(rounded).map_err(|_| PixelError::TooLarge)
}

fn nearest_index(dest: u32, dest_len: u32, src_len: u32) -> i64 {
  let pos = (f64::from(dest) + 0.5) * f64::from(src_len) / f64::from(dest_len);
  pos.floor() as i64
}

struct Triangle {
  origin_x: i64,
  origin_y: i64,
  crop_w: u32,
  crop_h: u32,
  dest_w: u32,
  dest_h: u32,
}

fn triangle_sample(
  raster: &Raster,
  window: Triangle,
  x: u32,
  y: u32,
) -> [f64; 4] {
  let xs = triangle_weights(x, window.dest_w, window.crop_w);
  let ys = triangle_weights(y, window.dest_h, window.crop_h);
  let mut acc = [0.0; 4];
  for (sy, wy) in &ys {
    for (sx, wx) in &xs {
      let weight = wy * wx;
      let sample = raster.sample(window.origin_x + sx, window.origin_y + sy);
      for channel in 0..4 {
        acc[channel] += sample[channel] * weight;
      }
    }
  }
  acc
}

fn triangle_weights(dest: u32, dest_len: u32, src_len: u32) -> Vec<(i64, f64)> {
  let scale = f64::from(src_len) / f64::from(dest_len);
  let support = scale.max(1.0);
  let center = (f64::from(dest) + 0.5) * scale - 0.5;
  let start = (center - support).floor() as i64;
  let end = (center + support).ceil() as i64;
  let mut weights = Vec::new();
  let mut sum = 0.0;
  for index in start..=end {
    let distance = (index as f64 - center).abs() / support;
    if distance < 1.0 {
      let weight = 1.0 - distance;
      weights.push((index, weight));
      sum += weight;
    }
  }
  if sum > 0.0 {
    for weight in &mut weights {
      weight.1 /= sum;
    }
  }
  weights
}

pub fn premultiply(raster: &mut Raster) {
  if raster.premultiplied {
    return;
  }
  map_alpha(raster, true);
  raster.premultiplied = true;
}

pub fn unpremultiply(raster: &mut Raster) {
  if !raster.premultiplied {
    return;
  }
  map_alpha(raster, false);
  raster.premultiplied = false;
}

fn map_alpha(raster: &mut Raster, multiply: bool) {
  let count = raster.width as usize * raster.height as usize;
  for pixel in 0..count {
    let index = pixel * 4;
    let alpha = raster.channel(index + 3);
    for channel in 0..3 {
      let color = raster.channel(index + channel);
      let next = if raster.format == SampleFormat::Unorm8 {
        let alpha_u = alpha as u8;
        let color_u = color as u8;
        f64::from(if multiply {
          premul_u8(color_u, alpha_u)
        } else {
          unpremul_u8(color_u, alpha_u)
        })
      } else if multiply {
        color * alpha
      } else if alpha == 0.0 {
        0.0
      } else {
        color / alpha
      };
      raster.put_channel(index + channel, next);
    }
  }
}

fn premul_u8(color: u8, alpha: u8) -> u8 {
  ((u16::from(color) * u16::from(alpha) + 127) / 255) as u8
}

fn unpremul_u8(color: u8, alpha: u8) -> u8 {
  if alpha == 0 {
    0
  } else {
    ((u16::from(color) * 255 + u16::from(alpha) / 2) / u16::from(alpha))
      .min(255) as u8
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn exif_orientation_6_rotates_clockwise() {
    let raster = Raster::from_rgba8(
      2,
      1,
      ColorSpace::Srgb,
      &[255, 0, 0, 255, 0, 255, 0, 255],
    )
    .unwrap();
    let turned = orient(raster, 6).unwrap();
    assert_eq!((turned.width, turned.height), (1, 2));
    assert_eq!(turned.pixel_u8(0, 0), [255, 0, 0, 255]);
    assert_eq!(turned.pixel_u8(0, 1), [0, 255, 0, 255]);
  }

  #[test]
  fn negative_crop_normalizes_and_pads_transparent() {
    let raster =
      Raster::from_rgba8(1, 1, ColorSpace::Srgb, &[10, 20, 30, 255]).unwrap();
    let cropped = crop_resize(
      &raster,
      Some(Crop {
        sx: 1,
        sy: 1,
        sw: -2,
        sh: -2,
      }),
      None,
      None,
      ResizeQuality::Nearest,
    )
    .unwrap();
    assert_eq!((cropped.width, cropped.height), (2, 2));
    assert_eq!(cropped.pixel_u8(0, 0), [0, 0, 0, 0]);
    assert_eq!(cropped.pixel_u8(1, 1), [10, 20, 30, 255]);
  }

  #[test]
  fn resize_rejects_an_edge_past_the_limit() {
    let raster =
      Raster::from_rgba8(1, 1, ColorSpace::Srgb, &[0, 0, 0, 255]).unwrap();
    let error = crop_resize(
      &raster,
      None,
      Some(MAX_EDGE + 1),
      Some(1),
      ResizeQuality::Nearest,
    );
    assert_eq!(error.err(), Some(PixelError::TooLarge));
  }
}
