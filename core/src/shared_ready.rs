//! Data-ready replay from a durable signed log and manifest-bound objects.

use std::collections::BTreeMap;

use crate::{
    batch,
    cbor::{self, Value},
    control_chain, crypto,
    enrollment::EnrollmentAttempt,
    grant, membership,
    operation::NewOperation,
    projection::{self, Projection, VerifiedEpochKey},
    ready_replay::{self, initial_epoch_projection, verify_manifest},
    shared_history::{self, PublicHistorySession},
    sqlite_store::{self, AppendedOperation, FamilyHandle, PreparedBatch, SqliteStore},
};

#[derive(Debug)]
pub enum Error {
    Batch(batch::Error),
    Cbor(cbor::Error),
    Control(control_chain::Error),
    Crypto(crypto::Error),
    Grant(grant::Error),
    Membership(membership::Error),
    Projection(projection::Error),
    Ready(ready_replay::Error),
    Public(shared_history::Error),
    Store(sqlite_store::Error),
    Invalid(&'static str),
}
impl From<batch::Error> for Error {
    fn from(value: batch::Error) -> Self {
        Self::Batch(value)
    }
}
impl From<cbor::Error> for Error {
    fn from(value: cbor::Error) -> Self {
        Self::Cbor(value)
    }
}
impl From<control_chain::Error> for Error {
    fn from(value: control_chain::Error) -> Self {
        Self::Control(value)
    }
}
impl From<crypto::Error> for Error {
    fn from(value: crypto::Error) -> Self {
        Self::Crypto(value)
    }
}
impl From<grant::Error> for Error {
    fn from(value: grant::Error) -> Self {
        Self::Grant(value)
    }
}
impl From<membership::Error> for Error {
    fn from(value: membership::Error) -> Self {
        Self::Membership(value)
    }
}
impl From<projection::Error> for Error {
    fn from(value: projection::Error) -> Self {
        Self::Projection(value)
    }
}
impl From<ready_replay::Error> for Error {
    fn from(value: ready_replay::Error) -> Self {
        Self::Ready(value)
    }
}
impl From<shared_history::Error> for Error {
    fn from(value: shared_history::Error) -> Self {
        Self::Public(value)
    }
}
impl From<sqlite_store::Error> for Error {
    fn from(value: sqlite_store::Error) -> Self {
        Self::Store(value)
    }
}

pub struct ReadyFamilySession {
    family: FamilyHandle,
    projection: Projection,
    observed_cursor: u64,
    observed_head: [u8; 32],
    relay_id: [u8; 32],
    active_epoch: u32,
    current_key: VerifiedEpochKey,
    signing_public: [u8; 32],
    next_sequence: u64,
}

pub enum NextUpload {
    /// Keep querying or retrying these exact signed bytes until a verified
    /// acceptance or signed rejection resolves the outcome.
    RetryExact(PreparedBatch),
    /// Fresh ID and nonce reserved together with the signed envelope.
    Fresh(PreparedBatch),
}

impl ReadyFamilySession {
    /// Any missing, malformed, or mismatched object leaves the public pin
    /// intact but prevents a data-ready view. The caller supplies its local
    /// secrets; this method never obtains keys from the relay.
    pub fn from_store(
        store: &SqliteStore,
        family: FamilyHandle,
        initial_epoch_key: [u8; 32],
        manager_agreement_private: [u8; 32],
    ) -> Result<Self, Error> {
        Self::from_store_with_initial_key(
            store,
            family,
            initial_epoch_key,
            manager_agreement_private,
            true,
        )
    }

    /// Open the recipient's committed admission grant using only the
    /// agreement key saved before its claim, then replay full history.
    pub fn from_enrollment(
        store: &SqliteStore,
        enrollment: &EnrollmentAttempt,
    ) -> Result<Self, Error> {
        Self::from_admission_grant(store, enrollment.family(), enrollment.agreement_private())
    }

    /// Reopen an admitted device using its locally held agreement key. The
    /// grant must be named by verified public history and downloaded into
    /// this Family's store before any data-ready view is returned.
    pub fn from_admission_grant(
        store: &SqliteStore,
        family: FamilyHandle,
        agreement_private: [u8; 32],
    ) -> Result<Self, Error> {
        let public = PublicHistorySession::resume(store, family)?;
        let grant = public
            .chain()
            .initial_admission_grant(&family.device_id)
            .ok_or(Error::Invalid("recipient has no committed admission grant"))?;
        let objects: BTreeMap<_, _> = store.shared_objects(family)?.into_iter().collect();
        let grant_object = objects
            .get(&grant.grant_id())
            .ok_or(Error::Invalid("recipient admission grant not downloaded"))?;
        let key = grant.open(grant_object, &agreement_private)?;
        let mut keys = BTreeMap::from([(key.epoch, key.clone())]);
        if key.epoch > 1 {
            let (transition_id, rotation) = public
                .chain()
                .rotation_for_epoch(key.epoch)
                .ok_or(Error::Invalid("admission epoch rotation missing"))?;
            let keyring = objects
                .get(&rotation.keyring_id())
                .ok_or(Error::Invalid("admission history keyring missing"))?;
            let membership = public
                .chain()
                .membership_check(&transition_id)
                .ok_or(Error::Invalid("admission epoch membership missing"))?;
            let membership_object = objects
                .get(&membership.object_id())
                .ok_or(Error::Invalid("admission epoch membership object missing"))?;
            let recovered = public.chain().open_rotation_from_known_epoch_key(
                &transition_id,
                &key,
                keyring,
                membership_object,
            )?;
            for epoch in 1..key.epoch {
                keys.insert(
                    epoch,
                    recovered
                        .earlier(epoch)
                        .ok_or(Error::Invalid("admission history epoch missing"))?
                        .clone(),
                );
            }
        }
        Self::from_store_with_keys(store, family, keys, agreement_private, false)
    }

    fn from_store_with_initial_key(
        store: &SqliteStore,
        family: FamilyHandle,
        initial_epoch_key: [u8; 32],
        manager_agreement_private: [u8; 32],
        require_initial_manager: bool,
    ) -> Result<Self, Error> {
        Self::from_store_with_keys(
            store,
            family,
            BTreeMap::from([(
                1,
                VerifiedEpochKey {
                    family_id: family.family_id,
                    epoch: 1,
                    bytes: initial_epoch_key,
                },
            )]),
            manager_agreement_private,
            require_initial_manager,
        )
    }

    fn from_store_with_keys(
        store: &SqliteStore,
        family: FamilyHandle,
        mut keys: BTreeMap<u32, VerifiedEpochKey>,
        manager_agreement_private: [u8; 32],
        require_initial_manager: bool,
    ) -> Result<Self, Error> {
        let public = PublicHistorySession::resume(store, family)?;
        let history = store
            .shared_history(family)?
            .ok_or(Error::Invalid("Family has no shared genesis"))?;
        let genesis =
            crate::control::verify_genesis(&history.genesis_bytes, &history.relay_public_key)
                .map_err(control_chain::Error::Control)?;
        if require_initial_manager && genesis.manager_device_id() != family.device_id {
            return Err(Error::Invalid("device is not the initial manager"));
        }
        let objects: BTreeMap<_, _> = store.shared_objects(family)?.into_iter().collect();
        let initial_epoch_key = keys
            .get(&1)
            .ok_or(Error::Invalid("initial history key missing"))?
            .bytes;
        let (mut chain, mut projection, first_key) = initial_epoch_projection(
            &history.genesis_bytes,
            history.relay_public_key,
            initial_epoch_key,
            &objects,
        )?;
        if keys.get(&1) != Some(&first_key) {
            return Err(Error::Invalid("initial history key differs from genesis"));
        }
        for entry in history.entries {
            match entry.kind {
                1 => {
                    let (transition_id, kind) = control_id_and_kind(&entry.committed_bytes)?;
                    chain.apply_control(&entry.committed_bytes)?;
                    verify_manifest(&entry.committed_bytes, &objects)?;
                    if kind == 8 {
                        let rotation = chain
                            .rotation(&transition_id)
                            .ok_or(Error::Invalid("rotation missing from verified chain"))?;
                        let keyring = objects
                            .get(&rotation.keyring_id())
                            .ok_or(Error::Invalid("rotation keyring object missing"))?;
                        let membership = chain
                            .membership_check(&transition_id)
                            .ok_or(Error::Invalid("rotation membership missing"))?;
                        let membership_object = objects
                            .get(&membership.object_id())
                            .ok_or(Error::Invalid("rotation membership object missing"))?;
                        let rotated = if let Some(known) = keys.get(&rotation.epoch) {
                            chain.open_rotation_from_known_epoch_key(
                                &transition_id,
                                known,
                                keyring,
                                membership_object,
                            )?
                        } else {
                            let grants = rotation
                                .grant_ids()
                                .into_iter()
                                .map(|id| {
                                    objects
                                        .get(&id)
                                        .cloned()
                                        .map(|bytes| (id, bytes))
                                        .ok_or(Error::Invalid("rotation grant object missing"))
                                })
                                .collect::<Result<Vec<_>, _>>()?;
                            chain.open_rotation_for(
                                &transition_id,
                                family.device_id,
                                &manager_agreement_private,
                                &grants,
                                keyring,
                                membership_object,
                            )?
                        };
                        for (epoch, prior) in
                            keys.iter().filter(|(epoch, _)| **epoch < rotation.epoch)
                        {
                            if rotated.earlier(*epoch) != Some(prior) {
                                return Err(Error::Invalid(
                                    "rotation history key differs from locally verified key",
                                ));
                            }
                        }
                        keys.insert(rotated.current().epoch, rotated.current().clone());
                    } else if let Some(membership) = chain.membership_check(&transition_id) {
                        let object = objects
                            .get(&membership.object_id())
                            .ok_or(Error::Invalid("membership object missing"))?;
                        let key = keys
                            .get(&membership.epoch)
                            .ok_or(Error::Invalid("membership epoch key missing"))?;
                        membership.verify(object, key)?;
                    }
                    projection.advance_control(entry.cursor)?;
                }
                2 => {
                    let signed =
                        chain.apply_public_batch(&entry.committed_bytes, &entry.receipt_bytes)?;
                    let key = keys
                        .get(&signed.header().epoch)
                        .ok_or(Error::Invalid("batch epoch key missing"))?;
                    projection.apply_authorized_signed(&signed, key, entry.cursor)?;
                }
                _ => return Err(Error::Invalid("shared entry kind invalid")),
            }
        }
        if chain.last_global_cursor() != public.cursor()
            || chain.head_hash() != public.head_hash()
            || projection.last_cursor() != public.cursor()
        {
            return Err(Error::Invalid(
                "ready replay differs from pinned public history",
            ));
        }
        let active_epoch = *keys.keys().next_back().unwrap();
        let current_key = keys
            .remove(&active_epoch)
            .ok_or(Error::Invalid("ready epoch key missing"))?;
        let signing_public = chain.active_signing_public(family.device_id)?;
        let next_sequence = chain.next_sequence_for(family.device_id)?;
        Ok(Self {
            family,
            projection,
            observed_cursor: public.cursor(),
            observed_head: public.head_hash(),
            relay_id: chain.relay_id(),
            active_epoch,
            current_key,
            signing_public,
            next_sequence,
        })
    }

    pub fn stage_next_local(
        &self,
        store: &mut SqliteStore,
        signing_seed: &[u8; 32],
    ) -> Result<NextUpload, Error> {
        if crypto::signing_public_key(signing_seed) != self.signing_public {
            return Err(Error::Invalid("signing seed differs from active device"));
        }
        let existed = store.pending_batch(self.family)?.is_some();
        let prepared = store.stage_next_batch(
            self.family,
            self.relay_id,
            self.observed_head,
            self.active_epoch,
            &self.current_key.bytes,
            signing_seed,
        )?;
        let signed = batch::verify_signed_envelope(
            &prepared.envelope_bytes,
            &self.family.family_id,
            &self.relay_id,
            &self.signing_public,
        )?;
        let header = signed.header();
        if header.author_device_id != self.family.device_id
            || header.batch_id != prepared.batch_id
            || signed.object_hash() != prepared.object_hash
            || header.device_sequence != self.next_sequence
            || prepared.sequence != self.next_sequence
        {
            return Err(Error::Invalid(
                "local outbox differs from verified authority",
            ));
        }
        if existed {
            return Ok(NextUpload::RetryExact(prepared));
        }
        if header.control_head != self.observed_head || header.epoch != self.active_epoch {
            return Err(Error::Invalid("new outbox uses stale head or epoch"));
        }
        Ok(NextUpload::Fresh(prepared))
    }

    pub fn stage_enrolled_local(
        &self,
        store: &mut SqliteStore,
        enrollment: &EnrollmentAttempt,
    ) -> Result<NextUpload, Error> {
        if enrollment.family() != self.family {
            return Err(Error::Invalid(
                "enrollment belongs to another device or Family",
            ));
        }
        self.stage_next_local(store, &enrollment.signing_seed())
    }

    pub fn has_unsent_local(&self, store: &SqliteStore) -> Result<bool, Error> {
        Ok(self.unsent_local_count(store)? != 0)
    }

    pub fn unsent_local_count(&self, store: &SqliteStore) -> Result<u64, Error> {
        let public = PublicHistorySession::resume(store, self.family)?;
        if public.cursor() != self.observed_cursor || public.head_hash() != self.observed_head {
            return Err(Error::Invalid("ready view is behind verified history"));
        }
        Ok(store.unsent_operations(self.family)?.len() as u64)
    }

    pub fn projection(&self) -> &Projection {
        &self.projection
    }

    pub fn projection_with_pending(&self, store: &SqliteStore) -> Result<Projection, Error> {
        let public = PublicHistorySession::resume(store, self.family)?;
        if public.cursor() != self.observed_cursor || public.head_hash() != self.observed_head {
            return Err(Error::Invalid("ready view is behind verified history"));
        }
        Ok(self
            .projection
            .with_local_overlay(&store.unsent_operations(self.family)?)?)
    }

    pub fn append_local(
        &self,
        store: &mut SqliteStore,
        operation: NewOperation,
        now_ms: i64,
    ) -> Result<AppendedOperation, Error> {
        Ok(store.append_shared_local(
            self.family,
            operation,
            now_ms,
            &self.projection,
            self.observed_cursor,
            self.observed_head,
        )?)
    }

    pub fn family(&self) -> FamilyHandle {
        self.family
    }

    pub fn observed_cursor(&self) -> u64 {
        self.observed_cursor
    }

    pub fn observed_head(&self) -> [u8; 32] {
        self.observed_head
    }

    pub fn active_epoch(&self) -> u32 {
        self.active_epoch
    }

    pub(crate) fn current_key(&self) -> &VerifiedEpochKey {
        &self.current_key
    }
}

fn control_id_and_kind(bytes: &[u8]) -> Result<([u8; 16], u64), Error> {
    let value = cbor::decode(bytes)?;
    let Value::Map(root) = value else {
        return Err(Error::Invalid("control not map"));
    };
    let Value::Map(unsigned) = &root[0].1 else {
        return Err(Error::Invalid("unsigned control not map"));
    };
    let Value::Bytes(id) = &unsigned[4].1 else {
        return Err(Error::Invalid("transition ID not bytes"));
    };
    let Value::Integer(kind) = unsigned[5].1 else {
        return Err(Error::Invalid("transition kind not integer"));
    };
    Ok((
        id.as_slice()
            .try_into()
            .map_err(|_| Error::Invalid("transition ID length"))?,
        kind.try_into()
            .map_err(|_| Error::Invalid("transition kind negative"))?,
    ))
}

#[cfg(test)]
mod later_admission_tests {
    use super::*;

    fn hex(value: &str) -> Vec<u8> {
        value
            .as_bytes()
            .chunks_exact(2)
            .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
            .collect()
    }

    fn fixed<const N: usize>(value: &str) -> [u8; N] {
        hex(value).try_into().unwrap()
    }

    #[test]
    fn known_rotated_key_replays_prior_history_without_an_old_rotation_grant() {
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../../tests/vectors/contiguous-chain-v1.json"))
                .unwrap();
        let input = &fixture["test_only_inputs"];
        let family = FamilyHandle {
            family_id: fixed(input["family_id_hex"].as_str().unwrap()),
            device_id: fixed(input["manager_device_id_hex"].as_str().unwrap()),
        };
        let relay_public = crypto::signing_public_key(&fixed::<32>(
            input["relay_sign_seed_hex"].as_str().unwrap(),
        ));
        let transitions = fixture["transitions"].as_array().unwrap();
        let wire = |index: usize| hex(transitions[index]["committed_cbor_hex"].as_str().unwrap());
        let directory = tempfile::tempdir().unwrap();
        let mut store = SqliteStore::open(directory.path().join("family.sqlite")).unwrap();
        store
            .create_family(family.family_id, family.device_id)
            .unwrap();
        let mut public =
            PublicHistorySession::begin(&mut store, family, &wire(0), relay_public).unwrap();
        let objects = &fixture["objects_by_id_hex"];
        let accept_objects =
            |public: &mut PublicHistorySession, store: &mut SqliteStore, index: usize| {
                for row in transitions[index]["manifest"].as_array().unwrap() {
                    let id = row[1].as_str().unwrap();
                    public
                        .accept_object(store, fixed(id), &hex(objects[id].as_str().unwrap()))
                        .unwrap();
                }
            };
        accept_objects(&mut public, &mut store, 0);
        for index in 1..=7 {
            if index == 7 {
                let batch = &fixture["batch"];
                public
                    .accept_batch(
                        &mut store,
                        &hex(batch["envelope_cbor_hex"].as_str().unwrap()),
                        &hex(batch["receipt_cbor_hex"].as_str().unwrap()),
                    )
                    .unwrap();
            }
            public.accept_control(&mut store, &wire(index)).unwrap();
            accept_objects(&mut public, &mut store, index);
        }
        let epoch_one = fixed::<32>(input["epoch_1_key_hex"].as_str().unwrap());
        let epoch_two = fixed::<32>(input["epoch_2_key_hex"].as_str().unwrap());
        let keys = BTreeMap::from([
            (
                1,
                VerifiedEpochKey {
                    family_id: family.family_id,
                    epoch: 1,
                    bytes: epoch_one,
                },
            ),
            (
                2,
                VerifiedEpochKey {
                    family_id: family.family_id,
                    epoch: 2,
                    bytes: epoch_two,
                },
            ),
        ]);
        let ready =
            ReadyFamilySession::from_store_with_keys(&store, family, keys.clone(), [0; 32], false)
                .unwrap();
        assert_eq!(ready.observed_cursor(), 9);
        assert_eq!(ready.active_epoch(), 2);
        assert!(
            ReadyFamilySession::from_store_with_keys(
                &store,
                family,
                BTreeMap::from([(1, keys[&1].clone())]),
                [0; 32],
                false,
            )
            .is_err()
        );
        let mut wrong = keys;
        wrong.get_mut(&2).unwrap().bytes[0] ^= 1;
        assert!(
            ReadyFamilySession::from_store_with_keys(&store, family, wrong, [0; 32], false)
                .is_err()
        );
    }
}
