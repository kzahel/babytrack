//! Enrollment persistence. Each method retains its complete transaction.

use super::*;

impl SqliteStore {
    pub(crate) fn create_enrollment_attempt(
        &mut self,
        row: &EnrollmentRow,
        sparse_controls: &[(u64, Vec<u8>)],
        relay_public_key: [u8; 32],
        genesis_head: [u8; 32],
    ) -> Result<(), Error> {
        if !ids::is_v4(&row.family.family_id)
            || !ids::is_v4(&row.family.device_id)
            || !ids::is_v4(&row.invitation_id)
        {
            return Err(Error::InvalidId);
        }
        let transaction = self.connection.transaction()?;
        transaction.execute(
            "INSERT INTO families(family_id, device_id) VALUES (?1, ?2)",
            params![
                row.family.family_id.as_slice(),
                row.family.device_id.as_slice()
            ],
        )?;
        transaction.execute(
            "INSERT INTO local_sync_state(family_id) VALUES (?1)",
            [row.family.family_id.as_slice()],
        )?;
        transaction.execute(
            "INSERT INTO shared_roots
             (family_id, relay_public_key, genesis_bytes, pinned_cursor, pinned_head)
             VALUES (?1, ?2, ?3, 1, ?4)",
            params![
                row.family.family_id.as_slice(),
                relay_public_key.as_slice(),
                &row.genesis_bytes,
                genesis_head.as_slice(),
            ],
        )?;
        transaction.execute(
            "INSERT INTO enrollment_attempts
             (family_id, device_id, invitation_id, genesis_bytes, issue_bytes,
              candidate_bytes, secret_nonce, secret_ciphertext)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                row.family.family_id.as_slice(),
                row.family.device_id.as_slice(),
                row.invitation_id.as_slice(),
                &row.genesis_bytes,
                &row.issue_bytes,
                &row.candidate_bytes,
                row.secret_nonce.as_slice(),
                &row.secret_ciphertext
            ],
        )?;
        for (cursor, bytes) in sparse_controls {
            transaction.execute(
                "INSERT INTO enrollment_controls(family_id,cursor,committed_bytes) VALUES(?1,?2,?3)",
                params![
                    row.family.family_id.as_slice(),
                    i64::try_from(*cursor).map_err(|_| Error::CorruptState)?,
                    bytes,
                ],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub(crate) fn enrollment_controls(
        &self,
        family: FamilyHandle,
    ) -> Result<Vec<(u64, Vec<u8>)>, Error> {
        let _ = checked_family(&self.connection, family)?;
        let mut query = self.connection.prepare(
            "SELECT cursor,committed_bytes FROM enrollment_controls
             WHERE family_id=?1 ORDER BY cursor",
        )?;
        query
            .query_map([family.family_id.as_slice()], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, Vec<u8>>(1)?))
            })?
            .map(|result| {
                let (cursor, bytes) = result?;
                Ok((
                    u64::try_from(cursor).map_err(|_| Error::CorruptState)?,
                    bytes,
                ))
            })
            .collect()
    }

    pub(crate) fn append_enrollment_control(
        &mut self,
        family: FamilyHandle,
        cursor: u64,
        bytes: &[u8],
    ) -> Result<(), Error> {
        let tx = self.connection.transaction()?;
        let _ = checked_family(&tx, family)?;
        let cursor = i64::try_from(cursor).map_err(|_| Error::CorruptState)?;
        tx.execute(
            "INSERT OR IGNORE INTO enrollment_controls(family_id,cursor,committed_bytes)
             VALUES(?1,?2,?3)",
            params![family.family_id.as_slice(), cursor, bytes],
        )?;
        let prior: Vec<u8> = tx.query_row(
            "SELECT committed_bytes FROM enrollment_controls WHERE family_id=?1 AND cursor=?2",
            params![family.family_id.as_slice(), cursor],
            |row| row.get(0),
        )?;
        if prior != bytes {
            return Err(Error::CorruptState);
        }
        tx.commit()?;
        Ok(())
    }

    pub(crate) fn archived_enrollment_claims(
        &self,
        family: FamilyHandle,
    ) -> Result<Vec<Vec<u8>>, Error> {
        let _ = checked_family(&self.connection, family)?;
        let mut query = self.connection.prepare(
            "SELECT candidate_bytes FROM enrollment_claim_candidates
             WHERE family_id=?1 ORDER BY transition_id",
        )?;
        query
            .query_map([family.family_id.as_slice()], |row| row.get(0))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(Error::from)
    }

    pub(crate) fn enrollment_terminal_status(
        &self,
        family: FamilyHandle,
    ) -> Result<Option<Vec<u8>>, Error> {
        let _ = checked_family(&self.connection, family)?;
        self.connection
            .query_row(
                "SELECT response_bytes FROM enrollment_terminal_status WHERE family_id=?1",
                [family.family_id.as_slice()],
                |row| row.get(0),
            )
            .optional()
            .map_err(Error::from)
    }

    pub(crate) fn save_enrollment_terminal_status(
        &mut self,
        family: FamilyHandle,
        response: &[u8],
    ) -> Result<(), Error> {
        let tx = self.connection.transaction()?;
        let _ = checked_family(&tx, family)?;
        tx.execute(
            "INSERT OR IGNORE INTO enrollment_terminal_status(family_id,response_bytes)
             VALUES(?1,?2)",
            params![family.family_id.as_slice(), response],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Keep every superseded exact claim so an accepted lost response can
    /// still be reconciled after a newer verified head requires rebasing.
    pub(crate) fn swap_enrollment_claim(
        &mut self,
        family: FamilyHandle,
        expected: &[u8],
        expected_transition: [u8; 16],
        replacement: &[u8],
    ) -> Result<(), Error> {
        let tx = self.connection.transaction()?;
        let _ = checked_family(&tx, family)?;
        let prior: Vec<u8> = tx.query_row(
            "SELECT candidate_bytes FROM enrollment_attempts WHERE family_id=?1 AND device_id=?2",
            params![family.family_id.as_slice(), family.device_id.as_slice()],
            |row| row.get(0),
        )?;
        if prior != expected {
            return Err(Error::CorruptState);
        }
        tx.execute(
            "INSERT OR IGNORE INTO enrollment_claim_candidates(family_id,transition_id,candidate_bytes)
             VALUES(?1,?2,?3)",
            params![family.family_id.as_slice(), expected_transition.as_slice(), expected],
        )?;
        let archived: Vec<u8> = tx.query_row(
            "SELECT candidate_bytes FROM enrollment_claim_candidates
             WHERE family_id=?1 AND transition_id=?2",
            params![family.family_id.as_slice(), expected_transition.as_slice()],
            |row| row.get(0),
        )?;
        if archived != expected {
            return Err(Error::CorruptState);
        }
        tx.execute(
            "UPDATE enrollment_attempts SET candidate_bytes=?1 WHERE family_id=?2 AND device_id=?3",
            params![
                replacement,
                family.family_id.as_slice(),
                family.device_id.as_slice()
            ],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub(crate) fn enrollment_attempt(
        &self,
        family_id: [u8; 16],
    ) -> Result<Option<EnrollmentRow>, Error> {
        self.connection
            .query_row(
                "SELECT device_id, invitation_id, genesis_bytes, issue_bytes,
                        candidate_bytes, secret_nonce, secret_ciphertext
                 FROM enrollment_attempts WHERE family_id = ?1",
                [family_id.as_slice()],
                |row| {
                    Ok((
                        row.get::<_, Vec<u8>>(0)?,
                        row.get::<_, Vec<u8>>(1)?,
                        row.get::<_, Vec<u8>>(2)?,
                        row.get::<_, Vec<u8>>(3)?,
                        row.get::<_, Vec<u8>>(4)?,
                        row.get::<_, Vec<u8>>(5)?,
                        row.get::<_, Vec<u8>>(6)?,
                    ))
                },
            )
            .optional()?
            .map(|row| {
                let family = FamilyHandle {
                    family_id,
                    device_id: row.0.try_into().map_err(|_| Error::CorruptState)?,
                };
                let _ = checked_family(&self.connection, family)?;
                Ok(EnrollmentRow {
                    family,
                    invitation_id: row.1.try_into().map_err(|_| Error::CorruptState)?,
                    genesis_bytes: row.2,
                    issue_bytes: row.3,
                    candidate_bytes: row.4,
                    secret_nonce: row.5.try_into().map_err(|_| Error::CorruptState)?,
                    secret_ciphertext: row.6,
                })
            })
            .transpose()
    }

    pub fn has_enrollment_attempt(&self, family_id: [u8; 16]) -> Result<bool, Error> {
        Ok(self
            .connection
            .query_row(
                "SELECT 1 FROM enrollment_attempts WHERE family_id = ?1",
                [family_id.as_slice()],
                |_| Ok(()),
            )
            .optional()?
            .is_some())
    }
}
