//! Packaged-application updates.
//!
//! The feed carries a signature and does not carry a public key. The caller
//! passes the feed URL and the Ed25519 trust anchor to `autoUpdate`. The key
//! that verified a staged payload is stored beside that stage and used on the
//! next launch. The feed URL is not stored.

use std::cmp::Ordering;
use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
use std::sync::{Arc, Mutex, Once, OnceLock};
use std::time::Duration;

use base64::Engine;
use base64::prelude::BASE64_STANDARD;
use ed25519_dalek::Signature;
use ed25519_dalek::VerifyingKey;
use serde::Deserialize;

const FEED_LIMIT: usize = 1024 * 1024;
const PAYLOAD_LIMIT: usize = 512 * 1024 * 1024;
const FILE_LIMIT: usize = 100_000;
const PATH_LIMIT: usize = 1024;
const HEADER_LIMIT: usize = 64 * 1024;
const LINE_LIMIT: usize = 2048;
const TIMEOUT: Duration = Duration::from_secs(30);

const NO_FEED: Failure = Failure {
  name: "InvalidStateError",
  message: "There is no update feed.",
};
const NO_ANCHOR: Failure = Failure {
  name: "InvalidStateError",
  message: "There is no update trust anchor.",
};
const BAD_PACKAGED: Failure = Failure {
  name: "InvalidStateError",
  message: "The packaged version cannot be compared.",
};
const INVALID_FEED: Failure = Failure {
  name: "InvalidStateError",
  message: "The update feed is not valid.",
};
const BAD_SIGNATURE: Failure = Failure {
  name: "InvalidStateError",
  message: "The update signature is not valid.",
};
const INVALID_PAYLOAD: Failure = Failure {
  name: "InvalidStateError",
  message: "The update payload is not valid.",
};
const DOWNLOAD_FAILED: Failure = Failure {
  name: "NetworkError",
  message: "The update could not be downloaded.",
};
const TRUNCATED: Failure = Failure {
  name: "NetworkError",
  message: "The connection closed before the body ended.",
};
const NET_DENIED: Failure = Failure {
  name: "NotAllowedError",
  message: "Requires net access, run again with the --allow-net flag",
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Failure {
  pub name: &'static str,
  pub message: &'static str,
}

const EXPECTS_OBJECT: &str = "Yuzuko.autoUpdate expects an options object.";
const BAD_URL: &str = "url must be an absolute http or https URL.";
const BAD_KEY: &str =
  "publicKey must be standard base64 of a 32-byte Ed25519 public key.";
const BAD_CALLBACK: &str = "onUpdateReady must be a function.";
const BAD_INTERVAL: &str = "The update interval must be a finite number of milliseconds from 1 through 2147483647.";

struct Options {
  ready: Option<v8::Global<v8::Function>>,
  interval: Option<Duration>,
  public_key: Option<[u8; 32]>,
  url: Option<String>,
}

enum OptionsError {
  Type(&'static str),
  Thrown(v8::Global<v8::Value>),
}

pub(crate) struct BundleMeta {
  pub name: String,
  pub identifier: Option<String>,
  pub version: String,
  pub entry: String,
  pub permissions: Option<serde_json::Value>,
}

struct PayloadFile {
  path: String,
  bytes: Vec<u8>,
}

struct Payload {
  version: String,
  entry: String,
  files: Vec<PayloadFile>,
}

#[derive(Deserialize)]
struct FeedJson {
  version: String,
  payload: String,
  signature: String,
}

static RESOURCES: Mutex<Option<PathBuf>> = Mutex::new(None);
static INSTALL: Once = Once::new();
static GENERATION: AtomicU64 = AtomicU64::new(0);
static TIMER: Mutex<Option<i32>> = Mutex::new(None);
static FETCH: Mutex<()> = Mutex::new(());

/// Installs `Yuzuko.appVersion` and `Yuzuko.autoUpdate` on the main realm.
pub fn install_auto_update() {
  INSTALL.call_once(|| yuzuko_core::add_extension(&install_updater));
}

pub fn enable_updater(resources: PathBuf) {
  *lock(&RESOURCES) = Some(resources);
  install_auto_update();
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
  mutex.lock().unwrap_or_else(|error| error.into_inner())
}

fn install_updater(scope: &mut v8::PinScope) {
  if yuzuko_core::is_dedicated_worker() {
    return;
  }
  let yuzuko = yuzuko_core::namespace(scope, "Yuzuko");
  let version = match packaged_version() {
    Some(text) => match v8::String::new(scope, &text) {
      Some(text) => text.into(),
      None => v8::null(scope).into(),
    },
    None => v8::null(scope).into(),
  };
  yuzuko_core::set_value(scope, yuzuko, "appVersion", version);
  yuzuko_core::set_function(scope, yuzuko, "autoUpdate", auto_update);
}

fn auto_update(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut ret: v8::ReturnValue,
) {
  let options = match read_options(scope, args.get(0)) {
    Ok(options) => options,
    Err(OptionsError::Type(message)) => {
      yuzuko_core::throw_type(scope, message);
      return;
    }
    Err(OptionsError::Thrown(error)) => {
      let error = v8::Local::new(scope, error);
      scope.throw_exception(error);
      return;
    }
  };
  let Some(resolver) = v8::PromiseResolver::new(scope) else {
    return;
  };
  let promise = resolver.get_promise(scope);
  ret.set(promise.into());
  if !packaged() {
    resolver.resolve(scope, v8::undefined(scope).into());
    return;
  }
  let generation = GENERATION.fetch_add(1, AtomicOrdering::Relaxed) + 1;
  if let Some(id) = lock(&TIMER).take() {
    yuzuko_core::cancel_host(id);
  }
  let resolver = v8::Global::new(scope, resolver);
  start_check(
    generation,
    options.interval,
    options.ready,
    Some(resolver),
    options.public_key,
    options.url,
  );
}

fn start_check(
  generation: u64,
  interval: Option<Duration>,
  ready: Option<v8::Global<v8::Function>>,
  resolver: Option<v8::Global<v8::PromiseResolver>>,
  key: Option<[u8; 32]>,
  url: Option<String>,
) {
  let fetch_url = url.clone();
  yuzuko_core::spawn(
    move || match resources_dir() {
      Ok(resources) => stage_update(&resources, key, fetch_url.as_deref()),
      Err(error) => Err(error),
    },
    move |scope, result| {
      finish_check(
        scope, generation, interval, ready, resolver, key, url, result,
      );
    },
  );
}

fn finish_check(
  scope: &mut v8::PinScope,
  generation: u64,
  interval: Option<Duration>,
  ready: Option<v8::Global<v8::Function>>,
  resolver: Option<v8::Global<v8::PromiseResolver>>,
  key: Option<[u8; 32]>,
  url: Option<String>,
  result: Result<Result<Option<String>, Failure>, ()>,
) {
  let live = generation == GENERATION.load(AtomicOrdering::Relaxed);
  let outcome = match result {
    Ok(outcome) => outcome,
    Err(()) => Err(DOWNLOAD_FAILED),
  };
  match outcome {
    Ok(staged) => {
      let thrown = if live {
        staged.as_ref().and_then(|version| {
          ready
            .as_ref()
            .and_then(|ready| call_ready(scope, ready, version).err())
        })
      } else {
        None
      };
      if let Some(resolver) = resolver.as_ref() {
        let resolver = v8::Local::new(scope, resolver);
        if let Some(error) = &thrown {
          let error = v8::Local::new(scope, error);
          resolver.reject(scope, error);
        } else {
          resolver.resolve(scope, v8::undefined(scope).into());
        }
      } else if let Some(error) = &thrown {
        let error = v8::Local::new(scope, error);
        report_uncaught(scope, error);
      }
      let continue_poll = generation
        == GENERATION.load(AtomicOrdering::Relaxed)
        && (thrown.is_none() || resolver.is_none());
      if continue_poll {
        schedule_next(generation, interval, ready, key, url);
      }
    }
    Err(error) => {
      if let Some(resolver) = resolver.as_ref() {
        let resolver = v8::Local::new(scope, resolver);
        reject(scope, resolver, &error);
      } else if live {
        schedule_next(generation, interval, ready, key, url);
      }
    }
  }
}

fn schedule_next(
  generation: u64,
  interval: Option<Duration>,
  ready: Option<v8::Global<v8::Function>>,
  key: Option<[u8; 32]>,
  url: Option<String>,
) {
  let Some(interval) = interval else {
    return;
  };
  if generation != GENERATION.load(AtomicOrdering::Relaxed) {
    return;
  }
  let id = yuzuko_core::schedule_host(interval, move |_scope| {
    if generation != GENERATION.load(AtomicOrdering::Relaxed) {
      return;
    }
    start_check(generation, Some(interval), ready, None, key, url);
  });
  *lock(&TIMER) = Some(id);
}

fn call_ready(
  scope: &mut v8::PinScope,
  function: &v8::Global<v8::Function>,
  version: &str,
) -> Result<(), v8::Global<v8::Value>> {
  let captured = {
    v8::tc_scope!(let tc, scope);
    let function = v8::Local::new(tc, function);
    match v8::String::new(tc, version) {
      Some(arg) => {
        let receiver = v8::undefined(tc);
        if function.call(tc, receiver.into(), &[arg.into()]).is_some() {
          Ok(())
        } else {
          Err(caught(tc))
        }
      }
      None => Err(caught(tc)),
    }
  };
  captured
}

fn caught(
  scope: &mut v8::PinnedRef<v8::TryCatch<v8::HandleScope>>,
) -> v8::Global<v8::Value> {
  let exception = scope
    .exception()
    .unwrap_or_else(|| v8::undefined(scope).into());
  v8::Global::new(scope, exception)
}

fn report_uncaught(scope: &mut v8::PinScope, error: v8::Local<v8::Value>) {
  let global = yuzuko_core::global(scope);
  let Some(key) = v8::String::new(scope, "reportError") else {
    return;
  };
  let Some(report) = global
    .get(scope, key.into())
    .and_then(|value| v8::Local::<v8::Function>::try_from(value).ok())
  else {
    return;
  };
  let _ = report.call(scope, global.into(), &[error]);
}

fn read_options(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<Options, OptionsError> {
  if value.is_undefined() {
    return Ok(Options {
      ready: None,
      interval: None,
      public_key: None,
      url: None,
    });
  }
  if value.is_null()
    || value.is_function()
    || value.is_array()
    || !value.is_object()
  {
    return Err(OptionsError::Type(EXPECTS_OBJECT));
  }
  let Ok(object) = v8::Local::<v8::Object>::try_from(value) else {
    return Err(OptionsError::Type(EXPECTS_OBJECT));
  };
  let url_value = property(scope, object, "url")?;
  let url_value = v8::Local::new(scope, url_value);
  let url = read_feed_url(scope, url_value)?;
  let key_value = property(scope, object, "publicKey")?;
  let key_value = v8::Local::new(scope, key_value);
  let public_key = if key_value.is_undefined() {
    None
  } else if let Ok(text) = v8::Local::<v8::String>::try_from(key_value) {
    let text = text.to_rust_string_lossy(scope);
    Some(decode_public_key(&text).map_err(|()| OptionsError::Type(BAD_KEY))?)
  } else {
    return Err(OptionsError::Type(BAD_KEY));
  };
  let ready_value = property(scope, object, "onUpdateReady")?;
  let ready_value = v8::Local::new(scope, ready_value);
  let ready = if ready_value.is_undefined() {
    None
  } else {
    let Ok(function) = v8::Local::<v8::Function>::try_from(ready_value) else {
      return Err(OptionsError::Type(BAD_CALLBACK));
    };
    Some(v8::Global::new(scope, function))
  };
  let interval_value = property(scope, object, "interval")?;
  let interval_value = v8::Local::new(scope, interval_value);
  let interval = if interval_value.is_undefined() {
    None
  } else if interval_value.is_number() {
    let ms = interval_value.number_value(scope).unwrap_or(f64::NAN);
    if !ms.is_finite() || ms < 1.0 || ms > 2_147_483_647.0 {
      return Err(OptionsError::Type(BAD_INTERVAL));
    }
    Some(Duration::from_millis(ms.trunc() as u64))
  } else {
    return Err(OptionsError::Type(BAD_INTERVAL));
  };
  Ok(Options {
    ready,
    interval,
    public_key,
    url,
  })
}

fn read_feed_url(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<Option<String>, OptionsError> {
  if value.is_undefined() {
    return Ok(None);
  }
  let text = if let Ok(text) = v8::Local::<v8::String>::try_from(value) {
    text.to_rust_string_lossy(scope)
  } else if url_instance(scope, value) {
    let object = v8::Local::<v8::Object>::try_from(value)
      .map_err(|_| OptionsError::Type(BAD_URL))?;
    let href = property(scope, object, "href")?;
    let href = v8::Local::new(scope, href);
    let Ok(href) = v8::Local::<v8::String>::try_from(href) else {
      return Err(OptionsError::Type(BAD_URL));
    };
    href.to_rust_string_lossy(scope)
  } else {
    return Err(OptionsError::Type(BAD_URL));
  };
  if absolute_http(&text) {
    Ok(Some(text))
  } else {
    Err(OptionsError::Type(BAD_URL))
  }
}

fn url_instance(scope: &mut v8::PinScope, value: v8::Local<v8::Value>) -> bool {
  let global = scope.get_current_context().global(scope);
  let Some(key) = v8::String::new(scope, "URL") else {
    return false;
  };
  let Some(ctor) = global.get(scope, key.into()) else {
    return false;
  };
  let Ok(ctor) = v8::Local::<v8::Object>::try_from(ctor) else {
    return false;
  };
  value.instance_of(scope, ctor) == Some(true)
}

fn absolute_http(text: &str) -> bool {
  let Ok(url) = url::Url::parse(text) else {
    return false;
  };
  matches!(url.scheme(), "http" | "https") && url.host_str().is_some()
}

fn property(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
) -> Result<v8::Global<v8::Value>, OptionsError> {
  let captured = {
    v8::tc_scope!(let tc, scope);
    let Some(key) = v8::String::new(tc, name) else {
      return Err(OptionsError::Type(EXPECTS_OBJECT));
    };
    match object.get(tc, key.into()) {
      Some(value) if tc.exception().is_none() => Ok(v8::Global::new(tc, value)),
      _ => Err(OptionsError::Thrown(caught(tc))),
    }
  };
  captured
}

fn resources_dir() -> Result<PathBuf, Failure> {
  lock(&RESOURCES).clone().ok_or(NO_FEED)
}

fn packaged() -> bool {
  lock(&RESOURCES).is_some()
}

fn packaged_version() -> Option<String> {
  let resources = lock(&RESOURCES).clone()?;
  read_bundle(&resources)
    .ok()
    .map(|manifest| manifest.version)
}

fn reject(
  scope: &mut v8::PinScope,
  resolver: v8::Local<v8::PromiseResolver>,
  error: &Failure,
) {
  let value =
    yuzuko_core::op::new_dom_exception(scope, error.name, error.message)
      .map(v8::Local::<v8::Value>::from)
      .unwrap_or_else(|| {
        let message = v8::String::new(scope, error.message).unwrap();
        v8::Exception::type_error(scope, message)
      });
  resolver.reject(scope, value);
}

fn stage_update(
  resources: &Path,
  key: Option<[u8; 32]>,
  url: Option<&str>,
) -> Result<Option<String>, Failure> {
  let _fetch = lock(&FETCH);
  let manifest = read_bundle(resources).map_err(|()| NO_FEED)?;
  let Some(base) = url else {
    return Err(NO_FEED);
  };
  let Some(key) = key else {
    return Err(NO_ANCHOR);
  };
  if version_parts(&manifest.version).is_err() {
    return Err(BAD_PACKAGED);
  }
  let url = feed_url(&base)?;
  let body = http_get(&url, FEED_LIMIT)?;
  let feed = parse_feed(&body)?;
  let order =
    version_cmp(&feed.version, &manifest.version).map_err(|()| INVALID_FEED)?;
  if order != Ordering::Greater {
    return Ok(None);
  }
  if already_staged(resources, &feed.signature, &key) {
    return Ok(None);
  }
  let payload_url = payload_url(&base, &feed.payload)?;
  let payload = http_get(&payload_url, PAYLOAD_LIMIT)?;
  if !verify_bytes(&key, &payload, &feed.signature) {
    return Err(BAD_SIGNATURE);
  }
  let parsed = parse_payload(&payload)?;
  if parsed.version != feed.version {
    return Err(INVALID_PAYLOAD);
  }
  let order = version_cmp(&parsed.version, &manifest.version)
    .map_err(|()| INVALID_PAYLOAD)?;
  if order != Ordering::Greater {
    return Err(INVALID_PAYLOAD);
  }
  publish_stage(resources, &payload, &feed.signature, &key)?;
  Ok(Some(feed.version))
}

fn already_staged(
  resources: &Path,
  signature: &[u8; 64],
  key: &[u8; 32],
) -> bool {
  let updates = resources.join("Updates");
  let Ok(stored_signature) = read_limited(&updates.join("signature"), 64)
  else {
    return false;
  };
  if stored_signature.as_slice() != signature {
    return false;
  }
  let Ok(stored_key) = read_limited(&updates.join("public-key"), 32) else {
    return false;
  };
  stored_key.as_slice() == key && updates.join("payload").is_file()
}

pub(crate) fn version_is_ordered(text: &str) -> bool {
  version_parts(text).is_ok()
}

fn version_cmp(left: &str, right: &str) -> Result<Ordering, ()> {
  let left = version_parts(left)?;
  let right = version_parts(right)?;
  let width = left.len().max(right.len());
  for index in 0..width {
    let a = left.get(index).copied().unwrap_or(0);
    let b = right.get(index).copied().unwrap_or(0);
    if a != b {
      return Ok(a.cmp(&b));
    }
  }
  Ok(Ordering::Equal)
}

fn version_parts(text: &str) -> Result<Vec<u64>, ()> {
  if text.is_empty() {
    return Err(());
  }
  let mut parts = Vec::new();
  for part in text.split('.') {
    if part.is_empty()
      || part.len() > 20
      || !part.bytes().all(|byte| byte.is_ascii_digit())
    {
      return Err(());
    }
    parts.push(part.parse::<u64>().map_err(|_| ())?);
    if parts.len() > 32 {
      return Err(());
    }
  }
  Ok(parts)
}

pub(crate) fn decode_public_key(text: &str) -> Result<[u8; 32], ()> {
  let bytes = decode_standard(text)?;
  let bytes: [u8; 32] = bytes.try_into().map_err(|_| ())?;
  let key = VerifyingKey::from_bytes(&bytes).map_err(|_| ())?;
  if key.is_weak() {
    return Err(());
  }
  Ok(bytes)
}

fn decode_standard(text: &str) -> Result<Vec<u8>, ()> {
  if text.is_empty() || !text.len().is_multiple_of(4) {
    return Err(());
  }
  let padding = text.chars().rev().take_while(|char| *char == '=').count();
  if padding > 2 || text.trim_end_matches('=').contains('=') {
    return Err(());
  }
  if !text.bytes().all(|byte| {
    matches!(
      byte,
      b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'+' | b'/' | b'='
    )
  }) {
    return Err(());
  }
  BASE64_STANDARD.decode(text).map_err(|_| ())
}

fn relative_path_ok(path: &str) -> bool {
  if path.is_empty() || path.len() > PATH_LIMIT || path.starts_with('/') {
    return false;
  }
  if path
    .bytes()
    .any(|byte| byte < 0x20 || matches!(byte, b'\\' | b':' | b'?' | b'#' | 0))
  {
    return false;
  }
  let mut any = false;
  for segment in path.split('/') {
    if segment.is_empty() || segment == "." || segment == ".." {
      return false;
    }
    any = true;
  }
  any
}

pub(crate) fn push_relative(
  root: &Path,
  relative: &str,
) -> Result<PathBuf, ()> {
  if !relative_path_ok(relative) {
    return Err(());
  }
  let mut path = root.to_path_buf();
  for segment in relative.split('/') {
    path.push(segment);
  }
  Ok(path)
}

fn feed_url(base: &str) -> Result<String, Failure> {
  let mut url = anchored_url(base)?;
  {
    let mut segments = url.path_segments_mut().map_err(|()| INVALID_FEED)?;
    segments.pop_if_empty();
    segments.push("feed.json");
  }
  Ok(url.to_string())
}

fn payload_url(base: &str, relative: &str) -> Result<String, Failure> {
  if !relative_path_ok(relative) {
    return Err(INVALID_FEED);
  }
  let mut url = anchored_url(base)?;
  {
    let mut segments = url.path_segments_mut().map_err(|()| INVALID_FEED)?;
    segments.pop_if_empty();
    for part in relative.split('/') {
      segments.push(part);
    }
  }
  Ok(url.to_string())
}

fn anchored_url(base: &str) -> Result<url::Url, Failure> {
  let mut url = url::Url::parse(base).map_err(|_| INVALID_FEED)?;
  if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
    return Err(INVALID_FEED);
  }
  url.set_username("").map_err(|()| INVALID_FEED)?;
  url.set_password(None).map_err(|()| INVALID_FEED)?;
  url.set_query(None);
  url.set_fragment(None);
  Ok(url)
}

struct ParsedFeed {
  version: String,
  payload: String,
  signature: [u8; 64],
}

fn parse_feed(bytes: &[u8]) -> Result<ParsedFeed, Failure> {
  let feed: FeedJson =
    serde_json::from_slice(bytes).map_err(|_| INVALID_FEED)?;
  if version_parts(&feed.version).is_err() || !relative_path_ok(&feed.payload) {
    return Err(INVALID_FEED);
  }
  let signature = decode_standard(&feed.signature).map_err(|_| INVALID_FEED)?;
  let signature: [u8; 64] = signature.try_into().map_err(|_| INVALID_FEED)?;
  Ok(ParsedFeed {
    version: feed.version,
    payload: feed.payload,
    signature,
  })
}

fn parse_payload(bytes: &[u8]) -> Result<Payload, Failure> {
  let mut cursor = 0;
  if read_line(bytes, &mut cursor)? != "YUZUKO1" {
    return Err(INVALID_PAYLOAD);
  }
  let version = read_line(bytes, &mut cursor)?;
  if version_parts(&version).is_err() {
    return Err(INVALID_PAYLOAD);
  }
  let entry = read_line(bytes, &mut cursor)?;
  if !relative_path_ok(&entry) {
    return Err(INVALID_PAYLOAD);
  }
  let count = parse_usize(&read_line(bytes, &mut cursor)?)?;
  if count > FILE_LIMIT {
    return Err(INVALID_PAYLOAD);
  }
  let mut files = Vec::with_capacity(count);
  let mut seen = BTreeSet::new();
  for _ in 0..count {
    let path = read_line(bytes, &mut cursor)?;
    if !relative_path_ok(&path) || !seen.insert(path.clone()) {
      return Err(INVALID_PAYLOAD);
    }
    let len = parse_usize(&read_line(bytes, &mut cursor)?)?;
    let end = cursor.checked_add(len).ok_or(INVALID_PAYLOAD)?;
    if end > bytes.len() {
      return Err(INVALID_PAYLOAD);
    }
    let data = bytes[cursor..end].to_vec();
    cursor = end;
    files.push(PayloadFile { path, bytes: data });
  }
  if cursor != bytes.len() || !files.iter().any(|file| file.path == entry) {
    return Err(INVALID_PAYLOAD);
  }
  Ok(Payload {
    version,
    entry,
    files,
  })
}

fn read_line(bytes: &[u8], cursor: &mut usize) -> Result<String, Failure> {
  let start = *cursor;
  let limit = start.saturating_add(LINE_LIMIT).min(bytes.len());
  let mut index = start;
  while index < limit {
    if bytes[index] == b'\n' {
      let line = &bytes[start..index];
      if line.contains(&b'\r') {
        return Err(INVALID_PAYLOAD);
      }
      let text = std::str::from_utf8(line).map_err(|_| INVALID_PAYLOAD)?;
      *cursor = index + 1;
      return Ok(text.to_string());
    }
    index += 1;
  }
  Err(INVALID_PAYLOAD)
}

fn parse_usize(text: &str) -> Result<usize, Failure> {
  if text.is_empty()
    || text.len() > 20
    || !text.bytes().all(|byte| byte.is_ascii_digit())
  {
    return Err(INVALID_PAYLOAD);
  }
  let value: u64 = text.parse().map_err(|_| INVALID_PAYLOAD)?;
  usize::try_from(value).map_err(|_| INVALID_PAYLOAD)
}

fn verify_bytes(key: &[u8; 32], payload: &[u8], signature: &[u8; 64]) -> bool {
  let Ok(key) = VerifyingKey::from_bytes(key) else {
    return false;
  };
  if key.is_weak() {
    return false;
  }
  let signature = Signature::from_bytes(signature);
  key.verify_strict(payload, &signature).is_ok()
}

fn staged_key(updates: &Path) -> Result<[u8; 32], ()> {
  let bytes = read_limited(&updates.join("public-key"), 32)?;
  let bytes: [u8; 32] = bytes.try_into().map_err(|_| ())?;
  decode_public_key_bytes(&bytes)?;
  Ok(bytes)
}

fn decode_public_key_bytes(bytes: &[u8; 32]) -> Result<(), ()> {
  let key = VerifyingKey::from_bytes(bytes).map_err(|_| ())?;
  if key.is_weak() { Err(()) } else { Ok(()) }
}

pub(crate) fn read_bundle(resources: &Path) -> Result<BundleMeta, ()> {
  let text =
    fs::read_to_string(resources.join("yuzuko.bundle.json")).map_err(|_| ())?;
  let value: serde_json::Value = serde_json::from_str(&text).map_err(|_| ())?;
  let name = value
    .get("name")
    .and_then(|item| item.as_str())
    .ok_or(())?
    .to_string();
  let version = value
    .get("version")
    .and_then(|item| item.as_str())
    .ok_or(())?
    .to_string();
  let entry = value
    .get("entry")
    .and_then(|item| item.as_str())
    .ok_or(())?
    .to_string();
  let identifier = value
    .get("identifier")
    .and_then(|item| item.as_str())
    .filter(|id| !id.is_empty())
    .map(str::to_string);
  let permissions = match value.get("permissions") {
    None => None,
    Some(item) if item.is_object() => Some(item.clone()),
    Some(_) => return Err(()),
  };
  Ok(BundleMeta {
    name,
    identifier,
    version,
    entry,
    permissions,
  })
}

pub(crate) fn apply_pending(resources: &Path) {
  heal_app_dir(resources);
  let _ = remove_any(&resources.join(".app.updating"));
  if try_apply(resources).is_err() {
    delete_stage(resources);
  }
}

fn try_apply(resources: &Path) -> Result<(), ()> {
  let updates = resources.join("Updates");
  let payload_path = updates.join("payload");
  let signature_path = updates.join("signature");
  if !payload_path.is_file() || !signature_path.is_file() {
    return Ok(());
  }
  let key = staged_key(&updates)?;
  let payload = read_limited(&payload_path, PAYLOAD_LIMIT)?;
  let signature = read_limited(&signature_path, 64)?;
  let signature: [u8; 64] = signature.try_into().map_err(|_| ())?;
  if !verify_bytes(&key, &payload, &signature) {
    return Err(());
  }
  let parsed = parse_payload(&payload).map_err(|_| ())?;
  let manifest = read_bundle(resources)?;
  if version_parts(&manifest.version).is_err() {
    return Err(());
  }
  if version_cmp(&parsed.version, &manifest.version)? != Ordering::Greater {
    return Err(());
  }
  replace_app(resources, &parsed.files)?;
  if write_manifest_fields(resources, &parsed.version, &parsed.entry).is_err() {
    rollback_app(resources);
    return Err(());
  }
  let _ = remove_any(&resources.join(".app.previous"));
  delete_stage(resources);
  Ok(())
}

fn publish_stage(
  resources: &Path,
  payload: &[u8],
  signature: &[u8; 64],
  key: &[u8; 32],
) -> Result<(), Failure> {
  let updates = resources.join("Updates");
  fs::create_dir_all(&updates).map_err(|_| INVALID_PAYLOAD)?;
  let payload_partial = updates.join("payload.partial");
  let signature_partial = updates.join("signature.partial");
  let key_partial = updates.join("public-key.partial");
  if fs::write(&payload_partial, payload).is_err()
    || fs::write(&signature_partial, signature).is_err()
    || fs::write(&key_partial, key).is_err()
  {
    let _ = fs::remove_file(&payload_partial);
    let _ = fs::remove_file(&signature_partial);
    let _ = fs::remove_file(&key_partial);
    return Err(INVALID_PAYLOAD);
  }
  let payload_final = updates.join("payload");
  let signature_final = updates.join("signature");
  let key_final = updates.join("public-key");
  let _ = fs::remove_file(&payload_final);
  if fs::rename(&payload_partial, &payload_final).is_err() {
    let _ = fs::remove_file(&payload_partial);
    let _ = fs::remove_file(&signature_partial);
    let _ = fs::remove_file(&key_partial);
    return Err(INVALID_PAYLOAD);
  }
  let _ = fs::remove_file(&signature_final);
  if fs::rename(&signature_partial, &signature_final).is_err() {
    let _ = fs::remove_file(&payload_final);
    let _ = fs::remove_file(&signature_partial);
    let _ = fs::remove_file(&key_partial);
    return Err(INVALID_PAYLOAD);
  }
  let _ = fs::remove_file(&key_final);
  if fs::rename(&key_partial, &key_final).is_err() {
    let _ = fs::remove_file(&payload_final);
    let _ = fs::remove_file(&signature_final);
    let _ = fs::remove_file(&key_partial);
    return Err(INVALID_PAYLOAD);
  }
  Ok(())
}

fn delete_stage(resources: &Path) {
  let updates = resources.join("Updates");
  for name in [
    "payload",
    "signature",
    "public-key",
    "payload.partial",
    "signature.partial",
    "public-key.partial",
  ] {
    let _ = fs::remove_file(updates.join(name));
  }
  let _ = fs::remove_dir(&updates);
}

fn replace_app(resources: &Path, files: &[PayloadFile]) -> Result<(), ()> {
  heal_app_dir(resources);
  let app = resources.join("app");
  let staging = resources.join(".app.updating");
  let backup = resources.join(".app.previous");
  remove_any(&staging)?;
  if backup.exists() {
    remove_any(&backup)?;
  }
  if let Err(error) = write_tree(&staging, files) {
    let _ = remove_any(&staging);
    return Err(error);
  }
  if app.exists() {
    fs::rename(&app, &backup).map_err(|_| ())?;
  }
  if fs::rename(&staging, &app).is_err() {
    let _ = fs::rename(&backup, &app);
    let _ = remove_any(&staging);
    return Err(());
  }
  Ok(())
}

fn write_tree(root: &Path, files: &[PayloadFile]) -> Result<(), ()> {
  fs::create_dir_all(root).map_err(|_| ())?;
  for file in files {
    let dest = push_relative(root, &file.path)?;
    if let Some(parent) = dest.parent() {
      fs::create_dir_all(parent).map_err(|_| ())?;
    }
    fs::write(&dest, &file.bytes).map_err(|_| ())?;
  }
  Ok(())
}

fn write_manifest_fields(
  resources: &Path,
  version: &str,
  entry: &str,
) -> Result<(), ()> {
  let path = resources.join("yuzuko.bundle.json");
  let text = fs::read_to_string(&path).map_err(|_| ())?;
  let mut value: serde_json::Value =
    serde_json::from_str(&text).map_err(|_| ())?;
  let object = value.as_object_mut().ok_or(())?;
  object.insert(
    "version".to_string(),
    serde_json::Value::String(version.to_string()),
  );
  object.insert(
    "entry".to_string(),
    serde_json::Value::String(entry.to_string()),
  );
  let next = serde_json::to_string_pretty(&value).map_err(|_| ())?;
  let partial = resources.join("yuzuko.bundle.json.partial");
  fs::write(&partial, next).map_err(|_| ())?;
  fs::rename(&partial, &path).map_err(|_| ())?;
  Ok(())
}

fn heal_app_dir(resources: &Path) {
  let app = resources.join("app");
  let backup = resources.join(".app.previous");
  if !app.exists() && backup.exists() {
    let _ = fs::rename(&backup, &app);
  }
}

fn rollback_app(resources: &Path) {
  let app = resources.join("app");
  let backup = resources.join(".app.previous");
  if !backup.exists() {
    return;
  }
  let _ = remove_any(&app);
  let _ = fs::rename(&backup, &app);
}

fn remove_any(path: &Path) -> Result<(), ()> {
  let Ok(meta) = fs::symlink_metadata(path) else {
    return Ok(());
  };
  let result = if meta.file_type().is_dir() {
    fs::remove_dir_all(path)
  } else {
    fs::remove_file(path)
  };
  result.map_err(|_| ())
}

fn read_limited(path: &Path, max: usize) -> Result<Vec<u8>, ()> {
  let file = File::open(path).map_err(|_| ())?;
  let max_len = u64::try_from(max).map_err(|_| ())?;
  if file.metadata().map_err(|_| ())?.len() > max_len {
    return Err(());
  }
  let mut bytes = Vec::new();
  file
    .take(max_len.saturating_add(1))
    .read_to_end(&mut bytes)
    .map_err(|_| ())?;
  if bytes.len() > max {
    return Err(());
  }
  Ok(bytes)
}

fn http_get(url_text: &str, max: usize) -> Result<Vec<u8>, Failure> {
  let url = url::Url::parse(url_text).map_err(|_| DOWNLOAD_FAILED)?;
  let host = url.host_str().ok_or(DOWNLOAD_FAILED)?.to_string();
  let port = url.port_or_known_default().ok_or(DOWNLOAD_FAILED)?;
  if yuzuko_core::check_net(&host, port).is_err() {
    return Err(NET_DENIED);
  }
  let stream = tcp_connect(&host, port)?;
  match url.scheme() {
    "http" => exchange(stream, &url, max),
    "https" => {
      let stream = tls_handshake(stream, &host)?;
      exchange(stream, &url, max)
    }
    _ => Err(DOWNLOAD_FAILED),
  }
}

/// Connects only to resolved addresses the net policy allows.
///
/// The feed names the payload URL, so its host can resolve to a loopback or
/// private address. Each such address is checked again, as `fetch` does.
fn tcp_connect(host: &str, port: u16) -> Result<TcpStream, Failure> {
  let addresses: Vec<_> =
    yuzuko_core::with_env(|| (host, port).to_socket_addrs())
      .map_err(|_| DOWNLOAD_FAILED)?
      .collect();
  let addresses = yuzuko_core::retain_resolved_addresses(addresses)
    .map_err(|_| NET_DENIED)?;
  for address in addresses {
    if let Ok(stream) = TcpStream::connect_timeout(&address, TIMEOUT) {
      let _ = stream.set_read_timeout(Some(TIMEOUT));
      let _ = stream.set_write_timeout(Some(TIMEOUT));
      return Ok(stream);
    }
  }
  Err(DOWNLOAD_FAILED)
}

fn tls_handshake(
  stream: TcpStream,
  host: &str,
) -> Result<rustls::StreamOwned<rustls::ClientConnection, TcpStream>, Failure> {
  let name = rustls::pki_types::ServerName::try_from(host)
    .map_err(|_| DOWNLOAD_FAILED)?
    .to_owned();
  let connection = rustls::ClientConnection::new(tls_config()?, name)
    .map_err(|_| DOWNLOAD_FAILED)?;
  Ok(rustls::StreamOwned::new(connection, stream))
}

fn tls_config() -> Result<Arc<rustls::ClientConfig>, Failure> {
  static CONFIG: OnceLock<Arc<rustls::ClientConfig>> = OnceLock::new();
  if let Some(config) = CONFIG.get() {
    return Ok(Arc::clone(config));
  }
  let roots = rustls::RootCertStore {
    roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
  };
  let provider = Arc::new(rustls::crypto::ring::default_provider());
  let config = rustls::ClientConfig::builder_with_provider(provider)
    .with_safe_default_protocol_versions()
    .map_err(|_| DOWNLOAD_FAILED)?
    .with_root_certificates(roots)
    .with_no_client_auth();
  let config = Arc::new(config);
  Ok(Arc::clone(CONFIG.get_or_init(|| config)))
}

fn exchange(
  mut stream: impl Read + Write,
  url: &url::Url,
  max: usize,
) -> Result<Vec<u8>, Failure> {
  let host = host_header(url)?;
  let target = request_target(url);
  let request = format!(
    "GET {target} HTTP/1.1\r\nHost: {host}\r\nUser-Agent: Yuzuko\r\nAccept: */*\r\nConnection: close\r\n\r\n"
  );
  stream
    .write_all(request.as_bytes())
    .map_err(|_| DOWNLOAD_FAILED)?;
  stream.flush().map_err(|_| DOWNLOAD_FAILED)?;
  read_response(&mut stream, max)
}

fn host_header(url: &url::Url) -> Result<String, Failure> {
  let host = match url.host() {
    Some(url::Host::Domain(domain)) => domain.to_string(),
    Some(url::Host::Ipv4(address)) => address.to_string(),
    Some(url::Host::Ipv6(address)) => format!("[{address}]"),
    None => return Err(DOWNLOAD_FAILED),
  };
  Ok(match url.port() {
    Some(port) => format!("{host}:{port}"),
    None => host,
  })
}

fn request_target(url: &url::Url) -> String {
  let path = if url.path().is_empty() {
    "/"
  } else {
    url.path()
  };
  match url.query() {
    Some(query) if !query.is_empty() => format!("{path}?{query}"),
    _ => path.to_string(),
  }
}

fn read_response(
  stream: &mut impl Read,
  max: usize,
) -> Result<Vec<u8>, Failure> {
  let mut raw = Vec::new();
  let mut tmp = [0u8; 8192];
  let header_at = loop {
    if raw.len() > HEADER_LIMIT {
      return Err(DOWNLOAD_FAILED);
    }
    let count = read_some(stream, &mut tmp).map_err(|()| DOWNLOAD_FAILED)?;
    if count == 0 {
      return Err(DOWNLOAD_FAILED);
    }
    raw.extend_from_slice(&tmp[..count]);
    if let Some(position) =
      raw.windows(4).position(|window| window == b"\r\n\r\n")
    {
      break position;
    }
  };
  let header =
    std::str::from_utf8(&raw[..header_at]).map_err(|_| DOWNLOAD_FAILED)?;
  let mut body = raw[header_at + 4..].to_vec();
  let mut lines = header.split("\r\n");
  let status = lines.next().ok_or(DOWNLOAD_FAILED)?;
  let code = status.split_whitespace().nth(1).ok_or(DOWNLOAD_FAILED)?;
  if code != "200" {
    return Err(DOWNLOAD_FAILED);
  }
  let mut content_length = None;
  for line in lines {
    if line.is_empty() {
      continue;
    }
    let Some((name, value)) = line.split_once(':') else {
      return Err(DOWNLOAD_FAILED);
    };
    let value = value.trim();
    if name.eq_ignore_ascii_case("content-length") {
      if content_length.is_some() {
        return Err(DOWNLOAD_FAILED);
      }
      let len: u64 = value.parse().map_err(|_| DOWNLOAD_FAILED)?;
      content_length = Some(len);
    } else if name.eq_ignore_ascii_case("transfer-encoding")
      && value.to_ascii_lowercase().contains("chunked")
    {
      return Err(DOWNLOAD_FAILED);
    }
  }
  if let Some(len) = content_length {
    let len = usize::try_from(len).map_err(|_| DOWNLOAD_FAILED)?;
    if len > max {
      return Err(DOWNLOAD_FAILED);
    }
    while body.len() < len {
      let count = read_some(stream, &mut tmp).map_err(|()| DOWNLOAD_FAILED)?;
      if count == 0 {
        return Err(TRUNCATED);
      }
      let need = len - body.len();
      body.extend_from_slice(&tmp[..count.min(need)]);
    }
    body.truncate(len);
    return Ok(body);
  }
  loop {
    if body.len() > max {
      return Err(DOWNLOAD_FAILED);
    }
    let count = read_some(stream, &mut tmp).map_err(|()| DOWNLOAD_FAILED)?;
    if count == 0 {
      break;
    }
    body.extend_from_slice(&tmp[..count]);
  }
  if body.len() > max {
    return Err(DOWNLOAD_FAILED);
  }
  Ok(body)
}

fn read_some(stream: &mut impl Read, buf: &mut [u8]) -> Result<usize, ()> {
  loop {
    match stream.read(buf) {
      Ok(count) => return Ok(count),
      Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {
        continue;
      }
      Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => {
        return Ok(0);
      }
      Err(_) => return Err(()),
    }
  }
}

#[cfg(test)]
mod tests {
  use std::cmp::Ordering;
  use std::fs;
  use std::io::{Read, Write};
  use std::net::TcpListener;
  use std::path::{Path, PathBuf};
  use std::sync::atomic::{
    AtomicBool, AtomicUsize, Ordering as AtomicOrdering,
  };
  use std::sync::{Arc, Mutex};
  use std::time::Duration;

  use super::*;
  use base64::Engine;
  use base64::prelude::BASE64_STANDARD;
  use ed25519_dalek::Signer;
  use ed25519_dalek::SigningKey;

  fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
      "yuzuko-update-{name}-{}-{}",
      std::process::id(),
      std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
  }

  fn signing() -> SigningKey {
    SigningKey::from_bytes(&[7u8; 32])
  }

  fn encode_payload(
    version: &str,
    entry: &str,
    files: &[(&str, &[u8])],
  ) -> Vec<u8> {
    let mut out = Vec::new();
    for line in ["YUZUKO1", version, entry, &files.len().to_string()] {
      out.extend_from_slice(line.as_bytes());
      out.push(b'\n');
    }
    for (path, bytes) in files {
      out.extend_from_slice(path.as_bytes());
      out.push(b'\n');
      out.extend_from_slice(bytes.len().to_string().as_bytes());
      out.push(b'\n');
      out.extend_from_slice(bytes);
    }
    out
  }

  #[test]
  fn version_order_matches_the_numeric_rules() {
    assert_eq!(version_cmp("1.0", "1.0.0").unwrap(), Ordering::Equal);
    assert_eq!(version_cmp("1.10", "1.9").unwrap(), Ordering::Greater);
    assert_eq!(version_cmp("1.0.1", "1.0").unwrap(), Ordering::Greater);
    assert_eq!(version_cmp("01.2", "1.2.0").unwrap(), Ordering::Equal);
    assert_eq!(version_cmp("2", "1.9.9").unwrap(), Ordering::Greater);
    assert!(version_cmp("1..0", "1.0").is_err());
    assert!(version_cmp("1.a", "1.0").is_err());
    assert!(version_cmp("-1", "1").is_err());
    assert!(version_cmp("", "0").is_err());
    assert!(!version_is_ordered("1.0.0-beta"));
    assert!(version_is_ordered("0.0.0"));
  }

  #[test]
  fn standard_base64_rejects_other_alphabets() {
    assert_eq!(decode_standard("YQ==").unwrap(), b"a");
    assert!(decode_standard("YQ").is_err());
    assert!(decode_standard("YQ===").is_err());
    assert!(decode_standard("_-wgVQA=").is_err());
    assert!(decode_standard("YQ==\n").is_err());
    assert!(decode_public_key(&BASE64_STANDARD.encode([0u8; 32])).is_err());
    let public = signing().verifying_key().to_bytes();
    assert_eq!(
      decode_public_key(&BASE64_STANDARD.encode(public)).unwrap(),
      public
    );
  }

  #[test]
  fn feed_and_payload_urls_stay_under_the_base() {
    assert_eq!(
      feed_url("https://releases.example.com/my-game").unwrap(),
      "https://releases.example.com/my-game/feed.json"
    );
    assert_eq!(
      feed_url("http://127.0.0.1:9/game/").unwrap(),
      "http://127.0.0.1:9/game/feed.json"
    );
    assert_eq!(
      feed_url("http://127.0.0.1:9/game?x=1#frag").unwrap(),
      "http://127.0.0.1:9/game/feed.json"
    );
    assert_eq!(
      payload_url("http://127.0.0.1:9/game", "builds/1.1.0.yuzuko").unwrap(),
      "http://127.0.0.1:9/game/builds/1.1.0.yuzuko"
    );
    assert!(payload_url("http://127.0.0.1:9/game", "../x").is_err());
    assert!(payload_url("http://127.0.0.1:9/game", "/abs").is_err());
    assert!(payload_url("http://127.0.0.1:9/game", "https://evil/x").is_err());
  }

  #[test]
  fn absolute_http_accepts_only_a_hosted_http_url() {
    assert!(absolute_http("https://releases.example.com/my-game"));
    assert!(absolute_http("http://127.0.0.1:9/game?x=1#frag"));
    assert!(absolute_http("http://user:pass@127.0.0.1:9/game"));
    assert!(!absolute_http("file:///tmp"));
    assert!(!absolute_http("https://"));
    assert!(!absolute_http("/game"));
    assert!(!absolute_http(""));
  }

  #[test]
  fn payload_framing_rejects_a_path_that_escapes() {
    let good =
      encode_payload("1.1.0", "src/main.ts", &[("src/main.ts", b"hi")]);
    let parsed = parse_payload(&good).unwrap();
    assert_eq!(parsed.version, "1.1.0");
    assert_eq!(parsed.files.len(), 1);
    let escaped = encode_payload(
      "1.1.0",
      "../yuzuko.update-key",
      &[("../yuzuko.update-key", b"no")],
    );
    assert!(parse_payload(&escaped).is_err());
    let mut extra = good.clone();
    extra.push(b'x');
    assert!(parse_payload(&extra).is_err());
  }

  #[test]
  fn signature_uses_the_passed_key() {
    let key = signing();
    let public = key.verifying_key().to_bytes();
    let payload = encode_payload("1.2.0", "a.js", &[("a.js", b"next")]);
    let signature = Signer::try_sign(&key, &payload).unwrap().to_bytes();
    assert!(verify_bytes(&public, &payload, &signature));
    let mut bad = signature;
    bad[0] ^= 0xff;
    assert!(!verify_bytes(&public, &payload, &bad));
  }

  fn stage_files(
    dir: &Path,
    payload: &[u8],
    signature: &[u8],
    key: Option<&[u8]>,
  ) {
    fs::create_dir_all(dir.join("Updates")).unwrap();
    fs::write(dir.join("Updates/payload"), payload).unwrap();
    fs::write(dir.join("Updates/signature"), signature).unwrap();
    let key_path = dir.join("Updates/public-key");
    match key {
      Some(key) => fs::write(key_path, key).unwrap(),
      None => {
        let _ = fs::remove_file(key_path);
      }
    }
  }

  #[test]
  fn apply_keeps_the_previous_tree_when_the_stage_is_bad() {
    let dir = temp_dir("apply-bad");
    let key = signing().verifying_key().to_bytes();
    // A leftover package file is not the trust anchor.
    fs::write(dir.join("yuzuko.update-key"), key).unwrap();
    fs::write(
      dir.join("yuzuko.bundle.json"),
      r#"{"name":"Game","version":"1.0.0","entry":"src/main.ts"}"#,
    )
    .unwrap();
    fs::create_dir_all(dir.join("app/src")).unwrap();
    fs::write(dir.join("app/src/main.ts"), "token-old\n").unwrap();
    let payload = encode_payload(
      "1.1.0",
      "src/main.ts",
      &[("src/main.ts", b"token-new\n")],
    );
    let signature = Signer::try_sign(&signing(), &payload).unwrap().to_bytes();
    stage_files(&dir, &payload, &[9u8; 64], Some(&key));
    apply_pending(&dir);
    assert_eq!(
      fs::read(dir.join("app/src/main.ts")).unwrap(),
      b"token-old\n"
    );
    assert!(!dir.join("Updates/payload").exists());
    assert!(!dir.join("Updates/public-key").exists());

    let older =
      encode_payload("0.9.0", "src/main.ts", &[("src/main.ts", b"older\n")]);
    let older_sig = Signer::try_sign(&signing(), &older).unwrap().to_bytes();
    stage_files(&dir, &older, &older_sig, Some(&key));
    apply_pending(&dir);
    assert_eq!(
      fs::read(dir.join("app/src/main.ts")).unwrap(),
      b"token-old\n"
    );
    assert!(!dir.join("Updates/payload").exists());

    stage_files(&dir, &payload, &signature, None);
    apply_pending(&dir);
    assert_eq!(
      fs::read(dir.join("app/src/main.ts")).unwrap(),
      b"token-old\n"
    );
    assert!(!dir.join("Updates/payload").exists());
    assert_eq!(fs::read(dir.join("yuzuko.update-key")).unwrap(), key);

    let other = SigningKey::from_bytes(&[9u8; 32])
      .verifying_key()
      .to_bytes();
    stage_files(&dir, &payload, &signature, Some(&other));
    apply_pending(&dir);
    assert_eq!(
      fs::read(dir.join("app/src/main.ts")).unwrap(),
      b"token-old\n"
    );

    stage_files(&dir, &payload, &signature, Some(&key));
    apply_pending(&dir);
    assert_eq!(
      fs::read(dir.join("app/src/main.ts")).unwrap(),
      b"token-new\n"
    );
    assert!(!dir.join("app/src/old.js").exists());
    let manifest = fs::read_to_string(dir.join("yuzuko.bundle.json")).unwrap();
    assert!(manifest.contains("1.1.0"), "{manifest}");
    assert!(manifest.contains("src/main.ts"), "{manifest}");
    assert!(!dir.join("Updates/payload").exists());
    assert!(!dir.join("Updates/public-key").exists());
    assert_eq!(fs::read(dir.join("yuzuko.update-key")).unwrap(), key);
  }

  #[test]
  fn directory_bundles_omit_a_trust_anchor_file() {
    let dir = temp_dir("anchor-layout");
    fs::write(dir.join("main.ts"), "console.log(\"anchor\");\n").unwrap();
    fs::write(dir.join("runtime"), b"not-a-real-binary").unwrap();
    fs::write(
      dir.join("yuzuko.json"),
      r#"{
          "name": "Anchor",
          "version": "1.2.0"
        }"#,
    )
    .unwrap();
    let config =
      crate::bundle::config::load_package(&dir.join("main.ts"), None).unwrap();
    for (target, output) in [
      (crate::bundle::package::Target::Linux, dir.join("linux-app")),
      (
        crate::bundle::package::Target::Windows,
        dir.join("windows-app"),
      ),
    ] {
      crate::bundle::package::package(
        &config,
        target,
        &dir.join("runtime"),
        &output,
      )
      .unwrap();
      assert!(!output.join("yuzuko.update-key").exists());
      assert!(output.join("yuzuko.bundle.json").is_file());
      assert!(output.join("app/main.ts").is_file());
    }
    assert!(output_absent(
      &dir,
      r#"{"name":"Anchor","version":"1.0.0-beta"}"#
    ));
  }

  fn output_absent(dir: &Path, config: &str) -> bool {
    fs::write(dir.join("yuzuko.json"), config).unwrap();
    let missing = dir.join("should-not-exist");
    let error = crate::bundle::config::load_package(&dir.join("main.ts"), None)
      .unwrap_err();
    !missing.exists() && !error.to_string().is_empty()
  }

  #[test]
  fn package_rejects_a_bad_version_and_a_bad_public_key() {
    let dir = temp_dir("reject-config");
    fs::write(dir.join("main.ts"), "console.log(1);\n").unwrap();
    fs::write(
      dir.join("yuzuko.json"),
      "{\"name\":\"Bad\",\"version\":\"1.0.0-beta\"}\n",
    )
    .unwrap();
    let error = crate::bundle::config::load_package(&dir.join("main.ts"), None)
      .unwrap_err();
    assert!(error.to_string().contains("dot-separated"), "{error}");
    fs::write(
      dir.join("yuzuko.json"),
      "{\"name\":\"Bad\",\"release\":{\"baseUrl\":\"https://example.invalid/app\"}}\n",
    )
    .unwrap();
    let error = crate::bundle::config::load_package(&dir.join("main.ts"), None)
      .unwrap_err();
    assert!(error.to_string().contains("unknown field"), "{error}");
    assert!(error.to_string().contains("release"), "{error}");
  }

  struct Serve {
    port: u16,
    hits: Arc<AtomicUsize>,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
  }

  impl Drop for Serve {
    fn drop(&mut self) {
      self.stop.store(true, AtomicOrdering::SeqCst);
      let _ = TcpStream::connect(("127.0.0.1", self.port));
      if let Some(thread) = self.thread.take() {
        let _ = thread.join();
      }
    }
  }

  fn serve(handler: impl Fn(&mut TcpStream, &str) + Send + 'static) -> Serve {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let hits = Arc::new(AtomicUsize::new(0));
    let stop = Arc::new(AtomicBool::new(false));
    let hits_thread = Arc::clone(&hits);
    let stop_thread = Arc::clone(&stop);
    let thread = std::thread::spawn(move || {
      loop {
        if stop_thread.load(AtomicOrdering::SeqCst) {
          break;
        }
        let Ok((mut stream, _)) = listener.accept() else {
          break;
        };
        if stop_thread.load(AtomicOrdering::SeqCst) {
          break;
        }
        let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
        let mut buf = Vec::new();
        let mut tmp = [0u8; 1024];
        loop {
          match stream.read(&mut tmp) {
            Ok(0) => break,
            Ok(count) => {
              buf.extend_from_slice(&tmp[..count]);
              if buf.windows(4).any(|window| window == b"\r\n\r\n") {
                break;
              }
            }
            Err(_) => break,
          }
          if buf.len() > 8192 {
            break;
          }
        }
        let text = String::from_utf8_lossy(&buf);
        let path = text.split_whitespace().nth(1).unwrap_or("").to_string();
        if !path.is_empty() {
          hits_thread.fetch_add(1, AtomicOrdering::SeqCst);
        }
        handler(&mut stream, &path);
      }
    });
    Serve {
      port,
      hits,
      stop,
      thread: Some(thread),
    }
  }

  fn ok_response(body: &[u8]) -> Vec<u8> {
    let mut response = format!(
      "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
      body.len()
    )
    .into_bytes();
    response.extend_from_slice(body);
    response
  }

  #[test]
  fn http_get_rejects_a_bad_status_a_short_body_and_chunking() {
    let server = serve(|stream, path| {
      let response = match path {
        "/ok" => ok_response(b"hello"),
        "/short" => b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\nConnection: close\r\n\r\nhi".to_vec(),
        "/status" => b"HTTP/1.1 500 No\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec(),
        "/chunked" => b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n5\r\nhello\r\n0\r\n\r\n".to_vec(),
        "/drop" => {
          let _ = stream.shutdown(std::net::Shutdown::Both);
          return;
        }
        _ => ok_response(b""),
      };
      let _ = stream.write_all(&response);
    });
    let base = format!("http://127.0.0.1:{}", server.port);
    assert_eq!(http_get(&format!("{base}/ok"), 100).unwrap(), b"hello");
    assert_eq!(
      http_get(&format!("{base}/short"), 100).unwrap_err(),
      TRUNCATED
    );
    assert_eq!(
      http_get(&format!("{base}/status"), 100).unwrap_err(),
      DOWNLOAD_FAILED
    );
    assert_eq!(
      http_get(&format!("{base}/chunked"), 100).unwrap_err(),
      DOWNLOAD_FAILED
    );
    assert_eq!(
      http_get(&format!("{base}/drop"), 100).unwrap_err(),
      DOWNLOAD_FAILED
    );
  }

  #[test]
  fn https_to_a_plain_socket_fails_closed() {
    let server = serve(|stream, _path| {
      let _ = stream.shutdown(std::net::Shutdown::Both);
    });
    let error =
      http_get(&format!("https://127.0.0.1:{}/feed.json", server.port), 100)
        .unwrap_err();
    assert_eq!(error, DOWNLOAD_FAILED);
  }

  #[test]
  fn check_does_not_connect_without_a_feed_or_a_key() {
    let hits = Arc::new(AtomicUsize::new(0));
    let seen = Arc::clone(&hits);
    let server = serve(move |stream, path| {
      seen.fetch_add(1, AtomicOrdering::SeqCst);
      let body = if path.ends_with("/feed.json") {
        br#"{"version":"9.0.0","payload":"a.js","signature":"aaaa"}"#.to_vec()
      } else {
        b"no".to_vec()
      };
      let _ = stream.write_all(&ok_response(&body));
    });
    let dir = temp_dir("no-connect");
    let key = signing().verifying_key().to_bytes();
    fs::write(
      dir.join("yuzuko.bundle.json"),
      r#"{"name":"Game","version":"1.0.0","entry":"main.ts"}"#,
    )
    .unwrap();
    let error = stage_update(&dir, Some(key), None).unwrap_err();
    assert_eq!(error, NO_FEED);
    assert_eq!(server.hits.load(AtomicOrdering::SeqCst), 0);

    let error = stage_update(
      &dir,
      None,
      Some(&format!("http://127.0.0.1:{}/game", server.port)),
    )
    .unwrap_err();
    assert_eq!(error, NO_ANCHOR);
    assert_eq!(server.hits.load(AtomicOrdering::SeqCst), 0);
    assert_eq!(hits.load(AtomicOrdering::SeqCst), 0);
    let _ = server;
  }

  #[test]
  fn download_stages_only_a_verified_newer_payload() {
    let key = signing();
    let public = key.verifying_key().to_bytes();
    let payload = encode_payload(
      "1.1.0",
      "src/main.ts",
      &[("src/main.ts", b"token-new\n")],
    );
    let signature = Signer::try_sign(&key, &payload).unwrap().to_bytes();
    let signature_b64 = BASE64_STANDARD.encode(signature);
    let mode = Arc::new(Mutex::new("older"));
    let mode_thread = Arc::clone(&mode);
    let payload_thread = payload.clone();
    let server = serve(move |stream, path| {
      let mode = mode_thread.lock().unwrap().to_string();
      if mode == "drop" {
        let _ = stream.shutdown(std::net::Shutdown::Both);
        return;
      }
      if path.ends_with("/feed.json") {
        let body = match mode.as_str() {
          "older" => format!(
            r#"{{"version":"0.9.0","payload":"builds/app.yuzuko","signature":"{signature_b64}"}}"#
          ),
          "missing" => {
            r#"{"version":"9.0.0","payload":"builds/app.yuzuko"}"#.to_string()
          }
          "bad" => format!(
            r#"{{"version":"1.1.0","payload":"builds/app.yuzuko","signature":"{}"}}"#,
            BASE64_STANDARD.encode([9u8; 64])
          ),
          "truncate" | "newer" => format!(
            r#"{{"version":"1.1.0","payload":"builds/app.yuzuko","signature":"{signature_b64}"}}"#
          ),
          _ => "{}".to_string(),
        };
        let _ = stream.write_all(&ok_response(body.as_bytes()));
        return;
      }
      if mode == "truncate" {
        let mut response = format!(
          "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
          payload_thread.len() + 40
        )
        .into_bytes();
        response.extend_from_slice(&payload_thread);
        let _ = stream.write_all(&response);
        return;
      }
      let _ = stream.write_all(&ok_response(&payload_thread));
    });
    let dir = temp_dir("stage");
    fs::create_dir_all(dir.join("app/src")).unwrap();
    fs::write(dir.join("app/src/main.ts"), "token-old\n").unwrap();
    let base = format!("http://127.0.0.1:{}/game", server.port);
    fs::write(
      dir.join("yuzuko.bundle.json"),
      r#"{"name":"Game","version":"1.0.0","entry":"src/main.ts"}"#,
    )
    .unwrap();

    assert_eq!(stage_update(&dir, Some(public), Some(&base)).unwrap(), None);
    assert!(!dir.join("Updates/payload").exists());

    *mode.lock().unwrap() = "missing";
    assert_eq!(
      stage_update(&dir, Some(public), Some(&base)).unwrap_err(),
      INVALID_FEED
    );
    assert!(!dir.join("Updates/payload").exists());

    *mode.lock().unwrap() = "bad";
    assert_eq!(
      stage_update(&dir, Some(public), Some(&base)).unwrap_err(),
      BAD_SIGNATURE
    );
    assert!(!dir.join("Updates/payload").exists());

    *mode.lock().unwrap() = "drop";
    assert_eq!(
      stage_update(&dir, Some(public), Some(&base)).unwrap_err(),
      DOWNLOAD_FAILED
    );
    assert!(!dir.join("Updates/payload").exists());

    *mode.lock().unwrap() = "truncate";
    assert_eq!(
      stage_update(&dir, Some(public), Some(&base)).unwrap_err(),
      TRUNCATED
    );
    assert!(!dir.join("Updates/payload").exists());
    assert_eq!(
      fs::read(dir.join("app/src/main.ts")).unwrap(),
      b"token-old\n"
    );

    *mode.lock().unwrap() = "newer";
    assert_eq!(
      stage_update(&dir, Some(public), Some(&base))
        .unwrap()
        .as_deref(),
      Some("1.1.0")
    );
    assert_eq!(stage_update(&dir, Some(public), Some(&base)).unwrap(), None);
    assert_eq!(fs::read(dir.join("Updates/payload")).unwrap(), payload);
    assert_eq!(fs::read(dir.join("Updates/signature")).unwrap(), signature);
    assert_eq!(fs::read(dir.join("Updates/public-key")).unwrap(), public);
    assert_eq!(
      fs::read(dir.join("app/src/main.ts")).unwrap(),
      b"token-old\n"
    );
    apply_pending(&dir);
    assert_eq!(
      fs::read(dir.join("app/src/main.ts")).unwrap(),
      b"token-new\n"
    );
    assert!(!dir.join("Updates/payload").exists());
    assert!(!dir.join("Updates/public-key").exists());
  }

  #[test]
  fn a_rotated_key_replaces_the_staged_trust_anchor() {
    let first_key = signing();
    let first_public = first_key.verifying_key().to_bytes();
    let second_key = SigningKey::from_bytes(&[9u8; 32]);
    let second_public = second_key.verifying_key().to_bytes();
    let first_payload =
      encode_payload("1.1.0", "src/main.ts", &[("src/main.ts", b"one\n")]);
    let second_payload =
      encode_payload("1.2.0", "src/main.ts", &[("src/main.ts", b"two\n")]);
    let first_sig = Signer::try_sign(&first_key, &first_payload)
      .unwrap()
      .to_bytes();
    let second_sig = Signer::try_sign(&second_key, &second_payload)
      .unwrap()
      .to_bytes();
    let mode = Arc::new(Mutex::new("first"));
    let mode_thread = Arc::clone(&mode);
    let first_body = first_payload.clone();
    let second_body = second_payload.clone();
    let first_b64 = BASE64_STANDARD.encode(first_sig);
    let second_b64 = BASE64_STANDARD.encode(second_sig);
    let server = serve(move |stream, path| {
      let current = *mode_thread.lock().unwrap();
      let (version, name, signature, body): (&str, &str, &str, &[u8]) =
        if current == "first" {
          ("1.1.0", "builds/1.1.0.yuzuko", &first_b64, &first_body)
        } else {
          ("1.2.0", "builds/1.2.0.yuzuko", &second_b64, &second_body)
        };
      if path.ends_with("/feed.json") {
        let feed = format!(
          r#"{{"version":"{version}","payload":"{name}","signature":"{signature}"}}"#
        );
        let _ = stream.write_all(&ok_response(feed.as_bytes()));
      } else {
        let _ = stream.write_all(&ok_response(body));
      }
    });
    let dir = temp_dir("rotate");
    fs::create_dir_all(dir.join("app/src")).unwrap();
    fs::write(dir.join("app/src/main.ts"), "old\n").unwrap();
    fs::write(dir.join("yuzuko.update-key"), first_public).unwrap();
    let base = format!("http://127.0.0.1:{}/game", server.port);
    fs::write(
      dir.join("yuzuko.bundle.json"),
      r#"{"name":"Game","version":"1.0.0","entry":"src/main.ts"}"#,
    )
    .unwrap();

    assert_eq!(
      stage_update(&dir, None, Some(&base)).unwrap_err(),
      NO_ANCHOR
    );
    assert_eq!(server.hits.load(AtomicOrdering::SeqCst), 0);
    assert_eq!(
      stage_update(&dir, Some(first_public), Some(&base))
        .unwrap()
        .as_deref(),
      Some("1.1.0")
    );
    assert_eq!(
      fs::read(dir.join("Updates/public-key")).unwrap(),
      first_public
    );
    assert_eq!(
      stage_update(&dir, Some(second_public), Some(&base)).unwrap_err(),
      BAD_SIGNATURE
    );
    assert_eq!(
      fs::read(dir.join("Updates/public-key")).unwrap(),
      first_public
    );
    assert_eq!(
      fs::read(dir.join("Updates/payload")).unwrap(),
      first_payload
    );

    *mode.lock().unwrap() = "second";
    assert_eq!(
      stage_update(&dir, Some(second_public), Some(&base))
        .unwrap()
        .as_deref(),
      Some("1.2.0")
    );
    assert_eq!(
      fs::read(dir.join("Updates/public-key")).unwrap(),
      second_public
    );
    assert_eq!(
      fs::read(dir.join("Updates/payload")).unwrap(),
      second_payload
    );
    apply_pending(&dir);
    assert_eq!(fs::read(dir.join("app/src/main.ts")).unwrap(), b"two\n");
    assert!(!dir.join("Updates/public-key").exists());
    assert_eq!(
      fs::read(dir.join("yuzuko.update-key")).unwrap(),
      first_public
    );
    let _ = server;
  }
}
