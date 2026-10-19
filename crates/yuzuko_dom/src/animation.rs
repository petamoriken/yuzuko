//! `requestAnimationFrame` on a dedicated worker.

use std::cell::{Cell, RefCell};

use yuzuko_core::{performance_now_ms, schedule_callback};

struct Frame {
  id: i32,
  callback: v8::Global<v8::Function>,
}

thread_local! {
  static NEXT: Cell<i32> = const { Cell::new(1) };
  static FRAMES: RefCell<Vec<Frame>> = const { RefCell::new(Vec::new()) };
  static ARMED: Cell<bool> = const { Cell::new(false) };
}

pub fn install(scope: &mut v8::PinScope) {
  let global = yuzuko_core::global(scope);
  yuzuko_core::set_function(scope, global, "requestAnimationFrame", request);
  yuzuko_core::set_function(scope, global, "cancelAnimationFrame", cancel);
}

fn request(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut ret: v8::ReturnValue,
) {
  let Ok(callback) = v8::Local::<v8::Function>::try_from(args.get(0)) else {
    yuzuko_core::throw_type(scope, "requestAnimationFrame requires a callback");
    return;
  };
  let id = NEXT.with(|next| {
    let id = next.get().max(1);
    next.set(id.saturating_add(1));
    id
  });
  FRAMES.with(|frames| {
    frames.borrow_mut().push(Frame {
      id,
      callback: v8::Global::new(scope, callback),
    });
  });
  if !ARMED.with(Cell::get) {
    ARMED.with(|armed| armed.set(true));
    if let Some(wake) = v8::Function::builder(fire).build(scope) {
      schedule_callback(scope, wake, 16.0, &[]);
    }
  }
  ret.set(v8::Integer::new(scope, id).into());
}

fn cancel(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut _ret: v8::ReturnValue,
) {
  let Some(id) = args.get(0).int32_value(scope) else {
    return;
  };
  FRAMES.with(|frames| {
    frames.borrow_mut().retain(|frame| frame.id != id);
  });
}

fn fire(
  scope: &mut v8::PinScope,
  _args: v8::FunctionCallbackArguments,
  mut _ret: v8::ReturnValue,
) {
  ARMED.with(|armed| armed.set(false));
  let frames = FRAMES.with(|frames| std::mem::take(&mut *frames.borrow_mut()));
  for frame in frames {
    v8::tc_scope!(let scope, scope);
    let callback = v8::Local::new(scope, frame.callback);
    let time = v8::Number::new(scope, performance_now_ms());
    let global = yuzuko_core::global(scope);
    if callback
      .call(scope, global.into(), &[time.into()])
      .is_none()
      && let Some(error) = scope.exception()
    {
      crate::report::report_error(scope, error);
    }
  }
  yuzuko_core::run_worker_frame_hook(scope);
}
