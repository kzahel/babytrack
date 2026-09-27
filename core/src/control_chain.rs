//! Incremental public authority replay against a pinned signed genesis.
//! Invitation issue is the first implemented transition; other kinds fail
//! closed until their state rules and object checks are implemented.

use std::collections::{BTreeMap, BTreeSet};

use crate::{
    batch,
    cbor::{self, Value},
    control::{self, Genesis, array, exact_map, fixed, number, signed_number},
    crypto,
    grant::{self, VerifiedAdmissionGrant, VerifiedRepairGrant},
    handoff::{self, VerifiedChallenge},
    membership::{self, VerifiedMembership},
    projection::VerifiedEpochKey,
    rotation::{self, ObjectRef, VerifiedRotation},
    session,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    Public(babytrack_wire::authority::Error),
    Control(control::Error),
    Cbor(cbor::Error),
    Crypto(crypto::Error),
    Batch(batch::Error),
    Handoff(handoff::Error),
    Grant(grant::Error),
    Membership(membership::Error),
    Rotation(rotation::Error),
    Invalid(&'static str),
    UnsupportedKind,
}
impl From<babytrack_wire::authority::Error> for Error {
    fn from(value: babytrack_wire::authority::Error) -> Self {
        Self::Public(value)
    }
}
impl From<control::Error> for Error {
    fn from(value: control::Error) -> Self {
        Self::Control(value)
    }
}
impl From<cbor::Error> for Error {
    fn from(value: cbor::Error) -> Self {
        Self::Cbor(value)
    }
}
impl From<crypto::Error> for Error {
    fn from(value: crypto::Error) -> Self {
        Self::Crypto(value)
    }
}
impl From<batch::Error> for Error {
    fn from(value: batch::Error) -> Self {
        Self::Batch(value)
    }
}
impl From<handoff::Error> for Error {
    fn from(value: handoff::Error) -> Self {
        Self::Handoff(value)
    }
}
impl From<grant::Error> for Error {
    fn from(value: grant::Error) -> Self {
        Self::Grant(value)
    }
}
impl From<membership::Error> for Error {
    fn from(value: membership::Error) -> Self {
        Self::Membership(value)
    }
}
impl From<rotation::Error> for Error {
    fn from(value: rotation::Error) -> Self {
        Self::Rotation(value)
    }
}

#[derive(Clone)]
pub struct ControlChain {
    genesis: Genesis,
    relay_public_key: [u8; 32],
    state: Value,
    head_hash: [u8; 32],
    last_global_cursor: u64,
    last_commit_ms: i64,
    issue_times: BTreeMap<[u8; 16], i64>,
    challenges: BTreeMap<[u8; 16], VerifiedChallenge>,
    seen_challenge_ids: BTreeSet<[u8; 16]>,
    seen_device_ids: BTreeSet<[u8; 16]>,
    seen_transition_ids: BTreeSet<[u8; 16]>,
    seen_object_ids: BTreeSet<[u8; 16]>,
    admissions: BTreeMap<[u8; 16], [u8; 16]>,
    admission_grants: BTreeMap<[u8; 16], VerifiedAdmissionGrant>,
    repair_grants: BTreeMap<[u8; 16], VerifiedRepairGrant>,
    known_heads: BTreeMap<[u8; 32], u32>,
    next_sequences: BTreeMap<[u8; 16], u64>,
    seen_all_ids: BTreeSet<[u8; 16]>,
    current_commitment: [u8; 32],
    memberships: BTreeMap<[u8; 16], VerifiedMembership>,
    epoch_commitments: BTreeMap<u32, [u8; 32]>,
    rotations: BTreeMap<[u8; 16], VerifiedRotation>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActiveDevice {
    pub device_id: [u8; 16],
    pub role: u8,
}

struct FinalizePlan<'a> {
    next_state: Value,
    expected_signers: &'a [([u8; 16], [u8; 32])],
    manifest_kinds: &'a [u64],
    before_ms: Option<i64>,
}

type NewControlIds = ([u8; 16], Vec<[u8; 16]>, Vec<[u8; 16]>);

impl ControlChain {
    pub fn from_genesis(bytes: &[u8], relay_public_key: [u8; 32]) -> Result<Self, Error> {
        let genesis = control::verify_genesis(bytes, &relay_public_key)?;
        let object = cbor::decode(bytes)?;
        let root = exact_map(&object, 4)?;
        let unsigned = exact_map(&root[0].1, 11)?;
        let delta = exact_map(&unsigned[6].1, 3)?;
        let state = Value::Map(vec![
            (1, Value::Integer(1)),
            (2, Value::Bytes(genesis.family_id().to_vec())),
            (3, Value::Bytes(genesis.relay_id().to_vec())),
            (4, Value::Integer(1)),
            (5, Value::Array(vec![delta[0].1.clone()])),
            (6, Value::Array(vec![])),
            (7, Value::Array(vec![])),
        ]);
        let genesis_head = genesis.head_hash();
        let genesis_commitment = genesis.epoch_key_commitment();
        let manager_device_id = genesis.manager_device_id();
        let genesis_transition_id = genesis.transition_id();
        let Value::Array(genesis_manifest) = &unsigned[9].1 else {
            return Err(Error::Invalid("genesis manifest not array"));
        };
        let mut genesis_objects = BTreeSet::new();
        for entry in genesis_manifest {
            if !genesis_objects.insert(fixed::<16>(&array(entry, 4)?[1])?) {
                return Err(Error::Invalid("genesis object ID reused"));
            }
        }
        let mut seen_all_ids = BTreeSet::from([manager_device_id, genesis_transition_id]);
        if seen_all_ids.len() != 2 || !genesis_objects.iter().all(|id| seen_all_ids.insert(*id)) {
            return Err(Error::Invalid("genesis ID reused across categories"));
        }
        Ok(Self {
            head_hash: genesis_head,
            last_global_cursor: 1,
            last_commit_ms: genesis.committed_ms(),
            genesis,
            relay_public_key,
            state,
            issue_times: BTreeMap::new(),
            challenges: BTreeMap::new(),
            seen_challenge_ids: BTreeSet::new(),
            seen_device_ids: BTreeSet::from([manager_device_id]),
            seen_transition_ids: BTreeSet::from([genesis_transition_id]),
            seen_object_ids: genesis_objects,
            admissions: BTreeMap::new(),
            admission_grants: BTreeMap::new(),
            repair_grants: BTreeMap::new(),
            known_heads: BTreeMap::from([(genesis_head, 1)]),
            next_sequences: BTreeMap::new(),
            seen_all_ids,
            current_commitment: genesis_commitment,
            memberships: BTreeMap::new(),
            epoch_commitments: BTreeMap::from([(1, genesis_commitment)]),
            rotations: BTreeMap::new(),
        })
    }

    pub fn head_hash(&self) -> [u8; 32] {
        self.head_hash
    }
    pub fn family_id(&self) -> [u8; 16] {
        self.genesis.family_id()
    }
    pub fn relay_id(&self) -> [u8; 32] {
        self.genesis.relay_id()
    }
    pub fn epoch(&self) -> Result<u32, Error> {
        self.current_epoch()?
            .try_into()
            .map_err(|_| Error::Invalid("epoch outside u32"))
    }
    pub fn active_signing_public(&self, device_id: [u8; 16]) -> Result<[u8; 32], Error> {
        let state = exact_map(&self.state, 7)?;
        let row = find_row(&state[4].1, 5, 0, device_id)?
            .ok_or(Error::Invalid("device is not active"))?;
        Ok(fixed::<32>(&row[1])?)
    }
    /// Public, verified device grants. IDs identify credentials, not people.
    pub fn active_devices(&self) -> Result<Vec<ActiveDevice>, Error> {
        let state = exact_map(&self.state, 7)?;
        let Value::Array(rows) = &state[4].1 else {
            return Err(Error::Invalid("active devices not array"));
        };
        rows.iter()
            .map(|row| {
                let fields = array(row, 5)?;
                Ok(ActiveDevice {
                    device_id: fixed::<16>(&fields[0])?,
                    role: number(&fields[4])?
                        .try_into()
                        .map_err(|_| Error::Invalid("device role outside u8"))?,
                })
            })
            .collect()
    }
    pub fn next_sequence_for(&self, device_id: [u8; 16]) -> Result<u64, Error> {
        let _ = self.active_signing_public(device_id)?;
        Ok(self.next_sequences.get(&device_id).copied().unwrap_or(1))
    }
    pub fn last_global_cursor(&self) -> u64 {
        self.last_global_cursor
    }
    pub fn state_bytes(&self) -> Result<Vec<u8>, Error> {
        Ok(cbor::encode(&self.state)?)
    }

    /// Dispatch one committed control object by its signed kind. Unknown or
    /// not-yet-implemented kinds fail closed and leave the cursor unchanged.
    pub fn apply_control(&mut self, bytes: &[u8]) -> Result<(), Error> {
        let object = cbor::decode_with_limits(
            bytes,
            cbor::Limits {
                max_bytes: 1024 * 1024,
                max_depth: 16,
            },
        )?;
        let root = exact_map(&object, 4)?;
        let unsigned = exact_map(&root[0].1, 11)?;
        match number(&unsigned[5].1)? {
            2 => self.apply_invite_issue(bytes),
            3 => self.apply_invite_cancel(bytes),
            4 => self.apply_invite_claim(bytes),
            5 => self.apply_key_proof(bytes),
            6 => self.apply_admit_grant(bytes),
            7 => self.apply_role_change(bytes),
            8 => self.apply_remove_active(bytes),
            9 => self.apply_remove_pending(bytes),
            10 => self.apply_grant_repair(bytes),
            11 => self.apply_holder_challenge(bytes),
            _ => Err(Error::UnsupportedKind),
        }
    }

    /// Pending enrollment may see signed controls while intervening data
    /// entries remain unreadable. This verifies the control's relay-signed
    /// cursor and full authority transition, but never establishes data
    /// readiness or authorizes batch writes. Complete history is replayed
    /// separately after admission.
    pub(crate) fn apply_sparse_control(&mut self, bytes: &[u8]) -> Result<(), Error> {
        let value = cbor::decode_with_limits(
            bytes,
            cbor::Limits {
                max_bytes: 1024 * 1024,
                max_depth: 16,
            },
        )?;
        let root = exact_map(&value, 4)?;
        let receipt = array(&root[2].1, 6)?;
        let cursor = number(&receipt[3])?;
        if cursor <= self.last_global_cursor {
            return Err(Error::Invalid("sparse control cursor did not advance"));
        }
        let mut candidate = self.clone();
        candidate.last_global_cursor = cursor - 1;
        candidate.apply_control(bytes)?;
        *self = candidate;
        Ok(())
    }

    pub fn latest_challenge(&self, invitation_id: &[u8; 16]) -> Option<&VerifiedChallenge> {
        self.challenges.get(invitation_id)
    }

    pub fn initial_admission_grant(&self, device_id: &[u8; 16]) -> Option<&VerifiedAdmissionGrant> {
        self.admission_grants.get(device_id)
    }

    pub fn membership_check(&self, transition_id: &[u8; 16]) -> Option<&VerifiedMembership> {
        self.memberships.get(transition_id)
    }

    pub fn latest_repair_grant(&self, device_id: &[u8; 16]) -> Option<&VerifiedRepairGrant> {
        self.repair_grants.get(device_id)
    }

    pub fn rotation(&self, transition_id: &[u8; 16]) -> Option<&VerifiedRotation> {
        self.rotations.get(transition_id)
    }

    /// Only the genesis manager may verify the initial key by commitment.
    /// Later epochs require the complete committed grant, keyring, and
    /// encrypted membership objects before a usable key token is issued.
    pub fn verify_initial_epoch_key(&self, key: &[u8; 32]) -> Result<VerifiedEpochKey, Error> {
        if self.current_epoch()? != 1 {
            return Err(Error::Invalid(
                "rotated epoch requires complete grant and keyring verification",
            ));
        }
        let epoch = u32::try_from(self.current_epoch()?)
            .map_err(|_| Error::Invalid("epoch outside u32"))?;
        let bytes = cbor::encode(&Value::Array(vec![
            Value::Bytes(self.genesis.family_id().to_vec()),
            Value::Integer(epoch.into()),
            Value::Bytes(key.to_vec()),
        ]))?;
        if crypto::hash("epoch-key", &bytes)? != self.current_commitment {
            return Err(Error::Invalid(
                "key differs from committed epoch commitment",
            ));
        }
        Ok(VerifiedEpochKey {
            family_id: self.genesis.family_id(),
            epoch,
            bytes: *key,
        })
    }

    /// Open a committed rotation only after every active recipient grant,
    /// the complete history keyring, and its encrypted membership agree.
    pub fn open_rotation_for(
        &self,
        transition_id: &[u8; 16],
        device_id: [u8; 16],
        agreement_private: &[u8; 32],
        grant_objects: &[([u8; 16], Vec<u8>)],
        keyring_object: &[u8],
        membership_object: &[u8],
    ) -> Result<rotation::VerifiedRotationKeys, Error> {
        let rotation = self
            .rotations
            .get(transition_id)
            .ok_or(Error::Invalid("no committed rotation"))?;
        if rotation.epoch as u64 != self.current_epoch()? {
            return Err(Error::Invalid("rotation is not the current epoch"));
        }
        let keys =
            rotation.open_for(device_id, agreement_private, grant_objects, keyring_object)?;
        self.memberships
            .get(transition_id)
            .ok_or(Error::Invalid("rotation membership is missing"))?
            .verify(membership_object, keys.current())?;
        Ok(keys)
    }

    pub fn verify_latest_holder_proof(
        &self,
        invitation_id: &[u8; 16],
        verifier_object: &[u8],
        key: &VerifiedEpochKey,
        proof_signature: &[u8; 64],
    ) -> Result<(), Error> {
        let challenge = self
            .challenges
            .get(invitation_id)
            .ok_or(Error::Invalid("no latest challenge"))?;
        let state = exact_map(&self.state, 7)?;
        let pending = find_row(&state[5].1, 9, 0, *invitation_id)?
            .ok_or(Error::Invalid("proof no longer pending"))?;
        let committed_hash = fixed::<32>(&pending[8])?;
        challenge.verify_holder_proof(verifier_object, key, proof_signature, &committed_hash)?;
        Ok(())
    }

    pub fn apply_invite_issue(&mut self, bytes: &[u8]) -> Result<(), Error> {
        let object = cbor::decode_with_limits(
            bytes,
            cbor::Limits {
                max_bytes: 1024 * 1024,
                max_depth: 16,
            },
        )?;
        let root = exact_map(&object, 4)?;
        let unsigned = exact_map(&root[0].1, 11)?;
        let (new_transition_id, new_object_ids, all_ids) = self.check_new_control_ids(unsigned)?;
        let signed_candidate = cbor::encode(&Value::Map(vec![
            (1, root[0].1.clone()),
            (2, root[1].1.clone()),
        ]))?;
        let prepared = babytrack_wire::authority::prepare_issue(
            &signed_candidate,
            &self.state,
            self.head_hash,
        )?;
        let transition_id = prepared.transition_id;
        let invitation_id = prepared.invitation_id;
        let next_state = prepared.next_state;
        let membership_check = membership_from_unsigned(self.genesis.family_id(), unsigned)?
            .ok_or(Error::Invalid("invite issue missing membership object"))?;
        let receipt = array(&root[2].1, 6)?;
        let expected_cursor = self
            .last_global_cursor
            .checked_add(1)
            .ok_or(Error::Invalid("cursor overflow"))?;
        let committed_ms = signed_number(&receipt[4])?;
        if fixed::<16>(&receipt[0])? != self.genesis.family_id()
            || fixed::<32>(&receipt[1])? != self.genesis.relay_id()
            || fixed::<16>(&receipt[2])? != transition_id
            || number(&receipt[3])? != expected_cursor
            || committed_ms < self.last_commit_ms
        {
            return Err(Error::Invalid(
                "invite issue receipt context, cursor, or time invalid",
            ));
        }
        let signed = Value::Array(vec![root[0].1.clone(), root[1].1.clone()]);
        if fixed::<32>(&receipt[5])? != crypto::hash("control-signed", &cbor::encode(&signed)?)? {
            return Err(Error::Invalid("invite issue signed hash mismatch"));
        }
        crypto::verify_cbor(
            "control-receipt",
            &cbor::encode(&root[2].1)?,
            &self.relay_public_key,
            &fixed::<64>(&root[3].1)?,
        )?;
        let next_head = crypto::hash("control-head", bytes)?;
        let epoch = u32::try_from(number(&exact_map(&next_state, 7)?[3].1)?)
            .map_err(|_| Error::Invalid("epoch outside u32"))?;
        self.state = next_state;
        self.head_hash = next_head;
        self.last_global_cursor = expected_cursor;
        self.last_commit_ms = committed_ms;
        self.known_heads.insert(self.head_hash, epoch);
        self.memberships.insert(transition_id, membership_check);
        self.issue_times.insert(invitation_id, committed_ms);
        self.remember_control_ids(new_transition_id, new_object_ids, all_ids);
        Ok(())
    }

    pub fn apply_invite_cancel(&mut self, bytes: &[u8]) -> Result<(), Error> {
        let object = cbor::decode_with_limits(
            bytes,
            cbor::Limits {
                max_bytes: 1024 * 1024,
                max_depth: 16,
            },
        )?;
        let root = exact_map(&object, 4)?;
        let unsigned = exact_map(&root[0].1, 11)?;
        self.check_unsigned(unsigned, 3, self.current_epoch()?)?;
        let delta = exact_map(&unsigned[6].1, 1)?;
        let invitation_id = fixed::<16>(&delta[0].1)?;
        let state = exact_map(&self.state, 7)?;
        let invitation = find_row(&state[6].1, 6, 0, invitation_id)?
            .ok_or(Error::Invalid("invitation to cancel is absent"))?;
        if number(&invitation[5])? != 1 {
            return Err(Error::Invalid("only an unused invitation can be canceled"));
        }
        let signatures = array(&root[1].1, 1)?;
        let signer_id = fixed::<16>(&array(&signatures[0], 2)?[0])?;
        let signer = find_row(&state[4].1, 5, 0, signer_id)?
            .ok_or(Error::Invalid("cancel signer not active"))?;
        if number(&signer[4])? != 2 {
            return Err(Error::Invalid("cancel signer not manager"));
        }
        let signer_key = fixed::<32>(&signer[1])?;
        let mut next_state = self.state.clone();
        let Value::Map(map) = &mut next_state else {
            unreachable!()
        };
        let Value::Array(invitations) = &mut map[6].1 else {
            unreachable!()
        };
        for row in invitations {
            let Value::Array(fields) = row else {
                unreachable!()
            };
            if fixed::<16>(&fields[0])? == invitation_id {
                fields[5] = Value::Integer(3);
            }
        }
        self.finalize(
            bytes,
            root,
            unsigned,
            FinalizePlan {
                next_state,
                expected_signers: &[(signer_id, signer_key)],
                manifest_kinds: &[1],
                before_ms: None,
            },
        )
    }

    pub fn apply_role_change(&mut self, bytes: &[u8]) -> Result<(), Error> {
        let object = cbor::decode_with_limits(
            bytes,
            cbor::Limits {
                max_bytes: 1024 * 1024,
                max_depth: 16,
            },
        )?;
        let root = exact_map(&object, 4)?;
        let unsigned = exact_map(&root[0].1, 11)?;
        self.check_unsigned(unsigned, 7, self.current_epoch()?)?;
        let delta = exact_map(&unsigned[6].1, 3)?;
        let device_id = fixed::<16>(&delta[0].1)?;
        let old_role = number(&delta[1].1)?;
        let new_role = number(&delta[2].1)?;
        if !matches!((old_role, new_role), (1, 2) | (2, 1)) {
            return Err(Error::Invalid("role change must switch member and manager"));
        }
        let state = exact_map(&self.state, 7)?;
        let target = find_row(&state[4].1, 5, 0, device_id)?
            .ok_or(Error::Invalid("role target not active"))?;
        if number(&target[4])? != old_role {
            return Err(Error::Invalid("role target's prior role differs"));
        }
        let signatures = array(&root[1].1, 1)?;
        let signer_id = fixed::<16>(&array(&signatures[0], 2)?[0])?;
        let signer = find_row(&state[4].1, 5, 0, signer_id)?
            .ok_or(Error::Invalid("role signer not active"))?;
        if number(&signer[4])? != 2 {
            return Err(Error::Invalid("role signer not manager"));
        }
        let signer_key = fixed::<32>(&signer[1])?;
        let mut next_state = self.state.clone();
        let Value::Map(map) = &mut next_state else {
            unreachable!()
        };
        let Value::Array(active) = &mut map[4].1 else {
            unreachable!()
        };
        for row in active.iter_mut() {
            let Value::Array(fields) = row else {
                unreachable!()
            };
            if fixed::<16>(&fields[0])? == device_id {
                fields[4] = Value::Integer(new_role.into());
            }
        }
        if !active
            .iter()
            .any(|row| array(row, 5).is_ok_and(|fields| number(&fields[4]).ok() == Some(2)))
        {
            return Err(Error::Invalid("role change would remove the last manager"));
        }
        if old_role == 2 {
            let Value::Array(invitations) = &mut map[6].1 else {
                unreachable!()
            };
            for row in invitations {
                let Value::Array(fields) = row else {
                    unreachable!()
                };
                if fixed::<16>(&fields[1])? == device_id && number(&fields[5])? == 1 {
                    fields[5] = Value::Integer(3);
                }
            }
        }
        self.finalize(
            bytes,
            root,
            unsigned,
            FinalizePlan {
                next_state,
                expected_signers: &[(signer_id, signer_key)],
                manifest_kinds: &[1],
                before_ms: None,
            },
        )
    }

    pub fn apply_remove_pending(&mut self, bytes: &[u8]) -> Result<(), Error> {
        let object = cbor::decode_with_limits(
            bytes,
            cbor::Limits {
                max_bytes: 1024 * 1024,
                max_depth: 16,
            },
        )?;
        let root = exact_map(&object, 4)?;
        let unsigned = exact_map(&root[0].1, 11)?;
        self.check_unsigned(unsigned, 9, self.current_epoch()?)?;
        let delta = exact_map(&unsigned[6].1, 2)?;
        let invitation_id = fixed::<16>(&delta[0].1)?;
        let device_id = fixed::<16>(&delta[1].1)?;
        let state = exact_map(&self.state, 7)?;
        let pending = find_row(&state[5].1, 9, 0, invitation_id)?
            .ok_or(Error::Invalid("pending invitation to remove is absent"))?;
        if fixed::<16>(&pending[1])? != device_id {
            return Err(Error::Invalid("pending removal device mismatch"));
        }
        let signatures = array(&root[1].1, 1)?;
        let signer_id = fixed::<16>(&array(&signatures[0], 2)?[0])?;
        let signer = find_row(&state[4].1, 5, 0, signer_id)?
            .ok_or(Error::Invalid("pending removal signer not active"))?;
        if number(&signer[4])? != 2 {
            return Err(Error::Invalid("pending removal signer not manager"));
        }
        let signer_key = fixed::<32>(&signer[1])?;
        let mut next_state = self.state.clone();
        let Value::Map(map) = &mut next_state else {
            unreachable!()
        };
        let Value::Array(pending_rows) = &mut map[5].1 else {
            unreachable!()
        };
        pending_rows.retain(|row| {
            !array(row, 9).is_ok_and(|fields| fixed::<16>(&fields[0]).ok() == Some(invitation_id))
        });
        self.finalize(
            bytes,
            root,
            unsigned,
            FinalizePlan {
                next_state,
                expected_signers: &[(signer_id, signer_key)],
                manifest_kinds: &[1],
                before_ms: None,
            },
        )?;
        self.challenges.remove(&invitation_id);
        Ok(())
    }

    pub fn apply_invite_claim(&mut self, bytes: &[u8]) -> Result<(), Error> {
        let object = cbor::decode_with_limits(
            bytes,
            cbor::Limits {
                max_bytes: 1024 * 1024,
                max_depth: 16,
            },
        )?;
        let root = exact_map(&object, 4)?;
        let unsigned = exact_map(&root[0].1, 11)?;
        let (new_transition_id, new_object_ids, all_ids) = self.check_new_control_ids(unsigned)?;
        let signed_candidate = cbor::encode(&Value::Map(vec![
            (1, root[0].1.clone()),
            (2, root[1].1.clone()),
        ]))?;
        let prepared = babytrack_wire::authority::prepare_claim(
            &signed_candidate,
            &self.state,
            self.head_hash,
        )?;
        if self.seen_device_ids.contains(&prepared.device_id) {
            return Err(Error::Invalid("claim reuses a historical device ID"));
        }
        let transition_id = prepared.transition_id;
        let invitation_id = prepared.invitation_id;
        let device_id = prepared.device_id;
        let next_state = prepared.next_state;
        let receipt = array(&root[2].1, 6)?;
        let expected_cursor = self
            .last_global_cursor
            .checked_add(1)
            .ok_or(Error::Invalid("cursor overflow"))?;
        let committed_ms = signed_number(&receipt[4])?;
        let expiry = self
            .issue_times
            .get(&invitation_id)
            .ok_or(Error::Invalid("missing verified issue time"))?
            .checked_add(604_800_000)
            .ok_or(Error::Invalid("invite expiry overflow"))?;
        if fixed::<16>(&receipt[0])? != self.genesis.family_id()
            || fixed::<32>(&receipt[1])? != self.genesis.relay_id()
            || fixed::<16>(&receipt[2])? != transition_id
            || number(&receipt[3])? != expected_cursor
            || committed_ms < self.last_commit_ms
            || committed_ms >= expiry
        {
            return Err(Error::Invalid(
                "claim receipt context, cursor, time, or expiry invalid",
            ));
        }
        let signed = Value::Array(vec![root[0].1.clone(), root[1].1.clone()]);
        if fixed::<32>(&receipt[5])? != crypto::hash("control-signed", &cbor::encode(&signed)?)? {
            return Err(Error::Invalid("claim signed hash mismatch"));
        }
        crypto::verify_cbor(
            "control-receipt",
            &cbor::encode(&root[2].1)?,
            &self.relay_public_key,
            &fixed::<64>(&root[3].1)?,
        )?;
        let next_head = crypto::hash("control-head", bytes)?;
        let epoch = u32::try_from(number(&exact_map(&next_state, 7)?[3].1)?)
            .map_err(|_| Error::Invalid("epoch outside u32"))?;
        self.state = next_state;
        self.head_hash = next_head;
        self.last_global_cursor = expected_cursor;
        self.last_commit_ms = committed_ms;
        self.known_heads.insert(self.head_hash, epoch);
        self.seen_device_ids.insert(device_id);
        self.remember_control_ids(new_transition_id, new_object_ids, all_ids);
        Ok(())
    }

    pub fn apply_holder_challenge(&mut self, bytes: &[u8]) -> Result<(), Error> {
        let object = cbor::decode_with_limits(
            bytes,
            cbor::Limits {
                max_bytes: 1024 * 1024,
                max_depth: 16,
            },
        )?;
        let root = exact_map(&object, 4)?;
        let unsigned = exact_map(&root[0].1, 11)?;
        self.check_unsigned(unsigned, 11, self.current_epoch()?)?;
        let delta = exact_map(&unsigned[6].1, 4)?;
        let invitation_id = fixed::<16>(&delta[0].1)?;
        let device_id = fixed::<16>(&delta[1].1)?;
        let challenge_id = fixed::<16>(&delta[2].1)?;
        let challenge_hash = fixed::<32>(&delta[3].1)?;
        if self.seen_challenge_ids.contains(&challenge_id) {
            return Err(Error::Invalid("challenge ID reused"));
        }
        let state = exact_map(&self.state, 7)?;
        let pending = find_row(&state[5].1, 9, 0, invitation_id)?
            .ok_or(Error::Invalid("challenge invitation not pending"))?;
        if fixed::<16>(&pending[1])? != device_id {
            return Err(Error::Invalid("challenge device mismatch"));
        }
        let claim_hash = fixed::<32>(&pending[6])?;
        let agree_public = fixed::<32>(&pending[3])?;
        let sign_public = fixed::<32>(&pending[2])?;
        let key_version: u32 = number(&pending[4])?
            .try_into()
            .map_err(|_| Error::Invalid("key version outside u32"))?;
        let epoch: u32 = self
            .current_epoch()?
            .try_into()
            .map_err(|_| Error::Invalid("epoch outside u32"))?;
        let context_bytes = cbor::encode(&Value::Array(vec![
            Value::Bytes(self.genesis.family_id().to_vec()),
            Value::Bytes(self.genesis.relay_id().to_vec()),
            Value::Bytes(invitation_id.to_vec()),
            Value::Bytes(device_id.to_vec()),
            Value::Bytes(claim_hash.to_vec()),
            Value::Bytes(challenge_id.to_vec()),
            Value::Bytes(agree_public.to_vec()),
            Value::Integer(key_version.into()),
            Value::Bytes(self.head_hash.to_vec()),
        ]))?;
        let manifest = array(&unsigned[9].1, 2)?;
        let hpke_entry = array(&manifest[0], 4)?;
        let verifier_entry = array(&manifest[1], 4)?;
        if number(&hpke_entry[0])? != 2 || number(&verifier_entry[0])? != 3 {
            return Err(Error::Invalid("challenge object manifest kinds invalid"));
        }
        let challenge_record = VerifiedChallenge {
            family_id: self.genesis.family_id(),
            epoch,
            device_id,
            challenge_id,
            context_bytes,
            challenge_hash,
            hpke_object_id: fixed::<16>(&hpke_entry[1])?,
            hpke_object_hash: fixed::<32>(&hpke_entry[2])?,
            verifier_object_id: fixed::<16>(&verifier_entry[1])?,
            verifier_object_hash: fixed::<32>(&verifier_entry[2])?,
            pending_sign_public: sign_public,
            pending_agree_public: agree_public,
            pending_key_version: key_version,
        };
        let signatures = array(&root[1].1, 1)?;
        let signer_id = fixed::<16>(&array(&signatures[0], 2)?[0])?;
        let active = find_row(&state[4].1, 5, 0, signer_id)?
            .ok_or(Error::Invalid("challenge signer not active"))?;
        let signer_key = fixed::<32>(&active[1])?;
        let mut next_state = self.state.clone();
        let Value::Map(map) = &mut next_state else {
            unreachable!()
        };
        let Value::Array(rows) = &mut map[5].1 else {
            unreachable!()
        };
        let row = rows
            .iter_mut()
            .find(|row| {
                array(row, 9)
                    .is_ok_and(|fields| fixed::<16>(&fields[0]).ok() == Some(invitation_id))
            })
            .unwrap();
        let Value::Array(row) = row else {
            unreachable!()
        };
        row[7] = Value::Bytes(challenge_id.to_vec());
        row[8] = Value::Null;
        self.finalize(
            bytes,
            root,
            unsigned,
            FinalizePlan {
                next_state,
                expected_signers: &[(signer_id, signer_key)],
                manifest_kinds: &[2, 3],
                before_ms: None,
            },
        )?;
        self.challenges.insert(invitation_id, challenge_record);
        self.seen_challenge_ids.insert(challenge_id);
        Ok(())
    }

    pub fn apply_key_proof(&mut self, bytes: &[u8]) -> Result<(), Error> {
        let object = cbor::decode_with_limits(
            bytes,
            cbor::Limits {
                max_bytes: 1024 * 1024,
                max_depth: 16,
            },
        )?;
        let root = exact_map(&object, 4)?;
        let unsigned = exact_map(&root[0].1, 11)?;
        self.check_unsigned(unsigned, 5, self.current_epoch()?)?;
        let delta = exact_map(&unsigned[6].1, 4)?;
        let invitation_id = fixed::<16>(&delta[0].1)?;
        let device_id = fixed::<16>(&delta[1].1)?;
        let challenge_hash = fixed::<32>(&delta[2].1)?;
        let proof_signature = fixed::<64>(&delta[3].1)?;
        let state = exact_map(&self.state, 7)?;
        let pending = find_row(&state[5].1, 9, 0, invitation_id)?
            .ok_or(Error::Invalid("proof invitation not pending"))?;
        let challenge = self
            .challenges
            .get(&invitation_id)
            .ok_or(Error::Invalid("proof has no verified challenge transition"))?;
        if fixed::<16>(&pending[1])? != device_id
            || challenge.device_id != device_id
            || pending[7] != Value::Bytes(challenge.challenge_id.to_vec())
            || challenge.challenge_hash != challenge_hash
        {
            return Err(Error::Invalid(
                "proof does not match latest pending challenge",
            ));
        }
        let signer_key = fixed::<32>(&pending[2])?;
        let proof = Value::Array(vec![
            Value::Bytes(challenge.challenge_id.to_vec()),
            Value::Bytes(proof_signature.to_vec()),
        ]);
        let proof_hash = crypto::hash("proof", &cbor::encode(&proof)?)?;
        let mut next_state = self.state.clone();
        let Value::Map(map) = &mut next_state else {
            unreachable!()
        };
        let Value::Array(rows) = &mut map[5].1 else {
            unreachable!()
        };
        let row = rows
            .iter_mut()
            .find(|row| {
                array(row, 9)
                    .is_ok_and(|fields| fixed::<16>(&fields[0]).ok() == Some(invitation_id))
            })
            .unwrap();
        let Value::Array(row) = row else {
            unreachable!()
        };
        row[8] = Value::Bytes(proof_hash.to_vec());
        self.finalize(
            bytes,
            root,
            unsigned,
            FinalizePlan {
                next_state,
                expected_signers: &[(device_id, signer_key)],
                manifest_kinds: &[],
                before_ms: None,
            },
        )?;
        Ok(())
    }

    pub fn apply_admit_grant(&mut self, bytes: &[u8]) -> Result<(), Error> {
        let object = cbor::decode_with_limits(
            bytes,
            cbor::Limits {
                max_bytes: 1024 * 1024,
                max_depth: 16,
            },
        )?;
        let root = exact_map(&object, 4)?;
        let unsigned = exact_map(&root[0].1, 11)?;
        self.check_unsigned(unsigned, 6, self.current_epoch()?)?;
        let transition_id = fixed::<16>(&unsigned[4].1)?;
        let delta = exact_map(&unsigned[6].1, 4)?;
        let invitation_id = fixed::<16>(&delta[0].1)?;
        let device_id = fixed::<16>(&delta[1].1)?;
        let role = number(&delta[2].1)?;
        let commitment = fixed::<32>(&delta[3].1)?;
        if commitment != self.current_commitment {
            return Err(Error::Invalid(
                "admission key commitment differs from active epoch",
            ));
        }
        let state = exact_map(&self.state, 7)?;
        let pending = find_row(&state[5].1, 9, 0, invitation_id)?
            .ok_or(Error::Invalid("admission invitation not pending"))?;
        if fixed::<16>(&pending[1])? != device_id
            || number(&pending[5])? != role
            || !matches!(pending[8], Value::Bytes(ref bytes) if bytes.len() == 32)
        {
            return Err(Error::Invalid(
                "admission needs the fixed role and committed proof",
            ));
        }
        let sign_public = fixed::<32>(&pending[2])?;
        let agree_public = fixed::<32>(&pending[3])?;
        let key_version = number(&pending[4])?;
        let manifest = array(&unsigned[9].1, 2)?;
        let grant_entry = array(&manifest[1], 4)?;
        if number(&grant_entry[0])? != 4 {
            return Err(Error::Invalid("admission grant manifest kind invalid"));
        }
        let grant = VerifiedAdmissionGrant {
            family_id: self.genesis.family_id(),
            epoch: self
                .current_epoch()?
                .try_into()
                .map_err(|_| Error::Invalid("epoch outside u32"))?,
            epoch_commitment: self.current_commitment,
            core_hash: fixed::<32>(&unsigned[10].1)?,
            invitation_id,
            device_id,
            role: role
                .try_into()
                .map_err(|_| Error::Invalid("admission role outside u8"))?,
            agree_public,
            key_version: key_version
                .try_into()
                .map_err(|_| Error::Invalid("key version outside u32"))?,
            grant_id: fixed::<16>(&grant_entry[1])?,
            object_hash: fixed::<32>(&grant_entry[2])?,
        };
        let signatures = array(&root[1].1, 1)?;
        let signer_id = fixed::<16>(&array(&signatures[0], 2)?[0])?;
        let active = find_row(&state[4].1, 5, 0, signer_id)?
            .ok_or(Error::Invalid("grant signer not active"))?;
        let signer_key = fixed::<32>(&active[1])?;
        if find_row(&state[4].1, 5, 0, device_id)?.is_some() {
            return Err(Error::Invalid("recipient already active"));
        }
        let mut next_state = self.state.clone();
        let Value::Map(map) = &mut next_state else {
            unreachable!()
        };
        let Value::Array(next_pending) = &mut map[5].1 else {
            unreachable!()
        };
        next_pending.retain(|row| {
            !array(row, 9).is_ok_and(|fields| fixed::<16>(&fields[0]).ok() == Some(invitation_id))
        });
        let Value::Array(next_active) = &mut map[4].1 else {
            unreachable!()
        };
        next_active.push(Value::Array(vec![
            Value::Bytes(device_id.to_vec()),
            Value::Bytes(sign_public.to_vec()),
            Value::Bytes(agree_public.to_vec()),
            Value::Integer(key_version.into()),
            Value::Integer(role.into()),
        ]));
        sort_rows_by_id(next_active);
        self.finalize(
            bytes,
            root,
            unsigned,
            FinalizePlan {
                next_state,
                expected_signers: &[(signer_id, signer_key)],
                manifest_kinds: &[1, 4],
                before_ms: None,
            },
        )?;
        self.admissions.insert(device_id, transition_id);
        self.admission_grants.insert(device_id, grant);
        Ok(())
    }

    pub fn apply_grant_repair(&mut self, bytes: &[u8]) -> Result<(), Error> {
        let object = cbor::decode_with_limits(
            bytes,
            cbor::Limits {
                max_bytes: 1024 * 1024,
                max_depth: 16,
            },
        )?;
        let root = exact_map(&object, 4)?;
        let unsigned = exact_map(&root[0].1, 11)?;
        self.check_unsigned(unsigned, 10, self.current_epoch()?)?;
        let delta = exact_map(&unsigned[6].1, 3)?;
        let device_id = fixed::<16>(&delta[0].1)?;
        let admission_id = fixed::<16>(&delta[1].1)?;
        let commitment = fixed::<32>(&delta[2].1)?;
        if self.admissions.get(&device_id) != Some(&admission_id)
            || commitment != self.current_commitment
        {
            return Err(Error::Invalid(
                "repair recipient, admission, or epoch commitment mismatch",
            ));
        }
        let state = exact_map(&self.state, 7)?;
        let recipient = find_row(&state[4].1, 5, 0, device_id)?
            .ok_or(Error::Invalid("repair recipient no longer active"))?;
        let agree_public = fixed::<32>(&recipient[2])?;
        let key_version: u32 = number(&recipient[3])?
            .try_into()
            .map_err(|_| Error::Invalid("key version outside u32"))?;
        let manifest = array(&unsigned[9].1, 2)?;
        let grant_entry = array(&manifest[1], 4)?;
        if number(&grant_entry[0])? != 4 {
            return Err(Error::Invalid("repair grant manifest kind invalid"));
        }
        let grant = VerifiedRepairGrant {
            family_id: self.genesis.family_id(),
            relay_id: self.genesis.relay_id(),
            epoch: self
                .current_epoch()?
                .try_into()
                .map_err(|_| Error::Invalid("epoch outside u32"))?,
            epoch_commitment: self.current_commitment,
            core_hash: fixed::<32>(&unsigned[10].1)?,
            prior_head: fixed::<32>(&unsigned[3].1)?,
            transition_id: fixed::<16>(&unsigned[4].1)?,
            admission_id,
            device_id,
            agree_public,
            key_version,
            grant_id: fixed::<16>(&grant_entry[1])?,
            object_hash: fixed::<32>(&grant_entry[2])?,
        };
        let signatures = array(&root[1].1, 1)?;
        let signer_id = fixed::<16>(&array(&signatures[0], 2)?[0])?;
        let active = find_row(&state[4].1, 5, 0, signer_id)?
            .ok_or(Error::Invalid("repair signer not active"))?;
        let signer_key = fixed::<32>(&active[1])?;
        self.finalize(
            bytes,
            root,
            unsigned,
            FinalizePlan {
                next_state: self.state.clone(),
                expected_signers: &[(signer_id, signer_key)],
                manifest_kinds: &[1, 4],
                before_ms: None,
            },
        )?;
        self.repair_grants.insert(device_id, grant);
        Ok(())
    }

    /// Verify only public batch authorization and its relay acceptance. Data
    /// decryption/projection is a separate core step; this is useful for
    /// following the global cursor before a later control transition.
    pub fn apply_public_batch(
        &mut self,
        envelope_bytes: &[u8],
        receipt_bytes: &[u8],
    ) -> Result<batch::SignedEnvelope, Error> {
        let value = cbor::decode_with_limits(
            envelope_bytes,
            cbor::Limits {
                max_bytes: 256 * 1024 + 2048,
                max_depth: 16,
            },
        )?;
        let envelope = exact_map(&value, 3)?;
        let header_bytes = cbor::encode(&envelope[0].1)?;
        let header = batch::Header::decode(&header_bytes)?;
        let state = exact_map(&self.state, 7)?;
        let active = find_row(&state[4].1, 5, 0, header.author_device_id)?
            .ok_or(Error::Invalid("batch author is not active"))?;
        let signer = fixed::<32>(&active[1])?;
        let signed = batch::verify_signed_envelope(
            envelope_bytes,
            &self.genesis.family_id(),
            &self.genesis.relay_id(),
            &signer,
        )?;
        let current_epoch: u32 = number(&state[3].1)?
            .try_into()
            .map_err(|_| Error::Invalid("epoch outside u32"))?;
        if header.epoch != current_epoch
            || self.known_heads.get(&header.control_head) != Some(&current_epoch)
        {
            return Err(Error::Invalid(
                "batch epoch or authoring head not current ancestry",
            ));
        }
        let expected_sequence = self
            .next_sequences
            .get(&header.author_device_id)
            .copied()
            .unwrap_or(1);
        if header.device_sequence != expected_sequence
            || self.seen_all_ids.contains(&header.batch_id)
        {
            return Err(Error::Invalid("batch sequence or ID reused"));
        }
        let receipt = session::verify_accepted_receipt(receipt_bytes, &self.relay_public_key)
            .map_err(|_| Error::Invalid("batch acceptance receipt invalid"))?;
        let expected_cursor = self
            .last_global_cursor
            .checked_add(1)
            .ok_or(Error::Invalid("cursor overflow"))?;
        if receipt.family_id != self.genesis.family_id()
            || receipt.relay_id != self.genesis.relay_id()
            || receipt.batch_id != header.batch_id
            || receipt.object_hash != signed.object_hash()
            || receipt.cursor != expected_cursor
            || receipt.control_head != header.control_head
            || receipt.device_sequence != expected_sequence
            || receipt.next_expected_sequence
                != expected_sequence
                    .checked_add(1)
                    .ok_or(Error::Invalid("sequence overflow"))?
        {
            return Err(Error::Invalid(
                "batch receipt does not match authorized cursor",
            ));
        }
        self.last_global_cursor = expected_cursor;
        self.next_sequences
            .insert(header.author_device_id, receipt.next_expected_sequence);
        self.seen_all_ids.insert(header.batch_id);
        Ok(signed)
    }

    pub fn apply_remove_active(&mut self, bytes: &[u8]) -> Result<(), Error> {
        let object = cbor::decode_with_limits(
            bytes,
            cbor::Limits {
                max_bytes: 1024 * 1024,
                max_depth: 16,
            },
        )?;
        let root = exact_map(&object, 4)?;
        let unsigned = exact_map(&root[0].1, 11)?;
        let next_epoch = self
            .current_epoch()?
            .checked_add(1)
            .ok_or(Error::Invalid("epoch overflow"))?;
        let _: u32 = next_epoch
            .try_into()
            .map_err(|_| Error::Invalid("epoch outside u32"))?;
        self.check_unsigned(unsigned, 8, next_epoch)?;
        let delta = exact_map(&unsigned[6].1, 3)?;
        let target_id = fixed::<16>(&delta[0].1)?;
        let old_role = number(&delta[1].1)?;
        let new_commitment = fixed::<32>(&delta[2].1)?;
        let state = exact_map(&self.state, 7)?;
        let target = find_row(&state[4].1, 5, 0, target_id)?
            .ok_or(Error::Invalid("removal target not active"))?;
        if number(&target[4])? != old_role {
            return Err(Error::Invalid("removal old role mismatch"));
        }
        let signatures = array(&root[1].1, 1)?;
        let signer_id = fixed::<16>(&array(&signatures[0], 2)?[0])?;
        let signer = find_row(&state[4].1, 5, 0, signer_id)?
            .ok_or(Error::Invalid("removal signer not active"))?;
        if number(&signer[4])? != 2 {
            return Err(Error::Invalid("removal signer not manager"));
        }
        let signer_key = fixed::<32>(&signer[1])?;
        let mut next_state = self.state.clone();
        let Value::Map(map) = &mut next_state else {
            unreachable!()
        };
        map[3].1 = Value::Integer(next_epoch.into());
        let Value::Array(active) = &mut map[4].1 else {
            unreachable!()
        };
        active.retain(|row| {
            !array(row, 5).is_ok_and(|fields| fixed::<16>(&fields[0]).ok() == Some(target_id))
        });
        if !active
            .iter()
            .any(|row| array(row, 5).is_ok_and(|fields| number(&fields[4]).ok() == Some(2)))
        {
            return Err(Error::Invalid("cannot remove final manager"));
        }
        let remaining = active.len();
        let Value::Array(invitations) = &mut map[6].1 else {
            unreachable!()
        };
        for invitation in invitations {
            let Value::Array(fields) = invitation else {
                unreachable!()
            };
            if fixed::<16>(&fields[1])? == target_id && number(&fields[5])? == 1 {
                fields[5] = Value::Integer(3);
            }
        }
        let Value::Array(pending) = &mut map[5].1 else {
            unreachable!()
        };
        for row in pending {
            let Value::Array(fields) = row else {
                unreachable!()
            };
            fields[7] = Value::Null;
            fields[8] = Value::Null;
        }
        let mut kinds = vec![1];
        kinds.extend(std::iter::repeat_n(4, remaining));
        kinds.push(5);
        let next_auth = exact_map(&next_state, 7)?;
        let Value::Array(active_rows) = &next_auth[4].1 else {
            unreachable!()
        };
        let mut recipients = BTreeMap::new();
        for row in active_rows {
            let fields = array(row, 5)?;
            recipients.insert(
                fixed::<16>(&fields[0])?,
                (
                    fixed::<32>(&fields[2])?,
                    number(&fields[3])?
                        .try_into()
                        .map_err(|_| Error::Invalid("agreement key version outside u32"))?,
                ),
            );
        }
        let manifest = array(&unsigned[9].1, remaining + 2)?;
        let mut grants = Vec::with_capacity(remaining);
        for entry in &manifest[1..=remaining] {
            let fields = array(entry, 4)?;
            if number(&fields[0])? != 4 {
                return Err(Error::Invalid("rotation grant manifest kind invalid"));
            }
            grants.push(ObjectRef {
                id: fixed::<16>(&fields[1])?,
                hash: fixed::<32>(&fields[2])?,
            });
        }
        let keyring_entry = array(&manifest[remaining + 1], 4)?;
        if number(&keyring_entry[0])? != 5 {
            return Err(Error::Invalid("rotation keyring manifest kind invalid"));
        }
        let rotation = VerifiedRotation {
            family_id: self.genesis.family_id(),
            relay_id: self.genesis.relay_id(),
            prior_head: fixed::<32>(&unsigned[3].1)?,
            transition_id: fixed::<16>(&unsigned[4].1)?,
            state_hash: fixed::<32>(&unsigned[7].1)?,
            epoch: next_epoch
                .try_into()
                .map_err(|_| Error::Invalid("epoch outside u32"))?,
            commitment: new_commitment,
            core_hash: fixed::<32>(&unsigned[10].1)?,
            delta: unsigned[6].1.clone(),
            grants,
            keyring: ObjectRef {
                id: fixed::<16>(&keyring_entry[1])?,
                hash: fixed::<32>(&keyring_entry[2])?,
            },
            recipients,
            prior_commitments: self.epoch_commitments.clone(),
        };
        self.finalize(
            bytes,
            root,
            unsigned,
            FinalizePlan {
                next_state,
                expected_signers: &[(signer_id, signer_key)],
                manifest_kinds: &kinds,
                before_ms: None,
            },
        )?;
        self.current_commitment = new_commitment;
        self.epoch_commitments
            .insert(rotation.epoch, new_commitment);
        self.rotations.insert(rotation.transition_id, rotation);
        self.challenges.clear();
        self.admission_grants.remove(&target_id);
        self.repair_grants.remove(&target_id);
        Ok(())
    }

    fn current_epoch(&self) -> Result<u64, Error> {
        Ok(number(&exact_map(&self.state, 7)?[3].1)?)
    }

    fn check_new_control_ids(&self, unsigned: &[(u64, Value)]) -> Result<NewControlIds, Error> {
        let transition_id = fixed::<16>(&unsigned[4].1)?;
        if self.seen_transition_ids.contains(&transition_id) {
            return Err(Error::Invalid("transition ID reused"));
        }
        let Value::Array(manifest) = &unsigned[9].1 else {
            return Err(Error::Invalid("manifest not array"));
        };
        let mut object_ids = Vec::with_capacity(manifest.len());
        let mut within_transition = BTreeSet::new();
        for item in manifest {
            let object_id = fixed::<16>(&array(item, 4)?[1])?;
            if self.seen_object_ids.contains(&object_id) || !within_transition.insert(object_id) {
                return Err(Error::Invalid("object ID reused"));
            }
            object_ids.push(object_id);
        }
        let mut all = Vec::with_capacity(object_ids.len() + 2);
        all.push(transition_id);
        all.extend_from_slice(&object_ids);
        let newborn = match number(&unsigned[5].1)? {
            2 => Some(fixed::<16>(&exact_map(&unsigned[6].1, 4)?[0].1)?),
            4 => Some(fixed::<16>(&exact_map(&unsigned[6].1, 7)?[1].1)?),
            11 => Some(fixed::<16>(&exact_map(&unsigned[6].1, 4)?[2].1)?),
            _ => None,
        };
        if let Some(id) = newborn {
            all.push(id);
        }
        let mut within_transition = BTreeSet::new();
        if !all
            .iter()
            .all(|id| !self.seen_all_ids.contains(id) && within_transition.insert(*id))
        {
            return Err(Error::Invalid("protocol ID reused across categories"));
        }
        Ok((transition_id, object_ids, all))
    }

    fn remember_control_ids(
        &mut self,
        transition_id: [u8; 16],
        object_ids: Vec<[u8; 16]>,
        all_ids: Vec<[u8; 16]>,
    ) {
        self.seen_transition_ids.insert(transition_id);
        self.seen_object_ids.extend(object_ids);
        self.seen_all_ids.extend(all_ids);
    }

    fn check_unsigned(
        &self,
        unsigned: &[(u64, Value)],
        kind: u64,
        epoch: u64,
    ) -> Result<(), Error> {
        if number(&unsigned[0].1)? != 1
            || number(&unsigned[5].1)? != kind
            || number(&unsigned[8].1)? != epoch
            || fixed::<16>(&unsigned[1].1)? != self.genesis.family_id()
            || fixed::<32>(&unsigned[2].1)? != self.genesis.relay_id()
            || fixed::<32>(&unsigned[3].1)? != self.head_hash
        {
            return Err(Error::Invalid(
                "control version, kind, Family, relay, epoch, or parent mismatch",
            ));
        }
        Ok(())
    }

    fn finalize(
        &mut self,
        bytes: &[u8],
        root: &[(u64, Value)],
        unsigned: &[(u64, Value)],
        plan: FinalizePlan<'_>,
    ) -> Result<(), Error> {
        let FinalizePlan {
            next_state,
            expected_signers,
            manifest_kinds,
            before_ms,
        } = plan;
        let (new_transition_id, new_object_ids, all_ids) = self.check_new_control_ids(unsigned)?;
        let signed_candidate = cbor::encode(&Value::Map(vec![
            (1, root[0].1.clone()),
            (2, root[1].1.clone()),
        ]))?;
        babytrack_wire::authority::prepare_following(
            &signed_candidate,
            self.genesis.family_id(),
            self.genesis.relay_id(),
            self.head_hash,
            number(&unsigned[5].1)?,
            number(&unsigned[8].1)?,
            &next_state,
            expected_signers,
            manifest_kinds,
        )?;
        let membership_check = membership_from_unsigned(self.genesis.family_id(), unsigned)?;
        let receipt = array(&root[2].1, 6)?;
        let expected_cursor = self
            .last_global_cursor
            .checked_add(1)
            .ok_or(Error::Invalid("cursor overflow"))?;
        let committed_ms = signed_number(&receipt[4])?;
        if fixed::<16>(&receipt[0])? != self.genesis.family_id()
            || fixed::<32>(&receipt[1])? != self.genesis.relay_id()
            || fixed::<16>(&receipt[2])? != fixed::<16>(&unsigned[4].1)?
            || number(&receipt[3])? != expected_cursor
            || committed_ms < self.last_commit_ms
            || before_ms.is_some_and(|limit| committed_ms >= limit)
        {
            return Err(Error::Invalid(
                "control receipt context, cursor, or time invalid",
            ));
        }
        let signed = Value::Array(vec![root[0].1.clone(), root[1].1.clone()]);
        if fixed::<32>(&receipt[5])? != crypto::hash("control-signed", &cbor::encode(&signed)?)? {
            return Err(Error::Invalid("control signed-object hash mismatch"));
        }
        crypto::verify_cbor(
            "control-receipt",
            &cbor::encode(&root[2].1)?,
            &self.relay_public_key,
            &fixed::<64>(&root[3].1)?,
        )?;
        let next_head = crypto::hash("control-head", bytes)?;
        let epoch: u32 = number(&exact_map(&next_state, 7)?[3].1)?
            .try_into()
            .map_err(|_| Error::Invalid("epoch outside u32"))?;
        self.state = next_state;
        self.head_hash = next_head;
        self.last_global_cursor = expected_cursor;
        self.last_commit_ms = committed_ms;
        self.known_heads.insert(self.head_hash, epoch);
        if let Some(check) = membership_check {
            self.memberships.insert(check.transition_id, check);
        }
        self.remember_control_ids(new_transition_id, new_object_ids, all_ids);
        Ok(())
    }
}

fn find_row(
    value: &Value,
    width: usize,
    id_index: usize,
    id: [u8; 16],
) -> Result<Option<&[Value]>, Error> {
    let Value::Array(rows) = value else {
        return Err(Error::Invalid("state rows must be array"));
    };
    for row in rows {
        let fields = array(row, width)?;
        if fixed::<16>(&fields[id_index])? == id {
            return Ok(Some(fields));
        }
    }
    Ok(None)
}

fn sort_rows_by_id(rows: &mut [Value]) {
    rows.sort_by(|left, right| {
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
}

fn membership_from_unsigned(
    family_id: [u8; 16],
    unsigned: &[(u64, Value)],
) -> Result<Option<VerifiedMembership>, Error> {
    let Value::Array(manifest) = &unsigned[9].1 else {
        return Err(Error::Invalid("manifest not array"));
    };
    let Some(first) = manifest.first() else {
        return Ok(None);
    };
    let entry = array(first, 4)?;
    if number(&entry[0])? != 1 {
        return Ok(None);
    }
    Ok(Some(VerifiedMembership {
        family_id,
        epoch: number(&unsigned[8].1)?
            .try_into()
            .map_err(|_| Error::Invalid("epoch outside u32"))?,
        object_id: fixed::<16>(&entry[1])?,
        object_hash: fixed::<32>(&entry[2])?,
        core_hash: fixed::<32>(&unsigned[10].1)?,
        transition_id: fixed::<16>(&unsigned[4].1)?,
        prior_head: fixed::<32>(&unsigned[3].1)?,
        state_hash: fixed::<32>(&unsigned[7].1)?,
        delta: unsigned[6].1.clone(),
    }))
}
