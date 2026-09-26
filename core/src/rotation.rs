//! Verify every addressed grant and the history keyring of a rotating removal.

use std::collections::{BTreeMap, BTreeSet};

use crate::{
    cbor::{self, Value},
    crypto, hpke,
    projection::VerifiedEpochKey,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    Cbor(cbor::Error),
    Crypto(crypto::Error),
    Hpke(hpke::Error),
    Invalid(&'static str),
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
impl From<hpke::Error> for Error {
    fn from(value: hpke::Error) -> Self {
        Self::Hpke(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ObjectRef {
    pub id: [u8; 16],
    pub hash: [u8; 32],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedRotation {
    pub(crate) family_id: [u8; 16],
    pub(crate) relay_id: [u8; 32],
    pub(crate) prior_head: [u8; 32],
    pub(crate) transition_id: [u8; 16],
    pub(crate) state_hash: [u8; 32],
    pub(crate) epoch: u32,
    pub(crate) commitment: [u8; 32],
    pub(crate) core_hash: [u8; 32],
    pub(crate) delta: Value,
    pub(crate) grants: Vec<ObjectRef>,
    pub(crate) keyring: ObjectRef,
    pub(crate) recipients: BTreeMap<[u8; 16], ([u8; 32], u32)>,
    pub(crate) prior_commitments: BTreeMap<u32, [u8; 32]>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedRotationKeys {
    current: VerifiedEpochKey,
    earlier: BTreeMap<u32, VerifiedEpochKey>,
}
impl VerifiedRotationKeys {
    pub fn current(&self) -> &VerifiedEpochKey {
        &self.current
    }
    pub fn earlier(&self, epoch: u32) -> Option<&VerifiedEpochKey> {
        self.earlier.get(&epoch)
    }
}

impl VerifiedRotation {
    pub fn grant_ids(&self) -> Vec<[u8; 16]> {
        self.grants.iter().map(|item| item.id).collect()
    }
    pub fn keyring_id(&self) -> [u8; 16] {
        self.keyring.id
    }

    /// Require exactly one valid grant for every remaining active device.
    /// The new key is usable only after the complete keyring also verifies.
    pub fn open_for(
        &self,
        device_id: [u8; 16],
        agreement_private: &[u8; 32],
        grant_objects: &[([u8; 16], Vec<u8>)],
        keyring_object: &[u8],
    ) -> Result<VerifiedRotationKeys, Error> {
        let (agree_public, key_version) = self
            .recipients
            .get(&device_id)
            .ok_or(Error::Invalid("device removed from rotation"))?;
        if hpke::public_key_from_private(agreement_private)? != *agree_public {
            return Err(Error::Invalid(
                "rotation agreement key differs from active device",
            ));
        }
        if grant_objects.len() != self.grants.len() || self.grants.len() != self.recipients.len() {
            return Err(Error::Invalid("rotation grant set incomplete"));
        }
        let context = cbor::encode(&Value::Array(vec![
            Value::Bytes(self.core_hash.to_vec()),
            Value::Bytes(self.family_id.to_vec()),
            Value::Bytes(self.relay_id.to_vec()),
            Value::Bytes(self.prior_head.to_vec()),
            Value::Bytes(self.transition_id.to_vec()),
            Value::Bytes(self.state_hash.to_vec()),
            Value::Integer(self.epoch.into()),
            Value::Bytes(self.commitment.to_vec()),
            self.delta.clone(),
        ]))?;
        let mut object_ids = BTreeSet::new();
        let mut recipient_ids = BTreeSet::new();
        let mut addressed = None;
        for (object_id, object_bytes) in grant_objects {
            if !object_ids.insert(*object_id) {
                return Err(Error::Invalid("duplicate rotation grant object"));
            }
            let listed = self
                .grants
                .iter()
                .find(|item| item.id == *object_id)
                .ok_or(Error::Invalid("unlisted rotation grant object"))?;
            if crypto::hash("object", object_bytes)? != listed.hash {
                return Err(Error::Invalid("rotation grant object hash mismatch"));
            }
            let value = cbor::decode_with_limits(
                object_bytes,
                cbor::Limits {
                    max_bytes: 1024 * 1024,
                    max_depth: 16,
                },
            )?;
            let fields = exact_map(&value, 9)?;
            let recipient_id = fixed::<16>(&fields[3].1)?;
            let (_, declared_version) = self.recipients.get(&recipient_id).ok_or(
                Error::Invalid("grant addressed to removed or pending device"),
            )?;
            if number(&fields[0].1)? != 1
                || fixed::<16>(&fields[1].1)? != *object_id
                || number(&fields[2].1)? != 2
                || number(&fields[4].1)? != *declared_version as u64
                || fixed::<32>(&fields[6].1)? != self.core_hash
            {
                return Err(Error::Invalid("rotation grant fields mismatch"));
            }
            let Value::Array(suite) = &fields[5].1 else {
                return Err(Error::Invalid("rotation suite not array"));
            };
            if suite != &[Value::Integer(32), Value::Integer(1), Value::Integer(3)] {
                return Err(Error::Invalid("rotation HPKE suite mismatch"));
            }
            if !recipient_ids.insert(recipient_id) {
                return Err(Error::Invalid("duplicate recipient rotation grant"));
            }
            if recipient_id == device_id {
                addressed = Some((
                    fixed::<32>(&fields[7].1)?,
                    byte_string(&fields[8].1)?.to_vec(),
                ));
            }
        }
        if recipient_ids.len() != self.recipients.len() {
            return Err(Error::Invalid("missing recipient rotation grant"));
        }
        let (enc, ciphertext) = addressed.ok_or(Error::Invalid("no addressed rotation grant"))?;
        let info_value = cbor::encode(&Value::Array(vec![
            Value::Bytes(self.core_hash.to_vec()),
            Value::Bytes(device_id.to_vec()),
            Value::Integer((*key_version).into()),
        ]))?;
        let info = crypto::hash("rotation-grant-info", &info_value)?;
        let plaintext = hpke::open(agreement_private, &enc, &info, &context, &ciphertext)?;
        let new_key = epoch_key_from_plaintext(&plaintext, self.epoch)?;
        check_commitment(self.family_id, self.epoch, &new_key, self.commitment)?;
        let current = VerifiedEpochKey {
            family_id: self.family_id,
            epoch: self.epoch,
            bytes: new_key,
        };
        let earlier = self.open_keyring(keyring_object, &current)?;
        Ok(VerifiedRotationKeys { current, earlier })
    }

    fn open_keyring(
        &self,
        object_bytes: &[u8],
        current: &VerifiedEpochKey,
    ) -> Result<BTreeMap<u32, VerifiedEpochKey>, Error> {
        if crypto::hash("object", object_bytes)? != self.keyring.hash {
            return Err(Error::Invalid("rotation keyring object hash mismatch"));
        }
        let value = cbor::decode_with_limits(
            object_bytes,
            cbor::Limits {
                max_bytes: 1024 * 1024,
                max_depth: 16,
            },
        )?;
        let fields = exact_map(&value, 3)?;
        if number(&fields[0].1)? != 1 {
            return Err(Error::Invalid("keyring object version invalid"));
        }
        let nonce = fixed::<24>(&fields[1].1)?;
        let aad = crypto::hash("keyring-aad", &self.core_hash)?;
        let plaintext = crypto::open(&current.bytes, &nonce, &aad, byte_string(&fields[2].1)?)?;
        let value = cbor::decode_with_limits(
            &plaintext,
            cbor::Limits {
                max_bytes: 1024 * 1024,
                max_depth: 16,
            },
        )?;
        let Value::Array(parts) = value else {
            return Err(Error::Invalid("keyring plaintext not array"));
        };
        if parts.len() != 2 || number(&parts[0])? != 1 {
            return Err(Error::Invalid("keyring plaintext version invalid"));
        }
        let Value::Array(entries) = &parts[1] else {
            return Err(Error::Invalid("keyring entries not array"));
        };
        if entries.len() != self.prior_commitments.len()
            || entries.len() != (self.epoch - 1) as usize
        {
            return Err(Error::Invalid("keyring missing or extra epoch"));
        }
        let mut result = BTreeMap::new();
        for (index, entry) in entries.iter().enumerate() {
            let Value::Array(pair) = entry else {
                return Err(Error::Invalid("keyring entry not array"));
            };
            if pair.len() != 2 {
                return Err(Error::Invalid("keyring entry width invalid"));
            }
            let epoch: u32 = number(&pair[0])?
                .try_into()
                .map_err(|_| Error::Invalid("keyring epoch outside u32"))?;
            if epoch != index as u32 + 1 {
                return Err(Error::Invalid("keyring epochs not contiguous"));
            }
            let key = fixed::<32>(&pair[1])?;
            let commitment = *self
                .prior_commitments
                .get(&epoch)
                .ok_or(Error::Invalid("unknown earlier epoch"))?;
            check_commitment(self.family_id, epoch, &key, commitment)?;
            result.insert(
                epoch,
                VerifiedEpochKey {
                    family_id: self.family_id,
                    epoch,
                    bytes: key,
                },
            );
        }
        Ok(result)
    }
}

fn epoch_key_from_plaintext(bytes: &[u8], epoch: u32) -> Result<[u8; 32], Error> {
    let value = cbor::decode_with_limits(
        bytes,
        cbor::Limits {
            max_bytes: 128,
            max_depth: 3,
        },
    )?;
    let Value::Array(parts) = value else {
        return Err(Error::Invalid("rotation grant plaintext not array"));
    };
    if parts.len() != 3 || number(&parts[0])? != 1 || number(&parts[1])? != epoch as u64 {
        return Err(Error::Invalid(
            "rotation grant plaintext version or epoch invalid",
        ));
    }
    fixed::<32>(&parts[2])
}
fn check_commitment(
    family: [u8; 16],
    epoch: u32,
    key: &[u8; 32],
    expected: [u8; 32],
) -> Result<(), Error> {
    let input = cbor::encode(&Value::Array(vec![
        Value::Bytes(family.to_vec()),
        Value::Integer(epoch.into()),
        Value::Bytes(key.to_vec()),
    ]))?;
    if crypto::hash("epoch-key", &input)? != expected {
        return Err(Error::Invalid("rotation key commitment mismatch"));
    }
    Ok(())
}
fn exact_map(value: &Value, count: usize) -> Result<&[(u64, Value)], Error> {
    let Value::Map(fields) = value else {
        return Err(Error::Invalid("rotation object not map"));
    };
    if fields.len() != count
        || fields
            .iter()
            .enumerate()
            .any(|(i, (key, _))| *key != i as u64 + 1)
    {
        return Err(Error::Invalid("rotation object keys invalid"));
    }
    Ok(fields)
}
fn fixed<const N: usize>(value: &Value) -> Result<[u8; N], Error> {
    byte_string(value)?
        .try_into()
        .map_err(|_| Error::Invalid("rotation byte length invalid"))
}
fn byte_string(value: &Value) -> Result<&[u8], Error> {
    let Value::Bytes(bytes) = value else {
        return Err(Error::Invalid("rotation expected bytes"));
    };
    Ok(bytes)
}
fn number(value: &Value) -> Result<u64, Error> {
    let Value::Integer(value) = value else {
        return Err(Error::Invalid("rotation expected integer"));
    };
    (*value)
        .try_into()
        .map_err(|_| Error::Invalid("rotation expected unsigned integer"))
}
