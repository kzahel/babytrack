//! Durable first active-device removal. Save one exact proposal before any
//! network stage/commit; a retry never creates another epoch key or grant.

use crate::{
    cbor::{self, Value},
    control_build,
    creation::ManagerCreation,
    crypto,
    rotation_build::{self, RotationProposal},
    shared_history::{self, PublicHistorySession},
    sqlite_store::{self, FamilyHandle, PreparedControlRow, SqliteStore},
};

type RemovalObject = (u16, [u8; 16], Vec<u8>);
type StagedBody = ([u8; 16], Vec<u8>);

#[derive(Debug)]
pub enum Error {
    Cbor(cbor::Error),
    Chain(crate::control_chain::Error),
    Crypto(crypto::Error),
    Creation(crate::creation::Error),
    Build(rotation_build::Error),
    History(shared_history::Error),
    Store(sqlite_store::Error),
    Random(getrandom::Error),
    Invalid(&'static str),
}
impl From<cbor::Error> for Error {
    fn from(value: cbor::Error) -> Self {
        Self::Cbor(value)
    }
}
impl From<crate::control_chain::Error> for Error {
    fn from(value: crate::control_chain::Error) -> Self {
        Self::Chain(value)
    }
}
impl From<crypto::Error> for Error {
    fn from(value: crypto::Error) -> Self {
        Self::Crypto(value)
    }
}
impl From<crate::creation::Error> for Error {
    fn from(value: crate::creation::Error) -> Self {
        Self::Creation(value)
    }
}
impl From<rotation_build::Error> for Error {
    fn from(value: rotation_build::Error) -> Self {
        Self::Build(value)
    }
}
impl From<shared_history::Error> for Error {
    fn from(value: shared_history::Error) -> Self {
        Self::History(value)
    }
}
impl From<sqlite_store::Error> for Error {
    fn from(value: sqlite_store::Error) -> Self {
        Self::Store(value)
    }
}
impl From<control_build::Error> for Error {
    fn from(value: control_build::Error) -> Self {
        match value {
            control_build::Error::Cbor(error) => Self::Cbor(error),
            control_build::Error::Crypto(error) => Self::Crypto(error),
            control_build::Error::Invalid(reason) => Self::Invalid(reason),
        }
    }
}
impl From<getrandom::Error> for Error {
    fn from(value: getrandom::Error) -> Self {
        Self::Random(value)
    }
}

pub struct FirstRemoval {
    family: FamilyHandle,
    target_id: [u8; 16],
    transition_id: [u8; 16],
    candidate_bytes: Vec<u8>,
    objects: Vec<RemovalObject>,
}

impl FirstRemoval {
    pub fn prepare(
        store: &mut SqliteStore,
        manager: &ManagerCreation,
        wrapping_key: &[u8; 32],
        target_id: [u8; 16],
    ) -> Result<Self, Error> {
        if store.prepared_control(manager.family(), 8)?.is_some() {
            let saved = Self::resume(store, manager, wrapping_key)?;
            if saved.target_id != target_id {
                return Err(Error::Invalid("another removal is already prepared"));
            }
            return Ok(saved);
        }
        manager.ready_session(store)?;
        let public = PublicHistorySession::resume(store, manager.family())?;
        let proposal = rotation_build::prepare_first_removal(
            public.chain(),
            manager.family(),
            manager.signing_seed(),
            manager.epoch_key(),
            target_id,
        )?;
        let secret_nonce = random::<24>()?;
        let secret_ciphertext = crypto::seal_with_nonce(
            wrapping_key,
            &secret_nonce,
            &secret_aad(manager.family(), proposal.transition_id)?,
            &cbor::encode(&Value::Array(vec![
                Value::Integer(1),
                Value::Bytes(target_id.to_vec()),
                Value::Bytes(proposal.new_epoch_key.to_vec()),
            ]))?,
        )?;
        let objects_bytes = encode_objects(&proposal)?;
        store.save_prepared_control(&PreparedControlRow {
            family: manager.family(),
            kind: 8,
            transition_id: proposal.transition_id,
            candidate_bytes: proposal.candidate_bytes,
            objects_bytes,
            secret_nonce,
            secret_ciphertext,
        })?;
        Self::resume(store, manager, wrapping_key)
    }

    pub fn resume(
        store: &SqliteStore,
        manager: &ManagerCreation,
        wrapping_key: &[u8; 32],
    ) -> Result<Self, Error> {
        let family = manager.family();
        let row = store
            .prepared_control(family, 8)?
            .ok_or(Error::Invalid("no prepared removal"))?;
        let secret = crypto::open(
            wrapping_key,
            &row.secret_nonce,
            &secret_aad(family, row.transition_id)?,
            &row.secret_ciphertext,
        )?;
        let Value::Array(secret) = cbor::decode(&secret)? else {
            return Err(Error::Invalid("removal secret not an array"));
        };
        if secret.len() != 3 || secret[0] != Value::Integer(1) {
            return Err(Error::Invalid("removal secret version"));
        }
        let target_id = fixed::<16>(&secret[1])?;
        let new_key = fixed::<32>(&secret[2])?;
        let Value::Map(candidate) = cbor::decode(&row.candidate_bytes)? else {
            return Err(Error::Invalid("removal candidate not a map"));
        };
        if candidate.len() != 2 || candidate[0].0 != 1 || candidate[1].0 != 2 {
            return Err(Error::Invalid("removal candidate keys"));
        }
        let Value::Map(unsigned) = &candidate[0].1 else {
            return Err(Error::Invalid("removal unsigned not a map"));
        };
        if unsigned.len() != 11
            || unsigned[5].1 != Value::Integer(8)
            || fixed::<16>(&unsigned[4].1)? != row.transition_id
            || fixed::<16>(&unsigned[1].1)? != family.family_id
        {
            return Err(Error::Invalid("removal identity mismatch"));
        }
        let Value::Map(delta) = &unsigned[6].1 else {
            return Err(Error::Invalid("removal delta not a map"));
        };
        if delta.len() != 3 || fixed::<16>(&delta[0].1)? != target_id {
            return Err(Error::Invalid("removal target mismatch"));
        }
        let commitment = crypto::hash(
            "epoch-key",
            &cbor::encode(&Value::Array(vec![
                Value::Bytes(family.family_id.to_vec()),
                Value::Integer(2),
                Value::Bytes(new_key.to_vec()),
            ]))?,
        )?;
        if fixed::<32>(&delta[2].1)? != commitment {
            return Err(Error::Invalid("saved rotation key differs from commitment"));
        }
        let objects = decode_objects(&row.objects_bytes)?;
        let Value::Array(manifest) = &unsigned[9].1 else {
            return Err(Error::Invalid("removal manifest not an array"));
        };
        if manifest.len() != objects.len() || objects.len() < 3 {
            return Err(Error::Invalid("removal object count mismatch"));
        }
        for (item, (kind, id, bytes)) in manifest.iter().zip(&objects) {
            let Value::Array(fields) = item else {
                return Err(Error::Invalid("manifest row not an array"));
            };
            if fields.len() != 4
                || fields[0] != Value::Integer((*kind).into())
                || fields[1] != Value::Bytes(id.to_vec())
                || fixed::<32>(&fields[2])? != crypto::hash("object", bytes)?
                || fields[3] != Value::Integer(bytes.len() as i128)
            {
                return Err(Error::Invalid("saved removal object differs from manifest"));
            }
        }
        let public = PublicHistorySession::resume(store, family)?;
        if public.chain().epoch()? == 1 {
            if public.head_hash() != fixed::<32>(&unsigned[3].1)? {
                return Err(Error::Invalid("prepared removal head is stale"));
            }
        } else if !committed_matches(store, family, &row.candidate_bytes)? {
            return Err(Error::Invalid(
                "committed removal differs from saved proposal",
            ));
        }
        Ok(Self {
            family,
            target_id,
            transition_id: row.transition_id,
            candidate_bytes: row.candidate_bytes,
            objects,
        })
    }

    pub fn target_id(&self) -> [u8; 16] {
        self.target_id
    }
    pub fn transition_id(&self) -> [u8; 16] {
        self.transition_id
    }
    pub fn candidate_bytes(&self) -> &[u8] {
        &self.candidate_bytes
    }
    pub fn stage_bodies(&self) -> Result<Vec<StagedBody>, Error> {
        self.objects
            .iter()
            .map(|(kind, id, bytes)| {
                Ok((
                    *id,
                    control_build::stage_body(&self.candidate_bytes, *kind, *id, bytes)?,
                ))
            })
            .collect()
    }
    pub fn confirm(
        &self,
        store: &mut SqliteStore,
        manager: &ManagerCreation,
        committed: &[u8],
    ) -> Result<(), Error> {
        let Value::Map(root) = cbor::decode(committed)? else {
            return Err(Error::Invalid("committed removal not a map"));
        };
        if root.len() != 4
            || cbor::encode(&Value::Map(vec![
                (1, root[0].1.clone()),
                (2, root[1].1.clone()),
            ]))? != self.candidate_bytes
        {
            return Err(Error::Invalid("committed removal candidate mismatch"));
        }
        let mut public = PublicHistorySession::resume(store, self.family)?;
        if public.chain().epoch()? == 1 {
            public.accept_control(store, committed)?;
        } else if !committed_matches(store, self.family, &self.candidate_bytes)? {
            return Err(Error::Invalid("different removal already committed"));
        }
        for (_, id, bytes) in &self.objects {
            public.accept_object(store, *id, bytes)?;
        }
        manager.ready_session(store)?;
        Ok(())
    }
}

fn committed_matches(
    store: &SqliteStore,
    family: FamilyHandle,
    candidate: &[u8],
) -> Result<bool, Error> {
    let history = store
        .shared_history(family)?
        .ok_or(Error::Invalid("shared history absent"))?;
    for entry in history.entries {
        if entry.kind != 1 {
            continue;
        }
        let Value::Map(root) = cbor::decode(&entry.committed_bytes)? else {
            continue;
        };
        if root.len() != 4 {
            continue;
        }
        let saved = cbor::encode(&Value::Map(vec![
            (1, root[0].1.clone()),
            (2, root[1].1.clone()),
        ]))?;
        if saved == candidate {
            return Ok(true);
        }
    }
    Ok(false)
}
fn encode_objects(proposal: &RotationProposal) -> Result<Vec<u8>, Error> {
    Ok(cbor::encode(&Value::Array(
        proposal
            .objects
            .iter()
            .map(|(kind, id, bytes)| {
                Value::Array(vec![
                    Value::Integer((*kind).into()),
                    Value::Bytes(id.to_vec()),
                    Value::Bytes(bytes.clone()),
                ])
            })
            .collect(),
    ))?)
}
fn decode_objects(bytes: &[u8]) -> Result<Vec<RemovalObject>, Error> {
    let Value::Array(rows) = cbor::decode(bytes)? else {
        return Err(Error::Invalid("objects not an array"));
    };
    rows.iter()
        .map(|row| {
            let Value::Array(fields) = row else {
                return Err(Error::Invalid("object row not an array"));
            };
            if fields.len() != 3 {
                return Err(Error::Invalid("object row width"));
            }
            let Value::Integer(kind) = fields[0] else {
                return Err(Error::Invalid("object kind not integer"));
            };
            let kind: u16 = kind
                .try_into()
                .map_err(|_| Error::Invalid("object kind outside u16"))?;
            let Value::Bytes(body) = &fields[2] else {
                return Err(Error::Invalid("object body not bytes"));
            };
            Ok((kind, fixed::<16>(&fields[1])?, body.clone()))
        })
        .collect()
}
fn fixed<const N: usize>(value: &Value) -> Result<[u8; N], Error> {
    let Value::Bytes(bytes) = value else {
        return Err(Error::Invalid("expected bytes"));
    };
    bytes
        .as_slice()
        .try_into()
        .map_err(|_| Error::Invalid("byte length"))
}
fn secret_aad(family: FamilyHandle, transition_id: [u8; 16]) -> Result<[u8; 32], Error> {
    Ok(crypto::hash(
        "prepared-removal-secret",
        &cbor::encode(&Value::Array(vec![
            Value::Bytes(family.family_id.to_vec()),
            Value::Bytes(family.device_id.to_vec()),
            Value::Bytes(transition_id.to_vec()),
        ]))?,
    )?)
}
fn random<const N: usize>() -> Result<[u8; N], Error> {
    let mut bytes = [0; N];
    getrandom::fill(&mut bytes)?;
    Ok(bytes)
}
