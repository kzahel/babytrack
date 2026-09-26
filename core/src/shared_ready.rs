//! Data-ready replay from a durable signed log and manifest-bound objects.

use std::collections::BTreeMap;

use crate::{
    batch,
    cbor::{self, Value},
    control_chain::{self, ControlChain},
    crypto,
    enrollment::EnrollmentAttempt,
    grant, membership,
    projection::{self, Projection, VerifiedEpochKey},
    shared_history::{self, PublicHistorySession},
    sqlite_store::{self, FamilyHandle, PreparedBatch, SqliteStore},
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
        if key.epoch != 1 {
            return Err(Error::Invalid(
                "admission after rotation needs historical keyring delivery",
            ));
        }
        Self::from_store_with_initial_key(store, family, key.bytes, agreement_private, false)
    }

    fn from_store_with_initial_key(
        store: &SqliteStore,
        family: FamilyHandle,
        initial_epoch_key: [u8; 32],
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
        verify_manifest(&history.genesis_bytes, &objects)?;
        let mut chain =
            ControlChain::from_genesis(&history.genesis_bytes, history.relay_public_key)?;
        let first_key = chain.verify_initial_epoch_key(&initial_epoch_key)?;
        let mut keys = BTreeMap::from([(1, first_key)]);
        let mut projection = Projection::new(family.family_id);
        projection.advance_control(1)?;
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
                        let keyring = objects
                            .get(&rotation.keyring_id())
                            .ok_or(Error::Invalid("rotation keyring object missing"))?;
                        let membership = chain
                            .membership_check(&transition_id)
                            .ok_or(Error::Invalid("rotation membership missing"))?;
                        let membership_object = objects
                            .get(&membership.object_id())
                            .ok_or(Error::Invalid("rotation membership object missing"))?;
                        let rotated = chain.open_rotation_for(
                            &transition_id,
                            family.device_id,
                            &manager_agreement_private,
                            &grants,
                            keyring,
                            membership_object,
                        )?;
                        for (epoch, prior) in &keys {
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

    pub fn projection(&self) -> &Projection {
        &self.projection
    }

    pub fn observed_cursor(&self) -> u64 {
        self.observed_cursor
    }

    pub fn active_epoch(&self) -> u32 {
        self.active_epoch
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

fn verify_manifest(
    committed_bytes: &[u8],
    objects: &BTreeMap<[u8; 16], Vec<u8>>,
) -> Result<(), Error> {
    let value = cbor::decode(committed_bytes)?;
    let Value::Map(root) = value else {
        return Err(Error::Invalid("control not map"));
    };
    let Value::Map(unsigned) = &root[0].1 else {
        return Err(Error::Invalid("unsigned control not map"));
    };
    let Value::Array(manifest) = &unsigned[9].1 else {
        return Err(Error::Invalid("manifest not array"));
    };
    for entry in manifest {
        let Value::Array(fields) = entry else {
            return Err(Error::Invalid("manifest entry not array"));
        };
        let Value::Bytes(id) = &fields[1] else {
            return Err(Error::Invalid("manifest object ID not bytes"));
        };
        let id: [u8; 16] = id
            .as_slice()
            .try_into()
            .map_err(|_| Error::Invalid("manifest object ID length"))?;
        let object = objects
            .get(&id)
            .ok_or(Error::Invalid("committed object not downloaded"))?;
        if fields[3] != Value::Integer(object.len() as i128)
            || fields[2] != Value::Bytes(crypto::hash("object", object)?.to_vec())
        {
            return Err(Error::Invalid(
                "committed object bytes differ from manifest",
            ));
        }
    }
    Ok(())
}
