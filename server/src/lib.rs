//! Opaque relay protocol. Client storage and plaintext operations are absent.

#![forbid(unsafe_code)]

mod authority;
mod batch_authority;
mod http;
mod public_ledger;
pub use http::serve;
mod read_auth;
mod receipt;
mod store;

/// Explicit one-time trust baseline for development relays created before
/// private integrity checkpoints existed. Normal startup never rebaselines.
pub fn migrate_legacy_private_checkpoint(
    db_path: impl AsRef<std::path::Path>,
    relay_seed: [u8; 32],
) -> Result<(), String> {
    store::RelayStore::migrate_legacy_private_checkpoint(db_path, relay_seed)
        .map_err(|error| format!("{error:?}"))
}

pub use receipt::{RelayEntry, encode_control_page};
#[cfg(feature = "test-harness")]
pub use store::RelayStore;
#[cfg(feature = "test-harness")]
pub fn test_router(
    db_path: impl AsRef<std::path::Path>,
    relay_seed: [u8; 32],
) -> Result<axum::Router, String> {
    http::router(db_path, relay_seed).map_err(|error| format!("{error:?}"))
}
