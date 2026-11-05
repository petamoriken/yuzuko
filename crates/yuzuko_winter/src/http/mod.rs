//! Fetch, WebSocket, and WebTransport.
//!
//! `fetch`, `XMLHttpRequest`, `caches`, and `EventSource` belong to the
//! `fetch` feature. WebSocket and WebTransport belong to the `socket`
//! feature. `FormData` is part of the JavaScript runtime.

#[cfg(any(feature = "fetch", feature = "socket"))]
use std::sync::Once;

#[cfg(feature = "fetch")]
mod alt_svc;
#[cfg(feature = "fetch")]
mod client;
#[cfg(feature = "fetch")]
mod content_coding;
#[cfg(feature = "fetch")]
mod request;

#[cfg(feature = "fetch")]
pub(crate) mod cache;
#[cfg(feature = "fetch")]
pub(crate) mod fetch;
pub(crate) mod form;
#[cfg(feature = "fetch")]
pub(crate) mod sse;
#[cfg(feature = "socket")]
pub(crate) mod websocket;
#[cfg(feature = "socket")]
pub(crate) mod webtransport;
#[cfg(feature = "fetch")]
pub(crate) mod xhr;

#[cfg(all(test, feature = "fetch"))]
mod protocol;

#[cfg(feature = "fetch")]
pub use client::{
  BodyNote, HttpBody, HttpFailure, HttpResponse, perform, read_get,
};

#[cfg(all(test, feature = "fetch"))]
pub(crate) use client::perform_with_roots;

#[cfg(any(feature = "fetch", feature = "socket"))]
fn install_tls() {
  static ONCE: Once = Once::new();
  ONCE.call_once(|| {
    let _ = rustls::crypto::ring::default_provider().install_default();
  });
}
