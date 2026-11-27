//! HTTP `Content-Encoding` decoders for fetch response bodies.
//!
//! Supported codings are `gzip`, `x-gzip`, `deflate`, `br`, and `zstd`.
//! A zstd window above 8 MiB is rejected. Browsers refuse that window, and
//! the large-window fixture expects the body read to fail.
//!
//! Each push hands its decoded bytes to the caller and keeps none of them.
//! One push may decode at most `STEP_LIMIT` bytes per coding, so a small
//! compressed input cannot expand without bound before the caller sees it.
//! zstd decodes once the body ends, so its compressed input and its output
//! are limited the same way.

use std::io::Write;

use flate2::write::GzDecoder;
use flate2::write::ZlibDecoder;

const ZSTD_MAX_WINDOW: u64 = 8 * 1024 * 1024;

/// The most bytes one push may decode in one coding.
const STEP_LIMIT: usize = 128 * 1024 * 1024;

/// Compressed input is written in slices this long, so `STEP_LIMIT` is
/// checked before a slice can expand much past it.
const SLICE: usize = 16 * 1024;

/// The most codings one response may stack. Each one holds a decoder's
/// state from the start, so a long `gzip,gzip,...` header must not build one
/// per item. A longer list is left undecoded, like an unknown coding.
const MAX_CODINGS: usize = 8;

const INVALID: &str = "The content encoding is invalid";
const TOO_LARGE: &str = "The decoded body is too large";

enum Stage {
  Gzip(GzDecoder<Vec<u8>>),
  Zlib(ZlibDecoder<Vec<u8>>),
  Brotli(Option<Box<brotli::DecompressorWriter<Vec<u8>>>>),
  Zstd { compressed: Vec<u8> },
}

pub struct ContentDecoder {
  stages: Vec<Stage>,
}

pub fn prepare(headers: &mut Vec<(String, String)>) -> Option<ContentDecoder> {
  let mut codings = Vec::new();
  for (name, value) in headers.iter() {
    if name != "content-encoding" {
      continue;
    }
    for part in value.split(',') {
      let part = part.trim();
      if part.is_empty() || part.eq_ignore_ascii_case("identity") {
        continue;
      }
      if codings.len() == MAX_CODINGS {
        return None;
      }
      codings.push(part.to_ascii_lowercase());
    }
  }
  if codings.is_empty() || codings.iter().any(|coding| !known(coding)) {
    return None;
  }
  headers
    .retain(|(name, _)| name != "content-encoding" && name != "content-length");
  // The last coding was applied last, so it is the outer wrapper.
  codings.reverse();
  Some(ContentDecoder::new(codings))
}

fn known(coding: &str) -> bool {
  matches!(coding, "gzip" | "x-gzip" | "deflate" | "br" | "zstd")
}

impl ContentDecoder {
  fn new(codings: Vec<String>) -> Self {
    let stages = codings
      .into_iter()
      .map(|coding| match coding.as_str() {
        "deflate" => Stage::Zlib(ZlibDecoder::new(Vec::new())),
        "br" => Stage::Brotli(Some(Box::new(brotli::DecompressorWriter::new(
          Vec::new(),
          4096,
        )))),
        "zstd" => Stage::Zstd {
          compressed: Vec::new(),
        },
        _ => Stage::Gzip(GzDecoder::new(Vec::new())),
      })
      .collect();
    Self { stages }
  }

  pub fn push(
    &mut self,
    input: &[u8],
    finish: bool,
  ) -> Result<Vec<u8>, String> {
    let mut current = input.to_vec();
    for stage in &mut self.stages {
      current = stage.feed(&current, finish)?;
    }
    Ok(current)
  }

  pub fn finish(&mut self, input: &[u8]) -> Result<Vec<u8>, String> {
    let mut output = self.push(input, false)?;
    output.extend(self.push(&[], true)?);
    Ok(output)
  }
}

impl Stage {
  fn feed(&mut self, input: &[u8], finish: bool) -> Result<Vec<u8>, String> {
    match self {
      Self::Gzip(decoder) => write_flate(decoder, input, finish),
      Self::Zlib(decoder) => write_flate(decoder, input, finish),
      Self::Brotli(decoder) => feed_brotli(decoder, input, finish),
      Self::Zstd { compressed } => {
        if compressed.len() + input.len() > STEP_LIMIT {
          return Err(TOO_LARGE.to_string());
        }
        compressed.extend_from_slice(input);
        if !finish {
          return Ok(Vec::new());
        }
        decode_zstd(&std::mem::take(compressed))
      }
    }
  }
}

/// Moves the decoder's new output into `output`, failing past `STEP_LIMIT`.
fn collect(buf: &mut Vec<u8>, output: &mut Vec<u8>) -> Result<(), String> {
  if output.len() + buf.len() > STEP_LIMIT {
    return Err(TOO_LARGE.to_string());
  }
  output.append(buf);
  Ok(())
}

fn write_flate<D>(
  decoder: &mut D,
  input: &[u8],
  finish: bool,
) -> Result<Vec<u8>, String>
where
  D: Write + FlateBuf,
{
  let mut output = Vec::new();
  for slice in input.chunks(SLICE) {
    decoder.write_all(slice).map_err(|_| INVALID.to_string())?;
    decoder.flush().map_err(|_| INVALID.to_string())?;
    collect(decoder.buf(), &mut output)?;
  }
  if finish {
    decoder.try_finish().map_err(|_| INVALID.to_string())?;
    collect(decoder.buf(), &mut output)?;
  }
  Ok(output)
}

trait FlateBuf {
  fn buf(&mut self) -> &mut Vec<u8>;
  fn try_finish(&mut self) -> std::io::Result<()>;
}

impl FlateBuf for GzDecoder<Vec<u8>> {
  fn buf(&mut self) -> &mut Vec<u8> {
    self.get_mut()
  }

  fn try_finish(&mut self) -> std::io::Result<()> {
    GzDecoder::try_finish(self)
  }
}

impl FlateBuf for ZlibDecoder<Vec<u8>> {
  fn buf(&mut self) -> &mut Vec<u8> {
    self.get_mut()
  }

  fn try_finish(&mut self) -> std::io::Result<()> {
    ZlibDecoder::try_finish(self)
  }
}

fn feed_brotli(
  slot: &mut Option<Box<brotli::DecompressorWriter<Vec<u8>>>>,
  input: &[u8],
  finish: bool,
) -> Result<Vec<u8>, String> {
  let mut output = Vec::new();
  let Some(decoder) = slot else {
    if input.is_empty() {
      return Ok(output);
    }
    return Err(INVALID.to_string());
  };
  for slice in input.chunks(SLICE) {
    decoder.write_all(slice).map_err(|_| INVALID.to_string())?;
    collect(decoder.get_mut(), &mut output)?;
  }
  if finish {
    decoder.flush().map_err(|_| INVALID.to_string())?;
    collect(decoder.get_mut(), &mut output)?;
    let Some(decoder) = slot.take() else {
      return Err(INVALID.to_string());
    };
    let mut rest = decoder.into_inner().map_err(|_| INVALID.to_string())?;
    collect(&mut rest, &mut output)?;
  }
  Ok(output)
}

fn decode_zstd(compressed: &[u8]) -> Result<Vec<u8>, String> {
  use std::io::Read;
  let cursor = std::io::Cursor::new(compressed);
  let decoder = ruzstd::decoding::StreamingDecoder::new_with_max_window_size(
    cursor,
    ZSTD_MAX_WINDOW,
  )
  .map_err(|_| INVALID.to_string())?;
  let mut output = Vec::new();
  decoder
    .take(STEP_LIMIT as u64 + 1)
    .read_to_end(&mut output)
    .map_err(|_| INVALID.to_string())?;
  if output.len() > STEP_LIMIT {
    return Err(TOO_LARGE.to_string());
  }
  Ok(output)
}

#[cfg(test)]
mod tests {
  use super::{STEP_LIMIT, TOO_LARGE, prepare};
  use flate2::Compression;
  use flate2::write::GzEncoder;
  use std::io::Write;

  #[test]
  fn a_long_coding_list_is_left_undecoded() {
    let stacked = |count: usize| {
      let mut headers = vec![(
        "content-encoding".to_string(),
        vec!["gzip"; count].join(","),
      )];
      let decoded = prepare(&mut headers).is_some();
      (decoded, headers.len())
    };
    assert_eq!(stacked(super::MAX_CODINGS), (true, 0));
    // The header stays, and no decoder state is built for any item.
    assert_eq!(stacked(super::MAX_CODINGS + 1), (false, 1));
    assert_eq!(stacked(200_000), (false, 1));
  }

  #[test]
  fn gzip_round_trip() {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(b"hello content encoding").unwrap();
    let compressed = encoder.finish().unwrap();
    let mut headers = vec![
      ("content-type".to_string(), "text/plain".to_string()),
      ("content-encoding".to_string(), "gzip".to_string()),
      ("content-length".to_string(), compressed.len().to_string()),
    ];
    let mut decoder = prepare(&mut headers).unwrap();
    assert!(headers.iter().all(|(name, _)| name == "content-type"));
    let output = decoder.finish(&compressed).unwrap();
    assert_eq!(output, b"hello content encoding");
  }
  fn gzip(bytes: &[u8]) -> Vec<u8> {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::fast());
    encoder.write_all(bytes).unwrap();
    encoder.finish().unwrap()
  }

  fn decoder(coding: &str) -> super::ContentDecoder {
    let mut headers =
      vec![("content-encoding".to_string(), coding.to_string())];
    prepare(&mut headers).unwrap()
  }

  #[test]
  fn decoded_output_is_not_kept() {
    let compressed = gzip(&vec![7u8; 4 * 1024 * 1024]);
    let mut decoder = decoder("gzip");
    let mut total = 0;
    for piece in compressed.chunks(1024) {
      total += decoder.push(piece, false).unwrap().len();
      if let super::Stage::Gzip(inner) = &mut decoder.stages[0] {
        assert!(inner.get_mut().is_empty(), "decoded bytes were kept");
      }
    }
    total += decoder.push(&[], true).unwrap().len();
    assert_eq!(total, 4 * 1024 * 1024);
  }

  #[test]
  fn one_push_cannot_expand_past_the_step_limit() {
    // Nested gzip multiplies the ratio, so a few kilobytes name gigabytes.
    let inner = gzip(&vec![0u8; STEP_LIMIT + 1]);
    let outer = gzip(&inner);
    assert!(outer.len() < STEP_LIMIT / 1024);
    let error = decoder("gzip, gzip").push(&outer, true).unwrap_err();
    assert_eq!(error, TOO_LARGE);
  }
}
