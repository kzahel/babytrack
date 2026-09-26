use babytrack_core::{
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
fn bytes<const N: usize>(hex: &str) -> [u8; N] {
    hex_bytes(hex).try_into().unwrap()
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
