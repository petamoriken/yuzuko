//! Canvas `font` shorthand. The getter returns this serialization.

use crate::paint::record::Family;
use crate::paint::record::FontDesc;

use super::intern_family;
use super::ready_family;

enum Listed {
  Sans,
  Serif,
  Mono,
  Cursive,
  Fantasy,
  Named(String),
}

enum Token {
  Ident(String),
  String(String),
  Number(f32),
  Dimension(f32, String),
  Percentage,
  Slash,
  Comma,
  Bad,
}

struct Css<'a> {
  s: &'a str,
  i: usize,
}

pub fn parse_font(input: &str) -> Option<FontDesc> {
  let tokens = tokenize(input)?;
  let mut p = 0usize;
  let mut italic = false;
  let mut small_caps = false;
  let mut weight = 400u16;
  let mut saw_style = false;
  let mut saw_variant = false;
  let mut saw_weight = false;
  let mut saw_stretch = false;

  let size = loop {
    let token = tokens.get(p)?;
    match token {
      Token::Dimension(value, unit) if is_px_pt(unit) => {
        p += 1;
        break px_size(*value, unit)?;
      }
      Token::Number(value) => {
        if saw_weight {
          return None;
        }
        saw_weight = true;
        weight = css_weight(*value)?;
        p += 1;
      }
      Token::Ident(name) => match name.to_ascii_lowercase().as_str() {
        "normal" => p += 1,
        "italic" | "oblique" => {
          if saw_style {
            return None;
          }
          saw_style = true;
          italic = true;
          let oblique = name.eq_ignore_ascii_case("oblique");
          p += 1;
          if oblique {
            skip_angle(&tokens, &mut p);
          }
        }
        "small-caps" => {
          if saw_variant {
            return None;
          }
          saw_variant = true;
          small_caps = true;
          p += 1;
        }
        "bold" => {
          if saw_weight {
            return None;
          }
          saw_weight = true;
          weight = 700;
          p += 1;
        }
        "bolder" | "lighter" => {
          if saw_weight {
            return None;
          }
          saw_weight = true;
          p += 1;
        }
        "ultra-condensed" | "extra-condensed" | "condensed"
        | "semi-condensed" | "semi-expanded" | "expanded"
        | "extra-expanded" | "ultra-expanded" => {
          if saw_stretch {
            return None;
          }
          saw_stretch = true;
          p += 1;
        }
        _ => return None,
      },
      _ => return None,
    }
  };

  if matches!(tokens.get(p), Some(Token::Slash)) {
    p += 1;
    match tokens.get(p)? {
      Token::Number(_) | Token::Dimension(_, _) | Token::Percentage => {
        p += 1;
      }
      Token::Ident(name) if name.eq_ignore_ascii_case("normal") => p += 1,
      _ => return None,
    }
  }

  let mut list = Vec::new();
  let mut expect = true;
  while p < tokens.len() {
    match &tokens[p] {
      Token::Comma => {
        if expect {
          return None;
        }
        expect = true;
        p += 1;
      }
      Token::String(name) => {
        if !expect || name.is_empty() {
          return None;
        }
        list.push(Listed::Named(name.clone()));
        expect = false;
        p += 1;
      }
      Token::Ident(_) => {
        if !expect {
          return None;
        }
        let mut parts = Vec::new();
        while let Some(Token::Ident(name)) = tokens.get(p) {
          parts.push(name.clone());
          p += 1;
        }
        list.push(listed_from_idents(&parts)?);
        expect = false;
      }
      _ => return None,
    }
  }
  if expect || list.is_empty() {
    return None;
  }

  let family = shaping_family(&list)?;
  let css = serialize(italic, small_caps, weight, size, &list);
  Some(FontDesc {
    size,
    weight,
    italic,
    family,
    css,
  })
}

pub fn font_css(font: &FontDesc) -> String {
  font.css.clone()
}

fn shaping_family(list: &[Listed]) -> Option<Family> {
  for item in list {
    if let Listed::Named(name) = item
      && let Some(id) = ready_family(name)
    {
      return Some(Family::Named(id));
    }
  }
  for item in list {
    match item {
      Listed::Sans => return Some(Family::Sans),
      Listed::Serif => return Some(Family::Serif),
      Listed::Mono => return Some(Family::Mono),
      _ => {}
    }
  }
  for item in list {
    if let Listed::Named(name) = item {
      return Some(Family::Unloaded(intern_family(name)));
    }
  }
  if list.is_empty() {
    None
  } else {
    // `cursive` and `fantasy` are stored and shaped as `sans-serif`.
    Some(Family::Sans)
  }
}

fn serialize(
  italic: bool,
  small_caps: bool,
  weight: u16,
  size: f32,
  list: &[Listed],
) -> String {
  let mut text = String::new();
  if italic {
    text.push_str("italic ");
  }
  if small_caps {
    text.push_str("small-caps ");
  }
  if weight != 400 {
    text.push_str(&weight.to_string());
    text.push(' ');
  }
  text.push_str(&format!("{size}"));
  text.push_str("px ");
  for (index, family) in list.iter().enumerate() {
    if index > 0 {
      text.push_str(", ");
    }
    text.push_str(&serialize_family(family));
  }
  text
}

fn serialize_family(family: &Listed) -> String {
  match family {
    Listed::Sans => "sans-serif".to_string(),
    Listed::Serif => "serif".to_string(),
    Listed::Mono => "monospace".to_string(),
    Listed::Cursive => "cursive".to_string(),
    Listed::Fantasy => "fantasy".to_string(),
    Listed::Named(name) => {
      if unquoted_family(name) {
        name.clone()
      } else {
        serialize_string(name)
      }
    }
  }
}

fn serialize_string(name: &str) -> String {
  let mut text = String::from("\"");
  for ch in name.chars() {
    let code = u32::from(ch);
    if ch == '\0' {
      text.push('\u{FFFD}');
    } else if (1..=0x1f).contains(&code) || code == 0x7f {
      text.push('\\');
      text.push_str(&format!("{code:x}"));
      text.push(' ');
    } else if ch == '"' || ch == '\\' {
      text.push('\\');
      text.push(ch);
    } else {
      text.push(ch);
    }
  }
  text.push('"');
  text
}

fn unquoted_family(name: &str) -> bool {
  let parts: Vec<&str> = name.split(' ').collect();
  if parts.iter().any(|part| part.is_empty()) {
    return false;
  }
  if parts.len() == 1 {
    let lower = parts[0].to_ascii_lowercase();
    if generic(&lower).is_some() || is_reserved(&lower) {
      return false;
    }
  }
  parts.iter().copied().all(is_css_ident)
}

fn listed_from_idents(parts: &[String]) -> Option<Listed> {
  if parts.len() == 1 {
    let lower = parts[0].to_ascii_lowercase();
    if let Some(generic) = generic(&lower) {
      return Some(generic);
    }
    if is_reserved(&lower) {
      return None;
    }
  }
  Some(Listed::Named(parts.join(" ")))
}

fn generic(lower: &str) -> Option<Listed> {
  Some(match lower {
    "sans-serif" => Listed::Sans,
    "serif" => Listed::Serif,
    "monospace" => Listed::Mono,
    "cursive" => Listed::Cursive,
    "fantasy" => Listed::Fantasy,
    _ => return None,
  })
}

fn is_reserved(lower: &str) -> bool {
  matches!(
    lower,
    "inherit" | "initial" | "unset" | "revert" | "revert-layer" | "default"
  )
}

fn is_px_pt(unit: &str) -> bool {
  unit.eq_ignore_ascii_case("px") || unit.eq_ignore_ascii_case("pt")
}

fn px_size(value: f32, unit: &str) -> Option<f32> {
  let size = if unit.eq_ignore_ascii_case("pt") {
    value * 4.0 / 3.0
  } else {
    value
  };
  if size.is_finite() && size > 0.0 {
    Some(size)
  } else {
    None
  }
}

fn css_weight(value: f32) -> Option<u16> {
  if !value.is_finite() || !(100.0..=900.0).contains(&value) {
    return None;
  }
  let weight = value as u16;
  if f32::from(weight) == value && weight.is_multiple_of(100) {
    Some(weight)
  } else {
    None
  }
}

fn skip_angle(tokens: &[Token], p: &mut usize) {
  if let Some(Token::Dimension(_, unit)) = tokens.get(*p)
    && matches!(
      unit.to_ascii_lowercase().as_str(),
      "deg" | "grad" | "rad" | "turn"
    )
  {
    *p += 1;
  }
}

fn tokenize(input: &str) -> Option<Vec<Token>> {
  let mut css = Css { s: input, i: 0 };
  let mut tokens = Vec::new();
  loop {
    css.skip_ws()?;
    if css.eof() {
      break;
    }
    let token = css.one_token()?;
    if matches!(token, Token::Bad) {
      return None;
    }
    tokens.push(token);
  }
  Some(tokens)
}

impl Css<'_> {
  fn eof(&self) -> bool {
    self.i >= self.s.len()
  }

  fn rest(&self) -> &str {
    &self.s[self.i..]
  }

  fn peek(&self) -> Option<char> {
    self.rest().chars().next()
  }

  fn bump(&mut self) -> Option<char> {
    let ch = self.peek()?;
    self.i += ch.len_utf8();
    Some(ch)
  }

  fn skip_ws(&mut self) -> Option<()> {
    loop {
      let rest = self.rest();
      if let Some(body) = rest.strip_prefix("/*") {
        let end = body.find("*/")?;
        self.i += 2 + end + 2;
        continue;
      }
      match self.peek() {
        Some(ch) if is_ws(ch) => {
          self.bump();
        }
        _ => return Some(()),
      }
    }
  }

  fn one_token(&mut self) -> Option<Token> {
    match self.peek()? {
      '"' | '\'' => self.string(),
      '/' => {
        self.bump();
        Some(Token::Slash)
      }
      ',' => {
        self.bump();
        Some(Token::Comma)
      }
      _ if self.starts_number() => self.numeric(),
      _ if self.starts_ident() => self.ident().map(Token::Ident),
      _ => {
        self.bump();
        Some(Token::Bad)
      }
    }
  }

  fn string(&mut self) -> Option<Token> {
    let quote = self.bump()?;
    let mut text = String::new();
    loop {
      let ch = self.peek()?;
      if ch == quote {
        self.bump();
        return Some(Token::String(text));
      }
      if ch == '\n' || ch == '\r' || ch == '\u{000c}' {
        return None;
      }
      if ch == '\\' {
        if let Some(escaped) = self.escape()? {
          text.push(escaped);
        }
        continue;
      }
      text.push(ch);
      self.bump();
    }
  }

  fn ident(&mut self) -> Option<String> {
    let mut text = String::new();
    while let Some(ch) = self.peek() {
      if ch == '\\' {
        if let Some(escaped) = self.escape()? {
          text.push(escaped);
        }
        continue;
      }
      let ok = if text.is_empty() {
        ch == '-' || is_ident_start(ch)
      } else {
        is_ident_char(ch)
      };
      if !ok {
        break;
      }
      text.push(ch);
      self.bump();
    }
    if text.is_empty() { None } else { Some(text) }
  }

  fn numeric(&mut self) -> Option<Token> {
    let start = self.i;
    if matches!(self.peek(), Some('+' | '-')) {
      self.bump();
    }
    while matches!(self.peek(), Some(ch) if ch.is_ascii_digit()) {
      self.bump();
    }
    if self.peek() == Some('.') {
      let mark = self.i;
      self.bump();
      if matches!(self.peek(), Some(ch) if ch.is_ascii_digit()) {
        while matches!(self.peek(), Some(ch) if ch.is_ascii_digit()) {
          self.bump();
        }
      } else {
        self.i = mark;
      }
    }
    if matches!(self.peek(), Some('e' | 'E')) {
      let mark = self.i;
      self.bump();
      if matches!(self.peek(), Some('+' | '-')) {
        self.bump();
      }
      if matches!(self.peek(), Some(ch) if ch.is_ascii_digit()) {
        while matches!(self.peek(), Some(ch) if ch.is_ascii_digit()) {
          self.bump();
        }
      } else {
        self.i = mark;
      }
    }
    let value: f32 = self.s[start..self.i].parse().ok()?;
    if self.peek() == Some('%') {
      self.bump();
      return Some(Token::Percentage);
    }
    if self.starts_ident() {
      let unit = self.ident()?;
      return Some(Token::Dimension(value, unit));
    }
    Some(Token::Number(value))
  }

  /// CSS escape. `None` is an escaped newline, which appends nothing.
  fn escape(&mut self) -> Option<Option<char>> {
    if self.bump() != Some('\\') {
      return None;
    }
    let Some(ch) = self.peek() else {
      return Some(Some('\u{FFFD}'));
    };
    if ch == '\n' {
      self.bump();
      return Some(None);
    }
    if ch.is_ascii_hexdigit() {
      let mut hex = String::new();
      while hex.len() < 6 {
        let Some(digit) = self.peek() else {
          break;
        };
        if !digit.is_ascii_hexdigit() {
          break;
        }
        hex.push(digit);
        self.bump();
      }
      if matches!(self.peek(), Some(next) if is_ws(next)) {
        self.bump();
      }
      let code = u32::from_str_radix(&hex, 16).ok()?;
      if code == 0 || code > 0x10_FFFF {
        return Some(Some('\u{FFFD}'));
      }
      return Some(Some(char::from_u32(code).unwrap_or('\u{FFFD}')));
    }
    self.bump();
    Some(Some(ch))
  }

  fn starts_number(&self) -> bool {
    let rest = self.rest();
    let rest = rest.strip_prefix(['+', '-']).unwrap_or(rest);
    let bytes = rest.as_bytes();
    match bytes.first() {
      Some(ch) if ch.is_ascii_digit() => true,
      Some(b'.') => bytes.get(1).is_some_and(u8::is_ascii_digit),
      _ => false,
    }
  }

  fn starts_ident(&self) -> bool {
    let mut chars = self.rest().chars();
    let Some(first) = chars.next() else {
      return false;
    };
    if first == '-' {
      let Some(second) = chars.next() else {
        return false;
      };
      return second == '-' || is_ident_start(second) || second == '\\';
    }
    is_ident_start(first) || first == '\\'
  }
}

fn is_ws(ch: char) -> bool {
  matches!(ch, ' ' | '\t' | '\n' | '\r' | '\u{000c}')
}

fn is_ident_start(ch: char) -> bool {
  ch.is_ascii_alphabetic() || ch == '_' || !ch.is_ascii()
}

fn is_ident_char(ch: char) -> bool {
  is_ident_start(ch) || ch.is_ascii_digit() || ch == '-'
}

fn is_css_ident(name: &str) -> bool {
  let mut chars = name.chars();
  let Some(first) = chars.next() else {
    return false;
  };
  if first == '-' {
    let Some(second) = chars.next() else {
      return false;
    };
    if second == '-' {
      return chars.all(is_ident_char);
    }
    return is_ident_start(second) && chars.all(is_ident_char);
  }
  is_ident_start(first) && chars.all(is_ident_char)
}

#[cfg(test)]
mod tests {
  use super::font_css;
  use super::parse_font;
  use crate::paint::record::Family;
  use crate::paint::record::FontDesc;

  #[test]
  fn font_shorthand_keeps_variant_and_family_list() {
    let complex =
      parse_font("small-caps italic 400 12px/2 Unknown Font, sans-serif")
        .expect("complex");
    let css = font_css(&complex);
    assert!(
      css == "italic small-caps 12px \"Unknown Font\", sans-serif"
        || css == "italic small-caps 12px Unknown Font, sans-serif",
      "{css}"
    );
    assert!(matches!(complex.family, Family::Sans));

    let complex2 = parse_font(
      "small-caps italic 400 12px/2 \"Unknown Font #2\", sans-serif",
    )
    .expect("complex2");
    assert_eq!(
      font_css(&complex2),
      "italic small-caps 12px \"Unknown Font #2\", sans-serif"
    );

    let family = parse_font(
      "20px cursive,fantasy,monospace,sans-serif,serif,UnquotedFont,\"QuotedFont\\\\\\\",\"",
    )
    .expect("family");
    assert_eq!(
      font_css(&family),
      "20px cursive, fantasy, monospace, sans-serif, serif, UnquotedFont, \"QuotedFont\\\\\\\",\""
    );
    assert!(matches!(family.family, Family::Mono));

    assert_eq!(
      font_css(&parse_font("italic 400 12px serif").unwrap()),
      "italic 12px serif"
    );
    assert_eq!(
      font_css(&parse_font("italic 300 12px serif").unwrap()),
      "italic 300 12px serif"
    );
    assert_eq!(font_css(&parse_font("20PX   SERIF").unwrap()), "20px serif");
    assert_eq!(font_css(&FontDesc::default()), "10px sans-serif");
    assert!(parse_font("inherit").is_none());
    assert!(parse_font("1em sans-serif").is_none());
    assert!(parse_font("message-box").is_none());
    assert!(parse_font("10px {bogus}").is_none());
    assert!(parse_font("10px initial").is_none());
    assert!(parse_font("").is_none());
  }
}
