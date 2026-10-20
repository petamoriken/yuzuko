//! The Windows bundle icon as resources of the packaged executable.
//!
//! Explorer, the taskbar pin, and the Start menu read an executable's icon
//! from its first `RT_GROUP_ICON` resource. An `.ico` file holds the same
//! directory with file offsets. Each image becomes an `RT_ICON` resource,
//! and the directory becomes `RT_GROUP_ICON` 1 with resource IDs in place of
//! offsets.

use crate::bundle::config::ConfigError;

/// One image of an `.ico` file. Only a Windows host writes it out.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) struct IconImage<'a> {
  /// The 12 directory bytes shared by the file and the resource: width,
  /// height, color count, reserved, planes, bit count, and byte size.
  header: [u8; 12],
  data: &'a [u8],
}

/// Reads the images of an `.ico` file. A file that is not a well-formed icon
/// directory is rejected rather than embedded.
pub(crate) fn parse_ico(
  bytes: &[u8],
) -> Result<Vec<IconImage<'_>>, ConfigError> {
  let invalid = || ConfigError::new("icons.windows is not a valid .ico file.");
  let word = |at: usize| -> Option<u16> {
    bytes
      .get(at..at + 2)
      .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
  };
  let dword = |at: usize| -> Option<u32> {
    bytes
      .get(at..at + 4)
      .map(|quad| u32::from_le_bytes([quad[0], quad[1], quad[2], quad[3]]))
  };
  if word(0) != Some(0) || word(2) != Some(1) {
    return Err(invalid());
  }
  let count = usize::from(word(4).ok_or_else(invalid)?);
  if count == 0 {
    return Err(invalid());
  }
  let mut images = Vec::with_capacity(count);
  for index in 0..count {
    let entry = 6 + index * 16;
    let header: [u8; 12] = bytes
      .get(entry..entry + 12)
      .and_then(|slice| slice.try_into().ok())
      .ok_or_else(invalid)?;
    let size = dword(entry + 8).ok_or_else(invalid)? as usize;
    let offset = dword(entry + 12).ok_or_else(invalid)? as usize;
    let data = offset
      .checked_add(size)
      .and_then(|end| bytes.get(offset..end))
      .filter(|data| !data.is_empty())
      .ok_or_else(invalid)?;
    images.push(IconImage { header, data });
  }
  Ok(images)
}

/// The `RT_GROUP_ICON` resource for `images` stored as `RT_ICON` resources
/// numbered from `first_id`.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn group_resource(
  images: &[IconImage<'_>],
  first_id: u16,
) -> Vec<u8> {
  let mut group = Vec::with_capacity(6 + images.len() * 14);
  group.extend_from_slice(&0u16.to_le_bytes());
  group.extend_from_slice(&1u16.to_le_bytes());
  group.extend_from_slice(&(images.len() as u16).to_le_bytes());
  for (index, image) in images.iter().enumerate() {
    group.extend_from_slice(&image.header);
    group.extend_from_slice(&(first_id + index as u16).to_le_bytes());
  }
  group
}

/// Writes the icon of `ico` into the executable at `exe`.
#[cfg(windows)]
pub(crate) fn embed(
  exe: &std::path::Path,
  ico: &[u8],
) -> Result<(), ConfigError> {
  use std::os::windows::ffi::OsStrExt;
  use windows_sys::Win32::System::LibraryLoader::{
    BeginUpdateResourceW, EndUpdateResourceW, UpdateResourceW,
  };
  use windows_sys::Win32::UI::WindowsAndMessaging::{RT_GROUP_ICON, RT_ICON};

  const FIRST_ID: u16 = 1;
  const GROUP_ID: u16 = 1;
  const LANG_NEUTRAL: u16 = 0;
  let images = parse_ico(ico)?;
  if images.len() > usize::from(u16::MAX - FIRST_ID) {
    return Err(ConfigError::new("icons.windows has too many images."));
  }
  let failed = |step: &str| {
    ConfigError::new(format!(
      "cannot write the icon into {}: {step} failed: {}",
      exe.display(),
      std::io::Error::last_os_error()
    ))
  };
  let path: Vec<u16> = exe
    .as_os_str()
    .encode_wide()
    .chain(std::iter::once(0))
    .collect();
  let update = unsafe { BeginUpdateResourceW(path.as_ptr(), 0) };
  if update.is_null() {
    return Err(failed("BeginUpdateResource"));
  }
  let put = |kind: *const u16, id: u16, data: &[u8]| -> bool {
    let size = u32::try_from(data.len()).unwrap_or(u32::MAX);
    unsafe {
      UpdateResourceW(
        update,
        kind,
        id as usize as *const u16,
        LANG_NEUTRAL,
        data.as_ptr().cast(),
        size,
      ) != 0
    }
  };
  let mut written = true;
  for (index, image) in images.iter().enumerate() {
    written &= put(RT_ICON, FIRST_ID + index as u16, image.data);
  }
  written &= put(RT_GROUP_ICON, GROUP_ID, &group_resource(&images, FIRST_ID));
  if !written {
    let error = failed("UpdateResource");
    unsafe { EndUpdateResourceW(update, 1) };
    return Err(error);
  }
  if unsafe { EndUpdateResourceW(update, 0) } == 0 {
    return Err(failed("EndUpdateResource"));
  }
  Ok(())
}

#[cfg(test)]
mod tests {
  use super::{group_resource, parse_ico};

  /// An `.ico` with two images of the given payloads.
  fn ico(first: &[u8], second: &[u8]) -> Vec<u8> {
    let mut file = vec![0, 0, 1, 0, 2, 0];
    let offset = 6 + 2 * 16;
    for (index, (size, payload)) in
      [(16u8, first), (32u8, second)].into_iter().enumerate()
    {
      let at = offset + if index == 0 { 0 } else { first.len() };
      file.extend_from_slice(&[size, size, 0, 0, 1, 0, 32, 0]);
      file.extend_from_slice(&(payload.len() as u32).to_le_bytes());
      file.extend_from_slice(&(at as u32).to_le_bytes());
    }
    file.extend_from_slice(first);
    file.extend_from_slice(second);
    file
  }

  #[test]
  fn icon_directory_becomes_a_group_with_resource_ids() {
    let file = ico(b"first-image", b"second");
    let images = parse_ico(&file).unwrap();
    assert_eq!(images.len(), 2);
    assert_eq!(images[0].data, b"first-image");
    assert_eq!(images[1].data, b"second");
    let group = group_resource(&images, 1);
    assert_eq!(&group[..6], &[0, 0, 1, 0, 2, 0]);
    assert_eq!(group.len(), 6 + 2 * 14);
    assert_eq!(&group[6..18], &file[6..18]);
    assert_eq!(&group[18..20], &1u16.to_le_bytes());
    assert_eq!(&group[20..32], &file[22..34]);
    assert_eq!(&group[32..34], &2u16.to_le_bytes());
  }

  #[test]
  fn malformed_icons_are_rejected() {
    assert!(parse_ico(b"ICO-BYTES").is_err());
    assert!(parse_ico(&[0, 0, 1, 0, 0, 0]).is_err());
    assert!(parse_ico(&[0, 0, 2, 0, 1, 0]).is_err());
    let mut truncated = ico(b"first-image", b"second");
    truncated.truncate(truncated.len() - 1);
    assert!(parse_ico(&truncated).is_err());
  }
}
