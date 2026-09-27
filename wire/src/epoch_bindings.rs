//! Public head and key-commitment history. Callers add only authenticated
//! committed controls after their transition-specific verifier succeeds.

use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EpochBindings {
    heads: BTreeMap<[u8; 32], u32>,
    commitments: BTreeMap<u32, [u8; 32]>,
    latest_epoch: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    HeadReused,
    EpochOrder,
    CommitmentChangedWithinEpoch,
    RotationKeptCommitment,
}

impl EpochBindings {
    pub fn from_genesis(head: [u8; 32], commitment: [u8; 32]) -> Self {
        Self {
            heads: BTreeMap::from([(head, 1)]),
            commitments: BTreeMap::from([(1, commitment)]),
            latest_epoch: 1,
        }
    }

    pub fn record(
        &mut self,
        head: [u8; 32],
        epoch: u32,
        commitment: [u8; 32],
    ) -> Result<(), Error> {
        if self.heads.contains_key(&head) {
            return Err(Error::HeadReused);
        }
        match epoch {
            value if value == self.latest_epoch => {
                if self.commitments.get(&epoch) != Some(&commitment) {
                    return Err(Error::CommitmentChangedWithinEpoch);
                }
            }
            value if self.latest_epoch.checked_add(1) == Some(value) => {
                if self.commitments.get(&self.latest_epoch) == Some(&commitment) {
                    return Err(Error::RotationKeptCommitment);
                }
                self.commitments.insert(epoch, commitment);
                self.latest_epoch = epoch;
            }
            _ => return Err(Error::EpochOrder),
        }
        self.heads.insert(head, epoch);
        Ok(())
    }

    pub fn epoch_for_head(&self, head: &[u8; 32]) -> Option<u32> {
        self.heads.get(head).copied()
    }

    pub fn commitment_for_epoch(&self, epoch: u32) -> Option<[u8; 32]> {
        self.commitments.get(&epoch).copied()
    }

    pub fn latest_epoch(&self) -> u32 {
        self.latest_epoch
    }

    pub fn commitments_snapshot(&self) -> BTreeMap<u32, [u8; 32]> {
        self.commitments.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binds_heads_and_epochs_without_rebinding() {
        let mut ledger = EpochBindings::from_genesis([1; 32], [10; 32]);
        ledger.record([2; 32], 1, [10; 32]).unwrap();
        assert_eq!(ledger.epoch_for_head(&[1; 32]), Some(1));
        assert_eq!(ledger.epoch_for_head(&[2; 32]), Some(1));
        assert_eq!(ledger.commitment_for_epoch(1), Some([10; 32]));
        assert_eq!(
            ledger.record([3; 32], 1, [11; 32]),
            Err(Error::CommitmentChangedWithinEpoch)
        );
        assert_eq!(
            ledger.record([3; 32], 2, [10; 32]),
            Err(Error::RotationKeptCommitment)
        );
        assert_eq!(ledger.record([3; 32], 3, [12; 32]), Err(Error::EpochOrder));
        ledger.record([3; 32], 2, [12; 32]).unwrap();
        assert_eq!(ledger.commitment_for_epoch(2), Some([12; 32]));
        assert_eq!(ledger.latest_epoch(), 2);
        assert_eq!(ledger.record([3; 32], 2, [12; 32]), Err(Error::HeadReused));
        assert_eq!(ledger.record([4; 32], 1, [10; 32]), Err(Error::EpochOrder));
    }
}
