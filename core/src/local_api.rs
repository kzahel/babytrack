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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Activity {
    pub id: [u8; 16],
    pub child_id: [u8; 16],
    pub kind: String,
    pub start_utc_ms: i64,
    pub offset_minutes: i16,
    pub end_utc_ms: Option<i64>,
    pub note: Option<String>,
    pub diaper_kind: Option<u8>,
    pub bottle_ml: Option<i64>,
    pub breast_side: Option<u8>,
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
        self.ensure_local_surface(family)?;
        let (child_id, operation) = child_operation(family, name, now_ms)?;
        self.store.append_local(family, operation, now_ms)?;
        Ok(child_id)
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
        let (activity_id, operation) =
            sleep_operation(family, child_id, time, end_utc_ms, end_offset_minutes)?;
        self.append_activity(family, child_id, operation, time.saved_at_ms)?;
        Ok(activity_id)
    }

    pub fn start_sleep(
        &mut self,
        family: FamilyHandle,
        child_id: [u8; 16],
        time: ActivityTime,
    ) -> Result<[u8; 16], Error> {
        let (activity_id, operation) = running_sleep_operation(family, child_id, time)?;
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
    check_time(now_ms)?;
    if name.trim().is_empty() || name.len() > 16 * 1024 {
        return Err(Error::Invalid("child name empty or too long"));
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
            fields: Some(vec![(1, Value::Text(name.trim().to_owned()))]),
        },
    ))
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
    if !(1..=1_000_000).contains(&amount_ml) || !(1..=4).contains(&content) {
        return Err(Error::Invalid("bottle amount or content invalid"));
    }
    activity_operation(
        family,
        child_id,
        "feed.bottle",
        vec![
            (
                100,
                Value::Map(vec![
                    (1, Value::Integer(amount_ml.into())),
                    (2, Value::Text(amount_ml.to_string())),
                    (3, Value::Integer(1)),
                ]),
            ),
            (101, Value::Integer(content.into())),
        ],
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
    if !(1..=2).contains(&side) || end_utc_ms < time.start_utc_ms || end_utc_ms > time.saved_at_ms {
        return Err(Error::Invalid("breast side or completed interval invalid"));
    }
    let start = Value::Array(vec![
        Value::Integer(time.start_utc_ms.into()),
        Value::Integer(time.offset_minutes.into()),
    ]);
    let end = Value::Array(vec![
        Value::Integer(end_utc_ms.into()),
        Value::Integer(time.offset_minutes.into()),
    ]);
    activity_operation(
        family,
        child_id,
        "feed.breast",
        vec![
            (2, end.clone()),
            (
                100,
                Value::Array(vec![Value::Array(vec![
                    Value::Integer(side.into()),
                    start,
                    end,
                ])]),
            ),
        ],
        time,
    )
}

pub fn pump_operation(
    family: FamilyHandle,
    child_id: [u8; 16],
    amounts: PumpAmounts,
    time: ActivityTime,
    end_utc_ms: i64,
) -> Result<([u8; 16], NewOperation), Error> {
    let PumpAmounts {
        left_ml,
        right_ml,
        total_ml,
    } = amounts;
    if end_utc_ms < time.start_utc_ms
        || end_utc_ms > time.saved_at_ms
        || total_ml.is_some() && (left_ml.is_some() || right_ml.is_some())
        || total_ml.is_none() && left_ml.unwrap_or(0) <= 0 && right_ml.unwrap_or(0) <= 0
        || [left_ml, right_ml, total_ml]
            .into_iter()
            .flatten()
            .any(|amount| !(0..=1_000_000).contains(&amount))
        || total_ml == Some(0)
    {
        return Err(Error::Invalid("pump interval or amounts invalid"));
    }
    let mut fields = vec![(
        2,
        Value::Array(vec![
            Value::Integer(end_utc_ms.into()),
            Value::Integer(time.offset_minutes.into()),
        ]),
    )];
    for (id, amount) in [(100, left_ml), (101, right_ml), (102, total_ml)] {
        if let Some(amount) = amount {
            fields.push((id, whole_measure(amount, 1)));
        }
    }
    activity_operation(family, child_id, "pump", fields, time)
}

pub fn solids_operation(
    family: FamilyHandle,
    child_id: [u8; 16],
    foods: &[String],
    amount: &str,
    time: ActivityTime,
) -> Result<([u8; 16], NewOperation), Error> {
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
    activity_operation(
        family,
        child_id,
        "feed.solids",
        vec![
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
        ],
        time,
    )
}

pub fn sleep_operation(
    family: FamilyHandle,
    child_id: [u8; 16],
    time: ActivityTime,
    end_utc_ms: i64,
    end_offset_minutes: i16,
) -> Result<([u8; 16], NewOperation), Error> {
    if end_utc_ms < time.start_utc_ms || end_utc_ms > time.saved_at_ms {
        return Err(Error::Invalid("sleep end outside completed interval"));
    }
    if !(-840..=840).contains(&end_offset_minutes) {
        return Err(Error::Invalid("sleep end offset outside v1 range"));
    }
    activity_operation(
        family,
        child_id,
        "sleep",
        vec![(
            2,
            Value::Array(vec![
                Value::Integer(end_utc_ms.into()),
                Value::Integer(end_offset_minutes.into()),
            ]),
        )],
        time,
    )
}

pub fn running_sleep_operation(
    family: FamilyHandle,
    child_id: [u8; 16],
    time: ActivityTime,
) -> Result<([u8; 16], NewOperation), Error> {
    activity_operation(family, child_id, "sleep", vec![], time)
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

pub fn growth_operation(
    family: FamilyHandle,
    child_id: [u8; 16],
    weight_g: Option<i64>,
    length_mm: Option<i64>,
    time: ActivityTime,
) -> Result<([u8; 16], NewOperation), Error> {
    if weight_g.is_none() && length_mm.is_none() {
        return Err(Error::Invalid("growth needs weight or length"));
    }
    if weight_g.is_some_and(|value| !(1..=100_000).contains(&value))
        || length_mm.is_some_and(|value| !(1..=2_500).contains(&value))
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
    activity_operation(family, child_id, "growth", fields, time)
}

pub fn temperature_c_operation(
    family: FamilyHandle,
    child_id: [u8; 16],
    entered_c: &str,
    time: ActivityTime,
) -> Result<([u8; 16], NewOperation), Error> {
    let decimal = entered_c.trim();
    if decimal.is_empty() || decimal.len() > 16 {
        return Err(Error::Invalid("temperature decimal empty or too long"));
    }
    let (numerator, denominator) = crate::record_validity::parse_decimal(decimal)
        .ok_or(Error::Invalid("temperature decimal invalid"))?;
    let scaled = numerator
        .checked_mul(100)
        .ok_or(Error::Invalid("temperature decimal overflow"))?;
    let base = crate::record_validity::round_ratio(scaled, denominator)
        .map_err(|_| Error::Invalid("temperature decimal overflow"))?;
    i64::try_from(base).map_err(|_| Error::Invalid("temperature outside i64 range"))?;
    activity_operation(
        family,
        child_id,
        "temperature",
        vec![(
            100,
            Value::Map(vec![
                (1, Value::Integer(base)),
                (2, Value::Text(decimal.to_owned())),
                (3, Value::Integer(30)),
            ]),
        )],
        time,
    )
}

pub fn medication_operation(
    family: FamilyHandle,
    child_id: [u8; 16],
    name: &str,
    dose_amount: &str,
    dose_unit: &str,
    time: ActivityTime,
) -> Result<([u8; 16], NewOperation), Error> {
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
    activity_operation(
        family,
        child_id,
        "medication",
        vec![
            (100, Value::Text(name.to_owned())),
            (
                101,
                Value::Array(vec![
                    Value::Text(dose_amount.to_owned()),
                    Value::Text(dose_unit.to_owned()),
                ]),
            ),
        ],
        time,
    )
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
    let bottle_ml = if record.record_type == "feed.bottle" {
        let Value::Map(measure) = &record.field(100)?.value else {
            return None;
        };
        let Value::Integer(amount) = measure.first()?.1 else {
            return None;
        };
        i64::try_from(amount).ok()
    } else {
        None
    };
    let breast_side = if record.record_type == "feed.breast" {
        let Value::Array(segments) = &record.field(100)?.value else {
            return None;
        };
        let [Value::Array(parts)] = segments.as_slice() else {
            return None;
        };
        let [Value::Integer(side), _, _] = parts.as_slice() else {
            return None;
        };
        u8::try_from(*side).ok()
    } else {
        None
    };
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
    let growth_measure = |field| -> Option<i64> {
        let Value::Map(measure) = &record.field(field)?.value else {
            return None;
        };
        let (1, Value::Integer(value)) = measure.first()? else {
            return None;
        };
        i64::try_from(*value).ok()
    };
    let pump_measure = |field| -> Option<i64> {
        let Value::Map(measure) = &record.field(field)?.value else {
            return None;
        };
        let (1, Value::Integer(value)) = measure.first()? else {
            return None;
        };
        i64::try_from(*value).ok()
    };
    let temperature_c = if record.record_type == "temperature" {
        let Value::Map(measure) = &record.field(100)?.value else {
            return None;
        };
        match measure.get(1) {
            Some((2, Value::Text(decimal))) => Some(decimal.clone()),
            _ => None,
        }
    } else {
        None
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
        note: if record.record_type == "note" {
            let Value::Text(note) = &record.field(4)?.value else {
                return None;
            };
            Some(note.clone())
        } else {
            None
        },
        diaper_kind,
        bottle_ml,
        breast_side,
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
        growth_weight_g: if record.record_type == "growth" {
            growth_measure(100)
        } else {
            None
        },
        growth_length_mm: if record.record_type == "growth" {
            growth_measure(101)
        } else {
            None
        },
        temperature_c,
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
