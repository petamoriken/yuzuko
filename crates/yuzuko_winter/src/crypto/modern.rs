//! SHA-3, SHAKE, KMAC, AES-OCB, ChaCha20-Poly1305, Argon2, ML-KEM,
//! ML-DSA, SLH-DSA-SHA2-128f, and the CG/C2PRI hybrid KEMs.
//!
//! Hybrid encapsulation follows draft-irtf-cfrg-hybrid-kems (CG framework,
//! C2PRI combiner) and draft-irtf-cfrg-concrete-hybrid-kems. The stored
//! private key is the 32-byte hybrid seed. `expandDecapsKey` is SHAKE256
//! into `KEM.Nseed || Group.Nseed`. Decapsulation combines
//! `Decaps(dk_PQ, ct_PQ)` with `ElementToSharedSecret(Exp(ct_T, dk_T))`.

use aes::cipher::consts::{U6, U7, U8, U9, U10, U11, U12, U13, U14, U15, U16};
use argon2::Argon2;
use argon2::ParamsBuilder;
use p256::elliptic_curve::ff::Field;
use p256::elliptic_curve::ff::PrimeField;
use p256::elliptic_curve::sec1::FromEncodedPoint;
use p256::elliptic_curve::sec1::ToEncodedPoint;
use sha3::Digest;
use sha3::Sha3_256;
use sha3::Sha3_384;
use sha3::Sha3_512;
use yuzuko_core::op::OpError;

#[derive(Clone, Copy, Debug)]
pub(crate) enum Fail {
  Operation(&'static str),
  Data(&'static str),
  Unsupported(&'static str),
}

impl Fail {
  pub(crate) fn into_op(self) -> OpError {
    let (name, message) = match self {
      Self::Operation(message) => ("OperationError", message),
      Self::Data(message) => ("DataError", message),
      Self::Unsupported(message) => ("NotSupportedError", message),
    };
    OpError::dom(name, message)
  }
}

fn owned(bytes: impl AsRef<[u8]>) -> Vec<u8> {
  bytes.as_ref().to_vec()
}

fn random(len: usize) -> Result<Vec<u8>, Fail> {
  let mut bytes = vec![0u8; len];
  getrandom::fill(&mut bytes)
    .map_err(|_| Fail::Operation("The random number generator failed"))?;
  Ok(bytes)
}

fn permute(state: &mut [u64; 25]) {
  keccak::Keccak::new().with_f1600(|permute| permute(state));
}

fn xor_byte(state: &mut [u64; 25], index: usize, byte: u8) {
  let lane = index / 8;
  let shift = (index % 8) * 8;
  state[lane] ^= u64::from(byte) << shift;
}

/// Keccak sponge. `delim` is the domain byte (SHA-3 `0x06`, SHAKE `0x1f`,
/// cSHAKE `0x04`). The rate is in bytes.
fn sponge(rate: usize, delim: u8, input: &[u8], out_len: usize) -> Vec<u8> {
  let mut state = [0u64; 25];
  let mut offset = 0;
  while offset + rate <= input.len() {
    for index in 0..rate {
      xor_byte(&mut state, index, input[offset + index]);
    }
    permute(&mut state);
    offset += rate;
  }
  let rest = &input[offset..];
  for (index, byte) in rest.iter().enumerate() {
    xor_byte(&mut state, index, *byte);
  }
  // The domain byte and the pad's final 1 share a byte when the partial
  // block already ends on the last rate byte.
  xor_byte(&mut state, rest.len(), delim);
  xor_byte(&mut state, rate - 1, 0x80);
  permute(&mut state);

  let mut out = vec![0u8; out_len];
  let mut produced = 0;
  while produced < out_len {
    let take = (out_len - produced).min(rate);
    for index in 0..take {
      let lane = index / 8;
      let shift = (index % 8) * 8;
      out[produced + index] = (state[lane] >> shift) as u8;
    }
    produced += take;
    if produced < out_len {
      permute(&mut state);
    }
  }
  out
}

pub(crate) fn sha3(name: &str, data: &[u8]) -> Result<Vec<u8>, Fail> {
  Ok(match name {
    "SHA3-256" => Sha3_256::digest(data).to_vec(),
    "SHA3-384" => Sha3_384::digest(data).to_vec(),
    "SHA3-512" => Sha3_512::digest(data).to_vec(),
    _ => {
      return Err(Fail::Unsupported("The digest algorithm is not supported"));
    }
  })
}

pub(crate) fn shake(
  name: &str,
  data: &[u8],
  out_len: usize,
) -> Result<Vec<u8>, Fail> {
  if out_len == 0 {
    return Err(Fail::Operation("The output length is zero"));
  }
  let rate = match name {
    "SHAKE128" => 168,
    "SHAKE256" => 136,
    _ => {
      return Err(Fail::Unsupported("The digest algorithm is not supported"));
    }
  };
  Ok(sponge(rate, 0x1f, data, out_len))
}

fn left_encode(value: usize) -> Vec<u8> {
  let mut width = 1usize;
  let mut rest = value;
  while rest >= 256 {
    rest >>= 8;
    width += 1;
  }
  let bytes = value.to_be_bytes();
  let mut out = Vec::with_capacity(1 + width);
  out.push(width as u8);
  out.extend_from_slice(&bytes[bytes.len() - width..]);
  out
}

fn right_encode(value: usize) -> Vec<u8> {
  let mut width = 1usize;
  let mut rest = value;
  while rest >= 256 {
    rest >>= 8;
    width += 1;
  }
  let bytes = value.to_be_bytes();
  let mut out = Vec::with_capacity(width + 1);
  out.extend_from_slice(&bytes[bytes.len() - width..]);
  out.push(width as u8);
  out
}

fn encode_string(bytes: &[u8]) -> Vec<u8> {
  let mut out = left_encode(bytes.len().saturating_mul(8));
  out.extend_from_slice(bytes);
  out
}

fn bytepad(encoded: &[u8], width: usize) -> Vec<u8> {
  let mut out = left_encode(width);
  out.extend_from_slice(encoded);
  let remainder = out.len() % width;
  if remainder != 0 {
    out.resize(out.len() + (width - remainder), 0);
  }
  out
}

pub(crate) fn kmac(
  name: &str,
  key: &[u8],
  data: &[u8],
  out_len: usize,
  custom: &[u8],
) -> Result<Vec<u8>, Fail> {
  if out_len == 0 {
    return Err(Fail::Operation("The output length is zero"));
  }
  let rate = match name {
    "KMAC128" => 168,
    "KMAC256" => 136,
    _ => {
      return Err(Fail::Unsupported(
        "The signature algorithm is not supported",
      ));
    }
  };
  // SP 800-185 KMAC is cSHAKE with N = "KMAC" and the encoded key.
  let mut prefix = encode_string(b"KMAC");
  prefix.extend(encode_string(custom));
  let mut input = bytepad(&prefix, rate);
  input.extend(bytepad(&encode_string(key), rate));
  input.extend_from_slice(data);
  input.extend(right_encode(out_len.saturating_mul(8)));
  Ok(sponge(rate, 0x04, &input, out_len))
}

macro_rules! ocb_go {
  ($cipher:ty, $nonce:ty, $tag:ty, $key:expr, $iv:expr, $aad:expr, $input:expr, $decrypt:expr) => {{
    let cipher =
      <ocb3::Ocb3<$cipher, $nonce, $tag> as ocb3::KeyInit>::new_from_slice(
        $key,
      )
      .map_err(|_| Fail::Operation("The AES-OCB key is invalid"))?;
    let nonce = ocb3::Nonce::<$nonce>::try_from($iv)
      .map_err(|_| Fail::Operation("The AES-OCB nonce is invalid"))?;
    let payload = ocb3::aead::Payload {
      msg: $input,
      aad: $aad,
    };
    let result = if $decrypt {
      ocb3::aead::Aead::decrypt(&cipher, &nonce, payload)
    } else {
      ocb3::aead::Aead::encrypt(&cipher, &nonce, payload)
    };
    result.map_err(|_| Fail::Operation("AES-OCB authentication failed"))
  }};
}

macro_rules! ocb_nonce {
  ($cipher:ty, $tag:ty, $key:expr, $iv:expr, $aad:expr, $input:expr, $decrypt:expr) => {
    match $iv.len() {
      6 => ocb_go!($cipher, U6, $tag, $key, $iv, $aad, $input, $decrypt),
      7 => ocb_go!($cipher, U7, $tag, $key, $iv, $aad, $input, $decrypt),
      8 => ocb_go!($cipher, U8, $tag, $key, $iv, $aad, $input, $decrypt),
      9 => ocb_go!($cipher, U9, $tag, $key, $iv, $aad, $input, $decrypt),
      10 => ocb_go!($cipher, U10, $tag, $key, $iv, $aad, $input, $decrypt),
      11 => ocb_go!($cipher, U11, $tag, $key, $iv, $aad, $input, $decrypt),
      12 => ocb_go!($cipher, U12, $tag, $key, $iv, $aad, $input, $decrypt),
      13 => ocb_go!($cipher, U13, $tag, $key, $iv, $aad, $input, $decrypt),
      14 => ocb_go!($cipher, U14, $tag, $key, $iv, $aad, $input, $decrypt),
      15 => ocb_go!($cipher, U15, $tag, $key, $iv, $aad, $input, $decrypt),
      _ => Err(Fail::Operation("The AES-OCB nonce must be 6 to 15 bytes")),
    }
  };
}

pub(crate) fn ocb_crypt(
  key: &[u8],
  nonce: &[u8],
  aad: &[u8],
  input: &[u8],
  tag_bytes: usize,
  decrypt: bool,
) -> Result<Vec<u8>, Fail> {
  if !(6..=15).contains(&nonce.len()) {
    return Err(Fail::Operation("The AES-OCB nonce must be 6 to 15 bytes"));
  }
  match (key.len(), tag_bytes) {
    (16, 8) => {
      ocb_nonce!(aes::Aes128, U8, key, nonce, aad, input, decrypt)
    }
    (16, 12) => {
      ocb_nonce!(aes::Aes128, U12, key, nonce, aad, input, decrypt)
    }
    (16, 16) => {
      ocb_nonce!(aes::Aes128, U16, key, nonce, aad, input, decrypt)
    }
    (24, 8) => {
      ocb_nonce!(aes::Aes192, U8, key, nonce, aad, input, decrypt)
    }
    (24, 12) => {
      ocb_nonce!(aes::Aes192, U12, key, nonce, aad, input, decrypt)
    }
    (24, 16) => {
      ocb_nonce!(aes::Aes192, U16, key, nonce, aad, input, decrypt)
    }
    (32, 8) => {
      ocb_nonce!(aes::Aes256, U8, key, nonce, aad, input, decrypt)
    }
    (32, 12) => {
      ocb_nonce!(aes::Aes256, U12, key, nonce, aad, input, decrypt)
    }
    (32, 16) => {
      ocb_nonce!(aes::Aes256, U16, key, nonce, aad, input, decrypt)
    }
    (16 | 24 | 32, _) => {
      Err(Fail::Operation("The AES-OCB tag length is invalid"))
    }
    _ => Err(Fail::Data("The AES key length is invalid")),
  }
}

pub(crate) fn chacha_crypt(
  key: &[u8],
  nonce: &[u8],
  aad: &[u8],
  input: &[u8],
  decrypt: bool,
) -> Result<Vec<u8>, Fail> {
  if key.len() != 32 {
    return Err(Fail::Data("The ChaCha20-Poly1305 key must be 32 bytes"));
  }
  if nonce.len() != 12 {
    return Err(Fail::Operation(
      "The ChaCha20-Poly1305 nonce must be 12 bytes",
    ));
  }
  let cipher = <chacha20poly1305::ChaCha20Poly1305 as chacha20poly1305::KeyInit>::new_from_slice(key)
    .map_err(|_| Fail::Data("The ChaCha20-Poly1305 key is invalid"))?;
  let nonce = chacha20poly1305::Nonce::try_from(nonce).map_err(|_| {
    Fail::Operation("The ChaCha20-Poly1305 nonce must be 12 bytes")
  })?;
  let payload = chacha20poly1305::aead::Payload { msg: input, aad };
  let result = if decrypt {
    chacha20poly1305::aead::Aead::decrypt(&cipher, &nonce, payload)
  } else {
    chacha20poly1305::aead::Aead::encrypt(&cipher, &nonce, payload)
  };
  result.map_err(|_| Fail::Operation("ChaCha20-Poly1305 authentication failed"))
}

pub(crate) struct ArgonInput<'a> {
  pub algorithm: &'a str,
  pub password: &'a [u8],
  pub salt: &'a [u8],
  pub secret: &'a [u8],
  pub associated: &'a [u8],
  pub memory: u32,
  pub passes: u32,
  pub parallelism: u32,
  pub output_len: usize,
}

pub(crate) fn argon2(input: &ArgonInput<'_>) -> Result<Vec<u8>, Fail> {
  let algorithm = match input.algorithm {
    "Argon2d" => argon2::Algorithm::Argon2d,
    "Argon2i" => argon2::Algorithm::Argon2i,
    "Argon2id" => argon2::Algorithm::Argon2id,
    _ => {
      return Err(Fail::Unsupported(
        "The derivation algorithm is not supported",
      ));
    }
  };
  let mut builder = ParamsBuilder::new();
  builder
    .m_cost(input.memory)
    .t_cost(input.passes)
    .p_cost(input.parallelism)
    .output_len(input.output_len);
  if !input.associated.is_empty() {
    let data = argon2::AssociatedData::new(input.associated).map_err(|_| {
      Fail::Operation("The Argon2 associated data is longer than 32 bytes")
    })?;
    builder.data(data);
  }
  let params = builder
    .build()
    .map_err(|_| Fail::Operation("The Argon2 parameters are invalid"))?;
  let mut out = vec![0u8; input.output_len];
  let hashed = if input.secret.is_empty() {
    Argon2::new(algorithm, argon2::Version::V0x13, params).hash_password_into(
      input.password,
      input.salt,
      &mut out,
    )
  } else {
    Argon2::new_with_secret(
      input.secret,
      algorithm,
      argon2::Version::V0x13,
      params,
    )
    .map_err(|_| Fail::Operation("The Argon2 secret is invalid"))?
    .hash_password_into(input.password, input.salt, &mut out)
  };
  hashed.map_err(|_| Fail::Operation("Argon2 failed"))?;
  Ok(out)
}

pub(crate) fn is_mlkem(name: &str) -> bool {
  matches!(name, "ML-KEM-512" | "ML-KEM-768" | "ML-KEM-1024")
}

pub(crate) fn is_mldsa(name: &str) -> bool {
  matches!(name, "ML-DSA-44" | "ML-DSA-65" | "ML-DSA-87")
}

pub(crate) fn is_hybrid(name: &str) -> bool {
  matches!(name, "MLKEM768-P256" | "MLKEM768-X25519" | "MLKEM1024-P384")
}

pub(crate) fn is_slh(name: &str) -> bool {
  name == "SLH-DSA-SHA2-128f"
}

const MLKEM_OID_512: &[u8] = &[
  0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x04, 0x01,
];
const MLKEM_OID_768: &[u8] = &[
  0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x04, 0x02,
];
const MLKEM_OID_1024: &[u8] = &[
  0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x04, 0x03,
];
const MLDSA_OID_44: &[u8] = &[
  0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x03, 0x11,
];
const MLDSA_OID_65: &[u8] = &[
  0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x03, 0x12,
];
const MLDSA_OID_87: &[u8] = &[
  0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x03, 0x13,
];

fn oid(name: &str) -> Option<&'static [u8]> {
  Some(match name {
    "ML-KEM-512" => MLKEM_OID_512,
    "ML-KEM-768" => MLKEM_OID_768,
    "ML-KEM-1024" => MLKEM_OID_1024,
    "ML-DSA-44" => MLDSA_OID_44,
    "ML-DSA-65" => MLDSA_OID_65,
    "ML-DSA-87" => MLDSA_OID_87,
    _ => return None,
  })
}

fn oid_name(content: &[u8]) -> Option<&'static str> {
  Some(match content {
    MLKEM_OID_512 => "ML-KEM-512",
    MLKEM_OID_768 => "ML-KEM-768",
    MLKEM_OID_1024 => "ML-KEM-1024",
    MLDSA_OID_44 => "ML-DSA-44",
    MLDSA_OID_65 => "ML-DSA-65",
    MLDSA_OID_87 => "ML-DSA-87",
    _ => return None,
  })
}

pub(crate) fn seed_len(name: &str) -> Option<usize> {
  if is_mlkem(name) {
    Some(64)
  } else if is_mldsa(name) {
    Some(32)
  } else if is_hybrid(name) {
    Some(32)
  } else {
    None
  }
}

fn der_len(len: usize) -> Vec<u8> {
  if len < 0x80 {
    vec![len as u8]
  } else if len <= 0xff {
    vec![0x81, len as u8]
  } else if len <= 0xffff {
    vec![0x82, (len >> 8) as u8, len as u8]
  } else {
    vec![0x83, (len >> 16) as u8, (len >> 8) as u8, len as u8]
  }
}

fn der_tlv(tag: u8, content: &[u8]) -> Vec<u8> {
  let mut out = Vec::with_capacity(4 + content.len());
  out.push(tag);
  out.extend(der_len(content.len()));
  out.extend_from_slice(content);
  out
}

struct Cursor<'a> {
  data: &'a [u8],
  index: usize,
}

impl<'a> Cursor<'a> {
  fn new(data: &'a [u8]) -> Self {
    Self { data, index: 0 }
  }

  fn rest(&self) -> &'a [u8] {
    &self.data[self.index..]
  }

  fn take(&mut self, len: usize) -> Result<&'a [u8], Fail> {
    let end = self
      .index
      .checked_add(len)
      .filter(|end| *end <= self.data.len())
      .ok_or(Fail::Data("The key encoding is truncated"))?;
    let bytes = &self.data[self.index..end];
    self.index = end;
    Ok(bytes)
  }

  fn byte(&mut self) -> Result<u8, Fail> {
    Ok(self.take(1)?[0])
  }

  fn length(&mut self) -> Result<usize, Fail> {
    let first = self.byte()?;
    if first < 0x80 {
      return Ok(usize::from(first));
    }
    let width = usize::from(first & 0x7f);
    if width == 0 || width > 3 {
      return Err(Fail::Data("The key encoding length is invalid"));
    }
    let bytes = self.take(width)?;
    let mut value = 0usize;
    for byte in bytes {
      value = (value << 8) | usize::from(*byte);
    }
    Ok(value)
  }

  fn tlv(&mut self, tag: u8) -> Result<&'a [u8], Fail> {
    let actual = self.byte()?;
    if actual != tag {
      return Err(Fail::Data("The key encoding tag is invalid"));
    }
    let len = self.length()?;
    self.take(len)
  }
}

pub(crate) fn pkcs8_encode(name: &str, seed: &[u8]) -> Result<Vec<u8>, Fail> {
  let oid =
    oid(name).ok_or(Fail::Unsupported("The key algorithm is not supported"))?;
  let expected = seed_len(name).unwrap_or(0);
  if seed.len() != expected || seed.len() >= 128 {
    return Err(Fail::Data("The seed length is invalid"));
  }
  let mut wrapped = Vec::with_capacity(2 + seed.len());
  wrapped.push(0x80);
  wrapped.push(seed.len() as u8);
  wrapped.extend_from_slice(seed);
  let mut body = vec![0x02, 0x01, 0x00];
  body.extend(der_tlv(0x30, oid));
  body.extend(der_tlv(0x04, &wrapped));
  Ok(der_tlv(0x30, &body))
}

pub(crate) fn pkcs8_decode(
  bytes: &[u8],
) -> Result<(&'static str, Vec<u8>), Fail> {
  let mut cursor = Cursor::new(bytes);
  let body = cursor.tlv(0x30)?;
  if !cursor.rest().is_empty() {
    return Err(Fail::Data("The PKCS#8 encoding has trailing bytes"));
  }
  let mut inner = Cursor::new(body);
  let version = inner.tlv(0x02)?;
  if version != [0] {
    return Err(Fail::Data("The PKCS#8 version is invalid"));
  }
  let algorithm = inner.tlv(0x30)?;
  let name =
    oid_name(algorithm).ok_or(Fail::Data("The key algorithm is invalid"))?;
  let octet = inner.tlv(0x04)?;
  if !inner.rest().is_empty() {
    return Err(Fail::Data("The PKCS#8 encoding has trailing bytes"));
  }
  if octet.len() < 2 || octet[0] != 0x80 {
    return Err(Fail::Data("The PKCS#8 seed is invalid"));
  }
  let len = usize::from(octet[1]);
  if len != seed_len(name).unwrap_or(0) || octet.len() != 2 + len {
    return Err(Fail::Data("The PKCS#8 seed length is invalid"));
  }
  Ok((name, octet[2..].to_vec()))
}

pub(crate) fn spki_encode(name: &str, public: &[u8]) -> Result<Vec<u8>, Fail> {
  let oid =
    oid(name).ok_or(Fail::Unsupported("The key algorithm is not supported"))?;
  let mut bits = Vec::with_capacity(1 + public.len());
  bits.push(0);
  bits.extend_from_slice(public);
  let mut body = der_tlv(0x30, oid);
  body.extend(der_tlv(0x03, &bits));
  Ok(der_tlv(0x30, &body))
}

pub(crate) fn spki_decode(
  bytes: &[u8],
) -> Result<(&'static str, Vec<u8>), Fail> {
  let mut cursor = Cursor::new(bytes);
  let body = cursor.tlv(0x30)?;
  if !cursor.rest().is_empty() {
    return Err(Fail::Data("The SPKI encoding has trailing bytes"));
  }
  let mut inner = Cursor::new(body);
  let algorithm = inner.tlv(0x30)?;
  let name =
    oid_name(algorithm).ok_or(Fail::Data("The key algorithm is invalid"))?;
  let bits = inner.tlv(0x03)?;
  if !inner.rest().is_empty() {
    return Err(Fail::Data("The SPKI encoding has trailing bytes"));
  }
  if bits.first() != Some(&0) || bits.len() < 2 {
    return Err(Fail::Data("The SPKI public key is invalid"));
  }
  let public = bits[1..].to_vec();
  if is_mlkem(name) {
    mlkem_validate_public(name, &public)?;
  } else if is_mldsa(name) {
    mldsa_validate_public(name, &public)?;
  }
  Ok((name, public))
}

macro_rules! mlkem_public {
  ($set:ty, $seed:expr) => {{
    let seed = ml_kem::Seed::try_from($seed)
      .map_err(|_| Fail::Data("The ML-KEM seed must be 64 bytes"))?;
    let key = ml_kem::DecapsulationKey::<$set>::from_seed(seed);
    Ok(owned(ml_kem::KeyExport::to_bytes(key.encapsulation_key())))
  }};
}

macro_rules! mlkem_check {
  ($set:ty, $public:expr) => {{
    let key = ml_kem::Key::<ml_kem::EncapsulationKey<$set>>::try_from($public)
      .map_err(|_| Fail::Data("The ML-KEM public key length is invalid"))?;
    ml_kem::EncapsulationKey::<$set>::new(&key)
      .map_err(|_| Fail::Data("The ML-KEM public key is invalid"))?;
    Ok(())
  }};
}

macro_rules! mlkem_encap {
  ($set:ty, $public:expr) => {{
    let key = ml_kem::Key::<ml_kem::EncapsulationKey<$set>>::try_from($public)
      .map_err(|_| {
        Fail::Operation("The ML-KEM public key length is invalid")
      })?;
    let key = ml_kem::EncapsulationKey::<$set>::new(&key)
      .map_err(|_| Fail::Operation("The ML-KEM public key is invalid"))?;
    let (ciphertext, shared) = ml_kem::Encapsulate::encapsulate(&key);
    Ok((owned(ciphertext), owned(shared)))
  }};
}

macro_rules! mlkem_decap {
  ($set:ty, $seed:expr, $ciphertext:expr) => {{
    let seed = ml_kem::Seed::try_from($seed)
      .map_err(|_| Fail::Data("The ML-KEM seed must be 64 bytes"))?;
    let key = ml_kem::DecapsulationKey::<$set>::from_seed(seed);
    let shared = ml_kem::Decapsulate::decapsulate_slice(&key, $ciphertext)
      .map_err(|_| {
        Fail::Operation("The ML-KEM ciphertext length is invalid")
      })?;
    Ok(owned(shared))
  }};
}

pub(crate) fn mlkem_public_from_seed(
  name: &str,
  seed: &[u8],
) -> Result<Vec<u8>, Fail> {
  match name {
    "ML-KEM-512" => mlkem_public!(ml_kem::MlKem512, seed),
    "ML-KEM-768" => mlkem_public!(ml_kem::MlKem768, seed),
    "ML-KEM-1024" => mlkem_public!(ml_kem::MlKem1024, seed),
    _ => Err(Fail::Unsupported("The KEM algorithm is not supported")),
  }
}

pub(crate) fn mlkem_validate_public(
  name: &str,
  public: &[u8],
) -> Result<(), Fail> {
  match name {
    "ML-KEM-512" => mlkem_check!(ml_kem::MlKem512, public),
    "ML-KEM-768" => mlkem_check!(ml_kem::MlKem768, public),
    "ML-KEM-1024" => mlkem_check!(ml_kem::MlKem1024, public),
    _ => Err(Fail::Unsupported("The KEM algorithm is not supported")),
  }
}

pub(crate) fn mlkem_encapsulate(
  name: &str,
  public: &[u8],
) -> Result<(Vec<u8>, Vec<u8>), Fail> {
  match name {
    "ML-KEM-512" => mlkem_encap!(ml_kem::MlKem512, public),
    "ML-KEM-768" => mlkem_encap!(ml_kem::MlKem768, public),
    "ML-KEM-1024" => mlkem_encap!(ml_kem::MlKem1024, public),
    _ => Err(Fail::Unsupported("The KEM algorithm is not supported")),
  }
}

pub(crate) fn mlkem_decapsulate(
  name: &str,
  seed: &[u8],
  ciphertext: &[u8],
) -> Result<Vec<u8>, Fail> {
  match name {
    "ML-KEM-512" => mlkem_decap!(ml_kem::MlKem512, seed, ciphertext),
    "ML-KEM-768" => mlkem_decap!(ml_kem::MlKem768, seed, ciphertext),
    "ML-KEM-1024" => mlkem_decap!(ml_kem::MlKem1024, seed, ciphertext),
    _ => Err(Fail::Unsupported("The KEM algorithm is not supported")),
  }
}

macro_rules! mldsa_public {
  ($set:ty, $seed:expr) => {{
    let seed = ml_dsa::Seed::try_from($seed)
      .map_err(|_| Fail::Data("The ML-DSA seed must be 32 bytes"))?;
    let key = ml_dsa::SigningKey::<$set>::from_seed(&seed);
    let verifying: &ml_dsa::VerifyingKey<$set> = key.as_ref();
    Ok(owned(verifying.encode()))
  }};
}

macro_rules! mldsa_check {
  ($set:ty, $public:expr) => {{
    let encoded = ml_dsa::EncodedVerifyingKey::<$set>::try_from($public)
      .map_err(|_| Fail::Data("The ML-DSA public key length is invalid"))?;
    let _ = ml_dsa::VerifyingKey::<$set>::decode(&encoded);
    Ok(())
  }};
}

macro_rules! mldsa_sign {
  ($set:ty, $seed:expr, $message:expr, $context:expr) => {{
    if $context.len() > 255 {
      return Err(Fail::Operation("The context is longer than 255 bytes"));
    }
    let seed = ml_dsa::Seed::try_from($seed)
      .map_err(|_| Fail::Data("The ML-DSA seed must be 32 bytes"))?;
    let key = ml_dsa::SigningKey::<$set>::from_seed(&seed);
    let signature = key
      .expanded_key()
      .sign_deterministic($message, $context)
      .map_err(|_| Fail::Operation("ML-DSA signing failed"))?;
    Ok(owned(signature.encode()))
  }};
}

macro_rules! mldsa_verify {
  ($set:ty, $public:expr, $message:expr, $signature:expr, $context:expr) => {{
    if $context.len() > 255 {
      return Err(Fail::Operation("The context is longer than 255 bytes"));
    }
    let encoded = ml_dsa::EncodedVerifyingKey::<$set>::try_from($public)
      .map_err(|_| Fail::Data("The ML-DSA public key length is invalid"))?;
    let key = ml_dsa::VerifyingKey::<$set>::decode(&encoded);
    let Ok(signature) = ml_dsa::Signature::<$set>::try_from($signature) else {
      return Ok(false);
    };
    Ok(key.verify_with_context($message, $context, &signature))
  }};
}

pub(crate) fn mldsa_public_from_seed(
  name: &str,
  seed: &[u8],
) -> Result<Vec<u8>, Fail> {
  match name {
    "ML-DSA-44" => mldsa_public!(ml_dsa::MlDsa44, seed),
    "ML-DSA-65" => mldsa_public!(ml_dsa::MlDsa65, seed),
    "ML-DSA-87" => mldsa_public!(ml_dsa::MlDsa87, seed),
    _ => Err(Fail::Unsupported(
      "The signature algorithm is not supported",
    )),
  }
}

pub(crate) fn mldsa_validate_public(
  name: &str,
  public: &[u8],
) -> Result<(), Fail> {
  match name {
    "ML-DSA-44" => mldsa_check!(ml_dsa::MlDsa44, public),
    "ML-DSA-65" => mldsa_check!(ml_dsa::MlDsa65, public),
    "ML-DSA-87" => mldsa_check!(ml_dsa::MlDsa87, public),
    _ => Err(Fail::Unsupported(
      "The signature algorithm is not supported",
    )),
  }
}

pub(crate) fn mldsa_sign(
  name: &str,
  seed: &[u8],
  message: &[u8],
  context: &[u8],
) -> Result<Vec<u8>, Fail> {
  match name {
    "ML-DSA-44" => mldsa_sign!(ml_dsa::MlDsa44, seed, message, context),
    "ML-DSA-65" => mldsa_sign!(ml_dsa::MlDsa65, seed, message, context),
    "ML-DSA-87" => mldsa_sign!(ml_dsa::MlDsa87, seed, message, context),
    _ => Err(Fail::Unsupported(
      "The signature algorithm is not supported",
    )),
  }
}

pub(crate) fn mldsa_verify(
  name: &str,
  public: &[u8],
  message: &[u8],
  signature: &[u8],
  context: &[u8],
) -> Result<bool, Fail> {
  match name {
    "ML-DSA-44" => {
      mldsa_verify!(ml_dsa::MlDsa44, public, message, signature, context)
    }
    "ML-DSA-65" => {
      mldsa_verify!(ml_dsa::MlDsa65, public, message, signature, context)
    }
    "ML-DSA-87" => {
      mldsa_verify!(ml_dsa::MlDsa87, public, message, signature, context)
    }
    _ => Err(Fail::Unsupported(
      "The signature algorithm is not supported",
    )),
  }
}

const SLH_N: usize = 16;

pub(crate) fn slh_generate() -> Result<(Vec<u8>, Vec<u8>), Fail> {
  let sk_seed = random(SLH_N)?;
  let sk_prf = random(SLH_N)?;
  let pk_seed = random(SLH_N)?;
  let key = slh_dsa::SigningKey::<slh_dsa::Sha2_128f>::slh_keygen_internal(
    &sk_seed, &sk_prf, &pk_seed,
  );
  let private = key.to_vec();
  let public = private[SLH_N * 2..].to_vec();
  Ok((private, public))
}

pub(crate) fn slh_public_from_private(private: &[u8]) -> Result<Vec<u8>, Fail> {
  let key = slh_dsa::SigningKey::<slh_dsa::Sha2_128f>::try_from(private)
    .map_err(|_| Fail::Data("The SLH-DSA private key is invalid"))?;
  Ok(key.to_vec()[SLH_N * 2..].to_vec())
}

pub(crate) fn slh_validate_public(public: &[u8]) -> Result<(), Fail> {
  slh_dsa::VerifyingKey::<slh_dsa::Sha2_128f>::try_from(public)
    .map(|_| ())
    .map_err(|_| Fail::Data("The SLH-DSA public key is invalid"))
}

pub(crate) fn slh_sign(
  private: &[u8],
  message: &[u8],
  context: &[u8],
) -> Result<Vec<u8>, Fail> {
  if context.len() > 255 {
    return Err(Fail::Operation("The context is longer than 255 bytes"));
  }
  let key = slh_dsa::SigningKey::<slh_dsa::Sha2_128f>::try_from(private)
    .map_err(|_| Fail::Data("The SLH-DSA private key is invalid"))?;
  let signature = key
    .try_sign_with_context(message, context, None)
    .map_err(|_| Fail::Operation("SLH-DSA signing failed"))?;
  Ok(signature.to_vec())
}

pub(crate) fn slh_verify(
  public: &[u8],
  message: &[u8],
  signature: &[u8],
  context: &[u8],
) -> Result<bool, Fail> {
  if context.len() > 255 {
    return Err(Fail::Operation("The context is longer than 255 bytes"));
  }
  let key = slh_dsa::VerifyingKey::<slh_dsa::Sha2_128f>::try_from(public)
    .map_err(|_| Fail::Data("The SLH-DSA public key is invalid"))?;
  let Ok(signature) =
    slh_dsa::Signature::<slh_dsa::Sha2_128f>::try_from(signature)
  else {
    return Ok(false);
  };
  Ok(
    key
      .try_verify_with_context(message, context, &signature)
      .is_ok(),
  )
}

struct HybridSpec {
  name: &'static str,
  pq: &'static str,
  ek_pq: usize,
  ct_pq: usize,
  nseed_t: usize,
  nelem: usize,
  label: &'static [u8],
}

fn hybrid_spec(name: &str) -> Result<HybridSpec, Fail> {
  Ok(match name {
    "MLKEM768-P256" => HybridSpec {
      name: "MLKEM768-P256",
      pq: "ML-KEM-768",
      ek_pq: 1184,
      ct_pq: 1088,
      nseed_t: 128,
      nelem: 65,
      label: b"MLKEM768-P256",
    },
    "MLKEM768-X25519" => HybridSpec {
      name: "MLKEM768-X25519",
      pq: "ML-KEM-768",
      ek_pq: 1184,
      ct_pq: 1088,
      nseed_t: 32,
      nelem: 32,
      label: &[0x5c, 0x2e, 0x2f, 0x2f, 0x5e, 0x5c],
    },
    "MLKEM1024-P384" => HybridSpec {
      name: "MLKEM1024-P384",
      pq: "ML-KEM-1024",
      ek_pq: 1568,
      ct_pq: 1568,
      nseed_t: 48,
      nelem: 97,
      label: b"MLKEM1024-P384",
    },
    _ => {
      return Err(Fail::Unsupported("The KEM algorithm is not supported"));
    }
  })
}

enum Trad {
  P256(p256::Scalar),
  P384(p384::Scalar),
  X25519([u8; 32]),
}

struct Expanded {
  pq: [u8; 64],
  trad: Trad,
  ek_t: Vec<u8>,
}

fn field_scalar_p256(bytes: &[u8]) -> Option<p256::Scalar> {
  if bytes.len() != 32 {
    return None;
  }
  let mut repr = p256::FieldBytes::default();
  repr.as_mut_slice().copy_from_slice(bytes);
  p256::Scalar::from_repr(repr).into_option()
}

fn field_scalar_p384(bytes: &[u8]) -> Option<p384::Scalar> {
  if bytes.len() != 48 {
    return None;
  }
  let mut repr = p384::FieldBytes::default();
  repr.as_mut_slice().copy_from_slice(bytes);
  p384::Scalar::from_repr(repr).into_option()
}

fn random_scalar_p256(seed: &[u8]) -> Result<p256::Scalar, Fail> {
  let mut start = 0;
  while start + 32 <= seed.len() {
    if let Some(scalar) = field_scalar_p256(&seed[start..start + 32])
      && !bool::from(scalar.is_zero())
    {
      return Ok(scalar);
    }
    start += 32;
  }
  Err(Fail::Operation("The P-256 scalar seed was rejected"))
}

fn random_scalar_p384(seed: &[u8]) -> Result<p384::Scalar, Fail> {
  let mut start = 0;
  while start + 48 <= seed.len() {
    if let Some(scalar) = field_scalar_p384(&seed[start..start + 48])
      && !bool::from(scalar.is_zero())
    {
      return Ok(scalar);
    }
    start += 48;
  }
  Err(Fail::Operation("The P-384 scalar seed was rejected"))
}

fn exp_p256(scalar: &p256::Scalar) -> Vec<u8> {
  let point = (p256::ProjectivePoint::GENERATOR * scalar).to_affine();
  point.to_encoded_point(false).as_bytes().to_vec()
}

fn exp_p384(scalar: &p384::Scalar) -> Vec<u8> {
  let point = (p384::ProjectivePoint::GENERATOR * scalar).to_affine();
  point.to_encoded_point(false).as_bytes().to_vec()
}

fn x25519_public(scalar: &[u8; 32]) -> Vec<u8> {
  let secret = x25519_dalek::StaticSecret::from(*scalar);
  x25519_dalek::PublicKey::from(&secret).to_bytes().to_vec()
}

fn parse_p256(bytes: &[u8], imported: bool) -> Result<p256::AffinePoint, Fail> {
  let invalid = if imported {
    Fail::Data("The P-256 element is invalid")
  } else {
    Fail::Operation("The P-256 element is invalid")
  };
  if bytes.len() != 65 || bytes.first() != Some(&0x04) {
    return Err(invalid);
  }
  let encoded = p256::EncodedPoint::from_bytes(bytes).map_err(|_| invalid)?;
  if encoded.is_identity() {
    return Err(invalid);
  }
  p256::AffinePoint::from_encoded_point(&encoded)
    .into_option()
    .ok_or(invalid)
}

fn parse_p384(bytes: &[u8], imported: bool) -> Result<p384::AffinePoint, Fail> {
  let invalid = if imported {
    Fail::Data("The P-384 element is invalid")
  } else {
    Fail::Operation("The P-384 element is invalid")
  };
  if bytes.len() != 97 || bytes.first() != Some(&0x04) {
    return Err(invalid);
  }
  let encoded = p384::EncodedPoint::from_bytes(bytes).map_err(|_| invalid)?;
  if encoded.is_identity() {
    return Err(invalid);
  }
  p384::AffinePoint::from_encoded_point(&encoded)
    .into_option()
    .ok_or(invalid)
}

fn nist_x_p256(scalar: &p256::Scalar, point: &[u8]) -> Result<Vec<u8>, Fail> {
  let affine = parse_p256(point, false)?;
  let shared = (p256::ProjectivePoint::from(affine) * scalar).to_affine();
  let encoded = shared.to_encoded_point(false);
  if encoded.is_identity() {
    return Err(Fail::Operation("The P-256 shared point is the identity"));
  }
  let x = encoded
    .x()
    .ok_or(Fail::Operation("The P-256 shared point is the identity"))?;
  Ok(owned(x))
}

fn nist_x_p384(scalar: &p384::Scalar, point: &[u8]) -> Result<Vec<u8>, Fail> {
  let affine = parse_p384(point, false)?;
  let shared = (p384::ProjectivePoint::from(affine) * scalar).to_affine();
  let encoded = shared.to_encoded_point(false);
  if encoded.is_identity() {
    return Err(Fail::Operation("The P-384 shared point is the identity"));
  }
  let x = encoded
    .x()
    .ok_or(Fail::Operation("The P-384 shared point is the identity"))?;
  Ok(owned(x))
}

fn x25519_shared(scalar: &[u8; 32], point: &[u8]) -> Result<Vec<u8>, Fail> {
  if point.len() != 32 {
    return Err(Fail::Operation("The X25519 element must be 32 bytes"));
  }
  let mut peer = [0u8; 32];
  peer.copy_from_slice(point);
  let secret = x25519_dalek::StaticSecret::from(*scalar);
  let shared = secret.diffie_hellman(&x25519_dalek::PublicKey::from(peer));
  let bytes = shared.to_bytes();
  if bytes == [0u8; 32] {
    return Err(Fail::Operation("The X25519 shared secret is zero"));
  }
  Ok(bytes.to_vec())
}

fn traditional(
  spec: &HybridSpec,
  seed: &[u8],
) -> Result<(Trad, Vec<u8>), Fail> {
  match spec.name {
    "MLKEM768-P256" => {
      let scalar = random_scalar_p256(seed)?;
      let public = exp_p256(&scalar);
      Ok((Trad::P256(scalar), public))
    }
    "MLKEM1024-P384" => {
      let scalar = random_scalar_p384(seed)?;
      let public = exp_p384(&scalar);
      Ok((Trad::P384(scalar), public))
    }
    "MLKEM768-X25519" => {
      if seed.len() != 32 {
        return Err(Fail::Operation("The X25519 seed must be 32 bytes"));
      }
      let mut raw = [0u8; 32];
      raw.copy_from_slice(seed);
      let public = x25519_public(&raw);
      Ok((Trad::X25519(raw), public))
    }
    _ => Err(Fail::Unsupported("The KEM algorithm is not supported")),
  }
}

fn expand(spec: &HybridSpec, seed: &[u8]) -> Result<Expanded, Fail> {
  if seed.len() != 32 {
    return Err(Fail::Data("The hybrid seed must be 32 bytes"));
  }
  let full = sponge(136, 0x1f, seed, 64 + spec.nseed_t);
  let mut pq = [0u8; 64];
  pq.copy_from_slice(&full[..64]);
  let (trad, ek_t) = traditional(spec, &full[64..])?;
  Ok(Expanded { pq, trad, ek_t })
}

fn element_shared(trad: &Trad, point: &[u8]) -> Result<Vec<u8>, Fail> {
  match trad {
    Trad::P256(scalar) => nist_x_p256(scalar, point),
    Trad::P384(scalar) => nist_x_p384(scalar, point),
    Trad::X25519(scalar) => x25519_shared(scalar, point),
  }
}

fn element_public(trad: &Trad) -> Vec<u8> {
  match trad {
    Trad::P256(scalar) => exp_p256(scalar),
    Trad::P384(scalar) => exp_p384(scalar),
    Trad::X25519(scalar) => x25519_public(scalar),
  }
}

fn validate_element(spec: &HybridSpec, point: &[u8]) -> Result<(), Fail> {
  match spec.name {
    "MLKEM768-P256" => parse_p256(point, true).map(|_| ()),
    "MLKEM1024-P384" => parse_p384(point, true).map(|_| ()),
    "MLKEM768-X25519" if point.len() == 32 => Ok(()),
    "MLKEM768-X25519" => Err(Fail::Data("The X25519 element must be 32 bytes")),
    _ => Err(Fail::Unsupported("The KEM algorithm is not supported")),
  }
}

fn combine(
  shared_pq: &[u8],
  shared_t: &[u8],
  ciphertext_t: &[u8],
  public_t: &[u8],
  label: &[u8],
) -> Vec<u8> {
  let mut input = Vec::with_capacity(
    shared_pq.len()
      + shared_t.len()
      + ciphertext_t.len()
      + public_t.len()
      + label.len(),
  );
  input.extend_from_slice(shared_pq);
  input.extend_from_slice(shared_t);
  input.extend_from_slice(ciphertext_t);
  input.extend_from_slice(public_t);
  input.extend_from_slice(label);
  Sha3_256::digest(&input).to_vec()
}

pub(crate) fn hybrid_public_from_seed(
  name: &str,
  seed: &[u8],
) -> Result<Vec<u8>, Fail> {
  let spec = hybrid_spec(name)?;
  let expanded = expand(&spec, seed)?;
  let mut public = mlkem_public_from_seed(spec.pq, &expanded.pq)?;
  public.extend_from_slice(&expanded.ek_t);
  Ok(public)
}

pub(crate) fn hybrid_validate_public(
  name: &str,
  public: &[u8],
) -> Result<(), Fail> {
  let spec = hybrid_spec(name)?;
  if public.len() != spec.ek_pq + spec.nelem {
    return Err(Fail::Data("The hybrid public key length is invalid"));
  }
  let (pq, traditional) = public.split_at(spec.ek_pq);
  mlkem_validate_public(spec.pq, pq)?;
  validate_element(&spec, traditional)
}

pub(crate) fn hybrid_encapsulate(
  name: &str,
  public: &[u8],
) -> Result<(Vec<u8>, Vec<u8>), Fail> {
  let spec = hybrid_spec(name)?;
  if public.len() != spec.ek_pq + spec.nelem {
    return Err(Fail::Operation("The hybrid public key length is invalid"));
  }
  let (pq, ek_t) = public.split_at(spec.ek_pq);
  validate_element(&spec, ek_t)
    .map_err(|_| Fail::Operation("The traditional public key is invalid"))?;
  let (ciphertext_pq, shared_pq) = mlkem_encapsulate(spec.pq, pq)?;
  let ephemeral = random(spec.nseed_t)?;
  let (trad, _) = traditional(&spec, &ephemeral)?;
  let ciphertext_t = element_public(&trad);
  let shared_t = element_shared(&trad, ek_t)?;
  let shared = combine(&shared_pq, &shared_t, &ciphertext_t, ek_t, spec.label);
  let mut ciphertext = ciphertext_pq;
  ciphertext.extend_from_slice(&ciphertext_t);
  Ok((ciphertext, shared))
}

pub(crate) fn hybrid_decapsulate(
  name: &str,
  seed: &[u8],
  ciphertext: &[u8],
) -> Result<Vec<u8>, Fail> {
  let spec = hybrid_spec(name)?;
  if ciphertext.len() != spec.ct_pq + spec.nelem {
    return Err(Fail::Operation("The ciphertext length is invalid"));
  }
  let expanded = expand(&spec, seed)?;
  let (ciphertext_pq, ciphertext_t) = ciphertext.split_at(spec.ct_pq);
  let shared_pq = mlkem_decapsulate(spec.pq, &expanded.pq, ciphertext_pq)?;
  let shared_t = element_shared(&expanded.trad, ciphertext_t)?;
  Ok(combine(
    &shared_pq,
    &shared_t,
    ciphertext_t,
    &expanded.ek_t,
    spec.label,
  ))
}

#[cfg(test)]
mod tests {
  use super::*;

  fn hex(text: &str) -> Vec<u8> {
    let digits: Vec<u8> = text.bytes().filter(u8::is_ascii_hexdigit).collect();
    assert_eq!(digits.len() % 2, 0, "odd hex length");
    digits
      .chunks(2)
      .map(|pair| {
        u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap()
      })
      .collect()
  }

  fn assert_bytes(actual: &[u8], expected: &[u8], label: &str) {
    if actual != expected {
      panic!(
        "{label} mismatch len {} vs {} actual {:02x?} expected {:02x?}",
        actual.len(),
        expected.len(),
        &actual[..actual.len().min(8)],
        &expected[..expected.len().min(8)],
      );
    }
  }

  fn suite(relative: &str) -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
      .join("../../tests/wpt/suite")
      .join(relative);
    std::fs::read_to_string(&path)
      .unwrap_or_else(|err| panic!("read {}: {err}", path.display()))
  }

  fn js_field(text: &str, section: &str, field: &str) -> Vec<u8> {
    let marker = format!("'{section}'");
    let start = text
      .find(&marker)
      .unwrap_or_else(|| panic!("missing {section}"));
    let rest = &text[start..];
    let next = rest[1..]
      .find("ml_kem_vectors[")
      .map(|index| index + 1)
      .unwrap_or(rest.len());
    let body = &rest[..next];
    let field_at = body
      .find(&format!("{field}:"))
      .unwrap_or_else(|| panic!("missing {field} in {section}"));
    let after = &body[field_at..];
    let quote = after.find('\'').unwrap();
    let end = after[quote + 1..].find('\'').unwrap() + quote + 1;
    hex(&after[quote + 1..end])
  }

  #[test]
  fn sha3_256_empty_matches_fips() {
    let digest = sha3("SHA3-256", b"").unwrap();
    assert_bytes(
      &digest,
      &hex("a7ffc6f8bf1ed76651c14756a061d662f580ff4de43b49fa82d80a4b80f8434a"),
      "SHA3-256",
    );
    assert_eq!(sha3("SHA3-384", b"").unwrap().len(), 48);
    assert_eq!(sha3("SHA3-512", b"").unwrap().len(), 64);
  }

  #[test]
  fn shake_empty_matches_fips() {
    assert_bytes(
      &shake("SHAKE128", b"", 32).unwrap(),
      &hex("7f9c2ba4e88f827d616045507605853ed73b8093f6efbc88eb1a6eacfa66ef26"),
      "SHAKE128",
    );
    assert_bytes(
      &shake("SHAKE256", b"", 32).unwrap(),
      &hex("46b9dd2b0ba88d13233b3feb743eeb243fcd52ea62b81b82b50c27646ed5762f"),
      "SHAKE256",
    );
    assert!(shake("SHAKE128", b"", 0).is_err());
  }

  #[test]
  fn kmac_samples_match_nist() {
    let key: Vec<u8> = (0x40..=0x5f).collect();
    let message = [0x00, 0x01, 0x02, 0x03];
    assert_bytes(
      &kmac("KMAC128", &key, &message, 32, b"").unwrap(),
      &hex("e5780b0d3ea6f7d3a429c5706aa43a00fadbd7d49628839e3187243f456ee14e"),
      "KMAC128",
    );
    let custom = b"My Tagged Application";
    assert_bytes(
      &kmac("KMAC128", &key, &message, 32, custom).unwrap(),
      &hex("3b1fba963cd8b0b59e8c1a6d71888b7143651af8ba0a7070c0979e2811324aa5"),
      "KMAC128 custom",
    );
  }

  #[test]
  fn argon2_rfc9106_vectors() {
    let password = [0x01u8; 32];
    let salt = [0x02u8; 16];
    let secret = [0x03u8; 8];
    let associated = [0x04u8; 12];
    let cases = [
      (
        "Argon2d",
        "512b391b6f1162975371d30919734294f868e3be3984f3c1a13a4db9fabe4acb",
      ),
      (
        "Argon2i",
        "c814d9d1dc7f37aa13f0d77f2494bda1c8de6b016dd388d29952a4c4672b6ce8",
      ),
      (
        "Argon2id",
        "0d640df58d78766c08c037a34a8b53c9d01ef0452d75b65eb52520e96b01e659",
      ),
    ];
    for (algorithm, expected) in cases {
      let output = argon2(&ArgonInput {
        algorithm,
        password: &password,
        salt: &salt,
        secret: &secret,
        associated: &associated,
        memory: 32,
        passes: 3,
        parallelism: 4,
        output_len: 32,
      })
      .unwrap();
      assert_bytes(&output, &hex(expected), algorithm);
    }
    assert!(
      argon2(&ArgonInput {
        algorithm: "Argon2id",
        password: &password,
        salt: &[0x02; 7],
        secret: &[],
        associated: &[],
        memory: 32,
        passes: 3,
        parallelism: 4,
        output_len: 32,
      })
      .is_err()
    );
  }

  #[test]
  fn aead_round_trip_and_bad_tag() {
    let key = [0x11u8; 32];
    let nonce = [0x22u8; 15];
    let aad = b"associated";
    let plain = b"ocb-plaintext";
    let sealed = ocb_crypt(&key, &nonce, aad, plain, 16, false).unwrap();
    let opened = ocb_crypt(&key, &nonce, aad, &sealed, 16, true).unwrap();
    assert_eq!(opened, plain);
    let mut bad = sealed.clone();
    *bad.last_mut().unwrap() ^= 1;
    assert!(ocb_crypt(&key, &nonce, aad, &bad, 16, true).is_err());
    assert!(ocb_crypt(&key, &[0; 5], aad, plain, 16, false).is_err());
    assert_ne!(
      ocb_crypt(&key, &nonce, b"", plain, 16, false).unwrap(),
      sealed
    );

    let chacha_key = [
      0x80, 0x81, 0x82, 0x83, 0x84, 0x85, 0x86, 0x87, 0x88, 0x89, 0x8a, 0x8b,
      0x8c, 0x8d, 0x8e, 0x8f, 0x90, 0x91, 0x92, 0x93, 0x94, 0x95, 0x96, 0x97,
      0x98, 0x99, 0x9a, 0x9b, 0x9c, 0x9d, 0x9e, 0x9f,
    ];
    let chacha_nonce = [
      0x07, 0x00, 0x00, 0x00, 0x40, 0x41, 0x42, 0x43, 0x44, 0x45, 0x46, 0x47,
    ];
    let chacha_aad = [
      0x50, 0x51, 0x52, 0x53, 0xc0, 0xc1, 0xc2, 0xc3, 0xc4, 0xc5, 0xc6, 0xc7,
    ];
    let chacha_plain = b"Ladies and Gentlemen of the class of '99: \
      If I could offer you only one tip for the future, sunscreen would be it.";
    let sealed = chacha_crypt(
      &chacha_key,
      &chacha_nonce,
      &chacha_aad,
      chacha_plain,
      false,
    )
    .unwrap();
    assert_eq!(sealed.len(), chacha_plain.len() + 16);
    assert_eq!(&sealed[..4], &[0xd3, 0x1a, 0x8d, 0x34]);
    assert_eq!(
      chacha_crypt(&chacha_key, &chacha_nonce, &chacha_aad, &sealed, true)
        .unwrap(),
      chacha_plain
    );
    let mut bad = sealed;
    bad[0] ^= 1;
    assert!(
      chacha_crypt(&chacha_key, &chacha_nonce, &chacha_aad, &bad, true)
        .is_err()
    );
  }

  #[test]
  fn pkcs8_and_spki_round_trip() {
    let seed = [0x11u8; 64];
    let encoded = pkcs8_encode("ML-KEM-512", &seed).unwrap();
    assert_eq!(
      &encoded[..22],
      &[
        0x30, 0x54, 0x02, 0x01, 0x00, 0x30, 0x0b, 0x06, 0x09, 0x60, 0x86, 0x48,
        0x01, 0x65, 0x03, 0x04, 0x04, 0x01, 0x04, 0x42, 0x80, 0x40,
      ]
    );
    assert_eq!(&encoded[22..], &seed);
    let (name, decoded) = pkcs8_decode(&encoded).unwrap();
    assert_eq!(name, "ML-KEM-512");
    assert_eq!(decoded, seed);

    let dsa = [0x22u8; 32];
    let encoded = pkcs8_encode("ML-DSA-44", &dsa).unwrap();
    assert_eq!(&encoded[..4], &[0x30, 0x34, 0x02, 0x01]);
    let (name, decoded) = pkcs8_decode(&encoded).unwrap();
    assert_eq!(name, "ML-DSA-44");
    assert_eq!(decoded, dsa);

    let public = mlkem_public_from_seed("ML-KEM-512", &seed).unwrap();
    let spki = spki_encode("ML-KEM-512", &public).unwrap();
    assert_eq!(
      &spki[..22],
      &[
        0x30, 0x82, 0x03, 0x32, 0x30, 0x0b, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01,
        0x65, 0x03, 0x04, 0x04, 0x01, 0x03, 0x82, 0x03, 0x21, 0x00,
      ]
    );
    let (name, decoded) = spki_decode(&spki).unwrap();
    assert_eq!(name, "ML-KEM-512");
    assert_eq!(decoded, public);
  }

  #[test]
  fn mlkem_vectors_expand_and_decapsulate() {
    let text = suite("WebCryptoAPI/modern-algos/encap_decap/ml_kem_vectors.js");
    for name in ["ML-KEM-512", "ML-KEM-768", "ML-KEM-1024"] {
      let seed = js_field(&text, name, "privateSeed");
      let public = mlkem_public_from_seed(name, &seed).unwrap();
      assert_bytes(&public, &js_field(&text, name, "publicKey"), name);
      let shared = mlkem_decapsulate(
        name,
        &seed,
        &js_field(&text, name, "sampleCiphertext"),
      )
      .unwrap();
      assert_bytes(
        &shared,
        &js_field(&text, name, "expectedSharedSecret"),
        name,
      );
      let (ciphertext, encapsulated) =
        mlkem_encapsulate(name, &public).unwrap();
      assert_eq!(
        mlkem_decapsulate(name, &seed, &ciphertext).unwrap(),
        encapsulated
      );
    }
  }

  #[test]
  fn hybrid_vectors_expand_and_decapsulate() {
    let text =
      suite("WebCryptoAPI/modern-algos/encap_decap/hybrid_kem_vectors.js");
    let spec = hybrid_spec("MLKEM768-P256").unwrap();
    let expanded = expand(&spec, &[0u8; 32]).unwrap();
    match expanded.trad {
      Trad::P256(scalar) => assert_bytes(
        scalar.to_repr().as_slice(),
        &hex(
          "e00b3f9d338de90488973787b0916a4a9ae8bebf4e2bc07a7bc18f1a62215182",
        ),
        "P-256 scalar",
      ),
      _ => panic!("expected a P-256 scalar"),
    }
    for name in ["MLKEM768-P256", "MLKEM768-X25519", "MLKEM1024-P384"] {
      let seed = js_field(&text, name, "privateSeed");
      let public = hybrid_public_from_seed(name, &seed).unwrap();
      assert_bytes(&public, &js_field(&text, name, "publicKey"), name);
      let shared = hybrid_decapsulate(
        name,
        &seed,
        &js_field(&text, name, "sampleCiphertext"),
      )
      .unwrap();
      assert_bytes(
        &shared,
        &js_field(&text, name, "expectedSharedSecret"),
        name,
      );
      let (ciphertext, encapsulated) =
        hybrid_encapsulate(name, &public).unwrap();
      assert_eq!(
        hybrid_decapsulate(name, &seed, &ciphertext).unwrap(),
        encapsulated
      );
    }
  }

  #[test]
  fn mldsa_and_slh_round_trip() {
    let seed = [0x33u8; 32];
    for name in ["ML-DSA-44", "ML-DSA-65", "ML-DSA-87"] {
      let public = mldsa_public_from_seed(name, &seed).unwrap();
      mldsa_validate_public(name, &public).unwrap();
      let signature = mldsa_sign(name, &seed, b"message", b"").unwrap();
      assert!(
        mldsa_verify(name, &public, b"message", &signature, b"").unwrap()
      );
      assert!(!mldsa_verify(name, &public, b"other", &signature, b"").unwrap());
      assert!(mldsa_sign(name, &seed, b"message", &[0u8; 256]).is_err());
    }
    let (private, public) = slh_generate().unwrap();
    assert_eq!(slh_public_from_private(&private).unwrap(), public);
    let signature = slh_sign(&private, b"message", b"ctx").unwrap();
    assert!(slh_verify(&public, b"message", &signature, b"ctx").unwrap());
    assert!(!slh_verify(&public, b"message", &signature, b"").unwrap());
    assert!(slh_sign(&private, b"message", &[0u8; 256]).is_err());
  }
}
