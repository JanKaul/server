//! Interface stub for `rdb_perf_context_h`.
//!
//! C++ source: `storage/rocksdb/rdb_perf_context.h` (168 LoC)
//! C++ classes: `Rdb_perf_counters`, `Rdb_atomic_perf_counters`, `Rdb_io_perf`
//!
//! ## Mapping
//! Per _DESIGN.md §1 (`rdb_perf_context: Re-impl`): RocksDB's `PerfContext` is
//! a thread-local counter struct populated as RocksDB executes work. MyRocks
//! samples it on every handler entry point and computes deltas. SlateDB
//! doesn't expose a direct equivalent; instead it ships **Prometheus-style
//! metrics** via `slatedb_common::metrics` and the per-request `WriteHandle`
//! futures expose latencies on completion.
//!
//! We re-shape the surface:
//!
//! - **`PC_*` enum constants** — kept 1:1 so the SHOW STATUS variable names
//!   (`rocksdb_block_cache_hit_count` etc.) survive. Slots that have no
//!   SlateDB analogue (e.g., `PC_BLOOM_MEMTABLE_HIT_COUNT` — SlateDB has
//!   bloom filters only at the SST level) report `0` with a doc comment.
//!
//! - **`PerfCounters`** — a snapshot struct. Populated by `from_slatedb_metrics()`
//!   instead of by RocksDB's `PerfContext::Get`.
//!
//! - **`AtomicPerfCounters`** — global aggregates. Implemented as
//!   `[AtomicU64; PC_MAX_IDX]`.
//!
//! - **`IoPerf`** — RAII timer that, on `end_and_record`, attributes the
//!   measured duration into the right `AtomicPerfCounters` slots. We keep
//!   the same start/end API but the implementation samples
//!   `Instant::now()` + reads from SlateDB's metric registry.
//!
//! ## Out-of-scope methods
//! - Some `PC_*` slots map to RocksDB internals (`PC_DB_MUTEX_LOCK_NANOS`,
//!   `PC_NEW_TABLE_BLOCK_ITER_NANOS`, etc.) with no SlateDB equivalent.
//!   These read `0`; documented at the variant.

use slatedb::Error;
use std::sync::atomic::{AtomicU64, Ordering};

/// Counter identifiers — 1:1 with the C++ `PC_*` enum so SHOW STATUS labels
/// don't change. Numeric values must stay aligned with the C++ side because
/// the bridge layer indexes the same array.
///
/// Variants flagged ❌ have no SlateDB analogue and always read 0.
#[repr(usize)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PerfCounter {
    UserKeyComparisonCount = 0,
    BlockCacheHitCount = 1,
    BlockReadCount = 2,
    BlockReadByte = 3,
    BlockReadTime = 4,
    BlockChecksumTime = 5,
    BlockDecompressTime = 6,
    GetReadBytes = 7,
    MultigetReadBytes = 8,
    IterReadBytes = 9,
    KeySkipped = 10,
    DeleteSkipped = 11,
    RecentSkipped = 12,
    Merge = 13,
    GetSnapshotTime = 14,
    GetFromMemtableTime = 15,
    GetFromMemtableCount = 16,
    GetPostProcessTime = 17,
    GetFromOutputFilesTime = 18,
    SeekOnMemtableTime = 19,
    SeekOnMemtableCount = 20,
    NextOnMemtableCount = 21,
    PrevOnMemtableCount = 22,
    SeekChildSeekTime = 23,
    SeekChildSeekCount = 24,
    SeekMinHeapTime = 25,
    SeekMaxHeapTime = 26,
    SeekInternalSeekTime = 27,
    FindNextUserEntryTime = 28,
    WriteWalTime = 29,
    WriteMemtableTime = 30,
    WriteDelayTime = 31,
    WritePreAndPostProcessTime = 32,
    /// ❌ no SlateDB analogue (it doesn't expose a single "db mutex").
    DbMutexLockNanos = 33,
    /// ❌ no SlateDB analogue.
    DbConditionWaitNanos = 34,
    MergeOperatorTimeNanos = 35,
    /// ❌ no SlateDB analogue (per-block timing not exposed).
    ReadIndexBlockNanos = 36,
    /// ❌ no SlateDB analogue.
    ReadFilterBlockNanos = 37,
    /// ❌ no SlateDB analogue.
    NewTableBlockIterNanos = 38,
    /// ❌ no SlateDB analogue.
    NewTableIteratorNanos = 39,
    /// ❌ no SlateDB analogue.
    BlockSeekNanos = 40,
    /// ❌ no SlateDB analogue.
    FindTableNanos = 41,
    /// ❌ SlateDB has no memtable-level bloom filter.
    BloomMemtableHitCount = 42,
    /// ❌ ditto.
    BloomMemtableMissCount = 43,
    BloomSstHitCount = 44,
    BloomSstMissCount = 45,
    KeyLockWaitTime = 46,
    KeyLockWaitCount = 47,
    IoThreadPoolId = 48,
    IoBytesWritten = 49,
    IoBytesRead = 50,
    IoOpenNanos = 51,
    IoAllocateNanos = 52,
    IoWriteNanos = 53,
    IoReadNanos = 54,
    IoRangeSyncNanos = 55,
    IoLoggerNanos = 56,
}

pub const PC_MAX_IDX: usize = 57;

/// Human-readable SHOW STATUS labels for each `PerfCounter`. Indexed by
/// `variant as usize`. Mirrors C++ `rdb_pc_stat_types[PC_MAX_IDX]`.
pub static PC_STAT_TYPES: [&str; PC_MAX_IDX] = [
    "user_key_comparison_count",
    "block_cache_hit_count",
    "block_read_count",
    "block_read_byte",
    "block_read_time",
    "block_checksum_time",
    "block_decompress_time",
    "get_read_bytes",
    "multiget_read_bytes",
    "iter_read_bytes",
    "internal_key_skipped_count",
    "internal_delete_skipped_count",
    "internal_recent_skipped_count",
    "internal_merge_count",
    "get_snapshot_time",
    "get_from_memtable_time",
    "get_from_memtable_count",
    "get_post_process_time",
    "get_from_output_files_time",
    "seek_on_memtable_time",
    "seek_on_memtable_count",
    "next_on_memtable_count",
    "prev_on_memtable_count",
    "seek_child_seek_time",
    "seek_child_seek_count",
    "seek_min_heap_time",
    "seek_max_heap_time",
    "seek_internal_seek_time",
    "find_next_user_entry_time",
    "write_wal_time",
    "write_memtable_time",
    "write_delay_time",
    "write_pre_and_post_process_time",
    "db_mutex_lock_nanos",
    "db_condition_wait_nanos",
    "merge_operator_time_nanos",
    "read_index_block_nanos",
    "read_filter_block_nanos",
    "new_table_block_iter_nanos",
    "new_table_iterator_nanos",
    "block_seek_nanos",
    "find_table_nanos",
    "bloom_memtable_hit_count",
    "bloom_memtable_miss_count",
    "bloom_sst_hit_count",
    "bloom_sst_miss_count",
    "key_lock_wait_time",
    "key_lock_wait_count",
    "io_thread_pool_id",
    "io_bytes_written",
    "io_bytes_read",
    "io_open_nanos",
    "io_allocate_nanos",
    "io_write_nanos",
    "io_read_nanos",
    "io_range_sync_nanos",
    "io_logger_nanos",
];

/// Global aggregates, atomically updated. One `[AtomicU64; PC_MAX_IDX]` row.
/// C++ `Rdb_atomic_perf_counters`.
pub struct AtomicPerfCounters {
    values: [AtomicU64; PC_MAX_IDX],
}

impl AtomicPerfCounters {
    pub fn new() -> Self {
        // Const-construct: AtomicU64 has a const new(), and we use a small
        // helper since [AtomicU64; N] doesn't have Default.
        const Z: AtomicU64 = AtomicU64::new(0);
        Self { values: [Z; PC_MAX_IDX] }
    }

    pub fn add(&self, c: PerfCounter, n: u64) {
        self.values[c as usize].fetch_add(n, Ordering::Relaxed);
    }

    pub fn load(&self, c: PerfCounter) -> u64 {
        self.values[c as usize].load(Ordering::Relaxed)
    }
}

impl Default for AtomicPerfCounters {
    fn default() -> Self { Self::new() }
}

/// Snapshot of per-thread counters. Returned to the SHOW STATUS layer.
/// C++ `Rdb_perf_counters`.
#[derive(Debug, Clone, Default)]
pub struct PerfCounters {
    pub values: [u64; PC_MAX_IDX],
}

impl PerfCounters {
    /// Read SlateDB's metric registry into a snapshot. Unmapped slots stay 0.
    /// Replaces C++ `Rdb_perf_counters::load(const Rdb_atomic_perf_counters&)`.
    pub fn from_slatedb_metrics() -> Result<Self, Error> {
        todo!("read slatedb metric registry, fill matching slots in `values`")
    }
}

/// RAII timer used around each read/write hot-path call. C++ `Rdb_io_perf`.
///
/// `start()` snapshots `Instant::now()`; `end_and_record()` computes the
/// delta and attributes to the configured counter set.
pub struct IoPerf<'a> {
    pub atomic_counters: Option<&'a AtomicPerfCounters>,
    pub shared_read: Option<&'a crate::rdb_mariadb_port_h::IoPerfAtomic>,
    pub shared_write: Option<&'a crate::rdb_mariadb_port_h::IoPerfAtomic>,
    start: Option<std::time::Instant>,
    /// Cumulative bytes written observed since `start()`. Updated by
    /// `update_bytes_written` callbacks.
    io_write_bytes: u64,
    io_write_requests: u64,
}

impl<'a> IoPerf<'a> {
    pub fn new() -> Self {
        Self {
            atomic_counters: None,
            shared_read: None,
            shared_write: None,
            start: None,
            io_write_bytes: 0,
            io_write_requests: 0,
        }
    }

    /// Begin a measurement window. `perf_context_level` is the sysvar level
    /// (0 = off, 4 = all). Returns true if any sampling is active.
    pub fn start(&mut self, perf_context_level: u32) -> bool {
        if perf_context_level == 0 { return false; }
        self.start = Some(std::time::Instant::now());
        true
    }

    pub fn update_bytes_written(&mut self, perf_context_level: u32, bytes: u64) {
        if perf_context_level == 0 || self.start.is_none() { return; }
        self.io_write_bytes = self.io_write_bytes.saturating_add(bytes);
        self.io_write_requests = self.io_write_requests.saturating_add(1);
    }

    /// Close the measurement window, attribute to counters. C++
    /// `end_and_record`.
    pub fn end_and_record(&mut self, perf_context_level: u32) {
        if perf_context_level == 0 { return; }
        let Some(_t0) = self.start.take() else { return };
        // TODO(human): pull the matching SlateDB metric deltas (request
        // bytes, latency histogram quantile if available) and fold them
        // into `atomic_counters`/`shared_read`/`shared_write`.
        todo!("attribute elapsed and observed deltas to the configured counters")
    }
}

impl<'a> Default for IoPerf<'a> {
    fn default() -> Self { Self::new() }
}
