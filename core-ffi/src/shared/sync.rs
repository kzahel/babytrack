//! Verified shared reads, sync, and recovery adapters.

use super::*;

#[uniffi::export]
impl NativeSharedStore {
    /// Ask for public controls before data reads, so a revoked credential
    /// can verify its removal even when the relay denies new ciphertext.
    pub fn check_recipient_removal(
        &self,
        family: FamilyRef,
        wrapping_key: Vec<u8>,
        now_ms: i64,
        transport: Box<dyn RelayReadTransport>,
    ) -> Result<Option<RemovedDeviceRow>, BindingError> {
        let mut store = self.store.lock().map_err(|_| BindingError::LockPoisoned)?;
        let handle = family.handle()?;
        let wrapping = fixed(&wrapping_key)?;
        let saved = if let Some(saved) = store.saved_removal(handle).map_err(rejected)? {
            Some(saved)
        } else {
            let attempt = EnrollmentAttempt::resume(&mut store, handle.family_id, &wrapping)
                .map_err(rejected)?;
            if attempt.family() != handle
                || !attempt.has_committed_admission(&store).map_err(rejected)?
            {
                return Ok(None);
            }
            let public = PublicHistorySession::resume(&store, handle).map_err(rejected)?;
            let path = format!(
                "/v1/families/{}/control?after={}",
                lower_hex(&handle.family_id),
                public.cursor()
            );
            let auth = attempt.sign_get(&path).map_err(rejected)?.bytes;
            let page = transport.get(path, auth)?;
            public
                .save_removed_control_page(&mut store, &page)
                .map_err(rejected)?
        };
        let Some(saved) = saved else { return Ok(None) };
        let public = PublicHistorySession::resume(&store, handle).map_err(rejected)?;
        let pending_result =
            if let Some(batch_id) = public.pending_batch_id(&store).map_err(rejected)? {
                let attempt = EnrollmentAttempt::resume(&mut store, handle.family_id, &wrapping)
                    .map_err(rejected)?;
                let path = format!(
                    "/v1/families/{}/batch-results/{}",
                    lower_hex(&handle.family_id),
                    lower_hex(&batch_id),
                );
                let auth = attempt.sign_get(&path).map_err(rejected)?.bytes;
                match transport.get(path, auth) {
                    Ok(bytes) => public
                        .inspect_removed_pending_result(&store, &saved, &bytes)
                        .unwrap_or(1),
                    Err(_) => 1,
                }
            } else {
                0
            };
        let copy = if let Some(existing) = store
            .removal_copy_of(handle, saved.transition_id)
            .map_err(rejected)?
        {
            Some(existing)
        } else {
            let ready = ready_session_for(&mut store, handle, &wrapping)?;
            if ready.has_unsent_local(&store).map_err(rejected)? {
                Some(
                    babytrack_core::portable_file::private_copy_after_removal(
                        &mut store, &ready, &saved, now_ms,
                    )
                    .map_err(rejected)?,
                )
            } else {
                None
            }
        };
        Ok(Some(RemovedDeviceRow {
            verified_cursor: saved.cursor,
            known_gap: saved.known_gap,
            private_copy: copy.map(Into::into),
            pending_result,
        }))
    }

    /// The original manager is also an ordinary device credential after
    /// other managers join. Verify its signed removal before any data pull.
    pub fn check_initial_manager_removal(
        &self,
        family: FamilyRef,
        wrapping_key: Vec<u8>,
        now_ms: i64,
        transport: Box<dyn RelayReadTransport>,
    ) -> Result<Option<RemovedDeviceRow>, BindingError> {
        let mut store = self.store.lock().map_err(|_| BindingError::LockPoisoned)?;
        let handle = family.handle()?;
        let wrapping = fixed(&wrapping_key)?;
        let manager = ManagerCreation::resume(&store, handle, &wrapping).map_err(rejected)?;
        let saved = if let Some(saved) = store.saved_removal(handle).map_err(rejected)? {
            Some(saved)
        } else {
            let public = PublicHistorySession::resume(&store, handle).map_err(rejected)?;
            let path = format!(
                "/v1/families/{}/control?after={}",
                lower_hex(&handle.family_id),
                public.cursor(),
            );
            let auth = manager.sign_get(&path).map_err(rejected)?.bytes;
            let page = transport.get(path, auth)?;
            public
                .save_removed_control_page(&mut store, &page)
                .map_err(rejected)?
        };
        let Some(saved) = saved else { return Ok(None) };
        let public = PublicHistorySession::resume(&store, handle).map_err(rejected)?;
        let pending_result =
            if let Some(batch_id) = public.pending_batch_id(&store).map_err(rejected)? {
                let path = format!(
                    "/v1/families/{}/batch-results/{}",
                    lower_hex(&handle.family_id),
                    lower_hex(&batch_id),
                );
                let auth = manager.sign_get(&path).map_err(rejected)?.bytes;
                match transport.get(path, auth) {
                    Ok(bytes) => public
                        .inspect_removed_pending_result(&store, &saved, &bytes)
                        .unwrap_or(1),
                    Err(_) => 1,
                }
            } else {
                0
            };
        let copy = if let Some(existing) = store
            .removal_copy_of(handle, saved.transition_id)
            .map_err(rejected)?
        {
            Some(existing)
        } else {
            let ready = manager.ready_session(&store).map_err(rejected)?;
            if ready.has_unsent_local(&store).map_err(rejected)? {
                Some(
                    babytrack_core::portable_file::private_copy_after_removal(
                        &mut store, &ready, &saved, now_ms,
                    )
                    .map_err(rejected)?,
                )
            } else {
                None
            }
        };
        Ok(Some(RemovedDeviceRow {
            verified_cursor: saved.cursor,
            known_gap: saved.known_gap,
            private_copy: copy.map(Into::into),
            pending_result,
        }))
    }

    /// One bounded sync pass. The platform fetches bytes; Rust signs every
    /// exact path, verifies the full log and manifests, and decides readiness.
    pub fn sync_recipient(
        &self,
        family: FamilyRef,
        wrapping_key: Vec<u8>,
        transport: Box<dyn RelayReadTransport>,
    ) -> Result<RecipientSyncRow, BindingError> {
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
        let attempt = Arc::new(attempt);
        let transport: Arc<dyn RelayReadTransport> = Arc::from(transport);
        let after = attempt.pending_control_cursor(&store).map_err(rejected)?;
        let path = format!(
            "/v1/families/{}/control?after={after}",
            lower_hex(&family.family_id)
        );
        let auth = attempt.sign_get(&path).map_err(rejected)?.bytes;
        let page_bytes = transport.get(path, auth)?;
        let page = ControlPage::decode(&page_bytes, family.handle()?.family_id, after)
            .map_err(rejected)?;
        for entry in page.entries {
            attempt
                .accept_sparse_control(&mut store, &entry.committed_bytes)
                .map_err(rejected)?;
        }
        let pending_control_cursor = attempt.pending_control_cursor(&store).map_err(rejected)?;
        let pending_phase = attempt.pending_phase(&store).map_err(rejected)?;
        if !attempt.has_committed_admission(&store).map_err(rejected)? {
            return Ok(RecipientSyncRow {
                verified_cursor: PublicHistorySession::resume(&store, family.handle()?)
                    .map_err(rejected)?
                    .cursor(),
                pending_control_cursor,
                awaiting_grant: pending_phase != 8,
                join_phase: pending_phase,
                no_more_visible: false,
                remaining_objects: pending_phase != 8,
                ready: false,
                child_count: 0,
                removed: false,
                private_copy: None,
                pending_result: 0,
            });
        }
        let active_pull::SyncProgress { pull, hydration } = futures::executor::block_on(
            active_pull::pull_and_hydrate(&mut store, family.handle()?, 4, 16, |path| {
                let attempt = Arc::clone(&attempt);
                let transport = Arc::clone(&transport);
                async move {
                    let auth = attempt.sign_get(&path).map_err(rejected)?.bytes;
                    transport.get(path, auth)
                }
            }),
        )
        .map_err(rejected)?;
        if !pull.no_more_visible || hydration.remaining {
            return Ok(RecipientSyncRow {
                verified_cursor: pull.verified_cursor,
                pending_control_cursor,
                awaiting_grant: false,
                join_phase: 5,
                no_more_visible: pull.no_more_visible,
                remaining_objects: hydration.remaining,
                ready: false,
                child_count: 0,
                removed: false,
                private_copy: None,
                pending_result: 0,
            });
        }
        let ready = ReadyFamilySession::from_enrollment(&store, &attempt).map_err(rejected)?;
        let children = ready
            .projection()
            .records()
            .filter(|record| {
                record.scope == babytrack_core::operation::Scope::Child && !record.deleted
            })
            .count();
        Ok(RecipientSyncRow {
            verified_cursor: pull.verified_cursor,
            pending_control_cursor,
            awaiting_grant: false,
            join_phase: 6,
            no_more_visible: true,
            remaining_objects: false,
            ready: true,
            child_count: children as u64,
            removed: false,
            private_copy: None,
            pending_result: 0,
        })
    }

    /// A usable view requires a locally held key and complete verified
    /// history. Include durable offline operations without publishing them.
    pub fn shared_snapshot(
        &self,
        family: FamilyRef,
        wrapping_key: Vec<u8>,
    ) -> Result<SharedSnapshotRow, BindingError> {
        let mut store = self.store.lock().map_err(|_| BindingError::LockPoisoned)?;
        let handle = family.handle()?;
        if store.saved_removal(handle).map_err(rejected)?.is_some() {
            return Err(BindingError::Rejected("device removal verified".to_owned()));
        }
        let ready = ready_session_for(&mut store, handle, &fixed(&wrapping_key)?)?;
        let projection = ready.projection_with_pending(&store).map_err(rejected)?;
        let public = PublicHistorySession::resume(&store, handle).map_err(rejected)?;
        let devices = public
            .chain()
            .active_devices()
            .map_err(rejected)?
            .into_iter()
            .map(|device| SharedDeviceRow {
                device_id: device.device_id.to_vec(),
                role: device.role,
            })
            .collect();
        let Value::Map(state) =
            cbor::decode(&public.chain().state_bytes().map_err(rejected)?).map_err(rejected)?
        else {
            return Err(BindingError::InvalidBytes);
        };
        let Value::Array(rows) = &state[5].1 else {
            return Err(BindingError::InvalidBytes);
        };
        let pending_devices = rows
            .iter()
            .map(|row| {
                let Value::Array(fields) = row else {
                    return Err(BindingError::InvalidBytes);
                };
                if fields.len() != 9 {
                    return Err(BindingError::InvalidBytes);
                }
                let (Value::Bytes(invitation_id), Value::Bytes(device_id)) =
                    (&fields[0], &fields[1])
                else {
                    return Err(BindingError::InvalidBytes);
                };
                if invitation_id.len() != 16 || device_id.len() != 16 {
                    return Err(BindingError::InvalidBytes);
                }
                Ok(PendingDeviceRow {
                    invitation_id: invitation_id.clone(),
                    device_id: device_id.clone(),
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let inert_count = projection.inert_batches().len() as u64;
        let recent_inert = projection
            .inert_batches()
            .iter()
            .rev()
            .take(16)
            .map(|row| InertBatchRow {
                cursor: row.cursor,
                object_hash: row.object_hash.to_vec(),
                reason: row.reason.clone(),
            })
            .collect();
        let children = local_api::children_from_records(projection.records())
            .into_iter()
            .map(|child| ChildRow {
                id: child.id.to_vec(),
                name: child.name,
                birth_day: child.birth_day,
                sex: child.sex,
            })
            .collect();
        let activities = local_api::activities_from_records(projection.records())
            .into_iter()
            .map(|row| ActivityRow {
                id: row.id.to_vec(),
                child_id: row.child_id.to_vec(),
                kind: row.kind,
                start_utc_ms: row.start_utc_ms,
                offset_minutes: row.offset_minutes,
                end_utc_ms: row.end_utc_ms,
                sleep_place: row.sleep_place,
                note: row.note,
                diaper_kind: row.diaper_kind,
                bottle_ml: row.bottle_ml,
                bottle_entered: row.bottle_entered,
                bottle_unit: row.bottle_unit,
                bottle_content: row.bottle_content,
                breast_side: row.breast_side,
                breast_segments: row
                    .breast_segments
                    .map(|segments| segments.into_iter().map(breast_segment_row).collect()),
                solids_foods: row.solids_foods,
                solids_amount: row.solids_amount,
                pump_left_ml: row.pump_left_ml,
                pump_right_ml: row.pump_right_ml,
                pump_total_ml: row.pump_total_ml,
                growth_weight_g: row.growth_weight_g,
                growth_weight_entered: row.growth_weight_entered,
                growth_weight_unit: row.growth_weight_unit,
                growth_length_mm: row.growth_length_mm,
                growth_length_entered: row.growth_length_entered,
                growth_length_unit: row.growth_length_unit,
                growth_head_mm: row.growth_head_mm,
                growth_head_entered: row.growth_head_entered,
                growth_head_unit: row.growth_head_unit,
                temperature_c: row.temperature_c,
                temperature_entered: row.temperature_entered,
                temperature_unit: row.temperature_unit,
                medication_name: row.medication_name,
                medication_dose_amount: row.medication_dose_amount,
                medication_dose_unit: row.medication_dose_unit,
            })
            .collect();
        Ok(SharedSnapshotRow {
            family,
            verified_cursor: ready.observed_cursor(),
            children,
            activities,
            unsent_count: ready.unsent_local_count(&store).map_err(rejected)?,
            inert_count,
            recent_inert,
            devices,
            pending_devices,
        })
    }

    pub fn shared_day_summary(
        &self,
        family: FamilyRef,
        wrapping_key: Vec<u8>,
        child_id: Vec<u8>,
        window: DayWindowRow,
    ) -> Result<DaySummaryRow, BindingError> {
        let mut store = self.store.lock().map_err(|_| BindingError::LockPoisoned)?;
        let ready = ready_session_for(&mut store, family.handle()?, &fixed(&wrapping_key)?)?;
        let projection = ready.projection_with_pending(&store).map_err(rejected)?;
        let child_id = fixed(&child_id)?;
        if projection
            .record(&child_id)
            .is_none_or(|child| child.scope != operation::Scope::Child || child.deleted)
        {
            return Err(BindingError::InvalidBytes);
        }
        local_api::summarize_day(
            local_api::activities_from_records(projection.records()),
            child_id,
            window.into(),
        )
        .map(Into::into)
        .map_err(rejected)
    }

    /// A shared file contains the verified prefix and durable local edits.
    /// Restoring it always creates an independent local-only Family.
    pub fn shared_backup_file(
        &self,
        family: FamilyRef,
        wrapping_key: Vec<u8>,
        now_ms: i64,
        password: Option<String>,
        available_memory_bytes: u64,
    ) -> Result<BackupFileRow, BindingError> {
        let mut store = self.store.lock().map_err(|_| BindingError::LockPoisoned)?;
        let ready = ready_session_for(&mut store, family.handle()?, &fixed(&wrapping_key)?)?;
        let readable =
            babytrack_core::portable_file::export_readable_shared(&store, &ready, now_ms)
                .map_err(rejected)?;
        let info = LocalRepository::inspect_readable(&readable).map_err(rejected)?;
        let bytes = if let Some(password) = password {
            babytrack_core::portable_file::protect_readable(
                &readable,
                &password,
                available_memory_bytes,
            )
            .map_err(rejected)?
        } else {
            readable
        };
        Ok(BackupFileRow {
            bytes,
            revision: 0,
            info: info.into(),
        })
    }

    pub fn shared_analysis_csv(
        &self,
        family: FamilyRef,
        wrapping_key: Vec<u8>,
    ) -> Result<Vec<u8>, BindingError> {
        let mut store = self.store.lock().map_err(|_| BindingError::LockPoisoned)?;
        let ready = ready_session_for(&mut store, family.handle()?, &fixed(&wrapping_key)?)?;
        let projection = ready.projection_with_pending(&store).map_err(rejected)?;
        Ok(babytrack_core::analysis_csv::export(
            ready.family().family_id,
            projection.records(),
        ))
    }

    /// Repeated requests return the same independent destination Family.
    pub fn private_copy_shared(
        &self,
        family: FamilyRef,
        wrapping_key: Vec<u8>,
        now_ms: i64,
    ) -> Result<FamilyRef, BindingError> {
        let mut store = self.store.lock().map_err(|_| BindingError::LockPoisoned)?;
        let handle = family.handle()?;
        if let Some(removal) = store.saved_removal(handle).map_err(rejected)?
            && let Some(copy) = store
                .removal_copy_of(handle, removal.transition_id)
                .map_err(rejected)?
        {
            return Ok(copy.into());
        }
        let ready = ready_session_for(&mut store, handle, &fixed(&wrapping_key)?)?;
        Ok(
            babytrack_core::portable_file::private_copy_shared(&mut store, &ready, now_ms)
                .map_err(rejected)?
                .into(),
        )
    }

    pub fn is_removed(&self, family: FamilyRef) -> Result<bool, BindingError> {
        self.store
            .lock()
            .map_err(|_| BindingError::LockPoisoned)?
            .saved_removal(family.handle()?)
            .map(|saved| saved.is_some())
            .map_err(rejected)
    }

    /// Read the destination of an automatic private copy after a verified
    /// removal, including when another process created it in the background.
    pub fn saved_removal_copy(&self, family: FamilyRef) -> Result<Option<FamilyRef>, BindingError> {
        let store = self.store.lock().map_err(|_| BindingError::LockPoisoned)?;
        let source = family.handle()?;
        let Some(removal) = store.saved_removal(source).map_err(rejected)? else {
            return Ok(None);
        };
        store
            .removal_copy_of(source, removal.transition_id)
            .map(|copy| copy.map(Into::into))
            .map_err(rejected)
    }

    pub fn is_shared(&self, family: FamilyRef) -> Result<bool, BindingError> {
        self.store
            .lock()
            .map_err(|_| BindingError::LockPoisoned)?
            .is_shared_family(family.handle()?)
            .map_err(rejected)
    }

    /// Return the exact durable next envelope. A lost HTTP response must
    /// retry these bytes until the signed log or rejection resolves it.
    pub fn prepare_shared_upload(
        &self,
        family: FamilyRef,
        wrapping_key: Vec<u8>,
    ) -> Result<Option<Vec<u8>>, BindingError> {
        let mut store = self.store.lock().map_err(|_| BindingError::LockPoisoned)?;
        let handle = family.handle()?;
        if store.saved_removal(handle).map_err(rejected)?.is_some() {
            return Err(BindingError::Rejected("device removal verified".to_owned()));
        }
        let wrapping = fixed(&wrapping_key)?;
        let ready = ready_session_for(&mut store, handle, &wrapping)?;
        if !ready.has_unsent_local(&store).map_err(rejected)? {
            return Ok(None);
        }
        let staged = if store
            .has_enrollment_attempt(handle.family_id)
            .map_err(rejected)?
        {
            let attempt = EnrollmentAttempt::resume(&mut store, handle.family_id, &wrapping)
                .map_err(rejected)?;
            ready
                .stage_enrolled_local(&mut store, &attempt)
                .map_err(rejected)?
        } else {
            let manager = ManagerCreation::resume(&store, handle, &wrapping).map_err(rejected)?;
            manager
                .stage_next_local(&ready, &mut store)
                .map_err(rejected)?
        };
        let batch = match staged {
            babytrack_core::shared_ready::NextUpload::Fresh(value)
            | babytrack_core::shared_ready::NextUpload::RetryExact(value) => value,
        };
        Ok(Some(batch.envelope_bytes))
    }

    /// Pull the active log and referenced objects through a byte transport.
    /// Only full Rust verification advances the durable cursor and outbox.
    pub fn sync_shared(
        &self,
        family: FamilyRef,
        wrapping_key: Vec<u8>,
        transport: Box<dyn RelayReadTransport>,
    ) -> Result<SharedSyncRow, BindingError> {
        let mut store = self.store.lock().map_err(|_| BindingError::LockPoisoned)?;
        let handle = family.handle()?;
        if store.saved_removal(handle).map_err(rejected)?.is_some() {
            return Err(BindingError::Rejected("device removal verified".to_owned()));
        }
        let wrapping = fixed(&wrapping_key)?;
        let signer = if store
            .has_enrollment_attempt(handle.family_id)
            .map_err(rejected)?
        {
            let attempt = EnrollmentAttempt::resume(&mut store, handle.family_id, &wrapping)
                .map_err(rejected)?;
            if attempt.family() != handle
                || !attempt.has_committed_admission(&store).map_err(rejected)?
            {
                return Err(BindingError::InvalidBytes);
            }
            ActiveReadSigner::Recipient(attempt)
        } else {
            ActiveReadSigner::Manager(
                ManagerCreation::resume(&store, handle, &wrapping).map_err(rejected)?,
            )
        };
        let signer = Arc::new(signer);
        let transport: Arc<dyn RelayReadTransport> = Arc::from(transport);
        let active_pull::SyncProgress { pull, hydration } = futures::executor::block_on(
            active_pull::pull_and_hydrate(&mut store, handle, 4, 16, |path| {
                let signer = Arc::clone(&signer);
                let transport = Arc::clone(&transport);
                async move {
                    let auth = signer.sign_get(&path)?.bytes;
                    transport.get(path, auth)
                }
            }),
        )
        .map_err(rejected)?;
        let (ready, outbox_state, inert_count) = if pull.no_more_visible && !hydration.remaining {
            let ready = ready_session_for(&mut store, handle, &wrapping)?;
            let unsent = ready.unsent_local_count(&store).map_err(rejected)?;
            let public = PublicHistorySession::resume(&store, handle).map_err(rejected)?;
            let outbox = if unsent == 0 {
                0
            } else if public.pending_batch_id(&store).map_err(rejected)?.is_some() {
                2
            } else {
                1
            };
            (
                true,
                outbox,
                ready.projection().inert_batches().len() as u64,
            )
        } else {
            (false, 0, 0)
        };
        Ok(SharedSyncRow {
            verified_cursor: pull.verified_cursor,
            no_more_visible: pull.no_more_visible,
            remaining_objects: hydration.remaining,
            ready,
            outbox_state,
            inert_count,
        })
    }

    /// Query the signed outcome of exact uncertain bytes. Code 0 means no
    /// outbox, 1 no result, 2 accepted ahead of our verified log, 3 verified
    /// rejection rebased the outbox, and 4 a different rejection blocks it.
    pub fn resolve_pending_batch_result(
        &self,
        family: FamilyRef,
        wrapping_key: Vec<u8>,
        transport: Box<dyn RelayReadTransport>,
    ) -> Result<u8, BindingError> {
        let mut store = self.store.lock().map_err(|_| BindingError::LockPoisoned)?;
        let handle = family.handle()?;
        let public = PublicHistorySession::resume(&store, handle).map_err(rejected)?;
        let Some(batch_id) = public.pending_batch_id(&store).map_err(rejected)? else {
            return Ok(0);
        };
        let wrapping = fixed(&wrapping_key)?;
        let signer = if store
            .has_enrollment_attempt(handle.family_id)
            .map_err(rejected)?
        {
            let attempt = EnrollmentAttempt::resume(&mut store, handle.family_id, &wrapping)
                .map_err(rejected)?;
            if attempt.family() != handle {
                return Err(BindingError::InvalidBytes);
            }
            ActiveReadSigner::Recipient(attempt)
        } else {
            ActiveReadSigner::Manager(
                ManagerCreation::resume(&store, handle, &wrapping).map_err(rejected)?,
            )
        };
        let path = format!(
            "/v1/families/{}/batch-results/{}",
            lower_hex(&family.family_id),
            lower_hex(&batch_id),
        );
        let auth = signer.sign_get(&path)?.bytes;
        let result = transport.get(path, auth)?;
        Ok(
            match public
                .resolve_pending_result(&mut store, &result)
                .map_err(rejected)?
            {
                PendingBatchResult::NoPending => 0,
                PendingBatchResult::Unresolved => 1,
                PendingBatchResult::AcceptedAhead => 2,
                PendingBatchResult::Rebased => 3,
                PendingBatchResult::Blocked => 4,
            },
        )
    }
}
