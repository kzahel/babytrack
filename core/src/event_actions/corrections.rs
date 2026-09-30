//! Portable corrections construction; IDs and clocks come from the caller.

use super::*;

pub fn delete_activity_operation(
    identity: Identity,
    child_id: [u8; 16],
    activity: &Record,
    saved_at_ms: i64,
) -> Result<NewOperation, &'static str> {
    if activity.scope != Scope::Activity || activity.child_id != Some(child_id) || activity.deleted
    {
        return Err("activity target unavailable");
    }
    check_time(saved_at_ms)?;
    Ok(NewOperation {
        family_id: identity.family,
        operation_id: identity.operation,
        record_id: activity.id,
        scope: Scope::Activity,
        kind: Kind::Delete,
        author_device_id: identity.device,
        hlc: identity.stamp,
        record_type: None,
        child_id: None,
        fields: None,
    })
}

pub fn restore_activity_operation(
    identity: Identity,
    child_id: [u8; 16],
    activity: &Record,
    saved_at_ms: i64,
) -> Result<NewOperation, &'static str> {
    if activity.scope != Scope::Activity || activity.child_id != Some(child_id) || !activity.deleted
    {
        return Err("deleted activity target unavailable");
    }
    check_time(saved_at_ms)?;
    Ok(NewOperation {
        family_id: identity.family,
        operation_id: identity.operation,
        record_id: activity.id,
        scope: Scope::Activity,
        kind: Kind::Restore,
        author_device_id: identity.device,
        hlc: identity.stamp,
        record_type: None,
        child_id: None,
        fields: None,
    })
}

pub fn edit_note_operation(
    identity: Identity,
    child_id: [u8; 16],
    activity: &Record,
    note: &str,
    saved_at_ms: i64,
) -> Result<NewOperation, &'static str> {
    if activity.scope != Scope::Activity
        || activity.child_id != Some(child_id)
        || activity.deleted
        || !matches!(
            activity.record_type.as_str(),
            "note"
                | "feed.breast"
                | "feed.bottle"
                | "feed.solids"
                | "sleep"
                | "pump"
                | "diaper"
                | "growth"
                | "medication"
                | "temperature"
        )
    {
        return Err("note target unavailable");
    }
    check_time(saved_at_ms)?;
    let note = note.trim();
    if (note.is_empty() && activity.record_type == "note") || note.len() > 4096 {
        return Err("note outside supported length");
    }
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
        fields: Some(vec![(
            4,
            if note.is_empty() {
                Value::Null
            } else {
                Value::Text(note.to_owned())
            },
        )]),
    })
}

/// Correct the recorded start of an instantaneous entry. Interval activities
/// keep their own start/end and segment editing rules.
pub fn edit_instant_time_operation(
    identity: Identity,
    child_id: [u8; 16],
    activity: &Record,
    start_utc_ms: i64,
    offset_minutes: i16,
    saved_at_ms: i64,
) -> Result<NewOperation, &'static str> {
    if activity.scope != Scope::Activity
        || activity.child_id != Some(child_id)
        || activity.deleted
        || !matches!(
            activity.record_type.as_str(),
            "note"
                | "feed.bottle"
                | "feed.solids"
                | "diaper"
                | "growth"
                | "medication"
                | "temperature"
        )
    {
        return Err("instant activity target unavailable");
    }
    check_time(saved_at_ms)?;
    if start_utc_ms < 0 || start_utc_ms > saved_at_ms {
        return Err("activity time must not be in the future");
    }
    if !(-840..=840).contains(&offset_minutes) {
        return Err("recorded offset outside v1 range");
    }
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
        fields: Some(vec![(
            1,
            Value::Array(vec![
                Value::Integer(start_utc_ms.into()),
                Value::Integer(offset_minutes.into()),
            ]),
        )]),
    })
}

pub fn edit_bottle_ml_operation(
    identity: Identity,
    child_id: [u8; 16],
    activity: &Record,
    amount_ml: i64,
    saved_at_ms: i64,
) -> Result<NewOperation, &'static str> {
    edit_bottle_measure_operation(
        identity,
        child_id,
        activity,
        &amount_ml.to_string(),
        1,
        saved_at_ms,
    )
}

fn edit_bottle_measure_operation(
    identity: Identity,
    child_id: [u8; 16],
    activity: &Record,
    entered: &str,
    unit: u8,
    saved_at_ms: i64,
) -> Result<NewOperation, &'static str> {
    if activity.scope != Scope::Activity
        || activity.child_id != Some(child_id)
        || activity.record_type != "feed.bottle"
        || activity.deleted
    {
        return Err("bottle target unavailable");
    }
    check_time(saved_at_ms)?;
    let measure = bottle_measure(entered, unit)?;
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
        fields: Some(vec![(100, measure)]),
    })
}

pub fn edit_bottle_entered_operation(
    identity: Identity,
    child_id: [u8; 16],
    activity: &Record,
    entered: &str,
    unit: u8,
    content: u8,
    saved_at_ms: i64,
) -> Result<NewOperation, &'static str> {
    if !(1..=4).contains(&content) {
        return Err("bottle content invalid");
    }
    let mut operation =
        edit_bottle_measure_operation(identity, child_id, activity, entered, unit, saved_at_ms)?;
    operation
        .fields
        .as_mut()
        .expect("bottle edit has fields")
        .push((101, Value::Integer(content.into())));
    Ok(operation)
}

pub fn edit_bottle_operation(
    identity: Identity,
    child_id: [u8; 16],
    activity: &Record,
    amount_ml: i64,
    content: u8,
    saved_at_ms: i64,
) -> Result<NewOperation, &'static str> {
    edit_bottle_entered_operation(
        identity,
        child_id,
        activity,
        &amount_ml.to_string(),
        1,
        content,
        saved_at_ms,
    )
}

pub fn edit_diaper_kind_operation(
    identity: Identity,
    child_id: [u8; 16],
    activity: &Record,
    kind: u8,
    saved_at_ms: i64,
) -> Result<NewOperation, &'static str> {
    if activity.scope != Scope::Activity
        || activity.child_id != Some(child_id)
        || activity.record_type != "diaper"
        || activity.deleted
    {
        return Err("diaper target unavailable");
    }
    check_time(saved_at_ms)?;
    if !(1..=4).contains(&kind) {
        return Err("diaper kind outside published codes");
    }
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
        fields: Some(vec![(100, Value::Integer(kind.into()))]),
    })
}
