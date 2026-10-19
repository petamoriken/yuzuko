//! sRGB and Display P3 conversion, plus ICC profiles that are neither.

use moxcms::ColorProfile;
use moxcms::Layout;
use moxcms::TransformOptions;

use crate::raster::ColorSpace;
use crate::raster::Raster;
use crate::raster::SampleFormat;

const LIN_SRGB_TO_XYZ: [[f64; 3]; 3] = [
  [
    0.412_390_799_265_959_34,
    0.357_584_339_383_878,
    0.180_480_788_401_834_3,
  ],
  [
    0.212_639_005_871_510_27,
    0.715_168_678_767_756,
    0.072_192_315_360_733_71,
  ],
  [
    0.019_330_818_715_591_82,
    0.119_194_779_794_625_98,
    0.950_532_152_249_660_7,
  ],
];

const XYZ_TO_LIN_SRGB: [[f64; 3]; 3] = [
  [
    3.240_969_941_904_522_6,
    -1.537_383_177_570_094,
    -0.498_610_760_293_003_4,
  ],
  [
    -0.969_243_636_280_879_6,
    1.875_967_501_507_720_2,
    0.041_555_057_407_175_59,
  ],
  [
    0.055_630_079_696_993_66,
    -0.203_976_958_888_976_52,
    1.056_971_514_242_878_6,
  ],
];

const LIN_P3_TO_XYZ: [[f64; 3]; 3] = [
  [
    0.486_570_948_648_216_2,
    0.265_667_693_169_093_06,
    0.198_217_285_234_362_5,
  ],
  [
    0.228_974_564_069_748_8,
    0.691_738_521_836_506_4,
    0.079_286_914_093_745,
  ],
  [0.0, 0.045_113_381_858_902_64, 1.043_944_368_900_976],
];

const XYZ_TO_LIN_P3: [[f64; 3]; 3] = [
  [
    2.493_496_911_941_425,
    -0.931_383_617_919_123_9,
    -0.402_710_784_450_716_84,
  ],
  [
    -0.829_488_969_561_574_7,
    1.762_664_060_318_346_3,
    0.023_624_685_841_943_577,
  ],
  [
    0.035_845_830_243_784_47,
    -0.076_172_389_268_041_82,
    0.956_884_524_007_687_2,
  ],
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KnownSpace {
  Srgb,
  DisplayP3,
  Other,
}

#[derive(Clone, Copy, Debug)]
pub struct Cicp {
  pub primaries: u8,
  pub transfer: u8,
}

pub fn identify(icc: Option<&[u8]>, cicp: Option<Cicp>) -> KnownSpace {
  if let Some(cicp) = cicp {
    return match (cicp.primaries, cicp.transfer) {
      (1, 13) => KnownSpace::Srgb,
      (12, 13) => KnownSpace::DisplayP3,
      _ => KnownSpace::Other,
    };
  }
  let Some(icc) = icc else {
    return KnownSpace::Srgb;
  };
  match ColorProfile::new_from_slice(icc) {
    Ok(profile) => classify_profile(&profile),
    Err(_) => KnownSpace::Other,
  }
}

fn classify_profile(profile: &ColorProfile) -> KnownSpace {
  let text = profile_text(profile);
  if text.contains("display p3") || text.contains("display-p3") {
    return KnownSpace::DisplayP3;
  }
  if text.contains("srgb") {
    return KnownSpace::Srgb;
  }
  KnownSpace::Other
}

fn profile_text(profile: &ColorProfile) -> String {
  format!("{profile:?}").to_ascii_lowercase()
}

/// Tags `raster` from the source profile. `"default"` stores sRGB.
/// A profile that is neither sRGB nor Display P3 is converted to sRGB.
pub fn apply_source(
  raster: &mut Raster,
  icc: Option<&[u8]>,
  cicp: Option<Cicp>,
  to_srgb: bool,
) -> Result<(), ()> {
  match identify(icc, cicp) {
    KnownSpace::Srgb => {
      raster.color_space = ColorSpace::Srgb;
      Ok(())
    }
    KnownSpace::DisplayP3 => {
      raster.color_space = ColorSpace::DisplayP3;
      if to_srgb {
        convert(raster, ColorSpace::Srgb);
      }
      Ok(())
    }
    KnownSpace::Other => {
      let Some(icc) = icc else {
        raster.color_space = ColorSpace::Srgb;
        return Ok(());
      };
      convert_icc(raster, icc)?;
      raster.color_space = ColorSpace::Srgb;
      Ok(())
    }
  }
}

pub fn convert(raster: &mut Raster, destination: ColorSpace) {
  if raster.color_space == destination {
    return;
  }
  let count = raster.width as usize * raster.height as usize;
  for pixel in 0..count {
    let index = pixel * 4;
    let encoded = [
      encoded_unit(raster, index),
      encoded_unit(raster, index + 1),
      encoded_unit(raster, index + 2),
    ];
    let converted = convert_encoded(raster.color_space, destination, encoded);
    write_encoded(raster, index, converted);
  }
  raster.color_space = destination;
}

fn encoded_unit(raster: &Raster, index: usize) -> f64 {
  let sample = raster.channel(index);
  match raster.format {
    SampleFormat::Unorm8 => sample / 255.0,
    SampleFormat::Float16 => sample,
  }
}

fn write_encoded(raster: &mut Raster, index: usize, rgb: [f64; 3]) {
  match raster.format {
    SampleFormat::Unorm8 => {
      for (offset, channel) in rgb.into_iter().enumerate() {
        raster.put_channel(index + offset, channel * 255.0);
      }
    }
    SampleFormat::Float16 => {
      for (offset, channel) in rgb.into_iter().enumerate() {
        raster.put_channel(index + offset, channel);
      }
    }
  }
}

pub fn convert_encoded(
  source: ColorSpace,
  destination: ColorSpace,
  encoded: [f64; 3],
) -> [f64; 3] {
  if source == destination {
    return encoded;
  }
  let linear = encoded.map(srgb_to_linear);
  let xyz = match source {
    ColorSpace::Srgb => mat_vec(LIN_SRGB_TO_XYZ, linear),
    ColorSpace::DisplayP3 => mat_vec(LIN_P3_TO_XYZ, linear),
  };
  let linear = match destination {
    ColorSpace::Srgb => mat_vec(XYZ_TO_LIN_SRGB, xyz),
    ColorSpace::DisplayP3 => mat_vec(XYZ_TO_LIN_P3, xyz),
  };
  linear.map(linear_to_srgb)
}

fn srgb_to_linear(channel: f64) -> f64 {
  if channel <= 0.04045 {
    channel / 12.92
  } else {
    ((channel + 0.055) / 1.055).powf(2.4)
  }
}

fn linear_to_srgb(channel: f64) -> f64 {
  if channel <= 0.003_130_8 {
    12.92 * channel
  } else {
    1.055 * channel.powf(1.0 / 2.4) - 0.055
  }
}

fn mat_vec(matrix: [[f64; 3]; 3], value: [f64; 3]) -> [f64; 3] {
  [
    matrix[0][0] * value[0] + matrix[0][1] * value[1] + matrix[0][2] * value[2],
    matrix[1][0] * value[0] + matrix[1][1] * value[1] + matrix[1][2] * value[2],
    matrix[2][0] * value[0] + matrix[2][1] * value[1] + matrix[2][2] * value[2],
  ]
}

fn convert_icc(raster: &mut Raster, icc: &[u8]) -> Result<(), ()> {
  let source = ColorProfile::new_from_slice(icc).map_err(|_| ())?;
  let destination = ColorProfile::new_srgb();
  match raster.format {
    SampleFormat::Unorm8 => {
      let transform = source
        .create_transform_8bit(
          Layout::Rgba,
          &destination,
          Layout::Rgba,
          TransformOptions::default(),
        )
        .map_err(|_| ())?;
      let source_bytes = raster.bytes.clone();
      transform
        .transform(&source_bytes, &mut raster.bytes)
        .map_err(|_| ())?;
    }
    SampleFormat::Float16 => convert_icc_f32(raster, &source, &destination)?,
  }
  Ok(())
}

fn convert_icc_f32(
  raster: &mut Raster,
  source: &ColorProfile,
  destination: &ColorProfile,
) -> Result<(), ()> {
  let transform = source
    .create_transform_f32(
      Layout::Rgba,
      destination,
      Layout::Rgba,
      TransformOptions::default(),
    )
    .map_err(|_| ())?;
  let count = raster.width as usize * raster.height as usize * 4;
  let mut samples = Vec::with_capacity(count);
  for index in 0..count {
    samples.push(raster.channel(index) as f32);
  }
  let input = samples.clone();
  transform.transform(&input, &mut samples).map_err(|_| ())?;
  for (index, sample) in samples.into_iter().enumerate() {
    raster.put_channel(index, f64::from(sample));
  }
  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn srgb_red_stays_red() {
    let pixel =
      convert_encoded(ColorSpace::Srgb, ColorSpace::Srgb, [1.0, 0.0, 0.0]);
    assert_eq!(pixel, [1.0, 0.0, 0.0]);
  }

  #[test]
  fn display_p3_red_is_outside_srgb() {
    let pixel =
      convert_encoded(ColorSpace::DisplayP3, ColorSpace::Srgb, [1.0, 0.0, 0.0]);
    assert!(pixel[0] > 1.0);
    assert!(pixel[1] < 0.0);
    assert!(pixel[2] < 0.0);
  }
}
