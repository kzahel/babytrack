//! Test-only native holder for a browser-created enrollment over one relay.
//! The browser uses HTTP; this driver uses the same RelayStore validation
//! methods directly so the two clients remain independently persisted.

use std::{
    env,
    time::{SystemTime, UNIX_EPOCH},
};

use babytrack_core::{
    batch::Header,
    cbor::{self, Value},
    creation::ManagerCreation,
    crypto,
    first_admission::FirstAdmission,
    first_challenge::FirstChallenge,
    issue::FirstInviteIssue,
    operation::{Hlc, Kind, NewOperation, Operation, Scope},
    shared_history::PublicHistorySession,
    shared_ready::NextUpload,
    sqlite_store::{FamilyHandle, SqliteStore},
    sync_wire::{BatchResult, ControlPage, LogPage},
};
use babytrack_server::RelayStore;

const RELAY_SEED: [u8; 32] = [0x6e; 32];
const WRAPPING_KEY: [u8; 32] = [0x4c; 32];

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

fn family() -> FamilyHandle {
    FamilyHandle {
        family_id: v4(0x91),
        device_id: v4(0x92),
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn parse_id(value: &str) -> [u8; 16] {
    assert_eq!(value.len(), 32);
    let bytes: Vec<u8> = value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect();
    bytes.try_into().unwrap()
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis()
        .try_into()
        .unwrap()
}

fn committed(response: &[u8]) -> Vec<u8> {
    let Value::Map(fields) = cbor::decode(response).unwrap() else {
        panic!("result not map")
    };
    let Value::Bytes(bytes) = &fields[1].1 else {
        panic!("result has no control")
    };
    bytes.clone()
}

fn pull_new_controls(local: &mut SqliteStore, relay: &mut RelayStore, manager: &ManagerCreation) {
    let family = manager.family();
    for _ in 0..8 {
        let after = PublicHistorySession::resume(local, family)
            .unwrap()
            .cursor();
        let path = format!(
            "/v1/families/{}/control?after={after}",
            hex(&family.family_id)
        );
        let auth = manager.sign_get(&path).unwrap();
        let bytes = relay
            .control_page_authenticated(family.family_id, after, &path, &auth.bytes)
            .unwrap();
        let page = ControlPage::decode(&bytes, family.family_id, after).unwrap();
        for entry in &page.entries {
            PublicHistorySession::resume(local, family)
                .unwrap()
                .accept_control(local, &entry.committed_bytes)
                .unwrap();
        }
        if !page.has_more {
            return;
        }
        assert!(
            !page.entries.is_empty(),
            "relay control page made no progress"
        );
    }
    panic!("holder control pull exceeded page budget");
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    assert!(
        args.len() >= 3,
        "expected mode, manager DB, relay DB, [browser origin]"
    );
    let mode = args[0].as_str();
    if mode == "recipient_operation" {
        let family_id = parse_id(&args[1]);
        let device_id = parse_id(&args[2]);
        let operation = Operation::encode_new(&NewOperation {
            family_id,
            operation_id: v7(0xb2),
            record_id: v7(0xb1),
            scope: Scope::Child,
            kind: Kind::Create,
            author_device_id: device_id,
            hlc: Hlc {
                wall_ms: now_ms(),
                counter: 0,
                device_id,
            },
            record_type: Some("child".to_owned()),
            child_id: None,
            fields: Some(vec![(1, Value::Text("BrowserRecipientChild".to_owned()))]),
        })
        .unwrap();
        println!("{}", hex(&operation));
        return;
    }
    let mut local = SqliteStore::open(&args[1]).unwrap();
    let mut relay = RelayStore::open(&args[2], RELAY_SEED).unwrap();
    let family = family();
    match mode {
        "setup" => {
            let origin = args.get(3).expect("browser origin required");
            local
                .create_family(family.family_id, family.device_id)
                .unwrap();
            let manager = ManagerCreation::prepare(
                &mut local,
                family,
                crypto::signing_public_key(&RELAY_SEED),
                &WRAPPING_KEY,
            )
            .unwrap();
            for (id, body) in manager.stage_bodies().unwrap() {
                relay
                    .stage_genesis_object(family.family_id, id, &body)
                    .unwrap();
            }
            let genesis = committed(
                &relay
                    .commit_genesis(family.family_id, manager.candidate_bytes(), now_ms())
                    .unwrap(),
            );
            manager.confirm(&mut local, &genesis).unwrap();
            let issue = FirstInviteIssue::prepare(&mut local, &manager, &WRAPPING_KEY, 1).unwrap();
            relay
                .stage_first_issue_object(
                    family.family_id,
                    issue.object_id(),
                    &issue.stage_body().unwrap(),
                )
                .unwrap();
            let committed_issue = committed(
                &relay
                    .commit_first_issue(family.family_id, issue.candidate_bytes(), now_ms())
                    .unwrap(),
            );
            let link = issue
                .confirm(&mut local, &manager, &committed_issue, origin)
                .unwrap();
            println!("{}", link.to_fragment().unwrap());
        }
        "challenge" => {
            let manager = ManagerCreation::resume(&local, family, &WRAPPING_KEY).unwrap();
            pull_new_controls(&mut local, &mut relay, &manager);
            let challenge =
                FirstChallenge::prepare_for_only_pending(&mut local, &manager, &WRAPPING_KEY)
                    .unwrap();
            for (id, body) in challenge.stage_bodies().unwrap() {
                relay
                    .stage_first_challenge_object(family.family_id, id, &body)
                    .unwrap();
            }
            let committed_challenge = committed(
                &relay
                    .commit_first_challenge(family.family_id, challenge.candidate_bytes(), now_ms())
                    .unwrap(),
            );
            challenge.confirm(&mut local, &committed_challenge).unwrap();
            println!("challenge committed");
        }
        "grant" => {
            let manager = ManagerCreation::resume(&local, family, &WRAPPING_KEY).unwrap();
            pull_new_controls(&mut local, &mut relay, &manager);
            let admission =
                FirstAdmission::prepare_for_only_proved(&mut local, &manager, &WRAPPING_KEY)
                    .unwrap();
            for (id, body) in admission.stage_bodies().unwrap() {
                relay
                    .stage_first_admission_object(family.family_id, id, &body)
                    .unwrap();
            }
            let committed_admission = committed(
                &relay
                    .commit_first_admission(family.family_id, admission.candidate_bytes(), now_ms())
                    .unwrap(),
            );
            admission
                .confirm(&mut local, &manager, &committed_admission)
                .unwrap();
            println!("grant committed");
        }
        "write" => {
            let manager = ManagerCreation::resume(&local, family, &WRAPPING_KEY).unwrap();
            let ready = manager.ready_session(&local).unwrap();
            let child = v7(0xa1);
            ready
                .append_local(
                    &mut local,
                    NewOperation {
                        family_id: family.family_id,
                        operation_id: v7(0xa2),
                        record_id: child,
                        scope: Scope::Child,
                        kind: Kind::Create,
                        author_device_id: family.device_id,
                        hlc: Hlc {
                            wall_ms: now_ms(),
                            counter: 0,
                            device_id: family.device_id,
                        },
                        record_type: Some("child".to_owned()),
                        child_id: None,
                        fields: Some(vec![(1, Value::Text("DynamicHolderChild".to_owned()))]),
                    },
                    now_ms(),
                )
                .unwrap();
            let upload = match manager.stage_next_local(&ready, &mut local).unwrap() {
                NextUpload::Fresh(batch) | NextUpload::RetryExact(batch) => batch,
            };
            let response = relay
                .commit_batch(family.family_id, &upload.envelope_bytes)
                .unwrap();
            let receipt = committed(&response);
            PublicHistorySession::resume(&local, family)
                .unwrap()
                .accept_batch(&mut local, &upload.envelope_bytes, &receipt)
                .unwrap();
            println!("{}", hex(&child));
        }
        "read" => {
            let manager = ManagerCreation::resume(&local, family, &WRAPPING_KEY).unwrap();
            let after = PublicHistorySession::resume(&local, family)
                .unwrap()
                .cursor();
            let path = format!("/v1/families/{}/log?after={after}", hex(&family.family_id));
            let auth = manager.sign_get(&path).unwrap();
            let page = LogPage::decode(
                &relay
                    .log_page_authenticated(family.family_id, after, &path, &auth.bytes)
                    .unwrap(),
                family.family_id,
                after,
            )
            .unwrap();
            assert_eq!(page.entries.len(), 1, "expected one browser batch");
            let entry = &page.entries[0];
            assert_eq!(entry.kind, 2, "expected encrypted batch");
            let Value::Map(envelope) = cbor::decode(&entry.committed_bytes).unwrap() else {
                panic!("batch envelope not map")
            };
            let header = Header::decode(&cbor::encode(&envelope[0].1).unwrap()).unwrap();
            let result_path = format!(
                "/v1/families/{}/batch-results/{}",
                hex(&family.family_id),
                hex(&header.batch_id)
            );
            let auth = manager.sign_get(&result_path).unwrap();
            let result = BatchResult::decode(
                &relay
                    .batch_result_authenticated(
                        family.family_id,
                        header.batch_id,
                        &result_path,
                        &auth.bytes,
                    )
                    .unwrap(),
            )
            .unwrap();
            PublicHistorySession::resume(&local, family)
                .unwrap()
                .accept_batch(
                    &mut local,
                    &entry.committed_bytes,
                    result
                        .receipt_bytes
                        .as_ref()
                        .expect("accepted browser batch"),
                )
                .unwrap();
            let ready = manager.ready_session(&local).unwrap();
            let child = ready
                .projection()
                .record(&v7(0xb1))
                .expect("browser child absent");
            assert_eq!(child.record_type, "child");
            assert_eq!(
                child.field(1).map(|field| &field.value),
                Some(&Value::Text("BrowserRecipientChild".to_owned()))
            );
            println!("browser child read");
        }
        _ => panic!("unknown holder mode"),
    }
}
