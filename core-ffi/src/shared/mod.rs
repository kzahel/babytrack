//! Durable shared native store and common binding adapters.

use super::*;

mod actions;
mod authority;
mod enrollment;
mod sync;

#[derive(uniffi::Object)]
pub struct NativeSharedStore {
    store: Mutex<SqliteStore>,
}

#[uniffi::export]
impl NativeSharedStore {
    #[uniffi::constructor]
    pub fn open(path: String) -> Result<Arc<Self>, BindingError> {
        Ok(Arc::new(Self {
            store: Mutex::new(SqliteStore::open(path).map_err(rejected)?),
        }))
    }
}

enum ActiveReadSigner {
    Recipient(EnrollmentAttempt),
    Manager(ManagerCreation),
}
impl ActiveReadSigner {
    fn sign_get(&self, path: &str) -> Result<babytrack_core::sync_wire::SignedRead, BindingError> {
        match self {
            Self::Recipient(value) => value.sign_get(path).map_err(rejected),
            Self::Manager(value) => value.sign_get(path).map_err(rejected),
        }
    }
}

fn ready_session_for(
    store: &mut SqliteStore,
    family: FamilyHandle,
    wrapping: &[u8; 32],
) -> Result<ReadyFamilySession, BindingError> {
    if store
        .has_enrollment_attempt(family.family_id)
        .map_err(rejected)?
    {
        let attempt =
            EnrollmentAttempt::resume(store, family.family_id, wrapping).map_err(rejected)?;
        if attempt.family() != family {
            return Err(BindingError::InvalidBytes);
        }
        ReadyFamilySession::from_enrollment(store, &attempt).map_err(rejected)
    } else {
        ManagerCreation::resume(store, family, wrapping)
            .map_err(rejected)?
            .ready_session(store)
            .map_err(rejected)
    }
}

fn has_issued_invitation(store: &SqliteStore, family: FamilyHandle) -> Result<bool, BindingError> {
    let public = PublicHistorySession::resume(store, family).map_err(rejected)?;
    let Value::Map(state) =
        cbor::decode(&public.chain().state_bytes().map_err(rejected)?).map_err(rejected)?
    else {
        return Err(BindingError::InvalidBytes);
    };
    let Some((7, Value::Array(invitations))) = state.get(6) else {
        return Err(BindingError::InvalidBytes);
    };
    Ok(!invitations.is_empty())
}

fn admitted_manager(
    store: &mut SqliteStore,
    family: FamilyHandle,
    wrapping_key: &[u8; 32],
) -> Result<Option<EnrollmentAttempt>, BindingError> {
    if !store
        .has_enrollment_attempt(family.family_id)
        .map_err(rejected)?
    {
        return Ok(None);
    }
    let holder =
        EnrollmentAttempt::resume(store, family.family_id, wrapping_key).map_err(rejected)?;
    if holder.family() != family {
        return Err(BindingError::InvalidBytes);
    }
    let ready = ReadyFamilySession::from_enrollment(store, &holder).map_err(rejected)?;
    let public = PublicHistorySession::resume(store, family).map_err(rejected)?;
    if ready.observed_cursor() != public.cursor()
        || ready.observed_head() != public.head_hash()
        || !public
            .chain()
            .active_devices()
            .map_err(rejected)?
            .iter()
            .any(|device| device.device_id == family.device_id && device.role == 2)
    {
        return Err(BindingError::Rejected(
            "Device is not a current ready manager".into(),
        ));
    }
    Ok(Some(holder))
}
