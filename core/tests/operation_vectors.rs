use babytrack_core::{
    cbor::{self, Value},
    operation::{Error, Kind, NewOperation, Operation, Scope},
};

fn hex_bytes(hex: &str) -> Vec<u8> {
    hex.as_bytes()
        .chunks_exact(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}

fn id16(hex: &str) -> [u8; 16] {
    hex_bytes(hex).try_into().unwrap()
}

fn family_operation_fixture() -> (Vec<u8>, [u8; 16], [u8; 16]) {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("../../tests/vectors/full-wire-v1.json")).unwrap();
    let case = fixture["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["id"] == "BATCHBYTE01")
        .unwrap();
    (
        hex_bytes(case["inputs"]["operation_cbor_hex"].as_str().unwrap()),
        id16("123e4567e89b42d3a456426614174000"),
        id16("223e4567e89b42d3a456426614174000"),
    )
}

#[test]
fn fixed_family_operation_decodes_and_preserves_bytes() {
    let (bytes, family, author) = family_operation_fixture();
    let operation = Operation::decode_bound(&bytes, &family, &author).unwrap();
    assert_eq!(operation.scope, Scope::Family);
    assert_eq!(operation.kind, Kind::Create);
    assert_eq!(operation.record_type.as_deref(), Some("family"));
    assert_eq!(operation.fields.len(), 1);
    assert_eq!(operation.field_bytes(1), Some(vec![0xa0]));
    assert_eq!(operation.canonical_bytes(), bytes);
    assert_eq!(operation.hlc.device_id, author);
    let constructed = NewOperation {
        family_id: family,
        operation_id: operation.operation_id,
        record_id: family,
        scope: Scope::Family,
        kind: Kind::Create,
        author_device_id: author,
        hlc: operation.hlc.clone(),
        record_type: Some("family".to_owned()),
        child_id: None,
        fields: Some(vec![(1, Value::Map(vec![]))]),
    };
    assert_eq!(Operation::encode_new(&constructed).unwrap(), bytes);
}

#[test]
fn newer_minor_field_stays_canonical_and_opaque() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("../../tests/vectors/negative-batch-v1.json")).unwrap();
    let case = fixture["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["id"] == "CROSSMINORBYTE01")
        .unwrap();
    let bytes = hex_bytes(case["input"]["operation_hex"].as_str().unwrap());
    let family = id16("123e4567e89b42d3a456426614174000");
    let author = id16("723e4567e89b42d3a456426614174000");
    let operation = Operation::decode_bound(&bytes, &family, &author).unwrap();
    assert_eq!(operation.field_bytes(500), Some(vec![0xf4]));
    assert_eq!(operation.canonical_bytes(), bytes);

    let Value::Map(mut map) = cbor::decode(&bytes).unwrap() else {
        panic!("operation map")
    };
    map.push((12, Value::Array(vec![Value::Bool(false)])));
    let with_extension = cbor::encode(&Value::Map(map)).unwrap();
    let parsed = Operation::decode_bound(&with_extension, &family, &author).unwrap();
    assert_eq!(parsed.canonical_bytes(), with_extension);
    assert_eq!(parsed.field_bytes(500), Some(vec![0xf4]));
    let mut invalid_record_id = operation.record_id;
    invalid_record_id[6] = (invalid_record_id[6] & 0x0f) | 0x40;
    let Value::Map(mut map) = cbor::decode(&bytes).unwrap() else {
        panic!("operation map")
    };
    map.iter_mut().find(|(key, _)| *key == 4).unwrap().1 = Value::Bytes(invalid_record_id.to_vec());
    assert_eq!(
        Operation::decode_bound(&cbor::encode(&Value::Map(map)).unwrap(), &family, &author),
        Err(Error::Invalid("child/activity record ID must be UUIDv7"))
    );
}

#[test]
fn rejects_wrong_scope_fixture() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("../../tests/vectors/negative-batch-v1.json")).unwrap();
    let case = fixture["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["id"] == "WRONGSCOPEBYTE01")
        .unwrap();
    let bytes = hex_bytes(case["input"]["operations_hex"][0].as_str().unwrap());
    let family = id16("123e4567e89b42d3a456426614174000");
    let author = id16("723e4567e89b42d3a456426614174000");
    assert_eq!(
        Operation::decode_bound(&bytes, &family, &author),
        Err(Error::Invalid("child scope requires child type"))
    );
}

#[test]
fn version_family_author_and_shape_fail_closed() {
    let (bytes, family, author) = family_operation_fixture();
    let mut wrong_family = family;
    wrong_family[0] ^= 1;
    assert_eq!(
        Operation::decode_bound(&bytes, &wrong_family, &author),
        Err(Error::WrongFamily)
    );
    let mut wrong_author = author;
    wrong_author[0] ^= 1;
    assert_eq!(
        Operation::decode_bound(&bytes, &family, &wrong_author),
        Err(Error::WrongAuthor)
    );

    let Value::Map(map) = cbor::decode(&bytes).unwrap() else {
        panic!("operation map")
    };
    let mutate = |key: u64, replacement: Value| {
        let mut changed = map.clone();
        changed.iter_mut().find(|(k, _)| *k == key).unwrap().1 = replacement;
        cbor::encode(&Value::Map(changed)).unwrap()
    };
    assert_eq!(
        Operation::decode_bound(&mutate(1, Value::Integer(2)), &family, &author),
        Err(Error::UnsupportedVersion)
    );
    let mut invalid_family = family;
    invalid_family[6] = (invalid_family[6] & 0x0f) | 0x70;
    assert_eq!(
        Operation::decode_bound(
            &mutate(2, Value::Bytes(invalid_family.to_vec())),
            &family,
            &author
        ),
        Err(Error::Invalid("Family ID must be UUIDv4"))
    );
    let mut invalid_author = author;
    invalid_author[8] = 0;
    assert_eq!(
        Operation::decode_bound(
            &mutate(7, Value::Bytes(invalid_author.to_vec())),
            &family,
            &author
        ),
        Err(Error::Invalid("author ID must be UUIDv4"))
    );
    assert!(Operation::decode_bound(&mutate(6, Value::Integer(9)), &family, &author).is_err());
    assert!(Operation::decode_bound(&mutate(8, Value::Array(vec![])), &family, &author).is_err());
    assert!(Operation::decode_bound(&mutate(11, Value::Array(vec![])), &family, &author).is_err());
    assert!(
        Operation::decode_bound(&mutate(9, Value::Text("child".into())), &family, &author).is_err()
    );
}
