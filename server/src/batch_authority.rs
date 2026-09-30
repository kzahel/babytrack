//! Public-only batch verification. The relay never sees operation plaintext.

use babytrack_wire::{
    cbor::{self, Value},
    crypto,
};

#[derive(Debug)]
#[allow(dead_code)] // Error variants retain diagnostic details at the relay boundary.
pub(crate) enum Error {
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

pub(crate) struct VerifiedBatch {
    pub family_id: [u8; 16],
    pub relay_id: [u8; 32],
    pub control_head: [u8; 32],
    pub epoch: u32,
    pub batch_id: [u8; 16],
    pub author_id: [u8; 16],
    pub sequence: u64,
    pub object_hash: [u8; 32],
}

pub(crate) fn verify(
    bytes: &[u8],
    family_id: [u8; 16],
    relay_id: [u8; 32],
    signing_key: [u8; 32],
) -> Result<VerifiedBatch, Error> {
    let value = cbor::decode_with_limits(
        bytes,
        cbor::Limits {
            max_bytes: 256 * 1024 + 2048,
            max_depth: 16,
        },
    )?;
    let outer = exact_map(&value, 3)?;
    let header = exact_map(&outer[0].1, 10)?;
    let Value::Array(version) = &header[0].1 else {
        return Err(Error::Invalid("batch version not array"));
    };
    if version.len() != 2 || version[0] != Value::Integer(1) {
        return Err(Error::Invalid("batch major version"));
    }
    let _minor = number(&version[1])?;
    let actual_family = fixed::<16>(&header[1].1)?;
    let actual_relay = fixed::<32>(&header[2].1)?;
    if actual_family != family_id || actual_relay != relay_id {
        return Err(Error::Invalid("batch Family or relay differs"));
    }
    let control_head = fixed::<32>(&header[3].1)?;
    let epoch: u32 = number(&header[4].1)?
        .try_into()
        .map_err(|_| Error::Invalid("epoch range"))?;
    let batch_id = fixed::<16>(&header[5].1)?;
    let author_id = fixed::<16>(&header[6].1)?;
    let sequence = number(&header[7].1)?;
    let _nonce = fixed::<24>(&header[8].1)?;
    let plain_len: usize = number(&header[9].1)?
        .try_into()
        .map_err(|_| Error::Invalid("length range"))?;
    if !is_v4(&family_id)
        || !is_v4(&batch_id)
        || !is_v4(&author_id)
        || epoch == 0
        || sequence == 0
        || plain_len == 0
        || plain_len > 256 * 1024
    {
        return Err(Error::Invalid("batch header constraints"));
    }
    let Value::Bytes(ciphertext) = &outer[1].1 else {
        return Err(Error::Invalid("ciphertext not bytes"));
    };
    if ciphertext.len() != plain_len + 16 {
        return Err(Error::Invalid("batch ciphertext length"));
    }
    let signature = fixed::<64>(&outer[2].1)?;
    let signed = cbor::encode(&Value::Array(vec![
        outer[0].1.clone(),
        Value::Bytes(crypto::hash("batch-ciphertext", ciphertext)?.to_vec()),
    ]))?;
    crypto::verify_cbor("batch-envelope", &signed, &signing_key, &signature)?;
    Ok(VerifiedBatch {
        family_id,
        relay_id,
        control_head,
        epoch,
        batch_id,
        author_id,
        sequence,
        object_hash: crypto::hash("object", bytes)?,
    })
}

pub(crate) fn claimed_identity(bytes: &[u8]) -> Result<([u8; 16], [u8; 16]), Error> {
    let value = cbor::decode_with_limits(
        bytes,
        cbor::Limits {
            max_bytes: 256 * 1024 + 2048,
            max_depth: 16,
        },
    )?;
    let outer = exact_map(&value, 3)?;
    let header = exact_map(&outer[0].1, 10)?;
    Ok((fixed::<16>(&header[5].1)?, fixed::<16>(&header[6].1)?))
}

fn exact_map(value: &Value, n: usize) -> Result<&[(u64, Value)], Error> {
    let Value::Map(fields) = value else {
        return Err(Error::Invalid("expected map"));
    };
    if fields.len() != n
        || fields
            .iter()
            .enumerate()
            .any(|(i, (k, _))| *k != i as u64 + 1)
    {
        return Err(Error::Invalid("map keys"));
    }
    Ok(fields)
}
fn fixed<const N: usize>(value: &Value) -> Result<[u8; N], Error> {
    let Value::Bytes(bytes) = value else {
        return Err(Error::Invalid("expected bytes"));
    };
    bytes
        .as_slice()
        .try_into()
        .map_err(|_| Error::Invalid("bytes length"))
}
fn number(value: &Value) -> Result<u64, Error> {
    let Value::Integer(n) = value else {
        return Err(Error::Invalid("expected integer"));
    };
    (*n).try_into()
        .map_err(|_| Error::Invalid("unsigned integer"))
}
fn is_v4(id: &[u8; 16]) -> bool {
    id[6] >> 4 == 4 && id[8] & 0xc0 == 0x80
}
