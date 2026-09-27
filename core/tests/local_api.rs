#![cfg(not(target_arch = "wasm32"))]

use babytrack_core::{
    local_api::{ActivityTime, LocalRepository},
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
    let timeline = app.timeline(first, child).unwrap();
    assert_eq!(timeline.len(), 2);
    assert_eq!(timeline[0].bottle_ml, Some(85));
    assert_eq!(timeline[1].diaper_kind, Some(3));
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
    assert_eq!(app.timeline(restored, child).unwrap().len(), 2);
    assert_eq!(app.children(second).unwrap().len(), 0);
}
