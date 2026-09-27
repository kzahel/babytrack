#![cfg(not(target_arch = "wasm32"))]

use std::{fs, path::PathBuf, time::SystemTime};

use babytrack_core::{
    bootstrap::InvitationBootstrap,
    cbor::{self, Value},
    control_chain::ControlChain,
    crypto,
    enrollment::EnrollmentAttempt,
    shared_history::PublicHistorySession,
    sqlite_store::SqliteStore,
};

fn hex_bytes(hex: &str) -> Vec<u8> {
    hex.as_bytes()
        .chunks_exact(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}

fn fixed<const N: usize>(hex: &str) -> [u8; N] {
    hex_bytes(hex).try_into().unwrap()
}

fn temp_db() -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "babytrack-enrollment-{}-{nonce}.sqlite",
        std::process::id()
    ))
}

#[test]
fn join_claim_keeps_two_keys_and_exact_candidate_through_restart() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("../../tests/vectors/contiguous-chain-v1.json")).unwrap();
    let transitions = fixture["transitions"].as_array().unwrap();
    let genesis = hex_bytes(transitions[0]["committed_cbor_hex"].as_str().unwrap());
    let issue = hex_bytes(transitions[1]["committed_cbor_hex"].as_str().unwrap());
    let bootstrap =
        InvitationBootstrap::from_fragment(fixture["bootstrap"]["fragment"].as_str().unwrap())
            .unwrap();
    let relay_public =
        fixed::<32>("2543b92ff1095511476adc8369db6ddc933665a11978dda1404ee1066ca9559d");
    let relay_seed = fixed::<32>(
        fixture["test_only_inputs"]["relay_sign_seed_hex"]
            .as_str()
            .unwrap(),
    );
    let path = temp_db();
    let wrapping_key = [0x5au8; 32];
    let mut store = SqliteStore::open(&path).unwrap();
    let prepared =
        EnrollmentAttempt::prepare(&mut store, &bootstrap, &genesis, &issue, &wrapping_key)
            .unwrap();
    assert_eq!(prepared.invitation_id(), bootstrap.invitation_id());
    assert_eq!(prepared.family().family_id, bootstrap.family_id());
    assert_eq!(
        EnrollmentAttempt::prepare(&mut store, &bootstrap, &genesis, &issue, &wrapping_key)
            .unwrap()
            .claim_candidate(),
        prepared.claim_candidate()
    );
    let candidate = prepared.claim_candidate().to_vec();
    let family = prepared.family();
    let public_sign = prepared.device_sign_public();
    let public_agree = prepared.device_agreement_public().unwrap();
    drop(store);
    let mut store = SqliteStore::open(&path).unwrap();
    assert!(EnrollmentAttempt::resume(&mut store, family.family_id, &[0u8; 32]).is_err());
    let resumed = EnrollmentAttempt::resume(&mut store, family.family_id, &wrapping_key).unwrap();
    assert_eq!(resumed.family(), family);
    assert_eq!(resumed.device_sign_public(), public_sign);
    assert_eq!(resumed.device_agreement_public().unwrap(), public_agree);
    assert_eq!(resumed.claim_candidate(), candidate);
    let stored_ciphertext: Vec<u8> = rusqlite::Connection::open(&path)
        .unwrap()
        .query_row(
            "SELECT secret_ciphertext FROM enrollment_attempts WHERE family_id = ?1",
            [family.family_id.as_slice()],
            |row| row.get(0),
        )
        .unwrap();
    let invitation_seed = hex_bytes(
        fixture["test_only_inputs"]["invitation_sign_seed_hex"]
            .as_str()
            .unwrap(),
    );
    assert!(
        !stored_ciphertext
            .windows(invitation_seed.len())
            .any(|window| window == invitation_seed)
    );

    // Add an honest relay receipt to the exact stored candidate. The core
    // independently verifies both signatures, claim transcript, fixed role,
    // pending row, and strict signed expiry when replaying it.
    let Value::Map(candidate_map) = cbor::decode(&candidate).unwrap() else {
        unreachable!()
    };
    let unsigned = candidate_map[0].1.clone();
    let signatures = candidate_map[1].1.clone();
    let Value::Map(unsigned_fields) = &unsigned else {
        unreachable!()
    };
    let Value::Bytes(transition_id) = &unsigned_fields[4].1 else {
        unreachable!()
    };
    let Value::Map(issue_map) = cbor::decode(&issue).unwrap() else {
        unreachable!()
    };
    let Value::Array(issue_receipt) = &issue_map[2].1 else {
        unreachable!()
    };
    let Value::Integer(issue_ms) = issue_receipt[4] else {
        unreachable!()
    };
    let signed = Value::Array(vec![unsigned.clone(), signatures.clone()]);
    let receipt = Value::Array(vec![
        Value::Bytes(family.family_id.to_vec()),
        Value::Bytes(
            bootstrap
                .verify_issue(&genesis, &issue)
                .unwrap()
                .relay_id()
                .to_vec(),
        ),
        Value::Bytes(transition_id.clone()),
        Value::Integer(3),
        Value::Integer(issue_ms + 1),
        Value::Bytes(
            crypto::hash("control-signed", &cbor::encode(&signed).unwrap())
                .unwrap()
                .to_vec(),
        ),
    ]);
    let relay_signature = crypto::sign_cbor(
        "control-receipt",
        &cbor::encode(&receipt).unwrap(),
        &relay_seed,
    )
    .unwrap();
    let committed = cbor::encode(&Value::Map(vec![
        (1, unsigned),
        (2, signatures),
        (3, receipt),
        (4, Value::Bytes(relay_signature.to_vec())),
    ]))
    .unwrap();
    let Value::Map(mut wrong_claim) = cbor::decode(&committed).unwrap() else {
        unreachable!()
    };
    wrong_claim[1].1 = Value::Array(vec![]);
    assert!(
        resumed
            .confirm_sparse_claim(&mut store, &cbor::encode(&Value::Map(wrong_claim)).unwrap())
            .is_err()
    );
    assert_eq!(
        PublicHistorySession::resume(&store, family)
            .unwrap()
            .cursor(),
        2
    );
    let mut chain = ControlChain::from_genesis(&genesis, relay_public).unwrap();
    chain.apply_invite_issue(&issue).unwrap();
    chain.apply_invite_claim(&committed).unwrap();
    assert_eq!(chain.last_global_cursor(), 3);
    let mut public = PublicHistorySession::resume(&store, family).unwrap();
    public.accept_control(&mut store, &committed).unwrap();
    assert_eq!(public.cursor(), 3);
    drop(store);
    let mut store = SqliteStore::open(&path).unwrap();
    assert_eq!(
        EnrollmentAttempt::resume(&mut store, family.family_id, &wrapping_key)
            .unwrap()
            .claim_candidate(),
        candidate
    );
    drop(store);
    fs::remove_file(path).unwrap();
}
