//! NIST P-256, P-384, and P-521 keys for ECDSA and ECDH.
//!
//! Private keys are PKCS#8. Public keys are SPKI. `raw` is the uncompressed
//! SEC1 point, `04 || x || y`.

use p256::elliptic_curve::rand_core::CryptoRng;
use p256::elliptic_curve::rand_core::RngCore;
use p256::pkcs8::DecodePrivateKey;
use p256::pkcs8::DecodePublicKey;
use p256::pkcs8::EncodePrivateKey;
use p256::pkcs8::EncodePublicKey;

#[derive(Debug)]
pub enum EcError {
  Data,
  Operation,
  Unsupported,
}

pub struct EcJwk {
  pub crv: String,
  pub x: String,
  pub y: String,
  pub d: Option<String>,
}

struct SysRng;

impl RngCore for SysRng {
  fn next_u32(&mut self) -> u32 {
    let mut bytes = [0u8; 4];
    self.fill_bytes(&mut bytes);
    u32::from_le_bytes(bytes)
  }

  fn next_u64(&mut self) -> u64 {
    let mut bytes = [0u8; 8];
    self.fill_bytes(&mut bytes);
    u64::from_le_bytes(bytes)
  }

  fn fill_bytes(&mut self, dest: &mut [u8]) {
    if self.try_fill_bytes(dest).is_err() {
      panic!("getrandom failed");
    }
  }

  fn try_fill_bytes(
    &mut self,
    dest: &mut [u8],
  ) -> Result<(), p256::elliptic_curve::rand_core::Error> {
    getrandom::fill(dest).map_err(|_| {
      p256::elliptic_curve::rand_core::Error::new(std::io::Error::other(
        "getrandom failed",
      ))
    })
  }
}

impl CryptoRng for SysRng {}

pub fn coordinate_len(curve: &str) -> Option<usize> {
  Some(match curve {
    "P-256" => 32,
    "P-384" => 48,
    "P-521" => 66,
    _ => return None,
  })
}

pub fn generate(curve: &str) -> Result<(Vec<u8>, Vec<u8>), EcError> {
  match curve {
    "P-256" => generate_p256(),
    "P-384" => generate_p384(),
    "P-521" => generate_p521(),
    _ => Err(EcError::Unsupported),
  }
}

pub fn raw_public(curve: &str, spki: &[u8]) -> Result<Vec<u8>, EcError> {
  match curve {
    "P-256" => raw_p256(spki),
    "P-384" => raw_p384(spki),
    "P-521" => raw_p521(spki),
    _ => Err(EcError::Unsupported),
  }
}

pub fn import_raw(curve: &str, raw: &[u8]) -> Result<Vec<u8>, EcError> {
  let len = coordinate_len(curve).ok_or(EcError::Unsupported)?;
  if raw.len() != 1 + len * 2 || raw.first() != Some(&0x04) {
    return Err(EcError::Data);
  }
  match curve {
    "P-256" => import_raw_p256(raw),
    "P-384" => import_raw_p384(raw),
    "P-521" => import_raw_p521(raw),
    _ => Err(EcError::Unsupported),
  }
}

pub fn private_ok(curve: &str, pkcs8: &[u8]) -> Result<(), EcError> {
  // Parsing also checks that the curve OID matches.
  let _ = raw_from_private(curve, pkcs8)?;
  Ok(())
}

/// SPKI with the public point in uncompressed form.
///
/// A compressed point is re-encoded. An uncompressed point is kept as the
/// original bytes so export stays byte-identical.
pub fn normalize_spki(curve: &str, spki: &[u8]) -> Result<Vec<u8>, EcError> {
  match curve {
    "P-256" => normalize_spki_curve::<p256::NistP256>(spki),
    "P-384" => normalize_spki_curve::<p384::NistP384>(spki),
    "P-521" => normalize_spki_curve::<p521::NistP521>(spki),
    _ => Err(EcError::Unsupported),
  }
}

fn normalize_spki_curve<C>(spki: &[u8]) -> Result<Vec<u8>, EcError>
where
  C: p256::elliptic_curve::CurveArithmetic + p256::pkcs8::AssociatedOid,
  p256::elliptic_curve::AffinePoint<C>:
    p256::elliptic_curve::sec1::FromEncodedPoint<C>
      + p256::elliptic_curve::sec1::ToEncodedPoint<C>,
  p256::elliptic_curve::FieldBytesSize<C>:
    p256::elliptic_curve::sec1::ModulusSize,
{
  use p256::elliptic_curve::sec1::ToEncodedPoint;
  let key = p256::elliptic_curve::PublicKey::<C>::from_public_key_der(spki)
    .map_err(|_| EcError::Data)?;
  let point = key.to_encoded_point(false);
  let point = point.as_bytes();
  if spki.windows(point.len()).any(|window| window == point) {
    return Ok(spki.to_vec());
  }
  Ok(
    key
      .to_public_key_der()
      .map_err(|_| EcError::Data)?
      .as_bytes()
      .to_vec(),
  )
}

pub fn public_jwk(curve: &str, spki: &[u8]) -> Result<EcJwk, EcError> {
  let raw = raw_public(curve, spki)?;
  let (x, y) = split_point(&raw, coordinate_len(curve).unwrap_or(0))?;
  Ok(EcJwk {
    crv: curve.to_string(),
    x: crate::text::base64::encode_url(x),
    y: crate::text::base64::encode_url(y),
    d: None,
  })
}

pub fn private_jwk(curve: &str, pkcs8: &[u8]) -> Result<EcJwk, EcError> {
  let (raw, d) = raw_from_private(curve, pkcs8)?;
  let (x, y) = split_point(&raw, coordinate_len(curve).unwrap_or(0))?;
  Ok(EcJwk {
    crv: curve.to_string(),
    x: crate::text::base64::encode_url(x),
    y: crate::text::base64::encode_url(y),
    d: Some(crate::text::base64::encode_url(&d)),
  })
}

pub fn import_public_jwk(
  curve: &str,
  x: &[u8],
  y: &[u8],
) -> Result<Vec<u8>, EcError> {
  let point = uncompressed(curve, x, y)?;
  import_raw(curve, &point)
}

pub fn import_private_jwk(
  curve: &str,
  x: &[u8],
  y: &[u8],
  d: &[u8],
) -> Result<Vec<u8>, EcError> {
  let point = uncompressed(curve, x, y)?;
  match curve {
    "P-256" => import_private_p256(&point, d),
    "P-384" => import_private_p384(&point, d),
    "P-521" => import_private_p521(&point, d),
    _ => Err(EcError::Unsupported),
  }
}

pub fn sign(
  curve: &str,
  pkcs8: &[u8],
  digest: &[u8],
) -> Result<Vec<u8>, EcError> {
  let digest = fit_digest(curve, digest)?;
  match curve {
    "P-256" => sign_p256(pkcs8, &digest),
    "P-384" => sign_p384(pkcs8, &digest),
    "P-521" => sign_p521(pkcs8, &digest),
    _ => Err(EcError::Unsupported),
  }
}

pub fn verify(
  curve: &str,
  spki: &[u8],
  digest: &[u8],
  signature: &[u8],
) -> Result<bool, EcError> {
  let digest = fit_digest(curve, digest)?;
  match curve {
    "P-256" => verify_p256(spki, &digest, signature),
    "P-384" => verify_p384(spki, &digest, signature),
    "P-521" => verify_p521(spki, &digest, signature),
    _ => Err(EcError::Unsupported),
  }
}

/// ECDSA reads the digest as an integer. A short hash is left-padded to the
/// field size, and a long hash keeps its leftmost field bytes. The `ecdsa`
/// crate rejects a prehash shorter than half the field, so expand it first.
fn fit_digest(curve: &str, digest: &[u8]) -> Result<Vec<u8>, EcError> {
  let len = coordinate_len(curve).ok_or(EcError::Unsupported)?;
  let mut fitted = vec![0u8; len];
  if digest.len() >= len {
    fitted.copy_from_slice(&digest[..len]);
  } else {
    fitted[len - digest.len()..].copy_from_slice(digest);
  }
  Ok(fitted)
}

pub fn agree(
  curve: &str,
  pkcs8: &[u8],
  peer_spki: &[u8],
) -> Result<Vec<u8>, EcError> {
  match curve {
    "P-256" => agree_p256(pkcs8, peer_spki),
    "P-384" => agree_p384(pkcs8, peer_spki),
    "P-521" => agree_p521(pkcs8, peer_spki),
    _ => Err(EcError::Unsupported),
  }
}

fn uncompressed(curve: &str, x: &[u8], y: &[u8]) -> Result<Vec<u8>, EcError> {
  let len = coordinate_len(curve).ok_or(EcError::Unsupported)?;
  let x = pad(x, len)?;
  let y = pad(y, len)?;
  let mut point = Vec::with_capacity(1 + len * 2);
  point.push(0x04);
  point.extend(x);
  point.extend(y);
  Ok(point)
}

fn pad(bytes: &[u8], len: usize) -> Result<Vec<u8>, EcError> {
  if bytes.is_empty() || bytes.len() > len {
    return Err(EcError::Data);
  }
  let mut out = vec![0u8; len];
  out[len - bytes.len()..].copy_from_slice(bytes);
  Ok(out)
}

fn split_point(raw: &[u8], len: usize) -> Result<(&[u8], &[u8]), EcError> {
  if raw.len() != 1 + len * 2 || raw.first() != Some(&0x04) {
    return Err(EcError::Data);
  }
  Ok((&raw[1..1 + len], &raw[1 + len..]))
}

fn generate_p256() -> Result<(Vec<u8>, Vec<u8>), EcError> {
  let secret = p256::SecretKey::random(&mut SysRng);
  let private = secret.to_pkcs8_der().map_err(|_| EcError::Operation)?;
  let public = secret
    .public_key()
    .to_public_key_der()
    .map_err(|_| EcError::Operation)?;
  Ok((private.as_bytes().to_vec(), public.as_bytes().to_vec()))
}

fn generate_p384() -> Result<(Vec<u8>, Vec<u8>), EcError> {
  let secret = p384::SecretKey::random(&mut SysRng);
  let private = secret.to_pkcs8_der().map_err(|_| EcError::Operation)?;
  let public = secret
    .public_key()
    .to_public_key_der()
    .map_err(|_| EcError::Operation)?;
  Ok((private.as_bytes().to_vec(), public.as_bytes().to_vec()))
}

fn generate_p521() -> Result<(Vec<u8>, Vec<u8>), EcError> {
  let secret = p521::SecretKey::random(&mut SysRng);
  let private = secret.to_pkcs8_der().map_err(|_| EcError::Operation)?;
  let public = secret
    .public_key()
    .to_public_key_der()
    .map_err(|_| EcError::Operation)?;
  Ok((private.as_bytes().to_vec(), public.as_bytes().to_vec()))
}

fn raw_p256(spki: &[u8]) -> Result<Vec<u8>, EcError> {
  let key =
    p256::PublicKey::from_public_key_der(spki).map_err(|_| EcError::Data)?;
  Ok(key.to_sec1_bytes().into_vec())
}

fn raw_p384(spki: &[u8]) -> Result<Vec<u8>, EcError> {
  let key =
    p384::PublicKey::from_public_key_der(spki).map_err(|_| EcError::Data)?;
  Ok(key.to_sec1_bytes().into_vec())
}

fn raw_p521(spki: &[u8]) -> Result<Vec<u8>, EcError> {
  let key =
    p521::PublicKey::from_public_key_der(spki).map_err(|_| EcError::Data)?;
  Ok(key.to_sec1_bytes().into_vec())
}

fn import_raw_p256(raw: &[u8]) -> Result<Vec<u8>, EcError> {
  let key = p256::PublicKey::from_sec1_bytes(raw).map_err(|_| EcError::Data)?;
  Ok(
    key
      .to_public_key_der()
      .map_err(|_| EcError::Data)?
      .as_bytes()
      .to_vec(),
  )
}

fn import_raw_p384(raw: &[u8]) -> Result<Vec<u8>, EcError> {
  let key = p384::PublicKey::from_sec1_bytes(raw).map_err(|_| EcError::Data)?;
  Ok(
    key
      .to_public_key_der()
      .map_err(|_| EcError::Data)?
      .as_bytes()
      .to_vec(),
  )
}

fn import_raw_p521(raw: &[u8]) -> Result<Vec<u8>, EcError> {
  let key = p521::PublicKey::from_sec1_bytes(raw).map_err(|_| EcError::Data)?;
  Ok(
    key
      .to_public_key_der()
      .map_err(|_| EcError::Data)?
      .as_bytes()
      .to_vec(),
  )
}

fn raw_from_private(
  curve: &str,
  pkcs8: &[u8],
) -> Result<(Vec<u8>, Vec<u8>), EcError> {
  match curve {
    "P-256" => {
      let secret =
        p256::SecretKey::from_pkcs8_der(pkcs8).map_err(|_| EcError::Data)?;
      Ok((
        secret.public_key().to_sec1_bytes().into_vec(),
        secret.to_bytes().to_vec(),
      ))
    }
    "P-384" => {
      let secret =
        p384::SecretKey::from_pkcs8_der(pkcs8).map_err(|_| EcError::Data)?;
      Ok((
        secret.public_key().to_sec1_bytes().into_vec(),
        secret.to_bytes().to_vec(),
      ))
    }
    "P-521" => {
      let secret =
        p521::SecretKey::from_pkcs8_der(pkcs8).map_err(|_| EcError::Data)?;
      Ok((
        secret.public_key().to_sec1_bytes().into_vec(),
        secret.to_bytes().to_vec(),
      ))
    }
    _ => Err(EcError::Unsupported),
  }
}

fn import_private_p256(point: &[u8], d: &[u8]) -> Result<Vec<u8>, EcError> {
  let len = 32;
  let d = pad(d, len)?;
  let secret = p256::SecretKey::from_slice(&d).map_err(|_| EcError::Data)?;
  if secret.public_key().to_sec1_bytes().as_ref() != point {
    return Err(EcError::Data);
  }
  Ok(
    secret
      .to_pkcs8_der()
      .map_err(|_| EcError::Data)?
      .as_bytes()
      .to_vec(),
  )
}

fn import_private_p384(point: &[u8], d: &[u8]) -> Result<Vec<u8>, EcError> {
  let d = pad(d, 48)?;
  let secret = p384::SecretKey::from_slice(&d).map_err(|_| EcError::Data)?;
  if secret.public_key().to_sec1_bytes().as_ref() != point {
    return Err(EcError::Data);
  }
  Ok(
    secret
      .to_pkcs8_der()
      .map_err(|_| EcError::Data)?
      .as_bytes()
      .to_vec(),
  )
}

fn import_private_p521(point: &[u8], d: &[u8]) -> Result<Vec<u8>, EcError> {
  let d = pad(d, 66)?;
  let secret = p521::SecretKey::from_slice(&d).map_err(|_| EcError::Data)?;
  if secret.public_key().to_sec1_bytes().as_ref() != point {
    return Err(EcError::Data);
  }
  Ok(
    secret
      .to_pkcs8_der()
      .map_err(|_| EcError::Data)?
      .as_bytes()
      .to_vec(),
  )
}

fn sign_p256(pkcs8: &[u8], digest: &[u8]) -> Result<Vec<u8>, EcError> {
  use p256::ecdsa::signature::hazmat::RandomizedPrehashSigner;
  let secret =
    p256::SecretKey::from_pkcs8_der(pkcs8).map_err(|_| EcError::Data)?;
  let signing =
    p256::ecdsa::SigningKey::from_slice(secret.to_bytes().as_slice())
      .map_err(|_| EcError::Data)?;
  let signature: p256::ecdsa::Signature =
    RandomizedPrehashSigner::sign_prehash_with_rng(
      &signing,
      &mut SysRng,
      digest,
    )
    .map_err(|_| EcError::Operation)?;
  Ok(signature.to_bytes().as_slice().to_vec())
}

fn sign_p384(pkcs8: &[u8], digest: &[u8]) -> Result<Vec<u8>, EcError> {
  use p384::ecdsa::signature::hazmat::RandomizedPrehashSigner;
  let secret =
    p384::SecretKey::from_pkcs8_der(pkcs8).map_err(|_| EcError::Data)?;
  let signing =
    p384::ecdsa::SigningKey::from_slice(secret.to_bytes().as_slice())
      .map_err(|_| EcError::Data)?;
  let signature: p384::ecdsa::Signature =
    RandomizedPrehashSigner::sign_prehash_with_rng(
      &signing,
      &mut SysRng,
      digest,
    )
    .map_err(|_| EcError::Operation)?;
  Ok(signature.to_bytes().as_slice().to_vec())
}

fn sign_p521(pkcs8: &[u8], digest: &[u8]) -> Result<Vec<u8>, EcError> {
  use p521::ecdsa::signature::hazmat::RandomizedPrehashSigner;
  let secret =
    p521::SecretKey::from_pkcs8_der(pkcs8).map_err(|_| EcError::Data)?;
  let signing =
    p521::ecdsa::SigningKey::from_slice(secret.to_bytes().as_slice())
      .map_err(|_| EcError::Data)?;
  let signature: p521::ecdsa::Signature =
    RandomizedPrehashSigner::sign_prehash_with_rng(
      &signing,
      &mut SysRng,
      digest,
    )
    .map_err(|_| EcError::Operation)?;
  Ok(signature.to_bytes().as_slice().to_vec())
}

fn verify_p256(
  spki: &[u8],
  digest: &[u8],
  signature: &[u8],
) -> Result<bool, EcError> {
  use p256::ecdsa::signature::hazmat::PrehashVerifier;
  let key =
    p256::PublicKey::from_public_key_der(spki).map_err(|_| EcError::Data)?;
  let verifying =
    p256::ecdsa::VerifyingKey::from_sec1_bytes(&key.to_sec1_bytes())
      .map_err(|_| EcError::Data)?;
  let Ok(signature) = p256::ecdsa::Signature::from_slice(signature) else {
    return Ok(false);
  };
  Ok(verifying.verify_prehash(digest, &signature).is_ok())
}

fn verify_p384(
  spki: &[u8],
  digest: &[u8],
  signature: &[u8],
) -> Result<bool, EcError> {
  use p384::ecdsa::signature::hazmat::PrehashVerifier;
  let key =
    p384::PublicKey::from_public_key_der(spki).map_err(|_| EcError::Data)?;
  let verifying =
    p384::ecdsa::VerifyingKey::from_sec1_bytes(&key.to_sec1_bytes())
      .map_err(|_| EcError::Data)?;
  let Ok(signature) = p384::ecdsa::Signature::from_slice(signature) else {
    return Ok(false);
  };
  Ok(verifying.verify_prehash(digest, &signature).is_ok())
}

fn verify_p521(
  spki: &[u8],
  digest: &[u8],
  signature: &[u8],
) -> Result<bool, EcError> {
  use p521::ecdsa::signature::hazmat::PrehashVerifier;
  let key =
    p521::PublicKey::from_public_key_der(spki).map_err(|_| EcError::Data)?;
  let verifying =
    p521::ecdsa::VerifyingKey::from_sec1_bytes(&key.to_sec1_bytes())
      .map_err(|_| EcError::Data)?;
  let Ok(signature) = p521::ecdsa::Signature::from_slice(signature) else {
    return Ok(false);
  };
  Ok(verifying.verify_prehash(digest, &signature).is_ok())
}

fn agree_p256(pkcs8: &[u8], peer: &[u8]) -> Result<Vec<u8>, EcError> {
  let secret =
    p256::SecretKey::from_pkcs8_der(pkcs8).map_err(|_| EcError::Data)?;
  let public =
    p256::PublicKey::from_public_key_der(peer).map_err(|_| EcError::Data)?;
  let shared = p256::elliptic_curve::ecdh::diffie_hellman(
    secret.to_nonzero_scalar(),
    public.as_affine(),
  );
  Ok(shared.raw_secret_bytes().to_vec())
}

fn agree_p384(pkcs8: &[u8], peer: &[u8]) -> Result<Vec<u8>, EcError> {
  let secret =
    p384::SecretKey::from_pkcs8_der(pkcs8).map_err(|_| EcError::Data)?;
  let public =
    p384::PublicKey::from_public_key_der(peer).map_err(|_| EcError::Data)?;
  let shared = p384::elliptic_curve::ecdh::diffie_hellman(
    secret.to_nonzero_scalar(),
    public.as_affine(),
  );
  Ok(shared.raw_secret_bytes().to_vec())
}

fn agree_p521(pkcs8: &[u8], peer: &[u8]) -> Result<Vec<u8>, EcError> {
  let secret =
    p521::SecretKey::from_pkcs8_der(pkcs8).map_err(|_| EcError::Data)?;
  let public =
    p521::PublicKey::from_public_key_der(peer).map_err(|_| EcError::Data)?;
  let shared = p521::elliptic_curve::ecdh::diffie_hellman(
    secret.to_nonzero_scalar(),
    public.as_affine(),
  );
  Ok(shared.raw_secret_bytes().to_vec())
}

#[cfg(test)]
mod tests {
  use super::agree;
  use super::generate;
  use super::sign;
  use super::verify;
  use sha2::Digest;
  use sha2::Sha256;

  #[test]
  fn p256_sign_and_agree() {
    let (private, public) = generate("P-256").unwrap();
    let (other_private, other_public) = generate("P-256").unwrap();
    let digest = Sha256::digest(b"hello").to_vec();
    let signature = sign("P-256", &private, &digest).unwrap();
    assert!(verify("P-256", &public, &digest, &signature).unwrap());
    assert!(
      !verify("P-256", &public, &Sha256::digest(b"nope"), &signature).unwrap()
    );
    let left = agree("P-256", &private, &other_public).unwrap();
    let right = agree("P-256", &other_private, &public).unwrap();
    assert_eq!(left, right);
    assert_eq!(left.len(), 32);
  }

  #[test]
  fn short_digest_round_trip() {
    // SHA-1 is 20 bytes. That is below half of P-384 (24) and P-521 (33).
    // SHA-256 is 32 bytes, still short of half of P-521.
    let cases = [("P-384", 20usize), ("P-521", 20), ("P-521", 32)];
    for (curve, len) in cases {
      let (private, public) = generate(curve).unwrap();
      let digest = vec![0xabu8; len];
      let signature = sign(curve, &private, &digest).unwrap();
      assert!(verify(curve, &public, &digest, &signature).unwrap());
      let mut other = digest.clone();
      other[0] ^= 0xff;
      assert!(!verify(curve, &public, &other, &signature).unwrap());
    }
  }
}
