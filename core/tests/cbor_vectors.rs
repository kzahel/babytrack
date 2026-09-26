use babytrack_core::cbor::{self, Error, Limits, Value};

fn hex_bytes(hex: &str) -> Vec<u8> {
    assert!(hex.len().is_multiple_of(2));
    hex.as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let text = std::str::from_utf8(pair).expect("ASCII hex");
            u8::from_str_radix(text, 16).expect("valid hex")
        })
        .collect()
}

#[test]
fn records_v1_cbor_vectors() {
    let fixtures: serde_json::Value =
        serde_json::from_str(include_str!("../../tests/vectors/records-v1.json"))
            .expect("records fixture JSON");
    let mut tested = 0;
    for case in fixtures["cases"].as_array().expect("cases") {
        let Some(id) = case["id"].as_str() else {
            continue;
        };
        if !id.starts_with("CB") {
            continue;
        }
        tested += 1;
        let bytes = hex_bytes(case["input_hex"].as_str().expect("input_hex"));
        match case["expect"].as_str().expect("expect") {
            "accept" => {
                let value = cbor::decode(&bytes).unwrap_or_else(|error| panic!("{id}: {error}"));
                assert_eq!(cbor::encode(&value).expect("reencode"), bytes, "{id}");
                if id == "CB01" {
                    assert_eq!(
                        value,
                        Value::Map(vec![(1, Value::Integer(1)), (2, Value::Bytes(Vec::new()))])
                    );
                }
            }
            "reject" => assert!(cbor::decode(&bytes).is_err(), "{id}"),
            other => panic!("{id}: unexpected expectation {other}"),
        }
    }
    assert_eq!(tested, 6, "all CB vectors must execute");
}

#[test]
fn canonical_values_round_trip_with_unknown_nested_field() {
    let value = Value::Map(vec![
        (1, Value::Integer(1)),
        (
            99,
            Value::Array(vec![
                Value::Integer(-18_446_744_073_709_551_616),
                Value::Bytes(vec![0, 255]),
                Value::Text("é".into()),
                Value::Map(vec![(2, Value::Bool(true)), (24, Value::Null)]),
            ]),
        ),
    ]);
    let encoded = cbor::encode(&value).expect("encode");
    assert_eq!(cbor::decode(&encoded), Ok(value));
    assert_eq!(cbor::encode(&cbor::decode(&encoded).unwrap()), Ok(encoded));
}

#[test]
fn rejects_noncanonical_and_unsupported_bytes() {
    for hex in [
        "1800",       // shortest integer
        "5800",       // shortest byte length
        "7800",       // shortest text length
        "9800",       // shortest array length
        "b800",       // shortest map length
        "a201010101", // duplicate key
        "a201010001", // reversed key
        "a12001",     // negative map key
        "a1617801",   // text map key
        "c001",       // tag
        "f7",         // undefined
        "f93c00",     // float
        "9f01ff",     // indefinite array
        "61ff",       // invalid UTF-8
        "0102",       // trailing bytes
        "82",         // truncated array
    ] {
        assert!(cbor::decode(&hex_bytes(hex)).is_err(), "{hex}");
    }
}

#[test]
fn respects_input_and_nesting_limits() {
    assert_eq!(
        cbor::decode_with_limits(
            &hex_bytes("4100"),
            Limits {
                max_bytes: 1,
                max_depth: 16,
            }
        ),
        Err(Error::TooLarge)
    );
    assert_eq!(
        cbor::decode_with_limits(
            &hex_bytes("818101"),
            Limits {
                max_bytes: 3,
                max_depth: 1,
            }
        ),
        Err(Error::TooDeep)
    );
    assert_eq!(
        cbor::encode(&Value::Integer(i128::MAX)),
        Err(Error::IntegerOutOfRange)
    );
    assert_eq!(
        cbor::encode(&Value::Map(vec![(2, Value::Null), (1, Value::Null)])),
        Err(Error::UnsortedMapKeys)
    );
}
