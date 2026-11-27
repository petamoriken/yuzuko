//! URL percent-decoding, shared by data URLs, form bodies, and URL
//! credentials.

/// URL Standard percent-decode. Each `%` followed by two ASCII hex digits
/// becomes that byte. Every other byte, including a `%` without two hex
/// digits after it, is kept.
pub fn decode(input: &str) -> Vec<u8> {
  percent_encoding::percent_decode_str(input).collect()
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
