//! WebCrypto dispatch for SHA-3, SHAKE, KMAC, AES-OCB, ChaCha20-Poly1305,
//! Argon2, ML-KEM, ML-DSA, SLH-DSA-SHA2-128f, the hybrid KEMs, and
//! `SubtleCrypto.supports`.

use v8::Local;
use yuzuko_core::op::OpError;

use super::CryptoKey;
use super::Generated;
use super::KeyView;
use super::algorithm_bytes;
use super::algorithm_hash;
use super::algorithm_name;
use super::algorithm_number;
use super::buffer;
use super::check_ext;
use super::check_key_ops;
use super::check_usages;
use super::check_use;
use super::ec_crypto_key;
use super::ec_error;
use super::enforce_octet;
use super::enforce_unsigned_long;
use super::filter_usages;
use super::hmac_block_bits;
use super::is_ec;
use super::is_okp;
use super::known_digest;
use super::load_key;
use super::modern;
use super::normalize_usages;
use super::okp_crypto_key;
use super::okp_error;
use super::optional_algorithm_bytes;
use super::random_bytes;
use super::read_jwk;
use super::read_usages;
use super::reject_empty_usages;
use super::reject_unknown_usages;
use super::rsa_crypto_key;
use super::rsa_error;
use super::secret_key;
use super::string_value;

fn op(error: modern::Fail) -> OpError {
  error.into_op()
}

pub(crate) fn is_sha3(name: &str) -> bool {
  matches!(name, "SHA3-256" | "SHA3-384" | "SHA3-512")
}

pub(crate) fn is_shake(name: &str) -> bool {
  matches!(name, "SHAKE128" | "SHAKE256")
}

pub(crate) fn is_kmac(name: &str) -> bool {
  matches!(name, "KMAC128" | "KMAC256")
}

pub(crate) fn is_argon(name: &str) -> bool {
  matches!(name, "Argon2d" | "Argon2i" | "Argon2id")
}

pub(crate) fn is_ocb(name: &str) -> bool {
  name == "AES-OCB"
}

pub(crate) fn is_chacha(name: &str) -> bool {
  name == "ChaCha20-Poly1305"
}

pub(crate) fn is_kem(name: &str) -> bool {
  modern::is_mlkem(name) || modern::is_hybrid(name)
}

pub(crate) fn is_asym(name: &str) -> bool {
  modern::is_mlkem(name)
    || modern::is_mldsa(name)
    || modern::is_hybrid(name)
    || modern::is_slh(name)
}

pub(crate) fn canonical(lower: &str) -> Option<&'static str> {
  Some(match lower {
    "sha3-256" => "SHA3-256",
    "sha3-384" => "SHA3-384",
    "sha3-512" => "SHA3-512",
    "shake128" => "SHAKE128",
    "shake256" => "SHAKE256",
    "kmac128" => "KMAC128",
    "kmac256" => "KMAC256",
    "aes-ocb" => "AES-OCB",
    "chacha20-poly1305" => "ChaCha20-Poly1305",
    "argon2d" => "Argon2d",
    "argon2i" => "Argon2i",
    "argon2id" => "Argon2id",
    "ml-kem-512" => "ML-KEM-512",
    "ml-kem-768" => "ML-KEM-768",
    "ml-kem-1024" => "ML-KEM-1024",
    "ml-dsa-44" => "ML-DSA-44",
    "ml-dsa-65" => "ML-DSA-65",
    "ml-dsa-87" => "ML-DSA-87",
    "mlkem768-p256" => "MLKEM768-P256",
    "mlkem768-x25519" => "MLKEM768-X25519",
    "mlkem1024-p384" => "MLKEM1024-P384",
    "slh-dsa-sha2-128f" => "SLH-DSA-SHA2-128f",
    _ => return None,
  })
}

fn dict<'a>(value: Local<'a, v8::Value>) -> Option<Local<'a, v8::Object>> {
  if value.is_object() && !value.is_string() {
    Local::<v8::Object>::try_from(value).ok()
  } else {
    None
  }
}

fn asym_key(
  kind: &str,
  algorithm: &str,
  extractable: bool,
  usages: Vec<String>,
  bytes: Vec<u8>,
) -> CryptoKey {
  let mut key = secret_key(algorithm, extractable, None, 0, usages, bytes);
  key.kind = kind.to_string();
  key
}

fn public_material(name: &str, private: &[u8]) -> Result<Vec<u8>, OpError> {
  let public = if modern::is_mlkem(name) {
    modern::mlkem_public_from_seed(name, private)
  } else if modern::is_mldsa(name) {
    modern::mldsa_public_from_seed(name, private)
  } else if modern::is_hybrid(name) {
    modern::hybrid_public_from_seed(name, private)
  } else if modern::is_slh(name) {
    modern::slh_public_from_private(private)
  } else {
    Err(modern::Fail::Unsupported(
      "The key algorithm is not supported",
    ))
  };
  public.map_err(op)
}

fn validate_public(name: &str, public: &[u8]) -> Result<(), OpError> {
  let result = if modern::is_mlkem(name) {
    modern::mlkem_validate_public(name, public)
  } else if modern::is_mldsa(name) {
    modern::mldsa_validate_public(name, public)
  } else if modern::is_hybrid(name) {
    modern::hybrid_validate_public(name, public)
  } else if modern::is_slh(name) {
    modern::slh_validate_public(public)
  } else {
    Err(modern::Fail::Unsupported(
      "The key algorithm is not supported",
    ))
  };
  result.map_err(op)
}

pub(crate) fn digest(
  scope: &mut v8::PinScope,
  algorithm: Local<v8::Value>,
  name: &str,
  data: Local<v8::Value>,
) -> Result<Vec<u8>, OpError> {
  if is_shake(name) {
    let length = shake_length(scope, algorithm)?;
    let bytes = buffer(scope, data)?;
    return modern::shake(name, &bytes, length).map_err(op);
  }
  let bytes = buffer(scope, data)?;
  modern::sha3(name, &bytes).map_err(op)
}

fn shake_length(
  scope: &mut v8::PinScope,
  algorithm: Local<v8::Value>,
) -> Result<usize, OpError> {
  let Some(number) = algorithm_number(scope, algorithm, "outputLength") else {
    return Err(OpError::type_("outputLength is required"));
  };
  output_bytes(number)
}

fn output_bytes(number: f64) -> Result<usize, OpError> {
  let bits = enforce_unsigned_long(number, "outputLength")?;
  if bits == 0 || !bits.is_multiple_of(8) {
    return Err(OpError::dom(
      "OperationError",
      "The output length is invalid",
    ));
  }
  Ok((bits / 8) as usize)
}

pub(super) fn generate(
  scope: &mut v8::PinScope,
  name: &str,
  algorithm: Local<v8::Value>,
  extractable: bool,
  usages: &[String],
) -> Result<Generated, OpError> {
  if is_chacha(name) {
    return generate_chacha(scope, algorithm, extractable, usages);
  }
  if is_kmac(name) {
    return generate_kmac(scope, name, algorithm, extractable, usages);
  }
  if is_argon(name) {
    reject_unknown_usages(name, usages)?;
    return Err(OpError::dom(
      "NotSupportedError",
      "generateKey is not used for this algorithm",
    ));
  }
  if is_asym(name) {
    return generate_pair(name, extractable, usages);
  }
  Err(OpError::dom(
    "NotSupportedError",
    "The key algorithm is not supported",
  ))
}

fn generate_chacha(
  scope: &mut v8::PinScope,
  algorithm: Local<v8::Value>,
  extractable: bool,
  usages: &[String],
) -> Result<Generated, OpError> {
  reject_unknown_usages("ChaCha20-Poly1305", usages)?;
  if let Some(number) = algorithm_number(scope, algorithm, "length") {
    let _ = enforce_unsigned_long(number, "length")?;
  }
  reject_empty_usages("ChaCha20-Poly1305", usages)?;
  let bytes = random_bytes(256)?;
  Ok(Generated::Secret(Box::new(secret_key(
    "ChaCha20-Poly1305",
    extractable,
    None,
    0,
    usages.to_vec(),
    bytes,
  ))))
}

fn generate_kmac(
  scope: &mut v8::PinScope,
  name: &str,
  algorithm: Local<v8::Value>,
  extractable: bool,
  usages: &[String],
) -> Result<Generated, OpError> {
  reject_unknown_usages(name, usages)?;
  let length = kmac_length(scope, name, algorithm)?;
  reject_empty_usages(name, usages)?;
  let bytes = random_bytes(length)?;
  Ok(Generated::Secret(Box::new(secret_key(
    name,
    extractable,
    None,
    length,
    usages.to_vec(),
    bytes,
  ))))
}

fn kmac_length(
  scope: &mut v8::PinScope,
  name: &str,
  algorithm: Local<v8::Value>,
) -> Result<u32, OpError> {
  let Some(number) = algorithm_number(scope, algorithm, "length") else {
    return Ok(if name == "KMAC128" { 128 } else { 256 });
  };
  let length = enforce_unsigned_long(number, "length")?;
  if matches!(length, 128 | 160 | 256) {
    Ok(length)
  } else {
    Err(OpError::dom(
      "OperationError",
      "The KMAC key length is invalid",
    ))
  }
}

fn generate_pair(
  name: &str,
  extractable: bool,
  usages: &[String],
) -> Result<Generated, OpError> {
  reject_unknown_usages(name, usages)?;
  reject_empty_usages(name, usages)?;
  let (public_allowed, private_allowed) = usage_sets(name);
  let private_usages = filter_usages(usages, private_allowed);
  if private_usages.is_empty() {
    return Err(OpError::dom("SyntaxError", "The key usage is invalid"));
  }
  let public_usages = filter_usages(usages, public_allowed);
  let (private_bytes, public_bytes) = if modern::is_slh(name) {
    modern::slh_generate().map_err(op)?
  } else {
    let seed_len = modern::seed_len(name).ok_or_else(|| {
      OpError::dom("NotSupportedError", "The key algorithm is not supported")
    })?;
    let seed = random_bytes((seed_len as u32) * 8)?;
    let public = public_material(name, &seed)?;
    (seed, public)
  };
  let public = asym_key("public", name, true, public_usages, public_bytes);
  let private =
    asym_key("private", name, extractable, private_usages, private_bytes);
  Ok(Generated::Pair {
    public: Box::new(public),
    private: Box::new(private),
  })
}

fn usage_sets(
  name: &str,
) -> (&'static [&'static str], &'static [&'static str]) {
  if is_kem(name) {
    (
      &["encapsulateBits", "encapsulateKey"],
      &["decapsulateBits", "decapsulateKey"],
    )
  } else {
    (&["verify"], &["sign"])
  }
}

pub(crate) fn import_format(algorithm: &str, format: &str) -> String {
  let format = format.to_ascii_lowercase();
  if matches!(
    algorithm,
    "HMAC" | "AES-GCM" | "AES-CBC" | "AES-CTR" | "AES-KW" | "HKDF" | "PBKDF2"
  ) && format == "raw-secret"
  {
    return "raw".to_string();
  }
  if (is_ec(algorithm) || is_okp(algorithm)) && format == "raw-public" {
    return "raw".to_string();
  }
  format
}

pub(crate) fn export_format(algorithm: &str, format: &str) -> String {
  import_format(algorithm, format)
}

pub(crate) fn export_supported(algorithm: &str, format: &str) -> bool {
  let format = format.to_ascii_lowercase();
  let format = format.as_str();
  if matches!(
    algorithm,
    "HMAC" | "AES-GCM" | "AES-CBC" | "AES-CTR" | "AES-KW"
  ) {
    return matches!(format, "raw" | "raw-secret" | "jwk");
  }
  if matches!(algorithm, "HKDF" | "PBKDF2") {
    return false;
  }
  if matches!(algorithm, "RSASSA-PKCS1-v1_5" | "RSA-PSS" | "RSA-OAEP") {
    return matches!(format, "pkcs8" | "spki" | "jwk");
  }
  if matches!(
    algorithm,
    "ECDH" | "ECDSA" | "Ed25519" | "Ed448" | "X25519" | "X448"
  ) {
    return matches!(format, "raw" | "raw-public" | "pkcs8" | "spki" | "jwk");
  }
  if is_ocb(algorithm) || is_chacha(algorithm) || is_kmac(algorithm) {
    return matches!(format, "raw-secret" | "jwk");
  }
  if is_argon(algorithm) {
    return format == "raw-secret";
  }
  if modern::is_mlkem(algorithm) || modern::is_mldsa(algorithm) {
    return matches!(
      format,
      "pkcs8" | "spki" | "jwk" | "raw-public" | "raw-seed"
    );
  }
  if modern::is_hybrid(algorithm) {
    return matches!(format, "jwk" | "raw-public" | "raw-seed");
  }
  if modern::is_slh(algorithm) {
    return matches!(format, "jwk" | "raw-public" | "raw-private");
  }
  false
}

pub(crate) fn import_secret(
  name: &str,
  bytes: Vec<u8>,
  extractable: bool,
  usages: Vec<String>,
) -> Result<CryptoKey, OpError> {
  if is_ocb(name) {
    let length = match bytes.len() {
      16 => 128,
      24 => 192,
      32 => 256,
      _ => {
        return Err(OpError::dom("DataError", "The AES key length is invalid"));
      }
    };
    return Ok(secret_key(name, extractable, None, length, usages, bytes));
  }
  if is_chacha(name) {
    if bytes.len() != 32 {
      return Err(OpError::dom(
        "DataError",
        "The ChaCha20-Poly1305 key must be 32 bytes",
      ));
    }
    return Ok(secret_key(name, extractable, None, 0, usages, bytes));
  }
  if is_kmac(name) {
    if bytes.is_empty() || bytes.len() > 65536 {
      return Err(OpError::dom("DataError", "The KMAC key length is invalid"));
    }
    let length = (bytes.len() * 8) as u32;
    return Ok(secret_key(name, extractable, None, length, usages, bytes));
  }
  if is_argon(name) {
    return Ok(secret_key(name, extractable, None, 0, usages, bytes));
  }
  Err(OpError::dom(
    "NotSupportedError",
    "The key algorithm is not supported",
  ))
}

pub(crate) fn import_asym(
  scope: &mut v8::PinScope,
  name: &str,
  format: &str,
  key_data: Local<v8::Value>,
  extractable: bool,
  usages: Vec<String>,
) -> Result<CryptoKey, OpError> {
  let format = format.to_ascii_lowercase();
  if !export_supported(name, &format) && format != "jwk" {
    return Err(OpError::dom(
      "NotSupportedError",
      "The key format is not supported",
    ));
  }
  if !importable_format(name, &format) {
    return Err(OpError::dom(
      "NotSupportedError",
      "The key format is not supported",
    ));
  }
  if format == "jwk" {
    return import_akp(scope, name, key_data, extractable, usages);
  }
  let bytes = buffer(scope, key_data)?;
  let private = matches!(format.as_str(), "pkcs8" | "raw-seed" | "raw-private");
  check_asym_usages(name, &usages, private)?;
  let material = match format.as_str() {
    "pkcs8" => {
      let (decoded, seed) = modern::pkcs8_decode(&bytes).map_err(op)?;
      if decoded != name {
        return Err(OpError::dom(
          "DataError",
          "The key algorithm does not match",
        ));
      }
      seed
    }
    "spki" => {
      let (decoded, public) = modern::spki_decode(&bytes).map_err(op)?;
      if decoded != name {
        return Err(OpError::dom(
          "DataError",
          "The key algorithm does not match",
        ));
      }
      validate_public(name, &public)?;
      public
    }
    "raw-seed" => {
      let expected = modern::seed_len(name).ok_or_else(|| {
        OpError::dom("NotSupportedError", "The key format is not supported")
      })?;
      if bytes.len() != expected {
        return Err(OpError::dom("DataError", "The seed length is invalid"));
      }
      let _ = public_material(name, &bytes)?;
      bytes
    }
    "raw-public" => {
      validate_public(name, &bytes)?;
      bytes
    }
    "raw-private" => {
      let _ = public_material(name, &bytes)?;
      bytes
    }
    _ => {
      return Err(OpError::dom(
        "NotSupportedError",
        "The key format is not supported",
      ));
    }
  };
  let kind = if private { "private" } else { "public" };
  Ok(asym_key(kind, name, extractable, usages, material))
}

fn importable_format(name: &str, format: &str) -> bool {
  export_supported(name, format)
}

fn check_asym_usages(
  name: &str,
  usages: &[String],
  private: bool,
) -> Result<(), OpError> {
  reject_unknown_usages(name, usages)?;
  let (public_allowed, private_allowed) = usage_sets(name);
  let allowed = if private {
    private_allowed
  } else {
    public_allowed
  };
  if usages
    .iter()
    .any(|usage| !allowed.contains(&usage.as_str()))
    || (private && usages.is_empty())
  {
    return Err(OpError::dom("SyntaxError", "The key usage is invalid"));
  }
  Ok(())
}

fn import_akp(
  scope: &mut v8::PinScope,
  name: &str,
  key_data: Local<v8::Value>,
  extractable: bool,
  usages: Vec<String>,
) -> Result<CryptoKey, OpError> {
  let jwk = read_jwk(scope, key_data)?;
  if jwk.kty.as_deref() != Some("AKP") {
    return Err(OpError::dom("DataError", "The JWK kty is invalid"));
  }
  if let Some(alg) = jwk.alg.as_deref()
    && alg != name
  {
    return Err(OpError::dom("DataError", "The JWK alg is invalid"));
  }
  let private = jwk.priv_b64.is_some();
  check_asym_usages(name, &usages, private)?;
  let use_ = if is_kem(name) { "enc" } else { "sig" };
  check_use(&usages, jwk.use_.as_deref(), use_)?;
  check_key_ops(jwk.key_ops.as_deref(), &usages)?;
  check_ext(jwk.ext, extractable)?;
  let (kind, bytes) = if private {
    let seed = decode_member(jwk.priv_b64.as_deref(), "priv")?;
    let public = public_material(name, &seed)?;
    if let Some(published) = jwk.pub_b64.as_deref() {
      let published =
        crate::text::base64::decode_url(published).map_err(|_| {
          OpError::dom("DataError", "The JWK pub member is not base64url")
        })?;
      if published != public {
        return Err(OpError::dom(
          "DataError",
          "The JWK pub member does not match",
        ));
      }
    }
    ("private", seed)
  } else {
    let public = decode_member(jwk.pub_b64.as_deref(), "pub")?;
    validate_public(name, &public)?;
    ("public", public)
  };
  Ok(asym_key(kind, name, extractable, usages, bytes))
}

fn decode_member(value: Option<&str>, name: &str) -> Result<Vec<u8>, OpError> {
  let Some(value) = value else {
    return Err(OpError::dom(
      "DataError",
      format!("The JWK {name} member is missing"),
    ));
  };
  crate::text::base64::decode_url(value).map_err(|_| {
    OpError::dom(
      "DataError",
      format!("The JWK {name} member is not base64url"),
    )
  })
}

pub(crate) fn accept_jwk_alg(
  name: &str,
  bytes: &[u8],
  alg: Option<&str>,
) -> Result<(), OpError> {
  if is_chacha(name) {
    if alg.is_some() {
      return Err(OpError::dom("DataError", "The JWK alg is invalid"));
    }
    return Ok(());
  }
  if is_kmac(name) {
    let expected = if name == "KMAC128" { "K128" } else { "K256" };
    if alg.is_some_and(|alg| alg != expected) {
      return Err(OpError::dom("DataError", "The JWK alg is invalid"));
    }
    return Ok(());
  }
  if is_ocb(name) {
    return super::aes_jwk_alg(name, bytes, alg);
  }
  Ok(())
}

pub(super) fn pkcs8_of(key: &KeyView) -> Result<Vec<u8>, OpError> {
  modern::pkcs8_encode(&key.algorithm, &key.bytes).map_err(op)
}

pub(super) fn spki_of(key: &KeyView) -> Result<Vec<u8>, OpError> {
  modern::spki_encode(&key.algorithm, &key.bytes).map_err(op)
}

pub(crate) fn der_private(name: &str) -> bool {
  modern::is_mlkem(name) || modern::is_mldsa(name)
}

pub(crate) struct AkpJwk {
  pub alg: String,
  pub public: String,
  pub private: Option<String>,
  pub key_ops: Vec<String>,
  pub ext: bool,
}

pub(super) fn akp_jwk(key: &KeyView) -> Result<AkpJwk, OpError> {
  let public = if key.kind == "public" {
    key.bytes.clone()
  } else {
    public_material(&key.algorithm, &key.bytes)?
  };
  let private = if key.kind == "private" {
    Some(crate::text::base64::encode_url(&key.bytes))
  } else {
    None
  };
  Ok(AkpJwk {
    alg: key.algorithm.clone(),
    public: crate::text::base64::encode_url(&public),
    private,
    key_ops: key.usages.clone(),
    ext: key.extractable,
  })
}

pub(crate) fn akp_json(jwk: &AkpJwk) -> String {
  let mut fields = vec![
    "\"kty\":\"AKP\"".to_string(),
    format!("\"alg\":\"{}\"", jwk.alg),
    format!("\"pub\":\"{}\"", jwk.public),
  ];
  if let Some(private) = &jwk.private {
    fields.push(format!("\"priv\":\"{private}\""));
  }
  let operations = jwk
    .key_ops
    .iter()
    .map(|usage| format!("\"{usage}\""))
    .collect::<Vec<_>>()
    .join(",");
  fields.push(format!("\"key_ops\":[{operations}]"));
  fields.push(format!(
    "\"ext\":{}",
    if jwk.ext { "true" } else { "false" }
  ));
  format!("{{{}}}", fields.join(","))
}

pub(crate) fn akp_value<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  jwk: &AkpJwk,
) -> Local<'s, v8::Value> {
  let object = v8::Object::new(scope);
  super::set_string(scope, object, "kty", "AKP");
  super::set_string(scope, object, "alg", &jwk.alg);
  super::set_string(scope, object, "pub", &jwk.public);
  if let Some(private) = &jwk.private {
    super::set_string(scope, object, "priv", private);
  }
  let operations = v8::Array::new(scope, jwk.key_ops.len() as i32);
  for (index, usage) in jwk.key_ops.iter().enumerate() {
    let text = v8::String::new(scope, usage).unwrap();
    operations.set_index(scope, index as u32, text.into());
  }
  let key = v8::String::new(scope, "key_ops").unwrap();
  object.set(scope, key.into(), operations.into());
  let ext = v8::Boolean::new(scope, jwk.ext);
  let key = v8::String::new(scope, "ext").unwrap();
  object.set(scope, key.into(), ext.into());
  object.into()
}

pub(crate) fn sign(
  scope: &mut v8::PinScope,
  name: &str,
  algorithm: Local<v8::Value>,
  key: Local<v8::Value>,
  data: Local<v8::Value>,
) -> Result<Vec<u8>, OpError> {
  if is_kmac(name) {
    let length = shake_length(scope, algorithm)?;
    let custom = optional_algorithm_bytes(scope, algorithm, "customization")?;
    let key = mac_key(scope, name, key, "sign")?;
    let data = buffer(scope, data)?;
    return modern::kmac(name, &key.bytes, &data, length, &custom).map_err(op);
  }
  let context = optional_algorithm_bytes(scope, algorithm, "context")?;
  let key = load_key(scope, key)?;
  if key.algorithm != name
    || key.kind != "private"
    || !key.usages.iter().any(|usage| usage == "sign")
  {
    return Err(OpError::dom(
      "InvalidAccessError",
      "The key usage is invalid",
    ));
  }
  let data = buffer(scope, data)?;
  if modern::is_mldsa(name) {
    modern::mldsa_sign(name, &key.bytes, &data, &context).map_err(op)
  } else if modern::is_slh(name) {
    modern::slh_sign(&key.bytes, &data, &context).map_err(op)
  } else {
    Err(OpError::dom(
      "NotSupportedError",
      "The signing algorithm is not supported",
    ))
  }
}

pub(crate) fn verify(
  scope: &mut v8::PinScope,
  name: &str,
  algorithm: Local<v8::Value>,
  key: Local<v8::Value>,
  signature: Local<v8::Value>,
  data: Local<v8::Value>,
) -> Result<bool, OpError> {
  if is_kmac(name) {
    let length = shake_length(scope, algorithm)?;
    let custom = optional_algorithm_bytes(scope, algorithm, "customization")?;
    let key = mac_key(scope, name, key, "verify")?;
    let signature = buffer(scope, signature)?;
    let data = buffer(scope, data)?;
    let expected =
      modern::kmac(name, &key.bytes, &data, length, &custom).map_err(op)?;
    return Ok(constant_time_eq(&expected, &signature));
  }
  let context = optional_algorithm_bytes(scope, algorithm, "context")?;
  let key = load_key(scope, key)?;
  if key.algorithm != name
    || key.kind != "public"
    || !key.usages.iter().any(|usage| usage == "verify")
  {
    return Err(OpError::dom(
      "InvalidAccessError",
      "The key usage is invalid",
    ));
  }
  let signature = buffer(scope, signature)?;
  let data = buffer(scope, data)?;
  if modern::is_mldsa(name) {
    modern::mldsa_verify(name, &key.bytes, &data, &signature, &context)
      .map_err(op)
  } else if modern::is_slh(name) {
    modern::slh_verify(&key.bytes, &data, &signature, &context).map_err(op)
  } else {
    Err(OpError::dom(
      "NotSupportedError",
      "The signing algorithm is not supported",
    ))
  }
}

fn mac_key(
  scope: &mut v8::PinScope,
  name: &str,
  key: Local<v8::Value>,
  usage: &str,
) -> Result<KeyView, OpError> {
  let key = load_key(scope, key)?;
  if key.algorithm != name || !key.usages.iter().any(|item| item == usage) {
    return Err(OpError::dom(
      "InvalidAccessError",
      "The key usage is invalid",
    ));
  }
  Ok(key)
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
  left.len() == right.len()
    && left
      .iter()
      .zip(right)
      .fold(0u8, |acc, (left, right)| acc | (left ^ right))
      == 0
}

pub(crate) fn encrypt(
  scope: &mut v8::PinScope,
  name: &str,
  algorithm: Local<v8::Value>,
  key: Local<v8::Value>,
  data: Local<v8::Value>,
  encrypting: bool,
) -> Result<Vec<u8>, OpError> {
  let prepared = prepare_aead(scope, name, algorithm)?;
  let key = load_key(scope, key)?;
  let usage = if encrypting { "encrypt" } else { "decrypt" };
  if key.algorithm != name || !key.usages.iter().any(|item| item == usage) {
    return Err(OpError::dom(
      "InvalidAccessError",
      "The key usage is invalid",
    ));
  }
  let data = buffer(scope, data)?;
  aead_with(name, &key.bytes, &prepared, &data, encrypting)
}

pub(crate) fn aead_crypt(
  scope: &mut v8::PinScope,
  name: &str,
  algorithm: Local<v8::Value>,
  key: &[u8],
  data: &[u8],
  encrypting: bool,
) -> Result<Vec<u8>, OpError> {
  let prepared = prepare_aead(scope, name, algorithm)?;
  aead_with(name, key, &prepared, data, encrypting)
}

struct AeadParams {
  iv: Vec<u8>,
  aad: Vec<u8>,
  tag: usize,
}

fn prepare_aead(
  scope: &mut v8::PinScope,
  name: &str,
  algorithm: Local<v8::Value>,
) -> Result<AeadParams, OpError> {
  let iv_name = "iv";
  let iv = algorithm_bytes(scope, algorithm, iv_name)?;
  let aad = optional_algorithm_bytes(scope, algorithm, "additionalData")?;
  let tag = if is_chacha(name) {
    if let Some(tag) = algorithm_number(scope, algorithm, "tagLength") {
      let tag = enforce_octet(tag, "tagLength")?;
      if tag != 128 {
        return Err(OpError::dom(
          "OperationError",
          "The ChaCha20-Poly1305 tag length is invalid",
        ));
      }
    }
    16
  } else {
    let bits = match algorithm_number(scope, algorithm, "tagLength") {
      None => 128,
      Some(tag) => {
        let tag = enforce_octet(tag, "tagLength")?;
        if !matches!(tag, 64 | 96 | 128) {
          return Err(OpError::dom(
            "OperationError",
            "The AES-OCB tag length is invalid",
          ));
        }
        tag
      }
    };
    (bits / 8) as usize
  };
  Ok(AeadParams { iv, aad, tag })
}

fn aead_with(
  name: &str,
  key: &[u8],
  params: &AeadParams,
  data: &[u8],
  encrypting: bool,
) -> Result<Vec<u8>, OpError> {
  // `encrypting` is true for encrypt. The AEAD helpers take a decrypt flag.
  let decrypt = !encrypting;
  if is_chacha(name) {
    modern::chacha_crypt(key, &params.iv, &params.aad, data, decrypt)
      .map_err(op)
  } else {
    modern::ocb_crypt(key, &params.iv, &params.aad, data, params.tag, decrypt)
      .map_err(op)
  }
}

pub(crate) fn argon_bits(
  scope: &mut v8::PinScope,
  name: &str,
  algorithm: Local<v8::Value>,
  password: &[u8],
  bits: u32,
) -> Result<Vec<u8>, OpError> {
  let memory = required_number(scope, algorithm, "memory")?;
  let passes = required_number(scope, algorithm, "passes")?;
  let parallelism = required_number(scope, algorithm, "parallelism")?;
  let salt = algorithm_bytes(scope, algorithm, "nonce")?;
  let secret = optional_algorithm_bytes(scope, algorithm, "secretValue")?;
  let associated =
    optional_algorithm_bytes(scope, algorithm, "associatedData")?;
  if let Some(version) = algorithm_number(scope, algorithm, "version") {
    let version = enforce_unsigned_long(version, "version")?;
    if version != 0x13 {
      return Err(OpError::dom(
        "OperationError",
        "The Argon2 version is not supported",
      ));
    }
  }
  modern::argon2(&modern::ArgonInput {
    algorithm: name,
    password,
    salt: &salt,
    secret: &secret,
    associated: &associated,
    memory,
    passes,
    parallelism,
    output_len: (bits / 8) as usize,
  })
  .map_err(op)
}

fn required_number(
  scope: &mut v8::PinScope,
  algorithm: Local<v8::Value>,
  name: &str,
) -> Result<u32, OpError> {
  let Some(number) = algorithm_number(scope, algorithm, name) else {
    return Err(OpError::type_(format!("{name} is required")));
  };
  enforce_unsigned_long(number, name)
}

pub(crate) fn encapsulate(
  scope: &mut v8::PinScope,
  algorithm: Local<v8::Value>,
  key: Local<v8::Value>,
  usage: &str,
) -> Result<(Vec<u8>, Vec<u8>), OpError> {
  let name = algorithm_name(scope, algorithm)?;
  if !is_kem(&name) {
    return Err(OpError::dom(
      "NotSupportedError",
      "The KEM algorithm is not supported",
    ));
  }
  let key = load_key(scope, key)?;
  if key.algorithm != name
    || key.kind != "public"
    || !key.usages.iter().any(|item| item == usage)
  {
    return Err(OpError::dom(
      "InvalidAccessError",
      "The key usage is invalid",
    ));
  }
  let encapsulated = if modern::is_hybrid(&name) {
    modern::hybrid_encapsulate(&name, &key.bytes)
  } else {
    modern::mlkem_encapsulate(&name, &key.bytes)
  };
  encapsulated.map_err(op)
}

pub(crate) fn decapsulate(
  scope: &mut v8::PinScope,
  algorithm: Local<v8::Value>,
  key: Local<v8::Value>,
  ciphertext: Local<v8::Value>,
  usage: &str,
) -> Result<Vec<u8>, OpError> {
  let name = algorithm_name(scope, algorithm)?;
  if !is_kem(&name) {
    return Err(OpError::dom(
      "NotSupportedError",
      "The KEM algorithm is not supported",
    ));
  }
  let key = load_key(scope, key)?;
  if key.algorithm != name
    || key.kind != "private"
    || !key.usages.iter().any(|item| item == usage)
  {
    return Err(OpError::dom(
      "InvalidAccessError",
      "The key usage is invalid",
    ));
  }
  let ciphertext = buffer(scope, ciphertext)?;
  let shared = if modern::is_hybrid(&name) {
    modern::hybrid_decapsulate(&name, &key.bytes, &ciphertext)
  } else {
    modern::mlkem_decapsulate(&name, &key.bytes, &ciphertext)
  };
  shared.map_err(op)
}

pub(crate) fn shared_secret_key(
  scope: &mut v8::PinScope,
  algorithm: Local<v8::Value>,
  secret: &[u8],
  extractable: bool,
  usages: Local<v8::Value>,
) -> Result<CryptoKey, OpError> {
  let name = algorithm_name(scope, algorithm)?;
  let usages = normalize_usages(read_usages(scope, usages)?);
  check_usages(&name, &usages)?;
  let bits = (secret.len() * 8) as u32;
  if name == "HMAC" {
    let hash = algorithm_hash(scope, algorithm)?
      .unwrap_or_else(|| "SHA-256".to_string());
    if !known_digest(&hash) {
      return Err(OpError::dom(
        "NotSupportedError",
        "The hash algorithm is not supported",
      ));
    }
    let length = match algorithm_number(scope, algorithm, "length") {
      None => bits,
      Some(number) => {
        let length = enforce_unsigned_long(number, "length")?;
        if length == 0 || !length.is_multiple_of(8) || length != bits {
          return Err(OpError::dom(
            "OperationError",
            "The length does not match the shared secret",
          ));
        }
        length
      }
    };
    return Ok(secret_key(
      name,
      extractable,
      Some(hash),
      length,
      usages,
      secret.to_vec(),
    ));
  }
  if matches!(
    name.as_str(),
    "AES-GCM" | "AES-CBC" | "AES-CTR" | "AES-KW" | "AES-OCB"
  ) {
    let length = match algorithm_number(scope, algorithm, "length") {
      None => 256,
      Some(number) => {
        let length = enforce_unsigned_long(number, "length")?;
        if !matches!(length, 128 | 192 | 256) {
          return Err(OpError::dom(
            "OperationError",
            "The AES key length is invalid",
          ));
        }
        length
      }
    };
    if length != bits {
      return Err(OpError::dom(
        "OperationError",
        "The length does not match the shared secret",
      ));
    }
    return Ok(secret_key(
      name,
      extractable,
      None,
      length,
      usages,
      secret.to_vec(),
    ));
  }
  if is_chacha(&name) {
    if let Some(number) = algorithm_number(scope, algorithm, "length") {
      let _ = enforce_unsigned_long(number, "length")?;
    }
    if secret.len() != 32 {
      return Err(OpError::dom(
        "OperationError",
        "The length does not match the shared secret",
      ));
    }
    return Ok(secret_key(
      name,
      extractable,
      None,
      0,
      usages,
      secret.to_vec(),
    ));
  }
  if is_kmac(&name) {
    let length = match algorithm_number(scope, algorithm, "length") {
      None => bits,
      Some(number) => {
        let length = enforce_unsigned_long(number, "length")?;
        if length == 0 || !length.is_multiple_of(8) || length != bits {
          return Err(OpError::dom(
            "OperationError",
            "The length does not match the shared secret",
          ));
        }
        length
      }
    };
    return Ok(secret_key(
      name,
      extractable,
      None,
      length,
      usages,
      secret.to_vec(),
    ));
  }
  Err(OpError::dom(
    "NotSupportedError",
    "The shared key algorithm is not supported",
  ))
}

pub(crate) fn get_public_key(
  scope: &mut v8::PinScope,
  key: Local<v8::Value>,
  usages: Local<v8::Value>,
) -> Result<CryptoKey, OpError> {
  let key = load_key(scope, key)?;
  if key.kind == "secret" {
    return Err(OpError::dom(
      "NotSupportedError",
      "getPublicKey is not supported for a secret key",
    ));
  }
  if key.kind != "private" {
    return Err(OpError::dom(
      "InvalidAccessError",
      "The key is not a private key",
    ));
  }
  let usages = normalize_usages(read_usages(scope, usages)?);
  let allowed = public_usage_list(&key.algorithm);
  if allowed.is_empty() && !usages.is_empty()
    || usages
      .iter()
      .any(|usage| !allowed.contains(&usage.as_str()))
  {
    return Err(OpError::dom("SyntaxError", "The key usage is invalid"));
  }
  public_from_private(&key, usages)
}

fn public_usage_list(name: &str) -> &'static [&'static str] {
  if is_kem(name) {
    &["encapsulateBits", "encapsulateKey"]
  } else if matches!(
    name,
    "ECDSA" | "Ed25519" | "Ed448" | "RSASSA-PKCS1-v1_5" | "RSA-PSS"
  ) || modern::is_mldsa(name)
    || modern::is_slh(name)
  {
    &["verify"]
  } else if name == "RSA-OAEP" {
    &["encrypt", "wrapKey"]
  } else {
    &[]
  }
}

fn public_from_private(
  key: &KeyView,
  usages: Vec<String>,
) -> Result<CryptoKey, OpError> {
  let name = key.algorithm.as_str();
  if matches!(name, "ECDH" | "ECDSA") {
    let curve = key.curve.as_deref().unwrap_or("");
    let jwk = crate::crypto::ec_key::private_jwk(curve, &key.bytes)
      .map_err(ec_error)?;
    let x = crate::text::base64::decode_url(&jwk.x)
      .map_err(|_| OpError::dom("DataError", "The key data is invalid"))?;
    let y = crate::text::base64::decode_url(&jwk.y)
      .map_err(|_| OpError::dom("DataError", "The key data is invalid"))?;
    let spki = crate::crypto::ec_key::import_public_jwk(curve, &x, &y)
      .map_err(ec_error)?;
    return Ok(ec_crypto_key("public", name, true, curve, usages, spki));
  }
  if matches!(name, "Ed25519" | "Ed448" | "X25519" | "X448") {
    let jwk = crate::crypto::okp_key::private_jwk(name, &key.bytes)
      .map_err(okp_error)?;
    let x = crate::text::base64::decode_url(&jwk.x)
      .map_err(|_| OpError::dom("DataError", "The key data is invalid"))?;
    let spki =
      crate::crypto::okp_key::import_public_jwk(name, &x).map_err(okp_error)?;
    return Ok(okp_crypto_key("public", name, true, usages, spki));
  }
  if matches!(name, "RSASSA-PKCS1-v1_5" | "RSA-PSS" | "RSA-OAEP") {
    let jwk =
      crate::crypto::rsa_key::private_jwk(&key.bytes).map_err(rsa_error)?;
    let modulus = crate::text::base64::decode_url(&jwk.n)
      .map_err(|_| OpError::dom("DataError", "The key data is invalid"))?;
    let exponent = crate::text::base64::decode_url(&jwk.e)
      .map_err(|_| OpError::dom("DataError", "The key data is invalid"))?;
    let (spki, parts) =
      crate::crypto::rsa_key::import_public_jwk(&modulus, &exponent)
        .map_err(rsa_error)?;
    let hash = key.hash.clone().unwrap_or_default();
    return Ok(rsa_crypto_key(
      "public",
      name,
      true,
      hash,
      parts.modulus_length,
      parts.public_exponent,
      usages,
      spki,
    ));
  }
  if is_asym(name) {
    let public = public_material(name, &key.bytes)?;
    return Ok(asym_key("public", name, true, usages, public));
  }
  Err(OpError::dom(
    "NotSupportedError",
    "getPublicKey is not supported for this algorithm",
  ))
}

pub(crate) fn supports(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
) -> bool {
  v8::tc_scope!(let tc, scope);
  let answered = supports_inner(tc, args);
  !tc.has_caught() && answered
}

fn supports_inner(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
) -> bool {
  let Ok(operation) = string_value(scope, args.get(0)) else {
    return false;
  };
  let algorithm = args.get(1);
  let Ok(name) = algorithm_name(scope, algorithm) else {
    return false;
  };
  if !allows(&operation, &name) {
    return false;
  }
  let third = if args.length() < 3 {
    None
  } else {
    let value = args.get(2);
    if value.is_null() || value.is_undefined() {
      None
    } else {
      Some(value)
    }
  };
  params_ok(scope, &operation, &name, algorithm, third)
}

fn allows(operation: &str, name: &str) -> bool {
  match operation {
    "digest" => is_digest(name),
    "generateKey" => can_generate(name),
    "importKey" => can_import(name),
    "exportKey" => can_export(name),
    "sign" | "verify" => can_sign(name),
    "encrypt" | "decrypt" => can_encrypt(name),
    "deriveBits" | "deriveKey" => can_derive(name),
    "getPublicKey" => can_public(name),
    "encapsulateBits" | "encapsulateKey" | "decapsulateBits"
    | "decapsulateKey" => is_kem(name),
    "wrapKey" | "unwrapKey" => can_wrap(name),
    _ => false,
  }
}

fn is_digest(name: &str) -> bool {
  matches!(name, "SHA-1" | "SHA-256" | "SHA-384" | "SHA-512")
    || is_sha3(name)
    || is_shake(name)
}

fn can_generate(name: &str) -> bool {
  matches!(
    name,
    "HMAC"
      | "AES-GCM"
      | "AES-CBC"
      | "AES-CTR"
      | "AES-KW"
      | "AES-OCB"
      | "ChaCha20-Poly1305"
      | "KMAC128"
      | "KMAC256"
      | "ECDSA"
      | "ECDH"
      | "Ed25519"
      | "Ed448"
      | "X25519"
      | "X448"
      | "RSASSA-PKCS1-v1_5"
      | "RSA-PSS"
      | "RSA-OAEP"
  ) || is_asym(name)
}

fn can_import(name: &str) -> bool {
  can_generate(name) || matches!(name, "HKDF" | "PBKDF2") || is_argon(name)
}

fn can_export(name: &str) -> bool {
  [
    "raw",
    "raw-secret",
    "raw-public",
    "raw-seed",
    "raw-private",
    "pkcs8",
    "spki",
    "jwk",
  ]
  .into_iter()
  .any(|format| export_supported(name, format))
}

fn can_sign(name: &str) -> bool {
  matches!(
    name,
    "HMAC"
      | "KMAC128"
      | "KMAC256"
      | "ECDSA"
      | "Ed25519"
      | "Ed448"
      | "RSASSA-PKCS1-v1_5"
      | "RSA-PSS"
  ) || modern::is_mldsa(name)
    || modern::is_slh(name)
}

fn can_encrypt(name: &str) -> bool {
  matches!(
    name,
    "AES-GCM"
      | "AES-CBC"
      | "AES-CTR"
      | "AES-OCB"
      | "ChaCha20-Poly1305"
      | "RSA-OAEP"
  )
}

fn can_derive(name: &str) -> bool {
  matches!(name, "HKDF" | "PBKDF2" | "ECDH" | "X25519" | "X448")
    || is_argon(name)
}

fn can_public(name: &str) -> bool {
  matches!(
    name,
    "ECDSA"
      | "ECDH"
      | "Ed25519"
      | "Ed448"
      | "X25519"
      | "X448"
      | "RSASSA-PKCS1-v1_5"
      | "RSA-PSS"
      | "RSA-OAEP"
  ) || is_asym(name)
}

fn can_wrap(name: &str) -> bool {
  matches!(
    name,
    "AES-GCM"
      | "AES-CBC"
      | "AES-CTR"
      | "AES-KW"
      | "AES-OCB"
      | "ChaCha20-Poly1305"
      | "RSA-OAEP"
  )
}

fn params_ok(
  scope: &mut v8::PinScope,
  operation: &str,
  name: &str,
  algorithm: Local<v8::Value>,
  third: Option<Local<v8::Value>>,
) -> bool {
  let object = dict(algorithm).is_some();
  if !normalization_ok(scope, operation, name, algorithm, object) {
    return false;
  }
  if !present_fields_ok(scope, operation, name, algorithm) {
    return false;
  }
  match operation {
    "deriveBits" => derive_bits_ok(scope, name, algorithm, third),
    "deriveKey" => derive_key_ok(scope, name, algorithm, third),
    "wrapKey" => wrap_ok(scope, third, true),
    "unwrapKey" => wrap_ok(scope, third, false),
    "encapsulateKey" | "decapsulateKey" => shared_ok(scope, third),
    _ => third.is_none() || true,
  }
}

fn normalization_ok(
  scope: &mut v8::PinScope,
  operation: &str,
  name: &str,
  algorithm: Local<v8::Value>,
  object: bool,
) -> bool {
  let rsa = matches!(name, "RSASSA-PKCS1-v1_5" | "RSA-PSS" | "RSA-OAEP");
  let curve = matches!(name, "ECDSA" | "ECDH");
  if operation == "generateKey" && rsa {
    if !object {
      return false;
    }
    return rsa_generate_ok(scope, algorithm);
  }
  if operation == "importKey" && rsa {
    return object && hash_ok(scope, algorithm, true);
  }
  if matches!(operation, "generateKey" | "importKey") && curve {
    return object && named_curve_ok(scope, algorithm, true);
  }
  true
}

fn rsa_generate_ok(
  scope: &mut v8::PinScope,
  algorithm: Local<v8::Value>,
) -> bool {
  if !hash_ok(scope, algorithm, true) {
    return false;
  }
  let Some(bits) = unsigned(scope, algorithm, "modulusLength") else {
    return false;
  };
  if !crate::crypto::rsa_key::modulus_acceptable(bits) {
    return false;
  }
  let Some(exponent) = bytes_member(scope, algorithm, "publicExponent") else {
    return false;
  };
  crate::crypto::rsa_key::acceptable_exponent(&exponent)
}

fn present_fields_ok(
  scope: &mut v8::PinScope,
  operation: &str,
  name: &str,
  algorithm: Local<v8::Value>,
) -> bool {
  if dict(algorithm).is_none() {
    return true;
  }
  if !hash_ok(scope, algorithm, false) {
    return false;
  }
  if !named_curve_ok(scope, algorithm, false) {
    return false;
  }
  if matches!(
    name,
    "AES-GCM"
      | "AES-CBC"
      | "AES-CTR"
      | "AES-KW"
      | "AES-OCB"
      | "HMAC"
      | "KMAC128"
      | "KMAC256"
  ) && !length_ok(scope, operation, name, algorithm)
  {
    return false;
  }
  if is_chacha(name)
    && let Some(number) = algorithm_number(scope, algorithm, "length")
    && enforce_unsigned_long(number, "length").is_err()
  {
    return false;
  }
  if matches!(operation, "encrypt" | "decrypt" | "wrapKey" | "unwrapKey")
    && !cipher_params_ok(scope, name, algorithm)
  {
    return false;
  }
  if is_shake(name)
    && let Some(number) = algorithm_number(scope, algorithm, "outputLength")
    && output_bytes(number).is_err()
  {
    return false;
  }
  if matches!(operation, "sign" | "verify")
    && !sign_params_ok(scope, name, algorithm)
  {
    return false;
  }
  if matches!(name, "HKDF" | "PBKDF2") && !kdf_fields_ok(scope, name, algorithm)
  {
    return false;
  }
  if is_argon(name) && !argon_fields_ok(scope, algorithm) {
    return false;
  }
  if name == "RSA-PSS"
    && let Some(number) = algorithm_number(scope, algorithm, "saltLength")
    && enforce_unsigned_long(number, "saltLength").is_err()
  {
    return false;
  }
  name != "RSA-OAEP" || optional_bytes_ok(scope, algorithm, "label")
}

fn hash_ok(
  scope: &mut v8::PinScope,
  algorithm: Local<v8::Value>,
  required: bool,
) -> bool {
  match algorithm_hash(scope, algorithm) {
    Ok(None) => !required,
    Ok(Some(hash)) => known_digest(&hash),
    Err(_) => false,
  }
}

fn named_curve_ok(
  scope: &mut v8::PinScope,
  algorithm: Local<v8::Value>,
  required: bool,
) -> bool {
  let Some(object) = dict(algorithm) else {
    return !required;
  };
  let key = v8::String::new(scope, "namedCurve").unwrap();
  let Some(value) = object.get(scope, key.into()) else {
    return false;
  };
  if value.is_undefined() {
    return !required;
  }
  let Ok(text) = string_value(scope, value) else {
    return false;
  };
  let curve = super::canonical_algorithm(&text);
  matches!(curve.as_str(), "P-256" | "P-384" | "P-521")
}

fn length_ok(
  scope: &mut v8::PinScope,
  operation: &str,
  name: &str,
  algorithm: Local<v8::Value>,
) -> bool {
  let Some(number) = algorithm_number(scope, algorithm, "length") else {
    return true;
  };
  let Ok(length) = enforce_unsigned_long(number, "length") else {
    return false;
  };
  // AES-CTR encryption uses `length` as the counter width, not the key size.
  if name == "AES-CTR"
    && matches!(operation, "encrypt" | "decrypt" | "wrapKey" | "unwrapKey")
  {
    return true;
  }
  if matches!(
    name,
    "AES-GCM" | "AES-CBC" | "AES-CTR" | "AES-KW" | "AES-OCB"
  ) {
    return matches!(length, 128 | 192 | 256);
  }
  if is_kmac(name) {
    return matches!(length, 128 | 160 | 256);
  }
  length != 0 && length.is_multiple_of(8)
}

fn cipher_params_ok(
  scope: &mut v8::PinScope,
  name: &str,
  algorithm: Local<v8::Value>,
) -> bool {
  if let Some(iv) = bytes_member(scope, algorithm, iv_field(name)) {
    let valid = match name {
      "AES-GCM" => matches!(iv.len(), 12 | 16 | 32),
      "AES-CBC" => iv.len() == 16,
      "AES-CTR" => iv.len() == 16,
      "AES-OCB" => (6..=15).contains(&iv.len()),
      "ChaCha20-Poly1305" => iv.len() == 12,
      _ => true,
    };
    if !valid {
      return false;
    }
  } else if member_present(scope, algorithm, iv_field(name)) {
    return false;
  }
  if name == "AES-CTR"
    && let Some(number) = algorithm_number(scope, algorithm, "length")
  {
    let Ok(length) = enforce_octet(number, "length") else {
      return false;
    };
    if !(1..=128).contains(&length) {
      return false;
    }
  }
  if let Some(number) = algorithm_number(scope, algorithm, "tagLength") {
    let Ok(tag) = enforce_octet(number, "tagLength") else {
      return false;
    };
    let valid = match name {
      "AES-GCM" => matches!(tag, 32 | 64 | 96 | 104 | 112 | 120 | 128),
      "AES-OCB" => matches!(tag, 64 | 96 | 128),
      "ChaCha20-Poly1305" => tag == 128,
      _ => true,
    };
    if !valid {
      return false;
    }
  }
  optional_bytes_ok(scope, algorithm, "additionalData")
}

fn iv_field(name: &str) -> &'static str {
  if name == "AES-CTR" { "counter" } else { "iv" }
}

fn sign_params_ok(
  scope: &mut v8::PinScope,
  name: &str,
  algorithm: Local<v8::Value>,
) -> bool {
  if is_kmac(name)
    && let Some(number) = algorithm_number(scope, algorithm, "outputLength")
    && output_bytes(number).is_err()
  {
    return false;
  }
  if (modern::is_mldsa(name) || modern::is_slh(name))
    && let Some(context) = bytes_member(scope, algorithm, "context")
    && context.len() > 255
  {
    return false;
  }
  if is_kmac(name) {
    return optional_bytes_ok(scope, algorithm, "customization");
  }
  if modern::is_mldsa(name) || modern::is_slh(name) {
    return !member_present(scope, algorithm, "context")
      || bytes_member(scope, algorithm, "context").is_some();
  }
  true
}

fn kdf_fields_ok(
  scope: &mut v8::PinScope,
  name: &str,
  algorithm: Local<v8::Value>,
) -> bool {
  if name == "PBKDF2"
    && let Some(number) = algorithm_number(scope, algorithm, "iterations")
  {
    match enforce_unsigned_long(number, "iteration count") {
      Ok(0) | Err(_) => return false,
      Ok(_) => {}
    }
  }
  optional_bytes_ok(scope, algorithm, "salt")
    && optional_bytes_ok(scope, algorithm, "info")
}

fn argon_fields_ok(
  scope: &mut v8::PinScope,
  algorithm: Local<v8::Value>,
) -> bool {
  for name in ["memory", "passes", "parallelism"] {
    if let Some(number) = algorithm_number(scope, algorithm, name) {
      match enforce_unsigned_long(number, name) {
        Ok(0) | Err(_) => return false,
        Ok(_) => {}
      }
    }
  }
  if let Some(nonce) = bytes_member(scope, algorithm, "nonce")
    && nonce.len() < 8
  {
    return false;
  }
  if member_present(scope, algorithm, "nonce")
    && bytes_member(scope, algorithm, "nonce").is_none()
  {
    return false;
  }
  if let Some(data) = bytes_member(scope, algorithm, "associatedData")
    && data.len() > 32
  {
    return false;
  }
  if let Some(version) = algorithm_number(scope, algorithm, "version") {
    match enforce_unsigned_long(version, "version") {
      Ok(0x13) => {}
      _ => return false,
    }
  }
  optional_bytes_ok(scope, algorithm, "secretValue")
    && optional_bytes_ok(scope, algorithm, "associatedData")
}

fn derive_bits_ok(
  scope: &mut v8::PinScope,
  name: &str,
  algorithm: Local<v8::Value>,
  third: Option<Local<v8::Value>>,
) -> bool {
  let bits = match third {
    None => None,
    Some(value) if value.is_number() => value.number_value(scope),
    Some(_) => return false,
  };
  if matches!(name, "ECDH" | "X25519" | "X448") {
    let peer = peer_state(scope, name, algorithm);
    if matches!(peer, Peer::Invalid) {
      return false;
    }
    let Some(bits) = bits else {
      return !matches!(peer, Peer::Invalid);
    };
    let Ok(bits) = enforce_unsigned_long(bits, "length") else {
      return false;
    };
    let max = match name {
      "X25519" => Some(256),
      "X448" => Some(448),
      _ => match peer {
        Peer::Key(key) => curve_bits(key.curve.as_deref().unwrap_or("")),
        _ => None,
      },
    };
    return max.is_some_and(|max| bits <= max);
  }
  let Some(bits) = bits else {
    return false;
  };
  let Ok(bits) = enforce_unsigned_long(bits, "length") else {
    return false;
  };
  if !bits.is_multiple_of(8) {
    return false;
  }
  if name == "HKDF" {
    let hash = hash_name(scope, algorithm);
    return hash
      .and_then(|hash| hash_bits(&hash))
      .is_some_and(|width| u64::from(bits) <= 255 * u64::from(width));
  }
  if name == "PBKDF2" {
    return hash_name(scope, algorithm).is_some_and(|hash| known_digest(&hash));
  }
  if is_argon(name) {
    return argon_fields_ok(scope, algorithm);
  }
  false
}

fn derive_key_ok(
  scope: &mut v8::PinScope,
  name: &str,
  algorithm: Local<v8::Value>,
  third: Option<Local<v8::Value>>,
) -> bool {
  let Some(derived) = third else {
    return false;
  };
  if derived.is_number() {
    return false;
  }
  let Ok(derived_name) = algorithm_name(scope, derived) else {
    return false;
  };
  let Some(needed) = derived_length(scope, &derived_name, derived) else {
    return false;
  };
  derive_bits_ok(
    scope,
    name,
    algorithm,
    Some(v8::Number::new(scope, f64::from(needed)).into()),
  )
}

fn derived_length(
  scope: &mut v8::PinScope,
  name: &str,
  algorithm: Local<v8::Value>,
) -> Option<u32> {
  match name {
    "HMAC" => {
      let hash = hash_name(scope, algorithm)?;
      if !known_digest(&hash) {
        return None;
      }
      match algorithm_number(scope, algorithm, "length") {
        None => Some(hmac_block_bits(&hash)),
        Some(number) => {
          let length = enforce_unsigned_long(number, "length").ok()?;
          if length == 0 || !length.is_multiple_of(8) {
            None
          } else {
            Some(length)
          }
        }
      }
    }
    "AES-GCM" | "AES-CBC" | "AES-CTR" | "AES-KW" => {
      match algorithm_number(scope, algorithm, "length") {
        None => Some(256),
        Some(number) => {
          let length = enforce_unsigned_long(number, "length").ok()?;
          if matches!(length, 128 | 192 | 256) {
            Some(length)
          } else {
            None
          }
        }
      }
    }
    _ => None,
  }
}

fn wrap_ok(
  scope: &mut v8::PinScope,
  third: Option<Local<v8::Value>>,
  wrapping: bool,
) -> bool {
  let Some(third) = third else {
    return false;
  };
  let Ok(name) = algorithm_name(scope, third) else {
    return false;
  };
  if wrapping {
    can_export(&name)
  } else {
    can_import(&name)
  }
}

fn shared_ok(
  scope: &mut v8::PinScope,
  third: Option<Local<v8::Value>>,
) -> bool {
  let Some(third) = third else {
    return false;
  };
  let Ok(name) = algorithm_name(scope, third) else {
    return false;
  };
  let secret = [0u8; 32];
  let usages = match name.as_str() {
    "HMAC" => &["sign"][..],
    "AES-KW" => &["wrapKey"][..],
    "AES-GCM" | "AES-CBC" | "AES-CTR" | "AES-OCB" | "ChaCha20-Poly1305"
    | "KMAC128" | "KMAC256" => &["encrypt"][..],
    _ => return false,
  };
  shared_length_matches(scope, &name, third, secret.len()).is_some()
    && usages.iter().any(|usage| {
      super::allowed_usages(&name)
        .is_some_and(|allowed| allowed.contains(usage))
    })
}

fn shared_length_matches(
  scope: &mut v8::PinScope,
  name: &str,
  algorithm: Local<v8::Value>,
  secret_len: usize,
) -> Option<()> {
  let bits = (secret_len * 8) as u32;
  if name == "HMAC" {
    let hash = hash_name(scope, algorithm)?;
    if !known_digest(&hash) {
      return None;
    }
    let length = match algorithm_number(scope, algorithm, "length") {
      None => bits,
      Some(number) => enforce_unsigned_long(number, "length").ok()?,
    };
    return (length == bits && length != 0 && length.is_multiple_of(8))
      .then_some(());
  }
  if matches!(
    name,
    "AES-GCM" | "AES-CBC" | "AES-CTR" | "AES-KW" | "AES-OCB"
  ) {
    let length = match algorithm_number(scope, algorithm, "length") {
      None => 256,
      Some(number) => enforce_unsigned_long(number, "length").ok()?,
    };
    return (length == bits && matches!(length, 128 | 192 | 256)).then_some(());
  }
  if is_chacha(name) {
    if let Some(number) = algorithm_number(scope, algorithm, "length") {
      enforce_unsigned_long(number, "length").ok()?;
    }
    return (secret_len == 32).then_some(());
  }
  if is_kmac(name) {
    let length = match algorithm_number(scope, algorithm, "length") {
      None => bits,
      Some(number) => enforce_unsigned_long(number, "length").ok()?,
    };
    return (length == bits).then_some(());
  }
  None
}

enum Peer {
  Absent,
  Invalid,
  Key(KeyView),
}

fn peer_state(
  scope: &mut v8::PinScope,
  name: &str,
  algorithm: Local<v8::Value>,
) -> Peer {
  let Some(object) = dict(algorithm) else {
    return Peer::Absent;
  };
  let key = v8::String::new(scope, "public").unwrap();
  let Some(value) = object.get(scope, key.into()) else {
    return Peer::Invalid;
  };
  if value.is_undefined() {
    return Peer::Absent;
  }
  if value.is_null() {
    return Peer::Invalid;
  }
  match load_key(scope, value) {
    Ok(key) if key.kind == "public" && key.algorithm == name => Peer::Key(key),
    _ => Peer::Invalid,
  }
}

fn curve_bits(curve: &str) -> Option<u32> {
  match curve {
    "P-256" => Some(256),
    "P-384" => Some(384),
    "P-521" => Some(528),
    _ => None,
  }
}

fn hash_bits(name: &str) -> Option<u32> {
  match name {
    "SHA-1" => Some(160),
    "SHA-256" => Some(256),
    "SHA-384" => Some(384),
    "SHA-512" => Some(512),
    _ => None,
  }
}

fn hash_name(
  scope: &mut v8::PinScope,
  algorithm: Local<v8::Value>,
) -> Option<String> {
  match algorithm_hash(scope, algorithm) {
    Ok(Some(hash)) => Some(hash),
    Ok(None) => Some("SHA-256".to_string()),
    Err(_) => None,
  }
}

fn unsigned(
  scope: &mut v8::PinScope,
  algorithm: Local<v8::Value>,
  name: &str,
) -> Option<u32> {
  let number = algorithm_number(scope, algorithm, name)?;
  enforce_unsigned_long(number, name).ok()
}

fn member_present(
  scope: &mut v8::PinScope,
  algorithm: Local<v8::Value>,
  name: &str,
) -> bool {
  let Some(object) = dict(algorithm) else {
    return false;
  };
  let key = v8::String::new(scope, name).unwrap();
  object
    .get(scope, key.into())
    .is_some_and(|value| !value.is_undefined())
}

fn bytes_member(
  scope: &mut v8::PinScope,
  algorithm: Local<v8::Value>,
  name: &str,
) -> Option<Vec<u8>> {
  if !member_present(scope, algorithm, name) {
    return None;
  }
  let object = dict(algorithm)?;
  let key = v8::String::new(scope, name).unwrap();
  let value = object.get(scope, key.into())?;
  buffer(scope, value).ok()
}

fn optional_bytes_ok(
  scope: &mut v8::PinScope,
  algorithm: Local<v8::Value>,
  name: &str,
) -> bool {
  if !member_present(scope, algorithm, name) {
    return true;
  }
  bytes_member(scope, algorithm, name).is_some()
}
