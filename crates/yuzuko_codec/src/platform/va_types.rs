//! `libva` 2.20 H.264 decode structs. Layout is checked against clang.

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

#[cfg(test)]
mod tests {
  use std::mem::{offset_of, size_of};

  use super::{
    VaImage, VaImageFormat, VaIqMatrixBufferH264, VaPictureH264,
    VaPictureParameterBufferH264, VaSliceParameterBufferH264,
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
  }
}
