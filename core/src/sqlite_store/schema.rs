//! Schema persistence. Each method retains its complete transaction.

use super::*;

impl SqliteStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, Error> {
        let mut connection = Connection::open(path)?;
        connection.busy_timeout(std::time::Duration::from_secs(10))?;
        connection.execute_batch("PRAGMA foreign_keys = ON;")?;
        let observed_version: u32 =
            connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
        if observed_version == 4 {
            return Ok(Self { connection });
        }
        if observed_version > 4 {
            return Err(Error::CorruptState);
        }
        if observed_version == 0 {
            // SQLite may return BUSY immediately while another fresh opener
            // switches the journal mode, even with busy_timeout installed.
            // Journal mode cannot be changed inside the schema transaction.
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
            loop {
                match connection.execute_batch("PRAGMA journal_mode = WAL;") {
                    Ok(()) => break,
                    Err(rusqlite::Error::SqliteFailure(code, _))
                        if matches!(
                            code.code,
                            rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked
                        ) && std::time::Instant::now() < deadline =>
                    {
                        std::thread::sleep(std::time::Duration::from_millis(10));
                    }
                    Err(error) => return Err(error.into()),
                }
            }
        }
        // Recheck under the writer lock: another opener may have completed a
        // migration after our first version read.
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let schema_version: u32 =
            transaction.pragma_query_value(None, "user_version", |row| row.get(0))?;
        if schema_version > 4 {
            return Err(Error::CorruptState);
        }
        if schema_version == 0 {
            transaction.execute_batch(
                "CREATE TABLE IF NOT EXISTS families (
               family_id BLOB PRIMARY KEY CHECK(length(family_id) = 16),
               device_id BLOB NOT NULL CHECK(length(device_id) = 16),
               last_index INTEGER NOT NULL DEFAULT 0 CHECK(last_index >= 0),
               last_hlc_wall INTEGER,
               last_hlc_counter INTEGER,
               CHECK ((last_hlc_wall IS NULL) = (last_hlc_counter IS NULL))
             );
             CREATE TABLE IF NOT EXISTS restored_origins (
               family_id BLOB PRIMARY KEY CHECK(length(family_id) = 16),
               source_family_id BLOB NOT NULL CHECK(length(source_family_id) = 16),
               snapshot_utc_ms INTEGER NOT NULL,
               source_cursor BLOB CHECK(source_cursor IS NULL OR length(source_cursor) = 8),
               known_gap INTEGER NOT NULL CHECK(known_gap IN (0,1)),
               FOREIGN KEY (family_id) REFERENCES families(family_id)
             );
             CREATE TABLE IF NOT EXISTS private_copies (
               source_family_id BLOB NOT NULL CHECK(length(source_family_id)=16),
               source_device_id BLOB NOT NULL CHECK(length(source_device_id)=16),
               copy_family_id BLOB NOT NULL UNIQUE CHECK(length(copy_family_id)=16),
               PRIMARY KEY (source_family_id,source_device_id),
               FOREIGN KEY (source_family_id) REFERENCES families(family_id),
               FOREIGN KEY (copy_family_id) REFERENCES families(family_id)
             );
             CREATE TABLE IF NOT EXISTS verified_removals (
               source_family_id BLOB NOT NULL CHECK(length(source_family_id)=16),
               source_device_id BLOB NOT NULL CHECK(length(source_device_id)=16),
               transition_id BLOB NOT NULL CHECK(length(transition_id)=16),
               cursor INTEGER NOT NULL CHECK(cursor > 0),
               source_cursor INTEGER NOT NULL CHECK(source_cursor > 0),
               known_gap INTEGER NOT NULL CHECK(known_gap IN (0,1)),
               committed_bytes BLOB NOT NULL,
               PRIMARY KEY (source_family_id,source_device_id),
               FOREIGN KEY (source_family_id) REFERENCES families(family_id)
             );
             CREATE TABLE IF NOT EXISTS removal_copies (
               source_family_id BLOB NOT NULL CHECK(length(source_family_id)=16),
               source_device_id BLOB NOT NULL CHECK(length(source_device_id)=16),
               transition_id BLOB NOT NULL CHECK(length(transition_id)=16),
               copy_family_id BLOB NOT NULL UNIQUE CHECK(length(copy_family_id)=16),
               PRIMARY KEY (source_family_id,source_device_id,transition_id),
               FOREIGN KEY (source_family_id,source_device_id)
                 REFERENCES verified_removals(source_family_id,source_device_id),
               FOREIGN KEY (copy_family_id) REFERENCES families(family_id)
             );
             CREATE TABLE IF NOT EXISTS local_operations (
               family_id BLOB NOT NULL,
               append_index INTEGER NOT NULL CHECK(append_index > 0),
               operation_id BLOB NOT NULL CHECK(length(operation_id) = 16),
               operation_bytes BLOB NOT NULL,
               PRIMARY KEY (family_id, append_index),
               UNIQUE (family_id, operation_id),
               FOREIGN KEY (family_id) REFERENCES families(family_id)
             );
             CREATE TABLE IF NOT EXISTS local_sync_state (
               family_id BLOB PRIMARY KEY,
               accepted_index INTEGER NOT NULL DEFAULT 0 CHECK(accepted_index >= 0),
               next_sequence INTEGER NOT NULL DEFAULT 1 CHECK(next_sequence > 0),
               FOREIGN KEY (family_id) REFERENCES families(family_id)
             );
             CREATE TABLE IF NOT EXISTS local_outbox (
               family_id BLOB PRIMARY KEY,
               from_index INTEGER NOT NULL CHECK(from_index > 0),
               to_index INTEGER NOT NULL CHECK(to_index >= from_index),
               sequence INTEGER NOT NULL CHECK(sequence > 0),
               batch_id BLOB NOT NULL CHECK(length(batch_id) = 16),
               envelope_bytes BLOB NOT NULL,
               object_hash BLOB NOT NULL CHECK(length(object_hash) = 32),
               FOREIGN KEY (family_id) REFERENCES families(family_id)
             );
             CREATE TABLE IF NOT EXISTS local_batch_reservations (
               family_id BLOB NOT NULL,
               epoch INTEGER NOT NULL CHECK(epoch > 0),
               nonce BLOB NOT NULL CHECK(length(nonce) = 24),
               batch_id BLOB NOT NULL CHECK(length(batch_id) = 16),
               PRIMARY KEY (family_id, batch_id),
               UNIQUE (family_id, epoch, nonce),
               FOREIGN KEY (family_id) REFERENCES families(family_id)
             );
             CREATE TABLE IF NOT EXISTS rejected_local_batches (
               family_id BLOB NOT NULL,
               batch_id BLOB NOT NULL CHECK(length(batch_id) = 16),
               envelope_bytes BLOB NOT NULL,
               receipt_bytes BLOB NOT NULL,
               rejected_at_cursor INTEGER NOT NULL CHECK(rejected_at_cursor >= 1),
               PRIMARY KEY (family_id, batch_id),
               FOREIGN KEY (family_id) REFERENCES families(family_id)
             );
             CREATE TABLE IF NOT EXISTS accepted_local_batches (
               family_id BLOB NOT NULL,
               cursor INTEGER NOT NULL CHECK(cursor > 1),
               to_index INTEGER NOT NULL CHECK(to_index > 0),
               batch_id BLOB NOT NULL CHECK(length(batch_id) = 16),
               envelope_bytes BLOB NOT NULL,
               receipt_bytes BLOB NOT NULL,
               PRIMARY KEY (family_id, cursor),
               UNIQUE (family_id, batch_id),
               FOREIGN KEY (family_id) REFERENCES families(family_id)
             );
             CREATE TABLE IF NOT EXISTS shared_roots (
               family_id BLOB PRIMARY KEY CHECK(length(family_id) = 16),
               relay_public_key BLOB NOT NULL CHECK(length(relay_public_key) = 32),
               genesis_bytes BLOB NOT NULL,
               pinned_cursor INTEGER NOT NULL CHECK(pinned_cursor >= 1),
               pinned_head BLOB NOT NULL CHECK(length(pinned_head) = 32),
               FOREIGN KEY (family_id) REFERENCES families(family_id)
             );
             CREATE TABLE IF NOT EXISTS shared_entries (
               family_id BLOB NOT NULL,
               cursor INTEGER NOT NULL CHECK(cursor > 1),
               kind INTEGER NOT NULL CHECK(kind IN (1, 2)),
               committed_bytes BLOB NOT NULL,
               receipt_bytes BLOB NOT NULL,
               PRIMARY KEY (family_id, cursor),
               FOREIGN KEY (family_id) REFERENCES shared_roots(family_id)
             );
             CREATE TABLE IF NOT EXISTS shared_objects (
               family_id BLOB NOT NULL,
               object_id BLOB NOT NULL CHECK(length(object_id) = 16),
               transition_id BLOB NOT NULL CHECK(length(transition_id) = 16),
               kind INTEGER NOT NULL CHECK(kind > 0),
               object_bytes BLOB NOT NULL,
               PRIMARY KEY (family_id, object_id),
               FOREIGN KEY (family_id) REFERENCES shared_roots(family_id)
             );
             CREATE TABLE IF NOT EXISTS enrollment_attempts (
               family_id BLOB PRIMARY KEY CHECK(length(family_id) = 16),
               device_id BLOB NOT NULL CHECK(length(device_id) = 16),
               invitation_id BLOB NOT NULL CHECK(length(invitation_id) = 16),
               genesis_bytes BLOB NOT NULL,
               issue_bytes BLOB NOT NULL,
               candidate_bytes BLOB NOT NULL,
               secret_nonce BLOB NOT NULL CHECK(length(secret_nonce) = 24),
               secret_ciphertext BLOB NOT NULL,
               FOREIGN KEY (family_id) REFERENCES families(family_id)
             );
             CREATE TABLE IF NOT EXISTS enrollment_controls (
               family_id BLOB NOT NULL,
               cursor INTEGER NOT NULL CHECK(cursor > 1),
               committed_bytes BLOB NOT NULL,
               PRIMARY KEY (family_id,cursor),
               FOREIGN KEY (family_id) REFERENCES enrollment_attempts(family_id)
             );
             CREATE TABLE IF NOT EXISTS manager_creations (
               family_id BLOB PRIMARY KEY CHECK(length(family_id) = 16),
               device_id BLOB NOT NULL CHECK(length(device_id) = 16),
               relay_public_key BLOB NOT NULL CHECK(length(relay_public_key) = 32),
               promotion_id BLOB NOT NULL CHECK(length(promotion_id) = 16),
               transition_id BLOB NOT NULL CHECK(length(transition_id) = 16),
               object_id BLOB NOT NULL CHECK(length(object_id) = 16),
               object_bytes BLOB NOT NULL,
               candidate_bytes BLOB NOT NULL,
               secret_nonce BLOB NOT NULL CHECK(length(secret_nonce) = 24),
               secret_ciphertext BLOB NOT NULL,
               FOREIGN KEY (family_id) REFERENCES families(family_id)
             );
             CREATE TABLE IF NOT EXISTS manager_promotion_chunks (
               family_id BLOB NOT NULL CHECK(length(family_id) = 16),
               chunk_index INTEGER NOT NULL CHECK(chunk_index >= 0),
               object_id BLOB NOT NULL CHECK(length(object_id) = 16),
               first_local_index INTEGER NOT NULL CHECK(first_local_index > 0),
               last_local_index INTEGER NOT NULL CHECK(last_local_index >= first_local_index),
               object_bytes BLOB NOT NULL,
               PRIMARY KEY (family_id, chunk_index),
               UNIQUE (family_id, object_id),
               FOREIGN KEY (family_id) REFERENCES manager_creations(family_id)
             );
             CREATE TABLE IF NOT EXISTS first_invite_issues (
               family_id BLOB PRIMARY KEY CHECK(length(family_id) = 16),
               device_id BLOB NOT NULL CHECK(length(device_id) = 16),
               invitation_id BLOB NOT NULL CHECK(length(invitation_id) = 16),
               transition_id BLOB NOT NULL CHECK(length(transition_id) = 16),
               object_id BLOB NOT NULL CHECK(length(object_id) = 16),
               object_bytes BLOB NOT NULL,
               candidate_bytes BLOB NOT NULL,
               secret_nonce BLOB NOT NULL CHECK(length(secret_nonce) = 24),
               secret_ciphertext BLOB NOT NULL,
               FOREIGN KEY (family_id) REFERENCES families(family_id)
             );
             CREATE TABLE IF NOT EXISTS prepared_controls (
               family_id BLOB NOT NULL CHECK(length(family_id) = 16),
               kind INTEGER NOT NULL CHECK(kind > 0),
               device_id BLOB NOT NULL CHECK(length(device_id) = 16),
               transition_id BLOB NOT NULL CHECK(length(transition_id) = 16),
               candidate_bytes BLOB NOT NULL,
               objects_bytes BLOB NOT NULL,
               secret_nonce BLOB NOT NULL CHECK(length(secret_nonce) = 24),
               secret_ciphertext BLOB NOT NULL,
               PRIMARY KEY (family_id, kind),
               FOREIGN KEY (family_id) REFERENCES families(family_id)
             );
             INSERT OR IGNORE INTO local_sync_state(family_id)
               SELECT family_id FROM families;
             PRAGMA user_version = 1;",
            )?;
        }
        if schema_version <= 1 {
            migrate_invite_issues(&transaction)?;
        }
        if schema_version <= 2 {
            migrate_claim_candidates(&transaction)?;
        }
        if schema_version <= 3 {
            migrate_enrollment_terminal_status(&transaction)?;
        }
        transaction.commit()?;
        Ok(Self { connection })
    }
}

/// Version two replaces the one-row invitation slot with per-invitation
/// durable preparation. Copying legacy bytes preserves exact retries and
/// bearer-link reconstruction after an upgrade.
fn migrate_invite_issues(connection: &Connection) -> Result<(), Error> {
    connection.execute_batch(
        "CREATE TABLE invite_issues (
           family_id BLOB NOT NULL CHECK(length(family_id) = 16),
           device_id BLOB NOT NULL CHECK(length(device_id) = 16),
           invitation_id BLOB NOT NULL CHECK(length(invitation_id) = 16),
           transition_id BLOB NOT NULL CHECK(length(transition_id) = 16),
           object_id BLOB NOT NULL CHECK(length(object_id) = 16),
           object_bytes BLOB NOT NULL,
           candidate_bytes BLOB NOT NULL,
           secret_nonce BLOB NOT NULL CHECK(length(secret_nonce) = 24),
           secret_ciphertext BLOB NOT NULL,
           is_first INTEGER NOT NULL CHECK(is_first IN (0,1)),
           PRIMARY KEY (family_id, invitation_id),
           UNIQUE (family_id, transition_id),
           UNIQUE (family_id, object_id),
           FOREIGN KEY (family_id) REFERENCES families(family_id)
         );
         CREATE UNIQUE INDEX first_invite_per_family
           ON invite_issues(family_id) WHERE is_first=1;
         INSERT INTO invite_issues
           (family_id,device_id,invitation_id,transition_id,object_id,
            object_bytes,candidate_bytes,secret_nonce,secret_ciphertext,is_first)
           SELECT family_id,device_id,invitation_id,transition_id,object_id,
                  object_bytes,candidate_bytes,secret_nonce,secret_ciphertext,1
           FROM first_invite_issues;
         PRAGMA user_version = 2;",
    )?;
    Ok(())
}

fn migrate_claim_candidates(connection: &Connection) -> Result<(), Error> {
    connection.execute_batch(
        "CREATE TABLE enrollment_claim_candidates (
           family_id BLOB NOT NULL CHECK(length(family_id) = 16),
           transition_id BLOB NOT NULL CHECK(length(transition_id) = 16),
           candidate_bytes BLOB NOT NULL,
           PRIMARY KEY (family_id, transition_id),
           FOREIGN KEY (family_id) REFERENCES enrollment_attempts(family_id)
         );
         PRAGMA user_version = 3;",
    )?;
    Ok(())
}

fn migrate_enrollment_terminal_status(connection: &Connection) -> Result<(), Error> {
    connection.execute_batch(
        "CREATE TABLE enrollment_terminal_status (
           family_id BLOB PRIMARY KEY CHECK(length(family_id) = 16),
           response_bytes BLOB NOT NULL,
           FOREIGN KEY (family_id) REFERENCES enrollment_attempts(family_id)
         );
         PRAGMA user_version = 4;",
    )?;
    Ok(())
}
