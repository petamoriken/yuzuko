//! H.264 stays on the platform hardware decoder. There is no software
//! fallback. AV1 uses that hardware decoder when the session opens, and
//! rav1d when it does not. `prefer-software` keeps AV1 on rav1d.

use yuzuko_image::Raster;

use crate::error::DecodeError;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Codec {
  Avc,
  Av1,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Acceleration {
  NoPreference,
  PreferHardware,
  PreferSoftware,
}

pub struct VideoConfig {
  pub codec: Codec,
  pub description: Vec<u8>,
  pub width: u32,
  pub height: u32,
  pub acceleration: Acceleration,
}

pub struct VideoBackend {
  inner: VideoInner,
  /// Empty `description`: samples are Annex B until rewritten.
  annex_b: bool,
  width: u32,
  height: u32,
}

// The Linux VA-API session owns the AV1 OBU parser. It stays unboxed on the
// decode path.
#[allow(clippy::large_enum_variant)]
enum VideoInner {
  #[cfg(target_os = "macos")]
  Hardware(crate::platform::Session),
  #[cfg(target_os = "windows")]
  Hardware(crate::platform::Session),
  #[cfg(target_os = "linux")]
  Hardware(crate::platform::Session),
  /// Annex B, waiting for an access unit that contains SPS and PPS.
  Deferred,
  Av1(yuzuko_image::av1::Av1Session),
  #[cfg(not(any(
    target_os = "macos",
    target_os = "windows",
    target_os = "linux"
  )))]
  Missing,
}

impl VideoBackend {
  pub fn open(config: VideoConfig) -> Result<Self, DecodeError> {
    if config.codec == Codec::Av1 {
      return open_av1(config);
    }
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
    if let Some(unit) = &converted
      && matches!(self.inner, VideoInner::Deferred)
    {
      let avcc = unit.avcc.as_deref().ok_or(DecodeError::Rejected)?;
      self.inner = hardware_session(avcc, self.width, self.height)?;
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
      VideoInner::Av1(session) => session.send(sample).map_err(map_av1),
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
      VideoInner::Av1(session) => session.flush().map_err(map_av1),
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

fn open_av1(config: VideoConfig) -> Result<VideoBackend, DecodeError> {
  if !crate::av1::description_supported(
    (!config.description.is_empty()).then_some(config.description.as_slice()),
  ) {
    return Err(DecodeError::Unsupported);
  }
  if config.acceleration != Acceleration::PreferSoftware
    && let Some(inner) = hardware_av1(&config)
  {
    return Ok(VideoBackend {
      inner,
      annex_b: false,
      width: config.width,
      height: config.height,
    });
  }
  let mut session = yuzuko_image::av1::Av1Session::open().map_err(map_av1)?;
  if let Some(obus) = crate::av1::config_obus(&config.description)
    && !obus.is_empty()
  {
    session.send(obus).map_err(map_av1)?;
  }
  Ok(VideoBackend {
    inner: VideoInner::Av1(session),
    annex_b: false,
    width: config.width,
    height: config.height,
  })
}

/// A hardware session, or nothing when this target cannot decode the
/// configuration. A missing decoder selects rav1d.
#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
fn hardware_av1(config: &VideoConfig) -> Option<VideoInner> {
  if !crate::platform::supports_av1() || config.width == 0 || config.height == 0
  {
    return None;
  }
  let obus = crate::av1::config_obus(&config.description)?;
  if obus.is_empty() {
    return None;
  }
  crate::platform::open_av1(&config.description, config.width, config.height)
    .ok()
    .map(VideoInner::Hardware)
}

#[cfg(not(any(
  target_os = "macos",
  target_os = "windows",
  target_os = "linux"
)))]
fn hardware_av1(_config: &VideoConfig) -> Option<VideoInner> {
  None
}

fn map_av1(error: yuzuko_image::av1::Av1Error) -> DecodeError {
  match error {
    yuzuko_image::av1::Av1Error::Unsupported
    | yuzuko_image::av1::Av1Error::TooLarge => DecodeError::Unsupported,
    yuzuko_image::av1::Av1Error::Corrupt => DecodeError::Rejected,
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

#[cfg(all(test, any(target_os = "macos", target_os = "linux")))]
mod tests {
  use super::{Acceleration, Codec, VideoBackend, VideoConfig, VideoInner};
  use crate::mp4::{Sample, parse};
  use yuzuko_image::Raster;

  fn open_track(
    name: &str,
    acceleration: Acceleration,
  ) -> (VideoBackend, Vec<Sample>, bool) {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
      .join("../../tests/fixtures/media")
      .join(name);
    let bytes = std::fs::read(&path).unwrap_or_else(|error| {
      panic!("{}: {error}", path.display());
    });
    let movie =
      parse(&bytes).unwrap_or_else(|error| panic!("{name}: {error:?}"));
    let track = movie.video.into_iter().next().expect("video");
    let description = track.description.clone();
    let width = track.coded_width;
    let height = track.coded_height;
    // Probe before the backend opens its own session. `supports_av1` can
    // be true when this `av1C` is still rejected, and that rejection is
    // the rav1d path.
    let opened = acceleration != Acceleration::PreferSoftware
      && crate::platform::open_av1(&description, width, height).is_ok();
    let backend = VideoBackend::open(VideoConfig {
      codec: Codec::Av1,
      description,
      width,
      height,
      acceleration,
    })
    .expect("configure");
    (backend, track.samples, opened)
  }

  fn assert_red(backend: &mut VideoBackend, samples: &[Sample]) {
    let mut frames = Vec::new();
    for sample in samples {
      frames.extend(backend.decode(&sample.data).expect("decode"));
    }
    frames.extend(backend.flush().expect("flush"));
    let frame = frames.first().expect("frame");
    let pixel = center(frame);
    assert!(
      pixel[0] > 180 && pixel[1] < 80 && pixel[2] < 80 && pixel[3] == 255,
      "{pixel:?} {}x{}",
      frame.width,
      frame.height
    );
  }

  fn center(frame: &Raster) -> [u8; 4] {
    let x = (frame.width / 2) as usize;
    let y = (frame.height / 2) as usize;
    let index = (y * frame.width as usize + x) * 4;
    frame.bytes[index..index + 4].try_into().expect("pixel")
  }

  fn assert_selected(backend: &VideoBackend, hardware: bool) {
    if hardware {
      assert!(matches!(backend.inner, VideoInner::Hardware(_)));
    } else {
      assert!(matches!(backend.inner, VideoInner::Av1(_)));
    }
  }

  #[test]
  fn av1_uses_hardware_when_the_session_opens() {
    let (mut software, samples, _) =
      open_track("red-av1-256.mp4", Acceleration::PreferSoftware);
    assert!(matches!(software.inner, VideoInner::Av1(_)));
    assert_red(&mut software, &samples);

    let (mut hardware, samples, opened) =
      open_track("red-av1-256.mp4", Acceleration::PreferHardware);
    let (omitted, _, omitted_opened) =
      open_track("red-av1-256.mp4", Acceleration::NoPreference);
    assert_eq!(opened, omitted_opened);
    assert_selected(&hardware, opened);
    assert_selected(&omitted, opened);
    assert_red(&mut hardware, &samples);

    // VideoToolbox rejects this 16×16 session on an M3. macOS 15 can
    // report an `av01` hardware decoder and still reject the 256×256
    // session. A VA-API driver may accept 16×16. The opened session is
    // the hardware path; a rejected one stays on rav1d.
    let (mut small, samples, opened) =
      open_track("red-av1.mp4", Acceleration::PreferHardware);
    assert_selected(&small, opened);
    assert_red(&mut small, &samples);
  }
}
