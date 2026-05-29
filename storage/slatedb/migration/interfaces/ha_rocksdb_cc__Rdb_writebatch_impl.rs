//! Interface stub for `ha_rocksdb_cc__Rdb_writebatch_impl`.
//!
//! C++ source: `storage/rocksdb/ha_rocksdb.cc` (lines 3531..3719, ~189 LoC)
//! v4 manifest sub-unit: `ha_rocksdb_cc__Rdb_writebatch_impl`
//!
//! ## Mapping
//! Per _DESIGN.md §1 row "Write batch" + §6: this is the lock-skipping,
//! conflict-detection-skipping transaction implementation used by the
//! replication thread (and any guaranteed-non-conflicting path).
//!
//! The C++ class wraps `rocksdb::WriteBatchWithIndex`. We map directly onto
//! **`slatedb::WriteBatch`** (per _DESIGN.md §0 "Write batch" — `batch.rs`
//! exposes `WriteBatch` as the atomic batch primitive committed via
//! `Db::write(batch)`).
//!
//! Key contract differences from `Rdb_transaction_impl`:
//! - `prepare(...)` is a **no-op** that returns `true` — there is no XA path
//!   for replication writes.
//! - `set_lock_timeout`, `release_lock` are no-ops — no locks held.
//! - `commit_no_binlog` calls `Db::write(batch)` with
//!   `WriteOptions { await_durable: <per session sysvar> }`.
//!
//! ## Out-of-scope methods
//! - `GetFromBatchAndDB` (read-your-own-writes from the batch) — SlateDB's
//!   `WriteBatch` is write-only; reads go through the Db directly. This is a
//!   **semantic change**: replication-path read-your-own-writes within a
//!   single statement is not preserved. Replication is single-threaded so
//!   the only way this matters is if the SAME statement reads-after-writes
//!   a key, which isn't a normal pattern.

use bytes::Bytes;
use slatedb::Error;

use crate::ha_rocksdb_cc__Rdb_transaction::{RdbTblDef, RdbTransaction};
use crate::rdb_global_h::GlIndexId;

/// Replication-path "transaction" — a bare SlateDB `WriteBatch` with no
/// conflict detection.
pub struct RdbWritebatchImpl {
    /// The buffered batch. `None` between commit/rollback and next `start_tx`.
    pub batch: Option<slatedb::WriteBatch>,

    /// Per-session durability: `WriteOptions::await_durable`.
    pub await_durable: bool,

    pub write_count: u64,
    pub insert_count: u64,
    pub update_count: u64,
    pub delete_count: u64,

    pub rollback_only: bool,
    pub tx_read_only: bool,

    /// Same savepoint stack as RdbTransactionImpl, only `writes_at_set` is
    /// meaningful (no read-set since we skip conflict detection).
    pub savepoint_stack: Vec<u64>,

    pub auto_incr_map: std::collections::HashMap<GlIndexId, u64>,
}

impl RdbWritebatchImpl {
    /// Build a fresh, empty replication-path txn.
    /// Original: ha_rocksdb.cc:3709 — `Rdb_writebatch_impl::Rdb_writebatch_impl`.
    pub fn new() -> Self {
        Self {
            batch: Some(slatedb::WriteBatch::new()),
            await_durable: true,
            write_count: 0,
            insert_count: 0,
            update_count: 0,
            delete_count: 0,
            rollback_only: false,
            tx_read_only: false,
            savepoint_stack: Vec::new(),
            auto_incr_map: std::collections::HashMap::new(),
        }
    }
}

impl Default for RdbWritebatchImpl {
    fn default() -> Self { Self::new() }
}

impl RdbTransaction for RdbWritebatchImpl {
    fn write_count(&self) -> u64 { self.write_count }
    fn insert_count(&self) -> u64 { self.insert_count }
    fn update_count(&self) -> u64 { self.update_count }
    fn delete_count(&self) -> u64 { self.delete_count }
    fn lock_count(&self) -> u64 { 0 }
    fn timeout_sec(&self) -> i32 { 0 }
    fn num_ongoing_bulk_load(&self) -> i32 { 0 }
    fn is_tx_read_only(&self) -> bool { self.tx_read_only }
    fn is_two_phase(&self) -> bool { false }
    fn is_writebatch_trx(&self) -> bool { true }
    fn is_tx_started(&self) -> bool { self.batch.is_some() }
    fn has_snapshot(&self) -> bool { false }
    fn has_modifications(&self) -> bool { self.write_count > 0 }

    fn incr_insert_count(&mut self) { self.insert_count += 1; }
    fn incr_update_count(&mut self) { self.update_count += 1; }
    fn incr_delete_count(&mut self) { self.delete_count += 1; }

    fn put(&mut self, _cf_id: u32, _key: Bytes, _value: Bytes, _assume_tracked: bool) -> Result<(), Error> {
        self.write_count += 1;
        todo!("prefix key with varint(cf_id), self.batch.as_mut().unwrap().put(prefixed_key, value)")
    }

    fn delete_key(&mut self, _cf_id: u32, _key: Bytes, _assume_tracked: bool) -> Result<(), Error> {
        self.write_count += 1;
        todo!("prefix key, self.batch.as_mut().unwrap().delete(prefixed_key)")
    }

    fn single_delete(&mut self, cf_id: u32, key: Bytes, assume_tracked: bool) -> Result<(), Error> {
        self.delete_key(cf_id, key, assume_tracked)
    }

    fn get(&self, _cf_id: u32, _key: &[u8]) -> Result<Option<Bytes>, Error> {
        // SlateDB WriteBatch has no "read from batch + DB" primitive.
        // Read straight from the Db; see file header.
        todo!("engine.db().get(prefixed_key).await — does NOT see this txn's pending writes")
    }

    fn get_for_update(
        &mut self,
        cf_id: u32,
        key: &[u8],
        _exclusive: bool,
        _do_validate: bool,
    ) -> Result<Option<Bytes>, Error> {
        // No locking on replication path; FOR UPDATE just reads.
        self.get(cf_id, key)
    }

    fn acquire_snapshot(&mut self, _acquire_now: bool) {
        // No snapshot tracking on the writebatch path — reads are off the
        // live Db; replication ordering already guarantees consistency.
    }
    fn release_snapshot(&mut self) {}

    fn set_initial_savepoint(&mut self) {
        self.savepoint_stack.push(self.write_count);
    }

    fn make_stmt_savepoint_permanent(&mut self) -> Result<(), Error> {
        let _ = self.savepoint_stack.pop();
        self.set_initial_savepoint();
        Ok(())
    }

    fn rollback_to_stmt_savepoint(&mut self) {
        todo!("slatedb::WriteBatch has no truncate-to-position; we'd need to rebuild from a buffered log of (op, key, value)")
    }

    fn rollback_to_savepoint(&mut self) -> Result<(), Error> {
        if self.has_modifications() {
            self.rollback_only = true;
            return Err(Error::invalid("ROLLBACK TO SAVEPOINT not supported with modifications".into()));
        }
        Ok(())
    }

    fn start_tx(&mut self) {
        self.batch = Some(slatedb::WriteBatch::new());
        self.write_count = 0;
        self.set_initial_savepoint();
    }

    fn start_stmt(&mut self) {}

    fn commit(&mut self) -> Result<bool, Error> {
        if self.write_count == 0 { self.rollback(); return Ok(false); }
        todo!("flush auto_incr_map as merge ops on batch; then engine.db().write_with_options(batch, WriteOptions { await_durable: self.await_durable }).await")
    }

    fn rollback(&mut self) {
        self.write_count = 0;
        self.insert_count = 0;
        self.update_count = 0;
        self.delete_count = 0;
        self.auto_incr_map.clear();
        self.savepoint_stack.clear();
        self.batch = Some(slatedb::WriteBatch::new());
        self.tx_read_only = false;
        self.rollback_only = false;
    }

    fn rollback_stmt(&mut self) {
        if self.batch.is_some() {
            self.rollback_to_stmt_savepoint();
        }
    }

    fn prepare(&mut self, _xid_name: &[u8]) -> Result<(), Error> {
        // Mirrors ha_rocksdb.cc:3542 — `prepare` returns true unconditionally
        // on the writebatch path. No XA support.
        Ok(())
    }

    fn can_prepare(&self) -> bool { !self.rollback_only }
    fn set_tx_failed(&mut self, failed: bool) { self.rollback_only = failed; }
    fn log_table_write_op(&mut self, _tbl: &RdbTblDef) { /* TODO(human) */ }

    fn set_auto_incr(&mut self, gl_index_id: GlIndexId, curr_id: u64) {
        let entry = self.auto_incr_map.entry(gl_index_id).or_insert(0);
        if curr_id > *entry { *entry = curr_id; }
    }

    fn rocksdb_tmpdir(&self) -> Option<&str> { None }
}
