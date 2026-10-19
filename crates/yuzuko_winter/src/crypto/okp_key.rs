//! Ed25519, Ed448, X25519, and X448 keys.
//!
//! Private keys are PKCS#8. Public keys are SPKI. `raw` is the public key:
//! 32 bytes for Ed25519 and X25519, 57 for Ed448, and 56 for X448.
//! Ed25519 signatures are 64 bytes and Ed448 signatures are 114 bytes.

use ed25519_dalek::Signer;
use ed25519_dalek::SigningKey;
use ed25519_dalek::VerifyingKey;
use ed25519_dalek::pkcs8::DecodePrivateKey;
use ed25519_dalek::pkcs8::DecodePublicKey;
use ed25519_dalek::pkcs8::EncodePrivateKey;
use ed25519_dalek::pkcs8::EncodePublicKey;
use p256::pkcs8::AlgorithmIdentifierRef;
use p256::pkcs8::ObjectIdentifier;
use p256::pkcs8::PrivateKeyInfo;
use p256::pkcs8::SubjectPublicKeyInfo;
use p256::pkcs8::SubjectPublicKeyInfoRef;
use p256::pkcs8::der::Encode;
use p256::pkcs8::der::asn1::BitStringRef;

#[derive(Debug)]
pub enum OkpError {
  Data,
  Operation,
}

pub struct OkpJwk {
  pub crv: String,
  pub x: String,
  pub d: Option<String>,
  pub alg: Option<String>,
}

const ED25519: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.3.101.112");
const X25519: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.3.101.110");
const ED448: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.3.101.113");
const X448: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.3.101.111");

pub fn generate(algorithm: &str) -> Result<(Vec<u8>, Vec<u8>), OkpError> {
  if algorithm == "Ed448" || algorithm == "X448" {
    return generate_448(algorithm);
  }
  let mut seed = [0u8; 32];
  getrandom::fill(&mut seed).map_err(|_| OkpError::Operation)?;
  match algorithm {
    "Ed25519" => {
      let signing = SigningKey::from_bytes(&seed);
      let private = signing
        .to_pkcs8_der()
        .map_err(|_| OkpError::Operation)?
        .as_bytes()
        .to_vec();
      let public = signing
        .verifying_key()
        .to_public_key_der()
        .map_err(|_| OkpError::Operation)?
        .as_bytes()
        .to_vec();
      Ok((private, public))
    }
    "X25519" => {
      let secret = x25519_dalek::StaticSecret::from(seed);
      let public = x25519_dalek::PublicKey::from(&secret);
      Ok((
        x25519_pkcs8(&seed, public.as_bytes())?,
        x25519_spki(public.as_bytes())?,
      ))
    }
    _ => Err(OkpError::Data),
  }
}

pub fn raw_public(algorithm: &str, spki: &[u8]) -> Result<Vec<u8>, OkpError> {
  if algorithm == "Ed448" {
    return Ok(ed448_public(spki)?.to_vec());
  }
  if algorithm == "X448" {
    return Ok(x448_public(spki)?.to_vec());
  }
  Ok(public_bytes(algorithm, spki)?.to_vec())
}

pub fn import_raw(algorithm: &str, raw: &[u8]) -> Result<Vec<u8>, OkpError> {
  if algorithm == "Ed448" {
    let raw: [u8; 57] = raw.try_into().map_err(|_| OkpError::Data)?;
    let _ = ed448_goldilocks::VerifyingKey::from_bytes(&raw)
      .map_err(|_| OkpError::Data)?;
    return okp_spki(ED448, &raw);
  }
  if algorithm == "X448" {
    let raw: [u8; 56] = raw.try_into().map_err(|_| OkpError::Data)?;
    return okp_spki(X448, &raw);
  }
  let raw: [u8; 32] = raw.try_into().map_err(|_| OkpError::Data)?;
  match algorithm {
    "Ed25519" => {
      let key = VerifyingKey::from_bytes(&raw).map_err(|_| OkpError::Data)?;
      Ok(
        key
          .to_public_key_der()
          .map_err(|_| OkpError::Data)?
          .as_bytes()
          .to_vec(),
      )
    }
    "X25519" => x25519_spki(&raw),
    _ => Err(OkpError::Data),
  }
}

pub fn private_ok(algorithm: &str, pkcs8: &[u8]) -> Result<(), OkpError> {
  if algorithm == "Ed448" || algorithm == "X448" {
    let _ = seed_448(algorithm, pkcs8)?;
    return Ok(());
  }
  let _ = seed_from_pkcs8(algorithm, pkcs8)?;
  Ok(())
}

pub fn public_jwk(algorithm: &str, spki: &[u8]) -> Result<OkpJwk, OkpError> {
  if algorithm == "Ed448" || algorithm == "X448" {
    return Ok(jwk(algorithm, &raw_public(algorithm, spki)?, None));
  }
  Ok(jwk(algorithm, &public_bytes(algorithm, spki)?, None))
}

pub fn private_jwk(algorithm: &str, pkcs8: &[u8]) -> Result<OkpJwk, OkpError> {
  if algorithm == "Ed448" || algorithm == "X448" {
    let seed = seed_448(algorithm, pkcs8)?;
    let public = public_448(algorithm, &seed)?;
    return Ok(jwk(algorithm, &public, Some(&seed)));
  }
  let seed = seed_from_pkcs8(algorithm, pkcs8)?;
  let public = public_from_seed(algorithm, &seed)?;
  Ok(jwk(algorithm, &public, Some(&seed)))
}

pub fn import_public_jwk(
  algorithm: &str,
  x: &[u8],
) -> Result<Vec<u8>, OkpError> {
  import_raw(algorithm, x)
}

pub fn import_private_jwk(
  algorithm: &str,
  x: &[u8],
  d: &[u8],
) -> Result<Vec<u8>, OkpError> {
  if algorithm == "Ed448" || algorithm == "X448" {
    let public = public_448(algorithm, d)?;
    if public != x {
      return Err(OkpError::Data);
    }
    let oid = if algorithm == "Ed448" { ED448 } else { X448 };
    return okp_pkcs8(oid, d, &public);
  }
  let seed: [u8; 32] = d.try_into().map_err(|_| OkpError::Data)?;
  let x: [u8; 32] = x.try_into().map_err(|_| OkpError::Data)?;
  let public = public_from_seed(algorithm, &seed)?;
  if public != x {
    return Err(OkpError::Data);
  }
  match algorithm {
    "Ed25519" => {
      let signing = SigningKey::from_bytes(&seed);
      Ok(
        signing
          .to_pkcs8_der()
          .map_err(|_| OkpError::Data)?
          .as_bytes()
          .to_vec(),
      )
    }
    "X25519" => x25519_pkcs8(&seed, &x),
    _ => Err(OkpError::Data),
  }
}

pub fn sign_ed448(pkcs8: &[u8], message: &[u8]) -> Result<Vec<u8>, OkpError> {
  let seed = seed_448("Ed448", pkcs8)?;
  let signing = ed448_goldilocks::SigningKey::try_from(seed.as_slice())
    .map_err(|_| OkpError::Data)?;
  Ok(signing.sign_raw(message).to_bytes().to_vec())
}

pub fn verify_ed448(
  spki: &[u8],
  message: &[u8],
  signature: &[u8],
) -> Result<bool, OkpError> {
  let raw = ed448_public(spki)?;
  let key = ed448_goldilocks::VerifyingKey::from_bytes(&raw)
    .map_err(|_| OkpError::Data)?;
  let Ok(signature) = ed448_goldilocks::Signature::from_slice(signature) else {
    return Ok(false);
  };
  Ok(key.verify_raw(&signature, message).is_ok())
}

pub fn agree_x448(pkcs8: &[u8], peer_spki: &[u8]) -> Result<Vec<u8>, OkpError> {
  let seed = seed_448("X448", pkcs8)?;
  let seed: [u8; 56] = seed.try_into().map_err(|_| OkpError::Data)?;
  let peer = x448_public(peer_spki)?;
  let shared = x448::x448_unchecked(seed, peer);
  if shared == [0u8; 56] {
    return Err(OkpError::Operation);
  }
  Ok(shared.to_vec())
}

pub fn sign(pkcs8: &[u8], message: &[u8]) -> Result<Vec<u8>, OkpError> {
  let seed = seed_from_pkcs8("Ed25519", pkcs8)?;
  let signing = SigningKey::from_bytes(&seed);
  let signature =
    Signer::try_sign(&signing, message).map_err(|_| OkpError::Operation)?;
  Ok(signature.to_bytes().to_vec())
}

pub fn verify(
  spki: &[u8],
  message: &[u8],
  signature: &[u8],
) -> Result<bool, OkpError> {
  let raw = public_bytes("Ed25519", spki)?;
  let key = VerifyingKey::from_bytes(&raw).map_err(|_| OkpError::Data)?;
  let Ok(signature) = ed25519_dalek::Signature::from_slice(signature) else {
    return Ok(false);
  };
  Ok(key.verify_strict(message, &signature).is_ok())
}

pub fn agree(pkcs8: &[u8], peer_spki: &[u8]) -> Result<Vec<u8>, OkpError> {
  let seed = seed_from_pkcs8("X25519", pkcs8)?;
  let peer = public_bytes("X25519", peer_spki)?;
  let secret = x25519_dalek::StaticSecret::from(seed);
  let shared = secret.diffie_hellman(&x25519_dalek::PublicKey::from(peer));
  let bytes = shared.to_bytes();
  if bytes == [0u8; 32] {
    return Err(OkpError::Operation);
  }
  Ok(bytes.to_vec())
}

fn jwk(algorithm: &str, public: &[u8], seed: Option<&[u8]>) -> OkpJwk {
  OkpJwk {
    crv: algorithm.to_string(),
    x: crate::text::base64::encode_url(public),
    d: seed.map(crate::text::base64::encode_url),
    alg: matches!(algorithm, "Ed25519" | "Ed448")
      .then(|| algorithm.to_string()),
  }
}

fn public_from_seed(
  algorithm: &str,
  seed: &[u8; 32],
) -> Result<[u8; 32], OkpError> {
  Ok(match algorithm {
    "Ed25519" => SigningKey::from_bytes(seed).verifying_key().to_bytes(),
    "X25519" => {
      x25519_dalek::PublicKey::from(&x25519_dalek::StaticSecret::from(*seed))
        .to_bytes()
    }
    _ => return Err(OkpError::Data),
  })
}

fn public_bytes(algorithm: &str, spki: &[u8]) -> Result<[u8; 32], OkpError> {
  match algorithm {
    "Ed25519" => {
      let key =
        VerifyingKey::from_public_key_der(spki).map_err(|_| OkpError::Data)?;
      Ok(key.to_bytes())
    }
    "X25519" => {
      let info =
        SubjectPublicKeyInfoRef::try_from(spki).map_err(|_| OkpError::Data)?;
      if info.algorithm.oid != X25519 {
        return Err(OkpError::Data);
      }
      let raw = info.subject_public_key.as_bytes().ok_or(OkpError::Data)?;
      raw.try_into().map_err(|_| OkpError::Data)
    }
    _ => Err(OkpError::Data),
  }
}

fn seed_from_pkcs8(
  algorithm: &str,
  pkcs8: &[u8],
) -> Result<[u8; 32], OkpError> {
  let info = PrivateKeyInfo::try_from(pkcs8).map_err(|_| OkpError::Data)?;
  let oid = match algorithm {
    "Ed25519" => ED25519,
    "X25519" => X25519,
    _ => return Err(OkpError::Data),
  };
  if info.algorithm.oid != oid {
    return Err(OkpError::Data);
  }
  let seed = seed_from_private_key(info.private_key)?;
  if algorithm == "Ed25519" {
    // Reject a key the signature crate will not load, including a
    // public key that does not match the seed.
    SigningKey::from_pkcs8_der(pkcs8).map_err(|_| OkpError::Data)?;
  } else if let Some(public) = info.public_key {
    let expected = public_from_seed(algorithm, &seed)?;
    if public != expected {
      return Err(OkpError::Data);
    }
  }
  Ok(seed)
}

fn seed_from_private_key(bytes: &[u8]) -> Result<[u8; 32], OkpError> {
  if let Ok(seed) = <[u8; 32]>::try_from(bytes) {
    return Ok(seed);
  }
  if bytes.len() == 34 && bytes[0] == 0x04 && bytes[1] == 0x20 {
    return Ok(bytes[2..].try_into().unwrap());
  }
  Err(OkpError::Data)
}

fn generate_448(algorithm: &str) -> Result<(Vec<u8>, Vec<u8>), OkpError> {
  let len = if algorithm == "Ed448" { 57 } else { 56 };
  let mut seed = vec![0u8; len];
  getrandom::fill(&mut seed).map_err(|_| OkpError::Operation)?;
  let public = public_448(algorithm, &seed)?;
  let oid = if algorithm == "Ed448" { ED448 } else { X448 };
  Ok((okp_pkcs8(oid, &seed, &public)?, okp_spki(oid, &public)?))
}

fn public_448(algorithm: &str, seed: &[u8]) -> Result<Vec<u8>, OkpError> {
  match algorithm {
    "Ed448" => {
      let signing = ed448_goldilocks::SigningKey::try_from(seed)
        .map_err(|_| OkpError::Data)?;
      Ok(signing.verifying_key().to_bytes().to_vec())
    }
    "X448" => {
      let seed: [u8; 56] = seed.try_into().map_err(|_| OkpError::Data)?;
      Ok(x448::x448_unchecked(seed, x448::X448_BASEPOINT_BYTES).to_vec())
    }
    _ => Err(OkpError::Data),
  }
}

fn seed_448(algorithm: &str, pkcs8: &[u8]) -> Result<Vec<u8>, OkpError> {
  let (oid, len) = match algorithm {
    "Ed448" => (ED448, 57),
    "X448" => (X448, 56),
    _ => return Err(OkpError::Data),
  };
  let info = PrivateKeyInfo::try_from(pkcs8).map_err(|_| OkpError::Data)?;
  if info.algorithm.oid != oid {
    return Err(OkpError::Data);
  }
  let seed = extract_seed(info.private_key, len)?;
  if let Some(public) = info.public_key {
    let expected = public_448(algorithm, &seed)?;
    if public != expected {
      return Err(OkpError::Data);
    }
  }
  if algorithm == "Ed448" {
    ed448_goldilocks::SigningKey::try_from(seed.as_slice())
      .map_err(|_| OkpError::Data)?;
  }
  Ok(seed)
}

fn extract_seed(bytes: &[u8], len: usize) -> Result<Vec<u8>, OkpError> {
  if bytes.len() == len {
    return Ok(bytes.to_vec());
  }
  if bytes.len() == len + 2 && bytes[0] == 0x04 && bytes[1] as usize == len {
    return Ok(bytes[2..].to_vec());
  }
  Err(OkpError::Data)
}

fn ed448_public(spki: &[u8]) -> Result<[u8; 57], OkpError> {
  point_bytes(spki, ED448)
}

fn x448_public(spki: &[u8]) -> Result<[u8; 56], OkpError> {
  point_bytes(spki, X448)
}

fn point_bytes<const N: usize>(
  spki: &[u8],
  oid: ObjectIdentifier,
) -> Result<[u8; N], OkpError> {
  let info =
    SubjectPublicKeyInfoRef::try_from(spki).map_err(|_| OkpError::Data)?;
  if info.algorithm.oid != oid {
    return Err(OkpError::Data);
  }
  let raw = info.subject_public_key.as_bytes().ok_or(OkpError::Data)?;
  raw.try_into().map_err(|_| OkpError::Data)
}

fn okp_spki(oid: ObjectIdentifier, public: &[u8]) -> Result<Vec<u8>, OkpError> {
  let bits = BitStringRef::new(0, public).map_err(|_| OkpError::Operation)?;
  let info = SubjectPublicKeyInfo {
    algorithm: algorithm(oid),
    subject_public_key: bits,
  };
  info.to_der().map_err(|_| OkpError::Operation)
}

fn okp_pkcs8(
  oid: ObjectIdentifier,
  seed: &[u8],
  public: &[u8],
) -> Result<Vec<u8>, OkpError> {
  let mut private_key = Vec::with_capacity(seed.len() + 2);
  private_key.push(0x04);
  private_key.push(seed.len() as u8);
  private_key.extend_from_slice(seed);
  let info = PrivateKeyInfo {
    algorithm: algorithm(oid),
    private_key: &private_key,
    public_key: Some(public),
  };
  info.to_der().map_err(|_| OkpError::Operation)
}

fn algorithm(oid: ObjectIdentifier) -> AlgorithmIdentifierRef<'static> {
  AlgorithmIdentifierRef {
    oid,
    parameters: None,
  }
}

fn x25519_spki(public: &[u8; 32]) -> Result<Vec<u8>, OkpError> {
  let bits = BitStringRef::new(0, public).map_err(|_| OkpError::Operation)?;
  let info = SubjectPublicKeyInfo {
    algorithm: algorithm(X25519),
    subject_public_key: bits,
  };
  info.to_der().map_err(|_| OkpError::Operation)
}

fn x25519_pkcs8(
  seed: &[u8; 32],
  public: &[u8; 32],
) -> Result<Vec<u8>, OkpError> {
  let mut private_key = Vec::with_capacity(34);
  private_key.extend_from_slice(&[0x04, 0x20]);
  private_key.extend_from_slice(seed);
  let info = PrivateKeyInfo {
    algorithm: algorithm(X25519),
    private_key: &private_key,
    public_key: Some(public),
  };
  info.to_der().map_err(|_| OkpError::Operation)
}

#[cfg(test)]
mod tests {
  use super::agree;
  use super::generate;
  use super::private_jwk;
  use super::raw_public;
  use super::sign;
  use super::verify;

  fn hex(text: &str) -> Vec<u8> {
    (0..text.len())
      .step_by(2)
      .map(|index| u8::from_str_radix(&text[index..index + 2], 16).unwrap())
      .collect()
  }

  #[test]
  fn ed25519_leading_zero_seed_signs() {
    let pkcs8 = hex(
      "302e020100300506032b6570042204200007070707070707070707070707070707070707070707070707070707070707",
    );
    let spki = hex(
      "302a300506032b6570032100ff812f84e53570c36ae9479aad71e649c3cc4a04a6768632150febdd66a238a8",
    );
    let signature = sign(&pkcs8, &[1, 2, 3]).unwrap();
    assert_eq!(
      signature,
      hex(
        "94ff57055f3b90728d704bfbf19a3e4b49806f98a10384ae01ed5c700e6d482a12dd5f6372ce160f5cf94e8cd53555a7b525658d8f4126d463fa4200578d710e"
      )
    );
    assert!(verify(&spki, &[1, 2, 3], &signature).unwrap());
    assert_eq!(
      raw_public("Ed25519", &spki).unwrap(),
      hex("ff812f84e53570c36ae9479aad71e649c3cc4a04a6768632150febdd66a238a8")
    );
  }

  #[test]
  fn x25519_shared_secret_matches() {
    let pkcs8 = hex(
      "302e020100300506032b656e042204200007070707070707070707070707070707070707070707070707070707070707",
    );
    let peer = hex(
      "302a300506032b656e032100a1d178cd56e2528f7b8d82ba2e3772b219c944f0e8fbfb0d1ca3951d0ce00b13",
    );
    let shared = agree(&pkcs8, &peer).unwrap();
    assert_eq!(
      shared,
      hex("51eafdc7ed3371472837de1aca43bee0b198aa02d1f619eb26ed1d1045bcd936")
    );
    let jwk = private_jwk("X25519", &pkcs8).unwrap();
    assert!(jwk.alg.is_none());
    assert!(jwk.d.is_some());
  }

  #[test]
  fn generated_keys_round_trip() {
    let (private, public) = generate("Ed25519").unwrap();
    let message = b"yuzuko";
    let signature = sign(&private, message).unwrap();
    assert!(verify(&public, message, &signature).unwrap());
    let (left, left_public) = generate("X25519").unwrap();
    let (right, right_public) = generate("X25519").unwrap();
    assert_eq!(
      agree(&left, &right_public).unwrap(),
      agree(&right, &left_public).unwrap()
    );
  }
}
