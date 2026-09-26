//! First manager-to-recipient key challenge, stored before upload.

use rand_chacha::{ChaCha20Rng, rand_core::SeedableRng};

use crate::{
    cbor::{self, Value},
    control_build,
    control_chain::{self, ControlChain},
    creation::ManagerCreation,
    crypto, hpke,
    shared_history::{self, PublicHistorySession},
    sqlite_store::{self, FamilyHandle, PreparedControlRow, SqliteStore},
};

type StagedBody = ([u8; 16], Vec<u8>);
type ChallengeObject = (u16, [u8; 16], Vec<u8>);

#[derive(Debug)]
pub enum Error {
    Cbor(cbor::Error),
    Chain(control_chain::Error),
    Crypto(crypto::Error),
    History(shared_history::Error),
    Hpke(hpke::Error),
    Random(getrandom::Error),
    Store(sqlite_store::Error),
    Invalid(&'static str),
}
impl From<control_build::Error> for Error {
    fn from(v: control_build::Error) -> Self {
        match v {
            control_build::Error::Cbor(error) => Self::Cbor(error),
            control_build::Error::Crypto(error) => Self::Crypto(error),
            control_build::Error::Invalid(reason) => Self::Invalid(reason),
        }
    }
}
impl From<cbor::Error> for Error {
    fn from(v: cbor::Error) -> Self {
        Self::Cbor(v)
    }
}
impl From<control_chain::Error> for Error {
    fn from(v: control_chain::Error) -> Self {
        Self::Chain(v)
    }
}
impl From<crypto::Error> for Error {
    fn from(v: crypto::Error) -> Self {
        Self::Crypto(v)
    }
}
impl From<shared_history::Error> for Error {
    fn from(v: shared_history::Error) -> Self {
        Self::History(v)
    }
}
impl From<hpke::Error> for Error {
    fn from(v: hpke::Error) -> Self {
        Self::Hpke(v)
    }
}
impl From<sqlite_store::Error> for Error {
    fn from(v: sqlite_store::Error) -> Self {
        Self::Store(v)
    }
}

pub struct FirstChallenge {
    family: FamilyHandle,
    invitation_id: [u8; 16],
    pending_device_id: [u8; 16],
    transition_id: [u8; 16],
    challenge_id: [u8; 16],
    candidate_bytes: Vec<u8>,
    objects: Vec<ChallengeObject>,
}

impl FirstChallenge {
    pub fn prepare(
        store: &mut SqliteStore,
        manager: &ManagerCreation,
        invitation_id: [u8; 16],
        pending_device_id: [u8; 16],
        local_wrapping_key: &[u8; 32],
    ) -> Result<Self, Error> {
        let family = manager.family();
        if store.prepared_control(family, 11)?.is_some() {
            let existing = Self::resume(store, manager, local_wrapping_key)?;
            if existing.invitation_id != invitation_id
                || existing.pending_device_id != pending_device_id
            {
                return Err(Error::Invalid("another first challenge already prepared"));
            }
            return Ok(existing);
        }
        let chain = PublicHistorySession::resume(store, family)?;
        if chain.cursor() != 3 {
            return Err(Error::Invalid(
                "challenge requires committed claim at cursor three",
            ));
        }
        let pending = pending_row(chain.chain(), invitation_id, pending_device_id)?;
        let transition_id = random_v4()?;
        let challenge_id = random_v4()?;
        let hpke_id = random_v4()?;
        let verifier_id = random_v4()?;
        let secret = random::<32>()?;
        let verifier_nonce = random::<24>()?;
        let context = context(
            chain.chain(),
            invitation_id,
            pending_device_id,
            pending.claim_hash,
            challenge_id,
            pending.agree_public,
            pending.key_version,
        )?;
        let plaintext = cbor::encode(&Value::Array(vec![
            Value::Integer(1),
            Value::Bytes(secret.to_vec()),
        ]))?;
        let info = crypto::hash("challenge-info", &context)?;
        let mut rng = ChaCha20Rng::from_seed(random::<32>()?);
        let sealed =
            hpke::seal_with_rng(&pending.agree_public, &info, &context, &plaintext, &mut rng)?;
        let hpke_object = cbor::encode(&Value::Map(vec![
            (1, Value::Integer(1)),
            (2, Value::Bytes(challenge_id.to_vec())),
            (3, Value::Bytes(pending_device_id.to_vec())),
            (4, Value::Integer(pending.key_version.into())),
            (5, Value::Bytes(context.clone())),
            (6, Value::Bytes(sealed.enc.to_vec())),
            (7, Value::Bytes(sealed.ciphertext)),
        ]))?;
        let aad = crypto::hash("challenge-verifier-aad", &context)?;
        let verifier_ciphertext =
            crypto::seal_with_nonce(&manager.epoch_key(), &verifier_nonce, &aad, &plaintext)?;
        let verifier_object = cbor::encode(&Value::Map(vec![
            (1, Value::Integer(1)),
            (2, Value::Bytes(challenge_id.to_vec())),
            (
                3,
                Value::Bytes(crypto::hash("challenge-context", &context)?.to_vec()),
            ),
            (4, Value::Bytes(verifier_nonce.to_vec())),
            (5, Value::Bytes(verifier_ciphertext)),
        ]))?;
        let objects = vec![(2, hpke_id, hpke_object), (3, verifier_id, verifier_object)];
        let candidate_bytes = build_candidate(
            chain.chain(),
            manager,
            invitation_id,
            pending_device_id,
            transition_id,
            challenge_id,
            secret,
            &objects,
        )?;
        let objects_bytes = objects_bytes(&objects)?;
        let secret_nonce = random::<24>()?;
        let secret_ciphertext = crypto::seal_with_nonce(
            local_wrapping_key,
            &secret_nonce,
            &secret_aad(family, invitation_id, transition_id)?,
            &cbor::encode(&Value::Array(vec![
                Value::Integer(1),
                Value::Bytes(secret.to_vec()),
            ]))?,
        )?;
        store.save_prepared_control(&PreparedControlRow {
            family,
            kind: 11,
            transition_id,
            candidate_bytes,
            objects_bytes,
            secret_nonce,
            secret_ciphertext,
        })?;
        Self::resume(store, manager, local_wrapping_key)
    }

    pub fn resume(
        store: &SqliteStore,
        manager: &ManagerCreation,
        local_wrapping_key: &[u8; 32],
    ) -> Result<Self, Error> {
        let family = manager.family();
        let row = store
            .prepared_control(family, 11)?
            .ok_or(Error::Invalid("no prepared first challenge"))?;
        let candidate = cbor::decode(&row.candidate_bytes)?;
        let Value::Map(parts) = candidate else {
            return Err(Error::Invalid("challenge candidate not map"));
        };
        let Value::Map(unsigned) = &parts[0].1 else {
            return Err(Error::Invalid("unsigned challenge not map"));
        };
        let Value::Map(delta) = &unsigned[6].1 else {
            return Err(Error::Invalid("challenge delta not map"));
        };
        let invitation_id = fixed::<16>(&delta[0].1)?;
        let pending_device_id = fixed::<16>(&delta[1].1)?;
        let challenge_id = fixed::<16>(&delta[2].1)?;
        let secret_plain = crypto::open(
            local_wrapping_key,
            &row.secret_nonce,
            &secret_aad(family, invitation_id, row.transition_id)?,
            &row.secret_ciphertext,
        )?;
        let Value::Array(secret_parts) = cbor::decode(&secret_plain)? else {
            return Err(Error::Invalid("challenge secret not array"));
        };
        if secret_parts.len() != 2 || secret_parts[0] != Value::Integer(1) {
            return Err(Error::Invalid("challenge secret version"));
        }
        let secret = fixed::<32>(&secret_parts[1])?;
        let objects = parse_objects(&row.objects_bytes)?;
        let chain = base_chain(store, family, manager.relay_public_key())?;
        let rebuilt = build_candidate(
            &chain,
            manager,
            invitation_id,
            pending_device_id,
            row.transition_id,
            challenge_id,
            secret,
            &objects,
        )?;
        if row.candidate_bytes != rebuilt || objects.len() != 2 {
            return Err(Error::Invalid("challenge differs from durable secrets"));
        }
        Ok(Self {
            family,
            invitation_id,
            pending_device_id,
            transition_id: row.transition_id,
            challenge_id,
            candidate_bytes: row.candidate_bytes,
            objects,
        })
    }

    pub fn candidate_bytes(&self) -> &[u8] {
        &self.candidate_bytes
    }
    pub fn challenge_id(&self) -> [u8; 16] {
        self.challenge_id
    }
    pub fn transition_id(&self) -> [u8; 16] {
        self.transition_id
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
    pub fn confirm(&self, store: &mut SqliteStore, committed: &[u8]) -> Result<(), Error> {
        let Value::Map(root) = cbor::decode(committed)? else {
            return Err(Error::Invalid("committed challenge not map"));
        };
        let candidate = cbor::encode(&Value::Map(vec![
            (1, root[0].1.clone()),
            (2, root[1].1.clone()),
        ]))?;
        if candidate != self.candidate_bytes {
            return Err(Error::Invalid("challenge candidate mismatch"));
        }
        let mut public = PublicHistorySession::resume(store, self.family)?;
        if public.cursor() == 3 {
            public.accept_control(store, committed)?;
        } else {
            let history = store
                .shared_history(self.family)?
                .ok_or(Error::Invalid("shared history missing"))?;
            if history
                .entries
                .get(2)
                .is_none_or(|entry| entry.kind != 1 || entry.committed_bytes != committed)
            {
                return Err(Error::Invalid("committed challenge differs from history"));
            }
        }
        for (_, id, bytes) in &self.objects {
            public.accept_object(store, *id, bytes)?;
        }
        Ok(())
    }
}

struct PendingRow {
    agree_public: [u8; 32],
    key_version: u32,
    claim_hash: [u8; 32],
}
fn pending_row(
    chain: &ControlChain,
    invitation_id: [u8; 16],
    device_id: [u8; 16],
) -> Result<PendingRow, Error> {
    let Value::Map(state) = cbor::decode(&chain.state_bytes()?)? else {
        return Err(Error::Invalid("auth state not map"));
    };
    let Value::Array(pending) = &state[5].1 else {
        return Err(Error::Invalid("pending state not array"));
    };
    if pending.len() != 1 {
        return Err(Error::Invalid("first challenge expects one pending device"));
    }
    let Value::Array(row) = &pending[0] else {
        return Err(Error::Invalid("pending row not array"));
    };
    if row.len() != 9
        || fixed::<16>(&row[0])? != invitation_id
        || fixed::<16>(&row[1])? != device_id
        || row[7] != Value::Null
        || row[8] != Value::Null
    {
        return Err(Error::Invalid("pending challenge target invalid"));
    }
    Ok(PendingRow {
        agree_public: fixed::<32>(&row[3])?,
        key_version: number(&row[4])?
            .try_into()
            .map_err(|_| Error::Invalid("key version range"))?,
        claim_hash: fixed::<32>(&row[6])?,
    })
}
fn context(
    chain: &ControlChain,
    invitation_id: [u8; 16],
    device_id: [u8; 16],
    claim_hash: [u8; 32],
    challenge_id: [u8; 16],
    agree_public: [u8; 32],
    key_version: u32,
) -> Result<Vec<u8>, Error> {
    Ok(cbor::encode(&Value::Array(vec![
        Value::Bytes(chain.family_id().to_vec()),
        Value::Bytes(chain.relay_id().to_vec()),
        Value::Bytes(invitation_id.to_vec()),
        Value::Bytes(device_id.to_vec()),
        Value::Bytes(claim_hash.to_vec()),
        Value::Bytes(challenge_id.to_vec()),
        Value::Bytes(agree_public.to_vec()),
        Value::Integer(key_version.into()),
        Value::Bytes(chain.head_hash().to_vec()),
    ]))?)
}
#[allow(clippy::too_many_arguments)]
fn build_candidate(
    chain: &ControlChain,
    manager: &ManagerCreation,
    invitation_id: [u8; 16],
    pending_device_id: [u8; 16],
    transition_id: [u8; 16],
    challenge_id: [u8; 16],
    secret: [u8; 32],
    objects: &[(u16, [u8; 16], Vec<u8>)],
) -> Result<Vec<u8>, Error> {
    let pending = pending_row(chain, invitation_id, pending_device_id)?;
    let context = context(
        chain,
        invitation_id,
        pending_device_id,
        pending.claim_hash,
        challenge_id,
        pending.agree_public,
        pending.key_version,
    )?;
    let challenge_hash = crypto::hash(
        "challenge",
        &cbor::encode(&Value::Array(vec![
            Value::Bytes(context),
            Value::Bytes(secret.to_vec()),
        ]))?,
    )?;
    let delta = Value::Map(vec![
        (1, Value::Bytes(invitation_id.to_vec())),
        (2, Value::Bytes(pending_device_id.to_vec())),
        (3, Value::Bytes(challenge_id.to_vec())),
        (4, Value::Bytes(challenge_hash.to_vec())),
    ]);
    let Value::Map(mut state) = cbor::decode(&chain.state_bytes()?)? else {
        return Err(Error::Invalid("auth state not map"));
    };
    let Value::Array(rows) = &mut state[5].1 else {
        return Err(Error::Invalid("pending state not array"));
    };
    let Value::Array(row) = &mut rows[0] else {
        return Err(Error::Invalid("pending row not array"));
    };
    row[7] = Value::Bytes(challenge_id.to_vec());
    row[8] = Value::Null;
    Ok(control_build::candidate(
        manager.family(),
        chain.relay_id(),
        chain.head_hash(),
        transition_id,
        11,
        delta,
        Value::Map(state),
        chain.epoch()?,
        objects,
        &manager.signing_seed(),
    )?)
}
fn base_chain(
    store: &SqliteStore,
    family: FamilyHandle,
    relay_public: [u8; 32],
) -> Result<ControlChain, Error> {
    let history = store
        .shared_history(family)?
        .ok_or(Error::Invalid("genesis missing"))?;
    let mut chain = ControlChain::from_genesis(&history.genesis_bytes, relay_public)?;
    for entry in history.entries.iter().take(2) {
        if entry.kind != 1 {
            return Err(Error::Invalid("initial join history not controls"));
        }
        chain.apply_control(&entry.committed_bytes)?;
    }
    if chain.last_global_cursor() != 3 {
        return Err(Error::Invalid("claim history missing"));
    }
    Ok(chain)
}
fn objects_bytes(objects: &[ChallengeObject]) -> Result<Vec<u8>, Error> {
    Ok(cbor::encode(&Value::Array(
        objects
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
fn parse_objects(bytes: &[u8]) -> Result<Vec<ChallengeObject>, Error> {
    let Value::Array(items) = cbor::decode(bytes)? else {
        return Err(Error::Invalid("objects not array"));
    };
    if items.len() != 2 {
        return Err(Error::Invalid("challenge object count"));
    }
    items
        .iter()
        .map(|item| {
            let Value::Array(fields) = item else {
                return Err(Error::Invalid("object row not array"));
            };
            if fields.len() != 3 {
                return Err(Error::Invalid("object row width"));
            }
            let Value::Bytes(body) = &fields[2] else {
                return Err(Error::Invalid("object body not bytes"));
            };
            Ok((
                number(&fields[0])?
                    .try_into()
                    .map_err(|_| Error::Invalid("object kind range"))?,
                fixed::<16>(&fields[1])?,
                body.clone(),
            ))
        })
        .collect()
}
fn secret_aad(
    family: FamilyHandle,
    invitation_id: [u8; 16],
    transition_id: [u8; 16],
) -> Result<[u8; 32], Error> {
    Ok(crypto::hash(
        "first-challenge-aad",
        &cbor::encode(&Value::Array(vec![
            Value::Bytes(family.family_id.to_vec()),
            Value::Bytes(family.device_id.to_vec()),
            Value::Bytes(invitation_id.to_vec()),
            Value::Bytes(transition_id.to_vec()),
        ]))?,
    )?)
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
    let Value::Integer(n) = value else {
        return Err(Error::Invalid("expected integer"));
    };
    (*n).try_into()
        .map_err(|_| Error::Invalid("unsigned integer"))
}
fn random<const N: usize>() -> Result<[u8; N], Error> {
    let mut bytes = [0; N];
    getrandom::fill(&mut bytes).map_err(Error::Random)?;
    Ok(bytes)
}
fn random_v4() -> Result<[u8; 16], Error> {
    let mut bytes = random::<16>()?;
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Ok(bytes)
}
