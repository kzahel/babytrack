//! Version-1 signed encrypted batch bytes.
//!
//! Relay authorization, cursor assignment, duplicate IDs, and projection are
//! separate replay concerns. This module binds a batch to its header, signer,
//! Family, and plaintext operation bytes.

use crate::{
    cbor::{self, Limits, Value},
    crypto,
    operation::{self, Operation},
};

const MAX_PLAINTEXT_BYTES: usize = 256 * 1024;
const MAX_OPERATIONS: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Header {
    pub minor: u64,
    pub family_id: [u8; 16],
    pub relay_id: [u8; 32],
    pub control_head: [u8; 32],
    pub epoch: u32,
    pub batch_id: [u8; 16],
    pub author_device_id: [u8; 16],
    pub device_sequence: u64,
    pub nonce: [u8; 24],
    pub plaintext_len: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SealedBatch {
    pub header_bytes: Vec<u8>,
    pub plaintext_bytes: Vec<u8>,
    pub aad: [u8; 32],
    pub ciphertext: Vec<u8>,
    pub signature: [u8; 64],
    pub envelope_bytes: Vec<u8>,
    pub object_hash: [u8; 32],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenedBatch {
    pub header: Header,
    pub operations: Vec<Operation>,
    pub object_hash: [u8; 32],
}

/// Signature-checked and decrypted bytes. Payload parsing is separate so an
/// authorized malformed data entry can be recorded as inert at its cursor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthenticatedBatch {
    pub header: Header,
    pub object_hash: [u8; 32],
    plaintext: Vec<u8>,
}

impl AuthenticatedBatch {
    pub fn parse(&self) -> Result<OpenedBatch, Error> {
        let operations = parse_operations(&self.plaintext, &self.header)?;
        Ok(OpenedBatch {
            header: self.header.clone(),
            operations,
            object_hash: self.object_hash,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    Cbor(cbor::Error),
    Crypto(crypto::Error),
    Operation(operation::Error),
    Invalid(&'static str),
    WrongFamily,
    WrongRelay,
    UnsupportedVersion,
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

impl From<operation::Error> for Error {
    fn from(error: operation::Error) -> Self {
        Self::Operation(error)
    }
}

impl Header {
    fn value(&self) -> Value {
        Value::Map(vec![
            (
                1,
                Value::Array(vec![Value::Integer(1), Value::Integer(self.minor.into())]),
            ),
            (2, Value::Bytes(self.family_id.to_vec())),
            (3, Value::Bytes(self.relay_id.to_vec())),
            (4, Value::Bytes(self.control_head.to_vec())),
            (5, Value::Integer(self.epoch.into())),
            (6, Value::Bytes(self.batch_id.to_vec())),
            (7, Value::Bytes(self.author_device_id.to_vec())),
            (8, Value::Integer(self.device_sequence.into())),
            (9, Value::Bytes(self.nonce.to_vec())),
            (10, Value::Integer(self.plaintext_len.into())),
        ])
    }

    pub fn encode(&self) -> Result<Vec<u8>, Error> {
        if self.device_sequence == 0 || self.plaintext_len as usize > MAX_PLAINTEXT_BYTES {
            return Err(Error::Invalid(
                "batch sequence or plaintext length outside limits",
            ));
        }
        Ok(cbor::encode(&self.value())?)
    }

    fn from_value(value: &Value) -> Result<Self, Error> {
        let Value::Map(entries) = value else {
            return Err(Error::Invalid("batch header must be a map"));
        };
        if entries.len() != 10
            || entries
                .iter()
                .enumerate()
                .any(|(index, (key, _))| *key != index as u64 + 1)
        {
            return Err(Error::Invalid("batch header must have exact keys 1..10"));
        }
        let Value::Array(version) = &entries[0].1 else {
            return Err(Error::Invalid("batch version must be an array"));
        };
        if version.len() != 2 || number(&version[0])? != 1 {
            return Err(Error::UnsupportedVersion);
        }
        let header = Self {
            minor: number(&version[1])?,
            family_id: fixed_bytes(&entries[1].1)?,
            relay_id: fixed_bytes(&entries[2].1)?,
            control_head: fixed_bytes(&entries[3].1)?,
            epoch: number(&entries[4].1)?
                .try_into()
                .map_err(|_| Error::Invalid("epoch outside u32"))?,
            batch_id: fixed_bytes(&entries[5].1)?,
            author_device_id: fixed_bytes(&entries[6].1)?,
            device_sequence: number(&entries[7].1)?,
            nonce: fixed_bytes(&entries[8].1)?,
            plaintext_len: number(&entries[9].1)?
                .try_into()
                .map_err(|_| Error::Invalid("plaintext length outside u32"))?,
        };
        header.encode()?;
        Ok(header)
    }
}

/// Seal already-created canonical operation bytes with an allocated nonce and
/// sequence. The caller persists the returned envelope before any retry.
pub fn seal(
    header: &Header,
    operations: &[Vec<u8>],
    epoch_key: &[u8; 32],
    signing_seed: &[u8; 32],
) -> Result<SealedBatch, Error> {
    if operations.is_empty() || operations.len() > MAX_OPERATIONS {
        return Err(Error::Invalid("batch operation count outside limits"));
    }
    for operation in operations {
        Operation::decode_bound(operation, &header.family_id, &header.author_device_id)?;
    }
    let plaintext_value = Value::Array(operations.iter().cloned().map(Value::Bytes).collect());
    let plaintext_bytes = cbor::encode(&plaintext_value)?;
    if plaintext_bytes.len() > MAX_PLAINTEXT_BYTES
        || header.plaintext_len as usize != plaintext_bytes.len()
    {
        return Err(Error::Invalid(
            "batch plaintext length mismatch or over limit",
        ));
    }
    let header_bytes = header.encode()?;
    let aad = crypto::hash("batch-aad", &header_bytes)?;
    let ciphertext = crypto::seal_with_nonce(epoch_key, &header.nonce, &aad, &plaintext_bytes)?;
    let signature_value = signature_value(header.value(), &ciphertext)?;
    let signature_bytes = cbor::encode(&signature_value)?;
    let signature = crypto::sign_cbor("batch-envelope", &signature_bytes, signing_seed)?;
    let envelope_bytes = cbor::encode(&Value::Map(vec![
        (1, header.value()),
        (2, Value::Bytes(ciphertext.clone())),
        (3, Value::Bytes(signature.to_vec())),
    ]))?;
    let object_hash = crypto::hash("object", &envelope_bytes)?;
    Ok(SealedBatch {
        header_bytes,
        plaintext_bytes,
        aad,
        ciphertext,
        signature,
        envelope_bytes,
        object_hash,
    })
}

/// Verify and decrypt a batch under an expected Family, relay, and active
/// signer key. Authorization at the committed cursor remains the caller's job.
pub fn open_verified(
    envelope_bytes: &[u8],
    family_id: &[u8; 16],
    relay_id: &[u8; 32],
    epoch_key: &[u8; 32],
    signer_public_key: &[u8; 32],
) -> Result<OpenedBatch, Error> {
    open_authenticated(
        envelope_bytes,
        family_id,
        relay_id,
        epoch_key,
        signer_public_key,
    )?
    .parse()
}

/// Authenticate and decrypt before parsing the plaintext operation list.
pub fn open_authenticated(
    envelope_bytes: &[u8],
    family_id: &[u8; 16],
    relay_id: &[u8; 32],
    epoch_key: &[u8; 32],
    signer_public_key: &[u8; 32],
) -> Result<AuthenticatedBatch, Error> {
    let envelope = cbor::decode_with_limits(
        envelope_bytes,
        Limits {
            max_bytes: MAX_PLAINTEXT_BYTES + 2048,
            max_depth: 16,
        },
    )?;
    let Value::Map(entries) = envelope else {
        return Err(Error::Invalid("batch envelope must be a map"));
    };
    if entries.len() != 3
        || entries
            .iter()
            .enumerate()
            .any(|(index, (key, _))| *key != index as u64 + 1)
    {
        return Err(Error::Invalid("batch envelope must have exact keys 1..3"));
    }
    let header = Header::from_value(&entries[0].1)?;
    if &header.family_id != family_id {
        return Err(Error::WrongFamily);
    }
    if &header.relay_id != relay_id {
        return Err(Error::WrongRelay);
    }
    let Value::Bytes(ciphertext) = &entries[1].1 else {
        return Err(Error::Invalid("ciphertext must be bytes"));
    };
    let signature: [u8; 64] = fixed_bytes(&entries[2].1)?;
    let signature_value = signature_value(entries[0].1.clone(), ciphertext)?;
    let signature_bytes = cbor::encode(&signature_value)?;
    crypto::verify_cbor(
        "batch-envelope",
        &signature_bytes,
        signer_public_key,
        &signature,
    )?;

    let header_bytes = cbor::encode(&entries[0].1)?;
    let aad = crypto::hash("batch-aad", &header_bytes)?;
    let plaintext = crypto::open(epoch_key, &header.nonce, &aad, ciphertext)?;
    Ok(AuthenticatedBatch {
        header,
        plaintext,
        object_hash: crypto::hash("object", envelope_bytes)?,
    })
}

fn parse_operations(plaintext: &[u8], header: &Header) -> Result<Vec<Operation>, Error> {
    if plaintext.len() != header.plaintext_len as usize {
        return Err(Error::Invalid("decrypted length differs from header"));
    }
    let value = cbor::decode_with_limits(
        plaintext,
        Limits {
            max_bytes: MAX_PLAINTEXT_BYTES,
            max_depth: 16,
        },
    )?;
    let Value::Array(items) = value else {
        return Err(Error::Invalid("batch plaintext must be an array"));
    };
    if items.is_empty() || items.len() > MAX_OPERATIONS {
        return Err(Error::Invalid("batch operation count outside limits"));
    }
    let mut operations = Vec::with_capacity(items.len());
    for item in items {
        let Value::Bytes(bytes) = item else {
            return Err(Error::Invalid("batch item must be operation bytes"));
        };
        operations.push(Operation::decode_bound(
            &bytes,
            &header.family_id,
            &header.author_device_id,
        )?);
    }
    Ok(operations)
}

fn signature_value(header: Value, ciphertext: &[u8]) -> Result<Value, Error> {
    Ok(Value::Array(vec![
        header,
        Value::Bytes(crypto::hash("batch-ciphertext", ciphertext)?.to_vec()),
    ]))
}

fn number(value: &Value) -> Result<u64, Error> {
    match value {
        Value::Integer(number) => {
            u64::try_from(*number).map_err(|_| Error::Invalid("expected unsigned integer"))
        }
        _ => Err(Error::Invalid("expected unsigned integer")),
    }
}

fn fixed_bytes<const N: usize>(value: &Value) -> Result<[u8; N], Error> {
    match value {
        Value::Bytes(bytes) => bytes
            .as_slice()
            .try_into()
            .map_err(|_| Error::Invalid("invalid fixed byte length")),
        _ => Err(Error::Invalid("expected byte string")),
    }
}
