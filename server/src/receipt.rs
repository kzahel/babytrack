//! Exact relay-signed receipt and page encodings. These constructors remain
//! internal until the server's authority validator and transaction call them.

use crate::batch_authority::VerifiedBatch;
use babytrack_wire::{
    cbor::{self, Value},
    crypto,
};

#[derive(Debug)]
pub enum Error {
    Cbor(cbor::Error),
    Crypto(crypto::Error),
    Invalid(&'static str),
}
impl From<cbor::Error> for Error {
    fn from(value: cbor::Error) -> Self {
        Self::Cbor(value)
    }
}
impl From<crypto::Error> for Error {
    fn from(value: crypto::Error) -> Self {
        Self::Crypto(value)
    }
}

#[derive(Debug, Clone)]
pub struct RelayEntry {
    pub cursor: u64,
    pub kind: u8,
    pub committed_bytes: Vec<u8>,
}

#[derive(Debug)]
pub(crate) struct VerifiedControlReceipt {
    pub candidate_bytes: Vec<u8>,
    pub family_id: [u8; 16],
    pub relay_id: [u8; 32],
    pub cursor: u64,
}

/// Authenticate stored committed control bytes before they influence relay
/// authority reconstruction. The candidate and receipt context must agree.
pub(crate) fn verify_control_receipt(
    committed_bytes: &[u8],
    relay_public: &[u8; 32],
) -> Result<VerifiedControlReceipt, Error> {
    let value = cbor::decode_with_limits(
        committed_bytes,
        cbor::Limits {
            max_bytes: 1024 * 1024,
            max_depth: 16,
        },
    )?;
    let Value::Map(root) = value else {
        return Err(Error::Invalid("committed control not map"));
    };
    if root.len() != 4
        || root
            .iter()
            .enumerate()
            .any(|(i, (key, _))| *key != i as u64 + 1)
    {
        return Err(Error::Invalid("committed control keys"));
    }
    let Value::Map(unsigned) = &root[0].1 else {
        return Err(Error::Invalid("unsigned control not map"));
    };
    if unsigned.len() != 11
        || unsigned
            .iter()
            .enumerate()
            .any(|(i, (key, _))| *key != i as u64 + 1)
    {
        return Err(Error::Invalid("unsigned control keys"));
    }
    let family_id = fixed::<16>(&unsigned[1].1)?;
    let relay_id = fixed::<32>(&unsigned[2].1)?;
    let transition_id = fixed::<16>(&unsigned[4].1)?;
    let Value::Array(receipt) = &root[2].1 else {
        return Err(Error::Invalid("control receipt not array"));
    };
    if receipt.len() != 6
        || fixed::<16>(&receipt[0])? != family_id
        || fixed::<32>(&receipt[1])? != relay_id
        || fixed::<16>(&receipt[2])? != transition_id
    {
        return Err(Error::Invalid("control receipt context"));
    }
    let Value::Integer(cursor) = &receipt[3] else {
        return Err(Error::Invalid("control cursor not integer"));
    };
    let cursor: u64 = (*cursor)
        .try_into()
        .map_err(|_| Error::Invalid("control cursor range"))?;
    if cursor == 0 {
        return Err(Error::Invalid("zero control cursor"));
    }
    let Value::Integer(committed_ms) = &receipt[4] else {
        return Err(Error::Invalid("control time not integer"));
    };
    let _: i64 = (*committed_ms)
        .try_into()
        .map_err(|_| Error::Invalid("control time range"))?;
    let signed_hash = crypto::hash(
        "control-signed",
        &cbor::encode(&Value::Array(vec![root[0].1.clone(), root[1].1.clone()]))?,
    )?;
    if fixed::<32>(&receipt[5])? != signed_hash {
        return Err(Error::Invalid("control signed hash"));
    }
    crypto::verify_cbor(
        "control-receipt",
        &cbor::encode(&root[2].1)?,
        relay_public,
        &fixed::<64>(&root[3].1)?,
    )?;
    Ok(VerifiedControlReceipt {
        candidate_bytes: cbor::encode(&Value::Map(vec![
            (1, root[0].1.clone()),
            (2, root[1].1.clone()),
        ]))?,
        family_id,
        relay_id,
        cursor,
    })
}

#[allow(dead_code)] // Called by the authority transaction in the next relay slice.
pub(crate) fn commit_control(
    candidate_bytes: &[u8],
    relay_seed: &[u8; 32],
    cursor: u64,
    committed_ms: i64,
) -> Result<Vec<u8>, Error> {
    if cursor == 0 {
        return Err(Error::Invalid("zero commit cursor"));
    }
    let candidate = cbor::decode(candidate_bytes)?;
    let Value::Map(candidate) = candidate else {
        return Err(Error::Invalid("candidate not map"));
    };
    if candidate.len() != 2 || candidate[0].0 != 1 || candidate[1].0 != 2 {
        return Err(Error::Invalid("candidate keys"));
    }
    let unsigned = candidate[0].1.clone();
    let signatures = candidate[1].1.clone();
    let Value::Map(fields) = &unsigned else {
        return Err(Error::Invalid("unsigned transition not map"));
    };
    if fields.len() != 11
        || fields
            .iter()
            .enumerate()
            .any(|(i, (key, _))| *key != i as u64 + 1)
    {
        return Err(Error::Invalid("unsigned transition keys"));
    }
    let family_id = fixed::<16>(&fields[1].1)?;
    let relay_id = fixed::<32>(&fields[2].1)?;
    let transition_id = fixed::<16>(&fields[4].1)?;
    let signed_hash = crypto::hash(
        "control-signed",
        &cbor::encode(&Value::Array(vec![unsigned.clone(), signatures.clone()]))?,
    )?;
    let receipt = Value::Array(vec![
        Value::Bytes(family_id.to_vec()),
        Value::Bytes(relay_id.to_vec()),
        Value::Bytes(transition_id.to_vec()),
        Value::Integer(cursor.into()),
        Value::Integer(committed_ms.into()),
        Value::Bytes(signed_hash.to_vec()),
    ]);
    let receipt_bytes = cbor::encode(&receipt)?;
    let relay_signature = crypto::sign_cbor("control-receipt", &receipt_bytes, relay_seed)?;
    Ok(cbor::encode(&Value::Map(vec![
        (1, unsigned),
        (2, signatures),
        (3, receipt),
        (4, Value::Bytes(relay_signature.to_vec())),
    ]))?)
}

#[allow(dead_code)] // Called by the authority transaction in the next relay slice.
pub(crate) fn control_commit_response(committed_bytes: &[u8]) -> Result<Vec<u8>, Error> {
    Ok(cbor::encode(&Value::Map(vec![
        (1, Value::Integer(1)),
        (2, Value::Bytes(committed_bytes.to_vec())),
    ]))?)
}

#[allow(dead_code)] // Called by the object staging route in the next relay slice.
pub(crate) fn object_stage_response(object_bytes: &[u8]) -> Result<Vec<u8>, Error> {
    let hash = crypto::hash("object", object_bytes)?;
    Ok(cbor::encode(&Value::Map(vec![
        (1, Value::Integer(1)),
        (2, Value::Bytes(hash.to_vec())),
        (3, Value::Bool(true)),
    ]))?)
}

pub fn encode_control_page(
    family_id: [u8; 16],
    after: u64,
    entries: &[RelayEntry],
    has_more: bool,
) -> Result<Vec<u8>, Error> {
    encode_filtered_page(family_id, after, entries, has_more, 1)
}

pub(crate) fn encode_batch_page(
    family_id: [u8; 16],
    after: u64,
    entries: &[RelayEntry],
    has_more: bool,
) -> Result<Vec<u8>, Error> {
    encode_filtered_page(family_id, after, entries, has_more, 2)
}

fn encode_filtered_page(
    family_id: [u8; 16],
    after: u64,
    entries: &[RelayEntry],
    has_more: bool,
    kind: u8,
) -> Result<Vec<u8>, Error> {
    if entries.len() > 256 {
        return Err(Error::Invalid("page too large"));
    }
    let mut last = after;
    let mut encoded = Vec::with_capacity(entries.len());
    for entry in entries {
        if entry.kind != kind || entry.cursor <= last {
            return Err(Error::Invalid("filtered entries out of order"));
        }
        cbor::decode(&entry.committed_bytes)?;
        encoded.push(Value::Array(vec![
            Value::Integer(entry.cursor.into()),
            Value::Integer(entry.kind.into()),
            Value::Bytes(entry.committed_bytes.clone()),
        ]));
        last = entry.cursor;
    }
    let page = cbor::encode(&Value::Map(vec![
        (1, Value::Integer(1)),
        (2, Value::Bytes(family_id.to_vec())),
        (3, Value::Integer(after.into())),
        (4, Value::Array(encoded)),
        (5, Value::Integer(last.into())),
        (6, Value::Bool(has_more)),
    ]))?;
    if page.len() > 4 * 1024 * 1024 {
        return Err(Error::Invalid("page bytes too large"));
    }
    Ok(page)
}

pub(crate) fn encode_log_page(
    family_id: [u8; 16],
    after: u64,
    entries: &[RelayEntry],
    has_more: bool,
) -> Result<Vec<u8>, Error> {
    if entries.len() > 256 {
        return Err(Error::Invalid("page too large"));
    }
    let mut last = after;
    let mut encoded = Vec::with_capacity(entries.len());
    for entry in entries {
        if (entry.kind != 1 && entry.kind != 2) || entry.cursor != last.saturating_add(1) {
            return Err(Error::Invalid("log cursor gap or kind"));
        }
        cbor::decode(&entry.committed_bytes)?;
        encoded.push(Value::Array(vec![
            Value::Integer(entry.cursor.into()),
            Value::Integer(entry.kind.into()),
            Value::Bytes(entry.committed_bytes.clone()),
        ]));
        last = entry.cursor;
    }
    let page = cbor::encode(&Value::Map(vec![
        (1, Value::Integer(1)),
        (2, Value::Bytes(family_id.to_vec())),
        (3, Value::Integer(after.into())),
        (4, Value::Array(encoded)),
        (5, Value::Integer(last.into())),
        (6, Value::Bool(has_more)),
    ]))?;
    if page.len() > 4 * 1024 * 1024 {
        return Err(Error::Invalid("page bytes too large"));
    }
    Ok(page)
}

pub(crate) fn accepted_batch(
    batch: &VerifiedBatch,
    cursor: u64,
    relay_seed: &[u8; 32],
) -> Result<Vec<u8>, Error> {
    if cursor == 0 || batch.sequence == 0 || batch.sequence == u64::MAX {
        return Err(Error::Invalid("batch cursor or sequence"));
    }
    let body = Value::Map(vec![
        (1, Value::Integer(1)),
        (2, Value::Bytes(batch.family_id.to_vec())),
        (3, Value::Bytes(batch.relay_id.to_vec())),
        (4, Value::Bytes(batch.batch_id.to_vec())),
        (5, Value::Bytes(batch.object_hash.to_vec())),
        (6, Value::Bool(true)),
        (7, Value::Integer(cursor.into())),
        (8, Value::Bytes(batch.control_head.to_vec())),
        (9, Value::Integer(batch.sequence.into())),
        (10, Value::Null),
        (11, Value::Integer((batch.sequence + 1).into())),
    ]);
    let signed = crypto::sign_cbor("batch-receipt", &cbor::encode(&body)?, relay_seed)?;
    Ok(cbor::encode(&Value::Map(vec![
        (1, body),
        (2, Value::Bytes(signed.to_vec())),
    ]))?)
}

pub(crate) fn rejected_batch(
    batch: &VerifiedBatch,
    cursor: u64,
    current_head: [u8; 32],
    reason: u16,
    next_expected_sequence: u64,
    relay_seed: &[u8; 32],
) -> Result<Vec<u8>, Error> {
    if cursor == 0
        || batch.sequence == 0
        || next_expected_sequence == 0
        || !(1..=5).contains(&reason)
    {
        return Err(Error::Invalid("rejected batch cursor, sequence, or reason"));
    }
    let body = Value::Map(vec![
        (1, Value::Integer(1)),
        (2, Value::Bytes(batch.family_id.to_vec())),
        (3, Value::Bytes(batch.relay_id.to_vec())),
        (4, Value::Bytes(batch.batch_id.to_vec())),
        (5, Value::Bytes(batch.object_hash.to_vec())),
        (6, Value::Bool(false)),
        (7, Value::Integer(cursor.into())),
        (8, Value::Bytes(current_head.to_vec())),
        (9, Value::Integer(batch.sequence.into())),
        (10, Value::Integer(reason.into())),
        (11, Value::Integer(next_expected_sequence.into())),
    ]);
    let signed = crypto::sign_cbor("batch-receipt", &cbor::encode(&body)?, relay_seed)?;
    Ok(cbor::encode(&Value::Map(vec![
        (1, body),
        (2, Value::Bytes(signed.to_vec())),
    ]))?)
}

pub(crate) fn batch_commit_response(receipt_bytes: &[u8]) -> Result<Vec<u8>, Error> {
    Ok(cbor::encode(&Value::Map(vec![
        (1, Value::Integer(1)),
        (2, Value::Bytes(receipt_bytes.to_vec())),
    ]))?)
}

#[allow(dead_code)] // Used by commit_control after authority wiring.
fn fixed<const N: usize>(value: &Value) -> Result<[u8; N], Error> {
    let Value::Bytes(bytes) = value else {
        return Err(Error::Invalid("expected bytes"));
    };
    bytes
        .as_slice()
        .try_into()
        .map_err(|_| Error::Invalid("wrong bytes length"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value as Json;

    fn vector(name: &str) -> Json {
        let path = format!("../tests/vectors/{name}.json");
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }
    fn hex(value: &str) -> Vec<u8> {
        (0..value.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&value[i..i + 2], 16).unwrap())
            .collect()
    }
    fn field<'a>(json: &'a Json, path: &[&str]) -> &'a str {
        let mut current = json;
        for part in path {
            current = &current[*part];
        }
        current.as_str().unwrap()
    }

    #[test]
    fn api_genesis_and_issue_receipts_match_published_bytes() {
        let chain = vector("contiguous-chain-v1");
        let seed: [u8; 32] = hex(field(&chain, &["test_only_inputs", "relay_sign_seed_hex"]))
            .try_into()
            .unwrap();
        for (name, candidate_key) in [
            ("api-genesis-v1", "commit_candidate_cbor_hex"),
            ("api-v1", "commit_body_cbor_hex"),
        ] {
            let api = vector(name);
            let candidate = hex(field(&api, &["inputs", candidate_key]));
            let expected = hex(field(&api, &["expect", "commit_response_cbor_hex"]));
            let Value::Map(response) = cbor::decode(&expected).unwrap() else {
                panic!()
            };
            let Value::Bytes(expected_committed) = &response[1].1 else {
                panic!()
            };
            let Value::Map(committed) = cbor::decode(expected_committed).unwrap() else {
                panic!()
            };
            let Value::Array(receipt) = &committed[2].1 else {
                panic!()
            };
            let Value::Integer(cursor) = receipt[3] else {
                panic!()
            };
            let Value::Integer(time) = receipt[4] else {
                panic!()
            };
            let actual = commit_control(
                &candidate,
                &seed,
                cursor.try_into().unwrap(),
                time.try_into().unwrap(),
            )
            .unwrap();
            assert_eq!(actual, *expected_committed);
            assert_eq!(control_commit_response(&actual).unwrap(), expected);
        }
    }

    #[test]
    fn object_stage_and_control_page_match_published_bytes() {
        let api = vector("api-v1");
        let stage = hex(field(&api, &["inputs", "stage_body_cbor_hex"]));
        let Value::Map(fields) = cbor::decode(&stage).unwrap() else {
            panic!()
        };
        let Value::Bytes(object) = &fields[5].1 else {
            panic!()
        };
        assert_eq!(
            object_stage_response(object).unwrap(),
            hex(field(&api, &["expect", "stage_response_cbor_hex"]))
        );
        let expected = hex(field(&api, &["expect", "control_page_cbor_hex"]));
        let Value::Map(page) = cbor::decode(&expected).unwrap() else {
            panic!()
        };
        let Value::Bytes(family) = &page[1].1 else {
            panic!()
        };
        let Value::Integer(after) = page[2].1 else {
            panic!()
        };
        let Value::Array(entries) = &page[3].1 else {
            panic!()
        };
        let mut parsed = Vec::new();
        for entry in entries {
            let Value::Array(parts) = entry else { panic!() };
            let Value::Integer(cursor) = parts[0] else {
                panic!()
            };
            let Value::Integer(kind) = parts[1] else {
                panic!()
            };
            let Value::Bytes(bytes) = &parts[2] else {
                panic!()
            };
            parsed.push(RelayEntry {
                cursor: cursor.try_into().unwrap(),
                kind: kind.try_into().unwrap(),
                committed_bytes: bytes.clone(),
            });
        }
        let Value::Bool(has_more) = page[5].1 else {
            panic!()
        };
        assert_eq!(
            encode_control_page(
                family.as_slice().try_into().unwrap(),
                after.try_into().unwrap(),
                &parsed,
                has_more
            )
            .unwrap(),
            expected
        );
    }
}
