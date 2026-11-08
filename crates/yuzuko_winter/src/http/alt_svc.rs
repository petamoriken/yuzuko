//! `Alt-Svc` cache for HTTP/3.
//!
//! The cache is process-wide and is not written to disk. An entry names the
//! UDP endpoint of one `h3` alternative. `clear` and `ma=0` drop it.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use url::Host;

struct Record {
  /// Host to dial. An omitted alternative host uses the origin host.
  host: String,
  port: u16,
  expires: Instant,
}

static CACHE: Mutex<Option<HashMap<String, Record>>> = Mutex::new(None);

/// `https://host:port`, with brackets around an IPv6 address.
pub(super) fn origin_key(url: &str) -> Option<String> {
  let url = url::Url::parse(url).ok()?;
  if url.scheme() != "https" {
    return None;
  }
  let port = url.port_or_known_default()?;
  match url.host()? {
    Host::Ipv6(ip) => Some(format!("https://[{ip}]:{port}")),
    Host::Ipv4(ip) => Some(format!("https://{ip}:{port}")),
    Host::Domain(domain) => Some(format!("https://{domain}:{port}")),
  }
}

pub(super) fn lookup(url: &str) -> Option<(String, u16)> {
  // The policy check runs after the cache lock is released. A prompt can
  // re-enter the client, and that client takes this lock.
  lookup_if(url, alternative_allowed)
}

fn lookup_if(
  url: &str,
  allow: impl Fn(&str, u16) -> bool,
) -> Option<(String, u16)> {
  let (host, port) = cached(url)?;
  if !allow(&host, port) {
    return None;
  }
  Some((host, port))
}

/// The cached alternative, without a permission check.
fn cached(url: &str) -> Option<(String, u16)> {
  let key = origin_key(url)?;
  let mut cache = CACHE.lock().expect("alt-svc");
  let map = cache.get_or_insert_with(HashMap::new);
  let record = map.get(&key)?;
  if record.expires <= Instant::now() {
    map.remove(&key);
    return None;
  }
  Some((record.host.clone(), record.port))
}

/// Whether this process may dial `host` on `port`.
///
/// The origin check does not cover an `Alt-Svc` host or a different port.
fn alternative_allowed(host: &str, port: u16) -> bool {
  yuzuko_core::check_net(host, port).is_ok()
}

pub(super) fn invalidate(url: &str) {
  let Some(key) = origin_key(url) else {
    return;
  };
  if let Some(map) = CACHE.lock().expect("alt-svc").as_mut() {
    map.remove(&key);
  }
}

pub(super) fn observe(url: &str, headers: &hyper::HeaderMap) {
  let mut values = Vec::new();
  for value in headers.get_all(hyper::header::ALT_SVC) {
    values.push(value.as_bytes());
  }
  if values.is_empty() {
    return;
  }
  let mut text = String::new();
  for (index, value) in values.iter().enumerate() {
    if index > 0 {
      text.push(',');
    }
    text.push_str(&String::from_utf8_lossy(value));
  }
  record(url, &text);
}

pub(super) fn record(url: &str, header: &str) {
  let Some(key) = origin_key(url) else {
    return;
  };
  let Ok(parsed) = url::Url::parse(url) else {
    return;
  };
  let Some(origin_host) = origin_host(&parsed) else {
    return;
  };
  let mut cache = CACHE.lock().expect("alt-svc");
  let map = cache.get_or_insert_with(HashMap::new);
  if header_clears(header) {
    map.remove(&key);
    return;
  }
  let Some(alternative) = first_h3(header) else {
    return;
  };
  if alternative.max_age.is_zero() {
    map.remove(&key);
    return;
  }
  let host = alternative.host.unwrap_or(origin_host);
  map.insert(
    key,
    Record {
      host,
      port: alternative.port,
      expires: Instant::now() + alternative.max_age,
    },
  );
}

fn origin_host(url: &url::Url) -> Option<String> {
  match url.host()? {
    Host::Ipv6(ip) => Some(format!("[{ip}]")),
    Host::Ipv4(ip) => Some(ip.to_string()),
    Host::Domain(domain) => Some(domain.to_string()),
  }
}

struct Alternative {
  host: Option<String>,
  port: u16,
  max_age: Duration,
}

fn header_clears(header: &str) -> bool {
  let mut rest = header.trim();
  if rest.is_empty() {
    return false;
  }
  let token = take_token(&mut rest);
  token.eq_ignore_ascii_case("clear") && rest.trim().is_empty()
}

fn first_h3(header: &str) -> Option<Alternative> {
  let mut rest = header;
  while !rest.is_empty() {
    rest = rest.trim_start();
    if rest.is_empty() {
      break;
    }
    if let Some(alternative) = one_alternative(&mut rest) {
      return Some(alternative);
    }
    skip_until_comma(&mut rest);
  }
  None
}

fn one_alternative(rest: &mut &str) -> Option<Alternative> {
  let protocol = take_token(rest);
  if protocol.is_empty() {
    return None;
  }
  *rest = rest.trim_start();
  if !rest.starts_with('=') {
    return None;
  }
  *rest = &rest[1..];
  *rest = rest.trim_start();
  let authority = take_quoted(rest)?;
  let mut max_age = Duration::from_secs(24 * 60 * 60);
  let mut clear = false;
  while rest.trim_start().starts_with(';') {
    *rest = rest.trim_start();
    *rest = &rest[1..];
    *rest = rest.trim_start();
    let name = take_token(rest);
    *rest = rest.trim_start();
    let value = if rest.starts_with('=') {
      *rest = &rest[1..];
      *rest = rest.trim_start();
      if rest.starts_with('"') {
        take_quoted(rest).unwrap_or_default()
      } else {
        take_token(rest)
      }
    } else {
      String::new()
    };
    if name.eq_ignore_ascii_case("ma") {
      if value == "0" {
        max_age = Duration::ZERO;
      } else if let Ok(seconds) = value.parse::<u64>() {
        max_age = Duration::from_secs(seconds);
      }
    }
  }
  if protocol.eq_ignore_ascii_case("clear") {
    clear = true;
  }
  if clear || !protocol.eq_ignore_ascii_case("h3") {
    return None;
  }
  let (host, port) = split_authority(&authority)?;
  Some(Alternative {
    host,
    port,
    max_age,
  })
}

fn split_authority(authority: &str) -> Option<(Option<String>, u16)> {
  if let Some(port) = authority.strip_prefix(':') {
    return Some((None, port.parse().ok()?));
  }
  if let Some(rest) = authority.strip_prefix('[') {
    let (host, port) = rest.split_once("]:")?;
    if host.is_empty() {
      return None;
    }
    return Some((Some(format!("[{host}]")), port.parse().ok()?));
  }
  let (host, port) = authority.rsplit_once(':')?;
  if host.is_empty() {
    return None;
  }
  Some((Some(host.to_string()), port.parse().ok()?))
}

fn take_token(rest: &mut &str) -> String {
  let end = rest
    .find(|ch: char| {
      ch.is_ascii_whitespace() || matches!(ch, '=' | ';' | ',' | '"')
    })
    .unwrap_or(rest.len());
  let token = rest[..end].to_string();
  *rest = &rest[end..];
  token
}

fn take_quoted(rest: &mut &str) -> Option<String> {
  if !rest.starts_with('"') {
    return None;
  }
  let bytes = rest.as_bytes();
  let mut out = String::new();
  let mut index = 1;
  while index < bytes.len() {
    let byte = bytes[index];
    if byte == b'\\' && index + 1 < bytes.len() {
      out.push(char::from(bytes[index + 1]));
      index += 2;
      continue;
    }
    if byte == b'"' {
      *rest = &rest[index + 1..];
      return Some(out);
    }
    out.push(char::from(byte));
    index += 1;
  }
  None
}

fn skip_until_comma(rest: &mut &str) {
  let mut quoted = false;
  let mut escaped = false;
  for (index, byte) in rest.bytes().enumerate() {
    if escaped {
      escaped = false;
      continue;
    }
    if byte == b'\\' && quoted {
      escaped = true;
      continue;
    }
    if byte == b'"' {
      quoted = !quoted;
      continue;
    }
    if byte == b',' && !quoted {
      *rest = rest[index + 1..].trim_start();
      return;
    }
  }
  *rest = "";
}

pub(super) fn server_name(url: &str) -> Option<String> {
  let url = url::Url::parse(url).ok()?;
  origin_host(&url)
}

#[cfg(test)]
mod tests {
  use super::{cached, first_h3, invalidate, lookup_if, record};

  fn allow_origin_443(host: &str, port: u16) -> bool {
    host.eq_ignore_ascii_case("alt-svc-perm.example") && port == 443
  }

  #[test]
  fn parses_same_host_and_max_age() {
    let alternative = first_h3(r#"h3=":443"; ma=60"#).unwrap();
    assert!(alternative.host.is_none());
    assert_eq!(alternative.port, 443);
    assert_eq!(alternative.max_age.as_secs(), 60);
  }

  #[test]
  fn skips_non_h3_and_reads_host() {
    let alternative =
      first_h3(r#"h2=":443", h3="alt.example:444"; ma=10"#).unwrap();
    assert_eq!(alternative.host.as_deref(), Some("alt.example"));
    assert_eq!(alternative.port, 444);
  }

  #[test]
  fn clear_is_not_an_alternative() {
    assert!(first_h3("clear").is_none());
  }

  #[test]
  fn a_denied_alternative_is_not_dialed() {
    let url = "https://alt-svc-perm.example/path";
    let _clear = Clear(url);
    record(url, r#"h3="evil.example:443"; ma=60"#);
    assert!(lookup_if(url, allow_origin_443).is_none());
    let stored = cached(url).unwrap();
    assert_eq!(stored.0, "evil.example");
    assert_eq!(stored.1, 443);

    record(url, r#"h3=":444"; ma=60"#);
    assert!(lookup_if(url, allow_origin_443).is_none());
    assert_eq!(cached(url).unwrap().1, 444);

    record(url, r#"h3=":443"; ma=60"#);
    assert_eq!(
      lookup_if(url, allow_origin_443).unwrap(),
      ("alt-svc-perm.example".to_string(), 443)
    );
  }

  struct Clear(&'static str);

  impl Drop for Clear {
    fn drop(&mut self) {
      invalidate(self.0);
    }
  }
}
