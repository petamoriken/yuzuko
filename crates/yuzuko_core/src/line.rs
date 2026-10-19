//! Terminal line editor for `yuzuko repl`.
//!
//! A pipe reads whole lines. A Unix terminal handles Tab and Up/Down in this
//! process. Windows console input stays line-oriented.

use std::io::{self, IsTerminal, Write};

use crate::repl;

pub struct Editor {
  history: Vec<String>,
  nav: Option<usize>,
  draft: String,
  cancel_armed: bool,
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
            continue;
          }
          if repl::input_is_complete(&buffer) {
            println!();
            self.push_history(&buffer);
            return Some(std::mem::take(&mut buffer));
          }
          insert(&mut buffer, &mut cursor, "\n");
          print!("\n... ");
          let _ = io::stdout().flush();
        }
        0x04 => {
          if buffer.is_empty() {
            println!();
            return None;
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
        }
        0x7f | 0x08 => {
          if cursor == 0 {
            continue;
          }
          let prev = prev_boundary(&buffer, cursor);
          let removed = buffer[prev..cursor].to_string();
          buffer.replace_range(prev..cursor, "");
          cursor = prev;
          if removed == "\n" || cursor != buffer.len() {
            repaint(&buffer, cursor);
          } else {
            print!("\u{8} \u{8}");
            let _ = io::stdout().flush();
          }
        }
        0x1b => {
          let seq = take_escape(&mut self.pending);
          self.arrow(&seq, &mut buffer, &mut cursor);
        }
        b'\t' => {
          self.complete_at(complete, &mut buffer, &mut cursor);
        }
        byte if (0x20..0x7f).contains(&byte) => {
          insert(&mut buffer, &mut cursor, &(byte as char).to_string());
          if cursor == buffer.len() {
            print!("{}", byte as char);
            let _ = io::stdout().flush();
          } else {
            repaint(&buffer, cursor);
          }
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
          if cursor == buffer.len() {
            print!("{text}");
            let _ = io::stdout().flush();
          } else {
            repaint(&buffer, cursor);
          }
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

  #[cfg(unix)]
  fn arrow(&mut self, seq: &str, buffer: &mut String, cursor: &mut usize) {
    match seq {
      "[A" => self.history_step(-1, buffer, cursor),
      "[B" => self.history_step(1, buffer, cursor),
      "[C" if *cursor < buffer.len() => {
        *cursor = next_boundary(buffer, *cursor);
        repaint(buffer, *cursor);
      }
      "[D" if *cursor > 0 => {
        *cursor = prev_boundary(buffer, *cursor);
        repaint(buffer, *cursor);
      }
      _ => {}
    }
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
    repaint(buffer, *cursor);
  }

  #[cfg(unix)]
  fn complete_at(
    &mut self,
    complete: &mut dyn FnMut(Option<&str>) -> Vec<String>,
    buffer: &mut String,
    cursor: &mut usize,
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
      repaint(buffer, *cursor);
      return;
    }
    println!();
    println!("{}", names.join("  "));
    repaint(buffer, *cursor);
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

fn repaint(buffer: &str, cursor: usize) {
  let above = buffer[..cursor].chars().filter(|ch| *ch == '\n').count();
  if above > 0 {
    print!("\x1b[{above}A");
  }
  print!("\r\x1b[J");
  let mut lines = buffer.split('\n');
  if let Some(first) = lines.next() {
    print!("> {first}");
  } else {
    print!("> ");
  }
  for line in lines {
    print!("\n... {line}");
  }
  let _ = io::stdout().flush();
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
    let ready = unsafe { libc::poll(&mut poll, 1, 30) };
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
  if next != b'[' {
    return String::new();
  }
  let mut seq = String::from("[");
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
