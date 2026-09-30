//! Portable measurements construction; IDs and clocks come from the caller.

use super::*;

pub fn growth_operation(
    identity: Identity,
    child_id: [u8; 16],
    weight_g: Option<i64>,
    length_mm: Option<i64>,
    time: ActivityTime,
) -> Result<([u8; 16], NewOperation), &'static str> {
    growth_measurements_operation(identity, child_id, weight_g, length_mm, None, time)
}

pub fn growth_measurements_operation(
    identity: Identity,
    child_id: [u8; 16],
    weight_g: Option<i64>,
    length_mm: Option<i64>,
    head_mm: Option<i64>,
    time: ActivityTime,
) -> Result<([u8; 16], NewOperation), &'static str> {
    let fields = growth_fields(weight_g, length_mm, head_mm)?;
    activity_operation(identity, child_id, "growth", fields, time)
}

fn growth_fields(
    weight_g: Option<i64>,
    length_mm: Option<i64>,
    head_mm: Option<i64>,
) -> Result<Vec<(u64, Value)>, &'static str> {
    if weight_g.is_none() && length_mm.is_none() && head_mm.is_none() {
        return Err("growth needs a measurement");
    }
    if weight_g.is_some_and(|value| !(1..=100_000).contains(&value))
        || length_mm.is_some_and(|value| !(1..=2_500).contains(&value))
        || head_mm.is_some_and(|value| !(1..=1_000).contains(&value))
    {
        return Err("growth measurement outside supported range");
    }
    let mut fields = Vec::new();
    if let Some(value) = weight_g {
        fields.push((100, whole_measure(value, 10)));
    }
    if let Some(value) = length_mm {
        fields.push((101, whole_measure(value, 20)));
    }
    if let Some(value) = head_mm {
        fields.push((102, whole_measure(value, 20)));
    }
    Ok(fields)
}

fn entered_growth_measure(
    input: &MeasurementInput,
    units: std::ops::RangeInclusive<u8>,
    maximum: i128,
) -> Result<Value, &'static str> {
    let entered = input.entered.trim();
    if entered.is_empty() || entered.len() > 16 || !units.contains(&input.unit) {
        return Err("growth decimal or unit invalid");
    }
    let (numerator, denominator) =
        crate::record_validity::parse_decimal(entered).ok_or("growth decimal invalid")?;
    let (factor_num, factor_den) = crate::record_validity::unit_factor(input.unit.into());
    let scaled = numerator
        .checked_mul(factor_num)
        .ok_or("growth measurement overflow")?;
    let divisor = denominator
        .checked_mul(factor_den)
        .ok_or("growth measurement overflow")?;
    let base = crate::record_validity::round_ratio(scaled, divisor)
        .map_err(|_| "growth measurement overflow")?;
    if !(1..=maximum).contains(&base) {
        return Err("growth measurement outside supported range");
    }
    Ok(Value::Map(vec![
        (1, Value::Integer(base)),
        (2, Value::Text(entered.to_owned())),
        (3, Value::Integer(input.unit.into())),
    ]))
}

fn growth_entered_fields(input: &GrowthInput) -> Result<Vec<(u64, Value)>, &'static str> {
    let mut fields = Vec::new();
    if let Some(weight) = &input.weight {
        fields.push((100, entered_growth_measure(weight, 10..=13, 100_000)?));
    }
    if let Some(length) = &input.length {
        fields.push((101, entered_growth_measure(length, 20..=22, 2_500)?));
    }
    if let Some(head) = &input.head {
        fields.push((102, entered_growth_measure(head, 20..=22, 1_000)?));
    }
    if fields.is_empty() {
        return Err("growth needs a measurement");
    }
    Ok(fields)
}

pub fn growth_entered_operation(
    identity: Identity,
    child_id: [u8; 16],
    input: &GrowthInput,
    time: ActivityTime,
) -> Result<([u8; 16], NewOperation), &'static str> {
    activity_operation(
        identity,
        child_id,
        "growth",
        growth_entered_fields(input)?,
        time,
    )
}

pub fn edit_growth_entered_operation(
    identity: Identity,
    child_id: [u8; 16],
    activity: &Record,
    input: &GrowthInput,
    saved_at_ms: i64,
) -> Result<NewOperation, &'static str> {
    if activity.scope != Scope::Activity
        || activity.child_id != Some(child_id)
        || activity.record_type != "growth"
        || activity.deleted
    {
        return Err("growth target unavailable");
    }
    check_time(saved_at_ms)?;
    Ok(NewOperation {
        family_id: identity.family,
        operation_id: identity.operation,
        record_id: activity.id,
        scope: Scope::Activity,
        kind: Kind::Set,
        author_device_id: identity.device,
        hlc: identity.stamp,
        record_type: None,
        child_id: None,
        fields: Some(growth_entered_fields(input)?),
    })
}

pub fn edit_growth_operation(
    identity: Identity,
    child_id: [u8; 16],
    activity: &Record,
    weight_g: Option<i64>,
    length_mm: Option<i64>,
    saved_at_ms: i64,
) -> Result<NewOperation, &'static str> {
    edit_growth_measurements_operation(
        identity,
        child_id,
        activity,
        weight_g,
        length_mm,
        None,
        saved_at_ms,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn edit_growth_measurements_operation(
    identity: Identity,
    child_id: [u8; 16],
    activity: &Record,
    weight_g: Option<i64>,
    length_mm: Option<i64>,
    head_mm: Option<i64>,
    saved_at_ms: i64,
) -> Result<NewOperation, &'static str> {
    if activity.scope != Scope::Activity
        || activity.child_id != Some(child_id)
        || activity.record_type != "growth"
        || activity.deleted
    {
        return Err("growth target unavailable");
    }
    check_time(saved_at_ms)?;
    Ok(NewOperation {
        family_id: identity.family,
        operation_id: identity.operation,
        record_id: activity.id,
        scope: Scope::Activity,
        kind: Kind::Set,
        author_device_id: identity.device,
        hlc: identity.stamp,
        record_type: None,
        child_id: None,
        fields: Some(growth_fields(weight_g, length_mm, head_mm)?),
    })
}

pub fn temperature_c_operation(
    identity: Identity,
    child_id: [u8; 16],
    entered_c: &str,
    time: ActivityTime,
) -> Result<([u8; 16], NewOperation), &'static str> {
    temperature_entered_operation(identity, child_id, entered_c, 30, time)
}

pub fn temperature_entered_operation(
    identity: Identity,
    child_id: [u8; 16],
    entered: &str,
    unit: u8,
    time: ActivityTime,
) -> Result<([u8; 16], NewOperation), &'static str> {
    activity_operation(
        identity,
        child_id,
        "temperature",
        temperature_fields(entered, unit)?,
        time,
    )
}

fn temperature_fields(entered: &str, unit: u8) -> Result<Vec<(u64, Value)>, &'static str> {
    let decimal = entered.trim();
    if decimal.is_empty() || decimal.len() > 16 || !matches!(unit, 30 | 31) {
        return Err("temperature decimal or unit invalid");
    }
    let (numerator, denominator) =
        crate::record_validity::parse_decimal(decimal).ok_or("temperature decimal invalid")?;
    let (scaled, divisor) = if unit == 30 {
        (
            numerator
                .checked_mul(100)
                .ok_or("temperature decimal overflow")?,
            denominator,
        )
    } else {
        (
            numerator
                .checked_sub(
                    denominator
                        .checked_mul(32)
                        .ok_or("temperature decimal overflow")?,
                )
                .and_then(|difference| difference.checked_mul(500))
                .ok_or("temperature decimal overflow")?,
            denominator
                .checked_mul(9)
                .ok_or("temperature decimal overflow")?,
        )
    };
    let base = crate::record_validity::round_ratio(scaled, divisor)
        .map_err(|_| "temperature decimal overflow")?;
    i64::try_from(base).map_err(|_| "temperature outside i64 range")?;
    Ok(vec![(
        100,
        Value::Map(vec![
            (1, Value::Integer(base)),
            (2, Value::Text(decimal.to_owned())),
            (3, Value::Integer(unit.into())),
        ]),
    )])
}

pub fn edit_temperature_c_operation(
    identity: Identity,
    child_id: [u8; 16],
    activity: &Record,
    entered_c: &str,
    saved_at_ms: i64,
) -> Result<NewOperation, &'static str> {
    edit_temperature_entered_operation(identity, child_id, activity, entered_c, 30, saved_at_ms)
}

pub fn edit_temperature_entered_operation(
    identity: Identity,
    child_id: [u8; 16],
    activity: &Record,
    entered: &str,
    unit: u8,
    saved_at_ms: i64,
) -> Result<NewOperation, &'static str> {
    if activity.scope != Scope::Activity
        || activity.child_id != Some(child_id)
        || activity.record_type != "temperature"
        || activity.deleted
    {
        return Err("temperature target unavailable");
    }
    check_time(saved_at_ms)?;
    Ok(NewOperation {
        family_id: identity.family,
        operation_id: identity.operation,
        record_id: activity.id,
        scope: Scope::Activity,
        kind: Kind::Set,
        author_device_id: identity.device,
        hlc: identity.stamp,
        record_type: None,
        child_id: None,
        fields: Some(temperature_fields(entered, unit)?),
    })
}

pub(super) fn whole_measure(value: i64, unit: i128) -> Value {
    Value::Map(vec![
        (1, Value::Integer(value.into())),
        (2, Value::Text(value.to_string())),
        (3, Value::Integer(unit)),
    ])
}
