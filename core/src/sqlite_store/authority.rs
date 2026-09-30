//! Authority persistence. Each method retains its complete transaction.

use super::*;

impl SqliteStore {
    pub(crate) fn save_manager_creation(&mut self, row: &ManagerCreationRow) -> Result<(), Error> {
        let transaction = self.connection.transaction()?;
        let (last_index, _) = checked_family(&transaction, row.family)?;
        let watermark = i64::try_from(row.chunks.last().map_or(0, |chunk| chunk.last_local_index))
            .map_err(|_| Error::CorruptState)?;
        if last_index < watermark {
            return Err(Error::CorruptState);
        }
        transaction.execute(
            "INSERT INTO manager_creations
             (family_id,device_id,relay_public_key,promotion_id,transition_id,object_id,object_bytes,
              candidate_bytes,secret_nonce,secret_ciphertext)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
            params![
                &row.family.family_id[..],
                &row.family.device_id[..],
                &row.relay_public_key[..],
                &row.promotion_id[..],
                &row.transition_id[..],
                &row.object_id[..],
                &row.object_bytes,
                &row.candidate_bytes,
                &row.secret_nonce[..],
                &row.secret_ciphertext,
            ],
        )?;
        for chunk in &row.chunks {
            transaction.execute(
                "INSERT INTO manager_promotion_chunks
                 (family_id,chunk_index,object_id,first_local_index,last_local_index,object_bytes)
                 VALUES(?1,?2,?3,?4,?5,?6)",
                params![
                    row.family.family_id.as_slice(),
                    chunk.index,
                    chunk.object_id.as_slice(),
                    chunk.first_local_index,
                    chunk.last_local_index,
                    &chunk.object_bytes,
                ],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub(crate) fn manager_creation(
        &self,
        family: FamilyHandle,
    ) -> Result<Option<ManagerCreationRow>, Error> {
        let _ = checked_family(&self.connection, family)?;
        self.connection
            .query_row(
            "SELECT device_id,relay_public_key,promotion_id,transition_id,object_id,object_bytes,
                    candidate_bytes,secret_nonce,secret_ciphertext
             FROM manager_creations WHERE family_id=?1",
                [&family.family_id[..]],
                |r| {
                    Ok((
                        r.get::<_, Vec<u8>>(0)?,
                        r.get::<_, Vec<u8>>(1)?,
                r.get::<_, Vec<u8>>(2)?,
                r.get::<_, Vec<u8>>(3)?,
                r.get::<_, Vec<u8>>(4)?,
                r.get::<_, Vec<u8>>(5)?,
                r.get::<_, Vec<u8>>(6)?,
                r.get::<_, Vec<u8>>(7)?,
                r.get::<_, Vec<u8>>(8)?,
                    ))
                },
            )
            .optional()?
            .map(|row| {
                if row.0 != family.device_id {
                    return Err(Error::WrongDevice);
                }
                let mut statement = self.connection.prepare(
                    "SELECT chunk_index,object_id,first_local_index,last_local_index,object_bytes
                     FROM manager_promotion_chunks WHERE family_id=?1 ORDER BY chunk_index",
                )?;
                let chunks = statement
                    .query_map([family.family_id.as_slice()], |r| {
                        Ok((r.get::<_, u32>(0)?, r.get::<_, Vec<u8>>(1)?, r.get::<_, u64>(2)?,
                            r.get::<_, u64>(3)?, r.get::<_, Vec<u8>>(4)?))
                    })?
                    .map(|r| {
                        let (index, object_id, first_local_index, last_local_index, object_bytes) = r?;
                        Ok(PromotionChunkRow { index, object_id: object_id.try_into().map_err(|_| Error::CorruptState)?,
                            first_local_index, last_local_index, object_bytes })
                    })
                    .collect::<Result<Vec<_>, Error>>()?;
                Ok(ManagerCreationRow {
                    family,
                    relay_public_key: row.1.try_into().map_err(|_| Error::CorruptState)?,
                promotion_id: row.2.try_into().map_err(|_| Error::CorruptState)?,
                transition_id: row.3.try_into().map_err(|_| Error::CorruptState)?,
                object_id: row.4.try_into().map_err(|_| Error::CorruptState)?,
                object_bytes: row.5,
                candidate_bytes: row.6,
                secret_nonce: row.7.try_into().map_err(|_| Error::CorruptState)?,
                secret_ciphertext: row.8,
                chunks,
                })
            })
            .transpose()
    }

    pub(crate) fn save_first_invite_issue(&mut self, row: &InviteIssueRow) -> Result<(), Error> {
        self.save_invite_issue(row, true)
    }

    pub(crate) fn save_later_invite_issue(&mut self, row: &InviteIssueRow) -> Result<(), Error> {
        self.save_invite_issue(row, false)
    }

    fn save_invite_issue(&mut self, row: &InviteIssueRow, is_first: bool) -> Result<(), Error> {
        let transaction = self.connection.transaction()?;
        let _ = checked_family(&transaction, row.family)?;
        transaction.execute(
            "INSERT INTO invite_issues
             (family_id,device_id,invitation_id,transition_id,object_id,object_bytes,
              candidate_bytes,secret_nonce,secret_ciphertext,is_first)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
            params![
                &row.family.family_id[..],
                &row.family.device_id[..],
                &row.invitation_id[..],
                &row.transition_id[..],
                &row.object_id[..],
                &row.object_bytes,
                &row.candidate_bytes,
                &row.secret_nonce[..],
                &row.secret_ciphertext,
                i64::from(is_first),
            ],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub(crate) fn first_invite_issue(
        &self,
        family: FamilyHandle,
    ) -> Result<Option<InviteIssueRow>, Error> {
        let _ = checked_family(&self.connection, family)?;
        let invitation_id: Option<Vec<u8>> = self
            .connection
            .query_row(
                "SELECT invitation_id FROM invite_issues WHERE family_id=?1 AND is_first=1",
                [family.family_id.as_slice()],
                |row| row.get(0),
            )
            .optional()?;
        invitation_id
            .map(|id| self.invite_issue(family, id.try_into().map_err(|_| Error::CorruptState)?))
            .transpose()
            .map(|row| row.flatten())
    }

    pub(crate) fn invite_issue(
        &self,
        family: FamilyHandle,
        invitation_id: [u8; 16],
    ) -> Result<Option<InviteIssueRow>, Error> {
        let _ = checked_family(&self.connection, family)?;
        self.connection
            .query_row(
                "SELECT device_id,invitation_id,transition_id,object_id,object_bytes,
                    candidate_bytes,secret_nonce,secret_ciphertext
             FROM invite_issues WHERE family_id=?1 AND invitation_id=?2",
                params![family.family_id.as_slice(), invitation_id.as_slice()],
                |r| {
                    Ok((
                        r.get::<_, Vec<u8>>(0)?,
                        r.get::<_, Vec<u8>>(1)?,
                        r.get::<_, Vec<u8>>(2)?,
                        r.get::<_, Vec<u8>>(3)?,
                        r.get::<_, Vec<u8>>(4)?,
                        r.get::<_, Vec<u8>>(5)?,
                        r.get::<_, Vec<u8>>(6)?,
                        r.get::<_, Vec<u8>>(7)?,
                    ))
                },
            )
            .optional()?
            .map(|row| {
                if row.0 != family.device_id {
                    return Err(Error::WrongDevice);
                }
                Ok(InviteIssueRow {
                    family,
                    invitation_id: row.1.try_into().map_err(|_| Error::CorruptState)?,
                    transition_id: row.2.try_into().map_err(|_| Error::CorruptState)?,
                    object_id: row.3.try_into().map_err(|_| Error::CorruptState)?,
                    object_bytes: row.4,
                    candidate_bytes: row.5,
                    secret_nonce: row.6.try_into().map_err(|_| Error::CorruptState)?,
                    secret_ciphertext: row.7,
                })
            })
            .transpose()
    }

    pub(crate) fn save_prepared_control(&mut self, row: &PreparedControlRow) -> Result<(), Error> {
        let tx = self.connection.transaction()?;
        let _ = checked_family(&tx, row.family)?;
        tx.execute(
            "INSERT INTO prepared_controls
             (family_id,kind,device_id,transition_id,candidate_bytes,objects_bytes,
              secret_nonce,secret_ciphertext)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
            params![
                &row.family.family_id[..],
                i64::from(row.kind),
                &row.family.device_id[..],
                &row.transition_id[..],
                &row.candidate_bytes,
                &row.objects_bytes,
                &row.secret_nonce[..],
                &row.secret_ciphertext,
            ],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub(crate) fn prepared_control(
        &self,
        family: FamilyHandle,
        kind: u8,
    ) -> Result<Option<PreparedControlRow>, Error> {
        let _ = checked_family(&self.connection, family)?;
        self.connection
            .query_row(
                "SELECT device_id,transition_id,candidate_bytes,objects_bytes,
                    secret_nonce,secret_ciphertext
             FROM prepared_controls WHERE family_id=?1 AND kind=?2",
                params![&family.family_id[..], i64::from(kind)],
                |r| {
                    Ok((
                        r.get::<_, Vec<u8>>(0)?,
                        r.get::<_, Vec<u8>>(1)?,
                        r.get::<_, Vec<u8>>(2)?,
                        r.get::<_, Vec<u8>>(3)?,
                        r.get::<_, Vec<u8>>(4)?,
                        r.get::<_, Vec<u8>>(5)?,
                    ))
                },
            )
            .optional()?
            .map(|row| {
                if row.0 != family.device_id {
                    return Err(Error::WrongDevice);
                }
                Ok(PreparedControlRow {
                    family,
                    kind,
                    transition_id: row.1.try_into().map_err(|_| Error::CorruptState)?,
                    candidate_bytes: row.2,
                    objects_bytes: row.3,
                    secret_nonce: row.4.try_into().map_err(|_| Error::CorruptState)?,
                    secret_ciphertext: row.5,
                })
            })
            .transpose()
    }

    pub(crate) fn delete_prepared_control(
        &mut self,
        family: FamilyHandle,
        kind: u8,
        transition_id: [u8; 16],
    ) -> Result<(), Error> {
        let tx = self.connection.transaction()?;
        let _ = checked_family(&tx, family)?;
        tx.execute(
            "DELETE FROM prepared_controls
             WHERE family_id=?1 AND kind=?2 AND device_id=?3 AND transition_id=?4",
            params![
                family.family_id.as_slice(),
                i64::from(kind),
                family.device_id.as_slice(),
                transition_id.as_slice(),
            ],
        )?;
        tx.commit()?;
        Ok(())
    }
}
