use babytrack_core::{
    batch::{self, AuthenticatedBatch},
    cbor::Value,
    crypto,
    projection::{Outcome, Projection},
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

fn fixture() -> serde_json::Value {
    serde_json::from_str(include_str!("../../tests/vectors/negative-batch-v1.json")).unwrap()
}

fn authenticated(fixtures: &serde_json::Value, id: &str) -> AuthenticatedBatch {
    let case = fixtures["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["id"] == id)
        .unwrap();
    let base = &fixtures["base"];
    let family = bytes::<16>(base["family_id_hex"].as_str().unwrap());
    let relay = bytes::<32>("03396219237f75a64f12aeb7f39723abf400b160c364980a765dac24aeba2464");
    let key = bytes::<32>(base["epoch_key_hex"].as_str().unwrap());
    let signing_seed = bytes::<32>(base["recipient_sign_seed_hex"].as_str().unwrap());
    let signer = crypto::signing_public_key(&signing_seed);
    let envelope = hex_bytes(case["input"]["envelope_cbor_hex"].as_str().unwrap());
    batch::open_authenticated(&envelope, &family, &relay, &key, &signer).unwrap()
}

#[test]
fn encrypted_newer_minor_child_projects_and_rebuilds_identically() {
    let fixtures = fixture();
    let authenticated = authenticated(&fixtures, "CROSSMINORBYTE01");
    let opened = authenticated.parse().unwrap();
    let family = opened.header.family_id;
    let child_id = opened.operations[0].record_id;
    let mut projection = Projection::new(family);
    assert_eq!(
        projection.apply_authenticated(&authenticated, 1),
        Ok(Outcome::Applied)
    );
    let child = projection.record(&child_id).unwrap();
    assert_eq!(
        child.field(1).unwrap().value,
        Value::Text("Baby".to_owned())
    );
    assert_eq!(child.field(500).unwrap().canonical_bytes, vec![0xf4]);
    assert_eq!(child.field(500).unwrap().cursor, 1);
    assert_eq!(child.field(500).unwrap().operation_index, 0);
    assert!(!child.deleted);
    assert_eq!(projection.last_cursor(), 1);
    assert!(projection.inert_batches().is_empty());

    let mut rebuilt = Projection::new(family);
    assert_eq!(
        rebuilt.apply_authenticated(&authenticated, 1),
        Ok(Outcome::Applied)
    );
    assert_eq!(rebuilt, projection);
    assert_eq!(
        projection.apply_authenticated(&authenticated, 2),
        Ok(Outcome::Applied)
    );
    assert_eq!(
        projection
            .record(&child_id)
            .unwrap()
            .field(1)
            .unwrap()
            .cursor,
        1
    );
}

#[test]
fn signed_invalid_batches_are_wholly_inert_but_consume_cursor() {
    let fixtures = fixture();
    let family = bytes::<16>(fixtures["base"]["family_id_hex"].as_str().unwrap());
    for id in [
        "INERTBYTE01",
        "PRECREATEBYTE01",
        "WRONGSCOPEBYTE01",
        "PREFSBYTE01",
    ] {
        let batch = authenticated(&fixtures, id);
        let mut projection = Projection::new(family);
        let outcome = projection.apply_authenticated(&batch, 1).unwrap();
        assert!(matches!(outcome, Outcome::Inert(_)), "{id}: {outcome:?}");
        assert_eq!(projection.last_cursor(), 1, "{id}");
        assert_eq!(projection.inert_batches().len(), 1, "{id}");
        assert_eq!(
            projection.inert_batches()[0].object_hash,
            batch.object_hash,
            "{id}"
        );
        assert!(projection.record(&family).is_none(), "{id}");

        let valid = authenticated(&fixtures, "CROSSMINORBYTE01");
        assert_eq!(
            projection.apply_authenticated(&valid, 2),
            Ok(Outcome::Applied),
            "{id}"
        );
        let child_id = valid.parse().unwrap().operations[0].record_id;
        assert!(projection.record(&child_id).is_some(), "{id}");
    }
}

#[test]
fn cursor_and_family_guard_projection_before_mutation() {
    let fixtures = fixture();
    let batch = authenticated(&fixtures, "CROSSMINORBYTE01");
    let family = batch.header.family_id;
    let mut projection = Projection::new(family);
    assert!(projection.apply_authenticated(&batch, 2).is_err());
    assert_eq!(projection.last_cursor(), 0);
    let mut wrong_family = family;
    wrong_family[0] ^= 1;
    let mut other = Projection::new(wrong_family);
    assert!(other.apply_authenticated(&batch, 1).is_err());
    assert_eq!(other.last_cursor(), 0);
}

#[test]
fn verified_control_cursor_can_precede_a_data_batch() {
    let fixtures = fixture();
    let batch = authenticated(&fixtures, "CROSSMINORBYTE01");
    let mut projection = Projection::new(batch.header.family_id);
    assert_eq!(projection.advance_control(1), Ok(()));
    assert_eq!(
        projection.apply_authenticated(&batch, 2),
        Ok(Outcome::Applied)
    );
    assert_eq!(projection.last_cursor(), 2);
    assert_eq!(
        projection.advance_control(2),
        Err(babytrack_core::projection::Error::WrongCursor)
    );
}
