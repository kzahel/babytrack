//! Verification of committed v1 control bytes before any Family authority is
//! projected. The first slice verifies genesis; later transitions extend the
//! same pinned chain.

use crate::{
    cbor::{self, Value},
    crypto,
    projection::VerifiedEpochKey,
};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    Cbor(cbor::Error),
    Crypto(crypto::Error),
    Invalid(&'static str),
}

impl From<cbor::Error> for Error {
    fn from(error: cbor::Error) -> Self {
        Self::Cbor(error)
    }
}

impl From<crypto::Error> for Error {
    fn from(error: crypto::Error) -> Self {
        Self::Crypto(error)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Genesis {
    family_id: [u8; 16],
    relay_id: [u8; 32],
    manager_device_id: [u8; 16],
    manager_sign_public_key: [u8; 32],
    manager_agree_public_key: [u8; 32],
    epoch_key_commitment: [u8; 32],
    transition_id: [u8; 16],
    head_hash: [u8; 32],
    state_hash: [u8; 32],
    committed_ms: i64,
}

impl Genesis {
    pub fn family_id(&self) -> [u8; 16] {
        self.family_id
    }
    pub fn relay_id(&self) -> [u8; 32] {
        self.relay_id
    }
    pub fn manager_device_id(&self) -> [u8; 16] {
        self.manager_device_id
    }
    pub fn manager_sign_public_key(&self) -> [u8; 32] {
        self.manager_sign_public_key
    }
    pub fn manager_agree_public_key(&self) -> [u8; 32] {
        self.manager_agree_public_key
    }
    pub fn transition_id(&self) -> [u8; 16] {
        self.transition_id
    }
    pub fn epoch_key_commitment(&self) -> [u8; 32] {
        self.epoch_key_commitment
    }
    pub fn head_hash(&self) -> [u8; 32] {
        self.head_hash
    }
    pub fn state_hash(&self) -> [u8; 32] {
        self.state_hash
    }
    pub fn committed_ms(&self) -> i64 {
        self.committed_ms
    }
    /// The key token is issued only after comparing to the committed public
    /// commitment, so an authorized writer's bad AEAD can be inert without
    /// confusing it with this client's missing/wrong key.
    pub fn verify_epoch_key(&self, key: &[u8; 32]) -> Result<VerifiedEpochKey, Error> {
        let input = cbor::encode(&Value::Array(vec![
            Value::Bytes(self.family_id.to_vec()),
            Value::Integer(1),
            Value::Bytes(key.to_vec()),
        ]))?;
        if crypto::hash("epoch-key", &input)? != self.epoch_key_commitment {
            return Err(Error::Invalid(
                "epoch key does not match genesis commitment",
            ));
        }
        Ok(VerifiedEpochKey {
            family_id: self.family_id,
            epoch: 1,
            bytes: *key,
        })
    }
}

/// Verify every signed, hashed, and state-derived part of a committed
/// genesis. The relay key is pinned by the connection/bootstrap context.
pub fn verify_genesis(bytes: &[u8], relay_public_key: &[u8; 32]) -> Result<Genesis, Error> {
    let root = cbor::decode_with_limits(
        bytes,
        cbor::Limits {
            max_bytes: 1024 * 1024,
            max_depth: 16,
        },
    )?;
    let root = exact_map(&root, 4)?;
    let unsigned = &root[0].1;
    let unsigned_map = exact_map(unsigned, 11)?;
    if number(&unsigned_map[0].1)? != 1
        || number(&unsigned_map[5].1)? != 1
        || number(&unsigned_map[8].1)? != 1
    {
        return Err(Error::Invalid("not version-one genesis"));
    }
    let family_id = fixed::<16>(&unsigned_map[1].1)?;
    let relay_id = fixed::<32>(&unsigned_map[2].1)?;
    let pinned_relay_id: [u8; 32] = Sha256::digest(relay_public_key).into();
    if relay_id != pinned_relay_id {
        return Err(Error::Invalid("relay key does not match pinned ID"));
    }
    verify_genesis_body(
        bytes,
        relay_public_key,
        root,
        unsigned_map,
        family_id,
        relay_id,
    )
}

fn verify_genesis_body(
    bytes: &[u8],
    relay_public_key: &[u8; 32],
    root: &[(u64, Value)],
    unsigned_map: &[(u64, Value)],
    family_id: [u8; 16],
    relay_id: [u8; 32],
) -> Result<Genesis, Error> {
    if fixed::<32>(&unsigned_map[3].1)? != [0u8; 32] {
        return Err(Error::Invalid("genesis has previous head"));
    }
    let transition_id = fixed::<16>(&unsigned_map[4].1)?;
    let delta = exact_map(&unsigned_map[6].1, 3)?;
    let tuple = array(&delta[0].1, 5)?;
    let manager_device_id = fixed::<16>(&tuple[0])?;
    let manager_sign_public_key = fixed::<32>(&tuple[1])?;
    let manager_agree_public_key = fixed::<32>(&tuple[2])?;
    if number(&tuple[3])? == 0 || number(&tuple[4])? != 2 {
        return Err(Error::Invalid("genesis manager tuple invalid"));
    }
    let epoch_key_commitment = fixed::<32>(&delta[1].1)?;
    let promotion_manifest_hash = fixed::<32>(&delta[2].1)?;
    let state = Value::Map(vec![
        (1, Value::Integer(1)),
        (2, Value::Bytes(family_id.to_vec())),
        (3, Value::Bytes(relay_id.to_vec())),
        (4, Value::Integer(1)),
        (5, Value::Array(vec![delta[0].1.clone()])),
        (6, Value::Array(vec![])),
        (7, Value::Array(vec![])),
    ]);
    let state_hash = crypto::hash("auth-state", &cbor::encode(&state)?)?;
    if fixed::<32>(&unsigned_map[7].1)? != state_hash {
        return Err(Error::Invalid("genesis state hash mismatch"));
    }
    let core = Value::Array(vec![
        unsigned_map[0].1.clone(),
        unsigned_map[1].1.clone(),
        unsigned_map[2].1.clone(),
        unsigned_map[3].1.clone(),
        unsigned_map[4].1.clone(),
        unsigned_map[5].1.clone(),
        unsigned_map[6].1.clone(),
        unsigned_map[7].1.clone(),
        unsigned_map[8].1.clone(),
    ]);
    if fixed::<32>(&unsigned_map[10].1)? != crypto::hash("transition-core", &cbor::encode(&core)?)?
    {
        return Err(Error::Invalid("genesis core hash mismatch"));
    }
    let manifest = match &unsigned_map[9].1 {
        Value::Array(items) => items,
        _ => return Err(Error::Invalid("manifest must be array")),
    };
    if manifest.is_empty() || manifest.len() > 16_384 {
        return Err(Error::Invalid("genesis manifest count invalid"));
    }
    let mut prior: Option<(u64, [u8; 16])> = None;
    for (index, item) in manifest.iter().enumerate() {
        let fields = array(item, 4)?;
        let kind = number(&fields[0])?;
        let object_id = fixed::<16>(&fields[1])?;
        let hash = fixed::<32>(&fields[2])?;
        if (index == 0 && (kind != 6 || hash != promotion_manifest_hash))
            || (index > 0 && kind != 7)
        {
            return Err(Error::Invalid("genesis manifest kind or hash mismatch"));
        }
        if number(&fields[3])? > 1024 * 1024 || prior.is_some_and(|p| (kind, object_id) <= p) {
            return Err(Error::Invalid("manifest order or length invalid"));
        }
        prior = Some((kind, object_id));
    }
    let signatures = array(&root[1].1, 1)?;
    let signature = array(&signatures[0], 2)?;
    if fixed::<16>(&signature[0])? != manager_device_id {
        return Err(Error::Invalid("genesis signer is not manager"));
    }
    let manager_signature = fixed::<64>(&signature[1])?;
    let unsigned_bytes = cbor::encode(&root[0].1)?;
    crypto::verify_cbor(
        "control-transition",
        &unsigned_bytes,
        &manager_sign_public_key,
        &manager_signature,
    )?;
    let receipt = array(&root[2].1, 6)?;
    if fixed::<16>(&receipt[0])? != family_id
        || fixed::<32>(&receipt[1])? != relay_id
        || fixed::<16>(&receipt[2])? != transition_id
        || number(&receipt[3])? != 1
    {
        return Err(Error::Invalid("genesis receipt context mismatch"));
    }
    let committed_ms = signed_number(&receipt[4])?;
    let signed_bytes = cbor::encode(&Value::Array(vec![root[0].1.clone(), root[1].1.clone()]))?;
    if fixed::<32>(&receipt[5])? != crypto::hash("control-signed", &signed_bytes)? {
        return Err(Error::Invalid("genesis signed-object hash mismatch"));
    }
    let relay_signature = fixed::<64>(&root[3].1)?;
    crypto::verify_cbor(
        "control-receipt",
        &cbor::encode(&root[2].1)?,
        relay_public_key,
        &relay_signature,
    )?;
    Ok(Genesis {
        family_id,
        relay_id,
        manager_device_id,
        manager_sign_public_key,
        manager_agree_public_key,
        epoch_key_commitment,
        transition_id,
        head_hash: crypto::hash("control-head", bytes)?,
        state_hash,
        committed_ms,
    })
}

pub(crate) fn exact_map(value: &Value, count: usize) -> Result<&[(u64, Value)], Error> {
    let Value::Map(entries) = value else {
        return Err(Error::Invalid("expected map"));
    };
    if entries.len() != count
        || entries
            .iter()
            .enumerate()
            .any(|(i, (key, _))| *key != i as u64 + 1)
    {
        return Err(Error::Invalid("unexpected map keys"));
    }
    Ok(entries)
}

pub(crate) fn array(value: &Value, count: usize) -> Result<&[Value], Error> {
    let Value::Array(items) = value else {
        return Err(Error::Invalid("expected array"));
    };
    if items.len() != count {
        return Err(Error::Invalid("unexpected array length"));
    }
    Ok(items)
}

pub(crate) fn fixed<const N: usize>(value: &Value) -> Result<[u8; N], Error> {
    let Value::Bytes(bytes) = value else {
        return Err(Error::Invalid("expected bytes"));
    };
    bytes
        .as_slice()
        .try_into()
        .map_err(|_| Error::Invalid("unexpected byte length"))
}

pub(crate) fn number(value: &Value) -> Result<u64, Error> {
    let Value::Integer(number) = value else {
        return Err(Error::Invalid("expected integer"));
    };
    (*number)
        .try_into()
        .map_err(|_| Error::Invalid("expected unsigned integer"))
}

pub(crate) fn signed_number(value: &Value) -> Result<i64, Error> {
    let Value::Integer(number) = value else {
        return Err(Error::Invalid("expected integer"));
    };
    (*number)
        .try_into()
        .map_err(|_| Error::Invalid("integer outside i64"))
}
