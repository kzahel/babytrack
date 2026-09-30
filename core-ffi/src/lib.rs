//! Native Swift/Kotlin binding over the shared Rust replay path.

#![forbid(unsafe_code)]

use std::sync::{Arc, Mutex};

use babytrack_core::{
    active_pull,
    bootstrap::InvitationBootstrap,
    cbor::{self, Value},
    creation::{ManagerCreation, verified_join_target},
    enrollment::EnrollmentAttempt,
    first_admission::FirstAdmission,
    first_challenge::FirstChallenge,
    first_proof::FirstProof,
    first_removal::FirstRemoval,
    invite_cancel::InviteCancellation,
    issue::{FirstInviteIssue, LaterInviteIssue},
    local_api::{self, ActivityTime, LocalRepository},
    operation,
    pending_remove::PendingRemoval,
    role_change::RoleChange,
    shared_history::{PendingBatchResult, PublicHistorySession},
    shared_ready::ReadyFamilySession,
    sqlite_store::{FamilyHandle, SqliteStore},
    sync_wire::{ControlPage, OpaqueObject},
};

#[cfg(feature = "fixture-api")]
use babytrack_core::{
    batch, crypto,
    projection::{Outcome, Projection},
};

uniffi::setup_scaffolding!();

#[cfg(feature = "fixture-api")]
mod fixtures;
mod invitations;
mod local;
mod records;
mod shared;

#[cfg(feature = "fixture-api")]
pub use fixtures::*;
pub use invitations::*;
pub use local::NativeLocalStore;
pub use records::*;
pub use shared::NativeSharedStore;

fn fixed<const N: usize>(bytes: &[u8]) -> Result<[u8; N], BindingError> {
    bytes.try_into().map_err(|_| BindingError::InvalidBytes)
}

fn lower_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn committed_control(response: &[u8]) -> Result<Vec<u8>, BindingError> {
    let Value::Map(fields) = cbor::decode_with_limits(
        response,
        cbor::Limits {
            max_bytes: 1024 * 1024 + 128,
            max_depth: 16,
        },
    )
    .map_err(rejected)?
    else {
        return Err(BindingError::InvalidBytes);
    };
    if fields.len() != 2 || fields[0] != (1, Value::Integer(1)) || fields[1].0 != 2 {
        return Err(BindingError::InvalidBytes);
    }
    let Value::Bytes(committed) = &fields[1].1 else {
        return Err(BindingError::InvalidBytes);
    };
    Ok(committed.clone())
}

fn rejected(error: impl std::fmt::Debug) -> BindingError {
    BindingError::Rejected(format!("{error:?}"))
}

#[uniffi::export]
pub fn validate_relay_origin(origin: String) -> Result<(), BindingError> {
    babytrack_core::bootstrap::validate_relay_origin(&origin).map_err(rejected)
}
