//! Shared event actions over core-owned operation builders.

use super::*;

#[uniffi::export]
impl NativeSharedStore {
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

    pub fn rename_shared_child(
        &self,
        family: FamilyRef,
        wrapping_key: Vec<u8>,
        child_id: Vec<u8>,
        name: String,
        saved_at_ms: i64,
    ) -> Result<(), BindingError> {
        let mut store = self.store.lock().map_err(|_| BindingError::LockPoisoned)?;
        let handle = family.handle()?;
        let ready = ready_session_for(&mut store, handle, &fixed(&wrapping_key)?)?;
        let projection = ready.projection_with_pending(&store).map_err(rejected)?;
        let child = projection
            .record(&fixed(&child_id)?)
            .ok_or(BindingError::InvalidBytes)?;
        let operation = local_api::rename_child_operation(handle, child, &name, saved_at_ms)
            .map_err(rejected)?;
        ready
            .append_local(&mut store, operation, saved_at_ms)
            .map(|_| ())
            .map_err(rejected)
    }

    pub fn edit_shared_child_metadata(
        &self,
        family: FamilyRef,
        wrapping_key: Vec<u8>,
        child_id: Vec<u8>,
        birth_day: Option<i64>,
        sex: Option<u8>,
        saved_at_ms: i64,
    ) -> Result<(), BindingError> {
        let mut store = self.store.lock().map_err(|_| BindingError::LockPoisoned)?;
        let handle = family.handle()?;
        let ready = ready_session_for(&mut store, handle, &fixed(&wrapping_key)?)?;
        let projection = ready.projection_with_pending(&store).map_err(rejected)?;
        let child = projection
            .record(&fixed(&child_id)?)
            .ok_or(BindingError::InvalidBytes)?;
        let operation =
            local_api::edit_child_metadata_operation(handle, child, birth_day, sex, saved_at_ms)
                .map_err(rejected)?;
        ready
            .append_local(&mut store, operation, saved_at_ms)
            .map(|_| ())
            .map_err(rejected)
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

    #[allow(clippy::too_many_arguments)]
    pub fn log_shared_bottle_entered(
        &self,
        family: FamilyRef,
        wrapping_key: Vec<u8>,
        child_id: Vec<u8>,
        entered: String,
        unit: u8,
        content: u8,
        time: ActivityWhen,
    ) -> Result<Vec<u8>, BindingError> {
        let mut store = self.store.lock().map_err(|_| BindingError::LockPoisoned)?;
        let handle = family.handle()?;
        let ready = ready_session_for(&mut store, handle, &fixed(&wrapping_key)?)?;
        let saved_at_ms = time.saved_at_ms;
        let (id, operation) = local_api::bottle_entered_operation(
            handle,
            fixed(&child_id)?,
            &entered,
            unit,
            content,
            time.into(),
        )
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

    pub fn edit_shared_breast_feed_segments(
        &self,
        family: FamilyRef,
        wrapping_key: Vec<u8>,
        child_id: Vec<u8>,
        activity_id: Vec<u8>,
        segments: Vec<BreastSegmentRow>,
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
        let segments = segments.into_iter().map(Into::into).collect::<Vec<_>>();
        let operation = local_api::edit_breast_feed_segments_operation(
            handle,
            child_id,
            activity,
            &segments,
            saved_at_ms,
        )
        .map_err(rejected)?;
        ready
            .append_local(&mut store, operation, saved_at_ms)
            .map(|_| ())
            .map_err(rejected)
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
        self.log_shared_sleep_with_place(
            family,
            wrapping_key,
            child_id,
            time,
            end_utc_ms,
            end_offset_minutes,
            None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn log_shared_sleep_with_place(
        &self,
        family: FamilyRef,
        wrapping_key: Vec<u8>,
        child_id: Vec<u8>,
        time: ActivityWhen,
        end_utc_ms: i64,
        end_offset_minutes: i16,
        place: Option<u8>,
    ) -> Result<Vec<u8>, BindingError> {
        let mut store = self.store.lock().map_err(|_| BindingError::LockPoisoned)?;
        let handle = family.handle()?;
        let ready = ready_session_for(&mut store, handle, &fixed(&wrapping_key)?)?;
        let saved_at_ms = time.saved_at_ms;
        let (id, operation) = local_api::sleep_operation_with_place(
            handle,
            fixed(&child_id)?,
            time.into(),
            end_utc_ms,
            end_offset_minutes,
            place,
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
        self.start_shared_sleep_with_place(family, wrapping_key, child_id, time, None)
    }

    pub fn start_shared_sleep_with_place(
        &self,
        family: FamilyRef,
        wrapping_key: Vec<u8>,
        child_id: Vec<u8>,
        time: ActivityWhen,
        place: Option<u8>,
    ) -> Result<Vec<u8>, BindingError> {
        let mut store = self.store.lock().map_err(|_| BindingError::LockPoisoned)?;
        let handle = family.handle()?;
        let ready = ready_session_for(&mut store, handle, &fixed(&wrapping_key)?)?;
        let saved_at_ms = time.saved_at_ms;
        let (id, operation) = local_api::running_sleep_operation_with_place(
            handle,
            fixed(&child_id)?,
            time.into(),
            place,
        )
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

    pub fn edit_shared_sleep_end(
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
        let activity = projection
            .record(&fixed(&activity_id)?)
            .ok_or(BindingError::InvalidBytes)?;
        let operation = local_api::edit_sleep_end_operation(
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

    pub fn edit_shared_sleep_place(
        &self,
        family: FamilyRef,
        wrapping_key: Vec<u8>,
        child_id: Vec<u8>,
        activity_id: Vec<u8>,
        place: Option<u8>,
        saved_at_ms: i64,
    ) -> Result<(), BindingError> {
        let mut store = self.store.lock().map_err(|_| BindingError::LockPoisoned)?;
        let handle = family.handle()?;
        let ready = ready_session_for(&mut store, handle, &fixed(&wrapping_key)?)?;
        let projection = ready.projection_with_pending(&store).map_err(rejected)?;
        let activity = projection
            .record(&fixed(&activity_id)?)
            .ok_or(BindingError::InvalidBytes)?;
        let operation = local_api::edit_sleep_place_operation(
            handle,
            fixed(&child_id)?,
            activity,
            place,
            saved_at_ms,
        )
        .map_err(rejected)?;
        ready
            .append_local(&mut store, operation, saved_at_ms)
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

    pub fn restore_shared_activity(
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
            local_api::restore_activity_operation(handle, child_id, activity, saved_at_ms)
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

    pub fn edit_shared_instant_time(
        &self,
        family: FamilyRef,
        wrapping_key: Vec<u8>,
        child_id: Vec<u8>,
        activity_id: Vec<u8>,
        time: ActivityWhen,
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
        let operation = local_api::edit_instant_time_operation(
            handle,
            child_id,
            activity,
            time.start_utc_ms,
            time.offset_minutes,
            time.saved_at_ms,
        )
        .map_err(rejected)?;
        ready
            .append_local(&mut store, operation, time.saved_at_ms)
            .map(|_| ())
            .map_err(rejected)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn move_shared_completed_interval(
        &self,
        family: FamilyRef,
        wrapping_key: Vec<u8>,
        child_id: Vec<u8>,
        activity_id: Vec<u8>,
        time: ActivityWhen,
        end_utc_ms: i64,
        end_offset_minutes: i16,
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
        let operation = local_api::move_completed_interval_operation(
            handle,
            child_id,
            activity,
            time.clone().into(),
            end_utc_ms,
            end_offset_minutes,
        )
        .map_err(rejected)?;
        ready
            .append_local(&mut store, operation, time.saved_at_ms)
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

    #[allow(clippy::too_many_arguments)]
    pub fn edit_shared_bottle(
        &self,
        family: FamilyRef,
        wrapping_key: Vec<u8>,
        child_id: Vec<u8>,
        activity_id: Vec<u8>,
        amount_ml: i64,
        content: u8,
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
        let operation = local_api::edit_bottle_operation(
            handle,
            child_id,
            activity,
            amount_ml,
            content,
            saved_at_ms,
        )
        .map_err(rejected)?;
        ready
            .append_local(&mut store, operation, saved_at_ms)
            .map(|_| ())
            .map_err(rejected)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn edit_shared_bottle_entered(
        &self,
        family: FamilyRef,
        wrapping_key: Vec<u8>,
        child_id: Vec<u8>,
        activity_id: Vec<u8>,
        entered: String,
        unit: u8,
        content: u8,
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
        let operation = local_api::edit_bottle_entered_operation(
            handle,
            child_id,
            activity,
            &entered,
            unit,
            content,
            saved_at_ms,
        )
        .map_err(rejected)?;
        ready
            .append_local(&mut store, operation, saved_at_ms)
            .map(|_| ())
            .map_err(rejected)
    }

    pub fn edit_shared_diaper_kind(
        &self,
        family: FamilyRef,
        wrapping_key: Vec<u8>,
        child_id: Vec<u8>,
        activity_id: Vec<u8>,
        kind: u8,
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
            local_api::edit_diaper_kind_operation(handle, child_id, activity, kind, saved_at_ms)
                .map_err(rejected)?;
        ready
            .append_local(&mut store, operation, saved_at_ms)
            .map(|_| ())
            .map_err(rejected)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn edit_shared_solids(
        &self,
        family: FamilyRef,
        wrapping_key: Vec<u8>,
        child_id: Vec<u8>,
        activity_id: Vec<u8>,
        foods: Vec<String>,
        amount: String,
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
        let operation = local_api::edit_solids_operation(
            handle,
            child_id,
            activity,
            &foods,
            &amount,
            saved_at_ms,
        )
        .map_err(rejected)?;
        ready
            .append_local(&mut store, operation, saved_at_ms)
            .map(|_| ())
            .map_err(rejected)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn edit_shared_growth(
        &self,
        family: FamilyRef,
        wrapping_key: Vec<u8>,
        child_id: Vec<u8>,
        activity_id: Vec<u8>,
        weight_g: Option<i64>,
        length_mm: Option<i64>,
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
        let operation = local_api::edit_growth_operation(
            handle,
            child_id,
            activity,
            weight_g,
            length_mm,
            saved_at_ms,
        )
        .map_err(rejected)?;
        ready
            .append_local(&mut store, operation, saved_at_ms)
            .map(|_| ())
            .map_err(rejected)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn edit_shared_growth_measurements(
        &self,
        family: FamilyRef,
        wrapping_key: Vec<u8>,
        child_id: Vec<u8>,
        activity_id: Vec<u8>,
        weight_g: Option<i64>,
        length_mm: Option<i64>,
        head_mm: Option<i64>,
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
        let operation = local_api::edit_growth_measurements_operation(
            handle,
            child_id,
            activity,
            weight_g,
            length_mm,
            head_mm,
            saved_at_ms,
        )
        .map_err(rejected)?;
        ready
            .append_local(&mut store, operation, saved_at_ms)
            .map(|_| ())
            .map_err(rejected)
    }

    pub fn edit_shared_growth_entered(
        &self,
        family: FamilyRef,
        wrapping_key: Vec<u8>,
        child_id: Vec<u8>,
        activity_id: Vec<u8>,
        input: GrowthInputRow,
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
        let operation = local_api::edit_growth_entered_operation(
            handle,
            child_id,
            activity,
            &input.into(),
            saved_at_ms,
        )
        .map_err(rejected)?;
        ready
            .append_local(&mut store, operation, saved_at_ms)
            .map(|_| ())
            .map_err(rejected)
    }

    pub fn edit_shared_pump_amounts(
        &self,
        family: FamilyRef,
        wrapping_key: Vec<u8>,
        child_id: Vec<u8>,
        activity_id: Vec<u8>,
        input: PumpInput,
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
        let operation = local_api::edit_pump_amounts_operation(
            handle,
            child_id,
            activity,
            input.into(),
            saved_at_ms,
        )
        .map_err(rejected)?;
        ready
            .append_local(&mut store, operation, saved_at_ms)
            .map(|_| ())
            .map_err(rejected)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn edit_shared_medication(
        &self,
        family: FamilyRef,
        wrapping_key: Vec<u8>,
        child_id: Vec<u8>,
        activity_id: Vec<u8>,
        input: MedicationInput,
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
        let operation = local_api::edit_medication_operation(
            handle,
            child_id,
            activity,
            &input.name,
            &input.dose_amount,
            &input.dose_unit,
            saved_at_ms,
        )
        .map_err(rejected)?;
        ready
            .append_local(&mut store, operation, saved_at_ms)
            .map(|_| ())
            .map_err(rejected)
    }

    pub fn edit_shared_temperature_c(
        &self,
        family: FamilyRef,
        wrapping_key: Vec<u8>,
        child_id: Vec<u8>,
        activity_id: Vec<u8>,
        entered_c: String,
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
        let operation = local_api::edit_temperature_c_operation(
            handle,
            child_id,
            activity,
            &entered_c,
            saved_at_ms,
        )
        .map_err(rejected)?;
        ready
            .append_local(&mut store, operation, saved_at_ms)
            .map(|_| ())
            .map_err(rejected)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn edit_shared_temperature_entered(
        &self,
        family: FamilyRef,
        wrapping_key: Vec<u8>,
        child_id: Vec<u8>,
        activity_id: Vec<u8>,
        entered: String,
        unit: u8,
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
        let operation = local_api::edit_temperature_entered_operation(
            handle,
            child_id,
            activity,
            &entered,
            unit,
            saved_at_ms,
        )
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

    #[allow(clippy::too_many_arguments)]
    pub fn log_shared_growth_measurements(
        &self,
        family: FamilyRef,
        wrapping_key: Vec<u8>,
        child_id: Vec<u8>,
        weight_g: Option<i64>,
        length_mm: Option<i64>,
        head_mm: Option<i64>,
        time: ActivityWhen,
    ) -> Result<Vec<u8>, BindingError> {
        let mut store = self.store.lock().map_err(|_| BindingError::LockPoisoned)?;
        let handle = family.handle()?;
        let ready = ready_session_for(&mut store, handle, &fixed(&wrapping_key)?)?;
        let saved_at_ms = time.saved_at_ms;
        let (id, operation) = local_api::growth_measurements_operation(
            handle,
            fixed(&child_id)?,
            weight_g,
            length_mm,
            head_mm,
            time.into(),
        )
        .map_err(rejected)?;
        ready
            .append_local(&mut store, operation, saved_at_ms)
            .map_err(rejected)?;
        Ok(id.to_vec())
    }

    pub fn log_shared_growth_entered(
        &self,
        family: FamilyRef,
        wrapping_key: Vec<u8>,
        child_id: Vec<u8>,
        input: GrowthInputRow,
        time: ActivityWhen,
    ) -> Result<Vec<u8>, BindingError> {
        let mut store = self.store.lock().map_err(|_| BindingError::LockPoisoned)?;
        let handle = family.handle()?;
        let ready = ready_session_for(&mut store, handle, &fixed(&wrapping_key)?)?;
        let saved_at_ms = time.saved_at_ms;
        let (id, operation) = local_api::growth_entered_operation(
            handle,
            fixed(&child_id)?,
            &input.into(),
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

    pub fn log_shared_temperature_entered(
        &self,
        family: FamilyRef,
        wrapping_key: Vec<u8>,
        child_id: Vec<u8>,
        entered: String,
        unit: u8,
        time: ActivityWhen,
    ) -> Result<Vec<u8>, BindingError> {
        let mut store = self.store.lock().map_err(|_| BindingError::LockPoisoned)?;
        let handle = family.handle()?;
        let ready = ready_session_for(&mut store, handle, &fixed(&wrapping_key)?)?;
        let saved_at_ms = time.saved_at_ms;
        let (id, operation) = local_api::temperature_entered_operation(
            handle,
            fixed(&child_id)?,
            &entered,
            unit,
            time.into(),
        )
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
}
