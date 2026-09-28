//! Pure recipient proof construction for native and wasm enrollment.

use crate::{
    cbor::{self, Value},
    control_chain::{self, ControlChain},
    crypto, handoff,
};

#[derive(Debug)]
pub enum Error {
    Cbor(cbor::Error),
    Chain(control_chain::Error),
    Crypto(crypto::Error),
    Handoff(handoff::Error),
    Authority(babytrack_wire::authority::Error),
    Invalid(&'static str),
}
impl From<cbor::Error> for Error {
    fn from(value: cbor::Error) -> Self {
        Self::Cbor(value)
    }
}
impl From<control_chain::Error> for Error {
    fn from(value: control_chain::Error) -> Self {
        Self::Chain(value)
    }
}
impl From<crypto::Error> for Error {
    fn from(value: crypto::Error) -> Self {
        Self::Crypto(value)
    }
}
impl From<handoff::Error> for Error {
    fn from(value: handoff::Error) -> Self {
        Self::Handoff(value)
    }
}
impl From<babytrack_wire::authority::Error> for Error {
    fn from(value: babytrack_wire::authority::Error) -> Self {
        Self::Authority(value)
    }
}

/// The challenge must already be in the verified control chain. The HPKE
/// object is checked against that commitment and opened with this device's
/// private key before a proof can be signed.
#[allow(clippy::too_many_arguments)]
pub fn build_candidate(
    chain: &ControlChain,
    invitation_id: [u8; 16],
    device_id: [u8; 16],
    signing_seed: &[u8; 32],
    agreement_private: &[u8; 32],
    hpke_object: &[u8],
    transition_id: [u8; 16],
) -> Result<(Vec<u8>, [u8; 64]), Error> {
    let challenge = chain
        .latest_challenge(&invitation_id)
        .ok_or(Error::Invalid("no verified challenge"))?;
    let proof = challenge.prepare_proof(hpke_object, agreement_private, signing_seed)?;
    let original_state = cbor::decode(&chain.state_bytes()?)?;
    let mut state = original_state.clone();
    let Value::Map(fields) = &mut state else {
        return Err(Error::Invalid("authority state not map"));
    };
    let Value::Array(pending) = &mut fields[5].1 else {
        return Err(Error::Invalid("pending state not array"));
    };
    let row = pending
        .iter_mut()
        .find(|item| {
            matches!(item, Value::Array(parts) if parts.len() == 9
            && parts[0] == Value::Bytes(invitation_id.to_vec())
            && parts[1] == Value::Bytes(device_id.to_vec()))
        })
        .ok_or(Error::Invalid("proof pending target absent"))?;
    let Value::Array(row) = row else {
        unreachable!()
    };
    if row[7] != Value::Bytes(challenge.challenge_id.to_vec()) {
        return Err(Error::Invalid("proof pending context mismatch"));
    }
    row[8] = Value::Bytes(proof.proof_hash.to_vec());
    let delta = Value::Map(vec![
        (1, Value::Bytes(invitation_id.to_vec())),
        (2, Value::Bytes(device_id.to_vec())),
        (3, Value::Bytes(challenge.challenge_hash.to_vec())),
        (4, Value::Bytes(proof.signature.to_vec())),
    ]);
    let core = vec![
        Value::Integer(1),
        Value::Bytes(chain.family_id().to_vec()),
        Value::Bytes(chain.relay_id().to_vec()),
        Value::Bytes(chain.head_hash().to_vec()),
        Value::Bytes(transition_id.to_vec()),
        Value::Integer(5),
        delta,
        Value::Bytes(crypto::hash("auth-state", &cbor::encode(&state)?)?.to_vec()),
        Value::Integer(chain.epoch()?.into()),
    ];
    let core_hash = crypto::hash(
        "transition-core",
        &cbor::encode(&Value::Array(core.clone()))?,
    )?;
    let unsigned = Value::Map(
        core.into_iter()
            .enumerate()
            .map(|(index, value)| (index as u64 + 1, value))
            .chain([
                (10, Value::Array(vec![])),
                (11, Value::Bytes(core_hash.to_vec())),
            ])
            .collect(),
    );
    let signature = crypto::sign_cbor(
        "control-transition",
        &cbor::encode(&unsigned)?,
        signing_seed,
    )?;
    let candidate = cbor::encode(&Value::Map(vec![
        (1, unsigned),
        (
            2,
            Value::Array(vec![Value::Array(vec![
                Value::Bytes(device_id.to_vec()),
                Value::Bytes(signature.to_vec()),
            ])]),
        ),
    ]))?;
    babytrack_wire::authority::prepare_proof(
        &candidate,
        &original_state,
        chain.head_hash(),
        challenge.challenge_id,
        challenge.challenge_hash,
    )?;
    Ok((candidate, proof.signature))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value as Json;

    fn hex(value: &str) -> Vec<u8> {
        value
            .as_bytes()
            .chunks_exact(2)
            .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
            .collect()
    }
    fn fixed<const N: usize>(value: &str) -> [u8; N] {
        hex(value).try_into().unwrap()
    }

    #[test]
    fn browser_ready_proof_builder_matches_published_transition() {
        let fixture: Json =
            serde_json::from_str(include_str!("../../tests/vectors/contiguous-chain-v1.json"))
                .unwrap();
        let input = &fixture["test_only_inputs"];
        let rows = fixture["transitions"].as_array().unwrap();
        let relay =
            crypto::signing_public_key(&fixed(input["relay_sign_seed_hex"].as_str().unwrap()));
        let mut chain = ControlChain::from_genesis(
            &hex(rows[0]["committed_cbor_hex"].as_str().unwrap()),
            relay,
        )
        .unwrap();
        for row in &rows[1..4] {
            chain
                .apply_control(&hex(row["committed_cbor_hex"].as_str().unwrap()))
                .unwrap();
        }
        let proof_unsigned =
            cbor::decode(&hex(rows[4]["unsigned_cbor_hex"].as_str().unwrap())).unwrap();
        let Value::Map(fields) = &proof_unsigned else {
            panic!("proof unsigned not map")
        };
        let Value::Bytes(transition_id) = &fields[4].1 else {
            panic!("transition ID not bytes")
        };
        let challenge_object_id = rows[3]["manifest"][0][1].as_str().unwrap();
        let object = hex(fixture["objects_by_id_hex"][challenge_object_id]
            .as_str()
            .unwrap());
        let invitation = crate::bootstrap::InvitationBootstrap::from_fragment(
            fixture["bootstrap"]["fragment"].as_str().unwrap(),
        )
        .unwrap();
        let (candidate, _) = build_candidate(
            &chain,
            invitation.invitation_id(),
            fixed(input["recipient_device_id_hex"].as_str().unwrap()),
            &fixed(input["recipient_sign_seed_hex"].as_str().unwrap()),
            &fixed(input["recipient_agreement_seed_hex"].as_str().unwrap()),
            &object,
            transition_id.as_slice().try_into().unwrap(),
        )
        .unwrap();
        let expected = cbor::encode(&Value::Map(vec![
            (1, proof_unsigned),
            (
                2,
                cbor::decode(&hex(rows[4]["signatures_cbor_hex"].as_str().unwrap())).unwrap(),
            ),
        ]))
        .unwrap();
        assert_eq!(candidate, expected);
    }
}
