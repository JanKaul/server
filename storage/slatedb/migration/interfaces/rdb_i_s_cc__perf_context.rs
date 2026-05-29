//! Interface stub for `rdb_i_s_cc__perf_context`.
//!
//! C++ source: `storage/rocksdb/rdb_i_s.cc` (lines 263..361, plug at 1800..1814)
//! Body LoC: ~99
//! v4 manifest sub-unit: `rdb_i_s_cc__perf_context`
//! Parent: `rdb_i_s_cc`
//!
//! ## Mapping
//! Per _DESIGN.md §1 row "`rdb_perf_context`" — **Re-implemented** against
//! `slatedb_common::metrics`. RocksDB perf counters (`PC_MAX_IDX` of them,
//! e.g. `BLOCK_CACHE_HIT_COUNT`, `BLOOM_SST_HIT_COUNT`, `INTERNAL_KEY_SKIPPED_COUNT`)
//! don't all have SlateDB analogues. The I_S surface is preserved
//! `(TABLE_SCHEMA, TABLE_NAME, PARTITION_NAME, STAT_TYPE, VALUE)` and rows
//! whose counters we can't map emit `VALUE = 0` so dashboards don't break.
//!
//! Per-table aggregation: MyRocks tracks counters per open table via
//! `rdb_get_table_perf_counters(name, &counters)`. We replicate this by labelling
//! `slatedb_common::metrics` histograms/counters with `table=<db>.<name>` and
//! reading the label set here.
//!
//! Mapping table (subset — full table in `crate::metrics::pc_map`):
//! - `USER_KEY_COMPARISON_COUNT`  → `metrics::iter_key_compare_total`
//! - `BLOCK_CACHE_HIT_COUNT`      → `metrics::block_cache_hits_total`
//! - `BLOCK_READ_COUNT`           → `metrics::sst_block_reads_total`
//! - `BLOCK_READ_BYTE`            → `metrics::sst_block_read_bytes_total`
//! - `GET_FROM_MEMTABLE_COUNT`    → `metrics::memtable_get_total`
//! - `BLOOM_SST_HIT_COUNT`        → `metrics::bloom_filter_hits_total`
//! - all `*_NANOS` timers         → SlateDB histogram `*_seconds * 1e9`
//! - rest                         → `0` (no analogue)
//!
//! ## Out-of-scope methods
//! None — schema preserved; missing counters surface as zero rows.

use crate::rdb_i_s_cc__shared::{Column, ColumnType, Nullable, Row};
use slatedb::Error;

/// Column layout: (TABLE_SCHEMA, TABLE_NAME, PARTITION_NAME, STAT_TYPE, VALUE).
/// Original: rdb_i_s.cc:270 — `rdb_i_s_perf_context_fields_info[]`.
pub fn fields_info() -> &'static [Column] {
    static FIELDS: once_cell::sync::Lazy<Vec<Column>> = once_cell::sync::Lazy::new(|| {
        vec![
            Column { name: "TABLE_SCHEMA",   ty: ColumnType::Varchar(65), nullable: Nullable::NotNull },
            Column { name: "TABLE_NAME",     ty: ColumnType::Varchar(65), nullable: Nullable::NotNull },
            Column { name: "PARTITION_NAME", ty: ColumnType::Varchar(65), nullable: Nullable::Nullable },
            Column { name: "STAT_TYPE",      ty: ColumnType::Varchar(65), nullable: Nullable::NotNull },
            Column { name: "VALUE",          ty: ColumnType::SLonglong,   nullable: Nullable::NotNull },
        ]
    });
    &FIELDS
}

/// One open-table descriptor as we know it. Built by walking the engine's
/// open-tables registry (translation of `rdb_get_open_table_names()`).
#[derive(Debug, Clone)]
pub struct OpenTable {
    pub dbname: String,
    pub tablename: String,
    /// `None` if the table isn't a partition.
    pub partname: Option<String>,
    /// The MyRocks-shaped fully-qualified name we use as the metric label.
    pub normalized: String,
}

pub struct FillCtx<'a> {
    pub open_tables: &'a [OpenTable],
}

/// Build the rowset for `information_schema.ROCKSDB_PERF_CONTEXT`.
///
/// For each open table × each `PC_*` stat name we emit one row; counters with
/// no SlateDB analogue emit `VALUE = 0` (preserves dashboards).
///
/// Original C++ source: rdb_i_s.cc:278 — `rdb_i_s_perf_context_fill_table`.
pub async fn fill_table(_ctx: FillCtx<'_>) -> Result<Vec<Row>, Error> {
    // TODO(human): finalise the `PC_*` → `slatedb_common::metrics` mapping
    // table; lives in `crate::metrics::pc_map` once that module exists.
    todo!("for each open_table × pc_stat_types, emit (schema, table, part, stat, val) row")
}

pub const PLUGIN_NAME: &str = "ROCKSDB_PERF_CONTEXT";

/// Original C++ source: rdb_i_s.cc:348 — `rdb_i_s_perf_context_init`.
pub fn init(_plugin: *mut std::ffi::c_void) -> Result<(), Error> {
    todo!("wire fields_info + sync wrapper into the ST_SCHEMA_TABLE")
}
