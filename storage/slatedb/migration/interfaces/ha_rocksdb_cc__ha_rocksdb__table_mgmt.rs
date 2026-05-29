//! Interface stub for `ha_rocksdb_cc__ha_rocksdb__table_mgmt`.
//!
//! C++ source: `storage/rocksdb/ha_rocksdb.cc` (442 LoC body, 14 methods)
//! v4 manifest sub-unit: `ha_rocksdb_cc__ha_rocksdb__table_mgmt`
//! parent: `ha_rocksdb_cc`
//!
//! ## Mapping
//! Table-level maintenance + stats: TRUNCATE, remove-rows by table-id,
//! update_create_info, update_stats, calculate_stats_for_table, get_range,
//! idx_cond_push, inplace_populate_sk, table_version, can_use_bloom_filter,
//! index_blocks, read_thd_vars, calc_updated_indexes,
//! should_recreate_snapshot.
//!
//! Per _DESIGN.md §1:
//! - TRUNCATE → range-delete of all keys with the table's CF/index prefix.
//!   SlateDB doesn't expose range-delete primitive; we issue a scan +
//!   per-key tombstone batch (or a CompactionFilter "drop the whole
//!   table" entry, depending on volume).
//! - Stats → derived from SlateDB metrics + `DbStatus` subscription
//!   (see `ha_rocksdb_cc__Rdb_snapshot_status.rs`).
//! - `idx_cond_push` → index-condition push-down: kept enabled, evaluated
//!   in the codec layer (see `rdb_datadic_cc__Rdb_key_def__decode.rs`).
//! - `can_use_bloom_filter` → consults the `PrefixExtractor` we registered.
//!
//! ## Out-of-scope methods
//! None.

use bytes::Bytes;
use slatedb::Error;

use crate::ha_rocksdb_h__ha_rocksdb::{HaSlateDb, KeyShareView};

impl HaSlateDb {
    /// MariaDB's `TRUNCATE TABLE`. Drops all rows for this table without
    /// touching the schema. Issued either as a per-row tombstone batch
    /// (small tables) or by registering a drop-table marker in the system
    /// CF that the CompactionFilter sweeps.
    /// Original: ha_rocksdb.h:894 — `truncate`.
    pub async fn truncate(&mut self) -> Result<(), Error> {
        todo!("decide path (tombstone batch vs filter marker) by table size")
    }

    /// Internal: same as truncate but takes the tbl_def explicitly. Used by
    /// `delete_table` to share rm-rows logic.
    /// Original: ha_rocksdb.h:907 — `remove_rows`.
    pub async fn remove_rows(&mut self) -> Result<(), Error> {
        todo!("scan + tombstone batch for tbl_def's prefix")
    }

    /// Refreshes `create_info` with engine-internal values (e.g., the
    /// current auto-increment value) so SHOW CREATE TABLE renders correctly.
    /// Original: ha_rocksdb.h:951 — `update_create_info`.
    pub fn update_create_info(&self, create_info: &mut CreateInfoView) {
        todo!("populate create_info.auto_increment_value from our dict")
    }

    /// Asynchronously kicks off a per-table stats refresh. The actual stats
    /// computation runs on a background Tokio task fed by `DbStatus`
    /// subscription; this call queues the refresh and returns.
    /// Original: per-handler `update_stats` helper.
    pub fn update_stats(&self) -> Result<(), Error> {
        todo!("send a refresh request to the Rdb_index_collector task")
    }

    /// Compute statistics for this table by walking SST props from
    /// `Db::manifest()`. Synchronous, used by ANALYZE TABLE.
    /// Original: ha_rocksdb.h:851 — `calculate_stats_for_table`.
    pub async fn calculate_stats_for_table(&self) -> Result<(), Error> {
        todo!("walk manifest SST list; aggregate per-CF stats; store in tbl_def's cache")
    }

    /// Returns the (lower, upper) byte range that covers all keys belonging
    /// to a given `index_id` within our CF. Used to size scan bounds.
    /// Original: free fn `get_range` (3 overloads) in ha_rocksdb.cc.
    pub fn get_range(&self, index_id: u32) -> (Bytes, Bytes) {
        todo!("build (cf_prefix||index_id_be, cf_prefix||(index_id+1)_be) bound pair")
    }

    /// Pushed-down index condition. Stored on the handler for evaluation
    /// during the next index scan.
    /// Original: ha_rocksdb.h:663 — `idx_cond_push`.
    ///
    /// Returns `None` if the condition was fully consumed (no remainder
    /// returned to the SQL layer); else `Some(remainder)`.
    pub fn idx_cond_push(&mut self, keyno: u32, _idx_cond: ()) -> Option<()> {
        todo!("store idx_cond on the handler; evaluation happens in the index_next path")
    }

    /// Backfill SKs for the given added-index set. Called from
    /// `inplace_alter_table`. Streams rows through the per-statement
    /// WriteBatch.
    /// Original: ha_rocksdb.h:843 — `inplace_populate_sk`.
    pub async fn inplace_populate_sk(
        &mut self,
        added_indexes: &[&KeyShareView],
    ) -> Result<(), Error> {
        todo!("PK scan via DbSnapshot; for each row, encode SK for each added index; WriteBatch flush")
    }

    /// Returns the current table-version generation (incremented on each
    /// DDL). Used by query plan caches.
    /// Original: ha_rocksdb.h:975 — `table_version`.
    pub fn table_version(&self) -> u64 {
        todo!("read from the tbl_def's atomic version counter")
    }

    /// Compute the bitmap of indexes that the current UPDATE will modify.
    /// Original: helper in ha_rocksdb.cc — `calc_updated_indexes`.
    pub fn calc_updated_indexes(
        &self,
        old_row: &[u8],
        new_row: &[u8],
    ) -> Vec<u32> {
        todo!("compare old/new field bytes per indexed column; emit index_ids whose key bytes changed")
    }

    /// Bool: should this scan consult the SST bloom filter? Consults our
    /// `PrefixExtractor` to confirm the lookup prefix matches the SST's
    /// expected prefix length.
    /// Original: ha_rocksdb.cc — `can_use_bloom_filter`.
    pub fn can_use_bloom_filter(&self, key_prefix_len: u32) -> bool {
        todo!("consult cf's PrefixExtractor: prefix_len must equal extractor's target length")
    }

    /// Per-index block-count estimate for cost model.
    /// Original: ha_rocksdb.h:633 — `index_blocks`.
    pub fn index_blocks(&self, index: u32, ranges: u32, rows: u64) -> u64 {
        todo!("derive from per-index SST count + avg block size; see manifest stats")
    }

    /// Reads THD session variables that affect this handler's behavior
    /// (bulk-load mode flag, isolation level override, etc.) and caches
    /// them on `self`. Called at statement start.
    /// Original: ha_rocksdb.h:838 — `read_thd_vars`.
    pub fn read_thd_vars(&mut self, thd_session_vars: &ThdSessionVars) {
        todo!("copy bulk_load / isolation_level / skip_unique_check / etc. into self")
    }

    /// Whether we need to drop+recreate the snapshot mid-scan after a
    /// retry. Returns true on TRY_AGAIN-like SlateDB errors.
    /// Original: ha_rocksdb.h:854 — `should_recreate_snapshot`.
    pub fn should_recreate_snapshot(&self, rc: i32, is_new_snapshot: bool) -> bool {
        todo!("rc == HA_ERR_LOCK_DEADLOCK && !is_new_snapshot")
    }
}

/// Read-only view of MariaDB's `HA_CREATE_INFO`. Subset the engine consumes.
#[derive(Debug)]
pub struct CreateInfoView {
    pub auto_increment_value: u64,
    pub options: u64,
}

/// Cached subset of THD session variables that affect handler behavior.
#[derive(Debug, Clone)]
pub struct ThdSessionVars {
    pub bulk_load: bool,
    pub bulk_load_allow_unsorted: bool,
    pub skip_unique_check: bool,
    pub isolation_level: u32,
    pub deadlock_detect: bool,
}
