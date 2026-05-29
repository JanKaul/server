//! Interface stub for `ha_rocksdb_cc__ha_rocksdb__write_path`.
//!
//! C++ source: `storage/rocksdb/ha_rocksdb.cc` (span 8995..13865, body ~351 LoC)
//! v4 manifest sub-unit: `ha_rocksdb_cc__ha_rocksdb__write_path`
//!
//! ## Mapping
//! INSERT/UPDATE/DELETE primitives for `ha_rocksdb`. Maps directly to
//! **`DbTransaction::put` / `delete` / `merge`** (_DESIGN.md §1 row
//! "Write batch" + §6).
//!
//! Two key collapses:
//!
//! 1. **`can_use_single_delete`** is preserved as-is — but its meaning
//!    changes: in MyRocks a "SingleDelete" promises RocksDB the key was
//!    PUT exactly once, enabling a deletion-marker optimization. SlateDB
//!    has no `SingleDelete` primitive in its public `ops.rs`; all deletes
//!    are `DbWriteOps::delete`. The flag is retained so the engine can
//!    enable that optimization later if SlateDB grows the feature, but for
//!    Stage 0 it routes through plain `delete`.
//!
//! 2. **`update_write_pk` / `update_write_sk`** dispatch decisions that
//!    in MyRocks chose between `Put`, `SingleDelete`, and `bulk_load_key`
//!    now all funnel through `DbTransaction::put` / `delete` (with the
//!    bulk_load_helpers bucket handling the bulk path via `WriteBatch`).
//!
//! 3. **Read-Free Replication** (`use_read_free_rpl`) is gated upstream:
//!    `is_blind_delete_enabled` checks for the THDVAR, but the actual
//!    RFR slave path is non-goal per _DESIGN.md §1 row "Read-Free Replication".
//!
//! ## Out-of-scope methods
//! None inside this bucket — but `delete_or_singledelete`'s
//! single-delete branch is degraded (see note 1 above). All ten methods are
//! preserved.

use slatedb::Error;
use bytes::Bytes;

use crate::rdb_global_h::GlIndexId;

use crate::ha_rocksdb_h__ha_rocksdb::HaSlateDb;
use crate::ha_rocksdb_h__update_row_info::UpdateRowInfo;

impl HaSlateDb {
    /// True iff the session has enabled `blind_delete_primary_key` AND the
    /// current statement is a single-table DELETE on a non-replicated table
    /// with no hidden PK and exactly one key. Pure THDVAR + statement-shape
    /// check; no I/O.
    ///
    /// Original C++: ha_rocksdb.cc:8995.
    pub fn is_blind_delete_enabled(&self) -> bool {
        todo!("port THDVAR/statement-shape conditions from ha_rocksdb.cc:8995")
    }

    /// Release any lock held on the last-read row. Maps to
    /// `txn.unmark_write(&[m_last_rowkey])` for SSI mode (so the row is
    /// excluded from conflict checks on commit) and is a no-op under
    /// `IsolationLevel::Snapshot`.
    ///
    /// Original C++: ha_rocksdb.cc:9407.
    pub fn unlock_row(&mut self) -> Result<(), Error> {
        todo!("if SSI: txn.unmark_write(&[m_last_rowkey.clone()]); else: no-op")
    }

    /// True iff a `SingleDelete` would be safe for index position `index`:
    ///   - Secondary indexes: always true (they're never re-put under the
    ///     same key without a prior delete by the engine itself).
    ///   - Primary key: true iff every column is part of the PK (no value
    ///     payload to "merge with" on a duplicate insert).
    ///
    /// As noted in the file header, SlateDB has no SingleDelete primitive;
    /// the flag is retained but currently has no behavioral effect.
    ///
    /// Original C++: ha_rocksdb.cc:9425.
    pub fn can_use_single_delete(&self, index: u32) -> bool {
        let _ = index;
        todo!("index != pk_index() || (!has_hidden_pk() && key_info[index].ext_key_parts == table.fields)")
    }

    /// True iff the engine should skip uniqueness checks on this write.
    /// Honors:
    ///   - `bulk_load` THDVAR
    ///   - per-table skip-unique-check whitelist
    ///   - `OPTION_RELAXED_UNIQUE_CHECKS` + single-index table
    ///   (Read-Free Replication path is non-goal per _DESIGN.md.)
    ///
    /// Pure config check; no I/O.
    ///
    /// Original C++: ha_rocksdb.cc:9431.
    pub fn skip_unique_check(&self) -> bool {
        todo!("port the 4-arm OR from ha_rocksdb.cc:9431 (omit use_read_free_rpl arm)")
    }

    /// Write (or delete-then-write) the PK entry for a row update.
    ///
    /// Three branches inside:
    ///   1. **Bulk-load**: hand off to `bulk_load_key` (handled by the
    ///      bulk_load_helpers bucket via a `WriteBatch`).
    ///   2. **Skip-unique-check or DDL txn**: `WriteBatch::put` (no
    ///      conflict check at commit because we're not in SSI mode for
    ///      these paths).
    ///   3. **Normal**: `DbTransaction::put` — SlateDB will conflict-check
    ///      at commit under SSI.
    ///
    /// Returns `Err(slatedb::Error::invalid(...))` for the "duplicate PK"
    /// case so the caller (`update_write_row`) can re-map to MyRocks'
    /// `HA_ERR_FOUND_DUPP_KEY`.
    ///
    /// Original C++: ha_rocksdb.cc:10121.
    pub async fn update_write_pk(
        &mut self,
        row_info: &UpdateRowInfo,
        pk_changed: bool,
    ) -> Result<(), Error> {
        let _ = (row_info, pk_changed);
        todo!("pick branch by bulk_load / skip_unique_check / normal; map duplicate to invalid()")
    }

    /// Write (or delete-then-write) a secondary-key entry. Same three
    /// branches as `update_write_pk`. Fast-paths the "key bytes didn't
    /// change AND TTL bytes didn't change" case by returning early.
    ///
    /// `key_id` is the position in `m_key_descr_arr`; the engine resolves
    /// the `Rdb_key_def` internally — no `KEY*` exposed here.
    ///
    /// Original C++: ha_rocksdb.cc:10210.
    pub async fn update_write_sk(
        &mut self,
        row_info: &UpdateRowInfo,
        key_id: u32,
        bulk_load_sk: bool,
    ) -> Result<(), Error> {
        let _ = (row_info, key_id, bulk_load_sk);
        todo!("compare old vs new packed keys; SingleDelete old then Put new (or bulk_load_key)")
    }

    /// Loop over all indexes calling `update_write_pk` for the PK and
    /// `update_write_sk` for each SK. Allocated as its own method because
    /// it's also the entry point for the inplace-alter populator.
    ///
    /// Original C++: ha_rocksdb.cc:10315.
    pub async fn update_write_indexes(
        &mut self,
        row_info: &UpdateRowInfo,
        pk_changed: bool,
    ) -> Result<(), Error> {
        let _ = (row_info, pk_changed);
        todo!("PK first (so TTL bytes are computed); then for each SK call update_write_sk")
    }

    /// Top-level INSERT / UPDATE driver. Called by `write_row` (new INSERT)
    /// and `update_row` (UPDATE). Steps:
    ///   1. `get_pk_for_update` (assemble new PK bytes).
    ///   2. If !skip_unique_check: `check_uniqueness_and_lock` (repair bucket).
    ///   3. `update_write_indexes`.
    ///   4. Increment per-txn insert/update counter.
    ///   5. `do_bulk_commit` (flush batch if past threshold).
    ///
    /// `old_data = None` ⇒ INSERT; `old_data = Some(_)` ⇒ UPDATE.
    ///
    /// Errors map per _DESIGN.md §4.
    ///
    /// Original C++: ha_rocksdb.cc:10356.
    pub async fn update_write_row(
        &mut self,
        old_data: Option<&Bytes>,
        new_data: &Bytes,
        skip_unique_check: bool,
    ) -> Result<(), Error> {
        let _ = (old_data, new_data, skip_unique_check);
        todo!("orchestrate the 5 steps above; honor thd.killed; map duplicate-PK to invalid()")
    }

    /// Wrapper for delete-row that picks between `DbTransaction::delete`
    /// (regular tombstone) and the would-be single-delete (currently the
    /// same call — see header note). `assume_tracked` comes from
    /// `can_assume_tracked` (table_mgmt bucket).
    ///
    /// Original C++: ha_rocksdb.cc:10926.
    pub async fn delete_or_singledelete(
        &mut self,
        index: u32,
        key: &Bytes,
    ) -> Result<(), Error> {
        let _ = (index, key);
        todo!("for now: txn.delete(key) regardless of can_use_single_delete (SlateDB has no SD)")
    }

    /// True iff the caller can pass `assume_tracked=true` to SlateDB's
    /// txn put/delete (i.e., the row has already been locked via
    /// `mark_read`/`get_for_update`). Returns false for blind-delete +
    /// read-free-replication code paths.
    ///
    /// In SlateDB our "assume_tracked" equivalent is **whether to call
    /// `txn.mark_read` before the write**. The flag's name is preserved
    /// for MyRocks call-site fidelity; the semantics rotate slightly.
    ///
    /// Original C++: ha_rocksdb.cc:13860.
    pub fn can_assume_tracked(&self) -> bool {
        todo!("port: !THDVAR(blind_delete_primary_key) (use_read_free_rpl is non-goal)")
    }
}
