//! `DOMMatrix`, including CSS `<transform-list>` strings and 3D matrices.

use std::cell::Cell;

use kurbo::Affine;
use yuzuko_core::op::Js;
use yuzuko_core::op::New;
use yuzuko_core::op::OpError;
use yuzuko_macros::op;

use crate::geom::css_transform::Angle;
use crate::geom::css_transform::Length;
use crate::geom::css_transform::Transform;
use crate::geom::css_transform::parse_transform_list;
use crate::geom::mat4::Mat4;

#[derive(Clone, Copy, Debug)]
pub struct Matrix {
  pub a: f64,
  pub b: f64,
  pub c: f64,
  pub d: f64,
  pub e: f64,
  pub f: f64,
}

impl Matrix {
  pub const IDENTITY: Self = Self {
    a: 1.0,
    b: 0.0,
    c: 0.0,
    d: 1.0,
    e: 0.0,
    f: 0.0,
  };

  pub fn from_affine(affine: Affine) -> Self {
    let [a, b, c, d, e, f] = affine.as_coeffs();
    Self { a, b, c, d, e, f }
  }

  pub fn affine(self) -> Affine {
    Affine::new([self.a, self.b, self.c, self.d, self.e, self.f])
  }
}

impl Default for Matrix {
  fn default() -> Self {
    Self::IDENTITY
  }
}

#[repr(C)]
pub struct DomMatrixReadOnly {
  value: Cell<Mat4>,
  is_2d: Cell<bool>,
}

unsafe impl v8::cppgc::GarbageCollected for DomMatrixReadOnly {
  fn trace(&self, _visitor: &mut v8::cppgc::Visitor) {}

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"DOMMatrixReadOnly"
  }
}

impl DomMatrixReadOnly {
  fn from_parts(value: Mat4, is_2d: bool) -> Self {
    Self {
      value: Cell::new(value),
      is_2d: Cell::new(is_2d),
    }
  }

  fn updated(&self, update: impl FnOnce(&mut Mat4, &mut bool)) -> DomMatrix {
    let mut value = self.value.get();
    let mut is_2d = self.is_2d.get();
    update(&mut value, &mut is_2d);
    DomMatrix::from_parts(value, is_2d)
  }

  fn scaled_by(
    &self,
    scale_x: f64,
    scale_y: f64,
    scale_z: f64,
    origin_x: f64,
    origin_y: f64,
    origin_z: f64,
  ) -> DomMatrix {
    self.updated(|value, is_2d| {
      value.translate(origin_x, origin_y, origin_z, is_2d);
      value.scale(scale_x, scale_y, scale_z, is_2d);
      value.translate(-origin_x, -origin_y, -origin_z, is_2d);
    })
  }
}

#[repr(C)]
pub struct DomMatrix {
  base: DomMatrixReadOnly,
}

unsafe impl v8::cppgc::GarbageCollected for DomMatrix {
  fn trace(&self, visitor: &mut v8::cppgc::Visitor) {
    v8::cppgc::GarbageCollected::trace(&self.base, visitor);
  }

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"DOMMatrix"
  }
}

impl DomMatrix {
  pub fn from_matrix(value: Matrix) -> Self {
    Self::from_parts(Mat4::from_2d(value), true)
  }

  fn from_parts(value: Mat4, is_2d: bool) -> Self {
    Self {
      base: DomMatrixReadOnly::from_parts(value, is_2d),
    }
  }

  pub fn get(&self) -> Matrix {
    self.base.value.get().project_2d()
  }

  fn component(&self, index: usize) -> f64 {
    self.base.value.get().get(index)
  }

  fn set_component(&self, index: usize, value: f64) {
    let mut matrix = self.base.value.get();
    matrix.set(index, value);
    self.base.value.set(matrix);
  }

  /// A 3D element that is not its 2D identity value clears `is2D`.
  /// `-0` compares equal to `0`, so it leaves a 2D matrix 2D.
  fn set_3d(&self, index: usize, value: f64, identity: f64) {
    self.set_component(index, value);
    if value != identity {
      self.base.is_2d.set(false);
    }
  }

  fn adopt(&self, matrix: New<DomMatrix>) {
    let New(matrix) = matrix;
    self.base.value.set(matrix.base.value.get());
    self.base.is_2d.set(matrix.base.is_2d.get());
  }

  fn from_css(text: &str) -> Result<Self, OpError> {
    if text.is_empty() {
      return Ok(Self::from_matrix(Matrix::IDENTITY));
    }
    let mut value = Mat4::identity();
    let mut is_2d = true;
    for transform in parse_transform_list(text).map_err(|message| {
      OpError::dom(
        "SyntaxError",
        format!("Failed to parse as CSS <transform-list>: {message}"),
      )
    })? {
      apply_css(&mut value, &mut is_2d, &transform)?;
    }
    Ok(Self::from_parts(value, is_2d))
  }
}

pub fn read_dom_matrix(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Option<Matrix> {
  let object = v8::Local::<v8::Object>::try_from(value).ok()?;
  let host = unsafe { yuzuko_core::op::host_ref::<DomMatrix>(scope, object) }?;
  Some(host.get().get())
}

/// `DOMMatrix2DInit` validate-and-fixup. A live matrix contributes its 2D
/// projection. A dictionary only consults `a`–`f` and their `m*` aliases.
pub(crate) fn read_canvas_transform(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<Matrix, OpError> {
  if let Some(matrix) = read_dom_matrix(scope, value) {
    return Ok(matrix);
  }
  if let Some((matrix, _)) = read_dom_matrix_parts(scope, value) {
    return Ok(matrix.project_2d());
  }
  let object = v8::Local::<v8::Object>::try_from(value)
    .map_err(|_| OpError::type_("The transform is not a DOMMatrix2DInit"))?;
  let mut present = [false; 6];
  let mut numbers = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
  let names = [
    ("m11", 0),
    ("m12", 1),
    ("m21", 2),
    ("m22", 3),
    ("m41", 4),
    ("m42", 5),
  ];
  for (name, index) in names {
    if let Some(number) = optional_member(scope, object, name) {
      present[index] = true;
      numbers[index] = number;
    }
  }
  let aliases = [("a", 0), ("b", 1), ("c", 2), ("d", 3), ("e", 4), ("f", 5)];
  for (name, index) in aliases {
    let Some(number) = optional_member(scope, object, name) else {
      continue;
    };
    if present[index] && !same_number(numbers[index], number) {
      return Err(OpError::type_("Inconsistent 2d matrix value"));
    }
    if !present[index] {
      numbers[index] = number;
    }
  }
  Ok(Matrix {
    a: numbers[0],
    b: numbers[1],
    c: numbers[2],
    d: numbers[3],
    e: numbers[4],
    f: numbers[5],
  })
}

pub(crate) fn read_dom_matrix_parts(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Option<(Mat4, bool)> {
  let object = v8::Local::<v8::Object>::try_from(value).ok()?;
  if let Some(host) =
    unsafe { yuzuko_core::op::host_ref::<DomMatrix>(scope, object) }
  {
    let matrix = host.get();
    return Some((matrix.base.value.get(), matrix.base.is_2d.get()));
  }
  let host =
    unsafe { yuzuko_core::op::host_ref::<DomMatrixReadOnly>(scope, object) }?;
  let matrix = host.get();
  Some((matrix.value.get(), matrix.is_2d.get()))
}

fn number(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<f64, OpError> {
  <f64 as yuzuko_core::op::FromV8>::from_v8(scope, value)
}

#[op(name = "DOMMatrixReadOnly")]
impl DomMatrixReadOnly {
  #[constructor]
  fn new(
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<Self, OpError> {
    Ok(DomMatrix::new_ctor_impl(scope, args)?.base)
  }

  #[getter]
  fn a(&self) -> f64 {
    self.value.get().get(0)
  }
  #[getter]
  fn b(&self) -> f64 {
    self.value.get().get(1)
  }
  #[getter]
  fn c(&self) -> f64 {
    self.value.get().get(4)
  }
  #[getter]
  fn d(&self) -> f64 {
    self.value.get().get(5)
  }
  #[getter]
  fn e(&self) -> f64 {
    self.value.get().get(12)
  }
  #[getter]
  fn f(&self) -> f64 {
    self.value.get().get(13)
  }
  #[getter]
  fn m11(&self) -> f64 {
    self.value.get().get(0)
  }
  #[getter]
  fn m12(&self) -> f64 {
    self.value.get().get(1)
  }
  #[getter]
  fn m13(&self) -> f64 {
    self.value.get().get(2)
  }
  #[getter]
  fn m14(&self) -> f64 {
    self.value.get().get(3)
  }
  #[getter]
  fn m21(&self) -> f64 {
    self.value.get().get(4)
  }
  #[getter]
  fn m22(&self) -> f64 {
    self.value.get().get(5)
  }
  #[getter]
  fn m23(&self) -> f64 {
    self.value.get().get(6)
  }
  #[getter]
  fn m24(&self) -> f64 {
    self.value.get().get(7)
  }
  #[getter]
  fn m31(&self) -> f64 {
    self.value.get().get(8)
  }
  #[getter]
  fn m32(&self) -> f64 {
    self.value.get().get(9)
  }
  #[getter]
  fn m33(&self) -> f64 {
    self.value.get().get(10)
  }
  #[getter]
  fn m34(&self) -> f64 {
    self.value.get().get(11)
  }
  #[getter]
  fn m41(&self) -> f64 {
    self.value.get().get(12)
  }
  #[getter]
  fn m42(&self) -> f64 {
    self.value.get().get(13)
  }
  #[getter]
  fn m43(&self) -> f64 {
    self.value.get().get(14)
  }
  #[getter]
  fn m44(&self) -> f64 {
    self.value.get().get(15)
  }
  #[getter]
  fn is_identity(&self) -> bool {
    self.value.get().is_identity()
  }
  #[getter]
  #[rename("is2D")]
  fn is_2d(&self) -> bool {
    self.is_2d.get()
  }

  #[static_method]
  #[length(0)]
  fn from_matrix(
    scope: &mut v8::PinScope,
    init: v8::Local<v8::Value>,
  ) -> Result<New<DomMatrixReadOnly>, OpError> {
    let (value, is_2d) = matrix_operand(scope, init)?;
    Ok(New(DomMatrixReadOnly::from_parts(value, is_2d)))
  }

  #[static_method]
  #[rename("fromFloat32Array")]
  fn from_float32_array(
    scope: &mut v8::PinScope,
    array: v8::Local<v8::Value>,
  ) -> Result<New<DomMatrixReadOnly>, OpError> {
    readonly_from_floats(scope, array, true)
  }

  #[static_method]
  #[rename("fromFloat64Array")]
  fn from_float64_array(
    scope: &mut v8::PinScope,
    array: v8::Local<v8::Value>,
  ) -> Result<New<DomMatrixReadOnly>, OpError> {
    readonly_from_floats(scope, array, false)
  }

  #[rename("toJSON")]
  fn to_json(&self, scope: &mut v8::PinScope) -> Js {
    matrix_json(scope, self.value.get(), self.is_2d.get())
  }

  fn translate(
    &self,
    tx: Option<f64>,
    ty: Option<f64>,
    tz: Option<f64>,
  ) -> New<DomMatrix> {
    New(self.updated(|value, is_2d| {
      value.translate(
        tx.unwrap_or(0.0),
        ty.unwrap_or(0.0),
        tz.unwrap_or(0.0),
        is_2d,
      );
    }))
  }

  fn scale(
    &self,
    scale_x: Option<f64>,
    scale_y: Option<f64>,
    scale_z: Option<f64>,
    origin_x: Option<f64>,
    origin_y: Option<f64>,
    origin_z: Option<f64>,
  ) -> New<DomMatrix> {
    New(self.scaled_by(
      scale_x.unwrap_or(1.0),
      scale_y.unwrap_or(scale_x.unwrap_or(1.0)),
      scale_z.unwrap_or(1.0),
      origin_x.unwrap_or(0.0),
      origin_y.unwrap_or(0.0),
      origin_z.unwrap_or(0.0),
    ))
  }

  #[rename("scaleNonUniform")]
  fn scale_non_uniform(
    &self,
    scale_x: Option<f64>,
    scale_y: Option<f64>,
  ) -> New<DomMatrix> {
    New(self.scaled_by(
      scale_x.unwrap_or(1.0),
      scale_y.unwrap_or(1.0),
      1.0,
      0.0,
      0.0,
      0.0,
    ))
  }

  fn scale3d(
    &self,
    scale: Option<f64>,
    origin_x: Option<f64>,
    origin_y: Option<f64>,
    origin_z: Option<f64>,
  ) -> New<DomMatrix> {
    let scale = scale.unwrap_or(1.0);
    New(self.scaled_by(
      scale,
      scale,
      scale,
      origin_x.unwrap_or(0.0),
      origin_y.unwrap_or(0.0),
      origin_z.unwrap_or(0.0),
    ))
  }

  fn rotate(
    &self,
    rot_x: Option<f64>,
    rot_y: Option<f64>,
    rot_z: Option<f64>,
  ) -> New<DomMatrix> {
    New(self.updated(|value, is_2d| {
      if rot_y.is_none() && rot_z.is_none() {
        value.rotate_axis(
          0.0,
          0.0,
          1.0,
          rot_x.unwrap_or(0.0).to_radians(),
          is_2d,
        );
      } else {
        value.rotate_axis(
          0.0,
          0.0,
          1.0,
          rot_z.unwrap_or(0.0).to_radians(),
          is_2d,
        );
        value.rotate_axis(
          0.0,
          1.0,
          0.0,
          rot_y.unwrap_or(0.0).to_radians(),
          is_2d,
        );
        value.rotate_axis(
          1.0,
          0.0,
          0.0,
          rot_x.unwrap_or(0.0).to_radians(),
          is_2d,
        );
      }
    }))
  }

  #[rename("rotateFromVector")]
  fn rotate_from_vector(
    &self,
    x: Option<f64>,
    y: Option<f64>,
  ) -> New<DomMatrix> {
    let x = x.unwrap_or(0.0);
    let y = y.unwrap_or(0.0);
    if x == 0.0 && y == 0.0 {
      return New(self.updated(|_, _| {}));
    }
    let angle = y.atan2(x);
    New(self.updated(|value, is_2d| {
      value.rotate_axis(0.0, 0.0, 1.0, angle, is_2d);
    }))
  }

  #[rename("rotateAxisAngle")]
  fn rotate_axis_angle(
    &self,
    x: Option<f64>,
    y: Option<f64>,
    z: Option<f64>,
    angle: Option<f64>,
  ) -> New<DomMatrix> {
    New(self.updated(|value, is_2d| {
      value.rotate_axis(
        x.unwrap_or(0.0),
        y.unwrap_or(0.0),
        z.unwrap_or(0.0),
        angle.unwrap_or(0.0).to_radians(),
        is_2d,
      );
    }))
  }

  #[rename("skewX")]
  fn skew_x(&self, angle: Option<f64>) -> New<DomMatrix> {
    New(self.updated(|value, _| {
      value.skew(angle.unwrap_or(0.0).to_radians(), 0.0);
    }))
  }

  #[rename("skewY")]
  fn skew_y(&self, angle: Option<f64>) -> New<DomMatrix> {
    New(self.updated(|value, _| {
      value.skew(0.0, angle.unwrap_or(0.0).to_radians());
    }))
  }

  #[length(0)]
  fn multiply(
    &self,
    scope: &mut v8::PinScope,
    other: v8::Local<v8::Value>,
  ) -> Result<New<DomMatrix>, OpError> {
    let (right, right_2d) = operand_or_identity(scope, other)?;
    let value = Mat4::multiply(&self.value.get(), &right);
    Ok(New(DomMatrix::from_parts(
      value,
      self.is_2d.get() && right_2d,
    )))
  }

  #[rename("flipX")]
  fn flip_x(&self) -> New<DomMatrix> {
    New(self.scaled_by(-1.0, 1.0, 1.0, 0.0, 0.0, 0.0))
  }

  #[rename("flipY")]
  fn flip_y(&self) -> New<DomMatrix> {
    New(self.scaled_by(1.0, -1.0, 1.0, 0.0, 0.0, 0.0))
  }

  fn inverse(&self) -> New<DomMatrix> {
    let (value, is_2d) = inverted(self.value.get(), self.is_2d.get());
    New(DomMatrix::from_parts(value, is_2d))
  }

  #[length(0)]
  #[rename("transformPoint")]
  fn transform_point(
    &self,
    scope: &mut v8::PinScope,
    point: v8::Local<v8::Value>,
  ) -> New<crate::geom::geometry::DomPoint> {
    transformed_point(scope, self.value.get(), point)
  }

  #[rename("toFloat32Array")]
  fn to_float32_array(&self, scope: &mut v8::PinScope) -> Js {
    float_array(scope, &self.value.get().columns(), true)
  }

  #[rename("toFloat64Array")]
  fn to_float64_array(&self, scope: &mut v8::PinScope) -> Js {
    float_array(scope, &self.value.get().columns(), false)
  }

  #[rename("toString")]
  fn to_string(&self, scope: &mut v8::PinScope) -> Result<String, OpError> {
    matrix_string(scope, self.value.get(), self.is_2d.get())
  }
}

#[op(name = "DOMMatrix", inherit = DomMatrixReadOnly)]
impl DomMatrix {
  #[constructor]
  fn new(
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<DomMatrix, OpError> {
    if args.length() == 0 || args.get(0).is_undefined() {
      return Ok(DomMatrix::from_matrix(Matrix::IDENTITY));
    }
    let init = args.get(0);
    if let Some((matrix, is_2d)) = read_dom_matrix_parts(scope, init) {
      return Ok(DomMatrix::from_parts(matrix, is_2d));
    }
    // Workers have no CSS stringifier and no CSS-string constructor.
    if yuzuko_core::is_dedicated_worker()
      && (init.is_null() || init.is_string())
    {
      return Err(OpError::type_(
        "DOMMatrix does not accept a CSS string in a worker",
      ));
    }
    // `null` is the DOMString `"null"` in this union, not a sequence.
    if init.is_null() {
      return DomMatrix::from_css("null");
    }
    if init.is_string() {
      let text = <String as yuzuko_core::op::FromV8>::from_v8(scope, init)?;
      return DomMatrix::from_css(&text);
    }
    if let Some(values) = read_sequence(scope, init)? {
      return matrix_from_numbers(&values);
    }
    Err(OpError::type_("The matrix init is not a sequence"))
  }

  #[getter]
  fn a(&self) -> f64 {
    self.component(0)
  }
  #[setter]
  fn a(&self, value: f64) {
    self.set_component(0, value);
  }
  #[getter]
  fn b(&self) -> f64 {
    self.component(1)
  }
  #[setter]
  fn b(&self, value: f64) {
    self.set_component(1, value);
  }
  #[getter]
  fn c(&self) -> f64 {
    self.component(4)
  }
  #[setter]
  fn c(&self, value: f64) {
    self.set_component(4, value);
  }
  #[getter]
  fn d(&self) -> f64 {
    self.component(5)
  }
  #[setter]
  fn d(&self, value: f64) {
    self.set_component(5, value);
  }
  #[getter]
  fn e(&self) -> f64 {
    self.component(12)
  }
  #[setter]
  fn e(&self, value: f64) {
    self.set_component(12, value);
  }
  #[getter]
  fn f(&self) -> f64 {
    self.component(13)
  }
  #[setter]
  fn f(&self, value: f64) {
    self.set_component(13, value);
  }

  #[getter]
  fn m11(&self) -> f64 {
    self.component(0)
  }
  #[setter]
  fn m11(&self, value: f64) {
    self.set_component(0, value);
  }
  #[getter]
  fn m12(&self) -> f64 {
    self.component(1)
  }
  #[setter]
  fn m12(&self, value: f64) {
    self.set_component(1, value);
  }
  #[getter]
  fn m13(&self) -> f64 {
    self.component(2)
  }
  #[setter]
  fn m13(&self, value: f64) {
    self.set_3d(2, value, 0.0);
  }
  #[getter]
  fn m14(&self) -> f64 {
    self.component(3)
  }
  #[setter]
  fn m14(&self, value: f64) {
    self.set_3d(3, value, 0.0);
  }
  #[getter]
  fn m21(&self) -> f64 {
    self.component(4)
  }
  #[setter]
  fn m21(&self, value: f64) {
    self.set_component(4, value);
  }
  #[getter]
  fn m22(&self) -> f64 {
    self.component(5)
  }
  #[setter]
  fn m22(&self, value: f64) {
    self.set_component(5, value);
  }
  #[getter]
  fn m23(&self) -> f64 {
    self.component(6)
  }
  #[setter]
  fn m23(&self, value: f64) {
    self.set_3d(6, value, 0.0);
  }
  #[getter]
  fn m24(&self) -> f64 {
    self.component(7)
  }
  #[setter]
  fn m24(&self, value: f64) {
    self.set_3d(7, value, 0.0);
  }
  #[getter]
  fn m31(&self) -> f64 {
    self.component(8)
  }
  #[setter]
  fn m31(&self, value: f64) {
    self.set_3d(8, value, 0.0);
  }
  #[getter]
  fn m32(&self) -> f64 {
    self.component(9)
  }
  #[setter]
  fn m32(&self, value: f64) {
    self.set_3d(9, value, 0.0);
  }
  #[getter]
  fn m33(&self) -> f64 {
    self.component(10)
  }
  #[setter]
  fn m33(&self, value: f64) {
    self.set_3d(10, value, 1.0);
  }
  #[getter]
  fn m34(&self) -> f64 {
    self.component(11)
  }
  #[setter]
  fn m34(&self, value: f64) {
    self.set_3d(11, value, 0.0);
  }
  #[getter]
  fn m41(&self) -> f64 {
    self.component(12)
  }
  #[setter]
  fn m41(&self, value: f64) {
    self.set_component(12, value);
  }
  #[getter]
  fn m42(&self) -> f64 {
    self.component(13)
  }
  #[setter]
  fn m42(&self, value: f64) {
    self.set_component(13, value);
  }
  #[getter]
  fn m43(&self) -> f64 {
    self.component(14)
  }
  #[setter]
  fn m43(&self, value: f64) {
    self.set_3d(14, value, 0.0);
  }
  #[getter]
  fn m44(&self) -> f64 {
    self.component(15)
  }
  #[setter]
  fn m44(&self, value: f64) {
    self.set_3d(15, value, 1.0);
  }

  #[getter]
  fn is_identity(&self) -> bool {
    self.base.value.get().is_identity()
  }

  #[getter]
  #[rename("is2D")]
  fn is_2d(&self) -> bool {
    self.base.is_2d.get()
  }

  #[length(0)]
  fn multiply_self(
    &self,
    #[this] this: v8::Local<v8::Object>,
    scope: &mut v8::PinScope,
    other: v8::Local<v8::Value>,
  ) -> Result<Js, OpError> {
    let (right, right_2d) = matrix_operand(scope, other)?;
    let left = self.base.value.get();
    self.base.value.set(Mat4::multiply(&left, &right));
    self.base.is_2d.set(self.base.is_2d.get() && right_2d);
    Ok(same_object(scope, this))
  }

  #[length(0)]
  fn pre_multiply_self(
    &self,
    #[this] this: v8::Local<v8::Object>,
    scope: &mut v8::PinScope,
    other: v8::Local<v8::Value>,
  ) -> Result<Js, OpError> {
    let (left, left_2d) = matrix_operand(scope, other)?;
    let right = self.base.value.get();
    self.base.value.set(Mat4::multiply(&left, &right));
    self.base.is_2d.set(self.base.is_2d.get() && left_2d);
    Ok(same_object(scope, this))
  }

  #[length(0)]
  fn translate_self(
    &self,
    #[this] this: v8::Local<v8::Object>,
    scope: &mut v8::PinScope,
    x: f64,
    y: Option<f64>,
    z: Option<f64>,
  ) -> Js {
    self.map_2d(|matrix, is_2d| {
      matrix.translate(x, y.unwrap_or(0.0), z.unwrap_or(0.0), is_2d);
    });
    same_object(scope, this)
  }

  #[length(0)]
  fn scale_self(
    &self,
    #[this] this: v8::Local<v8::Object>,
    scope: &mut v8::PinScope,
    x: f64,
    y: Option<f64>,
  ) -> Js {
    let scaled = self.scaled(x, y.unwrap_or(x), 1.0, 0.0, 0.0, 0.0);
    self.base.value.set(scaled.base.value.get());
    self.base.is_2d.set(scaled.base.is_2d.get());
    same_object(scope, this)
  }

  #[length(0)]
  #[rename("scale3dSelf")]
  fn scale3d_self(
    &self,
    #[this] this: v8::Local<v8::Object>,
    scope: &mut v8::PinScope,
    scale: f64,
  ) -> Js {
    let scaled = self.scaled(scale, scale, scale, 0.0, 0.0, 0.0);
    self.base.value.set(scaled.base.value.get());
    self.base.is_2d.set(false);
    same_object(scope, this)
  }

  #[length(0)]
  fn rotate_self(
    &self,
    #[this] this: v8::Local<v8::Object>,
    scope: &mut v8::PinScope,
    angle: f64,
  ) -> Js {
    self.map_2d(|matrix, is_2d| {
      matrix.rotate_axis(0.0, 0.0, 1.0, angle.to_radians(), is_2d);
    });
    same_object(scope, this)
  }

  #[rename("rotateFromVectorSelf")]
  fn rotate_from_vector_self(
    &self,
    #[this] this: v8::Local<v8::Object>,
    scope: &mut v8::PinScope,
    x: Option<f64>,
    y: Option<f64>,
  ) -> Js {
    self.adopt(self.base.rotate_from_vector_call_impl(x, y));
    same_object(scope, this)
  }

  #[rename("rotateAxisAngleSelf")]
  fn rotate_axis_angle_self(
    &self,
    #[this] this: v8::Local<v8::Object>,
    scope: &mut v8::PinScope,
    x: Option<f64>,
    y: Option<f64>,
    z: Option<f64>,
    angle: Option<f64>,
  ) -> Js {
    self.adopt(self.base.rotate_axis_angle_call_impl(x, y, z, angle));
    same_object(scope, this)
  }

  #[rename("skewXSelf")]
  fn skew_x_self(
    &self,
    #[this] this: v8::Local<v8::Object>,
    scope: &mut v8::PinScope,
    angle: Option<f64>,
  ) -> Js {
    self.adopt(self.base.skew_x_call_impl(angle));
    same_object(scope, this)
  }

  #[rename("skewYSelf")]
  fn skew_y_self(
    &self,
    #[this] this: v8::Local<v8::Object>,
    scope: &mut v8::PinScope,
    angle: Option<f64>,
  ) -> Js {
    self.adopt(self.base.skew_y_call_impl(angle));
    same_object(scope, this)
  }

  fn invert_self(
    &self,
    #[this] this: v8::Local<v8::Object>,
    scope: &mut v8::PinScope,
  ) -> Js {
    let (value, is_2d) = inverted(self.base.value.get(), self.base.is_2d.get());
    self.base.value.set(value);
    self.base.is_2d.set(is_2d);
    same_object(scope, this)
  }

  #[rename("setMatrixValue")]
  fn set_matrix_value(
    &self,
    #[this] this: v8::Local<v8::Object>,
    scope: &mut v8::PinScope,
    transform: String,
  ) -> Result<Js, OpError> {
    let parsed = DomMatrix::from_css(&transform)?;
    self.base.value.set(parsed.base.value.get());
    self.base.is_2d.set(parsed.base.is_2d.get());
    Ok(same_object(scope, this))
  }

  #[static_method]
  #[length(0)]
  fn from_matrix(
    scope: &mut v8::PinScope,
    init: v8::Local<v8::Value>,
  ) -> Result<New<DomMatrix>, OpError> {
    let (value, is_2d) = matrix_operand(scope, init)?;
    Ok(New(DomMatrix::from_parts(value, is_2d)))
  }

  #[static_method]
  #[rename("fromFloat32Array")]
  fn from_float32_array(
    scope: &mut v8::PinScope,
    array: v8::Local<v8::Value>,
  ) -> Result<New<DomMatrix>, OpError> {
    mutable_from_floats(scope, array, true)
  }

  #[static_method]
  #[rename("fromFloat64Array")]
  fn from_float64_array(
    scope: &mut v8::PinScope,
    array: v8::Local<v8::Value>,
  ) -> Result<New<DomMatrix>, OpError> {
    mutable_from_floats(scope, array, false)
  }
}

fn inverted(value: Mat4, is_2d: bool) -> (Mat4, bool) {
  if is_2d {
    return match value.inverse_2d() {
      Some(inverse) => (inverse, true),
      None => (Mat4::nan(), false),
    };
  }
  match value.inverse() {
    Some(inverse) => (inverse, false),
    None => (Mat4::nan(), false),
  }
}

impl DomMatrix {
  fn cloned(&self) -> Self {
    Self::from_parts(self.base.value.get(), self.base.is_2d.get())
  }

  fn scaled(
    &self,
    scale_x: f64,
    scale_y: f64,
    scale_z: f64,
    origin_x: f64,
    origin_y: f64,
    origin_z: f64,
  ) -> Self {
    let matrix = self.cloned();
    matrix.map_2d(|value, is_2d| {
      value.translate(origin_x, origin_y, origin_z, is_2d);
      value.scale(scale_x, scale_y, scale_z, is_2d);
      value.translate(-origin_x, -origin_y, -origin_z, is_2d);
    });
    matrix
  }

  fn map_2d(&self, update: impl FnOnce(&mut Mat4, &mut bool)) {
    let mut matrix = self.base.value.get();
    let mut is_2d = self.base.is_2d.get();
    update(&mut matrix, &mut is_2d);
    self.base.value.set(matrix);
    self.base.is_2d.set(is_2d);
  }
}

const TAG_MATRIX_READ_ONLY: u32 = 9;
const TAG_MATRIX: u32 = 10;

pub(crate) fn register_clone() {
  yuzuko_core::add_host_clone(write_matrix_clone, read_matrix_clone);
}

fn write_matrix_clone(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  helper: &dyn v8::ValueSerializerHelper,
) -> yuzuko_core::WriteResult {
  let (tag, matrix, is_2d) = if let Some(host) =
    unsafe { yuzuko_core::op::host_ref::<DomMatrix>(scope, object) }
  {
    let matrix = host.get();
    (TAG_MATRIX, matrix.base.value.get(), matrix.base.is_2d.get())
  } else if let Some(host) =
    unsafe { yuzuko_core::op::host_ref::<DomMatrixReadOnly>(scope, object) }
  {
    let matrix = host.get();
    (TAG_MATRIX_READ_ONLY, matrix.value.get(), matrix.is_2d.get())
  } else {
    return yuzuko_core::WriteResult::Skip;
  };
  helper.write_uint32(tag);
  helper.write_uint32(u32::from(is_2d));
  let columns = matrix.columns();
  if is_2d {
    for index in [0, 1, 4, 5, 12, 13] {
      helper.write_double(columns[index]);
    }
  } else {
    for value in columns {
      helper.write_double(value);
    }
  }
  yuzuko_core::WriteResult::Wrote
}

fn read_matrix_clone(
  scope: &mut v8::PinScope,
  tag: u32,
  helper: &dyn v8::ValueDeserializerHelper,
) -> yuzuko_core::ReadResult {
  if tag != TAG_MATRIX && tag != TAG_MATRIX_READ_ONLY {
    return yuzuko_core::ReadResult::Skip;
  }
  let mut is_2d = 0u32;
  if !helper.read_uint32(&mut is_2d) {
    yuzuko_core::reject_clone(scope);
    return yuzuko_core::ReadResult::Failed;
  }
  let count = if is_2d == 1 { 6 } else { 16 };
  let Some(values) = read_doubles(helper, count) else {
    yuzuko_core::reject_clone(scope);
    return yuzuko_core::ReadResult::Failed;
  };
  let (matrix, two_d) = if is_2d == 1 {
    (
      Mat4::from_2d_coeffs(
        values[0], values[1], values[2], values[3], values[4], values[5],
      ),
      true,
    )
  } else {
    let mut columns = [0.0; 16];
    columns.copy_from_slice(&values);
    (Mat4::from_column(columns), false)
  };
  let object = if tag == TAG_MATRIX {
    unsafe {
      yuzuko_core::op::wrap_instance(
        scope,
        DomMatrix::from_parts(matrix, two_d),
      )
    }
  } else {
    unsafe {
      yuzuko_core::op::wrap_instance(
        scope,
        DomMatrixReadOnly::from_parts(matrix, two_d),
      )
    }
  };
  yuzuko_core::ReadResult::Done(v8::Global::new(scope, object))
}

fn read_doubles(
  helper: &dyn v8::ValueDeserializerHelper,
  count: usize,
) -> Option<Vec<f64>> {
  let mut values = Vec::with_capacity(count);
  for _ in 0..count {
    let mut value = 0.0;
    if !helper.read_double(&mut value) {
      return None;
    }
    values.push(value);
  }
  Some(values)
}

fn same_object(scope: &mut v8::PinScope, object: v8::Local<v8::Object>) -> Js {
  let value: v8::Local<v8::Value> = object.into();
  Js(v8::Global::new(scope, value))
}

fn css_number(scope: &mut v8::PinScope, value: f64) -> String {
  // CSS number serialization matches JavaScript's number `toString`.
  if value == 0.0 {
    return "0".to_string();
  }
  let number = v8::Number::new(scope, value);
  number
    .to_string(scope)
    .map(|text| text.to_rust_string_lossy(scope))
    .unwrap_or_else(|| "0".to_string())
}

fn matrix_string(
  scope: &mut v8::PinScope,
  value: Mat4,
  is_2d: bool,
) -> Result<String, OpError> {
  let columns = value.columns();
  if columns.iter().any(|value| !value.is_finite()) {
    return Err(OpError::dom(
      "InvalidStateError",
      "The matrix contains non-finite values",
    ));
  }
  let text = if is_2d {
    format!(
      "matrix({}, {}, {}, {}, {}, {})",
      css_number(scope, columns[0]),
      css_number(scope, columns[1]),
      css_number(scope, columns[4]),
      css_number(scope, columns[5]),
      css_number(scope, columns[12]),
      css_number(scope, columns[13]),
    )
  } else {
    let parts: Vec<String> = columns
      .iter()
      .copied()
      .map(|value| css_number(scope, value))
      .collect();
    format!("matrix3d({})", parts.join(", "))
  };
  Ok(text)
}

fn operand_or_identity(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<(Mat4, bool), OpError> {
  if value.is_undefined() || value.is_null() {
    return Ok((Mat4::identity(), true));
  }
  matrix_operand(scope, value)
}

fn transformed_point(
  scope: &mut v8::PinScope,
  matrix: Mat4,
  point: v8::Local<v8::Value>,
) -> New<crate::geom::geometry::DomPoint> {
  let (x, y, z, w) = if point.is_undefined() || point.is_null() {
    (0.0, 0.0, 0.0, 1.0)
  } else {
    crate::geom::geometry::point_init(scope, point)
  };
  New(crate::geom::geometry::DomPoint::at(
    matrix[(0, 0)] * x
      + matrix[(0, 1)] * y
      + matrix[(0, 2)] * z
      + matrix[(0, 3)] * w,
    matrix[(1, 0)] * x
      + matrix[(1, 1)] * y
      + matrix[(1, 2)] * z
      + matrix[(1, 3)] * w,
    matrix[(2, 0)] * x
      + matrix[(2, 1)] * y
      + matrix[(2, 2)] * z
      + matrix[(2, 3)] * w,
    matrix[(3, 0)] * x
      + matrix[(3, 1)] * y
      + matrix[(3, 2)] * z
      + matrix[(3, 3)] * w,
  ))
}

fn read_sequence(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<Option<Vec<f64>>, OpError> {
  if let Ok(array) = v8::Local::<v8::Array>::try_from(value) {
    let length = array.length();
    let mut values = Vec::with_capacity(length as usize);
    for index in 0..length {
      let item = array.get_index(scope, index).unwrap();
      values.push(number(scope, item)?);
    }
    return Ok(Some(values));
  }
  if let Ok(typed) = v8::Local::<v8::TypedArray>::try_from(value) {
    let length = typed.length();
    let object = v8::Local::<v8::Object>::try_from(value).unwrap();
    let mut values = Vec::with_capacity(length);
    for index in 0..length {
      let item = object.get_index(scope, index as u32).unwrap();
      values.push(number(scope, item)?);
    }
    return Ok(Some(values));
  }
  Ok(None)
}

fn readonly_from_floats(
  scope: &mut v8::PinScope,
  array: v8::Local<v8::Value>,
  narrow: bool,
) -> Result<New<DomMatrixReadOnly>, OpError> {
  let matrix = mutable_from_floats(scope, array, narrow)?;
  Ok(New(DomMatrixReadOnly::from_parts(
    matrix.0.base.value.get(),
    matrix.0.base.is_2d.get(),
  )))
}

fn mutable_from_floats(
  scope: &mut v8::PinScope,
  array: v8::Local<v8::Value>,
  narrow: bool,
) -> Result<New<DomMatrix>, OpError> {
  let typed = if narrow {
    array.is_float32_array()
  } else {
    array.is_float64_array()
  };
  if !typed {
    return Err(OpError::type_(if narrow {
      "The matrix is not a Float32Array"
    } else {
      "The matrix is not a Float64Array"
    }));
  }
  let values = read_sequence(scope, array)?
    .ok_or_else(|| OpError::type_("The matrix is not a sequence"))?;
  Ok(New(matrix_from_numbers(&values)?))
}

fn matrix_json(scope: &mut v8::PinScope, value: Mat4, is_2d: bool) -> Js {
  let object = v8::Object::new(scope);
  let columns = value.columns();
  let names = [
    ("a", 0usize),
    ("b", 1),
    ("c", 4),
    ("d", 5),
    ("e", 12),
    ("f", 13),
    ("m11", 0),
    ("m12", 1),
    ("m13", 2),
    ("m14", 3),
    ("m21", 4),
    ("m22", 5),
    ("m23", 6),
    ("m24", 7),
    ("m31", 8),
    ("m32", 9),
    ("m33", 10),
    ("m34", 11),
    ("m41", 12),
    ("m42", 13),
    ("m43", 14),
    ("m44", 15),
  ];
  for (name, index) in names {
    let key = v8::String::new(scope, name).unwrap();
    let number = v8::Number::new(scope, columns[index]);
    object.set(scope, key.into(), number.into());
  }
  let key = v8::String::new(scope, "is2D").unwrap();
  object.set(scope, key.into(), v8::Boolean::new(scope, is_2d).into());
  let key = v8::String::new(scope, "isIdentity").unwrap();
  object.set(
    scope,
    key.into(),
    v8::Boolean::new(scope, value.is_identity()).into(),
  );
  let value: v8::Local<v8::Value> = object.into();
  Js(v8::Global::new(scope, value))
}

fn matrix_from_numbers(values: &[f64]) -> Result<DomMatrix, OpError> {
  match values.len() {
    6 => Ok(DomMatrix::from_matrix(Matrix {
      a: values[0],
      b: values[1],
      c: values[2],
      d: values[3],
      e: values[4],
      f: values[5],
    })),
    16 => {
      let mut data = [0.0; 16];
      data.copy_from_slice(values);
      Ok(DomMatrix::from_parts(Mat4::from_column(data), false))
    }
    _ => Err(OpError::type_("The matrix sequence length is not 6 or 16")),
  }
}

fn float_array(scope: &mut v8::PinScope, values: &[f64], narrow: bool) -> Js {
  let bytes = if narrow {
    let mut bytes = Vec::with_capacity(values.len() * 4);
    for value in values {
      bytes.extend_from_slice(&(*value as f32).to_le_bytes());
    }
    bytes
  } else {
    let mut bytes = Vec::with_capacity(values.len() * 8);
    for value in values {
      bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes
  };
  let length = values.len();
  let store = v8::ArrayBuffer::new_backing_store_from_vec(bytes).make_shared();
  let buffer = v8::ArrayBuffer::with_backing_store(scope, &store);
  let array: v8::Local<v8::Value> = if narrow {
    v8::Float32Array::new(scope, buffer, 0, length)
      .unwrap()
      .into()
  } else {
    v8::Float64Array::new(scope, buffer, 0, length)
      .unwrap()
      .into()
  };
  Js(v8::Global::new(scope, array))
}

fn matrix_operand(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<(Mat4, bool), OpError> {
  if let Some(parts) = read_dom_matrix_parts(scope, value) {
    return Ok(parts);
  }
  let object = v8::Local::<v8::Object>::try_from(value)
    .map_err(|_| OpError::type_("The matrix init is not an object"))?;
  let mut present = [false; 16];
  let mut numbers = [0.0; 16];
  let names = [
    "m11", "m12", "m13", "m14", "m21", "m22", "m23", "m24", "m31", "m32",
    "m33", "m34", "m41", "m42", "m43", "m44",
  ];
  for (index, name) in names.iter().enumerate() {
    if let Some(number) = optional_member(scope, object, name) {
      present[index] = true;
      numbers[index] = number;
    }
  }
  let aliases = [("a", 0), ("b", 1), ("c", 4), ("d", 5), ("e", 12), ("f", 13)];
  for (name, index) in aliases {
    let Some(number) = optional_member(scope, object, name) else {
      continue;
    };
    if present[index] && !same_number(numbers[index], number) {
      return Err(OpError::type_("Inconsistent 2d matrix value"));
    }
    if !present[index] {
      numbers[index] = number;
      present[index] = true;
    }
  }
  // `is2D` omitted stays 2D until a 3D member differs from its default.
  // An explicit `true` rejects those members. An explicit `false`, including
  // `null`, makes the result 3D.
  let is_2d_flag = optional_member(scope, object, "is2D");
  let mut is_2d = is_2d_flag.unwrap_or(1.0) != 0.0;
  let defaults = [
    1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0,
    1.0,
  ];
  let threed = [2, 3, 6, 7, 8, 9, 10, 11, 14, 15];
  if is_2d_flag == Some(1.0) {
    for index in threed {
      if present[index] && !same_number(numbers[index], defaults[index]) {
        return Err(OpError::type_("Inconsistent 2d matrix value"));
      }
    }
  } else if is_2d_flag.is_none() {
    for index in threed {
      if present[index] && !same_number(numbers[index], defaults[index]) {
        is_2d = false;
        break;
      }
    }
  }
  for (index, default) in defaults.iter().enumerate() {
    if !present[index] {
      numbers[index] = *default;
    }
  }
  // A 2D matrix is created from the six 2D elements. 3D members that were
  // only `-0` or the identity default become `+0` and `1`.
  if is_2d {
    return Ok((
      Mat4::from_2d_coeffs(
        numbers[0],
        numbers[1],
        numbers[4],
        numbers[5],
        numbers[12],
        numbers[13],
      ),
      true,
    ));
  }
  Ok((Mat4::from_column(numbers), false))
}

fn optional_member(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
) -> Option<f64> {
  let key = v8::String::new(scope, name).unwrap();
  let value = object.get(scope, key.into())?;
  if value.is_undefined() {
    return None;
  }
  if name == "is2D" {
    return Some(if value.boolean_value(scope) { 1.0 } else { 0.0 });
  }
  Some(value.number_value(scope).unwrap_or(f64::NAN))
}

fn same_number(left: f64, right: f64) -> bool {
  left == right || (left.is_nan() && right.is_nan())
}

fn length_px(length: &Length) -> Result<f64, OpError> {
  length.to_pixels().ok_or_else(|| {
    OpError::dom(
      "SyntaxError",
      "Failed to parse as CSS <transform-list>: relative lengths are not supported",
    )
  })
}

fn apply_css(
  matrix: &mut Mat4,
  is_2d: &mut bool,
  transform: &Transform,
) -> Result<(), OpError> {
  match transform {
    Transform::Translate(x, y) => {
      let y = match y {
        Some(y) => length_px(y)?,
        None => 0.0,
      };
      matrix.translate(length_px(x)?, y, 0.0, is_2d);
    }
    Transform::TranslateX(x) => {
      matrix.translate(length_px(x)?, 0.0, 0.0, is_2d);
    }
    Transform::TranslateY(y) => {
      matrix.translate(0.0, length_px(y)?, 0.0, is_2d);
    }
    Transform::TranslateZ(z) => {
      matrix.translate(0.0, 0.0, length_px(z)?, is_2d);
    }
    Transform::Translate3d(x, y, z) => {
      matrix.translate(length_px(x)?, length_px(y)?, length_px(z)?, is_2d);
      *is_2d = false;
    }
    Transform::Scale(x, y) => {
      matrix.scale(*x, y.unwrap_or(*x), 1.0, is_2d);
    }
    Transform::ScaleX(x) => matrix.scale(*x, 1.0, 1.0, is_2d),
    Transform::ScaleY(y) => matrix.scale(1.0, *y, 1.0, is_2d),
    Transform::ScaleZ(z) => {
      matrix.scale(1.0, 1.0, *z, is_2d);
      *is_2d = false;
    }
    Transform::Scale3d(x, y, z) => {
      matrix.scale(*x, *y, *z, is_2d);
      *is_2d = false;
    }
    Transform::Rotate(angle) => {
      matrix.rotate_axis(0.0, 0.0, 1.0, angle.to_radians(), is_2d);
    }
    Transform::RotateX(angle) => {
      matrix.rotate_axis(1.0, 0.0, 0.0, angle.to_radians(), is_2d);
      *is_2d = false;
    }
    Transform::RotateY(angle) => {
      matrix.rotate_axis(0.0, 1.0, 0.0, angle.to_radians(), is_2d);
      *is_2d = false;
    }
    Transform::RotateZ(angle) => {
      matrix.rotate_axis(0.0, 0.0, 1.0, angle.to_radians(), is_2d);
      *is_2d = false;
    }
    Transform::Rotate3d(x, y, z, angle) => {
      matrix.rotate_axis(*x, *y, *z, angle.to_radians(), is_2d);
      *is_2d = false;
    }
    Transform::Skew(x, y) => {
      let y = y.as_ref().map(Angle::to_radians).unwrap_or(0.0);
      matrix.skew(x.to_radians(), y);
    }
    Transform::SkewX(angle) => matrix.skew(angle.to_radians(), 0.0),
    Transform::SkewY(angle) => matrix.skew(0.0, angle.to_radians()),
    Transform::Perspective(length) => {
      if let Some(length) = length {
        matrix.perspective(length_px(length)?);
      }
      *is_2d = false;
    }
    Transform::Matrix([a, b, c, d, e, f]) => {
      let right = Mat4::from_2d_coeffs(*a, *b, *c, *d, *e, *f);
      *matrix = Mat4::multiply(matrix, &right);
    }
    Transform::Matrix3d(values) => {
      let right = Mat4::from_column(*values);
      *matrix = Mat4::multiply(matrix, &right);
      *is_2d = false;
    }
  }
  Ok(())
}

#[cfg(test)]
mod tests {
  use super::DomMatrix;

  fn near(actual: f64, expected: f64) {
    assert!((actual - expected).abs() < 1e-9, "{actual} != {expected}");
  }

  #[test]
  fn css_string_covers_math_functions_and_3d() {
    let plain = DomMatrix::from_css("matrix(1, 2, 3, 4, 5, 6)").unwrap();
    let matrix = plain.get();
    assert!(plain.base.is_2d.get());
    assert_eq!((matrix.a, matrix.b, matrix.c), (1.0, 2.0, 3.0));
    assert_eq!((matrix.d, matrix.e, matrix.f), (4.0, 5.0, 6.0));

    let translated = DomMatrix::from_css("translate(1in, 10px)").unwrap();
    assert_eq!(translated.get().e, 96.0);
    assert_eq!(translated.get().f, 10.0);

    let scaled = DomMatrix::from_css("scale(2) translate(10px)").unwrap();
    near(scaled.get().a, 2.0);
    near(scaled.get().e, 20.0);

    let calc = DomMatrix::from_css("translate(calc(10px + 5px))").unwrap();
    assert_eq!(calc.get().e, 15.0);
    let min = DomMatrix::from_css("translate(min(10px, 4px))").unwrap();
    assert_eq!(min.get().e, 4.0);
    let clamp =
      DomMatrix::from_css("translate(clamp(1px, 10px, 4px))").unwrap();
    assert_eq!(clamp.get().e, 4.0);
    let hypot =
      DomMatrix::from_css("translate(calc(hypot(3px, 4px)))").unwrap();
    near(hypot.get().e, 5.0);
    let sine = DomMatrix::from_css("scale(sin(90deg))").unwrap();
    near(sine.get().a, 1.0);
    near(sine.get().d, 1.0);

    let turned = DomMatrix::from_css("rotate(90deg)").unwrap();
    assert!(turned.base.is_2d.get());
    near(turned.get().a, 0.0);
    near(turned.get().b, 1.0);
    near(turned.get().c, -1.0);
    near(turned.get().d, 0.0);

    let pitched = DomMatrix::from_css("rotateX(90deg)").unwrap();
    assert!(!pitched.base.is_2d.get());
    near(pitched.component(5), 0.0);
    near(pitched.component(6), 1.0);
    near(pitched.component(9), -1.0);
    near(pitched.component(10), 0.0);

    let z_turn = DomMatrix::from_css("rotateZ(90deg)").unwrap();
    assert!(!z_turn.base.is_2d.get());
    near(z_turn.get().b, 1.0);

    let shifted = DomMatrix::from_css(
      "matrix3d(1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 5, 6, 7, 1)",
    )
    .unwrap();
    assert!(!shifted.base.is_2d.get());
    assert_eq!(shifted.component(12), 5.0);
    assert_eq!(shifted.component(13), 6.0);
    assert_eq!(shifted.component(14), 7.0);

    assert!(
      DomMatrix::from_css("none")
        .unwrap()
        .base
        .value
        .get()
        .is_identity()
    );
    assert!(
      DomMatrix::from_css("")
        .unwrap()
        .base
        .value
        .get()
        .is_identity()
    );
    assert!(DomMatrix::from_css("translate(10px, 20px, 30px)").is_err());
    assert!(DomMatrix::from_css("translate(1em)").is_err());
    assert!(DomMatrix::from_css("translateX(1vw)").is_err());
    assert!(DomMatrix::from_css("perspective(1cqw)").is_err());
  }
}
