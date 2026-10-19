//! YUV and BGRA conversion into tightly packed RGBA.

#[cfg(any(target_os = "linux", target_os = "windows"))]
fn clamp(value: i32) -> u8 {
  value.clamp(0, 255) as u8
}

#[cfg(target_os = "linux")]
pub struct I420<'a> {
  pub y: &'a [u8],
  pub u: &'a [u8],
  pub v: &'a [u8],
  pub y_stride: usize,
  pub u_stride: usize,
  pub v_stride: usize,
}

/// BT.601 conversion. `limited` selects studio swing.
#[cfg(target_os = "linux")]
pub fn i420_to_rgba(
  planes: I420<'_>,
  width: usize,
  height: usize,
  limited: bool,
) -> Option<Vec<u8>> {
  let I420 {
    y,
    u,
    v,
    y_stride,
    u_stride,
    v_stride,
  } = planes;
  if width == 0 || height == 0 || y_stride < width {
    return None;
  }
  let chroma_w = width.div_ceil(2);
  let chroma_h = height.div_ceil(2);
  if u_stride < chroma_w || v_stride < chroma_w {
    return None;
  }
  if y.len() < height * y_stride
    || u.len() < chroma_h * u_stride
    || v.len() < chroma_h * v_stride
  {
    return None;
  }
  let mut rgba = vec![0u8; width * height * 4];
  for row in 0..height {
    let y_row = &y[row * y_stride..];
    let u_row = &u[(row / 2) * u_stride..];
    let v_row = &v[(row / 2) * v_stride..];
    for col in 0..width {
      let (red, green, blue) = rgb(
        i32::from(y_row[col]),
        i32::from(u_row[col / 2]),
        i32::from(v_row[col / 2]),
        limited,
      );
      let offset = (row * width + col) * 4;
      rgba[offset] = red;
      rgba[offset + 1] = green;
      rgba[offset + 2] = blue;
      rgba[offset + 3] = 255;
    }
  }
  Some(rgba)
}

#[cfg(any(target_os = "linux", target_os = "windows"))]
fn rgb(y: i32, u: i32, v: i32, limited: bool) -> (u8, u8, u8) {
  let d = u - 128;
  let e = v - 128;
  if limited {
    let c = y - 16;
    (
      clamp((298 * c + 409 * e + 128) >> 8),
      clamp((298 * c - 100 * d - 208 * e + 128) >> 8),
      clamp((298 * c + 516 * d + 128) >> 8),
    )
  } else {
    (
      clamp(y + ((359 * e) >> 8)),
      clamp(y - ((88 * d + 183 * e) >> 8)),
      clamp(y + ((454 * d) >> 8)),
    )
  }
}

/// NV12 is a Y plane plus an interleaved UV plane. `limited` selects studio swing.
#[cfg(any(target_os = "linux", target_os = "windows"))]
pub fn nv12_to_rgba(
  y: &[u8],
  uv: &[u8],
  y_stride: usize,
  uv_stride: usize,
  width: usize,
  height: usize,
  limited: bool,
) -> Option<Vec<u8>> {
  if width == 0 || height == 0 || y_stride < width || uv_stride < width {
    return None;
  }
  let chroma_h = height.div_ceil(2);
  if y.len() < height * y_stride || uv.len() < chroma_h * uv_stride {
    return None;
  }
  let mut rgba = vec![0u8; width * height * 4];
  for row in 0..height {
    let y_row = &y[row * y_stride..];
    let uv_row = &uv[(row / 2) * uv_stride..];
    for (col, sample) in y_row.iter().enumerate().take(width) {
      let uv_index = (col / 2) * 2;
      let (red, green, blue) = rgb(
        i32::from(*sample),
        i32::from(uv_row[uv_index]),
        i32::from(uv_row[uv_index + 1]),
        limited,
      );
      let offset = (row * width + col) * 4;
      rgba[offset] = red;
      rgba[offset + 1] = green;
      rgba[offset + 2] = blue;
      rgba[offset + 3] = 255;
    }
  }
  Some(rgba)
}

pub fn packed_to_rgba(
  src: &[u8],
  width: usize,
  height: usize,
  stride: usize,
  bgra: bool,
) -> Option<Vec<u8>> {
  if width == 0 || height == 0 || stride < width * 4 {
    return None;
  }
  if src.len() < height * stride {
    return None;
  }
  let mut rgba = vec![0u8; width * height * 4];
  for row in 0..height {
    let input = &src[row * stride..];
    let output = &mut rgba[row * width * 4..(row + 1) * width * 4];
    for col in 0..width {
      let pixel = &input[col * 4..col * 4 + 4];
      let dest = &mut output[col * 4..col * 4 + 4];
      if bgra {
        dest[0] = pixel[2];
        dest[1] = pixel[1];
        dest[2] = pixel[0];
      } else {
        dest[0] = pixel[0];
        dest[1] = pixel[1];
        dest[2] = pixel[2];
      }
      dest[3] = pixel[3];
    }
  }
  Some(rgba)
}
