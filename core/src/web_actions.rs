//! Portable operation construction and a small read model for the browser UI.
//! JavaScript supplies intent and storage; Rust owns record bytes and meaning.

use crate::{
    cbor::Value,
    operation::{Hlc, Kind, NewOperation, Operation, Scope},
    projection::{LocalProjection, Record},
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

pub struct Identity {
    pub family: [u8; 16],
    pub device: [u8; 16],
    pub operation: [u8; 16],
    pub record: [u8; 16],
    pub stamp: Hlc,
}

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
    let name = name.trim();
    if name.is_empty() || name.len() > 16 * 1024 {
        return Err(Error::Invalid("child name empty or too long"));
    }
    if sex.is_some_and(|code| !(1..=3).contains(&code)) {
        return Err(Error::Invalid("child sex code outside published range"));
    }
    let mut fields = vec![(1, Value::Text(name.to_owned()))];
    if let Some(day) = birth_day {
        fields.push((2, Value::Integer(day.into())));
    }
    if let Some(code) = sex {
        fields.push((3, Value::Integer(code.into())));
    }
    encode(id, Scope::Child, "child", None, fields)
}

fn activity(
    id: Identity,
    child_id: [u8; 16],
    record_type: &str,
    start_ms: i64,
    offset_minutes: i16,
    mut fields: Vec<(u64, Value)>,
) -> Result<Vec<u8>, Error> {
    if start_ms < 0 || id.stamp.wall_ms < 0 || !(-840..=840).contains(&offset_minutes) {
        return Err(Error::Invalid("activity time or offset invalid"));
    }
    let mut all = vec![(
        1,
        Value::Array(vec![
            Value::Integer(start_ms.into()),
            Value::Integer(offset_minutes.into()),
        ]),
    )];
    all.append(&mut fields);
    encode(id, Scope::Activity, record_type, Some(child_id), all)
}

pub fn diaper(
    id: Identity,
    child_id: [u8; 16],
    kind: u8,
    start_ms: i64,
    offset: i16,
) -> Result<Vec<u8>, Error> {
    if !(1..=4).contains(&kind) {
        return Err(Error::Invalid("diaper kind outside published codes"));
    }
    activity(
        id,
        child_id,
        "diaper",
        start_ms,
        offset,
        vec![(100, Value::Integer(kind.into()))],
    )
}

pub fn bottle(
    id: Identity,
    child_id: [u8; 16],
    ml: u32,
    content: u8,
    start_ms: i64,
    offset: i16,
) -> Result<Vec<u8>, Error> {
    if !(1..=1_000_000).contains(&ml) || !(1..=4).contains(&content) {
        return Err(Error::Invalid("bottle amount or content invalid"));
    }
    let measure = Value::Map(vec![
        (1, Value::Integer(ml.into())),
        (2, Value::Text(ml.to_string())),
        (3, Value::Integer(1)),
    ]);
    activity(
        id,
        child_id,
        "feed.bottle",
        start_ms,
        offset,
        vec![(100, measure), (101, Value::Integer(content.into()))],
    )
}

pub fn note(
    id: Identity,
    child_id: [u8; 16],
    text: &str,
    start_ms: i64,
    offset: i16,
) -> Result<Vec<u8>, Error> {
    let text = text.trim();
    if text.is_empty() || text.len() > 4096 {
        return Err(Error::Invalid("note must contain 1 to 4096 bytes"));
    }
    activity(
        id,
        child_id,
        "note",
        start_ms,
        offset,
        vec![(4, Value::Text(text.to_owned()))],
    )
}

fn text(record: &Record, key: u64) -> Option<&str> {
    match &record.field(key)?.value {
        Value::Text(value) => Some(value),
        _ => None,
    }
}

fn integer(record: &Record, key: u64) -> Option<i64> {
    match &record.field(key)?.value {
        Value::Integer(value) => (*value).try_into().ok(),
        _ => None,
    }
}

fn hex(id: &[u8; 16]) -> String {
    id.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub fn local_snapshot(projection: &LocalProjection) -> String {
    let mut children = Vec::new();
    let mut activities = Vec::new();
    for record in projection.records().filter(|row| !row.deleted) {
        match record.scope {
            Scope::Child => children.push(json!({
                "id": hex(&record.id),
                "name": text(record, 1).unwrap_or("Child"),
                "birthDay": integer(record, 2),
                "sex": integer(record, 3),
            })),
            Scope::Activity => {
                let start = match &record.field(1).map(|field| &field.value) {
                    Some(Value::Array(parts)) if parts.len() == 2 => match &parts[0] {
                        Value::Integer(value) => i64::try_from(*value).ok(),
                        _ => None,
                    },
                    _ => None,
                };
                let Some(start) = start else { continue };
                let mut row = json!({
                    "id": hex(&record.id),
                    "childId": record.child_id.as_ref().map(hex),
                    "kind": record.record_type,
                    "startMs": start,
                });
                if let Json::Object(ref mut fields) = row {
                    match record.record_type.as_str() {
                        "diaper" => {
                            fields.insert("diaperKind".to_owned(), json!(integer(record, 100)));
                        }
                        "note" => {
                            fields.insert("note".to_owned(), json!(text(record, 4)));
                        }
                        "feed.bottle" => {
                            let amount = record.field(100).and_then(|field| match &field.value {
                                Value::Map(parts) => parts
                                    .iter()
                                    .find(|(id, _)| *id == 1)
                                    .and_then(|(_, value)| match value {
                                        Value::Integer(number) => i64::try_from(*number).ok(),
                                        _ => None,
                                    }),
                                _ => None,
                            });
                            fields.insert("bottleMl".to_owned(), json!(amount));
                            fields.insert("bottleContent".to_owned(), json!(integer(record, 101)));
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
    json!({ "children": children, "activities": activities }).to_string()
}
