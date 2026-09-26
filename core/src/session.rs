//! Core-owned verified replay of committed Family entries. This first slice
//! handles genesis and its initial manager's epoch-one data batches.

#[cfg(not(target_arch = "wasm32"))]
use crate::sqlite_store::{self, FamilyHandle, PreparedBatch, SqliteStore};
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
    #[cfg(not(target_arch = "wasm32"))]
    Store(String),
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
#[cfg(not(target_arch = "wasm32"))]
impl From<sqlite_store::Error> for Error {
    fn from(value: sqlite_store::Error) -> Self {
        Self::Store(format!("{value:?}"))
    }
}

#[derive(Clone)]
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

    #[cfg(not(target_arch = "wasm32"))]
    pub fn resume_from_store(
        genesis_bytes: &[u8],
        relay_public_key: [u8; 32],
        epoch_key: [u8; 32],
        store: &SqliteStore,
        family: FamilyHandle,
    ) -> Result<Self, Error> {
        let mut session = Self::from_genesis(genesis_bytes, relay_public_key, epoch_key)?;
        session.check_family_handle(family)?;
        for accepted in store.accepted_local_batches(family)? {
            let outcome =
                session.apply_initial_batch(&accepted.envelope_bytes, &accepted.receipt_bytes)?;
            if !matches!(outcome, Outcome::Applied) {
                return Err(Error::Invalid("stored own batch became inert"));
            }
        }
        Ok(session)
    }

    /// A validated local operation is staged by SQLite with a fresh nonce,
    /// or returns an existing uncertain batch byte-for-byte.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn stage_next_local(
        &self,
        store: &mut SqliteStore,
        family: FamilyHandle,
        signing_seed: &[u8; 32],
    ) -> Result<PreparedBatch, Error> {
        self.check_family_handle(family)?;
        if crypto::signing_public_key(signing_seed) != self.genesis.manager_sign_public_key() {
            return Err(Error::Invalid(
                "local signing key is not active manager key",
            ));
        }
        let pending = store.stage_next_batch(
            family,
            self.genesis.relay_id(),
            self.genesis.head_hash(),
            1,
            &self.epoch_key.bytes,
            signing_seed,
        )?;
        if pending.sequence != self.next_manager_sequence {
            return Err(Error::Invalid(
                "outbox sequence differs from verified history",
            ));
        }
        self.check_pending_metadata(&pending)?;
        Ok(pending)
    }

    /// Confirm only after core verification. On a storage error, the in-memory
    /// session is unchanged; the pending batch and its bytes remain durable.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn confirm_staged_local(
        &mut self,
        store: &mut SqliteStore,
        family: FamilyHandle,
        receipt_bytes: &[u8],
    ) -> Result<(), Error> {
        self.check_family_handle(family)?;
        let pending = store
            .pending_batch(family)?
            .ok_or(Error::Invalid("no pending batch"))?;
        self.check_pending_metadata(&pending)?;
        let mut candidate = self.clone();
        let outcome = candidate.apply_initial_batch(&pending.envelope_bytes, receipt_bytes)?;
        if !matches!(outcome, Outcome::Applied) {
            return Err(Error::Invalid("own staged batch is inert"));
        }
        store.record_verified_acceptance(
            family,
            &pending,
            receipt_bytes,
            candidate.projection.last_cursor(),
        )?;
        *self = candidate;
        Ok(())
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn check_family_handle(&self, family: FamilyHandle) -> Result<(), Error> {
        if family.family_id != self.genesis.family_id()
            || family.device_id != self.genesis.manager_device_id()
        {
            return Err(Error::Invalid(
                "local Family handle differs from verified manager",
            ));
        }
        Ok(())
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn check_pending_metadata(&self, pending: &PreparedBatch) -> Result<(), Error> {
        let signed = batch::verify_signed_envelope(
            &pending.envelope_bytes,
            &self.genesis.family_id(),
            &self.genesis.relay_id(),
            &self.genesis.manager_sign_public_key(),
        )?;
        self.check_header(signed.header())?;
        if signed.header().batch_id != pending.batch_id
            || signed.header().device_sequence != pending.sequence
            || signed.object_hash() != pending.object_hash
        {
            return Err(Error::Invalid(
                "stored outbox metadata differs from signed envelope",
            ));
        }
        Ok(())
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

pub(crate) struct AcceptedReceipt {
    pub(crate) family_id: [u8; 16],
    pub(crate) relay_id: [u8; 32],
    pub(crate) batch_id: [u8; 16],
    pub(crate) object_hash: [u8; 32],
    pub(crate) cursor: u64,
    pub(crate) control_head: [u8; 32],
    pub(crate) device_sequence: u64,
    pub(crate) next_expected_sequence: u64,
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) struct RejectedReceipt {
    pub(crate) family_id: [u8; 16],
    pub(crate) relay_id: [u8; 32],
    pub(crate) batch_id: [u8; 16],
    pub(crate) object_hash: [u8; 32],
    pub(crate) cursor: u64,
    pub(crate) control_head: [u8; 32],
    pub(crate) device_sequence: u64,
    pub(crate) reason: u16,
    pub(crate) next_expected_sequence: u64,
}

struct DecodedBatchReceipt {
    family_id: [u8; 16],
    relay_id: [u8; 32],
    batch_id: [u8; 16],
    object_hash: [u8; 32],
    accepted: bool,
    cursor: u64,
    control_head: [u8; 32],
    device_sequence: u64,
    reason: Option<u16>,
    next_expected_sequence: u64,
}

pub(crate) fn verify_accepted_receipt(
    bytes: &[u8],
    relay_public_key: &[u8; 32],
) -> Result<AcceptedReceipt, Error> {
    let receipt = decode_batch_receipt(bytes, relay_public_key)?;
    if !receipt.accepted || receipt.reason.is_some() {
        return Err(Error::Invalid("receipt is not an acceptance"));
    }
    Ok(AcceptedReceipt {
        family_id: receipt.family_id,
        relay_id: receipt.relay_id,
        batch_id: receipt.batch_id,
        object_hash: receipt.object_hash,
        cursor: receipt.cursor,
        control_head: receipt.control_head,
        device_sequence: receipt.device_sequence,
        next_expected_sequence: receipt.next_expected_sequence,
    })
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn verify_rejected_receipt(
    bytes: &[u8],
    relay_public_key: &[u8; 32],
) -> Result<RejectedReceipt, Error> {
    let receipt = decode_batch_receipt(bytes, relay_public_key)?;
    let reason = receipt
        .reason
        .ok_or(Error::Invalid("receipt is not a rejection"))?;
    if receipt.accepted {
        return Err(Error::Invalid("receipt is not a rejection"));
    }
    Ok(RejectedReceipt {
        family_id: receipt.family_id,
        relay_id: receipt.relay_id,
        batch_id: receipt.batch_id,
        object_hash: receipt.object_hash,
        cursor: receipt.cursor,
        control_head: receipt.control_head,
        device_sequence: receipt.device_sequence,
        reason,
        next_expected_sequence: receipt.next_expected_sequence,
    })
}

fn decode_batch_receipt(
    bytes: &[u8],
    relay_public_key: &[u8; 32],
) -> Result<DecodedBatchReceipt, Error> {
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
    if number(&body[0].1)? != 1 {
        return Err(Error::Invalid("receipt version mismatch"));
    }
    let signature = fixed::<64>(&outer[1].1)?;
    crypto::verify_cbor(
        "batch-receipt",
        &cbor::encode(&outer[0].1)?,
        relay_public_key,
        &signature,
    )?;
    let Value::Bool(accepted) = body[5].1 else {
        return Err(Error::Invalid("receipt acceptance is not boolean"));
    };
    let reason = match body[9].1 {
        Value::Null => None,
        Value::Integer(value) if (1..=5).contains(&value) => Some(value as u16),
        _ => return Err(Error::Invalid("receipt reason invalid")),
    };
    if accepted != reason.is_none() {
        return Err(Error::Invalid("receipt acceptance and reason disagree"));
    }
    let receipt = DecodedBatchReceipt {
        family_id: fixed(&body[1].1)?,
        relay_id: fixed(&body[2].1)?,
        batch_id: fixed(&body[3].1)?,
        object_hash: fixed(&body[4].1)?,
        accepted,
        cursor: number(&body[6].1)?,
        control_head: fixed(&body[7].1)?,
        device_sequence: number(&body[8].1)?,
        reason,
        next_expected_sequence: number(&body[10].1)?,
    };
    if receipt.cursor == 0 || receipt.device_sequence == 0 || receipt.next_expected_sequence == 0 {
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
