//! CSS colors accepted by Canvas 2D styles.
//!
//! Absolute colors go through the [`color`] crate. `calc()`, `color-mix()`,
//! and relative color syntax are parsed in `modern` and then converted with
//! the same crate.

use color::ColorSpaceTag;
use color::DynamicColor;
use color::Flags;
use cssparser::ParseError;
use cssparser::Parser;
use cssparser::Token;
use yuzuko_image::ColorSpace;
use yuzuko_image::convert_encoded;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColorSyntax {
  /// Hex, named, `rgb()`, and `hsl()` colors. Opaque values serialize as hex.
  Legacy,
  Srgb,
  DisplayP3,
}

#[derive(Clone, Copy, Debug)]
pub struct CssColor {
  pub r: f32,
  pub g: f32,
  pub b: f32,
  pub a: f32,
  pub syntax: ColorSyntax,
}

impl CssColor {
  pub const BLACK: Self = Self {
    r: 0.0,
    g: 0.0,
    b: 0.0,
    a: 1.0,
    syntax: ColorSyntax::Legacy,
  };

  pub const TRANSPARENT: Self = Self {
    r: 0.0,
    g: 0.0,
    b: 0.0,
    a: 0.0,
    syntax: ColorSyntax::Legacy,
  };

  pub fn to_srgb_peniko(self) -> vello_cpu::peniko::Color {
    let encoded = if self.syntax == ColorSyntax::DisplayP3 {
      convert_encoded(
        ColorSpace::DisplayP3,
        ColorSpace::Srgb,
        [f64::from(self.r), f64::from(self.g), f64::from(self.b)],
      )
    } else {
      [f64::from(self.r), f64::from(self.g), f64::from(self.b)]
    };
    vello_cpu::peniko::Color::new([
      encoded[0] as f32,
      encoded[1] as f32,
      encoded[2] as f32,
      self.a,
    ])
  }
}

pub fn parse_color(input: &str) -> Option<CssColor> {
  let text = input.trim();
  if text.is_empty() {
    return None;
  }
  if text.eq_ignore_ascii_case("currentcolor") {
    return Some(CssColor::BLACK);
  }
  let (color, legacy) = if needs_level5(text) {
    let lower = text.to_ascii_lowercase();
    Parser::new(&lower)
      .parse_entirely(crate::paint::modern::parse_tree)
      .ok()?
  } else {
    crate_color(text)?
  };
  css_color(color, legacy)
}

/// `color-mix()`, relative `from`, or `calc()` need the parser in `modern`.
fn needs_level5(text: &str) -> bool {
  let lower = text.to_ascii_lowercase();
  lower.contains("calc(")
    || lower.contains("color-mix(")
    || has_ident(&lower, "from")
}

fn has_ident(text: &str, word: &str) -> bool {
  text
    .split(|c: char| !c.is_ascii_alphanumeric() && c != '-' && c != '_')
    .any(|part| part == word)
}

/// Parse one absolute color with the `color` crate.
///
/// A function that reaches EOF without `)` still parses. The crate asks for
/// the closing parenthesis, so a single missing `)` is appended once.
/// The crate also rejects a space before a legacy comma and the `grey`
/// spellings, both of which CSS accepts.
pub(crate) fn crate_color(text: &str) -> Option<(DynamicColor, bool)> {
  let aliased;
  let text = if let Some(name) = named_alias(text) {
    aliased = name;
    aliased
  } else {
    text
  };
  // System colors are idents. Resolve them before the absolute parser so a
  // nested `color-mix()` stop uses the same values.
  if let Some(color) = system_color(text) {
    return Some((color, true));
  }
  let color = parse_absolute(text)?;
  // Legacy comma syntax cannot mix numbers and percentages. The crate accepts
  // that mix, and the space-separated form still can.
  if legacy_channels_mixed(text) {
    return None;
  }
  let color = supported(color)?;
  let legacy = color.flags.named()
    && matches!(color.cs, ColorSpaceTag::Srgb | ColorSpaceTag::Hsl);
  Some((color, legacy))
}

fn parse_absolute(text: &str) -> Option<DynamicColor> {
  if let Some(color) = parse_once(text) {
    return Some(color);
  }
  let tight = tighten_commas(text)?;
  parse_once(&tight)
}

fn parse_once(text: &str) -> Option<DynamicColor> {
  match color::parse_color(text) {
    Ok(color) => Some(color),
    Err(color::ParseError::ExpectedClosingParenthesis) => {
      let mut closed = String::with_capacity(text.len() + 1);
      closed.push_str(text.trim());
      closed.push(')');
      color::parse_color(&closed).ok()
    }
    Err(_) => None,
  }
}

/// `grey` and its compounds are the same colors as the `gray` spellings.
fn named_alias(text: &str) -> Option<&'static str> {
  Some(match text {
    name if name.eq_ignore_ascii_case("grey") => "gray",
    name if name.eq_ignore_ascii_case("darkgrey") => "darkgray",
    name if name.eq_ignore_ascii_case("darkslategrey") => "darkslategray",
    name if name.eq_ignore_ascii_case("dimgrey") => "dimgray",
    name if name.eq_ignore_ascii_case("lightgrey") => "lightgray",
    name if name.eq_ignore_ascii_case("lightslategrey") => "lightslategray",
    name if name.eq_ignore_ascii_case("slategrey") => "slategray",
    _ => return None,
  })
}

/// CSS system colors as fixed sRGB. There is no document and no OS theme.
///
/// Deprecated CSS2 names use the CSS Color 4 system color of the same role.
/// `ThreeDDarkShadow` and `ThreeDLightShadow` are `ButtonBorder`.
fn system_color(text: &str) -> Option<DynamicColor> {
  if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_alphabetic()) {
    return None;
  }
  let (r, g, b) = system_rgb(&text.to_ascii_lowercase())?;
  Some(DynamicColor {
    cs: ColorSpaceTag::Srgb,
    flags: Flags::default(),
    components: [
      f32::from(r) / 255.0,
      f32::from(g) / 255.0,
      f32::from(b) / 255.0,
      1.0,
    ],
  })
}

fn system_rgb(name: &str) -> Option<(u8, u8, u8)> {
  const CANVAS: (u8, u8, u8) = (0xff, 0xff, 0xff);
  const CANVAS_TEXT: (u8, u8, u8) = (0x00, 0x00, 0x00);
  const BUTTON_FACE: (u8, u8, u8) = (0xef, 0xef, 0xef);
  const BUTTON_BORDER: (u8, u8, u8) = (0x76, 0x76, 0x76);
  const GRAY_TEXT: (u8, u8, u8) = (0x6d, 0x6d, 0x6d);
  const HIGHLIGHT: (u8, u8, u8) = (0x19, 0x67, 0xd2);
  const ACCENT: (u8, u8, u8) = (0x1a, 0x73, 0xe8);
  Some(match name {
    "canvas" | "field" | "activecaption" | "appworkspace" | "background"
    | "inactivecaption" | "infobackground" | "menu" | "scrollbar"
    | "window" => CANVAS,
    "canvastext" | "buttontext" | "fieldtext" | "marktext" | "captiontext"
    | "infotext" | "menutext" | "windowtext" => CANVAS_TEXT,
    "buttonface" | "buttonhighlight" | "buttonshadow" | "threedface" => {
      BUTTON_FACE
    }
    "buttonborder" | "activeborder" | "inactiveborder" | "threeddarkshadow"
    | "threedhighlight" | "threedlightshadow" | "threedshadow"
    | "windowframe" => BUTTON_BORDER,
    "graytext" | "inactivecaptiontext" => GRAY_TEXT,
    "highlight" | "selecteditem" => HIGHLIGHT,
    "highlighttext" | "selecteditemtext" | "accentcolortext" => CANVAS,
    "linktext" => (0x00, 0x00, 0xee),
    "visitedtext" => (0x55, 0x1a, 0x8b),
    "activetext" => (0xff, 0x00, 0x00),
    "mark" => (0xff, 0xff, 0x00),
    "accentcolor" => ACCENT,
    _ => return None,
  })
}

/// Drop whitespace that sits immediately before a comma.
fn tighten_commas(text: &str) -> Option<String> {
  let bytes = text.as_bytes();
  if !bytes.contains(&b',') {
    return None;
  }
  let mut out = String::with_capacity(text.len());
  let mut changed = false;
  let mut index = 0;
  while index < bytes.len() {
    if bytes[index].is_ascii_whitespace() {
      let start = index;
      while index < bytes.len() && bytes[index].is_ascii_whitespace() {
        index += 1;
      }
      if index < bytes.len() && bytes[index] == b',' {
        changed = true;
        continue;
      }
      out.push_str(&text[start..index]);
    } else {
      let start = index;
      index += 1;
      while index < bytes.len() && !bytes[index].is_ascii_whitespace() {
        index += 1;
      }
      out.push_str(&text[start..index]);
    }
  }
  changed.then_some(out)
}

/// The comma form of `rgb()` / `rgba()` and of `hsl()` / `hsla()` keeps one
/// component type. Alpha is independent, and the space-separated form may mix.
fn legacy_channels_mixed(text: &str) -> bool {
  let mut parser = Parser::new(text);
  let Ok(token) = parser.next() else {
    return false;
  };
  let Token::Function(name) = token else {
    return false;
  };
  let hsl =
    name.eq_ignore_ascii_case("hsl") || name.eq_ignore_ascii_case("hsla");
  let rgb =
    name.eq_ignore_ascii_case("rgb") || name.eq_ignore_ascii_case("rgba");
  if !hsl && !rgb {
    return false;
  }
  parser
    .parse_nested_block(|inner| scan_legacy_mix(inner, hsl))
    .unwrap_or(false)
}

fn scan_legacy_mix(
  parser: &mut Parser<'_>,
  hsl: bool,
) -> Result<bool, ParseError<()>> {
  let mut legacy = false;
  if hsl {
    match parser.next()?.clone() {
      Token::Number { .. } | Token::Dimension { .. } => {}
      _ => return Err(ParseError::unexpected_token()),
    }
    if parser.try_parse(Parser::expect_comma).is_ok() {
      legacy = true;
    }
  }
  let mut saw_number = false;
  let mut saw_percent = false;
  let count = if hsl { 2 } else { 3 };
  for index in 0..count {
    if index > 0 {
      if parser.try_parse(Parser::expect_comma).is_ok() {
        legacy = true;
      } else if legacy {
        return Err(ParseError::unexpected_token());
      }
    }
    match parser.next()?.clone() {
      Token::Number { .. } => saw_number = true,
      Token::Percentage { .. } => saw_percent = true,
      _ => return Err(ParseError::unexpected_token()),
    }
  }
  if !parser.is_exhausted() {
    let comma = parser.try_parse(Parser::expect_comma).is_ok();
    if !comma && parser.try_parse(|inner| inner.expect_delim('/')).is_err() {
      return Err(ParseError::unexpected_token());
    }
    match parser.next()?.clone() {
      Token::Number { .. } | Token::Percentage { .. } => {}
      _ => return Err(ParseError::unexpected_token()),
    }
    parser.expect_exhausted()?;
  }
  Ok(legacy && saw_number && saw_percent)
}

pub(crate) fn css_color(color: DynamicColor, legacy: bool) -> Option<CssColor> {
  let color = supported(color)?;
  if color.cs == ColorSpaceTag::DisplayP3 {
    let [r, g, b, a] = color.components;
    if ![r, g, b, a].iter().all(|channel| channel.is_finite()) {
      return None;
    }
    return Some(CssColor {
      r,
      g,
      b,
      a,
      syntax: ColorSyntax::DisplayP3,
    });
  }
  let [r, g, b, a] = color.to_alpha_color::<color::Srgb>().components;
  if ![r, g, b, a].iter().all(|channel| channel.is_finite()) {
    return None;
  }
  Some(CssColor {
    r,
    g,
    b,
    a,
    syntax: if legacy {
      ColorSyntax::Legacy
    } else {
      ColorSyntax::Srgb
    },
  })
}

/// Spaces this runtime does not accept, including `xyz`.
fn supported(color: DynamicColor) -> Option<DynamicColor> {
  match color.cs {
    ColorSpaceTag::Srgb
    | ColorSpaceTag::LinearSrgb
    | ColorSpaceTag::Lab
    | ColorSpaceTag::Lch
    | ColorSpaceTag::Hsl
    | ColorSpaceTag::Hwb
    | ColorSpaceTag::Oklab
    | ColorSpaceTag::Oklch
    | ColorSpaceTag::DisplayP3 => Some(color),
    _ => None,
  }
}

/// An sRGB black used when a relative color or mix names `currentcolor`.
pub(crate) fn current_color() -> DynamicColor {
  DynamicColor {
    cs: ColorSpaceTag::Srgb,
    flags: Flags::default(),
    components: [0.0, 0.0, 0.0, 1.0],
  }
}

/// Canvas style getter serialization. Legacy opaque sRGB is `#rrggbb`.
pub fn serialize(color: CssColor) -> String {
  match color.syntax {
    ColorSyntax::DisplayP3 => serialize_function("display-p3", color),
    ColorSyntax::Srgb => serialize_function("srgb", color),
    ColorSyntax::Legacy if (color.a - 1.0).abs() < 0.5 / 255.0 => format!(
      "#{:02x}{:02x}{:02x}",
      byte(color.r),
      byte(color.g),
      byte(color.b)
    ),
    ColorSyntax::Legacy => format!(
      "rgba({}, {}, {}, {})",
      byte(color.r),
      byte(color.g),
      byte(color.b),
      format_number(color.a.clamp(0.0, 1.0))
    ),
  }
}

fn serialize_function(space: &str, color: CssColor) -> String {
  let channels = format!(
    "{} {} {}",
    format_number(color.r),
    format_number(color.g),
    format_number(color.b)
  );
  if (color.a - 1.0).abs() < 1.0e-6 {
    format!("color({space} {channels})")
  } else {
    format!("color({space} {channels} / {})", format_number(color.a))
  }
}

fn byte(channel: f32) -> u8 {
  (channel.clamp(0.0, 1.0) * 255.0).round() as u8
}

fn format_number(value: f32) -> String {
  let value = (f64::from(value) * 1_000_000.0).round() / 1_000_000.0;
  let mut text = format!("{value}");
  if text.contains('.') {
    while text.ends_with('0') {
      text.pop();
    }
    if text.ends_with('.') {
      text.pop();
    }
  }
  if text == "-0" { "0".to_string() } else { text }
}
