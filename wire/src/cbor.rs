//! The restricted deterministic CBOR subset used by protocol v1.
//!
//! Schema-specific limits and required keys belong to the calling decoder.
//! This layer rejects noncanonical bytes before they can be interpreted.

use std::fmt;

const DEFAULT_MAX_BYTES: usize = 1024 * 1024;
const DEFAULT_MAX_DEPTH: usize = 16;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    Integer(i128),
    Bytes(Vec<u8>),
    Text(String),
    Array(Vec<Value>),
    /// Keys are unsigned integers in ascending canonical order.
    Map(Vec<(u64, Value)>),
    Bool(bool),
    Null,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    TooLarge,
    TooDeep,
    Truncated,
    TrailingBytes,
    UnsupportedType,
    Noncanonical,
    InvalidUtf8,
    InvalidMapKey,
    UnsortedMapKeys,
    IntegerOutOfRange,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for Error {}

#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub max_bytes: usize,
    pub max_depth: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_bytes: DEFAULT_MAX_BYTES,
            max_depth: DEFAULT_MAX_DEPTH,
        }
    }
}

pub fn decode(bytes: &[u8]) -> Result<Value, Error> {
    decode_with_limits(bytes, Limits::default())
}

pub fn decode_with_limits(bytes: &[u8], limits: Limits) -> Result<Value, Error> {
    if bytes.len() > limits.max_bytes {
        return Err(Error::TooLarge);
    }
    let mut decoder = Decoder { bytes, offset: 0 };
    let value = decoder.value(0, limits.max_depth)?;
    if decoder.offset != bytes.len() {
        return Err(Error::TrailingBytes);
    }
    Ok(value)
}

/// Decode one canonical CBOR value from the start of a larger byte stream.
/// The caller must define and validate the format of any following bytes.
pub fn decode_prefix_with_limits(bytes: &[u8], limits: Limits) -> Result<(Value, usize), Error> {
    let bounded = &bytes[..bytes.len().min(limits.max_bytes)];
    let mut decoder = Decoder {
        bytes: bounded,
        offset: 0,
    };
    let value = decoder.value(0, limits.max_depth)?;
    Ok((value, decoder.offset))
}

pub fn encode(value: &Value) -> Result<Vec<u8>, Error> {
    let mut bytes = Vec::new();
    write_value(&mut bytes, value, 0, DEFAULT_MAX_DEPTH)?;
    Ok(bytes)
}

fn write_head(bytes: &mut Vec<u8>, major: u8, number: u64) {
    let prefix = major << 5;
    match number {
        0..=23 => bytes.push(prefix | number as u8),
        24..=0xff => bytes.extend([prefix | 24, number as u8]),
        0x100..=0xffff => {
            bytes.push(prefix | 25);
            bytes.extend(number.to_be_bytes()[6..].iter().copied());
        }
        0x1_0000..=0xffff_ffff => {
            bytes.push(prefix | 26);
            bytes.extend(number.to_be_bytes()[4..].iter().copied());
        }
        _ => {
            bytes.push(prefix | 27);
            bytes.extend(number.to_be_bytes());
        }
    }
}

fn write_value(
    bytes: &mut Vec<u8>,
    value: &Value,
    depth: usize,
    max_depth: usize,
) -> Result<(), Error> {
    if depth > max_depth {
        return Err(Error::TooDeep);
    }
    match value {
        Value::Integer(integer) if *integer >= 0 => {
            write_head(
                bytes,
                0,
                (*integer)
                    .try_into()
                    .map_err(|_| Error::IntegerOutOfRange)?,
            );
        }
        Value::Integer(integer) => {
            let magnitude = (-1 - integer)
                .try_into()
                .map_err(|_| Error::IntegerOutOfRange)?;
            write_head(bytes, 1, magnitude);
        }
        Value::Bytes(data) => {
            write_head(
                bytes,
                2,
                data.len().try_into().map_err(|_| Error::TooLarge)?,
            );
            bytes.extend(data);
        }
        Value::Text(text) => {
            write_head(
                bytes,
                3,
                text.len().try_into().map_err(|_| Error::TooLarge)?,
            );
            bytes.extend(text.as_bytes());
        }
        Value::Array(items) => {
            write_head(
                bytes,
                4,
                items.len().try_into().map_err(|_| Error::TooLarge)?,
            );
            for item in items {
                write_value(bytes, item, depth + 1, max_depth)?;
            }
        }
        Value::Map(entries) => {
            write_head(
                bytes,
                5,
                entries.len().try_into().map_err(|_| Error::TooLarge)?,
            );
            let mut previous = None;
            for (key, value) in entries {
                if previous.is_some_and(|prior| *key <= prior) {
                    return Err(Error::UnsortedMapKeys);
                }
                write_head(bytes, 0, *key);
                write_value(bytes, value, depth + 1, max_depth)?;
                previous = Some(*key);
            }
        }
        Value::Bool(false) => bytes.push(0xf4),
        Value::Bool(true) => bytes.push(0xf5),
        Value::Null => bytes.push(0xf6),
    }
    Ok(())
}

struct Decoder<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl Decoder<'_> {
    fn take(&mut self, count: usize) -> Result<&[u8], Error> {
        let end = self.offset.checked_add(count).ok_or(Error::TooLarge)?;
        let data = self.bytes.get(self.offset..end).ok_or(Error::Truncated)?;
        self.offset = end;
        Ok(data)
    }

    fn number(&mut self, additional: u8) -> Result<u64, Error> {
        let (count, minimum) = match additional {
            0..=23 => return Ok(u64::from(additional)),
            24 => (1, 24),
            25 => (2, 0x100),
            26 => (4, 0x1_0000),
            27 => (8, 0x1_0000_0000),
            _ => return Err(Error::UnsupportedType),
        };
        let mut number = 0u64;
        for byte in self.take(count)? {
            number = (number << 8) | u64::from(*byte);
        }
        if number < minimum {
            return Err(Error::Noncanonical);
        }
        Ok(number)
    }

    fn value(&mut self, depth: usize, max_depth: usize) -> Result<Value, Error> {
        if depth > max_depth {
            return Err(Error::TooDeep);
        }
        let initial = *self.take(1)?.first().ok_or(Error::Truncated)?;
        let major = initial >> 5;
        let additional = initial & 31;
        match major {
            0 => Ok(Value::Integer(i128::from(self.number(additional)?))),
            1 => Ok(Value::Integer(-1 - i128::from(self.number(additional)?))),
            2 | 3 => {
                let length: usize = self
                    .number(additional)?
                    .try_into()
                    .map_err(|_| Error::TooLarge)?;
                let data = self.take(length)?;
                if major == 2 {
                    Ok(Value::Bytes(data.to_vec()))
                } else {
                    Ok(Value::Text(
                        std::str::from_utf8(data)
                            .map_err(|_| Error::InvalidUtf8)?
                            .to_owned(),
                    ))
                }
            }
            4 | 5 => {
                let count: usize = self
                    .number(additional)?
                    .try_into()
                    .map_err(|_| Error::TooLarge)?;
                // Every array element needs at least one byte; each map pair
                // needs at least two. Bound allocation by the remaining input.
                let minimum_size = if major == 4 { 1 } else { 2 };
                if count > (self.bytes.len() - self.offset) / minimum_size {
                    return Err(Error::Truncated);
                }
                if major == 4 {
                    let mut items = Vec::with_capacity(count);
                    for _ in 0..count {
                        items.push(self.value(depth + 1, max_depth)?);
                    }
                    Ok(Value::Array(items))
                } else {
                    let mut entries = Vec::with_capacity(count);
                    let mut previous = None;
                    for _ in 0..count {
                        let key = match self.value(depth + 1, max_depth)? {
                            Value::Integer(number) => {
                                u64::try_from(number).map_err(|_| Error::InvalidMapKey)?
                            }
                            _ => return Err(Error::InvalidMapKey),
                        };
                        if previous.is_some_and(|prior| key <= prior) {
                            return Err(Error::UnsortedMapKeys);
                        }
                        let value = self.value(depth + 1, max_depth)?;
                        entries.push((key, value));
                        previous = Some(key);
                    }
                    Ok(Value::Map(entries))
                }
            }
            7 => match additional {
                20 => Ok(Value::Bool(false)),
                21 => Ok(Value::Bool(true)),
                22 => Ok(Value::Null),
                _ => Err(Error::UnsupportedType),
            },
            _ => Err(Error::UnsupportedType),
        }
    }
}
