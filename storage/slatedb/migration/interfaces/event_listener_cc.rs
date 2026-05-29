//! Interface stub for `event_listener_cc`.
//!
//! C++ source: `storage/rocksdb/event_listener.cc` (97 LoC)
//! C++ class:  `Rdb_event_listener` (impl)
//!
//! ## Mapping
//! Per _DESIGN.md §1 ("RocksDB event listener" row — Map, different shape):
//! the RocksDB callback pattern is replaced by a `tokio::sync::watch::Receiver`
//! returned by `DbMetadataOps::subscribe()`. This file is the impl half of
//! `event_listener_h.rs`: it instantiates `StatsRefreshTask`, wires it to
//! `Db::subscribe()`, and provides the helpers that translate a manifest
//! snapshot delta into the per-index stats updates the DDL manager consumes.
//!
//! The C++ class had four callback methods (`OnFlushCompleted`,
//! `OnCompactionCompleted`, `OnExternalFileIngested`, `OnBackgroundError`) plus
//! one helper (`update_index_stats`). All four callbacks collapse into a single
//! "manifest changed" event in SlateDB — the watch task handles them
//! uniformly. `OnBackgroundError` is unnecessary because SlateDB surfaces
//! errors synchronously through API returns and through `DbStatus::CloseReason`.
//!
//! ## Out-of-scope methods
//! - `OnCompactionCompleted`, `OnFlushCompleted`, `OnExternalFileIngested` —
//!   replaced by the single `on_status_change` handler.
//! - `OnBackgroundError` — replaced by `DbStatus::close_reason` inspection
//!   and by synchronous error propagation from the read/write API.
//! - `update_index_stats(rocksdb::TableProperties)` — RocksDB SST-side hook;
//!   SlateDB-side replacement reads `VersionedManifest` and walks SST props
//!   itself.

use crate::event_listener_h::{DdlManagerRef, StatsRefreshTask};
use slatedb::Error;
use std::sync::Arc;

/// Wires the `StatsRefreshTask` into the engine's Tokio runtime.
///
/// Inputs:
/// - `db`: the open `slatedb::Db` whose status we want to track.
/// - `ddl_manager`: handle to the DDL manager that owns the index-stats cache.
/// - `cancel`: cancellation token; dropped at handlerton shutdown to stop the
///   task cleanly.
///
/// Output: a `JoinHandle` for the spawned task. The caller (`plugin::init`)
/// stores it and awaits it during `plugin::done`.
///
/// Errors: returns `slatedb::Error` only if `db.subscribe()` is unavailable
/// (e.g., DB already closed) — wraps `ErrorKind::Closed`.
///
/// Original: event_listener.cc (entire file) — replaces the C++ constructor
/// call site at `ha_rocksdb.cc` that registered the listener via
/// `DBOptions::listeners.push_back(...)`.
pub async fn spawn_stats_refresh_task(
    db: &slatedb::Db,
    ddl_manager: Arc<dyn DdlManagerRef>,
    cancel: tokio_util::sync::CancellationToken,
) -> Result<tokio::task::JoinHandle<()>, Error> {
    // TODO(human): confirm whether `Db::subscribe()` is synchronous or async on
    // the pinned slatedb rev. The exemplar assumes sync.
    let _ = (db, ddl_manager, cancel);
    todo!(
        "let rx = db.subscribe(); let task = StatsRefreshTask::new(ddl_manager, rx); \
         Ok(tokio::spawn(task.run(cancel)))"
    )
}

/// Extract per-CF / per-index aggregate stats from one manifest snapshot.
/// Replaces the C++ `extract_index_stats` static helper which walked
/// `rocksdb::TablePropertiesCollection`.
///
/// In SlateDB the analogue is iterating `VersionedManifest::compacted` and
/// `VersionedManifest::l0`, reading the per-SST stats blocks. Each SST stores
/// the index-stats blob produced by our merge-time recorder (see
/// `properties_collector_cc`).
///
/// Inputs:
/// - `status`: the new `DbStatus` (cloned out of the watch channel).
///
/// Output: vector of `(GlIndexId, IndexStats)` tuples, ordered by `GlIndexId`.
///
/// Errors: returns `slatedb::ErrorKind::Data` on a corrupt stats blob; logs
/// and continues for any other read failure (transient I/O is expected).
///
/// Original: event_listener.cc:36 — `extract_index_stats`.
pub fn extract_index_stats_from_status(
    status: &slatedb::DbStatus,
) -> Result<Vec<(crate::rdb_global_h::GlIndexId, IndexStatsBlob)>, Error> {
    let _ = status;
    todo!("walk status.manifest SSTs, decode the indexstats blob from each, aggregate")
}

/// Opaque blob holding one index's compacted stats. The shape mirrors
/// `Rdb_index_stats` from rdb_datadic.h — defined in detail in that stub.
/// Re-exported here as a type alias placeholder until rdb_datadic_h stubs land.
#[derive(Debug, Clone, Default)]
pub struct IndexStatsBlob {
    pub gl_index_id: crate::rdb_global_h::GlIndexId,
    pub rows: u64,
    pub data_size: u64,
    pub actual_disk_size: u64,
    pub entry_deletes: u64,
    pub entry_single_deletes: u64,
    pub entry_merges: u64,
    pub entry_others: u64,
}
