//! Server-sent event field parser.
//!
//! Bytes are UTF-8. A line ends at U+000D, U+000A, or U+000D U+000A. A blank
//! line dispatches the buffered event. `retry` updates the reconnection time
//! as soon as the field is read.

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SseEvent {
  pub name: String,
  pub data: String,
  pub id: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SseItem {
  Event(SseEvent),
  Retry(u64),
}

#[derive(Debug)]
pub struct SseError;

#[derive(Default)]
pub struct SseParser {
  pending: Vec<u8>,
  checked_bom: bool,
  held_cr: bool,
  data: String,
  event: String,
  id: String,
}

impl SseParser {
  pub fn set_last_id(&mut self, id: String) {
    self.id = id;
  }

  pub fn push(&mut self, chunk: &[u8]) -> Result<Vec<SseItem>, SseError> {
    self.pending.extend_from_slice(chunk);
    self.drain(false)
  }

  pub fn finish(&mut self) -> Result<Vec<SseItem>, SseError> {
    self.drain(true)
  }

  fn drain(&mut self, eof: bool) -> Result<Vec<SseItem>, SseError> {
    if !self.checked_bom {
      if self.pending.starts_with(&[0xEF, 0xBB, 0xBF]) {
        self.pending.drain(..3);
        self.checked_bom = true;
      } else if eof || self.pending.len() >= 3 {
        self.checked_bom = true;
      } else {
        return Ok(Vec::new());
      }
    }
    let mut items = Vec::new();
    while let Some(line) = self.take_line(eof)? {
      if let Some(item) = self.line(&line)? {
        items.push(item);
      }
    }
    Ok(items)
  }

  fn take_line(&mut self, eof: bool) -> Result<Option<String>, SseError> {
    if self.held_cr {
      if self.pending.first() == Some(&b'\n') {
        self.pending.remove(0);
      }
      self.held_cr = false;
    }
    let mut index = 0;
    while index < self.pending.len() {
      match self.pending[index] {
        b'\n' => {
          let bytes: Vec<u8> = self.pending.drain(..=index).collect();
          return decode_line(&bytes[..bytes.len() - 1]);
        }
        b'\r' => {
          let drop_lf = self.pending.get(index + 1) == Some(&b'\n');
          let end = if drop_lf { index + 1 } else { index };
          let bytes: Vec<u8> = self.pending.drain(..=end).collect();
          if !drop_lf && self.pending.is_empty() && !eof {
            self.held_cr = true;
          }
          return decode_line(&bytes[..index]);
        }
        _ => index += 1,
      }
    }
    if eof && !self.pending.is_empty() {
      let bytes = std::mem::take(&mut self.pending);
      return decode_line(&bytes);
    }
    Ok(None)
  }

  fn line(&mut self, line: &str) -> Result<Option<SseItem>, SseError> {
    if line.is_empty() {
      return Ok(self.dispatch());
    }
    if line.starts_with(':') {
      return Ok(None);
    }
    let (field, value) = match line.split_once(':') {
      Some((field, rest)) => {
        let value = rest.strip_prefix(' ').unwrap_or(rest);
        (field, value)
      }
      None => (line, ""),
    };
    match field {
      "event" => self.event = value.to_string(),
      "data" => {
        self.data.push_str(value);
        self.data.push('\n');
      }
      "id" => {
        if !value.contains('\0') {
          self.id = value.to_string();
        }
      }
      "retry"
        if !value.is_empty()
          && value.bytes().all(|byte| byte.is_ascii_digit())
          && let Ok(ms) = value.parse::<u64>() =>
      {
        return Ok(Some(SseItem::Retry(ms)));
      }
      _ => {}
    }
    Ok(None)
  }

  fn dispatch(&mut self) -> Option<SseItem> {
    if self.data.is_empty() {
      self.event.clear();
      return None;
    }
    if self.data.ends_with('\n') {
      self.data.pop();
    }
    let name = if self.event.is_empty() {
      "message".to_string()
    } else {
      std::mem::take(&mut self.event)
    };
    if !self.event.is_empty() {
      self.event.clear();
    }
    let data = std::mem::take(&mut self.data);
    let id = self.id.clone();
    Some(SseItem::Event(SseEvent { name, data, id }))
  }
}

fn decode_line(bytes: &[u8]) -> Result<Option<String>, SseError> {
  let (text, malformed) = encoding_rs::UTF_8.decode_without_bom_handling(bytes);
  if malformed {
    return Err(SseError);
  }
  Ok(Some(text.into_owned()))
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn field_parsing_matches_the_eventsource_sample() {
    let message = "data:\0\ndata:  2\rData:1\ndata\0:2\ndata:1\r\0data:4\nda-ta:3\rdata_5\ndata:3\rdata:\r\n data:32\ndata:4\n\n";
    let mut parser = SseParser::default();
    let items = parser.push(message.as_bytes()).expect("utf-8");
    let events: Vec<_> = items
      .into_iter()
      .filter_map(|item| match item {
        SseItem::Event(event) => Some(event.data),
        SseItem::Retry(_) => None,
      })
      .collect();
    assert_eq!(events, vec!["\0\n 2\n1\n3\n\n4".to_string()]);
  }

  #[test]
  fn retry_is_decimal() {
    let mut parser = SseParser::default();
    let items = parser.push(b"retry:03000\ndata:x\n\n").expect("utf-8");
    assert!(
      items
        .iter()
        .any(|item| matches!(item, SseItem::Retry(3000)))
    );
  }
}
