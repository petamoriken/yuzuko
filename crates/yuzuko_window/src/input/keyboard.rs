//! `navigator.keyboard` on the main global.
//!
//! `lock` reserves UI Events `code` values for the focused window. The
//! reservation is process-wide. Headless tests record it and do not change
//! operating-system shortcuts. Inhibition applies only while a window of
//! this process is focused. Closing that window clears the reservation.

use std::cell::RefCell;

use yuzuko_core::op::FromV8;
use yuzuko_core::op::Js;
use yuzuko_core::op::OpError;
use yuzuko_dom::EventTarget;
use yuzuko_macros::op;

const NOT_SEQUENCE: &str = "The key code list is not a sequence";
const TOO_LONG: &str = "The key code list is too long";
const NO_FOCUS: &str = "No window is focused";
const BAD_CODE: &str = "The key code is not a valid code";
const MAX_CODES: usize = 1024;

struct Reservation {
  enabled: bool,
  codes: Vec<String>,
}

struct State {
  reservation: Reservation,
  held_for: Option<u64>,
  hold: Option<platform::Hold>,
}

std::thread_local! {
  static STATE: RefCell<State> = RefCell::new(State {
    reservation: Reservation {
      enabled: false,
      codes: Vec::new(),
    },
    held_for: None,
    hold: None,
  });
  static KEYBOARD: RefCell<Option<v8::Global<v8::Object>>> =
    const { RefCell::new(None) };
}

pub(crate) fn install(scope: &mut v8::PinScope) {
  Keyboard::install(scope);
}

pub(crate) fn instance(scope: &mut v8::PinScope) -> Js {
  let existing = KEYBOARD.with(|slot| {
    slot
      .borrow()
      .as_ref()
      .map(|value| v8::Local::new(scope, value))
  });
  let object = existing.unwrap_or_else(|| {
    let object = unsafe {
      yuzuko_core::op::wrap_instance(
        scope,
        Keyboard {
          base: EventTarget::new(),
        },
      )
    };
    KEYBOARD.with(|slot| {
      *slot.borrow_mut() = Some(v8::Global::new(scope, object));
    });
    object
  });
  let value: v8::Local<v8::Value> = object.into();
  Js(v8::Global::new(scope, value))
}

/// Drops the inhibitor and the cached object when the realm shuts down.
pub(crate) fn clear() {
  release_lock();
  KEYBOARD.with(|slot| *slot.borrow_mut() = None);
}

pub(crate) fn unlock() {
  release_lock();
}

/// Applies or drops the operating-system inhibitor for the current focus.
pub(crate) fn sync() {
  let action = STATE.with(|slot| {
    let state = slot.borrow();
    let focused = crate::activation::focused_id();
    let want = state.reservation.enabled
      && focused.is_some()
      && !crate::session::headless()
      && platform::wants(&state.reservation.codes);
    if !want {
      Action::Drop
    } else if state.hold.is_some() && state.held_for == focused {
      Action::Retarget
    } else if let Some(id) = focused {
      Action::Acquire(id)
    } else {
      Action::Drop
    }
  });
  match action {
    Action::Drop => STATE.with(|slot| {
      let mut state = slot.borrow_mut();
      state.hold = None;
      state.held_for = None;
    }),
    Action::Retarget => {
      let (all, codes) = STATE.with(|slot| {
        let state = slot.borrow();
        (
          state.reservation.codes.is_empty(),
          state.reservation.codes.clone(),
        )
      });
      platform::retarget(all, &codes);
    }
    Action::Acquire(id) => {
      STATE.with(|slot| {
        let mut state = slot.borrow_mut();
        state.hold = None;
        state.held_for = None;
      });
      let codes = STATE.with(|slot| slot.borrow().reservation.codes.clone());
      let hold = platform::acquire(id, &codes);
      STATE.with(|slot| {
        let mut state = slot.borrow_mut();
        if state.reservation.enabled
          && state.hold.is_none()
          && crate::activation::focused_id() == Some(id)
        {
          if hold.is_some() {
            state.held_for = Some(id);
          }
          state.hold = hold;
        }
      });
    }
  }
}

enum Action {
  Drop,
  Retarget,
  Acquire(u64),
}

fn release_lock() {
  STATE.with(|slot| {
    let mut state = slot.borrow_mut();
    state.reservation.enabled = false;
    state.reservation.codes.clear();
    state.hold = None;
    state.held_for = None;
  });
}

#[derive(Debug)]
enum LockReject {
  Unfocused,
  Invalid,
}

/// Stores a reservation. An unfocused call leaves `reservation` unchanged.
/// An invalid code clears it.
fn reserve(
  focused: bool,
  requested: Vec<String>,
  reservation: &mut Reservation,
) -> Result<(), LockReject> {
  if !focused {
    return Err(LockReject::Unfocused);
  }
  match normalize(requested) {
    Ok(codes) => {
      reservation.enabled = true;
      reservation.codes = codes;
      Ok(())
    }
    Err(()) => {
      reservation.enabled = false;
      reservation.codes.clear();
      Err(LockReject::Invalid)
    }
  }
}

fn normalize(codes: Vec<String>) -> Result<Vec<String>, ()> {
  let mut kept = Vec::new();
  for code in codes {
    if !super::keys::is_ui_code(&code) {
      return Err(());
    }
    if !kept.iter().any(|existing| existing == &code) {
      kept.push(code);
    }
  }
  Ok(kept)
}

/// macOS disables symbolic hotkeys only for an empty list or for keys that
/// usually belong to the system. A movement-only list leaves those hotkeys
/// alone.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn suppresses_symbolic(codes: &[String]) -> bool {
  const HOT: &[&str] = &["Escape", "Tab", "Space", "MetaLeft", "MetaRight"];
  codes.is_empty() || codes.iter().any(|code| HOT.contains(&code.as_str()))
}

/// Windows virtual-key code for a UI Events `code`, when one exists.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn virtual_key(code: &str) -> Option<u16> {
  if let Some(rest) = code.strip_prefix("Key")
    && rest.len() == 1
    && let Some(letter) = rest.chars().next()
    && letter.is_ascii_uppercase()
  {
    return Some(u16::from(letter as u8));
  }
  if let Some(rest) = code.strip_prefix("Digit")
    && rest.len() == 1
    && let Some(digit) = rest.chars().next()
    && digit.is_ascii_digit()
  {
    return Some(u16::from(digit as u8));
  }
  if let Some(rest) = code.strip_prefix('F')
    && let Ok(number) = rest.parse::<u16>()
    && (1..=24).contains(&number)
  {
    return Some(0x6F + number);
  }
  if let Some(rest) = code.strip_prefix("Numpad")
    && rest.len() == 1
    && let Some(digit) = rest.chars().next()
    && digit.is_ascii_digit()
  {
    return Some(0x60 + u16::from(digit as u8 - b'0'));
  }
  Some(match code {
    "Backspace" => 0x08,
    "Tab" => 0x09,
    "Enter" | "NumpadEnter" => 0x0D,
    "Pause" => 0x13,
    "CapsLock" => 0x14,
    "Escape" => 0x1B,
    "Space" => 0x20,
    "PageUp" => 0x21,
    "PageDown" => 0x22,
    "End" => 0x23,
    "Home" => 0x24,
    "ArrowLeft" => 0x25,
    "ArrowUp" => 0x26,
    "ArrowRight" => 0x27,
    "ArrowDown" => 0x28,
    "PrintScreen" => 0x2C,
    "Insert" => 0x2D,
    "Delete" => 0x2E,
    "Help" => 0x2F,
    "MetaLeft" => 0x5B,
    "MetaRight" => 0x5C,
    "ContextMenu" => 0x5D,
    "NumpadMultiply" | "NumpadStar" => 0x6A,
    "NumpadAdd" => 0x6B,
    "NumpadSubtract" => 0x6D,
    "NumpadDecimal" => 0x6E,
    "NumpadDivide" => 0x6F,
    "NumLock" => 0x90,
    "ScrollLock" => 0x91,
    "ShiftLeft" => 0xA0,
    "ShiftRight" => 0xA1,
    "ControlLeft" => 0xA2,
    "ControlRight" => 0xA3,
    "AltLeft" => 0xA4,
    "AltRight" => 0xA5,
    "BrowserBack" => 0xA6,
    "BrowserForward" => 0xA7,
    "BrowserRefresh" => 0xA8,
    "BrowserStop" => 0xA9,
    "BrowserSearch" => 0xAA,
    "BrowserFavorites" => 0xAB,
    "BrowserHome" => 0xAC,
    "AudioVolumeMute" => 0xAD,
    "AudioVolumeDown" => 0xAE,
    "AudioVolumeUp" => 0xAF,
    "MediaTrackNext" => 0xB0,
    "MediaTrackPrevious" => 0xB1,
    "MediaStop" => 0xB2,
    "MediaPlayPause" => 0xB3,
    "LaunchMail" => 0xB4,
    "MediaSelect" => 0xB5,
    "LaunchApp1" => 0xB6,
    "LaunchApp2" => 0xB7,
    "Semicolon" => 0xBA,
    "Equal" => 0xBB,
    "Comma" => 0xBC,
    "Minus" => 0xBD,
    "Period" => 0xBE,
    "Slash" => 0xBF,
    "Backquote" => 0xC0,
    "BracketLeft" => 0xDB,
    "Backslash" => 0xDC,
    "BracketRight" => 0xDD,
    "Quote" => 0xDE,
    _ => return None,
  })
}

#[repr(C)]
struct Keyboard {
  base: EventTarget,
}

unsafe impl v8::cppgc::GarbageCollected for Keyboard {
  fn trace(&self, visitor: &mut v8::cppgc::Visitor) {
    v8::cppgc::GarbageCollected::trace(&self.base, visitor);
  }

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"Keyboard"
  }
}

#[op(inherit = EventTarget)]
impl Keyboard {
  #[constructor]
  fn new() -> Result<Self, OpError> {
    Err(OpError::type_("Illegal constructor"))
  }

  #[promise]
  #[length(0)]
  fn lock(
    &self,
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Js {
    let _ = self;
    let requested = match read_codes(scope, args) {
      Ok(codes) => codes,
      Err(error) => return reject(scope, error),
    };
    let outcome = STATE.with(|slot| {
      let mut state = slot.borrow_mut();
      let focused = crate::activation::focused_id().is_some();
      reserve(focused, requested, &mut state.reservation)
    });
    match outcome {
      Ok(()) => {
        sync();
        resolve(scope)
      }
      Err(LockReject::Unfocused) => {
        reject(scope, OpError::dom("InvalidStateError", NO_FOCUS))
      }
      Err(LockReject::Invalid) => {
        sync();
        reject(scope, OpError::dom("InvalidAccessError", BAD_CODE))
      }
    }
  }

  fn unlock(&self) {
    let _ = self;
    release_lock();
  }
}

fn read_codes(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
) -> Result<Vec<String>, OpError> {
  if args.length() == 0 || args.get(0).is_undefined() {
    return Ok(Vec::new());
  }
  let value = args.get(0);
  if !value.is_object() && !value.is_string() {
    return Err(OpError::type_(NOT_SEQUENCE));
  }
  let object = to_object(scope, value)?;
  let symbol = v8::Symbol::get_iterator(scope);
  let method = get_value(scope, object, symbol.into())?;
  let Ok(method) = v8::Local::<v8::Function>::try_from(method) else {
    return Err(OpError::type_(NOT_SEQUENCE));
  };
  let iterator = call(scope, method, object.into(), &[])?;
  let Ok(iterator) = v8::Local::<v8::Object>::try_from(iterator) else {
    return Err(OpError::type_(NOT_SEQUENCE));
  };
  let mut codes = Vec::new();
  loop {
    let next = get_name(scope, iterator, "next")?;
    let Ok(next) = v8::Local::<v8::Function>::try_from(next) else {
      return Err(OpError::type_(NOT_SEQUENCE));
    };
    let step = call(scope, next, iterator.into(), &[])?;
    let Ok(step) = v8::Local::<v8::Object>::try_from(step) else {
      return Err(OpError::type_(NOT_SEQUENCE));
    };
    let done = get_name(scope, step, "done")?;
    if done.boolean_value(scope) {
      return Ok(codes);
    }
    if codes.len() == MAX_CODES {
      return Err(OpError::type_(TOO_LONG));
    }
    let item = get_name(scope, step, "value")?;
    codes.push(String::from_v8(scope, item)?);
  }
}

enum Grab<T> {
  Value(T),
  Thrown(v8::Global<v8::Value>),
  Missing,
}

fn to_object<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  value: v8::Local<v8::Value>,
) -> Result<v8::Local<'s, v8::Object>, OpError> {
  let captured = {
    v8::tc_scope!(let tc, scope);
    match value.to_object(tc) {
      Some(object) => Grab::Value(v8::Global::new(tc, object)),
      None => match tc.exception() {
        Some(exception) => Grab::Thrown(v8::Global::new(tc, exception)),
        None => Grab::Missing,
      },
    }
  };
  match captured {
    Grab::Value(object) => Ok(v8::Local::new(scope, object)),
    Grab::Thrown(exception) => {
      Err(OpError::thrown(scope, v8::Local::new(scope, exception)))
    }
    Grab::Missing => Err(OpError::type_(NOT_SEQUENCE)),
  }
}

fn get_name<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  object: v8::Local<v8::Object>,
  name: &str,
) -> Result<v8::Local<'s, v8::Value>, OpError> {
  let key = v8::String::new(scope, name).unwrap();
  get_value(scope, object, key.into())
}

fn get_value<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  object: v8::Local<v8::Object>,
  key: v8::Local<v8::Value>,
) -> Result<v8::Local<'s, v8::Value>, OpError> {
  let captured = {
    v8::tc_scope!(let tc, scope);
    match object.get(tc, key) {
      Some(value) => Grab::Value(v8::Global::new(tc, value)),
      None => match tc.exception() {
        Some(exception) => Grab::Thrown(v8::Global::new(tc, exception)),
        None => Grab::Missing,
      },
    }
  };
  match captured {
    Grab::Value(value) => Ok(v8::Local::new(scope, value)),
    Grab::Thrown(exception) => {
      Err(OpError::thrown(scope, v8::Local::new(scope, exception)))
    }
    Grab::Missing => Err(OpError::type_(NOT_SEQUENCE)),
  }
}

fn call<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  function: v8::Local<v8::Function>,
  receiver: v8::Local<v8::Value>,
  args: &[v8::Local<v8::Value>],
) -> Result<v8::Local<'s, v8::Value>, OpError> {
  let captured = {
    v8::tc_scope!(let tc, scope);
    match function.call(tc, receiver, args) {
      Some(value) => Grab::Value(v8::Global::new(tc, value)),
      None => match tc.exception() {
        Some(exception) => Grab::Thrown(v8::Global::new(tc, exception)),
        None => Grab::Missing,
      },
    }
  };
  match captured {
    Grab::Value(value) => Ok(v8::Local::new(scope, value)),
    Grab::Thrown(exception) => {
      Err(OpError::thrown(scope, v8::Local::new(scope, exception)))
    }
    Grab::Missing => Err(OpError::type_(NOT_SEQUENCE)),
  }
}

fn reject(scope: &mut v8::PinScope, error: OpError) -> Js {
  let resolver = v8::PromiseResolver::new(scope).unwrap();
  let promise = resolver.get_promise(scope);
  let value = exception_value(scope, error);
  resolver.reject(scope, value);
  let promise: v8::Local<v8::Value> = promise.into();
  Js(v8::Global::new(scope, promise))
}

fn resolve(scope: &mut v8::PinScope) -> Js {
  let resolver = v8::PromiseResolver::new(scope).unwrap();
  let promise = resolver.get_promise(scope);
  resolver.resolve(scope, v8::undefined(scope).into());
  let promise: v8::Local<v8::Value> = promise.into();
  Js(v8::Global::new(scope, promise))
}

fn exception_value<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  error: OpError,
) -> v8::Local<'s, v8::Value> {
  match error {
    OpError::Type(message) => {
      let message = v8::String::new(scope, &message).unwrap();
      v8::Exception::type_error(scope, message)
    }
    OpError::Range(message) => {
      let message = v8::String::new(scope, &message).unwrap();
      v8::Exception::range_error(scope, message)
    }
    OpError::Dom { name, message } => {
      yuzuko_core::op::new_dom_exception(scope, &name, &message)
        .map(v8::Local::<v8::Value>::from)
        .unwrap_or_else(|| {
          let message = v8::String::new(scope, &message).unwrap();
          v8::Exception::type_error(scope, message)
        })
    }
    OpError::Thrown(value) => v8::Local::new(scope, *value),
  }
}

#[cfg(target_os = "macos")]
mod platform {
  use std::ffi::c_void;

  /// `kHIHotKeyModeAllDisabled` from CarbonEvents.h.
  const ALL_DISABLED: u32 = 1;

  // The symbols are implemented in HIToolbox. The Carbon umbrella exports
  // them. `-framework HIToolbox` does not search that nested framework.
  #[link(name = "Carbon", kind = "framework")]
  unsafe extern "C" {
    fn PushSymbolicHotKeyMode(mode: u32) -> *mut c_void;
    fn PopSymbolicHotKeyMode(token: *mut c_void);
  }

  pub(super) struct Hold {
    token: *mut c_void,
  }

  impl Drop for Hold {
    fn drop(&mut self) {
      if self.token.is_null() {
        return;
      }
      unsafe { PopSymbolicHotKeyMode(self.token) };
      self.token = std::ptr::null_mut();
    }
  }

  pub(super) fn wants(codes: &[String]) -> bool {
    super::suppresses_symbolic(codes)
  }

  pub(super) fn retarget(_all: bool, _codes: &[String]) {}

  /// Pushes the symbolic hotkey mode. Headless callers never reach this.
  /// The mode reverts when the process is no longer frontmost, and `Drop`
  /// pops it. Command-Tab stays with the system.
  pub(super) fn acquire(_id: u64, _codes: &[String]) -> Option<Hold> {
    if crate::session::headless() {
      return None;
    }
    let token = unsafe { PushSymbolicHotKeyMode(ALL_DISABLED) };
    if token.is_null() {
      None
    } else {
      Some(Hold { token })
    }
  }
}

#[cfg(target_os = "windows")]
mod platform {
  use std::ffi::c_void;
  use std::sync::Mutex;
  use std::sync::atomic::AtomicPtr;
  use std::sync::atomic::Ordering;

  use windows_sys::Win32::Foundation::HWND;
  use windows_sys::Win32::Foundation::LPARAM;
  use windows_sys::Win32::Foundation::LRESULT;
  use windows_sys::Win32::Foundation::WPARAM;
  use windows_sys::Win32::UI::WindowsAndMessaging::CallNextHookEx;
  use windows_sys::Win32::UI::WindowsAndMessaging::GetForegroundWindow;
  use windows_sys::Win32::UI::WindowsAndMessaging::KBDLLHOOKSTRUCT;
  use windows_sys::Win32::UI::WindowsAndMessaging::LLKHF_ALTDOWN;
  use windows_sys::Win32::UI::WindowsAndMessaging::LLKHF_EXTENDED;
  use windows_sys::Win32::UI::WindowsAndMessaging::LLKHF_INJECTED;
  use windows_sys::Win32::UI::WindowsAndMessaging::LLKHF_LOWER_IL_INJECTED;
  use windows_sys::Win32::UI::WindowsAndMessaging::LLKHF_UP;
  use windows_sys::Win32::UI::WindowsAndMessaging::PostMessageW;
  use windows_sys::Win32::UI::WindowsAndMessaging::SetWindowsHookExW;
  use windows_sys::Win32::UI::WindowsAndMessaging::UnhookWindowsHookEx;
  use windows_sys::Win32::UI::WindowsAndMessaging::WH_KEYBOARD_LL;

  const INJECTED: u32 = LLKHF_INJECTED | LLKHF_LOWER_IL_INJECTED;

  struct Target {
    hwnd: isize,
    all: bool,
    vks: Vec<u16>,
  }

  static TARGET: Mutex<Target> = Mutex::new(Target {
    hwnd: 0,
    all: false,
    vks: Vec::new(),
  });
  static HOOK: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());

  pub(super) struct Hold;

  impl Drop for Hold {
    fn drop(&mut self) {
      let mut target = lock_target();
      target.hwnd = 0;
      target.all = false;
      target.vks.clear();
      drop(target);
      let hook = HOOK.swap(std::ptr::null_mut(), Ordering::AcqRel);
      if !hook.is_null() {
        unsafe { UnhookWindowsHookEx(hook) };
      }
    }
  }

  pub(super) fn wants(_codes: &[String]) -> bool {
    true
  }

  pub(super) fn retarget(all: bool, codes: &[String]) {
    let mut target = lock_target();
    target.all = all;
    target.vks = virtual_keys(codes);
  }

  /// Installs a low-level hook. A failure still lets `lock` resolve.
  /// The hook never runs from a headless test.
  pub(super) fn acquire(id: u64, codes: &[String]) -> Option<Hold> {
    if crate::session::headless() || crate::activation::focused_id() != Some(id)
    {
      return None;
    }
    let Some(hwnd) = crate::os::focused_hwnd() else {
      return None;
    };
    {
      let mut target = lock_target();
      target.hwnd = hwnd;
      target.all = codes.is_empty();
      target.vks = virtual_keys(codes);
    }
    if !ensure_hook() {
      let mut target = lock_target();
      target.hwnd = 0;
      target.all = false;
      target.vks.clear();
      return None;
    }
    Some(Hold)
  }

  fn virtual_keys(codes: &[String]) -> Vec<u16> {
    let mut keys = Vec::new();
    for code in codes {
      if let Some(vk) = super::virtual_key(code) {
        keys.push(vk);
      }
    }
    keys.sort_unstable();
    keys.dedup();
    keys
  }

  fn lock_target() -> std::sync::MutexGuard<'static, Target> {
    TARGET.lock().unwrap_or_else(|error| error.into_inner())
  }

  fn ensure_hook() -> bool {
    if !HOOK.load(Ordering::Acquire).is_null() {
      return true;
    }
    let hook = unsafe {
      SetWindowsHookExW(
        WH_KEYBOARD_LL,
        Some(hook_proc),
        std::ptr::null_mut(),
        0,
      )
    };
    if hook.is_null() {
      return false;
    }
    let previous = HOOK.swap(hook, Ordering::AcqRel);
    if !previous.is_null() {
      unsafe { UnhookWindowsHookEx(previous) };
    }
    true
  }

  /// Runs on the installing thread during the OS pump. It must not borrow
  /// the window map or enter V8. Keys are swallowed only while our window
  /// is in the foreground, and then posted back so the window still sees
  /// them. Ctrl+Alt+Delete never reaches this hook.
  unsafe extern "system" fn hook_proc(
    code: i32,
    wparam: WPARAM,
    lparam: LPARAM,
  ) -> LRESULT {
    if code < 0 || lparam == 0 {
      return forward(code, wparam, lparam);
    }
    let info = unsafe { &*(lparam as *const KBDLLHOOKSTRUCT) };
    if info.flags & INJECTED != 0 {
      return forward(code, wparam, lparam);
    }
    let Some(hwnd) = captured_hwnd(info) else {
      return forward(code, wparam, lparam);
    };
    let posted = unsafe {
      PostMessageW(hwnd, wparam as u32, info.vkCode as WPARAM, key_lparam(info))
    };
    if posted == 0 {
      return forward(code, wparam, lparam);
    }
    1
  }

  fn captured_hwnd(info: &KBDLLHOOKSTRUCT) -> Option<HWND> {
    let target = TARGET.try_lock().ok()?;
    if target.hwnd == 0 {
      return None;
    }
    let foreground = unsafe { GetForegroundWindow() };
    if foreground.is_null() || foreground as isize != target.hwnd {
      return None;
    }
    let vk = info.vkCode as u16;
    if target.all || target.vks.binary_search(&vk).is_ok() {
      Some(target.hwnd as HWND)
    } else {
      None
    }
  }

  fn key_lparam(info: &KBDLLHOOKSTRUCT) -> LPARAM {
    let mut value: i64 = 1;
    value |= i64::from(info.scanCode & 0xFF) << 16;
    if info.flags & LLKHF_EXTENDED != 0 {
      value |= 1 << 24;
    }
    if info.flags & LLKHF_ALTDOWN != 0 {
      value |= 1 << 29;
    }
    if info.flags & LLKHF_UP != 0 {
      value |= 1 << 30;
      value |= 1 << 31;
    }
    value as LPARAM
  }

  fn forward(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) }
  }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
mod platform {
  use std::ffi::CStr;
  use std::ffi::c_char;
  use std::ffi::c_ulong;
  use std::ffi::c_void;
  use std::sync::OnceLock;

  use winit::raw_window_handle::HasDisplayHandle;
  use winit::raw_window_handle::HasWindowHandle;
  use winit::raw_window_handle::RawDisplayHandle;
  use winit::raw_window_handle::RawWindowHandle;

  const OWNER_EVENTS: i32 = 1;
  const GRAB_MODE_ASYNC: i32 = 1;
  const CURRENT_TIME: c_ulong = 0;
  const GRAB_SUCCESS: i32 = 0;
  const RTLD_LAZY: i32 = 1;

  type Grab =
    unsafe extern "C" fn(*mut c_void, c_ulong, i32, i32, i32, c_ulong) -> i32;
  type Ungrab = unsafe extern "C" fn(*mut c_void, c_ulong) -> i32;
  type Flush = unsafe extern "C" fn(*mut c_void) -> i32;

  struct X11 {
    grab: Grab,
    ungrab: Ungrab,
    flush: Flush,
  }

  #[link(name = "dl")]
  unsafe extern "C" {
    fn dlopen(filename: *const c_char, flags: i32) -> *mut c_void;
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
  }

  pub(super) struct Hold {
    display: *mut c_void,
  }

  impl Drop for Hold {
    fn drop(&mut self) {
      if self.display.is_null() {
        return;
      }
      if let Some(x11) = x11() {
        unsafe {
          (x11.ungrab)(self.display, CURRENT_TIME);
          (x11.flush)(self.display);
        }
      }
      self.display = std::ptr::null_mut();
    }
  }

  pub(super) fn wants(codes: &[String]) -> bool {
    codes.is_empty()
  }

  pub(super) fn retarget(_all: bool, _codes: &[String]) {}

  /// Grabs the X11 keyboard for an empty code list. Wayland and a failed
  /// grab leave the reservation in place without an inhibitor. The library
  /// handle is leaked so a later ungrab can still call `XUngrabKeyboard`.
  pub(super) fn acquire(id: u64, codes: &[String]) -> Option<Hold> {
    if crate::session::headless()
      || !codes.is_empty()
      || crate::activation::focused_id() != Some(id)
    {
      return None;
    }
    let (display, window) = x11_window(id)?;
    let x11 = x11()?;
    let status = unsafe {
      (x11.grab)(
        display,
        window,
        OWNER_EVENTS,
        GRAB_MODE_ASYNC,
        GRAB_MODE_ASYNC,
        CURRENT_TIME,
      )
    };
    if status != GRAB_SUCCESS {
      return None;
    }
    unsafe { (x11.flush)(display) };
    Some(Hold { display })
  }

  fn x11_window(id: u64) -> Option<(*mut c_void, c_ulong)> {
    crate::os::with_window(id, |window| {
      let display = window.display_handle().ok()?;
      let handle = window.window_handle().ok()?;
      match (display.as_raw(), handle.as_raw()) {
        (RawDisplayHandle::Xlib(display), RawWindowHandle::Xlib(window)) => {
          let display = display.display?.as_ptr();
          Some((display, window.window))
        }
        _ => None,
      }
    })
    .flatten()
  }

  fn x11() -> Option<&'static X11> {
    static LIB: OnceLock<Option<X11>> = OnceLock::new();
    LIB.get_or_init(load_x11).as_ref()
  }

  fn load_x11() -> Option<X11> {
    let handle =
      open_library(c"libX11.so.6").or_else(|| open_library(c"libX11.so"))?;
    let grab = symbol(handle, c"XGrabKeyboard")?;
    let ungrab = symbol(handle, c"XUngrabKeyboard")?;
    let flush = symbol(handle, c"XFlush")?;
    Some(X11 {
      grab,
      ungrab,
      flush,
    })
  }

  fn open_library(name: &CStr) -> Option<*mut c_void> {
    let handle = unsafe { dlopen(name.as_ptr(), RTLD_LAZY) };
    if handle.is_null() { None } else { Some(handle) }
  }

  fn symbol<T>(handle: *mut c_void, name: &CStr) -> Option<T> {
    let pointer = unsafe { dlsym(handle, name.as_ptr()) };
    if pointer.is_null() {
      return None;
    }
    Some(unsafe { std::mem::transmute_copy(&pointer) })
  }
}

#[cfg(test)]
mod tests {
  use super::LockReject;
  use super::Reservation;
  use super::normalize;
  use super::reserve;
  use super::suppresses_symbolic;
  use super::virtual_key;

  #[test]
  fn symbolic_hotkeys_follow_the_reserved_set() {
    assert!(suppresses_symbolic(&[]));
    assert!(!suppresses_symbolic(&["KeyW".to_string()]));
    assert!(suppresses_symbolic(&["Escape".to_string()]));
    assert!(suppresses_symbolic(&[
      "KeyW".to_string(),
      "Tab".to_string()
    ]));
    assert!(suppresses_symbolic(&["Space".to_string()]));
    assert!(suppresses_symbolic(&["MetaLeft".to_string()]));
    assert!(suppresses_symbolic(&["MetaRight".to_string()]));
  }

  #[test]
  fn virtual_keys_cover_letters_and_function_keys() {
    assert_eq!(virtual_key("KeyW"), Some(0x57));
    assert_eq!(virtual_key("Escape"), Some(0x1B));
    assert_eq!(virtual_key("Tab"), Some(0x09));
    assert_eq!(virtual_key("MetaLeft"), Some(0x5B));
    assert_eq!(virtual_key("F1"), Some(0x70));
    assert_eq!(virtual_key("F24"), Some(0x87));
    assert_eq!(virtual_key("F25"), None);
    assert_eq!(virtual_key("Nope"), None);
    assert_eq!(virtual_key("Unidentified"), None);
  }

  #[test]
  fn normalize_dedups_and_rejects_unknown_codes() {
    let codes = normalize(vec![
      "KeyW".to_string(),
      "KeyW".to_string(),
      "Escape".to_string(),
    ])
    .unwrap();
    assert_eq!(codes, vec!["KeyW", "Escape"]);
    assert!(normalize(vec!["Nope".to_string()]).is_err());
  }

  #[test]
  fn an_unfocused_invalid_code_keeps_the_reservation() {
    let mut reservation = Reservation {
      enabled: true,
      codes: vec!["KeyW".to_string()],
    };
    let error = reserve(false, vec!["Nope".to_string()], &mut reservation);
    assert!(matches!(error, Err(LockReject::Unfocused)));
    assert!(reservation.enabled);
    assert_eq!(reservation.codes, vec!["KeyW"]);

    let error = reserve(true, vec!["Nope".to_string()], &mut reservation);
    assert!(matches!(error, Err(LockReject::Invalid)));
    assert!(!reservation.enabled);
    assert!(reservation.codes.is_empty());

    reserve(
      true,
      vec!["KeyA".to_string(), "KeyA".to_string()],
      &mut reservation,
    )
    .unwrap();
    assert!(reservation.enabled);
    assert_eq!(reservation.codes, vec!["KeyA"]);
  }
}
