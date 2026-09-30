use babytrack_core::{
    cbor::Value,
    day_summary::{DayWindow, summarize_day},
    event_actions::{
        self as actions, ActivityTime, GrowthInput, Identity, MeasurementInput, PumpAmounts,
    },
    operation::{Hlc, NewOperation, Operation},
    projection::LocalProjection,
    read_model::activities_from_records,
};

const NOW: i64 = 1_790_000_000_000;
const FAMILY: [u8; 16] = v4(0x41);
const DEVICE: [u8; 16] = v4(0x42);

const fn v4(tag: u8) -> [u8; 16] {
    let mut id = [tag; 16];
    id[6] = 0x40;
    id[8] = 0x80;
    id
}

fn record(tag: u8) -> [u8; 16] {
    let mut id = [tag; 16];
    id[6] = 0x70;
    id[8] = 0x80;
    id
}

fn identity(tag: u8, target: u8) -> Identity {
    Identity {
        family: FAMILY,
        device: DEVICE,
        operation: record(tag),
        record: record(target),
        stamp: Hlc {
            wall_ms: NOW,
            counter: tag.into(),
            device_id: DEVICE,
        },
    }
}

fn append(projection: &mut LocalProjection, operation: NewOperation, index: u64) {
    let bytes = Operation::encode_new(&operation).unwrap();
    let operation = Operation::decode_bound(&bytes, &FAMILY, &DEVICE).unwrap();
    projection.append(&operation, index).unwrap();
}

fn projection() -> LocalProjection {
    let mut projection = LocalProjection::new(FAMILY);
    append(
        &mut projection,
        actions::child(identity(1, 1), "Baby", None, None).unwrap(),
        1,
    );
    projection
}

fn time(start_utc_ms: i64, saved_at_ms: i64) -> ActivityTime {
    ActivityTime {
        start_utc_ms,
        offset_minutes: 120,
        saved_at_ms,
    }
}

#[test]
fn portable_measurement_corrections_preserve_untouched_and_unknown_fields() {
    let mut projection = projection();
    let input = GrowthInput {
        weight: Some(MeasurementInput {
            entered: "4.2".into(),
            unit: 11,
        }),
        length: None,
        head: Some(MeasurementInput {
            entered: "38".into(),
            unit: 21,
        }),
    };
    let (_, mut create) =
        actions::growth_entered_operation(identity(2, 2), record(1), &input, time(NOW, NOW))
            .unwrap();
    create
        .fields
        .as_mut()
        .unwrap()
        .push((999, Value::Text("future value".into())));
    append(&mut projection, create, 2);
    let target = projection.record(&record(2)).unwrap().clone();
    let correction = GrowthInput {
        weight: None,
        length: Some(MeasurementInput {
            entered: "21".into(),
            unit: 22,
        }),
        head: None,
    };
    assert!(
        actions::edit_growth_entered_operation(
            identity(3, 2),
            record(9),
            &target,
            &correction,
            NOW
        )
        .is_err()
    );
    append(
        &mut projection,
        actions::edit_growth_entered_operation(
            identity(3, 2),
            record(1),
            &target,
            &correction,
            NOW,
        )
        .unwrap(),
        3,
    );
    let revised = projection.record(&record(2)).unwrap();
    assert_eq!(revised.field(100), target.field(100));
    assert_eq!(revised.field(102), target.field(102));
    assert_eq!(revised.field(999), target.field(999));
    let rows = activities_from_records(projection.records());
    assert_eq!(rows[0].growth_weight_g, Some(4200));
    assert_eq!(rows[0].growth_length_mm, Some(533));
    assert_eq!(rows[0].growth_length_entered.as_deref(), Some("21"));
    assert_eq!(rows[0].growth_length_unit, Some(22));
    assert_eq!(rows[0].growth_head_mm, Some(380));

    let (_, temperature) = actions::temperature_entered_operation(
        identity(4, 4),
        record(1),
        "98.6",
        31,
        time(NOW, NOW),
    )
    .unwrap();
    append(&mut projection, temperature, 4);
    let rows = activities_from_records(projection.records());
    let row = rows.iter().find(|row| row.kind == "temperature").unwrap();
    assert_eq!(row.temperature_entered.as_deref(), Some("98.6"));
    assert_eq!(row.temperature_unit, Some(31));
    assert_eq!(row.temperature_c.as_deref(), Some("37.00"));
}

#[test]
fn portable_pump_correction_clears_alternative_amount_without_moving_interval() {
    let mut projection = projection();
    let (_, pump) = actions::pump_operation(
        identity(2, 2),
        record(1),
        PumpAmounts {
            left_ml: None,
            right_ml: None,
            total_ml: Some(80),
        },
        time(NOW - 60_000, NOW),
        NOW,
    )
    .unwrap();
    append(&mut projection, pump, 2);
    let target = projection.record(&record(2)).unwrap().clone();
    let correction = actions::edit_pump_amounts_operation(
        identity(3, 2),
        record(1),
        &target,
        PumpAmounts {
            left_ml: Some(40),
            right_ml: Some(30),
            total_ml: None,
        },
        NOW,
    )
    .unwrap();
    append(&mut projection, correction, 3);
    let revised = projection.record(&record(2)).unwrap();
    assert_eq!(revised.field(1), target.field(1));
    assert_eq!(revised.field(2), target.field(2));
    assert_eq!(revised.field(102).unwrap().value, Value::Null);
    let rows = activities_from_records(projection.records());
    assert_eq!(rows[0].pump_left_ml, Some(40));
    assert_eq!(rows[0].pump_right_ml, Some(30));
    assert_eq!(rows[0].pump_total_ml, None);
}

#[test]
fn portable_day_summary_uses_actual_midnights_and_observation_cutoff() {
    for hours in [23, 25] {
        let mut projection = projection();
        let end = NOW + hours * 3_600_000;
        let (_, sleep) = actions::sleep_operation(
            identity(2, 2),
            record(1),
            time(NOW - 3_600_000, end + 3_600_000),
            end + 3_600_000,
            60,
        )
        .unwrap();
        append(&mut projection, sleep, 2);
        // Running sleep only counts until the observation time, even on a long day.
        let (_, running) =
            actions::running_sleep_operation(identity(3, 3), record(1), time(end - 3_600_000, end))
                .unwrap();
        append(&mut projection, running, 3);
        let (_, before) = actions::solids_operation(
            identity(4, 4),
            record(1),
            &["pear".into()],
            "some",
            time(NOW - 1, end),
        )
        .unwrap();
        append(&mut projection, before, 4);
        let feed = actions::bottle(identity(5, 5), record(1), "120", 1, 2, NOW, 120).unwrap();
        append(&mut projection, feed, 5);
        let (_, after) = actions::solids_operation(
            identity(6, 6),
            record(1),
            &["pear".into()],
            "some",
            time(end, end),
        )
        .unwrap();
        append(&mut projection, after, 6);
        let window = DayWindow {
            start_utc_ms: NOW,
            end_utc_ms: end,
            through_utc_ms: end - 30 * 60_000,
        };
        let rows = activities_from_records(projection.records());
        let summary = summarize_day(rows.clone(), record(1), window).unwrap();
        assert_eq!(summary.sleep_ms, (hours * 3_600_000) as u64);
        assert_eq!(summary.feed_count, 1);
        assert_eq!(summary.bottle_ml, 120);
        assert_eq!(
            summarize_day(rows.clone(), record(9), window)
                .unwrap()
                .sleep_ms,
            0
        );
        assert!(
            summarize_day(
                rows,
                record(1),
                DayWindow {
                    end_utc_ms: NOW,
                    ..window
                }
            )
            .is_err()
        );
    }
}
