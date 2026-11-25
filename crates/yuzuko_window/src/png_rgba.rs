//! PNG bytes expanded to straight RGBA8.

use std::io::Cursor;

/// Decodes a PNG into RGBA8 pixels.
///
/// 16-bit samples are reduced to 8-bit and a palette is expanded. A file that
/// is not a PNG, or whose pixels do not cover `width * height`, is `None`.
pub(crate) fn decode(bytes: &[u8]) -> Option<(Vec<u8>, u32, u32)> {
  let mut decoder = png::Decoder::new(Cursor::new(bytes));
  decoder.set_transformations(
    png::Transformations::EXPAND | png::Transformations::STRIP_16,
  );
  let mut reader = decoder.read_info().ok()?;
  let size = reader.output_buffer_size()?;
  let mut buffer = vec![0; size];
  let frame = reader.next_frame(&mut buffer).ok()?;
  let width = frame.width;
  let height = frame.height;
  let pixels = &buffer[..frame.buffer_size()];
  let rgba = match frame.color_type {
    png::ColorType::Rgba => pixels.to_vec(),
    png::ColorType::Rgb => {
      let mut rgba = Vec::with_capacity(pixels.len() / 3 * 4);
      for chunk in pixels.as_chunks::<3>().0 {
        rgba.extend_from_slice(chunk);
        rgba.push(255);
      }
      rgba
    }
    png::ColorType::Grayscale => pixels
      .iter()
      .flat_map(|sample| [*sample, *sample, *sample, 255])
      .collect(),
    png::ColorType::GrayscaleAlpha => pixels
      .as_chunks::<2>()
      .0
      .iter()
      .flat_map(|chunk| [chunk[0], chunk[0], chunk[0], chunk[1]])
      .collect(),
    _ => return None,
  };
  let expected = (width as usize)
    .checked_mul(height as usize)
    .and_then(|count| count.checked_mul(4))?;
  if rgba.len() != expected {
    return None;
  }
  Some((rgba, width, height))
}

#[cfg(test)]
mod tests {
  use super::decode;

  fn png(
    width: u32,
    height: u32,
    color: png::ColorType,
    depth: png::BitDepth,
    samples: &[u8],
    palette: Option<&[u8]>,
    trns: Option<&[u8]>,
  ) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut encoder = png::Encoder::new(&mut bytes, width, height);
    encoder.set_color(color);
    encoder.set_depth(depth);
    if let Some(palette) = palette {
      encoder.set_palette(palette);
    }
    if let Some(trns) = trns {
      encoder.set_trns(trns);
    }
    let mut writer = encoder.write_header().unwrap();
    writer.write_image_data(samples).unwrap();
    writer.finish().unwrap();
    bytes
  }

  #[test]
  fn straight_rgba8() {
    let rgba = png(
      1,
      1,
      png::ColorType::Rgba,
      png::BitDepth::Eight,
      &[1, 2, 3, 4],
      None,
      None,
    );
    assert_eq!(decode(&rgba), Some((vec![1, 2, 3, 4], 1, 1)));

    let rgb = png(
      1,
      1,
      png::ColorType::Rgb,
      png::BitDepth::Eight,
      &[9, 8, 7],
      None,
      None,
    );
    assert_eq!(decode(&rgb), Some((vec![9, 8, 7, 255], 1, 1)));

    let gray = png(
      1,
      1,
      png::ColorType::Grayscale,
      png::BitDepth::Eight,
      &[40],
      None,
      None,
    );
    assert_eq!(decode(&gray), Some((vec![40, 40, 40, 255], 1, 1)));

    let gray_alpha = png(
      1,
      1,
      png::ColorType::GrayscaleAlpha,
      png::BitDepth::Eight,
      &[40, 10],
      None,
      None,
    );
    assert_eq!(decode(&gray_alpha), Some((vec![40, 40, 40, 10], 1, 1)));

    // `STRIP_16` keeps the high byte of each sample.
    let deep = png(
      1,
      1,
      png::ColorType::Rgb,
      png::BitDepth::Sixteen,
      &[0xAB, 0x00, 0x00, 0xCD, 0x12, 0x34],
      None,
      None,
    );
    assert_eq!(decode(&deep), Some((vec![0xAB, 0x00, 0x12, 255], 1, 1)));
  }

  #[test]
  fn expands_palette_and_transparency() {
    let indexed = png(
      2,
      1,
      png::ColorType::Indexed,
      png::BitDepth::Eight,
      &[0, 1],
      Some(&[255, 0, 0, 0, 0, 255]),
      None,
    );
    assert_eq!(
      decode(&indexed),
      Some((vec![255, 0, 0, 255, 0, 0, 255, 255], 2, 1))
    );

    let keyed = png(
      2,
      1,
      png::ColorType::Indexed,
      png::BitDepth::Eight,
      &[0, 1],
      Some(&[255, 0, 0, 0, 255, 0]),
      Some(&[255, 0]),
    );
    assert_eq!(
      decode(&keyed),
      Some((vec![255, 0, 0, 255, 0, 255, 0, 0], 2, 1))
    );

    // An 8-bit RGB `tRNS` chunk stores each key sample in two bytes.
    let rgb_key = png(
      2,
      1,
      png::ColorType::Rgb,
      png::BitDepth::Eight,
      &[255, 0, 0, 0, 255, 0],
      None,
      Some(&[0, 255, 0, 0, 0, 0]),
    );
    assert_eq!(
      decode(&rgb_key),
      Some((vec![255, 0, 0, 0, 0, 255, 0, 255], 2, 1))
    );
  }

  #[test]
  fn rejects_bytes_that_are_not_a_png() {
    assert!(decode(b"not a png").is_none());
  }
}
