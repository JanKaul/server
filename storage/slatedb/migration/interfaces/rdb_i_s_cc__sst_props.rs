//! Interface stub for `rdb_i_s_cc__sst_props`.
//!
//! C++ source: `storage/rocksdb/rdb_i_s.cc` (lines 1117..1278, plug at 1896..1910)
//! Body LoC: ~162
//! v4 manifest sub-unit: `rdb_i_s_cc__sst_props`
//! Parent: `rdb_i_s_cc`
//!
//! ## Mapping
//! Per _DESIGN.md §1 — **Re-implemented**.
//!
//! MyRocks calls `DB::GetPropertiesOfAllTables(cfh, &props_collection)` and
//! emits one row per SST file. SlateDB exposes per-SST metadata via
//! `VersionedManifest.l0` / `VersionedManifest.compacted` lists; each entry
//! carries `SsTableId`, `SsTableInfo` (entry count, size, compression codec),
//! `created_at`. We map those fields onto the existing 17-column schema:
//!
//! - `SST_NAME`            — SlateDB SST id (UUID hex stringified).
//! - `COLUMN_FAMILY`       — `cf_id` decoded from the key-prefix of the first
//!                           entry's `min_key`; `-1` if heterogeneous.
//! - `DATA_BLOCKS`         — `SsTableInfo.num_blocks`.
//! - `ENTRIES`             — `SsTableInfo.entry_count`.
//! - `RAW_KEY_SIZE`        — `SsTableInfo.raw_key_bytes`.
//! - `RAW_VALUE_SIZE`      — `SsTableInfo.raw_value_bytes`.
//! - `DATA_BLOCK_SIZE`     — `SsTableInfo.data_size`.
//! - `INDEX_BLOCK_SIZE`    — `SsTableInfo.index_size`.
//! - `INDEX_PARTITIONS`    — `0` (SlateDB doesn't partition indexes).
//! - `TOP_LEVEL_INDEX_SIZE`— `0` (same).
//! - `FILTER_BLOCK_SIZE`   — `SsTableInfo.filter_size`.
//! - `COMPRESSION_ALGO`    — codec name from `Settings.compression_codec`.
//! - `CREATION_TIME`       — `SsTableInfo.created_at` (unix seconds).
//! - `FILE_CREATION_TIME`  — same.
//! - `OLDEST_KEY_TIME`     — derivable from SST `min_create_ts`; `0` if missing.
//! - `FILTER_POLICY`       — `"slatedb-bloom"` if `Settings.min_filter_keys` < usize::MAX else `NULL`.
//! - `COMPRESSION_OPTIONS` — JSON of settings.compression_codec params; `NULL` if uncompressed.
//!
//! ## Out-of-scope methods
//! None — all 17 fields preserved; SlateDB-only gaps surface as `0` or `NULL`.

use crate::rdb_i_s_cc__shared::{Column, ColumnType, Nullable, Row};
use slatedb::Error;

/// Column layout: 17 fields.
/// Original: rdb_i_s.cc:1142 — `rdb_i_s_sst_props_fields_info[]`.
pub fn fields_info() -> &'static [Column] {
    static FIELDS: once_cell::sync::Lazy<Vec<Column>> = once_cell::sync::Lazy::new(|| {
        vec![
            Column { name: "SST_NAME",             ty: ColumnType::Varchar(65), nullable: Nullable::NotNull },
            Column { name: "COLUMN_FAMILY",        ty: ColumnType::SLong,       nullable: Nullable::NotNull },
            Column { name: "DATA_BLOCKS",          ty: ColumnType::SLonglong,   nullable: Nullable::NotNull },
            Column { name: "ENTRIES",              ty: ColumnType::SLonglong,   nullable: Nullable::NotNull },
            Column { name: "RAW_KEY_SIZE",         ty: ColumnType::SLonglong,   nullable: Nullable::NotNull },
            Column { name: "RAW_VALUE_SIZE",       ty: ColumnType::SLonglong,   nullable: Nullable::NotNull },
            Column { name: "DATA_BLOCK_SIZE",      ty: ColumnType::SLonglong,   nullable: Nullable::NotNull },
            Column { name: "INDEX_BLOCK_SIZE",     ty: ColumnType::SLonglong,   nullable: Nullable::NotNull },
            Column { name: "INDEX_PARTITIONS",     ty: ColumnType::SLong,       nullable: Nullable::NotNull },
            Column { name: "TOP_LEVEL_INDEX_SIZE", ty: ColumnType::SLonglong,   nullable: Nullable::NotNull },
            Column { name: "FILTER_BLOCK_SIZE",    ty: ColumnType::SLonglong,   nullable: Nullable::NotNull },
            Column { name: "COMPRESSION_ALGO",     ty: ColumnType::Varchar(65), nullable: Nullable::NotNull },
            Column { name: "CREATION_TIME",        ty: ColumnType::SLonglong,   nullable: Nullable::NotNull },
            Column { name: "FILE_CREATION_TIME",   ty: ColumnType::SLonglong,   nullable: Nullable::NotNull },
            Column { name: "OLDEST_KEY_TIME",      ty: ColumnType::SLonglong,   nullable: Nullable::NotNull },
            Column { name: "FILTER_POLICY",        ty: ColumnType::Varchar(65), nullable: Nullable::NotNull },
            Column { name: "COMPRESSION_OPTIONS",  ty: ColumnType::Varchar(65), nullable: Nullable::NotNull },
        ]
    });
    &FIELDS
}

pub struct FillCtx<'a> {
    pub manifest: &'a slatedb::config::VersionedManifest,
    pub settings: &'a slatedb::config::Settings,
}

/// Build the rowset for `information_schema.ROCKSDB_SST_PROPS`.
///
/// One row per SST in `manifest.l0` ∪ `manifest.compacted`. The bridge wrapper
/// is responsible for resolving the SST-id → display-name format
/// (UUID hex preserves the MyRocks `<number>.sst` shape closely enough for
/// dashboards).
///
/// Original C++ source: rdb_i_s.cc:1162 — `rdb_i_s_sst_props_fill_table`.
pub async fn fill_table(_ctx: FillCtx<'_>) -> Result<Vec<Row>, Error> {
    // TODO(human): confirm `SsTableInfo` field names against the slatedb workspace
    // (currently inferred from `_DESIGN.md` §0). If a field is missing emit 0.
    todo!("walk manifest.l0 + manifest.compacted, build 17-column row per SST")
}

pub const PLUGIN_NAME: &str = "ROCKSDB_SST_PROPS";

/// Original C++ source: rdb_i_s.cc:1265 — `rdb_i_s_sst_props_init`.
pub fn init(_plugin: *mut std::ffi::c_void) -> Result<(), Error> {
    todo!("wire fields_info + sync wrapper into the ST_SCHEMA_TABLE")
}
