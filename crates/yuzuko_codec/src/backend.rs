//! H.264 stays on the platform hardware decoder. There is no software fallback.

use yuzuko_image::Raster;

use crate::error::DecodeError;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Codec {
  Avc,
}

pub struct VideoConfig {
  pub codec: Codec,
  pub description: Vec<u8>,
  pub width: u32,
  pub height: u32,
}

pub struct VideoBackend {
  inner: VideoInner,
  /// Empty `description`: samples are Annex B until rewritten.
  annex_b: bool,
  width: u32,
  height: u32,
}

enum VideoInner {
  #[cfg(target_os = "macos")]
  Hardware(crate::platform::Session),
  #[cfg(target_os = "windows")]
  Hardware(crate::platform::Session),
  #[cfg(target_os = "linux")]
  Hardware(crate::platform::Session),
  /// Annex B, waiting for an access unit that contains SPS and PPS.
  Deferred,
  #[cfg(not(any(
    target_os = "macos",
    target_os = "windows",
    target_os = "linux"
  )))]
  Missing,
}

impl VideoBackend {
  pub fn open(config: VideoConfig) -> Result<Self, DecodeError> {
    if config.codec != Codec::Avc || !hardware_avc() {
      return Err(DecodeError::Unsupported);
    }
    let width = config.width;
    let height = config.height;
    if config.description.is_empty() {
      return Ok(Self {
        inner: VideoInner::Deferred,
        annex_b: true,
        width,
        height,
      });
    }
    Ok(Self {
      inner: hardware_session(&config.description, width, height)?,
      annex_b: false,
      width,
      height,
    })
  }

  pub fn decode(&mut self, bytes: &[u8]) -> Result<Vec<Raster>, DecodeError> {
    let converted = if self.annex_b {
      Some(crate::h264::annex_b_access_unit(bytes)?)
    } else {
      None
    };
    if let Some(unit) = &converted {
      if matches!(self.inner, VideoInner::Deferred) {
        let avcc = unit.avcc.as_deref().ok_or(DecodeError::Rejected)?;
        self.inner = hardware_session(avcc, self.width, self.height)?;
      }
    }
    let sample = match &converted {
      Some(unit) => unit.sample.as_slice(),
      None => bytes,
    };
    match &mut self.inner {
      #[cfg(any(
        target_os = "macos",
        target_os = "windows",
        target_os = "linux"
      ))]
      VideoInner::Hardware(session) => session.decode(sample),
      VideoInner::Deferred => Err(DecodeError::Rejected),
      #[cfg(not(any(
        target_os = "macos",
        target_os = "windows",
        target_os = "linux"
      )))]
      VideoInner::Missing => Err(DecodeError::Unsupported),
    }
  }

  pub fn flush(&mut self) -> Result<Vec<Raster>, DecodeError> {
    match &mut self.inner {
      #[cfg(any(
        target_os = "macos",
        target_os = "windows",
        target_os = "linux"
      ))]
      VideoInner::Hardware(session) => session.flush(),
      VideoInner::Deferred => Ok(Vec::new()),
      #[cfg(not(any(
        target_os = "macos",
        target_os = "windows",
        target_os = "linux"
      )))]
      VideoInner::Missing => Err(DecodeError::Unsupported),
    }
  }
}

#[cfg(target_os = "macos")]
fn hardware_avc() -> bool {
  crate::platform::supports_avc()
}

#[cfg(target_os = "macos")]
fn hardware_session(
  description: &[u8],
  width: u32,
  height: u32,
) -> Result<VideoInner, DecodeError> {
  let _ = (width, height);
  Ok(VideoInner::Hardware(crate::platform::open_avc(
    description,
  )?))
}

#[cfg(target_os = "windows")]
fn hardware_avc() -> bool {
  crate::platform::supports_avc()
}

#[cfg(target_os = "windows")]
fn hardware_session(
  description: &[u8],
  width: u32,
  height: u32,
) -> Result<VideoInner, DecodeError> {
  Ok(VideoInner::Hardware(crate::platform::open_avc(
    description,
    width,
    height,
  )?))
}

#[cfg(target_os = "linux")]
fn hardware_avc() -> bool {
  crate::platform::supports_avc()
}

#[cfg(target_os = "linux")]
fn hardware_session(
  description: &[u8],
  width: u32,
  height: u32,
) -> Result<VideoInner, DecodeError> {
  Ok(VideoInner::Hardware(crate::platform::open_avc(
    description,
    width,
    height,
  )?))
}

#[cfg(not(any(
  target_os = "macos",
  target_os = "windows",
  target_os = "linux"
)))]
fn hardware_avc() -> bool {
  false
}

#[cfg(not(any(
  target_os = "macos",
  target_os = "windows",
  target_os = "linux"
)))]
fn hardware_session(
  _description: &[u8],
  _width: u32,
  _height: u32,
) -> Result<VideoInner, DecodeError> {
  Err(DecodeError::Unsupported)
}
