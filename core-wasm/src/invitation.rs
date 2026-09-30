use super::*;

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

    pub fn status_read_path(&self) -> String {
        let family: String = self
            .bootstrap
            .family_id()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        let invitation: String = self
            .bootstrap
            .invitation_id()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        format!("/v1/families/{family}/invitation-status/{invitation}")
    }

    pub fn sign_status_read(&self, request_id: &[u8]) -> Result<Vec<u8>, JsError> {
        self.bootstrap
            .sign_get_with_id(&self.status_read_path(), fixed(request_id, "request ID")?)
            .map_err(debug_error)
    }

    pub fn verify_status_reason(&self, response: &[u8]) -> Result<u8, JsError> {
        Ok(self
            .bootstrap
            .verify_status(response)
            .map_err(debug_error)?
            .reason)
    }

    pub fn candidate_uses_current_head(&self, candidate: &[u8]) -> Result<bool, JsError> {
        let Value::Map(root) = cbor::decode(candidate).map_err(debug_error)? else {
            return Err(JsError::new("candidate not map"));
        };
        let Value::Map(unsigned) = &root[0].1 else {
            return Err(JsError::new("candidate unsigned not map"));
        };
        let Value::Bytes(prior) = &unsigned[3].1 else {
            return Err(JsError::new("candidate prior head not bytes"));
        };
        Ok(fixed::<32>(prior, "candidate prior head")?
            == self
                .chain
                .as_ref()
                .ok_or_else(|| JsError::new("genesis absent"))?
                .head_hash())
    }

    /// Return an exact committed response only when verified sparse history
    /// contains this candidate. Empty means the candidate was not observed.
    pub fn candidate_result_in_history(&self, candidate: &[u8]) -> Result<Vec<u8>, JsError> {
        for committed in &self.controls {
            let Value::Map(root) = cbor::decode(committed).map_err(debug_error)? else {
                return Err(JsError::new("saved control not map"));
            };
            if root.len() != 4 {
                return Err(JsError::new("saved control width invalid"));
            }
            let prefix = cbor::encode(&Value::Map(vec![
                (1, root[0].1.clone()),
                (2, root[1].1.clone()),
            ]))
            .map_err(debug_error)?;
            if prefix == candidate {
                return cbor::encode(&Value::Map(vec![
                    (1, Value::Integer(1)),
                    (2, Value::Bytes(committed.clone())),
                ]))
                .map_err(debug_error);
            }
        }
        Ok(Vec::new())
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
