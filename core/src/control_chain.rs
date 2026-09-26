//! Incremental public authority replay against a pinned signed genesis.
//! Invitation issue is the first implemented transition; other kinds fail
//! closed until their state rules and object checks are implemented.

use std::collections::BTreeMap;

use crate::{
    cbor::{self, Value},
    control::{self, Genesis, array, exact_map, fixed, number, signed_number},
    crypto,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    Control(control::Error),
    Cbor(cbor::Error),
    Crypto(crypto::Error),
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

pub struct ControlChain {
    genesis: Genesis,
    relay_public_key: [u8; 32],
    state: Value,
    head_hash: [u8; 32],
    last_global_cursor: u64,
    last_commit_ms: i64,
    issue_times: BTreeMap<[u8; 16], i64>,
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
        Ok(Self {
            head_hash: genesis.head_hash(),
            last_global_cursor: 1,
            last_commit_ms: genesis.committed_ms(),
            genesis,
            relay_public_key,
            state,
            issue_times: BTreeMap::new(),
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
            || number(&unsigned[8].1)? != 1
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
        self.state = next_state;
        self.head_hash = crypto::hash("control-head", bytes)?;
        self.last_global_cursor = expected_cursor;
        self.last_commit_ms = committed_ms;
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
            || number(&unsigned[8].1)? != 1
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
        self.state = next_state;
        self.head_hash = crypto::hash("control-head", bytes)?;
        self.last_global_cursor = expected_cursor;
        self.last_commit_ms = committed_ms;
        Ok(())
    }
}
