use babytrack_core::{
    batch::{self, AuthenticatedBatch, Header},
    cbor::{self, Value},
    crypto,
    operation::{Hlc, Kind, NewOperation, Operation, Scope},
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
    let family = opened.header().family_id;
    let child_id = opened.operations()[0].record_id;
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
            batch.object_hash(),
            "{id}"
        );
        assert!(projection.record(&family).is_none(), "{id}");

        let valid = authenticated(&fixtures, "CROSSMINORBYTE01");
        assert_eq!(
            projection.apply_authenticated(&valid, 2),
            Ok(Outcome::Applied),
            "{id}"
        );
        let child_id = valid.parse().unwrap().operations()[0].record_id;
        assert!(projection.record(&child_id).is_some(), "{id}");
    }
}

#[test]
fn cursor_and_family_guard_projection_before_mutation() {
    let fixtures = fixture();
    let batch = authenticated(&fixtures, "CROSSMINORBYTE01");
    let family = batch.header().family_id;
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
    let mut projection = Projection::new(batch.header().family_id);
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

fn operation_id(suffix: u8) -> [u8; 16] {
    let mut id = bytes::<16>("0183f9d0000070008000000000000000");
    id[15] = suffix;
    id
}

fn new_child_operation(
    family: [u8; 16],
    author: [u8; 16],
    child: [u8; 16],
    suffix: u8,
    kind: Kind,
    field: Option<(u64, Value)>,
    wall_ms: i64,
) -> Vec<u8> {
    Operation::encode_new(&NewOperation {
        family_id: family,
        operation_id: operation_id(suffix),
        record_id: child,
        scope: Scope::Child,
        kind,
        author_device_id: author,
        hlc: Hlc {
            wall_ms,
            counter: 0,
            device_id: author,
        },
        record_type: (kind == Kind::Create).then(|| "child".to_owned()),
        child_id: None,
        fields: field.map(|field| vec![field]),
    })
    .unwrap()
}

fn authenticated_test_batch(
    family: [u8; 16],
    author: [u8; 16],
    cursor: u64,
    operations: &[Vec<u8>],
) -> AuthenticatedBatch {
    let relay = [4; 32];
    let key = [5; 32];
    let seed = [6; 32];
    let plaintext = cbor::encode(&Value::Array(
        operations.iter().cloned().map(Value::Bytes).collect(),
    ))
    .unwrap();
    let mut batch_id = bytes::<16>("533e4567e89b42d3a456426614174000");
    batch_id[15] = cursor as u8;
    let header = Header {
        minor: 0,
        family_id: family,
        relay_id: relay,
        control_head: [7; 32],
        epoch: 1,
        batch_id,
        author_device_id: author,
        device_sequence: cursor,
        nonce: [cursor as u8; 24],
        plaintext_len: plaintext.len().try_into().unwrap(),
    };
    let sealed = batch::seal(&header, operations, &key, &seed).unwrap();
    batch::open_authenticated(
        &sealed.envelope_bytes,
        &family,
        &relay,
        &key,
        &crypto::signing_public_key(&seed),
    )
    .unwrap()
}

#[test]
fn cursor_order_beats_hostile_hlc_and_tombstones_need_explicit_restore() {
    let family = bytes::<16>("123e4567e89b42d3a456426614174000");
    let author = bytes::<16>("723e4567e89b42d3a456426614174000");
    let child = operation_id(0x50);
    let create = new_child_operation(
        family,
        author,
        child,
        1,
        Kind::Create,
        Some((1, Value::Text("First".to_owned()))),
        100,
    );
    let hostile = new_child_operation(
        family,
        author,
        child,
        2,
        Kind::Set,
        Some((1, Value::Text("Hostile".to_owned()))),
        i64::MAX,
    );
    let delete = new_child_operation(family, author, child, 3, Kind::Delete, None, 101);
    let later = new_child_operation(
        family,
        author,
        child,
        4,
        Kind::Set,
        Some((1, Value::Text("Later".to_owned()))),
        1,
    );
    let restore = new_child_operation(family, author, child, 5, Kind::Restore, None, 102);
    let conflict = new_child_operation(
        family,
        author,
        child,
        2,
        Kind::Set,
        Some((1, Value::Text("Conflict".to_owned()))),
        i64::MAX,
    );
    let second_create = new_child_operation(
        family,
        author,
        child,
        6,
        Kind::Create,
        Some((1, Value::Text("Second create".to_owned()))),
        103,
    );
    let future_field = new_child_operation(
        family,
        author,
        child,
        7,
        Kind::Set,
        Some((500, Value::Bool(false))),
        104,
    );
    let operations = [
        create,
        hostile.clone(),
        delete,
        later,
        restore,
        conflict,
        hostile, // Same operation bytes are a deduplication no-op.
        second_create,
        future_field,
    ];
    let batches: Vec<_> = operations
        .iter()
        .enumerate()
        .map(|(index, operation)| {
            authenticated_test_batch(
                family,
                author,
                index as u64 + 1,
                std::slice::from_ref(operation),
            )
        })
        .collect();
    let mut projection = Projection::new(family);
    for (index, batch) in batches.iter().enumerate() {
        let cursor = index as u64 + 1;
        let outcome = projection.apply_authenticated(batch, cursor).unwrap();
        if [6, 8].contains(&cursor) {
            assert!(matches!(outcome, Outcome::Inert(_)));
        } else {
            assert_eq!(outcome, Outcome::Applied);
        }
        if cursor == 3 || cursor == 4 {
            assert!(projection.record(&child).unwrap().deleted);
        }
    }
    let record = projection.record(&child).unwrap();
    assert!(!record.deleted);
    assert_eq!(record.tombstone_stamp, Some((5, 0)));
    assert_eq!(
        record.field(1).unwrap().value,
        Value::Text("Later".to_owned())
    );
    assert_eq!(record.field(1).unwrap().cursor, 4);
    assert_eq!(record.field(500).unwrap().canonical_bytes, vec![0xf4]);
    assert_eq!(projection.inert_batches().len(), 2);

    let mut rebuilt = Projection::new(family);
    for (index, batch) in batches.iter().enumerate() {
        rebuilt
            .apply_authenticated(batch, index as u64 + 1)
            .unwrap();
    }
    assert_eq!(rebuilt, projection);
}
