//! Linux VA-API H.264 decode.
//!
//! The entrypoints are loaded from libva at runtime. A missing library, a
//! missing H.264 profile, or a rejected picture fails the decode. There is
//! no software H.264 path.

use std::ffi::c_void;
use std::fs::OpenOptions;
use std::mem::size_of;
use std::os::fd::IntoRawFd;
use std::ptr::null_mut;
use std::sync::OnceLock;

use libloading::Library;

use yuzuko_image::{ColorSpace, Raster};

use super::va_types::{
  VaImage, VaImageFormat, VaIqMatrixBufferH264, VaPictureH264,
  VaPictureParameterBufferH264, VaSliceParameterBufferH264,
};
use crate::color::{self, I420};
use crate::error::DecodeError;
use crate::h264::{self, Picture, Pps, Slice, Sps};

const H264_PROFILES: &[i32] = &[5, 6, 7, 13];
const ENTRY_VLD: i32 = 1;
const YUV420: u32 = 1;
const INVALID_ID: u32 = u32::MAX;
const SHORT_TERM: u32 = 0x08;
const LONG_TERM: u32 = 0x10;
const PICTURE_INVALID: u32 = 0x01;
const PICTURE_PARAM: i32 = 0;
const IQ_MATRIX: i32 = 1;
const SLICE_PARAM: i32 = 4;
const SLICE_DATA: i32 = 5;
const NV12: u32 = 0x3231_564E;
const NV21: u32 = 0x3132_564E;
const I420: u32 = 0x3032_3449;
const YV12: u32 = 0x3231_5659;

struct Api {
  _lib: Library,
  _drm: Library,
  get_display: unsafe extern "C" fn(i32) -> *mut c_void,
  initialize: unsafe extern "C" fn(*mut c_void, *mut i32, *mut i32) -> i32,
  terminate: unsafe extern "C" fn(*mut c_void) -> i32,
  max_profiles: unsafe extern "C" fn(*mut c_void) -> i32,
  query_profiles: unsafe extern "C" fn(*mut c_void, *mut i32, *mut i32) -> i32,
  create_config: unsafe extern "C" fn(
    *mut c_void,
    i32,
    i32,
    *const c_void,
    i32,
    *mut u32,
  ) -> i32,
  destroy_config: unsafe extern "C" fn(*mut c_void, u32) -> i32,
  create_context: unsafe extern "C" fn(
    *mut c_void,
    u32,
    i32,
    i32,
    i32,
    *mut u32,
    i32,
    *mut u32,
  ) -> i32,
  destroy_context: unsafe extern "C" fn(*mut c_void, u32) -> i32,
  create_surfaces: unsafe extern "C" fn(
    *mut c_void,
    u32,
    u32,
    u32,
    *mut u32,
    u32,
    *mut c_void,
    u32,
  ) -> i32,
  destroy_surfaces: unsafe extern "C" fn(*mut c_void, *mut u32, i32) -> i32,
  create_buffer: unsafe extern "C" fn(
    *mut c_void,
    u32,
    i32,
    u32,
    u32,
    *mut c_void,
    *mut u32,
  ) -> i32,
  destroy_buffer: unsafe extern "C" fn(*mut c_void, u32) -> i32,
  begin_picture: unsafe extern "C" fn(*mut c_void, u32, u32) -> i32,
  render_picture: unsafe extern "C" fn(*mut c_void, u32, *mut u32, i32) -> i32,
  end_picture: unsafe extern "C" fn(*mut c_void, u32) -> i32,
  sync_surface: unsafe extern "C" fn(*mut c_void, u32) -> i32,
  derive_image: unsafe extern "C" fn(*mut c_void, u32, *mut VaImage) -> i32,
  create_image: unsafe extern "C" fn(
    *mut c_void,
    *mut VaImageFormat,
    i32,
    i32,
    *mut VaImage,
  ) -> i32,
  get_image:
    unsafe extern "C" fn(*mut c_void, u32, i32, i32, u32, u32, u32) -> i32,
  destroy_image: unsafe extern "C" fn(*mut c_void, u32) -> i32,
  map_buffer: unsafe extern "C" fn(*mut c_void, u32, *mut *mut c_void) -> i32,
  unmap_buffer: unsafe extern "C" fn(*mut c_void, u32) -> i32,
}

unsafe impl Send for Api {}
unsafe impl Sync for Api {}

#[derive(Clone)]
struct Reference {
  surface: u32,
  frame_num: u32,
  poc: i32,
  long_term: bool,
}

pub struct Session {
  display: *mut c_void,
  config: u32,
  context: u32,
  surfaces: Vec<u32>,
  fd: i32,
  sets: h264::Sets,
  coded_width: u32,
  coded_height: u32,
  origin_x: u32,
  origin_y: u32,
  visible_width: u32,
  visible_height: u32,
  refs: Vec<Reference>,
  prev_poc_msb: i32,
  prev_poc_lsb: u32,
  prev_poc_valid: bool,
}

unsafe impl Send for Session {}

pub fn supports_avc() -> bool {
  supports(H264_PROFILES)
}

pub fn open_avc(
  description: &[u8],
  width: u32,
  height: u32,
) -> Result<Session, DecodeError> {
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
  let pool = (sps.num_ref_frames.max(1) + 1).min(17);
  let profile = sps.profile_idc;
  open_session(Opened {
    profile,
    coded_width,
    coded_height,
    origin_x,
    origin_y,
    visible_width,
    visible_height,
    pool,
    sets,
  })
}

struct Opened {
  profile: u8,
  coded_width: u32,
  coded_height: u32,
  origin_x: u32,
  origin_y: u32,
  visible_width: u32,
  visible_height: u32,
  pool: u32,
  sets: h264::Sets,
}

impl Session {
  pub fn decode(&mut self, bytes: &[u8]) -> Result<Vec<Raster>, DecodeError> {
    let Some(api) = api() else {
      return Err(DecodeError::Unsupported);
    };
    let picture = h264::parse_access_unit(bytes, &mut self.sets)?;
    let leading = &picture.slices[0];
    if picture
      .slices
      .iter()
      .any(|slice| slice.field_pic || slice.frame_num != leading.frame_num)
    {
      return Err(DecodeError::Rejected);
    }
    let pps = self
      .sets
      .pps(leading.pps_id)
      .cloned()
      .ok_or(DecodeError::Rejected)?;
    let sps = self
      .sets
      .sps(pps.sps_id)
      .cloned()
      .ok_or(DecodeError::Rejected)?;
    if sps.coded_size() != (self.coded_width, self.coded_height) {
      return Err(DecodeError::Rejected);
    }
    let surface = self.free_surface()?;
    let counts = self.order_counts(leading, &sps);
    self.submit(api, surface, &sps, &pps, &picture, counts)?;
    let raster = self.read_surface(api, surface)?;
    self.note_reference(leading, &sps, surface, counts);
    Ok(vec![raster])
  }

  pub fn flush(&mut self) -> Result<Vec<Raster>, DecodeError> {
    Ok(Vec::new())
  }

  fn free_surface(&self) -> Result<u32, DecodeError> {
    self
      .surfaces
      .iter()
      .copied()
      .find(|surface| {
        self
          .refs
          .iter()
          .all(|reference| reference.surface != *surface)
      })
      .ok_or(DecodeError::Rejected)
  }

  fn order_counts(&self, slice: &Slice, sps: &Sps) -> (i32, i32, i32) {
    if sps.poc_type != 0 {
      let top = if slice.idr {
        0
      } else {
        i32::try_from(slice.frame_num)
          .unwrap_or(0)
          .saturating_mul(2)
      };
      return (top, top, 0);
    }
    let max_lsb = 1i32 << sps.log2_max_poc_lsb.min(30);
    let lsb = i32::try_from(slice.poc_lsb).unwrap_or(0);
    let msb = if slice.idr || !self.prev_poc_valid {
      0
    } else {
      let previous = i32::try_from(self.prev_poc_lsb).unwrap_or(0);
      if lsb < previous && previous - lsb >= max_lsb / 2 {
        self.prev_poc_msb.saturating_add(max_lsb)
      } else if lsb > previous && lsb - previous > max_lsb / 2 {
        self.prev_poc_msb.saturating_sub(max_lsb)
      } else {
        self.prev_poc_msb
      }
    };
    let top = msb.saturating_add(lsb);
    let bottom = top.saturating_add(slice.delta_bottom);
    (top, bottom, msb)
  }

  fn note_reference(
    &mut self,
    slice: &Slice,
    sps: &Sps,
    surface: u32,
    counts: (i32, i32, i32),
  ) {
    if slice.idr {
      self.refs.clear();
    }
    if slice.nal_ref_idc == 0 {
      return;
    }
    if sps.poc_type == 0 {
      self.prev_poc_msb = counts.2;
      self.prev_poc_lsb = slice.poc_lsb;
      self.prev_poc_valid = true;
    }
    if sps.num_ref_frames == 0 {
      return;
    }
    self.refs.push(Reference {
      surface,
      frame_num: slice.frame_num,
      poc: counts.0,
      long_term: slice.long_term,
    });
    while self.refs.len() > sps.num_ref_frames as usize {
      let index = self
        .refs
        .iter()
        .position(|reference| !reference.long_term)
        .unwrap_or(0);
      self.refs.remove(index);
    }
  }

  fn submit(
    &self,
    api: &Api,
    surface: u32,
    sps: &Sps,
    pps: &Pps,
    picture: &Picture,
    counts: (i32, i32, i32),
  ) -> Result<(), DecodeError> {
    let leading = &picture.slices[0];
    let mut param =
      picture_param(sps, pps, leading, surface, &self.refs, counts)?;
    let mut iq = iq_matrix(pps);
    let mut owned = BufferList::new(api, self.display);
    owned.push(create_buffer(
      api,
      self.display,
      self.context,
      PICTURE_PARAM,
      (&raw mut param).cast::<c_void>(),
      size_of::<VaPictureParameterBufferH264>(),
    )?);
    owned.push(create_buffer(
      api,
      self.display,
      self.context,
      IQ_MATRIX,
      (&raw mut iq).cast::<c_void>(),
      size_of::<VaIqMatrixBufferH264>(),
    )?);
    let mut params = Vec::with_capacity(picture.slices.len());
    for slice in &picture.slices {
      params.push(slice_param(slice, &self.refs, counts.0)?);
    }
    for (index, slice) in picture.slices.iter().enumerate() {
      owned.push(create_buffer(
        api,
        self.display,
        self.context,
        SLICE_PARAM,
        (&raw mut params[index]).cast::<c_void>(),
        size_of::<VaSliceParameterBufferH264>(),
      )?);
      owned.push(create_buffer(
        api,
        self.display,
        self.context,
        SLICE_DATA,
        slice.bytes.as_ptr().cast_mut().cast::<c_void>(),
        slice.bytes.len(),
      )?);
    }
    let status =
      unsafe { (api.begin_picture)(self.display, self.context, surface) };
    if status != 0 {
      return Err(DecodeError::Rejected);
    }
    let rendered = unsafe {
      (api.render_picture)(
        self.display,
        self.context,
        owned.ids.as_mut_ptr(),
        i32::try_from(owned.ids.len()).unwrap_or(i32::MAX),
      )
    };
    let ended = unsafe { (api.end_picture)(self.display, self.context) };
    if rendered != 0 || ended != 0 {
      Err(DecodeError::Rejected)
    } else {
      Ok(())
    }
  }

  fn read_surface(
    &self,
    api: &Api,
    surface: u32,
  ) -> Result<Raster, DecodeError> {
    let status = unsafe { (api.sync_surface)(self.display, surface) };
    if status != 0 {
      return Err(DecodeError::Rejected);
    }
    let mut image = zero_image();
    let derived =
      unsafe { (api.derive_image)(self.display, surface, &mut image) };
    if derived == 0 {
      let raster = map_image(api, self.display, &image, self.visible_rect());
      unsafe { (api.destroy_image)(self.display, image.image_id) };
      return raster;
    }
    let mut format = VaImageFormat {
      fourcc: NV12,
      byte_order: 1,
      bits_per_pixel: 12,
      depth: 0,
      red_mask: 0,
      green_mask: 0,
      blue_mask: 0,
      alpha_mask: 0,
      reserved: [0; 4],
    };
    let status = unsafe {
      (api.create_image)(
        self.display,
        &mut format,
        i32::try_from(self.coded_width).unwrap_or(0),
        i32::try_from(self.coded_height).unwrap_or(0),
        &mut image,
      )
    };
    if status != 0 {
      return Err(DecodeError::Rejected);
    }
    let copied = unsafe {
      (api.get_image)(
        self.display,
        surface,
        0,
        0,
        self.coded_width,
        self.coded_height,
        image.image_id,
      )
    };
    let raster = if copied == 0 {
      map_image(api, self.display, &image, self.visible_rect())
    } else {
      Err(DecodeError::Rejected)
    };
    unsafe { (api.destroy_image)(self.display, image.image_id) };
    raster
  }

  fn visible_rect(&self) -> (u32, u32, u32, u32) {
    (
      self.origin_x,
      self.origin_y,
      self.visible_width,
      self.visible_height,
    )
  }
}

impl Drop for Session {
  fn drop(&mut self) {
    let Some(api) = api() else {
      return;
    };
    release_display(
      api,
      self.display,
      self.fd,
      self.config,
      &mut self.surfaces,
      self.context,
    );
    self.display = null_mut();
    self.fd = -1;
  }
}

struct BufferList<'a> {
  api: &'a Api,
  display: *mut c_void,
  ids: Vec<u32>,
}

impl<'a> BufferList<'a> {
  fn new(api: &'a Api, display: *mut c_void) -> Self {
    Self {
      api,
      display,
      ids: Vec::new(),
    }
  }

  fn push(&mut self, id: u32) {
    self.ids.push(id);
  }
}

impl Drop for BufferList<'_> {
  fn drop(&mut self) {
    for id in self.ids.drain(..) {
      unsafe { (self.api.destroy_buffer)(self.display, id) };
    }
  }
}

fn open_session(opened: Opened) -> Result<Session, DecodeError> {
  let Opened {
    profile: profile_idc,
    coded_width,
    coded_height,
    origin_x,
    origin_y,
    visible_width,
    visible_height,
    pool,
    sets,
  } = opened;
  let Some(api) = api() else {
    return Err(DecodeError::Unsupported);
  };
  let Some((fd, display)) = open_display(api) else {
    return Err(DecodeError::Unsupported);
  };
  let mut setup = Setup {
    api,
    display,
    fd,
    config: INVALID_ID,
    surfaces: Vec::new(),
    context: INVALID_ID,
    release: true,
  };
  let Some(profile) = select_profile(api, display, profile_order(profile_idc))
  else {
    return Err(DecodeError::Unsupported);
  };
  let mut config = INVALID_ID;
  let status = unsafe {
    (api.create_config)(
      display,
      profile,
      ENTRY_VLD,
      std::ptr::null(),
      0,
      &mut config,
    )
  };
  if status != 0 {
    return Err(DecodeError::Rejected);
  }
  setup.config = config;
  let mut surfaces = vec![0u32; pool as usize];
  let status = unsafe {
    (api.create_surfaces)(
      display,
      YUV420,
      coded_width,
      coded_height,
      surfaces.as_mut_ptr(),
      pool,
      null_mut(),
      0,
    )
  };
  if status != 0 {
    return Err(DecodeError::Rejected);
  }
  setup.surfaces = surfaces;
  let mut context = INVALID_ID;
  let status = unsafe {
    (api.create_context)(
      display,
      config,
      i32::try_from(coded_width).unwrap_or(0),
      i32::try_from(coded_height).unwrap_or(0),
      0,
      setup.surfaces.as_mut_ptr(),
      i32::try_from(pool).unwrap_or(0),
      &mut context,
    )
  };
  if status != 0 {
    return Err(DecodeError::Rejected);
  }
  setup.context = context;
  setup.release = false;
  Ok(Session {
    display,
    config,
    context,
    surfaces: std::mem::take(&mut setup.surfaces),
    fd,
    sets,
    coded_width,
    coded_height,
    origin_x,
    origin_y,
    visible_width,
    visible_height,
    refs: Vec::new(),
    prev_poc_msb: 0,
    prev_poc_lsb: 0,
    prev_poc_valid: false,
  })
}

struct Setup<'a> {
  api: &'a Api,
  display: *mut c_void,
  fd: i32,
  config: u32,
  surfaces: Vec<u32>,
  context: u32,
  release: bool,
}

impl Drop for Setup<'_> {
  fn drop(&mut self) {
    if !self.release {
      return;
    }
    release_display(
      self.api,
      self.display,
      self.fd,
      self.config,
      &mut self.surfaces,
      self.context,
    );
  }
}

fn release_display(
  api: &Api,
  display: *mut c_void,
  fd: i32,
  config: u32,
  surfaces: &mut Vec<u32>,
  context: u32,
) {
  unsafe {
    if context != INVALID_ID {
      (api.destroy_context)(display, context);
    }
    if !surfaces.is_empty() {
      (api.destroy_surfaces)(
        display,
        surfaces.as_mut_ptr(),
        i32::try_from(surfaces.len()).unwrap_or(0),
      );
      surfaces.clear();
    }
    if config != INVALID_ID {
      (api.destroy_config)(display, config);
    }
    if !display.is_null() {
      (api.terminate)(display);
    }
    if fd >= 0 {
      close(fd);
    }
  }
}

fn profile_order(profile_idc: u8) -> &'static [i32] {
  match profile_idc {
    66 | 44 => &[13, 5, 6, 7],
    77 => &[6, 7, 13, 5],
    _ => &[7, 6, 13, 5],
  }
}

fn picture_param(
  sps: &Sps,
  pps: &Pps,
  slice: &Slice,
  surface: u32,
  refs: &[Reference],
  counts: (i32, i32, i32),
) -> Result<VaPictureParameterBufferH264, DecodeError> {
  let mut param: VaPictureParameterBufferH264 = unsafe { std::mem::zeroed() };
  param.curr_pic = VaPictureH264 {
    picture_id: surface,
    frame_idx: slice.frame_num,
    flags: if slice.nal_ref_idc == 0 {
      0
    } else if slice.long_term {
      LONG_TERM
    } else {
      SHORT_TERM
    },
    top_field_order_cnt: counts.0,
    bottom_field_order_cnt: counts.1,
    reserved: [0; 4],
  };
  param.reference_frames = [invalid_picture(); 16];
  for (index, reference) in refs.iter().take(16).enumerate() {
    param.reference_frames[index] = referenced(reference);
  }
  param.picture_width_in_mbs_minus1 =
    u16::try_from(sps.mb_width - 1).map_err(|_| DecodeError::Rejected)?;
  param.picture_height_in_mbs_minus1 =
    u16::try_from(sps.mb_height - 1).map_err(|_| DecodeError::Rejected)?;
  param.num_ref_frames =
    u8::try_from(sps.num_ref_frames).map_err(|_| DecodeError::Rejected)?;
  param.seq_fields = seq_fields(sps);
  param.pic_init_qp_minus26 = narrow_i8(pps.init_qp_minus26)?;
  param.pic_init_qs_minus26 = narrow_i8(pps.init_qs_minus26)?;
  param.chroma_qp_index_offset = narrow_i8(pps.chroma_qp_index_offset)?;
  param.second_chroma_qp_index_offset =
    narrow_i8(pps.second_chroma_qp_index_offset)?;
  param.pic_fields = pic_fields(pps, slice.nal_ref_idc != 0);
  param.frame_num =
    u16::try_from(slice.frame_num).map_err(|_| DecodeError::Rejected)?;
  Ok(param)
}

fn narrow_i8(value: i32) -> Result<i8, DecodeError> {
  i8::try_from(value).map_err(|_| DecodeError::Rejected)
}

fn iq_matrix(pps: &Pps) -> VaIqMatrixBufferH264 {
  let mut matrix: VaIqMatrixBufferH264 = unsafe { std::mem::zeroed() };
  matrix.scaling_list_4x4 = pps.matrix4;
  matrix.scaling_list_8x8 = [pps.matrix8[0], pps.matrix8[3]];
  matrix
}

fn slice_param(
  slice: &Slice,
  refs: &[Reference],
  current_poc: i32,
) -> Result<VaSliceParameterBufferH264, DecodeError> {
  let mut param: VaSliceParameterBufferH264 = unsafe { std::mem::zeroed() };
  param.slice_data_size =
    u32::try_from(slice.bytes.len()).map_err(|_| DecodeError::Rejected)?;
  param.slice_data_flag = 0;
  param.slice_data_bit_offset = slice.header_bits;
  param.first_mb_in_slice =
    u16::try_from(slice.first_mb).map_err(|_| DecodeError::Rejected)?;
  param.slice_type = slice.slice_type;
  param.direct_spatial_mv_pred_flag = slice.direct_spatial;
  param.num_ref_idx_l0_active_minus1 = minus_one(slice.ref_l0)?;
  param.num_ref_idx_l1_active_minus1 = minus_one(slice.ref_l1)?;
  param.cabac_init_idc = slice.cabac_init_idc;
  param.slice_qp_delta =
    i8::try_from(slice.qp_delta).map_err(|_| DecodeError::Rejected)?;
  param.disable_deblocking_filter_idc = slice.disable_deblocking;
  param.slice_alpha_c0_offset_div2 =
    i8::try_from(slice.alpha).map_err(|_| DecodeError::Rejected)?;
  param.slice_beta_offset_div2 =
    i8::try_from(slice.beta).map_err(|_| DecodeError::Rejected)?;
  param.ref_pic_list0 = [invalid_picture(); 32];
  param.ref_pic_list1 = [invalid_picture(); 32];
  if slice.slice_type != 2 && slice.slice_type != 4 {
    for (index, reference) in
      ref_list(refs, current_poc).into_iter().enumerate()
    {
      if index >= 32 {
        break;
      }
      param.ref_pic_list0[index] = referenced(&reference);
    }
  }
  param.luma_log2_weight_denom = slice.weights.luma_log2;
  param.chroma_log2_weight_denom = slice.weights.chroma_log2;
  param.luma_weight_l0_flag = u8::from(slice.weights.luma_flag[0]);
  param.chroma_weight_l0_flag = u8::from(slice.weights.chroma_flag[0]);
  param.luma_weight_l1_flag = u8::from(slice.weights.luma_flag[1]);
  param.chroma_weight_l1_flag = u8::from(slice.weights.chroma_flag[1]);
  param.luma_weight_l0 = slice.weights.luma_weight[0];
  param.luma_offset_l0 = slice.weights.luma_offset[0];
  param.chroma_weight_l0 = slice.weights.chroma_weight[0];
  param.chroma_offset_l0 = slice.weights.chroma_offset[0];
  param.luma_weight_l1 = slice.weights.luma_weight[1];
  param.luma_offset_l1 = slice.weights.luma_offset[1];
  param.chroma_weight_l1 = slice.weights.chroma_weight[1];
  param.chroma_offset_l1 = slice.weights.chroma_offset[1];
  Ok(param)
}

fn minus_one(count: u32) -> Result<u8, DecodeError> {
  if count == 0 {
    Ok(0)
  } else {
    u8::try_from(count - 1).map_err(|_| DecodeError::Rejected)
  }
}

fn ref_list(refs: &[Reference], current_poc: i32) -> Vec<Reference> {
  let mut list: Vec<Reference> = refs
    .iter()
    .filter(|reference| !reference.long_term && reference.poc < current_poc)
    .cloned()
    .collect();
  list.sort_by_key(|reference| std::cmp::Reverse(reference.poc));
  let mut later: Vec<Reference> = refs
    .iter()
    .filter(|reference| !reference.long_term && reference.poc >= current_poc)
    .cloned()
    .collect();
  later.sort_by_key(|reference| reference.poc);
  list.extend(later);
  let mut long_term: Vec<Reference> = refs
    .iter()
    .filter(|reference| reference.long_term)
    .cloned()
    .collect();
  long_term.sort_by_key(|reference| reference.frame_num);
  list.extend(long_term);
  list
}

fn referenced(reference: &Reference) -> VaPictureH264 {
  VaPictureH264 {
    picture_id: reference.surface,
    frame_idx: reference.frame_num,
    flags: if reference.long_term {
      LONG_TERM
    } else {
      SHORT_TERM
    },
    top_field_order_cnt: reference.poc,
    bottom_field_order_cnt: reference.poc,
    reserved: [0; 4],
  }
}

fn invalid_picture() -> VaPictureH264 {
  VaPictureH264 {
    picture_id: INVALID_ID,
    frame_idx: 0,
    flags: PICTURE_INVALID,
    top_field_order_cnt: 0,
    bottom_field_order_cnt: 0,
    reserved: [0; 4],
  }
}

fn seq_fields(sps: &Sps) -> u32 {
  let mut value = sps.chroma_format_idc & 0x3;
  if sps.gaps_in_frame_num_allowed {
    value |= 1 << 3;
  }
  if sps.frame_mbs_only {
    value |= 1 << 4;
  }
  if sps.mb_aff {
    value |= 1 << 5;
  }
  if sps.direct_8x8 {
    value |= 1 << 6;
  }
  if sps.level_idc >= 31 {
    value |= 1 << 7;
  }
  value |= (sps.log2_max_frame_num - 4) << 8;
  value |= (sps.poc_type & 0x3) << 12;
  value |= sps.log2_max_poc_lsb.saturating_sub(4) << 14;
  if sps.delta_pic_order_always_zero {
    value |= 1 << 18;
  }
  value
}

fn pic_fields(pps: &Pps, reference: bool) -> u32 {
  let mut value = 0u32;
  if pps.cabac {
    value |= 1;
  }
  if pps.weighted_pred {
    value |= 1 << 1;
  }
  value |= (pps.weighted_bipred_idc & 0x3) << 2;
  if pps.transform_8x8 {
    value |= 1 << 4;
  }
  if pps.constrained_intra {
    value |= 1 << 6;
  }
  if pps.pic_order_present {
    value |= 1 << 7;
  }
  if pps.deblocking {
    value |= 1 << 8;
  }
  if pps.redundant_pic_cnt {
    value |= 1 << 9;
  }
  if reference {
    value |= 1 << 10;
  }
  value
}

fn create_buffer(
  api: &Api,
  display: *mut c_void,
  context: u32,
  kind: i32,
  data: *mut c_void,
  len: usize,
) -> Result<u32, DecodeError> {
  let size = u32::try_from(len).map_err(|_| DecodeError::Rejected)?;
  let mut id = INVALID_ID;
  let status = unsafe {
    (api.create_buffer)(display, context, kind, size, 1, data, &mut id)
  };
  if status != 0 || id == INVALID_ID {
    Err(DecodeError::Rejected)
  } else {
    Ok(id)
  }
}

fn map_image(
  api: &Api,
  display: *mut c_void,
  image: &VaImage,
  visible: (u32, u32, u32, u32),
) -> Result<Raster, DecodeError> {
  if image.data_size == 0 {
    return Err(DecodeError::Rejected);
  }
  let mut pointer = null_mut();
  let status = unsafe { (api.map_buffer)(display, image.buf, &mut pointer) };
  if status != 0 || pointer.is_null() {
    return Err(DecodeError::Rejected);
  }
  let bytes = unsafe {
    std::slice::from_raw_parts(pointer.cast::<u8>(), image.data_size as usize)
  };
  let raster = image_rgba(image, bytes, visible);
  unsafe { (api.unmap_buffer)(display, image.buf) };
  let rgba = raster.ok_or(DecodeError::Rejected)?;
  Raster::from_rgba8(visible.2, visible.3, ColorSpace::Srgb, &rgba)
    .map_err(|_| DecodeError::Rejected)
}

fn image_rgba(
  image: &VaImage,
  bytes: &[u8],
  visible: (u32, u32, u32, u32),
) -> Option<Vec<u8>> {
  let width = usize::from(image.width);
  let height = usize::from(image.height);
  if width == 0 || height == 0 {
    return None;
  }
  let full = match image.format.fourcc {
    NV12 => nv_plane(bytes, image, width, height, false),
    NV21 => nv_plane(bytes, image, width, height, true),
    I420 => yuv_planes(bytes, image, width, height, false),
    YV12 => yuv_planes(bytes, image, width, height, true),
    _ => None,
  }?;
  crop_rect(
    &full,
    width,
    height,
    visible.0 as usize,
    visible.1 as usize,
    visible.2 as usize,
    visible.3 as usize,
  )
}

fn nv_plane(
  bytes: &[u8],
  image: &VaImage,
  width: usize,
  height: usize,
  swap: bool,
) -> Option<Vec<u8>> {
  let y_stride = image.pitches[0] as usize;
  let y = bytes.get(image.offsets[0] as usize..)?;
  let (uv, uv_stride) = if image.num_planes >= 2 {
    (
      bytes.get(image.offsets[1] as usize..)?,
      image.pitches[1] as usize,
    )
  } else {
    let offset = y_stride.checked_mul(height)?;
    (bytes.get(offset..)?, y_stride)
  };
  if swap {
    let swapped = swap_chroma(uv);
    color::nv12_to_rgba(y, &swapped, y_stride, uv_stride, width, height, true)
  } else {
    color::nv12_to_rgba(y, uv, y_stride, uv_stride, width, height, true)
  }
}

fn swap_chroma(uv: &[u8]) -> Vec<u8> {
  let mut swapped = uv.to_vec();
  for pair in swapped.chunks_exact_mut(2) {
    pair.swap(0, 1);
  }
  swapped
}

fn yuv_planes(
  bytes: &[u8],
  image: &VaImage,
  width: usize,
  height: usize,
  swap: bool,
) -> Option<Vec<u8>> {
  let y = bytes.get(image.offsets[0] as usize..)?;
  let u = bytes.get(image.offsets[1] as usize..)?;
  let v = bytes.get(image.offsets[2] as usize..)?;
  let (u, v) = if swap { (v, u) } else { (u, v) };
  color::i420_to_rgba(
    I420 {
      y,
      u,
      v,
      y_stride: image.pitches[0] as usize,
      u_stride: image.pitches[1] as usize,
      v_stride: image.pitches[2] as usize,
    },
    width,
    height,
    true,
  )
}

fn crop_rect(
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
    return Some(rgba[..width * height * 4].to_vec());
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

fn zero_image() -> VaImage {
  unsafe { std::mem::zeroed() }
}

fn supports(profiles: &[i32]) -> bool {
  let Some(api) = api() else {
    return false;
  };
  let Some((fd, display)) = open_display(api) else {
    return false;
  };
  let found = select_profile(api, display, profiles).is_some();
  unsafe {
    (api.terminate)(display);
    close(fd);
  }
  found
}

fn select_profile(
  api: &Api,
  display: *mut c_void,
  preferred: &[i32],
) -> Option<i32> {
  let max = unsafe { (api.max_profiles)(display) };
  if max <= 0 {
    return None;
  }
  let mut profiles = vec![0i32; max as usize];
  let mut count = 0i32;
  let status =
    unsafe { (api.query_profiles)(display, profiles.as_mut_ptr(), &mut count) };
  if status != 0 || count <= 0 {
    return None;
  }
  profiles.truncate(count as usize);
  preferred
    .iter()
    .copied()
    .find(|profile| profiles.contains(profile))
}

fn open_display(api: &Api) -> Option<(i32, *mut c_void)> {
  for index in 128..136 {
    let path = format!("/dev/dri/renderD{index}");
    let Ok(file) = OpenOptions::new().read(true).write(true).open(&path) else {
      continue;
    };
    let fd = file.into_raw_fd();
    let display = unsafe { (api.get_display)(fd) };
    if display.is_null() {
      unsafe { close(fd) };
      continue;
    }
    let mut major = 0;
    let mut minor = 0;
    let status = unsafe { (api.initialize)(display, &mut major, &mut minor) };
    if status != 0 {
      unsafe { close(fd) };
      continue;
    }
    return Some((fd, display));
  }
  None
}

fn api() -> Option<&'static Api> {
  static API: OnceLock<Option<Api>> = OnceLock::new();
  API.get_or_init(Api::load).as_ref()
}

impl Api {
  fn load() -> Option<Self> {
    unsafe {
      let lib = Library::new("libva.so.2").ok()?;
      let drm = Library::new("libva-drm.so.2").ok()?;
      Some(Self {
        get_display: *drm.get(b"vaGetDisplayDRM\0").ok()?,
        initialize: *lib.get(b"vaInitialize\0").ok()?,
        terminate: *lib.get(b"vaTerminate\0").ok()?,
        max_profiles: *lib.get(b"vaMaxNumProfiles\0").ok()?,
        query_profiles: *lib.get(b"vaQueryConfigProfiles\0").ok()?,
        create_config: *lib.get(b"vaCreateConfig\0").ok()?,
        destroy_config: *lib.get(b"vaDestroyConfig\0").ok()?,
        create_context: *lib.get(b"vaCreateContext\0").ok()?,
        destroy_context: *lib.get(b"vaDestroyContext\0").ok()?,
        create_surfaces: *lib.get(b"vaCreateSurfaces\0").ok()?,
        destroy_surfaces: *lib.get(b"vaDestroySurfaces\0").ok()?,
        create_buffer: *lib.get(b"vaCreateBuffer\0").ok()?,
        destroy_buffer: *lib.get(b"vaDestroyBuffer\0").ok()?,
        begin_picture: *lib.get(b"vaBeginPicture\0").ok()?,
        render_picture: *lib.get(b"vaRenderPicture\0").ok()?,
        end_picture: *lib.get(b"vaEndPicture\0").ok()?,
        sync_surface: *lib.get(b"vaSyncSurface\0").ok()?,
        derive_image: *lib.get(b"vaDeriveImage\0").ok()?,
        create_image: *lib.get(b"vaCreateImage\0").ok()?,
        get_image: *lib.get(b"vaGetImage\0").ok()?,
        destroy_image: *lib.get(b"vaDestroyImage\0").ok()?,
        map_buffer: *lib.get(b"vaMapBuffer\0").ok()?,
        unmap_buffer: *lib.get(b"vaUnmapBuffer\0").ok()?,
        _lib: lib,
        _drm: drm,
      })
    }
  }
}

unsafe extern "C" {
  fn close(fd: i32) -> i32;
}
