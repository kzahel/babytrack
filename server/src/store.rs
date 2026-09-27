//! Durable genesis reservation and commit. All methods are internal until
//! authenticated routes and the remaining authority transitions are ready.

use std::{collections::BTreeSet, path::Path};

use babytrack_wire::{
    cbor::{self, Value},
    crypto,
};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};

use crate::{authority, batch_authority, read_auth, receipt};

#[derive(Debug)]
#[allow(dead_code)] // Detailed errors are mapped to protocol responses by routes.
#[allow(private_interfaces)] // The test-harness feature exposes only Debug/errors.
pub enum Error {
    Sql(rusqlite::Error),
    Cbor(cbor::Error),
    Crypto(crypto::Error),
    Authority(authority::Error),
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

    fn initialize(db: Connection, relay_seed: [u8; 32]) -> Result<Self, Error> {
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
        Ok(Self {
            db,
            relay_seed,
            relay_public,
        })
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
        Ok(self
            .db
            .query_row(
                "SELECT committed_bytes FROM families WHERE family_id=?1 AND active=1",
                params![&family_id[..]],
                |r| r.get(0),
            )
            .optional()?)
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
        let genesis = authority::verify_genesis_candidate(&genesis_bytes, &self.relay_public)?;
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
        let prior: Option<Vec<u8>> = tx
            .query_row(
                "SELECT candidate_bytes FROM staged_issues WHERE family_id=?1",
                params![&path_family[..]],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(saved) = prior {
            if saved != candidate_bytes {
                return Err(Error::Invalid("another issue already staged"));
            }
        } else {
            tx.execute("INSERT INTO staged_issues(family_id,transition_id,candidate_bytes) VALUES(?1,?2,?3)",
                params![&path_family[..],&issue.transition_id[..],&candidate_bytes])?;
        }
        let prior: Option<Vec<u8>> = tx
            .query_row(
                "SELECT object_bytes FROM staged_objects WHERE family_id=?1 AND object_id=?2",
                params![&path_family[..], &object_id[..]],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(saved) = prior {
            if saved != *object_bytes {
                return Err(Error::Invalid("staged object ID collision"));
            }
        } else {
            tx.execute("INSERT INTO staged_objects(family_id,object_id,kind,object_hash,object_bytes) VALUES(?1,?2,?3,?4,?5)",
                params![&path_family[..],&object_id[..],i64::from(kind),&issue.manifest.object_hash[..],object_bytes])?;
        }
        tx.commit()?;
        Ok(receipt::object_stage_response(object_bytes)?)
    }

    pub fn commit_first_issue(
        &mut self,
        path_family: [u8; 16],
        candidate_bytes: &[u8],
        committed_ms: i64,
    ) -> Result<Vec<u8>, Error> {
        let (genesis_bytes,cursor,head,genesis_committed) = self.db.query_row(
            "SELECT candidate_bytes,cursor,head_hash,committed_bytes FROM families WHERE family_id=?1 AND active=1",
            params![&path_family[..]], |r| Ok((r.get::<_,Vec<u8>>(0)?,r.get::<_,i64>(1)?,r.get::<_,Vec<u8>>(2)?,r.get::<_,Vec<u8>>(3)?)),
        )?;
        let genesis = authority::verify_genesis_candidate(&genesis_bytes, &self.relay_public)?;
        let controls = control_count(&self.db, path_family)?;
        let genesis_head = crypto::hash("control-head", &genesis_committed)?;
        let issue = authority::verify_first_invite_issue(candidate_bytes, &genesis, genesis_head)?;
        if issue.family_id != path_family {
            return Err(Error::Invalid("issue path Family mismatch"));
        }
        if controls >= 2 {
            let committed = control_at(&self.db, path_family, 1)?;
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
        let tx = self.db.transaction()?;
        ensure_control_ids(&tx, path_family, candidate_bytes)?;
        let staged_candidate: Option<Vec<u8>> = tx
            .query_row(
                "SELECT candidate_bytes FROM staged_issues WHERE family_id=?1 AND transition_id=?2",
                params![&path_family[..], &issue.transition_id[..]],
                |r| r.get(0),
            )
            .optional()?;
        if staged_candidate.as_deref() != Some(candidate_bytes) {
            return Err(Error::Invalid("issue candidate not staged"));
        }
        let staged: Option<(i64,Vec<u8>,Vec<u8>)> = tx.query_row(
            "SELECT kind,object_hash,object_bytes FROM staged_objects WHERE family_id=?1 AND object_id=?2",
            params![&path_family[..],&issue.manifest.object_id[..]], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?)),
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
            "DELETE FROM staged_issues WHERE family_id=?1",
            params![&path_family[..]],
        )?;
        tx.execute(
            "DELETE FROM staged_objects WHERE family_id=?1 AND object_id=?2",
            params![&path_family[..], &issue.manifest.object_id[..]],
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
        let genesis = authority::verify_genesis_candidate(&genesis_bytes, &self.relay_public)?;
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
        let prior: Option<Vec<u8>> = tx
            .query_row(
                "SELECT candidate_bytes FROM staged_challenges WHERE family_id=?1",
                params![&path_family[..]],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(saved) = prior {
            if saved != candidate_bytes {
                return Err(Error::Invalid("another challenge staged"));
            }
        } else {
            tx.execute("INSERT INTO staged_challenges(family_id,transition_id,candidate_bytes) VALUES(?1,?2,?3)",
                params![&path_family[..],&challenge.transition_id[..],&candidate_bytes])?;
        }
        let prior: Option<Vec<u8>> = tx
            .query_row(
                "SELECT object_bytes FROM staged_objects WHERE family_id=?1 AND object_id=?2",
                params![&path_family[..], &object_id[..]],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(saved) = prior {
            if saved != object_bytes {
                return Err(Error::Invalid("challenge object ID collision"));
            }
        } else {
            tx.execute("INSERT INTO staged_objects(family_id,object_id,kind,object_hash,object_bytes) VALUES(?1,?2,?3,?4,?5)",
                params![&path_family[..],&object_id[..],i64::from(kind),&listed.object_hash[..],&object_bytes])?;
        }
        tx.commit()?;
        Ok(receipt::object_stage_response(&object_bytes)?)
    }

    pub fn commit_first_challenge(
        &mut self,
        path_family: [u8; 16],
        candidate_bytes: &[u8],
        committed_ms: i64,
    ) -> Result<Vec<u8>, Error> {
        let prefix = load_join_prefix(&self.db, self.relay_public, path_family)?;
        let challenge = authority::verify_first_challenge(
            candidate_bytes,
            &prefix.genesis,
            &prefix.issue,
            &prefix.claim,
            prefix.claim_head,
        )?;
        if prefix.controls >= 4 {
            let committed = control_at(&self.db, path_family, 3)?;
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
        let tx = self.db.transaction()?;
        ensure_control_ids(&tx, path_family, candidate_bytes)?;
        let staged:Option<Vec<u8>>=tx.query_row("SELECT candidate_bytes FROM staged_challenges WHERE family_id=?1 AND transition_id=?2",
            params![&path_family[..],&challenge.transition_id[..]],|r|r.get(0)).optional()?;
        if staged.as_deref() != Some(candidate_bytes) {
            return Err(Error::Invalid("challenge candidate not staged"));
        }
        for entry in &challenge.manifest {
            let staged:Option<(i64,Vec<u8>,Vec<u8>)>=tx.query_row(
                "SELECT kind,object_hash,object_bytes FROM staged_objects WHERE family_id=?1 AND object_id=?2",
                params![&path_family[..],&entry.object_id[..]],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)),
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
            "DELETE FROM staged_challenges WHERE family_id=?1",
            params![&path_family[..]],
        )?;
        for entry in &challenge.manifest {
            tx.execute(
                "DELETE FROM staged_objects WHERE family_id=?1 AND object_id=?2",
                params![&path_family[..], &entry.object_id[..]],
            )?;
        }
        tx.commit()?;
        Ok(receipt::control_commit_response(&committed)?)
    }

    pub fn commit_first_proof(
        &mut self,
        path_family: [u8; 16],
        candidate_bytes: &[u8],
        committed_ms: i64,
    ) -> Result<Vec<u8>, Error> {
        let prefix = load_join_prefix(&self.db, self.relay_public, path_family)?;
        if prefix.controls < 4 {
            return Err(Error::Invalid("challenge not committed"));
        }
        let challenge_committed = control_at(&self.db, path_family, 3)?;
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
            let committed = control_at(&self.db, path_family, 4)?;
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
        let tx = self.db.transaction()?;
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
        let tx = self.db.transaction()?;
        let prior: Option<Vec<u8>> = tx
            .query_row(
                "SELECT candidate_bytes FROM staged_admissions WHERE family_id=?1",
                params![&path_family[..]],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(saved) = prior {
            if saved != candidate_bytes {
                return Err(Error::Invalid("another admission staged"));
            }
        } else {
            tx.execute("INSERT INTO staged_admissions(family_id,transition_id,candidate_bytes) VALUES(?1,?2,?3)",
                params![&path_family[..],&admission.transition_id[..],&candidate_bytes])?;
        }
        let prior: Option<Vec<u8>> = tx
            .query_row(
                "SELECT object_bytes FROM staged_objects WHERE family_id=?1 AND object_id=?2",
                params![&path_family[..], &object_id[..]],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(saved) = prior {
            if saved != object_bytes {
                return Err(Error::Invalid("admission object ID collision"));
            }
        } else {
            tx.execute("INSERT INTO staged_objects(family_id,object_id,kind,object_hash,object_bytes) VALUES(?1,?2,?3,?4,?5)",
                params![&path_family[..],&object_id[..],i64::from(kind),&listed.object_hash[..],&object_bytes])?;
        }
        tx.commit()?;
        Ok(receipt::object_stage_response(&object_bytes)?)
    }

    pub fn commit_first_admission(
        &mut self,
        path_family: [u8; 16],
        candidate_bytes: &[u8],
        committed_ms: i64,
    ) -> Result<Vec<u8>, Error> {
        let prefix = load_proved_prefix(&self.db, self.relay_public, path_family)?;
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
            let committed = control_at(&self.db, path_family, 5)?;
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
        let tx = self.db.transaction()?;
        ensure_control_ids(&tx, path_family, candidate_bytes)?;
        let staged:Option<Vec<u8>>=tx.query_row("SELECT candidate_bytes FROM staged_admissions WHERE family_id=?1 AND transition_id=?2",
            params![&path_family[..],&admission.transition_id[..]],|r|r.get(0)).optional()?;
        if staged.as_deref() != Some(candidate_bytes) {
            return Err(Error::Invalid("admission candidate not staged"));
        }
        for entry in &admission.manifest {
            let staged:Option<(i64,Vec<u8>,Vec<u8>)>=tx.query_row(
                "SELECT kind,object_hash,object_bytes FROM staged_objects WHERE family_id=?1 AND object_id=?2",
                params![&path_family[..],&entry.object_id[..]],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)),
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
            "DELETE FROM staged_admissions WHERE family_id=?1",
            params![&path_family[..]],
        )?;
        for entry in &admission.manifest {
            tx.execute(
                "DELETE FROM staged_objects WHERE family_id=?1 AND object_id=?2",
                params![&path_family[..], &entry.object_id[..]],
            )?;
        }
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
        let prior: Option<Vec<u8>> = tx
            .query_row(
                "SELECT candidate_bytes FROM staged_removals WHERE family_id=?1",
                params![&path_family[..]],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(saved) = prior {
            if saved != candidate_bytes {
                return Err(Error::Invalid("another removal staged"));
            }
        } else {
            tx.execute(
                "INSERT INTO staged_removals(family_id,transition_id,candidate_bytes) VALUES(?1,?2,?3)",
                params![&path_family[..], &removal.transition_id[..], &candidate_bytes],
            )?;
        }
        let prior: Option<Vec<u8>> = tx
            .query_row(
                "SELECT object_bytes FROM staged_objects WHERE family_id=?1 AND object_id=?2",
                params![&path_family[..], &object_id[..]],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(saved) = prior {
            if saved != object_bytes {
                return Err(Error::Invalid("removal object ID collision"));
            }
        } else {
            let committed: Option<i64> = tx
                .query_row(
                    "SELECT 1 FROM committed_objects WHERE family_id=?1 AND object_id=?2",
                    params![&path_family[..], &object_id[..]],
                    |r| r.get(0),
                )
                .optional()?;
            if committed.is_some() {
                return Err(Error::Invalid("removal object ID reused"));
            }
            tx.execute(
                "INSERT INTO staged_objects(family_id,object_id,kind,object_hash,object_bytes) VALUES(?1,?2,?3,?4,?5)",
                params![&path_family[..], &object_id[..], i64::from(kind), &listed.object_hash[..], &object_bytes],
            )?;
        }
        tx.commit()?;
        Ok(receipt::object_stage_response(&object_bytes)?)
    }

    pub fn commit_first_removal(
        &mut self,
        path_family: [u8; 16],
        candidate_bytes: &[u8],
        committed_ms: i64,
    ) -> Result<Vec<u8>, Error> {
        let prefix = load_admitted_prefix(&self.db, self.relay_public, path_family)?;
        let removal = authority::verify_first_removal(
            candidate_bytes,
            &prefix.proved.join.genesis,
            &prefix.proved.join.issue,
            &prefix.proved.join.claim,
            &prefix.admission,
            prefix.admission_head,
        )?;
        if prefix.proved.join.controls >= 7 {
            let committed = control_at(&self.db, path_family, 6)?;
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
        ensure_first_removal_ids_unused(&self.db, path_family, &removal)?;
        let tx = self.db.transaction()?;
        ensure_control_ids(&tx, path_family, candidate_bytes)?;
        let saved: Option<Vec<u8>> = tx.query_row(
            "SELECT candidate_bytes FROM staged_removals WHERE family_id=?1 AND transition_id=?2",
            params![&path_family[..], &removal.transition_id[..]], |r| r.get(0),
        ).optional()?;
        if saved.as_deref() != Some(candidate_bytes) {
            return Err(Error::Invalid("removal candidate not staged"));
        }
        for entry in &removal.manifest {
            let staged: Option<(i64, Vec<u8>, Vec<u8>)> = tx.query_row(
                "SELECT kind,object_hash,object_bytes FROM staged_objects WHERE family_id=?1 AND object_id=?2",
                params![&path_family[..], &entry.object_id[..]],
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
            "DELETE FROM staged_removals WHERE family_id=?1",
            params![&path_family[..]],
        )?;
        for entry in &removal.manifest {
            tx.execute(
                "DELETE FROM staged_objects WHERE family_id=?1 AND object_id=?2",
                params![&path_family[..], &entry.object_id[..]],
            )?;
        }
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
        let controls = control_count(&self.db, path_family)?;
        if !(1..=7).contains(&controls) {
            return Err(Error::Invalid("initial-cohort batch authority superseded"));
        }
        let (genesis_bytes, genesis_committed): (Vec<u8>, Vec<u8>) = self.db.query_row(
            "SELECT candidate_bytes,committed_bytes FROM families WHERE family_id=?1 AND active=1",
            params![&path_family[..]],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        let genesis = authority::verify_genesis_candidate(&genesis_bytes, &self.relay_public)?;
        let author = batch_authority::claimed_author(envelope_bytes)?;
        let (signer, active_author) = if author == genesis.manager_id {
            (genesis.manager_signing_key, true)
        } else if controls >= 3 {
            let prefix = load_join_prefix(&self.db, self.relay_public, path_family)?;
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
        let tx = self.db.transaction()?;
        let prior: Option<(Vec<u8>, Vec<u8>)> = tx.query_row(
            "SELECT envelope_bytes,receipt_bytes FROM batch_results WHERE family_id=?1 AND batch_id=?2",
            params![&path_family[..], &batch.batch_id[..]],
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
            params![&path_family[..], &batch.batch_id[..]],
            |r| Ok((r.get(0)?, r.get(1)?)),
        ).optional()?;
        if let Some((old_envelope, old_receipt)) = prior_rejection {
            if old_envelope != envelope_bytes {
                return Err(Error::Invalid("batch ID reused with different bytes"));
            }
            return Ok(receipt::batch_commit_response(&old_receipt)?);
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
        tx.execute(
            "UPDATE families SET cursor=?2 WHERE family_id=?1 AND cursor=?3",
            params![
                &path_family[..],
                i64::try_from(next_cursor).map_err(|_| Error::Invalid("cursor range"))?,
                cursor
            ],
        )?;
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
        let candidate = authority::verify_genesis_candidate(&candidate_bytes, &self.relay_public)?;
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
        let genesis = authority::verify_genesis_candidate(&genesis_bytes, &self.relay_public)?;
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
    let genesis = authority::verify_genesis_candidate(&genesis_bytes, &relay_public)?;
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
