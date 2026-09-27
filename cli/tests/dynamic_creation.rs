//! Fresh keys, promoted history, and exact signed bytes cross the relay HTTP boundary.

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Method, Request, StatusCode, header::CONTENT_TYPE},
};

use babytrack_core::{
    active_pull,
    bootstrap::InvitationBootstrap,
    cbor::{self, Value},
    creation::ManagerCreation,
    crypto,
    enrollment::EnrollmentAttempt,
    first_admission::FirstAdmission,
    first_challenge::FirstChallenge,
    first_proof::FirstProof,
    first_removal::FirstRemoval,
    issue::{FirstInviteIssue, LaterInviteIssue},
    operation::{Hlc, Kind, NewOperation, Scope},
    portable_file::{
        export_readable_local, export_readable_shared, parse_readable, private_copy_after_removal,
        private_copy_shared,
    },
    shared_history::{self, PendingBatchResult, PublicHistorySession},
    shared_ready::{NextUpload, ReadyFamilySession},
    sqlite_store::{FamilyHandle, SqliteStore},
    sync_wire::{BatchResult, ControlPage, LogPage, OpaqueObject},
};
use babytrack_server::{RelayStore, test_router};
use tower::ServiceExt;

async fn http_bytes(app: &Router, method: Method, path: &str, body: Vec<u8>) -> Vec<u8> {
    let (status, bytes) = http_response(app, method, path, body).await;
    assert_eq!(status, StatusCode::OK);
    bytes
}

async fn http_response(
    app: &Router,
    method: Method,
    path: &str,
    body: Vec<u8>,
) -> (StatusCode, Vec<u8>) {
    let request = Request::builder()
        .method(method)
        .uri(path)
        .header(CONTENT_TYPE, "application/cbor")
        .body(Body::from(body))
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 4 * 1024 * 1024)
        .await
        .unwrap()
        .to_vec();
    (status, bytes)
}

async fn stage_objects(app: &Router, family: [u8; 16], bodies: &[([u8; 16], Vec<u8>)]) {
    for (id, body) in bodies {
        let path = format!(
            "/v1/families/{}/objects/{}",
            lower_hex(&family),
            lower_hex(id)
        );
        http_bytes(app, Method::POST, &path, body.clone()).await;
    }
}

async fn commit_control(app: &Router, family: [u8; 16], candidate: &[u8]) -> Vec<u8> {
    let path = format!("/v1/families/{}/control", lower_hex(&family));
    http_bytes(app, Method::POST, &path, candidate.to_vec()).await
}

fn lower_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
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

/// Hostile-relay fixture: issue an internally signed but history-inconsistent
/// receipt so the client must classify it as unknown.
fn resign_batch_result(
    result_bytes: &[u8],
    relay_seed: &[u8; 32],
    accepted: bool,
    cursor: u64,
    head: [u8; 32],
    next_expected: Option<u64>,
) -> Vec<u8> {
    let Value::Map(result) = cbor::decode(result_bytes).unwrap() else {
        panic!()
    };
    let Value::Bytes(receipt_bytes) = &result[1].1 else {
        panic!()
    };
    let Value::Map(mut receipt) = cbor::decode(receipt_bytes).unwrap() else {
        panic!()
    };
    let Value::Map(body) = &mut receipt[0].1 else {
        panic!()
    };
    body[5].1 = Value::Bool(accepted);
    body[6].1 = Value::Integer(cursor.into());
    body[7].1 = Value::Bytes(head.to_vec());
    if accepted {
        body[9].1 = Value::Null;
        let Value::Integer(sequence) = body[8].1 else {
            panic!()
        };
        body[10].1 = Value::Integer(sequence + 1);
    }
    if let Some(next) = next_expected {
        body[10].1 = Value::Integer(next.into());
    }
    receipt[1].1 = Value::Bytes(
        crypto::sign_cbor(
            "batch-receipt",
            &cbor::encode(&receipt[0].1).unwrap(),
            relay_seed,
        )
        .unwrap()
        .to_vec(),
    );
    cbor::encode(&Value::Map(vec![
        (1, Value::Integer(1)),
        (2, Value::Bytes(cbor::encode(&Value::Map(receipt)).unwrap())),
    ]))
    .unwrap()
}

#[tokio::test]
async fn existing_local_family_promotes_and_shares_with_durable_keys() {
    dynamic_flow(false, false).await;
}

#[tokio::test]
async fn manager_batch_interleaves_before_invite_and_recipient_joins() {
    dynamic_flow(true, false).await;
}

#[tokio::test]
async fn removed_recipient_recovers_accepted_result_after_lost_response() {
    dynamic_flow(false, true).await;
}

#[tokio::test]
async fn later_issue_keeps_exact_candidate_and_commits_after_first_issue() {
    let dir = tempfile::tempdir().unwrap();
    let local_path = dir.path().join("manager.db");
    let relay_path = dir.path().join("relay.db");
    let relay_seed = [0x73; 32];
    let family = FamilyHandle {
        family_id: v4(0x41),
        device_id: v4(0x42),
    };
    let wrapping = [0x43; 32];
    let mut local = SqliteStore::open(&local_path).unwrap();
    local
        .create_family(family.family_id, family.device_id)
        .unwrap();
    let creation = ManagerCreation::prepare(
        &mut local,
        family,
        crypto::signing_public_key(&relay_seed),
        &wrapping,
    )
    .unwrap();
    RelayStore::open(&relay_path, relay_seed).unwrap();
    let app = test_router(&relay_path, relay_seed).unwrap();
    stage_objects(&app, family.family_id, &creation.stage_bodies().unwrap()).await;
    let genesis_response = commit_control(&app, family.family_id, creation.candidate_bytes()).await;
    let Value::Map(genesis_result) = cbor::decode(&genesis_response).unwrap() else {
        panic!()
    };
    let Value::Bytes(genesis) = &genesis_result[1].1 else {
        panic!()
    };
    creation.confirm(&mut local, genesis).unwrap();

    let first = FirstInviteIssue::prepare(&mut local, &creation, &wrapping, 1).unwrap();
    stage_objects(
        &app,
        family.family_id,
        &[(first.object_id(), first.stage_body().unwrap())],
    )
    .await;
    let first_response = commit_control(&app, family.family_id, first.candidate_bytes()).await;
    let Value::Map(first_result) = cbor::decode(&first_response).unwrap() else {
        panic!()
    };
    let Value::Bytes(first_committed) = &first_result[1].1 else {
        panic!()
    };
    first
        .confirm(
            &mut local,
            &creation,
            first_committed,
            "http://localhost:3400",
        )
        .unwrap();

    let later =
        LaterInviteIssue::prepare_for_initial_manager(&mut local, &creation, &wrapping, 2).unwrap();
    assert_ne!(later.invitation_id(), first.invitation_id());
    let later_candidate = later.candidate_bytes().to_vec();
    let later_stage = later.stage_body().unwrap();
    drop(local);
    let mut local = SqliteStore::open(&local_path).unwrap();
    let later = LaterInviteIssue::resume(&local, family, later.invitation_id(), &wrapping).unwrap();
    assert_eq!(later.candidate_bytes(), later_candidate);
    assert_eq!(later.stage_body().unwrap(), later_stage);
    assert!(LaterInviteIssue::resume(&local, family, later.invitation_id(), &[0; 32]).is_err());
    stage_objects(&app, family.family_id, &[(later.object_id(), later_stage)]).await;
    let second_response = commit_control(&app, family.family_id, &later_candidate).await;
    assert_eq!(
        commit_control(&app, family.family_id, &later_candidate).await,
        second_response
    );
    let Value::Map(second_result) = cbor::decode(&second_response).unwrap() else {
        panic!()
    };
    let Value::Bytes(second_committed) = &second_result[1].1 else {
        panic!()
    };
    let link = later
        .confirm(&mut local, second_committed, "http://localhost:3400")
        .unwrap();
    assert_eq!(
        InvitationBootstrap::from_fragment(&link.to_fragment().unwrap())
            .unwrap()
            .invitation_id(),
        later.invitation_id()
    );
    assert_eq!(
        PublicHistorySession::resume(&local, family)
            .unwrap()
            .cursor(),
        3
    );
    assert_eq!(
        ManagerCreation::resume(&local, family, &wrapping)
            .unwrap()
            .ready_session(&local)
            .unwrap()
            .observed_cursor(),
        3
    );
}

async fn dynamic_flow(early_batch: bool, accepted_before_removal: bool) {
    let shift = if early_batch { 1 } else { 0 };
    let dir = tempfile::tempdir().unwrap();
    let local_path = dir.path().join("manager.db");
    let relay_path = dir.path().join("relay.db");
    let relay_seed = [0x71; 32];
    let relay_public = crypto::signing_public_key(&relay_seed);
    let wrapping_key = [0x4c; 32];
    let family = FamilyHandle {
        family_id: v4(0x26),
        device_id: v4(0x27),
    };
    let mut local = SqliteStore::open(&local_path).unwrap();
    local
        .create_family(family.family_id, family.device_id)
        .unwrap();
    let pre_child_id = v7(0x20);
    local
        .append_local(
            family,
            NewOperation {
                family_id: family.family_id,
                operation_id: v7(0x21),
                record_id: pre_child_id,
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
                fields: Some(vec![(1, Value::Text("Before sharing".to_owned()))]),
            },
            1_700_000_000_000,
        )
        .unwrap();
    let creation =
        ManagerCreation::prepare(&mut local, family, relay_public, &wrapping_key).unwrap();
    let candidate = creation.candidate_bytes().to_vec();
    let stage = creation.stage_body().unwrap();
    let genesis_stages = creation.stage_bodies().unwrap();
    assert_eq!(genesis_stages.len(), 2);
    let post_child_id = v7(0x22);
    local
        .append_local(
            family,
            NewOperation {
                family_id: family.family_id,
                operation_id: v7(0x23),
                record_id: post_child_id,
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
                fields: Some(vec![(1, Value::Text("After sharing started".to_owned()))]),
            },
            1_700_000_000_001,
        )
        .unwrap();
    let promotion_id = creation.promotion_id();
    assert_ne!(creation.transition_id(), promotion_id);
    let object_id = creation.object_id();
    assert_eq!(object_id, promotion_id);
    assert_eq!(
        ManagerCreation::prepare(&mut local, family, relay_public, &wrapping_key)
            .unwrap()
            .candidate_bytes(),
        candidate
    );
    assert!(ManagerCreation::prepare(&mut local, family, [1; 32], &wrapping_key).is_err());
    drop(local);
    let mut local = SqliteStore::open(&local_path).unwrap();
    assert!(ManagerCreation::resume(&local, family, &[0; 32]).is_err());
    let resumed = ManagerCreation::resume(&local, family, &wrapping_key).unwrap();
    assert_eq!(resumed.candidate_bytes(), candidate);
    assert_eq!(resumed.stage_body().unwrap(), stage);
    assert_eq!(resumed.stage_bodies().unwrap(), genesis_stages);

    let mut relay = RelayStore::open(&relay_path, relay_seed).unwrap();
    let app = test_router(&relay_path, relay_seed).unwrap();
    stage_objects(&app, family.family_id, &genesis_stages).await;
    assert!(
        relay
            .committed_object(family.family_id, object_id)
            .unwrap()
            .is_none()
    );
    let response = commit_control(&app, family.family_id, &candidate).await;
    assert_eq!(
        commit_control(&app, family.family_id, &candidate).await,
        response
    );
    let Value::Map(fields) = cbor::decode(&response).unwrap() else {
        panic!()
    };
    let Value::Bytes(committed) = &fields[1].1 else {
        panic!()
    };
    let path = format!(
        "/v1/families/{}/promotions/{}",
        lower_hex(&family.family_id),
        lower_hex(&promotion_id)
    );
    let auth = resumed.sign_get(&path).unwrap();
    let result = relay
        .promotion_result_authenticated(family.family_id, promotion_id, &path, &auth.bytes)
        .unwrap();
    let Value::Map(result) = cbor::decode(&result).unwrap() else {
        panic!()
    };
    assert_eq!(result[1].1, Value::Bytes(committed.clone()));
    let ready = resumed.confirm(&mut local, committed).unwrap();
    assert_eq!(ready.observed_cursor(), 1);
    assert_eq!(ready.active_epoch(), 1);
    assert!(ready.projection().record(&pre_child_id).is_some());
    assert!(ready.projection().record(&post_child_id).is_none());
    drop(local);
    let mut local = SqliteStore::open(&local_path).unwrap();
    let resumed = ManagerCreation::resume(&local, family, &wrapping_key).unwrap();
    assert_eq!(
        resumed
            .confirm(&mut local, committed)
            .unwrap()
            .observed_cursor(),
        1
    );
    // The durable outbox is sealed against genesis before any invite exists.
    // It must remain uploadable after every join control advances the head.
    let genesis_ready = resumed.confirm(&mut local, committed).unwrap();
    let staged_manager_batch = match resumed
        .stage_next_local(&genesis_ready, &mut local)
        .unwrap()
    {
        NextUpload::Fresh(batch) => batch,
        NextUpload::RetryExact(_) => panic!("manager batch was already pending"),
    };
    assert_eq!(staged_manager_batch.from_index, 2);
    if early_batch {
        let batch_path = format!("/v1/families/{}/batches", lower_hex(&family.family_id));
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
        let mut writers = Vec::new();
        for _ in 0..2 {
            let mut connection = RelayStore::open(&relay_path, relay_seed).unwrap();
            let barrier = barrier.clone();
            let envelope = staged_manager_batch.envelope_bytes.clone();
            let family_id = family.family_id;
            writers.push(std::thread::spawn(move || {
                barrier.wait();
                for _ in 0..50 {
                    if let Ok(result) = connection.commit_batch(family_id, &envelope) {
                        return result;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
                panic!("concurrent exact batch retry did not resolve");
            }));
        }
        barrier.wait();
        let first = writers.remove(0).join().unwrap();
        assert_eq!(writers.remove(0).join().unwrap(), first);
        let response = http_bytes(
            &app,
            Method::POST,
            &batch_path,
            staged_manager_batch.envelope_bytes.clone(),
        )
        .await;
        assert_eq!(response, first);
        let db = rusqlite::Connection::open(&relay_path).unwrap();
        let cursor: i64 = db
            .query_row(
                "SELECT cursor FROM families WHERE family_id=?1",
                [&family.family_id[..]],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(cursor, 2);
        let batches: i64 = db
            .query_row(
                "SELECT COUNT(*) FROM entries WHERE family_id=?1 AND kind=2",
                [&family.family_id[..]],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(batches, 1);
        let Value::Map(fields) = cbor::decode(&response).unwrap() else {
            panic!()
        };
        let Value::Bytes(receipt) = &fields[1].1 else {
            panic!()
        };
        let mut public = PublicHistorySession::resume(&local, family).unwrap();
        public
            .accept_batch(&mut local, &staged_manager_batch.envelope_bytes, receipt)
            .unwrap();
    }

    let issue = FirstInviteIssue::prepare(&mut local, &resumed, &wrapping_key, 1).unwrap();
    let issue_candidate = issue.candidate_bytes().to_vec();
    let issue_stage = issue.stage_body().unwrap();
    assert_ne!(issue.transition_id(), issue.invitation_id());
    assert_eq!(
        FirstInviteIssue::prepare(&mut local, &resumed, &wrapping_key, 1)
            .unwrap()
            .candidate_bytes(),
        issue_candidate
    );
    drop(local);
    let mut local = SqliteStore::open(&local_path).unwrap();
    let resumed = ManagerCreation::resume(&local, family, &wrapping_key).unwrap();
    let issue = FirstInviteIssue::resume(&local, &resumed, &wrapping_key).unwrap();
    assert_eq!(issue.stage_body().unwrap(), issue_stage);
    stage_objects(
        &app,
        family.family_id,
        &[(issue.object_id(), issue_stage.clone())],
    )
    .await;
    let response = commit_control(&app, family.family_id, &issue_candidate).await;
    let Value::Map(fields) = cbor::decode(&response).unwrap() else {
        panic!()
    };
    let Value::Bytes(committed_issue) = &fields[1].1 else {
        panic!()
    };
    let bootstrap = issue
        .confirm(
            &mut local,
            &resumed,
            committed_issue,
            "http://localhost:3400",
        )
        .unwrap();
    let fragment = bootstrap.to_fragment().unwrap();
    assert_eq!(
        InvitationBootstrap::from_fragment(&fragment)
            .unwrap()
            .invitation_id(),
        issue.invitation_id()
    );

    let received_link = InvitationBootstrap::from_fragment(&fragment).unwrap();
    let invite_path = format!(
        "/v1/families/{}/invites/{}",
        lower_hex(&family.family_id),
        lower_hex(&issue.invitation_id()),
    );
    let auth = received_link.sign_get(&invite_path).unwrap();
    let Value::Map(invite_result) =
        cbor::decode(&http_bytes(&app, Method::GET, &invite_path, auth.bytes).await).unwrap()
    else {
        panic!()
    };
    assert_eq!(invite_result[1].1, Value::Bytes(committed_issue.clone()));
    let result_path = format!(
        "/v1/families/{}/control-results/{}",
        lower_hex(&family.family_id),
        lower_hex(&issue.transition_id()),
    );
    let auth = received_link.sign_get(&result_path).unwrap();
    let Value::Map(control_result) =
        cbor::decode(&http_bytes(&app, Method::GET, &result_path, auth.bytes).await).unwrap()
    else {
        panic!()
    };
    assert_eq!(control_result[1].1, Value::Bytes(committed_issue.clone()));
    let unknown_path = format!(
        "/v1/families/{}/control-results/{}",
        lower_hex(&family.family_id),
        lower_hex(&v4(0x79)),
    );
    let auth = received_link.sign_get(&unknown_path).unwrap();
    let Value::Map(unknown_result) =
        cbor::decode(&http_bytes(&app, Method::GET, &unknown_path, auth.bytes).await).unwrap()
    else {
        panic!()
    };
    assert_eq!(unknown_result[1].1, Value::Null);
    let path = format!(
        "/v1/families/{}/control?after=0",
        lower_hex(&family.family_id)
    );
    let auth = received_link.sign_get(&path).unwrap();
    let page = ControlPage::decode(
        &http_bytes(&app, Method::GET, &path, auth.bytes.clone()).await,
        family.family_id,
        0,
    )
    .unwrap();
    assert_eq!(page.entries.len(), 2);
    let recipient_path = dir.path().join("recipient.db");
    let recipient_wrap = [0x8b; 32];
    let mut recipient_store = SqliteStore::open(&recipient_path).unwrap();
    let enrollment = if early_batch {
        let log_path = format!("/v1/families/{}/log?after=0", lower_hex(&family.family_id));
        let unauthorized = received_link.sign_get(&log_path).unwrap();
        assert_eq!(
            http_response(&app, Method::GET, &log_path, unauthorized.bytes)
                .await
                .0,
            StatusCode::FORBIDDEN
        );
        EnrollmentAttempt::prepare_sparse(
            &mut recipient_store,
            &received_link,
            &page.entries[0].committed_bytes,
            &page.entries[1].committed_bytes,
            &recipient_wrap,
        )
    } else {
        EnrollmentAttempt::prepare(
            &mut recipient_store,
            &received_link,
            &page.entries[0].committed_bytes,
            &page.entries[1].committed_bytes,
            &recipient_wrap,
        )
    }
    .unwrap();
    let claim_candidate = enrollment.claim_candidate().to_vec();
    drop(recipient_store);
    let mut recipient_store = SqliteStore::open(&recipient_path).unwrap();
    let resumed_enrollment =
        EnrollmentAttempt::resume(&mut recipient_store, family.family_id, &recipient_wrap).unwrap();
    assert_eq!(resumed_enrollment.claim_candidate(), claim_candidate);
    let response = commit_control(&app, family.family_id, &claim_candidate).await;
    let Value::Map(fields) = cbor::decode(&response).unwrap() else {
        panic!()
    };
    let Value::Bytes(committed_claim) = &fields[1].1 else {
        panic!()
    };
    let mut public = PublicHistorySession::resume(&recipient_store, enrollment.family()).unwrap();
    if early_batch {
        resumed_enrollment
            .confirm_sparse_claim(&mut recipient_store, committed_claim)
            .unwrap();
        assert_eq!(public.cursor(), 1);
        let log_path = format!("/v1/families/{}/log?after=0", lower_hex(&family.family_id));
        let read = resumed_enrollment.sign_get(&log_path).unwrap();
        assert_eq!(
            http_response(&app, Method::GET, &log_path, read.bytes)
                .await
                .0,
            StatusCode::FORBIDDEN
        );
    } else {
        public
            .accept_control(&mut recipient_store, committed_claim)
            .unwrap();
        assert_eq!(public.cursor(), 3);
    }
    assert!(
        relay
            .control_page_authenticated(family.family_id, 0, &path, &auth.bytes)
            .is_err()
    );

    let mut manager_public = PublicHistorySession::resume(&local, family).unwrap();
    manager_public
        .accept_control(&mut local, committed_claim)
        .unwrap();
    let challenge = FirstChallenge::prepare(
        &mut local,
        &resumed,
        issue.invitation_id(),
        enrollment.family().device_id,
        &wrapping_key,
    )
    .unwrap();
    assert_ne!(challenge.challenge_id(), challenge.transition_id());
    let challenge_candidate = challenge.candidate_bytes().to_vec();
    let staged = challenge.stage_bodies().unwrap();
    drop(local);
    let mut local = SqliteStore::open(&local_path).unwrap();
    let resumed = ManagerCreation::resume(&local, family, &wrapping_key).unwrap();
    let challenge = FirstChallenge::resume(&local, &resumed, &wrapping_key).unwrap();
    assert_eq!(challenge.candidate_bytes(), challenge_candidate);
    assert_eq!(challenge.stage_bodies().unwrap(), staged);
    stage_objects(&app, family.family_id, &staged).await;
    let response = commit_control(&app, family.family_id, &challenge_candidate).await;
    let Value::Map(fields) = cbor::decode(&response).unwrap() else {
        panic!()
    };
    let Value::Bytes(committed_challenge) = &fields[1].1 else {
        panic!()
    };
    challenge.confirm(&mut local, committed_challenge).unwrap();
    let pending_path = format!(
        "/v1/families/{}/control?after={}",
        lower_hex(&family.family_id),
        3 + shift
    );
    let read = resumed_enrollment.sign_get(&pending_path).unwrap();
    let page = ControlPage::decode(
        &http_bytes(&app, Method::GET, &pending_path, read.bytes).await,
        family.family_id,
        3 + shift,
    )
    .unwrap();
    assert_eq!(page.entries.len(), 1);
    if early_batch {
        resumed_enrollment
            .accept_sparse_control(&mut recipient_store, &page.entries[0].committed_bytes)
            .unwrap();
    } else {
        public
            .accept_control(&mut recipient_store, &page.entries[0].committed_bytes)
            .unwrap();
    }
    let pending_chain =
        shared_history::first_join_chain(&recipient_store, enrollment.family(), relay_public, 4)
            .unwrap();
    let verified = pending_chain
        .latest_challenge(&issue.invitation_id())
        .unwrap();
    let hpke_id = verified.hpke_object_id();
    let object_path = format!(
        "/v1/families/{}/objects/{}",
        lower_hex(&family.family_id),
        lower_hex(&hpke_id)
    );
    let read = resumed_enrollment.sign_get(&object_path).unwrap();
    let hpke_object = OpaqueObject::decode(
        &http_bytes(&app, Method::GET, &object_path, read.bytes).await,
        hpke_id,
    )
    .unwrap();
    if early_batch {
        resumed_enrollment
            .accept_sparse_object(&mut recipient_store, hpke_id, &hpke_object.object_bytes)
            .unwrap();
    } else {
        public
            .accept_object(&mut recipient_store, hpke_id, &hpke_object.object_bytes)
            .unwrap();
    }
    assert!(
        resumed_enrollment
            .prove_challenge(verified, &hpke_object.object_bytes)
            .is_ok()
    );
    let verifier_path = format!(
        "/v1/families/{}/objects/{}",
        lower_hex(&family.family_id),
        lower_hex(&verified.verifier_object_id())
    );
    let read = resumed_enrollment.sign_get(&verifier_path).unwrap();
    assert!(
        relay
            .object_authenticated(
                family.family_id,
                verified.verifier_object_id(),
                &verifier_path,
                &read.bytes
            )
            .is_err()
    );

    let proof =
        FirstProof::prepare(&mut recipient_store, &resumed_enrollment, &recipient_wrap).unwrap();
    let proof_candidate = proof.candidate_bytes().to_vec();
    drop(recipient_store);
    let mut recipient_store = SqliteStore::open(&recipient_path).unwrap();
    let resumed_enrollment =
        EnrollmentAttempt::resume(&mut recipient_store, family.family_id, &recipient_wrap).unwrap();
    let proof = FirstProof::resume(&recipient_store, &resumed_enrollment, &recipient_wrap).unwrap();
    assert_eq!(proof.candidate_bytes(), proof_candidate);
    let response = commit_control(&app, family.family_id, &proof_candidate).await;
    let Value::Map(fields) = cbor::decode(&response).unwrap() else {
        panic!()
    };
    let Value::Bytes(committed_proof) = &fields[1].1 else {
        panic!()
    };
    proof
        .confirm(&mut recipient_store, committed_proof)
        .unwrap();
    let proof_path = format!(
        "/v1/families/{}/control?after={}",
        lower_hex(&family.family_id),
        4 + shift
    );
    let read = resumed.sign_get(&proof_path).unwrap();
    let page = ControlPage::decode(
        &relay
            .control_page_authenticated(family.family_id, 4 + shift, &proof_path, &read.bytes)
            .unwrap(),
        family.family_id,
        4 + shift,
    )
    .unwrap();
    assert_eq!(page.entries.len(), 1);
    let mut manager_public = PublicHistorySession::resume(&local, family).unwrap();
    manager_public
        .accept_control(&mut local, &page.entries[0].committed_bytes)
        .unwrap();
    let read = resumed.sign_get(&verifier_path).unwrap();
    let verifier_object = OpaqueObject::decode(
        &relay
            .object_authenticated(
                family.family_id,
                verified.verifier_object_id(),
                &verifier_path,
                &read.bytes,
            )
            .unwrap(),
        verified.verifier_object_id(),
    )
    .unwrap();
    resumed
        .verify_pending_proof(
            &local,
            issue.invitation_id(),
            &verifier_object.object_bytes,
            proof.proof_signature(),
        )
        .unwrap();

    let admission = FirstAdmission::prepare(
        &mut local,
        &resumed,
        issue.invitation_id(),
        enrollment.family().device_id,
        &wrapping_key,
    )
    .unwrap();
    let admission_candidate = admission.candidate_bytes().to_vec();
    let admission_stage = admission.stage_bodies().unwrap();
    drop(local);
    let mut local = SqliteStore::open(&local_path).unwrap();
    let resumed = ManagerCreation::resume(&local, family, &wrapping_key).unwrap();
    let admission = FirstAdmission::resume(&local, &resumed, &wrapping_key).unwrap();
    assert_eq!(admission.candidate_bytes(), admission_candidate);
    assert_eq!(admission.stage_bodies().unwrap(), admission_stage);
    stage_objects(&app, family.family_id, &admission_stage).await;
    let response = commit_control(&app, family.family_id, &admission_candidate).await;
    let Value::Map(fields) = cbor::decode(&response).unwrap() else {
        panic!()
    };
    let Value::Bytes(committed_admission) = &fields[1].1 else {
        panic!()
    };
    admission
        .confirm(&mut local, &resumed, committed_admission)
        .unwrap();
    let mut recipient_public =
        PublicHistorySession::resume(&recipient_store, enrollment.family()).unwrap();
    let admission_path = format!(
        "/v1/families/{}/control?after={}",
        lower_hex(&family.family_id),
        5 + shift
    );
    let read = resumed_enrollment.sign_get(&admission_path).unwrap();
    let page = ControlPage::decode(
        &http_bytes(&app, Method::GET, &admission_path, read.bytes).await,
        family.family_id,
        5 + shift,
    )
    .unwrap();
    assert_eq!(page.entries.len(), 1);
    if early_batch {
        resumed_enrollment
            .accept_sparse_control(&mut recipient_store, &page.entries[0].committed_bytes)
            .unwrap();
        assert!(
            ReadyFamilySession::from_enrollment(&recipient_store, &resumed_enrollment).is_err()
        );
        let interrupted =
            active_pull::pull_active_log(&mut recipient_store, enrollment.family(), 2, |path| {
                let app = app.clone();
                let read = resumed_enrollment.sign_get(&path).unwrap();
                async move {
                    if path.contains("/batch-results/") {
                        Err(())
                    } else {
                        Ok(http_bytes(&app, Method::GET, &path, read.bytes).await)
                    }
                }
            })
            .await;
        assert!(matches!(interrupted, Err(active_pull::Error::Transport)));
        assert_eq!(
            PublicHistorySession::resume(&recipient_store, enrollment.family())
                .unwrap()
                .cursor(),
            1
        );
        drop(recipient_store);
        recipient_store = SqliteStore::open(&recipient_path).unwrap();
        let progress =
            active_pull::pull_active_log(&mut recipient_store, enrollment.family(), 2, |path| {
                let app = app.clone();
                let read = resumed_enrollment.sign_get(&path).unwrap();
                async move { Ok::<_, ()>(http_bytes(&app, Method::GET, &path, read.bytes).await) }
            })
            .await
            .unwrap();
        assert_eq!(progress.verified_cursor, 7);
        assert!(progress.no_more_visible);
        recipient_public =
            PublicHistorySession::resume(&recipient_store, enrollment.family()).unwrap();
    } else {
        recipient_public
            .accept_control(&mut recipient_store, &page.entries[0].committed_bytes)
            .unwrap();
    }
    assert!(ReadyFamilySession::from_enrollment(&recipient_store, &resumed_enrollment).is_err());
    let chunk_id = genesis_stages[1].0;
    let substituted_object = active_pull::hydrate_manifest_objects(
        &mut recipient_store,
        enrollment.family(),
        16,
        |path| {
            let app = app.clone();
            let read = resumed_enrollment.sign_get(&path).unwrap();
            async move {
                let response = http_bytes(&app, Method::GET, &path, read.bytes).await;
                let Value::Map(mut fields) = cbor::decode(&response).unwrap() else {
                    panic!()
                };
                fields[1].1 = Value::Integer(65535);
                Ok::<_, ()>(cbor::encode(&Value::Map(fields)).unwrap())
            }
        },
    )
    .await;
    assert!(matches!(
        substituted_object,
        Err(active_pull::Error::Invalid(_))
    ));
    assert!(ReadyFamilySession::from_enrollment(&recipient_store, &resumed_enrollment).is_err());
    let missing_chunk = active_pull::hydrate_manifest_objects(
        &mut recipient_store,
        enrollment.family(),
        16,
        |path| {
            let app = app.clone();
            let read = resumed_enrollment.sign_get(&path).unwrap();
            let blocked = path.ends_with(&lower_hex(&chunk_id));
            async move {
                if blocked {
                    Err(())
                } else {
                    Ok(http_bytes(&app, Method::GET, &path, read.bytes).await)
                }
            }
        },
    )
    .await;
    assert!(matches!(missing_chunk, Err(active_pull::Error::Transport)));
    assert!(ReadyFamilySession::from_enrollment(&recipient_store, &resumed_enrollment).is_err());
    drop(recipient_store);
    recipient_store = SqliteStore::open(&recipient_path).unwrap();
    let hydrated = active_pull::hydrate_manifest_objects(
        &mut recipient_store,
        enrollment.family(),
        16,
        |path| {
            let app = app.clone();
            let read = resumed_enrollment.sign_get(&path).unwrap();
            async move { Ok::<_, ()>(http_bytes(&app, Method::GET, &path, read.bytes).await) }
        },
    )
    .await
    .unwrap();
    assert!(!hydrated.remaining);
    let ready = ReadyFamilySession::from_enrollment(&recipient_store, &resumed_enrollment).unwrap();
    assert_eq!(ready.observed_cursor(), 6 + shift);
    assert_eq!(ready.active_epoch(), 1);
    assert!(ready.projection().record(&pre_child_id).is_some());

    let child_id = v7(0x32);
    recipient_store
        .append_local(
            enrollment.family(),
            NewOperation {
                family_id: family.family_id,
                operation_id: v7(0x33),
                record_id: child_id,
                scope: Scope::Child,
                kind: Kind::Create,
                author_device_id: enrollment.family().device_id,
                hlc: Hlc {
                    wall_ms: 1_700_000_001_000,
                    counter: 0,
                    device_id: enrollment.family().device_id,
                },
                record_type: Some("child".to_owned()),
                child_id: None,
                fields: Some(vec![(1, Value::Text("Baby".to_owned()))]),
            },
            1_700_000_001_000,
        )
        .unwrap();
    let batch = match ready
        .stage_enrolled_local(&mut recipient_store, &resumed_enrollment)
        .unwrap()
    {
        NextUpload::Fresh(batch) => batch,
        NextUpload::RetryExact(_) => panic!("first upload was already pending"),
    };
    let batch_path = format!("/v1/families/{}/batches", lower_hex(&family.family_id));
    let response = http_bytes(
        &app,
        Method::POST,
        &batch_path,
        batch.envelope_bytes.clone(),
    )
    .await;
    assert_eq!(
        http_bytes(
            &app,
            Method::POST,
            &batch_path,
            batch.envelope_bytes.clone()
        )
        .await,
        response
    );
    let Value::Map(fields) = cbor::decode(&response).unwrap() else {
        panic!()
    };
    let Value::Bytes(receipt) = &fields[1].1 else {
        panic!()
    };
    recipient_public
        .accept_batch(&mut recipient_store, &batch.envelope_bytes, receipt)
        .unwrap();
    let recipient_ready =
        ReadyFamilySession::from_enrollment(&recipient_store, &resumed_enrollment).unwrap();
    assert_eq!(recipient_ready.observed_cursor(), 7 + shift);
    assert!(
        LaterInviteIssue::prepare_for_admitted_manager(
            &mut recipient_store,
            &resumed_enrollment,
            &recipient_wrap,
            1,
        )
        .is_err()
    );
    let batches_path = format!(
        "/v1/families/{}/batches?after={}",
        lower_hex(&family.family_id),
        6 + shift,
    );
    let auth = resumed_enrollment.sign_get(&batches_path).unwrap();
    let batch_page = LogPage::decode(
        &http_bytes(&app, Method::GET, &batches_path, auth.bytes).await,
        family.family_id,
        6 + shift,
    )
    .unwrap();
    assert_eq!(batch_page.entries.len(), 1);
    assert_eq!(batch_page.entries[0].committed_bytes, batch.envelope_bytes);
    let log_path = format!(
        "/v1/families/{}/log?after={}",
        lower_hex(&family.family_id),
        6 + shift
    );
    let read = resumed.sign_get(&log_path).unwrap();
    let log = LogPage::decode(
        &relay
            .log_page_authenticated(family.family_id, 6 + shift, &log_path, &read.bytes)
            .unwrap(),
        family.family_id,
        6 + shift,
    )
    .unwrap();
    assert_eq!(log.entries.len(), 1);
    let result_path = format!(
        "/v1/families/{}/batch-results/{}",
        lower_hex(&family.family_id),
        lower_hex(&batch.batch_id)
    );
    let read = resumed.sign_get(&result_path).unwrap();
    let result = BatchResult::decode(
        &relay
            .batch_result_authenticated(family.family_id, batch.batch_id, &result_path, &read.bytes)
            .unwrap(),
    )
    .unwrap();
    let mut manager_public = PublicHistorySession::resume(&local, family).unwrap();
    manager_public
        .accept_batch(
            &mut local,
            &log.entries[0].committed_bytes,
            result.receipt_bytes.as_ref().unwrap(),
        )
        .unwrap();
    let manager_ready = resumed.confirm(&mut local, committed).unwrap();
    assert_eq!(manager_ready.observed_cursor(), 7 + shift);
    assert_eq!(
        manager_ready.projection().record(&pre_child_id),
        recipient_ready.projection().record(&pre_child_id)
    );
    assert_eq!(
        manager_ready.projection().record(&child_id),
        recipient_ready.projection().record(&child_id)
    );
    let late_child_id = v7(0x24);
    if early_batch {
        assert_eq!(
            manager_ready.projection().record(&post_child_id),
            recipient_ready.projection().record(&post_child_id)
        );
        local
            .append_local(
                family,
                NewOperation {
                    family_id: family.family_id,
                    operation_id: v7(0x25),
                    record_id: late_child_id,
                    scope: Scope::Child,
                    kind: Kind::Create,
                    author_device_id: family.device_id,
                    hlc: Hlc {
                        wall_ms: 1_700_000_002_000,
                        counter: 0,
                        device_id: family.device_id,
                    },
                    record_type: Some("child".to_owned()),
                    child_id: None,
                    fields: Some(vec![(1, Value::Text("Later".to_owned()))]),
                },
                1_700_000_002_000,
            )
            .unwrap();
    }
    let mut clone = if early_batch {
        let clone_path = dir.path().join("manager-clone.db");
        rusqlite::Connection::open(&local_path)
            .unwrap()
            .execute("VACUUM INTO ?1", [clone_path.to_str().unwrap()])
            .unwrap();
        let mut clone_store = SqliteStore::open(&clone_path).unwrap();
        let clone_manager = ManagerCreation::resume(&clone_store, family, &wrapping_key).unwrap();
        let clone_ready = clone_manager.confirm(&mut clone_store, committed).unwrap();
        let clone_batch = match clone_manager
            .stage_next_local(&clone_ready, &mut clone_store)
            .unwrap()
        {
            NextUpload::Fresh(batch) => batch,
            NextUpload::RetryExact(_) => panic!("clone already had a pending batch"),
        };
        Some((clone_store, clone_manager, clone_batch))
    } else {
        None
    };
    let manager_batch = match resumed
        .stage_next_local(&manager_ready, &mut local)
        .unwrap()
    {
        NextUpload::RetryExact(batch) if !early_batch => batch,
        NextUpload::Fresh(batch) if early_batch => batch,
        _ => panic!("manager outbox stage differs from expected path"),
    };
    assert_eq!(manager_batch.from_index, if early_batch { 3 } else { 2 });
    if !early_batch {
        assert_eq!(
            manager_batch.envelope_bytes,
            staged_manager_batch.envelope_bytes
        );
    }
    let response = http_bytes(
        &app,
        Method::POST,
        &batch_path,
        manager_batch.envelope_bytes.clone(),
    )
    .await;
    let Value::Map(fields) = cbor::decode(&response).unwrap() else {
        panic!()
    };
    let Value::Bytes(manager_receipt) = &fields[1].1 else {
        panic!()
    };
    let mut manager_public = PublicHistorySession::resume(&local, family).unwrap();
    manager_public
        .accept_batch(&mut local, &manager_batch.envelope_bytes, manager_receipt)
        .unwrap();
    recipient_public
        .accept_batch(
            &mut recipient_store,
            &manager_batch.envelope_bytes,
            manager_receipt,
        )
        .unwrap();
    let manager_ready = resumed.confirm(&mut local, committed).unwrap();
    let recipient_ready =
        ReadyFamilySession::from_enrollment(&recipient_store, &resumed_enrollment).unwrap();
    assert_eq!(manager_ready.observed_cursor(), 8 + shift);
    assert_eq!(
        manager_ready.projection().record(&post_child_id),
        recipient_ready.projection().record(&post_child_id)
    );
    if early_batch {
        assert_eq!(
            manager_ready.projection().record(&late_child_id),
            recipient_ready.projection().record(&late_child_id)
        );
    }
    if let Some((ref mut clone_store, ref clone_manager, ref clone_batch)) = clone {
        assert_eq!(clone_batch.sequence, manager_batch.sequence);
        assert_ne!(clone_batch.envelope_bytes, manager_batch.envelope_bytes);
        let rejection = http_bytes(
            &app,
            Method::POST,
            &batch_path,
            clone_batch.envelope_bytes.clone(),
        )
        .await;
        assert_eq!(
            http_bytes(
                &app,
                Method::POST,
                &batch_path,
                clone_batch.envelope_bytes.clone(),
            )
            .await,
            rejection
        );
        let Value::Map(fields) = cbor::decode(&rejection).unwrap() else {
            panic!()
        };
        let Value::Bytes(rejected_receipt) = &fields[1].1 else {
            panic!()
        };
        let result_path = format!(
            "/v1/families/{}/batch-results/{}",
            lower_hex(&family.family_id),
            lower_hex(&clone_batch.batch_id)
        );
        let read = clone_manager.sign_get(&result_path).unwrap();
        let result_bytes = relay
            .batch_result_authenticated(
                family.family_id,
                clone_batch.batch_id,
                &result_path,
                &read.bytes,
            )
            .unwrap();
        let result = BatchResult::decode(&result_bytes).unwrap();
        assert_eq!(
            result.receipt_bytes.as_deref(),
            Some(rejected_receipt.as_slice())
        );
        let mut clone_public = PublicHistorySession::resume(clone_store, family).unwrap();
        assert!(
            clone_public
                .resolve_pending_result(clone_store, &result_bytes)
                .is_err()
        );
        clone_public
            .accept_batch(clone_store, &manager_batch.envelope_bytes, manager_receipt)
            .unwrap();
        let mut forged_result = result_bytes.clone();
        *forged_result.last_mut().unwrap() ^= 1;
        assert!(
            clone_public
                .resolve_pending_result(clone_store, &forged_result)
                .is_err()
        );
        assert_eq!(
            clone_public
                .resolve_pending_result(clone_store, &result_bytes)
                .unwrap(),
            PendingBatchResult::Rebased
        );
        let clone_ready = clone_manager.confirm(clone_store, committed).unwrap();
        let rebatch = match clone_manager
            .stage_next_local(&clone_ready, clone_store)
            .unwrap()
        {
            NextUpload::Fresh(batch) => batch,
            NextUpload::RetryExact(_) => panic!("verified conflict kept obsolete envelope"),
        };
        assert_eq!(rebatch.sequence, manager_batch.sequence + 1);
        assert_ne!(rebatch.batch_id, clone_batch.batch_id);
        let response = http_bytes(
            &app,
            Method::POST,
            &batch_path,
            rebatch.envelope_bytes.clone(),
        )
        .await;
        let Value::Map(fields) = cbor::decode(&response).unwrap() else {
            panic!()
        };
        let Value::Bytes(rebatch_receipt) = &fields[1].1 else {
            panic!()
        };
        clone_public
            .accept_batch(clone_store, &rebatch.envelope_bytes, rebatch_receipt)
            .unwrap();
        let mut manager_public = PublicHistorySession::resume(&local, family).unwrap();
        manager_public
            .accept_batch(&mut local, &rebatch.envelope_bytes, rebatch_receipt)
            .unwrap();
        recipient_public
            .accept_batch(
                &mut recipient_store,
                &rebatch.envelope_bytes,
                rebatch_receipt,
            )
            .unwrap();
        let manager_ready = resumed.confirm(&mut local, committed).unwrap();
        let recipient_ready =
            ReadyFamilySession::from_enrollment(&recipient_store, &resumed_enrollment).unwrap();
        assert_eq!(manager_ready.observed_cursor(), 10);
        assert_eq!(
            manager_ready.projection().record(&late_child_id),
            recipient_ready.projection().record(&late_child_id)
        );
    }
    let recipient_ready =
        ReadyFamilySession::from_enrollment(&recipient_store, &resumed_enrollment).unwrap();
    recipient_ready
        .append_local(
            &mut recipient_store,
            NewOperation {
                family_id: family.family_id,
                operation_id: v7(0x35),
                record_id: pre_child_id,
                scope: Scope::Child,
                kind: Kind::Set,
                author_device_id: enrollment.family().device_id,
                hlc: Hlc {
                    wall_ms: 1_700_000_003_000,
                    counter: 0,
                    device_id: enrollment.family().device_id,
                },
                record_type: None,
                child_id: None,
                fields: Some(vec![(1, Value::Text("Recipient edit".to_owned()))]),
            },
            1_700_000_003_000,
        )
        .unwrap();
    assert_eq!(
        recipient_ready
            .projection_with_pending(&recipient_store)
            .unwrap()
            .record(&pre_child_id)
            .unwrap()
            .field(1)
            .unwrap()
            .value,
        Value::Text("Recipient edit".to_owned())
    );
    let edited = match recipient_ready
        .stage_enrolled_local(&mut recipient_store, &resumed_enrollment)
        .unwrap()
    {
        NextUpload::Fresh(batch) => batch,
        NextUpload::RetryExact(_) => panic!("remote child edit was already staged"),
    };
    let response = http_bytes(
        &app,
        Method::POST,
        &batch_path,
        edited.envelope_bytes.clone(),
    )
    .await;
    let Value::Map(fields) = cbor::decode(&response).unwrap() else {
        panic!()
    };
    let Value::Bytes(receipt) = &fields[1].1 else {
        panic!()
    };
    recipient_public
        .accept_batch(&mut recipient_store, &edited.envelope_bytes, receipt)
        .unwrap();
    let mut manager_public = PublicHistorySession::resume(&local, family).unwrap();
    manager_public
        .accept_batch(&mut local, &edited.envelope_bytes, receipt)
        .unwrap();
    let manager_ready = resumed.confirm(&mut local, committed).unwrap();
    let recipient_ready =
        ReadyFamilySession::from_enrollment(&recipient_store, &resumed_enrollment).unwrap();
    assert_eq!(
        manager_ready.projection().record(&pre_child_id),
        recipient_ready.projection().record(&pre_child_id)
    );
    recipient_ready
        .append_local(
            &mut recipient_store,
            NewOperation {
                family_id: family.family_id,
                operation_id: v7(0x36),
                record_id: pre_child_id,
                scope: Scope::Child,
                kind: Kind::Set,
                author_device_id: enrollment.family().device_id,
                hlc: Hlc {
                    wall_ms: 1_700_000_004_000,
                    counter: 0,
                    device_id: enrollment.family().device_id,
                },
                record_type: None,
                child_id: None,
                fields: Some(vec![(1, Value::Text("Private pending".to_owned()))]),
            },
            1_700_000_004_000,
        )
        .unwrap();
    assert!(
        export_readable_local(&recipient_store, enrollment.family(), 1_700_000_004_001).is_err()
    );
    let backup =
        export_readable_shared(&recipient_store, &recipient_ready, 1_700_000_004_001).unwrap();
    let parsed = parse_readable(&backup).unwrap();
    assert_eq!(
        parsed.source_cursor,
        Some(recipient_ready.observed_cursor())
    );
    let copy =
        private_copy_shared(&mut recipient_store, &recipient_ready, 1_700_000_004_001).unwrap();
    assert_eq!(
        private_copy_shared(&mut recipient_store, &recipient_ready, 1_700_000_004_002).unwrap(),
        copy
    );
    assert_ne!(copy.family_id, family.family_id);
    assert_eq!(
        recipient_store
            .load_local(copy)
            .unwrap()
            .record(&pre_child_id)
            .unwrap()
            .field(1)
            .unwrap()
            .value,
        Value::Text("Private pending".to_owned())
    );
    assert_eq!(
        recipient_store
            .restored_origin(copy)
            .unwrap()
            .unwrap()
            .source_cursor,
        Some(recipient_ready.observed_cursor())
    );
    drop(recipient_store);
    let mut recipient_store = SqliteStore::open(&recipient_path).unwrap();
    assert_eq!(
        private_copy_shared(&mut recipient_store, &recipient_ready, 1_700_000_004_003).unwrap(),
        copy
    );

    let accepted_without_response = if accepted_before_removal {
        let pending = match recipient_ready
            .stage_enrolled_local(&mut recipient_store, &resumed_enrollment)
            .unwrap()
        {
            NextUpload::Fresh(batch) => batch,
            NextUpload::RetryExact(_) => panic!("recipient batch already staged"),
        };
        let response = http_bytes(
            &app,
            Method::POST,
            &batch_path,
            pending.envelope_bytes.clone(),
        )
        .await;
        let Value::Map(fields) = cbor::decode(&response).unwrap() else {
            panic!()
        };
        let Value::Bytes(receipt) = &fields[1].1 else {
            panic!()
        };
        let mut manager_public = PublicHistorySession::resume(&local, family).unwrap();
        manager_public
            .accept_batch(&mut local, &pending.envelope_bytes, receipt)
            .unwrap();
        // The recipient loses the POST response and keeps the exact pending
        // envelope while the manager proceeds to removal.
        Some(pending)
    } else {
        None
    };
    // The relay process can restart after an accepted POST whose response
    // never reached the recipient; its exact result must remain durable.
    drop(app);
    let app = test_router(&relay_path, relay_seed).unwrap();

    if early_batch {
        let current = resumed.ready_session(&local).unwrap();
        current
            .append_local(
                &mut local,
                NewOperation {
                    family_id: family.family_id,
                    operation_id: v7(0x3d),
                    record_id: v7(0x3e),
                    scope: Scope::Child,
                    kind: Kind::Create,
                    author_device_id: family.device_id,
                    hlc: Hlc {
                        wall_ms: 1_700_000_004_200,
                        counter: 0,
                        device_id: family.device_id,
                    },
                    record_type: Some("child".to_owned()),
                    child_id: None,
                    fields: Some(vec![(1, Value::Text("Unseen before removal".to_owned()))]),
                },
                1_700_000_004_200,
            )
            .unwrap();
        let unseen = match resumed.stage_next_local(&current, &mut local).unwrap() {
            NextUpload::Fresh(batch) => batch,
            NextUpload::RetryExact(_) => panic!("unseen edit already staged"),
        };
        let result = http_bytes(
            &app,
            Method::POST,
            &batch_path,
            unseen.envelope_bytes.clone(),
        )
        .await;
        let Value::Map(fields) = cbor::decode(&result).unwrap() else {
            panic!()
        };
        let Value::Bytes(receipt) = &fields[1].1 else {
            panic!()
        };
        let mut manager_public = PublicHistorySession::resume(&local, family).unwrap();
        manager_public
            .accept_batch(&mut local, &unseen.envelope_bytes, receipt)
            .unwrap();
        // The recipient remains offline and cannot fetch this old-epoch
        // batch after removal; its public proof must tolerate the gap.
    }

    let before_removal = resumed.ready_session(&local).unwrap();
    let stale_child = v7(0x3b);
    before_removal
        .append_local(
            &mut local,
            NewOperation {
                family_id: family.family_id,
                operation_id: v7(0x3c),
                record_id: stale_child,
                scope: Scope::Child,
                kind: Kind::Create,
                author_device_id: family.device_id,
                hlc: Hlc {
                    wall_ms: 1_700_000_004_500,
                    counter: 0,
                    device_id: family.device_id,
                },
                record_type: Some("child".to_owned()),
                child_id: None,
                fields: Some(vec![(1, Value::Text("Across rotation".to_owned()))]),
            },
            1_700_000_004_500,
        )
        .unwrap();
    let old_epoch_upload = match resumed
        .stage_next_local(&before_removal, &mut local)
        .unwrap()
    {
        NextUpload::Fresh(batch) => batch,
        NextUpload::RetryExact(_) => panic!("manager batch already pending before removal"),
    };

    let removal = FirstRemoval::prepare(
        &mut local,
        &resumed,
        &wrapping_key,
        enrollment.family().device_id,
    )
    .unwrap();
    let removal_candidate = removal.candidate_bytes().to_vec();
    let removal_stage = removal.stage_bodies().unwrap();
    drop(local);
    let mut local = SqliteStore::open(&local_path).unwrap();
    let resumed = ManagerCreation::resume(&local, family, &wrapping_key).unwrap();
    let removal = FirstRemoval::resume(&local, &resumed, &wrapping_key).unwrap();
    assert_eq!(removal.target_id(), enrollment.family().device_id);
    assert_eq!(removal.candidate_bytes(), removal_candidate);
    assert_eq!(removal.stage_bodies().unwrap(), removal_stage);
    let control_path = format!("/v1/families/{}/control", lower_hex(&family.family_id));
    assert_eq!(
        http_response(&app, Method::POST, &control_path, removal_candidate.clone())
            .await
            .0,
        StatusCode::CONFLICT,
    );
    stage_objects(&app, family.family_id, &removal_stage[..2]).await;
    assert_eq!(
        http_response(&app, Method::POST, &control_path, removal_candidate.clone())
            .await
            .0,
        StatusCode::CONFLICT,
    );
    // Resume a multi-object rotation after the relay process restarts.
    drop(app);
    let app = test_router(&relay_path, relay_seed).unwrap();
    stage_objects(&app, family.family_id, &removal_stage[..2]).await;
    stage_objects(&app, family.family_id, &removal_stage[2..]).await;
    let response = commit_control(&app, family.family_id, &removal_candidate).await;
    assert_eq!(
        response,
        commit_control(&app, family.family_id, &removal_candidate).await
    );
    let Value::Map(fields) = cbor::decode(&response).unwrap() else {
        panic!()
    };
    let Value::Bytes(committed_removal) = &fields[1].1 else {
        panic!()
    };
    removal
        .confirm(&mut local, &resumed, committed_removal)
        .unwrap();
    let manager_after = resumed.ready_session(&local).unwrap();
    assert_eq!(
        manager_after.observed_cursor(),
        recipient_ready.observed_cursor()
            + 1
            + u64::from(early_batch)
            + u64::from(accepted_before_removal)
    );

    let stale = match recipient_ready
        .stage_enrolled_local(&mut recipient_store, &resumed_enrollment)
        .unwrap()
    {
        NextUpload::Fresh(batch) if !accepted_before_removal => batch,
        NextUpload::RetryExact(batch) if accepted_before_removal => batch,
        _ => panic!("recipient batch retry differs from expected path"),
    };
    let rejected = http_bytes(
        &app,
        Method::POST,
        &batch_path,
        stale.envelope_bytes.clone(),
    )
    .await;
    assert_eq!(
        rejected,
        http_bytes(
            &app,
            Method::POST,
            &batch_path,
            stale.envelope_bytes.clone()
        )
        .await
    );
    let result = BatchResult::decode(&rejected).unwrap();
    assert!(result.receipt_bytes.is_some());
    if let Some(accepted) = &accepted_without_response {
        assert_eq!(stale.envelope_bytes, accepted.envelope_bytes);
        assert_eq!(stale.batch_id, accepted.batch_id);
    }
    for (id, expected) in [
        (batch.batch_id, true),
        (stale.batch_id, true),
        (v7(0x78), false),
    ] {
        let path = format!(
            "/v1/families/{}/batch-results/{}",
            lower_hex(&family.family_id),
            lower_hex(&id),
        );
        let auth = resumed_enrollment.sign_get(&path).unwrap();
        let found = BatchResult::decode(&http_bytes(&app, Method::GET, &path, auth.bytes).await)
            .unwrap()
            .receipt_bytes;
        assert_eq!(found.is_some(), expected);
    }
    let other_path = format!(
        "/v1/families/{}/batch-results/{}",
        lower_hex(&family.family_id),
        lower_hex(&manager_batch.batch_id),
    );
    let auth = resumed_enrollment.sign_get(&other_path).unwrap();
    assert_ne!(
        http_response(&app, Method::GET, &other_path, auth.bytes)
            .await
            .0,
        StatusCode::OK
    );
    let new_object_path = format!(
        "/v1/families/{}/objects/{}",
        lower_hex(&family.family_id),
        lower_hex(&removal_stage[0].0),
    );
    let auth = resumed_enrollment.sign_get(&new_object_path).unwrap();
    assert_ne!(
        http_response(&app, Method::GET, &new_object_path, auth.bytes)
            .await
            .0,
        StatusCode::OK
    );

    let proof_path = format!(
        "/v1/families/{}/control?after={}",
        lower_hex(&family.family_id),
        recipient_ready.observed_cursor(),
    );
    let read = resumed_enrollment.sign_get(&proof_path).unwrap();
    let proof_bytes = http_bytes(&app, Method::GET, &proof_path, read.bytes).await;
    let recipient_public =
        PublicHistorySession::resume(&recipient_store, enrollment.family()).unwrap();
    let proof = recipient_public
        .verify_removed_control_page(&proof_bytes)
        .unwrap()
        .unwrap();
    assert_eq!(proof.cursor, manager_after.observed_cursor());
    assert_eq!(proof.source_cursor, recipient_ready.observed_cursor());
    assert_eq!(proof.known_gap, early_batch || accepted_before_removal);
    let unsaved = proof.clone().into();
    assert!(
        recipient_public
            .inspect_removed_pending_result(&recipient_store, &unsaved, &rejected)
            .is_err()
    );
    let saved = recipient_public
        .save_removed_control_page(&mut recipient_store, &proof_bytes)
        .unwrap()
        .unwrap();
    assert_eq!(saved.transition_id, proof.transition_id);
    assert_eq!(
        recipient_public
            .inspect_removed_pending_result(&recipient_store, &saved, &rejected)
            .unwrap(),
        if accepted_before_removal { 2 } else { 3 },
    );
    for (accepted, cursor, head) in [
        (true, saved.cursor, recipient_public.head_hash()),
        (true, saved.cursor + 1, recipient_public.head_hash()),
        (true, saved.cursor - 1, [0x99; 32]),
        (
            false,
            saved.cursor - 1,
            crypto::hash("control-head", &saved.committed_bytes).unwrap(),
        ),
        (false, saved.cursor, [0x99; 32]),
    ] {
        let forged = resign_batch_result(&rejected, &relay_seed, accepted, cursor, head, None);
        assert!(
            recipient_public
                .inspect_removed_pending_result(&recipient_store, &saved, &forged)
                .is_err()
        );
    }
    let wrong_next = if !accepted_before_removal {
        let removal_head = crypto::hash("control-head", &saved.committed_bytes).unwrap();
        let forged = resign_batch_result(
            &rejected,
            &relay_seed,
            false,
            saved.cursor,
            removal_head,
            Some(stale.sequence + 1),
        );
        assert!(
            recipient_public
                .inspect_removed_pending_result(&recipient_store, &saved, &forged)
                .is_err()
        );
        Some(forged)
    } else {
        None
    };
    assert_eq!(
        recipient_store.saved_removal(enrollment.family()).unwrap(),
        Some(saved.clone())
    );
    assert!(
        recipient_ready
            .append_local(
                &mut recipient_store,
                NewOperation {
                    family_id: enrollment.family().family_id,
                    operation_id: v7(0x3f),
                    record_id: v7(0x40),
                    scope: Scope::Child,
                    kind: Kind::Create,
                    author_device_id: enrollment.family().device_id,
                    hlc: Hlc {
                        wall_ms: 1_700_000_004_600,
                        counter: 0,
                        device_id: enrollment.family().device_id
                    },
                    record_type: Some("child".to_owned()),
                    child_id: None,
                    fields: Some(vec![(1, Value::Text("Too late".to_owned()))]),
                },
                1_700_000_004_600,
            )
            .is_err()
    );
    let removed_copy = private_copy_after_removal(
        &mut recipient_store,
        &recipient_ready,
        &saved,
        1_700_000_004_601,
    )
    .unwrap();
    assert_ne!(removed_copy, copy);
    assert_eq!(
        recipient_store
            .load_local(removed_copy)
            .unwrap()
            .record(&pre_child_id)
            .unwrap()
            .field(1)
            .unwrap()
            .value,
        Value::Text("Private pending".to_owned())
    );
    assert_eq!(
        recipient_store
            .restored_origin(removed_copy)
            .unwrap()
            .unwrap()
            .known_gap,
        early_batch || accepted_before_removal
    );
    drop(recipient_store);
    let mut recipient_store = SqliteStore::open(&recipient_path).unwrap();
    assert_eq!(
        recipient_store.saved_removal(enrollment.family()).unwrap(),
        Some(saved.clone())
    );
    let reopened_public =
        PublicHistorySession::resume(&recipient_store, enrollment.family()).unwrap();
    if let Some(forged) = &wrong_next {
        assert!(
            reopened_public
                .inspect_removed_pending_result(&recipient_store, &saved, forged)
                .is_err()
        );
    }
    assert_eq!(
        reopened_public
            .inspect_removed_pending_result(&recipient_store, &saved, &rejected)
            .unwrap(),
        if accepted_before_removal { 2 } else { 3 }
    );
    assert_eq!(
        private_copy_after_removal(
            &mut recipient_store,
            &recipient_ready,
            &saved,
            1_700_000_004_602
        )
        .unwrap(),
        removed_copy
    );
    let page = ControlPage::decode(
        &proof_bytes,
        family.family_id,
        recipient_ready.observed_cursor(),
    )
    .unwrap();
    assert_eq!(page.entries.len(), 1);
    let mut recipient_public =
        PublicHistorySession::resume(&recipient_store, enrollment.family()).unwrap();
    if early_batch || accepted_before_removal {
        assert!(
            recipient_public
                .accept_control(&mut recipient_store, &page.entries[0].committed_bytes)
                .is_err()
        );
    } else {
        recipient_public
            .accept_control(&mut recipient_store, &page.entries[0].committed_bytes)
            .unwrap();
    }
    assert_eq!(
        recipient_public
            .resolve_pending_result(&mut recipient_store, &rejected)
            .unwrap(),
        if accepted_before_removal {
            PendingBatchResult::AcceptedAhead
        } else {
            PendingBatchResult::Blocked
        },
    );
    if !early_batch && !accepted_before_removal {
        assert_eq!(recipient_public.chain().epoch().unwrap(), 2);
        assert!(
            recipient_public
                .chain()
                .active_devices()
                .unwrap()
                .iter()
                .all(|row| row.device_id != enrollment.family().device_id)
        );
        assert!(
            ReadyFamilySession::from_enrollment(&recipient_store, &resumed_enrollment).is_err()
        );
    }
    let denied_path = format!(
        "/v1/families/{}/log?after={}",
        lower_hex(&family.family_id),
        recipient_ready.observed_cursor(),
    );
    let read = resumed_enrollment.sign_get(&denied_path).unwrap();
    assert_ne!(
        http_response(&app, Method::GET, &denied_path, read.bytes)
            .await
            .0,
        StatusCode::OK
    );
    let denied_batches = format!(
        "/v1/families/{}/batches?after={}",
        lower_hex(&family.family_id),
        recipient_ready.observed_cursor(),
    );
    let auth = resumed_enrollment.sign_get(&denied_batches).unwrap();
    assert_eq!(
        http_response(&app, Method::GET, &denied_batches, auth.bytes)
            .await
            .0,
        StatusCode::FORBIDDEN
    );

    let stale_result = http_bytes(
        &app,
        Method::POST,
        &batch_path,
        old_epoch_upload.envelope_bytes.clone(),
    )
    .await;
    let other_rejected_path = format!(
        "/v1/families/{}/batch-results/{}",
        lower_hex(&family.family_id),
        lower_hex(&old_epoch_upload.batch_id),
    );
    let auth = resumed_enrollment.sign_get(&other_rejected_path).unwrap();
    assert_ne!(
        http_response(&app, Method::GET, &other_rejected_path, auth.bytes)
            .await
            .0,
        StatusCode::OK
    );
    let mut manager_public = PublicHistorySession::resume(&local, family).unwrap();
    assert_eq!(
        manager_public
            .resolve_pending_result(&mut local, &stale_result)
            .unwrap(),
        PendingBatchResult::Rebased,
    );
    let rebatch = match resumed
        .stage_next_local(&manager_after, &mut local)
        .unwrap()
    {
        NextUpload::Fresh(batch) => batch,
        NextUpload::RetryExact(_) => panic!("old-epoch manager batch was not rebased"),
    };
    assert_ne!(rebatch.batch_id, old_epoch_upload.batch_id);
    let result = http_bytes(
        &app,
        Method::POST,
        &batch_path,
        rebatch.envelope_bytes.clone(),
    )
    .await;
    let Value::Map(fields) = cbor::decode(&result).unwrap() else {
        panic!()
    };
    let Value::Bytes(receipt) = &fields[1].1 else {
        panic!()
    };
    manager_public
        .accept_batch(&mut local, &rebatch.envelope_bytes, receipt)
        .unwrap();
    let manager_after = resumed.ready_session(&local).unwrap();
    assert!(manager_after.projection().record(&stale_child).is_some());

    let new_child = v7(0x39);
    manager_after
        .append_local(
            &mut local,
            NewOperation {
                family_id: family.family_id,
                operation_id: v7(0x3a),
                record_id: new_child,
                scope: Scope::Child,
                kind: Kind::Create,
                author_device_id: family.device_id,
                hlc: Hlc {
                    wall_ms: 1_700_000_005_000,
                    counter: 0,
                    device_id: family.device_id,
                },
                record_type: Some("child".to_owned()),
                child_id: None,
                fields: Some(vec![(1, Value::Text("After removal".to_owned()))]),
            },
            1_700_000_005_000,
        )
        .unwrap();
    let upload = match resumed
        .stage_next_local(&manager_after, &mut local)
        .unwrap()
    {
        NextUpload::Fresh(batch) => batch,
        NextUpload::RetryExact(_) => panic!("unexpected stale manager upload"),
    };
    let response = http_bytes(
        &app,
        Method::POST,
        &batch_path,
        upload.envelope_bytes.clone(),
    )
    .await;
    let Value::Map(fields) = cbor::decode(&response).unwrap() else {
        panic!()
    };
    let Value::Bytes(receipt) = &fields[1].1 else {
        panic!()
    };
    let mut manager_public = PublicHistorySession::resume(&local, family).unwrap();
    manager_public
        .accept_batch(&mut local, &upload.envelope_bytes, receipt)
        .unwrap();
    assert!(
        resumed
            .ready_session(&local)
            .unwrap()
            .projection()
            .record(&new_child)
            .is_some()
    );
    if !early_batch && !accepted_before_removal {
        let later =
            LaterInviteIssue::prepare_for_initial_manager(&mut local, &resumed, &wrapping_key, 2)
                .unwrap();
        drop(local);
        let mut local = SqliteStore::open(&local_path).unwrap();
        let later =
            LaterInviteIssue::resume(&local, family, later.invitation_id(), &wrapping_key).unwrap();
        stage_objects(
            &app,
            family.family_id,
            &[(later.object_id(), later.stage_body().unwrap())],
        )
        .await;
        let response = commit_control(&app, family.family_id, later.candidate_bytes()).await;
        let Value::Map(fields) = cbor::decode(&response).unwrap() else {
            panic!()
        };
        let Value::Bytes(committed) = &fields[1].1 else {
            panic!()
        };
        let link = later
            .confirm(&mut local, committed, "http://localhost:3400")
            .unwrap();
        assert_eq!(link.fixed_role(), 2);
        assert_eq!(resumed.ready_session(&local).unwrap().active_epoch(), 2);
    }
}
