//! Durable manager removal of a keyless pending device. No epoch rotation is
//! needed because this device has never received an epoch key.

use crate::{
    cbor::{self, Value},
    control_build, control_chain,
    creation::ManagerCreation,
    enrollment::EnrollmentAttempt,
    shared_history::{self, PublicHistorySession},
    shared_ready::{self, ReadyFamilySession},
    sqlite_store::{self, FamilyHandle, PreparedControlRow, SqliteStore},
};

#[derive(Debug)]
pub enum Error {
    Cbor(cbor::Error),
    Crypto(crate::crypto::Error),
    Chain(control_chain::Error),
    History(shared_history::Error),
    Ready(shared_ready::Error),
    Store(sqlite_store::Error),
    Random(getrandom::Error),
    Authority(babytrack_wire::authority::Error),
    Invalid(&'static str),
}

impl From<cbor::Error> for Error {
    fn from(value: cbor::Error) -> Self {
        Self::Cbor(value)
    }
}
impl From<control_build::Error> for Error {
    fn from(value: control_build::Error) -> Self {
        match value {
            control_build::Error::Cbor(error) => Self::Cbor(error),
            control_build::Error::Crypto(error) => Self::Crypto(error),
            control_build::Error::Invalid(reason) => Self::Invalid(reason),
        }
    }
}
impl From<control_chain::Error> for Error {
    fn from(value: control_chain::Error) -> Self {
        Self::Chain(value)
    }
}
impl From<shared_history::Error> for Error {
    fn from(value: shared_history::Error) -> Self {
        Self::History(value)
    }
}
impl From<shared_ready::Error> for Error {
    fn from(value: shared_ready::Error) -> Self {
        Self::Ready(value)
    }
}
impl From<sqlite_store::Error> for Error {
    fn from(value: sqlite_store::Error) -> Self {
        Self::Store(value)
    }
}
impl From<getrandom::Error> for Error {
    fn from(value: getrandom::Error) -> Self {
        Self::Random(value)
    }
}
impl From<babytrack_wire::authority::Error> for Error {
    fn from(value: babytrack_wire::authority::Error) -> Self {
        Self::Authority(value)
    }
}

pub struct PendingRemoval {
    family: FamilyHandle,
    invitation_id: [u8; 16],
    device_id: [u8; 16],
    transition_id: [u8; 16],
    candidate_bytes: Vec<u8>,
}

impl PendingRemoval {
    pub fn prepare_for_initial_manager(
        store: &mut SqliteStore,
        manager: &ManagerCreation,
        invitation_id: [u8; 16],
        device_id: [u8; 16],
    ) -> Result<Self, Error> {
        let ready = manager
            .ready_session(store)
            .map_err(|_| Error::Invalid("manager not ready"))?;
        Self::prepare(
            store,
            &ready,
            manager.signing_seed(),
            invitation_id,
            device_id,
        )
    }

    pub fn prepare_for_admitted_manager(
        store: &mut SqliteStore,
        holder: &EnrollmentAttempt,
        invitation_id: [u8; 16],
        device_id: [u8; 16],
    ) -> Result<Self, Error> {
        let ready = ReadyFamilySession::from_enrollment(store, holder)?;
        Self::prepare(
            store,
            &ready,
            holder.signing_seed(),
            invitation_id,
            device_id,
        )
    }

    fn prepare(
        store: &mut SqliteStore,
        ready: &ReadyFamilySession,
        signing_seed: [u8; 32],
        invitation_id: [u8; 16],
        device_id: [u8; 16],
    ) -> Result<Self, Error> {
        let family = ready.family();
        if store.prepared_control(family, 9)?.is_some() {
            let saved = Self::resume(store, family)?;
            if committed_matches(store, family, &saved.candidate_bytes)? {
                if saved.invitation_id == invitation_id && saved.device_id == device_id {
                    return Ok(saved);
                }
                store.delete_prepared_control(family, 9, saved.transition_id)?;
            } else {
                if saved.invitation_id != invitation_id || saved.device_id != device_id {
                    return Err(Error::Invalid("another pending removal is prepared"));
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
        let old_state = cbor::decode(&chain.state_bytes()?)?;
        let Value::Map(mut next) = old_state.clone() else {
            return Err(Error::Invalid("authority state not map"));
        };
        let Value::Array(pending) = &mut next[5].1 else {
            return Err(Error::Invalid("pending state not array"));
        };
        let mut matched = false;
        pending.retain(|row| {
            if let Value::Array(fields) = row
                && fields.len() == 9
                && fields[0] == Value::Bytes(invitation_id.to_vec())
                && fields[1] == Value::Bytes(device_id.to_vec())
            {
                matched = true;
                return false;
            }
            true
        });
        if !matched {
            return Err(Error::Invalid("pending device absent"));
        }
        let next_state = Value::Map(next);
        let mut transition_id = [0; 16];
        getrandom::fill(&mut transition_id)?;
        transition_id[6] = (transition_id[6] & 0x0f) | 0x40;
        transition_id[8] = (transition_id[8] & 0x3f) | 0x80;
        let candidate_bytes = control_build::candidate(
            family,
            chain.relay_id(),
            chain.head_hash(),
            transition_id,
            9,
            Value::Map(vec![
                (1, Value::Bytes(invitation_id.to_vec())),
                (2, Value::Bytes(device_id.to_vec())),
            ]),
            next_state.clone(),
            chain.epoch()?,
            &[],
            &signing_seed,
        )?;
        let prepared = babytrack_wire::authority::prepare_pending_removal(
            &candidate_bytes,
            &old_state,
            chain.head_hash(),
        )?;
        if prepared.next_state != next_state {
            return Err(Error::Invalid(
                "pending removal differs from authority reducer",
            ));
        }
        store.save_prepared_control(&PreparedControlRow {
            family,
            kind: 9,
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
            .prepared_control(family, 9)?
            .ok_or(Error::Invalid("no prepared pending removal"))?;
        if row.secret_nonce != [0; 24]
            || !row.secret_ciphertext.is_empty()
            || row.objects_bytes != cbor::encode(&Value::Array(vec![]))?
        {
            return Err(Error::Invalid(
                "pending removal has unexpected objects or secret",
            ));
        }
        let Value::Map(root) = cbor::decode(&row.candidate_bytes)? else {
            return Err(Error::Invalid("pending removal candidate not map"));
        };
        if root.len() != 2 {
            return Err(Error::Invalid("pending removal candidate width"));
        }
        let Value::Map(unsigned) = &root[0].1 else {
            return Err(Error::Invalid("pending removal unsigned not map"));
        };
        if unsigned.len() != 11
            || unsigned[5].1 != Value::Integer(9)
            || unsigned[4].1 != Value::Bytes(row.transition_id.to_vec())
            || unsigned[1].1 != Value::Bytes(family.family_id.to_vec())
        {
            return Err(Error::Invalid("saved pending removal identity mismatch"));
        }
        let Value::Map(delta) = &unsigned[6].1 else {
            return Err(Error::Invalid("pending removal delta not map"));
        };
        if delta.len() != 2 {
            return Err(Error::Invalid("pending removal delta width"));
        }
        let Value::Bytes(invitation) = &delta[0].1 else {
            return Err(Error::Invalid("pending removal invitation not bytes"));
        };
        let Value::Bytes(device) = &delta[1].1 else {
            return Err(Error::Invalid("pending removal device not bytes"));
        };
        let invitation_id = invitation
            .as_slice()
            .try_into()
            .map_err(|_| Error::Invalid("pending invitation length"))?;
        let device_id = device
            .as_slice()
            .try_into()
            .map_err(|_| Error::Invalid("pending device length"))?;
        Ok(Self {
            family,
            invitation_id,
            device_id,
            transition_id: row.transition_id,
            candidate_bytes: row.candidate_bytes,
        })
    }

    pub fn invitation_id(&self) -> [u8; 16] {
        self.invitation_id
    }
    pub fn device_id(&self) -> [u8; 16] {
        self.device_id
    }
    pub fn candidate_bytes(&self) -> &[u8] {
        &self.candidate_bytes
    }

    pub fn confirm(&self, store: &mut SqliteStore, committed: &[u8]) -> Result<(), Error> {
        let Value::Map(root) = cbor::decode(committed)? else {
            return Err(Error::Invalid("committed pending removal not map"));
        };
        if root.len() != 4
            || cbor::encode(&Value::Map(vec![
                (1, root[0].1.clone()),
                (2, root[1].1.clone()),
            ]))? != self.candidate_bytes
        {
            return Err(Error::Invalid(
                "committed pending removal candidate mismatch",
            ));
        }
        if !committed_matches(store, self.family, &self.candidate_bytes)? {
            PublicHistorySession::resume(store, self.family)?.accept_control(store, committed)?;
        }
        store.delete_prepared_control(self.family, 9, self.transition_id)?;
        Ok(())
    }
}

fn committed_matches(
    store: &SqliteStore,
    family: FamilyHandle,
    candidate: &[u8],
) -> Result<bool, Error> {
    let history = store
        .shared_history(family)?
        .ok_or(Error::Invalid("shared history absent"))?;
    for entry in history.entries {
        if entry.kind != 1 {
            continue;
        }
        let Value::Map(root) = cbor::decode(&entry.committed_bytes)? else {
            continue;
        };
        if root.len() != 4 {
            continue;
        }
        if cbor::encode(&Value::Map(vec![
            (1, root[0].1.clone()),
            (2, root[1].1.clone()),
        ]))? == candidate
        {
            return Ok(true);
        }
    }
    Ok(false)
}
