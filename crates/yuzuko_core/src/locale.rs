//! Preferred language tags for `navigator.language` and `navigator.languages`.
//!
//! The list is the operating system's list. A permission check would hide the
//! same fact `hardwareConcurrency` reports from the process.

/// Preferred language tags, most preferred first.
///
/// An empty operating-system list is `en`. Duplicate tags are removed,
/// keeping the first spelling.
pub fn preferred_languages() -> Vec<String> {
  finish(platform_languages())
}

/// The first preferred tag, or `en` when the list is empty.
pub fn primary_language() -> String {
  preferred_languages()
    .into_iter()
    .next()
    .unwrap_or_else(|| "en".to_string())
}

/// A new frozen array of [`preferred_languages`].
pub fn languages_array<'s>(
  scope: &mut v8::PinScope<'s, '_>,
) -> v8::Local<'s, v8::Array> {
  let tags = preferred_languages();
  let array = v8::Array::new(scope, tags.len() as i32);
  for (index, tag) in tags.iter().enumerate() {
    let value = v8::String::new(scope, tag).unwrap();
    array.set_index(scope, index as u32, value.into());
  }
  let _ = array.set_integrity_level(scope, v8::IntegrityLevel::Frozen);
  array
}

fn platform_languages() -> Vec<String> {
  #[cfg(target_os = "macos")]
  fn read() -> Vec<String> {
    use std::ffi::{CStr, c_char, c_void};

    type CFIndex = isize;
    type CFTypeRef = *const c_void;

    const UTF8: u32 = 0x0800_0100;

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
      fn CFLocaleCopyPreferredLanguages() -> CFTypeRef;
      fn CFArrayGetCount(array: CFTypeRef) -> CFIndex;
      fn CFArrayGetValueAtIndex(array: CFTypeRef, index: CFIndex) -> CFTypeRef;
      fn CFStringGetCString(
        string: CFTypeRef,
        buffer: *mut c_char,
        buffer_size: CFIndex,
        encoding: u32,
      ) -> u8;
      fn CFRelease(value: CFTypeRef);
    }

    unsafe {
      let array = CFLocaleCopyPreferredLanguages();
      if array.is_null() {
        return Vec::new();
      }
      let mut tags = Vec::new();
      let count = CFArrayGetCount(array);
      for index in 0..count {
        let value = CFArrayGetValueAtIndex(array, index);
        if value.is_null() {
          continue;
        }
        let mut buffer = [0u8; 256];
        let ok = CFStringGetCString(
          value,
          buffer.as_mut_ptr().cast::<c_char>(),
          buffer.len() as CFIndex,
          UTF8,
        );
        if ok == 0 {
          continue;
        }
        let text = CStr::from_ptr(buffer.as_ptr().cast::<c_char>());
        push_unique(&mut tags, text.to_string_lossy().into_owned());
      }
      CFRelease(array);
      tags
    }
  }

  #[cfg(target_os = "windows")]
  fn read() -> Vec<String> {
    use windows_sys::Win32::Globalization::{
      GetUserPreferredUILanguages, MUI_LANGUAGE_NAME,
    };

    unsafe {
      let mut count = 0u32;
      let mut size = 0u32;
      if GetUserPreferredUILanguages(
        MUI_LANGUAGE_NAME,
        &mut count,
        std::ptr::null_mut(),
        &mut size,
      ) == 0
        || size == 0
      {
        return Vec::new();
      }
      let mut buffer = vec![0u16; size as usize];
      if GetUserPreferredUILanguages(
        MUI_LANGUAGE_NAME,
        &mut count,
        buffer.as_mut_ptr(),
        &mut size,
      ) == 0
      {
        return Vec::new();
      }
      let mut tags = Vec::new();
      let mut start = 0usize;
      for (index, unit) in buffer.iter().enumerate() {
        if *unit != 0 {
          continue;
        }
        if index == start {
          break;
        }
        if let Ok(text) = String::from_utf16(&buffer[start..index]) {
          push_unique(&mut tags, text);
        }
        start = index + 1;
      }
      tags
    }
  }

  #[cfg(not(any(target_os = "macos", target_os = "windows")))]
  fn read() -> Vec<String> {
    fn env_string(name: &str) -> Option<String> {
      std::env::var_os(name).map(|value| value.to_string_lossy().into_owned())
    }
    collect_posix(
      env_string("LANGUAGE").as_deref(),
      env_string("LC_ALL").as_deref(),
      env_string("LC_MESSAGES").as_deref(),
      env_string("LANG").as_deref(),
    )
  }

  read()
}

/// POSIX locale environment values in lookup order.
///
/// macOS and Windows read the operating-system list instead. The helpers stay
/// so their tests run on every host.
///
/// `language` is `LANGUAGE` split on `:`. Then `LC_ALL`, `LC_MESSAGES`, and
/// `LANG`. `C` and `POSIX` are skipped. A codeset or modifier is dropped.
#[cfg_attr(any(target_os = "macos", target_os = "windows"), allow(dead_code))]
pub(crate) fn language_tags_from_posix(
  language: Option<&str>,
  lc_all: Option<&str>,
  lc_messages: Option<&str>,
  lang: Option<&str>,
) -> Vec<String> {
  finish(collect_posix(language, lc_all, lc_messages, lang))
}

#[cfg_attr(any(target_os = "macos", target_os = "windows"), allow(dead_code))]
fn collect_posix(
  language: Option<&str>,
  lc_all: Option<&str>,
  lc_messages: Option<&str>,
  lang: Option<&str>,
) -> Vec<String> {
  let mut tags = Vec::new();
  if let Some(language) = language {
    for part in language.split(':') {
      if let Some(tag) = posix_language_tag(part) {
        push_unique(&mut tags, tag);
      }
    }
  }
  for value in [lc_all, lc_messages, lang].into_iter().flatten() {
    if let Some(tag) = posix_language_tag(value) {
      push_unique(&mut tags, tag);
    }
  }
  tags
}

fn finish(mut tags: Vec<String>) -> Vec<String> {
  tags.retain(|tag| !tag.is_empty());
  if tags.is_empty() {
    tags.push("en".to_string());
  }
  tags
}

fn push_unique(tags: &mut Vec<String>, tag: String) {
  if tag.is_empty() {
    return;
  }
  if tags
    .iter()
    .any(|existing| existing.eq_ignore_ascii_case(&tag))
  {
    return;
  }
  tags.push(tag);
}

/// `en_US.UTF-8@euro` becomes `en-US`. `C` and `POSIX` are absent.
#[cfg_attr(any(target_os = "macos", target_os = "windows"), allow(dead_code))]
pub(crate) fn posix_language_tag(raw: &str) -> Option<String> {
  let head = raw.split(['.', '@']).next().unwrap_or("").trim();
  if head.is_empty()
    || head.eq_ignore_ascii_case("C")
    || head.eq_ignore_ascii_case("POSIX")
  {
    return None;
  }
  let tag = head.replace('_', "-");
  if tag.is_empty() { None } else { Some(tag) }
}

#[cfg(test)]
mod tests {
  use super::language_tags_from_posix;
  use super::posix_language_tag;

  #[test]
  fn posix_tags_drop_codeset_and_skip_c() {
    assert_eq!(posix_language_tag("en_US.UTF-8").as_deref(), Some("en-US"));
    assert_eq!(
      posix_language_tag("fr_FR.UTF-8@euro").as_deref(),
      Some("fr-FR")
    );
    assert_eq!(posix_language_tag("C.UTF-8"), None);
    assert_eq!(posix_language_tag("POSIX"), None);
    assert_eq!(posix_language_tag("c"), None);
    assert_eq!(posix_language_tag(""), None);
  }

  #[test]
  fn language_list_keeps_order_and_falls_back() {
    assert_eq!(
      language_tags_from_posix(
        Some("en_US:fr:en_US:C:"),
        Some("de_DE.UTF-8"),
        Some("fr_FR"),
        Some("ja_JP.UTF-8")
      ),
      vec!["en-US", "fr", "de-DE", "fr-FR", "ja-JP"]
    );
    assert_eq!(
      language_tags_from_posix(None, Some("C"), Some("POSIX"), Some("C.UTF-8")),
      vec!["en"]
    );
    assert_eq!(language_tags_from_posix(None, None, None, None), vec!["en"]);
    assert_eq!(
      language_tags_from_posix(Some("EN-us"), None, None, Some("en-US")),
      vec!["EN-us"]
    );
  }
}
