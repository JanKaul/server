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
//! Per _DESIGN.md §1: SlateDB exposes status changes via
//! `DbMetadataOps::subscribe()`, which returns a
//! `tokio::sync::watch::Receiver<DbStatus>` tracking the latest
//! `durable_seq` + manifest snapshot. We replace the callback model with a
//! Tokio task that `await`s on this watch channel and updates the DDL
//! manager's index-stats cache when the manifest snapshot rolls forward
//! (i.e., a flush or compaction committed).
//!
//! This is more Rust-idiomatic than a callback trait and is exactly the
//! pattern SlateDB ships for this use case.
//!
//! ## Out-of-scope methods
//! - `OnCompactionCompleted`, `OnFlushCompleted`, `OnExternalFileIngested` —
//!   no per-event hook in SlateDB; replaced by the DbStatus watch task below.
//! - `OnBackgroundError` — SlateDB surfaces errors synchronously via API
//!   returns; we don't need a callback.

use slatedb::Error;

// Forward decl — actual `DdlManager` lives in `rdb_datadic_h__Rdb_ddl_manager.rs`.
pub trait DdlManagerRef: Send + Sync {}

/// Background task that watches `DbStatus` updates and refreshes the DDL
/// manager's index-statistics cache whenever a new manifest snapshot
/// becomes durable. Replaces MyRocks' callback-based `Rdb_event_listener`.
///
/// Lifecycle: spawned in `plugin::init`, cancelled in `plugin::done` via a
/// `CancellationToken`.
pub struct StatsRefreshTask {
    pub ddl_manager: std::sync::Arc<dyn DdlManagerRef>,
    /// `Db::subscribe()` receiver. The task waits on changes here, not on a timer.
    pub status_rx: tokio::sync::watch::Receiver<slatedb::DbStatus>,
}

impl StatsRefreshTask {
    pub fn new(
        ddl_manager: std::sync::Arc<dyn DdlManagerRef>,
        status_rx: tokio::sync::watch::Receiver<slatedb::DbStatus>,
    ) -> Self {
        Self { ddl_manager, status_rx }
    }

    /// React to one status change: read the new manifest, walk SST props for
    /// each tracked CF/index, push aggregate stats into the DDL manager.
    ///
    /// Errors are logged but do not stop the task (transient I/O is expected).
    pub async fn on_status_change(&mut self) -> Result<(), Error> {
        todo!("snapshot status, walk manifest SSTs, update ddl_manager.index_stats")
    }

    /// Run-forever loop. Per the doc comment on `subscribe()`, the receiver
    /// guard must not be held across `await` — clone status out, drop the
    /// borrow, then act.
    pub async fn run(mut self, cancel: tokio_util::sync::CancellationToken) {
        todo!("loop: select! { changed = self.status_rx.changed() => self.on_status_change().await, _ = cancel.cancelled() => break }")
    }
}
