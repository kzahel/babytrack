//! Durable genesis reservation and commit. All methods are internal until
//! authenticated routes and the remaining authority transitions are ready.

use std::path::Path;

use babytrack_wire::{
    cbor::{self, Value},
    crypto,
};
use rusqlite::{Connection, OptionalExtension, params};

use crate::{authority, receipt};

#[derive(Debug)]
#[allow(dead_code)] // Detailed errors are mapped to protocol responses by routes.
pub(crate) enum Error {
    Sql(rusqlite::Error),
    Cbor(cbor::Error),
    Crypto(crypto::Error),
    Authority(authority::Error),
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
             CREATE TABLE IF NOT EXISTS committed_objects (
               family_id BLOB NOT NULL, object_id BLOB NOT NULL, kind INTEGER NOT NULL,
               object_hash BLOB NOT NULL, object_bytes BLOB NOT NULL,
               transition_id BLOB NOT NULL, PRIMARY KEY (family_id, object_id)
             );
             CREATE TABLE IF NOT EXISTS entries (
               family_id BLOB NOT NULL, cursor INTEGER NOT NULL, kind INTEGER NOT NULL,
               committed_bytes BLOB NOT NULL, PRIMARY KEY (family_id, cursor)
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

    pub fn stage_genesis_object(&mut self, body: &[u8]) -> Result<Vec<u8>, Error> {
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
        candidate_bytes: &[u8],
        committed_ms: i64,
    ) -> Result<Vec<u8>, Error> {
        let candidate = authority::verify_genesis_candidate(candidate_bytes, &self.relay_public)?;
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
                .commit_genesis(&candidate, time.try_into().unwrap())
                .is_err()
        );
        assert_eq!(store.stage_genesis_object(&stage).unwrap(), expected_stage);
        drop(store);
        let mut store = RelayStore::open(&path, seed).unwrap();
        assert_eq!(store.stage_genesis_object(&stage).unwrap(), expected_stage);
        assert!(store.genesis_result(family).unwrap().is_none());
        assert!(store.committed_object(family, object_id).unwrap().is_none());
        assert_eq!(
            store
                .commit_genesis(&candidate, time.try_into().unwrap())
                .unwrap(),
            expected_commit
        );
        assert_eq!(
            store
                .commit_genesis(&candidate, i64::try_from(time).unwrap() + 100)
                .unwrap(),
            expected_commit
        );
        assert_eq!(store.stage_genesis_object(&stage).unwrap(), expected_stage);
        drop(store);
        let store = RelayStore::open(&path, seed).unwrap();
        assert_eq!(store.genesis_result(family).unwrap().unwrap(), *committed);
        assert!(store.committed_object(family, object_id).unwrap().is_some());
        assert!(RelayStore::open(&path, [42; 32]).is_err());
    }
}
