//! Relay storage regressions, kept beside the storage implementation.
use super::*;
use serde_json::Value as Json;
fn hex(value: &str) -> Vec<u8> {
    (0..value.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&value[i..i + 2], 16).unwrap())
        .collect()
}

#[test]
fn large_canonical_entry_pages_advance_under_four_mib() {
    let db = Connection::open_in_memory().unwrap();
    db.execute_batch(
        "CREATE TABLE entries (
                family_id BLOB NOT NULL, cursor INTEGER NOT NULL,
                kind INTEGER NOT NULL, committed_bytes BLOB NOT NULL
            )",
    )
    .unwrap();
    let family = [0x44; 16];
    let entry = cbor::encode(&Value::Bytes(vec![0x77; 200 * 1024])).unwrap();
    for cursor in 1..=25 {
        db.execute(
            "INSERT INTO entries(family_id,cursor,kind,committed_bytes) VALUES(?1,?2,2,?3)",
            params![&family[..], cursor, &entry],
        )
        .unwrap();
    }
    let (first, more) = load_page_entries(&db, family, 0, None).unwrap();
    assert!(more);
    assert!(!first.is_empty());
    assert!(first.len() < 25);
    assert!(
        receipt::encode_log_page(family, 0, &first, more)
            .unwrap()
            .len()
            <= 4 * 1024 * 1024
    );
    let after = first.last().unwrap().cursor;
    let (second, more) = load_page_entries(&db, family, after, None).unwrap();
    assert!(!more);
    assert_eq!(first.len() + second.len(), 25);
    assert!(
        receipt::encode_log_page(family, after, &second, more)
            .unwrap()
            .len()
            <= 4 * 1024 * 1024
    );
    let (filtered, more) = load_page_entries(&db, family, 0, Some(2)).unwrap();
    assert!(more);
    assert_eq!(filtered.len(), first.len());
    assert!(
        receipt::encode_batch_page(family, 0, &filtered, more)
            .unwrap()
            .len()
            <= 4 * 1024 * 1024
    );
}

#[test]
fn signed_large_batches_page_through_authenticated_reads() {
    let genesis: Json = serde_json::from_str(
        &std::fs::read_to_string("../tests/vectors/api-genesis-v1.json").unwrap(),
    )
    .unwrap();
    let chain: Json = serde_json::from_str(
        &std::fs::read_to_string("../tests/vectors/contiguous-chain-v1.json").unwrap(),
    )
    .unwrap();
    let seed: [u8; 32] = hex(chain["test_only_inputs"]["relay_sign_seed_hex"]
        .as_str()
        .unwrap())
    .try_into()
    .unwrap();
    let manager_seed: [u8; 32] = hex(chain["test_only_inputs"]["manager_sign_seed_hex"]
        .as_str()
        .unwrap())
    .try_into()
    .unwrap();
    let family: [u8; 16] = hex(genesis["inputs"]["family_id_hex"].as_str().unwrap())
        .try_into()
        .unwrap();
    let promotion: [u8; 16] = hex(genesis["inputs"]["promotion_id_hex"].as_str().unwrap())
        .try_into()
        .unwrap();
    let candidate = hex(genesis["inputs"]["commit_candidate_cbor_hex"]
        .as_str()
        .unwrap());
    let response = hex(genesis["expect"]["commit_response_cbor_hex"]
        .as_str()
        .unwrap());
    let Value::Map(fields) = cbor::decode(&response).unwrap() else {
        panic!()
    };
    let Value::Bytes(genesis_committed) = &fields[1].1 else {
        panic!()
    };
    let parsed =
        authority::verify_genesis_candidate(&candidate, &crypto::signing_public_key(&seed))
            .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("relay.sqlite");
    let mut store = RelayStore::open(&path, seed).unwrap();
    store
        .stage_genesis_object(
            family,
            promotion,
            &hex(genesis["inputs"]["stage_body_cbor_hex"].as_str().unwrap()),
        )
        .unwrap();
    store
        .commit_genesis(
            family,
            &candidate,
            control_commit_time(genesis_committed).unwrap(),
        )
        .unwrap();
    let base = signed_manager_batch(genesis_committed, &parsed, manager_seed);
    let (base_id, _) = batch_authority::claimed_identity(&base).unwrap();
    let Value::Map(mut outer) = cbor::decode(&base).unwrap() else {
        panic!()
    };
    let Value::Map(header) = &mut outer[0].1 else {
        panic!()
    };
    header[9].1 = Value::Integer(200 * 1024);
    outer[1].1 = Value::Bytes(vec![0x7a; 200 * 1024 + 16]);
    let padded = cbor::encode(&Value::Map(outer)).unwrap();
    for sequence in 1..=25 {
        let mut batch_id = base_id;
        batch_id[15] = batch_id[15].wrapping_add(sequence as u8);
        let envelope = resign_batch_author(
            &padded,
            parsed.manager_id,
            sequence,
            Some(batch_id),
            manager_seed,
        );
        assert_eq!(
            batch_rejection_reason(&store.commit_batch(family, &envelope).unwrap()),
            None
        );
    }
    drop(store);
    let mut store = RelayStore::open(&path, seed).unwrap();
    for filtered in [false, true] {
        let mut after = 0;
        let mut count = 0;
        let mut finished = false;
        for page_number in 0..10 {
            let path = format!(
                "/v1/families/{}/{}?after={after}",
                lower_hex(&family),
                if filtered { "batches" } else { "log" },
            );
            let mut request_id = parsed.transition_id;
            request_id[15] ^= page_number + if filtered { 0x40 } else { 0x20 };
            let auth = signed_get(
                family,
                parsed.relay_id,
                parsed.manager_id,
                manager_seed,
                &path,
                request_id,
            );
            let page = if filtered {
                store.batch_page_authenticated(family, after, &path, &auth)
            } else {
                store.log_page_authenticated(family, after, &path, &auth)
            }
            .unwrap();
            assert!(page.len() <= 4 * 1024 * 1024);
            let Value::Map(fields) = cbor::decode_with_limits(
                &page,
                cbor::Limits {
                    max_bytes: 4 * 1024 * 1024,
                    max_depth: 16,
                },
            )
            .unwrap() else {
                panic!()
            };
            let Value::Array(entries) = &fields[3].1 else {
                panic!()
            };
            assert!(!entries.is_empty());
            count += entries.len();
            let Value::Integer(next) = fields[4].1 else {
                panic!()
            };
            after = next.try_into().unwrap();
            let Value::Bool(more) = fields[5].1 else {
                panic!()
            };
            if !more {
                finished = true;
                break;
            }
        }
        assert!(finished);
        assert_eq!(count, if filtered { 25 } else { 26 });
        assert_eq!(after, 26);
    }
}

#[test]
fn legacy_private_checkpoint_requires_explicit_rebaseline() {
    let genesis: Json = serde_json::from_str(
        &std::fs::read_to_string("../tests/vectors/api-genesis-v1.json").unwrap(),
    )
    .unwrap();
    let chain: Json = serde_json::from_str(
        &std::fs::read_to_string("../tests/vectors/contiguous-chain-v1.json").unwrap(),
    )
    .unwrap();
    let seed: [u8; 32] = hex(chain["test_only_inputs"]["relay_sign_seed_hex"]
        .as_str()
        .unwrap())
    .try_into()
    .unwrap();
    let family: [u8; 16] = hex(genesis["inputs"]["family_id_hex"].as_str().unwrap())
        .try_into()
        .unwrap();
    let promotion: [u8; 16] = hex(genesis["inputs"]["promotion_id_hex"].as_str().unwrap())
        .try_into()
        .unwrap();
    let stage = hex(genesis["inputs"]["stage_body_cbor_hex"].as_str().unwrap());
    let candidate = hex(genesis["inputs"]["commit_candidate_cbor_hex"]
        .as_str()
        .unwrap());
    let expected = hex(genesis["expect"]["commit_response_cbor_hex"]
        .as_str()
        .unwrap());
    let Value::Map(response) = cbor::decode(&expected).unwrap() else {
        panic!()
    };
    let Value::Bytes(committed) = &response[1].1 else {
        panic!()
    };
    let time = control_commit_time(committed).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("legacy.sqlite");
    let absent = dir.path().join("missing.sqlite");
    assert!(RelayStore::migrate_legacy_private_checkpoint(&absent, seed).is_err());
    assert!(!absent.exists());
    let mut store = RelayStore::open(&path, seed).unwrap();
    store
        .stage_genesis_object(family, promotion, &stage)
        .unwrap();
    store.commit_genesis(family, &candidate, time).unwrap();
    drop(store);
    let db = Connection::open(&path).unwrap();
    db.execute("DELETE FROM private_integrity", []).unwrap();
    drop(db);
    assert!(RelayStore::open(&path, seed).is_err());
    RelayStore::migrate_legacy_private_checkpoint(&path, seed).unwrap();
    assert!(RelayStore::open(&path, seed).is_ok());
}

fn cancel_candidate(
    issue_candidate: &[u8],
    state: &Value,
    head: [u8; 32],
    manager: [u8; 16],
    manager_seed: [u8; 32],
    transition: [u8; 16],
) -> Vec<u8> {
    let Value::Map(root) = cbor::decode(issue_candidate).unwrap() else {
        panic!()
    };
    let Value::Map(mut unsigned) = root[0].1.clone() else {
        panic!()
    };
    let Value::Map(issue_delta) = &unsigned[6].1 else {
        panic!()
    };
    let invitation = issue_delta[0].1.clone();
    let mut next = state.clone();
    let Value::Map(next_fields) = &mut next else {
        panic!()
    };
    let Value::Array(invitations) = &mut next_fields[6].1 else {
        panic!()
    };
    let Value::Array(row) = &mut invitations[0] else {
        panic!()
    };
    row[5] = Value::Integer(3);
    unsigned[3].1 = Value::Bytes(head.to_vec());
    unsigned[4].1 = Value::Bytes(transition.to_vec());
    unsigned[5].1 = Value::Integer(3);
    unsigned[6].1 = Value::Map(vec![(1, invitation)]);
    unsigned[7].1 = Value::Bytes(
        crypto::hash("auth-state", &cbor::encode(&next).unwrap())
            .unwrap()
            .to_vec(),
    );
    unsigned[9].1 = Value::Array(vec![]);
    let core = Value::Array(
        unsigned[..9]
            .iter()
            .map(|(_, value)| value.clone())
            .collect(),
    );
    unsigned[10].1 = Value::Bytes(
        crypto::hash("transition-core", &cbor::encode(&core).unwrap())
            .unwrap()
            .to_vec(),
    );
    let unsigned_bytes = cbor::encode(&Value::Map(unsigned.clone())).unwrap();
    let signature =
        crypto::sign_cbor("control-transition", &unsigned_bytes, &manager_seed).unwrap();
    cbor::encode(&Value::Map(vec![
        (1, Value::Map(unsigned)),
        (
            2,
            Value::Array(vec![Value::Array(vec![
                Value::Bytes(manager.to_vec()),
                Value::Bytes(signature.to_vec()),
            ])]),
        ),
    ]))
    .unwrap()
}

#[test]
fn manager_cancel_commits_from_verified_ledger_and_retries_exactly() {
    let genesis: Json = serde_json::from_str(
        &std::fs::read_to_string("../tests/vectors/api-genesis-v1.json").unwrap(),
    )
    .unwrap();
    let issue: Json =
        serde_json::from_str(&std::fs::read_to_string("../tests/vectors/api-v1.json").unwrap())
            .unwrap();
    let chain: Json = serde_json::from_str(
        &std::fs::read_to_string("../tests/vectors/contiguous-chain-v1.json").unwrap(),
    )
    .unwrap();
    let seed: [u8; 32] = hex(chain["test_only_inputs"]["relay_sign_seed_hex"]
        .as_str()
        .unwrap())
    .try_into()
    .unwrap();
    let manager_seed: [u8; 32] = hex(chain["test_only_inputs"]["manager_sign_seed_hex"]
        .as_str()
        .unwrap())
    .try_into()
    .unwrap();
    let manager: [u8; 16] = hex(chain["test_only_inputs"]["manager_device_id_hex"]
        .as_str()
        .unwrap())
    .try_into()
    .unwrap();
    let family: [u8; 16] = hex(genesis["inputs"]["family_id_hex"].as_str().unwrap())
        .try_into()
        .unwrap();
    let promotion: [u8; 16] = hex(genesis["inputs"]["promotion_id_hex"].as_str().unwrap())
        .try_into()
        .unwrap();
    let issue_object: [u8; 16] = hex("083e4567e89b42d3a456426614174000").try_into().unwrap();
    let genesis_stage = hex(genesis["inputs"]["stage_body_cbor_hex"].as_str().unwrap());
    let genesis_candidate = hex(genesis["inputs"]["commit_candidate_cbor_hex"]
        .as_str()
        .unwrap());
    let issue_stage = hex(issue["inputs"]["stage_body_cbor_hex"].as_str().unwrap());
    let issue_candidate = hex(issue["inputs"]["commit_body_cbor_hex"].as_str().unwrap());
    let genesis_response = hex(genesis["expect"]["commit_response_cbor_hex"]
        .as_str()
        .unwrap());
    let Value::Map(response) = cbor::decode(&genesis_response).unwrap() else {
        panic!()
    };
    let Value::Bytes(genesis_committed) = &response[1].1 else {
        panic!()
    };
    let genesis_ms = control_commit_time(genesis_committed).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("relay.sqlite");
    let mut store = RelayStore::open(&path, seed).unwrap();
    store
        .stage_genesis_object(family, promotion, &genesis_stage)
        .unwrap();
    store
        .commit_genesis(family, &genesis_candidate, genesis_ms)
        .unwrap();
    store
        .stage_first_issue_object(family, issue_object, &issue_stage)
        .unwrap();
    store
        .commit_first_issue(family, &issue_candidate, genesis_ms + 1_000)
        .unwrap();
    let issue_committed = control_at(&store.db, family, 1).unwrap();
    let genesis_verified =
        authority::verify_genesis_candidate(&genesis_candidate, &store.relay_public).unwrap();
    let batch = signed_manager_batch(&issue_committed, &genesis_verified, manager_seed);
    store.commit_batch(family, &batch).unwrap();
    let ledger =
        RelayStore::verify_saved_family(&store.db, family, store.relay_public, &seed).unwrap();
    let candidate = cancel_candidate(
        &issue_candidate,
        ledger.state(),
        ledger.head(),
        manager,
        manager_seed,
        [0xc3; 16],
    );
    let stale = cancel_candidate(
        &issue_candidate,
        ledger.state(),
        [0; 32],
        manager,
        manager_seed,
        [0xc4; 16],
    );
    assert!(
        store
            .commit_manager_change_with_clock(family, &stale, || Ok(genesis_ms + 2_000))
            .is_err()
    );
    let wrong_signer = cancel_candidate(
        &issue_candidate,
        ledger.state(),
        ledger.head(),
        manager,
        [0; 32],
        [0xc5; 16],
    );
    assert!(
        store
            .commit_manager_change_with_clock(family, &wrong_signer, || Ok(genesis_ms + 2_000))
            .is_err()
    );
    assert!(
        store
            .commit_manager_change_with_clock(family, &candidate, || Ok(genesis_ms))
            .is_err()
    );
    let accepted = store
        .commit_manager_change_with_clock(family, &candidate, || Ok(genesis_ms + 2_000))
        .unwrap();
    let after_cancel =
        RelayStore::verify_saved_family(&store.db, family, store.relay_public, &seed).unwrap();
    let Value::Map(issue_root) = cbor::decode(&issue_candidate).unwrap() else {
        panic!()
    };
    let Value::Map(issue_unsigned) = &issue_root[0].1 else {
        panic!()
    };
    let Value::Map(issue_delta) = &issue_unsigned[6].1 else {
        panic!()
    };
    let invitation_id = fixed::<16>(&issue_delta[0].1).unwrap();
    assert!(after_cancel.reader(invitation_id).unwrap().is_none());
    let invitation_seed: [u8; 32] = hex(chain["test_only_inputs"]["invitation_sign_seed_hex"]
        .as_str()
        .unwrap())
    .try_into()
    .unwrap();
    let status_path = format!(
        "/v1/families/{}/invitation-status/{}",
        lower_hex(&family),
        lower_hex(&invitation_id)
    );
    let status_read = signed_get(
        family,
        after_cancel.relay_id(),
        invitation_id,
        invitation_seed,
        &status_path,
        [0xd1; 16],
    );
    let status_response = store
        .invitation_status_authenticated(
            family,
            invitation_id,
            &status_path,
            &status_read,
            genesis_ms + 2_001,
        )
        .unwrap();
    let Value::Map(status_fields) = cbor::decode(&status_response).unwrap() else {
        panic!()
    };
    let Value::Bytes(status_body) = &status_fields[1].1 else {
        panic!()
    };
    let Value::Bytes(status_signature) = &status_fields[2].1 else {
        panic!()
    };
    crypto::verify_cbor(
        "invitation-status",
        status_body,
        &store.relay_public,
        &status_signature.as_slice().try_into().unwrap(),
    )
    .unwrap();
    let Value::Array(status_parts) = cbor::decode(status_body).unwrap() else {
        panic!()
    };
    assert_eq!(status_parts[4], Value::Integer(3));
    assert_eq!(status_parts[5], Value::Integer(4));
    assert_eq!(
        after_cancel
            .invitation_status(invitation_id, genesis_ms + 1_000 + 604_800_000)
            .unwrap()
            .unwrap()
            .1,
        3
    );
    assert!(
        store
            .control_page_authenticated(
                family,
                0,
                &format!("/v1/families/{}/control?after=0", lower_hex(&family)),
                &signed_get(
                    family,
                    after_cancel.relay_id(),
                    invitation_id,
                    invitation_seed,
                    &format!("/v1/families/{}/control?after=0", lower_hex(&family)),
                    [0xd2; 16],
                )
            )
            .is_err()
    );
    let cursor: i64 = store
        .db
        .query_row(
            "SELECT cursor FROM families WHERE family_id=?1",
            [&family[..]],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(cursor, 4);
    assert_eq!(
        store
            .commit_manager_change_with_clock(family, &candidate, || panic!("retry called clock"))
            .unwrap(),
        accepted
    );
    let different = cancel_candidate(
        &issue_candidate,
        ledger.state(),
        [0; 32],
        manager,
        manager_seed,
        [0xc3; 16],
    );
    assert_ne!(candidate, different);
    assert!(
        store
            .commit_manager_change_with_clock(family, &different, || panic!(
                "duplicate called clock"
            ))
            .is_err()
    );
    drop(store);
    let mut store = RelayStore::open(&path, seed).unwrap();
    let resumed_status = store
        .invitation_status_authenticated(
            family,
            invitation_id,
            &status_path,
            &signed_get(
                family,
                after_cancel.relay_id(),
                invitation_id,
                invitation_seed,
                &status_path,
                [0xd3; 16],
            ),
            genesis_ms + 3_000,
        )
        .unwrap();
    let Value::Map(resumed_fields) = cbor::decode(&resumed_status).unwrap() else {
        panic!()
    };
    let Value::Bytes(resumed_body) = &resumed_fields[1].1 else {
        panic!()
    };
    let Value::Array(resumed_parts) = cbor::decode(resumed_body).unwrap() else {
        panic!()
    };
    assert_eq!(resumed_parts[4], Value::Integer(3));
    assert_eq!(
        store
            .commit_manager_change_with_clock(family, &candidate, || panic!(
                "restart retry called clock"
            ))
            .unwrap(),
        accepted
    );
    // A later control changed the head, but this durable manager batch
    // still names an ancestor in the same epoch and uses the next sequence.
    let (mut second_id, _) = batch_authority::claimed_identity(&batch).unwrap();
    second_id[15] ^= 0x7f;
    let second = resign_batch_author(&batch, manager, 2, Some(second_id), manager_seed);
    let second_result = store.commit_batch(family, &second).unwrap();
    assert_eq!(store.commit_batch(family, &second).unwrap(), second_result);
    drop(store);
    let mut reopened = RelayStore::open(&path, seed).unwrap();
    assert_eq!(
        reopened.commit_batch(family, &second).unwrap(),
        second_result
    );
}

#[test]
fn general_control_writer_replays_published_chain_and_batch() {
    let fixture: Json = serde_json::from_str(
        &std::fs::read_to_string("../tests/vectors/contiguous-chain-v1.json").unwrap(),
    )
    .unwrap();
    let genesis: Json = serde_json::from_str(
        &std::fs::read_to_string("../tests/vectors/api-genesis-v1.json").unwrap(),
    )
    .unwrap();
    let seed: [u8; 32] = hex(fixture["test_only_inputs"]["relay_sign_seed_hex"]
        .as_str()
        .unwrap())
    .try_into()
    .unwrap();
    let family: [u8; 16] = hex(genesis["inputs"]["family_id_hex"].as_str().unwrap())
        .try_into()
        .unwrap();
    let promotion: [u8; 16] = hex(genesis["inputs"]["promotion_id_hex"].as_str().unwrap())
        .try_into()
        .unwrap();
    let transitions = fixture["transitions"].as_array().unwrap();
    let objects = fixture["objects_by_id_hex"].as_object().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("relay.sqlite");
    let mut store = RelayStore::open(&path, seed).unwrap();
    store
        .stage_genesis_object(
            family,
            promotion,
            &hex(genesis["inputs"]["stage_body_cbor_hex"].as_str().unwrap()),
        )
        .unwrap();
    let genesis_candidate = hex(genesis["inputs"]["commit_candidate_cbor_hex"]
        .as_str()
        .unwrap());
    let first_committed = hex(transitions[0]["committed_cbor_hex"].as_str().unwrap());
    store
        .commit_genesis(
            family,
            &genesis_candidate,
            control_commit_time(&first_committed).unwrap(),
        )
        .unwrap();
    assert_eq!(control_at(&store.db, family, 0).unwrap(), first_committed);
    for (index, transition) in transitions.iter().enumerate().skip(1) {
        if transition["name"] == "remove_active" {
            let batch = hex(fixture["batch"]["envelope_cbor_hex"].as_str().unwrap());
            store.commit_batch(family, &batch).unwrap();
        }
        let unsigned =
            cbor::decode(&hex(transition["unsigned_cbor_hex"].as_str().unwrap())).unwrap();
        let signatures =
            cbor::decode(&hex(transition["signatures_cbor_hex"].as_str().unwrap())).unwrap();
        let candidate = cbor::encode(&Value::Map(vec![
            (1, unsigned.clone()),
            (2, signatures.clone()),
        ]))
        .unwrap();
        if transition["name"] == "invite_issue" {
            let signed = hex(transition["committed_cbor_hex"].as_str().unwrap());
            assert!(
                store
                    .commit_general_control_with_clock(family, &candidate, || {
                        Ok(control_commit_time(&signed).unwrap())
                    })
                    .is_err()
            );
        }
        for item in transition["manifest"].as_array().unwrap() {
            let kind = item[0].as_u64().unwrap();
            let id_hex = item[1].as_str().unwrap();
            let id: [u8; 16] = hex(id_hex).try_into().unwrap();
            let bytes = hex(objects[id_hex].as_str().unwrap());
            if kind == 4 {
                let ledger =
                    RelayStore::verify_saved_family(&store.db, family, store.relay_public, &seed)
                        .unwrap();
                assert!(
                    validate_general_grant_object(&candidate, ledger.state(), 4, id, &bytes,)
                        .unwrap()
                        .is_some()
                );
                for field in [2, 3, 4, 6] {
                    let Value::Map(mut grant) = cbor::decode(&bytes).unwrap() else {
                        panic!()
                    };
                    grant[field].1 = match field {
                        2 | 4 => Value::Integer(999),
                        3 => Value::Bytes([0x99; 16].to_vec()),
                        6 => Value::Bytes([0x99; 32].to_vec()),
                        _ => unreachable!(),
                    };
                    let altered = cbor::encode(&Value::Map(grant)).unwrap();
                    assert!(
                        validate_general_grant_object(&candidate, ledger.state(), 4, id, &altered,)
                            .is_err()
                    );
                }
            }
            let body = cbor::encode(&Value::Map(vec![
                (1, Value::Integer(1)),
                (2, unsigned.clone()),
                (3, signatures.clone()),
                (4, Value::Integer(kind.into())),
                (5, Value::Bytes(id.to_vec())),
                (6, Value::Bytes(bytes)),
            ]))
            .unwrap();
            if transition["name"] == "invite_issue" {
                let mut changed = body.clone();
                *changed.last_mut().unwrap() ^= 1;
                assert!(
                    store
                        .stage_general_control_object(family, id, &changed)
                        .is_err()
                );
            }
            store
                .stage_general_control_object(family, id, &body)
                .unwrap();
        }
        let committed = hex(transition["committed_cbor_hex"].as_str().unwrap());
        let expected_time = control_commit_time(&committed).unwrap();
        store
            .commit_general_control_with_clock(family, &candidate, || Ok(expected_time))
            .unwrap();
        assert!(
            control_at(&store.db, family, index as i64).unwrap() == committed,
            "published bytes differ at {}",
            transition["name"]
        );
        assert!(
            store
                .commit_general_control_with_clock(family, &candidate, || panic!("retry clock"))
                .is_ok()
        );
    }
    let manager: [u8; 16] = hex(fixture["test_only_inputs"]["manager_device_id_hex"]
        .as_str()
        .unwrap())
    .try_into()
    .unwrap();
    let manager_seed: [u8; 32] = hex(fixture["test_only_inputs"]["manager_sign_seed_hex"]
        .as_str()
        .unwrap())
    .try_into()
    .unwrap();
    let original = hex(fixture["batch"]["envelope_cbor_hex"].as_str().unwrap());
    let (original_id, _) = batch_authority::claimed_identity(&original).unwrap();
    let Value::Map(mut outer) = cbor::decode(&original).unwrap() else {
        panic!()
    };
    let next_sequence =
        RelayStore::verify_saved_family(&store.db, family, store.relay_public, &seed)
            .unwrap()
            .next_sequence(manager);
    let mut stale_id = original_id;
    stale_id[15] ^= 0xe1;
    let stale = resign_batch_author(
        &original,
        manager,
        next_sequence,
        Some(stale_id),
        manager_seed,
    );
    assert_eq!(
        batch_rejection_reason(&store.commit_batch(family, &stale).unwrap()),
        Some(1)
    );
    let Value::Map(header) = &mut outer[0].1 else {
        panic!()
    };
    header[4].1 = Value::Integer(2);
    let mut wrong_id = original_id;
    wrong_id[15] ^= 0xe2;
    let wrong_head = resign_batch_author(
        &cbor::encode(&Value::Map(outer.clone())).unwrap(),
        manager,
        next_sequence,
        Some(wrong_id),
        manager_seed,
    );
    assert_eq!(
        batch_rejection_reason(&store.commit_batch(family, &wrong_head).unwrap()),
        Some(3)
    );
    let Value::Map(header) = &mut outer[0].1 else {
        panic!()
    };
    header[3].1 = Value::Bytes(hex(transitions.last().unwrap()["head_hash_hex"]
        .as_str()
        .unwrap()));
    let mut current_id = original_id;
    current_id[15] ^= 0xe3;
    let current = resign_batch_author(
        &cbor::encode(&Value::Map(outer)).unwrap(),
        manager,
        next_sequence,
        Some(current_id),
        manager_seed,
    );
    assert_eq!(
        batch_rejection_reason(&store.commit_batch(family, &current).unwrap()),
        None
    );
    let ledger =
        RelayStore::verify_saved_family(&store.db, family, store.relay_public, &seed).unwrap();
    let (candidate, stage, invitation, object_id) = later_issue_candidate(
        &ledger,
        &transitions[1],
        family,
        manager,
        manager_seed,
        hex(fixture["test_only_inputs"]["epoch_2_key_hex"]
            .as_str()
            .unwrap())
        .try_into()
        .unwrap(),
    );
    store
        .stage_general_control_object(family, object_id, &stage)
        .unwrap();
    let issue_response = store
        .commit_general_control_with_clock(family, &candidate, || {
            Ok(ledger.last_commit_ms() + 1_000)
        })
        .unwrap();
    assert_eq!(
        store
            .commit_general_control_with_clock(family, &candidate, || panic!("retry clock"))
            .unwrap(),
        issue_response
    );
    let after =
        RelayStore::verify_saved_family(&store.db, family, store.relay_public, &seed).unwrap();
    assert!(matches!(
        after.reader(invitation).unwrap(),
        Some(public_ledger::PublicReader::Invitation { .. })
    ));
    let (claim, recipient, recipient_seed) = later_claim_candidate(
        &after,
        &transitions[2],
        family,
        invitation,
        hex(fixture["test_only_inputs"]["recipient_device_id_hex"]
            .as_str()
            .unwrap())
        .try_into()
        .unwrap(),
    );
    store
        .commit_general_control_with_clock(family, &claim, || Ok(after.last_commit_ms() + 1_000))
        .unwrap();
    let pending =
        RelayStore::verify_saved_family(&store.db, family, store.relay_public, &seed).unwrap();
    assert!(pending.reader(invitation).unwrap().is_none());
    assert!(matches!(
        pending.reader(recipient).unwrap(),
        Some(public_ledger::PublicReader::Pending { .. })
    ));
    let mut pending_id = original_id;
    pending_id[15] ^= 0xe4;
    let pending_batch =
        resign_batch_author(&original, recipient, 1, Some(pending_id), recipient_seed);
    assert_eq!(
        batch_rejection_reason(&store.commit_batch(family, &pending_batch).unwrap()),
        Some(2)
    );
    drop(store);
    assert!(RelayStore::open(&path, seed).is_ok());
}
fn later_claim_candidate(
    ledger: &public_ledger::PublicLedger,
    first_claim: &Json,
    family: [u8; 16],
    invitation: [u8; 16],
    first_recipient: [u8; 16],
) -> (Vec<u8>, [u8; 16], [u8; 32]) {
    let mut recipient = first_recipient;
    recipient[15] ^= 0x84;
    let recipient_seed = [0x88; 32];
    let signing_public = crypto::signing_public_key(&recipient_seed);
    let Value::Map(old_unsigned) =
        cbor::decode(&hex(first_claim["unsigned_cbor_hex"].as_str().unwrap())).unwrap()
    else {
        panic!()
    };
    let Value::Map(old_delta) = &old_unsigned[6].1 else {
        panic!()
    };
    let agreement_public = fixed::<32>(&old_delta[3].1).unwrap();
    let mut transition = fixed::<16>(&old_unsigned[4].1).unwrap();
    transition[15] ^= 0x85;
    let enrollment_nonce = [0x86; 32];
    let claim_input = Value::Array(vec![
        Value::Bytes(family.to_vec()),
        Value::Bytes(ledger.relay_id().to_vec()),
        Value::Bytes(invitation.to_vec()),
        Value::Integer(1),
        Value::Bytes(recipient.to_vec()),
        Value::Bytes(signing_public.to_vec()),
        Value::Bytes(agreement_public.to_vec()),
        Value::Integer(1),
        Value::Bytes(enrollment_nonce.to_vec()),
        Value::Bytes(ledger.head().to_vec()),
    ]);
    let claim_hash = crypto::hash("claim", &cbor::encode(&claim_input).unwrap()).unwrap();
    let mut state = ledger.state().clone();
    let Value::Map(fields) = &mut state else {
        panic!()
    };
    let Value::Array(invitations) = &mut fields[6].1 else {
        panic!()
    };
    let row = invitations
            .iter_mut()
            .find(|row| matches!(row, Value::Array(parts) if parts[0] == Value::Bytes(invitation.to_vec())))
            .unwrap();
    let Value::Array(parts) = row else { panic!() };
    parts[5] = Value::Integer(2);
    let Value::Array(pending) = &mut fields[5].1 else {
        panic!()
    };
    pending.push(Value::Array(vec![
        Value::Bytes(invitation.to_vec()),
        Value::Bytes(recipient.to_vec()),
        Value::Bytes(signing_public.to_vec()),
        Value::Bytes(agreement_public.to_vec()),
        Value::Integer(1),
        Value::Integer(1),
        Value::Bytes(claim_hash.to_vec()),
        Value::Null,
        Value::Null,
    ]));
    pending.sort_by_key(|row| match row {
        Value::Array(parts) => match &parts[0] {
            Value::Bytes(id) => id.clone(),
            _ => panic!(),
        },
        _ => panic!(),
    });
    let delta = Value::Map(vec![
        (1, Value::Bytes(invitation.to_vec())),
        (2, Value::Bytes(recipient.to_vec())),
        (3, Value::Bytes(signing_public.to_vec())),
        (4, Value::Bytes(agreement_public.to_vec())),
        (5, Value::Integer(1)),
        (6, Value::Bytes(enrollment_nonce.to_vec())),
        (7, Value::Bytes(claim_hash.to_vec())),
    ]);
    let mut core = vec![
        Value::Integer(1),
        Value::Bytes(family.to_vec()),
        Value::Bytes(ledger.relay_id().to_vec()),
        Value::Bytes(ledger.head().to_vec()),
        Value::Bytes(transition.to_vec()),
        Value::Integer(4),
        delta,
        Value::Bytes(
            crypto::hash("auth-state", &cbor::encode(&state).unwrap())
                .unwrap()
                .to_vec(),
        ),
        Value::Integer(ledger.current_epoch().unwrap().into()),
    ];
    let core_hash = crypto::hash(
        "transition-core",
        &cbor::encode(&Value::Array(core.clone())).unwrap(),
    )
    .unwrap();
    core.push(Value::Array(vec![]));
    core.push(Value::Bytes(core_hash.to_vec()));
    let unsigned = Value::Map(
        core.into_iter()
            .enumerate()
            .map(|(index, value)| (index as u64 + 1, value))
            .collect(),
    );
    let unsigned_bytes = cbor::encode(&unsigned).unwrap();
    let mut signers = vec![(invitation, [0x77; 32]), (recipient, recipient_seed)];
    signers.sort_by_key(|entry| entry.0);
    let signatures = Value::Array(
        signers
            .into_iter()
            .map(|(id, seed)| {
                Value::Array(vec![
                    Value::Bytes(id.to_vec()),
                    Value::Bytes(
                        crypto::sign_cbor("control-transition", &unsigned_bytes, &seed)
                            .unwrap()
                            .to_vec(),
                    ),
                ])
            })
            .collect(),
    );
    (
        cbor::encode(&Value::Map(vec![(1, unsigned), (2, signatures)])).unwrap(),
        recipient,
        recipient_seed,
    )
}
fn later_issue_candidate(
    ledger: &public_ledger::PublicLedger,
    first_issue: &Json,
    family: [u8; 16],
    manager: [u8; 16],
    manager_seed: [u8; 32],
    epoch_key: [u8; 32],
) -> (Vec<u8>, Vec<u8>, [u8; 16], [u8; 16]) {
    let unsigned = cbor::decode(&hex(first_issue["unsigned_cbor_hex"].as_str().unwrap())).unwrap();
    let signatures =
        cbor::decode(&hex(first_issue["signatures_cbor_hex"].as_str().unwrap())).unwrap();
    let prior = cbor::encode(&Value::Map(vec![(1, unsigned), (2, signatures)])).unwrap();
    let ids = control_birth_ids(&prior).unwrap();
    let mut transition = ids[0];
    transition[15] ^= 0x81;
    let mut object_id = ids[1];
    object_id[15] ^= 0x82;
    let mut invitation = ids[2];
    invitation[15] ^= 0x83;
    let invite_public = crypto::signing_public_key(&[0x77; 32]);
    let delta = Value::Map(vec![
        (1, Value::Bytes(invitation.to_vec())),
        (2, Value::Bytes(manager.to_vec())),
        (3, Value::Bytes(invite_public.to_vec())),
        (4, Value::Integer(1)),
    ]);
    let mut next = ledger.state().clone();
    let Value::Map(fields) = &mut next else {
        panic!()
    };
    let Value::Array(invitations) = &mut fields[6].1 else {
        panic!()
    };
    invitations.push(Value::Array(vec![
        Value::Bytes(invitation.to_vec()),
        Value::Bytes(manager.to_vec()),
        Value::Bytes(invite_public.to_vec()),
        Value::Integer(1),
        Value::Bytes(transition.to_vec()),
        Value::Integer(1),
    ]));
    invitations.sort_by_key(|row| match row {
        Value::Array(parts) => match &parts[0] {
            Value::Bytes(id) => id.clone(),
            _ => panic!(),
        },
        _ => panic!(),
    });
    let state_hash = crypto::hash("auth-state", &cbor::encode(&next).unwrap()).unwrap();
    let core = Value::Array(vec![
        Value::Integer(1),
        Value::Bytes(family.to_vec()),
        Value::Bytes(ledger.relay_id().to_vec()),
        Value::Bytes(ledger.head().to_vec()),
        Value::Bytes(transition.to_vec()),
        Value::Integer(2),
        delta.clone(),
        Value::Bytes(state_hash.to_vec()),
        Value::Integer(ledger.current_epoch().unwrap().into()),
    ]);
    let core_hash = crypto::hash("transition-core", &cbor::encode(&core).unwrap()).unwrap();
    let membership = cbor::encode(&Value::Map(vec![
        (1, Value::Bytes(transition.to_vec())),
        (2, Value::Bytes(ledger.head().to_vec())),
        (3, Value::Bytes(state_hash.to_vec())),
        (4, Value::Integer(ledger.current_epoch().unwrap().into())),
        (5, delta),
    ]))
    .unwrap();
    let nonce = [0x44; 24];
    let aad = crypto::hash("membership-aad", &core_hash).unwrap();
    let ciphertext = crypto::seal_with_nonce(&epoch_key, &nonce, &aad, &membership).unwrap();
    let object = cbor::encode(&Value::Map(vec![
        (1, Value::Integer(1)),
        (2, Value::Bytes(nonce.to_vec())),
        (3, Value::Bytes(ciphertext)),
    ]))
    .unwrap();
    let Value::Array(core_fields) = core else {
        panic!()
    };
    let unsigned = Value::Map(
        core_fields
            .into_iter()
            .enumerate()
            .map(|(index, value)| (index as u64 + 1, value))
            .chain([
                (
                    10,
                    Value::Array(vec![Value::Array(vec![
                        Value::Integer(1),
                        Value::Bytes(object_id.to_vec()),
                        Value::Bytes(crypto::hash("object", &object).unwrap().to_vec()),
                        Value::Integer(object.len() as i128),
                    ])]),
                ),
                (11, Value::Bytes(core_hash.to_vec())),
            ])
            .collect(),
    );
    let signature = crypto::sign_cbor(
        "control-transition",
        &cbor::encode(&unsigned).unwrap(),
        &manager_seed,
    )
    .unwrap();
    let signatures = Value::Array(vec![Value::Array(vec![
        Value::Bytes(manager.to_vec()),
        Value::Bytes(signature.to_vec()),
    ])]);
    let candidate = cbor::encode(&Value::Map(vec![
        (1, unsigned.clone()),
        (2, signatures.clone()),
    ]))
    .unwrap();
    let stage = cbor::encode(&Value::Map(vec![
        (1, Value::Integer(1)),
        (2, unsigned),
        (3, signatures),
        (4, Value::Integer(1)),
        (5, Value::Bytes(object_id.to_vec())),
        (6, Value::Bytes(object)),
    ]))
    .unwrap();
    (candidate, stage, invitation, object_id)
}
fn batch_rejection_reason(response: &[u8]) -> Option<u64> {
    let Value::Map(fields) = cbor::decode(response).unwrap() else {
        panic!()
    };
    let Value::Bytes(receipt_bytes) = &fields[1].1 else {
        panic!()
    };
    let Value::Map(receipt) = cbor::decode(receipt_bytes).unwrap() else {
        panic!()
    };
    let Value::Map(body) = &receipt[0].1 else {
        panic!()
    };
    match body[9].1 {
        Value::Integer(reason) => Some(reason.try_into().unwrap()),
        Value::Null => None,
        _ => panic!(),
    }
}
fn signed_get(
    family: [u8; 16],
    relay: [u8; 32],
    signer: [u8; 16],
    seed: [u8; 32],
    path: &str,
    request_id: [u8; 16],
) -> Vec<u8> {
    let request = cbor::encode(&Value::Array(vec![
        Value::Integer(1),
        Value::Bytes(family.to_vec()),
        Value::Bytes(relay.to_vec()),
        Value::Bytes(signer.to_vec()),
        Value::Bytes(request_id.to_vec()),
        Value::Text("GET".into()),
        Value::Text(path.into()),
        Value::Bytes(crypto::hash("request-body", &[]).unwrap().to_vec()),
    ]))
    .unwrap();
    let signature = crypto::sign_cbor("read-request", &request, &seed).unwrap();
    cbor::encode(&Value::Map(vec![
        (1, Value::Bytes(request)),
        (2, Value::Bytes(signature.to_vec())),
    ]))
    .unwrap()
}

fn competing_issue(
    candidate_bytes: &[u8],
    stage_bytes: &[u8],
    genesis: &authority::GenesisCandidate,
    manager_seed: [u8; 32],
) -> (Vec<u8>, Vec<u8>) {
    let Value::Map(mut root) = cbor::decode(candidate_bytes).unwrap() else {
        panic!()
    };
    let Value::Map(mut unsigned) = root[0].1.clone() else {
        panic!()
    };
    let Value::Map(delta) = unsigned[6].1.clone() else {
        panic!()
    };
    let transition = [0xa7; 16];
    unsigned[4].1 = Value::Bytes(transition.to_vec());
    let invitation = Value::Array(vec![
        delta[0].1.clone(),
        delta[1].1.clone(),
        delta[2].1.clone(),
        delta[3].1.clone(),
        Value::Bytes(transition.to_vec()),
        Value::Integer(1),
    ]);
    let state = Value::Map(vec![
        (1, Value::Integer(1)),
        (2, Value::Bytes(genesis.family_id.to_vec())),
        (3, Value::Bytes(genesis.relay_id.to_vec())),
        (4, Value::Integer(1)),
        (5, Value::Array(vec![genesis.manager_row.clone()])),
        (6, Value::Array(vec![])),
        (7, Value::Array(vec![invitation])),
    ]);
    unsigned[7].1 = Value::Bytes(
        crypto::hash("auth-state", &cbor::encode(&state).unwrap())
            .unwrap()
            .to_vec(),
    );
    let core = Value::Array(
        unsigned[..9]
            .iter()
            .map(|(_, value)| value.clone())
            .collect(),
    );
    unsigned[10].1 = Value::Bytes(
        crypto::hash("transition-core", &cbor::encode(&core).unwrap())
            .unwrap()
            .to_vec(),
    );
    let unsigned_bytes = cbor::encode(&Value::Map(unsigned.clone())).unwrap();
    let signature =
        crypto::sign_cbor("control-transition", &unsigned_bytes, &manager_seed).unwrap();
    root[0].1 = Value::Map(unsigned);
    root[1].1 = Value::Array(vec![Value::Array(vec![
        Value::Bytes(genesis.manager_id.to_vec()),
        Value::Bytes(signature.to_vec()),
    ])]);
    let Value::Map(mut stage) = cbor::decode(stage_bytes).unwrap() else {
        panic!()
    };
    stage[1].1 = root[0].1.clone();
    stage[2].1 = root[1].1.clone();
    (
        cbor::encode(&Value::Map(root)).unwrap(),
        cbor::encode(&Value::Map(stage)).unwrap(),
    )
}

fn issue_with_object_id(
    candidate_bytes: &[u8],
    stage_bytes: &[u8],
    object_id: [u8; 16],
    manager_id: [u8; 16],
    manager_seed: [u8; 32],
) -> (Vec<u8>, Vec<u8>) {
    let Value::Map(mut candidate) = cbor::decode(candidate_bytes).unwrap() else {
        panic!()
    };
    let Value::Map(mut unsigned) = candidate[0].1.clone() else {
        panic!()
    };
    let Value::Array(mut manifest) = unsigned[9].1.clone() else {
        panic!()
    };
    let Value::Array(mut entry) = manifest[0].clone() else {
        panic!()
    };
    entry[1] = Value::Bytes(object_id.to_vec());
    manifest[0] = Value::Array(entry);
    unsigned[9].1 = Value::Array(manifest);
    let signature = crypto::sign_cbor(
        "control-transition",
        &cbor::encode(&Value::Map(unsigned.clone())).unwrap(),
        &manager_seed,
    )
    .unwrap();
    candidate[0].1 = Value::Map(unsigned);
    candidate[1].1 = Value::Array(vec![Value::Array(vec![
        Value::Bytes(manager_id.to_vec()),
        Value::Bytes(signature.to_vec()),
    ])]);
    let Value::Map(mut stage) = cbor::decode(stage_bytes).unwrap() else {
        panic!()
    };
    stage[1].1 = candidate[0].1.clone();
    stage[2].1 = candidate[1].1.clone();
    stage[4].1 = Value::Bytes(object_id.to_vec());
    (
        cbor::encode(&Value::Map(candidate)).unwrap(),
        cbor::encode(&Value::Map(stage)).unwrap(),
    )
}

fn issue_stage_with_changed_object(
    stage_bytes: &[u8],
    manager_id: [u8; 16],
    manager_seed: [u8; 32],
) -> Vec<u8> {
    let Value::Map(mut stage) = cbor::decode(stage_bytes).unwrap() else {
        panic!()
    };
    let Value::Bytes(mut object) = stage[5].1.clone() else {
        panic!()
    };
    object.push(0);
    let Value::Map(mut unsigned) = stage[1].1.clone() else {
        panic!()
    };
    let Value::Array(mut manifest) = unsigned[9].1.clone() else {
        panic!()
    };
    let Value::Array(mut entry) = manifest[0].clone() else {
        panic!()
    };
    entry[2] = Value::Bytes(crypto::hash("object", &object).unwrap().to_vec());
    entry[3] = Value::Integer(object.len() as i128);
    manifest[0] = Value::Array(entry);
    unsigned[9].1 = Value::Array(manifest);
    let signature = crypto::sign_cbor(
        "control-transition",
        &cbor::encode(&Value::Map(unsigned.clone())).unwrap(),
        &manager_seed,
    )
    .unwrap();
    stage[1].1 = Value::Map(unsigned);
    stage[2].1 = Value::Array(vec![Value::Array(vec![
        Value::Bytes(manager_id.to_vec()),
        Value::Bytes(signature.to_vec()),
    ])]);
    stage[5].1 = Value::Bytes(object);
    cbor::encode(&Value::Map(stage)).unwrap()
}

fn validly_resigned_genesis_with_changed_manifest(
    candidate_bytes: &[u8],
    manager_id: [u8; 16],
    manager_seed: [u8; 32],
) -> Vec<u8> {
    let Value::Map(mut root) = cbor::decode(candidate_bytes).unwrap() else {
        panic!()
    };
    let Value::Map(mut unsigned) = root[0].1.clone() else {
        panic!()
    };
    let Value::Array(mut manifest) = unsigned[9].1.clone() else {
        panic!()
    };
    let Value::Array(mut entry) = manifest[0].clone() else {
        panic!()
    };
    entry[2] = Value::Bytes(vec![0; 32]);
    manifest[0] = Value::Array(entry);
    unsigned[9].1 = Value::Array(manifest);
    let Value::Map(mut delta) = unsigned[6].1.clone() else {
        panic!()
    };
    delta[2].1 = Value::Bytes(vec![0; 32]);
    unsigned[6].1 = Value::Map(delta);
    let core = Value::Array(
        unsigned[..9]
            .iter()
            .map(|(_, value)| value.clone())
            .collect(),
    );
    unsigned[10].1 = Value::Bytes(
        crypto::hash("transition-core", &cbor::encode(&core).unwrap())
            .unwrap()
            .to_vec(),
    );
    let unsigned_bytes = cbor::encode(&Value::Map(unsigned.clone())).unwrap();
    let signature =
        crypto::sign_cbor("control-transition", &unsigned_bytes, &manager_seed).unwrap();
    root[0].1 = Value::Map(unsigned);
    root[1].1 = Value::Array(vec![Value::Array(vec![
        Value::Bytes(manager_id.to_vec()),
        Value::Bytes(signature.to_vec()),
    ])]);
    cbor::encode(&Value::Map(root)).unwrap()
}

fn signed_manager_batch(
    genesis_committed: &[u8],
    genesis: &authority::GenesisCandidate,
    manager_seed: [u8; 32],
) -> Vec<u8> {
    let mut batch_id = [0x91; 16];
    batch_id[6] = 0x40;
    batch_id[8] = 0x80;
    let ciphertext = vec![0x42; 17];
    let header = Value::Map(vec![
        (1, Value::Array(vec![Value::Integer(1), Value::Integer(0)])),
        (2, Value::Bytes(genesis.family_id.to_vec())),
        (3, Value::Bytes(genesis.relay_id.to_vec())),
        (
            4,
            Value::Bytes(
                crypto::hash("control-head", genesis_committed)
                    .unwrap()
                    .to_vec(),
            ),
        ),
        (5, Value::Integer(1)),
        (6, Value::Bytes(batch_id.to_vec())),
        (7, Value::Bytes(genesis.manager_id.to_vec())),
        (8, Value::Integer(1)),
        (9, Value::Bytes(vec![0x55; 24])),
        (10, Value::Integer(1)),
    ]);
    let signed = cbor::encode(&Value::Array(vec![
        header.clone(),
        Value::Bytes(
            crypto::hash("batch-ciphertext", &ciphertext)
                .unwrap()
                .to_vec(),
        ),
    ]))
    .unwrap();
    let signature = crypto::sign_cbor("batch-envelope", &signed, &manager_seed).unwrap();
    cbor::encode(&Value::Map(vec![
        (1, header),
        (2, Value::Bytes(ciphertext)),
        (3, Value::Bytes(signature.to_vec())),
    ]))
    .unwrap()
}

fn resign_batch_author(
    envelope: &[u8],
    author_id: [u8; 16],
    sequence: u64,
    batch_id: Option<[u8; 16]>,
    author_seed: [u8; 32],
) -> Vec<u8> {
    let Value::Map(mut outer) = cbor::decode(envelope).unwrap() else {
        panic!()
    };
    let Value::Map(header) = &mut outer[0].1 else {
        panic!()
    };
    header[6].1 = Value::Bytes(author_id.to_vec());
    header[7].1 = Value::Integer(sequence.into());
    if let Some(batch_id) = batch_id {
        header[5].1 = Value::Bytes(batch_id.to_vec());
    }
    let Value::Bytes(ciphertext) = &outer[1].1 else {
        panic!()
    };
    let signed = cbor::encode(&Value::Array(vec![
        outer[0].1.clone(),
        Value::Bytes(
            crypto::hash("batch-ciphertext", ciphertext)
                .unwrap()
                .to_vec(),
        ),
    ]))
    .unwrap();
    outer[2].1 = Value::Bytes(
        crypto::sign_cbor("batch-envelope", &signed, &author_seed)
            .unwrap()
            .to_vec(),
    );
    cbor::encode(&Value::Map(outer)).unwrap()
}

#[test]
fn control_and_batch_writers_share_one_family_cursor() {
    let genesis: Json = serde_json::from_str(
        &std::fs::read_to_string("../tests/vectors/api-genesis-v1.json").unwrap(),
    )
    .unwrap();
    let issue: Json =
        serde_json::from_str(&std::fs::read_to_string("../tests/vectors/api-v1.json").unwrap())
            .unwrap();
    let chain: Json = serde_json::from_str(
        &std::fs::read_to_string("../tests/vectors/contiguous-chain-v1.json").unwrap(),
    )
    .unwrap();
    let seed: [u8; 32] = hex(chain["test_only_inputs"]["relay_sign_seed_hex"]
        .as_str()
        .unwrap())
    .try_into()
    .unwrap();
    let manager_seed: [u8; 32] = hex(chain["test_only_inputs"]["manager_sign_seed_hex"]
        .as_str()
        .unwrap())
    .try_into()
    .unwrap();
    let family: [u8; 16] = hex(genesis["inputs"]["family_id_hex"].as_str().unwrap())
        .try_into()
        .unwrap();
    let promotion: [u8; 16] = hex(genesis["inputs"]["promotion_id_hex"].as_str().unwrap())
        .try_into()
        .unwrap();
    let issue_object: [u8; 16] = hex("083e4567e89b42d3a456426614174000").try_into().unwrap();
    let genesis_stage = hex(genesis["inputs"]["stage_body_cbor_hex"].as_str().unwrap());
    let genesis_candidate = hex(genesis["inputs"]["commit_candidate_cbor_hex"]
        .as_str()
        .unwrap());
    let issue_stage = hex(issue["inputs"]["stage_body_cbor_hex"].as_str().unwrap());
    let issue_candidate = hex(issue["inputs"]["commit_body_cbor_hex"].as_str().unwrap());
    let genesis_commit = hex(genesis["expect"]["commit_response_cbor_hex"]
        .as_str()
        .unwrap());
    let Value::Map(response) = cbor::decode(&genesis_commit).unwrap() else {
        panic!()
    };
    let Value::Bytes(genesis_committed) = &response[1].1 else {
        panic!()
    };
    let time = control_commit_time(genesis_committed).unwrap();
    let parsed =
        authority::verify_genesis_candidate(&genesis_candidate, &crypto::signing_public_key(&seed))
            .unwrap();
    let batch = signed_manager_batch(genesis_committed, &parsed, manager_seed);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("relay.sqlite");
    let mut setup = RelayStore::open(&path, seed).unwrap();
    setup
        .stage_genesis_object(family, promotion, &genesis_stage)
        .unwrap();
    setup
        .commit_genesis(family, &genesis_candidate, time)
        .unwrap();
    setup
        .stage_first_issue_object(family, issue_object, &issue_stage)
        .unwrap();
    drop(setup);
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
    let writers = [true, false].map(|control| {
        let path = path.clone();
        let barrier = barrier.clone();
        let issue_candidate = issue_candidate.clone();
        let batch = batch.clone();
        std::thread::spawn(move || {
            let mut store = RelayStore::open(&path, seed).unwrap();
            barrier.wait();
            for _ in 0..100 {
                let result = if control {
                    store.commit_first_issue(family, &issue_candidate, time)
                } else {
                    store.commit_batch(family, &batch)
                };
                if let Ok(result) = result {
                    return result;
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            panic!("concurrent writer did not resolve");
        })
    });
    barrier.wait();
    for writer in writers {
        assert!(!writer.join().unwrap().is_empty());
    }
    let db = Connection::open(&path).unwrap();
    let (cursor, controls, batches, head): (i64, i64, i64, Vec<u8>) = db.query_row(
            "SELECT f.cursor, (SELECT COUNT(*) FROM entries WHERE family_id=f.family_id AND kind=1), (SELECT COUNT(*) FROM entries WHERE family_id=f.family_id AND kind=2), f.head_hash FROM families f WHERE f.family_id=?1",
            params![&family[..]],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        ).unwrap();
    assert_eq!((cursor, controls, batches), (3, 2, 1));
    drop(db);
    assert!(RelayStore::open(&path, seed).is_ok());
    let db = Connection::open(&path).unwrap();
    let (batch_cursor, original_envelope): (i64, Vec<u8>) = db
        .query_row(
            "SELECT cursor,envelope_bytes FROM batch_results WHERE family_id=?1",
            params![&family[..]],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    let saved_receipt: Vec<u8> = db
        .query_row(
            "SELECT receipt_bytes FROM batch_results WHERE family_id=?1",
            params![&family[..]],
            |row| row.get(0),
        )
        .unwrap();
    let recipient_id: [u8; 16] = hex(chain["test_only_inputs"]["recipient_device_id_hex"]
        .as_str()
        .unwrap())
    .try_into()
    .unwrap();
    let recipient_seed: [u8; 32] = hex(chain["test_only_inputs"]["recipient_sign_seed_hex"]
        .as_str()
        .unwrap())
    .try_into()
    .unwrap();
    let forged = resign_batch_author(&original_envelope, recipient_id, 1, None, recipient_seed);
    let forged_batch = batch_authority::verify(
        &forged,
        family,
        parsed.relay_id,
        crypto::signing_public_key(&recipient_seed),
    )
    .unwrap();
    let forged_receipt =
        receipt::accepted_batch(&forged_batch, batch_cursor as u64, &seed).unwrap();
    db.execute(
        "UPDATE entries SET committed_bytes=?3 WHERE family_id=?1 AND cursor=?2",
        params![&family[..], batch_cursor, &forged],
    )
    .unwrap();
    db.execute(
            "UPDATE batch_results SET envelope_bytes=?2,receipt_bytes=?3,author_id=?4 WHERE family_id=?1",
            params![&family[..], &forged, &forged_receipt, &recipient_id[..]],
        )
        .unwrap();
    assert!(RelayStore::open(&path, seed).is_err());
    db.execute(
        "UPDATE entries SET committed_bytes=?3 WHERE family_id=?1 AND cursor=?2",
        params![&family[..], batch_cursor, &original_envelope],
    )
    .unwrap();
    db.execute(
            "UPDATE batch_results SET envelope_bytes=?2,receipt_bytes=?3,author_id=?4 WHERE family_id=?1",
            params![&family[..], &original_envelope, &saved_receipt, &parsed.manager_id[..]],
        )
        .unwrap();
    assert!(RelayStore::open(&path, seed).is_ok());
    let skipped = resign_batch_author(&original_envelope, parsed.manager_id, 2, None, manager_seed);
    let skipped_batch = batch_authority::verify(
        &skipped,
        family,
        parsed.relay_id,
        parsed.manager_signing_key,
    )
    .unwrap();
    let skipped_receipt =
        receipt::accepted_batch(&skipped_batch, batch_cursor as u64, &seed).unwrap();
    db.execute(
        "UPDATE entries SET committed_bytes=?3 WHERE family_id=?1 AND cursor=?2",
        params![&family[..], batch_cursor, &skipped],
    )
    .unwrap();
    db.execute(
        "UPDATE batch_results SET envelope_bytes=?2,receipt_bytes=?3,sequence=2 WHERE family_id=?1",
        params![&family[..], &skipped, &skipped_receipt],
    )
    .unwrap();
    assert!(RelayStore::open(&path, seed).is_err());
    db.execute(
        "UPDATE entries SET committed_bytes=?3 WHERE family_id=?1 AND cursor=?2",
        params![&family[..], batch_cursor, &original_envelope],
    )
    .unwrap();
    db.execute(
        "UPDATE batch_results SET envelope_bytes=?2,receipt_bytes=?3,sequence=1 WHERE family_id=?1",
        params![&family[..], &original_envelope, &saved_receipt],
    )
    .unwrap();
    assert!(RelayStore::open(&path, seed).is_ok());
    let (mut rejected_id, _) = batch_authority::claimed_identity(&original_envelope).unwrap();
    rejected_id[15] ^= 1;
    let rejected_envelope = resign_batch_author(
        &original_envelope,
        parsed.manager_id,
        1,
        Some(rejected_id),
        manager_seed,
    );
    let mut writer = RelayStore::open(&path, seed).unwrap();
    let rejected_response = writer.commit_batch(family, &rejected_envelope).unwrap();
    drop(writer);
    let Value::Map(rejected_fields) = cbor::decode(&rejected_response).unwrap() else {
        panic!()
    };
    let Value::Bytes(rejected_receipt) = &rejected_fields[1].1 else {
        panic!()
    };
    assert!(RelayStore::open(&path, seed).is_ok());
    let mut changed_rejection = rejected_receipt.clone();
    *changed_rejection.last_mut().unwrap() ^= 1;
    db.execute(
        "UPDATE rejected_batch_results SET receipt_bytes=?3 WHERE family_id=?1 AND batch_id=?2",
        params![&family[..], &rejected_id[..], &changed_rejection],
    )
    .unwrap();
    assert!(RelayStore::open(&path, seed).is_err());
    db.execute(
        "UPDATE rejected_batch_results SET receipt_bytes=?3 WHERE family_id=?1 AND batch_id=?2",
        params![&family[..], &rejected_id[..], rejected_receipt],
    )
    .unwrap();
    assert!(RelayStore::open(&path, seed).is_ok());
    db.execute(
        "DELETE FROM rejected_batch_results WHERE family_id=?1 AND batch_id=?2",
        params![&family[..], &rejected_id[..]],
    )
    .unwrap();
    assert!(RelayStore::open(&path, seed).is_err());
    db.execute(
            "INSERT INTO rejected_batch_results(family_id,batch_id,envelope_bytes,receipt_bytes) VALUES(?1,?2,?3,?4)",
            params![&family[..], &rejected_id[..], &rejected_envelope, rejected_receipt],
        )
        .unwrap();
    assert!(RelayStore::open(&path, seed).is_ok());
    let mut changed_receipt = saved_receipt.clone();
    *changed_receipt.last_mut().unwrap() ^= 1;
    db.execute(
        "UPDATE batch_results SET receipt_bytes=?2 WHERE family_id=?1",
        params![&family[..], &changed_receipt],
    )
    .unwrap();
    assert!(RelayStore::open(&path, seed).is_err());
    db.execute(
        "UPDATE batch_results SET receipt_bytes=?2 WHERE family_id=?1",
        params![&family[..], &saved_receipt],
    )
    .unwrap();
    db.execute(
        "UPDATE families SET head_hash=?2 WHERE family_id=?1",
        params![&family[..], &[0u8; 32][..]],
    )
    .unwrap();
    drop(db);
    assert!(RelayStore::open(&path, seed).is_err());
    let db = Connection::open(&path).unwrap();
    db.execute(
        "UPDATE families SET head_hash=?2 WHERE family_id=?1",
        params![&family[..], &head],
    )
    .unwrap();
    let issue_bytes: Vec<u8> = db
            .query_row(
                "SELECT committed_bytes FROM entries WHERE family_id=?1 AND kind=1 ORDER BY cursor LIMIT 1 OFFSET 1",
                params![&family[..]],
                |row| row.get(0),
            )
            .unwrap();
    let mut changed = issue_bytes;
    *changed.last_mut().unwrap() ^= 1;
    db.execute(
        "UPDATE entries SET committed_bytes=?2 WHERE family_id=?1 AND kind=1 AND cursor>1",
        params![&family[..], &changed],
    )
    .unwrap();
    drop(db);
    assert!(RelayStore::open(&path, seed).is_err());
}

#[test]
fn competing_staged_issue_does_not_block_valid_issue_after_restart() {
    let genesis: Json = serde_json::from_str(
        &std::fs::read_to_string("../tests/vectors/api-genesis-v1.json").unwrap(),
    )
    .unwrap();
    let issue: Json =
        serde_json::from_str(&std::fs::read_to_string("../tests/vectors/api-v1.json").unwrap())
            .unwrap();
    let chain: Json = serde_json::from_str(
        &std::fs::read_to_string("../tests/vectors/contiguous-chain-v1.json").unwrap(),
    )
    .unwrap();
    let seed: [u8; 32] = hex(chain["test_only_inputs"]["relay_sign_seed_hex"]
        .as_str()
        .unwrap())
    .try_into()
    .unwrap();
    let manager_seed: [u8; 32] = hex(chain["test_only_inputs"]["manager_sign_seed_hex"]
        .as_str()
        .unwrap())
    .try_into()
    .unwrap();
    let family: [u8; 16] = hex(genesis["inputs"]["family_id_hex"].as_str().unwrap())
        .try_into()
        .unwrap();
    let promotion: [u8; 16] = hex(genesis["inputs"]["promotion_id_hex"].as_str().unwrap())
        .try_into()
        .unwrap();
    let issue_object: [u8; 16] = hex("083e4567e89b42d3a456426614174000").try_into().unwrap();
    let genesis_stage = hex(genesis["inputs"]["stage_body_cbor_hex"].as_str().unwrap());
    let genesis_candidate = hex(genesis["inputs"]["commit_candidate_cbor_hex"]
        .as_str()
        .unwrap());
    let issue_stage = hex(issue["inputs"]["stage_body_cbor_hex"].as_str().unwrap());
    let issue_candidate = hex(issue["inputs"]["commit_body_cbor_hex"].as_str().unwrap());
    let parsed =
        authority::verify_genesis_candidate(&genesis_candidate, &crypto::signing_public_key(&seed))
            .unwrap();
    let (competing_candidate, competing_stage) =
        competing_issue(&issue_candidate, &issue_stage, &parsed, manager_seed);
    let genesis_commit = hex(genesis["expect"]["commit_response_cbor_hex"]
        .as_str()
        .unwrap());
    let Value::Map(genesis_response) = cbor::decode(&genesis_commit).unwrap() else {
        panic!()
    };
    let Value::Bytes(genesis_committed) = &genesis_response[1].1 else {
        panic!()
    };
    let time = control_commit_time(genesis_committed).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("relay.sqlite");
    let mut store = RelayStore::open(&path, seed).unwrap();
    store
        .stage_genesis_object(family, promotion, &genesis_stage)
        .unwrap();
    store
        .commit_genesis(family, &genesis_candidate, time)
        .unwrap();
    let head = crypto::hash("control-head", genesis_committed).unwrap();
    authority::verify_first_invite_issue(&competing_candidate, &parsed, head).unwrap();
    store
        .stage_first_issue_object(family, issue_object, &competing_stage)
        .unwrap();
    // Simulate a database interrupted under the original single-slot
    // schema, then let open() migrate that staged candidate.
    store.db.execute(
            "INSERT INTO staged_issues SELECT family_id,transition_id,candidate_bytes FROM staged_controls WHERE family_id=?1",
            params![&family[..]],
        ).unwrap();
    store.db.execute(
            "INSERT INTO staged_objects SELECT family_id,object_id,kind,object_hash,object_bytes FROM staged_control_objects WHERE family_id=?1",
            params![&family[..]],
        ).unwrap();
    store
        .db
        .execute(
            "DELETE FROM staged_controls WHERE family_id=?1",
            params![&family[..]],
        )
        .unwrap();
    store
        .db
        .execute(
            "DELETE FROM staged_control_objects WHERE family_id=?1",
            params![&family[..]],
        )
        .unwrap();
    drop(store);
    let mut store = RelayStore::open(&path, seed).unwrap();
    store
        .stage_first_issue_object(family, issue_object, &issue_stage)
        .unwrap();
    assert_eq!(
        store
            .db
            .query_row(
                "SELECT COUNT(*) FROM staged_controls WHERE family_id=?1",
                params![&family[..]],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
        2
    );
    store
        .commit_first_issue(family, &issue_candidate, time)
        .unwrap();
    assert!(
        store
            .commit_first_issue(family, &competing_candidate, time)
            .is_err()
    );
    assert_eq!(
        store
            .db
            .query_row(
                "SELECT COUNT(*) FROM staged_controls WHERE family_id=?1",
                params![&family[..]],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
        0
    );
    drop(store);
    let mut store = RelayStore::open(&path, seed).unwrap();
    assert!(
        store
            .stage_first_issue_object(family, issue_object, &competing_stage)
            .is_err()
    );
    assert!(
        store
            .commit_first_issue(family, &competing_candidate, time)
            .is_err()
    );
}

#[test]
fn public_batch_id_cannot_poison_later_object_staging() {
    let genesis: Json = serde_json::from_str(
        &std::fs::read_to_string("../tests/vectors/api-genesis-v1.json").unwrap(),
    )
    .unwrap();
    let issue: Json =
        serde_json::from_str(&std::fs::read_to_string("../tests/vectors/api-v1.json").unwrap())
            .unwrap();
    let chain: Json = serde_json::from_str(
        &std::fs::read_to_string("../tests/vectors/contiguous-chain-v1.json").unwrap(),
    )
    .unwrap();
    let seed: [u8; 32] = hex(chain["test_only_inputs"]["relay_sign_seed_hex"]
        .as_str()
        .unwrap())
    .try_into()
    .unwrap();
    let manager_seed: [u8; 32] = hex(chain["test_only_inputs"]["manager_sign_seed_hex"]
        .as_str()
        .unwrap())
    .try_into()
    .unwrap();
    let family: [u8; 16] = hex(genesis["inputs"]["family_id_hex"].as_str().unwrap())
        .try_into()
        .unwrap();
    let promotion: [u8; 16] = hex(genesis["inputs"]["promotion_id_hex"].as_str().unwrap())
        .try_into()
        .unwrap();
    let colliding_id: [u8; 16] = hex("083e4567e89b42d3a456426614174000").try_into().unwrap();
    let distinct_id: [u8; 16] = hex("183e4567e89b42d3a456426614174000").try_into().unwrap();
    let genesis_stage = hex(genesis["inputs"]["stage_body_cbor_hex"].as_str().unwrap());
    let genesis_candidate = hex(genesis["inputs"]["commit_candidate_cbor_hex"]
        .as_str()
        .unwrap());
    let issue_stage = hex(issue["inputs"]["stage_body_cbor_hex"].as_str().unwrap());
    let issue_candidate = hex(issue["inputs"]["commit_body_cbor_hex"].as_str().unwrap());
    let parsed =
        authority::verify_genesis_candidate(&genesis_candidate, &crypto::signing_public_key(&seed))
            .unwrap();
    let (valid_issue, valid_stage) = issue_with_object_id(
        &issue_candidate,
        &issue_stage,
        distinct_id,
        parsed.manager_id,
        manager_seed,
    );
    let genesis_response = hex(genesis["expect"]["commit_response_cbor_hex"]
        .as_str()
        .unwrap());
    let Value::Map(response) = cbor::decode(&genesis_response).unwrap() else {
        panic!()
    };
    let Value::Bytes(genesis_committed) = &response[1].1 else {
        panic!()
    };
    let time = control_commit_time(genesis_committed).unwrap();
    let template = signed_manager_batch(genesis_committed, &parsed, manager_seed);
    for rejected in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("relay.sqlite");
        let mut store = RelayStore::open(&path, seed).unwrap();
        store
            .stage_genesis_object(family, promotion, &genesis_stage)
            .unwrap();
        store
            .commit_genesis(family, &genesis_candidate, time)
            .unwrap();
        if rejected {
            store.commit_batch(family, &template).unwrap();
        }
        let colliding = resign_batch_author(
            &template,
            parsed.manager_id,
            if rejected { 3 } else { 1 },
            Some(colliding_id),
            manager_seed,
        );
        store.commit_batch(family, &colliding).unwrap();
        let table = if rejected {
            "rejected_batch_results"
        } else {
            "batch_results"
        };
        let count: i64 = store
            .db
            .query_row(
                &format!("SELECT COUNT(*) FROM {table} WHERE family_id=?1 AND batch_id=?2"),
                params![&family[..], &colliding_id[..]],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
        assert!(
            store
                .stage_first_issue_object(family, colliding_id, &issue_stage)
                .is_err()
        );
        let reservations: i64 = store
            .db
            .query_row(
                "SELECT COUNT(*) FROM object_reservations WHERE family_id=?1 AND object_id=?2",
                params![&family[..], &colliding_id[..]],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(reservations, 0);
        drop(store);
        let mut store = RelayStore::open(&path, seed).unwrap();
        let mut followup_id = [0xa9; 16];
        followup_id[6] = 0x49;
        followup_id[8] = 0x89;
        let followup = resign_batch_author(
            &template,
            parsed.manager_id,
            2,
            Some(followup_id),
            manager_seed,
        );
        store.commit_batch(family, &followup).unwrap();
        let accepted: i64 = store
            .db
            .query_row(
                "SELECT COUNT(*) FROM batch_results WHERE family_id=?1 AND batch_id=?2",
                params![&family[..], &followup_id[..]],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(accepted, 1);
        store
            .stage_first_issue_object(family, distinct_id, &valid_stage)
            .unwrap();
        store
            .commit_first_issue(family, &valid_issue, time)
            .unwrap();
        assert!(
            store
                .committed_object(family, distinct_id)
                .unwrap()
                .is_some()
        );
    }
}
#[test]
fn staged_object_id_keeps_its_hash_across_competitors_cleanup_and_restart() {
    let genesis: Json = serde_json::from_str(
        &std::fs::read_to_string("../tests/vectors/api-genesis-v1.json").unwrap(),
    )
    .unwrap();
    let issue: Json =
        serde_json::from_str(&std::fs::read_to_string("../tests/vectors/api-v1.json").unwrap())
            .unwrap();
    let chain: Json = serde_json::from_str(
        &std::fs::read_to_string("../tests/vectors/contiguous-chain-v1.json").unwrap(),
    )
    .unwrap();
    let seed: [u8; 32] = hex(chain["test_only_inputs"]["relay_sign_seed_hex"]
        .as_str()
        .unwrap())
    .try_into()
    .unwrap();
    let manager_seed: [u8; 32] = hex(chain["test_only_inputs"]["manager_sign_seed_hex"]
        .as_str()
        .unwrap())
    .try_into()
    .unwrap();
    let family: [u8; 16] = hex(genesis["inputs"]["family_id_hex"].as_str().unwrap())
        .try_into()
        .unwrap();
    let promotion: [u8; 16] = hex(genesis["inputs"]["promotion_id_hex"].as_str().unwrap())
        .try_into()
        .unwrap();
    let object_id: [u8; 16] = hex("083e4567e89b42d3a456426614174000").try_into().unwrap();
    let genesis_stage = hex(genesis["inputs"]["stage_body_cbor_hex"].as_str().unwrap());
    let genesis_candidate = hex(genesis["inputs"]["commit_candidate_cbor_hex"]
        .as_str()
        .unwrap());
    let issue_stage = hex(issue["inputs"]["stage_body_cbor_hex"].as_str().unwrap());
    let issue_candidate = hex(issue["inputs"]["commit_body_cbor_hex"].as_str().unwrap());
    let parsed =
        authority::verify_genesis_candidate(&genesis_candidate, &crypto::signing_public_key(&seed))
            .unwrap();
    let (_, competitor) = competing_issue(&issue_candidate, &issue_stage, &parsed, manager_seed);
    let changed = issue_stage_with_changed_object(&competitor, parsed.manager_id, manager_seed);
    let genesis_commit = hex(genesis["expect"]["commit_response_cbor_hex"]
        .as_str()
        .unwrap());
    let Value::Map(response) = cbor::decode(&genesis_commit).unwrap() else {
        panic!()
    };
    let Value::Bytes(committed) = &response[1].1 else {
        panic!()
    };
    let time = control_commit_time(committed).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("reservations.sqlite");
    let mut store = RelayStore::open(&path, seed).unwrap();
    store
        .stage_genesis_object(family, promotion, &genesis_stage)
        .unwrap();
    store
        .commit_genesis(family, &genesis_candidate, time)
        .unwrap();
    store
        .stage_first_issue_object(family, object_id, &issue_stage)
        .unwrap();
    let colliding_batch = resign_batch_author(
        &signed_manager_batch(committed, &parsed, manager_seed),
        parsed.manager_id,
        1,
        Some(object_id),
        manager_seed,
    );
    assert!(store.commit_batch(family, &colliding_batch).is_err());
    assert!(ensure_new_protocol_ids(&store.db, family, &[object_id], &[]).is_err());
    assert!(
        store
            .stage_first_issue_object(family, object_id, &changed)
            .is_err()
    );
    store
        .db
        .execute(
            "DELETE FROM staged_control_objects WHERE family_id=?1",
            params![&family[..]],
        )
        .unwrap();
    store
        .db
        .execute(
            "DELETE FROM staged_controls WHERE family_id=?1",
            params![&family[..]],
        )
        .unwrap();
    drop(store);
    let mut store = RelayStore::open(&path, seed).unwrap();
    assert!(store.commit_batch(family, &colliding_batch).is_err());
    assert!(
        store
            .stage_first_issue_object(family, object_id, &changed)
            .is_err()
    );
    store
        .stage_first_issue_object(family, object_id, &issue_stage)
        .unwrap();
    store
        .db
        .execute(
            "DELETE FROM staged_control_objects WHERE family_id=?1",
            params![&family[..]],
        )
        .unwrap();
    store
        .db
        .execute(
            "DELETE FROM staged_controls WHERE family_id=?1",
            params![&family[..]],
        )
        .unwrap();
    store
        .db
        .execute(
            "DELETE FROM object_reservations WHERE family_id=?1 AND object_id=?2",
            params![&family[..], &object_id[..]],
        )
        .unwrap();
    drop(store);
    assert!(RelayStore::open(&path, seed).is_err());
}

#[test]
fn restart_rejects_substituted_genesis_candidate_and_receipt() {
    let genesis: Json = serde_json::from_str(
        &std::fs::read_to_string("../tests/vectors/api-genesis-v1.json").unwrap(),
    )
    .unwrap();
    let issue: Json =
        serde_json::from_str(&std::fs::read_to_string("../tests/vectors/api-v1.json").unwrap())
            .unwrap();
    let chain: Json = serde_json::from_str(
        &std::fs::read_to_string("../tests/vectors/contiguous-chain-v1.json").unwrap(),
    )
    .unwrap();
    let seed: [u8; 32] = hex(chain["test_only_inputs"]["relay_sign_seed_hex"]
        .as_str()
        .unwrap())
    .try_into()
    .unwrap();
    let manager_seed: [u8; 32] = hex(chain["test_only_inputs"]["manager_sign_seed_hex"]
        .as_str()
        .unwrap())
    .try_into()
    .unwrap();
    let manager_id: [u8; 16] = hex(chain["test_only_inputs"]["manager_device_id_hex"]
        .as_str()
        .unwrap())
    .try_into()
    .unwrap();
    let family: [u8; 16] = hex(genesis["inputs"]["family_id_hex"].as_str().unwrap())
        .try_into()
        .unwrap();
    let promotion: [u8; 16] = hex(genesis["inputs"]["promotion_id_hex"].as_str().unwrap())
        .try_into()
        .unwrap();
    let issue_object: [u8; 16] = hex("083e4567e89b42d3a456426614174000").try_into().unwrap();
    let genesis_stage = hex(genesis["inputs"]["stage_body_cbor_hex"].as_str().unwrap());
    let genesis_candidate = hex(genesis["inputs"]["commit_candidate_cbor_hex"]
        .as_str()
        .unwrap());
    let issue_stage = hex(issue["inputs"]["stage_body_cbor_hex"].as_str().unwrap());
    let alternate = validly_resigned_genesis_with_changed_manifest(
        &genesis_candidate,
        manager_id,
        manager_seed,
    );
    authority::verify_genesis_candidate(&alternate, &crypto::signing_public_key(&seed)).unwrap();
    let genesis_commit = hex(genesis["expect"]["commit_response_cbor_hex"]
        .as_str()
        .unwrap());
    let Value::Map(response) = cbor::decode(&genesis_commit).unwrap() else {
        panic!()
    };
    let Value::Bytes(committed) = &response[1].1 else {
        panic!()
    };
    let time = control_commit_time(committed).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("genesis-corruption.sqlite");
    let mut store = RelayStore::open(&path, seed).unwrap();
    store
        .stage_genesis_object(family, promotion, &genesis_stage)
        .unwrap();
    store
        .commit_genesis(family, &genesis_candidate, time)
        .unwrap();
    store
        .db
        .execute(
            "UPDATE families SET candidate_bytes=?2 WHERE family_id=?1",
            params![&family[..], &alternate],
        )
        .unwrap();
    assert!(
        store
            .stage_first_issue_object(family, issue_object, &issue_stage)
            .is_err()
    );
    drop(store);
    assert!(RelayStore::open(&path, seed).is_err());
    let db = Connection::open(&path).unwrap();
    db.execute(
        "UPDATE families SET candidate_bytes=?2 WHERE family_id=?1",
        params![&family[..], &genesis_candidate],
    )
    .unwrap();
    let mut store = RelayStore::open(&path, seed).unwrap();
    let mut changed = committed.clone();
    *changed.last_mut().unwrap() ^= 1;
    store
        .db
        .execute(
            "UPDATE families SET committed_bytes=?2 WHERE family_id=?1",
            params![&family[..], &changed],
        )
        .unwrap();
    assert!(
        store
            .stage_first_issue_object(family, issue_object, &issue_stage)
            .is_err()
    );
    store
        .db
        .execute(
            "UPDATE entries SET committed_bytes=?2 WHERE family_id=?1 AND cursor=1",
            params![&family[..], &changed],
        )
        .unwrap();
    assert!(
        store
            .stage_first_issue_object(family, issue_object, &issue_stage)
            .is_err()
    );
    drop(store);
    assert!(RelayStore::open(&path, seed).is_err());
    let db = Connection::open(&path).unwrap();
    db.execute(
        "UPDATE families SET committed_bytes=?2 WHERE family_id=?1",
        params![&family[..], committed],
    )
    .unwrap();
    db.execute(
        "UPDATE entries SET committed_bytes=?2 WHERE family_id=?1 AND cursor=1",
        params![&family[..], committed],
    )
    .unwrap();
    let object: Vec<u8> = db
        .query_row(
            "SELECT object_bytes FROM committed_objects WHERE family_id=?1 AND object_id=?2",
            params![&family[..], &promotion[..]],
            |row| row.get(0),
        )
        .unwrap();
    let mut changed_object = object;
    *changed_object.last_mut().unwrap() ^= 1;
    db.execute(
        "UPDATE committed_objects SET object_bytes=?3 WHERE family_id=?1 AND object_id=?2",
        params![&family[..], &promotion[..], &changed_object],
    )
    .unwrap();
    drop(db);
    assert!(RelayStore::open(&path, seed).is_err());
}

#[test]
fn genesis_reservation_and_commit_are_atomic_and_survive_restart() {
    let api: Json = serde_json::from_str(
        &std::fs::read_to_string("../tests/vectors/api-genesis-v1.json").unwrap(),
    )
    .unwrap();
    let chain: Json = serde_json::from_str(
        &std::fs::read_to_string("../tests/vectors/contiguous-chain-v1.json").unwrap(),
    )
    .unwrap();
    let seed: [u8; 32] = hex(chain["test_only_inputs"]["relay_sign_seed_hex"]
        .as_str()
        .unwrap())
    .try_into()
    .unwrap();
    let stage = hex(api["inputs"]["stage_body_cbor_hex"].as_str().unwrap());
    let candidate = hex(api["inputs"]["commit_candidate_cbor_hex"].as_str().unwrap());
    let family: [u8; 16] = hex(api["inputs"]["family_id_hex"].as_str().unwrap())
        .try_into()
        .unwrap();
    let object_id: [u8; 16] = hex(api["inputs"]["promotion_id_hex"].as_str().unwrap())
        .try_into()
        .unwrap();
    let promotion_path = api["inputs"]["promotion_result_path"].as_str().unwrap();
    let promotion_auth = hex(api["inputs"]["promotion_result_read_auth_cbor_hex"]
        .as_str()
        .unwrap());
    let expected_stage = hex(api["expect"]["stage_response_cbor_hex"].as_str().unwrap());
    let expected_commit = hex(api["expect"]["commit_response_cbor_hex"].as_str().unwrap());
    let Value::Map(response) = cbor::decode(&expected_commit).unwrap() else {
        panic!()
    };
    let Value::Bytes(committed) = &response[1].1 else {
        panic!()
    };
    let Value::Map(commit_map) = cbor::decode(committed).unwrap() else {
        panic!()
    };
    let Value::Array(receipt) = &commit_map[2].1 else {
        panic!()
    };
    let Value::Integer(time) = receipt[4] else {
        panic!()
    };
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("relay.sqlite");
    let mut store = RelayStore::open(&path, seed).unwrap();
    assert!(
        store
            .commit_genesis(family, &candidate, time.try_into().unwrap())
            .is_err()
    );
    assert_eq!(
        store
            .stage_genesis_object(family, object_id, &stage)
            .unwrap(),
        expected_stage
    );
    drop(store);
    let mut store = RelayStore::open(&path, seed).unwrap();
    assert_eq!(
        store
            .stage_genesis_object(family, object_id, &stage)
            .unwrap(),
        expected_stage
    );
    assert!(store.genesis_result(family).unwrap().is_none());
    assert!(store.committed_object(family, object_id).unwrap().is_none());
    let pending_result = store
        .promotion_result_authenticated(family, object_id, promotion_path, &promotion_auth)
        .unwrap();
    assert_eq!(
        cbor::decode(&pending_result).unwrap(),
        Value::Map(vec![(1, Value::Integer(1)), (2, Value::Null)])
    );
    assert_eq!(
        store
            .commit_genesis(family, &candidate, time.try_into().unwrap())
            .unwrap(),
        expected_commit
    );
    assert_eq!(
        store
            .commit_genesis(family, &candidate, i64::try_from(time).unwrap() + 100)
            .unwrap(),
        expected_commit
    );
    assert_eq!(
        store
            .stage_genesis_object(family, object_id, &stage)
            .unwrap(),
        expected_stage
    );
    assert_eq!(
        store
            .promotion_result_authenticated(family, object_id, promotion_path, &promotion_auth)
            .unwrap(),
        hex(api["expect"]["promotion_result_response_cbor_hex"]
            .as_str()
            .unwrap())
    );
    assert!(
        store
            .promotion_result_authenticated(
                family,
                object_id,
                &format!("{promotion_path}/"),
                &promotion_auth
            )
            .is_err()
    );
    drop(store);
    let store = RelayStore::open(&path, seed).unwrap();
    assert_eq!(store.genesis_result(family).unwrap().unwrap(), *committed);
    assert!(store.committed_object(family, object_id).unwrap().is_some());
    assert!(RelayStore::open(&path, [42; 32]).is_err());
}

#[test]
fn first_issue_stages_membership_and_commits_exact_api_bytes() {
    let genesis: Json = serde_json::from_str(
        &std::fs::read_to_string("../tests/vectors/api-genesis-v1.json").unwrap(),
    )
    .unwrap();
    let issue: Json =
        serde_json::from_str(&std::fs::read_to_string("../tests/vectors/api-v1.json").unwrap())
            .unwrap();
    let chain: Json = serde_json::from_str(
        &std::fs::read_to_string("../tests/vectors/contiguous-chain-v1.json").unwrap(),
    )
    .unwrap();
    let seed: [u8; 32] = hex(chain["test_only_inputs"]["relay_sign_seed_hex"]
        .as_str()
        .unwrap())
    .try_into()
    .unwrap();
    let family: [u8; 16] = hex(genesis["inputs"]["family_id_hex"].as_str().unwrap())
        .try_into()
        .unwrap();
    let promotion: [u8; 16] = hex(genesis["inputs"]["promotion_id_hex"].as_str().unwrap())
        .try_into()
        .unwrap();
    let issue_object: [u8; 16] = hex("083e4567e89b42d3a456426614174000").try_into().unwrap();
    let genesis_stage = hex(genesis["inputs"]["stage_body_cbor_hex"].as_str().unwrap());
    let genesis_candidate = hex(genesis["inputs"]["commit_candidate_cbor_hex"]
        .as_str()
        .unwrap());
    let genesis_committed_response = hex(genesis["expect"]["commit_response_cbor_hex"]
        .as_str()
        .unwrap());
    let Value::Map(fields) = cbor::decode(&genesis_committed_response).unwrap() else {
        panic!()
    };
    let Value::Bytes(genesis_committed) = &fields[1].1 else {
        panic!()
    };
    let genesis_time = control_commit_time(genesis_committed).unwrap();
    let issue_stage = hex(issue["inputs"]["stage_body_cbor_hex"].as_str().unwrap());
    let issue_candidate = hex(issue["inputs"]["commit_body_cbor_hex"].as_str().unwrap());
    let issue_expected = hex(issue["expect"]["commit_response_cbor_hex"]
        .as_str()
        .unwrap());
    let Value::Map(fields) = cbor::decode(&issue_expected).unwrap() else {
        panic!()
    };
    let Value::Bytes(issue_committed) = &fields[1].1 else {
        panic!()
    };
    let issue_time = control_commit_time(issue_committed).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("relay.db");
    let mut store = RelayStore::open(&path, seed).unwrap();
    store
        .stage_genesis_object(family, promotion, &genesis_stage)
        .unwrap();
    store
        .commit_genesis(family, &genesis_candidate, genesis_time)
        .unwrap();
    assert!(
        store
            .commit_first_issue(family, &issue_candidate, issue_time)
            .is_err()
    );
    assert_eq!(
        store
            .stage_first_issue_object(family, issue_object, &issue_stage)
            .unwrap(),
        hex(issue["expect"]["stage_response_cbor_hex"].as_str().unwrap())
    );
    assert!(
        store
            .committed_object(family, issue_object)
            .unwrap()
            .is_none()
    );
    drop(store);
    let mut store = RelayStore::open(&path, seed).unwrap();
    assert_eq!(
        store
            .commit_first_issue(family, &issue_candidate, issue_time)
            .unwrap(),
        issue_expected
    );
    assert_eq!(
        store
            .commit_first_issue(family, &issue_candidate, issue_time + 1)
            .unwrap(),
        issue_expected
    );
    assert!(
        store
            .committed_object(family, issue_object)
            .unwrap()
            .is_some()
    );
    assert_eq!(
        store
            .stage_first_issue_object(family, issue_object, &issue_stage)
            .unwrap(),
        hex(issue["expect"]["stage_response_cbor_hex"].as_str().unwrap())
    );
    let genesis_parsed =
        authority::verify_genesis_candidate(&genesis_candidate, &store.relay_public).unwrap();
    let genesis_head = crypto::hash("control-head", genesis_committed).unwrap();
    let issue_parsed =
        authority::verify_first_invite_issue(&issue_candidate, &genesis_parsed, genesis_head)
            .unwrap();
    assert!(
        ensure_new_protocol_ids(&store.db, family, &[genesis_parsed.transition_id], &[]).is_err()
    );
    assert!(ensure_new_protocol_ids(&store.db, family, &[genesis_parsed.manager_id], &[]).is_err());
    assert!(
        ensure_new_protocol_ids(&store.db, family, &[issue_parsed.invitation_id], &[]).is_err()
    );
    assert!(ensure_new_protocol_ids(&store.db, family, &[issue_object], &[]).is_err());
    assert!(ensure_new_protocol_ids(&store.db, family, &[[0x9a; 16]], &[]).is_ok());
    let invite_seed: [u8; 32] = hex(chain["test_only_inputs"]["invitation_sign_seed_hex"]
        .as_str()
        .unwrap())
    .try_into()
    .unwrap();
    let control_path = format!("/v1/families/{}/control?after=0", lower_hex(&family));
    let invite_control = signed_get(
        family,
        genesis_parsed.relay_id,
        issue_parsed.invitation_id,
        invite_seed,
        &control_path,
        [1; 16],
    );
    assert!(
        store
            .control_page_authenticated(family, 0, &control_path, &invite_control)
            .is_ok()
    );
    let issue_object_path = issue["inputs"]["read_object_path"].as_str().unwrap();
    let invite_object = signed_get(
        family,
        genesis_parsed.relay_id,
        issue_parsed.invitation_id,
        invite_seed,
        issue_object_path,
        [2; 16],
    );
    assert!(
        store
            .object_authenticated(family, issue_object, issue_object_path, &invite_object)
            .is_ok()
    );
    let promotion_path = genesis["inputs"]["promotion_result_path"].as_str().unwrap();
    let invite_promotion = signed_get(
        family,
        genesis_parsed.relay_id,
        issue_parsed.invitation_id,
        invite_seed,
        promotion_path,
        [3; 16],
    );
    assert!(
        store
            .object_authenticated(family, promotion, promotion_path, &invite_promotion)
            .is_err()
    );
    let claim_transition = &chain["transitions"][2];
    let claim_candidate = cbor::encode(&Value::Map(vec![
        (
            1,
            cbor::decode(&hex(claim_transition["unsigned_cbor_hex"]
                .as_str()
                .unwrap()))
            .unwrap(),
        ),
        (
            2,
            cbor::decode(&hex(claim_transition["signatures_cbor_hex"]
                .as_str()
                .unwrap()))
            .unwrap(),
        ),
    ]))
    .unwrap();
    let claim_committed = hex(claim_transition["committed_cbor_hex"].as_str().unwrap());
    let claim_time = control_commit_time(&claim_committed).unwrap();
    let before_claim: (i64, Vec<u8>) = store
        .db
        .query_row(
            "SELECT cursor,head_hash FROM families WHERE family_id=?1",
            params![&family[..]],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    let mut clock_called = false;
    assert!(
        store
            .commit_first_claim_with_clock(family, &claim_candidate, || {
                clock_called = true;
                let other = Connection::open(&path).unwrap();
                other.busy_timeout(std::time::Duration::ZERO).unwrap();
                assert!(
                    other
                        .execute(
                            "UPDATE families SET cursor=cursor WHERE family_id=?1",
                            params![&family[..]],
                        )
                        .is_err()
                );
                Ok(issue_time + 604_800_000)
            })
            .is_err()
    );
    assert!(clock_called);
    let after_expiry: (i64, Vec<u8>) = store
        .db
        .query_row(
            "SELECT cursor,head_hash FROM families WHERE family_id=?1",
            params![&family[..]],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(after_expiry, before_claim);
    assert_eq!(
        store
            .commit_first_claim(family, &claim_candidate, claim_time)
            .unwrap(),
        receipt::control_commit_response(&claim_committed).unwrap()
    );
    assert_eq!(
        store
            .commit_first_claim_with_clock(family, &claim_candidate, || {
                panic!("exact claim retry must not recheck expiry or clock")
            })
            .unwrap(),
        receipt::control_commit_response(&claim_committed).unwrap()
    );
    assert_eq!(
        store
            .stage_first_issue_object(family, issue_object, &issue_stage)
            .unwrap(),
        hex(issue["expect"]["stage_response_cbor_hex"].as_str().unwrap())
    );
    assert_eq!(
        store
            .commit_first_issue(family, &issue_candidate, issue_time + 1)
            .unwrap(),
        issue_expected
    );
    let invite_after_claim = signed_get(
        family,
        genesis_parsed.relay_id,
        issue_parsed.invitation_id,
        invite_seed,
        &control_path,
        [4; 16],
    );
    assert!(
        store
            .control_page_authenticated(family, 0, &control_path, &invite_after_claim)
            .is_err()
    );
    let recipient_seed: [u8; 32] = hex(chain["test_only_inputs"]["recipient_sign_seed_hex"]
        .as_str()
        .unwrap())
    .try_into()
    .unwrap();
    let claim_parsed = authority::verify_first_claim(
        &claim_candidate,
        &genesis_parsed,
        &issue_parsed,
        crypto::hash("control-head", issue_committed).unwrap(),
    )
    .unwrap();
    let pending_read = signed_get(
        family,
        genesis_parsed.relay_id,
        claim_parsed.device_id,
        recipient_seed,
        &control_path,
        [5; 16],
    );
    assert!(
        store
            .control_page_authenticated(family, 0, &control_path, &pending_read)
            .is_ok()
    );
    let pending_object = signed_get(
        family,
        genesis_parsed.relay_id,
        claim_parsed.device_id,
        recipient_seed,
        issue_object_path,
        [6; 16],
    );
    assert!(
        store
            .object_authenticated(family, issue_object, issue_object_path, &pending_object)
            .is_err()
    );
    let challenge_transition = &chain["transitions"][3];
    let challenge_unsigned = cbor::decode(&hex(challenge_transition["unsigned_cbor_hex"]
        .as_str()
        .unwrap()))
    .unwrap();
    let challenge_signatures = cbor::decode(&hex(challenge_transition["signatures_cbor_hex"]
        .as_str()
        .unwrap()))
    .unwrap();
    let challenge_candidate = cbor::encode(&Value::Map(vec![
        (1, challenge_unsigned.clone()),
        (2, challenge_signatures.clone()),
    ]))
    .unwrap();
    let challenge_committed = hex(challenge_transition["committed_cbor_hex"].as_str().unwrap());
    let challenge_time = control_commit_time(&challenge_committed).unwrap();
    assert!(
        store
            .commit_first_challenge(family, &challenge_candidate, challenge_time)
            .is_err()
    );
    let mut challenge_ids = Vec::new();
    for (index, entry) in challenge_transition["manifest"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
    {
        let kind = entry[0].as_u64().unwrap();
        let id_text = entry[1].as_str().unwrap();
        let id: [u8; 16] = hex(id_text).try_into().unwrap();
        let object_bytes = hex(chain["objects_by_id_hex"][id_text].as_str().unwrap());
        let stage = cbor::encode(&Value::Map(vec![
            (1, Value::Integer(1)),
            (2, challenge_unsigned.clone()),
            (3, challenge_signatures.clone()),
            (4, Value::Integer(kind.into())),
            (5, Value::Bytes(id.to_vec())),
            (6, Value::Bytes(object_bytes)),
        ]))
        .unwrap();
        store
            .stage_first_challenge_object(family, id, &stage)
            .unwrap();
        challenge_ids.push((id, stage));
        if index == 0 {
            assert!(
                store
                    .commit_first_challenge(family, &challenge_candidate, challenge_time)
                    .is_err()
            );
        }
    }
    assert_eq!(
        store
            .commit_first_challenge(family, &challenge_candidate, challenge_time)
            .unwrap(),
        receipt::control_commit_response(&challenge_committed).unwrap()
    );
    assert_eq!(
        store
            .commit_first_challenge(family, &challenge_candidate, challenge_time + 1)
            .unwrap(),
        receipt::control_commit_response(&challenge_committed).unwrap()
    );
    let hpke_path = format!(
        "/v1/families/{}/objects/{}",
        lower_hex(&family),
        lower_hex(&challenge_ids[0].0)
    );
    let hpke_read = signed_get(
        family,
        genesis_parsed.relay_id,
        claim_parsed.device_id,
        recipient_seed,
        &hpke_path,
        [7; 16],
    );
    assert!(
        store
            .object_authenticated(family, challenge_ids[0].0, &hpke_path, &hpke_read)
            .is_ok()
    );
    let verifier_path = format!(
        "/v1/families/{}/objects/{}",
        lower_hex(&family),
        lower_hex(&challenge_ids[1].0)
    );
    let verifier_read = signed_get(
        family,
        genesis_parsed.relay_id,
        claim_parsed.device_id,
        recipient_seed,
        &verifier_path,
        [8; 16],
    );
    assert!(
        store
            .object_authenticated(family, challenge_ids[1].0, &verifier_path, &verifier_read)
            .is_err()
    );
    let proof_transition = &chain["transitions"][4];
    let proof_candidate = cbor::encode(&Value::Map(vec![
        (
            1,
            cbor::decode(&hex(proof_transition["unsigned_cbor_hex"]
                .as_str()
                .unwrap()))
            .unwrap(),
        ),
        (
            2,
            cbor::decode(&hex(proof_transition["signatures_cbor_hex"]
                .as_str()
                .unwrap()))
            .unwrap(),
        ),
    ]))
    .unwrap();
    let proof_committed = hex(proof_transition["committed_cbor_hex"].as_str().unwrap());
    let proof_time = control_commit_time(&proof_committed).unwrap();
    assert_eq!(
        store
            .commit_first_proof(family, &proof_candidate, proof_time)
            .unwrap(),
        receipt::control_commit_response(&proof_committed).unwrap()
    );
    assert_eq!(
        store
            .commit_first_proof(family, &proof_candidate, proof_time + 1)
            .unwrap(),
        receipt::control_commit_response(&proof_committed).unwrap()
    );
    let admission_transition = &chain["transitions"][5];
    let admission_unsigned = cbor::decode(&hex(admission_transition["unsigned_cbor_hex"]
        .as_str()
        .unwrap()))
    .unwrap();
    let admission_signatures = cbor::decode(&hex(admission_transition["signatures_cbor_hex"]
        .as_str()
        .unwrap()))
    .unwrap();
    let admission_candidate = cbor::encode(&Value::Map(vec![
        (1, admission_unsigned.clone()),
        (2, admission_signatures.clone()),
    ]))
    .unwrap();
    let admission_committed = hex(admission_transition["committed_cbor_hex"].as_str().unwrap());
    let admission_time = control_commit_time(&admission_committed).unwrap();
    assert!(
        store
            .commit_first_admission(family, &admission_candidate, admission_time)
            .is_err()
    );
    let mut admission_ids = Vec::new();
    for (index, entry) in admission_transition["manifest"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
    {
        let kind = entry[0].as_u64().unwrap();
        let id_text = entry[1].as_str().unwrap();
        let id: [u8; 16] = hex(id_text).try_into().unwrap();
        let object_bytes = hex(chain["objects_by_id_hex"][id_text].as_str().unwrap());
        let stage = cbor::encode(&Value::Map(vec![
            (1, Value::Integer(1)),
            (2, admission_unsigned.clone()),
            (3, admission_signatures.clone()),
            (4, Value::Integer(kind.into())),
            (5, Value::Bytes(id.to_vec())),
            (6, Value::Bytes(object_bytes)),
        ]))
        .unwrap();
        store
            .stage_first_admission_object(family, id, &stage)
            .unwrap();
        admission_ids.push(id);
        if index == 0 {
            assert!(
                store
                    .commit_first_admission(family, &admission_candidate, admission_time)
                    .is_err()
            );
        }
    }
    assert_eq!(
        store
            .commit_first_admission(family, &admission_candidate, admission_time)
            .unwrap(),
        receipt::control_commit_response(&admission_committed).unwrap()
    );
    assert_eq!(
        store
            .commit_first_admission(family, &admission_candidate, admission_time + 1)
            .unwrap(),
        receipt::control_commit_response(&admission_committed).unwrap()
    );
    let grant_path = format!(
        "/v1/families/{}/objects/{}",
        lower_hex(&family),
        lower_hex(&admission_ids[1])
    );
    let grant_read = signed_get(
        family,
        genesis_parsed.relay_id,
        claim_parsed.device_id,
        recipient_seed,
        &grant_path,
        [9; 16],
    );
    assert!(
        store
            .object_authenticated(family, admission_ids[1], &grant_path, &grant_read)
            .is_ok()
    );
    let verifier_after_grant = signed_get(
        family,
        genesis_parsed.relay_id,
        claim_parsed.device_id,
        recipient_seed,
        &verifier_path,
        [10; 16],
    );
    assert!(
        store
            .object_authenticated(
                family,
                challenge_ids[1].0,
                &verifier_path,
                &verifier_after_grant
            )
            .is_ok()
    );
}
