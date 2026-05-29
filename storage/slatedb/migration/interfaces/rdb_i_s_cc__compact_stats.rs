//! Interface stub for `rdb_i_s_cc__compact_stats`.
//!
//! C++ source: `storage/rocksdb/rdb_i_s.cc` (lines 837..907, plug at 1864..1878)
//! Body LoC: ~71
//! v4 manifest sub-unit: `rdb_i_s_cc__compact_stats`
//! Parent: `rdb_i_s_cc`
//!
//! ## Mapping
//! Per _DESIGN.md §1 — **Re-implemented**.
//!
//! MyRocks reads `DB::GetMapProperty(cfh, "rocksdb.cfstats", &props)` and
//! filters keys with the `"compaction."` prefix, parsing each as
//! `compaction.<level>.<type> = <double>`. SlateDB has no leveled compaction
//! and no `cfstats` map property; we substitute the **compactor state in
//! `VersionedManifest`** (`compactor_state.compacted_sst_ids`,
//! `compactor_state.pending_compaction_runs`, etc.) and synthesise the same
//! `(CF_NAME, LEVEL, TYPE, VALUE)` rows where the concept maps:
//!
//! - `LEVEL` becomes one of: `"L0"` (memtable→SST flushes), `"COMPACT"`
//!   (compactor merges), `"Sum"` (totals). MyRocks emits `"L0"`..`"Ln"`;
//!   SlateDB's tiered design collapses to these three.
//! - `TYPE` covers `NumFiles`, `SizeBytes`, `BytesIn`, `BytesOut` — derivable
//!   from manifest SST entries and compactor counters.
//! - Other RocksDB `TYPE`s (e.g. `WriteAmp`, `ReadAmp`) emit `VALUE = 0.0`
//!   if not measurable.
//!
//! ## Out-of-scope methods
//! None — schema preserved; gaps surface as zero rows.

use crate::rdb_i_s_cc__shared::{Column, ColumnType, Nullable, Row};
use slatedb::Error;

/// Column layout: (CF_NAME, LEVEL, TYPE, VALUE).
/// Original: rdb_i_s.cc:902 — `rdb_i_s_compact_stats_fields_info[]`.
pub fn fields_info() -> &'static [Column] {
    static FIELDS: once_cell::sync::Lazy<Vec<Column>> = once_cell::sync::Lazy::new(|| {
        vec![
            Column { name: "CF_NAME", ty: ColumnType::Varchar(65),  nullable: Nullable::NotNull },
            Column { name: "LEVEL",   ty: ColumnType::Varchar(513), nullable: Nullable::NotNull },
            Column { name: "TYPE",    ty: ColumnType::Varchar(513), nullable: Nullable::NotNull },
            Column { name: "VALUE",   ty: ColumnType::Double(20),   nullable: Nullable::NotNull },
        ]
    });
    &FIELDS
}

pub struct FillCtx<'a> {
    pub manifest: &'a slatedb::config::VersionedManifest,
    pub cf_table: &'a [(u32, String)],
}

/// Build the rowset for `information_schema.ROCKSDB_COMPACTION_STATS`.
///
/// Original C++ source: rdb_i_s.cc:840 — `rdb_i_s_compact_stats_fill_table`.
pub async fn fill_table(_ctx: FillCtx<'_>) -> Result<Vec<Row>, Error> {
    // TODO(human): pin down the exact set of `TYPE` strings to emit. Start with
    // {NumFiles, SizeBytes, BytesIn, BytesOut, CompactCount} per level
    // {L0, COMPACT, Sum} — see manifest `compactor_state` for the source fields.
    todo!("derive compaction.<level>.<type> rows from manifest.compactor_state")
}

pub const PLUGIN_NAME: &str = "ROCKSDB_COMPACTION_STATS";

/// Original C++ source: rdb_i_s.cc:1088 — `rdb_i_s_compact_stats_init`.
pub fn init(_plugin: *mut std::ffi::c_void) -> Result<(), Error> {
    todo!("wire fields_info + sync wrapper into the ST_SCHEMA_TABLE")
}
