//! First verified admission and HPKE epoch-one grant.

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
type AdmissionObject = (u16, [u8; 16], Vec<u8>);

#[derive(Debug)]
pub enum Error {
    Cbor(cbor::Error),
    Chain(control_chain::Error),
    Creation(crate::creation::Error),
    Crypto(crypto::Error),
    History(shared_history::Error),
    Hpke(hpke::Error),
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
impl From<crate::creation::Error> for Error {
    fn from(v: crate::creation::Error) -> Self {
        Self::Creation(v)
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
impl From<control_build::Error> for Error {
    fn from(v: control_build::Error) -> Self {
        match v {
            control_build::Error::Cbor(error) => Self::Cbor(error),
            control_build::Error::Crypto(error) => Self::Crypto(error),
            control_build::Error::Invalid(reason) => Self::Invalid(reason),
        }
    }
}

pub struct FirstAdmission {
    family: FamilyHandle,
    invitation_id: [u8; 16],
    recipient_id: [u8; 16],
    transition_id: [u8; 16],
    candidate_bytes: Vec<u8>,
    objects: Vec<AdmissionObject>,
}
impl FirstAdmission {
    pub fn prepare(
        store: &mut SqliteStore,
        manager: &ManagerCreation,
        invitation_id: [u8; 16],
        recipient_id: [u8; 16],
        local_wrapping_key: &[u8; 32],
    ) -> Result<Self, Error> {
        let family = manager.family();
        if store.prepared_control(family, 6)?.is_some() {
            let existing = Self::resume(store, manager, local_wrapping_key)?;
            if existing.invitation_id != invitation_id || existing.recipient_id != recipient_id {
                return Err(Error::Invalid("another first admission already prepared"));
            }
            return Ok(existing);
        }
        let public =
            shared_history::first_join_chain(store, family, manager.relay_public_key(), 5)?;
        verify_committed_proof(store, manager, invitation_id)?;
        let pending = pending_row(&public, invitation_id, recipient_id)?;
        let transition_id = random_v4()?;
        let membership_id = random_v4()?;
        let grant_id = random_v4()?;
        let membership_nonce = random::<24>()?;
        let (delta, next_state, commitment) =
            admission_state(&public, manager, invitation_id, recipient_id, &pending)?;
        let core_hash = control_build::core_hash(
            family,
            public.relay_id(),
            public.head_hash(),
            transition_id,
            6,
            &delta,
            &next_state,
            1,
        )?;
        let membership_plain = cbor::encode(&Value::Map(vec![
            (1, Value::Bytes(transition_id.to_vec())),
            (2, Value::Bytes(public.head_hash().to_vec())),
            (
                3,
                Value::Bytes(crypto::hash("auth-state", &cbor::encode(&next_state)?)?.to_vec()),
            ),
            (4, Value::Integer(1)),
            (5, delta.clone()),
        ]))?;
        let membership_aad = crypto::hash("membership-aad", &core_hash)?;
        let membership_ciphertext = crypto::seal_with_nonce(
            &manager.epoch_key(),
            &membership_nonce,
            &membership_aad,
            &membership_plain,
        )?;
        let membership_object = cbor::encode(&Value::Map(vec![
            (1, Value::Integer(1)),
            (2, Value::Bytes(membership_nonce.to_vec())),
            (3, Value::Bytes(membership_ciphertext)),
        ]))?;
        let grant_context = cbor::encode(&Value::Array(vec![
            Value::Bytes(core_hash.to_vec()),
            Value::Bytes(invitation_id.to_vec()),
            Value::Bytes(recipient_id.to_vec()),
            Value::Integer(pending.role.into()),
            Value::Bytes(pending.agree_public.to_vec()),
            Value::Integer(pending.key_version.into()),
            Value::Integer(1),
            Value::Bytes(commitment.to_vec()),
        ]))?;
        let grant_info = crypto::hash("grant-info", &grant_context)?;
        let grant_plain = cbor::encode(&Value::Array(vec![
            Value::Integer(1),
            Value::Integer(1),
            Value::Bytes(manager.epoch_key().to_vec()),
        ]))?;
        let mut rng = ChaCha20Rng::from_seed(random::<32>()?);
        let sealed = hpke::seal_with_rng(
            &pending.agree_public,
            &grant_info,
            &grant_context,
            &grant_plain,
            &mut rng,
        )?;
        let grant_object = cbor::encode(&Value::Map(vec![
            (1, Value::Integer(1)),
            (2, Value::Bytes(grant_id.to_vec())),
            (3, Value::Integer(1)),
            (4, Value::Bytes(recipient_id.to_vec())),
            (5, Value::Integer(pending.key_version.into())),
            (
                6,
                Value::Array(vec![
                    Value::Integer(32),
                    Value::Integer(1),
                    Value::Integer(3),
                ]),
            ),
            (7, Value::Bytes(core_hash.to_vec())),
            (8, Value::Bytes(sealed.enc.to_vec())),
            (9, Value::Bytes(sealed.ciphertext)),
        ]))?;
        let objects = vec![
            (1, membership_id, membership_object),
            (4, grant_id, grant_object),
        ];
        let candidate_bytes = control_build::candidate(
            family,
            public.relay_id(),
            public.head_hash(),
            transition_id,
            6,
            delta,
            next_state,
            1,
            &objects,
            &manager.signing_seed(),
        )?;
        let objects_bytes = objects_bytes(&objects)?;
        let secret_nonce = random::<24>()?;
        let secret_ciphertext = crypto::seal_with_nonce(
            local_wrapping_key,
            &secret_nonce,
            &secret_aad(family, invitation_id, transition_id)?,
            &cbor::encode(&Value::Array(vec![
                Value::Integer(1),
                Value::Bytes(grant_id.to_vec()),
            ]))?,
        )?;
        store.save_prepared_control(&PreparedControlRow {
            family,
            kind: 6,
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
            .prepared_control(family, 6)?
            .ok_or(Error::Invalid("no durable first admission"))?;
        let Value::Map(candidate) = cbor::decode(&row.candidate_bytes)? else {
            return Err(Error::Invalid("admission candidate not map"));
        };
        let Value::Map(unsigned) = &candidate[0].1 else {
            return Err(Error::Invalid("admission unsigned not map"));
        };
        let Value::Map(delta) = &unsigned[6].1 else {
            return Err(Error::Invalid("admission delta not map"));
        };
        let invitation_id = fixed::<16>(&delta[0].1)?;
        let recipient_id = fixed::<16>(&delta[1].1)?;
        let plaintext = crypto::open(
            local_wrapping_key,
            &row.secret_nonce,
            &secret_aad(family, invitation_id, row.transition_id)?,
            &row.secret_ciphertext,
        )?;
        let Value::Array(secret) = cbor::decode(&plaintext)? else {
            return Err(Error::Invalid("admission secret not array"));
        };
        if secret.len() != 2 || secret[0] != Value::Integer(1) {
            return Err(Error::Invalid("admission secret version"));
        }
        let grant_id = fixed::<16>(&secret[1])?;
        let objects = parse_objects(&row.objects_bytes)?;
        if objects.len() != 2 || objects[0].0 != 1 || objects[1].0 != 4 || objects[1].1 != grant_id
        {
            return Err(Error::Invalid("admission objects invalid"));
        }
        let chain = chain_through_proof(store, family, manager.relay_public_key())?;
        let pending = pending_row(&chain, invitation_id, recipient_id)?;
        let (expected_delta, next_state, _) =
            admission_state(&chain, manager, invitation_id, recipient_id, &pending)?;
        let rebuilt = control_build::candidate(
            family,
            chain.relay_id(),
            chain.head_hash(),
            row.transition_id,
            6,
            expected_delta,
            next_state,
            1,
            &objects,
            &manager.signing_seed(),
        )?;
        if rebuilt != row.candidate_bytes {
            return Err(Error::Invalid("admission differs from durable authority"));
        }
        Ok(Self {
            family,
            invitation_id,
            recipient_id,
            transition_id: row.transition_id,
            candidate_bytes: row.candidate_bytes,
            objects,
        })
    }

    pub fn candidate_bytes(&self) -> &[u8] {
        &self.candidate_bytes
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
    pub fn confirm(
        &self,
        store: &mut SqliteStore,
        manager: &ManagerCreation,
        committed: &[u8],
    ) -> Result<(), Error> {
        let Value::Map(root) = cbor::decode(committed)? else {
            return Err(Error::Invalid("committed admission not map"));
        };
        let candidate = cbor::encode(&Value::Map(vec![
            (1, root[0].1.clone()),
            (2, root[1].1.clone()),
        ]))?;
        if candidate != self.candidate_bytes {
            return Err(Error::Invalid("admission candidate mismatch"));
        }
        let history = store
            .shared_history(self.family)?
            .ok_or(Error::Invalid("shared history absent"))?;
        let mut public = PublicHistorySession::resume(store, self.family)?;
        let committed_prior = history
            .entries
            .iter()
            .filter(|entry| entry.kind == 1)
            .nth(4);
        if committed_prior.is_none() {
            public.accept_control(store, committed)?;
        } else if committed_prior.is_some_and(|entry| entry.committed_bytes != committed) {
            return Err(Error::Invalid("admission differs from pinned history"));
        }
        for (_, id, bytes) in &self.objects {
            public.accept_object(store, *id, bytes)?;
        }
        manager.confirm(store, &history.genesis_bytes)?;
        Ok(())
    }
}

struct Pending {
    sign_public: [u8; 32],
    agree_public: [u8; 32],
    key_version: u32,
    role: u8,
}
fn pending_row(
    chain: &ControlChain,
    invitation_id: [u8; 16],
    recipient_id: [u8; 16],
) -> Result<Pending, Error> {
    let Value::Map(state) = cbor::decode(&chain.state_bytes()?)? else {
        return Err(Error::Invalid("auth state not map"));
    };
    let Value::Array(rows) = &state[5].1 else {
        return Err(Error::Invalid("pending not array"));
    };
    if rows.len() != 1 {
        return Err(Error::Invalid("first admission expects one pending device"));
    }
    let Value::Array(row) = &rows[0] else {
        return Err(Error::Invalid("pending row not array"));
    };
    if row.len() != 9
        || fixed::<16>(&row[0])? != invitation_id
        || fixed::<16>(&row[1])? != recipient_id
        || !matches!(&row[8], Value::Bytes(hash) if hash.len() == 32)
    {
        return Err(Error::Invalid("pending proof not committed"));
    }
    Ok(Pending {
        sign_public: fixed::<32>(&row[2])?,
        agree_public: fixed::<32>(&row[3])?,
        key_version: number(&row[4])?
            .try_into()
            .map_err(|_| Error::Invalid("key version range"))?,
        role: number(&row[5])?
            .try_into()
            .map_err(|_| Error::Invalid("role range"))?,
    })
}
fn admission_state(
    chain: &ControlChain,
    manager: &ManagerCreation,
    invitation_id: [u8; 16],
    recipient_id: [u8; 16],
    pending: &Pending,
) -> Result<(Value, Value, [u8; 32]), Error> {
    let family = manager.family();
    let commitment = crypto::hash(
        "epoch-key",
        &cbor::encode(&Value::Array(vec![
            Value::Bytes(family.family_id.to_vec()),
            Value::Integer(1),
            Value::Bytes(manager.epoch_key().to_vec()),
        ]))?,
    )?;
    let delta = Value::Map(vec![
        (1, Value::Bytes(invitation_id.to_vec())),
        (2, Value::Bytes(recipient_id.to_vec())),
        (3, Value::Integer(pending.role.into())),
        (4, Value::Bytes(commitment.to_vec())),
    ]);
    let Value::Map(mut state) = cbor::decode(&chain.state_bytes()?)? else {
        return Err(Error::Invalid("auth state not map"));
    };
    let Value::Array(pending_rows) = &mut state[5].1 else {
        return Err(Error::Invalid("pending not array"));
    };
    pending_rows.clear();
    let Value::Array(active_rows) = &mut state[4].1 else {
        return Err(Error::Invalid("active not array"));
    };
    active_rows.push(Value::Array(vec![
        Value::Bytes(recipient_id.to_vec()),
        Value::Bytes(pending.sign_public.to_vec()),
        Value::Bytes(pending.agree_public.to_vec()),
        Value::Integer(pending.key_version.into()),
        Value::Integer(pending.role.into()),
    ]));
    active_rows.sort_by(|a, b| {
        let Value::Array(a) = a else { unreachable!() };
        let Value::Array(b) = b else { unreachable!() };
        let (Value::Bytes(a), Value::Bytes(b)) = (&a[0], &b[0]) else {
            unreachable!()
        };
        a.cmp(b)
    });
    Ok((delta, Value::Map(state), commitment))
}
fn verify_committed_proof(
    store: &SqliteStore,
    manager: &ManagerCreation,
    invitation_id: [u8; 16],
) -> Result<(), Error> {
    let history = store
        .shared_history(manager.family())?
        .ok_or(Error::Invalid("shared history absent"))?;
    let proof = history
        .entries
        .iter()
        .filter(|entry| entry.kind == 1)
        .nth(3)
        .ok_or(Error::Invalid("proof not committed"))?;
    if proof.kind != 1 {
        return Err(Error::Invalid("proof entry not control"));
    }
    let Value::Map(root) = cbor::decode(&proof.committed_bytes)? else {
        return Err(Error::Invalid("proof not map"));
    };
    let Value::Map(unsigned) = &root[0].1 else {
        return Err(Error::Invalid("proof unsigned not map"));
    };
    let Value::Map(delta) = &unsigned[6].1 else {
        return Err(Error::Invalid("proof delta not map"));
    };
    let proof_signature = fixed::<64>(&delta[3].1)?;
    let chain = PublicHistorySession::resume(store, manager.family())?;
    let challenge = chain
        .chain()
        .latest_challenge(&invitation_id)
        .ok_or(Error::Invalid("challenge missing"))?;
    let verifier = store
        .shared_objects(manager.family())?
        .into_iter()
        .find(|(id, _)| *id == challenge.verifier_object_id())
        .ok_or(Error::Invalid("verifier object missing"))?;
    manager.verify_pending_proof(store, invitation_id, &verifier.1, proof_signature)?;
    Ok(())
}
fn chain_through_proof(
    store: &SqliteStore,
    family: FamilyHandle,
    relay_public: [u8; 32],
) -> Result<ControlChain, Error> {
    Ok(shared_history::first_join_chain(
        store,
        family,
        relay_public,
        5,
    )?)
}
fn objects_bytes(objects: &[AdmissionObject]) -> Result<Vec<u8>, Error> {
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
fn parse_objects(bytes: &[u8]) -> Result<Vec<AdmissionObject>, Error> {
    let Value::Array(items) = cbor::decode(bytes)? else {
        return Err(Error::Invalid("objects not array"));
    };
    if items.len() != 2 {
        return Err(Error::Invalid("admission object count"));
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
        "first-admission-aad",
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
