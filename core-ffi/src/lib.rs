//! Native Swift/Kotlin binding over the shared Rust replay path.

#![forbid(unsafe_code)]

use std::sync::{Arc, Mutex};

use babytrack_core::{
    active_pull,
    bootstrap::InvitationBootstrap,
    cbor::{self, Value},
    creation::ManagerCreation,
    enrollment::EnrollmentAttempt,
    first_admission::FirstAdmission,
    first_challenge::FirstChallenge,
    first_proof::FirstProof,
    first_removal::FirstRemoval,
    issue::FirstInviteIssue,
    local_api::{self, ActivityTime, LocalRepository},
    operation,
    shared_history::{self, PendingBatchResult, PublicHistorySession},
    shared_ready::ReadyFamilySession,
    sqlite_store::{FamilyHandle, SqliteStore},
    sync_wire::{ControlPage, OpaqueObject},
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
    pub birth_day: Option<i64>,
    pub sex: Option<u8>,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct ActivityWhen {
    pub start_utc_ms: i64,
    pub offset_minutes: i16,
    pub saved_at_ms: i64,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct MedicationInput {
    pub name: String,
    pub dose_amount: String,
    pub dose_unit: String,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct PumpInput {
    pub left_ml: Option<i64>,
    pub right_ml: Option<i64>,
    pub total_ml: Option<i64>,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct BreastSegmentRow {
    pub side: u8,
    pub start_utc_ms: i64,
    pub end_utc_ms: i64,
    pub start_offset_minutes: i16,
    pub end_offset_minutes: i16,
}

impl From<BreastSegmentRow> for local_api::BreastSegment {
    fn from(value: BreastSegmentRow) -> Self {
        Self {
            side: value.side,
            start_utc_ms: value.start_utc_ms,
            end_utc_ms: value.end_utc_ms,
            start_offset_minutes: value.start_offset_minutes,
            end_offset_minutes: value.end_offset_minutes,
        }
    }
}

fn breast_segment_row(value: local_api::BreastSegment) -> BreastSegmentRow {
    BreastSegmentRow {
        side: value.side,
        start_utc_ms: value.start_utc_ms,
        end_utc_ms: value.end_utc_ms,
        start_offset_minutes: value.start_offset_minutes,
        end_offset_minutes: value.end_offset_minutes,
    }
}

impl From<PumpInput> for local_api::PumpAmounts {
    fn from(value: PumpInput) -> Self {
        Self {
            left_ml: value.left_ml,
            right_ml: value.right_ml,
            total_ml: value.total_ml,
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct ActivityRow {
    pub id: Vec<u8>,
    pub child_id: Vec<u8>,
    pub kind: String,
    pub start_utc_ms: i64,
    pub offset_minutes: i16,
    pub end_utc_ms: Option<i64>,
    pub note: Option<String>,
    pub diaper_kind: Option<u8>,
    pub bottle_ml: Option<i64>,
    pub breast_side: Option<u8>,
    pub breast_segments: Option<Vec<BreastSegmentRow>>,
    pub solids_foods: Option<Vec<String>>,
    pub solids_amount: Option<String>,
    pub pump_left_ml: Option<i64>,
    pub pump_right_ml: Option<i64>,
    pub pump_total_ml: Option<i64>,
    pub growth_weight_g: Option<i64>,
    pub growth_length_mm: Option<i64>,
    pub temperature_c: Option<String>,
    pub medication_name: Option<String>,
    pub medication_dose_amount: Option<String>,
    pub medication_dose_unit: Option<String>,
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

#[derive(Debug, Clone, uniffi::Record)]
pub struct StagedObjectRow {
    pub object_id: Vec<u8>,
    pub body: Vec<u8>,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct PreparedShareRow {
    pub promotion_id: Vec<u8>,
    pub candidate_bytes: Vec<u8>,
    pub objects: Vec<StagedObjectRow>,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct PreparedInviteRow {
    pub invitation_id: Vec<u8>,
    pub candidate_bytes: Vec<u8>,
    pub object: StagedObjectRow,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct InvitationPreviewRow {
    pub family_id: Vec<u8>,
    pub relay_origin: String,
    pub role: u8,
    pub control_path: String,
    pub read_auth: Vec<u8>,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct PreparedJoinRow {
    pub family: FamilyRef,
    pub candidate_bytes: Vec<u8>,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct SignedReadRow {
    pub path: String,
    pub auth: Vec<u8>,
    pub after: u64,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct PreparedChallengeRow {
    pub candidate_bytes: Vec<u8>,
    pub objects: Vec<StagedObjectRow>,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct ChallengeReadRow {
    pub path: String,
    pub auth: Vec<u8>,
    pub object_id: Vec<u8>,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct PreparedAdmissionRow {
    pub candidate_bytes: Vec<u8>,
    pub objects: Vec<StagedObjectRow>,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct PreparedRemovalRow {
    pub candidate_bytes: Vec<u8>,
    pub objects: Vec<StagedObjectRow>,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct RecipientSyncRow {
    pub verified_cursor: u64,
    pub pending_control_cursor: u64,
    pub awaiting_grant: bool,
    pub no_more_visible: bool,
    pub remaining_objects: bool,
    pub ready: bool,
    pub child_count: u64,
    pub removed: bool,
    pub private_copy: Option<FamilyRef>,
    pub pending_result: u8,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct RemovedDeviceRow {
    pub verified_cursor: u64,
    pub known_gap: bool,
    pub private_copy: Option<FamilyRef>,
    /// 0 no staged batch, 1 unknown, 2 accepted, 3 rejected.
    pub pending_result: u8,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct SharedSnapshotRow {
    pub family: FamilyRef,
    pub verified_cursor: u64,
    pub children: Vec<ChildRow>,
    pub activities: Vec<ActivityRow>,
    pub unsent_count: u64,
    pub inert_count: u64,
    pub recent_inert: Vec<InertBatchRow>,
    pub devices: Vec<SharedDeviceRow>,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct SharedDeviceRow {
    pub device_id: Vec<u8>,
    pub role: u8,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct InertBatchRow {
    pub cursor: u64,
    pub object_hash: Vec<u8>,
    pub reason: String,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct SharedSyncRow {
    pub verified_cursor: u64,
    pub no_more_visible: bool,
    pub remaining_objects: bool,
    pub ready: bool,
    /// 0 drained, 1 saved locally, 2 exact batch outcome uncertain.
    pub outbox_state: u8,
    pub inert_count: u64,
}

#[uniffi::export(callback_interface)]
pub trait RelayReadTransport: Send + Sync {
    fn get(&self, path: String, auth: Vec<u8>) -> Result<Vec<u8>, BindingError>;
}

#[derive(uniffi::Object)]
pub struct NativeSharedStore {
    store: Mutex<SqliteStore>,
}

#[uniffi::export]
impl NativeSharedStore {
    #[uniffi::constructor]
    pub fn open(path: String) -> Result<Arc<Self>, BindingError> {
        Ok(Arc::new(Self {
            store: Mutex::new(SqliteStore::open(path).map_err(rejected)?),
        }))
    }

    /// Persists the exact candidate, encrypted keys, and promotion chunks
    /// before the platform makes its first network request.
    pub fn prepare_share(
        &self,
        family: FamilyRef,
        relay_public_key: Vec<u8>,
        wrapping_key: Vec<u8>,
    ) -> Result<PreparedShareRow, BindingError> {
        let mut store = self.store.lock().map_err(|_| BindingError::LockPoisoned)?;
        let prepared = ManagerCreation::prepare(
            &mut store,
            family.handle()?,
            fixed(&relay_public_key)?,
            &fixed(&wrapping_key)?,
        )
        .map_err(rejected)?;
        Ok(PreparedShareRow {
            promotion_id: prepared.promotion_id().to_vec(),
            candidate_bytes: prepared.candidate_bytes().to_vec(),
            objects: prepared
                .stage_bodies()
                .map_err(rejected)?
                .into_iter()
                .map(|(object_id, body)| StagedObjectRow {
                    object_id: object_id.to_vec(),
                    body,
                })
                .collect(),
        })
    }

    /// A POST response alone cannot mark sharing complete: the core checks
    /// the committed relay signature and exact prepared candidate first.
    pub fn confirm_share(
        &self,
        family: FamilyRef,
        wrapping_key: Vec<u8>,
        commit_response: Vec<u8>,
    ) -> Result<u64, BindingError> {
        let mut store = self.store.lock().map_err(|_| BindingError::LockPoisoned)?;
        let prepared = ManagerCreation::resume(&store, family.handle()?, &fixed(&wrapping_key)?)
            .map_err(rejected)?;
        Ok(prepared
            .confirm(&mut store, &committed_control(&commit_response)?)
            .map_err(rejected)?
            .observed_cursor())
    }

    pub fn prepare_invite(
        &self,
        family: FamilyRef,
        wrapping_key: Vec<u8>,
        role: u8,
    ) -> Result<PreparedInviteRow, BindingError> {
        let mut store = self.store.lock().map_err(|_| BindingError::LockPoisoned)?;
        let creation = ManagerCreation::resume(&store, family.handle()?, &fixed(&wrapping_key)?)
            .map_err(rejected)?;
        let issue = FirstInviteIssue::prepare(&mut store, &creation, &fixed(&wrapping_key)?, role)
            .map_err(rejected)?;
        Ok(PreparedInviteRow {
            invitation_id: issue.invitation_id().to_vec(),
            candidate_bytes: issue.candidate_bytes().to_vec(),
            object: StagedObjectRow {
                object_id: issue.object_id().to_vec(),
                body: issue.stage_body().map_err(rejected)?,
            },
        })
    }

    /// The one-use link is available only after the signed issue commits.
    pub fn confirm_invite(
        &self,
        family: FamilyRef,
        wrapping_key: Vec<u8>,
        commit_response: Vec<u8>,
        relay_origin: String,
    ) -> Result<String, BindingError> {
        let mut store = self.store.lock().map_err(|_| BindingError::LockPoisoned)?;
        let creation = ManagerCreation::resume(&store, family.handle()?, &fixed(&wrapping_key)?)
            .map_err(rejected)?;
        let issue = FirstInviteIssue::resume(&store, &creation, &fixed(&wrapping_key)?)
            .map_err(rejected)?;
        issue
            .confirm(
                &mut store,
                &creation,
                &committed_control(&commit_response)?,
                &relay_origin,
            )
            .map_err(rejected)?
            .to_fragment()
            .map_err(rejected)
    }

    /// Retry the exact saved claim after invitation read access closes.
    pub fn resume_join(
        &self,
        fragment: String,
        wrapping_key: Vec<u8>,
    ) -> Result<Option<PreparedJoinRow>, BindingError> {
        let bootstrap = InvitationBootstrap::from_fragment(&fragment).map_err(rejected)?;
        let mut store = self.store.lock().map_err(|_| BindingError::LockPoisoned)?;
        Ok(
            EnrollmentAttempt::resume_for_invitation(
                &mut store,
                &bootstrap,
                &fixed(&wrapping_key)?,
            )
            .map_err(rejected)?
            .map(|attempt| PreparedJoinRow {
                family: attempt.family().into(),
                candidate_bytes: attempt.claim_candidate().to_vec(),
            }),
        )
    }

    /// Recover the exact encrypted-in-store claim after process restart,
    /// without requiring the bearer invitation fragment in UI memory.
    pub fn resume_join_family(
        &self,
        family: FamilyRef,
        wrapping_key: Vec<u8>,
    ) -> Result<PreparedJoinRow, BindingError> {
        let mut store = self.store.lock().map_err(|_| BindingError::LockPoisoned)?;
        let attempt = EnrollmentAttempt::resume(
            &mut store,
            family.handle()?.family_id,
            &fixed(&wrapping_key)?,
        )
        .map_err(rejected)?;
        if attempt.family() != family.handle()? {
            return Err(BindingError::InvalidBytes);
        }
        Ok(PreparedJoinRow {
            family,
            candidate_bytes: attempt.claim_candidate().to_vec(),
        })
    }

    /// Verify the invitation-linked public controls before generating or
    /// storing this installation's recipient credentials.
    pub fn prepare_join(
        &self,
        fragment: String,
        control_page: Vec<u8>,
        wrapping_key: Vec<u8>,
    ) -> Result<PreparedJoinRow, BindingError> {
        let bootstrap = InvitationBootstrap::from_fragment(&fragment).map_err(rejected)?;
        let page =
            ControlPage::decode(&control_page, bootstrap.family_id(), 0).map_err(rejected)?;
        if page.entries.len() < 2 || page.entries[0].cursor != 1 {
            return Err(BindingError::InvalidBytes);
        }
        let mut store = self.store.lock().map_err(|_| BindingError::LockPoisoned)?;
        let attempt = EnrollmentAttempt::prepare_sparse(
            &mut store,
            &bootstrap,
            &page.entries[0].committed_bytes,
            &page.entries[1].committed_bytes,
            &fixed(&wrapping_key)?,
        )
        .map_err(rejected)?;
        Ok(PreparedJoinRow {
            family: attempt.family().into(),
            candidate_bytes: attempt.claim_candidate().to_vec(),
        })
    }

    /// A successful HTTP status is insufficient: bind the signed relay
    /// commit to the exact durable claim before recording pending progress.
    pub fn confirm_join_claim(
        &self,
        family: FamilyRef,
        wrapping_key: Vec<u8>,
        commit_response: Vec<u8>,
    ) -> Result<(), BindingError> {
        let mut store = self.store.lock().map_err(|_| BindingError::LockPoisoned)?;
        let attempt = EnrollmentAttempt::resume(
            &mut store,
            family.handle()?.family_id,
            &fixed(&wrapping_key)?,
        )
        .map_err(rejected)?;
        if attempt.family() != family.handle()? {
            return Err(BindingError::InvalidBytes);
        }
        attempt
            .confirm_sparse_claim(&mut store, &committed_control(&commit_response)?)
            .map_err(rejected)
    }

    pub fn manager_control_read(
        &self,
        family: FamilyRef,
        wrapping_key: Vec<u8>,
    ) -> Result<SignedReadRow, BindingError> {
        let store = self.store.lock().map_err(|_| BindingError::LockPoisoned)?;
        let manager = ManagerCreation::resume(&store, family.handle()?, &fixed(&wrapping_key)?)
            .map_err(rejected)?;
        let after = PublicHistorySession::resume(&store, family.handle()?)
            .map_err(rejected)?
            .cursor();
        let path = format!(
            "/v1/families/{}/control?after={after}",
            lower_hex(&family.family_id)
        );
        let read = manager.sign_get(&path).map_err(rejected)?;
        Ok(SignedReadRow {
            path,
            auth: read.bytes,
            after,
        })
    }

    /// Accept verified controls before choosing the sole pending device
    /// from Rust authority state and preparing exact challenge bytes.
    pub fn prepare_first_challenge(
        &self,
        family: FamilyRef,
        wrapping_key: Vec<u8>,
        read: SignedReadRow,
        control_page: Vec<u8>,
    ) -> Result<PreparedChallengeRow, BindingError> {
        let mut store = self.store.lock().map_err(|_| BindingError::LockPoisoned)?;
        let manager = ManagerCreation::resume(&store, family.handle()?, &fixed(&wrapping_key)?)
            .map_err(rejected)?;
        let mut public =
            PublicHistorySession::resume(&store, family.handle()?).map_err(rejected)?;
        if read.after != public.cursor()
            || read.path
                != format!(
                    "/v1/families/{}/control?after={}",
                    lower_hex(&family.family_id),
                    read.after
                )
        {
            return Err(BindingError::InvalidBytes);
        }
        let page = ControlPage::decode(&control_page, family.handle()?.family_id, read.after)
            .map_err(rejected)?;
        for entry in page.entries {
            public
                .accept_control(&mut store, &entry.committed_bytes)
                .map_err(rejected)?;
        }
        let challenge =
            FirstChallenge::prepare_for_only_pending(&mut store, &manager, &fixed(&wrapping_key)?)
                .map_err(rejected)?;
        Ok(PreparedChallengeRow {
            candidate_bytes: challenge.candidate_bytes().to_vec(),
            objects: challenge
                .stage_bodies()
                .map_err(rejected)?
                .into_iter()
                .map(|(object_id, body)| StagedObjectRow {
                    object_id: object_id.to_vec(),
                    body,
                })
                .collect(),
        })
    }

    pub fn confirm_first_challenge(
        &self,
        family: FamilyRef,
        wrapping_key: Vec<u8>,
        commit_response: Vec<u8>,
    ) -> Result<(), BindingError> {
        let mut store = self.store.lock().map_err(|_| BindingError::LockPoisoned)?;
        let manager = ManagerCreation::resume(&store, family.handle()?, &fixed(&wrapping_key)?)
            .map_err(rejected)?;
        FirstChallenge::resume(&store, &manager, &fixed(&wrapping_key)?)
            .map_err(rejected)?
            .confirm(&mut store, &committed_control(&commit_response)?)
            .map_err(rejected)
    }

    pub fn recipient_control_read(
        &self,
        family: FamilyRef,
        wrapping_key: Vec<u8>,
    ) -> Result<SignedReadRow, BindingError> {
        let mut store = self.store.lock().map_err(|_| BindingError::LockPoisoned)?;
        let attempt = EnrollmentAttempt::resume(
            &mut store,
            family.handle()?.family_id,
            &fixed(&wrapping_key)?,
        )
        .map_err(rejected)?;
        if attempt.family() != family.handle()? {
            return Err(BindingError::InvalidBytes);
        }
        let after = attempt.pending_control_cursor(&store).map_err(rejected)?;
        let path = format!(
            "/v1/families/{}/control?after={after}",
            lower_hex(&family.family_id)
        );
        let auth = attempt.sign_get(&path).map_err(rejected)?.bytes;
        Ok(SignedReadRow { path, auth, after })
    }

    pub fn recipient_challenge_read(
        &self,
        family: FamilyRef,
        wrapping_key: Vec<u8>,
        read: SignedReadRow,
        control_page: Vec<u8>,
    ) -> Result<ChallengeReadRow, BindingError> {
        let mut store = self.store.lock().map_err(|_| BindingError::LockPoisoned)?;
        let attempt = EnrollmentAttempt::resume(
            &mut store,
            family.handle()?.family_id,
            &fixed(&wrapping_key)?,
        )
        .map_err(rejected)?;
        if attempt.family() != family.handle()?
            || read.after != attempt.pending_control_cursor(&store).map_err(rejected)?
            || read.path
                != format!(
                    "/v1/families/{}/control?after={}",
                    lower_hex(&family.family_id),
                    read.after
                )
        {
            return Err(BindingError::InvalidBytes);
        }
        let page = ControlPage::decode(&control_page, family.handle()?.family_id, read.after)
            .map_err(rejected)?;
        for entry in page.entries {
            attempt
                .accept_sparse_control(&mut store, &entry.committed_bytes)
                .map_err(rejected)?;
        }
        let chain = shared_history::first_join_chain(
            &store,
            family.handle()?,
            attempt.relay_public_key().map_err(rejected)?,
            4,
        )
        .map_err(rejected)?;
        let challenge = chain
            .latest_challenge(&attempt.invitation_id())
            .ok_or(BindingError::InvalidBytes)?;
        let object_id = challenge.hpke_object_id();
        let path = format!(
            "/v1/families/{}/objects/{}",
            lower_hex(&family.family_id),
            lower_hex(&object_id)
        );
        let auth = attempt.sign_get(&path).map_err(rejected)?.bytes;
        Ok(ChallengeReadRow {
            path,
            auth,
            object_id: object_id.to_vec(),
        })
    }

    pub fn prepare_first_proof(
        &self,
        family: FamilyRef,
        wrapping_key: Vec<u8>,
        challenge_read: ChallengeReadRow,
        object_response: Vec<u8>,
    ) -> Result<Vec<u8>, BindingError> {
        let mut store = self.store.lock().map_err(|_| BindingError::LockPoisoned)?;
        let attempt = EnrollmentAttempt::resume(
            &mut store,
            family.handle()?.family_id,
            &fixed(&wrapping_key)?,
        )
        .map_err(rejected)?;
        if attempt.family() != family.handle()? {
            return Err(BindingError::InvalidBytes);
        }
        let object_id = fixed::<16>(&challenge_read.object_id)?;
        let expected_path = format!(
            "/v1/families/{}/objects/{}",
            lower_hex(&family.family_id),
            lower_hex(&object_id)
        );
        if challenge_read.path != expected_path {
            return Err(BindingError::InvalidBytes);
        }
        let object = OpaqueObject::decode(&object_response, object_id).map_err(rejected)?;
        if object.kind != 2 {
            return Err(BindingError::InvalidBytes);
        }
        attempt
            .accept_sparse_object(&mut store, object_id, &object.object_bytes)
            .map_err(rejected)?;
        Ok(
            FirstProof::prepare(&mut store, &attempt, &fixed(&wrapping_key)?)
                .map_err(rejected)?
                .candidate_bytes()
                .to_vec(),
        )
    }

    pub fn saved_first_proof(
        &self,
        family: FamilyRef,
        wrapping_key: Vec<u8>,
    ) -> Result<Option<Vec<u8>>, BindingError> {
        let mut store = self.store.lock().map_err(|_| BindingError::LockPoisoned)?;
        let attempt = EnrollmentAttempt::resume(
            &mut store,
            family.handle()?.family_id,
            &fixed(&wrapping_key)?,
        )
        .map_err(rejected)?;
        if attempt.family() != family.handle()? {
            return Err(BindingError::InvalidBytes);
        }
        Ok(
            FirstProof::resume_optional(&store, &attempt, &fixed(&wrapping_key)?)
                .map_err(rejected)?
                .map(|proof| proof.candidate_bytes().to_vec()),
        )
    }

    pub fn confirm_first_proof(
        &self,
        family: FamilyRef,
        wrapping_key: Vec<u8>,
        commit_response: Vec<u8>,
    ) -> Result<(), BindingError> {
        let mut store = self.store.lock().map_err(|_| BindingError::LockPoisoned)?;
        let attempt = EnrollmentAttempt::resume(
            &mut store,
            family.handle()?.family_id,
            &fixed(&wrapping_key)?,
        )
        .map_err(rejected)?;
        if attempt.family() != family.handle()? {
            return Err(BindingError::InvalidBytes);
        }
        FirstProof::resume(&store, &attempt, &fixed(&wrapping_key)?)
            .map_err(rejected)?
            .confirm(&mut store, &committed_control(&commit_response)?)
            .map_err(rejected)
    }

    pub fn prepare_first_admission(
        &self,
        family: FamilyRef,
        wrapping_key: Vec<u8>,
        read: SignedReadRow,
        control_page: Vec<u8>,
    ) -> Result<PreparedAdmissionRow, BindingError> {
        let mut store = self.store.lock().map_err(|_| BindingError::LockPoisoned)?;
        let manager = ManagerCreation::resume(&store, family.handle()?, &fixed(&wrapping_key)?)
            .map_err(rejected)?;
        let mut public =
            PublicHistorySession::resume(&store, family.handle()?).map_err(rejected)?;
        if read.after != public.cursor()
            || read.path
                != format!(
                    "/v1/families/{}/control?after={}",
                    lower_hex(&family.family_id),
                    read.after
                )
        {
            return Err(BindingError::InvalidBytes);
        }
        let page = ControlPage::decode(&control_page, family.handle()?.family_id, read.after)
            .map_err(rejected)?;
        for entry in page.entries {
            public
                .accept_control(&mut store, &entry.committed_bytes)
                .map_err(rejected)?;
        }
        let admission =
            FirstAdmission::prepare_for_only_proved(&mut store, &manager, &fixed(&wrapping_key)?)
                .map_err(rejected)?;
        Ok(PreparedAdmissionRow {
            candidate_bytes: admission.candidate_bytes().to_vec(),
            objects: admission
                .stage_bodies()
                .map_err(rejected)?
                .into_iter()
                .map(|(object_id, body)| StagedObjectRow {
                    object_id: object_id.to_vec(),
                    body,
                })
                .collect(),
        })
    }

    pub fn confirm_first_admission(
        &self,
        family: FamilyRef,
        wrapping_key: Vec<u8>,
        commit_response: Vec<u8>,
    ) -> Result<(), BindingError> {
        let mut store = self.store.lock().map_err(|_| BindingError::LockPoisoned)?;
        let manager = ManagerCreation::resume(&store, family.handle()?, &fixed(&wrapping_key)?)
            .map_err(rejected)?;
        FirstAdmission::resume(&store, &manager, &fixed(&wrapping_key)?)
            .map_err(rejected)?
            .confirm(&mut store, &manager, &committed_control(&commit_response)?)
            .map_err(rejected)
    }

    /// Prepare exact durable removal bytes after a verified current sync.
    /// The target is one device credential, never a person-wide account.
    pub fn prepare_first_removal(
        &self,
        family: FamilyRef,
        wrapping_key: Vec<u8>,
        target_device_id: Vec<u8>,
    ) -> Result<PreparedRemovalRow, BindingError> {
        let mut store = self.store.lock().map_err(|_| BindingError::LockPoisoned)?;
        let manager = ManagerCreation::resume(&store, family.handle()?, &fixed(&wrapping_key)?)
            .map_err(rejected)?;
        let removal = FirstRemoval::prepare(
            &mut store,
            &manager,
            &fixed(&wrapping_key)?,
            fixed(&target_device_id)?,
        )
        .map_err(rejected)?;
        Ok(PreparedRemovalRow {
            candidate_bytes: removal.candidate_bytes().to_vec(),
            objects: removal
                .stage_bodies()
                .map_err(rejected)?
                .into_iter()
                .map(|(object_id, body)| StagedObjectRow {
                    object_id: object_id.to_vec(),
                    body,
                })
                .collect(),
        })
    }

    pub fn confirm_first_removal(
        &self,
        family: FamilyRef,
        wrapping_key: Vec<u8>,
        commit_response: Vec<u8>,
    ) -> Result<(), BindingError> {
        let mut store = self.store.lock().map_err(|_| BindingError::LockPoisoned)?;
        let manager = ManagerCreation::resume(&store, family.handle()?, &fixed(&wrapping_key)?)
            .map_err(rejected)?;
        FirstRemoval::resume(&store, &manager, &fixed(&wrapping_key)?)
            .map_err(rejected)?
            .confirm(&mut store, &manager, &committed_control(&commit_response)?)
            .map_err(rejected)
    }

    /// Ask for public controls before data reads, so a revoked credential
    /// can verify its removal even when the relay denies new ciphertext.
    pub fn check_recipient_removal(
        &self,
        family: FamilyRef,
        wrapping_key: Vec<u8>,
        now_ms: i64,
        transport: Box<dyn RelayReadTransport>,
    ) -> Result<Option<RemovedDeviceRow>, BindingError> {
        let mut store = self.store.lock().map_err(|_| BindingError::LockPoisoned)?;
        let handle = family.handle()?;
        let wrapping = fixed(&wrapping_key)?;
        let saved = if let Some(saved) = store.saved_removal(handle).map_err(rejected)? {
            Some(saved)
        } else {
            let attempt = EnrollmentAttempt::resume(&mut store, handle.family_id, &wrapping)
                .map_err(rejected)?;
            if attempt.family() != handle
                || !attempt.has_committed_admission(&store).map_err(rejected)?
            {
                return Ok(None);
            }
            let public = PublicHistorySession::resume(&store, handle).map_err(rejected)?;
            let path = format!(
                "/v1/families/{}/control?after={}",
                lower_hex(&handle.family_id),
                public.cursor()
            );
            let auth = attempt.sign_get(&path).map_err(rejected)?.bytes;
            let page = transport.get(path, auth)?;
            public
                .save_removed_control_page(&mut store, &page)
                .map_err(rejected)?
        };
        let Some(saved) = saved else { return Ok(None) };
        let public = PublicHistorySession::resume(&store, handle).map_err(rejected)?;
        let pending_result =
            if let Some(batch_id) = public.pending_batch_id(&store).map_err(rejected)? {
                let attempt = EnrollmentAttempt::resume(&mut store, handle.family_id, &wrapping)
                    .map_err(rejected)?;
                let path = format!(
                    "/v1/families/{}/batch-results/{}",
                    lower_hex(&handle.family_id),
                    lower_hex(&batch_id),
                );
                let auth = attempt.sign_get(&path).map_err(rejected)?.bytes;
                match transport.get(path, auth) {
                    Ok(bytes) => public
                        .inspect_removed_pending_result(&store, &saved, &bytes)
                        .unwrap_or(1),
                    Err(_) => 1,
                }
            } else {
                0
            };
        let copy = if let Some(existing) = store
            .removal_copy_of(handle, saved.transition_id)
            .map_err(rejected)?
        {
            Some(existing)
        } else {
            let ready = ready_session_for(&mut store, handle, &wrapping)?;
            if ready.has_unsent_local(&store).map_err(rejected)? {
                Some(
                    babytrack_core::portable_file::private_copy_after_removal(
                        &mut store, &ready, &saved, now_ms,
                    )
                    .map_err(rejected)?,
                )
            } else {
                None
            }
        };
        Ok(Some(RemovedDeviceRow {
            verified_cursor: saved.cursor,
            known_gap: saved.known_gap,
            private_copy: copy.map(Into::into),
            pending_result,
        }))
    }

    /// One bounded sync pass. The platform fetches bytes; Rust signs every
    /// exact path, verifies the full log and manifests, and decides readiness.
    pub fn sync_recipient(
        &self,
        family: FamilyRef,
        wrapping_key: Vec<u8>,
        transport: Box<dyn RelayReadTransport>,
    ) -> Result<RecipientSyncRow, BindingError> {
        let mut store = self.store.lock().map_err(|_| BindingError::LockPoisoned)?;
        let attempt = EnrollmentAttempt::resume(
            &mut store,
            family.handle()?.family_id,
            &fixed(&wrapping_key)?,
        )
        .map_err(rejected)?;
        if attempt.family() != family.handle()? {
            return Err(BindingError::InvalidBytes);
        }
        let attempt = Arc::new(attempt);
        let transport: Arc<dyn RelayReadTransport> = Arc::from(transport);
        let after = attempt.pending_control_cursor(&store).map_err(rejected)?;
        let path = format!(
            "/v1/families/{}/control?after={after}",
            lower_hex(&family.family_id)
        );
        let auth = attempt.sign_get(&path).map_err(rejected)?.bytes;
        let page_bytes = transport.get(path, auth)?;
        let page = ControlPage::decode(&page_bytes, family.handle()?.family_id, after)
            .map_err(rejected)?;
        for entry in page.entries {
            attempt
                .accept_sparse_control(&mut store, &entry.committed_bytes)
                .map_err(rejected)?;
        }
        let pending_control_cursor = attempt.pending_control_cursor(&store).map_err(rejected)?;
        if !attempt.has_committed_admission(&store).map_err(rejected)? {
            return Ok(RecipientSyncRow {
                verified_cursor: PublicHistorySession::resume(&store, family.handle()?)
                    .map_err(rejected)?
                    .cursor(),
                pending_control_cursor,
                awaiting_grant: true,
                no_more_visible: false,
                remaining_objects: true,
                ready: false,
                child_count: 0,
                removed: false,
                private_copy: None,
                pending_result: 0,
            });
        }
        let pull = futures::executor::block_on(active_pull::pull_active_log(
            &mut store,
            family.handle()?,
            4,
            |path| {
                let attempt = Arc::clone(&attempt);
                let transport = Arc::clone(&transport);
                async move {
                    let auth = attempt.sign_get(&path).map_err(rejected)?.bytes;
                    transport.get(path, auth)
                }
            },
        ))
        .map_err(rejected)?;
        let hydration = futures::executor::block_on(active_pull::hydrate_manifest_objects(
            &mut store,
            family.handle()?,
            16,
            |path| {
                let attempt = Arc::clone(&attempt);
                let transport = Arc::clone(&transport);
                async move {
                    let auth = attempt.sign_get(&path).map_err(rejected)?.bytes;
                    transport.get(path, auth)
                }
            },
        ))
        .map_err(rejected)?;
        if !pull.no_more_visible || hydration.remaining {
            return Ok(RecipientSyncRow {
                verified_cursor: pull.verified_cursor,
                pending_control_cursor,
                awaiting_grant: false,
                no_more_visible: pull.no_more_visible,
                remaining_objects: hydration.remaining,
                ready: false,
                child_count: 0,
                removed: false,
                private_copy: None,
                pending_result: 0,
            });
        }
        let ready = ReadyFamilySession::from_enrollment(&store, &attempt).map_err(rejected)?;
        let children = ready
            .projection()
            .records()
            .filter(|record| {
                record.scope == babytrack_core::operation::Scope::Child && !record.deleted
            })
            .count();
        Ok(RecipientSyncRow {
            verified_cursor: pull.verified_cursor,
            pending_control_cursor,
            awaiting_grant: false,
            no_more_visible: true,
            remaining_objects: false,
            ready: true,
            child_count: children as u64,
            removed: false,
            private_copy: None,
            pending_result: 0,
        })
    }

    /// A usable view requires a locally held key and complete verified
    /// history. Include durable offline operations without publishing them.
    pub fn shared_snapshot(
        &self,
        family: FamilyRef,
        wrapping_key: Vec<u8>,
    ) -> Result<SharedSnapshotRow, BindingError> {
        let mut store = self.store.lock().map_err(|_| BindingError::LockPoisoned)?;
        let ready = ready_session_for(&mut store, family.handle()?, &fixed(&wrapping_key)?)?;
        let projection = ready.projection_with_pending(&store).map_err(rejected)?;
        let devices = PublicHistorySession::resume(&store, family.handle()?)
            .map_err(rejected)?
            .chain()
            .active_devices()
            .map_err(rejected)?
            .into_iter()
            .map(|device| SharedDeviceRow {
                device_id: device.device_id.to_vec(),
                role: device.role,
            })
            .collect();
        let inert_count = projection.inert_batches().len() as u64;
        let recent_inert = projection
            .inert_batches()
            .iter()
            .rev()
            .take(16)
            .map(|row| InertBatchRow {
                cursor: row.cursor,
                object_hash: row.object_hash.to_vec(),
                reason: row.reason.clone(),
            })
            .collect();
        let children = local_api::children_from_records(projection.records())
            .into_iter()
            .map(|child| ChildRow {
                id: child.id.to_vec(),
                name: child.name,
                birth_day: child.birth_day,
                sex: child.sex,
            })
            .collect();
        let activities = local_api::activities_from_records(projection.records())
            .into_iter()
            .map(|row| ActivityRow {
                id: row.id.to_vec(),
                child_id: row.child_id.to_vec(),
                kind: row.kind,
                start_utc_ms: row.start_utc_ms,
                offset_minutes: row.offset_minutes,
                end_utc_ms: row.end_utc_ms,
                note: row.note,
                diaper_kind: row.diaper_kind,
                bottle_ml: row.bottle_ml,
                breast_side: row.breast_side,
                breast_segments: row
                    .breast_segments
                    .map(|segments| segments.into_iter().map(breast_segment_row).collect()),
                solids_foods: row.solids_foods,
                solids_amount: row.solids_amount,
                pump_left_ml: row.pump_left_ml,
                pump_right_ml: row.pump_right_ml,
                pump_total_ml: row.pump_total_ml,
                growth_weight_g: row.growth_weight_g,
                growth_length_mm: row.growth_length_mm,
                temperature_c: row.temperature_c,
                medication_name: row.medication_name,
                medication_dose_amount: row.medication_dose_amount,
                medication_dose_unit: row.medication_dose_unit,
            })
            .collect();
        Ok(SharedSnapshotRow {
            family,
            verified_cursor: ready.observed_cursor(),
            children,
            activities,
            unsent_count: ready.unsent_local_count(&store).map_err(rejected)?,
            inert_count,
            recent_inert,
            devices,
        })
    }

    /// A shared file contains the verified prefix and durable local edits.
    /// Restoring it always creates an independent local-only Family.
    pub fn shared_backup_file(
        &self,
        family: FamilyRef,
        wrapping_key: Vec<u8>,
        now_ms: i64,
        password: Option<String>,
        available_memory_bytes: u64,
    ) -> Result<BackupFileRow, BindingError> {
        let mut store = self.store.lock().map_err(|_| BindingError::LockPoisoned)?;
        let ready = ready_session_for(&mut store, family.handle()?, &fixed(&wrapping_key)?)?;
        let readable =
            babytrack_core::portable_file::export_readable_shared(&store, &ready, now_ms)
                .map_err(rejected)?;
        let info = LocalRepository::inspect_readable(&readable).map_err(rejected)?;
        let bytes = if let Some(password) = password {
            babytrack_core::portable_file::protect_readable(
                &readable,
                &password,
                available_memory_bytes,
            )
            .map_err(rejected)?
        } else {
            readable
        };
        Ok(BackupFileRow {
            bytes,
            revision: 0,
            info: info.into(),
        })
    }

    pub fn shared_analysis_csv(
        &self,
        family: FamilyRef,
        wrapping_key: Vec<u8>,
    ) -> Result<Vec<u8>, BindingError> {
        let mut store = self.store.lock().map_err(|_| BindingError::LockPoisoned)?;
        let ready = ready_session_for(&mut store, family.handle()?, &fixed(&wrapping_key)?)?;
        let projection = ready.projection_with_pending(&store).map_err(rejected)?;
        Ok(babytrack_core::analysis_csv::export(
            ready.family().family_id,
            projection.records(),
        ))
    }

    /// Repeated requests return the same independent destination Family.
    pub fn private_copy_shared(
        &self,
        family: FamilyRef,
        wrapping_key: Vec<u8>,
        now_ms: i64,
    ) -> Result<FamilyRef, BindingError> {
        let mut store = self.store.lock().map_err(|_| BindingError::LockPoisoned)?;
        let ready = ready_session_for(&mut store, family.handle()?, &fixed(&wrapping_key)?)?;
        Ok(
            babytrack_core::portable_file::private_copy_shared(&mut store, &ready, now_ms)
                .map_err(rejected)?
                .into(),
        )
    }

    pub fn is_shared(&self, family: FamilyRef) -> Result<bool, BindingError> {
        self.store
            .lock()
            .map_err(|_| BindingError::LockPoisoned)?
            .is_shared_family(family.handle()?)
            .map_err(rejected)
    }

    pub fn recipient_families(&self) -> Result<Vec<FamilyRef>, BindingError> {
        let store = self.store.lock().map_err(|_| BindingError::LockPoisoned)?;
        store
            .families()
            .map_err(rejected)?
            .into_iter()
            .filter_map(
                |family| match store.has_enrollment_attempt(family.family_id) {
                    Ok(true) => Some(Ok(family.into())),
                    Ok(false) => None,
                    Err(error) => Some(Err(rejected(error))),
                },
            )
            .collect::<Result<Vec<_>, _>>()
    }

    pub fn recipient_relay_origin(
        &self,
        family: FamilyRef,
        wrapping_key: Vec<u8>,
    ) -> Result<String, BindingError> {
        let mut store = self.store.lock().map_err(|_| BindingError::LockPoisoned)?;
        let attempt = EnrollmentAttempt::resume(
            &mut store,
            family.handle()?.family_id,
            &fixed(&wrapping_key)?,
        )
        .map_err(rejected)?;
        if attempt.family() != family.handle()? {
            return Err(BindingError::InvalidBytes);
        }
        attempt.relay_origin().map_err(rejected)
    }

    pub fn manager_first_join_action(
        &self,
        family: FamilyRef,
        wrapping_key: Vec<u8>,
    ) -> Result<u8, BindingError> {
        let store = self.store.lock().map_err(|_| BindingError::LockPoisoned)?;
        let manager = ManagerCreation::resume(&store, family.handle()?, &fixed(&wrapping_key)?)
            .map_err(rejected)?;
        manager.first_join_action(&store).map_err(rejected)
    }

    pub fn recipient_first_join_action(
        &self,
        family: FamilyRef,
        wrapping_key: Vec<u8>,
    ) -> Result<u8, BindingError> {
        let mut store = self.store.lock().map_err(|_| BindingError::LockPoisoned)?;
        let attempt = EnrollmentAttempt::resume(
            &mut store,
            family.handle()?.family_id,
            &fixed(&wrapping_key)?,
        )
        .map_err(rejected)?;
        if attempt.family() != family.handle()? {
            return Err(BindingError::InvalidBytes);
        }
        attempt.first_join_action(&store).map_err(rejected)
    }

    pub fn add_shared_child(
        &self,
        family: FamilyRef,
        wrapping_key: Vec<u8>,
        name: String,
        now_ms: i64,
    ) -> Result<Vec<u8>, BindingError> {
        self.add_shared_child_with_metadata(family, wrapping_key, name, None, None, now_ms)
    }

    pub fn add_shared_child_with_metadata(
        &self,
        family: FamilyRef,
        wrapping_key: Vec<u8>,
        name: String,
        birth_day: Option<i64>,
        sex: Option<u8>,
        now_ms: i64,
    ) -> Result<Vec<u8>, BindingError> {
        let mut store = self.store.lock().map_err(|_| BindingError::LockPoisoned)?;
        let handle = family.handle()?;
        let ready = ready_session_for(&mut store, handle, &fixed(&wrapping_key)?)?;
        let (id, operation) =
            local_api::child_operation_with_metadata(handle, &name, birth_day, sex, now_ms)
                .map_err(rejected)?;
        ready
            .append_local(&mut store, operation, now_ms)
            .map_err(rejected)?;
        Ok(id.to_vec())
    }

    pub fn log_shared_diaper(
        &self,
        family: FamilyRef,
        wrapping_key: Vec<u8>,
        child_id: Vec<u8>,
        kind: u8,
        time: ActivityWhen,
    ) -> Result<Vec<u8>, BindingError> {
        let mut store = self.store.lock().map_err(|_| BindingError::LockPoisoned)?;
        let handle = family.handle()?;
        let ready = ready_session_for(&mut store, handle, &fixed(&wrapping_key)?)?;
        let saved_at_ms = time.saved_at_ms;
        let (id, operation) =
            local_api::diaper_operation(handle, fixed(&child_id)?, kind, time.into())
                .map_err(rejected)?;
        ready
            .append_local(&mut store, operation, saved_at_ms)
            .map_err(rejected)?;
        Ok(id.to_vec())
    }

    pub fn log_shared_bottle_ml(
        &self,
        family: FamilyRef,
        wrapping_key: Vec<u8>,
        child_id: Vec<u8>,
        amount_ml: i64,
        content: u8,
        time: ActivityWhen,
    ) -> Result<Vec<u8>, BindingError> {
        let mut store = self.store.lock().map_err(|_| BindingError::LockPoisoned)?;
        let handle = family.handle()?;
        let ready = ready_session_for(&mut store, handle, &fixed(&wrapping_key)?)?;
        let saved_at_ms = time.saved_at_ms;
        let (id, operation) =
            local_api::bottle_operation(handle, fixed(&child_id)?, amount_ml, content, time.into())
                .map_err(rejected)?;
        ready
            .append_local(&mut store, operation, saved_at_ms)
            .map_err(rejected)?;
        Ok(id.to_vec())
    }

    pub fn log_shared_breast_feed(
        &self,
        family: FamilyRef,
        wrapping_key: Vec<u8>,
        child_id: Vec<u8>,
        side: u8,
        time: ActivityWhen,
        end_utc_ms: i64,
    ) -> Result<Vec<u8>, BindingError> {
        let mut store = self.store.lock().map_err(|_| BindingError::LockPoisoned)?;
        let handle = family.handle()?;
        let ready = ready_session_for(&mut store, handle, &fixed(&wrapping_key)?)?;
        let saved_at_ms = time.saved_at_ms;
        let (id, operation) = local_api::breast_feed_operation(
            handle,
            fixed(&child_id)?,
            side,
            time.into(),
            end_utc_ms,
        )
        .map_err(rejected)?;
        ready
            .append_local(&mut store, operation, saved_at_ms)
            .map_err(rejected)?;
        Ok(id.to_vec())
    }

    pub fn log_shared_breast_feed_segments(
        &self,
        family: FamilyRef,
        wrapping_key: Vec<u8>,
        child_id: Vec<u8>,
        segments: Vec<BreastSegmentRow>,
        time: ActivityWhen,
    ) -> Result<Vec<u8>, BindingError> {
        let mut store = self.store.lock().map_err(|_| BindingError::LockPoisoned)?;
        let handle = family.handle()?;
        let ready = ready_session_for(&mut store, handle, &fixed(&wrapping_key)?)?;
        let saved_at_ms = time.saved_at_ms;
        let segments = segments.into_iter().map(Into::into).collect::<Vec<_>>();
        let (id, operation) = local_api::breast_feed_segments_operation(
            handle,
            fixed(&child_id)?,
            &segments,
            time.into(),
        )
        .map_err(rejected)?;
        ready
            .append_local(&mut store, operation, saved_at_ms)
            .map_err(rejected)?;
        Ok(id.to_vec())
    }

    pub fn log_shared_pump(
        &self,
        family: FamilyRef,
        wrapping_key: Vec<u8>,
        child_id: Vec<u8>,
        input: PumpInput,
        time: ActivityWhen,
        end_utc_ms: i64,
    ) -> Result<Vec<u8>, BindingError> {
        let mut store = self.store.lock().map_err(|_| BindingError::LockPoisoned)?;
        let handle = family.handle()?;
        let ready = ready_session_for(&mut store, handle, &fixed(&wrapping_key)?)?;
        let saved_at_ms = time.saved_at_ms;
        let (id, operation) = local_api::pump_operation(
            handle,
            fixed(&child_id)?,
            input.into(),
            time.into(),
            end_utc_ms,
        )
        .map_err(rejected)?;
        ready
            .append_local(&mut store, operation, saved_at_ms)
            .map_err(rejected)?;
        Ok(id.to_vec())
    }

    pub fn log_shared_solids(
        &self,
        family: FamilyRef,
        wrapping_key: Vec<u8>,
        child_id: Vec<u8>,
        foods: Vec<String>,
        amount: String,
        time: ActivityWhen,
    ) -> Result<Vec<u8>, BindingError> {
        let mut store = self.store.lock().map_err(|_| BindingError::LockPoisoned)?;
        let handle = family.handle()?;
        let ready = ready_session_for(&mut store, handle, &fixed(&wrapping_key)?)?;
        let saved_at_ms = time.saved_at_ms;
        let (id, operation) =
            local_api::solids_operation(handle, fixed(&child_id)?, &foods, &amount, time.into())
                .map_err(rejected)?;
        ready
            .append_local(&mut store, operation, saved_at_ms)
            .map_err(rejected)?;
        Ok(id.to_vec())
    }

    pub fn log_shared_sleep(
        &self,
        family: FamilyRef,
        wrapping_key: Vec<u8>,
        child_id: Vec<u8>,
        time: ActivityWhen,
        end_utc_ms: i64,
        end_offset_minutes: i16,
    ) -> Result<Vec<u8>, BindingError> {
        let mut store = self.store.lock().map_err(|_| BindingError::LockPoisoned)?;
        let handle = family.handle()?;
        let ready = ready_session_for(&mut store, handle, &fixed(&wrapping_key)?)?;
        let saved_at_ms = time.saved_at_ms;
        let (id, operation) = local_api::sleep_operation(
            handle,
            fixed(&child_id)?,
            time.into(),
            end_utc_ms,
            end_offset_minutes,
        )
        .map_err(rejected)?;
        ready
            .append_local(&mut store, operation, saved_at_ms)
            .map_err(rejected)?;
        Ok(id.to_vec())
    }

    pub fn start_shared_sleep(
        &self,
        family: FamilyRef,
        wrapping_key: Vec<u8>,
        child_id: Vec<u8>,
        time: ActivityWhen,
    ) -> Result<Vec<u8>, BindingError> {
        let mut store = self.store.lock().map_err(|_| BindingError::LockPoisoned)?;
        let handle = family.handle()?;
        let ready = ready_session_for(&mut store, handle, &fixed(&wrapping_key)?)?;
        let saved_at_ms = time.saved_at_ms;
        let (id, operation) =
            local_api::running_sleep_operation(handle, fixed(&child_id)?, time.into())
                .map_err(rejected)?;
        ready
            .append_local(&mut store, operation, saved_at_ms)
            .map_err(rejected)?;
        Ok(id.to_vec())
    }

    pub fn stop_shared_sleep(
        &self,
        family: FamilyRef,
        wrapping_key: Vec<u8>,
        child_id: Vec<u8>,
        activity_id: Vec<u8>,
        end: ActivityWhen,
    ) -> Result<(), BindingError> {
        let mut store = self.store.lock().map_err(|_| BindingError::LockPoisoned)?;
        let handle = family.handle()?;
        let ready = ready_session_for(&mut store, handle, &fixed(&wrapping_key)?)?;
        let projection = ready.projection_with_pending(&store).map_err(rejected)?;
        let activity_id = fixed(&activity_id)?;
        let activity = projection
            .record(&activity_id)
            .ok_or(BindingError::InvalidBytes)?;
        let operation = local_api::stop_sleep_operation(
            handle,
            fixed(&child_id)?,
            activity,
            end.start_utc_ms,
            end.offset_minutes,
            end.saved_at_ms,
        )
        .map_err(rejected)?;
        ready
            .append_local(&mut store, operation, end.saved_at_ms)
            .map(|_| ())
            .map_err(rejected)
    }

    pub fn delete_shared_activity(
        &self,
        family: FamilyRef,
        wrapping_key: Vec<u8>,
        child_id: Vec<u8>,
        activity_id: Vec<u8>,
        saved_at_ms: i64,
    ) -> Result<(), BindingError> {
        let mut store = self.store.lock().map_err(|_| BindingError::LockPoisoned)?;
        let handle = family.handle()?;
        let ready = ready_session_for(&mut store, handle, &fixed(&wrapping_key)?)?;
        let projection = ready.projection_with_pending(&store).map_err(rejected)?;
        let child_id = fixed(&child_id)?;
        let child = projection
            .record(&child_id)
            .ok_or(BindingError::InvalidBytes)?;
        if child.scope != operation::Scope::Child || child.deleted {
            return Err(BindingError::InvalidBytes);
        }
        let activity_id = fixed(&activity_id)?;
        let activity = projection
            .record(&activity_id)
            .ok_or(BindingError::InvalidBytes)?;
        let operation =
            local_api::delete_activity_operation(handle, child_id, activity, saved_at_ms)
                .map_err(rejected)?;
        ready
            .append_local(&mut store, operation, saved_at_ms)
            .map(|_| ())
            .map_err(rejected)
    }

    pub fn edit_shared_note(
        &self,
        family: FamilyRef,
        wrapping_key: Vec<u8>,
        child_id: Vec<u8>,
        activity_id: Vec<u8>,
        note: String,
        saved_at_ms: i64,
    ) -> Result<(), BindingError> {
        let mut store = self.store.lock().map_err(|_| BindingError::LockPoisoned)?;
        let handle = family.handle()?;
        let ready = ready_session_for(&mut store, handle, &fixed(&wrapping_key)?)?;
        let projection = ready.projection_with_pending(&store).map_err(rejected)?;
        let child_id = fixed(&child_id)?;
        let child = projection
            .record(&child_id)
            .ok_or(BindingError::InvalidBytes)?;
        if child.scope != operation::Scope::Child || child.deleted {
            return Err(BindingError::InvalidBytes);
        }
        let activity = projection
            .record(&fixed(&activity_id)?)
            .ok_or(BindingError::InvalidBytes)?;
        let operation =
            local_api::edit_note_operation(handle, child_id, activity, &note, saved_at_ms)
                .map_err(rejected)?;
        ready
            .append_local(&mut store, operation, saved_at_ms)
            .map(|_| ())
            .map_err(rejected)
    }

    pub fn edit_shared_bottle_ml(
        &self,
        family: FamilyRef,
        wrapping_key: Vec<u8>,
        child_id: Vec<u8>,
        activity_id: Vec<u8>,
        amount_ml: i64,
        saved_at_ms: i64,
    ) -> Result<(), BindingError> {
        let mut store = self.store.lock().map_err(|_| BindingError::LockPoisoned)?;
        let handle = family.handle()?;
        let ready = ready_session_for(&mut store, handle, &fixed(&wrapping_key)?)?;
        let projection = ready.projection_with_pending(&store).map_err(rejected)?;
        let child_id = fixed(&child_id)?;
        let child = projection
            .record(&child_id)
            .ok_or(BindingError::InvalidBytes)?;
        if child.scope != operation::Scope::Child || child.deleted {
            return Err(BindingError::InvalidBytes);
        }
        let activity = projection
            .record(&fixed(&activity_id)?)
            .ok_or(BindingError::InvalidBytes)?;
        let operation =
            local_api::edit_bottle_ml_operation(handle, child_id, activity, amount_ml, saved_at_ms)
                .map_err(rejected)?;
        ready
            .append_local(&mut store, operation, saved_at_ms)
            .map(|_| ())
            .map_err(rejected)
    }

    pub fn log_shared_note(
        &self,
        family: FamilyRef,
        wrapping_key: Vec<u8>,
        child_id: Vec<u8>,
        note: String,
        time: ActivityWhen,
    ) -> Result<Vec<u8>, BindingError> {
        let mut store = self.store.lock().map_err(|_| BindingError::LockPoisoned)?;
        let handle = family.handle()?;
        let ready = ready_session_for(&mut store, handle, &fixed(&wrapping_key)?)?;
        let saved_at_ms = time.saved_at_ms;
        let (id, operation) =
            local_api::note_operation(handle, fixed(&child_id)?, &note, time.into())
                .map_err(rejected)?;
        ready
            .append_local(&mut store, operation, saved_at_ms)
            .map_err(rejected)?;
        Ok(id.to_vec())
    }

    pub fn log_shared_growth(
        &self,
        family: FamilyRef,
        wrapping_key: Vec<u8>,
        child_id: Vec<u8>,
        weight_g: Option<i64>,
        length_mm: Option<i64>,
        time: ActivityWhen,
    ) -> Result<Vec<u8>, BindingError> {
        let mut store = self.store.lock().map_err(|_| BindingError::LockPoisoned)?;
        let handle = family.handle()?;
        let ready = ready_session_for(&mut store, handle, &fixed(&wrapping_key)?)?;
        let saved_at_ms = time.saved_at_ms;
        let (id, operation) = local_api::growth_operation(
            handle,
            fixed(&child_id)?,
            weight_g,
            length_mm,
            time.into(),
        )
        .map_err(rejected)?;
        ready
            .append_local(&mut store, operation, saved_at_ms)
            .map_err(rejected)?;
        Ok(id.to_vec())
    }

    pub fn log_shared_temperature_c(
        &self,
        family: FamilyRef,
        wrapping_key: Vec<u8>,
        child_id: Vec<u8>,
        entered_c: String,
        time: ActivityWhen,
    ) -> Result<Vec<u8>, BindingError> {
        let mut store = self.store.lock().map_err(|_| BindingError::LockPoisoned)?;
        let handle = family.handle()?;
        let ready = ready_session_for(&mut store, handle, &fixed(&wrapping_key)?)?;
        let saved_at_ms = time.saved_at_ms;
        let (id, operation) =
            local_api::temperature_c_operation(handle, fixed(&child_id)?, &entered_c, time.into())
                .map_err(rejected)?;
        ready
            .append_local(&mut store, operation, saved_at_ms)
            .map_err(rejected)?;
        Ok(id.to_vec())
    }

    pub fn log_shared_medication(
        &self,
        family: FamilyRef,
        wrapping_key: Vec<u8>,
        child_id: Vec<u8>,
        input: MedicationInput,
        time: ActivityWhen,
    ) -> Result<Vec<u8>, BindingError> {
        let mut store = self.store.lock().map_err(|_| BindingError::LockPoisoned)?;
        let handle = family.handle()?;
        let ready = ready_session_for(&mut store, handle, &fixed(&wrapping_key)?)?;
        let saved_at_ms = time.saved_at_ms;
        let (id, operation) = local_api::medication_operation(
            handle,
            fixed(&child_id)?,
            &input.name,
            &input.dose_amount,
            &input.dose_unit,
            time.into(),
        )
        .map_err(rejected)?;
        ready
            .append_local(&mut store, operation, saved_at_ms)
            .map_err(rejected)?;
        Ok(id.to_vec())
    }

    /// Return the exact durable next envelope. A lost HTTP response must
    /// retry these bytes until the signed log or rejection resolves it.
    pub fn prepare_shared_upload(
        &self,
        family: FamilyRef,
        wrapping_key: Vec<u8>,
    ) -> Result<Option<Vec<u8>>, BindingError> {
        let mut store = self.store.lock().map_err(|_| BindingError::LockPoisoned)?;
        let handle = family.handle()?;
        if store.saved_removal(handle).map_err(rejected)?.is_some() {
            return Err(BindingError::Rejected("device removal verified".to_owned()));
        }
        let wrapping = fixed(&wrapping_key)?;
        let ready = ready_session_for(&mut store, handle, &wrapping)?;
        if !ready.has_unsent_local(&store).map_err(rejected)? {
            return Ok(None);
        }
        let staged = if store
            .has_enrollment_attempt(handle.family_id)
            .map_err(rejected)?
        {
            let attempt = EnrollmentAttempt::resume(&mut store, handle.family_id, &wrapping)
                .map_err(rejected)?;
            ready
                .stage_enrolled_local(&mut store, &attempt)
                .map_err(rejected)?
        } else {
            let manager = ManagerCreation::resume(&store, handle, &wrapping).map_err(rejected)?;
            manager
                .stage_next_local(&ready, &mut store)
                .map_err(rejected)?
        };
        let batch = match staged {
            babytrack_core::shared_ready::NextUpload::Fresh(value)
            | babytrack_core::shared_ready::NextUpload::RetryExact(value) => value,
        };
        Ok(Some(batch.envelope_bytes))
    }

    /// Pull the active log and referenced objects through a byte transport.
    /// Only full Rust verification advances the durable cursor and outbox.
    pub fn sync_shared(
        &self,
        family: FamilyRef,
        wrapping_key: Vec<u8>,
        transport: Box<dyn RelayReadTransport>,
    ) -> Result<SharedSyncRow, BindingError> {
        let mut store = self.store.lock().map_err(|_| BindingError::LockPoisoned)?;
        let handle = family.handle()?;
        if store.saved_removal(handle).map_err(rejected)?.is_some() {
            return Err(BindingError::Rejected("device removal verified".to_owned()));
        }
        let wrapping = fixed(&wrapping_key)?;
        let signer = if store
            .has_enrollment_attempt(handle.family_id)
            .map_err(rejected)?
        {
            let attempt = EnrollmentAttempt::resume(&mut store, handle.family_id, &wrapping)
                .map_err(rejected)?;
            if attempt.family() != handle
                || !attempt.has_committed_admission(&store).map_err(rejected)?
            {
                return Err(BindingError::InvalidBytes);
            }
            ActiveReadSigner::Recipient(attempt)
        } else {
            ActiveReadSigner::Manager(
                ManagerCreation::resume(&store, handle, &wrapping).map_err(rejected)?,
            )
        };
        let signer = Arc::new(signer);
        let transport: Arc<dyn RelayReadTransport> = Arc::from(transport);
        let pull = futures::executor::block_on(active_pull::pull_active_log(
            &mut store,
            handle,
            4,
            |path| {
                let signer = Arc::clone(&signer);
                let transport = Arc::clone(&transport);
                async move {
                    let auth = signer.sign_get(&path)?.bytes;
                    transport.get(path, auth)
                }
            },
        ))
        .map_err(rejected)?;
        let hydration = futures::executor::block_on(active_pull::hydrate_manifest_objects(
            &mut store,
            handle,
            16,
            |path| {
                let signer = Arc::clone(&signer);
                let transport = Arc::clone(&transport);
                async move {
                    let auth = signer.sign_get(&path)?.bytes;
                    transport.get(path, auth)
                }
            },
        ))
        .map_err(rejected)?;
        let (ready, outbox_state, inert_count) = if pull.no_more_visible && !hydration.remaining {
            let ready = ready_session_for(&mut store, handle, &wrapping)?;
            let unsent = ready.unsent_local_count(&store).map_err(rejected)?;
            let public = PublicHistorySession::resume(&store, handle).map_err(rejected)?;
            let outbox = if unsent == 0 {
                0
            } else if public.pending_batch_id(&store).map_err(rejected)?.is_some() {
                2
            } else {
                1
            };
            (
                true,
                outbox,
                ready.projection().inert_batches().len() as u64,
            )
        } else {
            (false, 0, 0)
        };
        Ok(SharedSyncRow {
            verified_cursor: pull.verified_cursor,
            no_more_visible: pull.no_more_visible,
            remaining_objects: hydration.remaining,
            ready,
            outbox_state,
            inert_count,
        })
    }

    /// Query the signed outcome of exact uncertain bytes. Code 0 means no
    /// outbox, 1 no result, 2 accepted ahead of our verified log, 3 verified
    /// rejection rebased the outbox, and 4 a different rejection blocks it.
    pub fn resolve_pending_batch_result(
        &self,
        family: FamilyRef,
        wrapping_key: Vec<u8>,
        transport: Box<dyn RelayReadTransport>,
    ) -> Result<u8, BindingError> {
        let mut store = self.store.lock().map_err(|_| BindingError::LockPoisoned)?;
        let handle = family.handle()?;
        let public = PublicHistorySession::resume(&store, handle).map_err(rejected)?;
        let Some(batch_id) = public.pending_batch_id(&store).map_err(rejected)? else {
            return Ok(0);
        };
        let wrapping = fixed(&wrapping_key)?;
        let signer = if store
            .has_enrollment_attempt(handle.family_id)
            .map_err(rejected)?
        {
            let attempt = EnrollmentAttempt::resume(&mut store, handle.family_id, &wrapping)
                .map_err(rejected)?;
            if attempt.family() != handle {
                return Err(BindingError::InvalidBytes);
            }
            ActiveReadSigner::Recipient(attempt)
        } else {
            ActiveReadSigner::Manager(
                ManagerCreation::resume(&store, handle, &wrapping).map_err(rejected)?,
            )
        };
        let path = format!(
            "/v1/families/{}/batch-results/{}",
            lower_hex(&family.family_id),
            lower_hex(&batch_id),
        );
        let auth = signer.sign_get(&path)?.bytes;
        let result = transport.get(path, auth)?;
        Ok(
            match public
                .resolve_pending_result(&mut store, &result)
                .map_err(rejected)?
            {
                PendingBatchResult::NoPending => 0,
                PendingBatchResult::Unresolved => 1,
                PendingBatchResult::AcceptedAhead => 2,
                PendingBatchResult::Rebased => 3,
                PendingBatchResult::Blocked => 4,
            },
        )
    }
}

enum ActiveReadSigner {
    Recipient(EnrollmentAttempt),
    Manager(ManagerCreation),
}
impl ActiveReadSigner {
    fn sign_get(&self, path: &str) -> Result<babytrack_core::sync_wire::SignedRead, BindingError> {
        match self {
            Self::Recipient(value) => value.sign_get(path).map_err(rejected),
            Self::Manager(value) => value.sign_get(path).map_err(rejected),
        }
    }
}

impl From<ActivityWhen> for ActivityTime {
    fn from(value: ActivityWhen) -> Self {
        Self {
            start_utc_ms: value.start_utc_ms,
            offset_minutes: value.offset_minutes,
            saved_at_ms: value.saved_at_ms,
        }
    }
}

fn ready_session_for(
    store: &mut SqliteStore,
    family: FamilyHandle,
    wrapping: &[u8; 32],
) -> Result<ReadyFamilySession, BindingError> {
    if store
        .has_enrollment_attempt(family.family_id)
        .map_err(rejected)?
    {
        let attempt =
            EnrollmentAttempt::resume(store, family.family_id, wrapping).map_err(rejected)?;
        if attempt.family() != family {
            return Err(BindingError::InvalidBytes);
        }
        ReadyFamilySession::from_enrollment(store, &attempt).map_err(rejected)
    } else {
        ManagerCreation::resume(store, family, wrapping)
            .map_err(rejected)?
            .ready_session(store)
            .map_err(rejected)
    }
}

#[uniffi::export]
pub fn preview_invitation(fragment: String) -> Result<InvitationPreviewRow, BindingError> {
    let bootstrap = InvitationBootstrap::from_fragment(&fragment).map_err(rejected)?;
    let path = format!(
        "/v1/families/{}/control?after=0",
        lower_hex(&bootstrap.family_id())
    );
    let read = bootstrap.sign_get(&path).map_err(rejected)?;
    Ok(InvitationPreviewRow {
        family_id: bootstrap.family_id().to_vec(),
        relay_origin: bootstrap.relay_origin().to_owned(),
        role: bootstrap.fixed_role(),
        control_path: path,
        read_auth: read.bytes,
    })
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
                birth_day: child.birth_day,
                sex: child.sex,
            })
            .collect())
    }

    pub fn add_child(
        &self,
        family: FamilyRef,
        name: String,
        now_ms: i64,
    ) -> Result<Vec<u8>, BindingError> {
        self.add_child_with_metadata(family, name, None, None, now_ms)
    }

    pub fn add_child_with_metadata(
        &self,
        family: FamilyRef,
        name: String,
        birth_day: Option<i64>,
        sex: Option<u8>,
        now_ms: i64,
    ) -> Result<Vec<u8>, BindingError> {
        Ok(self
            .repo
            .lock()
            .map_err(|_| BindingError::LockPoisoned)?
            .add_child_with_metadata(family.handle()?, &name, birth_day, sex, now_ms)
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

    pub fn log_breast_feed(
        &self,
        family: FamilyRef,
        child_id: Vec<u8>,
        side: u8,
        time: ActivityWhen,
        end_utc_ms: i64,
    ) -> Result<Vec<u8>, BindingError> {
        Ok(self
            .repo
            .lock()
            .map_err(|_| BindingError::LockPoisoned)?
            .log_breast_feed(
                family.handle()?,
                fixed(&child_id)?,
                side,
                time.into(),
                end_utc_ms,
            )
            .map_err(rejected)?
            .to_vec())
    }

    pub fn log_breast_feed_segments(
        &self,
        family: FamilyRef,
        child_id: Vec<u8>,
        segments: Vec<BreastSegmentRow>,
        time: ActivityWhen,
    ) -> Result<Vec<u8>, BindingError> {
        Ok(self
            .repo
            .lock()
            .map_err(|_| BindingError::LockPoisoned)?
            .log_breast_feed_segments(
                family.handle()?,
                fixed(&child_id)?,
                segments.into_iter().map(Into::into).collect(),
                time.into(),
            )
            .map_err(rejected)?
            .to_vec())
    }

    pub fn log_pump(
        &self,
        family: FamilyRef,
        child_id: Vec<u8>,
        input: PumpInput,
        time: ActivityWhen,
        end_utc_ms: i64,
    ) -> Result<Vec<u8>, BindingError> {
        Ok(self
            .repo
            .lock()
            .map_err(|_| BindingError::LockPoisoned)?
            .log_pump(
                family.handle()?,
                fixed(&child_id)?,
                input.into(),
                time.into(),
                end_utc_ms,
            )
            .map_err(rejected)?
            .to_vec())
    }

    pub fn log_solids(
        &self,
        family: FamilyRef,
        child_id: Vec<u8>,
        foods: Vec<String>,
        amount: String,
        time: ActivityWhen,
    ) -> Result<Vec<u8>, BindingError> {
        Ok(self
            .repo
            .lock()
            .map_err(|_| BindingError::LockPoisoned)?
            .log_solids(
                family.handle()?,
                fixed(&child_id)?,
                &foods,
                &amount,
                time.into(),
            )
            .map_err(rejected)?
            .to_vec())
    }

    pub fn log_sleep(
        &self,
        family: FamilyRef,
        child_id: Vec<u8>,
        time: ActivityWhen,
        end_utc_ms: i64,
        end_offset_minutes: i16,
    ) -> Result<Vec<u8>, BindingError> {
        Ok(self
            .repo
            .lock()
            .map_err(|_| BindingError::LockPoisoned)?
            .log_sleep(
                family.handle()?,
                fixed(&child_id)?,
                time.into(),
                end_utc_ms,
                end_offset_minutes,
            )
            .map_err(rejected)?
            .to_vec())
    }

    pub fn start_sleep(
        &self,
        family: FamilyRef,
        child_id: Vec<u8>,
        time: ActivityWhen,
    ) -> Result<Vec<u8>, BindingError> {
        Ok(self
            .repo
            .lock()
            .map_err(|_| BindingError::LockPoisoned)?
            .start_sleep(family.handle()?, fixed(&child_id)?, time.into())
            .map_err(rejected)?
            .to_vec())
    }

    pub fn stop_sleep(
        &self,
        family: FamilyRef,
        child_id: Vec<u8>,
        activity_id: Vec<u8>,
        end_utc_ms: i64,
        end_offset_minutes: i16,
        saved_at_ms: i64,
    ) -> Result<(), BindingError> {
        self.repo
            .lock()
            .map_err(|_| BindingError::LockPoisoned)?
            .stop_sleep(
                family.handle()?,
                fixed(&child_id)?,
                fixed(&activity_id)?,
                end_utc_ms,
                end_offset_minutes,
                saved_at_ms,
            )
            .map_err(rejected)
    }

    pub fn delete_activity(
        &self,
        family: FamilyRef,
        child_id: Vec<u8>,
        activity_id: Vec<u8>,
        saved_at_ms: i64,
    ) -> Result<(), BindingError> {
        self.repo
            .lock()
            .map_err(|_| BindingError::LockPoisoned)?
            .delete_activity(
                family.handle()?,
                fixed(&child_id)?,
                fixed(&activity_id)?,
                saved_at_ms,
            )
            .map_err(rejected)
    }

    pub fn edit_note(
        &self,
        family: FamilyRef,
        child_id: Vec<u8>,
        activity_id: Vec<u8>,
        note: String,
        saved_at_ms: i64,
    ) -> Result<(), BindingError> {
        self.repo
            .lock()
            .map_err(|_| BindingError::LockPoisoned)?
            .edit_note(
                family.handle()?,
                fixed(&child_id)?,
                fixed(&activity_id)?,
                &note,
                saved_at_ms,
            )
            .map_err(rejected)
    }

    pub fn edit_bottle_ml(
        &self,
        family: FamilyRef,
        child_id: Vec<u8>,
        activity_id: Vec<u8>,
        amount_ml: i64,
        saved_at_ms: i64,
    ) -> Result<(), BindingError> {
        self.repo
            .lock()
            .map_err(|_| BindingError::LockPoisoned)?
            .edit_bottle_ml(
                family.handle()?,
                fixed(&child_id)?,
                fixed(&activity_id)?,
                amount_ml,
                saved_at_ms,
            )
            .map_err(rejected)
    }

    pub fn log_note(
        &self,
        family: FamilyRef,
        child_id: Vec<u8>,
        note: String,
        time: ActivityWhen,
    ) -> Result<Vec<u8>, BindingError> {
        Ok(self
            .repo
            .lock()
            .map_err(|_| BindingError::LockPoisoned)?
            .log_note(family.handle()?, fixed(&child_id)?, &note, time.into())
            .map_err(rejected)?
            .to_vec())
    }

    pub fn log_growth(
        &self,
        family: FamilyRef,
        child_id: Vec<u8>,
        weight_g: Option<i64>,
        length_mm: Option<i64>,
        time: ActivityWhen,
    ) -> Result<Vec<u8>, BindingError> {
        Ok(self
            .repo
            .lock()
            .map_err(|_| BindingError::LockPoisoned)?
            .log_growth(
                family.handle()?,
                fixed(&child_id)?,
                weight_g,
                length_mm,
                time.into(),
            )
            .map_err(rejected)?
            .to_vec())
    }

    pub fn log_temperature_c(
        &self,
        family: FamilyRef,
        child_id: Vec<u8>,
        entered_c: String,
        time: ActivityWhen,
    ) -> Result<Vec<u8>, BindingError> {
        Ok(self
            .repo
            .lock()
            .map_err(|_| BindingError::LockPoisoned)?
            .log_temperature_c(family.handle()?, fixed(&child_id)?, &entered_c, time.into())
            .map_err(rejected)?
            .to_vec())
    }

    pub fn log_medication(
        &self,
        family: FamilyRef,
        child_id: Vec<u8>,
        input: MedicationInput,
        time: ActivityWhen,
    ) -> Result<Vec<u8>, BindingError> {
        Ok(self
            .repo
            .lock()
            .map_err(|_| BindingError::LockPoisoned)?
            .log_medication(
                family.handle()?,
                fixed(&child_id)?,
                &input.name,
                &input.dose_amount,
                &input.dose_unit,
                time.into(),
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
                end_utc_ms: row.end_utc_ms,
                note: row.note,
                diaper_kind: row.diaper_kind,
                bottle_ml: row.bottle_ml,
                breast_side: row.breast_side,
                breast_segments: row
                    .breast_segments
                    .map(|segments| segments.into_iter().map(breast_segment_row).collect()),
                solids_foods: row.solids_foods,
                solids_amount: row.solids_amount,
                pump_left_ml: row.pump_left_ml,
                pump_right_ml: row.pump_right_ml,
                pump_total_ml: row.pump_total_ml,
                growth_weight_g: row.growth_weight_g,
                growth_length_mm: row.growth_length_mm,
                temperature_c: row.temperature_c,
                medication_name: row.medication_name,
                medication_dose_amount: row.medication_dose_amount,
                medication_dose_unit: row.medication_dose_unit,
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

    pub fn analysis_csv(&self, family: FamilyRef) -> Result<Vec<u8>, BindingError> {
        self.repo
            .lock()
            .map_err(|_| BindingError::LockPoisoned)?
            .analysis_csv(family.handle()?)
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

fn lower_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn committed_control(response: &[u8]) -> Result<Vec<u8>, BindingError> {
    let Value::Map(fields) = cbor::decode_with_limits(
        response,
        cbor::Limits {
            max_bytes: 1024 * 1024 + 128,
            max_depth: 16,
        },
    )
    .map_err(rejected)?
    else {
        return Err(BindingError::InvalidBytes);
    };
    if fields.len() != 2 || fields[0] != (1, Value::Integer(1)) || fields[1].0 != 2 {
        return Err(BindingError::InvalidBytes);
    }
    let Value::Bytes(committed) = &fields[1].1 else {
        return Err(BindingError::InvalidBytes);
    };
    Ok(committed.clone())
}

fn rejected(error: impl std::fmt::Debug) -> BindingError {
    BindingError::Rejected(format!("{error:?}"))
}

#[uniffi::export]
pub fn validate_relay_origin(origin: String) -> Result<(), BindingError> {
    babytrack_core::bootstrap::validate_relay_origin(&origin).map_err(rejected)
}
