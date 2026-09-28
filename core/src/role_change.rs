//! Durable manager role change for an admitted device. The same signed
//! candidate is replayed after an uncertain relay result.

use crate::{
    cbor::{self, Value},
    control_build,
    creation::ManagerCreation,
    crypto,
    enrollment::EnrollmentAttempt,
    invite_cancel::{self, Error},
    shared_history::PublicHistorySession,
    shared_ready::ReadyFamilySession,
    sqlite_store::{FamilyHandle, PreparedControlRow, SqliteStore},
};

pub struct RoleChange {
    family: FamilyHandle,
    target_id: [u8; 16],
    new_role: u8,
    transition_id: [u8; 16],
    candidate_bytes: Vec<u8>,
}

impl RoleChange {
    pub fn prepare_for_initial_manager(
        store: &mut SqliteStore,
        manager: &ManagerCreation,
        target_id: [u8; 16],
        new_role: u8,
    ) -> Result<Self, Error> {
        let ready = manager
            .ready_session(store)
            .map_err(|_| Error::Invalid("manager not ready"))?;
        Self::prepare(store, &ready, manager.signing_seed(), target_id, new_role)
    }

    pub fn prepare_for_admitted_manager(
        store: &mut SqliteStore,
        holder: &EnrollmentAttempt,
        target_id: [u8; 16],
        new_role: u8,
    ) -> Result<Self, Error> {
        let ready = ReadyFamilySession::from_enrollment(store, holder)?;
        Self::prepare(store, &ready, holder.signing_seed(), target_id, new_role)
    }

    fn prepare(
        store: &mut SqliteStore,
        ready: &ReadyFamilySession,
        signing_seed: [u8; 32],
        target_id: [u8; 16],
        new_role: u8,
    ) -> Result<Self, Error> {
        if !matches!(new_role, 1 | 2) {
            return Err(Error::Invalid("role must be member or manager"));
        }
        let family = ready.family();
        if store.prepared_control(family, 7)?.is_some() {
            let saved = Self::resume(store, family)?;
            if invite_cancel::committed_matches(store, family, &saved.candidate_bytes)? {
                store.delete_prepared_control(family, 7, saved.transition_id)?;
            } else {
                if saved.target_id != target_id || saved.new_role != new_role {
                    return Err(Error::Invalid("another role change is pending"));
                }
                return Ok(saved);
            }
        }
        let public = PublicHistorySession::resume(store, family)?;
        if ready.observed_cursor() != public.cursor() || ready.observed_head() != public.head_hash()
        {
            return Err(Error::Invalid("manager view behind public authority"));
        }
        let chain = public.chain();
        if chain.active_signing_public(family.device_id)?
            != crypto::signing_public_key(&signing_seed)
            || !chain
                .active_devices()?
                .iter()
                .any(|device| device.device_id == family.device_id && device.role == 2)
        {
            return Err(Error::Invalid("role signer is not a manager"));
        }
        let old_role = chain
            .active_devices()?
            .iter()
            .find(|device| device.device_id == target_id)
            .ok_or(Error::Invalid("role target not active"))?
            .role;
        if old_role == new_role {
            return Err(Error::Invalid("device already has requested role"));
        }
        let old_state = cbor::decode(&chain.state_bytes()?)?;
        let Value::Map(mut next_state) = old_state.clone() else {
            return Err(Error::Invalid("authority state not a map"));
        };
        let Value::Array(active) = &mut next_state[4].1 else {
            return Err(Error::Invalid("active devices not an array"));
        };
        for row in active {
            let Value::Array(fields) = row else {
                return Err(Error::Invalid("active device row not an array"));
            };
            if fields.len() != 5 {
                return Err(Error::Invalid("active device row width"));
            }
            if fields[0] == Value::Bytes(target_id.to_vec()) {
                fields[4] = Value::Integer(new_role.into());
            }
        }
        if old_role == 2 {
            let Value::Array(invitations) = &mut next_state[6].1 else {
                return Err(Error::Invalid("invitations not an array"));
            };
            for row in invitations {
                let Value::Array(fields) = row else {
                    return Err(Error::Invalid("invitation row not an array"));
                };
                if fields.len() != 6 {
                    return Err(Error::Invalid("invitation row width"));
                }
                if fields[1] == Value::Bytes(target_id.to_vec()) && fields[5] == Value::Integer(1) {
                    fields[5] = Value::Integer(3);
                }
            }
        }
        let next_state = Value::Map(next_state);
        let transition_id = invite_cancel::random_v4()?;
        let candidate_bytes = control_build::candidate(
            family,
            chain.relay_id(),
            chain.head_hash(),
            transition_id,
            7,
            Value::Map(vec![
                (1, Value::Bytes(target_id.to_vec())),
                (2, Value::Integer(old_role.into())),
                (3, Value::Integer(new_role.into())),
            ]),
            next_state.clone(),
            chain.epoch()?,
            &[],
            &signing_seed,
        )?;
        let prepared = babytrack_wire::authority::prepare_role_change(
            &candidate_bytes,
            &old_state,
            chain.head_hash(),
        )?;
        if prepared.next_state != next_state {
            return Err(Error::Invalid("role state differs from authority reducer"));
        }
        store.save_prepared_control(&PreparedControlRow {
            family,
            kind: 7,
            transition_id,
            candidate_bytes,
            objects_bytes: cbor::encode(&Value::Array(vec![]))?,
            secret_nonce: [0; 24],
            secret_ciphertext: Vec::new(),
        })?;
        Self::resume(store, family)
    }

    pub fn resume(store: &SqliteStore, family: FamilyHandle) -> Result<Self, Error> {
        let row = store
            .prepared_control(family, 7)?
            .ok_or(Error::Invalid("no prepared role change"))?;
        if row.secret_nonce != [0; 24]
            || !row.secret_ciphertext.is_empty()
            || row.objects_bytes != cbor::encode(&Value::Array(vec![]))?
        {
            return Err(Error::Invalid(
                "role change has unexpected objects or secret",
            ));
        }
        let Value::Map(candidate) = cbor::decode(&row.candidate_bytes)? else {
            return Err(Error::Invalid("role candidate not a map"));
        };
        if candidate.len() != 2 {
            return Err(Error::Invalid("role candidate width"));
        }
        let Value::Map(unsigned) = &candidate[0].1 else {
            return Err(Error::Invalid("role unsigned not a map"));
        };
        if unsigned.len() != 11
            || unsigned[5].1 != Value::Integer(7)
            || unsigned[4].1 != Value::Bytes(row.transition_id.to_vec())
            || unsigned[1].1 != Value::Bytes(family.family_id.to_vec())
        {
            return Err(Error::Invalid("saved role identity mismatch"));
        }
        let Value::Map(delta) = &unsigned[6].1 else {
            return Err(Error::Invalid("role delta not a map"));
        };
        if delta.len() != 3 {
            return Err(Error::Invalid("role delta width"));
        }
        let Value::Bytes(target) = &delta[0].1 else {
            return Err(Error::Invalid("role target not bytes"));
        };
        let target_id = target
            .as_slice()
            .try_into()
            .map_err(|_| Error::Invalid("role target length"))?;
        let Value::Integer(new_role) = delta[2].1 else {
            return Err(Error::Invalid("role number missing"));
        };
        let new_role = new_role
            .try_into()
            .map_err(|_| Error::Invalid("role number outside u8"))?;
        Ok(Self {
            family,
            target_id,
            new_role,
            transition_id: row.transition_id,
            candidate_bytes: row.candidate_bytes,
        })
    }

    pub fn candidate_bytes(&self) -> &[u8] {
        &self.candidate_bytes
    }
    pub fn target_id(&self) -> [u8; 16] {
        self.target_id
    }
    pub fn new_role(&self) -> u8 {
        self.new_role
    }
    pub fn confirm(&self, store: &mut SqliteStore, committed: &[u8]) -> Result<(), Error> {
        let Value::Map(root) = cbor::decode(committed)? else {
            return Err(Error::Invalid("committed role change not a map"));
        };
        if root.len() != 4
            || cbor::encode(&Value::Map(vec![
                (1, root[0].1.clone()),
                (2, root[1].1.clone()),
            ]))? != self.candidate_bytes
        {
            return Err(Error::Invalid("committed role candidate mismatch"));
        }
        if !invite_cancel::committed_matches(store, self.family, &self.candidate_bytes)? {
            PublicHistorySession::resume(store, self.family)?.accept_control(store, committed)?;
        }
        store.delete_prepared_control(self.family, 7, self.transition_id)?;
        Ok(())
    }
}
