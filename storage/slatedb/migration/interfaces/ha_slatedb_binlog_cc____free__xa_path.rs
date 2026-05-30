//! Interface stub for `ha_slatedb_binlog_cc____free__xa_path` (NEW unit).
//!
//! C++ source: `sql/handler.h` slots:
//! - `binlog_write_xa_prepare_ordered` / `binlog_write_xa_prepare`
//!     (lines 1676..1679)
//! - `binlog_xa_rollback_ordered` / `binlog_xa_rollback`
//!     (lines 1684..1686)
//! - `binlog_unlog`                                       (line  1697)
//!
//! Coordinator call sites: `sql/log.cc:2620..2624` (XA PREPARE),
//! `:2405..2409` (XA ROLLBACK), `:2935`/`:2956`/`:2976`/`:2996`/`:13257`
//! (`binlog_unlog`).
//!
//! v4 manifest sub-unit: NEW — added 2026-05-29 for the SlateDB-as-binlog
//! interface (`_DESIGN.md §14`).
//!
//! ## Mapping
//!
//! Per `_DESIGN.md §14`: **all five slots are Stage 2 stubs.** User-XA
//! is not part of the initial cut. The XA recovery scan in
//! `lifecycle::binlog_init` returns an empty `recover_xid_hash` in
//! Stage 1, which is correct for "no user-XA support."
//!
//! **Internal 2PC (binlog ↔ data engine for normal commits) does NOT
//! go through these slots.** Per the coordinator-flow analysis, normal
//! commits use `binlog_write_direct[_ordered]` only; the XA slots are
//! invoked solely from `SQLCOM_XA_PREPARE` / `SQLCOM_XA_COMMIT` /
//! `SQLCOM_XA_ROLLBACK` SQL paths. So stubbing these does NOT break
//! the normal commit path.
//!
//! `binlog_unlog` is the one slot that IS called for internal 2PC too
//! (after the data engine commits). In Stage 1 we make it a sync no-op:
//! we don't persist XID markers for internal 2PC because we don't need
//! to (the data engine already records the commit; recovery doesn't
//! need cross-engine reconciliation in a single-engine deployment).
//!
//! ## Out-of-scope methods
//! - All four async XA slots: return `Err(Error::invalid("non-goal:
//!   user-XA binlog (Stage 2)"))`. This maps to `HA_ERR_WRONG_COMMAND`
//!   via the standard error translation.
//! - `binlog_unlog`: sync no-op in Stage 1 (see above).

use crate::ha_slatedb_binlog_h__types::{
    BinlogEventGroupInfo, EngineDataPtr, ThdRef, XidBytes,
};
use slatedb::Error;

/// Coordinator entry point: ordered half of user-XA PREPARE write.
/// **Stage 2 stub.**
///
/// Original: `sql/handler.h:1676..1677`.
pub async fn binlog_write_xa_prepare_ordered(
    _thd: ThdRef,
    _info: &mut BinlogEventGroupInfo,
    _engine_count: u8,
) -> Result<(), Error> {
    Err(Error::invalid(
        "binlog_write_xa_prepare_ordered: user-XA binlog not supported in Stage 1 (Stage 2 feature)"
            .into(),
    ))
}

/// Coordinator entry point: non-ordered half. **Stage 2 stub.**
///
/// Original: `sql/handler.h:1678..1679`.
pub async fn binlog_write_xa_prepare(
    _thd: ThdRef,
    _info: &mut BinlogEventGroupInfo,
    _engine_count: u8,
) -> Result<(), Error> {
    Err(Error::invalid(
        "binlog_write_xa_prepare: user-XA binlog not supported in Stage 1".into(),
    ))
}

/// Coordinator entry point: ordered half of user-XA ROLLBACK.
/// **Stage 2 stub.**
///
/// Original: `sql/handler.h:1684..1685`.
pub async fn binlog_xa_rollback_ordered(
    _thd: ThdRef,
    _xid: &XidBytes,
    _engine_data: &mut Option<Box<EngineDataPtr>>,
) -> Result<(), Error> {
    Err(Error::invalid(
        "binlog_xa_rollback_ordered: user-XA binlog not supported in Stage 1".into(),
    ))
}

/// Coordinator entry point: non-ordered half. **Stage 2 stub.**
///
/// Original: `sql/handler.h:1686`.
pub async fn binlog_xa_rollback(
    _thd: ThdRef,
    _xid: &XidBytes,
    _engine_data: &mut Option<Box<EngineDataPtr>>,
) -> Result<(), Error> {
    Err(Error::invalid(
        "binlog_xa_rollback: user-XA binlog not supported in Stage 1".into(),
    ))
}

/// Coordinator entry point: post-commit release marker. Called for
/// BOTH internal 2PC and user-XA flows.
///
/// Original: `sql/handler.h:1697` —
/// `void (*binlog_unlog)(const XID *xid, void **engine_data);`
///
/// Coordinator dispatch (per `sql/log.cc:13243..13263`,
/// `TC_LOG_BINLOG::unlog`):
/// - The cookie returned by `binlog_write_direct[_ordered]` controls
///   whether `binlog_unlog` fires for internal 2PC. Only when the
///   cookie carries `BINLOG_COOKIE_IS_ENGINE_UNLOG` does the
///   coordinator route the unlog through this slot.
/// - **Stage 1** `binlog_write_direct*` returns a cookie WITHOUT the
///   engine-unlog bit, so this slot is never invoked for normal
///   commits — purge progress is governed by dump-thread
///   `cur_file_no` (the standard `handler_binlog_purge_info::limit_file_no`
///   mechanism), not by XID release.
/// - **Stage 2** will set the cookie for user-XA commits and this slot
///   will need to delete the persisted `binlog_meta:xa:<xid>` marker
///   that `binlog_write_xa_prepare` wrote.
///
/// Stage 1 implementation: no-op. The slot is unreachable from any
/// active Stage 1 code path; the no-op is a safety net rather than a
/// behavioural commitment.
pub fn binlog_unlog(_xid: &XidBytes, _engine_data: &mut Option<Box<EngineDataPtr>>) {
    // Stage 1: see module header.
}
