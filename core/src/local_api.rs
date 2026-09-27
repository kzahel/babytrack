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
    pub diaper_kind: Option<u8>,
    pub bottle_ml: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActivityTime {
    pub start_utc_ms: i64,
    pub offset_minutes: i16,
    pub saved_at_ms: i64,
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
        Ok(self.store.families()?)
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
        let projection = self.store.load_local(family)?;
        let mut children = Vec::new();
        for record in projection.records() {
            if record.scope != Scope::Child || record.deleted {
                continue;
            }
            let Some(Value::Text(name)) = record.field(1).map(|field| &field.value) else {
                continue;
            };
            children.push(Child {
                id: record.id,
                name: name.clone(),
            });
        }
        children.sort_by(|a, b| a.name.cmp(&b.name).then(a.id.cmp(&b.id)));
        Ok(children)
    }

    pub fn add_child(
        &mut self,
        family: FamilyHandle,
        name: &str,
        now_ms: i64,
    ) -> Result<[u8; 16], Error> {
        check_time(now_ms)?;
        if name.trim().is_empty() || name.len() > 16 * 1024 {
            return Err(Error::Invalid("child name empty or too long"));
        }
        let child_id = ids::random_v7(now_ms)?;
        self.store.append_local(
            family,
            NewOperation {
                family_id: family.family_id,
                operation_id: ids::random_v7(now_ms)?,
                record_id: child_id,
                scope: Scope::Child,
                kind: Kind::Create,
                author_device_id: family.device_id,
                hlc: placeholder_hlc(family),
                record_type: Some("child".to_owned()),
                child_id: None,
                fields: Some(vec![(1, Value::Text(name.trim().to_owned()))]),
            },
            now_ms,
        )?;
        Ok(child_id)
    }

    pub fn log_diaper(
        &mut self,
        family: FamilyHandle,
        child_id: [u8; 16],
        diaper_kind: u8,
        time: ActivityTime,
    ) -> Result<[u8; 16], Error> {
        if !(1..=4).contains(&diaper_kind) {
            return Err(Error::Invalid("diaper kind outside published codes"));
        }
        self.log_activity(
            family,
            child_id,
            "diaper",
            vec![(100, Value::Integer(diaper_kind.into()))],
            time,
        )
    }

    pub fn log_bottle_ml(
        &mut self,
        family: FamilyHandle,
        child_id: [u8; 16],
        amount_ml: i64,
        content: u8,
        time: ActivityTime,
    ) -> Result<[u8; 16], Error> {
        if !(1..=1_000_000).contains(&amount_ml) || !(1..=4).contains(&content) {
            return Err(Error::Invalid("bottle amount or content invalid"));
        }
        self.log_activity(
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

    fn log_activity(
        &mut self,
        family: FamilyHandle,
        child_id: [u8; 16],
        record_type: &str,
        fields: Vec<(u64, Value)>,
        time: ActivityTime,
    ) -> Result<[u8; 16], Error> {
        check_time(time.saved_at_ms)?;
        if !(-840..=840).contains(&time.offset_minutes) {
            return Err(Error::Invalid("recorded offset outside v1 range"));
        }
        let projection = self.store.load_local(family)?;
        if projection
            .record(&child_id)
            .is_none_or(|record| record.scope != Scope::Child || record.deleted)
        {
            return Err(Error::Invalid("target child is unavailable"));
        }
        let activity_id = ids::random_v7(time.saved_at_ms)?;
        let mut all_fields = vec![(
            1,
            Value::Array(vec![
                Value::Integer(time.start_utc_ms.into()),
                Value::Integer(time.offset_minutes.into()),
            ]),
        )];
        all_fields.extend(fields);
        self.store.append_local(
            family,
            NewOperation {
                family_id: family.family_id,
                operation_id: ids::random_v7(time.saved_at_ms)?,
                record_id: activity_id,
                scope: Scope::Activity,
                kind: Kind::Create,
                author_device_id: family.device_id,
                hlc: placeholder_hlc(family),
                record_type: Some(record_type.to_owned()),
                child_id: Some(child_id),
                fields: Some(all_fields),
            },
            time.saved_at_ms,
        )?;
        Ok(activity_id)
    }

    pub fn timeline(
        &self,
        family: FamilyHandle,
        child_id: [u8; 16],
    ) -> Result<Vec<Activity>, Error> {
        let projection = self.store.load_local(family)?;
        if projection
            .record(&child_id)
            .is_none_or(|record| record.scope != Scope::Child || record.deleted)
        {
            return Err(Error::Invalid("target child is unavailable"));
        }
        let mut activities = projection
            .records()
            .filter(|record| {
                record.scope == Scope::Activity
                    && record.child_id == Some(child_id)
                    && !record.deleted
            })
            .filter_map(activity_summary)
            .collect::<Vec<_>>();
        activities.sort_by(|a, b| b.start_utc_ms.cmp(&a.start_utc_ms).then(b.id.cmp(&a.id)));
        Ok(activities)
    }

    pub fn backup(&self, family: FamilyHandle, now_ms: i64) -> Result<Vec<u8>, Error> {
        Ok(portable_file::export_readable_local(
            &self.store,
            family,
            now_ms,
        )?)
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
    Some(Activity {
        id: record.id,
        child_id: record.child_id?,
        kind: record.record_type.clone(),
        start_utc_ms: i64::try_from(*start).ok()?,
        offset_minutes: i16::try_from(*offset).ok()?,
        diaper_kind,
        bottle_ml,
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
