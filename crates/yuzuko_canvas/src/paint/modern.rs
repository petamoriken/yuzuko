//! `calc()`, `color-mix()`, and relative color syntax.
//!
//! The `color` crate parses absolute colors and converts between spaces.
//! It does not parse these three forms, so this module reads them with
//! cssparser and hands the components to that crate.

use color::ColorSpaceTag;
use color::DynamicColor;
use color::Flags;
use color::HueDirection;
use color::Missing;
use cssparser::ParseError;
use cssparser::Parser;
use cssparser::Token;

use crate::paint::color::crate_color;
use crate::paint::color::current_color;

pub(crate) fn parse_tree(
  parser: &mut Parser<'_>,
) -> Result<(DynamicColor, bool), ParseError<()>> {
  let start = parser.position();
  let token = parser.next()?.clone();
  match token {
    Token::Hash(_) | Token::IDHash(_) => {
      absolute(parser.slice_from(start).trim())
    }
    Token::Ident(name) => {
      if name.eq_ignore_ascii_case("currentcolor") {
        Ok((current_color(), true))
      } else {
        absolute(parser.slice_from(start).trim())
      }
    }
    Token::Function(name) => {
      let name = name.to_ascii_lowercase();
      let mut parsed = None;
      parser.parse_nested_block(|inner| {
        // `lch()` and `oklch()` stay here. The `color` crate maps LCH
        // `100%` chroma to 125, and CSS Color 4 maps it to 150.
        if name == "color-mix"
          || name == "lch"
          || name == "oklch"
          || body_is_custom(inner)
        {
          parsed = Some(parse_custom(&name, inner)?);
          Ok(())
        } else {
          skip_rest(inner)
        }
      })?;
      if let Some(parsed) = parsed {
        Ok(parsed)
      } else {
        absolute(parser.slice_from(start).trim())
      }
    }
    _ => Err(ParseError::unexpected_token()),
  }
}

fn absolute(text: &str) -> Result<(DynamicColor, bool), ParseError<()>> {
  crate_color(text).ok_or(ParseError::unexpected_token())
}

/// `from` starts a relative color. `calc()` is evaluated here.
fn body_is_custom(parser: &mut Parser<'_>) -> bool {
  let mut custom = false;
  parser
    .try_parse(|parser| {
      let from = parser
        .try_parse(|inner| inner.expect_ident_matching("from"))
        .is_ok();
      custom = from || walk_has_calc(parser).unwrap_or(false);
      Err::<(), ParseError<()>>(ParseError::unexpected_token())
    })
    .ok();
  custom
}

fn walk_has_calc(parser: &mut Parser<'_>) -> Result<bool, ParseError<()>> {
  let mut found = false;
  while let Ok(token) = parser.next() {
    match token.clone() {
      Token::Function(name) if name.eq_ignore_ascii_case("calc") => {
        found = true;
        parser.parse_nested_block(skip_rest)?;
      }
      Token::Function(_) | Token::ParenthesisBlock
        if parser.parse_nested_block(walk_has_calc)? =>
      {
        found = true;
      }
      _ => {}
    }
  }
  Ok(found)
}

fn skip_rest(parser: &mut Parser<'_>) -> Result<(), ParseError<()>> {
  while parser.next().is_ok() {}
  Ok(())
}

fn parse_custom(
  name: &str,
  parser: &mut Parser<'_>,
) -> Result<(DynamicColor, bool), ParseError<()>> {
  if name == "color-mix" {
    return parse_mix(parser);
  }
  if parser
    .try_parse(|inner| inner.expect_ident_matching("from"))
    .is_ok()
  {
    return parse_relative(name, parser);
  }
  parse_with_calc(name, parser)
}

#[derive(Clone, Copy)]
struct Axis {
  /// Number units of `100%`. A hue rejects a percentage.
  percent: Option<f64>,
  /// Multiply a stored component to get a bare number. `rgb()` channels use 255.
  scale: f64,
  clamp: Clamp,
}

#[derive(Clone, Copy)]
enum Clamp {
  None,
  Unit,
  Min0,
  ZeroTo(f64),
}

#[derive(Clone, Copy)]
struct Model {
  tag: ColorSpaceTag,
  legacy: bool,
  commas: bool,
  names: [&'static str; 4],
  axes: [Axis; 4],
}

fn alpha_axis() -> Axis {
  Axis {
    percent: Some(1.0),
    scale: 1.0,
    clamp: Clamp::Unit,
  }
}

fn hue_axis() -> Axis {
  Axis {
    percent: None,
    scale: 1.0,
    clamp: Clamp::None,
  }
}

fn rgb_model() -> Model {
  let channel = Axis {
    percent: Some(255.0),
    scale: 255.0,
    clamp: Clamp::Unit,
  };
  Model {
    tag: ColorSpaceTag::Srgb,
    legacy: true,
    commas: true,
    names: ["r", "g", "b", "alpha"],
    axes: [channel, channel, channel, alpha_axis()],
  }
}

fn color_model(tag: ColorSpaceTag) -> Model {
  let channel = Axis {
    percent: Some(1.0),
    scale: 1.0,
    clamp: Clamp::None,
  };
  let xyz = matches!(tag, ColorSpaceTag::XyzD50 | ColorSpaceTag::XyzD65);
  Model {
    tag,
    legacy: false,
    commas: false,
    names: if xyz {
      ["x", "y", "z", "alpha"]
    } else {
      ["r", "g", "b", "alpha"]
    },
    axes: [channel, channel, channel, alpha_axis()],
  }
}

fn hsl_model() -> Model {
  Model {
    tag: ColorSpaceTag::Hsl,
    legacy: true,
    commas: true,
    names: ["h", "s", "l", "alpha"],
    axes: [
      hue_axis(),
      Axis {
        percent: Some(100.0),
        scale: 1.0,
        clamp: Clamp::Min0,
      },
      Axis {
        percent: Some(100.0),
        scale: 1.0,
        clamp: Clamp::None,
      },
      alpha_axis(),
    ],
  }
}

fn hwb_model() -> Model {
  Model {
    tag: ColorSpaceTag::Hwb,
    legacy: false,
    commas: false,
    names: ["h", "w", "b", "alpha"],
    axes: [
      hue_axis(),
      Axis {
        percent: Some(100.0),
        scale: 1.0,
        clamp: Clamp::None,
      },
      Axis {
        percent: Some(100.0),
        scale: 1.0,
        clamp: Clamp::None,
      },
      alpha_axis(),
    ],
  }
}

fn lab_model(ok: bool) -> Model {
  let (tag, lightness, axis) = if ok {
    (ColorSpaceTag::Oklab, 1.0, 0.4)
  } else {
    (ColorSpaceTag::Lab, 100.0, 125.0)
  };
  Model {
    tag,
    legacy: false,
    commas: false,
    names: ["l", "a", "b", "alpha"],
    axes: [
      Axis {
        percent: Some(lightness),
        scale: 1.0,
        clamp: Clamp::ZeroTo(lightness),
      },
      Axis {
        percent: Some(axis),
        scale: 1.0,
        clamp: Clamp::None,
      },
      Axis {
        percent: Some(axis),
        scale: 1.0,
        clamp: Clamp::None,
      },
      alpha_axis(),
    ],
  }
}

fn lch_model(ok: bool) -> Model {
  // CSS Color 4 maps `100%` LCH chroma to 150. Oklch `100%` chroma is 0.4.
  let (tag, lightness, chroma) = if ok {
    (ColorSpaceTag::Oklch, 1.0, 0.4)
  } else {
    (ColorSpaceTag::Lch, 100.0, 150.0)
  };
  Model {
    tag,
    legacy: false,
    commas: false,
    names: ["l", "c", "h", "alpha"],
    axes: [
      Axis {
        percent: Some(lightness),
        scale: 1.0,
        clamp: Clamp::ZeroTo(lightness),
      },
      Axis {
        percent: Some(chroma),
        scale: 1.0,
        clamp: Clamp::Min0,
      },
      hue_axis(),
      alpha_axis(),
    ],
  }
}

fn model_for(name: &str) -> Result<Model, ParseError<()>> {
  Ok(match name {
    "rgb" | "rgba" => rgb_model(),
    "hsl" | "hsla" => hsl_model(),
    "hwb" => hwb_model(),
    "lab" => lab_model(false),
    "oklab" => lab_model(true),
    "lch" => lch_model(false),
    "oklch" => lch_model(true),
    _ => return Err(ParseError::unexpected_token()),
  })
}

fn color_tag(name: &str) -> Result<ColorSpaceTag, ParseError<()>> {
  match name {
    "srgb" => Ok(ColorSpaceTag::Srgb),
    "srgb-linear" => Ok(ColorSpaceTag::LinearSrgb),
    "display-p3" => Ok(ColorSpaceTag::DisplayP3),
    "xyz" | "xyz-d65" => Ok(ColorSpaceTag::XyzD65),
    "xyz-d50" => Ok(ColorSpaceTag::XyzD50),
    _ => Err(ParseError::unexpected_token()),
  }
}

fn parse_with_calc(
  name: &str,
  parser: &mut Parser<'_>,
) -> Result<(DynamicColor, bool), ParseError<()>> {
  let model = if name == "color" {
    let ident = parser.expect_ident_cloned()?;
    color_model(color_tag(&ident)?)
  } else {
    model_for(name)?
  };
  let (channels, alpha) = read_channels(parser, &model, None)?;
  Ok((finish(&model, channels, alpha)?, model.legacy))
}

fn parse_relative(
  name: &str,
  parser: &mut Parser<'_>,
) -> Result<(DynamicColor, bool), ParseError<()>> {
  let (origin, _) = parse_tree(parser)?;
  let mut model = if name == "color" {
    let ident = parser.expect_ident_cloned()?;
    color_model(color_tag(&ident)?)
  } else {
    model_for(name)?
  };
  // Relative syntax is the modern form. Legacy commas stay on absolute rgb and hsl.
  model.commas = false;
  let units = number_units(&model, origin.convert(model.tag));
  let (channels, alpha) = read_channels(parser, &model, Some(units))?;
  Ok((finish(&model, channels, alpha)?, false))
}

fn number_units(model: &Model, color: DynamicColor) -> [f64; 4] {
  let mut units = [0.0; 4];
  for (index, axis) in model.axes.iter().enumerate() {
    if !color.flags.missing().contains(index) {
      units[index] = f64::from(color.components[index]) * axis.scale;
    }
  }
  units
}

enum ParsedAlpha {
  Omitted,
  Missing,
  Value(f64),
}

fn read_channels(
  parser: &mut Parser<'_>,
  model: &Model,
  origin: Option<[f64; 4]>,
) -> Result<([Option<f64>; 3], ParsedAlpha), ParseError<()>> {
  let origin = origin.as_ref();
  let mut values = [None, None, None];
  let mut commas = false;
  values[0] = component(parser, &model.axes[0], &model.names, origin)?;
  let mut count = 1;
  loop {
    if parser.try_parse(Parser::expect_comma).is_ok() {
      if !model.commas || (!commas && count > 1) {
        return Err(ParseError::unexpected_token());
      }
      commas = true;
      if count == 3 {
        let alpha = read_alpha(parser, model, origin)?;
        parser.expect_exhausted()?;
        return Ok((values, alpha));
      }
      values[count] =
        component(parser, &model.axes[count], &model.names, origin)?;
      count += 1;
      continue;
    }
    if parser.try_parse(|inner| inner.expect_delim('/')).is_ok() {
      if commas || count != 3 {
        return Err(ParseError::unexpected_token());
      }
      let alpha = read_alpha(parser, model, origin)?;
      parser.expect_exhausted()?;
      return Ok((values, alpha));
    }
    if parser.is_exhausted() {
      break;
    }
    if commas || count == 3 {
      return Err(ParseError::unexpected_token());
    }
    values[count] =
      component(parser, &model.axes[count], &model.names, origin)?;
    count += 1;
  }
  if count == 3 {
    Ok((values, ParsedAlpha::Omitted))
  } else {
    Err(ParseError::unexpected_token())
  }
}

fn read_alpha(
  parser: &mut Parser<'_>,
  model: &Model,
  origin: Option<&[f64; 4]>,
) -> Result<ParsedAlpha, ParseError<()>> {
  match component(parser, &model.axes[3], &model.names, origin)? {
    None => Ok(ParsedAlpha::Missing),
    Some(value) => Ok(ParsedAlpha::Value(value)),
  }
}

fn component(
  parser: &mut Parser<'_>,
  axis: &Axis,
  names: &[&str; 4],
  origin: Option<&[f64; 4]>,
) -> Result<Option<f64>, ParseError<()>> {
  match factor(parser, axis, names, origin, true)? {
    Term::Missing => Ok(None),
    Term::Number(number) => Ok(Some(store_channel(number, axis)?)),
  }
}

fn store_channel(number: f64, axis: &Axis) -> Result<f64, ParseError<()>> {
  if !number.is_finite() {
    return Err(ParseError::unexpected_token());
  }
  let value = number / axis.scale;
  let value = match axis.clamp {
    Clamp::None => value,
    Clamp::Unit => value.clamp(0.0, 1.0),
    Clamp::Min0 => value.max(0.0),
    Clamp::ZeroTo(max) => value.clamp(0.0, max),
  };
  if value.is_finite() {
    Ok(value)
  } else {
    Err(ParseError::unexpected_token())
  }
}

fn finish(
  model: &Model,
  channels: [Option<f64>; 3],
  alpha: ParsedAlpha,
) -> Result<DynamicColor, ParseError<()>> {
  let mut missing = Missing::default();
  let mut components = [0.0; 4];
  for (index, channel) in channels.iter().enumerate() {
    match channel {
      None => missing.insert(index),
      Some(value) => components[index] = *value as f32,
    }
  }
  match alpha {
    ParsedAlpha::Omitted => components[3] = 1.0,
    ParsedAlpha::Missing => missing.insert(3),
    ParsedAlpha::Value(value) => components[3] = value as f32,
  }
  if !components.iter().all(|channel| channel.is_finite()) {
    return Err(ParseError::unexpected_token());
  }
  Ok(DynamicColor {
    cs: model.tag,
    flags: Flags::from_missing(missing),
    components,
  })
}

fn parse_mix(
  parser: &mut Parser<'_>,
) -> Result<(DynamicColor, bool), ParseError<()>> {
  parser.expect_ident_matching("in")?;
  let name = parser.expect_ident_cloned()?;
  let tag = mix_tag(&name)?;
  let direction = mix_direction(parser, tag)?;
  parser.expect_comma()?;
  let (left, left_weight) = mix_stop(parser)?;
  parser.expect_comma()?;
  let (right, right_weight) = mix_stop(parser)?;
  parser.expect_exhausted()?;
  let (weight_a, weight_b) = match (left_weight, right_weight) {
    (None, None) => (0.5, 0.5),
    (Some(left), None) => (left, 1.0 - left),
    (None, Some(right)) => (1.0 - right, right),
    (Some(left), Some(right)) => (left, right),
  };
  if !weight_a.is_finite()
    || !weight_b.is_finite()
    || weight_a + weight_b == 0.0
  {
    return Err(ParseError::unexpected_token());
  }
  let toward_right = (weight_b / (weight_a + weight_b)) as f32;
  let mixed = left
    .interpolate(right, tag, direction)
    .eval(toward_right)
    .convert(ColorSpaceTag::Srgb);
  if !mixed.components.iter().all(|channel| channel.is_finite()) {
    return Err(ParseError::unexpected_token());
  }
  Ok((
    DynamicColor {
      cs: ColorSpaceTag::Srgb,
      flags: Flags::default(),
      components: mixed.components,
    },
    false,
  ))
}

fn mix_tag(name: &str) -> Result<ColorSpaceTag, ParseError<()>> {
  match name {
    "srgb" => Ok(ColorSpaceTag::Srgb),
    "srgb-linear" => Ok(ColorSpaceTag::LinearSrgb),
    "display-p3" => Ok(ColorSpaceTag::DisplayP3),
    "lab" => Ok(ColorSpaceTag::Lab),
    "oklab" => Ok(ColorSpaceTag::Oklab),
    "lch" => Ok(ColorSpaceTag::Lch),
    "oklch" => Ok(ColorSpaceTag::Oklch),
    "hsl" => Ok(ColorSpaceTag::Hsl),
    "hwb" => Ok(ColorSpaceTag::Hwb),
    "xyz" | "xyz-d65" => Ok(ColorSpaceTag::XyzD65),
    "xyz-d50" => Ok(ColorSpaceTag::XyzD50),
    _ => Err(ParseError::unexpected_token()),
  }
}

fn mix_direction(
  parser: &mut Parser<'_>,
  tag: ColorSpaceTag,
) -> Result<HueDirection, ParseError<()>> {
  let polar = matches!(
    tag,
    ColorSpaceTag::Lch
      | ColorSpaceTag::Oklch
      | ColorSpaceTag::Hsl
      | ColorSpaceTag::Hwb
  );
  let state = parser.state();
  let Ok(direction) = parser.try_parse(Parser::expect_ident_cloned) else {
    return Ok(HueDirection::Shorter);
  };
  if parser
    .try_parse(|inner| inner.expect_ident_matching("hue"))
    .is_err()
  {
    parser.reset(&state);
    return Ok(HueDirection::Shorter);
  }
  if !polar {
    return Err(ParseError::unexpected_token());
  }
  match &*direction {
    "shorter" => Ok(HueDirection::Shorter),
    "longer" => Ok(HueDirection::Longer),
    "increasing" => Ok(HueDirection::Increasing),
    "decreasing" => Ok(HueDirection::Decreasing),
    _ => Err(ParseError::unexpected_token()),
  }
}

fn mix_stop(
  parser: &mut Parser<'_>,
) -> Result<(DynamicColor, Option<f64>), ParseError<()>> {
  let (color, _) = parse_tree(parser)?;
  if parser.is_exhausted() {
    return Ok((color, None));
  }
  let state = parser.state();
  let token = parser.next()?.clone();
  match token {
    Token::Comma => {
      parser.reset(&state);
      Ok((color, None))
    }
    Token::Percentage { .. } | Token::ParenthesisBlock => {
      parser.reset(&state);
      Ok((color, Some(weight(parser)?)))
    }
    Token::Function(name) if name.eq_ignore_ascii_case("calc") => {
      parser.reset(&state);
      Ok((color, Some(weight(parser)?)))
    }
    _ => Err(ParseError::unexpected_token()),
  }
}

fn weight(parser: &mut Parser<'_>) -> Result<f64, ParseError<()>> {
  let axis = Axis {
    percent: Some(1.0),
    scale: 1.0,
    clamp: Clamp::None,
  };
  let names = ["", "", "", ""];
  match factor(parser, &axis, &names, None, false)? {
    Term::Number(value) if value.is_finite() => Ok(value),
    _ => Err(ParseError::unexpected_token()),
  }
}

enum Term {
  Missing,
  Number(f64),
}

/// `+` and `-` are operators when cssparser emits them as delimiters.
/// A sign written against a digit is part of that number.
fn expr(
  parser: &mut Parser<'_>,
  axis: &Axis,
  names: &[&str; 4],
  origin: Option<&[f64; 4]>,
) -> Result<f64, ParseError<()>> {
  let mut value = term(parser, axis, names, origin)?;
  loop {
    if parser.try_parse(|inner| inner.expect_delim('+')).is_ok() {
      value += term(parser, axis, names, origin)?;
    } else if parser.try_parse(|inner| inner.expect_delim('-')).is_ok() {
      value -= term(parser, axis, names, origin)?;
    } else {
      break;
    }
  }
  Ok(value)
}

fn term(
  parser: &mut Parser<'_>,
  axis: &Axis,
  names: &[&str; 4],
  origin: Option<&[f64; 4]>,
) -> Result<f64, ParseError<()>> {
  let mut value = signed(parser, axis, names, origin)?;
  loop {
    if parser.try_parse(|inner| inner.expect_delim('*')).is_ok() {
      value *= signed(parser, axis, names, origin)?;
    } else if parser.try_parse(|inner| inner.expect_delim('/')).is_ok() {
      let divisor = signed(parser, axis, names, origin)?;
      if divisor == 0.0 || !divisor.is_finite() {
        return Err(ParseError::unexpected_token());
      }
      value /= divisor;
    } else {
      break;
    }
  }
  Ok(value)
}

/// A sign separated from its number is math, so it applies inside `calc()`
/// and parentheses. A color component takes the sign only as part of the token.
fn signed(
  parser: &mut Parser<'_>,
  axis: &Axis,
  names: &[&str; 4],
  origin: Option<&[f64; 4]>,
) -> Result<f64, ParseError<()>> {
  if parser.try_parse(|inner| inner.expect_delim('+')).is_ok() {
    return signed(parser, axis, names, origin);
  }
  if parser.try_parse(|inner| inner.expect_delim('-')).is_ok() {
    return Ok(-signed(parser, axis, names, origin)?);
  }
  match factor(parser, axis, names, origin, false)? {
    Term::Number(value) => Ok(value),
    Term::Missing => Err(ParseError::unexpected_token()),
  }
}

/// One color component. `/` is the alpha separator here, so division stays
/// inside `calc()` and parentheses, which call [`expr`].
fn factor(
  parser: &mut Parser<'_>,
  axis: &Axis,
  names: &[&str; 4],
  origin: Option<&[f64; 4]>,
  allow_none: bool,
) -> Result<Term, ParseError<()>> {
  let start = parser.position();
  let token = parser.next()?.clone();
  match token {
    Token::Number { value, .. } => Ok(Term::Number(number_literal(
      parser,
      start,
      f64::from(value),
    ))),
    Token::Percentage { unit_value, .. } => {
      let Some(percent) = axis.percent else {
        return Err(ParseError::unexpected_token());
      };
      let number = number_literal(parser, start, f64::from(unit_value) * 100.0);
      Ok(Term::Number(number / 100.0 * percent))
    }
    Token::Dimension { value, unit, .. } => {
      if axis.percent.is_some() {
        return Err(ParseError::unexpected_token());
      }
      let number = number_literal(parser, start, f64::from(value));
      angle_degrees(number, &unit)
        .map(Term::Number)
        .ok_or(ParseError::unexpected_token())
    }
    Token::Ident(name) => {
      if allow_none && name.eq_ignore_ascii_case("none") {
        return Ok(Term::Missing);
      }
      if let Some(origin) = origin
        && let Some(index) = names
          .iter()
          .position(|channel| name.eq_ignore_ascii_case(channel))
      {
        return Ok(Term::Number(origin[index]));
      }
      Err(ParseError::unexpected_token())
    }
    Token::ParenthesisBlock => parser
      .parse_nested_block(|inner| expr(inner, axis, names, origin))
      .map(Term::Number),
    Token::Function(name) if name.eq_ignore_ascii_case("calc") => parser
      .parse_nested_block(|inner| expr(inner, axis, names, origin))
      .map(Term::Number),
    _ => Err(ParseError::unexpected_token()),
  }
}

/// cssparser stores a numeric token as `f32`. The source literal keeps the
/// `f64` digits the hue tests compare.
fn number_literal(
  parser: &Parser<'_>,
  start: cssparser::SourcePosition,
  fallback: f64,
) -> f64 {
  literal_number(parser.slice_from(start).trim_start()).unwrap_or(fallback)
}

fn literal_number(text: &str) -> Option<f64> {
  let bytes = text.as_bytes();
  let mut end = 0;
  if matches!(bytes.first(), Some(b'+' | b'-')) {
    end = 1;
  }
  let mut saw_digit = false;
  while end < bytes.len() && bytes[end].is_ascii_digit() {
    saw_digit = true;
    end += 1;
  }
  if end < bytes.len() && bytes[end] == b'.' {
    end += 1;
    while end < bytes.len() && bytes[end].is_ascii_digit() {
      saw_digit = true;
      end += 1;
    }
  }
  if !saw_digit {
    return None;
  }
  if end < bytes.len() && matches!(bytes[end], b'e' | b'E') {
    let exponent = end;
    end += 1;
    if end < bytes.len() && matches!(bytes[end], b'+' | b'-') {
      end += 1;
    }
    let mut saw_exp = false;
    while end < bytes.len() && bytes[end].is_ascii_digit() {
      saw_exp = true;
      end += 1;
    }
    if !saw_exp {
      end = exponent;
    }
  }
  text[..end].parse().ok()
}

fn angle_degrees(value: f64, unit: &str) -> Option<f64> {
  if unit.eq_ignore_ascii_case("deg") {
    Some(value)
  } else if unit.eq_ignore_ascii_case("turn") {
    Some(value * 360.0)
  } else if unit.eq_ignore_ascii_case("grad") {
    Some(value * 0.9)
  } else if unit.eq_ignore_ascii_case("rad") {
    Some(value * 180.0 / std::f64::consts::PI)
  } else {
    None
  }
}

#[cfg(test)]
mod tests {
  use crate::paint::color::parse_color;
  use crate::paint::color::serialize;

  #[test]
  fn a_percentage_is_not_a_hue() {
    assert!(parse_color("hsl(0%, 100%, 50%)").is_none());
    assert!(parse_color("hsla(0%, 100%, 50%, 1)").is_none());
    assert!(parse_color("hsl(calc(0%), 100%, 50%)").is_none());
    let green = parse_color("hsl(133.33333333grad, 100%, 50%)").unwrap();
    assert!(
      green.g > 0.99 && green.r < 0.01 && green.b < 0.01,
      "{green:?}"
    );
    let degrees = parse_color("hsl(120deg, 100%, 50%)").unwrap();
    assert!(degrees.g > 0.99 && degrees.r < 0.01, "{degrees:?}");
  }

  #[test]
  fn calc_mixes_a_percent_and_a_number() {
    let mixed = parse_color("rgb(calc(50% + 10), 0, 0)").unwrap();
    assert!((f64::from(mixed.r) - 137.5 / 255.0).abs() < 1.0e-6);
    let folded = parse_color("rgb(CALC(50% + 10), 0, 0)").unwrap();
    assert!((folded.r - mixed.r).abs() < 1.0e-6);
    // A sign written against the next digit is part of that number.
    assert!(parse_color("rgb(calc(50%+10), 0, 0)").is_none());
  }

  #[test]
  fn legacy_commas_and_modern_slashes_do_not_mix() {
    assert!(parse_color("rgb (255, 0, 0)").is_none());
    assert!(parse_color("rgb(255, 0, 0 / 1)").is_none());
    assert!(parse_color("rgb(100%, 0, 0)").is_none());
    assert!(parse_color("rgba(100%, 0, 0, 1)").is_none());
    assert!(parse_color("hsl(0, 0, 50%)").is_none());
    assert!(parse_color("hsl(0, 100%, 0)").is_none());
    assert!(parse_color("hsla(0, 0, 50%, 1)").is_none());
    assert!(parse_color("rgb(0%, 100%, 0%)").is_some());
    assert!(parse_color("rgb(100% 0 0)").is_some());
    assert!(parse_color("rgb(0, 255, 0, 20%)").is_some());
    assert!(parse_color("rgb(255, 0 0)").is_none());
    assert!(parse_color("rgb(255 0 0, 1)").is_none());
    assert!(parse_color("hsl(0, 100%, 50% / 1)").is_none());
    assert!(parse_color("rgb(255, - 1, 0)").is_none());
    assert!(parse_color("rgba(255, 0, 0, 1.)").is_none());
    // A function that reaches EOF without ')' still parses.
    let open = parse_color("rgb(0, 255, 0").unwrap();
    assert!(open.g > 0.99 && open.r < 0.01, "{open:?}");
    let negative = parse_color("rgb(-1000, 1000, -1000)").unwrap();
    assert!(negative.r < 0.01 && negative.g > 0.99 && negative.b < 0.01);
    let slash = parse_color("rgb(0 255 0 / 0.2)").unwrap();
    assert!(
      slash.g > 0.99 && (slash.a - 0.2).abs() < 1.0e-6,
      "{slash:?}"
    );
    let commas = parse_color("rgba(0, 255, 0, 20%)").unwrap();
    assert!(
      commas.g > 0.99 && (commas.a - 0.2).abs() < 1.0e-6,
      "{commas:?}"
    );
    let spaced = parse_color("hsl( -240 , 100% , 50% )").unwrap();
    assert!(spaced.g > 0.99 && spaced.r < 0.01, "{spaced:?}");
    let decimal = parse_color("hsla( 120.0 , 100.0% , 50.0% , 1 )").unwrap();
    assert!(decimal.g > 0.99 && decimal.r < 0.01, "{decimal:?}");
    assert_eq!(serialize(parse_color("grey").unwrap()), "#808080");
    assert_eq!(
      serialize(parse_color("ThreeDDarkShadow").unwrap()),
      "#767676"
    );
    assert_eq!(
      serialize(parse_color("ThreeDLightShadow").unwrap()),
      "#767676"
    );
    assert_eq!(serialize(parse_color("canvas").unwrap()), "#ffffff");
    assert_eq!(serialize(parse_color("WindowText").unwrap()), "#000000");
    assert_eq!(
      serialize(parse_color("InactiveCaptionText").unwrap()),
      "#6d6d6d"
    );
    assert_eq!(serialize(parse_color("red").unwrap()), "#ff0000");
    assert!(parse_color("button-face").is_none());
    assert_eq!(
      serialize(parse_color("lightgrey").unwrap()),
      serialize(parse_color("lightgray").unwrap())
    );
    assert_eq!(serialize(parse_color("RGB(255, 0, 0)").unwrap()), "#ff0000");
    assert_eq!(
      serialize(parse_color("color(srgb 1 0 1/0)").unwrap()),
      "color(srgb 1 0 1 / 0)"
    );
  }

  #[test]
  fn color_mix_in_srgb_serializes_like_the_canvas_test() {
    let mixed = parse_color("color-mix(in srgb, red, blue)").unwrap();
    assert_eq!(serialize(mixed), "color(srgb 0.5 0 0.5)");
    let same =
      parse_color("color-mix(in srgb, red, color(srgb 1 0 0))").unwrap();
    assert_eq!(serialize(same), "color(srgb 1 0 0)");
    let xyz = parse_color("color-mix(in xyz, red, blue)").unwrap();
    assert!(serialize(xyz).starts_with("color(srgb "));
    let xyz_d50 = parse_color("color-mix(in xyz-d50, red, blue)").unwrap();
    assert!(serialize(xyz_d50).starts_with("color(srgb "));
    assert!(parse_color("color-mix(in xyz-d65, white, black)").is_some());
    assert!(parse_color("color-mix(in xyz longer hue, red, blue)").is_none());
    assert!(
      serialize(parse_color("color(xyz 0 0 0)").unwrap())
        .starts_with("color(srgb ")
    );
    assert!(parse_color("color(xyz-d50 0 0 0)").is_some());
    assert!(parse_color("color(xyz-d65 0 0 0)").is_some());
    assert!(
      serialize(parse_color("color(from red xyz x y z)").unwrap())
        .starts_with("color(srgb ")
    );
  }

  #[test]
  fn hwb_lab_and_oklab_cover_red_and_white() {
    assert_eq!(
      serialize(parse_color("hwb(0 0% 0%)").unwrap()),
      "color(srgb 1 0 0)"
    );
    assert_eq!(
      serialize(parse_color("hwb(0 0% 100%)").unwrap()),
      "color(srgb 0 0 0)"
    );
    let white = parse_color("lab(100 0 0)").unwrap();
    assert!(
      white.r > 0.99 && white.g > 0.99 && white.b > 0.99,
      "{white:?}"
    );
    let oklab = parse_color("oklab(1 0 0)").unwrap();
    assert!(
      oklab.r > 0.99 && oklab.g > 0.99 && oklab.b > 0.99,
      "{oklab:?}"
    );
    let legacy = parse_color("rgb(255, 0, 0)").unwrap();
    assert_eq!(serialize(legacy), "#ff0000");
    let linear = parse_color("color(srgb-linear 1 0 0)").unwrap();
    assert_eq!(serialize(linear), "color(srgb 1 0 0)");
  }

  #[test]
  fn relative_color_uses_destination_channels() {
    assert_eq!(
      serialize(parse_color("rgb(from red g r b)").unwrap()),
      "color(srgb 0 1 0)"
    );
    assert_eq!(
      serialize(
        parse_color(
          "color(from color(srgb 0.25 0.5 0.75 / 0.5) srgb r g b / alpha)"
        )
        .unwrap()
      ),
      "color(srgb 0.25 0.5 0.75 / 0.5)"
    );
    assert_eq!(
      serialize(parse_color("rgb(from red calc(r / 2) g b)").unwrap()),
      "color(srgb 0.5 0 0)"
    );
    assert_eq!(
      serialize(
        parse_color("color(from color(display-p3 1 0 0) display-p3 r g b)")
          .unwrap()
      ),
      "color(display-p3 1 0 0)"
    );
    assert!(parse_color("rgb(from red x g b)").is_none());
    assert!(parse_color("rgb(from red, g, r, b)").is_none());
  }

  #[test]
  fn lch_percentage_chroma_is_150() {
    let percent = parse_color("lch(50 100% 40)").unwrap();
    let number = parse_color("lch(50 150 40)").unwrap();
    assert!(
      (percent.r - number.r).abs() < 1.0e-4,
      "{percent:?} {number:?}"
    );
    assert!((percent.g - number.g).abs() < 1.0e-4);
    assert!((percent.b - number.b).abs() < 1.0e-4);
    let calc = parse_color("lch(50 calc(100%) 40)").unwrap();
    assert!((calc.r - percent.r).abs() < 1.0e-4, "{calc:?}");
    let oklch_percent = parse_color("oklch(0.5 100% 40)").unwrap();
    let oklch_number = parse_color("oklch(0.5 0.4 40)").unwrap();
    assert!(
      (oklch_percent.r - oklch_number.r).abs() < 1.0e-4,
      "{oklch_percent:?} {oklch_number:?}"
    );
  }
}
