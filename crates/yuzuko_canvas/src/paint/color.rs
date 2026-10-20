//! CSS colors accepted by Canvas 2D styles, including `color()`.

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

  pub fn to_srgb_peniko(self) -> vello::peniko::Color {
    let encoded = if self.syntax == ColorSyntax::DisplayP3 {
      convert_encoded(
        ColorSpace::DisplayP3,
        ColorSpace::Srgb,
        [f64::from(self.r), f64::from(self.g), f64::from(self.b)],
      )
    } else {
      [f64::from(self.r), f64::from(self.g), f64::from(self.b)]
    };
    vello::peniko::Color::new([
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
  if let Some(hex) = text.strip_prefix('#') {
    return parse_hex(hex);
  }
  let lower = text.to_ascii_lowercase();
  if let Some(rgb) = named(&lower) {
    return Some(rgb);
  }
  parse_function(&lower)
}

fn parse_hex(hex: &str) -> Option<CssColor> {
  let byte = |slice: &str| u8::from_str_radix(slice, 16).ok();
  let expand = |digit: u8| digit * 17;
  let (r, g, b, a) = match hex.len() {
    3 | 4 => {
      let chars: Vec<char> = hex.chars().collect();
      let channel = |index: usize| {
        chars
          .get(index)?
          .to_digit(16)
          .map(|digit| expand(digit as u8))
      };
      let alpha = if hex.len() == 4 { channel(3)? } else { 255 };
      (channel(0)?, channel(1)?, channel(2)?, alpha)
    }
    6 | 8 => {
      let alpha = if hex.len() == 8 {
        byte(&hex[6..8])?
      } else {
        255
      };
      (
        byte(&hex[0..2])?,
        byte(&hex[2..4])?,
        byte(&hex[4..6])?,
        alpha,
      )
    }
    _ => return None,
  };
  Some(from_bytes(r, g, b, a))
}

fn from_bytes(r: u8, g: u8, b: u8, a: u8) -> CssColor {
  CssColor {
    r: f32::from(r) / 255.0,
    g: f32::from(g) / 255.0,
    b: f32::from(b) / 255.0,
    a: f32::from(a) / 255.0,
    syntax: ColorSyntax::Legacy,
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

fn parse_function(text: &str) -> Option<CssColor> {
  let (name, body) = text.split_once('(')?;
  let body = body.strip_suffix(')')?.trim();
  match name.trim() {
    "rgb" | "rgba" => parse_rgb(body, false),
    "hsl" | "hsla" => parse_hsl(body),
    "color" => parse_color_function(body),
    _ => None,
  }
}

fn parse_rgb(body: &str, unit: bool) -> Option<CssColor> {
  let (channels, alpha) = split_alpha(body)?;
  let parts = split_channels(channels)?;
  if parts.len() != 3 {
    return None;
  }
  let r = component(parts[0], unit)?;
  let g = component(parts[1], unit)?;
  let b = component(parts[2], unit)?;
  let a = alpha.unwrap_or(1.0);
  if ![r, g, b, a].iter().all(|value| value.is_finite()) {
    return None;
  }
  Some(CssColor {
    r: r as f32,
    g: g as f32,
    b: b as f32,
    a: a as f32,
    syntax: ColorSyntax::Legacy,
  })
}

fn parse_hsl(body: &str) -> Option<CssColor> {
  let (channels, alpha) = split_alpha(body)?;
  let parts = split_channels(channels)?;
  if parts.len() != 3 {
    return None;
  }
  let h = hue(parts[0])?;
  let s = percent(parts[1])?;
  let l = percent(parts[2])?;
  let a = alpha.unwrap_or(1.0);
  let (r, g, b) = hsl_to_rgb(h, s, l);
  Some(CssColor {
    r,
    g,
    b,
    a: a as f32,
    syntax: ColorSyntax::Legacy,
  })
}

fn parse_color_function(body: &str) -> Option<CssColor> {
  let mut parts = body.split_whitespace();
  let space = parts.next()?;
  let rest: Vec<&str> = parts.collect();
  let joined = rest.join(" ");
  let p3 = match space {
    "srgb" => false,
    "display-p3" => true,
    _ => return None,
  };
  let mut color = parse_rgb(&joined, true)?;
  color.syntax = if p3 {
    ColorSyntax::DisplayP3
  } else {
    ColorSyntax::Srgb
  };
  Some(color)
}

fn split_alpha(body: &str) -> Option<(&str, Option<f64>)> {
  if let Some((channels, alpha)) = body.split_once('/') {
    return Some((channels.trim(), Some(alpha_component(alpha.trim())?)));
  }
  let comma: Vec<&str> = body.split(',').map(str::trim).collect();
  if comma.len() == 4 {
    let alpha = alpha_component(comma[3])?;
    return Some((&body[..body.rfind(',')?], Some(alpha)));
  }
  if comma.len() == 3 || !body.contains(',') {
    return Some((body, None));
  }
  None
}

fn split_channels(body: &str) -> Option<Vec<&str>> {
  let body = body.trim().trim_end_matches(',').trim();
  if body.contains(',') {
    let parts: Vec<&str> = body
      .split(',')
      .map(str::trim)
      .filter(|part| !part.is_empty())
      .collect();
    return Some(parts);
  }
  Some(body.split_whitespace().collect())
}

fn component(text: &str, unit: bool) -> Option<f64> {
  if let Some(percent) = text.strip_suffix('%') {
    return percent
      .trim()
      .parse::<f64>()
      .ok()
      .map(|value| value / 100.0);
  }
  let value: f64 = text.parse().ok()?;
  if unit {
    Some(value)
  } else {
    Some(value / 255.0)
  }
}

fn percent(text: &str) -> Option<f64> {
  let text = text.strip_suffix('%').unwrap_or(text).trim();
  text.parse::<f64>().ok().map(|value| value / 100.0)
}

fn alpha_component(text: &str) -> Option<f64> {
  if let Some(percent) = text.strip_suffix('%') {
    return percent
      .trim()
      .parse::<f64>()
      .ok()
      .map(|value| value / 100.0);
  }
  text.parse().ok()
}

fn hue(text: &str) -> Option<f64> {
  let (number, turn) = if let Some(rest) = text.strip_suffix("deg") {
    (rest.trim(), 1.0)
  } else if let Some(rest) = text.strip_suffix("turn") {
    (rest.trim(), 360.0)
  } else if let Some(rest) = text.strip_suffix("rad") {
    (rest.trim(), 180.0 / std::f64::consts::PI)
  } else {
    (text, 1.0)
  };
  let value: f64 = number.parse().ok()?;
  Some(value * turn)
}

fn hsl_to_rgb(h: f64, s: f64, l: f64) -> (f32, f32, f32) {
  let h = h.rem_euclid(360.0);
  let s = s.clamp(0.0, 1.0);
  let l = l.clamp(0.0, 1.0);
  let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
  let h = h / 60.0;
  let x = c * (1.0 - (h.rem_euclid(2.0) - 1.0).abs());
  let (r, g, b) = match h as i32 {
    0 => (c, x, 0.0),
    1 => (x, c, 0.0),
    2 => (0.0, c, x),
    3 => (0.0, x, c),
    4 => (x, 0.0, c),
    _ => (c, 0.0, x),
  };
  let m = l - c / 2.0;
  ((r + m) as f32, (g + m) as f32, (b + m) as f32)
}

fn named(name: &str) -> Option<CssColor> {
  if name == "transparent" {
    return Some(CssColor {
      r: 0.0,
      g: 0.0,
      b: 0.0,
      a: 0.0,
      syntax: ColorSyntax::Legacy,
    });
  }
  if name == "currentcolor" {
    return Some(CssColor::BLACK);
  }
  let (r, g, b) = match name {
    "black" => (0, 0, 0),
    "silver" => (192, 192, 192),
    "gray" | "grey" => (128, 128, 128),
    "white" => (255, 255, 255),
    "maroon" => (128, 0, 0),
    "red" => (255, 0, 0),
    "purple" => (128, 0, 128),
    "fuchsia" | "magenta" => (255, 0, 255),
    "green" => (0, 128, 0),
    "lime" => (0, 255, 0),
    "olive" => (128, 128, 0),
    "yellow" => (255, 255, 0),
    "navy" => (0, 0, 128),
    "blue" => (0, 0, 255),
    "teal" => (0, 128, 128),
    "aqua" | "cyan" => (0, 255, 255),
    "aliceblue" => (240, 248, 255),
    "antiquewhite" => (250, 235, 215),
    "aquamarine" => (127, 255, 212),
    "azure" => (240, 255, 255),
    "beige" => (245, 245, 220),
    "bisque" => (255, 228, 196),
    "blanchedalmond" => (255, 235, 205),
    "blueviolet" => (138, 43, 226),
    "brown" => (165, 42, 42),
    "burlywood" => (222, 184, 135),
    "cadetblue" => (95, 158, 160),
    "chartreuse" => (127, 255, 0),
    "chocolate" => (210, 105, 30),
    "coral" => (255, 127, 80),
    "cornflowerblue" => (100, 149, 237),
    "cornsilk" => (255, 248, 220),
    "crimson" => (220, 20, 60),
    "darkblue" => (0, 0, 139),
    "darkcyan" => (0, 139, 139),
    "darkgoldenrod" => (184, 134, 11),
    "darkgray" | "darkgrey" => (169, 169, 169),
    "darkgreen" => (0, 100, 0),
    "darkkhaki" => (189, 183, 107),
    "darkmagenta" => (139, 0, 139),
    "darkolivegreen" => (85, 107, 47),
    "darkorange" => (255, 140, 0),
    "darkorchid" => (153, 50, 204),
    "darkred" => (139, 0, 0),
    "darksalmon" => (233, 150, 122),
    "darkseagreen" => (143, 188, 143),
    "darkslateblue" => (72, 61, 139),
    "darkslategray" | "darkslategrey" => (47, 79, 79),
    "darkturquoise" => (0, 206, 209),
    "darkviolet" => (148, 0, 211),
    "deeppink" => (255, 20, 147),
    "deepskyblue" => (0, 191, 255),
    "dimgray" | "dimgrey" => (105, 105, 105),
    "dodgerblue" => (30, 144, 255),
    "firebrick" => (178, 34, 34),
    "floralwhite" => (255, 250, 240),
    "forestgreen" => (34, 139, 34),
    "gainsboro" => (220, 220, 220),
    "ghostwhite" => (248, 248, 255),
    "gold" => (255, 215, 0),
    "goldenrod" => (218, 165, 32),
    "greenyellow" => (173, 255, 47),
    "honeydew" => (240, 255, 240),
    "hotpink" => (255, 105, 180),
    "indianred" => (205, 92, 92),
    "indigo" => (75, 0, 130),
    "ivory" => (255, 255, 240),
    "khaki" => (240, 230, 140),
    "lavender" => (230, 230, 250),
    "lavenderblush" => (255, 240, 245),
    "lawngreen" => (124, 252, 0),
    "lemonchiffon" => (255, 250, 205),
    "lightblue" => (173, 216, 230),
    "lightcoral" => (240, 128, 128),
    "lightcyan" => (224, 255, 255),
    "lightgoldenrodyellow" => (250, 250, 210),
    "lightgray" | "lightgrey" => (211, 211, 211),
    "lightgreen" => (144, 238, 144),
    "lightpink" => (255, 182, 193),
    "lightsalmon" => (255, 160, 122),
    "lightseagreen" => (32, 178, 170),
    "lightskyblue" => (135, 206, 250),
    "lightslategray" | "lightslategrey" => (119, 136, 153),
    "lightsteelblue" => (176, 196, 222),
    "lightyellow" => (255, 255, 224),
    "limegreen" => (50, 205, 50),
    "linen" => (250, 240, 230),
    "mediumaquamarine" => (102, 205, 170),
    "mediumblue" => (0, 0, 205),
    "mediumorchid" => (186, 85, 211),
    "mediumpurple" => (147, 112, 219),
    "mediumseagreen" => (60, 179, 113),
    "mediumslateblue" => (123, 104, 238),
    "mediumspringgreen" => (0, 250, 154),
    "mediumturquoise" => (72, 209, 204),
    "mediumvioletred" => (199, 21, 133),
    "midnightblue" => (25, 25, 112),
    "mintcream" => (245, 255, 250),
    "mistyrose" => (255, 228, 225),
    "moccasin" => (255, 228, 181),
    "navajowhite" => (255, 222, 173),
    "oldlace" => (253, 245, 230),
    "olivedrab" => (107, 142, 35),
    "orange" => (255, 165, 0),
    "orangered" => (255, 69, 0),
    "orchid" => (218, 112, 214),
    "palegoldenrod" => (238, 232, 170),
    "palegreen" => (152, 251, 152),
    "paleturquoise" => (175, 238, 238),
    "palevioletred" => (219, 112, 147),
    "papayawhip" => (255, 239, 213),
    "peachpuff" => (255, 218, 185),
    "peru" => (205, 133, 63),
    "pink" => (255, 192, 203),
    "plum" => (221, 160, 221),
    "powderblue" => (176, 224, 230),
    "rebeccapurple" => (102, 51, 153),
    "rosybrown" => (188, 143, 143),
    "royalblue" => (65, 105, 225),
    "saddlebrown" => (139, 69, 19),
    "salmon" => (250, 128, 114),
    "sandybrown" => (244, 164, 96),
    "seagreen" => (46, 139, 87),
    "seashell" => (255, 245, 238),
    "sienna" => (160, 82, 45),
    "skyblue" => (135, 206, 235),
    "slateblue" => (106, 90, 205),
    "slategray" | "slategrey" => (112, 128, 144),
    "snow" => (255, 250, 250),
    "springgreen" => (0, 255, 127),
    "steelblue" => (70, 130, 180),
    "tan" => (210, 180, 140),
    "thistle" => (216, 191, 216),
    "tomato" => (255, 99, 71),
    "turquoise" => (64, 224, 208),
    "violet" => (238, 130, 238),
    "wheat" => (245, 222, 179),
    "whitesmoke" => (245, 245, 245),
    "yellowgreen" => (154, 205, 50),
    _ => return None,
  };
  Some(from_bytes(r, g, b, 255))
}
