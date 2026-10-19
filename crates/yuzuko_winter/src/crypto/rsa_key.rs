//! RSA key material for RSASSA-PKCS1-v1_5, RSA-PSS, and RSA-OAEP.
//!
//! Private keys are PKCS#8 DER. Public keys are SPKI DER. Callers that
//! imported those encodings keep the original bytes so export is identical.

use rsa::BigUint;
use rsa::Oaep;
use rsa::RsaPrivateKey;
use rsa::RsaPublicKey;
use rsa::pkcs8::DecodePrivateKey;
use rsa::pkcs8::DecodePublicKey;
use rsa::pkcs8::EncodePrivateKey;
use rsa::pkcs8::EncodePublicKey;
use rsa::rand_core::CryptoRng;
use rsa::rand_core::RngCore;
use rsa::signature::RandomizedSigner;
use rsa::signature::SignatureEncoding;
use rsa::signature::Verifier;
use rsa::traits::PrivateKeyParts;
use rsa::traits::PublicKeyParts;

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
  ) -> Result<(), rsa::rand_core::Error> {
    getrandom::fill(dest).map_err(|_| {
      rsa::rand_core::Error::new(std::io::Error::other("getrandom failed"))
    })
  }
}

impl CryptoRng for SysRng {}

pub fn acceptable_exponent(bytes: &[u8]) -> bool {
  let value = BigUint::from_bytes_be(bytes);
  value == BigUint::from(3u32) || value == BigUint::from(65_537u32)
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
  let exponent = BigUint::from_bytes_be(exponent);
  let mut rng = SysRng;
  let private = RsaPrivateKey::new_with_exp(&mut rng, bits as usize, &exponent)
    .map_err(|_| RsaError::Operation)?;
  if private.n().bits() != bits as usize {
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
  let key =
    RsaPublicKey::from_public_key_der(spki).map_err(|_| RsaError::Data)?;
  Ok(parts_public(&key))
}

pub fn private_parts(pkcs8: &[u8]) -> Result<RsaParts, RsaError> {
  let key = private_key(pkcs8)?;
  Ok(parts_of(&key))
}

pub fn import_public_jwk(
  n: &[u8],
  e: &[u8],
) -> Result<(Vec<u8>, RsaParts), RsaError> {
  let key =
    RsaPublicKey::new(BigUint::from_bytes_be(n), BigUint::from_bytes_be(e))
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
  let key = RsaPrivateKey::from_components(
    BigUint::from_bytes_be(&jwk.n),
    BigUint::from_bytes_be(&jwk.e),
    BigUint::from_bytes_be(&jwk.d),
    vec![
      BigUint::from_bytes_be(&jwk.p),
      BigUint::from_bytes_be(&jwk.q),
    ],
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
  let key =
    RsaPublicKey::from_public_key_der(spki).map_err(|_| RsaError::Data)?;
  Ok(RsaJwk {
    n: encode_uint(key.n()),
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
  let qi = qinv_uint(&key)?;
  Ok(RsaJwk {
    n: encode_uint(key.n()),
    e: encode_uint(key.e()),
    d: Some(encode_uint(key.d())),
    p: Some(encode_uint(&primes[0])),
    q: Some(encode_uint(&primes[1])),
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
    "SHA-1" => Ok(pkcs1_sign::<sha1_10::Sha1>(key, data)),
    "SHA-256" => Ok(pkcs1_sign::<rsa::sha2::Sha256>(key, data)),
    "SHA-384" => Ok(pkcs1_sign::<rsa::sha2::Sha384>(key, data)),
    "SHA-512" => Ok(pkcs1_sign::<rsa::sha2::Sha512>(key, data)),
    _ => Err(RsaError::Unsupported),
  }
}

pub fn verify_pkcs1(
  spki: &[u8],
  hash: &str,
  data: &[u8],
  signature: &[u8],
) -> Result<bool, RsaError> {
  let key =
    RsaPublicKey::from_public_key_der(spki).map_err(|_| RsaError::Data)?;
  Ok(match hash {
    "SHA-1" => pkcs1_verify::<sha1_10::Sha1>(key, data, signature),
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
  let limit = key.size();
  check_salt(limit, hash, salt_len)?;
  match hash {
    "SHA-1" => Ok(pss_sign::<sha1_10::Sha1>(key, salt_len, data)),
    "SHA-256" => Ok(pss_sign::<rsa::sha2::Sha256>(key, salt_len, data)),
    "SHA-384" => Ok(pss_sign::<rsa::sha2::Sha384>(key, salt_len, data)),
    "SHA-512" => Ok(pss_sign::<rsa::sha2::Sha512>(key, salt_len, data)),
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
  let key =
    RsaPublicKey::from_public_key_der(spki).map_err(|_| RsaError::Data)?;
  check_salt(key.size(), hash, salt_len)?;
  Ok(match hash {
    "SHA-1" => pss_verify::<sha1_10::Sha1>(key, salt_len, data, signature),
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
  let key =
    RsaPublicKey::from_public_key_der(spki).map_err(|_| RsaError::Data)?;
  match hash {
    "SHA-1" => oaep_encrypt::<sha1_10::Sha1>(&key, label, data),
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
    "SHA-1" => oaep_decrypt::<sha1_10::Sha1>(&key, label, data),
    "SHA-256" => oaep_decrypt::<rsa::sha2::Sha256>(&key, label, data),
    "SHA-384" => oaep_decrypt::<rsa::sha2::Sha384>(&key, label, data),
    "SHA-512" => oaep_decrypt::<rsa::sha2::Sha512>(&key, label, data),
    _ => Err(RsaError::Unsupported),
  }
}

fn private_key(pkcs8: &[u8]) -> Result<RsaPrivateKey, RsaError> {
  RsaPrivateKey::from_pkcs8_der(pkcs8).map_err(|_| RsaError::Data)
}

fn parts_of(key: &RsaPrivateKey) -> RsaParts {
  parts_public(key)
}

fn parts_public(key: &impl PublicKeyParts) -> RsaParts {
  let mut exponent = key.e().to_bytes_be();
  if exponent.is_empty() {
    exponent.push(0);
  }
  RsaParts {
    modulus_length: key.n().bits() as u32,
    public_exponent: exponent,
  }
}

fn encode_uint(value: &BigUint) -> String {
  let bytes = value.to_bytes_be();
  let bytes = if bytes.is_empty() {
    &[0]
  } else {
    bytes.as_slice()
  };
  crate::text::base64::encode_url(bytes)
}

fn same_uint(value: Option<&BigUint>, bytes: &[u8]) -> bool {
  value.is_some_and(|value| value == &BigUint::from_bytes_be(bytes))
}

fn qinv_matches(key: &RsaPrivateKey, bytes: &[u8]) -> bool {
  qinv_uint(key).is_ok_and(|value| value == BigUint::from_bytes_be(bytes))
}

fn qinv_uint(key: &RsaPrivateKey) -> Result<BigUint, RsaError> {
  let qi = key.qinv().ok_or(RsaError::Data)?;
  let (_, magnitude) = qi.to_bytes_be();
  Ok(BigUint::from_bytes_be(&magnitude))
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

fn pkcs1_sign<D>(key: RsaPrivateKey, data: &[u8]) -> Vec<u8>
where
  D: rsa::sha2::digest::Digest + rsa::pkcs8::AssociatedOid,
{
  let signing = rsa::pkcs1v15::SigningKey::<D>::new(key);
  RandomizedSigner::sign_with_rng(&signing, &mut SysRng, data).to_vec()
}

fn pkcs1_verify<D>(key: RsaPublicKey, data: &[u8], signature: &[u8]) -> bool
where
  D: rsa::sha2::digest::Digest + rsa::pkcs8::AssociatedOid,
{
  let Ok(signature) = rsa::pkcs1v15::Signature::try_from(signature) else {
    return false;
  };
  let verifying = rsa::pkcs1v15::VerifyingKey::<D>::new(key);
  Verifier::verify(&verifying, data, &signature).is_ok()
}

fn pss_sign<D>(key: RsaPrivateKey, salt_len: usize, data: &[u8]) -> Vec<u8>
where
  D: rsa::sha2::digest::Digest + rsa::sha2::digest::FixedOutputReset,
{
  let signing = rsa::pss::SigningKey::<D>::new_with_salt_len(key, salt_len);
  RandomizedSigner::sign_with_rng(&signing, &mut SysRng, data).to_vec()
}

fn pss_verify<D>(
  key: RsaPublicKey,
  salt_len: usize,
  data: &[u8],
  signature: &[u8],
) -> bool
where
  D: rsa::sha2::digest::Digest + rsa::sha2::digest::FixedOutputReset,
{
  let Ok(signature) = rsa::pss::Signature::try_from(signature) else {
    return false;
  };
  let verifying = rsa::pss::VerifyingKey::<D>::new_with_salt_len(key, salt_len);
  Verifier::verify(&verifying, data, &signature).is_ok()
}

fn oaep_encrypt<D>(
  key: &RsaPublicKey,
  label: &[u8],
  data: &[u8],
) -> Result<Vec<u8>, RsaError>
where
  D: 'static
    + rsa::sha2::digest::Digest
    + rsa::sha2::digest::DynDigest
    + Send
    + Sync,
{
  let padding = oaep_padding::<D>(label)?;
  key
    .encrypt(&mut SysRng, padding, data)
    .map_err(|_| RsaError::Operation)
}

fn oaep_decrypt<D>(
  key: &RsaPrivateKey,
  label: &[u8],
  data: &[u8],
) -> Result<Vec<u8>, RsaError>
where
  D: 'static
    + rsa::sha2::digest::Digest
    + rsa::sha2::digest::DynDigest
    + Send
    + Sync,
{
  let padding = oaep_padding::<D>(label)?;
  key.decrypt(padding, data).map_err(|_| RsaError::Operation)
}

fn oaep_padding<D>(label: &[u8]) -> Result<Oaep, RsaError>
where
  D: 'static
    + rsa::sha2::digest::Digest
    + rsa::sha2::digest::DynDigest
    + Send
    + Sync,
{
  if label.is_empty() {
    return Ok(Oaep::new::<D>());
  }
  let label = std::str::from_utf8(label).map_err(|_| RsaError::Operation)?;
  Ok(Oaep::new_with_label::<D, _>(label))
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
