//! How a `WindowSurface` bitmap is placed inside its window.
//!
//! The names match CSS `object-fit`. The fitted rectangle is centered. Pixels
//! outside that rectangle stay black.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObjectFit {
  None,
  Contain,
  Cover,
  Fill,
  ScaleDown,
}

/// CSS `image-rendering`. `auto` samples like `smooth` (bilinear).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageRendering {
  Auto,
  Smooth,
  CrispEdges,
  Pixelated,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PresentStyle {
  pub fit: ObjectFit,
  pub rendering: ImageRendering,
}

pub fn parse(value: &str) -> Option<ObjectFit> {
  let value = value.trim();
  if value.eq_ignore_ascii_case("none") {
    Some(ObjectFit::None)
  } else if value.eq_ignore_ascii_case("contain") {
    Some(ObjectFit::Contain)
  } else if value.eq_ignore_ascii_case("cover") {
    Some(ObjectFit::Cover)
  } else if value.eq_ignore_ascii_case("fill") {
    Some(ObjectFit::Fill)
  } else if value.eq_ignore_ascii_case("scale-down") {
    Some(ObjectFit::ScaleDown)
  } else {
    None
  }
}

pub fn name(fit: ObjectFit) -> &'static str {
  match fit {
    ObjectFit::None => "none",
    ObjectFit::Contain => "contain",
    ObjectFit::Cover => "cover",
    ObjectFit::Fill => "fill",
    ObjectFit::ScaleDown => "scale-down",
  }
}

pub fn parse_rendering(value: &str) -> Option<ImageRendering> {
  let value = value.trim();
  if value.eq_ignore_ascii_case("auto") {
    Some(ImageRendering::Auto)
  } else if value.eq_ignore_ascii_case("smooth")
    || value.eq_ignore_ascii_case("optimizeQuality")
  {
    Some(ImageRendering::Smooth)
  } else if value.eq_ignore_ascii_case("crisp-edges") {
    Some(ImageRendering::CrispEdges)
  } else if value.eq_ignore_ascii_case("pixelated")
    || value.eq_ignore_ascii_case("optimizeSpeed")
  {
    Some(ImageRendering::Pixelated)
  } else {
    None
  }
}

pub fn rendering_name(rendering: ImageRendering) -> &'static str {
  match rendering {
    ImageRendering::Auto => "auto",
    ImageRendering::Smooth => "smooth",
    ImageRendering::CrispEdges => "crisp-edges",
    ImageRendering::Pixelated => "pixelated",
  }
}

pub fn rendering_is_smooth(rendering: ImageRendering) -> bool {
  matches!(rendering, ImageRendering::Auto | ImageRendering::Smooth)
}

/// Destination rectangle in target pixels. `x` and `y` may be negative when
/// the fitted bitmap is larger than the window.
pub fn dest_rect(
  fit: ObjectFit,
  source_width: u32,
  source_height: u32,
  target_width: u32,
  target_height: u32,
) -> (i32, i32, u32, u32) {
  if source_width == 0
    || source_height == 0
    || target_width == 0
    || target_height == 0
  {
    return (0, 0, 0, 0);
  }
  let (width, height) = match fit {
    ObjectFit::Fill => (target_width, target_height),
    ObjectFit::Contain => scaled(
      source_width,
      source_height,
      contain_scale(source_width, source_height, target_width, target_height),
    ),
    ObjectFit::Cover => scaled(
      source_width,
      source_height,
      cover_scale(source_width, source_height, target_width, target_height),
    ),
    ObjectFit::None => (source_width, source_height),
    ObjectFit::ScaleDown => {
      if source_width <= target_width && source_height <= target_height {
        (source_width, source_height)
      } else {
        scaled(
          source_width,
          source_height,
          contain_scale(
            source_width,
            source_height,
            target_width,
            target_height,
          ),
        )
      }
    }
  };
  let x = ((f64::from(target_width) - f64::from(width)) / 2.0).round() as i32;
  let y = ((f64::from(target_height) - f64::from(height)) / 2.0).round() as i32;
  (x, y, width, height)
}

/// Paints `source` into a `target_width` by `target_height` buffer. Uncovered
/// pixels are opaque black.
pub fn composite(
  source: &[u8],
  source_width: u32,
  source_height: u32,
  target_width: u32,
  target_height: u32,
  fit: ObjectFit,
  rendering: ImageRendering,
) -> Vec<u8> {
  composite_pad(
    source,
    source_width,
    source_height,
    target_width,
    target_height,
    fit,
    rendering,
    255,
  )
}

/// Like [`composite`], with `pad_alpha` on pixels the source does not cover.
#[allow(clippy::too_many_arguments)]
pub fn composite_pad(
  source: &[u8],
  source_width: u32,
  source_height: u32,
  target_width: u32,
  target_height: u32,
  fit: ObjectFit,
  rendering: ImageRendering,
  pad_alpha: u8,
) -> Vec<u8> {
  let mut out = vec![0u8; target_width as usize * target_height as usize * 4];
  if pad_alpha != 0 {
    for pixel in out.as_chunks_mut::<4>().0 {
      pixel[3] = pad_alpha;
    }
  }
  if source_width == 0
    || source_height == 0
    || target_width == 0
    || target_height == 0
    || source.len() < source_width as usize * source_height as usize * 4
  {
    return out;
  }
  let (origin_x, origin_y, dest_width, dest_height) = dest_rect(
    fit,
    source_width,
    source_height,
    target_width,
    target_height,
  );
  if dest_width == 0 || dest_height == 0 {
    return out;
  }
  let scaled = scale(
    source,
    source_width,
    source_height,
    dest_width,
    dest_height,
    rendering,
  );
  for row in 0..dest_height {
    let target_y = origin_y + row as i32;
    if target_y < 0 || target_y >= target_height as i32 {
      continue;
    }
    for column in 0..dest_width {
      let target_x = origin_x + column as i32;
      if target_x < 0 || target_x >= target_width as i32 {
        continue;
      }
      let from = (row as usize * dest_width as usize + column as usize) * 4;
      let to =
        (target_y as usize * target_width as usize + target_x as usize) * 4;
      out[to..to + 4].copy_from_slice(&scaled[from..from + 4]);
    }
  }
  out
}

fn scale(
  source: &[u8],
  source_width: u32,
  source_height: u32,
  dest_width: u32,
  dest_height: u32,
  rendering: ImageRendering,
) -> Vec<u8> {
  match rendering {
    ImageRendering::Auto | ImageRendering::Smooth => resample(
      source,
      source_width,
      source_height,
      dest_width,
      dest_height,
      true,
    ),
    ImageRendering::CrispEdges => resample(
      source,
      source_width,
      source_height,
      dest_width,
      dest_height,
      false,
    ),
    ImageRendering::Pixelated => {
      let factor_x = integer_factor(source_width, dest_width);
      let factor_y = integer_factor(source_height, dest_height);
      let mid_width = source_width.saturating_mul(factor_x);
      let mid_height = source_height.saturating_mul(factor_y);
      if mid_width == dest_width && mid_height == dest_height {
        resample(
          source,
          source_width,
          source_height,
          dest_width,
          dest_height,
          false,
        )
      } else {
        let nearest = resample(
          source,
          source_width,
          source_height,
          mid_width,
          mid_height,
          false,
        );
        resample(
          &nearest,
          mid_width,
          mid_height,
          dest_width,
          dest_height,
          true,
        )
      }
    }
  }
}

fn resample(
  source: &[u8],
  source_width: u32,
  source_height: u32,
  dest_width: u32,
  dest_height: u32,
  smooth: bool,
) -> Vec<u8> {
  let mut out = vec![0u8; dest_width as usize * dest_height as usize * 4];
  for row in 0..dest_height {
    for column in 0..dest_width {
      let pixel = if smooth {
        let u = (f64::from(column) + 0.5) * f64::from(source_width)
          / f64::from(dest_width)
          - 0.5;
        let v = (f64::from(row) + 0.5) * f64::from(source_height)
          / f64::from(dest_height)
          - 0.5;
        sample_bilinear(source, source_width, source_height, u, v)
      } else {
        let x = ((u64::from(column) * u64::from(source_width))
          / u64::from(dest_width))
        .min(u64::from(source_width - 1)) as u32;
        let y = ((u64::from(row) * u64::from(source_height))
          / u64::from(dest_height))
        .min(u64::from(source_height - 1)) as u32;
        texel(source, source_width, x, y)
      };
      let to = (row as usize * dest_width as usize + column as usize) * 4;
      out[to..to + 4].copy_from_slice(&pixel);
    }
  }
  out
}

fn sample_bilinear(
  source: &[u8],
  source_width: u32,
  source_height: u32,
  u: f64,
  v: f64,
) -> [u8; 4] {
  let max_x = f64::from(source_width - 1);
  let max_y = f64::from(source_height - 1);
  let u = u.clamp(0.0, max_x);
  let v = v.clamp(0.0, max_y);
  let x0 = u.floor() as u32;
  let y0 = v.floor() as u32;
  let x1 = (x0 + 1).min(source_width - 1);
  let y1 = (y0 + 1).min(source_height - 1);
  let tx = u - f64::from(x0);
  let ty = v - f64::from(y0);
  let p00 = texel(source, source_width, x0, y0);
  let p10 = texel(source, source_width, x1, y0);
  let p01 = texel(source, source_width, x0, y1);
  let p11 = texel(source, source_width, x1, y1);
  let mut pixel = [0u8; 4];
  for channel in 0..4 {
    let top = lerp(p00[channel], p10[channel], tx);
    let bottom = lerp(p01[channel], p11[channel], tx);
    pixel[channel] = lerp_f64(top, bottom, ty).round().clamp(0.0, 255.0) as u8;
  }
  pixel
}

fn texel(source: &[u8], source_width: u32, x: u32, y: u32) -> [u8; 4] {
  let index = (y as usize * source_width as usize + x as usize) * 4;
  source[index..index + 4]
    .try_into()
    .unwrap_or([0, 0, 0, 255])
}

fn lerp(left: u8, right: u8, t: f64) -> f64 {
  f64::from(left) * (1.0 - t) + f64::from(right) * t
}

fn lerp_f64(left: f64, right: f64, t: f64) -> f64 {
  left * (1.0 - t) + right * t
}

fn integer_factor(source: u32, dest: u32) -> u32 {
  if source == 0 || dest <= source {
    return 1;
  }
  let factor = (f64::from(dest) / f64::from(source)).round();
  if !factor.is_finite() || factor < 1.0 {
    return 1;
  }
  let factor = factor as u64;
  if u64::from(source).saturating_mul(factor) > u64::from(u32::MAX) {
    return 1;
  }
  factor as u32
}

fn contain_scale(
  source_width: u32,
  source_height: u32,
  target_width: u32,
  target_height: u32,
) -> f64 {
  (f64::from(target_width) / f64::from(source_width))
    .min(f64::from(target_height) / f64::from(source_height))
}

fn cover_scale(
  source_width: u32,
  source_height: u32,
  target_width: u32,
  target_height: u32,
) -> f64 {
  (f64::from(target_width) / f64::from(source_width))
    .max(f64::from(target_height) / f64::from(source_height))
}

fn scaled(source_width: u32, source_height: u32, scale: f64) -> (u32, u32) {
  let width = (f64::from(source_width) * scale).round().max(1.0) as u32;
  let height = (f64::from(source_height) * scale).round().max(1.0) as u32;
  (width, height)
}

#[cfg(test)]
mod tests {
  use super::{ImageRendering, ObjectFit, composite, dest_rect};

  #[test]
  fn none_centers_without_scaling() {
    assert_eq!(dest_rect(ObjectFit::None, 2, 2, 4, 4), (1, 1, 2, 2));
  }

  #[test]
  fn contain_fits_inside() {
    assert_eq!(dest_rect(ObjectFit::Contain, 2, 2, 4, 2), (1, 0, 2, 2));
  }

  #[test]
  fn cover_can_extend_past_the_window() {
    assert_eq!(dest_rect(ObjectFit::Cover, 2, 2, 4, 2), (0, -1, 4, 4));
  }

  #[test]
  fn fill_matches_the_window() {
    assert_eq!(dest_rect(ObjectFit::Fill, 2, 1, 5, 3), (0, 0, 5, 3));
  }

  #[test]
  fn scale_down_does_not_grow() {
    assert_eq!(dest_rect(ObjectFit::ScaleDown, 2, 2, 4, 4), (1, 1, 2, 2));
    assert_eq!(dest_rect(ObjectFit::ScaleDown, 4, 4, 2, 2), (0, 0, 2, 2));
  }

  #[test]
  fn outside_pixels_are_black() {
    let source = [
      255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 0, 255,
    ];
    let image = composite(
      &source,
      2,
      2,
      4,
      4,
      ObjectFit::None,
      ImageRendering::CrispEdges,
    );
    assert_eq!(&image[0..4], &[0, 0, 0, 255]);
    let clear = super::composite_pad(
      &source,
      2,
      2,
      4,
      4,
      ObjectFit::None,
      ImageRendering::CrispEdges,
      0,
    );
    assert_eq!(&clear[0..4], &[0, 0, 0, 0]);
    let middle = (4 + 1) * 4;
    assert_eq!(&image[middle..middle + 4], &[255, 0, 0, 255]);
  }

  #[test]
  fn smooth_blends_and_crisp_edges_do_not() {
    let source = [255, 0, 0, 255, 0, 0, 255, 255];
    let crisp = composite(
      &source,
      2,
      1,
      4,
      1,
      ObjectFit::Fill,
      ImageRendering::CrispEdges,
    );
    let smooth =
      composite(&source, 2, 1, 4, 1, ObjectFit::Fill, ImageRendering::Smooth);
    assert_eq!(&crisp[4..8], &[255, 0, 0, 255]);
    assert_ne!(&smooth[4..8], &crisp[4..8]);
  }

  #[test]
  fn pixelated_integer_scale_matches_nearest() {
    let source = [255, 0, 0, 255, 0, 0, 255, 255];
    let crisp = composite(
      &source,
      2,
      1,
      4,
      1,
      ObjectFit::Fill,
      ImageRendering::CrispEdges,
    );
    let pixelated = composite(
      &source,
      2,
      1,
      4,
      1,
      ObjectFit::Fill,
      ImageRendering::Pixelated,
    );
    assert_eq!(pixelated, crisp);
  }
}
