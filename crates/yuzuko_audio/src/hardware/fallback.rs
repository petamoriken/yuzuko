//! Platforms that do not report an output buffer size.

pub fn output_buffer_frames() -> Option<u32> {
  None
}
