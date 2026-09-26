//! Challenge/proof object verification for device-scoped key handoff.
//! Only a committed control-chain challenge constructs this context.

use crate::{
    cbor::{self, Value},
    crypto, hpke,
    projection::VerifiedEpochKey,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    Cbor(cbor::Error),
    Crypto(crypto::Error),
    Hpke(hpke::Error),
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
impl From<hpke::Error> for Error {
    fn from(value: hpke::Error) -> Self {
        Self::Hpke(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedChallenge {
    pub(crate) family_id: [u8; 16],
    pub(crate) epoch: u32,
    pub(crate) device_id: [u8; 16],
    pub(crate) challenge_id: [u8; 16],
    pub(crate) context_bytes: Vec<u8>,
    pub(crate) challenge_hash: [u8; 32],
    pub(crate) hpke_object_id: [u8; 16],
    pub(crate) hpke_object_hash: [u8; 32],
    pub(crate) verifier_object_id: [u8; 16],
    pub(crate) verifier_object_hash: [u8; 32],
    pub(crate) pending_sign_public: [u8; 32],
    pub(crate) pending_agree_public: [u8; 32],
    pub(crate) pending_key_version: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingProof {
    pub signature: [u8; 64],
    pub proof_hash: [u8; 32],
}

impl VerifiedChallenge {
    pub fn hpke_object_id(&self) -> [u8; 16] {
        self.hpke_object_id
    }
    pub fn verifier_object_id(&self) -> [u8; 16] {
        self.verifier_object_id
    }

    /// The pending device opens the HPKE object and signs the secret-bound
    /// context. The Family key never appears in its invitation link.
    pub fn prepare_proof(
        &self,
        hpke_object: &[u8],
        agreement_private: &[u8; 32],
        signing_seed: &[u8; 32],
    ) -> Result<PendingProof, Error> {
        if hpke::public_key_from_private(agreement_private)? != self.pending_agree_public
            || crypto::signing_public_key(signing_seed) != self.pending_sign_public
        {
            return Err(Error::Invalid(
                "pending device keys differ from committed claim",
            ));
        }
        if crypto::hash("object", hpke_object)? != self.hpke_object_hash {
            return Err(Error::Invalid("challenge HPKE object hash mismatch"));
        }
        let value = cbor::decode_with_limits(
            hpke_object,
            cbor::Limits {
                max_bytes: 1024 * 1024,
                max_depth: 16,
            },
        )?;
        let fields = exact_map(&value, 7)?;
        if number(&fields[0].1)? != 1
            || fixed::<16>(&fields[1].1)? != self.challenge_id
            || fixed::<16>(&fields[2].1)? != self.device_id
            || number(&fields[3].1)? != self.pending_key_version as u64
            || byte_string(&fields[4].1)? != self.context_bytes.as_slice()
        {
            return Err(Error::Invalid("challenge HPKE object context mismatch"));
        }
        let enc = fixed::<32>(&fields[5].1)?;
        let ciphertext = byte_string(&fields[6].1)?;
        let info = crypto::hash("challenge-info", &self.context_bytes)?;
        let plaintext = hpke::open(
            agreement_private,
            &enc,
            &info,
            &self.context_bytes,
            ciphertext,
        )?;
        let secret = secret_from_plaintext(&plaintext)?;
        let input = self.secret_input(&secret)?;
        if crypto::hash("challenge", &input)? != self.challenge_hash {
            return Err(Error::Invalid(
                "opened secret differs from committed challenge",
            ));
        }
        let signature = crypto::sign_cbor("key-proof", &input, signing_seed)?;
        let proof_hash = self.proof_hash(&signature)?;
        Ok(PendingProof {
            signature,
            proof_hash,
        })
    }

    /// A current holder checks the verifier and pending device's proof
    /// before authorizing admission. The epoch key token is derived from the
    /// committed control-chain commitment, not platform input alone.
    pub fn verify_holder_proof(
        &self,
        verifier_object: &[u8],
        key: &VerifiedEpochKey,
        proof_signature: &[u8; 64],
        committed_proof_hash: &[u8; 32],
    ) -> Result<(), Error> {
        if key.family_id != self.family_id || key.epoch != self.epoch {
            return Err(Error::Invalid(
                "challenge verifier key is from another epoch",
            ));
        }
        if crypto::hash("object", verifier_object)? != self.verifier_object_hash {
            return Err(Error::Invalid("challenge verifier object hash mismatch"));
        }
        let value = cbor::decode_with_limits(
            verifier_object,
            cbor::Limits {
                max_bytes: 1024 * 1024,
                max_depth: 16,
            },
        )?;
        let fields = exact_map(&value, 5)?;
        if number(&fields[0].1)? != 1
            || fixed::<16>(&fields[1].1)? != self.challenge_id
            || fixed::<32>(&fields[2].1)? != crypto::hash("challenge-context", &self.context_bytes)?
        {
            return Err(Error::Invalid("challenge verifier context mismatch"));
        }
        let nonce = fixed::<24>(&fields[3].1)?;
        let aad = crypto::hash("challenge-verifier-aad", &self.context_bytes)?;
        let plaintext = crypto::open(&key.bytes, &nonce, &aad, byte_string(&fields[4].1)?)?;
        let secret = secret_from_plaintext(&plaintext)?;
        let input = self.secret_input(&secret)?;
        if crypto::hash("challenge", &input)? != self.challenge_hash {
            return Err(Error::Invalid(
                "verifier secret differs from committed challenge",
            ));
        }
        crypto::verify_cbor(
            "key-proof",
            &input,
            &self.pending_sign_public,
            proof_signature,
        )?;
        if self.proof_hash(proof_signature)? != *committed_proof_hash {
            return Err(Error::Invalid("proof differs from committed pending state"));
        }
        Ok(())
    }

    fn secret_input(&self, secret: &[u8; 32]) -> Result<Vec<u8>, Error> {
        Ok(cbor::encode(&Value::Array(vec![
            Value::Bytes(self.context_bytes.clone()),
            Value::Bytes(secret.to_vec()),
        ]))?)
    }

    fn proof_hash(&self, signature: &[u8; 64]) -> Result<[u8; 32], Error> {
        let bytes = cbor::encode(&Value::Array(vec![
            Value::Bytes(self.challenge_id.to_vec()),
            Value::Bytes(signature.to_vec()),
        ]))?;
        Ok(crypto::hash("proof", &bytes)?)
    }
}

fn secret_from_plaintext(plaintext: &[u8]) -> Result<[u8; 32], Error> {
    let value = cbor::decode_with_limits(
        plaintext,
        cbor::Limits {
            max_bytes: 128,
            max_depth: 3,
        },
    )?;
    let Value::Array(fields) = value else {
        return Err(Error::Invalid("challenge plaintext not array"));
    };
    if fields.len() != 2 || number(&fields[0])? != 1 {
        return Err(Error::Invalid("challenge plaintext version invalid"));
    }
    fixed::<32>(&fields[1])
}

fn exact_map(value: &Value, count: usize) -> Result<&[(u64, Value)], Error> {
    let Value::Map(fields) = value else {
        return Err(Error::Invalid("object not map"));
    };
    if fields.len() != count
        || fields
            .iter()
            .enumerate()
            .any(|(index, (key, _))| *key != index as u64 + 1)
    {
        return Err(Error::Invalid("object map keys invalid"));
    }
    Ok(fields)
}
fn fixed<const N: usize>(value: &Value) -> Result<[u8; N], Error> {
    byte_string(value)?
        .try_into()
        .map_err(|_| Error::Invalid("object byte length invalid"))
}
fn byte_string(value: &Value) -> Result<&[u8], Error> {
    let Value::Bytes(bytes) = value else {
        return Err(Error::Invalid("object expected bytes"));
    };
    Ok(bytes)
}
fn number(value: &Value) -> Result<u64, Error> {
    let Value::Integer(value) = value else {
        return Err(Error::Invalid("object expected integer"));
    };
    (*value)
        .try_into()
        .map_err(|_| Error::Invalid("object expected unsigned integer"))
}
