//! Canonical relay read requests and untrusted response envelopes. Public
//! history replay verifies every signed entry before it reaches local state.

use crate::{
    cbor::{self, Value},
    crypto,
};

#[derive(Debug)]
pub enum Error {
    Cbor(cbor::Error),
    Crypto(crypto::Error),
    #[cfg(not(target_arch = "wasm32"))]
    Random(getrandom::Error),
    Invalid(&'static str),
}
impl From<cbor::Error> for Error {
    fn from(v: cbor::Error) -> Self {
        Self::Cbor(v)
    }
}
impl From<crypto::Error> for Error {
    fn from(v: crypto::Error) -> Self {
        Self::Crypto(v)
    }
}

pub struct SignedRead {
    pub request_id: [u8; 16],
    pub bytes: Vec<u8>,
}

#[cfg(not(target_arch = "wasm32"))]
pub fn sign_get(
    family_id: [u8; 16],
    relay_id: [u8; 32],
    signer_id: [u8; 16],
    signing_seed: &[u8; 32],
    exact_path: &str,
) -> Result<SignedRead, Error> {
    let mut request_id = [0u8; 16];
    getrandom::fill(&mut request_id).map_err(Error::Random)?;
    Ok(SignedRead {
        request_id,
        bytes: sign_get_with_id(
            family_id,
            relay_id,
            signer_id,
            signing_seed,
            exact_path,
            request_id,
        )?,
    })
}

fn sign_get_with_id(
    family_id: [u8; 16],
    relay_id: [u8; 32],
    signer_id: [u8; 16],
    signing_seed: &[u8; 32],
    exact_path: &str,
    request_id: [u8; 16],
) -> Result<Vec<u8>, Error> {
    if !exact_path.starts_with("/v1/families/")
        || exact_path.contains('#')
        || exact_path.len() > 512
    {
        return Err(Error::Invalid("read path invalid"));
    }
    let request = cbor::encode(&Value::Array(vec![
        Value::Integer(1),
        Value::Bytes(family_id.to_vec()),
        Value::Bytes(relay_id.to_vec()),
        Value::Bytes(signer_id.to_vec()),
        Value::Bytes(request_id.to_vec()),
        Value::Text("GET".into()),
        Value::Text(exact_path.into()),
        Value::Bytes(crypto::hash("request-body", &[])?.to_vec()),
    ]))?;
    let signature = crypto::sign_cbor("read-request", &request, signing_seed)?;
    Ok(cbor::encode(&Value::Map(vec![
        (1, Value::Bytes(request)),
        (2, Value::Bytes(signature.to_vec())),
    ]))?)
}

pub struct ControlPage {
    pub entries: Vec<ControlEntry>,
    pub next_after: u64,
    pub has_more: bool,
}
pub struct ControlEntry {
    pub cursor: u64,
    pub committed_bytes: Vec<u8>,
}
impl ControlPage {
    pub fn decode(bytes: &[u8], family_id: [u8; 16], requested_after: u64) -> Result<Self, Error> {
        let value = cbor::decode_with_limits(
            bytes,
            cbor::Limits {
                max_bytes: 4 * 1024 * 1024,
                max_depth: 16,
            },
        )?;
        let fields = exact_map(&value, 6)?;
        if fields[0].1 != Value::Integer(1)
            || fields[1].1 != Value::Bytes(family_id.to_vec())
            || fields[2].1 != Value::Integer(requested_after.into())
        {
            return Err(Error::Invalid("control page context mismatch"));
        }
        let Value::Array(items) = &fields[3].1 else {
            return Err(Error::Invalid("control page entries not array"));
        };
        if items.len() > 256 {
            return Err(Error::Invalid("control page too many entries"));
        }
        let mut entries = Vec::with_capacity(items.len());
        let mut last = requested_after;
        for item in items {
            let Value::Array(parts) = item else {
                return Err(Error::Invalid("control entry not array"));
            };
            if parts.len() != 3 || parts[1] != Value::Integer(1) {
                return Err(Error::Invalid("control entry kind invalid"));
            }
            let cursor = number(&parts[0])?;
            if cursor <= last {
                return Err(Error::Invalid("control cursor not increasing"));
            }
            let Value::Bytes(committed) = &parts[2] else {
                return Err(Error::Invalid("control bytes not bytes"));
            };
            cbor::decode(committed)?;
            entries.push(ControlEntry {
                cursor,
                committed_bytes: committed.clone(),
            });
            last = cursor;
        }
        if number(&fields[4].1)? != last {
            return Err(Error::Invalid("page next cursor mismatch"));
        }
        let Value::Bool(has_more) = fields[5].1 else {
            return Err(Error::Invalid("page has_more not bool"));
        };
        Ok(Self {
            entries,
            next_after: last,
            has_more,
        })
    }
}

pub struct LogPage {
    pub entries: Vec<LogEntry>,
    pub next_after: u64,
    pub has_more: bool,
}
pub struct LogEntry {
    pub cursor: u64,
    pub kind: u8,
    pub committed_bytes: Vec<u8>,
}
impl LogPage {
    pub fn decode(bytes: &[u8], family_id: [u8; 16], requested_after: u64) -> Result<Self, Error> {
        let value = cbor::decode_with_limits(
            bytes,
            cbor::Limits {
                max_bytes: 4 * 1024 * 1024,
                max_depth: 16,
            },
        )?;
        let fields = exact_map(&value, 6)?;
        if fields[0].1 != Value::Integer(1)
            || fields[1].1 != Value::Bytes(family_id.to_vec())
            || fields[2].1 != Value::Integer(requested_after.into())
        {
            return Err(Error::Invalid("log page context mismatch"));
        }
        let Value::Array(items) = &fields[3].1 else {
            return Err(Error::Invalid("log entries not array"));
        };
        if items.len() > 256 {
            return Err(Error::Invalid("log page too many entries"));
        }
        let mut entries = Vec::with_capacity(items.len());
        let mut last = requested_after;
        for item in items {
            let Value::Array(parts) = item else {
                return Err(Error::Invalid("log entry not array"));
            };
            if parts.len() != 3 {
                return Err(Error::Invalid("log entry length"));
            }
            let cursor = number(&parts[0])?;
            if cursor
                != last
                    .checked_add(1)
                    .ok_or(Error::Invalid("log cursor overflow"))?
            {
                return Err(Error::Invalid("log cursor gap"));
            }
            let kind: u8 = number(&parts[1])?
                .try_into()
                .map_err(|_| Error::Invalid("log kind range"))?;
            if kind != 1 && kind != 2 {
                return Err(Error::Invalid("log kind invalid"));
            }
            let Value::Bytes(committed) = &parts[2] else {
                return Err(Error::Invalid("log bytes not bytes"));
            };
            cbor::decode(committed)?;
            entries.push(LogEntry {
                cursor,
                kind,
                committed_bytes: committed.clone(),
            });
            last = cursor;
        }
        if number(&fields[4].1)? != last {
            return Err(Error::Invalid("log page next cursor mismatch"));
        }
        let Value::Bool(has_more) = fields[5].1 else {
            return Err(Error::Invalid("log page has_more not bool"));
        };
        Ok(Self {
            entries,
            next_after: last,
            has_more,
        })
    }
}

pub struct BatchResult {
    pub receipt_bytes: Option<Vec<u8>>,
}
impl BatchResult {
    pub fn decode(bytes: &[u8]) -> Result<Self, Error> {
        let value = cbor::decode_with_limits(
            bytes,
            cbor::Limits {
                max_bytes: 4096,
                max_depth: 8,
            },
        )?;
        let fields = exact_map(&value, 2)?;
        if fields[0].1 != Value::Integer(1) {
            return Err(Error::Invalid("batch result version"));
        }
        let receipt_bytes = match &fields[1].1 {
            Value::Null => None,
            Value::Bytes(bytes) => {
                cbor::decode(bytes)?;
                Some(bytes.clone())
            }
            _ => return Err(Error::Invalid("batch result shape")),
        };
        Ok(Self { receipt_bytes })
    }
}

pub struct OpaqueObject {
    pub kind: u16,
    pub object_id: [u8; 16],
    pub object_bytes: Vec<u8>,
    pub transition_id: [u8; 16],
}
impl OpaqueObject {
    pub fn decode(bytes: &[u8], expected_id: [u8; 16]) -> Result<Self, Error> {
        let value = cbor::decode_with_limits(
            bytes,
            cbor::Limits {
                max_bytes: 1024 * 1024 + 128,
                max_depth: 16,
            },
        )?;
        let fields = exact_map(&value, 5)?;
        if fields[0].1 != Value::Integer(1) {
            return Err(Error::Invalid("object response version"));
        }
        let kind: u16 = number(&fields[1].1)?
            .try_into()
            .map_err(|_| Error::Invalid("object kind range"))?;
        let object_id = fixed::<16>(&fields[2].1)?;
        if object_id != expected_id {
            return Err(Error::Invalid("object response ID mismatch"));
        }
        let Value::Bytes(object_bytes) = &fields[3].1 else {
            return Err(Error::Invalid("object not bytes"));
        };
        if object_bytes.len() > 1024 * 1024 {
            return Err(Error::Invalid("object too large"));
        }
        let transition_id = fixed::<16>(&fields[4].1)?;
        Ok(Self {
            kind,
            object_id,
            object_bytes: object_bytes.clone(),
            transition_id,
        })
    }
}

fn exact_map(value: &Value, count: usize) -> Result<&[(u64, Value)], Error> {
    let Value::Map(fields) = value else {
        return Err(Error::Invalid("expected map"));
    };
    if fields.len() != count
        || fields
            .iter()
            .enumerate()
            .any(|(i, (key, _))| *key != i as u64 + 1)
    {
        return Err(Error::Invalid("map keys"));
    }
    Ok(fields)
}
fn fixed<const N: usize>(value: &Value) -> Result<[u8; N], Error> {
    let Value::Bytes(bytes) = value else {
        return Err(Error::Invalid("expected bytes"));
    };
    bytes
        .as_slice()
        .try_into()
        .map_err(|_| Error::Invalid("bytes length"))
}
fn number(value: &Value) -> Result<u64, Error> {
    let Value::Integer(number) = value else {
        return Err(Error::Invalid("expected integer"));
    };
    (*number)
        .try_into()
        .map_err(|_| Error::Invalid("unsigned integer"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value as Json;
    fn vector() -> Json {
        serde_json::from_str(&std::fs::read_to_string("../tests/vectors/api-v1.json").unwrap())
            .unwrap()
    }
    fn chain() -> Json {
        serde_json::from_str(
            &std::fs::read_to_string("../tests/vectors/contiguous-chain-v1.json").unwrap(),
        )
        .unwrap()
    }
    fn hex(value: &str) -> Vec<u8> {
        (0..value.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&value[i..i + 2], 16).unwrap())
            .collect()
    }
    #[test]
    fn signed_read_and_control_page_match_api_fixture() {
        let api = vector();
        let chain = chain();
        let family: [u8; 16] = hex(chain["test_only_inputs"]["family_id_hex"].as_str().unwrap())
            .try_into()
            .unwrap();
        let seed: [u8; 32] = hex(chain["test_only_inputs"]["manager_sign_seed_hex"]
            .as_str()
            .unwrap())
        .try_into()
        .unwrap();
        let manager: [u8; 16] = hex(chain["test_only_inputs"]["manager_device_id_hex"]
            .as_str()
            .unwrap())
        .try_into()
        .unwrap();
        let expected = hex(api["inputs"]["read_auth_cbor_hex"].as_str().unwrap());
        let Value::Map(auth) = cbor::decode(&expected).unwrap() else {
            panic!()
        };
        let Value::Bytes(request) = &auth[0].1 else {
            panic!()
        };
        let Value::Array(parts) = cbor::decode(request).unwrap() else {
            panic!()
        };
        let Value::Bytes(relay) = &parts[2] else {
            panic!()
        };
        let Value::Bytes(id) = &parts[4] else {
            panic!()
        };
        let actual = sign_get_with_id(
            family,
            relay.as_slice().try_into().unwrap(),
            manager,
            &seed,
            api["inputs"]["read_control_path"].as_str().unwrap(),
            id.as_slice().try_into().unwrap(),
        )
        .unwrap();
        assert_eq!(actual, expected);
        let expected_page = hex(api["expect"]["control_page_cbor_hex"].as_str().unwrap());
        let page = ControlPage::decode(&expected_page, family, 1).unwrap();
        assert_eq!(page.entries.len(), 1);
        assert_eq!(page.entries[0].cursor, 2);
        assert_eq!(page.next_after, 2);
        assert!(!page.has_more);
        assert!(ControlPage::decode(&expected_page, family, 0).is_err());
        let object_id: [u8; 16] = hex("083e4567e89b42d3a456426614174000").try_into().unwrap();
        let object = OpaqueObject::decode(
            &hex(api["expect"]["object_response_cbor_hex"].as_str().unwrap()),
            object_id,
        )
        .unwrap();
        assert_eq!(object.kind, 1);
        assert!(
            OpaqueObject::decode(
                &hex(api["expect"]["object_response_cbor_hex"].as_str().unwrap()),
                [0; 16]
            )
            .is_err()
        );
    }
}
