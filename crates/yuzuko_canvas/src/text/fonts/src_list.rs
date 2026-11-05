//! CSS Fonts `src` list for a `FontFace` string source.
//!
//! A string with no `url(` or `local(` and no space or comma stays a single
//! URL. `format()` is a skip filter. `tech()` is accepted and ignored.

use cssparser::BasicParseErrorKind;
use cssparser::ParseError;
use cssparser::Parser;
use cssparser::ParserInput;
use cssparser::Token;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Item {
  Url {
    url: String,
    formats: Option<Vec<String>>,
  },
  Local(String),
}

#[derive(Debug)]
pub(super) enum Error {
  NotUrl,
  Syntax,
}

pub(super) fn parse(css: &str) -> Result<Vec<Item>, Error> {
  let trimmed = css.trim();
  if trimmed.is_empty() {
    return Err(Error::NotUrl);
  }
  if !looks_like_src(trimmed) {
    if trimmed.contains([',', ' ', '\t', '\n', '\r']) {
      return Err(Error::NotUrl);
    }
    return Ok(vec![Item::Url {
      url: trimmed.to_string(),
      formats: None,
    }]);
  }
  // cssparser accepts `url(` at EOF as an empty URL. An unclosed
  // parenthesis, string, or comment is still invalid CSS.
  if !balanced(trimmed) {
    return Err(Error::Syntax);
  }
  let mut source = ParserInput::new(trimmed);
  let mut parser = Parser::new(&mut source);
  let items = parser
    .parse_comma_separated(parse_item)
    .map_err(|_| Error::Syntax)?;
  if items.is_empty() {
    return Err(Error::Syntax);
  }
  Ok(items)
}

/// `true` when `format()` names a container this runtime can decode.
pub(super) fn format_supported(name: &str) -> bool {
  let name = name.to_ascii_lowercase();
  let name = name.strip_suffix("-variations").unwrap_or(&name);
  matches!(name, "woff2" | "woff" | "truetype" | "opentype")
}

fn looks_like_src(css: &str) -> bool {
  let lower = css.to_ascii_lowercase();
  lower.contains("url(") || lower.contains("local(")
}

/// Parentheses, quotes, and comments are closed. Escapes consume one
/// character so `url(a\(b)` is not treated as an extra parenthesis.
fn balanced(css: &str) -> bool {
  let mut chars = css.chars().peekable();
  let mut depth = 0i32;
  while let Some(ch) = chars.next() {
    match ch {
      '\\' => {
        if chars.next().is_none() {
          return false;
        }
      }
      '"' | '\'' => {
        let quote = ch;
        let mut closed = false;
        while let Some(inner) = chars.next() {
          if inner == '\\' {
            if chars.next().is_none() {
              return false;
            }
          } else if inner == quote {
            closed = true;
            break;
          }
        }
        if !closed {
          return false;
        }
      }
      '/' if chars.peek() == Some(&'*') => {
        chars.next();
        let mut closed = false;
        while let Some(inner) = chars.next() {
          if inner == '*' && chars.peek() == Some(&'/') {
            chars.next();
            closed = true;
            break;
          }
        }
        if !closed {
          return false;
        }
      }
      '(' => depth += 1,
      ')' => {
        depth -= 1;
        if depth < 0 {
          return false;
        }
      }
      _ => {}
    }
  }
  depth == 0
}

fn parse_item<'i>(
  parser: &mut Parser<'i, '_>,
) -> Result<Item, ParseError<'i, ()>> {
  if let Ok(item) = parser.try_parse(parse_url_item) {
    return Ok(item);
  }
  parse_local_item(parser)
}

fn parse_url_item<'i>(
  parser: &mut Parser<'i, '_>,
) -> Result<Item, ParseError<'i, ()>> {
  let url = parser.expect_url().map_err(basic)?.to_string();
  let mut formats = None;
  let mut saw_tech = false;
  loop {
    let state = parser.state();
    let name = match parser.expect_function() {
      Ok(name) => name.clone(),
      Err(_) => {
        parser.reset(&state);
        break;
      }
    };
    if name.eq_ignore_ascii_case("format") {
      if formats.is_some() || saw_tech {
        return Err(invalid(parser));
      }
      formats = Some(parser.parse_nested_block(parse_keywords)?);
    } else if name.eq_ignore_ascii_case("tech") {
      if saw_tech {
        return Err(invalid(parser));
      }
      saw_tech = true;
      parser.parse_nested_block(parse_keywords)?;
    } else {
      return Err(invalid(parser));
    }
  }
  Ok(Item::Url { url, formats })
}

fn parse_local_item<'i>(
  parser: &mut Parser<'i, '_>,
) -> Result<Item, ParseError<'i, ()>> {
  parser.expect_function_matching("local").map_err(basic)?;
  let name = parser.parse_nested_block(parse_family_name)?;
  Ok(Item::Local(name))
}

fn parse_keywords<'i>(
  parser: &mut Parser<'i, '_>,
) -> Result<Vec<String>, ParseError<'i, ()>> {
  parser.parse_comma_separated(|parser| {
    let value = parser.expect_ident_or_string().map_err(basic)?;
    Ok(value.to_string())
  })
}

fn parse_family_name<'i>(
  parser: &mut Parser<'i, '_>,
) -> Result<String, ParseError<'i, ()>> {
  if let Ok(name) = parser.try_parse(|parser| {
    parser
      .expect_string()
      .map(|value| value.to_string())
      .map_err(basic)
  }) {
    return Ok(name);
  }
  if parser.is_exhausted() {
    return Err(invalid(parser));
  }
  let mut name = String::new();
  while !parser.is_exhausted() {
    let word = {
      let token = parser.next().map_err(basic)?;
      match token {
        Token::Ident(value) => Some(value.to_string()),
        Token::Number {
          int_value: Some(value),
          ..
        } => Some(value.to_string()),
        Token::Number { value, .. } if value.is_finite() => {
          Some(number_word(*value))
        }
        _ => None,
      }
    };
    let Some(word) = word else {
      return Err(invalid(parser));
    };
    if !name.is_empty() {
      name.push(' ');
    }
    name.push_str(&word);
  }
  if name.is_empty() {
    Err(invalid(parser))
  } else {
    Ok(name)
  }
}

fn number_word(value: f32) -> String {
  if value.fract() == 0.0 && value.abs() < 1_000_000.0 {
    format!("{}", value as i32)
  } else {
    format!("{value}")
  }
}

fn basic<'i>(error: cssparser::BasicParseError<'i>) -> ParseError<'i, ()> {
  error.into()
}

fn invalid<'i>(parser: &mut Parser<'i, '_>) -> ParseError<'i, ()> {
  parser.new_error(BasicParseErrorKind::EndOfInput)
}

#[cfg(test)]
mod tests {
  use super::Error;
  use super::Item;
  use super::format_supported;
  use super::parse;

  fn url(value: &str, formats: Option<&[&str]>) -> Item {
    Item::Url {
      url: value.to_string(),
      formats: formats
        .map(|items| items.iter().map(|item| (*item).to_string()).collect()),
    }
  }

  #[test]
  fn bare_url_stays_one_source() {
    assert_eq!(
      parse("  https://example.test/a.ttf  ").unwrap(),
      vec![url("https://example.test/a.ttf", None)]
    );
    assert!(matches!(parse("not a url"), Err(Error::NotUrl)));
    assert!(matches!(parse(""), Err(Error::NotUrl)));
  }

  #[test]
  fn format_and_tech_parse() {
    let items = parse(
      r#"url("a.eot") format("embedded-opentype"), url(a.woff2) format(woff2) tech(variations), local("Helvetica Neue"), local(Source Sans 3)"#,
    )
    .unwrap();
    assert_eq!(
      items,
      vec![
        url("a.eot", Some(&["embedded-opentype"])),
        url("a.woff2", Some(&["woff2"])),
        Item::Local("Helvetica Neue".to_string()),
        Item::Local("Source Sans 3".to_string()),
      ]
    );
    assert!(!format_supported("embedded-opentype"));
    assert!(format_supported("WOFF2"));
    assert!(format_supported("woff2-variations"));
    assert!(!format_supported("svg"));
  }

  #[test]
  fn several_format_keywords_and_bad_grammar() {
    let items = parse(r#"url(a.ttf) format("collection", truetype)"#).unwrap();
    assert_eq!(items, vec![url("a.ttf", Some(&["collection", "truetype"]))]);
    assert_eq!(parse(r#"url("a(b)")"#).unwrap(), vec![url("a(b)", None)]);
    assert!(matches!(parse("url("), Err(Error::Syntax)));
    assert!(matches!(parse("url(a.ttf"), Err(Error::Syntax)));
    assert!(matches!(parse("url(a.ttf),"), Err(Error::Syntax)));
    assert!(matches!(parse("url(a.ttf) format()"), Err(Error::Syntax)));
    assert!(matches!(
      parse("url(a.ttf) tech(variations) format(woff2)"),
      Err(Error::Syntax)
    ));
    assert!(matches!(parse(r#"local("A" extra)"#), Err(Error::Syntax)));
  }
}
