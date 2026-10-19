//! Failures from a codec backend.

#[derive(Debug)]
pub enum DecodeError {
  /// The codec is not supported on this platform.
  Unsupported,
  /// The platform decoder rejected the configuration or the frame.
  Rejected,
  /// A decoder panicked on the bytes.
  Panic,
}
