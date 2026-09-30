//! Outbox persistence. Each method retains its complete transaction.

use super::*;

impl SqliteStore {
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
    /// verification is owned by the core Family session.
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
}

pub(super) fn load_pending(
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
