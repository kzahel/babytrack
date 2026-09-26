use babytrack_core::{
    batch::{self, Header},
    cbor::{self, Value},
    control_chain::ControlChain,
    crypto,
};

fn hex_bytes(hex: &str) -> Vec<u8> {
    hex.as_bytes()
        .chunks_exact(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
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
    assert_eq!(chain.apply_public_batch(&envelope, &receipt), Ok(()));
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
