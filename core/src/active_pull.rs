//! Pull a contiguous active-device log through a caller-owned transport.
//! The transport supplies authenticated GET responses; only signed relay
//! entries and receipts can advance the durable Family cursor.

use std::{collections::BTreeSet, future::Future};

use crate::{
    batch,
    cbor::{self, Value},
    shared_history::{self, PublicHistorySession},
    sqlite_store::{FamilyHandle, SqliteStore},
    sync_wire::{self, BatchResult, LogPage, OpaqueObject},
};

#[derive(Debug)]
pub enum Error {
    Batch(batch::Error),
    Cbor(cbor::Error),
    History(shared_history::Error),
    Store(crate::sqlite_store::Error),
    Wire(sync_wire::Error),
    Transport,
    Invalid(&'static str),
}

impl From<batch::Error> for Error {
    fn from(value: batch::Error) -> Self {
        Self::Batch(value)
    }
}
impl From<cbor::Error> for Error {
    fn from(value: cbor::Error) -> Self {
        Self::Cbor(value)
    }
}
impl From<shared_history::Error> for Error {
    fn from(value: shared_history::Error) -> Self {
        Self::History(value)
    }
}
impl From<crate::sqlite_store::Error> for Error {
    fn from(value: crate::sqlite_store::Error) -> Self {
        Self::Store(value)
    }
}
impl From<sync_wire::Error> for Error {
    fn from(value: sync_wire::Error) -> Self {
        Self::Wire(value)
    }
}

pub struct PullProgress {
    pub verified_cursor: u64,
    /// A response had no further visible entries. A withholding relay may
    /// still hide a suffix, so this is never proof of global currency.
    pub no_more_visible: bool,
}

pub struct HydrationProgress {
    pub fetched: usize,
    pub remaining: bool,
}

/// One bounded pass over signed history and its referenced opaque objects.
/// Readiness still requires the caller's credential/key checks after this pass.
pub struct SyncProgress {
    pub pull: PullProgress,
    pub hydration: HydrationProgress,
}

pub async fn pull_and_hydrate<F, Fut, E>(
    store: &mut SqliteStore,
    family: FamilyHandle,
    max_pages: usize,
    max_objects: usize,
    mut fetch: F,
) -> Result<SyncProgress, Error>
where
    F: FnMut(String) -> Fut,
    Fut: Future<Output = Result<Vec<u8>, E>>,
{
    let pull = pull_active_log(store, family, max_pages, &mut fetch).await?;
    let hydration = hydrate_manifest_objects(store, family, max_objects, fetch).await?;
    Ok(SyncProgress { pull, hydration })
}

struct ManifestObject {
    id: [u8; 16],
    kind: u16,
    transition_id: [u8; 16],
}

/// Fetch at most `max_pages` full-log pages. The caller signs each exact
/// path, carries its signature in the GET body or read header, and returns
/// the bounded CBOR response. Transport errors leave the verified prefix
/// durable.
/// Data readiness additionally requires all signed manifest objects and
/// keys; this pull alone makes no readiness claim.
pub async fn pull_active_log<F, Fut, E>(
    store: &mut SqliteStore,
    family: FamilyHandle,
    max_pages: usize,
    mut fetch: F,
) -> Result<PullProgress, Error>
where
    F: FnMut(String) -> Fut,
    Fut: Future<Output = Result<Vec<u8>, E>>,
{
    if max_pages == 0 {
        return Err(Error::Invalid("pull requires a positive page budget"));
    }
    let mut public = PublicHistorySession::resume(store, family)?;
    let family_hex = lower_hex(&family.family_id);
    for _ in 0..max_pages {
        let cursor = public.cursor();
        let path = format!("/v1/families/{family_hex}/log?after={cursor}");
        let bytes = fetch(path).await.map_err(|_| Error::Transport)?;
        let page = LogPage::decode(&bytes, family.family_id, cursor)?;
        if page.has_more && page.entries.is_empty() {
            return Err(Error::Invalid("relay claims more after an empty page"));
        }
        for entry in page.entries {
            match entry.kind {
                1 => public.accept_control(store, &entry.committed_bytes)?,
                2 => {
                    let batch_id = batch_id(&entry.committed_bytes)?;
                    let path = format!(
                        "/v1/families/{family_hex}/batch-results/{}",
                        lower_hex(&batch_id)
                    );
                    let bytes = fetch(path).await.map_err(|_| Error::Transport)?;
                    let result = BatchResult::decode(&bytes)?;
                    let receipt = result
                        .receipt_bytes
                        .ok_or(Error::Invalid("committed batch receipt unavailable"))?;
                    public.accept_batch(store, &entry.committed_bytes, &receipt)?;
                }
                _ => return Err(Error::Invalid("full log entry kind invalid")),
            }
            if public.cursor() != entry.cursor {
                return Err(Error::Invalid("accepted cursor differs from full log"));
            }
        }
        if !page.has_more {
            return Ok(PullProgress {
                verified_cursor: public.cursor(),
                no_more_visible: true,
            });
        }
    }
    Ok(PullProgress {
        verified_cursor: public.cursor(),
        no_more_visible: false,
    })
}

/// Fetch objects named by the locally verified control prefix. This can be
/// retried after a failed GET without moving the cursor or promoting a
/// partial Family to data-ready. The caller chooses a per-pass object budget.
pub async fn hydrate_manifest_objects<F, Fut, E>(
    store: &mut SqliteStore,
    family: FamilyHandle,
    max_objects: usize,
    mut fetch: F,
) -> Result<HydrationProgress, Error>
where
    F: FnMut(String) -> Fut,
    Fut: Future<Output = Result<Vec<u8>, E>>,
{
    if max_objects == 0 {
        return Err(Error::Invalid(
            "hydration requires a positive object budget",
        ));
    }
    let public = PublicHistorySession::resume(store, family)?;
    let history = store
        .shared_history(family)?
        .ok_or(Error::Invalid("shared history absent"))?;
    let mut existing: BTreeSet<_> = store
        .shared_objects(family)?
        .into_iter()
        .map(|(id, _)| id)
        .collect();
    let mut fetched = 0;
    let family_hex = lower_hex(&family.family_id);
    for control in std::iter::once(history.genesis_bytes.as_slice()).chain(
        history
            .entries
            .iter()
            .filter(|entry| entry.kind == 1)
            .map(|entry| entry.committed_bytes.as_slice()),
    ) {
        for object in manifest_objects(control)? {
            if existing.contains(&object.id) {
                continue;
            }
            if fetched == max_objects {
                return Ok(HydrationProgress {
                    fetched,
                    remaining: true,
                });
            }
            let path = format!(
                "/v1/families/{family_hex}/objects/{}",
                lower_hex(&object.id)
            );
            let bytes = fetch(path).await.map_err(|_| Error::Transport)?;
            let response = OpaqueObject::decode(&bytes, object.id)?;
            if response.kind != object.kind || response.transition_id != object.transition_id {
                return Err(Error::Invalid(
                    "object response differs from signed manifest",
                ));
            }
            public.accept_object(store, object.id, &response.object_bytes)?;
            existing.insert(object.id);
            fetched += 1;
        }
    }
    Ok(HydrationProgress {
        fetched,
        remaining: false,
    })
}

fn manifest_objects(bytes: &[u8]) -> Result<Vec<ManifestObject>, Error> {
    let value = cbor::decode_with_limits(
        bytes,
        cbor::Limits {
            max_bytes: 1024 * 1024,
            max_depth: 16,
        },
    )?;
    let Value::Map(root) = value else {
        return Err(Error::Invalid("control not map"));
    };
    let Some((1, Value::Map(unsigned))) = root.first() else {
        return Err(Error::Invalid("unsigned control absent"));
    };
    let Some((5, Value::Bytes(transition))) = unsigned.get(4) else {
        return Err(Error::Invalid("transition ID absent"));
    };
    let transition_id: [u8; 16] = transition
        .as_slice()
        .try_into()
        .map_err(|_| Error::Invalid("transition ID length"))?;
    let Some((10, Value::Array(manifest))) = unsigned.get(9) else {
        return Err(Error::Invalid("control manifest absent"));
    };
    let mut result = Vec::with_capacity(manifest.len());
    for item in manifest {
        let Value::Array(fields) = item else {
            return Err(Error::Invalid("manifest entry not array"));
        };
        if fields.len() != 4 {
            return Err(Error::Invalid("manifest entry length"));
        }
        let Value::Integer(kind) = fields[0] else {
            return Err(Error::Invalid("object kind absent"));
        };
        let kind = u16::try_from(kind).map_err(|_| Error::Invalid("object kind range"))?;
        let Value::Bytes(id) = &fields[1] else {
            return Err(Error::Invalid("object ID absent"));
        };
        let object_id = id
            .as_slice()
            .try_into()
            .map_err(|_| Error::Invalid("object ID length"))?;
        result.push(ManifestObject {
            id: object_id,
            kind,
            transition_id,
        });
    }
    Ok(result)
}

fn batch_id(envelope: &[u8]) -> Result<[u8; 16], Error> {
    let value = cbor::decode_with_limits(
        envelope,
        cbor::Limits {
            max_bytes: 1024 * 1024,
            max_depth: 16,
        },
    )?;
    let Value::Map(fields) = value else {
        return Err(Error::Invalid("batch envelope not map"));
    };
    let Some((1, header)) = fields.first() else {
        return Err(Error::Invalid("batch header absent"));
    };
    Ok(batch::Header::decode(&cbor::encode(header)?)?.batch_id)
}

fn lower_hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(DIGITS[(byte >> 4) as usize] as char);
        out.push(DIGITS[(byte & 15) as usize] as char);
    }
    out
}
