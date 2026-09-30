//! Validation helpers shared inside the relay store.

use super::*;

pub(super) fn validate_general_grant_object(
    candidate: &[u8],
    state: &Value,
    object_kind: u16,
    object_id: [u8; 16],
    bytes: &[u8],
) -> Result<Option<[u8; 16]>, Error> {
    if object_kind != 4 {
        return Ok(None);
    }
    let Value::Map(root) = cbor::decode(candidate)? else {
        return Err(Error::Invalid("grant candidate not map"));
    };
    let Some((1, Value::Map(unsigned))) = root.first() else {
        return Err(Error::Invalid("grant unsigned transition absent"));
    };
    let Value::Map(delta) = &unsigned[6].1 else {
        return Err(Error::Invalid("grant delta not map"));
    };
    let kind = number(&unsigned[5].1)?;
    let core_hash = fixed::<32>(&unsigned[10].1)?;
    let Value::Map(state) = state else {
        return Err(Error::Invalid("grant state not map"));
    };
    let Value::Array(active) = &state[4].1 else {
        return Err(Error::Invalid("grant active state not array"));
    };
    let Value::Array(pending) = &state[5].1 else {
        return Err(Error::Invalid("grant pending state not array"));
    };
    let (purpose, recipient, key_version) = match kind {
        6 => {
            let recipient = fixed::<16>(&delta[1].1)?;
            let row = pending
                .iter()
                .find_map(|row| match row {
                    Value::Array(parts) if fixed::<16>(&parts[1]).ok() == Some(recipient) => {
                        Some(parts)
                    }
                    _ => None,
                })
                .ok_or(Error::Invalid("admission grant recipient not pending"))?;
            (1, recipient, number(&row[4])?)
        }
        8 => {
            let removed = fixed::<16>(&delta[0].1)?;
            let Value::Map(object) = cbor::decode(bytes)? else {
                return Err(Error::Invalid("rotation grant not map"));
            };
            let Some((4, recipient_value)) = object.get(3) else {
                return Err(Error::Invalid("rotation grant recipient absent"));
            };
            let recipient = fixed::<16>(recipient_value)?;
            if recipient == removed {
                return Err(Error::Invalid("rotation grant addressed to removed device"));
            }
            let row = active
                .iter()
                .find_map(|row| match row {
                    Value::Array(parts) if fixed::<16>(&parts[0]).ok() == Some(recipient) => {
                        Some(parts)
                    }
                    _ => None,
                })
                .ok_or(Error::Invalid("rotation grant recipient not active"))?;
            (2, recipient, number(&row[3])?)
        }
        10 => {
            let recipient = fixed::<16>(&delta[0].1)?;
            let row = active
                .iter()
                .find_map(|row| match row {
                    Value::Array(parts) if fixed::<16>(&parts[0]).ok() == Some(recipient) => {
                        Some(parts)
                    }
                    _ => None,
                })
                .ok_or(Error::Invalid("repair grant recipient not active"))?;
            (3, recipient, number(&row[3])?)
        }
        _ => return Err(Error::Invalid("grant object in wrong control")),
    };
    public_authority::validate_public_grant(
        bytes,
        object_id,
        purpose,
        recipient,
        key_version
            .try_into()
            .map_err(|_| Error::Invalid("grant key version range"))?,
        core_hash,
    )
    .map_err(|_| Error::Invalid("grant public context differs"))?;
    Ok(Some(recipient))
}

pub(super) fn validate_first_removal_object(
    kind: u16,
    object_id: [u8; 16],
    object_bytes: &[u8],
    candidate_bytes: &[u8],
    genesis: &authority::GenesisCandidate,
) -> Result<(), Error> {
    let Value::Map(candidate) = cbor::decode(candidate_bytes)? else {
        return Err(Error::Invalid("removal candidate not map"));
    };
    let Value::Map(unsigned) = &candidate[0].1 else {
        return Err(Error::Invalid("removal unsigned not map"));
    };
    let core_hash = fixed::<32>(&unsigned[10].1)?;
    let Value::Map(fields) = cbor::decode(object_bytes)? else {
        return Err(Error::Invalid("removal object not map"));
    };
    if fields
        .iter()
        .enumerate()
        .any(|(i, (key, _))| *key != i as u64 + 1)
    {
        return Err(Error::Invalid("removal object keys"));
    }
    match kind {
        1 | 5 => {
            if fields.len() != 3
                || number(&fields[0].1)? != 1
                || !matches!(&fields[1].1, Value::Bytes(bytes) if bytes.len() == 24)
                || !matches!(&fields[2].1, Value::Bytes(bytes) if bytes.len() >= 16)
            {
                return Err(Error::Invalid("removal ciphertext object invalid"));
            }
        }
        4 => {
            let Value::Array(manager) = &genesis.manager_row else {
                return Err(Error::Invalid("manager row not array"));
            };
            if fields.len() != 9
                || number(&fields[0].1)? != 1
                || fixed::<16>(&fields[1].1)? != object_id
                || number(&fields[2].1)? != 2
                || fixed::<16>(&fields[3].1)? != genesis.manager_id
                || number(&fields[4].1)? != number(&manager[3])?
                || fields[5].1
                    != Value::Array(vec![
                        Value::Integer(32),
                        Value::Integer(1),
                        Value::Integer(3),
                    ])
                || fixed::<32>(&fields[6].1)? != core_hash
                || !matches!(&fields[7].1, Value::Bytes(bytes) if bytes.len() == 32)
                || !matches!(&fields[8].1, Value::Bytes(bytes) if bytes.len() >= 16)
            {
                return Err(Error::Invalid(
                    "rotation grant is not for remaining manager",
                ));
            }
        }
        _ => return Err(Error::Invalid("unexpected removal object kind")),
    }
    Ok(())
}

pub(super) fn ensure_first_removal_ids_unused(
    db: &Connection,
    family: [u8; 16],
    removal: &authority::FirstRemovalCandidate,
) -> Result<(), Error> {
    let mut prior_control_ids = Vec::new();
    for ordinal in 0..6 {
        let committed = control_at(db, family, ordinal)?;
        let Value::Map(root) = cbor::decode(&committed)? else {
            return Err(Error::Invalid("committed control not map"));
        };
        let Value::Map(unsigned) = &root[0].1 else {
            return Err(Error::Invalid("committed unsigned not map"));
        };
        let prior_id = fixed::<16>(&unsigned[4].1)?;
        if prior_id == removal.transition_id {
            return Err(Error::Invalid("removal transition ID reused"));
        }
        prior_control_ids.push(prior_id);
    }
    let reused_batch: Option<i64> = db
        .query_row(
            "SELECT 1 FROM batch_results WHERE family_id=?1 AND batch_id=?2",
            params![&family[..], &removal.transition_id[..]],
            |r| r.get(0),
        )
        .optional()?;
    if reused_batch.is_some() {
        return Err(Error::Invalid("removal transition ID reused as batch"));
    }
    let reused_object: Option<i64> = db
        .query_row(
            "SELECT 1 FROM committed_objects WHERE family_id=?1 AND object_id=?2",
            params![&family[..], &removal.transition_id[..]],
            |r| r.get(0),
        )
        .optional()?;
    if reused_object.is_some() {
        return Err(Error::Invalid("removal transition ID reused as object"));
    }
    let mut new_ids = BTreeSet::new();
    for entry in &removal.manifest {
        let reused_object: Option<i64> = db
            .query_row(
                "SELECT 1 FROM committed_objects WHERE family_id=?1 AND object_id=?2",
                params![&family[..], &entry.object_id[..]],
                |r| r.get(0),
            )
            .optional()?;
        let reused_batch: Option<i64> = db
            .query_row(
                "SELECT 1 FROM batch_results WHERE family_id=?1 AND batch_id=?2",
                params![&family[..], &entry.object_id[..]],
                |r| r.get(0),
            )
            .optional()?;
        if !new_ids.insert(entry.object_id)
            || reused_object.is_some()
            || reused_batch.is_some()
            || prior_control_ids.contains(&entry.object_id)
            || entry.object_id == removal.transition_id
        {
            return Err(Error::Invalid("removal object ID reused"));
        }
    }
    Ok(())
}

pub(super) fn control_candidate(committed: &[u8]) -> Result<Vec<u8>, Error> {
    let value = cbor::decode(committed)?;
    let Value::Map(fields) = value else {
        return Err(Error::Invalid("committed control not map"));
    };
    if fields.len() != 4
        || fields
            .iter()
            .enumerate()
            .any(|(i, (key, _))| *key != i as u64 + 1)
    {
        return Err(Error::Invalid("committed control keys"));
    }
    Ok(cbor::encode(&Value::Map(vec![
        (1, fields[0].1.clone()),
        (2, fields[1].1.clone()),
    ]))?)
}

/// New protocol identities in one control. References to an existing
/// invitation or device are intentionally excluded.
pub(super) fn control_birth_ids(bytes: &[u8]) -> Result<Vec<[u8; 16]>, Error> {
    let Value::Map(root) = cbor::decode_with_limits(
        bytes,
        cbor::Limits {
            max_bytes: 1024 * 1024,
            max_depth: 16,
        },
    )?
    else {
        return Err(Error::Invalid("control ID source not map"));
    };
    let Some((1, Value::Map(unsigned))) = root.first() else {
        return Err(Error::Invalid("control ID unsigned absent"));
    };
    let field = |index: usize| -> Result<&Value, Error> {
        unsigned
            .get(index)
            .map(|(_, value)| value)
            .ok_or(Error::Invalid("control ID field absent"))
    };
    let mut ids = vec![fixed::<16>(field(4)?)?];
    let Value::Array(manifest) = field(9)? else {
        return Err(Error::Invalid("control ID manifest not array"));
    };
    for entry in manifest {
        let Value::Array(parts) = entry else {
            return Err(Error::Invalid("control ID manifest entry not array"));
        };
        ids.push(fixed::<16>(
            parts
                .get(1)
                .ok_or(Error::Invalid("control ID object absent"))?,
        )?);
    }
    let Value::Map(delta) = field(6)? else {
        return Err(Error::Invalid("control ID delta not map"));
    };
    let extra = match number(field(5)?)? {
        1 => {
            let Value::Array(manager) = delta
                .first()
                .ok_or(Error::Invalid("genesis manager absent"))?
                .1
                .clone()
            else {
                return Err(Error::Invalid("genesis manager not array"));
            };
            Some(fixed::<16>(
                manager
                    .first()
                    .ok_or(Error::Invalid("manager device ID absent"))?,
            )?)
        }
        2 => Some(fixed::<16>(
            &delta
                .first()
                .ok_or(Error::Invalid("invitation ID absent"))?
                .1,
        )?),
        4 => Some(fixed::<16>(
            &delta
                .get(1)
                .ok_or(Error::Invalid("claim device ID absent"))?
                .1,
        )?),
        11 => Some(fixed::<16>(
            &delta.get(2).ok_or(Error::Invalid("challenge ID absent"))?.1,
        )?),
        _ => None,
    };
    if let Some(id) = extra {
        ids.push(id)
    }
    Ok(ids)
}

/// Existing committed controls and durable batch results form the first
/// cohort's per-Family ID registry, including rejected uploads.
pub(super) fn committed_protocol_ids(
    db: &Connection,
    family: [u8; 16],
) -> Result<BTreeSet<[u8; 16]>, Error> {
    let mut seen = BTreeSet::new();
    let mut controls = db.prepare(
        "SELECT committed_bytes FROM entries WHERE family_id=?1 AND kind=1 ORDER BY cursor",
    )?;
    let rows = controls.query_map([&family[..]], |row| row.get::<_, Vec<u8>>(0))?;
    for row in rows {
        for id in control_birth_ids(&row?)? {
            if !seen.insert(id) {
                return Err(Error::Invalid("stored protocol ID collision"));
            }
        }
    }
    for table in ["batch_results", "rejected_batch_results"] {
        let sql = format!("SELECT batch_id FROM {table} WHERE family_id=?1");
        let mut query = db.prepare(&sql)?;
        let rows = query.query_map([&family[..]], |row| row.get::<_, Vec<u8>>(0))?;
        for row in rows {
            let id: [u8; 16] = row?
                .try_into()
                .map_err(|_| Error::Invalid("stored batch ID length"))?;
            if !seen.insert(id) {
                return Err(Error::Invalid("stored protocol ID collision"));
            }
        }
    }
    Ok(seen)
}

pub(super) fn ensure_new_protocol_ids(
    db: &Connection,
    family: [u8; 16],
    candidate_ids: &[[u8; 16]],
    candidate_objects: &[([u8; 16], u16, [u8; 32])],
) -> Result<(), Error> {
    let relay_public: Vec<u8> = db.query_row(
        "SELECT public_key FROM relay_identity WHERE singleton=1",
        [],
        |row| row.get(0),
    )?;
    verify_private_integrity(
        db,
        relay_public
            .try_into()
            .map_err(|_| Error::Invalid("relay public key length"))?,
    )?;
    let mut seen = committed_protocol_ids(db, family)?;
    let mut reservations = db
        .prepare("SELECT object_id,kind,object_hash FROM object_reservations WHERE family_id=?1")?;
    let rows = reservations.query_map([&family[..]], |row| {
        Ok((
            row.get::<_, Vec<u8>>(0)?,
            row.get::<_, i64>(1)?,
            row.get::<_, Vec<u8>>(2)?,
        ))
    })?;
    for row in rows {
        let (id, kind, hash) = row?;
        let id: [u8; 16] = id
            .try_into()
            .map_err(|_| Error::Invalid("stored object reservation ID length"))?;
        let kind: u16 = kind
            .try_into()
            .map_err(|_| Error::Invalid("stored object reservation kind"))?;
        let hash: [u8; 32] = hash
            .try_into()
            .map_err(|_| Error::Invalid("stored object reservation hash length"))?;
        let committed: Option<(i64, Vec<u8>)> = db
            .query_row(
                "SELECT kind,object_hash FROM committed_objects WHERE family_id=?1 AND object_id=?2",
                params![&family[..], &id[..]],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        if let Some((committed_kind, committed_hash)) = committed {
            if committed_kind != i64::from(kind) || committed_hash != hash || !seen.contains(&id) {
                return Err(Error::Invalid("committed object reservation mismatch"));
            }
        } else if let Some((_, expected_kind, expected_hash)) = candidate_objects
            .iter()
            .find(|(object_id, _, _)| *object_id == id)
        {
            if *expected_kind != kind || *expected_hash != hash {
                return Err(Error::Invalid("object reservation differs from candidate"));
            }
        } else if !seen.insert(id) {
            return Err(Error::Invalid("stored protocol ID collision"));
        }
    }
    for id in candidate_ids {
        if !seen.insert(*id) {
            return Err(Error::Invalid("protocol ID reused across categories"));
        }
    }
    Ok(())
}

pub(super) fn ensure_control_ids(
    db: &Connection,
    family: [u8; 16],
    candidate: &[u8],
) -> Result<(), Error> {
    let value = cbor::decode(candidate)?;
    let Value::Map(root) = value else {
        return Err(Error::Invalid("control ID root not map"));
    };
    let Some((1, Value::Map(unsigned))) = root.iter().find(|(key, _)| *key == 1) else {
        return Err(Error::Invalid("control ID unsigned not map"));
    };
    let Some((10, Value::Array(manifest))) = unsigned.iter().find(|(key, _)| *key == 10) else {
        return Err(Error::Invalid("control ID manifest not array"));
    };
    let mut objects = Vec::with_capacity(manifest.len());
    for entry in manifest {
        let Value::Array(parts) = entry else {
            return Err(Error::Invalid("control ID manifest entry not array"));
        };
        let id = parts
            .get(1)
            .ok_or(Error::Invalid("control ID object absent"))?;
        let kind = parts
            .first()
            .ok_or(Error::Invalid("control ID object kind absent"))?;
        let hash = parts
            .get(2)
            .ok_or(Error::Invalid("control ID object hash absent"))?;
        objects.push((
            fixed::<16>(id)?,
            number(kind)?
                .try_into()
                .map_err(|_| Error::Invalid("control ID object kind"))?,
            fixed::<32>(hash)?,
        ));
    }
    ensure_new_protocol_ids(db, family, &control_birth_ids(candidate)?, &objects)
}
