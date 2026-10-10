//! Portable operation construction and a small read model for the browser UI.
//! JavaScript supplies intent and storage; Rust owns record bytes and meaning.

use crate::{
    breast::Segment,
    cbor::Value,
    operation::{Kind, NewOperation, Operation, Scope},
    projection::{LocalProjection, Projection, Record},
};
use serde_json::{Value as Json, json};

#[derive(Debug)]
pub enum Error {
    Invalid(&'static str),
    Operation(crate::operation::Error),
}

impl From<crate::operation::Error> for Error {
    fn from(value: crate::operation::Error) -> Self {
        Self::Operation(value)
    }
}

pub use crate::event_actions::Identity;

pub mod action;

fn encode(
    id: Identity,
    scope: Scope,
    record_type: &str,
    child: Option<[u8; 16]>,
    fields: Vec<(u64, Value)>,
) -> Result<Vec<u8>, Error> {
    Ok(Operation::encode_new(&NewOperation {
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
    })?)
}

pub fn family(id: Identity) -> Result<Vec<u8>, Error> {
    if id.record != id.family {
        return Err(Error::Invalid("Family record ID differs"));
    }
    encode(id, Scope::Family, "family", None, vec![])
}

pub fn child(
    id: Identity,
    name: &str,
    birth_day: Option<i64>,
    sex: Option<u8>,
) -> Result<Vec<u8>, Error> {
    let operation =
        crate::event_actions::child(id, name, birth_day, sex).map_err(Error::Invalid)?;
    Ok(Operation::encode_new(&operation)?)
}

fn check_activity_time(start_ms: i64, wall_ms: i64) -> Result<(), Error> {
    if start_ms < 0 || wall_ms < 0 {
        return Err(Error::Invalid("activity time or offset invalid"));
    }
    Ok(())
}

pub fn diaper(
    id: Identity,
    child_id: [u8; 16],
    kind: u8,
    start_ms: i64,
    offset: i16,
) -> Result<Vec<u8>, Error> {
    check_activity_time(start_ms, id.stamp.wall_ms)?;
    let operation = crate::event_actions::diaper(id, child_id, kind, start_ms, offset)
        .map_err(Error::Invalid)?;
    Ok(Operation::encode_new(&operation)?)
}

pub fn bottle(
    id: Identity,
    child_id: [u8; 16],
    ml: u32,
    content: u8,
    start_ms: i64,
    offset: i16,
) -> Result<Vec<u8>, Error> {
    check_activity_time(start_ms, id.stamp.wall_ms)?;
    let operation =
        crate::event_actions::bottle(id, child_id, &ml.to_string(), 1, content, start_ms, offset)
            .map_err(Error::Invalid)?;
    Ok(Operation::encode_new(&operation)?)
}

pub fn note(
    id: Identity,
    child_id: [u8; 16],
    text: &str,
    start_ms: i64,
    offset: i16,
) -> Result<Vec<u8>, Error> {
    check_activity_time(start_ms, id.stamp.wall_ms)?;
    let operation =
        crate::event_actions::note(id, child_id, text, start_ms, offset).map_err(Error::Invalid)?;
    Ok(Operation::encode_new(&operation)?)
}

pub fn start_sleep(
    id: Identity,
    child_id: [u8; 16],
    start_ms: i64,
    offset: i16,
) -> Result<Vec<u8>, Error> {
    check_activity_time(start_ms, id.stamp.wall_ms)?;
    let time = crate::event_actions::ActivityTime {
        start_utc_ms: start_ms,
        offset_minutes: offset,
        saved_at_ms: id.stamp.wall_ms,
    };
    let (_, operation) = crate::event_actions::running_sleep_operation(id, child_id, time)
        .map_err(Error::Invalid)?;
    Ok(Operation::encode_new(&operation)?)
}

pub fn stop_sleep(
    id: Identity,
    child_id: [u8; 16],
    target: &Record,
    end_ms: i64,
    offset: i16,
) -> Result<Vec<u8>, Error> {
    let saved_at_ms = id.stamp.wall_ms;
    let operation = crate::event_actions::stop_sleep_operation(
        id,
        child_id,
        target,
        end_ms,
        offset,
        saved_at_ms,
    )
    .map_err(Error::Invalid)?;
    Ok(Operation::encode_new(&operation)?)
}

fn segments_json(input: &str) -> Result<Vec<Segment>, Error> {
    if input.len() > 4096 {
        return Err(Error::Invalid("breast segments JSON too long"));
    }
    serde_json::from_str(input).map_err(|_| Error::Invalid("breast segments JSON invalid"))
}

pub fn breast(id: Identity, child_id: [u8; 16], input: &str) -> Result<Vec<u8>, Error> {
    let segments = segments_json(input)?;
    let first = segments
        .first()
        .ok_or(Error::Invalid("breast segment count or start invalid"))?;
    check_activity_time(first.start_utc_ms, id.stamp.wall_ms)?;
    let saved_at_ms = id.stamp.wall_ms;
    let operation = crate::event_actions::breast(
        id,
        child_id,
        &segments,
        first.start_utc_ms,
        first.start_offset_minutes,
        saved_at_ms,
    )
    .map_err(Error::Invalid)?;
    Ok(Operation::encode_new(&operation)?)
}

pub fn edit_breast(
    id: Identity,
    child_id: [u8; 16],
    target: &Record,
    input: &str,
) -> Result<Vec<u8>, Error> {
    let segments = segments_json(input)?;
    let saved_at_ms = id.stamp.wall_ms;
    let operation = crate::event_actions::edit_breast(id, child_id, target, &segments, saved_at_ms)
        .map_err(Error::Invalid)?;
    Ok(Operation::encode_new(&operation)?)
}

/// Every read-model field the browser shows or edits; absent values are omitted.
fn activity_json(activity: &crate::read_model::Activity) -> Json {
    let mut row = json!({
        "id": hex(&activity.id),
        "childId": hex(&activity.child_id),
        "kind": activity.kind,
        "startMs": activity.start_utc_ms,
        "offsetMinutes": activity.offset_minutes,
        "endMs": activity.end_utc_ms,
        "sleepPlace": activity.sleep_place,
        "note": activity.note,
        "diaperKind": activity.diaper_kind,
        "bottleMl": activity.bottle_ml,
        "bottleEntered": activity.bottle_entered,
        "bottleUnit": activity.bottle_unit,
        "bottleContent": activity.bottle_content,
        "breastSide": activity.breast_side,
        "breastSegments": activity.breast_segments,
        "solidsFoods": activity.solids_foods,
        "solidsAmount": activity.solids_amount,
        "pumpLeftMl": activity.pump_left_ml,
        "pumpRightMl": activity.pump_right_ml,
        "pumpTotalMl": activity.pump_total_ml,
        "growthWeightG": activity.growth_weight_g,
        "growthWeightEntered": activity.growth_weight_entered,
        "growthWeightUnit": activity.growth_weight_unit,
        "growthLengthMm": activity.growth_length_mm,
        "growthLengthEntered": activity.growth_length_entered,
        "growthLengthUnit": activity.growth_length_unit,
        "growthHeadMm": activity.growth_head_mm,
        "growthHeadEntered": activity.growth_head_entered,
        "growthHeadUnit": activity.growth_head_unit,
        "temperatureC": activity.temperature_c,
        "temperatureEntered": activity.temperature_entered,
        "temperatureUnit": activity.temperature_unit,
        "medicationName": activity.medication_name,
        "medicationDoseAmount": activity.medication_dose_amount,
        "medicationDoseUnit": activity.medication_dose_unit,
    });
    if let Json::Object(fields) = &mut row {
        fields.retain(|_, value| !value.is_null());
    }
    row
}

/// The core's day totals for one child in a viewer-supplied local-day window.
pub fn day_summary<'a>(
    records: impl Iterator<Item = &'a Record>,
    child_id: [u8; 16],
    window: crate::day_summary::DayWindow,
) -> Result<String, Error> {
    let summary = crate::day_summary::summarize_day(
        crate::read_model::activities_from_records(records),
        child_id,
        window,
    )
    .map_err(Error::Invalid)?;
    Ok(json!({
        "sleepMs": summary.sleep_ms,
        "feedCount": summary.feed_count,
        "bottleMl": summary.bottle_ml,
        "diaperCount": summary.diaper_count,
        "wetDiaperCount": summary.wet_diaper_count,
        "dirtyDiaperCount": summary.dirty_diaper_count,
    })
    .to_string())
}

fn hex(id: &[u8; 16]) -> String {
    id.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub fn local_snapshot(projection: &LocalProjection) -> String {
    snapshot_records(projection.records())
}

pub fn shared_snapshot(projection: &Projection) -> String {
    snapshot_records(projection.records())
}

fn snapshot_records<'a>(records: impl Iterator<Item = &'a Record>) -> String {
    let mut children = Vec::new();
    let mut activities = Vec::new();
    for record in records.filter(|row| !row.deleted) {
        match record.scope {
            Scope::Child => {
                if let Some(child) =
                    crate::read_model::children_from_records(std::iter::once(record)).pop()
                {
                    children.push(json!({"id":hex(&child.id), "name":child.name, "birthDay":child.birth_day,"sex":child.sex}));
                }
            }
            Scope::Activity => {
                if let Some(activity) = crate::read_model::activity_summary(record) {
                    activities.push(activity_json(&activity));
                }
            }
            Scope::Family => {}
        }
    }
    activities.sort_by(|a, b| b["startMs"].as_i64().cmp(&a["startMs"].as_i64()));
    json!({"children":children,"activities":activities}).to_string()
}
