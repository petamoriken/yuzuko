//! Terminal color for the REPL.
//!
//! `console` stays uncolored. A completion uses the Node inspector
//! palette. The input line uses the REPL editor palette. Both follow
//! `FORCE_COLOR` and `NO_COLOR`.

use std::borrow::Cow;
use std::io::IsTerminal;

use oxc_allocator::Allocator;
use oxc_parser::{Kind, ParseOptions, Parser};
use oxc_span::SourceType;

/// Node's inspector colours. The reset is the second code, not `0`.
#[derive(Clone, Copy)]
pub(crate) enum Ink {
  /// Cyan. Functions, markers, and collapsed objects.
  Special,
  /// Yellow. Numbers, bigints, and booleans.
  Number,
  /// Grey. `undefined` and empty array holes.
  Undefined,
  /// Bold. `null`.
  Null,
  /// Green. Strings and symbols.
  String,
  /// Magenta. Dates.
  Date,
  /// Red. Regular expressions.
  Regexp,
}

/// Whether the REPL paints completions, the input line, and errors.
///
/// A non-empty `FORCE_COLOR` enables color. Otherwise a non-empty
/// `NO_COLOR` disables it. Otherwise color follows stdout.
pub(crate) fn repl_color() -> bool {
  color_enabled(env_flag("FORCE_COLOR"), env_flag("NO_COLOR"), stdout_tty())
}

/// An empty value does not count as set.
fn env_flag(name: &str) -> bool {
  std::env::var_os(name).is_some_and(|value| !value.is_empty())
}

fn stdout_tty() -> bool {
  std::io::stdout().is_terminal()
}

fn color_enabled(force: bool, no_color: bool, terminal: bool) -> bool {
  if force {
    return true;
  }
  if no_color {
    return false;
  }
  terminal
}

pub(crate) fn paint(ink: Ink, text: &str, colors: bool) -> String {
  if !colors {
    return text.to_string();
  }
  let (open, close) = match ink {
    Ink::Special => (36, 39),
    Ink::Number => (33, 39),
    Ink::Undefined => (90, 39),
    Ink::Null => (1, 22),
    Ink::String => (32, 39),
    Ink::Date => (35, 39),
    Ink::Regexp => (31, 39),
  };
  format!("\x1b[{open}m{text}\x1b[{close}m")
}

/// Prints `text` on stdout. A colored REPL wraps the same text in red.
pub(crate) fn report_error(text: &str) {
  if repl_color() {
    println!("\x1b[31m{text}\x1b[0m");
  } else {
    println!("{text}");
  }
}

/// Removes SGR sequences. Cursor movement is left in place.
///
/// ESC has display width 0, and the rest of an SGR sequence does not,
/// so a measurement has to drop the whole sequence.
pub(crate) fn strip_sgr(text: &str) -> Cow<'_, str> {
  if !text.contains('\u{1b}') {
    return Cow::Borrowed(text);
  }
  let mut out = String::with_capacity(text.len());
  let mut rest = text;
  while let Some(pos) = rest.find('\u{1b}') {
    out.push_str(&rest[..pos]);
    let after = &rest[pos..];
    if let Some(len) = sgr_len(after) {
      rest = &after[len..];
    } else {
      out.push('\u{1b}');
      rest = &after['\u{1b}'.len_utf8()..];
    }
  }
  out.push_str(rest);
  Cow::Owned(out)
}

fn sgr_len(text: &str) -> Option<usize> {
  let bytes = text.as_bytes();
  if bytes.len() < 3 || bytes[0] != 0x1b || bytes[1] != b'[' {
    return None;
  }
  let mut index = 2;
  while index < bytes.len()
    && (bytes[index].is_ascii_digit() || bytes[index] == b';')
  {
    index += 1;
  }
  if index < bytes.len() && bytes[index] == b'm' {
    Some(index + 1)
  } else {
    None
  }
}

/// Colors one input line. The caller decides whether color is on.
pub(crate) fn highlight_line(line: &str) -> String {
  if line.is_empty() {
    return String::new();
  }
  let tokens = lex_line(line);
  let mut out = String::with_capacity(line.len());
  let mut cursor = 0usize;
  for index in 0..tokens.len() {
    let token = &tokens[index];
    if token.end <= token.start
      || token.end > line.len()
      || token.start < cursor
      || !line.is_char_boundary(token.start)
      || !line.is_char_boundary(token.end)
    {
      continue;
    }
    push_gap(&mut out, &line[cursor..token.start]);
    let slice = &line[token.start..token.end];
    if let Some(ink) = token_ink(&tokens, index, line) {
      out.push_str(&editor(ink, slice));
    } else {
      out.push_str(slice);
    }
    cursor = token.end;
  }
  push_gap(&mut out, &line[cursor..]);
  out
}

struct Lexed {
  start: usize,
  end: usize,
  kind: Kind,
}

fn lex_line(line: &str) -> Vec<Lexed> {
  let allocator = Allocator::default();
  let parsed = Parser::new(&allocator, line, SourceType::ts())
    .with_options(ParseOptions {
      allow_return_outside_function: true,
      ..ParseOptions::default()
    })
    .with_config(oxc_parser::config::TokensParserConfig)
    .parse();
  if !parsed.fatal_error {
    return parsed
      .tokens
      .iter()
      .filter_map(|token| lexed(token.start(), token.end(), token.kind(), line))
      .collect();
  }
  // `const` on its own is a fatal parse, and the parser then
  // discards every token. Drop that result before the lexer
  // allocates in the same arena.
  drop(parsed);
  lex_incomplete(&allocator, line)
}

fn lex_incomplete(allocator: &Allocator, line: &str) -> Vec<Lexed> {
  let mut lexer = oxc_parser::lexer::Lexer::new_for_benchmarks(
    allocator,
    line,
    SourceType::ts(),
    oxc_parser::config::TokensLexerConfig,
  );
  let mut out = Vec::new();
  let mut token = lexer.first_token();
  let mut previous = 0usize;
  for _ in 0..=line.len() {
    if token.kind().is_eof() {
      break;
    }
    let Some(lexed) = lexed(token.start(), token.end(), token.kind(), line)
    else {
      break;
    };
    if lexed.start < previous {
      break;
    }
    previous = lexed.end;
    out.push(lexed);
    token = lexer.next_token();
  }
  out
}

fn lexed(start: u32, end: u32, kind: Kind, line: &str) -> Option<Lexed> {
  let start = start as usize;
  let end = end as usize;
  if kind.is_eof() || end <= start || end > line.len() {
    return None;
  }
  if !line.is_char_boundary(start) || !line.is_char_boundary(end) {
    return None;
  }
  Some(Lexed { start, end, kind })
}

fn token_ink(tokens: &[Lexed], index: usize, line: &str) -> Option<EditorInk> {
  let kind = tokens[index].kind;
  if kind.is_number() {
    return Some(EditorInk::Yellow);
  }
  if is_string_token(kind) {
    return Some(EditorInk::Green);
  }
  if kind == Kind::RegExp {
    return Some(EditorInk::Red);
  }
  if matches!(kind, Kind::True | Kind::False | Kind::Null) {
    return Some(EditorInk::Yellow);
  }
  if kind == Kind::Undefined {
    return Some(EditorInk::Gray);
  }
  if kind == Kind::HashbangComment {
    return Some(EditorInk::Gray);
  }
  if kind == Kind::From {
    return (next_kind(tokens, index) == Some(Kind::Str))
      .then_some(EditorInk::Cyan);
  }
  if kind.is_any_keyword() {
    return Some(EditorInk::Cyan);
  }
  if kind == Kind::Ident {
    let token = &tokens[index];
    return ident_ink(&line[token.start..token.end], next_kind(tokens, index));
  }
  None
}

fn is_string_token(kind: Kind) -> bool {
  matches!(
    kind,
    Kind::Str
      | Kind::NoSubstitutionTemplate
      | Kind::TemplateHead
      | Kind::TemplateMiddle
      | Kind::TemplateTail
  )
}

fn next_kind(tokens: &[Lexed], index: usize) -> Option<Kind> {
  tokens.get(index + 1).map(|token| token.kind)
}

fn ident_ink(text: &str, next: Option<Kind>) -> Option<EditorInk> {
  match text {
    "undefined" => Some(EditorInk::Gray),
    "Infinity" | "NaN" => Some(EditorInk::Yellow),
    "async" | "of" => Some(EditorInk::Cyan),
    "from" if next == Some(Kind::Str) => Some(EditorInk::Cyan),
    _ if next == Some(Kind::LParen) => Some(EditorInk::Blue),
    _ => None,
  }
}

/// Comments are trivia, so they sit in the gaps between tokens.
fn push_gap(out: &mut String, gap: &str) {
  let bytes = gap.as_bytes();
  let mut index = 0usize;
  while index < bytes.len() {
    if bytes[index] == b'/'
      && index + 1 < bytes.len()
      && (bytes[index + 1] == b'/' || bytes[index + 1] == b'*')
    {
      let end = if bytes[index + 1] == b'/' {
        bytes.len()
      } else {
        gap[index + 2..]
          .find("*/")
          .map(|found| index + 2 + found + 2)
          .unwrap_or(bytes.len())
      };
      out.push_str(&editor(EditorInk::Gray, &gap[index..end]));
      index = end;
      continue;
    }
    let ch = gap[index..].chars().next().unwrap();
    out.push(ch);
    index += ch.len_utf8();
  }
}

#[derive(Clone, Copy)]
enum EditorInk {
  Green,
  Yellow,
  Cyan,
  Red,
  Blue,
  Gray,
}

fn editor(ink: EditorInk, text: &str) -> String {
  let open = match ink {
    EditorInk::Green => "32",
    EditorInk::Yellow => "33",
    EditorInk::Cyan => "36",
    EditorInk::Red => "31",
    EditorInk::Blue => "94",
    EditorInk::Gray => "38;5;245",
  };
  format!("\x1b[{open}m{text}\x1b[0m")
}

#[cfg(test)]
mod tests {
  use super::{color_enabled, highlight_line, strip_sgr};

  fn green(text: &str) -> String {
    format!("\x1b[32m{text}\x1b[0m")
  }

  fn yellow(text: &str) -> String {
    format!("\x1b[33m{text}\x1b[0m")
  }

  fn cyan(text: &str) -> String {
    format!("\x1b[36m{text}\x1b[0m")
  }

  fn red(text: &str) -> String {
    format!("\x1b[31m{text}\x1b[0m")
  }

  fn blue(text: &str) -> String {
    format!("\x1b[94m{text}\x1b[0m")
  }

  fn gray(text: &str) -> String {
    format!("\x1b[38;5;245m{text}\x1b[0m")
  }

  #[test]
  fn color_switch_follows_force_no_color_and_the_terminal() {
    assert!(color_enabled(true, true, false));
    assert!(color_enabled(true, false, false));
    assert!(!color_enabled(false, true, true));
    assert!(color_enabled(false, false, true));
    assert!(!color_enabled(false, false, false));
  }

  #[test]
  fn strip_sgr_keeps_cursor_movement() {
    assert_eq!(strip_sgr("12").as_ref(), "12");
    assert_eq!(
      strip_sgr("\x1b[J> \x1b[33m12\x1b[0m\x1b[3C").as_ref(),
      "\x1b[J> 12\x1b[3C"
    );
    assert_eq!(
      strip_sgr("\x1b[38;5;245mundefined\x1b[0m").as_ref(),
      "undefined"
    );
  }

  #[test]
  fn highlight_colors_a_line_like_the_repl_editor() {
    assert_eq!(
      highlight_line("const n = 1"),
      format!("{} n = {}", cyan("const"), yellow("1"))
    );
    assert_eq!(highlight_line("\"hi\""), green("\"hi\""));
    assert_eq!(highlight_line("`a`"), green("`a`"));
    assert_eq!(highlight_line("true"), yellow("true"));
    assert_eq!(highlight_line("false"), yellow("false"));
    assert_eq!(highlight_line("null"), yellow("null"));
    assert_eq!(highlight_line("undefined"), gray("undefined"));
    assert_eq!(highlight_line("Infinity"), yellow("Infinity"));
    assert_eq!(highlight_line("NaN"), yellow("NaN"));
    assert_eq!(highlight_line("1n"), yellow("1n"));
    assert_eq!(highlight_line("async"), cyan("async"));
    assert_eq!(highlight_line("abs()"), format!("{}()", blue("abs")));
    assert_eq!(
      highlight_line("const from = 1"),
      format!("{} from = {}", cyan("const"), yellow("1"))
    );
    assert_eq!(
      highlight_line("import x from \"m\""),
      format!("{} x {} {}", cyan("import"), cyan("from"), green("\"m\""))
    );
    assert_eq!(
      highlight_line("type N = number"),
      format!("{} N = {}", cyan("type"), cyan("number"))
    );
    assert_eq!(highlight_line("const"), cyan("const"));
    assert_eq!(
      highlight_line("1 / 2"),
      format!("{} / {}", yellow("1"), yellow("2"))
    );
    assert_eq!(
      highlight_line("1/2"),
      format!("{}/{}", yellow("1"), yellow("2"))
    );
    assert_eq!(highlight_line("/a/g"), red("/a/g"));
    assert_eq!(highlight_line("\"//\""), green("\"//\""));
    assert_eq!(highlight_line("`//`"), green("`//`"));
    assert_eq!(highlight_line("// c"), gray("// c"));
    assert_eq!(
      highlight_line("/* c */ const"),
      format!("{} {}", gray("/* c */"), cyan("const"))
    );
    assert_eq!(
      highlight_line("const a = 1 // c"),
      format!("{} a = {} {}", cyan("const"), yellow("1"), gray("// c"))
    );
  }
}
