//! `navigator` on the main global.
//!
//! The core runtime installs a plain object with `userAgent`, `platform`,
//! and `hardwareConcurrency`. This replaces it with a `Navigator` and copies
//! those values. `language`, `languages`, and the compatibility getters stay
//! on the prototype.

use yuzuko_core::op::Js;
use yuzuko_core::op::OpError;
use yuzuko_macros::op;

pub(crate) fn install(scope: &mut v8::PinScope) {
  Navigator::install(scope);
  let global = scope.get_current_context().global(scope);
  let key = v8::String::new(scope, "navigator").unwrap();
  let previous = global.get(scope, key.into());
  let navigator = unsafe { yuzuko_core::op::wrap_instance(scope, Navigator) };
  if let Some(previous) =
    previous.and_then(|value| v8::Local::<v8::Object>::try_from(value).ok())
  {
    for name in ["userAgent", "platform", "hardwareConcurrency"] {
      let name = v8::String::new(scope, name).unwrap();
      if let Some(value) = previous.get(scope, name.into()) {
        navigator.set(scope, name.into(), value);
      }
    }
  }
  global.set(scope, key.into(), navigator.into());
}

struct Navigator;

unsafe impl v8::cppgc::GarbageCollected for Navigator {
  fn trace(&self, _visitor: &mut v8::cppgc::Visitor) {}

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"Navigator"
  }
}

#[op]
impl Navigator {
  #[constructor]
  fn new() -> Result<Self, OpError> {
    Err(OpError::type_("Illegal constructor"))
  }

  #[getter]
  fn clipboard(&self, scope: &mut v8::PinScope) -> Js {
    let _ = self;
    crate::shell::clipboard::instance(scope)
  }

  #[getter]
  fn wake_lock(&self, scope: &mut v8::PinScope) -> Js {
    let _ = self;
    crate::wake::instance(scope)
  }

  #[getter]
  fn keyboard(&self, scope: &mut v8::PinScope) -> Js {
    let _ = self;
    crate::input::keyboard::instance(scope)
  }

  #[getter]
  fn max_touch_points(&self) -> i32 {
    let _ = self;
    crate::input::touch::max_touch_points()
  }

  #[getter]
  fn language(&self) -> String {
    let _ = self;
    yuzuko_core::primary_language()
  }

  #[getter]
  fn languages(&self, scope: &mut v8::PinScope) -> Js {
    let _ = self;
    let array = yuzuko_core::languages_array(scope);
    let value: v8::Local<v8::Value> = array.into();
    Js(v8::Global::new(scope, value))
  }

  #[getter]
  fn app_code_name(&self) -> &'static str {
    let _ = self;
    "Mozilla"
  }

  #[getter]
  fn app_name(&self) -> &'static str {
    let _ = self;
    "Netscape"
  }

  #[getter]
  fn app_version(&self) -> &'static str {
    let _ = self;
    "5.0"
  }

  #[getter]
  fn product(&self) -> &'static str {
    let _ = self;
    "Gecko"
  }

  #[getter]
  fn on_line(&self) -> bool {
    let _ = self;
    true
  }

  fn get_gamepads(&self, scope: &mut v8::PinScope) -> Js {
    let _ = self;
    crate::input::gamepad::get_gamepads(scope)
  }
}
