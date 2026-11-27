//! MIME type parsing and `data:` URL processing.
//!
//! Parsing and serialization follow the MIME Sniffing Standard. A blob read
//! from a response stores that serialization. A `Blob` or `File` constructed
//! in script stores the File API type instead.

/// A parsed MIME type. Parameters keep insertion order and the first value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Mime {
  type_: String,
  subtype: String,
  parameters: Vec<(String, String)>,
}

impl Mime {
  /// The first parameter value for `name`, already ASCII-lowercased.
  pub(crate) fn parameter(&self, name: &str) -> Option<&str> {
    self
      .parameters
      .iter()
      .find(|(key, _)| key == name)
      .map(|(_, value)| value.as_str())
  }

  /// Replaces the first parameter named `name`. A missing name is left absent.
  pub(crate) fn set_parameter(&mut self, name: &str, value: &str) {
    if let Some((_, slot)) =
      self.parameters.iter_mut().find(|(key, _)| key == name)
    {
      *slot = value.to_string();
    }
  }
}

/// Parse `input`, or `None` when the type or subtype is not a token.
pub(crate) fn parse(input: &str) -> Option<Mime> {
  let chars: Vec<char> = input.chars().collect();
  let mut cursor = Cursor {
    chars: &chars,
    index: 0,
  };
  trim_http_whitespace(&mut cursor);
  // Trailing whitespace is removed by shrinking the end, not the cursor.
  let mut end = cursor.chars.len();
  while end > cursor.index && is_http_whitespace(cursor.chars[end - 1]) {
    end -= 1;
  }
  cursor.chars = &cursor.chars[..end];

  let type_ = cursor.collect_while(|ch| ch != '/');
  if type_.is_empty() || !solely_http_tokens(&type_) || cursor.done() {
    return None;
  }
  cursor.bump();
  let mut subtype = cursor.collect_while(|ch| ch != ';');
  trim_trailing_http_whitespace(&mut subtype);
  if subtype.is_empty() || !solely_http_tokens(&subtype) {
    return None;
  }
  let mut mime = Mime {
    type_: type_.to_ascii_lowercase(),
    subtype: subtype.to_ascii_lowercase(),
    parameters: Vec::new(),
  };
  while !cursor.done() {
    cursor.bump();
    cursor.collect_while(is_http_whitespace);
    let name = cursor
      .collect_while(|ch| ch != ';' && ch != '=')
      .to_ascii_lowercase();
    if !cursor.done() {
      if cursor.current() == ';' {
        continue;
      }
      cursor.bump();
    }
    if cursor.done() {
      break;
    }
    let value = if cursor.current() == '"' {
      let value = cursor.collect_quoted();
      cursor.collect_while(|ch| ch != ';');
      value
    } else {
      let mut value = cursor.collect_while(|ch| ch != ';');
      trim_trailing_http_whitespace(&mut value);
      if value.is_empty() {
        continue;
      }
      value
    };
    if !name.is_empty()
      && solely_http_tokens(&name)
      && solely_quoted_tokens(&value)
      && !mime.parameters.iter().any(|(key, _)| key == &name)
    {
      mime.parameters.push((name, value));
    }
  }
  Some(mime)
}

/// Whether `input` is a MIME type, including parameters.
pub fn is_mime_type(input: &str) -> bool {
  parse(input).is_some()
}

/// The essence (`type/subtype`, lowercase) of a parsed MIME type, or `None`
/// when `input` does not parse.
pub fn mime_essence(input: &str) -> Option<String> {
  parse(input).map(|mime| format!("{}/{}", mime.type_, mime.subtype))
}

/// The `charset` parameter of a parsed MIME type.
#[cfg_attr(not(feature = "fetch"), allow(dead_code))]
pub(crate) fn mime_charset(input: &str) -> Option<String> {
  parse(input)?.parameter("charset").map(str::to_owned)
}

/// Serialized MIME type, or `None` when `input` does not parse.
pub fn serialized_mime(input: &str) -> Option<String> {
  parse(input).map(|mime| serialize(&mime))
}

/// MIME sniffing JavaScript MIME type. Parameters are ignored.
pub fn is_javascript_mime(input: &str) -> bool {
  let Some(mime) = parse(input) else {
    return false;
  };
  matches!(
    (mime.type_.as_str(), mime.subtype.as_str()),
    ("application", "ecmascript")
      | ("application", "javascript")
      | ("application", "x-ecmascript")
      | ("application", "x-javascript")
      | ("text", "ecmascript")
      | ("text", "javascript")
      | ("text", "javascript1.0")
      | ("text", "javascript1.1")
      | ("text", "javascript1.2")
      | ("text", "javascript1.3")
      | ("text", "javascript1.4")
      | ("text", "javascript1.5")
      | ("text", "jscript")
      | ("text", "livescript")
      | ("text", "x-ecmascript")
      | ("text", "x-javascript")
  )
}

/// Serialization used as a response blob's type and a `Content-Type` header.
pub(crate) fn serialize(mime: &Mime) -> String {
  let mut out = String::new();
  out.push_str(&mime.type_);
  out.push('/');
  out.push_str(&mime.subtype);
  for (name, value) in &mime.parameters {
    out.push(';');
    out.push_str(name);
    out.push('=');
    if value.is_empty() || !solely_http_tokens(value) {
      out.push('"');
      for ch in value.chars() {
        if ch == '"' || ch == '\\' {
          out.push('\\');
        }
        out.push(ch);
      }
      out.push('"');
    } else {
      out.push_str(value);
    }
  }
  out
}

/// Serialized MIME type. A blob read from a response stores this as `type`.
pub(crate) fn blob_type(input: &str) -> String {
  parse(input)
    .map(|mime| serialize(&mime))
    .unwrap_or_default()
}

/// File API constructor and `slice` type: ASCII lowercase when every
/// character is in U+0020..=U+007E, otherwise empty.
pub(crate) fn file_type(input: &str) -> String {
  if input.chars().any(|ch| !('\u{20}'..='\u{7e}').contains(&ch)) {
    String::new()
  } else {
    input.to_ascii_lowercase()
  }
}

/// The Fetch `data:` URL processor, from the `data-url` crate. `input` is
/// the URL serialization after `data:`. A fragment is ignored. `None` is a
/// network error.
#[cfg_attr(not(feature = "fetch"), allow(dead_code))]
pub(crate) fn process_data_url(input: &str) -> Option<(Vec<u8>, String)> {
  let url = format!("data:{input}");
  let processed = data_url::DataUrl::process(&url).ok()?;
  let (body, _) = processed.decode_to_vec().ok()?;
  Some((body, processed.mime_type().to_string()))
}

/// Run the processor on a parsed URL, excluding the fragment.
#[cfg_attr(not(feature = "fetch"), allow(dead_code))]
pub(crate) fn data_url(url: &url::Url) -> Option<(Vec<u8>, String)> {
  if url.scheme() != "data" {
    return None;
  }
  let input = url.as_str().strip_prefix("data:")?;
  process_data_url(input)
}

#[cfg_attr(not(feature = "fetch"), allow(unused_imports))]
pub(crate) use yuzuko_core::percent::decode as percent_decode;

struct Cursor<'a> {
  chars: &'a [char],
  index: usize,
}

impl Cursor<'_> {
  fn done(&self) -> bool {
    self.index >= self.chars.len()
  }

  fn current(&self) -> char {
    self.chars[self.index]
  }

  fn bump(&mut self) {
    self.index += 1;
  }

  fn collect_while(&mut self, mut pred: impl FnMut(char) -> bool) -> String {
    let start = self.index;
    while self.index < self.chars.len() && pred(self.chars[self.index]) {
      self.index += 1;
    }
    self.chars[start..self.index].iter().collect()
  }

  fn collect_quoted(&mut self) -> String {
    self.bump();
    let mut value = String::new();
    loop {
      value.push_str(&self.collect_while(|ch| ch != '"' && ch != '\\'));
      if self.done() {
        break;
      }
      let quote_or_backslash = self.current();
      self.bump();
      if quote_or_backslash == '\\' {
        if self.done() {
          value.push('\\');
          break;
        }
        value.push(self.current());
        self.bump();
      } else {
        break;
      }
    }
    value
  }
}

fn trim_http_whitespace(cursor: &mut Cursor<'_>) {
  while !cursor.done() && is_http_whitespace(cursor.current()) {
    cursor.bump();
  }
}

fn trim_trailing_http_whitespace(value: &mut String) {
  while value.ends_with(['\t', '\n', '\r', ' ']) {
    value.pop();
  }
}

fn is_http_whitespace(ch: char) -> bool {
  matches!(ch, '\t' | '\n' | '\r' | ' ')
}

fn solely_http_tokens(value: &str) -> bool {
  value.chars().all(is_http_token)
}

fn solely_quoted_tokens(value: &str) -> bool {
  value.chars().all(is_quoted_token)
}

fn is_http_token(ch: char) -> bool {
  ch.is_ascii_alphanumeric()
    || matches!(
      ch,
      '!'
        | '#'
        | '$'
        | '%'
        | '&'
        | '\''
        | '*'
        | '+'
        | '-'
        | '.'
        | '^'
        | '_'
        | '`'
        | '|'
        | '~'
    )
}

fn is_quoted_token(ch: char) -> bool {
  ch == '\t'
    || ('\u{20}'..='\u{7e}').contains(&ch)
    || ('\u{80}'..='\u{ff}').contains(&ch)
}

#[cfg(test)]
mod tests {
  use super::blob_type;
  use super::data_url;
  use super::is_javascript_mime;

  #[test]
  fn javascript_mime_ignores_parameters_and_case() {
    assert!(is_javascript_mime("text/javascript"));
    assert!(is_javascript_mime("Text/JavaScript; charset=utf-8"));
    assert!(is_javascript_mime("application/javascript"));
    assert!(is_javascript_mime("text/ecmascript"));
    assert!(is_javascript_mime("text/javascript;bla;bla"));
    assert!(!is_javascript_mime("text/plain"));
    assert!(!is_javascript_mime("text/html"));
    assert!(!is_javascript_mime("TEXT/HTML"));
    assert!(!is_javascript_mime("text/csv;charset=utf-8"));
    assert!(!is_javascript_mime("aaa/aaa"));
  }

  #[test]
  fn wpt_mime_type_resources() {
    let files = [
      include_str!(
        "../../../../tests/wpt/suite/mimesniff/mime-types/resources/mime-types.json"
      ),
      include_str!(
        "../../../../tests/wpt/suite/mimesniff/mime-types/resources/generated-mime-types.json"
      ),
    ];
    let mut checked = 0;
    for file in files {
      for case in json_cases(file) {
        let Json::Object(fields) = case else {
          continue;
        };
        let Json::String(input) = field(&fields, "input") else {
          continue;
        };
        let expected = match field(&fields, "output") {
          Json::Null => String::new(),
          Json::String(value) => value,
          other => panic!("output is {other:?}"),
        };
        let got = blob_type(&input);
        assert_eq!(got, expected, "input {input:?}");
        checked += 1;
      }
    }
    assert!(checked > 900, "checked {checked} MIME cases");
  }

  #[test]
  fn file_type_keeps_parameter_space() {
    assert_eq!(super::blob_type("a/b; c=d"), "a/b;c=d");
    assert_eq!(super::file_type("a/b; c=d"), "a/b; c=d");
    assert_eq!(
      super::blob_type("text/html; charset=gbk"),
      "text/html;charset=gbk"
    );
  }

  #[test]
  fn parsed_serialization_is_stable() {
    let files = [
      include_str!(
        "../../../../tests/wpt/suite/mimesniff/mime-types/resources/mime-types.json"
      ),
      include_str!(
        "../../../../tests/wpt/suite/mimesniff/mime-types/resources/generated-mime-types.json"
      ),
    ];
    for file in files {
      for case in json_cases(file) {
        let Json::Object(fields) = case else {
          continue;
        };
        let Json::String(output) = field(&fields, "output") else {
          continue;
        };
        assert_eq!(blob_type(&output), output, "reserialize {output:?}");
      }
    }
  }

  #[test]
  fn wpt_data_urls() {
    let file = include_str!(
      "../../../../tests/wpt/suite/fetch/data-urls/resources/data-urls.json"
    );
    let Json::Array(cases) = parse_json(file) else {
      panic!("data-urls.json is not an array");
    };
    assert!(cases.len() > 50);
    for case in cases {
      let Json::Array(items) = case else {
        panic!("case is not an array");
      };
      let Json::String(input) = items[0].clone() else {
        panic!("input is not a string");
      };
      let expected_mime = match &items[1] {
        Json::Null => None,
        Json::String(value) => Some(value.clone()),
        other => panic!("mime is {other:?}"),
      };
      let got = url::Url::parse(&input)
        .ok()
        .filter(|url| url.username().is_empty() && url.password().is_none())
        .and_then(|url| data_url(&url));
      match expected_mime {
        None => assert!(got.is_none(), "{input:?} should fail, got {got:?}"),
        Some(mime) => {
          let Json::Array(body) = &items[2] else {
            panic!("body missing for {input:?}");
          };
          let expected_body: Vec<u8> = body
            .iter()
            .map(|item| match item {
              Json::Number(value) => *value as u8,
              other => panic!("body byte {other:?}"),
            })
            .collect();
          let (bytes, got_mime) =
            got.unwrap_or_else(|| panic!("{input:?} should succeed"));
          assert_eq!(got_mime, mime, "{input:?}");
          assert_eq!(bytes, expected_body, "{input:?}");
        }
      }
    }
  }

  #[derive(Clone, Debug)]
  enum Json {
    Null,
    String(String),
    Number(u32),
    Bool,
    Array(Vec<Json>),
    Object(Vec<(String, Json)>),
  }

  fn field(fields: &[(String, Json)], name: &str) -> Json {
    fields
      .iter()
      .find(|(key, _)| key == name)
      .map(|(_, value)| value.clone())
      .unwrap_or(Json::Null)
  }

  fn json_cases(text: &str) -> Vec<Json> {
    match parse_json(text) {
      Json::Array(items) => items,
      other => panic!("expected array, got {other:?}"),
    }
  }

  fn parse_json(text: &str) -> Json {
    let mut parser = JsonParser {
      chars: text.chars().collect(),
      index: 0,
    };
    parser.skip_ws();
    parser.value()
  }

  struct JsonParser {
    chars: Vec<char>,
    index: usize,
  }

  impl JsonParser {
    fn skip_ws(&mut self) {
      while self.index < self.chars.len()
        && self.chars[self.index].is_whitespace()
      {
        self.index += 1;
      }
    }

    fn bump(&mut self) -> char {
      let ch = self.chars[self.index];
      self.index += 1;
      ch
    }

    fn value(&mut self) -> Json {
      self.skip_ws();
      match self.chars[self.index] {
        'n' => {
          self.index += 4;
          Json::Null
        }
        't' => {
          self.index += 4;
          Json::Bool
        }
        'f' => {
          self.index += 5;
          Json::Bool
        }
        '"' => Json::String(self.string()),
        '[' => self.array(),
        '{' => self.object(),
        '-' | '0'..='9' => self.number(),
        other => panic!("json {other} at {}", self.index),
      }
    }

    fn array(&mut self) -> Json {
      self.bump();
      let mut items = Vec::new();
      loop {
        self.skip_ws();
        if self.chars[self.index] == ']' {
          self.bump();
          return Json::Array(items);
        }
        items.push(self.value());
        self.skip_ws();
        if self.chars[self.index] == ',' {
          self.bump();
        }
      }
    }

    fn object(&mut self) -> Json {
      self.bump();
      let mut fields = Vec::new();
      loop {
        self.skip_ws();
        if self.chars[self.index] == '}' {
          self.bump();
          return Json::Object(fields);
        }
        let key = self.string();
        self.skip_ws();
        assert_eq!(self.bump(), ':');
        fields.push((key, self.value()));
        self.skip_ws();
        if self.chars[self.index] == ',' {
          self.bump();
        }
      }
    }

    fn number(&mut self) -> Json {
      let start = self.index;
      if self.chars[self.index] == '-' {
        self.index += 1;
      }
      while self.index < self.chars.len()
        && self.chars[self.index].is_ascii_digit()
      {
        self.index += 1;
      }
      let text: String = self.chars[start..self.index].iter().collect();
      Json::Number(text.parse().unwrap_or(0))
    }

    fn string(&mut self) -> String {
      assert_eq!(self.bump(), '"');
      let mut out = String::new();
      loop {
        match self.bump() {
          '"' => return out,
          '\\' => {
            let escaped = self.bump();
            match escaped {
              '"' | '\\' | '/' => out.push(escaped),
              'b' => out.push('\u{0008}'),
              'f' => out.push('\u{000c}'),
              'n' => out.push('\n'),
              'r' => out.push('\r'),
              't' => out.push('\t'),
              'u' => {
                let mut hex = String::new();
                for _ in 0..4 {
                  hex.push(self.bump());
                }
                let code = u32::from_str_radix(&hex, 16).unwrap();
                out.push(char::from_u32(code).unwrap());
              }
              other => panic!("escape {other}"),
            }
          }
          ch => out.push(ch),
        }
      }
    }
  }
}
