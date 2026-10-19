//! CSS `<filter-value-list>` parsing for `CanvasRenderingContext2D.filter`.
//!
//! A parsed filter is stored and is not painted. vello has no CSS filter.

use cssparser::BasicParseErrorKind;
use cssparser::ParseError;
use cssparser::Parser;
use cssparser::ParserInput;
use cssparser::Token;

use crate::paint::color::parse_color;

/// `true` when `input` is `none` or a `<filter-value-list>`.
pub fn is_filter(input: &str) -> bool {
  let mut source = ParserInput::new(input);
  let mut parser = Parser::new(&mut source);
  parse_filter(&mut parser).is_ok()
}

fn parse_filter<'i>(
  parser: &mut Parser<'i, '_>,
) -> Result<(), ParseError<'i, ()>> {
  if parser.is_exhausted() {
    return Err(fail(parser));
  }
  let state = parser.state();
  if let Ok(ident) = parser.expect_ident() {
    let wide = is_wide_keyword(&ident);
    let none = ident.eq_ignore_ascii_case("none");
    if parser.is_exhausted() {
      if none && !wide {
        return Ok(());
      }
      return Err(fail(parser));
    }
  }
  parser.reset(&state);
  let mut any = false;
  while !parser.is_exhausted() {
    parse_item(parser)?;
    any = true;
  }
  if any { Ok(()) } else { Err(fail(parser)) }
}

fn parse_item<'i>(
  parser: &mut Parser<'i, '_>,
) -> Result<(), ParseError<'i, ()>> {
  let state = parser.state();
  if parser.expect_url().is_ok() {
    return Ok(());
  }
  parser.reset(&state);
  let name = parser.expect_function()?.to_ascii_lowercase();
  parser.parse_nested_block(|parser| match name.as_str() {
    "blur" => optional_length(parser, true),
    "brightness" | "contrast" | "grayscale" | "invert" | "opacity"
    | "saturate" | "sepia" => optional_number_or_percent(parser),
    "hue-rotate" => optional_angle(parser),
    "drop-shadow" => drop_shadow(parser),
    _ => Err(fail(parser)),
  })
}

fn optional_length<'i>(
  parser: &mut Parser<'i, '_>,
  nonnegative: bool,
) -> Result<(), ParseError<'i, ()>> {
  if parser.is_exhausted() {
    return Ok(());
  }
  let value = length(parser)?;
  if nonnegative && value < 0.0 {
    return Err(fail(parser));
  }
  if parser.is_exhausted() {
    Ok(())
  } else {
    Err(fail(parser))
  }
}

fn optional_number_or_percent<'i>(
  parser: &mut Parser<'i, '_>,
) -> Result<(), ParseError<'i, ()>> {
  if parser.is_exhausted() {
    return Ok(());
  }
  let state = parser.state();
  if parser.expect_number().is_err() {
    parser.reset(&state);
    parser.expect_percentage()?;
  }
  if parser.is_exhausted() {
    Ok(())
  } else {
    Err(fail(parser))
  }
}

fn optional_angle<'i>(
  parser: &mut Parser<'i, '_>,
) -> Result<(), ParseError<'i, ()>> {
  if parser.is_exhausted() {
    return Ok(());
  }
  angle(parser)?;
  if parser.is_exhausted() {
    Ok(())
  } else {
    Err(fail(parser))
  }
}

fn length<'i>(parser: &mut Parser<'i, '_>) -> Result<f32, ParseError<'i, ()>> {
  let state = parser.state();
  if let Ok(value) = parser.try_parse(Parser::expect_number) {
    if value == 0.0 {
      return Ok(0.0);
    }
    parser.reset(&state);
    return Err(fail(parser));
  }
  let value = {
    let token = parser.next()?;
    match token {
      Token::Dimension { value, unit, .. } if is_length_unit(unit) => {
        Some(*value)
      }
      _ => None,
    }
  };
  value.ok_or_else(|| fail(parser))
}

fn angle<'i>(parser: &mut Parser<'i, '_>) -> Result<(), ParseError<'i, ()>> {
  let state = parser.state();
  if let Ok(value) = parser.try_parse(Parser::expect_number) {
    if value == 0.0 {
      return Ok(());
    }
    parser.reset(&state);
    return Err(fail(parser));
  }
  let ok = {
    let token = parser.next()?;
    match token {
      Token::Dimension { unit, .. } if is_angle_unit(unit) => true,
      _ => false,
    }
  };
  if ok { Ok(()) } else { Err(fail(parser)) }
}

fn drop_shadow<'i>(
  parser: &mut Parser<'i, '_>,
) -> Result<(), ParseError<'i, ()>> {
  let mut lengths = Vec::new();
  let mut saw_color = false;
  while !parser.is_exhausted() {
    let state = parser.state();
    if let Ok(value) = parser.try_parse(length) {
      lengths.push(value);
      continue;
    }
    parser.reset(&state);
    if saw_color {
      return Err(fail(parser));
    }
    color(parser)?;
    saw_color = true;
  }
  if !(2..=3).contains(&lengths.len()) {
    return Err(fail(parser));
  }
  if lengths.len() == 3 && lengths[2] < 0.0 {
    return Err(fail(parser));
  }
  Ok(())
}

fn color<'i>(parser: &mut Parser<'i, '_>) -> Result<(), ParseError<'i, ()>> {
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
    ColorToken::Word(text) => {
      if parse_color(&text).is_some() {
        Ok(())
      } else {
        Err(fail(parser))
      }
    }
    ColorToken::Function => {
      parser.parse_nested_block(|inner| {
        while inner.next().is_ok() {}
        Ok(())
      })?;
      let text = parser.slice_from(start);
      if parse_color(text).is_some() {
        Ok(())
      } else {
        Err(fail(parser))
      }
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

fn is_length_unit(unit: &str) -> bool {
  matches!(
    unit.to_ascii_lowercase().as_str(),
    "px"
      | "cm"
      | "mm"
      | "q"
      | "in"
      | "pt"
      | "pc"
      | "em"
      | "rem"
      | "ex"
      | "ch"
      | "cap"
      | "ic"
      | "lh"
      | "rlh"
      | "vw"
      | "vh"
      | "vi"
      | "vb"
      | "vmin"
      | "vmax"
      | "dvw"
      | "dvh"
      | "dvi"
      | "dvb"
      | "dvmin"
      | "dvmax"
      | "svw"
      | "svh"
      | "svi"
      | "svb"
      | "svmin"
      | "svmax"
      | "lvw"
      | "lvh"
      | "lvi"
      | "lvb"
      | "lvmin"
      | "lvmax"
      | "cqw"
      | "cqh"
      | "cqi"
      | "cqb"
      | "cqmin"
      | "cqmax"
  )
}

fn is_angle_unit(unit: &str) -> bool {
  matches!(
    unit.to_ascii_lowercase().as_str(),
    "deg" | "rad" | "grad" | "turn"
  )
}

fn fail<'i>(parser: &mut Parser<'i, '_>) -> ParseError<'i, ()> {
  parser.new_error::<()>(BasicParseErrorKind::QualifiedRuleInvalid)
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
}
