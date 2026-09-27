use babytrack_core::{
    batch::{self, AuthenticatedBatch, Header},
    cbor::{self, Value},
    crypto,
    operation::{Hlc, Kind, NewOperation, Operation, Scope},
    projection::{Outcome, Projection},
};

fn id(kind: u8, number: u8) -> [u8; 16] {
    let mut value = [0; 16];
    value[0] = kind;
    value[6] = 0x70;
    value[8] = 0x80;
    value[15] = number;
    value
}

fn v4(kind: u8, number: u8) -> [u8; 16] {
    let mut value = id(kind, number);
    value[6] = 0x40;
    value
}

fn next(state: &mut u64) -> u64 {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    *state
}

fn encrypted_operation(
    family: [u8; 16],
    author: [u8; 16],
    child: [u8; 16],
    cursor: u8,
    kind: Kind,
    clock: i64,
) -> AuthenticatedBatch {
    let operation = Operation::encode_new(&NewOperation {
        family_id: family,
        operation_id: id(2, cursor),
        record_id: child,
        scope: Scope::Child,
        kind,
        author_device_id: author,
        hlc: Hlc {
            wall_ms: clock,
            counter: 0,
            device_id: author,
        },
        record_type: (kind == Kind::Create).then(|| "child".to_owned()),
        child_id: None,
        fields: matches!(kind, Kind::Create | Kind::Set)
            .then(|| vec![(1, Value::Text(format!("child-{cursor}")))]),
    })
    .unwrap();
    let relay = [3; 32];
    let key = [4; 32];
    let seed = [5; 32];
    let mut nonce = [0; 24];
    nonce[0] = cursor;
    let plaintext_len = cbor::encode(&Value::Array(vec![Value::Bytes(operation.clone())]))
        .unwrap()
        .len()
        .try_into()
        .unwrap();
    let header = Header {
        minor: 0,
        family_id: family,
        relay_id: relay,
        control_head: [6; 32],
        epoch: 1,
        batch_id: v4(3, cursor),
        author_device_id: author,
        device_sequence: cursor.into(),
        nonce,
        plaintext_len,
    };
    let sealed = batch::seal(&header, &[operation], &key, &seed).unwrap();
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
fn seeded_encrypted_edits_rebuild_to_the_same_projection() {
    for seed in 1..=8u64 {
        let family = v4(7, seed as u8);
        let author = v4(8, seed as u8);
        let children = [id(9, 1), id(9, 2), id(9, 3), id(9, 4)];
        let mut rng = seed * 0x9e37_79b9;
        let mut deleted = [false; 4];
        let mut batches = Vec::new();
        let mut incremental = Projection::new(family);
        for cursor in 1..=64u8 {
            let (child, kind) = if cursor <= 4 {
                (usize::from(cursor - 1), Kind::Create)
            } else {
                let target = (next(&mut rng) % 4) as usize;
                let kind = if deleted[target] {
                    deleted[target] = false;
                    Kind::Restore
                } else if next(&mut rng).is_multiple_of(5) {
                    deleted[target] = true;
                    Kind::Delete
                } else {
                    Kind::Set
                };
                (target, kind)
            };
            let clock = if next(&mut rng).is_multiple_of(7) {
                i64::MAX - i64::from(cursor)
            } else {
                i64::from(cursor % 9)
            };
            let batch = encrypted_operation(family, author, children[child], cursor, kind, clock);
            let outcome = incremental
                .apply_authenticated(&batch, cursor.into())
                .unwrap();
            assert_eq!(outcome, Outcome::Applied, "seed {seed}, cursor {cursor}");
            batches.push(batch);
            if cursor % 8 == 0 {
                let mut rebuilt = Projection::new(family);
                for (index, entry) in batches.iter().enumerate() {
                    assert_eq!(
                        rebuilt
                            .apply_authenticated(entry, index as u64 + 1)
                            .unwrap(),
                        Outcome::Applied
                    );
                }
                assert_eq!(incremental, rebuilt, "seed {seed}, cursor {cursor}");
                assert!(
                    incremental
                        .apply_authenticated(batches.last().unwrap(), u64::from(cursor))
                        .is_err()
                );
                assert_eq!(incremental, rebuilt, "duplicate changed seed {seed}");
            }
        }
    }
}
