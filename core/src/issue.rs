//! First manager invitation, persisted before upload. A committed issue is
//! the only point at which its bearer link may be shown.

use crate::{
    bootstrap::{self, InvitationBootstrap},
    cbor::{self, Value},
    control_chain::{self, ControlChain},
    creation::ManagerCreation,
    crypto,
    shared_history::{self, PublicHistorySession},
    sqlite_store::{self, FamilyHandle, InviteIssueRow, SqliteStore},
};

#[derive(Debug)]
pub enum Error {
    Bootstrap(bootstrap::Error),
    Cbor(cbor::Error),
    Chain(control_chain::Error),
    Crypto(crypto::Error),
    History(shared_history::Error),
    Random(getrandom::Error),
    Store(sqlite_store::Error),
    Invalid(&'static str),
}
impl From<bootstrap::Error> for Error {
    fn from(v: bootstrap::Error) -> Self {
        Self::Bootstrap(v)
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
impl From<sqlite_store::Error> for Error {
    fn from(v: sqlite_store::Error) -> Self {
        Self::Store(v)
    }
}

pub struct FirstInviteIssue {
    family: FamilyHandle,
    invitation_id: [u8; 16],
    transition_id: [u8; 16],
    object_id: [u8; 16],
    object_bytes: Vec<u8>,
    candidate_bytes: Vec<u8>,
    invitation_seed: [u8; 32],
    fixed_role: u8,
}
impl FirstInviteIssue {
    pub fn prepare(
        store: &mut SqliteStore,
        creation: &ManagerCreation,
        local_wrapping_key: &[u8; 32],
        fixed_role: u8,
    ) -> Result<Self, Error> {
        if fixed_role != 1 && fixed_role != 2 {
            return Err(Error::Invalid("invitation role invalid"));
        }
        let family = creation.family();
        if store.first_invite_issue(family)?.is_some() {
            let existing = Self::resume(store, creation, local_wrapping_key)?;
            if existing.fixed_role != fixed_role {
                return Err(Error::Invalid("existing invitation has another role"));
            }
            return Ok(existing);
        }
        let public = PublicHistorySession::resume(store, family)?;
        let history = store
            .shared_history(family)?
            .ok_or(Error::Invalid("genesis absent"))?;
        if history.entries.iter().any(|entry| entry.kind == 1)
            || public.head_hash()
                != ControlChain::from_genesis(&history.genesis_bytes, creation.relay_public_key())?
                    .head_hash()
        {
            return Err(Error::Invalid("first invitation requires genesis head"));
        }
        let invitation_id = random_v4()?;
        let transition_id = random_v4()?;
        let object_id = random_v4()?;
        let invitation_seed = random::<32>()?;
        let nonce = random::<24>()?;
        let (candidate_bytes, object_bytes) = build(
            creation,
            invitation_id,
            transition_id,
            object_id,
            invitation_seed,
            fixed_role,
            nonce,
            &store
                .shared_history(family)?
                .ok_or(Error::Invalid("genesis absent"))?
                .genesis_bytes,
        )?;
        let secret_nonce = random::<24>()?;
        let secret = cbor::encode(&Value::Array(vec![
            Value::Integer(1),
            Value::Integer(fixed_role.into()),
            Value::Bytes(invitation_seed.to_vec()),
        ]))?;
        let aad = secret_aad(family, invitation_id, transition_id, object_id)?;
        let secret_ciphertext =
            crypto::seal_with_nonce(local_wrapping_key, &secret_nonce, &aad, &secret)?;
        store.save_first_invite_issue(&InviteIssueRow {
            family,
            invitation_id,
            transition_id,
            object_id,
            object_bytes,
            candidate_bytes,
            secret_nonce,
            secret_ciphertext,
        })?;
        Self::resume(store, creation, local_wrapping_key)
    }

    pub fn resume(
        store: &SqliteStore,
        creation: &ManagerCreation,
        local_wrapping_key: &[u8; 32],
    ) -> Result<Self, Error> {
        let family = creation.family();
        let row = store
            .first_invite_issue(family)?
            .ok_or(Error::Invalid("no durable first invitation"))?;
        let aad = secret_aad(family, row.invitation_id, row.transition_id, row.object_id)?;
        let secret = crypto::open(
            local_wrapping_key,
            &row.secret_nonce,
            &aad,
            &row.secret_ciphertext,
        )?;
        let value = cbor::decode_with_limits(
            &secret,
            cbor::Limits {
                max_bytes: 96,
                max_depth: 3,
            },
        )?;
        let Value::Array(parts) = value else {
            return Err(Error::Invalid("invite secret not array"));
        };
        if parts.len() != 3 || parts[0] != Value::Integer(1) {
            return Err(Error::Invalid("invite secret version"));
        }
        let fixed_role: u8 = number(&parts[1])?
            .try_into()
            .map_err(|_| Error::Invalid("invite role range"))?;
        let invitation_seed = fixed::<32>(&parts[2])?;
        let object = cbor::decode(&row.object_bytes)?;
        let Value::Map(object_fields) = object else {
            return Err(Error::Invalid("issue object not map"));
        };
        if object_fields.len() != 3 {
            return Err(Error::Invalid("issue object width"));
        }
        let nonce = fixed::<24>(&object_fields[1].1)?;
        let genesis = store
            .shared_history(family)?
            .ok_or(Error::Invalid("genesis absent"))?
            .genesis_bytes;
        let (candidate_bytes, object_bytes) = build(
            creation,
            row.invitation_id,
            row.transition_id,
            row.object_id,
            invitation_seed,
            fixed_role,
            nonce,
            &genesis,
        )?;
        if row.candidate_bytes != candidate_bytes || row.object_bytes != object_bytes {
            return Err(Error::Invalid(
                "invite candidate differs from durable secrets",
            ));
        }
        Ok(Self {
            family,
            invitation_id: row.invitation_id,
            transition_id: row.transition_id,
            object_id: row.object_id,
            object_bytes,
            candidate_bytes,
            invitation_seed,
            fixed_role,
        })
    }

    pub fn invitation_id(&self) -> [u8; 16] {
        self.invitation_id
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
    pub fn stage_body(&self) -> Result<Vec<u8>, Error> {
        let Value::Map(candidate) = cbor::decode(&self.candidate_bytes)? else {
            return Err(Error::Invalid("issue candidate not map"));
        };
        Ok(cbor::encode(&Value::Map(vec![
            (1, Value::Integer(1)),
            (2, candidate[0].1.clone()),
            (3, candidate[1].1.clone()),
            (4, Value::Integer(1)),
            (5, Value::Bytes(self.object_id.to_vec())),
            (6, Value::Bytes(self.object_bytes.clone())),
        ]))?)
    }

    pub fn confirm(
        &self,
        store: &mut SqliteStore,
        creation: &ManagerCreation,
        committed_issue: &[u8],
        relay_origin: &str,
    ) -> Result<InvitationBootstrap, Error> {
        if creation.family() != self.family {
            return Err(Error::Invalid("creation belongs to another Family"));
        }
        let Value::Map(root) = cbor::decode(committed_issue)? else {
            return Err(Error::Invalid("committed issue not map"));
        };
        if root.len() != 4 {
            return Err(Error::Invalid("committed issue width"));
        }
        let candidate = cbor::encode(&Value::Map(vec![
            (1, root[0].1.clone()),
            (2, root[1].1.clone()),
        ]))?;
        if candidate != self.candidate_bytes {
            return Err(Error::Invalid(
                "committed issue differs from prepared candidate",
            ));
        }
        let history = store
            .shared_history(self.family)?
            .ok_or(Error::Invalid("genesis absent"))?;
        let mut public = PublicHistorySession::resume(store, self.family)?;
        if history
            .entries
            .iter()
            .all(|entry| entry.committed_bytes != committed_issue)
        {
            public.accept_control(store, committed_issue)?;
        } else if history
            .entries
            .iter()
            .find(|entry| entry.kind == 1)
            .is_none_or(|entry| entry.committed_bytes != committed_issue)
        {
            return Err(Error::Invalid(
                "committed first issue differs from pinned history",
            ));
        }
        public.accept_object(store, self.object_id, &self.object_bytes)?;
        // Full data-ready replay also verifies the encrypted membership copy.
        creation
            .confirm(store, &history.genesis_bytes)
            .map_err(|_| Error::Invalid("membership object did not verify"))?;
        let prior_batches: Vec<(&[u8], &[u8])> = history
            .entries
            .iter()
            .take_while(|entry| entry.kind == 2)
            .map(|entry| {
                (
                    entry.committed_bytes.as_slice(),
                    entry.receipt_bytes.as_slice(),
                )
            })
            .collect();
        Ok(InvitationBootstrap::from_committed_issue_with_batches(
            relay_origin,
            creation.relay_public_key(),
            &history.genesis_bytes,
            committed_issue,
            self.invitation_seed,
            &prior_batches,
        )?)
    }
}

#[allow(clippy::too_many_arguments)]
fn build(
    creation: &ManagerCreation,
    invitation_id: [u8; 16],
    transition_id: [u8; 16],
    object_id: [u8; 16],
    invitation_seed: [u8; 32],
    role: u8,
    nonce: [u8; 24],
    genesis: &[u8],
) -> Result<(Vec<u8>, Vec<u8>), Error> {
    if role != 1 && role != 2 {
        return Err(Error::Invalid("invite role invalid"));
    }
    let family = creation.family();
    let chain = ControlChain::from_genesis(genesis, creation.relay_public_key())?;
    if chain.family_id() != family.family_id
        || chain.active_signing_public(family.device_id)? != creation.signing_public_key()
        || chain.epoch()? != 1
    {
        return Err(Error::Invalid("manager genesis context mismatch"));
    }
    let state = cbor::decode(&chain.state_bytes()?)?;
    let Value::Map(mut state_fields) = state else {
        return Err(Error::Invalid("auth state not map"));
    };
    let Value::Array(active) = &state_fields[4].1 else {
        return Err(Error::Invalid("active state not array"));
    };
    if active.len() != 1 || state_fields[6].1 != Value::Array(vec![]) {
        return Err(Error::Invalid("first invitation requires one manager"));
    }
    let invite_public = crypto::signing_public_key(&invitation_seed);
    let delta = Value::Map(vec![
        (1, Value::Bytes(invitation_id.to_vec())),
        (2, Value::Bytes(family.device_id.to_vec())),
        (3, Value::Bytes(invite_public.to_vec())),
        (4, Value::Integer(role.into())),
    ]);
    state_fields[6].1 = Value::Array(vec![Value::Array(vec![
        Value::Bytes(invitation_id.to_vec()),
        Value::Bytes(family.device_id.to_vec()),
        Value::Bytes(invite_public.to_vec()),
        Value::Integer(role.into()),
        Value::Bytes(transition_id.to_vec()),
        Value::Integer(1),
    ])]);
    let state_hash = crypto::hash("auth-state", &cbor::encode(&Value::Map(state_fields))?)?;
    let core = vec![
        Value::Integer(1),
        Value::Bytes(family.family_id.to_vec()),
        Value::Bytes(chain.relay_id().to_vec()),
        Value::Bytes(chain.head_hash().to_vec()),
        Value::Bytes(transition_id.to_vec()),
        Value::Integer(2),
        delta.clone(),
        Value::Bytes(state_hash.to_vec()),
        Value::Integer(1),
    ];
    let core_hash = crypto::hash(
        "transition-core",
        &cbor::encode(&Value::Array(core.clone()))?,
    )?;
    let membership = cbor::encode(&Value::Map(vec![
        (1, Value::Bytes(transition_id.to_vec())),
        (2, Value::Bytes(chain.head_hash().to_vec())),
        (3, Value::Bytes(state_hash.to_vec())),
        (4, Value::Integer(1)),
        (5, delta),
    ]))?;
    let aad = crypto::hash("membership-aad", &core_hash)?;
    let ciphertext = crypto::seal_with_nonce(&creation.epoch_key(), &nonce, &aad, &membership)?;
    let object_bytes = cbor::encode(&Value::Map(vec![
        (1, Value::Integer(1)),
        (2, Value::Bytes(nonce.to_vec())),
        (3, Value::Bytes(ciphertext)),
    ]))?;
    let unsigned = Value::Map(
        core.into_iter()
            .enumerate()
            .map(|(i, v)| (i as u64 + 1, v))
            .chain([
                (
                    10,
                    Value::Array(vec![Value::Array(vec![
                        Value::Integer(1),
                        Value::Bytes(object_id.to_vec()),
                        Value::Bytes(crypto::hash("object", &object_bytes)?.to_vec()),
                        Value::Integer(object_bytes.len() as i128),
                    ])]),
                ),
                (11, Value::Bytes(core_hash.to_vec())),
            ])
            .collect(),
    );
    let signature = crypto::sign_cbor(
        "control-transition",
        &cbor::encode(&unsigned)?,
        &creation.signing_seed(),
    )?;
    let candidate = cbor::encode(&Value::Map(vec![
        (1, unsigned),
        (
            2,
            Value::Array(vec![Value::Array(vec![
                Value::Bytes(family.device_id.to_vec()),
                Value::Bytes(signature.to_vec()),
            ])]),
        ),
    ]))?;
    Ok((candidate, object_bytes))
}
fn secret_aad(
    family: FamilyHandle,
    invitation_id: [u8; 16],
    transition_id: [u8; 16],
    object_id: [u8; 16],
) -> Result<[u8; 32], Error> {
    Ok(crypto::hash(
        "first-invite-aad",
        &cbor::encode(&Value::Array(vec![
            Value::Bytes(family.family_id.to_vec()),
            Value::Bytes(family.device_id.to_vec()),
            Value::Bytes(invitation_id.to_vec()),
            Value::Bytes(transition_id.to_vec()),
            Value::Bytes(object_id.to_vec()),
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
