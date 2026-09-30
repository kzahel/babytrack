//! Native boundary records and value conversions.

use super::*;

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
    pub(crate) fn handle(&self) -> Result<FamilyHandle, BindingError> {
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
pub struct DayWindowRow {
    pub start_utc_ms: i64,
    pub end_utc_ms: i64,
    pub through_utc_ms: i64,
}

impl From<DayWindowRow> for local_api::DayWindow {
    fn from(value: DayWindowRow) -> Self {
        Self {
            start_utc_ms: value.start_utc_ms,
            end_utc_ms: value.end_utc_ms,
            through_utc_ms: value.through_utc_ms,
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct DaySummaryRow {
    pub sleep_ms: u64,
    pub feed_count: u64,
    pub bottle_ml: u64,
    pub diaper_count: u64,
    pub wet_diaper_count: u64,
    pub dirty_diaper_count: u64,
}

impl From<local_api::DaySummary> for DaySummaryRow {
    fn from(value: local_api::DaySummary) -> Self {
        Self {
            sleep_ms: value.sleep_ms,
            feed_count: value.feed_count,
            bottle_ml: value.bottle_ml,
            diaper_count: value.diaper_count,
            wet_diaper_count: value.wet_diaper_count,
            dirty_diaper_count: value.dirty_diaper_count,
        }
    }
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
pub struct EnteredMeasureRow {
    pub entered: String,
    pub unit: u8,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct GrowthInputRow {
    pub weight: Option<EnteredMeasureRow>,
    pub length: Option<EnteredMeasureRow>,
    pub head: Option<EnteredMeasureRow>,
}

impl From<GrowthInputRow> for local_api::GrowthInput {
    fn from(value: GrowthInputRow) -> Self {
        let into_measure = |measure: EnteredMeasureRow| local_api::MeasurementInput {
            entered: measure.entered,
            unit: measure.unit,
        };
        Self {
            weight: value.weight.map(into_measure),
            length: value.length.map(into_measure),
            head: value.head.map(into_measure),
        }
    }
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

pub(crate) fn breast_segment_row(value: local_api::BreastSegment) -> BreastSegmentRow {
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
    pub sleep_place: Option<u8>,
    pub note: Option<String>,
    pub diaper_kind: Option<u8>,
    pub bottle_ml: Option<i64>,
    pub bottle_entered: Option<String>,
    pub bottle_unit: Option<u8>,
    pub bottle_content: Option<u8>,
    pub breast_side: Option<u8>,
    pub breast_segments: Option<Vec<BreastSegmentRow>>,
    pub solids_foods: Option<Vec<String>>,
    pub solids_amount: Option<String>,
    pub pump_left_ml: Option<i64>,
    pub pump_right_ml: Option<i64>,
    pub pump_total_ml: Option<i64>,
    pub growth_weight_g: Option<i64>,
    pub growth_weight_entered: Option<String>,
    pub growth_weight_unit: Option<u8>,
    pub growth_length_mm: Option<i64>,
    pub growth_length_entered: Option<String>,
    pub growth_length_unit: Option<u8>,
    pub growth_head_mm: Option<i64>,
    pub growth_head_entered: Option<String>,
    pub growth_head_unit: Option<u8>,
    pub temperature_c: Option<String>,
    pub temperature_entered: Option<String>,
    pub temperature_unit: Option<u8>,
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
pub struct PreparedCancelRow {
    pub invitation_id: Vec<u8>,
    pub candidate_bytes: Vec<u8>,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct PendingDeviceRow {
    pub invitation_id: Vec<u8>,
    pub device_id: Vec<u8>,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct PreparedPendingRemovalRow {
    pub invitation_id: Vec<u8>,
    pub device_id: Vec<u8>,
    pub candidate_bytes: Vec<u8>,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct PreparedRoleChangeRow {
    pub target_device_id: Vec<u8>,
    pub new_role: u8,
    pub candidate_bytes: Vec<u8>,
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
pub struct ControlPageProgressRow {
    pub next_after: u64,
    pub has_more: bool,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct InvitationStatusRow {
    pub reason: u8,
    pub cursor: u64,
    pub observed_ms: i64,
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
    /// 1 claim, 2 holder challenge, 3 local proof, 4 holder grant,
    /// 5 hydration, 6 ready, 7 removed.
    pub join_phase: u8,
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
    pub pending_devices: Vec<PendingDeviceRow>,
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

impl From<ActivityWhen> for ActivityTime {
    fn from(value: ActivityWhen) -> Self {
        Self {
            start_utc_ms: value.start_utc_ms,
            offset_minutes: value.offset_minutes,
            saved_at_ms: value.saved_at_ms,
        }
    }
}
