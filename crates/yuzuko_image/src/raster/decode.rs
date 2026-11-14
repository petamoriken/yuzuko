//! Signature sniffing and the image decoders.

use std::io::Cursor;
use std::panic::AssertUnwindSafe;

use zune_core::colorspace::ColorSpace;
use zune_core::options::DecoderOptions;
use zune_jpeg::JpegDecoder;

use crate::raster::ColorSpace as StoredSpace;
use crate::raster::Raster;
use crate::raster::SampleFormat;
use crate::raster::color::Cicp;
use crate::raster::color::apply_source;
use crate::raster::within_limits;

#[path = "animation.rs"]
mod animation;

#[derive(Debug, PartialEq, Eq)]
pub enum DecodeError {
  Unsupported,
  Corrupt,
  TooLarge,
}

pub struct Decoded {
  pub raster: Raster,
  /// EXIF orientation 1 through 8. JPEG XL is already oriented, so this is 1.
  pub orientation: u8,
  /// AVIF color planes. Absent for every other format.
  pub yuv: Option<crate::av1::Av1Picture>,
}

pub use animation::{ImageFrame, ImageFrames};

pub fn decode(bytes: &[u8], to_srgb: bool) -> Result<Decoded, DecodeError> {
  let caught =
    std::panic::catch_unwind(AssertUnwindSafe(|| decode_inner(bytes, to_srgb)));
  match caught {
    Ok(result) => result,
    Err(_) => Err(DecodeError::Corrupt),
  }
}

/// Every frame of a PNG, GIF, WebP, or JPEG XL animation.
///
/// AVIF and BMP are one still frame. An animated AVIF is not a frame sequence.
///
/// A still image is one frame with `animated` false, `repetition_count` 0,
/// and `duration_us` none. `createImageBitmap` keeps using [`decode`].
pub fn decode_frames(
  bytes: &[u8],
  to_srgb: bool,
) -> Result<ImageFrames, DecodeError> {
  let caught = std::panic::catch_unwind(AssertUnwindSafe(|| {
    animation::decode_all(bytes, to_srgb)
  }));
  match caught {
    Ok(result) => result,
    Err(_) => Err(DecodeError::Corrupt),
  }
}

fn decode_inner(bytes: &[u8], to_srgb: bool) -> Result<Decoded, DecodeError> {
  match sniff(bytes) {
    Some(Kind::Png) => decode_png(bytes, to_srgb),
    Some(Kind::Jpeg) => decode_jpeg(bytes, to_srgb),
    Some(Kind::Webp) => decode_webp(bytes, to_srgb),
    Some(Kind::Jxl) => decode_jxl(bytes, to_srgb),
    Some(Kind::Avif) => decode_avif(bytes, to_srgb),
    Some(Kind::Gif) => animation::decode_gif(bytes, to_srgb),
    Some(Kind::Bmp) => decode_bmp(bytes, to_srgb),
    None => Err(DecodeError::Unsupported),
  }
}

enum Kind {
  Png,
  Jpeg,
  Webp,
  Jxl,
  Avif,
  Gif,
  Bmp,
}

fn sniff(bytes: &[u8]) -> Option<Kind> {
  if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
    Some(Kind::Png)
  } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
    Some(Kind::Jpeg)
  } else if bytes.len() >= 12
    && &bytes[..4] == b"RIFF"
    && &bytes[8..12] == b"WEBP"
  {
    Some(Kind::Webp)
  } else if is_jxl(bytes) {
    Some(Kind::Jxl)
  } else if is_avif(bytes) {
    Some(Kind::Avif)
  } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
    Some(Kind::Gif)
  } else if bytes.starts_with(b"BM") {
    Some(Kind::Bmp)
  } else {
    None
  }
}

fn is_avif(bytes: &[u8]) -> bool {
  if bytes.len() < 16 || &bytes[4..8] != b"ftyp" {
    return false;
  }
  let size =
    u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize;
  let end = size.min(bytes.len());
  if end < 16 {
    return false;
  }
  bytes[8..end]
    .as_chunks::<4>()
    .0
    .iter()
    .enumerate()
    .any(|(index, brand)| index != 1 && (brand == b"avif" || brand == b"avis"))
}

fn decode_avif(bytes: &[u8], to_srgb: bool) -> Result<Decoded, DecodeError> {
  let mut image = crate::av1::decode_avif(bytes).map_err(av1_error)?;
  let mut decoded = finish(&mut image.picture.display, None, None, to_srgb, 1)?;
  image.picture.display = decoded.raster.clone();
  decoded.yuv = Some(image.picture);
  Ok(decoded)
}

fn av1_error(error: crate::av1::Av1Error) -> DecodeError {
  match error {
    crate::av1::Av1Error::Unsupported => DecodeError::Unsupported,
    crate::av1::Av1Error::TooLarge => DecodeError::TooLarge,
    crate::av1::Av1Error::Corrupt => DecodeError::Corrupt,
  }
}

fn is_jxl(bytes: &[u8]) -> bool {
  if bytes.starts_with(&[0xff, 0x0a]) {
    return true;
  }
  bytes.len() >= 12 && &bytes[4..8] == b"ftyp" && &bytes[8..12] == b"jxl "
}

fn finish(
  raster: &mut Raster,
  icc: Option<&[u8]>,
  cicp: Option<Cicp>,
  to_srgb: bool,
  orientation: u8,
) -> Result<Decoded, DecodeError> {
  apply_source(raster, icc, cicp, to_srgb).map_err(|_| DecodeError::Corrupt)?;
  Ok(Decoded {
    raster: std::mem::replace(
      raster,
      Raster::new(1, 1, StoredSpace::Srgb, SampleFormat::Unorm8)
        .map_err(|_| DecodeError::TooLarge)?,
    ),
    orientation,
    yuv: None,
  })
}

fn decode_png(bytes: &[u8], to_srgb: bool) -> Result<Decoded, DecodeError> {
  let mut decoder = png::Decoder::new(Cursor::new(bytes));
  decoder.set_limits(png::Limits { bytes: 800_000_000 });
  decoder.set_transformations(png::Transformations::EXPAND);
  let mut reader = decoder.read_info().map_err(|_| DecodeError::Corrupt)?;
  let (width, height, cicp, icc, srgb) = {
    let info = reader.info();
    if !within_limits(info.width, info.height) {
      return Err(DecodeError::TooLarge);
    }
    let cicp = info.coding_independent_code_points.map(|points| Cicp {
      primaries: points.color_primaries,
      transfer: points.transfer_function,
    });
    let icc = info
      .icc_profile
      .as_ref()
      .map(|profile| profile.clone().into_owned());
    (info.width, info.height, cicp, icc, info.srgb.is_some())
  };
  let size = reader.output_buffer_size().ok_or(DecodeError::TooLarge)?;
  let mut buffer = vec![0; size];
  let output = reader
    .next_frame(&mut buffer)
    .map_err(|_| DecodeError::Corrupt)?;
  let pixels = &buffer[..output.buffer_size()];
  let mut raster = png_raster(
    output.width,
    output.height,
    output.color_type,
    output.bit_depth,
    pixels,
  )?;
  let _ = (width, height);
  let cicp = if srgb {
    Some(Cicp {
      primaries: 1,
      transfer: 13,
    })
  } else {
    cicp
  };
  let icc = if srgb { None } else { icc };
  finish(
    &mut raster,
    icc.as_deref(),
    cicp,
    to_srgb,
    exif_orientation(bytes),
  )
}

fn png_raster(
  width: u32,
  height: u32,
  color: png::ColorType,
  depth: png::BitDepth,
  pixels: &[u8],
) -> Result<Raster, DecodeError> {
  let sixteen = depth == png::BitDepth::Sixteen;
  if sixteen {
    let samples = be_u16(pixels);
    let rgba = expand_u16(color, &samples)?;
    let floats: Vec<f64> = rgba
      .into_iter()
      .map(|sample| f64::from(sample) / 65535.0)
      .collect();
    Raster::from_float16(width, height, StoredSpace::Srgb, &floats)
      .map_err(pixel_error)
  } else {
    let rgba = expand_u8(color, pixels)?;
    Raster::from_rgba8(width, height, StoredSpace::Srgb, &rgba)
      .map_err(pixel_error)
  }
}

fn be_u16(bytes: &[u8]) -> Vec<u16> {
  bytes
    .as_chunks::<2>()
    .0
    .iter()
    .map(|pair| u16::from_be_bytes(*pair))
    .collect()
}

fn expand_u8(
  color: png::ColorType,
  pixels: &[u8],
) -> Result<Vec<u8>, DecodeError> {
  match color {
    png::ColorType::Rgba => Ok(pixels.to_vec()),
    png::ColorType::Rgb => Ok(insert_alpha_u8(pixels, 3)),
    png::ColorType::GrayscaleAlpha => Ok(gray_alpha_u8(pixels)),
    png::ColorType::Grayscale => {
      Ok(insert_alpha_u8(pixels, 1).into_iter().collect())
    }
    png::ColorType::Indexed => Err(DecodeError::Corrupt),
  }
}

fn expand_u16(
  color: png::ColorType,
  pixels: &[u16],
) -> Result<Vec<u16>, DecodeError> {
  match color {
    png::ColorType::Rgba => Ok(pixels.to_vec()),
    png::ColorType::Rgb => Ok(insert_alpha_u16(pixels, 3)),
    png::ColorType::GrayscaleAlpha => Ok(gray_alpha_u16(pixels)),
    png::ColorType::Grayscale => Ok(insert_alpha_u16(pixels, 1)),
    png::ColorType::Indexed => Err(DecodeError::Corrupt),
  }
}

fn insert_alpha_u8(pixels: &[u8], channels: usize) -> Vec<u8> {
  let mut out = Vec::with_capacity(pixels.len() / channels * 4);
  for pixel in pixels.chunks(channels) {
    if channels == 1 {
      out.extend_from_slice(&[pixel[0], pixel[0], pixel[0], 255]);
    } else {
      out.extend_from_slice(&[pixel[0], pixel[1], pixel[2], 255]);
    }
  }
  out
}

fn gray_alpha_u8(pixels: &[u8]) -> Vec<u8> {
  let mut out = Vec::with_capacity(pixels.len() / 2 * 4);
  for pixel in pixels.as_chunks::<2>().0 {
    out.extend_from_slice(&[pixel[0], pixel[0], pixel[0], pixel[1]]);
  }
  out
}

fn insert_alpha_u16(pixels: &[u16], channels: usize) -> Vec<u16> {
  let mut out = Vec::with_capacity(pixels.len() / channels * 4);
  for pixel in pixels.chunks(channels) {
    if channels == 1 {
      out.extend_from_slice(&[pixel[0], pixel[0], pixel[0], 65535]);
    } else {
      out.extend_from_slice(&[pixel[0], pixel[1], pixel[2], 65535]);
    }
  }
  out
}

fn gray_alpha_u16(pixels: &[u16]) -> Vec<u16> {
  let mut out = Vec::with_capacity(pixels.len() / 2 * 4);
  for pixel in pixels.as_chunks::<2>().0 {
    out.extend_from_slice(&[pixel[0], pixel[0], pixel[0], pixel[1]]);
  }
  out
}

fn decode_jpeg(bytes: &[u8], to_srgb: bool) -> Result<Decoded, DecodeError> {
  let options =
    DecoderOptions::default().jpeg_set_out_colorspace(ColorSpace::RGB);
  let mut decoder = JpegDecoder::new_with_options(Cursor::new(bytes), options);
  decoder.decode_headers().map_err(|_| DecodeError::Corrupt)?;
  let (width, height) = decoder.dimensions().ok_or(DecodeError::Corrupt)?;
  let width = u32::try_from(width).map_err(|_| DecodeError::TooLarge)?;
  let height = u32::try_from(height).map_err(|_| DecodeError::TooLarge)?;
  if !within_limits(width, height) {
    return Err(DecodeError::TooLarge);
  }
  let icc = decoder.icc_profile();
  let pixels = decoder.decode().map_err(|_| DecodeError::Corrupt)?;
  let rgba = to_rgba8(width, height, &pixels)?;
  let mut raster = Raster::from_rgba8(width, height, StoredSpace::Srgb, &rgba)
    .map_err(pixel_error)?;
  finish(
    &mut raster,
    icc.as_deref(),
    None,
    to_srgb,
    exif_orientation(bytes),
  )
}

fn decode_webp(bytes: &[u8], to_srgb: bool) -> Result<Decoded, DecodeError> {
  let mut decoder = image_webp::WebPDecoder::new(Cursor::new(bytes))
    .map_err(|_| DecodeError::Corrupt)?;
  let (width, height) = decoder.dimensions();
  if !within_limits(width, height) {
    return Err(DecodeError::TooLarge);
  }
  let size = decoder.output_buffer_size().ok_or(DecodeError::Corrupt)?;
  let mut pixels = vec![0; size];
  decoder
    .read_image(&mut pixels)
    .map_err(|_| DecodeError::Corrupt)?;
  let icc = decoder.icc_profile().ok().flatten();
  let rgba = to_rgba8(width, height, &pixels)?;
  let mut raster = Raster::from_rgba8(width, height, StoredSpace::Srgb, &rgba)
    .map_err(pixel_error)?;
  finish(
    &mut raster,
    icc.as_deref(),
    None,
    to_srgb,
    exif_orientation(bytes),
  )
}

fn decode_jxl(bytes: &[u8], to_srgb: bool) -> Result<Decoded, DecodeError> {
  let image = read_jxl(bytes)?;
  let (mut raster, _) = jxl_frame(&image, 0)?;
  let (icc, cicp) = jxl_color(&image);
  finish(&mut raster, icc.as_deref(), cicp, to_srgb, 1)
}

fn read_jxl(bytes: &[u8]) -> Result<jxl_oxide::JxlImage, DecodeError> {
  let image = jxl_oxide::JxlImage::builder()
    .read(Cursor::new(bytes))
    .map_err(|_| DecodeError::Corrupt)?;
  if !within_limits(image.width(), image.height()) {
    return Err(DecodeError::TooLarge);
  }
  Ok(image)
}

fn jxl_color(image: &jxl_oxide::JxlImage) -> (Option<Vec<u8>>, Option<Cicp>) {
  let icc = image.original_icc().map(|profile| profile.to_vec());
  let cicp = image.rendered_cicp().map(|points| Cicp {
    primaries: points[0],
    transfer: points[1],
  });
  (icc, cicp)
}

fn jxl_frame(
  image: &jxl_oxide::JxlImage,
  index: usize,
) -> Result<(Raster, u32), DecodeError> {
  let width = image.width();
  let height = image.height();
  let bits = image.image_header().metadata.bit_depth.bits_per_sample();
  let render = image
    .render_frame(index)
    .map_err(|_| DecodeError::Corrupt)?;
  let ticks = render.duration();
  let mut stream = render.stream();
  let channels = stream.channels() as usize;
  if channels == 0 {
    return Err(DecodeError::Corrupt);
  }
  let mut samples = vec![0.0f32; width as usize * height as usize * channels];
  let written = stream.write_to_buffer(&mut samples);
  if written != samples.len() {
    return Err(DecodeError::Corrupt);
  }
  let raster = jxl_raster(width, height, channels, bits, &samples)?;
  Ok((raster, ticks))
}

fn jxl_raster(
  width: u32,
  height: u32,
  channels: usize,
  bits: u32,
  samples: &[f32],
) -> Result<Raster, DecodeError> {
  let count = width as usize * height as usize;
  if bits > 8 {
    let mut rgba = Vec::with_capacity(count * 4);
    for pixel in samples.chunks(channels) {
      push_jxl_pixel(&mut rgba, pixel);
    }
    Raster::from_float16(width, height, StoredSpace::Srgb, &rgba)
      .map_err(pixel_error)
  } else {
    let mut rgba = Vec::with_capacity(count * 4);
    for pixel in samples.chunks(channels) {
      let mut encoded = [0.0f32; 4];
      copy_jxl_pixel(&mut encoded, pixel);
      for channel in encoded {
        rgba.push((f64::from(channel) * 255.0).round().clamp(0.0, 255.0) as u8);
      }
    }
    Raster::from_rgba8(width, height, StoredSpace::Srgb, &rgba)
      .map_err(pixel_error)
  }
}

fn push_jxl_pixel(out: &mut Vec<f64>, pixel: &[f32]) {
  let mut encoded = [0.0f32; 4];
  copy_jxl_pixel(&mut encoded, pixel);
  out.extend(encoded.into_iter().map(f64::from));
}

fn copy_jxl_pixel(out: &mut [f32; 4], pixel: &[f32]) {
  match pixel.len() {
    1 => {
      out[0] = pixel[0];
      out[1] = pixel[0];
      out[2] = pixel[0];
      out[3] = 1.0;
    }
    2 => {
      out[0] = pixel[0];
      out[1] = pixel[0];
      out[2] = pixel[0];
      out[3] = pixel[1];
    }
    3 => {
      out[0] = pixel[0];
      out[1] = pixel[1];
      out[2] = pixel[2];
      out[3] = 1.0;
    }
    _ => {
      out[0] = pixel[0];
      out[1] = pixel[1];
      out[2] = pixel[2];
      out[3] = pixel[3];
    }
  }
}

fn to_rgba8(
  width: u32,
  height: u32,
  pixels: &[u8],
) -> Result<Vec<u8>, DecodeError> {
  let count = width as usize * height as usize;
  if pixels.len() == count * 4 {
    Ok(pixels.to_vec())
  } else if pixels.len() == count * 3 {
    Ok(insert_alpha_u8(pixels, 3))
  } else if pixels.len() == count {
    Ok(insert_alpha_u8(pixels, 1))
  } else if pixels.len() == count * 2 {
    Ok(gray_alpha_u8(pixels))
  } else {
    Err(DecodeError::Corrupt)
  }
}

fn exif_orientation(bytes: &[u8]) -> u8 {
  let Ok(exif) =
    exif::Reader::new().read_from_container(&mut Cursor::new(bytes))
  else {
    return 1;
  };
  exif
    .get_field(exif::Tag::Orientation, exif::In::PRIMARY)
    .and_then(|field| field.value.get_uint(0))
    .and_then(|value| u8::try_from(value).ok())
    .filter(|value| (1..=8).contains(value))
    .unwrap_or(1)
}

fn decode_bmp(bytes: &[u8], to_srgb: bool) -> Result<Decoded, DecodeError> {
  let (width, height, rgba) = read_bmp(bytes)?;
  let mut raster = Raster::from_rgba8(width, height, StoredSpace::Srgb, &rgba)
    .map_err(pixel_error)?;
  finish(&mut raster, None, None, to_srgb, 1)
}

fn read_bmp(bytes: &[u8]) -> Result<(u32, u32, Vec<u8>), DecodeError> {
  if bytes.len() < 18 || &bytes[..2] != b"BM" {
    return Err(DecodeError::Corrupt);
  }
  let header_size = bmp_u32(bytes, 14)?;
  let header_size = usize::try_from(header_size).unwrap_or(usize::MAX);
  if header_size != 40 && header_size != 108 && header_size != 124 {
    return Err(DecodeError::Unsupported);
  }
  let header_end = 14 + header_size;
  if bytes.len() < header_end {
    return Err(DecodeError::Corrupt);
  }
  let width_raw = bmp_i32(bytes, 18)?;
  let height_raw = bmp_i32(bytes, 22)?;
  if width_raw <= 0 || height_raw == 0 {
    return Err(DecodeError::Corrupt);
  }
  let width = u32::try_from(width_raw).map_err(|_| DecodeError::Corrupt)?;
  let height = height_raw.unsigned_abs();
  let top_down = height_raw < 0;
  if !within_limits(width, height) {
    return Err(DecodeError::TooLarge);
  }
  if bmp_u16(bytes, 26)? != 1 {
    return Err(DecodeError::Corrupt);
  }
  let bit_count = bmp_u16(bytes, 28)?;
  let compression = bmp_u32(bytes, 30)?;
  let supported = matches!(
    (compression, bit_count),
    (0, 8) | (0, 24) | (0, 32) | (3, 32)
  );
  if !supported {
    return Err(DecodeError::Unsupported);
  }
  let off_bits = usize::try_from(bmp_u32(bytes, 10)?).unwrap_or(usize::MAX);
  if off_bits < header_end || off_bits > bytes.len() {
    return Err(DecodeError::Corrupt);
  }
  let masks = if compression == 3 {
    Some(bmp_masks(bytes, header_size, off_bits)?)
  } else {
    None
  };
  let palette = if bit_count == 8 {
    Some(bmp_palette(
      bytes,
      header_end,
      off_bits,
      bmp_u32(bytes, 46)?,
    )?)
  } else {
    None
  };
  let row_bytes = match bit_count {
    8 => width as usize,
    24 => width as usize * 3,
    32 => width as usize * 4,
    _ => return Err(DecodeError::Unsupported),
  };
  let stride = ((u64::from(width) * u64::from(bit_count) + 31) / 32) * 4;
  let stride = usize::try_from(stride).map_err(|_| DecodeError::TooLarge)?;
  if stride < row_bytes {
    return Err(DecodeError::Corrupt);
  }
  let rows_bytes = stride
    .checked_mul(height as usize)
    .ok_or(DecodeError::TooLarge)?;
  let pixels_end = off_bits
    .checked_add(rows_bytes)
    .ok_or(DecodeError::Corrupt)?;
  if pixels_end > bytes.len() {
    return Err(DecodeError::Corrupt);
  }
  let mut rgba = vec![0u8; width as usize * height as usize * 4];
  for y in 0..height {
    let file_y = if top_down { y } else { height - 1 - y };
    let row_at = off_bits + file_y as usize * stride;
    let row = bytes
      .get(row_at..row_at + stride)
      .ok_or(DecodeError::Corrupt)?;
    for x in 0..width as usize {
      let pixel = match bit_count {
        8 => {
          let index = usize::from(row[x]);
          *palette
            .as_ref()
            .and_then(|palette| palette.get(index))
            .ok_or(DecodeError::Corrupt)?
        }
        24 => {
          let at = x * 3;
          [row[at + 2], row[at + 1], row[at], 255]
        }
        32 if compression == 0 => {
          [row[x * 4 + 2], row[x * 4 + 1], row[x * 4], 255]
        }
        32 => {
          let at = x * 4;
          let packed = u32::from_le_bytes([
            row[at],
            row[at + 1],
            row[at + 2],
            row[at + 3],
          ]);
          let (red, green, blue, alpha) = masks.ok_or(DecodeError::Corrupt)?;
          let alpha = if alpha == 0 {
            255
          } else {
            bmp_channel(packed, alpha)?
          };
          [
            bmp_channel(packed, red)?,
            bmp_channel(packed, green)?,
            bmp_channel(packed, blue)?,
            alpha,
          ]
        }
        _ => return Err(DecodeError::Unsupported),
      };
      let dest = (y as usize * width as usize + x) * 4;
      rgba[dest..dest + 4].copy_from_slice(&pixel);
    }
  }
  Ok((width, height, rgba))
}

fn bmp_masks(
  bytes: &[u8],
  header_size: usize,
  off_bits: usize,
) -> Result<(u32, u32, u32, u32), DecodeError> {
  let base = 54;
  if bytes.len() < base + 12 {
    return Err(DecodeError::Corrupt);
  }
  let red = bmp_u32(bytes, base)?;
  let green = bmp_u32(bytes, base + 4)?;
  let blue = bmp_u32(bytes, base + 8)?;
  if red == 0 || green == 0 || blue == 0 {
    return Err(DecodeError::Corrupt);
  }
  let alpha = if header_size >= 108 || off_bits >= base + 16 {
    if bytes.len() < base + 16 {
      return Err(DecodeError::Corrupt);
    }
    bmp_u32(bytes, base + 12)?
  } else {
    0
  };
  Ok((red, green, blue, alpha))
}

fn bmp_palette(
  bytes: &[u8],
  header_end: usize,
  off_bits: usize,
  clr_used: u32,
) -> Result<Vec<[u8; 4]>, DecodeError> {
  if off_bits < header_end {
    return Err(DecodeError::Corrupt);
  }
  let available = (off_bits - header_end) / 4;
  let wanted = if clr_used == 0 {
    256
  } else {
    usize::try_from(clr_used).unwrap_or(usize::MAX)
  };
  if wanted == 0 || wanted > 256 || available < wanted {
    return Err(DecodeError::Corrupt);
  }
  let mut palette = Vec::with_capacity(wanted);
  for index in 0..wanted {
    let at = header_end + index * 4;
    let entry = bytes.get(at..at + 4).ok_or(DecodeError::Corrupt)?;
    palette.push([entry[2], entry[1], entry[0], 255]);
  }
  Ok(palette)
}

fn bmp_channel(pixel: u32, mask: u32) -> Result<u8, DecodeError> {
  if mask == 0 {
    return Err(DecodeError::Corrupt);
  }
  let shift = mask.trailing_zeros();
  let bits = mask.count_ones();
  let max = if bits == 32 {
    u32::MAX
  } else {
    (1u32 << bits) - 1
  };
  if (mask >> shift) != max {
    return Err(DecodeError::Corrupt);
  }
  let value = (pixel & mask) >> shift;
  if bits >= 8 {
    Ok((value >> (bits - 8)) as u8)
  } else {
    let max = u64::from(max);
    Ok(((u64::from(value) * 255 + max / 2) / max) as u8)
  }
}

fn bmp_u16(bytes: &[u8], offset: usize) -> Result<u16, DecodeError> {
  let end = offset.checked_add(2).ok_or(DecodeError::Corrupt)?;
  let slice = bytes.get(offset..end).ok_or(DecodeError::Corrupt)?;
  Ok(u16::from_le_bytes([slice[0], slice[1]]))
}

fn bmp_u32(bytes: &[u8], offset: usize) -> Result<u32, DecodeError> {
  let end = offset.checked_add(4).ok_or(DecodeError::Corrupt)?;
  let slice = bytes.get(offset..end).ok_or(DecodeError::Corrupt)?;
  Ok(u32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]))
}

fn bmp_i32(bytes: &[u8], offset: usize) -> Result<i32, DecodeError> {
  Ok(bmp_u32(bytes, offset)? as i32)
}

fn pixel_error(error: crate::raster::PixelError) -> DecodeError {
  match error {
    crate::raster::PixelError::TooLarge => DecodeError::TooLarge,
    crate::raster::PixelError::Empty => DecodeError::Corrupt,
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::raster::ColorSpace as Stored;

  fn fixture(name: &str) -> Vec<u8> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
      .join("../../tests/fixtures/images")
      .join(name);
    std::fs::read(path).expect(name)
  }

  fn assert_corners(raster: &Raster) {
    assert_eq!((raster.width, raster.height), (2, 2));
    assert_eq!(raster.format, SampleFormat::Unorm8);
    assert_eq!(raster.color_space, Stored::Srgb);
    assert_eq!(raster.pixel_u8(0, 0), [255, 0, 0, 255]);
    assert_eq!(raster.pixel_u8(1, 0), [0, 255, 0, 255]);
    assert_eq!(raster.pixel_u8(0, 1), [0, 0, 255, 255]);
    assert_eq!(raster.pixel_u8(1, 1), [0, 0, 0, 128]);
  }

  #[test]
  fn png_webp_and_jxl_match_the_corner_pixels() {
    assert_corners(&decode(&fixture("corners.png"), true).unwrap().raster);
    assert_corners(&decode(&fixture("corners.webp"), true).unwrap().raster);
    assert_corners(&decode(&fixture("corners.jxl"), true).unwrap().raster);
  }

  #[test]
  fn jpeg_solid_red_is_near_red() {
    let raster = decode(&fixture("red.jpg"), true).unwrap().raster;
    assert_eq!((raster.width, raster.height), (8, 8));
    assert_eq!(raster.format, SampleFormat::Unorm8);
    for y in 0..8 {
      for x in 0..8 {
        let pixel = raster.pixel_u8(x, y);
        assert!((pixel[0] as i16 - 255).abs() <= 2, "{pixel:?}");
        assert!(pixel[1] <= 2, "{pixel:?}");
        assert!(pixel[2] <= 2, "{pixel:?}");
        assert_eq!(pixel[3], 255);
      }
    }
  }

  #[test]
  fn sixteen_bit_png_is_float16() {
    let mut bytes = Vec::new();
    {
      let mut encoder = png::Encoder::new(&mut bytes, 1, 1);
      encoder.set_color(png::ColorType::Rgba);
      encoder.set_depth(png::BitDepth::Sixteen);
      let mut writer = encoder.write_header().unwrap();
      let mut data = Vec::new();
      for sample in [0x8000u16, 0, 0, 65535] {
        data.extend(sample.to_be_bytes());
      }
      writer.write_image_data(&data).unwrap();
    }
    let raster = decode(&bytes, true).unwrap().raster;
    assert_eq!(raster.format, SampleFormat::Float16);
    assert_eq!(raster.color_space, Stored::Srgb);
    let pixel = raster.pixel_f64(0, 0);
    assert!((pixel[0] - 0.5).abs() < 0.001, "{pixel:?}");
    assert_eq!(pixel[1], 0.0);
    assert_eq!(pixel[2], 0.0);
    assert!((pixel[3] - 1.0).abs() < 0.001, "{pixel:?}");
  }

  #[test]
  fn display_p3_cicp_stays_when_conversion_is_none() {
    let mut bytes = Vec::new();
    {
      let mut encoder = png::Encoder::new(&mut bytes, 1, 1);
      encoder.set_color(png::ColorType::Rgba);
      encoder.set_depth(png::BitDepth::Eight);
      let mut writer = encoder.write_header().unwrap();
      writer
        .write_chunk(png::chunk::cICP, &[12, 13, 0, 1])
        .unwrap();
      writer.write_image_data(&[255, 0, 0, 255]).unwrap();
    }
    let kept = decode(&bytes, false).unwrap().raster;
    assert_eq!(kept.color_space, Stored::DisplayP3);
    assert_eq!(kept.pixel_u8(0, 0), [255, 0, 0, 255]);
    let converted = decode(&bytes, true).unwrap().raster;
    assert_eq!(converted.color_space, Stored::Srgb);
  }

  #[test]
  fn corrupt_bytes_reject() {
    assert_eq!(
      decode(b"not an image", true).err(),
      Some(DecodeError::Unsupported)
    );
    assert_eq!(
      decode(b"\x89PNG\r\n\x1a\ntruncated", true).err(),
      Some(DecodeError::Corrupt)
    );
  }

  fn encode_gif(
    width: u16,
    height: u16,
    repeat: gif::Repeat,
    frames: &[GifPart<'_>],
  ) -> Vec<u8> {
    let mut bytes = Vec::new();
    {
      let mut encoder = gif::Encoder::new(
        &mut bytes,
        width,
        height,
        &[255, 0, 0, 0, 0, 255, 0, 0, 0],
      )
      .unwrap();
      if !matches!(repeat, gif::Repeat::Finite(0)) {
        encoder.set_repeat(repeat).unwrap();
      }
      for frame in frames {
        let mut part = gif::Frame::default();
        part.left = frame.left;
        part.top = frame.top;
        part.width = frame.width;
        part.height = frame.height;
        part.delay = frame.delay;
        part.dispose = frame.dispose;
        part.transparent = frame.transparent;
        part.buffer = std::borrow::Cow::Borrowed(frame.indices);
        encoder.write_frame(&part).unwrap();
      }
    }
    bytes
  }

  struct GifPart<'a> {
    indices: &'a [u8],
    delay: u16,
    left: u16,
    top: u16,
    width: u16,
    height: u16,
    dispose: gif::DisposalMethod,
    transparent: Option<u8>,
  }

  fn full_gif<'a>(
    indices: &'a [u8],
    delay: u16,
    dispose: gif::DisposalMethod,
  ) -> GifPart<'a> {
    GifPart {
      indices,
      delay,
      left: 0,
      top: 0,
      width: 2,
      height: 2,
      dispose,
      transparent: None,
    }
  }

  #[test]
  fn animated_gif_composites_delay_and_loops() {
    let red = [0u8; 4];
    let blue = [1u8; 4];
    let bytes = encode_gif(
      2,
      2,
      gif::Repeat::Infinite,
      &[
        full_gif(&red, 10, gif::DisposalMethod::Keep),
        full_gif(&blue, 10, gif::DisposalMethod::Keep),
      ],
    );
    let still = decode(&bytes, true).unwrap();
    assert_eq!(still.raster.pixel_u8(0, 0), [255, 0, 0, 255]);
    assert_eq!(still.raster.pixel_u8(1, 1), [255, 0, 0, 255]);
    let frames = decode_frames(&bytes, true).unwrap();
    assert!(frames.animated);
    assert!(frames.repetition_count.is_infinite());
    assert_eq!(frames.frames.len(), 2);
    assert_eq!(frames.frames[0].duration_us, Some(100_000.0));
    assert_eq!(frames.frames[1].duration_us, Some(100_000.0));
    assert_eq!(frames.frames[0].raster.pixel_u8(0, 0), [255, 0, 0, 255]);
    assert_eq!(frames.frames[0].raster.pixel_u8(1, 1), [255, 0, 0, 255]);
    assert_eq!(frames.frames[1].raster.pixel_u8(0, 0), [0, 0, 255, 255]);
    assert_eq!(frames.frames[1].raster.pixel_u8(1, 1), [0, 0, 255, 255]);

    let once = encode_gif(
      2,
      2,
      gif::Repeat::Finite(1),
      &[full_gif(&red, 10, gif::DisposalMethod::Keep)],
    );
    let once = decode_frames(&once, true).unwrap();
    assert!(!once.animated);
    assert_eq!(once.repetition_count, 0.0);
    assert!(once.frames[0].duration_us.is_none());

    let finite = encode_gif(
      2,
      2,
      gif::Repeat::Finite(2),
      &[
        full_gif(&red, 10, gif::DisposalMethod::Keep),
        full_gif(&blue, 0, gif::DisposalMethod::Keep),
      ],
    );
    let finite = decode_frames(&finite, true).unwrap();
    assert_eq!(finite.repetition_count, 1.0);
    assert_eq!(finite.frames[1].duration_us, Some(100_000.0));
    let absent = encode_gif(
      2,
      2,
      gif::Repeat::Finite(0),
      &[
        full_gif(&red, 10, gif::DisposalMethod::Keep),
        full_gif(&blue, 10, gif::DisposalMethod::Keep),
      ],
    );
    assert!(
      decode_frames(&absent, true)
        .unwrap()
        .repetition_count
        .is_infinite()
    );
  }

  #[test]
  fn gif_disposal_background_previous_and_transparent() {
    let red = [0u8; 4];
    let cleared = encode_gif(
      2,
      2,
      gif::Repeat::Infinite,
      &[
        full_gif(&red, 10, gif::DisposalMethod::Background),
        GifPart {
          indices: &[1],
          delay: 10,
          left: 0,
          top: 0,
          width: 1,
          height: 1,
          dispose: gif::DisposalMethod::Keep,
          transparent: None,
        },
      ],
    );
    let frames = decode_frames(&cleared, true).unwrap();
    assert_eq!(frames.frames[1].raster.pixel_u8(0, 0), [0, 0, 255, 255]);
    assert_eq!(frames.frames[1].raster.pixel_u8(1, 0), [0, 0, 0, 0]);
    assert_eq!(frames.frames[1].raster.pixel_u8(1, 1), [0, 0, 0, 0]);

    let restored = encode_gif(
      2,
      2,
      gif::Repeat::Infinite,
      &[
        full_gif(&red, 10, gif::DisposalMethod::Keep),
        full_gif(&[1, 1, 1, 1], 10, gif::DisposalMethod::Previous),
        GifPart {
          indices: &[1],
          delay: 10,
          left: 0,
          top: 0,
          width: 1,
          height: 1,
          dispose: gif::DisposalMethod::Keep,
          transparent: None,
        },
      ],
    );
    let frames = decode_frames(&restored, true).unwrap();
    assert_eq!(frames.frames[2].raster.pixel_u8(0, 0), [0, 0, 255, 255]);
    assert_eq!(frames.frames[2].raster.pixel_u8(1, 0), [255, 0, 0, 255]);

    let hole = encode_gif(
      2,
      2,
      gif::Repeat::Infinite,
      &[
        full_gif(&red, 10, gif::DisposalMethod::Keep),
        GifPart {
          indices: &[2, 1, 1, 1],
          delay: 1,
          left: 0,
          top: 0,
          width: 2,
          height: 2,
          dispose: gif::DisposalMethod::Keep,
          transparent: Some(2),
        },
      ],
    );
    let frames = decode_frames(&hole, true).unwrap();
    assert_eq!(frames.frames[1].raster.pixel_u8(0, 0), [255, 0, 0, 255]);
    assert_eq!(frames.frames[1].raster.pixel_u8(1, 0), [0, 0, 255, 255]);
    assert_eq!(frames.frames[1].duration_us, Some(100_000.0));
  }

  #[test]
  fn gif_screen_over_the_limit_is_too_large() {
    let mut bytes = Vec::new();
    {
      let mut encoder =
        gif::Encoder::new(&mut bytes, 16_385, 1, &[255, 0, 0]).unwrap();
      let mut frame = gif::Frame::default();
      frame.width = 16_385;
      frame.height = 1;
      frame.buffer = std::borrow::Cow::Owned(vec![0; 16_385]);
      encoder.write_frame(&frame).unwrap();
    }
    assert_eq!(decode(&bytes, true).err(), Some(DecodeError::TooLarge));
    assert_eq!(
      decode_frames(&bytes, true).err(),
      Some(DecodeError::TooLarge)
    );
  }

  fn bmp_file(
    width: i32,
    height: i32,
    bit_count: u16,
    compression: u32,
    header_size: u32,
    masks: &[u32],
    palette: &[[u8; 4]],
    rows_top_first: &[Vec<u8>],
  ) -> Vec<u8> {
    let mut info = Vec::new();
    info.extend_from_slice(&header_size.to_le_bytes());
    info.extend_from_slice(&width.to_le_bytes());
    info.extend_from_slice(&height.to_le_bytes());
    info.extend_from_slice(&1u16.to_le_bytes());
    info.extend_from_slice(&bit_count.to_le_bytes());
    info.extend_from_slice(&compression.to_le_bytes());
    info.extend_from_slice(&0u32.to_le_bytes());
    info.extend_from_slice(&0u32.to_le_bytes());
    info.extend_from_slice(&0u32.to_le_bytes());
    let colors = u32::try_from(palette.len()).unwrap_or(0);
    info.extend_from_slice(&colors.to_le_bytes());
    info.extend_from_slice(&0u32.to_le_bytes());
    if header_size >= 108 {
      for mask in masks {
        info.extend_from_slice(&mask.to_le_bytes());
      }
      info.resize(header_size as usize, 0);
    }
    let mut file = vec![b'B', b'M'];
    let pixel_bytes: usize = rows_top_first.iter().map(Vec::len).sum();
    let mask_bytes = if header_size == 40 {
      masks.len() * 4
    } else {
      0
    };
    let off_bits = 14 + info.len() + mask_bytes + palette.len() * 4;
    let file_size = u32::try_from(off_bits + pixel_bytes).unwrap();
    file.extend_from_slice(&file_size.to_le_bytes());
    file.extend_from_slice(&0u32.to_le_bytes());
    file.extend_from_slice(&(off_bits as u32).to_le_bytes());
    file.extend_from_slice(&info);
    if header_size == 40 {
      for mask in masks {
        file.extend_from_slice(&mask.to_le_bytes());
      }
    }
    for entry in palette {
      file.extend_from_slice(entry);
    }
    let top_down = height < 0;
    let order: Vec<usize> = if top_down {
      (0..rows_top_first.len()).collect()
    } else {
      (0..rows_top_first.len()).rev().collect()
    };
    for index in order {
      file.extend_from_slice(&rows_top_first[index]);
    }
    file
  }

  #[test]
  fn bmp_24_bottom_up_red_origin_and_rejects_truncation() {
    let bytes = bmp_file(
      2,
      2,
      24,
      0,
      40,
      &[],
      &[],
      &[
        vec![0, 0, 255, 255, 255, 255, 0, 0],
        vec![255, 0, 0, 0, 255, 0, 0, 0],
      ],
    );
    let raster = decode(&bytes, true).unwrap().raster;
    assert_eq!((raster.width, raster.height), (2, 2));
    assert_eq!(raster.pixel_u8(0, 0), [255, 0, 0, 255]);
    assert_eq!(raster.pixel_u8(1, 0), [255, 255, 255, 255]);
    assert_eq!(raster.pixel_u8(0, 1), [0, 0, 255, 255]);
    assert_eq!(raster.pixel_u8(1, 1), [0, 255, 0, 255]);
    let frames = decode_frames(&bytes, true).unwrap();
    assert!(!frames.animated);
    assert_eq!(frames.repetition_count, 0.0);
    assert!(frames.frames[0].duration_us.is_none());
    assert_eq!(frames.frames[0].raster.pixel_u8(0, 0), [255, 0, 0, 255]);

    let top_down = bmp_file(
      2,
      -2,
      24,
      0,
      40,
      &[],
      &[],
      &[
        vec![0, 0, 255, 255, 255, 255, 0, 0],
        vec![255, 0, 0, 0, 255, 0, 0, 0],
      ],
    );
    assert_eq!(
      decode(&top_down, true).unwrap().raster.pixel_u8(0, 0),
      [255, 0, 0, 255]
    );

    assert!(decode(b"GIF89a", true).is_err());
    assert!(decode_frames(b"GIF87a", true).is_err());
    assert!(decode(b"BM", true).is_err());
    assert!(decode_frames(&bytes[..20], true).is_err());
    let mut negative = bytes.clone();
    negative[18..22].copy_from_slice(&(-2i32).to_le_bytes());
    assert!(decode(&negative, true).is_err());
  }

  #[test]
  fn bmp_palette_bitfields_and_unsupported_compression() {
    let indexed = bmp_file(
      2,
      2,
      8,
      0,
      40,
      &[],
      &[
        [0, 0, 255, 0],
        [255, 255, 255, 0],
        [255, 0, 0, 0],
        [0, 255, 0, 0],
      ],
      &[vec![0, 1, 0, 0], vec![2, 3, 0, 0]],
    );
    let raster = decode(&indexed, true).unwrap().raster;
    assert_eq!(raster.pixel_u8(0, 0), [255, 0, 0, 255]);
    assert_eq!(raster.pixel_u8(1, 0), [255, 255, 255, 255]);
    assert_eq!(raster.pixel_u8(0, 1), [0, 0, 255, 255]);
    assert_eq!(raster.pixel_u8(1, 1), [0, 255, 0, 255]);

    let masks = [0x00FF_0000, 0x0000_FF00, 0x0000_00FF, 0xFF00_0000];
    let fields =
      bmp_file(1, 1, 32, 3, 40, &masks, &[], &[vec![0, 0, 255, 128]]);
    assert_eq!(
      decode(&fields, true).unwrap().raster.pixel_u8(0, 0),
      [255, 0, 0, 128]
    );
    let forced = bmp_file(
      1,
      1,
      32,
      3,
      40,
      &[0x00FF_0000, 0x0000_FF00, 0x0000_00FF],
      &[],
      &[vec![0, 0, 255, 0]],
    );
    assert_eq!(
      decode(&forced, true).unwrap().raster.pixel_u8(0, 0),
      [255, 0, 0, 255]
    );
    let v4 = bmp_file(1, 1, 32, 3, 108, &masks, &[], &[vec![0, 0, 255, 64]]);
    assert_eq!(
      decode(&v4, true).unwrap().raster.pixel_u8(0, 0),
      [255, 0, 0, 64]
    );

    let rle = bmp_file(1, 1, 8, 1, 40, &[], &[], &[vec![0, 0, 0, 0]]);
    assert_eq!(decode(&rle, true).err(), Some(DecodeError::Unsupported));
    let mut core = vec![0u8; 18];
    core[0] = b'B';
    core[1] = b'M';
    core[14..18].copy_from_slice(&12u32.to_le_bytes());
    assert_eq!(decode(&core, true).err(), Some(DecodeError::Unsupported));
  }
}
