//! Hardware H.264 decoders. Each target has its own module.

#[cfg(any(test, target_os = "linux"))]
mod av1_param;
#[cfg(target_os = "windows")]
mod mf;
#[cfg(target_os = "linux")]
mod va;
#[cfg(any(test, target_os = "linux"))]
mod va_types;
#[cfg(target_os = "macos")]
mod vt;

#[cfg(target_os = "windows")]
pub(crate) use mf::{Session, open_av1, open_avc, supports_av1, supports_avc};
#[cfg(target_os = "linux")]
pub(crate) use va::{Session, open_av1, open_avc, supports_av1, supports_avc};
#[cfg(target_os = "macos")]
pub(crate) use vt::{Session, open_av1, open_avc, supports_av1, supports_avc};
