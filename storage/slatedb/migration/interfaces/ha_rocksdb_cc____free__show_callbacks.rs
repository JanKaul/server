//! Interface stub for `ha_rocksdb_cc____free__show_callbacks`.
//!
//! C++ source: `storage/rocksdb/ha_rocksdb.cc` (sub-unit span 4613..13483)
//! v4 manifest sub-unit: `ha_rocksdb_cc____free__show_callbacks`
//!
//! ## Mapping
//! Per _DESIGN.md §1 row "rdb_perf_context (perf counters → Re-impl)":
//! these are the `SHOW STATUS LIKE 'rocksdb_%'` getter callbacks. In MyRocks
//! each is generated via the `DEF_SHOW_FUNC(name, key)` macro
//! (`ha_rocksdb.cc:13148`) which copies `rocksdb_stats->getTickerCount(key)`
//! into a `static rocksdb_status_counters_t` struct and points the SHOW_VAR
//! at it.
//!
//! In Rust, each callback returns a single numeric counter pulled from the
//! SlateDB metrics surface (`slatedb_common::metrics`, see _DESIGN.md §0).
//! The shape of the counters differs from RocksDB's; mapping is done in
//! `engine/stats.rs` (TRANSLATE phase). Counters with no SlateDB analogue
//! return 0 — they remain visible in SHOW STATUS for compatibility but
//! always read zero.
//!
//! ## Out-of-scope methods
//! None — all show callbacks are in scope as numeric getters. Several map
//! to 0 because SlateDB has no equivalent counter:
//!   - `block_cachecompressed_{miss,hit}` — SlateDB block cache (foyer/moka)
//!     does not expose compressed-block tier stats separately.
//!   - `number_superversion_{acquires,releases,cleanups}` — internal
//!     RocksDB concept, no SlateDB equivalent.
//!   - `bloom_filter_full_{positive,true_positive}` — filter-policy
//!     accounting in SlateDB does not split positives this way.
//!   - `wal_synced` is reported as the SlateDB `wal_fsync_total` counter.

use slatedb::Error;

/// Marker trait for the SHOW STATUS getter callback shape.
///
/// In C++ each is `int (*)(THD*, SHOW_VAR*, void* buff, status_var*, enum_var_type)`
/// returning the counter via the `var->value` pointer. The Rust equivalent
/// returns the value directly; the cxx bridge layer wraps each into the
/// MariaDB callback ABI.
pub type ShowU64 = fn() -> u64;
pub type ShowI64 = fn() -> i64;
pub type ShowF64 = fn() -> f64;

// -----------------------------------------------------------------------
// Block-cache counters (foyer/moka backed; per _DESIGN.md §0 block cache)
// -----------------------------------------------------------------------

/// SlateDB `block_cache.miss_count`. Original: ha_rocksdb.cc:13245.
pub fn show_block_cache_miss() -> u64 { todo!("read block_cache miss from slatedb metrics") }
/// SlateDB `block_cache.hit_count`. Original: ha_rocksdb.cc:13246.
pub fn show_block_cache_hit() -> u64 { todo!("read block_cache hit from slatedb metrics") }
/// SlateDB `block_cache.insert_count`. Original: ha_rocksdb.cc:13247.
pub fn show_block_cache_add() -> u64 { todo!("read block_cache insert from slatedb metrics") }
/// SlateDB `block_cache.insert_failure_count`. Original: ha_rocksdb.cc:13248.
pub fn show_block_cache_add_failures() -> u64 { todo!("metric") }

// Index-tier block cache (SlateDB does not split index vs filter vs data tiers).
// Returned as the overall counters; see _DESIGN.md §1 "Block cache → degraded".
pub fn show_block_cache_index_miss() -> u64 { todo!("returns 0 — no per-tier breakdown in SlateDB") }
pub fn show_block_cache_index_hit() -> u64 { todo!("returns 0 — no per-tier breakdown in SlateDB") }
pub fn show_block_cache_index_add() -> u64 { todo!("returns 0 — no per-tier breakdown in SlateDB") }
pub fn show_block_cache_index_bytes_insert() -> u64 { todo!("returns 0") }
pub fn show_block_cache_index_bytes_evict() -> u64 { todo!("returns 0") }
pub fn show_block_cache_filter_miss() -> u64 { todo!("returns 0") }
pub fn show_block_cache_filter_hit() -> u64 { todo!("returns 0") }
pub fn show_block_cache_filter_add() -> u64 { todo!("returns 0") }
pub fn show_block_cache_filter_bytes_insert() -> u64 { todo!("returns 0") }
pub fn show_block_cache_filter_bytes_evict() -> u64 { todo!("returns 0") }
pub fn show_block_cache_bytes_read() -> u64 { todo!("metric: block_cache.bytes_read") }
pub fn show_block_cache_bytes_write() -> u64 { todo!("metric: block_cache.bytes_written") }
pub fn show_block_cache_data_bytes_insert() -> u64 { todo!("metric: block_cache.data_bytes_insert") }
pub fn show_block_cache_data_miss() -> u64 { todo!("metric") }
pub fn show_block_cache_data_hit() -> u64 { todo!("metric") }
pub fn show_block_cache_data_add() -> u64 { todo!("metric") }

// -----------------------------------------------------------------------
// Bloom filter counters
// -----------------------------------------------------------------------

pub fn show_bloom_filter_useful() -> u64 { todo!("metric: filter.useful_total") }
pub fn show_bloom_filter_full_positive() -> u64 {
    todo!("returns 0 — SlateDB FilterPolicy does not split positives this way")
}
pub fn show_bloom_filter_full_true_positive() -> u64 { todo!("returns 0") }
pub fn show_bloom_filter_prefix_checked() -> u64 { todo!("metric: filter.prefix_checked") }
pub fn show_bloom_filter_prefix_useful() -> u64 { todo!("metric: filter.prefix_useful") }

// -----------------------------------------------------------------------
// Memtable / level-hit counters
// -----------------------------------------------------------------------

pub fn show_memtable_hit() -> u64 { todo!("metric: memtable.hit_count") }
pub fn show_memtable_miss() -> u64 { todo!("metric: memtable.miss_count") }
pub fn show_get_hit_l0() -> u64 { todo!("metric — SlateDB has L0/L1/L2 SST tier accounting") }
pub fn show_get_hit_l1() -> u64 { todo!("metric") }
pub fn show_get_hit_l2_and_up() -> u64 { todo!("metric") }

// -----------------------------------------------------------------------
// Compaction key-drop counters
// -----------------------------------------------------------------------

pub fn show_compaction_key_drop_new() -> u64 { todo!("metric: compaction.key_drop_newer_entry") }
pub fn show_compaction_key_drop_obsolete() -> u64 { todo!("metric: compaction.key_drop_obsolete") }
pub fn show_compaction_key_drop_user() -> u64 {
    todo!("metric: compaction.key_drop_user — from our CompactionFilter")
}

// -----------------------------------------------------------------------
// Row / byte throughput
// -----------------------------------------------------------------------

pub fn show_number_keys_written() -> u64 { todo!("metric: db.put_count + db.merge_count") }
pub fn show_number_keys_read() -> u64 { todo!("metric: db.get_count") }
pub fn show_number_keys_updated() -> u64 { todo!("metric: db.merge_count") }
pub fn show_bytes_written() -> u64 { todo!("metric: db.bytes_written") }
pub fn show_bytes_read() -> u64 { todo!("metric: db.bytes_read") }

// -----------------------------------------------------------------------
// Iterator-side counters
// -----------------------------------------------------------------------

pub fn show_number_db_seek() -> u64 { todo!("metric: iter.seek_count") }
pub fn show_number_db_seek_found() -> u64 { todo!("metric") }
pub fn show_number_db_next() -> u64 { todo!("metric") }
pub fn show_number_db_next_found() -> u64 { todo!("metric") }
pub fn show_number_db_prev() -> u64 { todo!("metric: iter.prev_count (Descending order)") }
pub fn show_number_db_prev_found() -> u64 { todo!("metric") }
pub fn show_iter_bytes_read() -> u64 { todo!("metric: iter.bytes_read") }
pub fn show_num_iterators() -> u64 { todo!("metric: iter.live_count (gauge)") }
pub fn show_number_reseeks_iteration() -> u64 { todo!("metric: iter.reseek_count") }

// -----------------------------------------------------------------------
// File-handle counters
// -----------------------------------------------------------------------

pub fn show_no_file_closes() -> u64 { todo!("metric: object_store.close_count") }
pub fn show_no_file_opens() -> u64 { todo!("metric: object_store.open_count") }
pub fn show_no_file_errors() -> u64 { todo!("metric: object_store.error_count") }

// -----------------------------------------------------------------------
// Stall / throttle counters
// -----------------------------------------------------------------------

pub fn show_stall_micros() -> u64 { todo!("metric: db.stall_micros_total") }

// -----------------------------------------------------------------------
// MultiGet counters — SlateDB has no batch-get API; report 0
// -----------------------------------------------------------------------

pub fn show_number_multiget_get() -> u64 { todo!("returns 0 — SlateDB has no MultiGet") }
pub fn show_number_multiget_keys_read() -> u64 { todo!("returns 0") }
pub fn show_number_multiget_bytes_read() -> u64 { todo!("returns 0") }

// -----------------------------------------------------------------------
// Tombstone / merge counters
// -----------------------------------------------------------------------

pub fn show_number_deletes_filtered() -> u64 { todo!("metric: deletes_filtered_by_compaction") }
pub fn show_number_merge_failures() -> u64 { todo!("metric: merge_failure_count") }
pub fn show_getupdatessince_calls() -> u64 {
    todo!("returns 0 — SlateDB has no GetUpdatesSince (replication API)")
}

// -----------------------------------------------------------------------
// Compressed-block tier — not in SlateDB
// -----------------------------------------------------------------------

pub fn show_block_cachecompressed_miss() -> u64 { todo!("returns 0 — no compressed tier") }
pub fn show_block_cachecompressed_hit() -> u64 { todo!("returns 0 — no compressed tier") }

// -----------------------------------------------------------------------
// WAL counters
// -----------------------------------------------------------------------

pub fn show_wal_synced() -> u64 { todo!("metric: wal.fsync_count") }
pub fn show_wal_bytes() -> u64 { todo!("metric: wal.bytes_written") }

// -----------------------------------------------------------------------
// Write-path counters
// -----------------------------------------------------------------------

pub fn show_write_self() -> u64 { todo!("metric: write.self_count") }
pub fn show_write_other() -> u64 { todo!("metric: write.other_count") }
pub fn show_write_timedout() -> u64 { todo!("metric: write.timeout_count") }
pub fn show_write_wal() -> u64 { todo!("metric: write.wal_count") }

// -----------------------------------------------------------------------
// Flush / compaction byte counters
// -----------------------------------------------------------------------

pub fn show_flush_write_bytes() -> u64 { todo!("metric: flush.bytes_written") }
pub fn show_compact_read_bytes() -> u64 { todo!("metric: compaction.bytes_read") }
pub fn show_compact_write_bytes() -> u64 { todo!("metric: compaction.bytes_written") }

// -----------------------------------------------------------------------
// Superversion counters (RocksDB-specific) — report 0
// -----------------------------------------------------------------------

pub fn show_number_superversion_acquires() -> u64 { todo!("returns 0 — RocksDB-specific concept") }
pub fn show_number_superversion_releases() -> u64 { todo!("returns 0") }
pub fn show_number_superversion_cleanups() -> u64 { todo!("returns 0") }
pub fn show_number_block_not_compressed() -> u64 { todo!("metric: compression.block_skipped_count") }

// -----------------------------------------------------------------------
// Aggregate SHOW STATUS handler (handlerton-level)
// -----------------------------------------------------------------------

/// Equivalent of `rocksdb_show_status` (ha_rocksdb.cc:4613) — the handlerton
/// `show_status` callback. Writes one or more `stat_print_fn` entries
/// describing the engine's overall state. The Rust side returns the body of
/// the report; the cxx bridge layer hands it to MariaDB.
///
/// On error: returns `slatedb::ErrorKind::Unavailable` (a transient read of
/// `DbStatus` failed); the bridge translates to MariaDB's error reporting.
pub fn show_status() -> Result<String, Error> {
    todo!("snapshot DbStatus + per-CF metrics, render as multi-section text")
}

// -----------------------------------------------------------------------
// Update + aggregate hooks (mirror the C++ `myrocks_update_status` flow)
// -----------------------------------------------------------------------

/// Refresh the per-process row counters (`ExportStats`) from the sharded
/// `GlobalStats` accumulator. Called by the SHOW STATUS dispatcher before
/// reading the per-row `rocksdb_rows_*` variables.
/// Original: ha_rocksdb.cc:13318 — `myrocks_update_status`.
pub fn update_status() {
    todo!("snapshot GlobalStats → ExportStats (rdb_global_h.rs)")
}

/// Refresh the memtable byte gauge.
/// Original: ha_rocksdb.cc:13339 — `myrocks_update_memory_status`.
pub fn update_memory_status() {
    todo!("read memtable_total + memtable_unflushed from slatedb DbStatus into MemoryStats")
}

/// Aggregate SHOW VAR callback. Original: ha_rocksdb.cc:13388 — `show_myrocks_vars`.
/// Calls `update_status` then writes the per-row counters as a `SHOW_ARRAY`.
pub fn show_myrocks_vars() -> Result<(), Error> {
    todo!("update_status(); fill caller's SHOW_VAR array from ExportStats")
}

/// Stall-status sub-getters used by `show_rocksdb_stall_vars`.
/// Original: ha_rocksdb.cc:13398 — `io_stall_prop_value`.
pub fn io_stall_prop_value(prop_name: &str) -> u64 {
    let _ = prop_name;
    todo!("look up the named stall-status property in slatedb metrics")
}

/// Refresh the per-stall-type counters (`IoStallStats`).
/// Original: ha_rocksdb.cc:13412 — `update_rocksdb_stall_status`.
pub fn update_rocksdb_stall_status() {
    todo!("populate IoStallStats from slatedb metrics; fields with no analogue → 0")
}

/// Aggregate stall SHOW VAR callback.
/// Original: ha_rocksdb.cc:13476 — `show_rocksdb_stall_vars`.
pub fn show_rocksdb_stall_vars() -> Result<(), Error> {
    todo!("update_rocksdb_stall_status(); fill caller's SHOW_VAR array from IoStallStats")
}
