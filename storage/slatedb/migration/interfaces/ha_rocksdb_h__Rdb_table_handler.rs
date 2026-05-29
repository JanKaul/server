//! Interface stub for `ha_rocksdb_h__Rdb_table_handler`.
//!
//! C++ source: `storage/rocksdb/ha_rocksdb.h` (lines 105..124, 20 LoC body)
//! v4 manifest sub-unit: `ha_rocksdb_h__Rdb_table_handler`
//! parent: `ha_rocksdb_h`
//!
//! ## Mapping
//! Per-open-table reference-counted record. The MyRocks original holds:
//! - Table name + ref-count for the global hash of open tables
//! - `THR_LOCK` (MySQL latch protecting `m_db_lock`)
//! - I/O perf counters (split into read/write)
//! - Memtable estimate cache (RocksDB-specific)
//!
//! Per _DESIGN.md §1 ("rdb_perf_context: Re-impl"), the per-CF perf counters
//! and the memtable cache are replaced by SlateDB metrics + `DbStatus`. The
//! ref-counting + name + `THR_LOCK` stay; the perf-counter fields point at our
//! re-implemented metrics.
//!
//! ## Out-of-scope fields
//! - `m_mtcache_*`: RocksDB memtable estimate. SlateDB exposes equivalent via
//!   `DbStatus`/`VersionedManifest`; the local cache is unnecessary.

use slatedb::Error;

use crate::atomic_stat_h::{AtomicStatI64, AtomicStatU64};

/// Reference-counted per-open-table record. One entry per unique table name
/// currently open across all connections; stored in a global hash map
/// (see `Rdb_open_tables_map` for the container).
///
/// Lifecycle:
/// - `Db::create()` (`engine/db.rs` handler-side) increments `ref_count` on
///   `handler::open` and decrements on `handler::close`.
/// - When `ref_count` reaches 0, the entry is evicted from the global map.
pub struct TableHandler {
    /// Normalized table name (`schema/table`). Used as the global-hash key.
    /// Original: ha_rocksdb.h:106 — `char *m_table_name`.
    pub table_name: String,

    /// Open-handle ref count.
    /// Original: ha_rocksdb.h:108 — `int m_ref_count`.
    pub ref_count: std::sync::atomic::AtomicI64,

    /// Cumulative lock-wait timeouts attributed to this table.
    /// Original: ha_rocksdb.h:109.
    pub lock_wait_timeout_counter: AtomicStatI64,

    /// Cumulative deadlocks attributed to this table.
    /// Original: ha_rocksdb.h:110.
    pub deadlock_counter: AtomicStatI64,

    /// I/O perf counters (cumulative) for reads from this table.
    /// Original: ha_rocksdb.h:115 — `my_io_perf_atomic_t m_io_perf_read`.
    /// Backed by SlateDB metrics; see `engine/stats_task.rs`.
    pub io_perf_read: IoPerfCounters,

    /// I/O perf counters (cumulative) for writes to this table.
    /// Original: ha_rocksdb.h:116 — `my_io_perf_atomic_t m_io_perf_write`.
    pub io_perf_write: IoPerfCounters,
}

/// Counterpart of `my_io_perf_atomic_t` from `rdb_mariadb_port.h:25`.
/// Aggregates I/O bytes/requests/timing for one side (read or write).
pub struct IoPerfCounters {
    pub bytes: AtomicStatU64,
    pub requests: AtomicStatU64,
    pub svc_time: AtomicStatU64,
    pub svc_time_max: AtomicStatU64,
    pub wait_time: AtomicStatU64,
    pub wait_time_max: AtomicStatU64,
    pub slow_ios: AtomicStatU64,
}

impl TableHandler {
    /// Construct a new handle with ref_count=1.
    pub fn new(table_name: String) -> Self {
        todo!("zero-init counters; set ref_count=1")
    }

    /// Atomically increment ref_count. Returns the new count.
    pub fn acquire(&self) -> i64 {
        todo!("ref_count.fetch_add(1, AcqRel) + 1")
    }

    /// Atomically decrement ref_count. Returns the new count; caller evicts on 0.
    pub fn release(&self) -> i64 {
        todo!("ref_count.fetch_sub(1, AcqRel) - 1")
    }
}
