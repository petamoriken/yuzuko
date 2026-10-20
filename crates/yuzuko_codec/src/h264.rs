//! Elementary H.264 parameter sets and slice headers.
//!
//! The bytes are an `avcC` record plus length-prefixed NALs. This module
//! does not decode picture data and does not demux a container. Windows uses
//! only the `avcC` validation. The slice headers feed VA-API on Linux.
#![cfg_attr(target_os = "windows", allow(dead_code))]

use crate::error::DecodeError;

const ZIGZAG_4: [usize; 16] =
  [0, 1, 4, 8, 5, 2, 3, 6, 9, 12, 13, 10, 7, 11, 14, 15];

const ZIGZAG_8: [usize; 64] = [
  0, 1, 8, 16, 9, 2, 3, 10, 17, 24, 32, 25, 18, 11, 4, 5, 12, 19, 26, 33, 40,
  48, 41, 34, 27, 20, 13, 6, 7, 14, 21, 28, 35, 42, 49, 56, 57, 50, 43, 36, 29,
  22, 15, 23, 30, 37, 44, 51, 58, 59, 52, 45, 38, 31, 39, 46, 53, 60, 61, 54,
  47, 55, 62, 63,
];

const DEFAULT_4X4: [[u8; 16]; 2] = [
  [
    6, 13, 20, 28, 13, 20, 28, 32, 20, 28, 32, 37, 28, 32, 37, 42,
  ],
  [
    10, 14, 20, 24, 14, 20, 24, 27, 20, 24, 27, 30, 24, 27, 30, 34,
  ],
];

const FLAT_4: [[u8; 16]; 6] = [[16; 16]; 6];
const FLAT_8: [[u8; 64]; 6] = [[16; 64]; 6];

const DEFAULT_8X8: [[u8; 64]; 2] = [
  [
    6, 10, 13, 16, 18, 23, 25, 27, 10, 11, 16, 18, 23, 25, 27, 29, 13, 16, 18,
    23, 25, 27, 29, 31, 16, 18, 23, 25, 27, 29, 31, 33, 18, 23, 25, 27, 29, 31,
    33, 36, 23, 25, 27, 29, 31, 33, 36, 38, 25, 27, 29, 31, 33, 36, 38, 40, 27,
    29, 31, 33, 36, 38, 40, 42,
  ],
  [
    9, 13, 15, 17, 19, 21, 22, 24, 13, 13, 17, 19, 21, 22, 24, 25, 15, 17, 19,
    21, 22, 24, 25, 27, 17, 19, 21, 22, 24, 25, 27, 28, 19, 21, 22, 24, 25, 27,
    28, 30, 21, 22, 24, 25, 27, 28, 30, 32, 22, 24, 25, 27, 28, 30, 32, 33, 24,
    25, 27, 28, 30, 32, 33, 35,
  ],
];

#[derive(Clone)]
#[cfg_attr(
  not(any(target_os = "linux", target_os = "windows")),
  allow(dead_code)
)]
pub struct Sps {
  pub profile_idc: u8,
  pub level_idc: u8,
  pub id: u32,
  pub chroma_format_idc: u32,
  pub log2_max_frame_num: u32,
  pub poc_type: u32,
  pub log2_max_poc_lsb: u32,
  pub delta_pic_order_always_zero: bool,
  pub num_ref_frames: u32,
  pub gaps_in_frame_num_allowed: bool,
  pub mb_width: u32,
  pub mb_height: u32,
  pub frame_mbs_only: bool,
  pub mb_aff: bool,
  pub direct_8x8: bool,
  pub crop_left: u32,
  pub crop_right: u32,
  pub crop_top: u32,
  pub crop_bottom: u32,
  pub matrix4: [[u8; 16]; 6],
  pub matrix8: [[u8; 64]; 6],
}

impl Sps {
  pub fn coded_size(&self) -> (u32, u32) {
    (
      self.mb_width.saturating_mul(16),
      self.mb_height.saturating_mul(16),
    )
  }

  pub fn visible_rect(&self) -> (u32, u32, u32, u32) {
    let (sub_x, sub_y) = match self.chroma_format_idc {
      0 | 3 => (1, 1),
      2 => (2, 1),
      _ => (2, 2),
    };
    let unit_y = sub_y * if self.frame_mbs_only { 1 } else { 2 };
    let x = self.crop_left.saturating_mul(sub_x);
    let y = self.crop_top.saturating_mul(unit_y);
    let (width, height) = self.coded_size();
    let crop_x = self.crop_left.saturating_add(self.crop_right) * sub_x;
    let crop_y = self.crop_top.saturating_add(self.crop_bottom) * unit_y;
    (
      x,
      y,
      width.saturating_sub(crop_x),
      height.saturating_sub(crop_y),
    )
  }

  #[cfg(test)]
  pub fn visible_size(&self) -> (u32, u32) {
    let (_x, _y, width, height) = self.visible_rect();
    (width, height)
  }
}

#[derive(Clone)]
#[cfg_attr(
  not(any(target_os = "linux", target_os = "windows")),
  allow(dead_code)
)]
pub struct Pps {
  pub id: u32,
  pub sps_id: u32,
  pub cabac: bool,
  pub pic_order_present: bool,
  pub num_ref_idx_l0: u32,
  pub num_ref_idx_l1: u32,
  pub weighted_pred: bool,
  pub weighted_bipred_idc: u32,
  pub init_qp_minus26: i32,
  pub init_qs_minus26: i32,
  pub chroma_qp_index_offset: i32,
  pub second_chroma_qp_index_offset: i32,
  pub deblocking: bool,
  pub constrained_intra: bool,
  pub redundant_pic_cnt: bool,
  pub transform_8x8: bool,
  pub matrix4: [[u8; 16]; 6],
  pub matrix8: [[u8; 64]; 6],
}

#[derive(Clone)]
pub struct Sets {
  sps: Vec<Sps>,
  pps: Vec<Pps>,
}

impl Sets {
  pub fn parse_avcc(bytes: &[u8]) -> Result<Self, DecodeError> {
    let mut sets = Self {
      sps: Vec::new(),
      pps: Vec::new(),
    };
    for nal in avcc_nals(bytes)? {
      sets.add_parameter_nal(&nal)?;
    }
    if sets.sps.is_empty() || sets.pps.is_empty() {
      return Err(DecodeError::Rejected);
    }
    Ok(sets)
  }

  pub fn sps(&self, id: u32) -> Option<&Sps> {
    self.sps.iter().find(|sps| sps.id == id)
  }

  pub fn pps(&self, id: u32) -> Option<&Pps> {
    self.pps.iter().find(|pps| pps.id == id)
  }

  /// The first SPS, used to size a hardware surface.
  pub fn primary_sps(&self) -> Result<&Sps, DecodeError> {
    self.sps.first().ok_or(DecodeError::Rejected)
  }

  fn add_parameter_nal(&mut self, nal: &[u8]) -> Result<(), DecodeError> {
    let kind = nal_type(nal)?;
    if kind == 7 {
      let sps = parse_sps(nal)?;
      if let Some(slot) = self.sps.iter_mut().find(|item| item.id == sps.id) {
        *slot = sps;
      } else {
        self.sps.push(sps);
      }
    } else if kind == 8 {
      let pps = parse_pps(nal, self)?;
      if let Some(slot) = self.pps.iter_mut().find(|item| item.id == pps.id) {
        *slot = pps;
      } else {
        self.pps.push(pps);
      }
    }
    Ok(())
  }
}

#[cfg_attr(
  not(any(target_os = "linux", target_os = "windows")),
  allow(dead_code)
)]
pub struct Weights {
  pub luma_log2: u8,
  pub chroma_log2: u8,
  pub luma_flag: [bool; 2],
  pub chroma_flag: [bool; 2],
  pub luma_weight: [[i16; 32]; 2],
  pub luma_offset: [[i16; 32]; 2],
  pub chroma_weight: [[[i16; 2]; 32]; 2],
  pub chroma_offset: [[[i16; 2]; 32]; 2],
}

#[cfg_attr(
  not(any(target_os = "linux", target_os = "windows")),
  allow(dead_code)
)]
pub struct Slice {
  /// Original NAL, including emulation-prevention bytes and the NAL header.
  pub bytes: Vec<u8>,
  /// Bits from the start of the NAL header through the slice header, counted
  /// after emulation-prevention bytes are removed.
  pub header_bits: u16,
  pub first_mb: u32,
  /// H.264 `slice_type` reduced to 0..=4.
  pub slice_type: u8,
  pub nal_ref_idc: u8,
  pub idr: bool,
  pub frame_num: u32,
  pub field_pic: bool,
  pub poc_lsb: u32,
  pub delta_bottom: i32,
  pub direct_spatial: u8,
  pub ref_l0: u32,
  pub ref_l1: u32,
  pub cabac_init_idc: u8,
  pub qp_delta: i32,
  pub disable_deblocking: u8,
  pub alpha: i32,
  pub beta: i32,
  pub long_term: bool,
  pub pps_id: u32,
  pub weights: Weights,
}

pub struct Picture {
  pub slices: Vec<Slice>,
}

/// Split one length-prefixed access unit. In-band SPS and PPS update `sets`.
pub fn parse_access_unit(
  bytes: &[u8],
  sets: &mut Sets,
) -> Result<Picture, DecodeError> {
  if bytes.is_empty() {
    return Err(DecodeError::Rejected);
  }
  let mut slices = Vec::new();
  let mut offset = 0;
  while offset < bytes.len() {
    if offset + 4 > bytes.len() {
      return Err(DecodeError::Rejected);
    }
    let length = u32::from_be_bytes([
      bytes[offset],
      bytes[offset + 1],
      bytes[offset + 2],
      bytes[offset + 3],
    ]) as usize;
    offset += 4;
    if length == 0 || offset + length > bytes.len() {
      return Err(DecodeError::Rejected);
    }
    let nal = &bytes[offset..offset + length];
    offset += length;
    let kind = nal_type(nal)?;
    match kind {
      1 | 5 => slices.push(parse_slice(nal, sets)?),
      6..=12 => sets.add_parameter_nal(nal)?,
      _ => return Err(DecodeError::Rejected),
    }
  }
  if slices.is_empty() {
    return Err(DecodeError::Rejected);
  }
  Ok(Picture { slices })
}

/// SPS and PPS NAL units from an `avcC`, in declaration order.
pub fn avcc_nals(bytes: &[u8]) -> Result<Vec<Vec<u8>>, DecodeError> {
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
  if sps_count == 0 {
    return Err(DecodeError::Rejected);
  }
  let mut nals = Vec::new();
  for _ in 0..sps_count {
    nals.push(take_nal(bytes, &mut offset)?);
  }
  if offset >= bytes.len() {
    return Err(DecodeError::Rejected);
  }
  let pps_count = usize::from(bytes[offset]);
  offset += 1;
  if pps_count == 0 {
    return Err(DecodeError::Rejected);
  }
  for _ in 0..pps_count {
    nals.push(take_nal(bytes, &mut offset)?);
  }
  Ok(nals)
}

const START_CODE: [u8; 4] = [0, 0, 0, 1];

/// The SPS and PPS of an `avcC` as an Annex B byte stream, each NAL after a
/// four-byte start code. Media Foundation reads its sequence header so.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub fn annex_b_header(avcc: &[u8]) -> Result<Vec<u8>, DecodeError> {
  let mut out = Vec::new();
  for nal in avcc_nals(avcc)? {
    out.extend_from_slice(&START_CODE);
    out.extend_from_slice(&nal);
  }
  Ok(out)
}

/// An access unit of four-byte length-prefixed NALs, the `avc1` sample
/// layout, rewritten as an Annex B byte stream.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub fn annex_b_sample(sample: &[u8]) -> Result<Vec<u8>, DecodeError> {
  let mut out = Vec::with_capacity(sample.len());
  let mut offset = 0;
  while offset < sample.len() {
    let header = sample
      .get(offset..offset + 4)
      .ok_or(DecodeError::Rejected)?;
    let length =
      u32::from_be_bytes([header[0], header[1], header[2], header[3]]) as usize;
    offset += 4;
    let nal = offset
      .checked_add(length)
      .and_then(|end| sample.get(offset..end))
      .filter(|nal| !nal.is_empty())
      .ok_or(DecodeError::Rejected)?;
    out.extend_from_slice(&START_CODE);
    out.extend_from_slice(nal);
    offset += length;
  }
  if out.is_empty() {
    return Err(DecodeError::Rejected);
  }
  Ok(out)
}

/// Whether a length-prefixed access unit holds an IDR slice (NAL type 5).
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub fn has_idr(sample: &[u8]) -> bool {
  let mut offset = 0;
  while let Some(header) = sample.get(offset..offset + 4) {
    let length =
      u32::from_be_bytes([header[0], header[1], header[2], header[3]]) as usize;
    offset += 4;
    if sample.get(offset).is_some_and(|byte| byte & 0x1f == 5) {
      return true;
    }
    offset = offset.saturating_add(length);
  }
  false
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

fn nal_type(nal: &[u8]) -> Result<u8, DecodeError> {
  let header = *nal.first().ok_or(DecodeError::Rejected)?;
  if header & 0x80 != 0 {
    return Err(DecodeError::Rejected);
  }
  Ok(header & 0x1f)
}

fn parse_sps(nal: &[u8]) -> Result<Sps, DecodeError> {
  let rbsp = rbsp_bytes(nal);
  let mut bits = Bits::new(&rbsp);
  bits.bits(8)?;
  let profile_idc = bits.bits(8)? as u8;
  bits.bits(8)?;
  let level_idc = bits.bits(8)? as u8;
  let id = bits.ue()?;
  if id > 31 {
    return Err(DecodeError::Rejected);
  }
  let mut chroma_format_idc = 1u32;
  let mut matrix4 = [[16u8; 16]; 6];
  let mut matrix8 = [[16u8; 64]; 6];
  if high_profile(profile_idc) {
    chroma_format_idc = bits.ue()?;
    if chroma_format_idc > 3 {
      return Err(DecodeError::Rejected);
    }
    if chroma_format_idc == 3 && bits.bit()? {
      return Err(DecodeError::Rejected);
    }
    let bit_depth_luma = bits.ue()?.saturating_add(8);
    let bit_depth_chroma = bits.ue()?.saturating_add(8);
    if bit_depth_luma != 8 || bit_depth_chroma != 8 {
      return Err(DecodeError::Rejected);
    }
    let _transform_bypass = bits.bit()?;
    let present = bits.bit()?;
    let mut matrices = Matrices {
      fallback4: &FLAT_4,
      fallback8: &FLAT_8,
      matrix4: &mut matrix4,
      matrix8: &mut matrix8,
    };
    decode_scaling_matrices(
      &mut bits,
      present,
      true,
      chroma_format_idc,
      false,
      &mut matrices,
    )?;
  }
  if chroma_format_idc != 1 {
    return Err(DecodeError::Rejected);
  }
  let log2_max_frame_num = bits.ue()?.saturating_add(4);
  if !(4..=16).contains(&log2_max_frame_num) {
    return Err(DecodeError::Rejected);
  }
  let poc_type = bits.ue()?;
  let mut log2_max_poc_lsb = 0u32;
  let mut delta_pic_order_always_zero = false;
  if poc_type == 0 {
    log2_max_poc_lsb = bits.ue()?.saturating_add(4);
    if log2_max_poc_lsb > 16 {
      return Err(DecodeError::Rejected);
    }
  } else if poc_type == 1 {
    delta_pic_order_always_zero = bits.bit()?;
    let _offset_non_ref = bits.se()?;
    let _offset_top_bottom = bits.se()?;
    let cycle = bits.ue()?;
    if cycle > 255 {
      return Err(DecodeError::Rejected);
    }
    for _ in 0..cycle {
      let _offset = bits.se()?;
    }
  } else if poc_type != 2 {
    return Err(DecodeError::Rejected);
  }
  let num_ref_frames = bits.ue()?;
  if num_ref_frames > 16 {
    return Err(DecodeError::Rejected);
  }
  let gaps = bits.bit()?;
  let mb_width = bits.ue()?.saturating_add(1);
  let mut mb_height = bits.ue()?.saturating_add(1);
  let frame_mbs_only = bits.bit()?;
  if !frame_mbs_only {
    mb_height = mb_height.saturating_mul(2);
  }
  if mb_width == 0
    || mb_height == 0
    || mb_width > 1024
    || mb_height > 1024
    || u64::from(mb_width) * u64::from(mb_height) > 16_384 * 16_384 / 256
  {
    return Err(DecodeError::Rejected);
  }
  let mb_aff = if frame_mbs_only { false } else { bits.bit()? };
  let direct_8x8 = bits.bit()?;
  let mut crop_left = 0;
  let mut crop_right = 0;
  let mut crop_top = 0;
  let mut crop_bottom = 0;
  if bits.bit()? {
    crop_left = bits.ue()?;
    crop_right = bits.ue()?;
    crop_top = bits.ue()?;
    crop_bottom = bits.ue()?;
  }
  Ok(Sps {
    profile_idc,
    level_idc,
    id,
    chroma_format_idc,
    log2_max_frame_num,
    poc_type,
    log2_max_poc_lsb,
    delta_pic_order_always_zero,
    num_ref_frames,
    gaps_in_frame_num_allowed: gaps,
    mb_width,
    mb_height,
    frame_mbs_only,
    mb_aff,
    direct_8x8,
    crop_left,
    crop_right,
    crop_top,
    crop_bottom,
    matrix4,
    matrix8,
  })
}

fn high_profile(profile: u8) -> bool {
  matches!(
    profile,
    100 | 110 | 122 | 244 | 44 | 83 | 86 | 118 | 128 | 138 | 144
  )
}

fn parse_pps(nal: &[u8], sets: &Sets) -> Result<Pps, DecodeError> {
  let rbsp = rbsp_bytes(nal);
  let mut bits = Bits::new(&rbsp);
  bits.bits(8)?;
  let id = bits.ue()?;
  if id > 255 {
    return Err(DecodeError::Rejected);
  }
  let sps_id = bits.ue()?;
  let Some(sps) = sets.sps(sps_id) else {
    return Err(DecodeError::Rejected);
  };
  let cabac = bits.bit()?;
  let pic_order_present = bits.bit()?;
  let slice_groups = bits.ue()?.saturating_add(1);
  if slice_groups != 1 {
    return Err(DecodeError::Rejected);
  }
  let num_ref_idx_l0 = bits.ue()?.saturating_add(1);
  let num_ref_idx_l1 = bits.ue()?.saturating_add(1);
  if num_ref_idx_l0 > 32 || num_ref_idx_l1 > 32 {
    return Err(DecodeError::Rejected);
  }
  let weighted_pred = bits.bit()?;
  let weighted_bipred_idc = bits.bits(2)?;
  let init_qp_minus26 = bits.se()?;
  let init_qs_minus26 = bits.se()?;
  let chroma_qp = bits.se()?;
  if !(-12..=12).contains(&chroma_qp) {
    return Err(DecodeError::Rejected);
  }
  let deblocking = bits.bit()?;
  let constrained_intra = bits.bit()?;
  let redundant_pic_cnt = bits.bit()?;
  let mut transform_8x8 = false;
  let mut second = chroma_qp;
  let mut matrix4 = sps.matrix4;
  let mut matrix8 = sps.matrix8;
  if more_rbsp(&bits) && high_profile(sps.profile_idc) {
    transform_8x8 = bits.bit()?;
    let present = bits.bit()?;
    let mut matrices = Matrices {
      fallback4: &sps.matrix4,
      fallback8: &sps.matrix8,
      matrix4: &mut matrix4,
      matrix8: &mut matrix8,
    };
    decode_scaling_matrices(
      &mut bits,
      present,
      false,
      sps.chroma_format_idc,
      transform_8x8,
      &mut matrices,
    )?;
    second = bits.se()?;
    if !(-12..=12).contains(&second) {
      return Err(DecodeError::Rejected);
    }
  }
  Ok(Pps {
    id,
    sps_id,
    cabac,
    pic_order_present,
    num_ref_idx_l0,
    num_ref_idx_l1,
    weighted_pred,
    weighted_bipred_idc,
    init_qp_minus26,
    init_qs_minus26,
    chroma_qp_index_offset: chroma_qp,
    second_chroma_qp_index_offset: second,
    deblocking,
    constrained_intra,
    redundant_pic_cnt,
    transform_8x8,
    matrix4,
    matrix8,
  })
}

fn parse_slice(nal: &[u8], sets: &Sets) -> Result<Slice, DecodeError> {
  let rbsp = rbsp_bytes(nal);
  let mut bits = Bits::new(&rbsp);
  let header = bits.bits(8)?;
  let nal_ref_idc = ((header >> 5) & 0x03) as u8;
  let nal_type = (header & 0x1f) as u8;
  let idr = nal_type == 5;
  let first_mb = bits.ue()?;
  let slice_type_raw = bits.ue()?;
  if slice_type_raw > 9 {
    return Err(DecodeError::Rejected);
  }
  let slice_type = (slice_type_raw % 5) as u8;
  let pps_id = bits.ue()?;
  let Some(pps) = sets.pps(pps_id) else {
    return Err(DecodeError::Rejected);
  };
  let Some(sps) = sets.sps(pps.sps_id) else {
    return Err(DecodeError::Rejected);
  };
  let frame_num = bits.bits(sps.log2_max_frame_num)?;
  let mut field_pic = false;
  if !sps.frame_mbs_only {
    field_pic = bits.bit()?;
    if field_pic {
      let _bottom = bits.bit()?;
    }
  }
  if idr {
    let _idr_pic_id = bits.ue()?;
  }
  let mut poc_lsb = 0u32;
  let mut delta_bottom = 0i32;
  if sps.poc_type == 0 {
    poc_lsb = bits.bits(sps.log2_max_poc_lsb)?;
    if pps.pic_order_present && !field_pic {
      delta_bottom = bits.se()?;
    }
  } else if sps.poc_type == 1 && !sps.delta_pic_order_always_zero {
    let _delta0 = bits.se()?;
    if pps.pic_order_present && !field_pic {
      let _delta1 = bits.se()?;
    }
  }
  if pps.redundant_pic_cnt {
    let _redundant = bits.ue()?;
  }
  let mut direct_spatial = 0u8;
  if slice_type == 1 {
    direct_spatial = u8::from(bits.bit()?);
  }
  let mut ref_l0 = 0u32;
  let mut ref_l1 = 0u32;
  if matches!(slice_type, 0 | 1 | 3) {
    ref_l0 = pps.num_ref_idx_l0;
    ref_l1 = if slice_type == 1 {
      pps.num_ref_idx_l1
    } else {
      0
    };
    if bits.bit()? {
      ref_l0 = bits.ue()?.saturating_add(1);
      if slice_type == 1 {
        ref_l1 = bits.ue()?.saturating_add(1);
      }
    }
  }
  if ref_l0 > 32 || ref_l1 > 32 {
    return Err(DecodeError::Rejected);
  }
  if slice_type != 2 && slice_type != 4 && bits.bit()? {
    skip_list_modification(&mut bits)?;
  }
  if slice_type == 1 && bits.bit()? {
    skip_list_modification(&mut bits)?;
  }
  let mut weights = default_weights();
  let weighted = (pps.weighted_pred && matches!(slice_type, 0 | 3))
    || (pps.weighted_bipred_idc == 1 && slice_type == 1);
  if weighted {
    weights = parse_weights(&mut bits, sps.chroma_format_idc, ref_l0, ref_l1)?;
  }
  let mut long_term = false;
  if nal_ref_idc != 0 {
    if idr {
      let _no_output = bits.bit()?;
      long_term = bits.bit()?;
    } else if bits.bit()? {
      skip_marking(&mut bits)?;
    }
  }
  let mut cabac_init_idc = 0u8;
  if pps.cabac && slice_type != 2 && slice_type != 4 {
    let idc = bits.ue()?;
    if idc > 2 {
      return Err(DecodeError::Rejected);
    }
    cabac_init_idc = idc as u8;
  }
  let qp_delta = bits.se()?;
  if slice_type == 3 || slice_type == 4 {
    if slice_type == 3 {
      let _sp = bits.bit()?;
    }
    let _qs = bits.se()?;
  }
  let mut disable_deblocking = 0u8;
  let mut alpha = 0i32;
  let mut beta = 0i32;
  if pps.deblocking {
    let idc = bits.ue()?;
    if idc > 2 {
      return Err(DecodeError::Rejected);
    }
    disable_deblocking = idc as u8;
    if idc != 1 {
      alpha = bits.se()?;
      beta = bits.se()?;
    }
  }
  if bits.pos > u16::MAX as usize {
    return Err(DecodeError::Rejected);
  }
  Ok(Slice {
    bytes: nal.to_vec(),
    header_bits: bits.pos as u16,
    first_mb,
    slice_type,
    nal_ref_idc,
    idr,
    frame_num,
    field_pic,
    poc_lsb,
    delta_bottom,
    direct_spatial,
    ref_l0,
    ref_l1,
    cabac_init_idc,
    qp_delta,
    disable_deblocking,
    alpha,
    beta,
    long_term,
    pps_id,
    weights,
  })
}

fn skip_list_modification(bits: &mut Bits<'_>) -> Result<(), DecodeError> {
  for _ in 0..64 {
    let idc = bits.ue()?;
    if idc == 3 {
      return Ok(());
    }
    if idc > 5 {
      return Err(DecodeError::Rejected);
    }
    let _diff = bits.ue()?;
  }
  Err(DecodeError::Rejected)
}

fn skip_marking(bits: &mut Bits<'_>) -> Result<(), DecodeError> {
  for _ in 0..64 {
    let op = bits.ue()?;
    if op == 0 {
      return Ok(());
    }
    if op > 6 {
      return Err(DecodeError::Rejected);
    }
    if matches!(op, 1 | 2 | 3 | 4 | 6) {
      let _arg = bits.ue()?;
    }
    if op == 3 {
      let _arg = bits.ue()?;
    }
  }
  Err(DecodeError::Rejected)
}

fn default_weights() -> Weights {
  Weights {
    luma_log2: 0,
    chroma_log2: 0,
    luma_flag: [false; 2],
    chroma_flag: [false; 2],
    luma_weight: [[1; 32]; 2],
    luma_offset: [[0; 32]; 2],
    chroma_weight: [[[1; 2]; 32]; 2],
    chroma_offset: [[[0; 2]; 32]; 2],
  }
}

fn parse_weights(
  bits: &mut Bits<'_>,
  chroma: u32,
  ref_l0: u32,
  ref_l1: u32,
) -> Result<Weights, DecodeError> {
  let mut weights = default_weights();
  let luma_log2 = bits.ue()?;
  if luma_log2 > 7 {
    return Err(DecodeError::Rejected);
  }
  weights.luma_log2 = luma_log2 as u8;
  let luma_default = 1i16 << luma_log2;
  if chroma != 0 {
    let chroma_log2 = bits.ue()?;
    if chroma_log2 > 7 {
      return Err(DecodeError::Rejected);
    }
    weights.chroma_log2 = chroma_log2 as u8;
  }
  let chroma_default = 1i16 << weights.chroma_log2;
  for list in 0..2 {
    let count = if list == 0 { ref_l0 } else { ref_l1 };
    if count == 0 {
      continue;
    }
    weights.luma_weight[list] = [luma_default; 32];
    weights.chroma_weight[list] = [[chroma_default; 2]; 32];
    let mut luma_flag = false;
    let mut chroma_flag = false;
    for index in 0..count as usize {
      if bits.bit()? {
        luma_flag = true;
        weights.luma_weight[list][index] = i16_from(bits.se()?)?;
        weights.luma_offset[list][index] = i16_from(bits.se()?)?;
      }
      if chroma != 0 && bits.bit()? {
        chroma_flag = true;
        for plane in 0..2 {
          weights.chroma_weight[list][index][plane] = i16_from(bits.se()?)?;
          weights.chroma_offset[list][index][plane] = i16_from(bits.se()?)?;
        }
      }
    }
    weights.luma_flag[list] = luma_flag;
    weights.chroma_flag[list] = chroma_flag;
  }
  Ok(weights)
}

fn i16_from(value: i32) -> Result<i16, DecodeError> {
  i16::try_from(value).map_err(|_| DecodeError::Rejected)
}

struct Matrices<'a> {
  fallback4: &'a [[u8; 16]; 6],
  fallback8: &'a [[u8; 64]; 6],
  matrix4: &'a mut [[u8; 16]; 6],
  matrix8: &'a mut [[u8; 64]; 6],
}

fn decode_scaling_matrices(
  bits: &mut Bits<'_>,
  present: bool,
  is_sps: bool,
  chroma: u32,
  transform_8x8: bool,
  matrices: &mut Matrices<'_>,
) -> Result<(), DecodeError> {
  if !present {
    return Ok(());
  }
  let fb4_intra = if is_sps {
    DEFAULT_4X4[0]
  } else {
    matrices.fallback4[0]
  };
  let fb4_inter = if is_sps {
    DEFAULT_4X4[1]
  } else {
    matrices.fallback4[3]
  };
  let fb8_intra = if is_sps {
    DEFAULT_8X8[0]
  } else {
    matrices.fallback8[0]
  };
  let fb8_inter = if is_sps {
    DEFAULT_8X8[1]
  } else {
    matrices.fallback8[3]
  };
  let matrix4 = &mut *matrices.matrix4;
  let matrix8 = &mut *matrices.matrix8;
  decode_list_4(bits, &mut matrix4[0], &DEFAULT_4X4[0], &fb4_intra)?;
  let copy = matrix4[0];
  decode_list_4(bits, &mut matrix4[1], &DEFAULT_4X4[0], &copy)?;
  let copy = matrix4[1];
  decode_list_4(bits, &mut matrix4[2], &DEFAULT_4X4[0], &copy)?;
  decode_list_4(bits, &mut matrix4[3], &DEFAULT_4X4[1], &fb4_inter)?;
  let copy = matrix4[3];
  decode_list_4(bits, &mut matrix4[4], &DEFAULT_4X4[1], &copy)?;
  let copy = matrix4[4];
  decode_list_4(bits, &mut matrix4[5], &DEFAULT_4X4[1], &copy)?;
  if is_sps || transform_8x8 {
    decode_list_8(bits, &mut matrix8[0], &DEFAULT_8X8[0], &fb8_intra)?;
    decode_list_8(bits, &mut matrix8[3], &DEFAULT_8X8[1], &fb8_inter)?;
    if chroma == 3 {
      let copy = matrix8[0];
      decode_list_8(bits, &mut matrix8[1], &DEFAULT_8X8[0], &copy)?;
      let copy = matrix8[3];
      decode_list_8(bits, &mut matrix8[4], &DEFAULT_8X8[1], &copy)?;
      let copy = matrix8[1];
      decode_list_8(bits, &mut matrix8[2], &DEFAULT_8X8[0], &copy)?;
      let copy = matrix8[4];
      decode_list_8(bits, &mut matrix8[5], &DEFAULT_8X8[1], &copy)?;
    }
  }
  Ok(())
}

fn decode_list_4(
  bits: &mut Bits<'_>,
  dest: &mut [u8; 16],
  preset: &[u8; 16],
  fallback: &[u8; 16],
) -> Result<(), DecodeError> {
  if !bits.bit()? {
    *dest = *fallback;
    return Ok(());
  }
  fill_list(bits, dest, &ZIGZAG_4, preset)
}

fn decode_list_8(
  bits: &mut Bits<'_>,
  dest: &mut [u8; 64],
  preset: &[u8; 64],
  fallback: &[u8; 64],
) -> Result<(), DecodeError> {
  if !bits.bit()? {
    *dest = *fallback;
    return Ok(());
  }
  fill_list(bits, dest, &ZIGZAG_8, preset)
}

fn fill_list<const N: usize>(
  bits: &mut Bits<'_>,
  dest: &mut [u8; N],
  scan: &[usize; N],
  preset: &[u8; N],
) -> Result<(), DecodeError> {
  let mut last = 8i32;
  let mut next = 8i32;
  for (index, slot) in scan.iter().enumerate() {
    if next != 0 {
      let delta = bits.se()?;
      if !(-128..=127).contains(&delta) {
        return Err(DecodeError::Rejected);
      }
      next = (last + delta) & 0xff;
    }
    if index == 0 && next == 0 {
      *dest = *preset;
      return Ok(());
    }
    last = if next == 0 { last } else { next };
    dest[*slot] = last as u8;
  }
  Ok(())
}

fn more_rbsp(bits: &Bits<'_>) -> bool {
  let total = bits.data.len() * 8;
  if bits.pos >= total {
    return false;
  }
  if !bits.peek(0).unwrap_or(false) {
    return true;
  }
  (1..(total - bits.pos)).any(|ahead| bits.peek(ahead).unwrap_or(false))
}

fn rbsp_bytes(nal: &[u8]) -> Vec<u8> {
  let mut out = Vec::with_capacity(nal.len());
  let mut zeros = 0u8;
  for &byte in nal {
    if zeros == 2 && byte == 3 {
      zeros = 0;
      continue;
    }
    out.push(byte);
    zeros = if byte == 0 {
      zeros.saturating_add(1).min(2)
    } else {
      0
    };
  }
  out
}

struct Bits<'a> {
  data: &'a [u8],
  pos: usize,
}

impl<'a> Bits<'a> {
  fn new(data: &'a [u8]) -> Self {
    Self { data, pos: 0 }
  }

  fn bit(&mut self) -> Result<bool, DecodeError> {
    let value = self.peek(0).ok_or(DecodeError::Rejected)?;
    self.pos += 1;
    Ok(value)
  }

  fn peek(&self, ahead: usize) -> Option<bool> {
    let index = self.pos + ahead;
    let byte = *self.data.get(index / 8)?;
    let shift = 7 - (index % 8);
    Some(((byte >> shift) & 1) == 1)
  }

  fn bits(&mut self, count: u32) -> Result<u32, DecodeError> {
    if count > 32 {
      return Err(DecodeError::Rejected);
    }
    let mut value = 0u32;
    for _ in 0..count {
      value = (value << 1) | u32::from(self.bit()?);
    }
    Ok(value)
  }

  fn ue(&mut self) -> Result<u32, DecodeError> {
    let mut zeros = 0u32;
    while !self.bit()? {
      zeros += 1;
      if zeros > 31 {
        return Err(DecodeError::Rejected);
      }
    }
    if zeros == 0 {
      return Ok(0);
    }
    let rest = self.bits(zeros)?;
    Ok(((1u32 << zeros) - 1).saturating_add(rest))
  }

  fn se(&mut self) -> Result<i32, DecodeError> {
    let code = self.ue()?;
    let magnitude = i32::try_from((code + 1) >> 1).unwrap_or(i32::MAX);
    if code & 1 == 0 {
      Ok(-magnitude)
    } else {
      Ok(magnitude)
    }
  }
}

#[cfg(test)]
mod tests {
  use super::{
    Sets, annex_b_header, annex_b_sample, avcc_nals, has_idr, parse_access_unit,
  };

  fn read(name: &str) -> Vec<u8> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
      .join("../../tests/fixtures/webcodecs")
      .join(name);
    std::fs::read(path).expect(name)
  }

  #[test]
  fn fixture_sps_is_64_square_and_idr_parses() {
    let mut sets = Sets::parse_avcc(&read("avc1.avcC")).expect("avcC");
    let sps = sets.primary_sps().expect("sps");
    assert_eq!(sps.coded_size(), (192, 96));
    assert_eq!(sps.visible_size(), (64, 64));
    assert_eq!(sps.profile_idc, 100);
    assert_eq!(sps.chroma_format_idc, 1);
    let picture = parse_access_unit(&read("avc1.idr"), &mut sets).expect("idr");
    assert_eq!(picture.slices.len(), 1);
    let slice = &picture.slices[0];
    assert!(slice.idr);
    assert_eq!(slice.slice_type, 2);
    assert_eq!(slice.first_mb, 0);
    assert!(slice.header_bits > 8);
    assert!(slice.header_bits as usize <= slice.bytes.len() * 8);
  }

  #[test]
  fn truncated_sps_is_rejected() {
    let bytes = read("avc1-bad.avcC");
    assert!(avcc_nals(&bytes).is_ok());
    assert!(Sets::parse_avcc(&bytes).is_err());
  }

  #[test]
  fn avcc_and_samples_become_annex_b() {
    let avcc = read("avc1.avcC");
    let header = annex_b_header(&avcc).expect("header");
    let mut expected = Vec::new();
    for nal in avcc_nals(&avcc).unwrap() {
      expected.extend_from_slice(&[0, 0, 0, 1]);
      expected.extend_from_slice(&nal);
    }
    assert_eq!(header, expected);

    let idr = read("avc1.idr");
    let stream = annex_b_sample(&idr).expect("sample");
    assert_eq!(stream.len(), idr.len());
    assert_eq!(&stream[..4], &[0, 0, 0, 1]);
    assert_eq!(&stream[4..], &idr[4..4 + (stream.len() - 4)]);

    assert!(has_idr(&idr));
    assert!(!has_idr(&[0, 0, 0, 1, 0x41]));
    assert!(!has_idr(&[]));

    assert!(annex_b_sample(&[]).is_err());
    assert!(annex_b_sample(&[0, 0, 0, 9, 1]).is_err());
    assert!(annex_b_sample(&[0, 0, 0, 0]).is_err());
    assert_eq!(
      annex_b_sample(&[0, 0, 0, 1, 7, 0, 0, 0, 2, 8, 9]).unwrap(),
      vec![0, 0, 0, 1, 7, 0, 0, 0, 1, 8, 9]
    );
  }
}
