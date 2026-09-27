//! Rebuildable public authority state from signed controls. The relay keeps
//! ciphertext opaque; the shared wire reducers decide each state transition.

use std::collections::{BTreeMap, BTreeSet};

use babytrack_wire::{
    authority as rules,
    cbor::{self, Value},
    crypto,
    epoch_bindings::EpochBindings,
};

use crate::{
    authority::GenesisCandidate,
    batch_authority,
    receipt::{StoredAcceptedBatch, VerifiedControlReceipt},
};

#[derive(Debug)]
#[allow(dead_code)] // Detailed validation errors are retained by the relay boundary.
pub(crate) enum Error {
    Cbor(cbor::Error),
    Crypto(crypto::Error),
    Public(rules::Error),
    Invalid(&'static str),
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
impl From<rules::Error> for Error {
    fn from(error: rules::Error) -> Self {
        Self::Public(error)
    }
}

pub(crate) struct PublicLedger {
    family_id: [u8; 16],
    relay_id: [u8; 32],
    state: Value,
    head: [u8; 32],
    epochs: EpochBindings,
    commitment: [u8; 32],
    last_commit_ms: i64,
    issue_times: BTreeMap<[u8; 16], i64>,
    invitation_objects: BTreeMap<[u8; 16], [u8; 16]>,
    challenges: BTreeMap<[u8; 16], ([u8; 16], [u8; 32])>,
    challenge_objects: BTreeMap<[u8; 16], [u8; 16]>,
    admissions: BTreeMap<[u8; 16], [u8; 16]>,
    admitted_signers: BTreeMap<[u8; 16], [u8; 32]>,
    next_sequences: BTreeMap<[u8; 16], u64>,
    seen_ids: BTreeSet<[u8; 16]>,
}

pub(crate) enum PublicReader {
    Manager([u8; 32]),
    Active([u8; 32]),
    Removed([u8; 32]),
    Invitation {
        signing_key: [u8; 32],
        issue_object: [u8; 16],
    },
    Pending {
        signing_key: [u8; 32],
        challenge_object: Option<[u8; 16]>,
    },
}

impl PublicLedger {
    pub(crate) fn from_genesis(
        genesis: &GenesisCandidate,
        receipt: &VerifiedControlReceipt,
        committed_bytes: &[u8],
    ) -> Result<Self, Error> {
        if receipt.kind != 1
            || receipt.epoch != 1
            || receipt.parent_head != [0; 32]
            || receipt.family_id != genesis.family_id
            || receipt.relay_id != genesis.relay_id
            || receipt.transition_id != genesis.transition_id
        {
            return Err(Error::Invalid("genesis public history context"));
        }
        let head = crypto::hash("control-head", committed_bytes)?;
        let mut seen_ids = BTreeSet::from([genesis.manager_id, genesis.transition_id]);
        if seen_ids.len() != 2 {
            return Err(Error::Invalid("genesis public ID reused"));
        }
        for object in &genesis.manifest {
            if !seen_ids.insert(object.object_id) {
                return Err(Error::Invalid("genesis public ID reused"));
            }
        }
        Ok(Self {
            family_id: genesis.family_id,
            relay_id: genesis.relay_id,
            state: Value::Map(vec![
                (1, Value::Integer(1)),
                (2, Value::Bytes(genesis.family_id.to_vec())),
                (3, Value::Bytes(genesis.relay_id.to_vec())),
                (4, Value::Integer(1)),
                (5, Value::Array(vec![genesis.manager_row.clone()])),
                (6, Value::Array(vec![])),
                (7, Value::Array(vec![])),
            ]),
            head,
            epochs: EpochBindings::from_genesis(head, genesis.epoch_commitment),
            commitment: genesis.epoch_commitment,
            last_commit_ms: receipt.committed_ms,
            issue_times: BTreeMap::new(),
            invitation_objects: BTreeMap::new(),
            challenges: BTreeMap::new(),
            challenge_objects: BTreeMap::new(),
            admissions: BTreeMap::new(),
            admitted_signers: BTreeMap::from([(genesis.manager_id, genesis.manager_signing_key)]),
            next_sequences: BTreeMap::new(),
            seen_ids,
        })
    }

    pub(crate) fn apply(
        &mut self,
        receipt: &VerifiedControlReceipt,
        committed_bytes: &[u8],
    ) -> Result<(), Error> {
        if receipt.family_id != self.family_id
            || receipt.relay_id != self.relay_id
            || receipt.parent_head != self.head
            || receipt.committed_ms < self.last_commit_ms
        {
            return Err(Error::Invalid("control public history context"));
        }
        let candidate = &receipt.candidate_bytes;
        let value = cbor::decode(candidate)?;
        let Value::Map(root) = value else {
            return Err(Error::Invalid("candidate not map"));
        };
        let Value::Map(unsigned) = &root[0].1 else {
            return Err(Error::Invalid("unsigned control not map"));
        };
        let mut new_ids = vec![receipt.transition_id];
        new_ids.extend(receipt.manifest.iter().map(|object| object.id));
        let mut issued = None;
        let mut challenged = None;
        let mut challenge_object = None;
        let mut admitted = None;
        let mut rotated = None;
        let next = match receipt.kind {
            2 => {
                let prepared = rules::prepare_issue(candidate, &self.state, self.head)?;
                new_ids.push(prepared.invitation_id);
                issued = Some((prepared.invitation_id, prepared.manifest.object_id));
                prepared.next_state
            }
            3 => rules::prepare_cancel(candidate, &self.state, self.head)?.next_state,
            4 => {
                let prepared = rules::prepare_claim(candidate, &self.state, self.head)?;
                let issued_at = self
                    .issue_times
                    .get(&prepared.invitation_id)
                    .ok_or(Error::Invalid("claim issue time absent"))?;
                let expires = issued_at
                    .checked_add(604_800_000)
                    .ok_or(Error::Invalid("claim expiry overflow"))?;
                if receipt.committed_ms >= expires {
                    return Err(Error::Invalid("claim committed after expiry"));
                }
                new_ids.push(prepared.device_id);
                prepared.next_state
            }
            5 => {
                let Value::Map(delta) = &unsigned[6].1 else {
                    return Err(Error::Invalid("proof delta not map"));
                };
                let invitation = fixed::<16>(&delta[0].1)?;
                let (challenge_id, challenge_hash) = self
                    .challenges
                    .get(&invitation)
                    .ok_or(Error::Invalid("proof challenge absent"))?;
                rules::prepare_proof(
                    candidate,
                    &self.state,
                    self.head,
                    *challenge_id,
                    *challenge_hash,
                )?
                .next_state
            }
            6 => {
                let prepared =
                    rules::prepare_admission(candidate, &self.state, self.head, self.commitment)?;
                admitted = Some((prepared.device_id, prepared.transition_id));
                prepared.next_state
            }
            7 => rules::prepare_role_change(candidate, &self.state, self.head)?.next_state,
            8 => {
                let prepared =
                    rules::prepare_removal(candidate, &self.state, self.head, self.commitment)?;
                rotated = Some(prepared.new_commitment);
                prepared.next_state
            }
            9 => rules::prepare_pending_removal(candidate, &self.state, self.head)?.next_state,
            10 => {
                let Value::Map(delta) = &unsigned[6].1 else {
                    return Err(Error::Invalid("repair delta not map"));
                };
                let device_id = fixed::<16>(&delta[0].1)?;
                rules::prepare_repair(
                    candidate,
                    &self.state,
                    self.head,
                    self.commitment,
                    self.admissions.get(&device_id).copied(),
                )?;
                self.state.clone()
            }
            11 => {
                let prepared = rules::prepare_challenge(candidate, &self.state, self.head)?;
                new_ids.push(prepared.challenge_id);
                challenged = Some((
                    prepared.invitation_id,
                    (prepared.challenge_id, prepared.challenge_hash),
                ));
                challenge_object = Some((
                    prepared.invitation_id,
                    prepared
                        .manifest
                        .iter()
                        .find(|entry| entry.kind == 2)
                        .ok_or(Error::Invalid("challenge HPKE object absent"))?
                        .object_id,
                ));
                prepared.next_state
            }
            _ => return Err(Error::Invalid("unsupported public control kind")),
        };
        let mut within = BTreeSet::new();
        if !new_ids
            .iter()
            .all(|id| !self.seen_ids.contains(id) && within.insert(*id))
        {
            return Err(Error::Invalid("public control ID reused"));
        }
        let head = crypto::hash("control-head", committed_bytes)?;
        let commitment = rotated.unwrap_or(self.commitment);
        self.epochs
            .record(head, receipt.epoch, commitment)
            .map_err(|_| Error::Invalid("public epoch binding differs"))?;
        if let Some((device, _)) = admitted {
            let signing_key = active_signing_key(&next, device)?
                .ok_or(Error::Invalid("admitted device absent from next state"))?;
            self.admitted_signers.insert(device, signing_key);
        }
        self.state = next;
        self.head = head;
        self.commitment = commitment;
        self.last_commit_ms = receipt.committed_ms;
        self.seen_ids.extend(new_ids);
        if let Some((invitation, object_id)) = issued {
            self.issue_times.insert(invitation, receipt.committed_ms);
            self.invitation_objects.insert(invitation, object_id);
        }
        if let Some((invitation, challenge)) = challenged {
            self.challenges.insert(invitation, challenge);
        }
        if let Some((invitation, object_id)) = challenge_object {
            self.challenge_objects.insert(invitation, object_id);
        }
        if let Some((device, admission)) = admitted {
            self.admissions.insert(device, admission);
        }
        if rotated.is_some() {
            self.challenges.clear();
            self.challenge_objects.clear();
        }
        Ok(())
    }

    pub(crate) fn head(&self) -> [u8; 32] {
        self.head
    }

    pub(crate) fn relay_id(&self) -> [u8; 32] {
        self.relay_id
    }

    pub(crate) fn current_epoch(&self) -> Result<u32, Error> {
        let Value::Map(state) = &self.state else {
            return Err(Error::Invalid("public state not map"));
        };
        let Value::Integer(epoch) = state[3].1 else {
            return Err(Error::Invalid("public epoch not integer"));
        };
        epoch
            .try_into()
            .map_err(|_| Error::Invalid("public epoch range"))
    }

    pub(crate) fn epoch_for_head(&self, head: &[u8; 32]) -> Option<u32> {
        self.epochs.epoch_for_head(head)
    }

    pub(crate) fn next_sequence(&self, author: [u8; 16]) -> u64 {
        self.next_sequences.get(&author).copied().unwrap_or(1)
    }

    pub(crate) fn last_commit_ms(&self) -> i64 {
        self.last_commit_ms
    }

    /// Historical invitation signing keys remain usable only for the narrow
    /// status route after the normal control-read credential closes.
    pub(crate) fn invitation_status(
        &self,
        invitation_id: [u8; 16],
        observed_ms: i64,
    ) -> Result<Option<([u8; 32], u8)>, Error> {
        let Value::Map(state) = &self.state else {
            return Err(Error::Invalid("public state not map"));
        };
        let Value::Array(invitations) = &state[6].1 else {
            return Err(Error::Invalid("invitations not array"));
        };
        for row in invitations {
            let Value::Array(fields) = row else {
                return Err(Error::Invalid("invitation row not array"));
            };
            if fixed::<16>(&fields[0])? != invitation_id {
                continue;
            }
            let status = match fields[5] {
                Value::Integer(2) => 2, // claimed
                Value::Integer(3) => 3, // canceled
                Value::Integer(1) => {
                    let issued = *self
                        .issue_times
                        .get(&invitation_id)
                        .ok_or(Error::Invalid("invitation issue time absent"))?;
                    let expires = issued
                        .checked_add(604_800_000)
                        .ok_or(Error::Invalid("invitation expiry overflow"))?;
                    if observed_ms >= expires {
                        4 // expired
                    } else {
                        let Value::Array(active) = &state[4].1 else {
                            return Err(Error::Invalid("active devices not array"));
                        };
                        let issuer = fixed::<16>(&fields[1])?;
                        if active.iter().any(|device| {
                            let Value::Array(device) = device else {
                                return false;
                            };
                            fixed::<16>(&device[0]).ok() == Some(issuer)
                                && device[4] == Value::Integer(2)
                        }) {
                            1 // currently unused
                        } else {
                            5 // issuer lost authority
                        }
                    }
                }
                _ => return Err(Error::Invalid("invitation status invalid")),
            };
            return Ok(Some((fixed::<32>(&fields[2])?, status)));
        }
        Ok(None)
    }

    pub(crate) fn reader(&self, signer_id: [u8; 16]) -> Result<Option<PublicReader>, Error> {
        let Value::Map(state) = &self.state else {
            return Err(Error::Invalid("public state not map"));
        };
        let Value::Array(active) = &state[4].1 else {
            return Err(Error::Invalid("active devices not array"));
        };
        for row in active {
            let Value::Array(fields) = row else {
                return Err(Error::Invalid("active row not array"));
            };
            if fixed::<16>(&fields[0])? == signer_id {
                let key = fixed::<32>(&fields[1])?;
                return Ok(Some(match fields[4] {
                    Value::Integer(2) => PublicReader::Manager(key),
                    Value::Integer(1) => PublicReader::Active(key),
                    _ => return Err(Error::Invalid("active role invalid")),
                }));
            }
        }
        let Value::Array(pending) = &state[5].1 else {
            return Err(Error::Invalid("pending devices not array"));
        };
        for row in pending {
            let Value::Array(fields) = row else {
                return Err(Error::Invalid("pending row not array"));
            };
            if fixed::<16>(&fields[1])? == signer_id {
                let invitation_id = fixed::<16>(&fields[0])?;
                return Ok(Some(PublicReader::Pending {
                    signing_key: fixed::<32>(&fields[2])?,
                    challenge_object: self.challenge_objects.get(&invitation_id).copied(),
                }));
            }
        }
        let Value::Array(invitations) = &state[6].1 else {
            return Err(Error::Invalid("invitations not array"));
        };
        for row in invitations {
            let Value::Array(fields) = row else {
                return Err(Error::Invalid("invitation row not array"));
            };
            if fixed::<16>(&fields[0])? == signer_id {
                let issued = self
                    .issue_times
                    .get(&signer_id)
                    .ok_or(Error::Invalid("invitation issue time absent"))?;
                let expires = issued
                    .checked_add(604_800_000)
                    .ok_or(Error::Invalid("invitation expiry overflow"))?;
                if fields[5] != Value::Integer(1)
                    || self.last_commit_ms >= expires
                    || self
                        .invitation_status(signer_id, self.last_commit_ms)?
                        .is_none_or(|(_, reason)| reason != 1)
                {
                    return Ok(None);
                }
                return Ok(Some(PublicReader::Invitation {
                    signing_key: fixed::<32>(&fields[2])?,
                    issue_object: *self
                        .invitation_objects
                        .get(&signer_id)
                        .ok_or(Error::Invalid("invitation object absent"))?,
                }));
            }
        }
        Ok(self
            .admitted_signers
            .get(&signer_id)
            .copied()
            .map(PublicReader::Removed))
    }

    pub(crate) fn contains_id(&self, id: &[u8; 16]) -> bool {
        self.seen_ids.contains(id)
    }

    /// Recheck an accepted data entry at its historical authority position.
    /// A valid relay receipt alone cannot establish author or sequence rights.
    pub(crate) fn apply_batch(
        &mut self,
        envelope: &[u8],
        saved: &StoredAcceptedBatch,
    ) -> Result<(), Error> {
        let Value::Map(state) = &self.state else {
            return Err(Error::Invalid("public state not map"));
        };
        let Value::Array(active) = &state[4].1 else {
            return Err(Error::Invalid("active devices not array"));
        };
        let signer = active
            .iter()
            .find_map(|row| {
                let Value::Array(fields) = row else {
                    return None;
                };
                (fixed::<16>(fields.first()?).ok()? == saved.author_id)
                    .then(|| fixed::<32>(fields.get(1)?).ok())
                    .flatten()
            })
            .ok_or(Error::Invalid("accepted batch author not active"))?;
        let batch = batch_authority::verify(envelope, self.family_id, self.relay_id, signer)
            .map_err(|_| Error::Invalid("accepted batch envelope differs"))?;
        let epoch: u32 = match state[3].1 {
            Value::Integer(value) => value
                .try_into()
                .map_err(|_| Error::Invalid("public epoch range"))?,
            _ => return Err(Error::Invalid("public epoch not integer")),
        };
        let expected = self
            .next_sequences
            .get(&batch.author_id)
            .copied()
            .unwrap_or(1);
        if batch.batch_id != saved.batch_id
            || batch.author_id != saved.author_id
            || batch.sequence != saved.sequence
            || batch.epoch != saved.epoch
            || batch.control_head != saved.control_head
            || batch.epoch != epoch
            || self.epochs.epoch_for_head(&batch.control_head) != Some(epoch)
            || batch.sequence != expected
            || !self.seen_ids.insert(batch.batch_id)
        {
            return Err(Error::Invalid("accepted batch public authority differs"));
        }
        self.next_sequences.insert(
            batch.author_id,
            expected
                .checked_add(1)
                .ok_or(Error::Invalid("accepted sequence overflow"))?,
        );
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn epochs(&self) -> &EpochBindings {
        &self.epochs
    }

    pub(crate) fn state(&self) -> &Value {
        &self.state
    }
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

fn active_signing_key(state: &Value, device: [u8; 16]) -> Result<Option<[u8; 32]>, Error> {
    let Value::Map(fields) = state else {
        return Err(Error::Invalid("public state not map"));
    };
    let Value::Array(active) = &fields[4].1 else {
        return Err(Error::Invalid("active devices not array"));
    };
    for row in active {
        let Value::Array(parts) = row else {
            return Err(Error::Invalid("active row not array"));
        };
        if fixed::<16>(&parts[0])? == device {
            return Ok(Some(fixed::<32>(&parts[1])?));
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{authority, receipt};
    use serde_json::Value as Json;

    fn hex(value: &str) -> Vec<u8> {
        (0..value.len())
            .step_by(2)
            .map(|index| u8::from_str_radix(&value[index..index + 2], 16).unwrap())
            .collect()
    }

    #[test]
    fn replays_published_controls_with_shared_public_state() {
        let vector: Json = serde_json::from_str(
            &std::fs::read_to_string("../tests/vectors/contiguous-chain-v1.json").unwrap(),
        )
        .unwrap();
        let seed: [u8; 32] = hex(vector["test_only_inputs"]["relay_sign_seed_hex"]
            .as_str()
            .unwrap())
        .try_into()
        .unwrap();
        let relay_public = crypto::signing_public_key(&seed);
        let manager_id: [u8; 16] = hex(vector["test_only_inputs"]["manager_device_id_hex"]
            .as_str()
            .unwrap())
        .try_into()
        .unwrap();
        let recipient_id: [u8; 16] = hex(vector["test_only_inputs"]["recipient_device_id_hex"]
            .as_str()
            .unwrap())
        .try_into()
        .unwrap();
        let issue_state = cbor::decode(&hex(vector["transitions"][1]["state_cbor_hex"]
            .as_str()
            .unwrap()))
        .unwrap();
        let Value::Map(issue_fields) = issue_state else {
            panic!()
        };
        let Value::Array(invitations) = &issue_fields[6].1 else {
            panic!()
        };
        let Value::Array(invitation) = &invitations[0] else {
            panic!()
        };
        let invitation_id = fixed::<16>(&invitation[0]).unwrap();
        let issue_object: [u8; 16] =
            hex(vector["transitions"][1]["manifest"][0][1].as_str().unwrap())
                .try_into()
                .unwrap();
        let challenge_object: [u8; 16] =
            hex(vector["transitions"][3]["manifest"][0][1].as_str().unwrap())
                .try_into()
                .unwrap();
        let mut ledger: Option<PublicLedger> = None;
        for transition in vector["transitions"].as_array().unwrap() {
            let committed = hex(transition["committed_cbor_hex"].as_str().unwrap());
            let verified = receipt::verify_control_receipt(&committed, &relay_public).unwrap();
            if let Some(current) = &mut ledger {
                current.apply(&verified, &committed).unwrap();
            } else {
                let genesis =
                    authority::verify_genesis_candidate(&verified.candidate_bytes, &relay_public)
                        .unwrap();
                ledger = Some(PublicLedger::from_genesis(&genesis, &verified, &committed).unwrap());
            }
            let current = ledger.as_mut().unwrap();
            assert_eq!(
                cbor::encode(current.state()).unwrap(),
                hex(transition["state_cbor_hex"].as_str().unwrap()),
                "{} state",
                transition["name"].as_str().unwrap()
            );
            assert_eq!(
                current.head().to_vec(),
                hex(transition["head_hash_hex"].as_str().unwrap()),
                "{} head",
                transition["name"].as_str().unwrap()
            );
            assert!(matches!(
                current.reader(manager_id).unwrap(),
                Some(PublicReader::Manager(_))
            ));
            match transition["name"].as_str().unwrap() {
                "genesis" => {
                    assert!(current.reader(invitation_id).unwrap().is_none());
                    assert!(current.reader(recipient_id).unwrap().is_none());
                }
                "invite_issue" => {
                    assert!(matches!(
                        current.reader(invitation_id).unwrap(),
                        Some(PublicReader::Invitation { issue_object: id, .. }) if id == issue_object
                    ));
                    assert_eq!(
                        current
                            .invitation_status(
                                invitation_id,
                                verified.committed_ms + 604_800_000 - 1
                            )
                            .unwrap()
                            .unwrap()
                            .1,
                        1
                    );
                    assert_eq!(
                        current
                            .invitation_status(invitation_id, verified.committed_ms + 604_800_000)
                            .unwrap()
                            .unwrap()
                            .1,
                        4
                    );
                    let Value::Map(state) = &mut current.state else {
                        panic!()
                    };
                    let Value::Array(active) = &mut state[4].1 else {
                        panic!()
                    };
                    let Value::Array(issuer) = &mut active[0] else {
                        panic!()
                    };
                    issuer[4] = Value::Integer(1);
                    assert_eq!(
                        current
                            .invitation_status(invitation_id, verified.committed_ms)
                            .unwrap()
                            .unwrap()
                            .1,
                        5
                    );
                    let Value::Map(state) = &mut current.state else {
                        panic!()
                    };
                    let Value::Array(active) = &mut state[4].1 else {
                        panic!()
                    };
                    let Value::Array(issuer) = &mut active[0] else {
                        panic!()
                    };
                    issuer[4] = Value::Integer(2);
                }
                "invite_claim" => {
                    assert!(current.reader(invitation_id).unwrap().is_none());
                    assert_eq!(
                        current
                            .invitation_status(invitation_id, verified.committed_ms)
                            .unwrap()
                            .unwrap()
                            .1,
                        2
                    );
                    assert!(matches!(
                        current.reader(recipient_id).unwrap(),
                        Some(PublicReader::Pending {
                            challenge_object: None,
                            ..
                        })
                    ));
                }
                "holder_challenge" | "key_proof" => assert!(matches!(
                    current.reader(recipient_id).unwrap(),
                    Some(PublicReader::Pending { challenge_object: Some(id), .. }) if id == challenge_object
                )),
                "admit_grant" | "grant_repair" => assert!(matches!(
                    current.reader(recipient_id).unwrap(),
                    Some(PublicReader::Active(_))
                )),
                "remove_active" => assert!(matches!(
                    current.reader(recipient_id).unwrap(),
                    Some(PublicReader::Removed(_))
                )),
                _ => panic!("unexpected vector transition"),
            }
        }
        assert_eq!(ledger.unwrap().epochs().latest_epoch(), 2);
    }
}
