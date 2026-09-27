//! Published v1 field validity. Future field IDs remain opaque.

use crate::{
    cbor::Value,
    operation::{Kind, Scope},
};

#[derive(Clone, Copy)]
enum Dimension {
    Volume,
    Mass,
    Length,
    Temperature,
}

pub(crate) fn validate_fields(
    scope: Scope,
    record_type: &str,
    kind: Kind,
    fields: &[(u64, Value)],
) -> Result<(), &'static str> {
    match scope {
        Scope::Family => validate_family(fields),
        Scope::Child => validate_child(fields),
        Scope::Activity => validate_activity(record_type, kind, fields),
    }
}

fn validate_family(fields: &[(u64, Value)]) -> Result<(), &'static str> {
    for (id, value) in fields {
        if *id == 1 {
            let Value::Map(preferences) = value else {
                return Err("unit preferences must be a map");
            };
            for (dimension, unit) in preferences {
                let code = unsigned(unit).ok_or("unit preference must be unsigned")?;
                let accepted = match dimension {
                    1 => (1..=3).contains(&code),
                    2 => (10..=13).contains(&code),
                    3 => (20..=22).contains(&code),
                    4 => (30..=31).contains(&code),
                    _ => false,
                };
                if !accepted {
                    return Err("unit preference outside published dimension");
                }
            }
        }
    }
    Ok(())
}

fn validate_child(fields: &[(u64, Value)]) -> Result<(), &'static str> {
    for (id, value) in fields {
        match id {
            1 if !matches!(value, Value::Text(_)) => return Err("child name must be text"),
            2 if signed_i64(value).is_none() => return Err("birth day must be signed i64"),
            3 if !matches!(unsigned(value), Some(1..=3)) => {
                return Err("child sex code outside v1 range");
            }
            _ => {}
        }
    }
    Ok(())
}

fn validate_activity(
    record_type: &str,
    kind: Kind,
    fields: &[(u64, Value)],
) -> Result<(), &'static str> {
    if !is_known_activity(record_type) {
        return Ok(());
    }
    if kind == Kind::Create && !fields.iter().any(|(id, _)| *id == 1) {
        return Err("activity create requires start");
    }
    if kind == Kind::Create && record_type == "note" && !fields.iter().any(|(id, _)| *id == 4) {
        return Err("note create requires note field");
    }
    for (id, value) in fields {
        match id {
            1 => instant(value)?,
            2 if !matches!(value, Value::Null) => instant(value)?,
            3 if !matches!(value, Value::Null) && !bytes16(value) => {
                return Err("group must be UUID or null");
            }
            4 if !matches!(value, Value::Text(_) | Value::Null) => {
                return Err("note must be text or null");
            }
            100.. => validate_type_specific(record_type, *id, value)?,
            _ => {}
        }
    }
    Ok(())
}

fn is_known_activity(record_type: &str) -> bool {
    matches!(
        record_type,
        "feed.breast"
            | "feed.bottle"
            | "feed.solids"
            | "sleep"
            | "pump"
            | "diaper"
            | "growth"
            | "medication"
            | "temperature"
            | "note"
    )
}

fn validate_type_specific(record_type: &str, id: u64, value: &Value) -> Result<(), &'static str> {
    match (record_type, id) {
        ("feed.breast", 100) => {
            let Value::Array(segments) = value else {
                return Err("breast segments must be an array");
            };
            for segment in segments {
                let Value::Array(parts) = segment else {
                    return Err("breast segment must be an array");
                };
                if parts.len() != 3 || !matches!(unsigned(&parts[0]), Some(1 | 2)) {
                    return Err("breast segment side/shape invalid");
                }
                instant(&parts[1])?;
                if !matches!(parts[2], Value::Null) {
                    instant(&parts[2])?;
                }
            }
        }
        ("feed.bottle", 100) => measure(value, Dimension::Volume)?,
        ("pump", 100..=102) => {
            if !matches!(value, Value::Null) {
                measure(value, Dimension::Volume)?;
            }
        }
        ("feed.bottle", 101) if !matches!(unsigned(value), Some(1..=4)) => {
            return Err("bottle content outside v1 range");
        }
        ("feed.solids", 100) => {
            let Value::Array(foods) = value else {
                return Err("foods must be an array");
            };
            if foods.iter().any(|food| !matches!(food, Value::Text(_))) {
                return Err("food must be text");
            }
        }
        ("feed.solids", 101) | ("medication", 100) if !matches!(value, Value::Text(_)) => {
            return Err("published field must be text");
        }
        ("sleep", 100)
            if !matches!(value, Value::Null) && !matches!(unsigned(value), Some(1..=5)) =>
        {
            return Err("sleep place outside v1 range");
        }
        ("diaper", 100) if !matches!(unsigned(value), Some(1..=4)) => {
            return Err("diaper kind outside v1 range");
        }
        ("growth", 100) | ("growth", 101..=102) => {
            if !matches!(value, Value::Null) {
                let dimension = if id == 100 {
                    Dimension::Mass
                } else {
                    Dimension::Length
                };
                measure(value, dimension)?;
            }
        }
        ("medication", 101) => {
            let Value::Array(parts) = value else {
                return Err("medication dose must be an array");
            };
            if parts.len() != 2 || parts.iter().any(|part| !matches!(part, Value::Text(_))) {
                return Err("medication dose must have amount/unit text");
            }
        }
        ("temperature", 100) => measure(value, Dimension::Temperature)?,
        ("temperature", 101) if !matches!(value, Value::Text(_) | Value::Null) => {
            return Err("temperature method must be text or null");
        }
        _ => {}
    }
    Ok(())
}

fn instant(value: &Value) -> Result<(), &'static str> {
    let Value::Array(parts) = value else {
        return Err("instant must be [utc_ms, offset_minutes]");
    };
    if parts.len() != 2 || signed_i64(&parts[0]).is_none() {
        return Err("instant UTC must be signed i64");
    }
    let offset = signed_i64(&parts[1]).ok_or("instant offset must be signed i64")?;
    if !(-840..=840).contains(&offset) {
        return Err("instant offset outside v1 range");
    }
    Ok(())
}

fn measure(value: &Value, dimension: Dimension) -> Result<(), &'static str> {
    let Value::Map(parts) = value else {
        return Err("measure must be a map");
    };
    let unit = parts
        .iter()
        .find(|(id, _)| *id == 3)
        .and_then(|(_, value)| unsigned(value))
        .ok_or("measure unit missing or invalid")?;
    let known_dimension = match unit {
        1..=3 => Some(Dimension::Volume),
        10..=13 => Some(Dimension::Mass),
        20..=22 => Some(Dimension::Length),
        30..=31 => Some(Dimension::Temperature),
        _ => None,
    };
    let Some(known_dimension) = known_dimension else {
        return Ok(()); // Future unit code: preserve this whole field opaquely.
    };
    if !matches!(
        (known_dimension, dimension),
        (Dimension::Volume, Dimension::Volume)
            | (Dimension::Mass, Dimension::Mass)
            | (Dimension::Length, Dimension::Length)
            | (Dimension::Temperature, Dimension::Temperature)
    ) {
        return Err("measure unit has wrong dimension");
    }
    if parts.len() != 3 || parts[0].0 != 1 || parts[1].0 != 2 || parts[2].0 != 3 {
        return Err("known measure needs exactly keys 1..3");
    }
    let Value::Integer(base) = parts[0].1 else {
        return Err("measure base must be integer");
    };
    let Value::Text(decimal) = &parts[1].1 else {
        return Err("measure decimal must be text");
    };
    let (numerator, denominator) = parse_decimal(decimal).ok_or("invalid measure decimal")?;
    let expected = match unit {
        31 => {
            let adjusted = numerator
                .checked_sub(denominator.checked_mul(32).ok_or("measure overflow")?)
                .ok_or("measure overflow")?;
            round_ratio(
                adjusted.checked_mul(500).ok_or("measure overflow")?,
                denominator.checked_mul(9).ok_or("measure overflow")?,
            )?
        }
        _ => {
            let (factor_num, factor_den) = unit_factor(unit);
            round_ratio(
                numerator
                    .checked_mul(factor_num)
                    .ok_or("measure overflow")?,
                denominator
                    .checked_mul(factor_den)
                    .ok_or("measure overflow")?,
            )?
        }
    };
    if expected != base {
        return Err("measure base differs from entered value");
    }
    Ok(())
}

fn unit_factor(unit: u64) -> (i128, i128) {
    match unit {
        1 | 10 | 20 => (1, 1),
        2 => (295_735_295_625, 10_000_000_000),
        3 => (284_130_625, 10_000_000),
        11 => (1000, 1),
        12 => (45_359_237, 100_000),
        13 => (28_349_523_125, 1_000_000_000),
        21 => (10, 1),
        22 => (254, 10),
        30 => (100, 1),
        _ => unreachable!("known non-Fahrenheit unit"),
    }
}

pub(crate) fn parse_decimal(decimal: &str) -> Option<(i128, i128)> {
    let (negative, digits) = if let Some(rest) = decimal.strip_prefix('-') {
        (true, rest)
    } else {
        (false, decimal)
    };
    let (whole, fraction) = digits.split_once('.').unwrap_or((digits, ""));
    if whole.is_empty()
        || (whole.starts_with('0') && whole.len() != 1)
        || !whole.bytes().all(|byte| byte.is_ascii_digit())
        || (!fraction.is_empty() && !fraction.bytes().all(|byte| byte.is_ascii_digit()))
        || (digits.contains('.') && fraction.is_empty())
    {
        return None;
    }
    let mut numerator = 0i128;
    for byte in whole.bytes().chain(fraction.bytes()) {
        numerator = numerator
            .checked_mul(10)?
            .checked_add(i128::from(byte - b'0'))?;
    }
    let mut denominator = 1i128;
    for _ in fraction.bytes() {
        denominator = denominator.checked_mul(10)?;
    }
    Some((if negative { -numerator } else { numerator }, denominator))
}

pub(crate) fn round_ratio(numerator: i128, denominator: i128) -> Result<i128, &'static str> {
    let magnitude = numerator.checked_abs().ok_or("measure overflow")?;
    let mut result = magnitude / denominator;
    let remainder = magnitude % denominator;
    if remainder >= denominator / 2 + denominator % 2 {
        result = result.checked_add(1).ok_or("measure overflow")?;
    }
    Ok(if numerator < 0 { -result } else { result })
}

fn unsigned(value: &Value) -> Option<u64> {
    match value {
        Value::Integer(number) => u64::try_from(*number).ok(),
        _ => None,
    }
}

fn signed_i64(value: &Value) -> Option<i64> {
    match value {
        Value::Integer(number) => i64::try_from(*number).ok(),
        _ => None,
    }
}

fn bytes16(value: &Value) -> bool {
    matches!(value, Value::Bytes(bytes) if bytes.len() == 16)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn measured(base: i128, decimal: &str, unit: u64) -> Value {
        Value::Map(vec![
            (1, Value::Integer(base)),
            (2, Value::Text(decimal.to_owned())),
            (3, Value::Integer(unit.into())),
        ])
    }

    #[test]
    fn published_unit_vectors_and_exact_rounding() {
        let fixtures: serde_json::Value =
            serde_json::from_str(include_str!("../../tests/vectors/records-v1.json")).unwrap();
        let cases = fixtures["cases"].as_array().unwrap();
        for (id, dimension, base_field) in [
            ("UNIT01", Dimension::Volume, "base_ml"),
            ("UNIT02", Dimension::Temperature, "base_celsius_hundredths"),
        ] {
            let case = cases.iter().find(|case| case["id"] == id).unwrap();
            let decimal = case["input"]["decimal"].as_str().unwrap();
            let unit = case["input"]["unit_code"].as_u64().unwrap();
            let base = case["expect"][base_field].as_i64().unwrap();
            assert_eq!(
                measure(&measured(i128::from(base), decimal, unit), dimension),
                Ok(())
            );
        }
        let opaque = cases.iter().find(|case| case["id"] == "UNIT03").unwrap();
        assert_eq!(
            measure(
                &measured(
                    0,
                    opaque["input"]["decimal"].as_str().unwrap(),
                    opaque["input"]["unit_code"].as_u64().unwrap(),
                ),
                Dimension::Mass
            ),
            Ok(())
        );
        assert_eq!(
            measure(&measured(119, "4", 2), Dimension::Volume),
            Err("measure base differs from entered value")
        );
        assert_eq!(measure(&measured(500, "0.5", 11), Dimension::Mass), Ok(()));
        assert_eq!(measure(&measured(-1, "-0.5", 10), Dimension::Mass), Ok(()));
        assert!(measure(&measured(1, "01", 1), Dimension::Volume).is_err());
        assert!(measure(&measured(1, "1e0", 1), Dimension::Volume).is_err());
        assert!(measure(&measured(1000, "1", 11), Dimension::Volume).is_err());
    }

    #[test]
    fn published_activity_fields_reject_invalid_values() {
        assert!(
            validate_fields(
                Scope::Activity,
                "feed.bottle",
                Kind::Create,
                &[
                    (1, Value::Array(vec![Value::Integer(0), Value::Integer(0)])),
                    (100, Value::Null),
                ]
            )
            .is_err()
        );
        assert!(
            validate_fields(
                Scope::Activity,
                "note",
                Kind::Create,
                &[(1, Value::Array(vec![Value::Integer(0), Value::Integer(0)])),]
            )
            .is_err()
        );
        assert_eq!(
            validate_fields(
                Scope::Activity,
                "future.type",
                Kind::Create,
                &[(500, Value::Null),]
            ),
            Ok(())
        );
    }
}
