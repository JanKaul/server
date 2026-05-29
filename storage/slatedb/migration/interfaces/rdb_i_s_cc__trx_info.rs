//! Interface stub for `rdb_i_s_cc__trx_info`.
//!
//! C++ source: `storage/rocksdb/rdb_i_s.cc` (lines 1521..1644, plug at 1944..1958)
//! Body LoC: ~124
//! v4 manifest sub-unit: `rdb_i_s_cc__trx_info`
//! Parent: `rdb_i_s_cc`
//!
//! ## Mapping
//! Per _DESIGN.md §1 — **Re-implemented**.
//!
//! MyRocks builds rows from `rdb_get_all_trx_info()` (declared in
//! `rdb_global_h.rs`). Our implementation of that function walks the per-Txn
//! registry built when we wrap each `slatedb::DbTransaction`. Each registry
//! entry stores the metadata required by the I_S row plus a handle into the
//! underlying SlateDB txn so we can read `seqnum()` / `id()` live.
//!
//! Schema preserved exactly (15 columns).
//!
//! ## Out-of-scope methods
//! None — every column is populatable; `IS_REPLICATION` is `0` until we add
//! row-based replication support (out of scope for Stage 1).

use crate::rdb_global_h::TrxInfo;
use crate::rdb_i_s_cc__shared::{Column, ColumnType, Nullable, Row};
use slatedb::Error;

/// Column layout: 15 fields.
/// Original: rdb_i_s.cc:1544 — `rdb_i_s_trx_info_fields_info[]`.
pub fn fields_info() -> &'static [Column] {
    static FIELDS: once_cell::sync::Lazy<Vec<Column>> = once_cell::sync::Lazy::new(|| {
        vec![
            Column { name: "TRANSACTION_ID",          ty: ColumnType::SLonglong,    nullable: Nullable::NotNull },
            Column { name: "STATE",                   ty: ColumnType::Varchar(65),  nullable: Nullable::NotNull },
            Column { name: "NAME",                    ty: ColumnType::Varchar(65),  nullable: Nullable::NotNull },
            Column { name: "WRITE_COUNT",             ty: ColumnType::SLonglong,    nullable: Nullable::NotNull },
            Column { name: "LOCK_COUNT",              ty: ColumnType::SLonglong,    nullable: Nullable::NotNull },
            Column { name: "TIMEOUT_SEC",             ty: ColumnType::SLong,        nullable: Nullable::NotNull },
            Column { name: "WAITING_KEY",             ty: ColumnType::Varchar(513), nullable: Nullable::NotNull },
            Column { name: "WAITING_COLUMN_FAMILY_ID",ty: ColumnType::SLong,        nullable: Nullable::NotNull },
            Column { name: "IS_REPLICATION",          ty: ColumnType::SLong,        nullable: Nullable::NotNull },
            Column { name: "SKIP_TRX_API",            ty: ColumnType::SLong,        nullable: Nullable::NotNull },
            Column { name: "READ_ONLY",               ty: ColumnType::SLong,        nullable: Nullable::NotNull },
            Column { name: "HAS_DEADLOCK_DETECTION",  ty: ColumnType::SLong,        nullable: Nullable::NotNull },
            Column { name: "NUM_ONGOING_BULKLOAD",    ty: ColumnType::SLong,        nullable: Nullable::NotNull },
            Column { name: "THREAD_ID",               ty: ColumnType::SLong,        nullable: Nullable::NotNull },
            Column { name: "QUERY",                   ty: ColumnType::Varchar(65),  nullable: Nullable::NotNull },
        ]
    });
    &FIELDS
}

pub struct FillCtx<'a> {
    /// Snapshot of `rdb_get_all_trx_info()`.
    pub trx_info: &'a [TrxInfo],
}

/// Build the rowset for `information_schema.ROCKSDB_TRX`.
///
/// The C++ hex-dumps `name` and `waiting_key` for printability; we preserve
/// that — the bridge wrapper calls our `crate::utils::hexdump` helper.
///
/// Original C++ source: rdb_i_s.cc:1563 — `rdb_i_s_trx_info_fill_table`.
pub async fn fill_table(_ctx: FillCtx<'_>) -> Result<Vec<Row>, Error> {
    todo!("map each TrxInfo → 15-column Row, hexdump name/waiting_key")
}

pub const PLUGIN_NAME: &str = "ROCKSDB_TRX";

/// Original C++ source: rdb_i_s.cc:1631 — `rdb_i_s_trx_info_init`.
pub fn init(_plugin: *mut std::ffi::c_void) -> Result<(), Error> {
    todo!("wire fields_info + sync wrapper into the ST_SCHEMA_TABLE")
}
