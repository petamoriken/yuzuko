//! MIME type parsing and `data:` URL processing.
//!
//! Parsing and serialization follow the MIME Sniffing Standard. A `Blob` type
//! is the serialization of a successful parse, or the empty string.

/// A parsed MIME type. Parameters keep insertion order and the first value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Mime {
  type_: String,
  subtype: String,
  parameters: Vec<(String, String)>,
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

/// Serialization used as a `Blob` type and a `Content-Type` header.
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

/// File API type: serialize a successful parse, otherwise `""`.
pub(crate) fn blob_type(input: &str) -> String {
  parse(input)
    .map(|mime| serialize(&mime))
    .unwrap_or_default()
}

/// File API wire type: ASCII-lowercase when every character is in
/// U+0020..=U+007E, otherwise empty. Fetch uses this as a blob body's
/// `Content-Type`. `Blob.type` stays [`blob_type`] so MIME parsing tests
/// keep the serialized form (`text/html;charset=gbk`, no space).
pub(crate) fn file_type(input: &str) -> String {
  if input.chars().any(|ch| !('\u{20}'..='\u{7e}').contains(&ch)) {
    String::new()
  } else {
    input.to_ascii_lowercase()
  }
}

/// The `data:` URL processor. `input` is the URL serialization with the
/// leading `data:` and the fragment removed. `None` is a network error.
pub(crate) fn process_data_url(input: &str) -> Option<(Vec<u8>, String)> {
  let chars: Vec<char> = input.chars().collect();
  let mut index = 0;
  while index < chars.len() && chars[index] != ',' {
    index += 1;
  }
  if index >= chars.len() {
    return None;
  }
  let mut mime: String = chars[..index].iter().collect();
  index += 1;
  mime = trim_ascii_whitespace(&mime).to_string();
  let encoded: String = chars[index..].iter().collect();
  let mut body = percent_decode(&encoded);
  if let Some(stripped) = strip_base64_suffix(&mime) {
    let text: String = body.iter().copied().map(char::from).collect();
    body = crate::text::base64::decode_forgiving(&text).ok()?;
    mime = stripped;
  }
  if mime.starts_with(';') {
    mime.insert_str(0, "text/plain");
  }
  let record = parse(&mime).unwrap_or_else(default_data_mime);
  Some((body, serialize(&record)))
}

/// Run the processor on a parsed URL, excluding the fragment.
pub(crate) fn data_url(url: &url::Url) -> Option<(Vec<u8>, String)> {
  if url.scheme() != "data" {
    return None;
  }
  let mut copy = url.clone();
  copy.set_fragment(None);
  let serialized = copy.as_str();
  let input = serialized.strip_prefix("data:")?;
  process_data_url(input)
}

fn default_data_mime() -> Mime {
  Mime {
    type_: "text".to_string(),
    subtype: "plain".to_string(),
    parameters: vec![("charset".to_string(), "US-ASCII".to_string())],
  }
}

fn strip_base64_suffix(mime: &str) -> Option<String> {
  let chars: Vec<char> = mime.chars().collect();
  if chars.len() < 6 {
    return None;
  }
  let tail: String = chars[chars.len() - 6..].iter().collect();
  if !tail.eq_ignore_ascii_case("base64") {
    return None;
  }
  let mut index = chars.len() - 6;
  while index > 0 && chars[index - 1] == ' ' {
    index -= 1;
  }
  if index == 0 || chars[index - 1] != ';' {
    return None;
  }
  Some(chars[..index - 1].iter().collect())
}

fn trim_ascii_whitespace(value: &str) -> &str {
  value.trim_matches(|ch| matches!(ch, '\t' | '\n' | '\u{000c}' | '\r' | ' '))
}

fn percent_decode(input: &str) -> Vec<u8> {
  let bytes = input.as_bytes();
  let mut out = Vec::with_capacity(bytes.len());
  let mut index = 0;
  while index < bytes.len() {
    if bytes[index] == b'%'
      && index + 2 < bytes.len()
      && let (Some(high), Some(low)) =
        (hex_digit(bytes[index + 1]), hex_digit(bytes[index + 2]))
    {
      out.push((high << 4) | low);
      index += 3;
      continue;
    }
    out.push(bytes[index]);
    index += 1;
  }
  out
}

fn hex_digit(byte: u8) -> Option<u8> {
  match byte {
    b'0'..=b'9' => Some(byte - b'0'),
    b'a'..=b'f' => Some(byte - b'a' + 10),
    b'A'..=b'F' => Some(byte - b'A' + 10),
    _ => None,
  }
}

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
