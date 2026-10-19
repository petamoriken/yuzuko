//! `reportError`, `onerror`, and the promise rejection events.

use std::cell::{Cell, RefCell};

use crate::event::{self, ErrorEvent, Event, PromiseRejectionEvent};
use crate::target;
use crate::util::{self, key};

struct Handler {
  callback: Option<v8::Global<v8::Function>>,
  wrapper: Option<v8::Global<v8::Function>>,
}

impl Handler {
  const fn empty() -> Self {
    Self {
      callback: None,
      wrapper: None,
    }
  }
}

thread_local! {
  static REPORTING: Cell<bool> = const { Cell::new(false) };
  static CONSOLE: RefCell<Option<(v8::Global<v8::Object>, v8::Global<v8::Function>)>> =
    const { RefCell::new(None) };
  static ONERROR: RefCell<Handler> = const { RefCell::new(Handler::empty()) };
  static ONUNHANDLED: RefCell<Handler> =
    const { RefCell::new(Handler::empty()) };
  static ONHANDLED: RefCell<Handler> = const { RefCell::new(Handler::empty()) };
}

struct ReportingGuard;

impl Drop for ReportingGuard {
  fn drop(&mut self) {
    REPORTING.with(|flag| flag.set(false));
  }
}

pub(crate) fn install(scope: &mut v8::PinScope) {
  capture_console(scope);
  let global = scope.get_current_context().global(scope);
  let report = v8::Function::new(scope, report_error_callback).unwrap();
  let _ = global.define_own_property(
    scope,
    key(scope, "reportError").into(),
    report.into(),
    v8::PropertyAttribute::DONT_ENUM,
  );
  install_onerror(scope, global);
  install_handler(
    scope,
    global,
    "onunhandledrejection",
    get_unhandled,
    set_unhandled,
  );
  install_handler(
    scope,
    global,
    "onrejectionhandled",
    get_handled,
    set_handled,
  );
  let hook = v8::Function::new(scope, rejection_callback).unwrap();
  yuzuko_core::set_rejection_hook(scope, hook);
}

pub(crate) fn clear() {
  REPORTING.with(|flag| flag.set(false));
  CONSOLE.with(|slot| *slot.borrow_mut() = None);
  ONERROR.with(|slot| *slot.borrow_mut() = Handler::empty());
  ONUNHANDLED.with(|slot| *slot.borrow_mut() = Handler::empty());
  ONHANDLED.with(|slot| *slot.borrow_mut() = Handler::empty());
}

pub fn report_error(scope: &mut v8::PinScope, error: v8::Local<v8::Value>) {
  if REPORTING.with(|flag| flag.get()) {
    console_error(scope, error);
    return;
  }
  REPORTING.with(|flag| flag.set(true));
  let _guard = ReportingGuard;
  let message = error_message(scope, error);
  let event = ErrorEvent::trusted(scope, message, error);
  let event = unsafe { yuzuko_core::op::wrap_instance(scope, event) };
  event::define_is_trusted(scope, event);
  let global = scope.get_current_context().global(scope);
  let log = target::dispatch(scope, global, event).unwrap_or(true);
  if log {
    console_error(scope, error);
  }
}

fn report_error_callback(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut _ret: v8::ReturnValue,
) {
  let error = if args.length() == 0 {
    v8::undefined(scope).into()
  } else {
    args.get(0)
  };
  report_error(scope, error);
}

fn rejection_callback(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut ret: v8::ReturnValue,
) {
  let type_name = util::string_value(scope, args.get(0));
  let promise = args.get(1);
  let reason = args.get(2);
  let event =
    PromiseRejectionEvent::rejection(scope, &type_name, promise, reason);
  let event = unsafe { yuzuko_core::op::wrap_instance(scope, event) };
  event::define_is_trusted(scope, event);
  let global = scope.get_current_context().global(scope);
  let not_prevented = target::dispatch(scope, global, event).unwrap_or(true);
  ret.set(v8::Boolean::new(scope, !not_prevented).into());
}

fn capture_console(scope: &mut v8::PinScope) {
  let global = scope.get_current_context().global(scope);
  let Some(console) = global
    .get(scope, key(scope, "console").into())
    .and_then(|value| v8::Local::<v8::Object>::try_from(value).ok())
  else {
    return;
  };
  let Some(error) = console
    .get(scope, key(scope, "error").into())
    .and_then(|value| v8::Local::<v8::Function>::try_from(value).ok())
  else {
    return;
  };
  CONSOLE.with(|slot| {
    *slot.borrow_mut() = Some((
      v8::Global::new(scope, console),
      v8::Global::new(scope, error),
    ));
  });
}

fn console_error(scope: &mut v8::PinScope, error: v8::Local<v8::Value>) {
  let Some((console, function)) = CONSOLE.with(|slot| slot.borrow().clone())
  else {
    return;
  };
  let console = v8::Local::new(scope, console);
  let function = v8::Local::new(scope, function);
  let _ = function.call(scope, console.into(), &[error]);
}

fn error_message(
  scope: &mut v8::PinScope,
  error: v8::Local<v8::Value>,
) -> String {
  if let Ok(object) = v8::Local::<v8::Object>::try_from(error)
    && is_error(scope, error)
    && let Some(message) = object.get(scope, key(scope, "message").into())
  {
    return util::string_value(scope, message);
  }
  util::string_value(scope, error)
}

fn is_error(scope: &mut v8::PinScope, value: v8::Local<v8::Value>) -> bool {
  let global = scope.get_current_context().global(scope);
  let Some(constructor) = global
    .get(scope, key(scope, "Error").into())
    .and_then(|value| v8::Local::<v8::Object>::try_from(value).ok())
  else {
    return false;
  };
  value.instance_of(scope, constructor).unwrap_or(false)
}

fn install_onerror(scope: &mut v8::PinScope, global: v8::Local<v8::Object>) {
  let _ = global.set_accessor_with_setter(
    scope,
    key(scope, "onerror").into(),
    get_onerror,
    set_onerror,
  );
}

fn install_handler(
  scope: &mut v8::PinScope,
  global: v8::Local<v8::Object>,
  name: &str,
  getter: impl v8::MapFnTo<v8::AccessorNameGetterCallback>,
  setter: impl v8::MapFnTo<v8::AccessorNameSetterCallback>,
) {
  let _ = global.set_accessor_with_setter(
    scope,
    key(scope, name).into(),
    getter,
    setter,
  );
}

fn get_onerror(
  scope: &mut v8::PinScope,
  _key: v8::Local<v8::Name>,
  _args: v8::PropertyCallbackArguments,
  mut ret: v8::ReturnValue<v8::Value>,
) {
  return_handler(scope, &ONERROR, &mut ret);
}

fn set_onerror(
  scope: &mut v8::PinScope,
  _key: v8::Local<v8::Name>,
  value: v8::Local<v8::Value>,
  _args: v8::PropertyCallbackArguments,
  mut _ret: v8::ReturnValue<()>,
) {
  let global = scope.get_current_context().global(scope);
  clear_wrapper(scope, global, "error", &ONERROR);
  let Ok(handler) = v8::Local::<v8::Function>::try_from(value) else {
    return;
  };
  let Some(wrapper) = v8::Function::new(scope, onerror_adapter) else {
    return;
  };
  if target::add_listener(scope, global, "error", wrapper.into(), None).is_err()
  {
    return;
  }
  ONERROR.with(|slot| {
    let mut slot = slot.borrow_mut();
    slot.callback = Some(v8::Global::new(scope, handler));
    slot.wrapper = Some(v8::Global::new(scope, wrapper));
  });
}

fn onerror_adapter(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut _ret: v8::ReturnValue,
) {
  let Ok(event) = v8::Local::<v8::Object>::try_from(args.get(0)) else {
    return;
  };
  let Some((message, filename, lineno, colno, error)) =
    event::error_fields(scope, event)
  else {
    return;
  };
  let Some(handler) = ONERROR.with(|slot| slot.borrow().callback.clone())
  else {
    return;
  };
  let handler = v8::Local::new(scope, handler);
  let global = scope.get_current_context().global(scope);
  let error = v8::Local::new(scope, error);
  let message = v8::String::new(scope, &message).unwrap();
  let filename = v8::String::new(scope, &filename).unwrap();
  let lineno = v8::Number::new(scope, lineno);
  let colno = v8::Number::new(scope, colno);
  let result = handler.call(
    scope,
    global.into(),
    &[
      message.into(),
      filename.into(),
      lineno.into(),
      colno.into(),
      error,
    ],
  );
  if result.is_some_and(|value| value.is_true()) {
    event::with_event(scope, event, Event::prevent);
  }
}

fn get_unhandled(
  scope: &mut v8::PinScope,
  _key: v8::Local<v8::Name>,
  _args: v8::PropertyCallbackArguments,
  mut ret: v8::ReturnValue<v8::Value>,
) {
  return_handler(scope, &ONUNHANDLED, &mut ret);
}

fn set_unhandled(
  scope: &mut v8::PinScope,
  _key: v8::Local<v8::Name>,
  value: v8::Local<v8::Value>,
  _args: v8::PropertyCallbackArguments,
  mut _ret: v8::ReturnValue<()>,
) {
  set_direct(scope, "unhandledrejection", &ONUNHANDLED, value);
}

fn get_handled(
  scope: &mut v8::PinScope,
  _key: v8::Local<v8::Name>,
  _args: v8::PropertyCallbackArguments,
  mut ret: v8::ReturnValue<v8::Value>,
) {
  return_handler(scope, &ONHANDLED, &mut ret);
}

fn set_handled(
  scope: &mut v8::PinScope,
  _key: v8::Local<v8::Name>,
  value: v8::Local<v8::Value>,
  _args: v8::PropertyCallbackArguments,
  mut _ret: v8::ReturnValue<()>,
) {
  set_direct(scope, "rejectionhandled", &ONHANDLED, value);
}

fn set_direct(
  scope: &mut v8::PinScope,
  event_type: &str,
  slot: &'static std::thread::LocalKey<RefCell<Handler>>,
  value: v8::Local<v8::Value>,
) {
  let global = scope.get_current_context().global(scope);
  clear_wrapper(scope, global, event_type, slot);
  let Ok(handler) = v8::Local::<v8::Function>::try_from(value) else {
    return;
  };
  if target::add_listener(scope, global, event_type, handler.into(), None)
    .is_err()
  {
    return;
  }
  slot.with(|slot| {
    slot.borrow_mut().callback = Some(v8::Global::new(scope, handler));
  });
}

fn clear_wrapper(
  scope: &mut v8::PinScope,
  global: v8::Local<v8::Object>,
  event_type: &str,
  slot: &'static std::thread::LocalKey<RefCell<Handler>>,
) {
  let (callback, wrapper) = slot.with(|slot| {
    let mut slot = slot.borrow_mut();
    (slot.callback.take(), slot.wrapper.take())
  });
  let Some(listener) = wrapper.or(callback) else {
    return;
  };
  let listener = v8::Local::new(scope, listener);
  let _ =
    target::remove_listener(scope, global, event_type, listener.into(), false);
}

fn return_handler(
  scope: &mut v8::PinScope,
  slot: &'static std::thread::LocalKey<RefCell<Handler>>,
  ret: &mut v8::ReturnValue<v8::Value>,
) {
  match slot.with(|slot| slot.borrow().callback.clone()) {
    Some(callback) => ret.set(v8::Local::new(scope, callback).into()),
    None => ret.set(v8::null(scope).into()),
  }
}
