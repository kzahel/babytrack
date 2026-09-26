//! Canonical signed GET requests. Route authorization chooses the signer key
//! from durable public authority; this module only verifies exact bytes.

use babytrack_wire::{
    cbor::{self, Value},
    crypto,
};

#[derive(Debug)]
#[allow(dead_code)] // Mapped by the HTTP read route.
pub(crate) enum Error {
    Cbor(cbor::Error),
    Crypto(crypto::Error),
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

#[derive(Debug)]
#[allow(dead_code)] // Request ID is persisted by the store on authenticated reads.
pub(crate) struct VerifiedRead {
    pub request_id: [u8; 16],
    pub signer_id: [u8; 16],
    pub request_hash: [u8; 32],
}

pub(crate) fn claimed_signer(auth_bytes: &[u8]) -> Result<[u8; 16], Error> {
    let auth = cbor::decode_with_limits(
        auth_bytes,
        cbor::Limits {
            max_bytes: 2048,
            max_depth: 4,
        },
    )?;
    let Value::Map(fields) = auth else {
        return Err(Error::Invalid("read auth not map"));
    };
    if fields.len() != 2 || fields[0].0 != 1 || fields[1].0 != 2 {
        return Err(Error::Invalid("read auth keys"));
    }
    let Value::Bytes(request_bytes) = &fields[0].1 else {
        return Err(Error::Invalid("request not bytes"));
    };
    let request = cbor::decode_with_limits(
        request_bytes,
        cbor::Limits {
            max_bytes: 1024,
            max_depth: 3,
        },
    )?;
    let Value::Array(parts) = request else {
        return Err(Error::Invalid("request not array"));
    };
    if parts.len() != 8 {
        return Err(Error::Invalid("request length"));
    }
    let Value::Bytes(signer) = &parts[3] else {
        return Err(Error::Invalid("signer not bytes"));
    };
    signer
        .as_slice()
        .try_into()
        .map_err(|_| Error::Invalid("signer length"))
}

pub(crate) fn verify_get(
    auth_bytes: &[u8],
    family_id: [u8; 16],
    relay_id: [u8; 32],
    signer_id: [u8; 16],
    signer_public: [u8; 32],
    exact_path: &str,
) -> Result<VerifiedRead, Error> {
    let auth = cbor::decode_with_limits(
        auth_bytes,
        cbor::Limits {
            max_bytes: 2048,
            max_depth: 4,
        },
    )?;
    let Value::Map(fields) = auth else {
        return Err(Error::Invalid("read auth not map"));
    };
    if fields.len() != 2 || fields[0].0 != 1 || fields[1].0 != 2 {
        return Err(Error::Invalid("read auth keys"));
    }
    let Value::Bytes(request_bytes) = &fields[0].1 else {
        return Err(Error::Invalid("request not bytes"));
    };
    let Value::Bytes(signature) = &fields[1].1 else {
        return Err(Error::Invalid("signature not bytes"));
    };
    let signature: [u8; 64] = signature
        .as_slice()
        .try_into()
        .map_err(|_| Error::Invalid("signature length"))?;
    let request = cbor::decode_with_limits(
        request_bytes,
        cbor::Limits {
            max_bytes: 1024,
            max_depth: 3,
        },
    )?;
    let Value::Array(parts) = request else {
        return Err(Error::Invalid("request not array"));
    };
    if parts.len() != 8
        || parts[0] != Value::Integer(1)
        || parts[1] != Value::Bytes(family_id.to_vec())
        || parts[2] != Value::Bytes(relay_id.to_vec())
        || parts[3] != Value::Bytes(signer_id.to_vec())
        || parts[5] != Value::Text("GET".into())
        || parts[6] != Value::Text(exact_path.into())
        || parts[7] != Value::Bytes(crypto::hash("request-body", &[])?.to_vec())
    {
        return Err(Error::Invalid("read request context"));
    }
    let Value::Bytes(id) = &parts[4] else {
        return Err(Error::Invalid("request ID not bytes"));
    };
    let request_id: [u8; 16] = id
        .as_slice()
        .try_into()
        .map_err(|_| Error::Invalid("request ID length"))?;
    crypto::verify_cbor("read-request", request_bytes, &signer_public, &signature)?;
    Ok(VerifiedRead {
        request_id,
        signer_id,
        request_hash: crypto::hash("read-request", request_bytes)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value as Json;
    use sha2::Digest;
    fn hex(value: &str) -> Vec<u8> {
        (0..value.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&value[i..i + 2], 16).unwrap())
            .collect()
    }
    #[test]
    fn promotion_read_signature_binds_exact_path_and_relay() {
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
        let relay_public = crypto::signing_public_key(&seed);
        let relay_id = <[u8; 32]>::from(sha2::Sha256::digest(relay_public));
        let family: [u8; 16] = hex(api["inputs"]["family_id_hex"].as_str().unwrap())
            .try_into()
            .unwrap();
        let manager: [u8; 16] = hex(api["inputs"]["initial_manager_id_hex"].as_str().unwrap())
            .try_into()
            .unwrap();
        let manager_seed: [u8; 32] = hex(chain["test_only_inputs"]["manager_sign_seed_hex"]
            .as_str()
            .unwrap())
        .try_into()
        .unwrap();
        let path = api["inputs"]["promotion_result_path"].as_str().unwrap();
        let auth = hex(api["inputs"]["promotion_result_read_auth_cbor_hex"]
            .as_str()
            .unwrap());
        assert!(
            verify_get(
                &auth,
                family,
                relay_id,
                manager,
                crypto::signing_public_key(&manager_seed),
                path
            )
            .is_ok()
        );
        assert!(
            verify_get(
                &auth,
                family,
                relay_id,
                manager,
                crypto::signing_public_key(&manager_seed),
                &format!("{path}/")
            )
            .is_err()
        );
        assert!(
            verify_get(
                &auth,
                family,
                [0; 32],
                manager,
                crypto::signing_public_key(&manager_seed),
                path
            )
            .is_err()
        );
    }
}
