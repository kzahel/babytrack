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
    let value = cbor::decode_with_limits(
        candidate_bytes,
        cbor::Limits {
            max_bytes: 1024 * 1024,
            max_depth: 16,
        },
    )?;
    let root = exact_map(&value, 2)?;
    let unsigned = exact_map(&root[0].1, 11)?;
    if number(&unsigned[0].1)? != 1
        || fixed::<16>(&unsigned[1].1)? != genesis.family_id
        || fixed::<32>(&unsigned[2].1)? != genesis.relay_id
        || fixed::<32>(&unsigned[3].1)? != current_head
        || number(&unsigned[5].1)? != 2
        || number(&unsigned[8].1)? != 1
    {
        return Err(Error::Invalid(
            "issue version, Family, relay, parent, kind, or epoch",
        ));
    }
    let transition_id = fixed::<16>(&unsigned[4].1)?;
    if transition_id == genesis.transition_id {
        return Err(Error::Invalid("issue transition ID reused"));
    }
    let delta = exact_map(&unsigned[6].1, 4)?;
    let invitation_id = fixed::<16>(&delta[0].1)?;
    let issuer = fixed::<16>(&delta[1].1)?;
    let invite_public = fixed::<32>(&delta[2].1)?;
    let role = number(&delta[3].1)?;
    if issuer != genesis.manager_id || (role != 1 && role != 2) {
        return Err(Error::Invalid("issue signer or role invalid"));
    }
    let invitation_row = Value::Array(vec![
        Value::Bytes(invitation_id.to_vec()),
        Value::Bytes(issuer.to_vec()),
        delta[2].1.clone(),
        Value::Integer(role.into()),
        Value::Bytes(transition_id.to_vec()),
        Value::Integer(1),
    ]);
    let resulting = Value::Map(vec![
        (1, Value::Integer(1)),
        (2, Value::Bytes(genesis.family_id.to_vec())),
        (3, Value::Bytes(genesis.relay_id.to_vec())),
        (4, Value::Integer(1)),
        (5, Value::Array(vec![genesis.manager_row.clone()])),
        (6, Value::Array(vec![])),
        (7, Value::Array(vec![invitation_row])),
    ]);
    if fixed::<32>(&unsigned[7].1)? != crypto::hash("auth-state", &cbor::encode(&resulting)?)? {
        return Err(Error::Invalid("issue state hash mismatch"));
    }
    let core = Value::Array(
        unsigned[..9]
            .iter()
            .map(|(_, value)| value.clone())
            .collect(),
    );
    if fixed::<32>(&unsigned[10].1)? != crypto::hash("transition-core", &cbor::encode(&core)?)? {
        return Err(Error::Invalid("issue core hash mismatch"));
    }
    let manifest = array(&unsigned[9].1, 1)?;
    let entry = array(&manifest[0], 4)?;
    let kind: u16 = number(&entry[0])?
        .try_into()
        .map_err(|_| Error::Invalid("kind range"))?;
    let object_id = fixed::<16>(&entry[1])?;
    let object_hash = fixed::<32>(&entry[2])?;
    let object_len: u32 = number(&entry[3])?
        .try_into()
        .map_err(|_| Error::Invalid("length range"))?;
    if kind != 1
        || object_len > 1024 * 1024
        || genesis
            .manifest
            .iter()
            .any(|prior| prior.object_id == object_id)
    {
        return Err(Error::Invalid("issue membership manifest invalid"));
    }
    let signatures = array(&root[1].1, 1)?;
    let signature = array(&signatures[0], 2)?;
    if fixed::<16>(&signature[0])? != issuer {
        return Err(Error::Invalid("issue signature signer invalid"));
    }
    crypto::verify_cbor(
        "control-transition",
        &cbor::encode(&root[0].1)?,
        &genesis.manager_signing_key,
        &fixed::<64>(&signature[1])?,
    )?;
    Ok(IssueCandidate {
        family_id: genesis.family_id,
        transition_id,
        invitation_id,
        invite_public,
        role,
        manifest: ManifestEntry {
            kind,
            object_id,
            object_hash,
            object_len,
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
    let value = cbor::decode_with_limits(
        candidate_bytes,
        cbor::Limits {
            max_bytes: 1024 * 1024,
            max_depth: 16,
        },
    )?;
    let root = exact_map(&value, 2)?;
    let unsigned = exact_map(&root[0].1, 11)?;
    if number(&unsigned[0].1)? != 1
        || fixed::<16>(&unsigned[1].1)? != genesis.family_id
        || fixed::<32>(&unsigned[2].1)? != genesis.relay_id
        || fixed::<32>(&unsigned[3].1)? != issue_head
        || number(&unsigned[5].1)? != 4
        || number(&unsigned[8].1)? != 1
    {
        return Err(Error::Invalid(
            "claim version, Family, relay, parent, kind, or epoch",
        ));
    }
    let transition_id = fixed::<16>(&unsigned[4].1)?;
    if transition_id == genesis.transition_id || transition_id == issue.transition_id {
        return Err(Error::Invalid("claim transition ID reused"));
    }
    let delta = exact_map(&unsigned[6].1, 7)?;
    let invitation_id = fixed::<16>(&delta[0].1)?;
    let device_id = fixed::<16>(&delta[1].1)?;
    let signing_public = fixed::<32>(&delta[2].1)?;
    let agreement_public = fixed::<32>(&delta[3].1)?;
    let key_version = number(&delta[4].1)?;
    let enrollment_nonce = fixed::<32>(&delta[5].1)?;
    let claim_hash = fixed::<32>(&delta[6].1)?;
    if invitation_id != issue.invitation_id
        || device_id == genesis.manager_id
        || key_version == 0
        || key_version > u32::MAX as u64
    {
        return Err(Error::Invalid(
            "claim invitation, device, or key version invalid",
        ));
    }
    let claim_input = Value::Array(vec![
        Value::Bytes(genesis.family_id.to_vec()),
        Value::Bytes(genesis.relay_id.to_vec()),
        Value::Bytes(invitation_id.to_vec()),
        Value::Integer(issue.role.into()),
        Value::Bytes(device_id.to_vec()),
        Value::Bytes(signing_public.to_vec()),
        Value::Bytes(agreement_public.to_vec()),
        Value::Integer(key_version.into()),
        Value::Bytes(enrollment_nonce.to_vec()),
        Value::Bytes(issue_head.to_vec()),
    ]);
    if claim_hash != crypto::hash("claim", &cbor::encode(&claim_input)?)? {
        return Err(Error::Invalid("claim transcript hash mismatch"));
    }
    let pending = Value::Array(vec![
        Value::Bytes(invitation_id.to_vec()),
        Value::Bytes(device_id.to_vec()),
        Value::Bytes(signing_public.to_vec()),
        Value::Bytes(agreement_public.to_vec()),
        Value::Integer(key_version.into()),
        Value::Integer(issue.role.into()),
        Value::Bytes(claim_hash.to_vec()),
        Value::Null,
        Value::Null,
    ]);
    let invitation = Value::Array(vec![
        Value::Bytes(invitation_id.to_vec()),
        Value::Bytes(genesis.manager_id.to_vec()),
        Value::Bytes(issue.invite_public.to_vec()),
        Value::Integer(issue.role.into()),
        Value::Bytes(issue.transition_id.to_vec()),
        Value::Integer(2),
    ]);
    let resulting = Value::Map(vec![
        (1, Value::Integer(1)),
        (2, Value::Bytes(genesis.family_id.to_vec())),
        (3, Value::Bytes(genesis.relay_id.to_vec())),
        (4, Value::Integer(1)),
        (5, Value::Array(vec![genesis.manager_row.clone()])),
        (6, Value::Array(vec![pending])),
        (7, Value::Array(vec![invitation])),
    ]);
    if fixed::<32>(&unsigned[7].1)? != crypto::hash("auth-state", &cbor::encode(&resulting)?)? {
        return Err(Error::Invalid("claim state hash mismatch"));
    }
    let core = Value::Array(unsigned[..9].iter().map(|(_, v)| v.clone()).collect());
    if fixed::<32>(&unsigned[10].1)? != crypto::hash("transition-core", &cbor::encode(&core)?)? {
        return Err(Error::Invalid("claim core hash mismatch"));
    }
    let _ = array(&unsigned[9].1, 0)?;
    let signatures = array(&root[1].1, 2)?;
    let mut seen_invite = false;
    let mut seen_device = false;
    let mut prior = None;
    for signature in signatures {
        let pair = array(signature, 2)?;
        let signer = fixed::<16>(&pair[0])?;
        if prior.is_some_and(|p| signer <= p) {
            return Err(Error::Invalid("claim signature order"));
        }
        prior = Some(signer);
        let key = if signer == invitation_id {
            seen_invite = true;
            issue.invite_public
        } else if signer == device_id {
            seen_device = true;
            signing_public
        } else {
            return Err(Error::Invalid("claim signer unknown"));
        };
        crypto::verify_cbor(
            "control-transition",
            &cbor::encode(&root[0].1)?,
            &key,
            &fixed::<64>(&pair[1])?,
        )?;
    }
    if !seen_invite || !seen_device {
        return Err(Error::Invalid("claim missing signer"));
    }
    Ok(ClaimCandidate {
        family_id: genesis.family_id,
        transition_id,
        invitation_id,
        device_id,
        signing_public,
        agreement_public,
        key_version,
        claim_hash,
        role: issue.role,
    })
}

#[derive(Debug, Clone)]
pub(crate) struct TransitionHeader {
    pub transition_id: [u8; 16],
    pub manifest: Vec<ManifestEntry>,
}

struct FollowingPlan<'a> {
    parent: [u8; 32],
    kind: u64,
    epoch: u64,
    resulting: &'a Value,
    expected_signers: &'a [([u8; 16], [u8; 32])],
    manifest_kinds: &'a [u16],
}

#[allow(dead_code)] // Shared by challenge, proof, and admission validators.
fn verify_following(
    candidate_bytes: &[u8],
    genesis: &GenesisCandidate,
    plan: FollowingPlan<'_>,
) -> Result<TransitionHeader, Error> {
    let value = cbor::decode_with_limits(
        candidate_bytes,
        cbor::Limits {
            max_bytes: 1024 * 1024,
            max_depth: 16,
        },
    )?;
    let root = exact_map(&value, 2)?;
    let unsigned = exact_map(&root[0].1, 11)?;
    if number(&unsigned[0].1)? != 1
        || fixed::<16>(&unsigned[1].1)? != genesis.family_id
        || fixed::<32>(&unsigned[2].1)? != genesis.relay_id
        || fixed::<32>(&unsigned[3].1)? != plan.parent
        || number(&unsigned[5].1)? != plan.kind
        || number(&unsigned[8].1)? != plan.epoch
    {
        return Err(Error::Invalid("transition context mismatch"));
    }
    let transition_id = fixed::<16>(&unsigned[4].1)?;
    if fixed::<32>(&unsigned[7].1)? != crypto::hash("auth-state", &cbor::encode(plan.resulting)?)? {
        return Err(Error::Invalid("resulting state hash mismatch"));
    }
    let core = Value::Array(
        unsigned[..9]
            .iter()
            .map(|(_, value)| value.clone())
            .collect(),
    );
    if fixed::<32>(&unsigned[10].1)? != crypto::hash("transition-core", &cbor::encode(&core)?)? {
        return Err(Error::Invalid("transition core hash mismatch"));
    }
    let manifest = array(&unsigned[9].1, plan.manifest_kinds.len())?;
    let mut parsed = Vec::with_capacity(manifest.len());
    let mut prior = None;
    for (item, expected_kind) in manifest.iter().zip(plan.manifest_kinds) {
        let fields = array(item, 4)?;
        let kind: u16 = number(&fields[0])?
            .try_into()
            .map_err(|_| Error::Invalid("manifest kind range"))?;
        let object_id = fixed::<16>(&fields[1])?;
        let object_hash = fixed::<32>(&fields[2])?;
        let object_len: u32 = number(&fields[3])?
            .try_into()
            .map_err(|_| Error::Invalid("manifest length range"))?;
        if kind != *expected_kind
            || object_len > 1024 * 1024
            || prior.is_some_and(|p| (kind, object_id) <= p)
        {
            return Err(Error::Invalid("manifest kind or order invalid"));
        }
        prior = Some((kind, object_id));
        parsed.push(ManifestEntry {
            kind,
            object_id,
            object_hash,
            object_len,
        });
    }
    let signatures = array(&root[1].1, plan.expected_signers.len())?;
    let mut prior = None;
    for (signature, (expected_id, key)) in signatures.iter().zip(plan.expected_signers) {
        let pair = array(signature, 2)?;
        let signer = fixed::<16>(&pair[0])?;
        if signer != *expected_id || prior.is_some_and(|p| signer <= p) {
            return Err(Error::Invalid("transition signer or order invalid"));
        }
        prior = Some(signer);
        crypto::verify_cbor(
            "control-transition",
            &cbor::encode(&root[0].1)?,
            key,
            &fixed::<64>(&pair[1])?,
        )?;
    }
    Ok(TransitionHeader {
        transition_id,
        manifest: parsed,
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
    let value = cbor::decode_with_limits(
        candidate_bytes,
        cbor::Limits {
            max_bytes: 1024 * 1024,
            max_depth: 16,
        },
    )?;
    let root = exact_map(&value, 2)?;
    let unsigned = exact_map(&root[0].1, 11)?;
    let delta = exact_map(&unsigned[6].1, 4)?;
    let invitation_id = fixed::<16>(&delta[0].1)?;
    let device_id = fixed::<16>(&delta[1].1)?;
    let challenge_id = fixed::<16>(&delta[2].1)?;
    let challenge_hash = fixed::<32>(&delta[3].1)?;
    if invitation_id != issue.invitation_id
        || device_id != claim.device_id
        || challenge_id == genesis.transition_id
        || challenge_id == issue.transition_id
        || challenge_id == claim.transition_id
    {
        return Err(Error::Invalid("challenge target or ID invalid"));
    }
    let context_bytes = cbor::encode(&Value::Array(vec![
        Value::Bytes(genesis.family_id.to_vec()),
        Value::Bytes(genesis.relay_id.to_vec()),
        Value::Bytes(invitation_id.to_vec()),
        Value::Bytes(device_id.to_vec()),
        Value::Bytes(claim.claim_hash.to_vec()),
        Value::Bytes(challenge_id.to_vec()),
        Value::Bytes(claim.agreement_public.to_vec()),
        Value::Integer(claim.key_version.into()),
        Value::Bytes(claim_head.to_vec()),
    ]))?;
    let resulting = state_with_pending(genesis, issue, claim, Some(challenge_id), None);
    let header = verify_following(
        candidate_bytes,
        genesis,
        FollowingPlan {
            parent: claim_head,
            kind: 11,
            epoch: 1,
            resulting: &resulting,
            expected_signers: &[(genesis.manager_id, genesis.manager_signing_key)],
            manifest_kinds: &[2, 3],
        },
    )?;
    if header.transition_id == genesis.transition_id
        || header.transition_id == issue.transition_id
        || header.transition_id == claim.transition_id
        || header.manifest[0].object_id == header.manifest[1].object_id
        || header.manifest.iter().any(|entry| {
            genesis
                .manifest
                .iter()
                .any(|prior| prior.object_id == entry.object_id)
                || entry.object_id == issue.manifest.object_id
        })
    {
        return Err(Error::Invalid("challenge transition or object ID reused"));
    }
    Ok(ChallengeCandidate {
        transition_id: header.transition_id,
        challenge_id,
        challenge_hash,
        context_bytes,
        manifest: header.manifest,
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
    let value = cbor::decode_with_limits(
        candidate_bytes,
        cbor::Limits {
            max_bytes: 1024 * 1024,
            max_depth: 16,
        },
    )?;
    let root = exact_map(&value, 2)?;
    let unsigned = exact_map(&root[0].1, 11)?;
    let delta = exact_map(&unsigned[6].1, 4)?;
    let invitation_id = fixed::<16>(&delta[0].1)?;
    let device_id = fixed::<16>(&delta[1].1)?;
    let challenge_hash = fixed::<32>(&delta[2].1)?;
    let proof_signature = fixed::<64>(&delta[3].1)?;
    if invitation_id != issue.invitation_id
        || device_id != claim.device_id
        || challenge_hash != challenge.challenge_hash
    {
        return Err(Error::Invalid("proof target or challenge mismatch"));
    }
    let proof_hash = crypto::hash(
        "proof",
        &cbor::encode(&Value::Array(vec![
            Value::Bytes(challenge.challenge_id.to_vec()),
            Value::Bytes(proof_signature.to_vec()),
        ]))?,
    )?;
    let resulting = state_with_pending(
        genesis,
        issue,
        claim,
        Some(challenge.challenge_id),
        Some(proof_hash),
    );
    let header = verify_following(
        candidate_bytes,
        genesis,
        FollowingPlan {
            parent: challenge_head,
            kind: 5,
            epoch: 1,
            resulting: &resulting,
            expected_signers: &[(claim.device_id, claim.signing_public)],
            manifest_kinds: &[],
        },
    )?;
    if [
        genesis.transition_id,
        issue.transition_id,
        claim.transition_id,
        challenge.transition_id,
    ]
    .contains(&header.transition_id)
    {
        return Err(Error::Invalid("proof transition ID reused"));
    }
    Ok(ProofCandidate {
        transition_id: header.transition_id,
        proof_hash,
    })
}

#[derive(Debug, Clone)]
#[allow(dead_code)] // Consumed by the admission staging transaction.
pub(crate) struct AdmissionCandidate {
    pub transition_id: [u8; 16],
    pub recipient_id: [u8; 16],
    pub manifest: Vec<ManifestEntry>,
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
    let value = cbor::decode_with_limits(
        candidate_bytes,
        cbor::Limits {
            max_bytes: 1024 * 1024,
            max_depth: 16,
        },
    )?;
    let root = exact_map(&value, 2)?;
    let unsigned = exact_map(&root[0].1, 11)?;
    let delta = exact_map(&unsigned[6].1, 3)?;
    let target_id = fixed::<16>(&delta[0].1)?;
    let old_role = number(&delta[1].1)?;
    let new_commitment = fixed::<32>(&delta[2].1)?;
    if target_id != claim.device_id
        || old_role != issue.role
        || new_commitment == genesis.epoch_commitment
    {
        return Err(Error::Invalid(
            "first removal target, role, or key commitment invalid",
        ));
    }
    let invitation = Value::Array(vec![
        Value::Bytes(issue.invitation_id.to_vec()),
        Value::Bytes(genesis.manager_id.to_vec()),
        Value::Bytes(issue.invite_public.to_vec()),
        Value::Integer(issue.role.into()),
        Value::Bytes(issue.transition_id.to_vec()),
        Value::Integer(2),
    ]);
    let resulting = Value::Map(vec![
        (1, Value::Integer(1)),
        (2, Value::Bytes(genesis.family_id.to_vec())),
        (3, Value::Bytes(genesis.relay_id.to_vec())),
        (4, Value::Integer(2)),
        (5, Value::Array(vec![genesis.manager_row.clone()])),
        (6, Value::Array(vec![])),
        (7, Value::Array(vec![invitation])),
    ]);
    let header = verify_following(
        candidate_bytes,
        genesis,
        FollowingPlan {
            parent: admission_head,
            kind: 8,
            epoch: 2,
            resulting: &resulting,
            expected_signers: &[(genesis.manager_id, genesis.manager_signing_key)],
            manifest_kinds: &[1, 4, 5],
        },
    )?;
    if header.transition_id == admission.transition_id
        || header.transition_id == genesis.transition_id
        || header.transition_id == issue.transition_id
        || header.transition_id == claim.transition_id
    {
        return Err(Error::Invalid("removal transition ID reused"));
    }
    Ok(FirstRemovalCandidate {
        transition_id: header.transition_id,
        manifest: header.manifest,
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
    let value = cbor::decode_with_limits(
        candidate_bytes,
        cbor::Limits {
            max_bytes: 1024 * 1024,
            max_depth: 16,
        },
    )?;
    let root = exact_map(&value, 2)?;
    let unsigned = exact_map(&root[0].1, 11)?;
    let delta = exact_map(&unsigned[6].1, 4)?;
    let invitation_id = fixed::<16>(&delta[0].1)?;
    let recipient_id = fixed::<16>(&delta[1].1)?;
    let role = number(&delta[2].1)?;
    let commitment = fixed::<32>(&delta[3].1)?;
    if invitation_id != issue.invitation_id
        || recipient_id != claim.device_id
        || role != issue.role
        || commitment != genesis.epoch_commitment
    {
        return Err(Error::Invalid(
            "admission target, role, or key commitment mismatch",
        ));
    }
    let recipient_row = Value::Array(vec![
        Value::Bytes(recipient_id.to_vec()),
        Value::Bytes(claim.signing_public.to_vec()),
        Value::Bytes(claim.agreement_public.to_vec()),
        Value::Integer(claim.key_version.into()),
        Value::Integer(role.into()),
    ]);
    let mut active = vec![genesis.manager_row.clone(), recipient_row];
    active.sort_by(|left, right| {
        let Value::Array(left) = left else {
            unreachable!()
        };
        let Value::Array(right) = right else {
            unreachable!()
        };
        let Value::Bytes(left) = &left[0] else {
            unreachable!()
        };
        let Value::Bytes(right) = &right[0] else {
            unreachable!()
        };
        left.cmp(right)
    });
    let invitation = Value::Array(vec![
        Value::Bytes(issue.invitation_id.to_vec()),
        Value::Bytes(genesis.manager_id.to_vec()),
        Value::Bytes(issue.invite_public.to_vec()),
        Value::Integer(issue.role.into()),
        Value::Bytes(issue.transition_id.to_vec()),
        Value::Integer(2),
    ]);
    let resulting = Value::Map(vec![
        (1, Value::Integer(1)),
        (2, Value::Bytes(genesis.family_id.to_vec())),
        (3, Value::Bytes(genesis.relay_id.to_vec())),
        (4, Value::Integer(1)),
        (5, Value::Array(active)),
        (6, Value::Array(vec![])),
        (7, Value::Array(vec![invitation])),
    ]);
    let header = verify_following(
        candidate_bytes,
        genesis,
        FollowingPlan {
            parent: proof_head,
            kind: 6,
            epoch: 1,
            resulting: &resulting,
            expected_signers: &[(genesis.manager_id, genesis.manager_signing_key)],
            manifest_kinds: &[1, 4],
        },
    )?;
    if [
        genesis.transition_id,
        issue.transition_id,
        claim.transition_id,
        challenge.transition_id,
        proof.transition_id,
    ]
    .contains(&header.transition_id)
        || header.manifest[0].object_id == header.manifest[1].object_id
        || header.manifest.iter().any(|entry| {
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
        transition_id: header.transition_id,
        recipient_id,
        manifest: header.manifest,
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
