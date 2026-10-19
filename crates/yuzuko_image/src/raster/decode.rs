//! Signature sniffing and the four phase-3 decoders.

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
}

pub fn decode(bytes: &[u8], to_srgb: bool) -> Result<Decoded, DecodeError> {
  let caught =
    std::panic::catch_unwind(AssertUnwindSafe(|| decode_inner(bytes, to_srgb)));
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
    None => Err(DecodeError::Unsupported),
  }
}

enum Kind {
  Png,
  Jpeg,
  Webp,
  Jxl,
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
  } else {
    None
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
  let mut decoder = JpegDecoder::new_with_options(bytes, options);
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
  let image = jxl_oxide::JxlImage::builder()
    .read(Cursor::new(bytes))
    .map_err(|_| DecodeError::Corrupt)?;
  let width = image.width();
  let height = image.height();
  if !within_limits(width, height) {
    return Err(DecodeError::TooLarge);
  }
  let bits = image.image_header().metadata.bit_depth.bits_per_sample();
  let icc = image.original_icc().map(|profile| profile.to_vec());
  let cicp = image.rendered_cicp().map(|points| Cicp {
    primaries: points[0],
    transfer: points[1],
  });
  let render = image.render_frame(0).map_err(|_| DecodeError::Corrupt)?;
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
  let mut raster = jxl_raster(width, height, channels, bits, &samples)?;
  finish(&mut raster, icc.as_deref(), cicp, to_srgb, 1)
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
}
