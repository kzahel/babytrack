use babytrack_core::{
    cbor::{self, Value},
    control_chain::ControlChain,
    crypto, rotation_build,
    sqlite_store::FamilyHandle,
};

fn bytes<const N: usize>(hex: &str) -> [u8; N] {
    let decoded: Vec<_> = hex
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect();
    decoded.try_into().unwrap()
}
fn blob(hex: &str) -> Vec<u8> {
    hex.as_bytes()
        .chunks_exact(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}

#[test]
fn first_removal_grants_only_remaining_manager_and_opens_history_keyring() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("../../tests/vectors/contiguous-chain-v1.json")).unwrap();
    let transitions = fixture["transitions"].as_array().unwrap();
    let wire = |index: usize| blob(transitions[index]["committed_cbor_hex"].as_str().unwrap());
    let relay_public =
        bytes::<32>("2543b92ff1095511476adc8369db6ddc933665a11978dda1404ee1066ca9559d");
    let mut chain = ControlChain::from_genesis(&wire(0), relay_public).unwrap();
    for index in 1..=6 {
        chain.apply_control(&wire(index)).unwrap();
    }
    let batch = &fixture["batch"];
    chain
        .apply_public_batch(
            &blob(batch["envelope_cbor_hex"].as_str().unwrap()),
            &blob(batch["receipt_cbor_hex"].as_str().unwrap()),
        )
        .unwrap();
    let input = &fixture["test_only_inputs"];
    let manager_id = bytes::<16>(input["manager_device_id_hex"].as_str().unwrap());
    let recipient_id = bytes::<16>(input["recipient_device_id_hex"].as_str().unwrap());
    let epoch_one = bytes::<32>(input["epoch_1_key_hex"].as_str().unwrap());
    let verified_epoch_one = chain.verify_initial_epoch_key(&epoch_one).unwrap();
    let family = FamilyHandle {
        family_id: chain.family_id(),
        device_id: manager_id,
    };
    let signing_seed = bytes(input["manager_sign_seed_hex"].as_str().unwrap());
    assert!(
        rotation_build::prepare_first_removal(&chain, family, signing_seed, epoch_one, manager_id,)
            .is_err()
    );
    assert!(
        rotation_build::prepare_first_removal(&chain, family, [0; 32], epoch_one, recipient_id,)
            .is_err()
    );
    let proposal = rotation_build::prepare_first_removal(
        &chain,
        family,
        signing_seed,
        epoch_one,
        recipient_id,
    )
    .unwrap();
    assert_eq!(
        proposal.objects.iter().map(|row| row.0).collect::<Vec<_>>(),
        vec![1, 4, 5]
    );
    assert_ne!(proposal.new_epoch_key, epoch_one);
    let Value::Map(candidate) = cbor::decode(&proposal.candidate_bytes).unwrap() else {
        unreachable!()
    };
    let signed = Value::Array(vec![candidate[0].1.clone(), candidate[1].1.clone()]);
    let receipt = Value::Array(vec![
        Value::Bytes(chain.family_id().to_vec()),
        Value::Bytes(chain.relay_id().to_vec()),
        Value::Bytes(proposal.transition_id.to_vec()),
        Value::Integer((chain.last_global_cursor() + 1).into()),
        Value::Integer(2_000_000_000_000),
        Value::Bytes(
            crypto::hash("control-signed", &cbor::encode(&signed).unwrap())
                .unwrap()
                .to_vec(),
        ),
    ]);
    let relay_seed = bytes::<32>(input["relay_sign_seed_hex"].as_str().unwrap());
    let signature = crypto::sign_cbor(
        "control-receipt",
        &cbor::encode(&receipt).unwrap(),
        &relay_seed,
    )
    .unwrap();
    let committed = cbor::encode(&Value::Map(vec![
        (1, candidate[0].1.clone()),
        (2, candidate[1].1.clone()),
        (3, receipt),
        (4, Value::Bytes(signature.to_vec())),
    ]))
    .unwrap();
    chain.apply_remove_active(&committed).unwrap();
    assert_eq!(chain.epoch().unwrap(), 2);
    assert_eq!(chain.active_devices().unwrap().len(), 1);
    assert_eq!(chain.active_devices().unwrap()[0].device_id, manager_id);
    let rotation = chain.rotation(&proposal.transition_id).unwrap();
    let grant_id = rotation.grant_ids()[0];
    let grant = proposal
        .objects
        .iter()
        .find(|row| row.1 == grant_id)
        .unwrap();
    let keyring = proposal
        .objects
        .iter()
        .find(|row| row.1 == rotation.keyring_id())
        .unwrap();
    let membership = chain.membership_check(&proposal.transition_id).unwrap();
    let member_object = proposal
        .objects
        .iter()
        .find(|row| row.1 == membership.object_id())
        .unwrap();
    let opened = chain
        .open_rotation_for(
            &proposal.transition_id,
            manager_id,
            &bytes(input["manager_agreement_seed_hex"].as_str().unwrap()),
            &[(grant_id, grant.2.clone())],
            &keyring.2,
            &member_object.2,
        )
        .unwrap();
    membership
        .verify(&member_object.2, opened.current())
        .unwrap();
    assert_eq!(opened.earlier(1), Some(&verified_epoch_one));
    assert!(
        chain
            .open_rotation_for(
                &proposal.transition_id,
                recipient_id,
                &bytes(input["recipient_agreement_seed_hex"].as_str().unwrap()),
                &[(grant_id, grant.2.clone())],
                &keyring.2,
                &member_object.2,
            )
            .is_err()
    );
}
