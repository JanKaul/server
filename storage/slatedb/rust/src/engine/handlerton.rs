//! `handlerton` transaction callbacks — translated from the
//! `ha_rocksdb_cc____free__txn_handlers` sub-unit.
//!
//! These are the per-process callbacks MariaDB invokes on transaction
//! boundaries (commit, rollback, connection close, savepoint). Each
//! resolves the per-THD transaction via [`crate::engine::txn_registry::
//! TxnRegistry`] keyed by `thd_id: u64` — the same identifier
//! [`crate::handler::HaSlateDb::external_lock`] uses to create the
//! txn.
//!
//! ## Scope of this commit
//!
//! - [`commit`] / [`rollback`] — delegate to registry. Statement-level
//!   (`commit_tx=false` / `rollback_tx=false`) is a no-op per Q10 (no
//!   savepoint support in Stage 0).
//! - [`close_connection`] — silent rollback of any in-flight txn.
//! - [`savepoint`] / [`rollback_to_savepoint`] — return
//!   `Err(ErrorKind::Invalid)` per Q10. The cxx wrapper maps to a
//!   `NOT_SUPPORTED` status code which the C++ side translates to
//!   `HA_ERR_WRONG_COMMAND`.
//! - [`rollback_to_savepoint_can_release_mdl`] — constant `false`,
//!   matching MyRocks.
//! - [`commit_ordered`] — no-op. SlateDB's `DbTransaction::commit`
//!   already serialises via the transaction manager.
//! - [`checkpoint_request`] — no-op. Galera/group-replication hook,
//!   out of scope for Stage 1.
//!
//! ## Deferred
//!
//! - `register_tx`, `mark_transaction_to_rollback` — need cxx
//!   callbacks **into** MariaDB (to flag the THD as having a handler
//!   txn / to request rollback at the next boundary). Land alongside
//!   the cxx callback surface.
//! - `prepare` / `commit_by_xid` / `rollback_by_xid` / `recover` — XA
//!   two-phase commit. Needs system-CF marker schema + WAL flush
//!   hook; bigger bear.
//! - `start_tx_and_assign_read_view` — `START TRANSACTION WITH
//!   CONSISTENT SNAPSHOT`. SlateDB captures the snapshot at
//!   `DbTransaction::begin` (not on first read), so the semantics
//!   collapse to plain `get_or_create`. Deferred until we have test
//!   coverage that distinguishes the two cases.

use slatedb::Error;

use crate::engine::txn_registry::TxnRegistry;

/// MariaDB `commit` callback. `commit_tx=true` → full transaction
/// commit; `commit_tx=false` → statement boundary (release the
/// per-statement savepoint).
///
/// Statement-level is a no-op in Stage 0 — we don't have savepoints
/// per Q10, so there's nothing to release. The data buffered in the
/// transaction stays in place for the next statement or the
/// eventual full commit.
///
/// Full commit delegates to [`TxnRegistry::commit`]. Errors propagate
/// — SSI conflicts come back as `ErrorKind::Transaction`
/// (mapped to `HA_ERR_LOCK_DEADLOCK`) and I/O failures as
/// `ErrorKind::Unavailable` (mapped to `HA_ERR_LOCK_WAIT_TIMEOUT`).
///
/// Translated from `rocksdb_commit` at `ha_rocksdb.cc:4157`.
pub async fn commit(
    registry: &TxnRegistry,
    thd_id: u64,
    commit_tx: bool,
) -> Result<(), Error> {
    if !commit_tx {
        // Per-statement boundary: no savepoint to release in Stage 0.
        return Ok(());
    }
    registry.commit(thd_id).await
}

/// MariaDB `rollback` callback. `rollback_tx=true` → full
/// transaction rollback; `rollback_tx=false` → statement rollback.
///
/// Statement rollback is a no-op in Stage 0 (no savepoints per Q10).
/// Without per-statement savepoints we can't selectively revert just
/// the in-flight statement's writes — they stay until the next full
/// rollback. This is a known Stage 0 limitation; tests that exercise
/// "error in the middle of a statement → revert just that statement"
/// will fail until Q10 is revisited.
///
/// Full rollback delegates to [`TxnRegistry::rollback`], which is
/// infallible per SlateDB contract (drop buffered writes).
///
/// Translated from `rocksdb_rollback` at `ha_rocksdb.cc:4238`.
pub fn rollback(
    registry: &TxnRegistry,
    thd_id: u64,
    rollback_tx: bool,
) -> Result<(), Error> {
    if !rollback_tx {
        return Ok(());
    }
    registry.rollback(thd_id);
    Ok(())
}

/// MariaDB `close_connection` callback — invoked when the client
/// disconnects. We must roll back any in-flight txn and detach.
///
/// Infallible — see [`TxnRegistry::rollback`]. A close on a THD with
/// no registered txn is a silent no-op.
///
/// Translated from `rocksdb_close_connection` at `ha_rocksdb.cc:3804`.
pub fn close_connection(registry: &TxnRegistry, thd_id: u64) -> Result<(), Error> {
    registry.rollback(thd_id);
    Ok(())
}

/// MariaDB `savepoint` callback — `SAVEPOINT name`. Stage 0 stub per
/// Q10: returns `Err(ErrorKind::Invalid)` so the bridge maps it to
/// `HA_ERR_WRONG_COMMAND` ("savepoint not supported by this engine").
///
/// Translated from `rocksdb_savepoint` at `ha_rocksdb.cc:5025`.
pub fn savepoint(_thd_id: u64) -> Result<(), Error> {
    Err(Error::invalid(
        "SAVEPOINT: not supported by the SlateDB engine in Stage 0 (Q10)".into(),
    ))
}

/// MariaDB `rollback_to_savepoint` callback. Stage 0 stub per Q10.
///
/// Translated from `rocksdb_rollback_to_savepoint`.
pub fn rollback_to_savepoint(_thd_id: u64) -> Result<(), Error> {
    Err(Error::invalid(
        "ROLLBACK TO SAVEPOINT: not supported by the SlateDB engine in Stage 0 (Q10)".into(),
    ))
}

/// MariaDB queries this to decide whether MDL locks can be released
/// as part of `ROLLBACK TO SAVEPOINT`. Constant `false` matching
/// MyRocks — rolling back to a savepoint must keep table-level
/// locks. Effectively dead code in Stage 0 since the rollback hook
/// itself errors out, but we keep the answer at `false` so behaviour
/// is preserved when the rollback hook is wired up.
///
/// Translated from `rocksdb_rollback_to_savepoint_can_release_mdl`.
pub fn rollback_to_savepoint_can_release_mdl(_thd_id: u64) -> bool {
    false
}

/// Group-commit ordering hook. MariaDB calls this before `commit`
/// to assign commit order; SlateDB's `DbTransaction::commit` already
/// serializes via the transaction manager, so this is a no-op.
///
/// Translated from `rocksdb_commit_ordered` at `ha_rocksdb.cc:4131`.
pub fn commit_ordered(_thd_id: u64, _all: bool) {
    // intentionally empty
}

/// Galera/group-replication checkpoint hook. Out of scope for
/// Stage 1 per the migration doc.
///
/// Translated from `rocksdb_checkpoint_request` at `ha_rocksdb.cc:4116`.
pub fn checkpoint_request() {
    // intentionally empty
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::db::EngineDb;
    use crate::engine::txn_registry::TxnRegistry;

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn commit_full_drains_registry_slot() {
        let db = EngineDb::open_in_memory("handlerton_commit_full")
            .await
            .expect("open");
        let reg = TxnRegistry::new();
        reg.get_or_create(7, &db).await.expect("create");
        assert!(reg.has(7));

        commit(&reg, 7, true).await.expect("commit");
        assert!(!reg.has(7));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn commit_stmt_is_noop_leaves_txn_in_place() {
        let db = EngineDb::open_in_memory("handlerton_commit_stmt")
            .await
            .expect("open");
        let reg = TxnRegistry::new();
        reg.get_or_create(8, &db).await.expect("create");

        commit(&reg, 8, false).await.expect("stmt commit");
        // Txn still registered — full commit would drain.
        assert!(reg.has(8));

        commit(&reg, 8, true).await.expect("final commit");
        assert!(!reg.has(8));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn commit_on_unknown_thd_is_ok() {
        let reg = TxnRegistry::new();
        // No txn ever registered — commit is silently OK
        // (matches the C++ no-op-if-no-tx semantic).
        commit(&reg, 99, true).await.expect("noop commit");
        assert!(!reg.has(99));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn rollback_full_drains_registry_slot() {
        let db = EngineDb::open_in_memory("handlerton_rollback_full")
            .await
            .expect("open");
        let reg = TxnRegistry::new();
        reg.get_or_create(11, &db).await.expect("create");

        rollback(&reg, 11, true).expect("rollback");
        assert!(!reg.has(11));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn rollback_stmt_is_noop_leaves_txn_in_place() {
        let db = EngineDb::open_in_memory("handlerton_rollback_stmt")
            .await
            .expect("open");
        let reg = TxnRegistry::new();
        reg.get_or_create(12, &db).await.expect("create");

        rollback(&reg, 12, false).expect("stmt rollback");
        // Stage 0: no savepoints, so statement-level rollback
        // can't revert anything. Txn stays open.
        assert!(reg.has(12));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn rollback_on_unknown_thd_is_ok() {
        let reg = TxnRegistry::new();
        rollback(&reg, 42, true).expect("noop rollback");
        assert!(!reg.has(42));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn close_connection_rolls_back_in_flight_txn() {
        let db = EngineDb::open_in_memory("handlerton_close")
            .await
            .expect("open");
        let reg = TxnRegistry::new();
        reg.get_or_create(20, &db).await.expect("create");
        reg.get_or_create(21, &db).await.expect("create");

        close_connection(&reg, 20).expect("close 20");
        assert!(!reg.has(20));
        // 21 is untouched.
        assert!(reg.has(21));
    }

    #[test]
    fn close_connection_on_unknown_thd_is_ok() {
        let reg = TxnRegistry::new();
        close_connection(&reg, 999).expect("noop close");
    }

    #[test]
    fn savepoint_returns_invalid_in_stage_0() {
        let err = savepoint(1).unwrap_err();
        assert_eq!(err.kind(), slatedb::ErrorKind::Invalid);
        assert!(err.to_string().contains("Q10"));
    }

    #[test]
    fn rollback_to_savepoint_returns_invalid_in_stage_0() {
        let err = rollback_to_savepoint(1).unwrap_err();
        assert_eq!(err.kind(), slatedb::ErrorKind::Invalid);
        assert!(err.to_string().contains("Q10"));
    }

    #[test]
    fn rollback_to_savepoint_can_release_mdl_is_false() {
        // Constant — match MyRocks behaviour for forward compat when
        // savepoints are wired up.
        assert!(!rollback_to_savepoint_can_release_mdl(0));
        assert!(!rollback_to_savepoint_can_release_mdl(u64::MAX));
    }

    #[test]
    fn commit_ordered_is_noop_no_panic() {
        commit_ordered(0, false);
        commit_ordered(u64::MAX, true);
    }

    #[test]
    fn checkpoint_request_is_noop_no_panic() {
        checkpoint_request();
    }
}
