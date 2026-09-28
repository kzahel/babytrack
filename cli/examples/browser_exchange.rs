//! Test driver for one real-relay browser/native encrypted exchange.
//! Browser-owned transport passes signed bytes as hex arguments. The native
//! core verifies the browser batch, then authors a second child operation.

use std::{collections::BTreeMap, env};

use babytrack_core::{
    batch::{self, Header},
    cbor::{self, Value},
    crypto,
    operation::{Hlc, Kind, NewOperation, Operation, Scope},
    projection::Outcome,
    ready_replay,
};

fn decode<const N: usize>(value: &str) -> [u8; N] {
    let bytes = hex_bytes(value);
    bytes.try_into().expect("fixed hex input length")
}

fn hex_bytes(value: &str) -> Vec<u8> {
    assert!(value.len().is_multiple_of(2), "hex input length");
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            u8::from_str_radix(std::str::from_utf8(pair).expect("hex text"), 16).expect("hex digit")
        })
        .collect()
}

fn hex(bytes: &[u8]) -> String {
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

fn main() {
    let args: Vec<_> = env::args().skip(1).collect();
    assert_eq!(
        args.len(),
        8,
        "expected genesis, relay key, epoch key, manager seed, object ID, object response, browser envelope, receipt"
    );
    let genesis = hex_bytes(&args[0]);
    let relay_public = decode::<32>(&args[1]);
    let epoch_key = decode::<32>(&args[2]);
    let manager_seed = decode::<32>(&args[3]);
    let object_id = decode::<16>(&args[4]);
    let object =
        ready_replay::verified_object_from_response(&genesis, object_id, &hex_bytes(&args[5]))
            .expect("browser-fetched object differs from signed genesis");
    let objects = BTreeMap::from([(object_id, object)]);
    let (mut chain, mut projection, key) =
        ready_replay::initial_epoch_projection(&genesis, relay_public, epoch_key, &objects)
            .expect("native initial ready replay");
    let manager = babytrack_core::control::verify_genesis(&genesis, &relay_public)
        .expect("signed genesis")
        .manager_device_id();
    assert_eq!(
        chain
            .active_signing_public(manager)
            .expect("active manager"),
        crypto::signing_public_key(&manager_seed)
    );
    let browser_envelope = hex_bytes(&args[6]);
    let browser_receipt = hex_bytes(&args[7]);
    let signed = chain
        .apply_public_batch(&browser_envelope, &browser_receipt)
        .expect("native verified browser acceptance");
    assert_eq!(
        projection
            .apply_authorized_signed(&signed, &key, chain.last_global_cursor())
            .expect("native opens browser batch"),
        Outcome::Applied
    );
    assert_eq!(
        projection
            .record(&chain.family_id())
            .map(|record| record.record_type.as_str()),
        Some("family")
    );

    let child_id = v7(0xb1);
    let operation = Operation::encode_new(&NewOperation {
        family_id: chain.family_id(),
        operation_id: v7(0xb2),
        record_id: child_id,
        scope: Scope::Child,
        kind: Kind::Create,
        author_device_id: manager,
        hlc: Hlc {
            wall_ms: 1_700_000_000_000,
            counter: 0,
            device_id: manager,
        },
        record_type: Some("child".to_owned()),
        child_id: None,
        fields: Some(vec![(1, Value::Text("MixedClientChild".to_owned()))]),
    })
    .expect("native child operation");
    let plaintext = cbor::encode(&Value::Array(vec![Value::Bytes(operation.clone())]))
        .expect("native plaintext");
    let header = Header {
        minor: 0,
        family_id: chain.family_id(),
        relay_id: chain.relay_id(),
        control_head: chain.head_hash(),
        epoch: chain.epoch().expect("active epoch"),
        batch_id: v4(0xb3),
        author_device_id: manager,
        device_sequence: chain.next_sequence_for(manager).expect("next sequence"),
        nonce: [0xb4; 24],
        plaintext_len: plaintext.len().try_into().expect("plaintext length"),
    };
    let envelope = batch::seal(&header, &[operation], &epoch_key, &manager_seed)
        .expect("native signed child batch")
        .envelope_bytes;
    println!("{} {}", hex(&envelope), hex(&child_id));
}
