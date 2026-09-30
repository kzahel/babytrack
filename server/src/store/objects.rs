//! Objects helpers shared inside the relay store.

use super::*;

pub(super) type StagedObjectParts = (Vec<u8>, u16, [u8; 16], Vec<u8>);
pub(super) fn stage_parts(body: &[u8]) -> Result<StagedObjectParts, Error> {
    let value = cbor::decode_with_limits(
        body,
        cbor::Limits {
            max_bytes: 2 * 1024 * 1024,
            max_depth: 16,
        },
    )?;
    let Value::Map(fields) = value else {
        return Err(Error::Invalid("stage body not map"));
    };
    if fields.len() != 6
        || fields
            .iter()
            .enumerate()
            .any(|(i, (key, _))| *key != i as u64 + 1)
        || fields[0].1 != Value::Integer(1)
    {
        return Err(Error::Invalid("stage keys or version"));
    }
    let candidate = cbor::encode(&Value::Map(vec![
        (1, fields[1].1.clone()),
        (2, fields[2].1.clone()),
    ]))?;
    let kind: u16 = number(&fields[3].1)?
        .try_into()
        .map_err(|_| Error::Invalid("stage kind range"))?;
    let object_id = fixed::<16>(&fields[4].1)?;
    let Value::Bytes(bytes) = &fields[5].1 else {
        return Err(Error::Invalid("stage object not bytes"));
    };
    Ok((candidate, kind, object_id, bytes.clone()))
}

pub(super) fn stage_control_object(
    tx: &Transaction<'_>,
    family: [u8; 16],
    transition: [u8; 16],
    candidate: &[u8],
    entry: &authority::ManifestEntry,
    object_bytes: &[u8],
) -> Result<(), Error> {
    let prior: Option<Vec<u8>> = tx
        .query_row(
            "SELECT candidate_bytes FROM staged_controls WHERE family_id=?1 AND transition_id=?2",
            params![&family[..], &transition[..]],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(prior) = prior {
        if prior != candidate {
            return Err(Error::Invalid(
                "transition ID staged with different candidate",
            ));
        }
    } else {
        tx.execute(
            "INSERT INTO staged_controls(family_id,transition_id,candidate_bytes) VALUES(?1,?2,?3)",
            params![&family[..], &transition[..], candidate],
        )?;
    }
    reserve_staged_object(tx, family, entry.object_id, entry.kind, entry.object_hash)?;
    let prior: Option<Vec<u8>> = tx.query_row(
        "SELECT object_bytes FROM staged_control_objects WHERE family_id=?1 AND transition_id=?2 AND object_id=?3",
        params![&family[..], &transition[..], &entry.object_id[..]],
        |r| r.get(0),
    ).optional()?;
    if let Some(prior) = prior {
        if prior != object_bytes {
            return Err(Error::Invalid(
                "candidate object ID staged with different bytes",
            ));
        }
    } else {
        tx.execute(
            "INSERT INTO staged_control_objects(family_id,transition_id,object_id,kind,object_hash,object_bytes) VALUES(?1,?2,?3,?4,?5,?6)",
            params![&family[..], &transition[..], &entry.object_id[..], i64::from(entry.kind), &entry.object_hash[..], object_bytes],
        )?;
    }
    Ok(())
}

pub(super) fn reserve_staged_object(
    tx: &Transaction<'_>,
    family: [u8; 16],
    object_id: [u8; 16],
    kind: u16,
    hash: [u8; 32],
) -> Result<(), Error> {
    let committed: Option<(i64, Vec<u8>)> = tx
        .query_row(
            "SELECT kind,object_hash FROM committed_objects WHERE family_id=?1 AND object_id=?2",
            params![&family[..], &object_id[..]],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    if let Some((committed_kind, committed_hash)) = committed {
        if committed_kind != i64::from(kind) || committed_hash != hash {
            return Err(Error::Invalid(
                "object ID already committed with other bytes",
            ));
        }
    } else if committed_protocol_ids(tx, family)?.contains(&object_id) {
        return Err(Error::Invalid("object ID already used by public protocol"));
    }
    let existing: Option<(i64, Vec<u8>)> = tx
        .query_row(
            "SELECT kind,object_hash FROM object_reservations WHERE family_id=?1 AND object_id=?2",
            params![&family[..], &object_id[..]],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    if let Some((prior_kind, prior_hash)) = existing {
        if prior_kind != i64::from(kind) || prior_hash != hash {
            return Err(Error::Invalid(
                "object ID reserved with different kind or bytes",
            ));
        }
    } else {
        tx.execute(
            "INSERT INTO object_reservations(family_id,object_id,kind,object_hash) VALUES(?1,?2,?3,?4)",
            params![&family[..], &object_id[..], i64::from(kind), &hash[..]],
        )?;
    }
    Ok(())
}
