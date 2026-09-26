#![cfg(not(target_arch = "wasm32"))]

use babytrack_core::{
    batch,
    cbor::{self, Value},
    crypto,
    operation::{Hlc, Kind, NewOperation, Scope},
    session::FamilySession,
    sqlite_store::{FamilyHandle, SqliteStore},
};
use std::{path::PathBuf, time::SystemTime};

fn hex_bytes(hex: &str) -> Vec<u8> {
    hex.as_bytes()
        .chunks_exact(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}
fn bytes<const N: usize>(hex: &str) -> [u8; N] {
    hex_bytes(hex).try_into().unwrap()
}
fn temp_db() -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "babytrack-session-{}-{nonce}.sqlite",
        std::process::id()
    ))
}
fn receipt(
    pending: &babytrack_core::sqlite_store::PreparedBatch,
    family: FamilyHandle,
    relay_id: [u8; 32],
    head: [u8; 32],
    cursor: u64,
    seed: &[u8; 32],
) -> Vec<u8> {
    let body = Value::Map(vec![
        (1, Value::Integer(1)),
        (2, Value::Bytes(family.family_id.to_vec())),
        (3, Value::Bytes(relay_id.to_vec())),
        (4, Value::Bytes(pending.batch_id.to_vec())),
        (5, Value::Bytes(pending.object_hash.to_vec())),
        (6, Value::Bool(true)),
        (7, Value::Integer(cursor.into())),
        (8, Value::Bytes(head.to_vec())),
        (9, Value::Integer(pending.sequence.into())),
        (10, Value::Null),
        (11, Value::Integer((pending.sequence + 1).into())),
    ]);
    let signature =
        crypto::sign_cbor("batch-receipt", &cbor::encode(&body).unwrap(), seed).unwrap();
    cbor::encode(&Value::Map(vec![
        (1, body),
        (2, Value::Bytes(signature.to_vec())),
    ]))
    .unwrap()
}

#[test]
fn verified_acceptance_moves_exact_outbox_bytes_to_replayable_history() {
    let fixtures: serde_json::Value =
        serde_json::from_str(include_str!("../../tests/vectors/full-wire-v1.json")).unwrap();
    let genesis = fixtures["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["id"] == "GENESIS01")
        .unwrap();
    let genesis_bytes = hex_bytes(genesis["expect"]["control_object_hex"].as_str().unwrap());
    let relay_public = bytes::<32>(genesis["expect"]["relay_public_key_hex"].as_str().unwrap());
    let relay_seed = bytes::<32>(genesis["inputs"]["relay_sign_seed_hex"].as_str().unwrap());
    let manager_seed = bytes::<32>(genesis["inputs"]["manager_sign_seed_hex"].as_str().unwrap());
    let epoch_key = bytes::<32>(genesis["inputs"]["epoch_key_hex"].as_str().unwrap());
    let family = FamilyHandle {
        family_id: bytes(genesis["inputs"]["family_id_hex"].as_str().unwrap()),
        device_id: bytes(genesis["inputs"]["device_id_hex"].as_str().unwrap()),
    };
    let relay_id = bytes::<32>(genesis["expect"]["relay_id_hex"].as_str().unwrap());
    let head = bytes::<32>(genesis["expect"]["control_head_hex"].as_str().unwrap());
    let path = temp_db();
    let first;
    {
        let mut store = SqliteStore::open(&path).unwrap();
        store
            .create_family(family.family_id, family.device_id)
            .unwrap();
        let new = NewOperation {
            family_id: family.family_id,
            operation_id: bytes("0183f9d0000070008000000000000004"),
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
        };
        store.append_local(family, new, 100).unwrap();
        let mut session =
            FamilySession::from_genesis(&genesis_bytes, relay_public, epoch_key).unwrap();
        first = session
            .stage_next_local(&mut store, family, &manager_seed)
            .unwrap();
        assert_eq!(first.sequence, 1);
        assert!(
            session
                .stage_next_local(&mut store, family, &[0u8; 32])
                .is_err()
        );
        let good = receipt(&first, family, relay_id, head, 2, &relay_seed);
        let mut bad = good.clone();
        *bad.last_mut().unwrap() ^= 1;
        assert!(
            session
                .confirm_staged_local(&mut store, family, &bad)
                .is_err()
        );
        assert_eq!(session.projection().last_cursor(), 1);
        assert_eq!(
            session
                .stage_next_local(&mut store, family, &manager_seed)
                .unwrap(),
            first
        );
    }
    {
        let mut store = SqliteStore::open(&path).unwrap();
        let mut session = FamilySession::resume_from_store(
            &genesis_bytes,
            relay_public,
            epoch_key,
            &store,
            family,
        )
        .unwrap();
        assert_eq!(
            session
                .stage_next_local(&mut store, family, &manager_seed)
                .unwrap(),
            first
        );
        let signed = batch::verify_signed_envelope(
            &first.envelope_bytes,
            &family.family_id,
            &relay_id,
            &crypto::signing_public_key(&manager_seed),
        )
        .unwrap();
        assert_eq!(signed.header().device_sequence, 1);
        let good = receipt(&first, family, relay_id, head, 2, &relay_seed);
        session
            .confirm_staged_local(&mut store, family, &good)
            .unwrap();
        assert_eq!(session.projection().last_cursor(), 2);
        assert!(
            session
                .confirm_staged_local(&mut store, family, &good)
                .is_err()
        );
    }
    {
        let store = SqliteStore::open(&path).unwrap();
        let resumed = FamilySession::resume_from_store(
            &genesis_bytes,
            relay_public,
            epoch_key,
            &store,
            family,
        )
        .unwrap();
        assert_eq!(resumed.projection().last_cursor(), 2);
        assert!(resumed.projection().record(&family.family_id).is_some());
    }
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(path.with_extension("sqlite-wal"));
    let _ = std::fs::remove_file(path.with_extension("sqlite-shm"));
}
