//! Process-wide language list and connectivity for `navigator`.
//!
//! The main realm and a dedicated worker read the same values. An operating
//! system change updates that pair and queues `languagechange`, `online`, or
//! `offline` on every live realm. The event runs on the realm thread.
//! `poll` samples at most once a second. Realm waits are capped to the time
//! remaining in that interval, so a parked loop still notices a change.
//! `yuzuko test` can replace the reported values from the object passed to a
//! test function. That function is not a property of `Yuzuko`. Staging a
//! value writes the override and does not publish; the next sample does.

use std::collections::VecDeque;
use std::sync::{Mutex, Weak};
use std::time::{Duration, Instant};

use crate::op::OpException;
use crate::platform::TaskQueue;

const SAMPLE: Duration = Duration::from_secs(1);
/// Win32 `ERROR_NO_DATA`. `GetAdaptersAddresses` returns it when no adapter
/// matches. That host has none, so `navigator.onLine` is false.
const ERROR_NO_DATA: u32 = 232;
const STATE: &str = "The navigator state must be an object.";
const LANGUAGES: &str = "The language list must be an array or null.";
const TAG: &str = "A language tag must be a string.";
const ONLINE: &str = "onLine must be a boolean or null.";

static HOST: Mutex<HostOverride> = Mutex::new(HostOverride {
  languages: None,
  online: None,
});
static ANNOUNCED: Mutex<Announced> = Mutex::new(Announced {
  languages: Vec::new(),
  online: true,
  ready: false,
});
static SAMPLED: Mutex<Option<Instant>> = Mutex::new(None);
static REALMS: Mutex<Vec<Weak<RealmSlot>>> = Mutex::new(Vec::new());
static HANDLER: Mutex<Option<NoticeHandler>> = Mutex::new(None);

thread_local! {
  static LOCAL: std::cell::RefCell<Option<std::sync::Arc<RealmSlot>>> =
    const { std::cell::RefCell::new(None) };
}

type NoticeHandler = fn(&mut v8::PinScope, HostNotice);

/// A connectivity or language change for one realm.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostNotice {
  Language,
  Online,
  Offline,
}

struct HostOverride {
  languages: Option<Vec<String>>,
  online: Option<bool>,
}

struct Announced {
  languages: Vec<String>,
  online: bool,
  ready: bool,
}

struct RealmSlot {
  pending: Mutex<VecDeque<HostNotice>>,
  queue: std::sync::Arc<TaskQueue>,
}

enum Languages {
  Leave,
  Clear,
  Set(Vec<String>),
}

enum Online {
  Leave,
  Clear,
  Set(bool),
}

/// Preferred language tags, most preferred first.
pub(crate) fn reported_languages() -> Vec<String> {
  let overridden = {
    let host = lock(&HOST);
    host.languages.clone()
  };
  match overridden {
    Some(tags) => tags,
    None => crate::locale::os_languages(),
  }
}

/// Whether this process will contact a non-loopback network.
pub fn on_line() -> bool {
  let overridden = {
    let host = lock(&HOST);
    host.online
  };
  match overridden {
    Some(online) => online,
    None => platform_online(),
  }
}

/// Records this realm and the values the getters currently report.
pub(crate) fn install() {
  let slot = std::sync::Arc::new(RealmSlot {
    pending: Mutex::new(VecDeque::new()),
    queue: crate::host::queue(),
  });
  lock(&REALMS).push(std::sync::Arc::downgrade(&slot));
  LOCAL.with(|local| *local.borrow_mut() = Some(slot));
  ensure_baseline();
}

/// Drops this realm. The main realm also clears a test override.
pub(crate) fn clear() {
  let slot = LOCAL.with(|local| local.borrow_mut().take());
  if let Some(slot) = slot {
    lock(&REALMS).retain(|weak| {
      weak
        .upgrade()
        .is_none_or(|live| !std::sync::Arc::ptr_eq(&live, &slot))
    });
  }
  if !crate::host::on_dedicated_worker() {
    let mut host = lock(&HOST);
    host.languages = None;
    host.online = None;
    drop(host);
    let mut announced = lock(&ANNOUNCED);
    announced.ready = false;
    announced.languages.clear();
  }
}

/// Attaches the test-only override to the object passed into a test function.
///
/// `Yuzuko` does not gain these functions. `yuzuko run` and `yuzuko repl`
/// never build that object, and a dedicated worker does not either.
/// `setHostNavigator` publishes immediately. `stageHostNavigator` only writes
/// the override, so the next sample is what fires the event.
pub(crate) fn bind_test_context(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
) {
  define_test_fn(scope, object, "setHostNavigator", set_host_navigator);
  define_test_fn(scope, object, "stageHostNavigator", stage_host_navigator);
}

fn define_test_fn(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
  callback: impl v8::MapFnTo<v8::FunctionCallback>,
) {
  let function = v8::Function::new(scope, callback).expect(name);
  let key = v8::String::new(scope, name).unwrap();
  let _ = object.create_data_property(scope, key.into(), function.into());
}

/// How long a parked realm may sleep before `poll` has to sample again.
pub(crate) fn next_sample_wait() -> Duration {
  let remaining = {
    let sampled = lock(&SAMPLED);
    match *sampled {
      None => Duration::ZERO,
      Some(then) => {
        SAMPLE.saturating_sub(Instant::now().saturating_duration_since(then))
      }
    }
  };
  if remaining.is_zero() {
    Duration::ZERO
  } else {
    remaining.max(Duration::from_millis(1))
  }
}

pub fn set_notice_handler(handler: NoticeHandler) {
  *lock(&HANDLER) = Some(handler);
}

/// Test-only. Writes the language override and does not publish.
#[cfg(test)]
pub(crate) fn stage_languages_for_test(tags: Vec<String>) {
  stage(Languages::Set(tags), Online::Leave);
}

/// Installs `handler` until dropped, then restores the previous one.
#[cfg(test)]
pub(crate) struct NoticeHandlerGuard(Option<NoticeHandler>);

#[cfg(test)]
impl NoticeHandlerGuard {
  pub(crate) fn install(handler: NoticeHandler) -> Self {
    Self(swap_notice_handler(Some(handler)))
  }
}

#[cfg(test)]
impl Drop for NoticeHandlerGuard {
  fn drop(&mut self) {
    swap_notice_handler(self.0.take());
  }
}

#[cfg(test)]
fn swap_notice_handler(
  handler: Option<NoticeHandler>,
) -> Option<NoticeHandler> {
  std::mem::replace(&mut *lock(&HANDLER), handler)
}

/// Dispatches notices queued for this realm.
///
/// Also samples the operating system, at most once a second, so a change
/// that the getters would report becomes an event on the next turn.
pub(crate) fn poll(scope: &mut v8::PinScope) -> bool {
  sample();
  let notices = LOCAL.with(|slot| {
    let local = slot.borrow();
    let Some(realm) = local.as_ref() else {
      return Vec::new();
    };
    lock(&realm.pending).drain(..).collect::<Vec<_>>()
  });
  let handler = *lock(&HANDLER);
  let Some(handler) = handler else {
    return false;
  };
  if notices.is_empty() {
    return false;
  }
  for notice in notices {
    handler(scope, notice);
  }
  true
}

pub(crate) fn pending() -> bool {
  LOCAL.with(|slot| {
    slot
      .borrow()
      .as_ref()
      .is_some_and(|realm| !lock(&realm.pending).is_empty())
  })
}

fn set_host_navigator(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut _ret: v8::ReturnValue,
) {
  match read_override(scope, &args) {
    Ok((languages, online)) => apply(languages, online),
    Err(error) => error.throw(scope),
  }
}

fn stage_host_navigator(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut _ret: v8::ReturnValue,
) {
  match read_override(scope, &args) {
    Ok((languages, online)) => stage(languages, online),
    Err(error) => error.throw(scope),
  }
}

fn read_override(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
) -> Result<(Languages, Online), crate::op::OpError> {
  if args.length() < 1 {
    return Err(crate::op::OpError::type_("Not enough arguments"));
  }
  let value = args.get(0);
  if value.is_null() || value.is_undefined() || !value.is_object() {
    return Err(crate::op::OpError::type_(STATE));
  }
  let object = v8::Local::<v8::Object>::try_from(value)
    .map_err(|_| crate::op::OpError::type_(STATE))?;
  let languages =
    match crate::js::dictionary_member(scope, object, "languages")? {
      None => Languages::Leave,
      Some(value) if value.is_null() => Languages::Clear,
      Some(value) => Languages::Set(language_list(scope, value)?),
    };
  let online = match crate::js::dictionary_member(scope, object, "onLine")? {
    None => Online::Leave,
    Some(value) if value.is_null() => Online::Clear,
    Some(value) if value.is_boolean() => {
      Online::Set(value.boolean_value(scope))
    }
    Some(_) => return Err(crate::op::OpError::type_(ONLINE)),
  };
  Ok((languages, online))
}

fn language_list(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<Vec<String>, crate::op::OpError> {
  let Ok(array) = v8::Local::<v8::Array>::try_from(value) else {
    return Err(crate::op::OpError::type_(LANGUAGES));
  };
  let mut tags = Vec::new();
  for index in 0..array.length() {
    let item = {
      v8::tc_scope!(let tc, scope);
      let array = v8::Local::new(tc, array);
      match array.get_index(tc, index) {
        Some(item) => v8::Global::new(tc, item),
        None => {
          return Err(match tc.exception() {
            Some(thrown) => crate::op::OpError::thrown(tc, thrown),
            None => crate::op::OpError::type_(TAG),
          });
        }
      }
    };
    let item = v8::Local::new(scope, item);
    if !item.is_string() {
      return Err(crate::op::OpError::type_(TAG));
    }
    let Some(text) = item.to_string(scope) else {
      return Err(crate::op::OpError::type_(TAG));
    };
    tags.push(text.to_rust_string_lossy(scope));
  }
  Ok(crate::locale::normalize_language_list(tags))
}

fn apply(languages: Languages, online: Online) {
  let (languages, online) = {
    let mut host = lock(&HOST);
    write_override(&mut host, languages, online);
    let languages = host.languages.clone();
    let online = host.online;
    (languages, online)
  };
  let languages = languages.unwrap_or_else(crate::locale::os_languages);
  let online = online.unwrap_or_else(platform_online);
  commit(languages, online);
}

/// Writes the test override and does not publish or poke.
fn stage(languages: Languages, online: Online) {
  let mut host = lock(&HOST);
  write_override(&mut host, languages, online);
}

fn write_override(
  host: &mut HostOverride,
  languages: Languages,
  online: Online,
) {
  match languages {
    Languages::Leave => {}
    Languages::Clear => host.languages = None,
    Languages::Set(tags) => host.languages = Some(tags),
  }
  match online {
    Online::Leave => {}
    Online::Clear => host.online = None,
    Online::Set(bit) => host.online = Some(bit),
  }
}

/// True when `GetAdaptersAddresses` is reporting an empty adapter list.
///
/// `NO_ERROR` on a filled buffer still has to be walked, so it is not empty.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn adapters_absent(status: u32) -> bool {
  status == ERROR_NO_DATA
}

fn ensure_baseline() {
  if lock(&ANNOUNCED).ready {
    return;
  }
  commit(reported_languages(), on_line());
}

fn sample() {
  let now = Instant::now();
  {
    let mut sampled = lock(&SAMPLED);
    if sampled.is_some_and(|then| now.saturating_duration_since(then) < SAMPLE)
    {
      return;
    }
    *sampled = Some(now);
  }
  commit(reported_languages(), on_line());
}

fn commit(languages: Vec<String>, online: bool) {
  let (language_changed, online_changed) = {
    let mut announced = lock(&ANNOUNCED);
    if !announced.ready {
      announced.languages = languages;
      announced.online = online;
      announced.ready = true;
      return;
    }
    let language_changed = announced.languages != languages;
    let online_changed = announced.online != online;
    announced.languages = languages;
    announced.online = online;
    (language_changed, online_changed)
  };
  if language_changed {
    publish(HostNotice::Language);
  }
  if online_changed {
    publish(if online {
      HostNotice::Online
    } else {
      HostNotice::Offline
    });
  }
}

fn publish(notice: HostNotice) {
  let realms = {
    let mut realms = lock(&REALMS);
    realms.retain(|weak| weak.strong_count() > 0);
    realms.iter().filter_map(Weak::upgrade).collect::<Vec<_>>()
  };
  for realm in realms {
    lock(&realm.pending).push_back(notice);
    realm.queue.poke();
  }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
  mutex.lock().expect("navigator host")
}

/// True when a non-loopback interface is up. Unknown is true: HTML returns
/// true unless the user agent knows it will not contact the network.
fn platform_online() -> bool {
  #[cfg(any(target_os = "macos", target_os = "linux"))]
  {
    unix_online()
  }
  #[cfg(target_os = "windows")]
  {
    windows_online()
  }
  #[cfg(not(any(
    target_os = "macos",
    target_os = "linux",
    target_os = "windows"
  )))]
  {
    true
  }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn unix_online() -> bool {
  struct Interfaces(*mut libc::ifaddrs);
  impl Drop for Interfaces {
    fn drop(&mut self) {
      if !self.0.is_null() {
        unsafe { libc::freeifaddrs(self.0) };
      }
    }
  }

  unsafe {
    let mut head: *mut libc::ifaddrs = std::ptr::null_mut();
    if libc::getifaddrs(&mut head) != 0 {
      return true;
    }
    let _guard = Interfaces(head);
    let mut cursor = head;
    while !cursor.is_null() {
      let iface = &*cursor;
      let flags = iface.ifa_flags;
      let up = flags & (libc::IFF_UP as u32) != 0;
      let loopback = flags & (libc::IFF_LOOPBACK as u32) != 0;
      if up && !loopback && !iface.ifa_addr.is_null() {
        let family = i32::from((*iface.ifa_addr).sa_family);
        if family == libc::AF_INET || family == libc::AF_INET6 {
          return true;
        }
      }
      cursor = iface.ifa_next;
    }
    false
  }
}

#[cfg(target_os = "windows")]
fn windows_online() -> bool {
  use windows_sys::Win32::Foundation::{
    ERROR_BUFFER_OVERFLOW, ERROR_NO_DATA as WIN_ERROR_NO_DATA,
  };
  const _: () = assert!(ERROR_NO_DATA == WIN_ERROR_NO_DATA);
  use windows_sys::Win32::NetworkManagement::IpHelper::{
    GAA_FLAG_SKIP_ANYCAST, GAA_FLAG_SKIP_DNS_SERVER,
    GAA_FLAG_SKIP_FRIENDLY_NAME, GAA_FLAG_SKIP_MULTICAST, GetAdaptersAddresses,
    IF_TYPE_SOFTWARE_LOOPBACK, IP_ADAPTER_ADDRESSES_LH,
  };
  use windows_sys::Win32::NetworkManagement::Ndis::IfOperStatusUp;
  use windows_sys::Win32::Networking::WinSock::AF_UNSPEC;

  let flags = GAA_FLAG_SKIP_ANYCAST
    | GAA_FLAG_SKIP_MULTICAST
    | GAA_FLAG_SKIP_DNS_SERVER
    | GAA_FLAG_SKIP_FRIENDLY_NAME;
  unsafe {
    let mut size = 0u32;
    let status = GetAdaptersAddresses(
      u32::from(AF_UNSPEC),
      flags,
      std::ptr::null(),
      std::ptr::null_mut(),
      &mut size,
    );
    if status == 0 || adapters_absent(status) {
      return false;
    }
    if status != ERROR_BUFFER_OVERFLOW || size == 0 {
      return true;
    }
    let mut buffer = vec![0u64; (size as usize / 8) + 2];
    let status = GetAdaptersAddresses(
      u32::from(AF_UNSPEC),
      flags,
      std::ptr::null(),
      buffer.as_mut_ptr().cast(),
      &mut size,
    );
    if status == ERROR_BUFFER_OVERFLOW {
      buffer.resize((size as usize / 8) + 2, 0);
      let status = GetAdaptersAddresses(
        u32::from(AF_UNSPEC),
        flags,
        std::ptr::null(),
        buffer.as_mut_ptr().cast(),
        &mut size,
      );
      if adapters_absent(status) {
        return false;
      }
      if status != 0 {
        return true;
      }
    } else if adapters_absent(status) {
      return false;
    } else if status != 0 {
      return true;
    }
    let mut cursor = buffer.as_ptr() as *const IP_ADAPTER_ADDRESSES_LH;
    while !cursor.is_null() {
      let adapter = &*cursor;
      if adapter.OperStatus == IfOperStatusUp
        && adapter.IfType != IF_TYPE_SOFTWARE_LOOPBACK
        && !adapter.FirstUnicastAddress.is_null()
      {
        return true;
      }
      cursor = adapter.Next;
    }
    false
  }
}

#[cfg(test)]
mod tests {
  use super::adapters_absent;

  #[test]
  fn no_adapter_data_is_offline() {
    // 232 is Win32 ERROR_NO_DATA. 111 is ERROR_BUFFER_OVERFLOW. 0 is NO_ERROR
    // on a filled buffer, which still has to be walked.
    assert!(adapters_absent(232));
    assert!(!adapters_absent(111));
    assert!(!adapters_absent(0));
    assert!(!adapters_absent(87));
  }
}
