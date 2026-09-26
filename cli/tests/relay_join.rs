//! An independent recipient database consumes the relay's committed join.

use babytrack_core::{
    cbor::{self, Value},
    control_chain::ControlChain,
    crypto,
    shared_history::PublicHistorySession,
    shared_ready::ReadyFamilySession,
    sqlite_store::{FamilyHandle, SqliteStore},
    sync_wire::{ControlPage, OpaqueObject, sign_get},
};
use babytrack_server::RelayStore;
use serde_json::Value as Json;

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

#[test]
fn separate_recipient_store_fetches_and_opens_relay_grant_after_restart() {
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
    drop(local);
    let reopened = SqliteStore::open(&recipient_path).unwrap();
    assert_eq!(
        ReadyFamilySession::from_admission_grant(&reopened, handle, agreement_private)
            .unwrap()
            .observed_cursor(),
        6
    );
}
