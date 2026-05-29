//! Interface stub for `rdb_i_s_cc__cfoptions`.
//!
//! C++ source: `storage/rocksdb/rdb_i_s.cc` (lines 430..689, plug at 1832..1846)
//! Body LoC: ~260
//! v4 manifest sub-unit: `rdb_i_s_cc__cfoptions`
//! Parent: `rdb_i_s_cc`
//!
//! ## Mapping
//! Per _DESIGN.md §1 — **Re-implemented**.
//!
//! MyRocks emits ~40 rows per CF describing every RocksDB `ColumnFamilyOptions`
//! field. SlateDB has **one global `Settings`** (per_DESIGN.md §1, block cache
//! / compression / bloom are global, "degraded" verdicts). Our re-impl emits:
//!
//! - The same `(CF_NAME, OPTION_TYPE, VALUE)` schema.
//! - For every CF in `cf_table`: rows describing the *effective* options for
//!   that CF, drawn from the single global `Settings` (so every CF reports the
//!   same compression codec, etc.).
//! - Options whose RocksDB concept doesn't apply to SlateDB (e.g. `NUM_LEVELS`,
//!   `LEVEL0_FILE_NUM_COMPACTION_TRIGGER`, the universal-compaction block)
//!   emit `VALUE = "N/A (slatedb)"` so dashboards still see the key but don't
//!   misinterpret numeric defaults.
//! - SlateDB-only options that have no MyRocks analogue (e.g.
//!   `FLUSH_INTERVAL`, `AWAIT_DURABLE`) are added at the tail using the
//!   `TABLE_FACTORY::` style key prefix used in the original.
//!
//! ## Out-of-scope methods
//! None — schema preserved, opaque options surface as `N/A (slatedb)` rows.

use crate::rdb_i_s_cc__shared::{Column, ColumnType, Nullable, Row};
use slatedb::Error;
use slatedb::config::Settings;

/// Column layout: (CF_NAME, OPTION_TYPE, VALUE).
/// Original: rdb_i_s.cc:437 — `rdb_i_s_cfoptions_fields_info[]`.
pub fn fields_info() -> &'static [Column] {
    static FIELDS: once_cell::sync::Lazy<Vec<Column>> = once_cell::sync::Lazy::new(|| {
        vec![
            Column { name: "CF_NAME",     ty: ColumnType::Varchar(65), nullable: Nullable::NotNull },
            Column { name: "OPTION_TYPE", ty: ColumnType::Varchar(65), nullable: Nullable::NotNull },
            Column { name: "VALUE",       ty: ColumnType::Varchar(65), nullable: Nullable::NotNull },
        ]
    });
    &FIELDS
}

pub struct FillCtx<'a> {
    /// Resolved settings the running `Db` was built with.
    pub settings: &'a Settings,
    /// CF-id → name pairs from the engine's CF registry.
    pub cf_table: &'a [(u32, String)],
}

/// Build the rowset for `information_schema.ROCKSDB_CFOPTIONS`.
///
/// One row per (cf_name, option_name). For SlateDB-mapped options we emit the
/// real value; for RocksDB-only options we emit `"N/A (slatedb)"`.
///
/// Original C++ source: rdb_i_s.cc:443 — `rdb_i_s_cfoptions_fill_table`.
pub async fn fill_table(_ctx: FillCtx<'_>) -> Result<Vec<Row>, Error> {
    // TODO(human): finalise the canonical option-name list. The C++ enumerates
    // ~40 fields plus `TABLE_FACTORY::*` keys; mirror that list verbatim so
    // dashboards stay byte-stable, then append SlateDB-only options at the
    // tail (`FLUSH_INTERVAL`, `AWAIT_DURABLE`, `MIN_FILTER_KEYS`).
    todo!("for each cf in cf_table, push (cf, opt_name, opt_value_or_NA) rows")
}

pub const PLUGIN_NAME: &str = "ROCKSDB_CFOPTIONS";

/// Original C++ source: rdb_i_s.cc:1058 — `rdb_i_s_cfoptions_init`.
pub fn init(_plugin: *mut std::ffi::c_void) -> Result<(), Error> {
    todo!("wire fields_info + sync wrapper into the ST_SCHEMA_TABLE")
}
