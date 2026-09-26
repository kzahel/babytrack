//! Deterministic in-memory replay of already verified, authorized batches.
//!
//! The caller verifies the relay receipt, cursor order, signer membership at
//! that cursor, and epoch before passing a batch here. Durable log writes and
//! projection updates must later be wrapped in one storage transaction.

use std::collections::BTreeMap;

use crate::{
    batch::{AuthenticatedBatch, OpenedBatch},
    cbor::{self, Value},
    operation::{Kind, Operation, Scope},
    record_validity,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    pub value: Value,
    pub canonical_bytes: Vec<u8>,
    pub cursor: u64,
    pub operation_index: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    pub id: [u8; 16],
    pub scope: Scope,
    pub record_type: String,
    pub child_id: Option<[u8; 16]>,
    pub deleted: bool,
    /// Cursor and zero-based operation index of the winning tombstone edit.
    pub tombstone_stamp: Option<(u64, usize)>,
    fields: BTreeMap<u64, Field>,
}

impl Record {
    pub fn field(&self, id: u64) -> Option<&Field> {
        self.fields.get(&id)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InertBatch {
    pub cursor: u64,
    pub object_hash: [u8; 32],
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Applied,
    Inert(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    WrongFamily,
    WrongCursor,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Projection {
    family_id: [u8; 16],
    last_cursor: u64,
    records: BTreeMap<[u8; 16], Record>,
    seen_operations: BTreeMap<[u8; 16], Vec<u8>>,
    inert_batches: Vec<InertBatch>,
}

impl Projection {
    pub fn new(family_id: [u8; 16]) -> Self {
        Self {
            family_id,
            last_cursor: 0,
            records: BTreeMap::new(),
            seen_operations: BTreeMap::new(),
            inert_batches: Vec::new(),
        }
    }

    pub fn last_cursor(&self) -> u64 {
        self.last_cursor
    }

    pub fn family_id(&self) -> [u8; 16] {
        self.family_id
    }

    pub fn record(&self, id: &[u8; 16]) -> Option<&Record> {
        self.records.get(id)
    }

    pub fn inert_batches(&self) -> &[InertBatch] {
        &self.inert_batches
    }

    /// Advance over a separately verified control-chain entry in the shared
    /// cursor stream. Its membership and key effects belong to control replay.
    pub fn advance_control(&mut self, cursor: u64) -> Result<(), Error> {
        if self.last_cursor.checked_add(1) != Some(cursor) {
            return Err(Error::WrongCursor);
        }
        self.last_cursor = cursor;
        Ok(())
    }

    /// Consume an authenticated data entry even if its plaintext is malformed.
    pub fn apply_authenticated(
        &mut self,
        batch: &AuthenticatedBatch,
        cursor: u64,
    ) -> Result<Outcome, Error> {
        self.check_position(&batch.header.family_id, cursor)?;
        match batch.parse() {
            Ok(opened) => self.apply(&opened, cursor),
            Err(error) => Ok(self.mark_inert(cursor, batch.object_hash, format!("{error:?}"))),
        }
    }

    /// Replay one batch in relay order. A semantic violation consumes its
    /// cursor and leaves all record and dedupe state from before the batch.
    pub fn apply(&mut self, batch: &OpenedBatch, cursor: u64) -> Result<Outcome, Error> {
        self.check_position(&batch.header.family_id, cursor)?;
        let mut records = self.records.clone();
        let mut seen_operations = self.seen_operations.clone();
        for (index, operation) in batch.operations.iter().enumerate() {
            if let Err(reason) =
                apply_operation(&mut records, &mut seen_operations, operation, cursor, index)
            {
                return Ok(self.mark_inert(cursor, batch.object_hash, reason.to_owned()));
            }
        }
        self.records = records;
        self.seen_operations = seen_operations;
        self.last_cursor = cursor;
        Ok(Outcome::Applied)
    }

    fn check_position(&self, family_id: &[u8; 16], cursor: u64) -> Result<(), Error> {
        if family_id != &self.family_id {
            return Err(Error::WrongFamily);
        }
        if self.last_cursor.checked_add(1) != Some(cursor) {
            return Err(Error::WrongCursor);
        }
        Ok(())
    }

    fn mark_inert(&mut self, cursor: u64, object_hash: [u8; 32], reason: String) -> Outcome {
        self.last_cursor = cursor;
        self.inert_batches.push(InertBatch {
            cursor,
            object_hash,
            reason: reason.clone(),
        });
        Outcome::Inert(reason)
    }
}

fn apply_operation(
    records: &mut BTreeMap<[u8; 16], Record>,
    seen_operations: &mut BTreeMap<[u8; 16], Vec<u8>>,
    operation: &Operation,
    cursor: u64,
    index: usize,
) -> Result<(), &'static str> {
    if let Some(prior) = seen_operations.get(&operation.operation_id) {
        return if prior == operation.canonical_bytes() {
            Ok(())
        } else {
            Err("operation ID reused with different bytes")
        };
    }
    match operation.kind {
        Kind::Create => {
            if records.contains_key(&operation.record_id) {
                return Err("record already created");
            }
            if let Some(child_id) = operation.child_id
                && records
                    .get(&child_id)
                    .is_none_or(|child| child.scope != Scope::Child)
            {
                return Err("activity child does not exist");
            }
            let record_type = operation
                .record_type
                .as_deref()
                .expect("decoded create type");
            record_validity::validate_fields(
                operation.scope,
                record_type,
                operation.kind,
                &operation.fields,
            )?;
            let mut record = Record {
                id: operation.record_id,
                scope: operation.scope,
                record_type: record_type.to_owned(),
                child_id: operation.child_id,
                deleted: false,
                tombstone_stamp: None,
                fields: BTreeMap::new(),
            };
            apply_fields(&mut record, operation, cursor, index);
            validate_record(&record)?;
            records.insert(operation.record_id, record);
        }
        Kind::Set | Kind::Delete | Kind::Restore => {
            let record = records
                .get_mut(&operation.record_id)
                .ok_or("record does not exist")?;
            if record.scope != operation.scope {
                return Err("operation scope differs from record");
            }
            match operation.kind {
                Kind::Set => {
                    record_validity::validate_fields(
                        record.scope,
                        &record.record_type,
                        operation.kind,
                        &operation.fields,
                    )?;
                    apply_fields(record, operation, cursor, index);
                    validate_record(record)?;
                }
                Kind::Delete => {
                    record.deleted = true;
                    record.tombstone_stamp = Some((cursor, index));
                }
                Kind::Restore => {
                    record.deleted = false;
                    record.tombstone_stamp = Some((cursor, index));
                }
                Kind::Create => unreachable!(),
            }
        }
    }
    seen_operations.insert(operation.operation_id, operation.canonical_bytes().to_vec());
    Ok(())
}

fn apply_fields(record: &mut Record, operation: &Operation, cursor: u64, index: usize) {
    for (id, value) in &operation.fields {
        record.fields.insert(
            *id,
            Field {
                value: value.clone(),
                canonical_bytes: cbor::encode(value).expect("decoded canonical field"),
                cursor,
                operation_index: index,
            },
        );
    }
}

fn validate_record(record: &Record) -> Result<(), &'static str> {
    if record.record_type == "pump"
        && record
            .field(102)
            .is_some_and(|field| field.value != Value::Null)
        && [100, 101].iter().any(|id| {
            record
                .field(*id)
                .is_some_and(|field| field.value != Value::Null)
        })
    {
        return Err("pump total conflicts with side amounts");
    }
    Ok(())
}
