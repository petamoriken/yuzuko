//! Bytes for an encoded chunk or `AudioData`.
//!
//! When `init.transfer` contains the `ArrayBuffer` that `data` views, the
//! value keeps that V8 backing store. JavaScript is detached from it before
//! the constructor returns. Every other source is an owned copy, because the
//! caller can still write the buffer. A `SharedArrayBuffer` is always copied.

use std::ops::Deref;

/// A byte range. `Shared` borrows a V8 allocation. `Owned` is a copy.
#[derive(Clone)]
pub(crate) enum ByteStore {
  Owned(Vec<u8>),
  Shared {
    store: v8::SharedRef<v8::BackingStore>,
    offset: usize,
    len: usize,
  },
}

impl std::fmt::Debug for ByteStore {
  fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      Self::Owned(bytes) => formatter
        .debug_struct("Owned")
        .field("len", &bytes.len())
        .finish(),
      Self::Shared { offset, len, .. } => formatter
        .debug_struct("Shared")
        .field("offset", offset)
        .field("len", len)
        .finish(),
    }
  }
}

impl ByteStore {
  pub(crate) fn owned(bytes: Vec<u8>) -> Self {
    Self::Owned(bytes)
  }

  /// `None` when `store` is shared memory or the range does not fit.
  /// A shared store stays writable from JavaScript, so the caller copies it.
  pub(crate) fn shared(
    store: v8::SharedRef<v8::BackingStore>,
    offset: usize,
    len: usize,
  ) -> Option<Self> {
    if store.is_shared() {
      return None;
    }
    let end = offset.checked_add(len)?;
    if len > 0 && (store.data().is_none() || end > store.byte_length()) {
      return None;
    }
    if len == 0 && offset > store.byte_length() {
      return None;
    }
    Some(Self::Shared { store, offset, len })
  }

  pub(crate) fn len(&self) -> usize {
    match self {
      Self::Owned(bytes) => bytes.len(),
      Self::Shared { len, .. } => *len,
    }
  }

  /// The first `len` bytes. An owned buffer is truncated in place. A shared
  /// buffer keeps the same allocation and records the shorter length.
  pub(crate) fn prefix(self, len: usize) -> Option<Self> {
    if len > self.len() {
      return None;
    }
    match self {
      Self::Owned(mut bytes) => {
        bytes.truncate(len);
        Some(Self::Owned(bytes))
      }
      Self::Shared { store, offset, .. } => {
        Some(Self::Shared { store, offset, len })
      }
    }
  }

  pub(crate) fn as_slice(&self) -> &[u8] {
    match self {
      Self::Owned(bytes) => bytes,
      Self::Shared { store, offset, len } => shared_slice(store, *offset, *len),
    }
  }
}

impl Deref for ByteStore {
  type Target = [u8];

  fn deref(&self) -> &Self::Target {
    self.as_slice()
  }
}

fn shared_slice(store: &v8::BackingStore, offset: usize, len: usize) -> &[u8] {
  if len == 0 {
    return &[];
  }
  let Some(data) = store.data() else {
    return &[];
  };
  let Some(end) = offset.checked_add(len) else {
    return &[];
  };
  if end > store.byte_length() {
    return &[];
  }
  // The store owns this allocation. Detach has dropped JavaScript's access,
  // and this shared reference keeps the bytes alive.
  unsafe {
    std::slice::from_raw_parts(data.as_ptr().cast::<u8>().add(offset), len)
  }
}
