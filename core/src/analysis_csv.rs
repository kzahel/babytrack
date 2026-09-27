//! Human-readable current-state analysis export. This is not a backup: the
//! operation log, tombstones, and unknown fields remain in portable files.

use std::collections::HashMap;

use crate::{
    local_api::{activities_from_records, children_from_records},
    projection::Record,
};

const HEADER: &str = "family_id,child_id,child_name,child_birth_day,child_sex,activity_id,type,start_utc_ms,start_offset_minutes,end_utc_ms,note,diaper_kind,bottle_ml,breast_side,breast_segments,solids_foods,solids_amount,pump_left_ml,pump_right_ml,pump_total_ml,growth_weight_g,growth_length_mm,temperature_c,medication_name,medication_dose_amount,medication_dose_unit\r\n";

pub fn export<'a>(family_id: [u8; 16], records: impl Iterator<Item = &'a Record>) -> Vec<u8> {
    let records = records.collect::<Vec<_>>();
    let children = children_from_records(records.iter().copied())
        .into_iter()
        .map(|child| (child.id, child))
        .collect::<HashMap<_, _>>();
    let activities = activities_from_records(records.into_iter());
    let mut csv = String::from(HEADER);
    for activity in activities {
        let child = children.get(&activity.child_id);
        let segments = activity.breast_segments.as_ref().map(|items| {
            serde_json::to_string(
                &items
                    .iter()
                    .map(|item| {
                        [
                            i64::from(item.side),
                            item.start_utc_ms,
                            item.end_utc_ms,
                            i64::from(item.start_offset_minutes),
                            i64::from(item.end_offset_minutes),
                        ]
                    })
                    .collect::<Vec<_>>(),
            )
            .expect("integer segments serialize")
        });
        let foods = activity
            .solids_foods
            .as_ref()
            .map(|items| serde_json::to_string(items).expect("string foods serialize"));
        let cells = [
            hex(family_id),
            hex(activity.child_id),
            child.map(|row| row.name.clone()).unwrap_or_default(),
            display(child.and_then(|row| row.birth_day)),
            display(child.and_then(|row| row.sex)),
            hex(activity.id),
            activity.kind,
            activity.start_utc_ms.to_string(),
            activity.offset_minutes.to_string(),
            display(activity.end_utc_ms),
            activity.note.unwrap_or_default(),
            display(activity.diaper_kind),
            display(activity.bottle_ml),
            display(activity.breast_side),
            segments.unwrap_or_default(),
            foods.unwrap_or_default(),
            activity.solids_amount.unwrap_or_default(),
            display(activity.pump_left_ml),
            display(activity.pump_right_ml),
            display(activity.pump_total_ml),
            display(activity.growth_weight_g),
            display(activity.growth_length_mm),
            activity.temperature_c.unwrap_or_default(),
            activity.medication_name.unwrap_or_default(),
            activity.medication_dose_amount.unwrap_or_default(),
            activity.medication_dose_unit.unwrap_or_default(),
        ];
        for (index, cell) in cells.iter().enumerate() {
            if index != 0 {
                csv.push(',');
            }
            // Spreadsheet applications can evaluate formula-looking text even
            // in quoted CSV cells. This export is for analysis, not byte-exact
            // restore, so prefix such input with an apostrophe.
            let safe = if matches!(index, 2 | 6 | 10 | 16 | 23 | 24 | 25)
                && cell
                    .trim_start()
                    .starts_with(['=', '+', '-', '@', '\t', '\r'])
            {
                format!("'{cell}")
            } else {
                cell.clone()
            };
            csv.push('"');
            csv.push_str(&safe.replace('"', "\"\""));
            csv.push('"');
        }
        csv.push_str("\r\n");
    }
    csv.into_bytes()
}

fn display(value: Option<impl std::fmt::Display>) -> String {
    value.map(|value| value.to_string()).unwrap_or_default()
}

fn hex(bytes: [u8; 16]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
