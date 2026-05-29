//! Interface stub for `ha_rocksdb_cc__ha_rocksdb__info`.
//!
//! C++ source: `storage/rocksdb/ha_rocksdb.cc` (body LoC 347, span 8277..14639)
//! v4 manifest sub-unit: `ha_rocksdb_cc__ha_rocksdb__info`
//!
//! ## Mapping
//! Handler-vtable introspection bucket: capability bits (`index_flags`,
//! `table_flags`, `max_supported_key_part_length`) and cardinality estimates
//! (`info`, `records_in_range`, `keyread_time`).
//!
//! These map to a mix of:
//! - Pure constants (`max_supported_key_part_length`, `table_flags`).
//! - `Db::scan_with_options(range, opts)` + per-SST property reads (SlateDB
//!   manifest walk) for `info(HA_STATUS_VARIABLE)`.
//! - SlateDB's `DbMetadataOps` (manifest) + the cached `Rdb_index_stats`
//!   maintained by the `StatsRefreshTask` (`event_listener_h.rs`) for
//!   `info(HA_STATUS_CONST)` and `records_in_range`.
//!
//! Per _DESIGN.md §1 (RocksDB event listener row): the per-SST property data
//! that RocksDB exposed via `EventListener::OnFlushCompleted` is replaced by
//! SlateDB's `DbStatus.manifest` snapshot which the `StatsRefreshTask` polls;
//! `info` reads the cached `Rdb_index_stats` populated there.
//!
//! `records_in_range` calls `Db::scan_with_options(range,
//! ScanOptions::approximate_size())` — TODO(human) below on whether SlateDB
//! exposes an approximate-size primitive equivalent to RocksDB's
//! `GetApproximateSizes`.
//!
//! ## Out-of-scope methods
//! None — all introspection paths translate to SlateDB primitives.

use bytes::Bytes;
use slatedb::Error;

use crate::ha_rocksdb_cc__ha_rocksdb__index::KeyRange;
use crate::ha_rocksdb_cc__ha_rocksdb__lifecycle::HaSlateDb;
use crate::rdb_global_h::{MAX_INDEX_COL_LEN_LARGE, MAX_INDEX_COL_LEN_SMALL};

/// Bitmask passed to `handler::info(uint flag)`. Mirrors the
/// `HA_STATUS_*` family from `sql/handler.h`. We model the four bits the
/// MyRocks impl actually inspects.
#[derive(Debug, Clone, Copy, Default)]
pub struct InfoFlag(pub u32);

impl InfoFlag {
    pub const VARIABLE: u32 = 1 << 0;  // HA_STATUS_VARIABLE
    pub const CONST: u32 = 1 << 1;     // HA_STATUS_CONST
    pub const AUTO: u32 = 1 << 2;      // HA_STATUS_AUTO
    pub const ERRKEY: u32 = 1 << 3;    // HA_STATUS_ERRKEY
}

/// Per-handler-call I/O + CPU cost estimate returned by `keyread_time`.
/// Forwarded as POD; SQL layer uses these to score query plans.
#[derive(Debug, Clone, Copy, Default)]
pub struct IoAndCpuCost {
    pub io: f64,
    pub cpu: f64,
}

/// Forwarded to a future TABLE-shape unit. Mirrors `page_range` from the
/// MariaDB optimizer (range estimation pagination hint).
#[derive(Debug, Clone, Copy, Default)]
pub struct PageRange {
    pub first_page: u64,
    pub last_page: u64,
}

impl HaSlateDb {
    /// `ulong ha_rocksdb::index_flags(uint inx, uint part, bool all_parts)
    /// const` — original C++ source line 8277.
    ///
    /// Inputs: `inx` (key index), `part` (key part index — 0 for the first
    /// part), `all_parts`.
    /// Outputs: bitwise-OR of `HA_READ_NEXT | HA_READ_ORDER | HA_READ_RANGE
    /// | HA_READ_PREV` plus conditional `HA_KEYREAD_ONLY` /
    /// `HA_CLUSTERED_INDEX` / `HA_DO_INDEX_COND_PUSHDOWN`.
    /// Errors: none — pure metadata. Returns 0 if the key is malformed (we
    /// surface it as `Err(ErrorKind::Invalid)` if `inx >= key_count`).
    /// Invariants:
    /// - PK gets `HA_KEYREAD_ONLY | HA_CLUSTERED_INDEX`.
    /// - SK gets `HA_DO_INDEX_COND_PUSHDOWN` (we support ICP) and
    ///   `HA_KEYREAD_ONLY` if `check_keyread_allowed(inx, part, all_parts)`
    ///   returns true (i.e., every column under `part` can be decoded from
    ///   the SK alone).
    pub fn index_flags(&self, inx: u32, part: u32, all_parts: bool) -> Result<u64, Error> {
        let _ = (inx, part, all_parts);
        todo!("base flags | (keyread_allowed ? HA_KEYREAD_ONLY : 0) | (inx==pk ? CLUSTERED : ICP)")
    }

    /// `uint ha_rocksdb::max_supported_key_part_length() const` — original
    /// C++ source line 9521.
    ///
    /// Inputs: none (reads the `rocksdb_large_prefix` sysvar, surfaced via
    /// `self.large_prefix_enabled`).
    /// Outputs: `MAX_INDEX_COL_LEN_LARGE` (3072) or `MAX_INDEX_COL_LEN_SMALL`
    /// (767).
    /// Errors: none.
    /// Invariants: must match the value used by `create_key_def` at
    /// table-create time — narrowing the sysvar after a table is created
    /// with `large_prefix=true` does NOT shrink existing indexes.
    pub fn max_supported_key_part_length(&self) -> u32 {
        if self.large_prefix_enabled() {
            MAX_INDEX_COL_LEN_LARGE
        } else {
            MAX_INDEX_COL_LEN_SMALL
        }
    }

    /// Helper: read the `slatedb_large_prefix` sysvar (default: true).
    fn large_prefix_enabled(&self) -> bool {
        todo!("read the sysvar from the handlerton config snapshot")
    }

    /// `int ha_rocksdb::info(uint flag)` — original C++ source line 10962.
    ///
    /// Inputs: `flag` — a bitwise-OR of `InfoFlag::{VARIABLE,CONST,AUTO,ERRKEY}`.
    /// Outputs: `Ok(())` after `self.stats` is repopulated.
    /// Errors:
    /// - `ErrorKind::Unavailable` on manifest-walk I/O failure.
    /// - `ErrorKind::Internal` if `calculate_stats_for_table` returns an
    ///   inconsistent result after a re-analyze attempt.
    /// Invariants:
    /// - `VARIABLE` triggers `update_stats` (read cached `Rdb_index_stats`
    ///   from DdlManager); if any field is negative, re-runs
    ///   `calculate_stats_for_table` first.
    /// - `VARIABLE` with `stats.records == 0` triggers a SlateDB approximate
    ///   size probe (see TODO below) + memtable cardinality sample.
    /// - `CONST` populates per-key cardinality from cached stats.
    /// - `ERRKEY` writes `self.dupp_errkey` into `self.dup_ref` etc.
    ///
    // TODO(human): RocksDB had `DB::GetApproximateSizes(cf, range, &sz,
    // INCLUDE_FILES)` for fast SST-aware row-count estimates. SlateDB doesn't
    // (yet?) expose an equivalent in its public API surface (`ops.rs`
    // `DbMetadataOps` only has `subscribe`/manifest). Decide: (a) walk the
    // manifest ourselves and sum SST sizes touching the range, or (b) cache
    // a periodic estimate refreshed by the `StatsRefreshTask`. Lean (b) —
    // matches our event-listener replacement pattern. Flag for reviewer.
    pub async fn info(&mut self, flag: InfoFlag) -> Result<(), Error> {
        let _ = flag;
        todo!("VARIABLE: update_stats; if negative -> calculate_stats_for_table; if records==0 -> approximate-size probe + memtable sample; CONST: per-key cardinality")
    }

    /// `ulonglong ha_rocksdb::table_flags() const` — original C++ source
    /// line 11367.
    ///
    /// Inputs: implicit `self.thd_unsafe_for_binlog` + `self.thd_is_slave`.
    /// Outputs: bitwise-OR of `HA_BINLOG_ROW_CAPABLE | HA_REC_NOT_IN_SEQ |
    /// HA_CAN_INDEX_BLOBS | HA_PRIMARY_KEY_IN_READ_INDEX |
    /// HA_PRIMARY_KEY_REQUIRED_FOR_POSITION | HA_NULL_IN_KEY |
    /// HA_PARTIAL_COLUMN_READ | HA_REUSES_FILE_NAMES | HA_TABLE_SCAN_ON_INDEX`,
    /// plus `HA_BINLOG_STMT_CAPABLE` if the THD allows SBR.
    /// Errors: none — pure constant function of THD state.
    /// Invariants: callers MUST cache the result per-`TABLE_SHARE` (we set
    /// `cached_table_flags` in `init_with_fields`).
    pub fn table_flags(&self) -> u64 {
        todo!("base flags | (thd_unsafe_for_binlog||thd_is_slave ? HA_BINLOG_STMT_CAPABLE : 0)")
    }

    /// `ha_rows ha_rocksdb::records_in_range(uint inx, const key_range *min_key,
    /// const key_range *max_key, page_range *pages)` — original C++ source
    /// line 11979.
    ///
    /// Inputs: `inx` (key index), `min_key` / `max_key` (range bounds, both
    /// optional), `pages` (out — populated with the page range for the
    /// optimizer).
    /// Outputs: estimated row count in the range.
    /// Errors: none — returns 1 (the SQL-layer "at least one row" sentinel)
    /// on any underlying error per upstream.
    /// Invariants:
    /// - Honors the per-session `records_in_range` sysvar (returned verbatim
    ///   if non-zero — used for testing / hint-overrides).
    /// - Honors `table->force_index` (returns `HA_POS_ERROR` to discourage
    ///   non-index plans).
    /// - Underlying probe: same approximate-size flow as `info(VARIABLE)`
    ///   (TODO above) — divides bytes by `ROCKSDB_ASSUMED_KEY_VALUE_DISK_SIZE`
    ///   to get a row count.
    pub async fn records_in_range(
        &mut self,
        inx: u32,
        min_key: Option<&KeyRange>,
        max_key: Option<&KeyRange>,
        pages: &mut PageRange,
    ) -> Result<u64, Error> {
        let _ = (inx, min_key, max_key, pages);
        todo!("if THDVAR set: return it; if force_index: HA_POS_ERROR; else: approx size of range / assumed_kv_disk_size")
    }

    /// `IO_AND_CPU_COST ha_rocksdb::keyread_time(uint index, ulong ranges,
    /// ha_rows rows, ulonglong blocks)` — original C++ source line ~14640
    /// (the `index_blocks` companion). The default `handler::keyread_time`
    /// is overridden to account for SlateDB's column-LSM I/O profile.
    ///
    /// Inputs: `index`, `ranges` (number of range-scan operations), `rows`
    /// (estimated rows touched), `blocks` (estimated blocks read).
    /// Outputs: `IoAndCpuCost { io, cpu }`.
    /// Errors: none — pure cost-model math.
    /// Invariants: returns a value monotone in `rows`/`blocks` so the
    /// optimizer's `IndexPlan` vs `RangePlan` comparison is well-ordered.
    /// Per _DESIGN.md §1 (perf counters → Re-impl): the numerator constants
    /// are SlateDB-specific (block-cache hit rate, foyer-vs-moka latency).
    pub fn keyread_time(&self, index: u32, ranges: u64, rows: u64, blocks: u64) -> IoAndCpuCost {
        let _ = (index, ranges, rows, blocks);
        todo!("io = (blocks + ranges) * slatedb_block_io_cost; cpu = rows * decode_cpu_per_row")
    }
}

/// `ulonglong ha_rocksdb::index_blocks(uint, uint, ha_rows)` companion of
/// `keyread_time`. Original C++ source line 14642. Not on the SQL vtable
/// directly but referenced by the cost model; listed in the `info` bucket
/// per v4 manifest.
#[allow(dead_code)]
fn _index_blocks_marker() {
    // Real impl in `HaSlateDb` once `keyread_time` is fleshed out — the
    // formula `(rows * key_storage_length / 4) / block_size + ranges` from
    // the C++ side carries over unchanged (75% compression heuristic).
    let _ = Bytes::new();
}
