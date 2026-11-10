//! RSA key material for RSASSA-PKCS1-v1_5, RSA-PSS, and RSA-OAEP.
//!
//! Private keys are PKCS#8 DER. Public keys are SPKI DER. Callers that
//! imported those encodings keep the original bytes so export is identical.

use rsa::BoxedUint;
use rsa::Oaep;
use rsa::PrivateKey;
use rsa::PublicKey;
use rsa::pkcs8::DecodePrivateKey;
use rsa::pkcs8::EncodePrivateKey;
use rsa::pkcs8::EncodePublicKey;
use rsa::rand_core::TryCryptoRng;
use rsa::rand_core::TryRng;
use rsa::signature::RandomizedSigner;
use rsa::signature::SignatureEncoding;
use rsa::signature::Verifier;
use rsa::traits::PrivateKeyParts;
use rsa::traits::PublicKeyParts;

/// WebCrypto modulus lengths accepted by this runtime, in bits.
const MODULUS_BITS: std::ops::RangeInclusive<usize> = 256..=16_384;

/// A signature for the largest accepted modulus is 2048 bytes.
const SIGNATURE_LIMIT: usize = 16_384 / 8;

#[derive(Debug)]
pub enum RsaError {
  Operation,
  Data,
  Unsupported,
}

pub struct RsaParts {
  pub modulus_length: u32,
  pub public_exponent: Vec<u8>,
}

pub struct RsaJwk {
  pub n: String,
  pub e: String,
  pub d: Option<String>,
  pub p: Option<String>,
  pub q: Option<String>,
  pub dp: Option<String>,
  pub dq: Option<String>,
  pub qi: Option<String>,
}

pub struct PrivateJwk {
  pub n: Vec<u8>,
  pub e: Vec<u8>,
  pub d: Vec<u8>,
  pub p: Vec<u8>,
  pub q: Vec<u8>,
  pub dp: Vec<u8>,
  pub dq: Vec<u8>,
  pub qi: Vec<u8>,
}

#[derive(Debug)]
struct SysRngError;

impl core::fmt::Display for SysRngError {
  fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    formatter.write_str("getrandom failed")
  }
}

impl core::error::Error for SysRngError {}

struct SysRng;

impl TryRng for SysRng {
  type Error = SysRngError;

  fn try_next_u32(&mut self) -> Result<u32, Self::Error> {
    let mut bytes = [0u8; 4];
    self.try_fill_bytes(&mut bytes)?;
    Ok(u32::from_le_bytes(bytes))
  }

  fn try_next_u64(&mut self) -> Result<u64, Self::Error> {
    let mut bytes = [0u8; 8];
    self.try_fill_bytes(&mut bytes)?;
    Ok(u64::from_le_bytes(bytes))
  }

  fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), Self::Error> {
    getrandom::fill(dest).map_err(|_| SysRngError)
  }
}

impl TryCryptoRng for SysRng {}

pub fn acceptable_exponent(bytes: &[u8]) -> bool {
  let bytes = minimal(bytes);
  bytes == [3] || bytes == [1, 0, 1]
}

pub fn modulus_acceptable(bits: u32) -> bool {
  (256..=16_384).contains(&bits) && bits.is_multiple_of(8)
}

pub fn jwk_alg(algorithm: &str, hash: &str) -> Option<&'static str> {
  Some(match (algorithm, hash) {
    ("RSASSA-PKCS1-v1_5", "SHA-1") => "RS1",
    ("RSASSA-PKCS1-v1_5", "SHA-256") => "RS256",
    ("RSASSA-PKCS1-v1_5", "SHA-384") => "RS384",
    ("RSASSA-PKCS1-v1_5", "SHA-512") => "RS512",
    ("RSA-PSS", "SHA-1") => "PS1",
    ("RSA-PSS", "SHA-256") => "PS256",
    ("RSA-PSS", "SHA-384") => "PS384",
    ("RSA-PSS", "SHA-512") => "PS512",
    ("RSA-OAEP", "SHA-1") => "RSA-OAEP",
    ("RSA-OAEP", "SHA-256") => "RSA-OAEP-256",
    ("RSA-OAEP", "SHA-384") => "RSA-OAEP-384",
    ("RSA-OAEP", "SHA-512") => "RSA-OAEP-512",
    _ => return None,
  })
}

pub fn hash_len(hash: &str) -> Option<usize> {
  Some(match hash {
    "SHA-1" => 20,
    "SHA-256" => 32,
    "SHA-384" => 48,
    "SHA-512" => 64,
    _ => return None,
  })
}

pub fn generate(
  bits: u32,
  exponent: &[u8],
) -> Result<(Vec<u8>, Vec<u8>, RsaParts), RsaError> {
  if !modulus_acceptable(bits) || !acceptable_exponent(exponent) {
    return Err(RsaError::Operation);
  }
  let exponent = uint_from_be(exponent)?;
  let mut rng = SysRng;
  let bits = bits as usize;
  let private = if bits < PublicKey::MIN_SIZE {
    PrivateKey::new_with_exp_unchecked(&mut rng, bits, exponent)
  } else {
    PrivateKey::new_with_exp(&mut rng, bits, exponent)
  }
  .map_err(|_| RsaError::Operation)?;
  if PublicKeyParts::n(&private).bit_width() != bits as u32 {
    return Err(RsaError::Operation);
  }
  let public = private.to_public_key();
  let pkcs8 = private
    .to_pkcs8_der()
    .map_err(|_| RsaError::Operation)?
    .as_bytes()
    .to_vec();
  let spki = public
    .to_public_key_der()
    .map_err(|_| RsaError::Operation)?
    .as_bytes()
    .to_vec();
  Ok((pkcs8, spki, parts_of(&private)))
}

pub fn public_parts(spki: &[u8]) -> Result<RsaParts, RsaError> {
  Ok(parts_public(&public_key(spki)?))
}

pub fn private_parts(pkcs8: &[u8]) -> Result<RsaParts, RsaError> {
  Ok(parts_of(&private_key(pkcs8)?))
}

pub fn import_public_jwk(
  n: &[u8],
  e: &[u8],
) -> Result<(Vec<u8>, RsaParts), RsaError> {
  let key = PublicKey::new_with_size_limits(
    uint_from_be(n)?,
    uint_from_be(e)?,
    MODULUS_BITS,
  )
  .map_err(|_| RsaError::Data)?;
  let spki = key
    .to_public_key_der()
    .map_err(|_| RsaError::Data)?
    .as_bytes()
    .to_vec();
  Ok((spki, parts_public(&key)))
}

pub fn import_private_jwk(
  jwk: &PrivateJwk,
) -> Result<(Vec<u8>, RsaParts), RsaError> {
  let key = PrivateKey::from_components(
    uint_from_be(&jwk.n)?,
    uint_from_be(&jwk.e)?,
    uint_from_be(&jwk.d)?,
    vec![uint_from_be(&jwk.p)?, uint_from_be(&jwk.q)?],
  )
  .map_err(|_| RsaError::Data)?;
  if !same_uint(key.dp(), &jwk.dp)
    || !same_uint(key.dq(), &jwk.dq)
    || !qinv_matches(&key, &jwk.qi)
  {
    return Err(RsaError::Data);
  }
  let pkcs8 = key
    .to_pkcs8_der()
    .map_err(|_| RsaError::Data)?
    .as_bytes()
    .to_vec();
  Ok((pkcs8, parts_of(&key)))
}

pub fn public_jwk(spki: &[u8]) -> Result<RsaJwk, RsaError> {
  let key = public_key(spki)?;
  Ok(RsaJwk {
    n: encode_uint(key.n().as_ref()),
    e: encode_uint(key.e()),
    d: None,
    p: None,
    q: None,
    dp: None,
    dq: None,
    qi: None,
  })
}

pub fn private_jwk(pkcs8: &[u8]) -> Result<RsaJwk, RsaError> {
  let key = private_key(pkcs8)?;
  let primes = key.primes();
  if primes.len() != 2 {
    return Err(RsaError::Data);
  }
  let dp = key.dp().ok_or(RsaError::Data)?;
  let dq = key.dq().ok_or(RsaError::Data)?;
  let qi = key.crt_coefficient().ok_or(RsaError::Data)?;
  Ok(RsaJwk {
    n: encode_uint(key.n().as_ref()),
    e: encode_uint(key.e()),
    d: Some(encode_uint(key.d())),
    p: Some(encode_uint(primes[0].as_ref())),
    q: Some(encode_uint(primes[1].as_ref())),
    dp: Some(encode_uint(dp)),
    dq: Some(encode_uint(dq)),
    qi: Some(encode_uint(&qi)),
  })
}

pub fn sign_pkcs1(
  pkcs8: &[u8],
  hash: &str,
  data: &[u8],
) -> Result<Vec<u8>, RsaError> {
  let key = private_key(pkcs8)?;
  match hash {
    "SHA-1" => pkcs1_sign::<sha1::Sha1>(key, data),
    "SHA-256" => pkcs1_sign::<rsa::sha2::Sha256>(key, data),
    "SHA-384" => pkcs1_sign::<rsa::sha2::Sha384>(key, data),
    "SHA-512" => pkcs1_sign::<rsa::sha2::Sha512>(key, data),
    _ => Err(RsaError::Unsupported),
  }
}

pub fn verify_pkcs1(
  spki: &[u8],
  hash: &str,
  data: &[u8],
  signature: &[u8],
) -> Result<bool, RsaError> {
  let key = public_key(spki)?;
  Ok(match hash {
    "SHA-1" => pkcs1_verify::<sha1::Sha1>(key, data, signature),
    "SHA-256" => pkcs1_verify::<rsa::sha2::Sha256>(key, data, signature),
    "SHA-384" => pkcs1_verify::<rsa::sha2::Sha384>(key, data, signature),
    "SHA-512" => pkcs1_verify::<rsa::sha2::Sha512>(key, data, signature),
    _ => return Err(RsaError::Unsupported),
  })
}

pub fn sign_pss(
  pkcs8: &[u8],
  hash: &str,
  salt_len: usize,
  data: &[u8],
) -> Result<Vec<u8>, RsaError> {
  let key = private_key(pkcs8)?;
  check_salt(key.size(), hash, salt_len)?;
  match hash {
    "SHA-1" => pss_sign::<sha1::Sha1>(key, salt_len, data),
    "SHA-256" => pss_sign::<rsa::sha2::Sha256>(key, salt_len, data),
    "SHA-384" => pss_sign::<rsa::sha2::Sha384>(key, salt_len, data),
    "SHA-512" => pss_sign::<rsa::sha2::Sha512>(key, salt_len, data),
    _ => Err(RsaError::Unsupported),
  }
}

pub fn verify_pss(
  spki: &[u8],
  hash: &str,
  salt_len: usize,
  data: &[u8],
  signature: &[u8],
) -> Result<bool, RsaError> {
  let key = public_key(spki)?;
  check_salt(key.size(), hash, salt_len)?;
  Ok(match hash {
    "SHA-1" => pss_verify::<sha1::Sha1>(key, salt_len, data, signature),
    "SHA-256" => {
      pss_verify::<rsa::sha2::Sha256>(key, salt_len, data, signature)
    }
    "SHA-384" => {
      pss_verify::<rsa::sha2::Sha384>(key, salt_len, data, signature)
    }
    "SHA-512" => {
      pss_verify::<rsa::sha2::Sha512>(key, salt_len, data, signature)
    }
    _ => return Err(RsaError::Unsupported),
  })
}

pub fn encrypt_oaep(
  spki: &[u8],
  hash: &str,
  label: &[u8],
  data: &[u8],
) -> Result<Vec<u8>, RsaError> {
  let key = public_key(spki)?;
  match hash {
    "SHA-1" => oaep_encrypt::<sha1::Sha1>(&key, label, data),
    "SHA-256" => oaep_encrypt::<rsa::sha2::Sha256>(&key, label, data),
    "SHA-384" => oaep_encrypt::<rsa::sha2::Sha384>(&key, label, data),
    "SHA-512" => oaep_encrypt::<rsa::sha2::Sha512>(&key, label, data),
    _ => Err(RsaError::Unsupported),
  }
}

pub fn decrypt_oaep(
  pkcs8: &[u8],
  hash: &str,
  label: &[u8],
  data: &[u8],
) -> Result<Vec<u8>, RsaError> {
  let key = private_key(pkcs8)?;
  match hash {
    "SHA-1" => oaep_decrypt::<sha1::Sha1>(&key, label, data),
    "SHA-256" => oaep_decrypt::<rsa::sha2::Sha256>(&key, label, data),
    "SHA-384" => oaep_decrypt::<rsa::sha2::Sha384>(&key, label, data),
    "SHA-512" => oaep_decrypt::<rsa::sha2::Sha512>(&key, label, data),
    _ => Err(RsaError::Unsupported),
  }
}

fn private_key(pkcs8: &[u8]) -> Result<PrivateKey, RsaError> {
  PrivateKey::from_pkcs8_der(pkcs8).map_err(|_| RsaError::Data)
}

/// `PublicKey::from_public_key_der` only accepts 1024 through 8192 bits.
/// WebCrypto here accepts 256 through 16384, so the components are decoded
/// and checked against that range.
fn public_key(bytes: &[u8]) -> Result<PublicKey, RsaError> {
  use rsa::pkcs1::RsaPublicKeyRef;
  use rsa::pkcs8::SubjectPublicKeyInfoRef;
  use rsa::pkcs8::der::Decode;
  use rsa::pkcs8::der::asn1::Null;

  let info =
    SubjectPublicKeyInfoRef::from_der(bytes).map_err(|_| RsaError::Data)?;
  if info.algorithm.oid != rsa::pkcs1::ALGORITHM_OID {
    return Err(RsaError::Data);
  }
  let parameters = info
    .algorithm
    .parameters_any()
    .map_err(|_| RsaError::Data)?;
  if parameters != Null.into() {
    return Err(RsaError::Data);
  }
  let key_bytes = info.subject_public_key.as_bytes().ok_or(RsaError::Data)?;
  let key = RsaPublicKeyRef::from_der(key_bytes).map_err(|_| RsaError::Data)?;
  let n = uint_from_be(key.modulus.as_bytes())?;
  let e = uint_from_be(key.public_exponent.as_bytes())?;
  PublicKey::new_with_size_limits(n, e, MODULUS_BITS)
    .map_err(|_| RsaError::Data)
}

fn parts_of(key: &PrivateKey) -> RsaParts {
  parts_public(key)
}

fn parts_public(key: &impl PublicKeyParts) -> RsaParts {
  let mut exponent = key.e_bytes().into_vec();
  if exponent.is_empty() {
    exponent.push(0);
  }
  RsaParts {
    modulus_length: key.n().bit_width(),
    public_exponent: exponent,
  }
}

fn encode_uint(value: &BoxedUint) -> String {
  let bytes = value.to_be_bytes_trimmed_vartime();
  let bytes = nonzero_or_zero(bytes.as_ref());
  crate::text::base64::encode_url(bytes)
}

fn same_uint(value: Option<&BoxedUint>, bytes: &[u8]) -> bool {
  value.is_some_and(|value| {
    minimal(value.to_be_bytes_trimmed_vartime().as_ref()) == minimal(bytes)
  })
}

fn qinv_matches(key: &PrivateKey, bytes: &[u8]) -> bool {
  key.crt_coefficient().is_some_and(|value| {
    minimal(value.to_be_bytes_trimmed_vartime().as_ref()) == minimal(bytes)
  })
}

fn uint_from_be(bytes: &[u8]) -> Result<BoxedUint, RsaError> {
  if bytes.is_empty() {
    return Err(RsaError::Data);
  }
  let bits = u32::try_from(bytes.len())
    .ok()
    .and_then(|len| len.checked_mul(8))
    .ok_or(RsaError::Data)?;
  BoxedUint::from_be_slice(bytes, bits).map_err(|_| RsaError::Data)
}

fn minimal(bytes: &[u8]) -> &[u8] {
  match bytes.iter().position(|byte| *byte != 0) {
    Some(index) => &bytes[index..],
    None => &[],
  }
}

fn nonzero_or_zero(bytes: &[u8]) -> &[u8] {
  let bytes = minimal(bytes);
  if bytes.is_empty() { &[0] } else { bytes }
}

fn check_salt(
  modulus_bytes: usize,
  hash: &str,
  salt_len: usize,
) -> Result<(), RsaError> {
  let hash_len = hash_len(hash).ok_or(RsaError::Unsupported)?;
  if modulus_bytes < hash_len + 2 || salt_len > modulus_bytes - hash_len - 2 {
    return Err(RsaError::Operation);
  }
  Ok(())
}

fn pkcs1_sign<D>(key: PrivateKey, data: &[u8]) -> Result<Vec<u8>, RsaError>
where
  D: rsa::sha2::digest::Digest + rsa::pkcs8::AssociatedOid,
{
  let signing = rsa::pkcs1v15::SigningKey::<D>::new(key);
  let signature =
    RandomizedSigner::try_sign_with_rng(&signing, &mut SysRng, data)
      .map_err(|_| RsaError::Operation)?;
  Ok(signature.to_vec())
}

fn pkcs1_verify<D>(key: PublicKey, data: &[u8], signature: &[u8]) -> bool
where
  D: rsa::sha2::digest::Digest + rsa::pkcs8::AssociatedOid,
{
  let Ok(signature) = rsa::pkcs1v15::Signature::from_bytes_with_max_len(
    signature,
    SIGNATURE_LIMIT,
  ) else {
    return false;
  };
  let verifying = rsa::pkcs1v15::VerifyingKey::<D>::new(key);
  Verifier::verify(&verifying, data, &signature).is_ok()
}

fn pss_sign<D>(
  key: PrivateKey,
  salt_len: usize,
  data: &[u8],
) -> Result<Vec<u8>, RsaError>
where
  D: rsa::sha2::digest::Digest + rsa::sha2::digest::FixedOutputReset,
{
  let signing = rsa::pss::SigningKey::<D>::new_with_salt_len(key, salt_len);
  let signature =
    RandomizedSigner::try_sign_with_rng(&signing, &mut SysRng, data)
      .map_err(|_| RsaError::Operation)?;
  Ok(signature.to_vec())
}

fn pss_verify<D>(
  key: PublicKey,
  salt_len: usize,
  data: &[u8],
  signature: &[u8],
) -> bool
where
  D: rsa::sha2::digest::Digest + rsa::sha2::digest::FixedOutputReset,
{
  let Ok(signature) =
    rsa::pss::Signature::from_bytes_with_max_len(signature, SIGNATURE_LIMIT)
  else {
    return false;
  };
  let verifying = rsa::pss::VerifyingKey::<D>::new_with_salt_len(key, salt_len);
  Verifier::verify(&verifying, data, &signature).is_ok()
}

fn oaep_encrypt<D>(
  key: &PublicKey,
  label: &[u8],
  data: &[u8],
) -> Result<Vec<u8>, RsaError>
where
  D: rsa::sha2::digest::Digest + rsa::sha2::digest::FixedOutputReset,
{
  key
    .encrypt(&mut SysRng, oaep_padding::<D>(label), data)
    .map_err(|_| RsaError::Operation)
}

fn oaep_decrypt<D>(
  key: &PrivateKey,
  label: &[u8],
  data: &[u8],
) -> Result<Vec<u8>, RsaError>
where
  D: rsa::sha2::digest::Digest + rsa::sha2::digest::FixedOutputReset,
{
  key
    .decrypt_blinded(&mut SysRng, oaep_padding::<D>(label), data)
    .map_err(|_| RsaError::Operation)
}

fn oaep_padding<D>(label: &[u8]) -> Oaep<D>
where
  D: rsa::sha2::digest::Digest + rsa::sha2::digest::FixedOutputReset,
{
  if label.is_empty() {
    Oaep::new()
  } else {
    Oaep::new_with_label(label)
  }
}

#[cfg(test)]
mod tests {
  use super::PrivateJwk;
  use super::acceptable_exponent;
  use super::decrypt_oaep;
  use super::encrypt_oaep;
  use super::import_private_jwk;
  use super::private_jwk;
  use super::private_key;
  use super::sign_pkcs1;
  use super::sign_pss;
  use super::verify_pkcs1;
  use super::verify_pss;
  use crate::text::base64;
  use rsa::pkcs8::EncodePublicKey;

  fn component(value: &str) -> Vec<u8> {
    base64::decode_url(value).unwrap()
  }

  #[test]
  fn jwk_1024_round_trip_sign_and_oaep() {
    let n = "zZn4sRGfjQos56yL_Qy1R9NI-THMnFynn94g5RxA6wGrJh4BJT3x6I9x0IbpS3q-d4ORA6R2vuDMh8dDFRr9RDH6XY-gUScc9U5Jz3UA2KmVfsCbnUPvcAmMV_ENA7_TF0ivVjuIFodyDTx7EKHNVTrHHSlrbt7spbmcivs23Zc";
    let e = "AQAB";
    let d = "YqIK_GdH85F-GWZdgfgmv15NE78gOaL5h2g4v7DeM9-JC7A5PHSLKNYn87HFGcC4vv0PBIBRtyCA_mJJfEaGWORVCOXSBpWNepMYpio52n3w5uj5UZEsBnbtZc0EtWhVF2Auqa7VbiKrWcQUEgEI8V0gE5D4tyBg8GXv9975dQE";
    let p = "9BrAg5L1zfqGPuWJDuDCBX-TmtZdrOI3Ys4ZaN-yMPlTjwWSEPO0qnfjEZcw2VgXHgJJmbVco6TxckJCmEYqeQ";
    let q = "157jDJ1Ya5nmQvTPbhKAPAeMWogxCyaQTkBrp30pEKd6mGSB385hqr4BIk8s3f7MdXpM-USpaZgUoT4o_2VEjw";
    let dp = "qdd_QUzcaB-6jkKo1Ug-1xKIAgDLFsIjJUUfWt_iHL8ti2Kl2dOnTcCypgebPm5TT1bqHN-agGYAdK5zpX2UiQ";
    let dq = "hNRfwOSplNfhLvxLUN7a2qA3yYm-1MSz_1DWQP7srlLORlUcYPht2FZmsnEeDcAqynBGPQUcbG2Av_hgHz2OZw";
    let qi = "zbpJQAhinrxSbVKxBQ2EZGFUD2e3WCXbAJRYpk8HVQ5AA52OhKTicOye2hEHnrgpFKzC8iznTsCG3FMkvwcj4Q";
    let jwk = PrivateJwk {
      n: component(n),
      e: component(e),
      d: component(d),
      p: component(p),
      q: component(q),
      dp: component(dp),
      dq: component(dq),
      qi: component(qi),
    };
    let (pkcs8, parts) = import_private_jwk(&jwk).expect("import");
    assert_eq!(parts.modulus_length, 1024);
    assert_eq!(parts.public_exponent, vec![1, 0, 1]);
    let exported = private_jwk(&pkcs8).expect("export");
    assert_eq!(exported.n, n);
    assert_eq!(exported.e, e);
    assert_eq!(exported.d.as_deref(), Some(d));
    assert_eq!(exported.p.as_deref(), Some(p));
    assert_eq!(exported.q.as_deref(), Some(q));
    assert_eq!(exported.dp.as_deref(), Some(dp));
    assert_eq!(exported.dq.as_deref(), Some(dq));
    assert_eq!(exported.qi.as_deref(), Some(qi));
    let private = private_key(&pkcs8).unwrap();
    let spki = private
      .to_public_key()
      .to_public_key_der()
      .unwrap()
      .as_bytes()
      .to_vec();
    let data = b"hello yuzuko";
    let signature = sign_pkcs1(&pkcs8, "SHA-256", data).unwrap();
    assert!(verify_pkcs1(&spki, "SHA-256", data, &signature).unwrap());
    assert!(!verify_pkcs1(&spki, "SHA-256", b"other", &signature).unwrap());
    let pss = sign_pss(&pkcs8, "SHA-256", 32, data).unwrap();
    assert!(verify_pss(&spki, "SHA-256", 32, data, &pss).unwrap());
    let label = [0u8; 8];
    let cipher = encrypt_oaep(&spki, "SHA-256", &label, b"secret").unwrap();
    let plain = decrypt_oaep(&pkcs8, "SHA-256", &label, &cipher).unwrap();
    assert_eq!(plain, b"secret");
    assert!(decrypt_oaep(&pkcs8, "SHA-256", b"nope", &cipher).is_err());
  }

  #[test]
  fn rejects_exponents_other_than_3_and_65537() {
    assert!(!acceptable_exponent(&[1]));
    assert!(!acceptable_exponent(&[1, 0, 0]));
    assert!(acceptable_exponent(&[1, 0, 1]));
    assert!(acceptable_exponent(&[0, 1, 0, 1]));
    assert!(acceptable_exponent(&[3]));
  }
}
