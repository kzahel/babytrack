#![cfg(not(target_arch = "wasm32"))]

use std::{
    fs,
    path::PathBuf,
    time::{Duration, SystemTime},
};

use babytrack_core::{
    cbor::Value,
    operation::{Hlc, Kind, NewOperation, Operation, Scope},
    sqlite_store::{Error, FamilyHandle, SqliteStore},
};

fn v4(suffix: u8) -> [u8; 16] {
    let mut id = [
        0x12, 0x3e, 0x45, 0x67, 0xe8, 0x9b, 0x42, 0xd3, 0xa4, 0x56, 0x42, 0x66, 0x14, 0x17, 0x40, 0,
    ];
    id[15] = suffix;
    id
}

fn v7(suffix: u8) -> [u8; 16] {
    let mut id = [
        0x01, 0x83, 0xf9, 0xd0, 0, 0, 0x70, 0, 0x80, 0, 0, 0, 0, 0, 0, 0,
    ];
    id[15] = suffix;
    id
}

fn temp_db() -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "babytrack-local-{}-{nonce}.sqlite",
        std::process::id()
    ))
}

#[test]
fn concurrent_first_open_initializes_one_complete_schema() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("fresh.db");
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(8));
    let openings: Vec<_> = (0..8)
        .map(|_| {
            let path = path.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                SqliteStore::open(path).and_then(|store| store.families())
            })
        })
        .collect();
    for opening in openings {
        assert!(opening.join().unwrap().is_ok());
    }
    let connection = rusqlite::Connection::open(path).unwrap();
    let version: u32 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .unwrap();
    assert_eq!(version, 4);
}

fn child_operation(
    family: FamilyHandle,
    op: u8,
    child: u8,
    kind: Kind,
    name: &str,
) -> NewOperation {
    NewOperation {
        family_id: family.family_id,
        operation_id: v7(op),
        record_id: v7(child),
        scope: Scope::Child,
        kind,
        author_device_id: family.device_id,
        // The store replaces this placeholder inside its append transaction.
        hlc: Hlc {
            wall_ms: 0,
            counter: 0,
            device_id: family.device_id,
        },
        record_type: (kind == Kind::Create).then(|| "child".to_owned()),
        child_id: None,
        fields: Some(vec![(1, Value::Text(name.to_owned()))]),
    }
}

#[test]
fn second_connection_waits_for_background_writer() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("busy.db");
    SqliteStore::open(&path).unwrap();
    let mut writer = rusqlite::Connection::open(&path).unwrap();
    let transaction = writer
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .unwrap();
    let other_path = path.clone();
    let (started, receiver) = std::sync::mpsc::channel();
    let opening = std::thread::spawn(move || {
        started.send(()).unwrap();
        let mut store = SqliteStore::open(other_path).unwrap();
        store.create_family(v4(70), v4(71)).unwrap();
    });
    receiver.recv().unwrap();
    std::thread::sleep(Duration::from_millis(250));
    transaction.commit().unwrap();
    opening.join().unwrap();
    let store = SqliteStore::open(&path).unwrap();
    assert!(
        store
            .families()
            .unwrap()
            .iter()
            .any(|family| family.family_id == v4(70))
    );
}

#[test]
fn opening_initialized_store_does_not_write_while_another_writer_is_active() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("read-during-write.db");
    SqliteStore::open(&path).unwrap();
    let mut writer = rusqlite::Connection::open(&path).unwrap();
    let transaction = writer
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .unwrap();
    let (sender, receiver) = std::sync::mpsc::channel();
    let opening = std::thread::spawn(move || {
        let result = SqliteStore::open(path).and_then(|store| store.families());
        sender.send(result).unwrap();
    });
    let result = receiver.recv_timeout(Duration::from_secs(2));
    transaction.commit().unwrap();
    opening.join().unwrap();
    assert!(
        result
            .expect("opening an initialized store waited for a writer")
            .is_ok()
    );
}

#[test]
fn committed_local_edits_rebuild_after_reopen_and_failed_edits_leave_no_gap() {
    let path = temp_db();
    let first = FamilyHandle {
        family_id: v4(1),
        device_id: v4(11),
    };
    let second = FamilyHandle {
        family_id: v4(2),
        device_id: v4(12),
    };
    {
        let mut store = SqliteStore::open(&path).unwrap();
        assert_eq!(
            store
                .create_family(first.family_id, first.device_id)
                .unwrap(),
            first
        );
        assert_eq!(
            store
                .create_family(second.family_id, second.device_id)
                .unwrap(),
            second
        );
        let create = store
            .append_local(
                first,
                child_operation(first, 1, 30, Kind::Create, "Baby"),
                100,
            )
            .unwrap();
        assert_eq!(create.index, 1);
        let decoded =
            Operation::decode_bound(&create.bytes, &first.family_id, &first.device_id).unwrap();
        assert_eq!(decoded.hlc.wall_ms, 100);
        assert_eq!(decoded.hlc.counter, 0);

        let missing = child_operation(first, 2, 31, Kind::Set, "No create");
        assert!(matches!(
            store.append_local(first, missing, 1000),
            Err(Error::Projection(_))
        ));
        assert_eq!(store.load_local(first).unwrap().last_append_index(), 1);
        let rename = store
            .append_local(
                first,
                child_operation(first, 3, 30, Kind::Set, "Renamed"),
                90,
            )
            .unwrap();
        assert_eq!(rename.index, 2);
        let decoded =
            Operation::decode_bound(&rename.bytes, &first.family_id, &first.device_id).unwrap();
        assert_eq!((decoded.hlc.wall_ms, decoded.hlc.counter), (100, 1));
        assert!(matches!(
            store.append_local(
                first,
                child_operation(first, 3, 30, Kind::Set, "Different"),
                110
            ),
            Err(Error::DuplicateOperation)
        ));
        assert_eq!(store.load_local(first).unwrap().last_append_index(), 2);
        assert_eq!(store.load_local(second).unwrap().last_append_index(), 0);
        assert!(matches!(
            store.load_local(FamilyHandle {
                device_id: second.device_id,
                ..first
            }),
            Err(Error::WrongDevice)
        ));
    }
    {
        let store = SqliteStore::open(&path).unwrap();
        let projection = store.load_local(first).unwrap();
        assert_eq!(projection.last_append_index(), 2);
        assert_eq!(
            projection.record(&v7(30)).unwrap().field(1).unwrap().value,
            Value::Text("Renamed".to_owned())
        );
        assert!(store.load_local(second).unwrap().record(&v7(30)).is_none());
    }
    fs::remove_file(&path).unwrap();
}
