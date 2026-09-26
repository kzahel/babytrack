//! Incremental public authority replay against a pinned signed genesis.
//! Invitation issue is the first implemented transition; other kinds fail
//! closed until their state rules and object checks are implemented.

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
        Ok(())
    }
}
