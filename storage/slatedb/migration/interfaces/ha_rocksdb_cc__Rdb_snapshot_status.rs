//! Interface stub for `ha_rocksdb_cc__Rdb_snapshot_status`.
//!
//! C++ source: `storage/rocksdb/ha_rocksdb.cc` (lines 4328..4494, ~167 LoC)
//! v4 manifest sub-unit: `ha_rocksdb_cc__Rdb_snapshot_status`
//!
//! ## Mapping
//! `Rdb_snapshot_status : public Rdb_tx_list_walker` builds the text body
//! shown by `SHOW ENGINE ROCKSDB STATUS` — header + per-txn snapshot lines
//! ("---SNAPSHOT, ACTIVE %lld sec…") + the deadlock-info dump.
//!
//! Per _DESIGN.md §1 rows "RocksDB event listener" and "information_schema":
//! - The per-txn snapshot age comes from our `RdbTransaction` registry walk
//!   (`crate::ha_rocksdb_cc__Rdb_transaction::walk_tx_list`).
//! - The deadlock buffer comes from our ring buffer of `slatedb::ErrorKind::Transaction`
//!   conflicts caught at `DbTransaction::commit()` time (replaces RocksDB's
//!   `GetDeadlockInfoBuffer`). See `rdb_global_h::DeadlockInfo`.
//!
//! Per _DESIGN.md §0, **`DbMetadataOps::subscribe()`** is the SlateDB
//! analogue for status changes; this class doesn't need to subscribe (it's
//! invoked synchronously per `SHOW STATUS`), but it pulls the latest
//! `DbStatus` snapshot in `populate_db_status` to display
//! `durable_seq`/manifest age.
//!
//! ## Out-of-scope methods
//! - `populate_deadlock_buffer` returning the C++ `rocksdb::DeadlockPath`
//!   structure verbatim — replaced by our `rdb_global_h::DeadlockInfo`.

use slatedb::Error;

use crate::ha_rocksdb_cc__Rdb_transaction::{RdbTransaction, TxListWalker};
use crate::rdb_global_h::DeadlockInfo;

/// Accumulates a human-readable "SHOW ENGINE ROCKSDB STATUS" snapshot from
/// the live txn list and the deadlock-history ring buffer.
pub struct RdbSnapshotStatus {
    /// Growing text buffer. Header is set in `new`; footer appended by `result`.
    pub data: String,
}

impl RdbSnapshotStatus {
    /// Construct with the timestamped header pre-written.
    /// Original: ha_rocksdb.cc:4413 — constructor.
    pub fn new() -> Self {
        let mut data = String::new();
        data.push_str(&Self::header());
        Self { data }
    }

    /// Append the footer, return the assembled string.
    /// Original: ha_rocksdb.cc:4415 — `getResult`.
    pub fn result(mut self) -> String {
        self.data.push_str(Self::footer());
        self.data
    }

    fn header() -> String {
        let ts = chrono_format_now_utc();
        format!(
            "\n============================================================\n\
             {ts} SLATEDB TRANSACTION MONITOR OUTPUT\n\
             ============================================================\n\
             ---------\nSNAPSHOTS\n---------\n\
             LIST OF SNAPSHOTS FOR EACH SESSION:\n"
        )
    }

    fn footer() -> &'static str {
        "-----------------------------------------\n\
         END OF SLATEDB TRANSACTION MONITOR OUTPUT\n\
         =========================================\n"
    }

    /// Pull current `DbStatus` (durable_seq, manifest snapshot age) and append
    /// a summary line. SlateDB-specific addition; no MyRocks counterpart.
    pub async fn append_db_status(&mut self, _db: &slatedb::Db) -> Result<(), Error> {
        todo!("fetch DbStatus via db.subscribe() borrow_and_update, format and push")
    }

    /// Append the deadlock-history ring buffer in MyRocks' DEADLOCK PATH /
    /// TRANSACTION ID / etc. format. Reads from our engine-side ring buffer.
    /// Original: ha_rocksdb.cc:4443 — `populate_deadlock_buffer`.
    pub fn populate_deadlock_buffer(&mut self, _deadlocks: &[DeadlockInfo]) {
        todo!("for each DeadlockInfo: append the 'DEADLOCK PATH' formatted block")
    }

    /// Return only the structured deadlock info (no text rendering).
    /// Original: ha_rocksdb.cc:4484 — `get_deadlock_info`.
    pub fn deadlock_info(&self) -> Vec<DeadlockInfo> {
        todo!("snapshot the engine-side deadlock ring buffer into a Vec")
    }
}

impl TxListWalker for RdbSnapshotStatus {
    /// One row in the SNAPSHOTS section. Mirrors ha_rocksdb.cc:4419 exactly:
    /// "---SNAPSHOT, ACTIVE %lld sec" + thread security context + lock/write
    /// counts.
    fn process_tran(&mut self, _tx: &dyn RdbTransaction) {
        todo!("compute snapshot age (now - snapshot_acquire_ts); format and append")
    }
}

impl Default for RdbSnapshotStatus {
    fn default() -> Self { Self::new() }
}

/// Lightweight timestamp formatter — kept here to avoid a chrono dep at this
/// stub level. Real impl uses `chrono::Utc::now().format("%Y-%m-%d %H:%M:%S")`.
fn chrono_format_now_utc() -> String {
    todo!("format SystemTime::now() as %Y-%m-%d %H:%M:%S — replace with chrono in TRANSLATE")
}
