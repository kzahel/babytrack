//! Shared data and protocol behavior used by every client.

#![forbid(unsafe_code)]

pub mod batch;
pub mod cbor;
pub mod crypto;
pub mod hpke;
pub mod operation;
pub mod projection;

mod record_validity;
