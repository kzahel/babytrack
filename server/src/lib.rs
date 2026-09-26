//! Opaque relay protocol. Client storage and plaintext operations are absent.

#![forbid(unsafe_code)]

mod authority;
mod http;
pub use http::serve;
mod read_auth;
mod receipt;
mod store;

pub use receipt::{RelayEntry, encode_control_page};
#[cfg(feature = "test-harness")]
pub use store::RelayStore;
