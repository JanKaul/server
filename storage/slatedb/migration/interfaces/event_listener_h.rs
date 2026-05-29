//! Interface stub for `event_listener_h`.
//!
//! C++ source: `storage/rocksdb/event_listener.h` (49 LoC)
//! C++ class: `Rdb_event_listener : public rocksdb::EventListener`
//!
//! ## Mapping
//! In MyRocks, this is the bridge from RocksDB's compaction/flush/ingest event
//! callbacks into the MyRocks DDL manager — primarily to update index statistics
//! when SST file properties change.
//!
//! Per _DESIGN.md §1: SlateDB has no equivalent compaction/flush event hook
//! (its compactor runs out-of-process or as an internal task without exposing
//! per-job callbacks). We map this to a **background sweep task** that polls
//! SlateDB stats periodically and updates our index-stats cache.
//!
//! ## Out-of-scope methods
//! - `OnCompactionCompleted`, `OnFlushCompleted`, `OnExternalFileIngested` —
//!   no SlateDB analogue; replaced by `stats_refresh_task` (a Tokio interval task)
//! - `OnBackgroundError` — SlateDB surfaces errors synchronously in API returns,
//!   not via callback; we propagate as `SlateError` instead

use crate::error::SlateError;

// Forward decl — actual `DdlManager` lives in `rdb_datadic_h__Rdb_ddl_manager.rs`.
pub trait DdlManagerRef: Send + Sync {}

/// Background task that periodically polls SlateDB stats and refreshes the
/// DDL manager's index-statistics cache. Replaces MyRocks' event-driven
/// `Rdb_event_listener` since SlateDB exposes no per-event hooks.
///
/// Lifecycle: spawned in `plugin::init`, cancelled in `plugin::done`.
pub struct StatsRefreshTask {
    pub ddl_manager: std::sync::Arc<dyn DdlManagerRef>,
    pub interval_ms: u64, // sysvar `slatedb_stats_refresh_interval_ms`
}

impl StatsRefreshTask {
    /// Construct (does not spawn). Caller spawns onto the Tokio runtime.
    pub fn new(ddl_manager: std::sync::Arc<dyn DdlManagerRef>, interval_ms: u64) -> Self {
        Self { ddl_manager, interval_ms }
    }

    /// One refresh cycle: fetch SlateDB stats, update index-stats cache.
    /// Async because SlateDB API is async.
    ///
    /// Returns `Ok(())` on success, `SlateError::Io` on stat-fetch failure.
    /// Failures are logged but do not stop the periodic task.
    pub async fn tick(&self) -> Result<(), SlateError> {
        todo!("fetch slatedb stats, call ddl_manager.update_index_stats(...)")
    }

    /// Run-forever loop. Spawned onto the Tokio runtime in plugin::init.
    /// Honors cancellation via the supplied `cancel_token`.
    pub async fn run(self, cancel_token: tokio_util::sync::CancellationToken) {
        todo!("loop: select! { tick / cancel }")
    }
}
