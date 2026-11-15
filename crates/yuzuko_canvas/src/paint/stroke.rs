//! Outer-only joins for a polyline whose stroke is wider than the corner.
//!
//! kurbo connects both offset sides. On a short corner the inner chord
//! crosses the outside of the stroke and the nonzero fill paints that
//! bowtie. A canvas join is only the triangle on the side furthest from
//! the other segment, plus a round arc or a miter when the limit allows
//! it. Dashes and curves stay on kurbo.

use std::f64::consts::PI;

use kurbo::{Arc, BezPath, Cap, Join, PathEl, Point, Shape, Stroke, Vec2};

const MIN_LEN: f64 = 1e-12;
const PARALLEL: f64 = 1e-9;
const EDGE: f64 = 1e-4;
const ARC_TOLERANCE: f64 = 0.25;

struct Seg {
  a: Point,
  b: Point,
  tangent: Vec2,
  len: f64,
}

struct Sub {
  segs: Vec<Seg>,
  closed: bool,
}

pub(crate) fn crossing_outline(
  path: &BezPath,
  stroke: &Stroke,
) -> Option<BezPath> {
  if !stroke.dash_pattern.is_empty()
    || !stroke.width.is_finite()
    || stroke.width <= 0.0
  {
    return None;
  }
  let subs = subpaths(path)?;
  let half = stroke.width * 0.5;
  if !subs.iter().any(|sub| sub_inverted(sub, half)) {
    return None;
  }
  Some(outline(&subs, stroke, half))
}

fn subpaths(path: &BezPath) -> Option<Vec<Sub>> {
  let mut subs = Vec::new();
  let mut segs = Vec::new();
  let mut start: Option<Point> = None;
  let mut current: Option<Point> = None;
  for element in path.elements() {
    match *element {
      PathEl::MoveTo(point) => {
        if start.take().is_some() {
          subs.push(Sub {
            segs: std::mem::take(&mut segs),
            closed: false,
          });
        }
        start = Some(point);
        current = Some(point);
      }
      PathEl::LineTo(point) => {
        let Some(cursor) = current else {
          start = Some(point);
          current = Some(point);
          continue;
        };
        if push_seg(&mut segs, cursor, point) {
          current = Some(point);
        }
      }
      PathEl::QuadTo(_, _) | PathEl::CurveTo(_, _, _) => return None,
      PathEl::ClosePath => {
        let Some(start_pt) = start.take() else {
          continue;
        };
        if let Some(cursor) = current.take() {
          push_seg(&mut segs, cursor, start_pt);
        }
        subs.push(Sub {
          segs: std::mem::take(&mut segs),
          closed: true,
        });
      }
    }
  }
  if start.is_some() {
    subs.push(Sub {
      segs,
      closed: false,
    });
  }
  Some(subs)
}

fn push_seg(segs: &mut Vec<Seg>, a: Point, b: Point) -> bool {
  let tangent = b - a;
  let len = tangent.hypot();
  if len < MIN_LEN {
    return false;
  }
  segs.push(Seg { a, b, tangent, len });
  true
}

fn sub_inverted(sub: &Sub, half: f64) -> bool {
  let count = sub.segs.len();
  if count < 2 {
    return false;
  }
  let joins = if sub.closed { count } else { count - 1 };
  (0..joins).any(|index| {
    inverted(&sub.segs[index], &sub.segs[(index + 1) % count], half)
  })
}

fn inverted(incoming: &Seg, outgoing: &Seg, half: f64) -> bool {
  let Some((first, second)) = inner_corners(incoming, outgoing, half) else {
    return false;
  };
  let mid = first.midpoint(second);
  outside_butt(mid, incoming, half) && outside_butt(mid, outgoing, half)
}

fn inner_corners(
  incoming: &Seg,
  outgoing: &Seg,
  half: f64,
) -> Option<(Point, Point)> {
  let cross = incoming.tangent.cross(outgoing.tangent);
  let scale = incoming.len * outgoing.len;
  if cross.abs() <= PARALLEL * scale.max(1.0) {
    return None;
  }
  let left0 = left_normal(incoming, half);
  let left1 = left_normal(outgoing, half);
  let join = incoming.b;
  if cross > 0.0 {
    Some((join + left0, join + left1))
  } else {
    Some((join - left0, join - left1))
  }
}

fn outside_butt(point: Point, seg: &Seg, half: f64) -> bool {
  let rel = point - seg.a;
  let t = rel.dot(seg.tangent) / seg.tangent.hypot2();
  if t < -EDGE || t > 1.0 + EDGE {
    return true;
  }
  rel.cross(seg.tangent).abs() / seg.len > half + EDGE
}

fn left_normal(seg: &Seg, half: f64) -> Vec2 {
  Vec2::new(-seg.tangent.y, seg.tangent.x) * (half / seg.len)
}

fn outline(subs: &[Sub], stroke: &Stroke, half: f64) -> BezPath {
  let mut out = BezPath::new();
  for sub in subs {
    for seg in &sub.segs {
      push_segment(&mut out, seg, half);
    }
    let count = sub.segs.len();
    let joins = if sub.closed {
      count
    } else {
      count.saturating_sub(1)
    };
    for index in 0..joins {
      push_join(
        &mut out,
        &sub.segs[index],
        &sub.segs[(index + 1) % count],
        stroke,
        half,
      );
    }
    if !sub.closed
      && let Some(first) = sub.segs.first()
    {
      push_cap(&mut out, first, stroke.start_cap, half, true);
      if let Some(last) = sub.segs.last() {
        push_cap(&mut out, last, stroke.end_cap, half, false);
      }
    }
  }
  out
}

fn push_segment(out: &mut BezPath, seg: &Seg, half: f64) {
  let left = left_normal(seg, half);
  push_poly(
    out,
    &[seg.a - left, seg.b - left, seg.b + left, seg.a + left],
  );
}

fn push_join(
  out: &mut BezPath,
  incoming: &Seg,
  outgoing: &Seg,
  stroke: &Stroke,
  half: f64,
) {
  let Some((start, end)) = outer_corners(incoming, outgoing, half) else {
    return;
  };
  let corner = Corner {
    join: incoming.b,
    start,
    end,
    incoming,
    outgoing,
    cross: incoming.tangent.cross(outgoing.tangent),
  };
  if corner.cross > 0.0 {
    push_poly(out, &[corner.join, corner.start, corner.end]);
  } else {
    push_poly(out, &[corner.join, corner.end, corner.start]);
  }
  match stroke.join {
    Join::Bevel => {}
    Join::Miter => push_miter(out, &corner, stroke.miter_limit, half),
    Join::Round => push_round(out, &corner, half),
  }
}

struct Corner<'a> {
  join: Point,
  start: Point,
  end: Point,
  incoming: &'a Seg,
  outgoing: &'a Seg,
  cross: f64,
}

fn outer_corners(
  incoming: &Seg,
  outgoing: &Seg,
  half: f64,
) -> Option<(Point, Point)> {
  let cross = incoming.tangent.cross(outgoing.tangent);
  let scale = incoming.len * outgoing.len;
  if cross.abs() <= PARALLEL * scale.max(1.0) {
    return None;
  }
  let left0 = left_normal(incoming, half);
  let left1 = left_normal(outgoing, half);
  let join = incoming.b;
  if cross > 0.0 {
    Some((join - left0, join - left1))
  } else {
    Some((join + left0, join + left1))
  }
}

fn push_miter(out: &mut BezPath, corner: &Corner<'_>, limit: f64, half: f64) {
  let Some(miter) = miter_point(corner) else {
    return;
  };
  if (miter - corner.join).hypot() > limit * half {
    return;
  }
  if corner.cross > 0.0 {
    push_poly(out, &[corner.start, miter, corner.end]);
  } else {
    push_poly(out, &[corner.start, corner.end, miter]);
  }
}

fn miter_point(corner: &Corner<'_>) -> Option<Point> {
  let ab = corner.incoming.tangent;
  let cd = corner.outgoing.tangent;
  let denom = ab.cross(cd);
  let scale = corner.incoming.len * corner.outgoing.len;
  if denom.abs() <= PARALLEL * scale.max(1.0) {
    return None;
  }
  let delta = corner.end - corner.start;
  let t = delta.cross(cd) / denom;
  let s = delta.cross(ab) / denom;
  if t < -EDGE || s > EDGE {
    return None;
  }
  Some(corner.start + ab * t)
}

fn push_round(out: &mut BezPath, corner: &Corner<'_>, half: f64) {
  let theta = corner
    .cross
    .atan2(corner.incoming.tangent.dot(corner.outgoing.tangent));
  let radii = Vec2::new(half, half);
  if corner.cross > 0.0 {
    let arc = Arc::new(
      corner.join,
      radii,
      (corner.start - corner.join).atan2(),
      theta,
      0.0,
    );
    out.move_to(corner.end);
    out.line_to(corner.start);
    append_arc(out, arc);
  } else {
    let arc = Arc::new(
      corner.join,
      radii,
      (corner.end - corner.join).atan2(),
      -theta,
      0.0,
    );
    out.move_to(corner.start);
    out.line_to(corner.end);
    append_arc(out, arc);
  }
  out.close_path();
}

fn push_cap(out: &mut BezPath, seg: &Seg, cap: Cap, half: f64, start: bool) {
  match cap {
    Cap::Butt => {}
    Cap::Square => push_square(out, seg, half, start),
    Cap::Round => push_round_cap(out, seg, half, start),
  }
}

fn push_square(out: &mut BezPath, seg: &Seg, half: f64, start: bool) {
  let left = left_normal(seg, half);
  let along = seg.tangent * (half / seg.len);
  if start {
    let back = seg.a - along;
    push_poly(out, &[back - left, seg.a - left, seg.a + left, back + left]);
  } else {
    let forward = seg.b + along;
    push_poly(
      out,
      &[seg.b - left, forward - left, forward + left, seg.b + left],
    );
  }
}

fn push_round_cap(out: &mut BezPath, seg: &Seg, half: f64, start: bool) {
  let left = left_normal(seg, half);
  let radii = Vec2::new(half, half);
  if start {
    let arc = Arc::new(seg.a, radii, left.atan2(), PI, 0.0);
    out.move_to(seg.a - left);
    out.line_to(seg.a + left);
    append_arc(out, arc);
  } else {
    let arc = Arc::new(seg.b, radii, (-left).atan2(), PI, 0.0);
    out.move_to(seg.b + left);
    out.line_to(seg.b - left);
    append_arc(out, arc);
  }
  out.close_path();
}

fn append_arc(out: &mut BezPath, arc: Arc) {
  for element in arc.path_elements(ARC_TOLERANCE) {
    if let PathEl::CurveTo(p1, p2, p3) = element {
      out.curve_to(p1, p2, p3);
    }
  }
}

fn push_poly(out: &mut BezPath, points: &[Point]) {
  let Some((first, rest)) = points.split_first() else {
    return;
  };
  if rest.len() < 2 || signed_area(points).abs() <= 1e-9 {
    return;
  }
  out.move_to(*first);
  for point in rest {
    out.line_to(*point);
  }
  out.close_path();
}

fn signed_area(points: &[Point]) -> f64 {
  let mut sum = 0.0;
  for (index, point) in points.iter().enumerate() {
    let next = points[(index + 1) % points.len()];
    sum += point.x * next.y - next.x * point.y;
  }
  sum * 0.5
}

#[cfg(test)]
mod tests {
  use super::crossing_outline;
  use kurbo::{BezPath, Cap, Join, Point, Shape, Stroke};

  fn line(width: f64, join: Join, cap: Cap) -> Stroke {
    Stroke::new(width)
      .with_caps(cap)
      .with_join(join)
      .with_miter_limit(10.0)
  }

  fn cross_path(out: bool) -> BezPath {
    let mut path = BezPath::new();
    path.move_to(Point::new(110.0, 50.0));
    path.line_to(Point::new(110.0, 60.0));
    if out {
      path.line_to(Point::new(120.0, 60.0));
    } else {
      path.line_to(Point::new(100.0, 60.0));
    }
    path
  }

  fn assert_outside(path: &BezPath, points: &[(f64, f64)]) {
    for (x, y) in points {
      assert_eq!(
        path.winding(Point::new(*x, *y)),
        0,
        "({x}, {y}) is inside the stroke"
      );
    }
  }

  fn assert_inside(path: &BezPath, points: &[(f64, f64)]) {
    for (x, y) in points {
      assert!(
        path.winding(Point::new(*x, *y)) > 0,
        "({x}, {y}) winding {}",
        path.winding(Point::new(*x, *y))
      );
    }
  }

  #[test]
  fn wide_bevel_does_not_fill_across_the_corner() {
    let outline = crossing_outline(
      &cross_path(false),
      &line(200.0, Join::Bevel, Cap::Butt),
    )
    .expect("inverted bevel");
    assert_outside(
      &outline,
      &[
        (1.0, 1.0),
        (48.0, 1.0),
        (48.0, 48.0),
        (1.0, 48.0),
        (60.0, 10.0),
        (180.7, 130.7),
        (200.0, 150.0),
      ],
    );
    assert_inside(
      &outline,
      &[(110.0, 55.0), (105.0, 30.0), (50.0, 55.0), (150.0, 100.0)],
    );
  }

  #[test]
  fn wide_turn_the_other_way_stays_outside_the_inner_chord() {
    let outline =
      crossing_outline(&cross_path(true), &line(200.0, Join::Bevel, Cap::Butt))
        .expect("inverted bevel");
    assert_outside(&outline, &[(160.0, 10.0), (1.0, 1.0)]);
    assert_inside(&outline, &[(110.0, 55.0)]);
  }

  #[test]
  fn ordinary_and_flat_joins_stay_on_the_rasterizer() {
    let mut elbow = BezPath::new();
    elbow.move_to(Point::new(0.0, 0.0));
    elbow.line_to(Point::new(100.0, 0.0));
    elbow.line_to(Point::new(100.0, 100.0));
    assert!(
      crossing_outline(&elbow, &line(10.0, Join::Bevel, Cap::Butt)).is_none()
    );

    let mut flat = BezPath::new();
    flat.move_to(Point::new(0.0, 24.0));
    flat.line_to(Point::new(100.0, 25.0));
    flat.line_to(Point::new(0.0, 26.0));
    flat.close_path();
    assert!(
      crossing_outline(&flat, &line(100.0, Join::Miter, Cap::Round)).is_none()
    );

    let mut uturn = BezPath::new();
    uturn.move_to(Point::new(-100.0, 25.0));
    uturn.line_to(Point::new(0.0, 25.0));
    uturn.line_to(Point::new(-100.0, 25.0));
    assert!(
      crossing_outline(&uturn, &line(300.0, Join::Round, Cap::Butt)).is_none()
    );
  }

  #[test]
  fn dashes_and_curves_stay_on_the_rasterizer() {
    let path = cross_path(false);
    let dashed =
      line(200.0, Join::Bevel, Cap::Butt).with_dashes(0.0, [1.0, 1.0]);
    assert!(crossing_outline(&path, &dashed).is_none());

    let mut curve = BezPath::new();
    curve.move_to(Point::new(110.0, 50.0));
    curve.quad_to(Point::new(110.0, 60.0), Point::new(100.0, 60.0));
    assert!(
      crossing_outline(&curve, &line(200.0, Join::Bevel, Cap::Butt)).is_none()
    );
  }

  #[test]
  fn miter_and_round_add_the_outer_piece_only() {
    let path = cross_path(false);
    let miter =
      crossing_outline(&path, &line(200.0, Join::Miter, Cap::Butt)).unwrap();
    assert_inside(&miter, &[(200.0, 150.0)]);
    assert_outside(&miter, &[(60.0, 10.0)]);

    let limited = line(200.0, Join::Miter, Cap::Butt).with_miter_limit(1.0);
    let bevel = crossing_outline(&path, &limited).unwrap();
    assert_outside(&bevel, &[(200.0, 150.0), (60.0, 10.0)]);

    let round =
      crossing_outline(&path, &line(200.0, Join::Round, Cap::Butt)).unwrap();
    assert_inside(&round, &[(180.7, 130.7)]);
    assert_outside(&round, &[(60.0, 10.0)]);
  }

  #[test]
  fn square_cap_extends_half_the_width_and_butt_does_not() {
    let path = cross_path(false);
    let butt =
      crossing_outline(&path, &line(200.0, Join::Bevel, Cap::Butt)).unwrap();
    assert_outside(&butt, &[(110.0, 0.0)]);

    let square =
      crossing_outline(&path, &line(200.0, Join::Bevel, Cap::Square)).unwrap();
    assert_inside(&square, &[(110.0, 0.0)]);

    let round =
      crossing_outline(&path, &line(200.0, Join::Bevel, Cap::Round)).unwrap();
    assert_inside(&round, &[(110.0, 0.0)]);
  }
}
