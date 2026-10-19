//! Terminal line editor for `yuzuko repl`.
//!
//! A pipe reads whole lines. A Unix terminal handles the Node.js readline
//! emacs bindings in this process. Windows console input stays line-oriented.

use std::io::{self, IsTerminal, Write};

use crate::repl;

pub struct Editor {
  history: Vec<String>,
  nav: Option<usize>,
  draft: String,
  cancel_armed: bool,
  /// Text last removed by Ctrl-U or Ctrl-K. Ctrl-Y inserts it.
  killed: String,
  /// Bytes already read from the terminal and not yet handled. A chunk
  /// boundary must not drop a paste that arrived with the submitting line.
  pending: std::collections::VecDeque<u8>,
}

impl Editor {
  pub fn new() -> Self {
    Self {
      history: Vec::new(),
      nav: None,
      draft: String::new(),
      cancel_armed: false,
      killed: String::new(),
      pending: std::collections::VecDeque::new(),
    }
  }

  pub fn read(
    &mut self,
    complete: &mut dyn FnMut(Option<&str>) -> Vec<String>,
  ) -> Option<String> {
    if !io::stdin().is_terminal() {
      return read_pipe();
    }
    #[cfg(unix)]
    {
      let _raw = match RawTerm::enter() {
        Some(raw) => raw,
        None => return read_pipe(),
      };
      self.read_unix(complete)
    }
    #[cfg(not(unix))]
    {
      let _ = complete;
      read_pipe()
    }
  }

  #[cfg(unix)]
  fn read_unix(
    &mut self,
    complete: &mut dyn FnMut(Option<&str>) -> Vec<String>,
  ) -> Option<String> {
    let mut buffer = String::new();
    let mut cursor = 0usize;
    let mut view = View::blank();
    prompt(buffer.is_empty());
    loop {
      if self.pending.is_empty() && !fill_bytes(&mut self.pending) {
        if buffer.is_empty() {
          println!();
          return None;
        }
        println!();
        if repl::input_is_complete(&buffer) {
          self.push_history(&buffer);
          return Some(std::mem::take(&mut buffer));
        }
        println!("SyntaxError: incomplete input");
        return None;
      }
      let byte = self.pending.pop_front().unwrap_or(0);
      if byte != 0x03 {
        self.cancel_armed = false;
      }
      match byte {
        b'\r' | b'\n' => {
          if buffer.trim().is_empty() {
            buffer.clear();
            cursor = 0;
            println!();
            prompt(true);
            view = View::blank();
            continue;
          }
          if repl::input_is_complete(&buffer) {
            println!();
            self.push_history(&buffer);
            return Some(std::mem::take(&mut buffer));
          }
          insert(&mut buffer, &mut cursor, "\n");
          render(&mut view, &buffer, cursor);
        }
        0x01 => {
          cursor = line_start(&buffer, cursor);
          render(&mut view, &buffer, cursor);
        }
        0x05 => {
          cursor = line_end(&buffer, cursor);
          render(&mut view, &buffer, cursor);
        }
        0x02 => {
          cursor = move_char(&buffer, cursor, -1);
          render(&mut view, &buffer, cursor);
        }
        0x06 => {
          cursor = move_char(&buffer, cursor, 1);
          render(&mut view, &buffer, cursor);
        }
        0x10 => {
          self.history_step(-1, &mut buffer, &mut cursor);
          render(&mut view, &buffer, cursor);
        }
        0x0e => {
          self.history_step(1, &mut buffer, &mut cursor);
          render(&mut view, &buffer, cursor);
        }
        0x04 => {
          if buffer.is_empty() {
            println!();
            return None;
          }
          if delete_forward(&mut buffer, cursor) {
            render(&mut view, &buffer, cursor);
          }
        }
        0x03 => {
          if buffer.is_empty() && self.cancel_armed {
            println!();
            return None;
          }
          self.cancel_armed = true;
          self.nav = None;
          buffer.clear();
          cursor = 0;
          println!("^C");
          prompt(true);
          view = View::blank();
        }
        0x0b => {
          self.kill_forward(&mut buffer, &mut cursor);
          render(&mut view, &buffer, cursor);
        }
        0x15 => {
          self.kill_backward_line(&mut buffer, &mut cursor);
          render(&mut view, &buffer, cursor);
        }
        0x17 => {
          let start = move_word_back(&buffer, cursor);
          if start < cursor {
            buffer.replace_range(start..cursor, "");
            cursor = start;
            render(&mut view, &buffer, cursor);
          }
        }
        0x19 => {
          if !self.killed.is_empty() {
            let yanked = self.killed.clone();
            insert(&mut buffer, &mut cursor, &yanked);
            render(&mut view, &buffer, cursor);
          }
        }
        0x0c => clear_screen(&mut view, &buffer, cursor),
        0x7f | 0x08 => {
          if cursor == 0 {
            continue;
          }
          let prev = prev_boundary(&buffer, cursor);
          buffer.replace_range(prev..cursor, "");
          cursor = prev;
          render(&mut view, &buffer, cursor);
        }
        0x1b => {
          let seq = take_escape(&mut self.pending);
          if self.escape(&seq, &mut buffer, &mut cursor) {
            render(&mut view, &buffer, cursor);
          }
        }
        b'\t' => {
          self.complete_at(complete, &mut buffer, &mut cursor, &mut view);
        }
        byte if (0x20..0x7f).contains(&byte) => {
          insert(&mut buffer, &mut cursor, &(byte as char).to_string());
          render(&mut view, &buffer, cursor);
        }
        byte if byte >= 0x80 => {
          let mut raw = vec![byte];
          let width = utf8_width(byte);
          while raw.len() < width {
            if self.pending.is_empty() && !fill_bytes(&mut self.pending) {
              break;
            }
            if let Some(next) = self.pending.pop_front() {
              raw.push(next);
            }
          }
          let text = String::from_utf8_lossy(&raw).to_string();
          insert(&mut buffer, &mut cursor, &text);
          render(&mut view, &buffer, cursor);
        }
        _ => {}
      }
    }
  }

  fn push_history(&mut self, chunk: &str) {
    self.nav = None;
    self.draft.clear();
    if self.history.last().is_some_and(|last| last == chunk) {
      return;
    }
    self.history.push(chunk.to_string());
  }

  /// Handles one sequence after Esc. Returns whether the buffer or cursor
  /// changed. The caller redraws.
  #[cfg(unix)]
  fn escape(
    &mut self,
    seq: &str,
    buffer: &mut String,
    cursor: &mut usize,
  ) -> bool {
    let before = (buffer.clone(), *cursor);
    match seq {
      "[A" | "OA" => self.history_step(-1, buffer, cursor),
      "[B" | "OB" => self.history_step(1, buffer, cursor),
      "[C" | "OC" => *cursor = move_char(buffer, *cursor, 1),
      "[D" | "OD" => *cursor = move_char(buffer, *cursor, -1),
      "[H" | "OH" | "[1~" | "[7~" => {
        *cursor = line_start(buffer, *cursor);
      }
      "[F" | "OF" | "[4~" | "[8~" => {
        *cursor = line_end(buffer, *cursor);
      }
      "[3~" => {
        delete_forward(buffer, *cursor);
      }
      "[1;5D" | "[1;3D" | "[5D" | "b" => {
        *cursor = move_word_back(buffer, *cursor);
      }
      "[1;5C" | "[1;3C" | "[5C" | "f" => {
        *cursor = move_word_forward(buffer, *cursor);
      }
      "d" => {
        let end = move_word_forward(buffer, *cursor);
        if end > *cursor {
          buffer.replace_range(*cursor..end, "");
        }
      }
      "\u{7f}" | "\u{8}" => {
        let start = move_word_back(buffer, *cursor);
        if start < *cursor {
          buffer.replace_range(start..*cursor, "");
          *cursor = start;
        }
      }
      _ => return false,
    }
    before != (buffer.clone(), *cursor)
  }

  #[cfg(unix)]
  fn kill_backward_line(&mut self, buffer: &mut String, cursor: &mut usize) {
    let start = line_start(buffer, *cursor);
    self.kill(buffer, cursor, start, *cursor);
  }

  #[cfg(unix)]
  fn kill_forward(&mut self, buffer: &mut String, cursor: &mut usize) {
    let mut end = line_end(buffer, *cursor);
    if end == *cursor
      && end < buffer.len()
      && buffer.as_bytes().get(end) == Some(&b'\n')
    {
      end += 1;
    }
    self.kill(buffer, cursor, *cursor, end);
  }

  #[cfg(unix)]
  fn kill(
    &mut self,
    buffer: &mut String,
    cursor: &mut usize,
    start: usize,
    end: usize,
  ) {
    if start >= end {
      return;
    }
    self.killed = buffer[start..end].to_string();
    buffer.replace_range(start..end, "");
    *cursor = start;
  }

  #[cfg(unix)]
  fn history_step(
    &mut self,
    delta: isize,
    buffer: &mut String,
    cursor: &mut usize,
  ) {
    if self.history.is_empty() {
      return;
    }
    if self.nav.is_none() {
      if delta > 0 {
        return;
      }
      self.draft = buffer.clone();
      self.nav = Some(self.history.len());
    }
    let current = self.nav.unwrap_or(self.history.len());
    let next = current as isize + delta;
    if next < 0 {
      return;
    }
    if next as usize >= self.history.len() {
      self.nav = None;
      *buffer = std::mem::take(&mut self.draft);
    } else {
      self.nav = Some(next as usize);
      *buffer = self.history[next as usize].clone();
    }
    *cursor = buffer.len();
  }

  #[cfg(unix)]
  fn complete_at(
    &mut self,
    complete: &mut dyn FnMut(Option<&str>) -> Vec<String>,
    buffer: &mut String,
    cursor: &mut usize,
    view: &mut View,
  ) {
    let (start, expr) = word_before(buffer, *cursor);
    if expr.is_empty() {
      return;
    }
    let (range_start, prefix, object) = match expr.rfind('.') {
      Some(dot) => {
        let object = &expr[..dot];
        if object.is_empty() {
          return;
        }
        (
          start + dot + 1,
          expr[dot + 1..].to_string(),
          Some(object.to_string()),
        )
      }
      None => (start, expr, None),
    };
    let mut names = complete(object.as_deref());
    names.retain(|name| name.starts_with(&prefix) && is_ident_name(name));
    names.sort();
    names.dedup();
    if names.is_empty() {
      return;
    }
    if names.len() == 1 {
      if names[0] == prefix {
        return;
      }
      let end = *cursor;
      buffer.replace_range(range_start..end, &names[0]);
      *cursor = range_start + names[0].len();
      render(view, buffer, *cursor);
      return;
    }
    println!();
    println!("{}", names.join("  "));
    *view = View::blank();
    render(view, buffer, *cursor);
  }
}

fn read_pipe() -> Option<String> {
  let stdin = io::stdin();
  let mut block = String::new();
  let mut line = String::new();
  loop {
    prompt(block.is_empty());
    line.clear();
    let read = io::BufRead::read_line(&mut stdin.lock(), &mut line);
    let count = match read {
      Ok(count) => count,
      Err(error) => {
        println!("SyntaxError: {error}");
        return None;
      }
    };
    if count == 0 {
      if block.is_empty() {
        println!();
        return None;
      }
      println!();
      if repl::input_is_complete(&block) {
        return Some(block);
      }
      println!("SyntaxError: incomplete input");
      return None;
    }
    if line.ends_with('\n') {
      line.pop();
      if line.ends_with('\r') {
        line.pop();
      }
    }
    if !block.is_empty() {
      block.push('\n');
    }
    block.push_str(&line);
    if block.trim().is_empty() {
      block.clear();
      println!();
      continue;
    }
    if repl::input_is_complete(&block) {
      println!();
      return Some(block);
    }
    println!();
  }
}

fn prompt(fresh: bool) {
  if fresh {
    print!("> ");
  } else {
    print!("... ");
  }
  let _ = io::stdout().flush();
}

/// Where the terminal cursor sits inside the buffer on screen.
#[cfg(unix)]
struct View {
  text: String,
  cursor: usize,
}

#[cfg(unix)]
impl View {
  fn blank() -> Self {
    Self {
      text: String::new(),
      cursor: 0,
    }
  }
}

/// Redraws the prompt block and leaves the terminal cursor on `cursor`.
/// `view` is where that cursor was before this redraw.
#[cfg(unix)]
fn render(view: &mut View, buffer: &str, cursor: usize) {
  let above = view.text[..view.cursor]
    .bytes()
    .filter(|byte| *byte == b'\n')
    .count();
  if above > 0 {
    print!("\x1b[{above}A");
  }
  print!("\r\x1b[J");
  write_buffer(buffer);
  place_cursor(buffer, cursor);
  let _ = io::stdout().flush();
  view.text = buffer.to_string();
  view.cursor = cursor;
}

#[cfg(unix)]
fn clear_screen(view: &mut View, buffer: &str, cursor: usize) {
  print!("\x1b[2J\x1b[H");
  let _ = io::stdout().flush();
  *view = View::blank();
  render(view, buffer, cursor);
}

#[cfg(unix)]
fn write_buffer(buffer: &str) {
  let mut lines = buffer.split('\n');
  if let Some(first) = lines.next() {
    print!("> {first}");
  } else {
    print!("> ");
  }
  for line in lines {
    print!("\n... {line}");
  }
}

#[cfg(unix)]
fn place_cursor(buffer: &str, cursor: usize) {
  let end_line = buffer.bytes().filter(|byte| *byte == b'\n').count();
  let cur_line = buffer[..cursor]
    .bytes()
    .filter(|byte| *byte == b'\n')
    .count();
  let up = end_line - cur_line;
  if up > 0 {
    print!("\x1b[{up}A");
  }
  print!("\r");
  let line_off = buffer[..cursor].rfind('\n').map_or(0, |index| index + 1);
  let col = prompt_columns(cur_line) + columns(&buffer[line_off..cursor]);
  if col > 0 {
    print!("\x1b[{col}C");
  }
}

#[cfg(unix)]
fn prompt_columns(line: usize) -> usize {
  if line == 0 { 2 } else { 4 }
}

#[cfg(unix)]
fn columns(text: &str) -> usize {
  use unicode_width::UnicodeWidthStr;
  text.width()
}

#[cfg(test)]
mod tests {
  use super::{line_end, line_start, move_word_back, move_word_forward};

  #[test]
  fn line_edges_stay_on_the_current_line() {
    assert_eq!(line_start("ab\ncd", 4), 3);
    assert_eq!(line_end("ab\ncd", 1), 2);
    assert_eq!(line_end("ab\ncd", 3), 5);
  }

  #[test]
  fn word_motion_stops_on_punctuation() {
    assert_eq!(move_word_forward("ab cd", 0), 2);
    assert_eq!(move_word_back("10 + abc", "10 + abc".len()), 5);
    assert_eq!(move_word_back("20+2", "20+2".len()), 3);
  }
}

fn insert(buffer: &mut String, cursor: &mut usize, text: &str) {
  buffer.insert_str(*cursor, text);
  *cursor += text.len();
}

fn prev_boundary(buffer: &str, cursor: usize) -> usize {
  buffer[..cursor]
    .char_indices()
    .next_back()
    .map(|(index, _)| index)
    .unwrap_or(0)
}

fn next_boundary(buffer: &str, cursor: usize) -> usize {
  let rest = &buffer[cursor..];
  rest
    .chars()
    .next()
    .map(|ch| cursor + ch.len_utf8())
    .unwrap_or(cursor)
}

fn move_char(buffer: &str, cursor: usize, delta: isize) -> usize {
  if delta < 0 {
    if cursor == 0 {
      cursor
    } else {
      prev_boundary(buffer, cursor)
    }
  } else if cursor >= buffer.len() {
    cursor
  } else {
    next_boundary(buffer, cursor)
  }
}

fn line_start(buffer: &str, cursor: usize) -> usize {
  buffer[..cursor].rfind('\n').map_or(0, |index| index + 1)
}

fn line_end(buffer: &str, cursor: usize) -> usize {
  buffer[cursor..]
    .find('\n')
    .map_or(buffer.len(), |index| cursor + index)
}

fn delete_forward(buffer: &mut String, cursor: usize) -> bool {
  if cursor >= buffer.len() {
    return false;
  }
  let next = next_boundary(buffer, cursor);
  buffer.replace_range(cursor..next, "");
  true
}

fn is_motion_word(ch: char) -> bool {
  ch == '_' || ch == '$' || ch.is_ascii_alphanumeric()
}

fn move_word_forward(buffer: &str, mut cursor: usize) -> usize {
  while cursor < buffer.len() {
    let ch = buffer[cursor..].chars().next().unwrap();
    if is_motion_word(ch) {
      break;
    }
    cursor += ch.len_utf8();
  }
  while cursor < buffer.len() {
    let ch = buffer[cursor..].chars().next().unwrap();
    if !is_motion_word(ch) {
      break;
    }
    cursor += ch.len_utf8();
  }
  cursor
}

fn move_word_back(buffer: &str, mut cursor: usize) -> usize {
  while cursor > 0 {
    let prev = prev_boundary(buffer, cursor);
    let ch = buffer[prev..].chars().next().unwrap();
    if is_motion_word(ch) {
      break;
    }
    cursor = prev;
  }
  while cursor > 0 {
    let prev = prev_boundary(buffer, cursor);
    let ch = buffer[prev..].chars().next().unwrap();
    if !is_motion_word(ch) {
      break;
    }
    cursor = prev;
  }
  cursor
}

fn word_before(buffer: &str, cursor: usize) -> (usize, String) {
  let head = &buffer[..cursor];
  let mut start = cursor;
  for (index, ch) in head.char_indices().rev() {
    if is_word(ch) {
      start = index;
    } else {
      break;
    }
  }
  (start, head[start..].to_string())
}

fn is_word(ch: char) -> bool {
  ch == '.' || ch == '_' || ch == '$' || ch.is_ascii_alphanumeric()
}

pub fn is_ident_name(name: &str) -> bool {
  let mut chars = name.chars();
  let Some(first) = chars.next() else {
    return false;
  };
  (first == '_' || first == '$' || first.is_ascii_alphabetic())
    && chars.all(|ch| ch == '_' || ch == '$' || ch.is_ascii_alphanumeric())
}

fn utf8_width(byte: u8) -> usize {
  if byte & 0x80 == 0 {
    1
  } else if byte & 0xe0 == 0xc0 {
    2
  } else if byte & 0xf0 == 0xe0 {
    3
  } else if byte & 0xf8 == 0xf0 {
    4
  } else {
    1
  }
}

#[cfg(unix)]
fn fill_bytes(bytes: &mut std::collections::VecDeque<u8>) -> bool {
  let mut buf = [0u8; 64];
  let count = unsafe { libc::read(0, buf.as_mut_ptr().cast(), buf.len()) };
  if count <= 0 {
    return false;
  }
  bytes.extend(buf[..count as usize].iter().copied());
  true
}

#[cfg(unix)]
fn take_escape(bytes: &mut std::collections::VecDeque<u8>) -> String {
  if bytes.is_empty() {
    let mut poll = libc::pollfd {
      fd: 0,
      events: libc::POLLIN,
      revents: 0,
    };
    // A bare Esc must not wait forever. A following byte of a cursor or
    // Meta sequence can arrive in the next read, so wait long enough for
    // that byte under load.
    let ready = unsafe { libc::poll(&mut poll, 1, 200) };
    if ready <= 0 {
      return String::new();
    }
    if !fill_bytes(bytes) {
      return String::new();
    }
  }
  let Some(next) = bytes.pop_front() else {
    return String::new();
  };
  if next != b'[' && next != b'O' {
    if next == 0x7f || next == 0x08 || next.is_ascii_graphic() {
      return (next as char).to_string();
    }
    return String::new();
  }
  let mut seq = String::from(next as char);
  for _ in 0..8 {
    if bytes.is_empty() && !fill_bytes(bytes) {
      break;
    }
    let Some(byte) = bytes.pop_front() else {
      break;
    };
    seq.push(byte as char);
    if byte.is_ascii_alphabetic() || byte == b'~' {
      break;
    }
  }
  seq
}

#[cfg(unix)]
struct RawTerm {
  original: libc::termios,
}

#[cfg(unix)]
impl RawTerm {
  fn enter() -> Option<Self> {
    unsafe {
      let mut original = std::mem::zeroed::<libc::termios>();
      if libc::tcgetattr(0, &mut original) != 0 {
        return None;
      }
      let mut raw = original;
      raw.c_lflag &= !(libc::ICANON | libc::ECHO | libc::ISIG);
      raw.c_cc[libc::VMIN] = 1;
      raw.c_cc[libc::VTIME] = 0;
      if libc::tcsetattr(0, libc::TCSANOW, &raw) != 0 {
        return None;
      }
      Some(Self { original })
    }
  }
}

#[cfg(unix)]
impl Drop for RawTerm {
  fn drop(&mut self) {
    unsafe {
      libc::tcsetattr(0, libc::TCSANOW, &self.original);
    }
  }
}
