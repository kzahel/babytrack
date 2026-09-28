//! Storage-independent initial epoch data readiness shared by native and wasm.

use std::collections::{BTreeMap, BTreeSet};

use crate::{
    cbor::{self, Value},
    control_chain::{self, ControlChain},
    crypto,
    operation::Operation,
    projection::{self, Projection, VerifiedEpochKey},
    sync_wire::{self, OpaqueObject},
};

#[derive(Debug)]
pub enum Error {
    Cbor(cbor::Error),
    Control(control_chain::Error),
    Crypto(crypto::Error),
    Projection(projection::Error),
    Wire(sync_wire::Error),
    Invalid(&'static str),
}
impl From<cbor::Error> for Error {
    fn from(value: cbor::Error) -> Self {
        Self::Cbor(value)
    }
}
impl From<control_chain::Error> for Error {
    fn from(value: control_chain::Error) -> Self {
        Self::Control(value)
    }
}
impl From<crypto::Error> for Error {
    fn from(value: crypto::Error) -> Self {
        Self::Crypto(value)
    }
}
impl From<projection::Error> for Error {
    fn from(value: projection::Error) -> Self {
        Self::Projection(value)
    }
}
impl From<sync_wire::Error> for Error {
    fn from(value: sync_wire::Error) -> Self {
        Self::Wire(value)
    }
}

pub struct ManifestObjectRef {
    pub id: [u8; 16],
    pub kind: u16,
    pub transition_id: [u8; 16],
    pub hash: [u8; 32],
    pub length: usize,
}

pub fn manifest_objects(committed_bytes: &[u8]) -> Result<Vec<ManifestObjectRef>, Error> {
    let value = cbor::decode_with_limits(
        committed_bytes,
        cbor::Limits {
            max_bytes: 1024 * 1024,
            max_depth: 16,
        },
    )?;
    let Value::Map(root) = value else {
        return Err(Error::Invalid("control not map"));
    };
    let Some((1, Value::Map(unsigned))) = root.first() else {
        return Err(Error::Invalid("unsigned control absent"));
    };
    let Some((5, Value::Bytes(transition))) = unsigned.get(4) else {
        return Err(Error::Invalid("transition ID absent"));
    };
    let transition_id: [u8; 16] = transition
        .as_slice()
        .try_into()
        .map_err(|_| Error::Invalid("transition ID length"))?;
    let Some((10, Value::Array(manifest))) = unsigned.get(9) else {
        return Err(Error::Invalid("control manifest absent"));
    };
    let mut result = Vec::with_capacity(manifest.len());
    let mut seen = BTreeSet::new();
    for item in manifest {
        let Value::Array(fields) = item else {
            return Err(Error::Invalid("manifest entry not array"));
        };
        if fields.len() != 4 {
            return Err(Error::Invalid("manifest entry length"));
        }
        let Value::Integer(kind) = fields[0] else {
            return Err(Error::Invalid("manifest kind absent"));
        };
        let kind = u16::try_from(kind).map_err(|_| Error::Invalid("manifest kind range"))?;
        let id = fixed_bytes::<16>(&fields[1])?;
        if !seen.insert(id) {
            return Err(Error::Invalid("manifest object ID repeated"));
        }
        let hash = fixed_bytes::<32>(&fields[2])?;
        let length = positive_or_zero(&fields[3])?
            .try_into()
            .map_err(|_| Error::Invalid("manifest object length range"))?;
        result.push(ManifestObjectRef {
            id,
            kind,
            transition_id,
            hash,
            length,
        });
    }
    Ok(result)
}

pub fn verified_object_from_response(
    committed_bytes: &[u8],
    object_id: [u8; 16],
    response_bytes: &[u8],
) -> Result<Vec<u8>, Error> {
    let expected = manifest_objects(committed_bytes)?
        .into_iter()
        .find(|object| object.id == object_id)
        .ok_or(Error::Invalid("object ID absent from signed manifest"))?;
    let actual = OpaqueObject::decode(response_bytes, object_id)?;
    if actual.kind != expected.kind
        || actual.transition_id != expected.transition_id
        || actual.object_bytes.len() != expected.length
        || crypto::hash("object", &actual.object_bytes)? != expected.hash
    {
        return Err(Error::Invalid("object differs from signed manifest"));
    }
    Ok(actual.object_bytes)
}

/// Construct the epoch-one data view from signed genesis and every object in
/// its manifest. This storage-independent entry point is shared by native
/// ready replay and browser wasm; a public cursor alone never supplies data
/// readiness or an epoch key.
pub fn initial_epoch_projection(
    genesis_bytes: &[u8],
    relay_public_key: [u8; 32],
    initial_epoch_key: [u8; 32],
    objects: &BTreeMap<[u8; 16], Vec<u8>>,
) -> Result<(ControlChain, Projection, VerifiedEpochKey), Error> {
    let genesis = crate::control::verify_genesis(genesis_bytes, &relay_public_key)
        .map_err(control_chain::Error::Control)?;
    let chain = ControlChain::from_genesis(genesis_bytes, relay_public_key)?;
    let key = chain.verify_initial_epoch_key(&initial_epoch_key)?;
    verify_manifest(genesis_bytes, objects)?;
    let mut projection = Projection::new(genesis.family_id());
    replay_promotion(
        genesis_bytes,
        objects,
        genesis.family_id(),
        genesis.relay_id(),
        genesis.manager_device_id(),
        &initial_epoch_key,
        &mut projection,
    )?;
    Ok((chain, projection, key))
}

pub fn verify_manifest(
    committed_bytes: &[u8],
    objects: &BTreeMap<[u8; 16], Vec<u8>>,
) -> Result<(), Error> {
    for entry in manifest_objects(committed_bytes)? {
        let object = objects
            .get(&entry.id)
            .ok_or(Error::Invalid("committed object not downloaded"))?;
        if entry.length != object.len() || entry.hash != crypto::hash("object", object)? {
            return Err(Error::Invalid(
                "committed object bytes differ from manifest",
            ));
        }
    }
    Ok(())
}

fn replay_promotion(
    genesis_bytes: &[u8],
    objects: &BTreeMap<[u8; 16], Vec<u8>>,
    family_id: [u8; 16],
    relay_id: [u8; 32],
    manager_id: [u8; 16],
    epoch_key: &[u8; 32],
    projection: &mut Projection,
) -> Result<(), Error> {
    let genesis = cbor::decode(genesis_bytes)?;
    let Value::Map(root) = genesis else {
        return Err(Error::Invalid("genesis not map"));
    };
    let Value::Map(unsigned) = &root[0].1 else {
        return Err(Error::Invalid("unsigned genesis not map"));
    };
    let Value::Array(signed_objects) = &unsigned[9].1 else {
        return Err(Error::Invalid("genesis object list not array"));
    };
    let Value::Array(first_object) = &signed_objects[0] else {
        return Err(Error::Invalid("promotion manifest entry not array"));
    };
    let manifest_id = fixed_bytes::<16>(&first_object[1])?;
    let manifest_bytes = objects
        .get(&manifest_id)
        .ok_or(Error::Invalid("promotion manifest missing"))?;
    let manifest = cbor::decode(manifest_bytes)?;
    let Value::Map(fields) = manifest else {
        return Err(Error::Invalid("promotion manifest not map"));
    };
    if fields.len() != 6
        || fields
            .iter()
            .enumerate()
            .any(|(i, (key, _))| *key != i as u64 + 1)
        || fields[0].1 != Value::Integer(1)
        || fixed_bytes::<16>(&fields[1].1)? != family_id
        || fixed_bytes::<32>(&fields[2].1)? != relay_id
        || fixed_bytes::<16>(&fields[3].1)? != manifest_id
    {
        return Err(Error::Invalid("promotion manifest context"));
    }
    let watermark = positive_or_zero(&fields[4].1)?;
    let Value::Array(rows) = &fields[5].1 else {
        return Err(Error::Invalid("promotion rows not array"));
    };
    if rows.len() + 1 != signed_objects.len() || rows.len() > 16_383 {
        return Err(Error::Invalid("promotion object count"));
    }
    let signed_chunk_ids = signed_objects[1..]
        .iter()
        .map(|entry| {
            let Value::Array(fields) = entry else {
                return Err(Error::Invalid("signed chunk row not array"));
            };
            fixed_bytes::<16>(&fields[1])
        })
        .collect::<Result<std::collections::BTreeSet<_>, Error>>()?;
    let mut referenced = std::collections::BTreeSet::new();
    let mut operations = Vec::new();
    let mut next = 1u64;
    for (index, row) in rows.iter().enumerate() {
        let Value::Array(parts) = row else {
            return Err(Error::Invalid("promotion chunk row not array"));
        };
        if parts.len() != 6 || positive_or_zero(&parts[0])? != index as u64 {
            return Err(Error::Invalid("promotion chunk index"));
        }
        let object_id = fixed_bytes::<16>(&parts[1])?;
        if !referenced.insert(object_id) || !signed_chunk_ids.contains(&object_id) {
            return Err(Error::Invalid("promotion chunk not in signed genesis"));
        }
        let object = objects
            .get(&object_id)
            .ok_or(Error::Invalid("promotion chunk missing"))?;
        if fixed_bytes::<32>(&parts[2])? != crypto::hash("object", object)?
            || positive_or_zero(&parts[3])? != object.len() as u64
            || positive_or_zero(&parts[4])? != next
        {
            return Err(Error::Invalid("promotion chunk hash or range"));
        }
        let last = positive_or_zero(&parts[5])?;
        if last < next {
            return Err(Error::Invalid("promotion chunk empty range"));
        }
        let Value::Map(chunk) = cbor::decode(object)? else {
            return Err(Error::Invalid("promotion chunk not map"));
        };
        if chunk.len() != 3
            || chunk[0].0 != 1
            || chunk[1].0 != 2
            || chunk[2].0 != 3
            || chunk[0].1 != Value::Integer(1)
        {
            return Err(Error::Invalid("promotion chunk shape"));
        }
        let Value::Array(header) = &chunk[1].1 else {
            return Err(Error::Invalid("promotion header not array"));
        };
        if header.len() != 6
            || fixed_bytes::<16>(&header[0])? != family_id
            || fixed_bytes::<32>(&header[1])? != relay_id
            || fixed_bytes::<16>(&header[2])? != manifest_id
            || positive_or_zero(&header[3])? != index as u64
            || header[4] != Value::Integer(1)
        {
            return Err(Error::Invalid("promotion header context"));
        }
        let nonce = fixed_bytes::<24>(&header[5])?;
        let Value::Bytes(ciphertext) = &chunk[2].1 else {
            return Err(Error::Invalid("promotion ciphertext not bytes"));
        };
        let aad = crypto::hash("promotion-aad", &cbor::encode(&chunk[1].1)?)?;
        let plaintext = crypto::open(epoch_key, &nonce, &aad, ciphertext)?;
        if plaintext.len() > 256 * 1024 {
            return Err(Error::Invalid("promotion plaintext too large"));
        }
        let Value::Array(encoded_ops) = cbor::decode(&plaintext)? else {
            return Err(Error::Invalid("promotion plaintext not array"));
        };
        if encoded_ops.is_empty()
            || encoded_ops.len() > 256
            || encoded_ops.len() as u64 != last - next + 1
        {
            return Err(Error::Invalid("promotion operation count"));
        }
        for encoded in encoded_ops {
            let Value::Bytes(bytes) = encoded else {
                return Err(Error::Invalid("promotion operation not bytes"));
            };
            operations.push(
                Operation::decode_bound(&bytes, &family_id, &manager_id)
                    .map_err(|_| Error::Invalid("promotion operation invalid"))?,
            );
        }
        next = last
            .checked_add(1)
            .ok_or(Error::Invalid("promotion index overflow"))?;
    }
    if referenced != signed_chunk_ids || watermark != next - 1 {
        return Err(Error::Invalid("promotion history incomplete"));
    }
    projection.apply_promotion(&operations)?;
    Ok(())
}

fn fixed_bytes<const N: usize>(value: &Value) -> Result<[u8; N], Error> {
    let Value::Bytes(bytes) = value else {
        return Err(Error::Invalid("expected bytes"));
    };
    bytes
        .as_slice()
        .try_into()
        .map_err(|_| Error::Invalid("byte length"))
}

fn positive_or_zero(value: &Value) -> Result<u64, Error> {
    let Value::Integer(number) = value else {
        return Err(Error::Invalid("expected integer"));
    };
    (*number)
        .try_into()
        .map_err(|_| Error::Invalid("negative or large integer"))
}
