//! `Path2D`, gradients, patterns, and `TextMetrics`.

#![allow(clippy::too_many_arguments)]

use std::cell::RefCell;

use kurbo::BezPath;
use yuzuko_core::op::OpError;
use yuzuko_macros::op;

use yuzuko_core::op::FromV8;
use yuzuko_core::op::Js;
use yuzuko_core::op::New;

use crate::geom::geometry::DomRectReadOnly;
use crate::geom::path;
use crate::paint::color::parse_color;
use crate::paint::record::Align;
use crate::paint::record::Baseline;
use crate::paint::record::LiveGradient;
use crate::paint::record::LivePattern;
use crate::paint::record::{self};
use crate::text;
use crate::text::ShapedCluster;
use crate::text::ShapedGlyph;
use crate::text::ShapedText;

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
      stops: Vec::new(),
    });
    Ok(Self { id })
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
    if !offset.is_finite() || !(0.0..=1.0).contains(&offset) {
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
  ascent: f64,
  descent: f64,
  align: Align,
  baseline: Baseline,
  rtl: bool,
  shaped: ShapedText,
}

unsafe impl v8::cppgc::GarbageCollected for TextMetrics {
  fn trace(&self, _visitor: &mut v8::cppgc::Visitor) {}

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"TextMetrics"
  }
}

impl TextMetrics {
  pub fn from_shape(
    shaped: ShapedText,
    align: Align,
    baseline: Baseline,
    rtl: bool,
  ) -> Self {
    Self {
      width: f64::from(shaped.width),
      ascent: f64::from(shaped.ascent),
      descent: f64::from(shaped.descent),
      align,
      baseline,
      rtl,
      shaped,
    }
  }

  fn cluster_offset(
    &self,
    cluster: &ShapedCluster,
    align: Align,
    baseline: Baseline,
  ) -> (f64, f64) {
    let shift = self.align.offset(self.rtl, self.width);
    let x = text::cluster_anchor_x(
      align,
      cluster.rtl,
      f64::from(cluster.left),
      f64::from(cluster.advance),
    ) + shift;
    let y = self.baseline.offset(self.ascent, self.descent)
      - baseline.offset(self.ascent, self.descent);
    (x, y)
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
    self.ascent
  }

  #[getter]
  fn actual_bounding_box_descent(&self) -> f64 {
    self.descent
  }

  #[getter]
  fn font_bounding_box_ascent(&self) -> f64 {
    self.ascent
  }

  #[getter]
  fn font_bounding_box_descent(&self) -> f64 {
    self.descent
  }

  #[getter]
  fn actual_bounding_box_left(&self) -> f64 {
    0.0
  }

  #[getter]
  fn actual_bounding_box_right(&self) -> f64 {
    self.width
  }

  #[getter]
  fn em_height_ascent(&self) -> f64 {
    self.ascent
  }

  #[getter]
  fn em_height_descent(&self) -> f64 {
    self.descent
  }

  #[getter]
  fn hanging_baseline(&self) -> f64 {
    self.ascent * 0.2
  }

  #[getter]
  fn alphabetic_baseline(&self) -> f64 {
    0.0
  }

  #[getter]
  fn ideographic_baseline(&self) -> f64 {
    -self.descent
  }

  fn get_selection_rects(
    &self,
    scope: &mut v8::PinScope,
    start: u32,
    end: u32,
  ) -> Js {
    let rects = self.shaped.selection_rects(
      start,
      end,
      self.align,
      self.baseline,
      self.rtl,
    );
    host_array(
      scope,
      rects.into_iter().map(|rect| {
        DomRectReadOnly::at(rect.x, rect.y, rect.width, rect.height)
      }),
    )
  }

  fn get_actual_bounding_box(
    &self,
    start: u32,
    end: u32,
  ) -> New<DomRectReadOnly> {
    let rect =
      self
        .shaped
        .actual_box(start, end, self.align, self.baseline, self.rtl);
    New(DomRectReadOnly::at(rect.x, rect.y, rect.width, rect.height))
  }

  fn get_text_clusters(
    &self,
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<Js, OpError> {
    let (start, end, options) =
      cluster_query(scope, args, self.shaped.utf16_len)?;
    let clusters: Vec<TextCluster> = self
      .shaped
      .clusters
      .iter()
      .filter(|cluster| cluster.end > start && cluster.start < end)
      .map(|cluster| {
        let align = options.align.unwrap_or(self.align);
        let baseline = options.baseline.unwrap_or(self.baseline);
        let (x, y) = self.cluster_offset(cluster, align, baseline);
        TextCluster {
          x: x + options.x.unwrap_or(0.0),
          y: y + options.y.unwrap_or(0.0),
          start: cluster.start,
          end: cluster.end,
          align,
          baseline,
          rtl: cluster.rtl,
          left: cluster.left,
          advance: cluster.advance,
          ascent: self.shaped.ascent,
          descent: self.shaped.descent,
          glyphs: cluster.glyphs.clone(),
        }
      })
      .collect();
    Ok(host_array(scope, clusters.into_iter()))
  }

  fn get_index_from_offset(&self, offset: f64) -> u32 {
    if !offset.is_finite() {
      return 0;
    }
    self.shaped.index_at(offset, self.align, self.rtl)
  }
}

pub struct ClusterPaintOptions {
  pub align: Option<Align>,
  pub baseline: Option<Baseline>,
  pub x: Option<f64>,
  pub y: Option<f64>,
}

fn cluster_query(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
  len: u32,
) -> Result<(u32, u32, ClusterPaintOptions), OpError> {
  match args.length() {
    0 => Ok((0, len, ClusterPaintOptions::default())),
    1 => {
      let options = cluster_paint_options(scope, args.get(0))?;
      Ok((0, len, options))
    }
    2 => {
      if args.get(0).is_object() {
        return Err(OpError::type_("TextCluster options must be a dictionary"));
      }
      let start: u32 = FromV8::from_v8(scope, args.get(0))?;
      let end: u32 = FromV8::from_v8(scope, args.get(1))?;
      Ok((start, end, ClusterPaintOptions::default()))
    }
    3 => {
      let start: u32 = FromV8::from_v8(scope, args.get(0))?;
      let end: u32 = FromV8::from_v8(scope, args.get(1))?;
      Ok((start, end, cluster_paint_options(scope, args.get(2))?))
    }
    _ => Err(OpError::type_(
      "getTextClusters received too many arguments",
    )),
  }
}

impl Default for ClusterPaintOptions {
  fn default() -> Self {
    Self {
      align: None,
      baseline: None,
      x: None,
      y: None,
    }
  }
}

pub fn cluster_paint_options(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<ClusterPaintOptions, OpError> {
  if value.is_null_or_undefined() {
    return Ok(ClusterPaintOptions::default());
  }
  let object = v8::Local::<v8::Object>::try_from(value)
    .map_err(|_| OpError::type_("TextCluster options must be a dictionary"))?;
  let align = enum_member(scope, object, "align")?;
  let baseline = enum_member(scope, object, "baseline")?;
  let align = match align.as_deref() {
    None => None,
    Some(value) => Some(Align::parse(value).ok_or_else(|| {
      OpError::type_("The text align is not a valid enum value")
    })?),
  };
  let baseline = match baseline.as_deref() {
    None => None,
    Some(value) => Some(Baseline::parse(value).ok_or_else(|| {
      OpError::type_("The text baseline is not a valid enum value")
    })?),
  };
  Ok(ClusterPaintOptions {
    align,
    baseline,
    x: finite_member(scope, object, "x")?,
    y: finite_member(scope, object, "y")?,
  })
}

fn enum_member(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
) -> Result<Option<String>, OpError> {
  let key = v8::String::new(scope, name).unwrap();
  let value = object
    .get(scope, key.into())
    .ok_or_else(|| OpError::type_("failed to read a dictionary member"))?;
  if value.is_null_or_undefined() {
    return Ok(None);
  }
  if !value.is_string() {
    return Err(OpError::type_(
      "The provided value is not a valid enum value",
    ));
  }
  Ok(Some(value.to_rust_string_lossy(scope)))
}

fn finite_member(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
) -> Result<Option<f64>, OpError> {
  let key = v8::String::new(scope, name).unwrap();
  let value = object
    .get(scope, key.into())
    .ok_or_else(|| OpError::type_("failed to read a dictionary member"))?;
  if value.is_null_or_undefined() {
    return Ok(None);
  }
  let number = number(scope, value)?;
  if number.is_finite() {
    Ok(Some(number))
  } else {
    Ok(Some(0.0))
  }
}

fn host_array<T>(
  scope: &mut v8::PinScope,
  items: impl ExactSizeIterator<Item = T>,
) -> Js
where
  T: v8::cppgc::GarbageCollected + 'static,
{
  let len = items.len();
  let array = v8::Array::new(scope, len as i32);
  for (index, item) in items.enumerate() {
    let object = unsafe { yuzuko_core::op::wrap_instance(scope, item) };
    array.set_index(scope, index as u32, object.into());
  }
  let value: v8::Local<v8::Value> = array.into();
  Js(v8::Global::new(scope, value))
}

pub struct TextCluster {
  x: f64,
  y: f64,
  start: u32,
  end: u32,
  align: Align,
  baseline: Baseline,
  rtl: bool,
  left: f32,
  advance: f32,
  ascent: f32,
  descent: f32,
  glyphs: Vec<ShapedGlyph>,
}

unsafe impl v8::cppgc::GarbageCollected for TextCluster {
  fn trace(&self, _visitor: &mut v8::cppgc::Visitor) {}

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"TextCluster"
  }
}

impl TextCluster {
  pub fn anchor(&self) -> (f64, f64) {
    (self.x, self.y)
  }

  pub fn paint_style(&self) -> (Align, Baseline) {
    (self.align, self.baseline)
  }

  pub fn placed(&self, align: Align, baseline: Baseline) -> Vec<ShapedGlyph> {
    ShapedCluster {
      start: self.start,
      end: self.end,
      left: self.left,
      advance: self.advance,
      rtl: self.rtl,
      glyphs: self.glyphs.clone(),
      ink: None,
    }
    .placed(align, baseline, self.ascent, self.descent)
  }
}

pub fn text_cluster(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<yuzuko_core::op::HostRef<TextCluster>, OpError> {
  let object = v8::Local::<v8::Object>::try_from(value)
    .map_err(|_| OpError::type_("The text cluster is not a TextCluster"))?;
  unsafe { yuzuko_core::op::host_ref::<TextCluster>(scope, object) }
    .ok_or_else(|| OpError::type_("The text cluster is not a TextCluster"))
}

#[op]
impl TextCluster {
  #[getter]
  fn x(&self) -> f64 {
    self.x
  }
  #[getter]
  fn y(&self) -> f64 {
    self.y
  }
  #[getter]
  fn start(&self) -> u32 {
    self.start
  }
  #[getter]
  fn end(&self) -> u32 {
    self.end
  }
  #[getter]
  fn align(&self) -> String {
    self.align.name().to_string()
  }
  #[getter]
  fn baseline(&self) -> String {
    self.baseline.name().to_string()
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

pub fn unsupported() -> OpError {
  OpError::dom(
    "NotSupportedError",
    "This Canvas 2D feature is not in level A",
  )
}
