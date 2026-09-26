use babytrack_core::{
    cbor::Value,
    operation::{Hlc, Kind, NewOperation, Operation, Scope},
    projection::{LocalError, LocalProjection},
};

const FAMILY: [u8; 16] = [
    0x12, 0x3e, 0x45, 0x67, 0xe8, 0x9b, 0x42, 0xd3, 0xa4, 0x56, 0x42, 0x66, 0x14, 0x17, 0x40, 0x00,
];
const DEVICE: [u8; 16] = [
    0x72, 0x3e, 0x45, 0x67, 0xe8, 0x9b, 0x42, 0xd3, 0xa4, 0x56, 0x42, 0x66, 0x14, 0x17, 0x40, 0x00,
];

fn v7(suffix: u8) -> [u8; 16] {
    let mut id = [
        0x01, 0x83, 0xf9, 0xd0, 0, 0, 0x70, 0, 0x80, 0, 0, 0, 0, 0, 0, 0,
    ];
    id[15] = suffix;
    id
}

fn operation(
    operation_id: u8,
    record_id: [u8; 16],
    kind: Kind,
    fields: Option<Vec<(u64, Value)>>,
) -> Operation {
    let bytes = Operation::encode_new(&NewOperation {
        family_id: FAMILY,
        operation_id: v7(operation_id),
        record_id,
        scope: Scope::Child,
        kind,
        author_device_id: DEVICE,
        hlc: Hlc {
            wall_ms: 100 + i64::from(operation_id),
            counter: 0,
            device_id: DEVICE,
        },
        record_type: (kind == Kind::Create).then(|| "child".to_owned()),
        child_id: None,
        fields,
    })
    .unwrap();
    Operation::decode_bound(&bytes, &FAMILY, &DEVICE).unwrap()
}

#[test]
fn local_append_order_is_durable_replay_order_and_rejection_is_atomic() {
    let child = v7(20);
    let create = operation(
        1,
        child,
        Kind::Create,
        Some(vec![
            (1, Value::Text("Baby".to_owned())),
            (500, Value::Bool(false)),
        ]),
    );
    let rename = operation(
        2,
        child,
        Kind::Set,
        Some(vec![(1, Value::Text("New name".to_owned()))]),
    );
    let missing = operation(
        3,
        v7(21),
        Kind::Set,
        Some(vec![(1, Value::Text("No create".to_owned()))]),
    );
    let delete = operation(4, child, Kind::Delete, None);
    let restore = operation(5, child, Kind::Restore, None);
    let mut local = LocalProjection::new(FAMILY);

    assert_eq!(
        local.append(&missing, 1),
        Err(LocalError::Invalid("record does not exist"))
    );
    assert_eq!(local.last_append_index(), 0);
    assert!(local.record(&child).is_none());
    local.append(&create, 1).unwrap();
    assert_eq!(local.append(&rename, 3), Err(LocalError::WrongAppendIndex));
    assert_eq!(local.record(&child).unwrap().field(1).unwrap().cursor, 1);
    local.append(&rename, 2).unwrap();
    local.append(&delete, 3).unwrap();
    local.append(&restore, 4).unwrap();

    let record = local.record(&child).unwrap();
    assert!(!record.deleted);
    assert_eq!(
        record.field(1).unwrap().value,
        Value::Text("New name".to_owned())
    );
    assert_eq!(record.field(1).unwrap().cursor, 2);
    assert_eq!(record.field(500).unwrap().canonical_bytes, vec![0xf4]);

    let mut rebuilt = LocalProjection::new(FAMILY);
    for (index, operation) in [&create, &rename, &delete, &restore].iter().enumerate() {
        rebuilt.append(operation, index as u64 + 1).unwrap();
    }
    assert_eq!(rebuilt, local);

    let mut foreign = create.clone();
    foreign.family_id[0] ^= 1;
    assert_eq!(local.append(&foreign, 5), Err(LocalError::WrongFamily));
    assert_eq!(local.last_append_index(), 4);
}
