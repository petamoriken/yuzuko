//! `on*` attributes for the window event targets.

/// Pointer, touch, and mouse events shared by `Window` and `WindowSurface`.
const POINTER_AND_MOUSE: &[&str] = &[
  "pointerdown",
  "pointerup",
  "pointermove",
  "pointerrawupdate",
  "pointercancel",
  "pointerover",
  "pointerout",
  "pointerenter",
  "pointerleave",
  "gotpointercapture",
  "lostpointercapture",
  "touchstart",
  "touchmove",
  "touchend",
  "touchcancel",
  "mousedown",
  "mouseup",
  "mousemove",
  "mouseover",
  "mouseout",
  "mouseenter",
  "mouseleave",
  "click",
  "dblclick",
  "auxclick",
  "contextmenu",
];

const WINDOW_ONLY: &[&str] = &[
  "close",
  "resize",
  "focus",
  "blur",
  "menuclick",
  "filehover",
  "filehovercancel",
  "filedrop",
  "keydown",
  "keyup",
  "wheel",
  "compositionstart",
  "compositionupdate",
  "compositionend",
  "pointerlockchange",
  "pointerlockerror",
];

const SURFACE_ONLY: &[&str] =
  &["visibilitychange", "beforeinput", "copy", "cut", "paste"];

pub(crate) fn define(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  events: &[&str],
) {
  yuzuko_dom::define_event_handlers(scope, object, events);
}

pub(crate) fn install_window(scope: &mut v8::PinScope) {
  let mut events =
    Vec::with_capacity(WINDOW_ONLY.len() + POINTER_AND_MOUSE.len());
  events.extend_from_slice(WINDOW_ONLY);
  events.extend_from_slice(POINTER_AND_MOUSE);
  define_class(scope, "Window", &events);
}

pub(crate) fn install_surface(scope: &mut v8::PinScope) {
  let mut events =
    Vec::with_capacity(SURFACE_ONLY.len() + POINTER_AND_MOUSE.len());
  events.extend_from_slice(SURFACE_ONLY);
  events.extend_from_slice(POINTER_AND_MOUSE);
  define_class(scope, "WindowSurface", &events);
}

pub(crate) fn install_tray(scope: &mut v8::PinScope) {
  define_class(scope, "Tray", &["click", "menuclick"]);
}

pub(crate) fn define_class(
  scope: &mut v8::PinScope,
  class_name: &str,
  events: &[&str],
) {
  let yuzuko = yuzuko_core::namespace(scope, "Yuzuko");
  let key = v8::String::new(scope, class_name).unwrap();
  let ctor = yuzuko
    .get(scope, key.into())
    .and_then(|value| v8::Local::<v8::Object>::try_from(value).ok())
    .unwrap_or_else(|| panic!("{class_name} constructor"));
  let proto_key = v8::String::new(scope, "prototype").unwrap();
  let proto = ctor
    .get(scope, proto_key.into())
    .and_then(|value| v8::Local::<v8::Object>::try_from(value).ok())
    .unwrap_or_else(|| panic!("{class_name} prototype"));
  define(scope, proto, events);
}
