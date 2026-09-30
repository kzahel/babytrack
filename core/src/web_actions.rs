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
                let Some(activity) = crate::read_model::activity_summary(record) else {
                    continue;
                };
                let mut row = json!({"id":hex(&activity.id),"childId":hex(&activity.child_id),"kind":activity.kind,"startMs":activity.start_utc_ms});
                if let Json::Object(ref mut fields) = row {
                    match activity.kind.as_str() {
                        "diaper" => {
                            fields.insert("diaperKind".to_owned(), json!(activity.diaper_kind));
                        }
                        "note" => {
                            fields.insert("note".to_owned(), json!(activity.note));
                        }
                        "feed.bottle" => {
                            fields.insert("bottleMl".to_owned(), json!(activity.bottle_ml));
                            fields
                                .insert("bottleContent".to_owned(), json!(activity.bottle_content));
                        }
                        "feed.breast" => {
                            fields.insert(
                                "breastSegments".to_owned(),
                                json!(activity.breast_segments),
                            );
                        }
                        _ => {}
                    }
                }
                activities.push(row);
            }
            Scope::Family => {}
        }
    }
    activities.sort_by(|a, b| b["startMs"].as_i64().cmp(&a["startMs"].as_i64()));
    json!({"children":children,"activities":activities}).to_string()
}
