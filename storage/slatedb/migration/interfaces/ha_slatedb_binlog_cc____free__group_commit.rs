//! Interface stub for `ha_slatedb_binlog_cc____free__group_commit` (NEW unit).
//!
//! C++ source: `sql/handler.h` slot `binlog_group_commit_ordered`
//! (lines 1619..1620). Coordinator call site: `sql/log.cc:10690`,
//! invoked by the group-commit leader for the **tail entry only**, with
//! **no locks held**.
//!
//! v4 manifest sub-unit: NEW — added 2026-05-29 for the SlateDB-as-binlog
//! interface (`_DESIGN.md §14`).
//!
//! ## Mapping
//!
//! Per `_DESIGN.md §14`: **this is the only durability site in the
//! entire binlog write path.** Everything upstream
//! (`binlog_write_direct*`, `binlog_oob_data*`) writes with
//! `await_durable=false` to the shared SlateDB `Db`. At the tail of a
//! commit group, the coordinator fires this method once on the
//! representative thread; we issue **one** `Db::flush_with_options` or
//! a final marker `WriteBatch` with `await_durable=true` and a fresh
//! `FlushOptions { flush_type: Wal }`.
//!
//! This single fsync covers every transaction in the just-finished
//! commit group — that's the "halve the fsyncs" claim from the article.
//! Combined with the data engine's own commit ordering (which sits on
//! the same `Db`), a normal commit costs one fsync end-to-end.
//!
//! ## Out-of-scope methods
//! None — single-slot file.

use crate::ha_slatedb_binlog_h__types::{BinlogEventGroupInfo, ThdRef};

/// Coordinator entry point: post-write durability fence for the
/// commit group. Tail-only, no locks held.
///
/// Original: `sql/handler.h:1619..1620` —
/// `void (*binlog_group_commit_ordered)(THD *thd,
///         handler_binlog_event_group_info *binlog_info);`
///
/// Contract:
/// - Returns void — failure here is `tracing::error!`-logged and the
///   server retries on next commit. There's no error path on the call
///   site (the coordinator has already returned commit success to
///   waiters; backing out is not an option).
/// - On a graceful crash the un-fsynced writes are lost — the next
///   server start will recover via the redo log and replay forward
///   to `last_durable_seq` (which is what `wait_durable=true` readers
///   already observe).
pub async fn binlog_group_commit_ordered(
    _thd: ThdRef,
    _info: &mut BinlogEventGroupInfo,
) {
    todo!(
        "1. db.flush_with_options(FlushOptions { flush_type: FlushType::Wal }).await\n\
         2. On Err: tracing::error!(?e, file=info.out_file_no, off=info.out_offset,\n\
              'binlog_group_commit_ordered: WAL flush failed').\n\
         3. Notify the durable-seq watch channel so wait_durable readers wake."
    )
}
