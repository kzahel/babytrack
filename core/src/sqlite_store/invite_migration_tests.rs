use super::*;

#[test]
fn simultaneous_legacy_openers_apply_each_migration_once() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("concurrent-legacy.db");
    let store = SqliteStore::open(&path).unwrap();
    store
        .connection
        .execute_batch(
            "DROP TABLE enrollment_terminal_status;
         DROP TABLE enrollment_claim_candidates;
         DROP TABLE invite_issues;
         PRAGMA user_version = 1;",
        )
        .unwrap();
    drop(store);
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(8));
    let openings: Vec<_> = (0..8)
        .map(|_| {
            let path = path.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                SqliteStore::open(path)
            })
        })
        .collect();
    for opening in openings {
        assert!(opening.join().unwrap().is_ok());
    }
    let store = SqliteStore::open(&path).unwrap();
    let version: u32 = store
        .connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .unwrap();
    assert_eq!(version, 4);
}

#[test]
fn version_three_store_adds_terminal_receipt_table() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("version-three.db");
    let mut store = SqliteStore::open(&path).unwrap();
    let family = store.create_family(v4(0x73), v4(0x74)).unwrap();
    store
        .connection
        .execute_batch("DROP TABLE enrollment_terminal_status; PRAGMA user_version = 3;")
        .unwrap();
    drop(store);
    let store = SqliteStore::open(&path).unwrap();
    assert_eq!(store.families().unwrap(), vec![family]);
    assert!(store.enrollment_terminal_status(family).unwrap().is_none());
    let version: u32 = store
        .connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .unwrap();
    assert_eq!(version, 4);
}

#[test]
fn version_two_store_adds_claim_archive_without_changing_families() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("version-two.db");
    let mut store = SqliteStore::open(&path).unwrap();
    let family = store.create_family(v4(0x71), v4(0x72)).unwrap();
    store
        .connection
        .execute_batch("DROP TABLE enrollment_terminal_status; DROP TABLE enrollment_claim_candidates; PRAGMA user_version = 2;")
        .unwrap();
    drop(store);
    let store = SqliteStore::open(&path).unwrap();
    assert_eq!(store.families().unwrap(), vec![family]);
    assert!(store.archived_enrollment_claims(family).unwrap().is_empty());
    let version: u32 = store
        .connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .unwrap();
    assert_eq!(version, 4);
}

#[test]
fn legacy_first_invite_keeps_exact_bytes_after_v4_upgrade() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("legacy-invite.db");
    let mut store = SqliteStore::open(&path).unwrap();
    let family = store.create_family(v4(1), v4(2)).unwrap();
    let row = InviteIssueRow {
        family,
        invitation_id: v4(3),
        transition_id: v4(4),
        object_id: v4(5),
        object_bytes: vec![0x81, 0x01],
        candidate_bytes: vec![0x82, 0x02, 0x03],
        secret_nonce: [6; 24],
        secret_ciphertext: vec![0x84, 0x04],
    };
    store
        .connection
        .execute(
            "INSERT INTO first_invite_issues
         (family_id,device_id,invitation_id,transition_id,object_id,
          object_bytes,candidate_bytes,secret_nonce,secret_ciphertext)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",
            params![
                family.family_id.as_slice(),
                family.device_id.as_slice(),
                row.invitation_id.as_slice(),
                row.transition_id.as_slice(),
                row.object_id.as_slice(),
                &row.object_bytes,
                &row.candidate_bytes,
                row.secret_nonce.as_slice(),
                &row.secret_ciphertext,
            ],
        )
        .unwrap();
    store
        .connection
        .execute_batch("DROP TABLE enrollment_terminal_status; DROP TABLE enrollment_claim_candidates; DROP TABLE invite_issues; PRAGMA user_version = 1;")
        .unwrap();
    drop(store);

    let store = SqliteStore::open(&path).unwrap();
    let migrated = store.first_invite_issue(family).unwrap().unwrap();
    assert_eq!(migrated.invitation_id, row.invitation_id);
    assert_eq!(migrated.transition_id, row.transition_id);
    assert_eq!(migrated.object_id, row.object_id);
    assert_eq!(migrated.object_bytes, row.object_bytes);
    assert_eq!(migrated.candidate_bytes, row.candidate_bytes);
    assert_eq!(migrated.secret_nonce, row.secret_nonce);
    assert_eq!(migrated.secret_ciphertext, row.secret_ciphertext);
    let version: u32 = store
        .connection
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .unwrap();
    assert_eq!(version, 4);
    let later = InviteIssueRow {
        family,
        invitation_id: v4(7),
        transition_id: v4(8),
        object_id: v4(9),
        object_bytes: vec![0x81, 0x07],
        candidate_bytes: vec![0x82, 0x08, 0x09],
        secret_nonce: [10; 24],
        secret_ciphertext: vec![0x84, 0x0a],
    };
    let mut store = store;
    store.save_later_invite_issue(&later).unwrap();
    assert_eq!(
        store
            .first_invite_issue(family)
            .unwrap()
            .unwrap()
            .invitation_id,
        row.invitation_id
    );
    assert_eq!(
        store
            .invite_issue(family, later.invitation_id)
            .unwrap()
            .unwrap()
            .candidate_bytes,
        later.candidate_bytes
    );
    drop(store);
    let reopened = SqliteStore::open(&path).unwrap();
    assert!(reopened.first_invite_issue(family).unwrap().is_some());
    assert!(
        reopened
            .invite_issue(family, later.invitation_id)
            .unwrap()
            .is_some()
    );
}

fn v4(fill: u8) -> [u8; 16] {
    let mut id = [fill; 16];
    id[6] = 0x40;
    id[8] = 0x80;
    id
}
