//! `imageSmoothingQuality` kernels for `drawImage`.
//!
//! `"high"` resamples a scaled image with a separable Lanczos-3 kernel
//! before the bilinear paint. An axis-aligned draw within 2% of 1:1 stays
//! bilinear so the three qualities match.

use kurbo::Affine;

use crate::paint::record::ImageBuf;
use crate::paint::record::SampleQuality;

const UNIT_SLOP: f64 = 0.02;
const MAX_EDGE: f64 = 8192.0;
const MAX_PIXELS: f64 = 8_000_000.0;

/// Choose the bitmap and the sampler for one `drawImage`.
pub fn prepare_draw(
  image: ImageBuf,
  transform: Affine,
  quality: SampleQuality,
) -> (ImageBuf, Affine, SampleQuality) {
  if quality == SampleQuality::Nearest {
    return (image, transform, quality);
  }
  let (sx, sy) = axis_scales(transform);
  if axis_aligned(transform) && near_one(sx) && near_one(sy) {
    return (image, transform, SampleQuality::Low);
  }
  if quality != SampleQuality::High {
    return (image, transform, quality);
  }
  if image.float16 || sx < 1.0e-6 || sy < 1.0e-6 {
    return (image, transform, SampleQuality::Medium);
  }
  let dest_w = (f64::from(image.width) * sx).round();
  let dest_h = (f64::from(image.height) * sy).round();
  if !(1.0..=MAX_EDGE).contains(&dest_w)
    || !(1.0..=MAX_EDGE).contains(&dest_h)
    || dest_w * dest_h > MAX_PIXELS
  {
    return (image, transform, SampleQuality::Medium);
  }
  let dest_w = dest_w as u32;
  let dest_h = dest_h as u32;
  if dest_w == image.width && dest_h == image.height {
    return (image, transform, SampleQuality::Medium);
  }
  let Some(resampled) = lanczos3(&image, dest_w, dest_h) else {
    return (image, transform, SampleQuality::Medium);
  };
  let adjusted = transform
    * Affine::scale_non_uniform(
      f64::from(image.width) / f64::from(dest_w),
      f64::from(image.height) / f64::from(dest_h),
    );
  (resampled, adjusted, SampleQuality::Low)
}

fn axis_scales(transform: Affine) -> (f64, f64) {
  let [a, b, c, d, _, _] = transform.as_coeffs();
  ((a * a + b * b).sqrt(), (c * c + d * d).sqrt())
}

fn axis_aligned(transform: Affine) -> bool {
  let [_, b, c, _, _, _] = transform.as_coeffs();
  b.abs() <= 1.0e-3 && c.abs() <= 1.0e-3
}

fn near_one(scale: f64) -> bool {
  (scale - 1.0).abs() <= UNIT_SLOP
}

/// Separable Lanczos-3 on premultiplied RGBA8. Channels are filtered
/// independently and clamped back to a byte.
pub fn lanczos3(image: &ImageBuf, dest_w: u32, dest_h: u32) -> Option<ImageBuf> {
  if image.float16 || dest_w == 0 || dest_h == 0 || image.width == 0 || image.height == 0 {
    return None;
  }
  let src_w = image.width as usize;
  let src_h = image.height as usize;
  let dest_w = dest_w as usize;
  let dest_h = dest_h as usize;
  let bytes = &image.bytes;
  if bytes.len() < src_w * src_h * 4 {
    return None;
  }
  let mut horizontal = vec![0.0f32; src_h * dest_w * 4];
  for y in 0..src_h {
    for x in 0..dest_w {
      let mut acc = [0.0f64; 4];
      let mut sum = 0.0f64;
      for (index, weight) in kernel(x, dest_w, src_w) {
        let sx = index.clamp(0, src_w as i32 - 1) as usize;
        let offset = (y * src_w + sx) * 4;
        for channel in 0..4 {
          acc[channel] += f64::from(bytes[offset + channel]) * weight;
        }
        sum += weight;
      }
      write_sample(&mut horizontal, (y * dest_w + x) * 4, acc, sum);
    }
  }
  let mut output = vec![0u8; dest_w * dest_h * 4];
  for y in 0..dest_h {
    for x in 0..dest_w {
      let mut acc = [0.0f64; 4];
      let mut sum = 0.0f64;
      for (index, weight) in kernel(y, dest_h, src_h) {
        let sy = index.clamp(0, src_h as i32 - 1) as usize;
        let offset = (sy * dest_w + x) * 4;
        for channel in 0..4 {
          acc[channel] += f64::from(horizontal[offset + channel]) * weight;
        }
        sum += weight;
      }
      let offset = (y * dest_w + x) * 4;
      if sum.abs() < 1.0e-12 {
        continue;
      }
      for channel in 0..4 {
        output[offset + channel] =
          (acc[channel] / sum).round().clamp(0.0, 255.0) as u8;
      }
    }
  }
  Some(ImageBuf {
    width: dest_w as u32,
    height: dest_h as u32,
    display_p3: false,
    float16: false,
    premultiplied: image.premultiplied,
    bytes: output,
  })
}

fn write_sample(output: &mut [f32], offset: usize, acc: [f64; 4], sum: f64) {
  if sum.abs() < 1.0e-12 {
    return;
  }
  for channel in 0..4 {
    output[offset + channel] = (acc[channel] / sum) as f32;
  }
}

fn kernel(dest_index: usize, dest_len: usize, src_len: usize) -> Vec<(i32, f64)> {
  let scale = src_len as f64 / dest_len as f64;
  let stretch = scale.max(1.0);
  let radius = 3.0 * stretch;
  let center = (dest_index as f64 + 0.5) * scale - 0.5;
  let start = (center - radius).ceil() as i32;
  let end = (center + radius).floor() as i32;
  (start..=end)
    .map(|index| {
      let distance = (f64::from(index) - center) / stretch;
      (index, lanczos3_weight(distance))
    })
    .collect()
}

fn lanczos3_weight(x: f64) -> f64 {
  let x = x.abs();
  if x < 1.0e-12 {
    return 1.0;
  }
  if x >= 3.0 {
    return 0.0;
  }
  let pix = x * std::f64::consts::PI;
  3.0 * pix.sin() * (pix / 3.0).sin() / (pix * pix)
}

#[cfg(test)]
mod tests {
  use super::lanczos3;
  use crate::paint::record::ImageBuf;

  fn solid(width: u32, height: u32, pixel: [u8; 4]) -> ImageBuf {
    ImageBuf {
      width,
      height,
      display_p3: false,
      float16: false,
      premultiplied: true,
      bytes: pixel.repeat((width * height) as usize),
    }
  }

  #[test]
  fn solid_color_survives_a_scale() {
    let image = solid(2, 2, [10, 20, 30, 255]);
    let scaled = lanczos3(&image, 4, 3).unwrap();
    assert_eq!(scaled.width, 4);
    assert_eq!(scaled.height, 3);
    assert!(scaled.bytes.chunks(4).all(|pixel| pixel == [10, 20, 30, 255]));
  }
}
