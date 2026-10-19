//! Default output buffer size. No playback stream is opened.

/// Current output callback size in frames, when the OS reports one.
pub fn output_buffer_frames() -> Option<u32> {
  platform::output_buffer_frames()
}

#[cfg(target_os = "macos")]
mod core_audio;
#[cfg(target_os = "macos")]
use self::core_audio as platform;

#[cfg(not(target_os = "macos"))]
mod fallback;
#[cfg(not(target_os = "macos"))]
use self::fallback as platform;

#[cfg(test)]
mod tests {
  use super::output_buffer_frames;

  #[test]
  fn output_buffer_query_matches_the_platform() {
    let frames = output_buffer_frames();
    if cfg!(target_os = "macos") {
      let frames = frames.expect("Core Audio default output buffer");
      assert!(frames >= 1, "{frames}");
    } else {
      assert!(frames.is_none());
    }
  }
}
