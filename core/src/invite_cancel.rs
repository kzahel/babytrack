//! Durable manager cancellation of one unused invitation. The signed exact
//! candidate is saved before POST and replayed after an uncertain response.

use crate::{
    cbor::{self, Value},
    control_build,
    creation::ManagerCreation,
    crypto,
    enrollment::EnrollmentAttempt,
    shared_history::{self, PublicHistorySession},
    shared_ready::{self, ReadyFamilySession},
    sqlite_store::{self, FamilyHandle, PreparedControlRow, SqliteStore},
};

#[derive(Debug)]
pub enum Error {
    Cbor(cbor::Error),
    Crypto(crypto::Error),
    Chain(crate::control_chain::Error),
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
impl From<crate::control_chain::Error> for Error {
    fn from(value: crate::control_chain::Error) -> Self {
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

pub struct InviteCancellation {
    family: FamilyHandle,
    invitation_id: [u8; 16],
    transition_id: [u8; 16],
    candidate_bytes: Vec<u8>,
}

impl InviteCancellation {
    pub fn prepare_for_initial_manager(
        store: &mut SqliteStore,
        manager: &ManagerCreation,
        invitation_id: [u8; 16],
    ) -> Result<Self, Error> {
        let ready = manager
            .ready_session(store)
            .map_err(|_| Error::Invalid("manager not ready"))?;
        Self::prepare(store, &ready, manager.signing_seed(), invitation_id)
    }

    pub fn prepare_for_admitted_manager(
        store: &mut SqliteStore,
        holder: &EnrollmentAttempt,
        invitation_id: [u8; 16],
    ) -> Result<Self, Error> {
        let ready = ReadyFamilySession::from_enrollment(store, holder)?;
        Self::prepare(store, &ready, holder.signing_seed(), invitation_id)
    }

    fn prepare(
        store: &mut SqliteStore,
        ready: &ReadyFamilySession,
        signing_seed: [u8; 32],
        invitation_id: [u8; 16],
    ) -> Result<Self, Error> {
        let family = ready.family();
        if store.prepared_control(family, 3)?.is_some() {
            let saved = Self::resume(store, family)?;
            if committed_matches(store, family, &saved.candidate_bytes)? {
                store.delete_prepared_control(family, 3, saved.transition_id)?;
            } else {
                if saved.invitation_id != invitation_id {
                    return Err(Error::Invalid("another invitation cancellation is pending"));
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
            return Err(Error::Invalid("cancellation signer is not a manager"));
        }
        let old_state = cbor::decode(&chain.state_bytes()?)?;
        let Value::Map(mut next_state) = old_state.clone() else {
            return Err(Error::Invalid("authority state not a map"));
        };
        let Value::Array(invitations) = &mut next_state[6].1 else {
            return Err(Error::Invalid("invitations not an array"));
        };
        let mut found = false;
        for row in invitations {
            let Value::Array(fields) = row else {
                return Err(Error::Invalid("invitation row not an array"));
            };
            if fields.len() != 6 {
                return Err(Error::Invalid("invitation row width"));
            }
            if fields[0] == Value::Bytes(invitation_id.to_vec()) {
                if fields[5] != Value::Integer(1) {
                    return Err(Error::Invalid("invitation is not unused"));
                }
                fields[5] = Value::Integer(3);
                found = true;
            }
        }
        if !found {
            return Err(Error::Invalid("invitation absent"));
        }
        let transition_id = random_v4()?;
        let next_state = Value::Map(next_state);
        let candidate_bytes = control_build::candidate(
            family,
            chain.relay_id(),
            chain.head_hash(),
            transition_id,
            3,
            Value::Map(vec![(1, Value::Bytes(invitation_id.to_vec()))]),
            next_state.clone(),
            chain.epoch()?,
            &[],
            &signing_seed,
        )?;
        let prepared = babytrack_wire::authority::prepare_cancel(
            &candidate_bytes,
            &old_state,
            chain.head_hash(),
        )?;
        if prepared.next_state != next_state {
            return Err(Error::Invalid(
                "cancel state differs from authority reducer",
            ));
        }
        store.save_prepared_control(&PreparedControlRow {
            family,
            kind: 3,
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
            .prepared_control(family, 3)?
            .ok_or(Error::Invalid("no prepared cancellation"))?;
        if row.secret_nonce != [0; 24]
            || !row.secret_ciphertext.is_empty()
            || row.objects_bytes != cbor::encode(&Value::Array(vec![]))?
        {
            return Err(Error::Invalid(
                "cancellation row has unexpected objects or secret",
            ));
        }
        let Value::Map(candidate) = cbor::decode(&row.candidate_bytes)? else {
            return Err(Error::Invalid("cancellation candidate not a map"));
        };
        if candidate.len() != 2 {
            return Err(Error::Invalid("cancellation candidate width"));
        }
        let Value::Map(unsigned) = &candidate[0].1 else {
            return Err(Error::Invalid("cancellation unsigned not a map"));
        };
        if unsigned.len() != 11 {
            return Err(Error::Invalid("cancellation unsigned width"));
        }
        let Value::Map(delta) = &unsigned[6].1 else {
            return Err(Error::Invalid("cancellation delta not a map"));
        };
        if delta.len() != 1
            || unsigned[5].1 != Value::Integer(3)
            || unsigned[4].1 != Value::Bytes(row.transition_id.to_vec())
            || unsigned[1].1 != Value::Bytes(family.family_id.to_vec())
        {
            return Err(Error::Invalid("saved cancellation identity mismatch"));
        }
        let Value::Bytes(invitation) = &delta[0].1 else {
            return Err(Error::Invalid("cancellation invitation not bytes"));
        };
        let invitation_id = invitation
            .as_slice()
            .try_into()
            .map_err(|_| Error::Invalid("cancellation invitation length"))?;
        Ok(Self {
            family,
            invitation_id,
            transition_id: row.transition_id,
            candidate_bytes: row.candidate_bytes,
        })
    }

    pub fn candidate_bytes(&self) -> &[u8] {
        &self.candidate_bytes
    }
    pub fn invitation_id(&self) -> [u8; 16] {
        self.invitation_id
    }

    pub fn confirm(&self, store: &mut SqliteStore, committed: &[u8]) -> Result<(), Error> {
        let Value::Map(root) = cbor::decode(committed)? else {
            return Err(Error::Invalid("committed cancellation not a map"));
        };
        if root.len() != 4
            || cbor::encode(&Value::Map(vec![
                (1, root[0].1.clone()),
                (2, root[1].1.clone()),
            ]))? != self.candidate_bytes
        {
            return Err(Error::Invalid("committed cancellation candidate mismatch"));
        }
        if !committed_matches(store, self.family, &self.candidate_bytes)? {
            PublicHistorySession::resume(store, self.family)?.accept_control(store, committed)?;
        }
        store.delete_prepared_control(self.family, 3, self.transition_id)?;
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
        let bytes = cbor::encode(&Value::Map(vec![
            (1, root[0].1.clone()),
            (2, root[1].1.clone()),
        ]))?;
        if bytes == candidate {
            return Ok(true);
        }
    }
    Ok(false)
}

fn random_v4() -> Result<[u8; 16], Error> {
    let mut id = [0; 16];
    getrandom::fill(&mut id)?;
    id[6] = (id[6] & 0x0f) | 0x40;
    id[8] = (id[8] & 0x3f) | 0x80;
    Ok(id)
}
