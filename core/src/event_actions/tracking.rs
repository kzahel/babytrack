//! Portable tracking construction; IDs and clocks come from the caller.

use super::*;

pub fn solids_operation(
    identity: Identity,
    child_id: [u8; 16],
    foods: &[String],
    amount: &str,
    time: ActivityTime,
) -> Result<([u8; 16], NewOperation), &'static str> {
    activity_operation(
        identity,
        child_id,
        "feed.solids",
        solids_fields(foods, amount)?,
        time,
    )
}

fn solids_fields(foods: &[String], amount: &str) -> Result<Vec<(u64, Value)>, &'static str> {
    let normalized: Vec<_> = foods.iter().map(|food| food.trim()).collect();
    let amount = amount.trim();
    if normalized.is_empty()
        || normalized.len() > 32
        || normalized
            .iter()
            .any(|food| food.is_empty() || food.len() > 256)
        || amount.len() > 256
    {
        return Err("solids foods or amount invalid");
    }
    Ok(vec![
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
    ])
}

pub fn edit_solids_operation(
    identity: Identity,
    child_id: [u8; 16],
    activity: &Record,
    foods: &[String],
    amount: &str,
    saved_at_ms: i64,
) -> Result<NewOperation, &'static str> {
    if activity.scope != Scope::Activity
        || activity.child_id != Some(child_id)
        || activity.record_type != "feed.solids"
        || activity.deleted
    {
        return Err("solids target unavailable");
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
        fields: Some(solids_fields(foods, amount)?),
    })
}

pub fn medication_operation(
    identity: Identity,
    child_id: [u8; 16],
    name: &str,
    dose_amount: &str,
    dose_unit: &str,
    time: ActivityTime,
) -> Result<([u8; 16], NewOperation), &'static str> {
    activity_operation(
        identity,
        child_id,
        "medication",
        medication_fields(name, dose_amount, dose_unit)?,
        time,
    )
}

fn medication_fields(
    name: &str,
    dose_amount: &str,
    dose_unit: &str,
) -> Result<Vec<(u64, Value)>, &'static str> {
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
        return Err("medication name or dose empty or too long");
    }
    Ok(vec![
        (100, Value::Text(name.to_owned())),
        (
            101,
            Value::Array(vec![
                Value::Text(dose_amount.to_owned()),
                Value::Text(dose_unit.to_owned()),
            ]),
        ),
    ])
}

pub fn edit_medication_operation(
    identity: Identity,
    child_id: [u8; 16],
    activity: &Record,
    name: &str,
    dose_amount: &str,
    dose_unit: &str,
    saved_at_ms: i64,
) -> Result<NewOperation, &'static str> {
    if activity.scope != Scope::Activity
        || activity.child_id != Some(child_id)
        || activity.record_type != "medication"
        || activity.deleted
    {
        return Err("medication target unavailable");
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
        fields: Some(medication_fields(name, dose_amount, dose_unit)?),
    })
}

pub(super) fn activity_operation(
    identity: Identity,
    child_id: [u8; 16],
    record_type: &str,
    fields: Vec<(u64, Value)>,
    time: ActivityTime,
) -> Result<([u8; 16], NewOperation), &'static str> {
    check_time(time.saved_at_ms)?;
    if !(-840..=840).contains(&time.offset_minutes) {
        return Err("recorded offset outside v1 range");
    }
    let id = identity.record;
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
            family_id: identity.family,
            operation_id: identity.operation,
            record_id: id,
            scope: Scope::Activity,
            kind: Kind::Create,
            author_device_id: identity.device,
            hlc: identity.stamp,
            record_type: Some(record_type.to_owned()),
            child_id: Some(child_id),
            fields: Some(all_fields),
        },
    ))
}
