//! Data-ready replay from a durable signed log and manifest-bound objects.

use std::collections::BTreeMap;

use crate::{
    batch,
    cbor::{self, Value},
    control_chain::{self, ControlChain},
    crypto,
    enrollment::EnrollmentAttempt,
    grant, membership,
    operation::NewOperation,
    operation::Operation,
    projection::{self, Projection, VerifiedEpochKey},
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
        replay_promotion(
            &history.genesis_bytes,
            &objects,
            family.family_id,
            genesis.relay_id(),
            genesis.manager_device_id(),
            &initial_epoch_key,
            &mut projection,
        )?;
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

fn replay_promotion(
    genesis_bytes: &[u8],
    objects: &BTreeMap<[u8; 16], Vec<u8>>,
    family_id: [u8; 16],
    relay_id: [u8; 32],
    manager_id: [u8; 16],
    epoch_key: &[u8; 32],
    projection: &mut Projection,
) -> Result<(), Error> {
    let genesis = cbor::decode(genesis_bytes)?;
    let Value::Map(root) = genesis else {
        return Err(Error::Invalid("genesis not map"));
    };
    let Value::Map(unsigned) = &root[0].1 else {
        return Err(Error::Invalid("unsigned genesis not map"));
    };
    let Value::Array(signed_objects) = &unsigned[9].1 else {
        return Err(Error::Invalid("genesis object list not array"));
    };
    let Value::Array(first_object) = &signed_objects[0] else {
        return Err(Error::Invalid("promotion manifest entry not array"));
    };
    let manifest_id = fixed_bytes::<16>(&first_object[1])?;
    let manifest_bytes = objects
        .get(&manifest_id)
        .ok_or(Error::Invalid("promotion manifest missing"))?;
    let manifest = cbor::decode(manifest_bytes)?;
    let Value::Map(fields) = manifest else {
        return Err(Error::Invalid("promotion manifest not map"));
    };
    if fields.len() != 6
        || fields
            .iter()
            .enumerate()
            .any(|(i, (key, _))| *key != i as u64 + 1)
        || fields[0].1 != Value::Integer(1)
        || fixed_bytes::<16>(&fields[1].1)? != family_id
        || fixed_bytes::<32>(&fields[2].1)? != relay_id
        || fixed_bytes::<16>(&fields[3].1)? != manifest_id
    {
        return Err(Error::Invalid("promotion manifest context"));
    }
    let watermark = positive_or_zero(&fields[4].1)?;
    let Value::Array(rows) = &fields[5].1 else {
        return Err(Error::Invalid("promotion rows not array"));
    };
    if rows.len() + 1 != signed_objects.len() || rows.len() > 16_383 {
        return Err(Error::Invalid("promotion object count"));
    }
    let signed_chunk_ids = signed_objects[1..]
        .iter()
        .map(|entry| {
            let Value::Array(fields) = entry else {
                return Err(Error::Invalid("signed chunk row not array"));
            };
            fixed_bytes::<16>(&fields[1])
        })
        .collect::<Result<std::collections::BTreeSet<_>, Error>>()?;
    let mut referenced = std::collections::BTreeSet::new();
    let mut operations = Vec::new();
    let mut next = 1u64;
    for (index, row) in rows.iter().enumerate() {
        let Value::Array(parts) = row else {
            return Err(Error::Invalid("promotion chunk row not array"));
        };
        if parts.len() != 6 || positive_or_zero(&parts[0])? != index as u64 {
            return Err(Error::Invalid("promotion chunk index"));
        }
        let object_id = fixed_bytes::<16>(&parts[1])?;
        if !referenced.insert(object_id) || !signed_chunk_ids.contains(&object_id) {
            return Err(Error::Invalid("promotion chunk not in signed genesis"));
        }
        let object = objects
            .get(&object_id)
            .ok_or(Error::Invalid("promotion chunk missing"))?;
        if fixed_bytes::<32>(&parts[2])? != crypto::hash("object", object)?
            || positive_or_zero(&parts[3])? != object.len() as u64
            || positive_or_zero(&parts[4])? != next
        {
            return Err(Error::Invalid("promotion chunk hash or range"));
        }
        let last = positive_or_zero(&parts[5])?;
        if last < next {
            return Err(Error::Invalid("promotion chunk empty range"));
        }
        let Value::Map(chunk) = cbor::decode(object)? else {
            return Err(Error::Invalid("promotion chunk not map"));
        };
        if chunk.len() != 3
            || chunk[0].0 != 1
            || chunk[1].0 != 2
            || chunk[2].0 != 3
            || chunk[0].1 != Value::Integer(1)
        {
            return Err(Error::Invalid("promotion chunk shape"));
        }
        let Value::Array(header) = &chunk[1].1 else {
            return Err(Error::Invalid("promotion header not array"));
        };
        if header.len() != 6
            || fixed_bytes::<16>(&header[0])? != family_id
            || fixed_bytes::<32>(&header[1])? != relay_id
            || fixed_bytes::<16>(&header[2])? != manifest_id
            || positive_or_zero(&header[3])? != index as u64
            || header[4] != Value::Integer(1)
        {
            return Err(Error::Invalid("promotion header context"));
        }
        let nonce = fixed_bytes::<24>(&header[5])?;
        let Value::Bytes(ciphertext) = &chunk[2].1 else {
            return Err(Error::Invalid("promotion ciphertext not bytes"));
        };
        let aad = crypto::hash("promotion-aad", &cbor::encode(&chunk[1].1)?)?;
        let plaintext = crypto::open(epoch_key, &nonce, &aad, ciphertext)?;
        if plaintext.len() > 256 * 1024 {
            return Err(Error::Invalid("promotion plaintext too large"));
        }
        let Value::Array(encoded_ops) = cbor::decode(&plaintext)? else {
            return Err(Error::Invalid("promotion plaintext not array"));
        };
        if encoded_ops.is_empty()
            || encoded_ops.len() > 256
            || encoded_ops.len() as u64 != last - next + 1
        {
            return Err(Error::Invalid("promotion operation count"));
        }
        for encoded in encoded_ops {
            let Value::Bytes(bytes) = encoded else {
                return Err(Error::Invalid("promotion operation not bytes"));
            };
            operations.push(
                Operation::decode_bound(&bytes, &family_id, &manager_id)
                    .map_err(|_| Error::Invalid("promotion operation invalid"))?,
            );
        }
        next = last
            .checked_add(1)
            .ok_or(Error::Invalid("promotion index overflow"))?;
    }
    if referenced != signed_chunk_ids || watermark != next - 1 {
        return Err(Error::Invalid("promotion history incomplete"));
    }
    projection.apply_promotion(&operations)?;
    Ok(())
}

fn fixed_bytes<const N: usize>(value: &Value) -> Result<[u8; N], Error> {
    let Value::Bytes(bytes) = value else {
        return Err(Error::Invalid("expected bytes"));
    };
    bytes
        .as_slice()
        .try_into()
        .map_err(|_| Error::Invalid("byte length"))
}

fn positive_or_zero(value: &Value) -> Result<u64, Error> {
    let Value::Integer(number) = value else {
        return Err(Error::Invalid("expected integer"));
    };
    (*number)
        .try_into()
        .map_err(|_| Error::Invalid("negative or large integer"))
}
