//! Public-only verification before the relay can reserve a new Family.
//! No event schema, payload decryption, or client key material lives here.

use babytrack_wire::{
    authority as public_authority,
    cbor::{self, Value},
    crypto,
};
use sha2::{Digest, Sha256};

#[derive(Debug)]
#[allow(dead_code)] // Error detail is consumed by the upcoming route boundary.
pub(crate) enum Error {
    Public(public_authority::Error),
    Cbor(cbor::Error),
    Crypto(crypto::Error),
    Invalid(&'static str),
}
impl From<public_authority::Error> for Error {
    fn from(value: public_authority::Error) -> Self {
        Self::Public(value)
    }
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
    pub manager_row: Value,
    pub epoch_commitment: [u8; 32],
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
    let epoch_commitment = fixed::<32>(&delta[1].1)?;
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
        manager_row: delta[0].1.clone(),
        epoch_commitment,
        manifest: entries,
    })
}

#[derive(Debug, Clone)]
#[allow(dead_code)] // Used by the first post-genesis control transaction.
pub(crate) struct IssueCandidate {
    pub family_id: [u8; 16],
    pub transition_id: [u8; 16],
    pub invitation_id: [u8; 16],
    pub invite_public: [u8; 32],
    pub role: u64,
    pub manifest: ManifestEntry,
}

#[allow(dead_code)] // Used by the first post-genesis control transaction.
pub(crate) fn verify_first_invite_issue(
    candidate_bytes: &[u8],
    genesis: &GenesisCandidate,
    current_head: [u8; 32],
) -> Result<IssueCandidate, Error> {
    let state = Value::Map(vec![
        (1, Value::Integer(1)),
        (2, Value::Bytes(genesis.family_id.to_vec())),
        (3, Value::Bytes(genesis.relay_id.to_vec())),
        (4, Value::Integer(1)),
        (5, Value::Array(vec![genesis.manager_row.clone()])),
        (6, Value::Array(vec![])),
        (7, Value::Array(vec![])),
    ]);
    let prepared = public_authority::prepare_issue(candidate_bytes, &state, current_head)?;
    if [
        prepared.transition_id,
        prepared.invitation_id,
        prepared.manifest.object_id,
    ]
    .iter()
    .any(|id| {
        *id == genesis.transition_id
            || *id == genesis.manager_id
            || genesis.manifest.iter().any(|prior| prior.object_id == *id)
    }) {
        return Err(Error::Invalid("issue ID reused from genesis"));
    }
    Ok(IssueCandidate {
        family_id: prepared.family_id,
        transition_id: prepared.transition_id,
        invitation_id: prepared.invitation_id,
        invite_public: prepared.invite_public,
        role: prepared.role.into(),
        manifest: ManifestEntry {
            kind: prepared.manifest.kind,
            object_id: prepared.manifest.object_id,
            object_hash: prepared.manifest.object_hash,
            object_len: prepared.manifest.object_len,
        },
    })
}

#[derive(Debug, Clone)]
#[allow(dead_code)] // Used by the pending-claim relay transaction.
pub(crate) struct ClaimCandidate {
    pub family_id: [u8; 16],
    pub transition_id: [u8; 16],
    pub invitation_id: [u8; 16],
    pub device_id: [u8; 16],
    pub signing_public: [u8; 32],
    pub agreement_public: [u8; 32],
    pub key_version: u64,
    pub claim_hash: [u8; 32],
    pub role: u64,
}

#[allow(dead_code)] // Used by the pending-claim relay transaction.
pub(crate) fn verify_first_claim(
    candidate_bytes: &[u8],
    genesis: &GenesisCandidate,
    issue: &IssueCandidate,
    issue_head: [u8; 32],
) -> Result<ClaimCandidate, Error> {
    let state = Value::Map(vec![
        (1, Value::Integer(1)),
        (2, Value::Bytes(genesis.family_id.to_vec())),
        (3, Value::Bytes(genesis.relay_id.to_vec())),
        (4, Value::Integer(1)),
        (5, Value::Array(vec![genesis.manager_row.clone()])),
        (6, Value::Array(vec![])),
        (
            7,
            Value::Array(vec![Value::Array(vec![
                Value::Bytes(issue.invitation_id.to_vec()),
                Value::Bytes(genesis.manager_id.to_vec()),
                Value::Bytes(issue.invite_public.to_vec()),
                Value::Integer(issue.role.into()),
                Value::Bytes(issue.transition_id.to_vec()),
                Value::Integer(1),
            ])]),
        ),
    ]);
    let prepared = public_authority::prepare_claim(candidate_bytes, &state, issue_head)?;
    if [prepared.transition_id, prepared.device_id]
        .iter()
        .any(|id| {
            *id == genesis.transition_id
                || *id == genesis.manager_id
                || *id == issue.transition_id
                || *id == issue.invitation_id
                || *id == issue.manifest.object_id
                || genesis.manifest.iter().any(|prior| prior.object_id == *id)
        })
    {
        return Err(Error::Invalid("claim historical ID reused"));
    }
    Ok(ClaimCandidate {
        family_id: prepared.family_id,
        transition_id: prepared.transition_id,
        invitation_id: prepared.invitation_id,
        device_id: prepared.device_id,
        signing_public: prepared.signing_public,
        agreement_public: prepared.agreement_public,
        key_version: prepared.key_version,
        claim_hash: prepared.claim_hash,
        role: prepared.role,
    })
}

fn state_with_pending(
    genesis: &GenesisCandidate,
    issue: &IssueCandidate,
    claim: &ClaimCandidate,
    challenge_id: Option<[u8; 16]>,
    proof_hash: Option<[u8; 32]>,
) -> Value {
    let pending = Value::Array(vec![
        Value::Bytes(claim.invitation_id.to_vec()),
        Value::Bytes(claim.device_id.to_vec()),
        Value::Bytes(claim.signing_public.to_vec()),
        Value::Bytes(claim.agreement_public.to_vec()),
        Value::Integer(claim.key_version.into()),
        Value::Integer(claim.role.into()),
        Value::Bytes(claim.claim_hash.to_vec()),
        challenge_id.map_or(Value::Null, |id| Value::Bytes(id.to_vec())),
        proof_hash.map_or(Value::Null, |hash| Value::Bytes(hash.to_vec())),
    ]);
    let invitation = Value::Array(vec![
        Value::Bytes(issue.invitation_id.to_vec()),
        Value::Bytes(genesis.manager_id.to_vec()),
        Value::Bytes(issue.invite_public.to_vec()),
        Value::Integer(issue.role.into()),
        Value::Bytes(issue.transition_id.to_vec()),
        Value::Integer(2),
    ]);
    Value::Map(vec![
        (1, Value::Integer(1)),
        (2, Value::Bytes(genesis.family_id.to_vec())),
        (3, Value::Bytes(genesis.relay_id.to_vec())),
        (4, Value::Integer(1)),
        (5, Value::Array(vec![genesis.manager_row.clone()])),
        (6, Value::Array(vec![pending])),
        (7, Value::Array(vec![invitation])),
    ])
}

#[derive(Debug, Clone)]
#[allow(dead_code)] // Consumed by challenge staging and proof validation.
pub(crate) struct ChallengeCandidate {
    pub transition_id: [u8; 16],
    pub challenge_id: [u8; 16],
    pub challenge_hash: [u8; 32],
    pub context_bytes: Vec<u8>,
    pub manifest: Vec<ManifestEntry>,
}

#[allow(dead_code)] // Consumed by challenge staging and proof validation.
pub(crate) fn verify_first_challenge(
    candidate_bytes: &[u8],
    genesis: &GenesisCandidate,
    issue: &IssueCandidate,
    claim: &ClaimCandidate,
    claim_head: [u8; 32],
) -> Result<ChallengeCandidate, Error> {
    let state = state_with_pending(genesis, issue, claim, None, None);
    let prepared = public_authority::prepare_challenge(candidate_bytes, &state, claim_head)?;
    if prepared.challenge_id == genesis.transition_id
        || prepared.challenge_id == issue.transition_id
        || prepared.challenge_id == claim.transition_id
        || prepared.transition_id == genesis.transition_id
        || prepared.transition_id == issue.transition_id
        || prepared.transition_id == claim.transition_id
        || prepared.manifest[0].object_id == prepared.manifest[1].object_id
        || prepared.manifest.iter().any(|entry| {
            genesis
                .manifest
                .iter()
                .any(|prior| prior.object_id == entry.object_id)
                || entry.object_id == issue.manifest.object_id
        })
    {
        return Err(Error::Invalid("challenge historical ID reused"));
    }
    Ok(ChallengeCandidate {
        transition_id: prepared.transition_id,
        challenge_id: prepared.challenge_id,
        challenge_hash: prepared.challenge_hash,
        context_bytes: prepared.context_bytes,
        manifest: prepared
            .manifest
            .into_iter()
            .map(|entry| ManifestEntry {
                kind: entry.kind,
                object_id: entry.object_id,
                object_hash: entry.object_hash,
                object_len: entry.object_len,
            })
            .collect(),
    })
}

#[derive(Debug, Clone)]
#[allow(dead_code)] // Consumed by the key-proof transaction.
pub(crate) struct ProofCandidate {
    pub transition_id: [u8; 16],
    pub proof_hash: [u8; 32],
}

#[allow(dead_code)] // Consumed by the key-proof transaction.
pub(crate) fn verify_first_proof(
    candidate_bytes: &[u8],
    genesis: &GenesisCandidate,
    issue: &IssueCandidate,
    claim: &ClaimCandidate,
    challenge: &ChallengeCandidate,
    challenge_head: [u8; 32],
) -> Result<ProofCandidate, Error> {
    let state = state_with_pending(genesis, issue, claim, Some(challenge.challenge_id), None);
    let prepared = public_authority::prepare_proof(
        candidate_bytes,
        &state,
        challenge_head,
        challenge.challenge_id,
        challenge.challenge_hash,
    )?;
    if [
        genesis.transition_id,
        issue.transition_id,
        claim.transition_id,
        challenge.transition_id,
    ]
    .contains(&prepared.transition_id)
    {
        return Err(Error::Invalid("proof transition ID reused"));
    }
    Ok(ProofCandidate {
        transition_id: prepared.transition_id,
        proof_hash: prepared.proof_hash,
    })
}

#[derive(Debug, Clone)]
#[allow(dead_code)] // Consumed by the admission staging transaction.
pub(crate) struct AdmissionCandidate {
    pub transition_id: [u8; 16],
    pub recipient_id: [u8; 16],
    pub key_version: u32,
    pub core_hash: [u8; 32],
    pub manifest: Vec<ManifestEntry>,
    pub next_state: Value,
}

pub(crate) fn validate_first_admission_grant(
    admission: &AdmissionCandidate,
    object_id: [u8; 16],
    object_bytes: &[u8],
) -> Result<(), Error> {
    public_authority::validate_public_grant(
        object_bytes,
        object_id,
        1,
        admission.recipient_id,
        admission.key_version,
        admission.core_hash,
    )?;
    Ok(())
}

#[derive(Debug, Clone)]
pub(crate) struct FirstRemovalCandidate {
    pub transition_id: [u8; 16],
    pub manifest: Vec<ManifestEntry>,
}

pub(crate) fn verify_first_removal(
    candidate_bytes: &[u8],
    genesis: &GenesisCandidate,
    issue: &IssueCandidate,
    claim: &ClaimCandidate,
    admission: &AdmissionCandidate,
    admission_head: [u8; 32],
) -> Result<FirstRemovalCandidate, Error> {
    let prepared = public_authority::prepare_removal(
        candidate_bytes,
        &admission.next_state,
        admission_head,
        genesis.epoch_commitment,
    )?;
    if prepared.target_id != claim.device_id
        || prepared.transition_id == admission.transition_id
        || prepared.transition_id == genesis.transition_id
        || prepared.transition_id == issue.transition_id
        || prepared.transition_id == claim.transition_id
    {
        return Err(Error::Invalid(
            "first removal target or transition ID invalid",
        ));
    }
    Ok(FirstRemovalCandidate {
        transition_id: prepared.transition_id,
        manifest: prepared
            .manifest
            .into_iter()
            .map(|entry| ManifestEntry {
                kind: entry.kind,
                object_id: entry.object_id,
                object_hash: entry.object_hash,
                object_len: entry.object_len,
            })
            .collect(),
    })
}

#[allow(dead_code)] // Consumed by the admission staging transaction.
pub(crate) fn verify_first_admission(
    candidate_bytes: &[u8],
    genesis: &GenesisCandidate,
    issue: &IssueCandidate,
    claim: &ClaimCandidate,
    challenge: &ChallengeCandidate,
    proof: &ProofCandidate,
    proof_head: [u8; 32],
) -> Result<AdmissionCandidate, Error> {
    let state = state_with_pending(
        genesis,
        issue,
        claim,
        Some(challenge.challenge_id),
        Some(proof.proof_hash),
    );
    let prepared = public_authority::prepare_admission(
        candidate_bytes,
        &state,
        proof_head,
        genesis.epoch_commitment,
    )?;
    if [
        genesis.transition_id,
        issue.transition_id,
        claim.transition_id,
        challenge.transition_id,
        proof.transition_id,
    ]
    .contains(&prepared.transition_id)
        || prepared.manifest[0].object_id == prepared.manifest[1].object_id
        || prepared.manifest.iter().any(|entry| {
            genesis
                .manifest
                .iter()
                .any(|prior| prior.object_id == entry.object_id)
                || entry.object_id == issue.manifest.object_id
                || challenge
                    .manifest
                    .iter()
                    .any(|prior| prior.object_id == entry.object_id)
        })
    {
        return Err(Error::Invalid("admission transition or object ID reused"));
    }
    Ok(AdmissionCandidate {
        transition_id: prepared.transition_id,
        recipient_id: prepared.device_id,
        key_version: prepared.key_version,
        core_hash: prepared.core_hash,
        next_state: prepared.next_state,
        manifest: prepared
            .manifest
            .into_iter()
            .map(|entry| ManifestEntry {
                kind: entry.kind,
                object_id: entry.object_id,
                object_hash: entry.object_hash,
                object_len: entry.object_len,
            })
            .collect(),
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
    fn admission_grant_public_context_rejects_wrong_recipient_purpose_and_core() {
        let fixture: Json = serde_json::from_str(
            &std::fs::read_to_string("../tests/vectors/contiguous-chain-v1.json").unwrap(),
        )
        .unwrap();
        let grant = hex(
            fixture["objects_by_id_hex"]["923e4567e89b42d3a456426614174000"]
                .as_str()
                .unwrap(),
        );
        let Value::Map(fields) = cbor::decode(&grant).unwrap() else {
            panic!()
        };
        let id = fixed::<16>(&fields[1].1).unwrap();
        let recipient = fixed::<16>(&fields[3].1).unwrap();
        let version = number(&fields[4].1).unwrap() as u32;
        let core = fixed::<32>(&fields[6].1).unwrap();
        public_authority::validate_public_grant(&grant, id, 1, recipient, version, core).unwrap();
        for (index, replacement) in [
            (2, Value::Integer(3)),
            (3, Value::Bytes(vec![0; 16])),
            (4, Value::Integer(2)),
            (6, Value::Bytes(vec![0; 32])),
        ] {
            let mut changed = fields.clone();
            changed[index].1 = replacement;
            let bytes = cbor::encode(&Value::Map(changed)).unwrap();
            assert!(
                public_authority::validate_public_grant(&bytes, id, 1, recipient, version, core)
                    .is_err()
            );
        }
        let mut opaque_ciphertext = fields;
        opaque_ciphertext[8].1 = Value::Bytes(vec![0; 16]);
        public_authority::validate_public_grant(
            &cbor::encode(&Value::Map(opaque_ciphertext)).unwrap(),
            id,
            1,
            recipient,
            version,
            core,
        )
        .unwrap();
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

    #[test]
    fn first_issue_requires_manager_and_current_genesis_head() {
        let genesis_api: Json = serde_json::from_str(
            &std::fs::read_to_string("../tests/vectors/api-genesis-v1.json").unwrap(),
        )
        .unwrap();
        let issue_api: Json =
            serde_json::from_str(&std::fs::read_to_string("../tests/vectors/api-v1.json").unwrap())
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
        let genesis_bytes = hex(genesis_api["inputs"]["commit_candidate_cbor_hex"]
            .as_str()
            .unwrap());
        let genesis =
            verify_genesis_candidate(&genesis_bytes, &crypto::signing_public_key(&seed)).unwrap();
        let genesis_response = hex(genesis_api["expect"]["commit_response_cbor_hex"]
            .as_str()
            .unwrap());
        let Value::Map(response) = cbor::decode(&genesis_response).unwrap() else {
            panic!()
        };
        let Value::Bytes(committed) = &response[1].1 else {
            panic!()
        };
        let head = crypto::hash("control-head", committed).unwrap();
        let issue_bytes = hex(issue_api["inputs"]["commit_body_cbor_hex"]
            .as_str()
            .unwrap());
        let issue = verify_first_invite_issue(&issue_bytes, &genesis, head).unwrap();
        assert_eq!(issue.manifest.kind, 1);
        assert!(verify_first_invite_issue(&issue_bytes, &genesis, [0; 32]).is_err());
        let mut colliding_genesis = genesis.clone();
        colliding_genesis.manifest[0].object_id = issue.invitation_id;
        assert!(verify_first_invite_issue(&issue_bytes, &colliding_genesis, head).is_err());
        let mut changed = issue_bytes.clone();
        *changed.last_mut().unwrap() ^= 1;
        assert!(verify_first_invite_issue(&changed, &genesis, head).is_err());
        let issue_committed = hex(chain["transitions"][1]["committed_cbor_hex"]
            .as_str()
            .unwrap());
        let issue_head = crypto::hash("control-head", &issue_committed).unwrap();
        let unsigned = cbor::decode(&hex(chain["transitions"][2]["unsigned_cbor_hex"]
            .as_str()
            .unwrap()))
        .unwrap();
        let signatures = cbor::decode(&hex(chain["transitions"][2]["signatures_cbor_hex"]
            .as_str()
            .unwrap()))
        .unwrap();
        let claim_bytes = cbor::encode(&Value::Map(vec![(1, unsigned), (2, signatures)])).unwrap();
        let claim = verify_first_claim(&claim_bytes, &genesis, &issue, issue_head).unwrap();
        assert_eq!(claim.invitation_id, issue.invitation_id);
        assert!(verify_first_claim(&claim_bytes, &genesis, &issue, [0; 32]).is_err());
        colliding_genesis.manifest[0].object_id = claim.device_id;
        assert!(verify_first_claim(&claim_bytes, &colliding_genesis, &issue, issue_head).is_err());
        let claim_committed = hex(chain["transitions"][2]["committed_cbor_hex"]
            .as_str()
            .unwrap());
        let claim_head = crypto::hash("control-head", &claim_committed).unwrap();
        let challenge_bytes = cbor::encode(&Value::Map(vec![
            (
                1,
                cbor::decode(&hex(chain["transitions"][3]["unsigned_cbor_hex"]
                    .as_str()
                    .unwrap()))
                .unwrap(),
            ),
            (
                2,
                cbor::decode(&hex(chain["transitions"][3]["signatures_cbor_hex"]
                    .as_str()
                    .unwrap()))
                .unwrap(),
            ),
        ]))
        .unwrap();
        let challenge =
            verify_first_challenge(&challenge_bytes, &genesis, &issue, &claim, claim_head).unwrap();
        assert_eq!(challenge.manifest.len(), 2);
        assert!(
            verify_first_challenge(&challenge_bytes, &genesis, &issue, &claim, [0; 32]).is_err()
        );
        let challenge_committed = hex(chain["transitions"][3]["committed_cbor_hex"]
            .as_str()
            .unwrap());
        let challenge_head = crypto::hash("control-head", &challenge_committed).unwrap();
        let proof_bytes = cbor::encode(&Value::Map(vec![
            (
                1,
                cbor::decode(&hex(chain["transitions"][4]["unsigned_cbor_hex"]
                    .as_str()
                    .unwrap()))
                .unwrap(),
            ),
            (
                2,
                cbor::decode(&hex(chain["transitions"][4]["signatures_cbor_hex"]
                    .as_str()
                    .unwrap()))
                .unwrap(),
            ),
        ]))
        .unwrap();
        let proof = verify_first_proof(
            &proof_bytes,
            &genesis,
            &issue,
            &claim,
            &challenge,
            challenge_head,
        )
        .unwrap();
        assert_ne!(proof.proof_hash, [0; 32]);
        assert!(
            verify_first_proof(&proof_bytes, &genesis, &issue, &claim, &challenge, [0; 32])
                .is_err()
        );
        let proof_committed = hex(chain["transitions"][4]["committed_cbor_hex"]
            .as_str()
            .unwrap());
        let proof_head = crypto::hash("control-head", &proof_committed).unwrap();
        let admission_bytes = cbor::encode(&Value::Map(vec![
            (
                1,
                cbor::decode(&hex(chain["transitions"][5]["unsigned_cbor_hex"]
                    .as_str()
                    .unwrap()))
                .unwrap(),
            ),
            (
                2,
                cbor::decode(&hex(chain["transitions"][5]["signatures_cbor_hex"]
                    .as_str()
                    .unwrap()))
                .unwrap(),
            ),
        ]))
        .unwrap();
        let admission = verify_first_admission(
            &admission_bytes,
            &genesis,
            &issue,
            &claim,
            &challenge,
            &proof,
            proof_head,
        )
        .unwrap();
        assert_eq!(admission.recipient_id, claim.device_id);
        assert_eq!(admission.manifest.len(), 2);
        assert!(
            verify_first_admission(
                &admission_bytes,
                &genesis,
                &issue,
                &claim,
                &challenge,
                &proof,
                [0; 32]
            )
            .is_err()
        );
    }
}
