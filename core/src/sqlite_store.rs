//! Native SQLite facade for local journals, encrypted sync, enrollment, and
//! recovery copies. Modules share one connection and retain atomic writes.

use std::path::Path;

use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};

use crate::{
    batch::{self, Header},
    cbor::{self, Value},
    hlc::{self, Clock},
    ids,
    operation::{self, Hlc, NewOperation, Operation},
    projection::{self, LocalError, LocalProjection, Projection},
};

#[derive(Debug)]
pub enum Error {
    Sqlite(rusqlite::Error),
    Operation(operation::Error),
    Clock(hlc::Error),
    Projection(LocalError),
    SharedProjection(projection::Error),
    InvalidId,
    WrongFamily,
    WrongDevice,
    MissingFamily,
    DuplicateOperation,
    AppendIndexOverflow,
    CorruptState,
    NoUnsentOperation,
    RemovedDevice,
    Random(getrandom::Error),
    Batch(batch::Error),
}

impl From<rusqlite::Error> for Error {
    fn from(error: rusqlite::Error) -> Self {
        Self::Sqlite(error)
    }
}

impl From<operation::Error> for Error {
    fn from(error: operation::Error) -> Self {
        Self::Operation(error)
    }
}

impl From<hlc::Error> for Error {
    fn from(error: hlc::Error) -> Self {
        Self::Clock(error)
    }
}

impl From<LocalError> for Error {
    fn from(error: LocalError) -> Self {
        Self::Projection(error)
    }
}
impl From<projection::Error> for Error {
    fn from(error: projection::Error) -> Self {
        Self::SharedProjection(error)
    }
}

impl From<batch::Error> for Error {
    fn from(error: batch::Error) -> Self {
        Self::Batch(error)
    }
}

/// A Family-specific capability; every read and write checks its device row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FamilyHandle {
    pub family_id: [u8; 16],
    pub device_id: [u8; 16],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RestoredOrigin {
    pub source_family_id: [u8; 16],
    pub snapshot_utc_ms: i64,
    pub source_cursor: Option<u64>,
    pub known_gap: bool,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum CopySource {
    Manual(FamilyHandle),
    Removal(FamilyHandle, [u8; 16]),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedRemoval {
    pub transition_id: [u8; 16],
    pub cursor: u64,
    pub source_cursor: u64,
    pub known_gap: bool,
    pub committed_bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppendedOperation {
    pub index: u64,
    pub bytes: Vec<u8>,
    pub clock_anomaly: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedBatch {
    pub from_index: u64,
    pub to_index: u64,
    pub sequence: u64,
    pub batch_id: [u8; 16],
    pub envelope_bytes: Vec<u8>,
    pub object_hash: [u8; 32],
}

pub(crate) struct AcceptedLocalBatch {
    pub envelope_bytes: Vec<u8>,
    pub receipt_bytes: Vec<u8>,
}

pub(crate) struct SharedHistoryRow {
    pub cursor: u64,
    pub kind: u8,
    pub committed_bytes: Vec<u8>,
    pub receipt_bytes: Vec<u8>,
}

pub(crate) struct SharedHistory {
    pub relay_public_key: [u8; 32],
    pub genesis_bytes: Vec<u8>,
    pub pinned_cursor: u64,
    pub pinned_head: [u8; 32],
    pub entries: Vec<SharedHistoryRow>,
}

pub(crate) struct VerifiedSharedEntry<'a> {
    pub previous_cursor: u64,
    pub previous_head: [u8; 32],
    pub next_head: [u8; 32],
    pub kind: u8,
    pub committed_bytes: &'a [u8],
    pub receipt_bytes: &'a [u8],
    pub own_sequence: Option<u64>,
    pub matching_pending: Option<&'a PreparedBatch>,
}

pub(crate) type SharedObjects = Vec<([u8; 16], Vec<u8>)>;

pub(crate) struct EnrollmentRow {
    pub family: FamilyHandle,
    pub invitation_id: [u8; 16],
    pub genesis_bytes: Vec<u8>,
    pub issue_bytes: Vec<u8>,
    pub candidate_bytes: Vec<u8>,
    pub secret_nonce: [u8; 24],
    pub secret_ciphertext: Vec<u8>,
}

pub(crate) struct ManagerCreationRow {
    pub family: FamilyHandle,
    pub relay_public_key: [u8; 32],
    pub promotion_id: [u8; 16],
    pub transition_id: [u8; 16],
    pub object_id: [u8; 16],
    pub object_bytes: Vec<u8>,
    pub candidate_bytes: Vec<u8>,
    pub secret_nonce: [u8; 24],
    pub secret_ciphertext: Vec<u8>,
    pub chunks: Vec<PromotionChunkRow>,
}

#[derive(Clone)]
pub(crate) struct PromotionChunkRow {
    pub index: u32,
    pub object_id: [u8; 16],
    pub first_local_index: u64,
    pub last_local_index: u64,
    pub object_bytes: Vec<u8>,
}

pub(crate) struct InviteIssueRow {
    pub family: FamilyHandle,
    pub invitation_id: [u8; 16],
    pub transition_id: [u8; 16],
    pub object_id: [u8; 16],
    pub object_bytes: Vec<u8>,
    pub candidate_bytes: Vec<u8>,
    pub secret_nonce: [u8; 24],
    pub secret_ciphertext: Vec<u8>,
}

pub(crate) struct PreparedControlRow {
    pub family: FamilyHandle,
    pub kind: u8,
    pub transition_id: [u8; 16],
    pub candidate_bytes: Vec<u8>,
    pub objects_bytes: Vec<u8>,
    pub secret_nonce: [u8; 24],
    pub secret_ciphertext: Vec<u8>,
}

pub struct SqliteStore {
    connection: Connection,
}

mod authority;
mod copies;
mod enrollment;
mod families;
mod history;
mod journal;
mod outbox;
mod schema;

use journal::checked_family;
use outbox::load_pending;

#[cfg(test)]
#[path = "sqlite_store/enrollment_atomicity_tests.rs"]
mod enrollment_atomicity_tests;

#[cfg(test)]
#[path = "sqlite_store/invite_migration_tests.rs"]
mod invite_migration_tests;

#[cfg(test)]
#[path = "sqlite_store/outbox_tests.rs"]
mod outbox_tests;
