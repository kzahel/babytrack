//! Portable child construction; IDs and clocks come from the caller.

use super::*;

pub fn rename_child_operation(
    identity: Identity,
    child: &Record,
    name: &str,
    saved_at_ms: i64,
) -> Result<NewOperation, &'static str> {
    if child.scope != Scope::Child || child.record_type != "child" || child.deleted {
        return Err("target child unavailable");
    }
    check_time(saved_at_ms)?;
    let name = name.trim();
    if name.is_empty() || name.len() > 16 * 1024 {
        return Err("child name empty or too long");
    }
    Ok(NewOperation {
        family_id: identity.family,
        operation_id: identity.operation,
        record_id: child.id,
        scope: Scope::Child,
        kind: Kind::Set,
        author_device_id: identity.device,
        hlc: identity.stamp,
        record_type: None,
        child_id: None,
        fields: Some(vec![(1, Value::Text(name.to_owned()))]),
    })
}

pub fn edit_child_metadata_operation(
    identity: Identity,
    child: &Record,
    birth_day: Option<i64>,
    sex: Option<u8>,
    saved_at_ms: i64,
) -> Result<NewOperation, &'static str> {
    if child.scope != Scope::Child || child.record_type != "child" || child.deleted {
        return Err("target child unavailable");
    }
    check_time(saved_at_ms)?;
    if sex.is_some_and(|code| !(1..=3).contains(&code)) {
        return Err("child sex code outside published range");
    }
    let mut fields = Vec::new();
    if let Some(day) = birth_day {
        fields.push((2, Value::Integer(day.into())));
    }
    if let Some(code) = sex {
        fields.push((3, Value::Integer(code.into())));
    }
    if fields.is_empty() {
        return Err("child metadata correction is empty");
    }
    Ok(NewOperation {
        family_id: identity.family,
        operation_id: identity.operation,
        record_id: child.id,
        scope: Scope::Child,
        kind: Kind::Set,
        author_device_id: identity.device,
        hlc: identity.stamp,
        record_type: None,
        child_id: None,
        fields: Some(fields),
    })
}
