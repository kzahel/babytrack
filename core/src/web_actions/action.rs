//! One JSON intent for every browser create and correction. JavaScript names
//! the action and its entered values; Rust validates and builds the bytes
//! through the same portable builders the native adapters use.

use super::{Error, Identity};
use crate::{
    breast::Segment,
    event_actions::{self as actions, ActivityTime, GrowthInput, MeasurementInput, PumpAmounts},
    operation::{NewOperation, Operation},
    projection::Record,
};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Measure {
    pub entered: String,
    pub unit: u8,
}

#[derive(Debug, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum Action {
    Child {
        name: String,
        birth_day: Option<i64>,
        sex: Option<u8>,
    },
    RenameChild {
        target: String,
        name: String,
    },
    ChildMetadata {
        target: String,
        birth_day: Option<i64>,
        sex: Option<u8>,
    },
    Diaper {
        child: String,
        kind: u8,
        start_ms: i64,
        offset: i16,
    },
    Bottle {
        child: String,
        entered: String,
        unit: u8,
        content: u8,
        start_ms: i64,
        offset: i16,
    },
    Note {
        child: String,
        text: String,
        start_ms: i64,
        offset: i16,
    },
    Breast {
        child: String,
        segments: Vec<Segment>,
    },
    Sleep {
        child: String,
        start_ms: i64,
        offset: i16,
        end_ms: Option<i64>,
        end_offset: Option<i16>,
        place: Option<u8>,
    },
    Pump {
        child: String,
        left_ml: Option<i64>,
        right_ml: Option<i64>,
        total_ml: Option<i64>,
        start_ms: i64,
        offset: i16,
        end_ms: i64,
    },
    Solids {
        child: String,
        foods: Vec<String>,
        amount: String,
        start_ms: i64,
        offset: i16,
    },
    Growth {
        child: String,
        weight: Option<Measure>,
        length: Option<Measure>,
        head: Option<Measure>,
        start_ms: i64,
        offset: i16,
    },
    Temperature {
        child: String,
        entered: String,
        unit: u8,
        start_ms: i64,
        offset: i16,
    },
    Medication {
        child: String,
        name: String,
        dose_amount: String,
        dose_unit: String,
        start_ms: i64,
        offset: i16,
    },
    Delete {
        child: String,
        target: String,
    },
    Restore {
        child: String,
        target: String,
    },
    EditNote {
        child: String,
        target: String,
        text: String,
    },
    EditTime {
        child: String,
        target: String,
        start_ms: i64,
        offset: i16,
    },
    EditBottle {
        child: String,
        target: String,
        entered: String,
        unit: u8,
        content: u8,
    },
    EditDiaper {
        child: String,
        target: String,
        kind: u8,
    },
    EditSolids {
        child: String,
        target: String,
        foods: Vec<String>,
        amount: String,
    },
    EditGrowth {
        child: String,
        target: String,
        weight: Option<Measure>,
        length: Option<Measure>,
        head: Option<Measure>,
    },
    EditPump {
        child: String,
        target: String,
        left_ml: Option<i64>,
        right_ml: Option<i64>,
        total_ml: Option<i64>,
    },
    EditTemperature {
        child: String,
        target: String,
        entered: String,
        unit: u8,
    },
    EditMedication {
        child: String,
        target: String,
        name: String,
        dose_amount: String,
        dose_unit: String,
    },
    EditBreast {
        child: String,
        target: String,
        segments: Vec<Segment>,
    },
    StopSleep {
        child: String,
        target: String,
        end_ms: i64,
        end_offset: i16,
    },
    EditSleepEnd {
        child: String,
        target: String,
        end_ms: i64,
        end_offset: i16,
    },
    EditSleepPlace {
        child: String,
        target: String,
        place: Option<u8>,
    },
    MoveInterval {
        child: String,
        target: String,
        start_ms: i64,
        offset: i16,
        end_ms: i64,
        end_offset: i16,
    },
}

impl Action {
    pub fn parse(input: &str) -> Result<Self, Error> {
        if input.len() > 64 * 1024 {
            return Err(Error::Invalid("action JSON too long"));
        }
        serde_json::from_str(input).map_err(|_| Error::Invalid("action JSON invalid"))
    }

    /// The entered start of a new activity, checked like the typed builders.
    fn created_start(&self) -> Option<i64> {
        use Action::*;
        match self {
            Diaper { start_ms, .. }
            | Bottle { start_ms, .. }
            | Note { start_ms, .. }
            | Sleep { start_ms, .. }
            | Pump { start_ms, .. }
            | Solids { start_ms, .. }
            | Growth { start_ms, .. }
            | Temperature { start_ms, .. }
            | Medication { start_ms, .. } => Some(*start_ms),
            Breast { segments, .. } => segments.first().map(|first| first.start_utc_ms),
            _ => None,
        }
    }

    /// The existing record a correction changes, as lowercase hex.
    pub fn target(&self) -> Option<&str> {
        use Action::*;
        match self {
            RenameChild { target, .. }
            | ChildMetadata { target, .. }
            | Delete { target, .. }
            | Restore { target, .. }
            | EditNote { target, .. }
            | EditTime { target, .. }
            | EditBottle { target, .. }
            | EditDiaper { target, .. }
            | EditSolids { target, .. }
            | EditGrowth { target, .. }
            | EditPump { target, .. }
            | EditTemperature { target, .. }
            | EditMedication { target, .. }
            | EditBreast { target, .. }
            | StopSleep { target, .. }
            | EditSleepEnd { target, .. }
            | EditSleepPlace { target, .. }
            | MoveInterval { target, .. } => Some(target),
            _ => None,
        }
    }
}

pub fn parse_id(value: &str) -> Result<[u8; 16], Error> {
    const INVALID: &str = "record ID must be 32 hex digits";
    if value.len() != 32 || !value.is_ascii() {
        return Err(Error::Invalid(INVALID));
    }
    let mut id = [0u8; 16];
    for (index, byte) in id.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16)
            .map_err(|_| Error::Invalid(INVALID))?;
    }
    Ok(id)
}

fn growth(weight: Option<Measure>, length: Option<Measure>, head: Option<Measure>) -> GrowthInput {
    let input = |measure: Option<Measure>| {
        measure.map(|value| MeasurementInput {
            entered: value.entered,
            unit: value.unit,
        })
    };
    GrowthInput {
        weight: input(weight),
        length: input(length),
        head: input(head),
    }
}

/// Build one canonical operation. For a create, `id.record` is the new
/// record ID; for a correction, `target` is the current record named by
/// [`Action::target`] and the operation is bound to it.
pub fn build(id: Identity, action: Action, target: Option<&Record>) -> Result<Vec<u8>, Error> {
    use Action::*;
    let saved = id.stamp.wall_ms;
    if let Some(start) = action.created_start() {
        super::check_activity_time(start, saved)?;
    }
    let when = |start_utc_ms, offset_minutes| ActivityTime {
        start_utc_ms,
        offset_minutes,
        saved_at_ms: saved,
    };
    let child = |value: &str| parse_id(value);
    let id = match target.map(|record| record.id) {
        Some(record) => Identity { record, ..id },
        None => id,
    };
    let invalid = Error::Invalid;
    let target = || target.ok_or(Error::Invalid("correction target unavailable"));
    let created = |result: Result<([u8; 16], NewOperation), &'static str>| result.map(|(_, op)| op);
    let operation = match action {
        Child {
            name,
            birth_day,
            sex,
        } => actions::child(id, &name, birth_day, sex),
        RenameChild { name, .. } => actions::rename_child_operation(id, target()?, &name, saved),
        ChildMetadata { birth_day, sex, .. } => {
            actions::edit_child_metadata_operation(id, target()?, birth_day, sex, saved)
        }
        Diaper {
            child: c,
            kind,
            start_ms,
            offset,
        } => actions::diaper(id, child(&c)?, kind, start_ms, offset),
        Bottle {
            child: c,
            entered,
            unit,
            content,
            start_ms,
            offset,
        } => actions::bottle(id, child(&c)?, &entered, unit, content, start_ms, offset),
        Note {
            child: c,
            text,
            start_ms,
            offset,
        } => actions::note(id, child(&c)?, &text, start_ms, offset),
        Breast { child: c, segments } => {
            let first = segments
                .first()
                .ok_or(invalid("breast segment count or start invalid"))?;
            let (start, offset) = (first.start_utc_ms, first.start_offset_minutes);
            actions::breast(id, child(&c)?, &segments, start, offset, saved)
        }
        Sleep {
            child: c,
            start_ms,
            offset,
            end_ms,
            end_offset,
            place,
        } => created(match end_ms {
            Some(end) => actions::sleep_operation_with_place(
                id,
                child(&c)?,
                when(start_ms, offset),
                end,
                end_offset.unwrap_or(offset),
                place,
            ),
            None => actions::running_sleep_operation_with_place(
                id,
                child(&c)?,
                when(start_ms, offset),
                place,
            ),
        }),
        Pump {
            child: c,
            left_ml,
            right_ml,
            total_ml,
            start_ms,
            offset,
            end_ms,
        } => created(actions::pump_operation(
            id,
            child(&c)?,
            PumpAmounts {
                left_ml,
                right_ml,
                total_ml,
            },
            when(start_ms, offset),
            end_ms,
        )),
        Solids {
            child: c,
            foods,
            amount,
            start_ms,
            offset,
        } => created(actions::solids_operation(
            id,
            child(&c)?,
            &foods,
            &amount,
            when(start_ms, offset),
        )),
        Growth {
            child: c,
            weight,
            length,
            head,
            start_ms,
            offset,
        } => created(actions::growth_entered_operation(
            id,
            child(&c)?,
            &growth(weight, length, head),
            when(start_ms, offset),
        )),
        Temperature {
            child: c,
            entered,
            unit,
            start_ms,
            offset,
        } => created(actions::temperature_entered_operation(
            id,
            child(&c)?,
            &entered,
            unit,
            when(start_ms, offset),
        )),
        Medication {
            child: c,
            name,
            dose_amount,
            dose_unit,
            start_ms,
            offset,
        } => created(actions::medication_operation(
            id,
            child(&c)?,
            &name,
            &dose_amount,
            &dose_unit,
            when(start_ms, offset),
        )),
        Delete { child: c, .. } => {
            actions::delete_activity_operation(id, child(&c)?, target()?, saved)
        }
        Restore { child: c, .. } => {
            actions::restore_activity_operation(id, child(&c)?, target()?, saved)
        }
        EditNote { child: c, text, .. } => {
            actions::edit_note_operation(id, child(&c)?, target()?, &text, saved)
        }
        EditTime {
            child: c,
            start_ms,
            offset,
            ..
        } => {
            actions::edit_instant_time_operation(id, child(&c)?, target()?, start_ms, offset, saved)
        }
        EditBottle {
            child: c,
            entered,
            unit,
            content,
            ..
        } => actions::edit_bottle_entered_operation(
            id,
            child(&c)?,
            target()?,
            &entered,
            unit,
            content,
            saved,
        ),
        EditDiaper { child: c, kind, .. } => {
            actions::edit_diaper_kind_operation(id, child(&c)?, target()?, kind, saved)
        }
        EditSolids {
            child: c,
            foods,
            amount,
            ..
        } => actions::edit_solids_operation(id, child(&c)?, target()?, &foods, &amount, saved),
        EditGrowth {
            child: c,
            weight,
            length,
            head,
            ..
        } => actions::edit_growth_entered_operation(
            id,
            child(&c)?,
            target()?,
            &growth(weight, length, head),
            saved,
        ),
        EditPump {
            child: c,
            left_ml,
            right_ml,
            total_ml,
            ..
        } => actions::edit_pump_amounts_operation(
            id,
            child(&c)?,
            target()?,
            PumpAmounts {
                left_ml,
                right_ml,
                total_ml,
            },
            saved,
        ),
        EditTemperature {
            child: c,
            entered,
            unit,
            ..
        } => actions::edit_temperature_entered_operation(
            id,
            child(&c)?,
            target()?,
            &entered,
            unit,
            saved,
        ),
        EditMedication {
            child: c,
            name,
            dose_amount,
            dose_unit,
            ..
        } => actions::edit_medication_operation(
            id,
            child(&c)?,
            target()?,
            &name,
            &dose_amount,
            &dose_unit,
            saved,
        ),
        EditBreast {
            child: c, segments, ..
        } => actions::edit_breast(id, child(&c)?, target()?, &segments, saved),
        StopSleep {
            child: c,
            end_ms,
            end_offset,
            ..
        } => actions::stop_sleep_operation(id, child(&c)?, target()?, end_ms, end_offset, saved),
        EditSleepEnd {
            child: c,
            end_ms,
            end_offset,
            ..
        } => {
            actions::edit_sleep_end_operation(id, child(&c)?, target()?, end_ms, end_offset, saved)
        }
        EditSleepPlace {
            child: c, place, ..
        } => actions::edit_sleep_place_operation(id, child(&c)?, target()?, place, saved),
        MoveInterval {
            child: c,
            start_ms,
            offset,
            end_ms,
            end_offset,
            ..
        } => actions::move_completed_interval_operation(
            id,
            child(&c)?,
            target()?,
            when(start_ms, offset),
            end_ms,
            end_offset,
        ),
    }
    .map_err(invalid)?;
    Ok(Operation::encode_new(&operation)?)
}
