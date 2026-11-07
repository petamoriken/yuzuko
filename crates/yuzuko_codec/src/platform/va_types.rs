//! `libva` 2.20 H.264 and AV1 decode structs. Layout is checked against clang.

#[repr(C)]
#[derive(Clone, Copy)]
pub struct VaPictureH264 {
  pub picture_id: u32,
  pub frame_idx: u32,
  pub flags: u32,
  pub top_field_order_cnt: i32,
  pub bottom_field_order_cnt: i32,
  pub reserved: [u32; 4],
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct VaPictureParameterBufferH264 {
  pub curr_pic: VaPictureH264,
  pub reference_frames: [VaPictureH264; 16],
  pub picture_width_in_mbs_minus1: u16,
  pub picture_height_in_mbs_minus1: u16,
  pub bit_depth_luma_minus8: u8,
  pub bit_depth_chroma_minus8: u8,
  pub num_ref_frames: u8,
  pub seq_fields: u32,
  pub num_slice_groups_minus1: u8,
  pub slice_group_map_type: u8,
  pub slice_group_change_rate_minus1: u16,
  pub pic_init_qp_minus26: i8,
  pub pic_init_qs_minus26: i8,
  pub chroma_qp_index_offset: i8,
  pub second_chroma_qp_index_offset: i8,
  pub pic_fields: u32,
  pub frame_num: u16,
  pub reserved: [u32; 8],
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct VaIqMatrixBufferH264 {
  pub scaling_list_4x4: [[u8; 16]; 6],
  pub scaling_list_8x8: [[u8; 64]; 2],
  pub reserved: [u32; 4],
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct VaSliceParameterBufferH264 {
  pub slice_data_size: u32,
  pub slice_data_offset: u32,
  pub slice_data_flag: u32,
  pub slice_data_bit_offset: u16,
  pub first_mb_in_slice: u16,
  pub slice_type: u8,
  pub direct_spatial_mv_pred_flag: u8,
  pub num_ref_idx_l0_active_minus1: u8,
  pub num_ref_idx_l1_active_minus1: u8,
  pub cabac_init_idc: u8,
  pub slice_qp_delta: i8,
  pub disable_deblocking_filter_idc: u8,
  pub slice_alpha_c0_offset_div2: i8,
  pub slice_beta_offset_div2: i8,
  pub ref_pic_list0: [VaPictureH264; 32],
  pub ref_pic_list1: [VaPictureH264; 32],
  pub luma_log2_weight_denom: u8,
  pub chroma_log2_weight_denom: u8,
  pub luma_weight_l0_flag: u8,
  pub luma_weight_l0: [i16; 32],
  pub luma_offset_l0: [i16; 32],
  pub chroma_weight_l0_flag: u8,
  pub chroma_weight_l0: [[i16; 2]; 32],
  pub chroma_offset_l0: [[i16; 2]; 32],
  pub luma_weight_l1_flag: u8,
  pub luma_weight_l1: [i16; 32],
  pub luma_offset_l1: [i16; 32],
  pub chroma_weight_l1_flag: u8,
  pub chroma_weight_l1: [[i16; 2]; 32],
  pub chroma_offset_l1: [[i16; 2]; 32],
  pub reserved: [u32; 4],
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct VaImageFormat {
  pub fourcc: u32,
  pub byte_order: u32,
  pub bits_per_pixel: u32,
  pub depth: u32,
  pub red_mask: u32,
  pub green_mask: u32,
  pub blue_mask: u32,
  pub alpha_mask: u32,
  pub reserved: [u32; 4],
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct VaImage {
  pub image_id: u32,
  pub format: VaImageFormat,
  pub buf: u32,
  pub width: u16,
  pub height: u16,
  pub data_size: u32,
  pub num_planes: u32,
  pub pitches: [u32; 3],
  pub offsets: [u32; 3],
  pub num_palette_entries: i32,
  pub entry_bytes: i32,
  pub component_order: [i8; 4],
  pub reserved: [u32; 4],
}

/// `VASegmentationStructAV1` from libva 2.20. `feature_data` is `[segment][feature]`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct VaSegmentationStructAV1 {
  pub segment_info_fields: u32,
  pub feature_data: [[i16; 8]; 8],
  pub feature_mask: [u8; 8],
  pub va_reserved: [u32; 4],
}

/// `VAFilmGrainStructAV1`. Multipliers and offsets are the raw syntax values.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct VaFilmGrainStructAV1 {
  pub film_grain_info_fields: u32,
  pub grain_seed: u16,
  pub num_y_points: u8,
  pub point_y_value: [u8; 14],
  pub point_y_scaling: [u8; 14],
  pub num_cb_points: u8,
  pub point_cb_value: [u8; 10],
  pub point_cb_scaling: [u8; 10],
  pub num_cr_points: u8,
  pub point_cr_value: [u8; 10],
  pub point_cr_scaling: [u8; 10],
  pub ar_coeffs_y: [i8; 24],
  pub ar_coeffs_cb: [i8; 25],
  pub ar_coeffs_cr: [i8; 25],
  pub cb_mult: u8,
  pub cb_luma_mult: u8,
  pub cb_offset: u16,
  pub cr_mult: u8,
  pub cr_luma_mult: u8,
  pub cr_offset: u16,
  pub va_reserved: [u32; 4],
}

/// `VAWarpedMotionParamsAV1`. `wmtype` is `VAAV1TransformationType` stored as `u32`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct VaWarpedMotionParamsAV1 {
  pub wmtype: u32,
  pub wmmat: [i32; 8],
  pub invalid: u8,
  pub va_reserved: [u32; 4],
}

/// `VADecPictureParameterBufferAV1`. The pointer makes the struct 8-byte aligned.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct VaDecPictureParameterBufferAV1 {
  pub profile: u8,
  pub order_hint_bits_minus_1: u8,
  pub bit_depth_idx: u8,
  pub matrix_coefficients: u8,
  pub seq_info_fields: u32,
  pub current_frame: u32,
  pub current_display_picture: u32,
  pub anchor_frames_num: u8,
  pub anchor_frames_list: *mut u32,
  pub frame_width_minus1: u16,
  pub frame_height_minus1: u16,
  pub output_frame_width_in_tiles_minus_1: u16,
  pub output_frame_height_in_tiles_minus_1: u16,
  pub ref_frame_map: [u32; 8],
  pub ref_frame_idx: [u8; 7],
  pub primary_ref_frame: u8,
  pub order_hint: u8,
  pub seg_info: VaSegmentationStructAV1,
  pub film_grain_info: VaFilmGrainStructAV1,
  pub tile_cols: u8,
  pub tile_rows: u8,
  pub width_in_sbs_minus_1: [u16; 63],
  pub height_in_sbs_minus_1: [u16; 63],
  pub tile_count_minus_1: u16,
  pub context_update_tile_id: u16,
  pub pic_info_fields: u32,
  pub superres_scale_denominator: u8,
  pub interp_filter: u8,
  pub filter_level: [u8; 2],
  pub filter_level_u: u8,
  pub filter_level_v: u8,
  pub loop_filter_info_fields: u8,
  pub ref_deltas: [i8; 8],
  pub mode_deltas: [i8; 2],
  pub base_qindex: u8,
  pub y_dc_delta_q: i8,
  pub u_dc_delta_q: i8,
  pub u_ac_delta_q: i8,
  pub v_dc_delta_q: i8,
  pub v_ac_delta_q: i8,
  pub qmatrix_fields: u16,
  pub mode_control_fields: u32,
  pub cdef_damping_minus_3: u8,
  pub cdef_bits: u8,
  pub cdef_y_strengths: [u8; 8],
  pub cdef_uv_strengths: [u8; 8],
  pub loop_restoration_fields: u16,
  pub wm: [VaWarpedMotionParamsAV1; 7],
  pub va_reserved: [u32; 8],
}

/// `VASliceParameterBufferAV1`. One buffer per tile. The payload has no size prefix.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct VaSliceParameterBufferAV1 {
  pub slice_data_size: u32,
  pub slice_data_offset: u32,
  pub slice_data_flag: u32,
  pub tile_row: u16,
  pub tile_column: u16,
  pub tg_start: u16,
  pub tg_end: u16,
  pub anchor_frame_idx: u8,
  pub tile_idx_in_tile_list: u16,
  pub va_reserved: [u32; 4],
}

#[cfg(test)]
mod tests {
  use std::mem::{offset_of, size_of};

  use super::{
    VaDecPictureParameterBufferAV1, VaFilmGrainStructAV1, VaImage,
    VaImageFormat, VaIqMatrixBufferH264, VaPictureH264,
    VaPictureParameterBufferH264, VaSegmentationStructAV1,
    VaSliceParameterBufferAV1, VaSliceParameterBufferH264,
    VaWarpedMotionParamsAV1,
  };

  #[test]
  fn layouts_match_libva_2_20() {
    assert_eq!(size_of::<VaPictureH264>(), 36);
    assert_eq!(size_of::<VaPictureParameterBufferH264>(), 672);
    assert_eq!(offset_of!(VaPictureParameterBufferH264, seq_fields), 620);
    assert_eq!(offset_of!(VaPictureParameterBufferH264, pic_fields), 632);
    assert_eq!(offset_of!(VaPictureParameterBufferH264, frame_num), 636);
    assert_eq!(offset_of!(VaPictureParameterBufferH264, reserved), 640);
    assert_eq!(size_of::<VaIqMatrixBufferH264>(), 240);
    assert_eq!(size_of::<VaSliceParameterBufferH264>(), 3128);
    assert_eq!(offset_of!(VaSliceParameterBufferH264, ref_pic_list0), 28);
    assert_eq!(
      offset_of!(VaSliceParameterBufferH264, luma_log2_weight_denom),
      2332
    );
    assert_eq!(offset_of!(VaSliceParameterBufferH264, luma_weight_l0), 2336);
    assert_eq!(
      offset_of!(VaSliceParameterBufferH264, chroma_weight_l0_flag),
      2464
    );
    assert_eq!(
      offset_of!(VaSliceParameterBufferH264, chroma_weight_l0),
      2466
    );
    assert_eq!(offset_of!(VaSliceParameterBufferH264, reserved), 3112);
    assert_eq!(size_of::<VaImageFormat>(), 48);
    assert_eq!(size_of::<VaImage>(), 120);

    assert_eq!(size_of::<VaSegmentationStructAV1>(), 156);
    assert_eq!(offset_of!(VaSegmentationStructAV1, feature_data), 4);
    assert_eq!(offset_of!(VaSegmentationStructAV1, feature_mask), 132);
    assert_eq!(offset_of!(VaSegmentationStructAV1, va_reserved), 140);
    assert_eq!(size_of::<VaFilmGrainStructAV1>(), 176);
    assert_eq!(offset_of!(VaFilmGrainStructAV1, cb_offset), 154);
    assert_eq!(offset_of!(VaFilmGrainStructAV1, va_reserved), 160);
    assert_eq!(size_of::<VaWarpedMotionParamsAV1>(), 56);
    assert_eq!(offset_of!(VaWarpedMotionParamsAV1, invalid), 36);
    assert_eq!(offset_of!(VaWarpedMotionParamsAV1, va_reserved), 40);
    assert_eq!(size_of::<VaDecPictureParameterBufferAV1>(), 1160);
    assert_eq!(
      offset_of!(VaDecPictureParameterBufferAV1, anchor_frames_list),
      24
    );
    assert_eq!(
      offset_of!(VaDecPictureParameterBufferAV1, frame_width_minus1),
      32
    );
    assert_eq!(
      offset_of!(VaDecPictureParameterBufferAV1, ref_frame_map),
      40
    );
    assert_eq!(offset_of!(VaDecPictureParameterBufferAV1, seg_info), 84);
    assert_eq!(
      offset_of!(VaDecPictureParameterBufferAV1, film_grain_info),
      240
    );
    assert_eq!(offset_of!(VaDecPictureParameterBufferAV1, tile_cols), 416);
    assert_eq!(
      offset_of!(VaDecPictureParameterBufferAV1, width_in_sbs_minus_1),
      418
    );
    assert_eq!(
      offset_of!(VaDecPictureParameterBufferAV1, pic_info_fields),
      676
    );
    assert_eq!(
      offset_of!(VaDecPictureParameterBufferAV1, qmatrix_fields),
      704
    );
    assert_eq!(
      offset_of!(VaDecPictureParameterBufferAV1, mode_control_fields),
      708
    );
    assert_eq!(offset_of!(VaDecPictureParameterBufferAV1, wm), 732);
    assert_eq!(
      offset_of!(VaDecPictureParameterBufferAV1, va_reserved),
      1124
    );
    assert_eq!(size_of::<VaSliceParameterBufferAV1>(), 40);
    assert_eq!(
      offset_of!(VaSliceParameterBufferAV1, tile_idx_in_tile_list),
      22
    );
  }
}
