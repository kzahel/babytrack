use super::*;
use crate::operation::{Hlc, Kind, Scope};

fn v4(suffix: u8) -> [u8; 16] {
    let mut id = [0u8; 16];
    id[6] = 0x40;
    id[8] = 0x80;
    id[15] = suffix;
    id
}

fn v7(suffix: u8) -> [u8; 16] {
    let mut id = [0u8; 16];
    id[6] = 0x70;
    id[8] = 0x80;
    id[15] = suffix;
    id
}

#[test]
fn staged_batch_retries_exact_bytes_after_reopen_and_does_not_expose_failed_write() {
    let path = std::env::temp_dir().join(format!(
        "babytrack-outbox-{}-{}.sqlite",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let family = FamilyHandle {
        family_id: v4(1),
        device_id: v4(2),
    };
    let operation = NewOperation {
        family_id: family.family_id,
        operation_id: v7(1),
        record_id: v7(2),
        scope: Scope::Child,
        kind: Kind::Create,
        author_device_id: family.device_id,
        hlc: Hlc {
            wall_ms: 0,
            counter: 0,
            device_id: family.device_id,
        },
        record_type: Some("child".to_owned()),
        child_id: None,
        fields: Some(vec![(1, Value::Text("Baby".to_owned()))]),
    };
    let relay = [3u8; 32];
    let head = [4u8; 32];
    let key = [5u8; 32];
    let seed = [6u8; 32];
    let first;
    {
        let mut store = SqliteStore::open(&path).unwrap();
        store
            .create_family(family.family_id, family.device_id)
            .unwrap();
        assert!(matches!(
            store.stage_next_batch(family, relay, head, 1, &key, &seed),
            Err(Error::NoUnsentOperation)
        ));
        store.append_local(family, operation.clone(), 100).unwrap();
        first = store
            .stage_next_batch(family, relay, head, 1, &key, &seed)
            .unwrap();
        assert_eq!(first.from_index, 1);
        assert_eq!(first.sequence, 1);
        assert!(ids::is_v4(&first.batch_id));
        assert_eq!(store.pending_batch(family).unwrap(), Some(first.clone()));
        let signer = crate::crypto::signing_public_key(&seed);
        assert!(
            batch::open_verified(
                &first.envelope_bytes,
                &family.family_id,
                &relay,
                &key,
                &signer
            )
            .is_ok()
        );
        let mut invalid = operation.clone();
        invalid.operation_id = v7(3);
        invalid.record_id = v7(4);
        invalid.kind = Kind::Set;
        invalid.record_type = None;
        assert!(matches!(
            store.append_local(family, invalid, 101),
            Err(Error::Projection(_))
        ));
    }
    {
        let mut store = SqliteStore::open(&path).unwrap();
        assert_eq!(store.pending_batch(family).unwrap(), Some(first.clone()));
        // A changed head/key must not silently re-encrypt an uncertain batch.
        let retry = store
            .stage_next_batch(family, relay, [8u8; 32], 2, &[9u8; 32], &seed)
            .unwrap();
        assert_eq!(retry, first);
        assert_eq!(store.load_local(family).unwrap().last_append_index(), 1);
    }
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(path.with_extension("sqlite-wal"));
    let _ = std::fs::remove_file(path.with_extension("sqlite-shm"));
}
