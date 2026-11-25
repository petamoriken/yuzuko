//! URL percent-decoding, shared by data URLs, form bodies, and URL
//! credentials.

/// URL Standard percent-decode. Each `%` followed by two ASCII hex digits
/// becomes that byte. Every other byte, including a `%` without two hex
/// digits after it, is kept.
pub fn decode(input: &str) -> Vec<u8> {
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

#[inline]
fn hex_digit(byte: u8) -> Option<u8> {
  match byte {
    b'0'..=b'9' => Some(byte - b'0'),
    b'a'..=b'f' => Some(byte - b'a' + 10),
    b'A'..=b'F' => Some(byte - b'A' + 10),
    _ => None,
  }
}

#[cfg(test)]
mod tests {
  use super::decode;

  #[test]
  fn valid_escapes_decode() {
    assert_eq!(decode("a%41%2f%2F"), b"aA//");
    assert_eq!(decode("%FF"), [0xFF]);
  }

  #[test]
  fn invalid_escapes_are_kept() {
    assert_eq!(decode("%zz%4%"), b"%zz%4%");
    assert_eq!(decode("%+1"), b"%+1");
    assert_eq!(decode("%\u{e9}1"), "%\u{e9}1".as_bytes());
  }
}
