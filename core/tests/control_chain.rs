use babytrack_core::{
    batch::{self, Header},
    cbor::{self, Value},
    control_chain::{ControlChain, Error as ControlError},
    crypto,
};

fn hex_bytes(hex: &str) -> Vec<u8> {
    hex.as_bytes()
        .chunks_exact(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}

#[test]
fn historical_control_object_and_device_ids_cannot_be_reused() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("../../tests/vectors/contiguous-chain-v1.json")).unwrap();
    let transitions = fixture["transitions"].as_array().unwrap();
    let wire = |index: usize| hex_bytes(transitions[index]["committed_cbor_hex"].as_str().unwrap());
    let relay_public =
        bytes::<32>("2543b92ff1095511476adc8369db6ddc933665a11978dda1404ee1066ca9559d");
    let mut chain = ControlChain::from_genesis(&wire(0), relay_public).unwrap();

    let mut reused_transition = cbor::decode(&wire(1)).unwrap();
    if let Value::Map(root) = &mut reused_transition
        && let Value::Map(unsigned) = &mut root[0].1
    {
        unsigned[4].1 = Value::Bytes(transition_id(&transitions[0]).to_vec());
    }
    assert_eq!(
        chain.apply_invite_issue(&cbor::encode(&reused_transition).unwrap()),
        Err(ControlError::Invalid("transition ID reused"))
    );

    let mut reused_object = cbor::decode(&wire(1)).unwrap();
    if let Value::Map(root) = &mut reused_object
        && let Value::Map(unsigned) = &mut root[0].1
        && let Value::Array(manifest) = &mut unsigned[9].1
        && let Value::Array(entry) = &mut manifest[0]
    {
        entry[1] = Value::Bytes(hex_bytes(
            transitions[0]["manifest"][0][1].as_str().unwrap(),
        ));
    }
    assert_eq!(
        chain.apply_invite_issue(&cbor::encode(&reused_object).unwrap()),
        Err(ControlError::Invalid("object ID reused"))
    );
    assert_eq!(chain.last_global_cursor(), 1);

    let mut invitation_equals_transition = cbor::decode(&wire(1)).unwrap();
    if let Value::Map(root) = &mut invitation_equals_transition
        && let Value::Map(unsigned) = &mut root[0].1
        && let Value::Map(delta) = &mut unsigned[6].1
    {
        delta[0].1 = Value::Bytes(transition_id(&transitions[0]).to_vec());
    }
    assert_eq!(
        chain.apply_invite_issue(&cbor::encode(&invitation_equals_transition).unwrap()),
        Err(ControlError::Invalid(
            "protocol ID reused across categories"
        ))
    );
    let mut transition_equals_device = cbor::decode(&wire(1)).unwrap();
    if let Value::Map(root) = &mut transition_equals_device
        && let Value::Map(unsigned) = &mut root[0].1
    {
        unsigned[4].1 = Value::Bytes(hex_bytes(
            fixture["test_only_inputs"]["manager_device_id_hex"]
                .as_str()
                .unwrap(),
        ));
    }
    assert_eq!(
        chain.apply_invite_issue(&cbor::encode(&transition_equals_device).unwrap()),
        Err(ControlError::Invalid(
            "protocol ID reused across categories"
        ))
    );
    assert_eq!(chain.last_global_cursor(), 1);

    chain.apply_invite_issue(&wire(1)).unwrap();
    let mut reused_device = cbor::decode(&wire(2)).unwrap();
    if let Value::Map(root) = &mut reused_device
        && let Value::Map(unsigned) = &mut root[0].1
        && let Value::Map(delta) = &mut unsigned[6].1
    {
        delta[1].1 = Value::Bytes(hex_bytes(
            fixture["test_only_inputs"]["manager_device_id_hex"]
                .as_str()
                .unwrap(),
        ));
    }
    assert_eq!(
        chain.apply_invite_claim(&cbor::encode(&reused_device).unwrap()),
        Err(ControlError::Invalid(
            "protocol ID reused across categories"
        ))
    );
    chain.apply_invite_claim(&wire(2)).unwrap();

    let mut duplicate_object = cbor::decode(&wire(3)).unwrap();
    if let Value::Map(root) = &mut duplicate_object
        && let Value::Map(unsigned) = &mut root[0].1
        && let Value::Array(manifest) = &mut unsigned[9].1
    {
        let first_id = match &manifest[0] {
            Value::Array(first) => first[1].clone(),
            _ => unreachable!(),
        };
        if let Value::Array(second) = &mut manifest[1] {
            second[1] = first_id;
        }
    }
    assert_eq!(
        chain.apply_holder_challenge(&cbor::encode(&duplicate_object).unwrap()),
        Err(ControlError::Invalid("object ID reused"))
    );
    assert_eq!(chain.last_global_cursor(), 3);
}

#[test]
fn claim_requires_both_keys_and_commits_before_signed_expiry() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("../../tests/vectors/contiguous-chain-v1.json")).unwrap();
    let transitions = fixture["transitions"].as_array().unwrap();
    let genesis = hex_bytes(transitions[0]["committed_cbor_hex"].as_str().unwrap());
    let issue = hex_bytes(transitions[1]["committed_cbor_hex"].as_str().unwrap());
    let claim = hex_bytes(transitions[2]["committed_cbor_hex"].as_str().unwrap());
    let relay_public =
        bytes::<32>("2543b92ff1095511476adc8369db6ddc933665a11978dda1404ee1066ca9559d");
    let relay_seed = bytes::<32>(
        fixture["test_only_inputs"]["relay_sign_seed_hex"]
            .as_str()
            .unwrap(),
    );
    let mut chain = ControlChain::from_genesis(&genesis, relay_public).unwrap();
    chain.apply_invite_issue(&issue).unwrap();
    assert_eq!(chain.apply_invite_claim(&claim), Ok(()));
    assert_eq!(
        chain.state_bytes().unwrap(),
        hex_bytes(transitions[2]["state_cbor_hex"].as_str().unwrap())
    );
    assert_eq!(
        chain.head_hash(),
        bytes(transitions[2]["head_hash_hex"].as_str().unwrap())
    );
    assert!(chain.apply_invite_claim(&claim).is_err());

    let mut no_device_signature = cbor::decode(&claim).unwrap();
    if let Value::Map(root) = &mut no_device_signature
        && let Value::Array(signatures) = &mut root[1].1
    {
        signatures.pop();
    }
    let mut clean = ControlChain::from_genesis(&genesis, relay_public).unwrap();
    clean.apply_invite_issue(&issue).unwrap();
    assert!(
        clean
            .apply_invite_claim(&cbor::encode(&no_device_signature).unwrap())
            .is_err()
    );
    assert_eq!(clean.last_global_cursor(), 2);

    let issue_value = cbor::decode(&issue).unwrap();
    let Value::Map(issue_fields) = issue_value else {
        unreachable!()
    };
    let Value::Array(issue_receipt) = &issue_fields[2].1 else {
        unreachable!()
    };
    let Value::Integer(issue_ms) = issue_receipt[4] else {
        unreachable!()
    };
    for (time, should_accept) in [
        (issue_ms + 604_800_000 - 1, true),
        (issue_ms + 604_800_000, false),
    ] {
        let mut value = cbor::decode(&claim).unwrap();
        if let Value::Map(root) = &mut value {
            if let Value::Array(receipt) = &mut root[2].1 {
                receipt[4] = Value::Integer(time);
            }
            let signature = crypto::sign_cbor(
                "control-receipt",
                &cbor::encode(&root[2].1).unwrap(),
                &relay_seed,
            )
            .unwrap();
            root[3].1 = Value::Bytes(signature.to_vec());
        }
        let mut boundary = ControlChain::from_genesis(&genesis, relay_public).unwrap();
        boundary.apply_invite_issue(&issue).unwrap();
        assert_eq!(
            boundary
                .apply_invite_claim(&cbor::encode(&value).unwrap())
                .is_ok(),
            should_accept
        );
    }
}

#[test]
fn holder_challenge_and_pending_proof_follow_the_latest_public_chain() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("../../tests/vectors/contiguous-chain-v1.json")).unwrap();
    let transitions = fixture["transitions"].as_array().unwrap();
    let relay_public =
        bytes::<32>("2543b92ff1095511476adc8369db6ddc933665a11978dda1404ee1066ca9559d");
    let wire = |index: usize| hex_bytes(transitions[index]["committed_cbor_hex"].as_str().unwrap());
    let mut chain = ControlChain::from_genesis(&wire(0), relay_public).unwrap();
    chain.apply_invite_issue(&wire(1)).unwrap();
    let epoch_one = bytes::<32>(
        fixture["test_only_inputs"]["epoch_1_key_hex"]
            .as_str()
            .unwrap(),
    );
    let verified_issue_key = chain.verify_initial_epoch_key(&epoch_one).unwrap();
    let issue_membership = chain
        .membership_check(&transition_id(&transitions[1]))
        .unwrap();
    issue_membership
        .verify(
            &object_bytes(&fixture, issue_membership.object_id()),
            &verified_issue_key,
        )
        .unwrap();
    chain.apply_invite_claim(&wire(2)).unwrap();
    assert!(chain.apply_key_proof(&wire(4)).is_err());
    assert_eq!(chain.last_global_cursor(), 3);
    chain.apply_holder_challenge(&wire(3)).unwrap();
    assert_eq!(
        chain.state_bytes().unwrap(),
        hex_bytes(transitions[3]["state_cbor_hex"].as_str().unwrap())
    );
    assert_eq!(chain.last_global_cursor(), 4);
    chain.apply_key_proof(&wire(4)).unwrap();
    assert_eq!(
        chain.state_bytes().unwrap(),
        hex_bytes(transitions[4]["state_cbor_hex"].as_str().unwrap())
    );
    assert_eq!(
        chain.head_hash(),
        bytes(transitions[4]["head_hash_hex"].as_str().unwrap())
    );
    assert!(chain.apply_key_proof(&wire(4)).is_err());
}

#[test]
fn admission_moves_proved_pending_device_to_active_and_repair_preserves_role() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("../../tests/vectors/contiguous-chain-v1.json")).unwrap();
    let transitions = fixture["transitions"].as_array().unwrap();
    let relay_public =
        bytes::<32>("2543b92ff1095511476adc8369db6ddc933665a11978dda1404ee1066ca9559d");
    let wire = |index: usize| hex_bytes(transitions[index]["committed_cbor_hex"].as_str().unwrap());
    let mut chain = ControlChain::from_genesis(&wire(0), relay_public).unwrap();
    chain.apply_invite_issue(&wire(1)).unwrap();
    chain.apply_invite_claim(&wire(2)).unwrap();
    chain.apply_holder_challenge(&wire(3)).unwrap();
    assert!(chain.apply_admit_grant(&wire(5)).is_err());
    chain.apply_key_proof(&wire(4)).unwrap();
    chain.apply_admit_grant(&wire(5)).unwrap();
    assert_eq!(chain.last_global_cursor(), 6);
    let recipient_id = bytes::<16>(
        fixture["test_only_inputs"]["recipient_device_id_hex"]
            .as_str()
            .unwrap(),
    );
    let grant = chain.initial_admission_grant(&recipient_id).unwrap();
    let grant_object = hex_bytes(
        fixture["objects_by_id_hex"]["923e4567e89b42d3a456426614174000"]
            .as_str()
            .unwrap(),
    );
    let agreement_private = bytes::<32>(
        fixture["test_only_inputs"]["recipient_agreement_seed_hex"]
            .as_str()
            .unwrap(),
    );
    let opened = grant.open(&grant_object, &agreement_private).unwrap();
    let expected_key = bytes::<32>(
        fixture["test_only_inputs"]["epoch_1_key_hex"]
            .as_str()
            .unwrap(),
    );
    assert_eq!(
        opened,
        chain.verify_initial_epoch_key(&expected_key).unwrap()
    );
    let admission_membership = chain
        .membership_check(&transition_id(&transitions[5]))
        .unwrap();
    admission_membership
        .verify(
            &object_bytes(&fixture, admission_membership.object_id()),
            &opened,
        )
        .unwrap();
    let mut changed_membership = object_bytes(&fixture, admission_membership.object_id());
    *changed_membership.last_mut().unwrap() ^= 1;
    assert!(
        admission_membership
            .verify(&changed_membership, &opened)
            .is_err()
    );
    assert!(grant.open(&grant_object, &[0u8; 32]).is_err());
    let mut tampered_grant = grant_object.clone();
    *tampered_grant.last_mut().unwrap() ^= 1;
    assert!(grant.open(&tampered_grant, &agreement_private).is_err());
    assert_eq!(
        chain.state_bytes().unwrap(),
        hex_bytes(transitions[5]["state_cbor_hex"].as_str().unwrap())
    );
    let Value::Map(repair_root) = cbor::decode(&wire(6)).unwrap() else {
        panic!()
    };
    let Value::Map(repair_unsigned) = &repair_root[0].1 else {
        panic!()
    };
    let Value::Map(repair_delta) = &repair_unsigned[6].1 else {
        panic!()
    };
    let manager_id = bytes::<16>(
        fixture["test_only_inputs"]["manager_device_id_hex"]
            .as_str()
            .unwrap(),
    );
    let manager_seed = bytes::<32>(
        fixture["test_only_inputs"]["manager_sign_seed_hex"]
            .as_str()
            .unwrap(),
    );
    let relay_seed = bytes::<32>(
        fixture["test_only_inputs"]["relay_sign_seed_hex"]
            .as_str()
            .unwrap(),
    );
    let wrong_admission = test_control(
        &chain,
        10,
        v4(244),
        Value::Map(vec![
            (1, Value::Bytes(recipient_id.to_vec())),
            (2, Value::Bytes(v4(245).to_vec())),
            (3, repair_delta[2].1.clone()),
        ]),
        cbor::decode(&chain.state_bytes().unwrap()).unwrap(),
        (manager_id, manager_seed),
        relay_seed,
    );
    assert!(chain.apply_grant_repair(&wrong_admission).is_err());
    assert_eq!(chain.last_global_cursor(), 6);
    chain.apply_grant_repair(&wire(6)).unwrap();
    let repair_grant = chain.latest_repair_grant(&recipient_id).unwrap();
    let repair_object = hex_bytes(
        fixture["objects_by_id_hex"]["963e4567e89b42d3a456426614174000"]
            .as_str()
            .unwrap(),
    );
    assert_eq!(
        repair_grant
            .open(&repair_object, &agreement_private)
            .unwrap(),
        opened
    );
    assert!(repair_grant.open(&repair_object, &[0u8; 32]).is_err());
    let repair_membership = chain
        .membership_check(&transition_id(&transitions[6]))
        .unwrap();
    repair_membership
        .verify(
            &object_bytes(&fixture, repair_membership.object_id()),
            &opened,
        )
        .unwrap();
    assert_eq!(chain.last_global_cursor(), 7);
    assert_eq!(
        chain.state_bytes().unwrap(),
        hex_bytes(transitions[6]["state_cbor_hex"].as_str().unwrap())
    );
    assert_eq!(
        chain.head_hash(),
        bytes(transitions[6]["head_hash_hex"].as_str().unwrap())
    );
    let batch = &fixture["batch"];
    let envelope = hex_bytes(batch["envelope_cbor_hex"].as_str().unwrap());
    let receipt = hex_bytes(batch["receipt_cbor_hex"].as_str().unwrap());
    assert!(chain.apply_public_batch(&envelope, &receipt).is_ok());
    assert_eq!(chain.last_global_cursor(), 8);
    assert!(chain.apply_public_batch(&envelope, &receipt).is_err());
    chain.apply_remove_active(&wire(7)).unwrap();
    let epoch_two = bytes::<32>(
        fixture["test_only_inputs"]["epoch_2_key_hex"]
            .as_str()
            .unwrap(),
    );
    assert!(chain.verify_initial_epoch_key(&epoch_two).is_err());
    let removal_id = transition_id(&transitions[7]);
    let rotation = chain.rotation(&removal_id).unwrap();
    let grants: Vec<_> = rotation
        .grant_ids()
        .into_iter()
        .map(|id| (id, object_bytes(&fixture, id)))
        .collect();
    let keyring = object_bytes(&fixture, rotation.keyring_id());
    let manager_id = bytes::<16>(
        fixture["test_only_inputs"]["manager_device_id_hex"]
            .as_str()
            .unwrap(),
    );
    let manager_agreement = bytes::<32>(
        fixture["test_only_inputs"]["manager_agreement_seed_hex"]
            .as_str()
            .unwrap(),
    );
    let removal_membership = chain.membership_check(&removal_id).unwrap();
    let membership_object = object_bytes(&fixture, removal_membership.object_id());
    let keys = chain
        .open_rotation_for(
            &removal_id,
            manager_id,
            &manager_agreement,
            &grants,
            &keyring,
            &membership_object,
        )
        .unwrap();
    assert_ne!(keys.current(), &opened);
    assert_eq!(keys.earlier(1), Some(&opened));
    assert!(
        chain
            .open_rotation_for(
                &removal_id,
                recipient_id,
                &agreement_private,
                &grants,
                &keyring,
                &membership_object,
            )
            .is_err()
    );
    let mut changed_keyring = keyring.clone();
    *changed_keyring.last_mut().unwrap() ^= 1;
    assert!(
        chain
            .open_rotation_for(
                &removal_id,
                manager_id,
                &manager_agreement,
                &grants,
                &changed_keyring,
                &membership_object,
            )
            .is_err()
    );
    let mut changed_membership = membership_object.clone();
    *changed_membership.last_mut().unwrap() ^= 1;
    assert!(
        chain
            .open_rotation_for(
                &removal_id,
                manager_id,
                &manager_agreement,
                &grants,
                &keyring,
                &changed_membership,
            )
            .is_err()
    );
    removal_membership
        .verify(&membership_object, keys.current())
        .unwrap();
    assert!(
        removal_membership
            .verify(&membership_object, &opened)
            .is_err()
    );
    assert_eq!(chain.last_global_cursor(), 9);
    assert_eq!(
        chain.state_bytes().unwrap(),
        hex_bytes(transitions[7]["state_cbor_hex"].as_str().unwrap())
    );
    assert_eq!(
        chain.head_hash(),
        bytes(transitions[7]["head_hash_hex"].as_str().unwrap())
    );
    assert!(chain.apply_public_batch(&envelope, &receipt).is_err());
    assert_eq!(chain.last_global_cursor(), 9);
    let mut stale_header =
        Header::decode(&hex_bytes(batch["header_cbor_hex"].as_str().unwrap())).unwrap();
    stale_header.batch_id[15] ^= 1;
    stale_header.nonce[0] ^= 1;
    stale_header.device_sequence = 2;
    let old_key = bytes::<32>(
        fixture["test_only_inputs"]["epoch_1_key_hex"]
            .as_str()
            .unwrap(),
    );
    let recipient_seed = bytes::<32>(
        fixture["test_only_inputs"]["recipient_sign_seed_hex"]
            .as_str()
            .unwrap(),
    );
    let operation = hex_bytes(batch["operation_cbor_hex"].as_str().unwrap());
    let stale = batch::seal(&stale_header, &[operation], &old_key, &recipient_seed).unwrap();
    assert!(
        chain
            .apply_public_batch(&stale.envelope_bytes, &receipt)
            .is_err()
    );
    assert_eq!(chain.last_global_cursor(), 9);
}

#[test]
fn committed_challenge_objects_prove_both_pending_keys_without_granting_data_access() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("../../tests/vectors/contiguous-chain-v1.json")).unwrap();
    let transitions = fixture["transitions"].as_array().unwrap();
    let relay_public =
        bytes::<32>("2543b92ff1095511476adc8369db6ddc933665a11978dda1404ee1066ca9559d");
    let wire = |index: usize| hex_bytes(transitions[index]["committed_cbor_hex"].as_str().unwrap());
    let mut chain = ControlChain::from_genesis(&wire(0), relay_public).unwrap();
    chain.apply_invite_issue(&wire(1)).unwrap();
    chain.apply_invite_claim(&wire(2)).unwrap();
    chain.apply_holder_challenge(&wire(3)).unwrap();
    let invitation_id = bytes::<16>("623e4567e89b42d3a456426614174000");
    let challenge = chain.latest_challenge(&invitation_id).unwrap();
    let objects = &fixture["objects_by_id_hex"];
    let hpke_object = hex_bytes(
        objects["363e4567e89b42d3a456426614174000"]
            .as_str()
            .unwrap(),
    );
    let verifier_object = hex_bytes(
        objects["373e4567e89b42d3a456426614174000"]
            .as_str()
            .unwrap(),
    );
    let agree_seed = bytes::<32>(
        fixture["test_only_inputs"]["recipient_agreement_seed_hex"]
            .as_str()
            .unwrap(),
    );
    let sign_seed = bytes::<32>(
        fixture["test_only_inputs"]["recipient_sign_seed_hex"]
            .as_str()
            .unwrap(),
    );
    let proof = challenge
        .prepare_proof(&hpke_object, &agree_seed, &sign_seed)
        .unwrap();
    let proof_control = cbor::decode(&wire(4)).unwrap();
    let Value::Map(root) = &proof_control else {
        unreachable!()
    };
    let Value::Map(unsigned) = &root[0].1 else {
        unreachable!()
    };
    let Value::Map(delta) = &unsigned[6].1 else {
        unreachable!()
    };
    assert_eq!(delta[3].1, Value::Bytes(proof.signature.to_vec()));
    assert!(
        challenge
            .prepare_proof(&hpke_object, &[0u8; 32], &sign_seed)
            .is_err()
    );
    let mut tampered = hpke_object.clone();
    *tampered.last_mut().unwrap() ^= 1;
    assert!(
        challenge
            .prepare_proof(&tampered, &agree_seed, &sign_seed)
            .is_err()
    );
    chain.apply_key_proof(&wire(4)).unwrap();
    let proof_state = cbor::decode(&chain.state_bytes().unwrap()).unwrap();
    let Value::Map(state) = proof_state else {
        unreachable!()
    };
    let Value::Array(pending) = &state[5].1 else {
        unreachable!()
    };
    let Value::Array(row) = &pending[0] else {
        unreachable!()
    };
    assert_eq!(row[8], Value::Bytes(proof.proof_hash.to_vec()));
    let key = bytes::<32>(
        fixture["test_only_inputs"]["epoch_1_key_hex"]
            .as_str()
            .unwrap(),
    );
    let verified_key = chain.verify_initial_epoch_key(&key).unwrap();
    assert!(
        chain
            .verify_latest_holder_proof(
                &invitation_id,
                &verifier_object,
                &verified_key,
                &proof.signature
            )
            .is_ok()
    );
    let mut wrong_proof = proof.signature;
    wrong_proof[0] ^= 1;
    assert!(
        chain
            .verify_latest_holder_proof(
                &invitation_id,
                &verifier_object,
                &verified_key,
                &wrong_proof
            )
            .is_err()
    );
    assert!(chain.verify_initial_epoch_key(&[0u8; 32]).is_err());
}
fn bytes<const N: usize>(hex: &str) -> [u8; N] {
    hex_bytes(hex).try_into().unwrap()
}

fn transition_id(case: &serde_json::Value) -> [u8; 16] {
    let unsigned = cbor::decode(&hex_bytes(case["unsigned_cbor_hex"].as_str().unwrap())).unwrap();
    let Value::Map(fields) = unsigned else {
        unreachable!()
    };
    let Value::Bytes(id) = &fields[4].1 else {
        unreachable!()
    };
    id.as_slice().try_into().unwrap()
}

fn test_control(
    chain: &ControlChain,
    kind: u64,
    transition_id: [u8; 16],
    delta: Value,
    next_state: Value,
    signer: ([u8; 16], [u8; 32]),
    relay_seed: [u8; 32],
) -> Vec<u8> {
    let Value::Map(state) = &next_state else {
        unreachable!()
    };
    let manifest_bytes = b"opaque membership";
    let mut object_id = transition_id;
    object_id[15] = object_id[15].wrapping_add(100);
    let manifest = if matches!(kind, 3 | 7 | 9) {
        Value::Array(vec![])
    } else {
        Value::Array(vec![Value::Array(vec![
            Value::Integer(1),
            Value::Bytes(object_id.to_vec()),
            Value::Bytes(crypto::hash("object", manifest_bytes).unwrap().to_vec()),
            Value::Integer(manifest_bytes.len() as i128),
        ])])
    };
    let mut parts = vec![
        Value::Integer(1),
        Value::Bytes(chain.family_id().to_vec()),
        Value::Bytes(chain.relay_id().to_vec()),
        Value::Bytes(chain.head_hash().to_vec()),
        Value::Bytes(transition_id.to_vec()),
        Value::Integer(kind.into()),
        delta,
        Value::Bytes(
            crypto::hash("auth-state", &cbor::encode(&next_state).unwrap())
                .unwrap()
                .to_vec(),
        ),
        state[3].1.clone(),
    ];
    let core = crypto::hash(
        "transition-core",
        &cbor::encode(&Value::Array(parts.clone())).unwrap(),
    )
    .unwrap();
    parts.push(manifest);
    parts.push(Value::Bytes(core.to_vec()));
    let unsigned = Value::Map(
        parts
            .into_iter()
            .enumerate()
            .map(|(index, value)| (index as u64 + 1, value))
            .collect(),
    );
    let unsigned_bytes = cbor::encode(&unsigned).unwrap();
    let signatures = Value::Array(vec![Value::Array(vec![
        Value::Bytes(signer.0.to_vec()),
        Value::Bytes(
            crypto::sign_cbor("control-transition", &unsigned_bytes, &signer.1)
                .unwrap()
                .to_vec(),
        ),
    ])]);
    let signed = Value::Array(vec![unsigned.clone(), signatures.clone()]);
    let receipt = Value::Array(vec![
        Value::Bytes(chain.family_id().to_vec()),
        Value::Bytes(chain.relay_id().to_vec()),
        Value::Bytes(transition_id.to_vec()),
        Value::Integer((chain.last_global_cursor() + 1).into()),
        Value::Integer(2_000_000_000_000),
        Value::Bytes(
            crypto::hash("control-signed", &cbor::encode(&signed).unwrap())
                .unwrap()
                .to_vec(),
        ),
    ]);
    let receipt_signature = crypto::sign_cbor(
        "control-receipt",
        &cbor::encode(&receipt).unwrap(),
        &relay_seed,
    )
    .unwrap();
    cbor::encode(&Value::Map(vec![
        (1, unsigned),
        (2, signatures),
        (3, receipt),
        (4, Value::Bytes(receipt_signature.to_vec())),
    ]))
    .unwrap()
}

fn v4(suffix: u8) -> [u8; 16] {
    let mut id = [
        0x12, 0x3e, 0x45, 0x67, 0xe8, 0x9b, 0x42, 0xd3, 0xa4, 0x56, 0x42, 0x66, 0x14, 0x17, 0x40, 0,
    ];
    id[15] = suffix;
    id
}

#[test]
fn manager_cancel_role_and_pending_removal_preserve_fixed_authority() {
    let exact: serde_json::Value = serde_json::from_str(include_str!(
        "../../tests/vectors/no-object-controls-v1.json"
    ))
    .unwrap();
    let check = |name: &str, bytes: &[u8]| {
        let case = exact["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|case| case["name"] == name)
            .unwrap();
        assert_eq!(
            bytes,
            hex_bytes(case["committed_cbor_hex"].as_str().unwrap())
        );
        let Value::Map(root) = cbor::decode(bytes).unwrap() else {
            unreachable!()
        };
        let Value::Map(unsigned) = &root[0].1 else {
            unreachable!()
        };
        assert_eq!(unsigned[9].1, Value::Array(vec![]));
    };
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("../../tests/vectors/contiguous-chain-v1.json")).unwrap();
    let transitions = fixture["transitions"].as_array().unwrap();
    let wire = |index: usize| hex_bytes(transitions[index]["committed_cbor_hex"].as_str().unwrap());
    let relay_public =
        bytes::<32>("2543b92ff1095511476adc8369db6ddc933665a11978dda1404ee1066ca9559d");
    let relay_seed = bytes::<32>(
        fixture["test_only_inputs"]["relay_sign_seed_hex"]
            .as_str()
            .unwrap(),
    );
    let manager_id = bytes::<16>(
        fixture["test_only_inputs"]["manager_device_id_hex"]
            .as_str()
            .unwrap(),
    );
    let manager_seed = bytes::<32>(
        fixture["test_only_inputs"]["manager_sign_seed_hex"]
            .as_str()
            .unwrap(),
    );
    let recipient_id = bytes::<16>(
        fixture["test_only_inputs"]["recipient_device_id_hex"]
            .as_str()
            .unwrap(),
    );
    let recipient_seed = bytes::<32>(
        fixture["test_only_inputs"]["recipient_sign_seed_hex"]
            .as_str()
            .unwrap(),
    );
    let mut chain = ControlChain::from_genesis(&wire(0), relay_public).unwrap();
    chain.apply_control(&wire(1)).unwrap();
    let invitation_id: [u8; 16] = if let Value::Map(state) =
        cbor::decode(&chain.state_bytes().unwrap()).unwrap()
        && let Value::Array(invitations) = &state[6].1
        && let Value::Array(row) = &invitations[0]
        && let Value::Bytes(id) = &row[0]
    {
        id.as_slice().try_into().unwrap()
    } else {
        unreachable!()
    };
    let mut canceled = cbor::decode(&chain.state_bytes().unwrap()).unwrap();
    if let Value::Map(state) = &mut canceled
        && let Value::Array(invitations) = &mut state[6].1
        && let Value::Array(row) = &mut invitations[0]
    {
        row[5] = Value::Integer(3);
    }
    let cancel = test_control(
        &chain,
        3,
        v4(103),
        Value::Map(vec![(1, Value::Bytes(invitation_id.to_vec()))]),
        canceled.clone(),
        (manager_id, manager_seed),
        relay_seed,
    );
    check("invite_cancel", &cancel);
    chain.apply_control(&cancel).unwrap();
    assert_eq!(
        chain.state_bytes().unwrap(),
        cbor::encode(&canceled).unwrap()
    );
    assert!(chain.apply_control(&wire(2)).is_err());

    let mut pending_chain = ControlChain::from_genesis(&wire(0), relay_public).unwrap();
    pending_chain.apply_control(&wire(1)).unwrap();
    pending_chain.apply_control(&wire(2)).unwrap();
    let mut removed = cbor::decode(&pending_chain.state_bytes().unwrap()).unwrap();
    if let Value::Map(state) = &mut removed {
        state[5].1 = Value::Array(vec![]);
    }
    let remove_pending = test_control(
        &pending_chain,
        9,
        v4(109),
        Value::Map(vec![
            (1, Value::Bytes(invitation_id.to_vec())),
            (2, Value::Bytes(recipient_id.to_vec())),
        ]),
        removed.clone(),
        (manager_id, manager_seed),
        relay_seed,
    );
    check("remove_pending", &remove_pending);
    pending_chain.apply_control(&remove_pending).unwrap();
    assert_eq!(
        pending_chain.state_bytes().unwrap(),
        cbor::encode(&removed).unwrap()
    );
    assert!(pending_chain.apply_control(&wire(3)).is_err());

    let mut role_chain = ControlChain::from_genesis(&wire(0), relay_public).unwrap();
    for index in 1..=5 {
        role_chain.apply_control(&wire(index)).unwrap();
    }
    let mut promoted = cbor::decode(&role_chain.state_bytes().unwrap()).unwrap();
    if let Value::Map(state) = &mut promoted
        && let Value::Array(active) = &mut state[4].1
    {
        for row in active {
            if let Value::Array(fields) = row
                && fields[0] == Value::Bytes(recipient_id.to_vec())
            {
                fields[4] = Value::Integer(2);
            }
        }
    }
    let promote = test_control(
        &role_chain,
        7,
        v4(107),
        Value::Map(vec![
            (1, Value::Bytes(recipient_id.to_vec())),
            (2, Value::Integer(1)),
            (3, Value::Integer(2)),
        ]),
        promoted.clone(),
        (manager_id, manager_seed),
        relay_seed,
    );
    check("role_change", &promote);
    role_chain.apply_control(&promote).unwrap();
    assert_eq!(
        role_chain.state_bytes().unwrap(),
        cbor::encode(&promoted).unwrap()
    );
    let extra_invitation_id = v4(201);
    let extra_issue_id = v4(202);
    let extra_invite_public = crypto::signing_public_key(&[9u8; 32]);
    let mut with_unused_invite = promoted;
    if let Value::Map(state) = &mut with_unused_invite
        && let Value::Array(invitations) = &mut state[6].1
    {
        invitations.push(Value::Array(vec![
            Value::Bytes(extra_invitation_id.to_vec()),
            Value::Bytes(manager_id.to_vec()),
            Value::Bytes(extra_invite_public.to_vec()),
            Value::Integer(1),
            Value::Bytes(extra_issue_id.to_vec()),
            Value::Integer(1),
        ]));
        invitations.sort_by(|left, right| {
            let (Value::Array(left), Value::Array(right)) = (left, right) else {
                unreachable!()
            };
            let (Value::Bytes(left), Value::Bytes(right)) = (&left[0], &right[0]) else {
                unreachable!()
            };
            left.cmp(right)
        });
    }
    let extra_issue = test_control(
        &role_chain,
        2,
        extra_issue_id,
        Value::Map(vec![
            (1, Value::Bytes(extra_invitation_id.to_vec())),
            (2, Value::Bytes(manager_id.to_vec())),
            (3, Value::Bytes(extra_invite_public.to_vec())),
            (4, Value::Integer(1)),
        ]),
        with_unused_invite.clone(),
        (manager_id, manager_seed),
        relay_seed,
    );
    role_chain.apply_control(&extra_issue).unwrap();
    let mut other_manager_cancels = role_chain.clone();
    let mut canceled_by_other_manager = with_unused_invite.clone();
    if let Value::Map(state) = &mut canceled_by_other_manager
        && let Value::Array(invitations) = &mut state[6].1
    {
        for row in invitations {
            if let Value::Array(fields) = row
                && fields[0] == Value::Bytes(extra_invitation_id.to_vec())
            {
                fields[5] = Value::Integer(3);
            }
        }
    }
    let cancel_by_other_manager = test_control(
        &other_manager_cancels,
        3,
        v4(204),
        Value::Map(vec![(1, Value::Bytes(extra_invitation_id.to_vec()))]),
        canceled_by_other_manager.clone(),
        (recipient_id, recipient_seed),
        relay_seed,
    );
    other_manager_cancels
        .apply_control(&cancel_by_other_manager)
        .unwrap();
    assert_eq!(
        other_manager_cancels.state_bytes().unwrap(),
        cbor::encode(&canceled_by_other_manager).unwrap()
    );
    let mut demoted = with_unused_invite;
    if let Value::Map(state) = &mut demoted
        && let Value::Array(active) = &mut state[4].1
    {
        for row in active {
            if let Value::Array(fields) = row
                && fields[0] == Value::Bytes(manager_id.to_vec())
            {
                fields[4] = Value::Integer(1);
            }
        }
    }
    if let Value::Map(state) = &mut demoted
        && let Value::Array(invitations) = &mut state[6].1
    {
        for row in invitations {
            if let Value::Array(fields) = row
                && fields[0] == Value::Bytes(extra_invitation_id.to_vec())
            {
                fields[5] = Value::Integer(3);
            }
        }
    }
    let demote = test_control(
        &role_chain,
        7,
        v4(117),
        Value::Map(vec![
            (1, Value::Bytes(manager_id.to_vec())),
            (2, Value::Integer(2)),
            (3, Value::Integer(1)),
        ]),
        demoted.clone(),
        (recipient_id, recipient_seed),
        relay_seed,
    );
    role_chain.apply_control(&demote).unwrap();
    assert_eq!(
        role_chain.state_bytes().unwrap(),
        cbor::encode(&demoted).unwrap()
    );

    let mut sole = ControlChain::from_genesis(&wire(0), relay_public).unwrap();
    let mut no_manager = cbor::decode(&sole.state_bytes().unwrap()).unwrap();
    if let Value::Map(state) = &mut no_manager
        && let Value::Array(active) = &mut state[4].1
        && let Value::Array(manager) = &mut active[0]
    {
        manager[4] = Value::Integer(1);
    }
    let invalid_demotion = test_control(
        &sole,
        7,
        v4(227),
        Value::Map(vec![
            (1, Value::Bytes(manager_id.to_vec())),
            (2, Value::Integer(2)),
            (3, Value::Integer(1)),
        ]),
        no_manager,
        (manager_id, manager_seed),
        relay_seed,
    );
    assert!(sole.apply_control(&invalid_demotion).is_err());
    assert_eq!(sole.last_global_cursor(), 1);
}

fn object_bytes(fixture: &serde_json::Value, id: [u8; 16]) -> Vec<u8> {
    let hex: String = id.iter().map(|byte| format!("{byte:02x}")).collect();
    hex_bytes(fixture["objects_by_id_hex"][hex.as_str()].as_str().unwrap())
}

#[test]
fn invite_issue_extends_pinned_genesis_only_with_manager_and_relay_signatures() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("../../tests/vectors/contiguous-chain-v1.json")).unwrap();
    let transitions = fixture["transitions"].as_array().unwrap();
    let genesis = &transitions[0];
    let issue = &transitions[1];
    let genesis_bytes = hex_bytes(genesis["committed_cbor_hex"].as_str().unwrap());
    let issue_bytes = hex_bytes(issue["committed_cbor_hex"].as_str().unwrap());
    let relay_public =
        bytes::<32>("2543b92ff1095511476adc8369db6ddc933665a11978dda1404ee1066ca9559d");
    let mut chain = ControlChain::from_genesis(&genesis_bytes, relay_public).unwrap();
    assert_eq!(chain.last_global_cursor(), 1);
    assert_eq!(chain.apply_invite_issue(&issue_bytes), Ok(()));
    assert_eq!(chain.last_global_cursor(), 2);
    assert_eq!(
        chain.head_hash(),
        bytes(issue["head_hash_hex"].as_str().unwrap())
    );
    assert_eq!(
        chain.state_bytes().unwrap(),
        hex_bytes(issue["state_cbor_hex"].as_str().unwrap())
    );
    assert!(chain.apply_invite_issue(&issue_bytes).is_err());
    assert_eq!(chain.last_global_cursor(), 2);

    let mut tampered = cbor::decode(&issue_bytes).unwrap();
    if let Value::Map(root) = &mut tampered
        && let Value::Map(unsigned) = &mut root[0].1
    {
        unsigned[7].1 = Value::Bytes([0u8; 32].to_vec());
    }
    let mut clean = ControlChain::from_genesis(&genesis_bytes, relay_public).unwrap();
    assert!(
        clean
            .apply_invite_issue(&cbor::encode(&tampered).unwrap())
            .is_err()
    );
    assert_eq!(clean.last_global_cursor(), 1);
}
