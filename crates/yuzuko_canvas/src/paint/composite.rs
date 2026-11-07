//! Isolated Canvas 2D compositing.
//!
//! The rasterizer blends a draw only where that draw covers pixels. HTML `source-in`,
//! `copy`, `clear`, and the other Porter-Duff operators also change pixels
//! inside the clip that the shape misses. Shadows are the shape's alpha,
//! offset in canvas pixels and blurred, then composited before the shape.
//! Both run on a premultiplied sRGB buffer. `source-over` with no shadow
//! stays on the normal replay path.

use std::sync::Arc;

use kurbo::Affine;
use kurbo::Rect;
use kurbo::Shape;
use vello_cpu::peniko::BlendMode;
use vello_cpu::peniko::Compose;
use vello_cpu::peniko::Fill;
use vello_cpu::peniko::Mix;

use crate::paint::color::CssColor;
use crate::paint::record::Brush;
use crate::paint::record::Clip;
use crate::paint::record::Command;
use crate::paint::shade::Shade;
use crate::paint::shade::Shadow;

pub fn needs_isolated(command: &Command) -> bool {
  command_filter(command).is_some_and(|filter| !filter.is_empty())
    || command_shade(command).is_some_and(|shade| !shade.is_plain())
}

pub fn apply_one(
  dest: &mut [u8],
  command: &Command,
  width: u32,
  height: u32,
  render_plain: &dyn Fn(&[Command]) -> Option<Vec<u8>>,
) {
  let Some(shade) = command_shade(command) else {
    return;
  };
  let expected = byte_len(width, height);
  if dest.len() != expected || expected == 0 {
    return;
  }
  let ink_command = unclipped_ink(command);
  let Some(ink) = render_plain(std::slice::from_ref(&ink_command)) else {
    return;
  };
  if ink.len() != expected {
    return;
  }
  let coverage = match clip_coverage(command, width, height, render_plain) {
    ClipMask::Full => None,
    ClipMask::Masked(mask) => Some(mask),
    ClipMask::Failed => return,
  };
  let filter = command_filter(command).filter(|filter| !filter.is_empty());
  if filter.is_none() {
    if let Some(shadow) = shade.shadow {
      let bytes =
        shadow_layer(&ink_command, &ink, width, height, shadow, render_plain);
      composite_buffer(dest, &bytes, shade.blend, coverage.as_deref());
    }
    composite_buffer(dest, &ink, shade.blend, coverage.as_deref());
    return;
  }
  let mut source = vec![0u8; expected];
  if let Some(shadow) = shade.shadow {
    let bytes =
      shadow_layer(&ink_command, &ink, width, height, shadow, render_plain);
    source_over(&mut source, &bytes);
  }
  source_over(&mut source, &ink);
  if let Some(filter) = filter {
    crate::paint::filter::apply(&mut source, width, height, filter);
  }
  composite_buffer(dest, &source, shade.blend, coverage.as_deref());
}

/// Premultiplied source-over. Opaque and empty sources copy bytes.
pub fn source_over(dest: &mut [u8], src: &[u8]) {
  if dest.len() != src.len() {
    return;
  }
  for (dest_px, src_px) in dest.chunks_exact_mut(4).zip(src.chunks_exact(4)) {
    if src_px[3] == 0 {
      continue;
    }
    if src_px[3] == 255 || dest_px[3] == 0 {
      dest_px.copy_from_slice(src_px);
      continue;
    }
    let src_a = f32::from(src_px[3]) / 255.0;
    let inv = 1.0 - src_a;
    for channel in 0..4 {
      let value = f32::from(src_px[channel]) / 255.0
        + f32::from(dest_px[channel]) / 255.0 * inv;
      dest_px[channel] = quantize(value);
    }
  }
}

fn byte_len(width: u32, height: u32) -> usize {
  width as usize * height as usize * 4
}

fn command_filter(
  command: &Command,
) -> Option<&[crate::paint::filter::Resolved]> {
  match command {
    Command::Fill { filter, .. }
    | Command::Stroke { filter, .. }
    | Command::Glyphs { filter, .. }
    | Command::Image { filter, .. } => Some(filter.as_ref()),
    Command::Clear { .. } | Command::Put { .. } => None,
  }
}

fn command_shade(command: &Command) -> Option<Shade> {
  match command {
    Command::Fill { shade, .. }
    | Command::Stroke { shade, .. }
    | Command::Glyphs { shade, .. }
    | Command::Image { shade, .. } => Some(*shade),
    Command::Clear { .. } | Command::Put { .. } => None,
  }
}

fn command_clips(command: &Command) -> Arc<[Clip]> {
  match command {
    Command::Fill { clips, .. }
    | Command::Stroke { clips, .. }
    | Command::Glyphs { clips, .. }
    | Command::Image { clips, .. } => Arc::clone(clips),
    Command::Clear { .. } | Command::Put { .. } => Arc::from([]),
  }
}

/// Ink for the drawing model: source-over, with the context clip removed.
/// A pattern's no-repeat clip stays on the brush.
fn unclipped_ink(command: &Command) -> Command {
  let mut ink = command.clone();
  let plain = Shade::source_over();
  let empty: Arc<[Clip]> = Arc::from([]);
  match &mut ink {
    Command::Fill { shade, clips, .. } => {
      *shade = plain;
      *clips = Arc::clone(&empty);
    }
    Command::Stroke { shade, clips, .. } => {
      *shade = plain;
      *clips = Arc::clone(&empty);
    }
    Command::Glyphs { shade, clips, .. } => {
      *shade = plain;
      *clips = Arc::clone(&empty);
    }
    Command::Image { shade, clips, .. } => {
      *shade = plain;
      *clips = Arc::clone(&empty);
    }
    Command::Clear { .. } | Command::Put { .. } => {}
  }
  ink
}

enum ClipMask {
  Full,
  Masked(Vec<u8>),
  Failed,
}

fn clip_coverage(
  command: &Command,
  width: u32,
  height: u32,
  render_plain: &dyn Fn(&[Command]) -> Option<Vec<u8>>,
) -> ClipMask {
  let clips = command_clips(command);
  if clips.is_empty() {
    return ClipMask::Full;
  }
  let fill = Command::Fill {
    path: Rect::new(0.0, 0.0, f64::from(width), f64::from(height)).to_path(0.1),
    fill: Fill::NonZero,
    brush: Brush::Solid(CssColor::BLACK),
    transform: Affine::IDENTITY,
    brush_transform: Affine::IDENTITY,
    alpha: 1.0,
    shade: Shade::source_over(),
    filter: crate::paint::record::empty_filter(),
    clips,
  };
  let Some(mask) = render_plain(std::slice::from_ref(&fill)) else {
    return ClipMask::Failed;
  };
  if mask.len() != byte_len(width, height) {
    return ClipMask::Failed;
  }
  ClipMask::Masked(mask)
}

fn composite_buffer(
  dest: &mut [u8],
  src: &[u8],
  blend: BlendMode,
  coverage: Option<&[u8]>,
) {
  let pixels = dest.len() / 4;
  for index in 0..pixels {
    let cover = match coverage {
      None => 255,
      Some(mask) => mask[index * 4 + 3],
    };
    if cover == 0 {
      continue;
    }
    let source = pixel_at(src, index);
    if source[3] == 0.0 && !blend.is_destructive() {
      continue;
    }
    let composed = blend_pixel(pixel_at(dest, index), source, blend);
    let out = if cover == 255 {
      composed.map(quantize)
    } else {
      let t = f32::from(cover) / 255.0;
      let dest_px = pixel_at(dest, index);
      let mut mixed = [0u8; 4];
      for channel in 0..4 {
        let value = dest_px[channel] * (1.0 - t) + composed[channel] * t;
        mixed[channel] = quantize(value);
      }
      mixed
    };
    let start = index * 4;
    dest[start..start + 4].copy_from_slice(&out);
  }
}

fn pixel_at(bytes: &[u8], index: usize) -> [f32; 4] {
  let start = index * 4;
  [
    f32::from(bytes[start]) / 255.0,
    f32::from(bytes[start + 1]) / 255.0,
    f32::from(bytes[start + 2]) / 255.0,
    f32::from(bytes[start + 3]) / 255.0,
  ]
}

fn quantize(value: f32) -> u8 {
  (value * 255.0).round().clamp(0.0, 255.0) as u8
}

/// A shape outside the canvas can still cast a shadow into it. Rasterize
/// the unclipped ink in a window shifted by the offset, with a blur margin,
/// then crop back to the canvas. The shape itself keeps the canvas-sized ink.
fn shadow_layer(
  command: &Command,
  canvas_ink: &[u8],
  width: u32,
  height: u32,
  shadow: Shadow,
  render_plain: &dyn Fn(&[Command]) -> Option<Vec<u8>>,
) -> Vec<u8> {
  let sigma = (shadow.blur * 0.5).clamp(0.0, 128.0);
  let radius = blur_radius(sigma);
  let Some(buf_w) = width.checked_add(radius.saturating_mul(2)) else {
    return shadow_bytes(canvas_ink, width, height, shadow);
  };
  let Some(buf_h) = height.checked_add(radius.saturating_mul(2)) else {
    return shadow_bytes(canvas_ink, width, height, shadow);
  };
  let shifted = translate_command(
    command,
    f64::from(shadow.offset_x) + f64::from(radius),
    f64::from(shadow.offset_y) + f64::from(radius),
  );
  let Some(rendered) = render_plain(std::slice::from_ref(&shifted)) else {
    return shadow_bytes(canvas_ink, width, height, shadow);
  };
  if rendered.len() != byte_len(buf_w, buf_h) {
    return shadow_bytes(canvas_ink, width, height, shadow);
  }
  let mut alpha = alpha_plane(&rendered);
  if sigma > 0.0 {
    blur_plane(&mut alpha, buf_w as usize, buf_h as usize, sigma);
  }
  colorize_crop(&alpha, buf_w, width, height, radius, shadow)
}

fn translate_command(command: &Command, tx: f64, ty: f64) -> Command {
  let mut shifted = command.clone();
  let shift = Affine::translate((tx, ty));
  match &mut shifted {
    Command::Fill { transform, .. }
    | Command::Stroke { transform, .. }
    | Command::Glyphs { transform, .. }
    | Command::Image { transform, .. } => {
      *transform = shift * *transform;
    }
    Command::Clear { .. } | Command::Put { .. } => {}
  }
  shifted
}

fn alpha_plane(ink: &[u8]) -> Vec<f32> {
  ink
    .chunks_exact(4)
    .map(|pixel| f32::from(pixel[3]) / 255.0)
    .collect()
}

fn colorize_crop(
  alpha: &[f32],
  buf_w: u32,
  width: u32,
  height: u32,
  radius: u32,
  shadow: Shadow,
) -> Vec<u8> {
  let [red, green, blue, color_a] = shadow.color;
  let mut out = vec![0u8; byte_len(width, height)];
  let buf_w = buf_w as usize;
  let radius = radius as usize;
  for y in 0..height as usize {
    for x in 0..width as usize {
      let coverage = alpha[(y + radius) * buf_w + (x + radius)];
      let alpha = coverage * color_a;
      let start = (y * width as usize + x) * 4;
      out[start] = quantize(red * alpha);
      out[start + 1] = quantize(green * alpha);
      out[start + 2] = quantize(blue * alpha);
      out[start + 3] = quantize(alpha);
    }
  }
  out
}

fn blur_radius(sigma: f32) -> u32 {
  if sigma <= 0.0 {
    0
  } else {
    (3.0 * sigma).ceil().max(1.0) as u32
  }
}

fn shadow_bytes(
  ink: &[u8],
  width: u32,
  height: u32,
  shadow: Shadow,
) -> Vec<u8> {
  let alpha = shadowed_alpha(ink, width, height, shadow);
  let mut out = vec![0u8; alpha.len() * 4];
  let [red, green, blue, color_a] = shadow.color;
  for (index, coverage) in alpha.iter().enumerate() {
    let alpha = coverage * color_a;
    let start = index * 4;
    out[start] = quantize(red * alpha);
    out[start + 1] = quantize(green * alpha);
    out[start + 2] = quantize(blue * alpha);
    out[start + 3] = quantize(alpha);
  }
  out
}

fn shadowed_alpha(
  ink: &[u8],
  width: u32,
  height: u32,
  shadow: Shadow,
) -> Vec<f32> {
  let width_i = width as i32;
  let height_i = height as i32;
  let mut alpha = vec![0.0f32; width as usize * height as usize];
  for y in 0..height_i {
    for x in 0..width_i {
      let sample_x = x as f32 - shadow.offset_x;
      let sample_y = y as f32 - shadow.offset_y;
      alpha[(y as u32 * width + x as u32) as usize] =
        sample_alpha(ink, width_i, height_i, sample_x, sample_y);
    }
  }
  let sigma = (shadow.blur * 0.5).clamp(0.0, 128.0);
  if sigma > 0.0 {
    blur_plane(&mut alpha, width as usize, height as usize, sigma);
  }
  alpha
}

fn sample_alpha(ink: &[u8], width: i32, height: i32, x: f32, y: f32) -> f32 {
  let x0 = x.floor() as i32;
  let y0 = y.floor() as i32;
  let fx = x - x0 as f32;
  let fy = y - y0 as f32;
  let a00 = alpha_at(ink, width, height, x0, y0);
  let a10 = alpha_at(ink, width, height, x0 + 1, y0);
  let a01 = alpha_at(ink, width, height, x0, y0 + 1);
  let a11 = alpha_at(ink, width, height, x0 + 1, y0 + 1);
  let top = a00 * (1.0 - fx) + a10 * fx;
  let bottom = a01 * (1.0 - fx) + a11 * fx;
  top * (1.0 - fy) + bottom * fy
}

fn alpha_at(ink: &[u8], width: i32, height: i32, x: i32, y: i32) -> f32 {
  if x < 0 || y < 0 || x >= width || y >= height {
    return 0.0;
  }
  let index = ((y as u32 * width as u32 + x as u32) as usize) * 4 + 3;
  f32::from(ink[index]) / 255.0
}

pub(crate) fn blur_plane(
  alpha: &mut [f32],
  width: usize,
  height: usize,
  sigma: f32,
) {
  let kernel = kernel(sigma);
  let radius = (kernel.len() - 1) / 2;
  let mut horizontal = vec![0.0f32; alpha.len()];
  for y in 0..height {
    for x in 0..width {
      let mut sum = 0.0;
      for (index, weight) in kernel.iter().enumerate() {
        let sample = x as isize + index as isize - radius as isize;
        if sample >= 0 && (sample as usize) < width {
          sum += alpha[y * width + sample as usize] * weight;
        }
      }
      horizontal[y * width + x] = sum;
    }
  }
  for x in 0..width {
    for y in 0..height {
      let mut sum = 0.0;
      for (index, weight) in kernel.iter().enumerate() {
        let sample = y as isize + index as isize - radius as isize;
        if sample >= 0 && (sample as usize) < height {
          sum += horizontal[sample as usize * width + x] * weight;
        }
      }
      alpha[y * width + x] = sum;
    }
  }
}

fn kernel(sigma: f32) -> Vec<f32> {
  let radius = blur_radius(sigma) as usize;
  let denom = 2.0 * sigma * sigma;
  let mut weights = Vec::with_capacity(radius * 2 + 1);
  let mut sum = 0.0;
  for index in 0..=radius * 2 {
    let x = index as f32 - radius as f32;
    let weight = (-x * x / denom).exp();
    weights.push(weight);
    sum += weight;
  }
  for weight in &mut weights {
    *weight /= sum;
  }
  weights
}

fn blend_pixel(dst: [f32; 4], src: [f32; 4], mode: BlendMode) -> [f32; 4] {
  if src[3] == 0.0 && !mode.is_destructive() {
    return dst;
  }
  if mode.mix == Mix::Normal && mode.compose == Compose::SrcOver {
    let inv = 1.0 - src[3];
    return [
      src[0] + dst[0] * inv,
      src[1] + dst[1] * inv,
      src[2] + dst[2] * inv,
      src[3] + dst[3] * inv,
    ];
  }
  if mode.compose == Compose::PlusLighter {
    return [
      (src[0] + dst[0]).min(1.0),
      (src[1] + dst[1]).min(1.0),
      (src[2] + dst[2]).min(1.0),
      (src[3] + dst[3]).min(1.0),
    ];
  }
  let straight_src = separate(src);
  let straight_dst = separate(dst);
  let mixed = blend_mix(straight_dst, straight_src, mode.mix);
  let straight_src = mix3(straight_src, mixed, dst[3]);
  if mode.compose == Compose::SrcOver {
    let color = mix3([dst[0], dst[1], dst[2]], straight_src, src[3]);
    return [
      color[0],
      color[1],
      color[2],
      src[3] + dst[3] * (1.0 - src[3]),
    ];
  }
  porter(straight_dst, straight_src, dst[3], src[3], mode.compose)
}

fn separate(color: [f32; 4]) -> [f32; 3] {
  let inv = 1.0 / color[3].max(1.0e-15);
  [color[0] * inv, color[1] * inv, color[2] * inv]
}

fn mix3(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
  [
    a[0] * (1.0 - t) + b[0] * t,
    a[1] * (1.0 - t) + b[1] * t,
    a[2] * (1.0 - t) + b[2] * t,
  ]
}

fn porter(
  dst: [f32; 3],
  src: [f32; 3],
  dst_a: f32,
  src_a: f32,
  compose: Compose,
) -> [f32; 4] {
  let (src_factor, dst_factor) = match compose {
    Compose::Clear => (0.0, 0.0),
    Compose::Copy => (1.0, 0.0),
    Compose::Dest => (0.0, 1.0),
    Compose::SrcOver => (1.0, 1.0 - src_a),
    Compose::DestOver => (1.0 - dst_a, 1.0),
    Compose::SrcIn => (dst_a, 0.0),
    Compose::DestIn => (0.0, src_a),
    Compose::SrcOut => (1.0 - dst_a, 0.0),
    Compose::DestOut => (0.0, 1.0 - src_a),
    Compose::SrcAtop => (dst_a, 1.0 - src_a),
    Compose::DestAtop => (1.0 - dst_a, src_a),
    Compose::Xor => (1.0 - dst_a, 1.0 - src_a),
    Compose::Plus | Compose::PlusLighter => (1.0, 1.0),
  };
  let src_w = src_a * src_factor;
  let dst_w = dst_a * dst_factor;
  [
    src_w * src[0] + dst_w * dst[0],
    src_w * src[1] + dst_w * dst[1],
    src_w * src[2] + dst_w * dst[2],
    (src_w + dst_w).min(1.0),
  ]
}

fn blend_mix(backdrop: [f32; 3], source: [f32; 3], mix: Mix) -> [f32; 3] {
  match mix {
    Mix::Normal => source,
    Mix::Multiply => mul3(backdrop, source),
    Mix::Screen => screen(backdrop, source),
    Mix::Overlay => hard_light(source, backdrop),
    Mix::Darken => zip3(backdrop, source, f32::min),
    Mix::Lighten => zip3(backdrop, source, f32::max),
    Mix::ColorDodge => zip3(backdrop, source, color_dodge),
    Mix::ColorBurn => zip3(backdrop, source, color_burn),
    Mix::HardLight => hard_light(backdrop, source),
    Mix::SoftLight => zip3(backdrop, source, soft_light),
    Mix::Difference => zip3(backdrop, source, |cb, cs| (cb - cs).abs()),
    Mix::Exclusion => zip3(backdrop, source, |cb, cs| cb + cs - 2.0 * cb * cs),
    Mix::Hue => set_lum(set_sat(source, sat(backdrop)), lum(backdrop)),
    Mix::Saturation => set_lum(set_sat(backdrop, sat(source)), lum(backdrop)),
    Mix::Color => set_lum(source, lum(backdrop)),
    Mix::Luminosity => set_lum(backdrop, lum(source)),
  }
}

fn mul3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
  [a[0] * b[0], a[1] * b[1], a[2] * b[2]]
}

fn zip3(a: [f32; 3], b: [f32; 3], f: impl Fn(f32, f32) -> f32) -> [f32; 3] {
  [f(a[0], b[0]), f(a[1], b[1]), f(a[2], b[2])]
}

fn screen(backdrop: [f32; 3], source: [f32; 3]) -> [f32; 3] {
  zip3(backdrop, source, |cb, cs| cb + cs - cb * cs)
}

fn color_dodge(backdrop: f32, source: f32) -> f32 {
  if backdrop == 0.0 {
    0.0
  } else if source == 1.0 {
    1.0
  } else {
    (backdrop / (1.0 - source)).min(1.0)
  }
}

fn color_burn(backdrop: f32, source: f32) -> f32 {
  if backdrop == 1.0 {
    1.0
  } else if source == 0.0 {
    0.0
  } else {
    1.0 - ((1.0 - backdrop) / source).min(1.0)
  }
}

fn hard_light(backdrop: [f32; 3], source: [f32; 3]) -> [f32; 3] {
  zip3(backdrop, source, |cb, cs| {
    if cs <= 0.5 {
      cb * 2.0 * cs
    } else {
      let doubled = 2.0 * cs - 1.0;
      cb + doubled - cb * doubled
    }
  })
}

fn soft_light(backdrop: f32, source: f32) -> f32 {
  if source <= 0.5 {
    backdrop - (1.0 - 2.0 * source) * backdrop * (1.0 - backdrop)
  } else {
    let d = if backdrop <= 0.25 {
      ((16.0 * backdrop - 12.0) * backdrop + 4.0) * backdrop
    } else {
      backdrop.sqrt()
    };
    backdrop + (2.0 * source - 1.0) * (d - backdrop)
  }
}

fn sat(color: [f32; 3]) -> f32 {
  color[0].max(color[1]).max(color[2]) - color[0].min(color[1]).min(color[2])
}

fn lum(color: [f32; 3]) -> f32 {
  color[0] * 0.3 + color[1] * 0.59 + color[2] * 0.11
}

fn clip_color(color: [f32; 3]) -> [f32; 3] {
  let mut color = color;
  let l = lum(color);
  let n = color[0].min(color[1]).min(color[2]);
  let x = color[0].max(color[1]).max(color[2]);
  if n < 0.0 && (l - n).abs() > 1.0e-6 {
    let scale = l / (l - n);
    color = scale_from(l, color, scale);
  }
  if x > 1.0 && (x - l).abs() > 1.0e-6 {
    let scale = (1.0 - l) / (x - l);
    color = scale_from(l, color, scale);
  }
  color
}

fn scale_from(l: f32, color: [f32; 3], scale: f32) -> [f32; 3] {
  [
    l + (color[0] - l) * scale,
    l + (color[1] - l) * scale,
    l + (color[2] - l) * scale,
  ]
}

fn set_lum(color: [f32; 3], l: f32) -> [f32; 3] {
  let delta = l - lum(color);
  clip_color([color[0] + delta, color[1] + delta, color[2] + delta])
}

fn set_sat(color: [f32; 3], saturation: f32) -> [f32; 3] {
  let mut red = color[0];
  let mut green = color[1];
  let mut blue = color[2];
  if red <= green {
    if green <= blue {
      set_sat_inner(&mut red, &mut green, &mut blue, saturation);
    } else if red <= blue {
      set_sat_inner(&mut red, &mut blue, &mut green, saturation);
    } else {
      set_sat_inner(&mut blue, &mut red, &mut green, saturation);
    }
  } else if red <= blue {
    set_sat_inner(&mut green, &mut red, &mut blue, saturation);
  } else if green <= blue {
    set_sat_inner(&mut green, &mut blue, &mut red, saturation);
  } else {
    set_sat_inner(&mut blue, &mut green, &mut red, saturation);
  }
  [red, green, blue]
}

fn set_sat_inner(
  cmin: &mut f32,
  cmid: &mut f32,
  cmax: &mut f32,
  saturation: f32,
) {
  if *cmax > *cmin {
    *cmid = ((*cmid - *cmin) * saturation) / (*cmax - *cmin);
    *cmax = saturation;
  } else {
    *cmid = 0.0;
    *cmax = 0.0;
  }
  *cmin = 0.0;
}

#[cfg(test)]
mod tests {
  use super::blend_pixel;
  use super::quantize;
  use super::shadow_bytes;
  use crate::paint::shade::Shadow;
  use vello_cpu::peniko::BlendMode;
  use vello_cpu::peniko::Compose;
  use vello_cpu::peniko::Mix;

  fn mode(mix: Mix, compose: Compose) -> BlendMode {
    BlendMode::new(mix, compose)
  }

  fn bytes(pixel: [f32; 4]) -> [u8; 4] {
    pixel.map(quantize)
  }

  #[test]
  fn porter_duff_and_multiply_match_premultiplied_bytes() {
    let red = [1.0, 0.0, 0.0, 1.0];
    let blue = [0.0, 0.0, 1.0, 1.0];
    let clear = [0.0, 0.0, 0.0, 0.0];
    let source_in = mode(Mix::Normal, Compose::SrcIn);
    assert_eq!(bytes(blend_pixel(red, clear, source_in)), [0, 0, 0, 0]);
    assert_eq!(bytes(blend_pixel(red, blue, source_in)), [0, 0, 255, 255]);

    let copy = mode(Mix::Normal, Compose::Copy);
    assert_eq!(bytes(blend_pixel(red, clear, copy)), [0, 0, 0, 0]);

    let lighter = mode(Mix::Normal, Compose::Plus);
    let cyan = [0.0, 1.0, 1.0, 1.0];
    let yellow = [1.0, 1.0, 0.0, 1.0];
    assert_eq!(
      bytes(blend_pixel(cyan, yellow, lighter)),
      [255, 255, 255, 255]
    );

    let multiply = mode(Mix::Multiply, Compose::SrcOver);
    assert_eq!(bytes(blend_pixel(yellow, blue, multiply)), [0, 0, 0, 255]);

    let over = BlendMode::default();
    assert_eq!(bytes(blend_pixel(red, clear, over)), [255, 0, 0, 255]);
  }

  #[test]
  fn an_integer_offset_copies_alpha_and_blur_falls_off() {
    let mut ink = vec![0u8; 4 * 2 * 4];
    ink[0..4].copy_from_slice(&[255, 0, 0, 255]);
    ink[4..8].copy_from_slice(&[255, 0, 0, 255]);
    let moved = shadow_bytes(
      &ink,
      4,
      2,
      Shadow {
        blur: 0.0,
        offset_x: 2.0,
        offset_y: 0.0,
        color: [0.0, 0.0, 0.0, 1.0],
      },
    );
    assert_eq!(&moved[0..4], &[0, 0, 0, 0]);
    assert_eq!(&moved[8..12], &[0, 0, 0, 255]);

    let mut dot = vec![0u8; 11 * 11 * 4];
    dot[(5 * 11 + 5) * 4 + 3] = 255;
    let blurred = shadow_bytes(
      &dot,
      11,
      11,
      Shadow {
        blur: 2.0,
        offset_x: 0.0,
        offset_y: 0.0,
        color: [0.0, 0.0, 1.0, 1.0],
      },
    );
    let alpha = |x: usize, y: usize| blurred[(y * 11 + x) * 4 + 3];
    assert!(alpha(5, 5) > alpha(6, 5));
    assert!(alpha(6, 5) > 0);
    assert_eq!(alpha(0, 5), 0);
    assert!(blurred[(6 * 11 + 5) * 4 + 2] > 0);
  }
}
