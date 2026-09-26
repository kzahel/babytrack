//! Durable public Family authority and accepted-batch cursor. This session
//! records *observed* signed history; data-key readiness and projection are
//! separate and must not be inferred from its cursor.

use crate::{
    cbor::{self, Value},
    control_chain::{self, ControlChain},
    crypto,
    sqlite_store::{self, FamilyHandle, SqliteStore, VerifiedSharedEntry},
};

#[derive(Debug)]
pub enum Error {
    Control(control_chain::Error),
    Cbor(cbor::Error),
    Crypto(crypto::Error),
    Store(sqlite_store::Error),
    Invalid(&'static str),
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
