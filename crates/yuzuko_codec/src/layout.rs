//! Pixel layout for `VideoFrame` buffers. Pure functions: bytes in, planes out.
//!
//! A buffer constructor copies only the visible rectangle and places it at the
//! origin, which is the behavior `videoFrame-copyTo` checks. An image or
//! another frame keeps the coded planes and stores the visible rectangle.

use yuzuko_image::{
  ColorSpace, Raster, SampleFormat, convert_encoded, unpremultiply,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PixelFormat {
  Rgba,
  Bgra,
  Rgbx,
  Bgrx,
  I420,
  I420A,
  I422,
  I444,
  Nv12,
}

pub(crate) enum FormatClass {
  Stored(PixelFormat),
  /// A WebCodecs format this runtime does not store, such as `I420P10`.
  Unsupported,
  Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct RectD {
  pub x: f64,
  pub y: f64,
  pub width: f64,
  pub height: f64,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct PlaneLayoutIn {
  pub offset: u64,
  pub stride: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PlaneLayoutOut {
  pub offset: u64,
  pub stride: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum LayoutError {
  PlaneCount,
  Stride,
  Overflow,
  Overlap,
  Rect,
  Alignment,
  TooSmall,
  Format,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FrameColor {
  pub primaries: Option<String>,
  pub transfer: Option<String>,
  pub matrix: Option<String>,
  pub full_range: Option<bool>,
}

impl FrameColor {
  pub(crate) fn srgb() -> Self {
    Self {
      primaries: Some("bt709".to_string()),
      transfer: Some("iec61966-2-1".to_string()),
      matrix: Some("rgb".to_string()),
      full_range: Some(true),
    }
  }

  pub(crate) fn display_p3() -> Self {
    Self {
      primaries: Some("smpte432".to_string()),
      transfer: Some("iec61966-2-1".to_string()),
      matrix: Some("rgb".to_string()),
      full_range: Some(true),
    }
  }

  pub(crate) fn rec709() -> Self {
    Self {
      primaries: Some("bt709".to_string()),
      transfer: Some("bt709".to_string()),
      matrix: Some("bt709".to_string()),
      full_range: Some(false),
    }
  }

  pub(crate) fn empty() -> Self {
    Self {
      primaries: None,
      transfer: None,
      matrix: None,
      full_range: None,
    }
  }

  pub(crate) fn from_color_space(space: ColorSpace) -> Self {
    match space {
      ColorSpace::Srgb => Self::srgb(),
      ColorSpace::DisplayP3 => Self::display_p3(),
    }
  }
}

#[derive(Clone, Debug)]
pub(crate) struct FrameData {
  pub format: PixelFormat,
  pub coded_width: u32,
  pub coded_height: u32,
  pub visible_x: f64,
  pub visible_y: f64,
  pub visible_width: f64,
  pub visible_height: f64,
  pub display_width: u32,
  pub display_height: u32,
  pub color: FrameColor,
  pub planes: Vec<Vec<u8>>,
  pub raster: Raster,
}

#[derive(Clone, Copy)]
struct PlaneKind {
  sample_width: u32,
  sample_height: u32,
  sample_bytes: u32,
}

struct ComputedPlane {
  source_top: u32,
  source_height: u32,
  source_left_bytes: u32,
  source_width_bytes: u32,
  destination_offset: u64,
  destination_stride: u64,
}

struct CombinedLayout {
  planes: Vec<ComputedPlane>,
  allocation_size: u64,
}

const UNSUPPORTED: &[&str] = &[
  "I420P10", "I420P12", "I420AP10", "I420AP12", "I422P10", "I422P12", "I422A",
  "I422AP10", "I422AP12", "I444P10", "I444P12", "I444A", "I444AP10",
  "I444AP12",
];

impl PixelFormat {
  pub(crate) fn name(self) -> &'static str {
    match self {
      Self::Rgba => "RGBA",
      Self::Bgra => "BGRA",
      Self::Rgbx => "RGBX",
      Self::Bgrx => "BGRX",
      Self::I420 => "I420",
      Self::I420A => "I420A",
      Self::I422 => "I422",
      Self::I444 => "I444",
      Self::Nv12 => "NV12",
    }
  }

  pub(crate) fn is_rgb(self) -> bool {
    matches!(self, Self::Rgba | Self::Bgra | Self::Rgbx | Self::Bgrx)
  }

  pub(crate) fn code(self) -> u32 {
    match self {
      Self::Rgba => 0,
      Self::Bgra => 1,
      Self::Rgbx => 2,
      Self::Bgrx => 3,
      Self::I420 => 4,
      Self::I420A => 5,
      Self::I422 => 6,
      Self::I444 => 7,
      Self::Nv12 => 8,
    }
  }

  pub(crate) fn from_code(code: u32) -> Option<Self> {
    Some(match code {
      0 => Self::Rgba,
      1 => Self::Bgra,
      2 => Self::Rgbx,
      3 => Self::Bgrx,
      4 => Self::I420,
      5 => Self::I420A,
      6 => Self::I422,
      7 => Self::I444,
      8 => Self::Nv12,
      _ => return None,
    })
  }

  pub(crate) fn plane_count(self) -> usize {
    match self {
      Self::Rgba | Self::Bgra | Self::Rgbx | Self::Bgrx => 1,
      Self::Nv12 => 2,
      Self::I420 | Self::I422 | Self::I444 => 3,
      Self::I420A => 4,
    }
  }

  fn plane(self, index: usize) -> PlaneKind {
    let full = PlaneKind {
      sample_width: 1,
      sample_height: 1,
      sample_bytes: 1,
    };
    match (self, index) {
      (Self::Rgba | Self::Bgra | Self::Rgbx | Self::Bgrx, 0) => PlaneKind {
        sample_bytes: 4,
        ..full
      },
      (Self::I420 | Self::I420A | Self::I422 | Self::I444, 0)
      | (Self::I420A, 3)
      | (Self::Nv12, 0) => full,
      (Self::I420 | Self::I420A, 1 | 2) | (Self::Nv12, 1) => PlaneKind {
        sample_width: 2,
        sample_height: 2,
        sample_bytes: if self == Self::Nv12 { 2 } else { 1 },
      },
      (Self::I422, 1 | 2) => PlaneKind {
        sample_width: 2,
        sample_height: 1,
        sample_bytes: 1,
      },
      (Self::I444, 1 | 2) => full,
      _ => full,
    }
  }
}

pub(crate) fn classify(name: &str) -> FormatClass {
  let stored = match name {
    "RGBA" => Some(PixelFormat::Rgba),
    "BGRA" => Some(PixelFormat::Bgra),
    "RGBX" => Some(PixelFormat::Rgbx),
    "BGRX" => Some(PixelFormat::Bgrx),
    "I420" => Some(PixelFormat::I420),
    "I420A" => Some(PixelFormat::I420A),
    "I422" => Some(PixelFormat::I422),
    "I444" => Some(PixelFormat::I444),
    "NV12" => Some(PixelFormat::Nv12),
    _ => None,
  };
  if let Some(format) = stored {
    return FormatClass::Stored(format);
  }
  if UNSUPPORTED.contains(&name) {
    FormatClass::Unsupported
  } else {
    FormatClass::Unknown
  }
}

pub(crate) fn full_rect(width: u32, height: u32) -> RectD {
  RectD {
    x: 0.0,
    y: 0.0,
    width: f64::from(width),
    height: f64::from(height),
  }
}

/// `default` is used when `override_rect` is absent. `x` and `y` must lie on
/// a sample boundary. Width and height may end on a partial sample.
pub(crate) fn parse_visible(
  default_rect: RectD,
  override_rect: Option<RectD>,
  coded_width: u32,
  coded_height: u32,
  format: PixelFormat,
) -> Result<RectD, LayoutError> {
  let rect = override_rect.unwrap_or(default_rect);
  if !rect.x.is_finite()
    || !rect.y.is_finite()
    || !rect.width.is_finite()
    || !rect.height.is_finite()
    || rect.x < 0.0
    || rect.y < 0.0
    || rect.width < 0.0
    || rect.height < 0.0
  {
    return Err(LayoutError::Rect);
  }
  if rect.width == 0.0 || rect.height == 0.0 {
    return Err(LayoutError::Rect);
  }
  if rect.x + rect.width > f64::from(coded_width)
    || rect.y + rect.height > f64::from(coded_height)
  {
    return Err(LayoutError::Rect);
  }
  for plane in 0..format.plane_count() {
    let kind = format.plane(plane);
    if !is_multiple(rect.x, kind.sample_width)
      || !is_multiple(rect.y, kind.sample_height)
    {
      return Err(LayoutError::Alignment);
    }
  }
  Ok(rect)
}

pub(crate) fn display_size(
  visible_width: f64,
  visible_height: f64,
  base_visible_width: f64,
  base_visible_height: f64,
  base_display_width: f64,
  base_display_height: f64,
) -> Result<(u32, u32), LayoutError> {
  if base_visible_width <= 0.0 || base_visible_height <= 0.0 {
    return Err(LayoutError::Rect);
  }
  let width = visible_width * base_display_width / base_visible_width;
  let height = visible_height * base_display_height / base_visible_height;
  unsigned_round(width)?;
  unsigned_round(height)?;
  Ok((unsigned_round(width)?, unsigned_round(height)?))
}

pub(crate) fn frame_from_raster(raster: &Raster) -> FrameData {
  FrameData {
    format: PixelFormat::Rgba,
    coded_width: raster.width,
    coded_height: raster.height,
    visible_x: 0.0,
    visible_y: 0.0,
    visible_width: f64::from(raster.width),
    visible_height: f64::from(raster.height),
    display_width: raster.width,
    display_height: raster.height,
    color: FrameColor::from_color_space(raster.color_space),
    planes: vec![rgba_plane(raster)],
    raster: raster.clone(),
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct BufferPixels {
  pub coded_width: u32,
  pub coded_height: u32,
  pub planes: Vec<Vec<u8>>,
}

/// Copy `data` into tight planes. A visible rectangle smaller than the coded
/// size becomes the new coded size, with the origin reset to `(0, 0)`.
pub(crate) fn read_buffer(
  data: &[u8],
  format: PixelFormat,
  coded_width: u32,
  coded_height: u32,
  visible: RectD,
  layout: Option<&[PlaneLayoutIn]>,
) -> Result<BufferPixels, LayoutError> {
  let full = full_rect(coded_width, coded_height);
  let source = compute_layout(full, format, layout)?;
  if data.len() < usize_size(source.allocation_size)? {
    return Err(LayoutError::TooSmall);
  }
  let window = compute_layout(visible, format, None)?;
  let mut planes = Vec::with_capacity(format.plane_count());
  for (index, plane) in window.planes.iter().enumerate() {
    let src = &source.planes[index];
    let mut tight = vec![0u8; plane_bytes(plane)?];
    copy_window(data, src, plane, &mut tight)?;
    planes.push(tight);
  }
  let coded_width = trunc_unsigned(visible.width)?;
  let coded_height = trunc_unsigned(visible.height)?;
  Ok(BufferPixels {
    coded_width,
    coded_height,
    planes,
  })
}

pub(crate) fn discard_alpha(
  format: PixelFormat,
  planes: &mut Vec<Vec<u8>>,
) -> PixelFormat {
  match format {
    PixelFormat::Rgba | PixelFormat::Rgbx => {
      fill_channel(planes, 3);
      PixelFormat::Rgbx
    }
    PixelFormat::Bgra | PixelFormat::Bgrx => {
      fill_channel(planes, 3);
      PixelFormat::Bgrx
    }
    PixelFormat::I420A => {
      planes.truncate(3);
      PixelFormat::I420
    }
    other => other,
  }
}

pub(crate) fn visible_raster(
  format: PixelFormat,
  planes: &[Vec<u8>],
  coded_width: u32,
  coded_height: u32,
  visible: RectD,
  color: ColorSpace,
  limited: bool,
) -> Result<Raster, LayoutError> {
  let width = trunc_unsigned(visible.width)?;
  let height = trunc_unsigned(visible.height)?;
  let origin_x = trunc_u32(visible.x);
  let origin_y = trunc_u32(visible.y);
  let mut rgba = Vec::with_capacity(width as usize * height as usize * 4);
  for y in 0..height {
    for x in 0..width {
      let pixel = rgba_at(
        format,
        planes,
        coded_width,
        coded_height,
        origin_x + x,
        origin_y + y,
        limited,
      )?;
      rgba.extend_from_slice(&pixel);
    }
  }
  Raster::from_rgba8(width, height, color, &rgba).map_err(|_| LayoutError::Rect)
}

pub(crate) fn allocation(
  format: PixelFormat,
  rect: RectD,
  layout: Option<&[PlaneLayoutIn]>,
) -> Result<(u64, Vec<PlaneLayoutOut>), LayoutError> {
  let combined = compute_layout(rect, format, layout)?;
  let layouts = combined
    .planes
    .iter()
    .map(|plane| PlaneLayoutOut {
      offset: plane.destination_offset,
      stride: plane.destination_stride,
    })
    .collect();
  Ok((combined.allocation_size, layouts))
}

/// Write `frame`'s `rect` into `dest`. Padding bytes already in `dest` stay.
///
/// `dest_color` is the RGB destination color space. `None` leaves the encoded
/// samples unchanged, which is the same-format copy that ignores `colorSpace`.
pub(crate) fn write_copy(
  frame: &FrameData,
  rect: RectD,
  dest_format: PixelFormat,
  layout: Option<&[PlaneLayoutIn]>,
  limited: bool,
  dest_color: Option<ColorSpace>,
  dest: &mut [u8],
) -> Result<Vec<PlaneLayoutOut>, LayoutError> {
  let (size, layouts) = allocation(dest_format, rect, layout)?;
  if dest.len() < usize_size(size)? {
    return Err(LayoutError::TooSmall);
  }
  if dest_format == frame.format {
    write_same(frame, rect, &layouts, dest_color, dest)?;
  } else if dest_format.is_rgb() {
    write_rgb(
      frame,
      rect,
      dest_format,
      &layouts,
      limited,
      dest_color,
      dest,
    )?;
  } else {
    return Err(LayoutError::Format);
  }
  Ok(layouts)
}

pub(crate) fn raster_color(color: &FrameColor) -> ColorSpace {
  if color.primaries.as_deref() == Some("smpte432") {
    ColorSpace::DisplayP3
  } else {
    ColorSpace::Srgb
  }
}

/// YUV with a missing or limited `fullRange` uses studio swing. RGB does not.
pub(crate) fn limited_range(format: PixelFormat, color: &FrameColor) -> bool {
  !format.is_rgb() && color.full_range != Some(true)
}

fn is_multiple(value: f64, sample: u32) -> bool {
  if sample == 0 {
    return false;
  }
  let quot = value / f64::from(sample);
  quot.is_finite() && quot.fract() == 0.0
}

fn unsigned_round(value: f64) -> Result<u32, LayoutError> {
  if !value.is_finite() || value < 1.0 || value > f64::from(u32::MAX) {
    return Err(LayoutError::Rect);
  }
  Ok(value.round() as u32)
}

fn trunc_unsigned(value: f64) -> Result<u32, LayoutError> {
  if !value.is_finite() || value < 1.0 || value > f64::from(u32::MAX) {
    return Err(LayoutError::Rect);
  }
  let truncated = value.trunc();
  if truncated < 1.0 {
    return Err(LayoutError::Rect);
  }
  Ok(truncated as u32)
}

fn trunc_u32(value: f64) -> u32 {
  if !value.is_finite() || value <= 0.0 {
    0
  } else if value >= f64::from(u32::MAX) {
    u32::MAX
  } else {
    value.trunc() as u32
  }
}

fn usize_size(value: u64) -> Result<usize, LayoutError> {
  usize::try_from(value).map_err(|_| LayoutError::Overflow)
}

fn plane_bytes(plane: &ComputedPlane) -> Result<usize, LayoutError> {
  let width = usize_size(u64::from(plane.source_width_bytes))?;
  let height = usize_size(u64::from(plane.source_height))?;
  width.checked_mul(height).ok_or(LayoutError::Overflow)
}

fn compute_layout(
  rect: RectD,
  format: PixelFormat,
  layout: Option<&[PlaneLayoutIn]>,
) -> Result<CombinedLayout, LayoutError> {
  let count = format.plane_count();
  if let Some(layout) = layout
    && layout.len() != count
  {
    return Err(LayoutError::PlaneCount);
  }
  let mut planes: Vec<ComputedPlane> = Vec::with_capacity(count);
  let mut ends: Vec<u64> = Vec::with_capacity(count);
  let mut min_size = 0u64;
  let origin_x = trunc_u32(rect.x);
  let origin_y = trunc_u32(rect.y);
  let width = trunc_unsigned(rect.width)?;
  let height = trunc_unsigned(rect.height)?;
  for index in 0..count {
    let kind = format.plane(index);
    let source_width_bytes =
      width.div_ceil(kind.sample_width) * kind.sample_bytes;
    let source_height = height.div_ceil(kind.sample_height);
    let source_left_bytes = origin_x / kind.sample_width * kind.sample_bytes;
    let source_top = origin_y.div_ceil(kind.sample_height);
    let (destination_offset, destination_stride) = if let Some(layout) = layout
    {
      let item = layout[index];
      if item.stride < u64::from(source_width_bytes) {
        return Err(LayoutError::Stride);
      }
      (item.offset, item.stride)
    } else {
      (min_size, u64::from(source_width_bytes))
    };
    let plane_size = destination_stride
      .checked_mul(u64::from(source_height))
      .ok_or(LayoutError::Overflow)?;
    if plane_size > u64::from(u32::MAX) {
      return Err(LayoutError::Overflow);
    }
    let plane_end = destination_offset
      .checked_add(plane_size)
      .ok_or(LayoutError::Overflow)?;
    if plane_end > u64::from(u32::MAX) {
      return Err(LayoutError::Overflow);
    }
    for earlier in 0..index {
      let earlier_end = ends[earlier];
      let earlier_offset = planes[earlier].destination_offset;
      let disjoint =
        plane_end <= earlier_offset || earlier_end <= destination_offset;
      if !disjoint {
        return Err(LayoutError::Overlap);
      }
    }
    ends.push(plane_end);
    if plane_end > min_size {
      min_size = plane_end;
    }
    planes.push(ComputedPlane {
      source_top,
      source_height,
      source_left_bytes,
      source_width_bytes,
      destination_offset,
      destination_stride,
    });
  }
  Ok(CombinedLayout {
    planes,
    allocation_size: min_size,
  })
}

fn copy_window(
  data: &[u8],
  source: &ComputedPlane,
  window: &ComputedPlane,
  tight: &mut [u8],
) -> Result<(), LayoutError> {
  let row_bytes = usize_size(u64::from(window.source_width_bytes))?;
  let stride = usize_size(source.destination_stride)?;
  let offset = usize_size(source.destination_offset)?;
  let left = usize_size(u64::from(window.source_left_bytes))?;
  for row in 0..window.source_height {
    let src_row = u64::from(window.source_top) + u64::from(row);
    let src = offset
      .checked_add(
        stride
          .checked_mul(usize_size(src_row)?)
          .ok_or(LayoutError::Overflow)?,
      )
      .and_then(|start| start.checked_add(left))
      .ok_or(LayoutError::Overflow)?;
    let end = src.checked_add(row_bytes).ok_or(LayoutError::Overflow)?;
    let src_bytes = data.get(src..end).ok_or(LayoutError::TooSmall)?;
    let dest = row as usize * row_bytes;
    tight[dest..dest + row_bytes].copy_from_slice(src_bytes);
  }
  Ok(())
}

fn write_same(
  frame: &FrameData,
  rect: RectD,
  layouts: &[PlaneLayoutOut],
  dest_color: Option<ColorSpace>,
  dest: &mut [u8],
) -> Result<(), LayoutError> {
  let window = compute_layout(rect, frame.format, None)?;
  for (index, plane) in window.planes.iter().enumerate() {
    let stored = frame.planes.get(index).ok_or(LayoutError::TooSmall)?;
    let row_bytes = plane.source_width_bytes as usize;
    let coded_stride = coded_row_bytes(frame.format, index, frame.coded_width)?;
    let dest_layout = layouts.get(index).ok_or(LayoutError::PlaneCount)?;
    let dest_offset = usize_size(dest_layout.offset)?;
    let dest_stride = usize_size(dest_layout.stride)?;
    for row in 0..plane.source_height {
      let src_y = plane.source_top as usize + row as usize;
      let src = src_y
        .checked_mul(coded_stride)
        .and_then(|start| start.checked_add(plane.source_left_bytes as usize))
        .ok_or(LayoutError::Overflow)?;
      let src_end = src.checked_add(row_bytes).ok_or(LayoutError::Overflow)?;
      let src_bytes = stored.get(src..src_end).ok_or(LayoutError::TooSmall)?;
      let dest_at = dest_offset
        .checked_add(
          (row as usize)
            .checked_mul(dest_stride)
            .ok_or(LayoutError::Overflow)?,
        )
        .ok_or(LayoutError::Overflow)?;
      let dest_end = dest_at
        .checked_add(row_bytes)
        .ok_or(LayoutError::Overflow)?;
      let slot = dest
        .get_mut(dest_at..dest_end)
        .ok_or(LayoutError::TooSmall)?;
      slot.copy_from_slice(src_bytes);
      // An explicit RGB destination converts into `dest_color`. RGBX and BGRX
      // also replace the unused byte with 255.
      // https://github.com/w3c/webcodecs/issues/971
      if frame.format.is_rgb()
        && (dest_color.is_some()
          || frame.format == PixelFormat::Rgbx
          || frame.format == PixelFormat::Bgrx)
      {
        let source = raster_color(&frame.color);
        let target = dest_color.unwrap_or(source);
        for pixel in slot.chunks_exact_mut(4) {
          let rgba = recolor(unpack_rgb(frame.format, pixel), source, target);
          pixel.copy_from_slice(&pack_rgb(frame.format, rgba));
        }
      }
    }
  }
  Ok(())
}

fn write_rgb(
  frame: &FrameData,
  rect: RectD,
  dest_format: PixelFormat,
  layouts: &[PlaneLayoutOut],
  limited: bool,
  dest_color: Option<ColorSpace>,
  dest: &mut [u8],
) -> Result<(), LayoutError> {
  let width = trunc_unsigned(rect.width)?;
  let height = trunc_unsigned(rect.height)?;
  let origin_x = trunc_u32(rect.x);
  let origin_y = trunc_u32(rect.y);
  let layout = layouts.first().ok_or(LayoutError::PlaneCount)?;
  let stride = usize_size(layout.stride)?;
  let offset = usize_size(layout.offset)?;
  for row in 0..height {
    for column in 0..width {
      let pixel = rgba_at(
        frame.format,
        &frame.planes,
        frame.coded_width,
        frame.coded_height,
        origin_x + column,
        origin_y + row,
        limited,
      )?;
      let pixel = match dest_color {
        Some(dest_color) => {
          recolor(pixel, raster_color(&frame.color), dest_color)
        }
        None => pixel,
      };
      let packed = pack_rgb(dest_format, pixel);
      let row_at = (row as usize)
        .checked_mul(stride)
        .ok_or(LayoutError::Overflow)?;
      let column_at = (column as usize)
        .checked_mul(4)
        .ok_or(LayoutError::Overflow)?;
      let at = offset
        .checked_add(row_at)
        .and_then(|start| start.checked_add(column_at))
        .ok_or(LayoutError::Overflow)?;
      let slot = dest.get_mut(at..at + 4).ok_or(LayoutError::TooSmall)?;
      slot.copy_from_slice(&packed);
    }
  }
  Ok(())
}

fn coded_row_bytes(
  format: PixelFormat,
  plane: usize,
  coded_width: u32,
) -> Result<usize, LayoutError> {
  let kind = format.plane(plane);
  let samples = coded_width.div_ceil(kind.sample_width);
  usize_size(u64::from(samples) * u64::from(kind.sample_bytes))
}

fn rgba_at(
  format: PixelFormat,
  planes: &[Vec<u8>],
  coded_width: u32,
  coded_height: u32,
  x: u32,
  y: u32,
  limited: bool,
) -> Result<[u8; 4], LayoutError> {
  if x >= coded_width || y >= coded_height {
    return Err(LayoutError::Rect);
  }
  if format.is_rgb() {
    let plane = planes.first().ok_or(LayoutError::TooSmall)?;
    let index = (y as usize * coded_width as usize + x as usize) * 4;
    let pixel = plane.get(index..index + 4).ok_or(LayoutError::TooSmall)?;
    return Ok(unpack_rgb(format, pixel));
  }
  let y_value = sample(planes, format, 0, coded_width, x, y, 0)?;
  let (u, v) = if format == PixelFormat::Nv12 {
    (
      sample(planes, format, 1, coded_width, x, y, 0)?,
      sample(planes, format, 1, coded_width, x, y, 1)?,
    )
  } else {
    (
      sample(planes, format, 1, coded_width, x, y, 0)?,
      sample(planes, format, 2, coded_width, x, y, 0)?,
    )
  };
  let (red, green, blue) =
    yuv_to_rgb(i32::from(y_value), i32::from(u), i32::from(v), limited);
  let alpha = if format == PixelFormat::I420A {
    sample(planes, format, 3, coded_width, x, y, 0)?
  } else {
    255
  };
  Ok([red, green, blue, alpha])
}

fn sample(
  planes: &[Vec<u8>],
  format: PixelFormat,
  plane: usize,
  coded_width: u32,
  x: u32,
  y: u32,
  channel: usize,
) -> Result<u8, LayoutError> {
  let kind = format.plane(plane);
  let px = x / kind.sample_width;
  let py = y / kind.sample_height;
  let row = coded_row_bytes(format, plane, coded_width)?;
  let index =
    py as usize * row + px as usize * kind.sample_bytes as usize + channel;
  planes
    .get(plane)
    .and_then(|bytes| bytes.get(index).copied())
    .ok_or(LayoutError::TooSmall)
}

fn unpack_rgb(format: PixelFormat, pixel: &[u8]) -> [u8; 4] {
  match format {
    PixelFormat::Bgra => [pixel[2], pixel[1], pixel[0], pixel[3]],
    PixelFormat::Bgrx => [pixel[2], pixel[1], pixel[0], 255],
    PixelFormat::Rgbx => [pixel[0], pixel[1], pixel[2], 255],
    _ => [pixel[0], pixel[1], pixel[2], pixel[3]],
  }
}

fn recolor(
  pixel: [u8; 4],
  source: ColorSpace,
  destination: ColorSpace,
) -> [u8; 4] {
  if source == destination {
    return pixel;
  }
  let encoded = [
    f64::from(pixel[0]) / 255.0,
    f64::from(pixel[1]) / 255.0,
    f64::from(pixel[2]) / 255.0,
  ];
  let converted = convert_encoded(source, destination, encoded);
  [
    quantize_u8(converted[0]),
    quantize_u8(converted[1]),
    quantize_u8(converted[2]),
    pixel[3],
  ]
}

fn quantize_u8(channel: f64) -> u8 {
  (channel * 255.0).round().clamp(0.0, 255.0) as u8
}

fn pack_rgb(format: PixelFormat, pixel: [u8; 4]) -> [u8; 4] {
  match format {
    PixelFormat::Bgra => [pixel[2], pixel[1], pixel[0], pixel[3]],
    PixelFormat::Bgrx => [pixel[2], pixel[1], pixel[0], 255],
    PixelFormat::Rgbx => [pixel[0], pixel[1], pixel[2], 255],
    _ => pixel,
  }
}

fn yuv_to_rgb(y: i32, u: i32, v: i32, limited: bool) -> (u8, u8, u8) {
  let d = u - 128;
  let e = v - 128;
  let (red, green, blue) = if limited {
    let c = y - 16;
    (
      (298 * c + 409 * e + 128) >> 8,
      (298 * c - 100 * d - 208 * e + 128) >> 8,
      (298 * c + 516 * d + 128) >> 8,
    )
  } else {
    (
      y + ((359 * e) >> 8),
      y - ((88 * d + 183 * e) >> 8),
      y + ((454 * d) >> 8),
    )
  };
  (clamp_u8(red), clamp_u8(green), clamp_u8(blue))
}

fn clamp_u8(value: i32) -> u8 {
  value.clamp(0, 255) as u8
}

fn fill_channel(planes: &mut [Vec<u8>], channel: usize) {
  if let Some(plane) = planes.first_mut() {
    for pixel in plane.chunks_exact_mut(4) {
      pixel[channel] = 255;
    }
  }
}

pub(crate) fn rgba_plane(raster: &Raster) -> Vec<u8> {
  let mut owned = raster.clone();
  if owned.premultiplied {
    unpremultiply(&mut owned);
  }
  match owned.format {
    SampleFormat::Unorm8 => owned.bytes,
    SampleFormat::Float16 => {
      let mut bytes =
        Vec::with_capacity(owned.width as usize * owned.height as usize * 4);
      for y in 0..owned.height {
        for x in 0..owned.width {
          bytes.extend_from_slice(&owned.pixel_u8(x, y));
        }
      }
      bytes
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn i420() -> FrameData {
    let data = [
      1, 2, 3, 4, 5, 6, 7, 8, // y
      9, 10, // u
      11, 12, // v
    ];
    let pixels =
      read_buffer(&data, PixelFormat::I420, 4, 2, full_rect(4, 2), None)
        .unwrap();
    let color = FrameColor::rec709();
    let raster = visible_raster(
      PixelFormat::I420,
      &pixels.planes,
      pixels.coded_width,
      pixels.coded_height,
      full_rect(pixels.coded_width, pixels.coded_height),
      ColorSpace::Srgb,
      true,
    )
    .unwrap();
    FrameData {
      format: PixelFormat::I420,
      coded_width: pixels.coded_width,
      coded_height: pixels.coded_height,
      visible_x: 0.0,
      visible_y: 0.0,
      visible_width: 4.0,
      visible_height: 2.0,
      display_width: 4,
      display_height: 2,
      color,
      planes: pixels.planes,
      raster,
    }
  }

  #[test]
  fn i420_full_copy_is_tight() {
    let frame = i420();
    let (size, layout) =
      allocation(frame.format, full_rect(4, 2), None).unwrap();
    assert_eq!(size, 12);
    assert_eq!(
      layout,
      vec![
        PlaneLayoutOut {
          offset: 0,
          stride: 4
        },
        PlaneLayoutOut {
          offset: 8,
          stride: 2
        },
        PlaneLayoutOut {
          offset: 10,
          stride: 2
        },
      ]
    );
    let mut dest = vec![0u8; 12];
    write_copy(
      &frame,
      full_rect(4, 2),
      frame.format,
      None,
      true,
      None,
      &mut dest,
    )
    .unwrap();
    assert_eq!(dest, [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]);
  }

  #[test]
  fn i420_left_crop_matches_wpt() {
    let frame = i420();
    let rect = RectD {
      x: 2.0,
      y: 0.0,
      width: 2.0,
      height: 2.0,
    };
    let (size, layout) = allocation(frame.format, rect, None).unwrap();
    assert_eq!(size, 6);
    assert_eq!(
      layout[0],
      PlaneLayoutOut {
        offset: 0,
        stride: 2
      }
    );
    assert_eq!(
      layout[1],
      PlaneLayoutOut {
        offset: 4,
        stride: 1
      }
    );
    assert_eq!(
      layout[2],
      PlaneLayoutOut {
        offset: 5,
        stride: 1
      }
    );
    let mut dest = vec![0u8; size as usize];
    write_copy(&frame, rect, frame.format, None, true, None, &mut dest)
      .unwrap();
    assert_eq!(dest, [3, 4, 7, 8, 10, 12]);
  }

  #[test]
  fn custom_layout_keeps_padding() {
    let frame = i420();
    let layout = [
      PlaneLayoutIn {
        offset: 9,
        stride: 5,
      },
      PlaneLayoutIn {
        offset: 1,
        stride: 3,
      },
      PlaneLayoutIn {
        offset: 5,
        stride: 3,
      },
    ];
    let (size, _) =
      allocation(frame.format, full_rect(4, 2), Some(&layout)).unwrap();
    assert_eq!(size, 19);
    let mut dest = vec![0u8; size as usize];
    write_copy(
      &frame,
      full_rect(4, 2),
      frame.format,
      Some(&layout),
      true,
      None,
      &mut dest,
    )
    .unwrap();
    assert_eq!(dest[1], 9);
    assert_eq!(dest[2], 10);
    assert_eq!(dest[3], 0);
    assert_eq!(&dest[9..13], &[1, 2, 3, 4]);
    assert_eq!(dest[13], 0);
    assert_eq!(&dest[14..18], &[5, 6, 7, 8]);
  }

  #[test]
  fn odd_i420_is_stored() {
    let data = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17];
    let pixels =
      read_buffer(&data, PixelFormat::I420, 3, 3, full_rect(3, 3), None)
        .unwrap();
    assert_eq!(pixels.coded_width, 3);
    assert_eq!(pixels.planes[0].len(), 9);
    assert_eq!(pixels.planes[1].len(), 4);
    assert_eq!(pixels.planes[2].len(), 4);
  }

  #[test]
  fn visible_rect_crops_buffer_to_origin() {
    let mut data = vec![0u8; 8 * 16 + 2 * (4 * 8)];
    data[2 * 8 + 2] = 76;
    let visible = RectD {
      x: 2.0,
      y: 2.0,
      width: 4.0,
      height: 8.0,
    };
    let pixels =
      read_buffer(&data, PixelFormat::I420, 8, 16, visible, None).unwrap();
    assert_eq!(pixels.coded_width, 4);
    assert_eq!(pixels.coded_height, 8);
    assert_eq!(pixels.planes[0][0], 76);
  }

  #[test]
  fn misaligned_origin_fails() {
    let error = parse_visible(
      full_rect(4, 2),
      Some(RectD {
        x: 1.0,
        y: 0.0,
        width: 2.0,
        height: 2.0,
      }),
      4,
      2,
      PixelFormat::I420,
    );
    assert_eq!(error, Err(LayoutError::Alignment));
  }

  #[test]
  fn anamorphic_display_scales() {
    assert_eq!(display_size(2.0, 2.0, 4.0, 2.0, 8.0, 2.0).unwrap(), (4, 2));
    assert_eq!(
      display_size(720.0, 540.0, 1440.0, 1080.0, 1920.0, 1080.0).unwrap(),
      (960, 540)
    );
    assert_eq!(
      display_size(320.0, 240.0, 640.0, 480.0, 160.0, 480.0).unwrap(),
      (80, 240)
    );
    assert!(display_size(187.0, 40.0, 300.0, 150.0, 1280.0, 1.0).is_err());
  }

  fn rgb_frame(
    format: PixelFormat,
    bytes: &[u8],
    width: u32,
    height: u32,
  ) -> FrameData {
    let pixels =
      read_buffer(bytes, format, width, height, full_rect(width, height), None)
        .unwrap();
    let color = FrameColor::srgb();
    let raster = visible_raster(
      format,
      &pixels.planes,
      pixels.coded_width,
      pixels.coded_height,
      full_rect(pixels.coded_width, pixels.coded_height),
      ColorSpace::Srgb,
      false,
    )
    .unwrap();
    FrameData {
      format,
      coded_width: pixels.coded_width,
      coded_height: pixels.coded_height,
      visible_x: 0.0,
      visible_y: 0.0,
      visible_width: f64::from(width),
      visible_height: f64::from(height),
      display_width: width,
      display_height: height,
      color,
      planes: pixels.planes,
      raster,
    }
  }

  #[test]
  fn rgbx_copy_writes_opaque_padding() {
    let frame =
      rgb_frame(PixelFormat::Rgbx, &[255, 0, 0, 0, 0, 255, 0, 0x40], 2, 1);
    let mut dest = vec![0u8; 8];
    write_copy(
      &frame,
      full_rect(2, 1),
      PixelFormat::Rgbx,
      None,
      false,
      None,
      &mut dest,
    )
    .unwrap();
    assert_eq!(dest, [255, 0, 0, 255, 0, 255, 0, 255]);
  }

  #[test]
  fn rgba_copy_converts_srgb_to_display_p3() {
    let frame = rgb_frame(PixelFormat::Rgba, &[50, 100, 150, 255], 1, 1);
    let mut same = [0u8; 4];
    write_copy(
      &frame,
      full_rect(1, 1),
      PixelFormat::Rgba,
      None,
      false,
      Some(ColorSpace::Srgb),
      &mut same,
    )
    .unwrap();
    assert_eq!(same, [50, 100, 150, 255]);
    let mut p3 = [0u8; 4];
    write_copy(
      &frame,
      full_rect(1, 1),
      PixelFormat::Rgba,
      None,
      false,
      Some(ColorSpace::DisplayP3),
      &mut p3,
    )
    .unwrap();
    assert_eq!(p3, [62, 99, 146, 255]);
  }

  #[test]
  fn layout_rejects_plane_count_and_overflow() {
    let short = [PlaneLayoutIn {
      offset: 0,
      stride: 4,
    }];
    assert_eq!(
      allocation(PixelFormat::I420, full_rect(4, 2), Some(&short)),
      Err(LayoutError::PlaneCount)
    );
    let overflow = [
      PlaneLayoutIn {
        offset: 0,
        stride: 4,
      },
      PlaneLayoutIn {
        offset: 8,
        stride: 2,
      },
      PlaneLayoutIn {
        offset: u64::from(u32::MAX) - 1,
        stride: 2,
      },
    ];
    assert_eq!(
      allocation(PixelFormat::I420, full_rect(4, 2), Some(&overflow)),
      Err(LayoutError::Overflow)
    );
  }

  #[test]
  fn stride_too_small_and_short_buffer() {
    let layout = [
      PlaneLayoutIn {
        offset: 0,
        stride: 1,
      },
      PlaneLayoutIn {
        offset: 8,
        stride: 2,
      },
      PlaneLayoutIn {
        offset: 10,
        stride: 2,
      },
    ];
    assert_eq!(
      allocation(PixelFormat::I420, full_rect(4, 2), Some(&layout)),
      Err(LayoutError::Stride)
    );
    let data = [0u8; 8];
    assert_eq!(
      read_buffer(&data, PixelFormat::Nv12, 4, 2, full_rect(4, 2), None),
      Err(LayoutError::TooSmall)
    );
  }
}
