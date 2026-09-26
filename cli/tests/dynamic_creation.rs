//! Fresh local manager keys and exact genesis bytes survive restart before
//! the first network request.

use babytrack_core::{
    bootstrap::InvitationBootstrap,
    cbor::{self, Value},
    creation::ManagerCreation,
    crypto,
    enrollment::EnrollmentAttempt,
    issue::FirstInviteIssue,
    shared_history::PublicHistorySession,
    sqlite_store::{FamilyHandle, SqliteStore},
    sync_wire::ControlPage,
};
use babytrack_server::RelayStore;

fn lower_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
fn v4(tag: u8) -> [u8; 16] {
    let mut id = [tag; 16];
    id[6] = 0x40;
    id[8] = 0x80;
    id
}

#[test]
fn fresh_empty_family_promotes_with_durable_local_keys() {
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
    let creation =
        ManagerCreation::prepare(&mut local, family, relay_public, &wrapping_key).unwrap();
    let candidate = creation.candidate_bytes().to_vec();
    let stage = creation.stage_body().unwrap();
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

    let mut relay = RelayStore::open(&relay_path, relay_seed).unwrap();
    relay
        .stage_genesis_object(family.family_id, object_id, &stage)
        .unwrap();
    assert!(
        relay
            .committed_object(family.family_id, object_id)
            .unwrap()
            .is_none()
    );
    let response = relay
        .commit_genesis(family.family_id, &candidate, 1_700_000_000_000)
        .unwrap();
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
    relay
        .stage_first_issue_object(family.family_id, issue.object_id(), &issue_stage)
        .unwrap();
    let response = relay
        .commit_first_issue(family.family_id, &issue_candidate, 1_700_000_000_001)
        .unwrap();
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
    let response = relay
        .commit_first_claim(family.family_id, &claim_candidate, 1_700_000_000_002)
        .unwrap();
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
}
