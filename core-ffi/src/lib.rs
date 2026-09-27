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
    issue::FirstInviteIssue,
    local_api::{self, ActivityTime, LocalRepository},
    shared_history::{self, PublicHistorySession},
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
pub struct RecipientSyncRow {
    pub verified_cursor: u64,
    pub pending_control_cursor: u64,
    pub awaiting_grant: bool,
    pub no_more_visible: bool,
    pub remaining_objects: bool,
    pub ready: bool,
    pub child_count: u64,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct SharedSnapshotRow {
    pub family: FamilyRef,
    pub verified_cursor: u64,
    pub children: Vec<ChildRow>,
    pub activities: Vec<ActivityRow>,
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
        let handle = family.handle()?;
        let wrapping = fixed(&wrapping_key)?;
        let ready = if store
            .has_enrollment_attempt(handle.family_id)
            .map_err(rejected)?
        {
            let attempt = EnrollmentAttempt::resume(&mut store, handle.family_id, &wrapping)
                .map_err(rejected)?;
            if attempt.family() != handle {
                return Err(BindingError::InvalidBytes);
            }
            ReadyFamilySession::from_enrollment(&store, &attempt).map_err(rejected)?
        } else {
            ManagerCreation::resume(&store, handle, &wrapping)
                .map_err(rejected)?
                .ready_session(&store)
                .map_err(rejected)?
        };
        let projection = ready.projection_with_pending(&store).map_err(rejected)?;
        let children = local_api::children_from_records(projection.records())
            .into_iter()
            .map(|child| ChildRow {
                id: child.id.to_vec(),
                name: child.name,
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
                diaper_kind: row.diaper_kind,
                bottle_ml: row.bottle_ml,
            })
            .collect();
        Ok(SharedSnapshotRow {
            family,
            verified_cursor: ready.observed_cursor(),
            children,
            activities,
        })
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
