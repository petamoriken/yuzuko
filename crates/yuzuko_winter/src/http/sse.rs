//! Server-sent event field parser.
//!
//! Bytes are UTF-8, decoded with U+FFFD for an invalid sequence. A line ends
//! at U+000D, U+000A, or U+000D U+000A. A blank line dispatches the buffered
//! event. `retry` updates the reconnection time as soon as the field is read.
//!
//! A line or the data of one event longer than [`MAX_EVENT`] bytes fails the
//! stream, the same bound as a WebSocket message.

/// Bytes of one line, and of the data of one event.
pub const MAX_EVENT: usize = 16 * 1024 * 1024;

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

/// A line or an event exceeded [`MAX_EVENT`].
#[derive(Debug)]
pub struct SseError;

#[derive(Default)]
pub struct SseParser {
  pending: Vec<u8>,
  /// Bytes of `pending` already searched for a line end.
  scanned: usize,
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
    // Lines are read from `start` and the consumed prefix is removed once,
    // so a chunk of many lines costs time linear in its length.
    let mut start = 0;
    if self.held_cr {
      if self.pending.first() == Some(&b'\n') {
        start = 1;
        self.scanned = self.scanned.max(1);
      }
      if !self.pending.is_empty() {
        self.held_cr = false;
      }
    }
    loop {
      let from = self.scanned.max(start);
      let found = self.pending[from..]
        .iter()
        .position(|byte| matches!(byte, b'\n' | b'\r'))
        .map(|offset| from + offset);
      let Some(end) = found else {
        break;
      };
      let mut next = end + 1;
      if self.pending[end] == b'\r' {
        match self.pending.get(next) {
          Some(b'\n') => next += 1,
          None if !eof => self.held_cr = true,
          _ => {}
        }
      }
      let line = decode_line(&self.pending[start..end]);
      start = next;
      self.scanned = next;
      if let Some(item) = self.line(&line)? {
        items.push(item);
      }
    }
    if eof && start < self.pending.len() {
      let line = decode_line(&self.pending[start..]);
      start = self.pending.len();
      if let Some(item) = self.line(&line)? {
        items.push(item);
      }
    }
    self.pending.drain(..start);
    self.scanned = self.pending.len();
    if self.pending.len() > MAX_EVENT {
      return Err(SseError);
    }
    Ok(items)
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
        if self.data.len() + value.len() + 1 > MAX_EVENT {
          return Err(SseError);
        }
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

fn decode_line(bytes: &[u8]) -> String {
  String::from_utf8_lossy(bytes).into_owned()
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

  fn data_of(items: Vec<SseItem>) -> Vec<String> {
    items
      .into_iter()
      .filter_map(|item| match item {
        SseItem::Event(event) => Some(event.data),
        SseItem::Retry(_) => None,
      })
      .collect()
  }

  #[test]
  fn lines_split_across_chunks_and_a_held_cr() {
    let mut parser = SseParser::default();
    let mut events = Vec::new();
    for chunk in ["da", "ta: a", "b\r", "\ndata: c\r", "\r", "\n"] {
      events.extend(data_of(parser.push(chunk.as_bytes()).unwrap()));
    }
    assert_eq!(events, vec!["ab\nc".to_string()]);
    events = data_of(parser.push(b"data: d\n").unwrap());
    events.extend(data_of(parser.finish().unwrap()));
    assert!(events.is_empty(), "{events:?}");
  }

  #[test]
  fn invalid_utf8_becomes_replacement_characters() {
    let mut parser = SseParser::default();
    let events = data_of(parser.push(b"data: \xff\xfe\n\n").unwrap());
    assert_eq!(events, vec!["\u{fffd}\u{fffd}".to_string()]);
  }

  #[test]
  fn an_event_past_the_limit_fails_the_stream() {
    let mut parser = SseParser::default();
    let line = vec![b'x'; MAX_EVENT + 1];
    assert!(parser.push(&line).is_err());
    let mut parser = SseParser::default();
    let half = format!("data: {}\n", "y".repeat(MAX_EVENT / 2));
    assert!(parser.push(half.as_bytes()).is_ok());
    assert!(parser.push(half.as_bytes()).is_err());
  }

  #[test]
  fn many_lines_in_one_chunk_are_linear() {
    let mut parser = SseParser::default();
    let chunk = "data: x\n\n".repeat(200_000);
    let started = std::time::Instant::now();
    let events = data_of(parser.push(chunk.as_bytes()).unwrap());
    assert_eq!(events.len(), 200_000);
    assert!(started.elapsed() < std::time::Duration::from_secs(5));
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
