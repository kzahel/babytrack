//! Copies persistence. Each method retains its complete transaction.

use super::*;

impl SqliteStore {
    /// Publish a restored local Family and its current-state operations in
    /// one transaction. A validation error cannot expose a partial Family.
    pub(crate) fn restore_family(
        &mut self,
        family_id: [u8; 16],
        device_id: [u8; 16],
        operations: Vec<NewOperation>,
        now_ms: i64,
        origin: RestoredOrigin,
    ) -> Result<FamilyHandle, Error> {
        self.restore_family_with_copy_source(family_id, device_id, operations, now_ms, origin, None)
    }

    pub(crate) fn restore_family_with_copy_source(
        &mut self,
        family_id: [u8; 16],
        device_id: [u8; 16],
        operations: Vec<NewOperation>,
        now_ms: i64,
        origin: RestoredOrigin,
        copy_source: Option<CopySource>,
    ) -> Result<FamilyHandle, Error> {
        if !ids::is_v4(&family_id) || !ids::is_v4(&device_id) {
            return Err(Error::InvalidId);
        }
        let family = FamilyHandle {
            family_id,
            device_id,
        };
        let transaction = self.connection.transaction()?;
        transaction.execute(
            "INSERT INTO families(family_id,device_id) VALUES(?1,?2)",
            params![family_id.as_slice(), device_id.as_slice()],
        )?;
        transaction.execute(
            "INSERT INTO local_sync_state(family_id) VALUES(?1)",
            [family_id.as_slice()],
        )?;
        transaction.execute(
            "INSERT INTO restored_origins(family_id,source_family_id,snapshot_utc_ms,source_cursor,known_gap)
             VALUES(?1,?2,?3,?4,?5)",
            params![family_id.as_slice(),origin.source_family_id.as_slice(),origin.snapshot_utc_ms,
                origin.source_cursor.map(|value| value.to_be_bytes().to_vec()),
                i64::from(origin.known_gap)],
        )?;
        if let Some(source) = copy_source {
            let source_family = match source {
                CopySource::Manual(value) | CopySource::Removal(value, _) => value,
            };
            if source_family.family_id != origin.source_family_id {
                return Err(Error::WrongFamily);
            }
            checked_family(&transaction, source_family)?;
            match source {
                CopySource::Manual(source) => {
                    transaction.execute(
                        "INSERT INTO private_copies(source_family_id,source_device_id,copy_family_id) VALUES(?1,?2,?3)",
                        params![source.family_id.as_slice(),source.device_id.as_slice(),family_id.as_slice()],
                    )?;
                }
                CopySource::Removal(source, transition_id) => {
                    transaction.execute(
                        "INSERT INTO removal_copies(source_family_id,source_device_id,transition_id,copy_family_id) VALUES(?1,?2,?3,?4)",
                        params![source.family_id.as_slice(),source.device_id.as_slice(),transition_id.as_slice(),family_id.as_slice()],
                    )?;
                }
            }
        }
        let mut projection = LocalProjection::new(family_id);
        let mut clock = Clock::restore(family_id, device_id, None, None)?;
        let mut last_stamp = None;
        for (position, mut new) in operations.into_iter().enumerate() {
            if new.family_id != family_id || new.author_device_id != device_id {
                return Err(Error::WrongFamily);
            }
            let index = i64::try_from(position + 1).map_err(|_| Error::AppendIndexOverflow)?;
            new.hlc = clock.next(now_ms).stamp;
            let bytes = Operation::encode_new(&new)?;
            let decoded = Operation::decode_bound(&bytes, &family_id, &device_id)?;
            projection.append(&decoded, index as u64)?;
            transaction.execute(
                "INSERT INTO local_operations(family_id,append_index,operation_id,operation_bytes)
                 VALUES(?1,?2,?3,?4)",
                params![
                    family_id.as_slice(),
                    index,
                    new.operation_id.as_slice(),
                    &bytes
                ],
            )?;
            last_stamp = Some(new.hlc);
        }
        if let Some(stamp) = last_stamp {
            transaction.execute(
                "UPDATE families SET last_index=?2,last_hlc_wall=?3,last_hlc_counter=?4 WHERE family_id=?1",
                params![family_id.as_slice(),projection.last_append_index(),stamp.wall_ms,stamp.counter],
            )?;
        }
        transaction.commit()?;
        Ok(family)
    }

    pub fn private_copy_of(&self, source: FamilyHandle) -> Result<Option<FamilyHandle>, Error> {
        checked_family(&self.connection, source)?;
        let saved: Option<(Vec<u8>, Vec<u8>)> = self
            .connection
            .query_row(
                "SELECT f.family_id,f.device_id FROM private_copies p
             JOIN families f ON f.family_id=p.copy_family_id
             WHERE p.source_family_id=?1 AND p.source_device_id=?2",
                params![source.family_id.as_slice(), source.device_id.as_slice()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        saved
            .map(|(family_id, device_id)| {
                Ok(FamilyHandle {
                    family_id: family_id.try_into().map_err(|_| Error::CorruptState)?,
                    device_id: device_id.try_into().map_err(|_| Error::CorruptState)?,
                })
            })
            .transpose()
    }

    pub fn saved_removal(&self, source: FamilyHandle) -> Result<Option<SavedRemoval>, Error> {
        checked_family(&self.connection, source)?;
        type SavedRemovalRow = (Vec<u8>, i64, i64, i64, Vec<u8>);
        let saved: Option<SavedRemovalRow> = self
            .connection
            .query_row(
                "SELECT transition_id,cursor,source_cursor,known_gap,committed_bytes
             FROM verified_removals WHERE source_family_id=?1 AND source_device_id=?2",
                params![source.family_id.as_slice(), source.device_id.as_slice()],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                },
            )
            .optional()?;
        saved
            .map(
                |(transition_id, cursor, source_cursor, known_gap, committed_bytes)| {
                    Ok(SavedRemoval {
                        transition_id: transition_id.try_into().map_err(|_| Error::CorruptState)?,
                        cursor: cursor.try_into().map_err(|_| Error::CorruptState)?,
                        source_cursor: source_cursor.try_into().map_err(|_| Error::CorruptState)?,
                        known_gap: match known_gap {
                            0 => false,
                            1 => true,
                            _ => return Err(Error::CorruptState),
                        },
                        committed_bytes,
                    })
                },
            )
            .transpose()
    }

    pub(crate) fn save_verified_removal(
        &mut self,
        source: FamilyHandle,
        removal: &SavedRemoval,
    ) -> Result<(), Error> {
        if let Some(existing) = self.saved_removal(source)? {
            return if existing == *removal {
                Ok(())
            } else {
                Err(Error::CorruptState)
            };
        }
        let history = self.shared_history(source)?.ok_or(Error::CorruptState)?;
        if history.pinned_cursor != removal.source_cursor || removal.cursor <= removal.source_cursor
        {
            return Err(Error::CorruptState);
        }
        self.connection.execute(
            "INSERT INTO verified_removals(source_family_id,source_device_id,transition_id,cursor,source_cursor,known_gap,committed_bytes)
             VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![source.family_id.as_slice(),source.device_id.as_slice(),removal.transition_id.as_slice(),
                i64::try_from(removal.cursor).map_err(|_| Error::CorruptState)?,
                i64::try_from(removal.source_cursor).map_err(|_| Error::CorruptState)?,
                i64::from(removal.known_gap),&removal.committed_bytes],
        )?;
        Ok(())
    }

    pub fn removal_copy_of(
        &self,
        source: FamilyHandle,
        transition_id: [u8; 16],
    ) -> Result<Option<FamilyHandle>, Error> {
        checked_family(&self.connection, source)?;
        let saved: Option<(Vec<u8>,Vec<u8>)> = self.connection.query_row(
            "SELECT f.family_id,f.device_id FROM removal_copies r JOIN families f ON f.family_id=r.copy_family_id
             WHERE r.source_family_id=?1 AND r.source_device_id=?2 AND r.transition_id=?3",
            params![source.family_id.as_slice(),source.device_id.as_slice(),transition_id.as_slice()],
            |row| Ok((row.get(0)?,row.get(1)?)),
        ).optional()?;
        saved
            .map(|(family_id, device_id)| {
                Ok(FamilyHandle {
                    family_id: family_id.try_into().map_err(|_| Error::CorruptState)?,
                    device_id: device_id.try_into().map_err(|_| Error::CorruptState)?,
                })
            })
            .transpose()
    }

    pub fn restored_origin(&self, family: FamilyHandle) -> Result<Option<RestoredOrigin>, Error> {
        let _ = checked_family(&self.connection, family)?;
        self.connection
            .query_row(
                "SELECT source_family_id,snapshot_utc_ms,source_cursor,known_gap
             FROM restored_origins WHERE family_id=?1",
                [family.family_id.as_slice()],
                |row| {
                    Ok((
                        row.get::<_, Vec<u8>>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, Option<Vec<u8>>>(2)?,
                        row.get::<_, i64>(3)?,
                    ))
                },
            )
            .optional()?
            .map(|(source, at, cursor, gap)| {
                Ok(RestoredOrigin {
                    source_family_id: source.try_into().map_err(|_| Error::CorruptState)?,
                    snapshot_utc_ms: at,
                    source_cursor: cursor
                        .map(|bytes| {
                            bytes
                                .try_into()
                                .map(u64::from_be_bytes)
                                .map_err(|_| Error::CorruptState)
                        })
                        .transpose()?,
                    known_gap: gap == 1,
                })
            })
            .transpose()
    }
}
