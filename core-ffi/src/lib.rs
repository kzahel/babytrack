//! Native Swift/Kotlin binding over the shared Rust replay path.

#![forbid(unsafe_code)]

#[cfg(feature = "fixture-api")]
use std::sync::{Arc, Mutex};

#[cfg(feature = "fixture-api")]
use babytrack_core::{
    batch, crypto,
    projection::{Outcome, Projection},
};

uniffi::setup_scaffolding!();

#[cfg(feature = "fixture-api")]
#[derive(Debug, uniffi::Error)]
pub enum BindingError {
    InvalidBytes,
    Rejected(String),
    LockPoisoned,
}

#[cfg(feature = "fixture-api")]
impl std::fmt::Display for BindingError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{self:?}")
    }
}

#[cfg(feature = "fixture-api")]
impl std::error::Error for BindingError {}

#[cfg(feature = "fixture-api")]
#[derive(uniffi::Object)]
pub struct NativeFamily {
    projection: Mutex<Projection>,
}

#[cfg(feature = "fixture-api")]
#[uniffi::export]
impl NativeFamily {
    #[uniffi::constructor]
    pub fn new(family_id: Vec<u8>) -> Result<Arc<Self>, BindingError> {
        Ok(Arc::new(Self {
            projection: Mutex::new(Projection::new(fixed(&family_id)?)),
        }))
    }

    /// Fixture-only primitive replay. Production sharing will accept raw
    /// committed entries through a core-owned authorization session.
    pub fn apply_envelope(
        &self,
        envelope: Vec<u8>,
        relay_id: Vec<u8>,
        epoch_key: Vec<u8>,
        signer_public_key: Vec<u8>,
        cursor: u64,
    ) -> Result<bool, BindingError> {
        let mut projection = self
            .projection
            .lock()
            .map_err(|_| BindingError::LockPoisoned)?;
        let authenticated = batch::open_authenticated(
            &envelope,
            &projection.family_id(),
            &fixed(&relay_id)?,
            &fixed(&epoch_key)?,
            &fixed(&signer_public_key)?,
        )
        .map_err(rejected)?;
        match projection
            .apply_authenticated(&authenticated, cursor)
            .map_err(rejected)?
        {
            Outcome::Applied => Ok(true),
            Outcome::Inert(_) => Ok(false),
        }
    }

    pub fn advance_control(&self, cursor: u64) -> Result<(), BindingError> {
        self.projection
            .lock()
            .map_err(|_| BindingError::LockPoisoned)?
            .advance_control(cursor)
            .map_err(rejected)
    }

    pub fn last_cursor(&self) -> Result<u64, BindingError> {
        Ok(self
            .projection
            .lock()
            .map_err(|_| BindingError::LockPoisoned)?
            .last_cursor())
    }

    pub fn inert_count(&self) -> Result<u64, BindingError> {
        Ok(self
            .projection
            .lock()
            .map_err(|_| BindingError::LockPoisoned)?
            .inert_batches()
            .len() as u64)
    }

    /// Empty means the record or field is absent.
    pub fn field_cbor(&self, record_id: Vec<u8>, field_id: u64) -> Result<Vec<u8>, BindingError> {
        let record_id = fixed(&record_id)?;
        Ok(self
            .projection
            .lock()
            .map_err(|_| BindingError::LockPoisoned)?
            .record(&record_id)
            .and_then(|record| record.field(field_id))
            .map_or_else(Vec::new, |field| field.canonical_bytes.clone()))
    }
}

#[cfg(feature = "fixture-api")]
#[uniffi::export]
pub fn ed25519_public_key(signing_seed: Vec<u8>) -> Result<Vec<u8>, BindingError> {
    Ok(crypto::signing_public_key(&fixed(&signing_seed)?).to_vec())
}

/// Fixture-only fixed-header byte path; never use for production writes.
#[cfg(feature = "fixture-api")]
#[uniffi::export]
pub fn seal_one(
    header_cbor: Vec<u8>,
    operation_cbor: Vec<u8>,
    epoch_key: Vec<u8>,
    signing_seed: Vec<u8>,
) -> Result<Vec<u8>, BindingError> {
    let header = batch::Header::decode(&header_cbor).map_err(rejected)?;
    Ok(batch::seal(
        &header,
        &[operation_cbor],
        &fixed(&epoch_key)?,
        &fixed(&signing_seed)?,
    )
    .map_err(rejected)?
    .envelope_bytes)
}

#[cfg(feature = "fixture-api")]
fn fixed<const N: usize>(bytes: &[u8]) -> Result<[u8; N], BindingError> {
    bytes.try_into().map_err(|_| BindingError::InvalidBytes)
}

#[cfg(feature = "fixture-api")]
fn rejected(error: impl std::fmt::Debug) -> BindingError {
    BindingError::Rejected(format!("{error:?}"))
}
