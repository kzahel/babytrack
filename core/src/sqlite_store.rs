//! Native local-only journal. Accepted shared entries and outbox persistence
//! extend this schema as the sync client is implemented.

use std::path::Path;

use rusqlite::{Connection, OptionalExtension, params};

use crate::{
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
             );",
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
        self.connection.execute(
            "INSERT INTO families(family_id, device_id) VALUES (?1, ?2)",
            params![family_id.as_slice(), device_id.as_slice()],
        )?;
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
