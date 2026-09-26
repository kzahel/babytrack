//! Incremental public authority replay against a pinned signed genesis.
//! Invitation issue is the first implemented transition; other kinds fail
//! closed until their state rules and object checks are implemented.

use std::collections::{BTreeMap, BTreeSet};

use crate::{
    batch,
    cbor::{self, Value},
    control::{self, Genesis, array, exact_map, fixed, number, signed_number},
    crypto,
    handoff::{self, VerifiedChallenge},
    projection::VerifiedEpochKey,
    session,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    Control(control::Error),
    Cbor(cbor::Error),
    Crypto(crypto::Error),
    Batch(batch::Error),
    Handoff(handoff::Error),
    Invalid(&'static str),
    UnsupportedKind,
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
    admissions: BTreeMap<[u8; 16], [u8; 16]>,
    known_heads: BTreeMap<[u8; 32], u32>,
    next_sequences: BTreeMap<[u8; 16], u64>,
    seen_batch_ids: BTreeSet<[u8; 16]>,
    current_commitment: [u8; 32],
}

struct FinalizePlan<'a> {
    next_state: Value,
    expected_signers: &'a [([u8; 16], [u8; 32])],
    manifest_kinds: &'a [u64],
    before_ms: Option<i64>,
}

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
            admissions: BTreeMap::new(),
            known_heads: BTreeMap::from([(genesis_head, 1)]),
            next_sequences: BTreeMap::new(),
            seen_batch_ids: BTreeSet::new(),
            current_commitment: genesis_commitment,
        })
    }

    pub fn head_hash(&self) -> [u8; 32] {
        self.head_hash
    }
    pub fn last_global_cursor(&self) -> u64 {
        self.last_global_cursor
    }
    pub fn state_bytes(&self) -> Result<Vec<u8>, Error> {
        Ok(cbor::encode(&self.state)?)
    }

    pub fn latest_challenge(&self, invitation_id: &[u8; 16]) -> Option<&VerifiedChallenge> {
        self.challenges.get(invitation_id)
    }

    pub fn verify_current_epoch_key(&self, key: &[u8; 32]) -> Result<VerifiedEpochKey, Error> {
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
        if number(&unsigned[0].1)? != 1
            || number(&unsigned[5].1)? != 2
            || number(&unsigned[8].1)? != self.current_epoch()?
            || fixed::<16>(&unsigned[1].1)? != self.genesis.family_id()
            || fixed::<32>(&unsigned[2].1)? != self.genesis.relay_id()
            || fixed::<32>(&unsigned[3].1)? != self.head_hash
        {
            return Err(Error::Invalid(
                "invite issue version, Family, relay, epoch, or parent mismatch",
            ));
        }
        let transition_id = fixed::<16>(&unsigned[4].1)?;
        let delta = exact_map(&unsigned[6].1, 4)?;
        let invitation_id = fixed::<16>(&delta[0].1)?;
        let issuer_id = fixed::<16>(&delta[1].1)?;
        let invite_public = fixed::<32>(&delta[2].1)?;
        let role = number(&delta[3].1)?;
        if role != 1 && role != 2 {
            return Err(Error::Invalid("invalid invite role"));
        }
        let state = exact_map(&self.state, 7)?;
        let active = match &state[4].1 {
            Value::Array(items) => items,
            _ => return Err(Error::Invalid("active state malformed")),
        };
        let issuer = active
            .iter()
            .find(|row| {
                array(row, 5).is_ok_and(|fields| fixed::<16>(&fields[0]).ok() == Some(issuer_id))
            })
            .ok_or(Error::Invalid("invite issuer is not active"))?;
        if number(&array(issuer, 5)?[4])? != 2 {
            return Err(Error::Invalid("invite issuer is not manager"));
        }
        let invitations = match &state[6].1 {
            Value::Array(items) => items,
            _ => return Err(Error::Invalid("invitation state malformed")),
        };
        if invitations.iter().any(|row| {
            array(row, 6).is_ok_and(|fields| fixed::<16>(&fields[0]).ok() == Some(invitation_id))
        }) {
            return Err(Error::Invalid("invitation ID reused"));
        }
        let mut next_state = self.state.clone();
        let Value::Map(next_map) = &mut next_state else {
            unreachable!()
        };
        let Value::Array(next_invitations) = &mut next_map[6].1 else {
            unreachable!()
        };
        next_invitations.push(Value::Array(vec![
            Value::Bytes(invitation_id.to_vec()),
            Value::Bytes(issuer_id.to_vec()),
            Value::Bytes(invite_public.to_vec()),
            Value::Integer(role.into()),
            Value::Bytes(transition_id.to_vec()),
            Value::Integer(1),
        ]));
        next_invitations.sort_by(|left, right| {
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
        let next_hash = crypto::hash("auth-state", &cbor::encode(&next_state)?)?;
        if fixed::<32>(&unsigned[7].1)? != next_hash {
            return Err(Error::Invalid("invite issue state hash mismatch"));
        }
        let core = Value::Array(
            unsigned[0..9]
                .iter()
                .map(|(_, value)| value.clone())
                .collect(),
        );
        if fixed::<32>(&unsigned[10].1)? != crypto::hash("transition-core", &cbor::encode(&core)?)?
        {
            return Err(Error::Invalid("invite issue core hash mismatch"));
        }
        let manifest = array(&unsigned[9].1, 1)?;
        let membership = array(&manifest[0], 4)?;
        if number(&membership[0])? != 1 || number(&membership[3])? > 1024 * 1024 {
            return Err(Error::Invalid("invite issue membership manifest invalid"));
        }
        let _object_id = fixed::<16>(&membership[1])?;
        let _object_hash = fixed::<32>(&membership[2])?;
        let signatures = array(&root[1].1, 1)?;
        let signature = array(&signatures[0], 2)?;
        if fixed::<16>(&signature[0])? != issuer_id {
            return Err(Error::Invalid("wrong invite issuer signer"));
        }
        let issuer_key = fixed::<32>(&array(issuer, 5)?[1])?;
        crypto::verify_cbor(
            "control-transition",
            &cbor::encode(&root[0].1)?,
            &issuer_key,
            &fixed::<64>(&signature[1])?,
        )?;
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
        self.issue_times.insert(invitation_id, committed_ms);
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
        if number(&unsigned[0].1)? != 1
            || number(&unsigned[5].1)? != 4
            || number(&unsigned[8].1)? != self.current_epoch()?
            || fixed::<16>(&unsigned[1].1)? != self.genesis.family_id()
            || fixed::<32>(&unsigned[2].1)? != self.genesis.relay_id()
            || fixed::<32>(&unsigned[3].1)? != self.head_hash
        {
            return Err(Error::Invalid(
                "claim version, Family, relay, epoch, or parent mismatch",
            ));
        }
        let transition_id = fixed::<16>(&unsigned[4].1)?;
        let delta = exact_map(&unsigned[6].1, 7)?;
        let invitation_id = fixed::<16>(&delta[0].1)?;
        let device_id = fixed::<16>(&delta[1].1)?;
        let sign_public = fixed::<32>(&delta[2].1)?;
        let agree_public = fixed::<32>(&delta[3].1)?;
        let key_version = number(&delta[4].1)?;
        let enrollment_nonce = fixed::<32>(&delta[5].1)?;
        let claim_hash = fixed::<32>(&delta[6].1)?;
        if key_version == 0 || key_version > u32::MAX as u64 {
            return Err(Error::Invalid("claim agreement key version invalid"));
        }
        let state = exact_map(&self.state, 7)?;
        let invitations = match &state[6].1 {
            Value::Array(items) => items,
            _ => return Err(Error::Invalid("invitation state malformed")),
        };
        let invitation = invitations
            .iter()
            .find(|row| {
                array(row, 6)
                    .is_ok_and(|fields| fixed::<16>(&fields[0]).ok() == Some(invitation_id))
            })
            .ok_or(Error::Invalid("claim invitation absent"))?;
        let invitation = array(invitation, 6)?;
        if number(&invitation[5])? != 1 {
            return Err(Error::Invalid("claim invitation is not unused"));
        }
        let issuer_id = fixed::<16>(&invitation[1])?;
        let invite_public = fixed::<32>(&invitation[2])?;
        let role = number(&invitation[3])?;
        let active = match &state[4].1 {
            Value::Array(items) => items,
            _ => return Err(Error::Invalid("active state malformed")),
        };
        if active.iter().any(|row| {
            array(row, 5).is_ok_and(|fields| fixed::<16>(&fields[0]).ok() == Some(device_id))
        }) {
            return Err(Error::Invalid("claim device already active"));
        }
        let issuer = active
            .iter()
            .find(|row| {
                array(row, 5).is_ok_and(|fields| fixed::<16>(&fields[0]).ok() == Some(issuer_id))
            })
            .ok_or(Error::Invalid("claim issuer no longer active"))?;
        if number(&array(issuer, 5)?[4])? != 2 {
            return Err(Error::Invalid("claim issuer no longer manager"));
        }
        let pending = match &state[5].1 {
            Value::Array(items) => items,
            _ => return Err(Error::Invalid("pending state malformed")),
        };
        if pending.iter().any(|row| {
            array(row, 9).is_ok_and(|fields| fixed::<16>(&fields[1]).ok() == Some(device_id))
        }) {
            return Err(Error::Invalid("claim device already pending"));
        }
        let claim_input = Value::Array(vec![
            Value::Bytes(self.genesis.family_id().to_vec()),
            Value::Bytes(self.genesis.relay_id().to_vec()),
            Value::Bytes(invitation_id.to_vec()),
            Value::Integer(role.into()),
            Value::Bytes(device_id.to_vec()),
            Value::Bytes(sign_public.to_vec()),
            Value::Bytes(agree_public.to_vec()),
            Value::Integer(key_version.into()),
            Value::Bytes(enrollment_nonce.to_vec()),
            Value::Bytes(self.head_hash.to_vec()),
        ]);
        if crypto::hash("claim", &cbor::encode(&claim_input)?)? != claim_hash {
            return Err(Error::Invalid("claim hash mismatch"));
        }
        let mut next_state = self.state.clone();
        let Value::Map(next_map) = &mut next_state else {
            unreachable!()
        };
        let Value::Array(next_pending) = &mut next_map[5].1 else {
            unreachable!()
        };
        next_pending.push(Value::Array(vec![
            Value::Bytes(invitation_id.to_vec()),
            Value::Bytes(device_id.to_vec()),
            Value::Bytes(sign_public.to_vec()),
            Value::Bytes(agree_public.to_vec()),
            Value::Integer(key_version.into()),
            Value::Integer(role.into()),
            Value::Bytes(claim_hash.to_vec()),
            Value::Null,
            Value::Null,
        ]));
        next_pending.sort_by(|left, right| {
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
        let Value::Array(next_invitations) = &mut next_map[6].1 else {
            unreachable!()
        };
        let row = next_invitations
            .iter_mut()
            .find(|row| {
                array(row, 6)
                    .is_ok_and(|fields| fixed::<16>(&fields[0]).ok() == Some(invitation_id))
            })
            .unwrap();
        let Value::Array(row) = row else {
            unreachable!()
        };
        row[5] = Value::Integer(2);
        if fixed::<32>(&unsigned[7].1)? != crypto::hash("auth-state", &cbor::encode(&next_state)?)?
        {
            return Err(Error::Invalid("claim state hash mismatch"));
        }
        let core = Value::Array(
            unsigned[0..9]
                .iter()
                .map(|(_, value)| value.clone())
                .collect(),
        );
        if fixed::<32>(&unsigned[10].1)? != crypto::hash("transition-core", &cbor::encode(&core)?)?
        {
            return Err(Error::Invalid("claim core hash mismatch"));
        }
        let _manifest = array(&unsigned[9].1, 0)?;
        let signatures = array(&root[1].1, 2)?;
        let mut prior_signer: Option<[u8; 16]> = None;
        let unsigned_bytes = cbor::encode(&root[0].1)?;
        for signature in signatures {
            let fields = array(signature, 2)?;
            let signer_id = fixed::<16>(&fields[0])?;
            if prior_signer.is_some_and(|prior| signer_id <= prior) {
                return Err(Error::Invalid("claim signature order invalid"));
            }
            prior_signer = Some(signer_id);
            let public = if signer_id == invitation_id {
                invite_public
            } else if signer_id == device_id {
                sign_public
            } else {
                return Err(Error::Invalid("claim signer unexpected"));
            };
            crypto::verify_cbor(
                "control-transition",
                &unsigned_bytes,
                &public,
                &fixed::<64>(&fields[1])?,
            )?;
        }
        if !signatures.iter().any(|entry| {
            array(entry, 2).is_ok_and(|fields| fixed::<16>(&fields[0]).ok() == Some(invitation_id))
        }) || !signatures.iter().any(|entry| {
            array(entry, 2).is_ok_and(|fields| fixed::<16>(&fields[0]).ok() == Some(device_id))
        }) {
            return Err(Error::Invalid(
                "claim needs invitation and device signatures",
            ));
        }
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
        if find_row(&state[4].1, 5, 0, device_id)?.is_none() {
            return Err(Error::Invalid("repair recipient no longer active"));
        }
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
        Ok(())
    }

    /// Verify only public batch authorization and its relay acceptance. Data
    /// decryption/projection is a separate core step; this is useful for
    /// following the global cursor before a later control transition.
    pub fn apply_public_batch(
        &mut self,
        envelope_bytes: &[u8],
        receipt_bytes: &[u8],
    ) -> Result<(), Error> {
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
            || self.seen_batch_ids.contains(&header.batch_id)
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
            || receipt.control_head != self.head_hash
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
        self.seen_batch_ids.insert(header.batch_id);
        Ok(())
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
        self.challenges.clear();
        Ok(())
    }

    fn current_epoch(&self) -> Result<u64, Error> {
        Ok(number(&exact_map(&self.state, 7)?[3].1)?)
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
        if fixed::<32>(&unsigned[7].1)? != crypto::hash("auth-state", &cbor::encode(&next_state)?)?
        {
            return Err(Error::Invalid("resulting authority state hash mismatch"));
        }
        let core = Value::Array(
            unsigned[0..9]
                .iter()
                .map(|(_, value)| value.clone())
                .collect(),
        );
        if fixed::<32>(&unsigned[10].1)? != crypto::hash("transition-core", &cbor::encode(&core)?)?
        {
            return Err(Error::Invalid("transition core hash mismatch"));
        }
        let manifest = array(&unsigned[9].1, manifest_kinds.len())?;
        let mut prior_manifest: Option<(u64, [u8; 16])> = None;
        for (item, kind) in manifest.iter().zip(manifest_kinds) {
            let fields = array(item, 4)?;
            let actual_kind = number(&fields[0])?;
            let object_id = fixed::<16>(&fields[1])?;
            let _hash = fixed::<32>(&fields[2])?;
            if actual_kind != *kind
                || number(&fields[3])? > 1024 * 1024
                || prior_manifest.is_some_and(|prior| (actual_kind, object_id) <= prior)
            {
                return Err(Error::Invalid("manifest kind, length, or order invalid"));
            }
            prior_manifest = Some((actual_kind, object_id));
        }
        let signatures = array(&root[1].1, expected_signers.len())?;
        let unsigned_bytes = cbor::encode(&root[0].1)?;
        let mut prior_signer = None;
        for (entry, (expected_id, public_key)) in signatures.iter().zip(expected_signers) {
            let fields = array(entry, 2)?;
            let signer_id = fixed::<16>(&fields[0])?;
            if signer_id != *expected_id || prior_signer.is_some_and(|prior| signer_id <= prior) {
                return Err(Error::Invalid("control signer set/order invalid"));
            }
            prior_signer = Some(signer_id);
            crypto::verify_cbor(
                "control-transition",
                &unsigned_bytes,
                public_key,
                &fixed::<64>(&fields[1])?,
            )?;
        }
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
