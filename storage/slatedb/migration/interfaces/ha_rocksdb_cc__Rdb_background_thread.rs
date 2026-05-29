//! Interface stub for `ha_rocksdb_cc__Rdb_background_thread`.
//!
//! C++ source: `storage/rocksdb/rdb_threads.h` (lines 140..160) +
//!             `storage/rocksdb/ha_rocksdb.cc` (lines 13598..13686, ~88 LoC) —
//!             `Rdb_background_thread::run()`.
//! v4 manifest sub-unit: `ha_rocksdb_cc__Rdb_background_thread`
//!
//! ## Mapping
//! Per _DESIGN.md §1 row "RocksDB event listener" and the task contract:
//! `Rdb_background_thread` is a MyRocks-specific maintenance loop that
//!  1. periodically persists in-memory index stats to the data dictionary
//!     (`ddl_manager.persist_stats()`),
//!  2. drives the index-stat recalculation queue (`rdb_indexes_to_recalc`),
//!  3. wakes up on demand via `request_save_stats()`.
//!
//! It is **on top of** RocksDB's own background threads (see the C++ comment
//! at rdb_threads.h:135). Per _DESIGN.md, **SlateDB's native compactor**
//! handles flush/compaction (`Db::builder().with_compactor_builder(...)`);
//! we do NOT replace it. This task only handles MyRocks-specific stats
//! bookkeeping.
//!
//! Rust shape:
//! - Spawned as a Tokio task on the engine runtime in `plugin::init`.
//! - Cancellation via `tokio_util::sync::CancellationToken` (replaces the
//!   C++ `m_stop` flag + `mysql_cond_timedwait`).
//! - Wake-up via `tokio::sync::Notify` (replaces `m_signal_cond`).
//! - Reads `DbStatus` changes by holding a `watch::Receiver<DbStatus>` from
//!   `DbMetadataOps::subscribe()` — that's our trigger for "the manifest
//!   moved; recompute stats for affected SSTs."
//!
//! ## Out-of-scope methods
//! - `mysql_mutex_t` / `mysql_cond_t` PSI integration — replaced by tokio
//!   primitives (no PSI on Rust side; metrics exposed via slatedb metrics).

use slatedb::Error;
use std::sync::Arc;
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;

// Forward decl — DDL manager lives in `rdb_datadic_h__Rdb_ddl_manager.rs`.
pub trait DdlManagerRef: Send + Sync {}

/// MyRocks background-maintenance task. Owns its cancel token + notifier.
///
/// One instance per handlerton lifetime, spawned in `plugin::init`.
pub struct RdbBackgroundThread {
    pub ddl_manager: Arc<dyn DdlManagerRef>,
    pub status_rx: tokio::sync::watch::Receiver<slatedb::DbStatus>,

    /// Wake-up signal — `wake.notify_one()` is the Rust analogue of
    /// `mysql_cond_signal(&m_signal_cond)`.
    pub wake: Arc<Notify>,

    /// Cancellation token; `cancel.cancel()` is the analogue of setting
    /// `m_stop = true` and signalling.
    pub cancel: CancellationToken,

    /// If true on next wake-up, force a `ddl_manager.persist_stats()` even if
    /// no DbStatus change happened. Set by `request_save_stats`.
    /// Original: rdb_threads.h:142 — `m_save_stats`.
    pub save_stats_requested: std::sync::atomic::AtomicBool,
}

impl RdbBackgroundThread {
    pub fn new(
        ddl_manager: Arc<dyn DdlManagerRef>,
        status_rx: tokio::sync::watch::Receiver<slatedb::DbStatus>,
    ) -> Self {
        Self {
            ddl_manager,
            status_rx,
            wake: Arc::new(Notify::new()),
            cancel: CancellationToken::new(),
            save_stats_requested: std::sync::atomic::AtomicBool::new(false),
        }
    }

    /// Request a stats save on next wake-up; also wakes the loop immediately.
    /// Original: rdb_threads.h:153 — `request_save_stats`.
    pub fn request_save_stats(&self) {
        self.save_stats_requested
            .store(true, std::sync::atomic::Ordering::Relaxed);
        self.wake.notify_one();
    }

    /// Run-forever loop. Select between:
    ///   - `cancel.cancelled()` → break
    ///   - `status_rx.changed()` → process the new DbStatus
    ///   - `wake.notified()` → process a manual wake (save stats / recalc)
    ///   - timer (sec interval) → periodic recalc batch
    ///
    /// On loop exit we call `ddl_manager.persist_stats()` one last time
    /// (mirrors the C++ code path at ha_rocksdb.cc:13685).
    ///
    /// Original: ha_rocksdb.cc:13598 — `Rdb_background_thread::run`.
    pub async fn run(self) {
        todo!("tokio::select! { _ = cancel.cancelled() => break, _ = self.wake.notified() => ..., _ = self.status_rx.changed() => ..., _ = sleep(1s) => ... }")
    }

    /// One iteration of the index-stats recalculation loop. Pulls up to
    /// `rocksdb_stats_recalc_rate` indexes from the queue, asks the DDL
    /// manager for their key-defs, runs `calculate_stats`.
    /// Original: ha_rocksdb.cc:13647.
    async fn drain_recalc_queue(&self) -> Result<(), Error> {
        todo!("walk rdb_indexes_to_recalc up to stats_recalc_rate; call ddl_manager hooks")
    }
}
