//! CSS `<filter-value-list>` parsing and painting for Canvas 2D.
//!
//! The rasterizer does not run CSS filters. A drawing that has one is
//! rasterized to premultiplied sRGB8 and the functions below run on that
//! bitmap.

use cssparser::BasicParseErrorKind;
use cssparser::ParseError;
use cssparser::Parser;
use cssparser::Token;

use crate::paint::color::CssColor;
use crate::paint::color::parse_color;
use crate::paint::composite;

/// `true` when `input` is `none` or a `<filter-value-list>`.
pub fn is_filter(input: &str) -> bool {
  parse(input).is_some()
}

/// `Some` empty list is `none`. `None` does not parse.
pub fn parse(input: &str) -> Option<Vec<FilterOp>> {
  let mut parser = Parser::new(input);
  parse_filter(&mut parser).ok()
}

#[derive(Clone, Copy, Debug)]
pub struct Length {
  pub value: f32,
  pub unit: Unit,
}

impl Length {
  fn zero() -> Self {
    Self {
      value: 0.0,
      unit: Unit::Px,
    }
  }
}

#[derive(Clone, Copy, Debug)]
pub enum Unit {
  Px,
  Cm,
  Mm,
  Q,
  In,
  Pt,
  Pc,
  Em,
  Rem,
  Ex,
  Ch,
  Cap,
  Ic,
  Lh,
  Rlh,
  Vw,
  Vh,
  Vi,
  Vb,
  Vmin,
  Vmax,
  Dvw,
  Dvh,
  Dvi,
  Dvb,
  Dvmin,
  Dvmax,
  Svw,
  Svh,
  Svi,
  Svb,
  Svmin,
  Svmax,
  Lvw,
  Lvh,
  Lvi,
  Lvb,
  Lvmin,
  Lvmax,
  Cqw,
  Cqh,
  Cqi,
  Cqb,
  Cqmin,
  Cqmax,
}

#[derive(Clone, Debug)]
pub enum FilterOp {
  Blur(Length),
  Brightness(f32),
  Contrast(f32),
  Grayscale(f32),
  HueRotate(f32),
  Invert(f32),
  Opacity(f32),
  Saturate(f32),
  Sepia(f32),
  DropShadow {
    x: Length,
    y: Length,
    blur: Length,
    color: CssColor,
  },
  /// No SVG document exists, so this function clears the bitmap.
  Url,
}

/// Lengths are canvas pixels. `font_px` resolves `em` and `rem`.
#[derive(Clone, Copy, Debug)]
pub enum Resolved {
  Blur(f32),
  Brightness(f32),
  Contrast(f32),
  Grayscale(f32),
  HueRotate(f32),
  Invert(f32),
  Opacity(f32),
  Saturate(f32),
  Sepia(f32),
  DropShadow {
    x: f32,
    y: f32,
    blur: f32,
    color: [f32; 4],
  },
  Url,
}

pub fn resolve(
  ops: &[FilterOp],
  font_px: f32,
  width: f32,
  height: f32,
) -> Vec<Resolved> {
  ops
    .iter()
    .map(|op| resolve_one(op, font_px, width, height))
    .collect()
}

fn resolve_one(
  op: &FilterOp,
  font_px: f32,
  width: f32,
  height: f32,
) -> Resolved {
  let length = |value: Length| resolve_length(value, font_px, width, height);
  match *op {
    FilterOp::Blur(value) => Resolved::Blur(length(value).max(0.0)),
    FilterOp::Brightness(value) => Resolved::Brightness(value),
    FilterOp::Contrast(value) => Resolved::Contrast(value),
    FilterOp::Grayscale(value) => Resolved::Grayscale(value),
    FilterOp::HueRotate(value) => Resolved::HueRotate(value),
    FilterOp::Invert(value) => Resolved::Invert(value),
    FilterOp::Opacity(value) => Resolved::Opacity(value),
    FilterOp::Saturate(value) => Resolved::Saturate(value),
    FilterOp::Sepia(value) => Resolved::Sepia(value),
    FilterOp::DropShadow { x, y, blur, color } => Resolved::DropShadow {
      x: length(x),
      y: length(y),
      blur: length(blur).max(0.0),
      color: [color.r, color.g, color.b, color.a],
    },
    FilterOp::Url => Resolved::Url,
  }
}

fn resolve_length(
  length: Length,
  font_px: f32,
  width: f32,
  height: f32,
) -> f32 {
  let value = length.value;
  let smaller = width.min(height);
  let larger = width.max(height);
  match length.unit {
    Unit::Px => value,
    Unit::In => value * 96.0,
    Unit::Cm => value * 96.0 / 2.54,
    Unit::Mm => value * 96.0 / 25.4,
    Unit::Q => value * 96.0 / 2.54 / 40.0,
    Unit::Pt => value * 96.0 / 72.0,
    Unit::Pc => value * 16.0,
    Unit::Em | Unit::Rem | Unit::Ic => value * font_px,
    Unit::Ex | Unit::Ch => value * font_px * 0.5,
    Unit::Cap => value * font_px * 0.7,
    Unit::Lh | Unit::Rlh => value * font_px * 1.2,
    Unit::Vw
    | Unit::Dvw
    | Unit::Svw
    | Unit::Lvw
    | Unit::Cqw
    | Unit::Vi
    | Unit::Dvi
    | Unit::Svi
    | Unit::Lvi
    | Unit::Cqi => value * width / 100.0,
    Unit::Vh
    | Unit::Dvh
    | Unit::Svh
    | Unit::Lvh
    | Unit::Cqh
    | Unit::Vb
    | Unit::Dvb
    | Unit::Svb
    | Unit::Lvb
    | Unit::Cqb => value * height / 100.0,
    Unit::Vmin | Unit::Dvmin | Unit::Svmin | Unit::Lvmin | Unit::Cqmin => {
      value * smaller / 100.0
    }
    Unit::Vmax | Unit::Dvmax | Unit::Svmax | Unit::Lvmax | Unit::Cqmax => {
      value * larger / 100.0
    }
  }
}

/// Applies `ops` left to right. `pixels` is premultiplied sRGB8.
/// A `url()` function clears the bitmap. There is no SVG filter.
pub fn apply(pixels: &mut [u8], width: u32, height: u32, ops: &[Resolved]) {
  let expected = width as usize * height as usize * 4;
  if pixels.len() != expected || expected == 0 || ops.is_empty() {
    return;
  }
  for op in ops {
    if matches!(op, Resolved::Url) {
      pixels.fill(0);
      return;
    }
    apply_one(pixels, width, height, *op);
  }
}

fn apply_one(pixels: &mut [u8], width: u32, height: u32, op: Resolved) {
  match op {
    Resolved::Blur(sigma) => blur_premul(pixels, width, height, sigma),
    Resolved::Brightness(amount) => {
      map_color(pixels, |r, g, b, a| [r * amount, g * amount, b * amount, a])
    }
    Resolved::Contrast(amount) => map_color(pixels, |r, g, b, a| {
      let contrast = |channel: f32| (channel - 0.5) * amount + 0.5;
      [contrast(r), contrast(g), contrast(b), a]
    }),
    Resolved::Grayscale(amount) => map_color(pixels, |r, g, b, a| {
      let y = 0.2126 * r + 0.7152 * g + 0.0722 * b;
      let mix = |channel: f32| channel * (1.0 - amount) + y * amount;
      [mix(r), mix(g), mix(b), a]
    }),
    Resolved::HueRotate(radians) => {
      map_color(pixels, |r, g, b, a| hue_rotate(r, g, b, radians, a))
    }
    Resolved::Invert(amount) => map_color(pixels, |r, g, b, a| {
      let invert =
        |channel: f32| channel * (1.0 - amount) + (1.0 - channel) * amount;
      [invert(r), invert(g), invert(b), a]
    }),
    Resolved::Opacity(amount) => {
      map_color(pixels, |r, g, b, a| [r, g, b, a * amount])
    }
    Resolved::Saturate(amount) => map_color(pixels, |r, g, b, a| {
      let y = 0.213 * r + 0.715 * g + 0.072 * b;
      let mix = |channel: f32| y * (1.0 - amount) + channel * amount;
      [mix(r), mix(g), mix(b), a]
    }),
    Resolved::Sepia(amount) => map_color(pixels, |r, g, b, a| {
      let sr = 0.393 * r + 0.769 * g + 0.189 * b;
      let sg = 0.349 * r + 0.686 * g + 0.168 * b;
      let sb = 0.272 * r + 0.534 * g + 0.131 * b;
      let mix =
        |channel: f32, toward: f32| channel * (1.0 - amount) + toward * amount;
      [mix(r, sr), mix(g, sg), mix(b, sb), a]
    }),
    Resolved::DropShadow { x, y, blur, color } => {
      drop_shadow(pixels, width, height, x, y, blur, color);
    }
    Resolved::Url => pixels.fill(0),
  }
}

fn hue_rotate(r: f32, g: f32, b: f32, radians: f32, a: f32) -> [f32; 4] {
  let (sin, cos) = radians.sin_cos();
  let row = |c0: f32, c1: f32, c2: f32| c0 * r + c1 * g + c2 * b;
  [
    row(
      0.213 + cos * 0.787 - sin * 0.213,
      0.715 - cos * 0.715 - sin * 0.715,
      0.072 - cos * 0.072 + sin * 0.928,
    ),
    row(
      0.213 - cos * 0.213 + sin * 0.143,
      0.715 + cos * 0.285 + sin * 0.140,
      0.072 - cos * 0.072 - sin * 0.283,
    ),
    row(
      0.213 - cos * 0.213 - sin * 0.787,
      0.715 - cos * 0.715 + sin * 0.715,
      0.072 + cos * 0.928 + sin * 0.072,
    ),
    a,
  ]
}

fn map_color(
  pixels: &mut [u8],
  mut map: impl FnMut(f32, f32, f32, f32) -> [f32; 4],
) {
  for pixel in pixels.chunks_exact_mut(4) {
    let a = f32::from(pixel[3]) / 255.0;
    let (r, g, b) = if a == 0.0 {
      (0.0, 0.0, 0.0)
    } else {
      (
        f32::from(pixel[0]) / 255.0 / a,
        f32::from(pixel[1]) / 255.0 / a,
        f32::from(pixel[2]) / 255.0 / a,
      )
    };
    let [r, g, b, a] = map(r, g, b, a);
    let a = if a.is_finite() {
      a.clamp(0.0, 1.0)
    } else {
      0.0
    };
    pixel[0] = quantize(r * a);
    pixel[1] = quantize(g * a);
    pixel[2] = quantize(b * a);
    pixel[3] = quantize(a);
  }
}

fn quantize(value: f32) -> u8 {
  if !value.is_finite() {
    return 0;
  }
  (value * 255.0).round().clamp(0.0, 255.0) as u8
}

fn blur_premul(pixels: &mut [u8], width: u32, height: u32, sigma: f32) {
  let sigma = sigma.clamp(0.0, 128.0);
  if sigma <= 0.0 {
    return;
  }
  let width = width as usize;
  let height = height as usize;
  let count = width * height;
  let mut planes = [
    vec![0.0f32; count],
    vec![0.0; count],
    vec![0.0; count],
    vec![0.0; count],
  ];
  for (index, pixel) in pixels.chunks_exact(4).enumerate() {
    for channel in 0..4 {
      planes[channel][index] = f32::from(pixel[channel]) / 255.0;
    }
  }
  for plane in &mut planes {
    composite::blur_plane(plane, width, height, sigma);
  }
  for index in 0..count {
    for channel in 0..4 {
      pixels[index * 4 + channel] = quantize(planes[channel][index]);
    }
  }
}

fn drop_shadow(
  pixels: &mut [u8],
  width: u32,
  height: u32,
  offset_x: f32,
  offset_y: f32,
  blur: f32,
  color: [f32; 4],
) {
  let width_i = width as i32;
  let height_i = height as i32;
  let count = width as usize * height as usize;
  let mut alpha = vec![0.0f32; count];
  for y in 0..height_i {
    for x in 0..width_i {
      alpha[(y as u32 * width + x as u32) as usize] = sample_alpha(
        pixels,
        width_i,
        height_i,
        x as f32 - offset_x,
        y as f32 - offset_y,
      );
    }
  }
  let sigma = blur.clamp(0.0, 128.0);
  if sigma > 0.0 {
    composite::blur_plane(&mut alpha, width as usize, height as usize, sigma);
  }
  let image = pixels.to_vec();
  let [red, green, blue, color_a] = color;
  for (index, coverage) in alpha.iter().enumerate() {
    let alpha = (coverage * color_a).clamp(0.0, 1.0);
    let start = index * 4;
    pixels[start] = quantize(red * alpha);
    pixels[start + 1] = quantize(green * alpha);
    pixels[start + 2] = quantize(blue * alpha);
    pixels[start + 3] = quantize(alpha);
  }
  composite::source_over(pixels, &image);
}

fn sample_alpha(pixels: &[u8], width: i32, height: i32, x: f32, y: f32) -> f32 {
  let x0 = x.floor() as i32;
  let y0 = y.floor() as i32;
  let fx = x - x0 as f32;
  let fy = y - y0 as f32;
  let sample = |x: i32, y: i32| -> f32 {
    if x < 0 || y < 0 || x >= width || y >= height {
      return 0.0;
    }
    let index = ((y as u32 * width as u32 + x as u32) as usize) * 4 + 3;
    f32::from(pixels[index]) / 255.0
  };
  let top = sample(x0, y0) * (1.0 - fx) + sample(x0 + 1, y0) * fx;
  let bottom = sample(x0, y0 + 1) * (1.0 - fx) + sample(x0 + 1, y0 + 1) * fx;
  top * (1.0 - fy) + bottom * fy
}

fn parse_filter(
  parser: &mut Parser<'_>,
) -> Result<Vec<FilterOp>, ParseError<()>> {
  if parser.is_exhausted() {
    return Err(fail(parser));
  }
  let state = parser.state();
  if let Ok(ident) = parser.expect_ident() {
    let wide = is_wide_keyword(&ident);
    let none = ident.eq_ignore_ascii_case("none");
    if parser.is_exhausted() {
      if none && !wide {
        return Ok(Vec::new());
      }
      return Err(fail(parser));
    }
  }
  parser.reset(&state);
  let mut ops = Vec::new();
  while !parser.is_exhausted() {
    ops.push(parse_item(parser)?);
  }
  if ops.is_empty() {
    Err(fail(parser))
  } else {
    Ok(ops)
  }
}

fn parse_item(parser: &mut Parser<'_>) -> Result<FilterOp, ParseError<()>> {
  let state = parser.state();
  if parser.expect_url().is_ok() {
    return Ok(FilterOp::Url);
  }
  parser.reset(&state);
  let name = parser.expect_function()?.to_ascii_lowercase();
  parser.parse_nested_block(|parser| match name.as_str() {
    "blur" => Ok(FilterOp::Blur(optional_length(parser, true)?)),
    "brightness" => Ok(FilterOp::Brightness(optional_amount(parser, 1.0)?)),
    "contrast" => Ok(FilterOp::Contrast(optional_amount(parser, 1.0)?)),
    "grayscale" => Ok(FilterOp::Grayscale(optional_amount(parser, 1.0)?)),
    "invert" => Ok(FilterOp::Invert(optional_amount(parser, 1.0)?)),
    "opacity" => Ok(FilterOp::Opacity(optional_amount(parser, 1.0)?)),
    "saturate" => Ok(FilterOp::Saturate(optional_amount(parser, 1.0)?)),
    "sepia" => Ok(FilterOp::Sepia(optional_amount(parser, 1.0)?)),
    "hue-rotate" => Ok(FilterOp::HueRotate(optional_angle(parser)?)),
    "drop-shadow" => drop_shadow_op(parser),
    _ => Err(fail(parser)),
  })
}

fn optional_length(
  parser: &mut Parser<'_>,
  nonnegative: bool,
) -> Result<Length, ParseError<()>> {
  if parser.is_exhausted() {
    return Ok(Length::zero());
  }
  let value = length(parser)?;
  if nonnegative && value.value < 0.0 {
    return Err(fail(parser));
  }
  if parser.is_exhausted() {
    Ok(value)
  } else {
    Err(fail(parser))
  }
}

fn optional_amount(
  parser: &mut Parser<'_>,
  default: f32,
) -> Result<f32, ParseError<()>> {
  if parser.is_exhausted() {
    return Ok(default);
  }
  let value = number_or_percent(parser)?;
  if parser.is_exhausted() {
    Ok(value)
  } else {
    Err(fail(parser))
  }
}

fn number_or_percent(parser: &mut Parser<'_>) -> Result<f32, ParseError<()>> {
  let state = parser.state();
  if let Ok(value) = parser.try_parse(Parser::expect_number) {
    return Ok(value);
  }
  parser.reset(&state);
  Ok(parser.expect_percentage()?)
}

fn optional_angle(parser: &mut Parser<'_>) -> Result<f32, ParseError<()>> {
  if parser.is_exhausted() {
    return Ok(0.0);
  }
  let value = angle(parser)?;
  if parser.is_exhausted() {
    Ok(value)
  } else {
    Err(fail(parser))
  }
}

fn length(parser: &mut Parser<'_>) -> Result<Length, ParseError<()>> {
  let state = parser.state();
  if let Ok(value) = parser.try_parse(Parser::expect_number) {
    if value == 0.0 {
      return Ok(Length::zero());
    }
    parser.reset(&state);
    return Err(fail(parser));
  }
  let parsed = {
    let token = parser.next()?;
    match token {
      Token::Dimension { value, unit, .. } => {
        length_unit(unit).map(|unit| Length {
          value: *value,
          unit,
        })
      }
      _ => None,
    }
  };
  parsed.ok_or_else(|| fail(parser))
}

fn angle(parser: &mut Parser<'_>) -> Result<f32, ParseError<()>> {
  let state = parser.state();
  if let Ok(value) = parser.try_parse(Parser::expect_number) {
    if value == 0.0 {
      return Ok(0.0);
    }
    parser.reset(&state);
    return Err(fail(parser));
  }
  let radians = {
    let token = parser.next()?;
    match token {
      Token::Dimension { value, unit, .. } => angle_radians(*value, unit),
      _ => None,
    }
  };
  radians.ok_or_else(|| fail(parser))
}

fn angle_radians(value: f32, unit: &str) -> Option<f32> {
  let radians = match unit.to_ascii_lowercase().as_str() {
    "deg" => value * std::f32::consts::PI / 180.0,
    "rad" => value,
    "grad" => value * std::f32::consts::PI / 200.0,
    "turn" => value * std::f32::consts::TAU,
    _ => return None,
  };
  Some(radians)
}

fn drop_shadow_op(parser: &mut Parser<'_>) -> Result<FilterOp, ParseError<()>> {
  let mut lengths = Vec::new();
  let mut color = None;
  while !parser.is_exhausted() {
    let state = parser.state();
    if let Ok(value) = parser.try_parse(length) {
      lengths.push(value);
      continue;
    }
    parser.reset(&state);
    if color.is_some() {
      return Err(fail(parser));
    }
    color = Some(color_value(parser)?);
  }
  if !(2..=3).contains(&lengths.len()) {
    return Err(fail(parser));
  }
  if lengths.len() == 3 && lengths[2].value < 0.0 {
    return Err(fail(parser));
  }
  Ok(FilterOp::DropShadow {
    x: lengths[0],
    y: lengths[1],
    blur: lengths.get(2).copied().unwrap_or_else(Length::zero),
    color: color.unwrap_or(CssColor::BLACK),
  })
}

fn color_value(parser: &mut Parser<'_>) -> Result<CssColor, ParseError<()>> {
  let start = parser.position();
  let kind = {
    let token = parser.next()?;
    match token {
      Token::Ident(name) => ColorToken::Word(name.to_string()),
      Token::Hash(hex) | Token::IDHash(hex) => {
        ColorToken::Word(format!("#{hex}"))
      }
      Token::Function(_) => ColorToken::Function,
      _ => ColorToken::Invalid,
    }
  };
  match kind {
    ColorToken::Invalid => Err(fail(parser)),
    ColorToken::Word(text) => parse_color(&text).ok_or_else(|| fail(parser)),
    ColorToken::Function => {
      parser.parse_nested_block(|inner| {
        while inner.next().is_ok() {}
        Ok(())
      })?;
      let text = parser.slice_from(start);
      parse_color(text).ok_or_else(|| fail(parser))
    }
  }
}

enum ColorToken {
  Word(String),
  Function,
  Invalid,
}

fn is_wide_keyword(ident: &str) -> bool {
  matches!(
    ident.to_ascii_lowercase().as_str(),
    "inherit" | "initial" | "unset" | "revert" | "revert-layer"
  )
}

fn length_unit(unit: &str) -> Option<Unit> {
  Some(match unit.to_ascii_lowercase().as_str() {
    "px" => Unit::Px,
    "cm" => Unit::Cm,
    "mm" => Unit::Mm,
    "q" => Unit::Q,
    "in" => Unit::In,
    "pt" => Unit::Pt,
    "pc" => Unit::Pc,
    "em" => Unit::Em,
    "rem" => Unit::Rem,
    "ex" => Unit::Ex,
    "ch" => Unit::Ch,
    "cap" => Unit::Cap,
    "ic" => Unit::Ic,
    "lh" => Unit::Lh,
    "rlh" => Unit::Rlh,
    "vw" => Unit::Vw,
    "vh" => Unit::Vh,
    "vi" => Unit::Vi,
    "vb" => Unit::Vb,
    "vmin" => Unit::Vmin,
    "vmax" => Unit::Vmax,
    "dvw" => Unit::Dvw,
    "dvh" => Unit::Dvh,
    "dvi" => Unit::Dvi,
    "dvb" => Unit::Dvb,
    "dvmin" => Unit::Dvmin,
    "dvmax" => Unit::Dvmax,
    "svw" => Unit::Svw,
    "svh" => Unit::Svh,
    "svi" => Unit::Svi,
    "svb" => Unit::Svb,
    "svmin" => Unit::Svmin,
    "svmax" => Unit::Svmax,
    "lvw" => Unit::Lvw,
    "lvh" => Unit::Lvh,
    "lvi" => Unit::Lvi,
    "lvb" => Unit::Lvb,
    "lvmin" => Unit::Lvmin,
    "lvmax" => Unit::Lvmax,
    "cqw" => Unit::Cqw,
    "cqh" => Unit::Cqh,
    "cqi" => Unit::Cqi,
    "cqb" => Unit::Cqb,
    "cqmin" => Unit::Cqmin,
    "cqmax" => Unit::Cqmax,
    _ => return None,
  })
}

fn fail(_parser: &mut Parser<'_>) -> ParseError<()> {
  ParseError::from_basic_kind(BasicParseErrorKind::QualifiedRuleInvalid)
}

#[cfg(test)]
mod tests {
  use super::is_filter;

  #[test]
  fn accepts_none_and_function_lists() {
    assert!(is_filter("none"));
    assert!(is_filter(" NONE "));
    assert!(is_filter("blur(5px)"));
    assert!(is_filter("blur(  5px)"));
    assert!(is_filter("blur()"));
    assert!(is_filter("blur(0)"));
    assert!(is_filter("blur(1em)"));
    assert!(is_filter("BLUR(1PX)"));
    assert!(is_filter("contrast(50%)"));
    assert!(is_filter("brightness(1.5)"));
    assert!(is_filter("hue-rotate(90deg)"));
    assert!(is_filter("hue-rotate(0)"));
    assert!(is_filter("hue-rotate(0.25turn)"));
    assert!(is_filter("opacity(50%) sepia(1)"));
    assert!(is_filter("drop-shadow(10px 5px 0px rgb(255, 165, 0))"));
    assert!(is_filter("drop-shadow(orange 10px 10px)"));
    assert!(is_filter("drop-shadow(-9px 9px 3px #e81)"));
    assert!(is_filter("url(#edge) blur(1px)"));
    assert!(is_filter("blur(5px) drop-shadow(10px 10px 5px orange)"));
  }

  #[test]
  fn rejects_unparsable_filters() {
    assert!(!is_filter(""));
    assert!(!is_filter("   "));
    assert!(!is_filter("blur(10)"));
    assert!(!is_filter("blur 10px"));
    assert!(!is_filter("blur(-1px)"));
    assert!(!is_filter("inherit"));
    assert!(!is_filter("initial"));
    assert!(!is_filter("unset"));
    assert!(!is_filter("revert"));
    assert!(!is_filter("null"));
    assert!(!is_filter("undefined"));
    assert!(!is_filter("none blur(1px)"));
    assert!(!is_filter("hue-rotate(90)"));
    assert!(!is_filter("drop-shadow(10px)"));
    assert!(!is_filter("drop-shadow(10px 10px -1px red)"));
    assert!(!is_filter("calc(1px)"));
    assert!(!is_filter("this string is not a filter"));
  }

  #[test]
  fn resolves_lengths_and_paints_color_functions() {
    let blur = super::parse("blur(1em)").unwrap();
    let resolved = super::resolve(&blur, 10.0, 100.0, 50.0);
    assert!(
      matches!(resolved[0], super::Resolved::Blur(sigma) if sigma == 10.0)
    );
    let viewport = super::parse("blur(10vw)").unwrap();
    let resolved = super::resolve(&viewport, 10.0, 200.0, 40.0);
    assert!(
      matches!(resolved[0], super::Resolved::Blur(sigma) if sigma == 20.0)
    );

    let mut red = vec![255, 0, 0, 255];
    super::apply(
      &mut red,
      1,
      1,
      &super::resolve(&super::parse("brightness(0)").unwrap(), 10.0, 1.0, 1.0),
    );
    assert_eq!(red, vec![0, 0, 0, 255]);

    let mut red = vec![255, 0, 0, 255];
    super::apply(
      &mut red,
      1,
      1,
      &super::resolve(&super::parse("invert(1)").unwrap(), 10.0, 1.0, 1.0),
    );
    assert_eq!(red, vec![0, 255, 255, 255]);

    let mut red = vec![255, 0, 0, 255];
    super::apply(
      &mut red,
      1,
      1,
      &super::resolve(&super::parse("opacity(0)").unwrap(), 10.0, 1.0, 1.0),
    );
    assert_eq!(red, vec![0, 0, 0, 0]);

    let mut red = vec![255, 0, 0, 255];
    super::apply(
      &mut red,
      1,
      1,
      &super::resolve(&super::parse("url(#missing)").unwrap(), 10.0, 1.0, 1.0),
    );
    assert_eq!(red, vec![0, 0, 0, 0]);

    let mut red = vec![255, 0, 0, 255];
    super::apply(
      &mut red,
      1,
      1,
      &super::resolve(&super::parse("grayscale(1)").unwrap(), 10.0, 1.0, 1.0),
    );
    assert_eq!(red[0], red[1]);
    assert_eq!(red[1], red[2]);
    assert_eq!(red[3], 255);
    assert!((i32::from(red[0]) - 54).abs() <= 1);

    let mut red = vec![255, 0, 0, 255, 0, 0, 0, 0];
    super::apply(
      &mut red,
      2,
      1,
      &super::resolve(&super::parse("blur(1px)").unwrap(), 10.0, 2.0, 1.0),
    );
    assert!(red[7] > 0, "blur reaches the neighbor");
    assert!(red[3] > 0, "source stays covered");
  }
}
