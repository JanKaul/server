//! Interface stub for `rdb_i_s_cc__ddl`.
//!
//! C++ source: `storage/rocksdb/rdb_i_s.cc` (lines 909..1056, plug at 1880..1894)
//! Body LoC: ~148
//! v4 manifest sub-unit: `rdb_i_s_cc__ddl`
//! Parent: `rdb_i_s_cc`
//!
//! ## Mapping
//! Per _DESIGN.md §1 — **Re-implemented**.
//!
//! MyRocks walks `Rdb_ddl_manager->scan_for_tables(&visitor)` and the visitor
//! emits one row per `(table, index)` pair. Our translation of
//! `Rdb_ddl_manager` lives in `rdb_datadic_h__Rdb_ddl_manager.rs` and stores
//! `Rdb_tbl_def` records in the SYSTEM CF (prefix `__system__`, key prefix
//! `tbl:`). We re-use that registry: walk every `TblDef`, walk its `KeyDef`s,
//! emit a row per key.
//!
//! The auto-increment column reads from `Rdb_dict_manager.get_auto_incr_val()`
//! — the persisted per-table auto-increment counter in the SYSTEM CF.
//!
//! Schema preserved exactly:
//! `(TABLE_SCHEMA, TABLE_NAME, PARTITION_NAME, INDEX_NAME, COLUMN_FAMILY,
//!   INDEX_NUMBER, INDEX_TYPE, KV_FORMAT_VERSION, TTL_DURATION, INDEX_FLAGS,
//!   CF, AUTO_INCREMENT)`.
//!
//! ## Out-of-scope methods
//! None — all 12 columns are still meaningful in the SlateDB layout.

use crate::rdb_i_s_cc__shared::{Column, ColumnType, Nullable, Row};
use slatedb::Error;

/// Column layout: 12 fields.
/// Original: rdb_i_s.cc:939 — `rdb_i_s_ddl_fields_info[]`.
pub fn fields_info() -> &'static [Column] {
    static FIELDS: once_cell::sync::Lazy<Vec<Column>> = once_cell::sync::Lazy::new(|| {
        vec![
            Column { name: "TABLE_SCHEMA",      ty: ColumnType::Varchar(65),  nullable: Nullable::NotNull },
            Column { name: "TABLE_NAME",        ty: ColumnType::Varchar(65),  nullable: Nullable::NotNull },
            Column { name: "PARTITION_NAME",    ty: ColumnType::Varchar(65),  nullable: Nullable::Nullable },
            Column { name: "INDEX_NAME",        ty: ColumnType::Varchar(65),  nullable: Nullable::NotNull },
            Column { name: "COLUMN_FAMILY",     ty: ColumnType::SLong,        nullable: Nullable::NotNull },
            Column { name: "INDEX_NUMBER",      ty: ColumnType::SLong,        nullable: Nullable::NotNull },
            Column { name: "INDEX_TYPE",        ty: ColumnType::SShort(6),    nullable: Nullable::NotNull },
            Column { name: "KV_FORMAT_VERSION", ty: ColumnType::SShort(6),    nullable: Nullable::NotNull },
            Column { name: "TTL_DURATION",      ty: ColumnType::SLonglong,    nullable: Nullable::NotNull },
            Column { name: "INDEX_FLAGS",       ty: ColumnType::SLonglong,    nullable: Nullable::NotNull },
            Column { name: "CF",                ty: ColumnType::Varchar(65),  nullable: Nullable::NotNull },
            Column { name: "AUTO_INCREMENT",    ty: ColumnType::ULonglong,    nullable: Nullable::Nullable },
        ]
    });
    &FIELDS
}

/// A snapshot of one index taken from the `Rdb_ddl_manager` translation.
/// Built by the bridge wrapper before calling `fill_table`.
#[derive(Debug, Clone)]
pub struct DdlIndexRow {
    pub schema: String,
    pub table: String,
    pub partition: Option<String>,
    pub index_name: String,
    pub cf_id: u32,
    pub index_id: u32,
    pub index_type: i16,
    pub kv_format_version: i16,
    pub ttl_duration: i64,
    pub index_flags: i64,
    pub cf_name: String,
    pub auto_increment: Option<u64>,
}

pub struct FillCtx<'a> {
    /// Pre-flattened `(table × index)` rows from `Rdb_ddl_manager::scan_for_tables()`.
    pub rows: &'a [DdlIndexRow],
}

/// Build the rowset for `information_schema.ROCKSDB_DDL`.
///
/// Original C++ source: rdb_i_s.cc:1014 — `rdb_i_s_ddl_fill_table`.
/// Inner row-builder logic: rdb_i_s.cc:954 — `Rdb_ddl_scanner::add_table`.
pub async fn fill_table(_ctx: FillCtx<'_>) -> Result<Vec<Row>, Error> {
    todo!("map each DdlIndexRow → 12-column Row, preserve nullable PARTITION_NAME / AUTO_INCREMENT")
}

pub const PLUGIN_NAME: &str = "ROCKSDB_DDL";

/// Original C++ source: rdb_i_s.cc:1043 — `rdb_i_s_ddl_init`.
pub fn init(_plugin: *mut std::ffi::c_void) -> Result<(), Error> {
    todo!("wire fields_info + sync wrapper into the ST_SCHEMA_TABLE")
}
