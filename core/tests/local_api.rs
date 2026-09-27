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
    let revision = app.revision(first).unwrap();
    let file = app.backup_file(first, 1_790_000_000_006, None, 0).unwrap();
    assert_eq!(file.revision, revision);
    assert_eq!(file.info.snapshot_utc_ms, 1_790_000_000_006);
    assert_eq!(file.info.record_count, 4);
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
    assert_eq!(app.timeline(restored, child).unwrap().len(), 2);
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
    assert_eq!(app.timeline(copy, child).unwrap().len(), 2);
}
