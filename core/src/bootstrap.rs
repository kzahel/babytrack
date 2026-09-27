//! Strict v1 invitation fragment parsing and committed-issue binding.
//! Parsing previews a link without contacting the relay or consuming it.

use crate::{
    cbor::{self, Value},
    control,
    control_chain::{self, ControlChain},
    crypto, ids,
};

const PREFIX: &str = "#bt-invite=v1.";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    Cbor(cbor::Error),
    Control(control::Error),
    Chain(control_chain::Error),
    Crypto(crypto::Error),
    Invalid(&'static str),
}
impl From<cbor::Error> for Error {
    fn from(value: cbor::Error) -> Self {
        Self::Cbor(value)
    }
}
impl From<control::Error> for Error {
    fn from(value: control::Error) -> Self {
        Self::Control(value)
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvitationBootstrap {
    relay_origin: String,
    relay_public_key: [u8; 32],
    family_id: [u8; 16],
    genesis_head: [u8; 32],
    invitation_id: [u8; 16],
    fixed_role: u8,
    invitation_sign_seed: [u8; 32],
    issue_signed_hash: [u8; 32],
}

impl InvitationBootstrap {
    pub fn from_committed_issue(
        relay_origin: &str,
        relay_public_key: [u8; 32],
        genesis_bytes: &[u8],
        issue_bytes: &[u8],
        invitation_sign_seed: [u8; 32],
    ) -> Result<Self, Error> {
        Self::from_committed_issue_with_batches(
            relay_origin,
            relay_public_key,
            genesis_bytes,
            issue_bytes,
            invitation_sign_seed,
            &[],
        )
    }

    pub fn from_committed_issue_with_batches(
        relay_origin: &str,
        relay_public_key: [u8; 32],
        genesis_bytes: &[u8],
        issue_bytes: &[u8],
        invitation_sign_seed: [u8; 32],
        prior_batches: &[(&[u8], &[u8])],
    ) -> Result<Self, Error> {
        validate_origin(relay_origin)?;
        let genesis = control::verify_genesis(genesis_bytes, &relay_public_key)?;
        let issue = cbor::decode_with_limits(
            issue_bytes,
            cbor::Limits {
                max_bytes: 1024 * 1024,
                max_depth: 16,
            },
        )?;
        let Value::Map(root) = issue else {
            return Err(Error::Invalid("issue control not map"));
        };
        if root.len() != 4 {
            return Err(Error::Invalid("issue control width invalid"));
        }
        let Value::Map(unsigned) = &root[0].1 else {
            return Err(Error::Invalid("issue unsigned not map"));
        };
        if unsigned.len() != 11 {
            return Err(Error::Invalid("issue unsigned width invalid"));
        }
        let Value::Map(delta) = &unsigned[6].1 else {
            return Err(Error::Invalid("issue delta not map"));
        };
        if delta.len() != 4 {
            return Err(Error::Invalid("issue delta width invalid"));
        }
        let signed = Value::Array(vec![root[0].1.clone(), root[1].1.clone()]);
        let descriptor = Self {
            relay_origin: relay_origin.to_owned(),
            relay_public_key,
            family_id: genesis.family_id(),
            genesis_head: genesis.head_hash(),
            invitation_id: fixed(&delta[0].1)?,
            fixed_role: number(&delta[3].1)?
                .try_into()
                .map_err(|_| Error::Invalid("invitation role outside u8"))?,
            invitation_sign_seed,
            issue_signed_hash: crypto::hash("control-signed", &cbor::encode(&signed)?)?,
        };
        descriptor.verify_issue_with_batches(genesis_bytes, issue_bytes, prior_batches)?;
        Ok(descriptor)
    }

    pub fn from_fragment(fragment: &str) -> Result<Self, Error> {
        let payload = fragment
            .strip_prefix(PREFIX)
            .ok_or(Error::Invalid("invitation fragment prefix invalid"))?;
        let bytes = decode_base64url(payload)?;
        if bytes.len() > 512 {
            return Err(Error::Invalid("invitation descriptor too large"));
        }
        let value = cbor::decode_with_limits(
            &bytes,
            cbor::Limits {
                max_bytes: 512,
                max_depth: 3,
            },
        )?;
        let Value::Array(parts) = value else {
            return Err(Error::Invalid("invitation descriptor not array"));
        };
        if parts.len() != 9 || parts[0] != Value::Integer(1) {
            return Err(Error::Invalid(
                "invitation descriptor version or width invalid",
            ));
        }
        let Value::Text(origin) = &parts[1] else {
            return Err(Error::Invalid("relay origin not text"));
        };
        validate_origin(origin)?;
        let fixed_role: u8 = number(&parts[6])?
            .try_into()
            .map_err(|_| Error::Invalid("invitation role outside u8"))?;
        if fixed_role != 1 && fixed_role != 2 {
            return Err(Error::Invalid("invitation role invalid"));
        }
        let family_id = fixed(&parts[3])?;
        let invitation_id = fixed(&parts[5])?;
        if !ids::is_v4(&family_id) || !ids::is_v4(&invitation_id) {
            return Err(Error::Invalid("Family or invitation ID is not UUIDv4"));
        }
        Ok(Self {
            relay_origin: origin.clone(),
            relay_public_key: fixed(&parts[2])?,
            family_id,
            genesis_head: fixed(&parts[4])?,
            invitation_id,
            fixed_role,
            invitation_sign_seed: fixed(&parts[7])?,
            issue_signed_hash: fixed(&parts[8])?,
        })
    }

    pub fn to_fragment(&self) -> Result<String, Error> {
        validate_origin(&self.relay_origin)?;
        let value = Value::Array(vec![
            Value::Integer(1),
            Value::Text(self.relay_origin.clone()),
            Value::Bytes(self.relay_public_key.to_vec()),
            Value::Bytes(self.family_id.to_vec()),
            Value::Bytes(self.genesis_head.to_vec()),
            Value::Bytes(self.invitation_id.to_vec()),
            Value::Integer(self.fixed_role.into()),
            Value::Bytes(self.invitation_sign_seed.to_vec()),
            Value::Bytes(self.issue_signed_hash.to_vec()),
        ]);
        let bytes = cbor::encode(&value)?;
        if bytes.len() > 512 {
            return Err(Error::Invalid("invitation descriptor too large"));
        }
        Ok(format!("{PREFIX}{}", encode_base64url(&bytes)))
    }

    pub fn relay_origin(&self) -> &str {
        &self.relay_origin
    }
    pub fn relay_public_key(&self) -> [u8; 32] {
        self.relay_public_key
    }
    #[cfg(not(target_arch = "wasm32"))]
    pub fn sign_get(
        &self,
        exact_path: &str,
    ) -> Result<crate::sync_wire::SignedRead, crate::sync_wire::Error> {
        use sha2::{Digest, Sha256};
        let relay_id: [u8; 32] = Sha256::digest(self.relay_public_key).into();
        crate::sync_wire::sign_get(
            self.family_id,
            relay_id,
            self.invitation_id,
            &self.invitation_sign_seed,
            exact_path,
        )
    }
    pub fn family_id(&self) -> [u8; 16] {
        self.family_id
    }
    pub fn invitation_id(&self) -> [u8; 16] {
        self.invitation_id
    }
    pub fn fixed_role(&self) -> u8 {
        self.fixed_role
    }
    pub(crate) fn invitation_sign_seed(&self) -> [u8; 32] {
        self.invitation_sign_seed
    }
    pub(crate) fn relay_public_key_internal(&self) -> [u8; 32] {
        self.relay_public_key
    }

    /// Verify the exact signed genesis and committed issue before a joiner
    /// generates credentials or sends a claim. Neither byte string may be a
    /// relay-supplied snapshot substituted for the linked ancestry.
    pub fn verify_issue(
        &self,
        genesis_bytes: &[u8],
        issue_bytes: &[u8],
    ) -> Result<ControlChain, Error> {
        self.verify_issue_with_batches(genesis_bytes, issue_bytes, &[])
    }

    pub fn verify_issue_with_batches(
        &self,
        genesis_bytes: &[u8],
        issue_bytes: &[u8],
        prior_batches: &[(&[u8], &[u8])],
    ) -> Result<ControlChain, Error> {
        self.verify_issue_inner(genesis_bytes, issue_bytes, prior_batches, false)
    }

    pub(crate) fn verify_issue_sparse(
        &self,
        genesis_bytes: &[u8],
        issue_bytes: &[u8],
    ) -> Result<ControlChain, Error> {
        self.verify_issue_inner(genesis_bytes, issue_bytes, &[], true)
    }

    fn verify_issue_inner(
        &self,
        genesis_bytes: &[u8],
        issue_bytes: &[u8],
        prior_batches: &[(&[u8], &[u8])],
        sparse: bool,
    ) -> Result<ControlChain, Error> {
        let genesis = control::verify_genesis(genesis_bytes, &self.relay_public_key)?;
        if genesis.family_id() != self.family_id || genesis.head_hash() != self.genesis_head {
            return Err(Error::Invalid("linked genesis differs from verified bytes"));
        }
        let mut chain = ControlChain::from_genesis(genesis_bytes, self.relay_public_key)?;
        for (envelope, receipt) in prior_batches {
            chain.apply_public_batch(envelope, receipt)?;
        }
        let issue = cbor::decode_with_limits(
            issue_bytes,
            cbor::Limits {
                max_bytes: 1024 * 1024,
                max_depth: 16,
            },
        )?;
        let Value::Map(root) = issue else {
            return Err(Error::Invalid("issue control not map"));
        };
        if root.len() != 4 {
            return Err(Error::Invalid("issue control width invalid"));
        }
        let signed = Value::Array(vec![root[0].1.clone(), root[1].1.clone()]);
        if crypto::hash("control-signed", &cbor::encode(&signed)?)? != self.issue_signed_hash {
            return Err(Error::Invalid("issue signed hash differs from link"));
        }
        let Value::Map(unsigned) = &root[0].1 else {
            return Err(Error::Invalid("issue unsigned not map"));
        };
        if unsigned.len() != 11 {
            return Err(Error::Invalid("issue unsigned width invalid"));
        }
        let Value::Map(delta) = &unsigned[6].1 else {
            return Err(Error::Invalid("issue delta not map"));
        };
        if delta.len() != 4
            || fixed::<16>(&delta[0].1)? != self.invitation_id
            || fixed::<32>(&delta[2].1)? != crypto::signing_public_key(&self.invitation_sign_seed)
            || number(&delta[3].1)? != u64::from(self.fixed_role)
        {
            return Err(Error::Invalid("issue invitation ID, role, or key mismatch"));
        }
        if sparse {
            chain.apply_sparse_control(issue_bytes)?;
        } else {
            chain.apply_invite_issue(issue_bytes)?;
        }
        Ok(chain)
    }
}

fn validate_origin(origin: &str) -> Result<(), Error> {
    let authority = if let Some(authority) = origin.strip_prefix("https://") {
        authority
    } else if let Some(authority) = origin.strip_prefix("http://localhost:") {
        if authority.is_empty()
            || authority.starts_with('0')
            || !authority.bytes().all(|byte| byte.is_ascii_digit())
            || authority
                .parse::<u16>()
                .ok()
                .filter(|port| *port > 0)
                .is_none()
        {
            return Err(Error::Invalid("local relay port invalid"));
        }
        return Ok(());
    } else {
        return Err(Error::Invalid("relay origin must use HTTPS"));
    };
    if authority.is_empty()
        || authority.contains(['/', '?', '#', '@', '%', '[', ']'])
        || authority.bytes().any(|byte| byte.is_ascii_uppercase())
    {
        return Err(Error::Invalid("relay origin authority invalid"));
    }
    let (host, port) = match authority.rsplit_once(':') {
        Some((host, port)) => (host, Some(port)),
        None => (authority, None),
    };
    if host.is_empty()
        || host.starts_with('.')
        || host.ends_with('.')
        || host.split('.').any(|label| {
            label.is_empty()
                || label.starts_with('-')
                || label.ends_with('-')
                || !label
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        })
    {
        return Err(Error::Invalid("relay host invalid"));
    }
    if let Some(port) = port
        && (port.is_empty()
            || port.starts_with('0')
            || port == "443"
            || !port.bytes().all(|byte| byte.is_ascii_digit())
            || port
                .parse::<u16>()
                .ok()
                .filter(|value| *value > 0)
                .is_none())
    {
        return Err(Error::Invalid("relay HTTPS port invalid"));
    }
    Ok(())
}

fn fixed<const N: usize>(value: &Value) -> Result<[u8; N], Error> {
    let Value::Bytes(bytes) = value else {
        return Err(Error::Invalid("descriptor field not bytes"));
    };
    bytes
        .as_slice()
        .try_into()
        .map_err(|_| Error::Invalid("descriptor byte length invalid"))
}

fn number(value: &Value) -> Result<u64, Error> {
    let Value::Integer(value) = value else {
        return Err(Error::Invalid("descriptor field not integer"));
    };
    (*value)
        .try_into()
        .map_err(|_| Error::Invalid("descriptor integer negative"))
}

fn decode_base64url(text: &str) -> Result<Vec<u8>, Error> {
    if text.is_empty() || text.len() % 4 == 1 {
        return Err(Error::Invalid("base64url descriptor length invalid"));
    }
    let mut result = Vec::with_capacity(text.len() * 3 / 4);
    let mut bits = 0u32;
    let mut count = 0u8;
    for byte in text.bytes() {
        let digit = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'-' => 62,
            b'_' => 63,
            _ => return Err(Error::Invalid("base64url descriptor character invalid")),
        };
        bits = (bits << 6) | u32::from(digit);
        count += 6;
        if count >= 8 {
            count -= 8;
            result.push((bits >> count) as u8);
            bits &= (1u32 << count) - 1;
        }
        if result.len() > 512 {
            return Err(Error::Invalid("invitation descriptor too large"));
        }
    }
    if bits != 0 || encode_base64url(&result) != text {
        return Err(Error::Invalid("base64url descriptor not canonical"));
    }
    Ok(result)
}

fn encode_base64url(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut result = String::with_capacity(bytes.len().div_ceil(3) * 4);
    let mut index = 0;
    while index < bytes.len() {
        let first = bytes[index];
        let second = bytes.get(index + 1).copied();
        let third = bytes.get(index + 2).copied();
        result.push(ALPHABET[(first >> 2) as usize] as char);
        result.push(ALPHABET[(((first & 3) << 4) | (second.unwrap_or(0) >> 4)) as usize] as char);
        if let Some(second) = second {
            result.push(
                ALPHABET[(((second & 15) << 2) | (third.unwrap_or(0) >> 6)) as usize] as char,
            );
        }
        if let Some(third) = third {
            result.push(ALPHABET[(third & 63) as usize] as char);
        }
        index += 3;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex_bytes(hex: &str) -> Vec<u8> {
        hex.as_bytes()
            .chunks_exact(2)
            .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
            .collect()
    }

    #[test]
    fn fixed_invitation_fragment_binds_genesis_issue_role_and_key() {
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../../tests/vectors/contiguous-chain-v1.json"))
                .unwrap();
        let bootstrap = &fixture["bootstrap"];
        let fragment = bootstrap["fragment"].as_str().unwrap();
        let descriptor = InvitationBootstrap::from_fragment(fragment).unwrap();
        assert_eq!(descriptor.to_fragment().unwrap(), fragment);
        assert_eq!(descriptor.relay_origin(), "https://relay.example");
        assert_eq!(descriptor.fixed_role(), 1);
        let transitions = fixture["transitions"].as_array().unwrap();
        let genesis = hex_bytes(transitions[0]["committed_cbor_hex"].as_str().unwrap());
        let issue = hex_bytes(transitions[1]["committed_cbor_hex"].as_str().unwrap());
        let chain = descriptor.verify_issue(&genesis, &issue).unwrap();
        assert_eq!(chain.last_global_cursor(), 2);
        assert_eq!(chain.family_id(), descriptor.family_id());
        let built = InvitationBootstrap::from_committed_issue(
            descriptor.relay_origin(),
            descriptor.relay_public_key,
            &genesis,
            &issue,
            descriptor.invitation_sign_seed,
        )
        .unwrap();
        assert_eq!(built.to_fragment().unwrap(), fragment);

        let bytes = hex_bytes(bootstrap["descriptor_cbor_hex"].as_str().unwrap());
        let mutations: &[(usize, Value)] = &[
            (1, Value::Text("https://relay.example/".to_owned())),
            (1, Value::Text("http://relay.example".to_owned())),
            (1, Value::Text("https://RELAY.example".to_owned())),
            (1, Value::Text("https://relay.example:443".to_owned())),
            (6, Value::Integer(3)),
        ];
        for (index, replacement) in mutations {
            let mut value = cbor::decode(&bytes).unwrap();
            let Value::Array(parts) = &mut value else {
                unreachable!()
            };
            parts[*index] = replacement.clone();
            let encoded = cbor::encode(&value).unwrap();
            assert!(
                InvitationBootstrap::from_fragment(&format!(
                    "{PREFIX}{}",
                    encode_base64url(&encoded)
                ))
                .is_err()
            );
        }
        for index in [2, 3, 4, 5, 7, 8] {
            let mut value = cbor::decode(&bytes).unwrap();
            let Value::Array(parts) = &mut value else {
                unreachable!()
            };
            let Value::Bytes(field) = &mut parts[index] else {
                unreachable!()
            };
            *field.last_mut().unwrap() ^= 1;
            let encoded = cbor::encode(&value).unwrap();
            let changed = InvitationBootstrap::from_fragment(&format!(
                "{PREFIX}{}",
                encode_base64url(&encoded)
            ))
            .unwrap();
            assert!(changed.verify_issue(&genesis, &issue).is_err());
        }
        let mut altered_issue = issue.clone();
        *altered_issue.last_mut().unwrap() ^= 1;
        assert!(descriptor.verify_issue(&genesis, &altered_issue).is_err());
    }

    #[test]
    fn fragment_rejects_padding_extra_keys_and_noncanonical_bits() {
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../../tests/vectors/contiguous-chain-v1.json"))
                .unwrap();
        let fragment = fixture["bootstrap"]["fragment"].as_str().unwrap();
        for changed in [
            format!("{fragment}="),
            format!("{fragment}&extra=1"),
            fragment.replace("#bt-invite=", "#invite="),
            fragment.trim_start_matches('#').to_owned(),
        ] {
            assert!(InvitationBootstrap::from_fragment(&changed).is_err());
        }
        assert!(decode_base64url("AB").is_err());
        assert!(decode_base64url("AA==").is_err());
        assert_eq!(decode_base64url("AA").unwrap(), vec![0]);
        assert!(
            InvitationBootstrap::from_fragment(&format!(
                "{PREFIX}{}",
                encode_base64url(&vec![0; 513])
            ))
            .is_err()
        );
    }
}
