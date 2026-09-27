#![cfg(not(target_arch = "wasm32"))]

use babytrack_core::{
    local_api::{ActivityTime, LocalRepository, temperature_c_operation},
    portable_file::parse_readable,
};

#[test]
fn local_tracking_targets_explicit_family_and_survives_restart() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("phone.db");
    let mut app = LocalRepository::open(&path).unwrap();
    let first = app.create_family(1_790_000_000_000).unwrap();
    let second = app.create_family(1_790_000_000_001).unwrap();
    assert_eq!(app.families().unwrap().len(), 2);
    let child = app.add_child(first, " Baby ", 1_790_000_000_002).unwrap();
    assert_eq!(app.children(first).unwrap()[0].name, "Baby");
    let time = ActivityTime {
        start_utc_ms: 1_790_000_000_003,
        offset_minutes: 120,
        saved_at_ms: 1_790_000_000_004,
    };
    assert!(app.log_diaper(second, child, 1, time).is_err());
    app.log_diaper(first, child, 3, time).unwrap();
    app.log_bottle_ml(
        first,
        child,
        85,
        2,
        ActivityTime {
            saved_at_ms: 1_790_000_000_005,
            ..time
        },
    )
    .unwrap();
    assert!(
        app.log_sleep(first, child, time, time.start_utc_ms - 1, 120)
            .is_err()
    );
    app.log_sleep(
        first,
        child,
        ActivityTime {
            start_utc_ms: 1_790_000_000_000 - 3_600_000,
            saved_at_ms: 1_790_000_000_005,
            ..time
        },
        1_790_000_000_000,
        60,
    )
    .unwrap();
    assert!(app.log_note(first, child, "   ", time).is_err());
    app.log_note(
        first,
        child,
        "  A family note  ",
        ActivityTime {
            start_utc_ms: 1_790_000_000_004,
            saved_at_ms: 1_790_000_000_005,
            ..time
        },
    )
    .unwrap();
    let running = app
        .start_sleep(
            first,
            child,
            ActivityTime {
                start_utc_ms: 1_789_999_000_000,
                saved_at_ms: 1_790_000_000_005,
                ..time
            },
        )
        .unwrap();
    assert!(
        app.timeline(first, child)
            .unwrap()
            .iter()
            .any(|row| row.id == running && row.end_utc_ms.is_none())
    );
    drop(app);
    let mut app = LocalRepository::open(&path).unwrap();
    assert!(
        app.stop_sleep(
            first,
            second.device_id,
            running,
            1_790_000_000_005,
            120,
            1_790_000_000_006
        )
        .is_err()
    );
    assert!(
        app.stop_sleep(
            second,
            child,
            running,
            1_790_000_000_005,
            120,
            1_790_000_000_006
        )
        .is_err()
    );
    app.stop_sleep(
        first,
        child,
        running,
        1_790_000_000_005,
        120,
        1_790_000_000_006,
    )
    .unwrap();
    assert!(
        app.stop_sleep(
            first,
            child,
            running,
            1_790_000_000_005,
            120,
            1_790_000_000_006
        )
        .is_err()
    );
    let timeline = app.timeline(first, child).unwrap();
    assert_eq!(timeline.len(), 5);
    assert_eq!(
        timeline
            .iter()
            .find(|row| row.id == running)
            .unwrap()
            .end_utc_ms,
        Some(1_790_000_000_005)
    );
    assert_eq!(
        timeline
            .iter()
            .find(|row| row.kind == "feed.bottle")
            .unwrap()
            .bottle_ml,
        Some(85)
    );
    assert_eq!(
        timeline
            .iter()
            .find(|row| row.kind == "diaper")
            .unwrap()
            .diaper_kind,
        Some(3)
    );
    assert_eq!(
        timeline
            .iter()
            .find(|row| row.kind == "sleep" && row.id != running)
            .unwrap()
            .end_utc_ms,
        Some(1_790_000_000_000)
    );
    assert_eq!(
        timeline
            .iter()
            .find(|row| row.kind == "note")
            .unwrap()
            .note
            .as_deref(),
        Some("A family note")
    );
    let revision = app.revision(first).unwrap();
    let file = app.backup_file(first, 1_790_000_000_006, None, 0).unwrap();
    assert_eq!(file.revision, revision);
    assert_eq!(file.info.snapshot_utc_ms, 1_790_000_000_006);
    assert_eq!(file.info.record_count, 7);
    assert!(!file.info.known_gap);
    let backup = app.backup(first, 1_790_000_000_006).unwrap();
    assert_eq!(
        parse_readable(&backup).unwrap().source_family_id,
        first.family_id
    );
    drop(app);
    let mut app = LocalRepository::open(&path).unwrap();
    assert_eq!(app.timeline(first, child).unwrap(), timeline);
    let restored = app.restore(&backup, 1_790_000_000_007).unwrap();
    assert_ne!(restored.family_id, first.family_id);
    assert_eq!(
        app.restored_origin(restored)
            .unwrap()
            .unwrap()
            .snapshot_utc_ms,
        1_790_000_000_006
    );
    assert_eq!(app.timeline(restored, child).unwrap().len(), 5);
    assert_eq!(app.children(second).unwrap().len(), 0);

    let protected = app
        .protected_backup(first, 1_790_000_000_008, "correct", 512 * 1024 * 1024)
        .unwrap();
    assert!(protected.starts_with(b"BTBK1"));
    assert_eq!(
        LocalRepository::inspect_protected(&protected, "correct", 512 * 1024 * 1024)
            .unwrap()
            .snapshot_utc_ms,
        1_790_000_000_008
    );
    let before = app.families().unwrap().len();
    assert!(
        app.restore_protected(&protected, "wrong", 512 * 1024 * 1024, 1_790_000_000_009,)
            .is_err()
    );
    assert_eq!(app.families().unwrap().len(), before);
    let copy = app
        .restore_protected(&protected, "correct", 512 * 1024 * 1024, 1_790_000_000_010)
        .unwrap();
    assert_ne!(copy.family_id, first.family_id);
    assert_eq!(app.timeline(copy, child).unwrap().len(), 5);
}

#[test]
fn growth_measurements_survive_restart_and_file_restore() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("growth.db");
    let mut app = LocalRepository::open(&path).unwrap();
    let family = app.create_family(1_790_000_000_000).unwrap();
    let child = app.add_child(family, "Baby", 1_790_000_000_001).unwrap();
    let time = ActivityTime {
        start_utc_ms: 1_790_000_000_002,
        offset_minutes: 120,
        saved_at_ms: 1_790_000_000_002,
    };
    assert!(app.log_growth(family, child, None, None, time).is_err());
    assert!(app.log_growth(family, child, Some(0), None, time).is_err());
    assert!(
        app.log_growth(family, child, None, Some(2_501), time)
            .is_err()
    );
    let id = app
        .log_growth(family, child, Some(4_200), Some(540), time)
        .unwrap();
    let weight_only = app
        .log_growth(family, child, Some(4_300), None, time)
        .unwrap();
    let before = app.timeline(family, child).unwrap();
    assert_eq!(
        before
            .iter()
            .find(|row| row.id == id)
            .unwrap()
            .growth_weight_g,
        Some(4_200)
    );
    assert_eq!(
        before
            .iter()
            .find(|row| row.id == id)
            .unwrap()
            .growth_length_mm,
        Some(540)
    );
    assert_eq!(
        before
            .iter()
            .find(|row| row.id == weight_only)
            .unwrap()
            .growth_length_mm,
        None
    );
    let backup = app.backup(family, 1_790_000_000_003).unwrap();
    drop(app);
    let mut app = LocalRepository::open(&path).unwrap();
    assert_eq!(app.timeline(family, child).unwrap(), before);
    let restored = app.restore(&backup, 1_790_000_000_004).unwrap();
    assert_eq!(app.timeline(restored, child).unwrap(), before);
}

#[test]
fn entered_celsius_round_trips_through_core_and_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("temperature.db");
    let mut app = LocalRepository::open(&path).unwrap();
    let family = app.create_family(1_790_000_000_000).unwrap();
    let child = app.add_child(family, "Baby", 1_790_000_000_001).unwrap();
    let time = ActivityTime {
        start_utc_ms: 1_790_000_000_002,
        offset_minutes: 120,
        saved_at_ms: 1_790_000_000_002,
    };
    for invalid in ["", "37.", "037.5", "37,5", "1e3"] {
        assert!(app.log_temperature_c(family, child, invalid, time).is_err());
    }
    let (_, rounded) = temperature_c_operation(family, child, "37.505", time).unwrap();
    let fields = rounded.fields.unwrap();
    let (100, babytrack_core::cbor::Value::Map(measure)) = &fields[1] else {
        panic!("temperature measure missing");
    };
    assert_eq!(measure[0], (1, babytrack_core::cbor::Value::Integer(3751)));
    assert_eq!(
        measure[1],
        (2, babytrack_core::cbor::Value::Text("37.505".into()))
    );
    let (_, negative) = temperature_c_operation(family, child, "-0.005", time).unwrap();
    let negative_fields = negative.fields.unwrap();
    let (100, babytrack_core::cbor::Value::Map(negative_measure)) = &negative_fields[1] else {
        panic!("negative temperature measure missing");
    };
    assert_eq!(
        negative_measure[0],
        (1, babytrack_core::cbor::Value::Integer(-1))
    );
    let id = app.log_temperature_c(family, child, "37.50", time).unwrap();
    let before = app.timeline(family, child).unwrap();
    assert_eq!(
        before
            .iter()
            .find(|row| row.id == id)
            .unwrap()
            .temperature_c
            .as_deref(),
        Some("37.50")
    );
    let backup = app.backup(family, 1_790_000_000_003).unwrap();
    drop(app);
    let mut app = LocalRepository::open(&path).unwrap();
    assert_eq!(app.timeline(family, child).unwrap(), before);
    let restored = app.restore(&backup, 1_790_000_000_004).unwrap();
    assert_eq!(app.timeline(restored, child).unwrap(), before);
}

#[test]
fn medication_record_preserves_entered_name_and_dose_after_restore() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("medication.db");
    let mut app = LocalRepository::open(&path).unwrap();
    let family = app.create_family(1_790_000_000_000).unwrap();
    let child = app.add_child(family, "Baby", 1_790_000_000_001).unwrap();
    let time = ActivityTime {
        start_utc_ms: 1_790_000_000_002,
        offset_minutes: 120,
        saved_at_ms: 1_790_000_000_002,
    };
    assert!(
        app.log_medication(family, child, " ", "2.5", "mL", time)
            .is_err()
    );
    assert!(
        app.log_medication(family, child, "Test medicine", "", "mL", time)
            .is_err()
    );
    assert!(
        app.log_medication(family, child, "Test medicine", "2.5", "", time)
            .is_err()
    );
    let id = app
        .log_medication(family, child, " Test medicine ", " 2.5 ", " mL ", time)
        .unwrap();
    let before = app.timeline(family, child).unwrap();
    let row = before.iter().find(|row| row.id == id).unwrap();
    assert_eq!(row.medication_name.as_deref(), Some("Test medicine"));
    assert_eq!(row.medication_dose_amount.as_deref(), Some("2.5"));
    assert_eq!(row.medication_dose_unit.as_deref(), Some("mL"));
    let backup = app.backup(family, 1_790_000_000_003).unwrap();
    drop(app);
    let mut app = LocalRepository::open(&path).unwrap();
    assert_eq!(app.timeline(family, child).unwrap(), before);
    let restored = app.restore(&backup, 1_790_000_000_004).unwrap();
    assert_eq!(app.timeline(restored, child).unwrap(), before);
}

#[test]
fn solids_foods_and_amount_survive_restart_and_file_restore() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("solids.db");
    let mut app = LocalRepository::open(&path).unwrap();
    let family = app.create_family(1_790_000_000_000).unwrap();
    let child = app.add_child(family, "Baby", 1_790_000_000_001).unwrap();
    let time = ActivityTime {
        start_utc_ms: 1_790_000_000_002,
        offset_minutes: 120,
        saved_at_ms: 1_790_000_000_002,
    };
    assert!(app.log_solids(family, child, &[], "a spoon", time).is_err());
    assert!(
        app.log_solids(family, child, &["  ".into()], "", time)
            .is_err()
    );
    let id = app
        .log_solids(
            family,
            child,
            &[" Pear ".into(), "Oatmeal".into()],
            " two spoons ",
            time,
        )
        .unwrap();
    let before = app.timeline(family, child).unwrap();
    let row = before.iter().find(|row| row.id == id).unwrap();
    assert_eq!(
        row.solids_foods.as_deref(),
        Some(&["Pear".into(), "Oatmeal".into()][..])
    );
    assert_eq!(row.solids_amount.as_deref(), Some("two spoons"));
    let backup = app.backup(family, 1_790_000_000_003).unwrap();
    drop(app);
    let mut app = LocalRepository::open(&path).unwrap();
    assert_eq!(app.timeline(family, child).unwrap(), before);
    let restored = app.restore(&backup, 1_790_000_000_004).unwrap();
    assert_eq!(app.timeline(restored, child).unwrap(), before);
}
