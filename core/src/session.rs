//! Core-owned verified replay of committed Family entries. This first slice
//! handles genesis and its initial manager's epoch-one data batches.

use crate::{
    batch::{self, Header},
    cbor::{self, Value},
    control::{self, Genesis},
    crypto,
    projection::{self, Outcome, Projection, VerifiedEpochKey},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    Cbor(cbor::Error),
    Control(control::Error),
    Batch(batch::Error),
    Crypto(crypto::Error),
    Projection(projection::Error),
    Invalid(&'static str),
}

impl From<cbor::Error> for Error {
    fn from(value: cbor::Error) -> Self {
        Self::Cbor(value)
    }
}
impl From<control::Error> for Error {
    fn from(value: control::Error) -> Self {
        Self::Control(value)
    }
}
impl From<batch::Error> for Error {
    fn from(value: batch::Error) -> Self {
        Self::Batch(value)
    }
}
impl From<crypto::Error> for Error {
    fn from(value: crypto::Error) -> Self {
        Self::Crypto(value)
    }
}
impl From<projection::Error> for Error {
    fn from(value: projection::Error) -> Self {
        Self::Projection(value)
    }
}

pub struct FamilySession {
    genesis: Genesis,
    relay_public_key: [u8; 32],
    epoch_key: VerifiedEpochKey,
    projection: Projection,
    next_manager_sequence: u64,
}

impl FamilySession {
    /// `genesis_bytes` come from the relay but are independently verified.
    /// An account identity or platform-supplied signer/cursor cannot replace
    /// the committed Family-specific manager credential.
    pub fn from_genesis(
        genesis_bytes: &[u8],
        relay_public_key: [u8; 32],
        epoch_key: [u8; 32],
    ) -> Result<Self, Error> {
        let genesis = control::verify_genesis(genesis_bytes, &relay_public_key)?;
        let epoch_key = genesis.verify_epoch_key(&epoch_key)?;
        let mut projection = Projection::new(genesis.family_id());
        projection.advance_control(1)?;
        Ok(Self {
            genesis,
            relay_public_key,
            epoch_key,
            projection,
            next_manager_sequence: 1,
        })
    }

    pub fn projection(&self) -> &Projection {
        &self.projection
    }
    pub fn genesis(&self) -> &Genesis {
        &self.genesis
    }

    /// Consume only a matching relay-signed acceptance and the exact
    /// committed envelope. A signed rejection or cursor gap leaves state
    /// unchanged. Membership transitions extend this session later.
    pub fn apply_initial_batch(
        &mut self,
        envelope_bytes: &[u8],
        receipt_bytes: &[u8],
    ) -> Result<Outcome, Error> {
        let signed = batch::verify_signed_envelope(
            envelope_bytes,
            &self.genesis.family_id(),
            &self.genesis.relay_id(),
            &self.genesis.manager_sign_public_key(),
        )?;
        let header = signed.header();
        self.check_header(header)?;
        let receipt = verify_accepted_receipt(receipt_bytes, &self.relay_public_key)?;
        if receipt.family_id != self.genesis.family_id()
            || receipt.relay_id != self.genesis.relay_id()
            || receipt.batch_id != header.batch_id
            || receipt.object_hash != signed.object_hash()
            || receipt.cursor
                != self
                    .projection
                    .last_cursor()
                    .checked_add(1)
                    .ok_or(Error::Invalid("cursor overflow"))?
            || receipt.control_head != self.genesis.head_hash()
            || receipt.device_sequence != header.device_sequence
            || receipt.next_expected_sequence
                != header
                    .device_sequence
                    .checked_add(1)
                    .ok_or(Error::Invalid("sequence overflow"))?
        {
            return Err(Error::Invalid(
                "accepted receipt does not match committed batch",
            ));
        }
        let outcome =
            self.projection
                .apply_authorized_signed(&signed, &self.epoch_key, receipt.cursor)?;
        self.next_manager_sequence = receipt.next_expected_sequence;
        Ok(outcome)
    }

    fn check_header(&self, header: &Header) -> Result<(), Error> {
        if header.author_device_id != self.genesis.manager_device_id()
            || header.control_head != self.genesis.head_hash()
            || header.epoch != 1
            || header.device_sequence != self.next_manager_sequence
        {
            return Err(Error::Invalid(
                "batch author, head, epoch, or sequence not authorized",
            ));
        }
        Ok(())
    }
}

struct AcceptedReceipt {
    family_id: [u8; 16],
    relay_id: [u8; 32],
    batch_id: [u8; 16],
    object_hash: [u8; 32],
    cursor: u64,
    control_head: [u8; 32],
    device_sequence: u64,
    next_expected_sequence: u64,
}

fn verify_accepted_receipt(
    bytes: &[u8],
    relay_public_key: &[u8; 32],
) -> Result<AcceptedReceipt, Error> {
    let value = cbor::decode_with_limits(
        bytes,
        cbor::Limits {
            max_bytes: 2048,
            max_depth: 8,
        },
    )?;
    let Value::Map(outer) = &value else {
        return Err(Error::Invalid("receipt must be map"));
    };
    exact_keys(outer, 2)?;
    let Value::Map(body) = &outer[0].1 else {
        return Err(Error::Invalid("receipt body must be map"));
    };
    exact_keys(body, 11)?;
    if number(&body[0].1)? != 1 || body[5].1 != Value::Bool(true) || body[9].1 != Value::Null {
        return Err(Error::Invalid("receipt is not a version-one acceptance"));
    }
    let signature = fixed::<64>(&outer[1].1)?;
    crypto::verify_cbor(
        "batch-receipt",
        &cbor::encode(&outer[0].1)?,
        relay_public_key,
        &signature,
    )?;
    let receipt = AcceptedReceipt {
        family_id: fixed(&body[1].1)?,
        relay_id: fixed(&body[2].1)?,
        batch_id: fixed(&body[3].1)?,
        object_hash: fixed(&body[4].1)?,
        cursor: number(&body[6].1)?,
        control_head: fixed(&body[7].1)?,
        device_sequence: number(&body[8].1)?,
        next_expected_sequence: number(&body[10].1)?,
    };
    if receipt.cursor == 0 || receipt.device_sequence == 0 {
        return Err(Error::Invalid("receipt cursor or sequence is zero"));
    }
    Ok(receipt)
}

fn exact_keys(entries: &[(u64, Value)], count: usize) -> Result<(), Error> {
    if entries.len() != count
        || entries
            .iter()
            .enumerate()
            .any(|(index, (key, _))| *key != index as u64 + 1)
    {
        return Err(Error::Invalid("receipt has unexpected keys"));
    }
    Ok(())
}

fn fixed<const N: usize>(value: &Value) -> Result<[u8; N], Error> {
    let Value::Bytes(bytes) = value else {
        return Err(Error::Invalid("expected byte string"));
    };
    bytes
        .as_slice()
        .try_into()
        .map_err(|_| Error::Invalid("byte length mismatch"))
}

fn number(value: &Value) -> Result<u64, Error> {
    let Value::Integer(value) = value else {
        return Err(Error::Invalid("expected integer"));
    };
    (*value)
        .try_into()
        .map_err(|_| Error::Invalid("expected unsigned integer"))
}
