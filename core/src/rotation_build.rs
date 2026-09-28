//! Construct a manager-signed removal with a fresh epoch key and one grant
//! for every remaining active device. The caller durably saves this proposal
//! before sending any object to the relay.

use std::collections::BTreeMap;

use rand_chacha::{ChaCha20Rng, rand_core::SeedableRng};

use crate::{
    cbor::{self, Value},
    control_build,
    control_chain::{self, ControlChain},
    crypto, hpke,
    sqlite_store::FamilyHandle,
};

#[derive(Debug)]
pub enum Error {
    Cbor(cbor::Error),
    Chain(control_chain::Error),
    Crypto(crypto::Error),
    Hpke(hpke::Error),
    Random(getrandom::Error),
    Invalid(&'static str),
}
impl From<cbor::Error> for Error {
    fn from(value: cbor::Error) -> Self {
        Self::Cbor(value)
    }
}
impl From<control_chain::Error> for Error {
    fn from(value: control_chain::Error) -> Self {
        Self::Chain(value)
    }
}
impl From<crypto::Error> for Error {
    fn from(value: crypto::Error) -> Self {
        Self::Crypto(value)
    }
}
impl From<hpke::Error> for Error {
    fn from(value: hpke::Error) -> Self {
        Self::Hpke(value)
    }
}
impl From<getrandom::Error> for Error {
    fn from(value: getrandom::Error) -> Self {
        Self::Random(value)
    }
}
impl From<control_build::Error> for Error {
    fn from(value: control_build::Error) -> Self {
        match value {
            control_build::Error::Cbor(error) => Self::Cbor(error),
            control_build::Error::Crypto(error) => Self::Crypto(error),
            control_build::Error::Invalid(reason) => Self::Invalid(reason),
        }
    }
}

pub struct RotationProposal {
    pub transition_id: [u8; 16],
    pub candidate_bytes: Vec<u8>,
    pub objects: Vec<(u16, [u8; 16], Vec<u8>)>,
    pub new_epoch_key: [u8; 32],
}

/// Compatibility entry for the first rotation's fixed byte-vector tests.
pub fn prepare_first_removal(
    chain: &ControlChain,
    family: FamilyHandle,
    manager_signing_seed: [u8; 32],
    epoch_one_key: [u8; 32],
    target_id: [u8; 16],
) -> Result<RotationProposal, Error> {
    if chain.epoch()? != 1 {
        return Err(Error::Invalid(
            "first rotation requires matching epoch-one Family",
        ));
    }
    prepare_removal(
        chain,
        family,
        manager_signing_seed,
        &BTreeMap::from([(1, epoch_one_key)]),
        target_id,
    )
}

/// Build one rotation from a fully verified, contiguous history keyring.
pub fn prepare_removal(
    chain: &ControlChain,
    family: FamilyHandle,
    manager_signing_seed: [u8; 32],
    prior_keys: &BTreeMap<u32, [u8; 32]>,
    target_id: [u8; 16],
) -> Result<RotationProposal, Error> {
    if family.family_id != chain.family_id() {
        return Err(Error::Invalid("rotation belongs to another Family"));
    }
    chain.verify_epoch_keys(prior_keys)?;
    let next_epoch = chain
        .epoch()?
        .checked_add(1)
        .ok_or(Error::Invalid("rotation epoch overflow"))?;
    let state = cbor::decode(&chain.state_bytes()?)?;
    let Value::Map(mut next) = state else {
        return Err(Error::Invalid("auth state is not a map"));
    };
    let Value::Array(active) = &mut next[4].1 else {
        return Err(Error::Invalid("active devices are not an array"));
    };
    let manager = active
        .iter()
        .find_map(|row| match row {
            Value::Array(fields)
                if fields.len() == 5 && fields[0] == Value::Bytes(family.device_id.to_vec()) =>
            {
                Some(fields)
            }
            _ => None,
        })
        .ok_or(Error::Invalid("manager device not active"))?;
    if manager[4] != Value::Integer(2)
        || manager[1] != Value::Bytes(crypto::signing_public_key(&manager_signing_seed).to_vec())
    {
        return Err(Error::Invalid("signing device is not an active manager"));
    }
    let target = active
        .iter()
        .find_map(|row| match row {
            Value::Array(fields)
                if fields.len() == 5 && fields[0] == Value::Bytes(target_id.to_vec()) =>
            {
                Some(fields)
            }
            _ => None,
        })
        .ok_or(Error::Invalid("removal target not active"))?;
    let old_role = target[4].clone();
    active.retain(
        |row| !matches!(row, Value::Array(fields) if fields[0] == Value::Bytes(target_id.to_vec())),
    );
    if !active
        .iter()
        .any(|row| matches!(row, Value::Array(fields) if fields[4] == Value::Integer(2)))
    {
        return Err(Error::Invalid("cannot remove final manager"));
    }
    let recipients = active.clone();
    next[3].1 = Value::Integer(next_epoch.into());
    let Value::Array(pending) = &mut next[5].1 else {
        return Err(Error::Invalid("pending devices are not an array"));
    };
    for row in pending {
        let Value::Array(fields) = row else {
            return Err(Error::Invalid("pending row is not an array"));
        };
        fields[7] = Value::Null;
        fields[8] = Value::Null;
    }
    let Value::Array(invitations) = &mut next[6].1 else {
        return Err(Error::Invalid("invitations are not an array"));
    };
    for row in invitations {
        let Value::Array(fields) = row else {
            return Err(Error::Invalid("invitation row is not an array"));
        };
        if fields[1] == Value::Bytes(target_id.to_vec()) && fields[5] == Value::Integer(1) {
            fields[5] = Value::Integer(3);
        }
    }
    let next_state = Value::Map(next);
    let transition_id = random_v4()?;
    let membership_id = random_v4()?;
    let keyring_id = random_v4()?;
    let new_key = random::<32>()?;
    if prior_keys.values().any(|key| *key == new_key) {
        return Err(Error::Invalid("new epoch key repeated old key"));
    }
    let commitment = crypto::hash(
        "epoch-key",
        &cbor::encode(&Value::Array(vec![
            Value::Bytes(family.family_id.to_vec()),
            Value::Integer(next_epoch.into()),
            Value::Bytes(new_key.to_vec()),
        ]))?,
    )?;
    let delta = Value::Map(vec![
        (1, Value::Bytes(target_id.to_vec())),
        (2, old_role),
        (3, Value::Bytes(commitment.to_vec())),
    ]);
    let core_hash = control_build::core_hash(
        family,
        chain.relay_id(),
        chain.head_hash(),
        transition_id,
        8,
        &delta,
        &next_state,
        next_epoch,
    )?;
    let membership_plain = cbor::encode(&Value::Map(vec![
        (1, Value::Bytes(transition_id.to_vec())),
        (2, Value::Bytes(chain.head_hash().to_vec())),
        (
            3,
            Value::Bytes(crypto::hash("auth-state", &cbor::encode(&next_state)?)?.to_vec()),
        ),
        (4, Value::Integer(next_epoch.into())),
        (5, delta.clone()),
    ]))?;
    let membership_nonce = random::<24>()?;
    let membership_aad = crypto::hash("membership-aad", &core_hash)?;
    let membership_ciphertext = crypto::seal_with_nonce(
        &new_key,
        &membership_nonce,
        &membership_aad,
        &membership_plain,
    )?;
    let membership = cbor::encode(&Value::Map(vec![
        (1, Value::Integer(1)),
        (2, Value::Bytes(membership_nonce.to_vec())),
        (3, Value::Bytes(membership_ciphertext)),
    ]))?;
    let context = cbor::encode(&Value::Array(vec![
        Value::Bytes(core_hash.to_vec()),
        Value::Bytes(family.family_id.to_vec()),
        Value::Bytes(chain.relay_id().to_vec()),
        Value::Bytes(chain.head_hash().to_vec()),
        Value::Bytes(transition_id.to_vec()),
        Value::Bytes(crypto::hash("auth-state", &cbor::encode(&next_state)?)?.to_vec()),
        Value::Integer(next_epoch.into()),
        Value::Bytes(commitment.to_vec()),
        delta.clone(),
    ]))?;
    let grant_plain = cbor::encode(&Value::Array(vec![
        Value::Integer(1),
        Value::Integer(next_epoch.into()),
        Value::Bytes(new_key.to_vec()),
    ]))?;
    let mut objects = vec![(1, membership_id, membership)];
    for row in recipients {
        let Value::Array(fields) = row else {
            return Err(Error::Invalid("active row is not an array"));
        };
        let Value::Bytes(device_bytes) = &fields[0] else {
            return Err(Error::Invalid("device ID is not bytes"));
        };
        let device_id: [u8; 16] = device_bytes
            .as_slice()
            .try_into()
            .map_err(|_| Error::Invalid("device ID length"))?;
        let Value::Bytes(agree_bytes) = &fields[2] else {
            return Err(Error::Invalid("agreement key is not bytes"));
        };
        let agree_public: [u8; 32] = agree_bytes
            .as_slice()
            .try_into()
            .map_err(|_| Error::Invalid("agreement key length"))?;
        let Value::Integer(version) = fields[3] else {
            return Err(Error::Invalid("key version is not integer"));
        };
        let version: u32 = version
            .try_into()
            .map_err(|_| Error::Invalid("key version outside u32"))?;
        let grant_id = random_v4()?;
        let info = crypto::hash(
            "rotation-grant-info",
            &cbor::encode(&Value::Array(vec![
                Value::Bytes(core_hash.to_vec()),
                Value::Bytes(device_id.to_vec()),
                Value::Integer(version.into()),
            ]))?,
        )?;
        let mut rng = ChaCha20Rng::from_seed(random::<32>()?);
        let sealed = hpke::seal_with_rng(&agree_public, &info, &context, &grant_plain, &mut rng)?;
        let grant = cbor::encode(&Value::Map(vec![
            (1, Value::Integer(1)),
            (2, Value::Bytes(grant_id.to_vec())),
            (3, Value::Integer(2)),
            (4, Value::Bytes(device_id.to_vec())),
            (5, Value::Integer(version.into())),
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
        objects.push((4, grant_id, grant));
    }
    let keyring_plain = cbor::encode(&Value::Array(vec![
        Value::Integer(1),
        Value::Array(
            prior_keys
                .iter()
                .map(|(epoch, key)| {
                    Value::Array(vec![
                        Value::Integer((*epoch).into()),
                        Value::Bytes(key.to_vec()),
                    ])
                })
                .collect(),
        ),
    ]))?;
    let keyring_nonce = random::<24>()?;
    let keyring_aad = crypto::hash("keyring-aad", &core_hash)?;
    let keyring_ciphertext =
        crypto::seal_with_nonce(&new_key, &keyring_nonce, &keyring_aad, &keyring_plain)?;
    let keyring = cbor::encode(&Value::Map(vec![
        (1, Value::Integer(1)),
        (2, Value::Bytes(keyring_nonce.to_vec())),
        (3, Value::Bytes(keyring_ciphertext)),
    ]))?;
    objects.push((5, keyring_id, keyring));
    objects.sort_by_key(|(kind, id, _)| (*kind, *id));
    let candidate_bytes = control_build::candidate(
        family,
        chain.relay_id(),
        chain.head_hash(),
        transition_id,
        8,
        delta,
        next_state,
        next_epoch,
        &objects,
        &manager_signing_seed,
    )?;
    Ok(RotationProposal {
        transition_id,
        candidate_bytes,
        objects,
        new_epoch_key: new_key,
    })
}

fn random<const N: usize>() -> Result<[u8; N], Error> {
    let mut bytes = [0; N];
    getrandom::fill(&mut bytes)?;
    Ok(bytes)
}
fn random_v4() -> Result<[u8; 16], Error> {
    let mut bytes = random::<16>()?;
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Ok(bytes)
}
