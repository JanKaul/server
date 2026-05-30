//! Interface stub for `ha_rocksdb_cc____free__txn_handlers`.
//!
//! C++ source: `storage/rocksdb/ha_rocksdb.cc` (sub-unit span 94..5036)
//! v4 manifest sub-unit: `ha_rocksdb_cc____free__txn_handlers`
//!
//! ## Mapping
//! These are the MariaDB **handlerton** transaction callbacks — entry
//! points wired to the engine's `handlerton->{prepare, commit, rollback,
//! savepoint, recover, ...}` slots in `rocksdb_init_func`.
//!
//! Per _DESIGN.md §5 (transaction model) + §1 row "Two-phase commit":
//!   - Each callback resolves the per-THD `Rdb_transaction` (see
//!     `ha_rocksdb_cc__Rdb_transaction.rs`) via the THD's slot.
//!   - `prepare` delegates to `Rdb_transaction::prepare(xid)` which calls
//!     `Db::flush_with_options(FlushType::Wal)` + writes a marker into the
//!     system CF (no native SlateDB XA primitive).
//!   - `commit` / `rollback` delegate to `DbTransaction::commit/rollback`.
//!   - `savepoint` operates on the engine-side savepoint stack (§5) —
//!     SlateDB has no native savepoints.
//!   - `recover` reads back the XA prepare markers from the system CF and
//!     reconstructs `XID` rows.
//!
//! ## Out-of-scope methods
//! - `rocksdb_commit_ordered` — group-commit hook; MariaDB calls it before
//!   `commit` to assign commit order. SlateDB's `DbTransaction::commit`
//!   already serializes via the transaction manager, so this is a no-op.
//! - `rocksdb_checkpoint_request` — Galera/group-replication hook; not in
//!   scope for Stage 1 (per the migration doc §1).

use slatedb::Error;

use crate::ha_rocksdb_cc__Rdb_transaction::RdbTransaction;

// Opaque handler-thread handle. The cxx bridge translates `THD*` to `OpaqueThd`;
// internally we use this only as a key for the per-THD txn registry.
pub struct OpaqueThd(pub *mut ());

unsafe impl Send for OpaqueThd {}
unsafe impl Sync for OpaqueThd {}

// XA xid serialization handle. Real (de)serialization is in `name_helpers`.
pub struct Xid(pub [u8; 144]);

// -----------------------------------------------------------------------
// Rollback-marker helper (called from many MyRocks error paths)
// -----------------------------------------------------------------------

/// Tell MariaDB this transaction must roll back at the next commit boundary.
/// We mirror the C++ inline by setting `Rdb_transaction::set_tx_failed(true)`
/// on the THD's txn and signalling MariaDB via `thd_mark_transaction_to_rollback`.
///
/// `all = true` → entire transaction; `all = false` → only the statement.
///
/// Original: ha_rocksdb.cc:94 — `thd_mark_transaction_to_rollback`.
pub fn mark_transaction_to_rollback(thd: &OpaqueThd, all: bool) {
    let _ = (thd, all);
    todo!("look up tx in registry, set_tx_failed; cxx-bridge call to MariaDB's THD")
}

// -----------------------------------------------------------------------
// Per-THD txn registry lookup
// -----------------------------------------------------------------------

/// Fetch the existing `Rdb_transaction` for this THD if one is attached.
/// Returns `None` if the connection has not yet started any txn (or has
/// just rolled back / committed and detached).
///
/// Original: ha_rocksdb.cc:3731 — `get_tx_from_thd`.
pub fn get_tx_from_thd(thd: &OpaqueThd) -> Option<Box<dyn RdbTransaction>> {
    let _ = thd;
    todo!("read per-THD slot; for cxx bridge, this is a thread-local lookup")
}

/// Fetch or create the per-THD `Rdb_transaction`. Picks
/// `IsolationLevel::Snapshot` or `SerializableSnapshot` based on the THD's
/// SQL isolation level (per _DESIGN.md §5 table).
///
/// Original: ha_rocksdb.cc:3779 — `get_or_create_tx`.
pub fn get_or_create_tx(thd: &OpaqueThd) -> Result<Box<dyn RdbTransaction>, Error> {
    let _ = thd;
    todo!("if !exists: build Rdb_transaction_impl with engine.db().begin(level); attach to THD")
}

// -----------------------------------------------------------------------
// handlerton lifecycle of a connection
// -----------------------------------------------------------------------

/// MariaDB calls this when a connection closes; we must roll back any
/// in-flight txn and detach.
///
/// Returns `slatedb::ErrorKind::Unavailable` (mapped to HA_ERR_LOCK_WAIT_TIMEOUT)
/// only if rollback I/O fails; we still proceed to detach.
///
/// Original: ha_rocksdb.cc:3804 — `rocksdb_close_connection`.
pub fn close_connection(thd: &OpaqueThd) -> Result<(), Error> {
    let _ = thd;
    todo!("rollback any open tx, detach from per-THD slot")
}

// -----------------------------------------------------------------------
// XA Two-Phase Commit
// -----------------------------------------------------------------------

/// XA prepare. `prepare_tx=true` means full-tx prepare; `false` means
/// statement-level (rare, group-replication path).
///
/// Per _DESIGN.md §1 "Two-phase commit": delegates to
/// `Rdb_transaction::prepare(xid_string)` which:
///   1. flushes the WAL (`Db::flush_with_options(FlushType::Wal)`)
///   2. writes `xa_prepare:<xid>` into the system CF as a durable marker
///
/// Original: ha_rocksdb.cc:3882 — `rocksdb_prepare`.
pub fn prepare(thd: &OpaqueThd, prepare_tx: bool) -> Result<(), Error> {
    let _ = (thd, prepare_tx);
    todo!("get_tx_from_thd; call tx.prepare(rdb_xid_to_string(thd_xid))")
}

/// XA commit-by-xid. Used during crash recovery to commit a previously-
/// prepared transaction not attached to any THD.
///
/// Original: ha_rocksdb.cc — `rocksdb_commit_by_xid` (declared in init).
pub fn commit_by_xid(xid: &Xid) -> Result<(), Error> {
    let _ = xid;
    todo!("look up the prepare marker in system CF, replay its WriteBatch, delete marker")
}

/// XA rollback-by-xid. Crash-recovery counterpart of `commit_by_xid`.
/// Original: ha_rocksdb.cc — `rocksdb_rollback_by_xid`.
pub fn rollback_by_xid(xid: &Xid) -> Result<(), Error> {
    let _ = xid;
    todo!("delete the prepare marker from system CF")
}

/// `recover()` — list all in-doubt prepared XIDs after startup.
/// Scans the system CF for `xa_prepare:*` markers and fills `xid_list`.
/// Returns count.
///
/// Original: ha_rocksdb.cc:4052 — `rocksdb_recover`.
pub fn recover(xid_list: &mut Vec<Xid>, max_len: usize) -> Result<usize, Error> {
    let _ = (xid_list, max_len);
    todo!("scan_prefix(b\"__system__/xa_prepare/\"); rdb_xid_from_string each value")
}

// -----------------------------------------------------------------------
// Optional MariaDB hooks (no-ops for SlateDB)
// -----------------------------------------------------------------------

/// Group-replication checkpoint request. Not in scope for Stage 1.
/// Original: ha_rocksdb.cc:4116 — `rocksdb_checkpoint_request`.
pub fn checkpoint_request(cookie: *mut ()) {
    let _ = cookie;
    // No-op: see "Out-of-scope methods" header.
}

/// Group-commit ordering hook. SlateDB serializes commits in its txn
/// manager already; no extra ordering needed.
/// Original: ha_rocksdb.cc:4131 — `rocksdb_commit_ordered`.
pub fn commit_ordered(thd: &OpaqueThd, all: bool) {
    let _ = (thd, all);
    // No-op
}

// -----------------------------------------------------------------------
// Commit / rollback / savepoint
// -----------------------------------------------------------------------

/// MariaDB commit callback. `commit_tx=true` → full transaction commit;
/// `false` → statement boundary (release stmt savepoint).
///
/// Per _DESIGN.md §5: on full-tx commit, calls
/// `Rdb_transaction::commit` which calls `DbTransaction::commit().await?`.
/// Returns:
///   - `Ok(())` on successful commit
///   - `Err(ErrorKind::Transaction)` on SSI conflict → mapped to HA_ERR_LOCK_DEADLOCK
///   - `Err(ErrorKind::Unavailable)` on I/O → HA_ERR_LOCK_WAIT_TIMEOUT
///
/// Original: ha_rocksdb.cc:4157 — `rocksdb_commit`.
pub fn commit(thd: &OpaqueThd, commit_tx: bool) -> Result<(), Error> {
    let _ = (thd, commit_tx);
    todo!("get_tx_from_thd; if commit_tx { tx.commit() } else { tx.make_stmt_savepoint_permanent() }")
}

/// MariaDB rollback callback. `rollback_tx=true` → full rollback;
/// `false` → rollback statement.
///
/// Original: ha_rocksdb.cc:4238 — `rocksdb_rollback`.
pub fn rollback(thd: &OpaqueThd, rollback_tx: bool) -> Result<(), Error> {
    let _ = (thd, rollback_tx);
    todo!("get_tx_from_thd; if rollback_tx { tx.rollback() } else { tx.rollback_stmt() }")
}

/// Register the current handler thread as participating in this transaction.
/// Forces MariaDB to call our `prepare`/`commit`/`rollback` at the end.
///
/// Original: ha_rocksdb.cc:4796 — `rocksdb_register_tx`.
pub fn register_tx(thd: &OpaqueThd) {
    let _ = thd;
    todo!("cxx-bridge: mark THD as having a handler transaction")
}

/// `START TRANSACTION WITH CONSISTENT SNAPSHOT` entry — pre-acquires the
/// `DbSnapshot` at statement start instead of at first read.
///
/// Original: ha_rocksdb.cc — `rocksdb_start_tx_and_assign_read_view`.
pub fn start_tx_and_assign_read_view(thd: &OpaqueThd) -> Result<(), Error> {
    let _ = thd;
    todo!("get_or_create_tx; tx.acquire_snapshot(true)")
}

// -----------------------------------------------------------------------
// Savepoints — Stage 0 stubs per _DESIGN.md §5 + Q10 ruling 2026-05-29.
//
// SlateDB has no native savepoint API and no write-batch truncate primitive.
// Per Q10 the three hooks return `HA_ERR_WRONG_COMMAND` for Stage 0; the
// engine-side savepoint stack on `RdbTransactionImpl` / `RdbWritebatchImpl`
// stays in place as latent code for the post-Stage-1 re-evaluation.
// -----------------------------------------------------------------------

/// `SAVEPOINT name` callback.
///
/// Stage 0: returns `Error::invalid("...")` which maps to
/// `HA_ERR_WRONG_COMMAND` via the standard error translation.
///
/// Original: ha_rocksdb.cc:5025 — `rocksdb_savepoint`.
pub fn savepoint(thd: &OpaqueThd, savepoint: *mut ()) -> Result<(), Error> {
    let _ = (thd, savepoint);
    Err(Error::invalid(
        "SAVEPOINT: not supported by the SlateDB engine in Stage 0 (Q10)".into(),
    ))
}

/// `ROLLBACK TO SAVEPOINT name` callback.
///
/// Stage 0: returns `Error::invalid("...")` per Q10.
///
/// Original: ha_rocksdb.cc — `rocksdb_rollback_to_savepoint`.
pub fn rollback_to_savepoint(thd: &OpaqueThd, savepoint: *mut ()) -> Result<(), Error> {
    let _ = (thd, savepoint);
    Err(Error::invalid(
        "ROLLBACK TO SAVEPOINT: not supported by the SlateDB engine in Stage 0 (Q10)".into(),
    ))
}

/// MariaDB asks whether MDL locks can be released as part of the
/// `ROLLBACK TO SAVEPOINT`. Constant `false` in MyRocks because rolling
/// back to a savepoint must keep table-level locks. Since the rollback
/// hook above is itself stubbed, this is effectively dead code in
/// Stage 0; we leave the answer at `false` so behaviour is unchanged
/// if the rollback hook is ever wired up.
///
/// Original: ha_rocksdb.cc — `rocksdb_rollback_to_savepoint_can_release_mdl`.
pub fn rollback_to_savepoint_can_release_mdl(thd: &OpaqueThd) -> bool {
    let _ = thd;
    false
}
