//! Terminal line editor for `yuzuko repl`.
//!
//! A pipe reads whole lines. A Unix terminal and a Windows console handle the
//! Node.js readline emacs bindings in this process. The Windows console runs
//! with virtual terminal input, so keys arrive as the same escape sequences a
//! Unix terminal sends.

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
    pace: &mut dyn FnMut() -> Option<std::time::Duration>,
  ) -> Option<String> {
    if !io::stdin().is_terminal() {
      return read_pipe(pace);
    }
    #[cfg(any(unix, windows))]
    {
      let _raw = match term::RawTerm::enter() {
        Some(raw) => raw,
        None => return read_pipe(pace),
      };
      self.read_raw(complete, pace)
    }
    #[cfg(not(any(unix, windows)))]
    {
      let _ = complete;
      read_pipe(pace)
    }
  }

  #[cfg(any(unix, windows))]
  fn read_raw(
    &mut self,
    complete: &mut dyn FnMut(Option<&str>) -> Vec<String>,
    pace: &mut dyn FnMut() -> Option<std::time::Duration>,
  ) -> Option<String> {
    let mut buffer = String::new();
    let mut cursor = 0usize;
    let mut view = View::blank();
    prompt(buffer.is_empty());
    loop {
      if self.pending.is_empty()
        && !await_input(&mut self.pending, &mut || {
          crate::runtime::editor_hold_line(true);
          let wait = pace();
          crate::runtime::editor_hold_line(false);
          if crate::runtime::editor_line_broke() {
            view = View::blank();
            render(&mut view, &buffer, cursor);
          }
          wait
        })
      {
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
            if self.pending.is_empty() && !read_more(&mut self.pending, -1) {
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
  #[cfg(any(unix, windows))]
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

  #[cfg(any(unix, windows))]
  fn kill_backward_line(&mut self, buffer: &mut String, cursor: &mut usize) {
    let start = line_start(buffer, *cursor);
    self.kill(buffer, cursor, start, *cursor);
  }

  #[cfg(any(unix, windows))]
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

  #[cfg(any(unix, windows))]
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

  #[cfg(any(unix, windows))]
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

  #[cfg(any(unix, windows))]
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

fn read_pipe(
  pace: &mut dyn FnMut() -> Option<std::time::Duration>,
) -> Option<String> {
  #[cfg(unix)]
  {
    read_pipe_unix(pace)
  }
  #[cfg(not(unix))]
  {
    let _ = pace;
    read_pipe_blocking()
  }
}

#[cfg(unix)]
fn read_pipe_unix(
  pace: &mut dyn FnMut() -> Option<std::time::Duration>,
) -> Option<String> {
  read_pipe_lines(&mut || {
    await_readable(&mut || {
      crate::runtime::editor_hold_line(true);
      let wait = pace();
      crate::runtime::editor_hold_line(false);
      let _ = crate::runtime::editor_line_broke();
      wait
    })
  })
}

#[cfg(not(unix))]
fn read_pipe_blocking() -> Option<String> {
  read_pipe_lines(&mut || true)
}

fn read_pipe_lines(ready: &mut dyn FnMut() -> bool) -> Option<String> {
  let stdin = io::stdin();
  let mut block = String::new();
  let mut line = String::new();
  loop {
    prompt(block.is_empty());
    line.clear();
    if !ready() {
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
#[cfg(any(unix, windows))]
struct View {
  text: String,
  cursor: usize,
}

#[cfg(any(unix, windows))]
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
#[cfg(any(unix, windows))]
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

#[cfg(any(unix, windows))]
fn clear_screen(view: &mut View, buffer: &str, cursor: usize) {
  print!("\x1b[2J\x1b[H");
  let _ = io::stdout().flush();
  *view = View::blank();
  render(view, buffer, cursor);
}

#[cfg(any(unix, windows))]
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

#[cfg(any(unix, windows))]
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

#[cfg(any(unix, windows))]
fn prompt_columns(line: usize) -> usize {
  if line == 0 { 2 } else { 4 }
}

#[cfg(any(unix, windows))]
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

/// Waits until stdin can be read without consuming the bytes.
#[cfg(unix)]
fn await_readable(
  pace: &mut dyn FnMut() -> Option<std::time::Duration>,
) -> bool {
  loop {
    match term::poll_stdin(0) {
      StdinWait::Data => return true,
      StdinWait::Eof => return false,
      StdinWait::Timeout => {}
    }
    let wait = pace();
    match term::poll_stdin(timeout_ms(wait)) {
      StdinWait::Data => return true,
      StdinWait::Eof => return false,
      StdinWait::Timeout => {}
    }
  }
}

#[cfg(any(unix, windows))]
fn await_input(
  bytes: &mut std::collections::VecDeque<u8>,
  pace: &mut dyn FnMut() -> Option<std::time::Duration>,
) -> bool {
  loop {
    match term::poll_stdin(0) {
      StdinWait::Data => return term::read_into(bytes),
      StdinWait::Eof => return false,
      StdinWait::Timeout => {}
    }
    let wait = pace();
    match term::poll_stdin(0) {
      StdinWait::Data => return term::read_into(bytes),
      StdinWait::Eof => return false,
      StdinWait::Timeout => {}
    }
    match term::poll_stdin(timeout_ms(wait)) {
      StdinWait::Data => return term::read_into(bytes),
      StdinWait::Eof => return false,
      StdinWait::Timeout => {}
    }
  }
}

#[cfg(any(unix, windows))]
fn read_more(
  bytes: &mut std::collections::VecDeque<u8>,
  timeout_ms: i32,
) -> bool {
  match term::poll_stdin(timeout_ms) {
    StdinWait::Data => term::read_into(bytes),
    StdinWait::Timeout => false,
    StdinWait::Eof => false,
  }
}

#[cfg(any(unix, windows))]
enum StdinWait {
  Data,
  Timeout,
  Eof,
}

#[cfg(any(unix, windows))]
fn timeout_ms(wait: Option<std::time::Duration>) -> i32 {
  let Some(wait) = wait else {
    return -1;
  };
  if wait.is_zero() {
    return 0;
  }
  let ms = wait.as_millis();
  if ms == 0 { 1 } else { ms.min(60_000) as i32 }
}

#[cfg(any(unix, windows))]
fn take_escape(bytes: &mut std::collections::VecDeque<u8>) -> String {
  // A bare Esc must not wait forever. A following byte of a cursor or Meta
  // sequence can arrive in the next read, so wait long enough for that byte
  // under load.
  if bytes.is_empty() && !read_more(bytes, 200) {
    return String::new();
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
    if bytes.is_empty() && !read_more(bytes, 200) {
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

/// Terminal mode and key input on a Unix tty.
#[cfg(unix)]
mod term {
  use super::StdinWait;

  pub fn poll_stdin(timeout_ms: i32) -> StdinWait {
    loop {
      let mut poll = libc::pollfd {
        fd: 0,
        events: libc::POLLIN,
        revents: 0,
      };
      let ready = unsafe { libc::poll(&mut poll, 1, timeout_ms) };
      if ready < 0 {
        let err = std::io::Error::last_os_error();
        if err.kind() == std::io::ErrorKind::Interrupted {
          continue;
        }
        return StdinWait::Eof;
      }
      if ready == 0 {
        return StdinWait::Timeout;
      }
      // Linux reports a closed pipe as `POLLHUP` without `POLLIN`, while
      // lines may still sit in the `Stdin` buffer. The read then reports EOF
      // once those lines are gone.
      if poll.revents & (libc::POLLIN | libc::POLLHUP) != 0 {
        return StdinWait::Data;
      }
      return StdinWait::Eof;
    }
  }

  pub fn read_into(bytes: &mut std::collections::VecDeque<u8>) -> bool {
    let mut buf = [0u8; 64];
    loop {
      let count = unsafe { libc::read(0, buf.as_mut_ptr().cast(), buf.len()) };
      if count > 0 {
        bytes.extend(buf[..count as usize].iter().copied());
        return true;
      }
      if count == 0 {
        return false;
      }
      let err = std::io::Error::last_os_error();
      if err.kind() == std::io::ErrorKind::Interrupted {
        continue;
      }
      return false;
    }
  }

  pub struct RawTerm {
    original: libc::termios,
  }

  impl RawTerm {
    pub fn enter() -> Option<Self> {
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

  impl Drop for RawTerm {
    fn drop(&mut self) {
      unsafe {
        libc::tcsetattr(0, libc::TCSANOW, &self.original);
      }
    }
  }
}

/// Console mode and key input on Windows.
///
/// Raw mode turns off line input, echo, and Ctrl-C processing, and turns on
/// virtual terminal input. Each key then arrives as key-down records whose
/// characters are the bytes a Unix terminal would send, including `ESC [ A`
/// for Up and `0x03` for Ctrl-C. Output gets virtual terminal processing so
/// the redraw sequences move the cursor.
#[cfg(windows)]
mod term {
  use std::collections::VecDeque;
  use std::time::{Duration, Instant};

  use windows_sys::Win32::Foundation::{
    HANDLE, INVALID_HANDLE_VALUE, WAIT_OBJECT_0, WAIT_TIMEOUT,
  };
  use windows_sys::Win32::System::Console::{
    CONSOLE_MODE, ENABLE_ECHO_INPUT, ENABLE_LINE_INPUT, ENABLE_PROCESSED_INPUT,
    ENABLE_VIRTUAL_TERMINAL_INPUT, ENABLE_VIRTUAL_TERMINAL_PROCESSING,
    GetConsoleMode, GetStdHandle, INPUT_RECORD, KEY_EVENT, PeekConsoleInputW,
    ReadConsoleInputW, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, SetConsoleMode,
  };
  use windows_sys::Win32::System::Threading::WaitForSingleObject;

  use super::StdinWait;

  thread_local! {
    /// A high surrogate whose low half has not been read yet.
    static SURROGATE: std::cell::Cell<Option<u16>> =
      const { std::cell::Cell::new(None) };
  }

  fn input() -> HANDLE {
    unsafe { GetStdHandle(STD_INPUT_HANDLE) }
  }

  /// The UTF-16 unit a record contributes, or `None` for a record that
  /// carries no text: key-up, a bare modifier, focus, mouse, and resize.
  fn record_unit(record: &INPUT_RECORD) -> Option<u16> {
    if u32::from(record.EventType) != KEY_EVENT {
      return None;
    }
    let key = unsafe { &record.Event.KeyEvent };
    let unit = unsafe { key.uChar.UnicodeChar };
    // A character with no key of its own, such as one outside the Basic
    // Multilingual Plane, arrives on the key-up of Alt, as Alt+Numpad input
    // does. Each half of a surrogate pair is one such record.
    const VK_MENU: u16 = 0x12;
    let alt_text = key.bKeyDown == 0 && key.wVirtualKeyCode == VK_MENU;
    if unit == 0 || (key.bKeyDown == 0 && !alt_text) {
      return None;
    }
    Some(unit)
  }

  /// Drops records at the front of the queue that carry no text. Returns
  /// whether a text record is waiting, or `None` when the console fails.
  fn skip_silent(handle: HANDLE) -> Option<bool> {
    loop {
      let mut record = unsafe { std::mem::zeroed::<INPUT_RECORD>() };
      let mut count = 0u32;
      if unsafe { PeekConsoleInputW(handle, &mut record, 1, &mut count) } == 0 {
        return None;
      }
      if count == 0 {
        return Some(false);
      }
      if record_unit(&record).is_some() {
        return Some(true);
      }
      let mut read = 0u32;
      if unsafe { ReadConsoleInputW(handle, &mut record, 1, &mut read) } == 0 {
        return None;
      }
    }
  }

  pub fn poll_stdin(timeout_ms: i32) -> StdinWait {
    let handle = input();
    let deadline = u64::try_from(timeout_ms)
      .ok()
      .map(|ms| Instant::now() + Duration::from_millis(ms));
    loop {
      match skip_silent(handle) {
        Some(true) => return StdinWait::Data,
        Some(false) => {}
        None => return StdinWait::Eof,
      }
      // `u32::MAX` is INFINITE.
      let wait = match deadline {
        None => u32::MAX,
        Some(deadline) => {
          let left = deadline.saturating_duration_since(Instant::now());
          u32::try_from(left.as_millis()).unwrap_or(u32::MAX - 1)
        }
      };
      match unsafe { WaitForSingleObject(handle, wait) } {
        WAIT_OBJECT_0 => {}
        // Records that carry no text also signal the handle, so look once
        // more before reporting the timeout.
        WAIT_TIMEOUT => {
          return match skip_silent(handle) {
            Some(true) => StdinWait::Data,
            Some(false) => StdinWait::Timeout,
            None => StdinWait::Eof,
          };
        }
        _ => return StdinWait::Eof,
      }
    }
  }

  pub fn read_into(bytes: &mut VecDeque<u8>) -> bool {
    let handle = input();
    let mut records = [unsafe { std::mem::zeroed::<INPUT_RECORD>() }; 64];
    let mut count = 0u32;
    if unsafe {
      ReadConsoleInputW(
        handle,
        records.as_mut_ptr(),
        records.len() as u32,
        &mut count,
      )
    } == 0
    {
      return false;
    }
    let before = bytes.len();
    for record in &records[..count as usize] {
      let Some(unit) = record_unit(record) else {
        continue;
      };
      let repeat = unsafe { record.Event.KeyEvent.wRepeatCount }.max(1);
      for _ in 0..repeat {
        push_unit(bytes, unit);
      }
    }
    bytes.len() > before
  }

  fn push_unit(bytes: &mut VecDeque<u8>, unit: u16) {
    let high = SURROGATE.with(|slot| slot.take());
    let units = match high {
      Some(high) if (0xdc00..0xe000).contains(&unit) => vec![high, unit],
      _ if (0xd800..0xdc00).contains(&unit) => {
        SURROGATE.with(|slot| slot.set(Some(unit)));
        return;
      }
      _ => vec![unit],
    };
    let text = String::from_utf16_lossy(&units);
    bytes.extend(text.as_bytes());
  }

  pub struct RawTerm {
    input: CONSOLE_MODE,
    output: Option<CONSOLE_MODE>,
  }

  impl RawTerm {
    pub fn enter() -> Option<Self> {
      let handle = input();
      if handle.is_null() || handle == INVALID_HANDLE_VALUE {
        return None;
      }
      let mut original = 0;
      if unsafe { GetConsoleMode(handle, &mut original) } == 0 {
        return None;
      }
      let raw = (original
        & !(ENABLE_LINE_INPUT | ENABLE_ECHO_INPUT | ENABLE_PROCESSED_INPUT))
        | ENABLE_VIRTUAL_TERMINAL_INPUT;
      // A console host without virtual terminal input keeps the line reader.
      if unsafe { SetConsoleMode(handle, raw) } == 0 {
        return None;
      }
      let out = unsafe { GetStdHandle(STD_OUTPUT_HANDLE) };
      let mut output = None;
      let mut mode = 0;
      if unsafe { GetConsoleMode(out, &mut mode) } != 0 {
        if unsafe {
          SetConsoleMode(out, mode | ENABLE_VIRTUAL_TERMINAL_PROCESSING)
        } == 0
        {
          unsafe { SetConsoleMode(handle, original) };
          return None;
        }
        output = Some(mode);
      }
      Some(Self {
        input: original,
        output,
      })
    }
  }

  impl Drop for RawTerm {
    fn drop(&mut self) {
      unsafe {
        SetConsoleMode(input(), self.input);
        if let Some(mode) = self.output {
          SetConsoleMode(GetStdHandle(STD_OUTPUT_HANDLE), mode);
        }
      }
    }
  }
}
