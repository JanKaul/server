//! Interface stub for `ha_rocksdb_cc__ha_rocksdb__txn`.
//!
//! C++ source: `storage/rocksdb/ha_rocksdb.cc` (12 LoC body, methods listed in v4 manifest)
//! v4 manifest sub-unit: `ha_rocksdb_cc__ha_rocksdb__txn`
//! parent: `ha_rocksdb_cc`
//!
//! ## Mapping
//! Per-statement transaction lifecycle hooks: `start_stmt`, `end_stmt`,
//! savepoint set/rollback/release, plus the ctor `ha_rocksdb()` (in this
//! bucket because MyRocks registered it in PSI inside the bucket).
//!
//! Per _DESIGN.md §5, these wrap `slatedb::DbTransaction`:
//! - `start_stmt` is a no-op if a Txn already exists for the connection;
//!   else `Db::begin(IsolationLevel::Snapshot)` (level from sysvar).
//! - `end_stmt` only commits at statement boundary for AUTOCOMMIT;
//!   otherwise the Txn stays alive until the user `COMMIT`s.
//! - Savepoints layered on top of the SlateDB Txn — Rust-side stack of
//!   `(write_batch_position, read_set_snapshot)` checkpoints (see
//!   `ha_rocksdb_cc__Rdb_transaction.rs`).
//!
//! ## Out-of-scope methods
//! None — all txn vtable methods are in-scope. PSI registration is no-op
//! per _DESIGN.md (PSI integration deferred).

use slatedb::Error;
use slatedb::IsolationLevel;

use crate::ha_rocksdb_h__ha_rocksdb::HaSlateDb;

/// Lock-type passed to `start_stmt` / `external_lock`. Map of MariaDB's
/// `thr_lock_type` enum to the subset we care about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StmtLockType {
    /// `TL_READ` / `TL_READ_NO_INSERT` — pure-read SELECT.
    Read,
    /// `TL_WRITE` / `TL_WRITE_DELAYED` — write DML.
    Write,
    /// `TL_WRITE_CONCURRENT_INSERT` — bulk insert.
    BulkWrite,
}

impl HaSlateDb {
    /// Called by MariaDB at the start of each statement. Ensures a
    /// `DbTransaction` exists for the connection (binding it to the current
    /// THD). For AUTOCOMMIT mode, opens a fresh single-statement Txn here.
    /// Original: ha_rocksdb.h:890.
    ///
    /// Errors: `slatedb::ErrorKind::Closed` if the engine is shutting down;
    /// `Invalid` if isolation-level sysvar is malformed.
    pub async fn start_stmt(
        &mut self,
        thread_id: u64,
        lock_type: StmtLockType,
    ) -> Result<(), Error> {
        todo!("ensure Txn exists for this thd; bind it to self")
    }

    /// Called by MariaDB at statement end (in any mode). For AUTOCOMMIT,
    /// `commit_stmt` will follow; for explicit transactions, this is a no-op
    /// (or releases per-statement resources only).
    /// Not in MariaDB handler.h directly; called from the engine's per-stmt
    /// cleanup path.
    pub async fn end_stmt(&mut self) -> Result<(), Error> {
        todo!("release per-stmt scratch; do not commit (commit_stmt does that)")
    }

    /// Set a savepoint. Records `(write_batch_position, read_set_snapshot)`
    /// onto the Txn's savepoint stack.
    /// Original handlerton callback: `hton->savepoint_set` (registered
    /// from `ha_rocksdb_cc____free__txn_handlers.rs`); per-handler entry
    /// `savepoint_set` from ha_rocksdb.cc.
    pub fn savepoint_set(&mut self, savepoint_name: &str) -> Result<(), Error> {
        todo!("push (writebatch_idx, mark_read_snapshot) onto Txn's savepoint stack")
    }

    /// Rollback to a named savepoint. Truncates the WriteBatch to the
    /// recorded position; restores the read-set snapshot.
    pub fn savepoint_rollback(&mut self, savepoint_name: &str) -> Result<(), Error> {
        todo!("truncate WriteBatch; restore read-set; pop savepoints above target")
    }

    /// Release a savepoint (drops it from the stack without rolling back).
    /// Subsequent ROLLBACK TO SAVEPOINT will fail with savepoint-not-found.
    pub fn savepoint_release(&mut self, savepoint_name: &str) -> Result<(), Error> {
        todo!("remove named savepoint from stack")
    }

    /// PSI thread-state registration. Per _DESIGN.md, PSI is out-of-scope v1.
    /// Stub no-op kept for vtable completeness.
    pub fn register_in_psi(&mut self) {
        // intentionally no-op (PSI integration deferred per _DESIGN.md §1)
    }

    /// PSI thread-state unregistration. Stub no-op.
    pub fn unregister_from_psi(&mut self) {
        // intentionally no-op
    }
}
