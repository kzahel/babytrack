//! Fresh keys, promoted history, and exact signed bytes cross the relay HTTP boundary.

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Method, Request, StatusCode, header::CONTENT_TYPE},
};

use babytrack_core::{
    bootstrap::InvitationBootstrap,
    cbor::{self, Value},
    creation::ManagerCreation,
    crypto,
    enrollment::EnrollmentAttempt,
    first_admission::FirstAdmission,
    first_challenge::FirstChallenge,
    first_proof::FirstProof,
    issue::FirstInviteIssue,
    operation::{Hlc, Kind, NewOperation, Scope},
    shared_history::PublicHistorySession,
    shared_ready::{NextUpload, ReadyFamilySession},
    sqlite_store::{FamilyHandle, SqliteStore},
    sync_wire::{BatchResult, ControlPage, LogPage, OpaqueObject},
};
use babytrack_server::{RelayStore, test_router};
use tower::ServiceExt;

async fn http_bytes(app: &Router, method: Method, path: &str, body: Vec<u8>) -> Vec<u8> {
    let request = Request::builder()
        .method(method)
        .uri(path)
        .header(CONTENT_TYPE, "application/cbor")
        .body(Body::from(body))
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    to_bytes(response.into_body(), 4 * 1024 * 1024)
        .await
        .unwrap()
        .to_vec()
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

#[tokio::test]
async fn existing_local_family_promotes_and_shares_with_durable_keys() {
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
    let path = format!(
        "/v1/families/{}/control?after=0",
        lower_hex(&family.family_id)
    );
    let auth = received_link.sign_get(&path).unwrap();
    let page = ControlPage::decode(
        &relay
            .control_page_authenticated(family.family_id, 0, &path, &auth.bytes)
            .unwrap(),
        family.family_id,
        0,
    )
    .unwrap();
    assert_eq!(page.entries.len(), 2);
    let recipient_path = dir.path().join("recipient.db");
    let recipient_wrap = [0x8b; 32];
    let mut recipient_store = SqliteStore::open(&recipient_path).unwrap();
    let enrollment = EnrollmentAttempt::prepare(
        &mut recipient_store,
        &received_link,
        &page.entries[0].committed_bytes,
        &page.entries[1].committed_bytes,
        &recipient_wrap,
    )
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
    public
        .accept_control(&mut recipient_store, committed_claim)
        .unwrap();
    assert_eq!(public.cursor(), 3);
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
        "/v1/families/{}/control?after=3",
        lower_hex(&family.family_id)
    );
    let read = resumed_enrollment.sign_get(&pending_path).unwrap();
    let page = ControlPage::decode(
        &relay
            .control_page_authenticated(family.family_id, 3, &pending_path, &read.bytes)
            .unwrap(),
        family.family_id,
        3,
    )
    .unwrap();
    assert_eq!(page.entries.len(), 1);
    public
        .accept_control(&mut recipient_store, &page.entries[0].committed_bytes)
        .unwrap();
    let verified = public
        .chain()
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
        &relay
            .object_authenticated(family.family_id, hpke_id, &object_path, &read.bytes)
            .unwrap(),
        hpke_id,
    )
    .unwrap();
    public
        .accept_object(&mut recipient_store, hpke_id, &hpke_object.object_bytes)
        .unwrap();
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
        "/v1/families/{}/control?after=4",
        lower_hex(&family.family_id)
    );
    let read = resumed.sign_get(&proof_path).unwrap();
    let page = ControlPage::decode(
        &relay
            .control_page_authenticated(family.family_id, 4, &proof_path, &read.bytes)
            .unwrap(),
        family.family_id,
        4,
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
        "/v1/families/{}/control?after=5",
        lower_hex(&family.family_id)
    );
    let read = resumed_enrollment.sign_get(&admission_path).unwrap();
    let page = ControlPage::decode(
        &relay
            .control_page_authenticated(family.family_id, 5, &admission_path, &read.bytes)
            .unwrap(),
        family.family_id,
        5,
    )
    .unwrap();
    assert_eq!(page.entries.len(), 1);
    recipient_public
        .accept_control(&mut recipient_store, &page.entries[0].committed_bytes)
        .unwrap();
    assert!(ReadyFamilySession::from_enrollment(&recipient_store, &resumed_enrollment).is_err());
    let chunk_id = genesis_stages[1].0;
    let mut object_ids = vec![object_id, issue.object_id()];
    object_ids.extend(staged.iter().map(|(id, _)| *id));
    object_ids.extend(admission_stage.iter().map(|(id, _)| *id));
    object_ids.push(chunk_id);
    for id in object_ids {
        if id == chunk_id {
            assert!(
                ReadyFamilySession::from_enrollment(&recipient_store, &resumed_enrollment).is_err()
            );
        }
        let path = format!(
            "/v1/families/{}/objects/{}",
            lower_hex(&family.family_id),
            lower_hex(&id)
        );
        let read = resumed_enrollment.sign_get(&path).unwrap();
        let object = OpaqueObject::decode(
            &relay
                .object_authenticated(family.family_id, id, &path, &read.bytes)
                .unwrap(),
            id,
        )
        .unwrap();
        recipient_public
            .accept_object(&mut recipient_store, id, &object.object_bytes)
            .unwrap();
    }
    let ready = ReadyFamilySession::from_enrollment(&recipient_store, &resumed_enrollment).unwrap();
    assert_eq!(ready.observed_cursor(), 6);
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
    assert_eq!(recipient_ready.observed_cursor(), 7);
    let log_path = format!("/v1/families/{}/log?after=6", lower_hex(&family.family_id));
    let read = resumed.sign_get(&log_path).unwrap();
    let log = LogPage::decode(
        &relay
            .log_page_authenticated(family.family_id, 6, &log_path, &read.bytes)
            .unwrap(),
        family.family_id,
        6,
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
    assert_eq!(manager_ready.observed_cursor(), 7);
    assert_eq!(
        manager_ready.projection().record(&pre_child_id),
        recipient_ready.projection().record(&pre_child_id)
    );
    assert_eq!(
        manager_ready.projection().record(&child_id),
        recipient_ready.projection().record(&child_id)
    );
    let manager_batch = match resumed
        .stage_next_local(&manager_ready, &mut local)
        .unwrap()
    {
        NextUpload::Fresh(batch) => batch,
        NextUpload::RetryExact(_) => panic!("post-watermark record was already pending"),
    };
    assert_eq!(manager_batch.from_index, 2);
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
    assert_eq!(manager_ready.observed_cursor(), 8);
    assert_eq!(
        manager_ready.projection().record(&post_child_id),
        recipient_ready.projection().record(&post_child_id)
    );
}
