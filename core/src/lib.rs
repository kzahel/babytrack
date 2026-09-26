//! Shared data and protocol behavior used by every client.

#![forbid(unsafe_code)]

pub mod batch;
pub mod bootstrap;
pub use babytrack_wire::cbor;
pub mod control;
#[cfg(not(target_arch = "wasm32"))]
mod control_build;
pub mod control_chain;
#[cfg(not(target_arch = "wasm32"))]
pub mod creation;
pub use babytrack_wire::crypto;
#[cfg(not(target_arch = "wasm32"))]
pub mod enrollment;
#[cfg(not(target_arch = "wasm32"))]
pub mod first_admission;
#[cfg(not(target_arch = "wasm32"))]
pub mod first_challenge;
#[cfg(not(target_arch = "wasm32"))]
pub mod first_proof;
pub mod grant;
pub mod handoff;
pub mod hlc;
pub mod hpke;
#[cfg(not(target_arch = "wasm32"))]
pub mod issue;
pub mod membership;
pub mod operation;
pub mod portable_file;
pub mod projection;
pub mod rotation;
pub mod session;
#[cfg(not(target_arch = "wasm32"))]
pub mod shared_history;
#[cfg(not(target_arch = "wasm32"))]
pub mod shared_ready;
#[cfg(not(target_arch = "wasm32"))]
pub mod sqlite_store;
pub mod sync_wire;

mod ids;
mod record_validity;
