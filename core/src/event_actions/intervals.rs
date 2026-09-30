//! Portable intervals construction; IDs and clocks come from the caller.

use super::*;

pub fn pump_operation(
    identity: Identity,
    child_id: [u8; 16],
    amounts: PumpAmounts,
    time: ActivityTime,
    end_utc_ms: i64,
) -> Result<([u8; 16], NewOperation), &'static str> {
    if end_utc_ms < time.start_utc_ms || end_utc_ms > time.saved_at_ms {
        return Err("pump interval invalid");
    }
    let mut fields = vec![(
        2,
        Value::Array(vec![
            Value::Integer(end_utc_ms.into()),
            Value::Integer(time.offset_minutes.into()),
        ]),
    )];
    fields.extend(pump_amount_fields(amounts, false)?);
    activity_operation(identity, child_id, "pump", fields, time)
}

fn pump_amount_fields(
    amounts: PumpAmounts,
    clear_missing: bool,
) -> Result<Vec<(u64, Value)>, &'static str> {
    let PumpAmounts {
        left_ml,
        right_ml,
        total_ml,
    } = amounts;
    if total_ml.is_some() && (left_ml.is_some() || right_ml.is_some())
        || total_ml.is_none() && left_ml.unwrap_or(0) <= 0 && right_ml.unwrap_or(0) <= 0
        || [left_ml, right_ml, total_ml]
            .into_iter()
            .flatten()
            .any(|amount| !(0..=1_000_000).contains(&amount))
        || total_ml == Some(0)
    {
        return Err("pump amounts invalid");
    }
    Ok([(100, left_ml), (101, right_ml), (102, total_ml)]
        .into_iter()
        .filter_map(|(id, amount)| {
            amount
                .map(|value| (id, whole_measure(value, 1)))
                .or_else(|| clear_missing.then_some((id, Value::Null)))
        })
        .collect())
}

pub fn edit_pump_amounts_operation(
    identity: Identity,
    child_id: [u8; 16],
    activity: &Record,
    amounts: PumpAmounts,
    saved_at_ms: i64,
) -> Result<NewOperation, &'static str> {
    if activity.scope != Scope::Activity
        || activity.child_id != Some(child_id)
        || activity.record_type != "pump"
        || activity.deleted
    {
        return Err("pump target unavailable");
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
        fields: Some(pump_amount_fields(amounts, true)?),
    })
}

pub fn sleep_operation(
    identity: Identity,
    child_id: [u8; 16],
    time: ActivityTime,
    end_utc_ms: i64,
    end_offset_minutes: i16,
) -> Result<([u8; 16], NewOperation), &'static str> {
    sleep_operation_with_place(
        identity,
        child_id,
        time,
        end_utc_ms,
        end_offset_minutes,
        None,
    )
}

pub fn sleep_operation_with_place(
    identity: Identity,
    child_id: [u8; 16],
    time: ActivityTime,
    end_utc_ms: i64,
    end_offset_minutes: i16,
    place: Option<u8>,
) -> Result<([u8; 16], NewOperation), &'static str> {
    if end_utc_ms < time.start_utc_ms || end_utc_ms > time.saved_at_ms {
        return Err("sleep end outside completed interval");
    }
    if !(-840..=840).contains(&end_offset_minutes) {
        return Err("sleep end offset outside v1 range");
    }
    let mut fields = vec![(
        2,
        Value::Array(vec![
            Value::Integer(end_utc_ms.into()),
            Value::Integer(end_offset_minutes.into()),
        ]),
    )];
    fields.extend(sleep_place_field(place)?);
    activity_operation(identity, child_id, "sleep", fields, time)
}

pub fn running_sleep_operation(
    identity: Identity,
    child_id: [u8; 16],
    time: ActivityTime,
) -> Result<([u8; 16], NewOperation), &'static str> {
    running_sleep_operation_with_place(identity, child_id, time, None)
}

pub fn running_sleep_operation_with_place(
    identity: Identity,
    child_id: [u8; 16],
    time: ActivityTime,
    place: Option<u8>,
) -> Result<([u8; 16], NewOperation), &'static str> {
    activity_operation(identity, child_id, "sleep", sleep_place_field(place)?, time)
}

fn sleep_place_field(place: Option<u8>) -> Result<Vec<(u64, Value)>, &'static str> {
    match place {
        Some(code @ 1..=5) => Ok(vec![(100, Value::Integer(code.into()))]),
        Some(_) => Err("sleep place outside v1 range"),
        None => Ok(Vec::new()),
    }
}

pub fn edit_sleep_place_operation(
    identity: Identity,
    child_id: [u8; 16],
    activity: &Record,
    place: Option<u8>,
    saved_at_ms: i64,
) -> Result<NewOperation, &'static str> {
    if activity.scope != Scope::Activity
        || activity.record_type != "sleep"
        || activity.child_id != Some(child_id)
        || activity.deleted
    {
        return Err("sleep activity unavailable");
    }
    check_time(saved_at_ms)?;
    let value = match place {
        Some(code @ 1..=5) => Value::Integer(code.into()),
        Some(_) => return Err("sleep place outside v1 range"),
        None => Value::Null,
    };
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
        fields: Some(vec![(100, value)]),
    })
}

pub fn stop_sleep_operation(
    identity: Identity,
    child_id: [u8; 16],
    activity: &Record,
    end_utc_ms: i64,
    end_offset_minutes: i16,
    saved_at_ms: i64,
) -> Result<NewOperation, &'static str> {
    if activity.scope != Scope::Activity
        || activity.record_type != "sleep"
        || activity.child_id != Some(child_id)
        || activity.deleted
        || activity
            .field(2)
            .is_some_and(|field| field.value != Value::Null)
    {
        return Err("sleep target is not running");
    }
    let Value::Array(start) = &activity.field(1).ok_or("sleep start absent")?.value else {
        return Err("sleep start invalid");
    };
    let [Value::Integer(start_utc_ms), Value::Integer(_)] = start.as_slice() else {
        return Err("sleep start invalid");
    };
    let start_utc_ms = i64::try_from(*start_utc_ms).map_err(|_| "sleep start outside i64 range")?;
    if end_utc_ms < start_utc_ms || end_utc_ms > saved_at_ms {
        return Err("sleep end outside completed interval");
    }
    if !(-840..=840).contains(&end_offset_minutes) {
        return Err("sleep end offset outside v1 range");
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
        fields: Some(vec![(
            2,
            Value::Array(vec![
                Value::Integer(end_utc_ms.into()),
                Value::Integer(end_offset_minutes.into()),
            ]),
        )]),
    })
}

pub fn edit_sleep_end_operation(
    identity: Identity,
    child_id: [u8; 16],
    activity: &Record,
    end_utc_ms: i64,
    end_offset_minutes: i16,
    saved_at_ms: i64,
) -> Result<NewOperation, &'static str> {
    if activity.scope != Scope::Activity
        || activity.record_type != "sleep"
        || activity.child_id != Some(child_id)
        || activity.deleted
        || !matches!(
            activity.field(2).map(|field| &field.value),
            Some(Value::Array(_))
        )
    {
        return Err("completed sleep target unavailable");
    }
    let Value::Array(start) = &activity.field(1).ok_or("sleep start absent")?.value else {
        return Err("sleep start invalid");
    };
    let [Value::Integer(start_utc_ms), Value::Integer(_)] = start.as_slice() else {
        return Err("sleep start invalid");
    };
    let start_utc_ms = i64::try_from(*start_utc_ms).map_err(|_| "sleep start outside i64 range")?;
    if end_utc_ms <= start_utc_ms || end_utc_ms > saved_at_ms {
        return Err("sleep end outside completed interval");
    }
    if !(-840..=840).contains(&end_offset_minutes) {
        return Err("sleep end offset outside v1 range");
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
        fields: Some(vec![(
            2,
            Value::Array(vec![
                Value::Integer(end_utc_ms.into()),
                Value::Integer(end_offset_minutes.into()),
            ]),
        )]),
    })
}

/// Move one completed sleep or pump interval in a single field-set operation.
/// The caller chooses both endpoint offsets; Android preserves the duration.
pub fn move_completed_interval_operation(
    identity: Identity,
    child_id: [u8; 16],
    activity: &Record,
    time: ActivityTime,
    end_utc_ms: i64,
    end_offset_minutes: i16,
) -> Result<NewOperation, &'static str> {
    if activity.scope != Scope::Activity
        || activity.child_id != Some(child_id)
        || activity.deleted
        || !matches!(activity.record_type.as_str(), "sleep" | "pump")
        || !matches!(
            activity.field(2).map(|field| &field.value),
            Some(Value::Array(_))
        )
    {
        return Err("completed interval target unavailable");
    }
    check_time(time.saved_at_ms)?;
    if time.start_utc_ms < 0 || end_utc_ms <= time.start_utc_ms || end_utc_ms > time.saved_at_ms {
        return Err("completed interval time invalid");
    }
    if !(-840..=840).contains(&time.offset_minutes) || !(-840..=840).contains(&end_offset_minutes) {
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
        fields: Some(vec![
            (
                1,
                Value::Array(vec![
                    Value::Integer(time.start_utc_ms.into()),
                    Value::Integer(time.offset_minutes.into()),
                ]),
            ),
            (
                2,
                Value::Array(vec![
                    Value::Integer(end_utc_ms.into()),
                    Value::Integer(end_offset_minutes.into()),
                ]),
            ),
        ]),
    })
}
