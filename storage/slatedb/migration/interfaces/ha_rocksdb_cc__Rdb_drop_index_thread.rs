//! Interface stub for `ha_rocksdb_cc__Rdb_drop_index_thread`.
//!
//! C++ source: `storage/rocksdb/rdb_threads.h` (lines 191..193) +
//!             `storage/rocksdb/ha_rocksdb.cc` (lines 11626..11770, ~145 LoC) —
//!             `Rdb_drop_index_thread::run()`.
//! v4 manifest sub-unit: `ha_rocksdb_cc__Rdb_drop_index_thread`
//!
//! ## Mapping
//! Per _DESIGN.md §1 row "Compaction filters (drop-secondary-index)":
//! MyRocks marks dropped indexes in `dict_manager.is_drop_index_empty()`'s
//! list. This thread:
//!  1. Waits until the drop list is non-empty (or 24h tick otherwise).
//!  2. For each `(cf_id, index_id)` in the drop list, scans to confirm the
//!     CF is empty of that index's prefix (using `is_myrocks_index_empty`
//!     at ha_rocksdb.cc:11600).
//!  3. Removes the index from the dict once empty.
//!
//! **SlateDB strategy** (_DESIGN.md §1): the actual data sweep is done by
//! SlateDB's compactor + our `CompactionFilter` (feature `compaction_filters`)
//! which returns `Drop` for any row whose `(cf_id, index_id)` prefix is in
//! the drop set. **We do NOT replace SlateDB's compactor**; we add this Tokio
//! task that polls SlateDB to see when the drop set has been swept and then
//! prunes the dict entry.
//!
//! Rust shape: Tokio task with `CancellationToken` + `Notify` for the wake-up
//! signal (mirrors `rocksdb_drop_index_wakeup_thread` free fn at
//! ha_rocksdb.cc:2215 which does `rdb_drop_idx_thread.signal()`).
//!
//! ## Out-of-scope methods
//! None — single-method class, fully ported.

use slatedb::Error;
use std::sync::Arc;
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;

// Forward decls — both live in the dict-manager unit.
pub trait DictManagerRef: Send + Sync {
    /// True when the drop list is empty (24h sleep cadence applies).
    fn is_drop_index_empty(&self) -> bool;
}

pub struct RdbDropIndexThread {
    pub dict_manager: Arc<dyn DictManagerRef>,
    pub db: Arc<slatedb::Db>,
    pub wake: Arc<Notify>,
    pub cancel: CancellationToken,
}

impl RdbDropIndexThread {
    pub fn new(dict_manager: Arc<dyn DictManagerRef>, db: Arc<slatedb::Db>) -> Self {
        Self {
            dict_manager,
            db,
            wake: Arc::new(Notify::new()),
            cancel: CancellationToken::new(),
        }
    }

    /// Wake the loop now. Mirrors `rocksdb_drop_index_wakeup_thread` →
    /// `rdb_drop_idx_thread.signal()`.
    pub fn signal(&self) { self.wake.notify_one(); }

    /// Run-forever loop. Sleep interval is 60s while the drop list is
    /// non-empty, 24h otherwise (matches ha_rocksdb.cc:11640).
    ///
    /// Errors are logged; the task does not exit on transient failure.
    ///
    /// Original: ha_rocksdb.cc:11626 — `Rdb_drop_index_thread::run`.
    pub async fn run(self) {
        todo!("loop: select! { _ = cancel.cancelled() => break, _ = wake.notified() => ..., _ = sleep(interval) => process_drops().await }")
    }

    /// One pass: for each dropped index, scan-and-confirm via `db.scan_prefix`
    /// with `key_prefix = varint(cf_id) || u32_be(index_id)` (per _DESIGN.md §2);
    /// if empty, call `dict_manager.finish_indexes_to_drop`.
    async fn process_drops(&self) -> Result<(), Error> {
        todo!("for each (cf_id, index_id) in dict.get_ongoing_drop_indexes(): db.scan_prefix(varint(cf)||u32_be(idx)).next().await — if None, mark done")
    }
}
