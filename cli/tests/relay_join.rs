//! An independent recipient database consumes the relay's committed join.

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Method, Request, StatusCode, header::CONTENT_TYPE},
};
use babytrack_core::{
    cbor::{self, Value},
    control_chain::ControlChain,
    crypto,
    operation::{NewOperation, Operation},
    shared_history::PublicHistorySession,
    shared_ready::{NextUpload, ReadyFamilySession},
    sqlite_store::{FamilyHandle, SqliteStore},
    sync_wire::{BatchResult, ControlPage, LogPage, OpaqueObject, sign_get},
};
use babytrack_server::{RelayStore, test_router};
use serde_json::Value as Json;
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

fn hex(s: &str) -> Vec<u8> {
    s.as_bytes()
        .chunks_exact(2)
        .map(|p| u8::from_str_radix(std::str::from_utf8(p).unwrap(), 16).unwrap())
        .collect()
}
fn fixed<const N: usize>(s: &str) -> [u8; N] {
    hex(s).try_into().unwrap()
}
fn lower_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
fn candidate(t: &Json) -> Vec<u8> {
    cbor::encode(&Value::Map(vec![
        (
            1,
            cbor::decode(&hex(t["unsigned_cbor_hex"].as_str().unwrap())).unwrap(),
        ),
        (
            2,
            cbor::decode(&hex(t["signatures_cbor_hex"].as_str().unwrap())).unwrap(),
        ),
    ]))
    .unwrap()
}
fn commit_time(t: &Json) -> i64 {
    let Value::Map(root) = cbor::decode(&hex(t["committed_cbor_hex"].as_str().unwrap())).unwrap()
    else {
        panic!()
    };
    let Value::Array(receipt) = &root[2].1 else {
        panic!()
    };
    let Value::Integer(ms) = receipt[4] else {
        panic!()
    };
    ms.try_into().unwrap()
}
fn stage(relay: &mut RelayStore, fixture: &Json, index: usize, family: [u8; 16]) {
    let t = &fixture["transitions"][index];
    let Value::Map(parts) = cbor::decode(&candidate(t)).unwrap() else {
        panic!()
    };
    for item in t["manifest"].as_array().unwrap() {
        let id_hex = item[1].as_str().unwrap();
        let id = fixed::<16>(id_hex);
        let body = cbor::encode(&Value::Map(vec![
            (1, Value::Integer(1)),
            (2, parts[0].1.clone()),
            (3, parts[1].1.clone()),
            (4, Value::Integer(item[0].as_u64().unwrap().into())),
            (5, Value::Bytes(id.to_vec())),
            (
                6,
                Value::Bytes(hex(fixture["objects_by_id_hex"][id_hex].as_str().unwrap())),
            ),
        ]))
        .unwrap();
        match index {
            0 => relay.stage_genesis_object(family, id, &body).unwrap(),
            1 => relay.stage_first_issue_object(family, id, &body).unwrap(),
            3 => relay
                .stage_first_challenge_object(family, id, &body)
                .unwrap(),
            5 => relay
                .stage_first_admission_object(family, id, &body)
                .unwrap(),
            _ => panic!("unexpected staged object"),
        };
    }
}
fn commit(relay: &mut RelayStore, fixture: &Json, index: usize, family: [u8; 16]) {
    let t = &fixture["transitions"][index];
    let body = candidate(t);
    let ms = commit_time(t);
    let response = match index {
        0 => relay.commit_genesis(family, &body, ms).unwrap(),
        1 => relay.commit_first_issue(family, &body, ms).unwrap(),
        2 => relay.commit_first_claim(family, &body, ms).unwrap(),
        3 => relay.commit_first_challenge(family, &body, ms).unwrap(),
        4 => relay.commit_first_proof(family, &body, ms).unwrap(),
        5 => relay.commit_first_admission(family, &body, ms).unwrap(),
        _ => panic!("unexpected transition"),
    };
    let Value::Map(result) = cbor::decode(&response).unwrap() else {
        panic!()
    };
    assert_eq!(
        result[1].1,
        Value::Bytes(hex(t["committed_cbor_hex"].as_str().unwrap()))
    );
}

#[tokio::test]
async fn separate_recipient_store_fetches_and_opens_relay_grant_after_restart() {
    let fixture: Json =
        serde_json::from_str(include_str!("../../tests/vectors/contiguous-chain-v1.json")).unwrap();
    let inputs = &fixture["test_only_inputs"];
    let family = fixed::<16>(inputs["family_id_hex"].as_str().unwrap());
    let recipient = fixed::<16>(inputs["recipient_device_id_hex"].as_str().unwrap());
    let signing_seed = fixed::<32>(inputs["recipient_sign_seed_hex"].as_str().unwrap());
    let agreement_private = fixed::<32>(inputs["recipient_agreement_seed_hex"].as_str().unwrap());
    let relay_seed = fixed::<32>(inputs["relay_sign_seed_hex"].as_str().unwrap());
    let temp = tempfile::tempdir().unwrap();
    let relay_path = temp.path().join("relay.db");
    let recipient_path = temp.path().join("recipient.db");
    let mut relay = RelayStore::open(&relay_path, relay_seed).unwrap();
    for index in 0..6 {
        stage(&mut relay, &fixture, index, family);
        commit(&mut relay, &fixture, index, family);
    }
    drop(relay);
    let mut relay = RelayStore::open(&relay_path, relay_seed).unwrap();
    let genesis = hex(fixture["transitions"][0]["committed_cbor_hex"]
        .as_str()
        .unwrap());
    let relay_public = crypto::signing_public_key(&relay_seed);
    let relay_id = ControlChain::from_genesis(&genesis, relay_public)
        .unwrap()
        .relay_id();
    let path = format!("/v1/families/{}/control?after=0", lower_hex(&family));
    let auth = sign_get(family, relay_id, recipient, &signing_seed, &path).unwrap();
    let page = ControlPage::decode(
        &relay
            .control_page_authenticated(family, 0, &path, &auth.bytes)
            .unwrap(),
        family,
        0,
    )
    .unwrap();
    assert_eq!(page.entries.len(), 6);
    assert_eq!(page.next_after, 6);
    let handle = FamilyHandle {
        family_id: family,
        device_id: recipient,
    };
    let mut local = SqliteStore::open(&recipient_path).unwrap();
    local.create_family(family, recipient).unwrap();
    let mut public = PublicHistorySession::begin(
        &mut local,
        handle,
        &page.entries[0].committed_bytes,
        relay_public,
    )
    .unwrap();
    for entry in page.entries.iter().skip(1) {
        public
            .accept_control(&mut local, &entry.committed_bytes)
            .unwrap();
        assert_eq!(public.cursor(), entry.cursor);
    }
    assert!(ReadyFamilySession::from_admission_grant(&local, handle, agreement_private).is_err());
    for transition in fixture["transitions"].as_array().unwrap().iter().take(6) {
        for item in transition["manifest"].as_array().unwrap() {
            let id = fixed::<16>(item[1].as_str().unwrap());
            let path = format!(
                "/v1/families/{}/objects/{}",
                lower_hex(&family),
                lower_hex(&id)
            );
            let auth = sign_get(family, relay_id, recipient, &signing_seed, &path).unwrap();
            let response = relay
                .object_authenticated(family, id, &path, &auth.bytes)
                .unwrap();
            let object = OpaqueObject::decode(&response, id).unwrap();
            assert_eq!(u64::from(object.kind), item[0].as_u64().unwrap());
            public
                .accept_object(&mut local, id, &object.object_bytes)
                .unwrap();
        }
    }
    assert!(ReadyFamilySession::from_admission_grant(&local, handle, [0; 32]).is_err());
    let ready =
        ReadyFamilySession::from_admission_grant(&local, handle, agreement_private).unwrap();
    assert_eq!(ready.observed_cursor(), 6);
    assert_eq!(ready.active_epoch(), 1);
    let fixture_operation = Operation::decode_bound(
        &hex(fixture["batch"]["operation_cbor_hex"].as_str().unwrap()),
        &family,
        &recipient,
    )
    .unwrap();
    let appended = local
        .append_local(
            handle,
            NewOperation {
                family_id: family,
                operation_id: fixture_operation.operation_id,
                record_id: fixture_operation.record_id,
                scope: fixture_operation.scope,
                kind: fixture_operation.kind,
                author_device_id: recipient,
                hlc: fixture_operation.hlc.clone(),
                record_type: fixture_operation.record_type.clone(),
                child_id: fixture_operation.child_id,
                fields: Some(fixture_operation.fields.clone()),
            },
            fixture_operation.hlc.wall_ms,
        )
        .unwrap();
    assert!(!appended.bytes.is_empty());
    let prepared = match ready.stage_next_local(&mut local, &signing_seed).unwrap() {
        NextUpload::Fresh(batch) => batch,
        NextUpload::RetryExact(_) => panic!("first batch unexpectedly pending"),
    };
    let retry = match ready.stage_next_local(&mut local, &signing_seed).unwrap() {
        NextUpload::RetryExact(batch) => batch,
        NextUpload::Fresh(_) => panic!("uncertain batch was replaced"),
    };
    assert_eq!(retry.envelope_bytes, prepared.envelope_bytes);
    let app = test_router(&relay_path, relay_seed).unwrap();
    let batch_path = format!("/v1/families/{}/batches", lower_hex(&family));
    let response = http_bytes(
        &app,
        Method::POST,
        &batch_path,
        prepared.envelope_bytes.clone(),
    )
    .await;
    assert_eq!(
        http_bytes(
            &app,
            Method::POST,
            &batch_path,
            prepared.envelope_bytes.clone()
        )
        .await,
        response
    );
    let mut tampered = prepared.envelope_bytes.clone();
    *tampered.last_mut().unwrap() ^= 1;
    let bad = Request::post(&batch_path)
        .header(CONTENT_TYPE, "application/cbor")
        .body(Body::from(tampered))
        .unwrap();
    assert_eq!(
        app.clone().oneshot(bad).await.unwrap().status(),
        StatusCode::CONFLICT
    );
    let Value::Map(response_fields) = cbor::decode(&response).unwrap() else {
        panic!("batch response not map");
    };
    let Value::Bytes(receipt_bytes) = &response_fields[1].1 else {
        panic!("batch receipt absent");
    };
    let result_path = format!(
        "/v1/families/{}/batch-results/{}",
        lower_hex(&family),
        lower_hex(&prepared.batch_id)
    );
    let result_auth = sign_get(family, relay_id, recipient, &signing_seed, &result_path).unwrap();
    let result =
        BatchResult::decode(&http_bytes(&app, Method::GET, &result_path, result_auth.bytes).await)
            .unwrap();
    assert_eq!(
        result.receipt_bytes.as_deref(),
        Some(receipt_bytes.as_slice())
    );
    public
        .accept_batch(&mut local, &prepared.envelope_bytes, receipt_bytes)
        .unwrap();
    let ready =
        ReadyFamilySession::from_admission_grant(&local, handle, agreement_private).unwrap();
    assert_eq!(ready.observed_cursor(), 7);
    assert!(
        ready
            .projection()
            .record(&fixture_operation.record_id)
            .is_some()
    );
    drop(local);
    let reopened = SqliteStore::open(&recipient_path).unwrap();
    assert_eq!(
        ReadyFamilySession::from_admission_grant(&reopened, handle, agreement_private)
            .unwrap()
            .observed_cursor(),
        7
    );

    // A second device independently fetches the same control, objects, and
    // encrypted batch. Its local projection converges after a relay restart.
    drop(relay);
    let mut relay = RelayStore::open(&relay_path, relay_seed).unwrap();
    let manager = fixed::<16>(inputs["manager_device_id_hex"].as_str().unwrap());
    let manager_sign = fixed::<32>(inputs["manager_sign_seed_hex"].as_str().unwrap());
    let manager_agree = fixed::<32>(inputs["manager_agreement_seed_hex"].as_str().unwrap());
    let epoch_key = fixed::<32>(inputs["epoch_1_key_hex"].as_str().unwrap());
    let manager_handle = FamilyHandle {
        family_id: family,
        device_id: manager,
    };
    let manager_path = temp.path().join("manager.db");
    let mut manager_store = SqliteStore::open(&manager_path).unwrap();
    manager_store.create_family(family, manager).unwrap();
    let path = format!("/v1/families/{}/control?after=0", lower_hex(&family));
    let auth = sign_get(family, relay_id, manager, &manager_sign, &path).unwrap();
    let page = ControlPage::decode(
        &relay
            .control_page_authenticated(family, 0, &path, &auth.bytes)
            .unwrap(),
        family,
        0,
    )
    .unwrap();
    let mut manager_public = PublicHistorySession::begin(
        &mut manager_store,
        manager_handle,
        &page.entries[0].committed_bytes,
        relay_public,
    )
    .unwrap();
    for entry in page.entries.iter().skip(1) {
        manager_public
            .accept_control(&mut manager_store, &entry.committed_bytes)
            .unwrap();
    }
    for transition in fixture["transitions"].as_array().unwrap().iter().take(6) {
        for item in transition["manifest"].as_array().unwrap() {
            let id = fixed::<16>(item[1].as_str().unwrap());
            let path = format!(
                "/v1/families/{}/objects/{}",
                lower_hex(&family),
                lower_hex(&id)
            );
            let auth = sign_get(family, relay_id, manager, &manager_sign, &path).unwrap();
            let object = OpaqueObject::decode(
                &relay
                    .object_authenticated(family, id, &path, &auth.bytes)
                    .unwrap(),
                id,
            )
            .unwrap();
            manager_public
                .accept_object(&mut manager_store, id, &object.object_bytes)
                .unwrap();
        }
    }
    let log_path = format!("/v1/families/{}/log?after=6", lower_hex(&family));
    let auth = sign_get(family, relay_id, manager, &manager_sign, &log_path).unwrap();
    let log = LogPage::decode(
        &http_bytes(&app, Method::GET, &log_path, auth.bytes).await,
        family,
        6,
    )
    .unwrap();
    assert_eq!(log.entries.len(), 1);
    assert_eq!(log.entries[0].kind, 2);
    assert_eq!(log.entries[0].committed_bytes, prepared.envelope_bytes);
    let result_auth = sign_get(family, relay_id, manager, &manager_sign, &result_path).unwrap();
    let manager_receipt =
        BatchResult::decode(&http_bytes(&app, Method::GET, &result_path, result_auth.bytes).await)
            .unwrap()
            .receipt_bytes
            .unwrap();
    manager_public
        .accept_batch(
            &mut manager_store,
            &log.entries[0].committed_bytes,
            &manager_receipt,
        )
        .unwrap();
    let manager_ready =
        ReadyFamilySession::from_store(&manager_store, manager_handle, epoch_key, manager_agree)
            .unwrap();
    assert_eq!(manager_ready.observed_cursor(), 7);
    assert!(
        manager_ready
            .projection()
            .record(&fixture_operation.record_id)
            .is_some()
    );
    assert_eq!(
        ready.projection().record(&fixture_operation.record_id),
        manager_ready
            .projection()
            .record(&fixture_operation.record_id),
    );
}
