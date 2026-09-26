//! Shared data and protocol behavior used by every client.

#![forbid(unsafe_code)]

pub mod batch;
pub mod cbor;
pub mod control;
pub mod crypto;
pub mod hlc;
pub mod hpke;
pub mod operation;
pub mod projection;
#[cfg(not(target_arch = "wasm32"))]
pub mod sqlite_store;

mod ids;
mod record_validity;
