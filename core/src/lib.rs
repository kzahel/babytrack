//! Shared data and protocol behavior used by every client.

#![forbid(unsafe_code)]

#[cfg(not(target_arch = "wasm32"))]
pub mod active_pull;
#[cfg(not(target_arch = "wasm32"))]
pub mod analysis_csv;
pub mod batch;
pub mod bootstrap;
mod breast;
pub mod claim;
pub mod event_actions;
pub mod read_model;
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
#[cfg(not(target_arch = "wasm32"))]
pub mod first_removal;
pub mod grant;
pub mod handoff;
pub mod hlc;
pub mod hpke;
#[cfg(not(target_arch = "wasm32"))]
pub mod invite_cancel;
#[cfg(not(target_arch = "wasm32"))]
pub mod issue;
#[cfg(not(target_arch = "wasm32"))]
pub mod local_api;
pub mod membership;
pub mod operation;
#[cfg(not(target_arch = "wasm32"))]
pub mod pending_remove;
pub mod portable_file;
pub mod projection;
pub mod proof;
pub mod ready_replay;
pub mod removal_probe;
#[cfg(not(target_arch = "wasm32"))]
pub mod role_change;
pub mod rotation;
#[cfg(not(target_arch = "wasm32"))]
pub mod rotation_build;
pub mod session;
#[cfg(not(target_arch = "wasm32"))]
pub mod shared_history;
#[cfg(not(target_arch = "wasm32"))]
pub mod shared_ready;
#[cfg(not(target_arch = "wasm32"))]
pub mod sqlite_store;
pub mod sync_wire;
pub mod web_actions;

mod ids;
mod record_validity;
