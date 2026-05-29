//! Interface stub for `rdb_i_s_cc__cfstats`.
//!
//! C++ source: `storage/rocksdb/rdb_i_s.cc` (lines 61..168, plug at 1768..1782)
//! Body LoC: ~108
//! v4 manifest sub-unit: `rdb_i_s_cc__cfstats`
//! Parent: `rdb_i_s_cc`
//!
//! ## Mapping
//! Per _DESIGN.md §1 row "`information_schema` tables (13)" — **Re-implemented**.
//!
//! MyRocks exposes per-CF runtime counters (memtable size, immutable count,
//! flush-pending flag, etc.) via `DB::GetIntProperty(cfh, kFoo, &val)`. With our
//! key-prefix CF scheme there is *one* SlateDB instance; "per-CF" is a view
//! derived from `Db::manifest()` and per-prefix scan stats from
//! `slatedb_common::metrics`. We preserve the three-column schema
//! `(CF_NAME, STAT_TYPE, VALUE)` and emit one row per (CF, stat-name) pair we
//! can compute, omitting stats with no SlateDB analogue.
//!
//! Specifically:
//! - `NUM_IMMUTABLE_MEM_TABLE`        — `VersionedManifest.wal_id_last_compacted`
//!                                       derived count of un-merged WALs.
//! - `MEM_TABLE_FLUSH_PENDING`        — `DbStatus.pending_flush` (boolean → 0/1).
//! - `COMPACTION_PENDING`             — derived from `VersionedManifest.compactor_state`.
//! - `CUR_SIZE_ACTIVE_MEM_TABLE`      — `slatedb_common::metrics::MEMTABLE_BYTES_ACTIVE`.
//! - `CUR_SIZE_ALL_MEM_TABLES`        — sum of active + immutable.
//! - `NUM_ENTRIES_ACTIVE_MEM_TABLE`   — `metrics::MEMTABLE_ENTRIES_ACTIVE`.
//! - `NUM_ENTRIES_IMM_MEM_TABLES`     — `metrics::MEMTABLE_ENTRIES_IMMUTABLE`.
//! - `NON_BLOCK_CACHE_SST_MEM_USAGE`  — not exposed by SlateDB; emit `0` row.
//! - `NUM_LIVE_VERSIONS`              — count of live `VersionedManifest` snapshots.
//!
//! ## Out-of-scope methods
//! None. The I_S surface is preserved; rows for stats SlateDB doesn't expose
//! still appear with `VALUE = 0` so dashboards keep parsing.

use crate::rdb_i_s_cc__shared::{Column, ColumnType, Nullable, Row};
use slatedb::Error;
use slatedb::config::DbStatus;

/// Column layout: (CF_NAME, STAT_TYPE, VALUE).
/// Original: rdb_i_s.cc:80 — `rdb_i_s_cfstats_fields_info[]`.
pub fn fields_info() -> &'static [Column] {
    static FIELDS: once_cell::sync::Lazy<Vec<Column>> = once_cell::sync::Lazy::new(|| {
        vec![
            Column { name: "CF_NAME",   ty: ColumnType::Varchar(65),  nullable: Nullable::NotNull },
            Column { name: "STAT_TYPE", ty: ColumnType::Varchar(65),  nullable: Nullable::NotNull },
            Column { name: "VALUE",     ty: ColumnType::SLonglong,    nullable: Nullable::NotNull },
        ]
    });
    &FIELDS
}

/// Context passed in from the sync bridge wrapper. Holds the live `slatedb::Db`
/// handle, the latest `DbStatus` snapshot, and the CF-id → CF-name mapping
/// from our `Rdb_cf_manager` translation.
pub struct FillCtx<'a> {
    pub manifest: &'a slatedb::config::VersionedManifest,
    pub status: &'a DbStatus,
    /// (cf_id, cf_name) pairs from `crate::codec::cf` registry.
    pub cf_table: &'a [(u32, String)],
}

/// Build the rowset for `information_schema.ROCKSDB_CFSTATS`.
///
/// - **Inputs:** live manifest + status snapshot + CF table.
/// - **Outputs:** `Vec<Row>` of three-column rows (one per (cf, stat) pair).
/// - **Errors:** `slatedb::ErrorKind::Unavailable` if a metrics read fails
///   (very rare — `slatedb_common::metrics` is in-process).
/// - **Invariants:** preserves the column layout MyRocks ships so existing
///   dashboards parse without change.
///
/// Original C++ source: rdb_i_s.cc:87 — `rdb_i_s_cfstats_fill_table`.
pub async fn fill_table(_ctx: FillCtx<'_>) -> Result<Vec<Row>, Error> {
    todo!("iterate cf_table × stat_kinds, read each metric, push (cf_name, stat, value) row")
}

/// Plugin descriptor name. Original: rdb_i_s.cc:1771 — `"ROCKSDB_CFSTATS"`.
pub const PLUGIN_NAME: &str = "ROCKSDB_CFSTATS";

/// Plugin init callback. Wires `fields_info` + a sync wrapper around `fill_table`
/// into the supplied `ST_SCHEMA_TABLE`. The wrapper does
/// `runtime.block_on(fill_table(...))` then materialises each `Row` into the
/// MariaDB row buffer via `schema_table_store_record`.
///
/// Original C++ source: rdb_i_s.cc:155 — `rdb_i_s_cfstats_init`.
pub fn init(_plugin: *mut std::ffi::c_void) -> Result<(), Error> {
    todo!("set schema->fields_info / schema->fill_table on the supplied ST_SCHEMA_TABLE")
}
