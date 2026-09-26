use babytrack_core::{
    cbor::{self, Value},
    projection::Outcome,
    session::FamilySession,
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
fn fixed_genesis_and_batch_receipt_advance_only_together() {
    let fixtures: serde_json::Value =
        serde_json::from_str(include_str!("../../tests/vectors/full-wire-v1.json")).unwrap();
    let cases = fixtures["cases"].as_array().unwrap();
    let genesis = cases.iter().find(|case| case["id"] == "GENESIS01").unwrap();
    let batch = cases
        .iter()
        .find(|case| case["id"] == "BATCHBYTE01")
        .unwrap();
    let genesis_bytes = hex_bytes(genesis["expect"]["control_object_hex"].as_str().unwrap());
    let relay_public = bytes::<32>(genesis["expect"]["relay_public_key_hex"].as_str().unwrap());
    let epoch_key = bytes::<32>(genesis["inputs"]["epoch_key_hex"].as_str().unwrap());
    let envelope = hex_bytes(batch["expect"]["envelope_cbor_hex"].as_str().unwrap());
    let receipt = hex_bytes(
        batch["expect"]["accepted_receipt_cbor_hex"]
            .as_str()
            .unwrap(),
    );
    let mut session = FamilySession::from_genesis(&genesis_bytes, relay_public, epoch_key).unwrap();
    assert_eq!(session.projection().last_cursor(), 1);
    let mut wrong_receipt = receipt.clone();
    *wrong_receipt.last_mut().unwrap() ^= 1;
    assert!(
        session
            .apply_initial_batch(&envelope, &wrong_receipt)
            .is_err()
    );
    assert_eq!(session.projection().last_cursor(), 1);
    let mut wrong_envelope = envelope.clone();
    *wrong_envelope.last_mut().unwrap() ^= 1;
    assert!(
        session
            .apply_initial_batch(&wrong_envelope, &receipt)
            .is_err()
    );
    assert_eq!(session.projection().last_cursor(), 1);
    assert_eq!(
        session.apply_initial_batch(&envelope, &receipt),
        Ok(Outcome::Applied)
    );
    assert_eq!(session.projection().last_cursor(), 2);
    assert!(session.apply_initial_batch(&envelope, &receipt).is_err());

    let mut swapped = cbor::decode(&receipt).unwrap();
    if let Value::Map(outer) = &mut swapped
        && let Value::Map(body) = &mut outer[0].1
    {
        body[6].1 = Value::Integer(3);
    }
    let altered_receipt = cbor::encode(&swapped).unwrap();
    let mut clean = FamilySession::from_genesis(&genesis_bytes, relay_public, epoch_key).unwrap();
    assert!(
        clean
            .apply_initial_batch(&envelope, &altered_receipt)
            .is_err()
    );
    assert_eq!(clean.projection().last_cursor(), 1);
}
