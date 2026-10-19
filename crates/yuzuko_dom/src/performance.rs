//! `performance` is an `EventTarget`. `now` reads the realm time origin.

use yuzuko_core::op::Js;
use yuzuko_macros::op;

use crate::target::EventTarget;
use crate::util::key;

#[repr(C)]
pub struct Performance {
  base: EventTarget,
}

unsafe impl v8::cppgc::GarbageCollected for Performance {
  fn trace(&self, visitor: &mut v8::cppgc::Visitor) {
    v8::cppgc::GarbageCollected::trace(&self.base, visitor);
  }

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"Performance"
  }
}

#[op(inherit = EventTarget)]
impl Performance {
  #[getter]
  fn time_origin(&self) -> f64 {
    yuzuko_core::time_origin_ms()
  }

  fn now(&self) -> f64 {
    yuzuko_core::performance_now_ms()
  }

  #[rename("toJSON")]
  fn to_json(&self, scope: &mut v8::PinScope) -> Js {
    let object = v8::Object::new(scope);
    let origin = v8::Number::new(scope, yuzuko_core::time_origin_ms());
    object.set(scope, key(scope, "timeOrigin").into(), origin.into());
    let value: v8::Local<v8::Value> = object.into();
    Js(v8::Global::new(scope, value))
  }
}

pub(crate) fn install(scope: &mut v8::PinScope) {
  Performance::install(scope);
  let object = unsafe {
    yuzuko_core::op::wrap_instance(
      scope,
      Performance {
        base: EventTarget::new(),
      },
    )
  };
  let global = scope.get_current_context().global(scope);
  global.set(scope, key(scope, "performance").into(), object.into());
}
