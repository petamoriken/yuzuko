//! HTTP `Content-Encoding` decoders for fetch response bodies.
//!
//! Supported codings are `gzip`, `x-gzip`, `deflate`, `br`, and `zstd`.
//! A zstd window above 8 MiB is rejected. Browsers refuse that window, and
//! the large-window fixture expects the body read to fail.

use std::io::Write;

use flate2::write::GzDecoder;
use flate2::write::ZlibDecoder;

const ZSTD_MAX_WINDOW: u64 = 8 * 1024 * 1024;

enum BrotliDec {
  Open(Box<brotli::DecompressorWriter<Vec<u8>>>),
  Done(Vec<u8>),
}

enum Stage {
  Gzip {
    decoder: GzDecoder<Vec<u8>>,
    emitted: usize,
  },
  Zlib {
    decoder: ZlibDecoder<Vec<u8>>,
    emitted: usize,
  },
  Brotli {
    inner: BrotliDec,
    emitted: usize,
  },
  Zstd {
    compressed: Vec<u8>,
  },
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
        "deflate" => Stage::Zlib {
          decoder: ZlibDecoder::new(Vec::new()),
          emitted: 0,
        },
        "br" => Stage::Brotli {
          inner: BrotliDec::Open(Box::new(brotli::DecompressorWriter::new(
            Vec::new(),
            4096,
          ))),
          emitted: 0,
        },
        "zstd" => Stage::Zstd {
          compressed: Vec::new(),
        },
        _ => Stage::Gzip {
          decoder: GzDecoder::new(Vec::new()),
          emitted: 0,
        },
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
      Self::Gzip { decoder, emitted } => {
        write_flate(decoder, emitted, input, finish)
      }
      Self::Zlib { decoder, emitted } => {
        write_flate(decoder, emitted, input, finish)
      }
      Self::Brotli { inner, emitted } => {
        feed_brotli(inner, emitted, input, finish)
      }
      Self::Zstd { compressed } => {
        compressed.extend_from_slice(input);
        if !finish {
          return Ok(Vec::new());
        }
        decode_zstd(compressed)
      }
    }
  }
}

fn write_flate<D>(
  decoder: &mut D,
  emitted: &mut usize,
  input: &[u8],
  finish: bool,
) -> Result<Vec<u8>, String>
where
  D: Write + FlateBuf,
{
  if !input.is_empty() {
    decoder
      .write_all(input)
      .map_err(|_| "The content encoding is invalid".to_string())?;
    decoder
      .flush()
      .map_err(|_| "The content encoding is invalid".to_string())?;
  }
  if finish {
    decoder
      .try_finish()
      .map_err(|_| "The content encoding is invalid".to_string())?;
  }
  Ok(take_new(decoder.buf(), emitted))
}

trait FlateBuf {
  fn buf(&mut self) -> &[u8];
  fn try_finish(&mut self) -> std::io::Result<()>;
}

impl FlateBuf for GzDecoder<Vec<u8>> {
  fn buf(&mut self) -> &[u8] {
    self.get_mut()
  }

  fn try_finish(&mut self) -> std::io::Result<()> {
    GzDecoder::try_finish(self)
  }
}

impl FlateBuf for ZlibDecoder<Vec<u8>> {
  fn buf(&mut self) -> &[u8] {
    self.get_mut()
  }

  fn try_finish(&mut self) -> std::io::Result<()> {
    ZlibDecoder::try_finish(self)
  }
}

fn feed_brotli(
  inner: &mut BrotliDec,
  emitted: &mut usize,
  input: &[u8],
  finish: bool,
) -> Result<Vec<u8>, String> {
  if let BrotliDec::Open(decoder) = inner {
    if !input.is_empty() {
      decoder
        .write_all(input)
        .map_err(|_| "The content encoding is invalid".to_string())?;
    }
    if finish {
      decoder
        .flush()
        .map_err(|_| "The content encoding is invalid".to_string())?;
    }
  }
  if finish && matches!(inner, BrotliDec::Open(_)) {
    let BrotliDec::Open(decoder) =
      std::mem::replace(inner, BrotliDec::Done(Vec::new()))
    else {
      return Err("The content encoding is invalid".to_string());
    };
    match decoder.into_inner() {
      Ok(bytes) => *inner = BrotliDec::Done(bytes),
      Err(_) => return Err("The content encoding is invalid".to_string()),
    }
  }
  let buf: &[u8] = match inner {
    BrotliDec::Open(decoder) => decoder.get_mut(),
    BrotliDec::Done(bytes) => bytes,
  };
  Ok(take_new(buf, emitted))
}

fn decode_zstd(compressed: &[u8]) -> Result<Vec<u8>, String> {
  use std::io::Read;
  let cursor = std::io::Cursor::new(compressed);
  let mut decoder =
    ruzstd::decoding::StreamingDecoder::new_with_max_window_size(
      cursor,
      ZSTD_MAX_WINDOW,
    )
    .map_err(|_| "The content encoding is invalid".to_string())?;
  let mut output = Vec::new();
  decoder
    .read_to_end(&mut output)
    .map_err(|_| "The content encoding is invalid".to_string())?;
  Ok(output)
}

fn take_new(buf: &[u8], emitted: &mut usize) -> Vec<u8> {
  let start = (*emitted).min(buf.len());
  let output = buf[start..].to_vec();
  *emitted = buf.len();
  output
}

#[cfg(test)]
mod tests {
  use super::prepare;
  use flate2::Compression;
  use flate2::write::GzEncoder;
  use std::io::Write;

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
}
