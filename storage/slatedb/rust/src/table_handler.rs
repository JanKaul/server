//! Per-open-table refcounted record.
//!
//! Translated from `ha_rocksdb.h:105..124`. One entry per unique table
//! name currently open across all connections; the engine keeps a global
//! map (`Rdb_open_tables_map` in MyRocks, ported separately) keyed by
//! `table_name`. Handler `open` increments the count, handler `close`
//! decrements; the map evicts entries when the count reaches zero.
//!
//! Per `_DESIGN.md §1`:
//! - The per-CF memtable estimate (`m_mtcache_*` in the C++) is dropped
//!   — SlateDB exposes equivalent state via `DbStatus`/`VersionedManifest`,
//!   so the local cache is redundant.
//! - The MyRocks `THR_LOCK` is dropped — SlateDB transactions run
//!   independently of MariaDB's table-level latch, and the SSI commit
//!   path (`engine::txn`) is the actual serialisation point.
//! - Per-table I/O perf counters are preserved (the SHOW STATUS surface
//!   exposes them). We reuse [`crate::utils::mariadb_port::IoPerfAtomic`]
//!   instead of duplicating its shape.

use std::sync::atomic::{AtomicI64, Ordering};

use crate::utils::atomic_stat::AtomicStatI64;
use crate::utils::mariadb_port::IoPerfAtomic;

/// Refcounted per-open-table record.
///
/// All counters are relaxed — they back monitoring surfaces, not
/// correctness-critical state. Ref-count updates use `AcqRel` so a
/// release that drops to zero happens-before the eviction observed by
/// the next acquirer.
pub struct TableHandler {
    /// Normalised table name (`schema/table`). The global-map key.
    pub table_name: String,

    /// Open-handle reference count. Eviction policy: when this reaches
    /// zero the entry is removed from the map.
    pub ref_count: AtomicI64,

    /// Cumulative lock-wait timeouts attributed to this table.
    pub lock_wait_timeout_counter: AtomicStatI64,

    /// Cumulative deadlocks attributed to this table.
    pub deadlock_counter: AtomicStatI64,

    /// Per-table I/O perf counters split by direction.
    pub io_perf_read: IoPerfAtomic,
    pub io_perf_write: IoPerfAtomic,
}

impl TableHandler {
    /// Construct a fresh handle. Initial `ref_count` is **1** so the
    /// creator already holds a reference — they must call
    /// [`release`](Self::release) (or drop the entry from the map) when
    /// done.
    pub fn new(table_name: impl Into<String>) -> Self {
        Self {
            table_name: table_name.into(),
            ref_count: AtomicI64::new(1),
            lock_wait_timeout_counter: AtomicStatI64::new(),
            deadlock_counter: AtomicStatI64::new(),
            io_perf_read: IoPerfAtomic::new(),
            io_perf_write: IoPerfAtomic::new(),
        }
    }

    /// Atomically increment `ref_count`. Returns the new count.
    pub fn acquire(&self) -> i64 {
        self.ref_count.fetch_add(1, Ordering::AcqRel) + 1
    }

    /// Atomically decrement `ref_count`. Returns the new count; caller
    /// is responsible for evicting from the map when the count reaches
    /// zero. Calling `release` past zero is a programmer bug — the
    /// returned negative count surfaces it loudly.
    pub fn release(&self) -> i64 {
        self.ref_count.fetch_sub(1, Ordering::AcqRel) - 1
    }

    /// Read the current ref-count (relaxed).
    pub fn ref_count(&self) -> i64 {
        self.ref_count.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_starts_at_ref_count_one() {
        let h = TableHandler::new("db/users");
        assert_eq!(h.table_name, "db/users");
        assert_eq!(h.ref_count(), 1);
    }

    #[test]
    fn counters_start_at_zero() {
        let h = TableHandler::new("db/t");
        assert_eq!(h.lock_wait_timeout_counter.load(), 0);
        assert_eq!(h.deadlock_counter.load(), 0);
        assert_eq!(h.io_perf_read.bytes.load(), 0);
        assert_eq!(h.io_perf_write.requests.load(), 0);
    }

    #[test]
    fn acquire_and_release_track_ref_count() {
        let h = TableHandler::new("db/t");
        assert_eq!(h.acquire(), 2);
        assert_eq!(h.acquire(), 3);
        assert_eq!(h.release(), 2);
        assert_eq!(h.release(), 1);
        assert_eq!(h.release(), 0);
        // Caller would evict here.
    }

    #[test]
    fn release_past_zero_returns_negative_count_to_surface_bug() {
        let h = TableHandler::new("db/t");
        h.release(); // 0
        let after = h.release(); // -1
        assert_eq!(after, -1, "going negative signals a programmer bug");
    }

    #[test]
    fn acquire_release_is_thread_safe() {
        use std::sync::Arc;
        let h = Arc::new(TableHandler::new("db/t"));
        // Drop to zero so we have a clean baseline.
        h.release();
        let mut threads = Vec::new();
        for _ in 0..8 {
            let h = Arc::clone(&h);
            threads.push(std::thread::spawn(move || {
                for _ in 0..1000 {
                    h.acquire();
                    h.release();
                }
            }));
        }
        for t in threads {
            let _ = t.join();
        }
        assert_eq!(h.ref_count(), 0);
    }
}
