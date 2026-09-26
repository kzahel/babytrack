//! Shared data and protocol behavior used by every client.

#![forbid(unsafe_code)]

pub mod batch;
pub mod cbor;
pub mod crypto;
pub mod hlc;
pub mod hpke;
pub mod operation;
pub mod projection;

mod ids;
mod record_validity;
