//! Interface stub for `ha_rocksdb_cc____free__status_helpers`.
//!
//! C++ source: `storage/rocksdb/ha_rocksdb.cc` (lines 4272..4326 + 4613..4810 + 13318..13396, ~270 LoC).
//! v4 manifest sub-unit: `ha_rocksdb_cc____free__status_helpers`
//!
//! ## Mapping
//! Helpers that aggregate counter state into the global `export_stats` /
//! `memory_stats` blobs that back the `SHOW STATUS` variable list, plus the
//! `rocksdb_show_status` handlerton hook that produces the `SHOW ENGINE
//! ROCKSDB STATUS` text.
//!
//! Three free functions in scope here:
//!   - `print_stats(thd, type, name, status, stat_print)` — wraps MariaDB's
//!     stat-print callback. The Rust side returns an opaque `Vec<StatusRow>`
//!     and the cxx shim feeds it back to MariaDB.
//!   - `format_string(format, ...)` — `vsnprintf`-style. Replaced by Rust's
//!     `format!()`; we just keep a thin wrapper for symmetry.
//!   - `myrocks_update_status` / `myrocks_update_memory_status` — snapshot
//!     the live counter values into the export blobs that the SHOW STATUS
//!     getters then read.
//!
//! Per _DESIGN.md §1 row "rdb_perf_context (Re-impl. Different shape; same
//! SHOW STATUS surface)" — we keep the SHOW STATUS variable names identical
//! and fill them from SlateDB metrics where possible, falling back to 0
//! where there's no equivalent (see `show_callbacks.rs`).
//!
//! ## Out-of-scope methods
//! - `show_myrocks_vars` / `show_rocksdb_stall_vars` — these are MariaDB
//!   `SHOW_FUNC` adapters; the bridging is in the cxx shim, not Rust.
//! - Direct `rocksdb::MemoryUtil::GetApproximateMemoryUsageByType` — SlateDB
//!   exposes memtable size via `DbStatus`/metrics, not via a separate util.

use slatedb::Error;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::ha_rocksdb_cc__Rdb_transaction::TxListWalker;
use crate::rdb_global_h::{ExportStats, MemoryStats};

/// Global row-op counter snapshot — populated by `update_status` from the
/// engine-side `GlobalStats`. The numeric SHOW STATUS getters read from
/// this struct via a `parking_lot::RwLock` (no poisoning, so `.write()`
/// is infallible — see §0.4).
pub static EXPORT_STATS: once_cell::sync::Lazy<parking_lot::RwLock<ExportStats>> =
    once_cell::sync::Lazy::new(|| parking_lot::RwLock::new(ExportStats::default()));

/// Memtable-usage snapshot. Populated from `DbStatus` once per status fetch.
pub static MEMORY_STATS: once_cell::sync::Lazy<parking_lot::RwLock<MemoryStats>> =
    once_cell::sync::Lazy::new(|| parking_lot::RwLock::new(MemoryStats::default()));

/// A single (type, name, status_body) row produced by `show_status`.
/// The cxx shim feeds these back to MariaDB's `stat_print_fn` one at a time.
#[derive(Debug, Clone)]
pub struct StatusRow {
    pub kind: String,   // "rocksdb" / "rocksdb_cf" / ...
    pub name: String,
    pub body: String,
}

/// Drain the global atomic counters into `EXPORT_STATS`. Called at the top of
/// every `SHOW STATUS` to guarantee freshness.
/// Original: ha_rocksdb.cc:13318 — `myrocks_update_status`.
pub fn update_status(global_rows: &[AtomicU64], global_system_rows: &[AtomicU64], queries: &[AtomicU64], covered: &AtomicU64) {
    // `parking_lot::RwLock` (not `std::sync::RwLock`) — no poisoning, so
    // `.write()` is infallible. Switched to parking_lot per §0.4 to drop
    // the `expect("...poisoned")` panic site.
    let mut s = EXPORT_STATS.write();
    s.rows_deleted = global_rows[0].load(Ordering::Relaxed);
    s.rows_inserted = global_rows[1].load(Ordering::Relaxed);
    s.rows_read = global_rows[2].load(Ordering::Relaxed);
    s.rows_updated = global_rows[3].load(Ordering::Relaxed);
    s.rows_deleted_blind = global_rows[4].load(Ordering::Relaxed);
    s.rows_expired = global_rows[5].load(Ordering::Relaxed);
    s.rows_filtered = global_rows[6].load(Ordering::Relaxed);
    s.rows_hidden_no_snapshot = global_rows[7].load(Ordering::Relaxed);

    s.system_rows_deleted = global_system_rows[0].load(Ordering::Relaxed);
    s.system_rows_inserted = global_system_rows[1].load(Ordering::Relaxed);
    s.system_rows_read = global_system_rows[2].load(Ordering::Relaxed);
    s.system_rows_updated = global_system_rows[3].load(Ordering::Relaxed);

    s.queries_point = queries[0].load(Ordering::Relaxed);
    s.queries_range = queries[1].load(Ordering::Relaxed);
    s.covered_secondary_key_lookups = covered.load(Ordering::Relaxed);
}

/// Snapshot SlateDB memtable usage into `MEMORY_STATS`. Reads from the latest
/// `DbStatus` (no separate util call needed — _DESIGN.md §0 exposes this).
/// Original: ha_rocksdb.cc:13339 — `myrocks_update_memory_status`.
pub async fn update_memory_status(_db: &slatedb::Db) -> Result<(), Error> {
    todo!("let status = db.status_borrow().await; populate memtable_total/unflushed; write to MEMORY_STATS")
}

/// `SHOW ENGINE ROCKSDB STATUS` body assembler. Drives `RdbSnapshotStatus`
/// via `walk_tx_list`, then converts to rows for the shim.
/// Original: ha_rocksdb.cc:4613 — `rocksdb_show_status`.
pub async fn show_status(_db: &slatedb::Db) -> Result<Vec<StatusRow>, Error> {
    let mut ss = crate::ha_rocksdb_cc__Rdb_snapshot_status::RdbSnapshotStatus::new();
    {
        // Walk via &mut TxListWalker
        let walker: &mut dyn TxListWalker = &mut ss;
        crate::ha_rocksdb_cc__Rdb_transaction::walk_tx_list(walker);
    }
    todo!(
        "ss.populate_deadlock_buffer(&deadlock_ring.snapshot());\n\
         ss.append_db_status(db).await?;\n\
         vec![StatusRow { kind: 'rocksdb'.into(), name: ''.into(), body: ss.result() }]"
    )
}

/// `print_stats` wrapper. The C++ version invokes the per-row callback
/// `stat_print(thd, kind, name, body)`; on the Rust side we just build a
/// `StatusRow` and let the shim iterate.
/// Original: ha_rocksdb.cc:4272.
pub fn build_stats_row(kind: &str, name: &str, body: &str) -> StatusRow {
    StatusRow { kind: kind.to_owned(), name: name.to_owned(), body: body.to_owned() }
}

/// vsnprintf-style. Provided for symmetry; callers should prefer `format!()`.
/// Original: ha_rocksdb.cc:4279.
pub fn format_string(args: std::fmt::Arguments) -> String { args.to_string() }
