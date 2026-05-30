//! Interface stub for `ha_slatedb_binlog_cc____free__admin` (NEW unit).
//!
//! C++ source: `sql/handler.h` slots:
//! - `binlog_status`           (line  1711)
//! - `get_filename`            (line  1713)
//! - `get_binlog_file_list`    (line  1715)
//! - `binlog_flush`            (line  1720)
//! - `binlog_get_init_state`   (line  1728)
//! - `reset_binlogs`           (line  1730)
//! - `binlog_purge`            (line  1737)
//!
//! Coordinator call sites: `sql/sql_repl.cc:5373` (`SHOW MASTER STATUS`),
//! `:1183` (`SHOW BINARY LOGS`), `:812`/`:879`/`sql/log.cc:6549`
//! (`PURGE BINARY LOGS`); `sql/log.cc:9654..9697` (`FLUSH BINARY LOGS`),
//! `:5728..5747` (`RESET MASTER`), `:9629` (`binlog_get_init_state`).
//!
//! v4 manifest sub-unit: NEW — added 2026-05-29 for the SlateDB-as-binlog
//! interface (`_DESIGN.md §14`).
//!
//! ## Mapping
//!
//! Per `_DESIGN.md §14`: virtual files map to key-range partitions.
//! Each `file_no` owns the prefix `binlog:<file_no>:*`. Rotation =
//! bump the `binlog_meta:rotation` counter and start writing under
//! the new `file_no`. Purge = SlateDB `delete_range` on the old prefix.
//!
//! Locking contracts (from coordinator-flow analysis):
//! - `binlog_status`: called inside `LOCK_log` for `SHOW MASTER STATUS`
//!   and inside `LOCK_commit_ordered` for IS PROCESSLIST snapshot;
//!   must be sync and fast.
//! - `binlog_flush`: holds `LOCK_log` + `LOCK_commit_ordered` across
//!   the call; must rotate atomically.
//! - `binlog_get_init_state`: holds `LOCK_commit_ordered`.
//! - `binlog_purge`: no special lock held; engine decides what to purge.
//!
//! `binlog_get_init_state` reads the GTID state from the inline
//! `ChunkType::GtidState` chunk at the start of the earliest non-purged
//! file (per the locked-in design decision: GTID state lives inline,
//! not in separate keys).
//!
//! ## Out-of-scope methods
//! None — all seven slots scoped here.

use crate::ha_slatedb_binlog_h__types::{
    BinlogFileEntry, BinlogPurgeInfo, RplBinlogState,
};
use slatedb::Error;

// ---------------------------------------------------------------------------
// SHOW MASTER STATUS support — must be sync (under LOCK_log or LOCK_commit_ordered)
// ---------------------------------------------------------------------------

/// Coordinator entry point: current write position. Sync, fast.
///
/// Original: `sql/handler.h:1711` —
/// `void (*binlog_status)(uint64_t *out_fileno, uint64_t *out_pos);`
pub fn binlog_status(_out_fileno: &mut u64, _out_pos: &mut u64) {
    todo!(
        "*_out_fileno = ROTATION_COUNTER.load(Relaxed);\n\
         *_out_pos    = NEXT_OFFSET.load(Relaxed);"
    )
}

/// Coordinator entry point: virtual filename for a file_no. Sync.
///
/// Original: `sql/handler.h:1713` —
/// `void (*get_filename)(char name[FN_REFLEN], uint64_t file_no);`
pub fn get_filename(_name: &mut [u8], _file_no: u64) {
    todo!("write 'slatedb-bin.{:06}' into name (null-terminated)")
}

// ---------------------------------------------------------------------------
// SHOW BINARY LOGS
// ---------------------------------------------------------------------------

/// Coordinator entry point: enumerate active binlog files. Async (does
/// a metadata range scan).
///
/// Original: `sql/handler.h:1715` —
/// `binlog_file_entry * (*get_binlog_file_list)(MEM_ROOT *mem_root);`
///
/// Server fills in `size` on each entry from `os.stat()`-equivalent
/// information; we only set `file_no` and `name`.
pub async fn get_binlog_file_list() -> Result<Vec<BinlogFileEntry>, Error> {
    todo!(
        "1. Scan binlog_meta:file_index:* keys (one per active file_no).\n\
         2. For each: emit BinlogFileEntry { file_no, name: format!('slatedb-bin.{:06}') }.\n\
         3. Sorted ascending by file_no."
    )
}

// ---------------------------------------------------------------------------
// FLUSH BINARY LOGS — rotation
// ---------------------------------------------------------------------------

/// Coordinator entry point: rotate to a new binlog file.
/// Holds `LOCK_log` + `LOCK_commit_ordered`.
///
/// Original: `sql/handler.h:1720` —
/// `bool (*binlog_flush)();`
///
/// Atomic rotation steps:
/// 1. Write a `ChunkType::Dummy` chunk at the tail of the current file
///    to mark end-of-file (helps reader robustness).
/// 2. Write a `ChunkType::GtidState` snapshot.
/// 3. Bump `binlog_meta:rotation` to `file_no + 1`.
/// 4. Insert `binlog_meta:file_index:<new_file_no>` metadata key.
/// 5. One `WriteBatch` with `await_durable=true` for all four.
pub async fn binlog_flush() -> Result<(), Error> {
    todo!(
        "Atomic rotation per the contract above; one WriteBatch with await_durable=true."
    )
}

// ---------------------------------------------------------------------------
// FLUSH BINARY LOGS DELETE_DOMAIN_ID validation
// ---------------------------------------------------------------------------

/// Coordinator entry point: GTID state at the start of the earliest
/// non-purged file. Holds `LOCK_commit_ordered`.
///
/// Original: `sql/handler.h:1728` —
/// `bool (*binlog_get_init_state)(rpl_binlog_state_base *out_state);`
pub async fn binlog_get_init_state(_out_state: &mut RplBinlogState) -> Result<(), Error> {
    todo!(
        "1. Read binlog_meta:rotation to find earliest active file_no.\n\
         2. Scan binlog:<earliest_file>:* until the first ChunkType::GtidState chunk.\n\
         3. Parse the chunk's payload (RplGtid triples) into out_state.entries."
    )
}

// ---------------------------------------------------------------------------
// RESET MASTER
// ---------------------------------------------------------------------------

/// Coordinator entry point: erase all binlog data and reset to file_no 0.
/// Holds `LOCK_log` + `LOCK_index` + `LOCK_commit_ordered`.
///
/// Original: `sql/handler.h:1730` —
/// `bool (*reset_binlogs)();`
pub async fn reset_binlogs() -> Result<(), Error> {
    todo!(
        "1. Db::delete_range(b\"binlog:\", b\"binlog;\")  // delete all binlog data\n\
         2. Db::delete_range(b\"binlog_meta:\", b\"binlog_meta;\")  // and metadata\n\
         3. Write binlog_meta:rotation = 0 fresh.\n\
         4. One WriteBatch with await_durable=true."
    )
}

// ---------------------------------------------------------------------------
// PURGE BINARY LOGS
// ---------------------------------------------------------------------------

/// Coordinator entry point: purge files up to a limit. No special
/// locks held by the coordinator.
///
/// Original: `sql/handler.h:1737` —
/// `int (*binlog_purge)(handler_binlog_purge_info *purge_info);`
///
/// Returns 0 on success, `LOG_INFO_*` error code on failure
/// (mapped from `slatedb::Error` at the cxx boundary).
pub async fn binlog_purge(_purge_info: &mut BinlogPurgeInfo) -> Result<i32, Error> {
    todo!(
        "1. Determine the cutoff file_no based on purge_info.purge_by_{date,size,name}\n\
              + purge_info.limit_file_no (the earliest in-use file by any dump thread).\n\
         2. If cutoff < purge_info.limit_file_no: set nonpurge_reason and bail.\n\
         3. Else: for each file_no < cutoff: Db::delete_range(binlog:<n>:, binlog:<n>;)\n\
              + Db::delete(binlog_meta:file_index:<n>).\n\
         4. One WriteBatch; await_durable=true.\n\
         5. Return 0."
    )
}
