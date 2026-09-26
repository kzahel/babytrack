use babytrack_core::hpke::{self, Error};
use rand_chacha::{ChaCha20Rng, rand_core::SeedableRng};

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
fn opens_fixed_hpke_vector_and_rejects_wrong_context() {
    let fixtures: serde_json::Value =
        serde_json::from_str(include_str!("../../tests/vectors/crypto-v1.json")).unwrap();
    let case = fixtures["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["id"] == "HPKE01")
        .unwrap();
    let private = bytes::<32>(case["recipient_private_key_hex"].as_str().unwrap());
    let public = bytes::<32>(case["recipient_public_key_hex"].as_str().unwrap());
    let enc = bytes::<32>(case["enc_hex"].as_str().unwrap());
    let info = hex_bytes(case["info_hex"].as_str().unwrap());
    let aad = hex_bytes(case["aad_hex"].as_str().unwrap());
    let plaintext = hex_bytes(case["plaintext_hex"].as_str().unwrap());
    let ciphertext = hex_bytes(case["ciphertext_hex"].as_str().unwrap());
    assert_eq!(hpke::public_key_from_private(&private), Ok(public));
    assert_eq!(
        hpke::open(&private, &enc, &info, &aad, &ciphertext),
        Ok(plaintext)
    );

    let mut wrong_aad = aad.clone();
    wrong_aad[0] ^= 1;
    assert_eq!(
        hpke::open(&private, &enc, &info, &wrong_aad, &ciphertext),
        Err(Error::OpenFailed)
    );
    let mut wrong_info = info.clone();
    wrong_info[0] ^= 1;
    assert_eq!(
        hpke::open(&private, &enc, &wrong_info, &aad, &ciphertext),
        Err(Error::OpenFailed)
    );
    let mut wrong_private = private;
    // X25519 clamps low bits of byte 0; change a significant key bit.
    wrong_private[5] ^= 1;
    assert_eq!(
        hpke::open(&wrong_private, &enc, &info, &aad, &ciphertext),
        Err(Error::OpenFailed)
    );
    let mut wrong_enc = enc;
    wrong_enc[0] ^= 1;
    assert_eq!(
        hpke::open(&private, &wrong_enc, &info, &aad, &ciphertext),
        Err(Error::OpenFailed)
    );
    let mut wrong_ciphertext = ciphertext;
    wrong_ciphertext[0] ^= 1;
    assert_eq!(
        hpke::open(&private, &enc, &info, &aad, &wrong_ciphertext),
        Err(Error::OpenFailed)
    );
}

#[test]
fn seeded_sender_round_trips_and_checks_info_length() {
    let private = [7u8; 32];
    let public = hpke::public_key_from_private(&private).unwrap();
    let mut rng = ChaCha20Rng::from_seed([9u8; 32]);
    let sealed =
        hpke::seal_with_rng(&public, b"grant-info", b"family-aad", b"secret", &mut rng).unwrap();
    assert_eq!(
        hpke::open(
            &private,
            &sealed.enc,
            b"grant-info",
            b"family-aad",
            &sealed.ciphertext
        ),
        Ok(b"secret".to_vec())
    );
    let mut next_rng = ChaCha20Rng::from_seed([9u8; 32]);
    assert_eq!(
        hpke::seal_with_rng(
            &public,
            b"grant-info",
            b"family-aad",
            b"secret",
            &mut next_rng
        ),
        Ok(sealed)
    );
    assert_eq!(
        hpke::open(&private, &[0; 32], &vec![0; 65_531], b"", b""),
        Err(Error::InvalidInfo)
    );
}
