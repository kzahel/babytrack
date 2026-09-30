//! Authenticated reads and replay-safe read request recording.

use super::*;

impl RelayStore {
    pub fn genesis_result(&self, family_id: [u8; 16]) -> Result<Option<Vec<u8>>, Error> {
        let saved: Option<(Vec<u8>, Vec<u8>)> = self
            .db
            .query_row(
                "SELECT candidate_bytes,committed_bytes FROM families WHERE family_id=?1 AND active=1",
                params![&family_id[..]],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let Some((candidate, committed)) = saved else {
            return Ok(None);
        };
        verify_stored_genesis(&self.db, family_id, &candidate, self.relay_public)?;
        Ok(Some(committed))
    }

    pub fn committed_object(
        &self,
        family_id: [u8; 16],
        object_id: [u8; 16],
    ) -> Result<Option<Vec<u8>>, Error> {
        Ok(self
            .db
            .query_row(
                "SELECT object_bytes FROM committed_objects WHERE family_id=?1 AND object_id=?2",
                params![&family_id[..], &object_id[..]],
                |r| r.get(0),
            )
            .optional()?)
    }

    pub fn current_control_count(&self, family: [u8; 16]) -> Result<i64, Error> {
        control_count(&self.db, family)
    }

    pub fn batch_result_authenticated(
        &mut self,
        family_id: [u8; 16],
        batch_id: [u8; 16],
        exact_path: &str,
        auth_bytes: &[u8],
    ) -> Result<Vec<u8>, Error> {
        let expected = format!(
            "/v1/families/{}/batch-results/{}",
            lower_hex(&family_id),
            lower_hex(&batch_id)
        );
        if exact_path != expected {
            return Err(Error::Invalid("batch result path not canonical"));
        }
        let reader = self.verify_control_reader(family_id, exact_path, auth_bytes)?;
        if !matches!(
            reader,
            ControlReader::Manager | ControlReader::Active | ControlReader::Removed { .. }
        ) {
            return Err(Error::Invalid("reader cannot fetch batch results"));
        }
        let accepted: Option<(Vec<u8>, Vec<u8>)> = self
            .db
            .query_row(
                "SELECT author_id,receipt_bytes FROM batch_results WHERE family_id=?1 AND batch_id=?2",
                params![&family_id[..], &batch_id[..]],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let receipt = if let Some((author, receipt)) = accepted {
            if let ControlReader::Removed { device_id, .. } = reader
                && author.as_slice() != device_id
            {
                return Err(Error::Invalid(
                    "removed reader cannot fetch another author's result",
                ));
            }
            Some(receipt)
        } else {
            let rejected: Option<(Vec<u8>, Vec<u8>)> = self.db.query_row(
                "SELECT envelope_bytes,receipt_bytes FROM rejected_batch_results WHERE family_id=?1 AND batch_id=?2",
                params![&family_id[..], &batch_id[..]],
                |r| Ok((r.get(0)?, r.get(1)?)),
            ).optional()?;
            match rejected {
                Some((envelope, receipt)) => {
                    verify_private_integrity(&self.db, self.relay_public)?;
                    if let ControlReader::Removed {
                        device_id,
                        signing_public,
                        relay_id,
                    } = reader
                    {
                        let batch = batch_authority::verify(
                            &envelope,
                            family_id,
                            relay_id,
                            signing_public,
                        )?;
                        if batch.author_id != device_id || batch.batch_id != batch_id {
                            return Err(Error::Invalid(
                                "removed reader cannot fetch another author's result",
                            ));
                        }
                    }
                    Some(receipt)
                }
                None => None,
            }
        };
        Ok(cbor::encode(&Value::Map(vec![
            (1, Value::Integer(1)),
            (2, receipt.map_or(Value::Null, Value::Bytes)),
        ]))?)
    }

    pub fn promotion_result_authenticated(
        &mut self,
        family_id: [u8; 16],
        promotion_id: [u8; 16],
        exact_path: &str,
        auth_bytes: &[u8],
    ) -> Result<Vec<u8>, Error> {
        let expected_path = format!(
            "/v1/families/{}/promotions/{}",
            lower_hex(&family_id),
            lower_hex(&promotion_id)
        );
        if exact_path != expected_path {
            return Err(Error::Invalid("promotion path is not canonical"));
        }
        let saved: Option<(Vec<u8>, i64, Option<Vec<u8>>)> = self
            .db
            .query_row(
                "SELECT candidate_bytes,active,committed_bytes FROM families WHERE family_id=?1",
                params![&family_id[..]],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        let Some((candidate_bytes, active, committed)) = saved else {
            return Err(Error::Invalid("Family has no genesis reservation"));
        };
        let candidate = if active == 1 {
            verify_stored_genesis(&self.db, family_id, &candidate_bytes, self.relay_public)?
        } else {
            authority::verify_genesis_candidate(&candidate_bytes, &self.relay_public)?
        };
        if candidate.manifest.first().map(|entry| entry.object_id) != Some(promotion_id) {
            return Err(Error::Invalid("promotion ID mismatch"));
        }
        let verified = read_auth::verify_get(
            auth_bytes,
            family_id,
            candidate.relay_id,
            candidate.manager_id,
            candidate.manager_signing_key,
            exact_path,
        )?;
        self.record_read_id(family_id, &verified)?;
        let body = cbor::encode(&Value::Map(vec![
            (1, Value::Integer(1)),
            (
                2,
                if active == 1 {
                    Value::Bytes(committed.ok_or(Error::Invalid("active genesis missing bytes"))?)
                } else {
                    Value::Null
                },
            ),
        ]))?;
        Ok(body)
    }

    pub fn control_page_authenticated(
        &mut self,
        family_id: [u8; 16],
        after: u64,
        exact_path: &str,
        auth_bytes: &[u8],
    ) -> Result<Vec<u8>, Error> {
        let expected_path = format!(
            "/v1/families/{}/control?after={after}",
            lower_hex(&family_id)
        );
        if exact_path != expected_path {
            return Err(Error::Invalid("control read path not canonical"));
        }
        self.verify_control_reader(family_id, exact_path, auth_bytes)?;
        let (entries, has_more) = load_page_entries(&self.db, family_id, after, Some(1))?;
        Ok(receipt::encode_control_page(
            family_id, after, &entries, has_more,
        )?)
    }

    pub fn batch_page_authenticated(
        &mut self,
        family_id: [u8; 16],
        after: u64,
        exact_path: &str,
        auth_bytes: &[u8],
    ) -> Result<Vec<u8>, Error> {
        let expected = format!(
            "/v1/families/{}/batches?after={after}",
            lower_hex(&family_id)
        );
        if exact_path != expected {
            return Err(Error::Invalid("batch page path not canonical"));
        }
        if !matches!(
            self.verify_control_reader(family_id, exact_path, auth_bytes)?,
            ControlReader::Manager | ControlReader::Active
        ) {
            return Err(Error::Invalid("reader cannot fetch batch page"));
        }
        let (entries, has_more) = load_page_entries(&self.db, family_id, after, Some(2))?;
        Ok(receipt::encode_batch_page(
            family_id, after, &entries, has_more,
        )?)
    }

    pub fn control_result_authenticated(
        &mut self,
        family_id: [u8; 16],
        transition_id: [u8; 16],
        exact_path: &str,
        auth_bytes: &[u8],
    ) -> Result<Vec<u8>, Error> {
        let expected = format!(
            "/v1/families/{}/control-results/{}",
            lower_hex(&family_id),
            lower_hex(&transition_id)
        );
        if exact_path != expected {
            return Err(Error::Invalid("control result path not canonical"));
        }
        self.verify_control_reader(family_id, exact_path, auth_bytes)?;
        let mut stmt = self.db.prepare(
            "SELECT committed_bytes FROM entries WHERE family_id=?1 AND kind=1 ORDER BY cursor",
        )?;
        let rows = stmt.query_map([&family_id[..]], |row| row.get::<_, Vec<u8>>(0))?;
        let mut found = None;
        for row in rows {
            let bytes = row?;
            let ids = control_birth_ids(&bytes)?;
            if ids.first() == Some(&transition_id) {
                found = Some(bytes);
                break;
            }
        }
        Ok(cbor::encode(&Value::Map(vec![
            (1, Value::Integer(1)),
            (2, found.map_or(Value::Null, Value::Bytes)),
        ]))?)
    }

    pub fn invite_authenticated(
        &mut self,
        family_id: [u8; 16],
        invitation_id: [u8; 16],
        exact_path: &str,
        auth_bytes: &[u8],
    ) -> Result<Vec<u8>, Error> {
        let expected = format!(
            "/v1/families/{}/invites/{}",
            lower_hex(&family_id),
            lower_hex(&invitation_id)
        );
        if exact_path != expected {
            return Err(Error::Invalid("invite path not canonical"));
        }
        let reader = self.verify_control_reader(family_id, exact_path, auth_bytes)?;
        let mut controls = self.db.prepare(
            "SELECT committed_bytes FROM entries WHERE family_id=?1 AND kind=1 ORDER BY cursor",
        )?;
        let rows = controls.query_map(params![&family_id[..]], |row| row.get::<_, Vec<u8>>(0))?;
        let mut issue = None;
        for row in rows {
            let committed = row?;
            if control_birth_ids(&committed)?.last() == Some(&invitation_id) {
                issue = Some(committed);
                break;
            }
        }
        let issue = issue.ok_or(Error::Invalid("invitation not committed"))?;
        if matches!(reader, ControlReader::Invitation { .. })
            && read_auth::claimed_signer(auth_bytes)? != invitation_id
        {
            return Err(Error::Invalid("invitation signer differs"));
        }
        if !matches!(
            reader,
            ControlReader::Manager | ControlReader::Active | ControlReader::Invitation { .. }
        ) {
            return Err(Error::Invalid("reader cannot fetch invitation"));
        }
        Ok(cbor::encode(&Value::Map(vec![
            (1, Value::Integer(1)),
            (2, Value::Bytes(issue)),
        ]))?)
    }

    pub fn invitation_status_authenticated(
        &mut self,
        family_id: [u8; 16],
        invitation_id: [u8; 16],
        exact_path: &str,
        auth_bytes: &[u8],
        now_ms: i64,
    ) -> Result<Vec<u8>, Error> {
        let expected = format!(
            "/v1/families/{}/invitation-status/{}",
            lower_hex(&family_id),
            lower_hex(&invitation_id)
        );
        if exact_path != expected {
            return Err(Error::Invalid("invitation status path not canonical"));
        }
        let ledger =
            Self::verify_saved_family(&self.db, family_id, self.relay_public, &self.relay_seed)?;
        if read_auth::claimed_signer(auth_bytes)? != invitation_id {
            return Err(Error::Invalid("status signer differs from invitation"));
        }
        let observed_ms = now_ms.max(ledger.last_commit_ms());
        let (signing_key, reason) = ledger
            .invitation_status(invitation_id, observed_ms)?
            .ok_or(Error::Invalid("invitation not committed"))?;
        let verified = read_auth::verify_get(
            auth_bytes,
            family_id,
            ledger.relay_id(),
            invitation_id,
            signing_key,
            exact_path,
        )?;
        self.record_read_id(family_id, &verified)?;
        let (cursor, stored_head): (i64, Vec<u8>) = self.db.query_row(
            "SELECT cursor,head_hash FROM families WHERE family_id=?1 AND active=1",
            [&family_id[..]],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        if stored_head.as_slice() != ledger.head() {
            return Err(Error::Invalid("status head differs from ledger"));
        }
        let mut issue = None;
        let mut controls = self.db.prepare(
            "SELECT committed_bytes FROM entries WHERE family_id=?1 AND kind=1 ORDER BY cursor",
        )?;
        let rows = controls.query_map([&family_id[..]], |row| row.get::<_, Vec<u8>>(0))?;
        for row in rows {
            let committed = row?;
            if control_birth_ids(&committed)?.last() == Some(&invitation_id) {
                issue = Some(committed);
                break;
            }
        }
        let issue = issue.ok_or(Error::Invalid("invitation issue missing"))?;
        let Value::Map(parts) = cbor::decode(&issue)? else {
            return Err(Error::Invalid("invitation issue not map"));
        };
        let signed = cbor::encode(&Value::Array(vec![parts[0].1.clone(), parts[1].1.clone()]))?;
        let issue_hash = crypto::hash("control-signed", &signed)?;
        let body = cbor::encode(&Value::Array(vec![
            Value::Integer(1),
            Value::Bytes(family_id.to_vec()),
            Value::Bytes(ledger.relay_id().to_vec()),
            Value::Bytes(invitation_id.to_vec()),
            Value::Integer(reason.into()),
            Value::Integer(cursor.into()),
            Value::Bytes(ledger.head().to_vec()),
            Value::Integer(observed_ms.into()),
            Value::Bytes(issue_hash.to_vec()),
        ]))?;
        let signature = crypto::sign_cbor("invitation-status", &body, &self.relay_seed)?;
        Ok(cbor::encode(&Value::Map(vec![
            (1, Value::Integer(1)),
            (2, Value::Bytes(body)),
            (3, Value::Bytes(signature.to_vec())),
        ]))?)
    }

    pub fn log_page_authenticated(
        &mut self,
        family_id: [u8; 16],
        after: u64,
        exact_path: &str,
        auth_bytes: &[u8],
    ) -> Result<Vec<u8>, Error> {
        let expected = format!("/v1/families/{}/log?after={after}", lower_hex(&family_id));
        if exact_path != expected {
            return Err(Error::Invalid("log read path not canonical"));
        }
        match self.verify_control_reader(family_id, exact_path, auth_bytes)? {
            ControlReader::Manager | ControlReader::Active => {}
            _ => return Err(Error::Invalid("reader cannot fetch full log")),
        }
        let (entries, has_more) = load_page_entries(&self.db, family_id, after, None)?;
        Ok(receipt::encode_log_page(
            family_id, after, &entries, has_more,
        )?)
    }

    pub fn object_authenticated(
        &mut self,
        family_id: [u8; 16],
        object_id: [u8; 16],
        exact_path: &str,
        auth_bytes: &[u8],
    ) -> Result<Vec<u8>, Error> {
        let expected_path = format!(
            "/v1/families/{}/objects/{}",
            lower_hex(&family_id),
            lower_hex(&object_id)
        );
        if exact_path != expected_path {
            return Err(Error::Invalid("object read path not canonical"));
        }
        let reader = self.verify_control_reader(family_id, exact_path, auth_bytes)?;
        match reader {
            ControlReader::Manager | ControlReader::Active => {}
            ControlReader::Invitation { issue_object } if issue_object == object_id => {}
            ControlReader::Pending {
                challenge_object: Some(id),
            } if id == object_id => {}
            _ => return Err(Error::Invalid("reader cannot fetch this object")),
        }
        let object: Option<(i64,Vec<u8>,Vec<u8>)> = self.db.query_row(
            "SELECT kind,object_bytes,transition_id FROM committed_objects WHERE family_id=?1 AND object_id=?2",
            params![&family_id[..],&object_id[..]], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?)),
        ).optional()?;
        let Some((kind, bytes, transition)) = object else {
            return Err(Error::Invalid("object not committed"));
        };
        Ok(cbor::encode(&Value::Map(vec![
            (1, Value::Integer(1)),
            (2, Value::Integer(kind.into())),
            (3, Value::Bytes(object_id.to_vec())),
            (4, Value::Bytes(bytes)),
            (5, Value::Bytes(transition)),
        ]))?)
    }

    pub(super) fn verify_control_reader(
        &mut self,
        family_id: [u8; 16],
        exact_path: &str,
        auth_bytes: &[u8],
    ) -> Result<ControlReader, Error> {
        let ledger =
            Self::verify_saved_family(&self.db, family_id, self.relay_public, &self.relay_seed)?;
        let signer = read_auth::claimed_signer(auth_bytes)?;
        let (reader, signing_key) = match ledger
            .reader(signer)?
            .ok_or(Error::Invalid("reader has no current control access"))?
        {
            public_ledger::PublicReader::Manager(key) => (ControlReader::Manager, key),
            public_ledger::PublicReader::Active(key) => (ControlReader::Active, key),
            public_ledger::PublicReader::Removed(key) => (
                ControlReader::Removed {
                    device_id: signer,
                    signing_public: key,
                    relay_id: ledger.relay_id(),
                },
                key,
            ),
            public_ledger::PublicReader::Invitation {
                signing_key,
                issue_object,
            } => (ControlReader::Invitation { issue_object }, signing_key),
            public_ledger::PublicReader::Pending {
                signing_key,
                challenge_object,
            } => (ControlReader::Pending { challenge_object }, signing_key),
        };
        let verified = read_auth::verify_get(
            auth_bytes,
            family_id,
            ledger.relay_id(),
            signer,
            signing_key,
            exact_path,
        )?;
        self.record_read_id(family_id, &verified)?;
        Ok(reader)
    }

    pub(super) fn record_read_id(
        &mut self,
        family_id: [u8; 16],
        verified: &read_auth::VerifiedRead,
    ) -> Result<(), Error> {
        let tx = self.db.transaction()?;
        let old:Option<Vec<u8>>=tx.query_row(
            "SELECT request_hash FROM read_requests WHERE family_id=?1 AND signer_id=?2 AND request_id=?3",
            params![&family_id[..],&verified.signer_id[..],&verified.request_id[..]],|r|r.get(0),
        ).optional()?;
        if let Some(hash) = old {
            if hash != verified.request_hash {
                return Err(Error::Invalid("request ID reused with different bytes"));
            }
        } else {
            tx.execute("INSERT INTO read_requests(family_id,signer_id,request_id,request_hash) VALUES(?1,?2,?3,?4)",
                params![&family_id[..],&verified.signer_id[..],&verified.request_id[..],&verified.request_hash[..]])?;
        }
        tx.commit()?;
        Ok(())
    }
}
