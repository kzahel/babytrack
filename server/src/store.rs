//! Durable genesis reservation and commit. All methods are internal until
//! authenticated routes and the remaining authority transitions are ready.

use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

use babytrack_wire::{
    cbor::{self, Value},
    crypto,
};
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};

use crate::{authority, batch_authority, public_ledger, read_auth, receipt};

#[derive(Debug)]
#[allow(dead_code)] // Detailed errors are mapped to protocol responses by routes.
#[allow(private_interfaces)] // The test-harness feature exposes only Debug/errors.
pub enum Error {
    Sql(rusqlite::Error),
    Cbor(cbor::Error),
    Crypto(crypto::Error),
    Authority(authority::Error),
    Ledger(public_ledger::Error),
    Batch(batch_authority::Error),
    ReadAuth(read_auth::Error),
    Receipt(receipt::Error),
    Clock,
    Invalid(&'static str),
}
impl From<rusqlite::Error> for Error {
    fn from(v: rusqlite::Error) -> Self {
        Self::Sql(v)
    }
}
impl From<cbor::Error> for Error {
    fn from(v: cbor::Error) -> Self {
        Self::Cbor(v)
    }
}
impl From<crypto::Error> for Error {
    fn from(v: crypto::Error) -> Self {
        Self::Crypto(v)
    }
}
impl From<authority::Error> for Error {
    fn from(v: authority::Error) -> Self {
        Self::Authority(v)
    }
}
impl From<public_ledger::Error> for Error {
    fn from(v: public_ledger::Error) -> Self {
        Self::Ledger(v)
    }
}
impl From<batch_authority::Error> for Error {
    fn from(v: batch_authority::Error) -> Self {
        Self::Batch(v)
    }
}
impl From<read_auth::Error> for Error {
    fn from(v: read_auth::Error) -> Self {
        Self::ReadAuth(v)
    }
}
impl From<receipt::Error> for Error {
    fn from(v: receipt::Error) -> Self {
        Self::Receipt(v)
    }
}

#[allow(dead_code)] // Will be owned by the HTTP service once routes are wired.
pub struct RelayStore {
    db: Connection,
    relay_seed: [u8; 32],
    relay_public: [u8; 32],
}

type SavedReservation = (Vec<u8>, Vec<u8>, i64, Option<Vec<u8>>);
type StoredCommittedObject = (i64, Vec<u8>, Vec<u8>, Vec<u8>);
type StoredBatchResult = (Vec<u8>, Vec<u8>, Vec<u8>, Vec<u8>, i64);

enum ControlReader {
    Manager,
    Active,
    Removed {
        device_id: [u8; 16],
        signing_public: [u8; 32],
        relay_id: [u8; 32],
    },
    Invitation {
        issue_object: [u8; 16],
    },
    Pending {
        challenge_object: Option<[u8; 16]>,
    },
}

#[allow(dead_code)] // The methods become route handlers after read authentication.
impl RelayStore {
    pub fn open(path: impl AsRef<Path>, relay_seed: [u8; 32]) -> Result<Self, Error> {
        let db = Connection::open(path)?;
        Self::initialize(db, relay_seed)
    }

    fn initialize(mut db: Connection, relay_seed: [u8; 32]) -> Result<Self, Error> {
        db.execute_batch(
            "PRAGMA foreign_keys=ON;
             PRAGMA synchronous=FULL;
             CREATE TABLE IF NOT EXISTS relay_identity (
               singleton INTEGER PRIMARY KEY CHECK (singleton=1), public_key BLOB NOT NULL
             );
             CREATE TABLE IF NOT EXISTS families (
               family_id BLOB PRIMARY KEY, reservation_hash BLOB NOT NULL,
               candidate_bytes BLOB NOT NULL, active INTEGER NOT NULL CHECK (active IN (0,1)),
               committed_bytes BLOB, head_hash BLOB, cursor INTEGER NOT NULL DEFAULT 0
             );
             CREATE TABLE IF NOT EXISTS staged_objects (
               family_id BLOB NOT NULL, object_id BLOB NOT NULL, kind INTEGER NOT NULL,
               object_hash BLOB NOT NULL, object_bytes BLOB NOT NULL,
               PRIMARY KEY (family_id, object_id)
             );
             CREATE TABLE IF NOT EXISTS staged_issues (
               family_id BLOB PRIMARY KEY, transition_id BLOB NOT NULL,
               candidate_bytes BLOB NOT NULL
             );
             CREATE TABLE IF NOT EXISTS staged_challenges (
               family_id BLOB PRIMARY KEY, transition_id BLOB NOT NULL,
               candidate_bytes BLOB NOT NULL
             );
             CREATE TABLE IF NOT EXISTS staged_admissions (
               family_id BLOB PRIMARY KEY, transition_id BLOB NOT NULL,
               candidate_bytes BLOB NOT NULL
             );
             CREATE TABLE IF NOT EXISTS staged_removals (
               family_id BLOB PRIMARY KEY, transition_id BLOB NOT NULL,
               candidate_bytes BLOB NOT NULL
             );
             CREATE TABLE IF NOT EXISTS staged_controls (
               family_id BLOB NOT NULL, transition_id BLOB NOT NULL,
               candidate_bytes BLOB NOT NULL,
               PRIMARY KEY (family_id, transition_id)
             );
             CREATE TABLE IF NOT EXISTS staged_control_objects (
               family_id BLOB NOT NULL, transition_id BLOB NOT NULL,
               object_id BLOB NOT NULL, kind INTEGER NOT NULL,
               object_hash BLOB NOT NULL, object_bytes BLOB NOT NULL,
               PRIMARY KEY (family_id, transition_id, object_id)
             );
             CREATE TABLE IF NOT EXISTS object_reservations (
               family_id BLOB NOT NULL, object_id BLOB NOT NULL,
               kind INTEGER NOT NULL, object_hash BLOB NOT NULL,
               PRIMARY KEY (family_id, object_id)
             );
             CREATE TABLE IF NOT EXISTS committed_objects (
               family_id BLOB NOT NULL, object_id BLOB NOT NULL, kind INTEGER NOT NULL,
               object_hash BLOB NOT NULL, object_bytes BLOB NOT NULL,
               transition_id BLOB NOT NULL, PRIMARY KEY (family_id, object_id)
             );
             CREATE TABLE IF NOT EXISTS entries (
               family_id BLOB NOT NULL, cursor INTEGER NOT NULL, kind INTEGER NOT NULL,
               committed_bytes BLOB NOT NULL, PRIMARY KEY (family_id, cursor)
             );
             CREATE TABLE IF NOT EXISTS read_requests (
               family_id BLOB NOT NULL, signer_id BLOB NOT NULL,
               request_id BLOB NOT NULL, request_hash BLOB NOT NULL,
               PRIMARY KEY (family_id, signer_id, request_id)
             );
             CREATE TABLE IF NOT EXISTS batch_results (
               family_id BLOB NOT NULL, batch_id BLOB NOT NULL,
               author_id BLOB NOT NULL, sequence INTEGER NOT NULL,
               envelope_bytes BLOB NOT NULL, receipt_bytes BLOB NOT NULL,
               cursor INTEGER NOT NULL,
               PRIMARY KEY (family_id, batch_id),
               UNIQUE (family_id, author_id, sequence),
               UNIQUE (family_id, cursor)
             );
             CREATE TABLE IF NOT EXISTS rejected_batch_results (
               family_id BLOB NOT NULL, batch_id BLOB NOT NULL,
               envelope_bytes BLOB NOT NULL, receipt_bytes BLOB NOT NULL,
               PRIMARY KEY (family_id, batch_id)
             );",
        )?;
        // Existing development relays used one staging slot per Family. Move
        // an interrupted candidate into the candidate-scoped tables before
        // accepting new staging requests. Genesis keeps its own staging table.
        let migration = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        migration.execute_batch(
            "INSERT INTO staged_controls(family_id,transition_id,candidate_bytes)
               SELECT family_id,transition_id,candidate_bytes FROM staged_issues
               UNION ALL SELECT family_id,transition_id,candidate_bytes FROM staged_challenges
               UNION ALL SELECT family_id,transition_id,candidate_bytes FROM staged_admissions
               UNION ALL SELECT family_id,transition_id,candidate_bytes FROM staged_removals;
             INSERT INTO staged_control_objects(family_id,transition_id,object_id,kind,object_hash,object_bytes)
               SELECT o.family_id,c.transition_id,o.object_id,o.kind,o.object_hash,o.object_bytes
               FROM staged_objects o JOIN staged_controls c ON c.family_id=o.family_id
               JOIN families f ON f.family_id=o.family_id AND f.active=1;
             DELETE FROM staged_objects WHERE family_id IN
               (SELECT family_id FROM families WHERE active=1);
             DELETE FROM staged_issues;
             DELETE FROM staged_challenges;
             DELETE FROM staged_admissions;
             DELETE FROM staged_removals;
             INSERT OR IGNORE INTO object_reservations(family_id,object_id,kind,object_hash)
               SELECT family_id,object_id,kind,object_hash FROM committed_objects;
             INSERT OR IGNORE INTO object_reservations(family_id,object_id,kind,object_hash)
               SELECT family_id,object_id,kind,object_hash FROM staged_objects;
             INSERT OR IGNORE INTO object_reservations(family_id,object_id,kind,object_hash)
               SELECT family_id,object_id,kind,object_hash FROM staged_control_objects;",
        )?;
        let inconsistent: i64 = migration.query_row(
            "SELECT EXISTS(
               SELECT 1 FROM committed_objects o JOIN object_reservations r USING(family_id,object_id)
                 WHERE o.kind!=r.kind OR o.object_hash!=r.object_hash
               UNION ALL
               SELECT 1 FROM staged_objects o JOIN object_reservations r USING(family_id,object_id)
                 WHERE o.kind!=r.kind OR o.object_hash!=r.object_hash
               UNION ALL
               SELECT 1 FROM staged_control_objects o JOIN object_reservations r USING(family_id,object_id)
                 WHERE o.kind!=r.kind OR o.object_hash!=r.object_hash
             )",
            [],
            |r| r.get(0),
        )?;
        if inconsistent != 0 {
            return Err(Error::Invalid("object reservation conflicts on restart"));
        }
        migration.commit()?;
        let relay_public = crypto::signing_public_key(&relay_seed);
        db.execute(
            "INSERT OR IGNORE INTO relay_identity(singleton, public_key) VALUES(1, ?1)",
            params![&relay_public[..]],
        )?;
        let stored: Vec<u8> = db.query_row(
            "SELECT public_key FROM relay_identity WHERE singleton=1",
            [],
            |r| r.get(0),
        )?;
        if stored != relay_public {
            return Err(Error::Invalid("relay signing identity changed on restart"));
        }
        let store = Self {
            db,
            relay_seed,
            relay_public,
        };
        store.verify_saved_families()?;
        Ok(store)
    }

    /// Reject a damaged durable log before serving a reopened relay. Derived
    /// authority rows must never outrank the signed control and receipt chain.
    fn verify_saved_families(&self) -> Result<(), Error> {
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
    fn verify_saved_family(
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

    pub fn relay_public_key(&self) -> [u8; 32] {
        self.relay_public
    }

    pub fn stage_genesis_object(
        &mut self,
        path_family: [u8; 16],
        path_object: [u8; 16],
        body: &[u8],
    ) -> Result<Vec<u8>, Error> {
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
            return Err(Error::Invalid("stage body keys or version"));
        }
        let candidate_bytes = cbor::encode(&Value::Map(vec![
            (1, fields[1].1.clone()),
            (2, fields[2].1.clone()),
        ]))?;
        let candidate = authority::verify_genesis_candidate(&candidate_bytes, &self.relay_public)?;
        let kind: u16 = number(&fields[3].1)?
            .try_into()
            .map_err(|_| Error::Invalid("object kind range"))?;
        let object_id = fixed::<16>(&fields[4].1)?;
        if candidate.family_id != path_family || object_id != path_object {
            return Err(Error::Invalid(
                "stage path differs from signed candidate or object",
            ));
        }
        let Value::Bytes(object_bytes) = &fields[5].1 else {
            return Err(Error::Invalid("object not bytes"));
        };
        let hash = crypto::hash("object", object_bytes)?;
        let listed = candidate
            .manifest
            .iter()
            .find(|entry| entry.kind == kind && entry.object_id == object_id)
            .ok_or(Error::Invalid("object absent from signed manifest"))?;
        if listed.object_hash != hash
            || usize::try_from(listed.object_len).ok() != Some(object_bytes.len())
        {
            return Err(Error::Invalid("object differs from signed manifest"));
        }
        let reservation = crypto::hash("genesis-reservation", &candidate_bytes)?;
        let tx = self.db.transaction()?;
        let existing: Option<(Vec<u8>, Vec<u8>, i64)> = tx
            .query_row(
                "SELECT reservation_hash, candidate_bytes, active FROM families WHERE family_id=?1",
                params![&candidate.family_id[..]],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        if let Some((saved_hash, saved_candidate, active)) = existing {
            if saved_hash != reservation || saved_candidate != candidate_bytes {
                return Err(Error::Invalid("Family ID reserved by another genesis"));
            }
            if active == 1 {
                let committed: Option<Vec<u8>> = tx.query_row(
                    "SELECT object_bytes FROM committed_objects WHERE family_id=?1 AND object_id=?2",
                    params![&candidate.family_id[..], &object_id[..]], |r| r.get(0),
                ).optional()?;
                if committed.as_deref() == Some(object_bytes) {
                    return Ok(receipt::object_stage_response(object_bytes)?);
                }
                return Err(Error::Invalid("committed object differs from staged retry"));
            }
        } else {
            tx.execute(
                "INSERT INTO families(family_id,reservation_hash,candidate_bytes,active,cursor) VALUES(?1,?2,?3,0,0)",
                params![&candidate.family_id[..], &reservation[..], &candidate_bytes],
            )?;
        }
        reserve_staged_object(&tx, candidate.family_id, object_id, kind, hash)?;
        let prior: Option<(i64, Vec<u8>, Vec<u8>)> = tx.query_row(
            "SELECT kind,object_hash,object_bytes FROM staged_objects WHERE family_id=?1 AND object_id=?2",
            params![&candidate.family_id[..], &object_id[..]], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?)),
        ).optional()?;
        if let Some((saved_kind, saved_hash, saved_bytes)) = prior {
            if saved_kind != i64::from(kind) || saved_hash != hash || saved_bytes != *object_bytes {
                return Err(Error::Invalid("object ID reused with different bytes"));
            }
        } else {
            tx.execute(
                "INSERT INTO staged_objects(family_id,object_id,kind,object_hash,object_bytes) VALUES(?1,?2,?3,?4,?5)",
                params![&candidate.family_id[..], &object_id[..], i64::from(kind), &hash[..], object_bytes],
            )?;
        }
        tx.commit()?;
        Ok(receipt::object_stage_response(object_bytes)?)
    }

    pub fn commit_genesis(
        &mut self,
        path_family: [u8; 16],
        candidate_bytes: &[u8],
        committed_ms: i64,
    ) -> Result<Vec<u8>, Error> {
        let candidate = authority::verify_genesis_candidate(candidate_bytes, &self.relay_public)?;
        if candidate.family_id != path_family {
            return Err(Error::Invalid("commit path differs from signed candidate"));
        }
        let reservation = crypto::hash("genesis-reservation", candidate_bytes)?;
        let tx = self.db.transaction()?;
        let saved: Option<SavedReservation> = tx.query_row(
            "SELECT reservation_hash,candidate_bytes,active,committed_bytes FROM families WHERE family_id=?1",
            params![&candidate.family_id[..]], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?)),
        ).optional()?;
        let Some((saved_hash, saved_candidate, active, committed)) = saved else {
            return Err(Error::Invalid("genesis has no staged reservation"));
        };
        if saved_hash != reservation || saved_candidate != candidate_bytes {
            return Err(Error::Invalid("genesis differs from reservation"));
        }
        if active == 1 {
            verify_stored_genesis(
                &tx,
                candidate.family_id,
                &saved_candidate,
                self.relay_public,
            )?;
            return Ok(receipt::control_commit_response(
                &committed.ok_or(Error::Invalid("active genesis missing bytes"))?,
            )?);
        }
        ensure_control_ids(&tx, candidate.family_id, candidate_bytes)?;
        for entry in &candidate.manifest {
            let staged: Option<(i64, Vec<u8>, Vec<u8>)> = tx.query_row(
                "SELECT kind,object_hash,object_bytes FROM staged_objects WHERE family_id=?1 AND object_id=?2",
                params![&candidate.family_id[..], &entry.object_id[..]], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?)),
            ).optional()?;
            let Some((kind, hash, bytes)) = staged else {
                return Err(Error::Invalid("manifest object not staged"));
            };
            if kind != i64::from(entry.kind)
                || hash != entry.object_hash
                || bytes.len() != entry.object_len as usize
                || crypto::hash("object", &bytes)? != entry.object_hash
            {
                return Err(Error::Invalid("staged object mismatch"));
            }
            tx.execute(
                "INSERT INTO committed_objects(family_id,object_id,kind,object_hash,object_bytes,transition_id) VALUES(?1,?2,?3,?4,?5,?6)",
                params![&candidate.family_id[..], &entry.object_id[..], kind, &hash, &bytes, &candidate.transition_id[..]],
            )?;
        }
        let committed =
            receipt::commit_control(candidate_bytes, &self.relay_seed, 1, committed_ms)?;
        let head = crypto::hash("control-head", &committed)?;
        tx.execute(
            "UPDATE families SET active=1, committed_bytes=?2,head_hash=?3,cursor=1 WHERE family_id=?1 AND active=0",
            params![&candidate.family_id[..], &committed, &head[..]],
        )?;
        tx.execute(
            "INSERT INTO entries(family_id,cursor,kind,committed_bytes) VALUES(?1,1,1,?2)",
            params![&candidate.family_id[..], &committed],
        )?;
        tx.execute(
            "DELETE FROM staged_objects WHERE family_id=?1",
            params![&candidate.family_id[..]],
        )?;
        tx.commit()?;
        Ok(receipt::control_commit_response(&committed)?)
    }

    pub fn genesis_result(&self, family_id: [u8; 16]) -> Result<Option<Vec<u8>>, Error> {
        let saved: Option<(Vec<u8>, Vec<u8>)> = self
            .db
            .query_row(
                "SELECT candidate_bytes,committed_bytes FROM families WHERE family_id=?1 AND active=1",
                params![&family_id[..]],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let Some((candidate, committed)) = saved else {
            return Ok(None);
        };
        verify_stored_genesis(&self.db, family_id, &candidate, self.relay_public)?;
        Ok(Some(committed))
    }

    pub fn committed_object(
        &self,
        family_id: [u8; 16],
        object_id: [u8; 16],
    ) -> Result<Option<Vec<u8>>, Error> {
        Ok(self
            .db
            .query_row(
                "SELECT object_bytes FROM committed_objects WHERE family_id=?1 AND object_id=?2",
                params![&family_id[..], &object_id[..]],
                |r| r.get(0),
            )
            .optional()?)
    }

    pub fn stage_first_issue_object(
        &mut self,
        path_family: [u8; 16],
        path_object: [u8; 16],
        body: &[u8],
    ) -> Result<Vec<u8>, Error> {
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
        let candidate_bytes = cbor::encode(&Value::Map(vec![
            (1, fields[1].1.clone()),
            (2, fields[2].1.clone()),
        ]))?;
        let kind: u16 = number(&fields[3].1)?
            .try_into()
            .map_err(|_| Error::Invalid("kind range"))?;
        let object_id = fixed::<16>(&fields[4].1)?;
        let Value::Bytes(object_bytes) = &fields[5].1 else {
            return Err(Error::Invalid("object not bytes"));
        };
        let (genesis_bytes, active, cursor, head) = self.db.query_row(
            "SELECT candidate_bytes,active,cursor,head_hash FROM families WHERE family_id=?1",
            params![&path_family[..]],
            |r| {
                Ok((
                    r.get::<_, Vec<u8>>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, Option<Vec<u8>>>(3)?,
                ))
            },
        )?;
        if active != 1 {
            return Err(Error::Invalid("Family not promoted"));
        }
        let genesis =
            verify_stored_genesis(&self.db, path_family, &genesis_bytes, self.relay_public)?;
        let controls = control_count(&self.db, path_family)?;
        let head: [u8; 32] = head
            .ok_or(Error::Invalid("Family head absent"))?
            .try_into()
            .map_err(|_| Error::Invalid("head length"))?;
        let issue = authority::verify_first_invite_issue(
            &candidate_bytes,
            &genesis,
            crypto::hash("control-head", &control_at(&self.db, path_family, 0)?)?,
        )?;
        if issue.family_id != path_family
            || issue.manifest.object_id != path_object
            || issue.manifest.object_id != object_id
            || issue.manifest.kind != kind
            || issue.manifest.object_hash != crypto::hash("object", object_bytes)?
            || issue.manifest.object_len as usize != object_bytes.len()
        {
            return Err(Error::Invalid("issue stage differs from path or manifest"));
        }
        if controls >= 2 {
            let first_issue = control_at(&self.db, path_family, 1)?;
            let existing: Option<Vec<u8>> = self.db.query_row(
                "SELECT object_bytes FROM committed_objects WHERE family_id=?1 AND object_id=?2 AND transition_id=?3",
                params![&path_family[..],&object_id[..],&issue.transition_id[..]], |r| r.get(0),
            ).optional()?;
            if control_candidate(&first_issue)? == candidate_bytes
                && existing.as_deref() == Some(object_bytes)
            {
                return Ok(receipt::object_stage_response(object_bytes)?);
            }
            return Err(Error::Invalid("issue already committed with other bytes"));
        }
        if controls != 1
            || cursor < 1
            || head != crypto::hash("control-head", &control_at(&self.db, path_family, 0)?)?
        {
            return Err(Error::Invalid("first issue only"));
        }
        let tx = self.db.transaction()?;
        stage_control_object(
            &tx,
            path_family,
            issue.transition_id,
            &candidate_bytes,
            &issue.manifest,
            object_bytes,
        )?;
        tx.commit()?;
        Ok(receipt::object_stage_response(object_bytes)?)
    }

    pub fn commit_first_issue(
        &mut self,
        path_family: [u8; 16],
        candidate_bytes: &[u8],
        committed_ms: i64,
    ) -> Result<Vec<u8>, Error> {
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (genesis_bytes,cursor,head,genesis_committed) = tx.query_row(
            "SELECT candidate_bytes,cursor,head_hash,committed_bytes FROM families WHERE family_id=?1 AND active=1",
            params![&path_family[..]], |r| Ok((r.get::<_,Vec<u8>>(0)?,r.get::<_,i64>(1)?,r.get::<_,Vec<u8>>(2)?,r.get::<_,Vec<u8>>(3)?)),
        )?;
        let genesis = verify_stored_genesis(&tx, path_family, &genesis_bytes, self.relay_public)?;
        let controls = control_count(&tx, path_family)?;
        let genesis_head = crypto::hash("control-head", &genesis_committed)?;
        let issue = authority::verify_first_invite_issue(candidate_bytes, &genesis, genesis_head)?;
        if issue.family_id != path_family {
            return Err(Error::Invalid("issue path Family mismatch"));
        }
        if controls >= 2 {
            let committed = control_at(&tx, path_family, 1)?;
            if control_candidate(&committed)? == candidate_bytes {
                return Ok(receipt::control_commit_response(&committed)?);
            }
            return Err(Error::Invalid("first issue already differs"));
        }
        if controls != 1 || head != genesis_head {
            return Err(Error::Invalid("issue prior head stale"));
        }
        let genesis_time = control_commit_time(&genesis_committed)?;
        if committed_ms < genesis_time {
            return Err(Error::Invalid("relay time moved backward"));
        }
        ensure_control_ids(&tx, path_family, candidate_bytes)?;
        let staged_candidate: Option<Vec<u8>> = tx
            .query_row(
                "SELECT candidate_bytes FROM staged_controls WHERE family_id=?1 AND transition_id=?2",
                params![&path_family[..], &issue.transition_id[..]],
                |r| r.get(0),
            )
            .optional()?;
        if staged_candidate.as_deref() != Some(candidate_bytes) {
            return Err(Error::Invalid("issue candidate not staged"));
        }
        let staged: Option<(i64,Vec<u8>,Vec<u8>)> = tx.query_row(
            "SELECT kind,object_hash,object_bytes FROM staged_control_objects WHERE family_id=?1 AND transition_id=?2 AND object_id=?3",
            params![&path_family[..],&issue.transition_id[..],&issue.manifest.object_id[..]], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?)),
        ).optional()?;
        let Some((kind, hash, bytes)) = staged else {
            return Err(Error::Invalid("issue membership object missing"));
        };
        if kind != 1
            || hash != issue.manifest.object_hash
            || bytes.len() != issue.manifest.object_len as usize
            || crypto::hash("object", &bytes)? != issue.manifest.object_hash
        {
            return Err(Error::Invalid("issue membership object changed"));
        }
        let next_cursor = cursor
            .checked_add(1)
            .ok_or(Error::Invalid("cursor overflow"))?;
        let committed = receipt::commit_control(
            candidate_bytes,
            &self.relay_seed,
            next_cursor as u64,
            committed_ms,
        )?;
        let next_head = crypto::hash("control-head", &committed)?;
        tx.execute("INSERT INTO committed_objects(family_id,object_id,kind,object_hash,object_bytes,transition_id) VALUES(?1,?2,1,?3,?4,?5)",
            params![&path_family[..],&issue.manifest.object_id[..],&hash,&bytes,&issue.transition_id[..]])?;
        tx.execute(
            "INSERT INTO entries(family_id,cursor,kind,committed_bytes) VALUES(?1,?2,1,?3)",
            params![&path_family[..], next_cursor, &committed],
        )?;
        let updated = tx.execute("UPDATE families SET cursor=?2,head_hash=?3 WHERE family_id=?1 AND cursor=?4 AND head_hash=?5",
            params![&path_family[..],next_cursor,&next_head[..],cursor,&genesis_head[..]])?;
        if updated != 1 {
            return Err(Error::Invalid("issue compare-and-swap failed"));
        }
        tx.execute(
            "DELETE FROM staged_controls WHERE family_id=?1",
            params![&path_family[..]],
        )?;
        tx.execute(
            "DELETE FROM staged_control_objects WHERE family_id=?1",
            params![&path_family[..]],
        )?;
        tx.commit()?;
        Ok(receipt::control_commit_response(&committed)?)
    }

    pub fn commit_first_claim(
        &mut self,
        path_family: [u8; 16],
        candidate_bytes: &[u8],
        committed_ms: i64,
    ) -> Result<Vec<u8>, Error> {
        self.commit_first_claim_with_clock(path_family, candidate_bytes, || Ok(committed_ms))
    }

    pub fn commit_first_claim_with_clock(
        &mut self,
        path_family: [u8; 16],
        candidate_bytes: &[u8],
        clock: impl FnOnce() -> Result<i64, Error>,
    ) -> Result<Vec<u8>, Error> {
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (genesis_bytes,cursor,head,genesis_committed)=tx.query_row(
            "SELECT candidate_bytes,cursor,head_hash,committed_bytes FROM families WHERE family_id=?1 AND active=1",
            params![&path_family[..]],|r|Ok((r.get::<_,Vec<u8>>(0)?,r.get::<_,i64>(1)?,r.get::<_,Vec<u8>>(2)?,r.get::<_,Vec<u8>>(3)?)),
        )?;
        let genesis = verify_stored_genesis(&tx, path_family, &genesis_bytes, self.relay_public)?;
        let controls = control_count(&tx, path_family)?;
        let genesis_head = crypto::hash("control-head", &genesis_committed)?;
        let issue_committed = control_at(&tx, path_family, 1)?;
        let issue_candidate = control_candidate(&issue_committed)?;
        let issue = authority::verify_first_invite_issue(&issue_candidate, &genesis, genesis_head)?;
        let issue_head = crypto::hash("control-head", &issue_committed)?;
        let claim = authority::verify_first_claim(candidate_bytes, &genesis, &issue, issue_head)?;
        if claim.family_id != path_family {
            return Err(Error::Invalid("claim Family path mismatch"));
        }
        if controls >= 3 {
            let committed = control_at(&tx, path_family, 2)?;
            if control_candidate(&committed)? == candidate_bytes {
                return Ok(receipt::control_commit_response(&committed)?);
            }
            return Err(Error::Invalid(
                "invitation already claimed by another candidate",
            ));
        }
        if controls != 2 || head != issue_head {
            return Err(Error::Invalid("claim head stale"));
        }
        let issue_time = control_commit_time(&issue_committed)?;
        let expiry = issue_time
            .checked_add(604_800_000)
            .ok_or(Error::Invalid("invite expiry overflow"))?;
        ensure_control_ids(&tx, path_family, candidate_bytes)?;
        let committed_ms = clock()?;
        if committed_ms < issue_time || committed_ms >= expiry {
            return Err(Error::Invalid("claim expired or relay time moved backward"));
        }
        let next_cursor = cursor
            .checked_add(1)
            .ok_or(Error::Invalid("cursor overflow"))?;
        let committed = receipt::commit_control(
            candidate_bytes,
            &self.relay_seed,
            next_cursor as u64,
            committed_ms,
        )?;
        let next_head = crypto::hash("control-head", &committed)?;
        tx.execute(
            "INSERT INTO entries(family_id,cursor,kind,committed_bytes) VALUES(?1,?2,1,?3)",
            params![&path_family[..], next_cursor, &committed],
        )?;
        let updated=tx.execute("UPDATE families SET cursor=?2,head_hash=?3 WHERE family_id=?1 AND cursor=?4 AND head_hash=?5",
            params![&path_family[..],next_cursor,&next_head[..],cursor,&issue_head[..]])?;
        if updated != 1 {
            return Err(Error::Invalid("claim compare-and-swap failed"));
        }
        tx.commit()?;
        Ok(receipt::control_commit_response(&committed)?)
    }

    /// Commit a no-object manager transition against the authenticated
    /// current public ledger. Exact retries are resolved before current
    /// authority is checked, since a later control may have changed it.
    pub fn commit_manager_change_with_clock(
        &mut self,
        family: [u8; 16],
        candidate: &[u8],
        clock: impl FnOnce() -> Result<i64, Error>,
    ) -> Result<Vec<u8>, Error> {
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let ids = control_birth_ids(candidate)?;
        if ids.len() != 1 {
            return Err(Error::Invalid("manager change has new object IDs"));
        }
        let Value::Map(root) = cbor::decode(candidate)? else {
            return Err(Error::Invalid("manager change not map"));
        };
        let Some((1, Value::Map(unsigned))) = root.first() else {
            return Err(Error::Invalid("manager change unsigned absent"));
        };
        if !matches!(number(&unsigned[5].1)?, 3 | 7 | 9) {
            return Err(Error::Invalid("not a no-object manager change"));
        }
        let transition_id = ids[0];
        let mut controls = tx.prepare(
            "SELECT committed_bytes FROM entries WHERE family_id=?1 AND kind=1 ORDER BY cursor",
        )?;
        let rows = controls.query_map(params![&family[..]], |row| row.get::<_, Vec<u8>>(0))?;
        for row in rows {
            let committed = row?;
            if control_birth_ids(&committed)?.first() == Some(&transition_id) {
                if control_candidate(&committed)? == candidate {
                    return Ok(receipt::control_commit_response(&committed)?);
                }
                return Err(Error::Invalid("transition ID reused with different bytes"));
            }
        }
        drop(controls);

        let mut ledger =
            Self::verify_saved_family(&tx, family, self.relay_public, &self.relay_seed)?;
        let (cursor, saved_head): (i64, Vec<u8>) = tx.query_row(
            "SELECT cursor,head_hash FROM families WHERE family_id=?1 AND active=1",
            params![&family[..]],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        if saved_head != ledger.head() {
            return Err(Error::Invalid("manager change head differs"));
        }
        ensure_control_ids(&tx, family, candidate)?;
        let next_cursor = cursor
            .checked_add(1)
            .ok_or(Error::Invalid("cursor overflow"))?;
        let committed = receipt::commit_control(
            candidate,
            &self.relay_seed,
            u64::try_from(next_cursor).map_err(|_| Error::Invalid("cursor range"))?,
            clock()?,
        )?;
        let verified = receipt::verify_control_receipt(&committed, &self.relay_public)?;
        if !matches!(verified.kind, 3 | 7 | 9)
            || verified.family_id != family
            || !verified.manifest.is_empty()
        {
            return Err(Error::Invalid("not a no-object manager change"));
        }
        ledger.apply(&verified, &committed)?;
        let next_head = ledger.head();
        tx.execute(
            "INSERT INTO entries(family_id,cursor,kind,committed_bytes) VALUES(?1,?2,1,?3)",
            params![&family[..], next_cursor, &committed],
        )?;
        let updated = tx.execute(
            "UPDATE families SET cursor=?2,head_hash=?3 WHERE family_id=?1 AND cursor=?4 AND head_hash=?5",
            params![&family[..], next_cursor, &next_head[..], cursor, &saved_head],
        )?;
        if updated != 1 {
            return Err(Error::Invalid("manager change compare-and-swap failed"));
        }
        tx.commit()?;
        Ok(receipt::control_commit_response(&committed)?)
    }

    pub fn stage_first_challenge_object(
        &mut self,
        path_family: [u8; 16],
        path_object: [u8; 16],
        body: &[u8],
    ) -> Result<Vec<u8>, Error> {
        let (candidate_bytes, kind, object_id, object_bytes) = stage_parts(body)?;
        let prefix = load_join_prefix(&self.db, self.relay_public, path_family)?;
        let challenge = authority::verify_first_challenge(
            &candidate_bytes,
            &prefix.genesis,
            &prefix.issue,
            &prefix.claim,
            prefix.claim_head,
        )?;
        let listed = challenge
            .manifest
            .iter()
            .find(|entry| entry.kind == kind && entry.object_id == object_id)
            .ok_or(Error::Invalid("challenge object absent from manifest"))?;
        if object_id != path_object
            || listed.object_hash != crypto::hash("object", &object_bytes)?
            || listed.object_len as usize != object_bytes.len()
        {
            return Err(Error::Invalid("challenge object path or bytes mismatch"));
        }
        if prefix.controls >= 4 {
            let committed = control_at(&self.db, path_family, 3)?;
            let existing:Option<Vec<u8>>=self.db.query_row(
                "SELECT object_bytes FROM committed_objects WHERE family_id=?1 AND object_id=?2 AND transition_id=?3",
                params![&path_family[..],&object_id[..],&challenge.transition_id[..]],|r|r.get(0),
            ).optional()?;
            if control_candidate(&committed)? == candidate_bytes
                && existing.as_deref() == Some(&object_bytes)
            {
                return Ok(receipt::object_stage_response(&object_bytes)?);
            }
            return Err(Error::Invalid("challenge already committed differently"));
        }
        if prefix.controls != 3 || prefix.head != prefix.claim_head {
            return Err(Error::Invalid("challenge head stale"));
        }
        let tx = self.db.transaction()?;
        stage_control_object(
            &tx,
            path_family,
            challenge.transition_id,
            &candidate_bytes,
            listed,
            &object_bytes,
        )?;
        tx.commit()?;
        Ok(receipt::object_stage_response(&object_bytes)?)
    }

    pub fn commit_first_challenge(
        &mut self,
        path_family: [u8; 16],
        candidate_bytes: &[u8],
        committed_ms: i64,
    ) -> Result<Vec<u8>, Error> {
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let prefix = load_join_prefix(&tx, self.relay_public, path_family)?;
        let challenge = authority::verify_first_challenge(
            candidate_bytes,
            &prefix.genesis,
            &prefix.issue,
            &prefix.claim,
            prefix.claim_head,
        )?;
        if prefix.controls >= 4 {
            let committed = control_at(&tx, path_family, 3)?;
            if control_candidate(&committed)? == candidate_bytes {
                return Ok(receipt::control_commit_response(&committed)?);
            }
            return Err(Error::Invalid("challenge already committed differently"));
        }
        if prefix.controls != 3
            || prefix.head != prefix.claim_head
            || committed_ms < prefix.claim_time
        {
            return Err(Error::Invalid("challenge head or relay time invalid"));
        }
        ensure_control_ids(&tx, path_family, candidate_bytes)?;
        let staged:Option<Vec<u8>>=tx.query_row("SELECT candidate_bytes FROM staged_controls WHERE family_id=?1 AND transition_id=?2",
            params![&path_family[..],&challenge.transition_id[..]],|r|r.get(0)).optional()?;
        if staged.as_deref() != Some(candidate_bytes) {
            return Err(Error::Invalid("challenge candidate not staged"));
        }
        for entry in &challenge.manifest {
            let staged:Option<(i64,Vec<u8>,Vec<u8>)>=tx.query_row(
                "SELECT kind,object_hash,object_bytes FROM staged_control_objects WHERE family_id=?1 AND transition_id=?2 AND object_id=?3",
                params![&path_family[..],&challenge.transition_id[..],&entry.object_id[..]],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)),
            ).optional()?;
            let Some((kind, hash, bytes)) = staged else {
                return Err(Error::Invalid("challenge object missing"));
            };
            if kind != i64::from(entry.kind)
                || hash != entry.object_hash
                || bytes.len() != entry.object_len as usize
                || crypto::hash("object", &bytes)? != entry.object_hash
            {
                return Err(Error::Invalid("challenge staged bytes changed"));
            }
            tx.execute("INSERT INTO committed_objects(family_id,object_id,kind,object_hash,object_bytes,transition_id) VALUES(?1,?2,?3,?4,?5,?6)",
                params![&path_family[..],&entry.object_id[..],kind,&hash,&bytes,&challenge.transition_id[..]])?;
        }
        let next_cursor = prefix
            .cursor
            .checked_add(1)
            .ok_or(Error::Invalid("cursor overflow"))?;
        let committed = receipt::commit_control(
            candidate_bytes,
            &self.relay_seed,
            next_cursor as u64,
            committed_ms,
        )?;
        let next_head = crypto::hash("control-head", &committed)?;
        tx.execute(
            "INSERT INTO entries(family_id,cursor,kind,committed_bytes) VALUES(?1,?2,1,?3)",
            params![&path_family[..], next_cursor, &committed],
        )?;
        let updated=tx.execute("UPDATE families SET cursor=?2,head_hash=?3 WHERE family_id=?1 AND cursor=?4 AND head_hash=?5",
            params![&path_family[..],next_cursor,&next_head[..],prefix.cursor,&prefix.claim_head[..]])?;
        if updated != 1 {
            return Err(Error::Invalid("challenge compare-and-swap failed"));
        }
        tx.execute(
            "DELETE FROM staged_controls WHERE family_id=?1",
            params![&path_family[..]],
        )?;
        tx.execute(
            "DELETE FROM staged_control_objects WHERE family_id=?1",
            params![&path_family[..]],
        )?;
        tx.commit()?;
        Ok(receipt::control_commit_response(&committed)?)
    }

    pub fn commit_first_proof(
        &mut self,
        path_family: [u8; 16],
        candidate_bytes: &[u8],
        committed_ms: i64,
    ) -> Result<Vec<u8>, Error> {
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let prefix = load_join_prefix(&tx, self.relay_public, path_family)?;
        if prefix.controls < 4 {
            return Err(Error::Invalid("challenge not committed"));
        }
        let challenge_committed = control_at(&tx, path_family, 3)?;
        let challenge = authority::verify_first_challenge(
            &control_candidate(&challenge_committed)?,
            &prefix.genesis,
            &prefix.issue,
            &prefix.claim,
            prefix.claim_head,
        )?;
        let challenge_head = crypto::hash("control-head", &challenge_committed)?;
        let _proof = authority::verify_first_proof(
            candidate_bytes,
            &prefix.genesis,
            &prefix.issue,
            &prefix.claim,
            &challenge,
            challenge_head,
        )?;
        if prefix.controls >= 5 {
            let committed = control_at(&tx, path_family, 4)?;
            if control_candidate(&committed)? == candidate_bytes {
                return Ok(receipt::control_commit_response(&committed)?);
            }
            return Err(Error::Invalid("proof already committed differently"));
        }
        if prefix.controls != 4
            || prefix.head != challenge_head
            || committed_ms < control_commit_time(&challenge_committed)?
        {
            return Err(Error::Invalid("proof head or relay time invalid"));
        }
        let next_cursor = prefix
            .cursor
            .checked_add(1)
            .ok_or(Error::Invalid("cursor overflow"))?;
        let committed = receipt::commit_control(
            candidate_bytes,
            &self.relay_seed,
            next_cursor as u64,
            committed_ms,
        )?;
        let next_head = crypto::hash("control-head", &committed)?;
        ensure_control_ids(&tx, path_family, candidate_bytes)?;
        tx.execute(
            "INSERT INTO entries(family_id,cursor,kind,committed_bytes) VALUES(?1,?2,1,?3)",
            params![&path_family[..], next_cursor, &committed],
        )?;
        let updated=tx.execute("UPDATE families SET cursor=?2,head_hash=?3 WHERE family_id=?1 AND cursor=?4 AND head_hash=?5",
            params![&path_family[..],next_cursor,&next_head[..],prefix.cursor,&challenge_head[..]])?;
        if updated != 1 {
            return Err(Error::Invalid("proof compare-and-swap failed"));
        }
        tx.commit()?;
        Ok(receipt::control_commit_response(&committed)?)
    }

    pub fn stage_first_admission_object(
        &mut self,
        path_family: [u8; 16],
        path_object: [u8; 16],
        body: &[u8],
    ) -> Result<Vec<u8>, Error> {
        let (candidate_bytes, kind, object_id, object_bytes) = stage_parts(body)?;
        let prefix = load_proved_prefix(&self.db, self.relay_public, path_family)?;
        let admission = authority::verify_first_admission(
            &candidate_bytes,
            &prefix.join.genesis,
            &prefix.join.issue,
            &prefix.join.claim,
            &prefix.challenge,
            &prefix.proof,
            prefix.proof_head,
        )?;
        let listed = admission
            .manifest
            .iter()
            .find(|entry| entry.kind == kind && entry.object_id == object_id)
            .ok_or(Error::Invalid("admission object absent from manifest"))?;
        if object_id != path_object
            || listed.object_hash != crypto::hash("object", &object_bytes)?
            || listed.object_len as usize != object_bytes.len()
        {
            return Err(Error::Invalid("admission object path or bytes mismatch"));
        }
        if prefix.join.controls >= 6 {
            let committed = control_at(&self.db, path_family, 5)?;
            let existing:Option<Vec<u8>>=self.db.query_row(
                "SELECT object_bytes FROM committed_objects WHERE family_id=?1 AND object_id=?2 AND transition_id=?3",
                params![&path_family[..],&object_id[..],&admission.transition_id[..]],|r|r.get(0),
            ).optional()?;
            if control_candidate(&committed)? == candidate_bytes
                && existing.as_deref() == Some(&object_bytes)
            {
                return Ok(receipt::object_stage_response(&object_bytes)?);
            }
            return Err(Error::Invalid("admission already committed differently"));
        }
        if prefix.join.controls != 5 || prefix.join.head != prefix.proof_head {
            return Err(Error::Invalid("admission head stale"));
        }
        if kind == 4 {
            authority::validate_first_admission_grant(&admission, object_id, &object_bytes)?;
        }
        let tx = self.db.transaction()?;
        stage_control_object(
            &tx,
            path_family,
            admission.transition_id,
            &candidate_bytes,
            listed,
            &object_bytes,
        )?;
        tx.commit()?;
        Ok(receipt::object_stage_response(&object_bytes)?)
    }

    pub fn commit_first_admission(
        &mut self,
        path_family: [u8; 16],
        candidate_bytes: &[u8],
        committed_ms: i64,
    ) -> Result<Vec<u8>, Error> {
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let prefix = load_proved_prefix(&tx, self.relay_public, path_family)?;
        let admission = authority::verify_first_admission(
            candidate_bytes,
            &prefix.join.genesis,
            &prefix.join.issue,
            &prefix.join.claim,
            &prefix.challenge,
            &prefix.proof,
            prefix.proof_head,
        )?;
        if prefix.join.controls >= 6 {
            let committed = control_at(&tx, path_family, 5)?;
            if control_candidate(&committed)? == candidate_bytes {
                return Ok(receipt::control_commit_response(&committed)?);
            }
            return Err(Error::Invalid("admission already committed differently"));
        }
        if prefix.join.controls != 5
            || prefix.join.head != prefix.proof_head
            || committed_ms < prefix.proof_time
        {
            return Err(Error::Invalid("admission head or relay time invalid"));
        }
        ensure_control_ids(&tx, path_family, candidate_bytes)?;
        let staged:Option<Vec<u8>>=tx.query_row("SELECT candidate_bytes FROM staged_controls WHERE family_id=?1 AND transition_id=?2",
            params![&path_family[..],&admission.transition_id[..]],|r|r.get(0)).optional()?;
        if staged.as_deref() != Some(candidate_bytes) {
            return Err(Error::Invalid("admission candidate not staged"));
        }
        for entry in &admission.manifest {
            let staged:Option<(i64,Vec<u8>,Vec<u8>)>=tx.query_row(
                "SELECT kind,object_hash,object_bytes FROM staged_control_objects WHERE family_id=?1 AND transition_id=?2 AND object_id=?3",
                params![&path_family[..],&admission.transition_id[..],&entry.object_id[..]],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)),
            ).optional()?;
            let Some((kind, hash, bytes)) = staged else {
                return Err(Error::Invalid("admission object missing"));
            };
            if kind != i64::from(entry.kind)
                || hash != entry.object_hash
                || bytes.len() != entry.object_len as usize
                || crypto::hash("object", &bytes)? != entry.object_hash
            {
                return Err(Error::Invalid("admission staged bytes changed"));
            }
            if entry.kind == 4 {
                authority::validate_first_admission_grant(&admission, entry.object_id, &bytes)?;
            }
            tx.execute("INSERT INTO committed_objects(family_id,object_id,kind,object_hash,object_bytes,transition_id) VALUES(?1,?2,?3,?4,?5,?6)",
                params![&path_family[..],&entry.object_id[..],kind,&hash,&bytes,&admission.transition_id[..]])?;
        }
        let next_cursor = prefix
            .join
            .cursor
            .checked_add(1)
            .ok_or(Error::Invalid("cursor overflow"))?;
        let committed = receipt::commit_control(
            candidate_bytes,
            &self.relay_seed,
            next_cursor as u64,
            committed_ms,
        )?;
        let next_head = crypto::hash("control-head", &committed)?;
        tx.execute(
            "INSERT INTO entries(family_id,cursor,kind,committed_bytes) VALUES(?1,?2,1,?3)",
            params![&path_family[..], next_cursor, &committed],
        )?;
        let updated=tx.execute("UPDATE families SET cursor=?2,head_hash=?3 WHERE family_id=?1 AND cursor=?4 AND head_hash=?5",
            params![&path_family[..],next_cursor,&next_head[..],prefix.join.cursor,&prefix.proof_head[..]])?;
        if updated != 1 {
            return Err(Error::Invalid("admission compare-and-swap failed"));
        }
        tx.execute(
            "DELETE FROM staged_controls WHERE family_id=?1",
            params![&path_family[..]],
        )?;
        tx.execute(
            "DELETE FROM staged_control_objects WHERE family_id=?1",
            params![&path_family[..]],
        )?;
        tx.commit()?;
        Ok(receipt::control_commit_response(&committed)?)
    }

    pub fn stage_first_removal_object(
        &mut self,
        path_family: [u8; 16],
        path_object: [u8; 16],
        body: &[u8],
    ) -> Result<Vec<u8>, Error> {
        let (candidate_bytes, kind, object_id, object_bytes) = stage_parts(body)?;
        let prefix = load_admitted_prefix(&self.db, self.relay_public, path_family)?;
        let removal = authority::verify_first_removal(
            &candidate_bytes,
            &prefix.proved.join.genesis,
            &prefix.proved.join.issue,
            &prefix.proved.join.claim,
            &prefix.admission,
            prefix.admission_head,
        )?;
        let listed = removal
            .manifest
            .iter()
            .find(|entry| entry.kind == kind && entry.object_id == object_id)
            .ok_or(Error::Invalid("removal object absent from manifest"))?;
        if object_id != path_object
            || listed.object_hash != crypto::hash("object", &object_bytes)?
            || listed.object_len as usize != object_bytes.len()
        {
            return Err(Error::Invalid("removal object path or bytes mismatch"));
        }
        validate_first_removal_object(
            kind,
            object_id,
            &object_bytes,
            &candidate_bytes,
            &prefix.proved.join.genesis,
        )?;
        if prefix.proved.join.controls >= 7 {
            let committed = control_at(&self.db, path_family, 6)?;
            let existing: Option<Vec<u8>> = self.db.query_row(
                "SELECT object_bytes FROM committed_objects WHERE family_id=?1 AND object_id=?2 AND transition_id=?3",
                params![&path_family[..], &object_id[..], &removal.transition_id[..]], |r| r.get(0),
            ).optional()?;
            if control_candidate(&committed)? == candidate_bytes
                && existing.as_deref() == Some(&object_bytes)
            {
                return Ok(receipt::object_stage_response(&object_bytes)?);
            }
            return Err(Error::Invalid("removal already committed differently"));
        }
        if prefix.proved.join.controls != 6 || prefix.proved.join.head != prefix.admission_head {
            return Err(Error::Invalid("removal head stale"));
        }
        ensure_first_removal_ids_unused(&self.db, path_family, &removal)?;
        let tx = self.db.transaction()?;
        stage_control_object(
            &tx,
            path_family,
            removal.transition_id,
            &candidate_bytes,
            listed,
            &object_bytes,
        )?;
        tx.commit()?;
        Ok(receipt::object_stage_response(&object_bytes)?)
    }

    pub fn commit_first_removal(
        &mut self,
        path_family: [u8; 16],
        candidate_bytes: &[u8],
        committed_ms: i64,
    ) -> Result<Vec<u8>, Error> {
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let prefix = load_admitted_prefix(&tx, self.relay_public, path_family)?;
        let removal = authority::verify_first_removal(
            candidate_bytes,
            &prefix.proved.join.genesis,
            &prefix.proved.join.issue,
            &prefix.proved.join.claim,
            &prefix.admission,
            prefix.admission_head,
        )?;
        if prefix.proved.join.controls >= 7 {
            let committed = control_at(&tx, path_family, 6)?;
            if control_candidate(&committed)? == candidate_bytes {
                return Ok(receipt::control_commit_response(&committed)?);
            }
            return Err(Error::Invalid("removal already committed differently"));
        }
        if prefix.proved.join.controls != 6
            || prefix.proved.join.head != prefix.admission_head
            || committed_ms < prefix.admission_time
        {
            return Err(Error::Invalid("removal head or relay time invalid"));
        }
        ensure_first_removal_ids_unused(&tx, path_family, &removal)?;
        ensure_control_ids(&tx, path_family, candidate_bytes)?;
        let saved: Option<Vec<u8>> = tx.query_row(
            "SELECT candidate_bytes FROM staged_controls WHERE family_id=?1 AND transition_id=?2",
            params![&path_family[..], &removal.transition_id[..]], |r| r.get(0),
        ).optional()?;
        if saved.as_deref() != Some(candidate_bytes) {
            return Err(Error::Invalid("removal candidate not staged"));
        }
        for entry in &removal.manifest {
            let staged: Option<(i64, Vec<u8>, Vec<u8>)> = tx.query_row(
                "SELECT kind,object_hash,object_bytes FROM staged_control_objects WHERE family_id=?1 AND transition_id=?2 AND object_id=?3",
                params![&path_family[..], &removal.transition_id[..], &entry.object_id[..]],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            ).optional()?;
            let Some((kind, hash, bytes)) = staged else {
                return Err(Error::Invalid("removal object missing"));
            };
            if kind != i64::from(entry.kind)
                || hash != entry.object_hash
                || bytes.len() != entry.object_len as usize
                || crypto::hash("object", &bytes)? != entry.object_hash
            {
                return Err(Error::Invalid("removal staged bytes changed"));
            }
            validate_first_removal_object(
                entry.kind,
                entry.object_id,
                &bytes,
                candidate_bytes,
                &prefix.proved.join.genesis,
            )?;
            tx.execute(
                "INSERT INTO committed_objects(family_id,object_id,kind,object_hash,object_bytes,transition_id) VALUES(?1,?2,?3,?4,?5,?6)",
                params![&path_family[..], &entry.object_id[..], kind, &hash, &bytes, &removal.transition_id[..]],
            )?;
        }
        let next_cursor = prefix
            .proved
            .join
            .cursor
            .checked_add(1)
            .ok_or(Error::Invalid("cursor overflow"))?;
        let committed = receipt::commit_control(
            candidate_bytes,
            &self.relay_seed,
            next_cursor as u64,
            committed_ms,
        )?;
        let next_head = crypto::hash("control-head", &committed)?;
        tx.execute(
            "INSERT INTO entries(family_id,cursor,kind,committed_bytes) VALUES(?1,?2,1,?3)",
            params![&path_family[..], next_cursor, &committed],
        )?;
        let updated = tx.execute(
            "UPDATE families SET cursor=?2,head_hash=?3 WHERE family_id=?1 AND cursor=?4 AND head_hash=?5",
            params![&path_family[..], next_cursor, &next_head[..], prefix.proved.join.cursor, &prefix.admission_head[..]],
        )?;
        if updated != 1 {
            return Err(Error::Invalid("removal compare-and-swap failed"));
        }
        tx.execute(
            "DELETE FROM staged_controls WHERE family_id=?1",
            params![&path_family[..]],
        )?;
        tx.execute(
            "DELETE FROM staged_control_objects WHERE family_id=?1",
            params![&path_family[..]],
        )?;
        tx.commit()?;
        Ok(receipt::control_commit_response(&committed)?)
    }

    /// Accept an epoch-one batch from the initial manager or first admitted
    /// recipient. The manager remains authorized throughout the join, so its
    /// durable outbox may refer to any committed first-cohort control ancestor.
    pub fn commit_initial_cohort_batch(
        &mut self,
        path_family: [u8; 16],
        envelope_bytes: &[u8],
    ) -> Result<Vec<u8>, Error> {
        let (claimed_id, author) = batch_authority::claimed_identity(envelope_bytes)?;
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let prior: Option<(Vec<u8>, Vec<u8>)> = tx.query_row(
            "SELECT envelope_bytes,receipt_bytes FROM batch_results WHERE family_id=?1 AND batch_id=?2",
            params![&path_family[..], &claimed_id[..]],
            |r| Ok((r.get(0)?, r.get(1)?)),
        ).optional()?;
        if let Some((old_envelope, old_receipt)) = prior {
            if old_envelope != envelope_bytes {
                return Err(Error::Invalid("batch ID reused with different bytes"));
            }
            return Ok(receipt::batch_commit_response(&old_receipt)?);
        }
        let prior_rejection: Option<(Vec<u8>, Vec<u8>)> = tx.query_row(
            "SELECT envelope_bytes,receipt_bytes FROM rejected_batch_results WHERE family_id=?1 AND batch_id=?2",
            params![&path_family[..], &claimed_id[..]],
            |r| Ok((r.get(0)?, r.get(1)?)),
        ).optional()?;
        if let Some((old_envelope, old_receipt)) = prior_rejection {
            if old_envelope != envelope_bytes {
                return Err(Error::Invalid("batch ID reused with different bytes"));
            }
            return Ok(receipt::batch_commit_response(&old_receipt)?);
        }
        let controls = control_count(&tx, path_family)?;
        if !(1..=7).contains(&controls) {
            return Err(Error::Invalid("initial-cohort batch authority superseded"));
        }
        let (genesis_bytes, genesis_committed): (Vec<u8>, Vec<u8>) = tx.query_row(
            "SELECT candidate_bytes,committed_bytes FROM families WHERE family_id=?1 AND active=1",
            params![&path_family[..]],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        let genesis = verify_stored_genesis(&tx, path_family, &genesis_bytes, self.relay_public)?;
        let (signer, active_author) = if author == genesis.manager_id {
            (genesis.manager_signing_key, true)
        } else if controls >= 3 {
            let prefix = load_join_prefix(&tx, self.relay_public, path_family)?;
            if author != prefix.claim.device_id {
                return Err(Error::Invalid("batch author not enrolled"));
            }
            (prefix.claim.signing_public, controls == 6)
        } else {
            return Err(Error::Invalid("batch author not enrolled"));
        };
        let batch = batch_authority::verify(envelope_bytes, path_family, genesis.relay_id, signer)?;
        if batch.family_id != path_family
            || batch.relay_id != genesis.relay_id
            || batch.author_id != author
        {
            return Err(Error::Invalid("batch identity mismatch"));
        }
        if batch.batch_id != claimed_id || batch.author_id != author {
            return Err(Error::Invalid("batch claimed identity mismatch"));
        }
        ensure_new_protocol_ids(&tx, path_family, &[batch.batch_id])?;
        let genesis_head = crypto::hash("control-head", &genesis_committed)?;
        let known_ancestor = if author == genesis.manager_id {
            let mut found = false;
            for ordinal in 0..controls {
                let committed = control_at(&tx, path_family, ordinal)?;
                found |= crypto::hash("control-head", &committed)? == batch.control_head;
            }
            found
        } else {
            crypto::hash("control-head", &control_at(&tx, path_family, 5)?)? == batch.control_head
        };
        let (cursor, current_head): (i64, Vec<u8>) = tx.query_row(
            "SELECT cursor,head_hash FROM families WHERE family_id=?1 AND active=1",
            params![&path_family[..]],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        let current_head: [u8; 32] = current_head
            .try_into()
            .map_err(|_| Error::Invalid("Family head length"))?;
        let last: Option<i64> = tx.query_row(
            "SELECT MAX(sequence) FROM batch_results WHERE family_id=?1 AND author_id=?2",
            params![&path_family[..], &author[..]],
            |r| r.get(0),
        )?;
        let next_sequence = u64::try_from(last.unwrap_or(0))
            .map_err(|_| Error::Invalid("stored sequence negative"))?
            .checked_add(1)
            .ok_or(Error::Invalid("sequence overflow"))?;
        let current_epoch = if controls == 7 { 2 } else { 1 };
        let reason = if !active_author {
            Some(2)
        } else if batch.epoch != current_epoch {
            Some(1)
        } else if !known_ancestor
            || (controls == 1 && batch.control_head != genesis_head)
            || (controls == 7 && batch.control_head != current_head)
        {
            Some(3)
        } else if batch.sequence != next_sequence {
            Some(4)
        } else {
            None
        };
        if let Some(reason) = reason {
            let rejected = receipt::rejected_batch(
                &batch,
                u64::try_from(cursor).map_err(|_| Error::Invalid("cursor negative"))?,
                current_head,
                reason,
                next_sequence,
                &self.relay_seed,
            )?;
            tx.execute(
                "INSERT INTO rejected_batch_results(family_id,batch_id,envelope_bytes,receipt_bytes) VALUES(?1,?2,?3,?4)",
                params![&path_family[..], &batch.batch_id[..], envelope_bytes, &rejected],
            )?;
            tx.commit()?;
            return Ok(receipt::batch_commit_response(&rejected)?);
        }
        let next_cursor: u64 = u64::try_from(cursor)
            .map_err(|_| Error::Invalid("cursor negative"))?
            .checked_add(1)
            .ok_or(Error::Invalid("cursor overflow"))?;
        let receipt = receipt::accepted_batch(&batch, next_cursor, &self.relay_seed)?;
        tx.execute(
            "INSERT INTO entries(family_id,cursor,kind,committed_bytes) VALUES(?1,?2,2,?3)",
            params![
                &path_family[..],
                i64::try_from(next_cursor).map_err(|_| Error::Invalid("cursor range"))?,
                envelope_bytes
            ],
        )?;
        tx.execute(
            "INSERT INTO batch_results(family_id,batch_id,author_id,sequence,envelope_bytes,receipt_bytes,cursor) VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![&path_family[..], &batch.batch_id[..], &author[..],
                i64::try_from(batch.sequence).map_err(|_| Error::Invalid("sequence range"))?,
                envelope_bytes, &receipt, i64::try_from(next_cursor).map_err(|_| Error::Invalid("cursor range"))?],
        )?;
        let updated = tx.execute(
            "UPDATE families SET cursor=?2 WHERE family_id=?1 AND cursor=?3 AND head_hash=?4",
            params![
                &path_family[..],
                i64::try_from(next_cursor).map_err(|_| Error::Invalid("cursor range"))?,
                cursor,
                &current_head[..]
            ],
        )?;
        if updated != 1 {
            return Err(Error::Invalid(
                "batch global cursor compare-and-swap failed",
            ));
        }
        tx.commit()?;
        Ok(receipt::batch_commit_response(&receipt)?)
    }

    pub fn batch_result_authenticated(
        &mut self,
        family_id: [u8; 16],
        batch_id: [u8; 16],
        exact_path: &str,
        auth_bytes: &[u8],
    ) -> Result<Vec<u8>, Error> {
        let expected = format!(
            "/v1/families/{}/batch-results/{}",
            lower_hex(&family_id),
            lower_hex(&batch_id)
        );
        if exact_path != expected {
            return Err(Error::Invalid("batch result path not canonical"));
        }
        let reader = self.verify_control_reader(family_id, exact_path, auth_bytes)?;
        if !matches!(
            reader,
            ControlReader::Manager | ControlReader::Active | ControlReader::Removed { .. }
        ) {
            return Err(Error::Invalid("reader cannot fetch batch results"));
        }
        let accepted: Option<(Vec<u8>, Vec<u8>)> = self
            .db
            .query_row(
                "SELECT author_id,receipt_bytes FROM batch_results WHERE family_id=?1 AND batch_id=?2",
                params![&family_id[..], &batch_id[..]],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let receipt = if let Some((author, receipt)) = accepted {
            if let ControlReader::Removed { device_id, .. } = reader
                && author.as_slice() != device_id
            {
                return Err(Error::Invalid(
                    "removed reader cannot fetch another author's result",
                ));
            }
            Some(receipt)
        } else {
            let rejected: Option<(Vec<u8>, Vec<u8>)> = self.db.query_row(
                "SELECT envelope_bytes,receipt_bytes FROM rejected_batch_results WHERE family_id=?1 AND batch_id=?2",
                params![&family_id[..], &batch_id[..]],
                |r| Ok((r.get(0)?, r.get(1)?)),
            ).optional()?;
            match rejected {
                Some((envelope, receipt)) => {
                    if let ControlReader::Removed {
                        device_id,
                        signing_public,
                        relay_id,
                    } = reader
                    {
                        let batch = batch_authority::verify(
                            &envelope,
                            family_id,
                            relay_id,
                            signing_public,
                        )?;
                        if batch.author_id != device_id || batch.batch_id != batch_id {
                            return Err(Error::Invalid(
                                "removed reader cannot fetch another author's result",
                            ));
                        }
                    }
                    Some(receipt)
                }
                None => None,
            }
        };
        Ok(cbor::encode(&Value::Map(vec![
            (1, Value::Integer(1)),
            (2, receipt.map_or(Value::Null, Value::Bytes)),
        ]))?)
    }

    pub fn promotion_result_authenticated(
        &mut self,
        family_id: [u8; 16],
        promotion_id: [u8; 16],
        exact_path: &str,
        auth_bytes: &[u8],
    ) -> Result<Vec<u8>, Error> {
        let expected_path = format!(
            "/v1/families/{}/promotions/{}",
            lower_hex(&family_id),
            lower_hex(&promotion_id)
        );
        if exact_path != expected_path {
            return Err(Error::Invalid("promotion path is not canonical"));
        }
        let saved: Option<(Vec<u8>, i64, Option<Vec<u8>>)> = self
            .db
            .query_row(
                "SELECT candidate_bytes,active,committed_bytes FROM families WHERE family_id=?1",
                params![&family_id[..]],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        let Some((candidate_bytes, active, committed)) = saved else {
            return Err(Error::Invalid("Family has no genesis reservation"));
        };
        let candidate = if active == 1 {
            verify_stored_genesis(&self.db, family_id, &candidate_bytes, self.relay_public)?
        } else {
            authority::verify_genesis_candidate(&candidate_bytes, &self.relay_public)?
        };
        if candidate.manifest.first().map(|entry| entry.object_id) != Some(promotion_id) {
            return Err(Error::Invalid("promotion ID mismatch"));
        }
        let verified = read_auth::verify_get(
            auth_bytes,
            family_id,
            candidate.relay_id,
            candidate.manager_id,
            candidate.manager_signing_key,
            exact_path,
        )?;
        self.record_read_id(family_id, &verified)?;
        let body = cbor::encode(&Value::Map(vec![
            (1, Value::Integer(1)),
            (
                2,
                if active == 1 {
                    Value::Bytes(committed.ok_or(Error::Invalid("active genesis missing bytes"))?)
                } else {
                    Value::Null
                },
            ),
        ]))?;
        Ok(body)
    }

    pub fn control_page_authenticated(
        &mut self,
        family_id: [u8; 16],
        after: u64,
        exact_path: &str,
        auth_bytes: &[u8],
    ) -> Result<Vec<u8>, Error> {
        let expected_path = format!(
            "/v1/families/{}/control?after={after}",
            lower_hex(&family_id)
        );
        if exact_path != expected_path {
            return Err(Error::Invalid("control read path not canonical"));
        }
        self.verify_control_reader(family_id, exact_path, auth_bytes)?;
        let mut statement = self.db.prepare(
            "SELECT cursor,committed_bytes FROM entries WHERE family_id=?1 AND kind=1 AND cursor>?2 ORDER BY cursor LIMIT 257"
        )?;
        let mut rows = statement.query(params![
            &family_id[..],
            i64::try_from(after).map_err(|_| Error::Invalid("cursor range"))?
        ])?;
        let mut entries = Vec::new();
        while let Some(row) = rows.next()? {
            let cursor: i64 = row.get(0)?;
            entries.push(receipt::RelayEntry {
                cursor: cursor
                    .try_into()
                    .map_err(|_| Error::Invalid("cursor range"))?,
                kind: 1,
                committed_bytes: row.get(1)?,
            });
        }
        let has_more = entries.len() > 256;
        entries.truncate(256);
        Ok(receipt::encode_control_page(
            family_id, after, &entries, has_more,
        )?)
    }

    pub fn batch_page_authenticated(
        &mut self,
        family_id: [u8; 16],
        after: u64,
        exact_path: &str,
        auth_bytes: &[u8],
    ) -> Result<Vec<u8>, Error> {
        let expected = format!(
            "/v1/families/{}/batches?after={after}",
            lower_hex(&family_id)
        );
        if exact_path != expected {
            return Err(Error::Invalid("batch page path not canonical"));
        }
        if !matches!(
            self.verify_control_reader(family_id, exact_path, auth_bytes)?,
            ControlReader::Manager | ControlReader::Active
        ) {
            return Err(Error::Invalid("reader cannot fetch batch page"));
        }
        let mut stmt = self.db.prepare(
            "SELECT cursor,committed_bytes FROM entries WHERE family_id=?1 AND kind=2 AND cursor>?2 ORDER BY cursor LIMIT 257"
        )?;
        let rows = stmt.query_map(
            params![
                &family_id[..],
                i64::try_from(after).map_err(|_| Error::Invalid("cursor range"))?
            ],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, Vec<u8>>(1)?)),
        )?;
        let mut entries = Vec::new();
        for row in rows {
            let (cursor, bytes) = row?;
            entries.push(receipt::RelayEntry {
                cursor: cursor
                    .try_into()
                    .map_err(|_| Error::Invalid("cursor range"))?,
                kind: 2,
                committed_bytes: bytes,
            });
        }
        let has_more = entries.len() > 256;
        entries.truncate(256);
        Ok(receipt::encode_batch_page(
            family_id, after, &entries, has_more,
        )?)
    }

    pub fn control_result_authenticated(
        &mut self,
        family_id: [u8; 16],
        transition_id: [u8; 16],
        exact_path: &str,
        auth_bytes: &[u8],
    ) -> Result<Vec<u8>, Error> {
        let expected = format!(
            "/v1/families/{}/control-results/{}",
            lower_hex(&family_id),
            lower_hex(&transition_id)
        );
        if exact_path != expected {
            return Err(Error::Invalid("control result path not canonical"));
        }
        self.verify_control_reader(family_id, exact_path, auth_bytes)?;
        let mut stmt = self.db.prepare(
            "SELECT committed_bytes FROM entries WHERE family_id=?1 AND kind=1 ORDER BY cursor",
        )?;
        let rows = stmt.query_map([&family_id[..]], |row| row.get::<_, Vec<u8>>(0))?;
        let mut found = None;
        for row in rows {
            let bytes = row?;
            let ids = control_birth_ids(&bytes)?;
            if ids.first() == Some(&transition_id) {
                found = Some(bytes);
                break;
            }
        }
        Ok(cbor::encode(&Value::Map(vec![
            (1, Value::Integer(1)),
            (2, found.map_or(Value::Null, Value::Bytes)),
        ]))?)
    }

    pub fn invite_authenticated(
        &mut self,
        family_id: [u8; 16],
        invitation_id: [u8; 16],
        exact_path: &str,
        auth_bytes: &[u8],
    ) -> Result<Vec<u8>, Error> {
        let expected = format!(
            "/v1/families/{}/invites/{}",
            lower_hex(&family_id),
            lower_hex(&invitation_id)
        );
        if exact_path != expected {
            return Err(Error::Invalid("invite path not canonical"));
        }
        let reader = self.verify_control_reader(family_id, exact_path, auth_bytes)?;
        let issue = control_at(&self.db, family_id, 1)?;
        let ids = control_birth_ids(&issue)?;
        if ids.last() != Some(&invitation_id) {
            return Err(Error::Invalid("invitation not committed"));
        }
        if matches!(reader, ControlReader::Invitation { .. })
            && read_auth::claimed_signer(auth_bytes)? != invitation_id
        {
            return Err(Error::Invalid("invitation signer differs"));
        }
        if !matches!(
            reader,
            ControlReader::Manager | ControlReader::Active | ControlReader::Invitation { .. }
        ) {
            return Err(Error::Invalid("reader cannot fetch invitation"));
        }
        Ok(cbor::encode(&Value::Map(vec![
            (1, Value::Integer(1)),
            (2, Value::Bytes(issue)),
        ]))?)
    }

    pub fn log_page_authenticated(
        &mut self,
        family_id: [u8; 16],
        after: u64,
        exact_path: &str,
        auth_bytes: &[u8],
    ) -> Result<Vec<u8>, Error> {
        let expected = format!("/v1/families/{}/log?after={after}", lower_hex(&family_id));
        if exact_path != expected {
            return Err(Error::Invalid("log read path not canonical"));
        }
        match self.verify_control_reader(family_id, exact_path, auth_bytes)? {
            ControlReader::Manager | ControlReader::Active => {}
            _ => return Err(Error::Invalid("reader cannot fetch full log")),
        }
        let mut stmt = self.db.prepare(
            "SELECT cursor,kind,committed_bytes FROM entries WHERE family_id=?1 AND cursor>?2 ORDER BY cursor LIMIT 257"
        )?;
        let mut rows = stmt.query(params![
            &family_id[..],
            i64::try_from(after).map_err(|_| Error::Invalid("cursor range"))?
        ])?;
        let mut entries = Vec::new();
        while let Some(row) = rows.next()? {
            let cursor: i64 = row.get(0)?;
            let kind: i64 = row.get(1)?;
            entries.push(receipt::RelayEntry {
                cursor: cursor
                    .try_into()
                    .map_err(|_| Error::Invalid("cursor range"))?,
                kind: kind
                    .try_into()
                    .map_err(|_| Error::Invalid("entry kind range"))?,
                committed_bytes: row.get(2)?,
            });
        }
        let has_more = entries.len() > 256;
        entries.truncate(256);
        Ok(receipt::encode_log_page(
            family_id, after, &entries, has_more,
        )?)
    }

    pub fn object_authenticated(
        &mut self,
        family_id: [u8; 16],
        object_id: [u8; 16],
        exact_path: &str,
        auth_bytes: &[u8],
    ) -> Result<Vec<u8>, Error> {
        let expected_path = format!(
            "/v1/families/{}/objects/{}",
            lower_hex(&family_id),
            lower_hex(&object_id)
        );
        if exact_path != expected_path {
            return Err(Error::Invalid("object read path not canonical"));
        }
        let reader = self.verify_control_reader(family_id, exact_path, auth_bytes)?;
        match reader {
            ControlReader::Manager | ControlReader::Active => {}
            ControlReader::Invitation { issue_object } if issue_object == object_id => {}
            ControlReader::Pending {
                challenge_object: Some(id),
            } if id == object_id => {}
            _ => return Err(Error::Invalid("reader cannot fetch this object")),
        }
        let object: Option<(i64,Vec<u8>,Vec<u8>)> = self.db.query_row(
            "SELECT kind,object_bytes,transition_id FROM committed_objects WHERE family_id=?1 AND object_id=?2",
            params![&family_id[..],&object_id[..]], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?)),
        ).optional()?;
        let Some((kind, bytes, transition)) = object else {
            return Err(Error::Invalid("object not committed"));
        };
        Ok(cbor::encode(&Value::Map(vec![
            (1, Value::Integer(1)),
            (2, Value::Integer(kind.into())),
            (3, Value::Bytes(object_id.to_vec())),
            (4, Value::Bytes(bytes)),
            (5, Value::Bytes(transition)),
        ]))?)
    }

    fn verify_control_reader(
        &mut self,
        family_id: [u8; 16],
        exact_path: &str,
        auth_bytes: &[u8],
    ) -> Result<ControlReader, Error> {
        let saved: Option<(Vec<u8>,Vec<u8>)> = self
            .db
            .query_row(
                "SELECT candidate_bytes,committed_bytes FROM families WHERE family_id=?1 AND active=1",
                params![&family_id[..]],
                |r| Ok((r.get(0)?,r.get(1)?)),
            )
            .optional()?;
        let (genesis_bytes, genesis_committed) =
            saved.ok_or(Error::Invalid("Family not active"))?;
        let controls = control_count(&self.db, family_id)?;
        if !(1..=7).contains(&controls) {
            return Err(Error::Invalid("initial-cohort reader authority superseded"));
        }
        let genesis =
            verify_stored_genesis(&self.db, family_id, &genesis_bytes, self.relay_public)?;
        let signer = read_auth::claimed_signer(auth_bytes)?;
        let (reader, signing_key) = if signer == genesis.manager_id {
            (ControlReader::Manager, genesis.manager_signing_key)
        } else {
            if controls < 2 {
                return Err(Error::Invalid("reader has no committed issue"));
            }
            let genesis_head = crypto::hash("control-head", &genesis_committed)?;
            let issue_committed = control_at(&self.db, family_id, 1)?;
            let issue = authority::verify_first_invite_issue(
                &control_candidate(&issue_committed)?,
                &genesis,
                genesis_head,
            )?;
            if signer == issue.invitation_id && controls == 2 {
                (
                    ControlReader::Invitation {
                        issue_object: issue.manifest.object_id,
                    },
                    issue.invite_public,
                )
            } else if controls >= 3 {
                let claim_committed = control_at(&self.db, family_id, 2)?;
                let issue_head = crypto::hash("control-head", &issue_committed)?;
                let claim = authority::verify_first_claim(
                    &control_candidate(&claim_committed)?,
                    &genesis,
                    &issue,
                    issue_head,
                )?;
                if signer != claim.device_id {
                    return Err(Error::Invalid("reader not pending device"));
                }
                if controls >= 6 {
                    let proved = load_proved_prefix(&self.db, self.relay_public, family_id)?;
                    let admitted = control_at(&self.db, family_id, 5)?;
                    let admission = authority::verify_first_admission(
                        &control_candidate(&admitted)?,
                        &proved.join.genesis,
                        &proved.join.issue,
                        &proved.join.claim,
                        &proved.challenge,
                        &proved.proof,
                        proved.proof_head,
                    )?;
                    if admission.recipient_id != signer {
                        return Err(Error::Invalid("reader not admitted device"));
                    }
                    if controls == 7 {
                        (
                            ControlReader::Removed {
                                device_id: claim.device_id,
                                signing_public: claim.signing_public,
                                relay_id: genesis.relay_id,
                            },
                            claim.signing_public,
                        )
                    } else {
                        (ControlReader::Active, claim.signing_public)
                    }
                } else {
                    let challenge_object = if controls >= 4 {
                        let challenge_committed = control_at(&self.db, family_id, 3)?;
                        let challenge = authority::verify_first_challenge(
                            &control_candidate(&challenge_committed)?,
                            &genesis,
                            &issue,
                            &claim,
                            crypto::hash("control-head", &claim_committed)?,
                        )?;
                        Some(challenge.manifest[0].object_id)
                    } else {
                        None
                    };
                    (
                        ControlReader::Pending { challenge_object },
                        claim.signing_public,
                    )
                }
            } else {
                return Err(Error::Invalid("reader has no control access"));
            }
        };
        let verified = read_auth::verify_get(
            auth_bytes,
            family_id,
            genesis.relay_id,
            signer,
            signing_key,
            exact_path,
        )?;
        self.record_read_id(family_id, &verified)?;
        Ok(reader)
    }

    fn record_read_id(
        &mut self,
        family_id: [u8; 16],
        verified: &read_auth::VerifiedRead,
    ) -> Result<(), Error> {
        let tx = self.db.transaction()?;
        let old:Option<Vec<u8>>=tx.query_row(
            "SELECT request_hash FROM read_requests WHERE family_id=?1 AND signer_id=?2 AND request_id=?3",
            params![&family_id[..],&verified.signer_id[..],&verified.request_id[..]],|r|r.get(0),
        ).optional()?;
        if let Some(hash) = old {
            if hash != verified.request_hash {
                return Err(Error::Invalid("request ID reused with different bytes"));
            }
        } else {
            tx.execute("INSERT INTO read_requests(family_id,signer_id,request_id,request_hash) VALUES(?1,?2,?3,?4)",
                params![&family_id[..],&verified.signer_id[..],&verified.request_id[..],&verified.request_hash[..]])?;
        }
        tx.commit()?;
        Ok(())
    }
}

fn lower_hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        out.push(DIGITS[(byte >> 4) as usize] as char);
        out.push(DIGITS[(byte & 15) as usize] as char);
    }
    out
}

type StagedObjectParts = (Vec<u8>, u16, [u8; 16], Vec<u8>);
fn stage_parts(body: &[u8]) -> Result<StagedObjectParts, Error> {
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

fn stage_control_object(
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

fn reserve_staged_object(
    tx: &Transaction<'_>,
    family: [u8; 16],
    object_id: [u8; 16],
    kind: u16,
    hash: [u8; 32],
) -> Result<(), Error> {
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

fn validate_first_removal_object(
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

fn ensure_first_removal_ids_unused(
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

fn control_candidate(committed: &[u8]) -> Result<Vec<u8>, Error> {
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
fn control_birth_ids(bytes: &[u8]) -> Result<Vec<[u8; 16]>, Error> {
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
fn ensure_new_protocol_ids(
    db: &Connection,
    family: [u8; 16],
    candidate_ids: &[[u8; 16]],
) -> Result<(), Error> {
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
    for id in candidate_ids {
        if !seen.insert(*id) {
            return Err(Error::Invalid("protocol ID reused across categories"));
        }
    }
    Ok(())
}

fn ensure_control_ids(db: &Connection, family: [u8; 16], candidate: &[u8]) -> Result<(), Error> {
    ensure_new_protocol_ids(db, family, &control_birth_ids(candidate)?)
}

// Join transitions have a fixed order, but data batches occupy the same global
// cursor space. Locate a control by its ordinal rather than assuming its cursor.
fn verify_stored_genesis(
    db: &Connection,
    family: [u8; 16],
    stored_candidate: &[u8],
    relay_public: [u8; 32],
) -> Result<authority::GenesisCandidate, Error> {
    let (committed, reservation): (Vec<u8>, Vec<u8>) = db.query_row(
        "SELECT committed_bytes,reservation_hash FROM families WHERE family_id=?1 AND active=1",
        params![&family[..]],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    if reservation != crypto::hash("genesis-reservation", stored_candidate)? {
        return Err(Error::Invalid("genesis reservation differs from candidate"));
    }
    let entry: Vec<u8> = db.query_row(
        "SELECT committed_bytes FROM entries WHERE family_id=?1 AND cursor=1 AND kind=1",
        params![&family[..]],
        |row| row.get(0),
    )?;
    if committed != entry {
        return Err(Error::Invalid(
            "genesis family row differs from committed log",
        ));
    }
    let receipt = receipt::verify_control_receipt(&entry, &relay_public)?;
    if receipt.family_id != family
        || receipt.cursor != 1
        || receipt.candidate_bytes != stored_candidate
    {
        return Err(Error::Invalid(
            "genesis candidate or receipt differs from committed log",
        ));
    }
    let genesis = authority::verify_genesis_candidate(&receipt.candidate_bytes, &relay_public)?;
    if receipt.relay_id != genesis.relay_id {
        return Err(Error::Invalid(
            "genesis relay differs from signed candidate",
        ));
    }
    Ok(genesis)
}

fn control_at(db: &Connection, family: [u8; 16], ordinal: i64) -> Result<Vec<u8>, Error> {
    Ok(db.query_row(
        "SELECT committed_bytes FROM entries WHERE family_id=?1 AND kind=1 ORDER BY cursor LIMIT 1 OFFSET ?2",
        params![&family[..], ordinal],
        |r| r.get(0),
    )?)
}

fn control_count(db: &Connection, family: [u8; 16]) -> Result<i64, Error> {
    Ok(db.query_row(
        "SELECT COUNT(*) FROM entries WHERE family_id=?1 AND kind=1",
        params![&family[..]],
        |r| r.get(0),
    )?)
}

struct JoinPrefix {
    genesis: authority::GenesisCandidate,
    issue: authority::IssueCandidate,
    claim: authority::ClaimCandidate,
    claim_head: [u8; 32],
    claim_time: i64,
    cursor: i64,
    controls: i64,
    head: [u8; 32],
}
fn load_join_prefix(
    db: &Connection,
    relay_public: [u8; 32],
    family: [u8; 16],
) -> Result<JoinPrefix, Error> {
    let (genesis_bytes,genesis_committed,cursor,head):(Vec<u8>,Vec<u8>,i64,Vec<u8>)=db.query_row(
        "SELECT candidate_bytes,committed_bytes,cursor,head_hash FROM families WHERE family_id=?1 AND active=1",
        params![&family[..]],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?)),
    )?;
    let controls = control_count(db, family)?;
    if controls < 3 {
        return Err(Error::Invalid("claim not committed"));
    }
    let genesis = verify_stored_genesis(db, family, &genesis_bytes, relay_public)?;
    let genesis_head = crypto::hash("control-head", &genesis_committed)?;
    let issue_committed = control_at(db, family, 1)?;
    let issue = authority::verify_first_invite_issue(
        &control_candidate(&issue_committed)?,
        &genesis,
        genesis_head,
    )?;
    let issue_head = crypto::hash("control-head", &issue_committed)?;
    let claim_committed = control_at(db, family, 2)?;
    let claim = authority::verify_first_claim(
        &control_candidate(&claim_committed)?,
        &genesis,
        &issue,
        issue_head,
    )?;
    let claim_head = crypto::hash("control-head", &claim_committed)?;
    Ok(JoinPrefix {
        genesis,
        issue,
        claim,
        claim_head,
        claim_time: control_commit_time(&claim_committed)?,
        cursor,
        controls,
        head: head
            .try_into()
            .map_err(|_| Error::Invalid("Family head length"))?,
    })
}

struct ProvedPrefix {
    join: JoinPrefix,
    challenge: authority::ChallengeCandidate,
    proof: authority::ProofCandidate,
    proof_head: [u8; 32],
    proof_time: i64,
}
struct AdmittedPrefix {
    proved: ProvedPrefix,
    admission: authority::AdmissionCandidate,
    admission_head: [u8; 32],
    admission_time: i64,
}
fn load_admitted_prefix(
    db: &Connection,
    relay_public: [u8; 32],
    family: [u8; 16],
) -> Result<AdmittedPrefix, Error> {
    let proved = load_proved_prefix(db, relay_public, family)?;
    if proved.join.controls < 6 {
        return Err(Error::Invalid("first admission not committed"));
    }
    let committed = control_at(db, family, 5)?;
    let admission = authority::verify_first_admission(
        &control_candidate(&committed)?,
        &proved.join.genesis,
        &proved.join.issue,
        &proved.join.claim,
        &proved.challenge,
        &proved.proof,
        proved.proof_head,
    )?;
    Ok(AdmittedPrefix {
        proved,
        admission,
        admission_head: crypto::hash("control-head", &committed)?,
        admission_time: control_commit_time(&committed)?,
    })
}
fn load_proved_prefix(
    db: &Connection,
    relay_public: [u8; 32],
    family: [u8; 16],
) -> Result<ProvedPrefix, Error> {
    let join = load_join_prefix(db, relay_public, family)?;
    if join.controls < 5 {
        return Err(Error::Invalid("proof not committed"));
    }
    let challenge_committed = control_at(db, family, 3)?;
    let challenge = authority::verify_first_challenge(
        &control_candidate(&challenge_committed)?,
        &join.genesis,
        &join.issue,
        &join.claim,
        join.claim_head,
    )?;
    let challenge_head = crypto::hash("control-head", &challenge_committed)?;
    let proof_committed = control_at(db, family, 4)?;
    let proof = authority::verify_first_proof(
        &control_candidate(&proof_committed)?,
        &join.genesis,
        &join.issue,
        &join.claim,
        &challenge,
        challenge_head,
    )?;
    let proof_head = crypto::hash("control-head", &proof_committed)?;
    Ok(ProvedPrefix {
        join,
        challenge,
        proof,
        proof_head,
        proof_time: control_commit_time(&proof_committed)?,
    })
}
fn control_commit_time(committed: &[u8]) -> Result<i64, Error> {
    let value = cbor::decode(committed)?;
    let Value::Map(fields) = value else {
        return Err(Error::Invalid("committed control not map"));
    };
    let Value::Array(receipt) = &fields[2].1 else {
        return Err(Error::Invalid("control receipt not array"));
    };
    if receipt.len() != 6 {
        return Err(Error::Invalid("control receipt length"));
    }
    let Value::Integer(time) = receipt[4] else {
        return Err(Error::Invalid("control time not integer"));
    };
    time.try_into()
        .map_err(|_| Error::Invalid("control time range"))
}

fn fixed<const N: usize>(value: &Value) -> Result<[u8; N], Error> {
    let Value::Bytes(bytes) = value else {
        return Err(Error::Invalid("expected bytes"));
    };
    bytes
        .as_slice()
        .try_into()
        .map_err(|_| Error::Invalid("bytes length"))
}
fn number(value: &Value) -> Result<u64, Error> {
    let Value::Integer(number) = value else {
        return Err(Error::Invalid("expected integer"));
    };
    (*number)
        .try_into()
        .map_err(|_| Error::Invalid("unsigned integer"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value as Json;
    fn hex(value: &str) -> Vec<u8> {
        (0..value.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&value[i..i + 2], 16).unwrap())
            .collect()
    }

    fn cancel_candidate(
        issue_candidate: &[u8],
        state: &Value,
        head: [u8; 32],
        manager: [u8; 16],
        manager_seed: [u8; 32],
        transition: [u8; 16],
    ) -> Vec<u8> {
        let Value::Map(root) = cbor::decode(issue_candidate).unwrap() else {
            panic!()
        };
        let Value::Map(mut unsigned) = root[0].1.clone() else {
            panic!()
        };
        let Value::Map(issue_delta) = &unsigned[6].1 else {
            panic!()
        };
        let invitation = issue_delta[0].1.clone();
        let mut next = state.clone();
        let Value::Map(next_fields) = &mut next else {
            panic!()
        };
        let Value::Array(invitations) = &mut next_fields[6].1 else {
            panic!()
        };
        let Value::Array(row) = &mut invitations[0] else {
            panic!()
        };
        row[5] = Value::Integer(3);
        unsigned[3].1 = Value::Bytes(head.to_vec());
        unsigned[4].1 = Value::Bytes(transition.to_vec());
        unsigned[5].1 = Value::Integer(3);
        unsigned[6].1 = Value::Map(vec![(1, invitation)]);
        unsigned[7].1 = Value::Bytes(
            crypto::hash("auth-state", &cbor::encode(&next).unwrap())
                .unwrap()
                .to_vec(),
        );
        unsigned[9].1 = Value::Array(vec![]);
        let core = Value::Array(
            unsigned[..9]
                .iter()
                .map(|(_, value)| value.clone())
                .collect(),
        );
        unsigned[10].1 = Value::Bytes(
            crypto::hash("transition-core", &cbor::encode(&core).unwrap())
                .unwrap()
                .to_vec(),
        );
        let unsigned_bytes = cbor::encode(&Value::Map(unsigned.clone())).unwrap();
        let signature =
            crypto::sign_cbor("control-transition", &unsigned_bytes, &manager_seed).unwrap();
        cbor::encode(&Value::Map(vec![
            (1, Value::Map(unsigned)),
            (
                2,
                Value::Array(vec![Value::Array(vec![
                    Value::Bytes(manager.to_vec()),
                    Value::Bytes(signature.to_vec()),
                ])]),
            ),
        ]))
        .unwrap()
    }

    #[test]
    fn manager_cancel_commits_from_verified_ledger_and_retries_exactly() {
        let genesis: Json = serde_json::from_str(
            &std::fs::read_to_string("../tests/vectors/api-genesis-v1.json").unwrap(),
        )
        .unwrap();
        let issue: Json =
            serde_json::from_str(&std::fs::read_to_string("../tests/vectors/api-v1.json").unwrap())
                .unwrap();
        let chain: Json = serde_json::from_str(
            &std::fs::read_to_string("../tests/vectors/contiguous-chain-v1.json").unwrap(),
        )
        .unwrap();
        let seed: [u8; 32] = hex(chain["test_only_inputs"]["relay_sign_seed_hex"]
            .as_str()
            .unwrap())
        .try_into()
        .unwrap();
        let manager_seed: [u8; 32] = hex(chain["test_only_inputs"]["manager_sign_seed_hex"]
            .as_str()
            .unwrap())
        .try_into()
        .unwrap();
        let manager: [u8; 16] = hex(chain["test_only_inputs"]["manager_device_id_hex"]
            .as_str()
            .unwrap())
        .try_into()
        .unwrap();
        let family: [u8; 16] = hex(genesis["inputs"]["family_id_hex"].as_str().unwrap())
            .try_into()
            .unwrap();
        let promotion: [u8; 16] = hex(genesis["inputs"]["promotion_id_hex"].as_str().unwrap())
            .try_into()
            .unwrap();
        let issue_object: [u8; 16] = hex("083e4567e89b42d3a456426614174000").try_into().unwrap();
        let genesis_stage = hex(genesis["inputs"]["stage_body_cbor_hex"].as_str().unwrap());
        let genesis_candidate = hex(genesis["inputs"]["commit_candidate_cbor_hex"]
            .as_str()
            .unwrap());
        let issue_stage = hex(issue["inputs"]["stage_body_cbor_hex"].as_str().unwrap());
        let issue_candidate = hex(issue["inputs"]["commit_body_cbor_hex"].as_str().unwrap());
        let genesis_response = hex(genesis["expect"]["commit_response_cbor_hex"]
            .as_str()
            .unwrap());
        let Value::Map(response) = cbor::decode(&genesis_response).unwrap() else {
            panic!()
        };
        let Value::Bytes(genesis_committed) = &response[1].1 else {
            panic!()
        };
        let genesis_ms = control_commit_time(genesis_committed).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("relay.sqlite");
        let mut store = RelayStore::open(&path, seed).unwrap();
        store
            .stage_genesis_object(family, promotion, &genesis_stage)
            .unwrap();
        store
            .commit_genesis(family, &genesis_candidate, genesis_ms)
            .unwrap();
        store
            .stage_first_issue_object(family, issue_object, &issue_stage)
            .unwrap();
        store
            .commit_first_issue(family, &issue_candidate, genesis_ms + 1_000)
            .unwrap();
        let issue_committed = control_at(&store.db, family, 1).unwrap();
        let genesis_verified =
            authority::verify_genesis_candidate(&genesis_candidate, &store.relay_public).unwrap();
        let batch = signed_manager_batch(&issue_committed, &genesis_verified, manager_seed);
        store.commit_initial_cohort_batch(family, &batch).unwrap();
        let ledger =
            RelayStore::verify_saved_family(&store.db, family, store.relay_public, &seed).unwrap();
        let candidate = cancel_candidate(
            &issue_candidate,
            ledger.state(),
            ledger.head(),
            manager,
            manager_seed,
            [0xc3; 16],
        );
        let stale = cancel_candidate(
            &issue_candidate,
            ledger.state(),
            [0; 32],
            manager,
            manager_seed,
            [0xc4; 16],
        );
        assert!(
            store
                .commit_manager_change_with_clock(family, &stale, || Ok(genesis_ms + 2_000))
                .is_err()
        );
        let wrong_signer = cancel_candidate(
            &issue_candidate,
            ledger.state(),
            ledger.head(),
            manager,
            [0; 32],
            [0xc5; 16],
        );
        assert!(
            store
                .commit_manager_change_with_clock(family, &wrong_signer, || Ok(genesis_ms + 2_000))
                .is_err()
        );
        assert!(
            store
                .commit_manager_change_with_clock(family, &candidate, || Ok(genesis_ms))
                .is_err()
        );
        let accepted = store
            .commit_manager_change_with_clock(family, &candidate, || Ok(genesis_ms + 2_000))
            .unwrap();
        let cursor: i64 = store
            .db
            .query_row(
                "SELECT cursor FROM families WHERE family_id=?1",
                [&family[..]],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(cursor, 4);
        assert_eq!(
            store
                .commit_manager_change_with_clock(family, &candidate, || panic!(
                    "retry called clock"
                ))
                .unwrap(),
            accepted
        );
        let different = cancel_candidate(
            &issue_candidate,
            ledger.state(),
            [0; 32],
            manager,
            manager_seed,
            [0xc3; 16],
        );
        assert_ne!(candidate, different);
        assert!(
            store
                .commit_manager_change_with_clock(family, &different, || panic!(
                    "duplicate called clock"
                ))
                .is_err()
        );
        drop(store);
        let mut store = RelayStore::open(&path, seed).unwrap();
        assert_eq!(
            store
                .commit_manager_change_with_clock(family, &candidate, || panic!(
                    "restart retry called clock"
                ))
                .unwrap(),
            accepted
        );
    }
    fn signed_get(
        family: [u8; 16],
        relay: [u8; 32],
        signer: [u8; 16],
        seed: [u8; 32],
        path: &str,
        request_id: [u8; 16],
    ) -> Vec<u8> {
        let request = cbor::encode(&Value::Array(vec![
            Value::Integer(1),
            Value::Bytes(family.to_vec()),
            Value::Bytes(relay.to_vec()),
            Value::Bytes(signer.to_vec()),
            Value::Bytes(request_id.to_vec()),
            Value::Text("GET".into()),
            Value::Text(path.into()),
            Value::Bytes(crypto::hash("request-body", &[]).unwrap().to_vec()),
        ]))
        .unwrap();
        let signature = crypto::sign_cbor("read-request", &request, &seed).unwrap();
        cbor::encode(&Value::Map(vec![
            (1, Value::Bytes(request)),
            (2, Value::Bytes(signature.to_vec())),
        ]))
        .unwrap()
    }

    fn competing_issue(
        candidate_bytes: &[u8],
        stage_bytes: &[u8],
        genesis: &authority::GenesisCandidate,
        manager_seed: [u8; 32],
    ) -> (Vec<u8>, Vec<u8>) {
        let Value::Map(mut root) = cbor::decode(candidate_bytes).unwrap() else {
            panic!()
        };
        let Value::Map(mut unsigned) = root[0].1.clone() else {
            panic!()
        };
        let Value::Map(delta) = unsigned[6].1.clone() else {
            panic!()
        };
        let transition = [0xa7; 16];
        unsigned[4].1 = Value::Bytes(transition.to_vec());
        let invitation = Value::Array(vec![
            delta[0].1.clone(),
            delta[1].1.clone(),
            delta[2].1.clone(),
            delta[3].1.clone(),
            Value::Bytes(transition.to_vec()),
            Value::Integer(1),
        ]);
        let state = Value::Map(vec![
            (1, Value::Integer(1)),
            (2, Value::Bytes(genesis.family_id.to_vec())),
            (3, Value::Bytes(genesis.relay_id.to_vec())),
            (4, Value::Integer(1)),
            (5, Value::Array(vec![genesis.manager_row.clone()])),
            (6, Value::Array(vec![])),
            (7, Value::Array(vec![invitation])),
        ]);
        unsigned[7].1 = Value::Bytes(
            crypto::hash("auth-state", &cbor::encode(&state).unwrap())
                .unwrap()
                .to_vec(),
        );
        let core = Value::Array(
            unsigned[..9]
                .iter()
                .map(|(_, value)| value.clone())
                .collect(),
        );
        unsigned[10].1 = Value::Bytes(
            crypto::hash("transition-core", &cbor::encode(&core).unwrap())
                .unwrap()
                .to_vec(),
        );
        let unsigned_bytes = cbor::encode(&Value::Map(unsigned.clone())).unwrap();
        let signature =
            crypto::sign_cbor("control-transition", &unsigned_bytes, &manager_seed).unwrap();
        root[0].1 = Value::Map(unsigned);
        root[1].1 = Value::Array(vec![Value::Array(vec![
            Value::Bytes(genesis.manager_id.to_vec()),
            Value::Bytes(signature.to_vec()),
        ])]);
        let Value::Map(mut stage) = cbor::decode(stage_bytes).unwrap() else {
            panic!()
        };
        stage[1].1 = root[0].1.clone();
        stage[2].1 = root[1].1.clone();
        (
            cbor::encode(&Value::Map(root)).unwrap(),
            cbor::encode(&Value::Map(stage)).unwrap(),
        )
    }

    fn issue_stage_with_changed_object(
        stage_bytes: &[u8],
        manager_id: [u8; 16],
        manager_seed: [u8; 32],
    ) -> Vec<u8> {
        let Value::Map(mut stage) = cbor::decode(stage_bytes).unwrap() else {
            panic!()
        };
        let Value::Bytes(mut object) = stage[5].1.clone() else {
            panic!()
        };
        object.push(0);
        let Value::Map(mut unsigned) = stage[1].1.clone() else {
            panic!()
        };
        let Value::Array(mut manifest) = unsigned[9].1.clone() else {
            panic!()
        };
        let Value::Array(mut entry) = manifest[0].clone() else {
            panic!()
        };
        entry[2] = Value::Bytes(crypto::hash("object", &object).unwrap().to_vec());
        entry[3] = Value::Integer(object.len() as i128);
        manifest[0] = Value::Array(entry);
        unsigned[9].1 = Value::Array(manifest);
        let signature = crypto::sign_cbor(
            "control-transition",
            &cbor::encode(&Value::Map(unsigned.clone())).unwrap(),
            &manager_seed,
        )
        .unwrap();
        stage[1].1 = Value::Map(unsigned);
        stage[2].1 = Value::Array(vec![Value::Array(vec![
            Value::Bytes(manager_id.to_vec()),
            Value::Bytes(signature.to_vec()),
        ])]);
        stage[5].1 = Value::Bytes(object);
        cbor::encode(&Value::Map(stage)).unwrap()
    }

    fn validly_resigned_genesis_with_changed_manifest(
        candidate_bytes: &[u8],
        manager_id: [u8; 16],
        manager_seed: [u8; 32],
    ) -> Vec<u8> {
        let Value::Map(mut root) = cbor::decode(candidate_bytes).unwrap() else {
            panic!()
        };
        let Value::Map(mut unsigned) = root[0].1.clone() else {
            panic!()
        };
        let Value::Array(mut manifest) = unsigned[9].1.clone() else {
            panic!()
        };
        let Value::Array(mut entry) = manifest[0].clone() else {
            panic!()
        };
        entry[2] = Value::Bytes(vec![0; 32]);
        manifest[0] = Value::Array(entry);
        unsigned[9].1 = Value::Array(manifest);
        let Value::Map(mut delta) = unsigned[6].1.clone() else {
            panic!()
        };
        delta[2].1 = Value::Bytes(vec![0; 32]);
        unsigned[6].1 = Value::Map(delta);
        let core = Value::Array(
            unsigned[..9]
                .iter()
                .map(|(_, value)| value.clone())
                .collect(),
        );
        unsigned[10].1 = Value::Bytes(
            crypto::hash("transition-core", &cbor::encode(&core).unwrap())
                .unwrap()
                .to_vec(),
        );
        let unsigned_bytes = cbor::encode(&Value::Map(unsigned.clone())).unwrap();
        let signature =
            crypto::sign_cbor("control-transition", &unsigned_bytes, &manager_seed).unwrap();
        root[0].1 = Value::Map(unsigned);
        root[1].1 = Value::Array(vec![Value::Array(vec![
            Value::Bytes(manager_id.to_vec()),
            Value::Bytes(signature.to_vec()),
        ])]);
        cbor::encode(&Value::Map(root)).unwrap()
    }

    fn signed_manager_batch(
        genesis_committed: &[u8],
        genesis: &authority::GenesisCandidate,
        manager_seed: [u8; 32],
    ) -> Vec<u8> {
        let mut batch_id = [0x91; 16];
        batch_id[6] = 0x40;
        batch_id[8] = 0x80;
        let ciphertext = vec![0x42; 17];
        let header = Value::Map(vec![
            (1, Value::Array(vec![Value::Integer(1), Value::Integer(0)])),
            (2, Value::Bytes(genesis.family_id.to_vec())),
            (3, Value::Bytes(genesis.relay_id.to_vec())),
            (
                4,
                Value::Bytes(
                    crypto::hash("control-head", genesis_committed)
                        .unwrap()
                        .to_vec(),
                ),
            ),
            (5, Value::Integer(1)),
            (6, Value::Bytes(batch_id.to_vec())),
            (7, Value::Bytes(genesis.manager_id.to_vec())),
            (8, Value::Integer(1)),
            (9, Value::Bytes(vec![0x55; 24])),
            (10, Value::Integer(1)),
        ]);
        let signed = cbor::encode(&Value::Array(vec![
            header.clone(),
            Value::Bytes(
                crypto::hash("batch-ciphertext", &ciphertext)
                    .unwrap()
                    .to_vec(),
            ),
        ]))
        .unwrap();
        let signature = crypto::sign_cbor("batch-envelope", &signed, &manager_seed).unwrap();
        cbor::encode(&Value::Map(vec![
            (1, header),
            (2, Value::Bytes(ciphertext)),
            (3, Value::Bytes(signature.to_vec())),
        ]))
        .unwrap()
    }

    fn resign_batch_author(
        envelope: &[u8],
        author_id: [u8; 16],
        sequence: u64,
        batch_id: Option<[u8; 16]>,
        author_seed: [u8; 32],
    ) -> Vec<u8> {
        let Value::Map(mut outer) = cbor::decode(envelope).unwrap() else {
            panic!()
        };
        let Value::Map(header) = &mut outer[0].1 else {
            panic!()
        };
        header[6].1 = Value::Bytes(author_id.to_vec());
        header[7].1 = Value::Integer(sequence.into());
        if let Some(batch_id) = batch_id {
            header[5].1 = Value::Bytes(batch_id.to_vec());
        }
        let Value::Bytes(ciphertext) = &outer[1].1 else {
            panic!()
        };
        let signed = cbor::encode(&Value::Array(vec![
            outer[0].1.clone(),
            Value::Bytes(
                crypto::hash("batch-ciphertext", ciphertext)
                    .unwrap()
                    .to_vec(),
            ),
        ]))
        .unwrap();
        outer[2].1 = Value::Bytes(
            crypto::sign_cbor("batch-envelope", &signed, &author_seed)
                .unwrap()
                .to_vec(),
        );
        cbor::encode(&Value::Map(outer)).unwrap()
    }

    #[test]
    fn control_and_batch_writers_share_one_family_cursor() {
        let genesis: Json = serde_json::from_str(
            &std::fs::read_to_string("../tests/vectors/api-genesis-v1.json").unwrap(),
        )
        .unwrap();
        let issue: Json =
            serde_json::from_str(&std::fs::read_to_string("../tests/vectors/api-v1.json").unwrap())
                .unwrap();
        let chain: Json = serde_json::from_str(
            &std::fs::read_to_string("../tests/vectors/contiguous-chain-v1.json").unwrap(),
        )
        .unwrap();
        let seed: [u8; 32] = hex(chain["test_only_inputs"]["relay_sign_seed_hex"]
            .as_str()
            .unwrap())
        .try_into()
        .unwrap();
        let manager_seed: [u8; 32] = hex(chain["test_only_inputs"]["manager_sign_seed_hex"]
            .as_str()
            .unwrap())
        .try_into()
        .unwrap();
        let family: [u8; 16] = hex(genesis["inputs"]["family_id_hex"].as_str().unwrap())
            .try_into()
            .unwrap();
        let promotion: [u8; 16] = hex(genesis["inputs"]["promotion_id_hex"].as_str().unwrap())
            .try_into()
            .unwrap();
        let issue_object: [u8; 16] = hex("083e4567e89b42d3a456426614174000").try_into().unwrap();
        let genesis_stage = hex(genesis["inputs"]["stage_body_cbor_hex"].as_str().unwrap());
        let genesis_candidate = hex(genesis["inputs"]["commit_candidate_cbor_hex"]
            .as_str()
            .unwrap());
        let issue_stage = hex(issue["inputs"]["stage_body_cbor_hex"].as_str().unwrap());
        let issue_candidate = hex(issue["inputs"]["commit_body_cbor_hex"].as_str().unwrap());
        let genesis_commit = hex(genesis["expect"]["commit_response_cbor_hex"]
            .as_str()
            .unwrap());
        let Value::Map(response) = cbor::decode(&genesis_commit).unwrap() else {
            panic!()
        };
        let Value::Bytes(genesis_committed) = &response[1].1 else {
            panic!()
        };
        let time = control_commit_time(genesis_committed).unwrap();
        let parsed = authority::verify_genesis_candidate(
            &genesis_candidate,
            &crypto::signing_public_key(&seed),
        )
        .unwrap();
        let batch = signed_manager_batch(genesis_committed, &parsed, manager_seed);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("relay.sqlite");
        let mut setup = RelayStore::open(&path, seed).unwrap();
        setup
            .stage_genesis_object(family, promotion, &genesis_stage)
            .unwrap();
        setup
            .commit_genesis(family, &genesis_candidate, time)
            .unwrap();
        setup
            .stage_first_issue_object(family, issue_object, &issue_stage)
            .unwrap();
        drop(setup);
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
        let writers = [true, false].map(|control| {
            let path = path.clone();
            let barrier = barrier.clone();
            let issue_candidate = issue_candidate.clone();
            let batch = batch.clone();
            std::thread::spawn(move || {
                let mut store = RelayStore::open(&path, seed).unwrap();
                barrier.wait();
                for _ in 0..100 {
                    let result = if control {
                        store.commit_first_issue(family, &issue_candidate, time)
                    } else {
                        store.commit_initial_cohort_batch(family, &batch)
                    };
                    if let Ok(result) = result {
                        return result;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
                panic!("concurrent writer did not resolve");
            })
        });
        barrier.wait();
        for writer in writers {
            assert!(!writer.join().unwrap().is_empty());
        }
        let db = Connection::open(&path).unwrap();
        let (cursor, controls, batches, head): (i64, i64, i64, Vec<u8>) = db.query_row(
            "SELECT f.cursor, (SELECT COUNT(*) FROM entries WHERE family_id=f.family_id AND kind=1), (SELECT COUNT(*) FROM entries WHERE family_id=f.family_id AND kind=2), f.head_hash FROM families f WHERE f.family_id=?1",
            params![&family[..]],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        ).unwrap();
        assert_eq!((cursor, controls, batches), (3, 2, 1));
        drop(db);
        assert!(RelayStore::open(&path, seed).is_ok());
        let db = Connection::open(&path).unwrap();
        let (batch_cursor, original_envelope): (i64, Vec<u8>) = db
            .query_row(
                "SELECT cursor,envelope_bytes FROM batch_results WHERE family_id=?1",
                params![&family[..]],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        let saved_receipt: Vec<u8> = db
            .query_row(
                "SELECT receipt_bytes FROM batch_results WHERE family_id=?1",
                params![&family[..]],
                |row| row.get(0),
            )
            .unwrap();
        let recipient_id: [u8; 16] = hex(chain["test_only_inputs"]["recipient_device_id_hex"]
            .as_str()
            .unwrap())
        .try_into()
        .unwrap();
        let recipient_seed: [u8; 32] = hex(chain["test_only_inputs"]["recipient_sign_seed_hex"]
            .as_str()
            .unwrap())
        .try_into()
        .unwrap();
        let forged = resign_batch_author(&original_envelope, recipient_id, 1, None, recipient_seed);
        let forged_batch = batch_authority::verify(
            &forged,
            family,
            parsed.relay_id,
            crypto::signing_public_key(&recipient_seed),
        )
        .unwrap();
        let forged_receipt =
            receipt::accepted_batch(&forged_batch, batch_cursor as u64, &seed).unwrap();
        db.execute(
            "UPDATE entries SET committed_bytes=?3 WHERE family_id=?1 AND cursor=?2",
            params![&family[..], batch_cursor, &forged],
        )
        .unwrap();
        db.execute(
            "UPDATE batch_results SET envelope_bytes=?2,receipt_bytes=?3,author_id=?4 WHERE family_id=?1",
            params![&family[..], &forged, &forged_receipt, &recipient_id[..]],
        )
        .unwrap();
        assert!(RelayStore::open(&path, seed).is_err());
        db.execute(
            "UPDATE entries SET committed_bytes=?3 WHERE family_id=?1 AND cursor=?2",
            params![&family[..], batch_cursor, &original_envelope],
        )
        .unwrap();
        db.execute(
            "UPDATE batch_results SET envelope_bytes=?2,receipt_bytes=?3,author_id=?4 WHERE family_id=?1",
            params![&family[..], &original_envelope, &saved_receipt, &parsed.manager_id[..]],
        )
        .unwrap();
        assert!(RelayStore::open(&path, seed).is_ok());
        let skipped =
            resign_batch_author(&original_envelope, parsed.manager_id, 2, None, manager_seed);
        let skipped_batch = batch_authority::verify(
            &skipped,
            family,
            parsed.relay_id,
            parsed.manager_signing_key,
        )
        .unwrap();
        let skipped_receipt =
            receipt::accepted_batch(&skipped_batch, batch_cursor as u64, &seed).unwrap();
        db.execute(
            "UPDATE entries SET committed_bytes=?3 WHERE family_id=?1 AND cursor=?2",
            params![&family[..], batch_cursor, &skipped],
        )
        .unwrap();
        db.execute(
            "UPDATE batch_results SET envelope_bytes=?2,receipt_bytes=?3,sequence=2 WHERE family_id=?1",
            params![&family[..], &skipped, &skipped_receipt],
        )
        .unwrap();
        assert!(RelayStore::open(&path, seed).is_err());
        db.execute(
            "UPDATE entries SET committed_bytes=?3 WHERE family_id=?1 AND cursor=?2",
            params![&family[..], batch_cursor, &original_envelope],
        )
        .unwrap();
        db.execute(
            "UPDATE batch_results SET envelope_bytes=?2,receipt_bytes=?3,sequence=1 WHERE family_id=?1",
            params![&family[..], &original_envelope, &saved_receipt],
        )
        .unwrap();
        assert!(RelayStore::open(&path, seed).is_ok());
        let (mut rejected_id, _) = batch_authority::claimed_identity(&original_envelope).unwrap();
        rejected_id[15] ^= 1;
        let rejected_envelope = resign_batch_author(
            &original_envelope,
            parsed.manager_id,
            1,
            Some(rejected_id),
            manager_seed,
        );
        let mut writer = RelayStore::open(&path, seed).unwrap();
        let rejected_response = writer
            .commit_initial_cohort_batch(family, &rejected_envelope)
            .unwrap();
        drop(writer);
        let Value::Map(rejected_fields) = cbor::decode(&rejected_response).unwrap() else {
            panic!()
        };
        let Value::Bytes(rejected_receipt) = &rejected_fields[1].1 else {
            panic!()
        };
        assert!(RelayStore::open(&path, seed).is_ok());
        let mut changed_rejection = rejected_receipt.clone();
        *changed_rejection.last_mut().unwrap() ^= 1;
        db.execute(
            "UPDATE rejected_batch_results SET receipt_bytes=?3 WHERE family_id=?1 AND batch_id=?2",
            params![&family[..], &rejected_id[..], &changed_rejection],
        )
        .unwrap();
        assert!(RelayStore::open(&path, seed).is_err());
        db.execute(
            "UPDATE rejected_batch_results SET receipt_bytes=?3 WHERE family_id=?1 AND batch_id=?2",
            params![&family[..], &rejected_id[..], rejected_receipt],
        )
        .unwrap();
        assert!(RelayStore::open(&path, seed).is_ok());
        let mut changed_receipt = saved_receipt.clone();
        *changed_receipt.last_mut().unwrap() ^= 1;
        db.execute(
            "UPDATE batch_results SET receipt_bytes=?2 WHERE family_id=?1",
            params![&family[..], &changed_receipt],
        )
        .unwrap();
        assert!(RelayStore::open(&path, seed).is_err());
        db.execute(
            "UPDATE batch_results SET receipt_bytes=?2 WHERE family_id=?1",
            params![&family[..], &saved_receipt],
        )
        .unwrap();
        db.execute(
            "UPDATE families SET head_hash=?2 WHERE family_id=?1",
            params![&family[..], &[0u8; 32][..]],
        )
        .unwrap();
        drop(db);
        assert!(RelayStore::open(&path, seed).is_err());
        let db = Connection::open(&path).unwrap();
        db.execute(
            "UPDATE families SET head_hash=?2 WHERE family_id=?1",
            params![&family[..], &head],
        )
        .unwrap();
        let issue_bytes: Vec<u8> = db
            .query_row(
                "SELECT committed_bytes FROM entries WHERE family_id=?1 AND kind=1 ORDER BY cursor LIMIT 1 OFFSET 1",
                params![&family[..]],
                |row| row.get(0),
            )
            .unwrap();
        let mut changed = issue_bytes;
        *changed.last_mut().unwrap() ^= 1;
        db.execute(
            "UPDATE entries SET committed_bytes=?2 WHERE family_id=?1 AND kind=1 AND cursor>1",
            params![&family[..], &changed],
        )
        .unwrap();
        drop(db);
        assert!(RelayStore::open(&path, seed).is_err());
    }

    #[test]
    fn competing_staged_issue_does_not_block_valid_issue_after_restart() {
        let genesis: Json = serde_json::from_str(
            &std::fs::read_to_string("../tests/vectors/api-genesis-v1.json").unwrap(),
        )
        .unwrap();
        let issue: Json =
            serde_json::from_str(&std::fs::read_to_string("../tests/vectors/api-v1.json").unwrap())
                .unwrap();
        let chain: Json = serde_json::from_str(
            &std::fs::read_to_string("../tests/vectors/contiguous-chain-v1.json").unwrap(),
        )
        .unwrap();
        let seed: [u8; 32] = hex(chain["test_only_inputs"]["relay_sign_seed_hex"]
            .as_str()
            .unwrap())
        .try_into()
        .unwrap();
        let manager_seed: [u8; 32] = hex(chain["test_only_inputs"]["manager_sign_seed_hex"]
            .as_str()
            .unwrap())
        .try_into()
        .unwrap();
        let family: [u8; 16] = hex(genesis["inputs"]["family_id_hex"].as_str().unwrap())
            .try_into()
            .unwrap();
        let promotion: [u8; 16] = hex(genesis["inputs"]["promotion_id_hex"].as_str().unwrap())
            .try_into()
            .unwrap();
        let issue_object: [u8; 16] = hex("083e4567e89b42d3a456426614174000").try_into().unwrap();
        let genesis_stage = hex(genesis["inputs"]["stage_body_cbor_hex"].as_str().unwrap());
        let genesis_candidate = hex(genesis["inputs"]["commit_candidate_cbor_hex"]
            .as_str()
            .unwrap());
        let issue_stage = hex(issue["inputs"]["stage_body_cbor_hex"].as_str().unwrap());
        let issue_candidate = hex(issue["inputs"]["commit_body_cbor_hex"].as_str().unwrap());
        let parsed = authority::verify_genesis_candidate(
            &genesis_candidate,
            &crypto::signing_public_key(&seed),
        )
        .unwrap();
        let (competing_candidate, competing_stage) =
            competing_issue(&issue_candidate, &issue_stage, &parsed, manager_seed);
        let genesis_commit = hex(genesis["expect"]["commit_response_cbor_hex"]
            .as_str()
            .unwrap());
        let Value::Map(genesis_response) = cbor::decode(&genesis_commit).unwrap() else {
            panic!()
        };
        let Value::Bytes(genesis_committed) = &genesis_response[1].1 else {
            panic!()
        };
        let time = control_commit_time(genesis_committed).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("relay.sqlite");
        let mut store = RelayStore::open(&path, seed).unwrap();
        store
            .stage_genesis_object(family, promotion, &genesis_stage)
            .unwrap();
        store
            .commit_genesis(family, &genesis_candidate, time)
            .unwrap();
        let head = crypto::hash("control-head", genesis_committed).unwrap();
        authority::verify_first_invite_issue(&competing_candidate, &parsed, head).unwrap();
        store
            .stage_first_issue_object(family, issue_object, &competing_stage)
            .unwrap();
        // Simulate a database interrupted under the original single-slot
        // schema, then let open() migrate that staged candidate.
        store.db.execute(
            "INSERT INTO staged_issues SELECT family_id,transition_id,candidate_bytes FROM staged_controls WHERE family_id=?1",
            params![&family[..]],
        ).unwrap();
        store.db.execute(
            "INSERT INTO staged_objects SELECT family_id,object_id,kind,object_hash,object_bytes FROM staged_control_objects WHERE family_id=?1",
            params![&family[..]],
        ).unwrap();
        store
            .db
            .execute(
                "DELETE FROM staged_controls WHERE family_id=?1",
                params![&family[..]],
            )
            .unwrap();
        store
            .db
            .execute(
                "DELETE FROM staged_control_objects WHERE family_id=?1",
                params![&family[..]],
            )
            .unwrap();
        drop(store);
        let mut store = RelayStore::open(&path, seed).unwrap();
        store
            .stage_first_issue_object(family, issue_object, &issue_stage)
            .unwrap();
        assert_eq!(
            store
                .db
                .query_row(
                    "SELECT COUNT(*) FROM staged_controls WHERE family_id=?1",
                    params![&family[..]],
                    |r| r.get::<_, i64>(0)
                )
                .unwrap(),
            2
        );
        store
            .commit_first_issue(family, &issue_candidate, time)
            .unwrap();
        assert!(
            store
                .commit_first_issue(family, &competing_candidate, time)
                .is_err()
        );
        assert_eq!(
            store
                .db
                .query_row(
                    "SELECT COUNT(*) FROM staged_controls WHERE family_id=?1",
                    params![&family[..]],
                    |r| r.get::<_, i64>(0)
                )
                .unwrap(),
            0
        );
        drop(store);
        let mut store = RelayStore::open(&path, seed).unwrap();
        assert!(
            store
                .stage_first_issue_object(family, issue_object, &competing_stage)
                .is_err()
        );
        assert!(
            store
                .commit_first_issue(family, &competing_candidate, time)
                .is_err()
        );
    }
    #[test]
    fn staged_object_id_keeps_its_hash_across_competitors_cleanup_and_restart() {
        let genesis: Json = serde_json::from_str(
            &std::fs::read_to_string("../tests/vectors/api-genesis-v1.json").unwrap(),
        )
        .unwrap();
        let issue: Json =
            serde_json::from_str(&std::fs::read_to_string("../tests/vectors/api-v1.json").unwrap())
                .unwrap();
        let chain: Json = serde_json::from_str(
            &std::fs::read_to_string("../tests/vectors/contiguous-chain-v1.json").unwrap(),
        )
        .unwrap();
        let seed: [u8; 32] = hex(chain["test_only_inputs"]["relay_sign_seed_hex"]
            .as_str()
            .unwrap())
        .try_into()
        .unwrap();
        let manager_seed: [u8; 32] = hex(chain["test_only_inputs"]["manager_sign_seed_hex"]
            .as_str()
            .unwrap())
        .try_into()
        .unwrap();
        let family: [u8; 16] = hex(genesis["inputs"]["family_id_hex"].as_str().unwrap())
            .try_into()
            .unwrap();
        let promotion: [u8; 16] = hex(genesis["inputs"]["promotion_id_hex"].as_str().unwrap())
            .try_into()
            .unwrap();
        let object_id: [u8; 16] = hex("083e4567e89b42d3a456426614174000").try_into().unwrap();
        let genesis_stage = hex(genesis["inputs"]["stage_body_cbor_hex"].as_str().unwrap());
        let genesis_candidate = hex(genesis["inputs"]["commit_candidate_cbor_hex"]
            .as_str()
            .unwrap());
        let issue_stage = hex(issue["inputs"]["stage_body_cbor_hex"].as_str().unwrap());
        let issue_candidate = hex(issue["inputs"]["commit_body_cbor_hex"].as_str().unwrap());
        let parsed = authority::verify_genesis_candidate(
            &genesis_candidate,
            &crypto::signing_public_key(&seed),
        )
        .unwrap();
        let (_, competitor) =
            competing_issue(&issue_candidate, &issue_stage, &parsed, manager_seed);
        let changed = issue_stage_with_changed_object(&competitor, parsed.manager_id, manager_seed);
        let genesis_commit = hex(genesis["expect"]["commit_response_cbor_hex"]
            .as_str()
            .unwrap());
        let Value::Map(response) = cbor::decode(&genesis_commit).unwrap() else {
            panic!()
        };
        let Value::Bytes(committed) = &response[1].1 else {
            panic!()
        };
        let time = control_commit_time(committed).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("reservations.sqlite");
        let mut store = RelayStore::open(&path, seed).unwrap();
        store
            .stage_genesis_object(family, promotion, &genesis_stage)
            .unwrap();
        store
            .commit_genesis(family, &genesis_candidate, time)
            .unwrap();
        store
            .stage_first_issue_object(family, object_id, &issue_stage)
            .unwrap();
        assert!(
            store
                .stage_first_issue_object(family, object_id, &changed)
                .is_err()
        );
        store
            .db
            .execute(
                "DELETE FROM staged_control_objects WHERE family_id=?1",
                params![&family[..]],
            )
            .unwrap();
        store
            .db
            .execute(
                "DELETE FROM staged_controls WHERE family_id=?1",
                params![&family[..]],
            )
            .unwrap();
        drop(store);
        let mut store = RelayStore::open(&path, seed).unwrap();
        assert!(
            store
                .stage_first_issue_object(family, object_id, &changed)
                .is_err()
        );
        store
            .stage_first_issue_object(family, object_id, &issue_stage)
            .unwrap();
    }

    #[test]
    fn restart_rejects_substituted_genesis_candidate_and_receipt() {
        let genesis: Json = serde_json::from_str(
            &std::fs::read_to_string("../tests/vectors/api-genesis-v1.json").unwrap(),
        )
        .unwrap();
        let issue: Json =
            serde_json::from_str(&std::fs::read_to_string("../tests/vectors/api-v1.json").unwrap())
                .unwrap();
        let chain: Json = serde_json::from_str(
            &std::fs::read_to_string("../tests/vectors/contiguous-chain-v1.json").unwrap(),
        )
        .unwrap();
        let seed: [u8; 32] = hex(chain["test_only_inputs"]["relay_sign_seed_hex"]
            .as_str()
            .unwrap())
        .try_into()
        .unwrap();
        let manager_seed: [u8; 32] = hex(chain["test_only_inputs"]["manager_sign_seed_hex"]
            .as_str()
            .unwrap())
        .try_into()
        .unwrap();
        let manager_id: [u8; 16] = hex(chain["test_only_inputs"]["manager_device_id_hex"]
            .as_str()
            .unwrap())
        .try_into()
        .unwrap();
        let family: [u8; 16] = hex(genesis["inputs"]["family_id_hex"].as_str().unwrap())
            .try_into()
            .unwrap();
        let promotion: [u8; 16] = hex(genesis["inputs"]["promotion_id_hex"].as_str().unwrap())
            .try_into()
            .unwrap();
        let issue_object: [u8; 16] = hex("083e4567e89b42d3a456426614174000").try_into().unwrap();
        let genesis_stage = hex(genesis["inputs"]["stage_body_cbor_hex"].as_str().unwrap());
        let genesis_candidate = hex(genesis["inputs"]["commit_candidate_cbor_hex"]
            .as_str()
            .unwrap());
        let issue_stage = hex(issue["inputs"]["stage_body_cbor_hex"].as_str().unwrap());
        let alternate = validly_resigned_genesis_with_changed_manifest(
            &genesis_candidate,
            manager_id,
            manager_seed,
        );
        authority::verify_genesis_candidate(&alternate, &crypto::signing_public_key(&seed))
            .unwrap();
        let genesis_commit = hex(genesis["expect"]["commit_response_cbor_hex"]
            .as_str()
            .unwrap());
        let Value::Map(response) = cbor::decode(&genesis_commit).unwrap() else {
            panic!()
        };
        let Value::Bytes(committed) = &response[1].1 else {
            panic!()
        };
        let time = control_commit_time(committed).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("genesis-corruption.sqlite");
        let mut store = RelayStore::open(&path, seed).unwrap();
        store
            .stage_genesis_object(family, promotion, &genesis_stage)
            .unwrap();
        store
            .commit_genesis(family, &genesis_candidate, time)
            .unwrap();
        store
            .db
            .execute(
                "UPDATE families SET candidate_bytes=?2 WHERE family_id=?1",
                params![&family[..], &alternate],
            )
            .unwrap();
        assert!(
            store
                .stage_first_issue_object(family, issue_object, &issue_stage)
                .is_err()
        );
        drop(store);
        assert!(RelayStore::open(&path, seed).is_err());
        let db = Connection::open(&path).unwrap();
        db.execute(
            "UPDATE families SET candidate_bytes=?2 WHERE family_id=?1",
            params![&family[..], &genesis_candidate],
        )
        .unwrap();
        let mut store = RelayStore::open(&path, seed).unwrap();
        let mut changed = committed.clone();
        *changed.last_mut().unwrap() ^= 1;
        store
            .db
            .execute(
                "UPDATE families SET committed_bytes=?2 WHERE family_id=?1",
                params![&family[..], &changed],
            )
            .unwrap();
        assert!(
            store
                .stage_first_issue_object(family, issue_object, &issue_stage)
                .is_err()
        );
        store
            .db
            .execute(
                "UPDATE entries SET committed_bytes=?2 WHERE family_id=?1 AND cursor=1",
                params![&family[..], &changed],
            )
            .unwrap();
        assert!(
            store
                .stage_first_issue_object(family, issue_object, &issue_stage)
                .is_err()
        );
        drop(store);
        assert!(RelayStore::open(&path, seed).is_err());
        let db = Connection::open(&path).unwrap();
        db.execute(
            "UPDATE families SET committed_bytes=?2 WHERE family_id=?1",
            params![&family[..], committed],
        )
        .unwrap();
        db.execute(
            "UPDATE entries SET committed_bytes=?2 WHERE family_id=?1 AND cursor=1",
            params![&family[..], committed],
        )
        .unwrap();
        let object: Vec<u8> = db
            .query_row(
                "SELECT object_bytes FROM committed_objects WHERE family_id=?1 AND object_id=?2",
                params![&family[..], &promotion[..]],
                |row| row.get(0),
            )
            .unwrap();
        let mut changed_object = object;
        *changed_object.last_mut().unwrap() ^= 1;
        db.execute(
            "UPDATE committed_objects SET object_bytes=?3 WHERE family_id=?1 AND object_id=?2",
            params![&family[..], &promotion[..], &changed_object],
        )
        .unwrap();
        drop(db);
        assert!(RelayStore::open(&path, seed).is_err());
    }

    #[test]
    fn genesis_reservation_and_commit_are_atomic_and_survive_restart() {
        let api: Json = serde_json::from_str(
            &std::fs::read_to_string("../tests/vectors/api-genesis-v1.json").unwrap(),
        )
        .unwrap();
        let chain: Json = serde_json::from_str(
            &std::fs::read_to_string("../tests/vectors/contiguous-chain-v1.json").unwrap(),
        )
        .unwrap();
        let seed: [u8; 32] = hex(chain["test_only_inputs"]["relay_sign_seed_hex"]
            .as_str()
            .unwrap())
        .try_into()
        .unwrap();
        let stage = hex(api["inputs"]["stage_body_cbor_hex"].as_str().unwrap());
        let candidate = hex(api["inputs"]["commit_candidate_cbor_hex"].as_str().unwrap());
        let family: [u8; 16] = hex(api["inputs"]["family_id_hex"].as_str().unwrap())
            .try_into()
            .unwrap();
        let object_id: [u8; 16] = hex(api["inputs"]["promotion_id_hex"].as_str().unwrap())
            .try_into()
            .unwrap();
        let promotion_path = api["inputs"]["promotion_result_path"].as_str().unwrap();
        let promotion_auth = hex(api["inputs"]["promotion_result_read_auth_cbor_hex"]
            .as_str()
            .unwrap());
        let expected_stage = hex(api["expect"]["stage_response_cbor_hex"].as_str().unwrap());
        let expected_commit = hex(api["expect"]["commit_response_cbor_hex"].as_str().unwrap());
        let Value::Map(response) = cbor::decode(&expected_commit).unwrap() else {
            panic!()
        };
        let Value::Bytes(committed) = &response[1].1 else {
            panic!()
        };
        let Value::Map(commit_map) = cbor::decode(committed).unwrap() else {
            panic!()
        };
        let Value::Array(receipt) = &commit_map[2].1 else {
            panic!()
        };
        let Value::Integer(time) = receipt[4] else {
            panic!()
        };
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("relay.sqlite");
        let mut store = RelayStore::open(&path, seed).unwrap();
        assert!(
            store
                .commit_genesis(family, &candidate, time.try_into().unwrap())
                .is_err()
        );
        assert_eq!(
            store
                .stage_genesis_object(family, object_id, &stage)
                .unwrap(),
            expected_stage
        );
        drop(store);
        let mut store = RelayStore::open(&path, seed).unwrap();
        assert_eq!(
            store
                .stage_genesis_object(family, object_id, &stage)
                .unwrap(),
            expected_stage
        );
        assert!(store.genesis_result(family).unwrap().is_none());
        assert!(store.committed_object(family, object_id).unwrap().is_none());
        let pending_result = store
            .promotion_result_authenticated(family, object_id, promotion_path, &promotion_auth)
            .unwrap();
        assert_eq!(
            cbor::decode(&pending_result).unwrap(),
            Value::Map(vec![(1, Value::Integer(1)), (2, Value::Null)])
        );
        assert_eq!(
            store
                .commit_genesis(family, &candidate, time.try_into().unwrap())
                .unwrap(),
            expected_commit
        );
        assert_eq!(
            store
                .commit_genesis(family, &candidate, i64::try_from(time).unwrap() + 100)
                .unwrap(),
            expected_commit
        );
        assert_eq!(
            store
                .stage_genesis_object(family, object_id, &stage)
                .unwrap(),
            expected_stage
        );
        assert_eq!(
            store
                .promotion_result_authenticated(family, object_id, promotion_path, &promotion_auth)
                .unwrap(),
            hex(api["expect"]["promotion_result_response_cbor_hex"]
                .as_str()
                .unwrap())
        );
        assert!(
            store
                .promotion_result_authenticated(
                    family,
                    object_id,
                    &format!("{promotion_path}/"),
                    &promotion_auth
                )
                .is_err()
        );
        drop(store);
        let store = RelayStore::open(&path, seed).unwrap();
        assert_eq!(store.genesis_result(family).unwrap().unwrap(), *committed);
        assert!(store.committed_object(family, object_id).unwrap().is_some());
        assert!(RelayStore::open(&path, [42; 32]).is_err());
    }

    #[test]
    fn first_issue_stages_membership_and_commits_exact_api_bytes() {
        let genesis: Json = serde_json::from_str(
            &std::fs::read_to_string("../tests/vectors/api-genesis-v1.json").unwrap(),
        )
        .unwrap();
        let issue: Json =
            serde_json::from_str(&std::fs::read_to_string("../tests/vectors/api-v1.json").unwrap())
                .unwrap();
        let chain: Json = serde_json::from_str(
            &std::fs::read_to_string("../tests/vectors/contiguous-chain-v1.json").unwrap(),
        )
        .unwrap();
        let seed: [u8; 32] = hex(chain["test_only_inputs"]["relay_sign_seed_hex"]
            .as_str()
            .unwrap())
        .try_into()
        .unwrap();
        let family: [u8; 16] = hex(genesis["inputs"]["family_id_hex"].as_str().unwrap())
            .try_into()
            .unwrap();
        let promotion: [u8; 16] = hex(genesis["inputs"]["promotion_id_hex"].as_str().unwrap())
            .try_into()
            .unwrap();
        let issue_object: [u8; 16] = hex("083e4567e89b42d3a456426614174000").try_into().unwrap();
        let genesis_stage = hex(genesis["inputs"]["stage_body_cbor_hex"].as_str().unwrap());
        let genesis_candidate = hex(genesis["inputs"]["commit_candidate_cbor_hex"]
            .as_str()
            .unwrap());
        let genesis_committed_response = hex(genesis["expect"]["commit_response_cbor_hex"]
            .as_str()
            .unwrap());
        let Value::Map(fields) = cbor::decode(&genesis_committed_response).unwrap() else {
            panic!()
        };
        let Value::Bytes(genesis_committed) = &fields[1].1 else {
            panic!()
        };
        let genesis_time = control_commit_time(genesis_committed).unwrap();
        let issue_stage = hex(issue["inputs"]["stage_body_cbor_hex"].as_str().unwrap());
        let issue_candidate = hex(issue["inputs"]["commit_body_cbor_hex"].as_str().unwrap());
        let issue_expected = hex(issue["expect"]["commit_response_cbor_hex"]
            .as_str()
            .unwrap());
        let Value::Map(fields) = cbor::decode(&issue_expected).unwrap() else {
            panic!()
        };
        let Value::Bytes(issue_committed) = &fields[1].1 else {
            panic!()
        };
        let issue_time = control_commit_time(issue_committed).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("relay.db");
        let mut store = RelayStore::open(&path, seed).unwrap();
        store
            .stage_genesis_object(family, promotion, &genesis_stage)
            .unwrap();
        store
            .commit_genesis(family, &genesis_candidate, genesis_time)
            .unwrap();
        assert!(
            store
                .commit_first_issue(family, &issue_candidate, issue_time)
                .is_err()
        );
        assert_eq!(
            store
                .stage_first_issue_object(family, issue_object, &issue_stage)
                .unwrap(),
            hex(issue["expect"]["stage_response_cbor_hex"].as_str().unwrap())
        );
        assert!(
            store
                .committed_object(family, issue_object)
                .unwrap()
                .is_none()
        );
        drop(store);
        let mut store = RelayStore::open(&path, seed).unwrap();
        assert_eq!(
            store
                .commit_first_issue(family, &issue_candidate, issue_time)
                .unwrap(),
            issue_expected
        );
        assert_eq!(
            store
                .commit_first_issue(family, &issue_candidate, issue_time + 1)
                .unwrap(),
            issue_expected
        );
        assert!(
            store
                .committed_object(family, issue_object)
                .unwrap()
                .is_some()
        );
        assert_eq!(
            store
                .stage_first_issue_object(family, issue_object, &issue_stage)
                .unwrap(),
            hex(issue["expect"]["stage_response_cbor_hex"].as_str().unwrap())
        );
        let genesis_parsed =
            authority::verify_genesis_candidate(&genesis_candidate, &store.relay_public).unwrap();
        let genesis_head = crypto::hash("control-head", genesis_committed).unwrap();
        let issue_parsed =
            authority::verify_first_invite_issue(&issue_candidate, &genesis_parsed, genesis_head)
                .unwrap();
        assert!(
            ensure_new_protocol_ids(&store.db, family, &[genesis_parsed.transition_id]).is_err()
        );
        assert!(ensure_new_protocol_ids(&store.db, family, &[genesis_parsed.manager_id]).is_err());
        assert!(ensure_new_protocol_ids(&store.db, family, &[issue_parsed.invitation_id]).is_err());
        assert!(ensure_new_protocol_ids(&store.db, family, &[issue_object]).is_err());
        assert!(ensure_new_protocol_ids(&store.db, family, &[[0x9a; 16]]).is_ok());
        let invite_seed: [u8; 32] = hex(chain["test_only_inputs"]["invitation_sign_seed_hex"]
            .as_str()
            .unwrap())
        .try_into()
        .unwrap();
        let control_path = format!("/v1/families/{}/control?after=0", lower_hex(&family));
        let invite_control = signed_get(
            family,
            genesis_parsed.relay_id,
            issue_parsed.invitation_id,
            invite_seed,
            &control_path,
            [1; 16],
        );
        assert!(
            store
                .control_page_authenticated(family, 0, &control_path, &invite_control)
                .is_ok()
        );
        let issue_object_path = issue["inputs"]["read_object_path"].as_str().unwrap();
        let invite_object = signed_get(
            family,
            genesis_parsed.relay_id,
            issue_parsed.invitation_id,
            invite_seed,
            issue_object_path,
            [2; 16],
        );
        assert!(
            store
                .object_authenticated(family, issue_object, issue_object_path, &invite_object)
                .is_ok()
        );
        let promotion_path = genesis["inputs"]["promotion_result_path"].as_str().unwrap();
        let invite_promotion = signed_get(
            family,
            genesis_parsed.relay_id,
            issue_parsed.invitation_id,
            invite_seed,
            promotion_path,
            [3; 16],
        );
        assert!(
            store
                .object_authenticated(family, promotion, promotion_path, &invite_promotion)
                .is_err()
        );
        let claim_transition = &chain["transitions"][2];
        let claim_candidate = cbor::encode(&Value::Map(vec![
            (
                1,
                cbor::decode(&hex(claim_transition["unsigned_cbor_hex"]
                    .as_str()
                    .unwrap()))
                .unwrap(),
            ),
            (
                2,
                cbor::decode(&hex(claim_transition["signatures_cbor_hex"]
                    .as_str()
                    .unwrap()))
                .unwrap(),
            ),
        ]))
        .unwrap();
        let claim_committed = hex(claim_transition["committed_cbor_hex"].as_str().unwrap());
        let claim_time = control_commit_time(&claim_committed).unwrap();
        let before_claim: (i64, Vec<u8>) = store
            .db
            .query_row(
                "SELECT cursor,head_hash FROM families WHERE family_id=?1",
                params![&family[..]],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        let mut clock_called = false;
        assert!(
            store
                .commit_first_claim_with_clock(family, &claim_candidate, || {
                    clock_called = true;
                    let other = Connection::open(&path).unwrap();
                    other.busy_timeout(std::time::Duration::ZERO).unwrap();
                    assert!(
                        other
                            .execute(
                                "UPDATE families SET cursor=cursor WHERE family_id=?1",
                                params![&family[..]],
                            )
                            .is_err()
                    );
                    Ok(issue_time + 604_800_000)
                })
                .is_err()
        );
        assert!(clock_called);
        let after_expiry: (i64, Vec<u8>) = store
            .db
            .query_row(
                "SELECT cursor,head_hash FROM families WHERE family_id=?1",
                params![&family[..]],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(after_expiry, before_claim);
        assert_eq!(
            store
                .commit_first_claim(family, &claim_candidate, claim_time)
                .unwrap(),
            receipt::control_commit_response(&claim_committed).unwrap()
        );
        assert_eq!(
            store
                .commit_first_claim_with_clock(family, &claim_candidate, || {
                    panic!("exact claim retry must not recheck expiry or clock")
                })
                .unwrap(),
            receipt::control_commit_response(&claim_committed).unwrap()
        );
        assert_eq!(
            store
                .stage_first_issue_object(family, issue_object, &issue_stage)
                .unwrap(),
            hex(issue["expect"]["stage_response_cbor_hex"].as_str().unwrap())
        );
        assert_eq!(
            store
                .commit_first_issue(family, &issue_candidate, issue_time + 1)
                .unwrap(),
            issue_expected
        );
        let invite_after_claim = signed_get(
            family,
            genesis_parsed.relay_id,
            issue_parsed.invitation_id,
            invite_seed,
            &control_path,
            [4; 16],
        );
        assert!(
            store
                .control_page_authenticated(family, 0, &control_path, &invite_after_claim)
                .is_err()
        );
        let recipient_seed: [u8; 32] = hex(chain["test_only_inputs"]["recipient_sign_seed_hex"]
            .as_str()
            .unwrap())
        .try_into()
        .unwrap();
        let claim_parsed = authority::verify_first_claim(
            &claim_candidate,
            &genesis_parsed,
            &issue_parsed,
            crypto::hash("control-head", issue_committed).unwrap(),
        )
        .unwrap();
        let pending_read = signed_get(
            family,
            genesis_parsed.relay_id,
            claim_parsed.device_id,
            recipient_seed,
            &control_path,
            [5; 16],
        );
        assert!(
            store
                .control_page_authenticated(family, 0, &control_path, &pending_read)
                .is_ok()
        );
        let pending_object = signed_get(
            family,
            genesis_parsed.relay_id,
            claim_parsed.device_id,
            recipient_seed,
            issue_object_path,
            [6; 16],
        );
        assert!(
            store
                .object_authenticated(family, issue_object, issue_object_path, &pending_object)
                .is_err()
        );
        let challenge_transition = &chain["transitions"][3];
        let challenge_unsigned = cbor::decode(&hex(challenge_transition["unsigned_cbor_hex"]
            .as_str()
            .unwrap()))
        .unwrap();
        let challenge_signatures = cbor::decode(&hex(challenge_transition["signatures_cbor_hex"]
            .as_str()
            .unwrap()))
        .unwrap();
        let challenge_candidate = cbor::encode(&Value::Map(vec![
            (1, challenge_unsigned.clone()),
            (2, challenge_signatures.clone()),
        ]))
        .unwrap();
        let challenge_committed = hex(challenge_transition["committed_cbor_hex"].as_str().unwrap());
        let challenge_time = control_commit_time(&challenge_committed).unwrap();
        assert!(
            store
                .commit_first_challenge(family, &challenge_candidate, challenge_time)
                .is_err()
        );
        let mut challenge_ids = Vec::new();
        for (index, entry) in challenge_transition["manifest"]
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
        {
            let kind = entry[0].as_u64().unwrap();
            let id_text = entry[1].as_str().unwrap();
            let id: [u8; 16] = hex(id_text).try_into().unwrap();
            let object_bytes = hex(chain["objects_by_id_hex"][id_text].as_str().unwrap());
            let stage = cbor::encode(&Value::Map(vec![
                (1, Value::Integer(1)),
                (2, challenge_unsigned.clone()),
                (3, challenge_signatures.clone()),
                (4, Value::Integer(kind.into())),
                (5, Value::Bytes(id.to_vec())),
                (6, Value::Bytes(object_bytes)),
            ]))
            .unwrap();
            store
                .stage_first_challenge_object(family, id, &stage)
                .unwrap();
            challenge_ids.push((id, stage));
            if index == 0 {
                assert!(
                    store
                        .commit_first_challenge(family, &challenge_candidate, challenge_time)
                        .is_err()
                );
            }
        }
        assert_eq!(
            store
                .commit_first_challenge(family, &challenge_candidate, challenge_time)
                .unwrap(),
            receipt::control_commit_response(&challenge_committed).unwrap()
        );
        assert_eq!(
            store
                .commit_first_challenge(family, &challenge_candidate, challenge_time + 1)
                .unwrap(),
            receipt::control_commit_response(&challenge_committed).unwrap()
        );
        let hpke_path = format!(
            "/v1/families/{}/objects/{}",
            lower_hex(&family),
            lower_hex(&challenge_ids[0].0)
        );
        let hpke_read = signed_get(
            family,
            genesis_parsed.relay_id,
            claim_parsed.device_id,
            recipient_seed,
            &hpke_path,
            [7; 16],
        );
        assert!(
            store
                .object_authenticated(family, challenge_ids[0].0, &hpke_path, &hpke_read)
                .is_ok()
        );
        let verifier_path = format!(
            "/v1/families/{}/objects/{}",
            lower_hex(&family),
            lower_hex(&challenge_ids[1].0)
        );
        let verifier_read = signed_get(
            family,
            genesis_parsed.relay_id,
            claim_parsed.device_id,
            recipient_seed,
            &verifier_path,
            [8; 16],
        );
        assert!(
            store
                .object_authenticated(family, challenge_ids[1].0, &verifier_path, &verifier_read)
                .is_err()
        );
        let proof_transition = &chain["transitions"][4];
        let proof_candidate = cbor::encode(&Value::Map(vec![
            (
                1,
                cbor::decode(&hex(proof_transition["unsigned_cbor_hex"]
                    .as_str()
                    .unwrap()))
                .unwrap(),
            ),
            (
                2,
                cbor::decode(&hex(proof_transition["signatures_cbor_hex"]
                    .as_str()
                    .unwrap()))
                .unwrap(),
            ),
        ]))
        .unwrap();
        let proof_committed = hex(proof_transition["committed_cbor_hex"].as_str().unwrap());
        let proof_time = control_commit_time(&proof_committed).unwrap();
        assert_eq!(
            store
                .commit_first_proof(family, &proof_candidate, proof_time)
                .unwrap(),
            receipt::control_commit_response(&proof_committed).unwrap()
        );
        assert_eq!(
            store
                .commit_first_proof(family, &proof_candidate, proof_time + 1)
                .unwrap(),
            receipt::control_commit_response(&proof_committed).unwrap()
        );
        let admission_transition = &chain["transitions"][5];
        let admission_unsigned = cbor::decode(&hex(admission_transition["unsigned_cbor_hex"]
            .as_str()
            .unwrap()))
        .unwrap();
        let admission_signatures = cbor::decode(&hex(admission_transition["signatures_cbor_hex"]
            .as_str()
            .unwrap()))
        .unwrap();
        let admission_candidate = cbor::encode(&Value::Map(vec![
            (1, admission_unsigned.clone()),
            (2, admission_signatures.clone()),
        ]))
        .unwrap();
        let admission_committed = hex(admission_transition["committed_cbor_hex"].as_str().unwrap());
        let admission_time = control_commit_time(&admission_committed).unwrap();
        assert!(
            store
                .commit_first_admission(family, &admission_candidate, admission_time)
                .is_err()
        );
        let mut admission_ids = Vec::new();
        for (index, entry) in admission_transition["manifest"]
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
        {
            let kind = entry[0].as_u64().unwrap();
            let id_text = entry[1].as_str().unwrap();
            let id: [u8; 16] = hex(id_text).try_into().unwrap();
            let object_bytes = hex(chain["objects_by_id_hex"][id_text].as_str().unwrap());
            let stage = cbor::encode(&Value::Map(vec![
                (1, Value::Integer(1)),
                (2, admission_unsigned.clone()),
                (3, admission_signatures.clone()),
                (4, Value::Integer(kind.into())),
                (5, Value::Bytes(id.to_vec())),
                (6, Value::Bytes(object_bytes)),
            ]))
            .unwrap();
            store
                .stage_first_admission_object(family, id, &stage)
                .unwrap();
            admission_ids.push(id);
            if index == 0 {
                assert!(
                    store
                        .commit_first_admission(family, &admission_candidate, admission_time)
                        .is_err()
                );
            }
        }
        assert_eq!(
            store
                .commit_first_admission(family, &admission_candidate, admission_time)
                .unwrap(),
            receipt::control_commit_response(&admission_committed).unwrap()
        );
        assert_eq!(
            store
                .commit_first_admission(family, &admission_candidate, admission_time + 1)
                .unwrap(),
            receipt::control_commit_response(&admission_committed).unwrap()
        );
        let grant_path = format!(
            "/v1/families/{}/objects/{}",
            lower_hex(&family),
            lower_hex(&admission_ids[1])
        );
        let grant_read = signed_get(
            family,
            genesis_parsed.relay_id,
            claim_parsed.device_id,
            recipient_seed,
            &grant_path,
            [9; 16],
        );
        assert!(
            store
                .object_authenticated(family, admission_ids[1], &grant_path, &grant_read)
                .is_ok()
        );
        let verifier_after_grant = signed_get(
            family,
            genesis_parsed.relay_id,
            claim_parsed.device_id,
            recipient_seed,
            &verifier_path,
            [10; 16],
        );
        assert!(
            store
                .object_authenticated(
                    family,
                    challenge_ids[1].0,
                    &verifier_path,
                    &verifier_after_grant
                )
                .is_ok()
        );
    }
}
