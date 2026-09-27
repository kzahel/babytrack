//! First manager invitation, persisted before upload. A committed issue is
//! the only point at which its bearer link may be shown.

use crate::{
    bootstrap::{self, InvitationBootstrap},
    cbor::{self, Value},
    control_chain::{self, ControlChain},
    creation::ManagerCreation,
    crypto,
    enrollment::EnrollmentAttempt,
    projection::VerifiedEpochKey,
    shared_history::{self, PublicHistorySession},
    shared_ready::{self, ReadyFamilySession},
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
    Authority(babytrack_wire::authority::Error),
    Creation(crate::creation::Error),
    Ready(shared_ready::Error),
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
impl From<babytrack_wire::authority::Error> for Error {
    fn from(value: babytrack_wire::authority::Error) -> Self {
        Self::Authority(value)
    }
}
impl From<crate::creation::Error> for Error {
    fn from(value: crate::creation::Error) -> Self {
        Self::Creation(value)
    }
}
impl From<shared_ready::Error> for Error {
    fn from(value: shared_ready::Error) -> Self {
        Self::Ready(value)
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

/// A later issue is tied to an already verified shared head. Its exact
/// candidate, membership object, and secret inputs survive process death.
/// The bearer link remains unavailable until a committed issue is verified.
pub struct LaterInviteIssue {
    family: FamilyHandle,
    invitation_id: [u8; 16],
    transition_id: [u8; 16],
    object_id: [u8; 16],
    candidate_bytes: Vec<u8>,
    object_bytes: Vec<u8>,
    invitation_seed: [u8; 32],
    epoch_key: VerifiedEpochKey,
}

impl LaterInviteIssue {
    pub fn prepare_for_initial_manager(
        store: &mut SqliteStore,
        creation: &ManagerCreation,
        local_wrapping_key: &[u8; 32],
        fixed_role: u8,
    ) -> Result<Self, Error> {
        let ready = creation.ready_session(store)?;
        Self::prepare(
            store,
            &ready,
            creation.signing_seed(),
            local_wrapping_key,
            fixed_role,
        )
    }

    pub fn prepare_for_admitted_manager(
        store: &mut SqliteStore,
        enrollment: &EnrollmentAttempt,
        local_wrapping_key: &[u8; 32],
        fixed_role: u8,
    ) -> Result<Self, Error> {
        let ready = ReadyFamilySession::from_enrollment(store, enrollment)?;
        Self::prepare(
            store,
            &ready,
            enrollment.signing_seed(),
            local_wrapping_key,
            fixed_role,
        )
    }

    fn prepare(
        store: &mut SqliteStore,
        ready: &ReadyFamilySession,
        signing_seed: [u8; 32],
        local_wrapping_key: &[u8; 32],
        fixed_role: u8,
    ) -> Result<Self, Error> {
        let family = ready.family();
        let public = PublicHistorySession::resume(store, family)?;
        if public.cursor() != ready.observed_cursor() || public.head_hash() != ready.observed_head()
        {
            return Err(Error::Invalid("ready view is behind public authority"));
        }
        let invitation_id = random_v4()?;
        let transition_id = random_v4()?;
        let object_id = random_v4()?;
        let invitation_seed = random::<32>()?;
        let nonce = random::<24>()?;
        let (candidate_bytes, object_bytes) = build_for_chain(
            family,
            public.chain(),
            ready.current_key(),
            &signing_seed,
            invitation_id,
            transition_id,
            object_id,
            invitation_seed,
            fixed_role,
            nonce,
        )?;
        let secret = cbor::encode(&Value::Array(vec![
            Value::Integer(2),
            Value::Integer(fixed_role.into()),
            Value::Bytes(invitation_seed.to_vec()),
            Value::Bytes(signing_seed.to_vec()),
            Value::Integer(ready.active_epoch().into()),
            Value::Bytes(ready.current_key().bytes.to_vec()),
            Value::Bytes(nonce.to_vec()),
        ]))?;
        let secret_nonce = random::<24>()?;
        let secret_ciphertext = crypto::seal_with_nonce(
            local_wrapping_key,
            &secret_nonce,
            &secret_aad(family, invitation_id, transition_id, object_id)?,
            &secret,
        )?;
        store.save_later_invite_issue(&InviteIssueRow {
            family,
            invitation_id,
            transition_id,
            object_id,
            object_bytes,
            candidate_bytes,
            secret_nonce,
            secret_ciphertext,
        })?;
        Self::resume(store, family, invitation_id, local_wrapping_key)
    }

    pub fn resume(
        store: &SqliteStore,
        family: FamilyHandle,
        invitation_id: [u8; 16],
        local_wrapping_key: &[u8; 32],
    ) -> Result<Self, Error> {
        let row = store
            .invite_issue(family, invitation_id)?
            .ok_or(Error::Invalid("no durable invitation"))?;
        let secret = crypto::open(
            local_wrapping_key,
            &row.secret_nonce,
            &secret_aad(family, row.invitation_id, row.transition_id, row.object_id)?,
            &row.secret_ciphertext,
        )?;
        let Value::Array(parts) = cbor::decode_with_limits(
            &secret,
            cbor::Limits {
                max_bytes: 256,
                max_depth: 3,
            },
        )?
        else {
            return Err(Error::Invalid("invitation secret not array"));
        };
        if parts.len() != 7 || parts[0] != Value::Integer(2) {
            return Err(Error::Invalid("invitation secret version"));
        }
        let role: u8 = number(&parts[1])?
            .try_into()
            .map_err(|_| Error::Invalid("role range"))?;
        let invitation_seed = fixed::<32>(&parts[2])?;
        let signing_seed = fixed::<32>(&parts[3])?;
        let epoch: u32 = number(&parts[4])?
            .try_into()
            .map_err(|_| Error::Invalid("epoch range"))?;
        let key = VerifiedEpochKey {
            family_id: family.family_id,
            epoch,
            bytes: fixed::<32>(&parts[5])?,
        };
        let nonce = fixed::<24>(&parts[6])?;
        let Value::Map(candidate) = cbor::decode(&row.candidate_bytes)? else {
            return Err(Error::Invalid("invitation candidate not map"));
        };
        let Value::Map(unsigned) = &candidate[0].1 else {
            return Err(Error::Invalid("invitation unsigned not map"));
        };
        let prior_head = fixed::<32>(&unsigned[3].1)?;
        let chain = chain_at_head(store, family, prior_head)?;
        let (candidate_bytes, object_bytes) = build_for_chain(
            family,
            &chain,
            &key,
            &signing_seed,
            row.invitation_id,
            row.transition_id,
            row.object_id,
            invitation_seed,
            role,
            nonce,
        )?;
        if candidate_bytes != row.candidate_bytes || object_bytes != row.object_bytes {
            return Err(Error::Invalid("invitation differs from durable secrets"));
        }
        Ok(Self {
            family,
            invitation_id,
            transition_id: row.transition_id,
            object_id: row.object_id,
            candidate_bytes,
            object_bytes,
            invitation_seed,
            epoch_key: key,
        })
    }

    pub fn family(&self) -> FamilyHandle {
        self.family
    }
    pub fn invitation_id(&self) -> [u8; 16] {
        self.invitation_id
    }
    pub fn candidate_bytes(&self) -> &[u8] {
        &self.candidate_bytes
    }
    pub fn object_id(&self) -> [u8; 16] {
        self.object_id
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
        committed_issue: &[u8],
        relay_origin: &str,
    ) -> Result<InvitationBootstrap, Error> {
        let Value::Map(root) = cbor::decode(committed_issue)? else {
            return Err(Error::Invalid("committed issue not map"));
        };
        if root.len() != 4
            || cbor::encode(&Value::Map(vec![
                (1, root[0].1.clone()),
                (2, root[1].1.clone()),
            ]))? != self.candidate_bytes
        {
            return Err(Error::Invalid(
                "committed issue differs from saved candidate",
            ));
        }
        let mut public = PublicHistorySession::resume(store, self.family)?;
        let history = store
            .shared_history(self.family)?
            .ok_or(Error::Invalid("shared history absent"))?;
        if !history
            .entries
            .iter()
            .any(|entry| entry.kind == 1 && entry.committed_bytes == committed_issue)
        {
            public.accept_control(store, committed_issue)?;
        }
        public.accept_object(store, self.object_id, &self.object_bytes)?;
        public
            .chain()
            .membership_check(&self.transition_id)
            .ok_or(Error::Invalid("committed issue membership absent"))?
            .verify(&self.object_bytes, &self.epoch_key)
            .map_err(|_| Error::Invalid("committed issue membership did not open"))?;
        let history = store
            .shared_history(self.family)?
            .ok_or(Error::Invalid("shared history absent"))?;
        let issue_index = history
            .entries
            .iter()
            .position(|entry| entry.kind == 1 && entry.committed_bytes == committed_issue)
            .ok_or(Error::Invalid(
                "committed issue missing from pinned history",
            ))?;
        let prior_controls: Vec<&[u8]> = history
            .entries
            .iter()
            .take(issue_index)
            .filter(|entry| entry.kind == 1)
            .map(|entry| entry.committed_bytes.as_slice())
            .collect();
        Ok(InvitationBootstrap::from_committed_issue_with_controls(
            relay_origin,
            history.relay_public_key,
            &history.genesis_bytes,
            committed_issue,
            self.invitation_seed,
            &prior_controls,
        )?)
    }
}

fn chain_at_head(
    store: &SqliteStore,
    family: FamilyHandle,
    head: [u8; 32],
) -> Result<ControlChain, Error> {
    PublicHistorySession::resume(store, family)?;
    let history = store
        .shared_history(family)?
        .ok_or(Error::Invalid("shared history absent"))?;
    let mut chain = ControlChain::from_genesis(&history.genesis_bytes, history.relay_public_key)?;
    if chain.head_hash() == head {
        return Ok(chain);
    }
    for entry in history.entries {
        if entry.kind == 1 {
            chain.apply_control(&entry.committed_bytes)?;
            if chain.head_hash() == head {
                return Ok(chain);
            }
        } else {
            chain.apply_public_batch(&entry.committed_bytes, &entry.receipt_bytes)?;
        }
    }
    Err(Error::Invalid(
        "invitation prior head absent from verified history",
    ))
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
    let Value::Map(state_fields) = state else {
        return Err(Error::Invalid("auth state not map"));
    };
    let Value::Array(active) = &state_fields[4].1 else {
        return Err(Error::Invalid("active state not array"));
    };
    if active.len() != 1 || state_fields[6].1 != Value::Array(vec![]) {
        return Err(Error::Invalid("first invitation requires one manager"));
    }
    let key = chain.verify_initial_epoch_key(&creation.epoch_key())?;
    build_for_chain(
        family,
        &chain,
        &key,
        &creation.signing_seed(),
        invitation_id,
        transition_id,
        object_id,
        invitation_seed,
        role,
        nonce,
    )
}

#[allow(clippy::too_many_arguments)]
fn build_for_chain(
    family: FamilyHandle,
    chain: &ControlChain,
    key: &VerifiedEpochKey,
    signing_seed: &[u8; 32],
    invitation_id: [u8; 16],
    transition_id: [u8; 16],
    object_id: [u8; 16],
    invitation_seed: [u8; 32],
    role: u8,
    nonce: [u8; 24],
) -> Result<(Vec<u8>, Vec<u8>), Error> {
    if !matches!(role, 1 | 2)
        || chain.family_id() != family.family_id
        || key.family_id != family.family_id
        || key.epoch != chain.epoch()?
        || chain.active_signing_public(family.device_id)?
            != crypto::signing_public_key(signing_seed)
        || !chain
            .active_devices()?
            .iter()
            .any(|device| device.device_id == family.device_id && device.role == 2)
    {
        return Err(Error::Invalid("invitation issuer is not a ready manager"));
    }
    let state = cbor::decode(&chain.state_bytes()?)?;
    let Value::Map(mut state_fields) = state else {
        return Err(Error::Invalid("auth state not map"));
    };
    let invite_public = crypto::signing_public_key(&invitation_seed);
    let delta = Value::Map(vec![
        (1, Value::Bytes(invitation_id.to_vec())),
        (2, Value::Bytes(family.device_id.to_vec())),
        (3, Value::Bytes(invite_public.to_vec())),
        (4, Value::Integer(role.into())),
    ]);
    let Value::Array(invitations) = &mut state_fields[6].1 else {
        return Err(Error::Invalid("invitations not array"));
    };
    invitations.push(Value::Array(vec![
        Value::Bytes(invitation_id.to_vec()),
        Value::Bytes(family.device_id.to_vec()),
        Value::Bytes(invite_public.to_vec()),
        Value::Integer(role.into()),
        Value::Bytes(transition_id.to_vec()),
        Value::Integer(1),
    ]));
    invitations.sort_by(|left, right| {
        let Value::Array(left) = left else {
            unreachable!()
        };
        let Value::Array(right) = right else {
            unreachable!()
        };
        let Value::Bytes(left) = &left[0] else {
            unreachable!()
        };
        let Value::Bytes(right) = &right[0] else {
            unreachable!()
        };
        left.cmp(right)
    });
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
        Value::Integer(key.epoch.into()),
    ];
    let core_hash = crypto::hash(
        "transition-core",
        &cbor::encode(&Value::Array(core.clone()))?,
    )?;
    let membership = cbor::encode(&Value::Map(vec![
        (1, Value::Bytes(transition_id.to_vec())),
        (2, Value::Bytes(chain.head_hash().to_vec())),
        (3, Value::Bytes(state_hash.to_vec())),
        (4, Value::Integer(key.epoch.into())),
        (5, delta),
    ]))?;
    let aad = crypto::hash("membership-aad", &core_hash)?;
    let ciphertext = crypto::seal_with_nonce(&key.bytes, &nonce, &aad, &membership)?;
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
        signing_seed,
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
    babytrack_wire::authority::prepare_issue(
        &candidate,
        &cbor::decode(&chain.state_bytes()?)?,
        chain.head_hash(),
    )?;
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
