#![cfg(not(target_arch = "wasm32"))]

use std::{fs, path::PathBuf, time::SystemTime};

use babytrack_core::{
    operation::Operation,
    shared_history::PublicHistorySession,
    shared_ready::ReadyManagerSession,
    sqlite_store::{FamilyHandle, SqliteStore},
};

fn bytes<const N: usize>(hex: &str) -> [u8; N] {
    hex.as_bytes()
        .chunks_exact(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect::<Vec<_>>()
        .try_into()
        .unwrap()
}

fn temp_db() -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "babytrack-shared-history-{}-{nonce}.sqlite",
        std::process::id()
    ))
}

#[test]
fn interleaved_authority_and_data_keep_one_durable_pinned_cursor() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("../../tests/vectors/contiguous-chain-v1.json")).unwrap();
    let transitions = fixture["transitions"].as_array().unwrap();
    let wire = |index: usize| {
        let hex = transitions[index]["committed_cbor_hex"].as_str().unwrap();
        hex.as_bytes()
            .chunks_exact(2)
            .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
            .collect::<Vec<_>>()
    };
    let family = FamilyHandle {
        family_id: bytes(
            fixture["test_only_inputs"]["family_id_hex"]
                .as_str()
                .unwrap(),
        ),
        device_id: bytes(
            fixture["test_only_inputs"]["manager_device_id_hex"]
                .as_str()
                .unwrap(),
        ),
    };
    let relay_public =
        bytes::<32>("2543b92ff1095511476adc8369db6ddc933665a11978dda1404ee1066ca9559d");
    let path = temp_db();
    let mut store = SqliteStore::open(&path).unwrap();
    store
        .create_family(family.family_id, family.device_id)
        .unwrap();
    let mut session =
        PublicHistorySession::begin(&mut store, family, &wire(0), relay_public).unwrap();
    assert_eq!(session.cursor(), 1);
    let original_head = session.head_hash();

    // Invalid signed bytes never advance the durable pin.
    let mut bad_issue = wire(1);
    *bad_issue.last_mut().unwrap() ^= 1;
    assert!(session.accept_control(&mut store, &bad_issue).is_err());
    drop(session);
    drop(store);
    let mut store = SqliteStore::open(&path).unwrap();
    let mut session = PublicHistorySession::resume(&store, family).unwrap();
    assert_eq!(session.cursor(), 1);
    assert_eq!(session.head_hash(), original_head);

    for index in 1..=6 {
        session.accept_control(&mut store, &wire(index)).unwrap();
        let cursor = session.cursor();
        let head = session.head_hash();
        drop(session);
        drop(store);
        store = SqliteStore::open(&path).unwrap();
        session = PublicHistorySession::resume(&store, family).unwrap();
        assert_eq!(session.cursor(), cursor);
        assert_eq!(session.head_hash(), head);
    }
    let batch = &fixture["batch"];
    let envelope = bytes_from_hex(batch["envelope_cbor_hex"].as_str().unwrap());
    let receipt = bytes_from_hex(batch["receipt_cbor_hex"].as_str().unwrap());
    let control_head = session.head_hash();
    let mut bad_receipt = receipt.clone();
    *bad_receipt.last_mut().unwrap() ^= 1;
    assert!(
        session
            .accept_batch(&mut store, &envelope, &bad_receipt)
            .is_err()
    );
    assert_eq!(session.cursor(), 7);
    session
        .accept_batch(&mut store, &envelope, &receipt)
        .unwrap();
    assert_eq!(session.cursor(), 8);
    assert_eq!(session.head_hash(), control_head);
    drop(session);
    drop(store);
    let mut store = SqliteStore::open(&path).unwrap();
    let mut session = PublicHistorySession::resume(&store, family).unwrap();
    assert_eq!(session.cursor(), 8);
    session.accept_control(&mut store, &wire(7)).unwrap();
    let head = session.head_hash();
    drop(session);
    drop(store);
    let mut store = SqliteStore::open(&path).unwrap();
    let mut session =
        PublicHistorySession::begin(&mut store, family, &wire(0), relay_public).unwrap();
    assert_eq!(session.cursor(), 9);
    assert_eq!(session.head_hash(), head);
    assert!(session.accept_control(&mut store, &wire(1)).is_err());
    assert!(
        session
            .accept_batch(&mut store, &envelope, &receipt)
            .is_err()
    );
    let initial_key = bytes::<32>(
        fixture["test_only_inputs"]["epoch_1_key_hex"]
            .as_str()
            .unwrap(),
    );
    let manager_agreement = bytes::<32>(
        fixture["test_only_inputs"]["manager_agreement_seed_hex"]
            .as_str()
            .unwrap(),
    );
    assert!(
        ReadyManagerSession::from_store(&store, family, initial_key, manager_agreement).is_err()
    );
    let objects = fixture["objects_by_id_hex"].as_object().unwrap();
    let mut altered = bytes_from_hex(
        objects["943e4567e89b42d3a456426614174000"]
            .as_str()
            .unwrap(),
    );
    *altered.last_mut().unwrap() ^= 1;
    assert!(
        session
            .accept_object(
                &mut store,
                bytes("943e4567e89b42d3a456426614174000"),
                &altered,
            )
            .is_err()
    );
    let held_back = "943e4567e89b42d3a456426614174000";
    for (id, hex) in objects {
        if id != held_back {
            session
                .accept_object(
                    &mut store,
                    bytes(id),
                    &bytes_from_hex(hex.as_str().unwrap()),
                )
                .unwrap();
        }
    }
    assert!(
        ReadyManagerSession::from_store(&store, family, initial_key, manager_agreement).is_err()
    );
    session
        .accept_object(
            &mut store,
            bytes(held_back),
            &bytes_from_hex(objects[held_back].as_str().unwrap()),
        )
        .unwrap();
    let ready =
        ReadyManagerSession::from_store(&store, family, initial_key, manager_agreement).unwrap();
    assert_eq!(ready.observed_cursor(), 9);
    assert_eq!(ready.active_epoch(), 2);
    assert_eq!(ready.projection().last_cursor(), 9);
    let operation = Operation::decode_bound(
        &bytes_from_hex(batch["operation_cbor_hex"].as_str().unwrap()),
        &family.family_id,
        &bytes::<16>(
            fixture["test_only_inputs"]["recipient_device_id_hex"]
                .as_str()
                .unwrap(),
        ),
    )
    .unwrap();
    assert!(ready.projection().record(&operation.record_id).is_some());
    drop(session);
    drop(store);
    let store = SqliteStore::open(&path).unwrap();
    let ready_after_restart =
        ReadyManagerSession::from_store(&store, family, initial_key, manager_agreement).unwrap();
    assert_eq!(ready_after_restart.observed_cursor(), 9);
    assert!(
        ready_after_restart
            .projection()
            .record(&operation.record_id)
            .is_some()
    );
    drop(store);

    // Deleting an earlier row while leaving the high-water pin cannot make
    // an older valid prefix silently become the current authority.
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute(
        "DELETE FROM shared_entries WHERE family_id = ?1 AND cursor = 9",
        [family.family_id.as_slice()],
    )
    .unwrap();
    assert!(PublicHistorySession::resume(&SqliteStore::open(&path).unwrap(), family).is_err());
    drop(db);
    fs::remove_file(path).unwrap();
}

fn bytes_from_hex(hex: &str) -> Vec<u8> {
    hex.as_bytes()
        .chunks_exact(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}
