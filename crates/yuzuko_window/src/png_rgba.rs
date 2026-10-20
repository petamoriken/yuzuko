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
