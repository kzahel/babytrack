use super::*;

/// outbox handling are separate steps, but every committed cursor is checked
/// against the same Rust control chain used by native clients and the relay.
#[wasm_bindgen]
pub struct WasmPublicFamily {
    chain: ControlChain,
    relay_public_key: [u8; 32],
    prefix: BTreeMap<u64, ControlChain>,
}

/// Core-decoded relay page; JavaScript owns transport and persistence.
#[wasm_bindgen]
pub struct WasmLogPage {
    page: LogPage,
}

#[wasm_bindgen]
impl WasmLogPage {
    #[wasm_bindgen(constructor)]
    pub fn new(bytes: &[u8], family_id: &[u8], after: u64) -> Result<Self, JsError> {
        Ok(Self {
            page: LogPage::decode(bytes, fixed(family_id, "Family ID")?, after)
                .map_err(debug_error)?,
        })
    }

    pub fn len(&self) -> usize {
        self.page.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.page.entries.is_empty()
    }

    pub fn has_more(&self) -> bool {
        self.page.has_more
    }

    pub fn next_after(&self) -> u64 {
        self.page.next_after
    }

    pub fn entry_cursor(&self, index: usize) -> Result<u64, JsError> {
        Ok(self.entry(index)?.cursor)
    }

    pub fn entry_kind(&self, index: usize) -> Result<u8, JsError> {
        Ok(self.entry(index)?.kind)
    }

    pub fn entry_bytes(&self, index: usize) -> Result<Vec<u8>, JsError> {
        Ok(self.entry(index)?.committed_bytes.clone())
    }

    fn entry(&self, index: usize) -> Result<&sync_wire::LogEntry, JsError> {
        self.page
            .entries
            .get(index)
            .ok_or_else(|| JsError::new("log entry index outside page"))
    }
}

#[wasm_bindgen]
impl WasmPublicFamily {
    #[wasm_bindgen(constructor)]
    pub fn new(genesis_bytes: &[u8], relay_public_key: &[u8]) -> Result<Self, JsError> {
        let relay_public_key = fixed(relay_public_key, "relay public key")?;
        let chain =
            ControlChain::from_genesis(genesis_bytes, relay_public_key).map_err(debug_error)?;
        let prefix = BTreeMap::from([(chain.last_global_cursor(), chain.clone())]);
        Ok(Self {
            chain,
            relay_public_key,
            prefix,
        })
    }

    pub fn apply_control(&mut self, committed_bytes: &[u8]) -> Result<(), JsError> {
        self.chain
            .apply_control(committed_bytes)
            .map_err(debug_error)?;
        self.prefix
            .insert(self.chain.last_global_cursor(), self.chain.clone());
        Ok(())
    }

    pub fn apply_batch(
        &mut self,
        envelope_bytes: &[u8],
        receipt_bytes: &[u8],
    ) -> Result<(), JsError> {
        self.chain
            .apply_public_batch(envelope_bytes, receipt_bytes)
            .map(|_| ())
            .map_err(debug_error)?;
        self.prefix
            .insert(self.chain.last_global_cursor(), self.chain.clone());
        Ok(())
    }

    pub fn family_id(&self) -> Vec<u8> {
        self.chain.family_id().to_vec()
    }

    pub fn relay_id(&self) -> Vec<u8> {
        self.chain.relay_id().to_vec()
    }

    pub fn initial_manager_device_id(&self) -> Vec<u8> {
        self.chain.initial_manager_device_id().to_vec()
    }

    pub fn last_cursor(&self) -> u64 {
        self.chain.last_global_cursor()
    }

    pub fn head_hash(&self) -> Vec<u8> {
        self.chain.head_hash().to_vec()
    }

    /// Open a recipient's committed epoch-one grant. Kept for older callers.
    pub fn open_initial_grant(
        &self,
        device_id: &[u8],
        agreement_private: &[u8],
        grant_object: &[u8],
    ) -> Result<Vec<u8>, JsError> {
        if self.chain.epoch().map_err(debug_error)? != 1 {
            return Err(JsError::new("rotated Family requires a verified keyring"));
        }
        let device_id = fixed(device_id, "device ID")?;
        self.chain
            .active_signing_public(device_id)
            .map_err(debug_error)?;
        let grant = self
            .chain
            .initial_admission_grant(&device_id)
            .ok_or_else(|| JsError::new("recipient has no committed admission grant"))?;
        let key = grant
            .open(
                grant_object,
                &fixed(agreement_private, "agreement private key")?,
            )
            .map_err(debug_error)?;
        self.chain
            .verify_initial_epoch_key(&key.bytes_for_storage())
            .map_err(debug_error)?;
        Ok(key.bytes_for_storage().to_vec())
    }

    pub fn initial_grant_id(&self, device_id: &[u8]) -> Result<Vec<u8>, JsError> {
        let device_id = fixed(device_id, "device ID")?;
        Ok(self
            .chain
            .initial_admission_grant(&device_id)
            .ok_or_else(|| JsError::new("recipient has no committed admission grant"))?
            .grant_id()
            .to_vec())
    }

    /// Keep the contiguous data pin intact while checking signed sparse
    /// controls for a later removal.
    pub fn removal_probe(&self, device_id: &[u8]) -> Result<WasmRemovalProbe, JsError> {
        Ok(WasmRemovalProbe {
            inner: removal_probe::RemovalControlProbe::new(
                self.chain.clone(),
                fixed(device_id, "device ID")?,
            )
            .map_err(debug_error)?,
        })
    }

    /// IDs of the signed admission grant and, for a rotated epoch, its
    /// keyring and membership objects. The adapter fetches manifest-bound
    /// bytes before asking the core to open any key.
    pub fn admission_object_ids(&self, device_id: &[u8]) -> Result<Vec<u8>, JsError> {
        let device_id = fixed(device_id, "device ID")?;
        let grant = self
            .chain
            .initial_admission_grant(&device_id)
            .ok_or_else(|| JsError::new("recipient has no committed admission grant"))?;
        let mut ids = grant.grant_id().to_vec();
        if grant.epoch() > 1 {
            let (transition_id, rotation) = self
                .chain
                .rotation_for_epoch(grant.epoch())
                .ok_or_else(|| JsError::new("admission epoch rotation is missing"))?;
            let membership = self
                .chain
                .membership_check(&transition_id)
                .ok_or_else(|| JsError::new("admission epoch membership is missing"))?;
            ids.extend_from_slice(&rotation.keyring_id());
            ids.extend_from_slice(&membership.object_id());
        }
        Ok(ids)
    }

    /// Return epoch keys in order only after the recipient grant and, when
    /// needed, the committed rotation keyring and membership are verified.
    pub fn open_admission_keys(
        &self,
        device_id: &[u8],
        agreement_private: &[u8],
        grant_object: &[u8],
        keyring_object: &[u8],
        membership_object: &[u8],
    ) -> Result<Vec<u8>, JsError> {
        let device_id = fixed(device_id, "device ID")?;
        self.chain
            .active_signing_public(device_id)
            .map_err(debug_error)?;
        let grant = self
            .chain
            .initial_admission_grant(&device_id)
            .ok_or_else(|| JsError::new("recipient has no committed admission grant"))?;
        let key = grant
            .open(
                grant_object,
                &fixed(agreement_private, "agreement private key")?,
            )
            .map_err(debug_error)?;
        if grant.epoch() == 1 {
            // The grant itself binds the key to the signed epoch commitment.
            return Ok(key.bytes_for_storage().to_vec());
        }
        let (transition_id, _) = self
            .chain
            .rotation_for_epoch(grant.epoch())
            .ok_or_else(|| JsError::new("admission epoch rotation is missing"))?;
        let recovered = self
            .chain
            .open_rotation_from_known_epoch_key(
                &transition_id,
                &key,
                keyring_object,
                membership_object,
            )
            .map_err(debug_error)?;
        let mut result = Vec::with_capacity(32 * grant.epoch() as usize);
        for epoch in 1..grant.epoch() {
            result.extend_from_slice(
                &recovered
                    .earlier(epoch)
                    .ok_or_else(|| JsError::new("admission history epoch is missing"))?
                    .bytes_for_storage(),
            );
        }
        result.extend_from_slice(&key.bytes_for_storage());
        Ok(result)
    }

    pub fn sign_get(
        &self,
        device_id: &[u8],
        signing_seed: &[u8],
        exact_path: &str,
        request_id: &[u8],
    ) -> Result<Vec<u8>, JsError> {
        let device_id = fixed(device_id, "device ID")?;
        let signing_seed = fixed(signing_seed, "signing seed")?;
        if self
            .chain
            .active_signing_public(device_id)
            .map_err(debug_error)?
            != crypto::signing_public_key(&signing_seed)
        {
            return Err(JsError::new("signing seed differs from active device"));
        }
        let family_hex: String = self
            .chain
            .family_id()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        if !exact_path.starts_with(&format!("/v1/families/{family_hex}/")) {
            return Err(JsError::new("read path belongs to another Family"));
        }
        sync_wire::sign_get_with_id(
            self.chain.family_id(),
            self.chain.relay_id(),
            device_id,
            &signing_seed,
            exact_path,
            fixed(request_id, "request ID")?,
        )
        .map_err(debug_error)
    }

    pub fn batch_id(&self, envelope: &[u8]) -> Result<Vec<u8>, JsError> {
        let value = cbor::decode_with_limits(
            envelope,
            cbor::Limits {
                max_bytes: 1024 * 1024,
                max_depth: 16,
            },
        )
        .map_err(debug_error)?;
        let Value::Map(fields) = value else {
            return Err(JsError::new("batch envelope not map"));
        };
        let Some((1, header)) = fields.first() else {
            return Err(JsError::new("batch header absent"));
        };
        Ok(
            batch::Header::decode(&cbor::encode(header).map_err(debug_error)?)
                .map_err(debug_error)?
                .batch_id
                .to_vec(),
        )
    }

    /// A stale batch can be resealed only after a relay-signed rejection is
    /// bound to this exact envelope and its stated authority prefix is saved.
    pub fn verified_stale_rejection(
        &self,
        envelope: &[u8],
        result_bytes: &[u8],
        device_id: &[u8],
    ) -> Result<bool, JsError> {
        let Some(receipt_bytes) = sync_wire::BatchResult::decode(result_bytes)
            .map_err(debug_error)?
            .receipt_bytes
        else {
            return Ok(false);
        };
        let receipt = match session::verify_rejected_receipt(&receipt_bytes, &self.relay_public_key)
        {
            Ok(receipt) => receipt,
            // An accepted result leaves the exact outbox bytes uncertain until
            // their accepted log entry is verified by ordinary replay.
            Err(_) => return Ok(false),
        };
        if receipt.reason != 1 {
            return Ok(false);
        }
        let device_id = fixed(device_id, "device ID")?;
        let signer = self
            .chain
            .active_signing_public(device_id)
            .map_err(debug_error)?;
        let signed = batch::verify_signed_envelope(
            envelope,
            &self.chain.family_id(),
            &self.chain.relay_id(),
            &signer,
        )
        .map_err(debug_error)?;
        let header = signed.header();
        let at_rejection = self
            .prefix
            .get(&receipt.cursor)
            .ok_or_else(|| JsError::new("rejection prefix is not saved"))?;
        if receipt.family_id != self.chain.family_id()
            || receipt.relay_id != self.chain.relay_id()
            || receipt.batch_id != header.batch_id
            || receipt.object_hash != signed.object_hash()
            || receipt.device_sequence != header.device_sequence
            || header.author_device_id != device_id
            || header.epoch >= at_rejection.epoch().map_err(debug_error)?
            || receipt.control_head != at_rejection.head_hash()
            || receipt.next_expected_sequence
                != at_rejection
                    .next_sequence_for(device_id)
                    .map_err(debug_error)?
            || self
                .chain
                .next_sequence_for(device_id)
                .map_err(debug_error)?
                < receipt.next_expected_sequence
        {
            return Err(JsError::new(
                "stale rejection differs from verified pending batch",
            ));
        }
        Ok(true)
    }
}

#[wasm_bindgen]
pub fn accepted_batch_receipt(result_bytes: &[u8]) -> Result<Vec<u8>, JsError> {
    sync_wire::BatchResult::decode(result_bytes)
        .map_err(debug_error)?
        .receipt_bytes
        .ok_or_else(|| JsError::new("committed batch receipt unavailable"))
}

/// IDs are returned as consecutive 16-byte values in signed manifest order.
#[wasm_bindgen]
pub fn manifest_object_ids(committed_control: &[u8]) -> Result<Vec<u8>, JsError> {
    Ok(ready_replay::manifest_objects(committed_control)
        .map_err(debug_error)?
        .into_iter()
        .flat_map(|object| object.id)
        .collect())
}

#[wasm_bindgen]
pub fn verified_manifest_object(
    committed_control: &[u8],
    object_id: &[u8],
    response: &[u8],
) -> Result<Vec<u8>, JsError> {
    ready_replay::verified_object_from_response(
        committed_control,
        fixed(object_id, "object ID")?,
        response,
    )
    .map_err(debug_error)
}
