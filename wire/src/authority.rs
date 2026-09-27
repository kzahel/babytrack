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
