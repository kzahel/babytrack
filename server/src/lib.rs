//! Opaque relay protocol. Client storage and plaintext operations are absent.

#![forbid(unsafe_code)]

mod authority;
mod receipt;
mod store;

pub use receipt::{RelayEntry, encode_control_page};
