//! First recipient key proof, signed and persisted before upload.

use crate::{
    cbor::{self, Value},
    control_build,
    control_chain::{self, ControlChain},
    crypto,
    enrollment::{self, EnrollmentAttempt},
    shared_history::{self, PublicHistorySession},
    sqlite_store::{self, FamilyHandle, PreparedControlRow, SqliteStore},
};

#[derive(Debug)]
pub enum Error {
    Cbor(cbor::Error),
    Chain(control_chain::Error),
    Crypto(crypto::Error),
    Enrollment(enrollment::Error),
    History(shared_history::Error),
    Random(getrandom::Error),
    Store(sqlite_store::Error),
    Invalid(&'static str),
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
impl From<enrollment::Error> for Error {
    fn from(v: enrollment::Error) -> Self {
        Self::Enrollment(v)
    }
}
impl From<shared_history::Error> for Error {
    fn from(v: shared_history::Error) -> Self {
        Self::History(v)
    }
}
impl From<sqlite_store::Error> for Error {
    fn from(v: sqlite_store::Error) -> Self {
        Self::Store(v)
    }
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

pub struct FirstProof {
    family: FamilyHandle,
    invitation_id: [u8; 16],
    transition_id: [u8; 16],
    candidate_bytes: Vec<u8>,
    proof_signature: [u8; 64],
}
impl FirstProof {
    pub fn prepare(
        store: &mut SqliteStore,
        enrollment: &EnrollmentAttempt,
        local_wrapping_key: &[u8; 32],
    ) -> Result<Self, Error> {
        let family = enrollment.family();
        if store.prepared_control(family, 5)?.is_some() {
            return Self::resume(store, enrollment, local_wrapping_key);
        }
        let public = PublicHistorySession::resume(store, family)?;
        if public.cursor() != 4 {
            return Err(Error::Invalid("proof requires committed challenge"));
        }
        let transition_id = random_v4()?;
        let (candidate_bytes, signature) = build(store, public.chain(), enrollment, transition_id)?;
        let secret_nonce = random::<24>()?;
        let secret_ciphertext = crypto::seal_with_nonce(
            local_wrapping_key,
            &secret_nonce,
            &secret_aad(family, enrollment.invitation_id(), transition_id)?,
            &cbor::encode(&Value::Array(vec![
                Value::Integer(1),
                Value::Bytes(signature.to_vec()),
            ]))?,
        )?;
        store.save_prepared_control(&PreparedControlRow {
            family,
            kind: 5,
            transition_id,
            candidate_bytes,
            objects_bytes: cbor::encode(&Value::Array(vec![]))?,
            secret_nonce,
            secret_ciphertext,
        })?;
        Self::resume(store, enrollment, local_wrapping_key)
    }
    pub fn resume(
        store: &SqliteStore,
        enrollment: &EnrollmentAttempt,
        local_wrapping_key: &[u8; 32],
    ) -> Result<Self, Error> {
        let family = enrollment.family();
        let row = store
            .prepared_control(family, 5)?
            .ok_or(Error::Invalid("no durable first proof"))?;
        if row.objects_bytes != cbor::encode(&Value::Array(vec![]))? {
            return Err(Error::Invalid("proof has unexpected objects"));
        }
        let plaintext = crypto::open(
            local_wrapping_key,
            &row.secret_nonce,
            &secret_aad(family, enrollment.invitation_id(), row.transition_id)?,
            &row.secret_ciphertext,
        )?;
        let Value::Array(secret) = cbor::decode(&plaintext)? else {
            return Err(Error::Invalid("proof secret not array"));
        };
        if secret.len() != 2 || secret[0] != Value::Integer(1) {
            return Err(Error::Invalid("proof secret version"));
        }
        let stored_signature = fixed::<64>(&secret[1])?;
        let chain = chain_through_challenge(store, family, enrollment.relay_public_key()?)?;
        let (candidate_bytes, signature) = build(store, &chain, enrollment, row.transition_id)?;
        if row.candidate_bytes != candidate_bytes || signature != stored_signature {
            return Err(Error::Invalid("proof differs from durable keys"));
        }
        Ok(Self {
            family,
            invitation_id: enrollment.invitation_id(),
            transition_id: row.transition_id,
            candidate_bytes,
            proof_signature: signature,
        })
    }
    pub fn candidate_bytes(&self) -> &[u8] {
        &self.candidate_bytes
    }
    pub fn invitation_id(&self) -> [u8; 16] {
        self.invitation_id
    }
    pub fn transition_id(&self) -> [u8; 16] {
        self.transition_id
    }
    pub fn proof_signature(&self) -> [u8; 64] {
        self.proof_signature
    }
    pub fn confirm(&self, store: &mut SqliteStore, committed: &[u8]) -> Result<(), Error> {
        let Value::Map(root) = cbor::decode(committed)? else {
            return Err(Error::Invalid("committed proof not map"));
        };
        let candidate = cbor::encode(&Value::Map(vec![
            (1, root[0].1.clone()),
            (2, root[1].1.clone()),
        ]))?;
        if candidate != self.candidate_bytes {
            return Err(Error::Invalid("proof candidate mismatch"));
        }
        let mut public = PublicHistorySession::resume(store, self.family)?;
        if public.cursor() == 4 {
            public.accept_control(store, committed)?;
        } else {
            let history = store
                .shared_history(self.family)?
                .ok_or(Error::Invalid("shared history absent"))?;
            if history
                .entries
                .get(3)
                .is_none_or(|entry| entry.kind != 1 || entry.committed_bytes != committed)
            {
                return Err(Error::Invalid("proof differs from pinned history"));
            }
        }
        Ok(())
    }
}

fn build(
    store: &SqliteStore,
    chain: &ControlChain,
    enrollment: &EnrollmentAttempt,
    transition_id: [u8; 16],
) -> Result<(Vec<u8>, [u8; 64]), Error> {
    let family = enrollment.family();
    let challenge = chain
        .latest_challenge(&enrollment.invitation_id())
        .ok_or(Error::Invalid("no verified challenge"))?;
    let objects = store.shared_objects(family)?;
    let hpke_object = objects
        .iter()
        .find(|(id, _)| *id == challenge.hpke_object_id())
        .ok_or(Error::Invalid("challenge HPKE object not downloaded"))?;
    let proof = enrollment.prove_challenge(challenge, &hpke_object.1)?;
    let Value::Map(mut state) = cbor::decode(&chain.state_bytes()?)? else {
        return Err(Error::Invalid("auth state not map"));
    };
    let Value::Array(pending) = &mut state[5].1 else {
        return Err(Error::Invalid("pending not array"));
    };
    if pending.len() != 1 {
        return Err(Error::Invalid("first proof expects one pending device"));
    }
    let Value::Array(row) = &mut pending[0] else {
        return Err(Error::Invalid("pending row not array"));
    };
    if row.len() != 9
        || fixed::<16>(&row[0])? != enrollment.invitation_id()
        || fixed::<16>(&row[1])? != family.device_id
        || row[7] != Value::Bytes(challenge.challenge_id.to_vec())
    {
        return Err(Error::Invalid("proof pending context mismatch"));
    }
    row[8] = Value::Bytes(proof.proof_hash.to_vec());
    let delta = Value::Map(vec![
        (1, Value::Bytes(enrollment.invitation_id().to_vec())),
        (2, Value::Bytes(family.device_id.to_vec())),
        (3, Value::Bytes(challenge.challenge_hash.to_vec())),
        (4, Value::Bytes(proof.signature.to_vec())),
    ]);
    let candidate = control_build::candidate(
        family,
        chain.relay_id(),
        chain.head_hash(),
        transition_id,
        5,
        delta,
        Value::Map(state),
        chain.epoch()?,
        &[],
        &enrollment.signing_seed(),
    )?;
    Ok((candidate, proof.signature))
}
fn chain_through_challenge(
    store: &SqliteStore,
    family: FamilyHandle,
    relay_public: [u8; 32],
) -> Result<ControlChain, Error> {
    let history = store
        .shared_history(family)?
        .ok_or(Error::Invalid("genesis missing"))?;
    let mut chain = ControlChain::from_genesis(&history.genesis_bytes, relay_public)?;
    for entry in history.entries.iter().take(3) {
        if entry.kind != 1 {
            return Err(Error::Invalid("initial join history not controls"));
        }
        chain.apply_control(&entry.committed_bytes)?;
    }
    if chain.last_global_cursor() != 4 {
        return Err(Error::Invalid("challenge history missing"));
    }
    Ok(chain)
}
fn secret_aad(
    family: FamilyHandle,
    invitation_id: [u8; 16],
    transition_id: [u8; 16],
) -> Result<[u8; 32], Error> {
    Ok(crypto::hash(
        "first-proof-aad",
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
