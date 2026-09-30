//! Saved recipient enrollment and invitation reconciliation.

use super::*;

#[uniffi::export]
impl NativeSharedStore {
    /// Retry the exact saved claim after invitation read access closes.
    pub fn resume_join(
        &self,
        fragment: String,
        wrapping_key: Vec<u8>,
    ) -> Result<Option<PreparedJoinRow>, BindingError> {
        let bootstrap = InvitationBootstrap::from_fragment(&fragment).map_err(rejected)?;
        let mut store = self.store.lock().map_err(|_| BindingError::LockPoisoned)?;
        Ok(
            EnrollmentAttempt::resume_for_invitation(
                &mut store,
                &bootstrap,
                &fixed(&wrapping_key)?,
            )
            .map_err(rejected)?
            .map(|attempt| PreparedJoinRow {
                family: attempt.family().into(),
                candidate_bytes: attempt.claim_candidate().to_vec(),
            }),
        )
    }

    /// Recover the exact encrypted-in-store claim after process restart,
    /// without requiring the bearer invitation fragment in UI memory.
    pub fn resume_join_family(
        &self,
        family: FamilyRef,
        wrapping_key: Vec<u8>,
    ) -> Result<PreparedJoinRow, BindingError> {
        let mut store = self.store.lock().map_err(|_| BindingError::LockPoisoned)?;
        let attempt = EnrollmentAttempt::resume(
            &mut store,
            family.handle()?.family_id,
            &fixed(&wrapping_key)?,
        )
        .map_err(rejected)?;
        if attempt.family() != family.handle()? {
            return Err(BindingError::InvalidBytes);
        }
        Ok(PreparedJoinRow {
            family,
            candidate_bytes: attempt.claim_candidate().to_vec(),
        })
    }

    /// Verify the invitation-linked public controls before generating or
    /// storing this installation's recipient credentials.
    pub fn prepare_join(
        &self,
        fragment: String,
        control_page: Vec<u8>,
        wrapping_key: Vec<u8>,
    ) -> Result<PreparedJoinRow, BindingError> {
        let bootstrap = InvitationBootstrap::from_fragment(&fragment).map_err(rejected)?;
        let page =
            ControlPage::decode(&control_page, bootstrap.family_id(), 0).map_err(rejected)?;
        if page.entries.len() < 2 || page.entries[0].cursor != 1 {
            return Err(BindingError::InvalidBytes);
        }
        let mut store = self.store.lock().map_err(|_| BindingError::LockPoisoned)?;
        let attempt = EnrollmentAttempt::prepare_sparse(
            &mut store,
            &bootstrap,
            &page.entries[0].committed_bytes,
            &page.entries[1].committed_bytes,
            &fixed(&wrapping_key)?,
        )
        .map_err(rejected)?;
        Ok(PreparedJoinRow {
            family: attempt.family().into(),
            candidate_bytes: attempt.claim_candidate().to_vec(),
        })
    }

    /// Decode every bounded relay page, then require the invitation's exact
    /// issue and all preceding signed controls before saving a claim.
    pub fn prepare_join_pages(
        &self,
        fragment: String,
        control_pages: Vec<Vec<u8>>,
        wrapping_key: Vec<u8>,
    ) -> Result<PreparedJoinRow, BindingError> {
        let bootstrap = InvitationBootstrap::from_fragment(&fragment).map_err(rejected)?;
        let controls = decode_join_control_pages(bootstrap.family_id(), &control_pages)?;
        let mut store = self.store.lock().map_err(|_| BindingError::LockPoisoned)?;
        let attempt = EnrollmentAttempt::prepare_sparse_prefix(
            &mut store,
            &bootstrap,
            &controls,
            &fixed(&wrapping_key)?,
        )
        .map_err(rejected)?;
        Ok(PreparedJoinRow {
            family: attempt.family().into(),
            candidate_bytes: attempt.claim_candidate().to_vec(),
        })
    }

    pub fn saved_join_control_read(
        &self,
        family: FamilyRef,
        wrapping_key: Vec<u8>,
        after: u64,
        pending_credential: bool,
    ) -> Result<SignedReadRow, BindingError> {
        let mut store = self.store.lock().map_err(|_| BindingError::LockPoisoned)?;
        let attempt = EnrollmentAttempt::resume(
            &mut store,
            family.handle()?.family_id,
            &fixed(&wrapping_key)?,
        )
        .map_err(rejected)?;
        if attempt.family() != family.handle()? {
            return Err(BindingError::InvalidBytes);
        }
        let path = format!(
            "/v1/families/{}/control?after={after}",
            lower_hex(&family.family_id)
        );
        let auth = if pending_credential {
            attempt.sign_get(&path).map_err(rejected)?.bytes
        } else {
            InvitationBootstrap::from_fragment(attempt.invitation_fragment())
                .map_err(rejected)?
                .sign_get(&path)
                .map_err(rejected)?
                .bytes
        };
        Ok(SignedReadRow { path, auth, after })
    }

    pub fn saved_invitation_status_read(
        &self,
        family: FamilyRef,
        wrapping_key: Vec<u8>,
    ) -> Result<SignedReadRow, BindingError> {
        let mut store = self.store.lock().map_err(|_| BindingError::LockPoisoned)?;
        let attempt = EnrollmentAttempt::resume(
            &mut store,
            family.handle()?.family_id,
            &fixed(&wrapping_key)?,
        )
        .map_err(rejected)?;
        if attempt.family() != family.handle()? {
            return Err(BindingError::InvalidBytes);
        }
        invitation_status_read(attempt.invitation_fragment().to_owned())
    }

    pub fn verify_saved_invitation_status(
        &self,
        family: FamilyRef,
        wrapping_key: Vec<u8>,
        response: Vec<u8>,
    ) -> Result<InvitationStatusRow, BindingError> {
        let mut store = self.store.lock().map_err(|_| BindingError::LockPoisoned)?;
        let attempt = EnrollmentAttempt::resume(
            &mut store,
            family.handle()?.family_id,
            &fixed(&wrapping_key)?,
        )
        .map_err(rejected)?;
        if attempt.family() != family.handle()? {
            return Err(BindingError::InvalidBytes);
        }
        verify_invitation_status(attempt.invitation_fragment().to_owned(), response)
    }

    pub fn record_join_terminal_status(
        &self,
        family: FamilyRef,
        wrapping_key: Vec<u8>,
        response: Vec<u8>,
    ) -> Result<InvitationStatusRow, BindingError> {
        let mut store = self.store.lock().map_err(|_| BindingError::LockPoisoned)?;
        let attempt = EnrollmentAttempt::resume(
            &mut store,
            family.handle()?.family_id,
            &fixed(&wrapping_key)?,
        )
        .map_err(rejected)?;
        if attempt.family() != family.handle()? {
            return Err(BindingError::InvalidBytes);
        }
        let status = attempt
            .record_terminal_status(&mut store, &response)
            .map_err(rejected)?;
        Ok(InvitationStatusRow {
            reason: status.reason,
            cursor: status.cursor,
            observed_ms: status.observed_ms,
        })
    }

    pub fn saved_join_terminal_status(
        &self,
        family: FamilyRef,
        wrapping_key: Vec<u8>,
    ) -> Result<Option<InvitationStatusRow>, BindingError> {
        let mut store = self.store.lock().map_err(|_| BindingError::LockPoisoned)?;
        let attempt = EnrollmentAttempt::resume(
            &mut store,
            family.handle()?.family_id,
            &fixed(&wrapping_key)?,
        )
        .map_err(rejected)?;
        if attempt.family() != family.handle()? {
            return Err(BindingError::InvalidBytes);
        }
        Ok(attempt
            .saved_terminal_status(&store)
            .map_err(rejected)?
            .map(|status| InvitationStatusRow {
                reason: status.reason,
                cursor: status.cursor,
                observed_ms: status.observed_ms,
            }))
    }

    pub fn refresh_join_pages(
        &self,
        family: FamilyRef,
        wrapping_key: Vec<u8>,
        control_pages: Vec<Vec<u8>>,
    ) -> Result<PreparedJoinRow, BindingError> {
        let mut store = self.store.lock().map_err(|_| BindingError::LockPoisoned)?;
        let attempt = EnrollmentAttempt::resume(
            &mut store,
            family.handle()?.family_id,
            &fixed(&wrapping_key)?,
        )
        .map_err(rejected)?;
        if attempt.family() != family.handle()? {
            return Err(BindingError::InvalidBytes);
        }
        let bootstrap =
            InvitationBootstrap::from_fragment(attempt.invitation_fragment()).map_err(rejected)?;
        let controls = decode_join_control_pages(bootstrap.family_id(), &control_pages)?;
        let refreshed = EnrollmentAttempt::refresh_sparse_prefix(
            &mut store,
            &bootstrap,
            &controls,
            &fixed(&wrapping_key)?,
        )
        .map_err(rejected)?;
        Ok(PreparedJoinRow {
            family,
            candidate_bytes: refreshed.claim_candidate().to_vec(),
        })
    }

    /// A successful HTTP status is insufficient: bind the signed relay
    /// commit to the exact durable claim before recording pending progress.
    pub fn confirm_join_claim(
        &self,
        family: FamilyRef,
        wrapping_key: Vec<u8>,
        commit_response: Vec<u8>,
    ) -> Result<(), BindingError> {
        let mut store = self.store.lock().map_err(|_| BindingError::LockPoisoned)?;
        let attempt = EnrollmentAttempt::resume(
            &mut store,
            family.handle()?.family_id,
            &fixed(&wrapping_key)?,
        )
        .map_err(rejected)?;
        if attempt.family() != family.handle()? {
            return Err(BindingError::InvalidBytes);
        }
        attempt
            .confirm_sparse_claim(&mut store, &committed_control(&commit_response)?)
            .map_err(rejected)
    }

    pub fn recipient_control_read(
        &self,
        family: FamilyRef,
        wrapping_key: Vec<u8>,
    ) -> Result<SignedReadRow, BindingError> {
        let mut store = self.store.lock().map_err(|_| BindingError::LockPoisoned)?;
        let attempt = EnrollmentAttempt::resume(
            &mut store,
            family.handle()?.family_id,
            &fixed(&wrapping_key)?,
        )
        .map_err(rejected)?;
        if attempt.family() != family.handle()? {
            return Err(BindingError::InvalidBytes);
        }
        let after = attempt.pending_control_cursor(&store).map_err(rejected)?;
        let path = format!(
            "/v1/families/{}/control?after={after}",
            lower_hex(&family.family_id)
        );
        let auth = attempt.sign_get(&path).map_err(rejected)?.bytes;
        Ok(SignedReadRow { path, auth, after })
    }

    pub fn recipient_challenge_read(
        &self,
        family: FamilyRef,
        wrapping_key: Vec<u8>,
        read: SignedReadRow,
        control_page: Vec<u8>,
    ) -> Result<ChallengeReadRow, BindingError> {
        let mut store = self.store.lock().map_err(|_| BindingError::LockPoisoned)?;
        let attempt = EnrollmentAttempt::resume(
            &mut store,
            family.handle()?.family_id,
            &fixed(&wrapping_key)?,
        )
        .map_err(rejected)?;
        if attempt.family() != family.handle()?
            || read.after != attempt.pending_control_cursor(&store).map_err(rejected)?
            || read.path
                != format!(
                    "/v1/families/{}/control?after={}",
                    lower_hex(&family.family_id),
                    read.after
                )
        {
            return Err(BindingError::InvalidBytes);
        }
        let page = ControlPage::decode(&control_page, family.handle()?.family_id, read.after)
            .map_err(rejected)?;
        for entry in page.entries {
            attempt
                .accept_sparse_control(&mut store, &entry.committed_bytes)
                .map_err(rejected)?;
        }
        let object_id = attempt
            .pending_challenge_object_id(&store)
            .map_err(rejected)?;
        let path = format!(
            "/v1/families/{}/objects/{}",
            lower_hex(&family.family_id),
            lower_hex(&object_id)
        );
        let auth = attempt.sign_get(&path).map_err(rejected)?.bytes;
        Ok(ChallengeReadRow {
            path,
            auth,
            object_id: object_id.to_vec(),
        })
    }

    pub fn prepare_first_proof(
        &self,
        family: FamilyRef,
        wrapping_key: Vec<u8>,
        challenge_read: ChallengeReadRow,
        object_response: Vec<u8>,
    ) -> Result<Vec<u8>, BindingError> {
        let mut store = self.store.lock().map_err(|_| BindingError::LockPoisoned)?;
        let attempt = EnrollmentAttempt::resume(
            &mut store,
            family.handle()?.family_id,
            &fixed(&wrapping_key)?,
        )
        .map_err(rejected)?;
        if attempt.family() != family.handle()? {
            return Err(BindingError::InvalidBytes);
        }
        let object_id = fixed::<16>(&challenge_read.object_id)?;
        let expected_path = format!(
            "/v1/families/{}/objects/{}",
            lower_hex(&family.family_id),
            lower_hex(&object_id)
        );
        if challenge_read.path != expected_path {
            return Err(BindingError::InvalidBytes);
        }
        let object = OpaqueObject::decode(&object_response, object_id).map_err(rejected)?;
        if object.kind != 2 {
            return Err(BindingError::InvalidBytes);
        }
        attempt
            .accept_sparse_object(&mut store, object_id, &object.object_bytes)
            .map_err(rejected)?;
        Ok(
            FirstProof::prepare(&mut store, &attempt, &fixed(&wrapping_key)?)
                .map_err(rejected)?
                .candidate_bytes()
                .to_vec(),
        )
    }

    pub fn saved_first_proof(
        &self,
        family: FamilyRef,
        wrapping_key: Vec<u8>,
    ) -> Result<Option<Vec<u8>>, BindingError> {
        let mut store = self.store.lock().map_err(|_| BindingError::LockPoisoned)?;
        let attempt = EnrollmentAttempt::resume(
            &mut store,
            family.handle()?.family_id,
            &fixed(&wrapping_key)?,
        )
        .map_err(rejected)?;
        if attempt.family() != family.handle()? {
            return Err(BindingError::InvalidBytes);
        }
        Ok(
            FirstProof::resume_optional(&store, &attempt, &fixed(&wrapping_key)?)
                .map_err(rejected)?
                .map(|proof| proof.candidate_bytes().to_vec()),
        )
    }

    pub fn confirm_first_proof(
        &self,
        family: FamilyRef,
        wrapping_key: Vec<u8>,
        commit_response: Vec<u8>,
    ) -> Result<(), BindingError> {
        let mut store = self.store.lock().map_err(|_| BindingError::LockPoisoned)?;
        let attempt = EnrollmentAttempt::resume(
            &mut store,
            family.handle()?.family_id,
            &fixed(&wrapping_key)?,
        )
        .map_err(rejected)?;
        if attempt.family() != family.handle()? {
            return Err(BindingError::InvalidBytes);
        }
        FirstProof::resume(&store, &attempt, &fixed(&wrapping_key)?)
            .map_err(rejected)?
            .confirm(&mut store, &committed_control(&commit_response)?)
            .map_err(rejected)
    }

    pub fn recipient_families(&self) -> Result<Vec<FamilyRef>, BindingError> {
        let store = self.store.lock().map_err(|_| BindingError::LockPoisoned)?;
        store
            .families()
            .map_err(rejected)?
            .into_iter()
            .filter_map(
                |family| match store.has_enrollment_attempt(family.family_id) {
                    Ok(true) => Some(Ok(family.into())),
                    Ok(false) => None,
                    Err(error) => Some(Err(rejected(error))),
                },
            )
            .collect::<Result<Vec<_>, _>>()
    }

    pub fn recipient_relay_origin(
        &self,
        family: FamilyRef,
        wrapping_key: Vec<u8>,
    ) -> Result<String, BindingError> {
        let mut store = self.store.lock().map_err(|_| BindingError::LockPoisoned)?;
        let attempt = EnrollmentAttempt::resume(
            &mut store,
            family.handle()?.family_id,
            &fixed(&wrapping_key)?,
        )
        .map_err(rejected)?;
        if attempt.family() != family.handle()? {
            return Err(BindingError::InvalidBytes);
        }
        attempt.relay_origin().map_err(rejected)
    }

    pub fn recipient_first_join_action(
        &self,
        family: FamilyRef,
        wrapping_key: Vec<u8>,
    ) -> Result<u8, BindingError> {
        let mut store = self.store.lock().map_err(|_| BindingError::LockPoisoned)?;
        let attempt = EnrollmentAttempt::resume(
            &mut store,
            family.handle()?.family_id,
            &fixed(&wrapping_key)?,
        )
        .map_err(rejected)?;
        if attempt.family() != family.handle()? {
            return Err(BindingError::InvalidBytes);
        }
        attempt.first_join_action(&store).map_err(rejected)
    }
}
