//! Browser binding over the shared Rust verification and projection path.

#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet};

use babytrack_core::projection::{Outcome, Projection, VerifiedEpochKey};
use babytrack_core::{
    analysis_csv, batch,
    bootstrap::InvitationBootstrap,
    cbor::{self, Value},
    claim,
    control_chain::ControlChain,
    crypto, day_summary, portable_file,
    projection::LocalProjection,
    proof, ready_replay, removal_probe, session,
    sync_wire::{self, LogPage},
    web_actions,
};
use babytrack_core::{
    hlc::Clock,
    operation::{Hlc, Operation},
};
use wasm_bindgen::prelude::*;

mod authority;
#[cfg(feature = "fixture-api")]
mod fixtures;
mod initial;
mod invitation;
mod local;
mod removal;
mod restore;

pub use authority::{
    WasmLogPage, WasmPublicFamily, accepted_batch_receipt, manifest_object_ids,
    verified_manifest_object,
};
#[cfg(feature = "fixture-api")]
pub use fixtures::{WasmFamily, ed25519_public_key, seal_one};
pub use initial::WasmInitialFamily;
pub use invitation::WasmInvitation;
pub use local::WasmLocalFamily;
pub use removal::WasmRemovalProbe;
pub use restore::WasmReadableRestore;

#[wasm_bindgen]
pub fn operation_clock(
    operation: &[u8],
    family_id: &[u8],
    device_id: &[u8],
) -> Result<Vec<i64>, JsError> {
    let decoded = Operation::decode_bound(
        operation,
        &fixed(family_id, "Family ID")?,
        &fixed(device_id, "device ID")?,
    )
    .map_err(debug_error)?;
    Ok(vec![decoded.hlc.wall_ms, i64::from(decoded.hlc.counter)])
}

fn random_v7(now_ms: i64) -> Result<[u8; 16], JsError> {
    if !(0..(1i64 << 48)).contains(&now_ms) {
        return Err(JsError::new("time outside UUIDv7 range"));
    }
    let mut id = [0u8; 16];
    getrandom::fill(&mut id).map_err(debug_error)?;
    id[..6].copy_from_slice(&(now_ms as u64).to_be_bytes()[2..]);
    id[6] = (id[6] & 0x0f) | 0x70;
    id[8] = (id[8] & 0x3f) | 0x80;
    Ok(id)
}

#[wasm_bindgen]
pub fn new_local_ids() -> Result<Vec<u8>, JsError> {
    let mut ids = [0u8; 32];
    getrandom::fill(&mut ids).map_err(debug_error)?;
    for offset in [0usize, 16] {
        ids[offset + 6] = (ids[offset + 6] & 0x0f) | 0x40;
        ids[offset + 8] = (ids[offset + 8] & 0x3f) | 0x80;
    }
    Ok(ids.to_vec())
}

fn fixed<const N: usize>(bytes: &[u8], label: &str) -> Result<[u8; N], JsError> {
    bytes
        .try_into()
        .map_err(|_| JsError::new(&format!("{label} must be {N} bytes")))
}

fn debug_error(error: impl std::fmt::Debug) -> JsError {
    JsError::new(&format!("{error:?}"))
}
