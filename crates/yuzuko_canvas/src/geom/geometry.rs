//! `DOMPoint`, `DOMRect`, and `DOMQuad`.

use std::cell::Cell;

use yuzuko_core::op::Js;
use yuzuko_core::op::New;
use yuzuko_core::op::OpError;
use yuzuko_macros::op;

use crate::geom::matrix::read_dom_matrix_parts;

fn js_min(left: f64, right: f64) -> f64 {
  if left.is_nan() || right.is_nan() {
    f64::NAN
  } else {
    left.min(right)
  }
}

fn js_max(left: f64, right: f64) -> f64 {
  if left.is_nan() || right.is_nan() {
    f64::NAN
  } else {
    left.max(right)
  }
}

#[repr(C)]
pub struct DomPointReadOnly {
  x: Cell<f64>,
  y: Cell<f64>,
  z: Cell<f64>,
  w: Cell<f64>,
}

unsafe impl v8::cppgc::GarbageCollected for DomPointReadOnly {
  fn trace(&self, _visitor: &mut v8::cppgc::Visitor) {}

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"DOMPointReadOnly"
  }
}

impl DomPointReadOnly {
  fn at(x: f64, y: f64, z: f64, w: f64) -> Self {
    Self {
      x: Cell::new(x),
      y: Cell::new(y),
      z: Cell::new(z),
      w: Cell::new(w),
    }
  }

  fn from_init(scope: &mut v8::PinScope, value: v8::Local<v8::Value>) -> Self {
    let Some(object) = v8::Local::<v8::Object>::try_from(value).ok() else {
      return Self::at(f64::NAN, 0.0, 0.0, 1.0);
    };
    if let Some(point) = point_of(scope, value) {
      return Self::at(point.0, point.1, point.2, point.3);
    }
    Self::at(
      member(scope, object, "x", 0.0),
      member(scope, object, "y", 0.0),
      member(scope, object, "z", 0.0),
      member(scope, object, "w", 1.0),
    )
  }
}

#[op(name = "DOMPointReadOnly")]
impl DomPointReadOnly {
  #[constructor]
  fn new(
    x: Option<f64>,
    y: Option<f64>,
    z: Option<f64>,
    w: Option<f64>,
  ) -> Self {
    Self::at(
      x.unwrap_or(0.0),
      y.unwrap_or(0.0),
      z.unwrap_or(0.0),
      w.unwrap_or(1.0),
    )
  }

  #[static_method]
  #[length(0)]
  fn from_point(
    scope: &mut v8::PinScope,
    init: v8::Local<v8::Value>,
  ) -> New<Self> {
    New(Self::from_init(scope, init))
  }

  #[getter]
  fn x(&self) -> f64 {
    self.x.get()
  }
  #[getter]
  fn y(&self) -> f64 {
    self.y.get()
  }
  #[getter]
  fn z(&self) -> f64 {
    self.z.get()
  }
  #[getter]
  fn w(&self) -> f64 {
    self.w.get()
  }

  #[rename("toJSON")]
  fn to_json(&self, scope: &mut v8::PinScope) -> Js {
    let object = v8::Object::new(scope);
    set_number(scope, object, "x", self.x.get());
    set_number(scope, object, "y", self.y.get());
    set_number(scope, object, "z", self.z.get());
    set_number(scope, object, "w", self.w.get());
    let value: v8::Local<v8::Value> = object.into();
    Js(v8::Global::new(scope, value))
  }

  #[length(0)]
  fn matrix_transform(
    &self,
    scope: &mut v8::PinScope,
    matrix: v8::Local<v8::Value>,
  ) -> Result<New<DomPoint>, OpError> {
    let (mat, _) = read_dom_matrix_parts(scope, matrix)
      .ok_or_else(|| OpError::type_("The matrix is not a DOMMatrix"))?;
    let x = self.x.get();
    let y = self.y.get();
    let z = self.z.get();
    let w = self.w.get();
    let next = [
      mat[(0, 0)] * x + mat[(0, 1)] * y + mat[(0, 2)] * z + mat[(0, 3)] * w,
      mat[(1, 0)] * x + mat[(1, 1)] * y + mat[(1, 2)] * z + mat[(1, 3)] * w,
      mat[(2, 0)] * x + mat[(2, 1)] * y + mat[(2, 2)] * z + mat[(2, 3)] * w,
      mat[(3, 0)] * x + mat[(3, 1)] * y + mat[(3, 2)] * z + mat[(3, 3)] * w,
    ];
    Ok(New(DomPoint {
      base: DomPointReadOnly::at(next[0], next[1], next[2], next[3]),
    }))
  }
}

#[repr(C)]
pub struct DomPoint {
  base: DomPointReadOnly,
}

unsafe impl v8::cppgc::GarbageCollected for DomPoint {
  fn trace(&self, visitor: &mut v8::cppgc::Visitor) {
    v8::cppgc::GarbageCollected::trace(&self.base, visitor);
  }

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"DOMPoint"
  }
}

#[op(name = "DOMPoint", inherit = DomPointReadOnly)]
impl DomPoint {
  #[constructor]
  fn new(
    x: Option<f64>,
    y: Option<f64>,
    z: Option<f64>,
    w: Option<f64>,
  ) -> Self {
    Self {
      base: DomPointReadOnly::at(
        x.unwrap_or(0.0),
        y.unwrap_or(0.0),
        z.unwrap_or(0.0),
        w.unwrap_or(1.0),
      ),
    }
  }

  #[getter]
  fn x(&self) -> f64 {
    self.base.x.get()
  }
  #[getter]
  fn y(&self) -> f64 {
    self.base.y.get()
  }
  #[getter]
  fn z(&self) -> f64 {
    self.base.z.get()
  }
  #[getter]
  fn w(&self) -> f64 {
    self.base.w.get()
  }

  #[static_method]
  #[length(0)]
  fn from_point(
    scope: &mut v8::PinScope,
    init: v8::Local<v8::Value>,
  ) -> New<Self> {
    New(Self {
      base: DomPointReadOnly::from_init(scope, init),
    })
  }

  #[setter]
  fn x(&self, value: f64) {
    self.base.x.set(value);
  }
  #[setter]
  fn y(&self, value: f64) {
    self.base.y.set(value);
  }
  #[setter]
  fn z(&self, value: f64) {
    self.base.z.set(value);
  }
  #[setter]
  fn w(&self, value: f64) {
    self.base.w.set(value);
  }
}

#[repr(C)]
pub struct DomRectReadOnly {
  x: Cell<f64>,
  y: Cell<f64>,
  width: Cell<f64>,
  height: Cell<f64>,
}

unsafe impl v8::cppgc::GarbageCollected for DomRectReadOnly {
  fn trace(&self, _visitor: &mut v8::cppgc::Visitor) {}

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"DOMRectReadOnly"
  }
}

impl DomRectReadOnly {
  pub(crate) fn at(x: f64, y: f64, width: f64, height: f64) -> Self {
    Self {
      x: Cell::new(x),
      y: Cell::new(y),
      width: Cell::new(width),
      height: Cell::new(height),
    }
  }

  fn top_edge(&self) -> f64 {
    js_min(self.y.get(), self.y.get() + self.height.get())
  }

  fn right_edge(&self) -> f64 {
    js_max(self.x.get(), self.x.get() + self.width.get())
  }

  fn bottom_edge(&self) -> f64 {
    js_max(self.y.get(), self.y.get() + self.height.get())
  }

  fn left_edge(&self) -> f64 {
    js_min(self.x.get(), self.x.get() + self.width.get())
  }
}

#[op(name = "DOMRectReadOnly")]
impl DomRectReadOnly {
  #[constructor]
  fn new(
    x: Option<f64>,
    y: Option<f64>,
    width: Option<f64>,
    height: Option<f64>,
  ) -> Self {
    Self::at(
      x.unwrap_or(0.0),
      y.unwrap_or(0.0),
      width.unwrap_or(0.0),
      height.unwrap_or(0.0),
    )
  }

  #[static_method]
  #[length(0)]
  fn from_rect(
    scope: &mut v8::PinScope,
    init: v8::Local<v8::Value>,
  ) -> New<Self> {
    New(rect_from_init(scope, init))
  }

  #[getter]
  fn x(&self) -> f64 {
    self.x.get()
  }
  #[getter]
  fn y(&self) -> f64 {
    self.y.get()
  }
  #[getter]
  fn width(&self) -> f64 {
    self.width.get()
  }
  #[getter]
  fn height(&self) -> f64 {
    self.height.get()
  }
  #[getter]
  fn top(&self) -> f64 {
    self.top_edge()
  }
  #[getter]
  fn right(&self) -> f64 {
    self.right_edge()
  }
  #[getter]
  fn bottom(&self) -> f64 {
    self.bottom_edge()
  }
  #[getter]
  fn left(&self) -> f64 {
    self.left_edge()
  }

  #[rename("toJSON")]
  fn to_json(&self, scope: &mut v8::PinScope) -> Js {
    let object = v8::Object::new(scope);
    set_number(scope, object, "x", self.x.get());
    set_number(scope, object, "y", self.y.get());
    set_number(scope, object, "width", self.width.get());
    set_number(scope, object, "height", self.height.get());
    set_number(scope, object, "top", self.top_edge());
    set_number(scope, object, "right", self.right_edge());
    set_number(scope, object, "bottom", self.bottom_edge());
    set_number(scope, object, "left", self.left_edge());
    let value: v8::Local<v8::Value> = object.into();
    Js(v8::Global::new(scope, value))
  }
}

#[repr(C)]
pub struct DomRect {
  base: DomRectReadOnly,
}

unsafe impl v8::cppgc::GarbageCollected for DomRect {
  fn trace(&self, visitor: &mut v8::cppgc::Visitor) {
    v8::cppgc::GarbageCollected::trace(&self.base, visitor);
  }

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"DOMRect"
  }
}

#[op(name = "DOMRect", inherit = DomRectReadOnly)]
impl DomRect {
  #[constructor]
  fn new(
    x: Option<f64>,
    y: Option<f64>,
    width: Option<f64>,
    height: Option<f64>,
  ) -> Self {
    Self {
      base: DomRectReadOnly::at(
        x.unwrap_or(0.0),
        y.unwrap_or(0.0),
        width.unwrap_or(0.0),
        height.unwrap_or(0.0),
      ),
    }
  }

  #[getter]
  fn x(&self) -> f64 {
    self.base.x.get()
  }
  #[getter]
  fn y(&self) -> f64 {
    self.base.y.get()
  }
  #[getter]
  fn width(&self) -> f64 {
    self.base.width.get()
  }
  #[getter]
  fn height(&self) -> f64 {
    self.base.height.get()
  }

  #[static_method]
  #[length(0)]
  fn from_rect(
    scope: &mut v8::PinScope,
    init: v8::Local<v8::Value>,
  ) -> New<Self> {
    New(Self {
      base: rect_from_init(scope, init),
    })
  }

  #[setter]
  fn x(&self, value: f64) {
    self.base.x.set(value);
  }
  #[setter]
  fn y(&self, value: f64) {
    self.base.y.set(value);
  }
  #[setter]
  fn width(&self, value: f64) {
    self.base.width.set(value);
  }
  #[setter]
  fn height(&self, value: f64) {
    self.base.height.set(value);
  }
}

pub struct DomQuad {
  p1: v8::TracedReference<v8::Object>,
  p2: v8::TracedReference<v8::Object>,
  p3: v8::TracedReference<v8::Object>,
  p4: v8::TracedReference<v8::Object>,
}

unsafe impl v8::cppgc::GarbageCollected for DomQuad {
  fn trace(&self, visitor: &mut v8::cppgc::Visitor) {
    visitor.trace(&self.p1);
    visitor.trace(&self.p2);
    visitor.trace(&self.p3);
    visitor.trace(&self.p4);
  }

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"DOMQuad"
  }
}

fn traced_point(
  scope: &mut v8::PinScope,
  point: (f64, f64, f64, f64),
) -> v8::TracedReference<v8::Object> {
  let object = unsafe {
    yuzuko_core::op::wrap_instance(
      scope,
      DomPoint::at(point.0, point.1, point.2, point.3),
    )
  };
  yuzuko_core::op::traced(scope, object)
}

fn quad_from_points(
  scope: &mut v8::PinScope,
  points: [(f64, f64, f64, f64); 4],
) -> DomQuad {
  DomQuad {
    p1: traced_point(scope, points[0]),
    p2: traced_point(scope, points[1]),
    p3: traced_point(scope, points[2]),
    p4: traced_point(scope, points[3]),
  }
}

fn stored_point(
  scope: &mut v8::PinScope,
  reference: &v8::TracedReference<v8::Object>,
) -> (f64, f64, f64, f64) {
  let Some(object) = yuzuko_core::op::traced_local(scope, reference) else {
    return (0.0, 0.0, 0.0, 1.0);
  };
  let Some(host) =
    (unsafe { yuzuko_core::op::host_ref::<DomPoint>(scope, object) })
  else {
    return (0.0, 0.0, 0.0, 1.0);
  };
  host.get().coords()
}

fn point_js(
  scope: &mut v8::PinScope,
  reference: &v8::TracedReference<v8::Object>,
) -> Js {
  let Some(object) = yuzuko_core::op::traced_local(scope, reference) else {
    let value: v8::Local<v8::Value> = v8::undefined(scope).into();
    return Js(v8::Global::new(scope, value));
  };
  let value: v8::Local<v8::Value> = object.into();
  Js(v8::Global::new(scope, value))
}

#[op(name = "DOMQuad")]
impl DomQuad {
  #[constructor]
  fn new(
    scope: &mut v8::PinScope,
    p1: Option<v8::Local<v8::Value>>,
    p2: Option<v8::Local<v8::Value>>,
    p3: Option<v8::Local<v8::Value>>,
    p4: Option<v8::Local<v8::Value>>,
  ) -> Result<Self, OpError> {
    let points = [
      require_point(scope, p1)?,
      require_point(scope, p2)?,
      require_point(scope, p3)?,
      require_point(scope, p4)?,
    ];
    Ok(quad_from_points(scope, points))
  }

  #[static_method]
  #[length(0)]
  fn from_rect(
    scope: &mut v8::PinScope,
    init: v8::Local<v8::Value>,
  ) -> New<Self> {
    let rect = rect_from_init(scope, init);
    let x = rect.x.get();
    let y = rect.y.get();
    let right = x + rect.width.get();
    let bottom = y + rect.height.get();
    New(quad_from_points(
      scope,
      [
        (x, y, 0.0, 1.0),
        (right, y, 0.0, 1.0),
        (right, bottom, 0.0, 1.0),
        (x, bottom, 0.0, 1.0),
      ],
    ))
  }

  #[static_method]
  #[length(0)]
  fn from_quad(
    scope: &mut v8::PinScope,
    init: v8::Local<v8::Value>,
  ) -> New<Self> {
    let object = v8::Local::<v8::Object>::try_from(init).ok();
    let mut read = |name: &str| {
      object
        .as_ref()
        .and_then(|object| {
          let key = v8::String::new(scope, name).unwrap();
          object.get(scope, key.into())
        })
        .map(|value| point_init(scope, value))
        .unwrap_or((0.0, 0.0, 0.0, 1.0))
    };
    let points = [read("p1"), read("p2"), read("p3"), read("p4")];
    New(quad_from_points(scope, points))
  }

  #[getter]
  fn p1(&self, scope: &mut v8::PinScope) -> Js {
    point_js(scope, &self.p1)
  }
  #[getter]
  fn p2(&self, scope: &mut v8::PinScope) -> Js {
    point_js(scope, &self.p2)
  }
  #[getter]
  fn p3(&self, scope: &mut v8::PinScope) -> Js {
    point_js(scope, &self.p3)
  }
  #[getter]
  fn p4(&self, scope: &mut v8::PinScope) -> Js {
    point_js(scope, &self.p4)
  }

  fn get_bounds(&self, scope: &mut v8::PinScope) -> New<DomRectReadOnly> {
    let points = [
      stored_point(scope, &self.p1),
      stored_point(scope, &self.p2),
      stored_point(scope, &self.p3),
      stored_point(scope, &self.p4),
    ];
    let left = points
      .iter()
      .map(|point| point.0)
      .fold(f64::INFINITY, js_min);
    let top = points
      .iter()
      .map(|point| point.1)
      .fold(f64::INFINITY, js_min);
    let right = points
      .iter()
      .map(|point| point.0)
      .fold(f64::NEG_INFINITY, js_max);
    let bottom = points
      .iter()
      .map(|point| point.1)
      .fold(f64::NEG_INFINITY, js_max);
    New(DomRectReadOnly::at(left, top, right - left, bottom - top))
  }

  #[rename("toJSON")]
  fn to_json(&self, scope: &mut v8::PinScope) -> Js {
    let object = v8::Object::new(scope);
    for (name, point) in [
      ("p1", point_js(scope, &self.p1)),
      ("p2", point_js(scope, &self.p2)),
      ("p3", point_js(scope, &self.p3)),
      ("p4", point_js(scope, &self.p4)),
    ] {
      let key = v8::String::new(scope, name).unwrap();
      let value = v8::Local::new(scope, &point.0);
      object.set(scope, key.into(), value);
    }
    let value: v8::Local<v8::Value> = object.into();
    Js(v8::Global::new(scope, value))
  }
}

pub struct DomRectList;

unsafe impl v8::cppgc::GarbageCollected for DomRectList {
  fn trace(&self, _visitor: &mut v8::cppgc::Visitor) {}

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"DOMRectList"
  }
}

#[op(name = "DOMRectList")]
impl DomRectList {
  #[getter]
  fn length(&self) -> u32 {
    0
  }

  fn item(&self, _index: f64) -> Option<New<DomRect>> {
    None
  }
}

const TAG_POINT_READ_ONLY: u32 = 4;
const TAG_POINT: u32 = 5;
const TAG_RECT_READ_ONLY: u32 = 6;
const TAG_RECT: u32 = 7;
const TAG_QUAD: u32 = 8;

pub(crate) fn register_clone() {
  yuzuko_core::add_host_clone(write_geometry_clone, read_geometry_clone);
}

fn write_geometry_clone(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  helper: &dyn v8::ValueSerializerHelper,
) -> yuzuko_core::WriteResult {
  if let Some(host) =
    unsafe { yuzuko_core::op::host_ref::<DomPoint>(scope, object) }
  {
    let point = host.get();
    helper.write_uint32(TAG_POINT);
    write_point(
      helper,
      point.base.x.get(),
      point.base.y.get(),
      point.base.z.get(),
      point.base.w.get(),
    );
    return yuzuko_core::WriteResult::Wrote;
  }
  if let Some(host) =
    unsafe { yuzuko_core::op::host_ref::<DomPointReadOnly>(scope, object) }
  {
    let point = host.get();
    helper.write_uint32(TAG_POINT_READ_ONLY);
    write_point(
      helper,
      point.x.get(),
      point.y.get(),
      point.z.get(),
      point.w.get(),
    );
    return yuzuko_core::WriteResult::Wrote;
  }
  if let Some(host) =
    unsafe { yuzuko_core::op::host_ref::<DomRect>(scope, object) }
  {
    helper.write_uint32(TAG_RECT);
    write_rect(helper, &host.get().base);
    return yuzuko_core::WriteResult::Wrote;
  }
  if let Some(host) =
    unsafe { yuzuko_core::op::host_ref::<DomRectReadOnly>(scope, object) }
  {
    helper.write_uint32(TAG_RECT_READ_ONLY);
    write_rect(helper, host.get());
    return yuzuko_core::WriteResult::Wrote;
  }
  if let Some(host) =
    unsafe { yuzuko_core::op::host_ref::<DomQuad>(scope, object) }
  {
    helper.write_uint32(TAG_QUAD);
    let quad = host.get();
    for point in [
      stored_point(scope, &quad.p1),
      stored_point(scope, &quad.p2),
      stored_point(scope, &quad.p3),
      stored_point(scope, &quad.p4),
    ] {
      write_point(helper, point.0, point.1, point.2, point.3);
    }
    return yuzuko_core::WriteResult::Wrote;
  }
  yuzuko_core::WriteResult::Skip
}

fn read_geometry_clone(
  scope: &mut v8::PinScope,
  tag: u32,
  helper: &dyn v8::ValueDeserializerHelper,
) -> yuzuko_core::ReadResult {
  let object = match tag {
    TAG_POINT_READ_ONLY => {
      let Some(point) = read_point(helper) else {
        yuzuko_core::reject_clone(scope);
        return yuzuko_core::ReadResult::Failed;
      };
      unsafe {
        yuzuko_core::op::wrap_instance(
          scope,
          DomPointReadOnly::at(point.0, point.1, point.2, point.3),
        )
      }
    }
    TAG_POINT => {
      let Some(point) = read_point(helper) else {
        yuzuko_core::reject_clone(scope);
        return yuzuko_core::ReadResult::Failed;
      };
      unsafe { yuzuko_core::op::wrap_instance(scope, point_value(point)) }
    }
    TAG_RECT_READ_ONLY => {
      let Some(rect) = read_rect(helper) else {
        yuzuko_core::reject_clone(scope);
        return yuzuko_core::ReadResult::Failed;
      };
      unsafe { yuzuko_core::op::wrap_instance(scope, rect) }
    }
    TAG_RECT => {
      let Some(rect) = read_rect(helper) else {
        yuzuko_core::reject_clone(scope);
        return yuzuko_core::ReadResult::Failed;
      };
      unsafe { yuzuko_core::op::wrap_instance(scope, DomRect { base: rect }) }
    }
    TAG_QUAD => {
      let Some(p1) = read_point(helper) else {
        yuzuko_core::reject_clone(scope);
        return yuzuko_core::ReadResult::Failed;
      };
      let Some(p2) = read_point(helper) else {
        yuzuko_core::reject_clone(scope);
        return yuzuko_core::ReadResult::Failed;
      };
      let Some(p3) = read_point(helper) else {
        yuzuko_core::reject_clone(scope);
        return yuzuko_core::ReadResult::Failed;
      };
      let Some(p4) = read_point(helper) else {
        yuzuko_core::reject_clone(scope);
        return yuzuko_core::ReadResult::Failed;
      };
      let quad = quad_from_points(scope, [p1, p2, p3, p4]);
      unsafe { yuzuko_core::op::wrap_instance(scope, quad) }
    }
    _ => return yuzuko_core::ReadResult::Skip,
  };
  yuzuko_core::ReadResult::Done(v8::Global::new(scope, object))
}

fn write_point(
  helper: &dyn v8::ValueSerializerHelper,
  x: f64,
  y: f64,
  z: f64,
  w: f64,
) {
  helper.write_double(x);
  helper.write_double(y);
  helper.write_double(z);
  helper.write_double(w);
}

fn write_rect(helper: &dyn v8::ValueSerializerHelper, rect: &DomRectReadOnly) {
  helper.write_double(rect.x.get());
  helper.write_double(rect.y.get());
  helper.write_double(rect.width.get());
  helper.write_double(rect.height.get());
}

fn read_point(
  helper: &dyn v8::ValueDeserializerHelper,
) -> Option<(f64, f64, f64, f64)> {
  let mut values = [0.0; 4];
  for value in &mut values {
    if !helper.read_double(value) {
      return None;
    }
  }
  Some((values[0], values[1], values[2], values[3]))
}

fn read_rect(
  helper: &dyn v8::ValueDeserializerHelper,
) -> Option<DomRectReadOnly> {
  let mut values = [0.0; 4];
  for value in &mut values {
    if !helper.read_double(value) {
      return None;
    }
  }
  Some(DomRectReadOnly::at(
    values[0], values[1], values[2], values[3],
  ))
}

fn point_value(point: (f64, f64, f64, f64)) -> DomPoint {
  DomPoint::at(point.0, point.1, point.2, point.3)
}

impl DomPoint {
  pub(crate) fn at(x: f64, y: f64, z: f64, w: f64) -> Self {
    Self {
      base: DomPointReadOnly::at(x, y, z, w),
    }
  }

  pub(crate) fn coords(&self) -> (f64, f64, f64, f64) {
    (
      self.base.x.get(),
      self.base.y.get(),
      self.base.z.get(),
      self.base.w.get(),
    )
  }
}

fn require_point(
  scope: &mut v8::PinScope,
  value: Option<v8::Local<v8::Value>>,
) -> Result<(f64, f64, f64, f64), OpError> {
  let Some(value) = value.filter(|value| !value.is_undefined()) else {
    return Ok((0.0, 0.0, 0.0, 1.0));
  };
  if value.is_null() {
    return Ok((0.0, 0.0, 0.0, 1.0));
  }
  if !value.is_object() {
    return Err(OpError::type_("The point is not a DOMPointInit"));
  }
  Ok(point_init(scope, value))
}

pub(crate) fn point_init(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> (f64, f64, f64, f64) {
  if let Some(point) = point_of(scope, value) {
    return point;
  }
  let Some(object) = v8::Local::<v8::Object>::try_from(value).ok() else {
    return (f64::NAN, 0.0, 0.0, 1.0);
  };
  (
    member(scope, object, "x", 0.0),
    member(scope, object, "y", 0.0),
    member(scope, object, "z", 0.0),
    member(scope, object, "w", 1.0),
  )
}

fn point_of(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Option<(f64, f64, f64, f64)> {
  let object = v8::Local::<v8::Object>::try_from(value).ok()?;
  if let Some(host) =
    unsafe { yuzuko_core::op::host_ref::<DomPoint>(scope, object) }
  {
    let point = host.get();
    return Some((
      point.base.x.get(),
      point.base.y.get(),
      point.base.z.get(),
      point.base.w.get(),
    ));
  }
  let host =
    unsafe { yuzuko_core::op::host_ref::<DomPointReadOnly>(scope, object) }?;
  let point = host.get();
  Some((point.x.get(), point.y.get(), point.z.get(), point.w.get()))
}

fn rect_from_init(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> DomRectReadOnly {
  if let Ok(object) = v8::Local::<v8::Object>::try_from(value) {
    if let Some(host) =
      unsafe { yuzuko_core::op::host_ref::<DomRect>(scope, object) }
    {
      let rect = &host.get().base;
      return DomRectReadOnly::at(
        rect.x.get(),
        rect.y.get(),
        rect.width.get(),
        rect.height.get(),
      );
    }
    if let Some(host) =
      unsafe { yuzuko_core::op::host_ref::<DomRectReadOnly>(scope, object) }
    {
      let rect = host.get();
      return DomRectReadOnly::at(
        rect.x.get(),
        rect.y.get(),
        rect.width.get(),
        rect.height.get(),
      );
    }
    return DomRectReadOnly::at(
      member(scope, object, "x", 0.0),
      member(scope, object, "y", 0.0),
      member(scope, object, "width", 0.0),
      member(scope, object, "height", 0.0),
    );
  }
  DomRectReadOnly::at(0.0, 0.0, 0.0, 0.0)
}

fn member(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
  default: f64,
) -> f64 {
  let key = v8::String::new(scope, name).unwrap();
  let Some(value) = object.get(scope, key.into()) else {
    return default;
  };
  if value.is_undefined() {
    return default;
  }
  value.number_value(scope).unwrap_or(f64::NAN)
}

fn set_number(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
  value: f64,
) {
  let key = v8::String::new(scope, name).unwrap();
  let number = v8::Number::new(scope, value);
  object.set(scope, key.into(), number.into());
}
