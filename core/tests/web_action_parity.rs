#![cfg(not(target_arch = "wasm32"))]

//! Every browser JSON action must build the same canonical bytes as the
//! native adapter for the same intent.

use babytrack_core::{
    event_actions::{GrowthInput, MeasurementInput, PumpAmounts},
    local_api::{self as native, ActivityTime, BreastSegment},
    operation::{Hlc, NewOperation, Operation},
    projection::{LocalProjection, Record},
    sqlite_store::FamilyHandle,
    web_actions::{
        self, Identity,
        action::{Action, build},
    },
};
use serde_json::{Value, json};

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

fn hex(id: &[u8; 16]) -> String {
    id.iter().map(|byte| format!("{byte:02x}")).collect()
}

struct Check {
    projection: LocalProjection,
    index: u64,
}

impl Check {
    fn record(&self, id: [u8; 16]) -> Record {
        self.projection.record(&id).unwrap().clone()
    }

    /// Build the browser form of `native` from `action` and append it.
    fn same(&mut self, mut native: NewOperation, action: Value, target: Option<[u8; 16]>) {
        native.hlc = Hlc {
            wall_ms: NOW,
            counter: 0,
            device_id: native.author_device_id,
        };
        let identity = Identity {
            family: native.family_id,
            device: native.author_device_id,
            operation: native.operation_id,
            record: native.record_id,
            stamp: native.hlc.clone(),
        };
        let target = target.map(|id| self.record(id));
        let parsed = Action::parse(&action.to_string()).unwrap();
        assert_eq!(
            parsed.target().map(str::to_owned),
            target.as_ref().map(|record| hex(&record.id))
        );
        let browser = build(identity, parsed, target.as_ref()).unwrap();
        let expected = Operation::encode_new(&native).unwrap();
        assert_eq!(browser, expected, "browser action differs: {action}");
        let operation =
            Operation::decode_bound(&expected, &native.family_id, &native.author_device_id)
                .unwrap();
        self.index += 1;
        self.projection.append(&operation, self.index).unwrap();
    }
}

#[test]
fn every_browser_action_matches_native_bytes() {
    let family = family();
    let mut check = Check {
        projection: LocalProjection::new(family.family_id),
        index: 0,
    };
    let (child, op) =
        native::child_operation_with_metadata(family, "Baby", Some(20_000), Some(1), NOW).unwrap();
    check.same(
        op,
        json!({"type":"child","name":"Baby","birthDay":20_000,"sex":1}),
        None,
    );
    let c = hex(&child);
    let when = ActivityTime {
        start_utc_ms: NOW - 3_600_000,
        offset_minutes: 120,
        saved_at_ms: NOW,
    };
    let start = when.start_utc_ms;
    let at = |extra: Value| {
        let mut value = json!({"child": c, "startMs": start, "offset": 120});
        value
            .as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        value
    };

    let op = native::rename_child_operation(family, &check.record(child), "Robin", NOW).unwrap();
    check.same(
        op,
        json!({"type":"renameChild","target":c,"name":"Robin"}),
        Some(child),
    );
    let op =
        native::edit_child_metadata_operation(family, &check.record(child), None, Some(3), NOW)
            .unwrap();
    check.same(
        op,
        json!({"type":"childMetadata","target":c,"birthDay":null,"sex":3}),
        Some(child),
    );

    let (diaper, op) = native::diaper_operation(family, child, 3, when).unwrap();
    check.same(op, at(json!({"type":"diaper","kind":3})), None);
    let (bottle, op) = native::bottle_entered_operation(family, child, "4.5", 2, 2, when).unwrap();
    check.same(
        op,
        at(json!({"type":"bottle","entered":"4.5","unit":2,"content":2})),
        None,
    );
    let (note, op) = native::note_operation(family, child, " Hello ", when).unwrap();
    check.same(op, at(json!({"type":"note","text":" Hello "})), None);
    let segments = vec![BreastSegment {
        side: 1,
        start_utc_ms: start,
        end_utc_ms: start + 600_000,
        start_offset_minutes: 120,
        end_offset_minutes: 120,
    }];
    let (breast, op) =
        native::breast_feed_segments_operation(family, child, &segments, when).unwrap();
    check.same(
        op,
        json!({"type":"breast","child":c,"segments":segments}),
        None,
    );
    let (sleep, op) =
        native::sleep_operation_with_place(family, child, when, start + 1_800_000, 60, Some(2))
            .unwrap();
    check.same(
        op,
        at(json!({"type":"sleep","endMs":start + 1_800_000,"endOffset":60,"place":2})),
        None,
    );
    let (running, op) =
        native::running_sleep_operation_with_place(family, child, when, None).unwrap();
    check.same(op, at(json!({"type":"sleep"})), None);
    let amounts = PumpAmounts {
        left_ml: Some(40),
        right_ml: Some(35),
        total_ml: None,
    };
    let (pump, op) = native::pump_operation(family, child, amounts, when, start + 900_000).unwrap();
    check.same(
        op,
        at(json!({"type":"pump","leftMl":40,"rightMl":35,"endMs":start + 900_000})),
        None,
    );
    let foods = vec![" pear ".to_owned(), "oats".to_owned()];
    let (solids, op) = native::solids_operation(family, child, &foods, " some ", when).unwrap();
    check.same(
        op,
        at(json!({"type":"solids","foods":foods,"amount":" some "})),
        None,
    );
    let input = GrowthInput {
        weight: Some(MeasurementInput {
            entered: "5.25".into(),
            unit: 12,
        }),
        length: Some(MeasurementInput {
            entered: "58".into(),
            unit: 22,
        }),
        head: None,
    };
    let (growth, op) = native::growth_entered_operation(family, child, &input, when).unwrap();
    check.same(
        op,
        at(
            json!({"type":"growth","weight":{"entered":"5.25","unit":12},
            "length":{"entered":"58","unit":22}}),
        ),
        None,
    );
    let (temperature, op) =
        native::temperature_entered_operation(family, child, "98.6", 31, when).unwrap();
    check.same(
        op,
        at(json!({"type":"temperature","entered":"98.6","unit":31})),
        None,
    );
    let (medication, op) =
        native::medication_operation(family, child, " vitamin D ", "1", " drop ", when).unwrap();
    check.same(
        op,
        at(json!({"type":"medication","name":" vitamin D ","doseAmount":"1","doseUnit":" drop "})),
        None,
    );

    let target = |id: &[u8; 16]| hex(id);
    let op =
        native::edit_note_operation(family, child, &check.record(note), "Changed", NOW).unwrap();
    check.same(
        op,
        json!({"type":"editNote","child":c,"target":target(&note),"text":"Changed"}),
        Some(note),
    );
    let op = native::edit_instant_time_operation(
        family,
        child,
        &check.record(diaper),
        start - 60_000,
        60,
        NOW,
    )
    .unwrap();
    check.same(
        op,
        json!({"type":"editTime","child":c,"target":target(&diaper),"startMs":start - 60_000,"offset":60}),
        Some(diaper),
    );
    let op =
        native::edit_diaper_kind_operation(family, child, &check.record(diaper), 1, NOW).unwrap();
    check.same(
        op,
        json!({"type":"editDiaper","child":c,"target":target(&diaper),"kind":1}),
        Some(diaper),
    );
    let op = native::edit_bottle_entered_operation(
        family,
        child,
        &check.record(bottle),
        "120",
        1,
        1,
        NOW,
    )
    .unwrap();
    check.same(
        op,
        json!({"type":"editBottle","child":c,"target":target(&bottle),"entered":"120","unit":1,"content":1}),
        Some(bottle),
    );
    let op = native::edit_solids_operation(
        family,
        child,
        &check.record(solids),
        &["rice".to_owned()],
        "",
        NOW,
    )
    .unwrap();
    check.same(
        op,
        json!({"type":"editSolids","child":c,"target":target(&solids),"foods":["rice"],"amount":""}),
        Some(solids),
    );
    let edit = GrowthInput {
        weight: None,
        length: None,
        head: Some(MeasurementInput {
            entered: "38".into(),
            unit: 21,
        }),
    };
    let op =
        native::edit_growth_entered_operation(family, child, &check.record(growth), &edit, NOW)
            .unwrap();
    check.same(
        op,
        json!({"type":"editGrowth","child":c,"target":target(&growth),"head":{"entered":"38","unit":21}}),
        Some(growth),
    );
    let total = PumpAmounts {
        left_ml: None,
        right_ml: None,
        total_ml: Some(90),
    };
    let op = native::edit_pump_amounts_operation(family, child, &check.record(pump), total, NOW)
        .unwrap();
    check.same(
        op,
        json!({"type":"editPump","child":c,"target":target(&pump),"totalMl":90}),
        Some(pump),
    );
    let op = native::edit_temperature_entered_operation(
        family,
        child,
        &check.record(temperature),
        "37.5",
        30,
        NOW,
    )
    .unwrap();
    check.same(
        op,
        json!({"type":"editTemperature","child":c,"target":target(&temperature),"entered":"37.5","unit":30}),
        Some(temperature),
    );
    let op = native::edit_medication_operation(
        family,
        child,
        &check.record(medication),
        "Iron",
        "2",
        "ml",
        NOW,
    )
    .unwrap();
    check.same(
        op,
        json!({"type":"editMedication","child":c,"target":target(&medication),"name":"Iron","doseAmount":"2","doseUnit":"ml"}),
        Some(medication),
    );
    let mut longer = segments.clone();
    longer[0].start_utc_ms -= 60_000;
    let op = native::edit_breast_feed_segments_operation(
        family,
        child,
        &check.record(breast),
        &longer,
        NOW,
    )
    .unwrap();
    check.same(
        op,
        json!({"type":"editBreast","child":c,"target":target(&breast),"segments":longer}),
        Some(breast),
    );
    let op = native::edit_sleep_end_operation(
        family,
        child,
        &check.record(sleep),
        start + 2_400_000,
        120,
        NOW,
    )
    .unwrap();
    check.same(
        op,
        json!({"type":"editSleepEnd","child":c,"target":target(&sleep),"endMs":start + 2_400_000,"endOffset":120}),
        Some(sleep),
    );
    let op =
        native::edit_sleep_place_operation(family, child, &check.record(sleep), None, NOW).unwrap();
    check.same(
        op,
        json!({"type":"editSleepPlace","child":c,"target":target(&sleep),"place":null}),
        Some(sleep),
    );
    let moved = ActivityTime {
        start_utc_ms: start - 600_000,
        ..when
    };
    let op = native::move_completed_interval_operation(
        family,
        child,
        &check.record(sleep),
        moved,
        start + 1_800_000,
        120,
    )
    .unwrap();
    check.same(
        op,
        json!({"type":"moveInterval","child":c,"target":target(&sleep),"startMs":start - 600_000,
            "offset":120,"endMs":start + 1_800_000,"endOffset":120}),
        Some(sleep),
    );
    let op =
        native::stop_sleep_operation(family, child, &check.record(running), NOW, 120, NOW).unwrap();
    check.same(
        op,
        json!({"type":"stopSleep","child":c,"target":target(&running),"endMs":NOW,"endOffset":120}),
        Some(running),
    );
    let op = native::delete_activity_operation(family, child, &check.record(note), NOW).unwrap();
    check.same(
        op,
        json!({"type":"delete","child":c,"target":target(&note)}),
        Some(note),
    );
    let op = native::restore_activity_operation(family, child, &check.record(note), NOW).unwrap();
    check.same(
        op,
        json!({"type":"restore","child":c,"target":target(&note)}),
        Some(note),
    );

    let snapshot: Value =
        serde_json::from_str(&web_actions::local_snapshot(&check.projection)).unwrap();
    let row = |id: &[u8; 16]| {
        snapshot["activities"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["id"] == hex(id))
            .unwrap()
            .clone()
    };
    assert_eq!(row(&bottle)["bottleEntered"], "120");
    assert_eq!(row(&pump)["pumpTotalMl"], 90);
    assert_eq!(row(&growth)["growthHeadEntered"], "38");
    assert_eq!(row(&temperature)["temperatureUnit"], 30);
    assert_eq!(row(&medication)["medicationName"], "Iron");
    assert_eq!(row(&sleep)["endMs"], start + 1_800_000);
    assert!(row(&sleep).get("sleepPlace").is_none());
    assert_eq!(row(&note)["note"], "Changed");
    assert!(row(&running).get("endMs").is_some());
}

#[test]
fn browser_actions_reject_bad_input_and_wrong_targets() {
    assert!(Action::parse("{\"type\":\"diaper\"}").is_err());
    assert!(Action::parse("{\"type\":\"unknown\"}").is_err());
    assert!(
        Action::parse(
            &json!({"type":"note","child":"00","text":"x","startMs":1,"offset":0,"extra":1})
                .to_string()
        )
        .is_err()
    );
    let family = family();
    let (child, op) = native::child_operation(family, "Baby", NOW).unwrap();
    let identity = |record| Identity {
        family: family.family_id,
        device: family.device_id,
        operation: [0x44; 16],
        record,
        stamp: Hlc {
            wall_ms: NOW,
            counter: 0,
            device_id: family.device_id,
        },
    };
    let action = |value: Value| Action::parse(&value.to_string()).unwrap();
    let c = hex(&child);
    assert!(
        build(
            identity([0x55; 16]),
            action(json!({"type":"diaper","child":"zz","kind":1,"startMs":NOW,"offset":0})),
            None
        )
        .is_err()
    );
    assert!(
        build(
            identity([0x55; 16]),
            action(json!({"type":"diaper","child":c,"kind":1,"startMs":-1,"offset":0})),
            None
        )
        .is_err()
    );
    assert!(
        build(
            identity([0x55; 16]),
            action(json!({"type":"editNote","child":c,"target":c,"text":"x"})),
            None
        )
        .is_err()
    );
    let mut projection = LocalProjection::new(family.family_id);
    let bytes = Operation::encode_new(&op).unwrap();
    projection
        .append(
            &Operation::decode_bound(&bytes, &family.family_id, &family.device_id).unwrap(),
            1,
        )
        .unwrap();
    let child_record = projection.record(&child).unwrap();
    assert!(
        build(
            identity(child),
            action(json!({"type":"editNote","child":c,"target":c,"text":"x"})),
            Some(child_record)
        )
        .is_err(),
        "a child record is not a note"
    );
}
