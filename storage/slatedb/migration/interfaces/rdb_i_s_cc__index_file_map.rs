//! Interface stub for `rdb_i_s_cc__index_file_map`.
//!
//! C++ source: `storage/rocksdb/rdb_i_s.cc` (lines 1280..1438, plug at 1912..1926)
//! Body LoC: ~159
//! v4 manifest sub-unit: `rdb_i_s_cc__index_file_map`
//! Parent: `rdb_i_s_cc`
//!
//! ## Mapping
//! Per _DESIGN.md §1 — **Re-implemented**.
//!
//! MyRocks reads `Rdb_index_stats` blobs stored in each SST's table-property
//! map (key `__indexstats__`) — those are populated by
//! `Rdb_tbl_prop_coll`, which RocksDB calls during flush/compaction. SlateDB
//! has no equivalent "table property collector" API, so we derive index→SST
//! mapping by **scanning each SST's `min_key`/`max_key` from `VersionedManifest`
//! and decoding the `(cf_id, index_id)` prefix per key boundary**:
//!
//! - For each SST entry in `manifest.l0` ∪ `manifest.compacted`:
//!     - Decode `cf_id`/`index_id` from `min_key` and `max_key`.
//!     - If both share the same prefix → emit one row for that index.
//!     - If they differ → emit a row per `(cf_id, index_id)` overlapping the
//!       range (rare for our key-prefix layout because each SlateDB SST tends
//!       to span one index in practice).
//!
//! Per-row counters (`NUM_ROWS`, `DATA_SIZE`, `ENTRY_DELETES`,
//! `ENTRY_SINGLEDELETES`, `ENTRY_MERGES`, `ENTRY_OTHERS`) come from the
//! `Rdb_index_stats` cache our `StatsRefreshTask` (from
//! `event_listener_h.rs`) maintains — it walks the same SSTs on every
//! `DbStatus` change and aggregates by `(cf_id, index_id)`. The
//! `DISTINCT_KEYS_PREFIX` column is populated from that cache too.
//!
//! ## Out-of-scope methods
//! None — schema preserved.

use crate::rdb_i_s_cc__shared::{Column, ColumnType, Nullable, Row};
use slatedb::Error;

/// Column layout: 10 fields.
/// Original: rdb_i_s.cc:1298 — `rdb_i_s_index_file_map_fields_info[]`.
pub fn fields_info() -> &'static [Column] {
    static FIELDS: once_cell::sync::Lazy<Vec<Column>> = once_cell::sync::Lazy::new(|| {
        vec![
            Column { name: "COLUMN_FAMILY",        ty: ColumnType::SLong,        nullable: Nullable::NotNull },
            Column { name: "INDEX_NUMBER",         ty: ColumnType::SLong,        nullable: Nullable::NotNull },
            Column { name: "SST_NAME",             ty: ColumnType::Varchar(65),  nullable: Nullable::NotNull },
            Column { name: "NUM_ROWS",             ty: ColumnType::SLonglong,    nullable: Nullable::NotNull },
            Column { name: "DATA_SIZE",            ty: ColumnType::SLonglong,    nullable: Nullable::NotNull },
            Column { name: "ENTRY_DELETES",        ty: ColumnType::SLonglong,    nullable: Nullable::NotNull },
            Column { name: "ENTRY_SINGLEDELETES",  ty: ColumnType::SLonglong,    nullable: Nullable::NotNull },
            Column { name: "ENTRY_MERGES",         ty: ColumnType::SLonglong,    nullable: Nullable::NotNull },
            Column { name: "ENTRY_OTHERS",         ty: ColumnType::SLonglong,    nullable: Nullable::NotNull },
            Column { name: "DISTINCT_KEYS_PREFIX", ty: ColumnType::Varchar(625), nullable: Nullable::NotNull },
        ]
    });
    &FIELDS
}

/// One pre-aggregated row from the `StatsRefreshTask` cache, keyed by
/// `(cf_id, index_id, sst_name)`.
#[derive(Debug, Clone, Default)]
pub struct IndexSstStats {
    pub cf_id: i32,
    pub index_id: i32,
    pub sst_name: String,
    pub num_rows: i64,
    pub data_size: i64,
    pub entry_deletes: i64,
    pub entry_single_deletes: i64,
    pub entry_merges: i64,
    pub entry_others: i64,
    /// Comma-separated list of distinct-key counts per prefix length.
    pub distinct_keys_prefix: String,
}

pub struct FillCtx<'a> {
    pub stats: &'a [IndexSstStats],
}

/// Build the rowset for `information_schema.ROCKSDB_INDEX_FILE_MAP`.
///
/// When an SST has no decoded index stats we emit a sentinel row with all
/// numeric fields = `-1`, matching the C++ behaviour at rdb_i_s.cc:1364.
///
/// Original C++ source: rdb_i_s.cc:1319 — `rdb_i_s_index_file_map_fill_table`.
pub async fn fill_table(_ctx: FillCtx<'_>) -> Result<Vec<Row>, Error> {
    todo!("map each IndexSstStats → 10-column Row")
}

pub const PLUGIN_NAME: &str = "ROCKSDB_INDEX_FILE_MAP";

/// Original C++ source: rdb_i_s.cc:1424 — `rdb_i_s_index_file_map_init`.
pub fn init(_plugin: *mut std::ffi::c_void) -> Result<(), Error> {
    todo!("wire fields_info + sync wrapper into the ST_SCHEMA_TABLE")
}
