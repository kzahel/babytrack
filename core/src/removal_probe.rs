//! Verify sparse signed controls after a device loses data-log access.

use crate::{
    cbor,
    control::{self, exact_map, fixed, number},
    control_chain::{self, ControlChain},
    sync_wire,
};

#[derive(Debug)]
pub enum Error {
    Cbor(cbor::Error),
    Control(control::Error),
    Chain(control_chain::Error),
    Wire(sync_wire::Error),
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
impl From<control_chain::Error> for Error {
    fn from(value: control_chain::Error) -> Self {
        Self::Chain(value)
    }
}
impl From<sync_wire::Error> for Error {
    fn from(value: sync_wire::Error) -> Self {
        Self::Wire(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedRemovalProof {
    pub transition_id: [u8; 16],
    pub cursor: u64,
    pub source_cursor: u64,
    pub known_gap: bool,
    pub committed_bytes: Vec<u8>,
}

pub struct RemovalControlProbe {
    chain: ControlChain,
    family_id: [u8; 16],
    device_id: [u8; 16],
    source_cursor: u64,
}

impl RemovalControlProbe {
    pub fn new(chain: ControlChain, device_id: [u8; 16]) -> Result<Self, Error> {
        chain.active_signing_public(device_id)?;
        Ok(Self {
            family_id: chain.family_id(),
            device_id,
            source_cursor: chain.last_global_cursor(),
            chain,
        })
    }

    pub fn cursor(&self) -> u64 {
        self.chain.last_global_cursor()
    }

    pub fn accept(
        &mut self,
        page_bytes: &[u8],
        after: u64,
    ) -> Result<(Option<VerifiedRemovalProof>, bool), Error> {
        if after != self.cursor() {
            return Err(Error::Invalid(
                "removal probe skips verified control prefix",
            ));
        }
        let page = sync_wire::ControlPage::decode(page_bytes, self.family_id, after)?;
        let mut chain = self.chain.clone();
        let has_more = page.has_more;
        for entry in page.entries {
            chain.apply_sparse_control(&entry.committed_bytes)?;
            if chain.last_global_cursor() != entry.cursor {
                return Err(Error::Invalid("removal proof cursor differs from receipt"));
            }
            let control = cbor::decode_with_limits(
                &entry.committed_bytes,
                cbor::Limits {
                    max_bytes: 1024 * 1024,
                    max_depth: 16,
                },
            )?;
            let root = exact_map(&control, 4)?;
            let unsigned = exact_map(&root[0].1, 11)?;
            if number(&unsigned[5].1)? != 8 {
                continue;
            }
            let delta = exact_map(&unsigned[6].1, 3)?;
            if fixed::<16>(&delta[0].1)? != self.device_id {
                continue;
            }
            if chain
                .active_devices()?
                .iter()
                .any(|row| row.device_id == self.device_id)
            {
                return Err(Error::Invalid("removal proof did not revoke this device"));
            }
            let transition_id = fixed::<16>(&unsigned[4].1)?;
            return Ok((
                Some(VerifiedRemovalProof {
                    transition_id,
                    cursor: entry.cursor,
                    source_cursor: self.source_cursor,
                    known_gap: entry.cursor > self.source_cursor.saturating_add(1),
                    committed_bytes: entry.committed_bytes,
                }),
                has_more,
            ));
        }
        self.chain = chain;
        Ok((None, has_more))
    }
}
