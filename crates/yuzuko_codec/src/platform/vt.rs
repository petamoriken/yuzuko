//! VideoToolbox hardware decode.
//!
//! H.264 has no software fallback in this module. AV1 is opened only when
//! VideoToolbox reports a hardware `av01` decoder and accepts the `av1C`
//! format description. A rejected session is returned to the caller, which
//! uses rav1d for that configuration.

use std::ffi::c_void;
use std::ptr::{null, null_mut};

use yuzuko_image::{ColorSpace, Raster};

use crate::color::packed_to_rgba;
use crate::error::DecodeError;

const BGRA: u32 = fourcc(b"BGRA");
const RGBA: u32 = fourcc(b"RGBA");
const AVC1: u32 = fourcc(b"avc1");
const AV01: u32 = fourcc(b"av01");
/// `kCFStringEncodingUTF8`.
const UTF8: u32 = 0x0800_0100;

const fn fourcc(text: &[u8; 4]) -> u32 {
  u32::from_be_bytes(*text)
}

pub fn supports_avc() -> bool {
  supports(AVC1)
}

pub fn supports_av1() -> bool {
  supports(AV01)
}

fn supports(codec: u32) -> bool {
  unsafe { VTIsHardwareDecodeSupported(codec) != 0 }
}

#[repr(C)]
struct KeyCallbacks {
  version: isize,
  retain: *const c_void,
  release: *const c_void,
  copy_description: *const c_void,
  equal: *const c_void,
  hash: *const c_void,
}

#[repr(C)]
struct ValueCallbacks {
  version: isize,
  retain: *const c_void,
  release: *const c_void,
  copy_description: *const c_void,
  equal: *const c_void,
}

#[repr(C)]
struct CmTime {
  value: i64,
  timescale: i32,
  flags: u32,
  epoch: i64,
}

#[repr(C)]
struct Timing {
  duration: CmTime,
  presentation: CmTime,
  decode: CmTime,
}

struct Slot {
  images: Vec<*mut c_void>,
  status: i32,
}

#[repr(C)]
struct CallbackRecord {
  callback: Option<
    unsafe extern "C" fn(
      *mut c_void,
      *mut c_void,
      i32,
      u32,
      *mut c_void,
      CmTime,
      CmTime,
    ),
  >,
  refcon: *mut c_void,
}

pub struct Session {
  format: *mut c_void,
  session: *mut c_void,
  spec: *mut c_void,
  destination: *mut c_void,
  pixel_format: *mut c_void,
  extensions: *mut c_void,
  slot: Box<Slot>,
  /// AV1 may produce no picture for a temporal unit that only carries a
  /// sequence header. H.264 still requires one picture.
  allow_empty: bool,
}

impl Session {
  pub fn decode(&mut self, sample: &[u8]) -> Result<Vec<Raster>, DecodeError> {
    if sample.is_empty() || self.session.is_null() {
      return Err(DecodeError::Rejected);
    }
    self.clear_slot();
    let block = Owned(block_buffer(sample)?);
    let timing = Timing {
      duration: cm_time(1, 30),
      presentation: cm_time(0, 30),
      decode: CmTime {
        value: 0,
        timescale: 0,
        flags: 0,
        epoch: 0,
      },
    };
    let size = sample.len();
    let mut sample_buffer = null_mut();
    let status = unsafe {
      CMSampleBufferCreate(
        null(),
        block.0,
        1,
        null(),
        null(),
        self.format,
        1,
        1,
        &timing,
        1,
        &size,
        &mut sample_buffer,
      )
    };
    let sample_buffer = Owned(sample_buffer);
    if status != 0 || sample_buffer.0.is_null() {
      return Err(hardware_status(status));
    }
    let status = unsafe {
      VTDecompressionSessionDecodeFrame(
        self.session,
        sample_buffer.0,
        0,
        null_mut(),
        null_mut(),
      )
    };
    if status != 0 || self.slot.status != 0 {
      let code = if status != 0 {
        status
      } else {
        self.slot.status
      };
      self.clear_slot();
      return Err(hardware_status(code));
    }
    self.take_frames()
  }

  pub fn flush(&mut self) -> Result<Vec<Raster>, DecodeError> {
    if !self.allow_empty || self.session.is_null() {
      return Ok(Vec::new());
    }
    self.slot.status = 0;
    let status =
      unsafe { VTDecompressionSessionWaitForAsynchronousFrames(self.session) };
    if status != 0 || self.slot.status != 0 {
      let code = if status != 0 {
        status
      } else {
        self.slot.status
      };
      self.clear_slot();
      return Err(hardware_status(code));
    }
    self.take_frames()
  }

  fn clear_slot(&mut self) {
    self.slot.status = 0;
    for image in self.slot.images.drain(..) {
      release(image);
    }
  }

  fn take_frames(&mut self) -> Result<Vec<Raster>, DecodeError> {
    let images: Vec<*mut c_void> = self.slot.images.drain(..).collect();
    if images.is_empty() {
      if self.allow_empty {
        return Ok(Vec::new());
      }
      return Err(DecodeError::Rejected);
    }
    if !self.allow_empty && images.len() != 1 {
      for image in images {
        release(image);
      }
      return Err(DecodeError::Rejected);
    }
    let mut frames = Vec::with_capacity(images.len());
    let mut failed = None;
    for image in images {
      if failed.is_none() {
        match copy_image(image) {
          Ok(frame) => frames.push(frame),
          Err(error) => failed = Some(error),
        }
      }
      release(image);
    }
    match failed {
      Some(error) => Err(error),
      None => Ok(frames),
    }
  }
}

impl Drop for Session {
  fn drop(&mut self) {
    for image in self.slot.images.drain(..) {
      release(image);
    }
    if !self.session.is_null() {
      unsafe { VTDecompressionSessionInvalidate(self.session) };
      release(self.session);
      self.session = null_mut();
    }
    release(self.format);
    release(self.spec);
    release(self.destination);
    release(self.pixel_format);
    release(self.extensions);
  }
}

struct Owned(*mut c_void);

impl Drop for Owned {
  fn drop(&mut self) {
    release(self.0);
  }
}

pub fn open_avc(description: &[u8]) -> Result<Session, DecodeError> {
  if !supports_avc() {
    return Err(DecodeError::Unsupported);
  }
  let sets = parse_avcc(description)?;
  let pointers: Vec<*const u8> = sets.iter().map(|set| set.as_ptr()).collect();
  let sizes: Vec<usize> = sets.iter().map(Vec::len).collect();
  let mut format = null_mut();
  let status = unsafe {
    CMVideoFormatDescriptionCreateFromH264ParameterSets(
      null(),
      sets.len(),
      pointers.as_ptr(),
      sizes.as_ptr(),
      4,
      &mut format,
    )
  };
  if status != 0 || format.is_null() {
    release(format);
    return Err(hardware_status(status));
  }
  open_format(format, null_mut(), false)
}

pub fn open_av1(
  description: &[u8],
  width: u32,
  height: u32,
) -> Result<Session, DecodeError> {
  if !supports_av1() {
    return Err(DecodeError::Unsupported);
  }
  let format = av1_format(description, width, height)?;
  open_format(format, null_mut(), true)
}

fn av1_format(
  description: &[u8],
  width: u32,
  height: u32,
) -> Result<*mut c_void, DecodeError> {
  let Ok(width) = i32::try_from(width) else {
    return Err(DecodeError::Rejected);
  };
  let Ok(height) = i32::try_from(height) else {
    return Err(DecodeError::Rejected);
  };
  if width == 0 || height == 0 || description.is_empty() {
    return Err(DecodeError::Rejected);
  }
  let name = cf_string(c"av1C")?;
  let Ok(length) = isize::try_from(description.len()) else {
    release(name);
    return Err(DecodeError::Rejected);
  };
  let data = unsafe { CFDataCreate(null(), description.as_ptr(), length) };
  if data.is_null() {
    release(name);
    return Err(DecodeError::Rejected);
  }
  let atom_keys = [name.cast_const()];
  let atom_values = [data.cast_const()];
  let atoms = unsafe {
    CFDictionaryCreate(
      null(),
      atom_keys.as_ptr(),
      atom_values.as_ptr(),
      1,
      &kCFTypeDictionaryKeyCallBacks,
      &kCFTypeDictionaryValueCallBacks,
    )
  };
  release(name);
  release(data);
  if atoms.is_null() {
    return Err(DecodeError::Rejected);
  }
  let extension_key =
    unsafe { kCMFormatDescriptionExtension_SampleDescriptionExtensionAtoms };
  let extension_keys = [extension_key];
  let extension_values = [atoms.cast_const()];
  let extensions = unsafe {
    CFDictionaryCreate(
      null(),
      extension_keys.as_ptr(),
      extension_values.as_ptr(),
      1,
      &kCFTypeDictionaryKeyCallBacks,
      &kCFTypeDictionaryValueCallBacks,
    )
  };
  release(atoms);
  if extensions.is_null() {
    return Err(DecodeError::Rejected);
  }
  let mut format = null_mut();
  let status = unsafe {
    CMVideoFormatDescriptionCreate(
      null(),
      AV01,
      width,
      height,
      extensions,
      &mut format,
    )
  };
  release(extensions);
  if status != 0 || format.is_null() {
    release(format);
    return Err(hardware_status(status));
  }
  Ok(format)
}

fn cf_string(text: &std::ffi::CStr) -> Result<*mut c_void, DecodeError> {
  let string =
    unsafe { CFStringCreateWithCString(null(), text.as_ptr(), UTF8) };
  if string.is_null() {
    Err(DecodeError::Rejected)
  } else {
    Ok(string)
  }
}

fn open_format(
  format: *mut c_void,
  extensions: *mut c_void,
  allow_empty: bool,
) -> Result<Session, DecodeError> {
  let spec = decoder_spec()?;
  let (destination, pixel_format) = match destination_buffer() {
    Ok(pair) => pair,
    Err(error) => {
      release(format);
      release(extensions);
      release(spec);
      return Err(error);
    }
  };
  let mut slot = Box::new(Slot {
    images: Vec::new(),
    status: 0,
  });
  let record = CallbackRecord {
    callback: Some(output_cb),
    refcon: std::ptr::from_mut::<Slot>(slot.as_mut()).cast::<c_void>(),
  };
  let mut session = null_mut();
  let status = unsafe {
    VTDecompressionSessionCreate(
      null(),
      format,
      spec,
      destination,
      &record,
      &mut session,
    )
  };
  if status != 0 || session.is_null() {
    release(format);
    release(extensions);
    release(spec);
    release(destination);
    release(pixel_format);
    release(session);
    return Err(hardware_status(status));
  }
  Ok(Session {
    format,
    session,
    spec,
    destination,
    pixel_format,
    extensions,
    slot,
    allow_empty,
  })
}

fn decoder_spec() -> Result<*mut c_void, DecodeError> {
  let key = unsafe {
    kVTVideoDecoderSpecification_RequireHardwareAcceleratedVideoDecoder
  };
  let value = unsafe { kCFBooleanTrue };
  let keys = [key];
  let values = [value];
  let spec = unsafe {
    CFDictionaryCreate(
      null(),
      keys.as_ptr(),
      values.as_ptr(),
      1,
      &kCFTypeDictionaryKeyCallBacks,
      &kCFTypeDictionaryValueCallBacks,
    )
  };
  if spec.is_null() {
    Err(DecodeError::Rejected)
  } else {
    Ok(spec)
  }
}

fn destination_buffer() -> Result<(*mut c_void, *mut c_void), DecodeError> {
  let pixel = BGRA;
  let number = unsafe { CFNumberCreate(null(), 3, (&raw const pixel).cast()) };
  if number.is_null() {
    return Err(DecodeError::Rejected);
  }
  let key = unsafe { kCVPixelBufferPixelFormatTypeKey };
  let keys = [key];
  let values = [number.cast_const()];
  let destination = unsafe {
    CFDictionaryCreate(
      null(),
      keys.as_ptr(),
      values.as_ptr(),
      1,
      &kCFTypeDictionaryKeyCallBacks,
      &kCFTypeDictionaryValueCallBacks,
    )
  };
  if destination.is_null() {
    release(number);
    return Err(DecodeError::Rejected);
  }
  Ok((destination, number))
}

fn hardware_status(status: i32) -> DecodeError {
  if status == 0 {
    DecodeError::Rejected
  } else {
    DecodeError::Hardware(status)
  }
}

fn parse_avcc(bytes: &[u8]) -> Result<Vec<Vec<u8>>, DecodeError> {
  if bytes.len() < 7 || bytes[0] != 1 {
    return Err(DecodeError::Rejected);
  }
  let nal_length = usize::from(bytes[4] & 0x03) + 1;
  if nal_length != 4 {
    return Err(DecodeError::Rejected);
  }
  let mut offset = 5;
  let sps_count = usize::from(bytes[offset] & 0x1f);
  offset += 1;
  let mut sets = Vec::new();
  for _ in 0..sps_count {
    sets.push(take_nal(bytes, &mut offset)?);
  }
  if offset >= bytes.len() {
    return Err(DecodeError::Rejected);
  }
  let pps_count = usize::from(bytes[offset]);
  offset += 1;
  for _ in 0..pps_count {
    sets.push(take_nal(bytes, &mut offset)?);
  }
  if sets.len() < 2 {
    return Err(DecodeError::Rejected);
  }
  Ok(sets)
}

fn take_nal(bytes: &[u8], offset: &mut usize) -> Result<Vec<u8>, DecodeError> {
  if *offset + 2 > bytes.len() {
    return Err(DecodeError::Rejected);
  }
  let length =
    u16::from_be_bytes([bytes[*offset], bytes[*offset + 1]]) as usize;
  *offset += 2;
  if length == 0 || *offset + length > bytes.len() {
    return Err(DecodeError::Rejected);
  }
  let nal = bytes[*offset..*offset + length].to_vec();
  *offset += length;
  Ok(nal)
}

fn block_buffer(sample: &[u8]) -> Result<*mut c_void, DecodeError> {
  let mut block = null_mut();
  let status = unsafe {
    CMBlockBufferCreateWithMemoryBlock(
      null(),
      null_mut(),
      sample.len(),
      null(),
      null(),
      0,
      sample.len(),
      0,
      &mut block,
    )
  };
  if status != 0 || block.is_null() {
    release(block);
    return Err(DecodeError::Rejected);
  }
  let status = unsafe {
    CMBlockBufferReplaceDataBytes(
      sample.as_ptr().cast(),
      block,
      0,
      sample.len(),
    )
  };
  if status != 0 {
    release(block);
    return Err(DecodeError::Rejected);
  }
  Ok(block)
}

fn copy_image(image: *mut c_void) -> Result<Raster, DecodeError> {
  let locked = unsafe { CVPixelBufferLockBaseAddress(image, 1) };
  if locked != 0 {
    return Err(DecodeError::Rejected);
  }
  let width = unsafe { CVPixelBufferGetWidth(image) };
  let height = unsafe { CVPixelBufferGetHeight(image) };
  let stride = unsafe { CVPixelBufferGetBytesPerRow(image) };
  let format = unsafe { CVPixelBufferGetPixelFormatType(image) };
  let base = unsafe { CVPixelBufferGetBaseAddress(image) };
  let raster = if base.is_null() || (format != BGRA && format != RGBA) {
    None
  } else {
    let bytes =
      unsafe { std::slice::from_raw_parts(base.cast::<u8>(), height * stride) };
    packed_to_rgba(bytes, width, height, stride, format == BGRA)
  };
  unsafe { CVPixelBufferUnlockBaseAddress(image, 1) };
  let rgba = raster.ok_or(DecodeError::Rejected)?;
  let width = u32::try_from(width).map_err(|_| DecodeError::Rejected)?;
  let height = u32::try_from(height).map_err(|_| DecodeError::Rejected)?;
  Raster::from_rgba8(width, height, ColorSpace::Srgb, &rgba)
    .map_err(|_| DecodeError::Rejected)
}

fn cm_time(value: i64, timescale: i32) -> CmTime {
  CmTime {
    value,
    timescale,
    flags: 1,
    epoch: 0,
  }
}

fn release(value: *mut c_void) {
  if !value.is_null() {
    unsafe { CFRelease(value) }
  }
}

unsafe extern "C" fn output_cb(
  refcon: *mut c_void,
  _source: *mut c_void,
  status: i32,
  _flags: u32,
  image: *mut c_void,
  _pts: CmTime,
  _duration: CmTime,
) {
  if refcon.is_null() {
    return;
  }
  unsafe {
    let slot = &mut *refcon.cast::<Slot>();
    slot.status = status;
    if image.is_null() {
      return;
    }
    CFRetain(image);
    slot.images.push(image);
  }
}

#[allow(clippy::duplicated_attributes)]
#[link(name = "CoreFoundation", kind = "framework")]
#[link(name = "CoreMedia", kind = "framework")]
#[link(name = "CoreVideo", kind = "framework")]
#[link(name = "VideoToolbox", kind = "framework")]
unsafe extern "C" {
  static kCFBooleanTrue: *const c_void;
  static kCFTypeDictionaryKeyCallBacks: KeyCallbacks;
  static kCFTypeDictionaryValueCallBacks: ValueCallbacks;
  static kVTVideoDecoderSpecification_RequireHardwareAcceleratedVideoDecoder:
    *const c_void;
  static kCVPixelBufferPixelFormatTypeKey: *const c_void;
  static kCMFormatDescriptionExtension_SampleDescriptionExtensionAtoms:
    *const c_void;

  fn CFRelease(value: *const c_void);
  fn CFRetain(value: *const c_void) -> *const c_void;
  fn CFDictionaryCreate(
    allocator: *const c_void,
    keys: *const *const c_void,
    values: *const *const c_void,
    count: isize,
    key_callbacks: *const KeyCallbacks,
    value_callbacks: *const ValueCallbacks,
  ) -> *mut c_void;
  fn CFNumberCreate(
    allocator: *const c_void,
    kind: i32,
    value: *const c_void,
  ) -> *mut c_void;
  fn CFStringCreateWithCString(
    allocator: *const c_void,
    text: *const i8,
    encoding: u32,
  ) -> *mut c_void;
  fn CFDataCreate(
    allocator: *const c_void,
    bytes: *const u8,
    length: isize,
  ) -> *mut c_void;

  fn CMVideoFormatDescriptionCreate(
    allocator: *const c_void,
    codec: u32,
    width: i32,
    height: i32,
    extensions: *const c_void,
    format: *mut *mut c_void,
  ) -> i32;

  fn CMVideoFormatDescriptionCreateFromH264ParameterSets(
    allocator: *const c_void,
    count: usize,
    pointers: *const *const u8,
    sizes: *const usize,
    nal_length: i32,
    format: *mut *mut c_void,
  ) -> i32;
  fn CMBlockBufferCreateWithMemoryBlock(
    allocator: *const c_void,
    memory: *mut c_void,
    block_length: usize,
    block_allocator: *const c_void,
    custom: *const c_void,
    offset: usize,
    data_length: usize,
    flags: u32,
    block: *mut *mut c_void,
  ) -> i32;
  fn CMBlockBufferReplaceDataBytes(
    source: *const c_void,
    block: *mut c_void,
    offset: usize,
    length: usize,
  ) -> i32;
  fn CMSampleBufferCreate(
    allocator: *const c_void,
    data: *mut c_void,
    data_ready: u8,
    ready: *const c_void,
    refcon: *const c_void,
    format: *mut c_void,
    sample_count: isize,
    timing_count: isize,
    timing: *const Timing,
    size_count: isize,
    sizes: *const usize,
    sample: *mut *mut c_void,
  ) -> i32;

  fn VTDecompressionSessionCreate(
    allocator: *const c_void,
    format: *mut c_void,
    specification: *const c_void,
    destination: *const c_void,
    callback: *const CallbackRecord,
    session: *mut *mut c_void,
  ) -> i32;
  fn VTDecompressionSessionInvalidate(session: *mut c_void);
  fn VTDecompressionSessionDecodeFrame(
    session: *mut c_void,
    sample: *mut c_void,
    flags: u32,
    source: *mut c_void,
    info: *mut u32,
  ) -> i32;
  fn VTDecompressionSessionWaitForAsynchronousFrames(
    session: *mut c_void,
  ) -> i32;
  fn VTIsHardwareDecodeSupported(codec: u32) -> u8;

  fn CVPixelBufferLockBaseAddress(buffer: *mut c_void, flags: u32) -> i32;
  fn CVPixelBufferUnlockBaseAddress(buffer: *mut c_void, flags: u32) -> i32;
  fn CVPixelBufferGetWidth(buffer: *mut c_void) -> usize;
  fn CVPixelBufferGetHeight(buffer: *mut c_void) -> usize;
  fn CVPixelBufferGetBytesPerRow(buffer: *mut c_void) -> usize;
  fn CVPixelBufferGetPixelFormatType(buffer: *mut c_void) -> u32;
  fn CVPixelBufferGetBaseAddress(buffer: *mut c_void) -> *mut c_void;
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn interlaced_mbaff_session_is_rejected() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
      .join("../../tests/wpt/suite/webcodecs/h264_interlaced.mp4");
    let file = std::fs::read(&path).expect("interlaced");
    let Err(error) = open_avc(&file[7501..7548]) else {
      panic!("session opened");
    };
    eprintln!("interlaced configure {error:?}");
    assert!(matches!(error, DecodeError::Hardware(-12911)), "{error:?}");
  }
}
