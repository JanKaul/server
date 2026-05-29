//! Interface stub for `properties_collector_cc`.
//!
//! C++ source: `storage/rocksdb/properties_collector.cc` (~570 LoC)
//! C++ class:  `Rdb_tbl_prop_coll : public rocksdb::TablePropertiesCollector`
//!             plus the `Rdb_tbl_prop_coll_factory`, helpers, and
//!             `Rdb_index_stats` serializer.
//!
//! ## Mapping
//! Per _DESIGN.md §1 ("Re-impl" verdict for perf counters AND for the event-
//! listener "different shape"): the RocksDB per-SST properties collector is
//! replaced by a combination of:
//!
//! - **SlateDB metrics** (`slatedb_common::metrics`) for the entry-type
//!   counters (put/delete/single-delete/merge/other) and the cumulative SST
//!   entry counts that drive deletion-heavy compaction heuristics.
//! - **The `StatsRefreshTask`** (see `event_listener_h.rs` exemplar and
//!   `event_listener_cc.rs`) for the per-index cardinality + row-count
//!   aggregates that MyRocks consumed via `read_stats_from_tbl_props`.
//!
//! In SlateDB there is no per-SST "table property collector" hook to register.
//! Instead, when a flush or compaction commits and the manifest snapshot rolls
//! forward, the watch task reads `VersionedManifest` and reconciles the
//! per-index stats cache. The result is identical for the DDL manager's
//! purposes (it just gets the new stats); the trigger is pull-based, not push.
//!
//! ## Out-of-scope methods (handled differently or dropped)
//! - `AddUserKey` / `AdjustDeletedRows` / `CollectStatsForRow` — per-SST hook
//!   not available in SlateDB; replaced by manifest scan in `StatsRefreshTask`.
//! - `Finish` (per-SST) / `GetReadableProperties` — same.
//! - `Rdb_tbl_prop_coll_factory` — RocksDB registration plumbing; no analogue.
//! - `Rdb_index_stats::materialize` / `unmaterialize` — moved into the
//!   `IndexStatsBlob` type in `event_listener_cc.rs` (codec-only helpers).

use crate::event_listener_h::{DdlManagerRef, StatsRefreshTask};
use slatedb::Error;
use std::sync::Arc;

// --- global counters (replace the `std::atomic<uint64_t>` globals at the
// top of properties_collector.cc:45-49) ---

/// Cumulative per-entry-type counters across all live SSTs. Updated by the
/// `StatsRefreshTask` after each manifest snapshot reconciliation. Exposed via
/// `SHOW STATUS` and the `rocksdb_num_sst_entry_*` sysvars.
///
/// Original: properties_collector.cc:45 — `rocksdb_num_sst_entry_put` etc.
#[derive(Debug, Default)]
pub struct SstEntryCounters {
    pub put: std::sync::atomic::AtomicU64,
    pub delete_: std::sync::atomic::AtomicU64,
    pub single_delete: std::sync::atomic::AtomicU64,
    pub merge: std::sync::atomic::AtomicU64,
    pub other: std::sync::atomic::AtomicU64,
}

/// One global instance, initialized at plugin load. Per _DESIGN.md §7 we keep
/// runtime singletons in a `OnceLock`.
/// Original: properties_collector.cc:45-49 — module-level globals.
pub static SST_ENTRY_COUNTERS: std::sync::OnceLock<SstEntryCounters> = std::sync::OnceLock::new();

/// Sysvar mirror — when ON, single-deletes count as deletes for the
/// "sequential deletes" compaction heuristic.
/// Original: properties_collector.cc:50.
pub static COMPACTION_SEQUENTIAL_DELETES_COUNT_SD: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

// --- compaction-params struct (the C++ `Rdb_compact_params` POD) ---

/// Heuristic thresholds the C++ collector consulted to mark an SST as
/// "needs early compaction" based on tombstone density.
///
/// In SlateDB the compaction scheduler is internal and we don't drive it from
/// outside, but we still surface these as sysvars for users porting from
/// MyRocks; they map onto our own compaction-trigger metrics (see
/// `engine/stats_task.rs`) which the maintainer wires up in TRANSLATE.
///
/// Original: properties_collector.h — `struct Rdb_compact_params`.
#[derive(Debug, Clone)]
pub struct CompactParams {
    pub deletes: u64,
    pub window: u64,
    pub file_size: u64,
}

// --- the StatsRefreshTask analogue spawner ---

/// Spawn the per-index stats refresh task that replaces this collector.
///
/// Inputs:
/// - `db`: the open SlateDB.
/// - `ddl_manager`: handle to the DDL manager.
/// - `cancel`: cancellation token honored at handlerton shutdown.
///
/// Output: `JoinHandle` returned by `tokio::spawn`.
///
/// Errors: `slatedb::ErrorKind::Closed` if the DB has shut down before we
/// subscribe; otherwise the task's own errors are logged-and-continued.
///
/// Original: replaces the `Rdb_tbl_prop_coll_factory::CreateTablePropertiesCollector`
/// registration site at properties_collector.cc end of file.
pub async fn spawn(
    db: &slatedb::Db,
    ddl_manager: Arc<dyn DdlManagerRef>,
    cancel: tokio_util::sync::CancellationToken,
) -> Result<tokio::task::JoinHandle<()>, Error> {
    // Note: this is intentionally the same task type as event_listener_cc's
    // spawn — the two C++ modules collapsed into one Rust task.
    let _ = (db, ddl_manager, cancel);
    todo!("rx = db.subscribe(); task = StatsRefreshTask::new(ddl_manager, rx); spawn task.run(cancel)")
}

// --- per-index aggregation (called from the watch task) ---

/// Cardinality-sampling helper. The C++ collector hashed keys at a configured
/// sampling rate to estimate `distinct_keys_per_prefix`. SlateDB doesn't
/// produce per-SST cardinality estimates, so we approximate them by sampling
/// the post-compaction iterator at the same rate.
///
/// Inputs:
/// - `iter`: a fresh `DbIterator` over the index's prefix range.
/// - `sampling_pct`: 1..=100, default 10 (see `DEFAULT_TBL_STATS_SAMPLE_PCT`).
///
/// Output: per-key-part distinct-key estimate, written into the
/// `IndexStatsBlob` returned to the caller.
///
/// Errors: passes through any read error from `DbIterator::next`.
///
/// Original: properties_collector.cc — `m_cardinality_collector.ProcessKey`.
pub async fn estimate_cardinality(
    iter: &mut slatedb::DbIterator<'_>,
    sampling_pct: u8,
) -> Result<Vec<u64>, Error> {
    let _ = (iter, sampling_pct);
    todo!("reservoir-sample keys from iter at sampling_pct, count prefix-distinct")
}
