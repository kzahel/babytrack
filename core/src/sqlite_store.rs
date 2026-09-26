//! Native local-only journal. Accepted shared entries and outbox persistence
//! extend this schema as the sync client is implemented.

use std::path::Path;

use rusqlite::{Connection, OptionalExtension, params};

use crate::{
    batch::{self, Header},
    cbor::{self, Value},
    hlc::{self, Clock},
    ids,
    operation::{self, Hlc, NewOperation, Operation},
    projection::{LocalError, LocalProjection},
};

#[derive(Debug)]
pub enum Error {
    Sqlite(rusqlite::Error),
    Operation(operation::Error),
    Clock(hlc::Error),
    Projection(LocalError),
    InvalidId,
    WrongFamily,
    WrongDevice,
    MissingFamily,
    DuplicateOperation,
    AppendIndexOverflow,
    CorruptState,
    NoUnsentOperation,
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

pub struct SqliteStore {
    connection: Connection,
}

impl SqliteStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, Error> {
        let connection = Connection::open(path)?;
        connection.execute_batch(
            "PRAGMA foreign_keys = ON;
             PRAGMA journal_mode = WAL;
             CREATE TABLE IF NOT EXISTS families (
               family_id BLOB PRIMARY KEY CHECK(length(family_id) = 16),
               device_id BLOB NOT NULL CHECK(length(device_id) = 16),
               last_index INTEGER NOT NULL DEFAULT 0 CHECK(last_index >= 0),
               last_hlc_wall INTEGER,
               last_hlc_counter INTEGER,
               CHECK ((last_hlc_wall IS NULL) = (last_hlc_counter IS NULL))
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
             INSERT OR IGNORE INTO local_sync_state(family_id)
               SELECT family_id FROM families;",
        )?;
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
