//! Durable genesis reservation and commit. All methods are internal until
//! authenticated routes and the remaining authority transitions are ready.

use std::path::Path;

use babytrack_wire::{
    cbor::{self, Value},
    crypto,
};
use rusqlite::{Connection, OptionalExtension, params};

use crate::{authority, read_auth, receipt};

#[derive(Debug)]
#[allow(dead_code)] // Detailed errors are mapped to protocol responses by routes.
pub(crate) enum Error {
    Sql(rusqlite::Error),
    Cbor(cbor::Error),
    Crypto(crypto::Error),
    Authority(authority::Error),
    ReadAuth(read_auth::Error),
    Receipt(receipt::Error),
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
pub(crate) struct RelayStore {
    db: Connection,
    relay_seed: [u8; 32],
    relay_public: [u8; 32],
}

type SavedReservation = (Vec<u8>, Vec<u8>, i64, Option<Vec<u8>>);

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
        let head: [u8; 32] = head
            .ok_or(Error::Invalid("Family head absent"))?
            .try_into()
            .map_err(|_| Error::Invalid("head length"))?;
        let issue = authority::verify_first_invite_issue(
            &candidate_bytes,
            &genesis,
            if cursor == 1 {
                head
            } else {
                let committed: Vec<u8> = self.db.query_row(
                    "SELECT committed_bytes FROM entries WHERE family_id=?1 AND cursor=1",
                    params![&path_family[..]],
                    |r| r.get(0),
                )?;
                crypto::hash("control-head", &committed)?
            },
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
        if cursor == 2 {
            let existing: Option<Vec<u8>> = self.db.query_row(
                "SELECT object_bytes FROM committed_objects WHERE family_id=?1 AND object_id=?2 AND transition_id=?3",
                params![&path_family[..],&object_id[..],&issue.transition_id[..]], |r| r.get(0),
            ).optional()?;
            if existing.as_deref() == Some(object_bytes) {
                return Ok(receipt::object_stage_response(object_bytes)?);
            }
            return Err(Error::Invalid("issue already committed with other bytes"));
        }
        if cursor != 1 {
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
        let genesis_head = crypto::hash("control-head", &genesis_committed)?;
        let issue = authority::verify_first_invite_issue(candidate_bytes, &genesis, genesis_head)?;
        if issue.family_id != path_family {
            return Err(Error::Invalid("issue path Family mismatch"));
        }
        if cursor == 2 {
            let committed: Vec<u8> = self.db.query_row(
                "SELECT committed_bytes FROM entries WHERE family_id=?1 AND cursor=2",
                params![&path_family[..]],
                |r| r.get(0),
            )?;
            if control_candidate(&committed)? == candidate_bytes {
                return Ok(receipt::control_commit_response(&committed)?);
            }
            return Err(Error::Invalid("first issue already differs"));
        }
        if cursor != 1 || head != genesis_head {
            return Err(Error::Invalid("issue prior head stale"));
        }
        let genesis_time = control_commit_time(&genesis_committed)?;
        if committed_ms < genesis_time {
            return Err(Error::Invalid("relay time moved backward"));
        }
        let tx = self.db.transaction()?;
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
        let committed =
            receipt::commit_control(candidate_bytes, &self.relay_seed, 2, committed_ms)?;
        let next_head = crypto::hash("control-head", &committed)?;
        tx.execute("INSERT INTO committed_objects(family_id,object_id,kind,object_hash,object_bytes,transition_id) VALUES(?1,?2,1,?3,?4,?5)",
            params![&path_family[..],&issue.manifest.object_id[..],&hash,&bytes,&issue.transition_id[..]])?;
        tx.execute(
            "INSERT INTO entries(family_id,cursor,kind,committed_bytes) VALUES(?1,2,1,?2)",
            params![&path_family[..], &committed],
        )?;
        let updated = tx.execute("UPDATE families SET cursor=2,head_hash=?2 WHERE family_id=?1 AND cursor=1 AND head_hash=?3",
            params![&path_family[..],&next_head[..],&genesis_head[..]])?;
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
        let tx = self.db.transaction()?;
        let old: Option<Vec<u8>> = tx.query_row(
            "SELECT request_hash FROM read_requests WHERE family_id=?1 AND signer_id=?2 AND request_id=?3",
            params![&family_id[..], &verified.signer_id[..], &verified.request_id[..]], |r| r.get(0),
        ).optional()?;
        if let Some(hash) = old {
            if hash != verified.request_hash {
                return Err(Error::Invalid("request ID reused with different bytes"));
            }
        } else {
            tx.execute(
                "INSERT INTO read_requests(family_id,signer_id,request_id,request_hash) VALUES(?1,?2,?3,?4)",
                params![&family_id[..], &verified.signer_id[..], &verified.request_id[..], &verified.request_hash[..]],
            )?;
        }
        tx.commit()?;
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
    }
}
