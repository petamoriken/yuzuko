//! URL, encoding, base64, `Blob`, and `File`.

mod blob;
mod bytes;
mod crypto;
mod http;
mod stream;
mod text;
mod url;

use std::sync::Once;

pub use blob::{
  Blob, BlobUrlLease, binary_string, blob_parts, data_url, decode_blob_text,
  lease_blob_url, new_blob, snapshot_blob,
};
pub use bytes::{array_buffer, buffer_source};
pub use http::sse::{SseError, SseItem, SseParser};
pub use http::websocket::{
  AddressSpace, Command, Connection, Note, NoteQueue, PullGate, abort_all,
  connect, environment_origin, is_bad_port, valid_protocol,
};
pub use http::webtransport::{
  Command as WebTransportCommand, Congestion as WebTransportCongestion,
  Connection as WebTransportConnection, Note as WebTransportNote,
  NoteQueue as WebTransportNotes, Options as WebTransportOptions,
  abort_all as abort_web_transports, connect as connect_web_transport,
  next_stream_id as next_web_transport_stream_id,
};
pub use http::{BodyNote, HttpBody, perform};
pub use text::mime::is_mime_type;

pub fn register() {
  static ONCE: Once = Once::new();
  ONCE.call_once(|| {
    yuzuko_core::add_extension(&install);
    yuzuko_core::on_realm_drop(blob::clear_jobs);
    yuzuko_core::on_realm_drop(http::cache::clear);
    yuzuko_core::on_realm_drop(stream::clear);
    blob::register_clone();
    crypto::register_clone();
  });
}

fn install(scope: &mut v8::PinScope) {
  text::base64::install(scope);
  text::encoding::install(scope);
  crypto::install(scope);
  blob::install(scope);
  url::install(scope);
  http::fetch::install(scope);
  http::cache::install(scope);
  http::xhr::XMLHttpRequest::install(scope);
  http::form::install(scope);
  stream::install(scope);
  url::pattern::install(scope);
}
