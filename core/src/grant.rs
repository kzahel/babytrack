//! Recipient-side admission grant verification against committed authority.

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
pub struct VerifiedAdmissionGrant {
    pub(crate) family_id: [u8; 16],
    pub(crate) epoch: u32,
    pub(crate) epoch_commitment: [u8; 32],
    pub(crate) core_hash: [u8; 32],
    pub(crate) invitation_id: [u8; 16],
    pub(crate) device_id: [u8; 16],
    pub(crate) role: u8,
    pub(crate) agree_public: [u8; 32],
    pub(crate) key_version: u32,
    pub(crate) grant_id: [u8; 16],
    pub(crate) object_hash: [u8; 32],
}

impl VerifiedAdmissionGrant {
    pub fn grant_id(&self) -> [u8; 16] {
        self.grant_id
    }

    pub fn open(
        &self,
        object_bytes: &[u8],
        recipient_private: &[u8; 32],
    ) -> Result<VerifiedEpochKey, Error> {
        if hpke::public_key_from_private(recipient_private)? != self.agree_public {
            return Err(Error::Invalid(
                "grant agreement key differs from admitted device",
            ));
        }
        if crypto::hash("object", object_bytes)? != self.object_hash {
            return Err(Error::Invalid("grant object hash mismatch"));
        }
        let value = cbor::decode_with_limits(
            object_bytes,
            cbor::Limits {
                max_bytes: 1024 * 1024,
                max_depth: 16,
            },
        )?;
        let fields = exact_map(&value, 9)?;
        if number(&fields[0].1)? != 1
            || fixed::<16>(&fields[1].1)? != self.grant_id
            || number(&fields[2].1)? != 1
            || fixed::<16>(&fields[3].1)? != self.device_id
            || number(&fields[4].1)? != self.key_version as u64
            || fixed::<32>(&fields[6].1)? != self.core_hash
        {
            return Err(Error::Invalid("admission grant context mismatch"));
        }
        let Value::Array(suite) = &fields[5].1 else {
            return Err(Error::Invalid("grant suite not array"));
        };
        if suite != &[Value::Integer(32), Value::Integer(1), Value::Integer(3)] {
            return Err(Error::Invalid("grant HPKE suite mismatch"));
        }
        let context = cbor::encode(&Value::Array(vec![
            Value::Bytes(self.core_hash.to_vec()),
            Value::Bytes(self.invitation_id.to_vec()),
            Value::Bytes(self.device_id.to_vec()),
            Value::Integer(self.role.into()),
            Value::Bytes(self.agree_public.to_vec()),
            Value::Integer(self.key_version.into()),
            Value::Integer(self.epoch.into()),
            Value::Bytes(self.epoch_commitment.to_vec()),
        ]))?;
        let info = crypto::hash("grant-info", &context)?;
        let plaintext = hpke::open(
            recipient_private,
            &fixed::<32>(&fields[7].1)?,
            &info,
            &context,
            byte_string(&fields[8].1)?,
        )?;
        let decoded = cbor::decode_with_limits(
            &plaintext,
            cbor::Limits {
                max_bytes: 128,
                max_depth: 3,
            },
        )?;
        let Value::Array(parts) = decoded else {
            return Err(Error::Invalid("grant plaintext not array"));
        };
        if parts.len() != 3 || number(&parts[0])? != 1 || number(&parts[1])? != self.epoch as u64 {
            return Err(Error::Invalid("grant plaintext version or epoch mismatch"));
        }
        let key = fixed::<32>(&parts[2])?;
        let commitment_input = cbor::encode(&Value::Array(vec![
            Value::Bytes(self.family_id.to_vec()),
            Value::Integer(self.epoch.into()),
            Value::Bytes(key.to_vec()),
        ]))?;
        if crypto::hash("epoch-key", &commitment_input)? != self.epoch_commitment {
            return Err(Error::Invalid("grant key differs from committed epoch"));
        }
        Ok(VerifiedEpochKey {
            family_id: self.family_id,
            epoch: self.epoch,
            bytes: key,
        })
    }
}

fn exact_map(value: &Value, count: usize) -> Result<&[(u64, Value)], Error> {
    let Value::Map(fields) = value else {
        return Err(Error::Invalid("grant not map"));
    };
    if fields.len() != count
        || fields
            .iter()
            .enumerate()
            .any(|(i, (key, _))| *key != i as u64 + 1)
    {
        return Err(Error::Invalid("grant keys invalid"));
    }
    Ok(fields)
}
fn fixed<const N: usize>(value: &Value) -> Result<[u8; N], Error> {
    byte_string(value)?
        .try_into()
        .map_err(|_| Error::Invalid("grant byte length invalid"))
}
fn byte_string(value: &Value) -> Result<&[u8], Error> {
    let Value::Bytes(bytes) = value else {
        return Err(Error::Invalid("grant expected bytes"));
    };
    Ok(bytes)
}
fn number(value: &Value) -> Result<u64, Error> {
    let Value::Integer(value) = value else {
        return Err(Error::Invalid("grant expected integer"));
    };
    (*value)
        .try_into()
        .map_err(|_| Error::Invalid("grant expected unsigned integer"))
}
