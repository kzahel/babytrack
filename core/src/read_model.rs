//! Platform-independent decoding of projected records for client display.

pub use crate::breast::Segment as BreastSegment;
use crate::{cbor::Value, operation::Scope, projection::Record};

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

pub(crate) fn activity_summary(record: &Record) -> Option<Activity> {
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
