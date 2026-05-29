//! Interface stub for `rdb_i_s_cc__perf_context_global`.
//!
//! C++ source: `storage/rocksdb/rdb_i_s.cc` (lines 363..428, plug at 1816..1830)
//! Body LoC: ~66
//! v4 manifest sub-unit: `rdb_i_s_cc__perf_context_global`
//! Parent: `rdb_i_s_cc`
//!
//! ## Mapping
//! Per _DESIGN.md §1 row "`rdb_perf_context`" — **Re-implemented** against
//! `slatedb_common::metrics`. Same `PC_*` enumeration as `perf_context`, but
//! with no per-table labelling: we read the *unlabelled* (or `__total__`-summed)
//! values of the same metric set.
//!
//! Schema: `(STAT_TYPE, VALUE)` — identical to `dbstats`, but data source is
//! the perf-counter mapping in `crate::metrics::pc_map` rather than the
//! DbStats enum.
//!
//! ## Out-of-scope methods
//! None — counters without a SlateDB analogue surface as `VALUE = 0` rows so
//! `SELECT STAT_TYPE FROM rocksdb_perf_context_global` returns the full set
//! every MyRocks dashboard expects.

use crate::rdb_i_s_cc__shared::{Column, ColumnType, Nullable, Row};
use slatedb::Error;

/// Column layout: (STAT_TYPE, VALUE).
/// Original: rdb_i_s.cc:370 — `rdb_i_s_perf_context_global_fields_info[]`.
pub fn fields_info() -> &'static [Column] {
    static FIELDS: once_cell::sync::Lazy<Vec<Column>> = once_cell::sync::Lazy::new(|| {
        vec![
            Column { name: "STAT_TYPE", ty: ColumnType::Varchar(65), nullable: Nullable::NotNull },
            Column { name: "VALUE",     ty: ColumnType::SLonglong,   nullable: Nullable::NotNull },
        ]
    });
    &FIELDS
}

pub struct FillCtx<'a> {
    /// Marker — the bridge passes a reference to the engine for metric lookup.
    /// We don't capture metric handles here to keep the stub layer dependency-free.
    pub _engine: &'a (),
}

/// One pre-resolved (stat, value) pair. The bridge wrapper does the metric
/// lookup before calling `fill_table` so the async fn body stays free of
/// blocking calls into `slatedb_common::metrics` snapshots.
#[derive(Debug, Clone)]
pub struct GlobalCounter {
    pub stat_name: &'static str,
    pub value: i64,
}

/// Convenience constructor used by the bridge wrapper.
pub fn pair(stat_name: &'static str, value: i64) -> GlobalCounter {
    GlobalCounter { stat_name, value }
}

/// Build the rowset for `information_schema.ROCKSDB_PERF_CONTEXT_GLOBAL`.
///
/// Original C++ source: rdb_i_s.cc:375 — `rdb_i_s_perf_context_global_fill_table`.
pub async fn fill_table(_ctx: FillCtx<'_>) -> Result<Vec<Row>, Error> {
    // TODO(human): same `PC_*` → SlateDB metric mapping as perf_context, but
    // reading the *unlabelled* sum. Lives in `crate::metrics::pc_map`.
    todo!("for each pc_stat_type, push (stat_name, global_value) row")
}

pub const PLUGIN_NAME: &str = "ROCKSDB_PERF_CONTEXT_GLOBAL";

/// Original C++ source: rdb_i_s.cc:415 — `rdb_i_s_perf_context_global_init`.
pub fn init(_plugin: *mut std::ffi::c_void) -> Result<(), Error> {
    todo!("wire fields_info + sync wrapper into the ST_SCHEMA_TABLE")
}
