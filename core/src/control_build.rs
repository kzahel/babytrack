//! Client-side construction of signed public transitions. Callers supply a
//! fully computed state and exact object set; committed replay is authoritative.

use crate::{
    cbor::{self, Value},
    crypto,
    sqlite_store::FamilyHandle,
};

#[allow(clippy::too_many_arguments)]
pub(crate) fn candidate(
    family: FamilyHandle,
    relay_id: [u8; 32],
    prior_head: [u8; 32],
    transition_id: [u8; 16],
    kind: u8,
    delta: Value,
    next_state: Value,
    epoch: u32,
    objects: &[(u16, [u8; 16], Vec<u8>)],
    signing_seed: &[u8; 32],
) -> Result<Vec<u8>, Error> {
    let mut manifest = Vec::with_capacity(objects.len());
    let mut previous = None;
    for (kind, id, bytes) in objects {
        if previous.is_some_and(|p| (*kind, *id) <= p) || bytes.len() > 1024 * 1024 {
            return Err(Error::Invalid("object order or length"));
        }
        previous = Some((*kind, *id));
        manifest.push(Value::Array(vec![
            Value::Integer((*kind).into()),
            Value::Bytes(id.to_vec()),
            Value::Bytes(crypto::hash("object", bytes)?.to_vec()),
            Value::Integer(bytes.len() as i128),
        ]));
    }
    let core = vec![
        Value::Integer(1),
        Value::Bytes(family.family_id.to_vec()),
        Value::Bytes(relay_id.to_vec()),
        Value::Bytes(prior_head.to_vec()),
        Value::Bytes(transition_id.to_vec()),
        Value::Integer(kind.into()),
        delta,
        Value::Bytes(crypto::hash("auth-state", &cbor::encode(&next_state)?)?.to_vec()),
        Value::Integer(epoch.into()),
    ];
    let core_hash = crypto::hash(
        "transition-core",
        &cbor::encode(&Value::Array(core.clone()))?,
    )?;
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
    let signature = crypto::sign_cbor(
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
                Value::Bytes(signature.to_vec()),
            ])]),
        ),
    ]))?)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn core_hash(
    family: FamilyHandle,
    relay_id: [u8; 32],
    prior_head: [u8; 32],
    transition_id: [u8; 16],
    kind: u8,
    delta: &Value,
    next_state: &Value,
    epoch: u32,
) -> Result<[u8; 32], Error> {
    let core = Value::Array(vec![
        Value::Integer(1),
        Value::Bytes(family.family_id.to_vec()),
        Value::Bytes(relay_id.to_vec()),
        Value::Bytes(prior_head.to_vec()),
        Value::Bytes(transition_id.to_vec()),
        Value::Integer(kind.into()),
        delta.clone(),
        Value::Bytes(crypto::hash("auth-state", &cbor::encode(next_state)?)?.to_vec()),
        Value::Integer(epoch.into()),
    ]);
    Ok(crypto::hash("transition-core", &cbor::encode(&core)?)?)
}

pub(crate) fn stage_body(
    candidate_bytes: &[u8],
    kind: u16,
    id: [u8; 16],
    bytes: &[u8],
) -> Result<Vec<u8>, Error> {
    let value = cbor::decode(candidate_bytes)?;
    let Value::Map(candidate) = value else {
        return Err(Error::Invalid("candidate not map"));
    };
    if candidate.len() != 2 || candidate[0].0 != 1 || candidate[1].0 != 2 {
        return Err(Error::Invalid("candidate keys"));
    }
    Ok(cbor::encode(&Value::Map(vec![
        (1, Value::Integer(1)),
        (2, candidate[0].1.clone()),
        (3, candidate[1].1.clone()),
        (4, Value::Integer(kind.into())),
        (5, Value::Bytes(id.to_vec())),
        (6, Value::Bytes(bytes.to_vec())),
    ]))?)
}

#[derive(Debug)]
pub(crate) enum Error {
    Cbor(cbor::Error),
    Crypto(crypto::Error),
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
