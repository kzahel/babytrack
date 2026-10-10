use super::*;

/// Data view rooted in a verified epoch-one key. Rotation objects and the
/// complete keyring are checked by the shared Rust authority path before a
/// later epoch or its records become usable.
#[wasm_bindgen]
pub struct WasmInitialFamily {
    genesis: Vec<u8>,
    relay_public_key: [u8; 32],
    epoch_key: [u8; 32],
    admitted_keys: Vec<[u8; 32]>,
    agreement_private: Option<[u8; 32]>,
    holder_device_id: Option<[u8; 16]>,
    objects: BTreeMap<[u8; 16], Vec<u8>>,
    ready: Option<InitialReady>,
}

struct InitialReady {
    chain: ControlChain,
    projection: Projection,
    overlay: Option<Projection>,
    overlay_operations: Vec<Operation>,
    keys: BTreeMap<u32, VerifiedEpochKey>,
}

#[wasm_bindgen]
impl WasmInitialFamily {
    #[wasm_bindgen(constructor)]
    pub fn new(genesis: &[u8], relay_public_key: &[u8], epoch_key: &[u8]) -> Result<Self, JsError> {
        Self::with_admission_keys(genesis, relay_public_key, epoch_key)
    }

    pub fn with_admission_keys(
        genesis: &[u8],
        relay_public_key: &[u8],
        keys: &[u8],
    ) -> Result<Self, JsError> {
        let relay_public_key = fixed(relay_public_key, "relay public key")?;
        if keys.is_empty() || !keys.len().is_multiple_of(32) || keys.len() > 32 * 1024 {
            return Err(JsError::new("admission key history has invalid length"));
        }
        let admitted_keys = keys
            .chunks_exact(32)
            .map(|key| fixed(key, "epoch key"))
            .collect::<Result<Vec<[u8; 32]>, _>>()?;
        let epoch_key = admitted_keys[0];
        ControlChain::from_genesis(genesis, relay_public_key)
            .map_err(debug_error)?
            .verify_initial_epoch_key(&epoch_key)
            .map_err(debug_error)?;
        Ok(Self {
            genesis: genesis.to_vec(),
            relay_public_key,
            epoch_key,
            admitted_keys,
            agreement_private: None,
            holder_device_id: None,
            objects: BTreeMap::new(),
            ready: None,
        })
    }

    /// The local holder's agreement secret is needed to open its addressed
    /// grant after a rotation. It never leaves this browser process.
    pub fn set_agreement_private(
        &mut self,
        device_id: &[u8],
        private: &[u8],
    ) -> Result<(), JsError> {
        if self.ready.is_some() {
            return Err(JsError::new("ready view already initialized"));
        }
        self.agreement_private = Some(fixed(private, "agreement private key")?);
        self.holder_device_id = Some(fixed(device_id, "holder device ID")?);
        Ok(())
    }

    pub fn add_object(&mut self, object_id: &[u8], object_bytes: &[u8]) -> Result<(), JsError> {
        if self.ready.is_some() {
            return Err(JsError::new("ready view already initialized"));
        }
        let object_id = fixed(object_id, "object ID")?;
        if self.objects.contains_key(&object_id) {
            return Err(JsError::new("duplicate manifest object"));
        }
        self.objects.insert(object_id, object_bytes.to_vec());
        Ok(())
    }

    pub fn finish(&mut self) -> Result<(), JsError> {
        if self.ready.is_some() {
            return Err(JsError::new("ready view already initialized"));
        }
        let (chain, projection, key) = ready_replay::initial_epoch_projection(
            &self.genesis,
            self.relay_public_key,
            self.epoch_key,
            &self.objects,
        )
        .map_err(debug_error)?;
        let mut keys = BTreeMap::from([(1, key)]);
        for (index, bytes) in self.admitted_keys.iter().enumerate().skip(1) {
            let epoch = u32::try_from(index + 1).map_err(debug_error)?;
            keys.insert(
                epoch,
                ready_replay::saved_history_key(chain.family_id(), epoch, *bytes),
            );
        }
        self.ready = Some(InitialReady {
            chain,
            projection,
            overlay: None,
            overlay_operations: Vec::new(),
            keys,
        });
        Ok(())
    }

    pub fn apply_batch(
        &mut self,
        envelope_bytes: &[u8],
        receipt_bytes: &[u8],
    ) -> Result<bool, JsError> {
        let ready = self
            .ready
            .as_mut()
            .ok_or_else(|| JsError::new("manifest objects not yet verified"))?;
        let mut chain = ready.chain.clone();
        let signed = chain
            .apply_public_batch(envelope_bytes, receipt_bytes)
            .map_err(debug_error)?;
        let mut projection = ready.projection.clone();
        let key = ready
            .keys
            .get(&signed.header().epoch)
            .ok_or_else(|| JsError::new("verified batch epoch key is missing"))?;
        let outcome = projection
            .apply_authorized_signed(&signed, key, chain.last_global_cursor())
            .map_err(debug_error)?;
        ready.chain = chain;
        ready.projection = projection;
        ready.overlay = None;
        ready.overlay_operations.clear();
        Ok(matches!(outcome, Outcome::Applied))
    }

    /// Replay a signed authority change. Rotation cannot advance the ready
    /// view without its committed grant, keyring, and membership objects.
    pub fn apply_control(&mut self, committed_bytes: &[u8]) -> Result<(), JsError> {
        let ready = self
            .ready
            .as_mut()
            .ok_or_else(|| JsError::new("manifest objects not yet verified"))?;
        let mut chain = ready.chain.clone();
        let previous_epoch = chain.epoch().map_err(debug_error)?;
        chain.apply_control(committed_bytes).map_err(debug_error)?;
        ready_replay::verify_manifest(committed_bytes, &self.objects).map_err(debug_error)?;
        let transition_id = ready_replay::manifest_objects(committed_bytes)
            .map_err(debug_error)?
            .first()
            .map(|object| object.transition_id);
        let mut keys = ready.keys.clone();
        let next_epoch = chain.epoch().map_err(debug_error)?;
        if next_epoch > previous_epoch {
            if next_epoch != previous_epoch + 1 {
                return Err(JsError::new("rotation skipped an epoch"));
            }
            let (transition_id, rotation) = chain
                .rotation_for_epoch(next_epoch)
                .ok_or_else(|| JsError::new("verified rotation is missing"))?;
            let keyring = self
                .objects
                .get(&rotation.keyring_id())
                .ok_or_else(|| JsError::new("rotation keyring is missing"))?;
            let membership = chain
                .membership_check(&transition_id)
                .ok_or_else(|| JsError::new("rotation membership is missing"))?;
            let membership_object = self
                .objects
                .get(&membership.object_id())
                .ok_or_else(|| JsError::new("rotation membership object is missing"))?;
            let agreement_private = self
                .agreement_private
                .as_ref()
                .ok_or_else(|| JsError::new("local rotation agreement key is missing"))?;
            let grants = rotation
                .grant_ids()
                .into_iter()
                .map(|id| {
                    self.objects
                        .get(&id)
                        .cloned()
                        .map(|object| (id, object))
                        .ok_or_else(|| JsError::new("rotation grant object is missing"))
                })
                .collect::<Result<Vec<_>, _>>()?;
            let rotated = if let Some(known) = keys.get(&next_epoch) {
                chain.open_rotation_from_known_epoch_key(
                    &transition_id,
                    known,
                    keyring,
                    membership_object,
                )
            } else {
                chain.open_rotation_for(
                    &transition_id,
                    self.holder_device_id
                        .ok_or_else(|| JsError::new("rotation holder device ID is missing"))?,
                    agreement_private,
                    &grants,
                    keyring,
                    membership_object,
                )
            }
            .map_err(debug_error)?;
            for (epoch, prior) in keys.iter().filter(|(epoch, _)| **epoch < next_epoch) {
                if rotated.earlier(*epoch) != Some(prior) {
                    return Err(JsError::new("rotation keyring differs from saved history"));
                }
            }
            keys.insert(next_epoch, rotated.current().clone());
        } else if let Some(transition_id) = transition_id
            && let Some(membership) = chain.membership_check(&transition_id)
        {
            let object = self
                .objects
                .get(&membership.object_id())
                .ok_or_else(|| JsError::new("membership object is missing"))?;
            let key = keys
                .get(&next_epoch)
                .ok_or_else(|| JsError::new("membership epoch key is missing"))?;
            membership.verify(object, key).map_err(debug_error)?;
        }
        let mut projection = ready.projection.clone();
        projection
            .advance_control(chain.last_global_cursor())
            .map_err(debug_error)?;
        ready.chain = chain;
        ready.keys = keys;
        ready.projection = projection;
        ready.overlay = None;
        ready.overlay_operations.clear();
        Ok(())
    }

    pub fn verify_admission_history(&self) -> Result<(), JsError> {
        let ready = self
            .ready
            .as_ref()
            .ok_or_else(|| JsError::new("manifest objects not yet verified"))?;
        if self.admitted_keys.len() > ready.chain.epoch().map_err(debug_error)? as usize {
            return Err(JsError::new("saved admission key exceeds verified history"));
        }
        Ok(())
    }

    /// Add one durable unsent operation to the local preview without advancing
    /// the verified relay cursor. The browser rebuilds this from saved work.
    pub fn preview_one(&mut self, operation_bytes: &[u8], device_id: &[u8]) -> Result<(), JsError> {
        let ready = self
            .ready
            .as_mut()
            .ok_or_else(|| JsError::new("manifest objects not yet verified"))?;
        let device_id = fixed(device_id, "device ID")?;
        ready
            .chain
            .active_signing_public(device_id)
            .map_err(debug_error)?;
        let operation =
            Operation::decode_bound(operation_bytes, &ready.chain.family_id(), &device_id)
                .map_err(debug_error)?;
        let mut operations = ready.overlay_operations.clone();
        operations.push(operation);
        let overlay = ready
            .projection
            .with_local_overlay(&operations)
            .map_err(debug_error)?;
        ready.overlay_operations = operations;
        ready.overlay = Some(overlay);
        Ok(())
    }

    pub fn last_cursor(&self) -> Result<u64, JsError> {
        Ok(self
            .ready
            .as_ref()
            .ok_or_else(|| JsError::new("manifest objects not yet verified"))?
            .projection
            .last_cursor())
    }

    pub fn head_hash(&self) -> Result<Vec<u8>, JsError> {
        Ok(self
            .ready
            .as_ref()
            .ok_or_else(|| JsError::new("manifest objects not yet verified"))?
            .chain
            .head_hash()
            .to_vec())
    }

    /// Seal one validated operation with a fresh core-generated identity and
    /// nonce. The browser must durably save these exact bytes before POST.
    pub fn prepare_one(
        &self,
        operation_bytes: &[u8],
        device_id: &[u8],
        signing_seed: &[u8],
    ) -> Result<Vec<u8>, JsError> {
        let ready = self
            .ready
            .as_ref()
            .ok_or_else(|| JsError::new("manifest objects not yet verified"))?;
        let device_id = fixed(device_id, "device ID")?;
        let signing_seed = fixed(signing_seed, "signing seed")?;
        if ready
            .chain
            .active_signing_public(device_id)
            .map_err(debug_error)?
            != crypto::signing_public_key(&signing_seed)
        {
            return Err(JsError::new(
                "active device authority differs from credential",
            ));
        }
        let operation =
            Operation::decode_bound(operation_bytes, &ready.chain.family_id(), &device_id)
                .map_err(debug_error)?;
        ready
            .projection
            .with_local_overlay(&[operation])
            .map_err(debug_error)?;
        let plaintext = cbor::encode(&Value::Array(vec![Value::Bytes(operation_bytes.to_vec())]))
            .map_err(debug_error)?;
        let mut batch_id = [0u8; 16];
        let mut nonce = [0u8; 24];
        getrandom::fill(&mut batch_id).map_err(debug_error)?;
        batch_id[6] = (batch_id[6] & 0x0f) | 0x40;
        batch_id[8] = (batch_id[8] & 0x3f) | 0x80;
        getrandom::fill(&mut nonce).map_err(debug_error)?;
        let header = batch::Header {
            minor: 0,
            family_id: ready.chain.family_id(),
            relay_id: ready.chain.relay_id(),
            control_head: ready.chain.head_hash(),
            epoch: ready.chain.epoch().map_err(debug_error)?,
            batch_id,
            author_device_id: device_id,
            device_sequence: ready
                .chain
                .next_sequence_for(device_id)
                .map_err(debug_error)?,
            nonce,
            plaintext_len: plaintext
                .len()
                .try_into()
                .map_err(|_| JsError::new("operation too large"))?,
        };
        let epoch = ready.chain.epoch().map_err(debug_error)?;
        let key = ready
            .keys
            .get(&epoch)
            .ok_or_else(|| JsError::new("active epoch key is missing"))?;
        Ok(batch::seal(
            &header,
            &[operation_bytes.to_vec()],
            &key.bytes_for_storage(),
            &signing_seed,
        )
        .map_err(debug_error)?
        .envelope_bytes)
    }

    pub fn field_cbor(&self, record_id: &[u8], field_id: u64) -> Result<Vec<u8>, JsError> {
        let record_id = fixed(record_id, "record ID")?;
        let ready = self
            .ready
            .as_ref()
            .ok_or_else(|| JsError::new("manifest objects not yet verified"))?;
        Ok(ready
            .overlay
            .as_ref()
            .unwrap_or(&ready.projection)
            .record(&record_id)
            .and_then(|record| record.field(field_id))
            .map_or_else(Vec::new, |field| field.canonical_bytes.clone()))
    }

    pub fn record_type(&self, record_id: &[u8]) -> Result<Option<String>, JsError> {
        let record_id = fixed(record_id, "record ID")?;
        let ready = self
            .ready
            .as_ref()
            .ok_or_else(|| JsError::new("manifest objects not yet verified"))?;
        Ok(ready
            .overlay
            .as_ref()
            .unwrap_or(&ready.projection)
            .record(&record_id)
            .map(|record| record.record_type.clone()))
    }

    /// Build any browser create or correction for the shared Family.
    pub fn action_operation(
        &self,
        device_id: &[u8],
        last_wall_ms: i64,
        last_counter: u32,
        action_json: &str,
        now_ms: i64,
    ) -> Result<Vec<u8>, JsError> {
        let action = web_actions::action::Action::parse(action_json).map_err(debug_error)?;
        let ready = self
            .ready
            .as_ref()
            .ok_or_else(|| JsError::new("Family not ready"))?;
        let projection = ready.overlay.as_ref().unwrap_or(&ready.projection);
        let target = match action.target() {
            Some(id) => Some(
                projection
                    .record(&web_actions::action::parse_id(id).map_err(debug_error)?)
                    .ok_or_else(|| JsError::new("correction target unavailable"))?,
            ),
            None => None,
        };
        let record = target.map_or(random_v7(now_ms)?, |record| record.id);
        web_actions::action::build(
            self.shared_identity(device_id, record, last_wall_ms, last_counter, now_ms)?,
            action,
            target,
        )
        .map_err(debug_error)
    }

    pub fn day_summary_json(
        &self,
        child_id: &[u8],
        start_utc_ms: i64,
        end_utc_ms: i64,
        through_utc_ms: i64,
    ) -> Result<String, JsError> {
        let ready = self
            .ready
            .as_ref()
            .ok_or_else(|| JsError::new("manifest objects not yet verified"))?;
        web_actions::day_summary(
            ready
                .overlay
                .as_ref()
                .unwrap_or(&ready.projection)
                .records(),
            fixed(child_id, "child ID")?,
            day_summary::DayWindow {
                start_utc_ms,
                end_utc_ms,
                through_utc_ms,
            },
        )
        .map_err(debug_error)
    }

    pub fn analysis_csv(&self) -> Result<Vec<u8>, JsError> {
        let ready = self
            .ready
            .as_ref()
            .ok_or_else(|| JsError::new("manifest objects not yet verified"))?;
        Ok(analysis_csv::export(
            ready.chain.family_id(),
            ready
                .overlay
                .as_ref()
                .unwrap_or(&ready.projection)
                .records(),
        ))
    }

    pub fn snapshot_json(&self) -> Result<String, JsError> {
        let ready = self
            .ready
            .as_ref()
            .ok_or_else(|| JsError::new("manifest objects not yet verified"))?;
        Ok(web_actions::shared_snapshot(
            ready.overlay.as_ref().unwrap_or(&ready.projection),
        ))
    }

    /// Export the locally held current state, including pending browser
    /// edits already previewed in the ready overlay.
    pub fn readable_file(&self, snapshot_utc_ms: i64, known_gap: bool) -> Result<Vec<u8>, JsError> {
        let ready = self
            .ready
            .as_ref()
            .ok_or_else(|| JsError::new("manifest objects not yet verified"))?;
        let projection = ready.overlay.as_ref().unwrap_or(&ready.projection);
        portable_file::encode_readable(
            ready.chain.family_id(),
            snapshot_utc_ms,
            Some(ready.chain.last_global_cursor()),
            known_gap || !projection.inert_batches().is_empty(),
            projection.records(),
        )
        .map_err(debug_error)
    }

    fn shared_identity(
        &self,
        device_id: &[u8],
        record: [u8; 16],
        last_wall_ms: i64,
        last_counter: u32,
        now_ms: i64,
    ) -> Result<web_actions::Identity, JsError> {
        let ready = self
            .ready
            .as_ref()
            .ok_or_else(|| JsError::new("Family not ready"))?;
        let family = ready.chain.family_id();
        let device = fixed(device_id, "device ID")?;
        ready
            .chain
            .active_signing_public(device)
            .map_err(debug_error)?;
        let last = (last_wall_ms >= 0).then_some(Hlc {
            wall_ms: last_wall_ms,
            counter: last_counter,
            device_id: device,
        });
        let mut clock = Clock::restore(family, device, last, None).map_err(debug_error)?;
        Ok(web_actions::Identity {
            family,
            device,
            operation: random_v7(now_ms)?,
            record,
            stamp: clock.next(now_ms).stamp,
        })
    }

    #[allow(clippy::too_many_arguments)] // Flat wasm boundary for browser form values.
    pub fn create_child_operation(
        &self,
        device_id: &[u8],
        last_wall_ms: i64,
        last_counter: u32,
        name: &str,
        birth_day: Option<i64>,
        sex: Option<u8>,
        now_ms: i64,
    ) -> Result<Vec<u8>, JsError> {
        web_actions::child(
            self.shared_identity(
                device_id,
                random_v7(now_ms)?,
                last_wall_ms,
                last_counter,
                now_ms,
            )?,
            name,
            birth_day,
            sex,
        )
        .map_err(debug_error)
    }

    #[allow(clippy::too_many_arguments)] // Flat wasm boundary for browser form values.
    pub fn log_diaper_operation(
        &self,
        device_id: &[u8],
        last_wall_ms: i64,
        last_counter: u32,
        child_id: &[u8],
        kind: u8,
        now_ms: i64,
        offset_minutes: i16,
    ) -> Result<Vec<u8>, JsError> {
        web_actions::diaper(
            self.shared_identity(
                device_id,
                random_v7(now_ms)?,
                last_wall_ms,
                last_counter,
                now_ms,
            )?,
            fixed(child_id, "child ID")?,
            kind,
            now_ms,
            offset_minutes,
        )
        .map_err(debug_error)
    }

    #[allow(clippy::too_many_arguments)] // Flat wasm boundary for browser form values.
    pub fn log_bottle_operation(
        &self,
        device_id: &[u8],
        last_wall_ms: i64,
        last_counter: u32,
        child_id: &[u8],
        ml: u32,
        content: u8,
        now_ms: i64,
        offset_minutes: i16,
    ) -> Result<Vec<u8>, JsError> {
        web_actions::bottle(
            self.shared_identity(
                device_id,
                random_v7(now_ms)?,
                last_wall_ms,
                last_counter,
                now_ms,
            )?,
            fixed(child_id, "child ID")?,
            ml,
            content,
            now_ms,
            offset_minutes,
        )
        .map_err(debug_error)
    }

    #[allow(clippy::too_many_arguments)] // Flat wasm boundary for browser form values.
    pub fn log_note_operation(
        &self,
        device_id: &[u8],
        last_wall_ms: i64,
        last_counter: u32,
        child_id: &[u8],
        note: &str,
        now_ms: i64,
        offset_minutes: i16,
    ) -> Result<Vec<u8>, JsError> {
        web_actions::note(
            self.shared_identity(
                device_id,
                random_v7(now_ms)?,
                last_wall_ms,
                last_counter,
                now_ms,
            )?,
            fixed(child_id, "child ID")?,
            note,
            now_ms,
            offset_minutes,
        )
        .map_err(debug_error)
    }

    pub fn log_breast_operation(
        &self,
        device_id: &[u8],
        last_wall_ms: i64,
        last_counter: u32,
        child_id: &[u8],
        segments_json: &str,
        now_ms: i64,
    ) -> Result<Vec<u8>, JsError> {
        web_actions::breast(
            self.shared_identity(
                device_id,
                random_v7(now_ms)?,
                last_wall_ms,
                last_counter,
                now_ms,
            )?,
            fixed(child_id, "child ID")?,
            segments_json,
        )
        .map_err(debug_error)
    }

    #[allow(clippy::too_many_arguments)] // Flat wasm boundary for browser form values.
    pub fn edit_breast_operation(
        &self,
        device_id: &[u8],
        last_wall_ms: i64,
        last_counter: u32,
        child_id: &[u8],
        activity_id: &[u8],
        segments_json: &str,
        now_ms: i64,
    ) -> Result<Vec<u8>, JsError> {
        let record_id = fixed(activity_id, "activity ID")?;
        let ready = self
            .ready
            .as_ref()
            .ok_or_else(|| JsError::new("Family not ready"))?;
        let target = ready
            .overlay
            .as_ref()
            .unwrap_or(&ready.projection)
            .record(&record_id)
            .ok_or_else(|| JsError::new("breast feed target unavailable"))?;
        web_actions::edit_breast(
            self.shared_identity(device_id, record_id, last_wall_ms, last_counter, now_ms)?,
            fixed(child_id, "child ID")?,
            target,
            segments_json,
        )
        .map_err(debug_error)
    }

    pub fn start_sleep_operation(
        &self,
        device_id: &[u8],
        last_wall_ms: i64,
        last_counter: u32,
        child_id: &[u8],
        now_ms: i64,
        offset_minutes: i16,
    ) -> Result<Vec<u8>, JsError> {
        web_actions::start_sleep(
            self.shared_identity(
                device_id,
                random_v7(now_ms)?,
                last_wall_ms,
                last_counter,
                now_ms,
            )?,
            fixed(child_id, "child ID")?,
            now_ms,
            offset_minutes,
        )
        .map_err(debug_error)
    }

    #[allow(clippy::too_many_arguments)] // Flat wasm boundary for browser form values.
    pub fn stop_sleep_operation(
        &self,
        device_id: &[u8],
        last_wall_ms: i64,
        last_counter: u32,
        child_id: &[u8],
        activity_id: &[u8],
        now_ms: i64,
        offset_minutes: i16,
    ) -> Result<Vec<u8>, JsError> {
        let record_id = fixed(activity_id, "activity ID")?;
        let ready = self
            .ready
            .as_ref()
            .ok_or_else(|| JsError::new("Family not ready"))?;
        let target = ready
            .overlay
            .as_ref()
            .unwrap_or(&ready.projection)
            .record(&record_id)
            .ok_or_else(|| JsError::new("sleep target unavailable"))?;
        web_actions::stop_sleep(
            self.shared_identity(device_id, record_id, last_wall_ms, last_counter, now_ms)?,
            fixed(child_id, "child ID")?,
            target,
            now_ms,
            offset_minutes,
        )
        .map_err(debug_error)
    }
}
