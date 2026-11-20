//! Bytes for an encoded chunk or `AudioData`.
//!
//! When `init.transfer` contains the `ArrayBuffer` that `data` views, the
//! value keeps that V8 backing store. JavaScript is detached from it before
//! the constructor returns. Every other source is an owned copy, because the
//! caller can still write the buffer. A `SharedArrayBuffer` is always copied.
//!
//! `Owned` is an `Arc<Vec<u8>>`. A later clone of the store shares that
//! allocation. A unique buffer can drop a tail without copying. `AudioData.clone`
//! and a structured clone that is not a transfer copy the bytes instead. A
//! transfer shares them.

use std::ops::Deref;
use std::sync::Arc;

/// A byte range. `Shared` borrows a V8 allocation. `Owned` is an `Arc` of a
/// `Vec`, so a unique prefix can truncate that `Vec` without copying.
#[derive(Clone)]
pub(crate) enum ByteStore {
  Owned(Arc<Vec<u8>>),
  Shared {
    store: v8::SharedRef<v8::BackingStore>,
    offset: usize,
    len: usize,
  },
}

/// A shareable sample allocation. `Shared` can move to another agent.
pub(crate) enum ByteShare {
  Owned(Arc<Vec<u8>>),
  Shared {
    store: yuzuko_core::SharedStore,
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
    Self::Owned(Arc::new(bytes))
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

  /// The first `len` bytes. A unique owned buffer is truncated in place. A
  /// shared buffer keeps the same allocation and records the shorter length.
  pub(crate) fn prefix(self, len: usize) -> Option<Self> {
    if len > self.len() {
      return None;
    }
    match self {
      Self::Owned(bytes) => {
        if len == bytes.len() {
          return Some(Self::Owned(bytes));
        }
        let bytes = match Arc::try_unwrap(bytes) {
          Ok(mut vec) => {
            vec.truncate(len);
            Arc::new(vec)
          }
          Err(bytes) => Arc::new(bytes[..len].to_vec()),
        };
        Some(Self::Owned(bytes))
      }
      Self::Shared { store, offset, .. } => {
        Some(Self::Shared { store, offset, len })
      }
    }
  }

  /// A new allocation with the same bytes. `AudioData.clone` and a
  /// structured clone that leaves the source open use this.
  pub(crate) fn to_unique(&self) -> Self {
    Self::owned(self.as_slice().to_vec())
  }

  /// Another handle to this allocation. No sample bytes are copied.
  pub(crate) fn share(&self) -> ByteShare {
    match self {
      Self::Owned(bytes) => ByteShare::Owned(Arc::clone(bytes)),
      Self::Shared { store, offset, len } => ByteShare::Shared {
        store: yuzuko_core::SharedStore::new(store.clone()),
        offset: *offset,
        len: *len,
      },
    }
  }

  pub(crate) fn from_share(share: ByteShare) -> Option<Self> {
    match share {
      ByteShare::Owned(bytes) => Some(Self::Owned(bytes)),
      ByteShare::Shared { store, offset, len } => {
        Self::shared(store.backing().clone(), offset, len)
      }
    }
  }

  pub(crate) fn as_slice(&self) -> &[u8] {
    match self {
      Self::Owned(bytes) => bytes.as_slice(),
      Self::Shared { store, offset, len } => shared_slice(store, *offset, *len),
    }
  }
}

impl ByteShare {
  /// Another handle. `Owned` clones the `Arc`. A backing store clones its
  /// `shared_ptr`. Neither copies sample bytes.
  pub(crate) fn share_again(&self) -> Self {
    match self {
      Self::Owned(bytes) => Self::Owned(Arc::clone(bytes)),
      Self::Shared { store, offset, len } => Self::Shared {
        store: yuzuko_core::SharedStore::new(store.backing().clone()),
        offset: *offset,
        len: *len,
      },
    }
  }

  #[cfg(test)]
  pub(crate) fn as_ptr(&self) -> *const u8 {
    match self {
      Self::Owned(bytes) => bytes.as_ptr(),
      Self::Shared { store, offset, .. } => store
        .backing()
        .data()
        .map(|data| data.as_ptr().cast::<u8>().wrapping_add(*offset))
        .unwrap_or(std::ptr::null_mut())
        .cast_const(),
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
