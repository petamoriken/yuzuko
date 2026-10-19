//! VideoToolbox hardware H.264 decode.
//! There is no software fallback in this module.

use std::ffi::c_void;
use std::ptr::{null, null_mut};

use yuzuko_image::{ColorSpace, Raster};

use crate::color::packed_to_rgba;
use crate::error::DecodeError;

const BGRA: u32 = fourcc(b"BGRA");
const RGBA: u32 = fourcc(b"RGBA");
const AVC1: u32 = fourcc(b"avc1");

const fn fourcc(text: &[u8; 4]) -> u32 {
  u32::from_be_bytes(*text)
}

pub fn supports_avc() -> bool {
  supports(AVC1)
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
  image: *mut c_void,
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
}

impl Session {
  pub fn decode(&mut self, sample: &[u8]) -> Result<Vec<Raster>, DecodeError> {
    if sample.is_empty() || self.session.is_null() {
      return Err(DecodeError::Rejected);
    }
    self.slot.status = 0;
    if !self.slot.image.is_null() {
      release(self.slot.image);
      self.slot.image = null_mut();
    }
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
      return Err(DecodeError::Rejected);
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
    if status != 0 || self.slot.status != 0 || self.slot.image.is_null() {
      return Err(DecodeError::Rejected);
    }
    let image = self.slot.image;
    self.slot.image = null_mut();
    let raster = copy_image(image);
    release(image);
    raster.map(|frame| vec![frame])
  }

  pub fn flush(&mut self) -> Result<Vec<Raster>, DecodeError> {
    Ok(Vec::new())
  }
}

impl Drop for Session {
  fn drop(&mut self) {
    if !self.slot.image.is_null() {
      release(self.slot.image);
      self.slot.image = null_mut();
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
    return Err(DecodeError::Rejected);
  }
  open_format(format, null_mut())
}

fn open_format(
  format: *mut c_void,
  extensions: *mut c_void,
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
    image: null_mut(),
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
    return Err(DecodeError::Rejected);
  }
  Ok(Session {
    format,
    session,
    spec,
    destination,
    pixel_format,
    extensions,
    slot,
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
    if !slot.image.is_null() {
      CFRelease(slot.image);
    }
    CFRetain(image);
    slot.image = image;
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
  fn VTIsHardwareDecodeSupported(codec: u32) -> u8;

  fn CVPixelBufferLockBaseAddress(buffer: *mut c_void, flags: u32) -> i32;
  fn CVPixelBufferUnlockBaseAddress(buffer: *mut c_void, flags: u32) -> i32;
  fn CVPixelBufferGetWidth(buffer: *mut c_void) -> usize;
  fn CVPixelBufferGetHeight(buffer: *mut c_void) -> usize;
  fn CVPixelBufferGetBytesPerRow(buffer: *mut c_void) -> usize;
  fn CVPixelBufferGetPixelFormatType(buffer: *mut c_void) -> u32;
  fn CVPixelBufferGetBaseAddress(buffer: *mut c_void) -> *mut c_void;
}
