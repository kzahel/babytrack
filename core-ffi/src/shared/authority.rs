//! Manager authority preparation and confirmation.

use super::*;

#[uniffi::export]
impl NativeSharedStore {
    /// Persists the exact candidate, encrypted keys, and promotion chunks
    /// before the platform makes its first network request.
    pub fn prepare_share(
        &self,
        family: FamilyRef,
        relay_public_key: Vec<u8>,
        wrapping_key: Vec<u8>,
    ) -> Result<PreparedShareRow, BindingError> {
        let mut store = self.store.lock().map_err(|_| BindingError::LockPoisoned)?;
        let prepared = ManagerCreation::prepare(
            &mut store,
            family.handle()?,
            fixed(&relay_public_key)?,
            &fixed(&wrapping_key)?,
        )
        .map_err(rejected)?;
        Ok(PreparedShareRow {
            promotion_id: prepared.promotion_id().to_vec(),
            candidate_bytes: prepared.candidate_bytes().to_vec(),
            objects: prepared
                .stage_bodies()
                .map_err(rejected)?
                .into_iter()
                .map(|(object_id, body)| StagedObjectRow {
                    object_id: object_id.to_vec(),
                    body,
                })
                .collect(),
        })
    }

    /// A POST response alone cannot mark sharing complete: the core checks
    /// the committed relay signature and exact prepared candidate first.
    pub fn confirm_share(
        &self,
        family: FamilyRef,
        wrapping_key: Vec<u8>,
        commit_response: Vec<u8>,
    ) -> Result<u64, BindingError> {
        let mut store = self.store.lock().map_err(|_| BindingError::LockPoisoned)?;
        let prepared = ManagerCreation::resume(&store, family.handle()?, &fixed(&wrapping_key)?)
            .map_err(rejected)?;
        Ok(prepared
            .confirm(&mut store, &committed_control(&commit_response)?)
            .map_err(rejected)?
            .observed_cursor())
    }

    pub fn prepare_invite(
        &self,
        family: FamilyRef,
        wrapping_key: Vec<u8>,
        role: u8,
    ) -> Result<PreparedInviteRow, BindingError> {
        let mut store = self.store.lock().map_err(|_| BindingError::LockPoisoned)?;
        let creation = ManagerCreation::resume(&store, family.handle()?, &fixed(&wrapping_key)?)
            .map_err(rejected)?;
        let issue = FirstInviteIssue::prepare(&mut store, &creation, &fixed(&wrapping_key)?, role)
            .map_err(rejected)?;
        Ok(PreparedInviteRow {
            invitation_id: issue.invitation_id().to_vec(),
            candidate_bytes: issue.candidate_bytes().to_vec(),
            object: StagedObjectRow {
                object_id: issue.object_id().to_vec(),
                body: issue.stage_body().map_err(rejected)?,
            },
        })
    }

    /// Issue the first or a later invitation from verified authority state.
    pub fn prepare_next_invite(
        &self,
        family: FamilyRef,
        wrapping_key: Vec<u8>,
        role: u8,
    ) -> Result<PreparedInviteRow, BindingError> {
        let mut store = self.store.lock().map_err(|_| BindingError::LockPoisoned)?;
        let handle = family.handle()?;
        let key = fixed(&wrapping_key)?;
        if let Some(holder) = admitted_manager(&mut store, handle, &key)? {
            let issue =
                LaterInviteIssue::prepare_for_admitted_manager(&mut store, &holder, &key, role)
                    .map_err(rejected)?;
            return Ok(PreparedInviteRow {
                invitation_id: issue.invitation_id().to_vec(),
                candidate_bytes: issue.candidate_bytes().to_vec(),
                object: StagedObjectRow {
                    object_id: issue.object_id().to_vec(),
                    body: issue.stage_body().map_err(rejected)?,
                },
            });
        }
        let manager = ManagerCreation::resume(&store, family.handle()?, &fixed(&wrapping_key)?)
            .map_err(rejected)?;
        if has_issued_invitation(&store, family.handle()?)? {
            let issue = LaterInviteIssue::prepare_for_initial_manager(
                &mut store,
                &manager,
                &fixed(&wrapping_key)?,
                role,
            )
            .map_err(rejected)?;
            Ok(PreparedInviteRow {
                invitation_id: issue.invitation_id().to_vec(),
                candidate_bytes: issue.candidate_bytes().to_vec(),
                object: StagedObjectRow {
                    object_id: issue.object_id().to_vec(),
                    body: issue.stage_body().map_err(rejected)?,
                },
            })
        } else {
            let issue =
                FirstInviteIssue::prepare(&mut store, &manager, &fixed(&wrapping_key)?, role)
                    .map_err(rejected)?;
            Ok(PreparedInviteRow {
                invitation_id: issue.invitation_id().to_vec(),
                candidate_bytes: issue.candidate_bytes().to_vec(),
                object: StagedObjectRow {
                    object_id: issue.object_id().to_vec(),
                    body: issue.stage_body().map_err(rejected)?,
                },
            })
        }
    }

    pub fn confirm_next_invite(
        &self,
        family: FamilyRef,
        wrapping_key: Vec<u8>,
        invitation_id: Vec<u8>,
        commit_response: Vec<u8>,
        relay_origin: String,
    ) -> Result<String, BindingError> {
        let mut store = self.store.lock().map_err(|_| BindingError::LockPoisoned)?;
        let admitted =
            admitted_manager(&mut store, family.handle()?, &fixed(&wrapping_key)?)?.is_some();
        let committed = committed_control(&commit_response)?;
        if admitted || has_issued_invitation(&store, family.handle()?)? {
            LaterInviteIssue::resume(
                &store,
                family.handle()?,
                fixed(&invitation_id)?,
                &fixed(&wrapping_key)?,
            )
            .map_err(rejected)?
            .confirm(&mut store, &committed, &relay_origin)
            .map_err(rejected)?
            .to_fragment()
            .map_err(rejected)
        } else {
            let manager = ManagerCreation::resume(&store, family.handle()?, &fixed(&wrapping_key)?)
                .map_err(rejected)?;
            let issue = FirstInviteIssue::resume(&store, &manager, &fixed(&wrapping_key)?)
                .map_err(rejected)?;
            if issue.invitation_id() != fixed(&invitation_id)? {
                return Err(BindingError::InvalidBytes);
            }
            issue
                .confirm(&mut store, &manager, &committed, &relay_origin)
                .map_err(rejected)?
                .to_fragment()
                .map_err(rejected)
        }
    }

    /// The one-use link is available only after the signed issue commits.
    pub fn confirm_invite(
        &self,
        family: FamilyRef,
        wrapping_key: Vec<u8>,
        commit_response: Vec<u8>,
        relay_origin: String,
    ) -> Result<String, BindingError> {
        let mut store = self.store.lock().map_err(|_| BindingError::LockPoisoned)?;
        let creation = ManagerCreation::resume(&store, family.handle()?, &fixed(&wrapping_key)?)
            .map_err(rejected)?;
        let issue = FirstInviteIssue::resume(&store, &creation, &fixed(&wrapping_key)?)
            .map_err(rejected)?;
        issue
            .confirm(
                &mut store,
                &creation,
                &committed_control(&commit_response)?,
                &relay_origin,
            )
            .map_err(rejected)?
            .to_fragment()
            .map_err(rejected)
    }

    /// Public IDs of unused links visible to a current data-ready manager.
    pub fn unused_invitation_ids(
        &self,
        family: FamilyRef,
        wrapping_key: Vec<u8>,
    ) -> Result<Vec<Vec<u8>>, BindingError> {
        let mut store = self.store.lock().map_err(|_| BindingError::LockPoisoned)?;
        let handle = family.handle()?;
        let key = fixed(&wrapping_key)?;
        if admitted_manager(&mut store, handle, &key)?.is_none() {
            let manager = ManagerCreation::resume(&store, handle, &key).map_err(rejected)?;
            let ready = manager.ready_session(&store).map_err(rejected)?;
            let public = PublicHistorySession::resume(&store, handle).map_err(rejected)?;
            if ready.observed_cursor() != public.cursor()
                || ready.observed_head() != public.head_hash()
            {
                return Err(BindingError::Rejected(
                    "Manager view behind public authority".into(),
                ));
            }
        }
        let public = PublicHistorySession::resume(&store, handle).map_err(rejected)?;
        let Value::Map(state) =
            cbor::decode(&public.chain().state_bytes().map_err(rejected)?).map_err(rejected)?
        else {
            return Err(BindingError::InvalidBytes);
        };
        let Some((7, Value::Array(invitations))) = state.get(6) else {
            return Err(BindingError::InvalidBytes);
        };
        let mut unused = Vec::new();
        for row in invitations {
            let Value::Array(fields) = row else {
                return Err(BindingError::InvalidBytes);
            };
            if fields.len() != 6 {
                return Err(BindingError::InvalidBytes);
            }
            if fields[5] == Value::Integer(1) {
                let Value::Bytes(id) = &fields[0] else {
                    return Err(BindingError::InvalidBytes);
                };
                if id.len() != 16 {
                    return Err(BindingError::InvalidBytes);
                }
                unused.push(id.clone());
            }
        }
        Ok(unused)
    }

    /// Save exact cancellation bytes before HTTP POST.
    pub fn prepare_invite_cancel(
        &self,
        family: FamilyRef,
        wrapping_key: Vec<u8>,
        invitation_id: Vec<u8>,
    ) -> Result<PreparedCancelRow, BindingError> {
        let mut store = self.store.lock().map_err(|_| BindingError::LockPoisoned)?;
        let handle = family.handle()?;
        let key = fixed(&wrapping_key)?;
        let target = fixed(&invitation_id)?;
        let cancellation = if let Some(holder) = admitted_manager(&mut store, handle, &key)? {
            InviteCancellation::prepare_for_admitted_manager(&mut store, &holder, target)
                .map_err(rejected)?
        } else {
            let manager = ManagerCreation::resume(&store, handle, &key).map_err(rejected)?;
            InviteCancellation::prepare_for_initial_manager(&mut store, &manager, target)
                .map_err(rejected)?
        };
        Ok(PreparedCancelRow {
            invitation_id: cancellation.invitation_id().to_vec(),
            candidate_bytes: cancellation.candidate_bytes().to_vec(),
        })
    }

    pub fn confirm_invite_cancel(
        &self,
        family: FamilyRef,
        commit_response: Vec<u8>,
    ) -> Result<(), BindingError> {
        let mut store = self.store.lock().map_err(|_| BindingError::LockPoisoned)?;
        InviteCancellation::resume(&store, family.handle()?)
            .map_err(rejected)?
            .confirm(&mut store, &committed_control(&commit_response)?)
            .map_err(rejected)
    }

    /// Remove a verified keyless device from pending authority without an
    /// epoch rotation. Save the exact candidate before contacting the relay.
    pub fn prepare_pending_removal(
        &self,
        family: FamilyRef,
        wrapping_key: Vec<u8>,
        invitation_id: Vec<u8>,
        device_id: Vec<u8>,
    ) -> Result<PreparedPendingRemovalRow, BindingError> {
        let mut store = self.store.lock().map_err(|_| BindingError::LockPoisoned)?;
        let handle = family.handle()?;
        let key = fixed(&wrapping_key)?;
        let invitation = fixed(&invitation_id)?;
        let device = fixed(&device_id)?;
        let removal = if let Some(holder) = admitted_manager(&mut store, handle, &key)? {
            PendingRemoval::prepare_for_admitted_manager(&mut store, &holder, invitation, device)
                .map_err(rejected)?
        } else {
            let manager = ManagerCreation::resume(&store, handle, &key).map_err(rejected)?;
            PendingRemoval::prepare_for_initial_manager(&mut store, &manager, invitation, device)
                .map_err(rejected)?
        };
        Ok(PreparedPendingRemovalRow {
            invitation_id: removal.invitation_id().to_vec(),
            device_id: removal.device_id().to_vec(),
            candidate_bytes: removal.candidate_bytes().to_vec(),
        })
    }

    pub fn confirm_pending_removal(
        &self,
        family: FamilyRef,
        commit_response: Vec<u8>,
    ) -> Result<(), BindingError> {
        let mut store = self.store.lock().map_err(|_| BindingError::LockPoisoned)?;
        PendingRemoval::resume(&store, family.handle()?)
            .map_err(rejected)?
            .confirm(&mut store, &committed_control(&commit_response)?)
            .map_err(rejected)
    }

    /// Save exact manager role-change bytes before HTTP POST.
    pub fn prepare_role_change(
        &self,
        family: FamilyRef,
        wrapping_key: Vec<u8>,
        target_device_id: Vec<u8>,
        new_role: u8,
    ) -> Result<PreparedRoleChangeRow, BindingError> {
        let mut store = self.store.lock().map_err(|_| BindingError::LockPoisoned)?;
        let handle = family.handle()?;
        let key = fixed(&wrapping_key)?;
        let target = fixed(&target_device_id)?;
        let change = if let Some(holder) = admitted_manager(&mut store, handle, &key)? {
            RoleChange::prepare_for_admitted_manager(&mut store, &holder, target, new_role)
                .map_err(rejected)?
        } else {
            let manager = ManagerCreation::resume(&store, handle, &key).map_err(rejected)?;
            RoleChange::prepare_for_initial_manager(&mut store, &manager, target, new_role)
                .map_err(rejected)?
        };
        Ok(PreparedRoleChangeRow {
            target_device_id: change.target_id().to_vec(),
            new_role: change.new_role(),
            candidate_bytes: change.candidate_bytes().to_vec(),
        })
    }

    pub fn confirm_role_change(
        &self,
        family: FamilyRef,
        commit_response: Vec<u8>,
    ) -> Result<(), BindingError> {
        let mut store = self.store.lock().map_err(|_| BindingError::LockPoisoned)?;
        RoleChange::resume(&store, family.handle()?)
            .map_err(rejected)?
            .confirm(&mut store, &committed_control(&commit_response)?)
            .map_err(rejected)
    }

    pub fn manager_control_read(
        &self,
        family: FamilyRef,
        wrapping_key: Vec<u8>,
    ) -> Result<SignedReadRow, BindingError> {
        let mut store = self.store.lock().map_err(|_| BindingError::LockPoisoned)?;
        let holder = admitted_manager(&mut store, family.handle()?, &fixed(&wrapping_key)?)?;
        let after = PublicHistorySession::resume(&store, family.handle()?)
            .map_err(rejected)?
            .cursor();
        let path = format!(
            "/v1/families/{}/control?after={after}",
            lower_hex(&family.family_id)
        );
        let read = match holder {
            Some(holder) => holder.sign_get(&path).map_err(rejected)?,
            None => ManagerCreation::resume(&store, family.handle()?, &fixed(&wrapping_key)?)
                .map_err(rejected)?
                .sign_get(&path)
                .map_err(rejected)?,
        };
        Ok(SignedReadRow {
            path,
            auth: read.bytes,
            after,
        })
    }

    /// Accept verified controls before choosing the sole pending device
    /// from Rust authority state and preparing exact challenge bytes.
    pub fn prepare_first_challenge(
        &self,
        family: FamilyRef,
        wrapping_key: Vec<u8>,
        read: SignedReadRow,
        control_page: Vec<u8>,
    ) -> Result<PreparedChallengeRow, BindingError> {
        let mut store = self.store.lock().map_err(|_| BindingError::LockPoisoned)?;
        let mut public =
            PublicHistorySession::resume(&store, family.handle()?).map_err(rejected)?;
        if read.after != public.cursor()
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
            public
                .accept_control(&mut store, &entry.committed_bytes)
                .map_err(rejected)?;
        }
        let target = verified_join_target(&store, family.handle()?).map_err(rejected)?;
        let challenge = if let Some(holder) =
            admitted_manager(&mut store, family.handle()?, &fixed(&wrapping_key)?)?
        {
            match target {
                Some(target) if target.action == 1 => FirstChallenge::prepare_for_admitted_manager(
                    &mut store,
                    &holder,
                    target.invitation_id,
                    target.device_id,
                    &fixed(&wrapping_key)?,
                )
                .map_err(rejected)?,
                _ => FirstChallenge::resume_for_admitted_manager(
                    &store,
                    &holder,
                    &fixed(&wrapping_key)?,
                )
                .map_err(rejected)?,
            }
        } else {
            let manager = ManagerCreation::resume(&store, family.handle()?, &fixed(&wrapping_key)?)
                .map_err(rejected)?;
            match target {
                Some(target) if target.action == 1 => {
                    FirstChallenge::prepare_later_for_initial_manager(
                        &mut store,
                        &manager,
                        target.invitation_id,
                        target.device_id,
                        &fixed(&wrapping_key)?,
                    )
                    .map_err(rejected)?
                }
                _ => FirstChallenge::resume(&store, &manager, &fixed(&wrapping_key)?)
                    .map_err(rejected)?,
            }
        };
        Ok(PreparedChallengeRow {
            candidate_bytes: challenge.candidate_bytes().to_vec(),
            objects: challenge
                .stage_bodies()
                .map_err(rejected)?
                .into_iter()
                .map(|(object_id, body)| StagedObjectRow {
                    object_id: object_id.to_vec(),
                    body,
                })
                .collect(),
        })
    }

    pub fn confirm_first_challenge(
        &self,
        family: FamilyRef,
        wrapping_key: Vec<u8>,
        commit_response: Vec<u8>,
    ) -> Result<(), BindingError> {
        let mut store = self.store.lock().map_err(|_| BindingError::LockPoisoned)?;
        let challenge = if let Some(holder) =
            admitted_manager(&mut store, family.handle()?, &fixed(&wrapping_key)?)?
        {
            FirstChallenge::resume_for_admitted_manager(&store, &holder, &fixed(&wrapping_key)?)
                .map_err(rejected)?
        } else {
            let manager = ManagerCreation::resume(&store, family.handle()?, &fixed(&wrapping_key)?)
                .map_err(rejected)?;
            FirstChallenge::resume(&store, &manager, &fixed(&wrapping_key)?).map_err(rejected)?
        };
        challenge
            .confirm(&mut store, &committed_control(&commit_response)?)
            .map_err(rejected)
    }

    pub fn prepare_first_admission(
        &self,
        family: FamilyRef,
        wrapping_key: Vec<u8>,
        read: SignedReadRow,
        control_page: Vec<u8>,
    ) -> Result<PreparedAdmissionRow, BindingError> {
        let mut store = self.store.lock().map_err(|_| BindingError::LockPoisoned)?;
        let mut public =
            PublicHistorySession::resume(&store, family.handle()?).map_err(rejected)?;
        if read.after != public.cursor()
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
            public
                .accept_control(&mut store, &entry.committed_bytes)
                .map_err(rejected)?;
        }
        let target = verified_join_target(&store, family.handle()?).map_err(rejected)?;
        let admission = if let Some(holder) =
            admitted_manager(&mut store, family.handle()?, &fixed(&wrapping_key)?)?
        {
            match target {
                Some(target) if target.action == 2 => FirstAdmission::prepare_for_admitted_manager(
                    &mut store,
                    &holder,
                    target.invitation_id,
                    target.device_id,
                    &fixed(&wrapping_key)?,
                )
                .map_err(rejected)?,
                _ => FirstAdmission::resume_for_admitted_manager(
                    &store,
                    &holder,
                    &fixed(&wrapping_key)?,
                )
                .map_err(rejected)?,
            }
        } else {
            let manager = ManagerCreation::resume(&store, family.handle()?, &fixed(&wrapping_key)?)
                .map_err(rejected)?;
            match target {
                Some(target) if target.action == 2 => FirstAdmission::prepare(
                    &mut store,
                    &manager,
                    target.invitation_id,
                    target.device_id,
                    &fixed(&wrapping_key)?,
                )
                .map_err(rejected)?,
                _ => FirstAdmission::resume(&store, &manager, &fixed(&wrapping_key)?)
                    .map_err(rejected)?,
            }
        };
        Ok(PreparedAdmissionRow {
            candidate_bytes: admission.candidate_bytes().to_vec(),
            objects: admission
                .stage_bodies()
                .map_err(rejected)?
                .into_iter()
                .map(|(object_id, body)| StagedObjectRow {
                    object_id: object_id.to_vec(),
                    body,
                })
                .collect(),
        })
    }

    pub fn confirm_first_admission(
        &self,
        family: FamilyRef,
        wrapping_key: Vec<u8>,
        commit_response: Vec<u8>,
    ) -> Result<(), BindingError> {
        let mut store = self.store.lock().map_err(|_| BindingError::LockPoisoned)?;
        let committed = committed_control(&commit_response)?;
        if let Some(holder) =
            admitted_manager(&mut store, family.handle()?, &fixed(&wrapping_key)?)?
        {
            FirstAdmission::resume_for_admitted_manager(&store, &holder, &fixed(&wrapping_key)?)
                .map_err(rejected)?
                .confirm_for_admitted_manager(&mut store, &committed)
                .map_err(rejected)
        } else {
            let manager = ManagerCreation::resume(&store, family.handle()?, &fixed(&wrapping_key)?)
                .map_err(rejected)?;
            FirstAdmission::resume(&store, &manager, &fixed(&wrapping_key)?)
                .map_err(rejected)?
                .confirm(&mut store, &manager, &committed)
                .map_err(rejected)
        }
    }

    /// Prepare exact durable removal bytes after a verified current sync.
    /// The target is one device credential, never a person-wide account.
    pub fn prepare_first_removal(
        &self,
        family: FamilyRef,
        wrapping_key: Vec<u8>,
        target_device_id: Vec<u8>,
    ) -> Result<PreparedRemovalRow, BindingError> {
        let mut store = self.store.lock().map_err(|_| BindingError::LockPoisoned)?;
        let removal = if let Some(holder) =
            admitted_manager(&mut store, family.handle()?, &fixed(&wrapping_key)?)?
        {
            FirstRemoval::prepare_for_admitted_manager(
                &mut store,
                &holder,
                &fixed(&wrapping_key)?,
                fixed(&target_device_id)?,
            )
            .map_err(rejected)?
        } else {
            let manager = ManagerCreation::resume(&store, family.handle()?, &fixed(&wrapping_key)?)
                .map_err(rejected)?;
            FirstRemoval::prepare(
                &mut store,
                &manager,
                &fixed(&wrapping_key)?,
                fixed(&target_device_id)?,
            )
            .map_err(rejected)?
        };
        Ok(PreparedRemovalRow {
            candidate_bytes: removal.candidate_bytes().to_vec(),
            objects: removal
                .stage_bodies()
                .map_err(rejected)?
                .into_iter()
                .map(|(object_id, body)| StagedObjectRow {
                    object_id: object_id.to_vec(),
                    body,
                })
                .collect(),
        })
    }

    pub fn confirm_first_removal(
        &self,
        family: FamilyRef,
        wrapping_key: Vec<u8>,
        commit_response: Vec<u8>,
    ) -> Result<(), BindingError> {
        let mut store = self.store.lock().map_err(|_| BindingError::LockPoisoned)?;
        let committed = committed_control(&commit_response)?;
        if let Some(holder) =
            admitted_manager(&mut store, family.handle()?, &fixed(&wrapping_key)?)?
        {
            FirstRemoval::resume_for_admitted_manager(&store, &holder, &fixed(&wrapping_key)?)
                .map_err(rejected)?
                .confirm_for_admitted_manager(&mut store, &holder, &committed)
                .map_err(rejected)
        } else {
            let manager = ManagerCreation::resume(&store, family.handle()?, &fixed(&wrapping_key)?)
                .map_err(rejected)?;
            FirstRemoval::resume(&store, &manager, &fixed(&wrapping_key)?)
                .map_err(rejected)?
                .confirm(&mut store, &manager, &committed)
                .map_err(rejected)
        }
    }

    pub fn manager_first_join_action(
        &self,
        family: FamilyRef,
        wrapping_key: Vec<u8>,
    ) -> Result<u8, BindingError> {
        let mut store = self.store.lock().map_err(|_| BindingError::LockPoisoned)?;
        if admitted_manager(&mut store, family.handle()?, &fixed(&wrapping_key)?)?.is_none() {
            ManagerCreation::resume(&store, family.handle()?, &fixed(&wrapping_key)?)
                .map_err(rejected)?;
        }
        Ok(verified_join_target(&store, family.handle()?)
            .map_err(rejected)?
            .map_or(0, |target| target.action))
    }

    pub fn is_admitted_manager(&self, family: FamilyRef) -> Result<bool, BindingError> {
        let store = self.store.lock().map_err(|_| BindingError::LockPoisoned)?;
        let handle = family.handle()?;
        if !store
            .has_enrollment_attempt(handle.family_id)
            .map_err(rejected)?
        {
            return Ok(false);
        }
        let public = PublicHistorySession::resume(&store, handle).map_err(rejected)?;
        Ok(public
            .chain()
            .active_devices()
            .map_err(rejected)?
            .iter()
            .any(|device| device.device_id == handle.device_id && device.role == 2))
    }
}
