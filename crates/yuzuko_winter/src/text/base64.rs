use yuzuko_core::op::OpError;
use yuzuko_macros::op;

const TABLE: &[u8] =
  b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub fn install(scope: &mut v8::PinScope) {
  let global = scope.get_current_context().global(scope);
  set_fn(scope, global, "btoa", btoa);
  set_fn(scope, global, "atob", atob);
}

fn set_fn(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
  callback: impl v8::MapFnTo<v8::FunctionCallback>,
) {
  let function = v8::Function::new(scope, callback).unwrap();
  let key = v8::String::new(scope, name).unwrap();
  object.set(scope, key.into(), function.into());
}

#[op]
fn btoa(input: String) -> Result<String, OpError> {
  encode_latin1(&input).map_err(|()| {
    OpError::dom(
      "InvalidCharacterError",
      "btoa failed: the string contains characters outside of the Latin1 range",
    )
  })
}

#[op]
fn atob(input: String) -> Result<String, OpError> {
  decode_forgiving(&input)
    .map(|decoded| decoded.iter().copied().map(char::from).collect())
    .map_err(|()| {
      OpError::dom(
        "InvalidCharacterError",
        "atob failed: the string is not correctly encoded",
      )
    })
}

fn encode_latin1(input: &str) -> Result<String, ()> {
  let mut bytes = Vec::with_capacity(input.len());
  for ch in input.chars() {
    let code = u32::from(ch);
    if code > 0xFF {
      return Err(());
    }
    bytes.push(code as u8);
  }
  Ok(encode(&bytes))
}

fn encode(bytes: &[u8]) -> String {
  let mut out = String::new();
  let mut index = 0;
  while index + 3 <= bytes.len() {
    push_quantum(
      &mut out,
      bytes[index],
      bytes[index + 1],
      bytes[index + 2],
      3,
    );
    index += 3;
  }
  let rest = &bytes[index..];
  if rest.len() == 1 {
    push_quantum(&mut out, rest[0], 0, 0, 1);
  } else if rest.len() == 2 {
    push_quantum(&mut out, rest[0], rest[1], 0, 2);
  }
  out
}

fn push_quantum(out: &mut String, a: u8, b: u8, c: u8, count: usize) {
  let value = ((a as u32) << 16) | ((b as u32) << 8) | c as u32;
  out.push(TABLE[((value >> 18) & 63) as usize] as char);
  out.push(TABLE[((value >> 12) & 63) as usize] as char);
  if count == 1 {
    out.push('=');
    out.push('=');
  } else if count == 2 {
    out.push(TABLE[((value >> 6) & 63) as usize] as char);
    out.push('=');
  } else {
    out.push(TABLE[((value >> 6) & 63) as usize] as char);
    out.push(TABLE[(value & 63) as usize] as char);
  }
}

pub(crate) fn decode_forgiving(input: &str) -> Result<Vec<u8>, ()> {
  let mut filtered: String = input
    .chars()
    .filter(|ch| !ch.is_ascii_whitespace())
    .collect();
  // Padding is removed only when the length is a multiple of four. A stray
  // "=" such as "ab=" stays and fails the alphabet check below.
  if filtered.len().is_multiple_of(4) {
    if filtered.ends_with("==") {
      filtered.truncate(filtered.len() - 2);
    } else if filtered.ends_with('=') {
      filtered.truncate(filtered.len() - 1);
    }
  }
  if filtered.is_empty() {
    return Ok(Vec::new());
  }
  if filtered.len() % 4 == 1 {
    return Err(());
  }
  let mut out = Vec::new();
  let mut buffer: u32 = 0;
  let mut bits = 0u32;
  for byte in filtered.bytes() {
    let value = u32::from(decode_digit(byte)?);
    buffer = (buffer << 6) | value;
    bits += 6;
    if bits >= 8 {
      bits -= 8;
      out.push((buffer >> bits) as u8);
      if bits == 0 {
        buffer = 0;
      } else {
        buffer &= (1 << bits) - 1;
      }
    }
  }
  Ok(out)
}

fn decode_digit(byte: u8) -> Result<u8, ()> {
  match byte {
    b'A'..=b'Z' => Ok(byte - b'A'),
    b'a'..=b'z' => Ok(byte - b'a' + 26),
    b'0'..=b'9' => Ok(byte - b'0' + 52),
    b'+' => Ok(62),
    b'/' => Ok(63),
    _ => Err(()),
  }
}

const URL_TABLE: &[u8] =
  b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

/// Base64url without padding, as used by JSON Web Key.
pub(crate) fn encode_url(bytes: &[u8]) -> String {
  let mut out = String::new();
  let mut index = 0;
  while index + 3 <= bytes.len() {
    push_url(
      &mut out,
      bytes[index],
      bytes[index + 1],
      bytes[index + 2],
      3,
    );
    index += 3;
  }
  let rest = &bytes[index..];
  if rest.len() == 1 {
    push_url(&mut out, rest[0], 0, 0, 1);
  } else if rest.len() == 2 {
    push_url(&mut out, rest[0], rest[1], 0, 2);
  }
  out
}

fn push_url(out: &mut String, a: u8, b: u8, c: u8, count: usize) {
  let value = ((a as u32) << 16) | ((b as u32) << 8) | c as u32;
  out.push(URL_TABLE[((value >> 18) & 63) as usize] as char);
  out.push(URL_TABLE[((value >> 12) & 63) as usize] as char);
  if count > 1 {
    out.push(URL_TABLE[((value >> 6) & 63) as usize] as char);
  }
  if count > 2 {
    out.push(URL_TABLE[(value & 63) as usize] as char);
  }
}

pub(crate) fn decode_url(input: &str) -> Result<Vec<u8>, ()> {
  let mut filtered = input.to_string();
  if filtered.len().is_multiple_of(4) {
    if filtered.ends_with("==") {
      filtered.truncate(filtered.len() - 2);
    } else if filtered.ends_with('=') {
      filtered.truncate(filtered.len() - 1);
    }
  }
  if filtered.is_empty() {
    return Ok(Vec::new());
  }
  if filtered.len() % 4 == 1 {
    return Err(());
  }
  let mut out = Vec::new();
  let mut buffer: u32 = 0;
  let mut bits = 0u32;
  for byte in filtered.bytes() {
    let value = u32::from(decode_url_digit(byte)?);
    buffer = (buffer << 6) | value;
    bits += 6;
    if bits >= 8 {
      bits -= 8;
      out.push((buffer >> bits) as u8);
      if bits == 0 {
        buffer = 0;
      } else {
        buffer &= (1 << bits) - 1;
      }
    }
  }
  Ok(out)
}

fn decode_url_digit(byte: u8) -> Result<u8, ()> {
  match byte {
    b'A'..=b'Z' => Ok(byte - b'A'),
    b'a'..=b'z' => Ok(byte - b'a' + 26),
    b'0'..=b'9' => Ok(byte - b'0' + 52),
    b'-' => Ok(62),
    b'_' => Ok(63),
    _ => Err(()),
  }
}

#[cfg(test)]
mod tests {
  use super::decode_url;
  use super::encode_url;

  #[test]
  fn url_roundtrip_omits_padding() {
    let bytes = [0u8, 1, 2, 255, 254];
    let encoded = encode_url(&bytes);
    assert!(!encoded.contains(['+', '/', '=']));
    assert_eq!(decode_url(&encoded).unwrap(), bytes);
    assert!(decode_url("ab+c").is_err());
    assert_eq!(decode_url(&format!("{encoded}=")).unwrap(), bytes);
  }
}
