//! Durable opaque relay storage, authority commits, and authenticated reads.

use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

use babytrack_wire::{
    authority as public_authority,
    cbor::{self, Value},
    crypto,
};
use rusqlite::{
    Connection, OpenFlags, OptionalExtension, Transaction, TransactionBehavior, params,
};

use crate::{authority, batch_authority, public_ledger, read_auth, receipt};

#[derive(Debug)]
#[allow(dead_code)] // Error variants retain diagnostic details at the relay boundary.
#[allow(private_interfaces)] // The test-harness feature exposes only Debug/errors.
pub enum Error {
    Sql(rusqlite::Error),
    Cbor(cbor::Error),
    Crypto(crypto::Error),
    Authority(authority::Error),
    Ledger(public_ledger::Error),
    Batch(batch_authority::Error),
    ReadAuth(read_auth::Error),
    Receipt(receipt::Error),
    Clock,
    Invalid(&'static str),
}
impl From<rusqlite::Error> for Error {
    fn from(v: rusqlite::Error) -> Self {
        Self::Sql(v)
    }
}
impl From<cbor::Error> for Error {
    fn from(v: cbor::Error) -> Self {
        Self::Cbor(v)
    }
}
impl From<crypto::Error> for Error {
    fn from(v: crypto::Error) -> Self {
        Self::Crypto(v)
    }
}
impl From<authority::Error> for Error {
    fn from(v: authority::Error) -> Self {
        Self::Authority(v)
    }
}
impl From<public_ledger::Error> for Error {
    fn from(v: public_ledger::Error) -> Self {
        Self::Ledger(v)
    }
}
impl From<batch_authority::Error> for Error {
    fn from(v: batch_authority::Error) -> Self {
        Self::Batch(v)
    }
}
impl From<read_auth::Error> for Error {
    fn from(v: read_auth::Error) -> Self {
        Self::ReadAuth(v)
    }
}
impl From<receipt::Error> for Error {
    fn from(v: receipt::Error) -> Self {
        Self::Receipt(v)
    }
}

pub struct RelayStore {
    db: Connection,
    relay_seed: [u8; 32],
    relay_public: [u8; 32],
}

type SavedReservation = (Vec<u8>, Vec<u8>, i64, Option<Vec<u8>>);
type StoredCommittedObject = (i64, Vec<u8>, Vec<u8>, Vec<u8>);
type StoredBatchResult = (Vec<u8>, Vec<u8>, Vec<u8>, Vec<u8>, i64);

enum ControlReader {
    Manager,
    Active,
    Removed {
        device_id: [u8; 16],
        signing_public: [u8; 32],
        relay_id: [u8; 32],
    },
    Invitation {
        issue_object: [u8; 16],
    },
    Pending {
        challenge_object: Option<[u8; 16]>,
    },
}

mod commits;
mod history;
mod initialization;
mod integrity;
mod objects;
mod reads;
mod staging;
mod validation;

use history::*;
use integrity::*;
use objects::*;
use validation::*;

#[cfg(test)]
mod tests;
