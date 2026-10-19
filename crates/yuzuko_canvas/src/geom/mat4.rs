//! Column-major 4×4 matrices for `DOMMatrix`.
//!
//! `data[column * 4 + row]` is the DOM component `m{row+1}{column+1}`.

use std::ops::Index;
use std::ops::IndexMut;

use crate::geom::matrix::Matrix;

#[derive(Clone, Copy)]
pub(crate) struct Mat4 {
  data: [f64; 16],
}

impl Mat4 {
  pub(crate) fn identity() -> Self {
    let mut matrix = Self { data: [0.0; 16] };
    matrix.data[0] = 1.0;
    matrix.data[5] = 1.0;
    matrix.data[10] = 1.0;
    matrix.data[15] = 1.0;
    matrix
  }

  pub(crate) fn nan() -> Self {
    Self {
      data: [f64::NAN; 16],
    }
  }

  pub(crate) fn from_2d(matrix: Matrix) -> Self {
    Self::from_2d_coeffs(
      matrix.a, matrix.b, matrix.c, matrix.d, matrix.e, matrix.f,
    )
  }

  /// `matrix(a, b, c, d, e, f)`.
  pub(crate) fn from_2d_coeffs(
    a: f64,
    b: f64,
    c: f64,
    d: f64,
    e: f64,
    f: f64,
  ) -> Self {
    Self {
      data: [
        a, b, 0.0, 0.0, c, d, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, e, f, 0.0, 1.0,
      ],
    }
  }

  pub(crate) fn from_column(data: [f64; 16]) -> Self {
    Self { data }
  }

  pub(crate) fn columns(self) -> [f64; 16] {
    self.data
  }

  pub(crate) fn get(self, index: usize) -> f64 {
    self.data[index]
  }

  pub(crate) fn set(&mut self, index: usize, value: f64) {
    self.data[index] = value;
  }

  pub(crate) fn project_2d(self) -> Matrix {
    Matrix {
      a: self[(0, 0)],
      b: self[(1, 0)],
      c: self[(0, 1)],
      d: self[(1, 1)],
      e: self[(0, 3)],
      f: self[(1, 3)],
    }
  }

  pub(crate) fn is_identity(self) -> bool {
    self.data
      == [
        1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0,
        0.0, 1.0,
      ]
  }

  pub(crate) fn translate(
    &mut self,
    tx: f64,
    ty: f64,
    tz: f64,
    is_2d: &mut bool,
  ) {
    let x = self[(0, 0)] * tx + self[(0, 1)] * ty + self[(0, 2)] * tz;
    let y = self[(1, 0)] * tx + self[(1, 1)] * ty + self[(1, 2)] * tz;
    let z = self[(2, 0)] * tx + self[(2, 1)] * ty + self[(2, 2)] * tz;
    let w = self[(3, 0)] * tx + self[(3, 1)] * ty + self[(3, 2)] * tz;
    self[(0, 3)] += x;
    self[(1, 3)] += y;
    self[(2, 3)] += z;
    self[(3, 3)] += w;
    *is_2d = *is_2d && tz == 0.0;
  }

  pub(crate) fn scale(&mut self, sx: f64, sy: f64, sz: f64, is_2d: &mut bool) {
    for row in 0..4 {
      self[(row, 0)] *= sx;
      self[(row, 1)] *= sy;
      self[(row, 2)] *= sz;
    }
    *is_2d = *is_2d && sz == 1.0;
  }

  pub(crate) fn rotate_axis(
    &mut self,
    x: f64,
    y: f64,
    z: f64,
    angle: f64,
    is_2d: &mut bool,
  ) {
    if x == 0.0 && y == 0.0 && z == 0.0 {
      return;
    }
    let rotation = rotation_from_axis_angle(x, y, z, angle);
    self.post_multiply_first3(&rotation);
    *is_2d = *is_2d && x == 0.0 && y == 0.0;
  }

  pub(crate) fn skew(&mut self, x: f64, y: f64) {
    let tan_x = x.tan();
    let tan_y = y.tan();
    let left = *self;
    for row in 0..4 {
      self[(row, 0)] = left[(row, 0)] + left[(row, 1)] * tan_y;
      self[(row, 1)] = left[(row, 0)] * tan_x + left[(row, 1)];
    }
  }

  pub(crate) fn perspective(&mut self, distance: f64) {
    if distance == 0.0 {
      return;
    }
    let left = *self;
    for row in 0..4 {
      self[(row, 2)] = left[(row, 2)] - left[(row, 3)] / distance;
    }
  }

  pub(crate) fn multiply(lhs: &Self, rhs: &Self) -> Self {
    let mut out = Self { data: [0.0; 16] };
    for col in 0..4 {
      for row in 0..4 {
        out[(row, col)] = lhs[(row, 0)] * rhs[(0, col)]
          + lhs[(row, 1)] * rhs[(1, col)]
          + lhs[(row, 2)] * rhs[(2, col)]
          + lhs[(row, 3)] * rhs[(3, col)];
      }
    }
    out
  }

  pub(crate) fn inverse(self) -> Option<Self> {
    let mut copy = self;
    if copy.try_inverse_mut() {
      Some(copy)
    } else {
      None
    }
  }

  /// Inverse of a 2D affine matrix. The 3D rows stay `0, 0, 1, 0`.
  pub(crate) fn inverse_2d(self) -> Option<Self> {
    let a = self[(0, 0)];
    let b = self[(1, 0)];
    let c = self[(0, 1)];
    let d = self[(1, 1)];
    let e = self[(0, 3)];
    let f = self[(1, 3)];
    let det = a * d - b * c;
    if det == 0.0 || !det.is_finite() {
      return None;
    }
    let a2 = d / det;
    let b2 = -b / det;
    let c2 = -c / det;
    let d2 = a / det;
    let e2 = (c * f - d * e) / det;
    let f2 = (b * e - a * f) / det;
    if ![a2, b2, c2, d2, e2, f2].iter().copied().all(f64::is_finite) {
      return None;
    }
    Some(Self::from_2d_coeffs(a2, b2, c2, d2, e2, f2))
  }

  fn post_multiply_first3(&mut self, rhs: &Self) {
    let left = *self;
    for col in 0..3 {
      for row in 0..4 {
        self[(row, col)] = left[(row, 0)] * rhs[(0, col)]
          + left[(row, 1)] * rhs[(1, col)]
          + left[(row, 2)] * rhs[(2, col)]
          + left[(row, 3)] * rhs[(3, col)];
      }
    }
  }

  fn try_inverse_mut(&mut self) -> bool {
    let m = self.data;
    let cofactor00 =
      m[5] * m[10] * m[15] - m[5] * m[11] * m[14] - m[9] * m[6] * m[15]
        + m[9] * m[7] * m[14]
        + m[13] * m[6] * m[11]
        - m[13] * m[7] * m[10];
    let cofactor01 =
      -m[4] * m[10] * m[15] + m[4] * m[11] * m[14] + m[8] * m[6] * m[15]
        - m[8] * m[7] * m[14]
        - m[12] * m[6] * m[11]
        + m[12] * m[7] * m[10];
    let cofactor02 =
      m[4] * m[9] * m[15] - m[4] * m[11] * m[13] - m[8] * m[5] * m[15]
        + m[8] * m[7] * m[13]
        + m[12] * m[5] * m[11]
        - m[12] * m[7] * m[9];
    let cofactor03 =
      -m[4] * m[9] * m[14] + m[4] * m[10] * m[13] + m[8] * m[5] * m[14]
        - m[8] * m[6] * m[13]
        - m[12] * m[5] * m[10]
        + m[12] * m[6] * m[9];
    let det = m[0] * cofactor00
      + m[1] * cofactor01
      + m[2] * cofactor02
      + m[3] * cofactor03;
    if det == 0.0 {
      return false;
    }
    self[(0, 0)] = cofactor00;
    self[(1, 0)] =
      -m[1] * m[10] * m[15] + m[1] * m[11] * m[14] + m[9] * m[2] * m[15]
        - m[9] * m[3] * m[14]
        - m[13] * m[2] * m[11]
        + m[13] * m[3] * m[10];
    self[(2, 0)] =
      m[1] * m[6] * m[15] - m[1] * m[7] * m[14] - m[5] * m[2] * m[15]
        + m[5] * m[3] * m[14]
        + m[13] * m[2] * m[7]
        - m[13] * m[3] * m[6];
    self[(3, 0)] =
      -m[1] * m[6] * m[11] + m[1] * m[7] * m[10] + m[5] * m[2] * m[11]
        - m[5] * m[3] * m[10]
        - m[9] * m[2] * m[7]
        + m[9] * m[3] * m[6];
    self[(0, 1)] = cofactor01;
    self[(1, 1)] =
      m[0] * m[10] * m[15] - m[0] * m[11] * m[14] - m[8] * m[2] * m[15]
        + m[8] * m[3] * m[14]
        + m[12] * m[2] * m[11]
        - m[12] * m[3] * m[10];
    self[(2, 1)] =
      -m[0] * m[6] * m[15] + m[0] * m[7] * m[14] + m[4] * m[2] * m[15]
        - m[4] * m[3] * m[14]
        - m[12] * m[2] * m[7]
        + m[12] * m[3] * m[6];
    self[(3, 1)] =
      m[0] * m[6] * m[11] - m[0] * m[7] * m[10] - m[4] * m[2] * m[11]
        + m[4] * m[3] * m[10]
        + m[8] * m[2] * m[7]
        - m[8] * m[3] * m[6];
    self[(0, 2)] = cofactor02;
    self[(1, 2)] =
      -m[0] * m[9] * m[15] + m[0] * m[11] * m[13] + m[8] * m[1] * m[15]
        - m[8] * m[3] * m[13]
        - m[12] * m[1] * m[11]
        + m[12] * m[3] * m[9];
    self[(2, 2)] =
      m[0] * m[5] * m[15] - m[0] * m[7] * m[13] - m[4] * m[1] * m[15]
        + m[4] * m[3] * m[13]
        + m[12] * m[1] * m[7]
        - m[12] * m[3] * m[5];
    self[(3, 2)] =
      -m[0] * m[5] * m[11] + m[0] * m[7] * m[9] + m[4] * m[1] * m[11]
        - m[4] * m[3] * m[9]
        - m[8] * m[1] * m[7]
        + m[8] * m[3] * m[5];
    self[(0, 3)] = cofactor03;
    self[(1, 3)] =
      m[0] * m[9] * m[14] - m[0] * m[10] * m[13] - m[8] * m[1] * m[14]
        + m[8] * m[2] * m[13]
        + m[12] * m[1] * m[10]
        - m[12] * m[2] * m[9];
    self[(2, 3)] =
      -m[0] * m[5] * m[14] + m[0] * m[6] * m[13] + m[4] * m[1] * m[14]
        - m[4] * m[2] * m[13]
        - m[12] * m[1] * m[6]
        + m[12] * m[2] * m[5];
    self[(3, 3)] =
      m[0] * m[5] * m[10] - m[0] * m[6] * m[9] - m[4] * m[1] * m[10]
        + m[4] * m[2] * m[9]
        + m[8] * m[1] * m[6]
        - m[8] * m[2] * m[5];
    let inv_det = 1.0 / det;
    for col in 0..4 {
      for row in 0..4 {
        self[(row, col)] *= inv_det;
      }
    }
    true
  }
}

impl Index<(usize, usize)> for Mat4 {
  type Output = f64;

  fn index(&self, (row, col): (usize, usize)) -> &Self::Output {
    &self.data[col * 4 + row]
  }
}

impl IndexMut<(usize, usize)> for Mat4 {
  fn index_mut(&mut self, (row, col): (usize, usize)) -> &mut Self::Output {
    &mut self.data[col * 4 + row]
  }
}

fn rotation_from_axis_angle(x: f64, y: f64, z: f64, angle: f64) -> Mat4 {
  if angle == 0.0 {
    return Mat4::identity();
  }
  let length = (x * x + y * y + z * z).sqrt();
  let inv_length = 1.0 / length;
  let x = x * inv_length;
  let y = y * inv_length;
  let z = z * inv_length;
  let (sin, cos) = angle.sin_cos();
  let one_m_cos = 1.0 - cos;
  let mut matrix = Mat4 { data: [0.0; 16] };
  matrix[(0, 0)] = x * x + (1.0 - x * x) * cos;
  matrix[(1, 0)] = x * y * one_m_cos + z * sin;
  matrix[(2, 0)] = x * z * one_m_cos - y * sin;
  matrix[(0, 1)] = x * y * one_m_cos - z * sin;
  matrix[(1, 1)] = y * y + (1.0 - y * y) * cos;
  matrix[(2, 1)] = y * z * one_m_cos + x * sin;
  matrix[(0, 2)] = x * z * one_m_cos + y * sin;
  matrix[(1, 2)] = y * z * one_m_cos - x * sin;
  matrix[(2, 2)] = z * z + (1.0 - z * z) * cos;
  matrix[(3, 3)] = 1.0;
  matrix
}
