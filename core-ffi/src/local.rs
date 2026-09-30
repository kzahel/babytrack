//! Offline native store adapter.

use super::*;

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

    pub fn rename_child(
        &self,
        family: FamilyRef,
        child_id: Vec<u8>,
        name: String,
        saved_at_ms: i64,
    ) -> Result<(), BindingError> {
        self.repo
            .lock()
            .map_err(|_| BindingError::LockPoisoned)?
            .rename_child(family.handle()?, fixed(&child_id)?, &name, saved_at_ms)
            .map_err(rejected)
    }

    pub fn edit_child_metadata(
        &self,
        family: FamilyRef,
        child_id: Vec<u8>,
        birth_day: Option<i64>,
        sex: Option<u8>,
        saved_at_ms: i64,
    ) -> Result<(), BindingError> {
        self.repo
            .lock()
            .map_err(|_| BindingError::LockPoisoned)?
            .edit_child_metadata(
                family.handle()?,
                fixed(&child_id)?,
                birth_day,
                sex,
                saved_at_ms,
            )
            .map_err(rejected)
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

    #[allow(clippy::too_many_arguments)]
    pub fn log_bottle_entered(
        &self,
        family: FamilyRef,
        child_id: Vec<u8>,
        entered: String,
        unit: u8,
        content: u8,
        time: ActivityWhen,
    ) -> Result<Vec<u8>, BindingError> {
        Ok(self
            .repo
            .lock()
            .map_err(|_| BindingError::LockPoisoned)?
            .log_bottle_entered(
                family.handle()?,
                fixed(&child_id)?,
                &entered,
                unit,
                content,
                time.into(),
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

    pub fn edit_breast_feed_segments(
        &self,
        family: FamilyRef,
        child_id: Vec<u8>,
        activity_id: Vec<u8>,
        segments: Vec<BreastSegmentRow>,
        saved_at_ms: i64,
    ) -> Result<(), BindingError> {
        self.repo
            .lock()
            .map_err(|_| BindingError::LockPoisoned)?
            .edit_breast_feed_segments(
                family.handle()?,
                fixed(&child_id)?,
                fixed(&activity_id)?,
                segments.into_iter().map(Into::into).collect(),
                saved_at_ms,
            )
            .map_err(rejected)
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
        self.log_sleep_with_place(family, child_id, time, end_utc_ms, end_offset_minutes, None)
    }

    pub fn log_sleep_with_place(
        &self,
        family: FamilyRef,
        child_id: Vec<u8>,
        time: ActivityWhen,
        end_utc_ms: i64,
        end_offset_minutes: i16,
        place: Option<u8>,
    ) -> Result<Vec<u8>, BindingError> {
        Ok(self
            .repo
            .lock()
            .map_err(|_| BindingError::LockPoisoned)?
            .log_sleep_with_place(
                family.handle()?,
                fixed(&child_id)?,
                time.into(),
                end_utc_ms,
                end_offset_minutes,
                place,
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
        self.start_sleep_with_place(family, child_id, time, None)
    }

    pub fn start_sleep_with_place(
        &self,
        family: FamilyRef,
        child_id: Vec<u8>,
        time: ActivityWhen,
        place: Option<u8>,
    ) -> Result<Vec<u8>, BindingError> {
        Ok(self
            .repo
            .lock()
            .map_err(|_| BindingError::LockPoisoned)?
            .start_sleep_with_place(family.handle()?, fixed(&child_id)?, time.into(), place)
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

    pub fn edit_sleep_end(
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
            .edit_sleep_end(
                family.handle()?,
                fixed(&child_id)?,
                fixed(&activity_id)?,
                end_utc_ms,
                end_offset_minutes,
                saved_at_ms,
            )
            .map_err(rejected)
    }

    pub fn edit_sleep_place(
        &self,
        family: FamilyRef,
        child_id: Vec<u8>,
        activity_id: Vec<u8>,
        place: Option<u8>,
        saved_at_ms: i64,
    ) -> Result<(), BindingError> {
        self.repo
            .lock()
            .map_err(|_| BindingError::LockPoisoned)?
            .edit_sleep_place(
                family.handle()?,
                fixed(&child_id)?,
                fixed(&activity_id)?,
                place,
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

    pub fn restore_activity(
        &self,
        family: FamilyRef,
        child_id: Vec<u8>,
        activity_id: Vec<u8>,
        saved_at_ms: i64,
    ) -> Result<(), BindingError> {
        self.repo
            .lock()
            .map_err(|_| BindingError::LockPoisoned)?
            .restore_activity(
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

    pub fn edit_instant_time(
        &self,
        family: FamilyRef,
        child_id: Vec<u8>,
        activity_id: Vec<u8>,
        time: ActivityWhen,
    ) -> Result<(), BindingError> {
        self.repo
            .lock()
            .map_err(|_| BindingError::LockPoisoned)?
            .edit_instant_time(
                family.handle()?,
                fixed(&child_id)?,
                fixed(&activity_id)?,
                time.start_utc_ms,
                time.offset_minutes,
                time.saved_at_ms,
            )
            .map_err(rejected)
    }

    pub fn move_completed_interval(
        &self,
        family: FamilyRef,
        child_id: Vec<u8>,
        activity_id: Vec<u8>,
        time: ActivityWhen,
        end_utc_ms: i64,
        end_offset_minutes: i16,
    ) -> Result<(), BindingError> {
        self.repo
            .lock()
            .map_err(|_| BindingError::LockPoisoned)?
            .move_completed_interval(
                family.handle()?,
                fixed(&child_id)?,
                fixed(&activity_id)?,
                time.into(),
                end_utc_ms,
                end_offset_minutes,
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

    pub fn edit_bottle(
        &self,
        family: FamilyRef,
        child_id: Vec<u8>,
        activity_id: Vec<u8>,
        amount_ml: i64,
        content: u8,
        saved_at_ms: i64,
    ) -> Result<(), BindingError> {
        self.repo
            .lock()
            .map_err(|_| BindingError::LockPoisoned)?
            .edit_bottle(
                family.handle()?,
                fixed(&child_id)?,
                fixed(&activity_id)?,
                amount_ml,
                content,
                saved_at_ms,
            )
            .map_err(rejected)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn edit_bottle_entered(
        &self,
        family: FamilyRef,
        child_id: Vec<u8>,
        activity_id: Vec<u8>,
        entered: String,
        unit: u8,
        content: u8,
        saved_at_ms: i64,
    ) -> Result<(), BindingError> {
        self.repo
            .lock()
            .map_err(|_| BindingError::LockPoisoned)?
            .edit_bottle_entered(
                family.handle()?,
                fixed(&child_id)?,
                fixed(&activity_id)?,
                &entered,
                unit,
                content,
                saved_at_ms,
            )
            .map_err(rejected)
    }

    pub fn edit_diaper_kind(
        &self,
        family: FamilyRef,
        child_id: Vec<u8>,
        activity_id: Vec<u8>,
        kind: u8,
        saved_at_ms: i64,
    ) -> Result<(), BindingError> {
        self.repo
            .lock()
            .map_err(|_| BindingError::LockPoisoned)?
            .edit_diaper_kind(
                family.handle()?,
                fixed(&child_id)?,
                fixed(&activity_id)?,
                kind,
                saved_at_ms,
            )
            .map_err(rejected)
    }

    pub fn edit_solids(
        &self,
        family: FamilyRef,
        child_id: Vec<u8>,
        activity_id: Vec<u8>,
        foods: Vec<String>,
        amount: String,
        saved_at_ms: i64,
    ) -> Result<(), BindingError> {
        self.repo
            .lock()
            .map_err(|_| BindingError::LockPoisoned)?
            .edit_solids(
                family.handle()?,
                fixed(&child_id)?,
                fixed(&activity_id)?,
                &foods,
                &amount,
                saved_at_ms,
            )
            .map_err(rejected)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn edit_growth(
        &self,
        family: FamilyRef,
        child_id: Vec<u8>,
        activity_id: Vec<u8>,
        weight_g: Option<i64>,
        length_mm: Option<i64>,
        saved_at_ms: i64,
    ) -> Result<(), BindingError> {
        self.repo
            .lock()
            .map_err(|_| BindingError::LockPoisoned)?
            .edit_growth(
                family.handle()?,
                fixed(&child_id)?,
                fixed(&activity_id)?,
                weight_g,
                length_mm,
                saved_at_ms,
            )
            .map_err(rejected)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn edit_growth_measurements(
        &self,
        family: FamilyRef,
        child_id: Vec<u8>,
        activity_id: Vec<u8>,
        weight_g: Option<i64>,
        length_mm: Option<i64>,
        head_mm: Option<i64>,
        saved_at_ms: i64,
    ) -> Result<(), BindingError> {
        self.repo
            .lock()
            .map_err(|_| BindingError::LockPoisoned)?
            .edit_growth_measurements(
                family.handle()?,
                fixed(&child_id)?,
                fixed(&activity_id)?,
                weight_g,
                length_mm,
                head_mm,
                saved_at_ms,
            )
            .map_err(rejected)
    }

    pub fn edit_growth_entered(
        &self,
        family: FamilyRef,
        child_id: Vec<u8>,
        activity_id: Vec<u8>,
        input: GrowthInputRow,
        saved_at_ms: i64,
    ) -> Result<(), BindingError> {
        self.repo
            .lock()
            .map_err(|_| BindingError::LockPoisoned)?
            .edit_growth_entered(
                family.handle()?,
                fixed(&child_id)?,
                fixed(&activity_id)?,
                &input.into(),
                saved_at_ms,
            )
            .map_err(rejected)
    }

    pub fn edit_pump_amounts(
        &self,
        family: FamilyRef,
        child_id: Vec<u8>,
        activity_id: Vec<u8>,
        input: PumpInput,
        saved_at_ms: i64,
    ) -> Result<(), BindingError> {
        self.repo
            .lock()
            .map_err(|_| BindingError::LockPoisoned)?
            .edit_pump_amounts(
                family.handle()?,
                fixed(&child_id)?,
                fixed(&activity_id)?,
                input.into(),
                saved_at_ms,
            )
            .map_err(rejected)
    }

    pub fn edit_medication(
        &self,
        family: FamilyRef,
        child_id: Vec<u8>,
        activity_id: Vec<u8>,
        input: MedicationInput,
        saved_at_ms: i64,
    ) -> Result<(), BindingError> {
        self.repo
            .lock()
            .map_err(|_| BindingError::LockPoisoned)?
            .edit_medication(
                family.handle()?,
                fixed(&child_id)?,
                fixed(&activity_id)?,
                &input.name,
                &input.dose_amount,
                &input.dose_unit,
                saved_at_ms,
            )
            .map_err(rejected)
    }

    pub fn edit_temperature_c(
        &self,
        family: FamilyRef,
        child_id: Vec<u8>,
        activity_id: Vec<u8>,
        entered_c: String,
        saved_at_ms: i64,
    ) -> Result<(), BindingError> {
        self.repo
            .lock()
            .map_err(|_| BindingError::LockPoisoned)?
            .edit_temperature_c(
                family.handle()?,
                fixed(&child_id)?,
                fixed(&activity_id)?,
                &entered_c,
                saved_at_ms,
            )
            .map_err(rejected)
    }

    pub fn edit_temperature_entered(
        &self,
        family: FamilyRef,
        child_id: Vec<u8>,
        activity_id: Vec<u8>,
        entered: String,
        unit: u8,
        saved_at_ms: i64,
    ) -> Result<(), BindingError> {
        self.repo
            .lock()
            .map_err(|_| BindingError::LockPoisoned)?
            .edit_temperature_entered(
                family.handle()?,
                fixed(&child_id)?,
                fixed(&activity_id)?,
                &entered,
                unit,
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

    pub fn log_growth_measurements(
        &self,
        family: FamilyRef,
        child_id: Vec<u8>,
        weight_g: Option<i64>,
        length_mm: Option<i64>,
        head_mm: Option<i64>,
        time: ActivityWhen,
    ) -> Result<Vec<u8>, BindingError> {
        Ok(self
            .repo
            .lock()
            .map_err(|_| BindingError::LockPoisoned)?
            .log_growth_measurements(
                family.handle()?,
                fixed(&child_id)?,
                weight_g,
                length_mm,
                head_mm,
                time.into(),
            )
            .map_err(rejected)?
            .to_vec())
    }

    pub fn log_growth_entered(
        &self,
        family: FamilyRef,
        child_id: Vec<u8>,
        input: GrowthInputRow,
        time: ActivityWhen,
    ) -> Result<Vec<u8>, BindingError> {
        Ok(self
            .repo
            .lock()
            .map_err(|_| BindingError::LockPoisoned)?
            .log_growth_entered(
                family.handle()?,
                fixed(&child_id)?,
                &input.into(),
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

    pub fn log_temperature_entered(
        &self,
        family: FamilyRef,
        child_id: Vec<u8>,
        entered: String,
        unit: u8,
        time: ActivityWhen,
    ) -> Result<Vec<u8>, BindingError> {
        Ok(self
            .repo
            .lock()
            .map_err(|_| BindingError::LockPoisoned)?
            .log_temperature_entered(
                family.handle()?,
                fixed(&child_id)?,
                &entered,
                unit,
                time.into(),
            )
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
                sleep_place: row.sleep_place,
                note: row.note,
                diaper_kind: row.diaper_kind,
                bottle_ml: row.bottle_ml,
                bottle_entered: row.bottle_entered,
                bottle_unit: row.bottle_unit,
                bottle_content: row.bottle_content,
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
                growth_weight_entered: row.growth_weight_entered,
                growth_weight_unit: row.growth_weight_unit,
                growth_length_mm: row.growth_length_mm,
                growth_length_entered: row.growth_length_entered,
                growth_length_unit: row.growth_length_unit,
                growth_head_mm: row.growth_head_mm,
                growth_head_entered: row.growth_head_entered,
                growth_head_unit: row.growth_head_unit,
                temperature_c: row.temperature_c,
                temperature_entered: row.temperature_entered,
                temperature_unit: row.temperature_unit,
                medication_name: row.medication_name,
                medication_dose_amount: row.medication_dose_amount,
                medication_dose_unit: row.medication_dose_unit,
            })
            .collect())
    }

    pub fn day_summary(
        &self,
        family: FamilyRef,
        child_id: Vec<u8>,
        window: DayWindowRow,
    ) -> Result<DaySummaryRow, BindingError> {
        self.repo
            .lock()
            .map_err(|_| BindingError::LockPoisoned)?
            .day_summary(family.handle()?, fixed(&child_id)?, window.into())
            .map(Into::into)
            .map_err(rejected)
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
