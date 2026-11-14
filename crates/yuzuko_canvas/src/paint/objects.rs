//! `Path2D`, gradients, patterns, and `TextMetrics`.

#![allow(clippy::too_many_arguments)]

use std::cell::RefCell;

use kurbo::BezPath;
use yuzuko_core::op::OpError;
use yuzuko_macros::op;

use crate::geom::path;
use crate::paint::color::parse_color;
use crate::paint::record::ConicGradient;
use crate::paint::record::LiveGradient;
use crate::paint::record::LivePattern;
use crate::paint::record::{self};

pub struct Path2D {
  path: RefCell<BezPath>,
}

unsafe impl v8::cppgc::GarbageCollected for Path2D {
  fn trace(&self, _visitor: &mut v8::cppgc::Visitor) {}

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"Path2D"
  }
}

impl Path2D {
  pub fn empty() -> Self {
    Self {
      path: RefCell::new(BezPath::new()),
    }
  }

  pub fn from_path(path: BezPath) -> Self {
    Self {
      path: RefCell::new(path),
    }
  }

  pub fn borrow_path(&self) -> BezPath {
    self.path.borrow().clone()
  }
}

pub fn path_of(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Option<BezPath> {
  let object = v8::Local::<v8::Object>::try_from(value).ok()?;
  let host = unsafe { yuzuko_core::op::host_ref::<Path2D>(scope, object) }?;
  Some(host.get().borrow_path())
}

fn finite(values: &[f64]) -> bool {
  values.iter().all(|value| value.is_finite())
}

#[op]
impl Path2D {
  #[constructor]
  fn new(
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<Path2D, OpError> {
    if args.length() == 0 || args.get(0).is_undefined() {
      return Ok(Path2D::empty());
    }
    let init = args.get(0);
    if let Some(path) = path_of(scope, init) {
      return Ok(Path2D::from_path(path));
    }
    if init.is_string() {
      let text = init.to_rust_string_lossy(scope);
      let path = BezPath::from_svg(&text).unwrap_or_else(|_| BezPath::new());
      return Ok(Path2D::from_path(path));
    }
    Err(OpError::type_("Path2D constructor argument is invalid"))
  }

  fn add_path(
    &self,
    scope: &mut v8::PinScope,
    path: v8::Local<v8::Value>,
    matrix: Option<v8::Local<v8::Value>>,
  ) -> Result<(), OpError> {
    let Some(mut path) = path_of(scope, path) else {
      return Err(OpError::type_("Path2D.addPath requires a Path2D"));
    };
    if let Some(matrix) = matrix
      && !matrix.is_null_or_undefined()
    {
      let matrix = crate::geom::matrix::read_canvas_transform(scope, matrix)?;
      path.apply_affine(matrix.affine());
    }
    self
      .path
      .borrow_mut()
      .extend(path.elements().iter().copied());
    Ok(())
  }

  fn close_path(&self) {
    path::close(&mut self.path.borrow_mut());
  }

  fn move_to(&self, x: f64, y: f64) {
    path::move_to(&mut self.path.borrow_mut(), x, y);
  }

  fn line_to(&self, x: f64, y: f64) {
    path::line_to(&mut self.path.borrow_mut(), x, y);
  }

  fn quadratic_curve_to(&self, cpx: f64, cpy: f64, x: f64, y: f64) {
    path::quad_to(&mut self.path.borrow_mut(), cpx, cpy, x, y);
  }

  fn bezier_curve_to(
    &self,
    cp1x: f64,
    cp1y: f64,
    cp2x: f64,
    cp2y: f64,
    x: f64,
    y: f64,
  ) {
    path::curve_to(&mut self.path.borrow_mut(), cp1x, cp1y, cp2x, cp2y, x, y);
  }

  fn rect(&self, x: f64, y: f64, w: f64, h: f64) {
    path::add_rect(&mut self.path.borrow_mut(), x, y, w, h);
  }

  fn arc(
    &self,
    x: f64,
    y: f64,
    radius: f64,
    start: f64,
    end: f64,
    counterclockwise: Option<bool>,
  ) -> Result<(), OpError> {
    path::add_ellipse(
      &mut self.path.borrow_mut(),
      x,
      y,
      radius,
      radius,
      0.0,
      start,
      end,
      counterclockwise.unwrap_or(false),
    )
  }

  fn ellipse(
    &self,
    x: f64,
    y: f64,
    radius_x: f64,
    radius_y: f64,
    rotation: f64,
    start: f64,
    end: f64,
    counterclockwise: Option<bool>,
  ) -> Result<(), OpError> {
    path::add_ellipse(
      &mut self.path.borrow_mut(),
      x,
      y,
      radius_x,
      radius_y,
      rotation,
      start,
      end,
      counterclockwise.unwrap_or(false),
    )
  }

  fn arc_to(
    &self,
    x1: f64,
    y1: f64,
    x2: f64,
    y2: f64,
    radius: f64,
  ) -> Result<(), OpError> {
    path::arc_to(&mut self.path.borrow_mut(), x1, y1, x2, y2, radius)
  }

  fn round_rect(
    &self,
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<(), OpError> {
    if args.length() < 4 {
      return Err(OpError::type_("roundRect requires a rectangle"));
    }
    let x = number(scope, args.get(0))?;
    let y = number(scope, args.get(1))?;
    let w = number(scope, args.get(2))?;
    let h = number(scope, args.get(3))?;
    let radii = if args.length() >= 5 {
      radii_of(scope, args.get(4))?
    } else {
      [0.0; 8]
    };
    if finite(&[x, y, w, h]) && finite(&radii) {
      path::add_round_rect(&mut self.path.borrow_mut(), x, y, w, h, radii);
    }
    Ok(())
  }
}

pub struct CanvasGradient {
  id: u64,
}

unsafe impl v8::cppgc::GarbageCollected for CanvasGradient {
  fn trace(&self, _visitor: &mut v8::cppgc::Visitor) {}

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"CanvasGradient"
  }
}

impl Drop for CanvasGradient {
  fn drop(&mut self) {
    record::drop_gradient(self.id);
  }
}

impl CanvasGradient {
  pub fn linear(x0: f64, y0: f64, x1: f64, y1: f64) -> Self {
    let id = record::alloc_gradient(LiveGradient {
      linear: Some(((x0 as f32, y0 as f32), (x1 as f32, y1 as f32))),
      radial: None,
      conic: None,
      stops: Vec::new(),
    });
    Self { id }
  }

  pub fn radial(
    x0: f64,
    y0: f64,
    r0: f64,
    x1: f64,
    y1: f64,
    r1: f64,
  ) -> Result<Self, OpError> {
    if r0 < 0.0 || r1 < 0.0 {
      return Err(OpError::dom(
        "IndexSizeError",
        "The gradient radius is negative",
      ));
    }
    let id = record::alloc_gradient(LiveGradient {
      linear: None,
      radial: Some((
        (x0 as f32, y0 as f32, r0 as f32),
        (x1 as f32, y1 as f32, r1 as f32),
      )),
      conic: None,
      stops: Vec::new(),
    });
    Ok(Self { id })
  }

  pub fn conic(start_angle: f64, x: f64, y: f64) -> Self {
    let id = record::alloc_gradient(LiveGradient {
      linear: None,
      radial: None,
      conic: Some(ConicGradient {
        start: start_angle as f32,
        center: (x as f32, y as f32),
      }),
      stops: Vec::new(),
    });
    Self { id }
  }

  pub fn id(&self) -> u64 {
    self.id
  }
}

pub fn gradient_id(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Option<u64> {
  let object = v8::Local::<v8::Object>::try_from(value).ok()?;
  let host =
    unsafe { yuzuko_core::op::host_ref::<CanvasGradient>(scope, object) }?;
  Some(host.get().id())
}

#[op]
impl CanvasGradient {
  fn add_color_stop(&self, offset: f64, color: String) -> Result<(), OpError> {
    if !offset.is_finite() {
      return Err(OpError::type_("The color stop offset is not finite"));
    }
    if !(0.0..=1.0).contains(&offset) {
      return Err(OpError::dom(
        "IndexSizeError",
        "The color stop offset is out of range",
      ));
    }
    let Some(color) = parse_color(&color) else {
      return Err(OpError::dom(
        "SyntaxError",
        "The color stop is not a CSS color",
      ));
    };
    record::with_gradient(self.id, |gradient| {
      gradient.stops.push((offset as f32, color));
      gradient.stops.sort_by(|left, right| {
        left
          .0
          .partial_cmp(&right.0)
          .unwrap_or(std::cmp::Ordering::Equal)
      });
    });
    Ok(())
  }
}

pub struct CanvasPattern {
  id: u64,
}

unsafe impl v8::cppgc::GarbageCollected for CanvasPattern {
  fn trace(&self, _visitor: &mut v8::cppgc::Visitor) {}

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"CanvasPattern"
  }
}

impl Drop for CanvasPattern {
  fn drop(&mut self) {
    record::drop_pattern(self.id);
  }
}

impl CanvasPattern {
  pub fn new(pattern: LivePattern) -> Self {
    Self {
      id: record::alloc_pattern(pattern),
    }
  }

  pub fn id(&self) -> u64 {
    self.id
  }
}

pub fn pattern_id(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Option<u64> {
  let object = v8::Local::<v8::Object>::try_from(value).ok()?;
  let host =
    unsafe { yuzuko_core::op::host_ref::<CanvasPattern>(scope, object) }?;
  Some(host.get().id())
}

#[op]
impl CanvasPattern {
  fn set_transform(
    &self,
    _scope: &mut v8::PinScope,
    _transform: Option<v8::Local<v8::Value>>,
  ) {
    let _ = self;
  }
}

pub struct TextMetrics {
  width: f64,
  actual_left: f64,
  actual_right: f64,
  actual_ascent: f64,
  actual_descent: f64,
  font_ascent: f64,
  font_descent: f64,
  em_ascent: f64,
  em_descent: f64,
  hanging: f64,
  alphabetic: f64,
  ideographic: f64,
}

unsafe impl v8::cppgc::GarbageCollected for TextMetrics {
  fn trace(&self, _visitor: &mut v8::cppgc::Visitor) {}

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"TextMetrics"
  }
}

impl TextMetrics {
  /// Ink bounds relative to `textAlign` and `textBaseline`.
  ///
  /// `actualBoundingBox*` is the glyph-outline box. An empty outline reports
  /// zeros. Font and em boxes use the typographic ascent and descent, shifted
  /// onto the selected baseline. Positive baseline distances point down.
  pub fn from_shaped(
    shaped: &crate::text::ShapedText,
    align: crate::paint::record::Align,
    baseline: crate::paint::record::Baseline,
    rtl: bool,
  ) -> Self {
    let width = f64::from(shaped.width);
    let ascent = f64::from(shaped.ascent);
    let descent = f64::from(shaped.descent);
    let align = align.offset(rtl, width);
    let baseline = baseline.offset(ascent, descent);
    let (actual_left, actual_right, actual_ascent, actual_descent) =
      match shaped.ink_union() {
        Some(ink) => (
          -(align + f64::from(ink.x0)),
          align + f64::from(ink.x1),
          -(baseline + f64::from(ink.y0)),
          baseline + f64::from(ink.y1),
        ),
        None => (0.0, 0.0, 0.0, 0.0),
      };
    Self {
      width,
      actual_left,
      actual_right,
      actual_ascent,
      actual_descent,
      font_ascent: ascent - baseline,
      font_descent: descent + baseline,
      em_ascent: ascent - baseline,
      em_descent: descent + baseline,
      hanging: baseline - 0.8 * ascent,
      alphabetic: baseline,
      ideographic: baseline + descent,
    }
  }
}

#[op]
impl TextMetrics {
  #[getter]
  fn width(&self) -> f64 {
    self.width
  }

  #[getter]
  fn actual_bounding_box_ascent(&self) -> f64 {
    self.actual_ascent
  }

  #[getter]
  fn actual_bounding_box_descent(&self) -> f64 {
    self.actual_descent
  }

  #[getter]
  fn font_bounding_box_ascent(&self) -> f64 {
    self.font_ascent
  }

  #[getter]
  fn font_bounding_box_descent(&self) -> f64 {
    self.font_descent
  }

  #[getter]
  fn actual_bounding_box_left(&self) -> f64 {
    self.actual_left
  }

  #[getter]
  fn actual_bounding_box_right(&self) -> f64 {
    self.actual_right
  }

  #[getter]
  fn em_height_ascent(&self) -> f64 {
    self.em_ascent
  }

  #[getter]
  fn em_height_descent(&self) -> f64 {
    self.em_descent
  }

  #[getter]
  fn hanging_baseline(&self) -> f64 {
    self.hanging
  }

  #[getter]
  fn alphabetic_baseline(&self) -> f64 {
    self.alphabetic
  }

  #[getter]
  fn ideographic_baseline(&self) -> f64 {
    self.ideographic
  }
}

pub fn number(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<f64, OpError> {
  <f64 as yuzuko_core::op::FromV8>::from_v8(scope, value)
}

pub fn radii_of(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<[f64; 8], OpError> {
  let values = if let Ok(array) = v8::Local::<v8::Array>::try_from(value) {
    let length = array.length();
    if length == 0 || length > 4 {
      return Err(OpError::range(
        "roundRect radii must contain one to four values",
      ));
    }
    let mut values = Vec::new();
    for index in 0..length {
      let item = array
        .get_index(scope, index)
        .ok_or_else(|| OpError::type_("roundRect radius is invalid"))?;
      values.push(radius_pair(scope, item)?);
    }
    values
  } else {
    vec![radius_pair(scope, value)?]
  };
  for (x, y) in &values {
    if (x.is_finite() && *x < 0.0) || (y.is_finite() && *y < 0.0) {
      return Err(OpError::range("The radius is negative"));
    }
  }
  Ok(expand_radii(&values))
}

fn radius_pair(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<(f64, f64), OpError> {
  if value.is_object() && !value.is_array() {
    let object = v8::Local::<v8::Object>::try_from(value)
      .map_err(|_| OpError::type_("roundRect radius is invalid"))?;
    let x = member(scope, object, "x")?.unwrap_or(0.0);
    let y = member(scope, object, "y")?.unwrap_or(0.0);
    return Ok((x, y));
  }
  let radius = number(scope, value)?;
  Ok((radius, radius))
}

fn member(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
) -> Result<Option<f64>, OpError> {
  let key = v8::String::new(scope, name).unwrap();
  let value = object
    .get(scope, key.into())
    .ok_or_else(|| OpError::type_("failed to read a radius"))?;
  if value.is_undefined() {
    Ok(None)
  } else {
    Ok(Some(number(scope, value)?))
  }
}

fn expand_radii(values: &[(f64, f64)]) -> [f64; 8] {
  let pair = |index: usize| values.get(index).copied().unwrap_or((0.0, 0.0));
  let (tl, tr, br, bl) = match values.len() {
    0 => ((0.0, 0.0), (0.0, 0.0), (0.0, 0.0), (0.0, 0.0)),
    1 => (pair(0), pair(0), pair(0), pair(0)),
    2 => (pair(0), pair(1), pair(0), pair(1)),
    3 => (pair(0), pair(1), pair(2), pair(1)),
    _ => (pair(0), pair(1), pair(2), pair(3)),
  };
  [tl.0, tl.1, tr.0, tr.1, br.0, br.1, bl.0, bl.1]
}

#[cfg(test)]
mod tests {
  use super::TextMetrics;
  use crate::paint::record::Align;
  use crate::paint::record::Baseline;
  use crate::paint::record::FontDesc;
  use crate::text::shape;

  fn metrics(text: &str, align: Align, baseline: Baseline) -> TextMetrics {
    let font = FontDesc {
      size: 80.0,
      ..FontDesc::default()
    };
    let shaped = shape(text, &font, false, "inherit");
    TextMetrics::from_shaped(&shaped, align, baseline, false)
  }

  #[test]
  fn actual_bounds_follow_ink_and_alignment() {
    let letter = metrics("A", Align::Left, Baseline::Alphabetic);
    let space = metrics(" ", Align::Left, Baseline::Alphabetic);
    let descender = metrics("g", Align::Left, Baseline::Alphabetic);
    assert_eq!(space.actual_ascent, 0.0);
    assert_eq!(space.actual_descent, 0.0);
    assert!(letter.actual_ascent > 0.0);
    assert!(
      letter.actual_left != 0.0 || (letter.actual_right - letter.width).abs() > 0.5
    );
    assert!(descender.actual_descent > letter.actual_descent);
    let centered = metrics("A", Align::Center, Baseline::Alphabetic);
    assert!(centered.actual_left > letter.actual_left + letter.width * 0.3);
    assert_eq!(letter.alphabetic, 0.0);
    assert!(letter.hanging < 0.0);
    assert!(letter.ideographic > 0.0);
    let top = metrics("A", Align::Left, Baseline::Top);
    assert!(top.alphabetic > 0.0);
    assert!(top.font_ascent < letter.font_ascent);
    let empty = metrics("", Align::Left, Baseline::Alphabetic);
    assert_eq!(empty.actual_left, 0.0);
    assert_eq!(empty.actual_right, 0.0);
    assert_eq!(empty.actual_ascent, 0.0);
    assert_eq!(empty.actual_descent, 0.0);
  }
}
