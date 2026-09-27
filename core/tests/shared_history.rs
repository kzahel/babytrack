#![cfg(not(target_arch = "wasm32"))]

use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    time::SystemTime,
};

use babytrack_core::{
    batch::{self, Header},
    cbor::{self, Value},
    crypto, local_api,
    operation::{Hlc, Kind, NewOperation, Operation, Scope},
    shared_history::PublicHistorySession,
    shared_ready::{NextUpload, ReadyFamilySession},
    sqlite_store::{FamilyHandle, SqliteStore},
};

static NEXT_DB_ID: AtomicU64 = AtomicU64::new(0);

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
        "babytrack-shared-history-{}-{nonce}-{}.sqlite",
        std::process::id(),
        NEXT_DB_ID.fetch_add(1, Ordering::Relaxed),
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
    let manager_seed = bytes::<32>(
        fixture["test_only_inputs"]["manager_sign_seed_hex"]
            .as_str()
            .unwrap(),
    );
    let relay_seed = bytes::<32>(
        fixture["test_only_inputs"]["relay_sign_seed_hex"]
            .as_str()
            .unwrap(),
    );
    let genesis_object_id = transitions[0]["manifest"][0][1].as_str().unwrap();
    session
        .accept_object(
            &mut store,
            bytes(genesis_object_id),
            &bytes_from_hex(
                fixture["objects_by_id_hex"][genesis_object_id]
                    .as_str()
                    .unwrap(),
            ),
        )
        .unwrap();
    store
        .append_local(
            family,
            NewOperation {
                family_id: family.family_id,
                operation_id: bytes("0183f9d0000070008000000000000044"),
                record_id: family.family_id,
                scope: Scope::Family,
                kind: Kind::Create,
                author_device_id: family.device_id,
                hlc: Hlc {
                    wall_ms: 0,
                    counter: 0,
                    device_id: family.device_id,
                },
                record_type: Some("family".to_owned()),
                child_id: None,
                fields: Some(vec![]),
            },
            100,
        )
        .unwrap();
    let initially_ready =
        ReadyFamilySession::from_store(&store, family, initial_key, manager_agreement).unwrap();
    let old_pending = match initially_ready
        .stage_next_local(&mut store, &manager_seed)
        .unwrap()
    {
        NextUpload::Fresh(pending) => pending,
        NextUpload::RetryExact(_) => panic!("first stage must reserve new bytes"),
    };
    match initially_ready
        .stage_next_local(&mut store, &manager_seed)
        .unwrap()
    {
        NextUpload::RetryExact(pending) => assert_eq!(pending, old_pending),
        NextUpload::Fresh(_) => panic!("uncertain retry must keep exact bytes"),
    }

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
    assert!(
        ReadyFamilySession::from_store(&store, family, initial_key, manager_agreement).is_err()
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
        ReadyFamilySession::from_store(&store, family, initial_key, manager_agreement).is_err()
    );
    session
        .accept_object(
            &mut store,
            bytes(held_back),
            &bytes_from_hex(objects[held_back].as_str().unwrap()),
        )
        .unwrap();
    let ready =
        ReadyFamilySession::from_store(&store, family, initial_key, manager_agreement).unwrap();
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
    match ready.stage_next_local(&mut store, &manager_seed).unwrap() {
        NextUpload::RetryExact(pending) => assert_eq!(pending, old_pending),
        NextUpload::Fresh(_) => panic!("rotation cannot silently replace an uncertain batch"),
    }
    let rejection_body = Value::Map(vec![
        (1, Value::Integer(1)),
        (2, Value::Bytes(family.family_id.to_vec())),
        (3, Value::Bytes(session.chain().relay_id().to_vec())),
        (4, Value::Bytes(old_pending.batch_id.to_vec())),
        (5, Value::Bytes(old_pending.object_hash.to_vec())),
        (6, Value::Bool(false)),
        (7, Value::Integer(session.cursor().into())),
        (8, Value::Bytes(session.head_hash().to_vec())),
        (9, Value::Integer(old_pending.sequence.into())),
        (10, Value::Integer(1)),
        (11, Value::Integer(old_pending.sequence.into())),
    ]);
    let rejection_signature = crypto::sign_cbor(
        "batch-receipt",
        &cbor::encode(&rejection_body).unwrap(),
        &relay_seed,
    )
    .unwrap();
    let rejection = cbor::encode(&Value::Map(vec![
        (1, rejection_body),
        (2, Value::Bytes(rejection_signature.to_vec())),
    ]))
    .unwrap();
    // The result names the rotation at cursor 9, but another authorized
    // device entry arrives before this installation queries it.
    let (_, later_child) = local_api::child_operation(family, "Later child", 200).unwrap();
    let later_operation = Operation::encode_new(&later_child).unwrap();
    let later_plain =
        cbor::encode(&Value::Array(vec![Value::Bytes(later_operation.clone())])).unwrap();
    let later_header = Header {
        minor: 0,
        family_id: family.family_id,
        relay_id: session.chain().relay_id(),
        control_head: session.head_hash(),
        epoch: 2,
        batch_id: bytes("a5e2c003a16b42d38a43df44a812b979"),
        author_device_id: family.device_id,
        device_sequence: old_pending.sequence,
        nonce: [7; 24],
        plaintext_len: later_plain.len() as u32,
    };
    let later_key = bytes::<32>(
        fixture["test_only_inputs"]["epoch_2_key_hex"]
            .as_str()
            .unwrap(),
    );
    let later = batch::seal(&later_header, &[later_operation], &later_key, &manager_seed).unwrap();
    let later_receipt_body = Value::Map(vec![
        (1, Value::Integer(1)),
        (2, Value::Bytes(family.family_id.to_vec())),
        (3, Value::Bytes(session.chain().relay_id().to_vec())),
        (4, Value::Bytes(later_header.batch_id.to_vec())),
        (5, Value::Bytes(later.object_hash.to_vec())),
        (6, Value::Bool(true)),
        (7, Value::Integer(10)),
        (8, Value::Bytes(session.head_hash().to_vec())),
        (9, Value::Integer(old_pending.sequence.into())),
        (10, Value::Null),
        (11, Value::Integer((old_pending.sequence + 1).into())),
    ]);
    let later_signature = crypto::sign_cbor(
        "batch-receipt",
        &cbor::encode(&later_receipt_body).unwrap(),
        &relay_seed,
    )
    .unwrap();
    let later_receipt = cbor::encode(&Value::Map(vec![
        (1, later_receipt_body),
        (2, Value::Bytes(later_signature.to_vec())),
    ]))
    .unwrap();
    session
        .accept_batch(&mut store, &later.envelope_bytes, &later_receipt)
        .unwrap();
    assert_eq!(session.cursor(), 10);
    let mut bad_rejection = rejection.clone();
    *bad_rejection.last_mut().unwrap() ^= 1;
    assert!(
        session
            .reject_stale_pending(&mut store, &bad_rejection)
            .is_err()
    );
    session
        .reject_stale_pending(&mut store, &rejection)
        .unwrap();
    let ready_after_later =
        ReadyFamilySession::from_store(&store, family, initial_key, manager_agreement).unwrap();
    let replacement = match ready_after_later
        .stage_next_local(&mut store, &manager_seed)
        .unwrap()
    {
        NextUpload::Fresh(pending) => pending,
        NextUpload::RetryExact(_) => panic!("signed rejection must permit a fresh batch"),
    };
    assert_eq!(replacement.sequence, old_pending.sequence + 1);
    assert_ne!(replacement.batch_id, old_pending.batch_id);
    assert_ne!(replacement.envelope_bytes, old_pending.envelope_bytes);
    drop(session);
    drop(store);
    let store = SqliteStore::open(&path).unwrap();
    let ready_after_restart =
        ReadyFamilySession::from_store(&store, family, initial_key, manager_agreement).unwrap();
    assert_eq!(ready_after_restart.observed_cursor(), 10);
    assert!(
        ready_after_restart
            .projection()
            .record(&operation.record_id)
            .is_some()
    );
    let mut store = store;
    match ready_after_restart
        .stage_next_local(&mut store, &manager_seed)
        .unwrap()
    {
        NextUpload::RetryExact(pending) => assert_eq!(pending, replacement),
        NextUpload::Fresh(_) => panic!("replacement retry changed bytes after restart"),
    }
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

#[test]
fn own_acceptance_and_interleaved_control_clear_outbox_atomically() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("../../tests/vectors/contiguous-chain-v1.json")).unwrap();
    let transitions = fixture["transitions"].as_array().unwrap();
    let wire =
        |index: usize| bytes_from_hex(transitions[index]["committed_cbor_hex"].as_str().unwrap());
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
    let relay_seed = bytes::<32>(
        fixture["test_only_inputs"]["relay_sign_seed_hex"]
            .as_str()
            .unwrap(),
    );
    let manager_seed = bytes::<32>(
        fixture["test_only_inputs"]["manager_sign_seed_hex"]
            .as_str()
            .unwrap(),
    );
    let initial_key = bytes::<32>(
        fixture["test_only_inputs"]["epoch_1_key_hex"]
            .as_str()
            .unwrap(),
    );
    let agreement = bytes::<32>(
        fixture["test_only_inputs"]["manager_agreement_seed_hex"]
            .as_str()
            .unwrap(),
    );
    let path = temp_db();
    let mut store = SqliteStore::open(&path).unwrap();
    store
        .create_family(family.family_id, family.device_id)
        .unwrap();
    let mut session =
        PublicHistorySession::begin(&mut store, family, &wire(0), relay_public).unwrap();
    for (index, transition) in transitions.iter().take(2).enumerate() {
        if index == 1 {
            session.accept_control(&mut store, &wire(index)).unwrap();
        }
        let id = transition["manifest"][0][1].as_str().unwrap();
        session
            .accept_object(
                &mut store,
                bytes(id),
                &bytes_from_hex(fixture["objects_by_id_hex"][id].as_str().unwrap()),
            )
            .unwrap();
    }
    store
        .append_local(
            family,
            NewOperation {
                family_id: family.family_id,
                operation_id: bytes("0183f9d0000070008000000000000055"),
                record_id: family.family_id,
                scope: Scope::Family,
                kind: Kind::Create,
                author_device_id: family.device_id,
                hlc: Hlc {
                    wall_ms: 0,
                    counter: 0,
                    device_id: family.device_id,
                },
                record_type: Some("family".to_owned()),
                child_id: None,
                fields: Some(vec![]),
            },
            100,
        )
        .unwrap();
    let ready = ReadyFamilySession::from_store(&store, family, initial_key, agreement).unwrap();
    let pending = match ready.stage_next_local(&mut store, &manager_seed).unwrap() {
        NextUpload::Fresh(pending) => pending,
        NextUpload::RetryExact(_) => panic!("first upload must be fresh"),
    };
    let batch_head = session.head_hash();
    session.accept_control(&mut store, &wire(2)).unwrap();
    let receipt_body = Value::Map(vec![
        (1, Value::Integer(1)),
        (2, Value::Bytes(family.family_id.to_vec())),
        (3, Value::Bytes(session.chain().relay_id().to_vec())),
        (4, Value::Bytes(pending.batch_id.to_vec())),
        (5, Value::Bytes(pending.object_hash.to_vec())),
        (6, Value::Bool(true)),
        (7, Value::Integer(4)),
        (8, Value::Bytes(batch_head.to_vec())),
        (9, Value::Integer(1)),
        (10, Value::Null),
        (11, Value::Integer(2)),
    ]);
    let signature = crypto::sign_cbor(
        "batch-receipt",
        &cbor::encode(&receipt_body).unwrap(),
        &relay_seed,
    )
    .unwrap();
    let receipt = cbor::encode(&Value::Map(vec![
        (1, receipt_body),
        (2, Value::Bytes(signature.to_vec())),
    ]))
    .unwrap();
    let blocker = rusqlite::Connection::open(&path).unwrap();
    blocker
        .execute_batch(
            "CREATE TRIGGER fail_outbox_clear BEFORE DELETE ON local_outbox
             BEGIN SELECT RAISE(FAIL, 'injected outbox failure'); END;",
        )
        .unwrap();
    assert!(
        session
            .accept_batch(&mut store, &pending.envelope_bytes, &receipt)
            .is_err()
    );
    assert_eq!(session.cursor(), 3);
    match ready.stage_next_local(&mut store, &manager_seed).unwrap() {
        NextUpload::RetryExact(again) => assert_eq!(again, pending),
        NextUpload::Fresh(_) => panic!("rollback lost uncertain batch"),
    }
    blocker
        .execute_batch("DROP TRIGGER fail_outbox_clear")
        .unwrap();
    drop(blocker);
    session
        .accept_batch(&mut store, &pending.envelope_bytes, &receipt)
        .unwrap();
    assert_eq!(session.cursor(), 4);
    drop(session);
    drop(store);
    let mut store = SqliteStore::open(&path).unwrap();
    let ready = ReadyFamilySession::from_store(&store, family, initial_key, agreement).unwrap();
    assert_eq!(ready.projection().last_cursor(), 4);
    assert!(ready.projection().record(&family.family_id).is_some());
    assert!(ready.stage_next_local(&mut store, &manager_seed).is_err());
    store
        .append_local(
            family,
            NewOperation {
                family_id: family.family_id,
                operation_id: bytes("0183f9d0000070008000000000000056"),
                record_id: bytes("0183f9d0000070008000000000000057"),
                scope: Scope::Child,
                kind: Kind::Create,
                author_device_id: family.device_id,
                hlc: Hlc {
                    wall_ms: 0,
                    counter: 0,
                    device_id: family.device_id,
                },
                record_type: Some("child".to_owned()),
                child_id: None,
                fields: Some(vec![(1, Value::Text("Baby".to_owned()))]),
            },
            101,
        )
        .unwrap();
    match ready.stage_next_local(&mut store, &manager_seed).unwrap() {
        NextUpload::Fresh(next) => assert_eq!(next.sequence, 2),
        NextUpload::RetryExact(_) => panic!("accepted outbox was not cleared"),
    }
    drop(store);
    fs::remove_file(path).unwrap();
}
