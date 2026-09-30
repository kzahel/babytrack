//! Families persistence. Each method retains its complete transaction.

use super::*;

impl SqliteStore {
    pub fn create_family(
        &mut self,
        family_id: [u8; 16],
        device_id: [u8; 16],
    ) -> Result<FamilyHandle, Error> {
        if !ids::is_v4(&family_id) || !ids::is_v4(&device_id) {
            return Err(Error::InvalidId);
        }
        let transaction = self.connection.transaction()?;
        transaction.execute(
            "INSERT INTO families(family_id, device_id) VALUES (?1, ?2)",
            params![family_id.as_slice(), device_id.as_slice()],
        )?;
        transaction.execute(
            "INSERT INTO local_sync_state(family_id) VALUES (?1)",
            [family_id.as_slice()],
        )?;
        transaction.commit()?;
        Ok(FamilyHandle {
            family_id,
            device_id,
        })
    }

    pub fn create_local_family_with_metadata(
        &mut self,
        family_id: [u8; 16],
        device_id: [u8; 16],
        operation_id: [u8; 16],
        now_ms: i64,
    ) -> Result<FamilyHandle, Error> {
        if !ids::is_v4(&family_id) || !ids::is_v4(&device_id) || !ids::is_v7(&operation_id) {
            return Err(Error::InvalidId);
        }
        let mut clock = Clock::restore(family_id, device_id, None, None)?;
        let stamp = clock.next(now_ms).stamp;
        let new = NewOperation {
            family_id,
            operation_id,
            record_id: family_id,
            scope: operation::Scope::Family,
            kind: operation::Kind::Create,
            author_device_id: device_id,
            hlc: stamp.clone(),
            record_type: Some("family".to_owned()),
            child_id: None,
            fields: Some(vec![]),
        };
        let bytes = Operation::encode_new(&new)?;
        let decoded = Operation::decode_bound(&bytes, &family_id, &device_id)?;
        LocalProjection::new(family_id).append(&decoded, 1)?;
        let transaction = self.connection.transaction()?;
        transaction.execute(
            "INSERT INTO families(family_id,device_id,last_index,last_hlc_wall,last_hlc_counter)
             VALUES(?1,?2,1,?3,?4)",
            params![
                family_id.as_slice(),
                device_id.as_slice(),
                stamp.wall_ms,
                stamp.counter
            ],
        )?;
        transaction.execute(
            "INSERT INTO local_sync_state(family_id) VALUES(?1)",
            [family_id.as_slice()],
        )?;
        transaction.execute(
            "INSERT INTO local_operations(family_id,append_index,operation_id,operation_bytes)
             VALUES(?1,1,?2,?3)",
            params![family_id.as_slice(), operation_id.as_slice(), bytes],
        )?;
        transaction.commit()?;
        Ok(FamilyHandle {
            family_id,
            device_id,
        })
    }

    pub fn families(&self) -> Result<Vec<FamilyHandle>, Error> {
        let mut statement = self
            .connection
            .prepare("SELECT family_id,device_id FROM families ORDER BY family_id")?;
        let rows = statement.query_map([], |row| {
            Ok((row.get::<_, Vec<u8>>(0)?, row.get::<_, Vec<u8>>(1)?))
        })?;
        let mut families = Vec::new();
        for row in rows {
            let (family_id, device_id) = row?;
            let family_id: [u8; 16] = family_id.try_into().map_err(|_| Error::CorruptState)?;
            let device_id: [u8; 16] = device_id.try_into().map_err(|_| Error::CorruptState)?;
            if !ids::is_v4(&family_id) || !ids::is_v4(&device_id) {
                return Err(Error::CorruptState);
            }
            families.push(FamilyHandle {
                family_id,
                device_id,
            });
        }
        Ok(families)
    }
}
