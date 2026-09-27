//! Durable public Family authority and accepted-batch cursor. This session
//! records *observed* signed history; data-key readiness and projection are
//! separate and must not be inferred from its cursor.

use crate::{
    batch,
    cbor::{self, Value},
    control_chain::{self, ControlChain},
    crypto, session,
    sqlite_store::{self, FamilyHandle, SqliteStore, VerifiedSharedEntry},
};

#[derive(Debug)]
pub enum Error {
    Batch(batch::Error),
    Control(control_chain::Error),
    Cbor(cbor::Error),
    Crypto(crypto::Error),
    Session(session::Error),
    Store(sqlite_store::Error),
    Invalid(&'static str),
}
impl From<batch::Error> for Error {
    fn from(value: batch::Error) -> Self {
        Self::Batch(value)
    }
}
impl From<control_chain::Error> for Error {
    fn from(value: control_chain::Error) -> Self {
        Self::Control(value)
    }
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
impl From<session::Error> for Error {
    fn from(value: session::Error) -> Self {
        Self::Session(value)
    }
}
impl From<sqlite_store::Error> for Error {
    fn from(value: sqlite_store::Error) -> Self {
        Self::Store(value)
    }
}

pub struct PublicHistorySession {
    family: FamilyHandle,
    chain: ControlChain,
}

impl PublicHistorySession {
    /// Pin a signed genesis or reopen the exact existing root. The Family
    /// must already have a local storage handle for this installation.
    pub fn begin(
        store: &mut SqliteStore,
        family: FamilyHandle,
        genesis_bytes: &[u8],
        relay_public_key: [u8; 32],
    ) -> Result<Self, Error> {
        let chain = ControlChain::from_genesis(genesis_bytes, relay_public_key)?;
        if chain.family_id() != family.family_id {
            return Err(Error::Invalid("genesis belongs to another Family"));
        }
        store.initialize_shared_history(
            family,
            relay_public_key,
            genesis_bytes,
            chain.head_hash(),
        )?;
        Self::resume(store, family)
    }

    /// Replay every durable byte, including all interleaved control and
    /// accepted data entries, then compare to the stored high-water pin.
    pub fn resume(store: &SqliteStore, family: FamilyHandle) -> Result<Self, Error> {
        let history = store
            .shared_history(family)?
            .ok_or(Error::Invalid("Family has no shared genesis"))?;
        let mut chain =
            ControlChain::from_genesis(&history.genesis_bytes, history.relay_public_key)?;
        if chain.family_id() != family.family_id {
            return Err(Error::Invalid("stored genesis belongs to another Family"));
        }
        for entry in history.entries {
            if entry.cursor != chain.last_global_cursor().saturating_add(1) {
                return Err(Error::Invalid("durable shared log has a cursor gap"));
            }
            match entry.kind {
                1 if entry.receipt_bytes.is_empty() => {
                    chain.apply_control(&entry.committed_bytes)?;
                }
                2 => {
                    chain.apply_public_batch(&entry.committed_bytes, &entry.receipt_bytes)?;
                }
                _ => return Err(Error::Invalid("durable shared entry kind invalid")),
            }
            if chain.last_global_cursor() != entry.cursor {
                return Err(Error::Invalid("durable entry cursor differs from receipt"));
            }
        }
        if chain.last_global_cursor() != history.pinned_cursor
            || chain.head_hash() != history.pinned_head
        {
            return Err(Error::Invalid("durable history differs from pinned head"));
        }
        Ok(Self { family, chain })
    }

    pub fn cursor(&self) -> u64 {
        self.chain.last_global_cursor()
    }

    pub fn head_hash(&self) -> [u8; 32] {
        self.chain.head_hash()
    }

    pub fn chain(&self) -> &ControlChain {
        &self.chain
    }

    /// Save only bytes named by an already pinned signed manifest. A missing
    /// object keeps readiness pending; it cannot alter observed authority.
    pub fn accept_object(
        &self,
        store: &mut SqliteStore,
        object_id: [u8; 16],
        object_bytes: &[u8],
    ) -> Result<(), Error> {
        let history = store
            .shared_history(self.family)?
            .ok_or(Error::Invalid("Family has no shared genesis"))?;
        let controls = std::iter::once(history.genesis_bytes.as_slice()).chain(
            history
                .entries
                .iter()
                .filter(|entry| entry.kind == 1)
                .map(|entry| entry.committed_bytes.as_slice()),
        );
        for bytes in controls {
            let value = cbor::decode_with_limits(
                bytes,
                cbor::Limits {
                    max_bytes: 1024 * 1024,
                    max_depth: 16,
                },
            )?;
            let Value::Map(root) = value else {
                return Err(Error::Invalid("stored control not map"));
            };
            let Value::Map(unsigned) = &root[0].1 else {
                return Err(Error::Invalid("stored unsigned control not map"));
            };
            let Value::Array(manifest) = &unsigned[9].1 else {
                return Err(Error::Invalid("stored manifest not array"));
            };
            for item in manifest {
                let Value::Array(fields) = item else {
                    return Err(Error::Invalid("stored manifest entry not array"));
                };
                if fields[1] != Value::Bytes(object_id.to_vec()) {
                    continue;
                }
                let Value::Bytes(expected_hash) = &fields[2] else {
                    return Err(Error::Invalid("stored object hash not bytes"));
                };
                if fields[3] != Value::Integer(object_bytes.len() as i128)
                    || expected_hash.as_slice() != crypto::hash("object", object_bytes)?
                {
                    return Err(Error::Invalid("object differs from signed manifest"));
                }
                let Value::Integer(kind) = fields[0] else {
                    return Err(Error::Invalid("stored object kind not integer"));
                };
                let kind: u16 = kind
                    .try_into()
                    .map_err(|_| Error::Invalid("object kind outside u16"))?;
                let Value::Bytes(transition_id) = &unsigned[4].1 else {
                    return Err(Error::Invalid("stored transition ID not bytes"));
                };
                let transition_id: [u8; 16] = transition_id
                    .as_slice()
                    .try_into()
                    .map_err(|_| Error::Invalid("stored transition ID length"))?;
                return Ok(store.store_verified_shared_object(
                    self.family,
                    transition_id,
                    kind,
                    object_id,
                    object_bytes,
                )?);
            }
        }
        Err(Error::Invalid("object ID is absent from signed history"))
    }

    pub fn accept_control(&mut self, store: &mut SqliteStore, bytes: &[u8]) -> Result<(), Error> {
        let mut candidate = self.chain.clone();
        candidate.apply_control(bytes)?;
        store.append_verified_shared_entry(
            self.family,
            VerifiedSharedEntry {
                previous_cursor: self.chain.last_global_cursor(),
                previous_head: self.chain.head_hash(),
                next_head: candidate.head_hash(),
                kind: 1,
                committed_bytes: bytes,
                receipt_bytes: &[],
                own_sequence: None,
                matching_pending: None,
            },
        )?;
        self.chain = candidate;
        Ok(())
    }

    pub fn accept_batch(
        &mut self,
        store: &mut SqliteStore,
        envelope_bytes: &[u8],
        receipt_bytes: &[u8],
    ) -> Result<(), Error> {
        let mut candidate = self.chain.clone();
        let signed = candidate.apply_public_batch(envelope_bytes, receipt_bytes)?;
        let own_sequence = (signed.header().author_device_id == self.family.device_id)
            .then_some(signed.header().device_sequence);
        let pending = if own_sequence.is_some() {
            store.pending_batch(self.family)?
        } else {
            None
        };
        let matching_pending = pending.as_ref().filter(|pending| {
            pending.envelope_bytes == envelope_bytes
                && pending.batch_id == signed.header().batch_id
                && pending.object_hash == signed.object_hash()
                && pending.sequence == signed.header().device_sequence
        });
        store.append_verified_shared_entry(
            self.family,
            VerifiedSharedEntry {
                previous_cursor: self.chain.last_global_cursor(),
                previous_head: self.chain.head_hash(),
                next_head: candidate.head_hash(),
                kind: 2,
                committed_bytes: envelope_bytes,
                receipt_bytes,
                own_sequence,
                matching_pending,
            },
        )?;
        self.chain = candidate;
        Ok(())
    }

    /// A signed stale-epoch rejection at the already verified new head ends
    /// uncertainty for this exact envelope. The local operation stays in the
    /// journal so the ready session can stage fresh nonce/ID bytes.
    pub fn reject_stale_pending(
        &self,
        store: &mut SqliteStore,
        receipt_bytes: &[u8],
    ) -> Result<(), Error> {
        let pending = store
            .pending_batch(self.family)?
            .ok_or(Error::Invalid("no uncertain local batch"))?;
        let signer = self.chain.active_signing_public(self.family.device_id)?;
        let signed = batch::verify_signed_envelope(
            &pending.envelope_bytes,
            &self.family.family_id,
            &self.chain.relay_id(),
            &signer,
        )?;
        if signed.header().batch_id != pending.batch_id
            || signed.object_hash() != pending.object_hash
            || signed.header().device_sequence != pending.sequence
            || signed.header().author_device_id != self.family.device_id
            || signed.header().epoch >= self.chain.epoch()?
            || self.chain.next_sequence_for(self.family.device_id)? != pending.sequence
        {
            return Err(Error::Invalid("pending batch is not a stale local epoch"));
        }
        let history = store
            .shared_history(self.family)?
            .ok_or(Error::Invalid("Family has no shared genesis"))?;
        let receipt = session::verify_rejected_receipt(receipt_bytes, &history.relay_public_key)?;
        if receipt.reason != 1
            || receipt.family_id != self.family.family_id
            || receipt.relay_id != self.chain.relay_id()
            || receipt.batch_id != pending.batch_id
            || receipt.object_hash != pending.object_hash
            || receipt.cursor != self.cursor()
            || receipt.control_head != self.head_hash()
            || receipt.device_sequence != pending.sequence
            || receipt.next_expected_sequence != pending.sequence
        {
            return Err(Error::Invalid("rejection differs from pinned stale batch"));
        }
        store.record_verified_rejection(
            self.family,
            &pending,
            receipt_bytes,
            self.cursor(),
            self.head_hash(),
            pending.sequence,
        )?;
        Ok(())
    }

    /// A signed sequence rejection becomes actionable only after the local
    /// verified prefix contains the competing accepted batch. Preserve the
    /// local operation and advance its next sealing sequence atomically.
    pub fn reject_conflicting_sequence_pending(
        &self,
        store: &mut SqliteStore,
        receipt_bytes: &[u8],
    ) -> Result<(), Error> {
        let pending = store
            .pending_batch(self.family)?
            .ok_or(Error::Invalid("no uncertain local batch"))?;
        let signer = self.chain.active_signing_public(self.family.device_id)?;
        let signed = batch::verify_signed_envelope(
            &pending.envelope_bytes,
            &self.family.family_id,
            &self.chain.relay_id(),
            &signer,
        )?;
        if signed.header().author_device_id != self.family.device_id
            || signed.header().batch_id != pending.batch_id
            || signed.header().device_sequence != pending.sequence
            || signed.object_hash() != pending.object_hash
        {
            return Err(Error::Invalid("pending sequence metadata differs"));
        }
        let history = store
            .shared_history(self.family)?
            .ok_or(Error::Invalid("Family has no shared genesis"))?;
        let receipt = session::verify_rejected_receipt(receipt_bytes, &history.relay_public_key)?;
        if receipt.reason != 4
            || receipt.family_id != self.family.family_id
            || receipt.relay_id != self.chain.relay_id()
            || receipt.batch_id != pending.batch_id
            || receipt.object_hash != pending.object_hash
            || receipt.device_sequence != pending.sequence
            || receipt.cursor > self.cursor()
        {
            return Err(Error::Invalid(
                "sequence rejection differs from pending batch",
            ));
        }
        let mut at_rejection =
            ControlChain::from_genesis(&history.genesis_bytes, history.relay_public_key)?;
        let mut competing = false;
        for entry in history
            .entries
            .iter()
            .filter(|entry| entry.cursor <= receipt.cursor)
        {
            match entry.kind {
                1 => at_rejection.apply_control(&entry.committed_bytes)?,
                2 => {
                    let accepted = at_rejection
                        .apply_public_batch(&entry.committed_bytes, &entry.receipt_bytes)?;
                    if accepted.header().author_device_id == self.family.device_id
                        && accepted.header().device_sequence == pending.sequence
                        && accepted.header().batch_id != pending.batch_id
                    {
                        competing = true;
                    }
                }
                _ => return Err(Error::Invalid("shared prefix entry kind invalid")),
            }
        }
        let current_next = self.chain.next_sequence_for(self.family.device_id)?;
        if !competing
            || at_rejection.last_global_cursor() != receipt.cursor
            || at_rejection.head_hash() != receipt.control_head
            || at_rejection.next_sequence_for(self.family.device_id)?
                != receipt.next_expected_sequence
            || receipt.next_expected_sequence <= pending.sequence
            || current_next < receipt.next_expected_sequence
        {
            return Err(Error::Invalid(
                "sequence conflict lacks verified accepted prefix",
            ));
        }
        store.record_verified_rejection(
            self.family,
            &pending,
            receipt_bytes,
            self.cursor(),
            self.head_hash(),
            current_next,
        )?;
        Ok(())
    }
}

/// Replay the complete local prefix, including data entries between join
/// controls, before building the next first-cohort transition.
pub fn first_join_chain(
    store: &SqliteStore,
    family: FamilyHandle,
    relay_public: [u8; 32],
    expected_controls: usize,
) -> Result<ControlChain, Error> {
    let sparse = store.enrollment_controls(family)?;
    if !sparse.is_empty() {
        if sparse.len() + 1 != expected_controls {
            return Err(Error::Invalid("sparse first join control prefix differs"));
        }
        return sparse_enrollment_chain(store, family, relay_public);
    }
    let history = store
        .shared_history(family)?
        .ok_or(Error::Invalid("genesis missing"))?;
    if history.relay_public_key != relay_public
        || history
            .entries
            .iter()
            .filter(|entry| entry.kind == 1)
            .count()
            + 1
            != expected_controls
    {
        return Err(Error::Invalid("first join control prefix differs"));
    }
    Ok(PublicHistorySession::resume(store, family)?.chain().clone())
}

/// Pending devices verify the signed control ancestry without treating
/// missing data cursors as verified. This never advances the contiguous
/// shared history high-water mark.
pub(crate) fn sparse_enrollment_chain(
    store: &SqliteStore,
    family: FamilyHandle,
    relay_public: [u8; 32],
) -> Result<ControlChain, Error> {
    let row = store
        .enrollment_attempt(family.family_id)?
        .ok_or(Error::Invalid("no enrollment attempt"))?;
    if row.family != family {
        return Err(Error::Invalid("sparse enrollment Family handle differs"));
    }
    let history = store
        .shared_history(family)?
        .ok_or(Error::Invalid("shared genesis absent"))?;
    if history.relay_public_key != relay_public || history.genesis_bytes != row.genesis_bytes {
        return Err(Error::Invalid("sparse enrollment genesis pin differs"));
    }
    let controls = store.enrollment_controls(family)?;
    if controls
        .first()
        .is_none_or(|(_, bytes)| *bytes != row.issue_bytes)
    {
        return Err(Error::Invalid("sparse enrollment issue absent"));
    }
    let mut chain = ControlChain::from_genesis(&row.genesis_bytes, relay_public)?;
    for (cursor, bytes) in controls {
        chain.apply_sparse_control(&bytes)?;
        if chain.last_global_cursor() != cursor {
            return Err(Error::Invalid(
                "sparse control cursor differs from signed receipt",
            ));
        }
    }
    Ok(chain)
}

pub fn accept_sparse_enrollment_control(
    store: &mut SqliteStore,
    family: FamilyHandle,
    relay_public: [u8; 32],
    bytes: &[u8],
) -> Result<(), Error> {
    let mut chain = sparse_enrollment_chain(store, family, relay_public)?;
    let prior = store.enrollment_controls(family)?;
    if prior.last().is_some_and(|(_, saved)| saved == bytes) {
        return Ok(());
    }
    chain.apply_sparse_control(bytes)?;
    store.append_enrollment_control(family, chain.last_global_cursor(), bytes)?;
    Ok(())
}

/// Verify an addressed object against a signed sparse control manifest.
/// The object is stored under the pinned genesis root, but it does not make
/// missing data cursors or Family keys ready.
pub fn accept_sparse_enrollment_object(
    store: &mut SqliteStore,
    family: FamilyHandle,
    relay_public: [u8; 32],
    object_id: [u8; 16],
    object_bytes: &[u8],
) -> Result<(), Error> {
    sparse_enrollment_chain(store, family, relay_public)?;
    let row = store
        .enrollment_attempt(family.family_id)?
        .ok_or(Error::Invalid("no enrollment attempt"))?;
    let controls = store.enrollment_controls(family)?;
    for bytes in std::iter::once(row.genesis_bytes.as_slice())
        .chain(controls.iter().map(|(_, bytes)| bytes.as_slice()))
    {
        let value = cbor::decode_with_limits(
            bytes,
            cbor::Limits {
                max_bytes: 1024 * 1024,
                max_depth: 16,
            },
        )?;
        let Value::Map(root) = value else {
            return Err(Error::Invalid("control not map"));
        };
        let Value::Map(unsigned) = &root[0].1 else {
            return Err(Error::Invalid("unsigned control not map"));
        };
        let Value::Array(manifest) = &unsigned[9].1 else {
            return Err(Error::Invalid("manifest not array"));
        };
        for item in manifest {
            let Value::Array(fields) = item else {
                return Err(Error::Invalid("manifest entry not array"));
            };
            if fields[1] != Value::Bytes(object_id.to_vec()) {
                continue;
            }
            let Value::Bytes(expected_hash) = &fields[2] else {
                return Err(Error::Invalid("manifest hash not bytes"));
            };
            if fields[3] != Value::Integer(object_bytes.len() as i128)
                || expected_hash.as_slice() != crypto::hash("object", object_bytes)?
            {
                return Err(Error::Invalid("object differs from sparse manifest"));
            }
            let Value::Integer(kind) = fields[0] else {
                return Err(Error::Invalid("manifest kind not integer"));
            };
            let kind = u16::try_from(kind).map_err(|_| Error::Invalid("object kind range"))?;
            let Value::Bytes(transition_id) = &unsigned[4].1 else {
                return Err(Error::Invalid("transition ID not bytes"));
            };
            let transition_id: [u8; 16] = transition_id
                .as_slice()
                .try_into()
                .map_err(|_| Error::Invalid("transition ID length"))?;
            store.store_verified_shared_object(
                family,
                transition_id,
                kind,
                object_id,
                object_bytes,
            )?;
            return Ok(());
        }
    }
    Err(Error::Invalid(
        "object absent from sparse control manifests",
    ))
}
