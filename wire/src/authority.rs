//! Deterministic public candidate checks shared by clients and the relay.
//! Commit time, relay receipts, storage, and encrypted object opening belong
//! to their respective callers.

use crate::{
    cbor::{self, Value},
    crypto,
};

#[derive(Debug, Clone, PartialEq, Eq)]
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
pub struct ManifestEntry {
    pub kind: u16,
    pub object_id: [u8; 16],
    pub object_hash: [u8; 32],
    pub object_len: u32,
}

#[derive(Debug, Clone)]
pub struct PreparedIssue {
    pub family_id: [u8; 16],
    pub transition_id: [u8; 16],
    pub invitation_id: [u8; 16],
    pub issuer_id: [u8; 16],
    pub invite_public: [u8; 32],
    pub role: u8,
    pub manifest: ManifestEntry,
    pub next_state: Value,
}

#[derive(Debug, Clone)]
pub struct PreparedClaim {
    pub family_id: [u8; 16],
    pub transition_id: [u8; 16],
    pub invitation_id: [u8; 16],
    pub device_id: [u8; 16],
    pub signing_public: [u8; 32],
    pub agreement_public: [u8; 32],
    pub key_version: u64,
    pub claim_hash: [u8; 32],
    pub role: u64,
    pub next_state: Value,
}

#[derive(Debug, Clone)]
pub struct PreparedFollowing {
    pub transition_id: [u8; 16],
    pub manifest: Vec<ManifestEntry>,
}

#[derive(Debug, Clone)]
pub struct PreparedChallenge {
    pub transition_id: [u8; 16],
    pub invitation_id: [u8; 16],
    pub device_id: [u8; 16],
    pub challenge_id: [u8; 16],
    pub challenge_hash: [u8; 32],
    pub context_bytes: Vec<u8>,
    pub signer_id: [u8; 16],
    pub signer_key: [u8; 32],
    pub pending_sign_public: [u8; 32],
    pub pending_agree_public: [u8; 32],
    pub pending_key_version: u32,
    pub manifest: Vec<ManifestEntry>,
    pub next_state: Value,
}

#[derive(Debug, Clone)]
pub struct PreparedProof {
    pub transition_id: [u8; 16],
    pub invitation_id: [u8; 16],
    pub device_id: [u8; 16],
    pub signing_public: [u8; 32],
    pub proof_hash: [u8; 32],
    pub next_state: Value,
}

#[derive(Debug, Clone)]
pub struct PreparedAdmission {
    pub transition_id: [u8; 16],
    pub invitation_id: [u8; 16],
    pub device_id: [u8; 16],
    pub role: u8,
    pub agreement_public: [u8; 32],
    pub key_version: u32,
    pub commitment: [u8; 32],
    pub core_hash: [u8; 32],
    pub signer_id: [u8; 16],
    pub signer_key: [u8; 32],
    pub manifest: Vec<ManifestEntry>,
    pub next_state: Value,
}

#[derive(Debug, Clone)]
pub struct PreparedRemoval {
    pub transition_id: [u8; 16],
    pub target_id: [u8; 16],
    pub new_epoch: u32,
    pub new_commitment: [u8; 32],
    pub signer_id: [u8; 16],
    pub signer_key: [u8; 32],
    pub manifest: Vec<ManifestEntry>,
    pub next_state: Value,
}

/// Remove an active device, advance the epoch, and invalidate outstanding
/// challenges. The caller checks grant/keyring object contents and history.
pub fn prepare_removal(
    candidate_bytes: &[u8],
    state: &Value,
    head: [u8; 32],
    current_commitment: [u8; 32],
) -> Result<PreparedRemoval, Error> {
    let candidate = cbor::decode_with_limits(
        candidate_bytes,
        cbor::Limits {
            max_bytes: 1024 * 1024,
            max_depth: 16,
        },
    )?;
    let root = exact_map(&candidate, 2)?;
    let unsigned = exact_map(&root[0].1, 11)?;
    let delta = exact_map(&unsigned[6].1, 3)?;
    let target_id = fixed::<16>(&delta[0].1)?;
    let old_role = number(&delta[1].1)?;
    let new_commitment = fixed::<32>(&delta[2].1)?;
    if new_commitment == current_commitment {
        return Err(Error::Invalid("rotation must change epoch commitment"));
    }
    let old = exact_map(state, 7)?;
    if number(&old[0].1)? != 1 {
        return Err(Error::Invalid("removal state version"));
    }
    let family_id = fixed::<16>(&old[1].1)?;
    let relay_id = fixed::<32>(&old[2].1)?;
    let next_epoch: u32 = number(&old[3].1)?
        .checked_add(1)
        .ok_or(Error::Invalid("removal epoch overflow"))?
        .try_into()
        .map_err(|_| Error::Invalid("removal epoch range"))?;
    let Value::Array(active) = &old[4].1 else {
        return Err(Error::Invalid("removal active state not array"));
    };
    let target = active
        .iter()
        .find_map(|row| {
            let fields = array(row, 5).ok()?;
            (fixed::<16>(&fields[0]).ok()? == target_id).then_some(fields)
        })
        .ok_or(Error::Invalid("removal target not active"))?;
    if number(&target[4])? != old_role {
        return Err(Error::Invalid("removal prior role"));
    }
    let signatures = array(&root[1].1, 1)?;
    let signer_id = fixed::<16>(&array(&signatures[0], 2)?[0])?;
    let signer = active
        .iter()
        .find_map(|row| {
            let fields = array(row, 5).ok()?;
            (fixed::<16>(&fields[0]).ok()? == signer_id).then_some(fields)
        })
        .ok_or(Error::Invalid("removal signer not active"))?;
    if number(&signer[4])? != 2 {
        return Err(Error::Invalid("removal signer not manager"));
    }
    let signer_key = fixed::<32>(&signer[1])?;
    let mut next_state = state.clone();
    let Value::Map(next) = &mut next_state else {
        unreachable!()
    };
    next[3].1 = Value::Integer(next_epoch.into());
    let Value::Array(next_active) = &mut next[4].1 else {
        unreachable!()
    };
    next_active.retain(|row| {
        !array(row, 5).is_ok_and(|fields| fixed::<16>(&fields[0]).ok() == Some(target_id))
    });
    if !next_active
        .iter()
        .any(|row| array(row, 5).is_ok_and(|fields| number(&fields[4]).ok() == Some(2)))
    {
        return Err(Error::Invalid("removal would leave no manager"));
    }
    let remaining = next_active.len();
    let Value::Array(invitations) = &mut next[6].1 else {
        return Err(Error::Invalid("removal invitations not array"));
    };
    for invitation in invitations {
        let fields = array_mut(invitation, 6)?;
        if fixed::<16>(&fields[1])? == target_id && number(&fields[5])? == 1 {
            fields[5] = Value::Integer(3);
        }
    }
    let Value::Array(pending) = &mut next[5].1 else {
        return Err(Error::Invalid("removal pending not array"));
    };
    for row in pending {
        let fields = array_mut(row, 9)?;
        fields[7] = Value::Null;
        fields[8] = Value::Null;
    }
    let mut kinds = Vec::with_capacity(remaining + 2);
    kinds.push(1);
    kinds.extend(std::iter::repeat_n(4, remaining));
    kinds.push(5);
    let header = prepare_following(
        candidate_bytes,
        family_id,
        relay_id,
        head,
        8,
        u64::from(next_epoch),
        &next_state,
        &[(signer_id, signer_key)],
        &kinds,
    )?;
    Ok(PreparedRemoval {
        transition_id: header.transition_id,
        target_id,
        new_epoch: next_epoch,
        new_commitment,
        signer_id,
        signer_key,
        manifest: header.manifest,
        next_state,
    })
}

/// Move a proved pending device into active membership for the current epoch.
/// Callers check that the membership and grant objects match their manifests.
pub fn prepare_admission(
    candidate_bytes: &[u8],
    state: &Value,
    head: [u8; 32],
    current_commitment: [u8; 32],
) -> Result<PreparedAdmission, Error> {
    let candidate = cbor::decode_with_limits(
        candidate_bytes,
        cbor::Limits {
            max_bytes: 1024 * 1024,
            max_depth: 16,
        },
    )?;
    let root = exact_map(&candidate, 2)?;
    let unsigned = exact_map(&root[0].1, 11)?;
    let delta = exact_map(&unsigned[6].1, 4)?;
    let invitation_id = fixed::<16>(&delta[0].1)?;
    let device_id = fixed::<16>(&delta[1].1)?;
    let role: u8 = number(&delta[2].1)?
        .try_into()
        .map_err(|_| Error::Invalid("admission role range"))?;
    let commitment = fixed::<32>(&delta[3].1)?;
    if commitment != current_commitment {
        return Err(Error::Invalid("admission epoch commitment"));
    }
    let old = exact_map(state, 7)?;
    if number(&old[0].1)? != 1 {
        return Err(Error::Invalid("admission state version"));
    }
    let family_id = fixed::<16>(&old[1].1)?;
    let relay_id = fixed::<32>(&old[2].1)?;
    let epoch = number(&old[3].1)?;
    let Value::Array(pending) = &old[5].1 else {
        return Err(Error::Invalid("admission pending state not array"));
    };
    let row = pending
        .iter()
        .find_map(|row| {
            let fields = array(row, 9).ok()?;
            (fixed::<16>(&fields[0]).ok()? == invitation_id).then_some(fields)
        })
        .ok_or(Error::Invalid("admission invitation not pending"))?;
    if fixed::<16>(&row[1])? != device_id
        || number(&row[5])? != u64::from(role)
        || !matches!(&row[8], Value::Bytes(bytes) if bytes.len() == 32)
    {
        return Err(Error::Invalid("admission role, device, or proof"));
    }
    let signing_public = fixed::<32>(&row[2])?;
    let agreement_public = fixed::<32>(&row[3])?;
    let key_version: u32 = number(&row[4])?
        .try_into()
        .map_err(|_| Error::Invalid("admission key version range"))?;
    let Value::Array(active) = &old[4].1 else {
        return Err(Error::Invalid("admission active state not array"));
    };
    if active.iter().any(|row| {
        array(row, 5).is_ok_and(|fields| fixed::<16>(&fields[0]).ok() == Some(device_id))
    }) {
        return Err(Error::Invalid("admission recipient already active"));
    }
    let signatures = array(&root[1].1, 1)?;
    let signer_id = fixed::<16>(&array(&signatures[0], 2)?[0])?;
    let signer = active
        .iter()
        .find_map(|row| {
            let fields = array(row, 5).ok()?;
            (fixed::<16>(&fields[0]).ok()? == signer_id).then_some(fields)
        })
        .ok_or(Error::Invalid("admission signer not active"))?;
    let signer_key = fixed::<32>(&signer[1])?;
    let mut next_state = state.clone();
    let Value::Map(next) = &mut next_state else {
        unreachable!()
    };
    let Value::Array(next_pending) = &mut next[5].1 else {
        unreachable!()
    };
    next_pending.retain(|row| {
        !array(row, 9).is_ok_and(|fields| fixed::<16>(&fields[0]).ok() == Some(invitation_id))
    });
    let Value::Array(next_active) = &mut next[4].1 else {
        unreachable!()
    };
    next_active.push(Value::Array(vec![
        Value::Bytes(device_id.to_vec()),
        Value::Bytes(signing_public.to_vec()),
        Value::Bytes(agreement_public.to_vec()),
        Value::Integer(key_version.into()),
        Value::Integer(role.into()),
    ]));
    next_active.sort_by(|left, right| {
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
    let header = prepare_following(
        candidate_bytes,
        family_id,
        relay_id,
        head,
        6,
        epoch,
        &next_state,
        &[(signer_id, signer_key)],
        &[1, 4],
    )?;
    Ok(PreparedAdmission {
        transition_id: header.transition_id,
        invitation_id,
        device_id,
        role,
        agreement_public,
        key_version,
        commitment,
        core_hash: fixed::<32>(&unsigned[10].1)?,
        signer_id,
        signer_key,
        manifest: header.manifest,
        next_state,
    })
}

/// Bind a device-signed proof transition to the latest verified challenge.
/// The challenge object's private proof is checked when opened by a client.
pub fn prepare_proof(
    candidate_bytes: &[u8],
    state: &Value,
    head: [u8; 32],
    expected_challenge_id: [u8; 16],
    expected_challenge_hash: [u8; 32],
) -> Result<PreparedProof, Error> {
    let candidate = cbor::decode_with_limits(
        candidate_bytes,
        cbor::Limits {
            max_bytes: 1024 * 1024,
            max_depth: 16,
        },
    )?;
    let root = exact_map(&candidate, 2)?;
    let unsigned = exact_map(&root[0].1, 11)?;
    let delta = exact_map(&unsigned[6].1, 4)?;
    let invitation_id = fixed::<16>(&delta[0].1)?;
    let device_id = fixed::<16>(&delta[1].1)?;
    let challenge_hash = fixed::<32>(&delta[2].1)?;
    let proof_signature = fixed::<64>(&delta[3].1)?;
    let old = exact_map(state, 7)?;
    if number(&old[0].1)? != 1 {
        return Err(Error::Invalid("proof state version"));
    }
    let family_id = fixed::<16>(&old[1].1)?;
    let relay_id = fixed::<32>(&old[2].1)?;
    let epoch = number(&old[3].1)?;
    let Value::Array(pending) = &old[5].1 else {
        return Err(Error::Invalid("proof pending state not array"));
    };
    let row = pending
        .iter()
        .find_map(|row| {
            let fields = array(row, 9).ok()?;
            (fixed::<16>(&fields[0]).ok()? == invitation_id).then_some(fields)
        })
        .ok_or(Error::Invalid("proof invitation not pending"))?;
    if fixed::<16>(&row[1])? != device_id
        || row[7] != Value::Bytes(expected_challenge_id.to_vec())
        || challenge_hash != expected_challenge_hash
    {
        return Err(Error::Invalid("proof does not match latest challenge"));
    }
    let signer_key = fixed::<32>(&row[2])?;
    let proof_hash = crypto::hash(
        "proof",
        &cbor::encode(&Value::Array(vec![
            Value::Bytes(expected_challenge_id.to_vec()),
            Value::Bytes(proof_signature.to_vec()),
        ]))?,
    )?;
    let mut next_state = state.clone();
    let Value::Map(next) = &mut next_state else {
        unreachable!()
    };
    let Value::Array(next_pending) = &mut next[5].1 else {
        unreachable!()
    };
    let updated = next_pending
        .iter_mut()
        .find(|row| {
            array(row, 9).is_ok_and(|fields| fixed::<16>(&fields[0]).ok() == Some(invitation_id))
        })
        .ok_or(Error::Invalid("proof pending row disappeared"))?;
    let Value::Array(fields) = updated else {
        unreachable!()
    };
    fields[8] = Value::Bytes(proof_hash.to_vec());
    let header = prepare_following(
        candidate_bytes,
        family_id,
        relay_id,
        head,
        5,
        epoch,
        &next_state,
        &[(device_id, signer_key)],
        &[],
    )?;
    Ok(PreparedProof {
        transition_id: header.transition_id,
        invitation_id,
        device_id,
        signing_public: signer_key,
        proof_hash,
        next_state,
    })
}

/// Prepare a holder challenge from the current pending row. The referenced
/// challenge objects and historical ID registry are checked by the caller.
pub fn prepare_challenge(
    candidate_bytes: &[u8],
    state: &Value,
    head: [u8; 32],
) -> Result<PreparedChallenge, Error> {
    let candidate = cbor::decode_with_limits(
        candidate_bytes,
        cbor::Limits {
            max_bytes: 1024 * 1024,
            max_depth: 16,
        },
    )?;
    let root = exact_map(&candidate, 2)?;
    let unsigned = exact_map(&root[0].1, 11)?;
    let delta = exact_map(&unsigned[6].1, 4)?;
    let invitation_id = fixed::<16>(&delta[0].1)?;
    let device_id = fixed::<16>(&delta[1].1)?;
    let challenge_id = fixed::<16>(&delta[2].1)?;
    let challenge_hash = fixed::<32>(&delta[3].1)?;
    let old = exact_map(state, 7)?;
    if number(&old[0].1)? != 1 {
        return Err(Error::Invalid("challenge state version"));
    }
    let family_id = fixed::<16>(&old[1].1)?;
    let relay_id = fixed::<32>(&old[2].1)?;
    let epoch = number(&old[3].1)?;
    let Value::Array(pending) = &old[5].1 else {
        return Err(Error::Invalid("pending state not array"));
    };
    let pending_row = pending
        .iter()
        .find_map(|row| {
            let fields = array(row, 9).ok()?;
            (fixed::<16>(&fields[0]).ok()? == invitation_id).then_some(fields)
        })
        .ok_or(Error::Invalid("challenge invitation not pending"))?;
    if fixed::<16>(&pending_row[1])? != device_id {
        return Err(Error::Invalid("challenge device mismatch"));
    }
    let claim_hash = fixed::<32>(&pending_row[6])?;
    let agreement_public = fixed::<32>(&pending_row[3])?;
    let signing_public = fixed::<32>(&pending_row[2])?;
    let key_version: u32 = number(&pending_row[4])?
        .try_into()
        .map_err(|_| Error::Invalid("challenge key version range"))?;
    let _: u32 = epoch
        .try_into()
        .map_err(|_| Error::Invalid("challenge epoch range"))?;
    let context_bytes = cbor::encode(&Value::Array(vec![
        Value::Bytes(family_id.to_vec()),
        Value::Bytes(relay_id.to_vec()),
        Value::Bytes(invitation_id.to_vec()),
        Value::Bytes(device_id.to_vec()),
        Value::Bytes(claim_hash.to_vec()),
        Value::Bytes(challenge_id.to_vec()),
        Value::Bytes(agreement_public.to_vec()),
        Value::Integer(key_version.into()),
        Value::Bytes(head.to_vec()),
    ]))?;
    let signatures = array(&root[1].1, 1)?;
    let signer_id = fixed::<16>(&array(&signatures[0], 2)?[0])?;
    let Value::Array(active) = &old[4].1 else {
        return Err(Error::Invalid("active state not array"));
    };
    let signer = active
        .iter()
        .find_map(|row| {
            let fields = array(row, 5).ok()?;
            (fixed::<16>(&fields[0]).ok()? == signer_id).then_some(fields)
        })
        .ok_or(Error::Invalid("challenge signer not active"))?;
    let signer_key = fixed::<32>(&signer[1])?;
    let mut next_state = state.clone();
    let Value::Map(next) = &mut next_state else {
        unreachable!()
    };
    let Value::Array(next_pending) = &mut next[5].1 else {
        unreachable!()
    };
    let updated = next_pending
        .iter_mut()
        .find(|row| {
            array(row, 9).is_ok_and(|fields| fixed::<16>(&fields[0]).ok() == Some(invitation_id))
        })
        .ok_or(Error::Invalid("challenge pending row disappeared"))?;
    let Value::Array(fields) = updated else {
        unreachable!()
    };
    fields[7] = Value::Bytes(challenge_id.to_vec());
    fields[8] = Value::Null;
    let header = prepare_following(
        candidate_bytes,
        family_id,
        relay_id,
        head,
        11,
        epoch,
        &next_state,
        &[(signer_id, signer_key)],
        &[2, 3],
    )?;
    Ok(PreparedChallenge {
        transition_id: header.transition_id,
        invitation_id,
        device_id,
        challenge_id,
        challenge_hash,
        context_bytes,
        signer_id,
        signer_key,
        pending_sign_public: signing_public,
        pending_agree_public: agreement_public,
        pending_key_version: key_version,
        manifest: header.manifest,
        next_state,
    })
}

/// Verify the common signed envelope for a deterministic public transition.
/// The caller derives `next_state` and signer authority for the particular
/// transition; historical IDs, receipts, and object bytes remain external.
#[allow(clippy::too_many_arguments)]
pub fn prepare_following(
    candidate_bytes: &[u8],
    family_id: [u8; 16],
    relay_id: [u8; 32],
    head: [u8; 32],
    kind: u64,
    epoch: u64,
    next_state: &Value,
    expected_signers: &[([u8; 16], [u8; 32])],
    manifest_kinds: &[u64],
) -> Result<PreparedFollowing, Error> {
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
        || fixed::<16>(&unsigned[1].1)? != family_id
        || fixed::<32>(&unsigned[2].1)? != relay_id
        || fixed::<32>(&unsigned[3].1)? != head
        || number(&unsigned[5].1)? != kind
        || number(&unsigned[8].1)? != epoch
    {
        return Err(Error::Invalid("transition context"));
    }
    let transition_id = fixed::<16>(&unsigned[4].1)?;
    if fixed::<32>(&unsigned[7].1)? != crypto::hash("auth-state", &cbor::encode(next_state)?)? {
        return Err(Error::Invalid("transition state hash"));
    }
    let core = Value::Array(
        unsigned[..9]
            .iter()
            .map(|(_, value)| value.clone())
            .collect(),
    );
    if fixed::<32>(&unsigned[10].1)? != crypto::hash("transition-core", &cbor::encode(&core)?)? {
        return Err(Error::Invalid("transition core hash"));
    }
    let listed = array(&unsigned[9].1, manifest_kinds.len())?;
    let mut manifest = Vec::with_capacity(listed.len());
    let mut prior = None;
    for (entry, expected_kind) in listed.iter().zip(manifest_kinds) {
        let fields = array(entry, 4)?;
        let kind = number(&fields[0])?;
        let object_id = fixed::<16>(&fields[1])?;
        let object_hash = fixed::<32>(&fields[2])?;
        let object_len = number(&fields[3])?;
        if kind != *expected_kind
            || kind > u16::MAX as u64
            || object_len > 1024 * 1024
            || prior.is_some_and(|p| (kind, object_id) <= p)
        {
            return Err(Error::Invalid("transition manifest kind, size, or order"));
        }
        prior = Some((kind, object_id));
        manifest.push(ManifestEntry {
            kind: kind as u16,
            object_id,
            object_hash,
            object_len: object_len as u32,
        });
    }
    let signatures = array(&root[1].1, expected_signers.len())?;
    let mut prior = None;
    let unsigned_bytes = cbor::encode(&root[0].1)?;
    for (entry, (expected_id, public_key)) in signatures.iter().zip(expected_signers) {
        let fields = array(entry, 2)?;
        let signer_id = fixed::<16>(&fields[0])?;
        if signer_id != *expected_id || prior.is_some_and(|p| signer_id <= p) {
            return Err(Error::Invalid("transition signer set or order"));
        }
        prior = Some(signer_id);
        crypto::verify_cbor(
            "control-transition",
            &unsigned_bytes,
            public_key,
            &fixed::<64>(&fields[1])?,
        )?;
    }
    Ok(PreparedFollowing {
        transition_id,
        manifest,
    })
}

/// Prepare a two-signature claim against verified public state. Historical ID
/// reuse and commit-time invitation expiry remain the callers' checks.
pub fn prepare_claim(
    candidate_bytes: &[u8],
    state: &Value,
    head: [u8; 32],
) -> Result<PreparedClaim, Error> {
    let candidate = cbor::decode_with_limits(
        candidate_bytes,
        cbor::Limits {
            max_bytes: 1024 * 1024,
            max_depth: 16,
        },
    )?;
    let root = exact_map(&candidate, 2)?;
    let unsigned = exact_map(&root[0].1, 11)?;
    let old = exact_map(state, 7)?;
    let family_id = fixed::<16>(&old[1].1)?;
    let relay_id = fixed::<32>(&old[2].1)?;
    let epoch = number(&old[3].1)?;
    if number(&old[0].1)? != 1
        || number(&unsigned[0].1)? != 1
        || fixed::<16>(&unsigned[1].1)? != family_id
        || fixed::<32>(&unsigned[2].1)? != relay_id
        || fixed::<32>(&unsigned[3].1)? != head
        || number(&unsigned[5].1)? != 4
        || number(&unsigned[8].1)? != epoch
    {
        return Err(Error::Invalid(
            "claim version, identity, head, kind, or epoch",
        ));
    }
    let transition_id = fixed::<16>(&unsigned[4].1)?;
    let delta = exact_map(&unsigned[6].1, 7)?;
    let invitation_id = fixed::<16>(&delta[0].1)?;
    let device_id = fixed::<16>(&delta[1].1)?;
    let signing_public = fixed::<32>(&delta[2].1)?;
    let agreement_public = fixed::<32>(&delta[3].1)?;
    let key_version = number(&delta[4].1)?;
    let enrollment_nonce = fixed::<32>(&delta[5].1)?;
    let claim_hash = fixed::<32>(&delta[6].1)?;
    if key_version == 0
        || key_version > u32::MAX as u64
        || transition_id == invitation_id
        || transition_id == device_id
        || invitation_id == device_id
    {
        return Err(Error::Invalid("claim key version or ID collision"));
    }
    let Value::Array(invitations) = &old[6].1 else {
        return Err(Error::Invalid("invitations not array"));
    };
    let invitation = invitations
        .iter()
        .find_map(|row| {
            let fields = array(row, 6).ok()?;
            (fixed::<16>(&fields[0]).ok()? == invitation_id).then_some(fields)
        })
        .ok_or(Error::Invalid("claim invitation absent"))?;
    if number(&invitation[5])? != 1 {
        return Err(Error::Invalid("claim invitation not unused"));
    }
    let issuer_id = fixed::<16>(&invitation[1])?;
    let invite_public = fixed::<32>(&invitation[2])?;
    let role = number(&invitation[3])?;
    let Value::Array(active) = &old[4].1 else {
        return Err(Error::Invalid("active state not array"));
    };
    if active.iter().any(|row| {
        array(row, 5).is_ok_and(|fields| fixed::<16>(&fields[0]).ok() == Some(device_id))
    }) {
        return Err(Error::Invalid("claim device already active"));
    }
    let issuer = active
        .iter()
        .find_map(|row| {
            let fields = array(row, 5).ok()?;
            (fixed::<16>(&fields[0]).ok()? == issuer_id).then_some(fields)
        })
        .ok_or(Error::Invalid("claim issuer not active"))?;
    if number(&issuer[4])? != 2 {
        return Err(Error::Invalid("claim issuer not manager"));
    }
    let Value::Array(pending) = &old[5].1 else {
        return Err(Error::Invalid("pending state not array"));
    };
    if pending.iter().any(|row| {
        array(row, 9).is_ok_and(|fields| fixed::<16>(&fields[1]).ok() == Some(device_id))
    }) {
        return Err(Error::Invalid("claim device already pending"));
    }
    let transcript = Value::Array(vec![
        Value::Bytes(family_id.to_vec()),
        Value::Bytes(relay_id.to_vec()),
        Value::Bytes(invitation_id.to_vec()),
        Value::Integer(role.into()),
        Value::Bytes(device_id.to_vec()),
        Value::Bytes(signing_public.to_vec()),
        Value::Bytes(agreement_public.to_vec()),
        Value::Integer(key_version.into()),
        Value::Bytes(enrollment_nonce.to_vec()),
        Value::Bytes(head.to_vec()),
    ]);
    if claim_hash != crypto::hash("claim", &cbor::encode(&transcript)?)? {
        return Err(Error::Invalid("claim transcript hash"));
    }
    let mut next_state = state.clone();
    let Value::Map(next) = &mut next_state else {
        unreachable!()
    };
    let Value::Array(next_pending) = &mut next[5].1 else {
        unreachable!()
    };
    next_pending.push(Value::Array(vec![
        Value::Bytes(invitation_id.to_vec()),
        Value::Bytes(device_id.to_vec()),
        Value::Bytes(signing_public.to_vec()),
        Value::Bytes(agreement_public.to_vec()),
        Value::Integer(key_version.into()),
        Value::Integer(role.into()),
        Value::Bytes(claim_hash.to_vec()),
        Value::Null,
        Value::Null,
    ]));
    next_pending.sort_by(|left, right| {
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
    let Value::Array(next_invitations) = &mut next[6].1 else {
        unreachable!()
    };
    let updated = next_invitations
        .iter_mut()
        .find(|row| {
            array(row, 6).is_ok_and(|fields| fixed::<16>(&fields[0]).ok() == Some(invitation_id))
        })
        .ok_or(Error::Invalid("claim invitation disappeared"))?;
    let Value::Array(fields) = updated else {
        unreachable!()
    };
    fields[5] = Value::Integer(2);
    if fixed::<32>(&unsigned[7].1)? != crypto::hash("auth-state", &cbor::encode(&next_state)?)? {
        return Err(Error::Invalid("claim state hash"));
    }
    let core = Value::Array(
        unsigned[..9]
            .iter()
            .map(|(_, value)| value.clone())
            .collect(),
    );
    if fixed::<32>(&unsigned[10].1)? != crypto::hash("transition-core", &cbor::encode(&core)?)? {
        return Err(Error::Invalid("claim core hash"));
    }
    let _ = array(&unsigned[9].1, 0)?;
    let signatures = array(&root[1].1, 2)?;
    let mut seen_invite = false;
    let mut seen_device = false;
    let mut prior = None;
    let unsigned_bytes = cbor::encode(&root[0].1)?;
    for signature in signatures {
        let pair = array(signature, 2)?;
        let signer = fixed::<16>(&pair[0])?;
        if prior.is_some_and(|previous| signer <= previous) {
            return Err(Error::Invalid("claim signature order"));
        }
        prior = Some(signer);
        let key = if signer == invitation_id {
            seen_invite = true;
            invite_public
        } else if signer == device_id {
            seen_device = true;
            signing_public
        } else {
            return Err(Error::Invalid("claim signer unknown"));
        };
        crypto::verify_cbor(
            "control-transition",
            &unsigned_bytes,
            &key,
            &fixed::<64>(&pair[1])?,
        )?;
    }
    if !seen_invite || !seen_device {
        return Err(Error::Invalid("claim missing signer"));
    }
    Ok(PreparedClaim {
        family_id,
        transition_id,
        invitation_id,
        device_id,
        signing_public,
        agreement_public,
        key_version,
        claim_hash,
        role,
        next_state,
    })
}

/// Check the signed issue and its deterministic public state effect against
/// an already verified state/head. The caller checks the historical ID ledger,
/// staged object bytes, commit-time rules, and the relay-signed receipt.
pub fn prepare_issue(
    candidate_bytes: &[u8],
    state: &Value,
    head: [u8; 32],
) -> Result<PreparedIssue, Error> {
    let candidate = cbor::decode_with_limits(
        candidate_bytes,
        cbor::Limits {
            max_bytes: 1024 * 1024,
            max_depth: 16,
        },
    )?;
    let root = exact_map(&candidate, 2)?;
    let unsigned = exact_map(&root[0].1, 11)?;
    let old = exact_map(state, 7)?;
    let family_id = fixed::<16>(&old[1].1)?;
    let relay_id = fixed::<32>(&old[2].1)?;
    let epoch = number(&old[3].1)?;
    if number(&old[0].1)? != 1
        || number(&unsigned[0].1)? != 1
        || fixed::<16>(&unsigned[1].1)? != family_id
        || fixed::<32>(&unsigned[2].1)? != relay_id
        || fixed::<32>(&unsigned[3].1)? != head
        || number(&unsigned[5].1)? != 2
        || number(&unsigned[8].1)? != epoch
    {
        return Err(Error::Invalid(
            "issue version, identity, head, kind, or epoch",
        ));
    }
    let transition_id = fixed::<16>(&unsigned[4].1)?;
    let delta = exact_map(&unsigned[6].1, 4)?;
    let invitation_id = fixed::<16>(&delta[0].1)?;
    let issuer_id = fixed::<16>(&delta[1].1)?;
    let invite_public = fixed::<32>(&delta[2].1)?;
    let role: u8 = number(&delta[3].1)?
        .try_into()
        .map_err(|_| Error::Invalid("issue role range"))?;
    if !matches!(role, 1 | 2) || transition_id == invitation_id {
        return Err(Error::Invalid("issue role or ID collision"));
    }
    let Value::Array(active) = &old[4].1 else {
        return Err(Error::Invalid("active state not array"));
    };
    let issuer = active
        .iter()
        .find_map(|row| {
            let fields = array(row, 5).ok()?;
            (fixed::<16>(&fields[0]).ok()? == issuer_id).then_some(fields)
        })
        .ok_or(Error::Invalid("issue signer not active"))?;
    if number(&issuer[4])? != 2 {
        return Err(Error::Invalid("issue signer not manager"));
    }
    let signing_key = fixed::<32>(&issuer[1])?;
    let Value::Array(invitations) = &old[6].1 else {
        return Err(Error::Invalid("invitations not array"));
    };
    if invitations.iter().any(|row| {
        array(row, 6).is_ok_and(|fields| fixed::<16>(&fields[0]).ok() == Some(invitation_id))
    }) {
        return Err(Error::Invalid("invitation ID reused"));
    }
    let manifest = array(&unsigned[9].1, 1)?;
    let listed = array(&manifest[0], 4)?;
    let kind: u16 = number(&listed[0])?
        .try_into()
        .map_err(|_| Error::Invalid("manifest kind range"))?;
    let object_id = fixed::<16>(&listed[1])?;
    let object_hash = fixed::<32>(&listed[2])?;
    let object_len: u32 = number(&listed[3])?
        .try_into()
        .map_err(|_| Error::Invalid("manifest length range"))?;
    if kind != 1
        || object_len > 1024 * 1024
        || object_id == transition_id
        || object_id == invitation_id
    {
        return Err(Error::Invalid("issue manifest kind, size, or ID"));
    }
    let mut next_state = state.clone();
    let Value::Map(next) = &mut next_state else {
        unreachable!()
    };
    let Value::Array(next_invitations) = &mut next[6].1 else {
        unreachable!()
    };
    next_invitations.push(Value::Array(vec![
        Value::Bytes(invitation_id.to_vec()),
        Value::Bytes(issuer_id.to_vec()),
        Value::Bytes(invite_public.to_vec()),
        Value::Integer(role.into()),
        Value::Bytes(transition_id.to_vec()),
        Value::Integer(1),
    ]));
    next_invitations.sort_by(|left, right| {
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
    if fixed::<32>(&unsigned[7].1)? != crypto::hash("auth-state", &cbor::encode(&next_state)?)? {
        return Err(Error::Invalid("issue state hash"));
    }
    let core = Value::Array(
        unsigned[..9]
            .iter()
            .map(|(_, value)| value.clone())
            .collect(),
    );
    if fixed::<32>(&unsigned[10].1)? != crypto::hash("transition-core", &cbor::encode(&core)?)? {
        return Err(Error::Invalid("issue core hash"));
    }
    let signatures = array(&root[1].1, 1)?;
    let signature = array(&signatures[0], 2)?;
    if fixed::<16>(&signature[0])? != issuer_id {
        return Err(Error::Invalid("issue signature signer"));
    }
    crypto::verify_cbor(
        "control-transition",
        &cbor::encode(&root[0].1)?,
        &signing_key,
        &fixed::<64>(&signature[1])?,
    )?;
    Ok(PreparedIssue {
        family_id,
        transition_id,
        invitation_id,
        issuer_id,
        invite_public,
        role,
        manifest: ManifestEntry {
            kind,
            object_id,
            object_hash,
            object_len,
        },
        next_state,
    })
}

fn exact_map(value: &Value, width: usize) -> Result<&[(u64, Value)], Error> {
    let Value::Map(fields) = value else {
        return Err(Error::Invalid("map expected"));
    };
    if fields.len() != width
        || fields
            .iter()
            .enumerate()
            .any(|(index, (key, _))| *key != index as u64 + 1)
    {
        return Err(Error::Invalid("map keys"));
    }
    Ok(fields)
}
fn array(value: &Value, width: usize) -> Result<&[Value], Error> {
    let Value::Array(items) = value else {
        return Err(Error::Invalid("array expected"));
    };
    if items.len() != width {
        return Err(Error::Invalid("array width"));
    }
    Ok(items)
}
fn array_mut(value: &mut Value, width: usize) -> Result<&mut [Value], Error> {
    let Value::Array(items) = value else {
        return Err(Error::Invalid("array expected"));
    };
    if items.len() != width {
        return Err(Error::Invalid("array width"));
    }
    Ok(items)
}
fn fixed<const N: usize>(value: &Value) -> Result<[u8; N], Error> {
    let Value::Bytes(bytes) = value else {
        return Err(Error::Invalid("bytes expected"));
    };
    bytes
        .as_slice()
        .try_into()
        .map_err(|_| Error::Invalid("bytes length"))
}
fn number(value: &Value) -> Result<u64, Error> {
    let Value::Integer(number) = value else {
        return Err(Error::Invalid("integer expected"));
    };
    (*number)
        .try_into()
        .map_err(|_| Error::Invalid("unsigned integer"))
}
