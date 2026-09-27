//! Pull a contiguous active-device log through a caller-owned transport.
//! The transport supplies authenticated GET responses; only signed relay
//! entries and receipts can advance the durable Family cursor.

use std::future::Future;

use crate::{
    batch,
    cbor::{self, Value},
    shared_history::{self, PublicHistorySession},
    sqlite_store::{FamilyHandle, SqliteStore},
    sync_wire::{self, BatchResult, LogPage},
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

/// Fetch at most `max_pages` full-log pages. The caller signs each exact
/// path, sends its signature as the GET body, and returns the bounded CBOR
/// response. Transport errors leave the already verified prefix durable.
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
