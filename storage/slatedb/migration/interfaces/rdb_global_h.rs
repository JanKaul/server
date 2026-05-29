//! Interface stub for `rdb_global_h`.
//!
//! C++ source: `storage/rocksdb/rdb_global.h` (396 LoC)
//!
//! ## Mapping
//! Global typedefs + constants. The most depended-on header in the manifest —
//! 25+ other units transitively use the types declared here. Translation is
//! mostly mechanical: C++ struct → Rust struct, `#define` → `pub const`, enum
//! → `#[repr(i32)] enum`.
//!
//! Per _DESIGN.md §2: `GL_INDEX_ID` is the (cf_id, index_id) tuple used to
//! identify an index across CFs. With our key-prefix scheme (one SlateDB
//! instance, CFs are prefixes) the meaning carries through unchanged.
//!
//! ## Out-of-scope items
//! None — this is pure type/constant declaration, fully in scope.

use crate::error::SlateError;

// --- transaction info (for I_S.rocksdb_trx) ---

/// One row of `information_schema.rocksdb_trx`. Populated by `rdb_get_all_trx_info()`.
///
/// Original: rdb_global.h:37 — `struct Rdb_trx_info`.
#[derive(Debug, Clone)]
pub struct TrxInfo {
    pub name: String,
    pub trx_id: u64,
    pub write_count: u64,
    pub lock_count: u64,
    pub timeout_sec: i32,
    pub state: String,
    pub waiting_key: String,
    pub waiting_cf_id: u64,
    pub is_replication: bool,
    pub skip_trx_api: bool,
    pub read_only: bool,
    pub deadlock_detect: bool,
    pub num_ongoing_bulk_load: i32,
    pub thread_id: u64,
    pub query_str: String,
}

/// Snapshot of all currently-active transactions. Called by I_S table fill_table fn.
/// Returns empty vec rather than error if the engine isn't initialized.
///
/// Original: rdb_global.h:55 — `std::vector<Rdb_trx_info> rdb_get_all_trx_info()`.
pub fn get_all_trx_info() -> Vec<TrxInfo> {
    todo!("walk the txn registry, snapshot each Txn's state into TrxInfo")
}

// --- deadlock info (for I_S.rocksdb_deadlock) ---

#[derive(Debug, Clone)]
pub struct DeadlockDlTrxInfo {
    pub trx_id: u64,
    pub cf_name: String,
    pub waiting_key: String,
    pub exclusive_lock: bool,
    pub index_name: String,
    pub table_name: String,
}

#[derive(Debug, Clone)]
pub struct DeadlockInfo {
    pub path: Vec<DeadlockDlTrxInfo>,
    pub deadlock_time: i64, // unix epoch seconds
    pub victim_trx_id: u64,
}

/// Recent deadlock history (bounded ring buffer, size = sysvar
/// `slatedb_max_latest_deadlocks`).
///
/// Original: rdb_global.h:75 — `std::vector<Rdb_deadlock_info> rdb_get_deadlock_info()`.
pub fn get_deadlock_info() -> Vec<DeadlockInfo> {
    todo!("snapshot the deadlock-history ring buffer")
}

// --- naming constants ---

/// Default Column Family name. With our key-prefix scheme this is the prefix
/// used for indexes that didn't pick a specific CF in their CREATE TABLE comment.
/// Original: rdb_global.h:84 — `DEFAULT_CF_NAME`.
pub const DEFAULT_CF_NAME: &str = "default";

/// CF name used for the data dictionary (schema metadata, table_id → name, etc.).
/// Original: rdb_global.h:89 — `DEFAULT_SYSTEM_CF_NAME`.
pub const DEFAULT_SYSTEM_CF_NAME: &str = "__system__";

/// Per-table hidden PK column name for tables without an explicit primary key.
/// Original: rdb_global.h:94 — `HIDDEN_PK_NAME`.
pub const HIDDEN_PK_NAME: &str = "HIDDEN_PK_ID";

/// Deprecated CF name marker — kept for backwards compatibility parsing.
/// Original: rdb_global.h:100 — `PER_INDEX_CF_NAME`.
pub const PER_INDEX_CF_NAME: &str = "$per_index_cf";

/// Per-partition comment qualifier syntax: `p<N>_cfname=foo;p<N>_ttl_duration=...`.
pub const PER_PARTITION_QUALIFIER_NAME_SEP: char = '_';
pub const QUALIFIER_VALUE_SEP: char = '=';
pub const QUALIFIER_SEP: char = ';';
pub const CF_NAME_QUALIFIER: &str = "cfname";
pub const TTL_DURATION_QUALIFIER: &str = "ttl_duration";
pub const TTL_COL_QUALIFIER: &str = "ttl_col";

// --- numeric constants ---

pub const DEFAULT_TBL_STATS_SAMPLE_PCT: u32 = 10;
pub const TBL_STATS_SAMPLE_PCT_MIN: u32 = 1;
pub const TBL_STATS_SAMPLE_PCT_MAX: u32 = 100;

/// Hidden PK column width (longlong). Static assertions in code verify size.
pub const SIZEOF_HIDDEN_PK_COLUMN: usize = 8;

/// TTL field prefix width on rows when TTL is enabled.
pub const SIZEOF_TTL_RECORD: usize = 8;

/// Width of the auto-increment value stored in the per-table dict entry.
pub const SIZEOF_AUTOINC_VALUE: usize = 8;

/// Max prefix length in bytes for indexed columns (large/small file format).
pub const MAX_INDEX_COL_LEN_LARGE: u32 = 3072;
pub const MAX_INDEX_COL_LEN_SMALL: u32 = 767;

// --- error code base ---

/// First MyRocks-specific HA error code. Engine-specific codes start here.
/// `rdb_global.h:242` — `HA_ERR_ROCKSDB_FIRST = 500`.
///
/// We reuse this range for SlateDB-specific errors since MyRocks-on-RocksDB
/// stays in tree (§1 — "ha_rocksdb removal: Never"). Our codes start at 550
/// to avoid collision until ha_rocksdb is retired.
pub const HA_ERR_SLATEDB_FIRST: i32 = 550;

// --- (cf_id, index_id) tuple ---

/// Identifies an index globally. With key-prefix scheme, this is the basis of
/// the key prefix (`varint(cf_id) || index_id_u32_be || memcmp_key`).
///
/// Original: rdb_global.h:283 — `GL_INDEX_ID`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GlIndexId {
    pub cf_id: u32,
    pub index_id: u32,
}

// --- row-operation counters ---

/// Per-table row-op buckets used for the `rows_*` SHOW STATUS counters.
/// Original: rdb_global.h:310 — `enum operation_type`.
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationType {
    Deleted = 0,
    Inserted = 1,
    Read = 2,
    Updated = 3,
    DeletedBlind = 4,
    Expired = 5,
    Filtered = 6,
    HiddenNoSnapshot = 7,
}
pub const OPERATION_TYPE_COUNT: usize = 8;

#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueryType {
    Point = 0,
    Range = 1,
}
pub const QUERY_TYPE_COUNT: usize = 2;

// --- stats structs ---

/// Internal global stats: per-op-type sharded counters.
/// Original: rdb_global.h:331 — `struct st_global_stats`.
///
/// Sharded via `ut0counter` in MyRocks; in Rust we use a per-shard
/// `[AtomicStatU64; N]` array (see atomic_stat_h.rs).
pub struct GlobalStats {
    pub rows: [crate::atomic_stat_h::AtomicStatU64; OPERATION_TYPE_COUNT],
    pub system_rows: [crate::atomic_stat_h::AtomicStatU64; OPERATION_TYPE_COUNT],
    pub queries: [crate::atomic_stat_h::AtomicStatU64; QUERY_TYPE_COUNT],
    pub covered_secondary_key_lookups: crate::atomic_stat_h::AtomicStatU64,
}

/// Exported stats snapshot — read once for SHOW STATUS, then no further atomics.
/// Original: rdb_global.h:344 — `struct st_export_stats`.
#[derive(Debug, Clone, Default)]
pub struct ExportStats {
    pub rows_deleted: u64,
    pub rows_inserted: u64,
    pub rows_read: u64,
    pub rows_updated: u64,
    pub rows_deleted_blind: u64,
    pub rows_expired: u64,
    pub rows_filtered: u64,
    pub rows_hidden_no_snapshot: u64,
    pub system_rows_deleted: u64,
    pub system_rows_inserted: u64,
    pub system_rows_read: u64,
    pub system_rows_updated: u64,
    pub queries_point: u64,
    pub queries_range: u64,
    pub covered_secondary_key_lookups: u64,
}

/// Original: rdb_global.h:366 — `struct st_memory_stats`.
#[derive(Debug, Clone, Default)]
pub struct MemoryStats {
    pub memtable_total: u64,
    pub memtable_unflushed: u64,
}

/// IO-stall counters. Maps to SlateDB's equivalent metrics (which differ in
/// shape from RocksDB's). Per _DESIGN.md §1, this row is re-implemented:
/// fields whose semantics don't map are reported as zero.
///
/// Original: rdb_global.h:372 — `struct st_io_stall_stats`.
#[derive(Debug, Clone, Default)]
pub struct IoStallStats {
    pub level0_slowdown: u64,
    pub level0_slowdown_with_compaction: u64,
    pub level0_numfiles: u64,
    pub level0_numfiles_with_compaction: u64,
    pub stop_for_pending_compaction_bytes: u64,
    pub slowdown_for_pending_compaction_bytes: u64,
    pub memtable_compaction: u64,
    pub memtable_slowdown: u64,
    pub total_stop: u64,
    pub total_slowdown: u64,
}
