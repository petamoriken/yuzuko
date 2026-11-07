//! Pack an AV1 picture into libva 2.20 parameter buffers.
//!
//! The tile payload is submitted separately. This module only fills the
//! picture and per-tile parameter structs. Superres and a frame whose size
//! is not the configured coded size are rejected.

use std::ptr::null_mut;

use super::va_types::{
  VaDecPictureParameterBufferAV1, VaFilmGrainStructAV1,
  VaSegmentationStructAV1, VaSliceParameterBufferAV1, VaWarpedMotionParamsAV1,
};
use crate::av1_obu::{Frame, Grain, Sequence};
use crate::error::DecodeError;

const INVALID_SURFACE: u32 = u32::MAX;

pub(crate) struct Av1Picture {
  pub(crate) picture: VaDecPictureParameterBufferAV1,
  pub(crate) slices: Vec<VaSliceParameterBufferAV1>,
}

pub(crate) fn prepare(
  sequence: &Sequence,
  frame: &Frame,
  coded_width: u32,
  coded_height: u32,
  recon: u32,
  display: u32,
  refs: &[Option<u32>; 8],
) -> Result<Av1Picture, DecodeError> {
  if frame.superres || frame.tiles.is_empty() {
    return Err(DecodeError::Rejected);
  }
  if frame.upscaled_width == 0
    || frame.height == 0
    || frame.upscaled_width != coded_width
    || frame.height != coded_height
  {
    return Err(DecodeError::Rejected);
  }
  if frame.frame_type == 1 || frame.frame_type == 3 {
    for index in frame.ref_idx {
      let slot = usize::from(index);
      if refs.get(slot).copied().flatten().is_none() {
        return Err(DecodeError::Rejected);
      }
    }
  }
  if frame.apply_grain && display == recon {
    return Err(DecodeError::Rejected);
  }
  let mut picture: VaDecPictureParameterBufferAV1 =
    unsafe { std::mem::zeroed() };
  picture.profile = sequence.profile;
  picture.order_hint_bits_minus_1 = if sequence.order_hint {
    sequence
      .order_hint_bits
      .checked_sub(1)
      .ok_or(DecodeError::Rejected)?
  } else {
    0
  };
  picture.bit_depth_idx = match sequence.bit_depth {
    8 => 0,
    10 => 1,
    12 => 2,
    _ => return Err(DecodeError::Rejected),
  };
  picture.matrix_coefficients = sequence.matrix;
  picture.seq_info_fields = seq_fields(sequence);
  picture.current_frame = recon;
  picture.current_display_picture =
    if frame.apply_grain { display } else { recon };
  picture.anchor_frames_num = 0;
  picture.anchor_frames_list = null_mut();
  picture.frame_width_minus1 = u16::try_from(frame.upscaled_width - 1)
    .map_err(|_| DecodeError::Rejected)?;
  picture.frame_height_minus1 =
    u16::try_from(frame.height - 1).map_err(|_| DecodeError::Rejected)?;
  picture.output_frame_width_in_tiles_minus_1 = 0;
  picture.output_frame_height_in_tiles_minus_1 = 0;
  picture.ref_frame_map = std::array::from_fn(|index| {
    refs
      .get(index)
      .copied()
      .flatten()
      .unwrap_or(INVALID_SURFACE)
  });
  picture.ref_frame_idx = frame.ref_idx;
  picture.primary_ref_frame = frame.primary_ref;
  picture.order_hint = frame.order_hint;
  picture.seg_info = segmentation(frame);
  picture.film_grain_info = if frame.apply_grain {
    film_grain(&frame.grain)
  } else {
    unsafe { std::mem::zeroed() }
  };
  picture.tile_cols = frame.tile_cols;
  picture.tile_rows = frame.tile_rows;
  picture.width_in_sbs_minus_1 = frame.width_in_sbs_minus_1;
  picture.height_in_sbs_minus_1 = frame.height_in_sbs_minus_1;
  picture.tile_count_minus_1 = 0;
  picture.context_update_tile_id = frame.context_update_tile_id;
  picture.pic_info_fields = pic_fields(frame);
  picture.superres_scale_denominator = 8;
  picture.interp_filter = frame.interp_filter;
  picture.filter_level = frame.filter_level;
  picture.filter_level_u = frame.filter_level_u;
  picture.filter_level_v = frame.filter_level_v;
  picture.loop_filter_info_fields = loop_filter_fields(frame);
  picture.ref_deltas = frame.ref_deltas;
  picture.mode_deltas = frame.mode_deltas;
  picture.base_qindex = frame.base_qindex;
  picture.y_dc_delta_q = frame.y_dc_delta_q;
  picture.u_dc_delta_q = frame.u_dc_delta_q;
  picture.u_ac_delta_q = frame.u_ac_delta_q;
  picture.v_dc_delta_q = frame.v_dc_delta_q;
  picture.v_ac_delta_q = frame.v_ac_delta_q;
  picture.qmatrix_fields = qmatrix_fields(frame);
  picture.mode_control_fields = mode_control_fields(frame);
  picture.cdef_damping_minus_3 = frame.cdef_damping_minus_3;
  picture.cdef_bits = frame.cdef_bits;
  picture.cdef_y_strengths = frame.cdef_y;
  picture.cdef_uv_strengths = frame.cdef_uv;
  picture.loop_restoration_fields = restoration_fields(frame);
  for index in 0..7 {
    picture.wm[index] = VaWarpedMotionParamsAV1 {
      wmtype: frame.wm_type[index],
      wmmat: frame.wm_mat[index],
      invalid: 0,
      va_reserved: [0; 4],
    };
  }
  picture.va_reserved = [0; 8];
  Ok(Av1Picture {
    picture,
    slices: slice_params(frame)?,
  })
}

fn seq_fields(sequence: &Sequence) -> u32 {
  let mut value = 0u32;
  let flags = [
    sequence.still_picture,
    sequence.sb128,
    sequence.filter_intra,
    sequence.intra_edge_filter,
    sequence.inter_intra,
    sequence.masked_compound,
    sequence.dual_filter,
    sequence.order_hint,
    sequence.jnt_comp,
    sequence.cdef,
    sequence.monochrome,
    sequence.full_range,
    sequence.subsampling_x,
    sequence.subsampling_y,
  ];
  for (bit, flag) in flags.into_iter().enumerate() {
    if flag {
      value |= 1 << bit;
    }
  }
  value |= u32::from(sequence.chroma_sample_position & 1) << 14;
  if sequence.film_grain {
    value |= 1 << 15;
  }
  value
}

fn pic_fields(frame: &Frame) -> u32 {
  let mut value = u32::from(frame.frame_type & 3);
  let flags = [
    frame.show,
    frame.showable,
    frame.error_resilient,
    frame.disable_cdf_update,
    frame.allow_screen_content_tools,
    frame.force_integer_mv,
    frame.allow_intrabc,
    false,
    frame.allow_high_precision_mv,
    frame.motion_mode_switchable,
    frame.use_ref_frame_mvs,
    frame.disable_frame_end_update_cdf,
    frame.uniform_tiles,
    frame.allow_warped_motion,
    false,
  ];
  for (bit, flag) in flags.into_iter().enumerate() {
    if flag {
      value |= 1 << (bit + 2);
    }
  }
  value
}

fn loop_filter_fields(frame: &Frame) -> u8 {
  let mut value = frame.sharpness & 7;
  if frame.loop_delta_enabled {
    value |= 1 << 3;
  }
  if frame.loop_delta_update {
    value |= 1 << 4;
  }
  value
}

fn qmatrix_fields(frame: &Frame) -> u16 {
  if !frame.using_qmatrix {
    return 0;
  }
  1 | (u16::from(frame.qm_y & 15) << 1)
    | (u16::from(frame.qm_u & 15) << 5)
    | (u16::from(frame.qm_v & 15) << 9)
}

fn mode_control_fields(frame: &Frame) -> u32 {
  let mut value = 0u32;
  if frame.delta_q_present {
    value |= 1;
  }
  value |= u32::from(frame.log2_delta_q_res & 3) << 1;
  if frame.delta_lf_present {
    value |= 1 << 3;
  }
  value |= u32::from(frame.log2_delta_lf_res & 3) << 4;
  if frame.delta_lf_multi {
    value |= 1 << 6;
  }
  value |= u32::from(frame.tx_mode & 3) << 7;
  if frame.reference_select {
    value |= 1 << 9;
  }
  if frame.reduced_tx_set {
    value |= 1 << 10;
  }
  if frame.skip_mode_present {
    value |= 1 << 11;
  }
  value
}

fn restoration_fields(frame: &Frame) -> u16 {
  u16::from(frame.restoration[0] & 3)
    | (u16::from(frame.restoration[1] & 3) << 2)
    | (u16::from(frame.restoration[2] & 3) << 4)
    | (u16::from(frame.lr_unit_shift & 3) << 6)
    | (u16::from(frame.lr_uv_shift & 1) << 8)
}

fn segmentation(frame: &Frame) -> VaSegmentationStructAV1 {
  let mut fields = 0u32;
  if frame.seg_enabled {
    fields |= 1;
  }
  if frame.seg_update_map {
    fields |= 1 << 1;
  }
  if frame.seg_temporal {
    fields |= 1 << 2;
  }
  if frame.seg_update_data {
    fields |= 1 << 3;
  }
  VaSegmentationStructAV1 {
    segment_info_fields: fields,
    feature_data: frame.seg_data,
    feature_mask: frame.seg_mask,
    va_reserved: [0; 4],
  }
}

fn film_grain(grain: &Grain) -> VaFilmGrainStructAV1 {
  let mut fields = 1u32;
  if grain.chroma_from_luma {
    fields |= 1 << 1;
  }
  fields |= u32::from(grain.scaling_shift.saturating_sub(8) & 3) << 2;
  fields |= u32::from(grain.ar_lag & 3) << 4;
  fields |= u32::from(grain.ar_shift.saturating_sub(6) & 3) << 6;
  fields |= u32::from(grain.grain_scale_shift & 3) << 8;
  if grain.overlap {
    fields |= 1 << 10;
  }
  if grain.clip {
    fields |= 1 << 11;
  }
  VaFilmGrainStructAV1 {
    film_grain_info_fields: fields,
    grain_seed: grain.seed,
    num_y_points: grain.num_y,
    point_y_value: grain.y_value,
    point_y_scaling: grain.y_scaling,
    num_cb_points: grain.num_cb,
    point_cb_value: grain.cb_value,
    point_cb_scaling: grain.cb_scaling,
    num_cr_points: grain.num_cr,
    point_cr_value: grain.cr_value,
    point_cr_scaling: grain.cr_scaling,
    ar_coeffs_y: grain.ar_y,
    ar_coeffs_cb: grain.ar_cb,
    ar_coeffs_cr: grain.ar_cr,
    cb_mult: grain.cb_mult,
    cb_luma_mult: grain.cb_luma_mult,
    cb_offset: grain.cb_offset,
    cr_mult: grain.cr_mult,
    cr_luma_mult: grain.cr_luma_mult,
    cr_offset: grain.cr_offset,
    va_reserved: [0; 4],
  }
}

fn slice_params(
  frame: &Frame,
) -> Result<Vec<VaSliceParameterBufferAV1>, DecodeError> {
  let last =
    u16::try_from(frame.tiles.len() - 1).map_err(|_| DecodeError::Rejected)?;
  let mut slices = Vec::with_capacity(frame.tiles.len());
  for tile in &frame.tiles {
    slices.push(VaSliceParameterBufferAV1 {
      slice_data_size: u32::try_from(tile.bytes.len())
        .map_err(|_| DecodeError::Rejected)?,
      slice_data_offset: 0,
      slice_data_flag: 0,
      tile_row: tile.row,
      tile_column: tile.column,
      tg_start: 0,
      tg_end: last,
      anchor_frame_idx: 0,
      tile_idx_in_tile_list: 0,
      va_reserved: [0; 4],
    });
  }
  Ok(slices)
}

#[cfg(test)]
mod tests {
  use super::prepare;
  use crate::av1_obu::{Picture, Stream};
  use crate::mp4::parse;

  fn shown_keyframe(
    name: &str,
  ) -> (crate::av1_obu::Sequence, crate::av1_obu::Frame) {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
      .join("../../tests/fixtures/media")
      .join(name);
    let bytes = std::fs::read(&path).unwrap_or_else(|error| {
      panic!("{}: {error}", path.display());
    });
    let movie =
      parse(&bytes).unwrap_or_else(|error| panic!("{name}: {error:?}"));
    let track = movie.video.into_iter().next().expect("video");
    let mut stream = Stream::new();
    let obus = crate::av1::config_obus(&track.description).expect("av1C");
    assert!(stream.push(obus).expect("config").is_empty());
    let mut shown = None;
    for sample in &track.samples {
      for picture in stream.push(&sample.data).expect("sample") {
        if let Picture::Frame(frame) = picture
          && frame.show
        {
          shown = Some(frame);
          break;
        }
      }
      if shown.is_some() {
        break;
      }
    }
    let frame = shown.expect("frame");
    let sequence = stream.sequence().expect("sequence").clone();
    (sequence, frame)
  }

  #[test]
  fn keyframe_picture_matches_the_sequence() {
    for name in ["red-av1-256.mp4", "red-av1.mp4"] {
      let (sequence, frame) = shown_keyframe(name);
      assert!(sequence.hardware_profile());
      let packed = prepare(
        &sequence,
        &frame,
        sequence.max_width,
        sequence.max_height,
        7,
        7,
        &[None; 8],
      )
      .unwrap_or_else(|error| panic!("{name}: {error:?}"));
      let picture = packed.picture;
      assert_eq!(picture.profile, 0);
      assert_eq!(picture.bit_depth_idx, 0);
      assert_eq!(picture.matrix_coefficients, sequence.matrix);
      assert_eq!(
        picture.frame_width_minus1,
        u16::try_from(sequence.max_width - 1).unwrap()
      );
      assert_eq!(
        picture.frame_height_minus1,
        u16::try_from(sequence.max_height - 1).unwrap()
      );
      assert_eq!(picture.current_frame, 7);
      assert_eq!(picture.current_display_picture, 7);
      assert_eq!(picture.superres_scale_denominator, 8);
      assert_eq!(picture.tile_cols, 1);
      assert_eq!(picture.tile_rows, 1);
      assert_eq!(picture.primary_ref_frame, 7);
      assert_eq!(picture.pic_info_fields & 3, 0);
      assert_eq!((picture.pic_info_fields >> 2) & 1, 1);
      assert_eq!((picture.seq_info_fields >> 11) & 1, 0);
      assert_eq!((picture.seq_info_fields >> 12) & 1, 1);
      assert_eq!((picture.seq_info_fields >> 13) & 1, 1);
      assert!(picture.ref_frame_map.iter().all(|id| *id == u32::MAX));
      assert_eq!(picture.film_grain_info.film_grain_info_fields & 1, 0);
      assert_eq!(packed.slices.len(), 1);
      assert_eq!(
        packed.slices[0].slice_data_size as usize,
        frame.tiles[0].bytes.len()
      );
      assert_ne!(packed.slices[0].slice_data_size, 0);
      assert_eq!(packed.slices[0].slice_data_offset, 0);
      assert_eq!(packed.slices[0].tile_row, 0);
      assert_eq!(packed.slices[0].tile_column, 0);
      assert_eq!(packed.slices[0].tg_start, 0);
      assert_eq!(packed.slices[0].tg_end, 0);

      let mut inter = frame.clone();
      inter.frame_type = 1;
      assert!(
        prepare(
          &sequence,
          &inter,
          sequence.max_width,
          sequence.max_height,
          1,
          1,
          &[None; 8],
        )
        .is_err()
      );
      let refs = [Some(4); 8];
      let referenced = prepare(
        &sequence,
        &inter,
        sequence.max_width,
        sequence.max_height,
        1,
        2,
        &refs,
      )
      .expect("refs");
      assert_eq!(referenced.picture.ref_frame_map, [4; 8]);
      assert_eq!(referenced.picture.current_display_picture, 1);

      inter.apply_grain = true;
      let grained = prepare(
        &sequence,
        &inter,
        sequence.max_width,
        sequence.max_height,
        1,
        2,
        &refs,
      )
      .expect("grain");
      assert_eq!(grained.picture.current_frame, 1);
      assert_eq!(grained.picture.current_display_picture, 2);
      assert_eq!(
        grained.picture.film_grain_info.film_grain_info_fields & 1,
        1
      );
      assert!(
        prepare(
          &sequence,
          &inter,
          sequence.max_width,
          sequence.max_height,
          1,
          1,
          &refs,
        )
        .is_err()
      );
      inter.superres = true;
      assert!(
        prepare(
          &sequence,
          &inter,
          sequence.max_width,
          sequence.max_height,
          1,
          2,
          &refs,
        )
        .is_err()
      );
    }
  }
}
