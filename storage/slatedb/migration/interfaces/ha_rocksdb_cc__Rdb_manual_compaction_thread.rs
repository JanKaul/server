//! Interface stub for `ha_rocksdb_cc__Rdb_manual_compaction_thread`.
//!
//! C++ source: `storage/rocksdb/rdb_threads.h` (lines 162..185) +
//!             `storage/rocksdb/ha_rocksdb.cc` (lines 13693..13822, ~130 LoC).
//! v4 manifest sub-unit: `ha_rocksdb_cc__Rdb_manual_compaction_thread`
//!
//! ## Mapping
//! Per _DESIGN.md §1 row "cf_ops" and the task contract:
//! `Rdb_manual_compaction_thread` runs a per-second loop dequeuing user-
//! requested manual compactions (via `SET GLOBAL rocksdb_compact_cf = 'cfname'`,
//! see free fn `rocksdb_compact_column_family` at ha_rocksdb.cc:2163) and
//! invokes `rdb->CompactRange(...)` on each.
//!
//! **SlateDB strategy** (_DESIGN.md):
//! - SlateDB's native compactor (`Db::builder().with_compactor_builder(...)`)
//!   handles automatic compaction. We don't replace it.
//! - For user-initiated `OPTIMIZE TABLE` / `SET GLOBAL slatedb_compact_cf`,
//!   we call `Db::flush_with_options(FlushType::MemTable)` — that's the
//!   closest SlateDB analogue to RocksDB's `CompactRange`. It forces the
//!   memtable to roll and triggers the compactor to consider the new SST.
//! - True ranged-compaction is not directly exposed by SlateDB; for a
//!   range-only sweep we issue a write of a tombstone marker into the
//!   range and rely on the compaction filter to actually evict.
//!
//! Cancellation is via `CancellationToken` (replaces `m_stop`).
//!
//! ## Out-of-scope methods
//! - `rocksdb::ColumnFamilyHandle*` / `rocksdb::Slice*` raw pointer args —
//!   replaced by typed `cf_id: u32` + `start_key: Option<Bytes>`.
//! - `rocksdb_debug_manual_compaction_delay` (debug sleep) — keep as a
//!   sysvar in `sysvar_set.rs` but the actual wait is just `tokio::time::sleep`.

use bytes::Bytes;
use slatedb::Error;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;

/// One pending manual-compaction request. `mc_id` is monotonic per thread.
#[derive(Debug, Clone)]
pub struct ManualCompactionRequest {
    pub mc_id: i32,
    pub state: McState,
    pub cf_id: u32,
    pub start: Option<Bytes>,
    pub limit: Option<Bytes>,
    pub concurrency: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McState { Inited, Running }

pub struct RdbManualCompactionThread {
    pub db: Arc<slatedb::Db>,
    pub wake: Arc<Notify>,
    pub cancel: CancellationToken,

    /// `mc_id → request`. Ordered so we always pull the oldest first.
    inner: Mutex<RequestState>,
}

struct RequestState {
    latest_mc_id: i32,
    requests: BTreeMap<i32, ManualCompactionRequest>,
}

impl RdbManualCompactionThread {
    pub fn new(db: Arc<slatedb::Db>) -> Self {
        Self {
            db,
            wake: Arc::new(Notify::new()),
            cancel: CancellationToken::new(),
            inner: Mutex::new(RequestState { latest_mc_id: 0, requests: BTreeMap::new() }),
        }
    }

    /// Enqueue a request; returns its `mc_id` (-1 if the queue is full per
    /// `rocksdb_max_manual_compactions`).
    /// Original: ha_rocksdb.cc:13793.
    pub fn request_manual_compaction(
        &self,
        _cf_id: u32,
        _start: Option<Bytes>,
        _limit: Option<Bytes>,
        _concurrency: i32,
        _max_pending: usize,
    ) -> i32 {
        todo!("lock inner, check size < max_pending, insert with new mc_id, signal wake; return mc_id")
    }

    /// Poll for completion. Returns true if the request with `mc_id` is no
    /// longer in the queue (either ran or was cleared).
    /// Original: ha_rocksdb.cc:13814.
    pub fn is_manual_compaction_finished(&self, mc_id: i32) -> bool {
        self.inner
            .lock()
            .map(|s| !s.requests.contains_key(&mc_id))
            .unwrap_or(true)
    }

    /// Remove a pending or finished request. `init_only` means only remove
    /// it if still in `Inited` state (used to cancel queued, not running).
    /// Original: ha_rocksdb.cc:13769.
    pub fn clear_manual_compaction_request(&self, mc_id: i32, init_only: bool) {
        let _ = (mc_id, init_only);
        todo!("lock inner; if init_only check state==Inited; erase from BTreeMap")
    }

    /// Clear all requests; called on shutdown.
    pub fn clear_all_manual_compaction_requests(&self) {
        if let Ok(mut s) = self.inner.lock() { s.requests.clear(); }
    }

    /// Run-forever loop. Per second (or on `wake`), pull the front request,
    /// mark it `Running`, then call SlateDB to compact. On shutdown we exit
    /// without finishing pending requests (matches MyRocks: `CancelAllBackgroundWork`).
    ///
    /// Per _DESIGN.md, "compaction" maps to `flush_with_options(FlushType::MemTable)`
    /// — that's the trigger; SlateDB's native compactor does the actual work.
    ///
    /// Original: ha_rocksdb.cc:13693 — `Rdb_manual_compaction_thread::run`.
    pub async fn run(self) {
        todo!("loop: select! cancel/wake/sleep(1s); pop front, set Running, db.flush_with_options(FlushType::MemTable).await")
    }
}

/// Translate the result of a single compaction into an `Err` only on true
/// SlateDB I/O failures; shutdown-in-progress is silently swallowed (matches
/// the C++ `if (!s.IsShutdownInProgress())` check at ha_rocksdb.cc:13747).
pub fn maybe_log_compaction_error(_e: &Error) {
    todo!("if e.kind() == Unavailable && cancel.is_cancelled() → swallow; else log")
}
