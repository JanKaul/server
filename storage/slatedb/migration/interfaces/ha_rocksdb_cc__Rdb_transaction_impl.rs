//! Interface stub for `ha_rocksdb_cc__Rdb_transaction_impl`.
//!
//! C++ source: `storage/rocksdb/ha_rocksdb.cc` (lines 3141..3521, ~380 LoC)
//! v4 manifest sub-unit: `ha_rocksdb_cc__Rdb_transaction_impl`
//!
//! ## Mapping
//! Per _DESIGN.md §5 (transaction model): the "real" `Rdb_transaction`
//! subclass — backed by **SlateDB's `DbTransaction`**. This is the default
//! path for user SQL transactions (the `Rdb_writebatch_impl` subclass is only
//! used by replication threads).
//!
//! Field-for-field:
//! - C++ `m_rocksdb_tx: rocksdb::Transaction*` → `inner: Option<slatedb::DbTransaction>`
//! - C++ `m_rocksdb_reuse_tx` (reuse pool) → DROPPED. SlateDB has no txn-object
//!   reuse pool; `Db::begin()` is cheap (constructs a `DbTransaction` value).
//! - C++ `m_notifier` (rocksdb::TransactionNotifier) → DROPPED. SlateDB's
//!   snapshot is acquired synchronously in `acquire_snapshot`; no async
//!   notification needed. See `Rdb_snapshot_notifier` stub for the explanation.
//! - C++ `set_snapshot_on_next_operation` / `m_is_delayed_snapshot` → preserved
//!   as a Rust enum `SnapshotState::{None, Pending, Held(Arc<DbSnapshot>)}`.
//!
//! ## Out-of-scope methods
//! - `release_tx` (txn object reuse) — N/A on SlateDB.
//! - `get_rdb_trx` (returns raw `rocksdb::Transaction*`) — leaks impl; we
//!   expose narrower accessors instead.
//! - Explicit-snapshot integration (`m_explicit_snapshot`) — MARIAROCKS_NOT_YET.

use bytes::Bytes;
use slatedb::Error;
use std::sync::Arc;

use crate::ha_rocksdb_cc__Rdb_transaction::{
    RdbKeyDef, RdbTableHandler, RdbTblDef, RdbTransaction,
};
use crate::rdb_global_h::GlIndexId;

/// Per-transaction snapshot state. Replaces MyRocks' `m_is_delayed_snapshot`
/// boolean + raw `m_read_opts.snapshot` pointer with an explicit ADT.
pub enum SnapshotState {
    /// No snapshot taken yet (read-uncommitted-ish, but rare).
    None,
    /// Will be acquired on first read (MyRocks' `SetSnapshotOnNextOperation`
    /// pattern; in SlateDB we model this as "lazy `Db::snapshot()` call").
    Pending,
    /// Acquired. Held as `Arc<DbSnapshot>` so reads can be served from it.
    Held(Arc<slatedb::DbSnapshot>),
}

/// Concrete transaction wrapping `slatedb::DbTransaction`.
pub struct RdbTransactionImpl {
    /// The live SlateDB transaction. `None` between commit/rollback and the
    /// next `start_tx`.
    pub inner: Option<slatedb::DbTransaction>,

    /// Per _DESIGN.md §5: SI vs. SSI.
    pub isolation: slatedb::IsolationLevel,

    pub snapshot: SnapshotState,

    // counters (mirror C++ Rdb_transaction protected fields)
    pub write_count: u64,
    pub insert_count: u64,
    pub update_count: u64,
    pub delete_count: u64,
    pub lock_count: u64,

    /// Cached `@@slatedb_lock_wait_timeout` (was `rocksdb_lock_wait_timeout`).
    pub timeout_sec: i32,

    /// Cached `@@slatedb_max_row_locks` — we honor this engine-side because
    /// SlateDB itself has no per-txn lock budget.
    pub max_row_locks: u64,

    pub rollback_only: bool,
    pub is_two_phase: bool,
    pub tx_read_only: bool,

    /// Stack of savepoint markers `(write_count_at_set, mark_read_snapshot_id)`.
    /// Per _DESIGN.md §5: engine-side savepoints — SlateDB has no native API.
    pub savepoint_stack: Vec<SavepointMarker>,

    /// Auto-increment merge buffer; drained at commit via SlateDB's merge op.
    pub auto_incr_map: std::collections::HashMap<GlIndexId, u64>,
}

/// One savepoint frame. We snapshot the txn's write count and the SSI
/// mark-read set size so rollback can prune both.
pub struct SavepointMarker {
    pub writes_at_set: u64,
    /// Snapshot of `DbTransaction`'s internal read-set length. Used to truncate
    /// the read-set on `rollback_to_stmt_savepoint`.
    pub read_set_len_at_set: usize,
}

impl RdbTransactionImpl {
    /// Construct an uninitialized txn; `start_tx` must be called before use.
    /// Original: ha_rocksdb.cc:3503 — `Rdb_transaction_impl::Rdb_transaction_impl`.
    pub fn new(timeout_sec: i32, max_row_locks: u64, isolation: slatedb::IsolationLevel) -> Self {
        Self {
            inner: None,
            isolation,
            snapshot: SnapshotState::None,
            write_count: 0,
            insert_count: 0,
            update_count: 0,
            delete_count: 0,
            lock_count: 0,
            timeout_sec,
            max_row_locks,
            rollback_only: false,
            is_two_phase: false,
            tx_read_only: false,
            savepoint_stack: Vec::new(),
            auto_incr_map: std::collections::HashMap::new(),
        }
    }

    /// Drain `auto_incr_map` into the txn via SlateDB merge ops. The merge
    /// operator we register (see `engine::merge`) routes `autoinc:*` keys to
    /// a max-merge.
    /// Original: ha_rocksdb.cc:2367 — `merge_auto_incr_map`.
    pub fn merge_auto_incr_map(&mut self) -> Result<(), Error> {
        todo!("for each (gl_index_id, max_val) in auto_incr_map: build key, txn.merge(key, max_val_be)")
    }
}

impl RdbTransaction for RdbTransactionImpl {
    fn write_count(&self) -> u64 { self.write_count }
    fn insert_count(&self) -> u64 { self.insert_count }
    fn update_count(&self) -> u64 { self.update_count }
    fn delete_count(&self) -> u64 { self.delete_count }
    fn lock_count(&self) -> u64 { self.lock_count }
    fn timeout_sec(&self) -> i32 { self.timeout_sec }
    fn num_ongoing_bulk_load(&self) -> i32 { 0 /* TODO(human): bulk-load aggregation */ }
    fn is_tx_read_only(&self) -> bool { self.tx_read_only }
    fn is_two_phase(&self) -> bool { self.is_two_phase }
    fn is_writebatch_trx(&self) -> bool { false }
    fn is_tx_started(&self) -> bool { self.inner.is_some() }
    fn has_snapshot(&self) -> bool { matches!(self.snapshot, SnapshotState::Held(_)) }
    fn has_modifications(&self) -> bool { self.write_count > 0 }

    fn incr_insert_count(&mut self) { self.insert_count += 1; }
    fn incr_update_count(&mut self) { self.update_count += 1; }
    fn incr_delete_count(&mut self) { self.delete_count += 1; }

    fn put(&mut self, cf_id: u32, key: Bytes, value: Bytes, _assume_tracked: bool) -> Result<(), Error> {
        // Pre-check budget; mirrors ha_rocksdb.cc:3319 (kLockLimit).
        self.write_count += 1;
        self.lock_count += 1;
        if self.write_count > self.max_row_locks || self.lock_count > self.max_row_locks {
            return Err(Error::invalid("row lock limit exceeded".into()));
        }
        todo!("prefix key with varint(cf_id) per _DESIGN.md §2, then self.inner.as_mut().unwrap().put(prefixed_key, value)")
    }

    fn delete_key(&mut self, cf_id: u32, key: Bytes, _assume_tracked: bool) -> Result<(), Error> {
        self.write_count += 1;
        self.lock_count += 1;
        if self.write_count > self.max_row_locks || self.lock_count > self.max_row_locks {
            return Err(Error::invalid("row lock limit exceeded".into()));
        }
        todo!("prefix key with varint(cf_id), then self.inner.as_mut().unwrap().delete(prefixed_key)")
    }

    fn single_delete(&mut self, cf_id: u32, key: Bytes, assume_tracked: bool) -> Result<(), Error> {
        // SlateDB has no "single delete" optimization (RocksDB compaction-time
        // detail). Translate to a plain delete; correctness is preserved.
        self.delete_key(cf_id, key, assume_tracked)
    }

    fn get(&self, _cf_id: u32, _key: &[u8]) -> Result<Option<Bytes>, Error> {
        todo!("prefix key, self.inner.as_ref().unwrap().get(prefixed_key).await")
    }

    fn get_for_update(
        &mut self,
        _cf_id: u32,
        _key: &[u8],
        _exclusive: bool,
        _do_validate: bool,
    ) -> Result<Option<Bytes>, Error> {
        self.lock_count += 1;
        if self.lock_count > self.max_row_locks {
            return Err(Error::invalid("row lock limit exceeded".into()));
        }
        todo!("for SerializableSnapshot mode, mark_read([key]) on the DbTransaction; then get()")
    }

    fn acquire_snapshot(&mut self, _acquire_now: bool) {
        todo!("if Pending and acquire_now: take Db::snapshot(); else stay Pending until first read")
    }

    fn release_snapshot(&mut self) {
        self.snapshot = SnapshotState::None;
    }

    fn set_initial_savepoint(&mut self) {
        self.savepoint_stack.push(SavepointMarker { writes_at_set: self.write_count, read_set_len_at_set: 0 });
    }

    fn make_stmt_savepoint_permanent(&mut self) -> Result<(), Error> {
        // Pop the stmt-level marker; the writes since become part of the txn.
        // Mirrors the loop at ha_rocksdb.cc:3051 that pops up to one save point.
        let _ = self.savepoint_stack.pop();
        self.set_initial_savepoint();
        Ok(())
    }

    fn rollback_to_stmt_savepoint(&mut self) {
        todo!("pop top marker; truncate the txn's buffered writes back to writes_at_set")
    }

    fn rollback_to_savepoint(&mut self) -> Result<(), Error> {
        // MyRocks always fails this if there are modifications (ha_rocksdb.cc:3096).
        if self.has_modifications() {
            self.rollback_only = true;
            return Err(Error::invalid("ROLLBACK TO SAVEPOINT not supported with modifications".into()));
        }
        Ok(())
    }

    fn start_tx(&mut self) {
        todo!("self.inner = Some(engine.db().begin(self.isolation).await); self.set_initial_savepoint();")
    }

    fn start_stmt(&mut self) {
        // Delayed-snapshot acquire — set Pending so the next read takes it.
        if matches!(self.snapshot, SnapshotState::None) {
            self.snapshot = SnapshotState::Pending;
        }
    }

    fn commit(&mut self) -> Result<bool, Error> {
        if self.write_count == 0 {
            self.rollback();
            return Ok(false);
        }
        if self.rollback_only {
            self.rollback();
            return Err(Error::invalid("transaction is rollback-only".into()));
        }
        todo!("self.merge_auto_incr_map()?; release_snapshot(); self.inner.take().unwrap().commit().await; reset counters")
    }

    fn rollback(&mut self) {
        self.write_count = 0;
        self.insert_count = 0;
        self.update_count = 0;
        self.delete_count = 0;
        self.lock_count = 0;
        self.auto_incr_map.clear();
        self.savepoint_stack.clear();
        if let Some(txn) = self.inner.take() {
            // DbTransaction::rollback is sync.
            txn.rollback();
        }
        self.release_snapshot();
        self.tx_read_only = false;
        self.rollback_only = false;
    }

    fn rollback_stmt(&mut self) {
        self.rollback_to_stmt_savepoint();
    }

    fn prepare(&mut self, xid_name: &[u8]) -> Result<(), Error> {
        // Per _DESIGN.md §1 (2PC re-impl thin layer):
        //   1. merge_auto_incr_map into the txn
        //   2. write a prepare marker `xa_prepare:<xid>` into the system CF
        //   3. flush_with_options(FlushType::Wal)  -- durability barrier
        // We do NOT call DbTransaction::commit() yet; commit happens at the
        // second phase via the rocksdb_commit_by_xid handlerton callback.
        let _ = xid_name;
        todo!("see _DESIGN.md §1 row 'Two-phase commit' — three-step prepare")
    }

    fn can_prepare(&self) -> bool {
        !self.rollback_only
    }

    fn set_tx_failed(&mut self, failed: bool) {
        self.rollback_only = failed;
    }

    fn log_table_write_op(&mut self, _tbl: &RdbTblDef) {
        // TODO(human): track Vec<&RdbTblDef> for on_commit to bump m_update_time.
    }

    fn set_auto_incr(&mut self, gl_index_id: GlIndexId, curr_id: u64) {
        let entry = self.auto_incr_map.entry(gl_index_id).or_insert(0);
        if curr_id > *entry { *entry = curr_id; }
    }

    fn rocksdb_tmpdir(&self) -> Option<&str> {
        None /* TODO(human): wire slatedb_tmpdir sysvar */
    }
}
