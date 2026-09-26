use babytrack_core::{
    batch::{self, Header},
    cbor::{self, Value},
    crypto,
    projection::Outcome,
    session::FamilySession,
};

fn hex_bytes(hex: &str) -> Vec<u8> {
    hex.as_bytes()
        .chunks_exact(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}

fn accepted_receipt(
    header: &Header,
    object_hash: [u8; 32],
    cursor: u64,
    relay_seed: &[u8; 32],
) -> Vec<u8> {
    let body = Value::Map(vec![
        (1, Value::Integer(1)),
        (2, Value::Bytes(header.family_id.to_vec())),
        (3, Value::Bytes(header.relay_id.to_vec())),
        (4, Value::Bytes(header.batch_id.to_vec())),
        (5, Value::Bytes(object_hash.to_vec())),
        (6, Value::Bool(true)),
        (7, Value::Integer(cursor.into())),
        (8, Value::Bytes(header.control_head.to_vec())),
        (9, Value::Integer(header.device_sequence.into())),
        (10, Value::Null),
        (11, Value::Integer((header.device_sequence + 1).into())),
    ]);
    let body_bytes = cbor::encode(&body).unwrap();
    let signature = crypto::sign_cbor("batch-receipt", &body_bytes, relay_seed).unwrap();
    cbor::encode(&Value::Map(vec![
        (1, body),
        (2, Value::Bytes(signature.to_vec())),
    ]))
    .unwrap()
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

#[test]
fn authorized_signed_unopenable_batch_is_inert_and_later_batch_applies() {
    let fixtures: serde_json::Value =
        serde_json::from_str(include_str!("../../tests/vectors/full-wire-v1.json")).unwrap();
    let cases = fixtures["cases"].as_array().unwrap();
    let genesis = cases.iter().find(|case| case["id"] == "GENESIS01").unwrap();
    let batch_case = cases
        .iter()
        .find(|case| case["id"] == "BATCHBYTE01")
        .unwrap();
    let genesis_bytes = hex_bytes(genesis["expect"]["control_object_hex"].as_str().unwrap());
    let relay_seed = bytes::<32>(genesis["inputs"]["relay_sign_seed_hex"].as_str().unwrap());
    let relay_public = crypto::signing_public_key(&relay_seed);
    let manager_seed = bytes::<32>(genesis["inputs"]["manager_sign_seed_hex"].as_str().unwrap());
    let key = bytes::<32>(genesis["inputs"]["epoch_key_hex"].as_str().unwrap());
    let header = Header::decode(&hex_bytes(
        batch_case["expect"]["header_cbor_hex"].as_str().unwrap(),
    ))
    .unwrap();
    let mut envelope = cbor::decode(&hex_bytes(
        batch_case["expect"]["envelope_cbor_hex"].as_str().unwrap(),
    ))
    .unwrap();
    if let Value::Map(fields) = &mut envelope {
        if let Value::Bytes(ciphertext) = &mut fields[1].1 {
            ciphertext[0] ^= 1;
        }
        let ciphertext = match &fields[1].1 {
            Value::Bytes(value) => value,
            _ => unreachable!(),
        };
        let ciphertext_hash = crypto::hash("batch-ciphertext", ciphertext).unwrap();
        let signed_value = Value::Array(vec![
            fields[0].1.clone(),
            Value::Bytes(ciphertext_hash.to_vec()),
        ]);
        let signature = crypto::sign_cbor(
            "batch-envelope",
            &cbor::encode(&signed_value).unwrap(),
            &manager_seed,
        )
        .unwrap();
        fields[2].1 = Value::Bytes(signature.to_vec());
    }
    let bad_bytes = cbor::encode(&envelope).unwrap();
    let bad_hash = crypto::hash("object", &bad_bytes).unwrap();
    let bad_receipt = accepted_receipt(&header, bad_hash, 2, &relay_seed);
    let mut session = FamilySession::from_genesis(&genesis_bytes, relay_public, key).unwrap();
    assert!(matches!(
        session.apply_initial_batch(&bad_bytes, &bad_receipt),
        Ok(Outcome::Inert(_))
    ));
    assert_eq!(session.projection().last_cursor(), 2);

    let mut later_header = header;
    later_header.batch_id[15] ^= 1;
    later_header.nonce[0] ^= 1;
    later_header.device_sequence = 2;
    let operation = hex_bytes(batch_case["inputs"]["operation_cbor_hex"].as_str().unwrap());
    let later = batch::seal(&later_header, &[operation], &key, &manager_seed).unwrap();
    let later_receipt = accepted_receipt(&later_header, later.object_hash, 3, &relay_seed);
    assert_eq!(
        session.apply_initial_batch(&later.envelope_bytes, &later_receipt),
        Ok(Outcome::Applied)
    );
    assert_eq!(session.projection().last_cursor(), 3);
    assert_eq!(session.projection().inert_batches().len(), 1);
}
