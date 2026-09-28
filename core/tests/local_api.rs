#![cfg(not(target_arch = "wasm32"))]

use babytrack_core::{
    local_api::{
        ActivityTime, BreastSegment, DayWindow, GrowthInput, LocalRepository, MeasurementInput,
        PumpAmounts, temperature_c_operation,
    },
    portable_file::parse_readable,
};

#[test]
fn local_day_summary_splits_sleep_and_counts_only_selected_current_records() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("day-summary.db");
    let mut app = LocalRepository::open(&path).unwrap();
    let day = 1_790_000_000_000;
    let family = app.create_family(day - 3_600_000).unwrap();
    let child = app.add_child(family, "Baby", day - 3_599_999).unwrap();
    let other = app.add_child(family, "Other", day - 3_599_998).unwrap();
    let when = |start_utc_ms, saved_at_ms| ActivityTime {
        start_utc_ms,
        offset_minutes: 60,
        saved_at_ms,
    };
    app.log_sleep(
        family,
        child,
        when(day - 30 * 60_000, day + 31 * 60_000),
        day + 30 * 60_000,
        60,
    )
    .unwrap();
    app.start_sleep(
        family,
        child,
        when(day + 4 * 3_600_000, day + 4 * 3_600_000),
    )
    .unwrap();
    app.log_bottle_ml(
        family,
        child,
        120,
        2,
        when(day + 3_600_000, day + 3_600_001),
    )
    .unwrap();
    app.log_bottle_ml(family, other, 90, 2, when(day + 3_600_000, day + 3_600_002))
        .unwrap();
    app.log_diaper(
        family,
        child,
        3,
        when(day + 2 * 3_600_000, day + 2 * 3_600_000 + 1),
    )
    .unwrap();
    app.log_diaper(
        family,
        child,
        4,
        when(day + 3 * 3_600_000, day + 3 * 3_600_000 + 1),
    )
    .unwrap();
    let deleted = app
        .log_diaper(
            family,
            child,
            1,
            when(day + 4 * 3_600_000, day + 4 * 3_600_000 + 1),
        )
        .unwrap();
    app.delete_activity(family, child, deleted, day + 4 * 3_600_000 + 2)
        .unwrap();
    let window = DayWindow {
        start_utc_ms: day,
        end_utc_ms: day + 23 * 3_600_000,
        through_utc_ms: day + 5 * 3_600_000,
    };
    let summary = app.day_summary(family, child, window).unwrap();
    assert_eq!(summary.sleep_ms, 90 * 60_000);
    assert_eq!(summary.feed_count, 1);
    assert_eq!(summary.bottle_ml, 120);
    assert_eq!(summary.diaper_count, 2);
    assert_eq!(summary.wet_diaper_count, 1);
    assert_eq!(summary.dirty_diaper_count, 1);
    assert_eq!(
        app.day_summary(family, other, window).unwrap().bottle_ml,
        90
    );
    assert!(
        app.day_summary(
            family,
            child,
            DayWindow {
                end_utc_ms: day,
                ..window
            }
        )
        .is_err()
    );
    drop(app);
    let app = LocalRepository::open(&path).unwrap();
    assert_eq!(app.day_summary(family, child, window).unwrap(), summary);
}

#[test]
fn instant_time_correction_keeps_identity_and_survives_backup() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("instant-time.db");
    let mut app = LocalRepository::open(&path).unwrap();
    let family = app.create_family(1_790_000_000_000).unwrap();
    let other_family = app.create_family(1_790_000_000_001).unwrap();
    let child = app.add_child(family, "Baby", 1_790_000_000_002).unwrap();
    let other_child = app.add_child(family, "Other", 1_790_000_000_003).unwrap();
    let time = ActivityTime {
        start_utc_ms: 1_790_000_000_004,
        offset_minutes: 60,
        saved_at_ms: 1_790_000_000_005,
    };
    let diaper = app.log_diaper(family, child, 1, time).unwrap();
    let sleep = app
        .log_sleep(family, child, time, time.start_utc_ms + 1, 60)
        .unwrap();
    let saved_at = time.saved_at_ms + 2;
    for (target_family, target_child, target_activity, start, offset) in [
        (other_family, child, diaper, time.start_utc_ms - 60_000, 0),
        (family, other_child, diaper, time.start_utc_ms - 60_000, 0),
        (family, child, sleep, time.start_utc_ms - 60_000, 0),
        (family, child, diaper, saved_at + 1, 0),
        (family, child, diaper, time.start_utc_ms - 60_000, 900),
    ] {
        assert!(
            app.edit_instant_time(
                target_family,
                target_child,
                target_activity,
                start,
                offset,
                saved_at,
            )
            .is_err()
        );
    }
    let corrected = time.start_utc_ms - 3_600_000;
    app.edit_instant_time(family, child, diaper, corrected, -60, saved_at)
        .unwrap();
    let rows = app.timeline(family, child).unwrap();
    let row = rows.iter().find(|row| row.id == diaper).unwrap();
    assert_eq!(row.start_utc_ms, corrected);
    assert_eq!(row.offset_minutes, -60);
    assert_eq!(row.diaper_kind, Some(1));
    let backup = app.backup(family, saved_at + 1).unwrap();
    drop(app);
    let mut app = LocalRepository::open(&path).unwrap();
    assert_eq!(app.timeline(family, child).unwrap(), rows);
    let restored = app.restore(&backup, saved_at + 2).unwrap();
    assert_eq!(app.timeline(restored, child).unwrap(), rows);
}

#[test]
fn moving_completed_sleep_and_pump_keeps_duration_and_fields() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("move-interval.db");
    let mut app = LocalRepository::open(&path).unwrap();
    let family = app.create_family(1_790_000_000_000).unwrap();
    let other_family = app.create_family(1_790_000_000_001).unwrap();
    let child = app.add_child(family, "Baby", 1_790_000_000_002).unwrap();
    let other_child = app.add_child(family, "Other", 1_790_000_000_003).unwrap();
    let start = 1_790_000_000_004;
    let saved_at = start + 30 * 60_000;
    let time = ActivityTime {
        start_utc_ms: start,
        offset_minutes: 60,
        saved_at_ms: saved_at,
    };
    let sleep = app
        .log_sleep_with_place(family, child, time, start + 20 * 60_000, 60, Some(2))
        .unwrap();
    let pump = app
        .log_pump(
            family,
            child,
            PumpAmounts {
                left_ml: Some(20),
                right_ml: None,
                total_ml: None,
            },
            time,
            start + 10 * 60_000,
        )
        .unwrap();
    let running = app.start_sleep(family, child, time).unwrap();
    let moved = ActivityTime {
        start_utc_ms: start - 3_600_000,
        offset_minutes: -60,
        saved_at_ms: saved_at + 1,
    };
    for (target_family, target_child, target_activity, target_time, end) in [
        (
            other_family,
            child,
            sleep,
            moved,
            moved.start_utc_ms + 20 * 60_000,
        ),
        (
            family,
            other_child,
            sleep,
            moved,
            moved.start_utc_ms + 20 * 60_000,
        ),
        (
            family,
            child,
            running,
            moved,
            moved.start_utc_ms + 20 * 60_000,
        ),
        (family, child, sleep, moved, moved.start_utc_ms),
        (family, child, sleep, moved, moved.saved_at_ms + 1),
    ] {
        assert!(
            app.move_completed_interval(
                target_family,
                target_child,
                target_activity,
                target_time,
                end,
                -60,
            )
            .is_err()
        );
    }
    app.move_completed_interval(
        family,
        child,
        sleep,
        moved,
        moved.start_utc_ms + 20 * 60_000,
        -60,
    )
    .unwrap();
    app.move_completed_interval(
        family,
        child,
        pump,
        moved,
        moved.start_utc_ms + 10 * 60_000,
        -60,
    )
    .unwrap();
    let rows = app.timeline(family, child).unwrap();
    let sleep_row = rows.iter().find(|row| row.id == sleep).unwrap();
    assert_eq!(sleep_row.start_utc_ms, moved.start_utc_ms);
    assert_eq!(sleep_row.end_utc_ms, Some(moved.start_utc_ms + 20 * 60_000));
    assert_eq!(sleep_row.sleep_place, Some(2));
    let pump_row = rows.iter().find(|row| row.id == pump).unwrap();
    assert_eq!(pump_row.start_utc_ms, moved.start_utc_ms);
    assert_eq!(pump_row.end_utc_ms, Some(moved.start_utc_ms + 10 * 60_000));
    assert_eq!(pump_row.pump_left_ml, Some(20));
    let backup = app.backup(family, moved.saved_at_ms + 1).unwrap();
    drop(app);
    let mut app = LocalRepository::open(&path).unwrap();
    assert_eq!(app.timeline(family, child).unwrap(), rows);
    let restored = app.restore(&backup, moved.saved_at_ms + 2).unwrap();
    assert_eq!(app.timeline(restored, child).unwrap(), rows);
}

#[test]
fn child_birth_day_and_sex_survive_restart_and_file_restore() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("child-metadata.db");
    let mut app = LocalRepository::open(&path).unwrap();
    let family = app.create_family(1_790_000_000_000).unwrap();
    assert!(
        app.add_child_with_metadata(family, "Invalid", Some(20_000), Some(4), 1_790_000_000_001)
            .is_err()
    );
    assert!(app.children(family).unwrap().is_empty());
    let child = app
        .add_child_with_metadata(family, "Baby", Some(20_000), Some(1), 1_790_000_000_002)
        .unwrap();
    assert_eq!(app.children(family).unwrap()[0].id, child);
    assert_eq!(app.children(family).unwrap()[0].birth_day, Some(20_000));
    assert_eq!(app.children(family).unwrap()[0].sex, Some(1));
    let other = app.create_family(1_790_000_000_003).unwrap();
    assert!(
        app.rename_child(other, child, "Wrong", 1_790_000_000_004)
            .is_err()
    );
    assert!(
        app.rename_child(family, child, " ", 1_790_000_000_004)
            .is_err()
    );
    app.rename_child(family, child, " New name ", 1_790_000_000_004)
        .unwrap();
    assert_eq!(app.children(family).unwrap()[0].name, "New name");
    assert!(
        app.edit_child_metadata(other, child, Some(20_001), Some(2), 1_790_000_000_004)
            .is_err()
    );
    assert!(
        app.edit_child_metadata(family, child, None, None, 1_790_000_000_004)
            .is_err()
    );
    assert!(
        app.edit_child_metadata(family, child, None, Some(4), 1_790_000_000_004)
            .is_err()
    );
    app.edit_child_metadata(family, child, Some(20_001), Some(2), 1_790_000_000_004)
        .unwrap();
    assert_eq!(app.children(family).unwrap()[0].birth_day, Some(20_001));
    assert_eq!(app.children(family).unwrap()[0].sex, Some(2));
    let backup = app.backup(family, 1_790_000_000_005).unwrap();
    drop(app);

    let mut app = LocalRepository::open(&path).unwrap();
    assert_eq!(app.children(family).unwrap()[0].birth_day, Some(20_001));
    assert_eq!(app.children(family).unwrap()[0].name, "New name");
    let restored = app.restore(&backup, 1_790_000_000_006).unwrap();
    assert_eq!(app.children(restored).unwrap()[0].birth_day, Some(20_001));
    assert_eq!(app.children(restored).unwrap()[0].sex, Some(2));
    assert_eq!(app.children(restored).unwrap()[0].name, "New name");
}

#[test]
fn growth_correction_keeps_activity_and_survives_restore() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("growth-edit.db");
    let mut app = LocalRepository::open(&path).unwrap();
    let family = app.create_family(1_790_000_000_000).unwrap();
    let child = app.add_child(family, "Baby", 1_790_000_000_001).unwrap();
    let other_child = app.add_child(family, "Other", 1_790_000_000_002).unwrap();
    let time = ActivityTime {
        start_utc_ms: 1_790_000_000_003,
        offset_minutes: 60,
        saved_at_ms: 1_790_000_000_004,
    };
    let id = app
        .log_growth_measurements(family, child, Some(4_200), Some(540), Some(350), time)
        .unwrap();
    assert!(
        app.edit_growth(
            family,
            other_child,
            id,
            Some(4_300),
            None,
            time.saved_at_ms + 1
        )
        .is_err()
    );
    assert!(
        app.edit_growth(family, child, id, None, None, time.saved_at_ms + 1)
            .is_err()
    );
    assert!(
        app.edit_growth(family, child, id, Some(0), None, time.saved_at_ms + 1)
            .is_err()
    );
    assert!(
        app.edit_growth_measurements(
            family,
            child,
            id,
            None,
            None,
            Some(1_001),
            time.saved_at_ms + 1
        )
        .is_err()
    );
    app.edit_growth_measurements(
        family,
        child,
        id,
        Some(4_300),
        None,
        Some(355),
        time.saved_at_ms + 1,
    )
    .unwrap();
    let before = app.timeline(family, child).unwrap();
    assert_eq!(before[0].id, id);
    assert_eq!(before[0].growth_weight_g, Some(4_300));
    assert_eq!(before[0].growth_length_mm, Some(540));
    assert_eq!(before[0].growth_head_mm, Some(355));
    assert_eq!(before[0].start_utc_ms, time.start_utc_ms);
    let csv = String::from_utf8(app.analysis_csv(family).unwrap()).unwrap();
    assert!(csv.contains(",growth_head_mm,"));
    assert!(csv.contains("\"355\""));
    let backup = app.backup(family, time.saved_at_ms + 2).unwrap();
    drop(app);
    let mut app = LocalRepository::open(&path).unwrap();
    assert_eq!(app.timeline(family, child).unwrap(), before);
    let restored = app.restore(&backup, time.saved_at_ms + 3).unwrap();
    assert_eq!(app.timeline(restored, child).unwrap(), before);
}

#[test]
fn entered_growth_units_round_and_survive_edit_restart_and_restore() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("growth-entered.db");
    let mut app = LocalRepository::open(&path).unwrap();
    let family = app.create_family(1_790_000_000_000).unwrap();
    let child = app.add_child(family, "Baby", 1_790_000_000_001).unwrap();
    let time = ActivityTime {
        start_utc_ms: 1_790_000_000_002,
        offset_minutes: 60,
        saved_at_ms: 1_790_000_000_003,
    };
    let measure = |entered: &str, unit| MeasurementInput {
        entered: entered.to_owned(),
        unit,
    };
    let input = GrowthInput {
        weight: Some(measure("4.25", 11)),
        length: Some(measure("52.3", 21)),
        head: Some(measure("13.5", 22)),
    };
    let id = app.log_growth_entered(family, child, &input, time).unwrap();
    let first = &app.timeline(family, child).unwrap()[0];
    assert_eq!(first.growth_weight_g, Some(4_250));
    assert_eq!(first.growth_weight_entered.as_deref(), Some("4.25"));
    assert_eq!(first.growth_weight_unit, Some(11));
    assert_eq!(first.growth_length_mm, Some(523));
    assert_eq!(first.growth_length_entered.as_deref(), Some("52.3"));
    assert_eq!(first.growth_length_unit, Some(21));
    assert_eq!(first.growth_head_mm, Some(343));
    assert_eq!(first.growth_head_unit, Some(22));
    let invalid = GrowthInput {
        weight: Some(measure("4.25", 21)),
        length: None,
        head: None,
    };
    assert!(
        app.log_growth_entered(family, child, &invalid, time)
            .is_err()
    );
    let correction = GrowthInput {
        weight: Some(measure("9", 12)),
        length: None,
        head: Some(measure("35.6", 21)),
    };
    app.edit_growth_entered(family, child, id, &correction, time.saved_at_ms + 1)
        .unwrap();
    let before = app.timeline(family, child).unwrap();
    assert_eq!(before.len(), 1);
    assert_eq!(before[0].id, id);
    assert_eq!(before[0].growth_weight_g, Some(4_082));
    assert_eq!(before[0].growth_weight_entered.as_deref(), Some("9"));
    assert_eq!(before[0].growth_weight_unit, Some(12));
    assert_eq!(before[0].growth_length_mm, Some(523));
    assert_eq!(before[0].growth_length_entered.as_deref(), Some("52.3"));
    assert_eq!(before[0].growth_head_mm, Some(356));
    assert_eq!(before[0].growth_head_entered.as_deref(), Some("35.6"));
    let backup = app.backup(family, time.saved_at_ms + 2).unwrap();
    drop(app);
    let mut app = LocalRepository::open(&path).unwrap();
    assert_eq!(app.timeline(family, child).unwrap(), before);
    let restored = app.restore(&backup, time.saved_at_ms + 3).unwrap();
    assert_eq!(app.timeline(restored, child).unwrap(), before);
}

#[test]
fn completed_sleep_duration_correction_survives_restore() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sleep-edit.db");
    let mut app = LocalRepository::open(&path).unwrap();
    let family = app.create_family(1_790_000_000_000).unwrap();
    let child = app.add_child(family, "Baby", 1_790_000_000_001).unwrap();
    let other = app.add_child(family, "Other", 1_790_000_000_002).unwrap();
    let start = 1_790_000_000_003;
    let saved = start + 60 * 60_000;
    assert!(
        app.log_sleep_with_place(
            family,
            child,
            ActivityTime {
                start_utc_ms: start,
                offset_minutes: 60,
                saved_at_ms: saved
            },
            saved,
            60,
            Some(6),
        )
        .is_err()
    );
    let id = app
        .log_sleep_with_place(
            family,
            child,
            ActivityTime {
                start_utc_ms: start,
                offset_minutes: 60,
                saved_at_ms: saved,
            },
            saved,
            60,
            Some(1),
        )
        .unwrap();
    assert!(
        app.edit_sleep_end(family, other, id, start + 20 * 60_000, 60, saved + 1)
            .is_err()
    );
    assert!(
        app.edit_sleep_end(family, child, id, start, 60, saved + 1)
            .is_err()
    );
    assert!(
        app.edit_sleep_end(family, child, id, saved + 2, 60, saved + 1)
            .is_err()
    );
    app.edit_sleep_end(family, child, id, start + 45 * 60_000, 60, saved + 1)
        .unwrap();
    assert!(
        app.edit_sleep_place(family, other, id, Some(2), saved + 1)
            .is_err()
    );
    assert!(
        app.edit_sleep_place(family, child, id, Some(6), saved + 1)
            .is_err()
    );
    app.edit_sleep_place(family, child, id, None, saved + 1)
        .unwrap();
    assert_eq!(app.timeline(family, child).unwrap()[0].sleep_place, None);
    app.edit_sleep_place(family, child, id, Some(2), saved + 1)
        .unwrap();
    let before = app.timeline(family, child).unwrap();
    assert_eq!(before[0].id, id);
    assert_eq!(before[0].start_utc_ms, start);
    assert_eq!(before[0].end_utc_ms, Some(start + 45 * 60_000));
    assert_eq!(before[0].sleep_place, Some(2));
    let csv = String::from_utf8(app.analysis_csv(family).unwrap()).unwrap();
    assert!(csv.contains(",sleep_place\r\n"));
    assert!(csv.contains("\"2\"\r\n"));
    let backup = app.backup(family, saved + 2).unwrap();
    drop(app);
    let mut app = LocalRepository::open(&path).unwrap();
    assert_eq!(app.timeline(family, child).unwrap(), before);
    let restored = app.restore(&backup, saved + 3).unwrap();
    assert_eq!(app.timeline(restored, child).unwrap(), before);
}

#[test]
fn analysis_csv_exports_current_activity_and_neutralizes_formula_text() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = LocalRepository::open(dir.path().join("analysis.db")).unwrap();
    let family = app.create_family(1_790_000_000_000).unwrap();
    let child = app
        .add_child_with_metadata(family, "=danger", Some(20_000), Some(2), 1_790_000_000_001)
        .unwrap();
    let note = app
        .log_note(
            family,
            child,
            "=SUM(1,2) \"quote\"",
            ActivityTime {
                start_utc_ms: 1_790_000_000_002,
                offset_minutes: 60,
                saved_at_ms: 1_790_000_000_003,
            },
        )
        .unwrap();
    let csv = String::from_utf8(app.analysis_csv(family).unwrap()).unwrap();
    assert!(csv.starts_with("family_id,child_id,child_name,"));
    assert!(csv.contains("\"'=danger\",\"20000\",\"2\""));
    assert!(csv.contains("\"'=SUM(1,2) \"\"quote\"\"\""));
    assert!(
        csv.contains(
            &note
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
        )
    );
    app.delete_activity(family, child, note, 1_790_000_000_004)
        .unwrap();
    assert_eq!(
        String::from_utf8(app.analysis_csv(family).unwrap())
            .unwrap()
            .lines()
            .count(),
        1
    );
}

#[test]
fn activity_delete_targets_one_child_and_survives_restart() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("delete.db");
    let mut app = LocalRepository::open(&path).unwrap();
    let first = app.create_family(1_790_000_000_000).unwrap();
    let second = app.create_family(1_790_000_000_001).unwrap();
    let child = app.add_child(first, "A", 1_790_000_000_002).unwrap();
    let other = app.add_child(first, "B", 1_790_000_000_003).unwrap();
    let time = ActivityTime {
        start_utc_ms: 1_790_000_000_004,
        offset_minutes: 0,
        saved_at_ms: 1_790_000_000_005,
    };
    let note = app.log_note(first, child, "Mistaken entry", time).unwrap();
    assert!(
        app.delete_activity(second, child, note, time.saved_at_ms + 1)
            .is_err()
    );
    assert!(
        app.delete_activity(first, other, note, time.saved_at_ms + 1)
            .is_err()
    );
    assert_eq!(app.timeline(first, child).unwrap().len(), 1);
    app.delete_activity(first, child, note, time.saved_at_ms + 1)
        .unwrap();
    assert!(app.timeline(first, child).unwrap().is_empty());
    assert!(
        app.delete_activity(first, child, note, time.saved_at_ms + 2)
            .is_err()
    );
    drop(app);
    let app = LocalRepository::open(&path).unwrap();
    assert!(app.timeline(first, child).unwrap().is_empty());
}

#[test]
fn activity_notes_keep_type_and_target_through_clear_restart_and_restore() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("edit-note.db");
    let mut app = LocalRepository::open(&path).unwrap();
    let family = app.create_family(1_790_000_000_000).unwrap();
    let other_family = app.create_family(1_790_000_000_001).unwrap();
    let child = app.add_child(family, "Baby", 1_790_000_000_002).unwrap();
    let other_child = app.add_child(family, "Other", 1_790_000_000_003).unwrap();
    let time = ActivityTime {
        start_utc_ms: 1_790_000_000_004,
        offset_minutes: 0,
        saved_at_ms: 1_790_000_000_005,
    };
    let note = app.log_note(family, child, "Before", time).unwrap();
    let bottle = app.log_bottle_ml(family, child, 90, 1, time).unwrap();
    assert!(
        app.edit_note(other_family, child, note, "Wrong", time.saved_at_ms + 1)
            .is_err()
    );
    assert!(
        app.edit_note(family, other_child, note, "Wrong", time.saved_at_ms + 1)
            .is_err()
    );
    app.edit_note(family, child, bottle, " Bottle note ", time.saved_at_ms + 1)
        .unwrap();
    assert_eq!(
        app.timeline(family, child)
            .unwrap()
            .iter()
            .find(|row| row.id == bottle)
            .unwrap()
            .note
            .as_deref(),
        Some("Bottle note")
    );
    app.edit_note(family, child, bottle, " ", time.saved_at_ms + 2)
        .unwrap();
    assert_eq!(
        app.timeline(family, child)
            .unwrap()
            .iter()
            .find(|row| row.id == bottle)
            .unwrap()
            .note,
        None
    );
    app.edit_note(family, child, bottle, "After feed", time.saved_at_ms + 3)
        .unwrap();
    assert!(
        app.edit_note(family, child, note, " ", time.saved_at_ms + 4)
            .is_err()
    );
    app.edit_note(family, child, note, " After ", time.saved_at_ms + 4)
        .unwrap();
    let before = app.timeline(family, child).unwrap();
    let bottle_row = before.iter().find(|row| row.id == bottle).unwrap();
    assert_eq!(bottle_row.kind, "feed.bottle");
    assert_eq!(bottle_row.bottle_ml, Some(90));
    assert_eq!(bottle_row.note.as_deref(), Some("After feed"));
    assert_eq!(
        before
            .iter()
            .find(|row| row.id == note)
            .unwrap()
            .note
            .as_deref(),
        Some("After")
    );
    assert_eq!(
        before
            .iter()
            .find(|row| row.id == note)
            .unwrap()
            .start_utc_ms,
        time.start_utc_ms
    );
    let backup = app.backup(family, time.saved_at_ms + 5).unwrap();
    drop(app);
    let mut app = LocalRepository::open(&path).unwrap();
    assert_eq!(app.timeline(family, child).unwrap(), before);
    let restored = app.restore(&backup, time.saved_at_ms + 6).unwrap();
    assert_eq!(app.timeline(restored, child).unwrap(), before);
}

#[test]
fn bottle_amount_edit_keeps_activity_and_reaches_restored_copy() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("edit-bottle.db");
    let mut app = LocalRepository::open(&path).unwrap();
    let family = app.create_family(1_790_000_000_000).unwrap();
    let child = app.add_child(family, "Baby", 1_790_000_000_001).unwrap();
    let other = app.add_child(family, "Other", 1_790_000_000_002).unwrap();
    let time = ActivityTime {
        start_utc_ms: 1_790_000_000_003,
        offset_minutes: 0,
        saved_at_ms: 1_790_000_000_004,
    };
    let bottle = app.log_bottle_ml(family, child, 90, 2, time).unwrap();
    assert!(
        app.edit_bottle_ml(family, other, bottle, 120, time.saved_at_ms + 1)
            .is_err()
    );
    assert!(
        app.edit_bottle_ml(family, child, bottle, 0, time.saved_at_ms + 1)
            .is_err()
    );
    app.edit_bottle_ml(family, child, bottle, 120, time.saved_at_ms + 1)
        .unwrap();
    let row = app
        .timeline(family, child)
        .unwrap()
        .into_iter()
        .next()
        .unwrap();
    assert_eq!(row.id, bottle);
    assert_eq!(row.bottle_ml, Some(120));
    assert_eq!(row.bottle_content, Some(2));
    assert_eq!(row.start_utc_ms, time.start_utc_ms);
    let backup = app.backup(family, time.saved_at_ms + 2).unwrap();
    drop(app);
    let mut app = LocalRepository::open(&path).unwrap();
    assert_eq!(app.timeline(family, child).unwrap()[0].bottle_ml, Some(120));
    assert_eq!(
        app.timeline(family, child).unwrap()[0].bottle_content,
        Some(2)
    );
    let restored = app.restore(&backup, time.saved_at_ms + 3).unwrap();
    assert_eq!(
        app.timeline(restored, child).unwrap()[0].bottle_ml,
        Some(120)
    );
    assert_eq!(
        app.timeline(restored, child).unwrap()[0].bottle_content,
        Some(2)
    );
}

#[test]
fn bottle_content_edit_keeps_activity_and_reaches_restored_copy() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("edit-bottle-content.db");
    let mut app = LocalRepository::open(&path).unwrap();
    let family = app.create_family(1_790_000_000_000).unwrap();
    let child = app.add_child(family, "Baby", 1_790_000_000_001).unwrap();
    let other = app.add_child(family, "Other", 1_790_000_000_002).unwrap();
    let time = ActivityTime {
        start_utc_ms: 1_790_000_000_003,
        offset_minutes: 0,
        saved_at_ms: 1_790_000_000_004,
    };
    let bottle = app.log_bottle_ml(family, child, 90, 1, time).unwrap();
    assert!(
        app.edit_bottle(family, other, bottle, 120, 3, time.saved_at_ms + 1)
            .is_err()
    );
    assert!(
        app.edit_bottle(family, child, bottle, 120, 5, time.saved_at_ms + 1)
            .is_err()
    );
    app.edit_bottle(family, child, bottle, 120, 3, time.saved_at_ms + 1)
        .unwrap();
    let rows = app.timeline(family, child).unwrap();
    let row = &rows[0];
    assert_eq!(row.id, bottle);
    assert_eq!(row.bottle_ml, Some(120));
    assert_eq!(row.bottle_content, Some(3));
    assert_eq!(row.start_utc_ms, time.start_utc_ms);
    let csv = String::from_utf8(app.analysis_csv(family).unwrap()).unwrap();
    assert!(csv.contains(",bottle_ml,bottle_content,"));
    assert!(csv.contains("\"120\",\"3\""));
    let backup = app.backup(family, time.saved_at_ms + 2).unwrap();
    drop(app);
    let mut app = LocalRepository::open(&path).unwrap();
    assert_eq!(
        app.timeline(family, child).unwrap()[0].bottle_content,
        Some(3)
    );
    let restored = app.restore(&backup, time.saved_at_ms + 3).unwrap();
    let rows = app.timeline(restored, child).unwrap();
    let row = &rows[0];
    assert_eq!(row.bottle_content, Some(3));
    assert_eq!(row.bottle_ml, Some(120));
}

#[test]
fn bottle_fluid_ounces_preserve_entered_unit_through_edit_and_restore() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bottle-ounces.db");
    let mut app = LocalRepository::open(&path).unwrap();
    let family = app.create_family(1_790_000_000_000).unwrap();
    let child = app.add_child(family, "Baby", 1_790_000_000_001).unwrap();
    let time = ActivityTime {
        start_utc_ms: 1_790_000_000_002,
        offset_minutes: 0,
        saved_at_ms: 1_790_000_000_003,
    };
    assert!(
        app.log_bottle_entered(family, child, "4.5", 4, 1, time)
            .is_err()
    );
    assert!(
        app.log_bottle_entered(family, child, "0", 2, 1, time)
            .is_err()
    );
    assert!(
        app.log_bottle_entered(family, child, "4e1", 2, 1, time)
            .is_err()
    );
    let bottle = app
        .log_bottle_entered(family, child, "4.5", 2, 1, time)
        .unwrap();
    let first = &app.timeline(family, child).unwrap()[0];
    assert_eq!(first.bottle_ml, Some(133));
    assert_eq!(first.bottle_entered.as_deref(), Some("4.5"));
    assert_eq!(first.bottle_unit, Some(2));
    assert!(
        app.edit_bottle_entered(family, child, bottle, "-1", 3, 2, time.saved_at_ms + 1)
            .is_err()
    );
    app.edit_bottle_entered(family, child, bottle, "4.5", 3, 2, time.saved_at_ms + 1)
        .unwrap();
    let after = app.timeline(family, child).unwrap();
    assert_eq!(after[0].id, bottle);
    assert_eq!(after[0].start_utc_ms, time.start_utc_ms);
    assert_eq!(after[0].bottle_ml, Some(128));
    assert_eq!(after[0].bottle_entered.as_deref(), Some("4.5"));
    assert_eq!(after[0].bottle_unit, Some(3));
    assert_eq!(after[0].bottle_content, Some(2));
    let backup = app.backup(family, time.saved_at_ms + 2).unwrap();
    drop(app);
    let mut app = LocalRepository::open(&path).unwrap();
    assert_eq!(app.timeline(family, child).unwrap(), after);
    let restored = app.restore(&backup, time.saved_at_ms + 3).unwrap();
    assert_eq!(app.timeline(restored, child).unwrap(), after);
}

#[test]
fn diaper_kind_edit_keeps_activity_and_reaches_restored_copy() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("edit-diaper.db");
    let mut app = LocalRepository::open(&path).unwrap();
    let family = app.create_family(1_790_000_000_000).unwrap();
    let other_family = app.create_family(1_790_000_000_001).unwrap();
    let child = app.add_child(family, "Baby", 1_790_000_000_002).unwrap();
    let other_child = app.add_child(family, "Other", 1_790_000_000_003).unwrap();
    let time = ActivityTime {
        start_utc_ms: 1_790_000_000_004,
        offset_minutes: 0,
        saved_at_ms: 1_790_000_000_005,
    };
    let diaper = app.log_diaper(family, child, 1, time).unwrap();
    let bottle = app.log_bottle_ml(family, child, 90, 2, time).unwrap();
    assert!(
        app.edit_diaper_kind(other_family, child, diaper, 2, time.saved_at_ms + 1)
            .is_err()
    );
    assert!(
        app.edit_diaper_kind(family, other_child, diaper, 2, time.saved_at_ms + 1)
            .is_err()
    );
    assert!(
        app.edit_diaper_kind(family, child, bottle, 2, time.saved_at_ms + 1)
            .is_err()
    );
    assert!(
        app.edit_diaper_kind(family, child, diaper, 5, time.saved_at_ms + 1)
            .is_err()
    );
    app.edit_diaper_kind(family, child, diaper, 2, time.saved_at_ms + 1)
        .unwrap();
    let before = app.timeline(family, child).unwrap();
    let row = before.iter().find(|row| row.id == diaper).unwrap();
    assert_eq!(row.diaper_kind, Some(2));
    assert_eq!(row.start_utc_ms, time.start_utc_ms);
    let backup = app.backup(family, time.saved_at_ms + 2).unwrap();
    drop(app);
    let mut app = LocalRepository::open(&path).unwrap();
    assert_eq!(app.timeline(family, child).unwrap(), before);
    let restored = app.restore(&backup, time.saved_at_ms + 3).unwrap();
    assert_eq!(app.timeline(restored, child).unwrap(), before);
}

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
    let other = app
        .add_child(family, "Other", time.saved_at_ms + 1)
        .unwrap();
    assert!(
        app.edit_temperature_c(family, other, id, "38.0", time.saved_at_ms + 2)
            .is_err()
    );
    assert!(
        app.edit_temperature_c(family, child, id, "37,8", time.saved_at_ms + 2)
            .is_err()
    );
    app.edit_temperature_c(family, child, id, " 37.8 ", time.saved_at_ms + 2)
        .unwrap();
    let before = app.timeline(family, child).unwrap();
    assert_eq!(
        before
            .iter()
            .find(|row| row.id == id)
            .unwrap()
            .temperature_c
            .as_deref(),
        Some("37.8")
    );
    assert_eq!(
        before.iter().find(|row| row.id == id).unwrap().start_utc_ms,
        time.start_utc_ms
    );
    let backup = app.backup(family, 1_790_000_000_005).unwrap();
    drop(app);
    let mut app = LocalRepository::open(&path).unwrap();
    assert_eq!(app.timeline(family, child).unwrap(), before);
    let restored = app.restore(&backup, 1_790_000_000_006).unwrap();
    assert_eq!(app.timeline(restored, child).unwrap(), before);
}

#[test]
fn fahrenheit_temperature_keeps_entered_unit_through_edit_and_restore() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("fahrenheit.db");
    let mut app = LocalRepository::open(&path).unwrap();
    let family = app.create_family(1_790_000_000_000).unwrap();
    let child = app.add_child(family, "Baby", 1_790_000_000_001).unwrap();
    let other = app.add_child(family, "Other", 1_790_000_000_002).unwrap();
    let time = ActivityTime {
        start_utc_ms: 1_790_000_000_003,
        offset_minutes: 0,
        saved_at_ms: 1_790_000_000_004,
    };
    assert!(
        app.log_temperature_entered(family, child, "98.6", 32, time)
            .is_err()
    );
    assert!(
        app.log_temperature_entered(family, child, "98e1", 31, time)
            .is_err()
    );
    let id = app
        .log_temperature_entered(family, child, "98.6", 31, time)
        .unwrap();
    let first = &app.timeline(family, child).unwrap()[0];
    assert_eq!(first.temperature_c.as_deref(), Some("37.00"));
    assert_eq!(first.temperature_entered.as_deref(), Some("98.6"));
    assert_eq!(first.temperature_unit, Some(31));
    assert!(
        app.edit_temperature_entered(family, other, id, "99", 31, time.saved_at_ms + 1)
            .is_err()
    );
    app.edit_temperature_entered(family, child, id, "99", 31, time.saved_at_ms + 1)
        .unwrap();
    let before = app.timeline(family, child).unwrap();
    assert_eq!(before[0].id, id);
    assert_eq!(before[0].start_utc_ms, time.start_utc_ms);
    assert_eq!(before[0].temperature_c.as_deref(), Some("37.22"));
    assert_eq!(before[0].temperature_entered.as_deref(), Some("99"));
    assert_eq!(before[0].temperature_unit, Some(31));
    let backup = app.backup(family, time.saved_at_ms + 2).unwrap();
    drop(app);
    let mut app = LocalRepository::open(&path).unwrap();
    assert_eq!(app.timeline(family, child).unwrap(), before);
    let restored = app.restore(&backup, time.saved_at_ms + 3).unwrap();
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
fn medication_correction_keeps_target_and_original_time() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("medication-edit.db");
    let mut app = LocalRepository::open(&path).unwrap();
    let family = app.create_family(1_790_000_000_000).unwrap();
    let child = app.add_child(family, "Baby", 1_790_000_000_001).unwrap();
    let other = app.add_child(family, "Other", 1_790_000_000_002).unwrap();
    let time = ActivityTime {
        start_utc_ms: 1_790_000_000_003,
        offset_minutes: 60,
        saved_at_ms: 1_790_000_000_004,
    };
    let medication = app
        .log_medication(family, child, "Before", "2", "mL", time)
        .unwrap();
    assert!(
        app.edit_medication(
            family,
            other,
            medication,
            "After",
            "3",
            "mL",
            time.saved_at_ms + 1
        )
        .is_err()
    );
    assert!(
        app.edit_medication(
            family,
            child,
            medication,
            "After",
            "",
            "mL",
            time.saved_at_ms + 1
        )
        .is_err()
    );
    app.edit_medication(
        family,
        child,
        medication,
        " After ",
        " 3.5 ",
        " mL ",
        time.saved_at_ms + 1,
    )
    .unwrap();
    let before = app.timeline(family, child).unwrap();
    assert_eq!(before.len(), 1);
    assert_eq!(before[0].id, medication);
    assert_eq!(before[0].start_utc_ms, time.start_utc_ms);
    assert_eq!(before[0].medication_name.as_deref(), Some("After"));
    assert_eq!(before[0].medication_dose_amount.as_deref(), Some("3.5"));
    assert_eq!(before[0].medication_dose_unit.as_deref(), Some("mL"));
    let backup = app.backup(family, time.saved_at_ms + 2).unwrap();
    drop(app);
    let mut app = LocalRepository::open(&path).unwrap();
    assert_eq!(app.timeline(family, child).unwrap(), before);
    let restored = app.restore(&backup, time.saved_at_ms + 3).unwrap();
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
    assert!(
        app.edit_solids(family, child, id, &[], "", time.saved_at_ms + 1)
            .is_err()
    );
    let other_child = app
        .add_child(family, "Other", time.saved_at_ms + 1)
        .unwrap();
    assert!(
        app.edit_solids(
            family,
            other_child,
            id,
            &["Apple".into()],
            "half",
            time.saved_at_ms + 1
        )
        .is_err()
    );
    app.edit_solids(
        family,
        child,
        id,
        &[" Apple ".into(), "Rice".into()],
        " half bowl ",
        time.saved_at_ms + 2,
    )
    .unwrap();
    let before = app.timeline(family, child).unwrap();
    let row = before.iter().find(|row| row.id == id).unwrap();
    assert_eq!(
        row.solids_foods.as_deref(),
        Some(&["Apple".into(), "Rice".into()][..])
    );
    assert_eq!(row.solids_amount.as_deref(), Some("half bowl"));
    assert_eq!(row.start_utc_ms, time.start_utc_ms);
    let backup = app.backup(family, 1_790_000_000_005).unwrap();
    drop(app);
    let mut app = LocalRepository::open(&path).unwrap();
    assert_eq!(app.timeline(family, child).unwrap(), before);
    let restored = app.restore(&backup, 1_790_000_000_006).unwrap();
    assert_eq!(app.timeline(restored, child).unwrap(), before);
}

#[test]
fn completed_breast_feed_keeps_side_and_interval_after_restore() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("breast.db");
    let mut app = LocalRepository::open(&path).unwrap();
    let family = app.create_family(1_790_000_000_000).unwrap();
    let child = app.add_child(family, "Baby", 1_790_000_000_001).unwrap();
    let end = 1_790_000_900_000;
    let time = ActivityTime {
        start_utc_ms: end - 15 * 60_000,
        offset_minutes: 120,
        saved_at_ms: end,
    };
    assert!(app.log_breast_feed(family, child, 0, time, end).is_err());
    assert!(app.log_breast_feed(family, child, 3, time, end).is_err());
    assert!(
        app.log_breast_feed(family, child, 1, time, time.start_utc_ms - 1)
            .is_err()
    );
    let id = app.log_breast_feed(family, child, 2, time, end).unwrap();
    let before = app.timeline(family, child).unwrap();
    let row = before.iter().find(|row| row.id == id).unwrap();
    assert_eq!(row.kind, "feed.breast");
    assert_eq!(row.breast_side, Some(2));
    assert_eq!(row.start_utc_ms, time.start_utc_ms);
    assert_eq!(row.end_utc_ms, Some(end));
    let backup = app.backup(family, end + 1).unwrap();
    drop(app);
    let mut app = LocalRepository::open(&path).unwrap();
    assert_eq!(app.timeline(family, child).unwrap(), before);
    let restored = app.restore(&backup, end + 2).unwrap();
    assert_eq!(app.timeline(restored, child).unwrap(), before);
}

#[test]
fn alternating_breast_segments_survive_restart_and_file_restore() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("breast-segments.db");
    let mut app = LocalRepository::open(&path).unwrap();
    let family = app.create_family(1_790_000_000_000).unwrap();
    let child = app.add_child(family, "Baby", 1_790_000_000_001).unwrap();
    let start = 1_790_000_000_002;
    let mut segments = vec![
        BreastSegment {
            side: 1,
            start_utc_ms: start,
            end_utc_ms: start + 5 * 60_000,
            start_offset_minutes: 120,
            end_offset_minutes: 120,
        },
        BreastSegment {
            side: 2,
            start_utc_ms: start + 5 * 60_000,
            end_utc_ms: start + 13 * 60_000,
            start_offset_minutes: 120,
            end_offset_minutes: 120,
        },
        BreastSegment {
            side: 1,
            start_utc_ms: start + 13 * 60_000,
            end_utc_ms: start + 16 * 60_000,
            start_offset_minutes: 120,
            end_offset_minutes: 120,
        },
    ];
    segments[0].end_offset_minutes = 60;
    segments[1].start_offset_minutes = 60;
    segments[1].end_offset_minutes = 60;
    segments[2].start_offset_minutes = 60;
    segments[2].end_offset_minutes = 60;
    let time = ActivityTime {
        start_utc_ms: start,
        offset_minutes: 120,
        saved_at_ms: start + 16 * 60_000,
    };
    let mut broken = segments.clone();
    broken[1].start_utc_ms += 1;
    assert!(
        app.log_breast_feed_segments(family, child, broken, time)
            .is_err()
    );
    let id = app
        .log_breast_feed_segments(family, child, segments.clone(), time)
        .unwrap();
    let before = app.timeline(family, child).unwrap();
    let row = before.iter().find(|row| row.id == id).unwrap();
    assert_eq!(row.breast_segments.as_deref(), Some(segments.as_slice()));
    assert_eq!(row.breast_side, None);
    assert_eq!(row.end_utc_ms, Some(time.saved_at_ms));
    let backup = app.backup(family, time.saved_at_ms + 1).unwrap();
    drop(app);
    let mut app = LocalRepository::open(&path).unwrap();
    assert_eq!(app.timeline(family, child).unwrap(), before);
    let restored = app.restore(&backup, time.saved_at_ms + 2).unwrap();
    assert_eq!(app.timeline(restored, child).unwrap(), before);
}

#[test]
fn breast_segment_correction_preserves_start_and_restores() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("breast-edit.db");
    let mut app = LocalRepository::open(&path).unwrap();
    let family = app.create_family(1_790_000_000_000).unwrap();
    let child = app.add_child(family, "Baby", 1_790_000_000_001).unwrap();
    let other = app.add_child(family, "Other", 1_790_000_000_002).unwrap();
    let start = 1_790_000_000_003;
    let time = ActivityTime {
        start_utc_ms: start,
        offset_minutes: 120,
        saved_at_ms: start + 13 * 60_000,
    };
    let original = vec![
        BreastSegment {
            side: 1,
            start_utc_ms: start,
            end_utc_ms: start + 5 * 60_000,
            start_offset_minutes: 120,
            end_offset_minutes: 120,
        },
        BreastSegment {
            side: 2,
            start_utc_ms: start + 5 * 60_000,
            end_utc_ms: start + 13 * 60_000,
            start_offset_minutes: 120,
            end_offset_minutes: 120,
        },
    ];
    let id = app
        .log_breast_feed_segments(family, child, original, time)
        .unwrap();
    let corrected = vec![
        BreastSegment {
            side: 2,
            start_utc_ms: start,
            end_utc_ms: start + 7 * 60_000,
            start_offset_minutes: 120,
            end_offset_minutes: 120,
        },
        BreastSegment {
            side: 1,
            start_utc_ms: start + 7 * 60_000,
            end_utc_ms: start + 16 * 60_000,
            start_offset_minutes: 120,
            end_offset_minutes: 120,
        },
    ];
    let mut broken = corrected.clone();
    broken[1].start_utc_ms += 1;
    assert!(
        app.edit_breast_feed_segments(family, child, id, broken, start + 20 * 60_000)
            .is_err()
    );
    assert!(
        app.edit_breast_feed_segments(family, other, id, corrected.clone(), start + 20 * 60_000)
            .is_err()
    );
    app.edit_breast_feed_segments(family, child, id, corrected.clone(), start + 20 * 60_000)
        .unwrap();
    let before = app.timeline(family, child).unwrap();
    let row = before.iter().find(|row| row.id == id).unwrap();
    assert_eq!(row.start_utc_ms, start);
    assert_eq!(row.end_utc_ms, Some(start + 16 * 60_000));
    assert_eq!(row.breast_segments.as_deref(), Some(corrected.as_slice()));
    let backup = app.backup(family, start + 21 * 60_000).unwrap();
    drop(app);
    let mut app = LocalRepository::open(&path).unwrap();
    assert_eq!(app.timeline(family, child).unwrap(), before);
    let restored = app.restore(&backup, start + 22 * 60_000).unwrap();
    assert_eq!(app.timeline(restored, child).unwrap(), before);
}

#[test]
fn pump_sides_or_total_survive_restart_and_file_restore() {
    let amounts = |left_ml, right_ml, total_ml| PumpAmounts {
        left_ml,
        right_ml,
        total_ml,
    };
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("pump.db");
    let mut app = LocalRepository::open(&path).unwrap();
    let family = app.create_family(1_790_000_000_000).unwrap();
    let child = app.add_child(family, "Baby", 1_790_000_000_001).unwrap();
    let end = 1_790_000_600_000;
    let time = ActivityTime {
        start_utc_ms: end - 10 * 60_000,
        offset_minutes: 120,
        saved_at_ms: end,
    };
    assert!(
        app.log_pump(family, child, amounts(None, None, None), time, end)
            .is_err()
    );
    assert!(
        app.log_pump(family, child, amounts(Some(0), Some(0), None), time, end)
            .is_err()
    );
    assert!(
        app.log_pump(family, child, amounts(Some(10), None, Some(10)), time, end)
            .is_err()
    );
    assert!(
        app.log_pump(
            family,
            child,
            amounts(Some(i64::MAX), Some(i64::MAX), None),
            time,
            end,
        )
        .is_err()
    );
    let sides = app
        .log_pump(family, child, amounts(Some(20), Some(15), None), time, end)
        .unwrap();
    let total = app
        .log_pump(family, child, amounts(None, None, Some(35)), time, end)
        .unwrap();
    let before = app.timeline(family, child).unwrap();
    let sides_row = before.iter().find(|row| row.id == sides).unwrap();
    assert_eq!(
        (
            sides_row.pump_left_ml,
            sides_row.pump_right_ml,
            sides_row.pump_total_ml
        ),
        (Some(20), Some(15), None)
    );
    let total_row = before.iter().find(|row| row.id == total).unwrap();
    assert_eq!(
        (
            total_row.pump_left_ml,
            total_row.pump_right_ml,
            total_row.pump_total_ml
        ),
        (None, None, Some(35))
    );
    let backup = app.backup(family, end + 1).unwrap();
    drop(app);
    let mut app = LocalRepository::open(&path).unwrap();
    assert_eq!(app.timeline(family, child).unwrap(), before);
    let restored = app.restore(&backup, end + 2).unwrap();
    assert_eq!(app.timeline(restored, child).unwrap(), before);
}

#[test]
fn pump_correction_switches_between_sides_and_total_without_moving_time() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("pump-edit.db");
    let mut app = LocalRepository::open(&path).unwrap();
    let family = app.create_family(1_790_000_000_000).unwrap();
    let child = app.add_child(family, "Baby", 1_790_000_000_001).unwrap();
    let other = app.add_child(family, "Other", 1_790_000_000_002).unwrap();
    let end = 1_790_000_600_000;
    let time = ActivityTime {
        start_utc_ms: end - 10 * 60_000,
        offset_minutes: 120,
        saved_at_ms: end,
    };
    let pump = app
        .log_pump(
            family,
            child,
            PumpAmounts {
                left_ml: Some(20),
                right_ml: Some(15),
                total_ml: None,
            },
            time,
            end,
        )
        .unwrap();
    let corrected = PumpAmounts {
        left_ml: None,
        right_ml: None,
        total_ml: Some(40),
    };
    assert!(
        app.edit_pump_amounts(family, other, pump, corrected, end + 1)
            .is_err()
    );
    assert!(
        app.edit_pump_amounts(
            family,
            child,
            pump,
            PumpAmounts {
                left_ml: Some(10),
                right_ml: None,
                total_ml: Some(40),
            },
            end + 1
        )
        .is_err()
    );
    app.edit_pump_amounts(family, child, pump, corrected, end + 1)
        .unwrap();
    let before = app.timeline(family, child).unwrap();
    assert_eq!(before.len(), 1);
    assert_eq!(before[0].id, pump);
    assert_eq!(before[0].start_utc_ms, time.start_utc_ms);
    assert_eq!(before[0].end_utc_ms, Some(end));
    assert_eq!(
        (
            before[0].pump_left_ml,
            before[0].pump_right_ml,
            before[0].pump_total_ml
        ),
        (None, None, Some(40))
    );
    let backup = app.backup(family, end + 2).unwrap();
    drop(app);
    let mut app = LocalRepository::open(&path).unwrap();
    assert_eq!(app.timeline(family, child).unwrap(), before);
    let restored = app.restore(&backup, end + 3).unwrap();
    assert_eq!(app.timeline(restored, child).unwrap(), before);
}
