//! AV1 codec strings and `av1C` records.
//!
//! Demux keeps a 10-bit or 4:4:4 track. `configure` rejects it.

#[derive(Clone, Copy)]
struct Av1c {
  profile: u8,
  level: u8,
  tier_high: bool,
  bit_depth: u8,
  monochrome: bool,
  subsampling_x: bool,
  subsampling_y: bool,
}

struct CodecString {
  profile: u8,
  bit_depth: u8,
  monochrome: bool,
  /// `(x, y)` when the string includes chroma subsampling.
  subsampling: Option<(u8, u8)>,
}

/// `av01` and an 8-bit 4:2:0 string. A 10-bit, 12-bit, monochrome, or
/// non-4:2:0 string is not supported. A malformed string is not either.
pub(crate) fn string_supported(codec: &str) -> bool {
  let Some(parsed) = parse_string(codec) else {
    return false;
  };
  parsed.profile == 0
    && parsed.bit_depth == 8
    && !parsed.monochrome
    && parsed.subsampling.is_none_or(|(x, y)| x == 1 && y == 1)
}

/// An omitted or empty description is a raw OBU sequence. A present `av1C`
/// must be 8-bit 4:2:0.
pub(crate) fn description_supported(description: Option<&[u8]>) -> bool {
  match description {
    None | Some([]) => true,
    Some(bytes) => parse_av1c(bytes).is_some_and(decodable),
  }
}

pub(crate) fn codec_string(record: &[u8]) -> Option<String> {
  let parsed = parse_av1c(record)?;
  let tier = if parsed.tier_high { "H" } else { "M" };
  Some(format!(
    "av01.{}.{:02}{tier}.{:02}",
    parsed.profile, parsed.level, parsed.bit_depth
  ))
}

/// OBUs that follow a parsed `av1C` header. Empty when the record is only
/// the four-byte header.
pub(crate) fn config_obus(record: &[u8]) -> Option<&[u8]> {
  parse_av1c(record)?;
  Some(&record[4..])
}

fn decodable(record: Av1c) -> bool {
  record.profile == 0
    && record.bit_depth == 8
    && !record.monochrome
    && record.subsampling_x
    && record.subsampling_y
}

fn parse_av1c(bytes: &[u8]) -> Option<Av1c> {
  if bytes.len() < 4 || bytes[0] != 0x81 {
    return None;
  }
  let profile = bytes[1] >> 5;
  if profile > 2 {
    return None;
  }
  let high = bytes[2] & 0x40 != 0;
  let twelve = bytes[2] & 0x20 != 0;
  let bit_depth = match (high, twelve) {
    (false, false) => 8,
    (true, false) => 10,
    (true, true) => 12,
    (false, true) => return None,
  };
  Some(Av1c {
    profile,
    level: bytes[1] & 0x1f,
    tier_high: bytes[2] & 0x80 != 0,
    bit_depth,
    monochrome: bytes[2] & 0x10 != 0,
    subsampling_x: bytes[2] & 0x08 != 0,
    subsampling_y: bytes[2] & 0x04 != 0,
  })
}

fn parse_string(codec: &str) -> Option<CodecString> {
  if codec == "av01" {
    return Some(CodecString {
      profile: 0,
      bit_depth: 8,
      monochrome: false,
      subsampling: None,
    });
  }
  let rest = codec.strip_prefix("av01.")?;
  let parts: Vec<&str> = rest.split('.').collect();
  if parts.len() != 3 && parts.len() != 9 {
    return None;
  }
  let profile = one_digit(parts[0])?;
  if profile > 2 {
    return None;
  }
  let level_tier = parts[1].as_bytes();
  if level_tier.len() != 3
    || !level_tier[0].is_ascii_digit()
    || !level_tier[1].is_ascii_digit()
  {
    return None;
  }
  if level_tier[2] != b'M' && level_tier[2] != b'H' {
    return None;
  }
  let bit_depth = parts[2].parse::<u8>().ok()?;
  if bit_depth != 8 && bit_depth != 10 && bit_depth != 12 {
    return None;
  }
  if parts.len() == 3 {
    return Some(CodecString {
      profile,
      bit_depth,
      monochrome: false,
      subsampling: None,
    });
  }
  let monochrome = one_digit(parts[3])?;
  if monochrome > 1 {
    return None;
  }
  let chroma = parts[4].as_bytes();
  if chroma.len() != 3 || !chroma.iter().all(u8::is_ascii_digit) {
    return None;
  }
  let x = chroma[0] - b'0';
  let y = chroma[1] - b'0';
  let position = chroma[2] - b'0';
  if x > 1 || y > 1 || position > 3 {
    return None;
  }
  if two_digits(parts[5]).is_none()
    || two_digits(parts[6]).is_none()
    || two_digits(parts[7]).is_none()
  {
    return None;
  }
  let range = one_digit(parts[8])?;
  if range > 1 {
    return None;
  }
  Some(CodecString {
    profile,
    bit_depth,
    monochrome: monochrome == 1,
    subsampling: Some((x, y)),
  })
}

fn one_digit(text: &str) -> Option<u8> {
  let bytes = text.as_bytes();
  if bytes.len() == 1 && bytes[0].is_ascii_digit() {
    Some(bytes[0] - b'0')
  } else {
    None
  }
}

fn two_digits(text: &str) -> Option<u8> {
  let bytes = text.as_bytes();
  if bytes.len() == 2 && bytes.iter().all(u8::is_ascii_digit) {
    text.parse().ok()
  } else {
    None
  }
}

#[cfg(test)]
mod tests {
  use super::codec_string;
  use super::description_supported;
  use super::string_supported;

  #[test]
  fn eight_bit_420_strings_are_supported() {
    assert!(string_supported("av01"));
    assert!(string_supported("av01.0.05M.08"));
    assert!(string_supported("av01.0.08H.08"));
    assert!(string_supported("av01.0.05M.08.0.110.01.01.01.0"));
    assert!(string_supported("av01.0.05M.08.0.112.01.01.01.1"));
  }

  #[test]
  fn other_av1_strings_are_rejected() {
    for codec in [
      "av01.0.05M.10",
      "av01.0.05M.12",
      "av01.1.05M.08",
      "av01.2.05M.08",
      "av01.0.05M.08.1.110.01.01.01.0",
      "av01.0.05M.08.0.000.01.01.01.0",
      "av01.0.05M.08.0.100.01.01.01.0",
      "av01.0.05M.08.0.110",
      "av01.9.99M.08",
      "av010",
      "av01.",
    ] {
      assert!(!string_supported(codec), "{codec}");
    }
  }

  #[test]
  fn av1c_codec_string_and_gate() {
    let eight = [0x81, 0x00, 0x0c, 0x00];
    assert_eq!(codec_string(&eight).as_deref(), Some("av01.0.00M.08"));
    assert!(description_supported(None));
    assert!(description_supported(Some(&[])));
    assert!(description_supported(Some(&eight)));
    let ten = [0x81, 0x00, 0x4c, 0x00];
    assert_eq!(codec_string(&ten).as_deref(), Some("av01.0.00M.10"));
    assert!(!description_supported(Some(&ten)));
    assert!(codec_string(&[0x01, 0x00, 0x0c, 0x00]).is_none());
    assert!(!description_supported(Some(&[1, 2, 3, 4])));
  }
}
