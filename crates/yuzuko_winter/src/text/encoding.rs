//! `TextEncoder` and `TextDecoder`. The decoder keeps its streaming state
//! in the cppgc object.

use std::cell::{Cell, RefCell};

use yuzuko_core::op::Bytes;
use yuzuko_core::op::OpError;
use yuzuko_macros::op;

use crate::bytes;

pub fn install(scope: &mut v8::PinScope) {
  TextEncoder::install(scope);
  TextDecoder::install(scope);
}

pub struct TextEncoder;

unsafe impl v8::cppgc::GarbageCollected for TextEncoder {
  fn trace(&self, _visitor: &mut v8::cppgc::Visitor) {}

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"TextEncoder"
  }
}

#[op]
impl TextEncoder {
  #[constructor]
  fn new() -> TextEncoder {
    TextEncoder
  }

  #[getter]
  fn encoding(&self) -> String {
    "utf-8".to_string()
  }

  fn encode(&self, input: Option<String>) -> Bytes {
    Bytes(input.unwrap_or_default().into_bytes())
  }

  #[rename("encodeInto")]
  fn encode_into(
    &self,
    source: String,
    destination: v8::Local<v8::Uint8Array>,
  ) -> EncodeIntoResult {
    let mut encoded = Vec::new();
    let mut read = 0usize;
    for ch in source.chars() {
      let mut buf = [0u8; 4];
      let piece = ch.encode_utf8(&mut buf);
      if encoded.len() + piece.len() > destination.byte_length() {
        break;
      }
      encoded.extend_from_slice(piece.as_bytes());
      read += ch.len_utf16();
    }
    let written = encoded.len();
    let ptr = destination.data();
    if !ptr.is_null() {
      unsafe {
        std::ptr::copy_nonoverlapping(encoded.as_ptr(), ptr.cast(), written);
      }
    }
    EncodeIntoResult { read, written }
  }
}

struct EncodeIntoResult {
  read: usize,
  written: usize,
}

impl yuzuko_core::op::ToV8 for EncodeIntoResult {
  fn to_v8(self, scope: &mut v8::PinScope, ret: &mut v8::ReturnValue) {
    let object = v8::Object::new(scope);
    let read = v8::String::new(scope, "read").unwrap();
    let written = v8::String::new(scope, "written").unwrap();
    object.set(
      scope,
      read.into(),
      v8::Integer::new(scope, self.read as i32).into(),
    );
    object.set(
      scope,
      written.into(),
      v8::Integer::new(scope, self.written as i32).into(),
    );
    ret.set(object.into());
  }
}

pub struct TextDecoder {
  encoding: &'static encoding_rs::Encoding,
  fatal: bool,
  ignore_bom: bool,
  decoder: RefCell<encoding_rs::Decoder>,
  /// Bytes the decoder pushed back before a fatal error (`read == 0`).
  pending: RefCell<Vec<u8>>,
  /// The previous `decode()` kept the decoder (`stream: true`).
  do_not_flush: Cell<bool>,
}

unsafe impl v8::cppgc::GarbageCollected for TextDecoder {
  fn trace(&self, _visitor: &mut v8::cppgc::Visitor) {}

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"TextDecoder"
  }
}

#[op]
impl TextDecoder {
  #[constructor]
  fn new(
    scope: &mut v8::PinScope,
    label: Option<String>,
    options: Option<v8::Local<v8::Value>>,
  ) -> Result<TextDecoder, OpError> {
    let label = label.unwrap_or_else(|| "utf-8".to_string());
    // Replacement labels are labels, but TextDecoder rejects that encoding.
    let Some(encoding) =
      encoding_rs::Encoding::for_label_no_replacement(label.as_bytes())
    else {
      return Err(OpError::range("Unknown encoding"));
    };
    let (fatal, ignore_bom) = match options {
      Some(options) => decoder_options(scope, options),
      None => (false, false),
    };
    let decoder = open_decoder(encoding, ignore_bom);
    Ok(TextDecoder {
      encoding,
      fatal,
      ignore_bom,
      decoder: RefCell::new(decoder),
      pending: RefCell::new(Vec::new()),
      do_not_flush: Cell::new(false),
    })
  }

  #[getter]
  fn encoding(&self) -> String {
    // The Encoding API returns the name ASCII-lowercased.
    self.encoding.name().to_ascii_lowercase()
  }

  #[getter]
  fn fatal(&self) -> bool {
    self.fatal
  }

  #[getter]
  #[rename("ignoreBOM")]
  fn ignore_bom(&self) -> bool {
    self.ignore_bom
  }

  fn decode(
    &self,
    scope: &mut v8::PinScope,
    input: Option<v8::Local<v8::Value>>,
    options: Option<v8::Local<v8::Value>>,
  ) -> Result<String, OpError> {
    // Dictionary conversion runs before the bytes are copied. A `stream`
    // getter can detach the input, and that detached buffer decodes as empty.
    let stream = options.is_some_and(|options| stream_option(scope, options));
    let bytes = match input {
      Some(value) => bytes::buffer_source(scope, value).unwrap_or_default(),
      None => Vec::new(),
    };
    // Reset when the previous call flushed. `stream` on this call is stored
    // for the next one.
    if !self.do_not_flush.get() {
      *self.decoder.borrow_mut() = open_decoder(self.encoding, self.ignore_bom);
      self.pending.borrow_mut().clear();
    }
    self.do_not_flush.set(stream);
    if self.fatal {
      return fatal_decode(
        &mut self.decoder.borrow_mut(),
        &mut self.pending.borrow_mut(),
        &bytes,
        stream,
      );
    }
    let mut output = String::new();
    // `decode_to_string` writes into the string's spare capacity.
    let extra = self
      .decoder
      .borrow()
      .max_utf8_buffer_length(bytes.len())
      .unwrap_or(bytes.len().saturating_mul(3).saturating_add(16));
    output.reserve(extra);
    let (_result, _, _had_errors) =
      self
        .decoder
        .borrow_mut()
        .decode_to_string(&bytes, &mut output, !stream);
    Ok(output)
  }
}

/// `new_decoder()` sniffs a UTF-8 or UTF-16 BOM and switches encoding.
/// TextDecoder keeps the label's encoding and only strips that encoding's BOM.
fn open_decoder(
  encoding: &'static encoding_rs::Encoding,
  ignore_bom: bool,
) -> encoding_rs::Decoder {
  if ignore_bom {
    encoding.new_decoder_without_bom_handling()
  } else {
    encoding.new_decoder_with_bom_removal()
  }
}

fn decoder_options(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> (bool, bool) {
  if !value.is_object() {
    return (false, false);
  }
  let Ok(object) = v8::Local::<v8::Object>::try_from(value) else {
    return (false, false);
  };
  let fatal_key = v8::String::new(scope, "fatal").unwrap();
  let ignore_key = v8::String::new(scope, "ignoreBOM").unwrap();
  let fatal = object
    .get(scope, fatal_key.into())
    .is_some_and(|value| value.boolean_value(scope));
  let ignore_bom = object
    .get(scope, ignore_key.into())
    .is_some_and(|value| value.boolean_value(scope));
  (fatal, ignore_bom)
}

enum Feed {
  Consumed,
  /// The byte was pushed back. It and every byte after it stay queued.
  PushedBack,
  Error,
}

/// Fatal decoding is one byte at a time. A bulk `Malformed` result hides
/// whether the next byte was pushed back (`read == 0`) or not yet visited.
/// EUC-JP restores the ASCII byte. UTF-8 does not keep `0xEF` after `0xFD`.
fn fatal_decode(
  decoder: &mut encoding_rs::Decoder,
  pending: &mut Vec<u8>,
  bytes: &[u8],
  stream: bool,
) -> Result<String, OpError> {
  let mut input = std::mem::take(pending);
  input.extend_from_slice(bytes);
  let mut output = String::new();
  let mut offset = 0usize;
  loop {
    if offset == input.len() {
      if stream {
        break;
      }
      // One flush. A finished decoder panics if `decode` is called again.
      match feed(decoder, &[], true, &mut output)? {
        Feed::Consumed => break,
        Feed::PushedBack | Feed::Error => {
          return Err(OpError::type_("TextDecoder failed to decode"));
        }
      }
    }
    match feed(decoder, &input[offset..offset + 1], false, &mut output)? {
      Feed::Consumed => offset += 1,
      Feed::PushedBack => {
        pending.extend_from_slice(&input[offset..]);
        return Err(OpError::type_("TextDecoder failed to decode"));
      }
      Feed::Error => {
        return Err(OpError::type_("TextDecoder failed to decode"));
      }
    }
  }
  Ok(output)
}

fn feed(
  decoder: &mut encoding_rs::Decoder,
  bytes: &[u8],
  last: bool,
  output: &mut String,
) -> Result<Feed, OpError> {
  loop {
    // BOM removal can emit a byte stashed on the previous call. That write
    // panics if the string has no spare capacity.
    if output.capacity() - output.len() < 16 {
      output.reserve(16);
    }
    let (result, read) =
      decoder.decode_to_string_without_replacement(bytes, output, last);
    match result {
      encoding_rs::DecoderResult::InputEmpty => return Ok(Feed::Consumed),
      encoding_rs::DecoderResult::Malformed(_, _) => {
        return Ok(if read == 0 {
          Feed::PushedBack
        } else {
          Feed::Error
        });
      }
      encoding_rs::DecoderResult::OutputFull => {
        let before = output.capacity();
        output.reserve(16);
        if output.capacity() == before {
          return Err(OpError::type_("TextDecoder failed to decode"));
        }
      }
    }
  }
}

fn stream_option(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> bool {
  if !value.is_object() {
    return false;
  }
  let Ok(object) = v8::Local::<v8::Object>::try_from(value) else {
    return false;
  };
  let key = v8::String::new(scope, "stream").unwrap();
  object
    .get(scope, key.into())
    .is_some_and(|value| value.boolean_value(scope))
}
