//! Software AV1 via rav1d, without assembly.
//!
//! H.264 stays on the platform decoder. Video and AVIF accept 8, 10, and 12
//! bits at 4:2:0, 4:2:2, 4:4:4, and monochrome. An AVIF alpha item is
//! composited into the display image. Film grain is applied.

use std::ffi::c_int;
use std::ffi::c_void;
use std::io::Cursor;
use std::mem::MaybeUninit;
use std::ptr;
use std::ptr::NonNull;

use rav1d::Dav1dResult;
use rav1d::include::dav1d::data::Dav1dData;
use rav1d::include::dav1d::dav1d::Dav1dContext;
use rav1d::include::dav1d::dav1d::Dav1dLogger;
use rav1d::include::dav1d::dav1d::Dav1dSettings;
use rav1d::include::dav1d::headers::DAV1D_PIXEL_LAYOUT_I400;
use rav1d::include::dav1d::headers::DAV1D_PIXEL_LAYOUT_I420;
use rav1d::include::dav1d::headers::DAV1D_PIXEL_LAYOUT_I422;
use rav1d::include::dav1d::headers::DAV1D_PIXEL_LAYOUT_I444;
use rav1d::include::dav1d::picture::Dav1dPicture;
use rav1d::src::lib::dav1d_close;
use rav1d::src::lib::dav1d_data_create;
use rav1d::src::lib::dav1d_data_unref;
use rav1d::src::lib::dav1d_default_settings;
use rav1d::src::lib::dav1d_flush;
use rav1d::src::lib::dav1d_get_picture;
use rav1d::src::lib::dav1d_open;
use rav1d::src::lib::dav1d_picture_unref;
use rav1d::src::lib::dav1d_send_data;

use crate::raster::ColorSpace;
use crate::raster::Raster;
use crate::raster::within_limits;

const MAX_PICTURES: usize = 64;

#[derive(Debug)]
pub enum Av1Error {
  Unsupported,
  Corrupt,
  TooLarge,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Av1PixelFormat {
  I420,
  I420P10,
  I420P12,
  I422,
  I422P10,
  I422P12,
  I444,
  I444P10,
  I444P12,
}

impl Av1PixelFormat {
  pub fn name(self) -> &'static str {
    match self {
      Self::I420 => "I420",
      Self::I420P10 => "I420P10",
      Self::I420P12 => "I420P12",
      Self::I422 => "I422",
      Self::I422P10 => "I422P10",
      Self::I422P12 => "I422P12",
      Self::I444 => "I444",
      Self::I444P10 => "I444P10",
      Self::I444P12 => "I444P12",
    }
  }
}

/// One rav1d picture. Planes are tight. 10-bit and 12-bit samples are
/// little-endian, and the low bits are significant. `display` is the unorm8
/// sRGB image used by `drawImage`.
#[derive(Clone)]
pub struct Av1Picture {
  pub format: Av1PixelFormat,
  pub width: u32,
  pub height: u32,
  pub planes: Vec<Vec<u8>>,
  pub primaries: u8,
  pub transfer: u8,
  pub matrix: u8,
  pub full_range: bool,
  pub display: Raster,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Matrix {
  Bt709,
  Bt601,
  Identity,
}

/// CICP from an AVIF `colr` box whose colour type is `nclx`.
#[derive(Clone, Copy)]
struct Nclx {
  primaries: u8,
  transfer: u8,
  matrix: u8,
  full_range: bool,
}

#[derive(Clone, Copy)]
enum Mode {
  /// Luma used as an alpha plane, at 8, 10, or 12 bits.
  Alpha,
  /// 8, 10, or 12 bits at 4:2:0, 4:2:2, 4:4:4, or monochrome.
  Video,
}

enum Frame {
  Alpha {
    width: u32,
    height: u32,
    samples: Vec<u8>,
  },
  Video(Av1Picture),
}

/// One still AVIF. `picture.display` includes a composited alpha item.
pub(crate) struct AvifDecoded {
  pub picture: Av1Picture,
}

pub struct Av1Session {
  ctx: Option<Dav1dContext>,
}

impl Av1Session {
  pub fn open() -> Result<Self, Av1Error> {
    unsafe {
      let mut settings = MaybeUninit::<Dav1dSettings>::uninit();
      dav1d_default_settings(NonNull::from(&mut settings).cast());
      let settings = settings.assume_init_mut();
      let threads = std::thread::available_parallelism()
        .map(|count| count.get())
        .unwrap_or(1)
        .min(256);
      settings.n_threads = c_int::try_from(threads).unwrap_or(1);
      settings.max_frame_delay = 1;
      settings.apply_grain = 1;
      settings.frame_size_limit = 100_000_000;
      settings.logger = Dav1dLogger::new(None, None);
      let mut ctx = None;
      let result = dav1d_open(
        Some(NonNull::from(&mut ctx)),
        Some(NonNull::from(settings)),
      );
      if result.0 != 0 || ctx.is_none() {
        return Err(Av1Error::Corrupt);
      }
      Ok(Self { ctx })
    }
  }

  pub fn send(&mut self, bytes: &[u8]) -> Result<Vec<Av1Picture>, Av1Error> {
    video_pictures(self.push(bytes, Mode::Video)?)
  }

  pub fn flush(&mut self) -> Result<Vec<Av1Picture>, Av1Error> {
    let ctx = self.ctx.ok_or(Av1Error::Corrupt)?;
    unsafe { dav1d_flush(ctx) };
    video_pictures(self.drain(ctx, Mode::Video)?)
  }

  fn push(&mut self, bytes: &[u8], mode: Mode) -> Result<Vec<Frame>, Av1Error> {
    if bytes.is_empty() {
      return Err(Av1Error::Corrupt);
    }
    let ctx = self.ctx.ok_or(Av1Error::Corrupt)?;
    let mut out = Vec::new();
    unsafe {
      let mut data = DataGuard(Dav1dData::default());
      let buf =
        dav1d_data_create(Some(NonNull::from(&mut data.0)), bytes.len());
      let Some(buf) = NonNull::new(buf) else {
        return Err(Av1Error::Corrupt);
      };
      ptr::copy_nonoverlapping(bytes.as_ptr(), buf.as_ptr(), bytes.len());
      let mut accepted = false;
      for _ in 0..MAX_PICTURES {
        let result =
          dav1d_send_data(Some(ctx), Some(NonNull::from(&mut data.0)));
        if result.0 == 0 {
          accepted = true;
          break;
        }
        match self.pull_one(ctx, mode)? {
          Some(frame) => out.push(frame),
          None => return Err(Av1Error::Corrupt),
        }
      }
      if !accepted {
        return Err(Av1Error::Corrupt);
      }
      out.extend(self.drain(ctx, mode)?);
    }
    Ok(out)
  }

  fn drain(
    &mut self,
    ctx: Dav1dContext,
    mode: Mode,
  ) -> Result<Vec<Frame>, Av1Error> {
    let mut out = Vec::new();
    for _ in 0..MAX_PICTURES {
      match self.pull_one(ctx, mode)? {
        Some(frame) => out.push(frame),
        None => return Ok(out),
      }
    }
    Err(Av1Error::Corrupt)
  }

  fn pull_one(
    &mut self,
    ctx: Dav1dContext,
    mode: Mode,
  ) -> Result<Option<Frame>, Av1Error> {
    unsafe {
      let mut picture = PictureGuard(Dav1dPicture::default());
      let result =
        dav1d_get_picture(Some(ctx), Some(NonNull::from(&mut picture.0)));
      if result.0 != 0 {
        return if is_again(result) {
          Ok(None)
        } else {
          Err(Av1Error::Corrupt)
        };
      }
      picture_frame(&picture.0, mode).map(Some)
    }
  }
}

impl Drop for Av1Session {
  fn drop(&mut self) {
    unsafe { dav1d_close(Some(NonNull::from(&mut self.ctx))) };
  }
}

struct DataGuard(Dav1dData);

impl Drop for DataGuard {
  fn drop(&mut self) {
    unsafe { dav1d_data_unref(Some(NonNull::from(&mut self.0))) };
  }
}

struct PictureGuard(Dav1dPicture);

impl Drop for PictureGuard {
  fn drop(&mut self) {
    unsafe { dav1d_picture_unref(Some(NonNull::from(&mut self.0))) };
  }
}

fn video_pictures(frames: Vec<Frame>) -> Result<Vec<Av1Picture>, Av1Error> {
  frames
    .into_iter()
    .map(|frame| match frame {
      Frame::Video(picture) => Ok(picture),
      Frame::Alpha { .. } => Err(Av1Error::Corrupt),
    })
    .collect()
}

/// One still AVIF: the primary item, plus its alpha item when present.
pub(crate) fn decode_avif(bytes: &[u8]) -> Result<AvifDecoded, Av1Error> {
  let parsed = avif_parse::read_avif(&mut Cursor::new(bytes))
    .map_err(|_| Av1Error::Corrupt)?;
  let meta = parsed
    .primary_item_metadata()
    .map_err(|_| Av1Error::Corrupt)?;
  if !matches!(meta.bit_depth, 8 | 10 | 12) || meta.seq_profile > 2 {
    return Err(Av1Error::Unsupported);
  }
  if !within_limits(meta.max_frame_width.get(), meta.max_frame_height.get()) {
    return Err(Av1Error::TooLarge);
  }
  let mut session = Av1Session::open()?;
  let mut frames = session.push(&parsed.primary_item, Mode::Video)?;
  let Some(Frame::Video(mut picture)) = frames.pop() else {
    return Err(Av1Error::Corrupt);
  };
  drop(frames);
  if let Some(color) = container_nclx(bytes) {
    apply_container_color(&mut picture, color)?;
  }
  let Some(alpha_item) = parsed.alpha_item.as_ref() else {
    return Ok(AvifDecoded { picture });
  };
  if let Some(alpha_meta) = parsed
    .alpha_item_metadata()
    .map_err(|_| Av1Error::Corrupt)?
    && !matches!(alpha_meta.bit_depth, 8 | 10 | 12)
  {
    return Err(Av1Error::Unsupported);
  }
  let mut alpha_session = Av1Session::open()?;
  let mut alpha_frames = alpha_session.push(alpha_item, Mode::Alpha)?;
  let Frame::Alpha {
    width,
    height,
    samples,
  } = alpha_frames.pop().ok_or(Av1Error::Corrupt)?
  else {
    return Err(Av1Error::Corrupt);
  };
  if width != picture.width || height != picture.height {
    return Err(Av1Error::Corrupt);
  }
  apply_alpha(&mut picture.display, &samples, parsed.premultiplied_alpha)?;
  Ok(AvifDecoded { picture })
}

fn apply_alpha(
  raster: &mut Raster,
  alpha: &[u8],
  premultiplied: bool,
) -> Result<(), Av1Error> {
  let pixels = raster.width as usize * raster.height as usize;
  if alpha.len() != pixels || raster.bytes.len() != pixels * 4 {
    return Err(Av1Error::Corrupt);
  }
  for (pixel, &sample) in
    raster.bytes.as_chunks_mut::<4>().0.iter_mut().zip(alpha)
  {
    pixel[3] = sample;
  }
  raster.premultiplied = premultiplied;
  Ok(())
}

fn picture_frame(
  picture: &Dav1dPicture,
  mode: Mode,
) -> Result<Frame, Av1Error> {
  if picture.p.w <= 0 || picture.p.h <= 0 {
    return Err(Av1Error::Corrupt);
  }
  match mode {
    Mode::Video => video_picture(picture),
    Mode::Alpha => alpha_frame(picture),
  }
}

fn video_picture(picture: &Dav1dPicture) -> Result<Frame, Av1Error> {
  let bpc = picture.p.bpc;
  if bpc != 8 && bpc != 10 && bpc != 12 {
    return Err(Av1Error::Unsupported);
  }
  let width = u32::try_from(picture.p.w).map_err(|_| Av1Error::Corrupt)?;
  let height = u32::try_from(picture.p.h).map_err(|_| Av1Error::Corrupt)?;
  if !within_limits(width, height) {
    return Err(Av1Error::TooLarge);
  }
  let sample_bytes = if bpc == 8 { 1 } else { 2 };
  let (format, chroma_x, chroma_y, mono) = match picture.p.layout {
    DAV1D_PIXEL_LAYOUT_I400 => (format_for(bpc, 0)?, 2, 2, true),
    DAV1D_PIXEL_LAYOUT_I420 => (format_for(bpc, 0)?, 2, 2, false),
    DAV1D_PIXEL_LAYOUT_I422 => (format_for(bpc, 1)?, 2, 1, false),
    DAV1D_PIXEL_LAYOUT_I444 => (format_for(bpc, 2)?, 1, 1, false),
    _ => return Err(Av1Error::Unsupported),
  };
  let y =
    plane_bytes(picture, 0, width as usize, height as usize, sample_bytes)?;
  let chroma_w = (width as usize).div_ceil(chroma_x);
  let chroma_h = (height as usize).div_ceil(chroma_y);
  let (u, v) = if mono {
    (
      neutral(chroma_w, chroma_h, bpc),
      neutral(chroma_w, chroma_h, bpc),
    )
  } else {
    (
      plane_bytes(picture, 1, chroma_w, chroma_h, sample_bytes)?,
      plane_bytes(picture, 2, chroma_w, chroma_h, sample_bytes)?,
    )
  };
  let (matrix, full, primaries, transfer, coefficients) = color_of(picture);
  let display = display_raster(
    &y, &u, &v, width, height, chroma_x, chroma_y, bpc, matrix, full,
  )?;
  Ok(Frame::Video(Av1Picture {
    format,
    width,
    height,
    planes: vec![y, u, v],
    primaries,
    transfer,
    matrix: coefficients,
    full_range: full,
    display,
  }))
}

fn format_for(bpc: c_int, chroma: u8) -> Result<Av1PixelFormat, Av1Error> {
  Ok(match (chroma, bpc) {
    (0, 8) => Av1PixelFormat::I420,
    (0, 10) => Av1PixelFormat::I420P10,
    (0, 12) => Av1PixelFormat::I420P12,
    (1, 8) => Av1PixelFormat::I422,
    (1, 10) => Av1PixelFormat::I422P10,
    (1, 12) => Av1PixelFormat::I422P12,
    (2, 8) => Av1PixelFormat::I444,
    (2, 10) => Av1PixelFormat::I444P10,
    (2, 12) => Av1PixelFormat::I444P12,
    _ => return Err(Av1Error::Unsupported),
  })
}

fn neutral(width: usize, height: usize, bpc: c_int) -> Vec<u8> {
  let count = width.saturating_mul(height);
  if bpc <= 8 {
    return vec![128; count];
  }
  let value = (1u16 << bpc) / 2;
  let bytes = value.to_le_bytes();
  let mut plane = Vec::with_capacity(count * 2);
  for _ in 0..count {
    plane.extend_from_slice(&bytes);
  }
  plane
}

fn plane_bytes(
  picture: &Dav1dPicture,
  index: usize,
  width: usize,
  height: usize,
  sample_bytes: usize,
) -> Result<Vec<u8>, Av1Error> {
  let Some(data) = picture.data[index] else {
    return Err(Av1Error::Corrupt);
  };
  let stride = picture.stride[usize::from(index != 0)];
  let row_bytes = width.checked_mul(sample_bytes).ok_or(Av1Error::Corrupt)?;
  if width == 0
    || height == 0
    || stride == 0
    || stride.unsigned_abs() < row_bytes
  {
    return Err(Av1Error::Corrupt);
  }
  let mut samples = vec![0u8; row_bytes.saturating_mul(height)];
  for row in 0..height {
    let start = row * row_bytes;
    copy_row(data, stride, row, &mut samples[start..start + row_bytes])?;
  }
  Ok(samples)
}

fn color_of(picture: &Dav1dPicture) -> (Matrix, bool, u8, u8, u8) {
  let Some(header) = sequence(picture) else {
    return (Matrix::Bt709, false, 2, 2, 2);
  };
  let matrix = cicp(header.mtrx);
  (
    matrix_kind(matrix),
    header.color_range != 0,
    cicp(header.pri),
    cicp(header.trc),
    matrix,
  )
}

fn matrix_kind(coefficients: u8) -> Matrix {
  match coefficients {
    0 => Matrix::Identity,
    5 | 6 => Matrix::Bt601,
    _ => Matrix::Bt709,
  }
}

/// MIAF gives an `nclx` colour property on the primary item precedence over
/// the AV1 sequence header. A missing or unreadable property leaves the
/// header color in place. ICC profiles are ignored.
fn container_nclx(bytes: &[u8]) -> Option<Nclx> {
  let mut top = bytes;
  let mut meta = None;
  while let Some(box_) = next_box(&mut top) {
    if box_.name == *b"meta" {
      meta = Some(box_.payload);
      break;
    }
  }
  let meta = meta?;
  let mut body = meta.get(4..)?;
  let mut primary = None;
  let mut iprp = None;
  while let Some(box_) = next_box(&mut body) {
    if box_.name == *b"pitm" {
      primary = pitm_id(box_.payload);
    } else if box_.name == *b"iprp" {
      iprp = Some(box_.payload);
    }
  }
  let primary = primary?;
  let mut iprp = iprp?;
  let mut properties = Vec::new();
  let mut ipma = None;
  while let Some(box_) = next_box(&mut iprp) {
    if box_.name == *b"ipco" {
      let mut items = box_.payload;
      while let Some(prop) = next_box(&mut items) {
        let color = if prop.name == *b"colr" {
          parse_nclx(prop.payload)
        } else {
          None
        };
        properties.push(color);
      }
    } else if box_.name == *b"ipma" {
      ipma = Some(box_.payload);
    }
  }
  associated_nclx(ipma?, primary, &properties)
}

fn apply_container_color(
  picture: &mut Av1Picture,
  color: Nclx,
) -> Result<(), Av1Error> {
  let next = matrix_kind(color.matrix);
  let rebuild = next != matrix_kind(picture.matrix)
    || picture.full_range != color.full_range;
  picture.primaries = color.primaries;
  picture.transfer = color.transfer;
  picture.matrix = color.matrix;
  picture.full_range = color.full_range;
  if !rebuild {
    return Ok(());
  }
  let (chroma_x, chroma_y, bpc) = layout_of(picture.format);
  let display = {
    let [y, u, v] = picture.planes.as_slice() else {
      return Err(Av1Error::Corrupt);
    };
    display_raster(
      y,
      u,
      v,
      picture.width,
      picture.height,
      chroma_x,
      chroma_y,
      bpc,
      next,
      color.full_range,
    )?
  };
  picture.display = display;
  Ok(())
}

fn layout_of(format: Av1PixelFormat) -> (usize, usize, c_int) {
  match format {
    Av1PixelFormat::I420 => (2, 2, 8),
    Av1PixelFormat::I420P10 => (2, 2, 10),
    Av1PixelFormat::I420P12 => (2, 2, 12),
    Av1PixelFormat::I422 => (2, 1, 8),
    Av1PixelFormat::I422P10 => (2, 1, 10),
    Av1PixelFormat::I422P12 => (2, 1, 12),
    Av1PixelFormat::I444 => (1, 1, 8),
    Av1PixelFormat::I444P10 => (1, 1, 10),
    Av1PixelFormat::I444P12 => (1, 1, 12),
  }
}

struct IsoBox<'a> {
  name: [u8; 4],
  payload: &'a [u8],
}

fn next_box<'a>(rest: &mut &'a [u8]) -> Option<IsoBox<'a>> {
  if rest.len() < 8 {
    return None;
  }
  let mut size = u32::from_be_bytes(rest[..4].try_into().ok()?) as usize;
  let mut header = 8usize;
  if size == 1 {
    if rest.len() < 16 {
      return None;
    }
    let large = u64::from_be_bytes(rest[8..16].try_into().ok()?);
    size = usize::try_from(large).ok()?;
    header = 16;
  } else if size == 0 {
    size = rest.len();
  }
  if size < header || size > rest.len() {
    return None;
  }
  let name = rest[4..8].try_into().ok()?;
  let payload = &rest[header..size];
  *rest = &rest[size..];
  Some(IsoBox { name, payload })
}

fn pitm_id(payload: &[u8]) -> Option<u32> {
  let version = *payload.first()?;
  if version == 0 {
    let id = payload.get(4..6)?;
    Some(u32::from(u16::from_be_bytes(id.try_into().ok()?)))
  } else {
    let id = payload.get(4..8)?;
    Some(u32::from_be_bytes(id.try_into().ok()?))
  }
}

fn parse_nclx(payload: &[u8]) -> Option<Nclx> {
  if payload.len() < 11 || &payload[..4] != b"nclx" {
    return None;
  }
  Some(Nclx {
    primaries: cicp_wide(u16::from_be_bytes(payload[4..6].try_into().ok()?)),
    transfer: cicp_wide(u16::from_be_bytes(payload[6..8].try_into().ok()?)),
    matrix: cicp_wide(u16::from_be_bytes(payload[8..10].try_into().ok()?)),
    full_range: payload[10] & 0x80 != 0,
  })
}

fn cicp_wide(value: u16) -> u8 {
  u8::try_from(value).unwrap_or(2)
}

fn associated_nclx(
  payload: &[u8],
  primary: u32,
  properties: &[Option<Nclx>],
) -> Option<Nclx> {
  let version = *payload.first()?;
  let large = *payload.get(3)? & 1 == 1;
  let mut rest = payload.get(4..)?;
  let count = take_u32(&mut rest)?;
  for _ in 0..count {
    let item_id = if version == 0 {
      u32::from(take_u16(&mut rest)?)
    } else {
      take_u32(&mut rest)?
    };
    let associations = usize::from(take_u8(&mut rest)?);
    for _ in 0..associations {
      let index = if large {
        usize::from(take_u16(&mut rest)? & 0x7fff)
      } else {
        usize::from(take_u8(&mut rest)? & 0x7f)
      };
      if item_id == primary
        && index > 0
        && let Some(Some(color)) = properties.get(index - 1)
      {
        return Some(*color);
      }
    }
  }
  None
}

fn take_u8(rest: &mut &[u8]) -> Option<u8> {
  let (first, tail) = rest.split_first()?;
  *rest = tail;
  Some(*first)
}

fn take_u16(rest: &mut &[u8]) -> Option<u16> {
  let (head, tail) = rest.split_at_checked(2)?;
  *rest = tail;
  Some(u16::from_be_bytes(head.try_into().ok()?))
}

fn take_u32(rest: &mut &[u8]) -> Option<u32> {
  let (head, tail) = rest.split_at_checked(4)?;
  *rest = tail;
  Some(u32::from_be_bytes(head.try_into().ok()?))
}

fn cicp(value: u32) -> u8 {
  u8::try_from(value).unwrap_or(2)
}

fn display_raster(
  y: &[u8],
  u: &[u8],
  v: &[u8],
  width: u32,
  height: u32,
  chroma_x: usize,
  chroma_y: usize,
  bpc: c_int,
  matrix: Matrix,
  full: bool,
) -> Result<Raster, Av1Error> {
  let width = width as usize;
  let height = height as usize;
  let chroma_w = width.div_ceil(chroma_x);
  let mut rgba = vec![0u8; width * height * 4];
  for row in 0..height {
    for col in 0..width {
      let luma = read_sample(y, width, col, row, bpc)?;
      let chroma_col = col / chroma_x;
      let chroma_row = row / chroma_y;
      let cb = read_sample(u, chroma_w, chroma_col, chroma_row, bpc)?;
      let cr = read_sample(v, chroma_w, chroma_col, chroma_row, bpc)?;
      let (red, green, blue) =
        rgb(i32::from(luma), i32::from(cb), i32::from(cr), matrix, full);
      let offset = (row * width + col) * 4;
      rgba[offset] = red;
      rgba[offset + 1] = green;
      rgba[offset + 2] = blue;
      rgba[offset + 3] = 255;
    }
  }
  Raster::from_rgba8(width as u32, height as u32, ColorSpace::Srgb, &rgba)
    .map_err(|_| Av1Error::TooLarge)
}

fn read_sample(
  plane: &[u8],
  stride: usize,
  x: usize,
  y: usize,
  bpc: c_int,
) -> Result<u8, Av1Error> {
  if bpc <= 8 {
    return plane.get(y * stride + x).copied().ok_or(Av1Error::Corrupt);
  }
  let index = (y * stride + x) * 2;
  let bytes = plane.get(index..index + 2).ok_or(Av1Error::Corrupt)?;
  let value = u16::from_le_bytes([bytes[0], bytes[1]]);
  let shift = u32::try_from(bpc - 8).map_err(|_| Av1Error::Corrupt)?;
  Ok((value >> shift) as u8)
}

fn alpha_frame(picture: &Dav1dPicture) -> Result<Frame, Av1Error> {
  let bpc = picture.p.bpc;
  if bpc != 8 && bpc != 10 && bpc != 12 {
    return Err(Av1Error::Unsupported);
  }
  let width = u32::try_from(picture.p.w).map_err(|_| Av1Error::Corrupt)?;
  let height = u32::try_from(picture.p.h).map_err(|_| Av1Error::Corrupt)?;
  if !within_limits(width, height) {
    return Err(Av1Error::TooLarge);
  }
  if picture.p.layout != DAV1D_PIXEL_LAYOUT_I400
    && picture.p.layout != DAV1D_PIXEL_LAYOUT_I420
    && picture.p.layout != DAV1D_PIXEL_LAYOUT_I422
    && picture.p.layout != DAV1D_PIXEL_LAYOUT_I444
  {
    return Err(Av1Error::Unsupported);
  }
  let full = sequence(picture).is_some_and(|header| header.color_range != 0);
  let samples = alpha_luma(picture, width, height, bpc, full)?;
  Ok(Frame::Alpha {
    width,
    height,
    samples,
  })
}

fn alpha_luma(
  picture: &Dav1dPicture,
  width: u32,
  height: u32,
  bpc: c_int,
  full: bool,
) -> Result<Vec<u8>, Av1Error> {
  let width = width as usize;
  let height = height as usize;
  if bpc <= 8 {
    let y = plane(picture, 0, width, height)?;
    let mut samples = Vec::with_capacity(y.samples.len());
    for row in 0..y.height {
      let start = row * y.stride;
      for sample in &y.samples[start..start + y.width] {
        samples.push(expand_luma(*sample, full));
      }
    }
    return Ok(samples);
  }
  let plane = plane_bytes(picture, 0, width, height, 2)?;
  let shift = u32::try_from(bpc - 8).map_err(|_| Av1Error::Corrupt)?;
  let mut samples = Vec::with_capacity(width * height);
  for row in 0..height {
    for col in 0..width {
      let index = (row * width + col) * 2;
      let bytes = plane.get(index..index + 2).ok_or(Av1Error::Corrupt)?;
      let value = u16::from_le_bytes([bytes[0], bytes[1]]);
      samples.push(expand_luma((value >> shift) as u8, full));
    }
  }
  Ok(samples)
}

struct Plane {
  samples: Vec<u8>,
  width: usize,
  height: usize,
  stride: usize,
}

fn plane(
  picture: &Dav1dPicture,
  index: usize,
  width: usize,
  height: usize,
) -> Result<Plane, Av1Error> {
  let Some(data) = picture.data[index] else {
    return Err(Av1Error::Corrupt);
  };
  let stride = picture.stride[usize::from(index != 0)];
  if width == 0 || height == 0 || stride == 0 || stride.unsigned_abs() < width {
    return Err(Av1Error::Corrupt);
  }
  let mut samples = vec![0u8; width * height];
  for row in 0..height {
    let start = row * width;
    copy_row(data, stride, row, &mut samples[start..start + width])?;
  }
  Ok(Plane {
    samples,
    width,
    height,
    stride: width,
  })
}

fn copy_row(
  data: NonNull<c_void>,
  stride: isize,
  row: usize,
  dest: &mut [u8],
) -> Result<(), Av1Error> {
  let width = dest.len();
  let row = isize::try_from(row).map_err(|_| Av1Error::Corrupt)?;
  let offset = stride.checked_mul(row).ok_or(Av1Error::Corrupt)?;
  // SAFETY: `data` is the top-left sample of a rav1d plane that is still
  // referenced. Row `row` starts at `data + stride * row` and holds `width`
  // bytes. The copy finishes before the picture is unreferenced.
  let ptr = unsafe { data.as_ptr().cast::<u8>().offset(offset) };
  if ptr.is_null() {
    return Err(Av1Error::Corrupt);
  }
  dest.copy_from_slice(unsafe { std::slice::from_raw_parts(ptr, width) });
  Ok(())
}

fn sequence(
  picture: &Dav1dPicture,
) -> Option<&rav1d::include::dav1d::headers::Dav1dSequenceHeader> {
  let header = picture.seq_hdr?;
  // SAFETY: the sequence header lives until `dav1d_picture_unref`.
  Some(unsafe { header.as_ref() })
}

fn rgb(y: i32, u: i32, v: i32, matrix: Matrix, full: bool) -> (u8, u8, u8) {
  let d = u - 128;
  let e = v - 128;
  let (red, green, blue) = match (matrix, full) {
    (Matrix::Bt709, false) => {
      let c = y - 16;
      (
        (298 * c + 459 * e + 128) >> 8,
        (298 * c - 55 * d - 136 * e + 128) >> 8,
        (298 * c + 541 * d + 128) >> 8,
      )
    }
    (Matrix::Bt709, true) => (
      y + ((403 * e) >> 8),
      y - ((48 * d + 120 * e) >> 8),
      y + ((475 * d) >> 8),
    ),
    (Matrix::Bt601, false) => {
      let c = y - 16;
      (
        (298 * c + 409 * e + 128) >> 8,
        (298 * c - 100 * d - 208 * e + 128) >> 8,
        (298 * c + 516 * d + 128) >> 8,
      )
    }
    (Matrix::Bt601, true) => (
      y + ((359 * e) >> 8),
      y - ((88 * d + 183 * e) >> 8),
      y + ((454 * d) >> 8),
    ),
    // AV1 identity stores G in Y, B in U, and R in V.
    (Matrix::Identity, false) => (
      i32::from(expand_chroma(v as u8)),
      i32::from(expand_luma(y as u8, false)),
      i32::from(expand_chroma(u as u8)),
    ),
    (Matrix::Identity, true) => (v, y, u),
  };
  (clamp(red), clamp(green), clamp(blue))
}

fn expand_luma(sample: u8, full: bool) -> u8 {
  if full {
    return sample;
  }
  let centered = i32::from(sample) - 16;
  let scaled = centered.clamp(0, 219) * 255 + 109;
  (scaled / 219) as u8
}

fn expand_chroma(sample: u8) -> u8 {
  let centered = i32::from(sample) - 16;
  let scaled = centered.clamp(0, 224) * 255 + 112;
  (scaled / 224) as u8
}

fn clamp(value: i32) -> u8 {
  value.clamp(0, 255) as u8
}

fn is_again(result: Dav1dResult) -> bool {
  result.0 == -libc::EAGAIN
}

#[cfg(test)]
mod tests {
  use super::Matrix;
  use super::rgb;

  #[test]
  fn limited_bt709_red_rounds_near_255() {
    let (red, green, blue) = rgb(81, 90, 240, Matrix::Bt709, false);
    assert_eq!((red, green, blue), (255, 24, 0));
  }

  #[test]
  fn container_color_rebuilds_when_the_matrix_changes() {
    let bytes = std::fs::read(concat!(
      env!("CARGO_MANIFEST_DIR"),
      "/../../tests/fixtures/images/red.avif"
    ))
    .unwrap();
    let mut image = super::decode_avif(&bytes).unwrap();
    let before = image.picture.display.bytes.clone();
    super::apply_container_color(
      &mut image.picture,
      super::Nclx {
        primaries: 1,
        transfer: 13,
        matrix: 0,
        full_range: true,
      },
    )
    .unwrap();
    assert_eq!(image.picture.matrix, 0);
    assert!(image.picture.full_range);
    assert_ne!(image.picture.display.bytes, before);
  }

  #[test]
  fn identity_planes_are_gbr() {
    let (red, green, blue) = rgb(255, 0, 255, Matrix::Identity, true);
    assert_eq!((red, green, blue), (255, 255, 0));
  }

  #[test]
  fn red_avif_is_a_red_square() {
    let bytes = std::fs::read(concat!(
      env!("CARGO_MANIFEST_DIR"),
      "/../../tests/fixtures/images/red.avif"
    ))
    .unwrap();
    let image = super::decode_avif(&bytes).unwrap();
    let raster = &image.picture.display;
    assert_eq!(image.picture.format, super::Av1PixelFormat::I420);
    assert_eq!(image.picture.primaries, 2);
    assert_eq!(image.picture.transfer, 2);
    assert_eq!(image.picture.matrix, 2);
    assert!(!image.picture.full_range);
    assert_eq!((raster.width, raster.height), (16, 16));
    assert!(!raster.premultiplied);
    let pixel = &raster.bytes[8 * 16 * 4 + 8 * 4..];
    assert!(pixel[0] > 180, "red {}", pixel[0]);
    assert!(pixel[1] < 80, "green {}", pixel[1]);
    assert!(pixel[2] < 80, "blue {}", pixel[2]);
    assert_eq!(pixel[3], 255);
  }

  #[test]
  fn avif_still_matches_chrome_formats() {
    let cases = [
      ("four-colors.avif", "I444", true),
      ("four-colors-limited-range-420-8bpc.avif", "I420", true),
      ("four-colors-limited-range-422-8bpc.avif", "I422", true),
      ("four-colors-limited-range-444-8bpc.avif", "I444", true),
      (
        "four-colors-full-range-hlg-420-10bpc.avif",
        "I420P10",
        false,
      ),
      (
        "four-colors-full-range-hlg-422-10bpc.avif",
        "I422P10",
        false,
      ),
      (
        "four-colors-full-range-hlg-444-10bpc.avif",
        "I444P10",
        false,
      ),
      (
        "four-colors-full-range-hlg-420-12bpc.avif",
        "I420P12",
        false,
      ),
      (
        "four-colors-full-range-hlg-422-12bpc.avif",
        "I422P12",
        false,
      ),
      (
        "four-colors-full-range-hlg-444-12bpc.avif",
        "I444P12",
        false,
      ),
      (
        "four-colors-full-range-bt2020-pq-444-10bpc.avif",
        "I444P10",
        false,
      ),
    ];
    for (name, format, corners) in cases {
      let path = format!(
        "{}/../../tests/wpt/suite/webcodecs/{name}",
        env!("CARGO_MANIFEST_DIR")
      );
      let bytes = std::fs::read(&path).unwrap();
      let image = super::decode_avif(&bytes).unwrap();
      assert_eq!(image.picture.format.name(), format, "{name}");
      assert_eq!(
        (image.picture.width, image.picture.height),
        (320, 240),
        "{name}"
      );
      if name == "four-colors.avif" {
        assert_eq!(image.picture.primaries, 1, "{name}");
        assert_eq!(image.picture.transfer, 13, "{name}");
        assert_eq!(image.picture.matrix, 0, "{name}");
        assert!(image.picture.full_range, "{name}");
      }
      if name.contains("hlg") {
        assert_eq!(image.picture.primaries, 9, "{name}");
        assert_eq!(image.picture.transfer, 18, "{name}");
        assert_eq!(image.picture.matrix, 9, "{name}");
        assert!(image.picture.full_range, "{name}");
      }
      if name.contains("pq") {
        assert_eq!(image.picture.primaries, 9, "{name}");
        assert_eq!(image.picture.transfer, 16, "{name}");
        assert_eq!(image.picture.matrix, 9, "{name}");
        assert!(image.picture.full_range, "{name}");
      }
      if corners {
        assert_corners(&image.picture.display, name);
      }
    }
  }

  fn assert_corners(raster: &super::Raster, name: &str) {
    assert_eq!((raster.width, raster.height), (320, 240), "{name}");
    let pixel = |x: u32, y: u32| {
      let index = ((y * raster.width + x) * 4) as usize;
      [
        raster.bytes[index],
        raster.bytes[index + 1],
        raster.bytes[index + 2],
        raster.bytes[index + 3],
      ]
    };
    let near = |actual: [u8; 4], expected: [u8; 4], corner: &str| {
      for (got, want) in actual.into_iter().zip(expected) {
        assert!(
          got.abs_diff(want) <= 3,
          "{name} {corner} {actual:?} vs {expected:?}"
        );
      }
    };
    near(pixel(0, 0), [255, 255, 0, 255], "yellow");
    near(pixel(319, 0), [255, 0, 0, 255], "red");
    near(pixel(0, 239), [0, 0, 255, 255], "blue");
    near(pixel(319, 239), [0, 255, 0, 255], "green");
  }
}
