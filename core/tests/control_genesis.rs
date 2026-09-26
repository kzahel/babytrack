use babytrack_core::{
    cbor::{self, Value},
    control, crypto,
};

fn hex_bytes(hex: &str) -> Vec<u8> {
    hex.as_bytes()
        .chunks_exact(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}

fn bytes<const N: usize>(hex: &str) -> [u8; N] {
    hex_bytes(hex).try_into().unwrap()
}

#[test]
fn fixed_genesis_verifies_every_link_before_issuing_epoch_key() {
    let fixtures: serde_json::Value =
        serde_json::from_str(include_str!("../../tests/vectors/full-wire-v1.json")).unwrap();
    let case = fixtures["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["id"] == "GENESIS01")
        .unwrap();
    let input = &case["inputs"];
    let expected = &case["expect"];
    let relay_public = bytes::<32>(expected["relay_public_key_hex"].as_str().unwrap());
    let wire = hex_bytes(expected["control_object_hex"].as_str().unwrap());
    let verified = control::verify_genesis(&wire, &relay_public).unwrap();
    assert_eq!(
        verified.family_id(),
        bytes(input["family_id_hex"].as_str().unwrap())
    );
    assert_eq!(
        verified.head_hash(),
        bytes(expected["control_head_hex"].as_str().unwrap())
    );
    assert_eq!(
        verified.state_hash(),
        bytes(expected["auth_state_hash_hex"].as_str().unwrap())
    );
    assert_eq!(
        verified.committed_ms(),
        input["commit_ms"].as_i64().unwrap()
    );
    let key = bytes::<32>(input["epoch_key_hex"].as_str().unwrap());
    assert!(verified.verify_epoch_key(&key).is_ok());
    let mut wrong_key = key;
    wrong_key[0] ^= 1;
    assert!(verified.verify_epoch_key(&wrong_key).is_err());
    let mut wrong_relay = relay_public;
    wrong_relay[0] ^= 1;
    assert!(control::verify_genesis(&wire, &wrong_relay).is_err());

    let mut object = cbor::decode(&wire).unwrap();
    if let Value::Map(root) = &mut object
        && let Value::Map(unsigned) = &mut root[0].1
    {
        unsigned[7].1 = Value::Bytes([0u8; 32].to_vec());
    }
    assert!(control::verify_genesis(&cbor::encode(&object).unwrap(), &relay_public).is_err());
    let mut tampered = wire.clone();
    *tampered.last_mut().unwrap() ^= 1;
    assert!(control::verify_genesis(&tampered, &relay_public).is_err());
    assert_eq!(
        crypto::signing_public_key(&bytes(input["relay_sign_seed_hex"].as_str().unwrap())),
        relay_public
    );
}
