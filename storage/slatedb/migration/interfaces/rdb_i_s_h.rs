//! Interface stub for `rdb_i_s_h`.
//!
//! C++ source: `storage/rocksdb/rdb_i_s.h` (37 LoC)
//!
//! ## Mapping
//! Forward declarations of the 13 `information_schema` plugin descriptors
//! exported by `rdb_i_s.cc`. Per _DESIGN.md §1, all 13 I_S tables are
//! re-implemented (see `rdb_i_s_cc__*.rs` files).
//!
//! ## Out-of-scope methods
//! None — pure forward decls; the actual plugin registration happens in
//! each per-table file via `init()`.

// The 13 I_S tables. Each per-table unit declares its own `PLUGIN_NAME`
// const and `init()` fn; this header just enumerates them for the lifecycle
// code (`ha_rocksdb_cc____free__lifecycle.rs`) that registers them all
// in `rocksdb_init_func`.

pub const I_S_TABLES: &[&str] = &[
    "ROCKSDB_CF_STATS",
    "ROCKSDB_DBSTATS",
    "ROCKSDB_PERF_CONTEXT",
    "ROCKSDB_PERF_CONTEXT_GLOBAL",
    "ROCKSDB_CF_OPTIONS",
    "ROCKSDB_GLOBAL_INFO",
    "ROCKSDB_COMPACTION_STATS",
    "ROCKSDB_DDL",
    "ROCKSDB_SST_PROPS",
    "ROCKSDB_INDEX_FILE_MAP",
    "ROCKSDB_LOCKS",
    "ROCKSDB_TRX",
    "ROCKSDB_DEADLOCK",
];
