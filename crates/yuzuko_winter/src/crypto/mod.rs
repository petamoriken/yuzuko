//! `crypto` and `SubtleCrypto` for digest, HMAC, AES-GCM, AES-CBC, AES-CTR,
//! AES-KW, HKDF, PBKDF2, RSASSA-PKCS1-v1_5, RSA-PSS, RSA-OAEP, ECDSA, ECDH,
//! Ed25519, Ed448, X25519, and X448. HMAC and AES accept `raw` and `jwk`. RSA accepts
//! `pkcs8`, `spki`, and `jwk`. Elliptic-curve and OKP keys accept `raw`,
//! `pkcs8`, `spki`, and `jwk`.

#![allow(clippy::too_many_arguments)]

pub(crate) mod ec_key;
pub(crate) mod okp_key;
pub(crate) mod rsa_key;

use std::cell::RefCell;

use aes_gcm::AesGcm;
use aes_gcm::KeyInit;
use aes_gcm::aead::Aead;
use aes_gcm::aead::Payload;
use aes_gcm::aes::Aes128;
use aes_gcm::aes::Aes192;
use aes_gcm::aes::Aes256;
use aes_gcm::aes::cipher::BlockCipherDecrypt;
use aes_gcm::aes::cipher::BlockCipherEncrypt;
use aes_gcm::aes::cipher::BlockSizeUser;
use aes_gcm::aes::cipher::consts::U16;

use hmac::Mac;
use sha1::Sha1;
use sha2::Digest;
use sha2::Sha256;
use sha2::Sha384;
use sha2::Sha512;
use yuzuko_core::op::Js;
use yuzuko_core::op::OpError;
use yuzuko_core::op::traced;
use yuzuko_core::op::traced_local;
use yuzuko_macros::op;

use crate::bytes;
use crate::text::base64;

enum Material {
  Bytes(Vec<u8>),
}

pub struct CryptoKey {
  kind: String,
  extractable: bool,
  algorithm: String,
  hash: Option<String>,
  curve: Option<String>,
  length: u32,
  modulus_length: u32,
  public_exponent: Vec<u8>,
  usages: Vec<String>,
  material: Material,
  algorithm_object: RefCell<Option<v8::TracedReference<v8::Object>>>,
  usages_object: RefCell<Option<v8::TracedReference<v8::Object>>>,
}

unsafe impl v8::cppgc::GarbageCollected for CryptoKey {
  fn trace(&self, visitor: &mut v8::cppgc::Visitor) {
    if let Ok(algorithm) = self.algorithm_object.try_borrow() {
      visitor.trace(&*algorithm);
    }
    if let Ok(usages) = self.usages_object.try_borrow() {
      visitor.trace(&*usages);
    }
  }

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"CryptoKey"
  }
}

#[op]
impl CryptoKey {
  #[constructor]
  fn new() -> Result<Self, OpError> {
    Err(OpError::type_("Illegal constructor"))
  }

  #[getter]
  #[rename("type")]
  fn type_(&self) -> String {
    self.kind.clone()
  }

  #[getter]
  fn extractable(&self) -> bool {
    self.extractable
  }

  #[getter]
  fn algorithm(&self, scope: &mut v8::PinScope) -> Js {
    if let Some(existing) = self.algorithm_object.borrow().as_ref()
      && let Some(object) = traced_local(scope, existing)
    {
      let value: v8::Local<v8::Value> = object.into();
      return Js(v8::Global::new(scope, value));
    }
    let object = v8::Object::new(scope);
    let name = v8::String::new(scope, &self.algorithm).unwrap();
    let name_key = v8::String::new(scope, "name").unwrap();
    object.set(scope, name_key.into(), name.into());
    if let Some(hash) = &self.hash {
      let hash_object = v8::Object::new(scope);
      let hash_name = v8::String::new(scope, hash).unwrap();
      hash_object.set(scope, name_key.into(), hash_name.into());
      let hash_key = v8::String::new(scope, "hash").unwrap();
      object.set(scope, hash_key.into(), hash_object.into());
    }
    if let Some(curve) = &self.curve {
      let curve_key = v8::String::new(scope, "namedCurve").unwrap();
      let curve_name = v8::String::new(scope, curve).unwrap();
      object.set(scope, curve_key.into(), curve_name.into());
    }
    if self.length != 0 {
      let length_key = v8::String::new(scope, "length").unwrap();
      object.set(
        scope,
        length_key.into(),
        v8::Number::new(scope, f64::from(self.length)).into(),
      );
    }
    if self.modulus_length != 0 {
      let modulus_key = v8::String::new(scope, "modulusLength").unwrap();
      object.set(
        scope,
        modulus_key.into(),
        v8::Number::new(scope, f64::from(self.modulus_length)).into(),
      );
      let exponent = bytes::uint8_array(scope, self.public_exponent.clone());
      let exponent_key = v8::String::new(scope, "publicExponent").unwrap();
      object.set(scope, exponent_key.into(), exponent.into());
    }
    *self.algorithm_object.borrow_mut() = Some(traced(scope, object));
    let value: v8::Local<v8::Value> = object.into();
    Js(v8::Global::new(scope, value))
  }

  #[getter]
  fn usages(&self, scope: &mut v8::PinScope) -> Js {
    if let Some(existing) = self.usages_object.borrow().as_ref()
      && let Some(object) = traced_local(scope, existing)
    {
      let value: v8::Local<v8::Value> = object.into();
      return Js(v8::Global::new(scope, value));
    }
    let array = v8::Array::new(scope, self.usages.len() as i32);
    for (index, usage) in self.usages.iter().enumerate() {
      let text = v8::String::new(scope, usage).unwrap();
      array.set_index(scope, index as u32, text.into());
    }
    let object: v8::Local<v8::Object> = array.into();
    *self.usages_object.borrow_mut() = Some(traced(scope, object));
    let value: v8::Local<v8::Value> = array.into();
    Js(v8::Global::new(scope, value))
  }
}

/// Host clone tag. Tags `1` through `10` belong to other host objects.
const TAG_CRYPTO_KEY: u32 = 11;

pub(crate) fn register_clone() {
  yuzuko_core::add_host_clone(write_crypto_key_clone, read_crypto_key_clone);
}

fn write_crypto_key_clone(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  helper: &dyn v8::ValueSerializerHelper,
) -> yuzuko_core::WriteResult {
  let Some(host) =
    (unsafe { yuzuko_core::op::host_ref::<CryptoKey>(scope, object) })
  else {
    return yuzuko_core::WriteResult::Skip;
  };
  let key = host.get();
  let Material::Bytes(bytes) = &key.material;
  let kind = key.kind.clone();
  let algorithm = key.algorithm.clone();
  let hash = key.hash.clone();
  let curve = key.curve.clone();
  let extractable = key.extractable;
  let length = key.length;
  let modulus_length = key.modulus_length;
  let exponent = key.public_exponent.clone();
  let usages = key.usages.clone();
  let bytes = bytes.clone();
  helper.write_uint32(TAG_CRYPTO_KEY);
  if !yuzuko_core::write_text(helper, &kind)
    || !yuzuko_core::write_text(helper, &algorithm)
    || !write_optional_text(helper, hash.as_deref())
    || !write_optional_text(helper, curve.as_deref())
  {
    yuzuko_core::reject_clone(scope);
    return yuzuko_core::WriteResult::Failed;
  }
  helper.write_uint32(u32::from(extractable));
  helper.write_uint32(length);
  helper.write_uint32(modulus_length);
  yuzuko_core::write_bytes(helper, &exponent);
  helper.write_uint32(usages.len() as u32);
  for usage in &usages {
    if !yuzuko_core::write_text(helper, usage) {
      yuzuko_core::reject_clone(scope);
      return yuzuko_core::WriteResult::Failed;
    }
  }
  yuzuko_core::write_bytes(helper, &bytes);
  yuzuko_core::WriteResult::Wrote
}

fn read_crypto_key_clone(
  scope: &mut v8::PinScope,
  tag: u32,
  helper: &dyn v8::ValueDeserializerHelper,
) -> yuzuko_core::ReadResult {
  if tag != TAG_CRYPTO_KEY {
    return yuzuko_core::ReadResult::Skip;
  }
  let Some(key) = read_cloned_key(helper) else {
    yuzuko_core::reject_clone(scope);
    return yuzuko_core::ReadResult::Failed;
  };
  let object = unsafe { yuzuko_core::op::wrap_instance(scope, key) };
  yuzuko_core::ReadResult::Done(v8::Global::new(scope, object))
}

fn read_cloned_key(
  helper: &dyn v8::ValueDeserializerHelper,
) -> Option<CryptoKey> {
  let kind = yuzuko_core::read_text(helper)?;
  let algorithm = yuzuko_core::read_text(helper)?;
  let hash = read_optional_text(helper)?;
  let curve = read_optional_text(helper)?;
  let mut extractable = 0u32;
  let mut length = 0u32;
  let mut modulus_length = 0u32;
  if !helper.read_uint32(&mut extractable)
    || !helper.read_uint32(&mut length)
    || !helper.read_uint32(&mut modulus_length)
  {
    return None;
  }
  let public_exponent = yuzuko_core::read_bytes(helper)?;
  let mut count = 0u32;
  if !helper.read_uint32(&mut count) {
    return None;
  }
  let mut usages = Vec::with_capacity(count as usize);
  for _ in 0..count {
    usages.push(yuzuko_core::read_text(helper)?);
  }
  let bytes = yuzuko_core::read_bytes(helper)?;
  Some(CryptoKey {
    kind,
    extractable: extractable != 0,
    algorithm,
    hash,
    curve,
    length,
    modulus_length,
    public_exponent,
    usages,
    material: Material::Bytes(bytes),
    algorithm_object: RefCell::new(None),
    usages_object: RefCell::new(None),
  })
}

fn write_optional_text(
  helper: &dyn v8::ValueSerializerHelper,
  text: Option<&str>,
) -> bool {
  match text {
    Some(text) => {
      helper.write_uint32(1);
      yuzuko_core::write_text(helper, text)
    }
    None => {
      helper.write_uint32(0);
      true
    }
  }
}

fn read_optional_text(
  helper: &dyn v8::ValueDeserializerHelper,
) -> Option<Option<String>> {
  let mut present = 0u32;
  if !helper.read_uint32(&mut present) {
    return None;
  }
  if present == 0 {
    return Some(None);
  }
  yuzuko_core::read_text(helper).map(Some)
}

pub struct SubtleCrypto;

unsafe impl v8::cppgc::GarbageCollected for SubtleCrypto {
  fn trace(&self, _visitor: &mut v8::cppgc::Visitor) {}

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"SubtleCrypto"
  }
}

#[op]
impl SubtleCrypto {
  #[constructor]
  fn new() -> Result<Self, OpError> {
    Err(OpError::type_("Illegal constructor"))
  }

  #[promise]
  fn digest(
    &self,
    scope: &mut v8::PinScope,
    algorithm: v8::Local<v8::Value>,
    data: v8::Local<v8::Value>,
  ) -> Js {
    let _ = self;
    match digest_bytes(scope, algorithm, data) {
      Ok(bytes) => resolve_bytes(scope, bytes),
      Err(error) => reject(scope, error),
    }
  }

  #[promise]
  fn generate_key(
    &self,
    scope: &mut v8::PinScope,
    algorithm: v8::Local<v8::Value>,
    extractable: bool,
    usages: v8::Local<v8::Value>,
  ) -> Js {
    let _ = self;
    match generate_key(scope, algorithm, extractable, usages) {
      Ok(Generated::Secret(key)) => resolve_key(scope, *key),
      Ok(Generated::Pair { public, private }) => {
        resolve_pair(scope, *public, *private)
      }
      Err(error) => reject(scope, error),
    }
  }

  #[promise]
  fn import_key(
    &self,
    scope: &mut v8::PinScope,
    format: String,
    key_data: v8::Local<v8::Value>,
    algorithm: v8::Local<v8::Value>,
    extractable: bool,
    usages: v8::Local<v8::Value>,
  ) -> Js {
    let _ = self;
    match import_key(scope, &format, key_data, algorithm, extractable, usages) {
      Ok(key) => resolve_key(scope, key),
      Err(error) => reject(scope, error),
    }
  }

  #[promise]
  fn export_key(
    &self,
    scope: &mut v8::PinScope,
    format: String,
    key: v8::Local<v8::Value>,
  ) -> Js {
    let _ = self;
    match export_key(scope, &format, key) {
      Ok(Exported::Raw(bytes)) => resolve_bytes(scope, bytes),
      Ok(Exported::Jwk(jwk)) => {
        let value = jwk_value(scope, &jwk);
        resolve(scope, value)
      }
      Ok(Exported::Rsa {
        jwk,
        alg,
        key_ops,
        ext,
      }) => {
        let value = rsa_jwk_value(scope, &jwk, &alg, &key_ops, ext);
        resolve(scope, value)
      }
      Ok(Exported::Ec { jwk, key_ops, ext }) => {
        let value = ec_jwk_value(scope, &jwk, &key_ops, ext);
        resolve(scope, value)
      }
      Ok(Exported::Okp { jwk, key_ops, ext }) => {
        let value = okp_jwk_value(scope, &jwk, &key_ops, ext);
        resolve(scope, value)
      }
      Err(error) => reject(scope, error),
    }
  }

  #[promise]
  fn wrap_key(
    &self,
    scope: &mut v8::PinScope,
    format: String,
    key: v8::Local<v8::Value>,
    wrapping_key: v8::Local<v8::Value>,
    algorithm: v8::Local<v8::Value>,
  ) -> Js {
    let _ = self;
    match wrap_key(scope, &format, key, wrapping_key, algorithm) {
      Ok(bytes) => resolve_bytes(scope, bytes),
      Err(error) => reject(scope, error),
    }
  }

  #[allow(clippy::too_many_arguments)]
  #[promise]
  fn unwrap_key(
    &self,
    scope: &mut v8::PinScope,
    format: String,
    wrapped: v8::Local<v8::Value>,
    unwrapping_key: v8::Local<v8::Value>,
    unwrap_algorithm: v8::Local<v8::Value>,
    unwrapped_algorithm: v8::Local<v8::Value>,
    extractable: bool,
    usages: v8::Local<v8::Value>,
  ) -> Js {
    let _ = self;
    match unwrap_key(
      scope,
      &format,
      wrapped,
      unwrapping_key,
      unwrap_algorithm,
      unwrapped_algorithm,
      extractable,
      usages,
    ) {
      Ok(key) => resolve_key(scope, key),
      Err(error) => reject(scope, error),
    }
  }

  #[promise]
  fn sign(
    &self,
    scope: &mut v8::PinScope,
    algorithm: v8::Local<v8::Value>,
    key: v8::Local<v8::Value>,
    data: v8::Local<v8::Value>,
  ) -> Js {
    let _ = self;
    match sign_bytes(scope, algorithm, key, data) {
      Ok(bytes) => resolve_bytes(scope, bytes),
      Err(error) => reject(scope, error),
    }
  }

  #[promise]
  fn verify(
    &self,
    scope: &mut v8::PinScope,
    algorithm: v8::Local<v8::Value>,
    key: v8::Local<v8::Value>,
    signature: v8::Local<v8::Value>,
    data: v8::Local<v8::Value>,
  ) -> Js {
    let _ = self;
    match verify_bytes(scope, algorithm, key, signature, data) {
      Ok(valid) => resolve_bool(scope, valid),
      Err(error) => reject(scope, error),
    }
  }

  #[promise]
  fn encrypt(
    &self,
    scope: &mut v8::PinScope,
    algorithm: v8::Local<v8::Value>,
    key: v8::Local<v8::Value>,
    data: v8::Local<v8::Value>,
  ) -> Js {
    let _ = self;
    match crypt_bytes(scope, algorithm, key, data, true) {
      Ok(bytes) => resolve_bytes(scope, bytes),
      Err(error) => reject(scope, error),
    }
  }

  #[promise]
  fn decrypt(
    &self,
    scope: &mut v8::PinScope,
    algorithm: v8::Local<v8::Value>,
    key: v8::Local<v8::Value>,
    data: v8::Local<v8::Value>,
  ) -> Js {
    let _ = self;
    match crypt_bytes(scope, algorithm, key, data, false) {
      Ok(bytes) => resolve_bytes(scope, bytes),
      Err(error) => reject(scope, error),
    }
  }

  #[promise]
  #[length(2)]
  fn derive_bits(
    &self,
    scope: &mut v8::PinScope,
    algorithm: v8::Local<v8::Value>,
    key: v8::Local<v8::Value>,
    length: v8::Local<v8::Value>,
  ) -> Js {
    let _ = self;
    match derive_bits(scope, algorithm, key, length) {
      Ok(bytes) => resolve_bytes(scope, bytes),
      Err(error) => reject(scope, error),
    }
  }

  #[promise]
  fn derive_key(
    &self,
    scope: &mut v8::PinScope,
    algorithm: v8::Local<v8::Value>,
    base: v8::Local<v8::Value>,
    derived: v8::Local<v8::Value>,
    extractable: bool,
    usages: v8::Local<v8::Value>,
  ) -> Js {
    let _ = self;
    match derive_key(scope, algorithm, base, derived, extractable, usages) {
      Ok(key) => resolve_key(scope, key),
      Err(error) => reject(scope, error),
    }
  }

  #[promise]
  fn encapsulate_key(
    &self,
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
    _algorithm: v8::Local<v8::Value>,
    _key: v8::Local<v8::Value>,
    _encapsulated: v8::Local<v8::Value>,
    _extractable: bool,
    _usages: v8::Local<v8::Value>,
  ) -> Js {
    let _ = self;
    modern_method(scope, args, 5, "encapsulateKey")
  }

  #[promise]
  fn encapsulate_bits(
    &self,
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
    _algorithm: v8::Local<v8::Value>,
    _key: v8::Local<v8::Value>,
  ) -> Js {
    let _ = self;
    modern_method(scope, args, 2, "encapsulateBits")
  }

  #[promise]
  fn decapsulate_key(
    &self,
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
    _algorithm: v8::Local<v8::Value>,
    _key: v8::Local<v8::Value>,
    _ciphertext: v8::Local<v8::Value>,
    _unwrapped: v8::Local<v8::Value>,
    _extractable: bool,
    _usages: v8::Local<v8::Value>,
  ) -> Js {
    let _ = self;
    modern_method(scope, args, 6, "decapsulateKey")
  }

  #[promise]
  fn decapsulate_bits(
    &self,
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
    _algorithm: v8::Local<v8::Value>,
    _key: v8::Local<v8::Value>,
    _ciphertext: v8::Local<v8::Value>,
  ) -> Js {
    let _ = self;
    modern_method(scope, args, 3, "decapsulateBits")
  }

  #[promise]
  #[rename("getPublicKey")]
  fn get_public_key(
    &self,
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
    _key: v8::Local<v8::Value>,
    _usages: v8::Local<v8::Value>,
  ) -> Js {
    let _ = self;
    modern_method(scope, args, 2, "getPublicKey")
  }

  #[static_method]
  fn supports(
    args: &v8::FunctionCallbackArguments,
    _operation: v8::Local<v8::Value>,
    _algorithm: v8::Local<v8::Value>,
    _third: Option<v8::Local<v8::Value>>,
  ) -> Result<(), OpError> {
    if args.length() < 2 {
      return Err(OpError::type_(
        "SubtleCrypto.supports requires an operation and an algorithm",
      ));
    }
    Err(OpError::dom(
      "NotSupportedError",
      "SubtleCrypto.supports is not supported",
    ))
  }
}

fn modern_method(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
  needed: i32,
  name: &str,
) -> Js {
  if args.length() < needed {
    return reject(
      scope,
      OpError::type_(format!("{name} requires {needed} arguments")),
    );
  }
  reject(
    scope,
    OpError::dom("NotSupportedError", format!("{name} is not supported")),
  )
}

fn digest_bytes(
  scope: &mut v8::PinScope,
  algorithm: v8::Local<v8::Value>,
  data: v8::Local<v8::Value>,
) -> Result<Vec<u8>, OpError> {
  let name = algorithm_name(scope, algorithm)?;
  let bytes = buffer(scope, data)?;
  Ok(match name.as_str() {
    "SHA-1" => Sha1::digest(&bytes).to_vec(),
    "SHA-256" => Sha256::digest(&bytes).to_vec(),
    "SHA-384" => Sha384::digest(&bytes).to_vec(),
    "SHA-512" => Sha512::digest(&bytes).to_vec(),
    _ => {
      return Err(OpError::dom(
        "NotSupportedError",
        "The digest algorithm is not supported",
      ));
    }
  })
}

fn generate_key(
  scope: &mut v8::PinScope,
  algorithm: v8::Local<v8::Value>,
  extractable: bool,
  usages: v8::Local<v8::Value>,
) -> Result<Generated, OpError> {
  let name = algorithm_name(scope, algorithm)?;
  let usages = normalize_usages(read_usages(scope, usages)?);
  match name.as_str() {
    "HMAC" => {
      let hash = algorithm_hash(scope, algorithm)?
        .unwrap_or_else(|| "SHA-256".to_string());
      // An unrecognized hash is NotSupportedError before usages are checked.
      if !known_digest(&hash) {
        return Err(OpError::dom(
          "NotSupportedError",
          "The hash algorithm is not supported",
        ));
      }
      reject_unknown_usages(&name, &usages)?;
      // A length outside the unsigned long range is TypeError even when
      // `usages` is empty. Empty usages are checked after the length.
      let length = hmac_generate_length(scope, algorithm, &hash)?;
      reject_empty_usages(&name, &usages)?;
      let bytes = random_bytes(length)?;
      Ok(Generated::Secret(Box::new(secret_key(
        "HMAC",
        extractable,
        Some(hash),
        length,
        usages,
        bytes,
      ))))
    }
    "AES-GCM" | "AES-CBC" | "AES-CTR" | "AES-KW" => {
      reject_unknown_usages(&name, &usages)?;
      let length = aes_generate_length(scope, algorithm)?;
      reject_empty_usages(&name, &usages)?;
      let bytes = random_bytes(length)?;
      Ok(Generated::Secret(Box::new(secret_key(
        name,
        extractable,
        None,
        length,
        usages,
        bytes,
      ))))
    }
    "ECDH" | "ECDSA" => {
      generate_ec(scope, &name, algorithm, extractable, &usages)
    }
    "Ed25519" | "Ed448" | "X25519" | "X448" => {
      generate_okp(&name, extractable, &usages)
    }
    "RSASSA-PKCS1-v1_5" | "RSA-PSS" | "RSA-OAEP" => {
      generate_rsa(scope, &name, algorithm, extractable, &usages)
    }
    "HKDF" | "PBKDF2" => {
      reject_unknown_usages(&name, &usages)?;
      Err(OpError::dom(
        "NotSupportedError",
        "generateKey is not used for this algorithm",
      ))
    }
    _ => Err(OpError::dom(
      "NotSupportedError",
      "The key algorithm is not supported",
    )),
  }
}

fn import_key(
  scope: &mut v8::PinScope,
  format: &str,
  key_data: v8::Local<v8::Value>,
  algorithm: v8::Local<v8::Value>,
  extractable: bool,
  usages: v8::Local<v8::Value>,
) -> Result<CryptoKey, OpError> {
  let name = algorithm_name(scope, algorithm)?;
  let usages = normalize_usages(read_usages(scope, usages)?);
  if is_ec(&name) {
    return import_ec(
      scope,
      &name,
      format,
      key_data,
      algorithm,
      extractable,
      usages,
    );
  }
  if is_okp(&name) {
    return import_okp(scope, &name, format, key_data, extractable, usages);
  }
  if is_rsa(&name) {
    return import_rsa(
      scope,
      &name,
      format,
      key_data,
      algorithm,
      extractable,
      usages,
    );
  }
  check_usages(&name, &usages)?;
  if format.eq_ignore_ascii_case("raw") {
    let bytes = buffer(scope, key_data)?;
    return import_oct_key(scope, &name, algorithm, extractable, usages, bytes);
  }
  if format.eq_ignore_ascii_case("jwk") {
    return import_jwk(scope, &name, key_data, algorithm, extractable, usages);
  }
  Err(OpError::dom(
    "NotSupportedError",
    "The key format is not supported",
  ))
}

fn import_oct_key(
  scope: &mut v8::PinScope,
  name: &str,
  algorithm: v8::Local<v8::Value>,
  extractable: bool,
  usages: Vec<String>,
  bytes: Vec<u8>,
) -> Result<CryptoKey, OpError> {
  match name {
    "HMAC" => {
      let hash = algorithm_hash(scope, algorithm)?
        .unwrap_or_else(|| "SHA-256".to_string());
      let length = (bytes.len() * 8) as u32;
      Ok(secret_key(
        "HMAC",
        extractable,
        Some(hash),
        length,
        usages,
        bytes,
      ))
    }
    "AES-GCM" | "AES-CBC" | "AES-CTR" | "AES-KW" => {
      let length = aes_key_bits(&bytes)?;
      Ok(secret_key(name, extractable, None, length, usages, bytes))
    }
    "HKDF" | "PBKDF2" => {
      if extractable {
        return Err(OpError::dom("SyntaxError", "The key is not extractable"));
      }
      Ok(secret_key(
        name,
        false,
        algorithm_hash(scope, algorithm)?,
        0,
        usages,
        bytes,
      ))
    }
    _ => Err(OpError::dom(
      "NotSupportedError",
      "The key algorithm is not supported",
    )),
  }
}

struct OctJwk {
  kty: Option<String>,
  k: Option<String>,
  alg: Option<String>,
  use_: Option<String>,
  key_ops: Option<Vec<String>>,
  ext: Option<bool>,
  n: Option<String>,
  e: Option<String>,
  d: Option<String>,
  p: Option<String>,
  q: Option<String>,
  dp: Option<String>,
  dq: Option<String>,
  qi: Option<String>,
  crv: Option<String>,
  x: Option<String>,
  y: Option<String>,
}

fn import_jwk(
  scope: &mut v8::PinScope,
  name: &str,
  key_data: v8::Local<v8::Value>,
  algorithm: v8::Local<v8::Value>,
  extractable: bool,
  usages: Vec<String>,
) -> Result<CryptoKey, OpError> {
  if name != "HMAC" && !is_aes_key(name) {
    return Err(OpError::dom(
      "NotSupportedError",
      "The key format is not supported",
    ));
  }
  let jwk = read_jwk(scope, key_data)?;
  if jwk.kty.as_deref() != Some("oct") {
    return Err(OpError::dom("DataError", "The JWK kty is invalid"));
  }
  let Some(k) = jwk.k.as_deref() else {
    return Err(OpError::dom("DataError", "The JWK k member is missing"));
  };
  let bytes = base64::decode_url(k).map_err(|_| {
    OpError::dom("DataError", "The JWK k member is not base64url")
  })?;
  match name {
    "HMAC" => {
      let hash = algorithm_hash(scope, algorithm)?
        .unwrap_or_else(|| "SHA-256".to_string());
      hmac_alg(&hash, jwk.alg.as_deref())?;
      check_use(&usages, jwk.use_.as_deref(), "sig")?;
    }
    name if is_aes_key(name) => {
      aes_jwk_alg(name, &bytes, jwk.alg.as_deref())?;
      check_use(&usages, jwk.use_.as_deref(), "enc")?;
    }
    _ => unreachable!(),
  }
  check_key_ops(jwk.key_ops.as_deref(), &usages)?;
  check_ext(jwk.ext, extractable)?;
  import_oct_key(scope, name, algorithm, extractable, usages, bytes)
}

fn hmac_alg(hash: &str, alg: Option<&str>) -> Result<(), OpError> {
  let Some(alg) = alg else {
    return Ok(());
  };
  let expected = match hash {
    "SHA-1" => "HS1",
    "SHA-256" => "HS256",
    "SHA-384" => "HS384",
    "SHA-512" => "HS512",
    _ => {
      return Err(OpError::dom("DataError", "The JWK alg is invalid"));
    }
  };
  if alg == expected {
    Ok(())
  } else {
    Err(OpError::dom("DataError", "The JWK alg is invalid"))
  }
}

fn aes_jwk_alg(
  algorithm: &str,
  bytes: &[u8],
  alg: Option<&str>,
) -> Result<(), OpError> {
  let bits = aes_key_bits(bytes)?;
  let Some(expected) = aes_jwk_name(algorithm, bits) else {
    return Err(OpError::dom("DataError", "The AES key length is invalid"));
  };
  if alg.is_none_or(|alg| alg == expected) {
    Ok(())
  } else {
    Err(OpError::dom("DataError", "The JWK alg is invalid"))
  }
}

fn aes_jwk_name(algorithm: &str, bits: u32) -> Option<&'static str> {
  Some(match (algorithm, bits) {
    ("AES-GCM", 128) => "A128GCM",
    ("AES-GCM", 192) => "A192GCM",
    ("AES-GCM", 256) => "A256GCM",
    ("AES-CBC", 128) => "A128CBC",
    ("AES-CBC", 192) => "A192CBC",
    ("AES-CBC", 256) => "A256CBC",
    ("AES-CTR", 128) => "A128CTR",
    ("AES-CTR", 192) => "A192CTR",
    ("AES-CTR", 256) => "A256CTR",
    ("AES-KW", 128) => "A128KW",
    ("AES-KW", 192) => "A192KW",
    ("AES-KW", 256) => "A256KW",
    _ => return None,
  })
}

fn aes_key_bits(bytes: &[u8]) -> Result<u32, OpError> {
  match bytes.len() {
    16 => Ok(128),
    24 => Ok(192),
    32 => Ok(256),
    _ => Err(OpError::dom("DataError", "The AES key length is invalid")),
  }
}

fn is_derivation_algorithm(name: &str) -> bool {
  matches!(name, "HKDF" | "PBKDF2" | "ECDH" | "X25519" | "X448")
}

fn is_aes(name: &str) -> bool {
  matches!(name, "AES-GCM" | "AES-CBC" | "AES-CTR")
}

fn is_aes_key(name: &str) -> bool {
  is_aes(name) || name == "AES-KW"
}

fn check_use(
  usages: &[String],
  use_: Option<&str>,
  expected: &str,
) -> Result<(), OpError> {
  if !usages.is_empty()
    && let Some(use_) = use_
    && use_ != expected
  {
    return Err(OpError::dom("DataError", "The JWK use is invalid"));
  }
  Ok(())
}

fn check_key_ops(
  key_ops: Option<&[String]>,
  usages: &[String],
) -> Result<(), OpError> {
  let Some(key_ops) = key_ops else {
    return Ok(());
  };
  let mut seen = Vec::new();
  for operation in key_ops {
    if !matches!(
      operation.as_str(),
      "sign"
        | "verify"
        | "encrypt"
        | "decrypt"
        | "wrapKey"
        | "unwrapKey"
        | "deriveKey"
        | "deriveBits"
    ) || seen.contains(operation)
    {
      return Err(OpError::dom("DataError", "The JWK key_ops are invalid"));
    }
    seen.push(operation.clone());
  }
  if usages.iter().any(|usage| !key_ops.contains(usage)) {
    return Err(OpError::dom("DataError", "The JWK key_ops are invalid"));
  }
  Ok(())
}

fn check_ext(ext: Option<bool>, extractable: bool) -> Result<(), OpError> {
  if ext == Some(false) && extractable {
    return Err(OpError::dom("DataError", "The JWK ext member is false"));
  }
  Ok(())
}

fn read_jwk(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<OctJwk, OpError> {
  let object = v8::Local::<v8::Object>::try_from(value)
    .map_err(|_| OpError::type_("The JWK is not an object"))?;
  Ok(OctJwk {
    kty: optional_string(scope, object, "kty")?,
    k: optional_string(scope, object, "k")?,
    alg: optional_string(scope, object, "alg")?,
    use_: optional_string(scope, object, "use")?,
    key_ops: optional_strings(scope, object, "key_ops")?,
    ext: optional_bool(scope, object, "ext")?,
    n: optional_string(scope, object, "n")?,
    e: optional_string(scope, object, "e")?,
    d: optional_string(scope, object, "d")?,
    p: optional_string(scope, object, "p")?,
    q: optional_string(scope, object, "q")?,
    dp: optional_string(scope, object, "dp")?,
    dq: optional_string(scope, object, "dq")?,
    qi: optional_string(scope, object, "qi")?,
    crv: optional_string(scope, object, "crv")?,
    x: optional_string(scope, object, "x")?,
    y: optional_string(scope, object, "y")?,
  })
}

enum Exported {
  Raw(Vec<u8>),
  Jwk(PublicJwk),
  Rsa {
    jwk: crate::crypto::rsa_key::RsaJwk,
    alg: String,
    key_ops: Vec<String>,
    ext: bool,
  },
  Ec {
    jwk: crate::crypto::ec_key::EcJwk,
    key_ops: Vec<String>,
    ext: bool,
  },
  Okp {
    jwk: crate::crypto::okp_key::OkpJwk,
    key_ops: Vec<String>,
    ext: bool,
  },
}

struct PublicJwk {
  k: String,
  alg: String,
  key_ops: Vec<String>,
  ext: bool,
}

fn export_key(
  scope: &mut v8::PinScope,
  format: &str,
  key: v8::Local<v8::Value>,
) -> Result<Exported, OpError> {
  let key = load_key(scope, key)?;
  // Formats the algorithm does not support are NotSupportedError before the
  // extractable check. HKDF export is one case; AES `raw` is the other.
  if !export_supported(&key.algorithm, format) {
    return Err(OpError::dom(
      "NotSupportedError",
      "The key format is not supported",
    ));
  }
  if !key.extractable {
    return Err(OpError::dom(
      "InvalidAccessError",
      "The key is not extractable",
    ));
  }
  match format.to_ascii_lowercase().as_str() {
    "raw" => {
      if is_ec(&key.algorithm) {
        if key.kind != "public" {
          return Err(OpError::dom(
            "NotSupportedError",
            "The key format is not supported",
          ));
        }
        let curve = key.curve.as_deref().unwrap_or("");
        let raw = crate::crypto::ec_key::raw_public(curve, &key.bytes)
          .map_err(ec_error)?;
        return Ok(Exported::Raw(raw));
      }
      if is_okp(&key.algorithm) {
        if key.kind != "public" {
          return Err(OpError::dom(
            "NotSupportedError",
            "The key format is not supported",
          ));
        }
        let raw =
          crate::crypto::okp_key::raw_public(&key.algorithm, &key.bytes)
            .map_err(okp_error)?;
        return Ok(Exported::Raw(raw));
      }
      return Ok(Exported::Raw(key.bytes));
    }
    "pkcs8" => {
      if key.kind != "private" {
        return Err(OpError::dom(
          "InvalidAccessError",
          "The key is not a private key",
        ));
      }
      return Ok(Exported::Raw(key.bytes));
    }
    "spki" => {
      if key.kind != "public" {
        return Err(OpError::dom(
          "InvalidAccessError",
          "The key is not a public key",
        ));
      }
      return Ok(Exported::Raw(key.bytes));
    }
    "jwk" if is_rsa(&key.algorithm) => {
      let jwk = if key.kind == "private" {
        crate::crypto::rsa_key::private_jwk(&key.bytes).map_err(rsa_error)?
      } else {
        crate::crypto::rsa_key::public_jwk(&key.bytes).map_err(rsa_error)?
      };
      let alg = crate::crypto::rsa_key::jwk_alg(
        &key.algorithm,
        key.hash.as_deref().unwrap_or(""),
      )
      .unwrap_or("")
      .to_string();
      return Ok(Exported::Rsa {
        jwk,
        alg,
        key_ops: key.usages,
        ext: key.extractable,
      });
    }
    "jwk" if is_ec(&key.algorithm) => {
      let curve = key.curve.as_deref().unwrap_or("");
      let jwk = if key.kind == "private" {
        crate::crypto::ec_key::private_jwk(curve, &key.bytes)
          .map_err(ec_error)?
      } else {
        crate::crypto::ec_key::public_jwk(curve, &key.bytes)
          .map_err(ec_error)?
      };
      return Ok(Exported::Ec {
        jwk,
        key_ops: key.usages,
        ext: key.extractable,
      });
    }
    "jwk" if is_okp(&key.algorithm) => {
      let jwk = if key.kind == "private" {
        crate::crypto::okp_key::private_jwk(&key.algorithm, &key.bytes)
          .map_err(okp_error)?
      } else {
        crate::crypto::okp_key::public_jwk(&key.algorithm, &key.bytes)
          .map_err(okp_error)?
      };
      return Ok(Exported::Okp {
        jwk,
        key_ops: key.usages,
        ext: key.extractable,
      });
    }
    _ => {}
  }
  let alg = match key.algorithm.as_str() {
    "HMAC" => {
      let hash = key.hash.as_deref().unwrap_or("SHA-256");
      match hash {
        "SHA-1" => "HS1",
        "SHA-256" => "HS256",
        "SHA-384" => "HS384",
        "SHA-512" => "HS512",
        _ => {
          return Err(OpError::dom(
            "NotSupportedError",
            "The HMAC hash is not supported",
          ));
        }
      }
    }
    "AES-GCM" | "AES-CBC" | "AES-CTR" | "AES-KW" => {
      let Some(alg) = aes_jwk_name(key.algorithm.as_str(), key.length) else {
        return Err(OpError::dom(
          "NotSupportedError",
          "The AES key length is not supported",
        ));
      };
      alg
    }
    _ => {
      return Err(OpError::dom(
        "NotSupportedError",
        "The key format is not supported",
      ));
    }
  };
  Ok(Exported::Jwk(PublicJwk {
    k: base64::encode_url(&key.bytes),
    alg: alg.to_string(),
    key_ops: key.usages,
    ext: true,
  }))
}

fn wrap_key(
  scope: &mut v8::PinScope,
  format: &str,
  key: v8::Local<v8::Value>,
  wrapping_key: v8::Local<v8::Value>,
  algorithm: v8::Local<v8::Value>,
) -> Result<Vec<u8>, OpError> {
  let wrap_name = algorithm_name(scope, algorithm)?;
  let plaintext = export_plaintext(scope, format, key)?;
  if wrap_name == "RSA-OAEP" {
    let (wrapping, label) =
      rsa_oaep_key(scope, algorithm, wrapping_key, "wrapKey")?;
    return oaep_run(&wrapping, &label, &plaintext, true);
  }
  let wrapping = wrapping_bytes(scope, wrapping_key, &wrap_name, "wrapKey")?;
  cipher_bytes(scope, algorithm, &wrapping, &plaintext, true)
}

#[allow(clippy::too_many_arguments)]
fn unwrap_key(
  scope: &mut v8::PinScope,
  format: &str,
  wrapped: v8::Local<v8::Value>,
  unwrapping_key: v8::Local<v8::Value>,
  unwrap_algorithm: v8::Local<v8::Value>,
  unwrapped_algorithm: v8::Local<v8::Value>,
  extractable: bool,
  usages: v8::Local<v8::Value>,
) -> Result<CryptoKey, OpError> {
  let wrap_name = algorithm_name(scope, unwrap_algorithm)?;
  let wrapped = buffer(scope, wrapped)?;
  let plaintext = if wrap_name == "RSA-OAEP" {
    let (unwrapping, label) =
      rsa_oaep_key(scope, unwrap_algorithm, unwrapping_key, "unwrapKey")?;
    oaep_run(&unwrapping, &label, &wrapped, false)?
  } else {
    let unwrapping =
      wrapping_bytes(scope, unwrapping_key, &wrap_name, "unwrapKey")?;
    cipher_bytes(scope, unwrap_algorithm, &unwrapping, &wrapped, false)?
  };
  let key_data = if format.eq_ignore_ascii_case("jwk") {
    let text = String::from_utf8(plaintext)
      .map_err(|_| OpError::dom("DataError", "The wrapped JWK is invalid"))?;
    let source = v8::String::new(scope, &text)
      .ok_or_else(|| OpError::dom("DataError", "The wrapped JWK is invalid"))?;
    v8::json::parse(scope, source)
      .ok_or_else(|| OpError::dom("DataError", "The wrapped JWK is invalid"))?
  } else {
    bytes::uint8_array(scope, plaintext).into()
  };
  import_key(
    scope,
    format,
    key_data,
    unwrapped_algorithm,
    extractable,
    usages,
  )
}

fn wrapping_bytes(
  scope: &mut v8::PinScope,
  key: v8::Local<v8::Value>,
  algorithm: &str,
  usage: &str,
) -> Result<Vec<u8>, OpError> {
  if !matches!(algorithm, "AES-KW" | "AES-GCM" | "AES-CBC" | "AES-CTR") {
    return Err(OpError::dom(
      "NotSupportedError",
      "The wrapping algorithm is not supported",
    ));
  }
  let key = load_key(scope, key)?;
  if key.algorithm != algorithm {
    return Err(OpError::dom(
      "InvalidAccessError",
      "The key algorithm does not match",
    ));
  }
  if !key.usages.iter().any(|item| item == usage) {
    return Err(OpError::dom(
      "InvalidAccessError",
      "The key usage is invalid",
    ));
  }
  Ok(key.bytes)
}

fn cipher_bytes(
  scope: &mut v8::PinScope,
  algorithm: v8::Local<v8::Value>,
  key: &[u8],
  data: &[u8],
  encrypting: bool,
) -> Result<Vec<u8>, OpError> {
  match algorithm_name(scope, algorithm)?.as_str() {
    "AES-KW" => {
      if encrypting {
        aes_kw_wrap(key, data)
      } else {
        aes_kw_unwrap(key, data)
      }
    }
    "AES-GCM" => {
      let tag_bits = match algorithm_number(scope, algorithm, "tagLength") {
        None => 128,
        Some(tag) => {
          let tag = enforce_octet(tag, "tagLength")?;
          if !matches!(tag, 32 | 64 | 96 | 104 | 112 | 120 | 128) {
            return Err(OpError::dom(
              "OperationError",
              "The AES-GCM tag length is invalid",
            ));
          }
          tag
        }
      };
      let iv = algorithm_bytes(scope, algorithm, "iv")?;
      let additional =
        optional_algorithm_bytes(scope, algorithm, "additionalData")?;
      gcm_crypt(key, &iv, &additional, data, encrypting, tag_bits)
    }
    "AES-CBC" => {
      let iv = algorithm_bytes(scope, algorithm, "iv")?;
      cbc_crypt(key, &iv, data, encrypting)
    }
    "AES-CTR" => {
      let counter = algorithm_bytes(scope, algorithm, "counter")?;
      let Some(length) = algorithm_number(scope, algorithm, "length") else {
        return Err(OpError::type_("The AES-CTR length is required"));
      };
      let length = enforce_octet(length, "length")?;
      if length == 0 || length > 128 {
        return Err(OpError::dom(
          "OperationError",
          "The AES-CTR counter length is invalid",
        ));
      }
      ctr_crypt(key, &counter, length, data)
    }
    _ => Err(OpError::dom(
      "NotSupportedError",
      "The wrapping algorithm is not supported",
    )),
  }
}

fn jwk_json(jwk: &PublicJwk) -> String {
  let operations = jwk
    .key_ops
    .iter()
    .map(|usage| format!("\"{usage}\""))
    .collect::<Vec<_>>()
    .join(",");
  format!(
    "{{\"kty\":\"oct\",\"k\":\"{}\",\"alg\":\"{}\",\"key_ops\":[{operations}],\"ext\":{}}}",
    jwk.k,
    jwk.alg,
    if jwk.ext { "true" } else { "false" }
  )
}

fn aes_kw_wrap(key: &[u8], plaintext: &[u8]) -> Result<Vec<u8>, OpError> {
  if plaintext.len() < 16 || !plaintext.len().is_multiple_of(8) {
    return Err(OpError::dom(
      "OperationError",
      "The AES-KW plaintext length is invalid",
    ));
  }
  let n = plaintext.len() / 8;
  let mut prefix = [0xa6u8; 8];
  let mut blocks = plaintext.as_chunks::<8>().0.to_vec();
  for round in 0..6 {
    for index in 1..=n {
      let mut input = [0u8; 16];
      input[..8].copy_from_slice(&prefix);
      input[8..].copy_from_slice(&blocks[index - 1]);
      let encrypted = aes_ecb_block(key, &input, false)?;
      let counter = ((n * round + index) as u64).to_be_bytes();
      for byte in 0..8 {
        prefix[byte] = encrypted[byte] ^ counter[byte];
      }
      blocks[index - 1].copy_from_slice(&encrypted[8..]);
    }
  }
  let mut out = Vec::with_capacity(plaintext.len() + 8);
  out.extend_from_slice(&prefix);
  for block in blocks {
    out.extend_from_slice(&block);
  }
  Ok(out)
}

fn aes_kw_unwrap(key: &[u8], ciphertext: &[u8]) -> Result<Vec<u8>, OpError> {
  if ciphertext.len() < 24 || !ciphertext.len().is_multiple_of(8) {
    return Err(OpError::dom("OperationError", "AES-KW failed"));
  }
  let n = ciphertext.len() / 8 - 1;
  let mut prefix = [0u8; 8];
  prefix.copy_from_slice(&ciphertext[..8]);
  let mut blocks = ciphertext[8..].as_chunks::<8>().0.to_vec();
  for round in (0..6).rev() {
    for index in (1..=n).rev() {
      let counter = ((n * round + index) as u64).to_be_bytes();
      for byte in 0..8 {
        prefix[byte] ^= counter[byte];
      }
      let mut input = [0u8; 16];
      input[..8].copy_from_slice(&prefix);
      input[8..].copy_from_slice(&blocks[index - 1]);
      let decrypted = aes_ecb_block(key, &input, true)?;
      prefix.copy_from_slice(&decrypted[..8]);
      blocks[index - 1].copy_from_slice(&decrypted[8..]);
    }
  }
  if prefix != [0xa6u8; 8] {
    return Err(OpError::dom("OperationError", "AES-KW failed"));
  }
  let mut out = Vec::with_capacity(n * 8);
  for block in blocks {
    out.extend_from_slice(&block);
  }
  Ok(out)
}

fn aes_ecb_block(
  key: &[u8],
  block: &[u8; 16],
  decrypt: bool,
) -> Result<[u8; 16], OpError> {
  match (key.len(), decrypt) {
    (16, false) => encrypt_block::<Aes128>(key, block),
    (24, false) => encrypt_block::<Aes192>(key, block),
    (32, false) => encrypt_block::<Aes256>(key, block),
    (16, true) => decrypt_ecb::<Aes128>(key, block),
    (24, true) => decrypt_ecb::<Aes192>(key, block),
    (32, true) => decrypt_ecb::<Aes256>(key, block),
    _ => Err(OpError::dom(
      "OperationError",
      "The AES key length is invalid",
    )),
  }
}

fn decrypt_ecb<C>(key: &[u8], block: &[u8; 16]) -> Result<[u8; 16], OpError>
where
  C: KeyInit + BlockCipherDecrypt + BlockSizeUser<BlockSize = U16>,
{
  let cipher = C::new_from_slice(key)
    .map_err(|_| OpError::dom("OperationError", "The AES key is invalid"))?;
  Ok(decrypt_block_owned(&cipher, block))
}

fn jwk_value<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  jwk: &PublicJwk,
) -> v8::Local<'s, v8::Value> {
  let object = v8::Object::new(scope);
  set_string(scope, object, "kty", "oct");
  set_string(scope, object, "k", &jwk.k);
  set_string(scope, object, "alg", &jwk.alg);
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

fn set_string(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
  value: &str,
) {
  let key = v8::String::new(scope, name).unwrap();
  let text = v8::String::new(scope, value).unwrap();
  object.set(scope, key.into(), text.into());
}

fn sign_bytes(
  scope: &mut v8::PinScope,
  algorithm: v8::Local<v8::Value>,
  key: v8::Local<v8::Value>,
  data: v8::Local<v8::Value>,
) -> Result<Vec<u8>, OpError> {
  let name = algorithm_name(scope, algorithm)?;
  if name == "HMAC" {
    return mac_or_error(scope, algorithm, key, data, true);
  }
  if is_rsa_signature(&name) {
    return rsa_sign(scope, &name, algorithm, key, data);
  }
  if name == "ECDSA" {
    return ec_sign(scope, algorithm, key, data);
  }
  if name == "Ed25519" || name == "Ed448" {
    return ed25519_sign(scope, key, data);
  }
  Err(OpError::dom(
    "NotSupportedError",
    "The signing algorithm is not supported",
  ))
}

fn verify_bytes(
  scope: &mut v8::PinScope,
  algorithm: v8::Local<v8::Value>,
  key: v8::Local<v8::Value>,
  signature: v8::Local<v8::Value>,
  data: v8::Local<v8::Value>,
) -> Result<bool, OpError> {
  let name = algorithm_name(scope, algorithm)?;
  if name == "HMAC" {
    return verify_mac(scope, algorithm, key, signature, data);
  }
  if is_rsa_signature(&name) {
    return rsa_verify(scope, &name, algorithm, key, signature, data);
  }
  if name == "ECDSA" {
    return ec_verify(scope, algorithm, key, signature, data);
  }
  if name == "Ed25519" || name == "Ed448" {
    return ed25519_verify(scope, key, signature, data);
  }
  Err(OpError::dom(
    "NotSupportedError",
    "The signing algorithm is not supported",
  ))
}

fn crypt_bytes(
  scope: &mut v8::PinScope,
  algorithm: v8::Local<v8::Value>,
  key: v8::Local<v8::Value>,
  data: v8::Local<v8::Value>,
  encrypting: bool,
) -> Result<Vec<u8>, OpError> {
  let name = algorithm_name(scope, algorithm)?;
  if is_aes(&name) {
    return aes_gcm(scope, algorithm, key, data, encrypting);
  }
  if name == "RSA-OAEP" {
    let usage = if encrypting { "encrypt" } else { "decrypt" };
    let (key, label) = rsa_oaep_key(scope, algorithm, key, usage)?;
    let data = buffer(scope, data)?;
    return oaep_run(&key, &label, &data, encrypting);
  }
  Err(OpError::dom(
    "NotSupportedError",
    "The encryption algorithm is not supported",
  ))
}

fn mac_or_error(
  scope: &mut v8::PinScope,
  algorithm: v8::Local<v8::Value>,
  key: v8::Local<v8::Value>,
  data: v8::Local<v8::Value>,
  signing: bool,
) -> Result<Vec<u8>, OpError> {
  let name = algorithm_name(scope, algorithm)?;
  if name != "HMAC" {
    return Err(OpError::dom(
      "NotSupportedError",
      "The signing algorithm is not supported",
    ));
  }
  let key = load_key(scope, key)?;
  let usage = if signing { "sign" } else { "verify" };
  if key.algorithm != "HMAC" || !key.usages.iter().any(|item| item == usage) {
    return Err(OpError::dom(
      "InvalidAccessError",
      "The key usage is invalid",
    ));
  }
  let hash = key.hash.unwrap_or_else(|| "SHA-256".to_string());
  let data = buffer(scope, data)?;
  hmac_bytes(&hash, &key.bytes, &data)
}

fn verify_mac(
  scope: &mut v8::PinScope,
  algorithm: v8::Local<v8::Value>,
  key: v8::Local<v8::Value>,
  signature: v8::Local<v8::Value>,
  data: v8::Local<v8::Value>,
) -> Result<bool, OpError> {
  let expected = mac_or_error(scope, algorithm, key, data, false)?;
  let signature = buffer(scope, signature)?;
  Ok(
    expected.len() == signature.len()
      && expected
        .iter()
        .zip(signature.iter())
        .fold(0u8, |acc, (left, right)| acc | (left ^ right))
        == 0,
  )
}

fn aes_gcm(
  scope: &mut v8::PinScope,
  algorithm: v8::Local<v8::Value>,
  key: v8::Local<v8::Value>,
  data: v8::Local<v8::Value>,
  encrypting: bool,
) -> Result<Vec<u8>, OpError> {
  let name = algorithm_name(scope, algorithm)?;
  if !is_aes(&name) {
    return Err(OpError::dom(
      "NotSupportedError",
      "The encryption algorithm is not supported",
    ));
  }
  let key = load_key(scope, key)?;
  if key.algorithm != name {
    return Err(OpError::dom(
      "InvalidAccessError",
      "The key algorithm does not match",
    ));
  }
  let usage = if encrypting { "encrypt" } else { "decrypt" };
  if !key.usages.iter().any(|item| item == usage) {
    return Err(OpError::dom(
      "InvalidAccessError",
      "The key usage is invalid",
    ));
  }
  let data = buffer(scope, data)?;
  match name.as_str() {
    "AES-GCM" => {
      let tag_bits = match algorithm_number(scope, algorithm, "tagLength") {
        None => 128,
        Some(tag) => {
          let tag = enforce_octet(tag, "tagLength")?;
          if !matches!(tag, 32 | 64 | 96 | 104 | 112 | 120 | 128) {
            return Err(OpError::dom(
              "OperationError",
              "The AES-GCM tag length is invalid",
            ));
          }
          tag
        }
      };
      let iv = algorithm_bytes(scope, algorithm, "iv")?;
      let additional =
        optional_algorithm_bytes(scope, algorithm, "additionalData")?;
      gcm_crypt(&key.bytes, &iv, &additional, &data, encrypting, tag_bits)
    }
    "AES-CBC" => {
      let iv = algorithm_bytes(scope, algorithm, "iv")?;
      cbc_crypt(&key.bytes, &iv, &data, encrypting)
    }
    "AES-CTR" => {
      let counter = algorithm_bytes(scope, algorithm, "counter")?;
      let Some(length) = algorithm_number(scope, algorithm, "length") else {
        return Err(OpError::type_("The AES-CTR length is required"));
      };
      let length = enforce_octet(length, "length")?;
      if length == 0 || length > 128 {
        return Err(OpError::dom(
          "OperationError",
          "The AES-CTR counter length is invalid",
        ));
      }
      ctr_crypt(&key.bytes, &counter, length, &data)
    }
    _ => unreachable!(),
  }
}

macro_rules! gcm_tags {
  ($aes:ty, $nonce:ty, $tag_bits:expr, $key:expr, $iv:expr, $payload:expr, $enc:expr) => {{
    match $tag_bits {
      32 => gcm_with::<AesGcm<$aes, $nonce, aes_gcm::aead::consts::U4>>(
        $key, $iv, $payload, $enc,
      ),
      64 => gcm_with::<AesGcm<$aes, $nonce, aes_gcm::aead::consts::U8>>(
        $key, $iv, $payload, $enc,
      ),
      96 => gcm_with::<AesGcm<$aes, $nonce, aes_gcm::aead::consts::U12>>(
        $key, $iv, $payload, $enc,
      ),
      104 => gcm_with::<AesGcm<$aes, $nonce, aes_gcm::aead::consts::U13>>(
        $key, $iv, $payload, $enc,
      ),
      112 => gcm_with::<AesGcm<$aes, $nonce, aes_gcm::aead::consts::U14>>(
        $key, $iv, $payload, $enc,
      ),
      120 => gcm_with::<AesGcm<$aes, $nonce, aes_gcm::aead::consts::U15>>(
        $key, $iv, $payload, $enc,
      ),
      128 => gcm_with::<AesGcm<$aes, $nonce, aes_gcm::aead::consts::U16>>(
        $key, $iv, $payload, $enc,
      ),
      _ => Err(OpError::dom(
        "OperationError",
        "The AES-GCM tag length is invalid",
      )),
    }
  }};
}

fn gcm_crypt(
  key: &[u8],
  iv: &[u8],
  additional: &[u8],
  data: &[u8],
  encrypting: bool,
  tag_bits: u32,
) -> Result<Vec<u8>, OpError> {
  let payload = Payload {
    msg: data,
    aad: additional,
  };
  let result = match (key.len(), iv.len()) {
    (16, 12) => {
      gcm_tags!(
        Aes128,
        aes_gcm::aead::consts::U12,
        tag_bits,
        key,
        iv,
        payload,
        encrypting
      )
    }
    (24, 12) => {
      gcm_tags!(
        Aes192,
        aes_gcm::aead::consts::U12,
        tag_bits,
        key,
        iv,
        payload,
        encrypting
      )
    }
    (32, 12) => {
      gcm_tags!(
        Aes256,
        aes_gcm::aead::consts::U12,
        tag_bits,
        key,
        iv,
        payload,
        encrypting
      )
    }
    (16, 16) => {
      gcm_tags!(
        Aes128,
        aes_gcm::aead::consts::U16,
        tag_bits,
        key,
        iv,
        payload,
        encrypting
      )
    }
    (24, 16) => {
      gcm_tags!(
        Aes192,
        aes_gcm::aead::consts::U16,
        tag_bits,
        key,
        iv,
        payload,
        encrypting
      )
    }
    (32, 16) => {
      gcm_tags!(
        Aes256,
        aes_gcm::aead::consts::U16,
        tag_bits,
        key,
        iv,
        payload,
        encrypting
      )
    }
    (16, 32) => {
      gcm_tags!(
        Aes128,
        aes_gcm::aead::consts::U32,
        tag_bits,
        key,
        iv,
        payload,
        encrypting
      )
    }
    (24, 32) => {
      gcm_tags!(
        Aes192,
        aes_gcm::aead::consts::U32,
        tag_bits,
        key,
        iv,
        payload,
        encrypting
      )
    }
    (32, 32) => {
      gcm_tags!(
        Aes256,
        aes_gcm::aead::consts::U32,
        tag_bits,
        key,
        iv,
        payload,
        encrypting
      )
    }
    (_, 12 | 16 | 32) => Err(OpError::dom(
      "OperationError",
      "The AES key length is invalid",
    )),
    _ => Err(OpError::dom(
      "OperationError",
      "The AES-GCM iv length is not supported",
    )),
  };
  result.and_then(|result| {
    result.map_err(|_| OpError::dom("OperationError", "AES-GCM failed"))
  })
}

fn gcm_with<C>(
  key: &[u8],
  iv: &[u8],
  payload: Payload<'_, '_>,
  encrypting: bool,
) -> Result<Result<Vec<u8>, aes_gcm::Error>, OpError>
where
  C: KeyInit + Aead,
{
  let cipher = C::new_from_slice(key)
    .map_err(|_| OpError::dom("OperationError", "The AES key is invalid"))?;
  let nonce = aes_gcm::Nonce::<C::NonceSize>::try_from(iv).map_err(|_| {
    OpError::dom("OperationError", "The AES-GCM iv length is not supported")
  })?;
  Ok(if encrypting {
    cipher.encrypt(&nonce, payload)
  } else {
    cipher.decrypt(&nonce, payload)
  })
}

fn cbc_crypt(
  key: &[u8],
  iv: &[u8],
  data: &[u8],
  encrypting: bool,
) -> Result<Vec<u8>, OpError> {
  if iv.len() != 16 {
    return Err(OpError::dom(
      "OperationError",
      "The AES-CBC iv must be 16 bytes",
    ));
  }
  match key.len() {
    16 => cbc_with::<Aes128>(key, iv, data, encrypting),
    24 => cbc_with::<Aes192>(key, iv, data, encrypting),
    32 => cbc_with::<Aes256>(key, iv, data, encrypting),
    _ => Err(OpError::dom(
      "OperationError",
      "The AES key length is invalid",
    )),
  }
}

fn cbc_with<C>(
  key: &[u8],
  iv: &[u8],
  data: &[u8],
  encrypting: bool,
) -> Result<Vec<u8>, OpError>
where
  C: KeyInit
    + BlockCipherEncrypt
    + BlockCipherDecrypt
    + BlockSizeUser<BlockSize = U16>,
{
  let cipher = C::new_from_slice(key)
    .map_err(|_| OpError::dom("OperationError", "The AES key is invalid"))?;
  if encrypting {
    Ok(cbc_encrypt(&cipher, iv, data))
  } else {
    cbc_decrypt(&cipher, iv, data)
  }
}

fn cbc_encrypt<C: BlockCipherEncrypt + BlockSizeUser<BlockSize = U16>>(
  cipher: &C,
  iv: &[u8],
  data: &[u8],
) -> Vec<u8> {
  let mut previous = [0u8; 16];
  previous.copy_from_slice(iv);
  let pad = 16 - (data.len() % 16);
  let mut padded = Vec::with_capacity(data.len() + pad);
  padded.extend_from_slice(data);
  padded.extend(std::iter::repeat_n(pad as u8, pad));
  let mut out = Vec::with_capacity(padded.len());
  for chunk in padded.as_chunks::<16>().0 {
    let mut block = [0u8; 16];
    for index in 0..16 {
      block[index] = chunk[index] ^ previous[index];
    }
    let encrypted = encrypt_block_owned(cipher, &block);
    previous = encrypted;
    out.extend_from_slice(&encrypted);
  }
  out
}

fn cbc_decrypt<C: BlockCipherDecrypt + BlockSizeUser<BlockSize = U16>>(
  cipher: &C,
  iv: &[u8],
  data: &[u8],
) -> Result<Vec<u8>, OpError> {
  if data.is_empty() || !data.len().is_multiple_of(16) {
    return Err(OpError::dom("OperationError", "AES-CBC failed"));
  }
  let mut previous = [0u8; 16];
  previous.copy_from_slice(iv);
  let mut out = Vec::with_capacity(data.len());
  for chunk in data.as_chunks::<16>().0 {
    let mut chunk_bytes = [0u8; 16];
    chunk_bytes.copy_from_slice(chunk);
    let decrypted = decrypt_block_owned(cipher, &chunk_bytes);
    let mut plain = [0u8; 16];
    for index in 0..16 {
      plain[index] = decrypted[index] ^ previous[index];
    }
    previous = chunk_bytes;
    out.extend_from_slice(&plain);
  }
  let pad = usize::from(*out.last().unwrap_or(&0));
  if pad == 0 || pad > 16 || out.len() < pad {
    return Err(OpError::dom("OperationError", "AES-CBC failed"));
  }
  if out[out.len() - pad..]
    .iter()
    .any(|byte| usize::from(*byte) != pad)
  {
    return Err(OpError::dom("OperationError", "AES-CBC failed"));
  }
  out.truncate(out.len() - pad);
  Ok(out)
}

fn ctr_crypt(
  key: &[u8],
  counter: &[u8],
  bits: u32,
  data: &[u8],
) -> Result<Vec<u8>, OpError> {
  if counter.len() != 16 {
    return Err(OpError::dom(
      "OperationError",
      "The AES-CTR counter must be 16 bytes",
    ));
  }
  let mut counter: [u8; 16] = counter.try_into().expect("16 bytes");
  let mut out = vec![0u8; data.len()];
  let mut offset = 0;
  while offset < data.len() {
    let block = match key.len() {
      16 => encrypt_block::<Aes128>(key, &counter)?,
      24 => encrypt_block::<Aes192>(key, &counter)?,
      32 => encrypt_block::<Aes256>(key, &counter)?,
      _ => {
        return Err(OpError::dom(
          "OperationError",
          "The AES key length is invalid",
        ));
      }
    };
    let width = (data.len() - offset).min(16);
    for index in 0..width {
      out[offset + index] = data[offset + index] ^ block[index];
    }
    increment_counter(&mut counter, bits as usize);
    offset += width;
  }
  Ok(out)
}

fn encrypt_block<C>(key: &[u8], block: &[u8; 16]) -> Result<[u8; 16], OpError>
where
  C: KeyInit + BlockCipherEncrypt + BlockSizeUser<BlockSize = U16>,
{
  let cipher = C::new_from_slice(key)
    .map_err(|_| OpError::dom("OperationError", "The AES key is invalid"))?;
  Ok(encrypt_block_owned(&cipher, block))
}

fn encrypt_block_owned<C>(cipher: &C, block: &[u8; 16]) -> [u8; 16]
where
  C: BlockCipherEncrypt + BlockSizeUser<BlockSize = U16>,
{
  let mut out = aes_gcm::aes::cipher::array::Array::<u8, U16>::from(*block);
  cipher.encrypt_block(&mut out);
  let mut bytes = [0u8; 16];
  bytes.copy_from_slice(&out);
  bytes
}

fn decrypt_block_owned<C>(cipher: &C, block: &[u8; 16]) -> [u8; 16]
where
  C: BlockCipherDecrypt + BlockSizeUser<BlockSize = U16>,
{
  let mut out = aes_gcm::aes::cipher::array::Array::<u8, U16>::from(*block);
  cipher.decrypt_block(&mut out);
  let mut bytes = [0u8; 16];
  bytes.copy_from_slice(&out);
  bytes
}

fn increment_counter(counter: &mut [u8; 16], bits: usize) {
  let mut carry = 1u16;
  let mut consumed = 0usize;
  for index in (0..16).rev() {
    if consumed >= bits || carry == 0 {
      break;
    }
    let width = (bits - consumed).min(8);
    let mask = if width == 8 {
      0xffu16
    } else {
      (1u16 << width) - 1
    };
    let value = u16::from(counter[index]) & mask;
    let sum = value + carry;
    let low = (sum as u8) & (mask as u8);
    let high = counter[index] & !(mask as u8);
    counter[index] = high | low;
    carry = sum >> width;
    consumed += width;
  }
}

/// WebIDL `[EnforceRange] unsigned long`. Values outside `0` to `2^32 - 1`,
/// and non-finite numbers, are `TypeError` before any buffer is allocated.
fn enforce_unsigned_long(number: f64, what: &str) -> Result<u32, OpError> {
  let integer = if number.is_finite() {
    number.trunc()
  } else {
    f64::NAN
  };
  if !integer.is_finite() || integer < 0.0 || integer > 4_294_967_295.0 {
    return Err(OpError::type_(format!("The {what} is out of range")));
  }
  Ok(integer as u32)
}

fn derive_length(
  scope: &mut v8::PinScope,
  length: v8::Local<v8::Value>,
) -> Result<Option<u32>, OpError> {
  if length.is_null() || length.is_undefined() {
    return Ok(None);
  }
  let number = length
    .number_value(scope)
    .ok_or_else(|| OpError::type_("The length is out of range"))?;
  Ok(Some(enforce_unsigned_long(number, "length")?))
}

fn derive_bits(
  scope: &mut v8::PinScope,
  algorithm: v8::Local<v8::Value>,
  key: v8::Local<v8::Value>,
  length: v8::Local<v8::Value>,
) -> Result<Vec<u8>, OpError> {
  let bits = derive_length(scope, length)?;
  derive_bits_len(scope, algorithm, key, bits, "deriveBits")
}

fn derive_bits_len(
  scope: &mut v8::PinScope,
  algorithm: v8::Local<v8::Value>,
  key: v8::Local<v8::Value>,
  bits: Option<u32>,
  required_usage: &str,
) -> Result<Vec<u8>, OpError> {
  let name = algorithm_name(scope, algorithm)?;
  let key = load_key(scope, key)?;
  // An unrecognized name is NotSupportedError. Comparing it with the key
  // first would report InvalidAccessError for "HKDF" (U+212A is not ASCII K).
  if !is_derivation_algorithm(&name) {
    return Err(OpError::dom(
      "NotSupportedError",
      "The derivation algorithm is not supported",
    ));
  }
  if key.algorithm != name {
    return Err(OpError::dom(
      "InvalidAccessError",
      "The key algorithm does not match",
    ));
  }
  if !key.usages.iter().any(|item| item == required_usage) {
    return Err(OpError::dom(
      "InvalidAccessError",
      "The key usage is invalid",
    ));
  }
  if name == "ECDH" {
    return derive_ecdh(scope, algorithm, &key, bits);
  }
  if name == "X25519" || name == "X448" {
    return derive_x25519(scope, algorithm, &key, bits);
  }
  let Some(bits) = bits else {
    return Err(OpError::dom("OperationError", "The length is null"));
  };
  if !bits.is_multiple_of(8) {
    return Err(OpError::dom(
      "OperationError",
      "The length is not a multiple of 8",
    ));
  }
  if bits == 0 {
    return Ok(Vec::new());
  }
  let mut output = vec![0u8; (bits / 8) as usize];
  match name.as_str() {
    "PBKDF2" => {
      let hash = algorithm_hash(scope, algorithm)?
        .unwrap_or_else(|| "SHA-256".to_string());
      let salt = algorithm_bytes(scope, algorithm, "salt")?;
      let iterations =
        algorithm_number(scope, algorithm, "iterations").unwrap_or(1.0);
      let iterations = enforce_unsigned_long(iterations, "iteration count")?;
      if iterations == 0 {
        return Err(OpError::dom(
          "OperationError",
          "PBKDF2 requires a non-zero iteration count",
        ));
      }
      pbkdf2_fill(&hash, &key.bytes, &salt, iterations, &mut output)?;
    }
    "HKDF" => {
      let hash = algorithm_hash(scope, algorithm)?
        .unwrap_or_else(|| "SHA-256".to_string());
      let salt = optional_algorithm_bytes(scope, algorithm, "salt")?;
      let info = optional_algorithm_bytes(scope, algorithm, "info")?;
      hkdf_fill(&hash, &key.bytes, &salt, &info, &mut output)?;
    }
    _ => {
      return Err(OpError::dom(
        "NotSupportedError",
        "The derivation algorithm is not supported",
      ));
    }
  }
  Ok(output)
}

fn derive_ecdh(
  scope: &mut v8::PinScope,
  algorithm: v8::Local<v8::Value>,
  key: &KeyView,
  bits: Option<u32>,
) -> Result<Vec<u8>, OpError> {
  let peer = member_key(scope, algorithm, "public")?;
  if peer.algorithm != "ECDH"
    || peer.kind != "public"
    || peer.curve != key.curve
  {
    return Err(OpError::dom(
      "InvalidAccessError",
      "The key algorithm does not match",
    ));
  }
  let curve = key.curve.as_deref().unwrap_or("");
  let secret = crate::crypto::ec_key::agree(curve, &key.bytes, &peer.bytes)
    .map_err(ec_error)?;
  // A missing length is the whole shared secret. HKDF and PBKDF2 reject it.
  take_leading_bits(secret, bits)
}

fn derive_x25519(
  scope: &mut v8::PinScope,
  algorithm: v8::Local<v8::Value>,
  key: &KeyView,
  bits: Option<u32>,
) -> Result<Vec<u8>, OpError> {
  let peer = member_key(scope, algorithm, "public")?;
  if peer.algorithm != key.algorithm || peer.kind != "public" {
    return Err(OpError::dom(
      "InvalidAccessError",
      "The key algorithm does not match",
    ));
  }
  let secret = if key.algorithm == "X448" {
    crate::crypto::okp_key::agree_x448(&key.bytes, &peer.bytes)
  } else {
    crate::crypto::okp_key::agree(&key.bytes, &peer.bytes)
  }
  .map_err(okp_error)?;
  take_leading_bits(secret, bits)
}

fn take_leading_bits(
  secret: Vec<u8>,
  bits: Option<u32>,
) -> Result<Vec<u8>, OpError> {
  let Some(bits) = bits else {
    return Ok(secret);
  };
  if bits == 0 {
    return Ok(Vec::new());
  }
  let length = bits.div_ceil(8) as usize;
  if length > secret.len() {
    return Err(OpError::dom("OperationError", "The length is too large"));
  }
  let mut output = secret[..length].to_vec();
  let extra = bits % 8;
  if extra != 0 {
    let last = output.len() - 1;
    output[last] &= 0xff << (8 - extra);
  }
  Ok(output)
}

fn derive_key(
  scope: &mut v8::PinScope,
  algorithm: v8::Local<v8::Value>,
  base: v8::Local<v8::Value>,
  derived: v8::Local<v8::Value>,
  extractable: bool,
  usages: v8::Local<v8::Value>,
) -> Result<CryptoKey, OpError> {
  let name = algorithm_name(scope, derived)?;
  let usages = normalize_usages(read_usages(scope, usages)?);
  if name != "HMAC" && !is_aes_key(&name) {
    return Err(OpError::dom(
      "NotSupportedError",
      "The derived key algorithm is not supported",
    ));
  }
  let (hash, bit_length) = if name == "HMAC" {
    let hash =
      algorithm_hash(scope, derived)?.unwrap_or_else(|| "SHA-256".to_string());
    (
      Some(hash.clone()),
      hmac_generate_length(scope, derived, &hash)?,
    )
  } else {
    (None, aes_generate_length(scope, derived)?)
  };
  let bytes =
    derive_bits_len(scope, algorithm, base, Some(bit_length), "deriveKey")?;
  // Usages are checked when the derived bits are imported.
  check_usages(&name, &usages)?;
  Ok(secret_key(
    name,
    extractable,
    hash,
    bit_length,
    usages,
    bytes,
  ))
}

fn pbkdf2_fill(
  hash: &str,
  password: &[u8],
  salt: &[u8],
  iterations: u32,
  output: &mut [u8],
) -> Result<(), OpError> {
  match hash {
    "SHA-1" => pbkdf2::pbkdf2_hmac::<Sha1>(password, salt, iterations, output),
    "SHA-256" => {
      pbkdf2::pbkdf2_hmac::<Sha256>(password, salt, iterations, output)
    }
    "SHA-384" => {
      pbkdf2::pbkdf2_hmac::<Sha384>(password, salt, iterations, output)
    }
    "SHA-512" => {
      pbkdf2::pbkdf2_hmac::<Sha512>(password, salt, iterations, output)
    }
    _ => {
      return Err(OpError::dom(
        "NotSupportedError",
        "The PBKDF2 hash is not supported",
      ));
    }
  }
  Ok(())
}

fn hkdf_fill(
  hash: &str,
  ikm: &[u8],
  salt: &[u8],
  info: &[u8],
  output: &mut [u8],
) -> Result<(), OpError> {
  let salt = if salt.is_empty() { None } else { Some(salt) };
  let result = match hash {
    "SHA-1" => hkdf::Hkdf::<Sha1>::new(salt, ikm).expand(info, output),
    "SHA-256" => hkdf::Hkdf::<Sha256>::new(salt, ikm).expand(info, output),
    "SHA-384" => hkdf::Hkdf::<Sha384>::new(salt, ikm).expand(info, output),
    "SHA-512" => hkdf::Hkdf::<Sha512>::new(salt, ikm).expand(info, output),
    _ => {
      return Err(OpError::dom(
        "NotSupportedError",
        "The HKDF hash is not supported",
      ));
    }
  };
  result.map_err(|_| OpError::dom("OperationError", "HKDF failed"))
}

fn hmac_bytes(hash: &str, key: &[u8], data: &[u8]) -> Result<Vec<u8>, OpError> {
  match hash {
    "SHA-1" => {
      let mut mac = hmac::Hmac::<Sha1>::new_from_slice(key).map_err(|_| {
        OpError::dom("OperationError", "The HMAC key is invalid")
      })?;
      mac.update(data);
      Ok(mac.finalize().into_bytes().to_vec())
    }
    "SHA-256" => {
      let mut mac =
        hmac::Hmac::<Sha256>::new_from_slice(key).map_err(|_| {
          OpError::dom("OperationError", "The HMAC key is invalid")
        })?;
      mac.update(data);
      Ok(mac.finalize().into_bytes().to_vec())
    }
    "SHA-384" => {
      let mut mac =
        hmac::Hmac::<Sha384>::new_from_slice(key).map_err(|_| {
          OpError::dom("OperationError", "The HMAC key is invalid")
        })?;
      mac.update(data);
      Ok(mac.finalize().into_bytes().to_vec())
    }
    "SHA-512" => {
      let mut mac =
        hmac::Hmac::<Sha512>::new_from_slice(key).map_err(|_| {
          OpError::dom("OperationError", "The HMAC key is invalid")
        })?;
      mac.update(data);
      Ok(mac.finalize().into_bytes().to_vec())
    }
    _ => Err(OpError::dom(
      "NotSupportedError",
      "The HMAC hash is not supported",
    )),
  }
}

enum Generated {
  Secret(Box<CryptoKey>),
  Pair {
    public: Box<CryptoKey>,
    private: Box<CryptoKey>,
  },
}

fn named_curve(
  scope: &mut v8::PinScope,
  algorithm: v8::Local<v8::Value>,
) -> Result<String, OpError> {
  let object = v8::Local::<v8::Object>::try_from(algorithm)
    .map_err(|_| OpError::type_("The named curve is required"))?;
  let key = v8::String::new(scope, "namedCurve").unwrap();
  let Some(value) = object.get(scope, key.into()) else {
    return Err(OpError::type_("The named curve is required"));
  };
  if value.is_undefined() {
    return Err(OpError::type_("The named curve is required"));
  }
  let curve = canonical_algorithm(&string_value(scope, value)?);
  match curve.as_str() {
    "P-256" | "P-384" | "P-521" => Ok(curve),
    _ => Err(OpError::dom(
      "NotSupportedError",
      "The named curve is not supported",
    )),
  }
}

fn resolve_pair(
  scope: &mut v8::PinScope,
  public: CryptoKey,
  private: CryptoKey,
) -> Js {
  let object = v8::Object::new(scope);
  let public_key = unsafe { yuzuko_core::op::wrap_instance(scope, public) };
  let private_key = unsafe { yuzuko_core::op::wrap_instance(scope, private) };
  let public_name = v8::String::new(scope, "publicKey").unwrap();
  let private_name = v8::String::new(scope, "privateKey").unwrap();
  object.set(scope, public_name.into(), public_key.into());
  object.set(scope, private_name.into(), private_key.into());
  resolve(scope, object.into())
}

fn secret_key(
  algorithm: impl Into<String>,
  extractable: bool,
  hash: Option<String>,
  length: u32,
  usages: Vec<String>,
  bytes: Vec<u8>,
) -> CryptoKey {
  CryptoKey {
    kind: "secret".to_string(),
    extractable,
    algorithm: algorithm.into(),
    hash,
    curve: None,
    length,
    modulus_length: 0,
    public_exponent: Vec::new(),
    usages,
    material: Material::Bytes(bytes),
    algorithm_object: RefCell::new(None),
    usages_object: RefCell::new(None),
  }
}

fn hmac_block_bits(hash: &str) -> u32 {
  match hash {
    "SHA-384" | "SHA-512" => 1024,
    _ => 512,
  }
}

fn hmac_generate_length(
  scope: &mut v8::PinScope,
  algorithm: v8::Local<v8::Value>,
  hash: &str,
) -> Result<u32, OpError> {
  let Some(number) = algorithm_number(scope, algorithm, "length") else {
    return Ok(hmac_block_bits(hash));
  };
  let length = enforce_unsigned_long(number, "length")?;
  if length == 0 || length % 8 != 0 {
    return Err(OpError::dom("OperationError", "The HMAC length is invalid"));
  }
  Ok(length)
}

fn aes_generate_length(
  scope: &mut v8::PinScope,
  algorithm: v8::Local<v8::Value>,
) -> Result<u32, OpError> {
  let Some(number) = algorithm_number(scope, algorithm, "length") else {
    return Ok(256);
  };
  let length = enforce_unsigned_long(number, "length")?;
  if matches!(length, 128 | 192 | 256) {
    Ok(length)
  } else {
    Err(OpError::dom(
      "OperationError",
      "The AES key length is invalid",
    ))
  }
}

fn random_bytes(bits: u32) -> Result<Vec<u8>, OpError> {
  let mut bytes = vec![0u8; (bits as usize) / 8];
  getrandom::fill(&mut bytes)
    .map_err(|_| OpError::dom("OperationError", "Random generation failed"))?;
  Ok(bytes)
}

fn enforce_octet(number: f64, what: &str) -> Result<u32, OpError> {
  let length = enforce_unsigned_long(number, what)?;
  if length > 255 {
    return Err(OpError::type_(format!("The {what} is out of range")));
  }
  Ok(length)
}

fn reject_unknown_usages(
  algorithm: &str,
  usages: &[String],
) -> Result<(), OpError> {
  let Some(allowed) = allowed_usages(algorithm) else {
    return Ok(());
  };
  if usages
    .iter()
    .any(|usage| !allowed.contains(&usage.as_str()))
  {
    return Err(OpError::dom("SyntaxError", "The key usage is invalid"));
  }
  Ok(())
}

fn reject_empty_usages(
  algorithm: &str,
  usages: &[String],
) -> Result<(), OpError> {
  if allowed_usages(algorithm).is_some() && usages.is_empty() {
    return Err(OpError::dom("SyntaxError", "The key usage is invalid"));
  }
  Ok(())
}

fn allowed_usages(algorithm: &str) -> Option<&'static [&'static str]> {
  Some(match algorithm {
    "HMAC" => &["sign", "verify"],
    "AES-GCM" | "AES-CBC" | "AES-CTR" => {
      &["encrypt", "decrypt", "wrapKey", "unwrapKey"]
    }
    "AES-KW" => &["wrapKey", "unwrapKey"],
    "ECDH" | "X25519" | "X448" => &["deriveKey", "deriveBits"],
    "ECDSA" | "Ed25519" | "Ed448" => &["sign", "verify"],
    "HKDF" | "PBKDF2" => &["deriveKey", "deriveBits"],
    "RSASSA-PKCS1-v1_5" | "RSA-PSS" => &["sign", "verify"],
    "RSA-OAEP" => &["encrypt", "decrypt", "wrapKey", "unwrapKey"],
    _ => return None,
  })
}

struct KeyView {
  kind: String,
  algorithm: String,
  hash: Option<String>,
  curve: Option<String>,
  length: u32,
  usages: Vec<String>,
  bytes: Vec<u8>,
  extractable: bool,
}

fn load_key(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<KeyView, OpError> {
  let object = v8::Local::<v8::Object>::try_from(value)
    .map_err(|_| OpError::type_("The value is not a CryptoKey"))?;
  let host = unsafe { yuzuko_core::op::host_ref::<CryptoKey>(scope, object) }
    .ok_or_else(|| OpError::type_("The value is not a CryptoKey"))?;
  let key = host.get();
  let Material::Bytes(bytes) = &key.material;
  Ok(KeyView {
    kind: key.kind.clone(),
    algorithm: key.algorithm.clone(),
    hash: key.hash.clone(),
    curve: key.curve.clone(),
    length: key.length,
    usages: key.usages.clone(),
    bytes: bytes.clone(),
    extractable: key.extractable,
  })
}

fn normalize_usages(usages: Vec<String>) -> Vec<String> {
  let mut unique = Vec::new();
  for usage in usages {
    if !unique.contains(&usage) {
      unique.push(usage);
    }
  }
  unique
}

fn check_usages(algorithm: &str, usages: &[String]) -> Result<(), OpError> {
  reject_unknown_usages(algorithm, usages)?;
  reject_empty_usages(algorithm, usages)
}

fn optional_string(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
) -> Result<Option<String>, OpError> {
  let key = v8::String::new(scope, name).unwrap();
  let Some(value) = object.get(scope, key.into()) else {
    return Ok(None);
  };
  if value.is_undefined() {
    return Ok(None);
  }
  Ok(Some(string_value(scope, value)?))
}

fn optional_bool(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
) -> Result<Option<bool>, OpError> {
  let key = v8::String::new(scope, name).unwrap();
  let Some(value) = object.get(scope, key.into()) else {
    return Ok(None);
  };
  if value.is_undefined() {
    return Ok(None);
  }
  Ok(Some(value.boolean_value(scope)))
}

fn optional_strings(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
) -> Result<Option<Vec<String>>, OpError> {
  let key = v8::String::new(scope, name).unwrap();
  let Some(value) = object.get(scope, key.into()) else {
    return Ok(None);
  };
  if value.is_undefined() {
    return Ok(None);
  }
  if !value.is_array() {
    return Err(OpError::type_("The JWK key_ops are not a sequence"));
  }
  let list = v8::Local::<v8::Object>::try_from(value)
    .map_err(|_| OpError::type_("The JWK key_ops are not a sequence"))?;
  let length = list
    .get(scope, v8::String::new(scope, "length").unwrap().into())
    .and_then(|value| value.uint32_value(scope))
    .unwrap_or(0);
  let mut values = Vec::new();
  for index in 0..length {
    let item = list
      .get_index(scope, index)
      .ok_or_else(|| OpError::type_("The JWK key_ops are not a sequence"))?;
    values.push(string_value(scope, item)?);
  }
  Ok(Some(values))
}

fn algorithm_name(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<String, OpError> {
  let name = if value.is_object() && !value.is_string() {
    let object = v8::Local::<v8::Object>::try_from(value)
      .map_err(|_| OpError::type_("The algorithm is invalid"))?;
    let key = v8::String::new(scope, "name").unwrap();
    let name = object
      .get(scope, key.into())
      .ok_or_else(|| OpError::type_("The algorithm is invalid"))?;
    if name.is_null() || name.is_undefined() {
      return Err(OpError::type_("The algorithm name is required"));
    }
    string_value(scope, name)?
  } else {
    string_value(scope, value)?
  };
  Ok(canonical_algorithm(&name))
}

fn canonical_algorithm(name: &str) -> String {
  match name.to_ascii_lowercase().as_str() {
    "hmac" => "HMAC".to_string(),
    "aes-gcm" => "AES-GCM".to_string(),
    "aes-cbc" => "AES-CBC".to_string(),
    "aes-ctr" => "AES-CTR".to_string(),
    "aes-kw" => "AES-KW".to_string(),
    "ecdh" => "ECDH".to_string(),
    "ecdsa" => "ECDSA".to_string(),
    "p-256" => "P-256".to_string(),
    "p-384" => "P-384".to_string(),
    "p-521" => "P-521".to_string(),
    "ed25519" => "Ed25519".to_string(),
    "ed448" => "Ed448".to_string(),
    "x25519" => "X25519".to_string(),
    "x448" => "X448".to_string(),
    "hkdf" => "HKDF".to_string(),
    "pbkdf2" => "PBKDF2".to_string(),
    "sha-1" => "SHA-1".to_string(),
    "sha-256" => "SHA-256".to_string(),
    "sha-384" => "SHA-384".to_string(),
    "sha-512" => "SHA-512".to_string(),
    "rsassa-pkcs1-v1_5" => "RSASSA-PKCS1-v1_5".to_string(),
    "rsa-pss" => "RSA-PSS".to_string(),
    "rsa-oaep" => "RSA-OAEP".to_string(),
    _ => name.to_string(),
  }
}

fn algorithm_hash(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<Option<String>, OpError> {
  let Ok(object) = v8::Local::<v8::Object>::try_from(value) else {
    return Ok(None);
  };
  let key = v8::String::new(scope, "hash").unwrap();
  let Some(hash) = object.get(scope, key.into()) else {
    return Ok(None);
  };
  if hash.is_undefined() {
    return Ok(None);
  }
  Ok(Some(algorithm_name(scope, hash)?))
}

fn algorithm_number(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
  name: &str,
) -> Option<f64> {
  let object = v8::Local::<v8::Object>::try_from(value).ok()?;
  let key = v8::String::new(scope, name)?;
  let value = object.get(scope, key.into())?;
  if value.is_undefined() {
    return None;
  }
  value.number_value(scope)
}

fn optional_algorithm_bytes(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
  name: &str,
) -> Result<Vec<u8>, OpError> {
  let Ok(object) = v8::Local::<v8::Object>::try_from(value) else {
    return Ok(Vec::new());
  };
  let key = v8::String::new(scope, name).unwrap();
  let Some(member) = object.get(scope, key.into()) else {
    return Ok(Vec::new());
  };
  if member.is_undefined() {
    return Ok(Vec::new());
  }
  buffer(scope, member)
}

fn algorithm_bytes(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
  name: &str,
) -> Result<Vec<u8>, OpError> {
  let object = v8::Local::<v8::Object>::try_from(value)
    .map_err(|_| OpError::type_("The algorithm is invalid"))?;
  let key = v8::String::new(scope, name).unwrap();
  let Some(value) = object.get(scope, key.into()) else {
    return Err(OpError::type_("The algorithm is invalid"));
  };
  if value.is_undefined() {
    return Err(OpError::type_(format!("{name} is required")));
  }
  buffer(scope, value)
}

fn read_usages(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<Vec<String>, OpError> {
  let object = v8::Local::<v8::Object>::try_from(value)
    .map_err(|_| OpError::type_("The key usages are invalid"))?;
  let length = object
    .get(scope, v8::String::new(scope, "length").unwrap().into())
    .and_then(|value| value.uint32_value(scope))
    .unwrap_or(0);
  let mut usages = Vec::new();
  for index in 0..length {
    let item = object
      .get_index(scope, index)
      .ok_or_else(|| OpError::type_("The key usages are invalid"))?;
    usages.push(string_value(scope, item)?);
  }
  Ok(usages)
}

fn buffer(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<Vec<u8>, OpError> {
  // SubtleCrypto BufferSource is not [AllowShared].
  if value.is_shared_array_buffer()
    || v8::Local::<v8::ArrayBufferView>::try_from(value)
      .ok()
      .and_then(|view| view.get_backing_store())
      .is_some_and(|store| store.is_shared())
  {
    return Err(OpError::type_("The buffer is shared"));
  }
  bytes::buffer_source(scope, value)
    .ok_or_else(|| OpError::type_("The value is not a BufferSource"))
}

fn string_value(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<String, OpError> {
  value
    .to_string(scope)
    .map(|text| text.to_rust_string_lossy(scope))
    .ok_or_else(|| OpError::type_("The value is not a string"))
}

fn resolve_bytes(scope: &mut v8::PinScope, bytes: Vec<u8>) -> Js {
  let buffer = bytes::array_buffer(scope, bytes);
  let value: v8::Local<v8::Value> = buffer.into();
  resolve(scope, value)
}

fn resolve_key(scope: &mut v8::PinScope, key: CryptoKey) -> Js {
  let _ = CryptoKey::template(scope);
  let object = unsafe { yuzuko_core::op::wrap_instance(scope, key) };
  let value: v8::Local<v8::Value> = object.into();
  resolve(scope, value)
}

fn resolve_bool(scope: &mut v8::PinScope, value: bool) -> Js {
  resolve(scope, v8::Boolean::new(scope, value).into())
}

fn resolve(scope: &mut v8::PinScope, value: v8::Local<v8::Value>) -> Js {
  let resolver = v8::PromiseResolver::new(scope).unwrap();
  resolver.resolve(scope, value);
  let promise = resolver.get_promise(scope);
  let value: v8::Local<v8::Value> = promise.into();
  Js(v8::Global::new(scope, value))
}

fn reject(scope: &mut v8::PinScope, error: OpError) -> Js {
  let resolver = v8::PromiseResolver::new(scope).unwrap();
  let message = match &error {
    OpError::Type(message) | OpError::Range(message) => message.clone(),
    OpError::Dom { message, .. } => message.clone(),
    OpError::Thrown(_) => "The operation failed".to_string(),
  };
  let name = match &error {
    OpError::Dom { name, .. } => name.clone(),
    OpError::Range(_) => "RangeError".to_string(),
    _ => "TypeError".to_string(),
  };
  let exception = if name == "TypeError" {
    let text = v8::String::new(scope, &message).unwrap();
    v8::Exception::type_error(scope, text)
  } else if name == "RangeError" {
    let text = v8::String::new(scope, &message).unwrap();
    v8::Exception::range_error(scope, text)
  } else {
    yuzuko_core::op::new_dom_exception(scope, &name, &message)
      .map(v8::Local::<v8::Value>::from)
      .unwrap_or_else(|| {
        let text = v8::String::new(scope, &message).unwrap();
        v8::Exception::error(scope, text)
      })
  };
  resolver.reject(scope, exception);
  let promise = resolver.get_promise(scope);
  let value: v8::Local<v8::Value> = promise.into();
  Js(v8::Global::new(scope, value))
}

fn quota_exceeded(scope: &mut v8::PinScope, message: &str) -> OpError {
  let global = scope.get_current_context().global(scope);
  let thrown = v8::String::new(scope, "QuotaExceededError")
    .and_then(|key| global.get(scope, key.into()))
    .and_then(|value| v8::Local::<v8::Function>::try_from(value).ok())
    .and_then(|constructor| {
      let message = v8::String::new(scope, message)?;
      constructor.new_instance(scope, &[message.into()])
    });
  if let Some(error) = thrown {
    return OpError::thrown(scope, error.into());
  }
  OpError::dom("QuotaExceededError", message)
}

fn random_values(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<Js, OpError> {
  if value.is_float16_array()
    || value.is_float32_array()
    || value.is_float64_array()
    || value.is_data_view()
  {
    return Err(OpError::dom(
      "TypeMismatchError",
      "getRandomValues requires an integer typed array",
    ));
  }
  let view = v8::Local::<v8::TypedArray>::try_from(value).map_err(|_| {
    OpError::type_("getRandomValues requires an integer typed array")
  })?;
  let length = view.byte_length();
  if length > 65536 {
    return Err(quota_exceeded(
      scope,
      "getRandomValues allows at most 65536 bytes",
    ));
  }
  if length > 0 {
    let mut bytes = vec![0u8; length];
    if getrandom::fill(&mut bytes).is_err() {
      return Err(OpError::dom("OperationError", "Random generation failed"));
    }
    let buffer = view.buffer(scope).ok_or_else(|| {
      OpError::type_("getRandomValues requires an integer typed array")
    })?;
    let store = buffer.get_backing_store();
    let data = store.data().ok_or_else(|| {
      OpError::type_("getRandomValues requires an integer typed array")
    })?;
    let offset = view.byte_offset();
    unsafe {
      std::ptr::copy_nonoverlapping(
        bytes.as_ptr(),
        data.as_ptr().cast::<u8>().add(offset),
        length,
      );
    }
  }
  Ok(Js(v8::Global::new(scope, value)))
}

fn random_uuid() -> Result<String, OpError> {
  let mut bytes = [0u8; 16];
  if getrandom::fill(&mut bytes).is_err() {
    return Err(OpError::dom("OperationError", "Random generation failed"));
  }
  bytes[6] = (bytes[6] & 0x0f) | 0x40;
  bytes[8] = (bytes[8] & 0x3f) | 0x80;
  Ok(format!(
    "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
    bytes[0],
    bytes[1],
    bytes[2],
    bytes[3],
    bytes[4],
    bytes[5],
    bytes[6],
    bytes[7],
    bytes[8],
    bytes[9],
    bytes[10],
    bytes[11],
    bytes[12],
    bytes[13],
    bytes[14],
    bytes[15],
  ))
}

pub struct Crypto {
  subtle: v8::TracedReference<v8::Object>,
}

unsafe impl v8::cppgc::GarbageCollected for Crypto {
  fn trace(&self, visitor: &mut v8::cppgc::Visitor) {
    visitor.trace(&self.subtle);
  }

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"Crypto"
  }
}

#[op]
impl Crypto {
  #[getter]
  fn subtle(&self, scope: &mut v8::PinScope) -> Js {
    let value = traced_local(scope, &self.subtle)
      .map(v8::Local::<v8::Value>::from)
      .unwrap_or_else(|| v8::undefined(scope).into());
    Js(v8::Global::new(scope, value))
  }

  #[rename("getRandomValues")]
  fn get_random_values(
    &self,
    scope: &mut v8::PinScope,
    value: v8::Local<v8::Value>,
  ) -> Result<Js, OpError> {
    let _ = self;
    random_values(scope, value)
  }

  #[rename("randomUUID")]
  fn random_uuid(&self, _scope: &mut v8::PinScope) -> Result<String, OpError> {
    let _ = self;
    random_uuid()
  }
}

fn crypto_attribute(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut ret: v8::ReturnValue,
) {
  let global = scope.get_current_context().global(scope);
  let this = args.this();
  if this.is_null_or_undefined() || this.strict_equals(global.into()) {
    ret.set(args.data());
    return;
  }
  yuzuko_core::op::OpException::throw(
    OpError::type_("illegal invocation"),
    scope,
  );
}

pub fn install(scope: &mut v8::PinScope) {
  CryptoKey::install(scope);
  SubtleCrypto::install(scope);
  Crypto::install(scope);
  let subtle = unsafe { yuzuko_core::op::wrap_instance(scope, SubtleCrypto) };
  let subtle = traced(scope, subtle);
  let crypto =
    unsafe { yuzuko_core::op::wrap_instance(scope, Crypto { subtle }) };
  let getter = v8::Function::builder(crypto_attribute)
    .data(crypto.into())
    .length(0)
    .constructor_behavior(v8::ConstructorBehavior::Throw)
    .build(scope)
    .unwrap();
  let getter_name = v8::String::new(scope, "get crypto").unwrap();
  getter.set_name(getter_name);
  let global = scope.get_current_context().global(scope);
  let key = v8::String::new(scope, "crypto").unwrap();
  let get: v8::Local<v8::Value> = getter.into();
  let set: v8::Local<v8::Value> = v8::undefined(scope).into();
  let mut descriptor = v8::PropertyDescriptor::new_from_get_set(get, set);
  descriptor.set_enumerable(true);
  descriptor.set_configurable(true);
  let _ = global.define_property(scope, key.into(), &descriptor);
}

fn generate_ec(
  scope: &mut v8::PinScope,
  name: &str,
  algorithm: v8::Local<v8::Value>,
  extractable: bool,
  usages: &[String],
) -> Result<Generated, OpError> {
  reject_unknown_usages(name, usages)?;
  let curve = named_curve(scope, algorithm)?;
  reject_empty_usages(name, usages)?;
  let (public_allowed, private_allowed): (&[&str], &[&str]) = if name == "ECDSA"
  {
    (&["verify"], &["sign"])
  } else {
    (&[], &["deriveKey", "deriveBits"])
  };
  let private_usages = filter_usages(usages, private_allowed);
  if private_usages.is_empty() {
    return Err(OpError::dom("SyntaxError", "The key usage is invalid"));
  }
  let public_usages = filter_usages(usages, public_allowed);
  let (pkcs8, spki) =
    crate::crypto::ec_key::generate(&curve).map_err(ec_error)?;
  let private =
    ec_crypto_key("private", name, extractable, &curve, private_usages, pkcs8);
  let public = ec_crypto_key("public", name, true, &curve, public_usages, spki);
  Ok(Generated::Pair {
    public: Box::new(public),
    private: Box::new(private),
  })
}

fn import_ec(
  scope: &mut v8::PinScope,
  name: &str,
  format: &str,
  key_data: v8::Local<v8::Value>,
  algorithm: v8::Local<v8::Value>,
  extractable: bool,
  usages: Vec<String>,
) -> Result<CryptoKey, OpError> {
  let curve = named_curve(scope, algorithm)?;
  match format.to_ascii_lowercase().as_str() {
    "raw" => {
      check_ec_usages(name, &usages, false)?;
      let bytes = buffer(scope, key_data)?;
      let spki =
        crate::crypto::ec_key::import_raw(&curve, &bytes).map_err(ec_error)?;
      Ok(ec_crypto_key(
        "public",
        name,
        extractable,
        &curve,
        usages,
        spki,
      ))
    }
    "spki" => {
      check_ec_usages(name, &usages, false)?;
      let der = buffer(scope, key_data)?;
      let der = crate::crypto::ec_key::normalize_spki(&curve, &der)
        .map_err(ec_error)?;
      Ok(ec_crypto_key(
        "public",
        name,
        extractable,
        &curve,
        usages,
        der,
      ))
    }
    "pkcs8" => {
      check_ec_usages(name, &usages, true)?;
      let der = buffer(scope, key_data)?;
      crate::crypto::ec_key::private_ok(&curve, &der).map_err(ec_error)?;
      Ok(ec_crypto_key(
        "private",
        name,
        extractable,
        &curve,
        usages,
        der,
      ))
    }
    "jwk" => import_ec_jwk(scope, name, key_data, curve, extractable, usages),
    _ => Err(OpError::dom(
      "NotSupportedError",
      "The key format is not supported",
    )),
  }
}

fn import_ec_jwk(
  scope: &mut v8::PinScope,
  name: &str,
  key_data: v8::Local<v8::Value>,
  curve: String,
  extractable: bool,
  usages: Vec<String>,
) -> Result<CryptoKey, OpError> {
  let jwk = read_jwk(scope, key_data)?;
  if jwk.kty.as_deref() != Some("EC") {
    return Err(OpError::dom("DataError", "The JWK kty is invalid"));
  }
  if jwk.crv.as_deref() != Some(curve.as_str()) {
    return Err(OpError::dom("DataError", "The JWK crv is invalid"));
  }
  let private = jwk.d.is_some();
  check_ec_usages(name, &usages, private)?;
  let expected_use = if name == "ECDSA" { "sig" } else { "enc" };
  check_use(&usages, jwk.use_.as_deref(), expected_use)?;
  check_key_ops(jwk.key_ops.as_deref(), &usages)?;
  check_ext(jwk.ext, extractable)?;
  let x = jwk_component(jwk.x.as_deref())?;
  let y = jwk_component(jwk.y.as_deref())?;
  if private {
    let d = jwk_component(jwk.d.as_deref())?;
    let der = crate::crypto::ec_key::import_private_jwk(&curve, &x, &y, &d)
      .map_err(ec_error)?;
    return Ok(ec_crypto_key(
      "private",
      name,
      extractable,
      &curve,
      usages,
      der,
    ));
  }
  let der = crate::crypto::ec_key::import_public_jwk(&curve, &x, &y)
    .map_err(ec_error)?;
  Ok(ec_crypto_key(
    "public",
    name,
    extractable,
    &curve,
    usages,
    der,
  ))
}

fn ec_sign(
  scope: &mut v8::PinScope,
  algorithm: v8::Local<v8::Value>,
  key: v8::Local<v8::Value>,
  data: v8::Local<v8::Value>,
) -> Result<Vec<u8>, OpError> {
  let hash = required_digest(scope, algorithm)?;
  let key = load_key(scope, key)?;
  if key.algorithm != "ECDSA"
    || key.kind != "private"
    || !key.usages.iter().any(|usage| usage == "sign")
  {
    return Err(OpError::dom(
      "InvalidAccessError",
      "The key usage is invalid",
    ));
  }
  let data = buffer(scope, data)?;
  let digest = hash_data(&hash, &data)?;
  let curve = key.curve.as_deref().unwrap_or("");
  crate::crypto::ec_key::sign(curve, &key.bytes, &digest).map_err(ec_error)
}

fn ec_verify(
  scope: &mut v8::PinScope,
  algorithm: v8::Local<v8::Value>,
  key: v8::Local<v8::Value>,
  signature: v8::Local<v8::Value>,
  data: v8::Local<v8::Value>,
) -> Result<bool, OpError> {
  let hash = required_digest(scope, algorithm)?;
  let key = load_key(scope, key)?;
  if key.algorithm != "ECDSA"
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
  let digest = hash_data(&hash, &data)?;
  let curve = key.curve.as_deref().unwrap_or("");
  crate::crypto::ec_key::verify(curve, &key.bytes, &digest, &signature)
    .map_err(ec_error)
}

fn member_key(
  scope: &mut v8::PinScope,
  algorithm: v8::Local<v8::Value>,
  name: &str,
) -> Result<KeyView, OpError> {
  let object = v8::Local::<v8::Object>::try_from(algorithm)
    .map_err(|_| OpError::type_("The public key is required"))?;
  let key = v8::String::new(scope, name).unwrap();
  let Some(value) = object.get(scope, key.into()) else {
    return Err(OpError::type_("The public key is required"));
  };
  if value.is_null() || value.is_undefined() {
    return Err(OpError::type_("The public key is required"));
  }
  load_key(scope, value)
}

fn ec_crypto_key(
  kind: &str,
  algorithm: &str,
  extractable: bool,
  curve: &str,
  usages: Vec<String>,
  der: Vec<u8>,
) -> CryptoKey {
  let mut key = secret_key(algorithm, extractable, None, 0, usages, der);
  key.kind = kind.to_string();
  key.curve = Some(curve.to_string());
  key
}

fn check_ec_usages(
  name: &str,
  usages: &[String],
  private: bool,
) -> Result<(), OpError> {
  reject_unknown_usages(name, usages)?;
  let allowed: &[&str] = if name == "ECDSA" {
    if private { &["sign"] } else { &["verify"] }
  } else if private {
    &["deriveKey", "deriveBits"]
  } else {
    &[]
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

fn ec_jwk_value<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  jwk: &crate::crypto::ec_key::EcJwk,
  key_ops: &[String],
  ext: bool,
) -> v8::Local<'s, v8::Value> {
  let object = v8::Object::new(scope);
  set_string(scope, object, "kty", "EC");
  set_string(scope, object, "crv", &jwk.crv);
  set_string(scope, object, "x", &jwk.x);
  set_string(scope, object, "y", &jwk.y);
  if let Some(d) = &jwk.d {
    set_string(scope, object, "d", d);
  }
  let operations = v8::Array::new(scope, key_ops.len() as i32);
  for (index, usage) in key_ops.iter().enumerate() {
    let text = v8::String::new(scope, usage).unwrap();
    operations.set_index(scope, index as u32, text.into());
  }
  let key = v8::String::new(scope, "key_ops").unwrap();
  object.set(scope, key.into(), operations.into());
  let ext = v8::Boolean::new(scope, ext);
  let key = v8::String::new(scope, "ext").unwrap();
  object.set(scope, key.into(), ext.into());
  object.into()
}

fn ec_jwk_json(
  jwk: &crate::crypto::ec_key::EcJwk,
  key_ops: &[String],
  ext: bool,
) -> String {
  let operations = key_ops
    .iter()
    .map(|usage| format!("\"{usage}\""))
    .collect::<Vec<_>>()
    .join(",");
  let d = jwk
    .d
    .as_ref()
    .map(|d| format!(",\"d\":\"{d}\""))
    .unwrap_or_default();
  format!(
    "{{\"kty\":\"EC\",\"crv\":\"{}\",\"x\":\"{}\",\"y\":\"{}\"{d},\"key_ops\":[{operations}],\"ext\":{}}}",
    jwk.crv,
    jwk.x,
    jwk.y,
    if ext { "true" } else { "false" }
  )
}

fn hash_data(hash: &str, data: &[u8]) -> Result<Vec<u8>, OpError> {
  Ok(match hash {
    "SHA-1" => Sha1::digest(data).to_vec(),
    "SHA-256" => Sha256::digest(data).to_vec(),
    "SHA-384" => Sha384::digest(data).to_vec(),
    "SHA-512" => Sha512::digest(data).to_vec(),
    _ => {
      return Err(OpError::dom(
        "NotSupportedError",
        "The hash algorithm is not supported",
      ));
    }
  })
}

fn is_ec(name: &str) -> bool {
  matches!(name, "ECDH" | "ECDSA")
}

fn ec_error(error: crate::crypto::ec_key::EcError) -> OpError {
  match error {
    crate::crypto::ec_key::EcError::Data => {
      OpError::dom("DataError", "The key data is invalid")
    }
    crate::crypto::ec_key::EcError::Operation => {
      OpError::dom("OperationError", "The operation failed")
    }
    crate::crypto::ec_key::EcError::Unsupported => {
      OpError::dom("NotSupportedError", "The named curve is not supported")
    }
  }
}

fn generate_okp(
  name: &str,
  extractable: bool,
  usages: &[String],
) -> Result<Generated, OpError> {
  reject_unknown_usages(name, usages)?;
  reject_empty_usages(name, usages)?;
  let (public_allowed, private_allowed): (&[&str], &[&str]) =
    if name == "Ed25519" || name == "Ed448" {
      (&["verify"], &["sign"])
    } else {
      (&[], &["deriveKey", "deriveBits"])
    };
  let private_usages = filter_usages(usages, private_allowed);
  if private_usages.is_empty() {
    return Err(OpError::dom("SyntaxError", "The key usage is invalid"));
  }
  let public_usages = filter_usages(usages, public_allowed);
  let (pkcs8, spki) =
    crate::crypto::okp_key::generate(name).map_err(okp_error)?;
  Ok(Generated::Pair {
    public: Box::new(okp_crypto_key("public", name, true, public_usages, spki)),
    private: Box::new(okp_crypto_key(
      "private",
      name,
      extractable,
      private_usages,
      pkcs8,
    )),
  })
}

fn import_okp(
  scope: &mut v8::PinScope,
  name: &str,
  format: &str,
  key_data: v8::Local<v8::Value>,
  extractable: bool,
  usages: Vec<String>,
) -> Result<CryptoKey, OpError> {
  match format.to_ascii_lowercase().as_str() {
    "raw" => {
      check_okp_usages(name, &usages, false)?;
      let bytes = buffer(scope, key_data)?;
      let spki =
        crate::crypto::okp_key::import_raw(name, &bytes).map_err(okp_error)?;
      Ok(okp_crypto_key("public", name, extractable, usages, spki))
    }
    "spki" => {
      check_okp_usages(name, &usages, false)?;
      let der = buffer(scope, key_data)?;
      // Parsing checks the OID. The original bytes stay so export matches.
      let _ =
        crate::crypto::okp_key::raw_public(name, &der).map_err(okp_error)?;
      Ok(okp_crypto_key("public", name, extractable, usages, der))
    }
    "pkcs8" => {
      check_okp_usages(name, &usages, true)?;
      let der = buffer(scope, key_data)?;
      crate::crypto::okp_key::private_ok(name, &der).map_err(okp_error)?;
      Ok(okp_crypto_key("private", name, extractable, usages, der))
    }
    "jwk" => import_okp_jwk(scope, name, key_data, extractable, usages),
    _ => Err(OpError::dom(
      "NotSupportedError",
      "The key format is not supported",
    )),
  }
}

fn import_okp_jwk(
  scope: &mut v8::PinScope,
  name: &str,
  key_data: v8::Local<v8::Value>,
  extractable: bool,
  usages: Vec<String>,
) -> Result<CryptoKey, OpError> {
  let jwk = read_jwk(scope, key_data)?;
  if jwk.kty.as_deref() != Some("OKP") {
    return Err(OpError::dom("DataError", "The JWK kty is invalid"));
  }
  if jwk.crv.as_deref() != Some(name) {
    return Err(OpError::dom("DataError", "The JWK crv is invalid"));
  }
  if (name == "Ed25519" || name == "Ed448")
    && let Some(alg) = jwk.alg.as_deref()
    && alg != name
    && alg != "EdDSA"
  {
    return Err(OpError::dom("DataError", "The JWK alg is invalid"));
  }
  let private = jwk.d.is_some();
  check_okp_usages(name, &usages, private)?;
  let expected_use = if name == "Ed25519" || name == "Ed448" {
    "sig"
  } else {
    "enc"
  };
  check_use(&usages, jwk.use_.as_deref(), expected_use)?;
  check_key_ops(jwk.key_ops.as_deref(), &usages)?;
  check_ext(jwk.ext, extractable)?;
  let x = jwk_component(jwk.x.as_deref())?;
  if private {
    let d = jwk_component(jwk.d.as_deref())?;
    let der = crate::crypto::okp_key::import_private_jwk(name, &x, &d)
      .map_err(okp_error)?;
    return Ok(okp_crypto_key("private", name, extractable, usages, der));
  }
  let der =
    crate::crypto::okp_key::import_public_jwk(name, &x).map_err(okp_error)?;
  Ok(okp_crypto_key("public", name, extractable, usages, der))
}

fn ed25519_sign(
  scope: &mut v8::PinScope,
  key: v8::Local<v8::Value>,
  data: v8::Local<v8::Value>,
) -> Result<Vec<u8>, OpError> {
  let key = load_key(scope, key)?;
  if (key.algorithm != "Ed25519" && key.algorithm != "Ed448")
    || key.kind != "private"
    || !key.usages.iter().any(|usage| usage == "sign")
  {
    return Err(OpError::dom(
      "InvalidAccessError",
      "The key usage is invalid",
    ));
  }
  let data = buffer(scope, data)?;
  if key.algorithm == "Ed448" {
    crate::crypto::okp_key::sign_ed448(&key.bytes, &data).map_err(okp_error)
  } else {
    crate::crypto::okp_key::sign(&key.bytes, &data).map_err(okp_error)
  }
}

fn ed25519_verify(
  scope: &mut v8::PinScope,
  key: v8::Local<v8::Value>,
  signature: v8::Local<v8::Value>,
  data: v8::Local<v8::Value>,
) -> Result<bool, OpError> {
  let key = load_key(scope, key)?;
  if (key.algorithm != "Ed25519" && key.algorithm != "Ed448")
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
  if key.algorithm == "Ed448" {
    crate::crypto::okp_key::verify_ed448(&key.bytes, &data, &signature)
      .map_err(okp_error)
  } else {
    crate::crypto::okp_key::verify(&key.bytes, &data, &signature)
      .map_err(okp_error)
  }
}

fn okp_crypto_key(
  kind: &str,
  algorithm: &str,
  extractable: bool,
  usages: Vec<String>,
  der: Vec<u8>,
) -> CryptoKey {
  let mut key = secret_key(algorithm, extractable, None, 0, usages, der);
  key.kind = kind.to_string();
  key
}

fn check_okp_usages(
  name: &str,
  usages: &[String],
  private: bool,
) -> Result<(), OpError> {
  reject_unknown_usages(name, usages)?;
  let allowed: &[&str] = if name == "Ed25519" || name == "Ed448" {
    if private { &["sign"] } else { &["verify"] }
  } else if private {
    &["deriveKey", "deriveBits"]
  } else {
    &[]
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

fn okp_jwk_value<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  jwk: &crate::crypto::okp_key::OkpJwk,
  key_ops: &[String],
  ext: bool,
) -> v8::Local<'s, v8::Value> {
  let object = v8::Object::new(scope);
  set_string(scope, object, "kty", "OKP");
  set_string(scope, object, "crv", &jwk.crv);
  set_string(scope, object, "x", &jwk.x);
  if let Some(d) = &jwk.d {
    set_string(scope, object, "d", d);
  }
  if let Some(alg) = &jwk.alg {
    set_string(scope, object, "alg", alg);
  }
  let operations = v8::Array::new(scope, key_ops.len() as i32);
  for (index, usage) in key_ops.iter().enumerate() {
    let text = v8::String::new(scope, usage).unwrap();
    operations.set_index(scope, index as u32, text.into());
  }
  let key = v8::String::new(scope, "key_ops").unwrap();
  object.set(scope, key.into(), operations.into());
  let ext_value = v8::Boolean::new(scope, ext);
  let key = v8::String::new(scope, "ext").unwrap();
  object.set(scope, key.into(), ext_value.into());
  object.into()
}

fn okp_jwk_json(
  jwk: &crate::crypto::okp_key::OkpJwk,
  key_ops: &[String],
  ext: bool,
) -> String {
  let operations = key_ops
    .iter()
    .map(|usage| format!("\"{usage}\""))
    .collect::<Vec<_>>()
    .join(",");
  let d = jwk
    .d
    .as_ref()
    .map(|d| format!(",\"d\":\"{d}\""))
    .unwrap_or_default();
  let alg = jwk
    .alg
    .as_ref()
    .map(|alg| format!(",\"alg\":\"{alg}\""))
    .unwrap_or_default();
  format!(
    "{{\"kty\":\"OKP\",\"crv\":\"{}\",\"x\":\"{}\"{d}{alg},\"key_ops\":[{operations}],\"ext\":{}}}",
    jwk.crv,
    jwk.x,
    if ext { "true" } else { "false" }
  )
}

fn is_okp(name: &str) -> bool {
  matches!(name, "Ed25519" | "Ed448" | "X25519" | "X448")
}

fn okp_error(error: crate::crypto::okp_key::OkpError) -> OpError {
  match error {
    crate::crypto::okp_key::OkpError::Data => {
      OpError::dom("DataError", "The key data is invalid")
    }
    crate::crypto::okp_key::OkpError::Operation => {
      OpError::dom("OperationError", "The operation failed")
    }
  }
}

fn generate_rsa(
  scope: &mut v8::PinScope,
  name: &str,
  algorithm: v8::Local<v8::Value>,
  extractable: bool,
  usages: &[String],
) -> Result<Generated, OpError> {
  let hash = required_digest(scope, algorithm)?;
  reject_unknown_usages(name, usages)?;
  let modulus = required_modulus(scope, algorithm)?;
  let exponent = algorithm_bytes(scope, algorithm, "publicExponent")?;
  if !crate::crypto::rsa_key::modulus_acceptable(modulus)
    || !crate::crypto::rsa_key::acceptable_exponent(&exponent)
  {
    return Err(OpError::dom(
      "OperationError",
      "The RSA key parameters are invalid",
    ));
  }
  let (public_allowed, private_allowed) = rsa_usage_sets(name);
  let private_usages = filter_usages(usages, private_allowed);
  if private_usages.is_empty() {
    return Err(OpError::dom("SyntaxError", "The key usage is invalid"));
  }
  let public_usages = filter_usages(usages, public_allowed);
  let (pkcs8, spki, parts) =
    crate::crypto::rsa_key::generate(modulus, &exponent).map_err(rsa_error)?;
  let private = rsa_crypto_key(
    "private",
    name,
    extractable,
    hash.clone(),
    parts.modulus_length,
    parts.public_exponent.clone(),
    private_usages,
    pkcs8,
  );
  let public = rsa_crypto_key(
    "public",
    name,
    true,
    hash,
    parts.modulus_length,
    parts.public_exponent,
    public_usages,
    spki,
  );
  Ok(Generated::Pair {
    public: Box::new(public),
    private: Box::new(private),
  })
}

fn import_rsa(
  scope: &mut v8::PinScope,
  name: &str,
  format: &str,
  key_data: v8::Local<v8::Value>,
  algorithm: v8::Local<v8::Value>,
  extractable: bool,
  usages: Vec<String>,
) -> Result<CryptoKey, OpError> {
  let hash = required_digest(scope, algorithm)?;
  match format.to_ascii_lowercase().as_str() {
    "pkcs8" => {
      check_rsa_usages(name, &usages, true)?;
      let der = buffer(scope, key_data)?;
      import_rsa_der(name, true, der, hash, extractable, usages)
    }
    "spki" => {
      check_rsa_usages(name, &usages, false)?;
      let der = buffer(scope, key_data)?;
      import_rsa_der(name, false, der, hash, extractable, usages)
    }
    "jwk" => import_rsa_jwk(scope, name, key_data, hash, extractable, usages),
    _ => Err(OpError::dom(
      "NotSupportedError",
      "The key format is not supported",
    )),
  }
}

fn import_rsa_der(
  name: &str,
  private: bool,
  der: Vec<u8>,
  hash: String,
  extractable: bool,
  usages: Vec<String>,
) -> Result<CryptoKey, OpError> {
  let parts = if private {
    crate::crypto::rsa_key::private_parts(&der)
  } else {
    crate::crypto::rsa_key::public_parts(&der)
  }
  .map_err(rsa_error)?;
  Ok(rsa_crypto_key(
    if private { "private" } else { "public" },
    name,
    extractable,
    hash,
    parts.modulus_length,
    parts.public_exponent,
    usages,
    der,
  ))
}

fn import_rsa_jwk(
  scope: &mut v8::PinScope,
  name: &str,
  key_data: v8::Local<v8::Value>,
  hash: String,
  extractable: bool,
  usages: Vec<String>,
) -> Result<CryptoKey, OpError> {
  let jwk = read_jwk(scope, key_data)?;
  if jwk.kty.as_deref() != Some("RSA") {
    return Err(OpError::dom("DataError", "The JWK kty is invalid"));
  }
  let private_values = [
    jwk.d.as_ref(),
    jwk.p.as_ref(),
    jwk.q.as_ref(),
    jwk.dp.as_ref(),
    jwk.dq.as_ref(),
    jwk.qi.as_ref(),
  ];
  let present = private_values
    .iter()
    .filter(|value| value.is_some())
    .count();
  if present != 0 && present != private_values.len() {
    return Err(OpError::dom("DataError", "The JWK member is missing"));
  }
  let private = present != 0;
  check_rsa_usages(name, &usages, private)?;
  let expected_use = if name == "RSA-OAEP" { "enc" } else { "sig" };
  check_use(&usages, jwk.use_.as_deref(), expected_use)?;
  check_key_ops(jwk.key_ops.as_deref(), &usages)?;
  check_ext(jwk.ext, extractable)?;
  if let Some(alg) = jwk.alg.as_deref() {
    let expected = crate::crypto::rsa_key::jwk_alg(name, &hash);
    if expected != Some(alg) {
      return Err(OpError::dom("DataError", "The JWK alg is invalid"));
    }
  }
  if private {
    let components = crate::crypto::rsa_key::PrivateJwk {
      n: jwk_component(jwk.n.as_deref())?,
      e: jwk_component(jwk.e.as_deref())?,
      d: jwk_component(jwk.d.as_deref())?,
      p: jwk_component(jwk.p.as_deref())?,
      q: jwk_component(jwk.q.as_deref())?,
      dp: jwk_component(jwk.dp.as_deref())?,
      dq: jwk_component(jwk.dq.as_deref())?,
      qi: jwk_component(jwk.qi.as_deref())?,
    };
    let (der, parts) = crate::crypto::rsa_key::import_private_jwk(&components)
      .map_err(rsa_error)?;
    return Ok(rsa_crypto_key(
      "private",
      name,
      extractable,
      hash,
      parts.modulus_length,
      parts.public_exponent,
      usages,
      der,
    ));
  }
  let (der, parts) = crate::crypto::rsa_key::import_public_jwk(
    &jwk_component(jwk.n.as_deref())?,
    &jwk_component(jwk.e.as_deref())?,
  )
  .map_err(rsa_error)?;
  Ok(rsa_crypto_key(
    "public",
    name,
    extractable,
    hash,
    parts.modulus_length,
    parts.public_exponent,
    usages,
    der,
  ))
}

fn rsa_sign(
  scope: &mut v8::PinScope,
  name: &str,
  algorithm: v8::Local<v8::Value>,
  key: v8::Local<v8::Value>,
  data: v8::Local<v8::Value>,
) -> Result<Vec<u8>, OpError> {
  let salt = if name == "RSA-PSS" {
    Some(pss_salt(scope, algorithm)?)
  } else {
    None
  };
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
  let hash = key.hash.as_deref().unwrap_or("");
  let signed = if let Some(salt) = salt {
    crate::crypto::rsa_key::sign_pss(&key.bytes, hash, salt as usize, &data)
  } else {
    crate::crypto::rsa_key::sign_pkcs1(&key.bytes, hash, &data)
  };
  signed.map_err(rsa_error)
}

fn rsa_verify(
  scope: &mut v8::PinScope,
  name: &str,
  algorithm: v8::Local<v8::Value>,
  key: v8::Local<v8::Value>,
  signature: v8::Local<v8::Value>,
  data: v8::Local<v8::Value>,
) -> Result<bool, OpError> {
  let salt = if name == "RSA-PSS" {
    Some(pss_salt(scope, algorithm)?)
  } else {
    None
  };
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
  let hash = key.hash.as_deref().unwrap_or("");
  let valid = if let Some(salt) = salt {
    crate::crypto::rsa_key::verify_pss(
      &key.bytes,
      hash,
      salt as usize,
      &data,
      &signature,
    )
  } else {
    crate::crypto::rsa_key::verify_pkcs1(&key.bytes, hash, &data, &signature)
  };
  valid.map_err(rsa_error)
}

fn rsa_oaep_key(
  scope: &mut v8::PinScope,
  algorithm: v8::Local<v8::Value>,
  key: v8::Local<v8::Value>,
  usage: &str,
) -> Result<(KeyView, Vec<u8>), OpError> {
  let label = optional_algorithm_bytes(scope, algorithm, "label")?;
  let key = load_key(scope, key)?;
  let encrypting = usage == "encrypt" || usage == "wrapKey";
  if key.algorithm != "RSA-OAEP"
    || !key.usages.iter().any(|item| item == usage)
    || (encrypting && key.kind != "public")
    || (!encrypting && key.kind != "private")
  {
    return Err(OpError::dom(
      "InvalidAccessError",
      "The key usage is invalid",
    ));
  }
  Ok((key, label))
}

fn oaep_run(
  key: &KeyView,
  label: &[u8],
  data: &[u8],
  encrypting: bool,
) -> Result<Vec<u8>, OpError> {
  let hash = key.hash.as_deref().unwrap_or("");
  let result = if encrypting {
    crate::crypto::rsa_key::encrypt_oaep(&key.bytes, hash, label, data)
  } else {
    crate::crypto::rsa_key::decrypt_oaep(&key.bytes, hash, label, data)
  };
  result.map_err(rsa_error)
}

fn export_plaintext(
  scope: &mut v8::PinScope,
  format: &str,
  key: v8::Local<v8::Value>,
) -> Result<Vec<u8>, OpError> {
  match export_key(scope, format, key)? {
    Exported::Raw(bytes) => Ok(bytes),
    Exported::Jwk(jwk) => Ok(jwk_json(&jwk).into_bytes()),
    Exported::Rsa {
      jwk,
      alg,
      key_ops,
      ext,
    } => Ok(rsa_jwk_json(&jwk, &alg, &key_ops, ext).into_bytes()),
    Exported::Ec { jwk, key_ops, ext } => {
      Ok(ec_jwk_json(&jwk, &key_ops, ext).into_bytes())
    }
    Exported::Okp { jwk, key_ops, ext } => {
      Ok(okp_jwk_json(&jwk, &key_ops, ext).into_bytes())
    }
  }
}

fn export_supported(algorithm: &str, format: &str) -> bool {
  let format = format.to_ascii_lowercase();
  match algorithm {
    "HMAC" | "AES-GCM" | "AES-CBC" | "AES-CTR" | "AES-KW" => {
      matches!(format.as_str(), "raw" | "jwk")
    }
    "RSASSA-PKCS1-v1_5" | "RSA-PSS" | "RSA-OAEP" => {
      matches!(format.as_str(), "pkcs8" | "spki" | "jwk")
    }
    "ECDH" | "ECDSA" | "Ed25519" | "Ed448" | "X25519" | "X448" => {
      matches!(format.as_str(), "raw" | "pkcs8" | "spki" | "jwk")
    }
    _ => false,
  }
}

fn rsa_jwk_value<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  jwk: &crate::crypto::rsa_key::RsaJwk,
  alg: &str,
  key_ops: &[String],
  ext: bool,
) -> v8::Local<'s, v8::Value> {
  let object = v8::Object::new(scope);
  set_string(scope, object, "kty", "RSA");
  set_string(scope, object, "n", &jwk.n);
  set_string(scope, object, "e", &jwk.e);
  if let Some(value) = &jwk.d {
    set_string(scope, object, "d", value);
  }
  if let Some(value) = &jwk.p {
    set_string(scope, object, "p", value);
  }
  if let Some(value) = &jwk.q {
    set_string(scope, object, "q", value);
  }
  if let Some(value) = &jwk.dp {
    set_string(scope, object, "dp", value);
  }
  if let Some(value) = &jwk.dq {
    set_string(scope, object, "dq", value);
  }
  if let Some(value) = &jwk.qi {
    set_string(scope, object, "qi", value);
  }
  if !alg.is_empty() {
    set_string(scope, object, "alg", alg);
  }
  let operations = v8::Array::new(scope, key_ops.len() as i32);
  for (index, usage) in key_ops.iter().enumerate() {
    let text = v8::String::new(scope, usage).unwrap();
    operations.set_index(scope, index as u32, text.into());
  }
  let key = v8::String::new(scope, "key_ops").unwrap();
  object.set(scope, key.into(), operations.into());
  let ext = v8::Boolean::new(scope, ext);
  let key = v8::String::new(scope, "ext").unwrap();
  object.set(scope, key.into(), ext.into());
  object.into()
}

fn rsa_jwk_json(
  jwk: &crate::crypto::rsa_key::RsaJwk,
  alg: &str,
  key_ops: &[String],
  ext: bool,
) -> String {
  let mut fields = vec![
    "\"kty\":\"RSA\"".to_string(),
    format!("\"n\":\"{}\"", jwk.n),
    format!("\"e\":\"{}\"", jwk.e),
  ];
  if let Some(value) = &jwk.d {
    fields.push(format!("\"d\":\"{value}\""));
  }
  if let Some(value) = &jwk.p {
    fields.push(format!("\"p\":\"{value}\""));
  }
  if let Some(value) = &jwk.q {
    fields.push(format!("\"q\":\"{value}\""));
  }
  if let Some(value) = &jwk.dp {
    fields.push(format!("\"dp\":\"{value}\""));
  }
  if let Some(value) = &jwk.dq {
    fields.push(format!("\"dq\":\"{value}\""));
  }
  if let Some(value) = &jwk.qi {
    fields.push(format!("\"qi\":\"{value}\""));
  }
  if !alg.is_empty() {
    fields.push(format!("\"alg\":\"{alg}\""));
  }
  let operations = key_ops
    .iter()
    .map(|usage| format!("\"{usage}\""))
    .collect::<Vec<_>>()
    .join(",");
  fields.push(format!("\"key_ops\":[{operations}]"));
  fields.push(format!("\"ext\":{}", if ext { "true" } else { "false" }));
  format!("{{{}}}", fields.join(","))
}

fn rsa_crypto_key(
  kind: &str,
  algorithm: &str,
  extractable: bool,
  hash: String,
  modulus_length: u32,
  public_exponent: Vec<u8>,
  usages: Vec<String>,
  der: Vec<u8>,
) -> CryptoKey {
  let mut key = secret_key(algorithm, extractable, Some(hash), 0, usages, der);
  key.kind = kind.to_string();
  key.modulus_length = modulus_length;
  key.public_exponent = public_exponent;
  key
}

fn rsa_usage_sets(
  name: &str,
) -> (&'static [&'static str], &'static [&'static str]) {
  if name == "RSA-OAEP" {
    (&["encrypt", "wrapKey"], &["decrypt", "unwrapKey"])
  } else {
    (&["verify"], &["sign"])
  }
}

fn check_rsa_usages(
  name: &str,
  usages: &[String],
  private: bool,
) -> Result<(), OpError> {
  reject_unknown_usages(name, usages)?;
  let (public, private_allowed) = rsa_usage_sets(name);
  let allowed = if private { private_allowed } else { public };
  if usages
    .iter()
    .any(|usage| !allowed.contains(&usage.as_str()))
    || (private && usages.is_empty())
  {
    return Err(OpError::dom("SyntaxError", "The key usage is invalid"));
  }
  Ok(())
}

fn filter_usages(usages: &[String], allowed: &[&str]) -> Vec<String> {
  usages
    .iter()
    .filter(|usage| allowed.contains(&usage.as_str()))
    .cloned()
    .collect()
}

fn is_rsa(name: &str) -> bool {
  matches!(name, "RSASSA-PKCS1-v1_5" | "RSA-PSS" | "RSA-OAEP")
}

fn is_rsa_signature(name: &str) -> bool {
  matches!(name, "RSASSA-PKCS1-v1_5" | "RSA-PSS")
}

fn known_digest(name: &str) -> bool {
  matches!(name, "SHA-1" | "SHA-256" | "SHA-384" | "SHA-512")
}

fn required_digest(
  scope: &mut v8::PinScope,
  algorithm: v8::Local<v8::Value>,
) -> Result<String, OpError> {
  let Some(hash) = algorithm_hash(scope, algorithm)? else {
    return Err(OpError::type_("The hash is required"));
  };
  if known_digest(&hash) {
    Ok(hash)
  } else {
    Err(OpError::dom(
      "NotSupportedError",
      "The hash algorithm is not supported",
    ))
  }
}

fn required_modulus(
  scope: &mut v8::PinScope,
  algorithm: v8::Local<v8::Value>,
) -> Result<u32, OpError> {
  let Some(number) = algorithm_number(scope, algorithm, "modulusLength") else {
    return Err(OpError::type_("modulusLength is required"));
  };
  enforce_unsigned_long(number, "modulusLength")
}

fn pss_salt(
  scope: &mut v8::PinScope,
  algorithm: v8::Local<v8::Value>,
) -> Result<u32, OpError> {
  let Some(number) = algorithm_number(scope, algorithm, "saltLength") else {
    return Err(OpError::type_("saltLength is required"));
  };
  enforce_unsigned_long(number, "saltLength")
}

fn jwk_component(value: Option<&str>) -> Result<Vec<u8>, OpError> {
  let Some(value) = value else {
    return Err(OpError::dom("DataError", "The JWK member is missing"));
  };
  base64::decode_url(value)
    .map_err(|_| OpError::dom("DataError", "The JWK member is not base64url"))
}

fn rsa_error(error: crate::crypto::rsa_key::RsaError) -> OpError {
  match error {
    crate::crypto::rsa_key::RsaError::Data => {
      OpError::dom("DataError", "The key data is invalid")
    }
    crate::crypto::rsa_key::RsaError::Operation => {
      OpError::dom("OperationError", "The operation failed")
    }
    crate::crypto::rsa_key::RsaError::Unsupported => {
      OpError::dom("NotSupportedError", "The hash algorithm is not supported")
    }
  }
}

#[cfg(test)]
mod tests {
  use super::Aes128;
  use super::aes_kw_unwrap;
  use super::aes_kw_wrap;
  use super::cbc_crypt;
  use super::encrypt_block;
  use super::gcm_crypt;
  use super::increment_counter;

  #[test]
  fn aes_cbc_matches_nist_block() {
    let zero = [0u8; 16];
    let raw = encrypt_block::<Aes128>(&zero, &zero).unwrap();
    assert_eq!(
      raw,
      [
        0x66, 0xe9, 0x4b, 0xd4, 0xef, 0x8a, 0x2c, 0x3b, 0x88, 0x4c, 0xfa, 0x59,
        0xca, 0x34, 0x2b, 0x2e,
      ]
    );
    let fips_key = [
      0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b,
      0x0c, 0x0d, 0x0e, 0x0f,
    ];
    let fips_plain = [
      0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb,
      0xcc, 0xdd, 0xee, 0xff,
    ];
    assert_eq!(
      encrypt_block::<Aes128>(&fips_key, &fips_plain).unwrap(),
      [
        0x69, 0xc4, 0xe0, 0xd8, 0x6a, 0x7b, 0x04, 0x30, 0xd8, 0xcd, 0xb7, 0x80,
        0x70, 0xb4, 0xc5, 0x5a,
      ]
    );
    let key = [
      0x2b, 0x7e, 0x15, 0x16, 0x28, 0xae, 0xd2, 0xa6, 0xab, 0xf7, 0x15, 0x88,
      0x09, 0xcf, 0x4f, 0x3c,
    ];
    let iv = [
      0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b,
      0x0c, 0x0d, 0x0e, 0x0f,
    ];
    let plain = [
      0x6b, 0xc1, 0xbe, 0xe2, 0x2e, 0x40, 0x9f, 0x96, 0xe9, 0x3d, 0x7e, 0x11,
      0x73, 0x93, 0x17, 0x2a,
    ];
    let cipher = cbc_crypt(&key, &iv, &plain, true).unwrap();
    assert_eq!(
      cipher[..16],
      [
        0x76, 0x49, 0xab, 0xac, 0x81, 0x19, 0xb2, 0x46, 0xce, 0xe9, 0x8e, 0x9b,
        0x12, 0xe9, 0x19, 0x7d,
      ]
    );
    let decoded = cbc_crypt(&key, &iv, &cipher, false).unwrap();
    assert_eq!(decoded, plain);
  }

  #[test]
  fn aes_kw_matches_rfc3394() {
    let key = [
      0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b,
      0x0c, 0x0d, 0x0e, 0x0f,
    ];
    let plain = [
      0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb,
      0xcc, 0xdd, 0xee, 0xff,
    ];
    let wrapped = aes_kw_wrap(&key, &plain).unwrap();
    assert_eq!(
      wrapped,
      [
        0x1f, 0xa6, 0x8b, 0x0a, 0x81, 0x12, 0xb4, 0x47, 0xae, 0xf3, 0x4b, 0xd8,
        0xfb, 0x5a, 0x7b, 0x82, 0x9d, 0x3e, 0x86, 0x23, 0x71, 0xd2, 0xcf, 0xe5,
      ]
    );
    assert_eq!(aes_kw_unwrap(&key, &wrapped).unwrap(), plain);
  }

  #[test]
  fn gcm_short_tag_and_long_iv_roundtrip() {
    let key = [0x11u8; 16];
    let iv = [0x22u8; 32];
    let aad = b"aad";
    let plain = b"hello wintertc";
    let cipher = gcm_crypt(&key, &iv, aad, plain, true, 104).unwrap();
    assert_eq!(cipher.len(), plain.len() + 13);
    let out = gcm_crypt(&key, &iv, aad, &cipher, false, 104).unwrap();
    assert_eq!(out, plain);
    assert!(gcm_crypt(&key, &iv, aad, &cipher, false, 128).is_err());
  }

  #[test]
  fn counter_wraps_only_the_low_bits() {
    let mut counter = [0u8; 16];
    counter[14] = 0xf0;
    counter[15] = 0x0f;
    increment_counter(&mut counter, 4);
    assert_eq!(counter[15], 0x00);
    assert_eq!(counter[14], 0xf0);

    counter[15] = 0xff;
    increment_counter(&mut counter, 12);
    assert_eq!(counter[15], 0x00);
    assert_eq!(counter[14], 0xf1);
  }
}
