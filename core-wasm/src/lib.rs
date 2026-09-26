//! Browser binding over the shared Rust verification and projection path.

#![forbid(unsafe_code)]

use babytrack_core::{
    batch, crypto,
    operation::Operation,
    projection::{LocalProjection, Outcome, Projection},
};
use wasm_bindgen::prelude::*;

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

#[wasm_bindgen]
impl WasmFamily {
    #[wasm_bindgen(constructor)]
    pub fn new(family_id: &[u8]) -> Result<WasmFamily, JsError> {
        Ok(Self {
            projection: Projection::new(fixed(family_id, "Family ID")?),
        })
    }

    /// The host verifies the relay receipt, authorization, and epoch at this
    /// cursor before passing its active signer and key to this method.
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

#[wasm_bindgen]
pub fn ed25519_public_key(signing_seed: &[u8]) -> Result<Vec<u8>, JsError> {
    Ok(crypto::signing_public_key(&fixed(signing_seed, "signing seed")?).to_vec())
}

/// Fixed-header byte path. The host must durably allocate nonce, batch ID,
/// and device sequence before real writes and persist the returned envelope.
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
