//! Offline local tracking API used by native platform bindings. Every action
//! names its Family and child; UI selection never changes a saved target.

use std::path::Path;

use crate::{
    ids,
    operation::{Hlc, NewOperation, Scope},
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

pub use crate::read_model::{Activity, Child, activities_from_records, children_from_records};

pub use crate::day_summary::{DaySummary, DayWindow};
pub use crate::event_actions::{ActivityTime, GrowthInput, MeasurementInput, PumpAmounts};

/// Compatibility adapter for native callers; calculation is portable.
pub fn summarize_day(
    activities: impl IntoIterator<Item = Activity>,
    child_id: [u8; 16],
    window: DayWindow,
) -> Result<DaySummary, Error> {
    crate::day_summary::summarize_day(activities, child_id, window).map_err(Error::Invalid)
}

pub use crate::breast::Segment as BreastSegment;

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

    pub fn restore_activity(
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
        let operation = restore_activity_operation(family, child_id, activity, saved_at_ms)?;
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

    pub fn edit_instant_time(
        &mut self,
        family: FamilyHandle,
        child_id: [u8; 16],
        activity_id: [u8; 16],
        start_utc_ms: i64,
        offset_minutes: i16,
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
        let operation = edit_instant_time_operation(
            family,
            child_id,
            activity,
            start_utc_ms,
            offset_minutes,
            saved_at_ms,
        )?;
        self.store.append_local(family, operation, saved_at_ms)?;
        Ok(())
    }

    pub fn move_completed_interval(
        &mut self,
        family: FamilyHandle,
        child_id: [u8; 16],
        activity_id: [u8; 16],
        time: ActivityTime,
        end_utc_ms: i64,
        end_offset_minutes: i16,
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
        let operation = move_completed_interval_operation(
            family,
            child_id,
            activity,
            time,
            end_utc_ms,
            end_offset_minutes,
        )?;
        self.store
            .append_local(family, operation, time.saved_at_ms)?;
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

    pub fn day_summary(
        &self,
        family: FamilyHandle,
        child_id: [u8; 16],
        window: DayWindow,
    ) -> Result<DaySummary, Error> {
        summarize_day(self.timeline(family, child_id)?, child_id, window)
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
    let identity = action_identity(family, None, now_ms)?;
    let id = identity.record;
    let operation =
        crate::event_actions::child(identity, name, birth_day, sex).map_err(Error::Invalid)?;
    Ok((id, operation))
}

pub fn rename_child_operation(
    family: FamilyHandle,
    child: &Record,
    name: &str,
    saved_at_ms: i64,
) -> Result<NewOperation, Error> {
    let identity = action_identity(family, Some(child.id), saved_at_ms)?;
    crate::event_actions::rename_child_operation(identity, child, name, saved_at_ms)
        .map_err(Error::Invalid)
}

pub fn edit_child_metadata_operation(
    family: FamilyHandle,
    child: &Record,
    birth_day: Option<i64>,
    sex: Option<u8>,
    saved_at_ms: i64,
) -> Result<NewOperation, Error> {
    let identity = action_identity(family, Some(child.id), saved_at_ms)?;
    crate::event_actions::edit_child_metadata_operation(
        identity,
        child,
        birth_day,
        sex,
        saved_at_ms,
    )
    .map_err(Error::Invalid)
}

pub fn diaper_operation(
    family: FamilyHandle,
    child_id: [u8; 16],
    kind: u8,
    time: ActivityTime,
) -> Result<([u8; 16], NewOperation), Error> {
    let identity = action_identity(family, None, time.saved_at_ms)?;
    let id = identity.record;
    let operation = crate::event_actions::diaper(
        identity,
        child_id,
        kind,
        time.start_utc_ms,
        time.offset_minutes,
    )
    .map_err(Error::Invalid)?;
    Ok((id, operation))
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

pub fn bottle_entered_operation(
    family: FamilyHandle,
    child_id: [u8; 16],
    entered: &str,
    unit: u8,
    content: u8,
    time: ActivityTime,
) -> Result<([u8; 16], NewOperation), Error> {
    let identity = action_identity(family, None, time.saved_at_ms)?;
    let id = identity.record;
    let operation = crate::event_actions::bottle(
        identity,
        child_id,
        entered,
        unit,
        content,
        time.start_utc_ms,
        time.offset_minutes,
    )
    .map_err(Error::Invalid)?;
    Ok((id, operation))
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
    let identity = action_identity(family, None, time.saved_at_ms)?;
    let id = identity.record;
    let operation = crate::event_actions::breast(
        identity,
        child_id,
        segments,
        time.start_utc_ms,
        time.offset_minutes,
        time.saved_at_ms,
    )
    .map_err(Error::Invalid)?;
    Ok((id, operation))
}

pub fn edit_breast_feed_segments_operation(
    family: FamilyHandle,
    child_id: [u8; 16],
    activity: &Record,
    segments: &[BreastSegment],
    saved_at_ms: i64,
) -> Result<NewOperation, Error> {
    let identity = action_identity(family, Some(activity.id), saved_at_ms)?;
    crate::event_actions::edit_breast(identity, child_id, activity, segments, saved_at_ms)
        .map_err(Error::Invalid)
}

pub fn pump_operation(
    family: FamilyHandle,
    child_id: [u8; 16],
    amounts: PumpAmounts,
    time: ActivityTime,
    end_utc_ms: i64,
) -> Result<([u8; 16], NewOperation), Error> {
    let identity = action_identity(family, None, time.saved_at_ms)?;
    crate::event_actions::pump_operation(identity, child_id, amounts, time, end_utc_ms)
        .map_err(Error::Invalid)
}

pub fn edit_pump_amounts_operation(
    family: FamilyHandle,
    child_id: [u8; 16],
    activity: &Record,
    amounts: PumpAmounts,
    saved_at_ms: i64,
) -> Result<NewOperation, Error> {
    let identity = action_identity(family, Some(activity.id), saved_at_ms)?;
    crate::event_actions::edit_pump_amounts_operation(
        identity,
        child_id,
        activity,
        amounts,
        saved_at_ms,
    )
    .map_err(Error::Invalid)
}

pub fn solids_operation(
    family: FamilyHandle,
    child_id: [u8; 16],
    foods: &[String],
    amount: &str,
    time: ActivityTime,
) -> Result<([u8; 16], NewOperation), Error> {
    let identity = action_identity(family, None, time.saved_at_ms)?;
    crate::event_actions::solids_operation(identity, child_id, foods, amount, time)
        .map_err(Error::Invalid)
}

pub fn edit_solids_operation(
    family: FamilyHandle,
    child_id: [u8; 16],
    activity: &Record,
    foods: &[String],
    amount: &str,
    saved_at_ms: i64,
) -> Result<NewOperation, Error> {
    let identity = action_identity(family, Some(activity.id), saved_at_ms)?;
    crate::event_actions::edit_solids_operation(
        identity,
        child_id,
        activity,
        foods,
        amount,
        saved_at_ms,
    )
    .map_err(Error::Invalid)
}

pub fn sleep_operation(
    family: FamilyHandle,
    child_id: [u8; 16],
    time: ActivityTime,
    end_utc_ms: i64,
    end_offset_minutes: i16,
) -> Result<([u8; 16], NewOperation), Error> {
    let identity = action_identity(family, None, time.saved_at_ms)?;
    crate::event_actions::sleep_operation(identity, child_id, time, end_utc_ms, end_offset_minutes)
        .map_err(Error::Invalid)
}

pub fn sleep_operation_with_place(
    family: FamilyHandle,
    child_id: [u8; 16],
    time: ActivityTime,
    end_utc_ms: i64,
    end_offset_minutes: i16,
    place: Option<u8>,
) -> Result<([u8; 16], NewOperation), Error> {
    let identity = action_identity(family, None, time.saved_at_ms)?;
    crate::event_actions::sleep_operation_with_place(
        identity,
        child_id,
        time,
        end_utc_ms,
        end_offset_minutes,
        place,
    )
    .map_err(Error::Invalid)
}

pub fn running_sleep_operation(
    family: FamilyHandle,
    child_id: [u8; 16],
    time: ActivityTime,
) -> Result<([u8; 16], NewOperation), Error> {
    let identity = action_identity(family, None, time.saved_at_ms)?;
    crate::event_actions::running_sleep_operation(identity, child_id, time).map_err(Error::Invalid)
}

pub fn running_sleep_operation_with_place(
    family: FamilyHandle,
    child_id: [u8; 16],
    time: ActivityTime,
    place: Option<u8>,
) -> Result<([u8; 16], NewOperation), Error> {
    let identity = action_identity(family, None, time.saved_at_ms)?;
    crate::event_actions::running_sleep_operation_with_place(identity, child_id, time, place)
        .map_err(Error::Invalid)
}

pub fn edit_sleep_place_operation(
    family: FamilyHandle,
    child_id: [u8; 16],
    activity: &Record,
    place: Option<u8>,
    saved_at_ms: i64,
) -> Result<NewOperation, Error> {
    let identity = action_identity(family, Some(activity.id), saved_at_ms)?;
    crate::event_actions::edit_sleep_place_operation(
        identity,
        child_id,
        activity,
        place,
        saved_at_ms,
    )
    .map_err(Error::Invalid)
}

pub fn stop_sleep_operation(
    family: FamilyHandle,
    child_id: [u8; 16],
    activity: &Record,
    end_utc_ms: i64,
    end_offset_minutes: i16,
    saved_at_ms: i64,
) -> Result<NewOperation, Error> {
    let identity = action_identity(family, Some(activity.id), saved_at_ms)?;
    crate::event_actions::stop_sleep_operation(
        identity,
        child_id,
        activity,
        end_utc_ms,
        end_offset_minutes,
        saved_at_ms,
    )
    .map_err(Error::Invalid)
}

pub fn edit_sleep_end_operation(
    family: FamilyHandle,
    child_id: [u8; 16],
    activity: &Record,
    end_utc_ms: i64,
    end_offset_minutes: i16,
    saved_at_ms: i64,
) -> Result<NewOperation, Error> {
    let identity = action_identity(family, Some(activity.id), saved_at_ms)?;
    crate::event_actions::edit_sleep_end_operation(
        identity,
        child_id,
        activity,
        end_utc_ms,
        end_offset_minutes,
        saved_at_ms,
    )
    .map_err(Error::Invalid)
}

pub fn delete_activity_operation(
    family: FamilyHandle,
    child_id: [u8; 16],
    activity: &Record,
    saved_at_ms: i64,
) -> Result<NewOperation, Error> {
    let identity = action_identity(family, Some(activity.id), saved_at_ms)?;
    crate::event_actions::delete_activity_operation(identity, child_id, activity, saved_at_ms)
        .map_err(Error::Invalid)
}

pub fn restore_activity_operation(
    family: FamilyHandle,
    child_id: [u8; 16],
    activity: &Record,
    saved_at_ms: i64,
) -> Result<NewOperation, Error> {
    let identity = action_identity(family, Some(activity.id), saved_at_ms)?;
    crate::event_actions::restore_activity_operation(identity, child_id, activity, saved_at_ms)
        .map_err(Error::Invalid)
}

pub fn note_operation(
    family: FamilyHandle,
    child_id: [u8; 16],
    note: &str,
    time: ActivityTime,
) -> Result<([u8; 16], NewOperation), Error> {
    let identity = action_identity(family, None, time.saved_at_ms)?;
    let id = identity.record;
    let operation = crate::event_actions::note(
        identity,
        child_id,
        note,
        time.start_utc_ms,
        time.offset_minutes,
    )
    .map_err(Error::Invalid)?;
    Ok((id, operation))
}

pub fn edit_note_operation(
    family: FamilyHandle,
    child_id: [u8; 16],
    activity: &Record,
    note: &str,
    saved_at_ms: i64,
) -> Result<NewOperation, Error> {
    let identity = action_identity(family, Some(activity.id), saved_at_ms)?;
    crate::event_actions::edit_note_operation(identity, child_id, activity, note, saved_at_ms)
        .map_err(Error::Invalid)
}

/// Correct the recorded start of an instantaneous entry. Interval activities
/// keep their own start/end and segment editing rules.
pub fn edit_instant_time_operation(
    family: FamilyHandle,
    child_id: [u8; 16],
    activity: &Record,
    start_utc_ms: i64,
    offset_minutes: i16,
    saved_at_ms: i64,
) -> Result<NewOperation, Error> {
    let identity = action_identity(family, Some(activity.id), saved_at_ms)?;
    crate::event_actions::edit_instant_time_operation(
        identity,
        child_id,
        activity,
        start_utc_ms,
        offset_minutes,
        saved_at_ms,
    )
    .map_err(Error::Invalid)
}

/// Move one completed sleep or pump interval in a single field-set operation.
/// The caller chooses both endpoint offsets; Android preserves the duration.
pub fn move_completed_interval_operation(
    family: FamilyHandle,
    child_id: [u8; 16],
    activity: &Record,
    time: ActivityTime,
    end_utc_ms: i64,
    end_offset_minutes: i16,
) -> Result<NewOperation, Error> {
    let identity = action_identity(family, Some(activity.id), time.saved_at_ms)?;
    crate::event_actions::move_completed_interval_operation(
        identity,
        child_id,
        activity,
        time,
        end_utc_ms,
        end_offset_minutes,
    )
    .map_err(Error::Invalid)
}

pub fn edit_bottle_ml_operation(
    family: FamilyHandle,
    child_id: [u8; 16],
    activity: &Record,
    amount_ml: i64,
    saved_at_ms: i64,
) -> Result<NewOperation, Error> {
    let identity = action_identity(family, Some(activity.id), saved_at_ms)?;
    crate::event_actions::edit_bottle_ml_operation(
        identity,
        child_id,
        activity,
        amount_ml,
        saved_at_ms,
    )
    .map_err(Error::Invalid)
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
    let identity = action_identity(family, Some(activity.id), saved_at_ms)?;
    crate::event_actions::edit_bottle_entered_operation(
        identity,
        child_id,
        activity,
        entered,
        unit,
        content,
        saved_at_ms,
    )
    .map_err(Error::Invalid)
}

pub fn edit_bottle_operation(
    family: FamilyHandle,
    child_id: [u8; 16],
    activity: &Record,
    amount_ml: i64,
    content: u8,
    saved_at_ms: i64,
) -> Result<NewOperation, Error> {
    let identity = action_identity(family, Some(activity.id), saved_at_ms)?;
    crate::event_actions::edit_bottle_operation(
        identity,
        child_id,
        activity,
        amount_ml,
        content,
        saved_at_ms,
    )
    .map_err(Error::Invalid)
}

pub fn edit_diaper_kind_operation(
    family: FamilyHandle,
    child_id: [u8; 16],
    activity: &Record,
    kind: u8,
    saved_at_ms: i64,
) -> Result<NewOperation, Error> {
    let identity = action_identity(family, Some(activity.id), saved_at_ms)?;
    crate::event_actions::edit_diaper_kind_operation(
        identity,
        child_id,
        activity,
        kind,
        saved_at_ms,
    )
    .map_err(Error::Invalid)
}

pub fn growth_operation(
    family: FamilyHandle,
    child_id: [u8; 16],
    weight_g: Option<i64>,
    length_mm: Option<i64>,
    time: ActivityTime,
) -> Result<([u8; 16], NewOperation), Error> {
    let identity = action_identity(family, None, time.saved_at_ms)?;
    crate::event_actions::growth_operation(identity, child_id, weight_g, length_mm, time)
        .map_err(Error::Invalid)
}

pub fn growth_measurements_operation(
    family: FamilyHandle,
    child_id: [u8; 16],
    weight_g: Option<i64>,
    length_mm: Option<i64>,
    head_mm: Option<i64>,
    time: ActivityTime,
) -> Result<([u8; 16], NewOperation), Error> {
    let identity = action_identity(family, None, time.saved_at_ms)?;
    crate::event_actions::growth_measurements_operation(
        identity, child_id, weight_g, length_mm, head_mm, time,
    )
    .map_err(Error::Invalid)
}

pub fn growth_entered_operation(
    family: FamilyHandle,
    child_id: [u8; 16],
    input: &GrowthInput,
    time: ActivityTime,
) -> Result<([u8; 16], NewOperation), Error> {
    let identity = action_identity(family, None, time.saved_at_ms)?;
    crate::event_actions::growth_entered_operation(identity, child_id, input, time)
        .map_err(Error::Invalid)
}

pub fn edit_growth_entered_operation(
    family: FamilyHandle,
    child_id: [u8; 16],
    activity: &Record,
    input: &GrowthInput,
    saved_at_ms: i64,
) -> Result<NewOperation, Error> {
    let identity = action_identity(family, Some(activity.id), saved_at_ms)?;
    crate::event_actions::edit_growth_entered_operation(
        identity,
        child_id,
        activity,
        input,
        saved_at_ms,
    )
    .map_err(Error::Invalid)
}

pub fn edit_growth_operation(
    family: FamilyHandle,
    child_id: [u8; 16],
    activity: &Record,
    weight_g: Option<i64>,
    length_mm: Option<i64>,
    saved_at_ms: i64,
) -> Result<NewOperation, Error> {
    let identity = action_identity(family, Some(activity.id), saved_at_ms)?;
    crate::event_actions::edit_growth_operation(
        identity,
        child_id,
        activity,
        weight_g,
        length_mm,
        saved_at_ms,
    )
    .map_err(Error::Invalid)
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
    let identity = action_identity(family, Some(activity.id), saved_at_ms)?;
    crate::event_actions::edit_growth_measurements_operation(
        identity,
        child_id,
        activity,
        weight_g,
        length_mm,
        head_mm,
        saved_at_ms,
    )
    .map_err(Error::Invalid)
}

pub fn temperature_c_operation(
    family: FamilyHandle,
    child_id: [u8; 16],
    entered_c: &str,
    time: ActivityTime,
) -> Result<([u8; 16], NewOperation), Error> {
    let identity = action_identity(family, None, time.saved_at_ms)?;
    crate::event_actions::temperature_c_operation(identity, child_id, entered_c, time)
        .map_err(Error::Invalid)
}

pub fn temperature_entered_operation(
    family: FamilyHandle,
    child_id: [u8; 16],
    entered: &str,
    unit: u8,
    time: ActivityTime,
) -> Result<([u8; 16], NewOperation), Error> {
    let identity = action_identity(family, None, time.saved_at_ms)?;
    crate::event_actions::temperature_entered_operation(identity, child_id, entered, unit, time)
        .map_err(Error::Invalid)
}

pub fn edit_temperature_c_operation(
    family: FamilyHandle,
    child_id: [u8; 16],
    activity: &Record,
    entered_c: &str,
    saved_at_ms: i64,
) -> Result<NewOperation, Error> {
    let identity = action_identity(family, Some(activity.id), saved_at_ms)?;
    crate::event_actions::edit_temperature_c_operation(
        identity,
        child_id,
        activity,
        entered_c,
        saved_at_ms,
    )
    .map_err(Error::Invalid)
}

pub fn edit_temperature_entered_operation(
    family: FamilyHandle,
    child_id: [u8; 16],
    activity: &Record,
    entered: &str,
    unit: u8,
    saved_at_ms: i64,
) -> Result<NewOperation, Error> {
    let identity = action_identity(family, Some(activity.id), saved_at_ms)?;
    crate::event_actions::edit_temperature_entered_operation(
        identity,
        child_id,
        activity,
        entered,
        unit,
        saved_at_ms,
    )
    .map_err(Error::Invalid)
}

pub fn medication_operation(
    family: FamilyHandle,
    child_id: [u8; 16],
    name: &str,
    dose_amount: &str,
    dose_unit: &str,
    time: ActivityTime,
) -> Result<([u8; 16], NewOperation), Error> {
    let identity = action_identity(family, None, time.saved_at_ms)?;
    crate::event_actions::medication_operation(
        identity,
        child_id,
        name,
        dose_amount,
        dose_unit,
        time,
    )
    .map_err(Error::Invalid)
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
    let identity = action_identity(family, Some(activity.id), saved_at_ms)?;
    crate::event_actions::edit_medication_operation(
        identity,
        child_id,
        activity,
        name,
        dose_amount,
        dose_unit,
        saved_at_ms,
    )
    .map_err(Error::Invalid)
}

fn check_time(now_ms: i64) -> Result<(), Error> {
    crate::event_actions::check_time(now_ms).map_err(Error::Invalid)
}

fn placeholder_hlc(family: FamilyHandle) -> Hlc {
    Hlc {
        wall_ms: 0,
        counter: 0,
        device_id: family.device_id,
    }
}

fn action_identity(
    family: FamilyHandle,
    record: Option<[u8; 16]>,
    saved_at_ms: i64,
) -> Result<crate::event_actions::Identity, Error> {
    check_time(saved_at_ms)?;
    Ok(crate::event_actions::Identity {
        family: family.family_id,
        device: family.device_id,
        record: match record {
            Some(id) => id,
            None => ids::random_v7(saved_at_ms)?,
        },
        operation: ids::random_v7(saved_at_ms)?,
        stamp: placeholder_hlc(family),
    })
}
