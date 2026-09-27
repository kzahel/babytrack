//! Native Swift/Kotlin binding over the shared Rust replay path.

#![forbid(unsafe_code)]

use std::sync::{Arc, Mutex};

use babytrack_core::{
    local_api::{ActivityTime, LocalRepository},
    sqlite_store::FamilyHandle,
};

#[cfg(feature = "fixture-api")]
use babytrack_core::{
    batch, crypto,
    projection::{Outcome, Projection},
};

uniffi::setup_scaffolding!();

#[derive(Debug, uniffi::Error)]
pub enum BindingError {
    InvalidBytes,
    Rejected(String),
    LockPoisoned,
}

impl std::fmt::Display for BindingError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for BindingError {}

#[derive(Debug, Clone, uniffi::Record)]
pub struct FamilyRef {
    pub family_id: Vec<u8>,
    pub device_id: Vec<u8>,
}

impl FamilyRef {
    fn handle(&self) -> Result<FamilyHandle, BindingError> {
        Ok(FamilyHandle {
            family_id: fixed(&self.family_id)?,
            device_id: fixed(&self.device_id)?,
        })
    }
}

impl From<FamilyHandle> for FamilyRef {
    fn from(value: FamilyHandle) -> Self {
        Self {
            family_id: value.family_id.to_vec(),
            device_id: value.device_id.to_vec(),
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct ChildRow {
    pub id: Vec<u8>,
    pub name: String,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct ActivityWhen {
    pub start_utc_ms: i64,
    pub offset_minutes: i16,
    pub saved_at_ms: i64,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct ActivityRow {
    pub id: Vec<u8>,
    pub child_id: Vec<u8>,
    pub kind: String,
    pub start_utc_ms: i64,
    pub offset_minutes: i16,
    pub diaper_kind: Option<u8>,
    pub bottle_ml: Option<i64>,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct BackupInfoRow {
    pub source_family_id: Vec<u8>,
    pub snapshot_utc_ms: i64,
    pub known_gap: bool,
    pub record_count: u64,
}

impl From<babytrack_core::local_api::BackupInfo> for BackupInfoRow {
    fn from(value: babytrack_core::local_api::BackupInfo) -> Self {
        Self {
            source_family_id: value.source_family_id.to_vec(),
            snapshot_utc_ms: value.snapshot_utc_ms,
            known_gap: value.known_gap,
            record_count: value.record_count,
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct BackupFileRow {
    pub bytes: Vec<u8>,
    pub revision: u64,
    pub info: BackupInfoRow,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct RestoredOriginRow {
    pub source_family_id: Vec<u8>,
    pub snapshot_utc_ms: i64,
    pub known_gap: bool,
}

#[derive(uniffi::Object)]
pub struct NativeLocalStore {
    repo: Mutex<LocalRepository>,
}

#[uniffi::export]
impl NativeLocalStore {
    #[uniffi::constructor]
    pub fn open(path: String) -> Result<Arc<Self>, BindingError> {
        Ok(Arc::new(Self {
            repo: Mutex::new(LocalRepository::open(path).map_err(rejected)?),
        }))
    }

    pub fn families(&self) -> Result<Vec<FamilyRef>, BindingError> {
        Ok(self
            .repo
            .lock()
            .map_err(|_| BindingError::LockPoisoned)?
            .families()
            .map_err(rejected)?
            .into_iter()
            .map(Into::into)
            .collect())
    }

    pub fn revision(&self, family: FamilyRef) -> Result<u64, BindingError> {
        self.repo
            .lock()
            .map_err(|_| BindingError::LockPoisoned)?
            .revision(family.handle()?)
            .map_err(rejected)
    }

    pub fn restored_origin(
        &self,
        family: FamilyRef,
    ) -> Result<Option<RestoredOriginRow>, BindingError> {
        Ok(self
            .repo
            .lock()
            .map_err(|_| BindingError::LockPoisoned)?
            .restored_origin(family.handle()?)
            .map_err(rejected)?
            .map(|value| RestoredOriginRow {
                source_family_id: value.source_family_id.to_vec(),
                snapshot_utc_ms: value.snapshot_utc_ms,
                known_gap: value.known_gap,
            }))
    }

    pub fn create_family(&self, now_ms: i64) -> Result<FamilyRef, BindingError> {
        Ok(self
            .repo
            .lock()
            .map_err(|_| BindingError::LockPoisoned)?
            .create_family(now_ms)
            .map_err(rejected)?
            .into())
    }

    pub fn children(&self, family: FamilyRef) -> Result<Vec<ChildRow>, BindingError> {
        Ok(self
            .repo
            .lock()
            .map_err(|_| BindingError::LockPoisoned)?
            .children(family.handle()?)
            .map_err(rejected)?
            .into_iter()
            .map(|child| ChildRow {
                id: child.id.to_vec(),
                name: child.name,
            })
            .collect())
    }

    pub fn add_child(
        &self,
        family: FamilyRef,
        name: String,
        now_ms: i64,
    ) -> Result<Vec<u8>, BindingError> {
        Ok(self
            .repo
            .lock()
            .map_err(|_| BindingError::LockPoisoned)?
            .add_child(family.handle()?, &name, now_ms)
            .map_err(rejected)?
            .to_vec())
    }

    pub fn log_diaper(
        &self,
        family: FamilyRef,
        child_id: Vec<u8>,
        kind: u8,
        time: ActivityWhen,
    ) -> Result<Vec<u8>, BindingError> {
        Ok(self
            .repo
            .lock()
            .map_err(|_| BindingError::LockPoisoned)?
            .log_diaper(
                family.handle()?,
                fixed(&child_id)?,
                kind,
                ActivityTime {
                    start_utc_ms: time.start_utc_ms,
                    offset_minutes: time.offset_minutes,
                    saved_at_ms: time.saved_at_ms,
                },
            )
            .map_err(rejected)?
            .to_vec())
    }

    pub fn log_bottle_ml(
        &self,
        family: FamilyRef,
        child_id: Vec<u8>,
        amount_ml: i64,
        content: u8,
        time: ActivityWhen,
    ) -> Result<Vec<u8>, BindingError> {
        Ok(self
            .repo
            .lock()
            .map_err(|_| BindingError::LockPoisoned)?
            .log_bottle_ml(
                family.handle()?,
                fixed(&child_id)?,
                amount_ml,
                content,
                ActivityTime {
                    start_utc_ms: time.start_utc_ms,
                    offset_minutes: time.offset_minutes,
                    saved_at_ms: time.saved_at_ms,
                },
            )
            .map_err(rejected)?
            .to_vec())
    }

    pub fn timeline(
        &self,
        family: FamilyRef,
        child_id: Vec<u8>,
    ) -> Result<Vec<ActivityRow>, BindingError> {
        Ok(self
            .repo
            .lock()
            .map_err(|_| BindingError::LockPoisoned)?
            .timeline(family.handle()?, fixed(&child_id)?)
            .map_err(rejected)?
            .into_iter()
            .map(|row| ActivityRow {
                id: row.id.to_vec(),
                child_id: row.child_id.to_vec(),
                kind: row.kind,
                start_utc_ms: row.start_utc_ms,
                offset_minutes: row.offset_minutes,
                diaper_kind: row.diaper_kind,
                bottle_ml: row.bottle_ml,
            })
            .collect())
    }

    pub fn backup(&self, family: FamilyRef, now_ms: i64) -> Result<Vec<u8>, BindingError> {
        self.repo
            .lock()
            .map_err(|_| BindingError::LockPoisoned)?
            .backup(family.handle()?, now_ms)
            .map_err(rejected)
    }

    pub fn backup_file(
        &self,
        family: FamilyRef,
        now_ms: i64,
        password: Option<String>,
        available_memory_bytes: u64,
    ) -> Result<BackupFileRow, BindingError> {
        let file = self
            .repo
            .lock()
            .map_err(|_| BindingError::LockPoisoned)?
            .backup_file(
                family.handle()?,
                now_ms,
                password.as_deref(),
                available_memory_bytes,
            )
            .map_err(rejected)?;
        Ok(BackupFileRow {
            bytes: file.bytes,
            revision: file.revision,
            info: file.info.into(),
        })
    }

    pub fn inspect_readable(&self, bytes: Vec<u8>) -> Result<BackupInfoRow, BindingError> {
        Ok(LocalRepository::inspect_readable(&bytes)
            .map_err(rejected)?
            .into())
    }

    pub fn inspect_protected(
        &self,
        bytes: Vec<u8>,
        password: String,
        available_memory_bytes: u64,
    ) -> Result<BackupInfoRow, BindingError> {
        Ok(
            LocalRepository::inspect_protected(&bytes, &password, available_memory_bytes)
                .map_err(rejected)?
                .into(),
        )
    }

    pub fn protected_backup(
        &self,
        family: FamilyRef,
        now_ms: i64,
        password: String,
        available_memory_bytes: u64,
    ) -> Result<Vec<u8>, BindingError> {
        self.repo
            .lock()
            .map_err(|_| BindingError::LockPoisoned)?
            .protected_backup(family.handle()?, now_ms, &password, available_memory_bytes)
            .map_err(rejected)
    }

    pub fn restore(&self, bytes: Vec<u8>, now_ms: i64) -> Result<FamilyRef, BindingError> {
        Ok(self
            .repo
            .lock()
            .map_err(|_| BindingError::LockPoisoned)?
            .restore(&bytes, now_ms)
            .map_err(rejected)?
            .into())
    }

    pub fn restore_protected(
        &self,
        bytes: Vec<u8>,
        password: String,
        available_memory_bytes: u64,
        now_ms: i64,
    ) -> Result<FamilyRef, BindingError> {
        Ok(self
            .repo
            .lock()
            .map_err(|_| BindingError::LockPoisoned)?
            .restore_protected(&bytes, &password, available_memory_bytes, now_ms)
            .map_err(rejected)?
            .into())
    }
}

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

fn fixed<const N: usize>(bytes: &[u8]) -> Result<[u8; N], BindingError> {
    bytes.try_into().map_err(|_| BindingError::InvalidBytes)
}

fn rejected(error: impl std::fmt::Debug) -> BindingError {
    BindingError::Rejected(format!("{error:?}"))
}
