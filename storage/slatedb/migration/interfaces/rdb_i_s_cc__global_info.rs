//! Interface stub for `rdb_i_s_cc__global_info`.
//!
//! C++ source: `storage/rocksdb/rdb_i_s.cc` (lines 691..835, plug at 1848..1862)
//! Body LoC: ~145
//! v4 manifest sub-unit: `rdb_i_s_cc__global_info`
//! Parent: `rdb_i_s_cc`
//!
//! ## Mapping
//! Per _DESIGN.md §1 — **Re-implemented**.
//!
//! MyRocks emits a `(TYPE, NAME, VALUE)` row-stream describing miscellaneous
//! globals — binlog position, max-index-id, every CF's id+flags, in-flight
//! `DDL_DROP_INDEX_ONGOING` records. We populate these from:
//!
//! - **BINLOG** (`FILE`/`POS`/`GTID`)  — from our `Rdb_binlog_manager`
//!   translation, which stores binlog state in the SYSTEM CF (§3 of
//!   `_DESIGN.md` references the system-CF prefix).
//! - **MAX_INDEX_ID**                 — from our `Rdb_dict_manager`
//!   translation; max-index-id is a counter persisted in the SYSTEM CF.
//! - **CF_FLAGS**                     — one row per CF in the engine's CF
//!   registry; format `"cf_name [flags]"` preserved.
//! - **DDL_DROP_INDEX_ONGOING**       — one row per in-flight drop sweep
//!   record in the SYSTEM CF; `Rdb_dict_manager.get_ongoing_index_operation()`
//!   returns these.
//!
//! ## Out-of-scope methods
//! None — schema preserved.

use crate::rdb_i_s_cc__shared::{Column, ColumnType, Nullable, Row};
use crate::rdb_global_h::GlIndexId;
use slatedb::Error;

/// Column layout: (TYPE, NAME, VALUE).
/// Original: rdb_i_s.cc:698 — `rdb_i_s_global_info_fields_info[]`.
pub fn fields_info() -> &'static [Column] {
    static FIELDS: once_cell::sync::Lazy<Vec<Column>> = once_cell::sync::Lazy::new(|| {
        vec![
            Column { name: "TYPE",  ty: ColumnType::Varchar(513), nullable: Nullable::NotNull },
            Column { name: "NAME",  ty: ColumnType::Varchar(513), nullable: Nullable::NotNull },
            Column { name: "VALUE", ty: ColumnType::Varchar(513), nullable: Nullable::NotNull },
        ]
    });
    &FIELDS
}

/// Cached binlog state — the bridge wrapper reads it once before invoking
/// `fill_table` so we don't take a lock inside an async fn.
#[derive(Debug, Clone, Default)]
pub struct BinlogState {
    pub file: String,
    pub pos: u64,
    pub gtid: String,
    pub present: bool,
}

pub struct FillCtx<'a> {
    pub binlog: &'a BinlogState,
    pub max_index_id: Option<u32>,
    /// (cf_id, cf_name, cf_flags) for each registered CF.
    pub cf_flags: &'a [(u32, String, u32)],
    /// In-flight `DDL_DROP_INDEX_ONGOING` records.
    pub drop_ongoing: &'a [GlIndexId],
}

/// Helper that mirrors `rdb_global_info_fill_row(thd, tables, type, name, value)`
/// from the C++ source — appends one `(type, name, value)` row to `rows`.
///
/// Original C++ source: rdb_i_s.cc:709 — `rdb_global_info_fill_row`.
pub fn push_row(rows: &mut Vec<Row>, type_str: &str, name: &str, value: &str) {
    use crate::rdb_i_s_cc__shared::FieldValue;
    rows.push(Row {
        values: vec![
            FieldValue::Str(type_str.to_string()),
            FieldValue::Str(name.to_string()),
            FieldValue::Str(value.to_string()),
        ],
    });
}

/// Build the rowset for `information_schema.ROCKSDB_GLOBAL_INFO`.
///
/// Original C++ source: rdb_i_s.cc:734 — `rdb_i_s_global_info_fill_table`.
pub async fn fill_table(_ctx: FillCtx<'_>) -> Result<Vec<Row>, Error> {
    // Order matches the C++: BINLOG triplet, then MAX_INDEX_ID, then CF_FLAGS
    // rows in CF-id iteration order, then DDL_DROP_INDEX_ONGOING rows.
    todo!("emit BINLOG/{FILE,POS,GTID}, MAX_INDEX_ID, CF_FLAGS×N, DDL_DROP_INDEX_ONGOING×N")
}

pub const PLUGIN_NAME: &str = "ROCKSDB_GLOBAL_INFO";

/// Original C++ source: rdb_i_s.cc:1073 — `rdb_i_s_global_info_init`.
pub fn init(_plugin: *mut std::ffi::c_void) -> Result<(), Error> {
    todo!("wire fields_info + sync wrapper into the ST_SCHEMA_TABLE")
}
