use super::*;
use crate::operation::{Kind, Scope};

fn v4(tag: u8) -> [u8; 16] {
    let mut id = [tag; 16];
    id[6] = 0x40;
    id[8] = 0x80;
    id
}

fn v7(tag: u8) -> [u8; 16] {
    let mut id = [tag; 16];
    id[6] = 0x70;
    id[8] = 0x80;
    id
}

#[test]
fn removal_copy_rolls_back_mapping_and_family_after_late_sqlite_failure() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("copy.db");
    let source = FamilyHandle {
        family_id: v4(0x41),
        device_id: v4(0x42),
    };
    let copy_id = v4(0x43);
    let copy_device = v4(0x44);
    let transition_id = v4(0x45);
    let child_id = v7(0x46);
    let origin = RestoredOrigin {
        source_family_id: source.family_id,
        snapshot_utc_ms: 1_000,
        source_cursor: Some(7),
        known_gap: true,
    };
    let operation = NewOperation {
        family_id: copy_id,
        operation_id: v7(0x47),
        record_id: child_id,
        scope: Scope::Child,
        kind: Kind::Create,
        author_device_id: copy_device,
        hlc: Hlc {
            wall_ms: 0,
            counter: 0,
            device_id: copy_device,
        },
        record_type: Some("child".to_owned()),
        child_id: None,
        fields: Some(vec![(1, Value::Text("Preserved".to_owned()))]),
    };
    let mut store = SqliteStore::open(&path).unwrap();
    store
        .create_family(source.family_id, source.device_id)
        .unwrap();
    // This test isolates the copy transaction after a removal proof has
    // already been verified and saved by the public-history path.
    store.connection.execute(
        "INSERT INTO verified_removals(source_family_id,source_device_id,transition_id,cursor,source_cursor,known_gap,committed_bytes)
         VALUES(?1,?2,?3,7,6,1,?4)",
        params![source.family_id.as_slice(),source.device_id.as_slice(),transition_id.as_slice(),&[0x80u8][..]],
    ).unwrap();
    store
        .connection
        .execute_batch(
            "CREATE TRIGGER fail_copy_op BEFORE INSERT ON local_operations
             BEGIN SELECT RAISE(ABORT, 'injected failure'); END;",
        )
        .unwrap();
    assert!(
        store
            .restore_family_with_copy_source(
                copy_id,
                copy_device,
                vec![operation.clone()],
                1_000,
                origin,
                Some(CopySource::Removal(source, transition_id)),
            )
            .is_err()
    );
    drop(store);
    let mut store = SqliteStore::open(&path).unwrap();
    assert_eq!(store.families().unwrap(), vec![source]);
    assert!(
        store
            .removal_copy_of(source, transition_id)
            .unwrap()
            .is_none()
    );
    for table in ["restored_origins", "removal_copies", "local_operations"] {
        let count: i64 = store
            .connection
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, 0, "partial {table} row survived");
    }
    store
        .connection
        .execute_batch("DROP TRIGGER fail_copy_op")
        .unwrap();
    let copy = store
        .restore_family_with_copy_source(
            copy_id,
            copy_device,
            vec![operation],
            1_000,
            origin,
            Some(CopySource::Removal(source, transition_id)),
        )
        .unwrap();
    assert_eq!(
        store.removal_copy_of(source, transition_id).unwrap(),
        Some(copy)
    );
    assert_eq!(store.restored_origin(copy).unwrap(), Some(origin));
    assert_eq!(
        store
            .load_local(copy)
            .unwrap()
            .record(&child_id)
            .unwrap()
            .field(1)
            .unwrap()
            .value,
        Value::Text("Preserved".to_owned())
    );
}

#[test]
fn sparse_attempt_and_shared_root_commit_or_roll_back_together() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("enrollment.db");
    let family = FamilyHandle {
        family_id: v4(0x31),
        device_id: v4(0x32),
    };
    let row = EnrollmentRow {
        family,
        invitation_id: v4(0x33),
        genesis_bytes: vec![0x80],
        issue_bytes: vec![0x81, 0x01],
        candidate_bytes: vec![0x82, 0x01, 0x02],
        secret_nonce: [0x34; 24],
        secret_ciphertext: vec![0x35],
    };
    let mut store = SqliteStore::open(&path).unwrap();
    // Fail the final write inside the transaction. No earlier Family,
    // key, root, or sparse control row may survive the failed attempt.
    store
        .connection
        .execute_batch(
            "CREATE TRIGGER fail_sparse_insert BEFORE INSERT ON enrollment_controls
             BEGIN SELECT RAISE(ABORT, 'injected failure'); END;",
        )
        .unwrap();
    assert!(
        store
            .create_enrollment_attempt(
                &row,
                &[(3, row.issue_bytes.clone())],
                [0x36; 32],
                [0x37; 32]
            )
            .is_err()
    );
    drop(store);
    let mut store = SqliteStore::open(&path).unwrap();
    for table in [
        "families",
        "local_sync_state",
        "shared_roots",
        "enrollment_attempts",
        "enrollment_controls",
    ] {
        let count: i64 = store
            .connection
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, 0, "partial {table} row survived");
    }
    store
        .connection
        .execute_batch("DROP TRIGGER fail_sparse_insert")
        .unwrap();
    store
        .create_enrollment_attempt(
            &row,
            &[(3, row.issue_bytes.clone())],
            [0x36; 32],
            [0x37; 32],
        )
        .unwrap();
    drop(store);
    let store = SqliteStore::open(&path).unwrap();
    assert!(
        store
            .enrollment_attempt(family.family_id)
            .unwrap()
            .is_some()
    );
    let history = store.shared_history(family).unwrap().unwrap();
    assert_eq!(history.genesis_bytes, row.genesis_bytes);
    assert_eq!(history.pinned_cursor, 1);
    assert_eq!(store.enrollment_controls(family).unwrap().len(), 1);
}
