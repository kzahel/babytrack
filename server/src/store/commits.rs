//! Authority and encrypted-batch commits with explicit SQLite transactions.

use super::*;

impl RelayStore {
    pub fn commit_genesis(
        &mut self,
        path_family: [u8; 16],
        candidate_bytes: &[u8],
        committed_ms: i64,
    ) -> Result<Vec<u8>, Error> {
        let candidate = authority::verify_genesis_candidate(candidate_bytes, &self.relay_public)?;
        if candidate.family_id != path_family {
            return Err(Error::Invalid("commit path differs from signed candidate"));
        }
        let reservation = crypto::hash("genesis-reservation", candidate_bytes)?;
        let tx = self.db.transaction()?;
        let saved: Option<SavedReservation> = tx.query_row(
            "SELECT reservation_hash,candidate_bytes,active,committed_bytes FROM families WHERE family_id=?1",
            params![&candidate.family_id[..]], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?)),
        ).optional()?;
        let Some((saved_hash, saved_candidate, active, committed)) = saved else {
            return Err(Error::Invalid("genesis has no staged reservation"));
        };
        if saved_hash != reservation || saved_candidate != candidate_bytes {
            return Err(Error::Invalid("genesis differs from reservation"));
        }
        if active == 1 {
            verify_stored_genesis(
                &tx,
                candidate.family_id,
                &saved_candidate,
                self.relay_public,
            )?;
            return Ok(receipt::control_commit_response(
                &committed.ok_or(Error::Invalid("active genesis missing bytes"))?,
            )?);
        }
        ensure_control_ids(&tx, candidate.family_id, candidate_bytes)?;
        for entry in &candidate.manifest {
            let staged: Option<(i64, Vec<u8>, Vec<u8>)> = tx.query_row(
                "SELECT kind,object_hash,object_bytes FROM staged_objects WHERE family_id=?1 AND object_id=?2",
                params![&candidate.family_id[..], &entry.object_id[..]], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?)),
            ).optional()?;
            let Some((kind, hash, bytes)) = staged else {
                return Err(Error::Invalid("manifest object not staged"));
            };
            if kind != i64::from(entry.kind)
                || hash != entry.object_hash
                || bytes.len() != entry.object_len as usize
                || crypto::hash("object", &bytes)? != entry.object_hash
            {
                return Err(Error::Invalid("staged object mismatch"));
            }
            tx.execute(
                "INSERT INTO committed_objects(family_id,object_id,kind,object_hash,object_bytes,transition_id) VALUES(?1,?2,?3,?4,?5,?6)",
                params![&candidate.family_id[..], &entry.object_id[..], kind, &hash, &bytes, &candidate.transition_id[..]],
            )?;
        }
        let committed =
            receipt::commit_control(candidate_bytes, &self.relay_seed, 1, committed_ms)?;
        let head = crypto::hash("control-head", &committed)?;
        tx.execute(
            "UPDATE families SET active=1, committed_bytes=?2,head_hash=?3,cursor=1 WHERE family_id=?1 AND active=0",
            params![&candidate.family_id[..], &committed, &head[..]],
        )?;
        tx.execute(
            "INSERT INTO entries(family_id,cursor,kind,committed_bytes) VALUES(?1,1,1,?2)",
            params![&candidate.family_id[..], &committed],
        )?;
        tx.execute(
            "DELETE FROM staged_objects WHERE family_id=?1",
            params![&candidate.family_id[..]],
        )?;
        tx.commit()?;
        Ok(receipt::control_commit_response(&committed)?)
    }

    pub fn commit_first_issue(
        &mut self,
        path_family: [u8; 16],
        candidate_bytes: &[u8],
        committed_ms: i64,
    ) -> Result<Vec<u8>, Error> {
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (genesis_bytes,cursor,head,genesis_committed) = tx.query_row(
            "SELECT candidate_bytes,cursor,head_hash,committed_bytes FROM families WHERE family_id=?1 AND active=1",
            params![&path_family[..]], |r| Ok((r.get::<_,Vec<u8>>(0)?,r.get::<_,i64>(1)?,r.get::<_,Vec<u8>>(2)?,r.get::<_,Vec<u8>>(3)?)),
        )?;
        let genesis = verify_stored_genesis(&tx, path_family, &genesis_bytes, self.relay_public)?;
        let controls = control_count(&tx, path_family)?;
        let genesis_head = crypto::hash("control-head", &genesis_committed)?;
        let issue = authority::verify_first_invite_issue(candidate_bytes, &genesis, genesis_head)?;
        if issue.family_id != path_family {
            return Err(Error::Invalid("issue path Family mismatch"));
        }
        if controls >= 2 {
            let committed = control_at(&tx, path_family, 1)?;
            if control_candidate(&committed)? == candidate_bytes {
                return Ok(receipt::control_commit_response(&committed)?);
            }
            return Err(Error::Invalid("first issue already differs"));
        }
        if controls != 1 || head != genesis_head {
            return Err(Error::Invalid("issue prior head stale"));
        }
        let genesis_time = control_commit_time(&genesis_committed)?;
        if committed_ms < genesis_time {
            return Err(Error::Invalid("relay time moved backward"));
        }
        ensure_control_ids(&tx, path_family, candidate_bytes)?;
        let staged_candidate: Option<Vec<u8>> = tx
            .query_row(
                "SELECT candidate_bytes FROM staged_controls WHERE family_id=?1 AND transition_id=?2",
                params![&path_family[..], &issue.transition_id[..]],
                |r| r.get(0),
            )
            .optional()?;
        if staged_candidate.as_deref() != Some(candidate_bytes) {
            return Err(Error::Invalid("issue candidate not staged"));
        }
        let staged: Option<(i64,Vec<u8>,Vec<u8>)> = tx.query_row(
            "SELECT kind,object_hash,object_bytes FROM staged_control_objects WHERE family_id=?1 AND transition_id=?2 AND object_id=?3",
            params![&path_family[..],&issue.transition_id[..],&issue.manifest.object_id[..]], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?)),
        ).optional()?;
        let Some((kind, hash, bytes)) = staged else {
            return Err(Error::Invalid("issue membership object missing"));
        };
        if kind != 1
            || hash != issue.manifest.object_hash
            || bytes.len() != issue.manifest.object_len as usize
            || crypto::hash("object", &bytes)? != issue.manifest.object_hash
        {
            return Err(Error::Invalid("issue membership object changed"));
        }
        let next_cursor = cursor
            .checked_add(1)
            .ok_or(Error::Invalid("cursor overflow"))?;
        let committed = receipt::commit_control(
            candidate_bytes,
            &self.relay_seed,
            next_cursor as u64,
            committed_ms,
        )?;
        let next_head = crypto::hash("control-head", &committed)?;
        tx.execute("INSERT INTO committed_objects(family_id,object_id,kind,object_hash,object_bytes,transition_id) VALUES(?1,?2,1,?3,?4,?5)",
            params![&path_family[..],&issue.manifest.object_id[..],&hash,&bytes,&issue.transition_id[..]])?;
        tx.execute(
            "INSERT INTO entries(family_id,cursor,kind,committed_bytes) VALUES(?1,?2,1,?3)",
            params![&path_family[..], next_cursor, &committed],
        )?;
        let updated = tx.execute("UPDATE families SET cursor=?2,head_hash=?3 WHERE family_id=?1 AND cursor=?4 AND head_hash=?5",
            params![&path_family[..],next_cursor,&next_head[..],cursor,&genesis_head[..]])?;
        if updated != 1 {
            return Err(Error::Invalid("issue compare-and-swap failed"));
        }
        tx.execute(
            "DELETE FROM staged_controls WHERE family_id=?1",
            params![&path_family[..]],
        )?;
        tx.execute(
            "DELETE FROM staged_control_objects WHERE family_id=?1",
            params![&path_family[..]],
        )?;
        tx.commit()?;
        Ok(receipt::control_commit_response(&committed)?)
    }

    pub fn commit_first_claim(
        &mut self,
        path_family: [u8; 16],
        candidate_bytes: &[u8],
        committed_ms: i64,
    ) -> Result<Vec<u8>, Error> {
        self.commit_first_claim_with_clock(path_family, candidate_bytes, || Ok(committed_ms))
    }

    pub fn commit_first_claim_with_clock(
        &mut self,
        path_family: [u8; 16],
        candidate_bytes: &[u8],
        clock: impl FnOnce() -> Result<i64, Error>,
    ) -> Result<Vec<u8>, Error> {
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (genesis_bytes,cursor,head,genesis_committed)=tx.query_row(
            "SELECT candidate_bytes,cursor,head_hash,committed_bytes FROM families WHERE family_id=?1 AND active=1",
            params![&path_family[..]],|r|Ok((r.get::<_,Vec<u8>>(0)?,r.get::<_,i64>(1)?,r.get::<_,Vec<u8>>(2)?,r.get::<_,Vec<u8>>(3)?)),
        )?;
        let genesis = verify_stored_genesis(&tx, path_family, &genesis_bytes, self.relay_public)?;
        let controls = control_count(&tx, path_family)?;
        let genesis_head = crypto::hash("control-head", &genesis_committed)?;
        let issue_committed = control_at(&tx, path_family, 1)?;
        let issue_candidate = control_candidate(&issue_committed)?;
        let issue = authority::verify_first_invite_issue(&issue_candidate, &genesis, genesis_head)?;
        let issue_head = crypto::hash("control-head", &issue_committed)?;
        let claim = authority::verify_first_claim(candidate_bytes, &genesis, &issue, issue_head)?;
        if claim.family_id != path_family {
            return Err(Error::Invalid("claim Family path mismatch"));
        }
        if controls >= 3 {
            let committed = control_at(&tx, path_family, 2)?;
            if control_candidate(&committed)? == candidate_bytes {
                return Ok(receipt::control_commit_response(&committed)?);
            }
            return Err(Error::Invalid(
                "invitation already claimed by another candidate",
            ));
        }
        if controls != 2 || head != issue_head {
            return Err(Error::Invalid("claim head stale"));
        }
        let issue_time = control_commit_time(&issue_committed)?;
        let expiry = issue_time
            .checked_add(604_800_000)
            .ok_or(Error::Invalid("invite expiry overflow"))?;
        ensure_control_ids(&tx, path_family, candidate_bytes)?;
        let committed_ms = clock()?;
        if committed_ms < issue_time || committed_ms >= expiry {
            return Err(Error::Invalid("claim expired or relay time moved backward"));
        }
        let next_cursor = cursor
            .checked_add(1)
            .ok_or(Error::Invalid("cursor overflow"))?;
        let committed = receipt::commit_control(
            candidate_bytes,
            &self.relay_seed,
            next_cursor as u64,
            committed_ms,
        )?;
        let next_head = crypto::hash("control-head", &committed)?;
        tx.execute(
            "INSERT INTO entries(family_id,cursor,kind,committed_bytes) VALUES(?1,?2,1,?3)",
            params![&path_family[..], next_cursor, &committed],
        )?;
        let updated=tx.execute("UPDATE families SET cursor=?2,head_hash=?3 WHERE family_id=?1 AND cursor=?4 AND head_hash=?5",
            params![&path_family[..],next_cursor,&next_head[..],cursor,&issue_head[..]])?;
        if updated != 1 {
            return Err(Error::Invalid("claim compare-and-swap failed"));
        }
        tx.commit()?;
        Ok(receipt::control_commit_response(&committed)?)
    }

    /// Commit a non-genesis transition through the same public reducers used
    /// by clients and relay restart. The writer lock serializes its cursor,
    /// control head, staged objects, and authority decision.
    pub fn commit_general_control_with_clock(
        &mut self,
        family: [u8; 16],
        candidate: &[u8],
        clock: impl FnOnce() -> Result<i64, Error>,
    ) -> Result<Vec<u8>, Error> {
        let ids = control_birth_ids(candidate)?;
        let transition = ids[0];
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut controls = tx.prepare(
            "SELECT committed_bytes FROM entries WHERE family_id=?1 AND kind=1 ORDER BY cursor",
        )?;
        let rows = controls.query_map(params![&family[..]], |row| row.get::<_, Vec<u8>>(0))?;
        for row in rows {
            let committed = row?;
            if control_birth_ids(&committed)?.first() == Some(&transition) {
                if control_candidate(&committed)? == candidate {
                    return Ok(receipt::control_commit_response(&committed)?);
                }
                return Err(Error::Invalid("transition ID reused with different bytes"));
            }
        }
        drop(controls);
        let mut ledger =
            Self::verify_saved_family(&tx, family, self.relay_public, &self.relay_seed)?;
        let (cursor, saved_head): (i64, Vec<u8>) = tx.query_row(
            "SELECT cursor,head_hash FROM families WHERE family_id=?1 AND active=1",
            params![&family[..]],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        if saved_head != ledger.head() {
            return Err(Error::Invalid("general control head differs"));
        }
        ensure_control_ids(&tx, family, candidate)?;
        let next_cursor = cursor
            .checked_add(1)
            .ok_or(Error::Invalid("cursor overflow"))?;
        let committed = receipt::commit_control(
            candidate,
            &self.relay_seed,
            u64::try_from(next_cursor).map_err(|_| Error::Invalid("cursor range"))?,
            clock()?,
        )?;
        let verified = receipt::verify_control_receipt(&committed, &self.relay_public)?;
        if !matches!(verified.kind, 2 | 4 | 5 | 6 | 8 | 10 | 11)
            || verified.family_id != family
            || verified.transition_id != transition
        {
            return Err(Error::Invalid("not a general control"));
        }
        let prior_state = ledger.state().clone();
        ledger.apply(&verified, &committed)?;
        if !verified.manifest.is_empty() {
            let staged: Option<Vec<u8>> = tx.query_row(
                "SELECT candidate_bytes FROM staged_controls WHERE family_id=?1 AND transition_id=?2",
                params![&family[..], &transition[..]],
                |row| row.get(0),
            ).optional()?;
            if staged.as_deref() != Some(candidate) {
                return Err(Error::Invalid("control candidate not staged"));
            }
            let mut grant_recipients = BTreeSet::new();
            for object in &verified.manifest {
                let saved: Option<(i64, Vec<u8>, Vec<u8>)> = tx.query_row(
                    "SELECT kind,object_hash,object_bytes FROM staged_control_objects WHERE family_id=?1 AND transition_id=?2 AND object_id=?3",
                    params![&family[..], &transition[..], &object.id[..]],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                ).optional()?;
                let Some((kind, hash, bytes)) = saved else {
                    return Err(Error::Invalid("control manifest object missing"));
                };
                if kind != i64::from(object.kind)
                    || hash != object.hash
                    || usize::try_from(object.len).ok() != Some(bytes.len())
                    || crypto::hash("object", &bytes)? != object.hash
                {
                    return Err(Error::Invalid("control manifest object differs"));
                }
                if let Some(recipient) = validate_general_grant_object(
                    candidate,
                    &prior_state,
                    object.kind,
                    object.id,
                    &bytes,
                )? && !grant_recipients.insert(recipient)
                {
                    return Err(Error::Invalid("grant recipient repeated"));
                }
                tx.execute(
                    "INSERT INTO committed_objects(family_id,object_id,kind,object_hash,object_bytes,transition_id) VALUES(?1,?2,?3,?4,?5,?6)",
                    params![&family[..], &object.id[..], kind, &hash, &bytes, &transition[..]],
                )?;
            }
            if verified.kind == 8 {
                let Value::Map(fields) = &prior_state else {
                    return Err(Error::Invalid("rotation prior state not map"));
                };
                let Value::Array(active) = &fields[4].1 else {
                    return Err(Error::Invalid("rotation active state not array"));
                };
                let Value::Map(root) = cbor::decode(candidate)? else {
                    return Err(Error::Invalid("rotation candidate not map"));
                };
                let Value::Map(unsigned) = &root[0].1 else {
                    return Err(Error::Invalid("rotation unsigned not map"));
                };
                let Value::Map(delta) = &unsigned[6].1 else {
                    return Err(Error::Invalid("rotation delta not map"));
                };
                let removed = fixed::<16>(&delta[0].1)?;
                let remaining: BTreeSet<_> = active
                    .iter()
                    .map(|row| match row {
                        Value::Array(parts) => fixed::<16>(&parts[0]),
                        _ => Err(Error::Invalid("rotation active row not array")),
                    })
                    .collect::<Result<_, _>>()?;
                let remaining: BTreeSet<_> = remaining
                    .into_iter()
                    .filter(|device| *device != removed)
                    .collect();
                if grant_recipients != remaining {
                    return Err(Error::Invalid("rotation grants omit active device"));
                }
            }
        }
        let next_head = ledger.head();
        tx.execute(
            "INSERT INTO entries(family_id,cursor,kind,committed_bytes) VALUES(?1,?2,1,?3)",
            params![&family[..], next_cursor, &committed],
        )?;
        let changed = tx.execute(
            "UPDATE families SET cursor=?2,head_hash=?3 WHERE family_id=?1 AND cursor=?4 AND head_hash=?5",
            params![&family[..], next_cursor, &next_head[..], cursor, &saved_head],
        )?;
        if changed != 1 {
            return Err(Error::Invalid("general control compare-and-swap failed"));
        }
        tx.execute(
            "DELETE FROM staged_control_objects WHERE family_id=?1 AND transition_id=?2",
            params![&family[..], &transition[..]],
        )?;
        tx.execute(
            "DELETE FROM staged_controls WHERE family_id=?1 AND transition_id=?2",
            params![&family[..], &transition[..]],
        )?;
        tx.commit()?;
        Ok(receipt::control_commit_response(&committed)?)
    }

    /// Commit a no-object manager transition against the authenticated
    /// current public ledger. Exact retries are resolved before current
    /// authority is checked, since a later control may have changed it.
    pub fn commit_manager_change_with_clock(
        &mut self,
        family: [u8; 16],
        candidate: &[u8],
        clock: impl FnOnce() -> Result<i64, Error>,
    ) -> Result<Vec<u8>, Error> {
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let ids = control_birth_ids(candidate)?;
        if ids.len() != 1 {
            return Err(Error::Invalid("manager change has new object IDs"));
        }
        let Value::Map(root) = cbor::decode(candidate)? else {
            return Err(Error::Invalid("manager change not map"));
        };
        let Some((1, Value::Map(unsigned))) = root.first() else {
            return Err(Error::Invalid("manager change unsigned absent"));
        };
        if !matches!(number(&unsigned[5].1)?, 3 | 7 | 9) {
            return Err(Error::Invalid("not a no-object manager change"));
        }
        let transition_id = ids[0];
        let mut controls = tx.prepare(
            "SELECT committed_bytes FROM entries WHERE family_id=?1 AND kind=1 ORDER BY cursor",
        )?;
        let rows = controls.query_map(params![&family[..]], |row| row.get::<_, Vec<u8>>(0))?;
        for row in rows {
            let committed = row?;
            if control_birth_ids(&committed)?.first() == Some(&transition_id) {
                if control_candidate(&committed)? == candidate {
                    return Ok(receipt::control_commit_response(&committed)?);
                }
                return Err(Error::Invalid("transition ID reused with different bytes"));
            }
        }
        drop(controls);

        let mut ledger =
            Self::verify_saved_family(&tx, family, self.relay_public, &self.relay_seed)?;
        let (cursor, saved_head): (i64, Vec<u8>) = tx.query_row(
            "SELECT cursor,head_hash FROM families WHERE family_id=?1 AND active=1",
            params![&family[..]],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        if saved_head != ledger.head() {
            return Err(Error::Invalid("manager change head differs"));
        }
        ensure_control_ids(&tx, family, candidate)?;
        let next_cursor = cursor
            .checked_add(1)
            .ok_or(Error::Invalid("cursor overflow"))?;
        let committed = receipt::commit_control(
            candidate,
            &self.relay_seed,
            u64::try_from(next_cursor).map_err(|_| Error::Invalid("cursor range"))?,
            clock()?,
        )?;
        let verified = receipt::verify_control_receipt(&committed, &self.relay_public)?;
        if !matches!(verified.kind, 3 | 7 | 9)
            || verified.family_id != family
            || !verified.manifest.is_empty()
        {
            return Err(Error::Invalid("not a no-object manager change"));
        }
        ledger.apply(&verified, &committed)?;
        let next_head = ledger.head();
        tx.execute(
            "INSERT INTO entries(family_id,cursor,kind,committed_bytes) VALUES(?1,?2,1,?3)",
            params![&family[..], next_cursor, &committed],
        )?;
        let updated = tx.execute(
            "UPDATE families SET cursor=?2,head_hash=?3 WHERE family_id=?1 AND cursor=?4 AND head_hash=?5",
            params![&family[..], next_cursor, &next_head[..], cursor, &saved_head],
        )?;
        if updated != 1 {
            return Err(Error::Invalid("manager change compare-and-swap failed"));
        }
        tx.commit()?;
        Ok(receipt::control_commit_response(&committed)?)
    }

    pub fn commit_first_challenge(
        &mut self,
        path_family: [u8; 16],
        candidate_bytes: &[u8],
        committed_ms: i64,
    ) -> Result<Vec<u8>, Error> {
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let prefix = load_join_prefix(&tx, self.relay_public, path_family)?;
        let challenge = authority::verify_first_challenge(
            candidate_bytes,
            &prefix.genesis,
            &prefix.issue,
            &prefix.claim,
            prefix.claim_head,
        )?;
        if prefix.controls >= 4 {
            let committed = control_at(&tx, path_family, 3)?;
            if control_candidate(&committed)? == candidate_bytes {
                return Ok(receipt::control_commit_response(&committed)?);
            }
            return Err(Error::Invalid("challenge already committed differently"));
        }
        if prefix.controls != 3
            || prefix.head != prefix.claim_head
            || committed_ms < prefix.claim_time
        {
            return Err(Error::Invalid("challenge head or relay time invalid"));
        }
        ensure_control_ids(&tx, path_family, candidate_bytes)?;
        let staged:Option<Vec<u8>>=tx.query_row("SELECT candidate_bytes FROM staged_controls WHERE family_id=?1 AND transition_id=?2",
            params![&path_family[..],&challenge.transition_id[..]],|r|r.get(0)).optional()?;
        if staged.as_deref() != Some(candidate_bytes) {
            return Err(Error::Invalid("challenge candidate not staged"));
        }
        for entry in &challenge.manifest {
            let staged:Option<(i64,Vec<u8>,Vec<u8>)>=tx.query_row(
                "SELECT kind,object_hash,object_bytes FROM staged_control_objects WHERE family_id=?1 AND transition_id=?2 AND object_id=?3",
                params![&path_family[..],&challenge.transition_id[..],&entry.object_id[..]],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)),
            ).optional()?;
            let Some((kind, hash, bytes)) = staged else {
                return Err(Error::Invalid("challenge object missing"));
            };
            if kind != i64::from(entry.kind)
                || hash != entry.object_hash
                || bytes.len() != entry.object_len as usize
                || crypto::hash("object", &bytes)? != entry.object_hash
            {
                return Err(Error::Invalid("challenge staged bytes changed"));
            }
            tx.execute("INSERT INTO committed_objects(family_id,object_id,kind,object_hash,object_bytes,transition_id) VALUES(?1,?2,?3,?4,?5,?6)",
                params![&path_family[..],&entry.object_id[..],kind,&hash,&bytes,&challenge.transition_id[..]])?;
        }
        let next_cursor = prefix
            .cursor
            .checked_add(1)
            .ok_or(Error::Invalid("cursor overflow"))?;
        let committed = receipt::commit_control(
            candidate_bytes,
            &self.relay_seed,
            next_cursor as u64,
            committed_ms,
        )?;
        let next_head = crypto::hash("control-head", &committed)?;
        tx.execute(
            "INSERT INTO entries(family_id,cursor,kind,committed_bytes) VALUES(?1,?2,1,?3)",
            params![&path_family[..], next_cursor, &committed],
        )?;
        let updated=tx.execute("UPDATE families SET cursor=?2,head_hash=?3 WHERE family_id=?1 AND cursor=?4 AND head_hash=?5",
            params![&path_family[..],next_cursor,&next_head[..],prefix.cursor,&prefix.claim_head[..]])?;
        if updated != 1 {
            return Err(Error::Invalid("challenge compare-and-swap failed"));
        }
        tx.execute(
            "DELETE FROM staged_controls WHERE family_id=?1",
            params![&path_family[..]],
        )?;
        tx.execute(
            "DELETE FROM staged_control_objects WHERE family_id=?1",
            params![&path_family[..]],
        )?;
        tx.commit()?;
        Ok(receipt::control_commit_response(&committed)?)
    }

    pub fn commit_first_proof(
        &mut self,
        path_family: [u8; 16],
        candidate_bytes: &[u8],
        committed_ms: i64,
    ) -> Result<Vec<u8>, Error> {
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let prefix = load_join_prefix(&tx, self.relay_public, path_family)?;
        if prefix.controls < 4 {
            return Err(Error::Invalid("challenge not committed"));
        }
        let challenge_committed = control_at(&tx, path_family, 3)?;
        let challenge = authority::verify_first_challenge(
            &control_candidate(&challenge_committed)?,
            &prefix.genesis,
            &prefix.issue,
            &prefix.claim,
            prefix.claim_head,
        )?;
        let challenge_head = crypto::hash("control-head", &challenge_committed)?;
        let _proof = authority::verify_first_proof(
            candidate_bytes,
            &prefix.genesis,
            &prefix.issue,
            &prefix.claim,
            &challenge,
            challenge_head,
        )?;
        if prefix.controls >= 5 {
            let committed = control_at(&tx, path_family, 4)?;
            if control_candidate(&committed)? == candidate_bytes {
                return Ok(receipt::control_commit_response(&committed)?);
            }
            return Err(Error::Invalid("proof already committed differently"));
        }
        if prefix.controls != 4
            || prefix.head != challenge_head
            || committed_ms < control_commit_time(&challenge_committed)?
        {
            return Err(Error::Invalid("proof head or relay time invalid"));
        }
        let next_cursor = prefix
            .cursor
            .checked_add(1)
            .ok_or(Error::Invalid("cursor overflow"))?;
        let committed = receipt::commit_control(
            candidate_bytes,
            &self.relay_seed,
            next_cursor as u64,
            committed_ms,
        )?;
        let next_head = crypto::hash("control-head", &committed)?;
        ensure_control_ids(&tx, path_family, candidate_bytes)?;
        tx.execute(
            "INSERT INTO entries(family_id,cursor,kind,committed_bytes) VALUES(?1,?2,1,?3)",
            params![&path_family[..], next_cursor, &committed],
        )?;
        let updated=tx.execute("UPDATE families SET cursor=?2,head_hash=?3 WHERE family_id=?1 AND cursor=?4 AND head_hash=?5",
            params![&path_family[..],next_cursor,&next_head[..],prefix.cursor,&challenge_head[..]])?;
        if updated != 1 {
            return Err(Error::Invalid("proof compare-and-swap failed"));
        }
        tx.commit()?;
        Ok(receipt::control_commit_response(&committed)?)
    }

    pub fn commit_first_admission(
        &mut self,
        path_family: [u8; 16],
        candidate_bytes: &[u8],
        committed_ms: i64,
    ) -> Result<Vec<u8>, Error> {
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let prefix = load_proved_prefix(&tx, self.relay_public, path_family)?;
        let admission = authority::verify_first_admission(
            candidate_bytes,
            &prefix.join.genesis,
            &prefix.join.issue,
            &prefix.join.claim,
            &prefix.challenge,
            &prefix.proof,
            prefix.proof_head,
        )?;
        if prefix.join.controls >= 6 {
            let committed = control_at(&tx, path_family, 5)?;
            if control_candidate(&committed)? == candidate_bytes {
                return Ok(receipt::control_commit_response(&committed)?);
            }
            return Err(Error::Invalid("admission already committed differently"));
        }
        if prefix.join.controls != 5
            || prefix.join.head != prefix.proof_head
            || committed_ms < prefix.proof_time
        {
            return Err(Error::Invalid("admission head or relay time invalid"));
        }
        ensure_control_ids(&tx, path_family, candidate_bytes)?;
        let staged:Option<Vec<u8>>=tx.query_row("SELECT candidate_bytes FROM staged_controls WHERE family_id=?1 AND transition_id=?2",
            params![&path_family[..],&admission.transition_id[..]],|r|r.get(0)).optional()?;
        if staged.as_deref() != Some(candidate_bytes) {
            return Err(Error::Invalid("admission candidate not staged"));
        }
        for entry in &admission.manifest {
            let staged:Option<(i64,Vec<u8>,Vec<u8>)>=tx.query_row(
                "SELECT kind,object_hash,object_bytes FROM staged_control_objects WHERE family_id=?1 AND transition_id=?2 AND object_id=?3",
                params![&path_family[..],&admission.transition_id[..],&entry.object_id[..]],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)),
            ).optional()?;
            let Some((kind, hash, bytes)) = staged else {
                return Err(Error::Invalid("admission object missing"));
            };
            if kind != i64::from(entry.kind)
                || hash != entry.object_hash
                || bytes.len() != entry.object_len as usize
                || crypto::hash("object", &bytes)? != entry.object_hash
            {
                return Err(Error::Invalid("admission staged bytes changed"));
            }
            if entry.kind == 4 {
                authority::validate_first_admission_grant(&admission, entry.object_id, &bytes)?;
            }
            tx.execute("INSERT INTO committed_objects(family_id,object_id,kind,object_hash,object_bytes,transition_id) VALUES(?1,?2,?3,?4,?5,?6)",
                params![&path_family[..],&entry.object_id[..],kind,&hash,&bytes,&admission.transition_id[..]])?;
        }
        let next_cursor = prefix
            .join
            .cursor
            .checked_add(1)
            .ok_or(Error::Invalid("cursor overflow"))?;
        let committed = receipt::commit_control(
            candidate_bytes,
            &self.relay_seed,
            next_cursor as u64,
            committed_ms,
        )?;
        let next_head = crypto::hash("control-head", &committed)?;
        tx.execute(
            "INSERT INTO entries(family_id,cursor,kind,committed_bytes) VALUES(?1,?2,1,?3)",
            params![&path_family[..], next_cursor, &committed],
        )?;
        let updated=tx.execute("UPDATE families SET cursor=?2,head_hash=?3 WHERE family_id=?1 AND cursor=?4 AND head_hash=?5",
            params![&path_family[..],next_cursor,&next_head[..],prefix.join.cursor,&prefix.proof_head[..]])?;
        if updated != 1 {
            return Err(Error::Invalid("admission compare-and-swap failed"));
        }
        tx.execute(
            "DELETE FROM staged_controls WHERE family_id=?1",
            params![&path_family[..]],
        )?;
        tx.execute(
            "DELETE FROM staged_control_objects WHERE family_id=?1",
            params![&path_family[..]],
        )?;
        tx.commit()?;
        Ok(receipt::control_commit_response(&committed)?)
    }

    pub fn commit_first_removal(
        &mut self,
        path_family: [u8; 16],
        candidate_bytes: &[u8],
        committed_ms: i64,
    ) -> Result<Vec<u8>, Error> {
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let prefix = load_admitted_prefix(&tx, self.relay_public, path_family)?;
        let removal = authority::verify_first_removal(
            candidate_bytes,
            &prefix.proved.join.genesis,
            &prefix.proved.join.issue,
            &prefix.proved.join.claim,
            &prefix.admission,
            prefix.admission_head,
        )?;
        if prefix.proved.join.controls >= 7 {
            let committed = control_at(&tx, path_family, 6)?;
            if control_candidate(&committed)? == candidate_bytes {
                return Ok(receipt::control_commit_response(&committed)?);
            }
            return Err(Error::Invalid("removal already committed differently"));
        }
        if prefix.proved.join.controls != 6
            || prefix.proved.join.head != prefix.admission_head
            || committed_ms < prefix.admission_time
        {
            return Err(Error::Invalid("removal head or relay time invalid"));
        }
        ensure_first_removal_ids_unused(&tx, path_family, &removal)?;
        ensure_control_ids(&tx, path_family, candidate_bytes)?;
        let saved: Option<Vec<u8>> = tx.query_row(
            "SELECT candidate_bytes FROM staged_controls WHERE family_id=?1 AND transition_id=?2",
            params![&path_family[..], &removal.transition_id[..]], |r| r.get(0),
        ).optional()?;
        if saved.as_deref() != Some(candidate_bytes) {
            return Err(Error::Invalid("removal candidate not staged"));
        }
        for entry in &removal.manifest {
            let staged: Option<(i64, Vec<u8>, Vec<u8>)> = tx.query_row(
                "SELECT kind,object_hash,object_bytes FROM staged_control_objects WHERE family_id=?1 AND transition_id=?2 AND object_id=?3",
                params![&path_family[..], &removal.transition_id[..], &entry.object_id[..]],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            ).optional()?;
            let Some((kind, hash, bytes)) = staged else {
                return Err(Error::Invalid("removal object missing"));
            };
            if kind != i64::from(entry.kind)
                || hash != entry.object_hash
                || bytes.len() != entry.object_len as usize
                || crypto::hash("object", &bytes)? != entry.object_hash
            {
                return Err(Error::Invalid("removal staged bytes changed"));
            }
            validate_first_removal_object(
                entry.kind,
                entry.object_id,
                &bytes,
                candidate_bytes,
                &prefix.proved.join.genesis,
            )?;
            tx.execute(
                "INSERT INTO committed_objects(family_id,object_id,kind,object_hash,object_bytes,transition_id) VALUES(?1,?2,?3,?4,?5,?6)",
                params![&path_family[..], &entry.object_id[..], kind, &hash, &bytes, &removal.transition_id[..]],
            )?;
        }
        let next_cursor = prefix
            .proved
            .join
            .cursor
            .checked_add(1)
            .ok_or(Error::Invalid("cursor overflow"))?;
        let committed = receipt::commit_control(
            candidate_bytes,
            &self.relay_seed,
            next_cursor as u64,
            committed_ms,
        )?;
        let next_head = crypto::hash("control-head", &committed)?;
        tx.execute(
            "INSERT INTO entries(family_id,cursor,kind,committed_bytes) VALUES(?1,?2,1,?3)",
            params![&path_family[..], next_cursor, &committed],
        )?;
        let updated = tx.execute(
            "UPDATE families SET cursor=?2,head_hash=?3 WHERE family_id=?1 AND cursor=?4 AND head_hash=?5",
            params![&path_family[..], next_cursor, &next_head[..], prefix.proved.join.cursor, &prefix.admission_head[..]],
        )?;
        if updated != 1 {
            return Err(Error::Invalid("removal compare-and-swap failed"));
        }
        tx.execute(
            "DELETE FROM staged_controls WHERE family_id=?1",
            params![&path_family[..]],
        )?;
        tx.execute(
            "DELETE FROM staged_control_objects WHERE family_id=?1",
            params![&path_family[..]],
        )?;
        tx.commit()?;
        Ok(receipt::control_commit_response(&committed)?)
    }

    /// Commit a data batch against the authenticated public Family ledger.
    /// An active author's durable outbox may refer to any control ancestor
    /// bound to the current epoch, even when newer controls have committed.
    pub fn commit_batch(
        &mut self,
        path_family: [u8; 16],
        envelope_bytes: &[u8],
    ) -> Result<Vec<u8>, Error> {
        let (claimed_id, author) = batch_authority::claimed_identity(envelope_bytes)?;
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let prior: Option<(Vec<u8>, Vec<u8>)> = tx.query_row(
            "SELECT envelope_bytes,receipt_bytes FROM batch_results WHERE family_id=?1 AND batch_id=?2",
            params![&path_family[..], &claimed_id[..]],
            |r| Ok((r.get(0)?, r.get(1)?)),
        ).optional()?;
        if let Some((old_envelope, old_receipt)) = prior {
            if old_envelope != envelope_bytes {
                return Err(Error::Invalid("batch ID reused with different bytes"));
            }
            return Ok(receipt::batch_commit_response(&old_receipt)?);
        }
        let prior_rejection: Option<(Vec<u8>, Vec<u8>)> = tx.query_row(
            "SELECT envelope_bytes,receipt_bytes FROM rejected_batch_results WHERE family_id=?1 AND batch_id=?2",
            params![&path_family[..], &claimed_id[..]],
            |r| Ok((r.get(0)?, r.get(1)?)),
        ).optional()?;
        if let Some((old_envelope, old_receipt)) = prior_rejection {
            verify_private_integrity(&tx, self.relay_public)?;
            if old_envelope != envelope_bytes {
                return Err(Error::Invalid("batch ID reused with different bytes"));
            }
            return Ok(receipt::batch_commit_response(&old_receipt)?);
        }
        let mut ledger =
            Self::verify_saved_family(&tx, path_family, self.relay_public, &self.relay_seed)?;
        let (signer, active_author) = match ledger.reader(author)? {
            Some(public_ledger::PublicReader::Manager(key))
            | Some(public_ledger::PublicReader::Active(key)) => (key, true),
            Some(public_ledger::PublicReader::Pending { signing_key, .. })
            | Some(public_ledger::PublicReader::Removed(signing_key)) => (signing_key, false),
            _ => return Err(Error::Invalid("batch author not enrolled")),
        };
        let batch =
            batch_authority::verify(envelope_bytes, path_family, ledger.relay_id(), signer)?;
        if batch.batch_id != claimed_id || batch.author_id != author {
            return Err(Error::Invalid("batch claimed identity mismatch"));
        }
        ensure_new_protocol_ids(&tx, path_family, &[batch.batch_id], &[])?;
        let known_ancestor = ledger.epoch_for_head(&batch.control_head) == Some(batch.epoch);
        let (cursor, current_head): (i64, Vec<u8>) = tx.query_row(
            "SELECT cursor,head_hash FROM families WHERE family_id=?1 AND active=1",
            params![&path_family[..]],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        let current_head: [u8; 32] = current_head
            .try_into()
            .map_err(|_| Error::Invalid("Family head length"))?;
        if current_head != ledger.head() {
            return Err(Error::Invalid("batch ledger head differs"));
        }
        let next_sequence = ledger.next_sequence(author);
        let current_epoch = ledger.current_epoch()?;
        let reason = if !active_author {
            Some(2)
        } else if batch.epoch != current_epoch {
            Some(1)
        } else if !known_ancestor {
            Some(3)
        } else if batch.sequence != next_sequence {
            Some(4)
        } else {
            None
        };
        if let Some(reason) = reason {
            verify_private_integrity(&tx, self.relay_public)?;
            let rejected = receipt::rejected_batch(
                &batch,
                u64::try_from(cursor).map_err(|_| Error::Invalid("cursor negative"))?,
                current_head,
                reason,
                next_sequence,
                &self.relay_seed,
            )?;
            tx.execute(
                "INSERT INTO rejected_batch_results(family_id,batch_id,envelope_bytes,receipt_bytes) VALUES(?1,?2,?3,?4)",
                params![&path_family[..], &batch.batch_id[..], envelope_bytes, &rejected],
            )?;
            refresh_private_integrity(&tx, &self.relay_seed)?;
            tx.commit()?;
            return Ok(receipt::batch_commit_response(&rejected)?);
        }
        let next_cursor: u64 = u64::try_from(cursor)
            .map_err(|_| Error::Invalid("cursor negative"))?
            .checked_add(1)
            .ok_or(Error::Invalid("cursor overflow"))?;
        let receipt = receipt::accepted_batch(&batch, next_cursor, &self.relay_seed)?;
        let saved = receipt::verify_stored_accepted_batch(
            envelope_bytes,
            &receipt,
            path_family,
            ledger.relay_id(),
            next_cursor,
            &self.relay_seed,
        )?;
        ledger.apply_batch(envelope_bytes, &saved)?;
        tx.execute(
            "INSERT INTO entries(family_id,cursor,kind,committed_bytes) VALUES(?1,?2,2,?3)",
            params![
                &path_family[..],
                i64::try_from(next_cursor).map_err(|_| Error::Invalid("cursor range"))?,
                envelope_bytes
            ],
        )?;
        tx.execute(
            "INSERT INTO batch_results(family_id,batch_id,author_id,sequence,envelope_bytes,receipt_bytes,cursor) VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![&path_family[..], &batch.batch_id[..], &author[..],
                i64::try_from(batch.sequence).map_err(|_| Error::Invalid("sequence range"))?,
                envelope_bytes, &receipt, i64::try_from(next_cursor).map_err(|_| Error::Invalid("cursor range"))?],
        )?;
        let updated = tx.execute(
            "UPDATE families SET cursor=?2 WHERE family_id=?1 AND cursor=?3 AND head_hash=?4",
            params![
                &path_family[..],
                i64::try_from(next_cursor).map_err(|_| Error::Invalid("cursor range"))?,
                cursor,
                &current_head[..]
            ],
        )?;
        if updated != 1 {
            return Err(Error::Invalid(
                "batch global cursor compare-and-swap failed",
            ));
        }
        tx.commit()?;
        Ok(receipt::batch_commit_response(&receipt)?)
    }
}
