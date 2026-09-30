//! Journal persistence. Each method retains its complete transaction.

use super::*;

impl SqliteStore {
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
}

pub(super) fn checked_family(
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
