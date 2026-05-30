//! Interface stub for `ha_slatedb_binlog_cc____free__write_direct` (NEW unit).
//!
//! C++ source: `sql/handler.h` slot pair `binlog_write_direct_ordered` /
//! `binlog_write_direct` (lines 1609..1614). Coordinator call sites:
//! `sql/log.cc:9009` + `:9020` (normal commit), `:5070..5077`
//! (FormatDescription on engine open), `:10621..10683` and `:11128`
//! (group commit fallback). Always paired: `_ordered` under
//! `LOCK_commit_ordered`, then non-ordered without the lock.
//!
//! v4 manifest sub-unit: NEW — added 2026-05-29 for the SlateDB-as-binlog
//! interface (`_DESIGN.md §14`).
//!
//! ## Mapping
//!
//! Per `_DESIGN.md §14`:
//! - `_ordered` runs serialized under `LOCK_commit_ordered`. It assigns
//!   the (file_no, offset) for this event group, writes them back into
//!   `info.out_file_no`/`info.out_offset`, and stages the chunk bytes
//!   into a per-thread Rust-side buffer. **No SlateDB I/O happens
//!   here** — we want the global lock held for as short as possible.
//! - Non-ordered runs without the lock. Pulls the staged bytes, builds
//!   them into chunked `binlog:<file_no>:<offset>` KV pairs, and issues
//!   the writes via the shared `Db` handle. Writes are
//!   `await_durable=false` — the actual fsync happens at
//!   `binlog_group_commit_ordered` (tail-only).
//!
//! `gtid` is non-NULL for any event group that the coordinator wants
//! tagged with a specific GTID; we write it as the FIRST chunk of the
//! group (matching the IO_CACHE layout: GTID logically first, physically
//! at the cache tail per `EventGroupInfo::gtid_offset`).
//!
//! ## Out-of-scope methods
//! None — both slots in scope.

use crate::ha_slatedb_binlog_h__types::{
    BinlogEventGroupInfo, IoCacheRef, RplGtid,
};
use slatedb::Error;

/// Coordinator entry point: ordered half of a non-commit-ordered event
/// group write. Runs under `LOCK_commit_ordered` — **sync** because
/// no I/O happens here (we only stage bytes into a thread-local buffer).
///
/// Original: `sql/handler.h:1609..1611` —
/// `bool (*binlog_write_direct_ordered)(IO_CACHE *cache,
///         handler_binlog_event_group_info *binlog_info,
///         const rpl_gtid *gtid);`
pub fn binlog_write_direct_ordered(
    _cache: &mut IoCacheRef,
    _info: &mut BinlogEventGroupInfo,
    _gtid: Option<&RplGtid>,
) -> Result<(), Error> {
    todo!(
        "1. Acquire next (file_no, offset) from the rotation counter (under our internal mutex).\n\
         2. Set info.out_file_no / info.out_offset.\n\
         3. Stage cache bytes + GTID into the thread-local pending buffer.\n\
         4. NO SlateDB writes here — return ASAP to release LOCK_commit_ordered."
    )
}

/// Coordinator entry point: non-ordered half. Runs without
/// `LOCK_commit_ordered`. Pulls the staged buffer and writes it.
///
/// Original: `sql/handler.h:1612..1614` —
/// `bool (*binlog_write_direct)(IO_CACHE *cache,
///         handler_binlog_event_group_info *binlog_info,
///         const rpl_gtid *gtid);`
pub async fn binlog_write_direct(
    _cache: &mut IoCacheRef,
    _info: &mut BinlogEventGroupInfo,
    _gtid: Option<&RplGtid>,
) -> Result<(), Error> {
    todo!(
        "1. Take the staged buffer for this thread.\n\
         2. Split into chunks: ChunkType::Commit, sequential file_no/offset.\n\
         3. WriteBatch ← put(BinlogKey, value) for each chunk; await_durable=false.\n\
         4. The fsync happens later in binlog_group_commit_ordered (tail-only)."
    )
}
