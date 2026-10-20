//! Application identity for a packaged Windows or Linux directory bundle.
//!
//! A macOS `.app` carries its identity in `Info.plist`. A directory bundle
//! stores the identifier and icon bytes beside the executable, and the
//! runtime applies them before the first window opens: the Windows
//! AppUserModelID and window icon, and the Linux `WM_CLASS`, `app_id`, and
//! window icon.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

struct Identity {
  /// Windows already applied it as the AppUserModelID.
  #[cfg_attr(not(all(unix, not(target_os = "macos"))), allow(dead_code))]
  identifier: String,
  resources: PathBuf,
}

static IDENTITY: Mutex<Option<Identity>> = Mutex::new(None);

/// Records the packaged identifier and the directory that holds the bundle
/// icons. On Windows this also sets the process AppUserModelID, which must
/// happen before a window is shown.
pub fn set_app_identity(identifier: &str, resources: &Path) {
  #[cfg(target_os = "windows")]
  set_app_user_model_id(identifier);
  *IDENTITY.lock().unwrap_or_else(|error| error.into_inner()) =
    Some(Identity {
      identifier: identifier.to_string(),
      resources: resources.to_path_buf(),
    });
}

/// Adds the packaged identity to new window attributes.
pub(crate) fn apply(
  attributes: winit::window::WindowAttributes,
) -> winit::window::WindowAttributes {
  let slot = IDENTITY.lock().unwrap_or_else(|error| error.into_inner());
  let Some(identity) = slot.as_ref() else {
    return attributes;
  };
  apply_platform(attributes, identity)
}

#[cfg(target_os = "windows")]
fn set_app_user_model_id(identifier: &str) {
  use windows_sys::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID;
  let wide: Vec<u16> = identifier
    .encode_utf16()
    .chain(std::iter::once(0))
    .collect();
  unsafe { SetCurrentProcessExplicitAppUserModelID(wide.as_ptr()) };
}

#[cfg(target_os = "windows")]
fn apply_platform(
  attributes: winit::window::WindowAttributes,
  identity: &Identity,
) -> winit::window::WindowAttributes {
  use winit::platform::windows::{IconExtWindows, WindowAttributesExtWindows};
  use winit::window::Icon;
  let path = identity.resources.join("AppIcon.ico");
  if !path.is_file() {
    return attributes;
  }
  // Windows picks the frame and taskbar sizes from the `.ico` directory.
  let small = Icon::from_path(&path, None).ok();
  let large = Icon::from_path(&path, None).ok();
  attributes.with_window_icon(small).with_taskbar_icon(large)
}

#[cfg(all(unix, not(target_os = "macos")))]
fn apply_platform(
  attributes: winit::window::WindowAttributes,
  identity: &Identity,
) -> winit::window::WindowAttributes {
  use winit::platform::wayland::WindowAttributesExtWayland;
  use winit::platform::x11::WindowAttributesExtX11;
  let id = identity.identifier.as_str();
  // X11 reads `WM_CLASS`; Wayland reads `app_id`. Only the running backend
  // uses its own value.
  let attributes = WindowAttributesExtX11::with_name(attributes, id, id);
  let attributes = WindowAttributesExtWayland::with_name(attributes, id, id);
  match largest_icon(&identity.resources.join("icons")) {
    Some(icon) => attributes.with_window_icon(Some(icon)),
    None => attributes,
  }
}

#[cfg(target_os = "macos")]
fn apply_platform(
  attributes: winit::window::WindowAttributes,
  identity: &Identity,
) -> winit::window::WindowAttributes {
  let _ = (&identity.identifier, &identity.resources);
  attributes
}

/// Decodes the widest PNG in `dir` into a winit icon. A file that does not
/// decode is skipped.
#[cfg(all(unix, not(target_os = "macos")))]
fn largest_icon(dir: &Path) -> Option<winit::window::Icon> {
  let mut best: Option<(u32, Vec<u8>, u32)> = None;
  for entry in std::fs::read_dir(dir).ok()?.flatten() {
    let path = entry.path();
    let is_png = path
      .extension()
      .and_then(|ext| ext.to_str())
      .is_some_and(|ext| ext.eq_ignore_ascii_case("png"));
    if !is_png {
      continue;
    }
    let Some((rgba, width, height)) = decode_rgba(&path) else {
      continue;
    };
    if best.as_ref().is_none_or(|(w, _, _)| width > *w) {
      best = Some((width, rgba, height));
    }
  }
  let (width, rgba, height) = best?;
  winit::window::Icon::from_rgba(rgba, width, height).ok()
}

/// RGBA8 pixels of a PNG file, or `None` when it does not decode.
#[cfg(any(test, all(unix, not(target_os = "macos"))))]
fn decode_rgba(path: &Path) -> Option<(Vec<u8>, u32, u32)> {
  let file = std::fs::File::open(path).ok()?;
  let mut decoder = png::Decoder::new(std::io::BufReader::new(file));
  decoder.set_transformations(png::Transformations::normalize_to_color8());
  let mut reader = decoder.read_info().ok()?;
  let mut buf = vec![0; reader.output_buffer_size()?];
  let info = reader.next_frame(&mut buf).ok()?;
  let pixels = &buf[..info.buffer_size()];
  let rgba = match info.color_type {
    png::ColorType::Rgba => pixels.to_vec(),
    png::ColorType::Rgb => pixels
      .as_chunks::<3>()
      .0
      .iter()
      .flat_map(|&[r, g, b]| [r, g, b, 255])
      .collect(),
    png::ColorType::GrayscaleAlpha => pixels
      .as_chunks::<2>()
      .0
      .iter()
      .flat_map(|&[v, a]| [v, v, v, a])
      .collect(),
    png::ColorType::Grayscale => {
      pixels.iter().flat_map(|&v| [v, v, v, 255]).collect()
    }
    png::ColorType::Indexed => return None,
  };
  Some((rgba, info.width, info.height))
}

#[cfg(test)]
mod tests {
  use super::decode_rgba;

  #[test]
  fn icon_png_decodes_to_rgba() {
    let dir = std::env::temp_dir()
      .join(format!("yuzuko-identity-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("icon.png");
    let file = std::fs::File::create(&path).unwrap();
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), 2, 1);
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().unwrap();
    writer.write_image_data(&[255, 0, 0, 0, 0, 255]).unwrap();
    writer.finish().unwrap();
    let (rgba, width, height) = decode_rgba(&path).unwrap();
    assert_eq!((width, height), (2, 1));
    assert_eq!(rgba, vec![255, 0, 0, 255, 0, 0, 255, 255]);
    assert!(decode_rgba(&dir.join("missing.png")).is_none());
    let _ = std::fs::remove_dir_all(&dir);
  }
}
