//! Portable current-state Family export. Readable files carry no authority.

use serde_json::{Map as JsonMap, Value as Json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
};

use argon2::{Algorithm, Argon2, Params, Version};
use serde::de::{self, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer};
use sha2::{Digest, Sha256};
use unicode_normalization::UnicodeNormalization;
use zeroize::Zeroizing;

use crate::{
    cbor,
    operation::{Hlc, Kind, NewOperation, Scope},
    projection::Record,
    record_validity,
};

#[cfg(not(target_arch = "wasm32"))]
use crate::{
    shared_ready::{self, ReadyFamilySession},
    sqlite_store::{self, FamilyHandle, RestoredOrigin, SqliteStore},
};

#[derive(Debug)]
pub enum Error {
    Cbor(cbor::Error),
    Json(serde_json::Error),
    #[cfg(not(target_arch = "wasm32"))]
    Store(sqlite_store::Error),
    #[cfg(not(target_arch = "wasm32"))]
    Ready(shared_ready::Error),
    Random(getrandom::Error),
    ProtectedFailure,
    Invalid(&'static str),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortableRow {
    pub kind: PortableKind,
    pub source_id: [u8; 16],
    pub source_child_id: Option<[u8; 16]>,
    pub record_type: String,
    pub deleted: bool,
    pub fields: BTreeMap<u64, Vec<u8>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortableKind {
    Child,
    Family,
    Activity,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedBackup {
    pub source_family_id: [u8; 16],
    pub snapshot_utc_ms: i64,
    pub source_cursor: Option<u64>,
    pub known_gap: bool,
    pub rows: Vec<PortableRow>,
}

#[derive(Debug)]
enum StrictJson {
    Null,
    Bool(bool),
    Integer(i128),
    String(String),
    Object(BTreeMap<String, StrictJson>),
}

impl<'de> Deserialize<'de> for StrictJson {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct StrictVisitor;
        impl<'de> Visitor<'de> for StrictVisitor {
            type Value = StrictJson;
            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("JSON with unique keys and integer numbers")
            }
            fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
                Ok(StrictJson::Null)
            }
            fn visit_none<E: de::Error>(self) -> Result<Self::Value, E> {
                Ok(StrictJson::Null)
            }
            fn visit_bool<E: de::Error>(self, value: bool) -> Result<Self::Value, E> {
                Ok(StrictJson::Bool(value))
            }
            fn visit_i64<E: de::Error>(self, value: i64) -> Result<Self::Value, E> {
                Ok(StrictJson::Integer(value.into()))
            }
            fn visit_u64<E: de::Error>(self, value: u64) -> Result<Self::Value, E> {
                Ok(StrictJson::Integer(value.into()))
            }
            fn visit_f64<E: de::Error>(self, _: f64) -> Result<Self::Value, E> {
                Err(E::custom("fraction or exponent not allowed"))
            }
            fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
                Ok(StrictJson::String(value.to_owned()))
            }
            fn visit_string<E: de::Error>(self, value: String) -> Result<Self::Value, E> {
                Ok(StrictJson::String(value))
            }
            fn visit_seq<A: SeqAccess<'de>>(self, _: A) -> Result<Self::Value, A::Error> {
                Err(de::Error::custom("arrays are not used in backup JSON"))
            }
            fn visit_map<A: MapAccess<'de>>(self, mut access: A) -> Result<Self::Value, A::Error> {
                let mut values = BTreeMap::new();
                while let Some((key, value)) = access.next_entry::<String, StrictJson>()? {
                    if values.insert(key, value).is_some() {
                        return Err(de::Error::custom("duplicate JSON key"));
                    }
                }
                Ok(StrictJson::Object(values))
            }
        }
        deserializer.deserialize_any(StrictVisitor)
    }
}
impl From<cbor::Error> for Error {
    fn from(value: cbor::Error) -> Self {
        Self::Cbor(value)
    }
}
impl From<serde_json::Error> for Error {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value)
    }
}
#[cfg(not(target_arch = "wasm32"))]
impl From<sqlite_store::Error> for Error {
    fn from(value: sqlite_store::Error) -> Self {
        Self::Store(value)
    }
}
#[cfg(not(target_arch = "wasm32"))]
impl From<shared_ready::Error> for Error {
    fn from(value: shared_ready::Error) -> Self {
        Self::Ready(value)
    }
}

/// Export a local-only Family at a caller-supplied UTC instant. The saved
/// point is the SQLite snapshot used by `load_local`; no relay credentials,
/// source outbox, or edit history enter the file.
#[cfg(not(target_arch = "wasm32"))]
pub fn export_readable_local(
    store: &SqliteStore,
    family: FamilyHandle,
    snapshot_utc_ms: i64,
) -> Result<Vec<u8>, Error> {
    if store.shared_history(family)?.is_some() {
        return Err(Error::Invalid(
            "shared Family requires verified shared export",
        ));
    }
    let projection = store.load_local(family)?;
    encode_readable(
        family.family_id,
        snapshot_utc_ms,
        None,
        false,
        projection.records(),
    )
}

/// Export the verified shared prefix together with every durable local edit
/// still waiting in the outbox. The cursor records only verified relay data.
#[cfg(not(target_arch = "wasm32"))]
pub fn export_readable_shared(
    store: &SqliteStore,
    ready: &ReadyFamilySession,
    snapshot_utc_ms: i64,
) -> Result<Vec<u8>, Error> {
    let projection = ready.projection_with_pending(store)?;
    encode_readable(
        ready.family().family_id,
        snapshot_utc_ms,
        Some(ready.observed_cursor()),
        !projection.inert_batches().is_empty(),
        projection.records(),
    )
}

pub fn encode_readable<'a>(
    source_family_id: [u8; 16],
    snapshot_utc_ms: i64,
    source_cursor: Option<u64>,
    known_gap: bool,
    records: impl Iterator<Item = &'a Record>,
) -> Result<Vec<u8>, Error> {
    let mut rows = Vec::new();
    let mut has_family = false;
    let mut children = 0u32;
    let mut other = 0u32;
    for record in records {
        let mut row = JsonMap::new();
        let kind = match record.scope {
            Scope::Child => {
                if record.record_type != "child" || record.child_id.is_some() {
                    return Err(Error::Invalid("child row identity invalid"));
                }
                children = children
                    .checked_add(1)
                    .ok_or(Error::Invalid("too many children"))?;
                "child"
            }
            Scope::Family => {
                if record.id != source_family_id
                    || record.record_type != "family"
                    || record.child_id.is_some()
                    || has_family
                {
                    return Err(Error::Invalid("family metadata identity invalid"));
                }
                has_family = true;
                other = other
                    .checked_add(1)
                    .ok_or(Error::Invalid("too many records"))?;
                "record"
            }
            Scope::Activity => {
                if record.child_id.is_none()
                    || matches!(record.record_type.as_str(), "child" | "family")
                {
                    return Err(Error::Invalid("activity row identity invalid"));
                }
                other = other
                    .checked_add(1)
                    .ok_or(Error::Invalid("too many records"))?;
                "record"
            }
        };
        row.insert("kind".into(), Json::String(kind.into()));
        row.insert("source_id".into(), Json::String(uuid_text(record.id)));
        row.insert(
            "record_type".into(),
            Json::String(record.record_type.clone()),
        );
        row.insert("deleted".into(), Json::Bool(record.deleted));
        let mut fields = JsonMap::new();
        for (id, field) in record.fields() {
            if id == 0 || cbor::encode(&field.value)? != field.canonical_bytes {
                return Err(Error::Invalid("record field differs from canonical CBOR"));
            }
            fields.insert(
                id.to_string(),
                Json::String(base64url(&field.canonical_bytes)),
            );
        }
        row.insert("fields".into(), Json::Object(fields));
        if kind == "record" {
            row.insert(
                "scope".into(),
                Json::Number(
                    match record.scope {
                        Scope::Family => 1,
                        Scope::Activity => 3,
                        Scope::Child => unreachable!(),
                    }
                    .into(),
                ),
            );
            row.insert(
                "source_child_id".into(),
                record
                    .child_id
                    .map_or(Json::Null, |id| Json::String(uuid_text(id))),
            );
        }
        rows.push((kind.to_owned(), record.id, Json::Object(row)));
    }
    if !has_family {
        let mut family = JsonMap::new();
        family.insert("kind".into(), Json::String("record".into()));
        family.insert(
            "source_id".into(),
            Json::String(uuid_text(source_family_id)),
        );
        family.insert("record_type".into(), Json::String("family".into()));
        family.insert("deleted".into(), Json::Bool(false));
        family.insert("fields".into(), Json::Object(JsonMap::new()));
        family.insert("scope".into(), Json::Number(1.into()));
        family.insert("source_child_id".into(), Json::Null);
        rows.push(("record".into(), source_family_id, Json::Object(family)));
        other = other
            .checked_add(1)
            .ok_or(Error::Invalid("too many records"))?;
    }
    rows.sort_by_key(|(kind, id, _)| (kind.clone(), *id));
    if rows.len() > 1_000_000 {
        return Err(Error::Invalid("too many backup rows"));
    }
    let mut state = Vec::new();
    for (_, _, row) in &rows {
        let line = serde_json::to_vec(row)?;
        if line.len() > 1024 * 1024 {
            return Err(Error::Invalid("backup row too large"));
        }
        state.extend_from_slice(&line);
        state.push(b'\n');
    }
    let digest: [u8; 32] = Sha256::digest(&state).into();
    let mut header = JsonMap::new();
    header.insert("kind".into(), Json::String("babytrack-backup".into()));
    header.insert("version".into(), Json::Number(1.into()));
    header.insert(
        "source_family_id".into(),
        Json::String(uuid_text(source_family_id)),
    );
    header.insert(
        "snapshot_utc_ms".into(),
        Json::Number(snapshot_utc_ms.into()),
    );
    header.insert(
        "source_cursor".into(),
        source_cursor.map_or(Json::Null, |v| Json::Number(v.into())),
    );
    header.insert("known_gap".into(), Json::Bool(known_gap));
    header.insert("child_count".into(), Json::Number(children.into()));
    header.insert("record_count".into(), Json::Number(other.into()));
    header.insert("state_sha256".into(), Json::String(lower_hex(&digest)));
    let mut result = serde_json::to_vec(&Json::Object(header))?;
    result.push(b'\n');
    result.extend_from_slice(&state);
    let mut trailer = JsonMap::new();
    trailer.insert("kind".into(), Json::String("end".into()));
    trailer.insert("rows".into(), Json::Number((rows.len() as u64).into()));
    result.extend_from_slice(&serde_json::to_vec(&Json::Object(trailer))?);
    result.push(b'\n');
    if result.len() > 2 * 1024 * 1024 * 1024 {
        return Err(Error::Invalid("backup file too large"));
    }
    Ok(result)
}

/// Parse and validate a readable snapshot without changing local storage.
/// The returned rows contain only current state and no source authority.
pub fn parse_readable(bytes: &[u8]) -> Result<ParsedBackup, Error> {
    if bytes.len() > 2 * 1024 * 1024 * 1024
        || bytes.first() == Some(&0xef)
        || !bytes.ends_with(b"\n")
        || bytes.contains(&b'\r')
    {
        return Err(Error::Invalid("backup file envelope invalid"));
    }
    let mut lines = Vec::new();
    for line in bytes.split(|byte| *byte == b'\n') {
        // Header, at most one million rows, trailer, and final empty slice.
        // Bound the index before collecting pointers from hostile input.
        if lines.len() == 1_000_003 || line.len() > 1024 * 1024 {
            return Err(Error::Invalid("backup line count or size invalid"));
        }
        lines.push(line);
    }
    if lines.len() < 4
        || !lines.last().is_some_and(|line| line.is_empty())
        || lines[..lines.len() - 1].iter().any(|line| line.is_empty())
    {
        return Err(Error::Invalid("backup line count or size invalid"));
    }
    let mut header = object(serde_json::from_slice(lines[0])?)?;
    if header.len() != 9
        || text(take(&mut header, "kind")?)? != "babytrack-backup"
        || integer(take(&mut header, "version")?)? != 1
    {
        return Err(Error::Invalid("backup header shape or version"));
    }
    let source_family_id = parse_uuid(text(take(&mut header, "source_family_id")?)?)?;
    let snapshot_utc_ms = i64::try_from(integer(take(&mut header, "snapshot_utc_ms")?)?)
        .map_err(|_| Error::Invalid("snapshot time outside i64"))?;
    let source_cursor = match take(&mut header, "source_cursor")? {
        StrictJson::Null => None,
        value => Some(
            u64::try_from(integer(value)?)
                .map_err(|_| Error::Invalid("source cursor outside u64"))?,
        ),
    };
    let known_gap = boolean(take(&mut header, "known_gap")?)?;
    let child_count = u32::try_from(integer(take(&mut header, "child_count")?)?)
        .map_err(|_| Error::Invalid("child count outside u32"))?;
    let record_count = u32::try_from(integer(take(&mut header, "record_count")?)?)
        .map_err(|_| Error::Invalid("record count outside u32"))?;
    let expected_hash = text(take(&mut header, "state_sha256")?)?.to_owned();
    if expected_hash.len() != 64
        || !expected_hash
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(Error::Invalid("state hash encoding invalid"));
    }
    let row_lines = &lines[1..lines.len() - 2];
    if row_lines.len() > 1_000_000
        || row_lines.len() != child_count as usize + record_count as usize
    {
        return Err(Error::Invalid("backup row counts differ"));
    }
    let mut hasher = Sha256::new();
    let mut rows = Vec::with_capacity(row_lines.len());
    let mut seen = BTreeSet::new();
    let mut children = BTreeSet::new();
    let mut prior: Option<(u8, [u8; 16])> = None;
    let mut counted_children = 0u32;
    let mut counted_records = 0u32;
    let mut family_rows = 0u32;
    for line in row_lines {
        hasher.update(line);
        hasher.update(b"\n");
        let row = parse_row(serde_json::from_slice(line)?, source_family_id)?;
        let order = (u8::from(row.kind != PortableKind::Child), row.source_id);
        if prior.is_some_and(|previous| order <= previous) || !seen.insert(row.source_id) {
            return Err(Error::Invalid("backup rows unsorted or duplicate ID"));
        }
        prior = Some(order);
        match row.kind {
            PortableKind::Child => {
                counted_children += 1;
                children.insert(row.source_id);
            }
            PortableKind::Family => {
                counted_records += 1;
                family_rows += 1;
            }
            PortableKind::Activity => counted_records += 1,
        }
        rows.push(row);
    }
    if lower_hex(&hasher.finalize()) != expected_hash
        || counted_children != child_count
        || counted_records != record_count
        || family_rows != 1
    {
        return Err(Error::Invalid("backup state hash or row kinds differ"));
    }
    if rows.iter().any(|row| {
        row.kind == PortableKind::Activity
            && row.source_child_id.is_none_or(|id| !children.contains(&id))
    }) {
        return Err(Error::Invalid("backup activity references missing child"));
    }
    let mut trailer = object(serde_json::from_slice(lines[lines.len() - 2])?)?;
    if trailer.len() != 2
        || text(take(&mut trailer, "kind")?)? != "end"
        || integer(take(&mut trailer, "rows")?)? != rows.len() as i128
    {
        return Err(Error::Invalid("backup trailer differs from rows"));
    }
    Ok(ParsedBackup {
        source_family_id,
        snapshot_utc_ms,
        source_cursor,
        known_gap,
        rows,
    })
}

/// Restore only saved current state into a fresh, offline Family. Parsing,
/// field checks, and all operation construction happen before one SQLite
/// transaction publishes the new Family and its saved-point metadata.
#[cfg(not(target_arch = "wasm32"))]
pub fn restore_readable(
    store: &mut SqliteStore,
    bytes: &[u8],
    now_ms: i64,
) -> Result<FamilyHandle, Error> {
    restore_readable_inner(store, bytes, now_ms, None)
}

#[cfg(not(target_arch = "wasm32"))]
fn restore_readable_inner(
    store: &mut SqliteStore,
    bytes: &[u8],
    now_ms: i64,
    copy_source: Option<sqlite_store::CopySource>,
) -> Result<FamilyHandle, Error> {
    let parsed = parse_readable(bytes)?;
    if copy_source.is_some_and(|source| {
        let family = match source {
            sqlite_store::CopySource::Manual(family)
            | sqlite_store::CopySource::Removal(family, _) => family,
        };
        family.family_id != parsed.source_family_id
    }) {
        return Err(Error::Invalid("copy source differs from backup"));
    }
    let mut family_id = new_v4()?;
    while family_id == parsed.source_family_id {
        family_id = new_v4()?;
    }
    let device_id = new_v4()?;
    let mut seen_ops = BTreeSet::new();
    let ids = (0..restore_operation_count(&parsed))
        .map(|_| fresh_v7(now_ms, &mut seen_ops))
        .collect::<Result<Vec<_>, _>>()?;
    let operations = restore_operations(&parsed, family_id, device_id, now_ms, &ids)?;
    let origin = RestoredOrigin {
        source_family_id: parsed.source_family_id,
        snapshot_utc_ms: parsed.snapshot_utc_ms,
        source_cursor: parsed.source_cursor,
        known_gap: parsed.known_gap,
    };
    Ok(if let Some(source) = copy_source {
        store.restore_family_with_copy_source(
            family_id,
            device_id,
            operations,
            now_ms,
            origin,
            Some(source),
        )?
    } else {
        store.restore_family(family_id, device_id, operations, now_ms, origin)?
    })
}

pub fn restore_operation_count(parsed: &ParsedBackup) -> usize {
    parsed
        .rows
        .iter()
        .map(|row| if row.deleted { 2 } else { 1 })
        .sum()
}

/// Build current-state restore operations from a parsed portable file.
/// Platform adapters supply unique UUIDv7 IDs; the core owns field and
/// record semantics on every platform.
pub fn restore_operations(
    parsed: &ParsedBackup,
    family_id: [u8; 16],
    device_id: [u8; 16],
    now_ms: i64,
    ids: &[[u8; 16]],
) -> Result<Vec<NewOperation>, Error> {
    if family_id == parsed.source_family_id || ids.len() != restore_operation_count(parsed) {
        return Err(Error::Invalid(
            "restore identity or operation ID count invalid",
        ));
    }
    let mut unique = BTreeSet::new();
    if ids
        .iter()
        .any(|id| !unique.insert(*id) || id[6] & 0xf0 != 0x70 || id[8] & 0xc0 != 0x80)
    {
        return Err(Error::Invalid("restore operation ID invalid or repeated"));
    }
    let mut operations = Vec::new();
    let mut next_id = ids.iter().copied();
    let ordered = parsed
        .rows
        .iter()
        .filter(|row| row.kind == PortableKind::Family)
        .chain(
            parsed
                .rows
                .iter()
                .filter(|row| row.kind == PortableKind::Child),
        )
        .chain(
            parsed
                .rows
                .iter()
                .filter(|row| row.kind == PortableKind::Activity),
        );
    for row in ordered {
        let scope = match row.kind {
            PortableKind::Family => Scope::Family,
            PortableKind::Child => Scope::Child,
            PortableKind::Activity => Scope::Activity,
        };
        let record_id = if row.kind == PortableKind::Family {
            family_id
        } else {
            row.source_id
        };
        let fields = row
            .fields
            .iter()
            .map(|(id, bytes)| {
                Ok((
                    *id,
                    cbor::decode_with_limits(
                        bytes,
                        cbor::Limits {
                            max_bytes: 64 * 1024,
                            max_depth: 16,
                        },
                    )?,
                ))
            })
            .collect::<Result<Vec<_>, Error>>()?;
        operations.push(NewOperation {
            family_id,
            operation_id: next_id.next().ok_or(Error::Invalid("restore ID absent"))?,
            record_id,
            scope,
            kind: Kind::Create,
            author_device_id: device_id,
            hlc: Hlc {
                wall_ms: now_ms,
                counter: 0,
                device_id,
            },
            record_type: Some(row.record_type.clone()),
            child_id: row.source_child_id,
            fields: Some(fields),
        });
        if row.deleted {
            operations.push(NewOperation {
                family_id,
                operation_id: next_id.next().ok_or(Error::Invalid("restore ID absent"))?,
                record_id,
                scope,
                kind: Kind::Delete,
                author_device_id: device_id,
                hlc: Hlc {
                    wall_ms: now_ms,
                    counter: 0,
                    device_id,
                },
                record_type: None,
                child_id: None,
                fields: None,
            });
        }
    }
    Ok(operations)
}

/// Create or return this installation's single private copy of the verified
/// Family state. A repeated notice reuses the same destination.
#[cfg(not(target_arch = "wasm32"))]
pub fn private_copy_shared(
    store: &mut SqliteStore,
    ready: &ReadyFamilySession,
    now_ms: i64,
) -> Result<FamilyHandle, Error> {
    let source = ready.family();
    if let Some(existing) = store.private_copy_of(source)? {
        return Ok(existing);
    }
    let readable = export_readable_shared(store, ready, now_ms)?;
    match restore_readable_inner(
        store,
        &readable,
        now_ms,
        Some(sqlite_store::CopySource::Manual(source)),
    ) {
        Ok(copy) => Ok(copy),
        Err(Error::Store(sqlite_store::Error::Sqlite(_))) => store
            .private_copy_of(source)?
            .ok_or(Error::Invalid("private copy failed")),
        Err(error) => Err(error),
    }
}

/// Recover locally held state after a signed removal. The proof is saved
/// before this call; a restart can retry the same transaction idempotently.
#[cfg(not(target_arch = "wasm32"))]
pub fn private_copy_after_removal(
    store: &mut SqliteStore,
    ready: &ReadyFamilySession,
    removal: &sqlite_store::SavedRemoval,
    now_ms: i64,
) -> Result<FamilyHandle, Error> {
    let source = ready.family();
    if store.saved_removal(source)?.as_ref() != Some(removal) {
        return Err(Error::Invalid("removal proof not saved"));
    }
    if let Some(existing) = store.removal_copy_of(source, removal.transition_id)? {
        return Ok(existing);
    }
    let projection = ready.projection_with_pending(store)?;
    let readable = encode_readable(
        source.family_id,
        now_ms,
        Some(ready.observed_cursor()),
        removal.known_gap || !projection.inert_batches().is_empty(),
        projection.records(),
    )?;
    match restore_readable_inner(
        store,
        &readable,
        now_ms,
        Some(sqlite_store::CopySource::Removal(
            source,
            removal.transition_id,
        )),
    ) {
        Ok(copy) => Ok(copy),
        Err(Error::Store(sqlite_store::Error::Sqlite(_))) => store
            .removal_copy_of(source, removal.transition_id)?
            .ok_or(Error::Invalid("removal copy failed")),
        Err(error) => Err(error),
    }
}

/// Encrypt a valid readable file under the fixed v1 Argon2id profile. The
/// caller supplies a fresh platform memory estimate so a low-memory device
/// reports failure instead of silently weakening the profile.
pub fn protect_readable(
    readable: &[u8],
    password: &str,
    available_memory_bytes: u64,
) -> Result<Vec<u8>, Error> {
    parse_readable(readable)?;
    check_kdf_memory(available_memory_bytes)?;
    let mut salt = [0u8; 16];
    let mut nonce = [0u8; 24];
    getrandom::fill(&mut salt).map_err(Error::Random)?;
    getrandom::fill(&mut nonce).map_err(Error::Random)?;
    let header = cbor::encode(&cbor::Value::Map(vec![
        (1, cbor::Value::Integer(1)),
        (2, cbor::Value::Bytes(salt.to_vec())),
        (3, cbor::Value::Integer(65_536)),
        (4, cbor::Value::Integer(3)),
        (5, cbor::Value::Integer(4)),
        (6, cbor::Value::Bytes(nonce.to_vec())),
        (7, cbor::Value::Integer(readable.len() as i128)),
    ]))?;
    let mut aad = b"BTBK1".to_vec();
    aad.extend_from_slice(&header);
    let key = derive_backup_key(password, &salt).map_err(|_| Error::ProtectedFailure)?;
    let ciphertext = crate::crypto::seal_with_nonce(&key, &nonce, &aad, readable)
        .map_err(|_| Error::ProtectedFailure)?;
    aad.extend_from_slice(&ciphertext);
    Ok(aad)
}

/// Password, tamper, truncation, and invalid plaintext share one failure.
pub fn open_protected(
    protected: &[u8],
    password: &str,
    available_memory_bytes: u64,
) -> Result<Vec<u8>, Error> {
    let open = || -> Result<Vec<u8>, Error> {
        check_kdf_memory(available_memory_bytes)?;
        if !protected.starts_with(b"BTBK1") || protected.len() > 2 * 1024 * 1024 * 1024 + 256 {
            return Err(Error::ProtectedFailure);
        }
        let (value, size) = cbor::decode_prefix_with_limits(
            &protected[5..],
            cbor::Limits {
                max_bytes: 128,
                max_depth: 3,
            },
        )?;
        let cbor::Value::Map(fields) = value else {
            return Err(Error::ProtectedFailure);
        };
        if fields.len() != 7
            || fields
                .iter()
                .enumerate()
                .any(|(i, (key, _))| *key != i as u64 + 1)
            || fields[0].1 != cbor::Value::Integer(1)
            || fields[2].1 != cbor::Value::Integer(65_536)
            || fields[3].1 != cbor::Value::Integer(3)
            || fields[4].1 != cbor::Value::Integer(4)
        {
            return Err(Error::ProtectedFailure);
        }
        let cbor::Value::Bytes(salt) = &fields[1].1 else {
            return Err(Error::ProtectedFailure);
        };
        let cbor::Value::Bytes(nonce) = &fields[5].1 else {
            return Err(Error::ProtectedFailure);
        };
        let cbor::Value::Integer(length) = fields[6].1 else {
            return Err(Error::ProtectedFailure);
        };
        let salt: [u8; 16] = salt
            .as_slice()
            .try_into()
            .map_err(|_| Error::ProtectedFailure)?;
        let nonce: [u8; 24] = nonce
            .as_slice()
            .try_into()
            .map_err(|_| Error::ProtectedFailure)?;
        let length = usize::try_from(length).map_err(|_| Error::ProtectedFailure)?;
        if length > 2 * 1024 * 1024 * 1024
            || protected.len().checked_sub(5 + size) != length.checked_add(16)
        {
            return Err(Error::ProtectedFailure);
        }
        let aad = &protected[..5 + size];
        let ciphertext = &protected[5 + size..];
        let key = derive_backup_key(password, &salt).map_err(|_| Error::ProtectedFailure)?;
        let readable = crate::crypto::open(&key, &nonce, aad, ciphertext)
            .map_err(|_| Error::ProtectedFailure)?;
        parse_readable(&readable).map_err(|_| Error::ProtectedFailure)?;
        Ok(readable)
    };
    open().map_err(|_| Error::ProtectedFailure)
}

#[cfg(not(target_arch = "wasm32"))]
pub fn restore_protected(
    store: &mut SqliteStore,
    protected: &[u8],
    password: &str,
    available_memory_bytes: u64,
    now_ms: i64,
) -> Result<FamilyHandle, Error> {
    let readable = open_protected(protected, password, available_memory_bytes)?;
    match restore_readable(store, &readable, now_ms) {
        Err(Error::Store(
            sqlite_store::Error::Operation(_) | sqlite_store::Error::Projection(_),
        )) => Err(Error::ProtectedFailure),
        result => result,
    }
}

fn check_kdf_memory(available_memory_bytes: u64) -> Result<(), Error> {
    if available_memory_bytes < 128 * 1024 * 1024 {
        return Err(Error::ProtectedFailure);
    }
    Ok(())
}

fn derive_backup_key(
    password: &str,
    salt: &[u8; 16],
) -> Result<Zeroizing<[u8; 32]>, argon2::Error> {
    let params = Params::new(65_536, 3, 4, Some(32))?;
    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let normalized = Zeroizing::new(password.nfc().collect::<String>());
    let mut key = Zeroizing::new([0u8; 32]);
    argon2.hash_password_into(normalized.as_bytes(), salt, &mut key[..])?;
    Ok(key)
}

#[cfg(not(target_arch = "wasm32"))]
fn new_v4() -> Result<[u8; 16], Error> {
    let mut id = [0u8; 16];
    getrandom::fill(&mut id).map_err(Error::Random)?;
    id[6] = (id[6] & 0x0f) | 0x40;
    id[8] = (id[8] & 0x3f) | 0x80;
    Ok(id)
}

#[cfg(not(target_arch = "wasm32"))]
fn fresh_v7(now_ms: i64, seen: &mut BTreeSet<[u8; 16]>) -> Result<[u8; 16], Error> {
    let at = u64::try_from(now_ms).map_err(|_| Error::Invalid("restore time before Unix epoch"))?;
    if at > 0xffff_ffff_ffff {
        return Err(Error::Invalid("restore time outside UUIDv7 range"));
    }
    for _ in 0..8 {
        let mut id = [0u8; 16];
        getrandom::fill(&mut id).map_err(Error::Random)?;
        id[..6].copy_from_slice(&at.to_be_bytes()[2..]);
        id[6] = (id[6] & 0x0f) | 0x70;
        id[8] = (id[8] & 0x3f) | 0x80;
        if seen.insert(id) {
            return Ok(id);
        }
    }
    Err(Error::Invalid("restore operation ID collision"))
}

fn parse_row(value: StrictJson, family_id: [u8; 16]) -> Result<PortableRow, Error> {
    let mut map = object(value)?;
    let kind = text(take(&mut map, "kind")?)?.to_owned();
    let expected_width = if kind == "child" {
        5
    } else if kind == "record" {
        7
    } else {
        return Err(Error::Invalid("backup row kind invalid"));
    };
    if map.len() + 1 != expected_width {
        return Err(Error::Invalid("backup row keys invalid"));
    }
    let source_id = parse_uuid(text(take(&mut map, "source_id")?)?)?;
    let record_type = text(take(&mut map, "record_type")?)?.to_owned();
    if record_type.is_empty() || record_type.len() > 16 * 1024 {
        return Err(Error::Invalid("backup record type invalid"));
    }
    let deleted = boolean(take(&mut map, "deleted")?)?;
    let mut fields = BTreeMap::new();
    for (key, value) in object(take(&mut map, "fields")?)? {
        if key.is_empty()
            || (key.len() > 1 && key.starts_with('0'))
            || !key.bytes().all(|b| b.is_ascii_digit())
        {
            return Err(Error::Invalid("backup field ID not decimal"));
        }
        let id: u64 = key
            .parse()
            .map_err(|_| Error::Invalid("backup field ID outside u64"))?;
        if id == 0 {
            return Err(Error::Invalid("backup field ID zero"));
        }
        let encoded = decode_base64url(text(value)?)?;
        if encoded.len() > 64 * 1024 {
            return Err(Error::Invalid("backup field too large"));
        }
        cbor::decode_with_limits(
            &encoded,
            cbor::Limits {
                max_bytes: 64 * 1024,
                max_depth: 16,
            },
        )?;
        fields.insert(id, encoded);
    }
    let (kind, source_child_id) = if kind == "child" {
        if record_type != "child" {
            return Err(Error::Invalid("child record type invalid"));
        }
        (PortableKind::Child, None)
    } else {
        let scope = integer(take(&mut map, "scope")?)?;
        let child = match take(&mut map, "source_child_id")? {
            StrictJson::Null => None,
            value => Some(parse_uuid(text(value)?)?),
        };
        match scope {
            1 if record_type == "family"
                && source_id == family_id
                && child.is_none()
                && !deleted =>
            {
                (PortableKind::Family, None)
            }
            3 if !matches!(record_type.as_str(), "family" | "child") && child.is_some() => {
                (PortableKind::Activity, child)
            }
            _ => return Err(Error::Invalid("record scope or identity invalid")),
        }
    };
    if !map.is_empty() {
        return Err(Error::Invalid("unknown backup row key"));
    }
    if fields.len() > 128 {
        return Err(Error::Invalid("backup record has too many fields"));
    }
    let scope = match kind {
        PortableKind::Child => Scope::Child,
        PortableKind::Family => Scope::Family,
        PortableKind::Activity => Scope::Activity,
    };
    let decoded = fields
        .iter()
        .map(|(id, bytes)| {
            let value = cbor::decode(bytes)?;
            check_texts(&value)?;
            Ok((*id, value))
        })
        .collect::<Result<Vec<_>, Error>>()?;
    record_validity::validate_fields(scope, &record_type, Kind::Create, &decoded)
        .map_err(Error::Invalid)?;
    if record_type == "pump" {
        let has_total = decoded
            .iter()
            .any(|(id, value)| *id == 102 && *value != cbor::Value::Null);
        let has_side = decoded
            .iter()
            .any(|(id, value)| (*id == 100 || *id == 101) && *value != cbor::Value::Null);
        if has_total && has_side {
            return Err(Error::Invalid("pump total conflicts with side amounts"));
        }
    }
    Ok(PortableRow {
        kind,
        source_id,
        source_child_id,
        record_type,
        deleted,
        fields,
    })
}

fn check_texts(value: &cbor::Value) -> Result<(), Error> {
    match value {
        cbor::Value::Text(text) if text.len() > 16 * 1024 => {
            Err(Error::Invalid("backup text too long"))
        }
        cbor::Value::Array(items) => {
            for value in items {
                check_texts(value)?;
            }
            Ok(())
        }
        cbor::Value::Map(entries) => {
            for (_, value) in entries {
                check_texts(value)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn object(value: StrictJson) -> Result<BTreeMap<String, StrictJson>, Error> {
    match value {
        StrictJson::Object(map) => Ok(map),
        _ => Err(Error::Invalid("expected JSON object")),
    }
}
fn take(map: &mut BTreeMap<String, StrictJson>, key: &str) -> Result<StrictJson, Error> {
    map.remove(key)
        .ok_or(Error::Invalid("required JSON key missing"))
}
fn text(value: StrictJson) -> Result<String, Error> {
    match value {
        StrictJson::String(text) => Ok(text),
        _ => Err(Error::Invalid("expected JSON string")),
    }
}
fn integer(value: StrictJson) -> Result<i128, Error> {
    match value {
        StrictJson::Integer(number) => Ok(number),
        _ => Err(Error::Invalid("expected JSON integer")),
    }
}
fn boolean(value: StrictJson) -> Result<bool, Error> {
    match value {
        StrictJson::Bool(value) => Ok(value),
        _ => Err(Error::Invalid("expected JSON boolean")),
    }
}

fn parse_uuid(text: String) -> Result<[u8; 16], Error> {
    let bytes = text.as_bytes();
    if bytes.len() != 36 || [8, 13, 18, 23].iter().any(|i| bytes[*i] != b'-') {
        return Err(Error::Invalid("backup UUID shape invalid"));
    }
    let mut raw = [0u8; 16];
    let digits = bytes
        .iter()
        .copied()
        .filter(|byte| *byte != b'-')
        .collect::<Vec<_>>();
    for (slot, pair) in raw.iter_mut().zip(digits.chunks_exact(2)) {
        let digit = |byte| match byte {
            b'0'..=b'9' => Ok(byte - b'0'),
            b'a'..=b'f' => Ok(byte - b'a' + 10),
            _ => Err(Error::Invalid("backup UUID must be lowercase hex")),
        };
        *slot = digit(pair[0])? << 4 | digit(pair[1])?;
    }
    Ok(raw)
}

fn decode_base64url(text: String) -> Result<Vec<u8>, Error> {
    if text.len() % 4 == 1
        || text
            .bytes()
            .any(|byte| !byte.is_ascii_alphanumeric() && byte != b'-' && byte != b'_')
    {
        return Err(Error::Invalid("backup base64url invalid"));
    }
    let mut output = Vec::with_capacity(text.len() * 3 / 4);
    let mut bits = 0u32;
    let mut count = 0u8;
    for byte in text.bytes() {
        let digit = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'-' => 62,
            b'_' => 63,
            _ => unreachable!(),
        };
        bits = (bits << 6) | u32::from(digit);
        count += 6;
        if count >= 8 {
            count -= 8;
            output.push((bits >> count) as u8);
            bits &= (1 << count) - 1;
        }
    }
    if bits != 0 || base64url(&output) != text {
        return Err(Error::Invalid("backup base64url noncanonical"));
    }
    Ok(output)
}

fn uuid_text(bytes: [u8; 16]) -> String {
    let hex = lower_hex(&bytes);
    format!(
        "{}-{}-{}-{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..]
    )
}
fn lower_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut result = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        result.push(HEX[(byte >> 4) as usize] as char);
        result.push(HEX[(byte & 15) as usize] as char);
    }
    result
}
fn base64url(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut result = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for group in bytes.chunks(3) {
        let word = (u32::from(group[0]) << 16)
            | (u32::from(*group.get(1).unwrap_or(&0)) << 8)
            | u32::from(*group.get(2).unwrap_or(&0));
        result.push(TABLE[((word >> 18) & 63) as usize] as char);
        result.push(TABLE[((word >> 12) & 63) as usize] as char);
        if group.len() > 1 {
            result.push(TABLE[((word >> 6) & 63) as usize] as char);
        }
        if group.len() > 2 {
            result.push(TABLE[(word & 63) as usize] as char);
        }
    }
    result
}
