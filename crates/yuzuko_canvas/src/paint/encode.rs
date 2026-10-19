//! `convertToBlob` encoders. WebP is lossless VP8L.

use std::io::Write;

use yuzuko_image::ColorSpace;
use yuzuko_image::Raster;
use yuzuko_image::SampleFormat;
use yuzuko_image::convert;

pub fn encode(
  raster: &Raster,
  mime: &str,
  quality: f32,
) -> Result<Vec<u8>, String> {
  let mut raster = raster.clone();
  if raster.premultiplied {
    yuzuko_image::unpremultiply(&mut raster);
  }
  convert(&mut raster, ColorSpace::Srgb);
  if raster.format != SampleFormat::Unorm8 {
    raster = quantize(raster);
  }
  match mime {
    "image/jpeg" => encode_jpeg(&raster, quality),
    "image/webp" => encode_webp(&raster),
    _ => encode_png(&raster),
  }
}

fn quantize(raster: Raster) -> Raster {
  let mut output = Raster::new(
    raster.width,
    raster.height,
    ColorSpace::Srgb,
    SampleFormat::Unorm8,
  )
  .expect("quantize");
  for y in 0..raster.height {
    for x in 0..raster.width {
      let sample = raster.sample(i64::from(x), i64::from(y));
      output.write_sample(
        i64::from(x),
        i64::from(y),
        sample.map(|channel| channel.clamp(0.0, 1.0) * 255.0),
      );
    }
  }
  output
}

fn encode_png(raster: &Raster) -> Result<Vec<u8>, String> {
  let mut bytes = Vec::new();
  let mut encoder = png::Encoder::new(&mut bytes, raster.width, raster.height);
  encoder.set_color(png::ColorType::Rgba);
  encoder.set_depth(png::BitDepth::Eight);
  let mut writer = encoder.write_header().map_err(|error| error.to_string())?;
  writer
    .write_image_data(&raster.bytes)
    .map_err(|error| error.to_string())?;
  drop(writer);
  Ok(bytes)
}

fn encode_jpeg(raster: &Raster, quality: f32) -> Result<Vec<u8>, String> {
  let mut rgb =
    Vec::with_capacity(raster.width as usize * raster.height as usize * 3);
  for pixel in raster.bytes.as_chunks::<4>().0 {
    let alpha = f32::from(pixel[3]) / 255.0;
    for channel in &pixel[..3] {
      let value = f32::from(*channel) * alpha + 255.0 * (1.0 - alpha);
      rgb.push(value.round().clamp(0.0, 255.0) as u8);
    }
  }
  let quality = (quality.clamp(0.0, 1.0) * 100.0).round() as u8;
  let quality = quality.max(1);
  let mut bytes = Vec::new();
  jpeg_encoder::Encoder::new(&mut bytes, quality)
    .encode(
      &rgb,
      u16::try_from(raster.width)
        .map_err(|_| "JPEG is too wide".to_string())?,
      u16::try_from(raster.height)
        .map_err(|_| "JPEG is too tall".to_string())?,
      jpeg_encoder::ColorType::Rgb,
    )
    .map_err(|error| error.to_string())?;
  Ok(bytes)
}

fn encode_webp(raster: &Raster) -> Result<Vec<u8>, String> {
  let mut bytes = Vec::new();
  image_webp::WebPEncoder::new(&mut bytes)
    .encode(
      &raster.bytes,
      raster.width,
      raster.height,
      image_webp::ColorType::Rgba8,
    )
    .map_err(|error| error.to_string())?;
  let _ = Write::flush(&mut bytes);
  Ok(bytes)
}
