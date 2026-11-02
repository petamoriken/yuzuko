//! `navigator.wakeLock` on the main global.
//!
//! Sentinels share one platform assertion. The first `request("screen")`
//! asks the operating system to keep the display awake. The last `release()`
//! drops that assertion.

use std::cell::Cell;
use std::cell::RefCell;

use yuzuko_core::op::Js;
use yuzuko_core::op::traced;
use yuzuko_core::op::traced_local;
use yuzuko_dom::EventTarget;
use yuzuko_macros::op;

const INVALID_TYPE: &str = "The wake lock type is invalid";
const REFUSED: &str = "The platform refused the screen wake lock";

struct ScreenLock {
  count: usize,
  hold: Option<platform::Hold>,
}

std::thread_local! {
  static SCREEN: RefCell<ScreenLock> = RefCell::new(ScreenLock {
    count: 0,
    hold: None,
  });
  static WAKE_LOCK: RefCell<Option<v8::Global<v8::Object>>> =
    const { RefCell::new(None) };
}

pub fn install(scope: &mut v8::PinScope) {
  WakeLock::install(scope);
  WakeLockSentinel::install(scope);
}

pub fn instance(scope: &mut v8::PinScope) -> Js {
  let existing = WAKE_LOCK.with(|slot| {
    slot
      .borrow()
      .as_ref()
      .map(|value| v8::Local::new(scope, value))
  });
  let object = existing.unwrap_or_else(|| {
    let object = unsafe { yuzuko_core::op::wrap_instance(scope, WakeLock) };
    WAKE_LOCK.with(|slot| {
      *slot.borrow_mut() = Some(v8::Global::new(scope, object));
    });
    object
  });
  js_object(scope, object)
}

/// Drops the platform assertion when the realm goes away.
pub fn clear() {
  SCREEN.with(|slot| {
    let mut screen = slot.borrow_mut();
    screen.count = 0;
    screen.hold = None;
  });
  WAKE_LOCK.with(|slot| *slot.borrow_mut() = None);
}

fn acquire_hold() -> Result<(), ()> {
  SCREEN.with(|slot| {
    let mut screen = slot.borrow_mut();
    if screen.count == 0 {
      screen.hold = Some(platform::Hold::acquire()?);
    }
    screen.count += 1;
    Ok(())
  })
}

fn release_hold() {
  SCREEN.with(|slot| {
    let mut screen = slot.borrow_mut();
    if screen.count == 0 {
      return;
    }
    screen.count -= 1;
    if screen.count == 0 {
      screen.hold = None;
    }
  });
}

struct WakeLock;

unsafe impl v8::cppgc::GarbageCollected for WakeLock {
  fn trace(&self, _visitor: &mut v8::cppgc::Visitor) {}

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"WakeLock"
  }
}

#[op]
impl WakeLock {
  #[constructor]
  fn new() -> Result<Self, yuzuko_core::op::OpError> {
    Err(yuzuko_core::op::OpError::type_("Illegal constructor"))
  }

  #[promise]
  fn request(&self, scope: &mut v8::PinScope, type_name: Option<String>) -> Js {
    let _ = self;
    let type_name = type_name.unwrap_or_else(|| "screen".to_string());
    if type_name != "screen" {
      return reject(scope, "TypeError", INVALID_TYPE);
    }
    if acquire_hold().is_err() {
      return reject(scope, "NotAllowedError", REFUSED);
    }
    let sentinel = WakeLockSentinel {
      base: EventTarget::new(),
      released: Cell::new(false),
      on_release: RefCell::new(None),
    };
    let object = unsafe { yuzuko_core::op::wrap_instance(scope, sentinel) };
    resolve(scope, object.into())
  }
}

#[repr(C)]
struct WakeLockSentinel {
  base: EventTarget,
  released: Cell<bool>,
  on_release: RefCell<Option<v8::TracedReference<v8::Value>>>,
}

unsafe impl v8::cppgc::GarbageCollected for WakeLockSentinel {
  fn trace(&self, visitor: &mut v8::cppgc::Visitor) {
    v8::cppgc::GarbageCollected::trace(&self.base, visitor);
    if let Ok(slot) = self.on_release.try_borrow() {
      visitor.trace(&*slot);
    }
  }

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"WakeLockSentinel"
  }
}

#[op(inherit = EventTarget)]
impl WakeLockSentinel {
  #[constructor]
  fn new() -> Result<Self, yuzuko_core::op::OpError> {
    Err(yuzuko_core::op::OpError::type_("Illegal constructor"))
  }

  #[getter]
  fn released(&self) -> bool {
    self.released.get()
  }

  #[getter]
  #[rename("type")]
  fn wake_type(&self) -> String {
    let _ = self;
    "screen".to_string()
  }

  #[promise]
  fn release(
    &self,
    scope: &mut v8::PinScope,
    #[this] this: v8::Local<v8::Object>,
  ) -> Js {
    if !self.released.replace(true) {
      release_hold();
      let _ = yuzuko_dom::dispatch_simple(scope, this, "release", false, false);
    }
    resolve(scope, v8::undefined(scope).into())
  }

  #[getter]
  fn onrelease(&self, scope: &mut v8::PinScope) -> Js {
    handler_value(scope, &self.on_release)
  }

  #[setter]
  fn onrelease(&self, scope: &mut v8::PinScope, value: v8::Local<v8::Value>) {
    set_handler(scope, &self.base, &self.on_release, "release", value);
  }
}

fn handler_value(
  scope: &mut v8::PinScope,
  slot: &RefCell<Option<v8::TracedReference<v8::Value>>>,
) -> Js {
  let value = slot
    .borrow()
    .as_ref()
    .and_then(|reference| traced_local(scope, reference))
    .unwrap_or_else(|| v8::null(scope).into());
  Js(v8::Global::new(scope, value))
}

fn set_handler(
  scope: &mut v8::PinScope,
  target: &EventTarget,
  slot: &RefCell<Option<v8::TracedReference<v8::Value>>>,
  type_name: &str,
  value: v8::Local<v8::Value>,
) {
  if let Some(previous) = slot.borrow().as_ref()
    && let Some(previous) = traced_local(scope, previous)
  {
    target.remove_callback(scope, type_name, previous, false);
  }
  if value.is_function() {
    target.add_callback(scope, type_name, value, false, false, false);
    *slot.borrow_mut() = Some(traced(scope, value));
  } else {
    *slot.borrow_mut() = None;
  }
}

fn reject(scope: &mut v8::PinScope, name: &str, message: &str) -> Js {
  let resolver = v8::PromiseResolver::new(scope).unwrap();
  let promise = resolver.get_promise(scope);
  let error = if name == "TypeError" {
    let message = v8::String::new(scope, message).unwrap();
    v8::Exception::type_error(scope, message)
  } else {
    yuzuko_core::op::new_dom_exception(scope, name, message)
      .map(v8::Local::<v8::Value>::from)
      .unwrap_or_else(|| {
        let message = v8::String::new(scope, message).unwrap();
        v8::Exception::type_error(scope, message)
      })
  };
  resolver.reject(scope, error);
  let promise: v8::Local<v8::Value> = promise.into();
  Js(v8::Global::new(scope, promise))
}

fn resolve(scope: &mut v8::PinScope, value: v8::Local<v8::Value>) -> Js {
  let resolver = v8::PromiseResolver::new(scope).unwrap();
  let promise = resolver.get_promise(scope);
  resolver.resolve(scope, value);
  let promise: v8::Local<v8::Value> = promise.into();
  Js(v8::Global::new(scope, promise))
}

fn js_object(scope: &mut v8::PinScope, object: v8::Local<v8::Object>) -> Js {
  let value: v8::Local<v8::Value> = object.into();
  Js(v8::Global::new(scope, value))
}

#[cfg(target_os = "macos")]
mod platform {
  use std::ffi::c_char;
  use std::ffi::c_void;

  const LEVEL_ON: u32 = 255;
  const SUCCESS: i32 = 0;
  const UTF8: u32 = 0x0800_0100;

  #[link(name = "CoreFoundation", kind = "framework")]
  unsafe extern "C" {
    fn CFStringCreateWithCString(
      allocator: *const c_void,
      string: *const c_char,
      encoding: u32,
    ) -> *const c_void;

    fn CFRelease(value: *const c_void);
  }

  #[link(name = "IOKit", kind = "framework")]
  unsafe extern "C" {
    fn IOPMAssertionCreateWithName(
      assertion_type: *const c_void,
      assertion_level: u32,
      assertion_name: *const c_void,
      assertion_id: *mut u32,
    ) -> i32;

    fn IOPMAssertionRelease(assertion_id: u32) -> i32;
  }

  pub struct Hold {
    id: u32,
  }

  impl Hold {
    pub fn acquire() -> Result<Self, ()> {
      unsafe {
        let assertion_type = cf_string(c"PreventUserIdleDisplaySleep");
        let name = cf_string(c"Yuzuko");
        if assertion_type.is_null() || name.is_null() {
          release_cf(assertion_type);
          release_cf(name);
          return Err(());
        }
        let mut id = 0;
        let result =
          IOPMAssertionCreateWithName(assertion_type, LEVEL_ON, name, &mut id);
        CFRelease(assertion_type);
        CFRelease(name);
        if result == SUCCESS {
          Ok(Self { id })
        } else {
          Err(())
        }
      }
    }
  }

  impl Drop for Hold {
    fn drop(&mut self) {
      unsafe {
        IOPMAssertionRelease(self.id);
      }
    }
  }

  fn cf_string(text: &std::ffi::CStr) -> *const c_void {
    unsafe { CFStringCreateWithCString(std::ptr::null(), text.as_ptr(), UTF8) }
  }

  fn release_cf(value: *const c_void) {
    if !value.is_null() {
      unsafe { CFRelease(value) };
    }
  }
}

#[cfg(windows)]
mod platform {
  use windows_sys::Win32::System::Power::ES_CONTINUOUS;
  use windows_sys::Win32::System::Power::ES_DISPLAY_REQUIRED;
  use windows_sys::Win32::System::Power::SetThreadExecutionState;

  pub struct Hold;

  impl Hold {
    pub fn acquire() -> Result<Self, ()> {
      let previous =
        unsafe { SetThreadExecutionState(ES_CONTINUOUS | ES_DISPLAY_REQUIRED) };
      if previous == 0 { Err(()) } else { Ok(Self) }
    }
  }

  impl Drop for Hold {
    fn drop(&mut self) {
      unsafe {
        SetThreadExecutionState(ES_CONTINUOUS);
      }
    }
  }
}

#[cfg(target_os = "linux")]
mod platform {
  use std::process::Child;
  use std::process::Command;
  use std::process::Stdio;

  pub struct Hold {
    child: Child,
  }

  impl Hold {
    pub fn acquire() -> Result<Self, ()> {
      let child = Command::new("systemd-inhibit")
        .args([
          "--what=idle:sleep",
          "--who=Yuzuko",
          "--why=screen wake lock",
          "--mode=block",
          "sleep",
          "infinity",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| ())?;
      Ok(Self { child })
    }
  }

  impl Drop for Hold {
    fn drop(&mut self) {
      let _ = self.child.kill();
      let _ = self.child.wait();
    }
  }
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
mod platform {
  pub struct Hold;

  impl Hold {
    pub fn acquire() -> Result<Self, ()> {
      Err(())
    }
  }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
  use std::process::Command;

  use super::acquire_hold;
  use super::release_hold;

  struct Holds(usize);

  impl Drop for Holds {
    fn drop(&mut self) {
      for _ in 0..self.0 {
        release_hold();
      }
    }
  }

  #[test]
  fn display_sleep_assertion_follows_the_sentinel_count() {
    let pid = std::process::id();
    let mut holds = Holds(0);
    assert_held(pid, false);
    acquire_hold().expect("first acquire");
    holds.0 = 1;
    assert_held(pid, true);
    acquire_hold().expect("second acquire");
    holds.0 = 2;
    assert_held(pid, true);
    release_hold();
    holds.0 = 1;
    assert_held(pid, true);
    release_hold();
    holds.0 = 0;
    assert_held(pid, false);
  }

  fn assert_held(pid: u32, held: bool) {
    let output = pmset();
    let actual = output.lines().any(|line| {
      line.contains(&format!("pid {pid}("))
        && line.contains("PreventUserIdleDisplaySleep")
        && line.contains("named: \"Yuzuko\"")
    });
    assert_eq!(actual, held, "{output}");
  }

  fn pmset() -> String {
    let output = Command::new("pmset")
      .args(["-g", "assertions"])
      .output()
      .expect("pmset");
    assert!(output.status.success(), "pmset failed");
    String::from_utf8_lossy(&output.stdout).into_owned()
  }
}
