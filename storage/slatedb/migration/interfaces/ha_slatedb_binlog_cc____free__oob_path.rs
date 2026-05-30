//! Interface stub for `ha_slatedb_binlog_cc____free__oob_path` (NEW unit).
//!
//! C++ source: `sql/handler.h` slots:
//! - `binlog_oob_data_ordered` / `binlog_oob_data`         (lines 1634..1639)
//! - `binlog_savepoint_rollback`                           (lines 1650..1652)
//! - `binlog_oob_reset`                                    (line  1658)
//! - `binlog_oob_free`                                     (line  1660)
//!
//! Coordinator call sites: `sql/log.cc:7446..7498` (OOB spill during a
//! large transaction); `sql/log_cache.h:148..173` (`reset_for_engine_binlog`,
//! calls `oob_reset`); `sql/log_cache.h:69..71` (cache destructor calls
//! `oob_free`); savepoint paths in `sql/handler.cc:1999` and `sql/log.cc:2564`.
//!
//! v4 manifest sub-unit: NEW — added 2026-05-29 for the SlateDB-as-binlog
//! interface (`_DESIGN.md §14`).
//!
//! ## Mapping
//!
//! Per `_DESIGN.md §14`: OOB chunks are written **directly into the
//! `binlog:` namespace** as the transaction executes — with
//! `PutOptions::ttl = ExpireAfter(BINLOG_OOB_TTL_SECS)` so that
//! orphans from uncommitted transactions self-expire. The commit-time
//! `WriteBatch` (in `group_commit.rs`) re-puts the chunks **without**
//! TTL, making them permanent. No GC pass needed.
//!
//! This is structurally different from InnoDB's Zeckendorf binary-tree
//! forest: on a KV substrate, range-scanning sequential offsets is
//! cheap, so the tree's log-N seek isn't needed.
//!
//! **Savepoint rollback** is supported (unlike data-side savepoints,
//! which are stubbed per Q10): the OOB-side savepoint just needs to
//! issue a SlateDB delete-range on `binlog:<file>:<savepoint_offset>..`,
//! which the TTL pre-commit-only semantic makes safe.
//!
//! ## Out-of-scope methods
//! None — all five OOB-related slots are scoped here.

use crate::ha_slatedb_binlog_h__types::{
    EngineDataPtr, SavepointHandle, ThdRef,
};
use bytes::Bytes;
use slatedb::Error;

/// Coordinator entry point: ordered half of an OOB spill. Runs under
/// `LOCK_commit_ordered` — **sync** because no I/O happens here (we
/// only stage into a thread-local buffer).
///
/// May also be called with `data_len == 0` purely to set savepoints
/// (when either savepoint param is non-None).
///
/// `engine_data` is `&mut Option<Box<EngineDataPtr>>` so the engine
/// can null/replace the pointer (matches the C++ `void **engine_data`
/// double-indirection contract).
///
/// Original: `sql/handler.h:1634..1637` —
/// `bool (*binlog_oob_data_ordered)(THD*, const unsigned char*, size_t,
///         void **engine_data, void **stmt_start_data, void **savepoint_data);`
pub fn binlog_oob_data_ordered(
    _thd: ThdRef,
    _data: Bytes,
    _engine_data: &mut Option<Box<EngineDataPtr>>,
    _stmt_start_data: Option<&mut SavepointHandle>,
    _savepoint_data: Option<&mut SavepointHandle>,
) -> Result<(), Error> {
    todo!(
        "1. If engine_data is None: allocate Box<EngineDataPtr::default()>.\n\
         2. If data_len > 0: stage (chunk_type=OobData, payload) into thread-local buffer.\n\
         3. If stmt_start_data: set *stmt_start_data = SavepointHandle{current_file_no, current_offset}.\n\
         4. If savepoint_data: same for savepoint slot.\n\
         5. NO SlateDB I/O — happens in non-ordered companion."
    )
}

/// Coordinator entry point: non-ordered half. Runs without
/// `LOCK_commit_ordered`. Issues the actual TTL-stamped writes.
///
/// Original: `sql/handler.h:1638..1639` —
/// `bool (*binlog_oob_data)(THD*, const unsigned char*, size_t,
///         void **engine_data);`
pub async fn binlog_oob_data(
    _thd: ThdRef,
    _data: Bytes,
    _engine_data: &mut Option<Box<EngineDataPtr>>,
) -> Result<(), Error> {
    todo!(
        "1. Take staged chunks for this thread.\n\
         2. For each: put(BinlogKey{file_no, offset}, value)\n\
              with PutOptions::ttl = ExpireAfter(BINLOG_OOB_TTL_SECS).\n\
         3. Update engine_data.current_offset.\n\
         4. await_durable=false; commit-time WriteBatch re-puts without TTL."
    )
}

/// Coordinator entry point: roll back OOB writes to a previously set
/// savepoint. Exactly one of `stmt_start_data` / `savepoint_data` is
/// non-None per call.
///
/// `engine_data` is `&mut Option<Box<EngineDataPtr>>` for the same
/// reason as the OOB slots (C++ `void **engine_data`).
///
/// Original: `sql/handler.h:1650..1652` —
/// `void (*binlog_savepoint_rollback)(THD*, void **engine_data,
///         void **stmt_start_data, void **savepoint_data);`
pub fn binlog_savepoint_rollback(
    _thd: ThdRef,
    _engine_data: &mut Option<Box<EngineDataPtr>>,
    _stmt_start_data: Option<&mut SavepointHandle>,
    _savepoint_data: Option<&mut SavepointHandle>,
) {
    todo!(
        "1. Determine target = stmt_start_data or savepoint_data (whichever is Some).\n\
         2. Discard any thread-local staged chunks above target.\n\
         3. Issue Db::delete on binlog:<file>:<offset> keys > target if any\n\
              were already written by binlog_oob_data — they have TTL so\n\
              delete is cheap and crash-safe even without await_durable.\n\
         4. Reset engine_data.current_offset = target.offset."
    )
}

/// Coordinator entry point: reset between transactions on the same
/// cache. Engine may keep the allocation and re-init, or replace.
///
/// Original: `sql/handler.h:1658` —
/// `void (*binlog_oob_reset)(void **engine_data);`
pub fn binlog_oob_reset(_engine_data: &mut Option<Box<EngineDataPtr>>) {
    todo!(
        "1. If Some, take and clear fields (reuse the Box for the next txn).\n\
         2. Or: set to None — coordinator handles re-alloc on first oob_data call."
    )
}

/// Coordinator entry point: free the `engine_data` allocation. Called
/// from `binlog_cache_data::~binlog_cache_data` (`sql/log_cache.h:69`)
/// when the per-thread cache is destroyed, and at XA recovery rollback
/// (`sql/handler.cc:2903`).
///
/// Original: `sql/handler.h:1660` —
/// `void (*binlog_oob_free)(void *engine_data);`
pub fn binlog_oob_free(_engine_data: Option<Box<EngineDataPtr>>) {
    // Drop is the entire implementation: Box::drop reclaims the heap.
    // No external resources to release in Stage 1 (TTL on the SlateDB
    // side handles orphan-chunk cleanup automatically).
}
