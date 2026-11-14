//! `calc()`, `hwb()`, `lab()`, `lch()`, `oklab()`, `oklch()`, and `color-mix()`.
//!
//! Matrices and the Lab transfer follow the sample code in CSS Color 4.
//! A modern color is stored as encoded sRGB and serializes as `color(srgb ...)`.

use yuzuko_image::ColorSpace;
use yuzuko_image::convert_encoded;

use crate::paint::color::ColorSyntax;
use crate::paint::color::CssColor;
use crate::paint::color::hsl_to_rgb;
use crate::paint::color::parse_color;

const KAPPA: f64 = 24389.0 / 27.0;
const EPSILON: f64 = 216.0 / 24389.0;
const D50: [f64; 3] = [0.3457 / 0.3585, 1.0, (1.0 - 0.3457 - 0.3585) / 0.3585];

const D50_TO_D65: [[f64; 3]; 3] = [
  [0.955473421488075, -0.02309845494876471, 0.06325924320057072],
  [
    -0.0283697093338637,
    1.0099953980813041,
    0.021041441191917323,
  ],
  [
    0.012314014864481998,
    -0.020507649298898964,
    1.330365926242124,
  ],
];
const D65_TO_D50: [[f64; 3]; 3] = [
  [
    1.0479297925449969,
    0.022946870601609652,
    -0.05019226628920524,
  ],
  [
    0.02962780877005599,
    0.9904344267538799,
    -0.017073799063418826,
  ],
  [
    -0.009243040646204504,
    0.015055191490298152,
    0.7518742814281371,
  ],
];
const XYZ_TO_LIN_SRGB: [[f64; 3]; 3] = [
  [12831.0 / 3959.0, -329.0 / 214.0, -1974.0 / 3959.0],
  [
    -851781.0 / 878810.0,
    1648619.0 / 878810.0,
    36519.0 / 878810.0,
  ],
  [705.0 / 12673.0, -2585.0 / 12673.0, 705.0 / 667.0],
];
const LIN_SRGB_TO_XYZ: [[f64; 3]; 3] = [
  [506752.0 / 1228815.0, 87881.0 / 245763.0, 12673.0 / 70218.0],
  [87098.0 / 409605.0, 175762.0 / 245763.0, 12673.0 / 175545.0],
  [7918.0 / 409605.0, 87881.0 / 737289.0, 1001167.0 / 1053270.0],
];
const OKLAB_TO_LMS: [[f64; 3]; 3] = [
  [1.0, 0.3963377773761749, 0.2158037573099136],
  [1.0, -0.1055613458156586, -0.0638541728258133],
  [1.0, -0.0894841775298119, -1.2914855480194092],
];
const LMS_TO_XYZ: [[f64; 3]; 3] = [
  [1.2268798758459243, -0.5578149944602171, 0.2813910456659647],
  [-0.0405757452148008, 1.112286803280317, -0.0717110580655164],
  [-0.0763729366746601, -0.4214933324022432, 1.5869240198367816],
];
const XYZ_TO_LMS: [[f64; 3]; 3] = [
  [0.819022437996703, 0.3619062600528904, -0.1288737815209879],
  [0.0329836539323885, 0.9292868615863434, 0.0361446663506424],
  [0.0481771893596242, 0.2642395317527308, 0.6335478284694309],
];
const LMS_TO_OKLAB: [[f64; 3]; 3] = [
  [0.210454268309314, 0.7936177747023054, -0.0040720430116193],
  [1.9779985324311684, -2.42859224204858, 0.450593709617411],
  [0.0259040424655478, 0.7827717124575296, -0.8086757549230774],
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Space {
  Srgb,
  SrgbLinear,
  DisplayP3,
  Lab,
  Oklab,
  Lch,
  Oklch,
  Hsl,
  Hwb,
}

impl Space {
  fn polar(self) -> bool {
    matches!(self, Self::Lch | Self::Oklch | Self::Hsl | Self::Hwb)
  }

  fn hue_index(self) -> usize {
    match self {
      Self::Hsl | Self::Hwb => 0,
      _ => 2,
    }
  }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum HueMethod {
  Shorter,
  Longer,
  Increasing,
  Decreasing,
}

/// A bare number stays in its unit. `100%` equals `percent_basis`.
pub fn eval_component(text: &str, percent_basis: f64) -> Option<f64> {
  eval(text, percent_basis, false)
}

pub fn eval_hue(text: &str) -> Option<f64> {
  eval(text, 360.0, true)
}

pub fn encode_srgb(rgb: [f64; 3]) -> [f64; 3] {
  rgb.map(encode_channel)
}

fn eval(text: &str, percent_basis: f64, hue: bool) -> Option<f64> {
  let text = text.trim();
  if text.is_empty() || !percent_basis.is_finite() {
    return None;
  }
  let mut parser = Parser { text, index: 0 };
  let value = parser.expr(percent_basis, hue)?;
  parser.skip();
  if parser.index != text.len() || !value.is_finite() {
    None
  } else {
    Some(value)
  }
}

struct Parser<'a> {
  text: &'a str,
  index: usize,
}

impl<'a> Parser<'a> {
  fn expr(&mut self, basis: f64, hue: bool) -> Option<f64> {
    let mut value = self.term(basis, hue)?;
    loop {
      self.skip();
      if self.eat(b'+') {
        value += self.term(basis, hue)?;
      } else if self.eat(b'-') {
        value -= self.term(basis, hue)?;
      } else {
        break;
      }
    }
    Some(value)
  }

  fn term(&mut self, basis: f64, hue: bool) -> Option<f64> {
    let mut value = self.factor(basis, hue)?;
    loop {
      self.skip();
      if self.eat(b'*') {
        value *= self.factor(basis, hue)?;
      } else if self.eat(b'/') {
        let divisor = self.factor(basis, hue)?;
        if divisor == 0.0 {
          return None;
        }
        value /= divisor;
      } else {
        break;
      }
    }
    Some(value)
  }

  fn factor(&mut self, basis: f64, hue: bool) -> Option<f64> {
    self.skip();
    if self.eat(b'+') {
      return self.factor(basis, hue);
    }
    if self.eat(b'-') {
      return Some(-self.factor(basis, hue)?);
    }
    if self.eat(b'(') {
      let value = self.expr(basis, hue)?;
      self.skip();
      if !self.eat(b')') {
        return None;
      }
      return Some(value);
    }
    if self.eat_word("calc") && self.eat(b'(') {
      let value = self.expr(basis, hue)?;
      self.skip();
      if !self.eat(b')') {
        return None;
      }
      return Some(value);
    }
    if self.eat_word("none") {
      return Some(0.0);
    }
    self.number(basis, hue)
  }

  fn number(&mut self, basis: f64, hue: bool) -> Option<f64> {
    let start = self.index;
    let mut saw_digit = false;
    while self.byte().is_some_and(|byte| byte.is_ascii_digit()) {
      saw_digit = true;
      self.index += 1;
    }
    if self.byte() == Some(b'.') {
      self.index += 1;
      while self.byte().is_some_and(|byte| byte.is_ascii_digit()) {
        saw_digit = true;
        self.index += 1;
      }
    }
    if !saw_digit {
      self.index = start;
      return None;
    }
    if matches!(self.byte(), Some(b'e' | b'E')) {
      let exponent = self.index;
      self.index += 1;
      if matches!(self.byte(), Some(b'+' | b'-')) {
        self.index += 1;
      }
      let mut saw_exp = false;
      while self.byte().is_some_and(|byte| byte.is_ascii_digit()) {
        saw_exp = true;
        self.index += 1;
      }
      if !saw_exp {
        self.index = exponent;
      }
    }
    let value: f64 = self.text[start..self.index].parse().ok()?;
    if self.eat(b'%') {
      let scale = if hue { 360.0 } else { basis };
      return Some(value / 100.0 * scale);
    }
    if hue {
      if self.eat_word("deg") {
        return Some(value);
      }
      if self.eat_word("turn") {
        return Some(value * 360.0);
      }
      if self.eat_word("grad") {
        return Some(value * 0.9);
      }
      if self.eat_word("rad") {
        return Some(value * 180.0 / std::f64::consts::PI);
      }
    }
    Some(value)
  }

  fn skip(&mut self) {
    while self.byte().is_some_and(|byte| byte.is_ascii_whitespace()) {
      self.index += 1;
    }
  }

  fn eat(&mut self, byte: u8) -> bool {
    if self.byte() == Some(byte) {
      self.index += 1;
      true
    } else {
      false
    }
  }

  fn eat_word(&mut self, word: &str) -> bool {
    let rest = &self.text[self.index..];
    if !rest.starts_with(word) {
      return false;
    }
    let after = rest[word.len()..].chars().next();
    if after
      .is_some_and(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
    {
      return false;
    }
    self.index += word.len();
    true
  }

  fn byte(&self) -> Option<u8> {
    self.text.as_bytes().get(self.index).copied()
  }
}

pub fn parse_modern(name: &str, body: &str) -> Option<CssColor> {
  match name {
    "hwb" => parse_hwb(body),
    "lab" => parse_lab(body, false),
    "lch" => parse_lch(body, false),
    "oklab" => parse_lab(body, true),
    "oklch" => parse_lch(body, true),
    "color-mix" => parse_mix(body),
    _ => None,
  }
}

fn parse_hwb(body: &str) -> Option<CssColor> {
  let (channels, alpha) = split_alpha(body)?;
  let parts = split_parts(channels)?;
  if parts.len() != 3 {
    return None;
  }
  let hue = eval_hue(parts[0])?;
  let white = eval_component(parts[1], 100.0)? / 100.0;
  let black = eval_component(parts[2], 100.0)? / 100.0;
  srgb_color(hwb_to_rgb(hue, white, black), alpha)
}

fn parse_lab(body: &str, oklab: bool) -> Option<CssColor> {
  let (channels, alpha) = split_alpha(body)?;
  let parts = split_parts(channels)?;
  if parts.len() != 3 {
    return None;
  }
  let (l_basis, ab_basis) = if oklab { (1.0, 0.4) } else { (100.0, 125.0) };
  let l = eval_component(parts[0], l_basis)?;
  let a = eval_component(parts[1], ab_basis)?;
  let b = eval_component(parts[2], ab_basis)?;
  let rgb = if oklab {
    oklab_to_srgb([l, a, b])
  } else {
    lab_to_srgb([l, a, b])
  };
  srgb_color(rgb, alpha)
}

fn parse_lch(body: &str, oklch: bool) -> Option<CssColor> {
  let (channels, alpha) = split_alpha(body)?;
  let parts = split_parts(channels)?;
  if parts.len() != 3 {
    return None;
  }
  let (l_basis, c_basis) = if oklch { (1.0, 0.4) } else { (100.0, 150.0) };
  let l = eval_component(parts[0], l_basis)?;
  let c = eval_component(parts[1], c_basis)?;
  let h = eval_hue(parts[2])?;
  let (a, b) = polar_to_cartesian(c, h);
  let rgb = if oklch {
    oklab_to_srgb([l, a, b])
  } else {
    lab_to_srgb([l, a, b])
  };
  srgb_color(rgb, alpha)
}

fn parse_mix(body: &str) -> Option<CssColor> {
  let rest = body.trim().strip_prefix("in")?.trim_start();
  let (preamble, rest) = split_comma(rest)?;
  let (first, second) = split_comma(rest)?;
  if split_comma(second).is_some() {
    return None;
  }
  let (space, method) = mix_preamble(preamble.trim())?;
  let (color_a, percent_a) = mix_color(first.trim())?;
  let (color_b, percent_b) = mix_color(second.trim())?;
  let (weight_a, weight_b) = match (percent_a, percent_b) {
    (None, None) => (0.5, 0.5),
    (Some(left), None) => (left, 1.0 - left),
    (None, Some(right)) => (1.0 - right, right),
    (Some(left), Some(right)) => (left, right),
  };
  if !weight_a.is_finite() || !weight_b.is_finite() {
    return None;
  }
  let sum = weight_a + weight_b;
  if sum == 0.0 {
    return None;
  }
  let toward_b = weight_b / sum;
  let (channels, alpha) = mix_in(space, method, color_a, color_b, toward_b)?;
  srgb_color(channels, Some(alpha))
}

fn mix_preamble(text: &str) -> Option<(Space, HueMethod)> {
  let words: Vec<&str> = text.split_whitespace().collect();
  let space = match words.first().copied()? {
    "srgb" => Space::Srgb,
    "srgb-linear" => Space::SrgbLinear,
    "display-p3" => Space::DisplayP3,
    "lab" => Space::Lab,
    "oklab" => Space::Oklab,
    "lch" => Space::Lch,
    "oklch" => Space::Oklch,
    "hsl" => Space::Hsl,
    "hwb" => Space::Hwb,
    _ => return None,
  };
  match words.as_slice() {
    [_] => Some((space, HueMethod::Shorter)),
    [_, direction, "hue"] if space.polar() => {
      let method = match *direction {
        "shorter" => HueMethod::Shorter,
        "longer" => HueMethod::Longer,
        "increasing" => HueMethod::Increasing,
        "decreasing" => HueMethod::Decreasing,
        _ => return None,
      };
      Some((space, method))
    }
    _ => None,
  }
}

fn mix_color(text: &str) -> Option<(CssColor, Option<f64>)> {
  if let Some((color, percent)) = trailing_percent(text) {
    let color = parse_color(color)?;
    let percent = eval_component(percent, 1.0)?;
    return Some((color, Some(percent)));
  }
  Some((parse_color(text)?, None))
}

fn trailing_percent(text: &str) -> Option<(&str, &str)> {
  let mut depth = 0i32;
  let mut split = None;
  for (index, byte) in text.bytes().enumerate() {
    match byte {
      b'(' => depth += 1,
      b')' => depth -= 1,
      b' ' | b'\t' | b'\n' | b'\r' if depth == 0 => split = Some(index),
      _ => {}
    }
  }
  let index = split?;
  let color = text[..index].trim();
  let percent = text[index..].trim();
  if color.is_empty() || percent.is_empty() {
    return None;
  }
  if percent.ends_with('%') || percent.starts_with("calc") {
    Some((color, percent))
  } else {
    None
  }
}

fn mix_in(
  space: Space,
  method: HueMethod,
  left: CssColor,
  right: CssColor,
  toward_right: f64,
) -> Option<([f64; 3], f64)> {
  let left_srgb = encoded_srgb(left);
  let right_srgb = encoded_srgb(right);
  let (left_ch, right_ch) = match space {
    Space::Srgb => (left_srgb, right_srgb),
    Space::SrgbLinear => (
      left_srgb.map(decode_channel),
      right_srgb.map(decode_channel),
    ),
    Space::DisplayP3 => (to_display_p3(left), to_display_p3(right)),
    Space::Lab => (srgb_to_lab(left_srgb), srgb_to_lab(right_srgb)),
    Space::Oklab => (srgb_to_oklab(left_srgb), srgb_to_oklab(right_srgb)),
    Space::Lch => (
      lab_to_lch(srgb_to_lab(left_srgb)),
      lab_to_lch(srgb_to_lab(right_srgb)),
    ),
    Space::Oklch => (
      oklab_to_oklch(srgb_to_oklab(left_srgb)),
      oklab_to_oklch(srgb_to_oklab(right_srgb)),
    ),
    Space::Hsl => (rgb_to_hsl(left_srgb), rgb_to_hsl(right_srgb)),
    Space::Hwb => (rgb_to_hwb(left_srgb), rgb_to_hwb(right_srgb)),
  };
  let (mixed, alpha) = if space.polar() {
    mix_polar(
      left_ch,
      left.a as f64,
      right_ch,
      right.a as f64,
      toward_right,
      space.hue_index(),
      method,
    )
  } else {
    mix_rectangular(
      left_ch,
      left.a as f64,
      right_ch,
      right.a as f64,
      toward_right,
    )
  };
  let encoded = match space {
    Space::Srgb => mixed,
    Space::SrgbLinear => encode_srgb(mixed),
    Space::DisplayP3 => {
      convert_encoded(ColorSpace::DisplayP3, ColorSpace::Srgb, mixed)
    }
    Space::Lab => lab_to_srgb(mixed),
    Space::Oklab => oklab_to_srgb(mixed),
    Space::Lch => lab_to_srgb(lch_to_lab(mixed)),
    Space::Oklch => oklab_to_srgb(oklch_to_oklab(mixed)),
    Space::Hsl => {
      let (r, g, b) = hsl_to_rgb(mixed[0], mixed[1], mixed[2]);
      [f64::from(r), f64::from(g), f64::from(b)]
    }
    Space::Hwb => hwb_to_rgb(mixed[0], mixed[1], mixed[2]),
  };
  Some((encoded, alpha))
}

fn mix_rectangular(
  left: [f64; 3],
  left_alpha: f64,
  right: [f64; 3],
  right_alpha: f64,
  toward_right: f64,
) -> ([f64; 3], f64) {
  let toward_left = 1.0 - toward_right;
  let alpha = left_alpha * toward_left + right_alpha * toward_right;
  let mixed = [0, 1, 2].map(|index| {
    left[index] * left_alpha * toward_left
      + right[index] * right_alpha * toward_right
  });
  if alpha == 0.0 {
    ([0.0; 3], 0.0)
  } else {
    (mixed.map(|channel| channel / alpha), alpha)
  }
}

fn mix_polar(
  left: [f64; 3],
  left_alpha: f64,
  right: [f64; 3],
  right_alpha: f64,
  toward_right: f64,
  hue_index: usize,
  method: HueMethod,
) -> ([f64; 3], f64) {
  let toward_left = 1.0 - toward_right;
  let alpha = left_alpha * toward_left + right_alpha * toward_right;
  let mut mixed = [0.0; 3];
  for index in 0..3 {
    if index == hue_index {
      mixed[index] = mix_hue(left[index], right[index], toward_right, method);
    } else {
      let value = left[index] * left_alpha * toward_left
        + right[index] * right_alpha * toward_right;
      mixed[index] = if alpha == 0.0 { 0.0 } else { value / alpha };
    }
  }
  (mixed, alpha)
}

fn mix_hue(left: f64, right: f64, toward_right: f64, method: HueMethod) -> f64 {
  let left = left.rem_euclid(360.0);
  let right = right.rem_euclid(360.0);
  let delta = match method {
    HueMethod::Shorter => {
      let mut delta = (right - left).rem_euclid(360.0);
      if delta > 180.0 {
        delta -= 360.0;
      }
      delta
    }
    HueMethod::Longer => {
      let mut delta = (right - left).rem_euclid(360.0);
      if delta > 0.0 && delta < 180.0 {
        delta -= 360.0;
      } else if delta == 0.0 {
        delta = 360.0;
      }
      delta
    }
    HueMethod::Increasing => (right - left).rem_euclid(360.0),
    HueMethod::Decreasing => -(left - right).rem_euclid(360.0),
  };
  (left + delta * toward_right).rem_euclid(360.0)
}

fn srgb_color(rgb: [f64; 3], alpha: Option<f64>) -> Option<CssColor> {
  let alpha = alpha.unwrap_or(1.0);
  if !rgb.iter().chain([&alpha]).all(|value| value.is_finite()) {
    return None;
  }
  Some(CssColor {
    r: rgb[0] as f32,
    g: rgb[1] as f32,
    b: rgb[2] as f32,
    a: alpha as f32,
    syntax: ColorSyntax::Srgb,
  })
}

fn encoded_srgb(color: CssColor) -> [f64; 3] {
  let rgb = [f64::from(color.r), f64::from(color.g), f64::from(color.b)];
  if color.syntax == ColorSyntax::DisplayP3 {
    convert_encoded(ColorSpace::DisplayP3, ColorSpace::Srgb, rgb)
  } else {
    rgb
  }
}

fn to_display_p3(color: CssColor) -> [f64; 3] {
  let rgb = [f64::from(color.r), f64::from(color.g), f64::from(color.b)];
  if color.syntax == ColorSyntax::DisplayP3 {
    rgb
  } else {
    convert_encoded(ColorSpace::Srgb, ColorSpace::DisplayP3, rgb)
  }
}

pub fn split_alpha(body: &str) -> Option<(&str, Option<f64>)> {
  if let Some(index) = top_byte(body, b'/') {
    let alpha = eval_component(body[index + 1..].trim(), 1.0)?;
    return Some((body[..index].trim(), Some(alpha)));
  }
  let commas = top_bytes(body, b',');
  if commas.len() == 3 {
    let alpha = eval_component(body[commas[2] + 1..].trim(), 1.0)?;
    return Some((body[..commas[2]].trim(), Some(alpha)));
  }
  if commas.len() == 2 || commas.is_empty() {
    Some((body, None))
  } else {
    None
  }
}

pub fn split_parts(body: &str) -> Option<Vec<&str>> {
  let parts = split_top(body);
  if parts.is_empty() { None } else { Some(parts) }
}

fn split_comma(body: &str) -> Option<(&str, &str)> {
  let index = top_byte(body, b',')?;
  Some((&body[..index], &body[index + 1..]))
}

fn split_top(body: &str) -> Vec<&str> {
  let mut parts = Vec::new();
  let mut start = 0;
  let mut depth = 0i32;
  for (index, byte) in body.bytes().enumerate() {
    match byte {
      b'(' => depth += 1,
      b')' => depth -= 1,
      b',' | b' ' | b'\t' | b'\n' | b'\r' if depth == 0 => {
        let part = body[start..index].trim();
        if !part.is_empty() {
          parts.push(part);
        }
        start = index + 1;
      }
      _ => {}
    }
  }
  let part = body[start..].trim();
  if !part.is_empty() {
    parts.push(part);
  }
  parts
}

fn top_byte(body: &str, wanted: u8) -> Option<usize> {
  top_bytes(body, wanted).into_iter().next()
}

fn top_bytes(body: &str, wanted: u8) -> Vec<usize> {
  let mut found = Vec::new();
  let mut depth = 0i32;
  for (index, byte) in body.bytes().enumerate() {
    match byte {
      b'(' => depth += 1,
      b')' => depth -= 1,
      byte if byte == wanted && depth == 0 => found.push(index),
      _ => {}
    }
  }
  found
}

fn hwb_to_rgb(hue: f64, white: f64, black: f64) -> [f64; 3] {
  if white + black >= 1.0 {
    let gray = if white + black == 0.0 {
      0.0
    } else {
      white / (white + black)
    };
    return [gray, gray, gray];
  }
  let (r, g, b) = hsl_to_rgb(hue, 1.0, 0.5);
  let scale = 1.0 - white - black;
  [
    f64::from(r) * scale + white,
    f64::from(g) * scale + white,
    f64::from(b) * scale + white,
  ]
}

fn rgb_to_hsl(rgb: [f64; 3]) -> [f64; 3] {
  let [r, g, b] = rgb;
  let max = r.max(g).max(b);
  let min = r.min(g).min(b);
  let lightness = (max + min) / 2.0;
  if (max - min).abs() < 1.0e-12 {
    return [0.0, 0.0, lightness];
  }
  let delta = max - min;
  let saturation = if lightness > 0.5 {
    delta / (2.0 - max - min)
  } else {
    delta / (max + min)
  };
  let hue = if max == r {
    (g - b) / delta + if g < b { 6.0 } else { 0.0 }
  } else if max == g {
    (b - r) / delta + 2.0
  } else {
    (r - g) / delta + 4.0
  };
  [hue * 60.0, saturation, lightness]
}

fn rgb_to_hwb(rgb: [f64; 3]) -> [f64; 3] {
  let hsl = rgb_to_hsl(rgb);
  [
    hsl[0],
    rgb[0].min(rgb[1]).min(rgb[2]),
    1.0 - rgb[0].max(rgb[1]).max(rgb[2]),
  ]
}

fn polar_to_cartesian(chroma: f64, hue: f64) -> (f64, f64) {
  let radians = hue.to_radians();
  (chroma * radians.cos(), chroma * radians.sin())
}

fn lab_to_lch(lab: [f64; 3]) -> [f64; 3] {
  let chroma = (lab[1] * lab[1] + lab[2] * lab[2]).sqrt();
  let mut hue = lab[2].atan2(lab[1]).to_degrees();
  if hue < 0.0 {
    hue += 360.0;
  }
  [lab[0], chroma, hue]
}

fn lch_to_lab(lch: [f64; 3]) -> [f64; 3] {
  let (a, b) = polar_to_cartesian(lch[1], lch[2]);
  [lch[0], a, b]
}

fn oklab_to_oklch(lab: [f64; 3]) -> [f64; 3] {
  lab_to_lch(lab)
}

fn oklch_to_oklab(lch: [f64; 3]) -> [f64; 3] {
  lch_to_lab(lch)
}

fn lab_to_srgb(lab: [f64; 3]) -> [f64; 3] {
  let xyz = mat_vec(D50_TO_D65, lab_to_xyz(lab));
  encode_srgb(mat_vec(XYZ_TO_LIN_SRGB, xyz))
}

fn srgb_to_lab(rgb: [f64; 3]) -> [f64; 3] {
  let xyz = mat_vec(LIN_SRGB_TO_XYZ, rgb.map(decode_channel));
  xyz_to_lab(mat_vec(D65_TO_D50, xyz))
}

fn lab_to_xyz(lab: [f64; 3]) -> [f64; 3] {
  let lightness = lab[0] / 116.0;
  let a = lab[1] / 500.0;
  let b = lab[2] / 200.0;
  let f1 = lightness + 16.0 / 116.0;
  let f0 = f1 + a;
  let f2 = f1 - b;
  let xyz = [
    if f0.powi(3) > EPSILON {
      f0.powi(3)
    } else {
      (lightness + a) * (116.0 / KAPPA)
    },
    if lab[0] > KAPPA * EPSILON {
      ((lab[0] + 16.0) / 116.0).powi(3)
    } else {
      lab[0] / KAPPA
    },
    if f2.powi(3) > EPSILON {
      f2.powi(3)
    } else {
      (lightness - b) * (116.0 / KAPPA)
    },
  ];
  [0, 1, 2].map(|index| xyz[index] * D50[index])
}

fn xyz_to_lab(xyz: [f64; 3]) -> [f64; 3] {
  let scaled = [0, 1, 2].map(|index| xyz[index] / D50[index]);
  let f = scaled.map(|value| {
    if value > EPSILON {
      value.cbrt()
    } else {
      (KAPPA * value + 16.0) / 116.0
    }
  });
  [
    (116.0 * f[1]) - 16.0,
    500.0 * (f[0] - f[1]),
    200.0 * (f[1] - f[2]),
  ]
}

fn oklab_to_srgb(lab: [f64; 3]) -> [f64; 3] {
  let lms =
    mat_vec(OKLAB_TO_LMS, lab).map(|channel| channel * channel * channel);
  encode_srgb(mat_vec(XYZ_TO_LIN_SRGB, mat_vec(LMS_TO_XYZ, lms)))
}

fn srgb_to_oklab(rgb: [f64; 3]) -> [f64; 3] {
  let lms = mat_vec(
    XYZ_TO_LMS,
    mat_vec(LIN_SRGB_TO_XYZ, rgb.map(decode_channel)),
  );
  mat_vec(LMS_TO_OKLAB, lms.map(f64::cbrt))
}

fn encode_channel(value: f64) -> f64 {
  let sign = if value < 0.0 { -1.0 } else { 1.0 };
  let abs = value.abs();
  if abs > 0.003_130_8 {
    sign * (1.055 * abs.powf(1.0 / 2.4) - 0.055)
  } else {
    12.92 * value
  }
}

fn decode_channel(value: f64) -> f64 {
  let sign = if value < 0.0 { -1.0 } else { 1.0 };
  let abs = value.abs();
  if abs <= 0.04045 {
    value / 12.92
  } else {
    sign * ((abs + 0.055) / 1.055).powf(2.4)
  }
}

fn mat_vec(matrix: [[f64; 3]; 3], value: [f64; 3]) -> [f64; 3] {
  [
    matrix[0][0] * value[0] + matrix[0][1] * value[1] + matrix[0][2] * value[2],
    matrix[1][0] * value[0] + matrix[1][1] * value[1] + matrix[1][2] * value[2],
    matrix[2][0] * value[0] + matrix[2][1] * value[1] + matrix[2][2] * value[2],
  ]
}

#[cfg(test)]
mod tests {
  use super::eval_component;
  use crate::paint::color::parse_color;
  use crate::paint::color::serialize;

  #[test]
  fn calc_mixes_a_percent_and_a_number() {
    let value = eval_component("calc(50% + 10)", 255.0).unwrap();
    assert!((value - 137.5).abs() < 1.0e-9);
  }

  #[test]
  fn color_mix_in_srgb_serializes_like_the_canvas_test() {
    let mixed = parse_color("color-mix(in srgb, red, blue)").unwrap();
    assert_eq!(serialize(mixed), "color(srgb 0.5 0 0.5)");
    let same =
      parse_color("color-mix(in srgb, red, color(srgb 1 0 0))").unwrap();
    assert_eq!(serialize(same), "color(srgb 1 0 0)");
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
}
