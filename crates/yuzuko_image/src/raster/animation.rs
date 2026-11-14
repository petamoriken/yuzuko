//! Frame sequences for `ImageDecoder`.
//!
//! Still images stay on the single-frame decoders. An animated PNG or GIF is
//! composited here. WebP and JPEG XL render each frame themselves.

use std::io::Cursor;

use crate::raster::Raster;

use super::{DecodeError, Decoded, Kind};

pub struct ImageFrame {
  pub raster: Raster,
  /// AVIF color planes. The display image is `raster`.
  pub yuv: Option<crate::av1::Av1Picture>,
  pub orientation: u8,
  /// Display time in microseconds. Absent for a still image.
  pub duration_us: Option<f64>,
}

pub struct ImageFrames {
  pub frames: Vec<ImageFrame>,
  pub animated: bool,
  /// Extra plays after the first. `Infinity` loops forever.
  pub repetition_count: f64,
}

pub(super) fn decode_all(
  bytes: &[u8],
  to_srgb: bool,
) -> Result<ImageFrames, DecodeError> {
  match super::sniff(bytes) {
    Some(Kind::Png) => decode_png_frames(bytes, to_srgb),
    Some(Kind::Jpeg) => still(super::decode_jpeg(bytes, to_srgb)),
    Some(Kind::Webp) => decode_webp_frames(bytes, to_srgb),
    Some(Kind::Jxl) => decode_jxl_frames(bytes, to_srgb),
    Some(Kind::Avif) => still(super::decode_avif(bytes, to_srgb)),
    Some(Kind::Gif) => decode_gif_frames(bytes, to_srgb),
    Some(Kind::Bmp) => still(super::decode_bmp(bytes, to_srgb)),
    None => Err(DecodeError::Unsupported),
  }
}

fn still(
  decoded: Result<Decoded, DecodeError>,
) -> Result<ImageFrames, DecodeError> {
  let decoded = decoded?;
  Ok(ImageFrames {
    frames: vec![ImageFrame {
      raster: decoded.raster,
      yuv: decoded.yuv,
      orientation: decoded.orientation,
      duration_us: None,
    }],
    animated: false,
    repetition_count: 0.0,
  })
}

fn decode_png_frames(
  bytes: &[u8],
  to_srgb: bool,
) -> Result<ImageFrames, DecodeError> {
  let mut decoder = png::Decoder::new(Cursor::new(bytes));
  decoder.set_limits(png::Limits { bytes: 800_000_000 });
  decoder.set_transformations(png::Transformations::EXPAND);
  let mut reader = decoder.read_info().map_err(|_| DecodeError::Corrupt)?;
  if reader.info().animation_control().is_none() {
    drop(reader);
    return still(super::decode_png(bytes, to_srgb));
  }
  let (canvas_w, canvas_h, cicp, icc, srgb) = {
    let info = reader.info();
    if !crate::raster::within_limits(info.width, info.height) {
      return Err(DecodeError::TooLarge);
    }
    let cicp = info
      .coding_independent_code_points
      .map(|points| super::Cicp {
        primaries: points.color_primaries,
        transfer: points.transfer_function,
      });
    let icc = info
      .icc_profile
      .as_ref()
      .map(|profile| profile.clone().into_owned());
    (info.width, info.height, cicp, icc, info.srgb.is_some())
  };
  let plays = reader
    .info()
    .animation_control()
    .map(|control| control.num_plays)
    .unwrap_or(0);
  let extra_default = reader.info().frame_control().is_none();
  let count = reader
    .info()
    .animation_control()
    .map(|control| control.num_frames)
    .unwrap_or(0)
    .saturating_add(u32::from(extra_default));
  let interlaced = reader.info().interlaced;
  let orientation = super::exif_orientation(bytes);
  let cicp = if srgb {
    Some(super::Cicp {
      primaries: 1,
      transfer: 13,
    })
  } else {
    cicp
  };
  let icc = if srgb { None } else { icc };
  let size = reader.output_buffer_size().ok_or(DecodeError::TooLarge)?;
  let mut buffer = vec![0u8; size];
  let mut canvas = PngCanvas::new(canvas_w, canvas_h)?;
  let mut frames = Vec::new();
  let mut pending: Option<PendingDispose> = None;
  for _ in 0..count {
    let output = reader
      .next_frame(&mut buffer)
      .map_err(|_| DecodeError::Corrupt)?;
    let Some(control) = reader.info().frame_control().copied() else {
      continue;
    };
    if output.width != control.width || output.height != control.height {
      return Err(DecodeError::Corrupt);
    }
    if interlaced
      && (control.x_offset != 0
        || control.y_offset != 0
        || control.width != canvas_w
        || control.height != canvas_h)
    {
      return Err(DecodeError::Corrupt);
    }
    if !rect_inside(
      control.x_offset,
      control.y_offset,
      control.width,
      control.height,
      canvas_w,
      canvas_h,
    ) {
      return Err(DecodeError::Corrupt);
    }
    let first = frames.is_empty();
    let mut dispose = control.dispose_op;
    if first && dispose == png::DisposeOp::Previous {
      dispose = png::DisposeOp::Background;
    }
    canvas.apply_dispose(pending)?;
    if dispose == png::DisposeOp::Previous {
      canvas.snapshot_previous();
    }
    let pixels = &buffer[..output.buffer_size()];
    canvas.blend(&output, pixels, &control)?;
    let mut raster = canvas.raster()?;
    let decoded =
      super::finish(&mut raster, icc.as_deref(), cicp, to_srgb, orientation)?;
    frames.push(ImageFrame {
      raster: decoded.raster,
      yuv: None,
      orientation: decoded.orientation,
      duration_us: Some(png_delay_us(control.delay_num, control.delay_den)),
    });
    pending = Some(PendingDispose {
      op: dispose,
      x: control.x_offset,
      y: control.y_offset,
      width: control.width,
      height: control.height,
    });
  }
  if frames.is_empty() {
    return Err(DecodeError::Corrupt);
  }
  Ok(ImageFrames {
    frames,
    animated: true,
    repetition_count: extra_plays(plays),
  })
}

struct PendingDispose {
  op: png::DisposeOp,
  x: u32,
  y: u32,
  width: u32,
  height: u32,
}

enum PngCanvas {
  Empty {
    width: u32,
    height: u32,
  },
  U8 {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
    previous: Option<Vec<u8>>,
  },
  U16 {
    width: u32,
    height: u32,
    pixels: Vec<u16>,
    previous: Option<Vec<u16>>,
  },
}

impl PngCanvas {
  fn new(width: u32, height: u32) -> Result<Self, DecodeError> {
    Ok(Self::Empty { width, height })
  }

  fn apply_dispose(
    &mut self,
    pending: Option<PendingDispose>,
  ) -> Result<(), DecodeError> {
    let Some(pending) = pending else {
      return Ok(());
    };
    match pending.op {
      png::DisposeOp::None => Ok(()),
      png::DisposeOp::Background => {
        self.clear(pending.x, pending.y, pending.width, pending.height);
        Ok(())
      }
      png::DisposeOp::Previous => self.restore_previous(),
    }
  }

  fn clear(&mut self, x: u32, y: u32, width: u32, height: u32) {
    match self {
      Self::Empty { .. } => {}
      Self::U8 {
        width: canvas_w,
        pixels,
        ..
      } => clear_rect(pixels, *canvas_w, x, y, width, height),
      Self::U16 {
        width: canvas_w,
        pixels,
        ..
      } => clear_rect(pixels, *canvas_w, x, y, width, height),
    }
  }

  fn restore_previous(&mut self) -> Result<(), DecodeError> {
    match self {
      Self::Empty { .. } => return Err(DecodeError::Corrupt),
      Self::U8 {
        pixels, previous, ..
      } => {
        *pixels = previous.take().ok_or(DecodeError::Corrupt)?;
      }
      Self::U16 {
        pixels, previous, ..
      } => {
        *pixels = previous.take().ok_or(DecodeError::Corrupt)?;
      }
    }
    Ok(())
  }

  fn snapshot_previous(&mut self) {
    match self {
      Self::Empty { .. } => {}
      Self::U8 {
        pixels, previous, ..
      } => *previous = Some(pixels.clone()),
      Self::U16 {
        pixels, previous, ..
      } => *previous = Some(pixels.clone()),
    }
  }

  fn blend(
    &mut self,
    output: &png::OutputInfo,
    pixels: &[u8],
    control: &png::FrameControl,
  ) -> Result<(), DecodeError> {
    let sixteen = output.bit_depth == png::BitDepth::Sixteen;
    self.ensure_depth(sixteen)?;
    let sample_bytes = if sixteen { 2 } else { 1 };
    let packed = packed_rows(output, pixels, sample_bytes)?;
    let over = control.blend_op == png::BlendOp::Over;
    match self {
      Self::Empty { .. } => return Err(DecodeError::Corrupt),
      Self::U8 { width, pixels, .. } => {
        let src = super::expand_u8(output.color_type, &packed)?;
        blend_rgba(pixels, *width, &src, control, over);
      }
      Self::U16 { width, pixels, .. } => {
        let samples = super::be_u16(&packed);
        let src = super::expand_u16(output.color_type, &samples)?;
        blend_rgba(pixels, *width, &src, control, over);
      }
    }
    Ok(())
  }

  fn ensure_depth(&mut self, sixteen: bool) -> Result<(), DecodeError> {
    let (width, height) = match self {
      Self::U8 { .. } if !sixteen => return Ok(()),
      Self::U16 { .. } if sixteen => return Ok(()),
      Self::Empty { width, height } => (*width, *height),
      _ => return Err(DecodeError::Corrupt),
    };
    let count = width as usize * height as usize * 4;
    *self = if sixteen {
      Self::U16 {
        width,
        height,
        pixels: vec![0; count],
        previous: None,
      }
    } else {
      Self::U8 {
        width,
        height,
        pixels: vec![0; count],
        previous: None,
      }
    };
    Ok(())
  }

  fn raster(&self) -> Result<Raster, DecodeError> {
    match self {
      Self::Empty { .. } => Err(DecodeError::Corrupt),
      Self::U8 {
        width,
        height,
        pixels,
        ..
      } => super::png_raster(
        *width,
        *height,
        png::ColorType::Rgba,
        png::BitDepth::Eight,
        pixels,
      ),
      Self::U16 {
        width,
        height,
        pixels,
        ..
      } => {
        let mut bytes = Vec::with_capacity(pixels.len() * 2);
        for sample in pixels {
          bytes.extend(sample.to_be_bytes());
        }
        super::png_raster(
          *width,
          *height,
          png::ColorType::Rgba,
          png::BitDepth::Sixteen,
          &bytes,
        )
      }
    }
  }
}

fn packed_rows(
  output: &png::OutputInfo,
  pixels: &[u8],
  sample_bytes: usize,
) -> Result<Vec<u8>, DecodeError> {
  let row_bytes =
    output.width as usize * output.color_type.samples() * sample_bytes;
  if output.line_size < row_bytes {
    return Err(DecodeError::Corrupt);
  }
  let mut packed = Vec::with_capacity(row_bytes * output.height as usize);
  for row in 0..output.height as usize {
    let start = row * output.line_size;
    let end = start + row_bytes;
    let row_pixels = pixels.get(start..end).ok_or(DecodeError::Corrupt)?;
    packed.extend_from_slice(row_pixels);
  }
  Ok(packed)
}

fn rect_inside(
  x: u32,
  y: u32,
  width: u32,
  height: u32,
  canvas_w: u32,
  canvas_h: u32,
) -> bool {
  width > 0
    && height > 0
    && x.checked_add(width).is_some_and(|right| right <= canvas_w)
    && y
      .checked_add(height)
      .is_some_and(|bottom| bottom <= canvas_h)
}

fn clear_rect<S: Copy + Default>(
  pixels: &mut [S],
  canvas_w: u32,
  x: u32,
  y: u32,
  width: u32,
  height: u32,
) {
  for row in 0..height {
    let start = ((y + row) * canvas_w + x) as usize * 4;
    let end = start + width as usize * 4;
    pixels[start..end].fill(S::default());
  }
}

fn blend_rgba<S: Channel>(
  canvas: &mut [S],
  canvas_w: u32,
  src: &[S],
  control: &png::FrameControl,
  over: bool,
) {
  for row in 0..control.height {
    for col in 0..control.width {
      let src_at = ((row * control.width + col) * 4) as usize;
      let dst_at =
        (((control.y_offset + row) * canvas_w + control.x_offset + col) * 4)
          as usize;
      let src_pixel = &src[src_at..src_at + 4];
      let dst_pixel = &mut canvas[dst_at..dst_at + 4];
      if !over || src_pixel[3].to_channel() == S::SCALE {
        dst_pixel.copy_from_slice(src_pixel);
        continue;
      }
      if src_pixel[3].to_channel() == 0 {
        continue;
      }
      let src_a = src_pixel[3].to_channel();
      let dst_a = dst_pixel[3].to_channel();
      let out_a = composite_alpha(src_a, dst_a, S::SCALE);
      if out_a == 0 {
        dst_pixel.fill(S::from_channel(0));
        continue;
      }
      for channel in 0..3 {
        let blended = over_channel(
          src_pixel[channel].to_channel(),
          src_a,
          dst_pixel[channel].to_channel(),
          dst_a,
          out_a,
          S::SCALE,
        );
        dst_pixel[channel] = S::from_channel(blended);
      }
      dst_pixel[3] = S::from_channel(out_a);
    }
  }
}

trait Channel: Copy {
  const SCALE: u32;
  fn to_channel(self) -> u32;
  fn from_channel(value: u32) -> Self;
}

impl Channel for u8 {
  const SCALE: u32 = 255;
  fn to_channel(self) -> u32 {
    u32::from(self)
  }
  fn from_channel(value: u32) -> Self {
    value.min(Self::SCALE) as u8
  }
}

impl Channel for u16 {
  const SCALE: u32 = 65535;
  fn to_channel(self) -> u32 {
    u32::from(self)
  }
  fn from_channel(value: u32) -> Self {
    value.min(Self::SCALE) as u16
  }
}

fn composite_alpha(src_a: u32, dst_a: u32, scale: u32) -> u32 {
  src_a
    + (u64::from(dst_a) * u64::from(scale - src_a) / u64::from(scale)) as u32
}

fn over_channel(
  src_c: u32,
  src_a: u32,
  dst_c: u32,
  dst_a: u32,
  out_a: u32,
  scale: u32,
) -> u32 {
  let blended = u64::from(src_c) * u64::from(src_a)
    + u64::from(dst_c) * u64::from(dst_a) * u64::from(scale - src_a)
      / u64::from(scale);
  ((blended + u64::from(out_a) / 2) / u64::from(out_a)) as u32
}

fn png_delay_us(numerator: u16, denominator: u16) -> f64 {
  let denominator = if denominator == 0 { 100 } else { denominator };
  (u64::from(numerator) * 1_000_000 / u64::from(denominator)) as f64
}

fn decode_webp_frames(
  bytes: &[u8],
  to_srgb: bool,
) -> Result<ImageFrames, DecodeError> {
  let mut decoder = image_webp::WebPDecoder::new(Cursor::new(bytes))
    .map_err(|_| DecodeError::Corrupt)?;
  if !decoder.is_animated() {
    return still(super::decode_webp(bytes, to_srgb));
  }
  let (width, height) = decoder.dimensions();
  if !crate::raster::within_limits(width, height) {
    return Err(DecodeError::TooLarge);
  }
  let repetition = match decoder.loop_count() {
    image_webp::LoopCount::Forever => f64::INFINITY,
    image_webp::LoopCount::Times(times) => f64::from(times.get() - 1),
  };
  let icc = decoder.icc_profile().ok().flatten();
  let orientation = super::exif_orientation(bytes);
  let size = decoder.output_buffer_size().ok_or(DecodeError::Corrupt)?;
  let mut pixels = vec![0u8; size];
  let count = decoder.num_frames();
  if count == 0 {
    return Err(DecodeError::Corrupt);
  }
  let mut frames = Vec::with_capacity(count as usize);
  for _ in 0..count {
    let milliseconds = decoder
      .read_frame(&mut pixels)
      .map_err(|_| DecodeError::Corrupt)?;
    let rgba = super::to_rgba8(width, height, &pixels)?;
    let mut raster =
      Raster::from_rgba8(width, height, crate::raster::ColorSpace::Srgb, &rgba)
        .map_err(super::pixel_error)?;
    let decoded =
      super::finish(&mut raster, icc.as_deref(), None, to_srgb, orientation)?;
    frames.push(ImageFrame {
      raster: decoded.raster,
      yuv: None,
      orientation: decoded.orientation,
      duration_us: Some(f64::from(milliseconds) * 1000.0),
    });
  }
  Ok(ImageFrames {
    frames,
    animated: true,
    repetition_count: repetition,
  })
}

fn decode_jxl_frames(
  bytes: &[u8],
  to_srgb: bool,
) -> Result<ImageFrames, DecodeError> {
  let image = super::read_jxl(bytes)?;
  let Some(animation) = image.image_header().metadata.animation.as_ref() else {
    let (mut raster, _) = super::jxl_frame(&image, 0)?;
    let (icc, cicp) = super::jxl_color(&image);
    return still(super::finish(&mut raster, icc.as_deref(), cicp, to_srgb, 1));
  };
  let numerator = animation.tps_numerator;
  let denominator = animation.tps_denominator;
  let repetition = extra_plays(animation.num_loops);
  let count = image.num_loaded_keyframes();
  if count == 0 {
    return Err(DecodeError::Corrupt);
  }
  let (icc, cicp) = super::jxl_color(&image);
  let mut frames = Vec::with_capacity(count);
  for index in 0..count {
    let (mut raster, ticks) = super::jxl_frame(&image, index)?;
    let decoded = super::finish(&mut raster, icc.as_deref(), cicp, to_srgb, 1)?;
    frames.push(ImageFrame {
      raster: decoded.raster,
      yuv: None,
      orientation: decoded.orientation,
      duration_us: Some(ticks_to_us(ticks, numerator, denominator)),
    });
  }
  let frames = drop_intermediate_zeros(frames);
  if frames.is_empty() {
    return Err(DecodeError::Corrupt);
  }
  Ok(ImageFrames {
    frames,
    animated: true,
    repetition_count: repetition,
  })
}

fn drop_intermediate_zeros(frames: Vec<ImageFrame>) -> Vec<ImageFrame> {
  let last = frames.len().saturating_sub(1);
  frames
    .into_iter()
    .enumerate()
    .filter(|(index, frame)| *index == last || frame.duration_us != Some(0.0))
    .map(|(_, frame)| frame)
    .collect()
}

fn ticks_to_us(ticks: u32, numerator: u32, denominator: u32) -> f64 {
  let numerator = if numerator == 0 { 1000 } else { numerator };
  let denominator = if denominator == 0 { 1 } else { denominator };
  let microseconds = u128::from(ticks) * 1_000_000 * u128::from(denominator)
    / u128::from(numerator);
  microseconds as f64
}

fn extra_plays(plays: u32) -> f64 {
  if plays == 0 {
    f64::INFINITY
  } else {
    f64::from(plays - 1)
  }
}

pub(super) fn decode_gif(
  bytes: &[u8],
  to_srgb: bool,
) -> Result<Decoded, DecodeError> {
  let loaded = load_gif(bytes, to_srgb, Some(1))?;
  let frame = loaded
    .frames
    .into_iter()
    .next()
    .ok_or(DecodeError::Corrupt)?;
  Ok(Decoded {
    raster: frame.raster,
    orientation: frame.orientation,
    yuv: None,
  })
}

fn decode_gif_frames(
  bytes: &[u8],
  to_srgb: bool,
) -> Result<ImageFrames, DecodeError> {
  let loaded = load_gif(bytes, to_srgb, None)?;
  if loaded.frames.len() < 2 {
    let mut frames = loaded.frames;
    if let Some(frame) = frames.first_mut() {
      frame.duration_us = None;
    }
    return Ok(ImageFrames {
      frames,
      animated: false,
      repetition_count: 0.0,
    });
  }
  Ok(ImageFrames {
    frames: loaded.frames,
    animated: true,
    repetition_count: gif_repetition(loaded.repeat),
  })
}

struct LoadedGif {
  frames: Vec<ImageFrame>,
  repeat: gif::Repeat,
}

struct GifCopied {
  left: u32,
  top: u32,
  width: u32,
  height: u32,
  delay: u16,
  dispose: gif::DisposalMethod,
  buffer: Vec<u8>,
}

struct GifPending {
  dispose: gif::DisposalMethod,
  left: u32,
  top: u32,
  width: u32,
  height: u32,
}

fn load_gif(
  bytes: &[u8],
  to_srgb: bool,
  limit: Option<usize>,
) -> Result<LoadedGif, DecodeError> {
  let mut options = gif::DecodeOptions::new();
  options.set_color_output(gif::ColorOutput::RGBA);
  options.check_frame_consistency(true);
  let mut decoder = options
    .read_info(Cursor::new(bytes))
    .map_err(|_| DecodeError::Corrupt)?;
  let width = u32::from(decoder.width());
  let height = u32::from(decoder.height());
  if width == 0 || height == 0 {
    return Err(DecodeError::Corrupt);
  }
  if !crate::raster::within_limits(width, height) {
    return Err(DecodeError::TooLarge);
  }
  let pixels = (width as usize)
    .checked_mul(height as usize)
    .and_then(|count| count.checked_mul(4))
    .ok_or(DecodeError::TooLarge)?;
  let mut canvas = vec![0u8; pixels];
  let mut saved: Option<Vec<u8>> = None;
  let mut pending: Option<GifPending> = None;
  let mut frames = Vec::new();
  loop {
    if limit.is_some_and(|limit| frames.len() >= limit) {
      break;
    }
    let copied = match decoder.read_next_frame() {
      Ok(Some(frame)) => {
        let frame_w = u32::from(frame.width);
        let frame_h = u32::from(frame.height);
        let expected = (frame_w as usize)
          .saturating_mul(frame_h as usize)
          .saturating_mul(4);
        let Some(buffer) = frame.buffer.get(..expected) else {
          return Err(DecodeError::Corrupt);
        };
        GifCopied {
          left: u32::from(frame.left),
          top: u32::from(frame.top),
          width: frame_w,
          height: frame_h,
          delay: frame.delay,
          dispose: frame.dispose,
          buffer: buffer.to_vec(),
        }
      }
      Ok(None) => break,
      Err(_) => return Err(DecodeError::Corrupt),
    };
    if !rect_inside(
      copied.left,
      copied.top,
      copied.width,
      copied.height,
      width,
      height,
    ) {
      return Err(DecodeError::Corrupt);
    }
    apply_gif_dispose(&mut canvas, width, pending.take(), &mut saved)?;
    if copied.dispose == gif::DisposalMethod::Previous {
      saved = Some(canvas.clone());
    }
    blit_gif(
      &mut canvas,
      width,
      &copied.buffer,
      copied.left,
      copied.top,
      copied.width,
      copied.height,
    );
    let mut raster = Raster::from_rgba8(
      width,
      height,
      crate::raster::ColorSpace::Srgb,
      &canvas,
    )
    .map_err(super::pixel_error)?;
    let decoded = super::finish(&mut raster, None, None, to_srgb, 1)?;
    frames.push(ImageFrame {
      raster: decoded.raster,
      yuv: None,
      orientation: decoded.orientation,
      duration_us: Some(gif_delay_us(copied.delay)),
    });
    pending = Some(GifPending {
      dispose: copied.dispose,
      left: copied.left,
      top: copied.top,
      width: copied.width,
      height: copied.height,
    });
  }
  if frames.is_empty() {
    return Err(DecodeError::Corrupt);
  }
  Ok(LoadedGif {
    frames,
    repeat: decoder.repeat(),
  })
}

fn apply_gif_dispose(
  canvas: &mut [u8],
  canvas_w: u32,
  pending: Option<GifPending>,
  saved: &mut Option<Vec<u8>>,
) -> Result<(), DecodeError> {
  let Some(pending) = pending else {
    return Ok(());
  };
  match pending.dispose {
    gif::DisposalMethod::Any | gif::DisposalMethod::Keep => Ok(()),
    gif::DisposalMethod::Background => {
      clear_rect(
        canvas,
        canvas_w,
        pending.left,
        pending.top,
        pending.width,
        pending.height,
      );
      Ok(())
    }
    gif::DisposalMethod::Previous => {
      let Some(previous) = saved.take() else {
        return Err(DecodeError::Corrupt);
      };
      if previous.len() != canvas.len() {
        return Err(DecodeError::Corrupt);
      }
      canvas.copy_from_slice(&previous);
      Ok(())
    }
  }
}

fn blit_gif(
  canvas: &mut [u8],
  canvas_w: u32,
  src: &[u8],
  left: u32,
  top: u32,
  width: u32,
  height: u32,
) {
  for row in 0..height {
    for col in 0..width {
      let src_at = (row * width + col) as usize * 4;
      if src[src_at + 3] == 0 {
        continue;
      }
      let dst_at = ((top + row) * canvas_w + left + col) as usize * 4;
      canvas[dst_at..dst_at + 4].copy_from_slice(&src[src_at..src_at + 4]);
    }
  }
}

/// Chrome treats a delay below 2 centiseconds as 100 ms. Otherwise one
/// centisecond is 10 ms.
fn gif_delay_us(centiseconds: u16) -> f64 {
  if centiseconds < 2 {
    100_000.0
  } else {
    f64::from(centiseconds) * 10_000.0
  }
}

/// Netscape loop 0 is [`gif::Repeat::Infinite`]. `Finite(0)` is that crate's
/// default when the loop block is absent; treat it as endless too. `Finite(n)`
/// is `n - 1` extra plays, matching WebP `LoopCount::Times`.
fn gif_repetition(repeat: gif::Repeat) -> f64 {
  match repeat {
    gif::Repeat::Infinite | gif::Repeat::Finite(0) => f64::INFINITY,
    gif::Repeat::Finite(count) => f64::from(count - 1),
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::raster::SampleFormat;

  fn fixture(name: &str) -> Vec<u8> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
      .join("../../tests/fixtures/images")
      .join(name);
    std::fs::read(path).expect(name)
  }

  fn wpt_jxl(name: &str) -> Vec<u8> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
      .join("../../tests/wpt/suite/jpegxl/resources")
      .join(name);
    std::fs::read(&path)
      .unwrap_or_else(|error| panic!("{}: {error}", path.display()))
  }

  fn rgba(width: u32, height: u32, pixel: [u8; 4]) -> Vec<u8> {
    pixel
      .into_iter()
      .cycle()
      .take(width as usize * height as usize * 4)
      .collect()
  }

  fn encode_apng(plays: u32, frames: &[ApngFrame]) -> Vec<u8> {
    let mut bytes = Vec::new();
    {
      let mut encoder = png::Encoder::new(&mut bytes, 2, 2);
      encoder.set_color(png::ColorType::Rgba);
      encoder.set_depth(png::BitDepth::Eight);
      encoder.set_animated(frames.len() as u32, plays).unwrap();
      let mut writer = encoder.write_header().unwrap();
      for frame in frames {
        writer
          .set_frame_delay(frame.delay_num, frame.delay_den)
          .unwrap();
        writer.reset_frame_position().unwrap();
        writer.reset_frame_dimension().unwrap();
        writer
          .set_frame_dimension(frame.width, frame.height)
          .unwrap();
        writer.set_frame_position(frame.x, frame.y).unwrap();
        writer.set_blend_op(frame.blend).unwrap();
        writer.set_dispose_op(frame.dispose).unwrap();
        writer.write_image_data(frame.pixels).unwrap();
      }
    }
    bytes
  }

  struct ApngFrame<'a> {
    pixels: &'a [u8],
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    delay_num: u16,
    delay_den: u16,
    blend: png::BlendOp,
    dispose: png::DisposeOp,
  }

  #[test]
  fn still_images_stay_one_frame() {
    for name in ["corners.png", "corners.webp", "corners.jxl", "red.jpg"] {
      let bytes = fixture(name);
      let still = super::super::decode(&bytes, true).unwrap();
      let frames = decode_all(&bytes, true).unwrap();
      assert!(!frames.animated, "{name}");
      assert_eq!(frames.repetition_count, 0.0, "{name}");
      assert_eq!(frames.frames.len(), 1, "{name}");
      assert!(frames.frames[0].duration_us.is_none(), "{name}");
      assert_eq!(frames.frames[0].raster.bytes, still.raster.bytes, "{name}");
    }
  }

  #[test]
  fn apng_composites_source_and_over() {
    let red = rgba(2, 2, [255, 0, 0, 255]);
    let green = [0, 255, 0, 128];
    let bytes = encode_apng(
      1,
      &[
        ApngFrame {
          pixels: &red,
          x: 0,
          y: 0,
          width: 2,
          height: 2,
          delay_num: 1,
          delay_den: 10,
          blend: png::BlendOp::Source,
          dispose: png::DisposeOp::None,
        },
        ApngFrame {
          pixels: &green,
          x: 1,
          y: 1,
          width: 1,
          height: 1,
          delay_num: 1,
          delay_den: 10,
          blend: png::BlendOp::Over,
          dispose: png::DisposeOp::None,
        },
      ],
    );
    let still = super::super::decode(&bytes, true).unwrap();
    assert_eq!(still.raster.pixel_u8(0, 0), [255, 0, 0, 255]);
    let frames = decode_all(&bytes, true).unwrap();
    assert!(frames.animated);
    assert_eq!(frames.repetition_count, 0.0);
    assert_eq!(frames.frames.len(), 2);
    assert_eq!(frames.frames[0].duration_us, Some(100_000.0));
    assert_eq!(frames.frames[0].raster.pixel_u8(1, 1), [255, 0, 0, 255]);
    assert_eq!(frames.frames[1].raster.pixel_u8(0, 0), [255, 0, 0, 255]);
    assert_eq!(frames.frames[1].raster.pixel_u8(1, 1), [127, 128, 0, 255]);
  }

  #[test]
  fn apng_repetition_follows_num_plays() {
    let red = rgba(2, 2, [255, 0, 0, 255]);
    let frame = |plays| {
      encode_apng(
        plays,
        &[ApngFrame {
          pixels: &red,
          x: 0,
          y: 0,
          width: 2,
          height: 2,
          delay_num: 0,
          delay_den: 0,
          blend: png::BlendOp::Source,
          dispose: png::DisposeOp::None,
        }],
      )
    };
    assert_eq!(decode_all(&frame(1), true).unwrap().repetition_count, 0.0);
    assert_eq!(decode_all(&frame(2), true).unwrap().repetition_count, 1.0);
    assert!(
      decode_all(&frame(0), true)
        .unwrap()
        .repetition_count
        .is_infinite()
    );
    assert_eq!(
      decode_all(&frame(1), true).unwrap().frames[0].duration_us,
      Some(0.0)
    );
  }

  #[test]
  fn apng_dispose_background_clears_the_rect() {
    let blue = rgba(2, 2, [0, 0, 255, 255]);
    let red = [255, 0, 0, 255];
    let bytes = encode_apng(
      1,
      &[
        ApngFrame {
          pixels: &blue,
          x: 0,
          y: 0,
          width: 2,
          height: 2,
          delay_num: 1,
          delay_den: 100,
          blend: png::BlendOp::Source,
          dispose: png::DisposeOp::Background,
        },
        ApngFrame {
          pixels: &red,
          x: 0,
          y: 0,
          width: 1,
          height: 1,
          delay_num: 1,
          delay_den: 100,
          blend: png::BlendOp::Source,
          dispose: png::DisposeOp::None,
        },
      ],
    );
    let frames = decode_all(&bytes, true).unwrap();
    assert_eq!(frames.frames[1].raster.pixel_u8(0, 0), [255, 0, 0, 255]);
    assert_eq!(frames.frames[1].raster.pixel_u8(1, 0), [0, 0, 0, 0]);
    assert_eq!(frames.frames[1].raster.pixel_u8(0, 1), [0, 0, 0, 0]);
  }

  #[test]
  fn apng_dispose_previous_restores_the_canvas() {
    let red = rgba(2, 2, [255, 0, 0, 255]);
    let green = rgba(2, 2, [0, 255, 0, 255]);
    let blue = [0, 0, 255, 255];
    let bytes = encode_apng(
      1,
      &[
        ApngFrame {
          pixels: &red,
          x: 0,
          y: 0,
          width: 2,
          height: 2,
          delay_num: 1,
          delay_den: 100,
          blend: png::BlendOp::Source,
          dispose: png::DisposeOp::None,
        },
        ApngFrame {
          pixels: &green,
          x: 0,
          y: 0,
          width: 2,
          height: 2,
          delay_num: 1,
          delay_den: 100,
          blend: png::BlendOp::Source,
          dispose: png::DisposeOp::Previous,
        },
        ApngFrame {
          pixels: &blue,
          x: 0,
          y: 0,
          width: 1,
          height: 1,
          delay_num: 1,
          delay_den: 100,
          blend: png::BlendOp::Source,
          dispose: png::DisposeOp::None,
        },
      ],
    );
    let frames = decode_all(&bytes, true).unwrap();
    assert_eq!(frames.frames[2].raster.pixel_u8(0, 0), [0, 0, 255, 255]);
    assert_eq!(frames.frames[2].raster.pixel_u8(1, 0), [255, 0, 0, 255]);
  }

  #[test]
  fn apng_first_previous_is_background() {
    let red = rgba(2, 2, [255, 0, 0, 255]);
    let green = [0, 255, 0, 255];
    let bytes = encode_apng(
      1,
      &[
        ApngFrame {
          pixels: &red,
          x: 0,
          y: 0,
          width: 2,
          height: 2,
          delay_num: 1,
          delay_den: 100,
          blend: png::BlendOp::Source,
          dispose: png::DisposeOp::Previous,
        },
        ApngFrame {
          pixels: &green,
          x: 0,
          y: 0,
          width: 1,
          height: 1,
          delay_num: 1,
          delay_den: 100,
          blend: png::BlendOp::Source,
          dispose: png::DisposeOp::None,
        },
      ],
    );
    let frame = &decode_all(&bytes, true).unwrap().frames[1];
    assert_eq!(frame.raster.pixel_u8(0, 0), [0, 255, 0, 255]);
    assert_eq!(frame.raster.pixel_u8(1, 0), [0, 0, 0, 0]);
  }

  #[test]
  fn sixteen_bit_apng_is_float16() {
    let mut bytes = Vec::new();
    {
      let mut encoder = png::Encoder::new(&mut bytes, 1, 1);
      encoder.set_color(png::ColorType::Rgba);
      encoder.set_depth(png::BitDepth::Sixteen);
      encoder.set_animated(2, 1).unwrap();
      let mut writer = encoder.write_header().unwrap();
      for pixel in [[0x8000u16, 0, 0, 65535], [0, 0x8000, 0, 65535]] {
        writer.set_frame_delay(1, 100).unwrap();
        let mut data = Vec::new();
        for sample in pixel {
          data.extend(sample.to_be_bytes());
        }
        writer.write_image_data(&data).unwrap();
      }
    }
    let frames = decode_all(&bytes, true).unwrap();
    assert_eq!(frames.frames[0].raster.format, SampleFormat::Float16);
    let first = frames.frames[0].raster.pixel_f64(0, 0);
    let second = frames.frames[1].raster.pixel_f64(0, 0);
    assert!((first[0] - 0.5).abs() < 0.001, "{first:?}");
    assert!((second[1] - 0.5).abs() < 0.001, "{second:?}");
  }

  fn write_chunk(out: &mut Vec<u8>, name: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(name);
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out.extend_from_slice(data);
    if data.len() % 2 == 1 {
      out.push(0);
    }
  }

  fn write_u24(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes()[..3]);
  }

  fn vp8l_chunk(rgba: &[u8], width: u32, height: u32) -> Vec<u8> {
    let mut file = Vec::new();
    image_webp::WebPEncoder::new(&mut file)
      .encode(rgba, width, height, image_webp::ColorType::Rgba8)
      .unwrap();
    file[12..].to_vec()
  }

  fn animated_webp(frames: &[(&[u8], u32)], loops: u16) -> Vec<u8> {
    let mut body = Vec::new();
    let mut vp8x = vec![0x12, 0, 0, 0];
    write_u24(&mut vp8x, 1);
    write_u24(&mut vp8x, 1);
    write_chunk(&mut body, b"VP8X", &vp8x);
    let mut anim = [0u8; 6];
    anim[4..6].copy_from_slice(&loops.to_le_bytes());
    write_chunk(&mut body, b"ANIM", &anim);
    for (rgba, duration_ms) in frames {
      let bitstream = vp8l_chunk(rgba, 2, 2);
      let mut anmf = Vec::new();
      write_u24(&mut anmf, 0);
      write_u24(&mut anmf, 0);
      write_u24(&mut anmf, 1);
      write_u24(&mut anmf, 1);
      write_u24(&mut anmf, *duration_ms);
      anmf.push(0x02);
      anmf.extend_from_slice(&bitstream);
      if anmf.len() % 2 == 1 {
        anmf.push(0);
      }
      write_chunk(&mut body, b"ANMF", &anmf);
    }
    let mut file = Vec::new();
    file.extend_from_slice(b"RIFF");
    file.extend_from_slice(&((body.len() + 4) as u32).to_le_bytes());
    file.extend_from_slice(b"WEBP");
    file.extend_from_slice(&body);
    file
  }

  #[test]
  fn animated_webp_reports_duration_and_repetition() {
    let red = rgba(2, 2, [255, 0, 0, 255]);
    let blue = rgba(2, 2, [0, 0, 255, 255]);
    let bytes = animated_webp(&[(&red, 100), (&blue, 250)], 2);
    let still = super::super::decode(&bytes, true).unwrap();
    assert_eq!(still.raster.pixel_u8(0, 0), [255, 0, 0, 255]);
    let frames = decode_all(&bytes, true).unwrap();
    assert!(frames.animated);
    assert_eq!(frames.repetition_count, 1.0);
    assert_eq!(frames.frames.len(), 2);
    assert_eq!(frames.frames[0].duration_us, Some(100_000.0));
    assert_eq!(frames.frames[1].duration_us, Some(250_000.0));
    assert_eq!(frames.frames[0].raster.pixel_u8(1, 1), [255, 0, 0, 255]);
    assert_eq!(frames.frames[1].raster.pixel_u8(0, 0), [0, 0, 255, 255]);
    let forever = animated_webp(&[(&red, 1)], 0);
    assert!(
      decode_all(&forever, true)
        .unwrap()
        .repetition_count
        .is_infinite()
    );
    let once = animated_webp(&[(&red, 1)], 1);
    assert_eq!(decode_all(&once, true).unwrap().repetition_count, 0.0);
  }

  #[test]
  fn jpegxl_animation_matches_the_wpt_files() {
    let durations = |name: &str| {
      let frames = decode_all(&wpt_jxl(name), true).unwrap();
      (
        frames.animated,
        frames.frames.len(),
        frames.repetition_count,
        frames
          .frames
          .iter()
          .map(|frame| frame.duration_us)
          .collect::<Vec<_>>(),
      )
    };
    let (animated, count, repetition, once) = durations("animation-1loop.jxl");
    assert!(animated);
    assert_eq!(count, 4, "{once:?}");
    assert_eq!(repetition, 0.0);
    let total: f64 = once.iter().map(|duration| duration.unwrap()).sum();
    assert_eq!(total, 2_000_000.0, "{once:?}");
    let mut previous = 0.0;
    for duration in &once {
      previous += duration.unwrap();
      assert!(duration.unwrap() > 0.0);
    }
    let _ = previous;
    let (_, count, repetition, twice) = durations("animation-2loops.jxl");
    assert_eq!(count, 4, "{twice:?}");
    assert_eq!(repetition, 1.0);
    let (_, _, repetition, _) = durations("animation-infiniteloop.jxl");
    assert!(repetition.is_infinite());
    let (_, count, repetition, spline) =
      durations("conformance_animation_spline.jxl");
    assert_eq!(count, 60, "{spline:?}");
    assert!(repetition.is_infinite());
    let total: f64 = spline.iter().map(|duration| duration.unwrap()).sum();
    assert_eq!(total, 1_200_000.0, "{spline:?}");
  }
}
