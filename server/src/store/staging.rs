//! Opaque object reservation and staging validation.

use super::*;

impl RelayStore {
    pub fn stage_genesis_object(
        &mut self,
        path_family: [u8; 16],
        path_object: [u8; 16],
        body: &[u8],
    ) -> Result<Vec<u8>, Error> {
        let value = cbor::decode_with_limits(
            body,
            cbor::Limits {
                max_bytes: 2 * 1024 * 1024,
                max_depth: 16,
            },
        )?;
        let Value::Map(fields) = value else {
            return Err(Error::Invalid("stage body not map"));
        };
        if fields.len() != 6
            || fields
                .iter()
                .enumerate()
                .any(|(i, (key, _))| *key != i as u64 + 1)
            || fields[0].1 != Value::Integer(1)
        {
            return Err(Error::Invalid("stage body keys or version"));
        }
        let candidate_bytes = cbor::encode(&Value::Map(vec![
            (1, fields[1].1.clone()),
            (2, fields[2].1.clone()),
        ]))?;
        let candidate = authority::verify_genesis_candidate(&candidate_bytes, &self.relay_public)?;
        let kind: u16 = number(&fields[3].1)?
            .try_into()
            .map_err(|_| Error::Invalid("object kind range"))?;
        let object_id = fixed::<16>(&fields[4].1)?;
        if candidate.family_id != path_family || object_id != path_object {
            return Err(Error::Invalid(
                "stage path differs from signed candidate or object",
            ));
        }
        let Value::Bytes(object_bytes) = &fields[5].1 else {
            return Err(Error::Invalid("object not bytes"));
        };
        let hash = crypto::hash("object", object_bytes)?;
        let listed = candidate
            .manifest
            .iter()
            .find(|entry| entry.kind == kind && entry.object_id == object_id)
            .ok_or(Error::Invalid("object absent from signed manifest"))?;
        if listed.object_hash != hash
            || usize::try_from(listed.object_len).ok() != Some(object_bytes.len())
        {
            return Err(Error::Invalid("object differs from signed manifest"));
        }
        let reservation = crypto::hash("genesis-reservation", &candidate_bytes)?;
        let tx = self.db.transaction()?;
        verify_private_integrity(&tx, self.relay_public)?;
        let existing: Option<(Vec<u8>, Vec<u8>, i64)> = tx
            .query_row(
                "SELECT reservation_hash, candidate_bytes, active FROM families WHERE family_id=?1",
                params![&candidate.family_id[..]],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        if let Some((saved_hash, saved_candidate, active)) = existing {
            if saved_hash != reservation || saved_candidate != candidate_bytes {
                return Err(Error::Invalid("Family ID reserved by another genesis"));
            }
            if active == 1 {
                let committed: Option<Vec<u8>> = tx.query_row(
                    "SELECT object_bytes FROM committed_objects WHERE family_id=?1 AND object_id=?2",
                    params![&candidate.family_id[..], &object_id[..]], |r| r.get(0),
                ).optional()?;
                if committed.as_deref() == Some(object_bytes) {
                    return Ok(receipt::object_stage_response(object_bytes)?);
                }
                return Err(Error::Invalid("committed object differs from staged retry"));
            }
        } else {
            tx.execute(
                "INSERT INTO families(family_id,reservation_hash,candidate_bytes,active,cursor) VALUES(?1,?2,?3,0,0)",
                params![&candidate.family_id[..], &reservation[..], &candidate_bytes],
            )?;
        }
        reserve_staged_object(&tx, candidate.family_id, object_id, kind, hash)?;
        let prior: Option<(i64, Vec<u8>, Vec<u8>)> = tx.query_row(
            "SELECT kind,object_hash,object_bytes FROM staged_objects WHERE family_id=?1 AND object_id=?2",
            params![&candidate.family_id[..], &object_id[..]], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?)),
        ).optional()?;
        if let Some((saved_kind, saved_hash, saved_bytes)) = prior {
            if saved_kind != i64::from(kind) || saved_hash != hash || saved_bytes != *object_bytes {
                return Err(Error::Invalid("object ID reused with different bytes"));
            }
        } else {
            tx.execute(
                "INSERT INTO staged_objects(family_id,object_id,kind,object_hash,object_bytes) VALUES(?1,?2,?3,?4,?5)",
                params![&candidate.family_id[..], &object_id[..], i64::from(kind), &hash[..], object_bytes],
            )?;
        }
        refresh_private_integrity(&tx, &self.relay_seed)?;
        tx.commit()?;
        Ok(receipt::object_stage_response(object_bytes)?)
    }

    pub fn stage_first_issue_object(
        &mut self,
        path_family: [u8; 16],
        path_object: [u8; 16],
        body: &[u8],
    ) -> Result<Vec<u8>, Error> {
        let value = cbor::decode_with_limits(
            body,
            cbor::Limits {
                max_bytes: 2 * 1024 * 1024,
                max_depth: 16,
            },
        )?;
        let Value::Map(fields) = value else {
            return Err(Error::Invalid("stage body not map"));
        };
        if fields.len() != 6
            || fields
                .iter()
                .enumerate()
                .any(|(i, (key, _))| *key != i as u64 + 1)
            || fields[0].1 != Value::Integer(1)
        {
            return Err(Error::Invalid("stage keys or version"));
        }
        let candidate_bytes = cbor::encode(&Value::Map(vec![
            (1, fields[1].1.clone()),
            (2, fields[2].1.clone()),
        ]))?;
        let kind: u16 = number(&fields[3].1)?
            .try_into()
            .map_err(|_| Error::Invalid("kind range"))?;
        let object_id = fixed::<16>(&fields[4].1)?;
        let Value::Bytes(object_bytes) = &fields[5].1 else {
            return Err(Error::Invalid("object not bytes"));
        };
        let (genesis_bytes, active, cursor, head) = self.db.query_row(
            "SELECT candidate_bytes,active,cursor,head_hash FROM families WHERE family_id=?1",
            params![&path_family[..]],
            |r| {
                Ok((
                    r.get::<_, Vec<u8>>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, Option<Vec<u8>>>(3)?,
                ))
            },
        )?;
        if active != 1 {
            return Err(Error::Invalid("Family not promoted"));
        }
        let genesis =
            verify_stored_genesis(&self.db, path_family, &genesis_bytes, self.relay_public)?;
        let controls = control_count(&self.db, path_family)?;
        let head: [u8; 32] = head
            .ok_or(Error::Invalid("Family head absent"))?
            .try_into()
            .map_err(|_| Error::Invalid("head length"))?;
        let issue = authority::verify_first_invite_issue(
            &candidate_bytes,
            &genesis,
            crypto::hash("control-head", &control_at(&self.db, path_family, 0)?)?,
        )?;
        if issue.family_id != path_family
            || issue.manifest.object_id != path_object
            || issue.manifest.object_id != object_id
            || issue.manifest.kind != kind
            || issue.manifest.object_hash != crypto::hash("object", object_bytes)?
            || issue.manifest.object_len as usize != object_bytes.len()
        {
            return Err(Error::Invalid("issue stage differs from path or manifest"));
        }
        if controls >= 2 {
            let first_issue = control_at(&self.db, path_family, 1)?;
            let existing: Option<Vec<u8>> = self.db.query_row(
                "SELECT object_bytes FROM committed_objects WHERE family_id=?1 AND object_id=?2 AND transition_id=?3",
                params![&path_family[..],&object_id[..],&issue.transition_id[..]], |r| r.get(0),
            ).optional()?;
            if control_candidate(&first_issue)? == candidate_bytes
                && existing.as_deref() == Some(object_bytes)
            {
                return Ok(receipt::object_stage_response(object_bytes)?);
            }
            return Err(Error::Invalid("issue already committed with other bytes"));
        }
        if controls != 1
            || cursor < 1
            || head != crypto::hash("control-head", &control_at(&self.db, path_family, 0)?)?
        {
            return Err(Error::Invalid("first issue only"));
        }
        let tx = self.db.transaction()?;
        verify_private_integrity(&tx, self.relay_public)?;
        stage_control_object(
            &tx,
            path_family,
            issue.transition_id,
            &candidate_bytes,
            &issue.manifest,
            object_bytes,
        )?;
        refresh_private_integrity(&tx, &self.relay_seed)?;
        tx.commit()?;
        Ok(receipt::object_stage_response(object_bytes)?)
    }

    /// Stage one object for a later control against the current public ledger.
    /// The signed candidate supplies the manifest; ciphertext stays opaque.
    pub fn stage_general_control_object(
        &mut self,
        family: [u8; 16],
        path_object: [u8; 16],
        body: &[u8],
    ) -> Result<Vec<u8>, Error> {
        let (candidate, kind, object_id, object_bytes) = stage_parts(body)?;
        if object_id != path_object {
            return Err(Error::Invalid("staged object path differs"));
        }
        let ids = control_birth_ids(&candidate)?;
        let transition = ids[0];
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        verify_private_integrity(&tx, self.relay_public)?;
        let mut controls = tx.prepare(
            "SELECT committed_bytes FROM entries WHERE family_id=?1 AND kind=1 ORDER BY cursor",
        )?;
        let rows = controls.query_map(params![&family[..]], |row| row.get::<_, Vec<u8>>(0))?;
        for row in rows {
            let committed = row?;
            if control_birth_ids(&committed)?.first() == Some(&transition) {
                if control_candidate(&committed)? != candidate {
                    return Err(Error::Invalid("transition ID reused with different bytes"));
                }
                let existing: Option<Vec<u8>> = tx.query_row(
                    "SELECT object_bytes FROM committed_objects WHERE family_id=?1 AND object_id=?2 AND transition_id=?3",
                    params![&family[..], &object_id[..], &transition[..]],
                    |row| row.get(0),
                ).optional()?;
                if existing.as_deref() == Some(&object_bytes) {
                    return Ok(receipt::object_stage_response(&object_bytes)?);
                }
                return Err(Error::Invalid("committed object differs from retry"));
            }
        }
        drop(controls);
        let mut ledger =
            Self::verify_saved_family(&tx, family, self.relay_public, &self.relay_seed)?;
        let cursor: i64 = tx.query_row(
            "SELECT cursor FROM families WHERE family_id=?1 AND active=1",
            params![&family[..]],
            |row| row.get(0),
        )?;
        let provisional = receipt::commit_control(
            &candidate,
            &self.relay_seed,
            u64::try_from(cursor)
                .map_err(|_| Error::Invalid("cursor range"))?
                .checked_add(1)
                .ok_or(Error::Invalid("cursor overflow"))?,
            ledger.last_commit_ms(),
        )?;
        let verified = receipt::verify_control_receipt(&provisional, &self.relay_public)?;
        if !matches!(verified.kind, 2 | 6 | 8 | 10 | 11)
            || verified.family_id != family
            || verified.transition_id != transition
        {
            return Err(Error::Invalid("not a staged general control"));
        }
        let prior_state = ledger.state().clone();
        ledger.apply(&verified, &provisional)?;
        ensure_control_ids(&tx, family, &candidate)?;
        let listed = verified
            .manifest
            .iter()
            .find(|entry| entry.id == object_id && entry.kind == kind)
            .ok_or(Error::Invalid("object absent from signed manifest"))?;
        if listed.hash != crypto::hash("object", &object_bytes)?
            || usize::try_from(listed.len).ok() != Some(object_bytes.len())
        {
            return Err(Error::Invalid("staged object differs from manifest"));
        }
        validate_general_grant_object(&candidate, &prior_state, kind, object_id, &object_bytes)?;
        let entry = authority::ManifestEntry {
            kind: listed.kind,
            object_id: listed.id,
            object_hash: listed.hash,
            object_len: listed.len,
        };
        stage_control_object(&tx, family, transition, &candidate, &entry, &object_bytes)?;
        refresh_private_integrity(&tx, &self.relay_seed)?;
        tx.commit()?;
        Ok(receipt::object_stage_response(&object_bytes)?)
    }

    pub fn stage_first_challenge_object(
        &mut self,
        path_family: [u8; 16],
        path_object: [u8; 16],
        body: &[u8],
    ) -> Result<Vec<u8>, Error> {
        let (candidate_bytes, kind, object_id, object_bytes) = stage_parts(body)?;
        let prefix = load_join_prefix(&self.db, self.relay_public, path_family)?;
        let challenge = authority::verify_first_challenge(
            &candidate_bytes,
            &prefix.genesis,
            &prefix.issue,
            &prefix.claim,
            prefix.claim_head,
        )?;
        let listed = challenge
            .manifest
            .iter()
            .find(|entry| entry.kind == kind && entry.object_id == object_id)
            .ok_or(Error::Invalid("challenge object absent from manifest"))?;
        if object_id != path_object
            || listed.object_hash != crypto::hash("object", &object_bytes)?
            || listed.object_len as usize != object_bytes.len()
        {
            return Err(Error::Invalid("challenge object path or bytes mismatch"));
        }
        if prefix.controls >= 4 {
            let committed = control_at(&self.db, path_family, 3)?;
            let existing:Option<Vec<u8>>=self.db.query_row(
                "SELECT object_bytes FROM committed_objects WHERE family_id=?1 AND object_id=?2 AND transition_id=?3",
                params![&path_family[..],&object_id[..],&challenge.transition_id[..]],|r|r.get(0),
            ).optional()?;
            if control_candidate(&committed)? == candidate_bytes
                && existing.as_deref() == Some(&object_bytes)
            {
                return Ok(receipt::object_stage_response(&object_bytes)?);
            }
            return Err(Error::Invalid("challenge already committed differently"));
        }
        if prefix.controls != 3 || prefix.head != prefix.claim_head {
            return Err(Error::Invalid("challenge head stale"));
        }
        let tx = self.db.transaction()?;
        verify_private_integrity(&tx, self.relay_public)?;
        stage_control_object(
            &tx,
            path_family,
            challenge.transition_id,
            &candidate_bytes,
            listed,
            &object_bytes,
        )?;
        refresh_private_integrity(&tx, &self.relay_seed)?;
        tx.commit()?;
        Ok(receipt::object_stage_response(&object_bytes)?)
    }

    pub fn stage_first_admission_object(
        &mut self,
        path_family: [u8; 16],
        path_object: [u8; 16],
        body: &[u8],
    ) -> Result<Vec<u8>, Error> {
        let (candidate_bytes, kind, object_id, object_bytes) = stage_parts(body)?;
        let prefix = load_proved_prefix(&self.db, self.relay_public, path_family)?;
        let admission = authority::verify_first_admission(
            &candidate_bytes,
            &prefix.join.genesis,
            &prefix.join.issue,
            &prefix.join.claim,
            &prefix.challenge,
            &prefix.proof,
            prefix.proof_head,
        )?;
        let listed = admission
            .manifest
            .iter()
            .find(|entry| entry.kind == kind && entry.object_id == object_id)
            .ok_or(Error::Invalid("admission object absent from manifest"))?;
        if object_id != path_object
            || listed.object_hash != crypto::hash("object", &object_bytes)?
            || listed.object_len as usize != object_bytes.len()
        {
            return Err(Error::Invalid("admission object path or bytes mismatch"));
        }
        if prefix.join.controls >= 6 {
            let committed = control_at(&self.db, path_family, 5)?;
            let existing:Option<Vec<u8>>=self.db.query_row(
                "SELECT object_bytes FROM committed_objects WHERE family_id=?1 AND object_id=?2 AND transition_id=?3",
                params![&path_family[..],&object_id[..],&admission.transition_id[..]],|r|r.get(0),
            ).optional()?;
            if control_candidate(&committed)? == candidate_bytes
                && existing.as_deref() == Some(&object_bytes)
            {
                return Ok(receipt::object_stage_response(&object_bytes)?);
            }
            return Err(Error::Invalid("admission already committed differently"));
        }
        if prefix.join.controls != 5 || prefix.join.head != prefix.proof_head {
            return Err(Error::Invalid("admission head stale"));
        }
        if kind == 4 {
            authority::validate_first_admission_grant(&admission, object_id, &object_bytes)?;
        }
        let tx = self.db.transaction()?;
        verify_private_integrity(&tx, self.relay_public)?;
        stage_control_object(
            &tx,
            path_family,
            admission.transition_id,
            &candidate_bytes,
            listed,
            &object_bytes,
        )?;
        refresh_private_integrity(&tx, &self.relay_seed)?;
        tx.commit()?;
        Ok(receipt::object_stage_response(&object_bytes)?)
    }

    pub fn stage_first_removal_object(
        &mut self,
        path_family: [u8; 16],
        path_object: [u8; 16],
        body: &[u8],
    ) -> Result<Vec<u8>, Error> {
        let (candidate_bytes, kind, object_id, object_bytes) = stage_parts(body)?;
        let prefix = load_admitted_prefix(&self.db, self.relay_public, path_family)?;
        let removal = authority::verify_first_removal(
            &candidate_bytes,
            &prefix.proved.join.genesis,
            &prefix.proved.join.issue,
            &prefix.proved.join.claim,
            &prefix.admission,
            prefix.admission_head,
        )?;
        let listed = removal
            .manifest
            .iter()
            .find(|entry| entry.kind == kind && entry.object_id == object_id)
            .ok_or(Error::Invalid("removal object absent from manifest"))?;
        if object_id != path_object
            || listed.object_hash != crypto::hash("object", &object_bytes)?
            || listed.object_len as usize != object_bytes.len()
        {
            return Err(Error::Invalid("removal object path or bytes mismatch"));
        }
        validate_first_removal_object(
            kind,
            object_id,
            &object_bytes,
            &candidate_bytes,
            &prefix.proved.join.genesis,
        )?;
        if prefix.proved.join.controls >= 7 {
            let committed = control_at(&self.db, path_family, 6)?;
            let existing: Option<Vec<u8>> = self.db.query_row(
                "SELECT object_bytes FROM committed_objects WHERE family_id=?1 AND object_id=?2 AND transition_id=?3",
                params![&path_family[..], &object_id[..], &removal.transition_id[..]], |r| r.get(0),
            ).optional()?;
            if control_candidate(&committed)? == candidate_bytes
                && existing.as_deref() == Some(&object_bytes)
            {
                return Ok(receipt::object_stage_response(&object_bytes)?);
            }
            return Err(Error::Invalid("removal already committed differently"));
        }
        if prefix.proved.join.controls != 6 || prefix.proved.join.head != prefix.admission_head {
            return Err(Error::Invalid("removal head stale"));
        }
        ensure_first_removal_ids_unused(&self.db, path_family, &removal)?;
        let tx = self.db.transaction()?;
        verify_private_integrity(&tx, self.relay_public)?;
        stage_control_object(
            &tx,
            path_family,
            removal.transition_id,
            &candidate_bytes,
            listed,
            &object_bytes,
        )?;
        refresh_private_integrity(&tx, &self.relay_seed)?;
        tx.commit()?;
        Ok(receipt::object_stage_response(&object_bytes)?)
    }
}
