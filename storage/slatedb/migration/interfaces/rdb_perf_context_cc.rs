//! Interface stub for `rdb_perf_context_cc`.
//!
//! C++ source: `storage/rocksdb/rdb_perf_context.cc` (286 LoC)
//! C++ classes: `Rdb_perf_counters`, `Rdb_atomic_perf_counters`, `Rdb_io_perf`
//!
//! ## Mapping
//! Per _DESIGN.md §1 ("rdb_perf_context" row — Re-impl): RocksDB's
//! `get_perf_context()` / `get_iostats_context()` thread-locals are
//! replaced by SlateDB's `slatedb_common::metrics`. The metric *shape* is
//! different (Prometheus families vs flat counter struct), but the SHOW
//! STATUS surface stays compatible — we expose the same `rocksdb_*_*` names
//! and report what we can; counters that don't map to a SlateDB metric
//! report `0` rather than fail.
//!
//! We keep the `PerfCounterIdx` enum because callers index into a counter
//! array by ordinal; the labels for SHOW STATUS are pulled from the same
//! array.
//!
//! ## Out-of-scope methods
//! - All RocksDB-specific fields (block_cache_*, bloom_*_hit_count when
//!   the SlateDB equivalent metric isn't enabled, etc.) — report 0 and
//!   move on. The PC_MAX_IDX layout is preserved for SHOW STATUS.

use slatedb::Error;
use std::sync::atomic::{AtomicU64, Ordering};

/// Index into the counter array. Order matches `rdb_pc_stat_types[]` in
/// rdb_perf_context.cc:41 verbatim so SHOW STATUS labels line up.
///
/// Original: rdb_perf_context.h — `enum perf_counter_idx { PC_USER_KEY_..., ... }`.
#[repr(usize)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PerfCounterIdx {
    UserKeyComparisonCount = 0,
    BlockCacheHitCount,
    BlockReadCount,
    BlockReadByte,
    BlockReadTime,
    BlockChecksumTime,
    BlockDecompressTime,
    GetReadBytes,
    MultigetReadBytes,
    IterReadBytes,
    InternalKeySkippedCount,
    InternalDeleteSkippedCount,
    InternalRecentSkippedCount,
    InternalMergeCount,
    GetSnapshotTime,
    GetFromMemtableTime,
    GetFromMemtableCount,
    GetPostProcessTime,
    GetFromOutputFilesTime,
    SeekOnMemtableTime,
    SeekOnMemtableCount,
    NextOnMemtableCount,
    PrevOnMemtableCount,
    SeekChildSeekTime,
    SeekChildSeekCount,
    SeekMinHeapTime,
    SeekMaxHeapTime,
    SeekInternalSeekTime,
    FindNextUserEntryTime,
    WriteWalTime,
    WriteMemtableTime,
    WriteDelayTime,
    WritePreAndPostProcessTime,
    DbMutexLockNanos,
    DbConditionWaitNanos,
    MergeOperatorTimeNanos,
    ReadIndexBlockNanos,
    ReadFilterBlockNanos,
    NewTableBlockIterNanos,
    NewTableIteratorNanos,
    BlockSeekNanos,
    FindTableNanos,
    BloomMemtableHitCount,
    BloomMemtableMissCount,
    BloomSstHitCount,
    BloomSstMissCount,
    KeyLockWaitTime,
    KeyLockWaitCount,
    IoThreadPoolId,
    IoBytesWritten,
    IoBytesRead,
    IoOpenNanos,
    IoAllocateNanos,
    IoWriteNanos,
    IoReadNanos,
    IoRangeSyncNanos,
    IoLoggerNanos,
}

pub const PC_MAX_IDX: usize = (PerfCounterIdx::IoLoggerNanos as usize) + 1;

/// SHOW STATUS label table (parallel array to `PerfCounterIdx`).
/// Original: rdb_perf_context.cc:41 — `rdb_pc_stat_types[]`.
pub const PC_STAT_TYPES: [&str; PC_MAX_IDX] = [
    "USER_KEY_COMPARISON_COUNT", "BLOCK_CACHE_HIT_COUNT", "BLOCK_READ_COUNT",
    "BLOCK_READ_BYTE", "BLOCK_READ_TIME", "BLOCK_CHECKSUM_TIME",
    "BLOCK_DECOMPRESS_TIME", "GET_READ_BYTES", "MULTIGET_READ_BYTES",
    "ITER_READ_BYTES", "INTERNAL_KEY_SKIPPED_COUNT",
    "INTERNAL_DELETE_SKIPPED_COUNT", "INTERNAL_RECENT_SKIPPED_COUNT",
    "INTERNAL_MERGE_COUNT", "GET_SNAPSHOT_TIME", "GET_FROM_MEMTABLE_TIME",
    "GET_FROM_MEMTABLE_COUNT", "GET_POST_PROCESS_TIME",
    "GET_FROM_OUTPUT_FILES_TIME", "SEEK_ON_MEMTABLE_TIME",
    "SEEK_ON_MEMTABLE_COUNT", "NEXT_ON_MEMTABLE_COUNT",
    "PREV_ON_MEMTABLE_COUNT", "SEEK_CHILD_SEEK_TIME", "SEEK_CHILD_SEEK_COUNT",
    "SEEK_MIN_HEAP_TIME", "SEEK_MAX_HEAP_TIME", "SEEK_INTERNAL_SEEK_TIME",
    "FIND_NEXT_USER_ENTRY_TIME", "WRITE_WAL_TIME", "WRITE_MEMTABLE_TIME",
    "WRITE_DELAY_TIME", "WRITE_PRE_AND_POST_PROCESS_TIME",
    "DB_MUTEX_LOCK_NANOS", "DB_CONDITION_WAIT_NANOS",
    "MERGE_OPERATOR_TIME_NANOS", "READ_INDEX_BLOCK_NANOS",
    "READ_FILTER_BLOCK_NANOS", "NEW_TABLE_BLOCK_ITER_NANOS",
    "NEW_TABLE_ITERATOR_NANOS", "BLOCK_SEEK_NANOS", "FIND_TABLE_NANOS",
    "BLOOM_MEMTABLE_HIT_COUNT", "BLOOM_MEMTABLE_MISS_COUNT",
    "BLOOM_SST_HIT_COUNT", "BLOOM_SST_MISS_COUNT", "KEY_LOCK_WAIT_TIME",
    "KEY_LOCK_WAIT_COUNT", "IO_THREAD_POOL_ID", "IO_BYTES_WRITTEN",
    "IO_BYTES_READ", "IO_OPEN_NANOS", "IO_ALLOCATE_NANOS", "IO_WRITE_NANOS",
    "IO_READ_NANOS", "IO_RANGE_SYNC_NANOS", "IO_LOGGER_NANOS",
];

/// Plain-copy counter struct returned to `SHOW STATUS` callers.
/// Original: rdb_perf_context.h — `struct Rdb_perf_counters`.
#[derive(Debug, Default, Clone)]
pub struct PerfCounters {
    pub value: [u64; PC_MAX_IDX],
}

/// Atomic-counter struct (the per-handler aggregator).
/// Original: rdb_perf_context.h — `struct Rdb_atomic_perf_counters`.
pub struct AtomicPerfCounters {
    pub value: [AtomicU64; PC_MAX_IDX],
}

impl Default for AtomicPerfCounters {
    fn default() -> Self {
        // Array of atomics — can't derive Default; build with array::from_fn.
        Self {
            value: std::array::from_fn(|_| AtomicU64::new(0)),
        }
    }
}

impl PerfCounters {
    /// Snapshot the atomic counter into the plain struct.
    /// Original: rdb_perf_context.cc:188 — `Rdb_perf_counters::load`.
    pub fn load(&mut self, src: &AtomicPerfCounters) {
        for i in 0..PC_MAX_IDX {
            self.value[i] = src.value[i].load(Ordering::Relaxed);
        }
    }
}

/// Global counters across all handlers. Updated by the metric-subscription
/// task at the same cadence as the `StatsRefreshTask`.
pub static RDB_GLOBAL_PERF_COUNTERS: std::sync::OnceLock<AtomicPerfCounters> =
    std::sync::OnceLock::new();

/// Read the global counters into the caller-supplied struct.
/// Original: rdb_perf_context.cc:184 — `rdb_get_global_perf_counters`.
pub fn get_global_perf_counters(counters: &mut PerfCounters) {
    if let Some(src) = RDB_GLOBAL_PERF_COUNTERS.get() {
        counters.load(src);
    }
}

/// Per-statement / per-handler I/O perf context. The C++ version managed
/// the `rocksdb::SetPerfLevel(...)` toggle plus the `harvest_diffs(...)`
/// snapshotting at end-of-statement. Our analogue subscribes to the
/// equivalent SlateDB metric series at handler-open time and snapshots them
/// at end-of-statement.
pub struct IoPerf {
    pub level: u32,
    pub started: bool,
    pub atomic_counters: Option<std::sync::Arc<AtomicPerfCounters>>,
    pub io_write_bytes: u64,
    pub io_write_requests: u64,
}

impl IoPerf {
    /// Begin a statement — capture baseline metric values.
    ///
    /// Inputs: `perf_context_level` (0 = disabled, 1..3 = increasing detail).
    /// Output: `true` if perf-context collection is active for this stmt.
    ///
    /// Original: rdb_perf_context.cc:194 — `Rdb_io_perf::start`.
    pub fn start(&mut self, perf_context_level: u32) -> bool {
        self.level = perf_context_level;
        if perf_context_level == 0 {
            self.started = false;
            return false;
        }
        self.started = true;
        todo!("snapshot baseline metric registry values into self.* for later diffing")
    }

    /// Account a write — increments `io_write_bytes` / `_requests` if perf
    /// is active.
    /// Original: rdb_perf_context.cc:211 — `update_bytes_written`.
    pub fn update_bytes_written(&mut self, perf_context_level: u32, bytes: u64) {
        if perf_context_level != 0 {
            self.io_write_bytes += bytes;
            self.io_write_requests += 1;
        }
    }

    /// End the statement, diff against baseline, fold into `atomic_counters`
    /// and the global counter.
    ///
    /// Original: rdb_perf_context.cc:221 — `end_and_record`.
    pub fn end_and_record(&mut self, perf_context_level: u32) {
        if perf_context_level == 0 {
            return;
        }
        todo!("diff metric registry values vs baseline, add to atomic_counters + RDB_GLOBAL_PERF_COUNTERS")
    }
}

/// Map a `slatedb_common::metrics` name to a `PerfCounterIdx`. Returns
/// `None` for metrics that have no MyRocks counterpart (those just remain
/// 0 in SHOW STATUS).
///
/// Original: implicit in rdb_perf_context.cc's `IO_PERF_RECORD` /
/// `IO_STAT_RECORD` macros — we make the mapping explicit so the metric
/// subscription task can drive it.
pub fn metric_to_idx(_metric_name: &str) -> Option<PerfCounterIdx> {
    todo!("table-driven mapping; populated as we discover slatedb metric names")
}

/// Stub-only Error stand-in for fn signature compatibility. Marks
/// non-applicability — never used at runtime.
#[allow(dead_code)]
fn _force_error_use() -> Result<(), Error> { Ok(()) }
