use babytrack_core::{
    batch::{self, Error, Header},
    crypto,
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
fn fixed_batch_matches_every_wire_byte_and_opens() {
    let fixtures: serde_json::Value =
        serde_json::from_str(include_str!("../../tests/vectors/full-wire-v1.json")).unwrap();
    let cases = fixtures["cases"].as_array().unwrap();
    let genesis = cases.iter().find(|case| case["id"] == "GENESIS01").unwrap();
    let case = cases
        .iter()
        .find(|case| case["id"] == "BATCHBYTE01")
        .unwrap();
    let inputs = &case["inputs"];
    let expected = &case["expect"];
    let family_id = bytes::<16>(genesis["inputs"]["family_id_hex"].as_str().unwrap());
    let author_device_id = bytes::<16>(genesis["inputs"]["device_id_hex"].as_str().unwrap());
    let relay_id = bytes::<32>(genesis["expect"]["relay_id_hex"].as_str().unwrap());
    let epoch_key = bytes::<32>(inputs["epoch_key_hex"].as_str().unwrap());
    let signing_seed = bytes::<32>(genesis["inputs"]["manager_sign_seed_hex"].as_str().unwrap());
    let signing_public = crypto::signing_public_key(&signing_seed);
    let expected_plaintext = hex_bytes(expected["plaintext_cbor_hex"].as_str().unwrap());
    let operation = hex_bytes(inputs["operation_cbor_hex"].as_str().unwrap());
    let header = Header {
        minor: 0,
        family_id,
        relay_id,
        control_head: bytes(inputs["control_head_hex"].as_str().unwrap()),
        epoch: 1,
        batch_id: bytes(inputs["batch_id_hex"].as_str().unwrap()),
        author_device_id,
        device_sequence: inputs["device_sequence"].as_u64().unwrap(),
        nonce: bytes(inputs["nonce_hex"].as_str().unwrap()),
        plaintext_len: expected_plaintext.len().try_into().unwrap(),
    };
    let sealed = batch::seal(
        &header,
        std::slice::from_ref(&operation),
        &epoch_key,
        &signing_seed,
    )
    .unwrap();
    assert_eq!(
        sealed.header_bytes,
        hex_bytes(expected["header_cbor_hex"].as_str().unwrap())
    );
    assert_eq!(Header::decode(&sealed.header_bytes), Ok(header.clone()));
    let mut invalid_header = header.clone();
    invalid_header.batch_id[6] = 0x70;
    assert!(invalid_header.encode().is_err());
    invalid_header = header.clone();
    invalid_header.epoch = 0;
    assert!(invalid_header.encode().is_err());
    assert_eq!(sealed.plaintext_bytes, expected_plaintext);
    assert_eq!(
        sealed.aad,
        bytes::<32>(expected["aad_hex"].as_str().unwrap())
    );
    assert_eq!(
        sealed.ciphertext,
        hex_bytes(expected["ciphertext_hex"].as_str().unwrap())
    );
    assert_eq!(
        sealed.signature,
        bytes::<64>(expected["device_signature_hex"].as_str().unwrap())
    );
    assert_eq!(
        sealed.envelope_bytes,
        hex_bytes(expected["envelope_cbor_hex"].as_str().unwrap())
    );
    assert_eq!(
        sealed.object_hash,
        bytes::<32>(expected["object_hash_hex"].as_str().unwrap())
    );

    let opened = batch::open_verified(
        &sealed.envelope_bytes,
        &family_id,
        &relay_id,
        &epoch_key,
        &signing_public,
    )
    .unwrap();
    assert_eq!(opened.header(), &header);
    assert_eq!(opened.operations().len(), 1);
    assert_eq!(opened.operations()[0].canonical_bytes(), operation);
    assert_eq!(opened.object_hash(), sealed.object_hash);

    let mut wrong_family = family_id;
    wrong_family[0] ^= 1;
    assert_eq!(
        batch::open_verified(
            &sealed.envelope_bytes,
            &wrong_family,
            &relay_id,
            &epoch_key,
            &signing_public
        ),
        Err(Error::WrongFamily)
    );
    let mut wrong_relay = relay_id;
    wrong_relay[0] ^= 1;
    assert_eq!(
        batch::open_verified(
            &sealed.envelope_bytes,
            &family_id,
            &wrong_relay,
            &epoch_key,
            &signing_public
        ),
        Err(Error::WrongRelay)
    );
    let mut wrong_key = epoch_key;
    wrong_key[0] ^= 1;
    assert_eq!(
        batch::open_verified(
            &sealed.envelope_bytes,
            &family_id,
            &relay_id,
            &wrong_key,
            &signing_public
        ),
        Err(Error::Crypto(crypto::Error::AuthenticationFailed))
    );
    let mut wrong_signer = signing_public;
    wrong_signer[0] ^= 1;
    assert!(
        batch::open_verified(
            &sealed.envelope_bytes,
            &family_id,
            &relay_id,
            &epoch_key,
            &wrong_signer
        )
        .is_err()
    );
}

#[test]
fn newer_minor_envelope_preserves_unknown_child_field() {
    let fixtures: serde_json::Value =
        serde_json::from_str(include_str!("../../tests/vectors/negative-batch-v1.json")).unwrap();
    let case = fixtures["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["id"] == "CROSSMINORBYTE01")
        .unwrap();
    let base = &fixtures["base"];
    let family_id = bytes::<16>(base["family_id_hex"].as_str().unwrap());
    let relay_id = bytes::<32>("03396219237f75a64f12aeb7f39723abf400b160c364980a765dac24aeba2464");
    let epoch_key = bytes::<32>(base["epoch_key_hex"].as_str().unwrap());
    let signing_seed = bytes::<32>(base["recipient_sign_seed_hex"].as_str().unwrap());
    let signing_public = crypto::signing_public_key(&signing_seed);
    let envelope = hex_bytes(case["input"]["envelope_cbor_hex"].as_str().unwrap());
    let header = Header::decode(&hex_bytes(
        case["input"]["header_cbor_hex"].as_str().unwrap(),
    ))
    .unwrap();
    let operation = hex_bytes(case["input"]["operation_hex"].as_str().unwrap());
    let sealed = batch::seal(
        &header,
        std::slice::from_ref(&operation),
        &epoch_key,
        &signing_seed,
    )
    .unwrap();
    assert_eq!(sealed.envelope_bytes, envelope);
    let opened = batch::open_verified(
        &envelope,
        &family_id,
        &relay_id,
        &epoch_key,
        &signing_public,
    )
    .unwrap();
    assert_eq!(opened.header().minor, 1);
    assert_eq!(opened.operations().len(), 1);
    assert_eq!(opened.operations()[0].field_bytes(500), Some(vec![0xf4]));
    assert_eq!(opened.operations()[0].canonical_bytes(), operation);
}
