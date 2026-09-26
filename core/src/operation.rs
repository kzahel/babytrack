//! Structural decoding for version-1 plaintext operations.
//!
//! History-dependent checks (membership, prior creates, duplicate operation
//! IDs, and field projection) belong to batch replay, not this decoder.

use crate::cbor::{self, Limits, Value};

const MAX_OPERATION_BYTES: usize = 64 * 1024;
const MAX_TEXT_BYTES: usize = 16 * 1024;
const MAX_FIELDS: usize = 128;
const MAX_HLC_COUNTER: u64 = 1_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    Family,
    Child,
    Activity,
}

impl Scope {
    fn wire(self) -> u64 {
        match self {
            Self::Family => 1,
            Self::Child => 2,
            Self::Activity => 3,
        }
    }

    fn from_wire(number: u64) -> Option<Self> {
        match number {
            1 => Some(Self::Family),
            2 => Some(Self::Child),
            3 => Some(Self::Activity),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Create,
    Set,
    Delete,
    Restore,
}

impl Kind {
    fn wire(self) -> u64 {
        match self {
            Self::Create => 1,
            Self::Set => 2,
            Self::Delete => 3,
            Self::Restore => 4,
        }
    }

    fn from_wire(number: u64) -> Option<Self> {
        match number {
            1 => Some(Self::Create),
            2 => Some(Self::Set),
            3 => Some(Self::Delete),
            4 => Some(Self::Restore),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hlc {
    pub wall_ms: i64,
    pub counter: u32,
    pub device_id: [u8; 16],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Operation {
    pub family_id: [u8; 16],
    pub operation_id: [u8; 16],
    pub record_id: [u8; 16],
    pub scope: Scope,
    pub kind: Kind,
    pub author_device_id: [u8; 16],
    pub hlc: Hlc,
    pub record_type: Option<String>,
    pub child_id: Option<[u8; 16]>,
    pub fields: Vec<(u64, Value)>,
    /// The original canonical bytes, including unknown top-level keys.
    raw: Vec<u8>,
}

/// Caller-allocated IDs and HLC for one new operation. The storage/outbox
/// transaction must durably reserve these values before network use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewOperation {
    pub family_id: [u8; 16],
    pub operation_id: [u8; 16],
    pub record_id: [u8; 16],
    pub scope: Scope,
    pub kind: Kind,
    pub author_device_id: [u8; 16],
    pub hlc: Hlc,
    pub record_type: Option<String>,
    pub child_id: Option<[u8; 16]>,
    pub fields: Option<Vec<(u64, Value)>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    Cbor(cbor::Error),
    Invalid(&'static str),
    WrongFamily,
    WrongAuthor,
    UnsupportedVersion,
}

impl From<cbor::Error> for Error {
    fn from(error: cbor::Error) -> Self {
        Self::Cbor(error)
    }
}

impl Operation {
    pub fn encode_new(new: &NewOperation) -> Result<Vec<u8>, Error> {
        let mut map = vec![
            (1, Value::Integer(1)),
            (2, Value::Bytes(new.family_id.to_vec())),
            (3, Value::Bytes(new.operation_id.to_vec())),
            (4, Value::Bytes(new.record_id.to_vec())),
            (5, Value::Integer(new.scope.wire().into())),
            (6, Value::Integer(new.kind.wire().into())),
            (7, Value::Bytes(new.author_device_id.to_vec())),
            (
                8,
                Value::Array(vec![
                    Value::Integer(new.hlc.wall_ms.into()),
                    Value::Integer(new.hlc.counter.into()),
                    Value::Bytes(new.hlc.device_id.to_vec()),
                ]),
            ),
        ];
        if let Some(record_type) = &new.record_type {
            map.push((9, Value::Text(record_type.clone())));
        }
        if let Some(child_id) = &new.child_id {
            map.push((10, Value::Bytes(child_id.to_vec())));
        }
        if let Some(fields) = &new.fields {
            map.push((11, Value::Map(fields.clone())));
        }
        let bytes = cbor::encode(&Value::Map(map))?;
        Self::decode_bound(&bytes, &new.family_id, &new.author_device_id)?;
        Ok(bytes)
    }

    /// Decode only in the context of the verified batch Family and signer.
    pub fn decode_bound(
        bytes: &[u8],
        family_id: &[u8; 16],
        author_device_id: &[u8; 16],
    ) -> Result<Self, Error> {
        let value = cbor::decode_with_limits(
            bytes,
            Limits {
                max_bytes: MAX_OPERATION_BYTES,
                max_depth: 16,
            },
        )?;
        let Value::Map(map) = value else {
            return Err(Error::Invalid("operation must be a map"));
        };
        let version = unsigned(required(&map, 1)?, "version must be unsigned")?;
        if version != 1 {
            return Err(Error::UnsupportedVersion);
        }
        let decoded_family = bytes16(required(&map, 2)?, "Family ID must be 16 bytes")?;
        if &decoded_family != family_id {
            return Err(Error::WrongFamily);
        }
        let operation_id = bytes16(required(&map, 3)?, "operation ID must be 16 bytes")?;
        if operation_id[6] >> 4 != 7 || operation_id[8] >> 6 != 2 {
            return Err(Error::Invalid("operation ID must be UUIDv7"));
        }
        let record_id = bytes16(required(&map, 4)?, "record ID must be 16 bytes")?;
        let scope = Scope::from_wire(unsigned(required(&map, 5)?, "invalid scope")?)
            .ok_or(Error::Invalid("invalid scope"))?;
        let kind = Kind::from_wire(unsigned(required(&map, 6)?, "invalid kind")?)
            .ok_or(Error::Invalid("invalid kind"))?;
        let decoded_author = bytes16(required(&map, 7)?, "author ID must be 16 bytes")?;
        if &decoded_author != author_device_id {
            return Err(Error::WrongAuthor);
        }
        let hlc = parse_hlc(required(&map, 8)?, &decoded_author)?;
        let record_type = optional(&map, 9)
            .map(|value| match value {
                Value::Text(text) => Ok(text.clone()),
                _ => Err(Error::Invalid("record type must be text")),
            })
            .transpose()?;
        let child_id = optional(&map, 10)
            .map(|value| bytes16(value, "child ID must be 16 bytes"))
            .transpose()?;
        let fields = optional(&map, 11)
            .map(|value| match value {
                Value::Map(fields) if fields.len() <= MAX_FIELDS => {
                    if fields.iter().any(|(key, _)| *key == 0) {
                        return Err(Error::Invalid("field IDs must be positive"));
                    }
                    Ok(fields.clone())
                }
                _ => Err(Error::Invalid("changed fields must be a bounded map")),
            })
            .transpose()?;

        match kind {
            Kind::Create => {
                if record_type.is_none() || fields.is_none() {
                    return Err(Error::Invalid("create requires type and fields"));
                }
                if (scope == Scope::Activity) != child_id.is_some() {
                    return Err(Error::Invalid("child ID only on activity create"));
                }
            }
            Kind::Set => {
                if record_type.is_some() || child_id.is_some() {
                    return Err(Error::Invalid("set cannot change type or child"));
                }
                if fields.as_ref().is_none_or(|fields| fields.is_empty()) {
                    return Err(Error::Invalid("set requires changed fields"));
                }
            }
            Kind::Delete | Kind::Restore => {
                if record_type.is_some() || child_id.is_some() || fields.is_some() {
                    return Err(Error::Invalid("delete/restore cannot change fields"));
                }
            }
        }
        if scope == Scope::Family && record_id != decoded_family {
            return Err(Error::Invalid("Family record ID must equal Family ID"));
        }
        if let Some(record_type) = &record_type {
            match scope {
                Scope::Family if record_type != "family" => {
                    return Err(Error::Invalid("Family scope requires family type"));
                }
                Scope::Child if record_type != "child" => {
                    return Err(Error::Invalid("child scope requires child type"));
                }
                Scope::Activity if record_type == "family" || record_type == "child" => {
                    return Err(Error::Invalid("reserved type in activity scope"));
                }
                _ => {}
            }
        }
        // Unknown positive top-level keys are retained verbatim in `raw`.
        if map.iter().any(|(key, _)| *key == 0) {
            return Err(Error::Invalid("operation keys must be positive"));
        }
        for (_, value) in &map {
            check_text_limits(value)?;
        }

        Ok(Self {
            family_id: decoded_family,
            operation_id,
            record_id,
            scope,
            kind,
            author_device_id: decoded_author,
            hlc,
            record_type,
            child_id,
            fields: fields.unwrap_or_default(),
            raw: bytes.to_vec(),
        })
    }

    pub fn canonical_bytes(&self) -> &[u8] {
        &self.raw
    }

    /// Canonical bytes for a field, including fields unknown to this client.
    pub fn field_bytes(&self, field_id: u64) -> Option<Vec<u8>> {
        self.fields
            .iter()
            .find(|(key, _)| *key == field_id)
            .map(|(_, value)| cbor::encode(value).expect("decoded canonical value"))
    }
}

fn required(map: &[(u64, Value)], key: u64) -> Result<&Value, Error> {
    optional(map, key).ok_or(Error::Invalid("missing operation field"))
}

fn optional(map: &[(u64, Value)], key: u64) -> Option<&Value> {
    map.iter()
        .find(|(current, _)| *current == key)
        .map(|(_, value)| value)
}

fn unsigned(value: &Value, message: &'static str) -> Result<u64, Error> {
    match value {
        Value::Integer(number) => u64::try_from(*number).map_err(|_| Error::Invalid(message)),
        _ => Err(Error::Invalid(message)),
    }
}

fn bytes16(value: &Value, message: &'static str) -> Result<[u8; 16], Error> {
    match value {
        Value::Bytes(bytes) => bytes
            .as_slice()
            .try_into()
            .map_err(|_| Error::Invalid(message)),
        _ => Err(Error::Invalid(message)),
    }
}

fn parse_hlc(value: &Value, author_device_id: &[u8; 16]) -> Result<Hlc, Error> {
    let Value::Array(parts) = value else {
        return Err(Error::Invalid("HLC must be a three-element array"));
    };
    if parts.len() != 3 {
        return Err(Error::Invalid("HLC must be a three-element array"));
    }
    let Value::Integer(wall) = parts[0] else {
        return Err(Error::Invalid("HLC wall must be signed i64"));
    };
    let wall_ms = i64::try_from(wall).map_err(|_| Error::Invalid("HLC wall outside i64"))?;
    let counter = unsigned(&parts[1], "HLC counter must be unsigned")?;
    if counter > MAX_HLC_COUNTER {
        return Err(Error::Invalid("HLC counter too large"));
    }
    let device_id = bytes16(&parts[2], "HLC device ID must be 16 bytes")?;
    if &device_id != author_device_id {
        return Err(Error::Invalid("HLC device ID differs from author"));
    }
    Ok(Hlc {
        wall_ms,
        counter: counter as u32,
        device_id,
    })
}

fn check_text_limits(value: &Value) -> Result<(), Error> {
    match value {
        Value::Text(text) if text.len() > MAX_TEXT_BYTES => Err(Error::Invalid("text too large")),
        Value::Array(items) => {
            for item in items {
                check_text_limits(item)?;
            }
            Ok(())
        }
        Value::Map(entries) => {
            for (_, item) in entries {
                check_text_limits(item)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}
