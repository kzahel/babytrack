//! Durable public Family authority and accepted-batch cursor. This session
//! records *observed* signed history; data-key readiness and projection are
//! separate and must not be inferred from its cursor.

use crate::{
    control_chain::{self, ControlChain},
    sqlite_store::{self, FamilyHandle, SqliteStore, VerifiedSharedEntry},
};

#[derive(Debug)]
pub enum Error {
    Control(control_chain::Error),
    Store(sqlite_store::Error),
    Invalid(&'static str),
}
impl From<control_chain::Error> for Error {
    fn from(value: control_chain::Error) -> Self {
        Self::Control(value)
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
        candidate.apply_public_batch(envelope_bytes, receipt_bytes)?;
        store.append_verified_shared_entry(
            self.family,
            VerifiedSharedEntry {
                previous_cursor: self.chain.last_global_cursor(),
                previous_head: self.chain.head_hash(),
                next_head: candidate.head_hash(),
                kind: 2,
                committed_bytes: envelope_bytes,
                receipt_bytes,
            },
        )?;
        self.chain = candidate;
        Ok(())
    }
}
