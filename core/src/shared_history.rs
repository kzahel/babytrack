//! Durable public Family authority and accepted-batch cursor. This session
//! records *observed* signed history; data-key readiness and projection are
//! separate and must not be inferred from its cursor.

use crate::{
    batch,
    cbor::{self, Value},
    control,
    control_chain::{self, ControlChain},
    crypto,
    removal_probe::{self, RemovalControlProbe},
    session,
    sqlite_store::{self, FamilyHandle, SavedRemoval, SqliteStore, VerifiedSharedEntry},
    sync_wire,
};

#[derive(Debug)]
pub enum Error {
    Batch(batch::Error),
    Control(control_chain::Error),
    ControlShape(control::Error),
    Cbor(cbor::Error),
    Crypto(crypto::Error),
    Session(session::Error),
    Store(sqlite_store::Error),
    Wire(sync_wire::Error),
    RemovalProbe(removal_probe::Error),
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
impl From<control::Error> for Error {
    fn from(value: control::Error) -> Self {
        Self::ControlShape(value)
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
impl From<sync_wire::Error> for Error {
    fn from(value: sync_wire::Error) -> Self {
        Self::Wire(value)
    }
}
impl From<removal_probe::Error> for Error {
    fn from(value: removal_probe::Error) -> Self {
        Self::RemovalProbe(value)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PendingBatchResult {
    NoPending,
    Unresolved,
    AcceptedAhead,
    Rebased,
    Blocked,
}

pub use removal_probe::VerifiedRemovalProof;

impl From<VerifiedRemovalProof> for SavedRemoval {
    fn from(value: VerifiedRemovalProof) -> Self {
        Self {
            transition_id: value.transition_id,
            cursor: value.cursor,
            source_cursor: value.source_cursor,
            known_gap: value.known_gap,
            committed_bytes: value.committed_bytes,
        }
    }
}

pub struct PublicHistorySession {
    family: FamilyHandle,
    chain: ControlChain,
}

impl PublicHistorySession {
    /// Classify an uncertain exact upload after removal without changing the
    /// source outbox or needing ciphertext that the credential cannot read.
    /// 0 means no staged batch, 1 unknown, 2 accepted, 3 rejected.
    pub fn inspect_removed_pending_result(
        &self,
        store: &SqliteStore,
        removal: &SavedRemoval,
        result_bytes: &[u8],
    ) -> Result<u8, Error> {
        if store.saved_removal(self.family)?.as_ref() != Some(removal)
            || removal.source_cursor != self.cursor()
            || removal.cursor <= removal.source_cursor
        {
            return Err(Error::Invalid("removed result needs saved cutover proof"));
        }
        let Some(pending) = store.pending_batch(self.family)? else {
            return Ok(0);
        };
        let signer = self.chain.active_signing_public(self.family.device_id)?;
        let signed = batch::verify_signed_envelope(
            &pending.envelope_bytes,
            &self.family.family_id,
            &self.chain.relay_id(),
            &signer,
        )?;
        let header = signed.header();
        if header.batch_id != pending.batch_id
            || header.device_sequence != pending.sequence
            || signed.object_hash() != pending.object_hash
            || header.author_device_id != self.family.device_id
        {
            return Err(Error::Invalid("saved pending envelope identity differs"));
        }
        let result = sync_wire::BatchResult::decode(result_bytes)?;
        let Some(receipt_bytes) = result.receipt_bytes else {
            return Ok(1);
        };
        let history = store
            .shared_history(self.family)?
            .ok_or(Error::Invalid("Family has no shared genesis"))?;
        if let Ok(accepted) =
            session::verify_accepted_receipt(&receipt_bytes, &history.relay_public_key)
        {
            if accepted.family_id == self.family.family_id
                && accepted.relay_id == self.chain.relay_id()
                && accepted.batch_id == pending.batch_id
                && accepted.object_hash == pending.object_hash
                && accepted.device_sequence == pending.sequence
                && accepted.control_head == header.control_head
                && accepted.next_expected_sequence
                    == pending
                        .sequence
                        .checked_add(1)
                        .ok_or(Error::Invalid("pending sequence overflow"))?
                && accepted.cursor > self.cursor()
                && accepted.cursor < removal.cursor
            {
                return Ok(2);
            }
            return Err(Error::Invalid(
                "accepted removal result differs from saved batch",
            ));
        }
        let rejected = session::verify_rejected_receipt(&receipt_bytes, &history.relay_public_key)?;
        if rejected.family_id != self.family.family_id
            || rejected.relay_id != self.chain.relay_id()
            || rejected.batch_id != pending.batch_id
            || rejected.object_hash != pending.object_hash
            || rejected.device_sequence != pending.sequence
            || rejected.next_expected_sequence != pending.sequence
            || rejected.reason != 2
            || rejected.cursor < removal.cursor
            || rejected.control_head != crypto::hash("control-head", &removal.committed_bytes)?
        {
            return Err(Error::Invalid(
                "rejected removal result differs from saved batch",
            ));
        }
        Ok(3)
    }

    pub fn save_removed_control_page(
        &self,
        store: &mut SqliteStore,
        page_bytes: &[u8],
    ) -> Result<Option<SavedRemoval>, Error> {
        let Some(proof) = self.verify_removed_control_page(page_bytes)? else {
            return Ok(None);
        };
        let saved = SavedRemoval::from(proof);
        store.save_verified_removal(self.family, &saved)?;
        Ok(Some(saved))
    }
    /// A removed credential can fetch signed public controls after its data
    /// reads have been revoked. Verify their ancestry without advancing the
    /// contiguous data pin or claiming any skipped batches were downloaded.
    pub fn verify_removed_control_page(
        &self,
        page_bytes: &[u8],
    ) -> Result<Option<VerifiedRemovalProof>, Error> {
        verify_removed_control_page_for(&self.chain, self.family, self.cursor(), page_bytes)
    }
    pub fn pending_batch_id(&self, store: &SqliteStore) -> Result<Option<[u8; 16]>, Error> {
        Ok(store
            .pending_batch(self.family)?
            .map(|batch| batch.batch_id))
    }

    /// A result read is only a hint until its embedded relay receipt verifies
    /// against the exact durable envelope. Acceptance still needs the full
    /// contiguous log; a supported rejection may clear uncertainty only after
    /// the required new authority prefix is locally verified.
    pub fn resolve_pending_result(
        &self,
        store: &mut SqliteStore,
        result_bytes: &[u8],
    ) -> Result<PendingBatchResult, Error> {
        let Some(pending) = store.pending_batch(self.family)? else {
            return Ok(PendingBatchResult::NoPending);
        };
        let result = sync_wire::BatchResult::decode(result_bytes)?;
        let Some(receipt_bytes) = result.receipt_bytes else {
            return Ok(PendingBatchResult::Unresolved);
        };
        let history = store
            .shared_history(self.family)?
            .ok_or(Error::Invalid("Family has no shared genesis"))?;
        if let Ok(accepted) =
            session::verify_accepted_receipt(&receipt_bytes, &history.relay_public_key)
        {
            if accepted.family_id != self.family.family_id
                || accepted.relay_id != self.chain.relay_id()
                || accepted.batch_id != pending.batch_id
                || accepted.object_hash != pending.object_hash
                || accepted.device_sequence != pending.sequence
                || accepted.cursor <= self.cursor()
            {
                return Err(Error::Invalid(
                    "accepted result differs from pending batch or verified prefix",
                ));
            }
            return Ok(PendingBatchResult::AcceptedAhead);
        }
        let rejected = session::verify_rejected_receipt(&receipt_bytes, &history.relay_public_key)?;
        if rejected.family_id != self.family.family_id
            || rejected.relay_id != self.chain.relay_id()
            || rejected.batch_id != pending.batch_id
            || rejected.object_hash != pending.object_hash
            || rejected.device_sequence != pending.sequence
        {
            return Err(Error::Invalid("rejected result differs from pending batch"));
        }
        match rejected.reason {
            1 => self.reject_stale_pending(store, &receipt_bytes)?,
            4 => self.reject_conflicting_sequence_pending(store, &receipt_bytes)?,
            _ => return Ok(PendingBatchResult::Blocked),
        }
        Ok(PendingBatchResult::Rebased)
    }
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

    /// A signed stale-epoch rejection ends uncertainty only after its named
    /// authority prefix has been verified, even if later entries arrived.
    /// The local operation stays available for a fresh epoch batch.
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
            || receipt.cursor > self.cursor()
            || receipt.device_sequence != pending.sequence
        {
            return Err(Error::Invalid("rejection differs from pinned stale batch"));
        }
        let mut at_rejection =
            ControlChain::from_genesis(&history.genesis_bytes, history.relay_public_key)?;
        for entry in history
            .entries
            .iter()
            .filter(|entry| entry.cursor <= receipt.cursor)
        {
            match entry.kind {
                1 => at_rejection.apply_control(&entry.committed_bytes)?,
                2 => {
                    at_rejection
                        .apply_public_batch(&entry.committed_bytes, &entry.receipt_bytes)?;
                }
                _ => return Err(Error::Invalid("shared prefix entry kind invalid")),
            }
        }
        let current_next = self.chain.next_sequence_for(self.family.device_id)?;
        if at_rejection.last_global_cursor() != receipt.cursor
            || at_rejection.head_hash() != receipt.control_head
            || signed.header().epoch >= at_rejection.epoch()?
            || at_rejection.next_sequence_for(self.family.device_id)?
                != receipt.next_expected_sequence
            || current_next < receipt.next_expected_sequence
        {
            return Err(Error::Invalid(
                "stale rejection lacks verified rotated prefix",
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

/// Verify a removed device's sparse control page against its last contiguous
/// data pin. Shared by native storage and browser adapters; skipped batches
/// are never treated as locally downloaded.
pub fn verify_removed_control_page_for(
    source: &ControlChain,
    family: FamilyHandle,
    source_cursor: u64,
    page_bytes: &[u8],
) -> Result<Option<VerifiedRemovalProof>, Error> {
    if source.last_global_cursor() != source_cursor || source.family_id() != family.family_id {
        return Err(Error::Invalid(
            "removal source differs from saved public pin",
        ));
    }
    let mut probe =
        RemovalControlProbe::new(source.clone(), family.device_id).map_err(Error::RemovalProbe)?;
    Ok(probe
        .accept(page_bytes, source_cursor)
        .map_err(Error::RemovalProbe)?
        .0)
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

/// Reconstruct an earlier verified control prefix after later entries have
/// committed. The full durable log is verified first, then the saved prefix
/// is replayed with every interleaved batch and its receipt.
pub fn first_join_prefix_chain(
    store: &SqliteStore,
    family: FamilyHandle,
    relay_public: [u8; 32],
    expected_controls: usize,
) -> Result<ControlChain, Error> {
    let sparse = store.enrollment_controls(family)?;
    if !sparse.is_empty() {
        sparse_enrollment_chain(store, family, relay_public)?;
        let row = store
            .enrollment_attempt(family.family_id)?
            .ok_or(Error::Invalid("enrollment attempt missing"))?;
        if expected_controls == 0 || sparse.len() + 1 < expected_controls {
            return Err(Error::Invalid("sparse first join prefix absent"));
        }
        let mut chain = ControlChain::from_genesis(&row.genesis_bytes, relay_public)?;
        for (_, bytes) in sparse.iter().take(expected_controls - 1) {
            chain.apply_sparse_control(bytes)?;
        }
        return Ok(chain);
    }
    PublicHistorySession::resume(store, family)?;
    let history = store
        .shared_history(family)?
        .ok_or(Error::Invalid("genesis missing"))?;
    if history.relay_public_key != relay_public || expected_controls == 0 {
        return Err(Error::Invalid("first join prefix relay mismatch"));
    }
    let mut chain = ControlChain::from_genesis(&history.genesis_bytes, relay_public)?;
    let mut controls = 1;
    for entry in history.entries {
        if controls == expected_controls {
            break;
        }
        match entry.kind {
            1 => {
                chain.apply_control(&entry.committed_bytes)?;
                controls += 1;
            }
            2 => {
                chain.apply_public_batch(&entry.committed_bytes, &entry.receipt_bytes)?;
            }
            _ => return Err(Error::Invalid("first join prefix entry kind")),
        }
    }
    if controls != expected_controls {
        return Err(Error::Invalid("first join control prefix absent"));
    }
    Ok(chain)
}

/// Reconstruct a previously verified public head, including interleaved
/// accepted batches. A durable prepared control can be checked against the
/// exact authority state it originally named after newer controls arrive.
pub(crate) fn chain_at_head(
    store: &SqliteStore,
    family: FamilyHandle,
    head: [u8; 32],
) -> Result<ControlChain, Error> {
    let sparse = store.enrollment_controls(family)?;
    if !sparse.is_empty() {
        let history = store
            .shared_history(family)?
            .ok_or(Error::Invalid("shared history absent"))?;
        sparse_enrollment_chain(store, family, history.relay_public_key)?;
        let mut chain =
            ControlChain::from_genesis(&history.genesis_bytes, history.relay_public_key)?;
        if chain.head_hash() == head {
            return Ok(chain);
        }
        for (_, bytes) in sparse {
            chain.apply_sparse_control(&bytes)?;
            if chain.head_hash() == head {
                return Ok(chain);
            }
        }
        // After enrollment, this same device can manage later invitations.
        // Their prepared heads occur beyond the saved join-only sparse prefix.
        // Continue into the fully verified contiguous history below.
    }
    PublicHistorySession::resume(store, family)?;
    let history = store
        .shared_history(family)?
        .ok_or(Error::Invalid("shared history absent"))?;
    let mut chain = ControlChain::from_genesis(&history.genesis_bytes, history.relay_public_key)?;
    if chain.head_hash() == head {
        return Ok(chain);
    }
    for entry in history.entries {
        if entry.kind == 1 {
            chain.apply_control(&entry.committed_bytes)?;
            if chain.head_hash() == head {
                return Ok(chain);
            }
        } else {
            chain.apply_public_batch(&entry.committed_bytes, &entry.receipt_bytes)?;
        }
    }
    Err(Error::Invalid(
        "prepared control prior head absent from verified history",
    ))
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
    if !controls.iter().any(|(_, bytes)| *bytes == row.issue_bytes) {
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
    // An exact earlier control remains an idempotent retry after subsequent
    // controls have been verified and stored.
    if prior.iter().any(|(_, saved)| saved == bytes) {
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
