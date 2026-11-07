//! Software AV1 via rav1d, without assembly and without 16-bit pixels.
//!
//! H.264 stays on the platform decoder. This session is 8-bit 4:2:0 for
//! video and for an AVIF color item. An AVIF alpha item may be monochrome.
//! Film grain is applied. Pixels are unorm8 sRGB.

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
use rav1d::include::dav1d::headers::DAV1D_MC_BT470BG;
use rav1d::include::dav1d::headers::DAV1D_MC_BT601;
use rav1d::include::dav1d::headers::DAV1D_MC_IDENTITY;
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

#[derive(Clone, Copy)]
enum Matrix {
  Bt709,
  Bt601,
}

#[derive(Clone, Copy)]
enum Mode {
  /// 8-bit 4:2:0, converted to opaque sRGB.
  Color,
  /// 8-bit luma used as an alpha plane.
  Alpha,
}

enum Frame {
  Color(Raster),
  Alpha {
    width: u32,
    height: u32,
    samples: Vec<u8>,
  },
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

  pub fn send(&mut self, bytes: &[u8]) -> Result<Vec<Raster>, Av1Error> {
    colors(self.push(bytes, Mode::Color)?)
  }

  pub fn flush(&mut self) -> Result<Vec<Raster>, Av1Error> {
    let ctx = self.ctx.ok_or(Av1Error::Corrupt)?;
    unsafe { dav1d_flush(ctx) };
    colors(self.drain(ctx, Mode::Color)?)
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

fn colors(frames: Vec<Frame>) -> Result<Vec<Raster>, Av1Error> {
  frames
    .into_iter()
    .map(|frame| match frame {
      Frame::Color(raster) => Ok(raster),
      Frame::Alpha { .. } => Err(Av1Error::Corrupt),
    })
    .collect()
}

/// One still AVIF: the primary item, plus its alpha item when present.
pub(crate) fn decode_avif(bytes: &[u8]) -> Result<Raster, Av1Error> {
  let parsed = avif_parse::read_avif(&mut Cursor::new(bytes))
    .map_err(|_| Av1Error::Corrupt)?;
  let meta = parsed
    .primary_item_metadata()
    .map_err(|_| Av1Error::Corrupt)?;
  if meta.bit_depth != 8
    || meta.monochrome
    || meta.seq_profile != 0
    || meta.chroma_subsampling != (true, true)
  {
    return Err(Av1Error::Unsupported);
  }
  if !within_limits(meta.max_frame_width.get(), meta.max_frame_height.get()) {
    return Err(Av1Error::TooLarge);
  }
  let mut session = Av1Session::open()?;
  let mut frames = session.push(&parsed.primary_item, Mode::Color)?;
  let color = match frames.pop() {
    Some(Frame::Color(raster)) => raster,
    _ => return Err(Av1Error::Corrupt),
  };
  drop(frames);
  let Some(alpha_item) = parsed.alpha_item.as_ref() else {
    return Ok(color);
  };
  if let Some(alpha_meta) = parsed
    .alpha_item_metadata()
    .map_err(|_| Av1Error::Corrupt)?
    && alpha_meta.bit_depth != 8
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
  if width != color.width || height != color.height {
    return Err(Av1Error::Corrupt);
  }
  apply_alpha(color, &samples, parsed.premultiplied_alpha)
}

fn apply_alpha(
  mut raster: Raster,
  alpha: &[u8],
  premultiplied: bool,
) -> Result<Raster, Av1Error> {
  let pixels = raster.width as usize * raster.height as usize;
  if alpha.len() != pixels || raster.bytes.len() != pixels * 4 {
    return Err(Av1Error::Corrupt);
  }
  for (pixel, &sample) in raster.bytes.chunks_exact_mut(4).zip(alpha) {
    pixel[3] = sample;
  }
  raster.premultiplied = premultiplied;
  Ok(raster)
}

fn picture_frame(
  picture: &Dav1dPicture,
  mode: Mode,
) -> Result<Frame, Av1Error> {
  if picture.p.bpc != 8 || picture.p.w <= 0 || picture.p.h <= 0 {
    return Err(if picture.p.bpc == 8 {
      Av1Error::Corrupt
    } else {
      Av1Error::Unsupported
    });
  }
  let width = u32::try_from(picture.p.w).map_err(|_| Av1Error::Corrupt)?;
  let height = u32::try_from(picture.p.h).map_err(|_| Av1Error::Corrupt)?;
  if !within_limits(width, height) {
    return Err(Av1Error::TooLarge);
  }
  match mode {
    Mode::Color => color_frame(picture, width, height),
    Mode::Alpha => alpha_frame(picture, width, height),
  }
}

fn color_frame(
  picture: &Dav1dPicture,
  width: u32,
  height: u32,
) -> Result<Frame, Av1Error> {
  if picture.p.layout != DAV1D_PIXEL_LAYOUT_I420 {
    return Err(Av1Error::Unsupported);
  }
  let (matrix, full) = matrix_of(picture)?;
  let y = plane(picture, 0, width as usize, height as usize)?;
  let chroma_w = (width as usize).div_ceil(2);
  let chroma_h = (height as usize).div_ceil(2);
  let u = plane(picture, 1, chroma_w, chroma_h)?;
  let v = plane(picture, 2, chroma_w, chroma_h)?;
  let rgba =
    yuv_to_rgba(&y, &u, &v, width as usize, height as usize, matrix, full)?;
  let raster = Raster::from_rgba8(width, height, ColorSpace::Srgb, &rgba)
    .map_err(|_| Av1Error::TooLarge)?;
  Ok(Frame::Color(raster))
}

fn alpha_frame(
  picture: &Dav1dPicture,
  width: u32,
  height: u32,
) -> Result<Frame, Av1Error> {
  if picture.p.layout != DAV1D_PIXEL_LAYOUT_I400
    && picture.p.layout != DAV1D_PIXEL_LAYOUT_I420
    && picture.p.layout != DAV1D_PIXEL_LAYOUT_I422
    && picture.p.layout != DAV1D_PIXEL_LAYOUT_I444
  {
    return Err(Av1Error::Unsupported);
  }
  let full = sequence(picture).is_some_and(|header| header.color_range != 0);
  let y = plane(picture, 0, width as usize, height as usize)?;
  let mut samples = Vec::with_capacity(y.samples.len());
  for row in 0..y.height {
    let start = row * y.stride;
    for sample in &y.samples[start..start + y.width] {
      samples.push(expand_luma(*sample, full));
    }
  }
  Ok(Frame::Alpha {
    width,
    height,
    samples,
  })
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

fn matrix_of(picture: &Dav1dPicture) -> Result<(Matrix, bool), Av1Error> {
  let Some(header) = sequence(picture) else {
    return Ok((Matrix::Bt709, false));
  };
  if header.mtrx == DAV1D_MC_IDENTITY {
    return Err(Av1Error::Unsupported);
  }
  let matrix =
    if header.mtrx == DAV1D_MC_BT470BG || header.mtrx == DAV1D_MC_BT601 {
      Matrix::Bt601
    } else {
      Matrix::Bt709
    };
  Ok((matrix, header.color_range != 0))
}

fn yuv_to_rgba(
  y: &Plane,
  u: &Plane,
  v: &Plane,
  width: usize,
  height: usize,
  matrix: Matrix,
  full: bool,
) -> Result<Vec<u8>, Av1Error> {
  let chroma_w = width.div_ceil(2);
  let chroma_h = height.div_ceil(2);
  if y.width != width
    || y.height != height
    || u.width < chroma_w
    || u.height < chroma_h
    || v.width < chroma_w
    || v.height < chroma_h
  {
    return Err(Av1Error::Corrupt);
  }
  let mut rgba = vec![0u8; width * height * 4];
  for row in 0..height {
    let y_row = &y.samples[row * y.stride..];
    let u_row = &u.samples[(row / 2) * u.stride..];
    let v_row = &v.samples[(row / 2) * v.stride..];
    for col in 0..width {
      let (red, green, blue) = rgb(
        i32::from(y_row[col]),
        i32::from(u_row[col / 2]),
        i32::from(v_row[col / 2]),
        matrix,
        full,
      );
      let offset = (row * width + col) * 4;
      rgba[offset] = red;
      rgba[offset + 1] = green;
      rgba[offset + 2] = blue;
      rgba[offset + 3] = 255;
    }
  }
  Ok(rgba)
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
  fn red_avif_is_a_red_square() {
    let bytes = std::fs::read(concat!(
      env!("CARGO_MANIFEST_DIR"),
      "/../../tests/fixtures/images/red.avif"
    ))
    .unwrap();
    let raster = super::decode_avif(&bytes).unwrap();
    assert_eq!((raster.width, raster.height), (16, 16));
    assert!(!raster.premultiplied);
    let pixel = &raster.bytes[8 * 16 * 4 + 8 * 4..];
    assert!(pixel[0] > 180, "red {}", pixel[0]);
    assert!(pixel[1] < 80, "green {}", pixel[1]);
    assert!(pixel[2] < 80, "blue {}", pixel[2]);
    assert_eq!(pixel[3], 255);
  }
}
