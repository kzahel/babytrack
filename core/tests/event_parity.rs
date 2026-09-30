#![cfg(not(target_arch = "wasm32"))]

use babytrack_core::{
    local_api::{self, ActivityTime, BreastSegment},
    operation::{Hlc, NewOperation, Operation},
    projection::LocalProjection,
    sqlite_store::FamilyHandle,
    web_actions::{self, Identity},
};

const NOW: i64 = 1_790_000_000_000;

fn family() -> FamilyHandle {
    let mut family_id = [0x11; 16];
    family_id[6] = 0x41;
    family_id[8] = 0x81;
    let mut device_id = [0x22; 16];
    device_id[6] = 0x42;
    device_id[8] = 0x82;
    FamilyHandle {
        family_id,
        device_id,
    }
}

fn identity(operation: &NewOperation) -> Identity {
    Identity {
        family: operation.family_id,
        device: operation.author_device_id,
        operation: operation.operation_id,
        record: operation.record_id,
        stamp: Hlc {
            wall_ms: NOW,
            counter: 0,
            device_id: operation.author_device_id,
        },
    }
}

fn assert_bytes(mut native: NewOperation, browser: Vec<u8>) -> Operation {
    native.hlc = identity(&native).stamp;
    let expected = Operation::encode_new(&native).unwrap();
    assert_eq!(
        browser, expected,
        "adapters must construct identical canonical operations"
    );
    Operation::decode_bound(&expected, &native.family_id, &native.author_device_id).unwrap()
}

#[test]
fn native_and_browser_construct_and_display_the_same_records() {
    let family = family();
    let (child, native) =
        local_api::child_operation_with_metadata(family, " Baby ", Some(20_000), Some(2), NOW)
            .unwrap();
    let browser = web_actions::child(identity(&native), " Baby ", Some(20_000), Some(2)).unwrap();
    let mut projection = LocalProjection::new(family.family_id);
    projection
        .append(&assert_bytes(native, browser), 1)
        .unwrap();
    let when = ActivityTime {
        start_utc_ms: NOW - 60_000,
        offset_minutes: 120,
        saved_at_ms: NOW,
    };
    let mut cases = Vec::new();
    let (_, native) = local_api::diaper_operation(family, child, 4, when).unwrap();
    let browser = web_actions::diaper(identity(&native), child, 4, when.start_utc_ms, 120).unwrap();
    cases.push(assert_bytes(native, browser));
    let (_, native) = local_api::bottle_operation(family, child, 120, 2, when).unwrap();
    let browser =
        web_actions::bottle(identity(&native), child, 120, 2, when.start_utc_ms, 120).unwrap();
    cases.push(assert_bytes(native, browser));
    let (_, native) = local_api::note_operation(family, child, " Hello ", when).unwrap();
    let browser =
        web_actions::note(identity(&native), child, " Hello ", when.start_utc_ms, 120).unwrap();
    cases.push(assert_bytes(native, browser));
    let segments = vec![BreastSegment {
        side: 1,
        start_utc_ms: when.start_utc_ms,
        end_utc_ms: NOW,
        start_offset_minutes: 120,
        end_offset_minutes: 120,
    }];
    let input = serde_json::to_string(&segments).unwrap();
    let (feed, native) =
        local_api::breast_feed_segments_operation(family, child, &segments, when).unwrap();
    let browser = web_actions::breast(identity(&native), child, &input).unwrap();
    cases.push(assert_bytes(native, browser));
    for (index, operation) in cases.iter().enumerate() {
        projection.append(operation, index as u64 + 2).unwrap();
    }

    let target = projection.record(&feed).unwrap().clone();
    let mut corrected = segments.clone();
    corrected[0].side = 2;
    let native =
        local_api::edit_breast_feed_segments_operation(family, child, &target, &corrected, NOW)
            .unwrap();
    let browser = web_actions::edit_breast(
        identity(&native),
        child,
        &target,
        &serde_json::to_string(&corrected).unwrap(),
    )
    .unwrap();
    projection
        .append(&assert_bytes(native, browser), 6)
        .unwrap();
    let snapshot: serde_json::Value =
        serde_json::from_str(&web_actions::local_snapshot(&projection)).unwrap();
    let native_children = local_api::children_from_records(projection.records());
    assert_eq!(snapshot["children"][0]["name"], native_children[0].name);
    let activities = local_api::activities_from_records(projection.records());
    assert_eq!(
        snapshot["activities"].as_array().unwrap().len(),
        activities.len()
    );
    for activity in activities {
        let id: String = activity
            .id
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        let row = snapshot["activities"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["id"] == id)
            .unwrap();
        assert_eq!(row["startMs"], activity.start_utc_ms);
        match activity.kind.as_str() {
            "diaper" => assert_eq!(row["diaperKind"], activity.diaper_kind.unwrap()),
            "feed.bottle" => {
                assert_eq!(row["bottleMl"], activity.bottle_ml.unwrap());
                assert_eq!(row["bottleContent"], activity.bottle_content.unwrap());
            }
            "note" => assert_eq!(row["note"], activity.note.unwrap()),
            "feed.breast" => assert_eq!(
                row["breastSegments"],
                serde_json::to_value(activity.breast_segments.unwrap()).unwrap()
            ),
            _ => unreachable!(),
        }
    }
    assert!(
        local_api::edit_breast_feed_segments_operation(
            family, [0x33; 16], &target, &corrected, NOW
        )
        .is_err()
    );
}

#[test]
fn adapters_agree_on_trimmed_limits_and_invalid_fields() {
    let family = family();
    for name in [
        " ".to_owned(),
        "é".repeat(8193),
        " Baby ".to_owned(),
        format!(" {} ", "x".repeat(16 * 1024)),
    ] {
        let native = local_api::child_operation(family, &name, NOW);
        let probe = local_api::child_operation(family, "Probe", NOW).unwrap().1;
        assert_eq!(
            native.is_ok(),
            web_actions::child(identity(&probe), &name, None, None).is_ok()
        );
    }
    let child = local_api::child_operation(family, "Baby", NOW).unwrap().0;
    let when = ActivityTime {
        start_utc_ms: NOW,
        offset_minutes: 120,
        saved_at_ms: NOW,
    };
    let probe = local_api::note_operation(family, child, "Probe", when)
        .unwrap()
        .1;
    for kind in [0, 1, 4, 5] {
        assert_eq!(
            local_api::diaper_operation(family, child, kind, when).is_ok(),
            web_actions::diaper(identity(&probe), child, kind, NOW, 120).is_ok()
        );
    }
    for amount in [0, 1, 1_000_000, 1_000_001] {
        assert_eq!(
            local_api::bottle_operation(family, child, amount.into(), 2, when).is_ok(),
            web_actions::bottle(identity(&probe), child, amount, 2, NOW, 120).is_ok()
        );
    }
    for note in [" ".to_owned(), "x".repeat(4096), "x".repeat(4097)] {
        assert_eq!(
            local_api::note_operation(family, child, &note, when).is_ok(),
            web_actions::note(identity(&probe), child, &note, NOW, 120).is_ok()
        );
    }
}

#[test]
fn native_adapters_preserve_portable_measurement_and_interval_bytes() {
    use babytrack_core::event_actions::{
        self as actions, GrowthInput, MeasurementInput, PumpAmounts,
    };
    let family = family();
    let (child, native) = local_api::child_operation(family, "Baby", NOW).unwrap();
    let mut projection = LocalProjection::new(family.family_id);
    let operation = assert_bytes(
        native.clone(),
        web_actions::child(identity(&native), "Baby", None, None).unwrap(),
    );
    projection.append(&operation, 1).unwrap();
    let when = ActivityTime {
        start_utc_ms: NOW - 60_000,
        offset_minutes: 120,
        saved_at_ms: NOW,
    };
    let growth = GrowthInput {
        weight: Some(MeasurementInput {
            entered: "9.25".into(),
            unit: 12,
        }),
        length: Some(MeasurementInput {
            entered: "21".into(),
            unit: 22,
        }),
        head: None,
    };
    let amounts = PumpAmounts {
        left_ml: Some(40),
        right_ml: Some(30),
        total_ml: None,
    };
    let foods = vec![" pear ".into(), " banana ".into()];
    let mut index = 1;
    macro_rules! create {
        ($name:ident, $($argument:expr),+ $(,)?) => {{
            let (_, native) = local_api::$name(family, child, $($argument),+).unwrap();
            let (_, portable) = actions::$name(identity(&native), child, $($argument),+).unwrap();
            let operation = assert_bytes(native, Operation::encode_new(&portable).unwrap());
            index += 1;
            projection.append(&operation, index).unwrap();
            operation.record_id
        }};
    }
    let pump = create!(pump_operation, amounts, when, NOW);
    let sleep = create!(sleep_operation_with_place, when, NOW, 120, Some(1));
    create!(running_sleep_operation_with_place, when, Some(2));
    create!(solids_operation, &foods, " some ", when);
    let growth_id = create!(growth_entered_operation, &growth, when);
    let temperature = create!(temperature_entered_operation, "98.6", 31, when);
    create!(medication_operation, " vitamin ", " 1 ", " drop ", when);
    macro_rules! correct {
        ($name:ident, $target:expr, $($argument:expr),+ $(,)?) => {{
            let target = projection.record(&$target).unwrap().clone();
            let native = local_api::$name(family, child, &target, $($argument),+).unwrap();
            let portable = actions::$name(identity(&native), child, &target, $($argument),+).unwrap();
            let operation = assert_bytes(native, Operation::encode_new(&portable).unwrap());
            index += 1;
            projection.append(&operation, index).unwrap();
        }};
    }
    correct!(
        edit_pump_amounts_operation,
        pump,
        PumpAmounts {
            left_ml: None,
            right_ml: None,
            total_ml: Some(90)
        },
        NOW
    );
    correct!(
        edit_growth_entered_operation,
        growth_id,
        &GrowthInput {
            weight: None,
            length: None,
            head: Some(MeasurementInput {
                entered: "38".into(),
                unit: 21
            })
        },
        NOW
    );
    correct!(
        edit_temperature_entered_operation,
        temperature,
        "37.8",
        30,
        NOW
    );
    correct!(
        move_completed_interval_operation,
        sleep,
        ActivityTime {
            start_utc_ms: NOW - 120_000,
            ..when
        },
        NOW - 60_000,
        60
    );
    assert_eq!(index, 12);
}
