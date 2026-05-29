//! Interface stub for `rdb_i_s_cc__dbstats`.
//!
//! C++ source: `storage/rocksdb/rdb_i_s.cc` (lines 170..261, plug at 1784..1798)
//! Body LoC: ~92
//! v4 manifest sub-unit: `rdb_i_s_cc__dbstats`
//! Parent: `rdb_i_s_cc`
//!
//! ## Mapping
//! Per _DESIGN.md §1 — **Re-implemented**.
//!
//! MyRocks emits four rows of DB-wide stats:
//! `DB_BACKGROUND_ERRORS`, `DB_NUM_SNAPSHOTS`, `DB_OLDEST_SNAPSHOT_TIME`,
//! `DB_BLOCK_CACHE_USAGE`. We map each from SlateDB-native sources:
//!
//! - `DB_BACKGROUND_ERRORS`   — count of `slatedb::ErrorKind::Internal` seen
//!                              since startup; tracked by our error adapter.
//! - `DB_NUM_SNAPSHOTS`       — count of live `Arc<DbSnapshot>` we've handed out
//!                              (tracked in our snapshot registry).
//! - `DB_OLDEST_SNAPSHOT_TIME`— `create_ts` of the oldest snapshot in the registry,
//!                              or `0` if none.
//! - `DB_BLOCK_CACHE_USAGE`   — from `DbCacheManagerOps::block_cache_metrics()`
//!                              if available; otherwise `0`.
//!
//! ## Out-of-scope methods
//! None — surface preserved.

use crate::rdb_i_s_cc__shared::{Column, ColumnType, Nullable, Row};
use slatedb::Error;
use slatedb::config::DbStatus;

/// Column layout: (STAT_TYPE, VALUE).
/// Original: rdb_i_s.cc:177 — `rdb_i_s_dbstats_fields_info[]`.
pub fn fields_info() -> &'static [Column] {
    static FIELDS: once_cell::sync::Lazy<Vec<Column>> = once_cell::sync::Lazy::new(|| {
        vec![
            Column { name: "STAT_TYPE", ty: ColumnType::Varchar(65), nullable: Nullable::NotNull },
            Column { name: "VALUE",     ty: ColumnType::SLonglong,   nullable: Nullable::NotNull },
        ]
    });
    &FIELDS
}

/// Inputs needed to compute the four rows. The bridge wrapper assembles this
/// from engine state before calling `fill_table`.
pub struct FillCtx<'a> {
    pub status: &'a DbStatus,
    /// Snapshot of our snapshot-registry — (snapshot_id, create_unix_ts).
    pub live_snapshots: &'a [(u64, i64)],
    /// Background error count from our `slatedb::Error` adapter.
    pub background_errors: u64,
    /// Block-cache bytes in use from `DbCacheManagerOps` if exposed; else 0.
    pub block_cache_bytes: u64,
}

/// Build the rowset for `information_schema.ROCKSDB_DBSTATS`.
///
/// Original C++ source: rdb_i_s.cc:182 — `rdb_i_s_dbstats_fill_table`.
pub async fn fill_table(_ctx: FillCtx<'_>) -> Result<Vec<Row>, Error> {
    todo!("emit the four stat rows, deriving oldest snapshot time from live_snapshots")
}

pub const PLUGIN_NAME: &str = "ROCKSDB_DBSTATS";

/// Original C++ source: rdb_i_s.cc:248 — `rdb_i_s_dbstats_init`.
pub fn init(_plugin: *mut std::ffi::c_void) -> Result<(), Error> {
    todo!("wire fields_info + sync wrapper into the ST_SCHEMA_TABLE")
}
