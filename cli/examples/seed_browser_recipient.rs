//! Seed a disposable relay with the published first-admission chain so a
//! real browser can fetch the committed recipient grant over HTTP.

use std::env;

use babytrack_core::bootstrap::InvitationBootstrap;
use babytrack_core::cbor::{self, Value};
use babytrack_server::RelayStore;
use serde_json::Value as Json;

fn hex(value: &str) -> Vec<u8> {
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            u8::from_str_radix(std::str::from_utf8(pair).expect("hex text"), 16).expect("hex digit")
        })
        .collect()
}

fn fixed<const N: usize>(value: &str) -> [u8; N] {
    hex(value).try_into().expect("fixed hex length")
}

fn candidate(transition: &Json) -> Vec<u8> {
    cbor::encode(&Value::Map(vec![
        (
            1,
            cbor::decode(&hex(transition["unsigned_cbor_hex"].as_str().unwrap())).unwrap(),
        ),
        (
            2,
            cbor::decode(&hex(transition["signatures_cbor_hex"].as_str().unwrap())).unwrap(),
        ),
    ]))
    .unwrap()
}

fn commit_time(transition: &Json) -> i64 {
    let Value::Map(root) =
        cbor::decode(&hex(transition["committed_cbor_hex"].as_str().unwrap())).unwrap()
    else {
        panic!("committed control not map");
    };
    let Value::Array(receipt) = &root[2].1 else {
        panic!("control receipt absent");
    };
    let Value::Integer(ms) = receipt[4] else {
        panic!("control time absent");
    };
    ms.try_into().unwrap()
}

fn main() {
    let path = env::args().nth(1).expect("pass disposable relay DB path");
    let origin = env::args().nth(2).expect("pass browser relay origin");
    let transitions: usize = env::args()
        .nth(3)
        .unwrap_or_else(|| "6".to_owned())
        .parse()
        .unwrap();
    assert!((2..=6).contains(&transitions));
    let fixture: Json =
        serde_json::from_str(include_str!("../../tests/vectors/contiguous-chain-v1.json")).unwrap();
    let inputs = &fixture["test_only_inputs"];
    let family = fixed::<16>(inputs["family_id_hex"].as_str().unwrap());
    let relay_seed = fixed::<32>(inputs["relay_sign_seed_hex"].as_str().unwrap());
    let mut relay = RelayStore::open(path, relay_seed).unwrap();

    for index in 0..transitions {
        let transition = &fixture["transitions"][index];
        let body = candidate(transition);
        let Value::Map(parts) = cbor::decode(&body).unwrap() else {
            panic!("candidate not map");
        };
        for item in transition["manifest"].as_array().unwrap() {
            let id_hex = item[1].as_str().unwrap();
            let id = fixed::<16>(id_hex);
            let staging = cbor::encode(&Value::Map(vec![
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
                0 => relay.stage_genesis_object(family, id, &staging).unwrap(),
                1 => relay
                    .stage_first_issue_object(family, id, &staging)
                    .unwrap(),
                3 => relay
                    .stage_first_challenge_object(family, id, &staging)
                    .unwrap(),
                5 => relay
                    .stage_first_admission_object(family, id, &staging)
                    .unwrap(),
                _ => panic!("unexpected staged object"),
            };
        }
        let response = match index {
            0 => relay
                .commit_genesis(family, &body, commit_time(transition))
                .unwrap(),
            1 => relay
                .commit_first_issue(family, &body, commit_time(transition))
                .unwrap(),
            2 => relay
                .commit_first_claim(family, &body, commit_time(transition))
                .unwrap(),
            3 => relay
                .commit_first_challenge(family, &body, commit_time(transition))
                .unwrap(),
            4 => relay
                .commit_first_proof(family, &body, commit_time(transition))
                .unwrap(),
            5 => relay
                .commit_first_admission(family, &body, commit_time(transition))
                .unwrap(),
            _ => unreachable!(),
        };
        let Value::Map(result) = cbor::decode(&response).unwrap() else {
            panic!("commit result not map");
        };
        assert_eq!(
            result[1].1,
            Value::Bytes(hex(transition["committed_cbor_hex"].as_str().unwrap()))
        );
    }
    let fragment = InvitationBootstrap::from_committed_issue(
        &origin,
        babytrack_core::crypto::signing_public_key(&relay_seed),
        &hex(fixture["transitions"][0]["committed_cbor_hex"]
            .as_str()
            .unwrap()),
        &hex(fixture["transitions"][1]["committed_cbor_hex"]
            .as_str()
            .unwrap()),
        fixed::<32>(inputs["invitation_sign_seed_hex"].as_str().unwrap()),
    )
    .unwrap()
    .to_fragment()
    .unwrap();
    println!("{fragment}");
}
