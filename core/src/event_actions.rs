//! Portable event construction. Callers supply IDs, clocks, and save time.

use crate::{
    breast::{self, Segment},
    cbor::Value,
    operation::{Hlc, Kind, NewOperation, Scope},
    projection::Record,
};

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

pub struct Identity {
    pub family: [u8; 16],
    pub device: [u8; 16],
    pub operation: [u8; 16],
    pub record: [u8; 16],
    pub stamp: Hlc,
}

fn create(
    id: Identity,
    scope: Scope,
    record_type: &str,
    child: Option<[u8; 16]>,
    fields: Vec<(u64, Value)>,
) -> NewOperation {
    NewOperation {
        family_id: id.family,
        operation_id: id.operation,
        record_id: id.record,
        scope,
        kind: Kind::Create,
        author_device_id: id.device,
        hlc: id.stamp,
        record_type: Some(record_type.to_owned()),
        child_id: child,
        fields: Some(fields),
    }
}

pub fn child(
    id: Identity,
    name: &str,
    birth_day: Option<i64>,
    sex: Option<u8>,
) -> Result<NewOperation, &'static str> {
    let name = name.trim();
    if name.is_empty() || name.len() > 16 * 1024 {
        return Err("child name empty or too long");
    }
    if sex.is_some_and(|code| !(1..=3).contains(&code)) {
        return Err("child sex code outside published range");
    }
    let mut fields = vec![(1, Value::Text(name.to_owned()))];
    if let Some(day) = birth_day {
        fields.push((2, Value::Integer(day.into())));
    }
    if let Some(code) = sex {
        fields.push((3, Value::Integer(code.into())));
    }
    Ok(create(id, Scope::Child, "child", None, fields))
}

fn activity(
    id: Identity,
    child: [u8; 16],
    kind: &str,
    start_ms: i64,
    offset: i16,
    fields: Vec<(u64, Value)>,
) -> Result<NewOperation, &'static str> {
    if !(-840..=840).contains(&offset) {
        return Err("recorded offset outside v1 range");
    }
    let mut all = vec![(
        1,
        Value::Array(vec![
            Value::Integer(start_ms.into()),
            Value::Integer(offset.into()),
        ]),
    )];
    all.extend(fields);
    Ok(create(id, Scope::Activity, kind, Some(child), all))
}

pub fn diaper(
    id: Identity,
    child: [u8; 16],
    kind: u8,
    start_ms: i64,
    offset: i16,
) -> Result<NewOperation, &'static str> {
    if !(1..=4).contains(&kind) {
        return Err("diaper kind outside published codes");
    }
    activity(
        id,
        child,
        "diaper",
        start_ms,
        offset,
        vec![(100, Value::Integer(kind.into()))],
    )
}

pub(crate) fn bottle_measure(entered: &str, unit: u8) -> Result<Value, &'static str> {
    let decimal = entered.trim();
    if decimal.is_empty() || decimal.len() > 16 || !(1..=3).contains(&unit) {
        return Err("bottle amount or unit invalid");
    }
    let (numerator, denominator) =
        crate::record_validity::parse_decimal(decimal).ok_or("bottle decimal invalid")?;
    let (factor_num, factor_den) = crate::record_validity::unit_factor(unit.into());
    let scaled = numerator
        .checked_mul(factor_num)
        .ok_or("bottle amount overflow")?;
    let divisor = denominator
        .checked_mul(factor_den)
        .ok_or("bottle amount overflow")?;
    let base = crate::record_validity::round_ratio(scaled, divisor)
        .map_err(|_| "bottle amount overflow")?;
    if !(1..=1_000_000).contains(&base) {
        return Err("bottle amount invalid");
    }
    Ok(Value::Map(vec![
        (1, Value::Integer(base)),
        (2, Value::Text(decimal.to_owned())),
        (3, Value::Integer(unit.into())),
    ]))
}

#[allow(clippy::too_many_arguments)]
pub fn bottle(
    id: Identity,
    child: [u8; 16],
    entered: &str,
    unit: u8,
    content: u8,
    start_ms: i64,
    offset: i16,
) -> Result<NewOperation, &'static str> {
    if !(1..=4).contains(&content) {
        return Err("bottle content invalid");
    }
    let measure = bottle_measure(entered, unit)?;
    activity(
        id,
        child,
        "feed.bottle",
        start_ms,
        offset,
        vec![(100, measure), (101, Value::Integer(content.into()))],
    )
}

pub fn note(
    id: Identity,
    child: [u8; 16],
    text: &str,
    start_ms: i64,
    offset: i16,
) -> Result<NewOperation, &'static str> {
    let text = text.trim();
    if text.is_empty() || text.len() > 4096 {
        return Err("note must contain 1 to 4096 bytes");
    }
    activity(
        id,
        child,
        "note",
        start_ms,
        offset,
        vec![(4, Value::Text(text.to_owned()))],
    )
}

pub fn breast(
    id: Identity,
    child: [u8; 16],
    segments: &[Segment],
    start_ms: i64,
    offset: i16,
    saved_at_ms: i64,
) -> Result<NewOperation, &'static str> {
    let fields = breast::fields(segments, start_ms, offset, saved_at_ms)?;
    activity(id, child, "feed.breast", start_ms, offset, fields)
}

pub fn edit_breast(
    id: Identity,
    child: [u8; 16],
    target: &Record,
    segments: &[Segment],
    saved_at_ms: i64,
) -> Result<NewOperation, &'static str> {
    if target.scope != Scope::Activity
        || target.record_type != "feed.breast"
        || target.child_id != Some(child)
        || target.deleted
        || id.record != target.id
    {
        return Err("breast feed target unavailable");
    }
    let Some(Value::Array(start)) = target.field(1).map(|field| &field.value) else {
        return Err("breast feed start unavailable");
    };
    let [Value::Integer(ms), Value::Integer(offset)] = start.as_slice() else {
        return Err("breast feed start unavailable");
    };
    let ms = i64::try_from(*ms).map_err(|_| "breast feed start unavailable")?;
    let offset = i16::try_from(*offset).map_err(|_| "breast feed start unavailable")?;
    let fields = breast::fields(segments, ms, offset, saved_at_ms)?;
    Ok(NewOperation {
        family_id: id.family,
        operation_id: id.operation,
        record_id: id.record,
        scope: Scope::Activity,
        kind: Kind::Set,
        author_device_id: id.device,
        hlc: id.stamp,
        record_type: None,
        child_id: None,
        fields: Some(fields),
    })
}

mod child;
mod corrections;
mod intervals;
mod measurements;
mod tracking;

pub use child::{edit_child_metadata_operation, rename_child_operation};
pub use corrections::{
    delete_activity_operation, edit_bottle_entered_operation, edit_bottle_ml_operation,
    edit_bottle_operation, edit_diaper_kind_operation, edit_instant_time_operation,
    edit_note_operation, restore_activity_operation,
};
pub use intervals::{
    edit_pump_amounts_operation, edit_sleep_end_operation, edit_sleep_place_operation,
    move_completed_interval_operation, pump_operation, running_sleep_operation,
    running_sleep_operation_with_place, sleep_operation, sleep_operation_with_place,
    stop_sleep_operation,
};
use measurements::whole_measure;
pub use measurements::{
    edit_growth_entered_operation, edit_growth_measurements_operation, edit_growth_operation,
    edit_temperature_c_operation, edit_temperature_entered_operation, growth_entered_operation,
    growth_measurements_operation, growth_operation, temperature_c_operation,
    temperature_entered_operation,
};
use tracking::activity_operation;
pub use tracking::{
    edit_medication_operation, edit_solids_operation, medication_operation, solids_operation,
};

pub(crate) fn check_time(now_ms: i64) -> Result<(), &'static str> {
    if now_ms < 0 || (now_ms as u64) >= (1u64 << 48) {
        return Err("time outside UUIDv7 range");
    }
    Ok(())
}
