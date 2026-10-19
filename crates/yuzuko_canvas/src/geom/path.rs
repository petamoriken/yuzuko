//! Path construction shared by the current path and `Path2D`.

#![allow(clippy::too_many_arguments)]

use kurbo::Affine;
use kurbo::Arc;
use kurbo::BezPath;
use kurbo::ParamCurve;
use kurbo::ParamCurveNearest;
use kurbo::Point;
use kurbo::Shape;
use kurbo::Vec2;
use yuzuko_core::op::OpError;

const TAU: f64 = std::f64::consts::TAU;

pub fn move_to(path: &mut BezPath, x: f64, y: f64) {
  if x.is_finite() && y.is_finite() {
    path.move_to(Point::new(x, y));
  }
}

pub fn line_to(path: &mut BezPath, x: f64, y: f64) {
  if path.elements().is_empty() {
    move_to(path, x, y);
    return;
  }
  if x.is_finite() && y.is_finite() {
    path.line_to(Point::new(x, y));
  }
}

pub fn quad_to(path: &mut BezPath, cpx: f64, cpy: f64, x: f64, y: f64) {
  if ![cpx, cpy, x, y].iter().all(|value| value.is_finite()) {
    return;
  }
  if path.elements().is_empty() {
    path.move_to(Point::new(cpx, cpy));
  }
  path.quad_to(Point::new(cpx, cpy), Point::new(x, y));
}

pub fn curve_to(
  path: &mut BezPath,
  cp1x: f64,
  cp1y: f64,
  cp2x: f64,
  cp2y: f64,
  x: f64,
  y: f64,
) {
  if ![cp1x, cp1y, cp2x, cp2y, x, y]
    .iter()
    .all(|value| value.is_finite())
  {
    return;
  }
  if path.elements().is_empty() {
    path.move_to(Point::new(cp1x, cp1y));
  }
  path.curve_to(
    Point::new(cp1x, cp1y),
    Point::new(cp2x, cp2y),
    Point::new(x, y),
  );
}

pub fn close(path: &mut BezPath) {
  if !path.elements().is_empty() {
    path.close_path();
  }
}

pub fn add_rect(path: &mut BezPath, x: f64, y: f64, w: f64, h: f64) {
  if ![x, y, w, h].iter().all(|value| value.is_finite()) {
    return;
  }
  path.move_to(Point::new(x, y));
  path.line_to(Point::new(x + w, y));
  path.line_to(Point::new(x + w, y + h));
  path.line_to(Point::new(x, y + h));
  path.close_path();
}

pub fn add_round_rect(
  path: &mut BezPath,
  x: f64,
  y: f64,
  w: f64,
  h: f64,
  radii: [f64; 8],
) {
  if ![x, y, w, h].iter().all(|value| value.is_finite()) {
    return;
  }
  // A negative edge flips that axis. The radius stays on the corner that was
  // named before the flip, which is the opposite corner afterwards.
  let mut radii = clamp_radii(w.abs(), h.abs(), radii);
  if w < 0.0 {
    radii.swap(0, 2);
    radii.swap(1, 3);
    radii.swap(4, 6);
    radii.swap(5, 7);
  }
  if h < 0.0 {
    radii.swap(0, 6);
    radii.swap(1, 7);
    radii.swap(2, 4);
    radii.swap(3, 5);
  }
  let (x, y, w, h) = normalize_rect(x, y, w, h);
  let [tlx, tly, trx, try_, brx, bry, blx, bly] = radii;
  path.move_to(Point::new(x + tlx, y));
  path.line_to(Point::new(x + w - trx, y));
  corner(path, x + w - trx, y + try_, trx, try_, -TAU / 4.0);
  path.line_to(Point::new(x + w, y + h - bry));
  corner(path, x + w - brx, y + h - bry, brx, bry, 0.0);
  path.line_to(Point::new(x + blx, y + h));
  corner(path, x + blx, y + h - bly, blx, bly, TAU / 4.0);
  path.line_to(Point::new(x, y + tly));
  corner(path, x + tlx, y + tly, tlx, tly, TAU / 2.0);
  path.close_path();
}

fn corner(path: &mut BezPath, cx: f64, cy: f64, rx: f64, ry: f64, start: f64) {
  if rx == 0.0 || ry == 0.0 {
    return;
  }
  let arc =
    Arc::new(Point::new(cx, cy), Vec2::new(rx, ry), start, TAU / 4.0, 0.0);
  path.line_to(arc.eval(0.0));
  path.extend(arc.append_iter(0.1));
}

pub fn add_ellipse(
  path: &mut BezPath,
  x: f64,
  y: f64,
  rx: f64,
  ry: f64,
  rotation: f64,
  start: f64,
  end: f64,
  counterclockwise: bool,
) -> Result<(), OpError> {
  if ![x, y, rx, ry, rotation, start, end]
    .iter()
    .all(|value| value.is_finite())
  {
    return Ok(());
  }
  if rx < 0.0 || ry < 0.0 {
    return Err(OpError::dom("IndexSizeError", "The radius is negative"));
  }
  if rx == 0.0 || ry == 0.0 {
    move_to(path, x, y);
    return Ok(());
  }
  let sweep = sweep_angle(start, end, counterclockwise);
  let arc =
    Arc::new(Point::new(x, y), Vec2::new(rx, ry), start, sweep, rotation);
  // `append_iter` emits only curves. The path must already be at the start.
  if path.elements().is_empty() {
    path.move_to(arc.eval(0.0));
  } else {
    path.line_to(arc.eval(0.0));
  }
  path.extend(arc.append_iter(0.1));
  Ok(())
}

pub fn arc_to(
  path: &mut BezPath,
  x1: f64,
  y1: f64,
  x2: f64,
  y2: f64,
  radius: f64,
) -> Result<(), OpError> {
  if ![x1, y1, x2, y2, radius]
    .iter()
    .all(|value| value.is_finite())
  {
    return Ok(());
  }
  if radius < 0.0 {
    return Err(OpError::dom("IndexSizeError", "The radius is negative"));
  }
  let Some(current) = current_point(path) else {
    move_to(path, x1, y1);
    return Ok(());
  };
  if radius == 0.0
    || (current.x == x1 && current.y == y1)
    || (x1 == x2 && y1 == y2)
  {
    line_to(path, x1, y1);
    return Ok(());
  }
  let p0 = current;
  let p1 = Point::new(x1, y1);
  let p2 = Point::new(x2, y2);
  let v1 = (p0 - p1).normalize();
  let v2 = (p2 - p1).normalize();
  if !v1.x.is_finite() || !v2.x.is_finite() {
    line_to(path, x1, y1);
    return Ok(());
  }
  let dot = (v1.dot(v2)).clamp(-1.0, 1.0);
  if (dot + 1.0).abs() < 1e-6 || (dot - 1.0).abs() < 1e-6 {
    line_to(path, x1, y1);
    return Ok(());
  }
  let angle = dot.acos();
  let distance = radius / (angle / 2.0).tan();
  let start = p1 + v1 * distance;
  let end = p1 + v2 * distance;
  let bisector = (v1 + v2).normalize();
  let center_distance = radius / (angle / 2.0).sin();
  let center = p1 + bisector * center_distance;
  line_to(path, start.x, start.y);
  let start_angle = (start - center).atan2();
  let end_angle = (end - center).atan2();
  let cross = v1.x * v2.y - v1.y * v2.x;
  let sweep = sweep_angle(start_angle, end_angle, cross > 0.0);
  let arc =
    Arc::new(center, Vec2::new(radius, radius), start_angle, sweep, 0.0);
  path.extend(arc.append_iter(0.1));
  Ok(())
}

pub fn inverse(transform: Affine) -> Option<Affine> {
  let determinant = transform.determinant();
  if determinant.is_finite() && determinant != 0.0 {
    Some(transform.inverse())
  } else {
    None
  }
}

/// Append geometry built in user space. `connect` turns a leading move into a
/// line when the destination path already has a point, which is what `arc`
/// does and what `rect` does not.
pub fn append_user<T>(
  path: &mut BezPath,
  transform: Affine,
  connect: bool,
  build: impl FnOnce(&mut BezPath) -> T,
) -> T {
  let mut segment = BezPath::new();
  let result = build(&mut segment);
  if segment.elements().is_empty() {
    return result;
  }
  if transform != Affine::IDENTITY {
    segment.apply_affine(transform);
  }
  let join = connect && !path.elements().is_empty();
  for (index, element) in segment.elements().iter().copied().enumerate() {
    if index == 0
      && join
      && let kurbo::PathEl::MoveTo(point) = element
    {
      path.line_to(point);
    } else {
      path.push(element);
    }
  }
  result
}

/// Continue the current path. The closure sees the last point in user space.
pub fn append_continuing(
  path: &mut BezPath,
  transform: Affine,
  build: impl FnOnce(&mut BezPath),
) {
  if transform == Affine::IDENTITY {
    build(path);
    return;
  }
  if path.elements().is_empty() {
    append_user(path, transform, false, |segment| build(segment));
    return;
  }
  let Some(inverse) = inverse(transform) else {
    return;
  };
  let Some(current) = current_point(path) else {
    return;
  };
  let mut segment = BezPath::new();
  segment.move_to(inverse * current);
  build(&mut segment);
  segment.apply_affine(transform);
  for element in segment.elements().iter().copied().skip(1) {
    path.push(element);
  }
}

/// `arcTo` reads the last point in user space, then the new segment is stored
/// in device space.
pub fn append_arc_to(
  path: &mut BezPath,
  transform: Affine,
  x1: f64,
  y1: f64,
  x2: f64,
  y2: f64,
  radius: f64,
) -> Result<(), OpError> {
  if transform == Affine::IDENTITY {
    return arc_to(path, x1, y1, x2, y2, radius);
  }
  if path.elements().is_empty() {
    return append_user(path, transform, false, |segment| {
      arc_to(segment, x1, y1, x2, y2, radius)
    });
  }
  let Some(inverse) = inverse(transform) else {
    let point = transform * Point::new(x1, y1);
    line_to(path, point.x, point.y);
    return Ok(());
  };
  let Some(current) = current_point(path) else {
    return Ok(());
  };
  let user = inverse * current;
  let mut segment = BezPath::new();
  segment.move_to(user);
  arc_to(&mut segment, x1, y1, x2, y2, radius)?;
  segment.apply_affine(transform);
  for element in segment.elements().iter().copied().skip(1) {
    path.push(element);
  }
  Ok(())
}

/// Sweep from `start` to `end`. Equal angles draw nothing. A span of at
/// least one full turn is the circumference. Any further turns wrap.
fn sweep_angle(start: f64, end: f64, counterclockwise: bool) -> f64 {
  let delta = end - start;
  if !delta.is_finite() || delta == 0.0 {
    return 0.0;
  }
  let full = if counterclockwise { -TAU } else { TAU };
  if counterclockwise {
    if delta <= -TAU {
      return -TAU;
    }
  } else if delta >= TAU {
    return TAU;
  }
  let turns = (delta / TAU).trunc();
  let mut wrapped = delta - turns * TAU;
  if counterclockwise && wrapped > 0.0 {
    wrapped -= TAU;
  } else if !counterclockwise && wrapped < 0.0 {
    wrapped += TAU;
  }
  if wrapped.abs() < 1e-10 { full } else { wrapped }
}

pub fn current_point(path: &BezPath) -> Option<Point> {
  let mut point = None;
  let mut start = None;
  for element in path.elements() {
    match element {
      kurbo::PathEl::MoveTo(p) => {
        point = Some(*p);
        start = Some(*p);
      }
      kurbo::PathEl::LineTo(p)
      | kurbo::PathEl::QuadTo(_, p)
      | kurbo::PathEl::CurveTo(_, _, p) => point = Some(*p),
      kurbo::PathEl::ClosePath => point = start,
    }
  }
  point
}

pub fn hit(
  path: &BezPath,
  transform: Affine,
  x: f64,
  y: f64,
  even_odd: bool,
) -> bool {
  if !x.is_finite() || !y.is_finite() {
    return false;
  }
  let det = transform.determinant();
  if det == 0.0 || det.is_nan() {
    return false;
  }
  let point = transform.inverse() * Point::new(x, y);
  contains(path, point.x, point.y, even_odd)
}

/// Canvas coordinate test. Open subpaths are closed for the test, and a point
/// on the outline counts as inside.
pub fn contains(path: &BezPath, x: f64, y: f64, even_odd: bool) -> bool {
  if !x.is_finite() || !y.is_finite() {
    return false;
  }
  let point = Point::new(x, y);
  let closed = close_open_subpaths(path);
  if on_path(&closed, point) {
    return true;
  }
  let winding = closed.winding(point);
  if even_odd {
    winding.abs() % 2 == 1
  } else {
    winding != 0
  }
}

fn close_open_subpaths(path: &BezPath) -> BezPath {
  let mut out = BezPath::new();
  let mut open = false;
  for element in path.elements() {
    if open && matches!(element, kurbo::PathEl::MoveTo(_)) {
      out.close_path();
    }
    out.push(*element);
    open = match element {
      kurbo::PathEl::ClosePath => false,
      kurbo::PathEl::MoveTo(_)
      | kurbo::PathEl::LineTo(_)
      | kurbo::PathEl::QuadTo(_, _)
      | kurbo::PathEl::CurveTo(_, _, _) => true,
    };
  }
  if open {
    out.close_path();
  }
  out
}

fn on_path(path: &BezPath, point: Point) -> bool {
  path
    .segments()
    .any(|segment| segment.nearest(point, 1.0e-6).distance_sq <= 1.0e-8)
}

pub fn non_finite(path: &BezPath) -> bool {
  path.elements().iter().any(|element| match element {
    kurbo::PathEl::MoveTo(point) | kurbo::PathEl::LineTo(point) => {
      !point.x.is_finite() || !point.y.is_finite()
    }
    kurbo::PathEl::QuadTo(control, point) => {
      ![control.x, control.y, point.x, point.y]
        .iter()
        .all(|value| value.is_finite())
    }
    kurbo::PathEl::CurveTo(control1, control2, point) => ![
      control1.x, control1.y, control2.x, control2.y, point.x, point.y,
    ]
    .iter()
    .all(|value| value.is_finite()),
    kurbo::PathEl::ClosePath => false,
  })
}

pub fn hit_stroke(
  path: &BezPath,
  transform: Affine,
  stroke: &kurbo::Stroke,
  x: f64,
  y: f64,
) -> bool {
  if !transform.determinant().is_finite() || transform.determinant() == 0.0 {
    return false;
  }
  let inverse = transform.inverse();
  let point = inverse * Point::new(x, y);
  let outline = kurbo::stroke(
    path.elements().iter().copied(),
    stroke,
    &kurbo::StrokeOpts::default(),
    0.25,
  );
  outline.winding(point) != 0
}

fn normalize_rect(x: f64, y: f64, w: f64, h: f64) -> (f64, f64, f64, f64) {
  let (x, w) = if w < 0.0 { (x + w, -w) } else { (x, w) };
  let (y, h) = if h < 0.0 { (y + h, -h) } else { (y, h) };
  (x, y, w, h)
}

fn clamp_radii(w: f64, h: f64, radii: [f64; 8]) -> [f64; 8] {
  let mut radii = radii.map(|radius| radius.max(0.0));
  let scale_x = {
    let sum_top = radii[0] + radii[2];
    let sum_bottom = radii[4] + radii[6];
    let limit = sum_top.max(sum_bottom);
    if limit > w && limit > 0.0 {
      w / limit
    } else {
      1.0
    }
  };
  let scale_y = {
    let sum_left = radii[1] + radii[7];
    let sum_right = radii[3] + radii[5];
    let limit = sum_left.max(sum_right);
    if limit > h && limit > 0.0 {
      h / limit
    } else {
      1.0
    }
  };
  let scale = scale_x.min(scale_y);
  for radius in &mut radii {
    *radius *= scale;
  }
  radii
}

pub fn rect_path(x: f64, y: f64, w: f64, h: f64) -> BezPath {
  let mut path = BezPath::new();
  add_rect(&mut path, x, y, w, h);
  path
}
