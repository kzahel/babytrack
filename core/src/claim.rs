//! Pure claim candidate construction shared by native and wasm enrollment.

use crate::{
    bootstrap::InvitationBootstrap,
    cbor::{self, Value},
    crypto, hpke,
};

#[derive(Debug)]
pub enum Error {
    Cbor(cbor::Error),
    Crypto(crypto::Error),
    Hpke(hpke::Error),
    Chain(crate::control_chain::Error),
    Authority(babytrack_wire::authority::Error),
    Invalid(&'static str),
}

#[derive(Debug, Clone, Copy)]
pub struct ClaimIdentity {
    pub family_id: [u8; 16],
    pub device_id: [u8; 16],
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
impl From<hpke::Error> for Error {
    fn from(v: hpke::Error) -> Self {
        Self::Hpke(v)
    }
}
impl From<crate::control_chain::Error> for Error {
    fn from(v: crate::control_chain::Error) -> Self {
        Self::Chain(v)
    }
}
impl From<babytrack_wire::authority::Error> for Error {
    fn from(v: babytrack_wire::authority::Error) -> Self {
        Self::Authority(v)
    }
}

/// Build a claim from a verified invitation prefix. Callers must durably save
/// the exact candidate and device secrets before sending it to a relay.
pub fn build_claim(
    bootstrap: &InvitationBootstrap,
    chain: &crate::control_chain::ControlChain,
    identity: ClaimIdentity,
    device_sign_seed: &[u8; 32],
    device_agreement_private: &[u8; 32],
    enrollment_nonce: &[u8; 32],
    transition_id: [u8; 16],
) -> Result<Vec<u8>, Error> {
    let ClaimIdentity {
        family_id,
        device_id,
    } = identity;
    let sign_public = crypto::signing_public_key(device_sign_seed);
    let agree_public = hpke::public_key_from_private(device_agreement_private)?;
    let claim_input = Value::Array(vec![
        Value::Bytes(family_id.to_vec()),
        Value::Bytes(chain.relay_id().to_vec()),
        Value::Bytes(bootstrap.invitation_id().to_vec()),
        Value::Integer(bootstrap.fixed_role().into()),
        Value::Bytes(device_id.to_vec()),
        Value::Bytes(sign_public.to_vec()),
        Value::Bytes(agree_public.to_vec()),
        Value::Integer(1),
        Value::Bytes(enrollment_nonce.to_vec()),
        Value::Bytes(chain.head_hash().to_vec()),
    ]);
    let claim_hash = crypto::hash("claim", &cbor::encode(&claim_input)?)?;
    let mut state = cbor::decode(&chain.state_bytes()?)?;
    let Value::Map(state_fields) = &mut state else {
        return Err(Error::Invalid("verified authority state not map"));
    };
    let Value::Array(invitations) = &mut state_fields[6].1 else {
        return Err(Error::Invalid("verified invitations not array"));
    };
    let row = invitations
        .iter_mut()
        .find(|row| {
            matches!(row, Value::Array(fields) if fields[0] == Value::Bytes(bootstrap.invitation_id().to_vec()))
        })
        .ok_or(Error::Invalid("linked invitation missing"))?;
    let Value::Array(invitation) = row else {
        unreachable!()
    };
    if invitation[5] != Value::Integer(1) {
        return Err(Error::Invalid("invitation is already consumed"));
    }
    invitation[5] = Value::Integer(2);
    let Value::Array(pending) = &mut state_fields[5].1 else {
        return Err(Error::Invalid("verified pending state not array"));
    };
    pending.push(Value::Array(vec![
        Value::Bytes(bootstrap.invitation_id().to_vec()),
        Value::Bytes(device_id.to_vec()),
        Value::Bytes(sign_public.to_vec()),
        Value::Bytes(agree_public.to_vec()),
        Value::Integer(1),
        Value::Integer(bootstrap.fixed_role().into()),
        Value::Bytes(claim_hash.to_vec()),
        Value::Null,
        Value::Null,
    ]));
    pending.sort_by(|left, right| {
        let (Value::Array(left), Value::Array(right)) = (left, right) else {
            unreachable!()
        };
        let (Value::Bytes(left), Value::Bytes(right)) = (&left[0], &right[0]) else {
            unreachable!()
        };
        left.cmp(right)
    });
    let delta = Value::Map(vec![
        (1, Value::Bytes(bootstrap.invitation_id().to_vec())),
        (2, Value::Bytes(device_id.to_vec())),
        (3, Value::Bytes(sign_public.to_vec())),
        (4, Value::Bytes(agree_public.to_vec())),
        (5, Value::Integer(1)),
        (6, Value::Bytes(enrollment_nonce.to_vec())),
        (7, Value::Bytes(claim_hash.to_vec())),
    ]);
    let mut parts = vec![
        Value::Integer(1),
        Value::Bytes(family_id.to_vec()),
        Value::Bytes(chain.relay_id().to_vec()),
        Value::Bytes(chain.head_hash().to_vec()),
        Value::Bytes(transition_id.to_vec()),
        Value::Integer(4),
        delta,
        Value::Bytes(crypto::hash("auth-state", &cbor::encode(&state)?)?.to_vec()),
        Value::Integer(chain.epoch()?.into()),
    ];
    let core_hash = crypto::hash(
        "transition-core",
        &cbor::encode(&Value::Array(parts.clone()))?,
    )?;
    parts.push(Value::Array(vec![]));
    parts.push(Value::Bytes(core_hash.to_vec()));
    let unsigned = Value::Map(
        parts
            .into_iter()
            .enumerate()
            .map(|(index, value)| (index as u64 + 1, value))
            .collect(),
    );
    let unsigned_bytes = cbor::encode(&unsigned)?;
    let mut signatures = vec![
        (bootstrap.invitation_id(), bootstrap.invitation_sign_seed()),
        (device_id, *device_sign_seed),
    ];
    signatures.sort_by_key(|entry| entry.0);
    let signatures = Value::Array(
        signatures
            .into_iter()
            .map(|(id, seed)| {
                Ok(Value::Array(vec![
                    Value::Bytes(id.to_vec()),
                    Value::Bytes(
                        crypto::sign_cbor("control-transition", &unsigned_bytes, &seed)?.to_vec(),
                    ),
                ]))
            })
            .collect::<Result<Vec<_>, Error>>()?,
    );
    let candidate = cbor::encode(&Value::Map(vec![(1, unsigned), (2, signatures)]))?;
    babytrack_wire::authority::prepare_claim(
        &candidate,
        &cbor::decode(&chain.state_bytes()?)?,
        chain.head_hash(),
    )?;
    Ok(candidate)
}
