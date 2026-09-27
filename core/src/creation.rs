//! Durable sharing preparation for an existing local Family.

use sha2::{Digest, Sha256};

use crate::{
    cbor::{self, Value},
    crypto, hpke,
    shared_history::{self, PublicHistorySession},
    shared_ready::{self, NextUpload, ReadyFamilySession},
    sqlite_store::{self, FamilyHandle, ManagerCreationRow, PromotionChunkRow, SqliteStore},
    sync_wire,
};

#[derive(Debug)]
pub enum Error {
    Cbor(cbor::Error),
    Crypto(crypto::Error),
    Chain(crate::control_chain::Error),
    Hpke(hpke::Error),
    Random(getrandom::Error),
    Store(sqlite_store::Error),
    History(shared_history::Error),
    Ready(shared_ready::Error),
    Wire(sync_wire::Error),
    Invalid(&'static str),
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
impl From<crate::control_chain::Error> for Error {
    fn from(v: crate::control_chain::Error) -> Self {
        Self::Chain(v)
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
impl From<shared_history::Error> for Error {
    fn from(v: shared_history::Error) -> Self {
        Self::History(v)
    }
}
impl From<shared_ready::Error> for Error {
    fn from(v: shared_ready::Error) -> Self {
        Self::Ready(v)
    }
}
impl From<sync_wire::Error> for Error {
    fn from(v: sync_wire::Error) -> Self {
        Self::Wire(v)
    }
}

pub struct ManagerCreation {
    family: FamilyHandle,
    relay_public_key: [u8; 32],
    promotion_id: [u8; 16],
    transition_id: [u8; 16],
    object_id: [u8; 16],
    object_bytes: Vec<u8>,
    chunks: Vec<PromotionChunkRow>,
    candidate_bytes: Vec<u8>,
    signing_seed: [u8; 32],
    agreement_private: [u8; 32],
    epoch_key: [u8; 32],
}
type StagedBody = ([u8; 16], Vec<u8>);
impl ManagerCreation {
    /// Persist keys and exact signed candidate before the first relay POST.
    /// Existing preparation resumes byte-for-byte, even after later local
    /// edits above the saved watermark.
    pub fn prepare(
        store: &mut SqliteStore,
        family: FamilyHandle,
        relay_public_key: [u8; 32],
        local_wrapping_key: &[u8; 32],
    ) -> Result<Self, Error> {
        if store.manager_creation(family)?.is_some() {
            let existing = Self::resume(store, family, local_wrapping_key)?;
            if existing.relay_public_key != relay_public_key {
                return Err(Error::Invalid("sharing already pinned to another relay"));
            }
            return Ok(existing);
        }
        if store.shared_history(family)?.is_some() {
            return Err(Error::Invalid(
                "Family already has a different shared genesis",
            ));
        }
        let operations = store.local_operation_snapshot(family)?;
        let promotion_id = random_v4()?;
        let mut transition_id = random_v4()?;
        while transition_id == promotion_id {
            transition_id = random_v4()?;
        }
        let object_id = promotion_id;
        let signing_seed = random::<32>()?;
        let agreement_private = random::<32>()?;
        let epoch_key = random::<32>()?;
        let relay_id: [u8; 32] = Sha256::digest(relay_public_key).into();
        let chunks = promotion_chunks(family, relay_id, promotion_id, &operations, &epoch_key)?;
        let object_bytes = promotion_object(family.family_id, relay_id, promotion_id, &chunks)?;
        let candidate_bytes = genesis_candidate(
            family,
            relay_id,
            transition_id,
            object_id,
            &object_bytes,
            &chunks,
            &signing_seed,
            &agreement_private,
            &epoch_key,
        )?;
        let secret = cbor::encode(&Value::Array(vec![
            Value::Integer(1),
            Value::Bytes(signing_seed.to_vec()),
            Value::Bytes(agreement_private.to_vec()),
            Value::Bytes(epoch_key.to_vec()),
        ]))?;
        let secret_nonce = random::<24>()?;
        let aad = secret_aad(family, relay_public_key, promotion_id)?;
        let secret_ciphertext =
            crypto::seal_with_nonce(local_wrapping_key, &secret_nonce, &aad, &secret)?;
        store.save_manager_creation(&ManagerCreationRow {
            family,
            relay_public_key,
            promotion_id,
            transition_id,
            object_id,
            object_bytes,
            candidate_bytes,
            secret_nonce,
            secret_ciphertext,
            chunks,
        })?;
        Self::resume(store, family, local_wrapping_key)
    }

    pub fn resume(
        store: &SqliteStore,
        family: FamilyHandle,
        local_wrapping_key: &[u8; 32],
    ) -> Result<Self, Error> {
        let row = store
            .manager_creation(family)?
            .ok_or(Error::Invalid("no manager sharing preparation"))?;
        let aad = secret_aad(family, row.relay_public_key, row.promotion_id)?;
        let secret = crypto::open(
            local_wrapping_key,
            &row.secret_nonce,
            &aad,
            &row.secret_ciphertext,
        )?;
        let value = cbor::decode_with_limits(
            &secret,
            cbor::Limits {
                max_bytes: 128,
                max_depth: 3,
            },
        )?;
        let Value::Array(parts) = value else {
            return Err(Error::Invalid("manager secret not array"));
        };
        if parts.len() != 4 || parts[0] != Value::Integer(1) {
            return Err(Error::Invalid("manager secret version"));
        }
        let signing_seed = fixed::<32>(&parts[1])?;
        let agreement_private = fixed::<32>(&parts[2])?;
        let epoch_key = fixed::<32>(&parts[3])?;
        if row.object_id != row.promotion_id || row.transition_id == row.promotion_id {
            return Err(Error::Invalid("promotion and transition IDs invalid"));
        }
        let relay_id: [u8; 32] = Sha256::digest(row.relay_public_key).into();
        let object_bytes =
            promotion_object(family.family_id, relay_id, row.promotion_id, &row.chunks)?;
        let candidate_bytes = genesis_candidate(
            family,
            relay_id,
            row.transition_id,
            row.object_id,
            &object_bytes,
            &row.chunks,
            &signing_seed,
            &agreement_private,
            &epoch_key,
        )?;
        if object_bytes != row.object_bytes || candidate_bytes != row.candidate_bytes {
            return Err(Error::Invalid(
                "manager candidate differs from durable secrets",
            ));
        }
        Ok(Self {
            family,
            relay_public_key: row.relay_public_key,
            promotion_id: row.promotion_id,
            transition_id: row.transition_id,
            object_id: row.object_id,
            object_bytes,
            chunks: row.chunks,
            candidate_bytes,
            signing_seed,
            agreement_private,
            epoch_key,
        })
    }

    pub fn family(&self) -> FamilyHandle {
        self.family
    }
    pub fn promotion_id(&self) -> [u8; 16] {
        self.promotion_id
    }
    pub fn transition_id(&self) -> [u8; 16] {
        self.transition_id
    }
    pub fn object_id(&self) -> [u8; 16] {
        self.object_id
    }
    pub fn candidate_bytes(&self) -> &[u8] {
        &self.candidate_bytes
    }
    pub fn signing_public_key(&self) -> [u8; 32] {
        crypto::signing_public_key(&self.signing_seed)
    }
    pub fn ready_session(&self, store: &SqliteStore) -> Result<ReadyFamilySession, Error> {
        Ok(ReadyFamilySession::from_store(
            store,
            self.family,
            self.epoch_key,
            self.agreement_private,
        )?)
    }
    pub(crate) fn signing_seed(&self) -> [u8; 32] {
        self.signing_seed
    }
    pub(crate) fn epoch_key(&self) -> [u8; 32] {
        self.epoch_key
    }
    pub(crate) fn relay_public_key(&self) -> [u8; 32] {
        self.relay_public_key
    }
    pub fn stage_body(&self) -> Result<Vec<u8>, Error> {
        self.stage_body_for(6, self.object_id, &self.object_bytes)
    }
    pub fn stage_bodies(&self) -> Result<Vec<StagedBody>, Error> {
        let mut bodies = vec![(self.object_id, self.stage_body()?)];
        for chunk in &self.chunks {
            bodies.push((
                chunk.object_id,
                self.stage_body_for(7, chunk.object_id, &chunk.object_bytes)?,
            ));
        }
        Ok(bodies)
    }
    fn stage_body_for(
        &self,
        kind: u16,
        object_id: [u8; 16],
        bytes: &[u8],
    ) -> Result<Vec<u8>, Error> {
        let Value::Map(candidate) = cbor::decode(&self.candidate_bytes)? else {
            return Err(Error::Invalid("stored candidate not map"));
        };
        Ok(cbor::encode(&Value::Map(vec![
            (1, Value::Integer(1)),
            (2, candidate[0].1.clone()),
            (3, candidate[1].1.clone()),
            (4, Value::Integer(kind.into())),
            (5, Value::Bytes(object_id.to_vec())),
            (6, Value::Bytes(bytes.to_vec())),
        ]))?)
    }
    pub fn sign_get(&self, exact_path: &str) -> Result<sync_wire::SignedRead, Error> {
        let relay_id: [u8; 32] = Sha256::digest(self.relay_public_key).into();
        Ok(sync_wire::sign_get(
            self.family.family_id,
            relay_id,
            self.family.device_id,
            &self.signing_seed,
            exact_path,
        )?)
    }
    pub fn stage_next_local(
        &self,
        ready: &ReadyFamilySession,
        store: &mut SqliteStore,
    ) -> Result<NextUpload, Error> {
        if ready.family() != self.family {
            return Err(Error::Invalid("ready session belongs to another Family"));
        }
        Ok(ready.stage_next_local(store, &self.signing_seed)?)
    }
    pub fn verify_pending_proof(
        &self,
        store: &SqliteStore,
        invitation_id: [u8; 16],
        verifier_object: &[u8],
        proof_signature: [u8; 64],
    ) -> Result<(), Error> {
        let public = PublicHistorySession::resume(store, self.family)?;
        let key = public.chain().verify_initial_epoch_key(&self.epoch_key)?;
        public.chain().verify_latest_holder_proof(
            &invitation_id,
            verifier_object,
            &key,
            &proof_signature,
        )?;
        Ok(())
    }

    /// A relay response is a hint. Verify its exact committed genesis and
    /// persist that pin and manifest object before reporting data readiness.
    pub fn confirm(
        &self,
        store: &mut SqliteStore,
        committed_genesis: &[u8],
    ) -> Result<ReadyFamilySession, Error> {
        let value = cbor::decode(committed_genesis)?;
        let Value::Map(root) = value else {
            return Err(Error::Invalid("committed genesis not map"));
        };
        if root.len() != 4 {
            return Err(Error::Invalid("committed genesis width"));
        }
        let candidate = cbor::encode(&Value::Map(vec![
            (1, root[0].1.clone()),
            (2, root[1].1.clone()),
        ]))?;
        if candidate != self.candidate_bytes {
            return Err(Error::Invalid(
                "committed genesis differs from prepared candidate",
            ));
        }
        let public = PublicHistorySession::begin(
            store,
            self.family,
            committed_genesis,
            self.relay_public_key,
        )?;
        public.accept_object(store, self.object_id, &self.object_bytes)?;
        for chunk in &self.chunks {
            public.accept_object(store, chunk.object_id, &chunk.object_bytes)?;
        }
        let ready = ReadyFamilySession::from_store(
            store,
            self.family,
            self.epoch_key,
            self.agreement_private,
        )?;
        store.mark_promotion_accepted(
            self.family,
            self.chunks.last().map_or(0, |chunk| chunk.last_local_index),
        )?;
        Ok(ready)
    }
}

fn promotion_chunks(
    family: FamilyHandle,
    relay_id: [u8; 32],
    promotion_id: [u8; 16],
    operations: &[Vec<u8>],
    epoch_key: &[u8; 32],
) -> Result<Vec<PromotionChunkRow>, Error> {
    let mut chunks = Vec::new();
    let mut start = 0usize;
    while start < operations.len() {
        let mut end = start;
        let mut plain = Vec::new();
        while end < operations.len() && end - start < 256 {
            plain.push(Value::Bytes(operations[end].clone()));
            let candidate = cbor::encode(&Value::Array(plain.clone()))?;
            if candidate.len() > 256 * 1024 {
                plain.pop();
                break;
            }
            end += 1;
        }
        if end == start {
            return Err(Error::Invalid(
                "one local operation exceeds promotion chunk limit",
            ));
        }
        let index =
            u32::try_from(chunks.len()).map_err(|_| Error::Invalid("too many promotion chunks"))?;
        let mut object_id = random_v4()?;
        for _ in 0..8 {
            if object_id != promotion_id
                && !chunks
                    .iter()
                    .any(|chunk: &PromotionChunkRow| chunk.object_id == object_id)
            {
                break;
            }
            object_id = random_v4()?;
        }
        if object_id == promotion_id
            || chunks
                .iter()
                .any(|chunk: &PromotionChunkRow| chunk.object_id == object_id)
        {
            return Err(Error::Invalid("promotion object ID collision"));
        }
        let nonce = random::<24>()?;
        let header = cbor::encode(&Value::Array(vec![
            Value::Bytes(family.family_id.to_vec()),
            Value::Bytes(relay_id.to_vec()),
            Value::Bytes(promotion_id.to_vec()),
            Value::Integer(index.into()),
            Value::Integer(1),
            Value::Bytes(nonce.to_vec()),
        ]))?;
        let aad = crypto::hash("promotion-aad", &header)?;
        let ciphertext = crypto::seal_with_nonce(
            epoch_key,
            &nonce,
            &aad,
            &cbor::encode(&Value::Array(plain))?,
        )?;
        let object_bytes = cbor::encode(&Value::Map(vec![
            (1, Value::Integer(1)),
            (2, cbor::decode(&header)?),
            (3, Value::Bytes(ciphertext)),
        ]))?;
        if object_bytes.len() > 1024 * 1024 {
            return Err(Error::Invalid("promotion object exceeds relay limit"));
        }
        chunks.push(PromotionChunkRow {
            index,
            object_id,
            first_local_index: (start + 1) as u64,
            last_local_index: end as u64,
            object_bytes,
        });
        if chunks.len() > 16_383 {
            return Err(Error::Invalid("too many promotion chunks"));
        }
        start = end;
    }
    Ok(chunks)
}

fn promotion_object(
    family_id: [u8; 16],
    relay_id: [u8; 32],
    promotion_id: [u8; 16],
    chunks: &[PromotionChunkRow],
) -> Result<Vec<u8>, Error> {
    let mut next = 1u64;
    let mut rows = Vec::with_capacity(chunks.len());
    for (index, chunk) in chunks.iter().enumerate() {
        if chunk.index as usize != index
            || chunk.first_local_index != next
            || chunk.last_local_index < next
        {
            return Err(Error::Invalid("promotion chunk range invalid"));
        }
        next = chunk
            .last_local_index
            .checked_add(1)
            .ok_or(Error::Invalid("promotion watermark overflow"))?;
        rows.push(Value::Array(vec![
            Value::Integer(index as i128),
            Value::Bytes(chunk.object_id.to_vec()),
            Value::Bytes(crypto::hash("object", &chunk.object_bytes)?.to_vec()),
            Value::Integer(chunk.object_bytes.len() as i128),
            Value::Integer(chunk.first_local_index as i128),
            Value::Integer(chunk.last_local_index as i128),
        ]));
    }
    Ok(cbor::encode(&Value::Map(vec![
        (1, Value::Integer(1)),
        (2, Value::Bytes(family_id.to_vec())),
        (3, Value::Bytes(relay_id.to_vec())),
        (4, Value::Bytes(promotion_id.to_vec())),
        (5, Value::Integer((next - 1) as i128)),
        (6, Value::Array(rows)),
    ]))?)
}
#[allow(clippy::too_many_arguments)]
fn genesis_candidate(
    family: FamilyHandle,
    relay_id: [u8; 32],
    transition_id: [u8; 16],
    object_id: [u8; 16],
    object_bytes: &[u8],
    chunks: &[PromotionChunkRow],
    signing_seed: &[u8; 32],
    agreement_private: &[u8; 32],
    epoch_key: &[u8; 32],
) -> Result<Vec<u8>, Error> {
    let manager = Value::Array(vec![
        Value::Bytes(family.device_id.to_vec()),
        Value::Bytes(crypto::signing_public_key(signing_seed).to_vec()),
        Value::Bytes(hpke::public_key_from_private(agreement_private)?.to_vec()),
        Value::Integer(1),
        Value::Integer(2),
    ]);
    let epoch_commitment = crypto::hash(
        "epoch-key",
        &cbor::encode(&Value::Array(vec![
            Value::Bytes(family.family_id.to_vec()),
            Value::Integer(1),
            Value::Bytes(epoch_key.to_vec()),
        ]))?,
    )?;
    let state = Value::Map(vec![
        (1, Value::Integer(1)),
        (2, Value::Bytes(family.family_id.to_vec())),
        (3, Value::Bytes(relay_id.to_vec())),
        (4, Value::Integer(1)),
        (5, Value::Array(vec![manager.clone()])),
        (6, Value::Array(vec![])),
        (7, Value::Array(vec![])),
    ]);
    let delta = Value::Map(vec![
        (1, manager),
        (2, Value::Bytes(epoch_commitment.to_vec())),
        (
            3,
            Value::Bytes(crypto::hash("object", object_bytes)?.to_vec()),
        ),
    ]);
    let core = vec![
        Value::Integer(1),
        Value::Bytes(family.family_id.to_vec()),
        Value::Bytes(relay_id.to_vec()),
        Value::Bytes(vec![0; 32]),
        Value::Bytes(transition_id.to_vec()),
        Value::Integer(1),
        delta,
        Value::Bytes(crypto::hash("auth-state", &cbor::encode(&state)?)?.to_vec()),
        Value::Integer(1),
    ];
    let core_hash = crypto::hash(
        "transition-core",
        &cbor::encode(&Value::Array(core.clone()))?,
    )?;
    let mut manifest = vec![Value::Array(vec![
        Value::Integer(6),
        Value::Bytes(object_id.to_vec()),
        Value::Bytes(crypto::hash("object", object_bytes)?.to_vec()),
        Value::Integer(object_bytes.len() as i128),
    ])];
    let mut ordered = chunks.iter().collect::<Vec<_>>();
    ordered.sort_by_key(|chunk| chunk.object_id);
    manifest.extend(
        ordered
            .into_iter()
            .map(|chunk| {
                Ok(Value::Array(vec![
                    Value::Integer(7),
                    Value::Bytes(chunk.object_id.to_vec()),
                    Value::Bytes(crypto::hash("object", &chunk.object_bytes)?.to_vec()),
                    Value::Integer(chunk.object_bytes.len() as i128),
                ]))
            })
            .collect::<Result<Vec<_>, Error>>()?,
    );
    let unsigned = Value::Map(
        core.into_iter()
            .enumerate()
            .map(|(i, v)| (i as u64 + 1, v))
            .chain([
                (10, Value::Array(manifest)),
                (11, Value::Bytes(core_hash.to_vec())),
            ])
            .collect(),
    );
    let signed = crypto::sign_cbor(
        "control-transition",
        &cbor::encode(&unsigned)?,
        signing_seed,
    )?;
    Ok(cbor::encode(&Value::Map(vec![
        (1, unsigned),
        (
            2,
            Value::Array(vec![Value::Array(vec![
                Value::Bytes(family.device_id.to_vec()),
                Value::Bytes(signed.to_vec()),
            ])]),
        ),
    ]))?)
}
fn secret_aad(
    family: FamilyHandle,
    relay_public_key: [u8; 32],
    promotion_id: [u8; 16],
) -> Result<[u8; 32], Error> {
    Ok(crypto::hash(
        "manager-creation-aad",
        &cbor::encode(&Value::Array(vec![
            Value::Bytes(family.family_id.to_vec()),
            Value::Bytes(family.device_id.to_vec()),
            Value::Bytes(relay_public_key.to_vec()),
            Value::Bytes(promotion_id.to_vec()),
        ]))?,
    )?)
}
fn fixed<const N: usize>(value: &Value) -> Result<[u8; N], Error> {
    let Value::Bytes(bytes) = value else {
        return Err(Error::Invalid("expected secret bytes"));
    };
    bytes
        .as_slice()
        .try_into()
        .map_err(|_| Error::Invalid("secret length"))
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
