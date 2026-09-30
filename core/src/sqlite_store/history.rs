//! History persistence. Each method retains its complete transaction.

use super::*;

impl SqliteStore {
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
}
