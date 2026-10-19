//! Display and accessibility settings used by `matchMedia`.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scheme {
  Light,
  Dark,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gamut {
  Srgb,
  P3,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Contrast {
  NoPreference,
  More,
}

#[derive(Clone, Copy, Debug)]
pub struct Preferences {
  pub scheme: Scheme,
  pub gamut: Gamut,
  pub reduced_motion: bool,
  pub contrast: Contrast,
}

pub fn current() -> Preferences {
  Preferences {
    scheme: scheme(),
    gamut: gamut(),
    reduced_motion: reduced_motion(),
    contrast: contrast(),
  }
}

pub fn note_scheme(dark: bool) {
  SCHEME.with(|slot| {
    slot.set(Some(if dark { Scheme::Dark } else { Scheme::Light }));
  });
}

fn scheme() -> Scheme {
  if let Some(scheme) = SCHEME.with(|slot| slot.get()) {
    return scheme;
  }
  let scheme = system_scheme();
  SCHEME.with(|slot| slot.set(Some(scheme)));
  scheme
}

fn gamut() -> Gamut {
  if system_p3() { Gamut::P3 } else { Gamut::Srgb }
}

fn reduced_motion() -> bool {
  system_flag("accessibilityDisplayShouldReduceMotion")
}

fn contrast() -> Contrast {
  if system_flag("accessibilityDisplayShouldIncreaseContrast") {
    Contrast::More
  } else {
    Contrast::NoPreference
  }
}

std::thread_local! {
  static SCHEME: std::cell::Cell<Option<Scheme>> = const { std::cell::Cell::new(None) };
}

#[cfg(target_os = "macos")]
fn system_scheme() -> Scheme {
  match apple::user_default("AppleInterfaceStyle").as_deref() {
    Some("Dark") => Scheme::Dark,
    _ => Scheme::Light,
  }
}

#[cfg(not(target_os = "macos"))]
fn system_scheme() -> Scheme {
  Scheme::Light
}

#[cfg(target_os = "macos")]
fn system_p3() -> bool {
  apple::screen_supports_p3()
}

#[cfg(not(target_os = "macos"))]
fn system_p3() -> bool {
  false
}

#[cfg(target_os = "macos")]
fn system_flag(selector: &str) -> bool {
  apple::workspace_bool(selector)
}

#[cfg(not(target_os = "macos"))]
fn system_flag(_selector: &str) -> bool {
  false
}

#[cfg(target_os = "macos")]
mod apple {
  use std::ffi::{CStr, CString, c_char, c_void};

  #[link(name = "objc")]
  unsafe extern "C" {
    fn objc_getClass(name: *const c_char) -> *mut c_void;
    fn sel_registerName(name: *const c_char) -> *mut c_void;
    fn objc_msgSend();
  }

  #[link(name = "AppKit", kind = "framework")]
  unsafe extern "C" {}

  pub fn user_default(key: &str) -> Option<String> {
    unsafe {
      let defaults =
        msg0(class("NSUserDefaults")?, sel("standardUserDefaults")?)?;
      let key = ns_string(key)?;
      let value = msg1(defaults, sel("stringForKey:")?, key)?;
      let utf8 = msg0(value, sel("UTF8String")?)? as *const c_char;
      Some(CStr::from_ptr(utf8).to_string_lossy().into_owned())
    }
  }

  pub fn screen_supports_p3() -> bool {
    unsafe {
      let Some(class) = class("NSScreen") else {
        return false;
      };
      let Some(selector) = sel("mainScreen") else {
        return false;
      };
      let Some(screen) = msg0(class, selector) else {
        return false;
      };
      let Some(selector) = sel("canRepresentDisplayGamut:") else {
        return false;
      };
      // NSDisplayGamutP3 = 2
      msg_bool_int(screen, selector, 2)
    }
  }

  pub fn workspace_bool(selector: &str) -> bool {
    unsafe {
      let Some(class) = class("NSWorkspace") else {
        return false;
      };
      let Some(shared) = sel("sharedWorkspace") else {
        return false;
      };
      let Some(workspace) = msg0(class, shared) else {
        return false;
      };
      let mut name = selector.to_string();
      name.push('\0');
      msg_bool(workspace, sel_registerName(name.as_ptr().cast()))
    }
  }

  unsafe fn class(name: &str) -> Option<*mut c_void> {
    let mut name = name.to_string();
    name.push('\0');
    let class = unsafe { objc_getClass(name.as_ptr().cast()) };
    if class.is_null() { None } else { Some(class) }
  }

  unsafe fn sel(name: &str) -> Option<*mut c_void> {
    let mut name = name.to_string();
    name.push('\0');
    let sel = unsafe { sel_registerName(name.as_ptr().cast()) };
    if sel.is_null() { None } else { Some(sel) }
  }

  unsafe fn ns_string(text: &str) -> Option<*mut c_void> {
    let c = CString::new(text).ok()?;
    unsafe {
      msg1(
        class("NSString")?,
        sel("stringWithUTF8String:")?,
        c.as_ptr() as *mut c_void,
      )
    }
  }

  unsafe fn msg0(obj: *mut c_void, sel: *mut c_void) -> Option<*mut c_void> {
    if obj.is_null() || sel.is_null() {
      return None;
    }
    let send: unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void =
      unsafe { std::mem::transmute(objc_msgSend as *const ()) };
    let value = unsafe { send(obj, sel) };
    if value.is_null() { None } else { Some(value) }
  }

  unsafe fn msg1(
    obj: *mut c_void,
    sel: *mut c_void,
    arg: *mut c_void,
  ) -> Option<*mut c_void> {
    if obj.is_null() || sel.is_null() {
      return None;
    }
    let send: unsafe extern "C" fn(
      *mut c_void,
      *mut c_void,
      *mut c_void,
    ) -> *mut c_void =
      unsafe { std::mem::transmute(objc_msgSend as *const ()) };
    let value = unsafe { send(obj, sel, arg) };
    if value.is_null() { None } else { Some(value) }
  }

  unsafe fn msg_bool(obj: *mut c_void, sel: *mut c_void) -> bool {
    if obj.is_null() || sel.is_null() {
      return false;
    }
    let send: unsafe extern "C" fn(*mut c_void, *mut c_void) -> i8 =
      unsafe { std::mem::transmute(objc_msgSend as *const ()) };
    unsafe { send(obj, sel) != 0 }
  }

  unsafe fn msg_bool_int(
    obj: *mut c_void,
    sel: *mut c_void,
    arg: isize,
  ) -> bool {
    if obj.is_null() || sel.is_null() {
      return false;
    }
    let send: unsafe extern "C" fn(*mut c_void, *mut c_void, isize) -> i8 =
      unsafe { std::mem::transmute(objc_msgSend as *const ()) };
    unsafe { send(obj, sel, arg) != 0 }
  }
}
