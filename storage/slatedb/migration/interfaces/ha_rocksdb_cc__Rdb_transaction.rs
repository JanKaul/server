//! Interface stub for `ha_rocksdb_cc__Rdb_transaction`.
//!
//! C++ source: `storage/rocksdb/ha_rocksdb.cc` (lines 2300..3130, ~830 LoC)
//! v4 manifest sub-unit: `ha_rocksdb_cc__Rdb_transaction`
//!
//! ## Mapping
//! Per _DESIGN.md §5 (transaction model) and §1 row "Two-phase commit":
//! `Rdb_transaction` is the abstract base for an in-flight per-THD transaction.
//! It wraps **SlateDB's `DbTransaction`** (one per active SQL transaction) and
//! layers on:
//!   - statement-level savepoint stack (since SlateDB has no native savepoints — §5)
//!   - per-table modified-set tracking (for `m_update_time` bookkeeping)
//!   - auto-increment merge map (drained at commit/prepare)
//!   - bulk-load SST aggregation (per §1 "SST bulk loader" — degraded WriteBatch path)
//!   - 2PC prepare-marker plumbing (per §1 row "Two-phase commit")
//!
//! Two concrete subclasses inherit:
//!   - `Rdb_transaction_impl`     — full DbTransaction (default path)
//!   - `Rdb_writebatch_impl`      — bare WriteBatch (replication / non-conflicting)
//!
//! ## Out-of-scope methods
//! - `mysql_bin_log_commit_pos` / `binlog_manager` integration — MARIAROCKS_NOT_YET
//!   (already disabled in C++); we keep the structural hook but it's a no-op.
//! - `m_explicit_snapshot` — guarded by `MARIAROCKS_NOT_YET`; not ported.

use bytes::Bytes;
use slatedb::Error;

use crate::rdb_global_h::GlIndexId;

/// I/O perf-counter handle stub. Real type lives in `rdb_perf_context_h.rs`.
pub struct RdbIoPerf;

/// Forward decl for the data-dictionary table-def. Lives in
/// `rdb_datadic_h__Rdb_tbl_def.rs`.
pub struct RdbTblDef;

/// Forward decl for key-def metadata. Lives in `rdb_datadic_h__Rdb_key_def.rs`.
pub struct RdbKeyDef;

/// Forward decl for the per-table handler shared state.
pub struct RdbTableHandler;

/// Reasons we mark a transaction as failed (mirrors MyRocks bookkeeping).
#[derive(Debug, Clone, Copy)]
pub enum TxnFailReason {
    None,
    LockWaitTimeout,
    Deadlock,
    SnapshotConflict,
}

/// Visitor passed to [`RdbTransaction::walk_tx_list`]. Replaces the C++
/// `Rdb_tx_list_walker` interface at `ha_rocksdb.cc:2250`.
pub trait TxListWalker {
    fn process_tran(&mut self, tx: &dyn RdbTransaction);
}

/// Abstract transaction. Implementations are `Rdb_transaction_impl` (full
/// SlateDB `DbTransaction`) and `Rdb_writebatch_impl` (bare `WriteBatch`).
///
/// The trait is `Send + Sync` only at the registry boundary; the per-THD
/// transaction itself is single-threaded.
pub trait RdbTransaction: Send {
    // --- accessors / counters ---
    fn write_count(&self) -> u64;
    fn insert_count(&self) -> u64;
    fn update_count(&self) -> u64;
    fn delete_count(&self) -> u64;
    fn lock_count(&self) -> u64;
    fn timeout_sec(&self) -> i32;
    fn num_ongoing_bulk_load(&self) -> i32;
    fn is_tx_read_only(&self) -> bool;
    fn is_two_phase(&self) -> bool;
    fn is_writebatch_trx(&self) -> bool;
    fn is_tx_started(&self) -> bool;
    fn is_prepared(&self) -> bool { false }
    fn has_snapshot(&self) -> bool;
    fn has_modifications(&self) -> bool;

    /// Per-statement counter increments. `++m_insert_count` etc.
    fn incr_insert_count(&mut self);
    fn incr_update_count(&mut self);
    fn incr_delete_count(&mut self);

    // --- write side (synchronous, buffered into the SlateDB WriteBatch) ---

    /// Buffered put. SlateDB's `DbTransaction::put` is sync and never errors;
    /// our wrapper bumps counters and enforces `m_max_row_locks`.
    ///
    /// Original: ha_rocksdb.cc:2944 — `Rdb_transaction::put` (pure virtual).
    fn put(&mut self, cf_id: u32, key: Bytes, value: Bytes, assume_tracked: bool) -> Result<(), Error>;

    /// Buffered delete; same error semantics as `put`.
    /// Original: ha_rocksdb.cc:2948 — `Rdb_transaction::delete_key`.
    fn delete_key(&mut self, cf_id: u32, key: Bytes, assume_tracked: bool) -> Result<(), Error>;

    /// Single-delete (MyRocks SingleDelete maps to SlateDB `delete` — SlateDB
    /// has only one delete primitive; the "single delete" optimization is
    /// internal to RocksDB's compaction and has no SlateDB analogue).
    /// Original: ha_rocksdb.cc:2951 — `Rdb_transaction::single_delete`.
    fn single_delete(&mut self, cf_id: u32, key: Bytes, assume_tracked: bool) -> Result<(), Error>;

    // --- read side ---

    /// Point lookup using the transaction's read view (its `DbSnapshot`).
    /// Returns `Ok(None)` on not-found; `Err` only on
    /// `slatedb::ErrorKind::{Unavailable, Closed, Data}`.
    fn get(&self, cf_id: u32, key: &[u8]) -> Result<Option<Bytes>, Error>;

    /// `SELECT … FOR UPDATE`. Maps to SlateDB SSI: in `Snapshot` mode we
    /// just call `get` (no lock taken — MyRocks' pessimistic lock has no
    /// SlateDB analogue); in `SerializableSnapshot` mode we additionally
    /// `mark_read([key])` to enforce write-write conflict detection at commit.
    ///
    /// Original: ha_rocksdb.cc:2969 — `Rdb_transaction::get_for_update`.
    fn get_for_update(
        &mut self,
        cf_id: u32,
        key: &[u8],
        exclusive: bool,
        do_validate: bool,
    ) -> Result<Option<Bytes>, Error>;

    // --- snapshot lifecycle ---

    /// Acquire the read view. `acquire_now=false` corresponds to MyRocks'
    /// "delayed snapshot" — we model this by deferring `Db::snapshot()` until
    /// the first read.
    fn acquire_snapshot(&mut self, acquire_now: bool);
    fn release_snapshot(&mut self);

    // --- savepoint stack (engine-side; SlateDB has no native savepoints) ---

    /// Push a savepoint marker `(write_batch_position, mark_read_set_snapshot)`
    /// onto the Rust-side stack. Called at every statement start; cheap.
    /// Original: ha_rocksdb.cc:2356 — `do_set_savepoint` (pure virtual).
    fn set_initial_savepoint(&mut self);

    /// "Promote" the current statement's savepoint into the permanent
    /// transaction record (pops the stmt-level marker since the stmt succeeded).
    /// Original: ha_rocksdb.cc:3044 — `make_stmt_savepoint_permanent`.
    fn make_stmt_savepoint_permanent(&mut self) -> Result<(), Error>;

    /// Discard write-batch entries above the last savepoint position.
    /// Original: ha_rocksdb.cc:3068 — `rollback_to_stmt_savepoint`.
    fn rollback_to_stmt_savepoint(&mut self);

    /// `ROLLBACK TO SAVEPOINT name` from SQL. MariaDB's SAVEPOINT API is
    /// not fully wired in MyRocks (the C++ returns failure if there are
    /// modifications — see ha_rocksdb.cc:3095); we preserve that contract.
    fn rollback_to_savepoint(&mut self) -> Result<(), Error>;

    // --- commit / rollback / prepare ---

    /// Start a fresh `DbTransaction` on the SlateDB side. Picks
    /// `IsolationLevel::SerializableSnapshot` if SQL session is SERIALIZABLE,
    /// else `IsolationLevel::Snapshot` (per _DESIGN.md §5 table).
    fn start_tx(&mut self);

    /// Hook called at each statement start; in REPEATABLE READ this is where
    /// the delayed snapshot is acquired.
    fn start_stmt(&mut self);

    /// Commit if `m_write_count > 0` and not `m_rollback_only`. Drains the
    /// auto-increment map into the system CF, then `DbTransaction::commit()`
    /// (which writes the batch atomically and returns `Option<WriteHandle>`).
    /// Returns `Ok(true)` on success, `Ok(false)` if empty (rollback-as-noop),
    /// `Err(ErrorKind::Transaction)` on conflict.
    ///
    /// Original: ha_rocksdb.cc:2582 — `Rdb_transaction::commit`.
    fn commit(&mut self) -> Result<bool, Error>;

    /// Rollback the entire transaction. Calls `DbTransaction::rollback()` and
    /// clears all engine-side counters.
    /// Original: ha_rocksdb.cc:2614 — `rollback` (pure virtual).
    fn rollback(&mut self);

    /// Rollback only the current statement (pop one savepoint).
    /// Original: ha_rocksdb.cc:3083 — `rollback_stmt`.
    fn rollback_stmt(&mut self);

    /// XA prepare. Per _DESIGN.md §1 "Two-phase commit": flush WAL via
    /// `Db::flush_with_options(FlushType::Wal)` and write a prepare marker
    /// `xa_prepare:<xid>` into the system CF.
    ///
    /// `name` is `rdb_xid_to_string(XID)` — see `name_helpers` stub.
    /// Original: ha_rocksdb.cc:2569 — `prepare` (pure virtual).
    fn prepare(&mut self, xid_name: &[u8]) -> Result<(), Error>;

    /// `can_prepare` — false if `m_rollback_only` is set.
    /// Original: ha_rocksdb.cc:3087.
    fn can_prepare(&self) -> bool;

    /// Marks the transaction "rollback only" (set by error helpers).
    fn set_tx_failed(&mut self, failed: bool);

    // --- per-table modification tracking ---

    /// Note that `tbl` was modified by this txn; used to bump
    /// `Rdb_tbl_def::m_update_time` on commit.
    /// Original: ha_rocksdb.cc:3026 — `log_table_write_op`.
    fn log_table_write_op(&mut self, tbl: &RdbTblDef);

    // --- auto-increment merge map ---

    /// Remember the largest auto-increment value seen for `gl_index_id`.
    /// Drained at commit via SlateDB's merge operator (per _DESIGN.md §1
    /// "Merge operators" — registered once at `Db::builder.with_merge_operator`).
    /// Original: ha_rocksdb.cc:2930 — `set_auto_incr`.
    fn set_auto_incr(&mut self, gl_index_id: GlIndexId, curr_id: u64);

    // --- bulk-load aggregation ---
    // `start_bulk_load` / `finish_bulk_load` are kept on the trait but the
    // implementation in TRANSLATE will go via SlateDB's `WriteBatch` with a
    // large `flush_interval`, per _DESIGN.md §1 row "SST bulk loader" (degraded).

    /// Returns the bulk-load tmp-dir for SST building. We pipe this from the
    /// `slatedb_tmpdir` sysvar.
    fn rocksdb_tmpdir(&self) -> Option<&str>;
}

/// Walk all live transactions and invoke `walker.process_tran` on each.
/// Replaces the C++ `Rdb_transaction::walk_tx_list` static + `s_tx_list`
/// global. In Rust, the registry is a `Mutex<Vec<Weak<dyn RdbTransaction>>>`
/// keyed by THD id.
///
/// Original: ha_rocksdb.cc:2425 — `Rdb_transaction::walk_tx_list`.
pub fn walk_tx_list(walker: &mut dyn TxListWalker) {
    todo!("snapshot the global txn registry under a Mutex, call walker.process_tran on each")
}
