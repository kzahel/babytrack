//! Browser binding over the shared Rust verification and projection path.

#![forbid(unsafe_code)]

use std::collections::BTreeMap;

use babytrack_core::projection::{Outcome, Projection, VerifiedEpochKey};
use babytrack_core::{
    batch,
    bootstrap::InvitationBootstrap,
    cbor::{self, Value},
    claim,
    control_chain::ControlChain,
    crypto,
    projection::LocalProjection,
    proof, ready_replay, session,
    sync_wire::{self, LogPage},
    web_actions,
};
use babytrack_core::{
    hlc::Clock,
    operation::{Hlc, Operation},
};
use wasm_bindgen::prelude::*;

#[cfg(feature = "fixture-api")]
#[wasm_bindgen]
pub struct WasmFamily {
    projection: Projection,
}

#[wasm_bindgen]
pub struct WasmLocalFamily {
    projection: LocalProjection,
    family_id: [u8; 16],
    device_id: [u8; 16],
    last_hlc: Option<Hlc>,
}

/// Public relay authority replay. Encrypted record projection and local
/// outbox handling are separate steps, but every committed cursor is checked
/// against the same Rust control chain used by native clients and the relay.
#[wasm_bindgen]
pub struct WasmPublicFamily {
    chain: ControlChain,
    relay_public_key: [u8; 32],
    prefix: BTreeMap<u64, ControlChain>,
}

/// Keyless browser invitation control replay. Each page is checked by the
/// shared Rust authority chain before a browser may prepare a claim.
#[wasm_bindgen]
pub struct WasmInvitation {
    bootstrap: InvitationBootstrap,
    genesis: Option<Vec<u8>>,
    chain: Option<ControlChain>,
    controls: Vec<Vec<u8>>,
    control_cursor: u64,
    linked_issue: bool,
    page_count: usize,
    total_bytes: usize,
}

#[wasm_bindgen]
impl WasmInvitation {
    #[wasm_bindgen(constructor)]
    pub fn new(fragment: &str) -> Result<Self, JsError> {
        Ok(Self {
            bootstrap: InvitationBootstrap::from_fragment(fragment).map_err(debug_error)?,
            genesis: None,
            chain: None,
            controls: Vec::new(),
            control_cursor: 0,
            linked_issue: false,
            page_count: 0,
            total_bytes: 0,
        })
    }

    pub fn family_id(&self) -> Vec<u8> {
        self.bootstrap.family_id().to_vec()
    }

    pub fn genesis_bytes(&self) -> Result<Vec<u8>, JsError> {
        self.genesis
            .clone()
            .ok_or_else(|| JsError::new("genesis absent"))
    }

    pub fn relay_public_key(&self) -> Vec<u8> {
        self.bootstrap.relay_public_key().to_vec()
    }

    pub fn has_admission(&self, device_id: &[u8]) -> Result<bool, JsError> {
        let chain = self
            .chain
            .as_ref()
            .ok_or_else(|| JsError::new("genesis absent"))?;
        Ok(chain
            .initial_admission_grant(&fixed(device_id, "device ID")?)
            .is_some())
    }

    pub fn relay_origin(&self) -> String {
        self.bootstrap.relay_origin().to_owned()
    }

    pub fn role(&self) -> u8 {
        self.bootstrap.fixed_role()
    }

    pub fn control_cursor(&self) -> u64 {
        self.control_cursor
    }

    pub fn linked_issue(&self) -> bool {
        self.linked_issue
    }

    pub fn head_hash(&self) -> Result<Vec<u8>, JsError> {
        Ok(self
            .chain
            .as_ref()
            .ok_or_else(|| JsError::new("invitation genesis not yet verified"))?
            .head_hash()
            .to_vec())
    }

    pub fn control_read_path(&self, after: u64) -> Result<String, JsError> {
        if after != self.control_cursor {
            return Err(JsError::new("invitation control read skips saved prefix"));
        }
        let family: String = self
            .bootstrap
            .family_id()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        Ok(format!("/v1/families/{family}/control?after={after}"))
    }

    pub fn sign_control_read(&self, after: u64, request_id: &[u8]) -> Result<Vec<u8>, JsError> {
        let path = self.control_read_path(after)?;
        self.bootstrap
            .sign_get_with_id(&path, fixed(request_id, "request ID")?)
            .map_err(debug_error)
    }

    /// Once the claim commits, invitation reads close. The saved pending
    /// device signs the same exact control path with its own credential.
    pub fn sign_pending_control_read(
        &self,
        after: u64,
        device_id: &[u8],
        signing_seed: &[u8],
        request_id: &[u8],
    ) -> Result<Vec<u8>, JsError> {
        let path = self.control_read_path(after)?;
        sync_wire::sign_get_with_id(
            self.bootstrap.family_id(),
            self.chain
                .as_ref()
                .ok_or_else(|| JsError::new("genesis absent"))?
                .relay_id(),
            fixed(device_id, "pending device ID")?,
            &fixed(signing_seed, "pending signing seed")?,
            &path,
            fixed(request_id, "request ID")?,
        )
        .map_err(debug_error)
    }

    /// An admitted device may use its fully verified sparse authority view
    /// to bootstrap a fresh full-log read from cursor one.
    pub fn sign_family_read(
        &self,
        exact_path: &str,
        device_id: &[u8],
        signing_seed: &[u8],
        request_id: &[u8],
    ) -> Result<Vec<u8>, JsError> {
        let family: String = self
            .bootstrap
            .family_id()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        if !exact_path.starts_with(&format!("/v1/families/{family}/")) {
            return Err(JsError::new("read path belongs to another Family"));
        }
        sync_wire::sign_get_with_id(
            self.bootstrap.family_id(),
            self.chain
                .as_ref()
                .ok_or_else(|| JsError::new("genesis absent"))?
                .relay_id(),
            fixed(device_id, "device ID")?,
            &fixed(signing_seed, "signing seed")?,
            exact_path,
            fixed(request_id, "request ID")?,
        )
        .map_err(debug_error)
    }

    pub fn challenge_hpke_object_id(&self) -> Result<Vec<u8>, JsError> {
        Ok(self
            .chain
            .as_ref()
            .and_then(|chain| chain.latest_challenge(&self.bootstrap.invitation_id()))
            .ok_or_else(|| JsError::new("no verified challenge"))?
            .hpke_object_id()
            .to_vec())
    }

    pub fn sign_challenge_object_read(
        &self,
        object_id: &[u8],
        device_id: &[u8],
        signing_seed: &[u8],
        request_id: &[u8],
    ) -> Result<Vec<u8>, JsError> {
        let id: [u8; 16] = fixed(object_id, "challenge object ID")?;
        if self.challenge_hpke_object_id()? != id {
            return Err(JsError::new("object is not the committed challenge"));
        }
        let family: String = self
            .bootstrap
            .family_id()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        let object: String = id.iter().map(|byte| format!("{byte:02x}")).collect();
        let path = format!("/v1/families/{family}/objects/{object}");
        sync_wire::sign_get_with_id(
            self.bootstrap.family_id(),
            self.chain
                .as_ref()
                .ok_or_else(|| JsError::new("genesis absent"))?
                .relay_id(),
            fixed(device_id, "pending device ID")?,
            &fixed(signing_seed, "pending signing seed")?,
            &path,
            fixed(request_id, "request ID")?,
        )
        .map_err(debug_error)
    }

    pub fn prepare_proof(
        &self,
        device_id: &[u8],
        signing_seed: &[u8],
        agreement_private: &[u8],
        object_response: &[u8],
        transition_id: &[u8],
    ) -> Result<Vec<u8>, JsError> {
        let object = self.verified_challenge_object(object_response)?;
        Ok(proof::build_candidate(
            self.chain
                .as_ref()
                .ok_or_else(|| JsError::new("genesis absent"))?,
            self.bootstrap.invitation_id(),
            fixed(device_id, "pending device ID")?,
            &fixed(signing_seed, "pending signing seed")?,
            &fixed(agreement_private, "agreement private key")?,
            &object,
            fixed(transition_id, "proof transition ID")?,
        )
        .map_err(debug_error)?
        .0)
    }

    pub fn verified_challenge_object(&self, response: &[u8]) -> Result<Vec<u8>, JsError> {
        let id: [u8; 16] = self
            .challenge_hpke_object_id()?
            .try_into()
            .map_err(|_| JsError::new("challenge object ID length"))?;
        for control in &self.controls {
            if ready_replay::manifest_objects(control)
                .map_err(debug_error)?
                .iter()
                .any(|object| object.id == id)
            {
                return ready_replay::verified_object_from_response(control, id, response)
                    .map_err(debug_error);
            }
        }
        Err(JsError::new("challenge object missing from saved controls"))
    }

    /// The caller saves this exact candidate and its four secrets before POST.
    pub fn prepare_claim(
        &self,
        device_id: &[u8],
        signing_seed: &[u8],
        agreement_private: &[u8],
        enrollment_nonce: &[u8],
        transition_id: &[u8],
    ) -> Result<Vec<u8>, JsError> {
        if !self.linked_issue {
            return Err(JsError::new("linked invitation issue not verified"));
        }
        claim::build_claim(
            &self.bootstrap,
            self.chain
                .as_ref()
                .ok_or_else(|| JsError::new("genesis absent"))?,
            claim::ClaimIdentity {
                family_id: self.bootstrap.family_id(),
                device_id: fixed(device_id, "device ID")?,
            },
            &fixed(signing_seed, "signing seed")?,
            &fixed(agreement_private, "agreement private key")?,
            &fixed(enrollment_nonce, "enrollment nonce")?,
            fixed(transition_id, "transition ID")?,
        )
        .map_err(debug_error)
    }

    /// A 200 response alone is insufficient: require the relay-signed
    /// committed control to contain the exact saved candidate.
    pub fn accept_claim_response(
        &mut self,
        response: &[u8],
        candidate: &[u8],
    ) -> Result<Vec<u8>, JsError> {
        self.accept_control_response(response, candidate)
    }

    pub fn accept_control_response(
        &mut self,
        response: &[u8],
        candidate: &[u8],
    ) -> Result<Vec<u8>, JsError> {
        let value = cbor::decode_with_limits(
            response,
            cbor::Limits {
                max_bytes: 1024 * 1024 + 128,
                max_depth: 16,
            },
        )
        .map_err(debug_error)?;
        let Value::Map(wrapper) = value else {
            return Err(JsError::new("control result not map"));
        };
        if wrapper.len() != 2 || wrapper[0] != (1, Value::Integer(1)) || wrapper[1].0 != 2 {
            return Err(JsError::new("control result shape invalid"));
        }
        let Value::Bytes(committed) = &wrapper[1].1 else {
            return Err(JsError::new("control result missing committed control"));
        };
        let Value::Map(root) = cbor::decode(committed).map_err(debug_error)? else {
            return Err(JsError::new("committed control not map"));
        };
        if root.len() != 4 {
            return Err(JsError::new("committed control width invalid"));
        }
        let extracted = cbor::encode(&Value::Map(vec![
            (1, root[0].1.clone()),
            (2, root[1].1.clone()),
        ]))
        .map_err(debug_error)?;
        if extracted != candidate {
            return Err(JsError::new(
                "committed control differs from saved candidate",
            ));
        }
        let mut chain = self
            .chain
            .clone()
            .ok_or_else(|| JsError::new("genesis absent"))?;
        chain.apply_sparse_control(committed).map_err(debug_error)?;
        self.control_cursor = chain.last_global_cursor();
        self.chain = Some(chain);
        self.controls.push(committed.clone());
        Ok(committed.clone())
    }

    /// Apply one bounded sparse control page atomically. Data-batch cursor
    /// gaps are allowed; unsigned gaps, forks, and another invitation's issue
    /// cannot establish this link's authority.
    pub fn accept_control_page(&mut self, bytes: &[u8], after: u64) -> Result<bool, JsError> {
        if after != self.control_cursor || self.page_count >= 64 {
            return Err(JsError::new(
                "invitation control page order or limit invalid",
            ));
        }
        let total_bytes = self
            .total_bytes
            .checked_add(bytes.len())
            .filter(|total| *total <= 16 * 1024 * 1024)
            .ok_or_else(|| JsError::new("invitation control history exceeds size limit"))?;
        let page = sync_wire::ControlPage::decode(bytes, self.bootstrap.family_id(), after)
            .map_err(debug_error)?;
        if page.has_more && page.entries.is_empty() {
            return Err(JsError::new(
                "relay claims more after empty invitation page",
            ));
        }
        let mut genesis = self.genesis.clone();
        let mut chain = self.chain.clone();
        let mut controls = self.controls.clone();
        let mut linked_issue = self.linked_issue;
        for entry in &page.entries {
            if genesis.is_none() {
                if entry.cursor != 1 {
                    return Err(JsError::new("invitation genesis cursor invalid"));
                }
                let verified = self
                    .bootstrap
                    .verify_genesis(&entry.committed_bytes)
                    .map_err(debug_error)?;
                if verified.last_global_cursor() != entry.cursor {
                    return Err(JsError::new("invitation genesis cursor differs"));
                }
                genesis = Some(entry.committed_bytes.clone());
                chain = Some(verified);
                continue;
            }
            if !linked_issue
                && self
                    .bootstrap
                    .matches_issue_signed_hash(&entry.committed_bytes)
                    .map_err(debug_error)?
            {
                let prior: Vec<&[u8]> = controls.iter().map(Vec::as_slice).collect();
                self.bootstrap
                    .verify_issue_sparse_with_controls(
                        genesis.as_ref().expect("genesis checked"),
                        &entry.committed_bytes,
                        &prior,
                    )
                    .map_err(debug_error)?;
                linked_issue = true;
            }
            let verified = chain.as_mut().expect("genesis checked");
            verified
                .apply_sparse_control(&entry.committed_bytes)
                .map_err(debug_error)?;
            if verified.last_global_cursor() != entry.cursor {
                return Err(JsError::new("invitation control cursor differs"));
            }
            controls.push(entry.committed_bytes.clone());
        }
        self.genesis = genesis;
        self.chain = chain;
        self.controls = controls;
        self.linked_issue = linked_issue;
        self.control_cursor = page.next_after;
        self.page_count += 1;
        self.total_bytes = total_bytes;
        Ok(page.has_more)
    }
}

/// Data view rooted in a verified epoch-one key. Rotation objects and the
/// complete keyring are checked by the shared Rust authority path before a
/// later epoch or its records become usable.
#[wasm_bindgen]
pub struct WasmInitialFamily {
    genesis: Vec<u8>,
    relay_public_key: [u8; 32],
    epoch_key: [u8; 32],
    admitted_keys: Vec<[u8; 32]>,
    agreement_private: Option<[u8; 32]>,
    holder_device_id: Option<[u8; 16]>,
    objects: BTreeMap<[u8; 16], Vec<u8>>,
    ready: Option<InitialReady>,
}

struct InitialReady {
    chain: ControlChain,
    projection: Projection,
    overlay: Option<Projection>,
    overlay_operations: Vec<Operation>,
    keys: BTreeMap<u32, VerifiedEpochKey>,
}

#[wasm_bindgen]
impl WasmInitialFamily {
    #[wasm_bindgen(constructor)]
    pub fn new(genesis: &[u8], relay_public_key: &[u8], epoch_key: &[u8]) -> Result<Self, JsError> {
        Self::with_admission_keys(genesis, relay_public_key, epoch_key)
    }

    pub fn with_admission_keys(
        genesis: &[u8],
        relay_public_key: &[u8],
        keys: &[u8],
    ) -> Result<Self, JsError> {
        let relay_public_key = fixed(relay_public_key, "relay public key")?;
        if keys.is_empty() || !keys.len().is_multiple_of(32) || keys.len() > 32 * 1024 {
            return Err(JsError::new("admission key history has invalid length"));
        }
        let admitted_keys = keys
            .chunks_exact(32)
            .map(|key| fixed(key, "epoch key"))
            .collect::<Result<Vec<[u8; 32]>, _>>()?;
        let epoch_key = admitted_keys[0];
        ControlChain::from_genesis(genesis, relay_public_key)
            .map_err(debug_error)?
            .verify_initial_epoch_key(&epoch_key)
            .map_err(debug_error)?;
        Ok(Self {
            genesis: genesis.to_vec(),
            relay_public_key,
            epoch_key,
            admitted_keys,
            agreement_private: None,
            holder_device_id: None,
            objects: BTreeMap::new(),
            ready: None,
        })
    }

    /// The local holder's agreement secret is needed to open its addressed
    /// grant after a rotation. It never leaves this browser process.
    pub fn set_agreement_private(
        &mut self,
        device_id: &[u8],
        private: &[u8],
    ) -> Result<(), JsError> {
        if self.ready.is_some() {
            return Err(JsError::new("ready view already initialized"));
        }
        self.agreement_private = Some(fixed(private, "agreement private key")?);
        self.holder_device_id = Some(fixed(device_id, "holder device ID")?);
        Ok(())
    }

    pub fn add_object(&mut self, object_id: &[u8], object_bytes: &[u8]) -> Result<(), JsError> {
        if self.ready.is_some() {
            return Err(JsError::new("ready view already initialized"));
        }
        let object_id = fixed(object_id, "object ID")?;
        if self.objects.contains_key(&object_id) {
            return Err(JsError::new("duplicate manifest object"));
        }
        self.objects.insert(object_id, object_bytes.to_vec());
        Ok(())
    }

    pub fn finish(&mut self) -> Result<(), JsError> {
        if self.ready.is_some() {
            return Err(JsError::new("ready view already initialized"));
        }
        let (chain, projection, key) = ready_replay::initial_epoch_projection(
            &self.genesis,
            self.relay_public_key,
            self.epoch_key,
            &self.objects,
        )
        .map_err(debug_error)?;
        let mut keys = BTreeMap::from([(1, key)]);
        for (index, bytes) in self.admitted_keys.iter().enumerate().skip(1) {
            let epoch = u32::try_from(index + 1).map_err(debug_error)?;
            keys.insert(
                epoch,
                ready_replay::saved_history_key(chain.family_id(), epoch, *bytes),
            );
        }
        self.ready = Some(InitialReady {
            chain,
            projection,
            overlay: None,
            overlay_operations: Vec::new(),
            keys,
        });
        Ok(())
    }

    pub fn apply_batch(
        &mut self,
        envelope_bytes: &[u8],
        receipt_bytes: &[u8],
    ) -> Result<bool, JsError> {
        let ready = self
            .ready
            .as_mut()
            .ok_or_else(|| JsError::new("manifest objects not yet verified"))?;
        let mut chain = ready.chain.clone();
        let signed = chain
            .apply_public_batch(envelope_bytes, receipt_bytes)
            .map_err(debug_error)?;
        let mut projection = ready.projection.clone();
        let key = ready
            .keys
            .get(&signed.header().epoch)
            .ok_or_else(|| JsError::new("verified batch epoch key is missing"))?;
        let outcome = projection
            .apply_authorized_signed(&signed, key, chain.last_global_cursor())
            .map_err(debug_error)?;
        ready.chain = chain;
        ready.projection = projection;
        ready.overlay = None;
        ready.overlay_operations.clear();
        Ok(matches!(outcome, Outcome::Applied))
    }

    /// Replay a signed authority change. Rotation cannot advance the ready
    /// view without its committed grant, keyring, and membership objects.
    pub fn apply_control(&mut self, committed_bytes: &[u8]) -> Result<(), JsError> {
        let ready = self
            .ready
            .as_mut()
            .ok_or_else(|| JsError::new("manifest objects not yet verified"))?;
        let mut chain = ready.chain.clone();
        let previous_epoch = chain.epoch().map_err(debug_error)?;
        chain.apply_control(committed_bytes).map_err(debug_error)?;
        ready_replay::verify_manifest(committed_bytes, &self.objects).map_err(debug_error)?;
        let transition_id = ready_replay::manifest_objects(committed_bytes)
            .map_err(debug_error)?
            .first()
            .map(|object| object.transition_id);
        let mut keys = ready.keys.clone();
        let next_epoch = chain.epoch().map_err(debug_error)?;
        if next_epoch > previous_epoch {
            if next_epoch != previous_epoch + 1 {
                return Err(JsError::new("rotation skipped an epoch"));
            }
            let (transition_id, rotation) = chain
                .rotation_for_epoch(next_epoch)
                .ok_or_else(|| JsError::new("verified rotation is missing"))?;
            let keyring = self
                .objects
                .get(&rotation.keyring_id())
                .ok_or_else(|| JsError::new("rotation keyring is missing"))?;
            let membership = chain
                .membership_check(&transition_id)
                .ok_or_else(|| JsError::new("rotation membership is missing"))?;
            let membership_object = self
                .objects
                .get(&membership.object_id())
                .ok_or_else(|| JsError::new("rotation membership object is missing"))?;
            let agreement_private = self
                .agreement_private
                .as_ref()
                .ok_or_else(|| JsError::new("local rotation agreement key is missing"))?;
            let grants = rotation
                .grant_ids()
                .into_iter()
                .map(|id| {
                    self.objects
                        .get(&id)
                        .cloned()
                        .map(|object| (id, object))
                        .ok_or_else(|| JsError::new("rotation grant object is missing"))
                })
                .collect::<Result<Vec<_>, _>>()?;
            let rotated = if let Some(known) = keys.get(&next_epoch) {
                chain.open_rotation_from_known_epoch_key(
                    &transition_id,
                    known,
                    keyring,
                    membership_object,
                )
            } else {
                chain.open_rotation_for(
                    &transition_id,
                    self.holder_device_id
                        .ok_or_else(|| JsError::new("rotation holder device ID is missing"))?,
                    agreement_private,
                    &grants,
                    keyring,
                    membership_object,
                )
            }
            .map_err(debug_error)?;
            for (epoch, prior) in keys.iter().filter(|(epoch, _)| **epoch < next_epoch) {
                if rotated.earlier(*epoch) != Some(prior) {
                    return Err(JsError::new("rotation keyring differs from saved history"));
                }
            }
            keys.insert(next_epoch, rotated.current().clone());
        } else if let Some(transition_id) = transition_id
            && let Some(membership) = chain.membership_check(&transition_id)
        {
            let object = self
                .objects
                .get(&membership.object_id())
                .ok_or_else(|| JsError::new("membership object is missing"))?;
            let key = keys
                .get(&next_epoch)
                .ok_or_else(|| JsError::new("membership epoch key is missing"))?;
            membership.verify(object, key).map_err(debug_error)?;
        }
        let mut projection = ready.projection.clone();
        projection
            .advance_control(chain.last_global_cursor())
            .map_err(debug_error)?;
        ready.chain = chain;
        ready.keys = keys;
        ready.projection = projection;
        ready.overlay = None;
        ready.overlay_operations.clear();
        Ok(())
    }

    pub fn verify_admission_history(&self) -> Result<(), JsError> {
        let ready = self
            .ready
            .as_ref()
            .ok_or_else(|| JsError::new("manifest objects not yet verified"))?;
        if self.admitted_keys.len() > ready.chain.epoch().map_err(debug_error)? as usize {
            return Err(JsError::new("saved admission key exceeds verified history"));
        }
        Ok(())
    }

    /// Add one durable unsent operation to the local preview without advancing
    /// the verified relay cursor. The browser rebuilds this from saved work.
    pub fn preview_one(&mut self, operation_bytes: &[u8], device_id: &[u8]) -> Result<(), JsError> {
        let ready = self
            .ready
            .as_mut()
            .ok_or_else(|| JsError::new("manifest objects not yet verified"))?;
        let device_id = fixed(device_id, "device ID")?;
        ready
            .chain
            .active_signing_public(device_id)
            .map_err(debug_error)?;
        let operation =
            Operation::decode_bound(operation_bytes, &ready.chain.family_id(), &device_id)
                .map_err(debug_error)?;
        let mut operations = ready.overlay_operations.clone();
        operations.push(operation);
        let overlay = ready
            .projection
            .with_local_overlay(&operations)
            .map_err(debug_error)?;
        ready.overlay_operations = operations;
        ready.overlay = Some(overlay);
        Ok(())
    }

    pub fn last_cursor(&self) -> Result<u64, JsError> {
        Ok(self
            .ready
            .as_ref()
            .ok_or_else(|| JsError::new("manifest objects not yet verified"))?
            .projection
            .last_cursor())
    }

    pub fn head_hash(&self) -> Result<Vec<u8>, JsError> {
        Ok(self
            .ready
            .as_ref()
            .ok_or_else(|| JsError::new("manifest objects not yet verified"))?
            .chain
            .head_hash()
            .to_vec())
    }

    /// Seal one validated operation with a fresh core-generated identity and
    /// nonce. The browser must durably save these exact bytes before POST.
    pub fn prepare_one(
        &self,
        operation_bytes: &[u8],
        device_id: &[u8],
        signing_seed: &[u8],
    ) -> Result<Vec<u8>, JsError> {
        let ready = self
            .ready
            .as_ref()
            .ok_or_else(|| JsError::new("manifest objects not yet verified"))?;
        let device_id = fixed(device_id, "device ID")?;
        let signing_seed = fixed(signing_seed, "signing seed")?;
        if ready
            .chain
            .active_signing_public(device_id)
            .map_err(debug_error)?
            != crypto::signing_public_key(&signing_seed)
        {
            return Err(JsError::new(
                "active device authority differs from credential",
            ));
        }
        let operation =
            Operation::decode_bound(operation_bytes, &ready.chain.family_id(), &device_id)
                .map_err(debug_error)?;
        ready
            .projection
            .with_local_overlay(&[operation])
            .map_err(debug_error)?;
        let plaintext = cbor::encode(&Value::Array(vec![Value::Bytes(operation_bytes.to_vec())]))
            .map_err(debug_error)?;
        let mut batch_id = [0u8; 16];
        let mut nonce = [0u8; 24];
        getrandom::fill(&mut batch_id).map_err(debug_error)?;
        batch_id[6] = (batch_id[6] & 0x0f) | 0x40;
        batch_id[8] = (batch_id[8] & 0x3f) | 0x80;
        getrandom::fill(&mut nonce).map_err(debug_error)?;
        let header = batch::Header {
            minor: 0,
            family_id: ready.chain.family_id(),
            relay_id: ready.chain.relay_id(),
            control_head: ready.chain.head_hash(),
            epoch: ready.chain.epoch().map_err(debug_error)?,
            batch_id,
            author_device_id: device_id,
            device_sequence: ready
                .chain
                .next_sequence_for(device_id)
                .map_err(debug_error)?,
            nonce,
            plaintext_len: plaintext
                .len()
                .try_into()
                .map_err(|_| JsError::new("operation too large"))?,
        };
        let epoch = ready.chain.epoch().map_err(debug_error)?;
        let key = ready
            .keys
            .get(&epoch)
            .ok_or_else(|| JsError::new("active epoch key is missing"))?;
        Ok(batch::seal(
            &header,
            &[operation_bytes.to_vec()],
            &key.bytes_for_storage(),
            &signing_seed,
        )
        .map_err(debug_error)?
        .envelope_bytes)
    }

    pub fn field_cbor(&self, record_id: &[u8], field_id: u64) -> Result<Vec<u8>, JsError> {
        let record_id = fixed(record_id, "record ID")?;
        let ready = self
            .ready
            .as_ref()
            .ok_or_else(|| JsError::new("manifest objects not yet verified"))?;
        Ok(ready
            .overlay
            .as_ref()
            .unwrap_or(&ready.projection)
            .record(&record_id)
            .and_then(|record| record.field(field_id))
            .map_or_else(Vec::new, |field| field.canonical_bytes.clone()))
    }

    pub fn record_type(&self, record_id: &[u8]) -> Result<Option<String>, JsError> {
        let record_id = fixed(record_id, "record ID")?;
        let ready = self
            .ready
            .as_ref()
            .ok_or_else(|| JsError::new("manifest objects not yet verified"))?;
        Ok(ready
            .overlay
            .as_ref()
            .unwrap_or(&ready.projection)
            .record(&record_id)
            .map(|record| record.record_type.clone()))
    }

    pub fn snapshot_json(&self) -> Result<String, JsError> {
        let ready = self
            .ready
            .as_ref()
            .ok_or_else(|| JsError::new("manifest objects not yet verified"))?;
        Ok(web_actions::shared_snapshot(
            ready.overlay.as_ref().unwrap_or(&ready.projection),
        ))
    }

    fn shared_identity(
        &self,
        device_id: &[u8],
        record: [u8; 16],
        last_wall_ms: i64,
        last_counter: u32,
        now_ms: i64,
    ) -> Result<web_actions::Identity, JsError> {
        let ready = self
            .ready
            .as_ref()
            .ok_or_else(|| JsError::new("Family not ready"))?;
        let family = ready.chain.family_id();
        let device = fixed(device_id, "device ID")?;
        ready
            .chain
            .active_signing_public(device)
            .map_err(debug_error)?;
        let last = (last_wall_ms >= 0).then_some(Hlc {
            wall_ms: last_wall_ms,
            counter: last_counter,
            device_id: device,
        });
        let mut clock = Clock::restore(family, device, last, None).map_err(debug_error)?;
        Ok(web_actions::Identity {
            family,
            device,
            operation: random_v7(now_ms)?,
            record,
            stamp: clock.next(now_ms).stamp,
        })
    }

    #[allow(clippy::too_many_arguments)] // Flat wasm boundary for browser form values.
    pub fn create_child_operation(
        &self,
        device_id: &[u8],
        last_wall_ms: i64,
        last_counter: u32,
        name: &str,
        birth_day: Option<i64>,
        sex: Option<u8>,
        now_ms: i64,
    ) -> Result<Vec<u8>, JsError> {
        web_actions::child(
            self.shared_identity(
                device_id,
                random_v7(now_ms)?,
                last_wall_ms,
                last_counter,
                now_ms,
            )?,
            name,
            birth_day,
            sex,
        )
        .map_err(debug_error)
    }

    #[allow(clippy::too_many_arguments)] // Flat wasm boundary for browser form values.
    pub fn log_diaper_operation(
        &self,
        device_id: &[u8],
        last_wall_ms: i64,
        last_counter: u32,
        child_id: &[u8],
        kind: u8,
        now_ms: i64,
        offset_minutes: i16,
    ) -> Result<Vec<u8>, JsError> {
        web_actions::diaper(
            self.shared_identity(
                device_id,
                random_v7(now_ms)?,
                last_wall_ms,
                last_counter,
                now_ms,
            )?,
            fixed(child_id, "child ID")?,
            kind,
            now_ms,
            offset_minutes,
        )
        .map_err(debug_error)
    }

    #[allow(clippy::too_many_arguments)] // Flat wasm boundary for browser form values.
    pub fn log_bottle_operation(
        &self,
        device_id: &[u8],
        last_wall_ms: i64,
        last_counter: u32,
        child_id: &[u8],
        ml: u32,
        content: u8,
        now_ms: i64,
        offset_minutes: i16,
    ) -> Result<Vec<u8>, JsError> {
        web_actions::bottle(
            self.shared_identity(
                device_id,
                random_v7(now_ms)?,
                last_wall_ms,
                last_counter,
                now_ms,
            )?,
            fixed(child_id, "child ID")?,
            ml,
            content,
            now_ms,
            offset_minutes,
        )
        .map_err(debug_error)
    }

    #[allow(clippy::too_many_arguments)] // Flat wasm boundary for browser form values.
    pub fn log_note_operation(
        &self,
        device_id: &[u8],
        last_wall_ms: i64,
        last_counter: u32,
        child_id: &[u8],
        note: &str,
        now_ms: i64,
        offset_minutes: i16,
    ) -> Result<Vec<u8>, JsError> {
        web_actions::note(
            self.shared_identity(
                device_id,
                random_v7(now_ms)?,
                last_wall_ms,
                last_counter,
                now_ms,
            )?,
            fixed(child_id, "child ID")?,
            note,
            now_ms,
            offset_minutes,
        )
        .map_err(debug_error)
    }

    pub fn log_breast_operation(
        &self,
        device_id: &[u8],
        last_wall_ms: i64,
        last_counter: u32,
        child_id: &[u8],
        segments_json: &str,
        now_ms: i64,
    ) -> Result<Vec<u8>, JsError> {
        web_actions::breast(
            self.shared_identity(
                device_id,
                random_v7(now_ms)?,
                last_wall_ms,
                last_counter,
                now_ms,
            )?,
            fixed(child_id, "child ID")?,
            segments_json,
        )
        .map_err(debug_error)
    }

    #[allow(clippy::too_many_arguments)] // Flat wasm boundary for browser form values.
    pub fn edit_breast_operation(
        &self,
        device_id: &[u8],
        last_wall_ms: i64,
        last_counter: u32,
        child_id: &[u8],
        activity_id: &[u8],
        segments_json: &str,
        now_ms: i64,
    ) -> Result<Vec<u8>, JsError> {
        let record_id = fixed(activity_id, "activity ID")?;
        let ready = self
            .ready
            .as_ref()
            .ok_or_else(|| JsError::new("Family not ready"))?;
        let target = ready
            .overlay
            .as_ref()
            .unwrap_or(&ready.projection)
            .record(&record_id)
            .ok_or_else(|| JsError::new("breast feed target unavailable"))?;
        web_actions::edit_breast(
            self.shared_identity(device_id, record_id, last_wall_ms, last_counter, now_ms)?,
            fixed(child_id, "child ID")?,
            target,
            segments_json,
        )
        .map_err(debug_error)
    }
}

#[wasm_bindgen]
pub fn operation_clock(
    operation: &[u8],
    family_id: &[u8],
    device_id: &[u8],
) -> Result<Vec<i64>, JsError> {
    let decoded = Operation::decode_bound(
        operation,
        &fixed(family_id, "Family ID")?,
        &fixed(device_id, "device ID")?,
    )
    .map_err(debug_error)?;
    Ok(vec![decoded.hlc.wall_ms, i64::from(decoded.hlc.counter)])
}

/// Bounded core-decoded relay log page. JavaScript handles transport and
/// persistence but never parses or interprets protocol CBOR.
#[wasm_bindgen]
pub struct WasmLogPage {
    page: LogPage,
}

#[wasm_bindgen]
impl WasmLogPage {
    #[wasm_bindgen(constructor)]
    pub fn new(bytes: &[u8], family_id: &[u8], after: u64) -> Result<Self, JsError> {
        Ok(Self {
            page: LogPage::decode(bytes, fixed(family_id, "Family ID")?, after)
                .map_err(debug_error)?,
        })
    }

    pub fn len(&self) -> usize {
        self.page.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.page.entries.is_empty()
    }

    pub fn has_more(&self) -> bool {
        self.page.has_more
    }

    pub fn next_after(&self) -> u64 {
        self.page.next_after
    }

    pub fn entry_cursor(&self, index: usize) -> Result<u64, JsError> {
        Ok(self.entry(index)?.cursor)
    }

    pub fn entry_kind(&self, index: usize) -> Result<u8, JsError> {
        Ok(self.entry(index)?.kind)
    }

    pub fn entry_bytes(&self, index: usize) -> Result<Vec<u8>, JsError> {
        Ok(self.entry(index)?.committed_bytes.clone())
    }

    fn entry(&self, index: usize) -> Result<&sync_wire::LogEntry, JsError> {
        self.page
            .entries
            .get(index)
            .ok_or_else(|| JsError::new("log entry index outside page"))
    }
}

#[wasm_bindgen]
impl WasmPublicFamily {
    #[wasm_bindgen(constructor)]
    pub fn new(genesis_bytes: &[u8], relay_public_key: &[u8]) -> Result<Self, JsError> {
        let relay_public_key = fixed(relay_public_key, "relay public key")?;
        let chain =
            ControlChain::from_genesis(genesis_bytes, relay_public_key).map_err(debug_error)?;
        let prefix = BTreeMap::from([(chain.last_global_cursor(), chain.clone())]);
        Ok(Self {
            chain,
            relay_public_key,
            prefix,
        })
    }

    pub fn apply_control(&mut self, committed_bytes: &[u8]) -> Result<(), JsError> {
        self.chain
            .apply_control(committed_bytes)
            .map_err(debug_error)?;
        self.prefix
            .insert(self.chain.last_global_cursor(), self.chain.clone());
        Ok(())
    }

    pub fn apply_batch(
        &mut self,
        envelope_bytes: &[u8],
        receipt_bytes: &[u8],
    ) -> Result<(), JsError> {
        self.chain
            .apply_public_batch(envelope_bytes, receipt_bytes)
            .map(|_| ())
            .map_err(debug_error)?;
        self.prefix
            .insert(self.chain.last_global_cursor(), self.chain.clone());
        Ok(())
    }

    pub fn family_id(&self) -> Vec<u8> {
        self.chain.family_id().to_vec()
    }

    pub fn relay_id(&self) -> Vec<u8> {
        self.chain.relay_id().to_vec()
    }

    pub fn initial_manager_device_id(&self) -> Vec<u8> {
        self.chain.initial_manager_device_id().to_vec()
    }

    pub fn last_cursor(&self) -> u64 {
        self.chain.last_global_cursor()
    }

    pub fn head_hash(&self) -> Vec<u8> {
        self.chain.head_hash().to_vec()
    }

    /// Open a recipient's committed epoch-one grant. Kept for older callers.
    pub fn open_initial_grant(
        &self,
        device_id: &[u8],
        agreement_private: &[u8],
        grant_object: &[u8],
    ) -> Result<Vec<u8>, JsError> {
        if self.chain.epoch().map_err(debug_error)? != 1 {
            return Err(JsError::new("rotated Family requires a verified keyring"));
        }
        let device_id = fixed(device_id, "device ID")?;
        self.chain
            .active_signing_public(device_id)
            .map_err(debug_error)?;
        let grant = self
            .chain
            .initial_admission_grant(&device_id)
            .ok_or_else(|| JsError::new("recipient has no committed admission grant"))?;
        let key = grant
            .open(
                grant_object,
                &fixed(agreement_private, "agreement private key")?,
            )
            .map_err(debug_error)?;
        self.chain
            .verify_initial_epoch_key(&key.bytes_for_storage())
            .map_err(debug_error)?;
        Ok(key.bytes_for_storage().to_vec())
    }

    pub fn initial_grant_id(&self, device_id: &[u8]) -> Result<Vec<u8>, JsError> {
        let device_id = fixed(device_id, "device ID")?;
        Ok(self
            .chain
            .initial_admission_grant(&device_id)
            .ok_or_else(|| JsError::new("recipient has no committed admission grant"))?
            .grant_id()
            .to_vec())
    }

    /// IDs of the signed admission grant and, for a rotated epoch, its
    /// keyring and membership objects. The adapter fetches manifest-bound
    /// bytes before asking the core to open any key.
    pub fn admission_object_ids(&self, device_id: &[u8]) -> Result<Vec<u8>, JsError> {
        let device_id = fixed(device_id, "device ID")?;
        let grant = self
            .chain
            .initial_admission_grant(&device_id)
            .ok_or_else(|| JsError::new("recipient has no committed admission grant"))?;
        let mut ids = grant.grant_id().to_vec();
        if grant.epoch() > 1 {
            let (transition_id, rotation) = self
                .chain
                .rotation_for_epoch(grant.epoch())
                .ok_or_else(|| JsError::new("admission epoch rotation is missing"))?;
            let membership = self
                .chain
                .membership_check(&transition_id)
                .ok_or_else(|| JsError::new("admission epoch membership is missing"))?;
            ids.extend_from_slice(&rotation.keyring_id());
            ids.extend_from_slice(&membership.object_id());
        }
        Ok(ids)
    }

    /// Return epoch keys in order only after the recipient grant and, when
    /// needed, the committed rotation keyring and membership are verified.
    pub fn open_admission_keys(
        &self,
        device_id: &[u8],
        agreement_private: &[u8],
        grant_object: &[u8],
        keyring_object: &[u8],
        membership_object: &[u8],
    ) -> Result<Vec<u8>, JsError> {
        let device_id = fixed(device_id, "device ID")?;
        self.chain
            .active_signing_public(device_id)
            .map_err(debug_error)?;
        let grant = self
            .chain
            .initial_admission_grant(&device_id)
            .ok_or_else(|| JsError::new("recipient has no committed admission grant"))?;
        let key = grant
            .open(
                grant_object,
                &fixed(agreement_private, "agreement private key")?,
            )
            .map_err(debug_error)?;
        if grant.epoch() == 1 {
            // The grant itself binds the key to the signed epoch commitment.
            return Ok(key.bytes_for_storage().to_vec());
        }
        let (transition_id, _) = self
            .chain
            .rotation_for_epoch(grant.epoch())
            .ok_or_else(|| JsError::new("admission epoch rotation is missing"))?;
        let recovered = self
            .chain
            .open_rotation_from_known_epoch_key(
                &transition_id,
                &key,
                keyring_object,
                membership_object,
            )
            .map_err(debug_error)?;
        let mut result = Vec::with_capacity(32 * grant.epoch() as usize);
        for epoch in 1..grant.epoch() {
            result.extend_from_slice(
                &recovered
                    .earlier(epoch)
                    .ok_or_else(|| JsError::new("admission history epoch is missing"))?
                    .bytes_for_storage(),
            );
        }
        result.extend_from_slice(&key.bytes_for_storage());
        Ok(result)
    }

    pub fn sign_get(
        &self,
        device_id: &[u8],
        signing_seed: &[u8],
        exact_path: &str,
        request_id: &[u8],
    ) -> Result<Vec<u8>, JsError> {
        let device_id = fixed(device_id, "device ID")?;
        let signing_seed = fixed(signing_seed, "signing seed")?;
        if self
            .chain
            .active_signing_public(device_id)
            .map_err(debug_error)?
            != crypto::signing_public_key(&signing_seed)
        {
            return Err(JsError::new("signing seed differs from active device"));
        }
        let family_hex: String = self
            .chain
            .family_id()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        if !exact_path.starts_with(&format!("/v1/families/{family_hex}/")) {
            return Err(JsError::new("read path belongs to another Family"));
        }
        sync_wire::sign_get_with_id(
            self.chain.family_id(),
            self.chain.relay_id(),
            device_id,
            &signing_seed,
            exact_path,
            fixed(request_id, "request ID")?,
        )
        .map_err(debug_error)
    }

    pub fn batch_id(&self, envelope: &[u8]) -> Result<Vec<u8>, JsError> {
        let value = cbor::decode_with_limits(
            envelope,
            cbor::Limits {
                max_bytes: 1024 * 1024,
                max_depth: 16,
            },
        )
        .map_err(debug_error)?;
        let Value::Map(fields) = value else {
            return Err(JsError::new("batch envelope not map"));
        };
        let Some((1, header)) = fields.first() else {
            return Err(JsError::new("batch header absent"));
        };
        Ok(
            batch::Header::decode(&cbor::encode(header).map_err(debug_error)?)
                .map_err(debug_error)?
                .batch_id
                .to_vec(),
        )
    }

    /// A stale batch can be resealed only after a relay-signed rejection is
    /// bound to this exact envelope and its stated authority prefix is saved.
    pub fn verified_stale_rejection(
        &self,
        envelope: &[u8],
        result_bytes: &[u8],
        device_id: &[u8],
    ) -> Result<bool, JsError> {
        let Some(receipt_bytes) = sync_wire::BatchResult::decode(result_bytes)
            .map_err(debug_error)?
            .receipt_bytes
        else {
            return Ok(false);
        };
        let receipt = match session::verify_rejected_receipt(&receipt_bytes, &self.relay_public_key)
        {
            Ok(receipt) => receipt,
            // An accepted result leaves the exact outbox bytes uncertain until
            // their accepted log entry is verified by ordinary replay.
            Err(_) => return Ok(false),
        };
        if receipt.reason != 1 {
            return Ok(false);
        }
        let device_id = fixed(device_id, "device ID")?;
        let signer = self
            .chain
            .active_signing_public(device_id)
            .map_err(debug_error)?;
        let signed = batch::verify_signed_envelope(
            envelope,
            &self.chain.family_id(),
            &self.chain.relay_id(),
            &signer,
        )
        .map_err(debug_error)?;
        let header = signed.header();
        let at_rejection = self
            .prefix
            .get(&receipt.cursor)
            .ok_or_else(|| JsError::new("rejection prefix is not saved"))?;
        if receipt.family_id != self.chain.family_id()
            || receipt.relay_id != self.chain.relay_id()
            || receipt.batch_id != header.batch_id
            || receipt.object_hash != signed.object_hash()
            || receipt.device_sequence != header.device_sequence
            || header.author_device_id != device_id
            || header.epoch >= at_rejection.epoch().map_err(debug_error)?
            || receipt.control_head != at_rejection.head_hash()
            || receipt.next_expected_sequence
                != at_rejection
                    .next_sequence_for(device_id)
                    .map_err(debug_error)?
            || self
                .chain
                .next_sequence_for(device_id)
                .map_err(debug_error)?
                < receipt.next_expected_sequence
        {
            return Err(JsError::new(
                "stale rejection differs from verified pending batch",
            ));
        }
        Ok(true)
    }
}

#[wasm_bindgen]
pub fn accepted_batch_receipt(result_bytes: &[u8]) -> Result<Vec<u8>, JsError> {
    sync_wire::BatchResult::decode(result_bytes)
        .map_err(debug_error)?
        .receipt_bytes
        .ok_or_else(|| JsError::new("committed batch receipt unavailable"))
}

/// IDs are returned as consecutive 16-byte values in signed manifest order.
#[wasm_bindgen]
pub fn manifest_object_ids(committed_control: &[u8]) -> Result<Vec<u8>, JsError> {
    Ok(ready_replay::manifest_objects(committed_control)
        .map_err(debug_error)?
        .into_iter()
        .flat_map(|object| object.id)
        .collect())
}

#[wasm_bindgen]
pub fn verified_manifest_object(
    committed_control: &[u8],
    object_id: &[u8],
    response: &[u8],
) -> Result<Vec<u8>, JsError> {
    ready_replay::verified_object_from_response(
        committed_control,
        fixed(object_id, "object ID")?,
        response,
    )
    .map_err(debug_error)
}

#[wasm_bindgen]
impl WasmLocalFamily {
    #[wasm_bindgen(constructor)]
    pub fn new(family_id: &[u8], device_id: &[u8]) -> Result<Self, JsError> {
        let family_id = fixed(family_id, "Family ID")?;
        Ok(Self {
            projection: LocalProjection::new(family_id),
            family_id,
            device_id: fixed(device_id, "device ID")?,
            last_hlc: None,
        })
    }

    pub fn append_operation(
        &mut self,
        operation_bytes: &[u8],
        index: u64,
    ) -> Result<Vec<u8>, JsError> {
        let operation = Operation::decode_bound(operation_bytes, &self.family_id, &self.device_id)
            .map_err(debug_error)?;
        self.projection
            .append(&operation, index)
            .map_err(debug_error)?;
        self.last_hlc = Some(operation.hlc.clone());
        Ok(operation.operation_id.to_vec())
    }

    pub fn last_append_index(&self) -> u64 {
        self.projection.last_append_index()
    }

    fn identity(&self, record: [u8; 16], now_ms: i64) -> Result<web_actions::Identity, JsError> {
        let mut clock = Clock::restore(self.family_id, self.device_id, self.last_hlc.clone(), None)
            .map_err(debug_error)?;
        let stamp = clock.next(now_ms).stamp;
        Ok(web_actions::Identity {
            family: self.family_id,
            device: self.device_id,
            operation: random_v7(now_ms)?,
            record,
            stamp,
        })
    }

    pub fn create_family_operation(&self, now_ms: i64) -> Result<Vec<u8>, JsError> {
        if self.last_append_index() != 0 {
            return Err(JsError::new("Family already initialized"));
        }
        web_actions::family(self.identity(self.family_id, now_ms)?).map_err(debug_error)
    }

    pub fn create_child_operation(
        &self,
        name: &str,
        birth_day: Option<i64>,
        sex: Option<u8>,
        now_ms: i64,
    ) -> Result<Vec<u8>, JsError> {
        web_actions::child(
            self.identity(random_v7(now_ms)?, now_ms)?,
            name,
            birth_day,
            sex,
        )
        .map_err(debug_error)
    }

    pub fn log_diaper_operation(
        &self,
        child_id: &[u8],
        kind: u8,
        now_ms: i64,
        offset_minutes: i16,
    ) -> Result<Vec<u8>, JsError> {
        web_actions::diaper(
            self.identity(random_v7(now_ms)?, now_ms)?,
            fixed(child_id, "child ID")?,
            kind,
            now_ms,
            offset_minutes,
        )
        .map_err(debug_error)
    }

    pub fn log_bottle_operation(
        &self,
        child_id: &[u8],
        ml: u32,
        content: u8,
        now_ms: i64,
        offset_minutes: i16,
    ) -> Result<Vec<u8>, JsError> {
        web_actions::bottle(
            self.identity(random_v7(now_ms)?, now_ms)?,
            fixed(child_id, "child ID")?,
            ml,
            content,
            now_ms,
            offset_minutes,
        )
        .map_err(debug_error)
    }

    pub fn log_note_operation(
        &self,
        child_id: &[u8],
        note: &str,
        now_ms: i64,
        offset_minutes: i16,
    ) -> Result<Vec<u8>, JsError> {
        web_actions::note(
            self.identity(random_v7(now_ms)?, now_ms)?,
            fixed(child_id, "child ID")?,
            note,
            now_ms,
            offset_minutes,
        )
        .map_err(debug_error)
    }

    pub fn log_breast_operation(
        &self,
        child_id: &[u8],
        segments_json: &str,
        now_ms: i64,
    ) -> Result<Vec<u8>, JsError> {
        web_actions::breast(
            self.identity(random_v7(now_ms)?, now_ms)?,
            fixed(child_id, "child ID")?,
            segments_json,
        )
        .map_err(debug_error)
    }

    pub fn edit_breast_operation(
        &self,
        child_id: &[u8],
        activity_id: &[u8],
        segments_json: &str,
        now_ms: i64,
    ) -> Result<Vec<u8>, JsError> {
        let activity_id = fixed(activity_id, "activity ID")?;
        let target = self
            .projection
            .record(&activity_id)
            .ok_or_else(|| JsError::new("breast feed target unavailable"))?;
        web_actions::edit_breast(
            self.identity(activity_id, now_ms)?,
            fixed(child_id, "child ID")?,
            target,
            segments_json,
        )
        .map_err(debug_error)
    }

    pub fn snapshot_json(&self) -> String {
        web_actions::local_snapshot(&self.projection)
    }

    pub fn field_cbor(&self, record_id: &[u8], field_id: u64) -> Result<Vec<u8>, JsError> {
        let record_id = fixed(record_id, "record ID")?;
        Ok(self
            .projection
            .record(&record_id)
            .and_then(|record| record.field(field_id))
            .map_or_else(Vec::new, |field| field.canonical_bytes.clone()))
    }
}

fn random_v7(now_ms: i64) -> Result<[u8; 16], JsError> {
    if !(0..(1i64 << 48)).contains(&now_ms) {
        return Err(JsError::new("time outside UUIDv7 range"));
    }
    let mut id = [0u8; 16];
    getrandom::fill(&mut id).map_err(debug_error)?;
    id[..6].copy_from_slice(&(now_ms as u64).to_be_bytes()[2..]);
    id[6] = (id[6] & 0x0f) | 0x70;
    id[8] = (id[8] & 0x3f) | 0x80;
    Ok(id)
}

#[wasm_bindgen]
pub fn new_local_ids() -> Result<Vec<u8>, JsError> {
    let mut ids = [0u8; 32];
    getrandom::fill(&mut ids).map_err(debug_error)?;
    for offset in [0usize, 16] {
        ids[offset + 6] = (ids[offset + 6] & 0x0f) | 0x40;
        ids[offset + 8] = (ids[offset + 8] & 0x3f) | 0x80;
    }
    Ok(ids.to_vec())
}

#[cfg(feature = "fixture-api")]
#[wasm_bindgen]
impl WasmFamily {
    #[wasm_bindgen(constructor)]
    pub fn new(family_id: &[u8]) -> Result<WasmFamily, JsError> {
        Ok(Self {
            projection: Projection::new(fixed(family_id, "Family ID")?),
        })
    }

    /// Fixture-only primitive replay. Production sharing will accept raw
    /// committed entries through a core-owned authorization session.
    pub fn apply_envelope(
        &mut self,
        envelope: &[u8],
        relay_id: &[u8],
        epoch_key: &[u8],
        signer_public_key: &[u8],
        cursor: u64,
    ) -> Result<bool, JsError> {
        let family_id = self.projection.family_id();
        let authenticated = batch::open_authenticated(
            envelope,
            &family_id,
            &fixed(relay_id, "relay ID")?,
            &fixed(epoch_key, "epoch key")?,
            &fixed(signer_public_key, "signer key")?,
        )
        .map_err(debug_error)?;
        match self
            .projection
            .apply_authenticated(&authenticated, cursor)
            .map_err(debug_error)?
        {
            Outcome::Applied => Ok(true),
            Outcome::Inert(_) => Ok(false),
        }
    }

    pub fn advance_control(&mut self, cursor: u64) -> Result<(), JsError> {
        self.projection.advance_control(cursor).map_err(debug_error)
    }

    pub fn last_cursor(&self) -> u64 {
        self.projection.last_cursor()
    }

    pub fn inert_count(&self) -> usize {
        self.projection.inert_batches().len()
    }

    /// Returns canonical CBOR bytes. Empty means the record or field is absent.
    pub fn field_cbor(&self, record_id: &[u8], field_id: u64) -> Result<Vec<u8>, JsError> {
        let record_id = fixed(record_id, "record ID")?;
        Ok(self
            .projection
            .record(&record_id)
            .and_then(|record| record.field(field_id))
            .map_or_else(Vec::new, |field| field.canonical_bytes.clone()))
    }
}

fn fixed<const N: usize>(bytes: &[u8], label: &str) -> Result<[u8; N], JsError> {
    bytes
        .try_into()
        .map_err(|_| JsError::new(&format!("{label} must be {N} bytes")))
}

fn debug_error(error: impl std::fmt::Debug) -> JsError {
    JsError::new(&format!("{error:?}"))
}

#[cfg(feature = "fixture-api")]
#[wasm_bindgen]
pub fn ed25519_public_key(signing_seed: &[u8]) -> Result<Vec<u8>, JsError> {
    Ok(crypto::signing_public_key(&fixed(signing_seed, "signing seed")?).to_vec())
}

/// Fixture-only fixed-header byte path; never use for production writes.
#[cfg(feature = "fixture-api")]
#[wasm_bindgen]
pub fn seal_one(
    header_cbor: &[u8],
    operation_cbor: &[u8],
    epoch_key: &[u8],
    signing_seed: &[u8],
) -> Result<Vec<u8>, JsError> {
    let header = batch::Header::decode(header_cbor).map_err(debug_error)?;
    Ok(batch::seal(
        &header,
        &[operation_cbor.to_vec()],
        &fixed(epoch_key, "epoch key")?,
        &fixed(signing_seed, "signing seed")?,
    )
    .map_err(debug_error)?
    .envelope_bytes)
}
