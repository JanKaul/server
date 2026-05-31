//! Per-THD transaction registry.
//!
//! Translates the MariaDB-side per-THD `Rdb_transaction *` slot from
//! `ha_rocksdb.cc:3731` (`get_tx_from_thd`) / `ha_rocksdb.cc:3779`
//! (`get_or_create_tx`).
//!
//! The C++ stashes the per-connection transaction inside a slot on
//! `THD`; in our Rust port we don't have direct access to THD storage
//! across the cxx boundary, so we maintain a process-global registry
//! keyed by an opaque `thd_id` (a `u64` the cxx side passes us from
//! `(uintptr_t)thd` or similar — exact derivation is the bridge's
//! concern).
//!
//! ## Lifecycle
//!
//! - `get_or_create(thd_id, db)` — idempotent; if the THD already has
//!   a transaction, returns OK without creating. Otherwise begins a
//!   new SSI transaction via [`EngineDb::begin_default`].
//! - `commit(thd_id)` — removes the txn from the registry and
//!   commits it. Returns `Ok(())` if no txn was registered (matches
//!   the C++ "no-op if no tx" semantics).
//! - `rollback(thd_id)` — removes and rolls back. Infallible (the
//!   underlying `rollback` is just dropping buffered writes).
//! - `has(thd_id)` / `len()` — diagnostic / test accessors.
//!
//! ## Isolation level
//!
//! Today every txn begins at `SerializableSnapshot` (the engine
//! default per `_DESIGN.md §11 Q9`). The C++ picks based on
//! `thd_tx_isolation` — `READ_COMMITTED`/`REPEATABLE_READ` →
//! Snapshot, `SERIALIZABLE` → SerializableSnapshot. We defer
//! sysvar / isolation plumbing until external_lock needs to surface
//! it.
//!
//! ## Concurrency model
//!
//! Each MariaDB connection (THD) is single-threaded — only one
//! statement at a time. So per-`thd_id` access is naturally
//! serialised by MariaDB. We still use a `Mutex<HashMap<...>>` for
//! cross-THD safety (different THDs can be running in parallel on
//! different threads). The map lock is held only across pointer
//! operations; the txn itself moves in/out by value.

use std::collections::HashMap;

use parking_lot::Mutex;
use slatedb::Error;

use crate::engine::db::EngineDb;
use crate::engine::txn::EngineTxn;

/// Opaque THD identifier. Passed across the cxx boundary as `u64`;
/// the C++ side derives it from `(uintptr_t)thd` or equivalent. We
/// don't dereference the value — it's just a hash key.
pub type ThdId = u64;

/// Process-global per-THD transaction registry. Wrapped in `Arc` by
/// [`crate::bridge::current_txn_registry`] so handler threads can
/// borrow it.
pub struct TxnRegistry {
    map: Mutex<HashMap<ThdId, EngineTxn>>,
}

impl TxnRegistry {
    pub fn new() -> Self {
        Self {
            map: Mutex::new(HashMap::new()),
        }
    }

    /// True iff `thd_id` currently has a registered transaction.
    pub fn has(&self, thd_id: ThdId) -> bool {
        self.map.lock().contains_key(&thd_id)
    }

    /// Number of registered transactions. Diagnostic accessor.
    pub fn len(&self) -> usize {
        self.map.lock().len()
    }

    /// Whether the registry has any transactions. Idiomatic
    /// counterpart to [`Self::len`].
    pub fn is_empty(&self) -> bool {
        self.map.lock().is_empty()
    }

    /// Get-or-create the transaction for `thd_id`. Idempotent — if
    /// the THD already has one, returns `Ok(())` without creating.
    ///
    /// Translates `get_or_create_tx` at `ha_rocksdb.cc:3779`.
    ///
    /// The check-then-create is racy in the general case (two threads
    /// passing the contains check before either inserts), but
    /// MariaDB serialises per-THD operations so the race can't happen
    /// in practice. If it ever did, the loser's freshly-created txn
    /// just gets dropped (releasing its writes) rather than installed.
    pub async fn get_or_create(
        &self,
        thd_id: ThdId,
        db: &EngineDb,
    ) -> Result<(), Error> {
        if self.has(thd_id) {
            return Ok(());
        }
        let txn = db.begin_default().await?;
        let mut map = self.map.lock();
        // entry/or_insert: if a concurrent caller raced us, the new
        // txn we just built is dropped here. Documented above.
        map.entry(thd_id).or_insert(txn);
        Ok(())
    }

    /// Remove `thd_id`'s txn from the registry and commit it. If no
    /// txn was registered, returns `Ok(())` (matches the C++
    /// no-op-if-no-tx semantics — `external_lock` calls commit
    /// unconditionally on autocommit-boundary `F_UNLCK`).
    ///
    /// Errors: `Transaction` on SSI conflict (mapped to
    /// `HA_ERR_LOCK_DEADLOCK`); `Unavailable` on I/O failure.
    pub async fn commit(&self, thd_id: ThdId) -> Result<(), Error> {
        let txn = match self.map.lock().remove(&thd_id) {
            Some(t) => t,
            None => return Ok(()),
        };
        txn.commit().await
    }

    /// Remove `thd_id`'s txn from the registry and roll it back.
    /// Infallible — the underlying rollback is just dropping the
    /// buffered writes (SlateDB guarantees this never fails). A
    /// rollback on an unregistered THD is a silent no-op.
    pub fn rollback(&self, thd_id: ThdId) {
        if let Some(txn) = self.map.lock().remove(&thd_id) {
            txn.rollback();
        }
    }

    /// Take the txn out of the registry without committing or rolling
    /// back. Useful when the caller wants to inspect / operate on the
    /// txn before deciding its fate. If no txn is registered,
    /// returns `None`.
    ///
    /// Caller is responsible for either committing/rolling-back the
    /// returned txn or putting it back via [`Self::reinsert`].
    pub fn take(&self, thd_id: ThdId) -> Option<EngineTxn> {
        self.map.lock().remove(&thd_id)
    }

    /// Reinstall a previously-taken txn at `thd_id`. If a different
    /// txn already exists at that slot it's dropped (matches the
    /// "last write wins" semantic of HashMap::insert).
    pub fn reinsert(&self, thd_id: ThdId, txn: EngineTxn) {
        self.map.lock().insert(thd_id, txn);
    }
}

impl Default for TxnRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    async fn fresh_engine() -> EngineDb {
        EngineDb::open_in_memory("txn_registry_tests")
            .await
            .expect("open")
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn new_registry_is_empty() {
        let reg = TxnRegistry::new();
        assert!(reg.is_empty());
        assert_eq!(reg.len(), 0);
        assert!(!reg.has(7));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn get_or_create_inserts_then_is_idempotent() {
        let engine = fresh_engine().await;
        let reg = TxnRegistry::new();

        reg.get_or_create(1, &engine).await.expect("create 1");
        assert!(reg.has(1));
        assert_eq!(reg.len(), 1);

        // Second call on the same thd_id is a no-op.
        reg.get_or_create(1, &engine).await.expect("idempotent");
        assert_eq!(reg.len(), 1);

        // Different thd_id gets a separate txn.
        reg.get_or_create(2, &engine).await.expect("create 2");
        assert_eq!(reg.len(), 2);

        engine.close().await.expect("close");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn commit_removes_from_registry() {
        let engine = fresh_engine().await;
        let reg = TxnRegistry::new();
        reg.get_or_create(1, &engine).await.expect("create");
        assert!(reg.has(1));

        reg.commit(1).await.expect("commit");
        assert!(!reg.has(1));
        assert_eq!(reg.len(), 0);

        engine.close().await.expect("close");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn commit_on_unknown_thd_is_a_noop() {
        let reg = TxnRegistry::new();
        // No engine needed — no txn exists, no work to do.
        reg.commit(42).await.expect("noop commit");
        assert!(reg.is_empty());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn rollback_removes_from_registry() {
        let engine = fresh_engine().await;
        let reg = TxnRegistry::new();
        reg.get_or_create(7, &engine).await.expect("create");
        reg.rollback(7);
        assert!(!reg.has(7));

        // Rollback on unknown thd_id is silent.
        reg.rollback(99);
        assert!(reg.is_empty());

        engine.close().await.expect("close");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn take_then_reinsert_round_trip() {
        let engine = fresh_engine().await;
        let reg = TxnRegistry::new();
        reg.get_or_create(3, &engine).await.expect("create");

        let txn = reg.take(3).expect("take");
        assert!(!reg.has(3), "take removes from registry");

        reg.reinsert(3, txn);
        assert!(reg.has(3));

        // Clean up so the underlying txn doesn't leak.
        reg.rollback(3);
        engine.close().await.expect("close");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn parallel_get_or_create_for_distinct_thd_ids_does_not_serialise() {
        let engine = Arc::new(fresh_engine().await);
        let reg = Arc::new(TxnRegistry::new());

        let handles: Vec<_> = (0u64..16)
            .map(|i| {
                let engine = engine.clone();
                let reg = reg.clone();
                tokio::spawn(async move {
                    reg.get_or_create(i, &engine).await.expect("create");
                })
            })
            .collect();
        for h in handles {
            h.await.expect("join");
        }
        assert_eq!(reg.len(), 16);

        // Clean up.
        for i in 0..16 {
            reg.rollback(i);
        }
        engine.close().await.expect("close");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn parallel_get_or_create_same_thd_id_keeps_exactly_one_txn() {
        // MariaDB serialises per-THD operations so this race shouldn't
        // happen in practice; the test pins our defensive behaviour.
        let engine = Arc::new(fresh_engine().await);
        let reg = Arc::new(TxnRegistry::new());

        let handles: Vec<_> = (0..8)
            .map(|_| {
                let engine = engine.clone();
                let reg = reg.clone();
                tokio::spawn(async move {
                    reg.get_or_create(42, &engine).await.expect("create");
                })
            })
            .collect();
        for h in handles {
            h.await.expect("join");
        }
        assert_eq!(reg.len(), 1, "all 8 callers see one txn");

        reg.rollback(42);
        engine.close().await.expect("close");
    }
}
