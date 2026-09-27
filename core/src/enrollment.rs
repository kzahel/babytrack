//! Durable keyless recipient enrollment. Candidate and Family-scoped secrets
//! commit locally before the first claim POST; retries use exact bytes.

use crate::{
    bootstrap::{self, InvitationBootstrap},
    cbor::{self, Value},
    control_chain, crypto, hpke,
    shared_history::{self, PublicHistorySession},
    sqlite_store::{self, EnrollmentRow, FamilyHandle, SqliteStore},
};

#[derive(Debug)]
pub enum Error {
    Bootstrap(bootstrap::Error),
    Chain(control_chain::Error),
    Cbor(cbor::Error),
    Crypto(crypto::Error),
    Hpke(hpke::Error),
    Handoff(crate::handoff::Error),
    History(shared_history::Error),
    Store(sqlite_store::Error),
    Random(getrandom::Error),
    Wire(crate::sync_wire::Error),
    Invalid(&'static str),
}
impl From<bootstrap::Error> for Error {
    fn from(value: bootstrap::Error) -> Self {
        Self::Bootstrap(value)
    }
}
impl From<control_chain::Error> for Error {
    fn from(value: control_chain::Error) -> Self {
        Self::Chain(value)
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
impl From<hpke::Error> for Error {
    fn from(value: hpke::Error) -> Self {
        Self::Hpke(value)
    }
}
impl From<crate::handoff::Error> for Error {
    fn from(value: crate::handoff::Error) -> Self {
        Self::Handoff(value)
    }
}
impl From<crate::sync_wire::Error> for Error {
    fn from(value: crate::sync_wire::Error) -> Self {
        Self::Wire(value)
    }
}
impl From<shared_history::Error> for Error {
    fn from(value: shared_history::Error) -> Self {
        Self::History(value)
    }
}
impl From<sqlite_store::Error> for Error {
    fn from(value: sqlite_store::Error) -> Self {
        Self::Store(value)
    }
}

pub struct EnrollmentAttempt {
    family: FamilyHandle,
    invitation_id: [u8; 16],
    bootstrap_fragment: String,
    candidate_bytes: Vec<u8>,
    device_sign_seed: [u8; 32],
    device_agreement_private: [u8; 32],
}

impl EnrollmentAttempt {
    pub fn prepare(
        store: &mut SqliteStore,
        bootstrap: &InvitationBootstrap,
        genesis_bytes: &[u8],
        issue_bytes: &[u8],
        local_wrapping_key: &[u8; 32],
    ) -> Result<Self, Error> {
        Self::prepare_with_batches(
            store,
            bootstrap,
            genesis_bytes,
            issue_bytes,
            &[],
            local_wrapping_key,
        )
    }

    pub fn prepare_with_batches(
        store: &mut SqliteStore,
        bootstrap: &InvitationBootstrap,
        genesis_bytes: &[u8],
        issue_bytes: &[u8],
        prior_batches: &[(&[u8], &[u8])],
        local_wrapping_key: &[u8; 32],
    ) -> Result<Self, Error> {
        Self::prepare_mode(
            store,
            bootstrap,
            genesis_bytes,
            issue_bytes,
            prior_batches,
            local_wrapping_key,
            false,
        )
    }

    /// Prepare from an invitation-visible control page with data gaps.
    /// The first full-log replay happens only after admission.
    pub fn prepare_sparse(
        store: &mut SqliteStore,
        bootstrap: &InvitationBootstrap,
        genesis_bytes: &[u8],
        issue_bytes: &[u8],
        local_wrapping_key: &[u8; 32],
    ) -> Result<Self, Error> {
        Self::prepare_mode(
            store,
            bootstrap,
            genesis_bytes,
            issue_bytes,
            &[],
            local_wrapping_key,
            true,
        )
    }

    fn prepare_mode(
        store: &mut SqliteStore,
        bootstrap: &InvitationBootstrap,
        genesis_bytes: &[u8],
        issue_bytes: &[u8],
        prior_batches: &[(&[u8], &[u8])],
        local_wrapping_key: &[u8; 32],
        sparse: bool,
    ) -> Result<Self, Error> {
        if store.enrollment_attempt(bootstrap.family_id())?.is_some() {
            let existing = Self::resume(store, bootstrap.family_id(), local_wrapping_key)?;
            if existing.bootstrap_fragment != bootstrap.to_fragment()? {
                return Err(Error::Invalid(
                    "another invitation already owns this Family",
                ));
            }
            return Ok(existing);
        }
        let chain = if sparse {
            bootstrap.verify_issue_sparse(genesis_bytes, issue_bytes)?
        } else {
            bootstrap.verify_issue_with_batches(genesis_bytes, issue_bytes, prior_batches)?
        };
        let family = FamilyHandle {
            family_id: bootstrap.family_id(),
            device_id: random_v4()?,
        };
        let device_sign_seed = random::<32>()?;
        let device_agreement_private = random::<32>()?;
        let enrollment_nonce = random::<32>()?;
        let transition_id = random_v4()?;
        let candidate_bytes = build_claim(
            bootstrap,
            &chain,
            family,
            &device_sign_seed,
            &device_agreement_private,
            &enrollment_nonce,
            transition_id,
        )?;
        let secret = cbor::encode(&Value::Array(vec![
            Value::Integer(1),
            Value::Text(bootstrap.to_fragment()?),
            Value::Bytes(device_sign_seed.to_vec()),
            Value::Bytes(device_agreement_private.to_vec()),
            Value::Bytes(enrollment_nonce.to_vec()),
        ]))?;
        let secret_nonce = random::<24>()?;
        let aad = local_aad(family, bootstrap.invitation_id())?;
        let secret_ciphertext =
            crypto::seal_with_nonce(local_wrapping_key, &secret_nonce, &aad, &secret)?;
        store.create_enrollment_attempt(
            &EnrollmentRow {
                family,
                invitation_id: bootstrap.invitation_id(),
                genesis_bytes: genesis_bytes.to_vec(),
                issue_bytes: issue_bytes.to_vec(),
                candidate_bytes: candidate_bytes.clone(),
                secret_nonce,
                secret_ciphertext,
            },
            sparse.then_some(chain.last_global_cursor()),
        )?;
        let mut public = PublicHistorySession::begin(
            store,
            family,
            genesis_bytes,
            bootstrap.relay_public_key_internal(),
        )?;
        if !sparse {
            for (envelope, receipt) in prior_batches {
                public.accept_batch(store, envelope, receipt)?;
            }
            public.accept_control(store, issue_bytes)?;
        }
        Self::resume(store, family.family_id, local_wrapping_key)
    }

    pub fn resume(
        store: &mut SqliteStore,
        family_id: [u8; 16],
        local_wrapping_key: &[u8; 32],
    ) -> Result<Self, Error> {
        let row = store
            .enrollment_attempt(family_id)?
            .ok_or(Error::Invalid("no durable enrollment attempt"))?;
        let aad = local_aad(row.family, row.invitation_id)?;
        let secret = crypto::open(
            local_wrapping_key,
            &row.secret_nonce,
            &aad,
            &row.secret_ciphertext,
        )?;
        let value = cbor::decode_with_limits(
            &secret,
            cbor::Limits {
                max_bytes: 1024,
                max_depth: 3,
            },
        )?;
        let Value::Array(parts) = value else {
            return Err(Error::Invalid("enrollment secret not array"));
        };
        if parts.len() != 5 || parts[0] != Value::Integer(1) {
            return Err(Error::Invalid("enrollment secret version invalid"));
        }
        let Value::Text(fragment) = &parts[1] else {
            return Err(Error::Invalid("enrollment fragment not text"));
        };
        let bootstrap = InvitationBootstrap::from_fragment(fragment)?;
        if bootstrap.family_id() != row.family.family_id
            || bootstrap.invitation_id() != row.invitation_id
        {
            return Err(Error::Invalid("stored enrollment context mismatch"));
        }
        let history = store
            .shared_history(row.family)?
            .ok_or(Error::Invalid("shared history absent after enrollment"))?;
        let sparse_controls = store.enrollment_controls(row.family)?;
        let sparse = !sparse_controls.is_empty();
        let prior_batches: Vec<(&[u8], &[u8])> = history
            .entries
            .iter()
            .take_while(|entry| entry.kind == 2)
            .map(|entry| {
                (
                    entry.committed_bytes.as_slice(),
                    entry.receipt_bytes.as_slice(),
                )
            })
            .collect();
        let chain = if sparse {
            shared_history::sparse_enrollment_chain(
                store,
                row.family,
                bootstrap.relay_public_key_internal(),
            )?;
            bootstrap.verify_issue_sparse(&row.genesis_bytes, &row.issue_bytes)?
        } else {
            bootstrap.verify_issue_with_batches(
                &row.genesis_bytes,
                &row.issue_bytes,
                &prior_batches,
            )?
        };
        let device_sign_seed = fixed(&parts[2])?;
        let device_agreement_private = fixed(&parts[3])?;
        let enrollment_nonce = fixed(&parts[4])?;
        let transition_id = candidate_transition_id(&row.candidate_bytes)?;
        let rebuilt = build_claim(
            &bootstrap,
            &chain,
            row.family,
            &device_sign_seed,
            &device_agreement_private,
            &enrollment_nonce,
            transition_id,
        )?;
        if rebuilt != row.candidate_bytes {
            return Err(Error::Invalid("stored claim differs from durable keys"));
        }
        let mut public = PublicHistorySession::begin(
            store,
            row.family,
            &row.genesis_bytes,
            bootstrap.relay_public_key_internal(),
        )?;
        if !sparse && public.cursor() < chain.last_global_cursor() {
            public.accept_control(store, &row.issue_bytes)?;
        }
        if !sparse
            && history
                .entries
                .iter()
                .find(|entry| entry.kind == 1)
                .is_none_or(|first| first.committed_bytes != row.issue_bytes)
        {
            return Err(Error::Invalid("stored issue differs from pinned history"));
        }
        let committed_controls: Vec<&[u8]> = if sparse {
            sparse_controls
                .iter()
                .map(|(_, bytes)| bytes.as_slice())
                .collect()
        } else {
            history
                .entries
                .iter()
                .filter(|entry| entry.kind == 1)
                .map(|entry| entry.committed_bytes.as_slice())
                .collect()
        };
        for bytes in committed_controls {
            let committed = cbor::decode_with_limits(
                bytes,
                cbor::Limits {
                    max_bytes: 1024 * 1024,
                    max_depth: 16,
                },
            )?;
            let Value::Map(root) = committed else {
                return Err(Error::Invalid("committed control not map"));
            };
            let Value::Map(unsigned) = &root[0].1 else {
                return Err(Error::Invalid("committed unsigned control not map"));
            };
            if unsigned[5].1 != Value::Integer(4) {
                continue;
            }
            let Value::Map(delta) = &unsigned[6].1 else {
                return Err(Error::Invalid("committed claim delta not map"));
            };
            if delta[0].1 != Value::Bytes(row.invitation_id.to_vec()) {
                continue;
            }
            let actual = cbor::encode(&Value::Map(vec![
                (1, root[0].1.clone()),
                (2, root[1].1.clone()),
            ]))?;
            if actual != row.candidate_bytes {
                return Err(Error::Invalid(
                    "invitation was claimed by a different candidate",
                ));
            }
        }
        Ok(Self {
            family: row.family,
            invitation_id: row.invitation_id,
            bootstrap_fragment: fragment.clone(),
            candidate_bytes: row.candidate_bytes,
            device_sign_seed,
            device_agreement_private,
        })
    }

    pub fn family(&self) -> FamilyHandle {
        self.family
    }
    pub fn invitation_id(&self) -> [u8; 16] {
        self.invitation_id
    }
    pub fn claim_candidate(&self) -> &[u8] {
        &self.candidate_bytes
    }
    pub fn accept_sparse_control(
        &self,
        store: &mut SqliteStore,
        committed: &[u8],
    ) -> Result<(), Error> {
        shared_history::accept_sparse_enrollment_control(
            store,
            self.family,
            self.relay_public_key()?,
            committed,
        )?;
        Ok(())
    }
    pub fn accept_sparse_object(
        &self,
        store: &mut SqliteStore,
        object_id: [u8; 16],
        object_bytes: &[u8],
    ) -> Result<(), Error> {
        shared_history::accept_sparse_enrollment_object(
            store,
            self.family,
            self.relay_public_key()?,
            object_id,
            object_bytes,
        )?;
        Ok(())
    }
    pub fn device_sign_public(&self) -> [u8; 32] {
        crypto::signing_public_key(&self.device_sign_seed)
    }
    pub fn device_agreement_public(&self) -> Result<[u8; 32], Error> {
        Ok(hpke::public_key_from_private(
            &self.device_agreement_private,
        )?)
    }
    pub fn sign_get(&self, exact_path: &str) -> Result<crate::sync_wire::SignedRead, Error> {
        use sha2::{Digest, Sha256};
        let bootstrap = InvitationBootstrap::from_fragment(&self.bootstrap_fragment)?;
        let relay_id: [u8; 32] = Sha256::digest(bootstrap.relay_public_key()).into();
        Ok(crate::sync_wire::sign_get(
            self.family.family_id,
            relay_id,
            self.family.device_id,
            &self.device_sign_seed,
            exact_path,
        )?)
    }
    pub fn prove_challenge(
        &self,
        challenge: &crate::handoff::VerifiedChallenge,
        hpke_object: &[u8],
    ) -> Result<crate::handoff::PendingProof, Error> {
        Ok(challenge.prepare_proof(
            hpke_object,
            &self.device_agreement_private,
            &self.device_sign_seed,
        )?)
    }
    pub(crate) fn agreement_private(&self) -> [u8; 32] {
        self.device_agreement_private
    }
    pub(crate) fn signing_seed(&self) -> [u8; 32] {
        self.device_sign_seed
    }
    pub(crate) fn relay_public_key(&self) -> Result<[u8; 32], Error> {
        Ok(InvitationBootstrap::from_fragment(&self.bootstrap_fragment)?.relay_public_key())
    }
}

fn build_claim(
    bootstrap: &InvitationBootstrap,
    chain: &crate::control_chain::ControlChain,
    family: FamilyHandle,
    device_sign_seed: &[u8; 32],
    device_agreement_private: &[u8; 32],
    enrollment_nonce: &[u8; 32],
    transition_id: [u8; 16],
) -> Result<Vec<u8>, Error> {
    let sign_public = crypto::signing_public_key(device_sign_seed);
    let agree_public = hpke::public_key_from_private(device_agreement_private)?;
    let claim_input = Value::Array(vec![
        Value::Bytes(family.family_id.to_vec()),
        Value::Bytes(chain.relay_id().to_vec()),
        Value::Bytes(bootstrap.invitation_id().to_vec()),
        Value::Integer(bootstrap.fixed_role().into()),
        Value::Bytes(family.device_id.to_vec()),
        Value::Bytes(sign_public.to_vec()),
        Value::Bytes(agree_public.to_vec()),
        Value::Integer(1),
        Value::Bytes(enrollment_nonce.to_vec()),
        Value::Bytes(chain.head_hash().to_vec()),
    ]);
    let claim_hash = crypto::hash("claim", &cbor::encode(&claim_input)?)?;
    let mut state = cbor::decode(&chain.state_bytes()?)?;
    let Value::Map(state_fields) = &mut state else {
        return Err(Error::Invalid("verified authority state not map"));
    };
    let Value::Array(invitations) = &mut state_fields[6].1 else {
        return Err(Error::Invalid("verified invitations not array"));
    };
    let row = invitations
        .iter_mut()
        .find(|row| {
            matches!(row, Value::Array(fields) if fields[0] == Value::Bytes(bootstrap.invitation_id().to_vec()))
        })
        .ok_or(Error::Invalid("linked invitation missing"))?;
    let Value::Array(invitation) = row else {
        unreachable!()
    };
    if invitation[5] != Value::Integer(1) {
        return Err(Error::Invalid("invitation is already consumed"));
    }
    invitation[5] = Value::Integer(2);
    let Value::Array(pending) = &mut state_fields[5].1 else {
        return Err(Error::Invalid("verified pending state not array"));
    };
    pending.push(Value::Array(vec![
        Value::Bytes(bootstrap.invitation_id().to_vec()),
        Value::Bytes(family.device_id.to_vec()),
        Value::Bytes(sign_public.to_vec()),
        Value::Bytes(agree_public.to_vec()),
        Value::Integer(1),
        Value::Integer(bootstrap.fixed_role().into()),
        Value::Bytes(claim_hash.to_vec()),
        Value::Null,
        Value::Null,
    ]));
    pending.sort_by(|left, right| {
        let (Value::Array(left), Value::Array(right)) = (left, right) else {
            unreachable!()
        };
        let (Value::Bytes(left), Value::Bytes(right)) = (&left[0], &right[0]) else {
            unreachable!()
        };
        left.cmp(right)
    });
    let delta = Value::Map(vec![
        (1, Value::Bytes(bootstrap.invitation_id().to_vec())),
        (2, Value::Bytes(family.device_id.to_vec())),
        (3, Value::Bytes(sign_public.to_vec())),
        (4, Value::Bytes(agree_public.to_vec())),
        (5, Value::Integer(1)),
        (6, Value::Bytes(enrollment_nonce.to_vec())),
        (7, Value::Bytes(claim_hash.to_vec())),
    ]);
    let mut parts = vec![
        Value::Integer(1),
        Value::Bytes(family.family_id.to_vec()),
        Value::Bytes(chain.relay_id().to_vec()),
        Value::Bytes(chain.head_hash().to_vec()),
        Value::Bytes(transition_id.to_vec()),
        Value::Integer(4),
        delta,
        Value::Bytes(crypto::hash("auth-state", &cbor::encode(&state)?)?.to_vec()),
        Value::Integer(chain.epoch()?.into()),
    ];
    let core_hash = crypto::hash(
        "transition-core",
        &cbor::encode(&Value::Array(parts.clone()))?,
    )?;
    parts.push(Value::Array(vec![]));
    parts.push(Value::Bytes(core_hash.to_vec()));
    let unsigned = Value::Map(
        parts
            .into_iter()
            .enumerate()
            .map(|(index, value)| (index as u64 + 1, value))
            .collect(),
    );
    let unsigned_bytes = cbor::encode(&unsigned)?;
    let mut signatures = vec![
        (bootstrap.invitation_id(), bootstrap.invitation_sign_seed()),
        (family.device_id, *device_sign_seed),
    ];
    signatures.sort_by_key(|entry| entry.0);
    let signatures = Value::Array(
        signatures
            .into_iter()
            .map(|(id, seed)| {
                Ok(Value::Array(vec![
                    Value::Bytes(id.to_vec()),
                    Value::Bytes(
                        crypto::sign_cbor("control-transition", &unsigned_bytes, &seed)?.to_vec(),
                    ),
                ]))
            })
            .collect::<Result<Vec<_>, Error>>()?,
    );
    Ok(cbor::encode(&Value::Map(vec![
        (1, unsigned),
        (2, signatures),
    ]))?)
}

fn local_aad(family: FamilyHandle, invitation_id: [u8; 16]) -> Result<[u8; 32], Error> {
    let bytes = cbor::encode(&Value::Array(vec![
        Value::Bytes(family.family_id.to_vec()),
        Value::Bytes(family.device_id.to_vec()),
        Value::Bytes(invitation_id.to_vec()),
    ]))?;
    Ok(crypto::hash("enrollment-local-aad", &bytes)?)
}

fn candidate_transition_id(bytes: &[u8]) -> Result<[u8; 16], Error> {
    let value = cbor::decode(bytes)?;
    let Value::Map(root) = value else {
        return Err(Error::Invalid("claim candidate not map"));
    };
    let Value::Map(unsigned) = &root[0].1 else {
        return Err(Error::Invalid("claim unsigned not map"));
    };
    fixed(&unsigned[4].1)
}

fn fixed<const N: usize>(value: &Value) -> Result<[u8; N], Error> {
    let Value::Bytes(bytes) = value else {
        return Err(Error::Invalid("enrollment field not bytes"));
    };
    bytes
        .as_slice()
        .try_into()
        .map_err(|_| Error::Invalid("enrollment byte length invalid"))
}

fn random<const N: usize>() -> Result<[u8; N], Error> {
    let mut bytes = [0u8; N];
    getrandom::fill(&mut bytes).map_err(Error::Random)?;
    Ok(bytes)
}

fn random_v4() -> Result<[u8; 16], Error> {
    let mut bytes = random::<16>()?;
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{operation::Operation, shared_ready::ReadyFamilySession};
    use std::{fs, time::SystemTime};

    fn hex(hex: &str) -> Vec<u8> {
        hex.as_bytes()
            .chunks_exact(2)
            .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
            .collect()
    }

    #[test]
    fn a_recipient_stays_keyless_until_grant_then_projects_history() {
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../../tests/vectors/contiguous-chain-v1.json"))
                .unwrap();
        let transitions = fixture["transitions"].as_array().unwrap();
        let wire = |index: usize| hex(transitions[index]["committed_cbor_hex"].as_str().unwrap());
        let family = FamilyHandle {
            family_id: hex(fixture["test_only_inputs"]["family_id_hex"]
                .as_str()
                .unwrap())
            .try_into()
            .unwrap(),
            device_id: hex(fixture["test_only_inputs"]["recipient_device_id_hex"]
                .as_str()
                .unwrap())
            .try_into()
            .unwrap(),
        };
        let relay_public: [u8; 32] =
            hex("2543b92ff1095511476adc8369db6ddc933665a11978dda1404ee1066ca9559d")
                .try_into()
                .unwrap();
        let bootstrap =
            InvitationBootstrap::from_fragment(fixture["bootstrap"]["fragment"].as_str().unwrap())
                .unwrap();
        let Value::Map(claim) = cbor::decode(&wire(2)).unwrap() else {
            unreachable!()
        };
        let candidate_bytes = cbor::encode(&Value::Map(vec![
            (1, claim[0].1.clone()),
            (2, claim[1].1.clone()),
        ]))
        .unwrap();
        let enrollment = EnrollmentAttempt {
            family,
            invitation_id: bootstrap.invitation_id(),
            bootstrap_fragment: bootstrap.to_fragment().unwrap(),
            candidate_bytes,
            device_sign_seed: hex(fixture["test_only_inputs"]["recipient_sign_seed_hex"]
                .as_str()
                .unwrap())
            .try_into()
            .unwrap(),
            device_agreement_private: hex(
                fixture["test_only_inputs"]["recipient_agreement_seed_hex"]
                    .as_str()
                    .unwrap(),
            )
            .try_into()
            .unwrap(),
        };
        let nonce = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "babytrack-recipient-ready-{}-{nonce}.sqlite",
            std::process::id()
        ));
        let mut store = SqliteStore::open(&path).unwrap();
        store
            .create_family(family.family_id, family.device_id)
            .unwrap();
        let mut public =
            PublicHistorySession::begin(&mut store, family, &wire(0), relay_public).unwrap();
        for index in 1..=4 {
            public.accept_control(&mut store, &wire(index)).unwrap();
        }
        assert!(ReadyFamilySession::from_enrollment(&store, &enrollment).is_err());
        for transition in transitions.iter().take(5) {
            for object in transition["manifest"].as_array().unwrap() {
                let id = object[1].as_str().unwrap();
                public
                    .accept_object(
                        &mut store,
                        hex(id).try_into().unwrap(),
                        &hex(fixture["objects_by_id_hex"][id].as_str().unwrap()),
                    )
                    .unwrap();
            }
        }
        public.accept_control(&mut store, &wire(5)).unwrap();
        assert!(ReadyFamilySession::from_enrollment(&store, &enrollment).is_err());
        for object in transitions[5]["manifest"].as_array().unwrap() {
            let id = object[1].as_str().unwrap();
            public
                .accept_object(
                    &mut store,
                    hex(id).try_into().unwrap(),
                    &hex(fixture["objects_by_id_hex"][id].as_str().unwrap()),
                )
                .unwrap();
        }
        let ready = ReadyFamilySession::from_enrollment(&store, &enrollment).unwrap();
        assert_eq!(ready.observed_cursor(), 6);
        assert_eq!(ready.active_epoch(), 1);
        assert_eq!(
            ReadyFamilySession::from_admission_grant(
                &store,
                family,
                enrollment.agreement_private(),
            )
            .unwrap()
            .observed_cursor(),
            6
        );
        public.accept_control(&mut store, &wire(6)).unwrap();
        for object in transitions[6]["manifest"].as_array().unwrap() {
            let id = object[1].as_str().unwrap();
            public
                .accept_object(
                    &mut store,
                    hex(id).try_into().unwrap(),
                    &hex(fixture["objects_by_id_hex"][id].as_str().unwrap()),
                )
                .unwrap();
        }
        let batch = &fixture["batch"];
        public
            .accept_batch(
                &mut store,
                &hex(batch["envelope_cbor_hex"].as_str().unwrap()),
                &hex(batch["receipt_cbor_hex"].as_str().unwrap()),
            )
            .unwrap();
        let ready = ReadyFamilySession::from_enrollment(&store, &enrollment).unwrap();
        assert_eq!(ready.observed_cursor(), 8);
        let operation = Operation::decode_bound(
            &hex(batch["operation_cbor_hex"].as_str().unwrap()),
            &family.family_id,
            &family.device_id,
        )
        .unwrap();
        assert!(ready.projection().record(&operation.record_id).is_some());
        public.accept_control(&mut store, &wire(7)).unwrap();
        assert!(ReadyFamilySession::from_enrollment(&store, &enrollment).is_err());
        drop(store);
        fs::remove_file(path).unwrap();
    }
}
