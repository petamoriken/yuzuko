//! Windows Media Foundation hardware H.264.
//!
//! This module is compiled only for Windows and is the only place in the
//! codec crate that calls the `windows` crate. `backend` uses the same
//! `Session` shape as VideoToolbox and VA-API. A missing hardware transform
//! fails the decode. There is no software H.264 decoder.

use std::mem::ManuallyDrop;
use std::ptr::{from_mut, from_ref, null_mut};

use windows::Win32::Media::MediaFoundation::{
  IMF2DBuffer, IMFActivate, IMFMediaBuffer, IMFMediaType, IMFSample,
  IMFTransform, MF_E_TRANSFORM_NEED_MORE_INPUT, MF_E_TRANSFORM_STREAM_CHANGE,
  MF_MT_DEFAULT_STRIDE, MF_MT_FRAME_SIZE, MF_MT_MAJOR_TYPE,
  MF_MT_MPEG_SEQUENCE_HEADER, MF_MT_SUBTYPE, MF_VERSION, MFCreateMediaType,
  MFCreateMemoryBuffer, MFCreateSample, MFMediaType_Video, MFSTARTUP_NOSOCKET,
  MFStartup, MFT_CATEGORY_VIDEO_DECODER, MFT_ENUM_FLAG_HARDWARE,
  MFT_ENUM_FLAG_SYNCMFT, MFT_MESSAGE_COMMAND_DRAIN,
  MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, MFT_MESSAGE_NOTIFY_START_OF_STREAM,
  MFT_OUTPUT_DATA_BUFFER, MFT_OUTPUT_STREAM_PROVIDES_SAMPLES,
  MFT_REGISTER_TYPE_INFO, MFTEnumEx, MFVideoFormat_ARGB32, MFVideoFormat_H264,
  MFVideoFormat_NV12, MFVideoFormat_RGB32,
};
use windows::Win32::System::Com::{
  COINIT_MULTITHREADED, CoInitializeEx, CoTaskMemFree,
};
use windows::core::{GUID, Interface};
use yuzuko_image::{ColorSpace, Raster};

use crate::color;
use crate::error::DecodeError;
use crate::h264;

pub struct Session {
  transform: IMFTransform,
  origin_x: u32,
  origin_y: u32,
  visible_width: u32,
  visible_height: u32,
  coded_width: u32,
  coded_height: u32,
}

pub fn supports_avc() -> bool {
  hardware_count(&MFVideoFormat_H264)
}

pub fn open_avc(
  description: &[u8],
  width: u32,
  height: u32,
) -> Result<Session, DecodeError> {
  if !supports_avc() {
    return Err(DecodeError::Unsupported);
  }
  let sets = h264::Sets::parse_avcc(description)?;
  let sps = sets.primary_sps()?;
  let (coded_width, coded_height) = sps.coded_size();
  let (origin_x, origin_y, visible_width, visible_height) = sps.visible_rect();
  if coded_width == 0
    || coded_height == 0
    || visible_width == 0
    || visible_height == 0
  {
    return Err(DecodeError::Rejected);
  }
  if width != 0
    && height != 0
    && (width, height) != (coded_width, coded_height)
    && (width, height) != (visible_width, visible_height)
  {
    return Err(DecodeError::Rejected);
  }
  let transform = activate_hardware(&MFVideoFormat_H264)?;
  unsafe {
    let input: IMFMediaType =
      MFCreateMediaType().map_err(|_| DecodeError::Rejected)?;
    input
      .SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)
      .map_err(|_| DecodeError::Rejected)?;
    input
      .SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_H264)
      .map_err(|_| DecodeError::Rejected)?;
    input
      .SetUINT64(&MF_MT_FRAME_SIZE, pack_size(coded_width, coded_height))
      .map_err(|_| DecodeError::Rejected)?;
    if !description.is_empty() {
      input
        .SetBlob(&MF_MT_MPEG_SEQUENCE_HEADER, description)
        .map_err(|_| DecodeError::Rejected)?;
    }
    transform
      .SetInputType(0, &input, 0)
      .map_err(|_| DecodeError::Rejected)?;
  }
  let mut session = Session {
    transform,
    origin_x,
    origin_y,
    visible_width,
    visible_height,
    coded_width,
    coded_height,
  };
  session.bind_output()?;
  unsafe {
    session
      .transform
      .ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0)
      .map_err(|_| DecodeError::Rejected)?;
    let _ = session
      .transform
      .ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0);
  }
  Ok(session)
}

impl Session {
  pub fn decode(&mut self, bytes: &[u8]) -> Result<Vec<Raster>, DecodeError> {
    if bytes.is_empty() {
      return Err(DecodeError::Rejected);
    }
    unsafe {
      let sample = sample_with_bytes(bytes)?;
      self
        .transform
        .ProcessInput(0, &sample, 0)
        .map_err(|_| DecodeError::Rejected)?;
    }
    self.collect_output()
  }

  pub fn flush(&mut self) -> Result<Vec<Raster>, DecodeError> {
    unsafe {
      let _ = self.transform.ProcessMessage(MFT_MESSAGE_COMMAND_DRAIN, 0);
    }
    self.collect_output()
  }

  fn collect_output(&mut self) -> Result<Vec<Raster>, DecodeError> {
    let mut frames = Vec::new();
    for _ in 0..8 {
      match self.pull_output()? {
        Some(raster) => frames.push(raster),
        None => break,
      }
    }
    Ok(frames)
  }

  fn pull_output(&mut self) -> Result<Option<Raster>, DecodeError> {
    for _ in 0..3 {
      let info = unsafe { self.transform.GetOutputStreamInfo(0) }
        .map_err(|_| DecodeError::Rejected)?;
      let mut buffer = MFT_OUTPUT_DATA_BUFFER::default();
      let provides =
        info.dwFlags & (MFT_OUTPUT_STREAM_PROVIDES_SAMPLES.0 as u32) != 0;
      if !provides {
        if info.cbSize == 0 {
          return Err(DecodeError::Rejected);
        }
        let sample = output_sample(info.cbSize)?;
        buffer.pSample = ManuallyDrop::new(Some(sample));
      }
      let mut output = [buffer];
      let mut status = 0u32;
      let result = unsafe {
        self
          .transform
          .ProcessOutput(0, &mut output, from_mut(&mut status))
      };
      let sample = take_sample(&mut output[0]);
      match result {
        Ok(()) => {
          let sample = sample.ok_or(DecodeError::Rejected)?;
          return self.raster_of(&sample).map(Some);
        }
        Err(error) if error.code() == MF_E_TRANSFORM_NEED_MORE_INPUT => {
          return Ok(None);
        }
        Err(error) if error.code() == MF_E_TRANSFORM_STREAM_CHANGE => {
          self.bind_output()?;
        }
        Err(_) => return Err(DecodeError::Rejected),
      }
    }
    Err(DecodeError::Rejected)
  }

  fn bind_output(&mut self) -> Result<(), DecodeError> {
    let chosen = choose_output(&self.transform)?;
    unsafe {
      chosen
        .SetUINT64(
          &MF_MT_FRAME_SIZE,
          pack_size(self.coded_width, self.coded_height),
        )
        .map_err(|_| DecodeError::Rejected)?;
      self
        .transform
        .SetOutputType(0, &chosen, 0)
        .map_err(|_| DecodeError::Rejected)?;
    }
    Ok(())
  }

  fn raster_of(&self, sample: &IMFSample) -> Result<Raster, DecodeError> {
    let kind = unsafe { self.transform.GetOutputCurrentType(0) }
      .map_err(|_| DecodeError::Rejected)?;
    let subtype = unsafe { kind.GetGUID(&MF_MT_SUBTYPE) }
      .map_err(|_| DecodeError::Rejected)?;
    let (width, height) =
      output_size(&kind, self.coded_width, self.coded_height)?;
    let stride = unsafe { kind.GetUINT32(&MF_MT_DEFAULT_STRIDE) }
      .ok()
      .map(|value| value as i32)
      .unwrap_or(0);
    let buffer = unsafe { sample.ConvertToContiguousBuffer() }
      .map_err(|_| DecodeError::Rejected)?;
    let rgba = if let Ok(image) = buffer.cast::<IMF2DBuffer>() {
      rgba_2d(&image, &subtype, width, height)?
    } else {
      rgba_linear(&buffer, &subtype, width, height, stride)?
    };
    let (origin_x, origin_y, visible_width, visible_height) =
      window_in_buffer(width, height, self);
    let cropped = crop_rgba(
      &rgba,
      width as usize,
      height as usize,
      origin_x as usize,
      origin_y as usize,
      visible_width as usize,
      visible_height as usize,
    )
    .ok_or(DecodeError::Rejected)?;
    Raster::from_rgba8(
      visible_width,
      visible_height,
      ColorSpace::Srgb,
      &cropped,
    )
    .map_err(|_| DecodeError::Rejected)
  }
}

struct Activates {
  list: *mut Option<IMFActivate>,
  count: u32,
}

impl Drop for Activates {
  fn drop(&mut self) {
    if self.list.is_null() {
      return;
    }
    unsafe {
      if self.count > 0 {
        for slot in
          std::slice::from_raw_parts_mut(self.list, self.count as usize)
        {
          drop(slot.take());
        }
      }
      CoTaskMemFree(Some(self.list.cast()));
    }
    self.list = null_mut();
    self.count = 0;
  }
}

fn hardware_count(subtype_id: &GUID) -> bool {
  enumerate(subtype_id).is_ok()
}

fn activate_hardware(subtype_id: &GUID) -> Result<IMFTransform, DecodeError> {
  let found = enumerate(subtype_id)?;
  let activate =
    unsafe { (*found.list).clone() }.ok_or(DecodeError::Unsupported)?;
  unsafe { activate.ActivateObject::<IMFTransform>() }
    .map_err(|_| DecodeError::Rejected)
}

fn enumerate(subtype_id: &GUID) -> Result<Activates, DecodeError> {
  startup();
  unsafe {
    let input = MFT_REGISTER_TYPE_INFO {
      guidMajorType: MFMediaType_Video,
      guidSubtype: *subtype_id,
    };
    let mut list: *mut Option<IMFActivate> = null_mut();
    let mut count = 0u32;
    let status = MFTEnumEx(
      MFT_CATEGORY_VIDEO_DECODER,
      MFT_ENUM_FLAG_HARDWARE | MFT_ENUM_FLAG_SYNCMFT,
      Some(from_ref(&input)),
      None,
      from_mut(&mut list),
      from_mut(&mut count),
    );
    if !status.is_ok() || count == 0 || list.is_null() {
      release_activates(list, count);
      return Err(DecodeError::Unsupported);
    }
    Ok(Activates { list, count })
  }
}

unsafe fn release_activates(list: *mut Option<IMFActivate>, count: u32) {
  if list.is_null() {
    return;
  }
  unsafe {
    if count > 0 {
      for slot in std::slice::from_raw_parts_mut(list, count as usize) {
        drop(slot.take());
      }
    }
    CoTaskMemFree(Some(list.cast()));
  }
}

fn choose_output(
  transform: &IMFTransform,
) -> Result<IMFMediaType, DecodeError> {
  let mut fallback = None;
  for index in 0..16 {
    let Ok(kind) = (unsafe { transform.GetOutputAvailableType(0, index) })
    else {
      break;
    };
    let Ok(subtype) = (unsafe { kind.GetGUID(&MF_MT_SUBTYPE) }) else {
      continue;
    };
    if subtype == MFVideoFormat_NV12 {
      return Ok(kind);
    }
    if fallback.is_none()
      && (subtype == MFVideoFormat_RGB32 || subtype == MFVideoFormat_ARGB32)
    {
      fallback = Some(kind);
    }
  }
  fallback.ok_or(DecodeError::Rejected)
}

fn output_sample(size: u32) -> Result<IMFSample, DecodeError> {
  unsafe {
    let sample = MFCreateSample().map_err(|_| DecodeError::Rejected)?;
    let buffer =
      MFCreateMemoryBuffer(size).map_err(|_| DecodeError::Rejected)?;
    buffer
      .SetCurrentLength(size)
      .map_err(|_| DecodeError::Rejected)?;
    sample
      .AddBuffer(&buffer)
      .map_err(|_| DecodeError::Rejected)?;
    Ok(sample)
  }
}

fn sample_with_bytes(bytes: &[u8]) -> Result<IMFSample, DecodeError> {
  let len = u32::try_from(bytes.len()).map_err(|_| DecodeError::Rejected)?;
  unsafe {
    let sample = MFCreateSample().map_err(|_| DecodeError::Rejected)?;
    let buffer =
      MFCreateMemoryBuffer(len).map_err(|_| DecodeError::Rejected)?;
    let mut data = null_mut();
    let mut max_length = 0u32;
    let mut current = 0u32;
    buffer
      .Lock(
        &mut data,
        Some(from_mut(&mut max_length)),
        Some(from_mut(&mut current)),
      )
      .map_err(|_| DecodeError::Rejected)?;
    if data.is_null() || max_length < len {
      let _ = buffer.Unlock();
      return Err(DecodeError::Rejected);
    }
    std::ptr::copy_nonoverlapping(bytes.as_ptr(), data, bytes.len());
    buffer.Unlock().map_err(|_| DecodeError::Rejected)?;
    buffer
      .SetCurrentLength(len)
      .map_err(|_| DecodeError::Rejected)?;
    sample
      .AddBuffer(&buffer)
      .map_err(|_| DecodeError::Rejected)?;
    Ok(sample)
  }
}

fn take_sample(buffer: &mut MFT_OUTPUT_DATA_BUFFER) -> Option<IMFSample> {
  let sample = unsafe { ManuallyDrop::take(&mut buffer.pSample) };
  let _events = unsafe { ManuallyDrop::take(&mut buffer.pEvents) };
  sample
}

fn rgba_2d(
  image: &IMF2DBuffer,
  subtype: &GUID,
  width: u32,
  height: u32,
) -> Result<Vec<u8>, DecodeError> {
  unsafe {
    let mut scan = null_mut();
    let mut pitch = 0i32;
    image
      .Lock2D(&mut scan, &mut pitch)
      .map_err(|_| DecodeError::Rejected)?;
    let rgba = pixels_from_pointer(scan, pitch, subtype, width, height);
    let _ = image.Unlock2D();
    rgba
  }
}

fn rgba_linear(
  buffer: &IMFMediaBuffer,
  subtype: &GUID,
  width: u32,
  height: u32,
  stride: i32,
) -> Result<Vec<u8>, DecodeError> {
  unsafe {
    let mut data = null_mut();
    let mut max_length = 0u32;
    let mut current = 0u32;
    buffer
      .Lock(
        &mut data,
        Some(from_mut(&mut max_length)),
        Some(from_mut(&mut current)),
      )
      .map_err(|_| DecodeError::Rejected)?;
    let pitch = if stride != 0 {
      stride
    } else if *subtype == MFVideoFormat_NV12 {
      i32::try_from(width).unwrap_or(0)
    } else {
      i32::try_from(width.saturating_mul(4)).unwrap_or(0)
    };
    let rgba = pixels_from_pointer(data, pitch, subtype, width, height);
    let _ = buffer.Unlock();
    rgba
  }
}

fn pixels_from_pointer(
  data: *mut u8,
  pitch: i32,
  subtype: &GUID,
  width: u32,
  height: u32,
) -> Result<Vec<u8>, DecodeError> {
  if data.is_null() || width == 0 || height == 0 || pitch == 0 {
    return Err(DecodeError::Rejected);
  }
  let abs_pitch = pitch.unsigned_abs() as usize;
  let rows = height as usize;
  let nv12 = *subtype == MFVideoFormat_NV12;
  let plane_rows = if nv12 { rows + rows.div_ceil(2) } else { rows };
  let bytes = abs_pitch
    .checked_mul(plane_rows)
    .ok_or(DecodeError::Rejected)?;
  let start = if pitch < 0 {
    unsafe { data.offset(pitch as isize * (plane_rows as isize - 1)) }
  } else {
    data
  };
  let copied = unsafe { std::slice::from_raw_parts(start, bytes) }.to_vec();
  if nv12 {
    let y = &copied[..abs_pitch * rows];
    let uv = copied
      .get(abs_pitch * rows..)
      .ok_or(DecodeError::Rejected)?;
    return color::nv12_to_rgba(
      y,
      uv,
      abs_pitch,
      abs_pitch,
      width as usize,
      height as usize,
      true,
    )
    .ok_or(DecodeError::Rejected);
  }
  if *subtype == MFVideoFormat_RGB32 || *subtype == MFVideoFormat_ARGB32 {
    return color::packed_to_rgba(
      &copied,
      width as usize,
      height as usize,
      abs_pitch,
      true,
    )
    .ok_or(DecodeError::Rejected);
  }
  Err(DecodeError::Rejected)
}

fn crop_rgba(
  rgba: &[u8],
  width: usize,
  height: usize,
  origin_x: usize,
  origin_y: usize,
  visible_width: usize,
  visible_height: usize,
) -> Option<Vec<u8>> {
  let right = origin_x.checked_add(visible_width)?;
  let bottom = origin_y.checked_add(visible_height)?;
  if visible_width == 0
    || visible_height == 0
    || right > width
    || bottom > height
    || rgba.len() < width * height * 4
  {
    return None;
  }
  if origin_x == 0
    && origin_y == 0
    && visible_width == width
    && visible_height == height
  {
    return Some(rgba.to_vec());
  }
  let mut cropped = vec![0u8; visible_width * visible_height * 4];
  for row in 0..visible_height {
    let from = ((origin_y + row) * width + origin_x) * 4;
    let to = row * visible_width * 4;
    cropped[to..to + visible_width * 4]
      .copy_from_slice(&rgba[from..from + visible_width * 4]);
  }
  Some(cropped)
}

fn window_in_buffer(
  width: u32,
  height: u32,
  session: &Session,
) -> (u32, u32, u32, u32) {
  let fits = session.origin_x.saturating_add(session.visible_width) <= width
    && session.origin_y.saturating_add(session.visible_height) <= height;
  if fits {
    (
      session.origin_x,
      session.origin_y,
      session.visible_width,
      session.visible_height,
    )
  } else {
    (0, 0, width, height)
  }
}

fn output_size(
  kind: &IMFMediaType,
  fallback_width: u32,
  fallback_height: u32,
) -> Result<(u32, u32), DecodeError> {
  match unsafe { kind.GetUINT64(&MF_MT_FRAME_SIZE) } {
    Ok(packed) => Ok(unpack_size(packed)),
    Err(_) => Ok((fallback_width, fallback_height)),
  }
}

fn startup() {
  static STARTED: std::sync::Once = std::sync::Once::new();
  STARTED.call_once(|| unsafe {
    let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    let _ = MFStartup(MF_VERSION, MFSTARTUP_NOSOCKET);
  });
}

fn pack_size(width: u32, height: u32) -> u64 {
  (u64::from(width) << 32) | u64::from(height)
}

fn unpack_size(packed: u64) -> (u32, u32) {
  ((packed >> 32) as u32, packed as u32)
}
