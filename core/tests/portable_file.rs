use babytrack_core::{
    cbor::Value,
    operation::{Hlc, Kind, NewOperation, Scope},
    portable_file::{
        Error as FileError, PortableKind, export_readable_local, open_protected, parse_readable,
        protect_readable, restore_protected, restore_readable,
    },
    sqlite_store::SqliteStore,
};

fn hex(text: &str) -> Vec<u8> {
    text.as_bytes()
        .chunks_exact(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}
fn v4(tag: u8) -> [u8; 16] {
    let mut id = [tag; 16];
    id[6] = 0x40;
    id[8] = 0x80;
    id
}
fn v7(tag: u8) -> [u8; 16] {
    let mut id = [tag; 16];
    id[6] = 0x70;
    id[8] = 0x80;
    id
}

#[test]
fn published_readable_file_parses_and_rejects_corruption() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("../../tests/vectors/portable-file-v1.json")).unwrap();
    let item = fixture["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"] == "FILEBYTE08")
        .unwrap();
    let bytes = hex(item["input"]["readable_file_hex"].as_str().unwrap());
    let parsed = parse_readable(&bytes).unwrap();
    assert_eq!(parsed.source_cursor, Some(9));
    assert!(!parsed.known_gap);
    assert_eq!(parsed.rows.len(), 2);
    assert_eq!(parsed.rows[0].kind, PortableKind::Child);
    assert_eq!(parsed.rows[1].kind, PortableKind::Family);
    let temp = tempfile::tempdir().unwrap();
    let mut store = SqliteStore::open(temp.path().join("fixture.db")).unwrap();
    let restored = restore_readable(&mut store, &bytes, 1_790_420_000_001).unwrap();
    assert_ne!(restored.family_id, parsed.source_family_id);
    assert_eq!(
        store
            .restored_origin(restored)
            .unwrap()
            .unwrap()
            .source_cursor,
        Some(9)
    );
    let current = store.load_local(restored).unwrap();
    assert!(current.record(&parsed.rows[0].source_id).is_some());
    assert!(current.record(&restored.family_id).is_some());
    let gapped = String::from_utf8(bytes.clone()).unwrap().replacen(
        "\"known_gap\":false",
        "\"known_gap\":true",
        1,
    );
    let parsed_gap = parse_readable(gapped.as_bytes()).unwrap();
    assert!(parsed_gap.known_gap);
    let gap_copy = restore_readable(&mut store, gapped.as_bytes(), 1_790_420_000_002).unwrap();
    assert!(store.restored_origin(gap_copy).unwrap().unwrap().known_gap);
    let mut damaged = bytes.clone();
    let position = damaged
        .windows(4)
        .position(|window| window == b"rows")
        .unwrap();
    damaged[position + 7] = b'3';
    assert!(parse_readable(&damaged).is_err());
    let duplicate = String::from_utf8(bytes.clone()).unwrap().replacen(
        "\"version\":1",
        "\"version\":1,\"version\":1",
        1,
    );
    assert!(parse_readable(duplicate.as_bytes()).is_err());
    let exponent =
        String::from_utf8(bytes)
            .unwrap()
            .replacen("\"version\":1", "\"version\":1e0", 1);
    assert!(parse_readable(exponent.as_bytes()).is_err());
}

#[test]
fn published_protected_file_opens_and_rejects_tampering() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("../../tests/vectors/crypto-v1.json")).unwrap();
    let item = fixture["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"] == "FILEBYTE01")
        .unwrap();
    let protected = hex(item["protected_file_hex"].as_str().unwrap());
    let readable = hex(item["readable_file_hex"].as_str().unwrap());
    assert!(protected.starts_with(b"BTBK1"));
    assert_eq!(
        &protected[5..5 + item["protected_header_hex"].as_str().unwrap().len() / 2],
        hex(item["protected_header_hex"].as_str().unwrap())
    );
    assert_eq!(
        open_protected(
            &protected,
            item["password"].as_str().unwrap(),
            512 * 1024 * 1024
        )
        .unwrap(),
        readable
    );
    assert!(open_protected(&protected, "wrong", 512 * 1024 * 1024).is_err());
    let mut tampered = protected;
    *tampered.last_mut().unwrap() ^= 1;
    assert!(
        open_protected(
            &tampered,
            item["password"].as_str().unwrap(),
            512 * 1024 * 1024
        )
        .is_err()
    );
}

#[test]
fn newline_dense_file_rejects_before_unbounded_line_index() {
    let mut bytes = Vec::with_capacity(2_000_010);
    bytes.extend_from_slice(b"{}\n");
    bytes.extend(std::iter::repeat_n(b'\n', 1_000_004));
    assert!(parse_readable(&bytes).is_err());
}

#[test]
fn local_export_contains_current_child_and_fresh_family_metadata() {
    let temp = tempfile::tempdir().unwrap();
    let mut store = SqliteStore::open(temp.path().join("local.db")).unwrap();
    let family_id = v4(0x31);
    let device_id = v4(0x32);
    let family = store.create_family(family_id, device_id).unwrap();
    let child_id = v7(0x41);
    store
        .append_local(
            family,
            NewOperation {
                family_id,
                operation_id: v7(0x42),
                record_id: child_id,
                scope: Scope::Child,
                kind: Kind::Create,
                author_device_id: device_id,
                hlc: Hlc {
                    wall_ms: 0,
                    counter: 0,
                    device_id,
                },
                record_type: Some("child".to_owned()),
                child_id: None,
                fields: Some(vec![(1, Value::Text("Baby".to_owned()))]),
            },
            1000,
        )
        .unwrap();
    let readable = export_readable_local(&store, family, 2000).unwrap();
    let restored = parse_readable(&readable).unwrap();
    assert_eq!(restored.source_family_id, family_id);
    assert_eq!(restored.source_cursor, None);
    assert_eq!(restored.snapshot_utc_ms, 2000);
    assert_eq!(restored.rows.len(), 2);
    assert_eq!(restored.rows[0].source_id, child_id);
    assert_eq!(restored.rows[1].source_id, family_id);
    let copy = restore_readable(&mut store, &readable, 3000).unwrap();
    assert_ne!(copy.family_id, family_id);
    assert_ne!(copy.device_id, device_id);
    let copied = store.load_local(copy).unwrap();
    assert_eq!(
        copied.record(&child_id).unwrap().field(1).unwrap().value,
        Value::Text("Baby".to_owned())
    );
    assert!(copied.record(&copy.family_id).is_some());
    assert_eq!(
        store
            .restored_origin(copy)
            .unwrap()
            .unwrap()
            .source_family_id,
        family_id
    );
    assert_eq!(
        store
            .restored_origin(copy)
            .unwrap()
            .unwrap()
            .snapshot_utc_ms,
        2000
    );
    assert!(store.restored_origin(family).unwrap().is_none());

    let mut corrupt = readable;
    let position = corrupt
        .windows(7)
        .position(|bytes| bytes == b"ZEJhYnk")
        .unwrap();
    corrupt[position + 4] = b'Z';
    let before: i64 = rusqlite::Connection::open(temp.path().join("local.db"))
        .unwrap()
        .query_row("SELECT COUNT(*) FROM families", [], |row| row.get(0))
        .unwrap();
    assert!(restore_readable(&mut store, &corrupt, 4000).is_err());
    let after: i64 = rusqlite::Connection::open(temp.path().join("local.db"))
        .unwrap()
        .query_row("SELECT COUNT(*) FROM families", [], |row| row.get(0))
        .unwrap();
    assert_eq!(before, after);
}

#[test]
fn protected_backup_normalizes_password_and_rejects_tampering_atomically() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("protected.db");
    let mut store = SqliteStore::open(&path).unwrap();
    let family = store.create_family(v4(0x51), v4(0x52)).unwrap();
    let readable = export_readable_local(&store, family, 5_000).unwrap();
    assert!(matches!(
        protect_readable(&readable, "Café", 32 * 1024 * 1024),
        Err(FileError::ProtectedFailure)
    ));
    let protected = protect_readable(&readable, "Cafe\u{301}", 512 * 1024 * 1024).unwrap();
    assert!(protected.starts_with(b"BTBK1"));
    assert_eq!(
        open_protected(&protected, "Café", 512 * 1024 * 1024).unwrap(),
        readable
    );
    assert!(matches!(
        open_protected(&protected, "wrong", 512 * 1024 * 1024),
        Err(FileError::ProtectedFailure)
    ));
    let mut tampered = protected.clone();
    *tampered.last_mut().unwrap() ^= 1;
    assert!(matches!(
        open_protected(&tampered, "Café", 512 * 1024 * 1024),
        Err(FileError::ProtectedFailure)
    ));
    assert!(matches!(
        open_protected(&protected[..protected.len() - 1], "Café", 512 * 1024 * 1024),
        Err(FileError::ProtectedFailure)
    ));
    let restored =
        restore_protected(&mut store, &protected, "Café", 512 * 1024 * 1024, 6_000).unwrap();
    assert_ne!(restored.family_id, family.family_id);
    assert_eq!(
        store
            .restored_origin(restored)
            .unwrap()
            .unwrap()
            .snapshot_utc_ms,
        5_000
    );
}
