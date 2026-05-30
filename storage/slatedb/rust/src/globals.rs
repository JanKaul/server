//! Crate-wide globals: `(cf_id, index_id)` tuple, naming constants, stat
//! container shapes.
//!
//! Translated from `storage/rocksdb/rdb_global.h`. Only the leaf subset (the
//! parts that don't depend on the txn registry or deadlock ring) is ported in
//! this TRANSLATE bucket; transaction/deadlock snapshot helpers remain on the
//! `txn` module's TODO list.
//!
//! Per `_DESIGN.md §2`: `GlIndexId` is the basis of every key prefix:
//! `varint(cf_id) || u32_be(index_id) || memcmp_key`. The `PrefixExtractor`
//! consumes the `varint(cf_id) || u32_be(index_id)` head for bloom-filter
//! prefix lookups.

use crate::utils::atomic_stat::AtomicStatU64;

// --- naming constants ---

pub const DEFAULT_CF_NAME: &str = "default";
pub const DEFAULT_SYSTEM_CF_NAME: &str = "__system__";
pub const SYSTEM_CF_ID: u32 = u32::MAX;

pub const HIDDEN_PK_NAME: &str = "HIDDEN_PK_ID";
pub const PER_INDEX_CF_NAME: &str = "$per_index_cf";

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

pub const SIZEOF_HIDDEN_PK_COLUMN: usize = 8;
pub const SIZEOF_AUTOINC_VALUE: usize = 8;

pub const MAX_INDEX_COL_LEN_LARGE: u32 = 3072;
pub const MAX_INDEX_COL_LEN_SMALL: u32 = 767;

/// First SlateDB-specific HA error code. MyRocks uses 500; we reuse from 550
/// to avoid collision while `ha_rocksdb` stays in tree.
pub const HA_ERR_SLATEDB_FIRST: i32 = 550;

// --- (cf_id, index_id) tuple ---

/// Identifies an index globally.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GlIndexId {
    pub cf_id: u32,
    pub index_id: u32,
}

// --- row-operation counters ---

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

// --- stats containers ---

/// Internal global stats: per-op-type sharded counters.
pub struct GlobalStats {
    pub rows: [AtomicStatU64; OPERATION_TYPE_COUNT],
    pub system_rows: [AtomicStatU64; OPERATION_TYPE_COUNT],
    pub queries: [AtomicStatU64; QUERY_TYPE_COUNT],
    pub covered_secondary_key_lookups: AtomicStatU64,
}

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

#[derive(Debug, Clone, Default)]
pub struct MemoryStats {
    pub memtable_total: u64,
    pub memtable_unflushed: u64,
}

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
