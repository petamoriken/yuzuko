//! `EventSource`, `WebSocket`, `WebSocketStream`, and `WebTransport`.
//!
//! `EventSource` is installed with the `fetch` feature. `WebSocket`,
//! `WebSocketStream`, and `WebTransport` are installed with the `socket`
//! feature.

#[cfg(feature = "fetch")]
pub(crate) mod event_source;
#[cfg(feature = "socket")]
pub(crate) mod websocket;
#[cfg(feature = "socket")]
pub(crate) mod websocket_stream;
#[cfg(feature = "socket")]
pub(crate) mod webtransport;
