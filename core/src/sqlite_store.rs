//! Native local-only journal. Accepted shared entries and outbox persistence
//! extend this schema as the sync client is implemented.

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

#[cfg(test)]
mod enrollment_atomicity_tests {
    use super::*;
    use crate::operation::{Kind, Scope};

    fn v4(tag: u8) -> [u8; 16] {
        let mut id = [tag; 16];
        id[6] = 0x40;
        id[8] = 0x80;
        id
    }

    fn v7(tag: u8) -> [u8; 16] {
        let mut id = [tag; 16];
        id[6] = 0x70;
        id[8] = 0x80;
        id
    }

    #[test]
    fn removal_copy_rolls_back_mapping_and_family_after_late_sqlite_failure() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("copy.db");
        let source = FamilyHandle {
            family_id: v4(0x41),
            device_id: v4(0x42),
        };
        let copy_id = v4(0x43);
        let copy_device = v4(0x44);
        let transition_id = v4(0x45);
        let child_id = v7(0x46);
        let origin = RestoredOrigin {
            source_family_id: source.family_id,
            snapshot_utc_ms: 1_000,
            source_cursor: Some(7),
            known_gap: true,
        };
        let operation = NewOperation {
            family_id: copy_id,
            operation_id: v7(0x47),
            record_id: child_id,
            scope: Scope::Child,
            kind: Kind::Create,
            author_device_id: copy_device,
            hlc: Hlc {
                wall_ms: 0,
                counter: 0,
                device_id: copy_device,
            },
            record_type: Some("child".to_owned()),
            child_id: None,
            fields: Some(vec![(1, Value::Text("Preserved".to_owned()))]),
        };
        let mut store = SqliteStore::open(&path).unwrap();
        store
            .create_family(source.family_id, source.device_id)
            .unwrap();
        // This test isolates the copy transaction after a removal proof has
        // already been verified and saved by the public-history path.
        store.connection.execute(
            "INSERT INTO verified_removals(source_family_id,source_device_id,transition_id,cursor,source_cursor,known_gap,committed_bytes)
             VALUES(?1,?2,?3,7,6,1,?4)",
            params![source.family_id.as_slice(),source.device_id.as_slice(),transition_id.as_slice(),&[0x80u8][..]],
        ).unwrap();
        store
            .connection
            .execute_batch(
                "CREATE TRIGGER fail_copy_op BEFORE INSERT ON local_operations
                 BEGIN SELECT RAISE(ABORT, 'injected failure'); END;",
            )
            .unwrap();
        assert!(
            store
                .restore_family_with_copy_source(
                    copy_id,
                    copy_device,
                    vec![operation.clone()],
                    1_000,
                    origin,
                    Some(CopySource::Removal(source, transition_id)),
                )
                .is_err()
        );
        drop(store);
        let mut store = SqliteStore::open(&path).unwrap();
        assert_eq!(store.families().unwrap(), vec![source]);
        assert!(
            store
                .removal_copy_of(source, transition_id)
                .unwrap()
                .is_none()
        );
        for table in ["restored_origins", "removal_copies", "local_operations"] {
            let count: i64 = store
                .connection
                .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                    row.get(0)
                })
                .unwrap();
            assert_eq!(count, 0, "partial {table} row survived");
        }
        store
            .connection
            .execute_batch("DROP TRIGGER fail_copy_op")
            .unwrap();
        let copy = store
            .restore_family_with_copy_source(
                copy_id,
                copy_device,
                vec![operation],
                1_000,
                origin,
                Some(CopySource::Removal(source, transition_id)),
            )
            .unwrap();
        assert_eq!(
            store.removal_copy_of(source, transition_id).unwrap(),
            Some(copy)
        );
        assert_eq!(store.restored_origin(copy).unwrap(), Some(origin));
        assert_eq!(
            store
                .load_local(copy)
                .unwrap()
                .record(&child_id)
                .unwrap()
                .field(1)
                .unwrap()
                .value,
            Value::Text("Preserved".to_owned())
        );
    }

    #[test]
    fn sparse_attempt_and_shared_root_commit_or_roll_back_together() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("enrollment.db");
        let family = FamilyHandle {
            family_id: v4(0x31),
            device_id: v4(0x32),
        };
        let row = EnrollmentRow {
            family,
            invitation_id: v4(0x33),
            genesis_bytes: vec![0x80],
            issue_bytes: vec![0x81, 0x01],
            candidate_bytes: vec![0x82, 0x01, 0x02],
            secret_nonce: [0x34; 24],
            secret_ciphertext: vec![0x35],
        };
        let mut store = SqliteStore::open(&path).unwrap();
        // Fail the final write inside the transaction. No earlier Family,
        // key, root, or sparse control row may survive the failed attempt.
        store
            .connection
            .execute_batch(
                "CREATE TRIGGER fail_sparse_insert BEFORE INSERT ON enrollment_controls
                 BEGIN SELECT RAISE(ABORT, 'injected failure'); END;",
            )
            .unwrap();
        assert!(
            store
                .create_enrollment_attempt(
                    &row,
                    &[(3, row.issue_bytes.clone())],
                    [0x36; 32],
                    [0x37; 32]
                )
                .is_err()
        );
        drop(store);
        let mut store = SqliteStore::open(&path).unwrap();
        for table in [
            "families",
            "local_sync_state",
            "shared_roots",
            "enrollment_attempts",
            "enrollment_controls",
        ] {
            let count: i64 = store
                .connection
                .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                    row.get(0)
                })
                .unwrap();
            assert_eq!(count, 0, "partial {table} row survived");
        }
        store
            .connection
            .execute_batch("DROP TRIGGER fail_sparse_insert")
            .unwrap();
        store
            .create_enrollment_attempt(
                &row,
                &[(3, row.issue_bytes.clone())],
                [0x36; 32],
                [0x37; 32],
            )
            .unwrap();
        drop(store);
        let store = SqliteStore::open(&path).unwrap();
        assert!(
            store
                .enrollment_attempt(family.family_id)
                .unwrap()
                .is_some()
        );
        let history = store.shared_history(family).unwrap().unwrap();
        assert_eq!(history.genesis_bytes, row.genesis_bytes);
        assert_eq!(history.pinned_cursor, 1);
        assert_eq!(store.enrollment_controls(family).unwrap().len(), 1);
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

impl SqliteStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, Error> {
        let mut connection = Connection::open(path)?;
        connection.busy_timeout(std::time::Duration::from_secs(10))?;
        connection.execute_batch("PRAGMA foreign_keys = ON;")?;
        let observed_version: u32 =
            connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
        if observed_version == 4 {
            return Ok(Self { connection });
        }
        if observed_version > 4 {
            return Err(Error::CorruptState);
        }
        if observed_version == 0 {
            // SQLite may return BUSY immediately while another fresh opener
            // switches the journal mode, even with busy_timeout installed.
            // Journal mode cannot be changed inside the schema transaction.
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
            loop {
                match connection.execute_batch("PRAGMA journal_mode = WAL;") {
                    Ok(()) => break,
                    Err(rusqlite::Error::SqliteFailure(code, _))
                        if matches!(
                            code.code,
                            rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked
                        ) && std::time::Instant::now() < deadline =>
                    {
                        std::thread::sleep(std::time::Duration::from_millis(10));
                    }
                    Err(error) => return Err(error.into()),
                }
            }
        }
        // Recheck under the writer lock: another opener may have completed a
        // migration after our first version read.
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let schema_version: u32 =
            transaction.pragma_query_value(None, "user_version", |row| row.get(0))?;
        if schema_version > 4 {
            return Err(Error::CorruptState);
        }
        if schema_version == 0 {
            transaction.execute_batch(
                "CREATE TABLE IF NOT EXISTS families (
               family_id BLOB PRIMARY KEY CHECK(length(family_id) = 16),
               device_id BLOB NOT NULL CHECK(length(device_id) = 16),
               last_index INTEGER NOT NULL DEFAULT 0 CHECK(last_index >= 0),
               last_hlc_wall INTEGER,
               last_hlc_counter INTEGER,
               CHECK ((last_hlc_wall IS NULL) = (last_hlc_counter IS NULL))
             );
             CREATE TABLE IF NOT EXISTS restored_origins (
               family_id BLOB PRIMARY KEY CHECK(length(family_id) = 16),
               source_family_id BLOB NOT NULL CHECK(length(source_family_id) = 16),
               snapshot_utc_ms INTEGER NOT NULL,
               source_cursor BLOB CHECK(source_cursor IS NULL OR length(source_cursor) = 8),
               known_gap INTEGER NOT NULL CHECK(known_gap IN (0,1)),
               FOREIGN KEY (family_id) REFERENCES families(family_id)
             );
             CREATE TABLE IF NOT EXISTS private_copies (
               source_family_id BLOB NOT NULL CHECK(length(source_family_id)=16),
               source_device_id BLOB NOT NULL CHECK(length(source_device_id)=16),
               copy_family_id BLOB NOT NULL UNIQUE CHECK(length(copy_family_id)=16),
               PRIMARY KEY (source_family_id,source_device_id),
               FOREIGN KEY (source_family_id) REFERENCES families(family_id),
               FOREIGN KEY (copy_family_id) REFERENCES families(family_id)
             );
             CREATE TABLE IF NOT EXISTS verified_removals (
               source_family_id BLOB NOT NULL CHECK(length(source_family_id)=16),
               source_device_id BLOB NOT NULL CHECK(length(source_device_id)=16),
               transition_id BLOB NOT NULL CHECK(length(transition_id)=16),
               cursor INTEGER NOT NULL CHECK(cursor > 0),
               source_cursor INTEGER NOT NULL CHECK(source_cursor > 0),
               known_gap INTEGER NOT NULL CHECK(known_gap IN (0,1)),
               committed_bytes BLOB NOT NULL,
               PRIMARY KEY (source_family_id,source_device_id),
               FOREIGN KEY (source_family_id) REFERENCES families(family_id)
             );
             CREATE TABLE IF NOT EXISTS removal_copies (
               source_family_id BLOB NOT NULL CHECK(length(source_family_id)=16),
               source_device_id BLOB NOT NULL CHECK(length(source_device_id)=16),
               transition_id BLOB NOT NULL CHECK(length(transition_id)=16),
               copy_family_id BLOB NOT NULL UNIQUE CHECK(length(copy_family_id)=16),
               PRIMARY KEY (source_family_id,source_device_id,transition_id),
               FOREIGN KEY (source_family_id,source_device_id)
                 REFERENCES verified_removals(source_family_id,source_device_id),
               FOREIGN KEY (copy_family_id) REFERENCES families(family_id)
             );
             CREATE TABLE IF NOT EXISTS local_operations (
               family_id BLOB NOT NULL,
               append_index INTEGER NOT NULL CHECK(append_index > 0),
               operation_id BLOB NOT NULL CHECK(length(operation_id) = 16),
               operation_bytes BLOB NOT NULL,
               PRIMARY KEY (family_id, append_index),
               UNIQUE (family_id, operation_id),
               FOREIGN KEY (family_id) REFERENCES families(family_id)
             );
             CREATE TABLE IF NOT EXISTS local_sync_state (
               family_id BLOB PRIMARY KEY,
               accepted_index INTEGER NOT NULL DEFAULT 0 CHECK(accepted_index >= 0),
               next_sequence INTEGER NOT NULL DEFAULT 1 CHECK(next_sequence > 0),
               FOREIGN KEY (family_id) REFERENCES families(family_id)
             );
             CREATE TABLE IF NOT EXISTS local_outbox (
               family_id BLOB PRIMARY KEY,
               from_index INTEGER NOT NULL CHECK(from_index > 0),
               to_index INTEGER NOT NULL CHECK(to_index >= from_index),
               sequence INTEGER NOT NULL CHECK(sequence > 0),
               batch_id BLOB NOT NULL CHECK(length(batch_id) = 16),
               envelope_bytes BLOB NOT NULL,
               object_hash BLOB NOT NULL CHECK(length(object_hash) = 32),
               FOREIGN KEY (family_id) REFERENCES families(family_id)
             );
             CREATE TABLE IF NOT EXISTS local_batch_reservations (
               family_id BLOB NOT NULL,
               epoch INTEGER NOT NULL CHECK(epoch > 0),
               nonce BLOB NOT NULL CHECK(length(nonce) = 24),
               batch_id BLOB NOT NULL CHECK(length(batch_id) = 16),
               PRIMARY KEY (family_id, batch_id),
               UNIQUE (family_id, epoch, nonce),
               FOREIGN KEY (family_id) REFERENCES families(family_id)
             );
             CREATE TABLE IF NOT EXISTS rejected_local_batches (
               family_id BLOB NOT NULL,
               batch_id BLOB NOT NULL CHECK(length(batch_id) = 16),
               envelope_bytes BLOB NOT NULL,
               receipt_bytes BLOB NOT NULL,
               rejected_at_cursor INTEGER NOT NULL CHECK(rejected_at_cursor >= 1),
               PRIMARY KEY (family_id, batch_id),
               FOREIGN KEY (family_id) REFERENCES families(family_id)
             );
             CREATE TABLE IF NOT EXISTS accepted_local_batches (
               family_id BLOB NOT NULL,
               cursor INTEGER NOT NULL CHECK(cursor > 1),
               to_index INTEGER NOT NULL CHECK(to_index > 0),
               batch_id BLOB NOT NULL CHECK(length(batch_id) = 16),
               envelope_bytes BLOB NOT NULL,
               receipt_bytes BLOB NOT NULL,
               PRIMARY KEY (family_id, cursor),
               UNIQUE (family_id, batch_id),
               FOREIGN KEY (family_id) REFERENCES families(family_id)
             );
             CREATE TABLE IF NOT EXISTS shared_roots (
               family_id BLOB PRIMARY KEY CHECK(length(family_id) = 16),
               relay_public_key BLOB NOT NULL CHECK(length(relay_public_key) = 32),
               genesis_bytes BLOB NOT NULL,
               pinned_cursor INTEGER NOT NULL CHECK(pinned_cursor >= 1),
               pinned_head BLOB NOT NULL CHECK(length(pinned_head) = 32),
               FOREIGN KEY (family_id) REFERENCES families(family_id)
             );
             CREATE TABLE IF NOT EXISTS shared_entries (
               family_id BLOB NOT NULL,
               cursor INTEGER NOT NULL CHECK(cursor > 1),
               kind INTEGER NOT NULL CHECK(kind IN (1, 2)),
               committed_bytes BLOB NOT NULL,
               receipt_bytes BLOB NOT NULL,
               PRIMARY KEY (family_id, cursor),
               FOREIGN KEY (family_id) REFERENCES shared_roots(family_id)
             );
             CREATE TABLE IF NOT EXISTS shared_objects (
               family_id BLOB NOT NULL,
               object_id BLOB NOT NULL CHECK(length(object_id) = 16),
               transition_id BLOB NOT NULL CHECK(length(transition_id) = 16),
               kind INTEGER NOT NULL CHECK(kind > 0),
               object_bytes BLOB NOT NULL,
               PRIMARY KEY (family_id, object_id),
               FOREIGN KEY (family_id) REFERENCES shared_roots(family_id)
             );
             CREATE TABLE IF NOT EXISTS enrollment_attempts (
               family_id BLOB PRIMARY KEY CHECK(length(family_id) = 16),
               device_id BLOB NOT NULL CHECK(length(device_id) = 16),
               invitation_id BLOB NOT NULL CHECK(length(invitation_id) = 16),
               genesis_bytes BLOB NOT NULL,
               issue_bytes BLOB NOT NULL,
               candidate_bytes BLOB NOT NULL,
               secret_nonce BLOB NOT NULL CHECK(length(secret_nonce) = 24),
               secret_ciphertext BLOB NOT NULL,
               FOREIGN KEY (family_id) REFERENCES families(family_id)
             );
             CREATE TABLE IF NOT EXISTS enrollment_controls (
               family_id BLOB NOT NULL,
               cursor INTEGER NOT NULL CHECK(cursor > 1),
               committed_bytes BLOB NOT NULL,
               PRIMARY KEY (family_id,cursor),
               FOREIGN KEY (family_id) REFERENCES enrollment_attempts(family_id)
             );
             CREATE TABLE IF NOT EXISTS manager_creations (
               family_id BLOB PRIMARY KEY CHECK(length(family_id) = 16),
               device_id BLOB NOT NULL CHECK(length(device_id) = 16),
               relay_public_key BLOB NOT NULL CHECK(length(relay_public_key) = 32),
               promotion_id BLOB NOT NULL CHECK(length(promotion_id) = 16),
               transition_id BLOB NOT NULL CHECK(length(transition_id) = 16),
               object_id BLOB NOT NULL CHECK(length(object_id) = 16),
               object_bytes BLOB NOT NULL,
               candidate_bytes BLOB NOT NULL,
               secret_nonce BLOB NOT NULL CHECK(length(secret_nonce) = 24),
               secret_ciphertext BLOB NOT NULL,
               FOREIGN KEY (family_id) REFERENCES families(family_id)
             );
             CREATE TABLE IF NOT EXISTS manager_promotion_chunks (
               family_id BLOB NOT NULL CHECK(length(family_id) = 16),
               chunk_index INTEGER NOT NULL CHECK(chunk_index >= 0),
               object_id BLOB NOT NULL CHECK(length(object_id) = 16),
               first_local_index INTEGER NOT NULL CHECK(first_local_index > 0),
               last_local_index INTEGER NOT NULL CHECK(last_local_index >= first_local_index),
               object_bytes BLOB NOT NULL,
               PRIMARY KEY (family_id, chunk_index),
               UNIQUE (family_id, object_id),
               FOREIGN KEY (family_id) REFERENCES manager_creations(family_id)
             );
             CREATE TABLE IF NOT EXISTS first_invite_issues (
               family_id BLOB PRIMARY KEY CHECK(length(family_id) = 16),
               device_id BLOB NOT NULL CHECK(length(device_id) = 16),
               invitation_id BLOB NOT NULL CHECK(length(invitation_id) = 16),
               transition_id BLOB NOT NULL CHECK(length(transition_id) = 16),
               object_id BLOB NOT NULL CHECK(length(object_id) = 16),
               object_bytes BLOB NOT NULL,
               candidate_bytes BLOB NOT NULL,
               secret_nonce BLOB NOT NULL CHECK(length(secret_nonce) = 24),
               secret_ciphertext BLOB NOT NULL,
               FOREIGN KEY (family_id) REFERENCES families(family_id)
             );
             CREATE TABLE IF NOT EXISTS prepared_controls (
               family_id BLOB NOT NULL CHECK(length(family_id) = 16),
               kind INTEGER NOT NULL CHECK(kind > 0),
               device_id BLOB NOT NULL CHECK(length(device_id) = 16),
               transition_id BLOB NOT NULL CHECK(length(transition_id) = 16),
               candidate_bytes BLOB NOT NULL,
               objects_bytes BLOB NOT NULL,
               secret_nonce BLOB NOT NULL CHECK(length(secret_nonce) = 24),
               secret_ciphertext BLOB NOT NULL,
               PRIMARY KEY (family_id, kind),
               FOREIGN KEY (family_id) REFERENCES families(family_id)
             );
             INSERT OR IGNORE INTO local_sync_state(family_id)
               SELECT family_id FROM families;
             PRAGMA user_version = 1;",
            )?;
        }
        if schema_version <= 1 {
            migrate_invite_issues(&transaction)?;
        }
        if schema_version <= 2 {
            migrate_claim_candidates(&transaction)?;
        }
        if schema_version <= 3 {
            migrate_enrollment_terminal_status(&transaction)?;
        }
        transaction.commit()?;
        Ok(Self { connection })
    }

    pub fn create_family(
        &mut self,
        family_id: [u8; 16],
        device_id: [u8; 16],
    ) -> Result<FamilyHandle, Error> {
        if !ids::is_v4(&family_id) || !ids::is_v4(&device_id) {
            return Err(Error::InvalidId);
        }
        let transaction = self.connection.transaction()?;
        transaction.execute(
            "INSERT INTO families(family_id, device_id) VALUES (?1, ?2)",
            params![family_id.as_slice(), device_id.as_slice()],
        )?;
        transaction.execute(
            "INSERT INTO local_sync_state(family_id) VALUES (?1)",
            [family_id.as_slice()],
        )?;
        transaction.commit()?;
        Ok(FamilyHandle {
            family_id,
            device_id,
        })
    }

    pub fn create_local_family_with_metadata(
        &mut self,
        family_id: [u8; 16],
        device_id: [u8; 16],
        operation_id: [u8; 16],
        now_ms: i64,
    ) -> Result<FamilyHandle, Error> {
        if !ids::is_v4(&family_id) || !ids::is_v4(&device_id) || !ids::is_v7(&operation_id) {
            return Err(Error::InvalidId);
        }
        let mut clock = Clock::restore(family_id, device_id, None, None)?;
        let stamp = clock.next(now_ms).stamp;
        let new = NewOperation {
            family_id,
            operation_id,
            record_id: family_id,
            scope: operation::Scope::Family,
            kind: operation::Kind::Create,
            author_device_id: device_id,
            hlc: stamp.clone(),
            record_type: Some("family".to_owned()),
            child_id: None,
            fields: Some(vec![]),
        };
        let bytes = Operation::encode_new(&new)?;
        let decoded = Operation::decode_bound(&bytes, &family_id, &device_id)?;
        LocalProjection::new(family_id).append(&decoded, 1)?;
        let transaction = self.connection.transaction()?;
        transaction.execute(
            "INSERT INTO families(family_id,device_id,last_index,last_hlc_wall,last_hlc_counter)
             VALUES(?1,?2,1,?3,?4)",
            params![
                family_id.as_slice(),
                device_id.as_slice(),
                stamp.wall_ms,
                stamp.counter
            ],
        )?;
        transaction.execute(
            "INSERT INTO local_sync_state(family_id) VALUES(?1)",
            [family_id.as_slice()],
        )?;
        transaction.execute(
            "INSERT INTO local_operations(family_id,append_index,operation_id,operation_bytes)
             VALUES(?1,1,?2,?3)",
            params![family_id.as_slice(), operation_id.as_slice(), bytes],
        )?;
        transaction.commit()?;
        Ok(FamilyHandle {
            family_id,
            device_id,
        })
    }

    pub fn families(&self) -> Result<Vec<FamilyHandle>, Error> {
        let mut statement = self
            .connection
            .prepare("SELECT family_id,device_id FROM families ORDER BY family_id")?;
        let rows = statement.query_map([], |row| {
            Ok((row.get::<_, Vec<u8>>(0)?, row.get::<_, Vec<u8>>(1)?))
        })?;
        let mut families = Vec::new();
        for row in rows {
            let (family_id, device_id) = row?;
            let family_id: [u8; 16] = family_id.try_into().map_err(|_| Error::CorruptState)?;
            let device_id: [u8; 16] = device_id.try_into().map_err(|_| Error::CorruptState)?;
            if !ids::is_v4(&family_id) || !ids::is_v4(&device_id) {
                return Err(Error::CorruptState);
            }
            families.push(FamilyHandle {
                family_id,
                device_id,
            });
        }
        Ok(families)
    }

    /// Publish a restored local Family and its current-state operations in
    /// one transaction. A validation error cannot expose a partial Family.
    pub(crate) fn restore_family(
        &mut self,
        family_id: [u8; 16],
        device_id: [u8; 16],
        operations: Vec<NewOperation>,
        now_ms: i64,
        origin: RestoredOrigin,
    ) -> Result<FamilyHandle, Error> {
        self.restore_family_with_copy_source(family_id, device_id, operations, now_ms, origin, None)
    }

    pub(crate) fn restore_family_with_copy_source(
        &mut self,
        family_id: [u8; 16],
        device_id: [u8; 16],
        operations: Vec<NewOperation>,
        now_ms: i64,
        origin: RestoredOrigin,
        copy_source: Option<CopySource>,
    ) -> Result<FamilyHandle, Error> {
        if !ids::is_v4(&family_id) || !ids::is_v4(&device_id) {
            return Err(Error::InvalidId);
        }
        let family = FamilyHandle {
            family_id,
            device_id,
        };
        let transaction = self.connection.transaction()?;
        transaction.execute(
            "INSERT INTO families(family_id,device_id) VALUES(?1,?2)",
            params![family_id.as_slice(), device_id.as_slice()],
        )?;
        transaction.execute(
            "INSERT INTO local_sync_state(family_id) VALUES(?1)",
            [family_id.as_slice()],
        )?;
        transaction.execute(
            "INSERT INTO restored_origins(family_id,source_family_id,snapshot_utc_ms,source_cursor,known_gap)
             VALUES(?1,?2,?3,?4,?5)",
            params![family_id.as_slice(),origin.source_family_id.as_slice(),origin.snapshot_utc_ms,
                origin.source_cursor.map(|value| value.to_be_bytes().to_vec()),
                i64::from(origin.known_gap)],
        )?;
        if let Some(source) = copy_source {
            let source_family = match source {
                CopySource::Manual(value) | CopySource::Removal(value, _) => value,
            };
            if source_family.family_id != origin.source_family_id {
                return Err(Error::WrongFamily);
            }
            checked_family(&transaction, source_family)?;
            match source {
                CopySource::Manual(source) => {
                    transaction.execute(
                        "INSERT INTO private_copies(source_family_id,source_device_id,copy_family_id) VALUES(?1,?2,?3)",
                        params![source.family_id.as_slice(),source.device_id.as_slice(),family_id.as_slice()],
                    )?;
                }
                CopySource::Removal(source, transition_id) => {
                    transaction.execute(
                        "INSERT INTO removal_copies(source_family_id,source_device_id,transition_id,copy_family_id) VALUES(?1,?2,?3,?4)",
                        params![source.family_id.as_slice(),source.device_id.as_slice(),transition_id.as_slice(),family_id.as_slice()],
                    )?;
                }
            }
        }
        let mut projection = LocalProjection::new(family_id);
        let mut clock = Clock::restore(family_id, device_id, None, None)?;
        let mut last_stamp = None;
        for (position, mut new) in operations.into_iter().enumerate() {
            if new.family_id != family_id || new.author_device_id != device_id {
                return Err(Error::WrongFamily);
            }
            let index = i64::try_from(position + 1).map_err(|_| Error::AppendIndexOverflow)?;
            new.hlc = clock.next(now_ms).stamp;
            let bytes = Operation::encode_new(&new)?;
            let decoded = Operation::decode_bound(&bytes, &family_id, &device_id)?;
            projection.append(&decoded, index as u64)?;
            transaction.execute(
                "INSERT INTO local_operations(family_id,append_index,operation_id,operation_bytes)
                 VALUES(?1,?2,?3,?4)",
                params![
                    family_id.as_slice(),
                    index,
                    new.operation_id.as_slice(),
                    &bytes
                ],
            )?;
            last_stamp = Some(new.hlc);
        }
        if let Some(stamp) = last_stamp {
            transaction.execute(
                "UPDATE families SET last_index=?2,last_hlc_wall=?3,last_hlc_counter=?4 WHERE family_id=?1",
                params![family_id.as_slice(),projection.last_append_index(),stamp.wall_ms,stamp.counter],
            )?;
        }
        transaction.commit()?;
        Ok(family)
    }

    pub fn private_copy_of(&self, source: FamilyHandle) -> Result<Option<FamilyHandle>, Error> {
        checked_family(&self.connection, source)?;
        let saved: Option<(Vec<u8>, Vec<u8>)> = self
            .connection
            .query_row(
                "SELECT f.family_id,f.device_id FROM private_copies p
             JOIN families f ON f.family_id=p.copy_family_id
             WHERE p.source_family_id=?1 AND p.source_device_id=?2",
                params![source.family_id.as_slice(), source.device_id.as_slice()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        saved
            .map(|(family_id, device_id)| {
                Ok(FamilyHandle {
                    family_id: family_id.try_into().map_err(|_| Error::CorruptState)?,
                    device_id: device_id.try_into().map_err(|_| Error::CorruptState)?,
                })
            })
            .transpose()
    }

    pub fn saved_removal(&self, source: FamilyHandle) -> Result<Option<SavedRemoval>, Error> {
        checked_family(&self.connection, source)?;
        type SavedRemovalRow = (Vec<u8>, i64, i64, i64, Vec<u8>);
        let saved: Option<SavedRemovalRow> = self
            .connection
            .query_row(
                "SELECT transition_id,cursor,source_cursor,known_gap,committed_bytes
             FROM verified_removals WHERE source_family_id=?1 AND source_device_id=?2",
                params![source.family_id.as_slice(), source.device_id.as_slice()],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                },
            )
            .optional()?;
        saved
            .map(
                |(transition_id, cursor, source_cursor, known_gap, committed_bytes)| {
                    Ok(SavedRemoval {
                        transition_id: transition_id.try_into().map_err(|_| Error::CorruptState)?,
                        cursor: cursor.try_into().map_err(|_| Error::CorruptState)?,
                        source_cursor: source_cursor.try_into().map_err(|_| Error::CorruptState)?,
                        known_gap: match known_gap {
                            0 => false,
                            1 => true,
                            _ => return Err(Error::CorruptState),
                        },
                        committed_bytes,
                    })
                },
            )
            .transpose()
    }

    pub(crate) fn save_verified_removal(
        &mut self,
        source: FamilyHandle,
        removal: &SavedRemoval,
    ) -> Result<(), Error> {
        if let Some(existing) = self.saved_removal(source)? {
            return if existing == *removal {
                Ok(())
            } else {
                Err(Error::CorruptState)
            };
        }
        let history = self.shared_history(source)?.ok_or(Error::CorruptState)?;
        if history.pinned_cursor != removal.source_cursor || removal.cursor <= removal.source_cursor
        {
            return Err(Error::CorruptState);
        }
        self.connection.execute(
            "INSERT INTO verified_removals(source_family_id,source_device_id,transition_id,cursor,source_cursor,known_gap,committed_bytes)
             VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![source.family_id.as_slice(),source.device_id.as_slice(),removal.transition_id.as_slice(),
                i64::try_from(removal.cursor).map_err(|_| Error::CorruptState)?,
                i64::try_from(removal.source_cursor).map_err(|_| Error::CorruptState)?,
                i64::from(removal.known_gap),&removal.committed_bytes],
        )?;
        Ok(())
    }

    pub fn removal_copy_of(
        &self,
        source: FamilyHandle,
        transition_id: [u8; 16],
    ) -> Result<Option<FamilyHandle>, Error> {
        checked_family(&self.connection, source)?;
        let saved: Option<(Vec<u8>,Vec<u8>)> = self.connection.query_row(
            "SELECT f.family_id,f.device_id FROM removal_copies r JOIN families f ON f.family_id=r.copy_family_id
             WHERE r.source_family_id=?1 AND r.source_device_id=?2 AND r.transition_id=?3",
            params![source.family_id.as_slice(),source.device_id.as_slice(),transition_id.as_slice()],
            |row| Ok((row.get(0)?,row.get(1)?)),
        ).optional()?;
        saved
            .map(|(family_id, device_id)| {
                Ok(FamilyHandle {
                    family_id: family_id.try_into().map_err(|_| Error::CorruptState)?,
                    device_id: device_id.try_into().map_err(|_| Error::CorruptState)?,
                })
            })
            .transpose()
    }

    pub fn restored_origin(&self, family: FamilyHandle) -> Result<Option<RestoredOrigin>, Error> {
        let _ = checked_family(&self.connection, family)?;
        self.connection
            .query_row(
                "SELECT source_family_id,snapshot_utc_ms,source_cursor,known_gap
             FROM restored_origins WHERE family_id=?1",
                [family.family_id.as_slice()],
                |row| {
                    Ok((
                        row.get::<_, Vec<u8>>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, Option<Vec<u8>>>(2)?,
                        row.get::<_, i64>(3)?,
                    ))
                },
            )
            .optional()?
            .map(|(source, at, cursor, gap)| {
                Ok(RestoredOrigin {
                    source_family_id: source.try_into().map_err(|_| Error::CorruptState)?,
                    snapshot_utc_ms: at,
                    source_cursor: cursor
                        .map(|bytes| {
                            bytes
                                .try_into()
                                .map(u64::from_be_bytes)
                                .map_err(|_| Error::CorruptState)
                        })
                        .transpose()?,
                    known_gap: gap == 1,
                })
            })
            .transpose()
    }

    pub fn local_revision(&self, family: FamilyHandle) -> Result<u64, Error> {
        let (last_index, _) = checked_family(&self.connection, family)?;
        u64::try_from(last_index).map_err(|_| Error::CorruptState)
    }

    pub fn load_local(&self, family: FamilyHandle) -> Result<LocalProjection, Error> {
        let (last_index, _) = checked_family(&self.connection, family)?;
        let projection = load_projection(&self.connection, family)?;
        if projection.last_append_index()
            != u64::try_from(last_index).map_err(|_| Error::CorruptState)?
        {
            return Err(Error::CorruptState);
        }
        Ok(projection)
    }

    /// Return immutable local operation bytes through one SQLite snapshot.
    pub(crate) fn local_operation_snapshot(
        &self,
        family: FamilyHandle,
    ) -> Result<Vec<Vec<u8>>, Error> {
        let (last_index, _) = checked_family(&self.connection, family)?;
        let mut statement = self.connection.prepare(
            "SELECT append_index, operation_bytes FROM local_operations
             WHERE family_id = ?1 ORDER BY append_index",
        )?;
        let rows = statement.query_map([family.family_id.as_slice()], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, Vec<u8>>(1)?))
        })?;
        let mut operations = Vec::new();
        for row in rows {
            let (index, bytes) = row?;
            if index != i64::try_from(operations.len() + 1).map_err(|_| Error::CorruptState)? {
                return Err(Error::CorruptState);
            }
            Operation::decode_bound(&bytes, &family.family_id, &family.device_id)?;
            operations.push(bytes);
        }
        if i64::try_from(operations.len()).map_err(|_| Error::CorruptState)? != last_index {
            return Err(Error::CorruptState);
        }
        Ok(operations)
    }

    pub(crate) fn unsent_operations(&self, family: FamilyHandle) -> Result<Vec<Operation>, Error> {
        let (last_index, _) = checked_family(&self.connection, family)?;
        let accepted_index: i64 = self.connection.query_row(
            "SELECT accepted_index FROM local_sync_state WHERE family_id=?1",
            [family.family_id.as_slice()],
            |row| row.get(0),
        )?;
        if accepted_index < 0 || accepted_index > last_index {
            return Err(Error::CorruptState);
        }
        let mut statement = self.connection.prepare(
            "SELECT append_index,operation_bytes FROM local_operations
             WHERE family_id=?1 AND append_index>?2 ORDER BY append_index",
        )?;
        let rows = statement.query_map(
            params![family.family_id.as_slice(), accepted_index],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, Vec<u8>>(1)?)),
        )?;
        let mut operations = Vec::new();
        for row in rows {
            let (index, bytes) = row?;
            if index
                != accepted_index
                    + i64::try_from(operations.len() + 1).map_err(|_| Error::CorruptState)?
            {
                return Err(Error::CorruptState);
            }
            operations.push(Operation::decode_bound(
                &bytes,
                &family.family_id,
                &family.device_id,
            )?);
        }
        if accepted_index + i64::try_from(operations.len()).map_err(|_| Error::CorruptState)?
            != last_index
        {
            return Err(Error::CorruptState);
        }
        Ok(operations)
    }

    pub(crate) fn mark_promotion_accepted(
        &mut self,
        family: FamilyHandle,
        watermark: u64,
    ) -> Result<(), Error> {
        let transaction = self.connection.transaction()?;
        let (last_index, _) = checked_family(&transaction, family)?;
        let watermark = i64::try_from(watermark).map_err(|_| Error::CorruptState)?;
        if watermark > last_index {
            return Err(Error::CorruptState);
        }
        let accepted: i64 = transaction.query_row(
            "SELECT accepted_index FROM local_sync_state WHERE family_id=?1",
            [family.family_id.as_slice()],
            |r| r.get(0),
        )?;
        if accepted != 0 && accepted < watermark {
            return Err(Error::CorruptState);
        }
        if accepted == 0 {
            if load_pending(&transaction, family)?.is_some() {
                return Err(Error::CorruptState);
            }
            transaction.execute(
                "UPDATE local_sync_state SET accepted_index=?2 WHERE family_id=?1",
                params![family.family_id.as_slice(), watermark],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    /// Reserve the HLC, append index, and operation bytes in one transaction.
    /// The full log is replayed before append until a materialized projection
    /// is added; no committed operation can be hidden by a stale cache.
    pub fn append_local(
        &mut self,
        family: FamilyHandle,
        mut new: NewOperation,
        now_ms: i64,
    ) -> Result<AppendedOperation, Error> {
        if new.family_id != family.family_id {
            return Err(Error::WrongFamily);
        }
        if new.author_device_id != family.device_id {
            return Err(Error::WrongDevice);
        }
        let transaction = self.connection.transaction()?;
        let (last_index, previous) = checked_family(&transaction, family)?;
        if transaction
            .query_row(
                "SELECT 1 FROM local_operations WHERE family_id = ?1 AND operation_id = ?2",
                params![family.family_id.as_slice(), new.operation_id.as_slice()],
                |_| Ok(()),
            )
            .optional()?
            .is_some()
        {
            return Err(Error::DuplicateOperation);
        }
        let index = last_index
            .checked_add(1)
            .ok_or(Error::AppendIndexOverflow)?;
        let mut projection = load_projection(&transaction, family)?;
        if projection.last_append_index()
            != u64::try_from(last_index).map_err(|_| Error::CorruptState)?
        {
            return Err(Error::CorruptState);
        }
        let mut clock = Clock::restore(family.family_id, family.device_id, previous, None)?;
        let next = clock.next(now_ms);
        new.hlc = next.stamp;
        let bytes = Operation::encode_new(&new)?;
        let operation = Operation::decode_bound(&bytes, &family.family_id, &family.device_id)?;
        projection.append(
            &operation,
            index.try_into().map_err(|_| Error::CorruptState)?,
        )?;
        transaction.execute(
            "INSERT INTO local_operations(family_id, append_index, operation_id, operation_bytes)
             VALUES (?1, ?2, ?3, ?4)",
            params![
                family.family_id.as_slice(),
                index,
                new.operation_id.as_slice(),
                &bytes
            ],
        )?;
        transaction.execute(
            "UPDATE families SET last_index = ?2, last_hlc_wall = ?3, last_hlc_counter = ?4
             WHERE family_id = ?1",
            params![
                family.family_id.as_slice(),
                index,
                new.hlc.wall_ms,
                new.hlc.counter
            ],
        )?;
        transaction.commit()?;
        Ok(AppendedOperation {
            index: index.try_into().map_err(|_| Error::CorruptState)?,
            bytes,
            clock_anomaly: next.anomaly,
        })
    }

    /// Append against an already verified shared projection while retaining
    /// every local operation above the accepted index. The pinned root check
    /// prevents a stale UI view from validating against the wrong history.
    pub(crate) fn append_shared_local(
        &mut self,
        family: FamilyHandle,
        mut new: NewOperation,
        now_ms: i64,
        verified: &Projection,
        pinned_cursor: u64,
        pinned_head: [u8; 32],
    ) -> Result<AppendedOperation, Error> {
        if new.family_id != family.family_id
            || new.author_device_id != family.device_id
            || verified.family_id() != family.family_id
            || verified.last_cursor() != pinned_cursor
        {
            return Err(Error::WrongFamily);
        }
        if self.saved_removal(family)?.is_some() {
            return Err(Error::RemovedDevice);
        }
        let transaction = self.connection.transaction()?;
        let (last_index, previous) = checked_family(&transaction, family)?;
        let (stored_cursor, stored_head): (i64, Vec<u8>) = transaction.query_row(
            "SELECT pinned_cursor,pinned_head FROM shared_roots WHERE family_id=?1",
            [family.family_id.as_slice()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        if stored_cursor != i64::try_from(pinned_cursor).map_err(|_| Error::CorruptState)?
            || stored_head != pinned_head
        {
            return Err(Error::CorruptState);
        }
        let accepted_index: i64 = transaction.query_row(
            "SELECT accepted_index FROM local_sync_state WHERE family_id=?1",
            [family.family_id.as_slice()],
            |row| row.get(0),
        )?;
        if accepted_index < 0 || accepted_index > last_index {
            return Err(Error::CorruptState);
        }
        let mut statement = transaction.prepare(
            "SELECT append_index,operation_bytes FROM local_operations
             WHERE family_id=?1 AND append_index>?2 ORDER BY append_index",
        )?;
        let rows = statement.query_map(
            params![family.family_id.as_slice(), accepted_index],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, Vec<u8>>(1)?)),
        )?;
        let mut pending = Vec::new();
        for row in rows {
            let (index, bytes) = row?;
            if index
                != accepted_index
                    + i64::try_from(pending.len() + 1).map_err(|_| Error::CorruptState)?
            {
                return Err(Error::CorruptState);
            }
            pending.push(Operation::decode_bound(
                &bytes,
                &family.family_id,
                &family.device_id,
            )?);
        }
        drop(statement);
        if accepted_index + i64::try_from(pending.len()).map_err(|_| Error::CorruptState)?
            != last_index
        {
            return Err(Error::CorruptState);
        }
        let next_index = last_index
            .checked_add(1)
            .ok_or(Error::AppendIndexOverflow)?;
        let mut clock = Clock::restore(family.family_id, family.device_id, previous, None)?;
        let next = clock.next(now_ms);
        new.hlc = next.stamp;
        let bytes = Operation::encode_new(&new)?;
        pending.push(Operation::decode_bound(
            &bytes,
            &family.family_id,
            &family.device_id,
        )?);
        verified.with_local_overlay(&pending)?;
        transaction.execute(
            "INSERT INTO local_operations(family_id,append_index,operation_id,operation_bytes)
             VALUES(?1,?2,?3,?4)",
            params![
                family.family_id.as_slice(),
                next_index,
                new.operation_id.as_slice(),
                &bytes
            ],
        )?;
        transaction.execute(
            "UPDATE families SET last_index=?2,last_hlc_wall=?3,last_hlc_counter=?4 WHERE family_id=?1",
            params![family.family_id.as_slice(),next_index,new.hlc.wall_ms,new.hlc.counter],
        )?;
        transaction.commit()?;
        Ok(AppendedOperation {
            index: u64::try_from(next_index).map_err(|_| Error::CorruptState)?,
            bytes,
            clock_anomaly: next.anomaly,
        })
    }

    /// Stage one already validated local operation as exact durable wire
    /// bytes. A retry returns the original envelope even if the caller now
    /// supplies different current control information. Authority/receipt
    /// verification is owned by the future core Family session.
    #[allow(dead_code)] // Wired to the core authority session, not exported to platform bindings.
    pub(crate) fn stage_next_batch(
        &mut self,
        family: FamilyHandle,
        relay_id: [u8; 32],
        control_head: [u8; 32],
        epoch: u32,
        epoch_key: &[u8; 32],
        signing_seed: &[u8; 32],
    ) -> Result<PreparedBatch, Error> {
        let transaction = self.connection.transaction()?;
        let _ = checked_family(&transaction, family)?;
        if let Some(pending) = load_pending(&transaction, family)? {
            return Ok(pending);
        }
        let (accepted_index, sequence): (i64, i64) = transaction.query_row(
            "SELECT accepted_index, next_sequence FROM local_sync_state WHERE family_id = ?1",
            [family.family_id.as_slice()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        let next_index = accepted_index
            .checked_add(1)
            .ok_or(Error::AppendIndexOverflow)?;
        let operation_bytes: Vec<u8> = transaction
            .query_row(
                "SELECT operation_bytes FROM local_operations
                 WHERE family_id = ?1 AND append_index = ?2",
                params![family.family_id.as_slice(), next_index],
                |row| row.get(0),
            )
            .optional()?
            .ok_or(Error::NoUnsentOperation)?;
        // Operation bytes were validated before the local append committed.
        let plaintext = cbor::encode(&Value::Array(vec![Value::Bytes(operation_bytes.clone())]))
            .map_err(|_| Error::CorruptState)?;
        let mut batch_id = [0u8; 16];
        let mut nonce = [0u8; 24];
        let mut reserved = false;
        for _ in 0..4 {
            getrandom::fill(&mut batch_id).map_err(Error::Random)?;
            batch_id[6] = (batch_id[6] & 0x0f) | 0x40;
            batch_id[8] = (batch_id[8] & 0x3f) | 0x80;
            getrandom::fill(&mut nonce).map_err(Error::Random)?;
            let result = transaction.execute(
                "INSERT INTO local_batch_reservations(family_id, epoch, nonce, batch_id)
                 VALUES (?1, ?2, ?3, ?4)",
                params![
                    family.family_id.as_slice(),
                    epoch,
                    nonce.as_slice(),
                    batch_id.as_slice()
                ],
            );
            match result {
                Ok(_) => {
                    reserved = true;
                    break;
                }
                Err(rusqlite::Error::SqliteFailure(error, _))
                    if error.code == rusqlite::ErrorCode::ConstraintViolation => {}
                Err(error) => return Err(Error::Sqlite(error)),
            }
        }
        if !reserved {
            return Err(Error::CorruptState);
        }
        let header = Header {
            minor: 0,
            family_id: family.family_id,
            relay_id,
            control_head,
            epoch,
            batch_id,
            author_device_id: family.device_id,
            device_sequence: sequence.try_into().map_err(|_| Error::CorruptState)?,
            nonce,
            plaintext_len: plaintext
                .len()
                .try_into()
                .map_err(|_| Error::CorruptState)?,
        };
        let sealed = batch::seal(&header, &[operation_bytes], epoch_key, signing_seed)?;
        transaction.execute(
            "INSERT INTO local_outbox(family_id, from_index, to_index, sequence, batch_id, envelope_bytes, object_hash)
             VALUES (?1, ?2, ?2, ?3, ?4, ?5, ?6)",
            params![family.family_id.as_slice(), next_index, sequence, batch_id.as_slice(),
                &sealed.envelope_bytes, sealed.object_hash.as_slice()],
        )?;
        transaction.commit()?;
        Ok(PreparedBatch {
            from_index: next_index.try_into().map_err(|_| Error::CorruptState)?,
            to_index: next_index.try_into().map_err(|_| Error::CorruptState)?,
            sequence: sequence.try_into().map_err(|_| Error::CorruptState)?,
            batch_id,
            envelope_bytes: sealed.envelope_bytes,
            object_hash: sealed.object_hash,
        })
    }

    #[allow(dead_code)] // Wired to the core authority session, not exported to platform bindings.
    pub(crate) fn pending_batch(
        &self,
        family: FamilyHandle,
    ) -> Result<Option<PreparedBatch>, Error> {
        let _ = checked_family(&self.connection, family)?;
        load_pending(&self.connection, family)
    }

    /// Only the core Family session calls this after independently verifying
    /// the signed receipt and matching committed envelope. The exact evidence
    /// and outbox advancement commit together, preserving crash replay.
    pub(crate) fn record_verified_acceptance(
        &mut self,
        family: FamilyHandle,
        pending: &PreparedBatch,
        receipt_bytes: &[u8],
        cursor: u64,
    ) -> Result<(), Error> {
        let transaction = self.connection.transaction()?;
        let _ = checked_family(&transaction, family)?;
        if load_pending(&transaction, family)?.as_ref() != Some(pending) {
            return Err(Error::CorruptState);
        }
        let (accepted_index, next_sequence): (i64, i64) = transaction.query_row(
            "SELECT accepted_index, next_sequence FROM local_sync_state WHERE family_id = ?1",
            [family.family_id.as_slice()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        if accepted_index.checked_add(1) != i64::try_from(pending.from_index).ok()
            || pending.to_index != pending.from_index
            || next_sequence != i64::try_from(pending.sequence).map_err(|_| Error::CorruptState)?
        {
            return Err(Error::CorruptState);
        }
        let expected_cursor: i64 = transaction.query_row(
            "SELECT COALESCE(MAX(cursor), 1) + 1 FROM accepted_local_batches WHERE family_id = ?1",
            [family.family_id.as_slice()],
            |row| row.get(0),
        )?;
        if expected_cursor != i64::try_from(cursor).map_err(|_| Error::CorruptState)? {
            return Err(Error::CorruptState);
        }
        transaction.execute(
            "INSERT INTO accepted_local_batches(family_id, cursor, to_index, batch_id, envelope_bytes, receipt_bytes)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![family.family_id.as_slice(), expected_cursor,
                i64::try_from(pending.to_index).map_err(|_| Error::CorruptState)?,
                pending.batch_id.as_slice(), &pending.envelope_bytes, receipt_bytes],
        )?;
        transaction.execute(
            "UPDATE local_sync_state SET accepted_index = ?2, next_sequence = ?3 WHERE family_id = ?1",
            params![family.family_id.as_slice(),
                i64::try_from(pending.to_index).map_err(|_| Error::CorruptState)?,
                next_sequence.checked_add(1).ok_or(Error::CorruptState)?],
        )?;
        transaction.execute(
            "DELETE FROM local_outbox WHERE family_id = ?1",
            [family.family_id.as_slice()],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub(crate) fn accepted_local_batches(
        &self,
        family: FamilyHandle,
    ) -> Result<Vec<AcceptedLocalBatch>, Error> {
        let _ = checked_family(&self.connection, family)?;
        let mut statement = self.connection.prepare(
            "SELECT envelope_bytes, receipt_bytes FROM accepted_local_batches
             WHERE family_id = ?1 ORDER BY cursor",
        )?;
        let rows = statement.query_map([family.family_id.as_slice()], |row| {
            Ok(AcceptedLocalBatch {
                envelope_bytes: row.get(0)?,
                receipt_bytes: row.get(1)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Error::Sqlite)
    }

    pub(crate) fn initialize_shared_history(
        &mut self,
        family: FamilyHandle,
        relay_public_key: [u8; 32],
        genesis_bytes: &[u8],
        genesis_head: [u8; 32],
    ) -> Result<(), Error> {
        let transaction = self.connection.transaction()?;
        let _ = checked_family(&transaction, family)?;
        transaction.execute(
            "INSERT OR IGNORE INTO shared_roots
             (family_id, relay_public_key, genesis_bytes, pinned_cursor, pinned_head)
             VALUES (?1, ?2, ?3, 1, ?4)",
            params![
                family.family_id.as_slice(),
                relay_public_key.as_slice(),
                genesis_bytes,
                genesis_head.as_slice()
            ],
        )?;
        let row: (Vec<u8>, Vec<u8>) = transaction.query_row(
            "SELECT relay_public_key, genesis_bytes FROM shared_roots WHERE family_id = ?1",
            [family.family_id.as_slice()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        if row.0 != relay_public_key || row.1 != genesis_bytes {
            return Err(Error::CorruptState);
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn is_shared_family(&self, family: FamilyHandle) -> Result<bool, Error> {
        Ok(self.shared_history(family)?.is_some())
    }

    pub(crate) fn shared_history(
        &self,
        family: FamilyHandle,
    ) -> Result<Option<SharedHistory>, Error> {
        let _ = checked_family(&self.connection, family)?;
        let root = self
            .connection
            .query_row(
                "SELECT relay_public_key, genesis_bytes, pinned_cursor, pinned_head
                 FROM shared_roots WHERE family_id = ?1",
                [family.family_id.as_slice()],
                |row| {
                    Ok((
                        row.get::<_, Vec<u8>>(0)?,
                        row.get::<_, Vec<u8>>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, Vec<u8>>(3)?,
                    ))
                },
            )
            .optional()?;
        let Some((relay, genesis, cursor, head)) = root else {
            return Ok(None);
        };
        let mut statement = self.connection.prepare(
            "SELECT cursor, kind, committed_bytes, receipt_bytes FROM shared_entries
             WHERE family_id = ?1 ORDER BY cursor",
        )?;
        let entries = statement
            .query_map([family.family_id.as_slice()], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, Vec<u8>>(2)?,
                    row.get::<_, Vec<u8>>(3)?,
                ))
            })?
            .map(|row| {
                let (cursor, kind, committed_bytes, receipt_bytes) = row?;
                Ok(SharedHistoryRow {
                    cursor: cursor.try_into().map_err(|_| Error::CorruptState)?,
                    kind: kind.try_into().map_err(|_| Error::CorruptState)?,
                    committed_bytes,
                    receipt_bytes,
                })
            })
            .collect::<Result<Vec<_>, Error>>()?;
        Ok(Some(SharedHistory {
            relay_public_key: relay.try_into().map_err(|_| Error::CorruptState)?,
            genesis_bytes: genesis,
            pinned_cursor: cursor.try_into().map_err(|_| Error::CorruptState)?,
            pinned_head: head.try_into().map_err(|_| Error::CorruptState)?,
            entries,
        }))
    }

    pub(crate) fn append_verified_shared_entry(
        &mut self,
        family: FamilyHandle,
        entry: VerifiedSharedEntry<'_>,
    ) -> Result<(), Error> {
        if (entry.kind != 1 && entry.kind != 2)
            || (entry.kind == 1
                && (entry.own_sequence.is_some() || entry.matching_pending.is_some()))
        {
            return Err(Error::CorruptState);
        }
        let cursor = entry
            .previous_cursor
            .checked_add(1)
            .ok_or(Error::CorruptState)?;
        let transaction = self.connection.transaction()?;
        let _ = checked_family(&transaction, family)?;
        let changed = transaction.execute(
            "UPDATE shared_roots SET pinned_cursor = ?4, pinned_head = ?5
             WHERE family_id = ?1 AND pinned_cursor = ?2 AND pinned_head = ?3",
            params![
                family.family_id.as_slice(),
                i64::try_from(entry.previous_cursor).map_err(|_| Error::CorruptState)?,
                entry.previous_head.as_slice(),
                i64::try_from(cursor).map_err(|_| Error::CorruptState)?,
                entry.next_head.as_slice()
            ],
        )?;
        if changed != 1 {
            return Err(Error::CorruptState);
        }
        transaction.execute(
            "INSERT INTO shared_entries
             (family_id, cursor, kind, committed_bytes, receipt_bytes)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                family.family_id.as_slice(),
                i64::try_from(cursor).map_err(|_| Error::CorruptState)?,
                i64::from(entry.kind),
                entry.committed_bytes,
                entry.receipt_bytes
            ],
        )?;
        if let Some(sequence) = entry.own_sequence {
            let (accepted_index, next_sequence): (i64, i64) = transaction.query_row(
                "SELECT accepted_index, next_sequence FROM local_sync_state
                 WHERE family_id = ?1",
                [family.family_id.as_slice()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )?;
            let sequence = i64::try_from(sequence).map_err(|_| Error::CorruptState)?;
            if let Some(pending) = entry.matching_pending {
                if load_pending(&transaction, family)?.as_ref() != Some(pending)
                    || pending.envelope_bytes != entry.committed_bytes
                    || pending.sequence != sequence as u64
                    || pending.from_index
                        != u64::try_from(accepted_index.checked_add(1).ok_or(Error::CorruptState)?)
                            .map_err(|_| Error::CorruptState)?
                    || pending.to_index != pending.from_index
                    || next_sequence != sequence
                {
                    return Err(Error::CorruptState);
                }
                transaction.execute(
                    "UPDATE local_sync_state
                     SET accepted_index = ?2, next_sequence = ?3 WHERE family_id = ?1",
                    params![
                        family.family_id.as_slice(),
                        i64::try_from(pending.to_index).map_err(|_| Error::CorruptState)?,
                        sequence.checked_add(1).ok_or(Error::CorruptState)?
                    ],
                )?;
                transaction.execute(
                    "DELETE FROM local_outbox WHERE family_id = ?1",
                    [family.family_id.as_slice()],
                )?;
            } else if load_pending(&transaction, family)?.is_none() && next_sequence <= sequence {
                transaction.execute(
                    "UPDATE local_sync_state SET next_sequence = ?2 WHERE family_id = ?1",
                    params![
                        family.family_id.as_slice(),
                        sequence.checked_add(1).ok_or(Error::CorruptState)?
                    ],
                )?;
            }
        }
        transaction.commit()?;
        Ok(())
    }

    pub(crate) fn record_verified_rejection(
        &mut self,
        family: FamilyHandle,
        pending: &PreparedBatch,
        receipt_bytes: &[u8],
        pinned_cursor: u64,
        pinned_head: [u8; 32],
        new_next_sequence: u64,
    ) -> Result<(), Error> {
        let transaction = self.connection.transaction()?;
        let _ = checked_family(&transaction, family)?;
        if load_pending(&transaction, family)?.as_ref() != Some(pending) {
            return Err(Error::CorruptState);
        }
        let (current_cursor, current_head): (i64, Vec<u8>) = transaction.query_row(
            "SELECT pinned_cursor, pinned_head FROM shared_roots WHERE family_id = ?1",
            [family.family_id.as_slice()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        if current_cursor != i64::try_from(pinned_cursor).map_err(|_| Error::CorruptState)?
            || current_head != pinned_head
        {
            return Err(Error::CorruptState);
        }
        let next_sequence: i64 = transaction.query_row(
            "SELECT next_sequence FROM local_sync_state WHERE family_id = ?1",
            [family.family_id.as_slice()],
            |row| row.get(0),
        )?;
        if next_sequence != i64::try_from(pending.sequence).map_err(|_| Error::CorruptState)? {
            return Err(Error::CorruptState);
        }
        if new_next_sequence < pending.sequence {
            return Err(Error::CorruptState);
        }
        transaction.execute(
            "INSERT INTO rejected_local_batches
             (family_id, batch_id, envelope_bytes, receipt_bytes, rejected_at_cursor)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                family.family_id.as_slice(),
                pending.batch_id.as_slice(),
                &pending.envelope_bytes,
                receipt_bytes,
                i64::try_from(pinned_cursor).map_err(|_| Error::CorruptState)?
            ],
        )?;
        let deleted = transaction.execute(
            "DELETE FROM local_outbox WHERE family_id = ?1 AND batch_id = ?2",
            params![family.family_id.as_slice(), pending.batch_id.as_slice()],
        )?;
        if deleted != 1 {
            return Err(Error::CorruptState);
        }
        transaction.execute(
            "UPDATE local_sync_state SET next_sequence = ?2 WHERE family_id = ?1",
            params![
                family.family_id.as_slice(),
                i64::try_from(new_next_sequence).map_err(|_| Error::CorruptState)?
            ],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub(crate) fn store_verified_shared_object(
        &mut self,
        family: FamilyHandle,
        transition_id: [u8; 16],
        kind: u16,
        object_id: [u8; 16],
        object_bytes: &[u8],
    ) -> Result<(), Error> {
        let transaction = self.connection.transaction()?;
        let _ = checked_family(&transaction, family)?;
        transaction.execute(
            "INSERT OR IGNORE INTO shared_objects
             (family_id, object_id, transition_id, kind, object_bytes)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                family.family_id.as_slice(),
                object_id.as_slice(),
                transition_id.as_slice(),
                i64::from(kind),
                object_bytes
            ],
        )?;
        let prior: (Vec<u8>, i64, Vec<u8>) = transaction.query_row(
            "SELECT transition_id, kind, object_bytes FROM shared_objects
             WHERE family_id = ?1 AND object_id = ?2",
            params![family.family_id.as_slice(), object_id.as_slice()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
        if prior.0 != transition_id || prior.1 != i64::from(kind) || prior.2 != object_bytes {
            return Err(Error::CorruptState);
        }
        transaction.commit()?;
        Ok(())
    }

    pub(crate) fn shared_objects(&self, family: FamilyHandle) -> Result<SharedObjects, Error> {
        let _ = checked_family(&self.connection, family)?;
        let mut statement = self.connection.prepare(
            "SELECT object_id, object_bytes FROM shared_objects
             WHERE family_id = ?1 ORDER BY object_id",
        )?;
        statement
            .query_map([family.family_id.as_slice()], |row| {
                Ok((row.get::<_, Vec<u8>>(0)?, row.get::<_, Vec<u8>>(1)?))
            })?
            .map(|row| {
                let (id, bytes) = row?;
                Ok((id.try_into().map_err(|_| Error::CorruptState)?, bytes))
            })
            .collect::<Result<Vec<_>, Error>>()
    }

    pub(crate) fn create_enrollment_attempt(
        &mut self,
        row: &EnrollmentRow,
        sparse_controls: &[(u64, Vec<u8>)],
        relay_public_key: [u8; 32],
        genesis_head: [u8; 32],
    ) -> Result<(), Error> {
        if !ids::is_v4(&row.family.family_id)
            || !ids::is_v4(&row.family.device_id)
            || !ids::is_v4(&row.invitation_id)
        {
            return Err(Error::InvalidId);
        }
        let transaction = self.connection.transaction()?;
        transaction.execute(
            "INSERT INTO families(family_id, device_id) VALUES (?1, ?2)",
            params![
                row.family.family_id.as_slice(),
                row.family.device_id.as_slice()
            ],
        )?;
        transaction.execute(
            "INSERT INTO local_sync_state(family_id) VALUES (?1)",
            [row.family.family_id.as_slice()],
        )?;
        transaction.execute(
            "INSERT INTO shared_roots
             (family_id, relay_public_key, genesis_bytes, pinned_cursor, pinned_head)
             VALUES (?1, ?2, ?3, 1, ?4)",
            params![
                row.family.family_id.as_slice(),
                relay_public_key.as_slice(),
                &row.genesis_bytes,
                genesis_head.as_slice(),
            ],
        )?;
        transaction.execute(
            "INSERT INTO enrollment_attempts
             (family_id, device_id, invitation_id, genesis_bytes, issue_bytes,
              candidate_bytes, secret_nonce, secret_ciphertext)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                row.family.family_id.as_slice(),
                row.family.device_id.as_slice(),
                row.invitation_id.as_slice(),
                &row.genesis_bytes,
                &row.issue_bytes,
                &row.candidate_bytes,
                row.secret_nonce.as_slice(),
                &row.secret_ciphertext
            ],
        )?;
        for (cursor, bytes) in sparse_controls {
            transaction.execute(
                "INSERT INTO enrollment_controls(family_id,cursor,committed_bytes) VALUES(?1,?2,?3)",
                params![
                    row.family.family_id.as_slice(),
                    i64::try_from(*cursor).map_err(|_| Error::CorruptState)?,
                    bytes,
                ],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub(crate) fn enrollment_controls(
        &self,
        family: FamilyHandle,
    ) -> Result<Vec<(u64, Vec<u8>)>, Error> {
        let _ = checked_family(&self.connection, family)?;
        let mut query = self.connection.prepare(
            "SELECT cursor,committed_bytes FROM enrollment_controls
             WHERE family_id=?1 ORDER BY cursor",
        )?;
        query
            .query_map([family.family_id.as_slice()], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, Vec<u8>>(1)?))
            })?
            .map(|result| {
                let (cursor, bytes) = result?;
                Ok((
                    u64::try_from(cursor).map_err(|_| Error::CorruptState)?,
                    bytes,
                ))
            })
            .collect()
    }

    pub(crate) fn append_enrollment_control(
        &mut self,
        family: FamilyHandle,
        cursor: u64,
        bytes: &[u8],
    ) -> Result<(), Error> {
        let tx = self.connection.transaction()?;
        let _ = checked_family(&tx, family)?;
        let cursor = i64::try_from(cursor).map_err(|_| Error::CorruptState)?;
        tx.execute(
            "INSERT OR IGNORE INTO enrollment_controls(family_id,cursor,committed_bytes)
             VALUES(?1,?2,?3)",
            params![family.family_id.as_slice(), cursor, bytes],
        )?;
        let prior: Vec<u8> = tx.query_row(
            "SELECT committed_bytes FROM enrollment_controls WHERE family_id=?1 AND cursor=?2",
            params![family.family_id.as_slice(), cursor],
            |row| row.get(0),
        )?;
        if prior != bytes {
            return Err(Error::CorruptState);
        }
        tx.commit()?;
        Ok(())
    }

    pub(crate) fn archived_enrollment_claims(
        &self,
        family: FamilyHandle,
    ) -> Result<Vec<Vec<u8>>, Error> {
        let _ = checked_family(&self.connection, family)?;
        let mut query = self.connection.prepare(
            "SELECT candidate_bytes FROM enrollment_claim_candidates
             WHERE family_id=?1 ORDER BY transition_id",
        )?;
        query
            .query_map([family.family_id.as_slice()], |row| row.get(0))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(Error::from)
    }

    pub(crate) fn enrollment_terminal_status(
        &self,
        family: FamilyHandle,
    ) -> Result<Option<Vec<u8>>, Error> {
        let _ = checked_family(&self.connection, family)?;
        self.connection
            .query_row(
                "SELECT response_bytes FROM enrollment_terminal_status WHERE family_id=?1",
                [family.family_id.as_slice()],
                |row| row.get(0),
            )
            .optional()
            .map_err(Error::from)
    }

    pub(crate) fn save_enrollment_terminal_status(
        &mut self,
        family: FamilyHandle,
        response: &[u8],
    ) -> Result<(), Error> {
        let tx = self.connection.transaction()?;
        let _ = checked_family(&tx, family)?;
        tx.execute(
            "INSERT OR IGNORE INTO enrollment_terminal_status(family_id,response_bytes)
             VALUES(?1,?2)",
            params![family.family_id.as_slice(), response],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Keep every superseded exact claim so an accepted lost response can
    /// still be reconciled after a newer verified head requires rebasing.
    pub(crate) fn swap_enrollment_claim(
        &mut self,
        family: FamilyHandle,
        expected: &[u8],
        expected_transition: [u8; 16],
        replacement: &[u8],
    ) -> Result<(), Error> {
        let tx = self.connection.transaction()?;
        let _ = checked_family(&tx, family)?;
        let prior: Vec<u8> = tx.query_row(
            "SELECT candidate_bytes FROM enrollment_attempts WHERE family_id=?1 AND device_id=?2",
            params![family.family_id.as_slice(), family.device_id.as_slice()],
            |row| row.get(0),
        )?;
        if prior != expected {
            return Err(Error::CorruptState);
        }
        tx.execute(
            "INSERT OR IGNORE INTO enrollment_claim_candidates(family_id,transition_id,candidate_bytes)
             VALUES(?1,?2,?3)",
            params![family.family_id.as_slice(), expected_transition.as_slice(), expected],
        )?;
        let archived: Vec<u8> = tx.query_row(
            "SELECT candidate_bytes FROM enrollment_claim_candidates
             WHERE family_id=?1 AND transition_id=?2",
            params![family.family_id.as_slice(), expected_transition.as_slice()],
            |row| row.get(0),
        )?;
        if archived != expected {
            return Err(Error::CorruptState);
        }
        tx.execute(
            "UPDATE enrollment_attempts SET candidate_bytes=?1 WHERE family_id=?2 AND device_id=?3",
            params![
                replacement,
                family.family_id.as_slice(),
                family.device_id.as_slice()
            ],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub(crate) fn enrollment_attempt(
        &self,
        family_id: [u8; 16],
    ) -> Result<Option<EnrollmentRow>, Error> {
        self.connection
            .query_row(
                "SELECT device_id, invitation_id, genesis_bytes, issue_bytes,
                        candidate_bytes, secret_nonce, secret_ciphertext
                 FROM enrollment_attempts WHERE family_id = ?1",
                [family_id.as_slice()],
                |row| {
                    Ok((
                        row.get::<_, Vec<u8>>(0)?,
                        row.get::<_, Vec<u8>>(1)?,
                        row.get::<_, Vec<u8>>(2)?,
                        row.get::<_, Vec<u8>>(3)?,
                        row.get::<_, Vec<u8>>(4)?,
                        row.get::<_, Vec<u8>>(5)?,
                        row.get::<_, Vec<u8>>(6)?,
                    ))
                },
            )
            .optional()?
            .map(|row| {
                let family = FamilyHandle {
                    family_id,
                    device_id: row.0.try_into().map_err(|_| Error::CorruptState)?,
                };
                let _ = checked_family(&self.connection, family)?;
                Ok(EnrollmentRow {
                    family,
                    invitation_id: row.1.try_into().map_err(|_| Error::CorruptState)?,
                    genesis_bytes: row.2,
                    issue_bytes: row.3,
                    candidate_bytes: row.4,
                    secret_nonce: row.5.try_into().map_err(|_| Error::CorruptState)?,
                    secret_ciphertext: row.6,
                })
            })
            .transpose()
    }

    pub fn has_enrollment_attempt(&self, family_id: [u8; 16]) -> Result<bool, Error> {
        Ok(self
            .connection
            .query_row(
                "SELECT 1 FROM enrollment_attempts WHERE family_id = ?1",
                [family_id.as_slice()],
                |_| Ok(()),
            )
            .optional()?
            .is_some())
    }

    pub(crate) fn save_manager_creation(&mut self, row: &ManagerCreationRow) -> Result<(), Error> {
        let transaction = self.connection.transaction()?;
        let (last_index, _) = checked_family(&transaction, row.family)?;
        let watermark = i64::try_from(row.chunks.last().map_or(0, |chunk| chunk.last_local_index))
            .map_err(|_| Error::CorruptState)?;
        if last_index < watermark {
            return Err(Error::CorruptState);
        }
        transaction.execute(
            "INSERT INTO manager_creations
             (family_id,device_id,relay_public_key,promotion_id,transition_id,object_id,object_bytes,
              candidate_bytes,secret_nonce,secret_ciphertext)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
            params![
                &row.family.family_id[..],
                &row.family.device_id[..],
                &row.relay_public_key[..],
                &row.promotion_id[..],
                &row.transition_id[..],
                &row.object_id[..],
                &row.object_bytes,
                &row.candidate_bytes,
                &row.secret_nonce[..],
                &row.secret_ciphertext,
            ],
        )?;
        for chunk in &row.chunks {
            transaction.execute(
                "INSERT INTO manager_promotion_chunks
                 (family_id,chunk_index,object_id,first_local_index,last_local_index,object_bytes)
                 VALUES(?1,?2,?3,?4,?5,?6)",
                params![
                    row.family.family_id.as_slice(),
                    chunk.index,
                    chunk.object_id.as_slice(),
                    chunk.first_local_index,
                    chunk.last_local_index,
                    &chunk.object_bytes,
                ],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub(crate) fn manager_creation(
        &self,
        family: FamilyHandle,
    ) -> Result<Option<ManagerCreationRow>, Error> {
        let _ = checked_family(&self.connection, family)?;
        self.connection
            .query_row(
            "SELECT device_id,relay_public_key,promotion_id,transition_id,object_id,object_bytes,
                    candidate_bytes,secret_nonce,secret_ciphertext
             FROM manager_creations WHERE family_id=?1",
                [&family.family_id[..]],
                |r| {
                    Ok((
                        r.get::<_, Vec<u8>>(0)?,
                        r.get::<_, Vec<u8>>(1)?,
                r.get::<_, Vec<u8>>(2)?,
                r.get::<_, Vec<u8>>(3)?,
                r.get::<_, Vec<u8>>(4)?,
                r.get::<_, Vec<u8>>(5)?,
                r.get::<_, Vec<u8>>(6)?,
                r.get::<_, Vec<u8>>(7)?,
                r.get::<_, Vec<u8>>(8)?,
                    ))
                },
            )
            .optional()?
            .map(|row| {
                if row.0 != family.device_id {
                    return Err(Error::WrongDevice);
                }
                let mut statement = self.connection.prepare(
                    "SELECT chunk_index,object_id,first_local_index,last_local_index,object_bytes
                     FROM manager_promotion_chunks WHERE family_id=?1 ORDER BY chunk_index",
                )?;
                let chunks = statement
                    .query_map([family.family_id.as_slice()], |r| {
                        Ok((r.get::<_, u32>(0)?, r.get::<_, Vec<u8>>(1)?, r.get::<_, u64>(2)?,
                            r.get::<_, u64>(3)?, r.get::<_, Vec<u8>>(4)?))
                    })?
                    .map(|r| {
                        let (index, object_id, first_local_index, last_local_index, object_bytes) = r?;
                        Ok(PromotionChunkRow { index, object_id: object_id.try_into().map_err(|_| Error::CorruptState)?,
                            first_local_index, last_local_index, object_bytes })
                    })
                    .collect::<Result<Vec<_>, Error>>()?;
                Ok(ManagerCreationRow {
                    family,
                    relay_public_key: row.1.try_into().map_err(|_| Error::CorruptState)?,
                promotion_id: row.2.try_into().map_err(|_| Error::CorruptState)?,
                transition_id: row.3.try_into().map_err(|_| Error::CorruptState)?,
                object_id: row.4.try_into().map_err(|_| Error::CorruptState)?,
                object_bytes: row.5,
                candidate_bytes: row.6,
                secret_nonce: row.7.try_into().map_err(|_| Error::CorruptState)?,
                secret_ciphertext: row.8,
                chunks,
                })
            })
            .transpose()
    }

    pub(crate) fn save_first_invite_issue(&mut self, row: &InviteIssueRow) -> Result<(), Error> {
        self.save_invite_issue(row, true)
    }

    pub(crate) fn save_later_invite_issue(&mut self, row: &InviteIssueRow) -> Result<(), Error> {
        self.save_invite_issue(row, false)
    }

    fn save_invite_issue(&mut self, row: &InviteIssueRow, is_first: bool) -> Result<(), Error> {
        let transaction = self.connection.transaction()?;
        let _ = checked_family(&transaction, row.family)?;
        transaction.execute(
            "INSERT INTO invite_issues
             (family_id,device_id,invitation_id,transition_id,object_id,object_bytes,
              candidate_bytes,secret_nonce,secret_ciphertext,is_first)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
            params![
                &row.family.family_id[..],
                &row.family.device_id[..],
                &row.invitation_id[..],
                &row.transition_id[..],
                &row.object_id[..],
                &row.object_bytes,
                &row.candidate_bytes,
                &row.secret_nonce[..],
                &row.secret_ciphertext,
                i64::from(is_first),
            ],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub(crate) fn first_invite_issue(
        &self,
        family: FamilyHandle,
    ) -> Result<Option<InviteIssueRow>, Error> {
        let _ = checked_family(&self.connection, family)?;
        let invitation_id: Option<Vec<u8>> = self
            .connection
            .query_row(
                "SELECT invitation_id FROM invite_issues WHERE family_id=?1 AND is_first=1",
                [family.family_id.as_slice()],
                |row| row.get(0),
            )
            .optional()?;
        invitation_id
            .map(|id| self.invite_issue(family, id.try_into().map_err(|_| Error::CorruptState)?))
            .transpose()
            .map(|row| row.flatten())
    }

    pub(crate) fn invite_issue(
        &self,
        family: FamilyHandle,
        invitation_id: [u8; 16],
    ) -> Result<Option<InviteIssueRow>, Error> {
        let _ = checked_family(&self.connection, family)?;
        self.connection
            .query_row(
                "SELECT device_id,invitation_id,transition_id,object_id,object_bytes,
                    candidate_bytes,secret_nonce,secret_ciphertext
             FROM invite_issues WHERE family_id=?1 AND invitation_id=?2",
                params![family.family_id.as_slice(), invitation_id.as_slice()],
                |r| {
                    Ok((
                        r.get::<_, Vec<u8>>(0)?,
                        r.get::<_, Vec<u8>>(1)?,
                        r.get::<_, Vec<u8>>(2)?,
                        r.get::<_, Vec<u8>>(3)?,
                        r.get::<_, Vec<u8>>(4)?,
                        r.get::<_, Vec<u8>>(5)?,
                        r.get::<_, Vec<u8>>(6)?,
                        r.get::<_, Vec<u8>>(7)?,
                    ))
                },
            )
            .optional()?
            .map(|row| {
                if row.0 != family.device_id {
                    return Err(Error::WrongDevice);
                }
                Ok(InviteIssueRow {
                    family,
                    invitation_id: row.1.try_into().map_err(|_| Error::CorruptState)?,
                    transition_id: row.2.try_into().map_err(|_| Error::CorruptState)?,
                    object_id: row.3.try_into().map_err(|_| Error::CorruptState)?,
                    object_bytes: row.4,
                    candidate_bytes: row.5,
                    secret_nonce: row.6.try_into().map_err(|_| Error::CorruptState)?,
                    secret_ciphertext: row.7,
                })
            })
            .transpose()
    }

    pub(crate) fn save_prepared_control(&mut self, row: &PreparedControlRow) -> Result<(), Error> {
        let tx = self.connection.transaction()?;
        let _ = checked_family(&tx, row.family)?;
        tx.execute(
            "INSERT INTO prepared_controls
             (family_id,kind,device_id,transition_id,candidate_bytes,objects_bytes,
              secret_nonce,secret_ciphertext)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
            params![
                &row.family.family_id[..],
                i64::from(row.kind),
                &row.family.device_id[..],
                &row.transition_id[..],
                &row.candidate_bytes,
                &row.objects_bytes,
                &row.secret_nonce[..],
                &row.secret_ciphertext,
            ],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub(crate) fn prepared_control(
        &self,
        family: FamilyHandle,
        kind: u8,
    ) -> Result<Option<PreparedControlRow>, Error> {
        let _ = checked_family(&self.connection, family)?;
        self.connection
            .query_row(
                "SELECT device_id,transition_id,candidate_bytes,objects_bytes,
                    secret_nonce,secret_ciphertext
             FROM prepared_controls WHERE family_id=?1 AND kind=?2",
                params![&family.family_id[..], i64::from(kind)],
                |r| {
                    Ok((
                        r.get::<_, Vec<u8>>(0)?,
                        r.get::<_, Vec<u8>>(1)?,
                        r.get::<_, Vec<u8>>(2)?,
                        r.get::<_, Vec<u8>>(3)?,
                        r.get::<_, Vec<u8>>(4)?,
                        r.get::<_, Vec<u8>>(5)?,
                    ))
                },
            )
            .optional()?
            .map(|row| {
                if row.0 != family.device_id {
                    return Err(Error::WrongDevice);
                }
                Ok(PreparedControlRow {
                    family,
                    kind,
                    transition_id: row.1.try_into().map_err(|_| Error::CorruptState)?,
                    candidate_bytes: row.2,
                    objects_bytes: row.3,
                    secret_nonce: row.4.try_into().map_err(|_| Error::CorruptState)?,
                    secret_ciphertext: row.5,
                })
            })
            .transpose()
    }

    pub(crate) fn delete_prepared_control(
        &mut self,
        family: FamilyHandle,
        kind: u8,
        transition_id: [u8; 16],
    ) -> Result<(), Error> {
        let tx = self.connection.transaction()?;
        let _ = checked_family(&tx, family)?;
        tx.execute(
            "DELETE FROM prepared_controls
             WHERE family_id=?1 AND kind=?2 AND device_id=?3 AND transition_id=?4",
            params![
                family.family_id.as_slice(),
                i64::from(kind),
                family.device_id.as_slice(),
                transition_id.as_slice(),
            ],
        )?;
        tx.commit()?;
        Ok(())
    }
}

/// Version two replaces the one-row invitation slot with per-invitation
/// durable preparation. Copying legacy bytes preserves exact retries and
/// bearer-link reconstruction after an upgrade.
fn migrate_invite_issues(connection: &Connection) -> Result<(), Error> {
    connection.execute_batch(
        "CREATE TABLE invite_issues (
           family_id BLOB NOT NULL CHECK(length(family_id) = 16),
           device_id BLOB NOT NULL CHECK(length(device_id) = 16),
           invitation_id BLOB NOT NULL CHECK(length(invitation_id) = 16),
           transition_id BLOB NOT NULL CHECK(length(transition_id) = 16),
           object_id BLOB NOT NULL CHECK(length(object_id) = 16),
           object_bytes BLOB NOT NULL,
           candidate_bytes BLOB NOT NULL,
           secret_nonce BLOB NOT NULL CHECK(length(secret_nonce) = 24),
           secret_ciphertext BLOB NOT NULL,
           is_first INTEGER NOT NULL CHECK(is_first IN (0,1)),
           PRIMARY KEY (family_id, invitation_id),
           UNIQUE (family_id, transition_id),
           UNIQUE (family_id, object_id),
           FOREIGN KEY (family_id) REFERENCES families(family_id)
         );
         CREATE UNIQUE INDEX first_invite_per_family
           ON invite_issues(family_id) WHERE is_first=1;
         INSERT INTO invite_issues
           (family_id,device_id,invitation_id,transition_id,object_id,
            object_bytes,candidate_bytes,secret_nonce,secret_ciphertext,is_first)
           SELECT family_id,device_id,invitation_id,transition_id,object_id,
                  object_bytes,candidate_bytes,secret_nonce,secret_ciphertext,1
           FROM first_invite_issues;
         PRAGMA user_version = 2;",
    )?;
    Ok(())
}

fn migrate_claim_candidates(connection: &Connection) -> Result<(), Error> {
    connection.execute_batch(
        "CREATE TABLE enrollment_claim_candidates (
           family_id BLOB NOT NULL CHECK(length(family_id) = 16),
           transition_id BLOB NOT NULL CHECK(length(transition_id) = 16),
           candidate_bytes BLOB NOT NULL,
           PRIMARY KEY (family_id, transition_id),
           FOREIGN KEY (family_id) REFERENCES enrollment_attempts(family_id)
         );
         PRAGMA user_version = 3;",
    )?;
    Ok(())
}

fn migrate_enrollment_terminal_status(connection: &Connection) -> Result<(), Error> {
    connection.execute_batch(
        "CREATE TABLE enrollment_terminal_status (
           family_id BLOB PRIMARY KEY CHECK(length(family_id) = 16),
           response_bytes BLOB NOT NULL,
           FOREIGN KEY (family_id) REFERENCES enrollment_attempts(family_id)
         );
         PRAGMA user_version = 4;",
    )?;
    Ok(())
}

#[cfg(test)]
mod invite_migration_tests {
    use super::*;

    #[test]
    fn simultaneous_legacy_openers_apply_each_migration_once() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("concurrent-legacy.db");
        let store = SqliteStore::open(&path).unwrap();
        store
            .connection
            .execute_batch(
                "DROP TABLE enrollment_terminal_status;
             DROP TABLE enrollment_claim_candidates;
             DROP TABLE invite_issues;
             PRAGMA user_version = 1;",
            )
            .unwrap();
        drop(store);
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(8));
        let openings: Vec<_> = (0..8)
            .map(|_| {
                let path = path.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    SqliteStore::open(path)
                })
            })
            .collect();
        for opening in openings {
            assert!(opening.join().unwrap().is_ok());
        }
        let store = SqliteStore::open(&path).unwrap();
        let version: u32 = store
            .connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, 4);
    }

    #[test]
    fn version_three_store_adds_terminal_receipt_table() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("version-three.db");
        let mut store = SqliteStore::open(&path).unwrap();
        let family = store.create_family(v4(0x73), v4(0x74)).unwrap();
        store
            .connection
            .execute_batch("DROP TABLE enrollment_terminal_status; PRAGMA user_version = 3;")
            .unwrap();
        drop(store);
        let store = SqliteStore::open(&path).unwrap();
        assert_eq!(store.families().unwrap(), vec![family]);
        assert!(store.enrollment_terminal_status(family).unwrap().is_none());
        let version: u32 = store
            .connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, 4);
    }

    #[test]
    fn version_two_store_adds_claim_archive_without_changing_families() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("version-two.db");
        let mut store = SqliteStore::open(&path).unwrap();
        let family = store.create_family(v4(0x71), v4(0x72)).unwrap();
        store
            .connection
            .execute_batch("DROP TABLE enrollment_terminal_status; DROP TABLE enrollment_claim_candidates; PRAGMA user_version = 2;")
            .unwrap();
        drop(store);
        let store = SqliteStore::open(&path).unwrap();
        assert_eq!(store.families().unwrap(), vec![family]);
        assert!(store.archived_enrollment_claims(family).unwrap().is_empty());
        let version: u32 = store
            .connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, 4);
    }

    #[test]
    fn legacy_first_invite_keeps_exact_bytes_after_v4_upgrade() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("legacy-invite.db");
        let mut store = SqliteStore::open(&path).unwrap();
        let family = store.create_family(v4(1), v4(2)).unwrap();
        let row = InviteIssueRow {
            family,
            invitation_id: v4(3),
            transition_id: v4(4),
            object_id: v4(5),
            object_bytes: vec![0x81, 0x01],
            candidate_bytes: vec![0x82, 0x02, 0x03],
            secret_nonce: [6; 24],
            secret_ciphertext: vec![0x84, 0x04],
        };
        store
            .connection
            .execute(
                "INSERT INTO first_invite_issues
             (family_id,device_id,invitation_id,transition_id,object_id,
              object_bytes,candidate_bytes,secret_nonce,secret_ciphertext)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",
                params![
                    family.family_id.as_slice(),
                    family.device_id.as_slice(),
                    row.invitation_id.as_slice(),
                    row.transition_id.as_slice(),
                    row.object_id.as_slice(),
                    &row.object_bytes,
                    &row.candidate_bytes,
                    row.secret_nonce.as_slice(),
                    &row.secret_ciphertext,
                ],
            )
            .unwrap();
        store
            .connection
            .execute_batch("DROP TABLE enrollment_terminal_status; DROP TABLE enrollment_claim_candidates; DROP TABLE invite_issues; PRAGMA user_version = 1;")
            .unwrap();
        drop(store);

        let store = SqliteStore::open(&path).unwrap();
        let migrated = store.first_invite_issue(family).unwrap().unwrap();
        assert_eq!(migrated.invitation_id, row.invitation_id);
        assert_eq!(migrated.transition_id, row.transition_id);
        assert_eq!(migrated.object_id, row.object_id);
        assert_eq!(migrated.object_bytes, row.object_bytes);
        assert_eq!(migrated.candidate_bytes, row.candidate_bytes);
        assert_eq!(migrated.secret_nonce, row.secret_nonce);
        assert_eq!(migrated.secret_ciphertext, row.secret_ciphertext);
        let version: u32 = store
            .connection
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap();
        assert_eq!(version, 4);
        let later = InviteIssueRow {
            family,
            invitation_id: v4(7),
            transition_id: v4(8),
            object_id: v4(9),
            object_bytes: vec![0x81, 0x07],
            candidate_bytes: vec![0x82, 0x08, 0x09],
            secret_nonce: [10; 24],
            secret_ciphertext: vec![0x84, 0x0a],
        };
        let mut store = store;
        store.save_later_invite_issue(&later).unwrap();
        assert_eq!(
            store
                .first_invite_issue(family)
                .unwrap()
                .unwrap()
                .invitation_id,
            row.invitation_id
        );
        assert_eq!(
            store
                .invite_issue(family, later.invitation_id)
                .unwrap()
                .unwrap()
                .candidate_bytes,
            later.candidate_bytes
        );
        drop(store);
        let reopened = SqliteStore::open(&path).unwrap();
        assert!(reopened.first_invite_issue(family).unwrap().is_some());
        assert!(
            reopened
                .invite_issue(family, later.invitation_id)
                .unwrap()
                .is_some()
        );
    }

    fn v4(fill: u8) -> [u8; 16] {
        let mut id = [fill; 16];
        id[6] = 0x40;
        id[8] = 0x80;
        id
    }
}

#[allow(dead_code)] // Used by the staged-batch session path above.
fn load_pending(
    connection: &Connection,
    family: FamilyHandle,
) -> Result<Option<PreparedBatch>, Error> {
    connection
        .query_row(
            "SELECT from_index, to_index, sequence, batch_id, envelope_bytes, object_hash
         FROM local_outbox WHERE family_id = ?1",
            [family.family_id.as_slice()],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, Vec<u8>>(3)?,
                    row.get::<_, Vec<u8>>(4)?,
                    row.get::<_, Vec<u8>>(5)?,
                ))
            },
        )
        .optional()?
        .map(|row| {
            Ok(PreparedBatch {
                from_index: row.0.try_into().map_err(|_| Error::CorruptState)?,
                to_index: row.1.try_into().map_err(|_| Error::CorruptState)?,
                sequence: row.2.try_into().map_err(|_| Error::CorruptState)?,
                batch_id: row.3.try_into().map_err(|_| Error::CorruptState)?,
                envelope_bytes: row.4,
                object_hash: row.5.try_into().map_err(|_| Error::CorruptState)?,
            })
        })
        .transpose()
}

fn checked_family(
    connection: &Connection,
    family: FamilyHandle,
) -> Result<(i64, Option<Hlc>), Error> {
    let row = connection
        .query_row(
            "SELECT device_id, last_index, last_hlc_wall, last_hlc_counter
             FROM families WHERE family_id = ?1",
            [family.family_id.as_slice()],
            |row| {
                Ok((
                    row.get::<_, Vec<u8>>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, Option<i64>>(2)?,
                    row.get::<_, Option<i64>>(3)?,
                ))
            },
        )
        .optional()?
        .ok_or(Error::MissingFamily)?;
    if row.0.as_slice() != family.device_id {
        return Err(Error::WrongDevice);
    }
    let previous = match (row.2, row.3) {
        (None, None) => None,
        (Some(wall_ms), Some(counter)) => Some(Hlc {
            wall_ms,
            counter: counter.try_into().map_err(|_| Error::CorruptState)?,
            device_id: family.device_id,
        }),
        _ => return Err(Error::CorruptState),
    };
    Ok((row.1, previous))
}

fn load_projection(
    connection: &Connection,
    family: FamilyHandle,
) -> Result<LocalProjection, Error> {
    let mut projection = LocalProjection::new(family.family_id);
    let mut statement = connection.prepare(
        "SELECT append_index, operation_bytes FROM local_operations
         WHERE family_id = ?1 ORDER BY append_index",
    )?;
    let rows = statement.query_map([family.family_id.as_slice()], |row| {
        Ok((row.get::<_, i64>(0)?, row.get::<_, Vec<u8>>(1)?))
    })?;
    for row in rows {
        let (index, bytes) = row?;
        let operation = Operation::decode_bound(&bytes, &family.family_id, &family.device_id)?;
        projection.append(
            &operation,
            index.try_into().map_err(|_| Error::CorruptState)?,
        )?;
    }
    Ok(projection)
}

#[cfg(test)]
mod outbox_tests {
    use super::*;
    use crate::operation::{Hlc, Kind, Scope};

    fn v4(suffix: u8) -> [u8; 16] {
        let mut id = [0u8; 16];
        id[6] = 0x40;
        id[8] = 0x80;
        id[15] = suffix;
        id
    }

    fn v7(suffix: u8) -> [u8; 16] {
        let mut id = [0u8; 16];
        id[6] = 0x70;
        id[8] = 0x80;
        id[15] = suffix;
        id
    }

    #[test]
    fn staged_batch_retries_exact_bytes_after_reopen_and_does_not_expose_failed_write() {
        let path = std::env::temp_dir().join(format!(
            "babytrack-outbox-{}-{}.sqlite",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let family = FamilyHandle {
            family_id: v4(1),
            device_id: v4(2),
        };
        let operation = NewOperation {
            family_id: family.family_id,
            operation_id: v7(1),
            record_id: v7(2),
            scope: Scope::Child,
            kind: Kind::Create,
            author_device_id: family.device_id,
            hlc: Hlc {
                wall_ms: 0,
                counter: 0,
                device_id: family.device_id,
            },
            record_type: Some("child".to_owned()),
            child_id: None,
            fields: Some(vec![(1, Value::Text("Baby".to_owned()))]),
        };
        let relay = [3u8; 32];
        let head = [4u8; 32];
        let key = [5u8; 32];
        let seed = [6u8; 32];
        let first;
        {
            let mut store = SqliteStore::open(&path).unwrap();
            store
                .create_family(family.family_id, family.device_id)
                .unwrap();
            assert!(matches!(
                store.stage_next_batch(family, relay, head, 1, &key, &seed),
                Err(Error::NoUnsentOperation)
            ));
            store.append_local(family, operation.clone(), 100).unwrap();
            first = store
                .stage_next_batch(family, relay, head, 1, &key, &seed)
                .unwrap();
            assert_eq!(first.from_index, 1);
            assert_eq!(first.sequence, 1);
            assert!(ids::is_v4(&first.batch_id));
            assert_eq!(store.pending_batch(family).unwrap(), Some(first.clone()));
            let signer = crate::crypto::signing_public_key(&seed);
            assert!(
                batch::open_verified(
                    &first.envelope_bytes,
                    &family.family_id,
                    &relay,
                    &key,
                    &signer
                )
                .is_ok()
            );
            let mut invalid = operation.clone();
            invalid.operation_id = v7(3);
            invalid.record_id = v7(4);
            invalid.kind = Kind::Set;
            invalid.record_type = None;
            assert!(matches!(
                store.append_local(family, invalid, 101),
                Err(Error::Projection(_))
            ));
        }
        {
            let mut store = SqliteStore::open(&path).unwrap();
            assert_eq!(store.pending_batch(family).unwrap(), Some(first.clone()));
            // A changed head/key must not silently re-encrypt an uncertain batch.
            let retry = store
                .stage_next_batch(family, relay, [8u8; 32], 2, &[9u8; 32], &seed)
                .unwrap();
            assert_eq!(retry, first);
            assert_eq!(store.load_local(family).unwrap().last_append_index(), 1);
        }
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(path.with_extension("sqlite-wal"));
        let _ = std::fs::remove_file(path.with_extension("sqlite-shm"));
    }
}
