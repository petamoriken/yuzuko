//! Failures from a codec backend.

#[derive(Debug)]
pub enum DecodeError {
  /// The codec is not supported on this platform.
  Unsupported,
  /// The platform decoder rejected the configuration or the frame.
  Rejected,
  /// VideoToolbox rejected the configuration or a frame. The value is the
  /// OSStatus. Other platforms do not construct this variant.
  Hardware(i32),
  /// A decoder panicked on the bytes.
  Panic,
}
