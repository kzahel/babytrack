//! Schema initialization and legacy checkpoint migration.

use super::*;

impl RelayStore {
    pub fn open(path: impl AsRef<Path>, relay_seed: [u8; 32]) -> Result<Self, Error> {
        let db = Connection::open(path)?;
        Self::initialize(db, relay_seed, false)
    }

    pub fn migrate_legacy_private_checkpoint(
        path: impl AsRef<Path>,
        relay_seed: [u8; 32],
    ) -> Result<(), Error> {
        let db = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
        Self::initialize(db, relay_seed, true)?;
        Ok(())
    }

    pub(super) fn initialize(
        mut db: Connection,
        relay_seed: [u8; 32],
        allow_legacy_checkpoint: bool,
    ) -> Result<Self, Error> {
        db.execute_batch(
            "PRAGMA foreign_keys=ON;
             PRAGMA synchronous=FULL;
             CREATE TABLE IF NOT EXISTS relay_identity (
               singleton INTEGER PRIMARY KEY CHECK (singleton=1), public_key BLOB NOT NULL
             );
             CREATE TABLE IF NOT EXISTS families (
               family_id BLOB PRIMARY KEY, reservation_hash BLOB NOT NULL,
               candidate_bytes BLOB NOT NULL, active INTEGER NOT NULL CHECK (active IN (0,1)),
               committed_bytes BLOB, head_hash BLOB, cursor INTEGER NOT NULL DEFAULT 0
             );
             CREATE TABLE IF NOT EXISTS staged_objects (
               family_id BLOB NOT NULL, object_id BLOB NOT NULL, kind INTEGER NOT NULL,
               object_hash BLOB NOT NULL, object_bytes BLOB NOT NULL,
               PRIMARY KEY (family_id, object_id)
             );
             CREATE TABLE IF NOT EXISTS staged_issues (
               family_id BLOB PRIMARY KEY, transition_id BLOB NOT NULL,
               candidate_bytes BLOB NOT NULL
             );
             CREATE TABLE IF NOT EXISTS staged_challenges (
               family_id BLOB PRIMARY KEY, transition_id BLOB NOT NULL,
               candidate_bytes BLOB NOT NULL
             );
             CREATE TABLE IF NOT EXISTS staged_admissions (
               family_id BLOB PRIMARY KEY, transition_id BLOB NOT NULL,
               candidate_bytes BLOB NOT NULL
             );
             CREATE TABLE IF NOT EXISTS staged_removals (
               family_id BLOB PRIMARY KEY, transition_id BLOB NOT NULL,
               candidate_bytes BLOB NOT NULL
             );
             CREATE TABLE IF NOT EXISTS staged_controls (
               family_id BLOB NOT NULL, transition_id BLOB NOT NULL,
               candidate_bytes BLOB NOT NULL,
               PRIMARY KEY (family_id, transition_id)
             );
             CREATE TABLE IF NOT EXISTS staged_control_objects (
               family_id BLOB NOT NULL, transition_id BLOB NOT NULL,
               object_id BLOB NOT NULL, kind INTEGER NOT NULL,
               object_hash BLOB NOT NULL, object_bytes BLOB NOT NULL,
               PRIMARY KEY (family_id, transition_id, object_id)
             );
             CREATE TABLE IF NOT EXISTS object_reservations (
               family_id BLOB NOT NULL, object_id BLOB NOT NULL,
               kind INTEGER NOT NULL, object_hash BLOB NOT NULL,
               PRIMARY KEY (family_id, object_id)
             );
             CREATE TABLE IF NOT EXISTS committed_objects (
               family_id BLOB NOT NULL, object_id BLOB NOT NULL, kind INTEGER NOT NULL,
               object_hash BLOB NOT NULL, object_bytes BLOB NOT NULL,
               transition_id BLOB NOT NULL, PRIMARY KEY (family_id, object_id)
             );
             CREATE TABLE IF NOT EXISTS entries (
               family_id BLOB NOT NULL, cursor INTEGER NOT NULL, kind INTEGER NOT NULL,
               committed_bytes BLOB NOT NULL, PRIMARY KEY (family_id, cursor)
             );
             CREATE TABLE IF NOT EXISTS read_requests (
               family_id BLOB NOT NULL, signer_id BLOB NOT NULL,
               request_id BLOB NOT NULL, request_hash BLOB NOT NULL,
               PRIMARY KEY (family_id, signer_id, request_id)
             );
             CREATE TABLE IF NOT EXISTS batch_results (
               family_id BLOB NOT NULL, batch_id BLOB NOT NULL,
               author_id BLOB NOT NULL, sequence INTEGER NOT NULL,
               envelope_bytes BLOB NOT NULL, receipt_bytes BLOB NOT NULL,
               cursor INTEGER NOT NULL,
               PRIMARY KEY (family_id, batch_id),
               UNIQUE (family_id, author_id, sequence),
               UNIQUE (family_id, cursor)
             );
             CREATE TABLE IF NOT EXISTS rejected_batch_results (
               family_id BLOB NOT NULL, batch_id BLOB NOT NULL,
               envelope_bytes BLOB NOT NULL, receipt_bytes BLOB NOT NULL,
               PRIMARY KEY (family_id, batch_id)
             );
             CREATE TABLE IF NOT EXISTS private_integrity (
               singleton INTEGER PRIMARY KEY CHECK(singleton=1),
               digest BLOB NOT NULL, signature BLOB NOT NULL
             );",
        )?;
        // Existing development relays used one staging slot per Family. Move
        // an interrupted candidate into the candidate-scoped tables before
        // accepting new staging requests. Genesis keeps its own staging table.
        let migration = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        migration.execute_batch(
            "INSERT INTO staged_controls(family_id,transition_id,candidate_bytes)
               SELECT family_id,transition_id,candidate_bytes FROM staged_issues
               UNION ALL SELECT family_id,transition_id,candidate_bytes FROM staged_challenges
               UNION ALL SELECT family_id,transition_id,candidate_bytes FROM staged_admissions
               UNION ALL SELECT family_id,transition_id,candidate_bytes FROM staged_removals;
             INSERT INTO staged_control_objects(family_id,transition_id,object_id,kind,object_hash,object_bytes)
               SELECT o.family_id,c.transition_id,o.object_id,o.kind,o.object_hash,o.object_bytes
               FROM staged_objects o JOIN staged_controls c ON c.family_id=o.family_id
               JOIN families f ON f.family_id=o.family_id AND f.active=1;
             DELETE FROM staged_objects WHERE family_id IN
               (SELECT family_id FROM families WHERE active=1);
             DELETE FROM staged_issues;
             DELETE FROM staged_challenges;
             DELETE FROM staged_admissions;
             DELETE FROM staged_removals;
             INSERT OR IGNORE INTO object_reservations(family_id,object_id,kind,object_hash)
               SELECT family_id,object_id,kind,object_hash FROM committed_objects;
             INSERT OR IGNORE INTO object_reservations(family_id,object_id,kind,object_hash)
               SELECT family_id,object_id,kind,object_hash FROM staged_objects;
             INSERT OR IGNORE INTO object_reservations(family_id,object_id,kind,object_hash)
               SELECT family_id,object_id,kind,object_hash FROM staged_control_objects;",
        )?;
        let inconsistent: i64 = migration.query_row(
            "SELECT EXISTS(
               SELECT 1 FROM committed_objects o JOIN object_reservations r USING(family_id,object_id)
                 WHERE o.kind!=r.kind OR o.object_hash!=r.object_hash
               UNION ALL
               SELECT 1 FROM staged_objects o JOIN object_reservations r USING(family_id,object_id)
                 WHERE o.kind!=r.kind OR o.object_hash!=r.object_hash
               UNION ALL
               SELECT 1 FROM staged_control_objects o JOIN object_reservations r USING(family_id,object_id)
                 WHERE o.kind!=r.kind OR o.object_hash!=r.object_hash
             )",
            [],
            |r| r.get(0),
        )?;
        if inconsistent != 0 {
            return Err(Error::Invalid("object reservation conflicts on restart"));
        }
        migration.commit()?;
        let relay_public = crypto::signing_public_key(&relay_seed);
        db.execute(
            "INSERT OR IGNORE INTO relay_identity(singleton, public_key) VALUES(1, ?1)",
            params![&relay_public[..]],
        )?;
        let stored: Vec<u8> = db.query_row(
            "SELECT public_key FROM relay_identity WHERE singleton=1",
            [],
            |r| r.get(0),
        )?;
        if stored != relay_public {
            return Err(Error::Invalid("relay signing identity changed on restart"));
        }
        let store = Self {
            db,
            relay_seed,
            relay_public,
        };
        store.verify_saved_families()?;
        initialize_private_integrity(
            &store.db,
            &store.relay_seed,
            store.relay_public,
            allow_legacy_checkpoint,
        )?;
        store.verify_uncommitted_reservations()?;
        Ok(store)
    }

    pub fn relay_public_key(&self) -> [u8; 32] {
        self.relay_public
    }
}
