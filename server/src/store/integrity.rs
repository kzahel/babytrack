//! Integrity helpers shared inside the relay store.

use super::*;

/// Signed private checkpoint for facts that are intentionally absent from the
/// public log. It detects selective row loss or substitution on reopen; a
/// coherent rollback of the whole SQLite file remains the v1 trust limit.
pub(super) fn private_state_digest(db: &Connection) -> Result<[u8; 32], Error> {
    let mut reservations = Vec::new();
    let mut query = db.prepare(
        "SELECT family_id,object_id,kind,object_hash FROM object_reservations ORDER BY family_id,object_id",
    )?;
    let rows = query.query_map([], |row| {
        Ok((
            row.get::<_, Vec<u8>>(0)?,
            row.get::<_, Vec<u8>>(1)?,
            row.get::<_, i64>(2)?,
            row.get::<_, Vec<u8>>(3)?,
        ))
    })?;
    for row in rows {
        let (family, id, kind, hash) = row?;
        let family: [u8; 16] = family
            .try_into()
            .map_err(|_| Error::Invalid("private reservation Family length"))?;
        let id: [u8; 16] = id
            .try_into()
            .map_err(|_| Error::Invalid("private reservation ID length"))?;
        let hash: [u8; 32] = hash
            .try_into()
            .map_err(|_| Error::Invalid("private reservation hash length"))?;
        let kind: u16 = kind
            .try_into()
            .map_err(|_| Error::Invalid("private reservation kind range"))?;
        reservations.push(Value::Array(vec![
            Value::Bytes(family.to_vec()),
            Value::Bytes(id.to_vec()),
            Value::Integer(kind.into()),
            Value::Bytes(hash.to_vec()),
        ]));
    }
    let mut rejections = Vec::new();
    let mut query = db.prepare(
        "SELECT family_id,batch_id,envelope_bytes,receipt_bytes FROM rejected_batch_results ORDER BY family_id,batch_id",
    )?;
    let rows = query.query_map([], |row| {
        Ok((
            row.get::<_, Vec<u8>>(0)?,
            row.get::<_, Vec<u8>>(1)?,
            row.get::<_, Vec<u8>>(2)?,
            row.get::<_, Vec<u8>>(3)?,
        ))
    })?;
    for row in rows {
        let (family, id, envelope, receipt) = row?;
        let family: [u8; 16] = family
            .try_into()
            .map_err(|_| Error::Invalid("private rejection Family length"))?;
        let id: [u8; 16] = id
            .try_into()
            .map_err(|_| Error::Invalid("private rejection ID length"))?;
        rejections.push(Value::Array(vec![
            Value::Bytes(family.to_vec()),
            Value::Bytes(id.to_vec()),
            Value::Bytes(crypto::hash("private-envelope", &envelope)?.to_vec()),
            Value::Bytes(crypto::hash("private-receipt", &receipt)?.to_vec()),
        ]));
    }
    Ok(crypto::hash(
        "relay-private-state",
        &cbor::encode(&Value::Array(vec![
            Value::Integer(1),
            Value::Array(reservations),
            Value::Array(rejections),
        ]))?,
    )?)
}

pub(super) fn private_checkpoint_body(digest: [u8; 32]) -> Result<Vec<u8>, Error> {
    Ok(cbor::encode(&Value::Array(vec![
        Value::Integer(1),
        Value::Bytes(digest.to_vec()),
    ]))?)
}

pub(super) fn verify_private_integrity(
    db: &Connection,
    relay_public: [u8; 32],
) -> Result<(), Error> {
    let (saved_digest, signature): (Vec<u8>, Vec<u8>) = db.query_row(
        "SELECT digest,signature FROM private_integrity WHERE singleton=1",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    let saved_digest: [u8; 32] = saved_digest
        .try_into()
        .map_err(|_| Error::Invalid("private checkpoint digest length"))?;
    let signature: [u8; 64] = signature
        .try_into()
        .map_err(|_| Error::Invalid("private checkpoint signature length"))?;
    crypto::verify_cbor(
        "relay-private-checkpoint",
        &private_checkpoint_body(saved_digest)?,
        &relay_public,
        &signature,
    )?;
    if private_state_digest(db)? != saved_digest {
        return Err(Error::Invalid("private checkpoint differs from saved rows"));
    }
    Ok(())
}

pub(super) fn refresh_private_integrity(
    db: &Connection,
    relay_seed: &[u8; 32],
) -> Result<(), Error> {
    let digest = private_state_digest(db)?;
    let signature = crypto::sign_cbor(
        "relay-private-checkpoint",
        &private_checkpoint_body(digest)?,
        relay_seed,
    )?;
    let updated = db.execute(
        "UPDATE private_integrity SET digest=?1,signature=?2 WHERE singleton=1",
        params![&digest[..], &signature[..]],
    )?;
    if updated != 1 {
        return Err(Error::Invalid("private checkpoint missing"));
    }
    Ok(())
}

pub(super) fn initialize_private_integrity(
    db: &Connection,
    relay_seed: &[u8; 32],
    relay_public: [u8; 32],
    allow_legacy_checkpoint: bool,
) -> Result<(), Error> {
    let exists: Option<i64> = db
        .query_row(
            "SELECT 1 FROM private_integrity WHERE singleton=1",
            [],
            |row| row.get(0),
        )
        .optional()?;
    if exists.is_none() {
        // A pre-checkpoint development database must be upgraded deliberately,
        // rather than silently trusting its potentially incomplete private
        // reservation history.
        let prior: i64 = db.query_row(
            "SELECT
              (SELECT COUNT(*) FROM families)
              +(SELECT COUNT(*) FROM entries)
              +(SELECT COUNT(*) FROM staged_objects)
              +(SELECT COUNT(*) FROM staged_controls)
              +(SELECT COUNT(*) FROM staged_control_objects)
              +(SELECT COUNT(*) FROM committed_objects)
              +(SELECT COUNT(*) FROM batch_results)
              +(SELECT COUNT(*) FROM object_reservations)
              +(SELECT COUNT(*) FROM rejected_batch_results)
              +(SELECT COUNT(*) FROM read_requests)",
            [],
            |row| row.get(0),
        )?;
        if prior != 0 && !allow_legacy_checkpoint {
            return Err(Error::Invalid(
                "legacy relay needs private checkpoint migration",
            ));
        }
        let digest = private_state_digest(db)?;
        let signature = crypto::sign_cbor(
            "relay-private-checkpoint",
            &private_checkpoint_body(digest)?,
            relay_seed,
        )?;
        db.execute(
            "INSERT INTO private_integrity(singleton,digest,signature) VALUES(1,?1,?2)",
            params![&digest[..], &signature[..]],
        )?;
    }
    verify_private_integrity(db, relay_public)
}

impl RelayStore {
    pub(super) fn verify_uncommitted_reservations(&self) -> Result<(), Error> {
        let mut rows = self.db.prepare(
            "SELECT r.family_id,r.object_id FROM object_reservations r
             LEFT JOIN committed_objects o USING(family_id,object_id)
             WHERE o.object_id IS NULL ORDER BY r.family_id,r.object_id",
        )?;
        let mut query = rows.query([])?;
        let mut family_ids = BTreeMap::new();
        while let Some(row) = query.next()? {
            let family: [u8; 16] = row
                .get::<_, Vec<u8>>(0)?
                .try_into()
                .map_err(|_| Error::Invalid("stored reservation Family ID length"))?;
            let object_id: [u8; 16] = row
                .get::<_, Vec<u8>>(1)?
                .try_into()
                .map_err(|_| Error::Invalid("stored reservation object ID length"))?;
            if let std::collections::btree_map::Entry::Vacant(entry) = family_ids.entry(family) {
                entry.insert(committed_protocol_ids(&self.db, family)?);
            }
            if family_ids[&family].contains(&object_id) {
                return Err(Error::Invalid(
                    "uncommitted object reservation overlaps public ID",
                ));
            }
        }
        Ok(())
    }

    /// Reject a damaged durable log before serving a reopened relay. Derived
    /// authority rows must never outrank the signed control and receipt chain.
    pub(super) fn verify_saved_families(&self) -> Result<(), Error> {
        let snapshot = self.db.unchecked_transaction()?;
        let mut families = snapshot.prepare("SELECT family_id FROM families WHERE active=1")?;
        let rows = families.query_map([], |row| row.get::<_, Vec<u8>>(0))?;
        for row in rows {
            let family: [u8; 16] = row?
                .try_into()
                .map_err(|_| Error::Invalid("stored Family ID length"))?;
            Self::verify_saved_family(&snapshot, family, self.relay_public, &self.relay_seed)?;
        }
        drop(families);
        snapshot.commit()?;
        Ok(())
    }

    /// Reconstruct one Family inside the caller's read or write transaction.
    /// The returned ledger is derived only from authenticated committed bytes.
    pub(super) fn verify_saved_family(
        snapshot: &Connection,
        family: [u8; 16],
        relay_public: [u8; 32],
        relay_seed: &[u8; 32],
    ) -> Result<public_ledger::PublicLedger, Error> {
        let (candidate, saved_head, saved_cursor): (Vec<u8>, Vec<u8>, i64) = snapshot.query_row(
            "SELECT candidate_bytes,head_hash,cursor FROM families WHERE family_id=?1 AND active=1",
            params![&family[..]],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
        let genesis = verify_stored_genesis(snapshot, family, &candidate, relay_public)?;
        let mut entries = snapshot.prepare(
            "SELECT cursor,kind,committed_bytes FROM entries WHERE family_id=?1 ORDER BY cursor",
        )?;
        let mut log = entries.query(params![&family[..]])?;
        let mut cursor = 0u64;
        let mut head = [0u8; 32];
        let mut control_count = 0u64;
        let mut batch_count = 0u64;
        let mut object_ids = BTreeSet::new();
        let mut historical_heads = BTreeMap::new();
        let mut ledger: Option<public_ledger::PublicLedger> = None;
        while let Some(row) = log.next()? {
            let position: i64 = row.get(0)?;
            let kind: i64 = row.get(1)?;
            let bytes: Vec<u8> = row.get(2)?;
            cursor = cursor
                .checked_add(1)
                .ok_or(Error::Invalid("stored cursor overflow"))?;
            if u64::try_from(position).ok() != Some(cursor) {
                return Err(Error::Invalid("stored log cursor gap"));
            }
            match kind {
                1 => {
                    let receipt = receipt::verify_control_receipt(&bytes, &relay_public)?;
                    if receipt.family_id != family
                        || receipt.relay_id != genesis.relay_id
                        || receipt.cursor != cursor
                        || receipt.parent_head != head
                    {
                        return Err(Error::Invalid("stored control chain differs"));
                    }
                    for object in &receipt.manifest {
                        if !object_ids.insert(object.id) {
                            return Err(Error::Invalid("stored object ID repeated"));
                        }
                        let saved: Option<StoredCommittedObject> = snapshot
                                .query_row(
                                    "SELECT kind,object_hash,object_bytes,transition_id FROM committed_objects WHERE family_id=?1 AND object_id=?2",
                                    params![&family[..], &object.id[..]],
                                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
                                )
                                .optional()?;
                        let Some((kind, hash, bytes, transition)) = saved else {
                            return Err(Error::Invalid("committed manifest object missing"));
                        };
                        if kind != i64::from(object.kind)
                            || hash != object.hash
                            || bytes.len() != object.len as usize
                            || crypto::hash("object", &bytes)? != object.hash
                            || transition != receipt.transition_id
                        {
                            return Err(Error::Invalid("committed manifest object differs"));
                        }
                    }
                    let next_head = crypto::hash("control-head", &bytes)?;
                    if control_count == 0 {
                        ledger = Some(public_ledger::PublicLedger::from_genesis(
                            &genesis, &receipt, &bytes,
                        )?);
                    } else {
                        ledger
                            .as_mut()
                            .ok_or(Error::Invalid("public ledger absent"))?
                            .apply(&receipt, &bytes)?;
                    }
                    head = next_head;
                    control_count += 1;
                }
                2 => {
                    let saved: Option<StoredBatchResult> = snapshot
                            .query_row(
                                "SELECT envelope_bytes,receipt_bytes,batch_id,author_id,sequence FROM batch_results WHERE family_id=?1 AND cursor=?2",
                                params![&family[..], position],
                                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
                            )
                            .optional()?;
                    let Some((envelope, signed_result, batch_id, author_id, sequence)) = saved
                    else {
                        return Err(Error::Invalid("stored batch result missing"));
                    };
                    if envelope != bytes {
                        return Err(Error::Invalid("stored batch differs from log"));
                    }
                    let verified = receipt::verify_stored_accepted_batch(
                        &bytes,
                        &signed_result,
                        family,
                        genesis.relay_id,
                        cursor,
                        relay_seed,
                    )?;
                    if batch_id != verified.batch_id
                        || author_id != verified.author_id
                        || u64::try_from(sequence).ok() != Some(verified.sequence)
                    {
                        return Err(Error::Invalid("stored batch result metadata differs"));
                    }
                    ledger
                        .as_mut()
                        .ok_or(Error::Invalid("batch before genesis"))?
                        .apply_batch(&bytes, &verified)?;
                    batch_count += 1;
                }
                _ => return Err(Error::Invalid("stored log kind")),
            }
            historical_heads.insert(cursor, head);
        }
        if control_count == 0
            || i64::try_from(cursor).ok() != Some(saved_cursor)
            || saved_head != head
            || ledger.as_ref().map(public_ledger::PublicLedger::head) != Some(head)
        {
            return Err(Error::Invalid("stored Family head or cursor differs"));
        }
        let committed_count: i64 = snapshot.query_row(
            "SELECT COUNT(*) FROM committed_objects WHERE family_id=?1",
            params![&family[..]],
            |row| row.get(0),
        )?;
        if usize::try_from(committed_count).ok() != Some(object_ids.len()) {
            return Err(Error::Invalid("committed objects outside signed manifests"));
        }
        let saved_batches: i64 = snapshot.query_row(
            "SELECT COUNT(*) FROM batch_results WHERE family_id=?1",
            params![&family[..]],
            |row| row.get(0),
        )?;
        if u64::try_from(saved_batches).ok() != Some(batch_count) {
            return Err(Error::Invalid("batch results outside committed log"));
        }
        let ledger = ledger.ok_or(Error::Invalid("public ledger absent"))?;
        let mut rejected = snapshot.prepare(
            "SELECT batch_id,envelope_bytes,receipt_bytes FROM rejected_batch_results WHERE family_id=?1",
        )?;
        let rows = rejected.query_map(params![&family[..]], |row| {
            Ok((
                row.get::<_, Vec<u8>>(0)?,
                row.get::<_, Vec<u8>>(1)?,
                row.get::<_, Vec<u8>>(2)?,
            ))
        })?;
        for row in rows {
            let (id, envelope, signed_result) = row?;
            let parsed = receipt::verify_stored_rejected_batch(
                &envelope,
                &signed_result,
                family,
                genesis.relay_id,
                &relay_public,
            )?;
            if id != parsed.batch_id
                || ledger.contains_id(&parsed.batch_id)
                || historical_heads.get(&parsed.cursor) != Some(&parsed.control_head)
            {
                return Err(Error::Invalid("stored rejected result differs"));
            }
        }
        Ok(ledger)
    }
}
