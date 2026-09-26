use babytrack_core::crypto::{self, Error};

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
fn domain_hashes_match_records_vectors() {
    let fixtures: serde_json::Value =
        serde_json::from_str(include_str!("../../tests/vectors/records-v1.json")).unwrap();
    let mut tested = 0;
    for case in fixtures["cases"].as_array().unwrap() {
        if !case["id"].as_str().unwrap().starts_with("HASH") {
            continue;
        }
        tested += 1;
        let actual = crypto::hash(
            case["label"].as_str().unwrap(),
            &hex_bytes(case["input_hex"].as_str().unwrap()),
        )
        .unwrap();
        assert_eq!(
            actual,
            bytes::<32>(case["expect_sha256_hex"].as_str().unwrap())
        );
    }
    assert_eq!(tested, 2);
    assert_eq!(crypto::hash("bad\0label", b""), Err(Error::InvalidLabel));
}

#[test]
fn ed25519_signature_matches_fixed_vector_and_rejects_tampering() {
    let fixtures: serde_json::Value =
        serde_json::from_str(include_str!("../../tests/vectors/crypto-v1.json")).unwrap();
    let case = fixtures["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["id"] == "SIG01")
        .unwrap();
    let seed = bytes::<32>(case["seed_hex"].as_str().unwrap());
    let public = bytes::<32>(case["public_key_hex"].as_str().unwrap());
    let message = hex_bytes(case["message_cbor_hex"].as_str().unwrap());
    let label = case["label"].as_str().unwrap();
    let signature = bytes::<64>(case["signature_hex"].as_str().unwrap());
    assert_eq!(crypto::signing_public_key(&seed), public);
    assert_eq!(
        crypto::hash(label, &message).unwrap(),
        bytes::<32>(case["digest_hex"].as_str().unwrap())
    );
    assert_eq!(crypto::sign_cbor(label, &message, &seed), Ok(signature));
    assert_eq!(
        crypto::verify_cbor(label, &message, &public, &signature),
        Ok(())
    );

    assert_eq!(
        crypto::verify_cbor("other", &message, &public, &signature),
        Err(Error::InvalidSignature)
    );
    let mut tampered_signature = signature;
    tampered_signature[0] ^= 1;
    assert_eq!(
        crypto::verify_cbor(label, &message, &public, &tampered_signature),
        Err(Error::InvalidSignature)
    );
    assert_eq!(
        crypto::sign_cbor(label, &[0x18, 0x01], &seed),
        Err(Error::NoncanonicalCbor)
    );
}

#[test]
fn xchacha20poly1305_matches_fixed_vector_and_authenticates_context() {
    let fixtures: serde_json::Value =
        serde_json::from_str(include_str!("../../tests/vectors/crypto-v1.json")).unwrap();
    let case = fixtures["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["id"] == "AEAD01")
        .unwrap();
    let key = bytes::<32>(case["key_hex"].as_str().unwrap());
    let nonce = bytes::<24>(case["nonce_hex"].as_str().unwrap());
    let aad = hex_bytes(case["aad_hex"].as_str().unwrap());
    let plaintext = hex_bytes(case["plaintext_hex"].as_str().unwrap());
    let ciphertext = hex_bytes(case["ciphertext_hex"].as_str().unwrap());
    assert_eq!(
        crypto::seal_with_nonce(&key, &nonce, &aad, &plaintext),
        Ok(ciphertext.clone())
    );
    assert_eq!(crypto::open(&key, &nonce, &aad, &ciphertext), Ok(plaintext));

    let mut wrong_key = key;
    wrong_key[0] ^= 1;
    assert_eq!(
        crypto::open(&wrong_key, &nonce, &aad, &ciphertext),
        Err(Error::AuthenticationFailed)
    );
    let mut wrong_aad = aad.clone();
    wrong_aad[0] ^= 1;
    assert_eq!(
        crypto::open(&key, &nonce, &wrong_aad, &ciphertext),
        Err(Error::AuthenticationFailed)
    );
    let mut wrong_nonce = nonce;
    wrong_nonce[0] ^= 1;
    assert_eq!(
        crypto::open(&key, &wrong_nonce, &aad, &ciphertext),
        Err(Error::AuthenticationFailed)
    );
    let mut wrong_ciphertext = ciphertext;
    wrong_ciphertext[0] ^= 1;
    assert_eq!(
        crypto::open(&key, &nonce, &aad, &wrong_ciphertext),
        Err(Error::AuthenticationFailed)
    );
}
