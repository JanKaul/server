//! Interface stub for `rdb_i_s_cc__deadlock_info`.
//!
//! C++ source: `storage/rocksdb/rdb_i_s.cc` (lines 1646..1757, plug at 1960..1975)
//! Body LoC: ~112
//! v4 manifest sub-unit: `rdb_i_s_cc__deadlock_info`
//! Parent: `rdb_i_s_cc`
//!
//! ## Mapping
//! Per _DESIGN.md §1 — **Re-implemented**.
//!
//! MyRocks builds rows from `rdb_get_deadlock_info()` (declared in
//! `rdb_global_h.rs`). Our impl returns deadlocks captured from SlateDB's
//! `IsolationLevel::SerializableSnapshot` conflict errors — at `txn.commit()`,
//! a `slatedb::ErrorKind::Transaction` indicates an SSI conflict; our adapter
//! formats the participants + waiting keys into a `DeadlockInfo` and appends
//! to a bounded ring buffer (size `slatedb_max_latest_deadlocks` sysvar).
//!
//! NB: SlateDB SSI surfaces a single "this commit lost" error, not the full
//! cycle that RocksDB's pessimistic deadlock detector reports. The `path`
//! field of each `DeadlockInfo` therefore typically has length 1 (the losing
//! txn) — preserving the schema while reflecting the SSI semantics. We mark
//! the losing txn as `ROLLED_BACK = 1` so users see which side aborted.
//!
//! ## Out-of-scope methods
//! None — schema preserved. SSI-derived paths are shorter than RocksDB's
//! pessimistic-cycle paths but that's a semantic difference, not a missing
//! column.

use crate::rdb_global_h::DeadlockInfo;
use crate::rdb_i_s_cc__shared::{Column, ColumnType, Nullable, Row};
use slatedb::Error;

/// Column layout: 9 fields.
/// Original: rdb_i_s.cc:1663 — `rdb_i_s_deadlock_info_fields_info[]`.
pub fn fields_info() -> &'static [Column] {
    static FIELDS: once_cell::sync::Lazy<Vec<Column>> = once_cell::sync::Lazy::new(|| {
        vec![
            Column { name: "DEADLOCK_ID",    ty: ColumnType::SLonglong,    nullable: Nullable::NotNull },
            Column { name: "TIMESTAMP",      ty: ColumnType::SLonglong,    nullable: Nullable::NotNull },
            Column { name: "TRANSACTION_ID", ty: ColumnType::SLonglong,    nullable: Nullable::NotNull },
            Column { name: "CF_NAME",        ty: ColumnType::Varchar(65),  nullable: Nullable::NotNull },
            Column { name: "WAITING_KEY",    ty: ColumnType::Varchar(513), nullable: Nullable::NotNull },
            Column { name: "LOCK_TYPE",      ty: ColumnType::Varchar(65),  nullable: Nullable::NotNull },
            Column { name: "INDEX_NAME",     ty: ColumnType::Varchar(65),  nullable: Nullable::NotNull },
            Column { name: "TABLE_NAME",     ty: ColumnType::Varchar(65),  nullable: Nullable::NotNull },
            Column { name: "ROLLED_BACK",    ty: ColumnType::SLonglong,    nullable: Nullable::NotNull },
        ]
    });
    &FIELDS
}

pub struct FillCtx<'a> {
    /// Snapshot of `rdb_get_deadlock_info()` — the deadlock-history ring buffer.
    pub all_dl_info: &'a [DeadlockInfo],
}

/// Build the rowset for `information_schema.ROCKSDB_DEADLOCK`.
///
/// Each `DeadlockInfo` expands into N rows where N = `path.len()`; the
/// `DEADLOCK_ID` column starts at 0 and is shared across the rows of one
/// deadlock event (mirrors C++ behaviour at rdb_i_s.cc:1698).
///
/// Original C++ source: rdb_i_s.cc:1676 — `rdb_i_s_deadlock_info_fill_table`.
pub async fn fill_table(_ctx: FillCtx<'_>) -> Result<Vec<Row>, Error> {
    todo!("expand each DeadlockInfo → path.len() rows; LOCK_TYPE = EXCLUSIVE|SHARED")
}

pub const PLUGIN_NAME: &str = "ROCKSDB_DEADLOCK";

/// Original C++ source: rdb_i_s.cc:1744 — `rdb_i_s_deadlock_info_init`.
pub fn init(_plugin: *mut std::ffi::c_void) -> Result<(), Error> {
    todo!("wire fields_info + sync wrapper into the ST_SCHEMA_TABLE")
}
