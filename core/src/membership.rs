//! Encrypted membership object must repeat the exact committed public
//! transition. Only a signed control entry constructs this check.

use crate::{
    cbor::{self, Value},
    crypto,
    projection::VerifiedEpochKey,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    Cbor(cbor::Error),
    Crypto(crypto::Error),
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedMembership {
    pub(crate) family_id: [u8; 16],
    pub(crate) epoch: u32,
    pub(crate) object_id: [u8; 16],
    pub(crate) object_hash: [u8; 32],
    pub(crate) core_hash: [u8; 32],
    pub(crate) transition_id: [u8; 16],
    pub(crate) prior_head: [u8; 32],
    pub(crate) state_hash: [u8; 32],
    pub(crate) delta: Value,
}

impl VerifiedMembership {
    pub fn object_id(&self) -> [u8; 16] {
        self.object_id
    }

    pub fn verify(&self, object_bytes: &[u8], key: &VerifiedEpochKey) -> Result<(), Error> {
        if key.family_id != self.family_id || key.epoch != self.epoch {
            return Err(Error::Invalid("membership key is from another epoch"));
        }
        if crypto::hash("object", object_bytes)? != self.object_hash {
            return Err(Error::Invalid("membership object hash mismatch"));
        }
        let object = cbor::decode_with_limits(
            object_bytes,
            cbor::Limits {
                max_bytes: 1024 * 1024,
                max_depth: 16,
            },
        )?;
        let Value::Map(fields) = object else {
            return Err(Error::Invalid("membership object not map"));
        };
        if fields.len() != 3
            || fields
                .iter()
                .enumerate()
                .any(|(i, (key, _))| *key != i as u64 + 1)
            || fields[0].1 != Value::Integer(1)
        {
            return Err(Error::Invalid("membership object keys or version invalid"));
        }
        let Value::Bytes(nonce) = &fields[1].1 else {
            return Err(Error::Invalid("membership nonce not bytes"));
        };
        let nonce: [u8; 24] = nonce
            .as_slice()
            .try_into()
            .map_err(|_| Error::Invalid("membership nonce length invalid"))?;
        let Value::Bytes(ciphertext) = &fields[2].1 else {
            return Err(Error::Invalid("membership ciphertext not bytes"));
        };
        let aad = crypto::hash("membership-aad", &self.core_hash)?;
        let plaintext = crypto::open(&key.bytes, &nonce, &aad, ciphertext)?;
        let value = cbor::decode_with_limits(
            &plaintext,
            cbor::Limits {
                max_bytes: 1024 * 1024,
                max_depth: 16,
            },
        )?;
        let expected = Value::Map(vec![
            (1, Value::Bytes(self.transition_id.to_vec())),
            (2, Value::Bytes(self.prior_head.to_vec())),
            (3, Value::Bytes(self.state_hash.to_vec())),
            (4, Value::Integer(self.epoch.into())),
            (5, self.delta.clone()),
        ]);
        if value != expected {
            return Err(Error::Invalid(
                "encrypted membership disagrees with public transition",
            ));
        }
        Ok(())
    }
}
