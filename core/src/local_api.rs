//! Offline local tracking API used by native platform bindings. Every action
//! names its Family and child; UI selection never changes a saved target.

use std::path::Path;

use crate::{
    cbor::Value,
    ids,
    operation::{Hlc, Kind, NewOperation, Scope},
    portable_file::{self},
    projection::Record,
    sqlite_store::{self, FamilyHandle, SqliteStore},
};

#[derive(Debug)]
pub enum Error {
    Store(sqlite_store::Error),
    Backup(portable_file::Error),
    Random(getrandom::Error),
    Invalid(&'static str),
}
impl From<sqlite_store::Error> for Error {
    fn from(value: sqlite_store::Error) -> Self {
        Self::Store(value)
    }
}
impl From<portable_file::Error> for Error {
    fn from(value: portable_file::Error) -> Self {
        Self::Backup(value)
    }
}
impl From<getrandom::Error> for Error {
    fn from(value: getrandom::Error) -> Self {
        Self::Random(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Child {
    pub id: [u8; 16],
    pub name: String,
    pub birth_day: Option<i64>,
    pub sex: Option<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Activity {
    pub id: [u8; 16],
    pub child_id: [u8; 16],
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
    pub breast_segments: Option<Vec<BreastSegment>>,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MeasurementInput {
    pub entered: String,
    pub unit: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrowthInput {
    pub weight: Option<MeasurementInput>,
    pub length: Option<MeasurementInput>,
    pub head: Option<MeasurementInput>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActivityTime {
    pub start_utc_ms: i64,
    pub offset_minutes: i16,
    pub saved_at_ms: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PumpAmounts {
    pub left_ml: Option<i64>,
    pub right_ml: Option<i64>,
    pub total_ml: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BreastSegment {
    pub side: u8,
    pub start_utc_ms: i64,
    pub end_utc_ms: i64,
    pub start_offset_minutes: i16,
    pub end_offset_minutes: i16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackupInfo {
    pub source_family_id: [u8; 16],
    pub snapshot_utc_ms: i64,
    pub known_gap: bool,
    pub record_count: u64,
}

pub struct BackupFile {
    pub bytes: Vec<u8>,
    pub revision: u64,
    pub info: BackupInfo,
}

pub struct LocalRepository {
    store: SqliteStore,
}

impl LocalRepository {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, Error> {
        Ok(Self {
            store: SqliteStore::open(path)?,
        })
    }

    pub fn families(&self) -> Result<Vec<FamilyHandle>, Error> {
        self.store
            .families()?
            .into_iter()
            .filter_map(
                |family| match self.store.has_enrollment_attempt(family.family_id) {
                    Ok(false) => Some(Ok(family)),
                    Ok(true) => None,
                    Err(error) => Some(Err(error.into())),
                },
            )
            .collect()
    }

    fn ensure_local_surface(&self, family: FamilyHandle) -> Result<(), Error> {
        if self.store.has_enrollment_attempt(family.family_id)? {
            return Err(Error::Invalid(
                "recipient enrollment is not a local-only Family",
            ));
        }
        if self.store.shared_history(family)?.is_some() {
            return Err(Error::Invalid("shared Family requires verified session"));
        }
        Ok(())
    }

    pub fn create_family(&mut self, now_ms: i64) -> Result<FamilyHandle, Error> {
        check_time(now_ms)?;
        let family_id = ids::random_v4()?;
        let device_id = ids::random_v4()?;
        let operation_id = ids::random_v7(now_ms)?;
        Ok(self.store.create_local_family_with_metadata(
            family_id,
            device_id,
            operation_id,
            now_ms,
        )?)
    }

    pub fn children(&self, family: FamilyHandle) -> Result<Vec<Child>, Error> {
        self.ensure_local_surface(family)?;
        let projection = self.store.load_local(family)?;
        Ok(children_from_records(projection.records()))
    }

    pub fn add_child(
        &mut self,
        family: FamilyHandle,
        name: &str,
        now_ms: i64,
    ) -> Result<[u8; 16], Error> {
        self.add_child_with_metadata(family, name, None, None, now_ms)
    }

    pub fn add_child_with_metadata(
        &mut self,
        family: FamilyHandle,
        name: &str,
        birth_day: Option<i64>,
        sex: Option<u8>,
        now_ms: i64,
    ) -> Result<[u8; 16], Error> {
        self.ensure_local_surface(family)?;
        let (child_id, operation) =
            child_operation_with_metadata(family, name, birth_day, sex, now_ms)?;
        self.store.append_local(family, operation, now_ms)?;
        Ok(child_id)
    }

    pub fn rename_child(
        &mut self,
        family: FamilyHandle,
        child_id: [u8; 16],
        name: &str,
        saved_at_ms: i64,
    ) -> Result<(), Error> {
        self.ensure_local_surface(family)?;
        let projection = self.store.load_local(family)?;
        let child = projection
            .record(&child_id)
            .ok_or(Error::Invalid("target child unavailable"))?;
        let operation = rename_child_operation(family, child, name, saved_at_ms)?;
        self.store.append_local(family, operation, saved_at_ms)?;
        Ok(())
    }

    pub fn edit_child_metadata(
        &mut self,
        family: FamilyHandle,
        child_id: [u8; 16],
        birth_day: Option<i64>,
        sex: Option<u8>,
        saved_at_ms: i64,
    ) -> Result<(), Error> {
        self.ensure_local_surface(family)?;
        let projection = self.store.load_local(family)?;
        let child = projection
            .record(&child_id)
            .ok_or(Error::Invalid("target child unavailable"))?;
        let operation = edit_child_metadata_operation(family, child, birth_day, sex, saved_at_ms)?;
        self.store.append_local(family, operation, saved_at_ms)?;
        Ok(())
    }

    pub fn log_diaper(
        &mut self,
        family: FamilyHandle,
        child_id: [u8; 16],
        diaper_kind: u8,
        time: ActivityTime,
    ) -> Result<[u8; 16], Error> {
        let (activity_id, operation) = diaper_operation(family, child_id, diaper_kind, time)?;
        self.append_activity(family, child_id, operation, time.saved_at_ms)?;
        Ok(activity_id)
    }

    pub fn log_bottle_ml(
        &mut self,
        family: FamilyHandle,
        child_id: [u8; 16],
        amount_ml: i64,
        content: u8,
        time: ActivityTime,
    ) -> Result<[u8; 16], Error> {
        let (activity_id, operation) =
            bottle_operation(family, child_id, amount_ml, content, time)?;
        self.append_activity(family, child_id, operation, time.saved_at_ms)?;
        Ok(activity_id)
    }

    pub fn log_bottle_entered(
        &mut self,
        family: FamilyHandle,
        child_id: [u8; 16],
        entered: &str,
        unit: u8,
        content: u8,
        time: ActivityTime,
    ) -> Result<[u8; 16], Error> {
        let (activity_id, operation) =
            bottle_entered_operation(family, child_id, entered, unit, content, time)?;
        self.append_activity(family, child_id, operation, time.saved_at_ms)?;
        Ok(activity_id)
    }

    pub fn log_breast_feed(
        &mut self,
        family: FamilyHandle,
        child_id: [u8; 16],
        side: u8,
        time: ActivityTime,
        end_utc_ms: i64,
    ) -> Result<[u8; 16], Error> {
        let (activity_id, operation) =
            breast_feed_operation(family, child_id, side, time, end_utc_ms)?;
        self.append_activity(family, child_id, operation, time.saved_at_ms)?;
        Ok(activity_id)
    }

    pub fn log_breast_feed_segments(
        &mut self,
        family: FamilyHandle,
        child_id: [u8; 16],
        segments: Vec<BreastSegment>,
        time: ActivityTime,
    ) -> Result<[u8; 16], Error> {
        let (activity_id, operation) =
            breast_feed_segments_operation(family, child_id, &segments, time)?;
        self.append_activity(family, child_id, operation, time.saved_at_ms)?;
        Ok(activity_id)
    }

    pub fn edit_breast_feed_segments(
        &mut self,
        family: FamilyHandle,
        child_id: [u8; 16],
        activity_id: [u8; 16],
        segments: Vec<BreastSegment>,
        saved_at_ms: i64,
    ) -> Result<(), Error> {
        self.ensure_local_surface(family)?;
        let projection = self.store.load_local(family)?;
        let child = projection
            .record(&child_id)
            .ok_or(Error::Invalid("target child unavailable"))?;
        if child.scope != Scope::Child || child.deleted {
            return Err(Error::Invalid("target child unavailable"));
        }
        let activity = projection
            .record(&activity_id)
            .ok_or(Error::Invalid("activity unavailable"))?;
        let operation = edit_breast_feed_segments_operation(
            family,
            child_id,
            activity,
            &segments,
            saved_at_ms,
        )?;
        self.store.append_local(family, operation, saved_at_ms)?;
        Ok(())
    }

    pub fn log_pump(
        &mut self,
        family: FamilyHandle,
        child_id: [u8; 16],
        amounts: PumpAmounts,
        time: ActivityTime,
        end_utc_ms: i64,
    ) -> Result<[u8; 16], Error> {
        let (activity_id, operation) = pump_operation(family, child_id, amounts, time, end_utc_ms)?;
        self.append_activity(family, child_id, operation, time.saved_at_ms)?;
        Ok(activity_id)
    }

    pub fn log_solids(
        &mut self,
        family: FamilyHandle,
        child_id: [u8; 16],
        foods: &[String],
        amount: &str,
        time: ActivityTime,
    ) -> Result<[u8; 16], Error> {
        let (activity_id, operation) = solids_operation(family, child_id, foods, amount, time)?;
        self.append_activity(family, child_id, operation, time.saved_at_ms)?;
        Ok(activity_id)
    }

    pub fn log_sleep(
        &mut self,
        family: FamilyHandle,
        child_id: [u8; 16],
        time: ActivityTime,
        end_utc_ms: i64,
        end_offset_minutes: i16,
    ) -> Result<[u8; 16], Error> {
        self.log_sleep_with_place(family, child_id, time, end_utc_ms, end_offset_minutes, None)
    }

    pub fn log_sleep_with_place(
        &mut self,
        family: FamilyHandle,
        child_id: [u8; 16],
        time: ActivityTime,
        end_utc_ms: i64,
        end_offset_minutes: i16,
        place: Option<u8>,
    ) -> Result<[u8; 16], Error> {
        let (activity_id, operation) = sleep_operation_with_place(
            family,
            child_id,
            time,
            end_utc_ms,
            end_offset_minutes,
            place,
        )?;
        self.append_activity(family, child_id, operation, time.saved_at_ms)?;
        Ok(activity_id)
    }

    pub fn start_sleep(
        &mut self,
        family: FamilyHandle,
        child_id: [u8; 16],
        time: ActivityTime,
    ) -> Result<[u8; 16], Error> {
        self.start_sleep_with_place(family, child_id, time, None)
    }

    pub fn start_sleep_with_place(
        &mut self,
        family: FamilyHandle,
        child_id: [u8; 16],
        time: ActivityTime,
        place: Option<u8>,
    ) -> Result<[u8; 16], Error> {
        let (activity_id, operation) =
            running_sleep_operation_with_place(family, child_id, time, place)?;
        self.append_activity(family, child_id, operation, time.saved_at_ms)?;
        Ok(activity_id)
    }

    pub fn stop_sleep(
        &mut self,
        family: FamilyHandle,
        child_id: [u8; 16],
        activity_id: [u8; 16],
        end_utc_ms: i64,
        end_offset_minutes: i16,
        saved_at_ms: i64,
    ) -> Result<(), Error> {
        self.ensure_local_surface(family)?;
        let projection = self.store.load_local(family)?;
        let activity = projection
            .record(&activity_id)
            .ok_or(Error::Invalid("sleep activity unavailable"))?;
        let operation = stop_sleep_operation(
            family,
            child_id,
            activity,
            end_utc_ms,
            end_offset_minutes,
            saved_at_ms,
        )?;
        self.store.append_local(family, operation, saved_at_ms)?;
        Ok(())
    }

    pub fn edit_sleep_end(
        &mut self,
        family: FamilyHandle,
        child_id: [u8; 16],
        activity_id: [u8; 16],
        end_utc_ms: i64,
        end_offset_minutes: i16,
        saved_at_ms: i64,
    ) -> Result<(), Error> {
        self.ensure_local_surface(family)?;
        let projection = self.store.load_local(family)?;
        let activity = projection
            .record(&activity_id)
            .ok_or(Error::Invalid("sleep activity unavailable"))?;
        let operation = edit_sleep_end_operation(
            family,
            child_id,
            activity,
            end_utc_ms,
            end_offset_minutes,
            saved_at_ms,
        )?;
        self.store.append_local(family, operation, saved_at_ms)?;
        Ok(())
    }

    pub fn edit_sleep_place(
        &mut self,
        family: FamilyHandle,
        child_id: [u8; 16],
        activity_id: [u8; 16],
        place: Option<u8>,
        saved_at_ms: i64,
    ) -> Result<(), Error> {
        self.ensure_local_surface(family)?;
        let projection = self.store.load_local(family)?;
        let activity = projection
            .record(&activity_id)
            .ok_or(Error::Invalid("sleep activity unavailable"))?;
        let operation = edit_sleep_place_operation(family, child_id, activity, place, saved_at_ms)?;
        self.store.append_local(family, operation, saved_at_ms)?;
        Ok(())
    }

    pub fn delete_activity(
        &mut self,
        family: FamilyHandle,
        child_id: [u8; 16],
        activity_id: [u8; 16],
        saved_at_ms: i64,
    ) -> Result<(), Error> {
        self.ensure_local_surface(family)?;
        let projection = self.store.load_local(family)?;
        let child = projection
            .record(&child_id)
            .ok_or(Error::Invalid("target child unavailable"))?;
        if child.scope != Scope::Child || child.deleted {
            return Err(Error::Invalid("target child unavailable"));
        }
        let activity = projection
            .record(&activity_id)
            .ok_or(Error::Invalid("activity unavailable"))?;
        let operation = delete_activity_operation(family, child_id, activity, saved_at_ms)?;
        self.store.append_local(family, operation, saved_at_ms)?;
        Ok(())
    }

    pub fn edit_note(
        &mut self,
        family: FamilyHandle,
        child_id: [u8; 16],
        activity_id: [u8; 16],
        note: &str,
        saved_at_ms: i64,
    ) -> Result<(), Error> {
        self.ensure_local_surface(family)?;
        let projection = self.store.load_local(family)?;
        let child = projection
            .record(&child_id)
            .ok_or(Error::Invalid("target child unavailable"))?;
        if child.scope != Scope::Child || child.deleted {
            return Err(Error::Invalid("target child unavailable"));
        }
        let activity = projection
            .record(&activity_id)
            .ok_or(Error::Invalid("activity unavailable"))?;
        let operation = edit_note_operation(family, child_id, activity, note, saved_at_ms)?;
        self.store.append_local(family, operation, saved_at_ms)?;
        Ok(())
    }

    pub fn edit_bottle_ml(
        &mut self,
        family: FamilyHandle,
        child_id: [u8; 16],
        activity_id: [u8; 16],
        amount_ml: i64,
        saved_at_ms: i64,
    ) -> Result<(), Error> {
        self.ensure_local_surface(family)?;
        let projection = self.store.load_local(family)?;
        let child = projection
            .record(&child_id)
            .ok_or(Error::Invalid("target child unavailable"))?;
        if child.scope != Scope::Child || child.deleted {
            return Err(Error::Invalid("target child unavailable"));
        }
        let activity = projection
            .record(&activity_id)
            .ok_or(Error::Invalid("activity unavailable"))?;
        let operation =
            edit_bottle_ml_operation(family, child_id, activity, amount_ml, saved_at_ms)?;
        self.store.append_local(family, operation, saved_at_ms)?;
        Ok(())
    }

    pub fn edit_bottle(
        &mut self,
        family: FamilyHandle,
        child_id: [u8; 16],
        activity_id: [u8; 16],
        amount_ml: i64,
        content: u8,
        saved_at_ms: i64,
    ) -> Result<(), Error> {
        self.ensure_local_surface(family)?;
        let projection = self.store.load_local(family)?;
        let child = projection
            .record(&child_id)
            .ok_or(Error::Invalid("target child unavailable"))?;
        if child.scope != Scope::Child || child.deleted {
            return Err(Error::Invalid("target child unavailable"));
        }
        let activity = projection
            .record(&activity_id)
            .ok_or(Error::Invalid("activity unavailable"))?;
        let operation =
            edit_bottle_operation(family, child_id, activity, amount_ml, content, saved_at_ms)?;
        self.store.append_local(family, operation, saved_at_ms)?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn edit_bottle_entered(
        &mut self,
        family: FamilyHandle,
        child_id: [u8; 16],
        activity_id: [u8; 16],
        entered: &str,
        unit: u8,
        content: u8,
        saved_at_ms: i64,
    ) -> Result<(), Error> {
        self.ensure_local_surface(family)?;
        let projection = self.store.load_local(family)?;
        let child = projection
            .record(&child_id)
            .ok_or(Error::Invalid("target child unavailable"))?;
        if child.scope != Scope::Child || child.deleted {
            return Err(Error::Invalid("target child unavailable"));
        }
        let activity = projection
            .record(&activity_id)
            .ok_or(Error::Invalid("activity unavailable"))?;
        let operation = edit_bottle_entered_operation(
            family,
            child_id,
            activity,
            entered,
            unit,
            content,
            saved_at_ms,
        )?;
        self.store.append_local(family, operation, saved_at_ms)?;
        Ok(())
    }

    pub fn edit_diaper_kind(
        &mut self,
        family: FamilyHandle,
        child_id: [u8; 16],
        activity_id: [u8; 16],
        kind: u8,
        saved_at_ms: i64,
    ) -> Result<(), Error> {
        self.ensure_local_surface(family)?;
        let projection = self.store.load_local(family)?;
        let child = projection
            .record(&child_id)
            .ok_or(Error::Invalid("target child unavailable"))?;
        if child.scope != Scope::Child || child.deleted {
            return Err(Error::Invalid("target child unavailable"));
        }
        let activity = projection
            .record(&activity_id)
            .ok_or(Error::Invalid("activity unavailable"))?;
        let operation = edit_diaper_kind_operation(family, child_id, activity, kind, saved_at_ms)?;
        self.store.append_local(family, operation, saved_at_ms)?;
        Ok(())
    }

    pub fn edit_solids(
        &mut self,
        family: FamilyHandle,
        child_id: [u8; 16],
        activity_id: [u8; 16],
        foods: &[String],
        amount: &str,
        saved_at_ms: i64,
    ) -> Result<(), Error> {
        self.ensure_local_surface(family)?;
        let projection = self.store.load_local(family)?;
        let child = projection
            .record(&child_id)
            .ok_or(Error::Invalid("target child unavailable"))?;
        if child.scope != Scope::Child || child.deleted {
            return Err(Error::Invalid("target child unavailable"));
        }
        let activity = projection
            .record(&activity_id)
            .ok_or(Error::Invalid("activity unavailable"))?;
        let operation =
            edit_solids_operation(family, child_id, activity, foods, amount, saved_at_ms)?;
        self.store.append_local(family, operation, saved_at_ms)?;
        Ok(())
    }

    pub fn edit_growth(
        &mut self,
        family: FamilyHandle,
        child_id: [u8; 16],
        activity_id: [u8; 16],
        weight_g: Option<i64>,
        length_mm: Option<i64>,
        saved_at_ms: i64,
    ) -> Result<(), Error> {
        self.ensure_local_surface(family)?;
        let projection = self.store.load_local(family)?;
        let child = projection
            .record(&child_id)
            .ok_or(Error::Invalid("target child unavailable"))?;
        if child.scope != Scope::Child || child.deleted {
            return Err(Error::Invalid("target child unavailable"));
        }
        let activity = projection
            .record(&activity_id)
            .ok_or(Error::Invalid("activity unavailable"))?;
        let operation =
            edit_growth_operation(family, child_id, activity, weight_g, length_mm, saved_at_ms)?;
        self.store.append_local(family, operation, saved_at_ms)?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn edit_growth_measurements(
        &mut self,
        family: FamilyHandle,
        child_id: [u8; 16],
        activity_id: [u8; 16],
        weight_g: Option<i64>,
        length_mm: Option<i64>,
        head_mm: Option<i64>,
        saved_at_ms: i64,
    ) -> Result<(), Error> {
        self.ensure_local_surface(family)?;
        let projection = self.store.load_local(family)?;
        let child = projection
            .record(&child_id)
            .ok_or(Error::Invalid("target child unavailable"))?;
        if child.scope != Scope::Child || child.deleted {
            return Err(Error::Invalid("target child unavailable"));
        }
        let activity = projection
            .record(&activity_id)
            .ok_or(Error::Invalid("activity unavailable"))?;
        let operation = edit_growth_measurements_operation(
            family,
            child_id,
            activity,
            weight_g,
            length_mm,
            head_mm,
            saved_at_ms,
        )?;
        self.store.append_local(family, operation, saved_at_ms)?;
        Ok(())
    }

    pub fn edit_growth_entered(
        &mut self,
        family: FamilyHandle,
        child_id: [u8; 16],
        activity_id: [u8; 16],
        input: &GrowthInput,
        saved_at_ms: i64,
    ) -> Result<(), Error> {
        self.ensure_local_surface(family)?;
        let projection = self.store.load_local(family)?;
        let child = projection
            .record(&child_id)
            .ok_or(Error::Invalid("target child unavailable"))?;
        if child.scope != Scope::Child || child.deleted {
            return Err(Error::Invalid("target child unavailable"));
        }
        let activity = projection
            .record(&activity_id)
            .ok_or(Error::Invalid("activity unavailable"))?;
        let operation =
            edit_growth_entered_operation(family, child_id, activity, input, saved_at_ms)?;
        self.store.append_local(family, operation, saved_at_ms)?;
        Ok(())
    }

    pub fn edit_pump_amounts(
        &mut self,
        family: FamilyHandle,
        child_id: [u8; 16],
        activity_id: [u8; 16],
        amounts: PumpAmounts,
        saved_at_ms: i64,
    ) -> Result<(), Error> {
        self.ensure_local_surface(family)?;
        let projection = self.store.load_local(family)?;
        let child = projection
            .record(&child_id)
            .ok_or(Error::Invalid("target child unavailable"))?;
        if child.scope != Scope::Child || child.deleted {
            return Err(Error::Invalid("target child unavailable"));
        }
        let activity = projection
            .record(&activity_id)
            .ok_or(Error::Invalid("activity unavailable"))?;
        let operation =
            edit_pump_amounts_operation(family, child_id, activity, amounts, saved_at_ms)?;
        self.store.append_local(family, operation, saved_at_ms)?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn edit_medication(
        &mut self,
        family: FamilyHandle,
        child_id: [u8; 16],
        activity_id: [u8; 16],
        name: &str,
        dose_amount: &str,
        dose_unit: &str,
        saved_at_ms: i64,
    ) -> Result<(), Error> {
        self.ensure_local_surface(family)?;
        let projection = self.store.load_local(family)?;
        let child = projection
            .record(&child_id)
            .ok_or(Error::Invalid("target child unavailable"))?;
        if child.scope != Scope::Child || child.deleted {
            return Err(Error::Invalid("target child unavailable"));
        }
        let activity = projection
            .record(&activity_id)
            .ok_or(Error::Invalid("activity unavailable"))?;
        let operation = edit_medication_operation(
            family,
            child_id,
            activity,
            name,
            dose_amount,
            dose_unit,
            saved_at_ms,
        )?;
        self.store.append_local(family, operation, saved_at_ms)?;
        Ok(())
    }

    pub fn edit_temperature_c(
        &mut self,
        family: FamilyHandle,
        child_id: [u8; 16],
        activity_id: [u8; 16],
        entered_c: &str,
        saved_at_ms: i64,
    ) -> Result<(), Error> {
        self.ensure_local_surface(family)?;
        let projection = self.store.load_local(family)?;
        let child = projection
            .record(&child_id)
            .ok_or(Error::Invalid("target child unavailable"))?;
        if child.scope != Scope::Child || child.deleted {
            return Err(Error::Invalid("target child unavailable"));
        }
        let activity = projection
            .record(&activity_id)
            .ok_or(Error::Invalid("activity unavailable"))?;
        let operation =
            edit_temperature_c_operation(family, child_id, activity, entered_c, saved_at_ms)?;
        self.store.append_local(family, operation, saved_at_ms)?;
        Ok(())
    }

    pub fn edit_temperature_entered(
        &mut self,
        family: FamilyHandle,
        child_id: [u8; 16],
        activity_id: [u8; 16],
        entered: &str,
        unit: u8,
        saved_at_ms: i64,
    ) -> Result<(), Error> {
        self.ensure_local_surface(family)?;
        let projection = self.store.load_local(family)?;
        let child = projection
            .record(&child_id)
            .ok_or(Error::Invalid("target child unavailable"))?;
        if child.scope != Scope::Child || child.deleted {
            return Err(Error::Invalid("target child unavailable"));
        }
        let activity = projection
            .record(&activity_id)
            .ok_or(Error::Invalid("activity unavailable"))?;
        let operation = edit_temperature_entered_operation(
            family,
            child_id,
            activity,
            entered,
            unit,
            saved_at_ms,
        )?;
        self.store.append_local(family, operation, saved_at_ms)?;
        Ok(())
    }

    pub fn log_note(
        &mut self,
        family: FamilyHandle,
        child_id: [u8; 16],
        note: &str,
        time: ActivityTime,
    ) -> Result<[u8; 16], Error> {
        let (activity_id, operation) = note_operation(family, child_id, note, time)?;
        self.append_activity(family, child_id, operation, time.saved_at_ms)?;
        Ok(activity_id)
    }

    pub fn log_growth(
        &mut self,
        family: FamilyHandle,
        child_id: [u8; 16],
        weight_g: Option<i64>,
        length_mm: Option<i64>,
        time: ActivityTime,
    ) -> Result<[u8; 16], Error> {
        let (activity_id, operation) =
            growth_operation(family, child_id, weight_g, length_mm, time)?;
        self.append_activity(family, child_id, operation, time.saved_at_ms)?;
        Ok(activity_id)
    }

    pub fn log_growth_measurements(
        &mut self,
        family: FamilyHandle,
        child_id: [u8; 16],
        weight_g: Option<i64>,
        length_mm: Option<i64>,
        head_mm: Option<i64>,
        time: ActivityTime,
    ) -> Result<[u8; 16], Error> {
        let (activity_id, operation) =
            growth_measurements_operation(family, child_id, weight_g, length_mm, head_mm, time)?;
        self.append_activity(family, child_id, operation, time.saved_at_ms)?;
        Ok(activity_id)
    }

    pub fn log_growth_entered(
        &mut self,
        family: FamilyHandle,
        child_id: [u8; 16],
        input: &GrowthInput,
        time: ActivityTime,
    ) -> Result<[u8; 16], Error> {
        let (activity_id, operation) = growth_entered_operation(family, child_id, input, time)?;
        self.append_activity(family, child_id, operation, time.saved_at_ms)?;
        Ok(activity_id)
    }

    pub fn log_temperature_c(
        &mut self,
        family: FamilyHandle,
        child_id: [u8; 16],
        entered_c: &str,
        time: ActivityTime,
    ) -> Result<[u8; 16], Error> {
        let (activity_id, operation) = temperature_c_operation(family, child_id, entered_c, time)?;
        self.append_activity(family, child_id, operation, time.saved_at_ms)?;
        Ok(activity_id)
    }

    pub fn log_temperature_entered(
        &mut self,
        family: FamilyHandle,
        child_id: [u8; 16],
        entered: &str,
        unit: u8,
        time: ActivityTime,
    ) -> Result<[u8; 16], Error> {
        let (activity_id, operation) =
            temperature_entered_operation(family, child_id, entered, unit, time)?;
        self.append_activity(family, child_id, operation, time.saved_at_ms)?;
        Ok(activity_id)
    }

    pub fn log_medication(
        &mut self,
        family: FamilyHandle,
        child_id: [u8; 16],
        name: &str,
        dose_amount: &str,
        dose_unit: &str,
        time: ActivityTime,
    ) -> Result<[u8; 16], Error> {
        let (activity_id, operation) =
            medication_operation(family, child_id, name, dose_amount, dose_unit, time)?;
        self.append_activity(family, child_id, operation, time.saved_at_ms)?;
        Ok(activity_id)
    }

    fn append_activity(
        &mut self,
        family: FamilyHandle,
        child_id: [u8; 16],
        operation: NewOperation,
        saved_at_ms: i64,
    ) -> Result<(), Error> {
        self.ensure_local_surface(family)?;
        let projection = self.store.load_local(family)?;
        if projection
            .record(&child_id)
            .is_none_or(|record| record.scope != Scope::Child || record.deleted)
        {
            return Err(Error::Invalid("target child is unavailable"));
        }
        self.store.append_local(family, operation, saved_at_ms)?;
        Ok(())
    }

    pub fn timeline(
        &self,
        family: FamilyHandle,
        child_id: [u8; 16],
    ) -> Result<Vec<Activity>, Error> {
        self.ensure_local_surface(family)?;
        let projection = self.store.load_local(family)?;
        if projection
            .record(&child_id)
            .is_none_or(|record| record.scope != Scope::Child || record.deleted)
        {
            return Err(Error::Invalid("target child is unavailable"));
        }
        Ok(activities_from_records(projection.records())
            .into_iter()
            .filter(|activity| activity.child_id == child_id)
            .collect())
    }

    pub fn backup(&self, family: FamilyHandle, now_ms: i64) -> Result<Vec<u8>, Error> {
        self.ensure_local_surface(family)?;
        Ok(portable_file::export_readable_local(
            &self.store,
            family,
            now_ms,
        )?)
    }

    pub fn analysis_csv(&self, family: FamilyHandle) -> Result<Vec<u8>, Error> {
        self.ensure_local_surface(family)?;
        let projection = self.store.load_local(family)?;
        Ok(crate::analysis_csv::export(
            family.family_id,
            projection.records(),
        ))
    }

    pub fn revision(&self, family: FamilyHandle) -> Result<u64, Error> {
        self.ensure_local_surface(family)?;
        Ok(self.store.local_revision(family)?)
    }

    pub fn restored_origin(
        &self,
        family: FamilyHandle,
    ) -> Result<Option<sqlite_store::RestoredOrigin>, Error> {
        self.ensure_local_surface(family)?;
        Ok(self.store.restored_origin(family)?)
    }

    pub fn backup_file(
        &self,
        family: FamilyHandle,
        now_ms: i64,
        password: Option<&str>,
        available_memory_bytes: u64,
    ) -> Result<BackupFile, Error> {
        let revision = self.revision(family)?;
        let readable = self.backup(family, now_ms)?;
        let info = Self::inspect_readable(&readable)?;
        let bytes = match password {
            Some(password) => {
                portable_file::protect_readable(&readable, password, available_memory_bytes)?
            }
            None => readable,
        };
        Ok(BackupFile {
            bytes,
            revision,
            info,
        })
    }

    pub fn inspect_readable(bytes: &[u8]) -> Result<BackupInfo, Error> {
        let parsed = portable_file::parse_readable(bytes)?;
        Ok(BackupInfo {
            source_family_id: parsed.source_family_id,
            snapshot_utc_ms: parsed.snapshot_utc_ms,
            known_gap: parsed.known_gap,
            record_count: parsed.rows.len() as u64,
        })
    }

    pub fn inspect_protected(
        bytes: &[u8],
        password: &str,
        available_memory_bytes: u64,
    ) -> Result<BackupInfo, Error> {
        let readable = portable_file::open_protected(bytes, password, available_memory_bytes)?;
        Self::inspect_readable(&readable)
    }

    pub fn protected_backup(
        &self,
        family: FamilyHandle,
        now_ms: i64,
        password: &str,
        available_memory_bytes: u64,
    ) -> Result<Vec<u8>, Error> {
        let readable = self.backup(family, now_ms)?;
        Ok(portable_file::protect_readable(
            &readable,
            password,
            available_memory_bytes,
        )?)
    }

    pub fn restore(&mut self, bytes: &[u8], now_ms: i64) -> Result<FamilyHandle, Error> {
        Ok(portable_file::restore_readable(
            &mut self.store,
            bytes,
            now_ms,
        )?)
    }

    pub fn restore_protected(
        &mut self,
        bytes: &[u8],
        password: &str,
        available_memory_bytes: u64,
        now_ms: i64,
    ) -> Result<FamilyHandle, Error> {
        Ok(portable_file::restore_protected(
            &mut self.store,
            bytes,
            password,
            available_memory_bytes,
            now_ms,
        )?)
    }
}

pub fn child_operation(
    family: FamilyHandle,
    name: &str,
    now_ms: i64,
) -> Result<([u8; 16], NewOperation), Error> {
    child_operation_with_metadata(family, name, None, None, now_ms)
}

pub fn child_operation_with_metadata(
    family: FamilyHandle,
    name: &str,
    birth_day: Option<i64>,
    sex: Option<u8>,
    now_ms: i64,
) -> Result<([u8; 16], NewOperation), Error> {
    check_time(now_ms)?;
    if name.trim().is_empty() || name.len() > 16 * 1024 {
        return Err(Error::Invalid("child name empty or too long"));
    }
    if sex.is_some_and(|code| !(1..=3).contains(&code)) {
        return Err(Error::Invalid("child sex code outside published range"));
    }
    let mut fields = vec![(1, Value::Text(name.trim().to_owned()))];
    if let Some(day) = birth_day {
        fields.push((2, Value::Integer(day.into())));
    }
    if let Some(code) = sex {
        fields.push((3, Value::Integer(code.into())));
    }
    let id = ids::random_v7(now_ms)?;
    Ok((
        id,
        NewOperation {
            family_id: family.family_id,
            operation_id: ids::random_v7(now_ms)?,
            record_id: id,
            scope: Scope::Child,
            kind: Kind::Create,
            author_device_id: family.device_id,
            hlc: placeholder_hlc(family),
            record_type: Some("child".to_owned()),
            child_id: None,
            fields: Some(fields),
        },
    ))
}

pub fn rename_child_operation(
    family: FamilyHandle,
    child: &Record,
    name: &str,
    saved_at_ms: i64,
) -> Result<NewOperation, Error> {
    if child.scope != Scope::Child || child.record_type != "child" || child.deleted {
        return Err(Error::Invalid("target child unavailable"));
    }
    check_time(saved_at_ms)?;
    let name = name.trim();
    if name.is_empty() || name.len() > 16 * 1024 {
        return Err(Error::Invalid("child name empty or too long"));
    }
    Ok(NewOperation {
        family_id: family.family_id,
        operation_id: ids::random_v7(saved_at_ms)?,
        record_id: child.id,
        scope: Scope::Child,
        kind: Kind::Set,
        author_device_id: family.device_id,
        hlc: placeholder_hlc(family),
        record_type: None,
        child_id: None,
        fields: Some(vec![(1, Value::Text(name.to_owned()))]),
    })
}

pub fn edit_child_metadata_operation(
    family: FamilyHandle,
    child: &Record,
    birth_day: Option<i64>,
    sex: Option<u8>,
    saved_at_ms: i64,
) -> Result<NewOperation, Error> {
    if child.scope != Scope::Child || child.record_type != "child" || child.deleted {
        return Err(Error::Invalid("target child unavailable"));
    }
    check_time(saved_at_ms)?;
    if sex.is_some_and(|code| !(1..=3).contains(&code)) {
        return Err(Error::Invalid("child sex code outside published range"));
    }
    let mut fields = Vec::new();
    if let Some(day) = birth_day {
        fields.push((2, Value::Integer(day.into())));
    }
    if let Some(code) = sex {
        fields.push((3, Value::Integer(code.into())));
    }
    if fields.is_empty() {
        return Err(Error::Invalid("child metadata correction is empty"));
    }
    Ok(NewOperation {
        family_id: family.family_id,
        operation_id: ids::random_v7(saved_at_ms)?,
        record_id: child.id,
        scope: Scope::Child,
        kind: Kind::Set,
        author_device_id: family.device_id,
        hlc: placeholder_hlc(family),
        record_type: None,
        child_id: None,
        fields: Some(fields),
    })
}

pub fn diaper_operation(
    family: FamilyHandle,
    child_id: [u8; 16],
    kind: u8,
    time: ActivityTime,
) -> Result<([u8; 16], NewOperation), Error> {
    if !(1..=4).contains(&kind) {
        return Err(Error::Invalid("diaper kind outside published codes"));
    }
    activity_operation(
        family,
        child_id,
        "diaper",
        vec![(100, Value::Integer(kind.into()))],
        time,
    )
}

pub fn bottle_operation(
    family: FamilyHandle,
    child_id: [u8; 16],
    amount_ml: i64,
    content: u8,
    time: ActivityTime,
) -> Result<([u8; 16], NewOperation), Error> {
    bottle_entered_operation(family, child_id, &amount_ml.to_string(), 1, content, time)
}

fn bottle_measure(entered: &str, unit: u8) -> Result<Value, Error> {
    let decimal = entered.trim();
    if decimal.is_empty() || decimal.len() > 16 || !(1..=3).contains(&unit) {
        return Err(Error::Invalid("bottle amount or unit invalid"));
    }
    let (numerator, denominator) = crate::record_validity::parse_decimal(decimal)
        .ok_or(Error::Invalid("bottle decimal invalid"))?;
    let (factor_num, factor_den) = crate::record_validity::unit_factor(unit.into());
    let scaled = numerator
        .checked_mul(factor_num)
        .ok_or(Error::Invalid("bottle amount overflow"))?;
    let divisor = denominator
        .checked_mul(factor_den)
        .ok_or(Error::Invalid("bottle amount overflow"))?;
    let base = crate::record_validity::round_ratio(scaled, divisor)
        .map_err(|_| Error::Invalid("bottle amount overflow"))?;
    if !(1..=1_000_000).contains(&base) {
        return Err(Error::Invalid("bottle amount invalid"));
    }
    Ok(Value::Map(vec![
        (1, Value::Integer(base)),
        (2, Value::Text(decimal.to_owned())),
        (3, Value::Integer(unit.into())),
    ]))
}

pub fn bottle_entered_operation(
    family: FamilyHandle,
    child_id: [u8; 16],
    entered: &str,
    unit: u8,
    content: u8,
    time: ActivityTime,
) -> Result<([u8; 16], NewOperation), Error> {
    if !(1..=4).contains(&content) {
        return Err(Error::Invalid("bottle content invalid"));
    }
    let measure = bottle_measure(entered, unit)?;
    activity_operation(
        family,
        child_id,
        "feed.bottle",
        vec![(100, measure), (101, Value::Integer(content.into()))],
        time,
    )
}

pub fn breast_feed_operation(
    family: FamilyHandle,
    child_id: [u8; 16],
    side: u8,
    time: ActivityTime,
    end_utc_ms: i64,
) -> Result<([u8; 16], NewOperation), Error> {
    breast_feed_segments_operation(
        family,
        child_id,
        &[BreastSegment {
            side,
            start_utc_ms: time.start_utc_ms,
            end_utc_ms,
            start_offset_minutes: time.offset_minutes,
            end_offset_minutes: time.offset_minutes,
        }],
        time,
    )
}

pub fn breast_feed_segments_operation(
    family: FamilyHandle,
    child_id: [u8; 16],
    segments: &[BreastSegment],
    time: ActivityTime,
) -> Result<([u8; 16], NewOperation), Error> {
    let fields = breast_segment_fields(segments, time)?;
    activity_operation(family, child_id, "feed.breast", fields, time)
}

fn breast_segment_fields(
    segments: &[BreastSegment],
    time: ActivityTime,
) -> Result<Vec<(u64, Value)>, Error> {
    if segments.is_empty()
        || segments.len() > 8
        || segments[0].start_utc_ms != time.start_utc_ms
        || segments[0].start_offset_minutes != time.offset_minutes
    {
        return Err(Error::Invalid("breast segment count or start invalid"));
    }
    let mut previous_end = time.start_utc_ms;
    let mut encoded = Vec::with_capacity(segments.len());
    for segment in segments {
        if !(1..=2).contains(&segment.side)
            || !(-840..=840).contains(&segment.start_offset_minutes)
            || !(-840..=840).contains(&segment.end_offset_minutes)
            || segment.start_utc_ms != previous_end
            || segment.end_utc_ms <= segment.start_utc_ms
            || segment.end_utc_ms > time.saved_at_ms
        {
            return Err(Error::Invalid("breast segment side or interval invalid"));
        }
        let start = Value::Array(vec![
            Value::Integer(segment.start_utc_ms.into()),
            Value::Integer(segment.start_offset_minutes.into()),
        ]);
        let end = Value::Array(vec![
            Value::Integer(segment.end_utc_ms.into()),
            Value::Integer(segment.end_offset_minutes.into()),
        ]);
        encoded.push(Value::Array(vec![
            Value::Integer(segment.side.into()),
            start,
            end,
        ]));
        previous_end = segment.end_utc_ms;
    }
    if previous_end
        .checked_sub(time.start_utc_ms)
        .is_none_or(|duration| duration > 240 * 60_000)
    {
        return Err(Error::Invalid("breast feed interval exceeds four hours"));
    }
    let end = Value::Array(vec![
        Value::Integer(previous_end.into()),
        Value::Integer(segments.last().unwrap().end_offset_minutes.into()),
    ]);
    Ok(vec![(2, end), (100, Value::Array(encoded))])
}

pub fn edit_breast_feed_segments_operation(
    family: FamilyHandle,
    child_id: [u8; 16],
    activity: &Record,
    segments: &[BreastSegment],
    saved_at_ms: i64,
) -> Result<NewOperation, Error> {
    if activity.scope != Scope::Activity
        || activity.child_id != Some(child_id)
        || activity.record_type != "feed.breast"
        || activity.deleted
    {
        return Err(Error::Invalid("breast feed target unavailable"));
    }
    check_time(saved_at_ms)?;
    let Some(Value::Array(start)) = activity.field(1).map(|field| &field.value) else {
        return Err(Error::Invalid("breast feed start unavailable"));
    };
    let [Value::Integer(start_ms), Value::Integer(offset)] = start.as_slice() else {
        return Err(Error::Invalid("breast feed start unavailable"));
    };
    let time = ActivityTime {
        start_utc_ms: i64::try_from(*start_ms)
            .map_err(|_| Error::Invalid("breast feed start unavailable"))?,
        offset_minutes: i16::try_from(*offset)
            .map_err(|_| Error::Invalid("breast feed start unavailable"))?,
        saved_at_ms,
    };
    Ok(NewOperation {
        family_id: family.family_id,
        operation_id: ids::random_v7(saved_at_ms)?,
        record_id: activity.id,
        scope: Scope::Activity,
        kind: Kind::Set,
        author_device_id: family.device_id,
        hlc: placeholder_hlc(family),
        record_type: None,
        child_id: None,
        fields: Some(breast_segment_fields(segments, time)?),
    })
}

pub fn pump_operation(
    family: FamilyHandle,
    child_id: [u8; 16],
    amounts: PumpAmounts,
    time: ActivityTime,
    end_utc_ms: i64,
) -> Result<([u8; 16], NewOperation), Error> {
    if end_utc_ms < time.start_utc_ms || end_utc_ms > time.saved_at_ms {
        return Err(Error::Invalid("pump interval invalid"));
    }
    let mut fields = vec![(
        2,
        Value::Array(vec![
            Value::Integer(end_utc_ms.into()),
            Value::Integer(time.offset_minutes.into()),
        ]),
    )];
    fields.extend(pump_amount_fields(amounts, false)?);
    activity_operation(family, child_id, "pump", fields, time)
}

fn pump_amount_fields(
    amounts: PumpAmounts,
    clear_missing: bool,
) -> Result<Vec<(u64, Value)>, Error> {
    let PumpAmounts {
        left_ml,
        right_ml,
        total_ml,
    } = amounts;
    if total_ml.is_some() && (left_ml.is_some() || right_ml.is_some())
        || total_ml.is_none() && left_ml.unwrap_or(0) <= 0 && right_ml.unwrap_or(0) <= 0
        || [left_ml, right_ml, total_ml]
            .into_iter()
            .flatten()
            .any(|amount| !(0..=1_000_000).contains(&amount))
        || total_ml == Some(0)
    {
        return Err(Error::Invalid("pump amounts invalid"));
    }
    Ok([(100, left_ml), (101, right_ml), (102, total_ml)]
        .into_iter()
        .filter_map(|(id, amount)| {
            amount
                .map(|value| (id, whole_measure(value, 1)))
                .or_else(|| clear_missing.then_some((id, Value::Null)))
        })
        .collect())
}

pub fn edit_pump_amounts_operation(
    family: FamilyHandle,
    child_id: [u8; 16],
    activity: &Record,
    amounts: PumpAmounts,
    saved_at_ms: i64,
) -> Result<NewOperation, Error> {
    if activity.scope != Scope::Activity
        || activity.child_id != Some(child_id)
        || activity.record_type != "pump"
        || activity.deleted
    {
        return Err(Error::Invalid("pump target unavailable"));
    }
    check_time(saved_at_ms)?;
    Ok(NewOperation {
        family_id: family.family_id,
        operation_id: ids::random_v7(saved_at_ms)?,
        record_id: activity.id,
        scope: Scope::Activity,
        kind: Kind::Set,
        author_device_id: family.device_id,
        hlc: placeholder_hlc(family),
        record_type: None,
        child_id: None,
        fields: Some(pump_amount_fields(amounts, true)?),
    })
}

pub fn solids_operation(
    family: FamilyHandle,
    child_id: [u8; 16],
    foods: &[String],
    amount: &str,
    time: ActivityTime,
) -> Result<([u8; 16], NewOperation), Error> {
    activity_operation(
        family,
        child_id,
        "feed.solids",
        solids_fields(foods, amount)?,
        time,
    )
}

fn solids_fields(foods: &[String], amount: &str) -> Result<Vec<(u64, Value)>, Error> {
    let normalized: Vec<_> = foods.iter().map(|food| food.trim()).collect();
    let amount = amount.trim();
    if normalized.is_empty()
        || normalized.len() > 32
        || normalized
            .iter()
            .any(|food| food.is_empty() || food.len() > 256)
        || amount.len() > 256
    {
        return Err(Error::Invalid("solids foods or amount invalid"));
    }
    Ok(vec![
        (
            100,
            Value::Array(
                normalized
                    .into_iter()
                    .map(|food| Value::Text(food.into()))
                    .collect(),
            ),
        ),
        (101, Value::Text(amount.into())),
    ])
}

pub fn edit_solids_operation(
    family: FamilyHandle,
    child_id: [u8; 16],
    activity: &Record,
    foods: &[String],
    amount: &str,
    saved_at_ms: i64,
) -> Result<NewOperation, Error> {
    if activity.scope != Scope::Activity
        || activity.child_id != Some(child_id)
        || activity.record_type != "feed.solids"
        || activity.deleted
    {
        return Err(Error::Invalid("solids target unavailable"));
    }
    check_time(saved_at_ms)?;
    Ok(NewOperation {
        family_id: family.family_id,
        operation_id: ids::random_v7(saved_at_ms)?,
        record_id: activity.id,
        scope: Scope::Activity,
        kind: Kind::Set,
        author_device_id: family.device_id,
        hlc: placeholder_hlc(family),
        record_type: None,
        child_id: None,
        fields: Some(solids_fields(foods, amount)?),
    })
}

pub fn sleep_operation(
    family: FamilyHandle,
    child_id: [u8; 16],
    time: ActivityTime,
    end_utc_ms: i64,
    end_offset_minutes: i16,
) -> Result<([u8; 16], NewOperation), Error> {
    sleep_operation_with_place(family, child_id, time, end_utc_ms, end_offset_minutes, None)
}

pub fn sleep_operation_with_place(
    family: FamilyHandle,
    child_id: [u8; 16],
    time: ActivityTime,
    end_utc_ms: i64,
    end_offset_minutes: i16,
    place: Option<u8>,
) -> Result<([u8; 16], NewOperation), Error> {
    if end_utc_ms < time.start_utc_ms || end_utc_ms > time.saved_at_ms {
        return Err(Error::Invalid("sleep end outside completed interval"));
    }
    if !(-840..=840).contains(&end_offset_minutes) {
        return Err(Error::Invalid("sleep end offset outside v1 range"));
    }
    let mut fields = vec![(
        2,
        Value::Array(vec![
            Value::Integer(end_utc_ms.into()),
            Value::Integer(end_offset_minutes.into()),
        ]),
    )];
    fields.extend(sleep_place_field(place)?);
    activity_operation(family, child_id, "sleep", fields, time)
}

pub fn running_sleep_operation(
    family: FamilyHandle,
    child_id: [u8; 16],
    time: ActivityTime,
) -> Result<([u8; 16], NewOperation), Error> {
    running_sleep_operation_with_place(family, child_id, time, None)
}

pub fn running_sleep_operation_with_place(
    family: FamilyHandle,
    child_id: [u8; 16],
    time: ActivityTime,
    place: Option<u8>,
) -> Result<([u8; 16], NewOperation), Error> {
    activity_operation(family, child_id, "sleep", sleep_place_field(place)?, time)
}

fn sleep_place_field(place: Option<u8>) -> Result<Vec<(u64, Value)>, Error> {
    match place {
        Some(code @ 1..=5) => Ok(vec![(100, Value::Integer(code.into()))]),
        Some(_) => Err(Error::Invalid("sleep place outside v1 range")),
        None => Ok(Vec::new()),
    }
}

pub fn edit_sleep_place_operation(
    family: FamilyHandle,
    child_id: [u8; 16],
    activity: &Record,
    place: Option<u8>,
    saved_at_ms: i64,
) -> Result<NewOperation, Error> {
    if activity.scope != Scope::Activity
        || activity.record_type != "sleep"
        || activity.child_id != Some(child_id)
        || activity.deleted
    {
        return Err(Error::Invalid("sleep activity unavailable"));
    }
    check_time(saved_at_ms)?;
    let value = match place {
        Some(code @ 1..=5) => Value::Integer(code.into()),
        Some(_) => return Err(Error::Invalid("sleep place outside v1 range")),
        None => Value::Null,
    };
    Ok(NewOperation {
        family_id: family.family_id,
        operation_id: ids::random_v7(saved_at_ms)?,
        record_id: activity.id,
        scope: Scope::Activity,
        kind: Kind::Set,
        author_device_id: family.device_id,
        hlc: placeholder_hlc(family),
        record_type: None,
        child_id: None,
        fields: Some(vec![(100, value)]),
    })
}

pub fn stop_sleep_operation(
    family: FamilyHandle,
    child_id: [u8; 16],
    activity: &Record,
    end_utc_ms: i64,
    end_offset_minutes: i16,
    saved_at_ms: i64,
) -> Result<NewOperation, Error> {
    if activity.scope != Scope::Activity
        || activity.record_type != "sleep"
        || activity.child_id != Some(child_id)
        || activity.deleted
        || activity
            .field(2)
            .is_some_and(|field| field.value != Value::Null)
    {
        return Err(Error::Invalid("sleep target is not running"));
    }
    let Value::Array(start) = &activity
        .field(1)
        .ok_or(Error::Invalid("sleep start absent"))?
        .value
    else {
        return Err(Error::Invalid("sleep start invalid"));
    };
    let [Value::Integer(start_utc_ms), Value::Integer(_)] = start.as_slice() else {
        return Err(Error::Invalid("sleep start invalid"));
    };
    let start_utc_ms = i64::try_from(*start_utc_ms)
        .map_err(|_| Error::Invalid("sleep start outside i64 range"))?;
    if end_utc_ms < start_utc_ms || end_utc_ms > saved_at_ms {
        return Err(Error::Invalid("sleep end outside completed interval"));
    }
    if !(-840..=840).contains(&end_offset_minutes) {
        return Err(Error::Invalid("sleep end offset outside v1 range"));
    }
    check_time(saved_at_ms)?;
    Ok(NewOperation {
        family_id: family.family_id,
        operation_id: ids::random_v7(saved_at_ms)?,
        record_id: activity.id,
        scope: Scope::Activity,
        kind: Kind::Set,
        author_device_id: family.device_id,
        hlc: placeholder_hlc(family),
        record_type: None,
        child_id: None,
        fields: Some(vec![(
            2,
            Value::Array(vec![
                Value::Integer(end_utc_ms.into()),
                Value::Integer(end_offset_minutes.into()),
            ]),
        )]),
    })
}

pub fn edit_sleep_end_operation(
    family: FamilyHandle,
    child_id: [u8; 16],
    activity: &Record,
    end_utc_ms: i64,
    end_offset_minutes: i16,
    saved_at_ms: i64,
) -> Result<NewOperation, Error> {
    if activity.scope != Scope::Activity
        || activity.record_type != "sleep"
        || activity.child_id != Some(child_id)
        || activity.deleted
        || !matches!(
            activity.field(2).map(|field| &field.value),
            Some(Value::Array(_))
        )
    {
        return Err(Error::Invalid("completed sleep target unavailable"));
    }
    let Value::Array(start) = &activity
        .field(1)
        .ok_or(Error::Invalid("sleep start absent"))?
        .value
    else {
        return Err(Error::Invalid("sleep start invalid"));
    };
    let [Value::Integer(start_utc_ms), Value::Integer(_)] = start.as_slice() else {
        return Err(Error::Invalid("sleep start invalid"));
    };
    let start_utc_ms = i64::try_from(*start_utc_ms)
        .map_err(|_| Error::Invalid("sleep start outside i64 range"))?;
    if end_utc_ms <= start_utc_ms || end_utc_ms > saved_at_ms {
        return Err(Error::Invalid("sleep end outside completed interval"));
    }
    if !(-840..=840).contains(&end_offset_minutes) {
        return Err(Error::Invalid("sleep end offset outside v1 range"));
    }
    check_time(saved_at_ms)?;
    Ok(NewOperation {
        family_id: family.family_id,
        operation_id: ids::random_v7(saved_at_ms)?,
        record_id: activity.id,
        scope: Scope::Activity,
        kind: Kind::Set,
        author_device_id: family.device_id,
        hlc: placeholder_hlc(family),
        record_type: None,
        child_id: None,
        fields: Some(vec![(
            2,
            Value::Array(vec![
                Value::Integer(end_utc_ms.into()),
                Value::Integer(end_offset_minutes.into()),
            ]),
        )]),
    })
}

pub fn delete_activity_operation(
    family: FamilyHandle,
    child_id: [u8; 16],
    activity: &Record,
    saved_at_ms: i64,
) -> Result<NewOperation, Error> {
    if activity.scope != Scope::Activity || activity.child_id != Some(child_id) || activity.deleted
    {
        return Err(Error::Invalid("activity target unavailable"));
    }
    check_time(saved_at_ms)?;
    Ok(NewOperation {
        family_id: family.family_id,
        operation_id: ids::random_v7(saved_at_ms)?,
        record_id: activity.id,
        scope: Scope::Activity,
        kind: Kind::Delete,
        author_device_id: family.device_id,
        hlc: placeholder_hlc(family),
        record_type: None,
        child_id: None,
        fields: None,
    })
}

pub fn note_operation(
    family: FamilyHandle,
    child_id: [u8; 16],
    note: &str,
    time: ActivityTime,
) -> Result<([u8; 16], NewOperation), Error> {
    let note = note.trim();
    if note.is_empty() || note.len() > 4096 {
        return Err(Error::Invalid("note must contain 1 to 4096 bytes"));
    }
    activity_operation(
        family,
        child_id,
        "note",
        vec![(4, Value::Text(note.to_owned()))],
        time,
    )
}

pub fn edit_note_operation(
    family: FamilyHandle,
    child_id: [u8; 16],
    activity: &Record,
    note: &str,
    saved_at_ms: i64,
) -> Result<NewOperation, Error> {
    if activity.scope != Scope::Activity
        || activity.child_id != Some(child_id)
        || activity.deleted
        || !matches!(
            activity.record_type.as_str(),
            "note"
                | "feed.breast"
                | "feed.bottle"
                | "feed.solids"
                | "sleep"
                | "pump"
                | "diaper"
                | "growth"
                | "medication"
                | "temperature"
        )
    {
        return Err(Error::Invalid("note target unavailable"));
    }
    check_time(saved_at_ms)?;
    let note = note.trim();
    if (note.is_empty() && activity.record_type == "note") || note.len() > 4096 {
        return Err(Error::Invalid("note outside supported length"));
    }
    Ok(NewOperation {
        family_id: family.family_id,
        operation_id: ids::random_v7(saved_at_ms)?,
        record_id: activity.id,
        scope: Scope::Activity,
        kind: Kind::Set,
        author_device_id: family.device_id,
        hlc: placeholder_hlc(family),
        record_type: None,
        child_id: None,
        fields: Some(vec![(
            4,
            if note.is_empty() {
                Value::Null
            } else {
                Value::Text(note.to_owned())
            },
        )]),
    })
}

pub fn edit_bottle_ml_operation(
    family: FamilyHandle,
    child_id: [u8; 16],
    activity: &Record,
    amount_ml: i64,
    saved_at_ms: i64,
) -> Result<NewOperation, Error> {
    edit_bottle_measure_operation(
        family,
        child_id,
        activity,
        &amount_ml.to_string(),
        1,
        saved_at_ms,
    )
}

fn edit_bottle_measure_operation(
    family: FamilyHandle,
    child_id: [u8; 16],
    activity: &Record,
    entered: &str,
    unit: u8,
    saved_at_ms: i64,
) -> Result<NewOperation, Error> {
    if activity.scope != Scope::Activity
        || activity.child_id != Some(child_id)
        || activity.record_type != "feed.bottle"
        || activity.deleted
    {
        return Err(Error::Invalid("bottle target unavailable"));
    }
    check_time(saved_at_ms)?;
    let measure = bottle_measure(entered, unit)?;
    Ok(NewOperation {
        family_id: family.family_id,
        operation_id: ids::random_v7(saved_at_ms)?,
        record_id: activity.id,
        scope: Scope::Activity,
        kind: Kind::Set,
        author_device_id: family.device_id,
        hlc: placeholder_hlc(family),
        record_type: None,
        child_id: None,
        fields: Some(vec![(100, measure)]),
    })
}

pub fn edit_bottle_entered_operation(
    family: FamilyHandle,
    child_id: [u8; 16],
    activity: &Record,
    entered: &str,
    unit: u8,
    content: u8,
    saved_at_ms: i64,
) -> Result<NewOperation, Error> {
    if !(1..=4).contains(&content) {
        return Err(Error::Invalid("bottle content invalid"));
    }
    let mut operation =
        edit_bottle_measure_operation(family, child_id, activity, entered, unit, saved_at_ms)?;
    operation
        .fields
        .as_mut()
        .expect("bottle edit has fields")
        .push((101, Value::Integer(content.into())));
    Ok(operation)
}

pub fn edit_bottle_operation(
    family: FamilyHandle,
    child_id: [u8; 16],
    activity: &Record,
    amount_ml: i64,
    content: u8,
    saved_at_ms: i64,
) -> Result<NewOperation, Error> {
    edit_bottle_entered_operation(
        family,
        child_id,
        activity,
        &amount_ml.to_string(),
        1,
        content,
        saved_at_ms,
    )
}

pub fn edit_diaper_kind_operation(
    family: FamilyHandle,
    child_id: [u8; 16],
    activity: &Record,
    kind: u8,
    saved_at_ms: i64,
) -> Result<NewOperation, Error> {
    if activity.scope != Scope::Activity
        || activity.child_id != Some(child_id)
        || activity.record_type != "diaper"
        || activity.deleted
    {
        return Err(Error::Invalid("diaper target unavailable"));
    }
    check_time(saved_at_ms)?;
    if !(1..=4).contains(&kind) {
        return Err(Error::Invalid("diaper kind outside published codes"));
    }
    Ok(NewOperation {
        family_id: family.family_id,
        operation_id: ids::random_v7(saved_at_ms)?,
        record_id: activity.id,
        scope: Scope::Activity,
        kind: Kind::Set,
        author_device_id: family.device_id,
        hlc: placeholder_hlc(family),
        record_type: None,
        child_id: None,
        fields: Some(vec![(100, Value::Integer(kind.into()))]),
    })
}

pub fn growth_operation(
    family: FamilyHandle,
    child_id: [u8; 16],
    weight_g: Option<i64>,
    length_mm: Option<i64>,
    time: ActivityTime,
) -> Result<([u8; 16], NewOperation), Error> {
    growth_measurements_operation(family, child_id, weight_g, length_mm, None, time)
}

pub fn growth_measurements_operation(
    family: FamilyHandle,
    child_id: [u8; 16],
    weight_g: Option<i64>,
    length_mm: Option<i64>,
    head_mm: Option<i64>,
    time: ActivityTime,
) -> Result<([u8; 16], NewOperation), Error> {
    let fields = growth_fields(weight_g, length_mm, head_mm)?;
    activity_operation(family, child_id, "growth", fields, time)
}

fn growth_fields(
    weight_g: Option<i64>,
    length_mm: Option<i64>,
    head_mm: Option<i64>,
) -> Result<Vec<(u64, Value)>, Error> {
    if weight_g.is_none() && length_mm.is_none() && head_mm.is_none() {
        return Err(Error::Invalid("growth needs a measurement"));
    }
    if weight_g.is_some_and(|value| !(1..=100_000).contains(&value))
        || length_mm.is_some_and(|value| !(1..=2_500).contains(&value))
        || head_mm.is_some_and(|value| !(1..=1_000).contains(&value))
    {
        return Err(Error::Invalid("growth measurement outside supported range"));
    }
    let mut fields = Vec::new();
    if let Some(value) = weight_g {
        fields.push((100, whole_measure(value, 10)));
    }
    if let Some(value) = length_mm {
        fields.push((101, whole_measure(value, 20)));
    }
    if let Some(value) = head_mm {
        fields.push((102, whole_measure(value, 20)));
    }
    Ok(fields)
}

fn entered_growth_measure(
    input: &MeasurementInput,
    units: std::ops::RangeInclusive<u8>,
    maximum: i128,
) -> Result<Value, Error> {
    let entered = input.entered.trim();
    if entered.is_empty() || entered.len() > 16 || !units.contains(&input.unit) {
        return Err(Error::Invalid("growth decimal or unit invalid"));
    }
    let (numerator, denominator) = crate::record_validity::parse_decimal(entered)
        .ok_or(Error::Invalid("growth decimal invalid"))?;
    let (factor_num, factor_den) = crate::record_validity::unit_factor(input.unit.into());
    let scaled = numerator
        .checked_mul(factor_num)
        .ok_or(Error::Invalid("growth measurement overflow"))?;
    let divisor = denominator
        .checked_mul(factor_den)
        .ok_or(Error::Invalid("growth measurement overflow"))?;
    let base = crate::record_validity::round_ratio(scaled, divisor)
        .map_err(|_| Error::Invalid("growth measurement overflow"))?;
    if !(1..=maximum).contains(&base) {
        return Err(Error::Invalid("growth measurement outside supported range"));
    }
    Ok(Value::Map(vec![
        (1, Value::Integer(base)),
        (2, Value::Text(entered.to_owned())),
        (3, Value::Integer(input.unit.into())),
    ]))
}

fn growth_entered_fields(input: &GrowthInput) -> Result<Vec<(u64, Value)>, Error> {
    let mut fields = Vec::new();
    if let Some(weight) = &input.weight {
        fields.push((100, entered_growth_measure(weight, 10..=13, 100_000)?));
    }
    if let Some(length) = &input.length {
        fields.push((101, entered_growth_measure(length, 20..=22, 2_500)?));
    }
    if let Some(head) = &input.head {
        fields.push((102, entered_growth_measure(head, 20..=22, 1_000)?));
    }
    if fields.is_empty() {
        return Err(Error::Invalid("growth needs a measurement"));
    }
    Ok(fields)
}

pub fn growth_entered_operation(
    family: FamilyHandle,
    child_id: [u8; 16],
    input: &GrowthInput,
    time: ActivityTime,
) -> Result<([u8; 16], NewOperation), Error> {
    activity_operation(
        family,
        child_id,
        "growth",
        growth_entered_fields(input)?,
        time,
    )
}

pub fn edit_growth_entered_operation(
    family: FamilyHandle,
    child_id: [u8; 16],
    activity: &Record,
    input: &GrowthInput,
    saved_at_ms: i64,
) -> Result<NewOperation, Error> {
    if activity.scope != Scope::Activity
        || activity.child_id != Some(child_id)
        || activity.record_type != "growth"
        || activity.deleted
    {
        return Err(Error::Invalid("growth target unavailable"));
    }
    check_time(saved_at_ms)?;
    Ok(NewOperation {
        family_id: family.family_id,
        operation_id: ids::random_v7(saved_at_ms)?,
        record_id: activity.id,
        scope: Scope::Activity,
        kind: Kind::Set,
        author_device_id: family.device_id,
        hlc: placeholder_hlc(family),
        record_type: None,
        child_id: None,
        fields: Some(growth_entered_fields(input)?),
    })
}

pub fn edit_growth_operation(
    family: FamilyHandle,
    child_id: [u8; 16],
    activity: &Record,
    weight_g: Option<i64>,
    length_mm: Option<i64>,
    saved_at_ms: i64,
) -> Result<NewOperation, Error> {
    edit_growth_measurements_operation(
        family,
        child_id,
        activity,
        weight_g,
        length_mm,
        None,
        saved_at_ms,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn edit_growth_measurements_operation(
    family: FamilyHandle,
    child_id: [u8; 16],
    activity: &Record,
    weight_g: Option<i64>,
    length_mm: Option<i64>,
    head_mm: Option<i64>,
    saved_at_ms: i64,
) -> Result<NewOperation, Error> {
    if activity.scope != Scope::Activity
        || activity.child_id != Some(child_id)
        || activity.record_type != "growth"
        || activity.deleted
    {
        return Err(Error::Invalid("growth target unavailable"));
    }
    check_time(saved_at_ms)?;
    Ok(NewOperation {
        family_id: family.family_id,
        operation_id: ids::random_v7(saved_at_ms)?,
        record_id: activity.id,
        scope: Scope::Activity,
        kind: Kind::Set,
        author_device_id: family.device_id,
        hlc: placeholder_hlc(family),
        record_type: None,
        child_id: None,
        fields: Some(growth_fields(weight_g, length_mm, head_mm)?),
    })
}

pub fn temperature_c_operation(
    family: FamilyHandle,
    child_id: [u8; 16],
    entered_c: &str,
    time: ActivityTime,
) -> Result<([u8; 16], NewOperation), Error> {
    temperature_entered_operation(family, child_id, entered_c, 30, time)
}

pub fn temperature_entered_operation(
    family: FamilyHandle,
    child_id: [u8; 16],
    entered: &str,
    unit: u8,
    time: ActivityTime,
) -> Result<([u8; 16], NewOperation), Error> {
    activity_operation(
        family,
        child_id,
        "temperature",
        temperature_fields(entered, unit)?,
        time,
    )
}

fn temperature_fields(entered: &str, unit: u8) -> Result<Vec<(u64, Value)>, Error> {
    let decimal = entered.trim();
    if decimal.is_empty() || decimal.len() > 16 || !matches!(unit, 30 | 31) {
        return Err(Error::Invalid("temperature decimal or unit invalid"));
    }
    let (numerator, denominator) = crate::record_validity::parse_decimal(decimal)
        .ok_or(Error::Invalid("temperature decimal invalid"))?;
    let (scaled, divisor) = if unit == 30 {
        (
            numerator
                .checked_mul(100)
                .ok_or(Error::Invalid("temperature decimal overflow"))?,
            denominator,
        )
    } else {
        (
            numerator
                .checked_sub(
                    denominator
                        .checked_mul(32)
                        .ok_or(Error::Invalid("temperature decimal overflow"))?,
                )
                .and_then(|difference| difference.checked_mul(500))
                .ok_or(Error::Invalid("temperature decimal overflow"))?,
            denominator
                .checked_mul(9)
                .ok_or(Error::Invalid("temperature decimal overflow"))?,
        )
    };
    let base = crate::record_validity::round_ratio(scaled, divisor)
        .map_err(|_| Error::Invalid("temperature decimal overflow"))?;
    i64::try_from(base).map_err(|_| Error::Invalid("temperature outside i64 range"))?;
    Ok(vec![(
        100,
        Value::Map(vec![
            (1, Value::Integer(base)),
            (2, Value::Text(decimal.to_owned())),
            (3, Value::Integer(unit.into())),
        ]),
    )])
}

pub fn edit_temperature_c_operation(
    family: FamilyHandle,
    child_id: [u8; 16],
    activity: &Record,
    entered_c: &str,
    saved_at_ms: i64,
) -> Result<NewOperation, Error> {
    edit_temperature_entered_operation(family, child_id, activity, entered_c, 30, saved_at_ms)
}

pub fn edit_temperature_entered_operation(
    family: FamilyHandle,
    child_id: [u8; 16],
    activity: &Record,
    entered: &str,
    unit: u8,
    saved_at_ms: i64,
) -> Result<NewOperation, Error> {
    if activity.scope != Scope::Activity
        || activity.child_id != Some(child_id)
        || activity.record_type != "temperature"
        || activity.deleted
    {
        return Err(Error::Invalid("temperature target unavailable"));
    }
    check_time(saved_at_ms)?;
    Ok(NewOperation {
        family_id: family.family_id,
        operation_id: ids::random_v7(saved_at_ms)?,
        record_id: activity.id,
        scope: Scope::Activity,
        kind: Kind::Set,
        author_device_id: family.device_id,
        hlc: placeholder_hlc(family),
        record_type: None,
        child_id: None,
        fields: Some(temperature_fields(entered, unit)?),
    })
}

pub fn medication_operation(
    family: FamilyHandle,
    child_id: [u8; 16],
    name: &str,
    dose_amount: &str,
    dose_unit: &str,
    time: ActivityTime,
) -> Result<([u8; 16], NewOperation), Error> {
    activity_operation(
        family,
        child_id,
        "medication",
        medication_fields(name, dose_amount, dose_unit)?,
        time,
    )
}

fn medication_fields(
    name: &str,
    dose_amount: &str,
    dose_unit: &str,
) -> Result<Vec<(u64, Value)>, Error> {
    let name = name.trim();
    let dose_amount = dose_amount.trim();
    let dose_unit = dose_unit.trim();
    if name.is_empty()
        || dose_amount.is_empty()
        || dose_unit.is_empty()
        || name.len() > 256
        || dose_amount.len() > 64
        || dose_unit.len() > 64
    {
        return Err(Error::Invalid("medication name or dose empty or too long"));
    }
    Ok(vec![
        (100, Value::Text(name.to_owned())),
        (
            101,
            Value::Array(vec![
                Value::Text(dose_amount.to_owned()),
                Value::Text(dose_unit.to_owned()),
            ]),
        ),
    ])
}

pub fn edit_medication_operation(
    family: FamilyHandle,
    child_id: [u8; 16],
    activity: &Record,
    name: &str,
    dose_amount: &str,
    dose_unit: &str,
    saved_at_ms: i64,
) -> Result<NewOperation, Error> {
    if activity.scope != Scope::Activity
        || activity.child_id != Some(child_id)
        || activity.record_type != "medication"
        || activity.deleted
    {
        return Err(Error::Invalid("medication target unavailable"));
    }
    check_time(saved_at_ms)?;
    Ok(NewOperation {
        family_id: family.family_id,
        operation_id: ids::random_v7(saved_at_ms)?,
        record_id: activity.id,
        scope: Scope::Activity,
        kind: Kind::Set,
        author_device_id: family.device_id,
        hlc: placeholder_hlc(family),
        record_type: None,
        child_id: None,
        fields: Some(medication_fields(name, dose_amount, dose_unit)?),
    })
}

fn whole_measure(value: i64, unit: i128) -> Value {
    Value::Map(vec![
        (1, Value::Integer(value.into())),
        (2, Value::Text(value.to_string())),
        (3, Value::Integer(unit)),
    ])
}

fn activity_operation(
    family: FamilyHandle,
    child_id: [u8; 16],
    record_type: &str,
    fields: Vec<(u64, Value)>,
    time: ActivityTime,
) -> Result<([u8; 16], NewOperation), Error> {
    check_time(time.saved_at_ms)?;
    if !(-840..=840).contains(&time.offset_minutes) {
        return Err(Error::Invalid("recorded offset outside v1 range"));
    }
    let id = ids::random_v7(time.saved_at_ms)?;
    let mut all_fields = vec![(
        1,
        Value::Array(vec![
            Value::Integer(time.start_utc_ms.into()),
            Value::Integer(time.offset_minutes.into()),
        ]),
    )];
    all_fields.extend(fields);
    Ok((
        id,
        NewOperation {
            family_id: family.family_id,
            operation_id: ids::random_v7(time.saved_at_ms)?,
            record_id: id,
            scope: Scope::Activity,
            kind: Kind::Create,
            author_device_id: family.device_id,
            hlc: placeholder_hlc(family),
            record_type: Some(record_type.to_owned()),
            child_id: Some(child_id),
            fields: Some(all_fields),
        },
    ))
}

pub fn children_from_records<'a>(records: impl Iterator<Item = &'a Record>) -> Vec<Child> {
    let mut children = records
        .filter(|record| record.scope == Scope::Child && !record.deleted)
        .filter_map(|record| {
            let Value::Text(name) = &record.field(1)?.value else {
                return None;
            };
            Some(Child {
                id: record.id,
                name: name.clone(),
                birth_day: record.field(2).and_then(|field| match field.value {
                    Value::Integer(value) => i64::try_from(value).ok(),
                    _ => None,
                }),
                sex: record.field(3).and_then(|field| match field.value {
                    Value::Integer(value) => u8::try_from(value).ok(),
                    _ => None,
                }),
            })
        })
        .collect::<Vec<_>>();
    children.sort_by(|a, b| a.name.cmp(&b.name).then(a.id.cmp(&b.id)));
    children
}

pub fn activities_from_records<'a>(records: impl Iterator<Item = &'a Record>) -> Vec<Activity> {
    let mut activities = records
        .filter(|record| record.scope == Scope::Activity && !record.deleted)
        .filter_map(activity_summary)
        .collect::<Vec<_>>();
    activities.sort_by(|a, b| b.start_utc_ms.cmp(&a.start_utc_ms).then(b.id.cmp(&a.id)));
    activities
}

fn activity_summary(record: &Record) -> Option<Activity> {
    let Value::Array(instant) = &record.field(1)?.value else {
        return None;
    };
    let [Value::Integer(start), Value::Integer(offset)] = instant.as_slice() else {
        return None;
    };
    let diaper_kind = if record.record_type == "diaper" {
        let Value::Integer(kind) = &record.field(100)?.value else {
            return None;
        };
        u8::try_from(*kind).ok()
    } else {
        None
    };
    let (bottle_ml, bottle_entered, bottle_unit) = if record.record_type == "feed.bottle" {
        let Value::Map(measure) = &record.field(100)?.value else {
            return None;
        };
        let Value::Integer(amount) = measure.first()?.1 else {
            return None;
        };
        let entered = match &measure.get(1)?.1 {
            Value::Text(value) => Some(value.clone()),
            _ => None,
        };
        let unit = match measure.get(2)?.1 {
            Value::Integer(value) => u8::try_from(value)
                .ok()
                .filter(|unit| (1..=3).contains(unit)),
            _ => None,
        };
        (
            i64::try_from(amount).ok(),
            entered.filter(|_| unit.is_some()),
            unit,
        )
    } else {
        (None, None, None)
    };
    let bottle_content = if record.record_type == "feed.bottle" {
        match &record.field(101)?.value {
            Value::Integer(content) => u8::try_from(*content).ok(),
            _ => None,
        }
    } else {
        None
    };
    let breast_segments = if record.record_type == "feed.breast" {
        let Value::Array(segments) = &record.field(100)?.value else {
            return None;
        };
        segments
            .iter()
            .map(|segment| {
                let Value::Array(parts) = segment else {
                    return None;
                };
                let [Value::Integer(side), Value::Array(start), Value::Array(end)] =
                    parts.as_slice()
                else {
                    return None;
                };
                let [Value::Integer(start_ms), Value::Integer(start_offset)] = start.as_slice()
                else {
                    return None;
                };
                let [Value::Integer(end_ms), Value::Integer(end_offset)] = end.as_slice() else {
                    return None;
                };
                Some(BreastSegment {
                    side: u8::try_from(*side).ok()?,
                    start_utc_ms: i64::try_from(*start_ms).ok()?,
                    end_utc_ms: i64::try_from(*end_ms).ok()?,
                    start_offset_minutes: i16::try_from(*start_offset).ok()?,
                    end_offset_minutes: i16::try_from(*end_offset).ok()?,
                })
            })
            .collect::<Option<Vec<_>>>()
    } else {
        None
    };
    let breast_side = breast_segments
        .as_ref()
        .filter(|segments| segments.len() == 1)
        .map(|segments| segments[0].side);
    let (solids_foods, solids_amount) = if record.record_type == "feed.solids" {
        let Value::Array(foods) = &record.field(100)?.value else {
            return None;
        };
        let Value::Text(amount) = &record.field(101)?.value else {
            return None;
        };
        let foods = foods
            .iter()
            .map(|food| match food {
                Value::Text(text) => Some(text.clone()),
                _ => None,
            })
            .collect::<Option<Vec<_>>>()?;
        (Some(foods), Some(amount.clone()))
    } else {
        (None, None)
    };
    let growth_measure = |field| -> Option<(i64, Option<String>, Option<u8>)> {
        let Value::Map(measure) = &record.field(field)?.value else {
            return None;
        };
        let (1, Value::Integer(value)) = measure.first()? else {
            return None;
        };
        let entered = match measure.get(1) {
            Some((2, Value::Text(text))) => Some(text.clone()),
            _ => None,
        };
        let unit = match measure.get(2) {
            Some((3, Value::Integer(code))) => u8::try_from(*code).ok(),
            _ => None,
        };
        Some((i64::try_from(*value).ok()?, entered, unit))
    };
    let growth_weight = (record.record_type == "growth")
        .then(|| growth_measure(100))
        .flatten();
    let growth_length = (record.record_type == "growth")
        .then(|| growth_measure(101))
        .flatten();
    let growth_head = (record.record_type == "growth")
        .then(|| growth_measure(102))
        .flatten();
    let pump_measure = |field| -> Option<i64> {
        let Value::Map(measure) = &record.field(field)?.value else {
            return None;
        };
        let (1, Value::Integer(value)) = measure.first()? else {
            return None;
        };
        i64::try_from(*value).ok()
    };
    let (temperature_c, temperature_entered, temperature_unit) =
        if record.record_type == "temperature" {
            let Value::Map(measure) = &record.field(100)?.value else {
                return None;
            };
            let (1, Value::Integer(base)) = measure.first()? else {
                return None;
            };
            let base = i64::try_from(*base).ok()?;
            let entered = match measure.get(1) {
                Some((2, Value::Text(decimal))) => Some(decimal.clone()),
                _ => None,
            };
            let unit = match measure.get(2) {
                Some((3, Value::Integer(value))) => u8::try_from(*value)
                    .ok()
                    .filter(|unit| matches!(unit, 30 | 31)),
                _ => None,
            };
            let celsius = if unit == Some(30) {
                entered.clone()
            } else {
                let magnitude = base.unsigned_abs();
                Some(format!(
                    "{}{}.{:02}",
                    if base < 0 { "-" } else { "" },
                    magnitude / 100,
                    magnitude % 100
                ))
            };
            (celsius, entered.filter(|_| unit.is_some()), unit)
        } else {
            (None, None, None)
        };
    let (medication_name, medication_dose_amount, medication_dose_unit) =
        if record.record_type == "medication" {
            let Value::Text(name) = &record.field(100)?.value else {
                return None;
            };
            let Value::Array(dose) = &record.field(101)?.value else {
                return None;
            };
            let [Value::Text(amount), Value::Text(unit)] = dose.as_slice() else {
                return None;
            };
            (Some(name.clone()), Some(amount.clone()), Some(unit.clone()))
        } else {
            (None, None, None)
        };
    Some(Activity {
        id: record.id,
        child_id: record.child_id?,
        kind: record.record_type.clone(),
        start_utc_ms: i64::try_from(*start).ok()?,
        offset_minutes: i16::try_from(*offset).ok()?,
        end_utc_ms: record.field(2).and_then(|field| {
            let Value::Array(parts) = &field.value else {
                return None;
            };
            let [Value::Integer(end), Value::Integer(_)] = parts.as_slice() else {
                return None;
            };
            i64::try_from(*end).ok()
        }),
        sleep_place: if record.record_type == "sleep" {
            record.field(100).and_then(|field| match field.value {
                Value::Integer(value) => u8::try_from(value).ok(),
                _ => None,
            })
        } else {
            None
        },
        note: match record.field(4).map(|field| &field.value) {
            Some(Value::Text(note)) => Some(note.clone()),
            _ => None,
        },
        diaper_kind,
        bottle_ml,
        bottle_entered,
        bottle_unit,
        bottle_content,
        breast_side,
        breast_segments,
        solids_foods,
        solids_amount,
        pump_left_ml: if record.record_type == "pump" {
            pump_measure(100)
        } else {
            None
        },
        pump_right_ml: if record.record_type == "pump" {
            pump_measure(101)
        } else {
            None
        },
        pump_total_ml: if record.record_type == "pump" {
            pump_measure(102)
        } else {
            None
        },
        growth_weight_g: growth_weight.as_ref().map(|measure| measure.0),
        growth_weight_entered: growth_weight.as_ref().and_then(|measure| measure.1.clone()),
        growth_weight_unit: growth_weight.as_ref().and_then(|measure| measure.2),
        growth_length_mm: growth_length.as_ref().map(|measure| measure.0),
        growth_length_entered: growth_length.as_ref().and_then(|measure| measure.1.clone()),
        growth_length_unit: growth_length.as_ref().and_then(|measure| measure.2),
        growth_head_mm: growth_head.as_ref().map(|measure| measure.0),
        growth_head_entered: growth_head.as_ref().and_then(|measure| measure.1.clone()),
        growth_head_unit: growth_head.as_ref().and_then(|measure| measure.2),
        temperature_c,
        temperature_entered,
        temperature_unit,
        medication_name,
        medication_dose_amount,
        medication_dose_unit,
    })
}

fn check_time(now_ms: i64) -> Result<(), Error> {
    if now_ms < 0 || (now_ms as u64) >= (1u64 << 48) {
        return Err(Error::Invalid("time outside UUIDv7 range"));
    }
    Ok(())
}

fn placeholder_hlc(family: FamilyHandle) -> Hlc {
    Hlc {
        wall_ms: 0,
        counter: 0,
        device_id: family.device_id,
    }
}
