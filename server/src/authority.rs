//! Public-only verification before the relay can reserve a new Family.
//! No event schema, payload decryption, or client key material lives here.

use babytrack_wire::{
    cbor::{self, Value},
    crypto,
};
use sha2::{Digest, Sha256};

#[derive(Debug)]
#[allow(dead_code)] // Error detail is consumed by the upcoming route boundary.
pub(crate) enum Error {
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
#[allow(dead_code)] // Consumed by the genesis reservation transaction.
pub(crate) struct GenesisCandidate {
    pub family_id: [u8; 16],
    pub relay_id: [u8; 32],
    pub transition_id: [u8; 16],
    pub manager_id: [u8; 16],
    pub manager_signing_key: [u8; 32],
    pub manifest: Vec<ManifestEntry>,
}

#[derive(Debug, Clone)]
#[allow(dead_code)] // Consumed by the object staging transaction.
pub(crate) struct ManifestEntry {
    pub kind: u16,
    pub object_id: [u8; 16],
    pub object_hash: [u8; 32],
    pub object_len: u32,
}

#[allow(dead_code)] // Called by the genesis reservation transaction.
pub(crate) fn verify_genesis_candidate(
    candidate_bytes: &[u8],
    relay_public_key: &[u8; 32],
) -> Result<GenesisCandidate, Error> {
    let candidate = cbor::decode_with_limits(
        candidate_bytes,
        cbor::Limits {
            max_bytes: 1024 * 1024,
            max_depth: 16,
        },
    )?;
    let root = exact_map(&candidate, 2)?;
    let unsigned = exact_map(&root[0].1, 11)?;
    if number(&unsigned[0].1)? != 1
        || number(&unsigned[5].1)? != 1
        || number(&unsigned[8].1)? != 1
        || fixed::<32>(&unsigned[3].1)? != [0; 32]
    {
        return Err(Error::Invalid("genesis version, kind, epoch, or parent"));
    }
    let family_id = fixed::<16>(&unsigned[1].1)?;
    let relay_id = fixed::<32>(&unsigned[2].1)?;
    if relay_id != <[u8; 32]>::from(Sha256::digest(relay_public_key)) {
        return Err(Error::Invalid("relay ID mismatch"));
    }
    let transition_id = fixed::<16>(&unsigned[4].1)?;
    let delta = exact_map(&unsigned[6].1, 3)?;
    let manager = array(&delta[0].1, 5)?;
    let manager_id = fixed::<16>(&manager[0])?;
    let manager_signing_key = fixed::<32>(&manager[1])?;
    let _manager_agreement_key = fixed::<32>(&manager[2])?;
    if number(&manager[3])? == 0 || number(&manager[4])? != 2 {
        return Err(Error::Invalid("initial manager row invalid"));
    }
    let _epoch_commitment = fixed::<32>(&delta[1].1)?;
    let promotion_hash = fixed::<32>(&delta[2].1)?;
    let state = Value::Map(vec![
        (1, Value::Integer(1)),
        (2, Value::Bytes(family_id.to_vec())),
        (3, Value::Bytes(relay_id.to_vec())),
        (4, Value::Integer(1)),
        (5, Value::Array(vec![delta[0].1.clone()])),
        (6, Value::Array(vec![])),
        (7, Value::Array(vec![])),
    ]);
    if fixed::<32>(&unsigned[7].1)? != crypto::hash("auth-state", &cbor::encode(&state)?)? {
        return Err(Error::Invalid("genesis state hash mismatch"));
    }
    let core = Value::Array(
        unsigned[..9]
            .iter()
            .map(|(_, value)| value.clone())
            .collect(),
    );
    if fixed::<32>(&unsigned[10].1)? != crypto::hash("transition-core", &cbor::encode(&core)?)? {
        return Err(Error::Invalid("genesis core hash mismatch"));
    }
    let Value::Array(manifest) = &unsigned[9].1 else {
        return Err(Error::Invalid("genesis manifest not array"));
    };
    if manifest.is_empty() || manifest.len() > 16_384 {
        return Err(Error::Invalid("genesis manifest count"));
    }
    let mut entries = Vec::with_capacity(manifest.len());
    let mut prior = None;
    for (index, value) in manifest.iter().enumerate() {
        let fields = array(value, 4)?;
        let kind: u16 = number(&fields[0])?
            .try_into()
            .map_err(|_| Error::Invalid("kind range"))?;
        let object_id = fixed::<16>(&fields[1])?;
        let object_hash = fixed::<32>(&fields[2])?;
        let object_len: u32 = number(&fields[3])?
            .try_into()
            .map_err(|_| Error::Invalid("length range"))?;
        if (index == 0 && (kind != 6 || object_hash != promotion_hash))
            || (index > 0 && kind != 7)
            || object_len > 1024 * 1024
            || prior.is_some_and(|previous| (kind, object_id) <= previous)
        {
            return Err(Error::Invalid("genesis manifest invalid"));
        }
        prior = Some((kind, object_id));
        entries.push(ManifestEntry {
            kind,
            object_id,
            object_hash,
            object_len,
        });
    }
    let signatures = array(&root[1].1, 1)?;
    let signature = array(&signatures[0], 2)?;
    if fixed::<16>(&signature[0])? != manager_id {
        return Err(Error::Invalid("genesis signer mismatch"));
    }
    let unsigned_bytes = cbor::encode(&root[0].1)?;
    crypto::verify_cbor(
        "control-transition",
        &unsigned_bytes,
        &manager_signing_key,
        &fixed::<64>(&signature[1])?,
    )?;
    Ok(GenesisCandidate {
        family_id,
        relay_id,
        transition_id,
        manager_id,
        manager_signing_key,
        manifest: entries,
    })
}

fn exact_map(value: &Value, count: usize) -> Result<&[(u64, Value)], Error> {
    let Value::Map(entries) = value else {
        return Err(Error::Invalid("expected map"));
    };
    if entries.len() != count
        || entries
            .iter()
            .enumerate()
            .any(|(i, (key, _))| *key != i as u64 + 1)
    {
        return Err(Error::Invalid("map keys"));
    }
    Ok(entries)
}
fn array(value: &Value, count: usize) -> Result<&[Value], Error> {
    let Value::Array(items) = value else {
        return Err(Error::Invalid("expected array"));
    };
    if items.len() != count {
        return Err(Error::Invalid("array length"));
    }
    Ok(items)
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

    fn hex(value: &str) -> Vec<u8> {
        (0..value.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&value[i..i + 2], 16).unwrap())
            .collect()
    }
    #[test]
    fn genesis_candidate_validates_public_state_and_signature() {
        let api: Json = serde_json::from_str(
            &std::fs::read_to_string("../tests/vectors/api-genesis-v1.json").unwrap(),
        )
        .unwrap();
        let chain: Json = serde_json::from_str(
            &std::fs::read_to_string("../tests/vectors/contiguous-chain-v1.json").unwrap(),
        )
        .unwrap();
        let seed: [u8; 32] = hex(chain["test_only_inputs"]["relay_sign_seed_hex"]
            .as_str()
            .unwrap())
        .try_into()
        .unwrap();
        let relay_key = crypto::signing_public_key(&seed);
        let candidate = hex(api["inputs"]["commit_candidate_cbor_hex"].as_str().unwrap());
        let parsed = verify_genesis_candidate(&candidate, &relay_key).unwrap();
        assert_eq!(
            parsed.family_id.to_vec(),
            hex(api["inputs"]["family_id_hex"].as_str().unwrap())
        );
        assert_eq!(parsed.manifest[0].kind, 6);
        assert!(verify_genesis_candidate(&candidate, &[0; 32]).is_err());
        let mut changed = candidate.clone();
        *changed.last_mut().unwrap() ^= 1;
        assert!(verify_genesis_candidate(&changed, &relay_key).is_err());
    }
}
