//! AV1 sequence and frame headers for the Linux VA-API decoder.
//!
//! VA-API does not accept an OBU sequence. This parser reads the
//! uncompressed header and splits out each tile. The driver decodes the
//! tile bytes. A session opens for profile 0, 8-bit, 4:2:0. Superres and a
//! non-zero scalability layer are rejected.

use crate::error::DecodeError;

const PRIMARY_NONE: u8 = 7;
const MAX_TILE_COLS: u32 = 64;
const MAX_TILE_ROWS: u32 = 64;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Select {
  Off,
  On,
  Adaptive,
}

#[derive(Clone, Copy)]
struct OperatingPoint {
  idc: u16,
  decoder_model: bool,
}

/// Sequence header fields the picture parameter buffer repeats.
#[derive(Clone)]
#[allow(dead_code)]
pub(crate) struct Sequence {
  pub(crate) profile: u8,
  pub(crate) still_picture: bool,
  pub(crate) reduced_still: bool,
  pub(crate) max_width: u32,
  pub(crate) max_height: u32,
  pub(crate) bit_depth: u8,
  pub(crate) monochrome: bool,
  pub(crate) full_range: bool,
  pub(crate) subsampling_x: bool,
  pub(crate) subsampling_y: bool,
  pub(crate) chroma_sample_position: u8,
  pub(crate) matrix: u8,
  pub(crate) sb128: bool,
  pub(crate) filter_intra: bool,
  pub(crate) intra_edge_filter: bool,
  pub(crate) inter_intra: bool,
  pub(crate) masked_compound: bool,
  pub(crate) warped_motion: bool,
  pub(crate) dual_filter: bool,
  pub(crate) order_hint: bool,
  pub(crate) jnt_comp: bool,
  pub(crate) ref_frame_mvs: bool,
  pub(crate) screen_content_tools: Select,
  pub(crate) force_integer_mv: Select,
  pub(crate) order_hint_bits: u8,
  pub(crate) superres: bool,
  pub(crate) cdef: bool,
  pub(crate) restoration: bool,
  pub(crate) film_grain: bool,
  pub(crate) separate_uv_delta_q: bool,
  width_bits: u8,
  height_bits: u8,
  frame_id_present: bool,
  frame_id_bits: u8,
  delta_frame_id_bits: u8,
  decoder_model: bool,
  equal_picture_interval: bool,
  buffer_removal_bits: u8,
  presentation_delay_bits: u8,
  operating_points: Vec<OperatingPoint>,
}

impl Sequence {
  pub(crate) fn hardware_profile(&self) -> bool {
    self.profile == 0
      && self.bit_depth == 8
      && !self.monochrome
      && self.subsampling_x
      && self.subsampling_y
      && self.max_width > 0
      && self.max_height > 0
  }
}

#[derive(Clone)]
#[allow(dead_code)]
struct Segments {
  enabled: bool,
  update_map: bool,
  temporal: bool,
  update_data: bool,
  data: [[i16; 8]; 8],
  mask: [u8; 8],
  lossless: [bool; 8],
}

#[derive(Clone, Copy)]
struct LoopDeltas {
  ref_delta: [i8; 8],
  mode_delta: [i8; 2],
}

#[derive(Clone)]
#[allow(dead_code)]
struct LoopFilter {
  level_y: [u8; 2],
  level_u: u8,
  level_v: u8,
  sharpness: u8,
  delta_enabled: bool,
  delta_update: bool,
  deltas: LoopDeltas,
}

#[derive(Clone, Copy)]
#[allow(dead_code)]
struct GlobalMotion {
  kind: u32,
  matrix: [i32; 8],
}

#[derive(Clone)]
#[allow(dead_code)]
pub(crate) struct Grain {
  pub(crate) seed: u16,
  pub(crate) chroma_from_luma: bool,
  pub(crate) scaling_shift: u8,
  pub(crate) ar_lag: u8,
  pub(crate) ar_shift: u8,
  pub(crate) grain_scale_shift: u8,
  pub(crate) overlap: bool,
  pub(crate) clip: bool,
  pub(crate) num_y: u8,
  pub(crate) y_value: [u8; 14],
  pub(crate) y_scaling: [u8; 14],
  pub(crate) num_cb: u8,
  pub(crate) cb_value: [u8; 10],
  pub(crate) cb_scaling: [u8; 10],
  pub(crate) num_cr: u8,
  pub(crate) cr_value: [u8; 10],
  pub(crate) cr_scaling: [u8; 10],
  pub(crate) ar_y: [i8; 24],
  pub(crate) ar_cb: [i8; 25],
  pub(crate) ar_cr: [i8; 25],
  pub(crate) cb_mult: u8,
  pub(crate) cb_luma_mult: u8,
  pub(crate) cb_offset: u16,
  pub(crate) cr_mult: u8,
  pub(crate) cr_luma_mult: u8,
  pub(crate) cr_offset: u16,
}

#[derive(Clone)]
struct Slot {
  order_hint: u8,
  frame_id: u32,
  upscaled_width: u32,
  height: u32,
  render_width: u32,
  render_height: u32,
  segments: Segments,
  loop_filter: LoopFilter,
  gmv: [GlobalMotion; 7],
  grain: Option<Grain>,
}

#[derive(Clone)]
pub(crate) struct Tile {
  pub(crate) row: u16,
  pub(crate) column: u16,
  pub(crate) bytes: Vec<u8>,
}

/// One decoded picture. `show` is false for a hidden reference frame.
#[derive(Clone)]
#[allow(dead_code)]
pub(crate) struct Frame {
  pub(crate) show: bool,
  pub(crate) frame_type: u8,
  pub(crate) showable: bool,
  pub(crate) error_resilient: bool,
  pub(crate) disable_cdf_update: bool,
  pub(crate) allow_screen_content_tools: bool,
  pub(crate) force_integer_mv: bool,
  pub(crate) upscaled_width: u32,
  pub(crate) height: u32,
  pub(crate) coded_width: u32,
  pub(crate) superres: bool,
  pub(crate) superres_denom: u8,
  pub(crate) refresh_flags: u8,
  pub(crate) ref_idx: [u8; 7],
  pub(crate) order_hint: u8,
  pub(crate) primary_ref: u8,
  pub(crate) allow_intrabc: bool,
  pub(crate) allow_high_precision_mv: bool,
  pub(crate) interp_filter: u8,
  pub(crate) motion_mode_switchable: bool,
  pub(crate) use_ref_frame_mvs: bool,
  pub(crate) disable_frame_end_update_cdf: bool,
  pub(crate) uniform_tiles: bool,
  pub(crate) allow_warped_motion: bool,
  pub(crate) tile_cols: u8,
  pub(crate) tile_rows: u8,
  pub(crate) tile_size_bytes: u8,
  /// `TileColsLog2 + TileRowsLog2`, the width of a tile-group index.
  tile_bits: u8,
  pub(crate) width_in_sbs_minus_1: [u16; 63],
  pub(crate) height_in_sbs_minus_1: [u16; 63],
  pub(crate) context_update_tile_id: u16,
  pub(crate) filter_level: [u8; 2],
  pub(crate) filter_level_u: u8,
  pub(crate) filter_level_v: u8,
  pub(crate) sharpness: u8,
  pub(crate) loop_delta_enabled: bool,
  pub(crate) loop_delta_update: bool,
  pub(crate) ref_deltas: [i8; 8],
  pub(crate) mode_deltas: [i8; 2],
  pub(crate) base_qindex: u8,
  pub(crate) y_dc_delta_q: i8,
  pub(crate) u_dc_delta_q: i8,
  pub(crate) u_ac_delta_q: i8,
  pub(crate) v_dc_delta_q: i8,
  pub(crate) v_ac_delta_q: i8,
  pub(crate) using_qmatrix: bool,
  pub(crate) qm_y: u8,
  pub(crate) qm_u: u8,
  pub(crate) qm_v: u8,
  pub(crate) seg_enabled: bool,
  pub(crate) seg_update_map: bool,
  pub(crate) seg_temporal: bool,
  pub(crate) seg_update_data: bool,
  pub(crate) seg_data: [[i16; 8]; 8],
  pub(crate) seg_mask: [u8; 8],
  pub(crate) delta_q_present: bool,
  pub(crate) log2_delta_q_res: u8,
  pub(crate) delta_lf_present: bool,
  pub(crate) log2_delta_lf_res: u8,
  pub(crate) delta_lf_multi: bool,
  pub(crate) tx_mode: u8,
  pub(crate) reference_select: bool,
  pub(crate) skip_mode_present: bool,
  pub(crate) reduced_tx_set: bool,
  pub(crate) cdef_damping_minus_3: u8,
  pub(crate) cdef_bits: u8,
  pub(crate) cdef_y: [u8; 8],
  pub(crate) cdef_uv: [u8; 8],
  pub(crate) restoration: [u8; 3],
  pub(crate) lr_unit_shift: u8,
  pub(crate) lr_uv_shift: u8,
  pub(crate) wm_type: [u32; 7],
  pub(crate) wm_mat: [[i32; 8]; 7],
  pub(crate) apply_grain: bool,
  pub(crate) grain: Grain,
  pub(crate) tiles: Vec<Tile>,
  refresh_slot: Slot,
}

#[allow(clippy::large_enum_variant)]
pub(crate) enum Picture {
  Frame(Frame),
  /// Display a reference that was decoded earlier.
  ShowExisting {
    #[allow(dead_code)]
    slot: usize,
  },
}

#[allow(clippy::large_enum_variant)]
enum Header {
  Show(usize),
  Frame(Frame),
}

struct Pending {
  frame: Frame,
  filled: Vec<bool>,
}

pub(crate) struct Stream {
  sequence: Option<Sequence>,
  slots: [Option<Slot>; 8],
  pending: Option<Pending>,
}

impl Stream {
  pub(crate) fn new() -> Self {
    Self {
      sequence: None,
      slots: std::array::from_fn(|_| None),
      pending: None,
    }
  }

  pub(crate) fn sequence(&self) -> Option<&Sequence> {
    self.sequence.as_ref()
  }

  /// Reads every OBU in one temporal unit. A later sequence header replaces
  /// the current one when the coded size and profile stay the same.
  pub(crate) fn push(
    &mut self,
    bytes: &[u8],
  ) -> Result<Vec<Picture>, DecodeError> {
    let mut pictures = Vec::new();
    let mut pos = 0usize;
    while pos < bytes.len() {
      let (next, picture) = self.obu(bytes, pos)?;
      if next <= pos || next > bytes.len() {
        return Err(DecodeError::Rejected);
      }
      pos = next;
      if let Some(picture) = picture {
        pictures.push(picture);
      }
    }
    if self.pending.is_some() {
      return Err(DecodeError::Rejected);
    }
    Ok(pictures)
  }

  fn obu(
    &mut self,
    bytes: &[u8],
    pos: usize,
  ) -> Result<(usize, Option<Picture>), DecodeError> {
    let mut bits = Bits::new(&bytes[pos..]);
    if bits.bit()? {
      return Err(DecodeError::Rejected);
    }
    let kind = bits.bits(4)?;
    let extension = bits.bit()?;
    let has_size = bits.bit()?;
    if bits.bit()? {
      return Err(DecodeError::Rejected);
    }
    let mut temporal = 0u32;
    let mut spatial = 0u32;
    if extension {
      temporal = bits.bits(3)?;
      spatial = bits.bits(2)?;
      let _reserved = bits.bits(3)?;
    }
    if !bits.pos.is_multiple_of(8) {
      return Err(DecodeError::Rejected);
    }
    let header = bits.pos / 8;
    let (payload_at, payload_end, total) = if has_size {
      let (len, after) = leb128(&bytes[pos..], header)?;
      let end = after.checked_add(len).ok_or(DecodeError::Rejected)?;
      if end > bytes.len() - pos {
        return Err(DecodeError::Rejected);
      }
      (after, end, pos + end)
    } else {
      let end = bytes.len() - pos;
      (header, end, bytes.len())
    };
    if temporal != 0 || spatial != 0 {
      return Err(DecodeError::Rejected);
    }
    let payload = &bytes[pos + payload_at..pos + payload_end];
    let picture = match kind {
      1 => {
        self.read_sequence(payload)?;
        None
      }
      2 => {
        if self.pending.is_some() {
          return Err(DecodeError::Rejected);
        }
        None
      }
      3 => self.frame_header(payload, false)?,
      4 => self.tile_group(payload)?,
      5 | 15 => None,
      6 => self.frame_header(payload, true)?,
      7 => {
        if self.pending.is_none() {
          return Err(DecodeError::Rejected);
        }
        None
      }
      _ => return Err(DecodeError::Rejected),
    };
    Ok((total, picture))
  }

  fn read_sequence(&mut self, payload: &[u8]) -> Result<(), DecodeError> {
    let parsed = parse_sequence(payload)?;
    if let Some(current) = &self.sequence
      && (parsed.max_width != current.max_width
        || parsed.max_height != current.max_height
        || parsed.profile != current.profile
        || parsed.bit_depth != current.bit_depth
        || parsed.monochrome != current.monochrome
        || parsed.subsampling_x != current.subsampling_x
        || parsed.subsampling_y != current.subsampling_y)
    {
      return Err(DecodeError::Rejected);
    }
    self.sequence = Some(parsed);
    Ok(())
  }

  fn frame_header(
    &mut self,
    payload: &[u8],
    tiles_follow: bool,
  ) -> Result<Option<Picture>, DecodeError> {
    let sequence = self.sequence.clone().ok_or(DecodeError::Rejected)?;
    let mut bits = Bits::new(payload);
    match parse_frame(&sequence, &self.slots, &mut bits)? {
      Header::Show(slot) => {
        if tiles_follow || self.slots[slot].is_none() {
          return Err(DecodeError::Rejected);
        }
        trailing_bits(&mut bits)?;
        if !bits.rest()?.is_empty() {
          return Err(DecodeError::Rejected);
        }
        Ok(Some(Picture::ShowExisting { slot }))
      }
      Header::Frame(frame) => {
        if frame.superres {
          return Err(DecodeError::Rejected);
        }
        if !tiles_follow {
          trailing_bits(&mut bits)?;
          if !bits.rest()?.is_empty() {
            return Err(DecodeError::Rejected);
          }
          self.pending = Some(Pending {
            filled: vec![false; frame.tile_count()],
            frame,
          });
          return Ok(None);
        }
        // A frame OBU has no trailing one. Pad with zeros, then the tile
        // group, which carries its own header.
        byte_align(&mut bits)?;
        let rest = bits.rest()?.to_vec();
        self.pending = Some(Pending {
          filled: vec![false; frame.tile_count()],
          frame,
        });
        self.tile_group(&rest)
      }
    }
  }

  fn tile_group(
    &mut self,
    payload: &[u8],
  ) -> Result<Option<Picture>, DecodeError> {
    let pending = self.pending.as_mut().ok_or(DecodeError::Rejected)?;
    let mut bits = Bits::new(payload);
    let count = pending.frame.tile_count();
    let (start, end) = if count > 1 && bits.bit()? {
      let n = u32::from(pending.frame.tile_bits);
      let start = bits.bits(n)? as usize;
      let end = bits.bits(n)? as usize;
      if start > end || end >= count {
        return Err(DecodeError::Rejected);
      }
      (start, end)
    } else {
      (0, count - 1)
    };
    byte_align(&mut bits)?;
    let tiles = read_tiles(&pending.frame, start, end, &mut bits)?;
    for tile in tiles {
      let index = tile.row as usize * pending.frame.tile_cols as usize
        + tile.column as usize;
      if pending.filled.get(index).copied().unwrap_or(true) {
        return Err(DecodeError::Rejected);
      }
      pending.filled[index] = true;
      pending.frame.tiles.push(tile);
    }
    if pending.filled.iter().any(|filled| !filled) {
      return Ok(None);
    }
    let pending = self.pending.take().ok_or(DecodeError::Rejected)?;
    self.finish(pending.frame).map(Some)
  }

  fn finish(&mut self, mut frame: Frame) -> Result<Picture, DecodeError> {
    frame.tiles.sort_by_key(|tile| (tile.row, tile.column));
    if frame.tiles.len() != frame.tile_count() {
      return Err(DecodeError::Rejected);
    }
    for index in 0..8 {
      if frame.refresh_flags & (1 << index) != 0 {
        self.slots[index] = Some(frame.refresh_slot.clone());
      }
    }
    Ok(Picture::Frame(frame))
  }
}

impl Frame {
  fn tile_count(&self) -> usize {
    self.tile_cols as usize * self.tile_rows as usize
  }
}

fn parse_sequence(payload: &[u8]) -> Result<Sequence, DecodeError> {
  let mut bits = Bits::new(payload);
  let profile = bits.bits(3)? as u8;
  if profile > 2 {
    return Err(DecodeError::Rejected);
  }
  let still_picture = bits.bit()?;
  let reduced_still = bits.bit()?;
  if reduced_still && !still_picture {
    return Err(DecodeError::Rejected);
  }
  let mut operating_points = Vec::new();
  let mut decoder_model = false;
  let mut equal_picture_interval = false;
  let mut buffer_removal_bits = 0u8;
  let mut presentation_delay_bits = 0u8;
  let mut encoder_delay_bits = 0u8;
  if reduced_still {
    let _level = bits.bits(3)?;
    let _minor = bits.bits(2)?;
    operating_points.push(OperatingPoint {
      idc: 0,
      decoder_model: false,
    });
  } else {
    if bits.bit()? {
      let _units = bits.bits(32)?;
      let _scale = bits.bits(32)?;
      equal_picture_interval = bits.bit()?;
      if equal_picture_interval && bits.uvlc()? == u32::MAX {
        return Err(DecodeError::Rejected);
      }
      decoder_model = bits.bit()?;
      if decoder_model {
        encoder_delay_bits = bits.bits(5)? as u8 + 1;
        let _tick = bits.bits(32)?;
        buffer_removal_bits = bits.bits(5)? as u8 + 1;
        presentation_delay_bits = bits.bits(5)? as u8 + 1;
      }
    }
    let display_model = bits.bit()?;
    let count = bits.bits(5)? as usize + 1;
    for _ in 0..count {
      let idc = bits.bits(12)? as u16;
      if idc != 0 && (idc & 0xff == 0 || idc & 0xf00 == 0) {
        return Err(DecodeError::Rejected);
      }
      let major = 2 + bits.bits(3)?;
      let _minor = bits.bits(2)?;
      if major > 3 {
        let _tier = bits.bit()?;
      }
      let mut point = OperatingPoint {
        idc,
        decoder_model: false,
      };
      if decoder_model {
        point.decoder_model = bits.bit()?;
        if point.decoder_model {
          let _decoder = bits.bits(u32::from(encoder_delay_bits))?;
          let _encoder = bits.bits(u32::from(encoder_delay_bits))?;
          let _low = bits.bit()?;
        }
      }
      if display_model && bits.bit()? {
        let _delay = bits.bits(4)?;
      }
      operating_points.push(point);
    }
  }
  let width_bits = bits.bits(4)? as u8 + 1;
  let height_bits = bits.bits(4)? as u8 + 1;
  let max_width = bits.bits(u32::from(width_bits))? + 1;
  let max_height = bits.bits(u32::from(height_bits))? + 1;
  let mut frame_id_present = false;
  let mut delta_frame_id_bits = 0u8;
  let mut frame_id_bits = 0u8;
  if !reduced_still {
    frame_id_present = bits.bit()?;
    if frame_id_present {
      delta_frame_id_bits = bits.bits(4)? as u8 + 2;
      frame_id_bits = bits.bits(3)? as u8 + delta_frame_id_bits + 1;
    }
  }
  let sb128 = bits.bit()?;
  let filter_intra = bits.bit()?;
  let intra_edge_filter = bits.bit()?;
  let inter_intra;
  let masked_compound;
  let warped_motion;
  let dual_filter;
  let order_hint;
  let jnt_comp;
  let ref_frame_mvs;
  let screen_content_tools;
  let force_integer_mv;
  let order_hint_bits;
  if reduced_still {
    inter_intra = false;
    masked_compound = false;
    warped_motion = false;
    dual_filter = false;
    order_hint = false;
    jnt_comp = false;
    ref_frame_mvs = false;
    screen_content_tools = Select::Adaptive;
    force_integer_mv = Select::Adaptive;
    order_hint_bits = 0;
  } else {
    inter_intra = bits.bit()?;
    masked_compound = bits.bit()?;
    warped_motion = bits.bit()?;
    dual_filter = bits.bit()?;
    order_hint = bits.bit()?;
    if order_hint {
      jnt_comp = bits.bit()?;
      ref_frame_mvs = bits.bit()?;
    } else {
      jnt_comp = false;
      ref_frame_mvs = false;
    }
    screen_content_tools = if bits.bit()? {
      Select::Adaptive
    } else if bits.bit()? {
      Select::On
    } else {
      Select::Off
    };
    // `Off` does not consume the following bits. The `||` keeps that order.
    force_integer_mv = if screen_content_tools == Select::Off || bits.bit()? {
      Select::Adaptive
    } else if bits.bit()? {
      Select::On
    } else {
      Select::Off
    };
    order_hint_bits = if order_hint {
      bits.bits(3)? as u8 + 1
    } else {
      0
    };
  }
  let superres = bits.bit()?;
  let cdef = bits.bit()?;
  let restoration = bits.bit()?;
  let mut bit_depth = 8u8;
  if bits.bit()? {
    bit_depth = 10;
    if profile == 2 && bits.bit()? {
      bit_depth = 12;
    }
  }
  let monochrome = if profile == 1 { false } else { bits.bit()? };
  let described = bits.bit()?;
  let (primaries, transfer, matrix) = if described {
    (bits.bits(8)?, bits.bits(8)?, bits.bits(8)? as u8)
  } else {
    (2, 2, 2)
  };
  let full_range;
  let subsampling_x;
  let subsampling_y;
  let mut chroma_sample_position = 0u8;
  if monochrome {
    full_range = bits.bit()?;
    subsampling_x = true;
    subsampling_y = true;
  } else if primaries == 1 && transfer == 13 && matrix == 0 {
    return Err(DecodeError::Rejected);
  } else {
    full_range = bits.bit()?;
    match profile {
      0 => {
        subsampling_x = true;
        subsampling_y = true;
      }
      1 => {
        subsampling_x = false;
        subsampling_y = false;
      }
      _ => {
        if bit_depth == 12 {
          subsampling_x = bits.bit()?;
          subsampling_y = if subsampling_x { bits.bit()? } else { false };
        } else {
          subsampling_x = true;
          subsampling_y = false;
        }
      }
    }
    if subsampling_x && subsampling_y {
      chroma_sample_position = bits.bits(2)? as u8;
    }
  }
  let separate_uv_delta_q = if monochrome { false } else { bits.bit()? };
  let film_grain = bits.bit()?;
  trailing_bits(&mut bits)?;
  if bits.pos != bits.data.len() * 8 {
    return Err(DecodeError::Rejected);
  }
  Ok(Sequence {
    profile,
    still_picture,
    reduced_still,
    max_width,
    max_height,
    bit_depth,
    monochrome,
    full_range,
    subsampling_x,
    subsampling_y,
    chroma_sample_position,
    matrix,
    sb128,
    filter_intra,
    intra_edge_filter,
    inter_intra,
    masked_compound,
    warped_motion,
    dual_filter,
    order_hint,
    jnt_comp,
    ref_frame_mvs,
    screen_content_tools,
    force_integer_mv,
    order_hint_bits,
    superres,
    cdef,
    restoration,
    film_grain,
    separate_uv_delta_q,
    width_bits,
    height_bits,
    frame_id_present,
    frame_id_bits,
    delta_frame_id_bits,
    decoder_model,
    equal_picture_interval,
    buffer_removal_bits,
    presentation_delay_bits,
    operating_points,
  })
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
    let byte = *self.data.get(self.pos / 8).ok_or(DecodeError::Rejected)?;
    let shift = 7 - (self.pos % 8);
    self.pos += 1;
    Ok((byte >> shift) & 1 == 1)
  }

  fn bits(&mut self, n: u32) -> Result<u32, DecodeError> {
    if n > 32 {
      return Err(DecodeError::Rejected);
    }
    let mut value = 0u64;
    for _ in 0..n {
      value = (value << 1) | u64::from(self.bit()?);
    }
    Ok(value as u32)
  }

  fn sbits(&mut self, n: u32) -> Result<i32, DecodeError> {
    if n == 0 || n > 32 {
      return Err(DecodeError::Rejected);
    }
    let value = self.bits(n)?;
    let shift = 32 - n;
    Ok(((value << shift) as i32) >> shift)
  }

  /// Unsigned variable-length code. A run of 32 zeros is rejected.
  fn uvlc(&mut self) -> Result<u32, DecodeError> {
    if self.bit()? {
      return Ok(0);
    }
    let mut zeros = 0u32;
    loop {
      zeros += 1;
      if zeros == 32 {
        return Err(DecodeError::Rejected);
      }
      if self.bit()? {
        break;
      }
    }
    Ok(((1u32 << zeros) - 1).wrapping_add(self.bits(zeros)?))
  }

  fn uniform(&mut self, max: u32) -> Result<u32, DecodeError> {
    if max == 0 {
      return Err(DecodeError::Rejected);
    }
    if max == 1 {
      return Ok(0);
    }
    let length = 32 - max.leading_zeros();
    let split = (1u32 << length) - max;
    let value = self.bits(length - 1)?;
    if value < split {
      Ok(value)
    } else {
      Ok((value << 1) - split + u32::from(self.bit()?))
    }
  }

  fn subexp(&mut self, reference: i32, n: u32) -> Result<i32, DecodeError> {
    if n >= 31 {
      return Err(DecodeError::Rejected);
    }
    let span = 1i32 << n;
    let recentered = reference.wrapping_add(span) as u32;
    let decoded = self.subexp_unsigned(recentered, 2u32 << n)?;
    Ok((decoded as i32).wrapping_sub(span))
  }

  fn subexp_unsigned(
    &mut self,
    reference: u32,
    n: u32,
  ) -> Result<u32, DecodeError> {
    let mut value = 0u32;
    let mut step = 0u32;
    loop {
      let width = if step == 0 { 3 } else { step + 2 };
      let span = 3u32.checked_shl(width).ok_or(DecodeError::Rejected)?;
      if value >= n || n - value < span {
        if value > n {
          return Err(DecodeError::Rejected);
        }
        value += self.uniform(n - value + 1)?;
        break;
      } else if !self.bit()? {
        value += self.bits(width)?;
        break;
      } else {
        value += 1u32 << width;
        step += 1;
        if step > 8 {
          return Err(DecodeError::Rejected);
        }
      }
    }
    if reference <= n / 2 {
      Ok(inv_recenter(reference, value))
    } else {
      Ok(n - inv_recenter(n - reference, value))
    }
  }

  fn rest(&self) -> Result<&'a [u8], DecodeError> {
    if !self.pos.is_multiple_of(8) {
      return Err(DecodeError::Rejected);
    }
    Ok(&self.data[self.pos / 8..])
  }

  fn remaining(&self) -> usize {
    self.data.len().saturating_sub(self.pos / 8)
  }

  fn take(&mut self, len: usize) -> Result<&'a [u8], DecodeError> {
    if !self.pos.is_multiple_of(8) {
      return Err(DecodeError::Rejected);
    }
    let start = self.pos / 8;
    let end = start.checked_add(len).ok_or(DecodeError::Rejected)?;
    if end > self.data.len() {
      return Err(DecodeError::Rejected);
    }
    self.pos = end * 8;
    Ok(&self.data[start..end])
  }

  fn read_le(&mut self, len: usize) -> Result<usize, DecodeError> {
    let bytes = self.take(len)?;
    let mut value = 0usize;
    for (index, byte) in bytes.iter().enumerate() {
      value |= usize::from(*byte) << (index * 8);
    }
    Ok(value)
  }
}

fn inv_recenter(reference: u32, value: u32) -> u32 {
  if u64::from(value) > (u64::from(reference) << 1) {
    value
  } else if value & 1 == 0 {
    (value >> 1).wrapping_add(reference)
  } else {
    reference.wrapping_sub(value.wrapping_add(1) >> 1)
  }
}

fn leb128(bytes: &[u8], offset: usize) -> Result<(usize, usize), DecodeError> {
  let mut value = 0u64;
  let mut shift = 0;
  let mut pos = offset;
  loop {
    if shift > 28 {
      return Err(DecodeError::Rejected);
    }
    let byte = *bytes.get(pos).ok_or(DecodeError::Rejected)?;
    pos += 1;
    value |= u64::from(byte & 0x7f) << shift;
    if byte & 0x80 == 0 {
      let len = usize::try_from(value).map_err(|_| DecodeError::Rejected)?;
      return Ok((len, pos));
    }
    shift += 7;
  }
}

fn trailing_bits(bits: &mut Bits<'_>) -> Result<(), DecodeError> {
  if !bits.bit()? {
    return Err(DecodeError::Rejected);
  }
  byte_align(bits)
}

fn byte_align(bits: &mut Bits<'_>) -> Result<(), DecodeError> {
  while !bits.pos.is_multiple_of(8) {
    if bits.bit()? {
      return Err(DecodeError::Rejected);
    }
  }
  Ok(())
}

fn tile_log2(block: u32, target: u32) -> u32 {
  let mut k = 0u32;
  while (u64::from(block) << k) < u64::from(target) {
    k += 1;
    if k >= 32 {
      break;
    }
  }
  k
}

fn read_tiles(
  frame: &Frame,
  start: usize,
  end: usize,
  bits: &mut Bits<'_>,
) -> Result<Vec<Tile>, DecodeError> {
  let cols = usize::from(frame.tile_cols);
  if cols == 0 || start > end {
    return Err(DecodeError::Rejected);
  }
  let mut tiles = Vec::new();
  for index in start..=end {
    let size = if index == end {
      bits.remaining()
    } else {
      let width = usize::from(frame.tile_size_bytes);
      if width == 0 || width > 4 {
        return Err(DecodeError::Rejected);
      }
      bits
        .read_le(width)?
        .checked_add(1)
        .ok_or(DecodeError::Rejected)?
    };
    if size == 0 {
      return Err(DecodeError::Rejected);
    }
    tiles.push(Tile {
      row: u16::try_from(index / cols).map_err(|_| DecodeError::Rejected)?,
      column: u16::try_from(index % cols).map_err(|_| DecodeError::Rejected)?,
      bytes: bits.take(size)?.to_vec(),
    });
  }
  Ok(tiles)
}

fn poc_diff(bits: u8, left: i32, right: i32) -> i32 {
  if bits == 0 {
    return 0;
  }
  let mask = 1i32 << (bits - 1);
  let diff = left.wrapping_sub(right);
  (diff & (mask - 1)) - (diff & mask)
}

fn slot_at(
  slots: &[Option<Slot>; 8],
  index: usize,
) -> Result<&Slot, DecodeError> {
  slots
    .get(index)
    .and_then(Option::as_ref)
    .ok_or(DecodeError::Rejected)
}

fn identity_matrix() -> [i32; 8] {
  [0, 0, 1 << 16, 0, 0, 1 << 16, 0, 0]
}

fn default_deltas() -> LoopDeltas {
  LoopDeltas {
    ref_delta: [1, 0, 0, 0, -1, 0, -1, -1],
    mode_delta: [0, 0],
  }
}

struct FrameSize {
  upscaled: u32,
  coded: u32,
  height: u32,
  render_width: u32,
  render_height: u32,
  superres: bool,
  denom: u8,
}

fn superres_size(
  sequence: &Sequence,
  upscaled: u32,
  bits: &mut Bits<'_>,
) -> Result<(bool, u8, u32), DecodeError> {
  if sequence.superres && bits.bit()? {
    let denom = 9 + bits.bits(3)? as u8;
    let divisor = u32::from(denom);
    let scaled =
      upscaled.saturating_mul(8).saturating_add(divisor >> 1) / divisor;
    Ok((true, denom, scaled.max(16.min(upscaled))))
  } else {
    Ok((false, 8, upscaled))
  }
}

fn parse_frame_size(
  sequence: &Sequence,
  slots: &[Option<Slot>; 8],
  refs: Option<&[u8; 7]>,
  override_size: bool,
  bits: &mut Bits<'_>,
) -> Result<FrameSize, DecodeError> {
  if let Some(refs) = refs {
    for &index in refs {
      if bits.bit()? {
        let slot = slot_at(slots, usize::from(index))?;
        let (superres, denom, coded) =
          superres_size(sequence, slot.upscaled_width, bits)?;
        return Ok(FrameSize {
          upscaled: slot.upscaled_width,
          coded,
          height: slot.height,
          render_width: slot.render_width,
          render_height: slot.render_height,
          superres,
          denom,
        });
      }
    }
  }
  let upscaled = if override_size {
    bits.bits(u32::from(sequence.width_bits))? + 1
  } else {
    sequence.max_width
  };
  let height = if override_size {
    bits.bits(u32::from(sequence.height_bits))? + 1
  } else {
    sequence.max_height
  };
  if upscaled == 0 || height == 0 {
    return Err(DecodeError::Rejected);
  }
  let (superres, denom, coded) = superres_size(sequence, upscaled, bits)?;
  let (render_width, render_height) = if bits.bit()? {
    (bits.bits(16)? + 1, bits.bits(16)? + 1)
  } else {
    (upscaled, height)
  };
  Ok(FrameSize {
    upscaled,
    coded,
    height,
    render_width,
    render_height,
    superres,
    denom,
  })
}

struct Tiling {
  uniform: bool,
  cols: u8,
  rows: u8,
  size_bytes: u8,
  update: u16,
  tile_bits: u8,
  width_sbs: [u16; 63],
  height_sbs: [u16; 63],
}

fn sbs_minus_1(widths: &[u32]) -> [u16; 63] {
  let mut out = [0u16; 63];
  let count = widths.len().saturating_sub(1).min(63);
  for (index, width) in widths.iter().take(count).enumerate() {
    out[index] = u16::try_from(width.saturating_sub(1)).unwrap_or(u16::MAX);
  }
  out
}

fn parse_tiling(
  sequence: &Sequence,
  size: &FrameSize,
  bits: &mut Bits<'_>,
) -> Result<Tiling, DecodeError> {
  let sb_log = if sequence.sb128 { 7 } else { 6 };
  let sb = 1u32 << sb_log;
  let sbw = size.coded.saturating_add(sb - 1) >> sb_log;
  let sbh = size.height.saturating_add(sb - 1) >> sb_log;
  if sbw == 0 || sbh == 0 {
    return Err(DecodeError::Rejected);
  }
  let max_tile_width = 4096 >> sb_log;
  let max_tile_area = (4096 * 2304) >> (2 * sb_log);
  let min_log2_cols = tile_log2(max_tile_width, sbw);
  let max_log2_cols = tile_log2(1, sbw.min(MAX_TILE_COLS));
  let max_log2_rows = tile_log2(1, sbh.min(MAX_TILE_ROWS));
  let min_log2_tiles =
    tile_log2(max_tile_area, sbw.saturating_mul(sbh)).max(min_log2_cols);
  let uniform = bits.bit()?;
  let (log2_cols, widths, log2_rows, heights) = if uniform {
    let mut log2_cols = min_log2_cols;
    while log2_cols < max_log2_cols && bits.bit()? {
      log2_cols += 1;
    }
    let tile_w = 1 + ((sbw - 1) >> log2_cols);
    let mut widths = Vec::new();
    let mut sbx = 0u32;
    while sbx < sbw {
      let width = tile_w.min(sbw - sbx);
      widths.push(width);
      sbx += width;
    }
    let min_log2_rows = min_log2_tiles.saturating_sub(log2_cols);
    let mut log2_rows = min_log2_rows;
    while log2_rows < max_log2_rows && bits.bit()? {
      log2_rows += 1;
    }
    let tile_h = 1 + ((sbh - 1) >> log2_rows);
    let mut heights = Vec::new();
    let mut sby = 0u32;
    while sby < sbh {
      let height = tile_h.min(sbh - sby);
      heights.push(height);
      sby += height;
    }
    (log2_cols, widths, log2_rows, heights)
  } else {
    let mut widths = Vec::new();
    let mut widest = 0u32;
    let mut sbx = 0u32;
    while sbx < sbw && widths.len() < MAX_TILE_COLS as usize {
      let room = (sbw - sbx).min(max_tile_width);
      let width = if room > 1 { 1 + bits.uniform(room)? } else { 1 };
      widths.push(width);
      widest = widest.max(width);
      sbx += width;
    }
    if sbx != sbw || widths.is_empty() {
      return Err(DecodeError::Rejected);
    }
    let log2_cols = tile_log2(1, widths.len() as u32);
    let mut area = sbw.saturating_mul(sbh);
    if min_log2_tiles != 0 {
      area >>= min_log2_tiles + 1;
    }
    let max_height = (area / widest).max(1);
    let mut heights = Vec::new();
    let mut sby = 0u32;
    while sby < sbh && heights.len() < MAX_TILE_ROWS as usize {
      let room = (sbh - sby).min(max_height);
      let height = if room > 1 { 1 + bits.uniform(room)? } else { 1 };
      heights.push(height);
      sby += height;
    }
    if sby != sbh || heights.is_empty() {
      return Err(DecodeError::Rejected);
    }
    let log2_rows = tile_log2(1, heights.len() as u32);
    (log2_cols, widths, log2_rows, heights)
  };
  if widths.len() > MAX_TILE_COLS as usize
    || heights.len() > MAX_TILE_ROWS as usize
  {
    return Err(DecodeError::Rejected);
  }
  let cols = widths.len() as u8;
  let rows = heights.len() as u8;
  let tile_bits = (log2_cols + log2_rows) as u8;
  let (update, size_bytes) = if tile_bits != 0 {
    let update = bits.bits(u32::from(tile_bits))? as u16;
    if u32::from(update) >= u32::from(cols) * u32::from(rows) {
      return Err(DecodeError::Rejected);
    }
    (update, bits.bits(2)? as u8 + 1)
  } else {
    (0, 0)
  };
  Ok(Tiling {
    uniform,
    cols,
    rows,
    size_bytes,
    update,
    tile_bits,
    width_sbs: sbs_minus_1(&widths),
    height_sbs: sbs_minus_1(&heights),
  })
}

fn read_delta(bits: &mut Bits<'_>) -> Result<i8, DecodeError> {
  if bits.bit()? {
    Ok(bits.sbits(7)? as i8)
  } else {
    Ok(0)
  }
}

struct Quant {
  base: u8,
  y_dc: i8,
  u_dc: i8,
  u_ac: i8,
  v_dc: i8,
  v_ac: i8,
  using: bool,
  qm_y: u8,
  qm_u: u8,
  qm_v: u8,
}

fn parse_quant(
  sequence: &Sequence,
  bits: &mut Bits<'_>,
) -> Result<Quant, DecodeError> {
  let base = bits.bits(8)? as u8;
  let y_dc = read_delta(bits)?;
  let (u_dc, u_ac, v_dc, v_ac) = if sequence.monochrome {
    (0, 0, 0, 0)
  } else {
    let separate = sequence.separate_uv_delta_q && bits.bit()?;
    let u_dc = read_delta(bits)?;
    let u_ac = read_delta(bits)?;
    if separate {
      (u_dc, u_ac, read_delta(bits)?, read_delta(bits)?)
    } else {
      (u_dc, u_ac, u_dc, u_ac)
    }
  };
  let using = bits.bit()?;
  let (qm_y, qm_u, qm_v) = if using {
    let y = bits.bits(4)? as u8;
    let u = bits.bits(4)? as u8;
    let v = if sequence.separate_uv_delta_q {
      bits.bits(4)? as u8
    } else {
      u
    };
    (y, u, v)
  } else {
    (0, 0, 0)
  };
  Ok(Quant {
    base,
    y_dc,
    u_dc,
    u_ac,
    v_dc,
    v_ac,
    using,
    qm_y,
    qm_u,
    qm_v,
  })
}

fn parse_seg_data(
  bits: &mut Bits<'_>,
) -> Result<([[i16; 8]; 8], [u8; 8]), DecodeError> {
  const MAX: [i32; 8] = [255, 63, 63, 63, 63, 7, 0, 0];
  const WIDTH: [u32; 8] = [8, 6, 6, 6, 6, 3, 0, 0];
  const SIGNED: [bool; 8] = [true, true, true, true, true, false, false, false];
  let mut data = [[0i16; 8]; 8];
  let mut mask = [0u8; 8];
  for segment in 0..8 {
    for feature in 0..8 {
      if !bits.bit()? {
        continue;
      }
      mask[segment] |= 1 << feature;
      let width = WIDTH[feature];
      let raw = if SIGNED[feature] {
        bits.sbits(width + 1)?
      } else if width > 0 {
        bits.bits(width)? as i32
      } else {
        0
      };
      let limit = MAX[feature];
      let min = if SIGNED[feature] { -limit } else { 0 };
      data[segment][feature] = raw.clamp(min, limit) as i16;
    }
  }
  Ok((data, mask))
}

fn lossless_flags(
  quant: &Quant,
  enabled: bool,
  data: &[[i16; 8]; 8],
) -> [bool; 8] {
  let flat = quant.y_dc == 0
    && quant.u_dc == 0
    && quant.u_ac == 0
    && quant.v_dc == 0
    && quant.v_ac == 0;
  let mut flags = [false; 8];
  for (index, flag) in flags.iter_mut().enumerate() {
    let q = if enabled {
      (i32::from(quant.base) + i32::from(data[index][0])).clamp(0, 255)
    } else {
      i32::from(quant.base)
    };
    *flag = q == 0 && flat;
  }
  flags
}

fn consider(
  used: &[bool; 8],
  slot: usize,
  value: i32,
  best: &mut i32,
  pick: &mut i8,
  later: bool,
) {
  let better = if later { value >= *best } else { value < *best };
  if !used[slot] && better {
    *pick = slot as i8;
    *best = value;
  }
}

fn short_refs(
  slots: &[Option<Slot>; 8],
  hint_bits: u8,
  order_hint: u8,
  bits: &mut Bits<'_>,
) -> Result<[u8; 7], DecodeError> {
  let mut refs = [-1i8; 7];
  refs[0] = bits.bits(3)? as i8;
  refs[3] = bits.bits(3)? as i8;
  let mut hint = [0i32; 8];
  let current = 1i32 << (hint_bits - 1);
  for (index, hint_at) in hint.iter_mut().enumerate() {
    let slot = slot_at(slots, index)?;
    *hint_at = current
      + poc_diff(hint_bits, i32::from(slot.order_hint), i32::from(order_hint));
  }
  let mut used = [false; 8];
  used[refs[0] as usize] = true;
  used[refs[3] as usize] = true;
  let mut latest = -1i32;
  let mut alt = refs[6];
  for (index, &value) in hint.iter().enumerate() {
    if value >= current {
      consider(&used, index, value, &mut latest, &mut alt, true);
    }
  }
  refs[6] = alt;
  if latest != -1 {
    used[refs[6] as usize] = true;
  }
  for which in [4, 5] {
    let mut earliest = i32::MAX;
    let mut pick = refs[which];
    for (index, &value) in hint.iter().enumerate() {
      if value >= current {
        consider(&used, index, value, &mut earliest, &mut pick, false);
      }
    }
    refs[which] = pick;
    if earliest != i32::MAX {
      used[refs[which] as usize] = true;
    }
  }
  for slot_ref in refs.iter_mut().skip(1) {
    if *slot_ref >= 0 {
      continue;
    }
    let mut latest = -1i32;
    let mut pick = -1i8;
    for (slot, &value) in hint.iter().enumerate() {
      if value < current {
        consider(&used, slot, value, &mut latest, &mut pick, true);
      }
    }
    if latest != -1 {
      *slot_ref = pick;
      used[pick as usize] = true;
    }
  }
  let mut earliest = i32::MAX;
  let mut fallback = 0i8;
  for (index, &value) in hint.iter().enumerate() {
    if value < earliest {
      fallback = index as i8;
      earliest = value;
    }
  }
  for item in &mut refs {
    if *item < 0 {
      *item = fallback;
    }
  }
  Ok(refs.map(|item| item as u8))
}

fn read_refs(
  sequence: &Sequence,
  slots: &[Option<Slot>; 8],
  short: bool,
  order_hint: u8,
  frame_id: u32,
  bits: &mut Bits<'_>,
) -> Result<[u8; 7], DecodeError> {
  let mut refs = if short {
    short_refs(slots, sequence.order_hint_bits, order_hint, bits)?
  } else {
    [0; 7]
  };
  for slot_ref in &mut refs {
    if !short {
      *slot_ref = bits.bits(3)? as u8;
    }
    if sequence.frame_id_present {
      let delta = bits.bits(u32::from(sequence.delta_frame_id_bits))? + 1;
      let span = 1u32 << sequence.frame_id_bits;
      let expected =
        frame_id.wrapping_add(span).wrapping_sub(delta) & (span - 1);
      if slot_at(slots, usize::from(*slot_ref))?.frame_id != expected {
        return Err(DecodeError::Rejected);
      }
    }
  }
  Ok(refs)
}

fn skip_allowed(
  slots: &[Option<Slot>; 8],
  hint_bits: u8,
  order_hint: u8,
  refs: &[u8; 7],
) -> Result<bool, DecodeError> {
  let poc = i32::from(order_hint);
  let mut before: Option<i32> = None;
  let mut after: Option<i32> = None;
  for &index in refs {
    let refpoc = i32::from(slot_at(slots, usize::from(index))?.order_hint);
    let diff = poc_diff(hint_bits, refpoc, poc);
    if diff > 0 {
      let replace =
        after.is_none_or(|prev| poc_diff(hint_bits, prev, refpoc) > 0);
      if replace {
        after = Some(refpoc);
      }
    } else if diff < 0 {
      let replace =
        before.is_none_or(|prev| poc_diff(hint_bits, refpoc, prev) > 0);
      if replace {
        before = Some(refpoc);
      }
    }
  }
  if before.is_some() && after.is_some() {
    return Ok(true);
  }
  let Some(before) = before else {
    return Ok(false);
  };
  for &index in refs {
    let refpoc = i32::from(slot_at(slots, usize::from(index))?.order_hint);
    if poc_diff(hint_bits, refpoc, before) < 0 {
      return Ok(true);
    }
  }
  Ok(false)
}

fn parse_motion(
  slots: &[Option<Slot>; 8],
  primary: Option<&Slot>,
  hp: bool,
  bits: &mut Bits<'_>,
) -> Result<([u32; 7], [[i32; 8]; 7]), DecodeError> {
  let _ = slots;
  let mut kinds = [0u32; 7];
  let mut matrices = [identity_matrix(); 7];
  for index in 0..7 {
    let kind = if !bits.bit()? {
      0
    } else if bits.bit()? {
      2
    } else if bits.bit()? {
      1
    } else {
      3
    };
    kinds[index] = kind;
    if kind == 0 {
      continue;
    }
    let reference = primary
      .map(|slot| slot.gmv[index].matrix)
      .unwrap_or_else(identity_matrix);
    let mut matrix = identity_matrix();
    let (precision, shift) = if kind >= 2 {
      matrix[2] =
        (1 << 16) + 2 * bits.subexp((reference[2] - (1 << 16)) >> 1, 12)?;
      matrix[3] = 2 * bits.subexp(reference[3] >> 1, 12)?;
      (12u32, 10u32)
    } else {
      let off = u32::from(!hp);
      (9 - off, 13 + off)
    };
    if kind == 3 {
      matrix[4] = 2 * bits.subexp(reference[4] >> 1, 12)?;
      matrix[5] =
        (1 << 16) + 2 * bits.subexp((reference[5] - (1 << 16)) >> 1, 12)?;
    } else {
      matrix[4] = matrix[3].wrapping_neg();
      matrix[5] = matrix[2];
    }
    let scale = 1i32 << shift;
    matrix[0] = bits
      .subexp(reference[0] >> shift, precision)?
      .checked_mul(scale)
      .ok_or(DecodeError::Rejected)?;
    matrix[1] = bits
      .subexp(reference[1] >> shift, precision)?
      .checked_mul(scale)
      .ok_or(DecodeError::Rejected)?;
    matrices[index] = matrix;
  }
  Ok((kinds, matrices))
}

fn empty_grain(seed: u16) -> Grain {
  Grain {
    seed,
    chroma_from_luma: false,
    scaling_shift: 0,
    ar_lag: 0,
    ar_shift: 0,
    grain_scale_shift: 0,
    overlap: false,
    clip: false,
    num_y: 0,
    y_value: [0; 14],
    y_scaling: [0; 14],
    num_cb: 0,
    cb_value: [0; 10],
    cb_scaling: [0; 10],
    num_cr: 0,
    cr_value: [0; 10],
    cr_scaling: [0; 10],
    ar_y: [0; 24],
    ar_cb: [0; 25],
    ar_cr: [0; 25],
    cb_mult: 0,
    cb_luma_mult: 0,
    cb_offset: 0,
    cr_mult: 0,
    cr_luma_mult: 0,
    cr_offset: 0,
  }
}

fn read_points(
  bits: &mut Bits<'_>,
  limit: usize,
  values: &mut [u8],
  scaling: &mut [u8],
) -> Result<u8, DecodeError> {
  let count = bits.bits(4)? as usize;
  if count > limit {
    return Err(DecodeError::Rejected);
  }
  let mut previous = None;
  for index in 0..count {
    let value = bits.bits(8)? as u8;
    if previous.is_some_and(|previous| previous >= value) {
      return Err(DecodeError::Rejected);
    }
    previous = Some(value);
    values[index] = value;
    scaling[index] = bits.bits(8)? as u8;
  }
  Ok(count as u8)
}

fn parse_grain_data(
  sequence: &Sequence,
  seed: u16,
  bits: &mut Bits<'_>,
) -> Result<Grain, DecodeError> {
  let mut grain = empty_grain(seed);
  grain.num_y =
    read_points(bits, 14, &mut grain.y_value, &mut grain.y_scaling)?;
  grain.chroma_from_luma = !sequence.monochrome && bits.bit()?;
  let skip_uv = sequence.monochrome
    || grain.chroma_from_luma
    || (sequence.subsampling_x && sequence.subsampling_y && grain.num_y == 0);
  if !skip_uv {
    grain.num_cb =
      read_points(bits, 10, &mut grain.cb_value, &mut grain.cb_scaling)?;
    grain.num_cr =
      read_points(bits, 10, &mut grain.cr_value, &mut grain.cr_scaling)?;
  }
  if sequence.subsampling_x
    && sequence.subsampling_y
    && (grain.num_cb != 0) != (grain.num_cr != 0)
  {
    return Err(DecodeError::Rejected);
  }
  grain.scaling_shift = bits.bits(2)? as u8 + 8;
  grain.ar_lag = bits.bits(2)? as u8;
  let y_pos = 2 * usize::from(grain.ar_lag) * (usize::from(grain.ar_lag) + 1);
  if grain.num_y != 0 {
    for coeff in grain.ar_y.iter_mut().take(y_pos) {
      *coeff = (bits.bits(8)? as u8).wrapping_sub(128) as i8;
    }
  }
  let extra = usize::from(grain.num_y != 0);
  for (count, coeffs) in [
    (grain.num_cb, &mut grain.ar_cb),
    (grain.num_cr, &mut grain.ar_cr),
  ] {
    if count != 0 || grain.chroma_from_luma {
      for coeff in coeffs.iter_mut().take(y_pos + extra) {
        *coeff = (bits.bits(8)? as u8).wrapping_sub(128) as i8;
      }
    }
  }
  grain.ar_shift = bits.bits(2)? as u8 + 6;
  grain.grain_scale_shift = bits.bits(2)? as u8;
  if grain.num_cb != 0 {
    grain.cb_mult = bits.bits(8)? as u8;
    grain.cb_luma_mult = bits.bits(8)? as u8;
    grain.cb_offset = bits.bits(9)? as u16;
  }
  if grain.num_cr != 0 {
    grain.cr_mult = bits.bits(8)? as u8;
    grain.cr_luma_mult = bits.bits(8)? as u8;
    grain.cr_offset = bits.bits(9)? as u16;
  }
  grain.overlap = bits.bit()?;
  grain.clip = bits.bit()?;
  Ok(grain)
}

fn parse_frame(
  sequence: &Sequence,
  slots: &[Option<Slot>; 8],
  bits: &mut Bits<'_>,
) -> Result<Header, DecodeError> {
  if !sequence.reduced_still && bits.bit()? {
    let index = bits.bits(3)? as usize;
    if sequence.decoder_model && !sequence.equal_picture_interval {
      let _ = bits.bits(u32::from(sequence.presentation_delay_bits))?;
    }
    if sequence.frame_id_present {
      let id = bits.bits(u32::from(sequence.frame_id_bits))?;
      if slot_at(slots, index)?.frame_id != id {
        return Err(DecodeError::Rejected);
      }
    } else {
      slot_at(slots, index)?;
    }
    return Ok(Header::Show(index));
  }
  let frame_type = if sequence.reduced_still {
    0
  } else {
    bits.bits(2)? as u8
  };
  let show = sequence.reduced_still || bits.bit()?;
  let showable = if show {
    if sequence.decoder_model && !sequence.equal_picture_interval {
      let _ = bits.bits(u32::from(sequence.presentation_delay_bits))?;
    }
    frame_type != 0
  } else {
    bits.bit()?
  };
  let error_resilient = (frame_type == 0 && show)
    || frame_type == 3
    || sequence.reduced_still
    || bits.bit()?;
  let disable_cdf = bits.bit()?;
  let screen = match sequence.screen_content_tools {
    Select::Adaptive => bits.bit()?,
    Select::On => true,
    Select::Off => false,
  };
  let mut integer_mv = if screen {
    match sequence.force_integer_mv {
      Select::Adaptive => bits.bit()?,
      Select::On => true,
      Select::Off => false,
    }
  } else {
    false
  };
  let intra = frame_type == 0 || frame_type == 2;
  if intra {
    integer_mv = true;
  }
  let frame_id = if sequence.frame_id_present {
    bits.bits(u32::from(sequence.frame_id_bits))?
  } else {
    0
  };
  let override_size = if sequence.reduced_still {
    false
  } else if frame_type == 3 {
    true
  } else {
    bits.bit()?
  };
  let order_hint = if sequence.order_hint {
    bits.bits(u32::from(sequence.order_hint_bits))? as u8
  } else {
    0
  };
  let primary = if !error_resilient && !intra {
    bits.bits(3)? as u8
  } else {
    PRIMARY_NONE
  };
  if sequence.decoder_model && bits.bit()? {
    for point in &sequence.operating_points {
      if !point.decoder_model {
        continue;
      }
      let temporal = point.idc & 1;
      let spatial = (point.idc >> 8) & 1;
      if point.idc == 0 || (temporal != 0 && spatial != 0) {
        let _ = bits.bits(u32::from(sequence.buffer_removal_bits))?;
      }
    }
  }
  let refresh = if intra {
    if frame_type == 0 && show {
      0xff
    } else {
      bits.bits(8)? as u8
    }
  } else if frame_type == 3 {
    0xff
  } else {
    bits.bits(8)? as u8
  };
  if intra && frame_type != 0 && refresh == 0xff {
    return Err(DecodeError::Rejected);
  }
  if sequence.order_hint
    && error_resilient
    && (intra && refresh != 0xff || !intra)
  {
    for _ in 0..8 {
      let _ = bits.bits(u32::from(sequence.order_hint_bits))?;
    }
  }
  let short = !intra && sequence.order_hint && bits.bit()?;
  let refs = if intra {
    [0u8; 7]
  } else {
    read_refs(sequence, slots, short, order_hint, frame_id, bits)?
  };
  let use_ref_size = !intra && !error_resilient && override_size;
  let size = parse_frame_size(
    sequence,
    slots,
    use_ref_size.then_some(&refs),
    override_size,
    bits,
  )?;
  let allow_intrabc = intra && screen && !size.superres && bits.bit()?;
  let hp = !intra && !integer_mv && bits.bit()?;
  let interp = if intra {
    0
  } else if bits.bit()? {
    4
  } else {
    bits.bits(2)? as u8
  };
  let motion_switch = !intra && bits.bit()?;
  let use_ref_mvs = !intra
    && !error_resilient
    && sequence.ref_frame_mvs
    && sequence.order_hint
    && bits.bit()?;
  let disable_end = if !sequence.reduced_still && !disable_cdf {
    bits.bit()?
  } else {
    true
  };
  let tiling = parse_tiling(sequence, &size, bits)?;
  let quant = parse_quant(sequence, bits)?;
  let primary_slot = if primary == PRIMARY_NONE {
    None
  } else {
    Some(slot_at(slots, usize::from(refs[usize::from(primary)]))?)
  };
  let seg_enabled = bits.bit()?;
  let (seg_update_map, seg_temporal, seg_update_data, seg_data, seg_mask) =
    if seg_enabled {
      let (update_map, temporal, update_data) = if primary == PRIMARY_NONE {
        (true, false, true)
      } else {
        let update_map = bits.bit()?;
        let temporal = update_map && bits.bit()?;
        (update_map, temporal, bits.bit()?)
      };
      let (data, mask) = if update_data {
        parse_seg_data(bits)?
      } else {
        let slot = primary_slot.ok_or(DecodeError::Rejected)?;
        (slot.segments.data, slot.segments.mask)
      };
      (update_map, temporal, update_data, data, mask)
    } else {
      (false, false, false, [[0i16; 8]; 8], [0u8; 8])
    };
  let lossless = lossless_flags(&quant, seg_enabled, &seg_data);
  let all_lossless = lossless.iter().all(|flag| *flag);
  let delta_q_present = quant.base != 0 && bits.bit()?;
  let log2_delta_q = if delta_q_present {
    bits.bits(2)? as u8
  } else {
    0
  };
  let delta_lf_present = delta_q_present && !allow_intrabc && bits.bit()?;
  let log2_delta_lf = if delta_lf_present {
    bits.bits(2)? as u8
  } else {
    0
  };
  let delta_lf_multi = delta_lf_present && bits.bit()?;
  let (level, level_u, level_v, sharpness, delta_enabled, delta_update, deltas) =
    if all_lossless || allow_intrabc {
      ([0u8, 0], 0, 0, 0, true, true, default_deltas())
    } else {
      let level = [bits.bits(6)? as u8, bits.bits(6)? as u8];
      let (level_u, level_v) =
        if !sequence.monochrome && (level[0] != 0 || level[1] != 0) {
          (bits.bits(6)? as u8, bits.bits(6)? as u8)
        } else {
          (0, 0)
        };
      let sharpness = bits.bits(3)? as u8;
      let mut deltas = primary_slot
        .map(|slot| slot.loop_filter.deltas)
        .unwrap_or_else(default_deltas);
      let delta_enabled = bits.bit()?;
      let delta_update = if delta_enabled {
        let update = bits.bit()?;
        if update {
          for index in 0..8 {
            if bits.bit()? {
              deltas.ref_delta[index] = bits.sbits(7)? as i8;
            }
          }
          for index in 0..2 {
            if bits.bit()? {
              deltas.mode_delta[index] = bits.sbits(7)? as i8;
            }
          }
        }
        update
      } else {
        false
      };
      (
        level,
        level_u,
        level_v,
        sharpness,
        delta_enabled,
        delta_update,
        deltas,
      )
    };
  let (cdef_damping, cdef_bits, cdef_y, cdef_uv) =
    if !all_lossless && sequence.cdef && !allow_intrabc {
      let damping = bits.bits(2)? as u8;
      let n_bits = bits.bits(2)? as u8;
      let mut y = [0u8; 8];
      let mut uv = [0u8; 8];
      for index in 0..1usize << n_bits {
        y[index] = bits.bits(6)? as u8;
        if !sequence.monochrome {
          uv[index] = bits.bits(6)? as u8;
        }
      }
      (damping, n_bits, y, uv)
    } else {
      (0, 0, [0; 8], [0; 8])
    };
  let (restoration, lr_unit, lr_uv) = if (!all_lossless || size.superres)
    && sequence.restoration
    && !allow_intrabc
  {
    let mut kind = [0u8; 3];
    kind[0] = bits.bits(2)? as u8;
    if !sequence.monochrome {
      kind[1] = bits.bits(2)? as u8;
      kind[2] = bits.bits(2)? as u8;
    }
    let (unit, uv) = if kind == [0, 0, 0] {
      (0, 0)
    } else {
      let mut unit = 0u8;
      if bits.bit()? {
        unit = 1;
        if !sequence.sb128 && bits.bit()? {
          unit = 2;
        }
      }
      let uv = if (kind[1] != 0 || kind[2] != 0)
        && sequence.subsampling_x
        && sequence.subsampling_y
      {
        u8::from(bits.bit()?)
      } else {
        0
      };
      (unit, uv)
    };
    (kind, unit, uv)
  } else {
    ([0u8; 3], 0, 0)
  };
  let tx_mode = if all_lossless {
    0
  } else if bits.bit()? {
    2
  } else {
    1
  };
  let reference_select = !intra && bits.bit()?;
  let skip_present = reference_select
    && sequence.order_hint
    && skip_allowed(slots, sequence.order_hint_bits, order_hint, &refs)?
    && bits.bit()?;
  let warped =
    !error_resilient && !intra && sequence.warped_motion && bits.bit()?;
  let reduced_tx = bits.bit()?;
  let (wm_type, wm_mat) = if intra {
    ([0u32; 7], [identity_matrix(); 7])
  } else {
    parse_motion(slots, primary_slot, hp, bits)?
  };
  let apply_grain = sequence.film_grain && (show || showable) && bits.bit()?;
  let grain = if apply_grain {
    let seed = bits.bits(16)? as u16;
    let update = intra || bits.bit()?;
    if update {
      parse_grain_data(sequence, seed, bits)?
    } else {
      let which = bits.bits(3)? as usize;
      if !refs.iter().any(|item| usize::from(*item) == which) {
        return Err(DecodeError::Rejected);
      }
      let mut copied = slot_at(slots, which)?
        .grain
        .clone()
        .ok_or(DecodeError::Rejected)?;
      copied.seed = seed;
      copied
    }
  } else {
    empty_grain(0)
  };
  let gmv = std::array::from_fn(|index| GlobalMotion {
    kind: wm_type[index],
    matrix: wm_mat[index],
  });
  let refresh_slot = Slot {
    order_hint,
    frame_id,
    upscaled_width: size.upscaled,
    height: size.height,
    render_width: size.render_width,
    render_height: size.render_height,
    segments: Segments {
      enabled: seg_enabled,
      update_map: seg_update_map,
      temporal: seg_temporal,
      update_data: seg_update_data,
      data: seg_data,
      mask: seg_mask,
      lossless,
    },
    loop_filter: LoopFilter {
      level_y: level,
      level_u,
      level_v,
      sharpness,
      delta_enabled,
      delta_update,
      deltas,
    },
    gmv,
    grain: apply_grain.then(|| grain.clone()),
  };
  Ok(Header::Frame(Frame {
    show,
    frame_type,
    showable,
    error_resilient,
    disable_cdf_update: disable_cdf,
    allow_screen_content_tools: screen,
    force_integer_mv: integer_mv,
    upscaled_width: size.upscaled,
    height: size.height,
    coded_width: size.coded,
    superres: size.superres,
    superres_denom: size.denom,
    refresh_flags: refresh,
    ref_idx: refs,
    order_hint,
    primary_ref: primary,
    allow_intrabc,
    allow_high_precision_mv: hp,
    interp_filter: interp,
    motion_mode_switchable: motion_switch,
    use_ref_frame_mvs: use_ref_mvs,
    disable_frame_end_update_cdf: disable_end,
    uniform_tiles: tiling.uniform,
    allow_warped_motion: warped,
    tile_cols: tiling.cols,
    tile_rows: tiling.rows,
    tile_size_bytes: tiling.size_bytes,
    tile_bits: tiling.tile_bits,
    width_in_sbs_minus_1: tiling.width_sbs,
    height_in_sbs_minus_1: tiling.height_sbs,
    context_update_tile_id: tiling.update,
    filter_level: level,
    filter_level_u: level_u,
    filter_level_v: level_v,
    sharpness,
    loop_delta_enabled: delta_enabled,
    loop_delta_update: delta_update,
    ref_deltas: deltas.ref_delta,
    mode_deltas: deltas.mode_delta,
    base_qindex: quant.base,
    y_dc_delta_q: quant.y_dc,
    u_dc_delta_q: quant.u_dc,
    u_ac_delta_q: quant.u_ac,
    v_dc_delta_q: quant.v_dc,
    v_ac_delta_q: quant.v_ac,
    using_qmatrix: quant.using,
    qm_y: quant.qm_y,
    qm_u: quant.qm_u,
    qm_v: quant.qm_v,
    seg_enabled,
    seg_update_map,
    seg_temporal,
    seg_update_data,
    seg_data,
    seg_mask,
    delta_q_present,
    log2_delta_q_res: log2_delta_q,
    delta_lf_present,
    log2_delta_lf_res: log2_delta_lf,
    delta_lf_multi,
    tx_mode,
    reference_select,
    skip_mode_present: skip_present,
    reduced_tx_set: reduced_tx,
    cdef_damping_minus_3: cdef_damping,
    cdef_bits,
    cdef_y,
    cdef_uv,
    restoration,
    lr_unit_shift: lr_unit,
    lr_uv_shift: lr_uv,
    wm_type,
    wm_mat,
    apply_grain,
    grain,
    tiles: Vec::new(),
    refresh_slot,
  }))
}

#[cfg(test)]
mod tests {
  use super::{Picture, Stream};
  use crate::av1::config_obus;
  use crate::mp4::parse;

  fn hex(text: &str) -> Vec<u8> {
    (0..text.len())
      .step_by(2)
      .map(|index| u8::from_str_radix(&text[index..index + 2], 16).unwrap())
      .collect()
  }

  #[test]
  fn obu_temporal_units_are_shown_keyframes() {
    let bytes = hex(
      "12000a0b020000049dffffc0af9004321c10008f0012488202084400ad3b67001eb5db79529bb8b9afe3b87a5012000a0b020000049dffffc0af9004321c10048f0012488202084400ad3b67001eb5db79529bb8b9afe3b87a50",
    );
    let mut stream = Stream::new();
    let pictures = stream.push(&bytes).expect("obu");
    assert_eq!(pictures.len(), 2);
    for picture in pictures {
      let Picture::Frame(frame) = picture else {
        panic!("show existing");
      };
      assert!(frame.show);
      assert_eq!(frame.frame_type, 0);
      assert!(!frame.superres);
      assert_eq!((frame.upscaled_width, frame.height), (256, 256));
      assert_eq!(frame.tiles.len(), 1);
      assert!(!frame.tiles[0].bytes.is_empty());
    }
    let sequence = stream.sequence().expect("sequence");
    assert!(sequence.hardware_profile());
    assert!(!sequence.full_range);
  }

  #[test]
  fn mp4_samples_match_the_coded_size() {
    for (name, width, height) in
      [("red-av1-256.mp4", 256, 256), ("red-av1.mp4", 16, 16)]
    {
      let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/media")
        .join(name);
      let movie = parse(&std::fs::read(&path).unwrap()).unwrap();
      let track = movie.video.into_iter().next().unwrap();
      assert_eq!((track.coded_width, track.coded_height), (width, height));
      let mut stream = Stream::new();
      let config = config_obus(&track.description).unwrap();
      assert!(stream.push(config).unwrap().is_empty());
      let sequence = stream.sequence().unwrap();
      assert!(sequence.hardware_profile());
      assert_eq!((sequence.max_width, sequence.max_height), (width, height));
      assert_eq!(track.samples.len(), 2);
      for sample in &track.samples {
        let pictures = stream.push(&sample.data).unwrap();
        assert_eq!(pictures.len(), 1);
        let Picture::Frame(frame) = &pictures[0] else {
          panic!("show existing");
        };
        assert!(frame.show);
        assert_eq!(frame.frame_type, 0);
        assert!(!frame.superres);
        assert_eq!((frame.upscaled_width, frame.height), (width, height));
        assert_eq!(frame.tiles.len(), 1);
        assert!(!frame.tiles[0].bytes.is_empty());
      }
    }
  }
}
