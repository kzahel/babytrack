//! Browser binding over the shared Rust verification and projection path.

#![forbid(unsafe_code)]

use std::collections::BTreeMap;

use babytrack_core::projection::{Outcome, Projection, VerifiedEpochKey};
use babytrack_core::{
    batch,
    cbor::{self, Value},
    control_chain::ControlChain,
    crypto,
    operation::Operation,
    projection::LocalProjection,
    ready_replay,
    sync_wire::{self, LogPage},
};
use wasm_bindgen::prelude::*;

#[cfg(feature = "fixture-api")]
#[wasm_bindgen]
pub struct WasmFamily {
    projection: Projection,
}

#[wasm_bindgen]
pub struct WasmLocalFamily {
    projection: LocalProjection,
    family_id: [u8; 16],
    device_id: [u8; 16],
}

/// Public relay authority replay. Encrypted record projection and local
/// outbox handling are separate steps, but every committed cursor is checked
/// against the same Rust control chain used by native clients and the relay.
#[wasm_bindgen]
pub struct WasmPublicFamily {
    chain: ControlChain,
}

/// Epoch-one initial manager data view. The signed genesis, committed
/// manifest objects, key commitment, and every batch receipt must verify
/// before record fields become visible. Same-epoch authority changes can
/// advance this view; rotation requires a separately verified keyring.
#[wasm_bindgen]
pub struct WasmInitialFamily {
    genesis: Vec<u8>,
    relay_public_key: [u8; 32],
    epoch_key: [u8; 32],
    objects: BTreeMap<[u8; 16], Vec<u8>>,
    ready: Option<InitialReady>,
}

struct InitialReady {
    chain: ControlChain,
    projection: Projection,
    overlay: Option<Projection>,
    overlay_operations: Vec<Operation>,
    key: VerifiedEpochKey,
}

#[wasm_bindgen]
impl WasmInitialFamily {
    #[wasm_bindgen(constructor)]
    pub fn new(genesis: &[u8], relay_public_key: &[u8], epoch_key: &[u8]) -> Result<Self, JsError> {
        let relay_public_key = fixed(relay_public_key, "relay public key")?;
        let epoch_key = fixed(epoch_key, "epoch key")?;
        ControlChain::from_genesis(genesis, relay_public_key)
            .map_err(debug_error)?
            .verify_initial_epoch_key(&epoch_key)
            .map_err(debug_error)?;
        Ok(Self {
            genesis: genesis.to_vec(),
            relay_public_key,
            epoch_key,
            objects: BTreeMap::new(),
            ready: None,
        })
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
        self.ready = Some(InitialReady {
            chain,
            projection,
            overlay: None,
            overlay_operations: Vec::new(),
            key,
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
        let outcome = projection
            .apply_authorized_signed(&signed, &ready.key, chain.last_global_cursor())
            .map_err(debug_error)?;
        ready.chain = chain;
        ready.projection = projection;
        ready.overlay = None;
        ready.overlay_operations.clear();
        Ok(matches!(outcome, Outcome::Applied))
    }

    /// Replay a signed same-epoch authority change in the global cursor
    /// stream. A rotation cannot use the initial manager key-only view.
    pub fn apply_control(&mut self, committed_bytes: &[u8]) -> Result<(), JsError> {
        let ready = self
            .ready
            .as_mut()
            .ok_or_else(|| JsError::new("manifest objects not yet verified"))?;
        let mut chain = ready.chain.clone();
        chain.apply_control(committed_bytes).map_err(debug_error)?;
        chain
            .verify_initial_epoch_key(&self.epoch_key)
            .map_err(debug_error)?;
        let mut projection = ready.projection.clone();
        projection
            .advance_control(chain.last_global_cursor())
            .map_err(debug_error)?;
        ready.chain = chain;
        ready.projection = projection;
        ready.overlay = None;
        ready.overlay_operations.clear();
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
        if ready.chain.epoch().map_err(debug_error)? != 1
            || ready
                .chain
                .active_signing_public(device_id)
                .map_err(debug_error)?
                != crypto::signing_public_key(&signing_seed)
        {
            return Err(JsError::new(
                "initial device authority differs from credential",
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
            epoch: 1,
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
        Ok(batch::seal(
            &header,
            &[operation_bytes.to_vec()],
            &self.epoch_key,
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
}

/// Bounded core-decoded relay log page. JavaScript handles transport and
/// persistence but never parses or interprets protocol CBOR.
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
        let chain =
            ControlChain::from_genesis(genesis_bytes, fixed(relay_public_key, "relay public key")?)
                .map_err(debug_error)?;
        Ok(Self { chain })
    }

    pub fn apply_control(&mut self, committed_bytes: &[u8]) -> Result<(), JsError> {
        self.chain
            .apply_control(committed_bytes)
            .map_err(debug_error)
    }

    pub fn apply_batch(
        &mut self,
        envelope_bytes: &[u8],
        receipt_bytes: &[u8],
    ) -> Result<(), JsError> {
        self.chain
            .apply_public_batch(envelope_bytes, receipt_bytes)
            .map(|_| ())
            .map_err(debug_error)
    }

    pub fn family_id(&self) -> Vec<u8> {
        self.chain.family_id().to_vec()
    }

    pub fn relay_id(&self) -> Vec<u8> {
        self.chain.relay_id().to_vec()
    }

    pub fn last_cursor(&self) -> u64 {
        self.chain.last_global_cursor()
    }

    pub fn head_hash(&self) -> Vec<u8> {
        self.chain.head_hash().to_vec()
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

#[wasm_bindgen]
impl WasmLocalFamily {
    #[wasm_bindgen(constructor)]
    pub fn new(family_id: &[u8], device_id: &[u8]) -> Result<Self, JsError> {
        let family_id = fixed(family_id, "Family ID")?;
        Ok(Self {
            projection: LocalProjection::new(family_id),
            family_id,
            device_id: fixed(device_id, "device ID")?,
        })
    }

    pub fn append_operation(
        &mut self,
        operation_bytes: &[u8],
        index: u64,
    ) -> Result<Vec<u8>, JsError> {
        let operation = Operation::decode_bound(operation_bytes, &self.family_id, &self.device_id)
            .map_err(debug_error)?;
        self.projection
            .append(&operation, index)
            .map_err(debug_error)?;
        Ok(operation.operation_id.to_vec())
    }

    pub fn last_append_index(&self) -> u64 {
        self.projection.last_append_index()
    }

    pub fn field_cbor(&self, record_id: &[u8], field_id: u64) -> Result<Vec<u8>, JsError> {
        let record_id = fixed(record_id, "record ID")?;
        Ok(self
            .projection
            .record(&record_id)
            .and_then(|record| record.field(field_id))
            .map_or_else(Vec::new, |field| field.canonical_bytes.clone()))
    }
}

#[cfg(feature = "fixture-api")]
#[wasm_bindgen]
impl WasmFamily {
    #[wasm_bindgen(constructor)]
    pub fn new(family_id: &[u8]) -> Result<WasmFamily, JsError> {
        Ok(Self {
            projection: Projection::new(fixed(family_id, "Family ID")?),
        })
    }

    /// Fixture-only primitive replay. Production sharing will accept raw
    /// committed entries through a core-owned authorization session.
    pub fn apply_envelope(
        &mut self,
        envelope: &[u8],
        relay_id: &[u8],
        epoch_key: &[u8],
        signer_public_key: &[u8],
        cursor: u64,
    ) -> Result<bool, JsError> {
        let family_id = self.projection.family_id();
        let authenticated = batch::open_authenticated(
            envelope,
            &family_id,
            &fixed(relay_id, "relay ID")?,
            &fixed(epoch_key, "epoch key")?,
            &fixed(signer_public_key, "signer key")?,
        )
        .map_err(debug_error)?;
        match self
            .projection
            .apply_authenticated(&authenticated, cursor)
            .map_err(debug_error)?
        {
            Outcome::Applied => Ok(true),
            Outcome::Inert(_) => Ok(false),
        }
    }

    pub fn advance_control(&mut self, cursor: u64) -> Result<(), JsError> {
        self.projection.advance_control(cursor).map_err(debug_error)
    }

    pub fn last_cursor(&self) -> u64 {
        self.projection.last_cursor()
    }

    pub fn inert_count(&self) -> usize {
        self.projection.inert_batches().len()
    }

    /// Returns canonical CBOR bytes. Empty means the record or field is absent.
    pub fn field_cbor(&self, record_id: &[u8], field_id: u64) -> Result<Vec<u8>, JsError> {
        let record_id = fixed(record_id, "record ID")?;
        Ok(self
            .projection
            .record(&record_id)
            .and_then(|record| record.field(field_id))
            .map_or_else(Vec::new, |field| field.canonical_bytes.clone()))
    }
}

fn fixed<const N: usize>(bytes: &[u8], label: &str) -> Result<[u8; N], JsError> {
    bytes
        .try_into()
        .map_err(|_| JsError::new(&format!("{label} must be {N} bytes")))
}

fn debug_error(error: impl std::fmt::Debug) -> JsError {
    JsError::new(&format!("{error:?}"))
}

#[cfg(feature = "fixture-api")]
#[wasm_bindgen]
pub fn ed25519_public_key(signing_seed: &[u8]) -> Result<Vec<u8>, JsError> {
    Ok(crypto::signing_public_key(&fixed(signing_seed, "signing seed")?).to_vec())
}

/// Fixture-only fixed-header byte path; never use for production writes.
#[cfg(feature = "fixture-api")]
#[wasm_bindgen]
pub fn seal_one(
    header_cbor: &[u8],
    operation_cbor: &[u8],
    epoch_key: &[u8],
    signing_seed: &[u8],
) -> Result<Vec<u8>, JsError> {
    let header = batch::Header::decode(header_cbor).map_err(debug_error)?;
    Ok(batch::seal(
        &header,
        &[operation_cbor.to_vec()],
        &fixed(epoch_key, "epoch key")?,
        &fixed(signing_seed, "signing seed")?,
    )
    .map_err(debug_error)?
    .envelope_bytes)
}
