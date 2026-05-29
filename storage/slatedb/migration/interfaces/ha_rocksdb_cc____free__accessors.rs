//! Interface stub for `ha_rocksdb_cc____free__accessors`.
//!
//! C++ source: `storage/rocksdb/ha_rocksdb.cc` (278 LoC body, 18 free fns)
//! v4 manifest sub-unit: `ha_rocksdb_cc____free__accessors`
//! parent: `ha_rocksdb_cc`
//!
//! ## Mapping
//! Free-function accessors and small helpers that didn't fit the other
//! 10 free-fn buckets: `rdb_get_*`, `rdb_is_*`, `is_valid`, `get_range`,
//! `is_myrocks_index_empty`, `calculate_stats` (free-fn variant),
//! `can_hold_read_locks_on_select`, `rdb_corruption_marker_file_name`,
//! `rmdir_force`, plus various small one-offs.
//!
//! Per _DESIGN.md §1, these mostly map to thin accessors over our
//! engine state (no direct SlateDB API beyond reads from `Db::manifest()`
//! or `DbStatus`).
//!
//! ## Out-of-scope methods
//! None — all in-scope. PSI / RocksDB-specific status accessors return
//! sensible defaults rather than non-goal errors so that SHOW STATUS
//! callers keep working.

use bytes::Bytes;
use slatedb::Error;
use std::sync::Arc;

// Forward decls — concrete types live in their owning units.
pub trait CfManagerRef: Send + Sync {}
pub trait DictManagerRef: Send + Sync {}
pub trait DdlManagerRef: Send + Sync {}
pub trait BinlogManagerRef: Send + Sync {}

// --- handle accessors (return globally-shared singletons after handlerton init) ---

/// Returns the global SlateDB Db handle. Panics if called before
/// `rocksdb_init_func` has completed (programming error).
/// Original: ha_rocksdb.cc — `rdb_get_rocksdb_db`.
pub fn rdb_get_slatedb_db() -> Arc<slatedb::Db> {
    todo!("OnceLock<Arc<Db>>.get().expect(\"engine not initialized\")")
}

/// Global CF-id ↔ name manager.
/// Original: `rdb_get_cf_manager`.
pub fn rdb_get_cf_manager() -> Arc<dyn CfManagerRef> {
    todo!()
}

/// Global table-options accessor.
/// Original: `rdb_get_table_options`.
pub fn rdb_get_table_options() -> Arc<()> {
    todo!()
}

/// Global data-dictionary manager.
/// Original: `rdb_get_dict_manager`.
pub fn rdb_get_dict_manager() -> Arc<dyn DictManagerRef> {
    todo!()
}

/// Global DDL manager.
/// Original: `rdb_get_ddl_manager`.
pub fn rdb_get_ddl_manager() -> Arc<dyn DdlManagerRef> {
    todo!()
}

/// Global binlog manager.
/// Original: `rdb_get_binlog_manager`.
pub fn rdb_get_binlog_manager() -> Arc<dyn BinlogManagerRef> {
    todo!()
}

/// Whether TTL is enabled (sysvar-driven).
/// Original: `rdb_is_ttl_enabled`.
pub fn rdb_is_ttl_enabled() -> bool {
    todo!("read sysvar slatedb_enable_ttl")
}

/// Whether TTL filtering applies during reads.
/// Original: `rdb_is_ttl_read_filtering_enabled`.
pub fn rdb_is_ttl_read_filtering_enabled() -> bool {
    todo!()
}

// --- misc helpers ---

/// True if the given handler instance has a valid open table.
/// Original: `is_valid` (free).
pub fn is_valid(_handler: &()) -> bool {
    todo!("check handler.table_handler is set")
}

/// Compute (lower, upper) byte range for an index id. Free-fn variant —
/// also exposed as a method on HaSlateDb (see ha_rocksdb__table_mgmt).
/// Original: free `get_range`.
pub fn get_range_for_index(index_id: u32) -> (Bytes, Bytes) {
    todo!()
}

/// True if no rows currently exist for the given index id.
/// Original: `is_myrocks_index_empty`.
pub async fn is_slatedb_index_empty(index_id: u32) -> Result<bool, Error> {
    todo!("Db::scan_prefix(index_prefix).next().await.is_none()")
}

/// Per-table stats recomputation. Free-fn variant for the background task;
/// the per-handler method is on HaSlateDb.
/// Original: free `calculate_stats`.
pub async fn calculate_stats() -> Result<(), Error> {
    todo!()
}

/// THD permission: can we hold read locks across this SELECT?
/// Returns true for non-skip-locked, non-read-uncommitted modes.
/// Original: `can_hold_read_locks_on_select`.
pub fn can_hold_read_locks_on_select(isolation_level: u32, _select_lock_type: u32) -> bool {
    todo!("map isolation level to behavior")
}

/// Returns the absolute path of the corruption-marker file in the data dir.
/// Original: `rdb_corruption_marker_file_name`.
pub fn rdb_corruption_marker_file_name() -> String {
    todo!("join data_dir + 'SLATEDB_CORRUPTED'")
}

/// `rmdir -rf`: recursively delete a directory tree. Used when dropping a
/// schema (drops all per-table state files).
/// Original: `rmdir_force`.
pub fn rmdir_force(path: &str) -> Result<(), Error> {
    todo!("std::fs::remove_dir_all; map io::Error to Error::unavailable")
}

/// Performance-counter snapshot for a single table; backed by SlateDB metrics.
/// Original: `rdb_get_table_perf_counters`.
pub fn rdb_get_table_perf_counters(_table_name: &str) -> TablePerfCounters {
    todo!()
}

/// Per-table snapshot of perf counters.
#[derive(Debug, Default, Clone)]
pub struct TablePerfCounters {
    pub reads: u64,
    pub writes: u64,
    pub cache_hits: u64,
    pub cache_misses: u64,
}

/// Returns max value for an integer-typed column (for AUTO_INCREMENT
/// overflow detection).
/// Original: `rdb_get_int_col_max_value`.
pub fn rdb_get_int_col_max_value(_field_type: u32, _unsigned: bool) -> u64 {
    todo!("derive from MariaDB field type metadata")
}

/// Walks the open-tables map and returns the names of all currently-open
/// tables. Used by I_S table fillers.
/// Original: `rdb_get_open_table_names`.
pub fn rdb_get_open_table_names() -> Vec<String> {
    todo!("snapshot the global Rdb_open_tables_map")
}
