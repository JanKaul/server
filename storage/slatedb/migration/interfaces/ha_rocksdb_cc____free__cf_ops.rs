//! Interface stub for `ha_rocksdb_cc____free__cf_ops`.
//!
//! C++ source: `storage/rocksdb/ha_rocksdb.cc` (lines 287..490 + 2163..2205, ~250 LoC).
//! v4 manifest sub-unit: `ha_rocksdb_cc____free__cf_ops`
//!
//! ## Mapping
//! Admin operations that target a column family (or "all CFs"):
//!   - `rocksdb_force_flush_memtable_now` — `SET GLOBAL rocksdb_force_flush_memtable_now = 1`
//!   - `rocksdb_force_flush_memtable_and_lzero_now` — same + L0 compact
//!   - `rocksdb_flush_all_memtables` — internal helper
//!   - `rocksdb_compact_column_family` — `SET GLOBAL rocksdb_compact_cf = 'name'`
//!   - `rocksdb_create_checkpoint` — `SET GLOBAL rocksdb_create_checkpoint = '/path'`
//!   - `rocksdb_delete_column_family` (currently stubbed to FAILURE in C++)
//!   - `rocksdb_remove_mariabackup_checkpoint`
//!
//! Per _DESIGN.md §1:
//!   - "Column families" → key-prefix scheme; there is only ONE SlateDB `Db`
//!     instance. Anything that took a `cfh` becomes a no-op or a
//!     prefix-targeted operation.
//!   - "Block cache / Bloom / Compression" rows: all global, not per-CF.
//!   - Flush: `Db::flush_with_options(FlushType::MemTable)`.
//!   - Checkpoint: `Db::create_checkpoint(opts)`.
//!   - Manual compaction: queue onto `RdbManualCompactionThread` (see that stub).
//!
//! ## Out-of-scope methods
//! - `rocksdb_delete_column_family` — already returns FAILURE in C++ pending
//!   race resolution; we keep that contract.
//! - `rocksdb_remove_mariabackup_checkpoint` — MariaBackup integration is
//!   non-goal for Stage 1 (S3 bucket replication is the per-_DESIGN.md path).

use slatedb::Error;
use std::sync::Arc;

/// Flush ALL CFs (i.e. flush the single SlateDB memtable).
/// Original: ha_rocksdb.cc:287 — `rocksdb_flush_all_memtables`.
pub async fn flush_all_memtables(db: &slatedb::Db) -> Result<(), Error> {
    db.flush_with_options(slatedb::FlushOptions { flush_type: slatedb::FlushType::MemTable })
        .await
}

/// `force_flush_memtable_now` admin op. Returns `Ok(())` on success.
/// Original: ha_rocksdb.cc:425 — `rocksdb_force_flush_memtable_now`.
pub async fn force_flush_memtable_now(db: &slatedb::Db) -> Result<(), Error> {
    flush_all_memtables(db).await
}

/// `force_flush_memtable_and_lzero_now` — flush + force a compaction of L0
/// files. Per _DESIGN.md, we don't drive SlateDB's compactor directly; we
/// flush the memtable and let the native compactor pick up the new SST.
///
/// MyRocks' "compact L0 explicitly" semantics is **lost**; we degrade to the
/// flush + best-effort wait pattern. Document in the warning log line.
///
/// Original: ha_rocksdb.cc:438.
pub async fn force_flush_memtable_and_lzero_now(db: &slatedb::Db) -> Result<(), Error> {
    flush_all_memtables(db).await?;
    todo!("log: 'L0 compaction is implicit on SlateDB; flush requested.'; return Ok(())")
}

/// `SET GLOBAL rocksdb_compact_cf = 'name'`. Enqueues a manual compaction
/// onto `RdbManualCompactionThread`. With our prefix scheme, `cf_name` maps
/// to a `cf_id` via the cf-manager, and we issue a flush — the actual
/// compaction is implicit (per `force_flush_memtable_and_lzero_now`).
///
/// Returns `mc_id` (>0) so the caller can poll for completion via
/// `is_manual_compaction_finished`. Returns `Err(Invalid)` if the CF is
/// unknown or the queue is full.
///
/// Original: ha_rocksdb.cc:2163 — `rocksdb_compact_column_family`.
pub fn compact_column_family(
    cf_name: &str,
    _mc_thread: &crate::ha_rocksdb_cc__Rdb_manual_compaction_thread::RdbManualCompactionThread,
    _concurrency: i32,
) -> Result<i32, Error> {
    let _ = cf_name;
    todo!(
        "1) cf_id = cf_manager.lookup_or_error(cf_name)?;\n\
         2) mc_id = mc_thread.request_manual_compaction(cf_id, None, None, concurrency, max_pending);\n\
         3) if mc_id < 0 → Err(invalid('queue full')); else Ok(mc_id)"
    )
}

/// `SET GLOBAL rocksdb_create_checkpoint = '/path'`. Creates a SlateDB
/// checkpoint (manifest snapshot usable as a backup root, per _DESIGN.md §1
/// "myrocks_hotbackup" row).
///
/// `path` is interpreted relative to the SlateDB store root if not absolute.
///
/// Original: ha_rocksdb.cc:374 — `rocksdb_create_checkpoint`.
pub async fn create_checkpoint(db: &slatedb::Db, path: &str) -> Result<(), Error> {
    let _ = (db, path);
    todo!(
        "let opts = CheckpointOptions { … };\n\
         let result = db.create_checkpoint(opts).await?;\n\
         // result holds manifest_id; log it for the operator"
    )
}

/// `SET GLOBAL rocksdb_drop_index_wakeup_thread = 1`. Wakes the drop-index
/// thread for immediate processing.
/// Original: ha_rocksdb.cc:2215.
pub fn drop_index_wakeup_thread(
    thread: &crate::ha_rocksdb_cc__Rdb_drop_index_thread::RdbDropIndexThread,
    requested: bool,
) {
    if requested { thread.signal(); }
}

/// Non-goal — kept to preserve the surface. Per the C++ comment at
/// ha_rocksdb.cc:298, this returns FAILURE pending race-condition resolution;
/// we preserve that contract.
pub fn delete_column_family(_cf_name: &str) -> Result<(), Error> {
    Err(Error::invalid(
        "DELETE COLUMN FAMILY is not supported (race condition pending)".into(),
    ))
}

/// Non-goal per _DESIGN.md — MariaBackup uses S3 bucket replication instead.
pub fn remove_mariabackup_checkpoint(_path: &str) -> Result<(), Error> {
    Err(Error::invalid(
        "MariaBackup checkpoint removal is non-goal; use object-store replication tools".into(),
    ))
}

/// Reference-only: held by free fns above. Used so we can pass `&Db` from
/// any sysvar callback that has the engine handle.
pub type DbRef = Arc<slatedb::Db>;
